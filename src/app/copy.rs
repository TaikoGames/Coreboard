//! Ctrl+C: copies the selection as a picture, to paste into Photoshop, Krita, Blender, a chat…
//! One image is copied at full resolution as the board shows it (crop, turns, mirroring,
//! study view); several items are copied as one picture, the way an export renders them.

use super::*;

impl CoreBoardApp {
    pub(super) fn copy_selection(&mut self) {
        let items: Vec<Item> = self
            .board
            .items
            .iter()
            .filter(|i| self.selection.contains(&i.id))
            .cloned()
            .collect();
        if items.is_empty() {
            self.toast(tr("Select something to copy").into());
            return;
        }
        let images_dir = self.store.dir.join("images");
        let picture = match items.as_slice() {
            [item] if item.is_image() => {
                let filter = match self.board_values {
                    1 => Filter::Gray,
                    2 => Filter::Study(Analysis::Values3),
                    _ => Filter::for_item(item),
                };
                export::shown_image(&images_dir, item, filter).map(|img| img.to_rgba8())
            }
            _ => export::render(&images_dir, &items, &board::Watermark::default()),
        };
        let picture = match picture {
            Ok(picture) => picture,
            Err(err) => {
                self.toast(trf("Couldn't copy: {}", &[&err]));
                return;
            }
        };
        let Some(clipboard) = &mut self.clipboard else {
            self.toast(trf("Couldn't copy: {}", &[&tr("no clipboard")]));
            return;
        };
        let (width, height) = picture.dimensions();
        let result = clipboard.set_image(arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: std::borrow::Cow::Owned(picture.into_raw()),
        });
        match result {
            Ok(()) if items.len() == 1 => self.toast(tr("Copied: paste it in any app").into()),
            Ok(()) => self.toast(trf("Copied {} items as one picture", &[&items.len()])),
            Err(err) => self.toast(trf("Couldn't copy: {}", &[&err])),
        }
    }
}
