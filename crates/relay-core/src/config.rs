//! The settings file.
//!
//! It lives in the normal config folder for each system, for example
//! `~/Library/Application Support/io.github.usmanhaider140.ai-file-relay/config.toml`
//! on macOS. It holds a secret token, so on macOS and Linux it is only
//! readable by the user.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::approvals::ApprovalMode;
use crate::folders::{AllowedFolders, PathError};
use crate::random_hex;

/// Lowest and highest port picked at random for the hub.
const PORT_RANGE: (u16, u16) = (40_000, 60_000);

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not find a config folder for this system")]
    NoConfigDir,
    #[error("could not read or write {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("config file {path} is not valid: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("could not write config: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error(transparent)]
    Folder(#[from] PathError),
}

/// Everything in `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Folders the AI may reach.
    pub allowed_folders: Vec<PathBuf>,
    /// Port for the hub on 127.0.0.1.
    pub port: u16,
    /// Secret that every HTTP MCP request must send as a bearer token.
    pub token: String,
    /// How transfers are approved.
    pub approval: ApprovalMode,
    /// Largest file that can be uploaded, in megabytes.
    pub max_file_mb: u64,
    /// Where finished downloads are copied for the AI.
    pub inbox_folder: Option<PathBuf>,
    /// Where the transfer log is written. Empty means memory only.
    pub history_file: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            allowed_folders: Vec::new(),
            port: 0,
            token: String::new(),
            approval: ApprovalMode::Ask,
            max_file_mb: 100,
            inbox_folder: None,
            history_file: None,
        }
    }
}

impl Config {
    /// A new config with a fresh random token and port, and default
    /// folders for the inbox and the history log.
    pub fn generate() -> Self {
        let dirs = project_dirs();
        Self {
            port: random_port(),
            token: random_hex(32),
            inbox_folder: dirs.as_ref().map(|d| d.data_dir().join("inbox")),
            history_file: dirs.as_ref().map(|d| d.data_dir().join("history.jsonl")),
            ..Self::default()
        }
    }

    /// The normal location of `config.toml` on this system.
    pub fn default_path() -> Result<PathBuf, ConfigError> {
        project_dirs()
            .map(|d| d.config_dir().join("config.toml"))
            .ok_or(ConfigError::NoConfigDir)
    }

    /// Read the config file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Read the config file, or create one with fresh secrets if it does
    /// not exist. Returns the config and whether it was just created.
    pub fn load_or_create(path: impl AsRef<Path>) -> Result<(Self, bool), ConfigError> {
        let path = path.as_ref();
        if path.exists() {
            let mut config = Self::load(path)?;
            // Fill in secrets a hand-written file may be missing.
            if config.token.is_empty() || config.port == 0 {
                let fresh = Self::generate();
                if config.token.is_empty() {
                    config.token = fresh.token;
                }
                if config.port == 0 {
                    config.port = fresh.port;
                }
                config.save(path)?;
            }
            Ok((config, false))
        } else {
            let config = Self::generate();
            config.save(path)?;
            Ok((config, true))
        }
    }

    /// Write the config file, readable only by this user where possible.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        let path = path.as_ref();
        let io_err = |source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io_err)?;
        }
        let text = format!(
            "# AI File Relay settings. Keep this file private: it holds a secret token.\n{}",
            self.to_toml()?
        );
        std::fs::write(path, text).map_err(io_err)?;
        restrict_permissions(path).map_err(io_err)?;
        Ok(())
    }

    /// The settings as TOML text.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Build the allowed folder list. Folders that no longer exist are
    /// skipped and returned so the caller can warn about them.
    pub fn allowed(&self) -> (AllowedFolders, Vec<PathBuf>) {
        let mut folders = AllowedFolders::new();
        let mut missing = Vec::new();
        for f in &self.allowed_folders {
            if folders.add(f).is_err() {
                missing.push(f.clone());
            }
        }
        (folders, missing)
    }

    /// Add a folder to the list (resolved to its real path).
    pub fn allow(&mut self, folder: impl AsRef<Path>) -> Result<PathBuf, ConfigError> {
        let mut check = AllowedFolders::new();
        check.add(&folder)?;
        let real = check.roots()[0].clone();
        if !self.allowed_folders.contains(&real) {
            self.allowed_folders.push(real.clone());
        }
        Ok(real)
    }

    /// Remove a folder from the list. Returns true if it was there.
    pub fn disallow(&mut self, folder: impl AsRef<Path>) -> bool {
        let folder = folder.as_ref();
        let real = folder
            .canonicalize()
            .unwrap_or_else(|_| folder.to_path_buf());
        let before = self.allowed_folders.len();
        self.allowed_folders.retain(|f| f != &real && f != folder);
        self.allowed_folders.len() != before
    }

    /// Largest upload in bytes.
    pub fn max_file_bytes(&self) -> u64 {
        self.max_file_mb.saturating_mul(1024 * 1024)
    }
}

fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("io.github", "usmanhaider140", "ai-file-relay")
}

fn random_port() -> u16 {
    let span = u32::from(PORT_RANGE.1 - PORT_RANGE.0);
    let mut buf = [0u8; 4];
    getrandom::fill(&mut buf).expect("the operating system's random source failed");
    // Safe: the result is below `span`, which fits in u16.
    PORT_RANGE.0 + (u32::from_le_bytes(buf) % span) as u16
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> std::io::Result<()> {
    // On Windows the file already lives in the user's own profile folder.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_then_load_keeps_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/config.toml");
        let (created, was_new) = Config::load_or_create(&path).unwrap();
        assert!(was_new);
        assert_eq!(created.token.len(), 64);
        assert!((PORT_RANGE.0..PORT_RANGE.1).contains(&created.port));
        assert_eq!(created.approval, ApprovalMode::Ask);

        let (loaded, was_new) = Config::load_or_create(&path).unwrap();
        assert!(!was_new);
        assert_eq!(loaded, created);
    }

    #[test]
    fn missing_fields_use_defaults_and_secrets_are_filled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "approval = \"allow_all\"\n").unwrap();
        let (config, _) = Config::load_or_create(&path).unwrap();
        assert_eq!(config.approval, ApprovalMode::AllowAll);
        assert_eq!(config.max_file_mb, 100);
        assert_eq!(config.token.len(), 64);
        assert_ne!(config.port, 0);
    }

    #[test]
    fn bad_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "approval = 42\n").unwrap();
        assert!(matches!(
            Config::load(&path),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn allow_and_disallow() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        let real = config.allow(dir.path()).unwrap();
        config.allow(dir.path()).unwrap();
        assert_eq!(config.allowed_folders, vec![real.clone()]);
        let (folders, missing) = config.allowed();
        assert_eq!(folders.roots(), &[real]);
        assert!(missing.is_empty());
        assert!(config.disallow(dir.path()));
        assert!(config.allowed_folders.is_empty());
    }

    #[test]
    fn allowing_a_missing_folder_fails() {
        let mut config = Config::default();
        assert!(config.allow("/does/not/exist/anywhere").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        Config::generate().save(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
