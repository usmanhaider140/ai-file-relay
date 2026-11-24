//! One-click "Connect" for AI apps: adds AI File Relay to their MCP
//! settings file.
//!
//! The change is small and safe: it reads the app's JSON settings, sets
//! `mcpServers["ai-file-relay"]`, keeps everything else, and saves a backup
//! of the original file the first time.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value, json};

/// Name of our entry in the apps' `mcpServers` list.
pub const SERVER_NAME: &str = "ai-file-relay";

/// AI apps we can connect with one click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiApp {
    ClaudeDesktop,
    Cursor,
}

impl AiApp {
    pub const ALL: [AiApp; 2] = [AiApp::ClaudeDesktop, AiApp::Cursor];

    pub fn name(self) -> &'static str {
        match self {
            AiApp::ClaudeDesktop => "Claude Desktop",
            AiApp::Cursor => "Cursor",
        }
    }

    /// Where the app keeps its MCP settings.
    pub fn config_file(self) -> Option<PathBuf> {
        let base = directories::BaseDirs::new()?;
        Some(match self {
            AiApp::ClaudeDesktop => {
                // macOS: ~/Library/Application Support/Claude
                // Windows: %APPDATA%\Claude    Linux: ~/.config/Claude
                base.config_dir()
                    .join("Claude")
                    .join("claude_desktop_config.json")
            }
            AiApp::Cursor => base.home_dir().join(".cursor").join("mcp.json"),
        })
    }

    /// Does the app look installed? (Its settings folder exists.)
    pub fn looks_installed(self) -> bool {
        self.config_file()
            .and_then(|f| f.parent().map(Path::exists))
            .unwrap_or(false)
    }
}

/// Is AI File Relay in this settings file?
pub fn is_connected(file: &Path) -> bool {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .is_some_and(|v| {
            v.get("mcpServers")
                .and_then(|s| s.get(SERVER_NAME))
                .is_some()
        })
}

/// Add (or update) AI File Relay in an app's MCP settings file.
pub fn connect(file: &Path, relay_mcp: &Path) -> Result<(), String> {
    let mut root: Value = match std::fs::read_to_string(file) {
        Ok(text) if text.trim().is_empty() => json!({}),
        Ok(text) => serde_json::from_str(&text).map_err(|e| {
            format!(
                "{} is not valid JSON ({e}). Fix or remove it, then try again.",
                file.display()
            )
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(format!("cannot read {}: {e}", file.display())),
    };
    let Some(obj) = root.as_object_mut() else {
        return Err(format!("{} does not hold a JSON object", file.display()));
    };
    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(servers) = servers.as_object_mut() else {
        return Err(format!(
            "\"mcpServers\" in {} is not an object",
            file.display()
        ));
    };
    servers.insert(
        SERVER_NAME.into(),
        json!({ "command": relay_mcp.display().to_string() }),
    );

    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    // Keep the user's original file once, in case they want it back.
    let backup = file.with_extension("json.before-ai-file-relay");
    if file.exists() && !backup.exists() {
        std::fs::copy(file, &backup)
            .map_err(|e| format!("cannot back up {}: {e}", file.display()))?;
    }
    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    std::fs::write(file, text + "\n").map_err(|e| format!("cannot write {}: {e}", file.display()))
}

/// Remove AI File Relay from an app's MCP settings file.
pub fn disconnect(file: &Path) -> Result<(), String> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Ok(());
    };
    let mut root: Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} is not valid JSON ({e})", file.display()))?;
    let removed = root
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .and_then(|s| s.remove(SERVER_NAME))
        .is_some();
    if removed {
        let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
        std::fs::write(file, text + "\n")
            .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    }
    Ok(())
}

/// The command for Claude Code users to paste.
pub fn claude_code_command(relay_mcp: &Path) -> String {
    format!(
        "claude mcp add --scope user {SERVER_NAME} -- \"{}\"",
        relay_mcp.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_keeps_other_settings_and_backs_up() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("claude_desktop_config.json");
        std::fs::write(
            &file,
            r#"{"theme":"dark","mcpServers":{"other":{"command":"x"}}}"#,
        )
        .unwrap();
        assert!(!is_connected(&file));

        connect(&file, Path::new("/apps/relay-mcp")).unwrap();
        assert!(is_connected(&file));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["mcpServers"][SERVER_NAME]["command"], "/apps/relay-mcp");

        let backup = dir
            .path()
            .join("claude_desktop_config.json.before-ai-file-relay");
        assert!(
            std::fs::read_to_string(&backup)
                .unwrap()
                .contains("\"other\"")
        );

        disconnect(&file).unwrap();
        assert!(!is_connected(&file));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
    }

    #[test]
    fn connect_creates_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".cursor/mcp.json");
        connect(&file, Path::new("/bin/relay-mcp")).unwrap();
        assert!(is_connected(&file));
    }

    #[test]
    fn broken_json_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mcp.json");
        std::fs::write(&file, "{ not json").unwrap();
        let err = connect(&file, Path::new("/bin/relay-mcp")).unwrap_err();
        assert!(err.contains("not valid JSON"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json");
    }

    #[test]
    fn claude_code_command_quotes_the_path() {
        let cmd = claude_code_command(Path::new("/Applications/AI File Relay.app/relay-mcp"));
        assert!(cmd.ends_with("-- \"/Applications/AI File Relay.app/relay-mcp\""));
    }
}
