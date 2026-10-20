//! Image processing for the study views. Shared by on-screen textures and PNG export, so an
//! export shows exactly what the board shows.

use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageBuffer, Luma, Rgba, RgbaImage};

use crate::board::{Analysis, Item};

/// Heavy filters (blurs) run at this size and are scaled back up; they're about big shapes
/// anyway, so full resolution would only cost time.
const WORK_SIDE: u32 = 1024;

/// What an item's texture shows. Each variant is decoded and cached separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Filter {
    Plain,
    Gray,
    Study(Analysis),
}

impl Filter {
    /// A study view replaces the plain/gray image; otherwise the grayscale toggle applies.
    pub fn for_item(item: &Item) -> Self {
        match (item.analysis, item.grayscale) {
            (Analysis::None, false) => Self::Plain,
            (Analysis::None, true) => Self::Gray,
            (analysis, _) => Self::Study(analysis),
        }
    }

    /// Texture cache key for `file` shown through this filter.
    pub fn key(self, file: &str) -> String {
        match self {
            Self::Plain => file.to_string(),
            Self::Gray => format!("{file}#gray"),
            Self::Study(a) => format!("{file}#{a:?}"),
        }
    }
}

pub fn apply(img: DynamicImage, filter: Filter) -> RgbaImage {
    match filter {
        Filter::Plain => img.to_rgba8(),
        Filter::Gray => img.grayscale().to_rgba8(),
        Filter::Study(Analysis::None) => img.to_rgba8(),
        Filter::Study(Analysis::Values3) => value_bands(img.to_rgba8(), 3),
        Filter::Study(Analysis::Values5) => value_bands(img.to_rgba8(), 5),
        Filter::Study(Analysis::Saturation) => map_pixels(img.to_rgba8(), |[r, g, b]| {
            let s = hsv(r, g, b).1;
            let v = (s * 255.0).round() as u8;
            [v, v, v]
        }),
        Filter::Study(Analysis::Detail) => at_work_size(img, detail_map),
        Filter::Study(Analysis::Squint) => at_work_size(img, |small| {
            let sigma = small.width().max(small.height()) as f32 * 0.02;
            imageops::fast_blur(&small, sigma)
        }),
    }
}

/// Groups perceptual lightness (CIE L*) into `n` evenly spaced tones.
fn value_bands(img: RgbaImage, n: u32) -> RgbaImage {
    // Tones from near-black to near-white, evenly spaced in L* so they look evenly stepped.
    let tones: Vec<u8> = (0..n)
        .map(|i| {
            let l = 8.0 + 87.0 * i as f32 / (n - 1) as f32;
            linear_to_srgb(lightness_to_y(l))
        })
        .collect();
    map_pixels(img, |[r, g, b]| {
        let band = ((lightness(r, g, b) / 100.0 * n as f32) as u32).min(n - 1);
        let t = tones[band as usize];
        [t, t, t]
    })
}

/// Bright where fine detail is dense ("noise"), dark where the image rests.
fn detail_map(small: RgbaImage) -> RgbaImage {
    let (w, h) = small.dimensions();
    let side = w.max(h) as f32;
    let l: ImageBuffer<Luma<f32>, Vec<f32>> = ImageBuffer::from_fn(w, h, |x, y| {
        let [r, g, b, _] = small.get_pixel(x, y).0;
        // The blur expects float images in 0..=1.
        Luma([lightness(r, g, b) / 100.0])
    });
    // High-pass (image minus a slight blur), then average its magnitude over a neighborhood.
    let blurred = imageops::fast_blur(&l, (side * 0.004).max(1.0));
    let mut energy = ImageBuffer::from_fn(w, h, |x, y| {
        Luma([(l.get_pixel(x, y)[0] - blurred.get_pixel(x, y)[0]).abs()])
    });
    energy = imageops::fast_blur(&energy, (side * 0.012).max(1.0));

    // Normalize against a high percentile so one sharp edge doesn't wash out the rest.
    let mut sorted: Vec<f32> = energy.pixels().map(|p| p[0]).collect();
    sorted.sort_by(f32::total_cmp);
    let top = sorted[(sorted.len() as f32 * 0.98) as usize].max(1e-5);

    RgbaImage::from_fn(w, h, |x, y| {
        let t = (energy.get_pixel(x, y)[0] / top).clamp(0.0, 1.0);
        let [r, g, b] = heat(t);
        Rgba([r, g, b, small.get_pixel(x, y)[3]])
    })
}

