//! Perspective lines: the fans of lines from a picture's vanishing points, and its horizon.
//! Shared by the board, focus view and exports, so all three draw the same lines.

use eframe::egui::{Pos2, Rect, Vec2};

/// One color per vanishing point.
pub const COLORS: [[u8; 3]; 3] = [[255, 150, 60], [80, 200, 255], [235, 105, 235]];
/// Lines fanning from each point across the picture (more when it sits inside the picture).
pub const RAYS_ACROSS: usize = 11;
pub const RAYS_AROUND: usize = 16;

/// What to draw for a picture at `rect`.
pub struct Lines {
    /// Each line, with which point (0, 1, 2) it comes from.
    pub rays: Vec<(Pos2, Pos2, usize)>,
    /// Through the first two points, or level through a single one; across `horizon_area`.
    pub horizon: Option<(Pos2, Pos2)>,
}

/// The lines for a picture drawn at `rect` with vanishing points `points` (same coordinates).
pub fn lines(rect: Rect, points: &[Pos2], horizon_area: Rect) -> Lines {
    let mut rays = Vec::new();
    for (n, p) in points.iter().enumerate() {
        let reach = rect.size().length() * 2.0 + p.distance(rect.center()) * 2.0;
        for angle in ray_angles(*p, rect) {
            let far = *p + Vec2::angled(angle) * reach;
            if let Some((a, b)) = clip_segment(*p, far, rect) {
                rays.push((a, b, n));
            }
        }
    }
    let horizon = points.first().and_then(|&first| {
        let second = points
            .get(1)
            .copied()
            .unwrap_or(first + Vec2::new(100.0, 0.0));
        clip_line(first, second, horizon_area)
    });
    Lines { rays, horizon }
}

/// The part of segment a-b inside `rect` (Liang–Barsky), if any.
pub fn clip_segment(a: Pos2, b: Pos2, rect: Rect) -> Option<(Pos2, Pos2)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [
        (-d.x, a.x - rect.min.x),
        (d.x, rect.max.x - a.x),
        (-d.y, a.y - rect.min.y),
        (d.y, rect.max.y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            t0 = t0.max(t);
        } else {
            t1 = t1.min(t);
        }
        if t0 > t1 {
            return None;
        }
    }
    Some((a + d * t0, a + d * t1))
}

/// The whole line through a and b, as far as `rect` shows it.
pub fn clip_line(a: Pos2, b: Pos2, rect: Rect) -> Option<(Pos2, Pos2)> {
    let d = (b - a).normalized() * (rect.size().length() * 4.0 + a.distance(rect.center()) * 2.0);
    if !d.x.is_finite() || d == Vec2::ZERO {
        return None;
    }
    clip_segment(a - d, a + d, rect)
}

/// Directions (radians) for the lines fanning out from `p` so they cover `rect` evenly: spread
/// between the picture's outermost corners as seen from the point, or all the way round when
/// the point is on the picture.
pub fn ray_angles(p: Pos2, rect: Rect) -> Vec<f32> {
    use std::f32::consts::{PI, TAU};
    if rect.contains(p) {
        return (0..RAYS_AROUND)
            .map(|i| i as f32 / RAYS_AROUND as f32 * TAU)
            .collect();
    }
    let toward = (rect.center() - p).angle();
    let wrap = |a: f32| (a + PI).rem_euclid(TAU) - PI;
    let (lo, hi) = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ]
    .iter()
    .map(|c| wrap((*c - p).angle() - toward))
    .fold((f32::MAX, f32::MIN), |(lo, hi), a| (lo.min(a), hi.max(a)));
    (0..RAYS_ACROSS)
        .map(|i| toward + lo + (hi - lo) * (i as f32 + 0.5) / RAYS_ACROSS as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    #[test]
    fn clips_lines_to_the_picture() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 50.0));
        // From far left, straight across.
        let (a, b) = clip_segment(pos2(-50.0, 25.0), pos2(250.0, 25.0), rect).unwrap();
        assert_eq!((a, b), (pos2(0.0, 25.0), pos2(100.0, 25.0)));
        // Missing it entirely.
        assert!(clip_segment(pos2(-50.0, -10.0), pos2(250.0, -10.0), rect).is_none());
        // A whole line through two points outside, crossing it diagonally.
        let (a, b) = clip_line(pos2(-10.0, -10.0), pos2(-5.0, -5.0), rect).unwrap();
        assert!(a.distance(pos2(0.0, 0.0)) < 1e-3 && b.distance(pos2(50.0, 50.0)) < 1e-3);
    }

    #[test]
    fn fans_cover_the_picture() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0));
        // From the left, every line crosses the picture.
        let from = pos2(-200.0, 50.0);
        let angles = ray_angles(from, rect);
        assert_eq!(angles.len(), RAYS_ACROSS);
        for a in angles {
            assert!(clip_segment(from, from + Vec2::angled(a) * 1000.0, rect).is_some());
        }
        assert_eq!(ray_angles(pos2(50.0, 50.0), rect).len(), RAYS_AROUND);
        let lines = lines(rect, &[pos2(-200.0, 50.0), pos2(300.0, 50.0)], rect);
        assert_eq!(lines.rays.len(), 2 * RAYS_ACROSS);
        assert!(lines.horizon.is_some());
    }
}
