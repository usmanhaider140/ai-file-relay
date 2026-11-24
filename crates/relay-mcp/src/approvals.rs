//! Approvals that a person answers.
//!
//! [`ApprovalQueue`] is the [`Approver`] the desktop app uses. When the
//! mode is `ask`, each request waits in the queue until someone answers it
//! (or it times out). The app subscribes to the queue to show a popup and
//! calls [`ApprovalQueue::answer`] when the user clicks.
//!
//! "Allow this site for 1 hour" is remembered here too, per site and per
//! kind of transfer.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use relay_core::{Action, ApprovalMode, ApprovalRequest, Approver, Decision, DecisionFuture};
use serde::Serialize;
use tokio::sync::{broadcast, oneshot};

/// How long a request waits for an answer before it is refused.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// How long "allow this site" lasts.
pub const SITE_GRANT: Duration = Duration::from_secs(60 * 60);

/// A request waiting for the user, as the app shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingApproval {
    pub id: String,
    pub action: Action,
    pub path: PathBuf,
    pub file_name: String,
    pub site: Option<String>,
    pub size: Option<u64>,
    /// Milliseconds since 1970-01-01 UTC.
    pub created_ms: u64,
    /// When it will be refused if nobody answers, in the same clock.
    pub expires_ms: u64,
}

/// What the user clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Allow,
    /// Allow, and allow the same kind of transfer with this site for an hour.
    AllowSite,
    Deny,
}

/// Something changed in the queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QueueEvent {
    Added { approval: PendingApproval },
    Resolved { id: String },
}

struct Waiting {
    approval: PendingApproval,
    tx: oneshot::Sender<Answer>,
}

/// The approval queue. Share it with `Arc`.
pub struct ApprovalQueue {
    mode: RwLock<ApprovalMode>,
    timeout: Duration,
    waiting: Mutex<HashMap<String, Waiting>>,
    site_grants: Mutex<HashMap<(String, Action), Instant>>,
    events: broadcast::Sender<QueueEvent>,
}

impl std::fmt::Debug for ApprovalQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalQueue")
            .field("mode", &self.mode())
            .field("waiting", &self.pending().len())
            .finish()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

impl ApprovalQueue {
    pub fn new(mode: ApprovalMode, timeout: Duration) -> Self {
        let (events, _) = broadcast::channel(64);
        Self {
            mode: RwLock::new(mode),
            timeout,
            waiting: Mutex::default(),
            site_grants: Mutex::default(),
            events,
        }
    }

    pub fn mode(&self) -> ApprovalMode {
        *self.mode.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Change the mode while running (from the app's settings).
    pub fn set_mode(&self, mode: ApprovalMode) {
        *self.mode.write().unwrap_or_else(|e| e.into_inner()) = mode;
        if mode != ApprovalMode::Ask {
            // Settle anything still waiting with the new rule.
            let answer = if mode == ApprovalMode::AllowAll {
                Answer::Allow
            } else {
                Answer::Deny
            };
            let ids: Vec<String> = self.pending().into_iter().map(|p| p.id).collect();
            for id in ids {
                self.answer(&id, answer);
            }
        }
    }

    /// Get told when requests arrive or are settled.
    pub fn subscribe(&self) -> broadcast::Receiver<QueueEvent> {
        self.events.subscribe()
    }

    /// Requests waiting for an answer, oldest first.
    pub fn pending(&self) -> Vec<PendingApproval> {
        let waiting = self.waiting.lock().unwrap_or_else(|e| e.into_inner());
        let mut list: Vec<PendingApproval> = waiting.values().map(|w| w.approval.clone()).collect();
        list.sort_by_key(|p| p.created_ms);
        list
    }

    /// Answer a request. Returns false if it is no longer waiting.
    pub fn answer(&self, id: &str, answer: Answer) -> bool {
        let waiting = self
            .waiting
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        let Some(w) = waiting else { return false };
        if answer == Answer::AllowSite
            && let Some(site) = &w.approval.site
        {
            self.site_grants
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(
                    (site.clone(), w.approval.action),
                    Instant::now() + SITE_GRANT,
                );
        }
        let _ = w.tx.send(answer);
        let _ = self.events.send(QueueEvent::Resolved { id: id.into() });
        true
    }

    /// Sites with a live "allow for an hour", and when each ends (ms).
    pub fn site_grants(&self) -> Vec<(String, Action, u64)> {
        let now = Instant::now();
        let wall = now_ms();
        let mut grants = self.site_grants.lock().unwrap_or_else(|e| e.into_inner());
        grants.retain(|_, until| *until > now);
        grants
            .iter()
            .map(|((site, action), until)| {
                let left = u64::try_from((*until - now).as_millis()).unwrap_or(0);
                (site.clone(), *action, wall + left)
            })
            .collect()
    }

    /// Forget all "allow this site" grants.
    pub fn clear_site_grants(&self) {
        self.site_grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    fn has_site_grant(&self, site: &str, action: Action) -> bool {
        let mut grants = self.site_grants.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        grants.retain(|_, until| *until > now);
        grants.contains_key(&(site.to_owned(), action))
    }

    async fn ask(&self, request: &ApprovalRequest) -> Decision {
        match self.mode() {
            ApprovalMode::AllowAll => return Decision::Allow,
            ApprovalMode::DenyAll => {
                return Decision::Deny("transfers are turned off in AI File Relay".into());
            }
            ApprovalMode::Ask => {}
        }
        if let Some(site) = &request.site
            && self.has_site_grant(site, request.action)
        {
            return Decision::Allow;
        }

        let id = relay_core::random_hex(8);
        let created_ms = now_ms();
        let approval = PendingApproval {
            id: id.clone(),
            action: request.action,
            path: request.path.clone(),
            file_name: request
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            site: request.site.clone(),
            size: request.size,
            created_ms,
            expires_ms: created_ms + u64::try_from(self.timeout.as_millis()).unwrap_or(0),
        };
        let (tx, rx) = oneshot::channel();
        self.waiting
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id.clone(),
                Waiting {
                    approval: approval.clone(),
                    tx,
                },
            );
        let _ = self.events.send(QueueEvent::Added { approval });

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(Answer::Allow | Answer::AllowSite)) => Decision::Allow,
            Ok(Ok(Answer::Deny)) => Decision::Deny("the user said no".into()),
            Ok(Err(_)) => Decision::Deny("the approval was cancelled".into()),
            Err(_) => {
                self.waiting
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
                let _ = self.events.send(QueueEvent::Resolved { id });
                Decision::Deny(format!(
                    "nobody answered the approval popup within {} seconds",
                    self.timeout.as_secs()
                ))
            }
        }
    }
}

