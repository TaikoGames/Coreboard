//! The floating video player: a small always-on-top window showing a video's page, so a
//! reference video plays next to the board. It runs as its own process (`coreboard
//! --play-video <url> <title>`), which keeps its window and web view apart from the board's.
//!
//! Windows and macOS have a web view built in; on Linux, videos open in the browser instead.

/// The command-line flag that starts the player instead of the board.
pub const FLAG: &str = "--play-video";

/// Opens `url` in the floating player (or the browser where there's no player).
pub fn open(url: &str, title: &str) -> Result<(), String> {
    if cfg!(any(windows, target_os = "macos")) {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        std::process::Command::new(exe)
            .args([FLAG, url, title])
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    } else {
        opener::open_browser(url).map_err(|e| e.to_string())
    }
}

/// Runs the player window (in the player process); returns when it's closed.
#[cfg(any(windows, target_os = "macos"))]
pub fn run(url: &str, title: &str) {
    use tao::dpi::LogicalSize;
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoop};
    use tao::window::WindowBuilder;
    use wry::{PageLoadEvent, WebViewBuilder};

    // For the automated test: notes when the page has loaded.
    let log = std::env::var_os("COREBOARD_PLAYER_LOG");
    let note = move |line: &str| {
        if let Some(path) = &log {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(f, "{line}");
            }
        }
    };

    let event_loop = EventLoop::new();
    let window = match WindowBuilder::new()
        .with_title(if title.is_empty() {
            "Core Board video"
        } else {
            title
        })
        .with_inner_size(LogicalSize::new(720.0, 460.0))
        .with_min_inner_size(LogicalSize::new(320.0, 200.0))
        .with_always_on_top(true)
        .build(&event_loop)
    {
        Ok(window) => window,
        Err(err) => {
            note(&format!("window failed: {err}"));
            let _ = opener::open_browser(url);
            return;
        }
    };
    let loaded = note.clone();
    let webview = WebViewBuilder::new()
        .with_url(url)
        .with_on_page_load_handler(move |event, page| {
            if matches!(event, PageLoadEvent::Finished) {
                loaded(&format!("loaded {page}"));
            }
        })
        .build(&window);
    let webview = match webview {
        Ok(webview) => {
            note("player opened");
            webview
        }
        Err(err) => {
            // No web view on this computer (e.g. WebView2 missing on an old Windows): use the
            // browser instead.
            note(&format!("web view failed: {err}"));
            let _ = opener::open_browser(url);
            return;
        }
    };
    event_loop.run(move |event, _, control_flow| {
        let _ = &webview;
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn run(url: &str, _title: &str) {
    let _ = opener::open_browser(url);
}
