//! PDF import: renders a PDF's pages into pictures (pure Rust, nothing to install) so an art
//! book, model sheet or magazine lands on the board page by page.

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::{PixmapSettings, RenderCache, RenderSettings};

/// Pages are rendered so their longest side is about this many pixels: sharp enough to study,
/// small enough that a book doesn't fill the disk.
const PAGE_SIDE: f32 = 1800.0;
/// At most this many pages are imported from one PDF.
pub const MAX_PAGES: usize = 40;

/// Whether `bytes` look like a PDF file.
pub fn is_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF")
}

/// The rendered pages (as JPEG files) and how many pages the PDF has in all.
pub fn render_pages(bytes: Vec<u8>) -> Result<(Vec<Vec<u8>>, usize), String> {
    let pdf = Pdf::new(bytes).map_err(|err| match err {
        hayro::hayro_syntax::LoadPdfError::Decryption(_) => {
            crate::i18n::tr("it's password-protected").to_string()
        }
        _ => crate::i18n::tr("it couldn't be read").to_string(),
    })?;
    let pages = pdf.pages();
    let total = pages.len();
    if total == 0 {
        return Err(crate::i18n::tr("it has no pages").into());
    }
    let cache = RenderCache::new();
    let settings = InterpreterSettings::default();
    let mut out = Vec::new();
    for page in pages.iter().take(MAX_PAGES) {
        let (w, h) = page.render_dimensions();
        let scale = (PAGE_SIDE / w.max(h).max(1.0)).min(4.0);
        let pixmap = hayro::render(
            page,
            &cache,
            &settings,
            &RenderSettings::default(),
            &PixmapSettings {
                x_scale: scale,
                y_scale: scale,
                bg_color: hayro::vello_cpu::color::palette::css::WHITE,
            },
        );
        let png = pixmap.into_png().map_err(|e| e.to_string())?;
        let rgb = image::load_from_memory(&png)
            .map_err(|e| e.to_string())?
            .to_rgb8();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode_image(&rgb)
            .map_err(|e| e.to_string())?;
        out.push(jpeg);
    }
    Ok((out, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-page PDF with a black square, written by hand.
    fn tiny_pdf() -> Vec<u8> {
        let content = "0 0 0 rg 20 20 60 60 re f";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>".to_string(),
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    #[test]
    fn renders_pages_to_pictures() {
        let bytes = tiny_pdf();
        assert!(is_pdf(&bytes));
        let (pages, total) = render_pages(bytes).unwrap();
        assert_eq!((pages.len(), total), (1, 1));
        let page = image::load_from_memory(&pages[0]).unwrap().to_rgb8();
        // A tiny 100-point page is enlarged at most 4×.
        assert_eq!(page.width(), 400);
        // White paper around a black square in the middle.
        assert!(page.get_pixel(10, 10)[0] > 240);
        assert!(page.get_pixel(200, 200)[0] < 15);
    }

    #[test]
    fn refuses_what_isnt_a_pdf() {
        assert!(!is_pdf(b"\x89PNG"));
        assert!(render_pages(b"%PDF-1.4 nonsense".to_vec()).is_err());
    }
}
