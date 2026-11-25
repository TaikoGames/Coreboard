//! Two study tools that take over clicks while they're on:
//!
//! - **Measure** (K): drag across something to make it the unit (a head, say), then drag across
//!   anything else to see how many units long it is. The classic way to check proportions.
//! - **Perspective** (J): click to put up to three vanishing points on a picture; lines fan out
//!   from each across the picture, with the horizon through the first two. Drag a point to
//!   move it. The points are saved with the picture.

use super::*;

pub(super) enum Tool {
    /// Lines in board coordinates, so they stay put while panning and zooming.
    Measure {
        unit: Option<[Pos2; 2]>,
        lines: Vec<[Pos2; 2]>,
        /// Where the line being dragged started.
        from: Option<Pos2>,
    },
    Perspective {
        id: u64,
        /// The vanishing point being dragged.
        moving: Option<usize>,
    },
}

/// One color per vanishing point.
const POINT_COLORS: [Color32; 3] = [
    Color32::from_rgb(255, 150, 60),
    Color32::from_rgb(80, 200, 255),
    Color32::from_rgb(235, 105, 235),
];
/// Lines fanning from each point across the picture (more when it sits inside the picture).
const RAYS_ACROSS: usize = 11;
const RAYS_AROUND: usize = 16;
/// How close (screen points) a click must be to grab a vanishing point.
const GRAB: f32 = 12.0;

