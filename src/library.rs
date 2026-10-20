//! Finding similar pictures across all your boards, and building web image searches.
//!
//! Each image gets a small signature: an 8x8 grid of perceptual values (its value structure,
//! like a tiny notan) and its weighted palette. Signatures are cached per board in
//! `signatures.json`, so only new images cost time.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use serde::{Deserialize, Serialize};

use crate::board::{Board, Kind};
use crate::boards;
use crate::filters;

const GRID: u32 = 8;
const CACHE_FILE: &str = "signatures.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Signature {
    /// Mean L* (0..=100) of each cell, row by row.
    pub values: Vec<u8>,
    /// Dominant colors with their share of the image.
    pub palette: Vec<([u8; 3], f32)>,
}

pub fn signature(path: &Path) -> Result<Signature, String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let small = img
        .resize_exact(GRID * 4, GRID * 4, FilterType::Triangle)
        .to_rgba8();
    let mut values = vec![0f32; (GRID * GRID) as usize];
    for (x, y, p) in small.enumerate_pixels() {
        values[((y / 4) * GRID + x / 4) as usize] += filters::lightness(p[0], p[1], p[2]) / 16.0;
    }
    Ok(Signature {
        values: values.into_iter().map(|v| v.round() as u8).collect(),
        palette: filters::weighted_palette(&img.to_rgba8(), 6),
    })
}

/// How alike two value structures are, 0..=1. A mirror image counts as alike.
pub fn value_similarity(a: &Signature, b: &Signature) -> f32 {
    if a.values.len() != b.values.len() || a.values.is_empty() {
        return 0.0;
    }
    let g = GRID as usize;
    let diff = |mirror: bool| {
        let mut sum = 0.0;
        for y in 0..g {
            for x in 0..g {
                let bx = if mirror { g - 1 - x } else { x };
                sum += (a.values[y * g + x] as f32 - b.values[y * g + bx] as f32).abs();
            }
        }
        sum / (g * g) as f32
    };
    // A mean difference of 40 L* or more is a completely different structure.
    (1.0 - diff(false).min(diff(true)) / 40.0).clamp(0.0, 1.0)
}

/// How alike two palettes are, 0..=1: each color is matched to its nearest counterpart in the
/// other palette (CIE76 distance), weighted by how much of the image it covers, both ways.
pub fn palette_similarity(a: &Signature, b: &Signature) -> f32 {
    fn one_way(from: &[([u8; 3], f32)], to: &[([u8; 3], f32)]) -> f32 {
        let to: Vec<[f32; 3]> = to.iter().map(|(c, _)| filters::lab(*c)).collect();
        from.iter()
            .map(|(c, w)| {
                let [l, a, b] = filters::lab(*c);
                let nearest = to
                    .iter()
                    .map(|t| ((l - t[0]).powi(2) + (a - t[1]).powi(2) + (b - t[2]).powi(2)).sqrt())
                    .fold(f32::MAX, f32::min);
                w * nearest
            })
            .sum()
    }
    if a.palette.is_empty() || b.palette.is_empty() {
        return 0.0;
    }
    let d = (one_way(&a.palette, &b.palette) + one_way(&b.palette, &a.palette)) / 2.0;
    // Around 60 delta-E apart on average reads as unrelated colors.
    (1.0 - d / 60.0).clamp(0.0, 1.0)
}

#[derive(Clone, Debug)]
pub struct Match {
    pub board: PathBuf,
    pub path: PathBuf,
    pub values: f32,
    pub palette: f32,
}

/// Every board we know of: recent ones, plus any board folder next to the default board.
fn known_boards(current: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![current.to_path_buf()];
    dirs.extend(boards::recent());
    if let Ok(entries) = fs::read_dir(boards::boards_root()) {
        dirs.extend(
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.join("board.json").is_file()),
        );
    }
    let mut seen = HashSet::new();
    dirs.into_iter()
        .filter(|d| seen.insert(d.canonicalize().unwrap_or_else(|_| d.clone())))
        .collect()
}

