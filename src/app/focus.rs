//! Focus view: one image big and centered on a dark backdrop, to study it without the rest of
//! the board around. Arrow keys step through every image on the board in reading order.
//!
//! Opening grows the image out of its spot on the board, closing shrinks it back. The board
//! itself doesn't change: study keys still act on the image (it's the selection while focused).

use super::*;

/// How long the image takes to grow out of (or shrink back into) the board.
const OPEN: Duration = Duration::from_millis(260);
/// How long a newly stepped-to image takes to fade in.
const STEP: Duration = Duration::from_millis(180);
/// Space kept around the image, and below it for the caption and counter.
const MARGIN: f32 = 40.0;
const FOOTER: f32 = 64.0;
const MAX_ZOOM: f32 = 16.0;

pub(super) struct Focus {
    id: u64,
    /// 1 = the whole image fits; more zooms into details.
    zoom: f32,
    /// Screen offset of the image from its fitted place.
    pan: Vec2,
    /// When it opened (or started closing), for the grow and shrink.
    opened: Instant,
    closing: bool,
    /// When the shown image last changed, for the fade-in.
    switched: Instant,
}

/// Images in reading order: rows top to bottom, each row left to right. An image joins a row
/// when its center sits within the first image of that row, so slightly uneven rows still read
/// as one.
fn reading_order(items: &[Item]) -> Vec<u64> {
    let mut images: Vec<&Item> = items.iter().filter(|i| i.is_image()).collect();
    let center_y = |i: &Item| i.pos[1] + i.size[1] / 2.0;
    images.sort_by(|a, b| center_y(a).total_cmp(&center_y(b)));
    let mut rows: Vec<Vec<&Item>> = Vec::new();
    for item in images {
        match rows.last_mut() {
            Some(row) if center_y(item) <= row[0].pos[1] + row[0].size[1] => row.push(item),
            _ => rows.push(vec![item]),
        }
    }
    rows.into_iter()
        .flat_map(|mut row| {
            row.sort_by(|a, b| a.pos[0].total_cmp(&b.pos[0]));
            row.into_iter().map(|i| i.id)
        })
        .collect()
}

/// A random number below `n` (n > 0), good enough for picking a picture.
fn random_below(n: usize) -> usize {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(Instant::now().elapsed().as_nanos());
    (hasher.finish() % n as u64) as usize
}

/// The biggest rect of the image's shape that fits in `area`, centered.
fn fit_into(area: Rect, size: [f32; 2]) -> Rect {
    let [w, h] = [size[0].max(1.0), size[1].max(1.0)];
    let scale = (area.width() / w).min(area.height() / h).max(0.01);
    Rect::from_center_size(area.center(), vec2(w, h) * scale)
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::from_min_max(a.min.lerp(b.min, t), a.max.lerp(b.max, t))
}

impl CoreBoardApp {
    pub(super) fn focus_open(&self) -> bool {
        self.focus.is_some()
    }

    /// Opens the selected image, or the one under the cursor, in focus view.
    pub(super) fn open_focus(&mut self, canvas: Rect, hover: Option<Pos2>) {
        let image = |id: u64| self.board.items.iter().any(|i| i.id == id && i.is_image());
        let under = hover
            .and_then(|p| self.hit(canvas, p))
            .filter(|id| image(*id));
        let selected = self.first_selected().map(|i| i.id);
        let Some(id) = under.or(selected).or_else(|| {
            // Nothing picked: start from the first image.
            reading_order(&self.board.items).first().copied()
        }) else {
            self.toast(tr("No images to focus on").into());
            return;
        };
        self.open_focus_on(id);
    }

    /// Opens focus view on image `id` of this board.
    fn open_focus_on(&mut self, id: u64) {
        self.radial = None;
        self.search = None;
        self.selection = HashSet::from([id]);
        let now = Instant::now();
        // With animations off, it starts already open.
        let opened = if self.animations {
            now
        } else {
            now.checked_sub(OPEN).unwrap_or(now)
        };
        self.focus = Some(Focus {
            id,
            zoom: 1.0,
            pan: Vec2::ZERO,
            opened,
            closing: false,
            switched: opened,
        });
    }

