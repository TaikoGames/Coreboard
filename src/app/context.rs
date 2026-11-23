//! The right-click menu: what can be done with the thing under the pointer, each with its
//! shortcut, so the menu teaches the keys as it's used. Right-dragging still pans.

use super::*;

/// What was right-clicked: where, and the item there, if any.
#[derive(Clone, Copy)]
pub(super) struct ContextTarget {
    at: Pos2,
    item: Option<u64>,
}

/// A menu entry with its shortcut on the right. True when clicked.
fn entry(ui: &mut egui::Ui, label: &str, shortcut: &str) -> bool {
    ui.add(egui::Button::new(label).shortcut_text(shortcut))
        .clicked()
}

impl CoreBoardApp {
    /// Remembers what a right-click landed on, and shows the menu for it.
    pub(super) fn context_menu(
        &mut self,
        ctx: &egui::Context,
        canvas: Rect,
        response: &egui::Response,
    ) {
        if response.secondary_clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            let item = self.hit(canvas, at);
            // Acting on an item acts on it (and its group), like a click would select it.
            if let Some(id) = item
                && !self.selection.contains(&id)
            {
                self.selection = self.group_of(id).into_iter().collect();
            }
            self.context = Some(ContextTarget { at, item });
        }
        let Some(target) = self.context else {
            return;
        };
        response.context_menu(|ui| {
            ui.set_min_width(230.0);
            let kind = target
                .item
                .and_then(|id| self.board.items.iter().find(|i| i.id == id))
                .map(|i| i.kind.clone());
            let done = match kind {
                Some(Kind::Image) => self.image_menu(ui, canvas, target),
                Some(kind) => self.item_menu(ui, ctx, canvas, target, &kind),
                None => self.board_menu(ui, ctx, canvas, target),
            };
            if done {
                ui.close();
            }
        });
    }

    fn image_menu(&mut self, ui: &mut egui::Ui, canvas: Rect, target: ContextTarget) -> bool {
        let id = target.item.unwrap_or_default();
        let label = if self.compare_pair().is_some() {
            tr("Compare side by side")
        } else {
            tr("Focus view")
        };
        if entry(ui, label, "Enter") {
            self.open_focus(canvas, Some(target.at));
            return true;
        }
        if entry(ui, tr("Copy"), "Ctrl+C") {
            self.copy_selection();
            return true;
        }
        if entry(ui, tr("Duplicate"), "Ctrl+D") {
            self.duplicate_selection();
            return true;
        }
        ui.separator();
        if entry(ui, tr("Crop"), "X") {
            self.toggle_crop_mode();
            return true;
        }
        if entry(ui, tr("Rotate"), "R") {
            self.rotate_selection(1);
            return true;
        }
        if entry(ui, tr("Mirror"), "M") {
            self.toggle_selection(|i| &mut i.flip_x);
            return true;
        }
        if entry(ui, tr("Black & white"), "B") {
            self.toggle_selection(|i| &mut i.grayscale);
            return true;
        }
        if entry(ui, tr("Study view"), "V") {
            self.cycle_analysis(true);
            return true;
        }
        if entry(ui, tr("Guide"), "C") {
            self.cycle_guide();
            return true;
        }
        if entry(ui, tr("Extract palette"), "P") {
            self.extract_palettes(6);
            return true;
        }
        ui.separator();
        if entry(ui, tr("Find similar"), "Q") {
            self.open_finder(FindMode::Values);
            return true;
        }
        if entry(ui, tr("Search the web"), "Q") {
            self.open_finder(FindMode::Web);
            return true;
        }
        if entry(ui, tr("Caption"), tr("Double-click")) {
            self.begin_edit(id, false);
            return true;
        }
        ui.separator();
        self.common_entries(ui, canvas)
    }

    /// Notes, frames, slots, palettes, drawings and cards.
    fn item_menu(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        canvas: Rect,
        target: ContextTarget,
        kind: &Kind,
    ) -> bool {
        let id = target.item.unwrap_or_default();
        match kind {
            Kind::File { .. } | Kind::Link { .. } if entry(ui, tr("Open"), tr("Double-click")) => {
                self.open_card(ctx, id);
                return true;
            }
            Kind::Note { .. } | Kind::Frame { .. } | Kind::Placeholder { .. }
                if entry(ui, tr("Edit text"), tr("Double-click")) =>
            {
                self.begin_edit(id, false);
                return true;
            }
            _ => {}
        }
        if entry(ui, tr("Copy"), "Ctrl+C") {
            self.copy_selection();
            return true;
        }
        if entry(ui, tr("Duplicate"), "Ctrl+D") {
            self.duplicate_selection();
            return true;
        }
        ui.separator();
        self.common_entries(ui, canvas)
    }

    /// Lock, pin, group and delete, for anything selected.
    fn common_entries(&mut self, ui: &mut egui::Ui, canvas: Rect) -> bool {
        let locked = self
            .board
            .items
            .iter()
            .any(|i| self.selection.contains(&i.id) && i.locked);
        if entry(ui, if locked { tr("Unlock") } else { tr("Lock") }, "L") {
            self.toggle_lock();
            return true;
        }
        let pinned = self
            .board
            .items
            .iter()
            .any(|i| self.selection.contains(&i.id) && i.pinned.is_some());
        if entry(
            ui,
            if pinned {
                tr("Unpin from the screen")
            } else {
                tr("Pin to the screen")
            },
            "Shift+L",
        ) {
            self.toggle_pin(canvas);
            return true;
        }
        let grouped = self
            .board
            .items
            .iter()
            .any(|i| self.selection.contains(&i.id) && i.group.is_some());
        if (grouped || self.selection.len() >= 2)
            && entry(
                ui,
                if grouped { tr("Ungroup") } else { tr("Group") },
                "Ctrl+Shift+G",
            )
        {
            self.toggle_group();
            return true;
        }
        if entry(ui, tr("Delete"), "Del") {
            self.delete_selection();
            return true;
        }
        false
    }

    fn board_menu(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        canvas: Rect,
        target: ContextTarget,
    ) -> bool {
        let at = target.at;
        if entry(ui, tr("Paste"), "Ctrl+V") {
            self.paste(canvas, at, None);
            return true;
        }
        if entry(ui, tr("Add a note"), "N") {
            self.add_note(canvas, at);
            return true;
        }
        if entry(ui, tr("Add a link"), "Ctrl+click") {
            self.open_link_prompt(canvas, at);
            return true;
        }
        if entry(ui, tr("Add an image slot"), "E") {
            self.add_placeholder(canvas, at);
            return true;
        }
        ui.separator();
        if entry(ui, tr("Fit everything"), "F") {
            self.fit(canvas);
            return true;
        }
        if entry(ui, tr("Zoom 100%"), "1") {
            let before = self.board.view.clone();
            self.zoom_around(canvas, at, 1.0);
            let to = std::mem::replace(&mut self.board.view, before);
            self.fly_view(to);
            return true;
        }
        if entry(ui, tr("Surprise me"), "S") {
            self.surprise(ctx);
            return true;
        }
        ui.separator();
        if entry(ui, tr("Boards…"), "Ctrl+O") {
            self.open_boards_window();
            return true;
        }
        if entry(ui, tr("Save this board as a file…"), "Ctrl+Shift+S") {
            self.save_board_file();
            return true;
        }
        if !self.board.items.is_empty() && entry(ui, tr("Export PNG…"), "Ctrl+E") {
            self.show_export = true;
            return true;
        }
        if entry(ui, tr("Help"), "H") {
            self.show_help = true;
            return true;
        }
        false
    }
}
