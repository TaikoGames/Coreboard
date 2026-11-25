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
    /// Displayed size in world units (already swapped for quarter-turn rotations).
    pub size: [f32; 2],
    /// Clockwise quarter turns, 0..=3.
    #[serde(default)]
    pub rotation: u8,
    /// Mirrored left-to-right on screen, applied after `rotation`.
    #[serde(default)]
    pub flip_x: bool,
    #[serde(default)]
    pub grayscale: bool,
    /// Study view shown instead of the plain image (value, saturation, ...).
    #[serde(default)]
    pub analysis: Analysis,
    /// Composition template drawn over the image.
    #[serde(default)]
    pub guide: Guide,
    /// Which way the golden spiral turns, 0..=3.
    #[serde(default)]
    pub guide_orientation: u8,
    /// Visible part of the source image as `[u0, v0, u1, v1]`, normalized and in the image's
    /// own orientation (before rotation and flip). The file itself is never changed.
    #[serde(default = "full_crop")]
    pub crop: [f32; 4],
    /// What this board element is. Older boards only had images.
    #[serde(default)]
    pub kind: Kind,
    /// Locked items can't be moved, resized or deleted until unlocked.
    #[serde(default)]
    pub locked: bool,
    /// Pinned items stay put on screen while the board pans and zooms underneath:
    /// `[x, y, width, height]` in screen points from the canvas corner.
    #[serde(default)]
    pub pinned: Option<[f32; 4]>,
    /// Lowercase words for search, e.g. "forest", "armor".
    #[serde(default)]
    pub tags: Vec<String>,
    /// Drawings on the item (arrows, circles, pen strokes); they move, resize, rotate, flip and
    /// crop along with it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<Mark>,
    /// A short line of text shown under an image, e.g. "Sargent, 1885, rim light".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub caption: String,
    /// Items with the same group are picked and moved together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<u64>,
    /// Perspective study: up to three vanishing points, in the same space as drawn marks (so
    /// they follow cropping, rotation and mirroring). They may lie outside the picture.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vanishing: Vec<[f32; 2]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkShape {
    #[default]
    Pen,
    Arrow,
    Circle,
    Box,
}

impl MarkShape {
    pub const ALL: [Self; 4] = [Self::Pen, Self::Arrow, Self::Circle, Self::Box];

    pub fn label(self) -> &'static str {
        crate::i18n::tr(match self {
            Self::Pen => "Pen",
            Self::Arrow => "Arrow",
            Self::Circle => "Circle",
            Self::Box => "Box",
        })
    }
}

/// One drawing on an item. Pen strokes keep every point; the other shapes use the first and
/// last point (arrow: tail to head; circle and box: opposite corners).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    pub shape: MarkShape,
    /// Points in the source picture's own 0..1 space (before crop, rotation and flip), so a
    /// drawing stays on the same spot of the picture whatever is done to it later.
    pub points: Vec<[f32; 2]>,
    /// Index into [`MARK_COLORS`].
    pub color: u8,
}

impl Mark {
    /// The mark as polylines in pixels, for an item drawn `w` by `h` with strokes `width` wide.
    pub fn polylines(&self, w: f32, h: f32, width: f32) -> Vec<Vec<[f32; 2]>> {
        let px = |q: [f32; 2]| [q[0] * w, q[1] * h];
        let (Some(first), Some(last)) = (self.points.first(), self.points.last()) else {
            return Vec::new();
        };
        let (a, b) = (px(*first), px(*last));
        match self.shape {
            MarkShape::Pen => vec![self.points.iter().map(|q| px(*q)).collect()],
            MarkShape::Arrow => {
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len = dx.hypot(dy);
                if len < 1e-3 {
                    return Vec::new();
                }
                let head = (width * 4.0).max(10.0).min(len * 0.6);
                let angle = dy.atan2(dx);
                let wing = |off: f32| {
                    [
                        b[0] - head * (angle + off).cos(),
                        b[1] - head * (angle + off).sin(),
                    ]
                };
                vec![vec![a, b], vec![wing(0.45), b, wing(-0.45)]]
            }
            MarkShape::Circle => {
                let center = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                let (rx, ry) = ((b[0] - a[0]).abs() / 2.0, (b[1] - a[1]).abs() / 2.0);
                let ring = (0..=48)
                    .map(|i| {
                        let t = i as f32 / 48.0 * std::f32::consts::TAU;
                        [center[0] + rx * t.cos(), center[1] + ry * t.sin()]
                    })
                    .collect();
                vec![ring]
            }
            MarkShape::Box => vec![vec![a, [b[0], a[1]], b, [a[0], b[1]], a]],
        }
    }
}

