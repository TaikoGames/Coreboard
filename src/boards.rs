//! Remembers recently opened boards, so the app can reopen the last one and list the rest.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_RECENT: usize = 12;

#[derive(Default, Serialize, Deserialize)]
struct Recent {
    boards: Vec<PathBuf>,
}

fn recent_file() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "coreboard").map(|d| d.config_dir().join("recent.json"))
}

/// App-wide preferences, kept next to the recent-boards list.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Glides, pops, swing and flicks. Off for anyone who prefers a still board.
    pub animations: bool,
    /// The overview of the board in the corner while part of it is off screen.
    pub minimap: bool,
    /// Ask mikibutler.ca for a newer version on startup.
    pub check_updates: bool,
    /// "auto" (the computer's language), "en" or "es".
    pub language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            animations: true,
            minimap: true,
            check_updates: true,
            language: "auto".into(),
        }
    }
}

fn settings_file() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "coreboard")
        .map(|d| d.config_dir().join("settings.json"))
}

pub fn settings() -> Settings {
    settings_file()
        .and_then(|f| fs::read(f).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_settings(settings: &Settings) {
    let Some(file) = settings_file() else { return };
    if let Some(parent) = file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_vec_pretty(settings) {
        let _ = fs::write(file, json);
    }
}

/// Recently opened boards, most recent first, skipping folders that no longer exist.
pub fn recent() -> Vec<PathBuf> {
    let Some(bytes) = recent_file().and_then(|f| fs::read(f).ok()) else {
        return Vec::new();
    };
    let recent: Recent = serde_json::from_slice(&bytes).unwrap_or_default();
    recent.boards.into_iter().filter(|p| p.is_dir()).collect()
}

/// Moves `dir` to the top of the recent list.
pub fn remember(dir: &Path) {
    let Some(file) = recent_file() else { return };
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let mut boards = recent();
    boards.retain(|p| p != &dir);
    boards.insert(0, dir);
    boards.truncate(MAX_RECENT);
    if let Some(parent) = file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_vec_pretty(&Recent { boards }) {
        let _ = fs::write(file, json);
    }
}

/// Adds `dir` to the end of the recent list (a board that arrived from the cloud), unless it's
/// there already.
pub fn add_recent(dir: &Path) {
    let Some(file) = recent_file() else { return };
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let mut boards = recent();
    if boards.contains(&dir) {
        return;
    }
    boards.push(dir);
    if let Some(parent) = file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_vec_pretty(&Recent { boards }) {
        let _ = fs::write(file, json);
    }
}

/// Where boards created by name live: next to the default board.
pub fn boards_root() -> PathBuf {
    crate::board::default_board_dir()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A folder name for a board called `name`: keeps letters, digits, spaces, `-`, `_` and
/// brackets.
pub fn folder_name(name: &str) -> Option<String> {
    let cleaned: String = name
        .trim()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '(' | ')'))
        .collect();
    let cleaned = cleaned.trim().to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// A short display name for a board folder.
pub fn display_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_safe() {
        assert_eq!(folder_name("  Mood board #2 "), Some("Mood board 2".into()));
        assert_eq!(folder_name("../../etc"), Some("etc".into()));
        assert_eq!(folder_name("///"), None);
        assert_eq!(
            folder_name("Forêt_concept-art"),
            Some("Forêt_concept-art".into())
        );
    }
}
