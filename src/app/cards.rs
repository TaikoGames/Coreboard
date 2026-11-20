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

/// (board generation, card id, the page's title and preview picture)
pub(super) type Previewed = (u64, u64, crate::web::Preview);

impl CoreBoardApp {
    /// Puts a card for each of `kinds` (files or links) at `at` (screen), slightly fanned out,
    /// and selects them.
    pub(super) fn add_cards(&mut self, canvas: Rect, at: Pos2, kinds: Vec<Kind>) -> Vec<u64> {
        // Part of a drop or paste that already added pictures: one undo step, one selection.
        let joined = std::mem::take(&mut self.join_cards);
        if kinds.is_empty() {
            return Vec::new();
        }
        if !joined {
            self.checkpoint();
            self.selection.clear();
        }
        let zoom = self.board.view.zoom;
        // Readable at any zoom: never smaller than its normal size on screen.
        let scale = (1.0 / zoom).max(1.0);
        let mut cursor = self.to_world(canvas, at);
        let mut ids = Vec::new();
        for kind in kinds {
            // Videos get a 16:9 card, with room for their picture.
            let size = if matches!(&kind, Kind::Link { url, .. } if crate::web::is_video(url)) {
                vec2(VIDEO_SIZE[0], VIDEO_SIZE[1]) * scale
            } else {
                vec2(CARD_SIZE[0], CARD_SIZE[1]) * scale
            };
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
        // The title and preview picture arrive in the background (see `receive_previews`).
        self.add_cards(
            canvas,
            at,
            vec![Kind::Link {
                url,
                title: title.unwrap_or_default(),
            }],
        );
    }

    /// Asks for the title and preview picture of every link card that has no picture yet
    /// (once per card per session: new cards, and ones made before previews existed).
    pub(super) fn request_previews(&mut self) {
        let generation = self.generation;
        let wanted: Vec<(u64, String)> = self
            .board
            .items
            .iter()
            .filter(|i| i.file.is_empty() && !self.preview_asked.contains(&(generation, i.id)))
            .filter_map(|i| match &i.kind {
                Kind::Link { url, .. } => Some((i.id, url.clone())),
                _ => None,
            })
            .collect();
        for (id, url) in wanted {
            self.preview_asked.insert((generation, id));
            let tx = self.preview_tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send((generation, id, crate::web::page_preview(&url)));
            });
        }
    }

    pub(super) fn receive_previews(&mut self) {
        while let Ok((generation, id, preview)) = self.preview_rx.try_recv() {
            if generation != self.generation {
                continue;
            }
            let file = preview
                .image
                .and_then(|bytes| self.store.import(&bytes, "jpg").ok());
            let Some(item) = self.board.items.iter_mut().find(|i| i.id == id) else {
                continue;
            };
            let Kind::Link { title, .. } = &mut item.kind else {
                continue;
            };
            let mut changed = false;
            if let Some(new) = preview.title.filter(|_| title.is_empty()) {
                *title = new;
                changed = true;
            }
            if let Some(file) = file.filter(|_| item.file.is_empty()) {
                item.file = file;
                changed = true;
            }
            if changed {
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
            Kind::Link { url, title } if crate::web::is_video(url) => {
                let (url, title) = (url.clone(), title.clone());
                match crate::player::open(&url, &title) {
                    Ok(()) => self.toast(tr("Opening the video…").into()),
                    Err(_) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                }
            }
            Kind::Link { url, .. } => {
                ctx.open_url(egui::OpenUrl::new_tab(url.clone()));
            }
            Kind::File { path, .. } => {
                let path = path.clone();
                if !std::path::Path::new(&path).exists() {
                    self.toast(trf("Can't find {}: it was moved or deleted", &[&path]));
                    return;
                }
                if let Err(err) = opener::open(&path) {
                    self.toast(trf("Couldn't open it: {}", &[&err]));
                } else {
                    self.toast(tr("Opening…").into());
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
                    ui.label(egui::RichText::new(tr("Add a link")).strong());
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut prompt.text)
                            .hint_text(tr("Paste or type a web address"))
                            .desired_width(300.0),
                    );
                    if std::mem::take(&mut prompt.focus) {
                        field.request_focus();
                    }
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    ui.horizontal(|ui| {
                        add = ui.button(tr("Add")).clicked() || enter;
                        cancel = ui.button(tr("Cancel")).clicked()
                            || ui.input(|i| i.key_pressed(Key::Escape));
                    });
                    ui.label(
                        egui::RichText::new(tr("Double-click the card to open the page."))
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
                self.toast(tr("That doesn't look like a web address").into());
                self.link_prompt = Some(LinkPrompt {
                    focus: true,
                    ..prompt
                });
            }
        }
    }
}

/// Draws a file or link card into `rect`.
/// A video card's size in world units (16:9).
const VIDEO_SIZE: [f32; 2] = [320.0, 180.0];

