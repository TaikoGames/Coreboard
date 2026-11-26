//! Contact sheets: pictures in a neat grid on printable pages, each with its caption, as a PDF
//! for printing or sending to a client.
//!
//! Pictures go in as they're shown on the board (cropped, turned, mirrored, black and white),
//! saved as JPEG at a size that prints sharp. Text uses the PDF's built-in Helvetica, so nothing
//! needs embedding; letters it can't show become "?".

use std::path::Path;

use image::DynamicImage;
use pdf_writer::TextStr as PdfText;
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str};
use serde::{Deserialize, Serialize};

use crate::board::Item;
use crate::filters::Filter as ViewFilter;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Paper {
    /// Letter in Canada and the US, A4 elsewhere.
    #[default]
    Auto,
    Letter,
    A4,
}

impl Paper {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Letter, Self::A4];

    /// Width and height in points (1/72 inch).
    fn size(self) -> (f32, f32) {
        match self.resolved() {
            Self::A4 => (595.28, 841.89),
            _ => (612.0, 792.0),
        }
    }

    fn resolved(self) -> Self {
        match self {
            Self::Auto => {
                let locale = sys_locale::get_locale()
                    .unwrap_or_default()
                    .to_ascii_uppercase();
                if ["-US", "-CA", "_US", "_CA", "-MX", "-PH"]
                    .iter()
                    .any(|r| locale.ends_with(r))
                {
                    Self::Letter
                } else {
                    Self::A4
                }
            }
            other => other,
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Auto => crate::i18n::trf(
                "Automatic ({})",
                &[&match self.resolved() {
                    Self::A4 => "A4",
                    _ => "Letter",
                }],
            ),
            Self::Letter => crate::i18n::tr("Letter").to_string(),
            Self::A4 => "A4".to_string(),
        }
    }
}

/// Contact sheet settings, kept with the board.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub columns: u32,
    pub paper: Paper,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            columns: 3,
            paper: Paper::Auto,
        }
    }
}

const MARGIN: f32 = 36.0;
const GAP: f32 = 14.0;
const HEADER: f32 = 30.0;
const CAPTION_SIZE: f32 = 8.5;
const CAPTION_LINES: usize = 2;
const TITLE_SIZE: f32 = 13.0;
const SMALL_SIZE: f32 = 8.0;
/// Pictures are saved at about this many pixels per point of their printed size (2 = 144 dpi,
/// sharp on paper without making the file huge).
const PIXELS_PER_POINT: f32 = 2.5;

/// Text as Helvetica can show it (Windows-1252), anything else as "?".
fn win_ansi(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c {
            ' '..='~' => c as u8,
            '\u{a0}'..='\u{ff}' => c as u32 as u8,
            '€' => 0x80,
            '‚' => 0x82,
            '„' => 0x84,
            '…' => 0x85,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            _ => b'?',
        })
        .collect()
}

/// Helvetica's average letter width, as a share of the font size: close enough to wrap
/// captions without measuring each letter.
const AVERAGE_WIDTH: f32 = 0.52;

/// Splits `text` into at most `max_lines` lines of about `width` points, ending in "…" when it
/// doesn't all fit.
fn wrap(text: &str, size: f32, width: f32, max_lines: usize) -> Vec<String> {
    let per_line = ((width / (size * AVERAGE_WIDTH)) as usize).max(4);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let words: Vec<&str> = text.split_whitespace().collect();
    for word in &words {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if candidate.chars().count() <= per_line {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        current = word.to_string();
        if lines.len() == max_lines {
            break;
        }
    }
    if !current.is_empty() && lines.len() < max_lines {
        lines.push(current);
    }
    let shown: usize = lines.iter().map(|l| l.split_whitespace().count()).sum();
    if shown < words.len() || lines.iter().any(|l| l.chars().count() > per_line) {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            let keep = per_line.saturating_sub(1);
            if last.chars().count() > keep {
                *last = last.chars().take(keep).collect();
            }
            *last = format!("{}…", last.trim_end());
        }
    }
    lines
}

/// Where things go on a page.
struct Layout {
    page: (f32, f32),
    columns: usize,
    rows: usize,
    cell_w: f32,
    picture_h: f32,
    row_h: f32,
}

