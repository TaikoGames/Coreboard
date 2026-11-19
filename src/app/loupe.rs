//! Magnifier: hold Z and a round glass follows the pointer, showing the pictures under it
//! enlarged, to check a detail (brush strokes, a texture, a face) without zooming the board.
//! Scrolling while holding Z changes how much it enlarges.

use super::*;

/// The glass's radius on screen.
const RADIUS: f32 = 120.0;
const MIN_POWER: f32 = 2.0;
const MAX_POWER: f32 = 8.0;
/// Corners of the circle; plenty for a smooth edge at this size.
const SEGMENTS: usize = 72;

/// The part of `polygon` inside `rect` (Sutherland–Hodgman, one rect edge at a time).
fn clip_to_rect(polygon: &[Pos2], rect: Rect) -> Vec<Pos2> {
    type Edge = (fn(Pos2, Rect) -> bool, fn(Pos2, Pos2, Rect) -> Pos2);
    let edges: [Edge; 4] = [
        (
            |p, r| p.x >= r.min.x,
            |a, b, r| {
                let t = (r.min.x - a.x) / (b.x - a.x);
                pos2(r.min.x, a.y + (b.y - a.y) * t)
            },
        ),
        (
            |p, r| p.x <= r.max.x,
            |a, b, r| {
                let t = (r.max.x - a.x) / (b.x - a.x);
                pos2(r.max.x, a.y + (b.y - a.y) * t)
            },
        ),
        (
            |p, r| p.y >= r.min.y,
            |a, b, r| {
                let t = (r.min.y - a.y) / (b.y - a.y);
                pos2(a.x + (b.x - a.x) * t, r.min.y)
            },
        ),
        (
            |p, r| p.y <= r.max.y,
            |a, b, r| {
                let t = (r.max.y - a.y) / (b.y - a.y);
                pos2(a.x + (b.x - a.x) * t, r.max.y)
            },
        ),
    ];
    let mut out = polygon.to_vec();
    for (inside, cross) in edges {
        let input = std::mem::take(&mut out);
        for (i, &cur) in input.iter().enumerate() {
            let prev = input[(i + input.len() - 1) % input.len()];
            match (inside(prev, rect), inside(cur, rect)) {
                (true, true) => out.push(cur),
                (true, false) => out.push(cross(prev, cur, rect)),
                (false, true) => {
                    out.push(cross(prev, cur, rect));
                    out.push(cur);
                }
                (false, false) => {}
            }
        }
        if out.is_empty() {
            break;
        }
    }
    out
}

/// The texture coordinates at `p` inside `rect`, for an image drawn there with its crop,
/// quarter turns and mirroring (the same corners as `image_shape`).
fn uv_at(item: &Item, rect: Rect, p: Pos2) -> Pos2 {
    let (quarters, flip_x) = (item.rotation % 4, item.flip_x);
    let [u0, v0, u1, v1] = item.crop;
    let uvs = [pos2(u0, v0), pos2(u1, v0), pos2(u1, v1), pos2(u0, v1)];
    let corner = |i: usize| {
        let source = if flip_x { i ^ 1 } else { i };
        uvs[(source + 4 - quarters as usize) % 4]
    };
    let s = (p.x - rect.min.x) / rect.width().max(f32::EPSILON);
    let t = (p.y - rect.min.y) / rect.height().max(f32::EPSILON);
    let top = corner(0).lerp(corner(1), s);
    let bottom = corner(3).lerp(corner(2), s);
    top.lerp(bottom, t)
}

impl CoreBoardApp {
    /// Whether Z is held (and nothing is being typed), so the magnifier shows.
    pub(super) fn loupe_held(&self, ctx: &egui::Context) -> bool {
        !ctx.egui_wants_keyboard_input()
            && ctx.input(|i| i.key_down(Key::Z) && !i.modifiers.command && !i.modifiers.alt)
    }

    /// While the magnifier shows, the wheel changes its power instead of zooming the board.
    pub(super) fn loupe_scroll(&mut self, scroll: f32) {
        let factor = (scroll * 0.004).exp();
        self.loupe_power = (self.loupe_power * factor).clamp(MIN_POWER, MAX_POWER);
    }

