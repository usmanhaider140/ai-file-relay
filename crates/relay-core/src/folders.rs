//! Allowed folders and path checks.

use std::path::{Path, PathBuf};

/// Folder names that are never reachable, even inside an allowed folder.
const BLOCKED_DIR_NAMES: &[&str] = &[".ssh", ".gnupg", ".aws", ".kube", ".docker"];

/// Why a path was refused.
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("path does not exist or cannot be read: {0}")]
    NotFound(PathBuf),
    #[error("path is outside the allowed folders: {0}")]
    OutsideAllowed(PathBuf),
    #[error("path is inside a blocked folder ({name}): {path}")]
    Blocked { path: PathBuf, name: String },
    #[error("allowed folder does not exist: {0}")]
    BadRoot(PathBuf),
    #[error("not a folder: {0}")]
    NotADirectory(PathBuf),
}

/// The folders an AI is allowed to read from and write to.
#[derive(Debug, Clone, Default)]
pub struct AllowedFolders {
    roots: Vec<PathBuf>,
}

impl AllowedFolders {
    /// An empty list: nothing is reachable.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a folder. The path is resolved first, so later checks compare
    /// real paths and a symlink cannot be used to escape.
    pub fn add(&mut self, folder: impl AsRef<Path>) -> Result<(), PathError> {
        let folder = folder.as_ref();
        let real = folder
            .canonicalize()
            .map_err(|_| PathError::BadRoot(folder.to_path_buf()))?;
        if !real.is_dir() {
            return Err(PathError::BadRoot(folder.to_path_buf()));
        }
        if !self.roots.contains(&real) {
            self.roots.push(real);
        }
        Ok(())
    }

    /// The resolved allowed folders.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Check a path and return its resolved form if it may be used.
    ///
    /// The path must exist. `..` parts and symlinks are resolved before the
    /// check, so they cannot point outside an allowed folder.
    pub fn check(&self, path: impl AsRef<Path>) -> Result<PathBuf, PathError> {
        let path = path.as_ref();
        let real = path
            .canonicalize()
            .map_err(|_| PathError::NotFound(path.to_path_buf()))?;

        let root = self
            .roots
            .iter()
            .find(|root| real.starts_with(root))
            .ok_or_else(|| PathError::OutsideAllowed(path.to_path_buf()))?;

        if let Ok(rest) = real.strip_prefix(root) {
            for part in rest.components() {
                let name = part.as_os_str().to_string_lossy();
                if BLOCKED_DIR_NAMES
                    .iter()
                    .any(|b| name.eq_ignore_ascii_case(b))
                {
                    return Err(PathError::Blocked {
                        path: path.to_path_buf(),
                        name: name.into_owned(),
                    });
                }
            }
        }

        Ok(real)
    }

    /// List one folder (not its subfolders). The folder must pass
    /// [`check`](Self::check). Blocked folders are left out, and so are
    /// entries that point outside the allowed folders (for example a
    /// symlink to somewhere else). Sorted: folders first, then by name.
    pub fn list(&self, folder: impl AsRef<Path>) -> Result<Vec<Entry>, PathError> {
        let folder = folder.as_ref();
        let real = self.check(folder)?;
        if !real.is_dir() {
            return Err(PathError::NotADirectory(folder.to_path_buf()));
        }
        let read =
            std::fs::read_dir(&real).map_err(|_| PathError::NotFound(folder.to_path_buf()))?;
        let mut entries: Vec<Entry> = read
            .filter_map(Result::ok)
            .filter_map(|e| {
                let path = self.check(e.path()).ok()?;
                let meta = std::fs::metadata(&path).ok()?;
                Some(Entry {
                    name: e.file_name().to_string_lossy().into_owned(),
                    path,
                    kind: if meta.is_dir() {
                        EntryKind::Folder
                    } else {
                        EntryKind::File
                    },
                    size: meta.is_file().then_some(meta.len()),
                })
            })
            .collect();
        entries.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
        Ok(entries)
    }
}

/// File or folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Folder,
    File,
}

