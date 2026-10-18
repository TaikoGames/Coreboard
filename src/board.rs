//! Board data model and on-disk storage.
//!
//! A board is a directory:
//!   board.json   – item layout and view state
//!   images/      – copies of every image dropped on the board, named by content hash

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub id: u64,
    /// File name inside the board's `images/` directory.
    pub file: String,
    /// Top-left corner in world coordinates.
    pub pos: [f32; 2],
    /// Displayed size in world units.
    pub size: [f32; 2],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct View {
    pub offset: [f32; 2],
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            offset: [0.0, 0.0],
            zoom: 1.0,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Board {
    /// Draw order: later items are on top.
    pub items: Vec<Item>,
    pub view: View,
    pub next_id: u64,
}

pub struct Store {
    pub dir: PathBuf,
}

impl Store {
    pub fn open(dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(dir.join("images"))?;
        Ok(Self { dir })
    }

    pub fn image_path(&self, file: &str) -> PathBuf {
        self.dir.join("images").join(file)
    }

    pub fn load(&self) -> Board {
        let path = self.dir.join("board.json");
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                eprintln!("coreboard: could not parse {}: {err}", path.display());
                Board::default()
            }),
            Err(_) => Board::default(),
        }
    }

    /// Writes atomically so a crash mid-save never leaves a truncated board.
    pub fn save(&self, board: &Board) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(board).map_err(io::Error::other)?;
        let tmp = self.dir.join("board.json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(tmp, self.dir.join("board.json"))
    }

    /// Copies image bytes into the board, deduplicating by content.
    pub fn import(&self, bytes: &[u8], ext: &str) -> io::Result<String> {
        let file = format!("{:016x}.{ext}", fnv1a(bytes));
        let path = self.image_path(&file);
        if !path.exists() {
            fs::write(path, bytes)?;
        }
        Ok(file)
    }

    /// Deletes image files no item refers to anymore.
    pub fn collect_garbage(&self, board: &Board) {
        let Ok(entries) = fs::read_dir(self.dir.join("images")) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !board.items.iter().any(|item| item.file == name) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

pub fn default_board_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "coreboard")
        .map(|dirs| dirs.data_dir().join("default"))
        .unwrap_or_else(|| PathBuf::from("coreboard-board"))
}

/// Picks a file extension from the image's magic bytes, falling back to the source path.
pub fn image_ext(bytes: &[u8], path: Option<&Path>) -> Option<String> {
    if let Ok(format) = image::guess_format(bytes)
        && let Some(ext) = format.extensions_str().first()
    {
        return Some((*ext).to_string());
    }
    path?
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
