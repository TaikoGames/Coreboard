//! Watching a folder: images that appear in it are added to the board, in a frame of their own.

use std::path::PathBuf;

use super::*;
use crate::board::WatchFolder;

/// How often the folder is checked.
const POLL_EVERY: Duration = Duration::from_millis(1500);
/// A file must keep the same size this long before it's imported, so half-written files
/// (an export or screenshot still being saved) are left alone.
const SETTLE: Duration = Duration::from_millis(900);
/// Watched images are scaled to this height (world units) and laid out in rows this wide.
const ROW_HEIGHT: f32 = 320.0;
const ROW_WIDTH: f32 = 1600.0;
const GAP: f32 = 20.0;
const PAD: f32 = 24.0;

const IMAGE_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "jfif"];

/// Image files in `dir` (name, size, modified), skipping hidden and partial downloads.
fn image_files(dir: &Path) -> std::io::Result<Vec<(String, u64, std::time::SystemTime)>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let ext = Path::new(&name)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if name.starts_with('.') || !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        if let Ok(meta) = entry.metadata()
            && meta.is_file()
        {
            let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            files.push((name, meta.len(), modified));
        }
    }
    files.sort_by_key(|(_, _, modified)| *modified);
    Ok(files)
}

impl CoreBoardApp {
    /// Starts watching `path`. Images already there are skipped (but can be added later).
    pub(super) fn start_watching(&mut self, path: PathBuf) {
        let existing = match image_files(&path) {
            Ok(files) => files
                .into_iter()
                .map(|(name, _, _)| name)
                .collect::<Vec<_>>(),
            Err(err) => {
                self.toast(format!("Can't watch {}: {err}", path.display()));
                return;
            }
        };
        self.toast(format!(
            "Watching {} · new images will appear here",
            boards::display_name(&path)
        ));
        self.board.watch = Some(WatchFolder {
            path,
            seen: existing.clone(),
            skipped: existing,
            frame: None,
            cursor: [0.0; 3],
        });
        self.watch_pending.clear();
        self.watch_error = None;
        self.touch();
    }

    pub(super) fn stop_watching(&mut self) {
        self.board.watch = None;
        self.watch_pending.clear();
        self.watch_error = None;
        self.touch();
    }

    /// Checks the watched folder now and then, importing images that have finished writing.
    pub(super) fn poll_watch(&mut self, ctx: &egui::Context) {
        let Some(watch) = &self.board.watch else {
            return;
        };
        ctx.request_repaint_after(POLL_EVERY);
        if self.watch_polled.elapsed() < POLL_EVERY {
            return;
        }
        self.watch_polled = Instant::now();
        let files = match image_files(&watch.path) {
            Ok(files) => {
                self.watch_error = None;
                files
            }
            Err(err) => {
                self.watch_error = Some(err.to_string());
                return;
            }
        };
        let now = Instant::now();
        let mut ready = Vec::new();
        for (name, size, _) in files {
            if watch.seen.contains(&name) {
                continue;
            }
            match self.watch_pending.get(&name) {
                Some(&(seen_size, since)) if seen_size == size => {
                    if now.duration_since(since) >= SETTLE && size > 0 {
                        ready.push(name);
                    }
                }
                _ => {
                    self.watch_pending.insert(name, (size, now));
                }
            }
        }
        if !ready.is_empty() {
            self.import_watched(&ready);
        }
    }

    /// Adds the images that were already in the folder when watching began.
    pub(super) fn add_skipped(&mut self) {
        let Some(watch) = &self.board.watch else {
            return;
        };
        let names = watch.skipped.clone();
        self.import_watched(&names);
        if let Some(watch) = &mut self.board.watch {
            watch.skipped.clear();
        }
    }

