//! Registers the native messaging host with Chrome (and Chromium, Edge,
//! Brave), so the extension can start `relay-mcp`.
//!
//! On macOS and Linux this writes a small JSON file into each browser's
//! `NativeMessagingHosts` folder. On Windows it writes the JSON file into the
//! app's data folder and points a registry key at it.

use std::path::{Path, PathBuf};

use relay_protocol::{DEV_EXTENSION_ID, NATIVE_HOST_NAME};
use serde_json::json;

/// What was done for one browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub browser: String,
    /// The manifest file (and on Windows, also the registry key).
    pub location: String,
}

/// Manifest JSON for the host.
pub fn manifest(exe: &Path, extension_ids: &[String]) -> serde_json::Value {
    let origins: Vec<String> = extension_ids
        .iter()
        .map(|id| format!("chrome-extension://{id}/"))
        .collect();
    json!({
        "name": NATIVE_HOST_NAME,
        "description": "AI File Relay: lets AI apps move files into and out of web pages",
        "path": exe,
        "type": "stdio",
        "allowed_origins": origins,
    })
}

/// The extension IDs to allow: the pinned dev ID plus any extra ones
/// (for example the Chrome Web Store ID).
pub fn extension_ids(extra: &[String]) -> Vec<String> {
    let mut ids = vec![DEV_EXTENSION_ID.to_string()];
    for id in extra {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}

/// Write the manifest into one `NativeMessagingHosts` folder.
pub fn write_manifest(
    folder: &Path,
    exe: &Path,
    extension_ids: &[String],
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let file = folder.join(format!("{NATIVE_HOST_NAME}.json"));
    let text = serde_json::to_string_pretty(&manifest(exe, extension_ids))
        .map_err(std::io::Error::other)?;
    std::fs::write(&file, text)?;
    Ok(file)
}

/// Browser folders that hold `NativeMessagingHosts`, relative to the user's
/// home (macOS, Linux). Chrome is always included; the others only if the
/// browser looks installed.
#[cfg(not(windows))]
fn browser_folders(home: &Path) -> Vec<(&'static str, PathBuf, bool)> {
    #[cfg(target_os = "macos")]
    let base = home.join("Library/Application Support");
    #[cfg(target_os = "macos")]
    let list = [
        ("Chrome", "Google/Chrome", true),
        ("Chrome Beta", "Google/Chrome Beta", false),
        ("Chrome for Testing", "Google/Chrome for Testing", false),
        ("Chromium", "Chromium", false),
        ("Edge", "Microsoft Edge", false),
        ("Brave", "BraveSoftware/Brave-Browser", false),
    ];
    #[cfg(not(target_os = "macos"))]
    let base = home.join(".config");
    #[cfg(not(target_os = "macos"))]
    let list = [
        ("Chrome", "google-chrome", true),
        ("Chrome Beta", "google-chrome-beta", false),
        ("Chrome for Testing", "google-chrome-for-testing", false),
        ("Chromium", "chromium", false),
        ("Edge", "microsoft-edge", false),
        ("Brave", "BraveSoftware/Brave-Browser", false),
    ];
    list.into_iter()
        .map(|(name, rel, always)| (name, base.join(rel), always))
        .collect()
}

/// Register the host for every browser found. `exe` is the full path of
/// `relay-mcp`.
pub fn install(exe: &Path, extension_ids: &[String]) -> Result<Vec<Installed>, String> {
    let home = directories::BaseDirs::new()
        .ok_or("could not find your home folder")?
        .home_dir()
        .to_path_buf();
    install_for_home(&home, exe, extension_ids)
}

#[cfg(not(windows))]
fn install_for_home(
    home: &Path,
    exe: &Path,
    extension_ids: &[String],
) -> Result<Vec<Installed>, String> {
    let mut done = Vec::new();
    for (browser, folder, always) in browser_folders(home) {
        if !always && !folder.exists() {
            continue;
        }
        let file = write_manifest(&folder.join("NativeMessagingHosts"), exe, extension_ids)
            .map_err(|e| format!("could not write the {browser} manifest: {e}"))?;
        done.push(Installed {
            browser: browser.into(),
            location: file.display().to_string(),
        });
    }
    Ok(done)
}

#[cfg(windows)]
fn install_for_home(
    _home: &Path,
    exe: &Path,
    extension_ids: &[String],
) -> Result<Vec<Installed>, String> {
    let data = directories::ProjectDirs::from("io.github", "usmanhaider140", "ai-file-relay")
        .ok_or("could not find the app data folder")?;
    let file = write_manifest(data.data_dir(), exe, extension_ids)
        .map_err(|e| format!("could not write the manifest: {e}"))?;
    let mut done = Vec::new();
    for (browser, key) in windows_keys() {
        let status = std::process::Command::new("reg")
            .args(["add", &key, "/ve", "/t", "REG_SZ", "/d"])
            .arg(&file)
            .arg("/f")
            .stdout(std::process::Stdio::null())
            .status()
            .map_err(|e| format!("could not run reg.exe: {e}"))?;
        if !status.success() {
            return Err(format!("reg.exe failed for {browser}"));
        }
        done.push(Installed {
            browser: browser.into(),
            location: format!("{} (registry: HKCU\\{key})", file.display()),
        });
    }
    Ok(done)
}

#[cfg(windows)]
fn windows_keys() -> Vec<(&'static str, String)> {
    [
        ("Chrome", "Software\\Google\\Chrome"),
        ("Chromium", "Software\\Chromium"),
        ("Edge", "Software\\Microsoft\\Edge"),
        ("Brave", "Software\\BraveSoftware\\Brave-Browser"),
    ]
    .into_iter()
    .map(|(name, base)| {
        (
            name,
            format!("HKCU\\{base}\\NativeMessagingHosts\\{NATIVE_HOST_NAME}"),
        )
    })
    .collect()
}

/// Remove the host registration. Returns what was removed.
pub fn uninstall() -> Result<Vec<String>, String> {
    let mut removed = Vec::new();
    #[cfg(not(windows))]
    {
        let home = directories::BaseDirs::new()
            .ok_or("could not find your home folder")?
            .home_dir()
            .to_path_buf();
        for (_, folder, _) in browser_folders(&home) {
            let file = folder
                .join("NativeMessagingHosts")
                .join(format!("{NATIVE_HOST_NAME}.json"));
            if file.exists() {
                std::fs::remove_file(&file).map_err(|e| format!("{}: {e}", file.display()))?;
                removed.push(file.display().to_string());
            }
        }
    }
    #[cfg(windows)]
    {
        for (_, key) in windows_keys() {
            let ok = std::process::Command::new("reg")
                .args(["delete", &key, "/f"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if ok {
                removed.push(format!("HKCU\\{key}"));
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_has_the_right_fields() {
        let ids = extension_ids(&["abc".into(), DEV_EXTENSION_ID.into()]);
        assert_eq!(ids.len(), 2, "no duplicates");
        let m = manifest(Path::new("/usr/local/bin/relay-mcp"), &ids);
        assert_eq!(m["name"], NATIVE_HOST_NAME);
        assert_eq!(m["type"], "stdio");
        assert_eq!(m["path"], "/usr/local/bin/relay-mcp");
        assert_eq!(
            m["allowed_origins"][0],
            format!("chrome-extension://{DEV_EXTENSION_ID}/")
        );
        assert_eq!(m["allowed_origins"][1], "chrome-extension://abc/");
    }

    #[test]
    fn writes_manifest_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = write_manifest(
            &dir.path().join("NativeMessagingHosts"),
            Path::new("/bin/relay-mcp"),
            &extension_ids(&[]),
        )
        .unwrap();
        assert!(file.ends_with(format!("{NATIVE_HOST_NAME}.json")));
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        assert_eq!(v["path"], "/bin/relay-mcp");
    }

    #[cfg(not(windows))]
    #[test]
    fn installs_chrome_always_and_others_only_if_present() {
        let home = tempfile::tempdir().unwrap();
        let (_, chromium, _) = browser_folders(home.path())
            .into_iter()
            .find(|(n, _, _)| *n == "Chromium")
            .unwrap();
        std::fs::create_dir_all(&chromium).unwrap();
        let done = install_for_home(
            home.path(),
            Path::new("/bin/relay-mcp"),
            &extension_ids(&[]),
        )
        .unwrap();
        let names: Vec<&str> = done.iter().map(|d| d.browser.as_str()).collect();
        assert_eq!(names, ["Chrome", "Chromium"]);
        for d in &done {
            assert!(Path::new(&d.location).exists());
        }
    }
}
