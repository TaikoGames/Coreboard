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
}

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
        }
    }
}

impl Item {
    pub fn is_image(&self) -> bool {
        self.kind == Kind::Image
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
            _ => String::new(),
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
        match self {
            Self::None => "Normal view",
            Self::Values3 => "Values: 3 tones",
            Self::Values5 => "Values: 5 tones",
            Self::Saturation => "Saturation map",
            Self::Detail => "Noise / detail map",
            Self::Squint => "Squint (big shapes)",
        }
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
        match self {
            Self::None => "No guide",
            Self::Thirds => "Rule of thirds",
            Self::PhiGrid => "Phi grid",
            Self::GoldenSpiral => "Golden spiral",
            Self::Diagonals => "Diagonals + reciprocals",
            Self::Center => "Center cross",
        }
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
        match self {
            Self::TopLeft => "Top left",
            Self::TopRight => "Top right",
            Self::BottomLeft => "Bottom left",
            Self::BottomRight => "Bottom right",
            Self::Center => "Center",
            Self::Tiled => "Tiled",
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn old_boards_load_uncropped() {
        let item: Item =
            serde_json::from_str(r#"{"id":1,"file":"a.png","pos":[0,0],"size":[10,10]}"#).unwrap();
        assert!(!item.is_cropped());
    }
}
