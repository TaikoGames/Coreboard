//! Sorting pictures by value (light to dark) or by color (around the color wheel), laid out in
//! rows like the grid arrange: to see a board's mood at a glance, or find every warm reference.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum SortBy {
    /// Lightest first.
    Value,
    /// Around the color wheel from red, with grays and near-grays last (light to dark).
    Color,
}

/// What a picture looks like overall: its average lightness (L*, 0-100), its main hue
/// (degrees) and how colorful it is (0-1).
#[derive(Clone, Copy, Debug)]
struct Look {
    lightness: f32,
    hue: f32,
    colorful: f32,
}

/// Below this, a picture reads as gray and sorts with the grays.
const GRAYISH: f32 = 0.12;

fn look(img: &image::RgbaImage) -> Option<Look> {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    // About 4000 pixels is plenty to judge a whole picture.
    let step = ((w as f32 * h as f32 / 4000.0).sqrt().ceil() as u32).max(1);
    let mut light = 0.0;
    let mut count = 0.0;
    let mut colorful = 0.0;
    // Hues weighted by how saturated they are, in 36 bins of 10 degrees.
    let mut bins = [0.0f32; 36];
    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            let [r, g, b, a] = img.get_pixel(x, y).0;
            if a < 16 {
                continue;
            }
            light += filters::lightness(r, g, b);
            let (hue, sat, val) = filters::hsv(r, g, b);
            let weight = sat * val;
            colorful += weight;
            bins[((hue / 10.0) as usize).min(35)] += weight;
            count += 1.0;
        }
    }
    if count == 0.0 {
        return None;
    }
    // The strongest hue, smoothed with its neighbours so one stray bin doesn't win.
    let smoothed = |i: usize| bins[(i + 35) % 36] + 2.0 * bins[i] + bins[(i + 1) % 36];
    let peak = (0..36).max_by(|&a, &b| smoothed(a).total_cmp(&smoothed(b)))?;
    Some(Look {
        lightness: light / count,
        hue: peak as f32 * 10.0 + 5.0,
        colorful: colorful / count,
    })
}

/// The number pictures are sorted by (lowest first).
fn sort_key(look: Look, by: SortBy) -> f32 {
    match by {
        SortBy::Value => 100.0 - look.lightness,
        SortBy::Color if look.colorful < GRAYISH => 1000.0 + (100.0 - look.lightness),
        SortBy::Color => look.hue,
    }
}

impl CoreBoardApp {
    /// Alt+L / Alt+R: lays the selected pictures (or all of them) out sorted.
    pub(super) fn sort_selection(&mut self, by: SortBy) {
        let targets = self.targets();
        let pictures: Vec<(u64, String)> = self
            .board
            .items
            .iter()
            .filter(|i| targets.contains(&i.id) && i.is_image() && i.movable())
            .map(|i| (i.id, i.file.clone()))
            .collect();
        if pictures.len() < 2 {
            self.toast(tr("Select two or more pictures to sort").into());
            return;
        }
        let mut keys = HashMap::new();
        for (id, file) in pictures {
            // The small copy the color picker keeps, when the picture has been shown; else
            // the file itself, read small.
            let look = match self.samples.get(&file) {
                Some(sample) => look(sample),
                None => image::open(self.store.image_path(&file))
                    .ok()
                    .map(|img| img.thumbnail(256, 256).to_rgba8())
                    .and_then(|img| look(&img)),
            };
            if let Some(look) = look {
                keys.insert(id, sort_key(look, by));
            }
        }
        self.arrange_grid_by(false, Some(&keys));
        self.toast(
            match by {
                SortBy::Value => tr("Sorted from light to dark"),
                SortBy::Color => tr("Sorted by color, grays last"),
            }
            .into(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(rgb: [u8; 3]) -> image::RgbaImage {
        image::RgbaImage::from_pixel(20, 20, image::Rgba([rgb[0], rgb[1], rgb[2], 255]))
    }

    #[test]
    fn judges_lightness_and_hue() {
        let white = look(&plain([250, 250, 250])).unwrap();
        let navy = look(&plain([20, 30, 90])).unwrap();
        let red = look(&plain([220, 30, 30])).unwrap();
        let green = look(&plain([40, 180, 60])).unwrap();
        let gray = look(&plain([128, 128, 128])).unwrap();
        // Light to dark.
        assert!(sort_key(white, SortBy::Value) < sort_key(gray, SortBy::Value));
        assert!(sort_key(gray, SortBy::Value) < sort_key(navy, SortBy::Value));
        // Around the wheel: red, then green, then blue; grays after every color.
        assert!(sort_key(red, SortBy::Color) < sort_key(green, SortBy::Color));
        assert!(sort_key(green, SortBy::Color) < sort_key(navy, SortBy::Color));
        assert!(sort_key(navy, SortBy::Color) < sort_key(gray, SortBy::Color));
        assert!(sort_key(white, SortBy::Color) < sort_key(gray, SortBy::Color));
    }

    #[test]
    fn a_mostly_gray_picture_with_a_splash_of_color_sorts_by_the_splash() {
        let mut img = plain([120, 120, 120]);
        for y in 0..20 {
            for x in 0..8 {
                img.put_pixel(x, y, image::Rgba([30, 60, 230, 255]));
            }
        }
        let look = look(&img).unwrap();
        assert!((200.0..260.0).contains(&look.hue), "hue {}", look.hue);
    }
}
