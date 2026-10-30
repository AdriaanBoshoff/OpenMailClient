//! Import from Outlook: what Outlook's profiles hold shown before anything is done, each account
//! and data file chosen or left, then brought by `file.importOutlook` on a thread of its own.
//! Outlook's files are copied, never changed, and no password is brought: the window says where
//! each account's password is given afterwards.
//!
//! Better than Outlook's own Import/Export, which brings one data file at a time into one folder:
//! every account of every profile with its servers and its mail, each shown with how big its data
//! file is and whether Outlook holds it open, together, while the window stays in use.

use crate::dialog::{self, Dialog};
use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::{MailUi, bold, font, one_line};
use egui::{Sense, Ui, vec2};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// The job's name, as `file.importOutlook` gives it.
const JOB: &str = "Importing from Outlook";

#[derive(Default)]
pub(crate) struct ImportWindow {
    /// What `file.importOutlook` showed when the window was opened or looked again.
    pub preview: Value,
    /// The ids of what is to be brought.
    pub chosen: BTreeSet<String>,
    /// Each account's data file is brought with it.
    pub with_data: bool,
    /// The import runs.
    pub running: bool,
    /// What came of the last import.
    pub result: Option<Value>,
    /// A word for the owner, and whether it is a warning.
    pub said: Option<(String, bool)>,
}

impl ImportWindow {
    /// Open on what the profiles hold: everything that can be brought is chosen, an account here
    /// already is not.
    fn of(preview: Value) -> Self {
        let mut chosen = BTreeSet::new();
        for p in profiles(&preview) {
            for a in list(p, "accounts") {
                if a["here"] != true
                    && let Some(id) = a["id"].as_str()
                {
                    chosen.insert(id.to_string());
                }
            }
            for f in list(p, "dataFiles") {
                if f["state"] == "ready"
                    && let Some(id) = f["id"].as_str()
                {
                    chosen.insert(id.to_string());
                }
            }
        }
        for g in list(&preview, "signatures") {
            if g["here"] != true
                && let Some(id) = g["id"].as_str()
            {
                chosen.insert(id.to_string());
            }
        }
        Self { preview, chosen, with_data: true, ..Self::default() }
    }

    /// Chosen data files Outlook holds open, the accounts' own when they come along.
    fn in_use(&self) -> usize {
        profiles(&self.preview)
            .flat_map(|p| {
                let accounts = list(p, "accounts").filter(|_| self.with_data).map(|a| (&a["id"], &a["dataFile"]["state"]));
                accounts.chain(list(p, "dataFiles").map(|f| (&f["id"], &f["state"]))).collect::<Vec<_>>()
            })
            .filter(|(id, state)| *state == "inUse" && id.as_str().is_some_and(|id| self.chosen.contains(id)))
            .count()
    }
}

fn profiles(preview: &Value) -> impl Iterator<Item = &Value> {
    preview["profiles"].as_array().into_iter().flatten()
}

fn list<'a>(p: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    p[key].as_array().into_iter().flatten()
}

fn text(v: &Value) -> String {
    one_line(v.as_str().unwrap_or_default())
}

/// A server as a line: its kind, where it is and how it is reached.
fn server(kind: &str, s: &Value) -> String {
    let security = match s["security"].as_str() {
        Some("tls") => "SSL/TLS",
        Some("starttls") => "STARTTLS",
        _ => "no encryption",
    };
    let port = s["port"].as_u64().map(|p| format!(":{p}")).unwrap_or_default();
    format!("{kind} {}{port}, {security}", text(&s["host"]))
}

/// A data file's size and state in words, and whether that is a warning.
fn file_state(f: &Value) -> (String, bool) {
    let size = f["size"].as_u64().map(|b| openmailclient_engine::attachments::size_words(usize::try_from(b).unwrap_or(usize::MAX)));
    let name = std::path::Path::new(f["path"].as_str().unwrap_or_default()).file_name().map(|n| one_line(&n.to_string_lossy())).unwrap_or_default();
    match f["state"].as_str() {
        Some("ready") => (format!("{name}, {}", size.unwrap_or_default()), false),
        Some("inUse") => (format!("{name}: Outlook has it open; close Outlook to bring it"), true),
        Some("missing") => (format!("{name}: not there any more"), true),
        _ => (format!("{name}: cannot be read"), true),
    }
}

