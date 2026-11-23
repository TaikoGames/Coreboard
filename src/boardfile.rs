//! Board files: a whole board (its layout and every picture in it) packed into one
//! `.coreboard` file, to send to someone, back up, or move to another computer.
//!
//! It's an ordinary zip: `board.json` plus the pictures under `images/`. Opening one unpacks
//! it into a new board folder next to the others, so the original file is never changed.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::board::Board;

/// The extension of board files.
pub const EXT: &str = "coreboard";
/// Nothing in a board file may unpack bigger than this (a guard against broken or hostile files).
const MAX_ENTRY: u64 = 512 * 1024 * 1024;
const MAX_TOTAL: u64 = 4 * 1024 * 1024 * 1024;

/// Whether `path` names a board file.
pub fn is_board_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(EXT))
}

/// A name for the board file of the board in `dir`: "Character refs.coreboard".
pub fn suggested_name(dir: &Path) -> String {
    format!("{}.{EXT}", crate::boards::display_name(dir))
}

/// Packs `board` (stored in `dir`) into `out`. Returns how many pictures went in.
pub fn save(dir: &Path, board: &Board, out: &Path) -> Result<usize, String> {
    let tmp = out.with_extension(format!("{EXT}.tmp"));
    let result = write_zip(dir, board, &tmp);
    match result {
        Ok(count) => {
            fs::rename(&tmp, out).map_err(|e| e.to_string())?;
            Ok(count)
        }
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            Err(err)
        }
    }
}

fn write_zip(dir: &Path, board: &Board, out: &Path) -> Result<usize, String> {
    use zip::write::SimpleFileOptions;
    let file = fs::File::create(out).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let json = serde_json::to_vec_pretty(board).map_err(|e| e.to_string())?;
    zip.start_file("board.json", SimpleFileOptions::default())
        .map_err(|e| e.to_string())?;
    zip.write_all(&json).map_err(|e| e.to_string())?;

    // Pictures are already compressed: store them as they are, which is fast.
    let stored = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);
    let mut files: Vec<&str> = board
        .items
        .iter()
        .map(|i| i.file.as_str())
        .filter(|f| !f.is_empty())
        .collect();
    files.sort_unstable();
    files.dedup();
    let mut count = 0;
    for name in files {
        let Ok(bytes) = fs::read(dir.join("images").join(name)) else {
            continue;
        };
        zip.start_file(format!("images/{name}"), stored)
            .map_err(|e| e.to_string())?;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
        count += 1;
    }
    zip.finish()
        .map_err(|e| e.to_string())?
        .flush()
        .map_err(|e| e.to_string())?;
    Ok(count)
}

/// A plain file name from inside the zip, or None for anything that could reach outside the
/// images folder.
fn safe_name(name: &str) -> Option<&str> {
    let name = name.strip_prefix("images/")?;
    let ok = !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':', '\0']);
    ok.then_some(name)
}

/// A folder under `root` for a board named after `file`, not used by any other board yet.
fn new_folder(root: &Path, file: &Path) -> PathBuf {
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = crate::boards::folder_name(&stem).unwrap_or_else(|| "Board".into());
    let mut dir = root.join(&name);
    let mut n = 2;
    while dir.exists() {
        dir = root.join(format!("{name} {n}"));
        n += 1;
    }
    dir
}