/// A video card: its picture filling the card, a play button, and the title along the bottom.
fn draw_video_card(
    painter: &egui::Painter,
    rect: Rect,
    face: &CardFace,
    opacity: f32,
    preview: Option<(egui::TextureId, Vec2)>,
) {
    let s = rect.height() / VIDEO_SIZE[1];
    let radius = CornerRadius::same((8.0 * s).clamp(1.0, 14.0) as u8);
    painter.rect_filled(
        rect,
        radius,
        Color32::from_rgb(20, 21, 24).gamma_multiply(opacity),
    );
    if let Some((texture, size)) = preview {
        let [u0, v0, u1, v1] = cover_uv(size, rect.size());
        let mut mesh = egui::Mesh::with_texture(texture);
        mesh.add_rect_with_uv(
            rect,
            Rect::from_min_max(pos2(u0, v0), pos2(u1, v1)),
            Color32::WHITE.gamma_multiply(opacity),
        );
        painter.add(egui::Shape::mesh(mesh));
    }
    // The play button.
    let r = (rect.height() * 0.16).max(3.0);
    painter.circle_filled(
        rect.center(),
        r * 1.25,
        Color32::from_black_alpha((160.0 * opacity) as u8),
    );
    let c = rect.center() + vec2(r * 0.12, 0.0);
    painter.add(egui::Shape::convex_polygon(
        vec![
            c + vec2(-r * 0.45, -r * 0.6),
            c + vec2(r * 0.6, 0.0),
            c + vec2(-r * 0.45, r * 0.6),
        ],
        Color32::WHITE.gamma_multiply(opacity),
        Stroke::NONE,
    ));
    // The title on a dark strip along the bottom.
    let font = CARD_TITLE_FONT * s * 0.9;
    if font >= 4.0 {
        let pad = 8.0 * s;
        let mut job = egui::text::LayoutJob::simple_singleline(
            face.title.clone(),
            FontId::proportional(font),
            Color32::from_gray(240).gamma_multiply(opacity),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 2.0 * pad).max(1.0));
        let galley = painter.layout_job(job);
        let strip = Rect::from_min_max(
            pos2(rect.min.x, rect.max.y - galley.size().y - 2.0 * pad),
            rect.max,
        );
        painter.rect_filled(
            strip,
            CornerRadius {
                sw: radius.sw,
                se: radius.se,
                ..Default::default()
            },
            Color32::from_black_alpha((170.0 * opacity) as u8),
        );
        painter.galley(strip.min + vec2(pad, pad), galley, Color32::WHITE);
    }
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, Color32::from_gray(66).gamma_multiply(opacity)),
        StrokeKind::Inside,
    );
}

/// `preview` is the link's picture (texture and its size in pixels), shown in place of the badge.
/// A video link in a card at least half as tall as it's wide is drawn as a video card.
pub(super) fn draw_card(
    painter: &egui::Painter,
    rect: Rect,
    face: &CardFace,
    opacity: f32,
    preview: Option<(egui::TextureId, Vec2)>,
    video: bool,
) {
    if video && rect.height() >= rect.width() * 0.4 {
        draw_video_card(painter, rect, face, opacity, preview);
        return;
    }
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
    let pad = 12.0 * s;
    let tile = if let Some((texture, size)) = preview {
        // The page's picture, filling a wide thumbnail on the left (cropped, not squashed).
        let inset = 6.0 * s;
        let h = rect.height() - 2.0 * inset;
        let thumb = Rect::from_min_size(rect.min + vec2(inset, inset), vec2(h * 1.5, h));
        let [u0, v0, u1, v1] = cover_uv(size, thumb.size());
        let mut mesh = egui::Mesh::with_texture(texture);
        mesh.add_rect_with_uv(
            thumb,
            Rect::from_min_max(pos2(u0, v0), pos2(u1, v1)),
            Color32::WHITE.gamma_multiply(opacity),
        );
        painter.add(egui::Shape::mesh(mesh));
        thumb.translate(vec2(inset - pad, 0.0))
    } else {
        // The badge: a colored tile with the file type, like a file icon.
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
        tile
    };
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

/// The part of a picture (as UV coordinates) that fills `area` without stretching: the middle,
/// cut to `area`'s shape.
pub(crate) fn cover_uv(image: Vec2, area: Vec2) -> [f32; 4] {
    let (iw, ih) = (image.x.max(1.0), image.y.max(1.0));
    let want = area.x.max(1.0) / area.y.max(1.0);
    if iw / ih > want {
        let w = want * ih / iw;
        [(1.0 - w) / 2.0, 0.0, (1.0 + w) / 2.0, 1.0]
    } else {
        let h = iw / want / ih;
        [0.0, (1.0 - h) / 2.0, 1.0, (1.0 + h) / 2.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_are_cropped_not_squashed() {
        // A square picture in a 3:2 thumbnail keeps the middle two thirds of its height.
        let [u0, v0, u1, v1] = cover_uv(vec2(300.0, 300.0), vec2(90.0, 60.0));
        assert_eq!([u0, u1], [0.0, 1.0]);
        assert!((v1 - v0 - 2.0 / 3.0).abs() < 1e-5);
        // A very wide picture keeps its middle.
        let [u0, v0, u1, v1] = cover_uv(vec2(1200.0, 300.0), vec2(90.0, 60.0));
        assert_eq!([v0, v1], [0.0, 1.0]);
        assert!((u1 - u0 - 0.375).abs() < 1e-5 && (u0 + u1 - 1.0).abs() < 1e-5);
    }
}
