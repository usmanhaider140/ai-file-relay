//! One-time links for file bytes.
//!
//! When an upload is approved, the hub makes a token for that one file.
//! The extension uses it once to download the bytes from the hub. A token
//! stops working after it is used or when it expires.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::random_hex;

/// Default life of a token.
pub const DEFAULT_TTL: Duration = Duration::from_secs(60);

#[derive(Debug)]
struct Grant {
    path: PathBuf,
    expires_at: Instant,
}

/// Keeps the tokens that are still valid.
#[derive(Debug, Default)]
pub struct TokenStore {
    grants: Mutex<HashMap<String, Grant>>,
}

impl TokenStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Make a token that gives access to `path` once, for `ttl`.
    pub fn issue(&self, path: impl AsRef<Path>, ttl: Duration) -> String {
        let token = random_hex(32);
        let mut grants = self.lock();
        Self::purge_expired(&mut grants);
        grants.insert(
            token.clone(),
            Grant {
                path: path.as_ref().to_path_buf(),
                expires_at: Instant::now() + ttl,
            },
        );
        token
    }

    /// Use a token. Returns the file path the first time only, and only
    /// before it expires.
    pub fn redeem(&self, token: &str) -> Option<PathBuf> {
        let mut grants = self.lock();
        let grant = grants.remove(token)?;
        (Instant::now() < grant.expires_at).then_some(grant.path)
    }

    /// Cancel a token before it is used. Returns true if it existed.
    pub fn revoke(&self, token: &str) -> bool {
        self.lock().remove(token).is_some()
    }

    /// Number of tokens that are still waiting to be used.
    pub fn pending(&self) -> usize {
        let mut grants = self.lock();
        Self::purge_expired(&mut grants);
        grants.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Grant>> {
        // A poisoned lock only means another thread panicked while holding
        // it; the map itself is still fine to use.
        self.grants.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn purge_expired(grants: &mut HashMap<String, Grant>) {
        let now = Instant::now();
        grants.retain(|_, g| g.expires_at > now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_works_once() {
        let store = TokenStore::new();
        let token = store.issue("/tmp/a.txt", DEFAULT_TTL);
        assert_eq!(store.redeem(&token), Some(PathBuf::from("/tmp/a.txt")));
        assert_eq!(store.redeem(&token), None);
    }

    #[test]
    fn revoked_token_fails() {
        let store = TokenStore::new();
        let token = store.issue("/tmp/a.txt", DEFAULT_TTL);
        assert!(store.revoke(&token));
        assert_eq!(store.redeem(&token), None);
        assert!(!store.revoke(&token));
    }

    #[test]
    fn unknown_token_fails() {
        let store = TokenStore::new();
        assert_eq!(store.redeem("nope"), None);
    }

    #[test]
    fn expired_token_fails() {
        let store = TokenStore::new();
        let token = store.issue("/tmp/a.txt", Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(store.redeem(&token), None);
    }

    #[test]
    fn expired_tokens_are_cleaned_up() {
        let store = TokenStore::new();
        store.issue("/tmp/a.txt", Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(20));
        store.issue("/tmp/b.txt", DEFAULT_TTL);
        assert_eq!(store.pending(), 1);
    }
}
