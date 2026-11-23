//! Saving a board as one `.coreboard` file, and opening one (from the Boards window, by
//! dropping it on the window, or from the command line / a double-click).

use super::*;
use crate::boardfile;

impl CoreBoardApp {
    /// Asks where to save, then packs the whole board into one file.
    pub(super) fn save_board_file(&mut self) {
        self.finish_edit();
        self.save_now();
        let Some(mut out) = rfd::FileDialog::new()
            .set_title(tr("Save board as a file"))
            .set_file_name(boardfile::suggested_name(&self.store.dir))
            .add_filter("Core Board", &[boardfile::EXT])
            .save_file()
        else {
            return;
        };
        if !boardfile::is_board_file(&out) {
            out.set_extension(boardfile::EXT);
        }
        match boardfile::save(&self.store.dir, &self.board, &out) {
            Ok(count) => {
                let name = out
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.toast(trf(
                    "Saved \u{201c}{}\u{201d} with {} pictures",
                    &[&name, &count],
                ));
            }
            Err(err) => self.toast(trf("Couldn't save the board file: {}", &[&err])),
        }
    }

    /// Asks for a board file and opens it.
    pub(super) fn pick_board_file(&mut self, ctx: &egui::Context) {
        if let Some(file) = rfd::FileDialog::new()
            .set_title(tr("Open a board file"))
            .add_filter("Core Board", &[boardfile::EXT])
            .pick_file()
        {
            self.open_board_file(ctx, &file);
        }
    }

    /// Unpacks a board file into a new board and switches to it.
    pub(super) fn open_board_file(&mut self, ctx: &egui::Context, file: &Path) {
        match boardfile::open(file, &boards::boards_root()) {
            Ok(dir) => self.switch_board(ctx, dir),
            Err(err) => self.toast(trf("Couldn't open that board file: {}", &[&err])),
        }
    }
}
