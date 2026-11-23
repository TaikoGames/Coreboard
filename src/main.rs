// Release builds on Windows are a normal windowed app, without a console window behind them.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod board;
mod boardfile;
mod boards;
mod export;
mod filters;
mod i18n;
mod library;
mod pdf;
mod player;
mod update;
mod web;

use std::path::PathBuf;

fn main() -> eframe::Result {
    // `coreboard --play-video <url> <title>` is the floating video player, not the board.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some(player::FLAG) {
        let url = args.get(2).map(String::as_str).unwrap_or_default();
        player::run(url, args.get(3).map(String::as_str).unwrap_or_default());
        return Ok(());
    }
    update::remove_old_copies();
    i18n::apply(&boards::settings().language);
    // A folder given on the command line, else the board used last, else the default one. A
    // board file (double-clicked, or given on the command line) opens as a new board.
    let dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .and_then(|arg| {
            if !boardfile::is_board_file(&arg) {
                return Some(arg);
            }
            boardfile::open(&arg, &boards::boards_root())
                .inspect_err(|err| eprintln!("coreboard: {}: {err}", arg.display()))
                .ok()
        })
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
            .with_title(format!(
                "Core Board {} — {}",
                env!("CARGO_PKG_VERSION"),
                dir.display()
            ))
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