/// Marker colors that read on most pictures: red, yellow, cyan, green, white, black.
pub const MARK_COLORS: [[u8; 3]; 6] = [
    [255, 64, 64],
    [255, 214, 0],
    [0, 214, 255],
    [60, 220, 90],
    [255, 255, 255],
    [20, 20, 20],
];
/// Stroke width in world units, so marks scale with the board like the picture does.
pub const MARK_WIDTH: f32 = 5.0;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Kind {
    /// An image from `images/`, named by `Item::file`.
    #[default]
    Image,
    /// A strip of color swatches, most common first.
    Palette { colors: Vec<[u8; 3]> },
    /// A text note. Its width is the item's; its height follows the text.
    Note {
        text: String,
        /// Index into [`NOTE_COLORS`].
        #[serde(default)]
        color: u8,
    },
    /// A titled section drawn behind everything; moving it carries what's inside.
    Frame { title: String },
    /// An empty, labeled slot for an image still to find; dropping one on it fills it.
    Placeholder { label: String },
    /// A file that isn't a picture (an Illustrator file, a video, an app shortcut, a folder),
    /// shown as a card. Double-click opens it with its usual app. The file stays where it is:
    /// the card is a shortcut to it.
    File {
        path: String,
        #[serde(default)]
        folder: bool,
    },
    /// A drawing on the board itself (an arrow, circle, box or pen stroke made on an empty
    /// spot), its own object to select, move, resize or delete. Its points are in 0..1 of the
    /// item's rect, so it scales with it.
    Drawing { mark: Mark },
    /// A web page, shown as a card. Double-click opens it in the browser.
    Link {
        url: String,
        #[serde(default)]
        title: String,
    },
}

/// A file or link card's size in world units.
pub const CARD_SIZE: [f32; 2] = [300.0, 76.0];
pub const CARD_TITLE_FONT: f32 = 17.0;
pub const CARD_SUB_FONT: f32 = 12.5;

/// What a file or link card shows: title, the line under it, and the badge's text and color.
pub struct CardFace {
    pub title: String,
    pub subtitle: String,
    pub badge: String,
    pub color: [u8; 3],
}

impl Kind {
    pub fn card_face(&self) -> Option<CardFace> {
        match self {
            Kind::File { path, folder } => {
                let p = Path::new(path);
                let ext = p
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone());
                let stem = p
                    .file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| name.clone());
                let (title, subtitle, badge, color) = if *folder {
                    (
                        name,
                        crate::i18n::tr("Folder").to_string(),
                        "DIR".to_string(),
                        [222, 182, 68],
                    )
                } else {
                    let color = match ext.as_str() {
                        "ai" | "eps" | "svg" => [255, 154, 0],
                        "psd" | "psb" => [49, 168, 255],
                        "blend" => [234, 118, 0],
                        "kra" | "clip" | "procreate" | "xcf" => [92, 160, 255],
                        "mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v" | "wmv" => [150, 96, 232],
                        "mp3" | "wav" | "ogg" | "flac" | "m4a" => [232, 96, 160],
                        "pdf" => [228, 64, 52],
                        "fbx" | "obj" | "glb" | "gltf" | "max" | "ma" | "mb" | "spp" | "sbsar"
                        | "ztl" | "zpr" | "usd" | "usdz" | "abc" | "stl" => [36, 178, 160],
                        "zip" | "rar" | "7z" | "tar" | "gz" => [168, 136, 92],
                        "doc" | "docx" | "txt" | "md" | "rtf" | "odt" | "pages" => [84, 140, 232],
                        "lnk" | "exe" | "app" | "url" | "desktop" => [112, 132, 168],
                        _ => [124, 124, 134],
                    };
                    let shortcut = matches!(ext.as_str(), "lnk" | "url" | "desktop" | "app");
                    let title = if shortcut { stem } else { name };
                    let subtitle = match ext.as_str() {
                        "lnk" | "url" | "desktop" => crate::i18n::tr("Shortcut").to_string(),
                        "exe" | "app" => crate::i18n::tr("App").to_string(),
                        _ => p
                            .parent()
                            .and_then(|d| d.file_name())
                            .map(|d| d.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                    };
                    let badge = if ext.is_empty() {
                        "FILE".to_string()
                    } else {
                        ext.to_uppercase().chars().take(5).collect()
                    };
                    (title, subtitle, badge, color)
                };
                Some(CardFace {
                    title,
                    subtitle,
                    badge,
                    color,
                })
            }
            Kind::Link { url, title } => {
                let domain = crate::web::domain(url);
                Some(CardFace {
                    title: if title.trim().is_empty() {
                        domain.clone()
                    } else {
                        title.trim().to_string()
                    },
                    subtitle: domain,
                    badge: "WEB".to_string(),
                    color: [72, 196, 120],
                })
            }
            _ => None,
        }
    }
}

