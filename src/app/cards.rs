//! File and link cards: anything that isn't a picture can still live on the board as a small
//! card. Double-click opens it the way the computer would (an Illustrator file in Illustrator,
//! a shortcut launches its app, a link opens in the browser).

use super::*;
use board::{CARD_SIZE, CARD_SUB_FONT, CARD_TITLE_FONT, CardFace};

/// The "add a link" box opened with Ctrl+click on an empty spot.
pub(super) struct LinkPrompt {
    /// Where the card goes, in world space.
    world: Pos2,
    /// Where the box is drawn, in screen space.
    screen: Pos2,
    text: String,
    focus: bool,
}

/// (board generation, card id, the page's title)
pub(super) type Titled = (u64, u64, Option<String>);

impl CoreBoardApp {
    /// Puts a card for each of `kinds` (files or links) at `at` (screen), slightly fanned out,
    /// and selects them.
    pub(super) fn add_cards(&mut self, canvas: Rect, at: Pos2, kinds: Vec<Kind>) -> Vec<u64> {
        if kinds.is_empty() {
            return Vec::new();
        }
        self.checkpoint();
        self.selection.clear();
        let zoom = self.board.view.zoom;
        // Readable at any zoom: never smaller than its normal size on screen.
        let scale = (1.0 / zoom).max(1.0);
        let size = vec2(CARD_SIZE[0], CARD_SIZE[1]) * scale;
        let mut cursor = self.to_world(canvas, at);
        let mut ids = Vec::new();
        for kind in kinds {
            let id = self.board.next_id;
            self.board.next_id += 1;
            self.board.items.push(Item {
                id,
                kind,
                pos: [cursor.x - size.x / 2.0, cursor.y - size.y / 2.0],
                size: [size.x, size.y],
                ..Item::default()
            });
            self.selection.insert(id);
            ids.push(id);
            cursor += vec2(0.0, size.y + 12.0 * scale);
        }
        self.touch();
        ids
    }

    /// File cards for `paths` (files that aren't pictures, folders, shortcuts).
    pub(super) fn add_file_cards(
        &mut self,
        canvas: Rect,
        at: Pos2,
        paths: Vec<std::path::PathBuf>,
    ) {
        let kinds = paths
            .into_iter()
            .map(|p| {
                let folder = p.is_dir();
                let p = p.canonicalize().unwrap_or(p);
                // Windows' canonical form starts with \\?\, which some apps don't like.
                let path = p.to_string_lossy().trim_start_matches(r"\\?\").to_string();
                Kind::File { path, folder }
            })
            .collect();
        self.add_cards(canvas, at, kinds);
    }

    /// A link card for `url` at `at` (screen); the page's title is filled in when it arrives.
    pub(super) fn add_link_card(
        &mut self,
        canvas: Rect,
        at: Pos2,
        url: String,
        title: Option<String>,
    ) {
        let known = title.is_some();
        let ids = self.add_cards(
            canvas,
            at,
            vec![Kind::Link {
                url: url.clone(),
                title: title.unwrap_or_default(),
            }],
        );
        if !known && let Some(&id) = ids.first() {
            let tx = self.title_tx.clone();
            let generation = self.generation;
            std::thread::spawn(move || {
                let _ = tx.send((generation, id, crate::web::page_title(&url)));
            });
        }
    }

    pub(super) fn receive_titles(&mut self) {
        while let Ok((generation, id, title)) = self.title_rx.try_recv() {
            let Some(title) = title.filter(|_| generation == self.generation) else {
                continue;
            };
            if let Some(Kind::Link { title: t, .. }) = self
                .board
                .items
                .iter_mut()
                .find(|i| i.id == id)
                .map(|i| &mut i.kind)
                && t.is_empty()
            {
                *t = title;
                self.touch();
            }
        }
    }

    /// Opens a card's file with its usual app, or its link in the browser.
    pub(super) fn open_card(&mut self, ctx: &egui::Context, id: u64) {
        let Some(item) = self.board.items.iter().find(|i| i.id == id) else {
            return;
        };
        match &item.kind {
            Kind::Link { url, .. } => {
                ctx.open_url(egui::OpenUrl::new_tab(url.clone()));
            }
            Kind::File { path, .. } => {
                let path = path.clone();
                if !std::path::Path::new(&path).exists() {
                    self.toast(format!("Can't find {path}: it was moved or deleted"));
                    return;
                }
                if let Err(err) = opener::open(&path) {
                    self.toast(format!("Couldn't open it: {err}"));
                } else {
                    self.toast("Opening…".into());
                }
            }
            _ => {}
        }
    }

    /// Ctrl+click on an empty spot: ask for a link to put there.
    pub(super) fn open_link_prompt(&mut self, canvas: Rect, at: Pos2) {
        self.link_prompt = Some(LinkPrompt {
            world: self.to_world(canvas, at),
            screen: at,
            text: String::new(),
            focus: true,
        });
    }

    pub(super) fn link_prompt_window(&mut self, ctx: &egui::Context, canvas: Rect) {
        let Some(prompt) = &mut self.link_prompt else {
            return;
        };
        let mut add = false;
        let mut cancel = false;
        egui::Area::new(egui::Id::unique("coreboard-link-prompt"))
            .order(egui::Order::Foreground)
            .fixed_pos(prompt.screen)
            .constrain_to(canvas)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new("Add a link").strong());
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut prompt.text)
                            .hint_text("Paste or type a web address")
                            .desired_width(300.0),
                    );
                    if std::mem::take(&mut prompt.focus) {
                        field.request_focus();
                    }
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    ui.horizontal(|ui| {
                        add = ui.button("Add").clicked() || enter;
                        cancel = ui.button("Cancel").clicked()
                            || ui.input(|i| i.key_pressed(Key::Escape));
                    });
                    ui.label(
                        egui::RichText::new("Double-click the card to open the page.")
                            .small()
                            .weak(),
                    );
                });
            });
        if cancel {
            self.link_prompt = None;
        } else if add {
            let prompt = self.link_prompt.take().expect("open");
            let text = prompt.text.trim().to_string();
            let at = self.to_screen(canvas, prompt.world);
            let path = std::path::PathBuf::from(text.trim_matches('"'));
            if !text.is_empty() && path.exists() {
                // A file or folder path works too.
                self.add_file_cards(canvas, at, vec![path]);
            } else if let Some(url) = crate::web::normalize_link(&text) {
                self.add_link_card(canvas, at, url, None);
            } else if !text.is_empty() {
                self.toast("That doesn't look like a web address".into());
                self.link_prompt = Some(LinkPrompt {
                    focus: true,
                    ..prompt
                });
            }
        }
    }
}

