// Release builds on Windows are a normal windowed app, without a console window behind them.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod board;
mod boards;
mod export;
mod filters;
mod library;
mod publish;

use std::path::PathBuf;

fn main() -> eframe::Result {
    // A folder given on the command line, else the board used last, else the default one.
    let dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| boards::recent().into_iter().next())
        .unwrap_or_else(board::default_board_dir);

    let store = match board::Store::open(dir.clone()) {
        Ok(store) => store,
        Err(err) => {
            eprintln!("coreboard: cannot open board at {}: {err}", dir.display());
            std::process::exit(1);
        }
    };
    boards::remember(&dir);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(format!("Core Board — {}", dir.display()))
            .with_inner_size([1280.0, 800.0])
            .with_drag_and_drop(true)
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
                    .unwrap_or_default(),
            )
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