impl MailUi {
    /// `ui.importOutlook`: open the window on what Outlook's profiles hold, look again, import
    /// what is chosen, or close it.
    pub(crate) fn import_command(&mut self, params: &Value) -> Result<Value, String> {
        if params.get("close").and_then(Value::as_bool) == Some(true) {
            self.import_window = None;
            return Ok(Value::Null);
        }
        if params.get("import").and_then(Value::as_bool) == Some(true) {
            let Some(mut window) = self.import_window.take() else { return Err("Import from Outlook is not open".to_string()) };
            let started = self.start_import(&mut window);
            self.import_window = Some(window);
            return started;
        }
        let preview = self.execute("file.importOutlook", &Value::Null)?;
        let window = ImportWindow::of(preview);
        let shown = json!({ "chosen": window.chosen.iter().collect::<Vec<_>>() });
        self.import_window = Some(window);
        Ok(shown)
    }

    fn start_import(&mut self, window: &mut ImportWindow) -> Result<Value, String> {
        if window.running {
            return Err("The import runs already".to_string());
        }
        if window.chosen.is_empty() {
            return Err("Nothing is chosen to import".to_string());
        }
        let ids: Vec<&String> = window.chosen.iter().collect();
        let params = json!({ "import": ids, "withData": window.with_data, "wait": false });
        match self.execute("file.importOutlook", &params) {
            Ok(v) => {
                window.running = v["started"] == true;
                window.result = None;
                window.said = None;
                if !window.running {
                    // Done at once, as a run that waits does it.
                    window.result = Some(v.clone());
                }
                Ok(v)
            }
            Err(e) => {
                window.said = Some((e.clone(), true));
                Err(e)
            }
        }
    }

    /// The Import from Outlook window, while it is open.
    pub(crate) fn import_window(&mut self, ctx: &egui::Context) {
        let Some(mut window) = self.import_window.take() else { return };
        let job = self.session.running().into_iter().find(|r| r.what == JOB);
        if window.running && job.is_none() {
            window.running = false;
            window.result = self.session.finished.iter().rev().find(|f| f["job"] == JOB).map(|f| f["result"].clone());
            window.said = Some((one_line(&self.session.status), false));
            // What is here now: an account brought is here already when looked at again.
            if let Ok(preview) = self.execute("file.importOutlook", &Value::Null) {
                window.preview = preview;
                window.chosen.clear();
            }
        }
        let t = self.tokens.clone();
        let mut close = false;
        let mut look_again = false;
        let mut start = false;
        let mut cancel = false;
        let mut passwords = false;
        let room = (ctx.content_rect().height() - 330.0).clamp(160.0, 520.0);
        let shown = Dialog::new("import-outlook", "Import from Outlook", Icon::Import).width(640.0).show(ctx, &t, |ui| {
            ui.label(
                egui::RichText::new("Accounts and data files found in Outlook's profiles. Outlook's files are copied, never changed. Passwords are not brought: Outlook keeps them locked to itself.")
                    .font(font(12.0))
                    .color(t.text_dim),
            );
            ui.add_space(10.0);
            let busy = window.running;
            egui::ScrollArea::vertical().id_salt("import-list").max_height(room).show(ui, |ui| {
                let mut any = false;
                for p in profiles(&window.preview) {
                    any = true;
                    let name = text(&p["name"]);
                    dialog::heading(ui, &t, &if p["default"] == true { format!("Profile {name} (the one Outlook starts with)") } else { format!("Profile {name}") });
                    for a in list(p, "accounts") {
                        account_card(ui, &t, a, &mut window.chosen, window.with_data, busy);
                    }
                    for f in list(p, "dataFiles") {
                        file_card(ui, &t, f, &mut window.chosen, busy);
                    }
                    for k in list(p, "skipped") {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                            icons::paint(ui.painter(), r, Icon::Info, t.text_dim);
                            let line = one_line(&format!("{} is not brought: {}", text(&k["name"]), text(&k["why"])));
                            ui.add(egui::Label::new(egui::RichText::new(line).font(font(12.0)).color(t.text_dim)).wrap());
                        });
                    }
                    ui.add_space(8.0);
                }
                let sigs: Vec<&Value> = list(&window.preview, "signatures").collect();
                if !sigs.is_empty() {
                    any = true;
                    dialog::heading(ui, &t, "Signatures");
                    for g in sigs {
                        signature_card(ui, &t, g, &mut window.chosen, busy);
                    }
                }
                if !any {
                    ui.label(egui::RichText::new("No Outlook profile was found on this computer.").font(font(13.0)).color(t.text));
                }
            });
            ui.add_space(6.0);
            ui.add_enabled_ui(!busy, |ui| {
                ui.checkbox(&mut window.with_data, egui::RichText::new("Bring each account's mail, calendar, contacts and tasks with it").font(font(13.0)));
            });
            let in_use = window.in_use();
            if in_use > 0 {
                ui.add_space(6.0);
                dialog::callout(ui, &t, t.danger, |ui| {
                    let what = if in_use == 1 { "a data file" } else { "data files" };
                    ui.label(egui::RichText::new(format!("Outlook has {what} chosen here open. Close Outlook, then press Look Again.")).font(font(12.5)).color(t.text));
                });
            }
            if let Some(j) = &job {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(one_line(&j.what)).font(bold(13.0)).color(t.text));
                crate::progress::bar(ui, j.done, j.stopping, &t);
            }
            if let Some(result) = &window.result {
                ui.add_space(8.0);
                passwords = shown_result(ui, &t, result);
            } else if let Some((said, warn)) = &window.said {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(one_line(said)).font(font(12.5)).color(if *warn { t.danger } else { t.text_dim }));
            }
            dialog::foot(ui, &t, |ui| {
                if ui.add_enabled(!busy, dialog::plain("Look Again")).clicked() {
                    look_again = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if busy {
                        if ui.add_enabled(job.as_ref().is_some_and(|j| !j.stopping), dialog::plain("Stop")).clicked() {
                            cancel = true;
                        }
                    } else {
                        let n = window.chosen.len();
                        let label = if n == 0 { "Import".to_string() } else { format!("Import {n}") };
                        if ui.add_enabled(n > 0, dialog::primary(&t, &label)).clicked() {
                            start = true;
                        }
                    }
                    if ui.add(dialog::plain(if window.result.is_some() { "Done" } else { "Cancel" })).clicked() {
                        close = true;
                    }
                });
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) && !busy {
                close = true;
            }
        });
        if (shown.closed || close) && !window.running {
            if passwords && close {
                self.act("ui.accountSettings", Value::Null);
            }
            return;
        }
        if look_again && let Ok(preview) = self.execute("file.importOutlook", &Value::Null) {
            window = ImportWindow { with_data: window.with_data, ..ImportWindow::of(preview) };
        }
        if start {
            let _ = self.start_import(&mut window);
        }
        if cancel && let Some(j) = &job {
            let _ = self.session.run("mail.cancelTask", &json!({ "id": j.id }));
        }
        if window.running {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.import_window = Some(window);
    }
}

