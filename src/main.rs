mod app;
mod board;
mod export;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(board::default_board_dir);

    let store = match board::Store::open(dir.clone()) {
        Ok(store) => store,
        Err(err) => {
            eprintln!("coreboard: cannot open board at {}: {err}", dir.display());
            std::process::exit(1);
        }
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(format!("Core Board — {}", dir.display()))
            .with_inner_size([1280.0, 800.0])
            .with_drag_and_drop(true)
            // Lets the board's opacity setting show the desktop through the window.
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "coreboard",
        options,
        Box::new(|cc| Ok(Box::new(app::CoreBoardApp::new(cc, store)))),
    )
}