/// Note backgrounds and their text colors.
pub const NOTE_COLORS: [([u8; 3], [u8; 3]); 5] = [
    ([247, 226, 124], [40, 36, 20]), // yellow
    ([246, 178, 196], [48, 22, 30]), // pink
    ([160, 206, 246], [18, 34, 52]), // blue
    ([174, 222, 170], [20, 44, 22]), // green
    ([43, 45, 49], [228, 228, 230]), // dark
];

/// Layout constants in world units, shared by the board and export so they match.
pub const NOTE_FONT: f32 = 18.0;
pub const NOTE_PAD: f32 = 12.0;
pub const FRAME_TITLE_HEIGHT: f32 = 34.0;
pub const FRAME_FONT: f32 = 19.0;
pub const PLACEHOLDER_FONT: f32 = 18.0;
/// Captions under images: font size, gap below the image and line spacing, in world units.
pub const CAPTION_FONT: f32 = 15.0;
pub const CAPTION_GAP: f32 = 8.0;
pub const CAPTION_LINE: f32 = CAPTION_FONT * 1.3;

fn full_crop() -> [f32; 4] {
    [0.0, 0.0, 1.0, 1.0]
}

impl Default for Item {
    fn default() -> Self {
        Self {
            id: 0,
            file: String::new(),
            pos: [0.0; 2],
            size: [1.0; 2],
            rotation: 0,
            flip_x: false,
            grayscale: false,
            analysis: Analysis::None,
            guide: Guide::None,
            guide_orientation: 0,
            crop: full_crop(),
            kind: Kind::Image,
            locked: false,
            pinned: None,
            tags: Vec::new(),
            marks: Vec::new(),
            caption: String::new(),
            group: None,
            vanishing: Vec::new(),
        }
    }
}

impl Item {
    pub fn is_image(&self) -> bool {
        self.kind == Kind::Image
    }

    /// A file or link card.
    pub fn is_card(&self) -> bool {
        matches!(self.kind, Kind::File { .. } | Kind::Link { .. })
    }

    pub fn is_drawing(&self) -> bool {
        matches!(self.kind, Kind::Drawing { .. })
    }

    pub fn is_frame(&self) -> bool {
        matches!(self.kind, Kind::Frame { .. })
    }

    /// Whether layout actions (move, align, arrange, rotate, crop) may change its geometry.
    /// Pinned items only move by dragging, in screen space.
    pub fn movable(&self) -> bool {
        !self.locked && self.pinned.is_none()
    }

    /// Whether every word of `query` matches this item: a tag starting with the word, or the
    /// word appearing in a note's text or a frame's title. Case-insensitive.
    pub fn matches(&self, query: &str) -> bool {
        let text = match &self.kind {
            Kind::Note { text, .. } => text.to_lowercase(),
            Kind::Frame { title } => title.to_lowercase(),
            Kind::Placeholder { label } => label.to_lowercase(),
            Kind::File { path, .. } => path.to_lowercase(),
            Kind::Link { url, title } => format!("{title} {url}").to_lowercase(),
            _ => self.caption.to_lowercase(),
        };
        query.split_whitespace().all(|word| {
            let word = word.to_lowercase();
            self.tags.iter().any(|t| t.starts_with(&word)) || text.contains(&word)
        })
    }
}

/// A tag as stored: trimmed, lowercase, inner spaces collapsed. `None` if nothing is left.
pub fn normalize_tag(tag: &str) -> Option<String> {
    let tag = tag
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    (!tag.is_empty()).then_some(tag)
}

impl Item {
    /// Maps a normalized point in the source image to where it shows on screen, within the
    /// whole (uncropped) image's display rect.
    pub fn tex_to_display(&self, (u, v): (f32, f32)) -> (f32, f32) {
        let (x, y) = match self.rotation % 4 {
            1 => (1.0 - v, u),
            2 => (1.0 - u, 1.0 - v),
            3 => (v, 1.0 - u),
            _ => (u, v),
        };
        if self.flip_x { (1.0 - x, y) } else { (x, y) }
    }