/// Black → purple → orange → pale yellow, readable from calm to busy.
fn heat(t: f32) -> [u8; 3] {
    const STOPS: [[f32; 3]; 5] = [
        [0.0, 0.0, 4.0],
        [87.0, 16.0, 110.0],
        [188.0, 55.0, 84.0],
        [249.0, 142.0, 9.0],
        [252.0, 255.0, 164.0],
    ];
    let pos = t * (STOPS.len() - 1) as f32;
    let i = (pos as usize).min(STOPS.len() - 2);
    let f = pos - i as f32;
    let mut out = [0u8; 3];
    for c in 0..3 {
        out[c] = (STOPS[i][c] + (STOPS[i + 1][c] - STOPS[i][c]) * f).round() as u8;
    }
    out
}

fn at_work_size(img: DynamicImage, f: impl FnOnce(RgbaImage) -> RgbaImage) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    let small = if w.max(h) > WORK_SIDE {
        img.thumbnail(WORK_SIDE, WORK_SIDE).to_rgba8()
    } else {
        img.to_rgba8()
    };
    let out = f(small);
    if out.dimensions() == (w, h) {
        out
    } else {
        imageops::resize(&out, w, h, FilterType::Triangle)
    }
}

/// Replaces each pixel's color, keeping its alpha.
fn map_pixels(mut img: RgbaImage, f: impl Fn([u8; 3]) -> [u8; 3]) -> RgbaImage {
    for p in img.pixels_mut() {
        let [r, g, b] = f([p[0], p[1], p[2]]);
        p.0 = [r, g, b, p[3]];
    }
    img
}

// ---------------------------------------------------------------- palettes

/// The `k` dominant colors of an image, most common first, via k-means on a small copy.
/// Fully transparent pixels are ignored.
pub fn palette(img: &RgbaImage, k: usize) -> Vec<[u8; 3]> {
    weighted_palette(img, k)
        .into_iter()
        .map(|(c, _)| c)
        .collect()
}