/// Signatures for every image on a board, computing and caching the missing ones.
fn board_signatures(dir: &Path) -> HashMap<String, Signature> {
    let Ok(bytes) = fs::read(dir.join("board.json")) else {
        return HashMap::new();
    };
    let Ok(board) = serde_json::from_slice::<Board>(&bytes) else {
        return HashMap::new();
    };
    let cache_path = dir.join(CACHE_FILE);
    let mut cache: HashMap<String, Signature> = fs::read(&cache_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let files: HashSet<&str> = board
        .items
        .iter()
        .filter(|i| i.kind == Kind::Image)
        .map(|i| i.file.as_str())
        .collect();
    let before = cache.len();
    cache.retain(|file, _| files.contains(file.as_str()));
    let mut changed = cache.len() != before;
    for file in files {
        if !cache.contains_key(file)
            && let Ok(sig) = signature(&dir.join("images").join(file))
        {
            cache.insert(file.to_string(), sig);
            changed = true;
        }
    }
    if changed && let Ok(json) = serde_json::to_vec(&cache) {
        let _ = fs::write(&cache_path, json);
    }
    cache
}

/// Ranks every image on every known board against `query` (a file on the `current` board).
/// Images already on the current board are left out. Slow the first time; run it off the UI
/// thread.
pub fn find_similar(current: &Path, query: &str) -> Result<(Signature, Vec<Match>), String> {
    let here = board_signatures(current);
    let query_sig = match here.get(query) {
        Some(sig) => sig.clone(),
        None => signature(&current.join("images").join(query))?,
    };
    let mut seen: HashSet<String> = here.keys().cloned().collect();
    let mut matches = Vec::new();
    for dir in known_boards(current).into_iter().skip(1) {
        for (file, sig) in board_signatures(&dir) {
            // Files are named by content, so the same name is the same picture.
            if !seen.insert(file.clone()) {
                continue;
            }
            matches.push(Match {
                path: dir.join("images").join(&file),
                board: dir.clone(),
                values: value_similarity(&query_sig, &sig),
                palette: palette_similarity(&query_sig, &sig),
            });
        }
    }
    Ok((query_sig, matches))
}

// ---------------------------------------------------------------- web search

/// Colors the image sites can filter by.
pub const COLOR_NAMES: [&str; 12] = [
    "red", "orange", "yellow", "green", "teal", "blue", "purple", "pink", "brown", "black",
    "white", "gray",
];

/// A color name the image sites understand for the most prominent real color in a palette,
/// or a neutral when the image is mostly gray. Swatches with the same name add up, since one
/// color often splits into a light and a dark swatch.
pub fn color_name(palette: &[([u8; 3], f32)]) -> &'static str {
    let mut shares: Vec<(&'static str, f32)> = Vec::new();
    for &([r, g, b], share) in palette {
        let (h, s, v) = filters::hsv(r, g, b);
        if s < 0.25 || v < 0.2 {
            continue;
        }
        let name = hue_name(h, v, filters::lightness(r, g, b));
        match shares.iter_mut().find(|(n, _)| *n == name) {
            Some((_, total)) => *total += share,
            None => shares.push((name, share)),
        }
    }
    if let Some(&(name, share)) = shares.iter().max_by(|a, b| a.1.total_cmp(&b.1))
        && share >= 0.05
    {
        return name;
    }
    let l = palette
        .first()
        .map(|&([r, g, b], _)| filters::lightness(r, g, b))
        .unwrap_or(50.0);
    if l > 80.0 {
        "white"
    } else if l < 25.0 {
        "black"
    } else {
        "gray"
    }
}

fn hue_name(h: f32, v: f32, l: f32) -> &'static str {
    match h {
        h if !(15.0..345.0).contains(&h) => {
            if l > 70.0 {
                "pink"
            } else {
                "red"
            }
        }
        h if h < 40.0 => {
            if v < 0.6 {
                "brown"
            } else {
                "orange"
            }
        }
        h if h < 70.0 => "yellow",
        h if h < 160.0 => "green",
        h if h < 195.0 => "teal",
        h if h < 255.0 => "blue",
        h if h < 290.0 => "purple",
        _ => "pink",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    Google,
    Unsplash,
    Pinterest,
    ArtStation,
}

impl Site {
    pub const ALL: [Self; 4] = [
        Self::Google,
        Self::Unsplash,
        Self::Pinterest,
        Self::ArtStation,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Google => "Google Images",
            Self::Unsplash => "Unsplash",
            Self::Pinterest => "Pinterest",
            Self::ArtStation => "ArtStation",
        }
    }
}

/// Color filter for a web search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Any,
    BlackAndWhite,
    Color(&'static str),
}