    /// Surprise me: focus view on a random image from this board or any recent one. A pick from
    /// another board opens that board.
    pub(super) fn surprise(&mut self, ctx: &egui::Context) {
        let here = self
            .store
            .dir
            .canonicalize()
            .unwrap_or_else(|_| self.store.dir.clone());
        let showing = self.focus.as_ref().map(|f| f.id);
        let mut picks: Vec<(Option<std::path::PathBuf>, u64)> = self
            .board
            .items
            .iter()
            .filter(|i| i.is_image() && Some(i.id) != showing)
            .map(|i| (None, i.id))
            .collect();
        for dir in boards::recent() {
            if dir.canonicalize().unwrap_or_else(|_| dir.clone()) == here {
                continue;
            }
            let Some(board) = std::fs::read(dir.join("board.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<Board>(&b).ok())
            else {
                continue;
            };
            picks.extend(
                board
                    .items
                    .iter()
                    .filter(|i| i.is_image())
                    .map(|i| (Some(dir.clone()), i.id)),
            );
        }
        if picks.is_empty() {
            self.toast(tr("Add some images first, then press S for a surprise").into());
            return;
        }
        let (dir, id) = picks.swap_remove(random_below(picks.len()));
        if let Some(dir) = dir {
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.switch_board(ctx, dir);
            if !self.board.items.iter().any(|i| i.id == id) {
                // The switch didn't go through (the board couldn't be opened).
                return;
            }
            self.toast(trf("Surprise! From \u{201c}{}\u{201d}", &[&name]));
        }
        self.open_focus_on(id);
    }

    /// Starts shrinking back into the board (or closes at once, with animations off).
    fn close_focus(&mut self) {
        let Some(focus) = &mut self.focus else {
            return;
        };
        if !self.animations {
            self.focus = None;
            return;
        }
        if !focus.closing {
            // Shrinking starts from however far it had grown.
            let grown = (focus.opened.elapsed().as_secs_f32() / OPEN.as_secs_f32()).min(1.0);
            focus.closing = true;
            focus.opened = Instant::now()
                .checked_sub(OPEN.mul_f32(1.0 - grown))
                .unwrap_or_else(Instant::now);
            focus.zoom = 1.0;
            focus.pan = Vec2::ZERO;
        }
    }

    /// Steps to the next (or previous) image, wrapping around.
    fn step_focus(&mut self, forward: bool) {
        let order = reading_order(&self.board.items);
        let Some(focus) = &mut self.focus else {
            return;
        };
        if order.len() < 2 {
            return;
        }
        let at = order.iter().position(|id| *id == focus.id).unwrap_or(0);
        let next = if forward {
            (at + 1) % order.len()
        } else {
            (at + order.len() - 1) % order.len()
        };
        focus.id = order[next];
        focus.zoom = 1.0;
        focus.pan = Vec2::ZERO;
        focus.switched = Instant::now();
        self.selection = HashSet::from([order[next]]);
    }

    /// Where the image sits when fitted (zoom 1, no pan).
    fn focus_fit_rect(canvas: Rect, item: &Item) -> Rect {
        let area = Rect::from_min_max(
            canvas.min + vec2(MARGIN, MARGIN),
            canvas.max - vec2(MARGIN, FOOTER),
        );
        fit_into(area, item.size)
    }

    /// Keys, wheel and pointer while focus view is open. The board itself gets no input.
    pub(super) fn focus_input(
        &mut self,
        ctx: &egui::Context,
        canvas: Rect,
        response: &egui::Response,
    ) {
        let Some(focus) = &self.focus else {
            return;
        };
        if focus.closing {
            return;
        }
        let Some(item) = self.board.items.iter().find(|i| i.id == focus.id).cloned() else {
            // Deleted (or undone away) while focused.
            self.focus = None;
            return;
        };
        let fit = Self::focus_fit_rect(canvas, &item);
        let shown = Rect::from_center_size(fit.center() + focus.pan, fit.size() * focus.zoom);

        let (events, scroll, pinch, hover) = ctx.input(|i| {
            (
                i.events.clone(),
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.pointer.hover_pos(),
            )
        });
        let typing = ctx.egui_wants_keyboard_input();
        for event in events.iter().filter(|_| !typing) {
            let Event::Key {
                key,
                pressed: true,
                modifiers: m,
                ..
            } = event
            else {
                continue;
            };
            match key {
                Key::Escape | Key::Enter | Key::Space => self.close_focus(),
                Key::S if !m.command && !m.shift => self.surprise(ctx),
                Key::ArrowRight | Key::ArrowDown | Key::PageDown => self.step_focus(true),
                Key::ArrowLeft | Key::ArrowUp | Key::PageUp => self.step_focus(false),
                Key::F | Key::Home | Key::Num0 => {
                    if let Some(f) = &mut self.focus {
                        f.zoom = 1.0;
                        f.pan = Vec2::ZERO;
                    }
                }
                Key::Z if m.command && m.shift => self.redo(),
                Key::Z if m.command => self.undo(),
                Key::Y if m.command => self.redo(),
                Key::R if !m.command => self.rotate_selection(if m.shift { -1 } else { 1 }),
                Key::M if !m.command => self.toggle_selection(|i| &mut i.flip_x),
                Key::B if !m.command && m.shift => self.cycle_board_values(),
                Key::B if !m.command => self.toggle_selection(|i| &mut i.grayscale),
                Key::V if !m.command => self.cycle_analysis(!m.shift),
                Key::C if !m.command && m.shift => self.turn_spiral(),
                Key::C if !m.command => self.cycle_guide(),
                Key::D if !m.command && m.shift => {
                    self.show_marks = !self.show_marks;
                    self.toast(
                        if self.show_marks {
                            tr("Drawings shown")
                        } else {
                            tr("Drawings hidden")
                        }
                        .into(),
                    );
                }
                _ => {}
            }
        }
        let Some(focus) = &mut self.focus else {
            return;
        };
        if focus.closing {
            return;
        }

        // Wheel or pinch zooms into details, keeping the spot under the cursor in place.
        let factor = (scroll.y * 0.0025).exp() * pinch;
        if (factor - 1.0).abs() > f32::EPSILON {
            let zoom = (focus.zoom * factor).clamp(1.0, MAX_ZOOM);
            let f = zoom / focus.zoom;
            let at = hover.unwrap_or(shown.center());
            let center = at + (shown.center() - at) * f;
            focus.pan += center - shown.center();
            focus.zoom = zoom;
            if zoom <= 1.0 {
                focus.pan = Vec2::ZERO;
            }
        }
        // Dragging pans (zoomed or not); a double-click resets.
        if response.dragged() {
            focus.pan += response.drag_delta();
        }
        if response.double_clicked() {
            focus.zoom = 1.0;
            focus.pan = Vec2::ZERO;
        } else if response.clicked()
            && response
                .interact_pointer_pos()
                .is_some_and(|p| !shown.contains(p))
        {
            // A click on the backdrop goes back to the board.
            self.close_focus();
        }
        // Keep the middle of the screen on the image, so it can't be dragged away and lost.
        if let Some(f) = &mut self.focus {
            let half = fit.size() * f.zoom / 2.0;
            f.pan = vec2(
                f.pan.x.clamp(-half.x, half.x),
                f.pan.y.clamp(-half.y, half.y),
            );
        }
        if self.focus.as_ref().is_some_and(|f| f.zoom > 1.0) {
            ctx.set_cursor_icon(egui::CursorIcon::Grab);
        }
    }

    /// Draws focus view on top of the board.
    pub(super) fn paint_focus(&mut self, ctx: &egui::Context, canvas: Rect) {
        let Some(focus) = &self.focus else {
            return;
        };
        let Some(item) = self.board.items.iter().find(|i| i.id == focus.id).cloned() else {
            self.focus = None;
            return;
        };
        let now = Instant::now();
        let t = (now.duration_since(focus.opened).as_secs_f32() / OPEN.as_secs_f32()).min(1.0);
        if focus.closing && t >= 1.0 {
            self.focus = None;
            return;
        }
        // 0 = on the board, 1 = fully open.
        let open = if focus.closing {
            1.0 - motion::ease_out_cubic(t)
        } else {
            motion::ease_out_cubic(t)
        };
        let fade = (now.duration_since(focus.switched).as_secs_f32() / STEP.as_secs_f32()).min(1.0);
        let (zoom, pan, closing) = (focus.zoom, focus.pan, focus.closing);
        let animating = t < 1.0 || fade < 1.0;

        let fit = Self::focus_fit_rect(canvas, &item);
        let target = Rect::from_center_size(fit.center() + pan, fit.size() * zoom);
        let on_board = self.item_screen_rect(canvas, &item);
        let rect = lerp_rect(on_board, target, open);

        let layer =
            egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("coreboard-focus"));
        let painter = ctx.layer_painter(layer).with_clip_rect(canvas);
        painter.rect_filled(
            canvas,
            CornerRadius::ZERO,
            Color32::from_black_alpha((238.0 * open) as u8),
        );

        let filter = match self.board_values {
            1 => Filter::Gray,
            2 => Filter::Study(Analysis::Values3),
            _ => Filter::for_item(&item),
        };
        let tint = Color32::WHITE.gamma_multiply(0.25 + 0.75 * fade);
        match self.texture(ctx, &item.file, filter) {
            Texture::Ready(tex) => {
                // A soft shadow lifts it off the backdrop.
                for (grow, alpha) in [(18.0, 40.0), (8.0, 60.0)] {
                    painter.rect_filled(
                        rect.translate(vec2(0.0, 8.0)).expand(grow * open),
                        CornerRadius::same((grow * open) as u8),
                        Color32::from_black_alpha((alpha * open) as u8),
                    );
                }
                painter.add(image_shape(tex.id(), rect, &item, tint, 0.0));
            }
            Texture::Loading => {
                painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_gray(50));
                ctx.request_repaint_after(Duration::from_millis(50));
            }
            Texture::Failed(err) => {
                painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(70, 30, 30));
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    err,
                    FontId::proportional(14.0),
                    Color32::LIGHT_RED,
                );
            }
        }
        if item.guide != Guide::None {
            draw_guide(&painter, rect, item.guide, item.guide_orientation);
        }
        if self.show_marks && !item.marks.is_empty() {
            // Mark widths follow the image's on-screen scale.
            let scale = rect.width() / item.size[0].max(1.0);
            let clipped = painter.with_clip_rect(rect.intersect(canvas));
            for mark in &item.marks {
                clipped.extend(item_mark_shapes(rect, &item, mark, scale, 1.0));
            }
        }

        // Caption, counter and hints under the image, once it's open.
        if !closing && open > 0.5 {
            let alpha = ((open - 0.5) * 2.0).min(1.0);
            let order = reading_order(&self.board.items);
            let index = order.iter().position(|id| *id == item.id).unwrap_or(0);
            let footer = canvas.max.y - FOOTER + 14.0;
            let mut y = footer;
            if !item.caption.trim().is_empty() {
                let galley = painter.layout(
                    item.caption.clone(),
                    FontId::proportional(17.0),
                    Color32::from_gray(225).gamma_multiply(alpha),
                    (canvas.width() - 2.0 * MARGIN).max(1.0),
                );
                let pos = pos2(canvas.center().x - galley.size().x / 2.0, y);
                y += galley.size().y + 4.0;
                painter.galley(pos, galley, Color32::from_gray(225));
            }
            let mut info = trf("image {} of {}", &[&(index + 1), &order.len()]);
            if zoom > 1.01 {
                info += &format!("  ·  {:.0}%", zoom * 100.0);
            }
            painter.text(
                pos2(canvas.center().x, y),
                Align2::CENTER_TOP,
                info,
                FontId::proportional(13.0),
                Color32::from_gray(150).gamma_multiply(alpha),
            );
            painter.text(
                canvas.right_bottom() + vec2(-16.0, -12.0),
                Align2::RIGHT_BOTTOM,
                tr("← → step   S surprise   scroll zoom   drag pan   V C B M R study   Esc back"),
                FontId::proportional(12.0),
                Color32::from_gray(110).gamma_multiply(alpha),
            );
            if order.len() > 1 {
                // Arrows at the sides, as a hint that there's more to step through.
                for (x, arrow) in [(canvas.min.x + 18.0, "‹"), (canvas.max.x - 18.0, "›")] {
                    painter.text(
                        pos2(x, canvas.center().y),
                        Align2::CENTER_CENTER,
                        arrow,
                        FontId::proportional(40.0),
                        Color32::from_gray(90).gamma_multiply(alpha),
                    );
                }
            }
        }

        // Messages from study keys show above the backdrop.
        if let Some((msg, at)) = &self.toast
            && at.elapsed() < Duration::from_secs(3)
        {
            painter.text(
                pos2(canvas.center().x, canvas.min.y + 18.0),
                Align2::CENTER_TOP,
                msg,
                FontId::proportional(15.0),
                Color32::WHITE,
            );
        }
        if animating {
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(id: u64, x: f32, y: f32, w: f32, h: f32) -> Item {
        Item {
            id,
            pos: [x, y],
            size: [w, h],
            ..Item::default()
        }
    }

    #[test]
    fn steps_through_rows_left_to_right() {
        let items = vec![
            image(1, 500.0, 10.0, 200.0, 200.0),
            image(2, 0.0, 400.0, 200.0, 200.0),
            // Slightly lower than the first in its row, still the same row.
            image(3, 250.0, 40.0, 200.0, 150.0),
            image(4, 0.0, 0.0, 200.0, 220.0),
        ];
        assert_eq!(reading_order(&items), vec![4, 3, 1, 2]);
    }

    #[test]
    fn fits_without_stretching() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 500.0));
        let r = fit_into(area, [400.0, 400.0]);
        assert_eq!(r.size(), vec2(500.0, 500.0));
        assert_eq!(r.center(), area.center());
        let r = fit_into(area, [4000.0, 1000.0]);
        assert_eq!(r.size(), vec2(1000.0, 250.0));
    }
}