    /// Imports files from the watched folder into its frame, row by row.
    fn import_watched(&mut self, names: &[String]) {
        let Some(dir) = self.board.watch.as_ref().map(|w| w.path.clone()) else {
            return;
        };
        let mut added = 0;
        for name in names {
            let path = dir.join(name);
            if let Some(watch) = &mut self.board.watch {
                watch.seen.push(name.clone());
                watch.skipped.retain(|n| n != name);
            }
            self.watch_pending.remove(name);
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let (Some(ext), Ok((w, h))) = (
                board::image_ext(&bytes, Some(&path)),
                image_dimensions(&bytes),
            ) else {
                continue;
            };
            let Ok(file) = self.store.import(&bytes, &ext) else {
                continue;
            };
            if added == 0 {
                self.checkpoint();
            }
            let frame = self.watch_frame(&dir);
            let size = [ROW_HEIGHT * w as f32 / h.max(1) as f32, ROW_HEIGHT];
            let watch = self.board.watch.as_mut().unwrap();
            let [mut x, mut y, mut row] = watch.cursor;
            if x > 0.0 && x + size[0] > ROW_WIDTH {
                x = 0.0;
                y += row + GAP;
                row = 0.0;
            }
            watch.cursor = [x + size[0] + GAP, y, row.max(size[1])];
            let origin = self
                .board
                .items
                .iter()
                .find(|i| i.id == frame)
                .map(|f| [f.pos[0] + PAD, f.pos[1] + FRAME_TITLE_HEIGHT + PAD])
                .unwrap_or([0.0, 0.0]);
            let id = self.board.next_id;
            self.board.next_id += 1;
            let pos = [origin[0] + x, origin[1] + y];
            self.board.items.push(Item {
                id,
                file,
                pos,
                size,
                ..Item::default()
            });
            // Grow the frame to hold the new image.
            if let Some(f) = self.board.items.iter_mut().find(|i| i.id == frame) {
                f.size[0] = f.size[0].max(pos[0] + size[0] + PAD - f.pos[0]);
                f.size[1] = f.size[1].max(pos[1] + size[1] + PAD - f.pos[1]);
            }
            added += 1;
        }
        if added > 0 {
            let what = if added == 1 {
                names[0].clone()
            } else {
                format!("{added} images")
            };
            self.toast(format!("Added {what} from {}", boards::display_name(&dir)));
            self.touch();
        }
    }

    /// The watched folder's frame, created to the right of everything if it's missing.
    fn watch_frame(&mut self, dir: &Path) -> u64 {
        let existing = self.board.watch.as_ref().and_then(|w| w.frame);
        if let Some(id) = existing
            && self.board.items.iter().any(|i| i.id == id && i.is_frame())
        {
            return id;
        }
        let right = self
            .board
            .items
            .iter()
            .map(|i| i.pos[0] + i.size[0])
            .fold(f32::MIN, f32::max);
        let top = self
            .board
            .items
            .iter()
            .map(|i| i.pos[1])
            .fold(f32::MAX, f32::min);
        let pos = if right == f32::MIN {
            [0.0, 0.0]
        } else {
            [right + 80.0, top]
        };
        let id = self.board.next_id;
        self.board.next_id += 1;
        self.board.items.push(Item {
            id,
            pos,
            size: [
                ROW_HEIGHT + 2.0 * PAD,
                ROW_HEIGHT + FRAME_TITLE_HEIGHT + 2.0 * PAD,
            ],
            kind: Kind::Frame {
                title: format!("Watching: {}", boards::display_name(dir)),
            },
            ..Item::default()
        });
        let watch = self.board.watch.as_mut().unwrap();
        watch.frame = Some(id);
        watch.cursor = [0.0; 3];
        id
    }

    /// The watch-folder section of the Boards window.
    pub(super) fn watch_controls(&mut self, ui: &mut egui::Ui) {
        ui.label("Watch a folder");
        match self.board.watch.clone() {
            None => {
                ui.label(
                    egui::RichText::new(
                        "New images saved there (screenshots, renders) appear on this board.",
                    )
                    .small()
                    .weak(),
                );
                ui.horizontal(|ui| {
                    if ui.button("Choose folder…").clicked()
                        && let Some(dir) = rfd::FileDialog::new()
                            .set_title("Folder to watch")
                            .pick_folder()
                    {
                        self.start_watching(dir);
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.watch_path_input)
                            .hint_text("or paste a folder path")
                            .desired_width(170.0),
                    );
                    let path = PathBuf::from(self.watch_path_input.trim());
                    if ui
                        .add_enabled(path.is_dir(), egui::Button::new("Watch"))
                        .clicked()
                    {
                        self.watch_path_input.clear();
                        self.start_watching(path);
                    }
                });
            }
            Some(watch) => {
                ui.label(
                    egui::RichText::new(format!("Watching {}", watch.path.display()))
                        .small()
                        .strong(),
                );
                if let Some(err) = &self.watch_error {
                    ui.colored_label(Color32::LIGHT_RED, format!("Can't read it: {err}"));
                }
                ui.horizontal(|ui| {
                    if !watch.skipped.is_empty()
                        && ui
                            .button(format!("Add the {} already there", watch.skipped.len()))
                            .clicked()
                    {
                        self.add_skipped();
                    }
                    if ui.button("Stop watching").clicked() {
                        self.stop_watching();
                    }
                });
            }
        }
    }
}
