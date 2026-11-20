//! Dropping (or pasting) a PDF: its pages are rendered in the background and laid out on the
//! board in a grid, like prints on a table.

use super::*;

/// (board generation, where it was dropped in world space, the file, its rendered pages and
/// page count, or why it couldn't be read)
pub(super) type RenderedPdf = (
    u64,
    Pos2,
    std::path::PathBuf,
    Result<(Vec<Vec<u8>>, usize), String>,
);

/// Whether the file at `path` is a PDF (by its first bytes).
pub(super) fn is_pdf_file(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 8];
    std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .is_ok_and(|n| crate::pdf::is_pdf(&head[..n]))
}

impl CoreBoardApp {
    /// Starts rendering `path`'s pages; they're placed at `at` (screen) when ready.
    pub(super) fn import_pdf(&mut self, canvas: Rect, at: Pos2, path: std::path::PathBuf) {
        let tx = self.pdf_tx.clone();
        let generation = self.generation;
        let world = self.to_world(canvas, at);
        self.rendering_pdfs += 1;
        self.toast(tr("Opening the PDF…").into());
        std::thread::spawn(move || {
            let result = std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(crate::pdf::render_pages);
            let _ = tx.send((generation, world, path, result));
        });
    }

    pub(super) fn receive_pdfs(&mut self, ctx: &egui::Context, canvas: Rect) {
        if self.rendering_pdfs > 0 {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        while let Ok((generation, world, path, result)) = self.pdf_rx.try_recv() {
            self.rendering_pdfs = self.rendering_pdfs.saturating_sub(1);
            if generation != self.generation {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match result {
                Ok((pages, total)) => self.place_pages(canvas, world, &name, pages, total),
                Err(err) => {
                    // Still on the board, as a card that opens it.
                    let at = self.to_screen(canvas, world);
                    self.add_file_cards(canvas, at, vec![path]);
                    self.toast(trf("Couldn't show the PDF's pages: {}", &[&err]));
                }
            }
        }
    }

    /// Lays the pages out in a grid centered on `center` (world), in reading order.
    fn place_pages(
        &mut self,
        canvas: Rect,
        center: Pos2,
        name: &str,
        pages: Vec<Vec<u8>>,
        total: usize,
    ) {
        let mut files = Vec::new();
        for bytes in &pages {
            let Ok((w, h)) = image_dimensions(bytes) else {
                continue;
            };
            if let Ok(file) = self.store.import(bytes, "jpg") {
                files.push((file, w as f32 / h.max(1) as f32));
            }
        }
        if files.is_empty() {
            return;
        }
        let n = files.len();
        let columns = ((n as f32 * 1.3).sqrt().ceil() as usize).clamp(1, 8);
        let rows = n.div_ceil(columns);
        // Each page about a third of the window tall when there are few, smaller for many.
        let zoom = self.board.view.zoom;
        let height = canvas.height() * 0.42 / zoom / (rows as f32).sqrt();
        let width = files.iter().map(|(_, a)| a).fold(0.0f32, |m, a| m.max(*a)) * height;
        let gap = height * 0.06;
        // Rows leave room for each page's caption ("book.pdf · 3") underneath.
        let row_gap = gap + board::CAPTION_GAP + board::CAPTION_LINE * 1.2;
        let grid = vec2(
            columns as f32 * (width + gap) - gap,
            rows as f32 * (height + row_gap) - row_gap,
        );
        let origin = center - grid / 2.0;
        self.checkpoint();
        self.selection.clear();
        for (i, (file, aspect)) in files.into_iter().enumerate() {
            let (col, row) = (i % columns, i / columns);
            let size = vec2(height * aspect, height);
            let cell = origin + vec2(col as f32 * (width + gap), row as f32 * (height + row_gap));
            // Centered in its cell, so landscape and portrait pages line up.
            let pos = cell + vec2((width - size.x) / 2.0, 0.0);
            let id = self.board.next_id;
            self.board.next_id += 1;
            self.board.items.push(Item {
                id,
                file,
                pos: [pos.x, pos.y],
                size: [size.x, size.y],
                caption: format!("{name} · {}", i + 1),
                ..Item::default()
            });
            self.selection.insert(id);
        }
        self.touch();
        // If some pages landed off screen, glide over so they're all in view.
        let pages = Rect::from_min_size(origin, grid);
        let view = Rect::from_min_max(
            self.to_world(canvas, canvas.min),
            self.to_world(canvas, canvas.max),
        );
        if !view.contains_rect(pages) {
            let zoom = (canvas.width() / pages.width().max(1.0))
                .min(canvas.height() / pages.height().max(1.0))
                .min(self.board.view.zoom)
                * 0.9;
            let offset = canvas.center() - canvas.min - pages.center().to_vec2() * zoom;
            self.fly_view(board::View {
                offset: [offset.x, offset.y],
                zoom,
            });
        }
        if total > n {
            self.toast(trf(
                "Added the first {} of {} pages from {}",
                &[&n, &total, &name],
            ));
        } else {
            self.toast(trf("Added {} pages from {}", &[&n, &name]));
        }
    }
}