/// One account: chosen or not, its address, its servers and its data file.
fn account_card(ui: &mut Ui, t: &Tokens, a: &Value, chosen: &mut BTreeSet<String>, with_data: bool, busy: bool) {
    let id = a["id"].as_str().unwrap_or_default().to_string();
    dialog::card(ui, t, |ui| {
        ui.horizontal_top(|ui| {
            let mut on = chosen.contains(&id);
            if ui.add_enabled(!busy, egui::Checkbox::without_text(&mut on)).changed() {
                if on {
                    chosen.insert(id.clone())
                } else {
                    chosen.remove(&id)
                };
            }
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(text(&a["name"])).font(bold(13.5)).color(t.text));
                    ui.label(egui::RichText::new(text(&a["email"])).font(font(12.5)).color(t.text_dim));
                });
                let kind = if a["kind"] == "pop3" { "POP3" } else { "IMAP" };
                let mut line = server(kind, &a["incoming"]);
                if a["outgoing"].is_object() {
                    line = format!("{line} · {}", server("SMTP", &a["outgoing"]));
                }
                ui.label(egui::RichText::new(line).font(font(12.0)).color(t.text_dim));
                if a["here"] == true {
                    ui.label(
                        egui::RichText::new("There is an account for this address here already: only its data file is brought, beside it.")
                            .font(font(12.0))
                            .color(t.text_dim),
                    );
                }
                if with_data && a["dataFile"].is_object() {
                    let (words, warn) = file_state(&a["dataFile"]);
                    small_line(ui, t, Icon::Folder, &format!("Its mail: {words}"), warn);
                } else if with_data {
                    small_line(ui, t, Icon::Folder, "Outlook keeps no data file of its own for it; its mail comes from the server", false);
                }
                let sig = &a["signatures"];
                let named = |v: &Value| v.as_str().map(str::to_string);
                match (named(&sig["new"]), named(&sig["replies"])) {
                    (Some(n), Some(r)) if n == r => small_line(ui, t, Icon::Rename, &format!("Signs with {n}"), false),
                    (Some(n), Some(r)) => {
                        small_line(ui, t, Icon::Rename, &format!("Signs new messages with {n}, replies and forwards with {r}"), false)
                    }
                    (Some(n), None) => small_line(ui, t, Icon::Rename, &format!("Signs new messages with {n}"), false),
                    (None, Some(r)) => small_line(ui, t, Icon::Rename, &format!("Signs replies and forwards with {r}"), false),
                    (None, None) => {}
                }
                for note in list(a, "notes") {
                    let note = text(note);
                    let mut chars = note.chars();
                    let note: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
                    small_line(ui, t, Icon::Info, &note, false);
                }
            });
        });
    });
    ui.add_space(4.0);
}

