//! Cloud sync in the app: the Cloud section of the Boards window, and syncing in the
//! background (on start, when boards change, and now and then to pick up other computers'
//! changes).

use std::sync::mpsc::{Receiver, Sender, channel};

use super::*;
use crate::cloud::{self, Account, Event, sync};

/// Sync this soon after the board changes.
const AFTER_CHANGES: Duration = Duration::from_secs(60);
/// And this often anyway, for changes made on other computers.
const EVERY: Duration = Duration::from_secs(10 * 60);

#[derive(PartialEq)]
enum Busy {
    No,
    SigningIn,
    Syncing,
}

pub(super) struct CloudSync {
    account: Option<Account>,
    busy: Busy,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    /// When the last sync finished, and what it said (an error stays until the next one).
    last: Option<(Instant, Result<String, String>)>,
    /// Edits counted when the running sync started, to tell if the open board changed during it.
    edits_at_start: u64,
    /// Edits counted at the last finished sync.
    edits_synced: u64,
    /// Set when sync should run at the next chance (start, board switched, "Sync now").
    wanted: bool,
}

impl CloudSync {
    pub(super) fn new() -> Self {
        let (tx, rx) = channel();
        let account = cloud::auth::load_account();
        Self {
            wanted: account.is_some(),
            account,
            busy: Busy::No,
            tx,
            rx,
            last: None,
            edits_at_start: 0,
            edits_synced: 0,
        }
    }

    /// Asks for a sync soon (after a board switch, for example).
    pub(super) fn want(&mut self) {
        self.wanted = self.account.is_some();
    }
}

/// "just now", "5 min ago"...
fn ago(at: Instant) -> String {
    let mins = at.elapsed().as_secs() / 60;
    if mins == 0 {
        tr("just now").to_string()
    } else {
        trf("{} min ago", &[&mins])
    }
}