/// Draws a file or link card into `rect`.
pub(super) fn draw_card(painter: &egui::Painter, rect: Rect, face: &CardFace, opacity: f32) {
    // Everything scales with the card, so it looks the same at any zoom or size.
    let s = rect.height() / CARD_SIZE[1];
    let radius = CornerRadius::same((8.0 * s).clamp(1.0, 14.0) as u8);
    painter.rect_filled(
        rect.translate(vec2(0.0, 2.0 * s.min(2.0))),
        radius,
        Color32::from_black_alpha((70.0 * opacity) as u8),
    );
    painter.rect_filled(
        rect,
        radius,
        Color32::from_rgb(38, 40, 45).gamma_multiply(opacity),
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, Color32::from_gray(66).gamma_multiply(opacity)),
        StrokeKind::Inside,
    );
    if s * CARD_SUB_FONT < 4.0 {
        return;
    }
    // The badge: a colored tile with the file type, like a file icon.
    let pad = 12.0 * s;
    let tile = Rect::from_min_size(
        rect.min + vec2(pad, pad),
        vec2(rect.height() - 2.0 * pad, rect.height() - 2.0 * pad) * vec2(0.86, 1.0),
    );
    let color = rgb(face.color).gamma_multiply(opacity);
    painter.rect_filled(
        tile,
        CornerRadius::same((6.0 * s).clamp(1.0, 10.0) as u8),
        color,
    );
    let badge_font = (tile.width() / (face.badge.chars().count().max(2) as f32 * 0.68))
        .min(15.0 * s)
        .max(1.0);
    painter.text(
        tile.center(),
        Align2::CENTER_CENTER,
        &face.badge,
        FontId::proportional(badge_font),
        Color32::WHITE.gamma_multiply(opacity),
    );
    // Title and the line under it, cut to fit.
    let left = tile.max.x + pad;
    let width = (rect.max.x - pad - left).max(1.0);
    let fit = |text: &str, font: f32, color: Color32| {
        let mut job = egui::text::LayoutJob::simple_singleline(
            text.to_string(),
            FontId::proportional(font),
            color,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(width);
        painter.layout_job(job)
    };
    let title = fit(
        &face.title,
        CARD_TITLE_FONT * s,
        Color32::from_gray(238).gamma_multiply(opacity),
    );
    let sub = fit(
        &face.subtitle,
        CARD_SUB_FONT * s,
        Color32::from_gray(150).gamma_multiply(opacity),
    );
    let total = title.size().y + 3.0 * s + sub.size().y;
    let top = rect.center().y - total / 2.0;
    let sub_y = top + title.size().y + 3.0 * s;
    painter.galley(pos2(left, top), title, Color32::WHITE);
    painter.galley(pos2(left, sub_y), sub, Color32::WHITE);
}
