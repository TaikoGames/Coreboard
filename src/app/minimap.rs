//! A small overview of the whole board in the bottom-right corner, with the part on screen
//! outlined. Click or drag in it to go anywhere. It only shows while something is off screen.

use super::*;

const MAP_W: f32 = 200.0;

/// What a press in the map does to the view.
enum Jump {
    /// Glide over to center this spot (world).
    Glide(Pos2),
    /// Center this spot at once.
    Center(Pos2),
    /// Move by this much (world), following a drag.
    By(Vec2),
}
const MAP_H: f32 = 140.0;

/// The world rect covering every item that lives on the board (pinned ones float above it).
fn board_bounds(items: &[Item]) -> Option<Rect> {
    items
        .iter()
        .filter(|i| i.pinned.is_none())
        .map(|i| Rect::from_min_size(pos2(i.pos[0], i.pos[1]), vec2(i.size[0], i.size[1])))
        .reduce(|a, b| a.union(b))
}

/// Maps `area` (world) into a map no bigger than MAP_W x MAP_H, keeping its shape.
fn map_scale(area: Rect) -> f32 {
    (MAP_W / area.width().max(1.0)).min(MAP_H / area.height().max(1.0))
}

impl CoreBoardApp {
    pub(super) fn minimap(&mut self, ctx: &egui::Context, canvas: Rect) {
        if !self.minimap {
            return;
        }
        let Some(bounds) = board_bounds(&self.board.items) else {
            return;
        };
        let view = Rect::from_min_max(
            self.to_world(canvas, canvas.min),
            self.to_world(canvas, canvas.max),
        );
        // While dragging in it, the map holds still (it would otherwise resize as the view
        // moves, shifting under the pointer and running away).
        let frozen = self.minimap_drag;
        if frozen.is_none() && view.contains_rect(bounds) {
            return;
        }
        let area = frozen.unwrap_or_else(|| {
            let area = bounds.union(view);
            area.expand(area.width().max(area.height()) * 0.03)
        });
        let scale = map_scale(area);
        let size = area.size() * scale;
        let to_map = move |origin: Pos2, p: Pos2| origin + (p - area.min) * scale;

        let at = canvas.right_bottom() - size - vec2(14.0, 34.0);
        let items = self.draw_order();
        let filter_of = |item: &Item, values: u8| match values {
            1 => Filter::Gray,
            2 => Filter::Study(Analysis::Values3),
            _ => Filter::for_item(item),
        };
        // Only textures already loaded: the map shouldn't load a big board's every picture.
        let ready: HashMap<u64, egui::TextureId> = items
            .iter()
            .filter(|i| i.is_image())
            .filter_map(|i| {
                let key = filter_of(i, self.board_values).key(&i.file);
                match self
                    .textures
                    .get(&key)
                    .or_else(|| self.textures.get(&i.file))
                {
                    Some(Texture::Ready(tex)) => Some((i.id, tex.id())),
                    _ => None,
                }
            })
            .collect();

        let mut jump = None;
        let mut drag_start = false;
        let mut drag_end = false;
        egui::Area::new(egui::Id::unique("coreboard-minimap"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
                let painter = ui.painter_at(rect.expand(2.0));
                painter.rect_filled(
                    rect.expand(2.0),
                    CornerRadius::same(6),
                    Color32::from_black_alpha(190),
                );
                for item in &items {
                    let r = Rect::from_min_max(
                        to_map(rect.min, pos2(item.pos[0], item.pos[1])),
                        to_map(
                            rect.min,
                            pos2(item.pos[0] + item.size[0], item.pos[1] + item.size[1]),
                        ),
                    );
                    // Tiny things still show as a dot.
                    let r = Rect::from_center_size(r.center(), r.size().max(vec2(2.0, 2.0)));
                    match &item.kind {
                        Kind::Image => match ready.get(&item.id) {
                            Some(tex) => {
                                painter.add(image_shape(*tex, r, item, Color32::WHITE, 0.0));
                            }
                            None => {
                                painter.rect_filled(r, CornerRadius::ZERO, Color32::from_gray(95));
                            }
                        },
                        Kind::Note { color, .. } => {
                            let (bg, _) = NOTE_COLORS[*color as usize % NOTE_COLORS.len()];
                            painter.rect_filled(r, CornerRadius::ZERO, rgb(bg));
                        }
                        Kind::Frame { .. } => {
                            painter.rect_stroke(
                                r,
                                CornerRadius::ZERO,
                                Stroke::new(1.0, Color32::from_gray(110)),
                                StrokeKind::Inside,
                            );
                        }
                        Kind::Palette { colors } => {
                            let c = colors.first().copied().unwrap_or([120, 120, 120]);
                            painter.rect_filled(r, CornerRadius::ZERO, rgb(c));
                        }
                        // Too thin to read at this size.
                        Kind::Drawing { .. } => {}
                        Kind::File { .. } | Kind::Link { .. } => {
                            let c = item.kind.card_face().map_or([120, 120, 130], |f| f.color);
                            painter.rect_filled(r, CornerRadius::ZERO, rgb(c));
                        }
                        Kind::Placeholder { .. } => {
                            painter.rect_stroke(
                                r,
                                CornerRadius::ZERO,
                                Stroke::new(1.0, Color32::from_gray(130)),
                                StrokeKind::Inside,
                            );
                        }
                    }
                }
                // The part of the board on screen.
                let shown =
                    Rect::from_min_max(to_map(rect.min, view.min), to_map(rect.min, view.max))
                        .intersect(rect);
                painter.rect_filled(shown, CornerRadius::ZERO, ACCENT.gamma_multiply(0.12));
                painter.rect_stroke(
                    shown,
                    CornerRadius::ZERO,
                    Stroke::new(1.5, ACCENT),
                    StrokeKind::Inside,
                );
                if response.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.drag_started()
                    && let Some(p) = response.interact_pointer_pos()
                {
                    // Grabbing outside the outlined part first brings it under the pointer.
                    if !shown.contains(p) {
                        jump = Some(Jump::Center(area.min + (p - rect.min) / scale));
                    }
                    drag_start = true;
                } else if response.dragged() {
                    // Then the view follows the pointer by exactly as much as it moves.
                    let moved = response.drag_delta() / scale;
                    if moved != Vec2::ZERO {
                        jump = Some(Jump::By(moved));
                    }
                } else if response.clicked()
                    && let Some(p) = response.interact_pointer_pos()
                {
                    jump = Some(Jump::Glide(area.min + (p - rect.min) / scale));
                }
                drag_end = response.drag_stopped();
            });

        if drag_start {
            self.minimap_drag = Some(area);
        }
        if drag_end {
            self.minimap_drag = None;
        }
        let zoom = self.board.view.zoom;
        let centered = |world: Pos2| {
            let offset = canvas.center() - canvas.min - world.to_vec2() * zoom;
            board::View {
                offset: [offset.x, offset.y],
                zoom,
            }
        };
        match jump {
            // A click glides there.
            Some(Jump::Glide(world)) => self.fly_view(centered(world)),
            Some(Jump::Center(world)) => {
                self.stop_motion();
                self.board.view = centered(world);
                self.touch();
            }
            Some(Jump::By(moved)) => {
                self.stop_motion();
                self.board.view.offset[0] -= moved.x * zoom;
                self.board.view.offset[1] -= moved.y * zoom;
                self.touch();
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_skip_pinned_items() {
        let mut pinned = Item {
            pos: [5000.0, 5000.0],
            size: [10.0, 10.0],
            ..Item::default()
        };
        pinned.pinned = Some([0.0, 0.0, 10.0, 10.0]);
        let items = vec![
            Item {
                pos: [0.0, 0.0],
                size: [100.0, 50.0],
                ..Item::default()
            },
            Item {
                pos: [300.0, -20.0],
                size: [100.0, 100.0],
                ..Item::default()
            },
            pinned,
        ];
        let b = board_bounds(&items).unwrap();
        assert_eq!(b, Rect::from_min_max(pos2(0.0, -20.0), pos2(400.0, 80.0)));
        assert!(board_bounds(&[]).is_none());
    }

    #[test]
    fn map_keeps_the_board_shape() {
        let wide = Rect::from_min_size(pos2(0.0, 0.0), vec2(4000.0, 1000.0));
        assert_eq!(wide.size() * map_scale(wide), vec2(MAP_W, 50.0));
        let tall = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 2000.0));
        assert_eq!(tall.size() * map_scale(tall), vec2(70.0, MAP_H));
    }
}