impl CoreBoardApp {
    /// Every board this computer knows: recent ones, every folder next to them, and the open one.
    fn local_boards(&self) -> Vec<std::path::PathBuf> {
        let mut dirs = vec![self.store.dir.clone()];
        dirs.extend(boards::recent());
        if let Ok(entries) = std::fs::read_dir(boards::boards_root()) {
            let mut found: Vec<_> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.join("board.json").is_file())
                .collect();
            found.sort();
            dirs.extend(found);
        }
        dirs
    }

    fn start_sync(&mut self, ctx: &egui::Context) {
        let Some(account) = self.cloud.account.clone() else {
            return;
        };
        // What's on screen goes up as it is now.
        self.finish_edit();
        self.save_now();
        self.cloud.busy = Busy::Syncing;
        self.cloud.wanted = false;
        self.cloud.edits_at_start = self.edits;
        cloud::start_sync(
            account,
            self.local_boards(),
            boards::boards_root(),
            self.store.dir.clone(),
            self.cloud.tx.clone(),
            ctx.clone(),
        );
    }

    /// Takes results from the background, and starts a sync when one is due.
    pub(super) fn cloud_tick(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.cloud.rx.try_recv() {
            match event {
                Event::SignedIn(Ok(account)) => {
                    self.cloud.busy = Busy::No;
                    self.toast(trf("Connected to Google Drive as {}", &[&account.email]));
                    self.cloud.account = Some(account);
                    self.cloud.wanted = true;
                }
                Event::SignedIn(Err(err)) => {
                    self.cloud.busy = Busy::No;
                    self.cloud.last = Some((Instant::now(), Err(err)));
                }
                Event::Synced(result) => {
                    self.cloud.busy = Busy::No;
                    self.cloud.edits_synced = self.cloud.edits_at_start;
                    match result {
                        Ok(report) => self.after_sync(ctx, report),
                        Err(err) => {
                            eprintln!("coreboard: cloud sync: {err}");
                            self.cloud.last = Some((Instant::now(), Err(err)));
                        }
                    }
                }
            }
        }
        if self.cloud.account.is_none() || self.cloud.busy != Busy::No {
            return;
        }
        let since = self.cloud.last.as_ref().map(|(at, _)| at.elapsed());
        let changed = self.edits != self.cloud.edits_synced
            && since.is_none_or(|s| s >= AFTER_CHANGES)
            && self
                .dirty_since
                .is_none_or(|d| d.elapsed() >= Duration::from_secs(3));
        if self.cloud.wanted || changed || since.is_some_and(|s| s >= EVERY) {
            self.start_sync(ctx);
        }
        ctx.request_repaint_after(Duration::from_secs(5));
    }

    fn after_sync(&mut self, ctx: &egui::Context, report: cloud::Report) {
        if report.incoming {
            if self.edits == self.cloud.edits_at_start {
                // Changed on another computer and not here meanwhile: show the new version.
                if let Some(mut board) = sync::take_incoming(&self.store.dir) {
                    self.checkpoint();
                    board.view = self.board.view.clone();
                    self.board = board;
                    self.selection
                        .retain(|id| self.board.items.iter().any(|i| i.id == *id));
                    self.save_now();
                    self.cloud.edits_synced = self.edits;
                    self.toast(tr("Updated with changes from your other computer").into());
                }
            } else {
                // Edited here meanwhile: the next sync keeps both versions.
                sync::discard_incoming(&self.store.dir);
                self.cloud.wanted = true;
            }
        }
        for dir in &report.new_boards {
            boards::add_recent(dir);
        }
        if !report.new_boards.is_empty() && self.show_boards {
            self.recent = boards::recent();
        }
        if report.conflicts > 0 {
            self.toast(
                tr("A board changed on two computers at once: both versions are kept, see Boards")
                    .into(),
            );
        }
        self.cloud.last = Some((Instant::now(), Ok(String::new())));
        ctx.request_repaint();
    }

    /// The Cloud section of the Boards window.
    pub(super) fn cloud_controls(&mut self, ui: &mut egui::Ui) {
        ui.label(tr("Cloud sync"));
        if !cloud::available() {
            ui.label(
                egui::RichText::new(tr("Cloud sync isn't switched on in this version yet.")).weak(),
            );
            return;
        }
        let ctx = ui.ctx().clone();
        match (&self.cloud.account, &self.cloud.busy) {
            (_, Busy::SigningIn) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr("Finish signing in in your browser…"));
                });
            }
            (None, _) => {
                ui.label(
                    egui::RichText::new(tr(
                        "Your boards on every computer, through your own Google Drive. Core \
                         Board only sees the files it makes there.",
                    ))
                    .weak(),
                );
                if ui.button(tr("Connect Google Drive…")).clicked() {
                    self.cloud.busy = Busy::SigningIn;
                    self.cloud.last = None;
                    cloud::start_sign_in(self.cloud.tx.clone(), ctx);
                }
            }
            (Some(account), busy) => {
                let email = account.email.clone();
                let syncing = *busy == Busy::Syncing;
                ui.label(trf("Connected: {}", &[&email]));
                ui.horizontal(|ui| {
                    if syncing {
                        ui.spinner();
                        ui.label(tr("Syncing…"));
                    } else {
                        if ui.button(tr("Sync now")).clicked() {
                            self.start_sync(&ctx);
                        }
                        if let Some((at, Ok(_))) = &self.cloud.last {
                            ui.label(egui::RichText::new(trf("Synced {}", &[&ago(*at)])).weak());
                        }
                    }
                });
                if !syncing
                    && ui
                        .small_button(tr("Disconnect"))
                        .on_hover_text(tr(
                            "Stops syncing on this computer. Your boards stay here and in Drive.",
                        ))
                        .clicked()
                {
                    cloud::auth::forget_account();
                    self.cloud.account = None;
                    self.cloud.last = None;
                }
            }
        }
        if let Some((_, Err(err))) = &self.cloud.last {
            ui.colored_label(
                Color32::from_rgb(255, 140, 120),
                trf("Couldn't sync: {}", &[err]),
            );
        }
    }
}
