//! Organizing the board: lock and pin, tags and search, snapping, align and distribute.

use super::*;

/// How close (in screen points) an edge must come to another before it snaps.
const SNAP_PX: f32 = 7.0;
const SNAP_COLOR: Color32 = Color32::from_rgb(255, 80, 200);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Align {
    Left,
    CenterX,
    Right,
    Top,
    CenterY,
    Bottom,
}

fn world_rect(i: &Item) -> Rect {
    Rect::from_min_size(pos2(i.pos[0], i.pos[1]), vec2(i.size[0], i.size[1]))
}

impl CoreBoardApp {
    // ---------------------------------------------------------------- lock and pin

    /// Locks the selection, or unlocks it if all of it is locked already.
    pub(super) fn toggle_lock(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        let all = self
            .board
            .items
            .iter()
            .filter(|i| self.selection.contains(&i.id))
            .all(|i| i.locked);
        for item in &mut self.board.items {
            if self.selection.contains(&item.id) {
                item.locked = !all;
            }
        }
        self.toast(if all { tr("Unlocked") } else { tr("Locked") }.into());
        self.touch();
    }

    /// Pins the selection to the screen where it is now, or unpins it if all of it is pinned.
    pub(super) fn toggle_pin(&mut self, canvas: Rect) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        let all = self
            .board
            .items
            .iter()
            .filter(|i| self.selection.contains(&i.id))
            .all(|i| i.pinned.is_some());
        let rects: HashMap<u64, Rect> = self
            .board
            .items
            .iter()
            .filter(|i| self.selection.contains(&i.id))
            .map(|i| (i.id, self.item_screen_rect(canvas, i)))
            .collect();
        for item in &mut self.board.items {
            if let Some(r) = rects.get(&item.id) {
                // World geometry is kept in sync while pinned, so unpinning leaves it in place.
                item.pinned = (!all).then(|| {
                    [
                        r.min.x - canvas.min.x,
                        r.min.y - canvas.min.y,
                        r.width(),
                        r.height(),
                    ]
                });
            }
        }
        self.toast(
            if all {
                tr("Unpinned")
            } else {
                tr("Pinned to the screen")
            }
            .into(),
        );
        self.touch();
    }

    /// Keeps pinned items' world position and size matching where they sit on screen, so
    /// everything that works in world space (export, fit, frames) sees them where they look.
    pub(super) fn sync_pinned(&mut self) {
        let zoom = self.board.view.zoom;
        let view = self.board.view.clone();
        for item in &mut self.board.items {
            if let Some([x, y, w, h]) = item.pinned {
                item.pos = [(x - view.offset[0]) / zoom, (y - view.offset[1]) / zoom];
                item.size = [w / zoom, h / zoom];
            }
        }
    }

    /// Small lock / pin badges in an item's top-right corner.
    pub(super) fn draw_badges(&self, painter: &egui::Painter, rect: Rect, item: &Item) {
        let badges: Vec<&str> = [(item.locked, "🔒"), (item.pinned.is_some(), "📌")]
            .into_iter()
            .filter_map(|(on, b)| on.then_some(b))
            .collect();
        if badges.is_empty() || rect.width().min(rect.height()) < 36.0 {
            return;
        }
        for (i, badge) in badges.iter().enumerate() {
            let center = rect.right_top() + vec2(-13.0 - 24.0 * i as f32, 13.0);
            painter.circle_filled(center, 10.0, Color32::from_black_alpha(170));
            painter.text(
                center,
                Align2::CENTER_CENTER,
                *badge,
                FontId::proportional(11.0),
                Color32::WHITE,
            );
        }
    }

    // ---------------------------------------------------------------- tags

    pub(super) fn open_tags_window(&mut self) {
        if self.selection.is_empty() {
            self.toast(tr("Select something to tag").into());
            return;
        }
        self.finish_edit();
        self.show_tags = true;
        self.tags_focus_pending = true;
        self.new_tag.clear();
    }

    /// Every tag on the board with how many items carry it, most used first.
    pub(super) fn board_tags(&self) -> Vec<(String, usize)> {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for item in &self.board.items {
            for tag in &item.tags {
                *counts.entry(tag.as_str()).or_default() += 1;
            }
        }
        let mut tags: Vec<(String, usize)> = counts
            .into_iter()
            .map(|(t, n)| (t.to_string(), n))
            .collect();
        tags.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        tags
    }

    fn add_tag_to_selection(&mut self, tag: &str) {
        let Some(tag) = board::normalize_tag(tag) else {
            return;
        };
        self.checkpoint();
        for item in &mut self.board.items {
            if self.selection.contains(&item.id) && !item.tags.contains(&tag) {
                item.tags.push(tag.clone());
            }
        }
        self.touch();
    }

    fn remove_tag_from_selection(&mut self, tag: &str) {
        self.checkpoint();
        for item in &mut self.board.items {
            if self.selection.contains(&item.id) {
                item.tags.retain(|t| t != tag);
            }
        }
        self.touch();
    }

    pub(super) fn tags_window(&mut self, ctx: &egui::Context) {
        if !self.show_tags {
            return;
        }
        if self.selection.is_empty() {
            self.show_tags = false;
            return;
        }
        let selected: Vec<&Item> = self
            .board
            .items
            .iter()
            .filter(|i| self.selection.contains(&i.id))
            .collect();
        let n = selected.len();
        // Tags on the selection, with how many of the selected items have each.
        let mut on: Vec<(String, usize)> = Vec::new();
        for item in &selected {
            for tag in &item.tags {
                match on.iter_mut().find(|(t, _)| t == tag) {
                    Some((_, c)) => *c += 1,
                    None => on.push((tag.clone(), 1)),
                }
            }
        }
        let suggestions: Vec<String> = self
            .board_tags()
            .into_iter()
            .map(|(t, _)| t)
            .filter(|t| !on.iter().any(|(o, c)| o == t && *c == n))
            .take(16)
            .collect();

        let mut add: Option<String> = None;
        let mut remove: Option<String> = None;
        let mut open = true;
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        egui::Window::new(trf("Tags · {} selected", &[&n]))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 60.0))
            .show(ctx, |ui| {
                if on.is_empty() {
                    ui.label(egui::RichText::new(tr("No tags yet")).weak());
                }
                ui.horizontal_wrapped(|ui| {
                    for (tag, count) in &on {
                        let label = if *count == n {
                            format!("{tag}  ✕")
                        } else {
                            format!("{tag} ({count}/{n})  ✕")
                        };
                        if ui
                            .button(label)
                            .on_hover_text(tr("Remove from the selection"))
                            .clicked()
                        {
                            remove = Some(tag.clone());
                        }
                    }
                });
                ui.add_space(4.0);
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.new_tag)
                        .hint_text(tr("Add a tag and press Enter"))
                        .desired_width(260.0),
                );
                if self.tags_focus_pending {
                    field.request_focus();
                    self.tags_focus_pending = false;
                }
                if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    add = Some(std::mem::take(&mut self.new_tag));
                    // Enter drops focus; take it back next frame so tags can be typed in a row.
                    self.tags_focus_pending = true;
                    close = false;
                }
                if !suggestions.is_empty() {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(tr("On this board")).small().weak());
                    ui.horizontal_wrapped(|ui| {
                        for tag in &suggestions {
                            if ui.small_button(format!("+ {tag}")).clicked() {
                                add = Some(tag.clone());
                            }
                        }
                    });
                }
            });
        if let Some(tag) = add {
            self.add_tag_to_selection(&tag);
        }
        if let Some(tag) = remove {
            self.remove_tag_from_selection(&tag);
        }
        if !open || close {
            self.show_tags = false;
        }
    }

    /// Tag chips under an item, shown while it's hovered or selected.
    pub(super) fn draw_tags(&self, painter: &egui::Painter, rect: Rect, tags: &[String]) {
        let mut x = rect.min.x;
        let y = rect.max.y + 6.0;
        for tag in tags {
            let galley = painter.layout_no_wrap(
                tag.clone(),
                FontId::proportional(11.5),
                Color32::from_gray(225),
            );
            let chip = Rect::from_min_size(pos2(x, y), galley.size() + vec2(12.0, 4.0));
            if chip.max.x > rect.max.x.max(rect.min.x + 160.0) {
                break;
            }
            painter.rect_filled(chip, CornerRadius::same(8), Color32::from_rgb(58, 62, 72));
            painter.galley(chip.min + vec2(6.0, 2.0), galley, Color32::WHITE);
            x = chip.max.x + 4.0;
        }
    }

    // ---------------------------------------------------------------- search

    pub(super) fn open_search(&mut self) {
        self.finish_edit();
        self.search.get_or_insert_with(String::new);
        self.search_focus_pending = true;
    }

    /// How strongly to draw an item: matches of the current search at full strength, the rest
    /// faded.
    pub(super) fn search_dim(&self, item: &Item) -> f32 {
        match &self.search {
            Some(q) if !q.trim().is_empty() && !item.matches(q) => 0.18,
            _ => 1.0,
        }
    }

    pub(super) fn search_bar(&mut self, ctx: &egui::Context, canvas: Rect) {
        let Some(mut query) = self.search.take() else {
            return;
        };
        let matches: Vec<u64> = if query.trim().is_empty() {
            Vec::new()
        } else {
            self.board
                .items
                .iter()
                .filter(|i| i.matches(&query))
                .map(|i| i.id)
                .collect()
        };
        let tags = self.board_tags();
        let mut keep = true;
        let mut select = false;
        egui::Area::new(egui::Id::unique("coreboard-search"))
            .anchor(Align2::CENTER_TOP, vec2(0.0, 10.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut query)
                                .hint_text(tr("Search tags, notes and frame titles"))
                                .desired_width(300.0),
                        );
                        if std::mem::take(&mut self.search_focus_pending) {
                            field.request_focus();
                        }
                        let found = if query.trim().is_empty() {
                            String::new()
                        } else {
                            trf("{} found", &[&matches.len()])
                        };
                        ui.label(egui::RichText::new(found).weak());
                        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            select = true;
                        }
                        if ui.input(|i| i.key_pressed(Key::Escape)) {
                            keep = false;
                        }
                        select |= ui
                            .add_enabled(!matches.is_empty(), egui::Button::new(tr("Select")))
                            .on_hover_text(tr("Select the matches and zoom to them (Enter)"))
                            .clicked();
                        keep &= !ui
                            .button("✕")
                            .on_hover_text(tr("Close search (Esc)"))
                            .clicked();
                    });
                    if !tags.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.set_max_width(460.0);
                            for (tag, count) in tags.iter().take(20) {
                                if ui.small_button(format!("{tag} {count}")).clicked() {
                                    query = tag.clone();
                                }
                            }
                        });
                    }
                });
            });
        if select && !matches.is_empty() {
            self.selection = matches.into_iter().collect();
            self.fit(canvas);
            keep = false;
        }
        if keep {
            self.search = Some(query);
        }
    }

    // ---------------------------------------------------------------- snapping

    /// Adjusts a move of the items in `start` by `total` (world units) so the moved group's
    /// edges or center snap to nearby items' edges and centers. Records the guide lines.
    pub(super) fn snap_move(
        &mut self,
        canvas: Rect,
        start: &HashMap<u64, [f32; 2]>,
        total: Vec2,
        enabled: bool,
    ) -> Vec2 {
        self.snap_lines.clear();
        let moving: Vec<Rect> = self
            .board
            .items
            .iter()
            .filter_map(|i| {
                let [x, y] = start.get(&i.id)?;
                Some(Rect::from_min_size(
                    pos2(*x, *y),
                    vec2(i.size[0], i.size[1]),
                ))
            })
            .collect();
        let Some(group) = moving.into_iter().reduce(|a, b| a.union(b)) else {
            return total;
        };
        if !enabled {
            return total;
        }
        let view = Rect::from_min_max(
            self.to_world(canvas, canvas.min),
            self.to_world(canvas, canvas.max),
        );
        let targets: Vec<Rect> = self
            .board
            .items
            .iter()
            .filter(|i| !start.contains_key(&i.id) && i.pinned.is_none())
            .map(world_rect)
            .filter(|r| r.intersects(view))
            .collect();
        let moved = group.translate(total);
        let threshold = SNAP_PX / self.board.view.zoom;

        // Best correction on one axis: (distance, correction, target rect).
        let best = |mine: [f32; 3], theirs: &dyn Fn(&Rect) -> [f32; 3]| {
            let mut found: Option<(f32, f32, Rect)> = None;
            for t in &targets {
                for m in mine {
                    for o in theirs(t) {
                        let d = o - m;
                        if d.abs() <= threshold && found.is_none_or(|(fd, _, _)| d.abs() < fd) {
                            found = Some((d.abs(), d, *t));
                        }
                    }
                }
            }
            found
        };
        let xs = |r: &Rect| [r.min.x, r.center().x, r.max.x];
        let ys = |r: &Rect| [r.min.y, r.center().y, r.max.y];
        let snap_x = best(xs(&moved), &xs);
        let snap_y = best(ys(&moved), &ys);
        let snapped = total
            + vec2(
                snap_x.map_or(0.0, |(_, d, _)| d),
                snap_y.map_or(0.0, |(_, d, _)| d),
            );
        let g = group.translate(snapped);

        // A guide along each edge that lines up, spanning both rects.
        if let Some((_, _, t)) = snap_x {
            for x in xs(&g) {
                if xs(&t).iter().any(|tx| (tx - x).abs() < 1e-3) {
                    let (top, bottom) = (g.min.y.min(t.min.y), g.max.y.max(t.max.y));
                    self.snap_lines.push([pos2(x, top), pos2(x, bottom)]);
                }
            }
        }
        if let Some((_, _, t)) = snap_y {
            for y in ys(&g) {
                if ys(&t).iter().any(|ty| (ty - y).abs() < 1e-3) {
                    let (left, right) = (g.min.x.min(t.min.x), g.max.x.max(t.max.x));
                    self.snap_lines.push([pos2(left, y), pos2(right, y)]);
                }
            }
        }
        snapped
    }

    pub(super) fn paint_snap_lines(&self, painter: &egui::Painter, canvas: Rect) {
        for [a, b] in &self.snap_lines {
            painter.line_segment(
                [self.to_screen(canvas, *a), self.to_screen(canvas, *b)],
                Stroke::new(1.0, SNAP_COLOR),
            );
        }
    }

    // ---------------------------------------------------------------- align and distribute

    /// Selected items that layout actions may move.
    fn movable_selection(&self) -> Vec<usize> {
        (0..self.board.items.len())
            .filter(|&i| {
                let item = &self.board.items[i];
                self.selection.contains(&item.id) && item.movable()
            })
            .collect()
    }

    pub(super) fn align_selection(&mut self, align: Align) {
        let picked = self.movable_selection();
        if picked.len() < 2 {
            return;
        }
        let bounds = picked
            .iter()
            .map(|&i| world_rect(&self.board.items[i]))
            .reduce(|a, b| a.union(b))
            .unwrap();
        self.checkpoint();
        for i in picked {
            let item = &mut self.board.items[i];
            let [w, h] = item.size;
            match align {
                Align::Left => item.pos[0] = bounds.min.x,
                Align::CenterX => item.pos[0] = bounds.center().x - w / 2.0,
                Align::Right => item.pos[0] = bounds.max.x - w,
                Align::Top => item.pos[1] = bounds.min.y,
                Align::CenterY => item.pos[1] = bounds.center().y - h / 2.0,
                Align::Bottom => item.pos[1] = bounds.max.y - h,
            }
        }
        self.touch();
    }

    /// Spaces the selection evenly along an axis, keeping the outermost items where they are.
    pub(super) fn distribute_selection(&mut self, horizontal: bool) {
        let mut picked = self.movable_selection();
        if picked.len() < 3 {
            self.toast(tr("Select 3 or more items to space evenly").into());
            return;
        }
        let axis = usize::from(!horizontal);
        let items = &self.board.items;
        picked.sort_by(|&a, &b| items[a].pos[axis].total_cmp(&items[b].pos[axis]));
        let start = items[picked[0]].pos[axis];
        let end = picked
            .iter()
            .map(|&i| items[i].pos[axis] + items[i].size[axis])
            .fold(f32::MIN, f32::max);
        let filled: f32 = picked.iter().map(|&i| items[i].size[axis]).sum();
        let gap = (end - start - filled) / (picked.len() - 1) as f32;
        self.checkpoint();
        let mut at = start;
        for i in picked {
            let item = &mut self.board.items[i];
            item.pos[axis] = at;
            at += item.size[axis] + gap;
        }
        self.touch();
    }

    /// A small bar of align and distribute buttons while 2+ movable items are selected.
    pub(super) fn align_toolbar(&mut self, ctx: &egui::Context, canvas: Rect) {
        if self.movable_selection().len() < 2
            || !matches!(self.drag, Drag::None)
            || self.editing.is_some()
            || self.crop_mode.is_some()
        {
            return;
        }
        let mut align = None;
        let mut distribute = None;
        egui::Area::new(egui::Id::unique("coreboard-align"))
            .fixed_pos(pos2(canvas.center().x - 250.0, canvas.max.y - 64.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(tr("Align")).weak());
                        for (a, label, tip) in [
                            (Align::Left, "Left", "Alt+←"),
                            (Align::CenterX, "Center", "Alt+C"),
                            (Align::Right, "Right", "Alt+→"),
                            (Align::Top, "Top", "Alt+↑"),
                            (Align::CenterY, "Middle", "Alt+M"),
                            (Align::Bottom, "Bottom", "Alt+↓"),
                        ] {
                            if ui.small_button(tr(label)).on_hover_text(tip).clicked() {
                                align = Some(a);
                            }
                        }
                        ui.separator();
                        ui.label(egui::RichText::new(tr("Space")).weak());
                        if ui
                            .small_button(tr("Across"))
                            .on_hover_text(tr("Alt+H"))
                            .clicked()
                        {
                            distribute = Some(true);
                        }
                        if ui
                            .small_button(tr("Down"))
                            .on_hover_text(tr("Alt+V"))
                            .clicked()
                        {
                            distribute = Some(false);
                        }
                    });
                });
            });
        if let Some(a) = align {
            self.align_selection(a);
        }
        if let Some(h) = distribute {
            self.distribute_selection(h);
        }
    }
}

