//! A local log of every transfer.
//!
//! Events are kept in memory (the most recent ones) and, when a file path is
//! set, appended to a JSON Lines file. Nothing ever leaves the computer.

use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::approvals::Action;

/// How many events stay in memory.
pub const KEEP_IN_MEMORY: usize = 200;

/// How a transfer ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "result", content = "reason")]
pub enum Outcome {
    Done,
    Denied(String),
    Failed(String),
}

/// One transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// Milliseconds since 1970-01-01 UTC.
    pub time_ms: u64,
    pub action: Action,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
    #[serde(flatten)]
    pub outcome: Outcome,
}

impl Event {
    pub fn now(
        action: Action,
        path: impl AsRef<Path>,
        site: Option<String>,
        outcome: Outcome,
    ) -> Self {
        let time_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self {
            time_ms,
            action,
            path: path.as_ref().to_path_buf(),
            site,
            outcome,
        }
    }
}

/// The transfer log.
#[derive(Debug, Default)]
pub struct History {
    file: Option<PathBuf>,
    recent: Mutex<VecDeque<Event>>,
}

impl History {
    /// Keep events in memory only.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Keep events in memory and append them to `file`.
    pub fn with_file(file: impl Into<PathBuf>) -> Self {
        Self {
            file: Some(file.into()),
            recent: Mutex::default(),
        }
    }

    /// Record an event. Writing to the file is best effort: a failure is
    /// returned but the event is still kept in memory.
    pub fn record(&self, event: Event) -> std::io::Result<()> {
        let line = serde_json::to_string(&event).map_err(std::io::Error::other)?;
        {
            let mut recent = self.recent.lock().unwrap_or_else(|e| e.into_inner());
            if recent.len() == KEEP_IN_MEMORY {
                recent.pop_front();
            }
            recent.push_back(event);
        }
        if let Some(file) = &self.file {
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut f = OpenOptions::new().create(true).append(true).open(file)?;
            writeln!(f, "{line}")?;
        }
        Ok(())
    }

    /// The most recent events, newest first.
    pub fn recent(&self, limit: usize) -> Vec<Event> {
        let recent = self.recent.lock().unwrap_or_else(|e| e.into_inner());
        recent.iter().rev().take(limit).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(n: u64) -> Event {
        let mut e = Event::now(Action::Upload, format!("/tmp/{n}.txt"), None, Outcome::Done);
        e.time_ms = n;
        e
    }

    #[test]
    fn newest_first_and_capped() {
        let h = History::in_memory();
        for n in 0..(KEEP_IN_MEMORY as u64 + 5) {
            h.record(event(n)).unwrap();
        }
        let recent = h.recent(usize::MAX);
        assert_eq!(recent.len(), KEEP_IN_MEMORY);
        assert_eq!(recent[0].time_ms, KEEP_IN_MEMORY as u64 + 4);
    }

    #[test]
    fn writes_json_lines() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("logs/history.jsonl");
        let h = History::with_file(&file);
        h.record(event(1)).unwrap();
        h.record(Event::now(
            Action::Download,
            "/tmp/x",
            Some("example.com".into()),
            Outcome::Denied("no".into()),
        ))
        .unwrap();
        let text = std::fs::read_to_string(file).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains(r#""result":"done""#));
        assert!(lines[1].contains(r#""result":"denied""#));
        assert!(lines[1].contains(r#""reason":"no""#));
        let back: Event = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(back.site.as_deref(), Some("example.com"));
    }
}
