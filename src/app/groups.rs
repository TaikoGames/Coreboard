//! Groups: items that are picked and moved together, without a frame around them.
//! Ctrl+Shift+G groups the selection, or ungroups it if it's already a group.

use super::*;

impl CoreBoardApp {
    /// `id` and everything grouped with it.
    pub(super) fn group_of(&self, id: u64) -> Vec<u64> {
        let group = self
            .board
            .items
            .iter()
            .find(|i| i.id == id)
            .and_then(|i| i.group);
        match group {
            Some(g) => self
                .board
                .items
                .iter()
                .filter(|i| i.group == Some(g))
                .map(|i| i.id)
                .collect(),
            None => vec![id],
        }
    }

    /// Adds the rest of any group that has a member selected.
    pub(super) fn select_whole_groups(&mut self) {
        let groups: HashSet<u64> = self
            .board
            .items
            .iter()
            .filter(|i| self.selection.contains(&i.id))
            .filter_map(|i| i.group)
            .collect();
        if groups.is_empty() {
            return;
        }
        for item in &self.board.items {
            if item.group.is_some_and(|g| groups.contains(&g)) {
                self.selection.insert(item.id);
            }
        }
    }

    pub(super) fn toggle_group(&mut self) {
        let selected: Vec<&Item> = self
            .board
            .items
            .iter()
            .filter(|i| self.selection.contains(&i.id))
            .collect();
        if selected.iter().any(|i| i.group.is_some()) {
            self.checkpoint();
            for item in &mut self.board.items {
                if self.selection.contains(&item.id) {
                    item.group = None;
                }
            }
            self.toast(tr("Ungrouped").into());
        } else if selected.len() >= 2 {
            let n = selected.len();
            self.checkpoint();
            let group = self.board.next_id;
            self.board.next_id += 1;
            for item in &mut self.board.items {
                if self.selection.contains(&item.id) {
                    item.group = Some(group);
                }
            }
            self.toast(trf("Grouped {} items", &[&n]));
        } else {
            self.toast(tr("Select 2 or more items to group").into());
            return;
        }
        self.touch();
    }

    /// A dashed outline around each group that's selected.
    pub(super) fn paint_group_outlines(&self, painter: &egui::Painter, canvas: Rect) {
        let mut bounds: HashMap<u64, Rect> = HashMap::new();
        for item in &self.board.items {
            if let Some(g) = item.group
                && self.selection.contains(&item.id)
            {
                let rect = self.item_screen_rect(canvas, item);
                bounds
                    .entry(g)
                    .and_modify(|r| *r = r.union(rect))
                    .or_insert(rect);
            }
        }
        for rect in bounds.values() {
            let r = rect.expand(8.0);
            let corners = [
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
                r.left_top(),
            ];
            painter.extend(egui::Shape::dashed_line(
                &corners,
                Stroke::new(1.5, ACCENT),
                7.0,
                5.0,
            ));
        }
    }
}
