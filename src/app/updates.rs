//! "A new version is out" banner, and the update buttons in the Boards window.

use super::*;
use crate::update::{self, Latest};

pub(super) enum UpdateMsg {
    /// The answer from the website; `manual` when the user pressed "Check for updates".
    Checked(Result<Latest, String>, bool),
    Downloaded(Result<std::path::PathBuf, String>),
}

pub(super) struct Updates {
    tx: Sender<UpdateMsg>,
    rx: Receiver<UpdateMsg>,
    /// The newest version found, if it's newer than this one.
    available: Option<Latest>,
    checking: bool,
    downloading: bool,
    /// "Later" hides the banner until the next start.
    dismissed: bool,
    /// What the last check said, for the Boards window.
    status: Option<String>,
    /// The startup check has been started (or skipped).
    started: bool,
}

impl Default for Updates {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            available: None,
            checking: false,
            downloading: false,
            dismissed: false,
            status: None,
            started: false,
        }
    }
}

const CURRENT: &str = env!("CARGO_PKG_VERSION");

impl CoreBoardApp {
    /// Asks the website for the latest version in the background.
    pub(super) fn check_for_updates(&mut self, manual: bool) {
        if self.updates.checking {
            return;
        }
        self.updates.checking = true;
        if manual {
            self.updates.status = Some(tr("Checking…").into());
        }
        let tx = self.updates.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(UpdateMsg::Checked(update::check(), manual));
        });
    }

    pub(super) fn save_settings(&self) {
        boards::save_settings(&boards::Settings {
            animations: self.animations,
            minimap: self.minimap,
            check_updates: self.check_updates,
            language: self.language.clone(),
        });
    }

    /// The language picker in the Boards window: automatic (the computer's), English, Español.
    pub(super) fn language_controls(&mut self, ui: &mut egui::Ui) {
        let auto = match crate::i18n::system() {
            crate::i18n::Lang::Spanish => "Español",
            crate::i18n::Lang::Catalan => "Català",
            crate::i18n::Lang::English => "English",
        };
        let name = |code: &str| match code {
            "en" => "English".to_string(),
            "es" => "Español".to_string(),
            "ca" => "Català".to_string(),
            _ => trf("Automatic ({})", &[&auto]),
        };
        let before = self.language.clone();
        ui.horizontal(|ui| {
            ui.label(tr("Language"));
            egui::ComboBox::from_id_salt("language")
                .selected_text(name(&self.language))
                .show_ui(ui, |ui| {
                    for code in ["auto", "en", "es", "ca"] {
                        ui.selectable_value(&mut self.language, code.to_string(), name(code));
                    }
                });
        });
        if self.language != before {
            crate::i18n::apply(&self.language);
            self.save_settings();
        }
    }

    pub(super) fn receive_updates(&mut self, ctx: &egui::Context) {
        if !self.updates.started {
            self.updates.started = true;
            if self.check_updates {
                self.check_for_updates(false);
            }
        }
        if self.updates.checking || self.updates.downloading {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        while let Ok(msg) = self.updates.rx.try_recv() {
            match msg {
                UpdateMsg::Checked(result, manual) => {
                    self.updates.checking = false;
                    match result {
                        Ok(latest) if update::is_newer(&latest.version, CURRENT) => {
                            self.updates.status =
                                Some(trf("Version {} is available", &[&latest.version]));
                            self.updates.available = Some(latest);
                            // Asking again brings the banner back.
                            if manual {
                                self.updates.dismissed = false;
                            }
                        }
                        Ok(_) => {
                            self.updates.status = Some(trf("You're up to date ({})", &[&CURRENT]));
                            if manual {
                                self.toast(trf("Core Board {} is the latest version", &[&CURRENT]));
                            }
                        }
                        Err(err) => {
                            eprintln!("coreboard: update check: {err}");
                            self.updates.status = Some(trf("Couldn't check: {}", &[&err]));
                            if manual {
                                self.toast(trf("Couldn't check for updates: {}", &[&err]));
                            }
                        }
                    }
                }
                UpdateMsg::Downloaded(result) => {
                    self.updates.downloading = false;
                    let started = result.and_then(|path| update::run_installer(&path));
                    match started {
                        Ok(()) => {
                            // The installer replaces this app and opens the new one.
                            self.save_now();
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        Err(err) => {
                            eprintln!("coreboard: update: {err}");
                            self.toast(trf(
                                "Update failed: {}. Opening the download page instead.",
                                &[&err],
                            ));
                            ctx.open_url(egui::OpenUrl::new_tab(update::DOWNLOAD_PAGE));
                        }
                    }
                }
            }
        }
    }

    /// One click: download the installer, then run it (Windows); elsewhere the download page.
    fn start_update(&mut self, ctx: &egui::Context) {
        let Some(latest) = self.updates.available.clone() else {
            return;
        };
        if update::can_self_install() && latest.windows_installer.is_some() {
            self.updates.downloading = true;
            let tx = self.updates.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(UpdateMsg::Downloaded(update::download_installer(&latest)));
            });
        } else {
            ctx.open_url(egui::OpenUrl::new_tab(latest.page().to_string()));
        }
    }

    /// The banner at the top while a newer version is available.
    pub(super) fn update_banner(&mut self, ctx: &egui::Context, canvas: Rect) {
        let Some(latest) = self.updates.available.clone() else {
            return;
        };
        if self.updates.dismissed {
            return;
        }
        let one_click = update::can_self_install() && latest.windows_installer.is_some();
        let mut update_now = false;
        let mut later = false;
        egui::Area::new(egui::Id::unique("coreboard-update"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 12.0))
            .constrain_to(canvas)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_max_width(420.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(trf(
                                    "Core Board {} is available",
                                    &[&latest.version],
                                ))
                                .strong()
                                .color(Color32::WHITE),
                            );
                            ui.label(
                                egui::RichText::new(trf("(you have {})", &[&CURRENT]))
                                    .small()
                                    .weak(),
                            );
                        });
                        let notes = match crate::i18n::current() {
                            crate::i18n::Lang::Spanish if !latest.notes_es.is_empty() => {
                                &latest.notes_es
                            }
                            crate::i18n::Lang::Catalan if !latest.notes_ca.is_empty() => {
                                &latest.notes_ca
                            }
                            _ => &latest.notes,
                        };
                        for note in notes.iter().take(3) {
                            ui.label(egui::RichText::new(format!("• {note}")).small());
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if self.updates.downloading {
                                ui.spinner();
                                ui.label(tr("Downloading the update…"));
                            } else {
                                let label = if one_click {
                                    tr("Update now")
                                } else {
                                    tr("Download")
                                };
                                update_now = ui
                                    .button(egui::RichText::new(label).strong())
                                    .on_hover_text(if one_click {
                                        tr("Downloads and installs it, then reopens Core Board. \
                                         Your boards are saved first.")
                                    } else {
                                        tr("Opens the download page")
                                    })
                                    .clicked();
                                if ui.button(tr("What's new")).clicked() {
                                    ctx.open_url(egui::OpenUrl::new_tab(
                                        "https://mikibutler.ca/coreboard/#releases",
                                    ));
                                }
                                later = ui.button(tr("Later")).clicked();
                            }
                        });
                    });
            });
        if update_now {
            self.start_update(ctx);
        }
        if later {
            self.updates.dismissed = true;
        }
    }

    /// The "Updates" part of the Boards window.
    pub(super) fn update_controls(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(egui::RichText::new(format!("Core Board {CURRENT}")).strong());
        ui.horizontal(|ui| {
            let checking = self.updates.checking;
            if ui
                .add_enabled(!checking, egui::Button::new(tr("Check for updates")))
                .clicked()
            {
                self.check_for_updates(true);
            }
            if ui
                .button(tr("Download page"))
                .on_hover_text(tr(
                    "mikibutler.ca/coreboard: every version, for Windows, macOS and Linux",
                ))
                .clicked()
            {
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(update::DOWNLOAD_PAGE));
            }
        });
        if let Some(status) = &self.updates.status {
            ui.label(egui::RichText::new(status).small().weak());
        }
        if self.updates.available.is_some()
            && self.updates.dismissed
            && ui.link(tr("Show the update")).clicked()
        {
            self.updates.dismissed = false;
        }
        self.language_controls(ui);
        if ui
            .checkbox(
                &mut self.check_updates,
                tr("Check for updates when Core Board starts"),
            )
            .on_hover_text(tr(
                "One small request to mikibutler.ca. Nothing about you or your boards is sent.",
            ))
            .changed()
        {
            self.save_settings();
        }
    }
}