impl Layout {
    fn new(options: Options) -> Self {
        let page = options.paper.size();
        let columns = options.columns.clamp(1, 6) as usize;
        let usable_w = page.0 - 2.0 * MARGIN;
        let cell_w = (usable_w - GAP * (columns - 1) as f32) / columns as f32;
        let picture_h = cell_w * 0.85;
        let caption_h = CAPTION_LINES as f32 * CAPTION_SIZE * 1.25 + 4.0;
        let row_h = picture_h + caption_h + GAP;
        let usable_h = page.1 - 2.0 * MARGIN - HEADER;
        let rows = ((usable_h + GAP) / row_h).floor().max(1.0) as usize;
        Self {
            page,
            columns,
            rows,
            cell_w,
            picture_h,
            row_h,
        }
    }

    fn per_page(&self) -> usize {
        self.columns * self.rows
    }

    /// The picture box of the `n`th cell on a page, from the bottom-left (PDF coordinates).
    fn cell(&self, n: usize) -> (f32, f32) {
        let (col, row) = (n % self.columns, n / self.columns);
        let x = MARGIN + col as f32 * (self.cell_w + GAP);
        let top = self.page.1 - MARGIN - HEADER - row as f32 * self.row_h;
        (x, top - self.picture_h)
    }
}

/// A picture as JPEG bytes, sized to print sharp in a box `box_w` points wide.
fn jpeg(img: DynamicImage, box_w: f32, box_h: f32) -> Result<(Vec<u8>, u32, u32), String> {
    let max_w = (box_w * PIXELS_PER_POINT).round() as u32;
    let max_h = (box_h * PIXELS_PER_POINT).round() as u32;
    let img = if img.width() > max_w || img.height() > max_h {
        img.resize(max_w, max_h, image::imageops::FilterType::CatmullRom)
    } else {
        img
    };
    let rgb = img.to_rgb8();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 88)
        .encode_image(&rgb)
        .map_err(|e| e.to_string())?;
    Ok((bytes, rgb.width(), rgb.height()))
}

