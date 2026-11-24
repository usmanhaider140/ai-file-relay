//! Approvals: who says yes or no to a transfer.
//!
//! The desktop app shows a popup and lets the user decide. Without the
//! desktop app, [`PolicyApprover`] decides from the `approval` setting.
//!
//! Deciding can take a while (a person has to click), so
//! [`Approver::decide`] returns a future.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

/// What the AI wants to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Put a local file into a web page.
    Upload,
    /// Hand a downloaded file to the AI.
    Download,
}

/// A request waiting for a yes or no.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub action: Action,
    /// The local file (for uploads) or the folder a download goes to.
    pub path: PathBuf,
    /// The web site involved, when known (for example `example.com`).
    pub site: Option<String>,
    /// File size in bytes, for uploads.
    pub size: Option<u64>,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(String),
}

/// A boxed future, so [`Approver`] can be used as `dyn Approver`.
pub type DecisionFuture<'a> = Pin<Box<dyn Future<Output = Decision> + Send + 'a>>;

/// Anything that can decide on a transfer.
pub trait Approver: Send + Sync {
    fn decide<'a>(&'a self, request: &'a ApprovalRequest) -> DecisionFuture<'a>;

    /// The mode in force right now, for status displays.
    fn mode(&self) -> ApprovalMode;
}

impl<T: Approver + ?Sized> Approver for std::sync::Arc<T> {
    fn decide<'a>(&'a self, request: &'a ApprovalRequest) -> DecisionFuture<'a> {
        (**self).decide(request)
    }

    fn mode(&self) -> ApprovalMode {
        (**self).mode()
    }
}

/// How transfers are approved, from the settings file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    /// Ask the user each time (needs the desktop app).
    #[default]
    Ask,
    /// Allow every transfer inside the allowed folders without asking.
    /// Only for testing on your own machine.
    AllowAll,
    /// Refuse every transfer.
    DenyAll,
}

/// Decides from an [`ApprovalMode`] alone.
#[derive(Debug, Clone, Copy)]
pub struct PolicyApprover {
    pub mode: ApprovalMode,
}

impl PolicyApprover {
    /// The decision, without waiting.
    pub fn decide_now(&self) -> Decision {
        match self.mode {
            ApprovalMode::AllowAll => Decision::Allow,
            ApprovalMode::DenyAll => {
                Decision::Deny("transfers are turned off (approval = \"deny_all\")".into())
            }
            ApprovalMode::Ask => Decision::Deny(
                "this transfer needs your approval, but the AI File Relay desktop app is not \
                 running. Open the desktop app and try again."
                    .into(),
            ),
        }
    }
}

impl Approver for PolicyApprover {
    fn decide<'a>(&'a self, _request: &'a ApprovalRequest) -> DecisionFuture<'a> {
        let decision = self.decide_now();
        Box::pin(async move { decision })
    }

    fn mode(&self) -> ApprovalMode {
        self.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> ApprovalRequest {
        ApprovalRequest {
            action: Action::Upload,
            path: PathBuf::from("/tmp/a.txt"),
            site: Some("example.com".into()),
            size: Some(10),
        }
    }

    #[test]
    fn default_mode_is_ask() {
        assert_eq!(ApprovalMode::default(), ApprovalMode::Ask);
    }

    #[test]
    fn ask_denies_without_the_desktop_app() {
        let a = PolicyApprover {
            mode: ApprovalMode::Ask,
        };
        assert!(matches!(a.decide_now(), Decision::Deny(_)));
    }

    #[test]
    fn allow_all_allows() {
        let a = PolicyApprover {
            mode: ApprovalMode::AllowAll,
        };
        assert_eq!(a.decide_now(), Decision::Allow);
    }

    #[test]
    fn trait_returns_the_same_answer() {
        let a = PolicyApprover {
            mode: ApprovalMode::AllowAll,
        };
        let request = req();
        let fut = Approver::decide(&a, &request);
        // A ready future: poll it once with a no-op waker.
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        let mut fut = fut;
        assert_eq!(
            fut.as_mut().poll(&mut cx),
            std::task::Poll::Ready(Decision::Allow)
        );
    }

    #[test]
    fn deny_all_denies() {
        let a = PolicyApprover {
            mode: ApprovalMode::DenyAll,
        };
        assert!(matches!(a.decide_now(), Decision::Deny(_)));
    }
}
