//! Check for Updates: this version and the newest release, what the release says of itself
//! (shown as plain text, the project's words), whether pre-releases are offered and whether the
//! newest is asked after on its own; the installer brought down and checked with how far it is,
//! and installed when the program closes or at once, every change saved or kept first.

use crate::icons::Icon;
use crate::{MailUi, UiRequest, bold, font, one_line};
use openmailclient_engine::update;
use serde_json::{Value, json};

/// How tall the release's notes stand before they scroll.
const NOTES_H: f32 = 200.0;

impl MailUi {
    pub(crate) fn updates_window(&mut self, ctx: &egui::Context) {
        if !self.updates_open {
            return;
        }
        let t = self.tokens.clone();
        let state = self.session.update.clone();
        let running = self.session.running();
        let checking = running.iter().any(|r| r.what == update::CHECKING);
        let downloading = running.iter().find(|r| r.what == update::DOWNLOADING).map(|r| (r.id.clone(), r.done, r.stopping));
        let current = update::current();
        let mut run: Vec<(&str, Value)> = Vec::new();
        let mut close = false;
        let mut quit = false;
        let mut page = None;
        let shown = crate::dialog::Dialog::new("updates", "Check for Updates", Icon::Update).width(480.0).show(ctx, &t, |ui| {
            ui.label(egui::RichText::new(format!("This is OpenMailClient {current}.")).font(font(13.0)).color(t.text));
            ui.add_space(6.0);
            match &state.found {
                _ if checking => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(14.0).color(t.accent));
                        ui.label(egui::RichText::new("Asking GitHub for the newest release…").font(font(13.0)).color(t.text_dim));
                    });
                }
                Some(r) => {
                    crate::dialog::card(ui, &t, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("OpenMailClient {} is available", r.version)).font(bold(14.0)).color(t.text));
                            if r.prerelease {
                                ui.label(egui::RichText::new(" Pre-release ").font(font(11.0)).color(t.surface).background_color(t.accent));
                            }
                        });
                        let mut about = Vec::new();
                        if let Some(day) = r.published.get(..10) {
                            about.push(format!("Published {day}"));
                        }
                        if let Some(size) = r.setup.as_ref().map(|a| a.size).filter(|s| *s > 0) {
                            about.push(format!("{:.1} MB", size as f64 / 1_048_576.0));
                        }
                        if !about.is_empty() {
                            ui.label(egui::RichText::new(about.join(" · ")).font(font(12.0)).color(t.text_dim));
                        }
                        if !r.name.is_empty() && r.name != format!("OpenMailClient {}", r.version) {
                            ui.label(egui::RichText::new(one_line(&r.name)).font(font(12.5)).color(t.text));
                        }
                        if !r.notes.trim().is_empty() {
                            ui.add_space(4.0);
                            egui::ScrollArea::vertical().id_salt("update-notes").max_height(NOTES_H).show(ui, |ui| {
                                // The release's own words, as text: nothing in them is followed.
                                ui.add(egui::Label::new(egui::RichText::new(r.notes.trim()).font(font(12.5)).color(t.text)).wrap());
                            });
                        }
                    });
                    ui.add_space(6.0);
                    if r.setup.is_none() || !cfg!(windows) {
                        ui.label(
                            egui::RichText::new("This release is installed from its page: there is no installer for this system in it.")
                                .font(font(12.5))
                                .color(t.text_dim),
                        );
                    } else if let Some((_, done, stopping)) = downloading {
                        ui.label(egui::RichText::new("Downloading and checking the installer").font(bold(13.0)).color(t.text));
                        crate::progress::bar(ui, done, stopping, &t);
                    } else if state.install.is_some() {
                        ui.label(
                            egui::RichText::new("It is installed when OpenMailClient closes, and OpenMailClient starts again after.")
                                .font(font(12.5))
                                .color(t.ok),
                        );
                    } else if state.ready.is_some() {
                        ui.label(
                            egui::RichText::new("Downloaded, and checked against the sums published with the release.").font(font(12.5)).color(t.ok),
                        );
                    }
                }
                None if state.checked => {
                    ui.label(egui::RichText::new("This is the newest version.").font(bold(13.0)).color(t.ok));
                }
                None => {
                    ui.label(egui::RichText::new("Not checked yet this time.").font(font(13.0)).color(t.text_dim));
                }
            }
            if let Some(failed) = &state.failed {
                ui.add_space(4.0);
                crate::dialog::callout(ui, &t, t.danger, |ui| {
                    ui.label(egui::RichText::new(one_line(failed)).font(font(12.5)).color(t.text));
                });
            }
            ui.add_space(10.0);
            let mut pre = state.prereleases;
            let pre_note = if current.is_pre() {
                "This is a pre-release: newer pre-releases are offered whatever this says"
            } else {
                "Versions that are still being tried, before they are released"
            };
            if ui.checkbox(&mut pre, "Offer pre-releases too").on_hover_text(pre_note).changed() {
                run.push(("help.updateOptions", json!({ "prereleases": pre })));
                run.push(("help.checkForUpdates", json!({ "wait": false })));
            }
            let mut automatic = state.automatic;
            if ui
                .checkbox(&mut automatic, "Check for updates once a day")
                .on_hover_text("Only tells: nothing is downloaded or installed without being asked")
                .changed()
            {
                run.push(("help.updateOptions", json!({ "automatic": automatic })));
            }
            crate::dialog::foot(ui, &t, |ui| {
                if ui.add_enabled(!checking && downloading.is_none(), crate::dialog::plain("Check Now")).clicked() {
                    run.push(("help.checkForUpdates", json!({ "wait": false })));
                }
                if let Some(r) = &state.found
                    && ui.add(crate::dialog::plain("Release Page")).on_hover_text(r.page.clone()).clicked()
                {
                    page = Some(r.page.clone());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let installable = cfg!(windows) && state.found.as_ref().is_some_and(|r| r.setup.is_some());
                    if let Some((id, _, stopping)) = &downloading {
                        if ui.add_enabled(!stopping, crate::dialog::plain("Stop")).clicked() {
                            run.push(("mail.cancelTask", json!({ "id": id })));
                        }
                    } else if installable && state.ready.is_none() {
                        if ui
                            .add(crate::dialog::primary(&t, "Download"))
                            .on_hover_text("Bring the installer down and check it; nothing is run yet")
                            .clicked()
                        {
                            run.push(("help.downloadUpdate", json!({ "wait": false })));
                        }
                    } else if installable && state.ready.is_some() {
                        if ui
                            .add(crate::dialog::primary(&t, "Restart and Install"))
                            .on_hover_text("Close OpenMailClient, install, and start it again")
                            .clicked()
                        {
                            run.push(("help.installUpdate", Value::Null));
                            quit = true;
                        }
                        if state.install.is_none() {
                            if ui.add(crate::dialog::plain("Install on Close")).clicked() {
                                run.push(("help.installUpdate", Value::Null));
                            }
                        } else if ui.add(crate::dialog::plain("Don't Install")).clicked() {
                            run.push(("help.installUpdate", json!({ "cancel": true })));
                        }
                    }
                    if ui.add(crate::dialog::plain("Close")).clicked() {
                        close = true;
                    }
                });
            });
        });
        if shown.closed || close {
            self.updates_open = false;
        }
        for (id, params) in run {
            let result = if id.starts_with("ui.") {
                self.execute(id, &params).map(|_| ())
            } else {
                self.session.run(id, &params).map(|_| ()).map_err(|e| e.to_string())
            };
            if let Err(e) = result {
                self.session.status = e;
                quit = false;
            }
        }
        if let Some(page) = page {
            self.requests.push(UiRequest::OpenLink(page));
        }
        if quit && self.session.update.install.is_some() {
            self.updates_open = false;
            if let Err(e) = self.execute("ui.quit", &Value::Null) {
                self.session.status = e;
            }
        }
        if checking || downloading.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}
