//! Image placeholders: empty, labeled slots that an image fills when dropped or pasted on them.

use super::*;
use crate::board::PLACEHOLDER_FONT;

/// Size of a new slot, in world units (4:3).
const SLOT_SIZE: [f32; 2] = [320.0, 240.0];

impl CoreBoardApp {
    /// Adds an empty slot centered at `at` and starts editing its label.
    pub(super) fn add_placeholder(&mut self, canvas: Rect, at: Pos2) {
        let center = self.to_world(canvas, at);
        self.checkpoint();
        let id = self.board.next_id;
        self.board.next_id += 1;
        self.board.items.push(Item {
            id,
            pos: [center.x - SLOT_SIZE[0] / 2.0, center.y - SLOT_SIZE[1] / 2.0],
            size: SLOT_SIZE,
            kind: Kind::Placeholder {
                label: String::new(),
            },
            ..Item::default()
        });
        self.selection = HashSet::from([id]);
        self.begin_edit(id, true);
    }

    /// The empty slot under `pos`, if any (locked slots don't take images).
    pub(super) fn slot_at(&self, canvas: Rect, pos: Pos2) -> Option<u64> {
        let id = self.hit(canvas, pos)?;
        self.board
            .items
            .iter()
            .any(|i| i.id == id && matches!(i.kind, Kind::Placeholder { .. }) && !i.locked)
            .then_some(id)
    }

    /// Turns slot `slot` into the image `bytes` (copied into the board), center-cropped to the
    /// slot's shape. The slot's label becomes a tag. Returns false if the bytes aren't an image.
    pub(super) fn fill_slot_with_bytes(
        &mut self,
        slot: u64,
        bytes: &[u8],
        path: Option<&Path>,
    ) -> bool {
        let (Some(ext), Ok((w, h))) = (board::image_ext(bytes, path), image_dimensions(bytes))
        else {
            return false;
        };
        let Ok(file) = self.store.import(bytes, &ext) else {
            return false;
        };
        let Some(index) = self.board.items.iter().position(|i| i.id == slot) else {
            return false;
        };
        self.checkpoint();
        let item = &mut self.board.items[index];
        let label = slot_label(item);
        let target = item.size;
        let mut image = Item {
            id: item.id,
            file,
            pos: item.pos,
            size: [w as f32, h.max(1) as f32],
            tags: item.tags.clone(),
            ..Item::default()
        };
        image.cover(target);
        tag_with(&mut image, label.as_deref());
        *item = image;
        self.selection = HashSet::from([slot]);
        self.toast(match label {
            Some(l) => trf("Filled “{}”", &[&l]),
            None => tr("Slot filled").into(),
        });
        self.touch();
        true
    }

    /// Moves board image `image` into slot `slot`, taking the slot's place and shape.
    pub(super) fn fill_slot_with_item(&mut self, slot: u64, image: u64) {
        let Some(slot_index) = self.board.items.iter().position(|i| i.id == slot) else {
            return;
        };
        let Some(image_index) = self.board.items.iter().position(|i| i.id == image) else {
            return;
        };
        let slot_item = self.board.items[slot_index].clone();
        let label = slot_label(&slot_item);
        let mut filled = self.board.items[image_index].clone();
        filled.pos = slot_item.pos;
        filled.cover(slot_item.size);
        tag_with(&mut filled, label.as_deref());
        for tag in &slot_item.tags {
            if !filled.tags.contains(tag) {
                filled.tags.push(tag.clone());
            }
        }
        // The image takes over the slot's spot in the stacking order.
        self.board.items[slot_index] = filled;
        self.board.items.remove(image_index);
        self.selection = HashSet::from([image]);
        self.toast(match label {
            Some(l) => trf("Filled “{}”", &[&l]),
            None => tr("Slot filled").into(),
        });
        self.touch();
    }

    /// Whether `slot` should light up as a drop target: a file is being dragged in over it, or
    /// a single board image is being dragged onto it.
    pub(super) fn slot_drop_target(&self, ctx: &egui::Context, canvas: Rect) -> Option<u64> {
        let pos = ctx.input(|i| i.pointer.hover_pos())?;
        let dragging_file = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let dragging_image = matches!(&self.drag, Drag::Move { moved: true, .. })
            && self.single_selected_image().is_some();
        if !dragging_file && !dragging_image {
            return None;
        }
        self.board
            .items
            .iter()
            .rev()
            .filter(|i| matches!(i.kind, Kind::Placeholder { .. }) && !i.locked)
            .filter(|i| !self.selection.contains(&i.id))
            .find(|i| self.item_screen_rect(canvas, i).contains(pos))
            .map(|i| i.id)
    }