/// Like [`palette`], with each color's share of the image (the shares sum to 1).
pub fn weighted_palette(img: &RgbaImage, k: usize) -> Vec<([u8; 3], f32)> {
    let small = if img.width().max(img.height()) > 160 {
        imageops::thumbnail(img, 160, 160)
    } else {
        img.clone()
    };
    let pixels: Vec<[f32; 3]> = small
        .pixels()
        .filter(|p| p[3] > 16)
        .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
        .collect();
    if pixels.is_empty() || k == 0 {
        return Vec::new();
    }
    let dist = |a: &[f32; 3], b: &[f32; 3]| {
        (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
    };

    // Deterministic farthest-point seeding: start at the mean, then repeatedly take the
    // pixel farthest from every chosen center. Spreads seeds across distinct colors.
    let n = pixels.len() as f32;
    let mean = pixels.iter().fold([0.0; 3], |acc, p| {
        [acc[0] + p[0] / n, acc[1] + p[1] / n, acc[2] + p[2] / n]
    });
    let mut centers = vec![mean];
    let mut nearest: Vec<f32> = pixels.iter().map(|p| dist(p, &mean)).collect();
    while centers.len() < k {
        let (i, &d) = nearest
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap();
        if d < 1.0 {
            break; // Fewer distinct colors than requested.
        }
        let c = pixels[i];
        for (p, best) in pixels.iter().zip(nearest.iter_mut()) {
            *best = best.min(dist(p, &c));
        }
        centers.push(c);
    }

    let mut counts = vec![0usize; centers.len()];
    for _ in 0..20 {
        let mut sums = vec![[0.0f32; 3]; centers.len()];
        counts.iter_mut().for_each(|c| *c = 0);
        for p in &pixels {
            let (j, _) = centers
                .iter()
                .enumerate()
                .map(|(j, c)| (j, dist(p, c)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap();
            counts[j] += 1;
            for c in 0..3 {
                sums[j][c] += p[c];
            }
        }
        let mut moved = 0.0f32;
        for (j, center) in centers.iter_mut().enumerate() {
            if counts[j] > 0 {
                let m = counts[j] as f32;
                let next = [sums[j][0] / m, sums[j][1] / m, sums[j][2] / m];
                moved = moved.max(dist(center, &next));
                *center = next;
            }
        }
        if moved < 0.25 {
            break;
        }
    }

    let mut ranked: Vec<(usize, [u8; 3])> = centers
        .iter()
        .zip(&counts)
        .filter(|&(_, &count)| count > 0)
        .map(|(c, &count)| (count, c.map(|v| v.round().clamp(0.0, 255.0) as u8)))
        .collect();
    ranked.sort_by_key(|r| std::cmp::Reverse(r.0));
    let total = pixels.len() as f32;
    ranked
        .into_iter()
        .map(|(count, c)| (c, count as f32 / total))
        .collect()
}

// ---------------------------------------------------------------- color math

/// HSV as a painter's color picker shows it: hue 0..360, saturation and value 0..1.
pub fn hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let s = if max > 0.0 { d / max } else { 0.0 };
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, max)
}

/// CIE L*a*b* (D65), for color distances that match what the eye sees.
pub fn lab([r, g, b]: [u8; 3]) -> [f32; 3] {
    let (r, g, b) = (srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Perceptual lightness, CIE L*, 0 (black) ..= 100 (white).
pub fn lightness(r: u8, g: u8, b: u8) -> f32 {
    let y = 0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g) + 0.0722 * srgb_to_linear(b);
    if y > 216.0 / 24389.0 {
        116.0 * y.cbrt() - 16.0
    } else {
        y * 24389.0 / 27.0
    }
}

fn lightness_to_y(l: f32) -> f32 {
    if l > 8.0 {
        ((l + 16.0) / 116.0).powi(3)
    } else {
        l * 27.0 / 24389.0
    }
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(y: f32) -> u8 {
    let c = if y <= 0.0031308 {
        y * 12.92
    } else {
        1.055 * y.powf(1.0 / 2.4) - 0.055
    };
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_primaries() {
        assert_eq!(hsv(255, 0, 0), (0.0, 1.0, 1.0));
        let (h, s, v) = hsv(0, 0, 255);
        assert_eq!((h.round(), s, v), (240.0, 1.0, 1.0));
        assert_eq!(hsv(128, 128, 128).1, 0.0);
    }

    #[test]
    fn lightness_endpoints_and_mid_gray() {
        assert!(lightness(0, 0, 0).abs() < 0.01);
        assert!((lightness(255, 255, 255) - 100.0).abs() < 0.01);
        // sRGB 119 is about L* 50.
        assert!((lightness(119, 119, 119) - 50.0).abs() < 0.5);
    }

    #[test]
    fn value_bands_use_n_distinct_tones() {
        let img = RgbaImage::from_fn(256, 1, |x, _| Rgba([x as u8, x as u8, x as u8, 255]));
        let out = value_bands(img, 3);
        let mut tones: Vec<u8> = out.pixels().map(|p| p[0]).collect();
        tones.dedup();
        assert_eq!(tones.len(), 3);
        // Dark to light.
        assert!(tones[0] < tones[1] && tones[1] < tones[2]);
    }

    #[test]
    fn palette_finds_the_main_colors_most_common_first() {
        // 60% red, 30% blue, 10% white.
        let img = RgbaImage::from_fn(100, 10, |x, _| match x {
            0..60 => Rgba([220, 30, 30, 255]),
            60..90 => Rgba([20, 40, 200, 255]),
            _ => Rgba([255, 255, 255, 255]),
        });
        let colors = palette(&img, 3);
        assert_eq!(colors, vec![[220, 30, 30], [20, 40, 200], [255, 255, 255]]);
    }

    #[test]
    fn palette_stops_at_the_number_of_distinct_colors() {
        let img = RgbaImage::from_pixel(20, 20, Rgba([10, 200, 90, 255]));
        assert_eq!(palette(&img, 6), vec![[10, 200, 90]]);
    }

    #[test]
    fn detail_map_is_brighter_where_busy() {
        // Left half flat gray, right half a fine checkerboard.
        let img = RgbaImage::from_fn(200, 100, |x, y| {
            let v = if x < 100 {
                128
            } else if (x + y) % 2 == 0 {
                0
            } else {
                255
            };
            Rgba([v, v, v, 255])
        });
        let out = detail_map(img);
        let calm = out.get_pixel(30, 50);
        let busy = out.get_pixel(170, 50);
        let sum = |p: &Rgba<u8>| p[0] as u32 + p[1] as u32 + p[2] as u32;
        assert!(
            sum(busy) > sum(calm) + 200,
            "busy {busy:?} vs calm {calm:?}"
        );
    }
}