    /// Inverse of [`Self::tex_to_display`].
    pub fn display_to_tex(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let x = if self.flip_x { 1.0 - x } else { x };
        match self.rotation % 4 {
            1 => (y, 1.0 - x),
            2 => (1.0 - x, 1.0 - y),
            3 => (1.0 - y, x),
            _ => (x, y),
        }
    }

    /// The crop as it appears on screen: `[x0, y0, x1, y1]` within the whole image's rect.
    pub fn display_crop(&self) -> [f32; 4] {
        let [u0, v0, u1, v1] = self.crop;
        let (ax, ay) = self.tex_to_display((u0, v0));
        let (bx, by) = self.tex_to_display((u1, v1));
        [ax.min(bx), ay.min(by), ax.max(bx), ay.max(by)]
    }

    /// Sets the crop from a rect given in screen orientation (see [`Self::display_crop`]).
    pub fn set_display_crop(&mut self, [x0, y0, x1, y1]: [f32; 4]) {
        let (au, av) = self.display_to_tex((x0, y0));
        let (bu, bv) = self.display_to_tex((x1, y1));
        self.crop = [au.min(bu), av.min(bv), au.max(bu), av.max(bv)];
    }

    /// Where a picture point (see [`Mark::points`]) shows on the item, in 0..1 of its visible
    /// rect. Points cropped away fall outside 0..1.
    pub fn mark_to_local(&self, p: [f32; 2]) -> [f32; 2] {
        let (x, y) = self.tex_to_display((p[0], p[1]));
        let [x0, y0, x1, y1] = self.display_crop();
        [(x - x0) / (x1 - x0), (y - y0) / (y1 - y0)]
    }

    /// Inverse of [`Self::mark_to_local`].
    pub fn local_to_mark(&self, q: [f32; 2]) -> [f32; 2] {
        let [x0, y0, x1, y1] = self.display_crop();
        let (u, v) = self.display_to_tex((x0 + q[0] * (x1 - x0), y0 + q[1] * (y1 - y0)));
        [u, v]
    }

    /// A mark as it appears on this item: its points in 0..1 of the visible rect.
    pub fn local_mark(&self, mark: &Mark) -> Mark {
        Mark {
            points: mark.points.iter().map(|p| self.mark_to_local(*p)).collect(),
            ..mark.clone()
        }
    }

    pub fn is_cropped(&self) -> bool {
        self.crop != full_crop()
    }

    /// Resizes the item to `target` (world units), cropping its visible part around the
    /// center so the picture fills the new shape without stretching.
    pub fn cover(&mut self, target: [f32; 2]) {
        let current = self.size[0] / self.size[1].max(f32::EPSILON);
        let wanted = target[0] / target[1].max(f32::EPSILON);
        let [x0, y0, x1, y1] = self.display_crop();
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let (mut w, mut h) = (x1 - x0, y1 - y0);
        if current > wanted {
            w *= wanted / current; // Too wide: trim the sides.
        } else {
            h *= current / wanted; // Too tall: trim top and bottom.
        }
        self.set_display_crop([cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]);
        self.size = target;
    }
}

/// Ways of looking at an image for painting study.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Analysis {
    #[default]
    None,
    /// Lightness grouped into 3 tones (dark / mid / light), like a notan study.
    Values3,
    /// Lightness grouped into 5 tones.
    Values5,
    /// Saturation as brightness: white = most saturated, black = gray.
    Saturation,
    /// Where the busy detail is: bright = noisy, dark = calm rest areas.
    Detail,
    /// Heavy blur, to judge big shapes like squinting at the subject.
    Squint,
}

impl Analysis {
    pub const CYCLE: [Self; 6] = [
        Self::None,
        Self::Values3,
        Self::Values5,
        Self::Saturation,
        Self::Detail,
        Self::Squint,
    ];

    pub fn label(self) -> &'static str {
        crate::i18n::tr(match self {
            Self::None => "Normal view",
            Self::Values3 => "Values: 3 tones",
            Self::Values5 => "Values: 5 tones",
            Self::Saturation => "Saturation map",
            Self::Detail => "Noise / detail map",
            Self::Squint => "Squint (big shapes)",
        })
    }
}

/// Composition templates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Guide {
    #[default]
    None,
    Thirds,
    PhiGrid,
    GoldenSpiral,
    /// Both diagonals plus their reciprocals (dynamic symmetry).
    Diagonals,
    Center,
}

