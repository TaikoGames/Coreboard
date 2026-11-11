//! Renders board items to a PNG, optionally stamped with a text watermark.
//!
//! This works from the original image files on the CPU, so exports are full quality no matter
//! how far the board is zoomed out or how much the on-screen textures were downscaled.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use image::imageops::{self, FilterType};
use image::{DynamicImage, Rgba, RgbaImage};

use crate::board::{
    CAPTION_FONT, CAPTION_GAP, CAPTION_LINE, FRAME_FONT, FRAME_TITLE_HEIGHT, Item, Kind,
    MARK_COLORS, MARK_WIDTH, Mark, NOTE_COLORS, NOTE_FONT, NOTE_PAD, Watermark, WatermarkPosition,
};
use crate::filters::{self, Filter};

/// Exports larger than this on either side are scaled down to fit.
const MAX_SIDE: f32 = 8192.0;
const PADDING: f32 = 32.0;
const BACKGROUND: Rgba<u8> = Rgba([30, 31, 34, 255]);

/// Renders `items` (in draw order) and writes the PNG into `out_dir`. Returns the file path.
pub fn export_png(
    images_dir: &Path,
    out_dir: &Path,
    items: &[Item],
    watermark: &Watermark,
) -> Result<PathBuf, String> {
    let image = render(images_dir, items, watermark)?;
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let path = out_dir.join(format!("coreboard-{}.png", timestamp()));
    image.save(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn render(images_dir: &Path, items: &[Item], watermark: &Watermark) -> Result<RgbaImage, String> {
    if items.is_empty() {
        return Err("nothing to export".into());
    }
    let font =
        FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("bundled font is valid");
    // Captions hang below their images, so they count toward the picture's size.
    let caption_lines = |i: &Item| -> Vec<String> {
        if i.is_image() && !i.caption.trim().is_empty() {
            wrap(&font, i.caption.trim(), CAPTION_FONT, i.size[0])
        } else {
            Vec::new()
        }
    };
    let (min, max) = items
        .iter()
        .fold(([f32::MAX; 2], [f32::MIN; 2]), |(min, max), i| {
            let lines = caption_lines(i).len();
            let below = if lines > 0 {
                CAPTION_GAP + lines as f32 * CAPTION_LINE
            } else {
                0.0
            };
            (
                [min[0].min(i.pos[0]), min[1].min(i.pos[1])],
                [
                    max[0].max(i.pos[0] + i.size[0]),
                    max[1].max(i.pos[1] + i.size[1] + below),
                ],
            )
        });
    // One world unit is one pixel, unless that would make the file huge.
    let content = [max[0] - min[0], max[1] - min[1]];
    let scale = (MAX_SIDE / (content[0].max(content[1]) + 2.0 * PADDING)).min(1.0);
    let width = ((content[0] + 2.0 * PADDING) * scale).round().max(1.0) as u32;
    let height = ((content[1] + 2.0 * PADDING) * scale).round().max(1.0) as u32;
    let mut canvas = RgbaImage::from_pixel(width, height, BACKGROUND);

    // Frames sit behind everything, as on the board.
    let mut ordered: Vec<&Item> = items.iter().collect();
    ordered.sort_by_key(|i| !i.is_frame());
    for item in ordered {
        let x = ((item.pos[0] - min[0] + PADDING) * scale).round() as i64;
        let y = ((item.pos[1] - min[1] + PADDING) * scale).round() as i64;
        let w = (item.size[0] * scale).round().max(1.0) as u32;
        let h = (item.size[1] * scale).round().max(1.0) as u32;
        match &item.kind {
            Kind::Image => {}
            Kind::Palette { colors } => {
                draw_palette(&mut canvas, &font, (x, y, w, h), colors);
                continue;
            }
            Kind::Note { text, color } => {
                draw_note(&mut canvas, &font, (x, y, w, h), text, *color, scale);
                continue;
            }
            Kind::Frame { title } => {
                draw_frame(&mut canvas, &font, (x, y, w, h), title, scale);
                continue;
            }
            Kind::Placeholder { label } => {
                draw_placeholder(&mut canvas, &font, (x, y, w, h), label, scale);
                continue;
            }
        }
        let path = images_dir.join(&item.file);
        let img = image::open(&path).map_err(|e| format!("{}: {e}", item.file))?;
        let mut img = DynamicImage::ImageRgba8(filters::apply(img, Filter::for_item(item)));
        if item.is_cropped() {
            let (w, h) = (img.width() as f32, img.height() as f32);
            let [u0, v0, u1, v1] = item.crop;
            let (x, y) = ((u0 * w).round() as u32, (v0 * h).round() as u32);
            let cw = (((u1 - u0) * w).round() as u32).max(1);
            let ch = (((v1 - v0) * h).round() as u32).max(1);
            img = img.crop_imm(x, y, cw, ch);
        }
        img = match item.rotation % 4 {
            1 => img.rotate90(),
            2 => img.rotate180(),
            3 => img.rotate270(),
            _ => img,
        };
        if item.flip_x {
            img = img.fliph();
        }
        let img = img.resize_exact(w, h, FilterType::CatmullRom).to_rgba8();
        imageops::overlay(&mut canvas, &img, x, y);
        let marks: Vec<Mark> = item.marks.iter().map(|m| item.local_mark(m)).collect();
        draw_marks(&mut canvas, (x, y, w, h), &marks, scale);
        let top = (y + h as i64) as f32 + CAPTION_GAP * scale;
        for (n, line) in caption_lines(item).iter().enumerate() {
            let line = Line::layout(&font, line, CAPTION_FONT * scale);
            let ly = top + n as f32 * CAPTION_LINE * scale;
            line.draw(&font, &mut canvas, x as f32, ly, [205, 205, 205], 1.0);
        }
    }

    let text = watermark.text.trim();
    if !text.is_empty() {
        stamp(&mut canvas, text, watermark);
    }
    Ok(canvas)
}

/// Same layout as on the board: equal swatches along the longer side, labeled when big enough.
fn draw_palette(
    canvas: &mut RgbaImage,
    font: &FontRef<'_>,
    (x, y, w, h): (i64, i64, u32, u32),
    colors: &[[u8; 3]],
) {
    let n = colors.len().max(1) as f32;
    let horizontal = w >= h;
    for (i, &[r, g, b]) in colors.iter().enumerate() {
        let (sx, sy, sw, sh) = if horizontal {
            let step = w as f32 / n;
            (
                x + (step * i as f32).round() as i64,
                y,
                step.ceil() as u32,
                h,
            )
        } else {
            let step = h as f32 / n;
            (
                x,
                y + (step * i as f32).round() as i64,
                w,
                step.ceil() as u32,
            )
        };
        fill_rect(canvas, (sx, sy, sw, sh), [r, g, b]);
        let side = sw.min(sh) as f32;
        if side >= 52.0 {
            let l = filters::lightness(r, g, b);
            let ink = if l > 60.0 {
                [20, 20, 20]
            } else {
                [245, 245, 245]
            };
            let px = (side * 0.13).clamp(9.0, 28.0);
            let hex = Line::layout(font, &format!("#{r:02X}{g:02X}{b:02X}"), px);
            let value = Line::layout(font, &format!("L* {l:.0}"), px);
            let left = sx as f32 + px * 0.5;
            let bottom = (sy + sh as i64) as f32 - px * 0.5;
            value.draw(font, canvas, left, bottom - value.height, ink, 0.9);
            hex.draw(
                font,
                canvas,
                left,
                bottom - value.height - hex.height,
                ink,
                0.9,
            );
        }
    }
}

fn draw_note(
    canvas: &mut RgbaImage,
    font: &FontRef<'_>,
    (x, y, w, h): (i64, i64, u32, u32),
    text: &str,
    color: u8,
    scale: f32,
) {
    let (bg, ink) = NOTE_COLORS[color as usize % NOTE_COLORS.len()];
    fill_rect(canvas, (x, y, w, h), bg);
    let px = NOTE_FONT * scale;
    let pad = NOTE_PAD * scale;
    let bottom = (y + h as i64) as f32 - pad * 0.5;
    let mut top = y as f32 + pad;
    for line in wrap(font, text, px, w as f32 - 2.0 * pad) {
        let line = Line::layout(font, &line, px);
        if top + line.height > bottom {
            break;
        }
        line.draw(font, canvas, x as f32 + pad, top, ink, 1.0);
        top += px * 1.25;
    }
}

fn draw_frame(
    canvas: &mut RgbaImage,
    font: &FontRef<'_>,
    (x, y, w, h): (i64, i64, u32, u32),
    title: &str,
    scale: f32,
) {
    let title_h = ((FRAME_TITLE_HEIGHT * scale).round() as u32).min(h);
    blend_rect(canvas, (x, y, w, h), [255, 255, 255], 10.0 / 255.0);
    blend_rect(canvas, (x, y, w, title_h), [255, 255, 255], 22.0 / 255.0);
    let t = (1.5 * scale).round().max(1.0) as u32;
    let border = [95, 95, 95];
    fill_rect(canvas, (x, y, w, t), border);
    fill_rect(canvas, (x, y + h as i64 - t as i64, w, t), border);
    fill_rect(canvas, (x, y, t, h), border);
    fill_rect(canvas, (x + w as i64 - t as i64, y, t, h), border);
    let px = FRAME_FONT * scale;
    let line = Line::layout(font, title, px);
    let ty = y as f32 + (title_h as f32 - line.height) / 2.0;
    line.draw(
        font,
        canvas,
        x as f32 + 10.0 * scale,
        ty,
        [235, 235, 235],
        1.0,
    );
}

/// An empty slot: a dashed outline with its label in the middle.
fn draw_placeholder(
    canvas: &mut RgbaImage,
    font: &FontRef<'_>,
    (x, y, w, h): (i64, i64, u32, u32),
    label: &str,
    scale: f32,
) {
    blend_rect(canvas, (x, y, w, h), [255, 255, 255], 9.0 / 255.0);
    let t = (1.5 * scale).round().max(1.0) as u32;
    let (dash, gap) = (((8.0 * scale) as i64).max(2), ((6.0 * scale) as i64).max(2));
    let ink = [140, 140, 140];
    let mut along = 0;
    while along < w as i64 {
        let len = dash.min(w as i64 - along) as u32;
        fill_rect(canvas, (x + along, y, len, t), ink);
        fill_rect(canvas, (x + along, y + h as i64 - t as i64, len, t), ink);
        along += dash + gap;
    }
    let mut along = 0;
    while along < h as i64 {
        let len = dash.min(h as i64 - along) as u32;
        fill_rect(canvas, (x, y + along, t, len), ink);
        fill_rect(canvas, (x + w as i64 - t as i64, y + along, t, len), ink);
        along += dash + gap;
    }
    let text = if label.trim().is_empty() {
        "Image to find"
    } else {
        label.trim()
    };
    let line = Line::layout(font, text, crate::board::PLACEHOLDER_FONT * scale);
    let lx = x as f32 + (w as f32 - line.width) / 2.0;
    let ly = y as f32 + (h as f32 - line.height) / 2.0;
    line.draw(font, canvas, lx, ly, [210, 210, 210], 1.0);
}

/// Greedy word wrap into lines no wider than `max_width` (a single long word may overflow).
fn wrap(font: &FontRef<'_>, text: &str, px: f32, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && Line::layout(font, &candidate, px).width > max_width {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines
}

/// Mixes `color` over the canvas at `alpha`.
fn blend_rect(
    canvas: &mut RgbaImage,
    (x, y, w, h): (i64, i64, u32, u32),
    color: [u8; 3],
    alpha: f32,
) {
    let (cw, ch) = (canvas.width() as i64, canvas.height() as i64);
    for py in y.max(0)..(y + h as i64).min(ch) {
        for px in x.max(0)..(x + w as i64).min(cw) {
            let p = canvas.get_pixel_mut(px as u32, py as u32);
            for c in 0..3 {
                p[c] = (color[c] as f32 * alpha + p[c] as f32 * (1.0 - alpha)).round() as u8;
            }
        }
    }
}

/// Drawings on an item, as on the board: a dark halo under each colored stroke.
fn draw_marks(
    canvas: &mut RgbaImage,
    (x, y, w, h): (i64, i64, u32, u32),
    marks: &[Mark],
    scale: f32,
) {
    let width = (MARK_WIDTH * scale).clamp(1.5, 24.0);
    for mark in marks {
        let lines = mark.polylines(w as f32, h as f32, width);
        let color = MARK_COLORS[mark.color as usize % MARK_COLORS.len()];
        for (radius, rgb, alpha) in [
            ((width + 2.5) / 2.0, [0, 0, 0], 110.0 / 255.0),
            (width / 2.0, color, 1.0),
        ] {
            for line in &lines {
                for pair in line.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    let steps = ((b[0] - a[0]).hypot(b[1] - a[1]) / (radius * 0.5).max(0.5))
                        .ceil()
                        .max(1.0) as usize;
                    for i in 0..=steps {
                        let t = i as f32 / steps as f32;
                        let cx = x as f32 + a[0] + (b[0] - a[0]) * t;
                        let cy = y as f32 + a[1] + (b[1] - a[1]) * t;
                        stamp_disc(canvas, (x, y, w, h), cx, cy, radius, rgb, alpha);
                    }
                }
            }
        }
    }
}

/// A filled, slightly soft-edged disc, blended over the canvas within the item's rect `clip`
/// (drawings on cropped-away parts stay hidden, as on the board).
fn stamp_disc(
    canvas: &mut RgbaImage,
    clip: (i64, i64, u32, u32),
    cx: f32,
    cy: f32,
    r: f32,
    rgb: [u8; 3],
    alpha: f32,
) {
    let (lx, ly) = (clip.0.max(0), clip.1.max(0));
    let hx = (clip.0 + clip.2 as i64).min(canvas.width() as i64);
    let hy = (clip.1 + clip.3 as i64).min(canvas.height() as i64);
    let (x0, x1) = ((cx - r - 1.0).floor() as i64, (cx + r + 1.0).ceil() as i64);
    let (y0, y1) = ((cy - r - 1.0).floor() as i64, (cy + r + 1.0).ceil() as i64);
    for py in y0.max(ly)..y1.min(hy) {
        for px in x0.max(lx)..x1.min(hx) {
            let d = (px as f32 + 0.5 - cx).hypot(py as f32 + 0.5 - cy);
            let coverage = (r + 0.5 - d).clamp(0.0, 1.0) * alpha;
            if coverage <= 0.0 {
                continue;
            }
            let p = canvas.get_pixel_mut(px as u32, py as u32);
            for c in 0..3 {
                // Stamps overlap along a stroke; keep the strongest coverage, don't stack it.
                let target = rgb[c] as f32;
                let current = p[c] as f32;
                let blended = target * coverage + current * (1.0 - coverage);
                p[c] = if (target - blended).abs() < (target - current).abs() {
                    blended.round() as u8
                } else {
                    p[c]
                };
            }
        }
    }
}

fn fill_rect(canvas: &mut RgbaImage, (x, y, w, h): (i64, i64, u32, u32), [r, g, b]: [u8; 3]) {
    let (cw, ch) = (canvas.width() as i64, canvas.height() as i64);
    for py in y.max(0)..(y + h as i64).min(ch) {
        for px in x.max(0)..(x + w as i64).min(cw) {
            canvas.put_pixel(px as u32, py as u32, Rgba([r, g, b, 255]));
        }
    }
}

/// Draws the watermark text, with a soft shadow so it reads on light and dark images alike.
fn stamp(canvas: &mut RgbaImage, text: &str, watermark: &Watermark) {
    let font =
        FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("bundled font is valid");
    let (w, h) = (canvas.width() as f32, canvas.height() as f32);
    let px = (w.min(h) * 0.045).max(14.0);
    let line = Line::layout(&font, text, px);
    let margin = px * 0.8;
    let opacity = watermark.opacity.clamp(0.1, 1.0);

    let mut spots = Vec::new();
    match watermark.position {
        WatermarkPosition::TopLeft => spots.push((margin, margin)),
        WatermarkPosition::TopRight => spots.push((w - line.width - margin, margin)),
        WatermarkPosition::BottomLeft => spots.push((margin, h - line.height - margin)),
        WatermarkPosition::BottomRight => {
            spots.push((w - line.width - margin, h - line.height - margin))
        }
        WatermarkPosition::Center => spots.push(((w - line.width) / 2.0, (h - line.height) / 2.0)),
        WatermarkPosition::Tiled => {
            let step_x = line.width + px * 3.0;
            let step_y = line.height * 3.0;
            let mut row = 0;
            let mut y = -line.height;
            while y < h {
                // Offset every other row so the pattern reads as a lattice, not columns.
                let mut x = if row % 2 == 0 { -step_x / 2.0 } else { 0.0 };
                while x < w {
                    spots.push((x, y));
                    x += step_x;
                }
                y += step_y;
                row += 1;
            }
        }
    }

    let shadow = (px / 16.0).max(1.0);
    for &(x, y) in &spots {
        line.draw(
            &font,
            canvas,
            x + shadow,
            y + shadow,
            [0, 0, 0],
            opacity * 0.55,
        );
        line.draw(&font, canvas, x, y, [255, 255, 255], opacity);
    }
}

/// A single line of laid-out glyphs, positioned relative to its top-left corner.
struct Line {
    glyphs: Vec<ab_glyph::Glyph>,
    width: f32,
    height: f32,
}

impl Line {
    fn layout(font: &FontRef<'_>, text: &str, px: f32) -> Self {
        let scaled = font.as_scaled(PxScale::from(px));
        let mut glyphs = Vec::new();
        let mut caret = 0.0;
        let mut previous = None;
        for ch in text.chars() {
            let id = scaled.glyph_id(ch);
            if let Some(previous) = previous {
                caret += scaled.kern(previous, id);
            }
            glyphs.push(id.with_scale_and_position(px, point(caret, scaled.ascent())));
            caret += scaled.h_advance(id);
            previous = Some(id);
        }
        Self {
            glyphs,
            width: caret,
            height: scaled.ascent() - scaled.descent(),
        }
    }

    fn draw(
        &self,
        font: &FontRef<'_>,
        canvas: &mut RgbaImage,
        x: f32,
        y: f32,
        color: [u8; 3],
        opacity: f32,
    ) {
        for glyph in &self.glyphs {
            let mut glyph = glyph.clone();
            glyph.position = point(glyph.position.x + x, glyph.position.y + y);
            let Some(outline) = font.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i64 + gx as i64;
                let py = bounds.min.y as i64 + gy as i64;
                if px < 0 || py < 0 || px >= canvas.width() as i64 || py >= canvas.height() as i64 {
                    return;
                }
                let alpha = coverage * opacity;
                let dst = canvas.get_pixel_mut(px as u32, py as u32);
                for c in 0..3 {
                    dst[c] =
                        (color[c] as f32 * alpha + dst[c] as f32 * (1.0 - alpha)).round() as u8;
                }
            });
        }
    }
}

/// UTC `YYYY-MM-DD-HHMMSS`, so exports sort by time and never overwrite each other.
fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
