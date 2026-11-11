//! Drawing on images: arrows, circles, boxes and pen strokes that stay attached to the picture.

use super::*;
use crate::board::{MARK_COLORS, MARK_WIDTH, Mark, MarkShape};

/// Points closer than this (screen points) to the previous one are skipped while drawing.
const PEN_STEP: f32 = 2.5;

impl CoreBoardApp {
    pub(super) fn toggle_draw_mode(&mut self) {
        self.draw_mode = !self.draw_mode;
        if self.draw_mode {
            self.finish_edit();
            self.crop_mode = None;
            self.show_marks = true;
            self.toast("Draw: drag on an image · Esc or D when done".into());
        } else {
            self.toast("Drawing done".into());
        }
    }

    /// The topmost image under `pos`, which is what a stroke starting there draws on.
    pub(super) fn drawable_at(&self, canvas: Rect, pos: Pos2) -> Option<u64> {
        self.board
            .items
            .iter()
            .rev()
            .filter(|i| i.is_image())
            .find(|i| self.item_screen_rect(canvas, i).contains(pos))
            .map(|i| i.id)
    }

    /// Adds a point to the stroke being drawn.
    pub(super) fn extend_stroke(&mut self, pos: Pos2) {
        if let Drag::Draw { points, .. } = &mut self.drag
            && points
                .last()
                .is_none_or(|last| last.distance(pos) >= PEN_STEP)
        {
            points.push(pos);
        }
    }

    /// Turns a finished stroke (screen points) into a mark on the item, if it's big enough.
    pub(super) fn finish_stroke(&mut self, canvas: Rect, item: u64, points: Vec<Pos2>) {
        let Some(rect) = self
            .board
            .items
            .iter()
            .find(|i| i.id == item)
            .map(|i| self.item_screen_rect(canvas, i))
        else {
            return;
        };
        let extent = points
            .iter()
            .fold(Rect::NOTHING, |r, p| r.union(Rect::from_min_max(*p, *p)));
        if extent.width().max(extent.height()) < 4.0 {
            return; // A click, not a drawing.
        }
        let local = |p: &Pos2| {
            let q = (*p - rect.min) / rect.size();
            [q.x, q.y]
        };
        let points: Vec<[f32; 2]> = match self.draw_tool {
            MarkShape::Pen => points.iter().map(local).collect(),
            _ => vec![local(&points[0]), local(points.last().unwrap())],
        };
        self.checkpoint();
        if let Some(target) = self.board.items.iter_mut().find(|i| i.id == item) {
            // Stored on the picture itself, so it follows later rotation, flips and crops.
            let points = points
                .into_iter()
                .map(|q| target.local_to_mark(q))
                .collect();
            target.marks.push(Mark {
                shape: self.draw_tool,
                points,
                color: self.draw_color,
            });
        }
        self.touch();
    }

    /// Removes the drawings from the selected images (or every image, if none is selected).
    fn clear_marks(&mut self) {
        let targets = self.targets();
        if !self
            .board
            .items
            .iter()
            .any(|i| targets.contains(&i.id) && !i.marks.is_empty())
        {
            return;
        }
        self.checkpoint();
        for item in &mut self.board.items {
            if targets.contains(&item.id) {
                item.marks.clear();
            }
        }
        self.toast("Drawings cleared".into());
        self.touch();
    }

    /// The draw-mode toolbar: tools, colors, clear, show/hide and done.
    pub(super) fn draw_toolbar(&mut self, ctx: &egui::Context) {
        if !self.draw_mode {
            return;
        }
        let mut done = false;
        let mut clear = false;
        egui::Area::new(egui::Id::unique("coreboard-draw"))
            .anchor(Align2::CENTER_TOP, vec2(0.0, 10.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for shape in MarkShape::ALL {
                            ui.selectable_value(&mut self.draw_tool, shape, shape.label());
                        }
                        ui.separator();
                        for (i, [r, g, b]) in MARK_COLORS.iter().enumerate() {
                            let (rect, response) =
                                ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
                            let selected = self.draw_color as usize == i;
                            ui.painter().circle_filled(
                                rect.center(),
                                8.0,
                                Color32::from_rgb(*r, *g, *b),
                            );
                            ui.painter().circle_stroke(
                                rect.center(),
                                9.5,
                                Stroke::new(
                                    if selected { 2.0 } else { 1.0 },
                                    if selected {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_gray(90)
                                    },
                                ),
                            );
                            if response.clicked() {
                                self.draw_color = i as u8;
                            }
                        }
                        ui.separator();
                        clear |= ui
                            .button("Clear")
                            .on_hover_text("Remove the drawings on the selected images")
                            .clicked();
                        let label = if self.show_marks { "Hide" } else { "Show" };
                        if ui
                            .button(label)
                            .on_hover_text("Show or hide all drawings (Shift+D)")
                            .clicked()
                        {
                            self.show_marks = !self.show_marks;
                        }
                        done |= ui.button("Done").on_hover_text("Esc or D").clicked();
                    });
                });
            });
        if clear {
            self.clear_marks();
        }
        if done {
            self.toggle_draw_mode();
        }
    }
}

/// Shapes for one of `item`'s marks drawn over `rect`.
pub(super) fn item_mark_shapes(
    rect: Rect,
    item: &Item,
    mark: &Mark,
    zoom: f32,
    opacity: f32,
) -> Vec<egui::Shape> {
    mark_shapes(rect, &item.local_mark(mark), zoom, opacity)
}

/// Shapes for a mark (points local to `rect`): a dark halo under the colored stroke, so it reads
/// on light and dark pictures alike.
fn mark_shapes(rect: Rect, mark: &Mark, zoom: f32, opacity: f32) -> Vec<egui::Shape> {
    let width = (MARK_WIDTH * zoom).clamp(1.5, 24.0);
    let [r, g, b] = MARK_COLORS[mark.color as usize % MARK_COLORS.len()];
    let color = Color32::from_rgb(r, g, b).gamma_multiply(opacity);
    let halo = Color32::from_black_alpha(110).gamma_multiply(opacity);
    let lines: Vec<Vec<Pos2>> = mark
        .polylines(rect.width(), rect.height(), width)
        .into_iter()
        .map(|line| {
            line.into_iter()
                .map(|[x, y]| rect.min + vec2(x, y))
                .collect()
        })
        .collect();
    let mut shapes = Vec::new();
    for (w, c) in [(width + 2.5, halo), (width, color)] {
        for line in &lines {
            shapes.push(egui::Shape::line(line.clone(), Stroke::new(w, c)));
            // Round the ends and joins a little.
            for p in [line.first(), line.last()].into_iter().flatten() {
                shapes.push(egui::Shape::circle_filled(*p, w / 2.0, c));
            }
        }
    }
    shapes
}

/// The stroke being drawn right now, as it would look once finished.
pub(super) fn preview_mark(
    rect: Rect,
    tool: MarkShape,
    color: u8,
    points: &[Pos2],
    zoom: f32,
) -> Vec<egui::Shape> {
    if points.len() < 2 && tool != MarkShape::Pen {
        return Vec::new();
    }
    let local = |p: &Pos2| {
        let q = (*p - rect.min) / rect.size();
        [q.x, q.y]
    };
    let points = match tool {
        MarkShape::Pen => points.iter().map(local).collect(),
        _ => vec![local(&points[0]), local(points.last().unwrap())],
    };
    mark_shapes(
        rect,
        &Mark {
            shape: tool,
            points,
            color,
        },
        zoom,
        1.0,
    )
}