/// One item from [`AllowedFolders::list`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub kind: EntryKind,
    /// Size in bytes, for files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup() -> (tempfile::TempDir, AllowedFolders) {
        let dir = tempfile::tempdir().unwrap();
        let allowed = dir.path().join("allowed");
        fs::create_dir(&allowed).unwrap();
        fs::write(allowed.join("ok.txt"), "hi").unwrap();
        fs::write(dir.path().join("secret.txt"), "no").unwrap();
        let mut folders = AllowedFolders::new();
        folders.add(&allowed).unwrap();
        (dir, folders)
    }

    #[test]
    fn empty_list_allows_nothing() {
        let (dir, _) = setup();
        let folders = AllowedFolders::new();
        let err = folders
            .check(dir.path().join("allowed/ok.txt"))
            .unwrap_err();
        assert!(matches!(err, PathError::OutsideAllowed(_)));
    }

    #[test]
    fn file_inside_allowed_folder_passes() {
        let (dir, folders) = setup();
        let real = folders.check(dir.path().join("allowed/ok.txt")).unwrap();
        assert!(real.ends_with("ok.txt"));
    }

    #[test]
    fn file_outside_is_refused() {
        let (dir, folders) = setup();
        let err = folders.check(dir.path().join("secret.txt")).unwrap_err();
        assert!(matches!(err, PathError::OutsideAllowed(_)));
    }

    #[test]
    fn dot_dot_cannot_escape() {
        let (dir, folders) = setup();
        let sneaky = dir.path().join("allowed/../secret.txt");
        let err = folders.check(sneaky).unwrap_err();
        assert!(matches!(err, PathError::OutsideAllowed(_)));
    }

    #[test]
    fn missing_file_is_refused() {
        let (dir, folders) = setup();
        let err = folders
            .check(dir.path().join("allowed/nope.txt"))
            .unwrap_err();
        assert!(matches!(err, PathError::NotFound(_)));
    }

    #[test]
    fn blocked_folder_is_refused_even_when_allowed() {
        let (dir, folders) = setup();
        let ssh = dir.path().join("allowed/.ssh");
        fs::create_dir(&ssh).unwrap();
        fs::write(ssh.join("id_ed25519"), "key").unwrap();
        let err = folders.check(ssh.join("id_ed25519")).unwrap_err();
        assert!(matches!(err, PathError::Blocked { .. }));
    }

    #[test]
    fn prefix_lookalike_is_not_inside() {
        // "allowed-2" starts with the same letters as "allowed" but is a
        // different folder.
        let (dir, folders) = setup();
        let twin = dir.path().join("allowed-2");
        fs::create_dir(&twin).unwrap();
        fs::write(twin.join("x.txt"), "x").unwrap();
        let err = folders.check(twin.join("x.txt")).unwrap_err();
        assert!(matches!(err, PathError::OutsideAllowed(_)));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_cannot_escape() {
        let (dir, folders) = setup();
        let link = dir.path().join("allowed/link.txt");
        std::os::unix::fs::symlink(dir.path().join("secret.txt"), &link).unwrap();
        let err = folders.check(link).unwrap_err();
        assert!(matches!(err, PathError::OutsideAllowed(_)));
    }

    #[test]
    fn list_shows_folders_first_and_hides_blocked() {
        let (dir, folders) = setup();
        let allowed = dir.path().join("allowed");
        fs::create_dir(allowed.join("sub")).unwrap();
        fs::create_dir(allowed.join(".ssh")).unwrap();
        fs::write(allowed.join("a.txt"), "12345").unwrap();
        let names: Vec<(String, EntryKind, Option<u64>)> = folders
            .list(&allowed)
            .unwrap()
            .into_iter()
            .map(|e| (e.name, e.kind, e.size))
            .collect();
        assert_eq!(
            names,
            vec![
                ("sub".to_string(), EntryKind::Folder, None),
                ("a.txt".to_string(), EntryKind::File, Some(5)),
                ("ok.txt".to_string(), EntryKind::File, Some(2)),
            ]
        );
    }

    #[test]
    fn list_outside_is_refused() {
        let (dir, folders) = setup();
        assert!(matches!(
            folders.list(dir.path()),
            Err(PathError::OutsideAllowed(_))
        ));
    }

    #[test]
    fn list_a_file_is_refused() {
        let (dir, folders) = setup();
        assert!(matches!(
            folders.list(dir.path().join("allowed/ok.txt")),
            Err(PathError::NotADirectory(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn list_hides_symlinks_that_escape() {
        let (dir, folders) = setup();
        let allowed = dir.path().join("allowed");
        std::os::unix::fs::symlink(dir.path().join("secret.txt"), allowed.join("link.txt"))
            .unwrap();
        let names: Vec<String> = folders
            .list(&allowed)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["ok.txt".to_string()]);
    }

    #[test]
    fn bad_root_is_refused() {
        let mut folders = AllowedFolders::new();
        assert!(matches!(
            folders.add("/this/folder/does/not/exist"),
            Err(PathError::BadRoot(_))
        ));
    }
}
