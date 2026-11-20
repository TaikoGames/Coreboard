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
use crate::i18n::{tr, trf};
use crate::library::{self, Tone};

mod cards;
mod context;
mod copy;
mod draw;
mod focus;
mod groups;
mod loupe;
mod minimap;
mod motion;
mod organize;
mod pdfs;
mod slots;
mod updates;
mod watch;
use cards::draw_card;
use draw::{item_mark_shapes, mark_shapes, preview_mark};
use organize::Align;
use slots::{draw_placeholder, placeholder_label_rect};

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
/// Room for the zoom controls in the bottom-left corner.
const ZOOM_CONTROLS_WIDTH: f32 = 150.0;
const RADIAL_INNER: f32 = 34.0;
const RADIAL_OUTER: f32 = 118.0;

/// The radial menu's slices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RadialAction {
    // For an image.
    SimilarValues,
    SimilarPalette,
    SearchWeb,
    Palette,
    StudyView,
    Guide,
    Crop,
    Focus,
    // For the board.
    Note,
    Link,
    Slot,
    Paste,
    Fit,
    Surprise,
    Boards,
    Help,
}

impl RadialAction {
    /// Slices over an image (or with one selected), clockwise from the top.
    const IMAGE: [Self; 8] = [
        Self::SimilarValues,
        Self::SimilarPalette,
        Self::SearchWeb,
        Self::Palette,
        Self::StudyView,
        Self::Guide,
        Self::Crop,
        Self::Focus,
    ];
    /// Slices over the empty board.
    const BOARD: [Self; 8] = [
        Self::Note,
        Self::Link,
        Self::Slot,
        Self::Paste,
        Self::Fit,
        Self::Surprise,
        Self::Boards,
        Self::Help,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::SimilarValues => tr("Similar\nvalues"),
            Self::SimilarPalette => tr("Similar\npalette"),
            Self::SearchWeb => tr("Search\nweb"),
            Self::Palette => tr("Extract\npalette"),
            Self::StudyView => tr("Study\nview"),
            Self::Guide => tr("Guide"),
            Self::Crop => tr("Crop"),
            Self::Focus => tr("Focus\nview"),
            Self::Note => tr("Note"),
            Self::Link => tr("Link"),
            Self::Slot => tr("Image\nslot"),
            Self::Paste => tr("Paste"),
            Self::Fit => tr("Fit\nview"),
            Self::Surprise => tr("Surprise\nme"),
            Self::Boards => tr("Boards"),
            Self::Help => tr("Help"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FindMode {
    Values,
    Palette,
    Web,
}

/// State of the Find more window.
struct Finder {
    /// The image to find more like.
    query: u64,
    mode: FindMode,
    /// `None` while the boards are being scanned.
    results: Option<Result<Vec<library::Match>, String>>,
    subject: String,
    /// The web color filter, once the user picks one; until then the image's own color.
    tone: Option<Tone>,
    suggested: &'static str,
}

/// (board generation, query image id, query signature and ranked library)
type Found = (
    u64,
    u64,
    Result<(library::Signature, Vec<library::Match>), String>,
);
type Thumb = (std::path::PathBuf, Result<ColorImage, String>);
/// (board generation, where to put it in world space, the link, the downloaded picture)
type Fetched = (u64, Pos2, String, Result<crate::web::Fetched, String>);

/// Whether `pos` is over a window or popup rather than the board itself.
fn over_ui(ctx: &egui::Context, pos: Pos2) -> bool {
    ctx.layer_id_at(pos)
        .is_some_and(|l| l.order != egui::Order::Background)
}

/// (board generation, source image id, its dominant colors)
type Extracted = (u64, u64, Result<Vec<[u8; 3]>, String>);

enum Drag {
    None,
    Pan,
    /// `start`: where each moving (unpinned) item began; `total`: the unsnapped offset so far.
    Move {
        moved: bool,
        start: HashMap<u64, [f32; 2]>,
        total: Vec2,
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
    /// Drawing a mark (screen points so far): on the image it started on, or a new drawing.
    Draw {
        item: Option<u64>,
        points: Vec<Pos2>,
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
    /// Drawing on images instead of selecting and moving.
    draw_mode: bool,
    draw_tool: board::MarkShape,
    draw_color: u8,
    show_marks: bool,
    /// Whole-board value check: 0 off, 1 grayscale, 2 three-tone notan.
    board_values: u8,
    motion: motion::Motion,
    animations: bool,
    minimap: bool,
    /// The mini-map's world area, held still while it's being dragged in.
    minimap_drag: Option<Rect>,
    /// How much the magnifier (hold Z) enlarges.
    loupe_power: f32,
    check_updates: bool,
    /// "auto", "en" or "es".
    language: String,
    updates: updates::Updates,
    /// The image open in focus view.
    focus: Option<focus::Focus>,
    /// The note or frame whose text is being edited.
    editing: Option<u64>,
    /// Its contents before editing (None for a just-created item), to skip no-op undo steps.
    edit_original: Option<Item>,
    edit_focus_pending: bool,
    /// Screen area of the text editor, where the board ignores presses.
    edit_rect: Option<Rect>,
    show_help: bool,
    /// Where the help panel was drawn last frame, to put its close button on it.
    help_rect: Option<Rect>,
    /// The OS window level is sent once on startup, then on each toggle.
    window_level_sent: bool,
    show_export: bool,
    exporting: bool,
    /// The last exported file, shown until its message is closed.
    export_notice: Option<std::path::PathBuf>,
    exported_tx: Sender<Exported>,
    exported_rx: Receiver<Exported>,
    palette_tx: Sender<Extracted>,
    palette_rx: Receiver<Extracted>,
    /// Bumped on every board switch, so late background results for the old board are dropped.
    generation: u64,
    show_boards: bool,
    new_board_name: String,
    /// Center of the open radial menu.
    radial: Option<Pos2>,
    /// The slices of the open radial menu: for an image, or for the board.
    radial_actions: [RadialAction; 8],
    finder: Option<Finder>,
    find_tx: Sender<Found>,
    find_rx: Receiver<Found>,
    /// Thumbnails of images on other boards, by path.
    thumbs: HashMap<std::path::PathBuf, Texture>,
    thumb_tx: Sender<Thumb>,
    thumb_rx: Receiver<Thumb>,
    pdf_tx: Sender<pdfs::RenderedPdf>,
    pdf_rx: Receiver<pdfs::RenderedPdf>,
    /// PDFs whose pages are being rendered.
    rendering_pdfs: usize,
    fetch_tx: Sender<Fetched>,
    fetch_rx: Receiver<Fetched>,
    preview_tx: Sender<cards::Previewed>,
    preview_rx: Receiver<cards::Previewed>,
    /// Link cards whose preview was already asked for: (board generation, card id).
    preview_asked: HashSet<(u64, u64)>,
    /// The next cards belong to the drop or paste that just added pictures (one undo step).
    join_cards: bool,
    /// When the selection was last nudged with an arrow key (a burst is one undo step).
    last_nudge: Option<Instant>,
    /// What the last right-click landed on, for its menu.
    context: Option<context::ContextTarget>,
    /// The "add a link" box (Ctrl+click on an empty spot).
    link_prompt: Option<cards::LinkPrompt>,
    /// Web links being downloaded.
    fetching: usize,
    show_tags: bool,
    tags_focus_pending: bool,
    new_tag: String,
    /// The search text while the search bar is open.
    search: Option<String>,
    search_focus_pending: bool,
    /// Alignment guides (world space) shown while a move snaps.
    snap_lines: Vec<[Pos2; 2]>,
    watch_polled: Instant,
    /// Watched files waiting to finish writing: name -> (size, when first seen at that size).
    watch_pending: HashMap<String, (u64, Instant)>,
    watch_error: Option<String>,
    watch_path_input: String,
    recent: Vec<std::path::PathBuf>,
    clipboard: Option<arboard::Clipboard>,
}

impl CoreBoardApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, mut store: Store) -> Self {
        let (decoded_tx, decoded_rx) = channel();
        let (exported_tx, exported_rx) = channel();
        let (palette_tx, palette_rx) = channel();
        let (find_tx, find_rx) = channel();
        let (thumb_tx, thumb_rx) = channel();
        let (fetch_tx, fetch_rx) = channel();
        let (pdf_tx, pdf_rx) = channel();
        let (preview_tx, preview_rx) = channel();
        let board = store.load();
        let show_help = board.items.is_empty() && store.unreadable.is_none();
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
            draw_mode: false,
            draw_tool: board::MarkShape::Arrow,
            draw_color: 0,
            show_marks: true,
            board_values: 0,
            motion: motion::Motion::default(),
            animations: boards::settings().animations,
            minimap: boards::settings().minimap,
            loupe_power: 3.0,
            minimap_drag: None,
            check_updates: boards::settings().check_updates,
            language: boards::settings().language,
            updates: updates::Updates::default(),
            focus: None,
            editing: None,
            edit_original: None,
            edit_focus_pending: false,
            edit_rect: None,
            show_help,
            help_rect: None,
            window_level_sent: false,
            show_export: false,
            exporting: false,
            export_notice: None,
            exported_tx,
            exported_rx,
            palette_tx,
            palette_rx,
            generation: 0,
            show_boards: false,
            new_board_name: String::new(),
            radial: None,
            radial_actions: RadialAction::BOARD,
            finder: None,
            find_tx,
            find_rx,
            thumbs: HashMap::new(),
            thumb_tx,
            thumb_rx,
            fetch_tx,
            fetch_rx,
            pdf_tx,
            pdf_rx,
            rendering_pdfs: 0,
            preview_tx,
            preview_rx,
            preview_asked: HashSet::new(),
            link_prompt: None,
            join_cards: false,
            last_nudge: None,
            context: None,
            fetching: 0,
            show_tags: false,
            tags_focus_pending: false,
            new_tag: String::new(),
            search: None,
            search_focus_pending: false,
            snap_lines: Vec::new(),
            watch_polled: Instant::now(),
            watch_pending: HashMap::new(),
            watch_error: None,
            watch_path_input: String::new(),
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
        if let Some([x, y, w, h]) = item.pinned {
            return Rect::from_min_size(canvas.min + vec2(x, y), vec2(w, h));
        }
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
            self.fly_view(board::View::default());
            return;
        };
        let avail = canvas.size() * 0.9;
        let zoom = (avail.x / bounds.width())
            .min(avail.y / bounds.height())
            .clamp(MIN_ZOOM, MAX_ZOOM);
        let offset = canvas.size() * 0.5 - bounds.center().to_vec2() * zoom;
        self.fly_view(board::View {
            offset: [offset.x, offset.y],
            zoom,
        });
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
        let doomed = self
            .board
            .items
            .iter()
            .filter(|i| self.selection.contains(&i.id) && !i.locked)
            .count();
        let kept = self.selection.len() - doomed;
        if kept > 0 {
            self.toast(trf("{} locked item(s) kept · unlock with L", &[&kept]));
        }
        if doomed == 0 {
            return;
        }
        self.checkpoint();
        self.board
            .items
            .retain(|i| !self.selection.contains(&i.id) || i.locked);
        self.selection
            .retain(|id| self.board.items.iter().any(|i| i.id == *id));
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
            if !self.selection.contains(&item.id) || !item.is_image() || !item.movable() {
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
            self.toast(tr("Select images first").into());
            return;
        };
        let next = board::cycle(&Analysis::CYCLE, current, forward);
        self.set_on_selection(|i| i.analysis = next);
        self.toast(next.label().into());
    }

    fn cycle_guide(&mut self) {
        let Some(current) = self.first_selected().map(|i| i.guide) else {
            self.toast(tr("Select images first").into());
            return;
        };
        let next = board::cycle(&Guide::CYCLE, current, true);
        self.set_on_selection(|i| i.guide = next);
        self.toast(next.label().into());
    }

    fn turn_spiral(&mut self) {
        if self.first_selected().is_none() {
            self.toast(tr("Select images first").into());
            return;
        }
        self.set_on_selection(|i| {
            i.guide = Guide::GoldenSpiral;
            i.guide_orientation = (i.guide_orientation + 1) % 4;
        });
        self.toast(tr("Golden spiral turned").into());
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
            Kind::Note { .. }
            | Kind::Frame { .. }
            | Kind::Placeholder { .. }
            | Kind::File { .. }
            | Kind::Link { .. }
            | Kind::Drawing { .. } => return None,
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
            "#{r:02X}{g:02X}{b:02X}\nH {h:.0}°  S {:.0}%  V {:.0}%\n{} (L*) {l:.0}",
            s * 100.0,
            v * 100.0,
            tr("Value")
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

    // ---------------------------------------------------------------- duplicate and values

    /// Copies the selection and puts the copies right beside it, selected.
    fn duplicate_selection(&mut self) {
        let picked: Vec<Item> = self
            .board
            .items
            .iter()
            .filter(|i| self.selection.contains(&i.id))
            .cloned()
            .collect();
        let Some(bounds) = picked
            .iter()
            .map(|i| Rect::from_min_size(pos2(i.pos[0], i.pos[1]), vec2(i.size[0], i.size[1])))
            .reduce(|a, b| a.union(b))
        else {
            return;
        };
        let shift = bounds.width() + 24.0;
        let zoom = self.board.view.zoom;
        self.checkpoint();
        self.selection.clear();
        // Copies of a group form a new group of their own.
        let mut new_groups: HashMap<u64, u64> = HashMap::new();
        for mut item in picked {
            item.id = self.board.next_id;
            self.board.next_id += 1;
            if let Some(g) = item.group {
                let next = &mut self.board.next_id;
                item.group = Some(*new_groups.entry(g).or_insert_with(|| {
                    *next += 1;
                    *next - 1
                }));
            }
            item.pos[0] += shift;
            if let Some(pin) = &mut item.pinned {
                pin[0] += shift * zoom;
            }
            self.selection.insert(item.id);
            self.board.items.push(item);
        }
        let n = self.selection.len();
        self.toast(if n == 1 {
            tr("Duplicated").into()
        } else {
            trf("Duplicated {} items", &[&n])
        });
        self.touch();
    }

    /// Off → whole board in grayscale → in 3 tones → off. A view only: nothing is changed.
    fn cycle_board_values(&mut self) {
        self.board_values = (self.board_values + 1) % 3;
        self.toast(
            match self.board_values {
                1 => tr("Values: whole board in grayscale"),
                2 => tr("Values: whole board in 3 tones"),
                _ => tr("Back to color"),
            }
            .into(),
        );
    }

    /// A palette's colors as the current view shows them.
    fn view_palette(&self, colors: &[[u8; 3]]) -> Vec<[u8; 3]> {
        match self.board_values {
            1 => colors
                .iter()
                .map(|c| filters::value_gray(*c, None))
                .collect(),
            2 => colors
                .iter()
                .map(|c| filters::value_gray(*c, Some(3)))
                .collect(),
            _ => colors.to_vec(),
        }
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
            self.toast(tr("Select images to extract colors from").into());
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
        self.toast(tr("Extracting colors…").into());
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
                    self.toast(trf("Color extraction failed: {}", &[&err]));
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
            self.toast(tr("Crop done").into());
            return;
        }
        let id = self.selection.iter().next().copied();
        let is_image = id
            .and_then(|id| self.board.items.iter().find(|i| i.id == id))
            .is_some_and(|i| i.is_image() && i.movable());
        if self.selection.len() != 1 || !is_image {
            self.toast(tr("Select one image to crop").into());
            return;
        }
        self.crop_mode = id;
        self.toast(tr("Drag over the image to crop · Enter/Esc when done · Shift+X resets").into());
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
        self.toast(tr("Crop reset").into());
        self.touch();
    }

    /// Packs items into rows, in reading order, starting at their current top-left corner.
    /// With `uniform`, every item is first scaled to the median height.
    fn arrange_grid(&mut self, uniform: bool) {
        const GAP: f32 = 16.0;
        let targets = self.targets();
        let mut picked: Vec<usize> = (0..self.board.items.len())
            .filter(|&i| {
                let item = &self.board.items[i];
                targets.contains(&item.id) && !item.is_frame() && item.movable()
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
        // Pinned items float above the board.
        let pinned = |i: &&Item| i.pinned.is_some();
        items
            .iter()
            .rev()
            .filter(pinned)
            .find(|i| self.item_hit(canvas, i, pos))
            .or_else(|| {
                items
                    .iter()
                    .rev()
                    .filter(|i| !i.is_frame() && i.pinned.is_none())
                    .find(|i| self.item_hit(canvas, i, pos))
            })
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
        let items = &self.board.items;
        let frames = items.iter().filter(|i| i.is_frame() && i.pinned.is_none());
        let rest = items.iter().filter(|i| !i.is_frame() && i.pinned.is_none());
        let pinned = items.iter().filter(|i| i.pinned.is_some());
        frames.chain(rest).chain(pinned).cloned().collect()
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
            .filter(|i| !self.selection.contains(&i.id) && i.movable())
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
                title: tr("Untitled").into(),
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

    /// Starts editing a note's text, a frame's title, a slot's label or an image's caption.
    /// `created` means the item was just added, so its creation is already on the undo stack.
    fn begin_edit(&mut self, id: u64, created: bool) {
        self.finish_edit();
        let Some(item) = self.board.items.iter().find(|i| i.id == id) else {
            return;
        };
        if matches!(item.kind, Kind::Palette { .. }) {
            return;
        }
        self.edit_original = if created {
            None
        } else {
            let original = item.clone();
            self.checkpoint();
            Some(original)
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
            Some(original) if original == *item => {
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
        let mut centered = false;
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
            Kind::Placeholder { .. } => {
                centered = true;
                (
                    placeholder_label_rect(rect, zoom),
                    board::PLACEHOLDER_FONT * zoom,
                    Color32::from_gray(225),
                    false,
                )
            }
            Kind::Image => {
                // Keep the caption editor readable even when zoomed far out.
                let font = (board::CAPTION_FONT * zoom).max(12.0);
                (
                    Rect::from_min_size(
                        pos2(rect.min.x, rect.max.y + board::CAPTION_GAP * zoom),
                        vec2(rect.width().max(160.0), font * 1.35),
                    ),
                    font,
                    Color32::from_gray(225),
                    false,
                )
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
        let is_image = item.is_image();
        let text = match &mut item.kind {
            Kind::Note { text, .. } => text,
            Kind::Frame { title } => title,
            Kind::Placeholder { label } => label,
            Kind::Image => &mut item.caption,
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
                let edit = if centered {
                    edit.align(Align2::CENTER_TOP)
                        .hint_text(tr("Name this slot, e.g. Front view"))
                } else if is_image {
                    edit.hint_text(tr("Add a caption, e.g. Sargent, 1885, rim light"))
                } else {
                    edit
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
            .filter(|i| self.selection.contains(&i.id) && !i.locked)
            .find(|i| Self::handle_rect(self.item_screen_rect(canvas, i)).contains(pos))
            .map(|i| i.id)
    }

    // ---------------------------------------------------------------- import

    /// Adds images to the board, cascading them from `at` (screen space).
    /// Adds pictures at `at`; returns how many were added.
    fn import_all(
        &mut self,
        canvas: Rect,
        at: Pos2,
        mut sources: Vec<(Vec<u8>, Option<&Path>)>,
    ) -> usize {
        // Dropped or pasted onto an empty slot: the first image fills it.
        if !sources.is_empty()
            && let Some(slot) = self.slot_at(canvas, at)
        {
            let (bytes, path) = sources.remove(0);
            if !self.fill_slot_with_bytes(slot, &bytes, path) {
                self.toast(tr("That file isn't an image I can read").into());
            }
        }
        let mut added = 0;
        let mut failed = 0;
        let mut unreadable = Vec::new();
        let mut cursor = self.to_world(canvas, at);
        let max_screen_side = canvas.width().min(canvas.height()) * 0.5;

        for (bytes, path) in sources {
            let decoded = board::image_ext(&bytes, path).zip(image_dimensions(&bytes).ok());
            let Some((ext, (w, h))) = decoded else {
                // A file it can't show as a picture still goes on the board, as a card.
                match path {
                    Some(path) => unreadable.push(path.to_path_buf()),
                    None => failed += 1,
                }
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
        if !unreadable.is_empty() {
            let at = self.to_screen(canvas, cursor);
            self.join_cards = added > 0;
            self.add_file_cards(canvas, at, unreadable);
        }
        if failed > 0 {
            self.toast(tr("That isn't a picture I can read").into());
        }
        added
    }

    fn paste(&mut self, canvas: Rect, at: Pos2, pasted_text: Option<String>) {
        // With one empty slot selected, pasting fills it wherever the pointer is.
        let at = match self.selection.iter().collect::<Vec<_>>().as_slice() {
            [id] => self
                .board
                .items
                .iter()
                .find(|i| i.id == **id && matches!(i.kind, Kind::Placeholder { .. }))
                .map(|i| self.item_screen_rect(canvas, i).center())
                .unwrap_or(at),
            _ => at,
        };
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
        // A web link (copied image address or page): download it in the background.
        if let Some(url) = text.as_deref().and_then(crate::web::link) {
            // A video becomes a video card that plays in the floating player; anything else
            // is downloaded as a picture.
            if crate::web::is_video(&url) {
                self.add_link_card(canvas, at, url, None);
            } else {
                self.fetch_link(canvas, at, url);
            }
            return;
        }
        let paths: Vec<_> = text
            .iter()
            .flat_map(|t| t.lines())
            .map(|l| l.trim().trim_start_matches("file://"))
            .filter(|l| !l.is_empty())
            .map(std::path::PathBuf::from)
            .filter(|p| p.exists())
            .collect();
        if paths.is_empty() {
            self.toast(tr("Clipboard has no image").into());
            return;
        }
        let (pictures, others): (Vec<_>, Vec<_>) =
            paths.into_iter().partition(|p| looks_like_picture(p));
        let sources = pictures
            .iter()
            .filter_map(|p| Some((std::fs::read(p).ok()?, Some(p.as_path()))))
            .collect();
        let added = self.import_all(canvas, at, sources);
        let (pdfs, others): (Vec<_>, Vec<_>) =
            others.into_iter().partition(|p| pdfs::is_pdf_file(p));
        for pdf in pdfs {
            self.import_pdf(canvas, at, pdf);
        }
        self.join_cards = added > 0;
        self.add_file_cards(canvas, at + vec2(0.0, 40.0), others);
    }

    fn fetch_link(&mut self, canvas: Rect, at: Pos2, url: String) {
        let tx = self.fetch_tx.clone();
        let generation = self.generation;
        let world = self.to_world(canvas, at);
        self.fetching += 1;
        self.toast(tr("Downloading picture…").into());
        std::thread::spawn(move || {
            let result = crate::web::fetch_image(&url);
            let _ = tx.send((generation, world, url, result));
        });
    }

    /// Puts downloaded pictures where the link was pasted.
    fn receive_fetches(&mut self, ctx: &egui::Context, canvas: Rect) {
        if self.fetching > 0 {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        while let Ok((generation, world, url, result)) = self.fetch_rx.try_recv() {
            self.fetching = self.fetching.saturating_sub(1);
            if generation != self.generation {
                continue;
            }
            let at = self.to_screen(canvas, world);
            match result {
                Ok(crate::web::Fetched::Image(bytes)) => {
                    self.import_all(canvas, at, vec![(bytes, None)]);
                }
                // No picture there: keep the page itself as a link card.
                Ok(crate::web::Fetched::Page { title }) => {
                    self.add_link_card(canvas, at, url, title);
                }
                Err(err) => {
                    eprintln!("coreboard: {url}: {err}");
                    self.toast(trf("Couldn't get that picture: {}", &[&err]));
                }
            }
        }
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
            trf("all {} images", &[&self.board.items.len()])
        } else {
            trf("{} selected image(s)", &[&self.selection.len()])
        };
        let mut export = false;
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        egui::Window::new(tr("Export PNG"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(trf("Exports {} as one PNG.", &[&count]));
                ui.add_space(6.0);
                let wm = &mut self.board.watermark;
                ui.label(tr("Watermark text (leave empty for none):"));
                let field = ui.add(
                    egui::TextEdit::singleline(&mut wm.text)
                        .hint_text(tr("e.g. © Your Name"))
                        .desired_width(260.0),
                );
                if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    export = true;
                }
                ui.horizontal(|ui| {
                    ui.label(tr("Position"));
                    egui::ComboBox::from_id_salt("watermark-position")
                        .selected_text(wm.position.label())
                        .show_ui(ui, |ui| {
                            for pos in WatermarkPosition::ALL {
                                ui.selectable_value(&mut wm.position, pos, pos.label());
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label(tr("Opacity"));
                    let mut percent = (wm.opacity * 100.0).round();
                    ui.add(egui::Slider::new(&mut percent, 10.0..=100.0).suffix("%"));
                    wm.opacity = percent / 100.0;
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let label = if self.exporting {
                        tr("Exporting…")
                    } else {
                        tr("Export")
                    };
                    export |= ui
                        .add_enabled(!self.exporting, egui::Button::new(label))
                        .clicked();
                    close |= ui.button(tr("Cancel")).clicked();
                });
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(trf("Saved to {}", &[&self.export_dir().display()]))
                            .small()
                            .weak(),
                    );
                    if ui.small_button(tr("Change…")).clicked()
                        && let Some(dir) = rfd::FileDialog::new()
                            .set_title(tr("Folder for exports"))
                            .set_directory(self.export_dir())
                            .pick_folder()
                    {
                        self.board.export_dir = Some(dir);
                    }
                    if self.board.export_dir.is_some() && ui.small_button(tr("Reset")).clicked() {
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
        self.toast(tr("Exporting…").into());
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
                // Stays on screen until closed, with a way to the file.
                Ok(path) => self.export_notice = Some(path),
                Err(err) => self.toast(trf("Export failed: {}", &[&err])),
            }
        }
    }

    // ---------------------------------------------------------------- radial menu

    /// Opens the radial menu at `center`, selecting the image under it if it isn't already.
    /// Image actions over an image (or with one selected and the pointer on empty board);
    /// board actions otherwise.
    fn open_radial(&mut self, canvas: Rect, center: Pos2) {
        let under = self
            .hit(canvas, center)
            .and_then(|id| self.board.items.iter().find(|i| i.id == id));
        let image = match under {
            Some(item) if item.is_image() => {
                let id = item.id;
                if !self.selection.contains(&id) {
                    self.selection = HashSet::from([id]);
                }
                true
            }
            Some(_) => false,
            None => self.first_selected().is_some(),
        };
        self.radial_actions = if image {
            RadialAction::IMAGE
        } else {
            RadialAction::BOARD
        };
        self.radial = Some(center);
    }

    /// The slice the pointer points at, if it has moved far enough from the center.
    fn radial_choice(&self, pointer: Option<Pos2>) -> Option<RadialAction> {
        let (center, pos) = (self.radial?, pointer?);
        let d = pos - center;
        if d.length() < RADIAL_INNER {
            return None;
        }
        // Slice 0 points straight up; slices go clockwise.
        let actions = &self.radial_actions;
        let n = actions.len() as f32;
        let angle = d.x.atan2(-d.y).rem_euclid(std::f32::consts::TAU);
        let slice = ((angle / std::f32::consts::TAU * n + 0.5) as usize) % actions.len();
        Some(actions[slice])
    }

    fn run_radial(&mut self, ctx: &egui::Context, canvas: Rect, action: RadialAction) {
        let center = self.radial.unwrap_or(canvas.center());
        match action {
            RadialAction::SimilarValues => self.open_finder(FindMode::Values),
            RadialAction::SimilarPalette => self.open_finder(FindMode::Palette),
            RadialAction::SearchWeb => self.open_finder(FindMode::Web),
            RadialAction::Palette => self.extract_palettes(6),
            RadialAction::StudyView => self.cycle_analysis(true),
            RadialAction::Guide => self.cycle_guide(),
            RadialAction::Crop => self.toggle_crop_mode(),
            RadialAction::Focus => self.open_focus(canvas, Some(center)),
            RadialAction::Note => self.add_note(canvas, center),
            RadialAction::Link => self.open_link_prompt(canvas, center),
            RadialAction::Slot => self.add_placeholder(canvas, center),
            RadialAction::Paste => self.paste(canvas, center, None),
            RadialAction::Fit => self.fit(canvas),
            RadialAction::Surprise => self.surprise(ctx),
            RadialAction::Boards => self.open_boards_window(),
            RadialAction::Help => self.show_help = true,
        }
    }

    fn paint_radial(&self, ctx: &egui::Context) {
        let Some(center) = self.radial else { return };
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("coreboard-radial"),
        ));
        let chosen = self.radial_choice(ctx.input(|i| i.pointer.hover_pos()));
        let n = self.radial_actions.len();
        let step = std::f32::consts::TAU / n as f32;
        painter.circle_filled(center, RADIAL_OUTER + 6.0, Color32::from_black_alpha(150));
        for (i, action) in self.radial_actions.iter().enumerate() {
            // A wedge from the inner to the outer ring, centered on its direction.
            let mid = i as f32 * step;
            let dir = |a: f32| vec2(a.sin(), -a.cos());
            let mut points = Vec::new();
            for k in 0..=8 {
                let a = mid - step / 2.0 + step * k as f32 / 8.0 + 0.01;
                points.push(center + dir(a) * RADIAL_OUTER);
            }
            for k in (0..=8).rev() {
                let a = mid - step / 2.0 + step * k as f32 / 8.0 + 0.01;
                points.push(center + dir(a) * RADIAL_INNER);
            }
            let fill = if chosen == Some(*action) {
                ACCENT.gamma_multiply(0.85)
            } else {
                Color32::from_rgb(44, 46, 52)
            };
            painter.add(egui::Shape::convex_polygon(
                points,
                fill,
                Stroke::new(1.0, Color32::from_gray(25)),
            ));
            painter.text(
                center + dir(mid) * (RADIAL_INNER + RADIAL_OUTER) / 2.0,
                Align2::CENTER_CENTER,
                action.label(),
                FontId::proportional(12.5),
                Color32::WHITE,
            );
        }
        painter.circle_filled(center, RADIAL_INNER - 6.0, Color32::from_rgb(30, 31, 34));
        painter.text(
            center,
            Align2::CENTER_CENTER,
            "Q",
            FontId::proportional(13.0),
            Color32::from_gray(150),
        );
    }

    // ---------------------------------------------------------------- find more

    /// Opens the Find window for the first selected image and starts ranking the library.
    fn open_finder(&mut self, mode: FindMode) {
        let Some(item) = self.first_selected() else {
            self.toast(tr("Select an image to find more like it").into());
            return;
        };
        let (query, file) = (item.id, item.file.clone());
        let item_tags = item.tags.clone();
        let world = Rect::from_min_size(
            pos2(item.pos[0], item.pos[1]),
            vec2(item.size[0], item.size[1]),
        );
        // Prefill the subject with the title of a frame the image sits in.
        let subject = self
            .board
            .items
            .iter()
            .filter_map(|i| match &i.kind {
                Kind::Frame { title } => Some((i, title)),
                _ => None,
            })
            .find(|(f, _)| {
                Rect::from_min_size(pos2(f.pos[0], f.pos[1]), vec2(f.size[0], f.size[1]))
                    .contains_rect(world)
            })
            .map(|(_, t)| t.clone())
            .filter(|t| t != "Untitled" && t != tr("Untitled"))
            .or_else(|| (!item_tags.is_empty()).then(|| item_tags.join(" ")))
            .or_else(|| self.finder.as_ref().map(|f| f.subject.clone()))
            .unwrap_or_default();
        self.finder = Some(Finder {
            query,
            mode,
            results: None,
            subject,
            tone: None,
            suggested: "gray",
        });
        let (tx, dir, generation) = (
            self.find_tx.clone(),
            self.store.dir.clone(),
            self.generation,
        );
        std::thread::spawn(move || {
            let _ = tx.send((generation, query, library::find_similar(&dir, &file)));
        });
    }

    fn receive_finds(&mut self) {
        while let Ok((generation, query, result)) = self.find_rx.try_recv() {
            let Some(finder) = self.finder.as_mut() else {
                continue;
            };
            if generation != self.generation || finder.query != query {
                continue;
            }
            finder.results = Some(result.map(|(sig, matches)| {
                finder.suggested = library::color_name(&sig.palette);
                matches
            }));
        }
    }

    /// A small texture for a library result, loaded in the background.
    fn thumb(&mut self, ctx: &egui::Context, path: &Path) -> Option<egui::TextureId> {
        if !self.thumbs.contains_key(path) {
            self.thumbs.insert(path.to_path_buf(), Texture::Loading);
            let (tx, ctx, path) = (self.thumb_tx.clone(), ctx.clone(), path.to_path_buf());
            std::thread::spawn(move || {
                let result = image::open(&path).map_err(|e| e.to_string()).map(|img| {
                    let rgba = img.thumbnail(220, 220).to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())
                });
                let _ = tx.send((path, result));
                ctx.request_repaint();
            });
        }
        match &self.thumbs[path] {
            Texture::Ready(tex) => Some(tex.id()),
            _ => None,
        }
    }

    fn receive_thumbs(&mut self, ctx: &egui::Context) {
        while let Ok((path, result)) = self.thumb_rx.try_recv() {
            let tex = match result {
                Ok(img) => Texture::Ready(ctx.load_texture(
                    path.display().to_string(),
                    img,
                    TextureOptions::LINEAR,
                )),
                Err(err) => Texture::Failed(err),
            };
            self.thumbs.insert(path, tex);
        }
    }

    fn finder_window(&mut self, ctx: &egui::Context, canvas: Rect) {
        let Some(mut finder) = self.finder.take() else {
            return;
        };
        let mut open = true;
        let mut add: Option<std::path::PathBuf> = None;
        egui::Window::new(tr("Find more"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(372.0)
            .anchor(Align2::RIGHT_TOP, vec2(-12.0, 12.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut finder.mode, FindMode::Values, tr("Similar values"));
                    ui.selectable_value(&mut finder.mode, FindMode::Palette, tr("Similar palette"));
                    ui.selectable_value(&mut finder.mode, FindMode::Web, tr("Search the web"));
                });
                ui.separator();
                match finder.mode {
                    FindMode::Values | FindMode::Palette => {
                        add = self.finder_results(ui, &finder);
                    }
                    FindMode::Web => self.finder_web(ui, &mut finder),
                }
            });
        if let Some(path) = add {
            self.add_from_library(canvas, finder.query, &path);
        }
        if open {
            self.finder = Some(finder);
        }
    }

    /// The ranked thumbnails. Returns a result the user clicked, to copy onto this board.
    fn finder_results(&mut self, ui: &mut egui::Ui, finder: &Finder) -> Option<std::path::PathBuf> {
        let what = if finder.mode == FindMode::Values {
            tr("value structure")
        } else {
            tr("colors")
        };
        ui.label(
            egui::RichText::new(trf(
                "Images on your other boards ranked by {}. Click one to copy it here.",
                &[&what],
            ))
            .small()
            .weak(),
        );
        let matches = match &finder.results {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr("Scanning your boards…"));
                });
                return None;
            }
            Some(Err(err)) => {
                ui.label(trf("Couldn't read this image: {}", &[&err]));
                return None;
            }
            Some(Ok(matches)) if matches.is_empty() => {
                ui.label(tr(
                    "No images on your other boards yet. Make more boards with Ctrl+O and drop \
                     references in; they'll show up here.",
                ));
                return None;
            }
            Some(Ok(matches)) => matches,
        };
        let score = |m: &library::Match| match finder.mode {
            FindMode::Palette => m.palette,
            _ => m.values,
        };
        let mut ranked: Vec<&library::Match> = matches.iter().collect();
        ranked.sort_by(|a, b| score(b).total_cmp(&score(a)));
        let mut clicked = None;
        let ctx = ui.ctx().clone();
        egui::ScrollArea::vertical()
            .max_height(440.0)
            .show(ui, |ui| {
                egui::Grid::new("find-results")
                    .spacing(vec2(8.0, 8.0))
                    .show(ui, |ui| {
                        for (i, m) in ranked.iter().take(30).enumerate() {
                            ui.vertical(|ui| {
                                let size = Vec2::splat(108.0);
                                let response = match self.thumb(&ctx, &m.path) {
                                    Some(tex) => ui.add(
                                        egui::Button::image(
                                            egui::Image::new((tex, size))
                                                .fit_to_exact_size(size)
                                                .maintain_aspect_ratio(true),
                                        )
                                        .frame(false),
                                    ),
                                    None => ui.add_sized(size, egui::Spinner::new()),
                                };
                                if response
                                    .on_hover_text(trf(
                                        "From {}\nClick to copy onto this board",
                                        &[&boards::display_name(&m.board)],
                                    ))
                                    .clicked()
                                {
                                    clicked = Some(m.path.clone());
                                }
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:.0}% · {}",
                                        score(m) * 100.0,
                                        boards::display_name(&m.board)
                                    ))
                                    .small(),
                                );
                            });
                            if i % 3 == 2 {
                                ui.end_row();
                            }
                        }
                    });
            });
        clicked
    }

    fn finder_web(&mut self, ui: &mut egui::Ui, finder: &mut Finder) {
        ui.label(
            egui::RichText::new(tr("Opens an image search in your browser."))
                .small()
                .weak(),
        );
        ui.add(
            egui::TextEdit::singleline(&mut finder.subject)
                .hint_text(tr("Subject, e.g. oak tree at dusk"))
                .desired_width(340.0),
        );
        let tone = finder.tone.unwrap_or(Tone::Color(finder.suggested));
        let label = |t: Tone| match t {
            Tone::Any => tr("Any color").to_string(),
            Tone::BlackAndWhite => tr("Black & white").to_string(),
            Tone::Color(c) if c == finder.suggested => trf("{} (from this image)", &[&c]),
            Tone::Color(c) => c.to_string(),
        };
        let mut chosen = tone;
        ui.horizontal(|ui| {
            ui.label(tr("Color"));
            egui::ComboBox::from_id_salt("web-tone")
                .selected_text(label(tone))
                .show_ui(ui, |ui| {
                    let mut options = vec![
                        Tone::Color(finder.suggested),
                        Tone::BlackAndWhite,
                        Tone::Any,
                    ];
                    options.extend(
                        library::COLOR_NAMES
                            .iter()
                            .filter(|&&c| c != finder.suggested)
                            .map(|&c| Tone::Color(c)),
                    );
                    for option in options {
                        ui.selectable_value(&mut chosen, option, label(option));
                    }
                });
        });
        if chosen != tone {
            finder.tone = Some(chosen);
        }
        ui.add_space(4.0);
        let ready = !finder.subject.trim().is_empty();
        ui.horizontal_wrapped(|ui| {
            for site in library::Site::ALL {
                if ui
                    .add_enabled(ready, egui::Button::new(site.label()))
                    .clicked()
                {
                    let url = library::search_url(site, &finder.subject, chosen);
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            }
        });
        if !ready {
            ui.label(
                egui::RichText::new(tr("Type a subject to search."))
                    .small()
                    .weak(),
            );
        }
    }

    /// Copies a library image onto this board, next to the image it was found from.
    fn add_from_library(&mut self, canvas: Rect, query: u64, path: &Path) {
        let Ok(bytes) = std::fs::read(path) else {
            self.toast(trf("Can't read {}", &[&path.display()]));
            return;
        };
        let at = self
            .board
            .items
            .iter()
            .find(|i| i.id == query)
            .map(|i| {
                let r = self.item_screen_rect(canvas, i);
                r.right_center() + vec2(r.width() * 0.6, 0.0)
            })
            .unwrap_or(canvas.center());
        self.import_all(canvas, at, vec![(bytes, Some(path))]);
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
        egui::Window::new(tr("Boards"))
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

                ui.label(tr("New board"));
                ui.horizontal(|ui| {
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.new_board_name)
                            .hint_text(tr("Name, e.g. Character refs"))
                            .desired_width(220.0),
                    );
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    let name = boards::folder_name(&self.new_board_name);
                    if (ui
                        .add_enabled(name.is_some(), egui::Button::new(tr("Create")))
                        .clicked()
                        || enter)
                        && let Some(name) = name
                    {
                        open = Some(boards::boards_root().join(name));
                    }
                });
                if ui.button(tr("Open or create in a folder…")).clicked()
                    && let Some(dir) = rfd::FileDialog::new()
                        .set_title(tr("Board folder"))
                        .pick_folder()
                {
                    open = Some(dir);
                }
                ui.separator();

                ui.label(tr("Recent"));
                let others: Vec<_> = self.recent.iter().filter(|p| **p != current).collect();
                if others.is_empty() {
                    ui.label(egui::RichText::new(tr("No other boards yet")).weak());
                }
                for dir in others {
                    let response = ui
                        .button(boards::display_name(dir))
                        .on_hover_text(dir.display().to_string());
                    if response.clicked() {
                        open = Some(dir.clone());
                    }
                }
                ui.separator();
                self.watch_controls(ui);
                ui.separator();
                let animations = ui
                    .checkbox(&mut self.animations, tr("Animations"))
                    .on_hover_text(tr(
                        "Glides, pops, swing and flicks. Turn off for a still board.",
                    ))
                    .changed();
                let minimap = ui
                    .checkbox(&mut self.minimap, tr("Mini-map"))
                    .on_hover_text(tr(
                        "An overview of the whole board in the corner, while part of it is off \
                         screen. Click or drag in it to go there.",
                    ))
                    .changed();
                if animations || minimap {
                    self.save_settings();
                }
                if animations && !self.animations {
                    self.reset_motion();
                }
                self.update_controls(ui);
                ui.add_space(6.0);
                close |= ui.button(tr("Close")).clicked();
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
        let mut store = match Store::open(dir.clone()) {
            Ok(store) => store,
            Err(err) => {
                self.toast(trf("Can't open {}: {}", &[&dir.display(), &err]));
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
        self.draw_mode = false;
        self.radial = None;
        self.finder = None;
        self.reset_motion();
        self.focus = None;
        self.watch_pending.clear();
        self.watch_error = None;
        self.dirty_since = None;
        self.show_help = self.board.items.is_empty() && self.store.unreadable.is_none();
        self.window_level_sent = false;
        self.new_board_name.clear();
        boards::remember(&self.store.dir);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "Core Board {} — {}",
            env!("CARGO_PKG_VERSION"),
            self.store.dir.display()
        )));
        self.toast(trf("Opened {}", &[&boards::display_name(&self.store.dir)]));
    }

