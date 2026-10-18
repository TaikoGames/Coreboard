use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, ColorImage, CornerRadius, Event, FontId, Key, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextureHandle, TextureOptions, Vec2, pos2, vec2,
};

use crate::board::{self, Board, Item, Store};

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

type Decoded = (String, Result<ColorImage, String>);

enum Drag {
    None,
    Pan,
    Move { moved: bool },
    Resize { id: u64, aspect: f32 },
    Marquee { start: Pos2, additive: bool },
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
    show_help: bool,
    clipboard: Option<arboard::Clipboard>,
}

impl CoreBoardApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, store: Store) -> Self {
        let (decoded_tx, decoded_rx) = channel();
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
            show_help,
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
            if !self.selection.contains(&item.id) {
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
            .filter(|i| selection.contains(&i.id))
            .collect();
        let all_on = picked.iter_mut().all(|i| *flag(i));
        for item in picked {
            *flag(item) = !all_on;
        }
        self.touch();
    }

    /// Packs items into rows, in reading order, starting at their current top-left corner.
    /// With `uniform`, every item is first scaled to the median height.
    fn arrange_grid(&mut self, uniform: bool) {
        const GAP: f32 = 16.0;
        let targets = self.targets();
        let mut picked: Vec<usize> = (0..self.board.items.len())
            .filter(|&i| targets.contains(&self.board.items[i].id))
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

    /// Topmost item under a screen position.
    fn hit(&self, canvas: Rect, pos: Pos2) -> Option<u64> {
        self.board
            .items
            .iter()
            .rev()
            .find(|i| self.item_screen_rect(canvas, i).contains(pos))
            .map(|i| i.id)
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
                rotation: 0,
                flip_x: false,
                grayscale: false,
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

    /// Grayscale gets its own texture so toggling it back and forth is instant.
    fn texture(&mut self, ctx: &egui::Context, file: &str, grayscale: bool) -> &Texture {
        let key = if grayscale {
            format!("{file}#gray")
        } else {
            file.to_string()
        };
        if !self.textures.contains_key(&key) {
            self.textures.insert(key.clone(), Texture::Loading);
            let path = self.store.image_path(file);
            let tx = self.decoded_tx.clone();
            let ctx = ctx.clone();
            let key = key.clone();
            std::thread::spawn(move || {
                let _ = tx.send((key, decode(&path, grayscale)));
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
        while let Ok((file, result)) = self.decoded_rx.try_recv() {
            let tex = match result {
                Ok(img) => Texture::Ready(ctx.load_texture(&file, img, options)),
                Err(err) => Texture::Failed(err),
            };
            self.textures.insert(file, tex);
        }
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

        // Keyboard.
        let mut paste_requested = None;
        for event in &events {
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
                    Key::S if m.command => {
                        self.save_now();
                        self.toast("Saved".into());
                    }
                    Key::F | Key::Home => self.fit(canvas),
                    Key::R if !m.command => self.rotate_selection(if m.shift { -1 } else { 1 }),
                    Key::G if !m.command => self.arrange_grid(m.shift),
                    Key::M if !m.command => self.toggle_selection(|i| &mut i.flip_x),
                    Key::B if !m.command => self.toggle_selection(|i| &mut i.grayscale),
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
            } if canvas.contains(*pos) => Some((*pos, *button)),
            _ => None,
        });
        if let Some((pos, button)) = press
            && matches!(self.drag, Drag::None)
        {
            let space = ctx.input(|i| i.key_down(Key::Space));
            self.drag = if button != egui::PointerButton::Primary || space {
                Drag::Pan
            } else if let Some(id) = self.hit_handle(canvas, pos) {
                let item = self.board.items.iter().find(|i| i.id == id).unwrap();
                let aspect = item.size[0] / item.size[1];
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
                Drag::Move { moved: false }
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
            Drag::None | Drag::Marquee { .. } => {}
            Drag::Pan => {
                self.board.view.offset[0] += delta.x;
                self.board.view.offset[1] += delta.y;
                if delta != Vec2::ZERO {
                    self.touch();
                }
            }
            Drag::Move { moved } => {
                if delta != Vec2::ZERO {
                    *moved = true;
                    let d = delta / zoom;
                    for item in &mut self.board.items {
                        if self.selection.contains(&item.id) {
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
                        let w = (world.x - item.pos[0])
                            .max((world.y - item.pos[1]) * aspect)
                            .max(min_w);
                        item.size = [w, w / aspect];
                    }
                    self.touch();
                }
            }
        }

        // Finish the drag.
        if !pointer.any_down() {
            match std::mem::replace(&mut self.drag, Drag::None) {
                Drag::Move { moved: false } => {
                    // A plain click: drop the no-op history entry and collapse to one item.
                    self.undo.pop();
                    if !(modifiers.shift || modifiers.command)
                        && let Some(id) = pointer.interact_pos().and_then(|p| self.hit(canvas, p))
                    {
                        self.selection = HashSet::from([id]);
                    }
                }
                Drag::Marquee { start, additive } => {
                    if let Some(end) = pointer.interact_pos() {
                        let area = Rect::from_two_pos(start, end);
                        if !additive {
                            self.selection.clear();
                        }
                        for item in &self.board.items {
                            if area.intersects(self.item_screen_rect(canvas, item)) {
                                self.selection.insert(item.id);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn paint(&mut self, ui: &egui::Ui, canvas: Rect) {
        let ctx = ui.ctx().clone();
        let painter = ui.painter_at(canvas);
        painter.rect_filled(canvas, CornerRadius::ZERO, BG);

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
                painter.circle_filled(pos2(x, y), 1.2, GRID);
                y += step;
            }
            x += step;
        }

        // Items, bottom to top.
        let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
        let items = self.board.items.clone();
        for item in &items {
            let rect = self.item_screen_rect(canvas, item);
            if !rect.intersects(canvas) {
                continue;
            }
            match self.texture(&ctx, &item.file, item.grayscale) {
                Texture::Ready(tex) => {
                    if item.rotation == 0 && !item.flip_x {
                        painter.image(tex.id(), rect, uv, Color32::WHITE);
                    } else {
                        painter.add(transformed_image(
                            tex.id(),
                            rect,
                            item.rotation,
                            item.flip_x,
                        ));
                    }
                }
                Texture::Loading => {
                    painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_gray(50));
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
            if self.selection.contains(&item.id) {
                // Dark halo keeps the outline visible on images of any color.
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
                painter.rect_filled(Self::handle_rect(rect), CornerRadius::same(2), ACCENT);
            }
        }

        // Marquee.
        if let (Drag::Marquee { start, .. }, Some(end)) =
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
                "Drop images here, or paste with Ctrl+V",
                FontId::proportional(22.0),
                Color32::from_gray(120),
            );
        }

        // Status line.
        let status = format!(
            "{:.0}%  ·  {} images{}",
            zoom * 100.0,
            self.board.items.len(),
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
            if at.elapsed() < Duration::from_secs(2) {
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
        self.handle_input(ui, canvas);
        self.paint(ui, canvas);

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

/// An image quad whose texture is turned by `quarters` clockwise quarter turns,
/// then optionally mirrored left-to-right.
fn transformed_image(
    texture: egui::TextureId,
    rect: Rect,
    quarters: u8,
    flip_x: bool,
) -> egui::Shape {
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    let uvs = [
        pos2(0.0, 0.0),
        pos2(1.0, 0.0),
        pos2(1.0, 1.0),
        pos2(0.0, 1.0),
    ];
    let mut mesh = egui::Mesh::with_texture(texture);
    for (i, pos) in corners.into_iter().enumerate() {
        // Mirroring swaps left and right corners: 0<->1 (top), 2<->3 (bottom).
        let source = if flip_x { i ^ 1 } else { i };
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: uvs[(source + 4 - quarters as usize) % 4],
            color: Color32::WHITE,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    egui::Shape::mesh(mesh)
}

fn image_dimensions(bytes: &[u8]) -> image::ImageResult<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()?
        .into_dimensions()
}

fn decode(path: &Path, grayscale: bool) -> Result<ColorImage, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    if img.width().max(img.height()) > MAX_TEXTURE_SIDE {
        img = img.thumbnail(MAX_TEXTURE_SIDE, MAX_TEXTURE_SIDE);
    }
    if grayscale {
        img = img.grayscale();
    }
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}