impl Approver for ApprovalQueue {
    fn decide<'a>(&'a self, request: &'a ApprovalRequest) -> DecisionFuture<'a> {
        Box::pin(self.ask(request))
    }

    fn mode(&self) -> ApprovalMode {
        ApprovalQueue::mode(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn upload(site: &str) -> ApprovalRequest {
        ApprovalRequest {
            action: Action::Upload,
            path: PathBuf::from("/docs/cv.pdf"),
            site: Some(site.into()),
            size: Some(1234),
        }
    }

    /// Start a request in the background and wait until it is queued.
    async fn start(
        q: &Arc<ApprovalQueue>,
        req: ApprovalRequest,
    ) -> (String, tokio::task::JoinHandle<Decision>) {
        let mut events = q.subscribe();
        let q2 = q.clone();
        let handle = tokio::spawn(async move { q2.decide(&req).await });
        match events.recv().await.unwrap() {
            QueueEvent::Added { approval } => (approval.id, handle),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn allow_and_deny() {
        let q = Arc::new(ApprovalQueue::new(ApprovalMode::Ask, DEFAULT_TIMEOUT));
        let (id, h) = start(&q, upload("a.com")).await;
        let p = q.pending();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].file_name, "cv.pdf");
        assert_eq!(p[0].size, Some(1234));
        assert!(q.answer(&id, Answer::Allow));
        assert_eq!(h.await.unwrap(), Decision::Allow);
        assert!(q.pending().is_empty());
        assert!(!q.answer(&id, Answer::Allow), "already answered");

        let (id, h) = start(&q, upload("a.com")).await;
        q.answer(&id, Answer::Deny);
        assert!(matches!(h.await.unwrap(), Decision::Deny(_)));
    }

    #[tokio::test]
    async fn allow_site_skips_the_next_popup_for_that_site_only() {
        let q = Arc::new(ApprovalQueue::new(ApprovalMode::Ask, DEFAULT_TIMEOUT));
        let (id, h) = start(&q, upload("jobs.com")).await;
        q.answer(&id, Answer::AllowSite);
        assert_eq!(h.await.unwrap(), Decision::Allow);

        // Same site, same action: allowed at once.
        assert_eq!(q.decide(&upload("jobs.com")).await, Decision::Allow);
        assert_eq!(q.site_grants().len(), 1);

        // Another site still asks.
        let (id, h) = start(&q, upload("other.com")).await;
        q.answer(&id, Answer::Deny);
        assert!(matches!(h.await.unwrap(), Decision::Deny(_)));

        // Downloads from the granted site still ask.
        let mut dl = upload("jobs.com");
        dl.action = Action::Download;
        let (id, h) = start(&q, dl).await;
        q.answer(&id, Answer::Allow);
        assert_eq!(h.await.unwrap(), Decision::Allow);

        q.clear_site_grants();
        assert!(q.site_grants().is_empty());
    }

    #[tokio::test]
    async fn no_answer_times_out() {
        let q = Arc::new(ApprovalQueue::new(
            ApprovalMode::Ask,
            Duration::from_millis(50),
        ));
        let mut events = q.subscribe();
        let d = q.decide(&upload("a.com")).await;
        assert!(matches!(d, Decision::Deny(ref why) if why.contains("nobody answered")));
        assert!(q.pending().is_empty());
        assert!(matches!(
            events.recv().await.unwrap(),
            QueueEvent::Added { .. }
        ));
        assert!(matches!(
            events.recv().await.unwrap(),
            QueueEvent::Resolved { .. }
        ));
    }

    #[tokio::test]
    async fn modes_skip_the_queue() {
        let q = ApprovalQueue::new(ApprovalMode::AllowAll, DEFAULT_TIMEOUT);
        assert_eq!(q.decide(&upload("a.com")).await, Decision::Allow);
        q.set_mode(ApprovalMode::DenyAll);
        assert!(matches!(
            q.decide(&upload("a.com")).await,
            Decision::Deny(_)
        ));
    }

    #[tokio::test]
    async fn switching_mode_settles_waiting_requests() {
        let q = Arc::new(ApprovalQueue::new(ApprovalMode::Ask, DEFAULT_TIMEOUT));
        let (_id, h) = start(&q, upload("a.com")).await;
        q.set_mode(ApprovalMode::DenyAll);
        assert!(matches!(h.await.unwrap(), Decision::Deny(_)));
        assert!(q.pending().is_empty());
    }
}