    // ---------------------------------------------------------------- persistence

    fn save_now(&mut self) {
        match self.store.save(&self.board) {
            Ok(()) => self.dirty_since = None,
            Err(err) => self.toast(trf("Save failed: {}", &[&err])),
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
            // Pictures go on the board as images; anything else (an Illustrator file, a video,
            // a shortcut, a folder) as a card that opens it. Only the start of each file is
            // read to tell, so dropping a big video doesn't load it all.
            let mut sources = Vec::new();
            let mut others = Vec::new();
            for file in &dropped {
                let path = file.path();
                if looks_like_picture(path) {
                    match file.bytes() {
                        Ok(bytes) => sources.push((bytes, Some(path))),
                        Err(err) => eprintln!("coreboard: {}: {err}", path.display()),
                    }
                } else {
                    others.push(path.to_path_buf());
                }
            }
            let added = self.import_all(canvas, anchor, sources);
            // PDFs land as their pages; everything else as cards.
            let (pdfs, others): (Vec<_>, Vec<_>) =
                others.into_iter().partition(|p| pdfs::is_pdf_file(p));
            for pdf in pdfs {
                self.import_pdf(canvas, anchor, pdf);
            }
            if !others.is_empty() {
                let at = anchor + vec2(0.0, 40.0);
                self.join_cards = added > 0;
                self.add_file_cards(canvas, at, others);
            }
        }

        // Keyboard, unless a text field (note, frame title, dialog) is taking typing.
        let mut paste_requested = None;
        // Ctrl+C can arrive as a copy event, a key press or both: copy once.
        let mut copy_requested = false;
        let typing = ctx.egui_wants_keyboard_input();
        for event in events.iter().filter(|_| !typing) {
            match event {
                Event::Paste(text) => paste_requested = Some(Some(text.clone())),
                Event::Copy => copy_requested = true,
                // Hold Q for the radial menu; releasing it picks the slice pointed at.
                Event::Key {
                    key: Key::Q,
                    pressed,
                    repeat,
                    modifiers: m,
                    ..
                } if !m.command => {
                    if *pressed && !*repeat && self.radial.is_none() {
                        self.open_radial(canvas, anchor);
                    } else if !*pressed && self.radial.is_some() {
                        if let Some(action) = self.radial_choice(pointer.hover_pos()) {
                            self.run_radial(&ctx, canvas, action);
                        }
                        self.radial = None;
                    }
                }
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
                    // Modified keys first, so they aren't caught by the plain-key arms below.
                    Key::L if !m.command && m.shift => self.toggle_pin(canvas),
                    Key::L if !m.command => self.toggle_lock(),
                    Key::C if m.command => copy_requested = true,
                    Key::G if m.command && m.shift => self.toggle_group(),
                    Key::T if m.command => self.open_tags_window(),
                    Key::D if m.command => self.duplicate_selection(),
                    Key::F if m.command => self.open_search(),
                    Key::ArrowLeft if m.alt => self.align_selection(Align::Left),
                    Key::ArrowRight if m.alt => self.align_selection(Align::Right),
                    Key::ArrowUp if m.alt => self.align_selection(Align::Top),
                    Key::ArrowDown if m.alt => self.align_selection(Align::Bottom),
                    Key::C if m.alt => self.align_selection(Align::CenterX),
                    Key::M if m.alt => self.align_selection(Align::CenterY),
                    Key::H if m.alt => self.distribute_selection(true),
                    Key::V if m.alt => self.distribute_selection(false),
                    Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown
                        if !m.command =>
                    {
                        self.nudge(*key, m.shift)
                    }
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
                        self.toast(tr("Saved").into());
                    }
                    Key::F | Key::Home => self.fit(canvas),
                    Key::R if !m.command => self.rotate_selection(if m.shift { -1 } else { 1 }),
                    Key::G if !m.command => self.arrange_grid(m.shift),
                    Key::M if !m.command => self.toggle_selection(|i| &mut i.flip_x),
                    Key::B if !m.command && m.shift => self.cycle_board_values(),
                    Key::B if !m.command => self.toggle_selection(|i| &mut i.grayscale),
                    Key::V if !m.command => self.cycle_analysis(!m.shift),
                    Key::C if !m.command && m.shift => self.turn_spiral(),
                    Key::C if !m.command => self.cycle_guide(),
                    Key::P if !m.command => self.extract_palettes(if m.shift { 10 } else { 6 }),
                    Key::E if !m.command && !m.shift => self.add_placeholder(canvas, anchor),
                    Key::N if !m.command && m.shift => self.cycle_note_color(),
                    Key::N if !m.command => self.add_note(canvas, anchor),
                    Key::G if m.command => self.add_frame(canvas, anchor),
                    Key::X if !m.command && m.shift => self.reset_crop(canvas),
                    Key::X if !m.command => self.toggle_crop_mode(),
                    Key::Enter if self.crop_mode.is_some() => self.toggle_crop_mode(),
                    Key::S if !m.command && !m.shift => self.surprise(&ctx),
                    Key::Enter if !m.command && self.editing.is_none() => {
                        self.open_focus(canvas, hover);
                    }
                    Key::Escape if self.crop_mode.is_some() => self.toggle_crop_mode(),
                    Key::Escape if self.radial.is_some() => self.radial = None,
                    Key::Escape if self.draw_mode => self.toggle_draw_mode(),
                    Key::D if !m.command && m.shift => {
                        self.show_marks = !self.show_marks;
                        self.toast(
                            if self.show_marks {
                                tr("Drawings shown")
                            } else {
                                tr("Drawings hidden")
                            }
                            .into(),
                        );
                    }
                    Key::D if !m.command => self.toggle_draw_mode(),
                    Key::I if !m.command => {
                        self.picker = !self.picker;
                        self.toast(
                            if self.picker {
                                tr("Color picker on: hover an image")
                            } else {
                                tr("Color picker off")
                            }
                            .into(),
                        );
                    }
                    Key::T if !m.command => {
                        let on = !self.board.window.always_on_top;
                        self.board.window.always_on_top = on;
                        self.window_level_sent = false;
                        self.toast(
                            if on {
                                tr("Always on top: on")
                            } else {
                                tr("Always on top: off")
                            }
                            .into(),
                        );
                        self.touch();
                    }
                    Key::OpenBracket | Key::CloseBracket => {
                        let step = if *key == Key::OpenBracket { -0.1 } else { 0.1 };
                        // Round to whole steps so repeated presses land on 10%, 20%, ...
                        let opacity = ((self.board.window.opacity + step) * 10.0).round() / 10.0;
                        self.board.window.opacity = opacity.clamp(0.1, 1.0);
                        self.toast(trf(
                            "Opacity {}%",
                            &[&(self.board.window.opacity * 100.0).round()],
                        ));
                        self.touch();
                    }
                    Key::Num1 if !m.command => {
                        // Work out the 100% view, then fly there.
                        let before = self.board.view.clone();
                        self.zoom_around(canvas, anchor, 1.0);
                        let target = std::mem::replace(&mut self.board.view, before);
                        self.fly_view(target);
                    }
                    Key::H | Key::F1 => self.show_help = !self.show_help,
                    Key::Escape => self.selection.clear(),
                    _ => {}
                },
                _ => {}
            }
        }
        if copy_requested {
            self.copy_selection();
        }
        if let Some(text) = paste_requested {
            self.paste(canvas, anchor, text);
        }

        // Zoom: wheel or trackpad pinch, anchored at the cursor.
        // (While the magnifier shows, the wheel sets its power instead.)
        if self.loupe_held(&ctx) {
            if scroll.y != 0.0 {
                self.loupe_scroll(scroll.y);
            }
        } else if let Some(pos) = hover.filter(|p| !over_ui(&ctx, *p)) {
            let factor = (scroll.y * 0.0025).exp() * pinch;
            if (factor - 1.0).abs() > f32::EPSILON {
                self.stop_motion();
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
            } if canvas.contains(*pos)
                && !self.edit_rect.is_some_and(|r| r.contains(*pos))
                && !over_ui(&ctx, *pos) =>
            {
                Some((*pos, *button))
            }
            _ => None,
        });
        // With the radial menu open, a click picks a slice (or closes it) instead.
        let press = match (self.radial.is_some(), press) {
            (true, Some((pos, _))) => {
                if let Some(action) = self.radial_choice(Some(pos)) {
                    self.run_radial(&ctx, canvas, action);
                }
                self.radial = None;
                None
            }
            (_, press) => press,
        };
        if press.is_some() {
            // Grabbing the board stops anything coasting or flying.
            self.stop_motion();
        }
        if let Some((pos, button)) = press
            && matches!(self.drag, Drag::None)
        {
            let space = ctx.input(|i| i.key_down(Key::Space));
            self.drag = if button != egui::PointerButton::Primary || space {
                Drag::Pan
            } else if self.draw_mode {
                // Drawing starts anywhere: on a picture it sticks to it, elsewhere it becomes
                // its own drawing object.
                Drag::Draw {
                    item: self.drawable_at(canvas, pos),
                    points: vec![pos],
                }
            } else if self.crop_mode.is_some() {
                Drag::Crop { start: pos }
            } else if let Some(id) = self.hit_handle(canvas, pos) {
                let item = self.board.items.iter().find(|i| i.id == id).unwrap();
                let aspect = match item.kind {
                    Kind::Image | Kind::Palette { .. } | Kind::File { .. } | Kind::Link { .. } => {
                        Some(item.size[0] / item.size[1])
                    }
                    Kind::Note { .. }
                    | Kind::Frame { .. }
                    | Kind::Placeholder { .. }
                    | Kind::Drawing { .. } => None,
                };
                self.checkpoint();
                Drag::Resize { id, aspect }
            } else if let Some(id) = self.hit(canvas, pos) {
                // A grouped item brings its whole group.
                let members = self.group_of(id);
                if modifiers.shift || modifiers.command {
                    if self.selection.contains(&id) {
                        for m in &members {
                            self.selection.remove(m);
                        }
                    } else {
                        self.selection.extend(members);
                    }
                } else if !self.selection.contains(&id) {
                    self.selection = members.into_iter().collect();
                }
                self.checkpoint();
                self.raise_selection();
                // Everything that moves in world space: the movable selection plus what a
                // dragged frame carries. Pinned items move in screen space; locked ones stay.
                let carried = self.carried_by_selected_frames();
                let start = self
                    .board
                    .items
                    .iter()
                    .filter(|i| {
                        i.movable() && (self.selection.contains(&i.id) || carried.contains(&i.id))
                    })
                    .map(|i| (i.id, i.pos))
                    .collect();
                Drag::Move {
                    moved: false,
                    start,
                    total: Vec2::ZERO,
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
            Drag::None | Drag::Marquee { .. } | Drag::Crop { .. } | Drag::Draw { .. } => {}
            Drag::Pan => {
                self.board.view.offset[0] += delta.x;
                self.board.view.offset[1] += delta.y;
                if delta != Vec2::ZERO {
                    self.touch();
                }
            }
            Drag::Move { moved, total, .. } => {
                if delta != Vec2::ZERO {
                    *moved = true;
                    *total += delta / zoom;
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
                        if let Some(pin) = &mut item.pinned {
                            pin[2] = item.size[0] * zoom;
                            pin[3] = item.size[1] * zoom;
                        }
                    }
                    self.touch();
                }
            }
        }

        if matches!(self.drag, Drag::Draw { .. })
            && let Some(pos) = pointer.interact_pos()
        {
            self.extend_stroke(pos);
        }

        // Moves snap their group's edges and centers to nearby items (hold Alt to move freely).
        if let Drag::Move {
            moved: true,
            start,
            total,
        } = &self.drag
            && delta != Vec2::ZERO
        {
            let (start, total) = (start.clone(), *total);
            let snapped = self.snap_move(canvas, &start, total, !modifiers.alt);
            for item in &mut self.board.items {
                if let Some([x, y]) = start.get(&item.id) {
                    item.pos = [x + snapped.x, y + snapped.y];
                } else if self.selection.contains(&item.id)
                    && !item.locked
                    && let Some(pin) = &mut item.pinned
                {
                    pin[0] += delta.x;
                    pin[1] += delta.y;
                }
            }
            self.touch();
        }

        // Finish the drag.
        if !pointer.any_down() {
            self.snap_lines.clear();
            // Releasing a dragged image over an empty slot puts it in the slot.
            if let Some(slot) = self.slot_drop_target(&ctx, canvas)
                && let Some(image) = self.single_selected_image()
            {
                self.fill_slot_with_item(slot, image);
            }
            match std::mem::replace(&mut self.drag, Drag::None) {
                Drag::Move {
                    moved: true, start, ..
                } => {
                    // Only what actually moved can be thrown (locked items stay put; an empty
                    // list would mean "the board").
                    let ids: Vec<u64> = start.into_keys().collect();
                    if !ids.is_empty() {
                        self.release_flick(&ctx, ids);
                    }
                }
                Drag::Pan => self.release_flick(&ctx, Vec::new()),
                Drag::Move { moved: false, .. } => {
                    // A plain click: drop the no-op history entry and collapse to one item.
                    self.undo.pop();
                    if !(modifiers.shift || modifiers.command)
                        && let Some(id) = pointer.interact_pos().and_then(|p| self.hit(canvas, p))
                    {
                        self.selection = self.group_of(id).into_iter().collect();
                    }
                }
                Drag::Draw { item, points } => self.finish_stroke(canvas, item, points),
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
                // Ctrl+click (no drag) on an empty spot: add a link there.
                Drag::Marquee { start, additive }
                    if additive
                        && modifiers.command
                        && !modifiers.shift
                        && pointer
                            .interact_pos()
                            .is_some_and(|end| end.distance(start) < 4.0) =>
                {
                    self.open_link_prompt(canvas, start);
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
                        self.select_whole_groups();
                    }
                }
                _ => {}
            }
        }

        // Double-click a note, or a frame's title, to edit its text.
        if pointer.button_double_clicked(egui::PointerButton::Primary)
            && let Some(pos) = pointer.interact_pos().filter(|p| canvas.contains(*p))
            && !self.edit_rect.is_some_and(|r| r.contains(pos))
            && !over_ui(&ctx, pos)
            && !self.draw_mode
            && self.crop_mode.is_none()
            && let Some(id) = self.hit(canvas, pos)
        {
            if self.board.items.iter().any(|i| i.id == id && i.is_card()) {
                self.open_card(&ctx, id);
            } else {
                self.begin_edit(id, false);
            }
        }
    }

    fn paint(&mut self, ui: &egui::Ui, canvas: Rect) {
        let ctx = ui.ctx().clone();
        let painter = ui.painter_at(canvas);
        // Opacity fades the board itself; labels and overlays stay fully readable.
        let opacity = self.board.window.opacity;
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
        let drop_target = self.slot_drop_target(&ctx, canvas);
        let mut note_heights = Vec::new();
        let hover = ctx.input(|i| i.pointer.hover_pos());
        let mut tag_rows = Vec::new();
        let mut badge_rows = Vec::new();
        for item in &items {
            let (rect, appear) = self.shown_rect(canvas, item);
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
            let opacity = opacity * self.search_dim(item) * appear;
            let (tilt, lift) = self.swing_of(item.id);
            let tint = Color32::WHITE.gamma_multiply(opacity);
            let handle = !item.locked;
            let caption_h = if item.is_image() && !item.caption.trim().is_empty() && !editing {
                let font = board::CAPTION_FONT * zoom;
                let galley = painter.layout(
                    item.caption.clone(),
                    FontId::proportional(font.max(1.0)),
                    Color32::from_gray(205).gamma_multiply(opacity),
                    rect.width().max(1.0),
                );
                let h = board::CAPTION_GAP * zoom + galley.size().y;
                if font >= 5.0 {
                    let pos = pos2(rect.min.x, rect.max.y + board::CAPTION_GAP * zoom);
                    painter.galley(pos, galley, Color32::from_gray(205));
                }
                h
            } else {
                0.0
            };
            if !item.tags.is_empty() && (selected || hover.is_some_and(|p| rect.contains(p))) {
                // Tag chips go under the caption.
                let below = Rect::from_min_max(rect.min, rect.max + vec2(0.0, caption_h));
                tag_rows.push((below, item.tags.clone()));
            }
            if item.locked || item.pinned.is_some() {
                badge_rows.push((rect, item.clone()));
            }
            match &item.kind {
                Kind::Image => {}
                Kind::Palette { colors } => {
                    let colors = self.view_palette(colors);
                    draw_palette(&painter, rect, &colors, opacity);
                    if selected {
                        draw_selection(&painter, rect, handle);
                    }
                    continue;
                }
                Kind::Note { text, color } => {
                    draw_note(&painter, rect, text, *color, zoom, opacity, editing);
                    if selected {
                        draw_selection(&painter, rect, handle);
                    }
                    continue;
                }
                Kind::Frame { title } => {
                    let title_rect = self.frame_title_rect(canvas, item);
                    draw_frame(&painter, rect, title_rect, title, zoom, opacity, editing);
                    if selected {
                        draw_selection(&painter, rect, handle);
                    }
                    continue;
                }
                Kind::Drawing { mark } => {
                    if self.show_marks {
                        painter.extend(mark_shapes(rect, mark, zoom, opacity));
                    }
                    if selected {
                        draw_selection(&painter, rect, handle);
                    }
                    continue;
                }
                Kind::File { .. } | Kind::Link { .. } => {
                    let preview = if item.file.is_empty() {
                        None
                    } else {
                        match self.texture(&ctx, &item.file, Filter::Plain) {
                            Texture::Ready(tex) => Some((tex.id(), tex.size_vec2())),
                            _ => None,
                        }
                    };
                    if let Some(face) = item.kind.card_face() {
                        let video = matches!(&item.kind, Kind::Link { url, .. } if crate::web::is_video(url));
                        draw_card(&painter, rect, &face, opacity, preview, video);
                    }
                    if selected {
                        draw_selection(&painter, rect, handle);
                    }
                    continue;
                }
                Kind::Placeholder { label } => {
                    let highlight = drop_target == Some(item.id);
                    draw_placeholder(&painter, rect, label, zoom, opacity, highlight, editing);
                    if selected {
                        draw_selection(&painter, rect, handle);
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
            let filter = match self.board_values {
                1 => Filter::Gray,
                2 => Filter::Study(Analysis::Values3),
                _ => Filter::for_item(item),
            };
            match self.texture(&ctx, &item.file, filter) {
                Texture::Ready(tex) => {
                    let tex = tex.id();
                    if cropping {
                        // The parts being cut away, dimmed, around the kept part.
                        let dim = Color32::from_gray(90).gamma_multiply(opacity);
                        let whole = Item {
                            crop: [0.0, 0.0, 1.0, 1.0],
                            ..item.clone()
                        };
                        painter.add(image_shape(tex, full, &whole, dim, 0.0));
                    }
                    if lift > 0.01 {
                        // Lifted off the board: a soft shadow below.
                        for (grow, alpha) in [(14.0, 30.0), (7.0, 45.0), (2.0, 60.0)] {
                            painter.rect_filled(
                                rect.translate(vec2(0.0, 10.0 * lift)).expand(grow * lift),
                                CornerRadius::same((grow * lift) as u8),
                                Color32::from_black_alpha((alpha * lift) as u8),
                            );
                        }
                    }
                    painter.add(image_shape(tex, rect, item, tint, tilt));
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
            if self.show_marks && !item.marks.is_empty() {
                // Drawings on parts that are cropped away stay hidden.
                let clipped = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
                for mark in &item.marks {
                    clipped.extend(item_mark_shapes(rect, item, mark, zoom, opacity));
                }
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
            // The straight outline would look off on a swinging image; it's back once it settles.
            if self.selection.contains(&item.id) && tilt.abs() < 0.01 {
                draw_selection(&painter, rect, handle);
            }
        }

        self.paint_group_outlines(&painter, canvas);
        for (rect, item) in badge_rows {
            self.draw_badges(&painter, rect, &item);
        }
        for (rect, tags) in tag_rows {
            self.draw_tags(&painter, rect, &tags);
        }
        self.paint_snap_lines(&painter, canvas);
        if let Drag::Draw { item, points } = &self.drag {
            let rect = item
                .and_then(|id| self.board.items.iter().find(|i| i.id == id))
                .map_or(canvas, |target| self.item_screen_rect(canvas, target));
            painter.extend(preview_mark(
                rect,
                self.draw_tool,
                self.draw_color,
                points,
                zoom,
            ));
        }
        // The slot under a dragged image is hidden by it, so mark it on top of everything.
        if let Some(slot) = drop_target.and_then(|id| self.board.items.iter().find(|i| i.id == id))
        {
            let rect = self.item_screen_rect(canvas, slot);
            painter.rect_stroke(
                rect.expand(3.0),
                CornerRadius::same(6),
                Stroke::new(3.0, ACCENT),
                StrokeKind::Outside,
            );
            let label = match &slot.kind {
                Kind::Placeholder { label } if !label.trim().is_empty() => {
                    trf("Drop to fill “{}”", &[&label.trim()])
                }
                _ => tr("Drop to fill this slot").to_string(),
            };
            let galley = painter.layout_no_wrap(label, FontId::proportional(13.0), Color32::WHITE);
            let pos = rect.center_top() - vec2(galley.size().x / 2.0, galley.size().y + 12.0);
            painter.rect_filled(
                Rect::from_min_size(pos, galley.size()).expand(5.0),
                CornerRadius::same(5),
                ACCENT,
            );
            painter.galley(pos, galley, Color32::WHITE);
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

        if self.board.items.is_empty() && self.store.unreadable.is_none() {
            painter.text(
                canvas.center(),
                Align2::CENTER_CENTER,
                tr("Drop images or files here, paste with Ctrl+V, or press N for a note"),
                FontId::proportional(22.0),
                Color32::from_gray(120),
            );
        }

        // Status line.
        // (The zoom shows in the zoom controls just left of this.)
        let status = format!(
            "{}{}{}{}",
            trf("{} items", &[&self.board.items.len()]),
            if opacity < 1.0 {
                format!("  ·  {}", trf("opacity {}%", &[&(opacity * 100.0).round()]))
            } else {
                String::new()
            },
            if self.board.window.always_on_top {
                format!("  ·  {}", tr("on top"))
            } else if self.draw_mode {
                format!("  ·  {}", tr("DRAW: drag anywhere, Esc when done"))
            } else if self.board_values == 1 {
                format!("  ·  {}", tr("VALUES: whole board in grayscale (Shift+B)"))
            } else if self.board_values == 2 {
                format!("  ·  {}", tr("VALUES: whole board in 3 tones (Shift+B)"))
            } else if self.crop_mode.is_some() {
                format!("  ·  {}", tr("CROP: drag over the image, Enter when done"))
            } else {
                String::new()
            },
            if self.selection.is_empty() {
                String::new()
            } else {
                format!("  ·  {}", trf("{} selected", &[&self.selection.len()]))
            },
        );
        painter.text(
            canvas.left_bottom() + vec2(ZOOM_CONTROLS_WIDTH + 22.0, -8.0),
            Align2::LEFT_BOTTOM,
            status,
            FontId::monospace(12.0),
            Color32::from_gray(140),
        );

        self.help_rect = self.show_help.then(|| paint_help(&painter, canvas));

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

impl CoreBoardApp {
    /// "Exported …" with Open folder and ✕, until it's closed (clicking it opens the folder).
    fn export_notice_window(&mut self, ctx: &egui::Context) {
        let Some(path) = self.export_notice.clone() else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut open = false;
        let mut close = false;
        egui::Area::new(egui::Id::unique("coreboard-export-notice"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -44.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let text = ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new(trf("✓ Exported {}", &[&name]))
                                            .color(Color32::WHITE),
                                    )
                                    .sense(Sense::click()),
                                )
                                .on_hover_text(path.display().to_string());
                            open |= text.clicked();
                            open |= ui
                                .button(egui::RichText::new(tr("Open folder")).strong())
                                .clicked();
                            close = ui.button("✕").on_hover_text(tr("Close")).clicked();
                        });
                    });
            });
        if open {
            let folder = path.parent().map(|p| p.to_path_buf()).unwrap_or(path);
            if let Err(err) = opener::open(&folder) {
                self.toast(trf("Couldn't open it: {}", &[&err]));
            }
        }
        if close {
            self.export_notice = None;
        }
    }

    /// − 43% + ⤢ in the bottom-left corner: zoom out, back to 100%, zoom in, fit everything.
    fn zoom_controls(&mut self, ctx: &egui::Context, canvas: Rect) {
        let mut action = None;
        egui::Area::new(egui::Id::unique("coreboard-zoom"))
            .order(egui::Order::Foreground)
            .fixed_pos(canvas.left_bottom() + vec2(6.0, -32.0))
            .show(ctx, |ui| {
                ui.set_width(ZOOM_CONTROLS_WIDTH);
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.horizontal(|ui| {
                    let small = |text: &str| {
                        egui::Button::new(egui::RichText::new(text).size(16.0))
                            .min_size(vec2(28.0, 24.0))
                    };
                    if ui.add(small("−")).on_hover_text(tr("Zoom out")).clicked() {
                        action = Some(1.0 / 1.25);
                    }
                    let percent = format!("{:.0}%", self.board.view.zoom * 100.0);
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new(percent).monospace().size(12.5))
                                .min_size(vec2(52.0, 24.0))
                                .frame(false),
                        )
                        .on_hover_text(tr("Back to 100%"))
                        .clicked()
                    {
                        action = Some(0.0);
                    }
                    if ui.add(small("+")).on_hover_text(tr("Zoom in")).clicked() {
                        action = Some(1.25);
                    }
                    if ui
                        .add(small("⤢"))
                        .on_hover_text(tr("Fit everything (F)"))
                        .clicked()
                    {
                        action = Some(-1.0);
                    }
                });
            });
        let Some(action) = action else {
            return;
        };
        if action < 0.0 {
            self.fit(canvas);
            return;
        }
        // Work out the new view around the window's center, then glide there.
        let before = self.board.view.clone();
        let target = if action == 0.0 {
            1.0
        } else {
            before.zoom * action
        };
        self.zoom_around(canvas, canvas.center(), target);
        let to = std::mem::replace(&mut self.board.view, before);
        self.fly_view(to);
    }

    /// A close button on the help panel, and a "?" button to open it when it's closed.
    fn help_buttons(&mut self, ctx: &egui::Context, canvas: Rect) {
        let (at, label, tip) = match self.help_rect.filter(|_| self.show_help) {
            Some(panel) => (
                panel.right_top() + vec2(-34.0, 6.0),
                "✕",
                tr("Close the help (H)"),
            ),
            None => (
                canvas.right_top() + vec2(-46.0, 12.0),
                "?",
                tr("Shortcuts and help (H)"),
            ),
        };
        egui::Area::new(egui::Id::unique("coreboard-help-button"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ctx, |ui| {
                let button = egui::Button::new(egui::RichText::new(label).size(15.0).strong())
                    .min_size(vec2(28.0, 28.0))
                    .corner_radius(CornerRadius::same(14));
                if ui.add(button).on_hover_text(tip).clicked() {
                    self.show_help = !self.show_help;
                }
            });
    }

    /// Editors, toolbars and menus that sit over the board (hidden while focus view is open).
    fn board_overlays(&mut self, ctx: &egui::Context, canvas: Rect) {
        self.edit_overlay(ctx, canvas);
        self.finder_window(ctx, canvas);
        self.search_bar(ctx, canvas);
        self.draw_toolbar(ctx);
        if self.draw_mode
            && ctx
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|p| canvas.contains(p) && !over_ui(ctx, p))
        {
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        self.tags_window(ctx);
        self.align_toolbar(ctx, canvas);
        self.paint_radial(ctx);
        self.link_prompt_window(ctx, canvas);
        self.minimap(ctx, canvas);
        self.help_buttons(ctx, canvas);
        self.zoom_controls(ctx, canvas);
        self.export_notice_window(ctx);
        self.paint_loupe(ctx, canvas);
        self.update_banner(ctx, canvas);
        self.unreadable_banner(ctx, canvas);
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
        self.sync_pinned();
        // Claim the pointer so egui treats the whole window as one draggable surface.
        let response = ui.interact(
            canvas,
            ui.scope_id().with("canvas"),
            Sense::click_and_drag(),
        );
        // Dialogs are modal: the board ignores input while one is open, and while focus view
        // is open only it gets input.
        if self.focus_open() {
            if !self.show_export && !self.show_boards {
                self.focus_input(&ctx, canvas, &response);
            }
        } else if !self.show_export && !self.show_boards && !self.show_tags {
            self.handle_input(ui, canvas);
            self.context_menu(&ctx, canvas, &response);
        }
        // After input, so a change made this frame (undo, align, grid...) glides from where
        // things were drawn last frame instead of jumping.
        self.update_motion(&ctx, canvas);
        self.paint(ui, canvas);
        if self.focus_open() {
            self.paint_focus(&ctx, canvas);
        } else {
            self.board_overlays(&ctx, canvas);
        }
        self.export_dialog(&ctx);
        self.boards_window(&ctx);
        self.receive_finds();
        self.receive_thumbs(&ctx);
        self.receive_fetches(&ctx, canvas);
        self.receive_pdfs(&ctx, canvas);
        self.request_previews();
        self.receive_previews();
        self.receive_updates(&ctx);
        self.receive_exports();
        self.receive_palettes();
        self.poll_watch(&ctx);

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
        (tr("Empty note"), rgb(ink).gamma_multiply(0.45))
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
fn draw_selection(painter: &egui::Painter, rect: Rect, handle: bool) {
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
    if handle {
        painter.rect_filled(
            CoreBoardApp::handle_rect(rect),
            CornerRadius::same(2),
            ACCENT,
        );
    }
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
/// `tilt` turns the drawn quad around its center (radians), for an image swinging while dragged.
fn image_shape(
    texture: egui::TextureId,
    rect: Rect,
    item: &Item,
    tint: Color32,
    tilt: f32,
) -> egui::Shape {
    let (quarters, flip_x) = (item.rotation % 4, item.flip_x);
    let center = rect.center();
    let (sin, cos) = tilt.sin_cos();
    let turn = |p: Pos2| {
        let d = p - center;
        center + vec2(d.x * cos - d.y * sin, d.x * sin + d.y * cos)
    };
    let corners = [
        turn(rect.left_top()),
        turn(rect.right_top()),
        turn(rect.right_bottom()),
        turn(rect.left_bottom()),
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

/// The shortcuts overlay (H), grouped by what they're for.
const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "Add & move around",
        &[
            ("Drag & drop / Ctrl+V", "add images, or any file as a card"),
            ("Ctrl+V a web link", "download its picture (or add a link)"),
            ("Ctrl+click empty spot", "add a link card"),
            ("Double-click a card", "open the file or link"),
            ("Scroll / pinch", "zoom"),
            ("Right/middle/Space drag", "pan"),
            ("F / 1", "fit to view / zoom 100%"),
            ("Drag empty space", "box select"),
            ("Shift+click", "add to / remove from selection"),
            ("Corner handle", "resize"),
            ("Del", "remove"),
            ("Ctrl+Z / Ctrl+Shift+Z", "undo / redo"),
            ("Ctrl+D", "duplicate"),
            ("Ctrl+C", "copy as a picture, to paste in any app"),
        ],
    ),
    (
        "Arrange",
        &[
            ("G / Shift+G", "grid arrange / same height"),
            ("Alt+arrows, Alt+C / M", "align"),
            ("Alt+H / Alt+V", "space evenly"),
            ("Alt while dragging", "move without snapping"),
            ("Arrows / Shift+arrows", "nudge 1 / 10 points"),
            ("L / Shift+L", "lock / pin to screen"),
            ("Ctrl+G", "frame around selection"),
            ("Ctrl+Shift+G", "group / ungroup"),
        ],
    ),
    (
        "Images",
        &[
            ("R / Shift+R", "rotate 90° cw / ccw"),
            ("M", "mirror"),
            ("X / Shift+X", "crop (drag) / reset crop"),
            ("Double-click image", "caption"),
            ("Enter", "focus view, ← → to step through"),
            ("S", "surprise me: a random image"),
        ],
    ),
    (
        "Study",
        &[
            ("V / Shift+V", "value, saturation, noise, squint"),
            ("Hold Z", "magnifier (scroll to change its zoom)"),
            ("B", "black & white"),
            ("Shift+B", "whole board in values"),
            ("C / Shift+C", "composition guide / turn spiral"),
            ("I", "color picker (HSV + value)"),
            ("P / Shift+P", "6 / 10 color palette"),
            ("Hold Q", "radial menu: find similar, web…"),
        ],
    ),
    (
        "Notes & drawing",
        &[
            ("N / Shift+N", "new note / note color"),
            ("E", "empty image slot"),
            ("Double-click", "edit a note, frame or slot"),
            ("D / Shift+D", "draw (anywhere) / hide drawings"),
        ],
    ),
    (
        "Boards & window",
        &[
            ("Ctrl+O", "boards, updates, settings"),
            ("Ctrl+T / Ctrl+F", "tags / search"),
            ("Ctrl+E", "export PNG (+ watermark)"),
            ("[ / ]", "window opacity"),
            ("T", "always on top"),
            ("H", "show / hide this help"),
        ],
    ),
];

/// Draws the shortcuts overlay in the top-right corner: sections flow into as many columns
/// as the window's height needs.
/// Returns the panel's rect, for its close button.
fn paint_help(painter: &egui::Painter, canvas: Rect) -> Rect {
    const KEY_FONT: f32 = 12.0;
    const ROW: f32 = 18.0;
    const HEADER: f32 = 26.0;
    const SECTION_GAP: f32 = 12.0;
    const COLUMN_GAP: f32 = 28.0;
    let key_color = Color32::from_rgb(170, 205, 255);
    let text_color = Color32::from_gray(205);
    let key_galley =
        |t: &str| painter.layout_no_wrap(t.to_string(), FontId::monospace(KEY_FONT), key_color);
    let text_galley =
        |t: &str| painter.layout_no_wrap(t.to_string(), FontId::proportional(13.0), text_color);
    // Group sections into columns no taller than the window.
    let max_height = (canvas.height() - 80.0).max(200.0);
    let height_of = |rows: usize| HEADER + rows as f32 * ROW + SECTION_GAP;
    let mut columns: Vec<Vec<usize>> = vec![Vec::new()];
    let mut used = 0.0;
    for (i, (_, rows)) in HELP.iter().enumerate() {
        let h = height_of(rows.len());
        if used + h > max_height && !columns.last().is_some_and(|c| c.is_empty()) {
            columns.push(Vec::new());
            used = 0.0;
        }
        columns.last_mut().expect("one column").push(i);
        used += h;
    }
    // Each column is as wide as its widest key plus its widest description.
    let widths: Vec<(f32, f32)> = columns
        .iter()
        .map(|col| {
            col.iter().flat_map(|&i| HELP[i].1.iter()).fold(
                (0.0f32, 0.0f32),
                |(k, d), (key, desc)| {
                    (
                        k.max(key_galley(tr(key)).size().x),
                        d.max(text_galley(tr(desc)).size().x),
                    )
                },
            )
        })
        .collect();
    let total_w: f32 = widths.iter().map(|(k, d)| k + 14.0 + d).sum::<f32>()
        + COLUMN_GAP * (columns.len() as f32 - 1.0);
    let title = format!(
        "Core Board {}  ·  {}",
        env!("CARGO_PKG_VERSION"),
        tr("shortcuts")
    );
    let title_galley = painter.layout_no_wrap(title, FontId::proportional(15.0), Color32::WHITE);
    let total_h = columns
        .iter()
        .map(|c| c.iter().map(|&i| height_of(HELP[i].1.len())).sum::<f32>())
        .fold(0.0, f32::max)
        + 30.0;
    let origin = pos2(
        (canvas.max.x - total_w - 26.0).max(canvas.min.x + 12.0),
        canvas.min.y + 14.0,
    );
    let bg = Rect::from_min_size(origin, vec2(total_w, total_h)).expand(12.0);
    painter.rect_filled(bg, CornerRadius::same(8), Color32::from_rgb(18, 19, 22));
    painter.galley(origin, title_galley, Color32::WHITE);
    let mut x = origin.x;
    for (col, (key_w, desc_w)) in columns.iter().zip(&widths) {
        let mut y = origin.y + 30.0;
        for &i in col {
            let (name, rows) = HELP[i];
            painter.text(
                pos2(x, y + 4.0),
                Align2::LEFT_TOP,
                tr(name).to_uppercase(),
                FontId::proportional(11.5),
                ACCENT,
            );
            y += HEADER;
            for (key, desc) in rows.iter() {
                painter.galley(pos2(x, y), key_galley(tr(key)), Color32::WHITE);
                painter.galley(
                    pos2(x + key_w + 14.0, y - 1.0),
                    text_galley(tr(desc)),
                    Color32::WHITE,
                );
                y += ROW;
            }
            y += SECTION_GAP;
        }
        x += key_w + 14.0 + desc_w + COLUMN_GAP;
    }
    bg
}

/// Whether the file at `path` is a picture, judged by its first bytes (so a big video isn't
/// read in full just to find out). Folders and unreadable files aren't.
fn looks_like_picture(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 64];
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let n = file.read(&mut head).unwrap_or(0);
    path.is_file() && image::guess_format(&head[..n]).is_ok()
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
