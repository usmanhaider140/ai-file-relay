//! Transfer rules for AI File Relay.
//!
//! - [`folders`]: which folders an AI may reach, and safe path checks
//! - [`tokens`]: one-time links for file bytes
//! - [`approvals`]: who says yes or no to a transfer
//! - [`history`]: a local log of every transfer
//! - [`config`]: the settings file

pub mod approvals;
pub mod config;
pub mod folders;
pub mod history;
pub mod tokens;

pub use approvals::{
    Action, ApprovalMode, ApprovalRequest, Approver, Decision, DecisionFuture, PolicyApprover,
};
pub use config::{Config, ConfigError};
pub use folders::{AllowedFolders, Entry, EntryKind, PathError};
pub use history::{Event, History, Outcome};
pub use tokens::TokenStore;

/// Make a random hex string with `bytes` random bytes (so 2 × `bytes` characters).
pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("the operating system's random source failed");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn random_hex_has_right_length_and_differs() {
        let a = super::random_hex(32);
        let b = super::random_hex(32);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
