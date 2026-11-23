//! Cloud sync: boards follow the person to every computer they use, through their own Google
//! Drive. Nothing goes through a Core Board server, and Core Board sees only its own files there.

pub mod auth;
pub mod drive;
pub mod sync;

use std::path::PathBuf;
use std::sync::mpsc::Sender;

pub use auth::Account;
pub use sync::Report;

/// What the background work reports back to the app.
pub enum Event {
    SignedIn(Result<Account, String>),
    Synced(Result<Report, String>),
}

/// Whether this build can sync (it was built with a Google sign-in client).
pub fn available() -> bool {
    auth::client().is_some()
}

/// Signs in, in the background (it waits for the browser).
pub fn start_sign_in(tx: Sender<Event>, ctx: eframe::egui::Context) {
    std::thread::spawn(move || {
        let _ = tx.send(Event::SignedIn(auth::sign_in()));
        ctx.request_repaint();
    });
}

/// Syncs `locals` (and brings down boards this computer lacks into `root`) in the background.
pub fn start_sync(
    account: Account,
    locals: Vec<PathBuf>,
    root: PathBuf,
    open: PathBuf,
    tx: Sender<Event>,
    ctx: eframe::egui::Context,
) {
    std::thread::spawn(move || {
        let result = auth::access_token(&account).and_then(|token| {
            let mut drive = drive::Drive::new(token);
            sync::sync_all(&mut drive, &locals, &root, &open)
        });
        let _ = tx.send(Event::Synced(result));
        ctx.request_repaint();
    });
}
