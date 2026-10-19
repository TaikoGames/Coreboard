//! Renders board items to a PNG, optionally stamped with a text watermark.
//!
//! This works from the original image files on the CPU, so exports are full quality no matter
//! how far the board is zoomed out or how much the on-screen textures were downscaled.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use image::imageops::{self, FilterType};
use image::{DynamicImage, Rgba, RgbaImage};

use crate::board::{Item, Watermark, WatermarkPosition};
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
    let (min, max) = items
        .iter()
        .fold(([f32::MAX; 2], [f32::MIN; 2]), |(min, max), i| {
            (
                [min[0].min(i.pos[0]), min[1].min(i.pos[1])],
                [
                    max[0].max(i.pos[0] + i.size[0]),
                    max[1].max(i.pos[1] + i.size[1]),
                ],
            )
        });
    // One world unit is one pixel, unless that would make the file huge.
    let content = [max[0] - min[0], max[1] - min[1]];
    let scale = (MAX_SIDE / (content[0].max(content[1]) + 2.0 * PADDING)).min(1.0);
    let width = ((content[0] + 2.0 * PADDING) * scale).round().max(1.0) as u32;
    let height = ((content[1] + 2.0 * PADDING) * scale).round().max(1.0) as u32;
    let mut canvas = RgbaImage::from_pixel(width, height, BACKGROUND);

    for item in items {
        let path = images_dir.join(&item.file);
        let img = image::open(&path).map_err(|e| format!("{}: {e}", item.file))?;
        let mut img = DynamicImage::ImageRgba8(filters::apply(img, Filter::for_item(item)));
        img = match item.rotation % 4 {
            1 => img.rotate90(),
            2 => img.rotate180(),
            3 => img.rotate270(),
            _ => img,
        };
        if item.flip_x {
            img = img.fliph();
        }
        let w = (item.size[0] * scale).round().max(1.0) as u32;
        let h = (item.size[1] * scale).round().max(1.0) as u32;
        let img = img.resize_exact(w, h, FilterType::CatmullRom).to_rgba8();
        let x = ((item.pos[0] - min[0] + PADDING) * scale).round() as i64;
        let y = ((item.pos[1] - min[1] + PADDING) * scale).round() as i64;
        imageops::overlay(&mut canvas, &img, x, y);
    }

    let text = watermark.text.trim();
    if !text.is_empty() {
        stamp(&mut canvas, text, watermark);
    }
    Ok(canvas)
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