impl Guide {
    pub const CYCLE: [Self; 6] = [
        Self::None,
        Self::Thirds,
        Self::PhiGrid,
        Self::GoldenSpiral,
        Self::Diagonals,
        Self::Center,
    ];

    pub fn label(self) -> &'static str {
        crate::i18n::tr(match self {
            Self::None => "No guide",
            Self::Thirds => "Rule of thirds",
            Self::PhiGrid => "Phi grid",
            Self::GoldenSpiral => "Golden spiral",
            Self::Diagonals => "Diagonals + reciprocals",
            Self::Center => "Center cross",
        })
    }
}

/// Steps through a cycle, forwards or backwards, wrapping around.
pub fn cycle<T: Copy + PartialEq>(all: &[T], current: T, forward: bool) -> T {
    let i = all.iter().position(|&x| x == current).unwrap_or(0);
    let n = all.len();
    all[if forward {
        (i + 1) % n
    } else {
        (i + n - 1) % n
    }]
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSettings {
    /// 0.1..=1.0; below 1.0 the desktop shows through the board.
    pub opacity: f32,
    pub always_on_top: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            always_on_top: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatermarkPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
    Center,
    /// Repeated across the whole image.
    Tiled,
}

impl WatermarkPosition {
    pub const ALL: [Self; 6] = [
        Self::BottomRight,
        Self::BottomLeft,
        Self::TopRight,
        Self::TopLeft,
        Self::Center,
        Self::Tiled,
    ];

    pub fn label(self) -> &'static str {
        crate::i18n::tr(match self {
            Self::TopLeft => "Top left",
            Self::TopRight => "Top right",
            Self::BottomLeft => "Bottom left",
            Self::BottomRight => "Bottom right",
            Self::Center => "Center",
            Self::Tiled => "Tiled",
        })
    }
}

/// Stamped on exported PNGs. An empty `text` exports without a watermark.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Watermark {
    pub text: String,
    pub position: WatermarkPosition,
    /// 0.1..=1.0
    pub opacity: f32,
}