/// Writes a contact sheet of `pictures` (images, in order) to `out`. Returns the page count.
pub fn save(
    images_dir: &Path,
    pictures: &[Item],
    title: &str,
    date: &str,
    options: Options,
    out: &Path,
) -> Result<usize, String> {
    if pictures.is_empty() {
        return Err(crate::i18n::tr("no pictures to put on it").into());
    }
    let layout = Layout::new(options);
    let pages = pictures.len().div_ceil(layout.per_page());

    let mut pdf = Pdf::new();
    let mut next = 1;
    let mut new_ref = || {
        let r = Ref::new(next);
        next += 1;
        r
    };
    let catalog = new_ref();
    let tree = new_ref();
    let font = new_ref();
    let info = new_ref();
    let font_name = Name(b"F1");
    pdf.type1_font(font)
        .base_font(Name(b"Helvetica"))
        .encoding_predefined(Name(b"WinAnsiEncoding"));
    pdf.document_info(info)
        .title(PdfText(title))
        .creator(PdfText("Core Board"));

    let mut page_refs = Vec::new();
    for (page_no, chunk) in pictures.chunks(layout.per_page()).enumerate() {
        let page_ref = new_ref();
        let content_ref = new_ref();
        page_refs.push(page_ref);
        let mut content = Content::new();
        let mut images: Vec<(Vec<u8>, Ref)> = Vec::new();
        // Header: the board's name on the left, date and page number on the right.
        content.set_fill_gray(0.1);
        content.begin_text();
        content.set_font(font_name, TITLE_SIZE);
        content.next_line(MARGIN, layout.page.1 - MARGIN - TITLE_SIZE);
        content.show(Str(&win_ansi(title)));
        content.end_text();
        let right = crate::i18n::trf("{}  ·  page {} of {}", &[&date, &(page_no + 1), &pages]);
        let right = win_ansi(&right);
        let right_w = right.len() as f32 * SMALL_SIZE * AVERAGE_WIDTH;
        content.set_fill_gray(0.45);
        content.begin_text();
        content.set_font(font_name, SMALL_SIZE);
        content.next_line(
            layout.page.0 - MARGIN - right_w,
            layout.page.1 - MARGIN - TITLE_SIZE,
        );
        content.show(Str(&right));
        content.end_text();
        // A thin rule under the header.
        content.set_stroke_gray(0.8);
        content.set_line_width(0.5);
        content.move_to(MARGIN, layout.page.1 - MARGIN - HEADER + 8.0);
        content.line_to(
            layout.page.0 - MARGIN,
            layout.page.1 - MARGIN - HEADER + 8.0,
        );
        content.stroke();

        for (n, item) in chunk.iter().enumerate() {
            let (x, y) = layout.cell(n);
            let shown = crate::export::shown_image(images_dir, item, ViewFilter::for_item(item))?;
            let (bytes, w, h) = jpeg(shown, layout.cell_w, layout.picture_h)?;
            // Fit inside the box, centered at the bottom so captions line up.
            let scale = (layout.cell_w / w as f32).min(layout.picture_h / h as f32);
            let (dw, dh) = (w as f32 * scale, h as f32 * scale);
            let dx = x + (layout.cell_w - dw) / 2.0;
            let dy = y + (layout.picture_h - dh) / 2.0;
            let image_ref = new_ref();
            let name = format!("Im{}", n + 1).into_bytes();
            let mut image = pdf.image_xobject(image_ref, &bytes);
            image.filter(Filter::DctDecode);
            image.width(w as i32);
            image.height(h as i32);
            image.color_space().device_rgb();
            image.bits_per_component(8);
            image.finish();
            content.save_state();
            content.transform([dw, 0.0, 0.0, dh, dx, dy]);
            content.x_object(Name(&name));
            images.push((name, image_ref));
            content.restore_state();
            // The caption under the picture's box.
            let caption = item.caption.trim();
            if !caption.is_empty() {
                content.set_fill_gray(0.2);
                for (line_no, line) in wrap(caption, CAPTION_SIZE, layout.cell_w, CAPTION_LINES)
                    .iter()
                    .enumerate()
                {
                    content.begin_text();
                    content.set_font(font_name, CAPTION_SIZE);
                    content.next_line(x, y - CAPTION_SIZE * 1.25 * (line_no + 1) as f32 - 1.0);
                    content.show(Str(&win_ansi(line)));
                    content.end_text();
                }
            }
        }
        pdf.stream(content_ref, &content.finish());
        let mut page = pdf.page(page_ref);
        page.media_box(Rect::new(0.0, 0.0, layout.page.0, layout.page.1));
        page.parent(tree);
        page.contents(content_ref);
        let mut resources = page.resources();
        resources.fonts().pair(font_name, font);
        let mut objects = resources.x_objects();
        for (name, r) in &images {
            objects.pair(Name(name), *r);
        }
        objects.finish();
        resources.finish();
        page.finish();
    }
    pdf.pages(tree)
        .kids(page_refs.iter().copied())
        .count(page_refs.len() as i32);
    pdf.catalog(catalog).pages(tree);

    let tmp = out.with_extension("pdf.tmp");
    std::fs::write(&tmp, pdf.finish()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, out).map_err(|e| e.to_string())?;
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_captions_to_two_lines() {
        assert_eq!(wrap("Front view", 10.0, 200.0, 2), vec!["Front view"]);
        let long = "Light from the left, warm bounce under the chin, cool rim on the shoulder";
        let lines = wrap(long, 10.0, 100.0, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with('…'), "{lines:?}");
    }

    #[test]
    fn keeps_accents_helvetica_can_show() {
        assert_eq!(win_ansi("Àlex – café"), b"\xc0lex \x96 caf\xe9".to_vec());
        assert_eq!(win_ansi("猫"), b"?".to_vec());
    }

    #[test]
    fn fits_more_rows_on_taller_pages() {
        let letter = Layout::new(Options {
            columns: 3,
            paper: Paper::Letter,
        });
        assert_eq!(letter.columns, 3);
        assert!(letter.rows >= 3);
        let four = Layout::new(Options {
            columns: 4,
            paper: Paper::A4,
        });
        assert!(four.per_page() > letter.per_page());
    }

    #[test]
    fn writes_a_pdf() {
        let dir = std::env::temp_dir().join(format!("coreboard-contact-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut pictures = Vec::new();
        for n in 0..11u8 {
            let file = format!("p{n}.png");
            image::RgbImage::from_pixel(40, 30, image::Rgb([n * 20, 100, 200]))
                .save(dir.join(&file))
                .unwrap();
            pictures.push(Item {
                id: n as u64,
                file,
                size: [40.0, 30.0],
                caption: format!("Picture {n} – café"),
                ..Item::default()
            });
        }
        let out = dir.join("sheet.pdf");
        let options = Options {
            columns: 3,
            paper: Paper::Letter,
        };
        let pages = save(&dir, &pictures, "Faces", "2026-11-26", options, &out).unwrap();
        let per_page = Layout::new(options).per_page();
        assert_eq!(pages, 11usize.div_ceil(per_page));
        let bytes = std::fs::read(&out).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(text.matches("/Subtype /Image").count(), 11);
        assert!(save(&dir, &[], "Faces", "x", options, &out).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