    pub(super) fn paint_loupe(&mut self, ctx: &egui::Context, canvas: Rect) {
        if !self.loupe_held(ctx) {
            return;
        }
        let Some(center) = ctx
            .input(|i| i.pointer.hover_pos())
            .filter(|p| canvas.contains(*p))
        else {
            return;
        };
        let power = self.loupe_power;
        // The glass shows the board around the pointer, enlarged about the pointer.
        let magnify = |p: Pos2| center + (p - center) * power;
        let circle: Vec<Pos2> = (0..SEGMENTS)
            .map(|i| {
                let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
                center + vec2(a.cos(), a.sin()) * RADIUS
            })
            .collect();
        let layer =
            egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("coreboard-loupe"));
        let painter = ctx.layer_painter(layer);
        painter.add(egui::Shape::convex_polygon(
            circle.clone(),
            BG,
            Stroke::NONE,
        ));

        let seen = Rect::from_center_size(center, Vec2::splat(2.0 * RADIUS / power));
        let items: Vec<Item> = self
            .draw_order()
            .into_iter()
            .chain(
                self.board
                    .items
                    .iter()
                    .filter(|i| i.pinned.is_some())
                    .cloned(),
            )
            .filter(|i| i.is_image())
            .collect();
        for item in &items {
            let rect = self.item_screen_rect(canvas, item);
            if !rect.intersects(seen) {
                continue;
            }
            let filter = match self.board_values {
                1 => Filter::Gray,
                2 => Filter::Study(Analysis::Values3),
                _ => Filter::for_item(item),
            };
            let Texture::Ready(tex) = self.texture(ctx, &item.file, filter) else {
                continue;
            };
            let big = Rect::from_min_max(magnify(rect.min), magnify(rect.max));
            let shape = clip_to_rect(&circle, big);
            if shape.len() < 3 {
                continue;
            }
            let mut mesh = egui::Mesh::with_texture(tex.id());
            for &p in &shape {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: p,
                    uv: uv_at(item, big, p),
                    color: Color32::WHITE,
                });
            }
            for i in 1..shape.len() as u32 - 1 {
                mesh.add_triangle(0, i, i + 1);
            }
            painter.add(egui::Shape::mesh(mesh));
        }

        // The rim, and how much it enlarges.
        let mut rim = circle.clone();
        rim.push(circle[0]);
        painter.add(egui::Shape::line(
            rim.clone(),
            Stroke::new(5.0, Color32::from_black_alpha(150)),
        ));
        painter.add(egui::Shape::line(rim, Stroke::new(2.0, ACCENT)));
        let label = format!("{power:.1}×");
        let at = center + vec2(0.0, RADIUS + 14.0);
        let galley = painter.layout_no_wrap(label, FontId::proportional(13.0), Color32::WHITE);
        let bg = Rect::from_center_size(at, galley.size() + vec2(12.0, 6.0));
        painter.rect_filled(bg, CornerRadius::same(8), Color32::from_black_alpha(190));
        painter.galley(bg.center() - galley.size() / 2.0, galley, Color32::WHITE);
        ctx.set_cursor_icon(egui::CursorIcon::None);
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_the_glass_to_a_picture() {
        let square = vec![
            pos2(0.0, 0.0),
            pos2(10.0, 0.0),
            pos2(10.0, 10.0),
            pos2(0.0, 10.0),
        ];
        let rect = Rect::from_min_max(pos2(5.0, -5.0), pos2(20.0, 5.0));
        let clipped = clip_to_rect(&square, rect);
        let area: f32 = (0..clipped.len())
            .map(|i| {
                let (a, b) = (clipped[i], clipped[(i + 1) % clipped.len()]);
                a.x * b.y - b.x * a.y
            })
            .sum::<f32>()
            / 2.0;
        assert!((area.abs() - 25.0).abs() < 1e-3);
        assert!(
            clip_to_rect(
                &square,
                Rect::from_min_max(pos2(50.0, 50.0), pos2(60.0, 60.0))
            )
            .is_empty()
        );
    }

    #[test]
    fn maps_points_to_the_right_part_of_the_picture() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 50.0));
        let plain = Item::default();
        assert_eq!(uv_at(&plain, rect, pos2(25.0, 50.0)), pos2(0.25, 1.0));
        // Mirrored: the left edge shows the picture's right side.
        let mirrored = Item {
            flip_x: true,
            ..Item::default()
        };
        assert_eq!(uv_at(&mirrored, rect, pos2(0.0, 0.0)), pos2(1.0, 0.0));
        // Cropped to the right half.
        let cropped = Item {
            crop: [0.5, 0.0, 1.0, 1.0],
            ..Item::default()
        };
        assert_eq!(uv_at(&cropped, rect, pos2(50.0, 25.0)), pos2(0.75, 0.5));
    }
}
