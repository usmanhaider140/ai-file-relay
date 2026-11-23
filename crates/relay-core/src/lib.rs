//! Transfer rules for AI File Relay.
//!
//! M0 has the first safety rule: an AI may only reach files inside folders
//! the user allowed. Approvals, one-time tokens and history come in M1.

pub mod folders;

pub use folders::{AllowedFolders, PathError};