/// The part of segment a-b inside `rect` (Liang–Barsky), if any.
fn clip_segment(a: Pos2, b: Pos2, rect: Rect) -> Option<(Pos2, Pos2)> {
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
fn clip_line(a: Pos2, b: Pos2, rect: Rect) -> Option<(Pos2, Pos2)> {
    let d = (b - a).normalized() * (rect.size().length() * 4.0 + a.distance(rect.center()) * 2.0);
    if !d.x.is_finite() || d == Vec2::ZERO {
        return None;
    }
    clip_segment(a - d, a + d, rect)
}

/// Directions (radians) for the lines fanning out from `p` so they cover `rect` evenly: spread
/// between the picture's outermost corners as seen from the point, or all the way round when
/// the point is on the picture.
fn ray_angles(p: Pos2, rect: Rect) -> Vec<f32> {
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

/// "2.4×" (how many units long), or "unit".
fn units_label(length: f32, unit: Option<f32>) -> String {
    match unit {
        Some(unit) if unit > 0.0 => {
            let n = length / unit;
            // Halves read naturally for proportions ("3½"), decimals for the rest.
            let halves = (n * 2.0).round() / 2.0;
            if (n - halves).abs() < 0.04 {
                let whole = halves.trunc() as i32;
                match (whole, halves.fract() > 0.0) {
                    (0, true) => "½".to_string(),
                    (w, true) => format!("{w}½"),
                    (w, false) => format!("{w}"),
                }
            } else {
                format!("{n:.1}")
            }
        }
        _ => tr("unit").to_string(),
    }
}

impl CoreBoardApp {
    fn end_other_modes(&mut self) {
        self.crop_mode = None;
        self.draw_mode = false;
        self.picker = false;
        self.radial = None;
    }

    /// K: measuring on or off.
    pub(super) fn toggle_measure(&mut self) {
        if matches!(self.tool, Some(Tool::Measure { .. })) {
            self.tool = None;
            return;
        }
        self.end_other_modes();
        self.tool = Some(Tool::Measure {
            unit: None,
            lines: Vec::new(),
            from: None,
        });
        self.toast(tr("Measure: drag across something to use it as the unit (a head, say)").into());
    }

    /// J: perspective lines for the selected picture, on or off.
    pub(super) fn toggle_perspective(&mut self) {
        if matches!(self.tool, Some(Tool::Perspective { .. })) {
            self.tool = None;
            return;
        }
        let picked: Vec<u64> = self
            .board
            .items
            .iter()
            .filter(|i| self.selection.contains(&i.id) && i.is_image())
            .map(|i| i.id)
            .collect();
        let [id] = picked.as_slice() else {
            self.toast(tr("Select one picture, then press J for perspective lines").into());
            return;
        };
        self.end_other_modes();
        self.tool = Some(Tool::Perspective {
            id: *id,
            moving: None,
        });
        self.toast(tr("Perspective: click where lines meet to add a vanishing point").into());
    }

    /// Shift+J: removes the selected pictures' vanishing points.
    pub(super) fn clear_perspective(&mut self) {
        let any = self
            .board
            .items
            .iter()
            .any(|i| self.selection.contains(&i.id) && !i.vanishing.is_empty());
        if !any {
            return;
        }
        self.checkpoint();
        for item in &mut self.board.items {
            if self.selection.contains(&item.id) {
                item.vanishing.clear();
            }
        }
        self.touch();
        self.toast(tr("Perspective lines removed").into());
    }

    /// Backspace while a tool is on: takes back the last line or point.
    pub(super) fn tool_back(&mut self) {
        match &mut self.tool {
            Some(Tool::Measure { unit, lines, .. }) => {
                if lines.pop().is_none() {
                    *unit = None;
                }
            }
            Some(Tool::Perspective { id, .. }) => {
                let id = *id;
                if self
                    .board
                    .items
                    .iter()
                    .any(|i| i.id == id && !i.vanishing.is_empty())
                {
                    self.checkpoint();
                    if let Some(item) = self.board.items.iter_mut().find(|i| i.id == id) {
                        item.vanishing.pop();
                    }
                    self.touch();
                }
            }
            None => {}
        }
    }

    /// A vanishing point of `item` on screen.
    fn point_on_screen(&self, canvas: Rect, item: &Item, p: [f32; 2]) -> Pos2 {
        let rect = self.item_screen_rect(canvas, item);
        let [x, y] = item.mark_to_local(p);
        rect.min + vec2(x, y) * rect.size()
    }

    /// A left click while a tool is on. True when the tool took it.
    pub(super) fn tool_press(&mut self, canvas: Rect, pos: Pos2) -> bool {
        let world = self.to_world(canvas, pos);
        match &self.tool {
            Some(Tool::Measure { .. }) => {
                if let Some(Tool::Measure { from, .. }) = &mut self.tool {
                    *from = Some(world);
                }
                true
            }
            Some(Tool::Perspective { id, .. }) => {
                let id = *id;
                let Some(item) = self.board.items.iter().find(|i| i.id == id).cloned() else {
                    self.tool = None;
                    return false;
                };
                // Grab a point near the click, else add one (up to three).
                let near = item
                    .vanishing
                    .iter()
                    .position(|p| self.point_on_screen(canvas, &item, *p).distance(pos) <= GRAB);
                let index = match near {
                    Some(i) => Some(i),
                    None if item.vanishing.len() < POINT_COLORS.len() => Some(item.vanishing.len()),
                    None => {
                        self.toast(
                            tr("Three points at most: drag one to move it, Backspace removes")
                                .into(),
                        );
                        None
                    }
                };
                if let Some(index) = index {
                    self.checkpoint();
                    let rect = self.item_screen_rect(canvas, &item);
                    let local = (pos - rect.min) / rect.size();
                    let mark = item.local_to_mark([local.x, local.y]);
                    if let Some(item) = self.board.items.iter_mut().find(|i| i.id == id) {
                        if index == item.vanishing.len() {
                            item.vanishing.push(mark);
                        } else {
                            item.vanishing[index] = mark;
                        }
                    }
                    if let Some(Tool::Perspective { moving, .. }) = &mut self.tool {
                        *moving = Some(index);
                    }
                    self.touch();
                }
                true
            }
            None => false,
        }
    }

    /// Follows a drag started by `tool_press`, and finishes it on release.
    pub(super) fn tool_drag(&mut self, ctx: &egui::Context, canvas: Rect) {
        let (down, at, shift) = ctx.input(|i| {
            (
                i.pointer.primary_down(),
                i.pointer.latest_pos(),
                i.modifiers.shift,
            )
        });
        if self.tool.is_some() && at.is_some_and(|p| canvas.contains(p)) {
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        let end = at.map(|p| self.to_world(canvas, p));
        match &mut self.tool {
            Some(Tool::Measure {
                unit, lines, from, ..
            }) => {
                let (Some(start), Some(mut end)) = (*from, end) else {
                    return;
                };
                if shift {
                    end = snap_straight(start, end);
                }
                if down {
                    return;
                }
                *from = None;
                // Too short to be a measurement: a stray click.
                if start.distance(end) * self.board.view.zoom < 6.0 {
                    return;
                }
                if unit.is_none() {
                    *unit = Some([start, end]);
                    self.toast(tr("Unit set: now drag across anything to compare").into());
                } else {
                    lines.push([start, end]);
                }
            }
            Some(Tool::Perspective { id, moving }) => {
                let (id, Some(index)) = (*id, *moving) else {
                    return;
                };
                if !down {
                    *moving = None;
                    return;
                }
                let Some(at) = at else { return };
                let Some(item) = self.board.items.iter().find(|i| i.id == id).cloned() else {
                    return;
                };
                let rect = self.item_screen_rect(canvas, &item);
                let local = (at - rect.min) / rect.size();
                let mark = item.local_to_mark([local.x, local.y]);
                if let Some(item) = self.board.items.iter_mut().find(|i| i.id == id)
                    && let Some(p) = item.vanishing.get_mut(index)
                {
                    *p = mark;
                }
                self.touch();
            }
            None => {}
        }
    }

    /// Perspective lines on every picture that has vanishing points, and the measuring lines.
    pub(super) fn paint_tools(&self, ctx: &egui::Context, painter: &egui::Painter, canvas: Rect) {
        let editing = match &self.tool {
            Some(Tool::Perspective { id, .. }) => Some(*id),
            _ => None,
        };
        for item in self.board.items.iter().filter(|i| !i.vanishing.is_empty()) {
            self.paint_perspective(painter, canvas, item, editing == Some(item.id));
        }
        if let Some(Tool::Measure { unit, lines, from }) = &self.tool {
            let (at, shift) = ctx.input(|i| (i.pointer.latest_pos(), i.modifiers.shift));
            let unit_len = unit.map(|[a, b]| a.distance(b));
            let mut all: Vec<([Pos2; 2], bool)> = Vec::new();
            if let Some(u) = unit {
                all.push((*u, true));
            }
            all.extend(lines.iter().map(|l| (*l, false)));
            if let (Some(start), Some(at)) = (*from, at) {
                let mut end = self.to_world(canvas, at);
                if shift {
                    end = snap_straight(start, end);
                }
                all.push(([start, end], unit.is_none()));
            }
            for ([a, b], is_unit) in all {
                let (sa, sb) = (self.to_screen(canvas, a), self.to_screen(canvas, b));
                let color = if is_unit {
                    ACCENT
                } else {
                    Color32::from_rgb(255, 210, 80)
                };
                painter.line_segment([sa, sb], Stroke::new(4.0, Color32::from_black_alpha(140)));
                painter.line_segment([sa, sb], Stroke::new(2.0, color));
                // Small ticks across each end.
                let n = (sb - sa).normalized().rot90() * 6.0;
                for p in [sa, sb] {
                    painter.line_segment([p - n, p + n], Stroke::new(2.0, color));
                }
                let label = if is_unit {
                    tr("unit").to_string()
                } else {
                    units_label(a.distance(b), unit_len)
                };
                let galley =
                    painter.layout_no_wrap(label, FontId::proportional(14.0), Color32::BLACK);
                let mid = sa.lerp(sb, 0.5) - n * 2.4;
                let bg = Rect::from_center_size(mid, galley.size() + vec2(10.0, 4.0));
                painter.rect_filled(bg, CornerRadius::same(6), color);
                painter.galley(bg.center() - galley.size() / 2.0, galley, Color32::BLACK);
            }
        }
        // What the tool does, while it's on.
        let hint = match &self.tool {
            Some(Tool::Measure { unit: None, .. }) => Some(tr(
                "Measuring · drag across the unit (a head, say) · Shift keeps it straight · Esc done",
            )),
            Some(Tool::Measure { .. }) => Some(tr(
                "Measuring · drag to compare · Backspace undo · Shift keeps it straight · Esc done",
            )),
            Some(Tool::Perspective { .. }) => Some(tr(
                "Perspective · click to add a vanishing point · drag to move · Backspace remove · Esc done",
            )),
            None => None,
        };
        if let Some(hint) = hint {
            let galley = painter.layout_no_wrap(
                hint.to_string(),
                FontId::proportional(13.0),
                Color32::WHITE,
            );
            let bg = Rect::from_center_size(
                pos2(canvas.center().x, canvas.min.y + 22.0),
                galley.size() + vec2(20.0, 10.0),
            );
            painter.rect_filled(bg, CornerRadius::same(12), Color32::from_black_alpha(200));
            painter.galley(bg.center() - galley.size() / 2.0, galley, Color32::WHITE);
        }
    }

    fn paint_perspective(&self, painter: &egui::Painter, canvas: Rect, item: &Item, editing: bool) {
        let rect = self.item_screen_rect(canvas, item);
        if !rect.intersects(canvas) && !editing {
            return;
        }
        let points: Vec<Pos2> = item
            .vanishing
            .iter()
            .map(|p| self.point_on_screen(canvas, item, *p))
            .collect();
        let inside = painter.with_clip_rect(rect.intersect(canvas));
        for (p, color) in points.iter().zip(POINT_COLORS) {
            let reach = rect.size().length() * 2.0 + p.distance(rect.center()) * 2.0;
            for angle in ray_angles(*p, rect) {
                let far = *p + Vec2::angled(angle) * reach;
                if let Some((a, b)) = clip_segment(*p, far, rect) {
                    inside.line_segment([a, b], Stroke::new(2.5, Color32::from_black_alpha(80)));
                    inside.line_segment([a, b], Stroke::new(1.2, color));
                }
            }
        }
        // The horizon: through the first two points, or level through a single one.
        if let Some(&first) = points.first() {
            let second = points.get(1).copied().unwrap_or(first + vec2(100.0, 0.0));
            let area = if editing { canvas } else { rect };
            if let Some((a, b)) = clip_line(first, second, area) {
                let p = painter.with_clip_rect(area);
                p.line_segment([a, b], Stroke::new(4.0, Color32::from_black_alpha(120)));
                p.line_segment([a, b], Stroke::new(2.0, ACCENT));
            }
        }
        if editing {
            for (n, (p, color)) in points.iter().zip(POINT_COLORS).enumerate() {
                painter.circle_filled(*p, 9.0, Color32::from_black_alpha(200));
                painter.circle_stroke(*p, 9.0, Stroke::new(2.0, color));
                painter.text(
                    *p,
                    Align2::CENTER_CENTER,
                    (n + 1).to_string(),
                    FontId::proportional(11.0),
                    Color32::WHITE,
                );
            }
        }
    }
}

/// With Shift: the end moved so the line is level or upright, whichever it's closer to.
fn snap_straight(start: Pos2, end: Pos2) -> Pos2 {
    let d = end - start;
    if d.x.abs() >= d.y.abs() {
        pos2(end.x, start.y)
    } else {
        pos2(start.x, end.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn says_lengths_in_units() {
        assert_eq!(units_label(35.0, Some(10.0)), "3½");
        assert_eq!(units_label(70.0, Some(10.0)), "7");
        assert_eq!(units_label(5.0, Some(10.0)), "½");
        assert_eq!(units_label(23.0, Some(10.0)), "2.3");
        assert_eq!(
            snap_straight(pos2(0.0, 0.0), pos2(10.0, 3.0)),
            pos2(10.0, 0.0)
        );
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0));
        // From the left, every line crosses the picture.
        let from = pos2(-200.0, 50.0);
        let angles = ray_angles(from, rect);
        assert_eq!(angles.len(), RAYS_ACROSS);
        for a in angles {
            assert!(clip_segment(from, from + Vec2::angled(a) * 1000.0, rect).is_some());
        }
        assert_eq!(ray_angles(pos2(50.0, 50.0), rect).len(), RAYS_AROUND);
    }
}