/// One of Outlook's signatures: its name and the first lines of its words.
fn signature_card(ui: &mut Ui, t: &Tokens, g: &Value, chosen: &mut BTreeSet<String>, busy: bool) {
    let id = g["id"].as_str().unwrap_or_default().to_string();
    dialog::card(ui, t, |ui| {
        ui.horizontal_top(|ui| {
            let mut on = chosen.contains(&id);
            if ui.add_enabled(!busy, egui::Checkbox::without_text(&mut on)).changed() {
                if on {
                    chosen.insert(id.clone())
                } else {
                    chosen.remove(&id)
                };
            }
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(text(&g["name"])).font(bold(13.5)).color(t.text));
                let words = text(&g["text"]);
                let first: Vec<&str> = words.lines().filter(|l| !l.trim().is_empty()).take(2).collect();
                if !first.is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new(one_line(&first.join(" · "))).font(font(12.0)).color(t.text_dim)).truncate());
                }
                if g["here"] == true {
                    small_line(ui, t, Icon::Check, "There is one of this name and these words here already", false);
                } else if g["formatted"] == true {
                    small_line(ui, t, Icon::Info, "Its words are brought; its formatting and pictures are not yet", false);
                }
            });
        });
    });
    ui.add_space(4.0);
}

/// A data file opened beside the accounts: an archive, a file of old mail.
fn file_card(ui: &mut Ui, t: &Tokens, f: &Value, chosen: &mut BTreeSet<String>, busy: bool) {
    let id = f["id"].as_str().unwrap_or_default().to_string();
    let ready = f["state"] == "ready" || f["state"] == "inUse";
    dialog::card(ui, t, |ui| {
        ui.horizontal_top(|ui| {
            let mut on = chosen.contains(&id);
            if ui.add_enabled(!busy && ready, egui::Checkbox::without_text(&mut on)).changed() {
                if on {
                    chosen.insert(id.clone())
                } else {
                    chosen.remove(&id)
                };
            }
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(text(&f["name"])).font(bold(13.5)).color(t.text));
                let (words, warn) = file_state(f);
                small_line(ui, t, Icon::Folder, &format!("Data file: {words}"), warn);
            });
        });
    });
    ui.add_space(4.0);
}

fn small_line(ui: &mut Ui, t: &Tokens, icon: Icon, words: &str, warn: bool) {
    let color = if warn { t.danger } else { t.text_dim };
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(13.0, 13.0), Sense::hover());
        icons::paint(ui.painter(), r, icon, color);
        ui.add(egui::Label::new(egui::RichText::new(one_line(words)).font(font(12.0)).color(color)).wrap());
    });
}

/// What the import brought and what it could not; true when accounts came that want passwords.
fn shown_result(ui: &mut Ui, t: &Tokens, result: &Value) -> bool {
    let accounts: Vec<&Value> = result["accounts"].as_array().into_iter().flatten().collect();
    let files: Vec<&Value> = result["dataFiles"].as_array().into_iter().flatten().collect();
    let failed: Vec<&Value> = result["failed"].as_array().into_iter().flatten().collect();
    let signatures = result["signatures"].as_u64().unwrap_or(0);
    if !accounts.is_empty() || !files.is_empty() || signatures > 0 {
        dialog::callout(ui, t, t.ok, |ui| {
            if signatures > 0 {
                let what = if signatures == 1 { "1 signature was brought".to_string() } else { format!("{signatures} signatures were brought") };
                small_line(ui, t, Icon::Check, &what, false);
            }
            for a in &accounts {
                small_line(ui, t, Icon::Check, &format!("{} ({}) was added", text(&a["name"]), text(&a["email"])), false);
            }
            for f in &files {
                small_line(ui, t, Icon::Check, &format!("{} was opened: {}", text(&f["name"]), text(&f["path"])), false);
            }
            if !accounts.is_empty() {
                ui.label(
                    egui::RichText::new("Give each account its password in Account Settings; Done takes you there.").font(bold(12.5)).color(t.text),
                );
            }
        });
    }
    if !failed.is_empty() {
        ui.add_space(4.0);
        dialog::callout(ui, t, t.danger, |ui| {
            for f in &failed {
                small_line(ui, t, Icon::Importance, &format!("{}: {}", text(&f["name"]), text(&f["why"])), true);
            }
        });
    }
    !accounts.is_empty()
}