/// A search URL for `subject`, filtered by `tone` where the site supports it (otherwise the
/// color is added to the query words).
pub fn search_url(site: Site, subject: &str, tone: Tone) -> String {
    let subject = subject.trim();
    let with_words = |extra: &str| {
        let q = format!("{subject} {extra}");
        encode(q.trim())
    };
    match site {
        Site::Google => {
            let filter = match tone {
                Tone::Any => String::new(),
                Tone::BlackAndWhite => "&tbs=ic:gray".into(),
                Tone::Color(c) => format!("&tbs=ic:specific,isc:{c}"),
            };
            format!(
                "https://www.google.com/search?tbm=isch&q={}{filter}",
                encode(subject)
            )
        }
        Site::Unsplash => {
            let color = match tone {
                Tone::Any => String::new(),
                Tone::BlackAndWhite => "?color=black_and_white".into(),
                // Unsplash has no brown, pink or gray filter; those go in the words instead.
                Tone::Color(c @ ("brown" | "pink" | "gray")) => {
                    return format!(
                        "https://unsplash.com/s/photos/{}",
                        with_words(c).replace("%20", "-")
                    );
                }
                Tone::Color(c) => format!("?color={c}"),
            };
            format!(
                "https://unsplash.com/s/photos/{}{color}",
                encode(subject).replace("%20", "-")
            )
        }
        Site::Pinterest => {
            let words = match tone {
                Tone::Any => "",
                Tone::BlackAndWhite => "black and white",
                Tone::Color(c) => c,
            };
            format!(
                "https://www.pinterest.com/search/pins/?q={}",
                with_words(words)
            )
        }
        Site::ArtStation => {
            let words = match tone {
                Tone::Any => "",
                Tone::BlackAndWhite => "black and white",
                Tone::Color(c) => c,
            };
            format!(
                "https://www.artstation.com/search?query={}",
                with_words(words)
            )
        }
    }
}

/// Percent-encodes everything but unreserved URL characters.
fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(values: Vec<u8>, palette: Vec<([u8; 3], f32)>) -> Signature {
        Signature { values, palette }
    }

    #[test]
    fn values_match_themselves_and_their_mirror() {
        // Dark left half, light right half.
        let left_dark: Vec<u8> = (0..64).map(|i| if i % 8 < 4 { 10 } else { 90 }).collect();
        let right_dark: Vec<u8> = (0..64).map(|i| if i % 8 < 4 { 90 } else { 10 }).collect();
        let flat: Vec<u8> = vec![50; 64];
        let a = sig(left_dark, vec![]);
        assert_eq!(value_similarity(&a, &a), 1.0);
        assert_eq!(value_similarity(&a, &sig(right_dark, vec![])), 1.0);
        assert!(value_similarity(&a, &sig(flat, vec![])) < 0.1);
    }

    #[test]
    fn palettes_rank_close_colors_above_far_ones() {
        let warm = sig(vec![], vec![([220, 60, 30], 0.7), ([240, 200, 120], 0.3)]);
        let warmer = sig(vec![], vec![([200, 50, 40], 0.6), ([250, 210, 110], 0.4)]);
        let cold = sig(vec![], vec![([30, 60, 200], 0.7), ([20, 120, 90], 0.3)]);
        assert_eq!(palette_similarity(&warm, &warm), 1.0);
        assert!(palette_similarity(&warm, &warmer) > 0.8);
        assert!(palette_similarity(&warm, &cold) < 0.2);
    }

    #[test]
    fn color_names() {
        assert_eq!(color_name(&[([230, 30, 30], 0.5)]), "red");
        assert_eq!(color_name(&[([30, 60, 200], 0.5)]), "blue");
        assert_eq!(color_name(&[([240, 200, 30], 0.5)]), "yellow");
        // Mostly white with a little blue: the blue still names it.
        assert_eq!(
            color_name(&[([250, 250, 250], 0.8), ([30, 60, 200], 0.2)]),
            "blue"
        );
        // A color split into light and dark swatches still wins over a bright background.
        let wizard = [
            ([253, 253, 252], 0.73),
            ([193, 191, 188], 0.076),
            ([55, 57, 124], 0.066),
            ([138, 130, 128], 0.048),
            ([78, 87, 189], 0.047),
        ];
        assert_eq!(color_name(&wizard), "blue");
        // A speck of color isn't enough.
        assert_eq!(
            color_name(&[([250, 250, 250], 0.97), ([230, 30, 30], 0.03)]),
            "white"
        );
        assert_eq!(color_name(&[([128, 128, 128], 1.0)]), "gray");
        assert_eq!(color_name(&[([10, 10, 12], 1.0)]), "black");
    }

    #[test]
    fn search_urls() {
        assert_eq!(
            search_url(Site::Google, "old oak tree", Tone::Color("green")),
            "https://www.google.com/search?tbm=isch&q=old%20oak%20tree&tbs=ic:specific,isc:green"
        );
        assert_eq!(
            search_url(Site::Unsplash, "old oak tree", Tone::BlackAndWhite),
            "https://unsplash.com/s/photos/old-oak-tree?color=black_and_white"
        );
        assert_eq!(
            search_url(Site::Unsplash, "fox", Tone::Color("brown")),
            "https://unsplash.com/s/photos/fox-brown"
        );
        assert_eq!(
            search_url(Site::Pinterest, "knight & dragon", Tone::Any),
            "https://www.pinterest.com/search/pins/?q=knight%20%26%20dragon"
        );
    }
}