impl Default for Watermark {
    fn default() -> Self {
        Self {
            text: String::new(),
            position: WatermarkPosition::default(),
            opacity: 0.6,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Board {
    /// Draw order: later items are on top.
    pub items: Vec<Item>,
    pub view: View,
    pub next_id: u64,
    #[serde(default)]
    pub window: WindowSettings,
    #[serde(default)]
    pub watermark: Watermark,
    /// Where PNG exports go; `None` means the board's own `exports/` folder.
    #[serde(default)]
    pub export_dir: Option<PathBuf>,
    /// A folder whose new images are added to the board automatically.
    #[serde(default)]
    pub watch: Option<WatchFolder>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchFolder {
    pub path: PathBuf,
    /// File names already handled: imported, or skipped as already there when watching began.
    pub seen: Vec<String>,
    /// Images that were in the folder when watching began, which can still be added on request.
    pub skipped: Vec<String>,
    /// The frame new images go into.
    pub frame: Option<u64>,
    /// Where the next image goes, relative to the frame's corner: `[x, y, row height]`.
    pub cursor: [f32; 3],
}

pub struct Store {
    pub dir: PathBuf,
    /// Set when board.json exists but couldn't be read (made by a newer Core Board, or
    /// damaged). Then nothing is saved over it and no images are cleaned up, so opening it in
    /// an older version can never wipe it.
    pub unreadable: Option<String>,
}

impl Store {
    pub fn open(dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(dir.join("images"))?;
        Ok(Self {
            dir,
            unreadable: None,
        })
    }

    pub fn image_path(&self, file: &str) -> PathBuf {
        self.dir.join("images").join(file)
    }

    pub fn load(&mut self) -> Board {
        self.unreadable = None;
        let path = self.dir.join("board.json");
        let Ok(bytes) = fs::read(&path) else {
            return Board::default();
        };
        match serde_json::from_slice(&bytes) {
            Ok(board) => board,
            Err(err) => {
                eprintln!("coreboard: could not parse {}: {err}", path.display());
                // Keep a copy too, in case something else touches the file later.
                let backup = self.dir.join("board.json.unreadable-backup");
                if !backup.exists() {
                    let _ = fs::write(&backup, &bytes);
                }
                self.unreadable = Some(err.to_string());
                Board::default()
            }
        }
    }

    /// Writes atomically so a crash mid-save never leaves a truncated board.
    pub fn save(&self, board: &Board) -> io::Result<()> {
        if self.unreadable.is_some() {
            return Ok(());
        }
        let json = serde_json::to_vec_pretty(board).map_err(io::Error::other)?;
        // A backup can't be made? Saving still goes ahead.
        let _ = self.back_up(today());
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

    /// Deletes image files no item refers to anymore, on the board or in any of its backups.
    pub fn collect_garbage(&self, board: &Board) {
        // An unreadable board's items aren't known: every image would look unused.
        if self.unreadable.is_some() {
            return;
        }
        let Ok(entries) = fs::read_dir(self.dir.join("images")) else {
            return;
        };
        let mut used: std::collections::HashSet<String> =
            board.items.iter().map(|i| i.file.clone()).collect();
        for day in self.backups() {
            match self.load_backup(&day) {
                Some(old) => used.extend(old.items.into_iter().map(|i| i.file)),
                // A backup that can't be read might still need its pictures: keep them all.
                None => return,
            }
        }
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !used.contains(&name) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    fn backup_dir(&self) -> PathBuf {
        self.dir.join("backups")
    }

    /// Before the first save of a day, keeps the board as it was last saved on an earlier day,
    /// named after that day. The newest `KEEP_BACKUPS` days are kept.
    fn back_up(&self, today: Day) -> io::Result<()> {
        let current = self.dir.join("board.json");
        let Ok(modified) = fs::metadata(&current).and_then(|m| m.modified()) else {
            return Ok(());
        };
        let day = local_day(modified);
        if day >= today {
            return Ok(());
        }
        let dir = self.backup_dir();
        let path = dir.join(format!("{}.json", date_text(day)));
        if path.exists() {
            return Ok(());
        }
        fs::create_dir_all(&dir)?;
        fs::copy(&current, &path)?;
        for old in self.backups().into_iter().skip(KEEP_BACKUPS) {
            let _ = fs::remove_file(dir.join(format!("{old}.json")));
        }
        Ok(())
    }

    /// Days with a backup ("2026-11-23"), newest first.
    pub fn backups(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.backup_dir()) else {
            return Vec::new();
        };
        let mut days: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let day = name.strip_suffix(".json")?;
                (day.len() == 10 && day.chars().all(|c| c.is_ascii_digit() || c == '-'))
                    .then(|| day.to_string())
            })
            .collect();
        days.sort_unstable_by(|a, b| b.cmp(a));
        days
    }

    /// The board as it was on `day`.
    pub fn load_backup(&self, day: &str) -> Option<Board> {
        let bytes = fs::read(self.backup_dir().join(format!("{day}.json"))).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

/// How many days of backups each board keeps.
pub const KEEP_BACKUPS: usize = 7;

/// A calendar day on this computer's clock (its own time zone, so an evening's changes are
/// filed under that evening's date).
pub type Day = chrono::NaiveDate;

fn local_day(time: std::time::SystemTime) -> Day {
    chrono::DateTime::<chrono::Local>::from(time).date_naive()
}

fn today() -> Day {
    chrono::Local::now().date_naive()
}

/// "2026-11-23".
pub fn date_text(day: Day) -> String {
    day.format("%Y-%m-%d").to_string()
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

#[cfg(test)]
mod backup_tests {
    use super::*;

    fn day(n: u64) -> Day {
        Day::from_ymd_opt(2026, 1, 1).unwrap() + chrono::Days::new(n)
    }

    #[test]
    fn writes_dates() {
        assert_eq!(date_text(day(0)), "2026-01-01");
        assert_eq!(
            date_text(Day::from_ymd_opt(2024, 2, 29).unwrap()),
            "2024-02-29"
        );
    }

    #[test]
    fn keeps_one_backup_a_day_and_their_pictures() {
        let dir = std::env::temp_dir().join(format!("coreboard-backups-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = Store::open(dir.clone()).unwrap();
        let picture = |id: u64, file: &str| Item {
            id,
            file: file.into(),
            ..Item::default()
        };
        fs::write(store.image_path("old.png"), b"old").unwrap();
        fs::write(store.image_path("new.png"), b"new").unwrap();
        let old = Board {
            items: vec![picture(1, "old.png")],
            ..Board::default()
        };
        store.save(&old).unwrap();
        // Saved today: no backup yet.
        assert!(store.backups().is_empty());
        // The next day's first save keeps today's version first.
        let tomorrow = today() + chrono::Days::new(1);
        store.back_up(tomorrow).unwrap();
        store.back_up(tomorrow).unwrap();
        assert_eq!(store.backups(), vec![date_text(today())]);
        let new = Board {
            items: vec![picture(1, "new.png")],
            ..Board::default()
        };
        store.save(&new).unwrap();
        // The old picture is only in the backup, and survives clean-up.
        store.collect_garbage(&new);
        assert!(store.image_path("old.png").exists());
        let restored = store.load_backup(&date_text(today())).unwrap();
        assert_eq!(restored.items[0].file, "old.png");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn keeps_only_the_newest_days() {
        let dir = std::env::temp_dir().join(format!("coreboard-backups7-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = Store::open(dir.clone()).unwrap();
        store.save(&Board::default()).unwrap();
        fs::create_dir_all(store.backup_dir()).unwrap();
        for n in 1..=10u64 {
            let path = store
                .backup_dir()
                .join(format!("{}.json", date_text(day(n))));
            fs::write(path, b"{}").unwrap();
        }
        store.back_up(today() + chrono::Days::new(1)).unwrap();
        let kept = store.backups();
        assert_eq!(kept.len(), KEEP_BACKUPS);
        assert_eq!(kept[0], date_text(today()));
        assert_eq!(kept[1], date_text(day(10)));
        let _ = fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_overwrites_a_board_it_cannot_read() {
        let dir = std::env::temp_dir().join(format!("coreboard-unreadable-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut store = Store::open(dir.clone()).unwrap();
        // A board from a future version, with a kind of item this one doesn't know.
        let future = br#"{"items":[{"id":1,"kind":{"type":"Hologram"},"file":"a.png","pos":[0,0],"size":[10,10]}]}"#;
        fs::write(dir.join("board.json"), future).unwrap();
        fs::write(store.image_path("a.png"), b"picture").unwrap();
        let board = store.load();
        assert!(store.unreadable.is_some());
        store.save(&board).unwrap();
        store.collect_garbage(&board);
        assert_eq!(fs::read(dir.join("board.json")).unwrap(), future);
        assert!(store.image_path("a.png").exists());
        assert_eq!(
            fs::read(dir.join("board.json.unreadable-backup")).unwrap(),
            future
        );
        let _ = fs::remove_dir_all(&dir);
    }

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    #[test]
    fn display_mapping_round_trips_for_every_orientation() {
        for rotation in 0..4 {
            for flip_x in [false, true] {
                let item = Item {
                    rotation,
                    flip_x,
                    ..Item::default()
                };
                for p in [(0.1, 0.2), (0.9, 0.3), (0.5, 0.75)] {
                    let back = item.display_to_tex(item.tex_to_display(p));
                    assert!((back.0 - p.0).abs() < 1e-6 && (back.1 - p.1).abs() < 1e-6);
                }
            }
        }
    }

    #[test]
    fn display_crop_round_trips() {
        let rect = [0.1, 0.2, 0.6, 0.9];
        for rotation in 0..4 {
            for flip_x in [false, true] {
                let mut item = Item {
                    rotation,
                    flip_x,
                    ..Item::default()
                };
                item.set_display_crop(rect);
                assert!(
                    close(item.display_crop(), rect),
                    "rot {rotation} flip {flip_x}"
                );
            }
        }
    }

    #[test]
    fn quarter_turn_moves_top_left_of_image_to_top_right_of_screen() {
        let item = Item {
            rotation: 1,
            ..Item::default()
        };
        assert_eq!(item.tex_to_display((0.0, 0.0)), (1.0, 0.0));
        // So cropping the image's left strip shows up as the screen's top strip.
        let mut item = item;
        item.crop = [0.0, 0.0, 0.25, 1.0];
        assert!(close(item.display_crop(), [0.0, 0.0, 1.0, 0.25]));
    }

    #[test]
    fn search_matches_tag_prefixes_and_text() {
        let image = Item {
            tags: vec!["forest".into(), "dusk".into()],
            ..Item::default()
        };
        assert!(image.matches("for"));
        assert!(image.matches("Forest DUSK"));
        assert!(!image.matches("forest armor"));
        let note = Item {
            kind: Kind::Note {
                text: "Rim light from the left".into(),
                color: 0,
            },
            ..Item::default()
        };
        assert!(note.matches("light"));
        assert!(!note.matches("forest"));
        assert_eq!(normalize_tag("  Old   Oak "), Some("old oak".into()));
        assert_eq!(normalize_tag("   "), None);
    }

    #[test]
    fn cover_crops_to_the_slot_shape_around_the_center() {
        // A 400x200 picture into a square slot keeps the middle half.
        let mut wide = Item {
            size: [400.0, 200.0],
            ..Item::default()
        };
        wide.cover([300.0, 300.0]);
        assert_eq!(wide.size, [300.0, 300.0]);
        assert!(close(wide.crop, [0.25, 0.0, 0.75, 1.0]));

        // A tall picture into a wide slot keeps a horizontal band.
        let mut tall = Item {
            size: [100.0, 400.0],
            ..Item::default()
        };
        tall.cover([200.0, 100.0]);
        assert!(close(tall.crop, [0.0, 0.4375, 1.0, 0.5625]));

        // Rotated a quarter turn, the band is taken across the image's own width instead.
        let mut turned = Item {
            size: [400.0, 200.0],
            rotation: 1,
            ..Item::default()
        };
        turned.cover([200.0, 200.0]);
        assert!(close(turned.crop, [0.0, 0.25, 1.0, 0.75]));
    }

    #[test]
    fn marks_follow_the_picture_through_rotation_flip_and_crop() {
        let mut item = Item::default();
        // A mark near the picture's top-right corner...
        let picture_point = item.local_to_mark([0.9, 0.1]);
        // ...after a clockwise quarter turn sits at the bottom-right, and mirroring then moves
        // it to the bottom-left.
        item.rotation = 1;
        item.flip_x = true;
        let [x, y] = item.mark_to_local(picture_point);
        assert!((x - 0.1).abs() < 1e-5 && (y - 0.9).abs() < 1e-5, "{x} {y}");
        // Cropping to the right half of the picture's display keeps points mapped to the same
        // picture spot, at new local coordinates.
        let mut cropped = Item::default();
        cropped.set_display_crop([0.5, 0.0, 1.0, 1.0]);
        let p = Item::default().local_to_mark([0.75, 0.5]);
        let [x, y] = cropped.mark_to_local(p);
        assert!((x - 0.5).abs() < 1e-5 && (y - 0.5).abs() < 1e-5);
        // And round trips for every orientation.
        for rotation in 0..4 {
            for flip_x in [false, true] {
                let mut item = Item {
                    rotation,
                    flip_x,
                    ..Item::default()
                };
                item.set_display_crop([0.1, 0.2, 0.7, 0.9]);
                let q = item.mark_to_local(item.local_to_mark([0.3, 0.6]));
                assert!((q[0] - 0.3).abs() < 1e-5 && (q[1] - 0.6).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn mark_geometry() {
        let mark = |shape, points: Vec<[f32; 2]>| Mark {
            shape,
            points,
            color: 0,
        };
        // Arrow from the top-left quarter to the center of a 200x100 item: a shaft and a head
        // whose tip is the arrow's end.
        let arrow =
            mark(MarkShape::Arrow, vec![[0.25, 0.25], [0.5, 0.5]]).polylines(200.0, 100.0, 4.0);
        assert_eq!(arrow.len(), 2);
        assert_eq!(arrow[0], vec![[50.0, 25.0], [100.0, 50.0]]);
        assert_eq!(arrow[1][1], [100.0, 50.0]);
        // A circle closes on itself and stays inside its box.
        let ring =
            &mark(MarkShape::Circle, vec![[0.0, 0.0], [1.0, 1.0]]).polylines(100.0, 50.0, 4.0)[0];
        assert!((ring[0][0] - ring[48][0]).abs() < 1e-3 && (ring[0][1] - ring[48][1]).abs() < 1e-3);
        assert!(
            ring.iter()
                .all(|p| (-0.01..=100.01).contains(&p[0]) && (-0.01..=50.01).contains(&p[1]))
        );
        // A box has its four corners and closes.
        let boxed =
            &mark(MarkShape::Box, vec![[0.1, 0.2], [0.5, 0.6]]).polylines(10.0, 10.0, 1.0)[0];
        assert_eq!(boxed.len(), 5);
        assert_eq!(boxed[0], boxed[4]);
        // A zero-length arrow draws nothing.
        assert!(
            mark(MarkShape::Arrow, vec![[0.3, 0.3], [0.3, 0.3]])
                .polylines(10.0, 10.0, 1.0)
                .is_empty()
        );
    }

    #[test]
    fn old_boards_load_uncropped() {
        let item: Item =
            serde_json::from_str(r#"{"id":1,"file":"a.png","pos":[0,0],"size":[10,10]}"#).unwrap();
        assert!(!item.is_cropped());
    }
}