impl CoreBoardApp {
    /// Moves the selection by one screen point with an arrow key (ten with Shift). A burst of
    /// nudges is one undo step.
    pub(super) fn nudge(&mut self, key: Key, big: bool) {
        let step = if big { 10.0 } else { 1.0 };
        let screen = match key {
            Key::ArrowLeft => vec2(-step, 0.0),
            Key::ArrowRight => vec2(step, 0.0),
            Key::ArrowUp => vec2(0.0, -step),
            Key::ArrowDown => vec2(0.0, step),
            _ => return,
        };
        if !self
            .board
            .items
            .iter()
            .any(|i| self.selection.contains(&i.id) && !i.locked)
        {
            return;
        }
        if self
            .last_nudge
            .is_none_or(|t| t.elapsed() > Duration::from_millis(900))
        {
            self.checkpoint();
        }
        self.last_nudge = Some(Instant::now());
        let world = screen / self.board.view.zoom;
        for item in &mut self.board.items {
            if !self.selection.contains(&item.id) || item.locked {
                continue;
            }
            // Pinned items live in screen space.
            match &mut item.pinned {
                Some(pin) => {
                    pin[0] += screen.x;
                    pin[1] += screen.y;
                }
                None => {
                    item.pos[0] += world.x;
                    item.pos[1] += world.y;
                }
            }
        }
        self.touch();
    }
}
