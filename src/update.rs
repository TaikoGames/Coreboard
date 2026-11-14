//! Checks mikibutler.ca for a newer Core Board, and on Windows installs it in one click.
//!
//! The website publishes `latest.json` next to the download page with every release. The check
//! is one small request on startup (it can be turned off in the Boards window); nothing about
//! the user or their boards is sent.

use std::path::PathBuf;

use serde::Deserialize;

/// Where the latest version is announced.
pub const LATEST_URL: &str = "https://mikibutler.ca/coreboard/latest.json";
/// The download page, for the button that's always there and for platforms without one-click.
pub const DOWNLOAD_PAGE: &str = "https://mikibutler.ca/coreboard/#install";

#[derive(Clone, Debug, Deserialize)]
pub struct Latest {
    pub version: String,
    /// A few one-line highlights for the banner.
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub page: Option<String>,
    /// The Windows installer, for one-click updates.
    #[serde(default)]
    pub windows_installer: Option<String>,
}

impl Latest {
    pub fn page(&self) -> &str {
        self.page.as_deref().unwrap_or(DOWNLOAD_PAGE)
    }
}

/// "0.3.10" -> [0, 3, 10]; anything after a '-' or '+' (pre-release tags) is ignored.
fn parts(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches('v')
        .split(['-', '+'])
        .next()
        .unwrap_or("")
        .split('.')
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

/// Whether `latest` is a newer version than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let (mut a, mut b) = (parts(latest), parts(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

/// Asks the website for the latest version.
pub fn check() -> Result<Latest, String> {
    let agent = crate::web::agent();
    // Overridable for testing against a local server.
    let url = std::env::var("COREBOARD_UPDATE_URL").unwrap_or_else(|_| LATEST_URL.to_string());
    let (bytes, _) = crate::web::get(&agent, &url, 64 * 1024)?;
    serde_json::from_slice(&bytes).map_err(|err| format!("unexpected answer: {err}"))
}

/// Whether this copy was put there by the Windows installer (rather than unzipped from the
/// portable zip, or on macOS/Linux), so it can update itself by running the next installer.
pub fn can_self_install() -> bool {
    cfg!(windows)
        && std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("unins000.exe").is_file()))
            .unwrap_or(false)
}

/// Downloads the installer into the temp folder.
pub fn download_installer(latest: &Latest) -> Result<PathBuf, String> {
    let url = latest
        .windows_installer
        .as_deref()
        .ok_or("no installer for this version")?;
    let agent = crate::web::agent();
    let (bytes, _) = crate::web::get(&agent, url, 200 * 1024 * 1024)?;
    // Every Windows program starts with "MZ"; anything else is an error page, not an installer.
    if !bytes.starts_with(b"MZ") {
        return Err("the download wasn't an installer".into());
    }
    let path = std::env::temp_dir().join(format!("CoreBoard-Setup-{}.exe", latest.version));
    std::fs::write(&path, bytes).map_err(|err| format!("couldn't save the installer: {err}"))?;
    Ok(path)
}

/// Starts the installer. It shows only a progress bar, moves this running copy aside, installs
/// the new one and opens it. The caller should save and quit right after.
pub fn run_installer(path: &PathBuf) -> Result<(), String> {
    std::process::Command::new(path)
        .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("couldn't start the installer: {err}"))
}

/// Deletes copies of the app that an update moved aside while they were running (Windows).
pub fn remove_old_copies() {
    if !cfg!(windows) {
        return;
    }
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.to_path_buf()))
    else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.starts_with("coreboard.old") && name.ends_with(".exe") {
            // Still running somewhere? Then it goes next time.
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_by_number() {
        assert!(is_newer("0.3.4", "0.3.3"));
        assert!(is_newer("0.3.10", "0.3.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(is_newer("v0.4.0", "0.3.3"));
        assert!(!is_newer("0.3.3", "0.3.3"));
        assert!(!is_newer("0.3.3", "0.3.4"));
        assert!(!is_newer("0.3.3-beta", "0.3.3"));
        assert!(!is_newer("garbage", "0.3.3"));
    }

    #[test]
    fn reads_the_announcement() {
        let latest: Latest = serde_json::from_str(
            r#"{"version":"0.3.4","notes":["A","B"],"windows_installer":"https://x/y.exe"}"#,
        )
        .unwrap();
        assert_eq!(latest.version, "0.3.4");
        assert_eq!(latest.page(), DOWNLOAD_PAGE);
        // Unknown fields from future versions of the file are fine.
        let latest: Latest =
            serde_json::from_str(r#"{"version":"1.0.0","extra":{"a":1}}"#).unwrap();
        assert!(latest.notes.is_empty());
    }
}