    pub(super) fn single_selected_image(&self) -> Option<u64> {
        match self.selection.iter().collect::<Vec<_>>().as_slice() {
            [id] => self
                .board
                .items
                .iter()
                // A locked or pinned image stays where it is: it can't go into a slot.
                .find(|i| i.id == **id && i.is_image() && i.movable())
                .map(|i| i.id),
            _ => None,
        }
    }
}

/// The slot's label, if it has a real one.
fn slot_label(item: &Item) -> Option<String> {
    match &item.kind {
        Kind::Placeholder { label } if !label.trim().is_empty() => Some(label.trim().to_string()),
        _ => None,
    }
}

/// Keeps a slot's label on the image that fills it, as a tag.
fn tag_with(item: &mut Item, label: Option<&str>) {
    if let Some(tag) = label.and_then(board::normalize_tag)
        && !item.tags.contains(&tag)
    {
        item.tags.push(tag);
    }
}

/// An empty slot: dashed outline, a picture icon and its label.
pub(super) fn draw_placeholder(
    painter: &egui::Painter,
    rect: Rect,
    label: &str,
    zoom: f32,
    opacity: f32,
    highlight: bool,
    editing: bool,
) {
    let ink = if highlight {
        ACCENT
    } else {
        Color32::from_gray(140)
    }
    .gamma_multiply(opacity);
    let fill = if highlight {
        ACCENT.gamma_multiply(0.14 * opacity)
    } else {
        Color32::from_white_alpha(9).gamma_multiply(opacity)
    };
    painter.rect_filled(rect, CornerRadius::same(6), fill);
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    let stroke = Stroke::new(if highlight { 2.5 } else { 1.5 }, ink);
    painter.extend(egui::Shape::dashed_line(&corners, stroke, 8.0, 6.0));

    // A small "picture" icon: a frame with a sun and two hills.
    let side = (rect.width().min(rect.height()) * 0.3).clamp(10.0, 90.0);
    let label_font = PLACEHOLDER_FONT * zoom;
    let show_label = label_font >= 7.0;
    let icon_center = rect.center() - vec2(0.0, if show_label { label_font * 0.9 } else { 0.0 });
    let icon = Rect::from_center_size(icon_center, vec2(side, side * 0.78));
    let thin = Stroke::new((side / 28.0).max(1.2), ink);
    painter.rect_stroke(icon, CornerRadius::same(3), thin, StrokeKind::Inside);
    painter.circle_filled(
        icon.min + vec2(icon.width() * 0.72, icon.height() * 0.3),
        side * 0.08,
        ink,
    );
    let base = icon.max.y - thin.width;
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(icon.min.x + thin.width, base),
            pos2(
                icon.min.x + icon.width() * 0.35,
                icon.min.y + icon.height() * 0.45,
            ),
            pos2(icon.min.x + icon.width() * 0.62, base),
        ],
        ink,
        Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(icon.min.x + icon.width() * 0.45, base),
            pos2(
                icon.min.x + icon.width() * 0.68,
                icon.min.y + icon.height() * 0.6,
            ),
            pos2(icon.max.x - thin.width, base),
        ],
        ink,
        Stroke::NONE,
    ));

    if !show_label || editing {
        return;
    }
    let (text, color) = if label.trim().is_empty() {
        (tr("Drop an image here"), ink.gamma_multiply(0.8))
    } else {
        (label, Color32::from_gray(225).gamma_multiply(opacity))
    };
    let galley = painter.layout(
        text.to_string(),
        FontId::proportional(label_font),
        color,
        (rect.width() - 16.0).max(1.0),
    );
    let pos = pos2(
        rect.center().x - galley.size().x / 2.0,
        icon.max.y + label_font * 0.5,
    );
    painter
        .with_clip_rect(rect.intersect(painter.clip_rect()))
        .galley(pos, galley, color);
}

/// Where the label editor sits on a slot, matching [`draw_placeholder`]'s layout.
pub(super) fn placeholder_label_rect(rect: Rect, zoom: f32) -> Rect {
    let side = (rect.width().min(rect.height()) * 0.3).clamp(10.0, 90.0);
    let font = PLACEHOLDER_FONT * zoom;
    let icon_bottom = rect.center().y - font * 0.9 + side * 0.39;
    Rect::from_min_size(
        pos2(rect.min.x + 8.0, icon_bottom + font * 0.5),
        vec2(rect.width() - 16.0, font * 1.3),
    )
}
