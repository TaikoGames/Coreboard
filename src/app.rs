use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, ColorImage, CornerRadius, Event, FontId, Key, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextureHandle, TextureOptions, Vec2, pos2, vec2,
};

use crate::board::{
    self, Analysis, Board, FRAME_FONT, FRAME_TITLE_HEIGHT, Guide, Item, Kind, NOTE_COLORS,
    NOTE_FONT, NOTE_PAD, Store, WatermarkPosition,
};
use crate::boards;
use crate::export;
use crate::filters::{self, Filter};

const MIN_ZOOM: f32 = 0.02;
const MAX_ZOOM: f32 = 50.0;
/// Textures larger than this on either side are downscaled; the original file is kept.
const MAX_TEXTURE_SIDE: u32 = 4096;
const HANDLE_SIZE: f32 = 12.0;
const UNDO_LIMIT: usize = 200;
const AUTOSAVE_DELAY: Duration = Duration::from_millis(800);

const BG: Color32 = Color32::from_rgb(30, 31, 34);
const GRID: Color32 = Color32::from_rgb(48, 50, 54);
const ACCENT: Color32 = Color32::from_rgb(90, 160, 255);

enum Texture {
    Loading,
    Ready(TextureHandle),
    Failed(String),
}

/// (texture key, file, decoded texture + for plain textures a small copy for color picking)
type Decoded = (
    String,
    String,
    Result<(ColorImage, Option<image::RgbaImage>), String>,
);
/// Color picking reads from copies this size, which is plenty for a single pixel readout.
const SAMPLE_SIDE: u32 = 1024;
type Exported = Result<std::path::PathBuf, String>;
/// (board generation, source image id, its dominant colors)
type Extracted = (u64, u64, Result<Vec<[u8; 3]>, String>);

enum Drag {
    None,
    Pan,
    /// `carried`: items inside a dragged frame, which move along with it.
    Move {
        moved: bool,
        carried: HashSet<u64>,
    },
    /// `aspect` is locked for images and palettes; notes and frames resize freely.
    Resize {
        id: u64,
        aspect: Option<f32>,
    },
    Marquee {
        start: Pos2,
        additive: bool,
    },
    /// Drawing a new crop rectangle in crop mode.
    Crop {
        start: Pos2,
    },
}

pub struct CoreBoardApp {
    store: Store,
    board: Board,
    textures: HashMap<String, Texture>,
    decoded_tx: Sender<Decoded>,
    decoded_rx: Receiver<Decoded>,
    selection: HashSet<u64>,
    drag: Drag,
    undo: Vec<Vec<Item>>,
    redo: Vec<Vec<Item>>,
    dirty_since: Option<Instant>,
    toast: Option<(String, Instant)>,
    /// Small CPU copies of images for the color picker, by file name.
    samples: HashMap<String, image::RgbaImage>,
    picker: bool,
    /// The image being cropped, if crop mode is on.
    crop_mode: Option<u64>,
    /// The note or frame whose text is being edited.
    editing: Option<u64>,
    /// Its contents before editing (None for a just-created item), to skip no-op undo steps.
    edit_original: Option<Kind>,
    edit_focus_pending: bool,
    /// Screen area of the text editor, where the board ignores presses.
    edit_rect: Option<Rect>,
    show_help: bool,
    /// The OS window level is sent once on startup, then on each toggle.
    window_level_sent: bool,
    show_export: bool,
    exporting: bool,
    exported_tx: Sender<Exported>,
    exported_rx: Receiver<Exported>,
    palette_tx: Sender<Extracted>,
    palette_rx: Receiver<Extracted>,
    /// Bumped on every board switch, so late background results for the old board are dropped.
    generation: u64,
    show_boards: bool,
    new_board_name: String,
    recent: Vec<std::path::PathBuf>,
    clipboard: Option<arboard::Clipboard>,
}

