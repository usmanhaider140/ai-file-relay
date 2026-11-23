//! Approvals: who says yes or no to a transfer.
//!
//! The desktop app (M3) will show a popup and let the user decide. Until
//! then, [`PolicyApprover`] decides from the `approval` setting.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What the AI wants to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(String),
}

/// Anything that can decide on a transfer.
pub trait Approver: Send + Sync {
    fn decide(&self, request: &ApprovalRequest) -> Decision;
}

/// How transfers are approved, from the settings file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    /// Ask the user each time (needs the desktop app, coming in M3).
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

impl Approver for PolicyApprover {
    fn decide(&self, _request: &ApprovalRequest) -> Decision {
        match self.mode {
            ApprovalMode::AllowAll => Decision::Allow,
            ApprovalMode::DenyAll => {
                Decision::Deny("transfers are turned off (approval = \"deny_all\")".into())
            }
            ApprovalMode::Ask => Decision::Deny(
                "this transfer needs your approval, but the approval window comes with the \
                 desktop app (M3). To test now, set approval = \"allow_all\" in the config file."
                    .into(),
            ),
        }
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
        }
    }

    #[test]
    fn default_mode_is_ask() {
        assert_eq!(ApprovalMode::default(), ApprovalMode::Ask);
    }

    #[test]
    fn ask_denies_until_the_desktop_app_exists() {
        let a = PolicyApprover {
            mode: ApprovalMode::Ask,
        };
        assert!(matches!(a.decide(&req()), Decision::Deny(_)));
    }

    #[test]
    fn allow_all_allows() {
        let a = PolicyApprover {
            mode: ApprovalMode::AllowAll,
        };
        assert_eq!(a.decide(&req()), Decision::Allow);
    }

    #[test]
    fn deny_all_denies() {
        let a = PolicyApprover {
            mode: ApprovalMode::DenyAll,
        };
        assert!(matches!(a.decide(&req()), Decision::Deny(_)));
    }
}