/// Unpacks the board file `file` into a new board folder under `root` and returns the folder.
pub fn open(file: &Path, root: &Path) -> Result<PathBuf, String> {
    let reader = fs::File::open(file).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(reader))
        .map_err(|_| crate::i18n::tr("this isn't a Core Board file").to_string())?;

    let board: Board = {
        let mut entry = zip
            .by_name("board.json")
            .map_err(|_| crate::i18n::tr("this isn't a Core Board file").to_string())?;
        let mut json = Vec::new();
        entry
            .by_ref()
            .take(MAX_ENTRY)
            .read_to_end(&mut json)
            .map_err(|e| e.to_string())?;
        serde_json::from_slice(&json).map_err(|e| {
            crate::i18n::trf(
                "it was made by a newer Core Board, or is damaged ({})",
                &[&e],
            )
        })?
    };

    let dir = new_folder(root, file);
    let images = dir.join("images");
    fs::create_dir_all(&images).map_err(|e| e.to_string())?;
    let result = (|| {
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let Ok(raw) = entry.name() else {
                continue;
            };
            let Some(name) = safe_name(&raw).map(str::to_owned) else {
                continue;
            };
            if !board.items.iter().any(|item| item.file == name) {
                continue;
            }
            let mut bytes = Vec::new();
            entry
                .by_ref()
                .take(MAX_ENTRY + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            total += bytes.len() as u64;
            if bytes.len() as u64 > MAX_ENTRY || total > MAX_TOTAL {
                return Err(crate::i18n::tr("it's too big").to_string());
            }
            fs::write(images.join(&name), bytes).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_vec_pretty(&board).map_err(|e| e.to_string())?;
        fs::write(dir.join("board.json"), json).map_err(|e| e.to_string())
    })();
    match result {
        Ok(()) => Ok(dir),
        Err(err) => {
            // Don't leave half a board behind.
            let _ = fs::remove_dir_all(&dir);
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Item;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("coreboard-boardfile-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn saves_and_opens_a_whole_board() {
        let base = temp("roundtrip");
        let source = base.join("Character refs");
        fs::create_dir_all(source.join("images")).unwrap();
        fs::write(source.join("images/a.png"), b"picture a").unwrap();
        fs::write(source.join("images/b.jpg"), b"picture b").unwrap();
        // Left over from a deleted picture: not part of the board, so not packed.
        fs::write(source.join("images/old.png"), b"unused").unwrap();
        let board = Board {
            items: vec![
                Item {
                    id: 1,
                    file: "a.png".into(),
                    caption: "Front".into(),
                    ..Item::default()
                },
                Item {
                    id: 2,
                    file: "b.jpg".into(),
                    ..Item::default()
                },
                Item {
                    id: 3,
                    file: "a.png".into(),
                    ..Item::default()
                },
            ],
            ..Board::default()
        };
        let file = base.join(suggested_name(&source));
        assert!(file.ends_with("Character refs.coreboard"));
        assert_eq!(save(&source, &board, &file).unwrap(), 2);
        assert!(is_board_file(&file));

        let root = base.join("boards");
        let opened = open(&file, &root).unwrap();
        assert_eq!(opened, root.join("Character refs"));
        assert_eq!(fs::read(opened.join("images/a.png")).unwrap(), b"picture a");
        assert_eq!(fs::read(opened.join("images/b.jpg")).unwrap(), b"picture b");
        assert!(!opened.join("images/old.png").exists());
        let back: Board =
            serde_json::from_slice(&fs::read(opened.join("board.json")).unwrap()).unwrap();
        assert_eq!(back.items.len(), 3);
        assert_eq!(back.items[0].caption, "Front");

        // Opening it again makes a second board rather than mixing into the first.
        assert_eq!(open(&file, &root).unwrap(), root.join("Character refs 2"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn refuses_other_files_and_unsafe_names() {
        let base = temp("refuse");
        let bogus = base.join("photo.coreboard");
        fs::write(&bogus, b"not a zip").unwrap();
        assert!(open(&bogus, &base.join("boards")).is_err());
        assert!(!base.join("boards/photo").exists());

        assert_eq!(safe_name("images/a.png"), Some("a.png"));
        assert_eq!(safe_name("images/../../evil"), None);
        assert_eq!(safe_name("images/sub/x.png"), None);
        assert_eq!(safe_name("images/..\\x"), None);
        assert_eq!(safe_name("board.json"), None);
        let _ = fs::remove_dir_all(&base);
    }
}