impl CoreBoardApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, store: Store) -> Self {
        let (decoded_tx, decoded_rx) = channel();
        let (exported_tx, exported_rx) = channel();
        let (palette_tx, palette_rx) = channel();
        let board = store.load();
        let show_help = board.items.is_empty();
        Self {
            store,
            board,
            textures: HashMap::new(),
            decoded_tx,
            decoded_rx,
            selection: HashSet::new(),
            drag: Drag::None,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty_since: None,
            toast: None,
            samples: HashMap::new(),
            picker: false,
            crop_mode: None,
            editing: None,
            edit_original: None,
            edit_focus_pending: false,
            edit_rect: None,
            show_help,
            window_level_sent: false,
            show_export: false,
            exporting: false,
            exported_tx,
            exported_rx,
            palette_tx,
            palette_rx,
            generation: 0,
            show_boards: false,
            new_board_name: String::new(),
            recent: Vec::new(),
            clipboard: arboard::Clipboard::new().ok(),
        }
    }

    // ---------------------------------------------------------------- coordinates

    fn to_screen(&self, canvas: Rect, world: Pos2) -> Pos2 {
        let v = &self.board.view;
        canvas.min + vec2(v.offset[0], v.offset[1]) + world.to_vec2() * v.zoom
    }

    fn to_world(&self, canvas: Rect, screen: Pos2) -> Pos2 {
        let v = &self.board.view;
        ((screen - canvas.min - vec2(v.offset[0], v.offset[1])) / v.zoom).to_pos2()
    }

    fn item_screen_rect(&self, canvas: Rect, item: &Item) -> Rect {
        let min = self.to_screen(canvas, pos2(item.pos[0], item.pos[1]));
        Rect::from_min_size(min, vec2(item.size[0], item.size[1]) * self.board.view.zoom)
    }

    fn handle_rect(item_rect: Rect) -> Rect {
        Rect::from_center_size(item_rect.right_bottom(), Vec2::splat(HANDLE_SIZE))
    }

    fn zoom_around(&mut self, canvas: Rect, anchor: Pos2, new_zoom: f32) {
        let new_zoom = new_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let world = self.to_world(canvas, anchor);
        let offset = (anchor - canvas.min) - world.to_vec2() * new_zoom;
        self.board.view = board::View {
            offset: [offset.x, offset.y],
            zoom: new_zoom,
        };
        self.touch();
    }

    /// Frames the selection (or everything, if nothing is selected).
    fn fit(&mut self, canvas: Rect) {
        let items: Vec<&Item> = if self.selection.is_empty() {
            self.board.items.iter().collect()
        } else {
            self.board
                .items
                .iter()
                .filter(|i| self.selection.contains(&i.id))
                .collect()
        };
        let Some(bounds) = items
            .iter()
            .map(|i| Rect::from_min_size(pos2(i.pos[0], i.pos[1]), vec2(i.size[0], i.size[1])))
            .reduce(|a, b| a.union(b))
        else {
            self.board.view = board::View::default();
            return;
        };
        let avail = canvas.size() * 0.9;
        let zoom = (avail.x / bounds.width())
            .min(avail.y / bounds.height())
            .clamp(MIN_ZOOM, MAX_ZOOM);
        let offset = canvas.size() * 0.5 - bounds.center().to_vec2() * zoom;
        self.board.view = board::View {
            offset: [offset.x, offset.y],
            zoom,
        };
        self.touch();
    }

    // ---------------------------------------------------------------- edits

    fn touch(&mut self) {
        self.dirty_since.get_or_insert_with(Instant::now);
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.board.items.clone());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self) {
        if let Some(items) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.board.items, items));
            self.after_history_change();
        }
    }

    fn redo(&mut self) {
        if let Some(items) = self.redo.pop() {
            self.undo
                .push(std::mem::replace(&mut self.board.items, items));
            self.after_history_change();
        }
    }

    fn after_history_change(&mut self) {
        let ids: HashSet<u64> = self.board.items.iter().map(|i| i.id).collect();
        self.selection.retain(|id| ids.contains(id));
        self.touch();
    }

    fn delete_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        self.board.items.retain(|i| !self.selection.contains(&i.id));
        self.selection.clear();
        self.touch();
    }

    /// Items an action applies to: the selection, or everything when nothing is selected.
    fn targets(&self) -> HashSet<u64> {
        if self.selection.is_empty() {
            self.board.items.iter().map(|i| i.id).collect()
        } else {
            self.selection.clone()
        }
    }

    /// Rotates selected items by quarter turns (positive = clockwise) about their centers.
    fn rotate_selection(&mut self, quarters: i32) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        for item in &mut self.board.items {
            if !self.selection.contains(&item.id) || !item.is_image() {
                continue;
            }
            // Rotation is applied before the flip, so mirrored images turn the other way.
            let turn = if item.flip_x { -quarters } else { quarters };
            item.rotation = (item.rotation as i32 + turn).rem_euclid(4) as u8;
            if quarters % 2 != 0 {
                let [w, h] = item.size;
                item.pos = [item.pos[0] + (w - h) / 2.0, item.pos[1] + (h - w) / 2.0];
                item.size = [h, w];
            }
        }
        self.touch();
    }

    /// Sets `flag` on every selected item, or clears it if all of them already have it.
    fn toggle_selection(&mut self, flag: fn(&mut Item) -> &mut bool) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        let selection = &self.selection;
        let mut picked: Vec<&mut Item> = self
            .board
            .items
            .iter_mut()
            .filter(|i| selection.contains(&i.id) && i.is_image())
            .collect();
        if picked.is_empty() {
            return;
        }
        let all_on = picked.iter_mut().all(|i| *flag(i));
        for item in picked {
            *flag(item) = !all_on;
        }
        self.touch();
    }

    /// Steps the study view of the selection, starting from the first selected item's view.
    fn cycle_analysis(&mut self, forward: bool) {
        let Some(current) = self.first_selected().map(|i| i.analysis) else {
            self.toast("Select images first".into());
            return;
        };
        let next = board::cycle(&Analysis::CYCLE, current, forward);
        self.set_on_selection(|i| i.analysis = next);
        self.toast(next.label().into());
    }

    fn cycle_guide(&mut self) {
        let Some(current) = self.first_selected().map(|i| i.guide) else {
            self.toast("Select images first".into());
            return;
        };
        let next = board::cycle(&Guide::CYCLE, current, true);
        self.set_on_selection(|i| i.guide = next);
        self.toast(next.label().into());
    }

    fn turn_spiral(&mut self) {
        if self.first_selected().is_none() {
            self.toast("Select images first".into());
            return;
        }
        self.set_on_selection(|i| {
            i.guide = Guide::GoldenSpiral;
            i.guide_orientation = (i.guide_orientation + 1) % 4;
        });
        self.toast("Golden spiral turned".into());
    }

    /// The first selected image (study views and guides only apply to images).
    fn first_selected(&self) -> Option<&Item> {
        self.board
            .items
            .iter()
            .find(|i| self.selection.contains(&i.id) && i.is_image())
    }

    fn set_on_selection(&mut self, f: impl Fn(&mut Item)) {
        self.checkpoint();
        for item in &mut self.board.items {
            if self.selection.contains(&item.id) && item.is_image() {
                f(item);
            }
        }
        self.touch();
    }

    /// Original color under `pos`, mapped back through the item's rotation and flip.
    fn sample_at(&self, canvas: Rect, pos: Pos2) -> Option<[u8; 3]> {
        let id = self.hit(canvas, pos)?;
        let item = self.board.items.iter().find(|i| i.id == id)?;
        let rect = self.item_screen_rect(canvas, item);
        match &item.kind {
            Kind::Image => {}
            Kind::Palette { colors } => {
                return swatch_rects(rect, colors.len())
                    .zip(colors)
                    .find(|(r, _)| r.contains(pos))
                    .map(|(_, &c)| c);
            }
            Kind::Note { .. } | Kind::Frame { .. } => return None,
        }
        let sample = self.samples.get(&item.file)?;
        let s = (pos - rect.min) / rect.size();
        let [x0, y0, x1, y1] = item.display_crop();
        let (u, v) = item.display_to_tex((x0 + s.x * (x1 - x0), y0 + s.y * (y1 - y0)));
        let x = ((u * sample.width() as f32) as u32).min(sample.width() - 1);
        let y = ((v * sample.height() as f32) as u32).min(sample.height() - 1);
        let [r, g, b, _] = sample.get_pixel(x, y).0;
        Some([r, g, b])
    }

    fn paint_picker(&self, painter: &egui::Painter, canvas: Rect, pos: Pos2) {
        let Some([r, g, b]) = self.sample_at(canvas, pos) else {
            return;
        };
        let (h, s, v) = filters::hsv(r, g, b);
        let l = filters::lightness(r, g, b);
        let text = format!(
            "#{r:02X}{g:02X}{b:02X}\nH {h:.0}°  S {:.0}%  V {:.0}%\nValue (L*) {l:.0}",
            s * 100.0,
            v * 100.0
        );
        let galley = painter.layout_no_wrap(text, FontId::monospace(13.0), Color32::WHITE);
        let swatch = Vec2::splat(galley.size().y);
        let size = vec2(swatch.x + 8.0 + galley.size().x, galley.size().y);
        // Sit below-right of the cursor, flipping inward near the canvas edges.
        let mut min = pos + vec2(18.0, 18.0);
        if min.x + size.x + 10.0 > canvas.max.x {
            min.x = pos.x - 18.0 - size.x;
        }
        if min.y + size.y + 10.0 > canvas.max.y {
            min.y = pos.y - 18.0 - size.y;
        }
        let bg = Rect::from_min_size(min, size).expand(8.0);
        painter.rect_filled(bg, CornerRadius::same(6), Color32::from_black_alpha(220));
        let swatch_rect = Rect::from_min_size(min, swatch);
        painter.rect_filled(
            swatch_rect,
            CornerRadius::same(3),
            Color32::from_rgb(r, g, b),
        );
        painter.rect_stroke(
            swatch_rect,
            CornerRadius::same(3),
            Stroke::new(1.0, Color32::from_gray(160)),
            StrokeKind::Outside,
        );
        painter.galley(min + vec2(swatch.x + 8.0, 0.0), galley, Color32::WHITE);
        // Small ring marking the exact pixel.
        painter.circle_stroke(pos, 5.0, Stroke::new(3.0, Color32::BLACK));
        painter.circle_stroke(pos, 5.0, Stroke::new(1.5, Color32::WHITE));
    }

    // ---------------------------------------------------------------- palettes

    /// Extracts the dominant colors of each selected image in the background; each palette
    /// lands under its image when ready.
    fn extract_palettes(&mut self, count: usize) {
        let sources: Vec<(u64, std::path::PathBuf, [f32; 4])> = self
            .board
            .items
            .iter()
            .filter(|i| self.selection.contains(&i.id) && i.is_image())
            .map(|i| (i.id, self.store.image_path(&i.file), i.crop))
            .collect();
        if sources.is_empty() {
            self.toast("Select images to extract colors from".into());
            return;
        }
        let generation = self.generation;
        for (id, path, crop) in sources {
            let tx = self.palette_tx.clone();
            std::thread::spawn(move || {
                let result = image::open(&path).map_err(|e| e.to_string()).map(|img| {
                    // Only the visible part of a cropped image counts.
                    let (w, h) = (img.width() as f32, img.height() as f32);
                    let [u0, v0, u1, v1] = crop;
                    let x = (u0 * w) as u32;
                    let y = (v0 * h) as u32;
                    let cw = (((u1 - u0) * w) as u32).max(1);
                    let ch = (((v1 - v0) * h) as u32).max(1);
                    filters::palette(&img.crop_imm(x, y, cw, ch).to_rgba8(), count)
                });
                let _ = tx.send((generation, id, result));
            });
        }
        self.toast("Extracting colors…".into());
    }

    fn receive_palettes(&mut self) {
        let mut first = true;
        while let Ok((generation, source, result)) = self.palette_rx.try_recv() {
            if generation != self.generation {
                continue;
            }
            let colors = match result {
                Ok(colors) if !colors.is_empty() => colors,
                Ok(_) => continue,
                Err(err) => {
                    self.toast(format!("Color extraction failed: {err}"));
                    continue;
                }
            };
            let Some(image) = self.board.items.iter().find(|i| i.id == source) else {
                continue;
            };
            // A strip as wide as the image, one square swatch per color, just below it.
            let swatch = image.size[0] / colors.len() as f32;
            let pos = [image.pos[0], image.pos[1] + image.size[1] + swatch * 0.15];
            let size = [image.size[0], swatch];
            if first {
                self.checkpoint();
                self.selection.clear();
                first = false;
            }
            let id = self.board.next_id;
            self.board.next_id += 1;
            self.board.items.push(Item {
                id,
                pos,
                size,
                kind: Kind::Palette { colors },
                ..Item::default()
            });
            self.selection.insert(id);
            self.touch();
        }
    }

    // ---------------------------------------------------------------- crop

    fn toggle_crop_mode(&mut self) {
        if self.crop_mode.take().is_some() {
            self.toast("Crop done".into());
            return;
        }
        let id = self.selection.iter().next().copied();
        let is_image = id
            .and_then(|id| self.board.items.iter().find(|i| i.id == id))
            .is_some_and(Item::is_image);
        if self.selection.len() != 1 || !is_image {
            self.toast("Select one image to crop".into());
            return;
        }
        self.crop_mode = id;
        self.toast("Drag over the image to crop · Enter/Esc when done · Shift+X resets".into());
    }

    /// Where the whole, uncropped image sits on screen for an item showing only its crop.
    fn full_image_rect(&self, canvas: Rect, item: &Item) -> Rect {
        let rect = self.item_screen_rect(canvas, item);
        let [x0, y0, x1, y1] = item.display_crop();
        let size = vec2(rect.width() / (x1 - x0), rect.height() / (y1 - y0));
        Rect::from_min_size(rect.min - vec2(x0 * size.x, y0 * size.y), size)
    }

    /// Crops the crop-mode image to `area` (screen space), keeping it in place on the board.
    fn apply_crop(&mut self, canvas: Rect, area: Rect) {
        let Some(id) = self.crop_mode else { return };
        let Some(item) = self.board.items.iter().find(|i| i.id == id) else {
            return;
        };
        let full = self.full_image_rect(canvas, item);
        let area = area.intersect(full);
        if area.width() < 4.0 || area.height() < 4.0 {
            return;
        }
        let min = self.to_world(canvas, area.min);
        let zoom = self.board.view.zoom;
        let n = |p: Pos2| ((p - full.min) / full.size()).to_pos2();
        let (a, b) = (n(area.min), n(area.max));
        self.checkpoint();
        if let Some(item) = self.board.items.iter_mut().find(|i| i.id == id) {
            item.set_display_crop([a.x, a.y, b.x, b.y]);
            item.pos = [min.x, min.y];
            item.size = [area.width() / zoom, area.height() / zoom];
        }
        self.touch();
    }

    /// Shows the whole image again, growing the item around its current crop.
    fn reset_crop(&mut self, canvas: Rect) {
        let fulls: Vec<(u64, Rect)> = self
            .board
            .items
            .iter()
            .filter(|i| self.selection.contains(&i.id) && i.is_cropped())
            .map(|i| (i.id, self.full_image_rect(canvas, i)))
            .collect();
        if fulls.is_empty() {
            return;
        }
        self.checkpoint();
        let zoom = self.board.view.zoom;
        for (id, full) in fulls {
            let min = self.to_world(canvas, full.min);
            if let Some(item) = self.board.items.iter_mut().find(|i| i.id == id) {
                item.crop = [0.0, 0.0, 1.0, 1.0];
                item.pos = [min.x, min.y];
                item.size = [full.width() / zoom, full.height() / zoom];
            }
        }
        self.toast("Crop reset".into());
        self.touch();
    }

    /// Packs items into rows, in reading order, starting at their current top-left corner.
    /// With `uniform`, every item is first scaled to the median height.
    fn arrange_grid(&mut self, uniform: bool) {
        const GAP: f32 = 16.0;
        let targets = self.targets();
        let mut picked: Vec<usize> = (0..self.board.items.len())
            .filter(|&i| {
                targets.contains(&self.board.items[i].id) && !self.board.items[i].is_frame()
            })
            .collect();
        if picked.is_empty() {
            return;
        }
        self.checkpoint();
        let items = &mut self.board.items;

        let origin = picked.iter().fold([f32::MAX, f32::MAX], |acc, &i| {
            [acc[0].min(items[i].pos[0]), acc[1].min(items[i].pos[1])]
        });
        picked.sort_by(|&a, &b| {
            let (a, b) = (&items[a], &items[b]);
            let ay = a.pos[1] + a.size[1] / 2.0;
            let by = b.pos[1] + b.size[1] / 2.0;
            ay.total_cmp(&by).then(a.pos[0].total_cmp(&b.pos[0]))
        });

        if uniform {
            let mut heights: Vec<f32> = picked.iter().map(|&i| items[i].size[1]).collect();
            heights.sort_by(f32::total_cmp);
            let target = heights[heights.len() / 2];
            for &i in &picked {
                let scale = target / items[i].size[1];
                items[i].size = [items[i].size[0] * scale, target];
            }
        }

        // Aim for a block with roughly the shape of a 16:10 screen.
        let area: f32 = picked
            .iter()
            .map(|&i| (items[i].size[0] + GAP) * (items[i].size[1] + GAP))
            .sum();
        let widest = picked.iter().map(|&i| items[i].size[0]).fold(0.0, f32::max);
        let row_width = (area * 1.6).sqrt().max(widest);

        let (mut x, mut y, mut row_height) = (0.0, 0.0, 0.0_f32);
        for &i in &picked {
            let [w, h] = items[i].size;
            if x > 0.0 && x + w > row_width {
                x = 0.0;
                y += row_height + GAP;
                row_height = 0.0;
            }
            items[i].pos = [origin[0] + x, origin[1] + y];
            x += w + GAP;
            row_height = row_height.max(h);
        }
        self.touch();
    }

    fn raise_selection(&mut self) {
        let (mut picked, rest): (Vec<Item>, Vec<Item>) = self
            .board
            .items
            .drain(..)
            .partition(|i| self.selection.contains(&i.id));
        self.board.items = rest;
        self.board.items.append(&mut picked);
    }

    /// Topmost item under a screen position. Frames sit behind everything and are only
    /// grabbed by their title bar, so their body stays free for box-selecting.
    fn hit(&self, canvas: Rect, pos: Pos2) -> Option<u64> {
        let items = &self.board.items;
        items
            .iter()
            .rev()
            .filter(|i| !i.is_frame())
            .find(|i| self.item_screen_rect(canvas, i).contains(pos))
            .or_else(|| {
                items
                    .iter()
                    .rev()
                    .filter(|i| i.is_frame())
                    .find(|i| self.frame_title_rect(canvas, i).contains(pos))
            })
            .map(|i| i.id)
    }

    fn frame_title_rect(&self, canvas: Rect, item: &Item) -> Rect {
        let rect = self.item_screen_rect(canvas, item);
        let height = (FRAME_TITLE_HEIGHT * self.board.view.zoom).min(rect.height());
        Rect::from_min_size(rect.min, vec2(rect.width(), height))
    }

    /// Items to draw, bottom to top: frames first, then everything else in stacking order.
    fn draw_order(&self) -> Vec<Item> {
        let (frames, rest): (Vec<&Item>, Vec<&Item>) =
            self.board.items.iter().partition(|i| i.is_frame());
        frames.into_iter().chain(rest).cloned().collect()
    }

    /// Unselected items lying fully inside a selected frame; they move with it.
    fn carried_by_selected_frames(&self) -> HashSet<u64> {
        let world_rect =
            |i: &Item| Rect::from_min_size(pos2(i.pos[0], i.pos[1]), vec2(i.size[0], i.size[1]));
        let frames: Vec<Rect> = self
            .board
            .items
            .iter()
            .filter(|i| i.is_frame() && self.selection.contains(&i.id))
            .map(world_rect)
            .collect();
        self.board
            .items
            .iter()
            .filter(|i| !self.selection.contains(&i.id))
            .filter(|i| frames.iter().any(|f| f.contains_rect(world_rect(i))))
            .map(|i| i.id)
            .collect()
    }

    // ---------------------------------------------------------------- notes and frames

    fn add_note(&mut self, canvas: Rect, at: Pos2) {
        let world = self.to_world(canvas, at);
        self.checkpoint();
        let id = self.board.next_id;
        self.board.next_id += 1;
        self.board.items.push(Item {
            id,
            pos: [world.x, world.y],
            size: [260.0, NOTE_FONT * 1.3 + 2.0 * NOTE_PAD],
            kind: Kind::Note {
                text: String::new(),
                color: 0,
            },
            ..Item::default()
        });
        self.selection = HashSet::from([id]);
        self.begin_edit(id, true);
    }

    /// Wraps the selection in a titled frame (or adds an empty one at `at`).
    fn add_frame(&mut self, canvas: Rect, at: Pos2) {
        const PAD: f32 = 24.0;
        let bounds = self
            .board
            .items
            .iter()
            .filter(|i| self.selection.contains(&i.id))
            .map(|i| Rect::from_min_size(pos2(i.pos[0], i.pos[1]), vec2(i.size[0], i.size[1])))
            .reduce(|a, b| a.union(b));
        let rect = match bounds {
            Some(b) => Rect::from_min_max(
                b.min - vec2(PAD, PAD + FRAME_TITLE_HEIGHT),
                b.max + vec2(PAD, PAD),
            ),
            None => Rect::from_min_size(self.to_world(canvas, at), vec2(640.0, 420.0)),
        };
        self.checkpoint();
        let id = self.board.next_id;
        self.board.next_id += 1;
        self.board.items.push(Item {
            id,
            pos: [rect.min.x, rect.min.y],
            size: [rect.width(), rect.height()],
            kind: Kind::Frame {
                title: "Untitled".into(),
            },
            ..Item::default()
        });
        self.selection = HashSet::from([id]);
        self.begin_edit(id, true);
    }

    fn cycle_note_color(&mut self) {
        let any = self
            .board
            .items
            .iter()
            .any(|i| self.selection.contains(&i.id) && matches!(i.kind, Kind::Note { .. }));
        if !any {
            return;
        }
        self.checkpoint();
        for item in &mut self.board.items {
            if self.selection.contains(&item.id)
                && let Kind::Note { color, .. } = &mut item.kind
            {
                *color = (*color + 1) % NOTE_COLORS.len() as u8;
            }
        }
        self.touch();
    }

    /// Starts editing a note's text or a frame's title. `created` means the item was just
    /// added, so its creation is already on the undo stack.
    fn begin_edit(&mut self, id: u64, created: bool) {
        self.finish_edit();
        let Some(item) = self.board.items.iter().find(|i| i.id == id) else {
            return;
        };
        if !matches!(item.kind, Kind::Note { .. } | Kind::Frame { .. }) {
            return;
        }
        self.edit_original = if created {
            None
        } else {
            let kind = item.kind.clone();
            self.checkpoint();
            Some(kind)
        };
        self.editing = Some(id);
        self.edit_focus_pending = true;
    }

    fn finish_edit(&mut self) {
        let Some(id) = self.editing.take() else {
            return;
        };
        self.edit_rect = None;
        let original = self.edit_original.take();
        let Some(index) = self.board.items.iter().position(|i| i.id == id) else {
            return;
        };
        let item = &self.board.items[index];
        let empty_note = matches!(&item.kind, Kind::Note { text, .. } if text.trim().is_empty());
        match original {
            // A new note left empty was never really there.
            None if empty_note => {
                self.board.items.remove(index);
                self.undo.pop();
                self.selection.remove(&id);
            }
            None => {}
            Some(kind) if kind == item.kind => {
                self.undo.pop();
            }
            // Clearing a note's text deletes it (undo brings it back).
            Some(_) if empty_note => {
                self.board.items.remove(index);
                self.selection.remove(&id);
            }
            Some(_) => {}
        }
        self.touch();
    }

    /// The text editor drawn over the note or frame title being edited.
    fn edit_overlay(&mut self, ctx: &egui::Context, canvas: Rect) {
        let Some(id) = self.editing else { return };
        let Some(index) = self.board.items.iter().position(|i| i.id == id) else {
            self.editing = None;
            self.edit_rect = None;
            return;
        };
        let zoom = self.board.view.zoom;
        let item = &self.board.items[index];
        let rect = self.item_screen_rect(canvas, item);
        let title = self.frame_title_rect(canvas, item);
        let (area, font, ink, multiline) = match &item.kind {
            Kind::Note { color, .. } => {
                let [r, g, b] = NOTE_COLORS[*color as usize % NOTE_COLORS.len()].1;
                (
                    rect.shrink(NOTE_PAD * zoom),
                    NOTE_FONT * zoom,
                    Color32::from_rgb(r, g, b),
                    true,
                )
            }
            Kind::Frame { .. } => {
                let font = FRAME_FONT * zoom;
                let inset = vec2(10.0 * zoom, ((title.height() - font * 1.25) / 2.0).max(0.0));
                (title.shrink2(inset), font, Color32::from_gray(235), false)
            }
            _ => {
                self.editing = None;
                return;
            }
        };
        let focus = std::mem::take(&mut self.edit_focus_pending);
        let mut changed = false;
        let mut done = false;
        let mut editor_rect = area;
        let item = &mut self.board.items[index];
        let text = match &mut item.kind {
            Kind::Note { text, .. } => text,
            Kind::Frame { title } => title,
            _ => return,
        };
        egui::Area::new(egui::Id::unique("coreboard-text-editor"))
            .fixed_pos(area.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let edit = if multiline {
                    egui::TextEdit::multiline(text)
                } else {
                    egui::TextEdit::singleline(text)
                };
                let response = ui.add(
                    edit.font(FontId::proportional(font.max(4.0)))
                        .text_color(ink)
                        .frame(egui::Frame::NONE)
                        .margin(0.0)
                        .desired_width(area.width().max(20.0))
                        .desired_rows(1),
                );
                if focus {
                    response.request_focus();
                    if !multiline {
                        // Select the placeholder title so typing replaces it.
                        let mut state =
                            egui::TextEdit::load_state(ui.ctx(), response.id).unwrap_or_default();
                        let all = egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(text.chars().count()),
                        );
                        state.cursor.set_char_range(Some(all));
                        state.store(ui.ctx(), response.id);
                    }
                }
                changed = response.changed();
                done = !focus && !response.has_focus();
                editor_rect = response.rect.union(area);
            });
        self.edit_rect = Some(editor_rect);
        if changed {
            self.touch();
        }
        if done {
            self.finish_edit();
        }
    }

    fn hit_handle(&self, canvas: Rect, pos: Pos2) -> Option<u64> {
        self.board
            .items
            .iter()
            .rev()
            .filter(|i| self.selection.contains(&i.id))
            .find(|i| Self::handle_rect(self.item_screen_rect(canvas, i)).contains(pos))
            .map(|i| i.id)
    }

    // ---------------------------------------------------------------- import

    /// Adds images to the board, cascading them from `at` (screen space).
    fn import_all(&mut self, canvas: Rect, at: Pos2, sources: Vec<(Vec<u8>, Option<&Path>)>) {
        let mut added = 0;
        let mut failed = 0;
        let mut cursor = self.to_world(canvas, at);
        let max_screen_side = canvas.width().min(canvas.height()) * 0.5;

        for (bytes, path) in sources {
            let Some(ext) = board::image_ext(&bytes, path) else {
                failed += 1;
                continue;
            };
            let Ok((w, h)) = image_dimensions(&bytes) else {
                failed += 1;
                continue;
            };
            let file = match self.store.import(&bytes, &ext) {
                Ok(file) => file,
                Err(err) => {
                    eprintln!("coreboard: import failed: {err}");
                    failed += 1;
                    continue;
                }
            };
            if added == 0 {
                self.checkpoint();
                self.selection.clear();
            }
            let zoom = self.board.view.zoom;
            let mut size = vec2(w as f32, h as f32);
            let longest = size.max_elem() * zoom;
            if longest > max_screen_side {
                size *= max_screen_side / longest;
            }
            let id = self.board.next_id;
            self.board.next_id += 1;
            self.board.items.push(Item {
                id,
                file,
                pos: [cursor.x - size.x / 2.0, cursor.y - size.y / 2.0],
                size: [size.x, size.y],
                ..Item::default()
            });
            self.selection.insert(id);
            cursor += Vec2::splat(24.0 / zoom);
            added += 1;
        }

        if added > 0 {
            self.touch();
        }
        if failed > 0 {
            self.toast(format!("{failed} file(s) were not images I can read"));
        }
    }

    fn paste(&mut self, canvas: Rect, at: Pos2, pasted_text: Option<String>) {
        if let Some(clipboard) = &mut self.clipboard
            && let Ok(img) = clipboard.get_image()
        {
            let mut png = Vec::new();
            let encoded = image::RgbaImage::from_raw(
                img.width as u32,
                img.height as u32,
                img.bytes.into_owned(),
            )
            .map(|rgba| {
                rgba.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            });
            if let Some(Ok(())) = encoded {
                self.import_all(canvas, at, vec![(png, None)]);
                return;
            }
        }
        // Copying files in a file manager usually puts their paths/URIs on the clipboard as text.
        let text = pasted_text.or_else(|| self.clipboard.as_mut()?.get_text().ok());
        let paths: Vec<_> = text
            .iter()
            .flat_map(|t| t.lines())
            .map(|l| l.trim().trim_start_matches("file://"))
            .filter(|l| !l.is_empty())
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_file())
            .collect();
        if paths.is_empty() {
            self.toast("Clipboard has no image".into());
            return;
        }
        let sources = paths
            .iter()
            .filter_map(|p| Some((std::fs::read(p).ok()?, Some(p.as_path()))))
            .collect();
        self.import_all(canvas, at, sources);
    }

    fn toast(&mut self, msg: String) {
        self.toast = Some((msg, Instant::now()));
    }

    // ---------------------------------------------------------------- textures

    /// Each filter gets its own cached texture, so switching views back and forth is instant.
    fn texture(&mut self, ctx: &egui::Context, file: &str, filter: Filter) -> &Texture {
        let key = filter.key(file);
        if !self.textures.contains_key(&key) {
            self.textures.insert(key.clone(), Texture::Loading);
            let path = self.store.image_path(file);
            let tx = self.decoded_tx.clone();
            let ctx = ctx.clone();
            let (key, file) = (key.clone(), file.to_string());
            std::thread::spawn(move || {
                let _ = tx.send((key, file, decode(&path, filter)));
                ctx.request_repaint();
            });
        }
        &self.textures[&key]
    }

    fn receive_textures(&mut self, ctx: &egui::Context) {
        let options = TextureOptions {
            mipmap_mode: Some(egui::TextureFilter::Linear),
            ..TextureOptions::LINEAR
        };
        while let Ok((key, file, result)) = self.decoded_rx.try_recv() {
            let tex = match result {
                Ok((img, sample)) => {
                    if let Some(sample) = sample {
                        self.samples.insert(file, sample);
                    }
                    Texture::Ready(ctx.load_texture(&key, img, options))
                }
                Err(err) => Texture::Failed(err),
            };
            self.textures.insert(key, tex);
        }
    }

    // ---------------------------------------------------------------- export

    fn export_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_export {
            return;
        }
        let count = if self.selection.is_empty() {
            format!("all {} images", self.board.items.len())
        } else {
            format!("{} selected image(s)", self.selection.len())
        };
        let mut export = false;
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        egui::Window::new("Export PNG")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("Exports {count} as one PNG."));
                ui.add_space(6.0);
                let wm = &mut self.board.watermark;
                ui.label("Watermark text (leave empty for none):");
                let field = ui.add(
                    egui::TextEdit::singleline(&mut wm.text)
                        .hint_text("e.g. © Your Name")
                        .desired_width(260.0),
                );
                if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    export = true;
                }
                ui.horizontal(|ui| {
                    ui.label("Position");
                    egui::ComboBox::from_id_salt("watermark-position")
                        .selected_text(wm.position.label())
                        .show_ui(ui, |ui| {
                            for pos in WatermarkPosition::ALL {
                                ui.selectable_value(&mut wm.position, pos, pos.label());
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Opacity");
                    let mut percent = (wm.opacity * 100.0).round();
                    ui.add(egui::Slider::new(&mut percent, 10.0..=100.0).suffix("%"));
                    wm.opacity = percent / 100.0;
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let label = if self.exporting {
                        "Exporting…"
                    } else {
                        "Export"
                    };
                    export |= ui
                        .add_enabled(!self.exporting, egui::Button::new(label))
                        .clicked();
                    close |= ui.button("Cancel").clicked();
                });
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("Saved to {}", self.export_dir().display()))
                            .small()
                            .weak(),
                    );
                    if ui.small_button("Change…").clicked()
                        && let Some(dir) = rfd::FileDialog::new()
                            .set_title("Folder for exports")
                            .set_directory(self.export_dir())
                            .pick_folder()
                    {
                        self.board.export_dir = Some(dir);
                    }
                    if self.board.export_dir.is_some() && ui.small_button("Reset").clicked() {
                        self.board.export_dir = None;
                    }
                });
            });

        if export && !self.exporting {
            self.start_export();
            close = true;
        }
        if close {
            self.show_export = false;
            // Keep the watermark settings for next time.
            self.touch();
        }
    }

    fn start_export(&mut self) {
        let targets = self.targets();
        let items: Vec<Item> = self
            .board
            .items
            .iter()
            .filter(|i| targets.contains(&i.id))
            .cloned()
            .collect();
        let images_dir = self.store.dir.join("images");
        let out_dir = self.export_dir();
        let watermark = self.board.watermark.clone();
        let tx = self.exported_tx.clone();
        self.exporting = true;
        self.toast("Exporting…".into());
        std::thread::spawn(move || {
            let _ = tx.send(export::export_png(
                &images_dir,
                &out_dir,
                &items,
                &watermark,
            ));
        });
    }

    fn export_dir(&self) -> std::path::PathBuf {
        self.board
            .export_dir
            .clone()
            .unwrap_or_else(|| self.store.dir.join("exports"))
    }

    fn receive_exports(&mut self) {
        if let Ok(result) = self.exported_rx.try_recv() {
            self.exporting = false;
            match result {
                Ok(path) => self.toast(format!("Exported {}", path.display())),
                Err(err) => self.toast(format!("Export failed: {err}")),
            }
        }
    }

    // ---------------------------------------------------------------- boards

    fn open_boards_window(&mut self) {
        self.finish_edit();
        self.recent = boards::recent();
        self.show_boards = true;
    }

    fn boards_window(&mut self, ctx: &egui::Context) {
        if !self.show_boards {
            return;
        }
        let mut open: Option<std::path::PathBuf> = None;
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let current = self
            .store
            .dir
            .canonicalize()
            .unwrap_or_else(|_| self.store.dir.clone());
        egui::Window::new("Boards")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(boards::display_name(&current)).strong());
                ui.label(
                    egui::RichText::new(current.display().to_string())
                        .small()
                        .weak(),
                );
                ui.separator();

                ui.label("New board");
                ui.horizontal(|ui| {
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.new_board_name)
                            .hint_text("Name, e.g. Character refs")
                            .desired_width(220.0),
                    );
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    let name = boards::folder_name(&self.new_board_name);
                    if (ui
                        .add_enabled(name.is_some(), egui::Button::new("Create"))
                        .clicked()
                        || enter)
                        && let Some(name) = name
                    {
                        open = Some(boards::boards_root().join(name));
                    }
                });
                if ui.button("Open or create in a folder…").clicked()
                    && let Some(dir) = rfd::FileDialog::new()
                        .set_title("Board folder")
                        .pick_folder()
                {
                    open = Some(dir);
                }
                ui.separator();

                ui.label("Recent");
                let others: Vec<_> = self.recent.iter().filter(|p| **p != current).collect();
                if others.is_empty() {
                    ui.label(egui::RichText::new("No other boards yet").weak());
                }
                for dir in others {
                    let response = ui
                        .button(boards::display_name(dir))
                        .on_hover_text(dir.display().to_string());
                    if response.clicked() {
                        open = Some(dir.clone());
                    }
                }
                ui.add_space(6.0);
                close |= ui.button("Close").clicked();
            });
        if let Some(dir) = open {
            self.switch_board(ctx, dir);
            close = true;
        }
        if close {
            self.show_boards = false;
        }
    }

    /// Saves the current board and opens (or creates) the board in `dir`.
    fn switch_board(&mut self, ctx: &egui::Context, dir: std::path::PathBuf) {
        let store = match Store::open(dir.clone()) {
            Ok(store) => store,
            Err(err) => {
                self.toast(format!("Can't open {}: {err}", dir.display()));
                return;
            }
        };
        self.finish_edit();
        self.save_now();
        self.store.collect_garbage(&self.board);

        self.board = store.load();
        self.store = store;
        self.generation += 1;
        self.textures.clear();
        self.samples.clear();
        self.selection.clear();
        self.undo.clear();
        self.redo.clear();
        self.drag = Drag::None;
        self.crop_mode = None;
        self.dirty_since = None;
        self.show_help = self.board.items.is_empty();
        self.window_level_sent = false;
        self.new_board_name.clear();
        boards::remember(&self.store.dir);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "Core Board — {}",
            self.store.dir.display()
        )));
        self.toast(format!("Opened {}", boards::display_name(&self.store.dir)));
    }

    // ---------------------------------------------------------------- persistence

    fn save_now(&mut self) {
        match self.store.save(&self.board) {
            Ok(()) => self.dirty_since = None,
            Err(err) => self.toast(format!("Save failed: {err}")),
        }
    }

    // ---------------------------------------------------------------- frame

    fn handle_input(&mut self, ui: &egui::Ui, canvas: Rect) {
        let ctx = ui.ctx().clone();
        let (pointer, modifiers, events, scroll, pinch, dropped) = ctx.input_mut(|i| {
            (
                i.pointer.clone(),
                i.modifiers,
                i.events.clone(),
                i.smooth_scroll_delta,
                i.zoom_delta(),
                std::mem::take(&mut i.raw.dropped_files),
            )
        });
        let hover = pointer.hover_pos().filter(|p| canvas.contains(*p));
        let anchor = hover.unwrap_or(canvas.center());

        // Files dropped from the OS.
        if !dropped.is_empty() {
            let mut sources = Vec::new();
            for file in &dropped {
                match file.bytes() {
                    Ok(bytes) => sources.push((bytes, Some(file.path()))),
                    Err(err) => eprintln!("coreboard: {}: {err}", file.path().display()),
                }
            }
            self.import_all(canvas, anchor, sources);
        }

        // Keyboard, unless a text field (note, frame title, dialog) is taking typing.
        let mut paste_requested = None;
        let typing = ctx.egui_wants_keyboard_input();
        for event in events.iter().filter(|_| !typing) {
            match event {
                Event::Paste(text) => paste_requested = Some(Some(text.clone())),
                Event::Key {
                    key: Key::V,
                    pressed: true,
                    modifiers: m,
                    ..
                } if m.command => {
                    paste_requested.get_or_insert(None);
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers: m,
                    ..
                } => match key {
                    Key::Delete | Key::Backspace => self.delete_selection(),
                    Key::A if m.command => {
                        self.selection = self.board.items.iter().map(|i| i.id).collect();
                    }
                    Key::Z if m.command && m.shift => self.redo(),
                    Key::Z if m.command => self.undo(),
                    Key::Y if m.command => self.redo(),
                    Key::E if m.command => self.show_export = !self.board.items.is_empty(),
                    Key::O if m.command => self.open_boards_window(),
                    Key::S if m.command => {
                        self.save_now();
                        self.toast("Saved".into());
                    }
                    Key::F | Key::Home => self.fit(canvas),
                    Key::R if !m.command => self.rotate_selection(if m.shift { -1 } else { 1 }),
                    Key::G if !m.command => self.arrange_grid(m.shift),
                    Key::M if !m.command => self.toggle_selection(|i| &mut i.flip_x),
                    Key::B if !m.command => self.toggle_selection(|i| &mut i.grayscale),
                    Key::V if !m.command => self.cycle_analysis(!m.shift),
                    Key::C if !m.command && m.shift => self.turn_spiral(),
                    Key::C if !m.command => self.cycle_guide(),
                    Key::P if !m.command => self.extract_palettes(if m.shift { 10 } else { 6 }),
                    Key::N if !m.command && m.shift => self.cycle_note_color(),
                    Key::N if !m.command => self.add_note(canvas, anchor),
                    Key::G if m.command => self.add_frame(canvas, anchor),
                    Key::X if !m.command && m.shift => self.reset_crop(canvas),
                    Key::X if !m.command => self.toggle_crop_mode(),
                    Key::Enter if self.crop_mode.is_some() => self.toggle_crop_mode(),
                    Key::Escape if self.crop_mode.is_some() => self.toggle_crop_mode(),
                    Key::I if !m.command => {
                        self.picker = !self.picker;
                        let state = if self.picker {
                            "on: hover an image"
                        } else {
                            "off"
                        };
                        self.toast(format!("Color picker {state}"));
                    }
                    Key::T if !m.command => {
                        let on = !self.board.window.always_on_top;
                        self.board.window.always_on_top = on;
                        self.window_level_sent = false;
                        self.toast(format!("Always on top: {}", if on { "on" } else { "off" }));
                        self.touch();
                    }
                    Key::OpenBracket | Key::CloseBracket => {
                        let step = if *key == Key::OpenBracket { -0.1 } else { 0.1 };
                        // Round to whole steps so repeated presses land on 10%, 20%, ...
                        let opacity = ((self.board.window.opacity + step) * 10.0).round() / 10.0;
                        self.board.window.opacity = opacity.clamp(0.1, 1.0);
                        self.toast(format!("Opacity {:.0}%", self.board.window.opacity * 100.0));
                        self.touch();
                    }
                    Key::Num1 if !m.command => self.zoom_around(canvas, anchor, 1.0),
                    Key::H | Key::F1 => self.show_help = !self.show_help,
                    Key::Escape => self.selection.clear(),
                    _ => {}
                },
                _ => {}
            }
        }
        if let Some(text) = paste_requested {
            self.paste(canvas, anchor, text);
        }

        // Zoom: wheel or trackpad pinch, anchored at the cursor.
        if let Some(pos) = hover {
            let factor = (scroll.y * 0.0025).exp() * pinch;
            if (factor - 1.0).abs() > f32::EPSILON {
                self.zoom_around(canvas, pos, self.board.view.zoom * factor);
            }
        }

        // Pointer presses start a drag mode. Read from raw events rather than pointer state,
        // which forgets a press released within the same frame (a quick click or trackpad tap).
        let press = events.iter().find_map(|e| match e {
            Event::PointerButton {
                pos,
                button,
                pressed: true,
                ..
            } if canvas.contains(*pos) && !self.edit_rect.is_some_and(|r| r.contains(*pos)) => {
                Some((*pos, *button))
            }
            _ => None,
        });
        if let Some((pos, button)) = press
            && matches!(self.drag, Drag::None)
        {
            let space = ctx.input(|i| i.key_down(Key::Space));
            self.drag = if button != egui::PointerButton::Primary || space {
                Drag::Pan
            } else if self.crop_mode.is_some() {
                Drag::Crop { start: pos }
            } else if let Some(id) = self.hit_handle(canvas, pos) {
                let item = self.board.items.iter().find(|i| i.id == id).unwrap();
                let aspect = match item.kind {
                    Kind::Image | Kind::Palette { .. } => Some(item.size[0] / item.size[1]),
                    Kind::Note { .. } | Kind::Frame { .. } => None,
                };
                self.checkpoint();
                Drag::Resize { id, aspect }
            } else if let Some(id) = self.hit(canvas, pos) {
                if modifiers.shift || modifiers.command {
                    if !self.selection.remove(&id) {
                        self.selection.insert(id);
                    }
                } else if !self.selection.contains(&id) {
                    self.selection.clear();
                    self.selection.insert(id);
                }
                self.checkpoint();
                self.raise_selection();
                Drag::Move {
                    moved: false,
                    carried: self.carried_by_selected_frames(),
                }
            } else if modifiers.shift || modifiers.command {
                Drag::Marquee {
                    start: pos,
                    additive: true,
                }
            } else {
                self.selection.clear();
                Drag::Marquee {
                    start: pos,
                    additive: false,
                }
            };
        }

        // Continue the drag. Motion in the press frame happened before the press, so skip it.
        let delta = if press.is_some() {
            Vec2::ZERO
        } else {
            pointer.delta()
        };
        let zoom = self.board.view.zoom;
        match &mut self.drag {
            Drag::None | Drag::Marquee { .. } | Drag::Crop { .. } => {}
            Drag::Pan => {
                self.board.view.offset[0] += delta.x;
                self.board.view.offset[1] += delta.y;
                if delta != Vec2::ZERO {
                    self.touch();
                }
            }
            Drag::Move { moved, carried } => {
                if delta != Vec2::ZERO {
                    *moved = true;
                    let d = delta / zoom;
                    for item in &mut self.board.items {
                        if self.selection.contains(&item.id) || carried.contains(&item.id) {
                            item.pos[0] += d.x;
                            item.pos[1] += d.y;
                        }
                    }
                    self.touch();
                }
            }
            Drag::Resize { id, aspect } => {
                let (id, aspect) = (*id, *aspect);
                if let Some(p) = pointer.hover_pos() {
                    let world = self.to_world(canvas, p);
                    let min_w = 16.0 / zoom;
                    if let Some(item) = self.board.items.iter_mut().find(|i| i.id == id) {
                        let w = world.x - item.pos[0];
                        let h = world.y - item.pos[1];
                        item.size = match (aspect, &item.kind) {
                            (Some(aspect), _) => {
                                let w = w.max(h * aspect).max(min_w);
                                [w, w / aspect]
                            }
                            // A note's height follows its text.
                            (None, Kind::Note { .. }) => [w.max(80.0), item.size[1]],
                            (None, _) => [w.max(160.0), h.max(FRAME_TITLE_HEIGHT + 40.0)],
                        };
                    }
                    self.touch();
                }
            }
        }

        // Finish the drag.
        if !pointer.any_down() {
            match std::mem::replace(&mut self.drag, Drag::None) {
                Drag::Move { moved: false, .. } => {
                    // A plain click: drop the no-op history entry and collapse to one item.
                    self.undo.pop();
                    if !(modifiers.shift || modifiers.command)
                        && let Some(id) = pointer.interact_pos().and_then(|p| self.hit(canvas, p))
                    {
                        self.selection = HashSet::from([id]);
                    }
                }
                Drag::Crop { start } => {
                    if let Some(end) = pointer.interact_pos() {
                        let area = Rect::from_two_pos(start, end);
                        if area.width() < 4.0 && area.height() < 4.0 {
                            // A click instead of a drag: finish cropping.
                            self.toggle_crop_mode();
                        } else {
                            self.apply_crop(canvas, area);
                        }
                    }
                }
                Drag::Marquee { start, additive } => {
                    if let Some(end) = pointer.interact_pos() {
                        let area = Rect::from_two_pos(start, end);
                        if !additive {
                            self.selection.clear();
                        }
                        for item in &self.board.items {
                            let rect = self.item_screen_rect(canvas, item);
                            // Frames are big; only select them when boxed in completely.
                            let hit = if item.is_frame() {
                                area.contains_rect(rect)
                            } else {
                                area.intersects(rect)
                            };
                            if hit {
                                self.selection.insert(item.id);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Double-click a note, or a frame's title, to edit its text.
        if pointer.button_double_clicked(egui::PointerButton::Primary)
            && let Some(pos) = pointer.interact_pos().filter(|p| canvas.contains(*p))
            && !self.edit_rect.is_some_and(|r| r.contains(pos))
            && let Some(id) = self.hit(canvas, pos)
        {
            self.begin_edit(id, false);
        }
    }

    fn paint(&mut self, ui: &egui::Ui, canvas: Rect) {
        let ctx = ui.ctx().clone();
        let painter = ui.painter_at(canvas);
        // Opacity fades the board itself; labels and overlays stay fully readable.
        let opacity = self.board.window.opacity;
        let tint = Color32::WHITE.gamma_multiply(opacity);
        painter.rect_filled(canvas, CornerRadius::ZERO, BG.gamma_multiply(opacity));

        // Dot grid, skipped when it would be too dense to read.
        let zoom = self.board.view.zoom;
        let mut step = 64.0 * zoom;
        while step < 24.0 {
            step *= 4.0;
        }
        let origin = self.to_screen(canvas, Pos2::ZERO);
        let start_x = canvas.min.x + (origin.x - canvas.min.x).rem_euclid(step);
        let start_y = canvas.min.y + (origin.y - canvas.min.y).rem_euclid(step);
        let mut x = start_x;
        while x < canvas.max.x {
            let mut y = start_y;
            while y < canvas.max.y {
                painter.circle_filled(pos2(x, y), 1.2, GRID.gamma_multiply(opacity));
                y += step;
            }
            x += step;
        }

        // Items, bottom to top.
        if let Some(id) = self.crop_mode
            && !self.board.items.iter().any(|i| i.id == id)
        {
            self.crop_mode = None;
        }
        let items = self.draw_order();
        let mut note_heights = Vec::new();
        for item in &items {
            let rect = self.item_screen_rect(canvas, item);
            if let Kind::Note { text, .. } = &item.kind {
                // Fit the note's height to its text, measured at 100% so zoom doesn't matter.
                let wrap = (item.size[0] - 2.0 * NOTE_PAD).max(1.0);
                let galley = painter.layout(
                    text.clone(),
                    FontId::proportional(NOTE_FONT),
                    Color32::WHITE,
                    wrap,
                );
                let height = galley.size().y.max(NOTE_FONT * 1.3) + 2.0 * NOTE_PAD;
                if (height - item.size[1]).abs() > 0.5 {
                    note_heights.push((item.id, height));
                }
            }
            if !rect.intersects(canvas) {
                continue;
            }
            let selected = self.selection.contains(&item.id);
            let editing = self.editing == Some(item.id);
            match &item.kind {
                Kind::Image => {}
                Kind::Palette { colors } => {
                    draw_palette(&painter, rect, colors, opacity);
                    if selected {
                        draw_selection(&painter, rect);
                    }
                    continue;
                }
                Kind::Note { text, color } => {
                    draw_note(&painter, rect, text, *color, zoom, opacity, editing);
                    if selected {
                        draw_selection(&painter, rect);
                    }
                    continue;
                }
                Kind::Frame { title } => {
                    let title_rect = self.frame_title_rect(canvas, item);
                    draw_frame(&painter, rect, title_rect, title, zoom, opacity, editing);
                    if selected {
                        draw_selection(&painter, rect);
                    }
                    continue;
                }
            }
            if self.picker {
                // The picker reads original colors, whatever view the image is shown in.
                self.texture(&ctx, &item.file, Filter::Plain);
            }
            let cropping = self.crop_mode == Some(item.id);
            let full = self.full_image_rect(canvas, item);
            match self.texture(&ctx, &item.file, Filter::for_item(item)) {
                Texture::Ready(tex) => {
                    let tex = tex.id();
                    if cropping {
                        // The parts being cut away, dimmed, around the kept part.
                        let dim = Color32::from_gray(90).gamma_multiply(opacity);
                        let whole = Item {
                            crop: [0.0, 0.0, 1.0, 1.0],
                            ..item.clone()
                        };
                        painter.add(image_shape(tex, full, &whole, dim));
                    }
                    painter.add(image_shape(tex, rect, item, tint));
                }
                Texture::Loading => {
                    painter.rect_filled(
                        rect,
                        CornerRadius::ZERO,
                        Color32::from_gray(50).gamma_multiply(opacity),
                    );
                }
                Texture::Failed(err) => {
                    painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(70, 30, 30));
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        err,
                        FontId::proportional(12.0),
                        Color32::LIGHT_RED,
                    );
                }
            }
            if item.guide != Guide::None {
                draw_guide(&painter, rect, item.guide, item.guide_orientation);
            }
            if cropping {
                painter.rect_stroke(
                    full,
                    CornerRadius::ZERO,
                    Stroke::new(1.0, Color32::from_gray(150)),
                    StrokeKind::Outside,
                );
                painter.rect_stroke(
                    rect,
                    CornerRadius::ZERO,
                    Stroke::new(2.0, Color32::WHITE),
                    StrokeKind::Outside,
                );
                continue;
            }
            if self.selection.contains(&item.id) {
                draw_selection(&painter, rect);
            }
        }

        for (id, height) in note_heights {
            if let Some(item) = self.board.items.iter_mut().find(|i| i.id == id) {
                item.size[1] = height;
            }
            self.touch();
        }

        if self.picker
            && let Some(pos) = ctx.input(|i| i.pointer.hover_pos())
        {
            self.paint_picker(&painter, canvas, pos);
        }

        // Marquee or new crop rectangle.
        if let (Drag::Marquee { start, .. } | Drag::Crop { start }, Some(end)) =
            (&self.drag, ctx.input(|i| i.pointer.interact_pos()))
        {
            let area = Rect::from_two_pos(*start, end);
            painter.rect_filled(area, CornerRadius::ZERO, ACCENT.gamma_multiply(0.12));
            painter.rect_stroke(
                area,
                CornerRadius::ZERO,
                Stroke::new(1.0, ACCENT),
                StrokeKind::Inside,
            );
        }

        // Drop target highlight.
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            painter.rect_stroke(
                canvas.shrink(6.0),
                CornerRadius::same(8),
                Stroke::new(3.0, ACCENT),
                StrokeKind::Inside,
            );
        }

        if self.board.items.is_empty() {
            painter.text(
                canvas.center(),
                Align2::CENTER_CENTER,
                "Drop images here, paste with Ctrl+V, or press N for a note",
                FontId::proportional(22.0),
                Color32::from_gray(120),
            );
        }

        // Status line.
        let status = format!(
            "{:.0}%  ·  {} items{}{}{}",
            zoom * 100.0,
            self.board.items.len(),
            if opacity < 1.0 {
                format!("  ·  opacity {:.0}%", opacity * 100.0)
            } else {
                String::new()
            },
            if self.board.window.always_on_top {
                "  ·  on top"
            } else if self.crop_mode.is_some() {
                "  ·  CROP: drag over the image, Enter when done"
            } else {
                ""
            },
            if self.selection.is_empty() {
                String::new()
            } else {
                format!("  ·  {} selected", self.selection.len())
            },
        );
        painter.text(
            canvas.left_bottom() + vec2(10.0, -8.0),
            Align2::LEFT_BOTTOM,
            status,
            FontId::monospace(12.0),
            Color32::from_gray(140),
        );

        if self.show_help {
            let help = "Drag & drop / Ctrl+V   add images\n\
                        Scroll / pinch         zoom\n\
                        Right/middle/Space drag pan\n\
                        Drag empty space       box select\n\
                        Shift+click            toggle select\n\
                        Corner handle          resize\n\
                        R / Shift+R            rotate 90° cw / ccw\n\
                        G / Shift+G            grid arrange / same height\n\
                        M                      mirror (flip horizontal)\n\
                        B                      black & white\n\
                        [ / ]                  window opacity\n\
                        T                      always on top\n\
                        Ctrl+E                 export PNG (+ watermark)\n\
                        Ctrl+O                 boards: new, open, recent\n\
                        V / Shift+V            value, saturation, noise, squint\n\
                        C / Shift+C            composition guide / turn spiral\n\
                        I                      color picker (HSV + value)\n\
                        X / Shift+X            crop (drag) / reset crop\n\
                        P / Shift+P            extract 6 / 10 color palette\n\
                        N / Shift+N            new note / note color\n\
                        Ctrl+G                 frame around selection\n\
                        Double-click           edit note or frame title\n\
                        Del                    remove\n\
                        Ctrl+Z / Ctrl+Shift+Z  undo / redo\n\
                        F                      fit to view\n\
                        1                      zoom 100%\n\
                        H                      toggle this help";
            let galley = painter.layout_no_wrap(
                help.into(),
                FontId::monospace(12.0),
                Color32::from_gray(200),
            );
            let pos = canvas.right_top() + vec2(-galley.size().x - 18.0, 12.0);
            let bg = Rect::from_min_size(pos, galley.size()).expand(8.0);
            painter.rect_filled(bg, CornerRadius::same(6), Color32::from_black_alpha(180));
            painter.galley(pos, galley, Color32::WHITE);
        }

        if let Some((msg, at)) = &self.toast {
            if at.elapsed() < Duration::from_secs(3) {
                painter.text(
                    canvas.center_bottom() + vec2(0.0, -24.0),
                    Align2::CENTER_BOTTOM,
                    msg,
                    FontId::proportional(15.0),
                    Color32::WHITE,
                );
                ctx.request_repaint_after(Duration::from_millis(250));
            } else {
                self.toast = None;
            }
        }
    }
}

impl eframe::App for CoreBoardApp {
    /// Fully clear, so the board background alone decides how much desktop shows through.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.receive_textures(&ctx);

        let canvas = ui.max_rect();
        // Claim the pointer so egui treats the whole window as one draggable surface.
        ui.interact(
            canvas,
            ui.scope_id().with("canvas"),
            Sense::click_and_drag(),
        );
        // Dialogs are modal: the board ignores input while one is open.
        if !self.show_export && !self.show_boards {
            self.handle_input(ui, canvas);
        }
        self.paint(ui, canvas);
        self.edit_overlay(&ctx, canvas);
        self.export_dialog(&ctx);
        self.boards_window(&ctx);
        self.receive_exports();
        self.receive_palettes();

        if !self.window_level_sent {
            let level = if self.board.window.always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
            self.window_level_sent = true;
        }

        let closing = ctx.input(|i| i.viewport().close_requested());
        if closing {
            self.save_now();
            self.store.collect_garbage(&self.board);
        } else if let Some(since) = self.dirty_since {
            let wait = AUTOSAVE_DELAY.saturating_sub(since.elapsed());
            if wait.is_zero() {
                self.save_now();
            } else {
                ctx.request_repaint_after(wait);
            }
        }
    }
}

fn rgb([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

fn draw_note(
    painter: &egui::Painter,
    rect: Rect,
    text: &str,
    color: u8,
    zoom: f32,
    opacity: f32,
    editing: bool,
) {
    let (bg, ink) = NOTE_COLORS[color as usize % NOTE_COLORS.len()];
    let radius = CornerRadius::same((4.0 * zoom).clamp(1.0, 8.0) as u8);
    // Soft drop shadow, then the card.
    painter.rect_filled(
        rect.translate(vec2(0.0, 2.0 * zoom.min(2.0))),
        radius,
        Color32::from_black_alpha(70).gamma_multiply(opacity),
    );
    painter.rect_filled(rect, radius, rgb(bg).gamma_multiply(opacity));
    if editing {
        return; // The text editor draws the text.
    }
    let inner = rect.shrink(NOTE_PAD * zoom);
    let font = NOTE_FONT * zoom;
    if font < 3.0 {
        return; // Too small to read; the card alone shows where it is.
    }
    let (shown, ink) = if text.trim().is_empty() {
        ("Empty note", rgb(ink).gamma_multiply(0.45))
    } else {
        (text, rgb(ink))
    };
    let ink = ink.gamma_multiply(opacity);
    let galley = painter.layout(
        shown.to_string(),
        FontId::proportional(font),
        ink,
        inner.width().max(1.0),
    );
    painter
        .with_clip_rect(rect.intersect(painter.clip_rect()))
        .galley(inner.min, galley, ink);
}

fn draw_frame(
    painter: &egui::Painter,
    rect: Rect,
    title_rect: Rect,
    title: &str,
    zoom: f32,
    opacity: f32,
    editing: bool,
) {
    let r = (6.0 * zoom).clamp(1.0, 10.0) as u8;
    painter.rect_filled(
        rect,
        CornerRadius::same(r),
        Color32::from_white_alpha(10).gamma_multiply(opacity),
    );
    painter.rect_filled(
        title_rect,
        CornerRadius {
            nw: r,
            ne: r,
            sw: 0,
            se: 0,
        },
        Color32::from_white_alpha(22).gamma_multiply(opacity),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(r),
        Stroke::new(1.5, Color32::from_gray(95).gamma_multiply(opacity)),
        StrokeKind::Inside,
    );
    let font = FRAME_FONT * zoom;
    if editing || font < 4.0 {
        return;
    }
    let ink = Color32::from_gray(235);
    let galley = painter.layout_no_wrap(title.to_string(), FontId::proportional(font), ink);
    let pos = pos2(
        title_rect.min.x + 10.0 * zoom,
        title_rect.center().y - galley.size().y / 2.0,
    );
    painter
        .with_clip_rect(title_rect.intersect(painter.clip_rect()))
        .galley(pos, galley, ink);
}

/// Accent outline with a dark halo (visible on any color) and the resize handle.
fn draw_selection(painter: &egui::Painter, rect: Rect) {
    painter.rect_stroke(
        rect.expand(2.0),
        CornerRadius::ZERO,
        Stroke::new(2.0, Color32::BLACK),
        StrokeKind::Outside,
    );
    painter.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(2.0, ACCENT),
        StrokeKind::Outside,
    );
    painter.rect_filled(
        CoreBoardApp::handle_rect(rect),
        CornerRadius::same(2),
        ACCENT,
    );
}

/// Splits a palette's rect into equal swatches, along its longer side.
fn swatch_rects(rect: Rect, n: usize) -> impl Iterator<Item = Rect> {
    let horizontal = rect.width() >= rect.height();
    let step = if horizontal {
        vec2(rect.width() / n.max(1) as f32, 0.0)
    } else {
        vec2(0.0, rect.height() / n.max(1) as f32)
    };
    let size = if horizontal {
        vec2(step.x, rect.height())
    } else {
        vec2(rect.width(), step.y)
    };
    (0..n).map(move |i| Rect::from_min_size(rect.min + step * i as f32, size))
}

/// Swatches with their hex and perceptual value, when there's room to read them.
fn draw_palette(painter: &egui::Painter, rect: Rect, colors: &[[u8; 3]], opacity: f32) {
    for (r, &[cr, cg, cb]) in swatch_rects(rect, colors.len()).zip(colors) {
        painter.rect_filled(
            r,
            CornerRadius::ZERO,
            Color32::from_rgb(cr, cg, cb).gamma_multiply(opacity),
        );
        let side = r.width().min(r.height());
        if side >= 52.0 {
            let l = filters::lightness(cr, cg, cb);
            let ink = if l > 60.0 {
                Color32::from_black_alpha(200)
            } else {
                Color32::from_white_alpha(230)
            };
            painter.text(
                r.left_bottom() + vec2(6.0, -6.0),
                Align2::LEFT_BOTTOM,
                format!("#{cr:02X}{cg:02X}{cb:02X}\nL* {l:.0}"),
                FontId::monospace((side * 0.13).clamp(9.0, 14.0)),
                ink,
            );
        }
    }
}

/// An item's image drawn into `rect`: only its crop, turned by its clockwise quarter turns,
/// then optionally mirrored left-to-right.
fn image_shape(texture: egui::TextureId, rect: Rect, item: &Item, tint: Color32) -> egui::Shape {
    let (quarters, flip_x) = (item.rotation % 4, item.flip_x);
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    let [u0, v0, u1, v1] = item.crop;
    let uvs = [pos2(u0, v0), pos2(u1, v0), pos2(u1, v1), pos2(u0, v1)];
    let mut mesh = egui::Mesh::with_texture(texture);
    for (i, pos) in corners.into_iter().enumerate() {
        // Mirroring swaps left and right corners: 0<->1 (top), 2<->3 (bottom).
        let source = if flip_x { i ^ 1 } else { i };
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: uvs[(source + 4 - quarters as usize) % 4],
            color: tint,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    egui::Shape::mesh(mesh)
}

/// Draws a composition template over an image, light lines on a dark edge so they read
/// on any picture.
fn draw_guide(painter: &egui::Painter, rect: Rect, guide: Guide, orientation: u8) {
    let at = |fx: f32, fy: f32| rect.min + vec2(fx * rect.width(), fy * rect.height());
    let mut lines: Vec<Vec<Pos2>> = Vec::new();
    let mut grid = |f: f32| {
        lines.push(vec![at(f, 0.0), at(f, 1.0)]);
        lines.push(vec![at(0.0, f), at(1.0, f)]);
    };
    match guide {
        Guide::None => return,
        Guide::Thirds => {
            grid(1.0 / 3.0);
            grid(2.0 / 3.0);
        }
        Guide::PhiGrid => {
            let phi = (1.0 + 5f32.sqrt()) / 2.0;
            grid(1.0 / (1.0 + phi));
            grid(phi / (1.0 + phi));
        }
        Guide::Center => {
            grid(0.5);
        }
        Guide::Diagonals => {
            let (w, h) = (rect.width(), rect.height());
            let [tl, tr, br, bl] = [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
            ];
            lines.push(vec![tl, br]);
            lines.push(vec![tr, bl]);
            // Reciprocals: from each corner, perpendicular to the diagonal that misses it.
            for (corner, dir) in [
                (tr, vec2(-h, w)),
                (bl, vec2(h, -w)),
                (tl, vec2(h, w)),
                (br, vec2(-h, -w)),
            ] {
                lines.push(vec![corner, ray_to_edge(rect, corner, dir)]);
            }
        }
        Guide::GoldenSpiral => lines = golden_spiral(rect, orientation),
    }
    for line in &lines {
        painter.add(egui::Shape::line(
            line.clone(),
            Stroke::new(3.0, Color32::from_black_alpha(140)),
        ));
    }
    for line in lines {
        painter.add(egui::Shape::line(
            line,
            Stroke::new(1.2, Color32::from_white_alpha(220)),
        ));
    }
}

/// Where a ray from `origin` (on the rect's border) in `dir` leaves the rect.
fn ray_to_edge(rect: Rect, origin: Pos2, dir: Vec2) -> Pos2 {
    let mut t = f32::MAX;
    for (d, lo, hi, o) in [
        (dir.x, rect.min.x, rect.max.x, origin.x),
        (dir.y, rect.min.y, rect.max.y, origin.y),
    ] {
        if d > 0.0 {
            t = t.min((hi - o) / d);
        } else if d < 0.0 {
            t = t.min((lo - o) / d);
        }
    }
    origin + dir * t
}

/// The golden spiral and its square divisions, stretched to fit `rect`. `orientation`
/// mirrors it into each of the four corners.
fn golden_spiral(rect: Rect, orientation: u8) -> Vec<Vec<Pos2>> {
    let phi = (1.0 + 5f32.sqrt()) / 2.0;
    // Build in a phi x 1 landscape rectangle, cutting squares off left, top, right, bottom.
    let (mut x, mut y, mut w, mut h) = (0.0f32, 0.0f32, phi, 1.0f32);
    let mut arcs: Vec<Vec<(f32, f32)>> = Vec::new();
    for k in 0..10 {
        let (center, from, to, cut) = match k % 4 {
            0 => {
                let s = h;
                (
                    (x + s, y + s),
                    (x, y + s),
                    (x + s, y),
                    [(x + s, y), (x + s, y + h)],
                )
            }
            1 => {
                let s = w;
                (
                    (x, y + s),
                    (x, y),
                    (x + s, y + s),
                    [(x, y + s), (x + w, y + s)],
                )
            }
            2 => {
                let s = h;
                (
                    (x + w - s, y),
                    (x + w, y),
                    (x + w - s, y + s),
                    [(x + w - s, y), (x + w - s, y + h)],
                )
            }
            _ => {
                let s = w;
                (
                    (x + s, y + h - s),
                    (x + s, y + h),
                    (x, y + h - s),
                    [(x, y + h - s), (x + w, y + h - s)],
                )
            }
        };
        arcs.push(cut.to_vec());
        let r = h.min(w);
        let a0 = (from.1 - center.1).atan2(from.0 - center.0);
        let a1 = (to.1 - center.1).atan2(to.0 - center.0);
        let mut sweep = a1 - a0;
        if sweep > std::f32::consts::PI {
            sweep -= std::f32::consts::TAU;
        } else if sweep < -std::f32::consts::PI {
            sweep += std::f32::consts::TAU;
        }
        arcs.push(
            (0..=16)
                .map(|i| {
                    let a = a0 + sweep * i as f32 / 16.0;
                    (center.0 + r * a.cos(), center.1 + r * a.sin())
                })
                .collect(),
        );
        match k % 4 {
            0 => {
                x += h;
                w -= h;
            }
            1 => {
                y += w;
                h -= w;
            }
            2 => w -= h,
            _ => h -= w,
        }
    }
    let portrait = rect.height() > rect.width();
    arcs.into_iter()
        .map(|pts| {
            pts.into_iter()
                .map(|(px, py)| {
                    let (mut u, mut v) = (px / phi, py);
                    if orientation & 1 != 0 {
                        u = 1.0 - u;
                    }
                    if orientation & 2 != 0 {
                        v = 1.0 - v;
                    }
                    if portrait {
                        (u, v) = (v, u);
                    }
                    rect.min + vec2(u * rect.width(), v * rect.height())
                })
                .collect()
        })
        .collect()
}

fn image_dimensions(bytes: &[u8]) -> image::ImageResult<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()?
        .into_dimensions()
}

fn decode(path: &Path, filter: Filter) -> Result<(ColorImage, Option<image::RgbaImage>), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    if img.width().max(img.height()) > MAX_TEXTURE_SIDE {
        img = img.thumbnail(MAX_TEXTURE_SIDE, MAX_TEXTURE_SIDE);
    }
    let sample = (filter == Filter::Plain).then(|| {
        if img.width().max(img.height()) > SAMPLE_SIDE {
            img.thumbnail(SAMPLE_SIDE, SAMPLE_SIDE).to_rgba8()
        } else {
            img.to_rgba8()
        }
    });
    let rgba = filters::apply(img, filter);
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok((
        ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        sample,
    ))
}