impl CoreBoardApp {
    /// Shown while the open board couldn't be read: it's left untouched, and this explains why.
    pub(super) fn unreadable_banner(&mut self, ctx: &egui::Context, canvas: Rect) {
        let Some(reason) = self.store.unreadable.clone() else {
            return;
        };
        let mut check = false;
        egui::Area::new(egui::Id::unique("coreboard-unreadable"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .constrain_to(canvas)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::symmetric(18, 14))
                    .show(ui, |ui| {
                        ui.set_max_width(440.0);
                        ui.label(
                            egui::RichText::new(tr("This board can't be opened in this version"))
                                .strong()
                                .size(16.0)
                                .color(Color32::WHITE),
                        );
                        ui.add_space(4.0);
                        ui.label(tr(
                            "It was probably saved by a newer Core Board. Your board is safe: \
                             this version won't change or save over it. Update Core Board to \
                             open it.",
                        ));
                        ui.label(
                            egui::RichText::new(trf("Details: {}", &[&reason]))
                                .small()
                                .weak(),
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            check = ui
                                .button(egui::RichText::new(tr("Check for updates")).strong())
                                .clicked();
                            if ui.button(tr("Download page")).clicked() {
                                ctx.open_url(egui::OpenUrl::new_tab(update::DOWNLOAD_PAGE));
                            }
                            if ui.button(tr("Other boards…")).clicked() {
                                self.show_boards = true;
                            }
                        });
                    });
            });
        if check {
            self.check_for_updates(true);
        }
    }
}
