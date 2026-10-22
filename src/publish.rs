//! Publishing a board as a web-ready copy.
//!
//! Writes `board.json` plus web-sized copies of every referenced image into a destination
//! folder (for example a static site's `board/<name>/`). An online viewer can then read that
//! `board.json` and the `images/` beside it and redraw the board, read-only.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::board::Board;

pub struct Report {
    pub images: usize,
    pub dir: PathBuf,
}

/// Copies `board` and its images into `dest`, shrinking images whose longest edge exceeds
/// `max_edge` so the published board stays light. The board is written as given, so the caller
/// should clear any local-only fields (export/publish folders, watch) first.
pub fn publish(
    board: &Board,
    images_dir: &Path,
    dest: &Path,
    max_edge: u32,
) -> Result<Report, String> {
    let dest_images = dest.join("images");
    fs::create_dir_all(&dest_images).map_err(|e| e.to_string())?;

    // board.json
    let json = serde_json::to_vec_pretty(board).map_err(|e| e.to_string())?;
    fs::write(dest.join("board.json"), json).map_err(|e| e.to_string())?;

    // every image an item refers to, once
    let used: HashSet<&str> = board
        .items
        .iter()
        .map(|item| item.file.as_str())
        .filter(|file| !file.is_empty())
        .collect();

    let mut count = 0usize;
    for file in &used {
        let src = images_dir.join(file);
        let out = dest_images.join(file);
        let bytes = match fs::read(&src) {
            Ok(bytes) => bytes,
            Err(_) => continue, // a missing source is skipped rather than failing the whole publish
        };
        let ext = Path::new(file)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        // Only re-encode formats the `image` crate writes back reliably; copy anything else as-is.
        let resizable = matches!(ext.as_str(), "jpg" | "jpeg" | "png");
        let mut wrote = false;
        if resizable && let Ok(img) = image::load_from_memory(&bytes) {
            if img.width().max(img.height()) > max_edge {
                let resized = img.resize(max_edge, max_edge, image::imageops::FilterType::Lanczos3);
                wrote = resized.save(&out).is_ok();
            }
        }
        if !wrote {
            fs::write(&out, &bytes).map_err(|e| e.to_string())?;
        }
        count += 1;
    }

    // drop images in the destination that this board no longer uses
    if let Ok(entries) = fs::read_dir(&dest_images) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !used.contains(name.as_str()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    Ok(Report {
        images: count,
        dir: dest.to_path_buf(),
    })
}
