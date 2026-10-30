//! The Signatures page of Options: signatures made, changed, renamed and deleted, and the one
//! each account puts in new messages and in replies and forwards, or the one every account
//! takes. Every change goes through its `signatures.*` command.

use crate::MailUi;
use crate::line_edit::LineEdit;
use egui::Ui;
use openmailclient_engine::signatures::{self, MAX_NAME};
use serde_json::{Value, json};

/// The signature being written on the page.
#[derive(Default)]
pub(crate) struct SignatureEdit {
    /// The name it had when it was opened; none for a new one.
    was: Option<String>,
    name: String,
    text: String,
    /// A save asked for and not yet looked at: the name it was saved under.
    saving: Option<String>,
}

/// What a choice box can be set to.
#[derive(Clone, PartialEq)]
enum Pick {
    /// The account takes what every account takes.
    AsAll,
    Nothing,
    Named(String),
}

impl Pick {
    fn of(name: &Option<String>) -> Pick {
        name.clone().map_or(Pick::Nothing, Pick::Named)
    }

    fn shown(&self) -> &str {
        match self {
            Pick::AsAll => "As every account",
            Pick::Nothing => "(none)",
            Pick::Named(n) => n,
        }
    }

    fn value(&self) -> Value {
        match self {
            Pick::Named(n) => json!(n),
            _ => Value::Null,
        }
    }
}

impl MailUi {
    pub(crate) fn signatures_page(&mut self, ui: &mut Ui) {
        let t = self.tokens.clone();
        // A save asked for last frame: kept, it is the one written on now; refused, it stays new.
        if let Some(name) = self.signature_edit.saving.take() {
            let kept = signatures::clean_text(&self.signature_edit.text);
            if let Some(g) = self.session.signatures.get(&name).filter(|g| g.text == kept) {
                self.signature_edit.was = Some(g.name.clone());
                self.signature_edit.name = g.name.clone();
            }
        }
        let names: Vec<String> = self.session.signatures.list.iter().map(|g| g.name.clone()).collect();
        crate::dialog::heading(ui, &t, "Signatures");
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(150.0);
                egui::ScrollArea::vertical().id_salt("signature-names").max_height(170.0).show(ui, |ui| {
                    if names.is_empty() {
                        ui.label(egui::RichText::new("None yet").color(t.text_faint));
                    }
                    for n in &names {
                        let on = self.signature_edit.was.as_deref() == Some(n.as_str());
                        if ui.selectable_label(on, n).clicked()
                            && let Some(g) = self.session.signatures.get(n)
                        {
                            self.signature_edit =
                                SignatureEdit { was: Some(g.name.clone()), name: g.name.clone(), text: g.text.clone(), saving: None };
                        }
                    }
                });
                if ui.add(crate::dialog::plain("New")).clicked() {
                    self.signature_edit = SignatureEdit::default();
                }
            });
            ui.vertical(|ui| {
                let e = &mut self.signature_edit;
                ui.add(
                    LineEdit::new(&mut e.name, egui::Id::new("signature-name")).desired_width(f32::INFINITY).char_limit(MAX_NAME).hint("Name", None),
                );
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::multiline(&mut e.text)
                        .id_salt("signature-text")
                        .desired_rows(6)
                        .desired_width(f32::INFINITY)
                        .char_limit(signatures::MAX_TEXT)
                        .hint_text("Regards,\n{name}\n{email}"),
                );
                ui.label(
                    egui::RichText::new("{name} and {email} are filled with those of the account a message goes from.")
                        .font(crate::font(11.5))
                        .color(t.text_dim),
                );
                ui.horizontal(|ui| {
                    let named = signatures::clean_name(&self.signature_edit.name).is_some();
                    if ui.add_enabled(named, crate::dialog::plain("Save")).clicked() {
                        let e = &self.signature_edit;
                        let name = signatures::clean_name(&e.name).unwrap_or_default();
                        let mut p = json!({ "name": name, "text": e.text });
                        if let Some(was) = &e.was {
                            p["was"] = json!(was);
                        }
                        self.signature_edit.saving = Some(name);
                        self.act("signatures.save", p);
                    }
                    if let Some(was) = self.signature_edit.was.clone()
                        && ui.add(crate::dialog::plain("Delete")).clicked()
                    {
                        self.act("signatures.delete", json!({ "name": was }));
                        self.signature_edit = SignatureEdit::default();
                    }
                });
            });
        });
        ui.add_space(8.0);
        crate::dialog::heading(ui, &t, "Put in messages");
        let sigs = &self.session.signatures;
        let mut rows: Vec<(String, String, Pick, Pick)> =
            vec![(String::new(), "Every account".to_string(), Pick::of(&sigs.default.new), Pick::of(&sigs.default.replies))];
        for a in &self.session.accounts {
            match sigs.by_account.iter().find(|(id, _)| *id == a.id) {
                Some((_, c)) => rows.push((a.id.clone(), a.name.clone(), Pick::of(&c.new), Pick::of(&c.replies))),
                None => rows.push((a.id.clone(), a.name.clone(), Pick::AsAll, Pick::AsAll)),
            }
        }
        let mut chosen: Option<Value> = None;
        egui::Grid::new("signature-choices").num_columns(3).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(egui::RichText::new("Account").color(t.text_faint));
            ui.label(egui::RichText::new("New messages").color(t.text_faint));
            ui.label(egui::RichText::new("Replies and forwards").color(t.text_faint));
            ui.end_row();
            for (id, label, new, replies) in &rows {
                ui.scope(|ui| {
                    ui.set_width(130.0);
                    ui.add(egui::Label::new(egui::RichText::new(label).color(t.text)).truncate());
                });
                for (key, now) in [("new", new), ("replies", replies)] {
                    let mut picks = Vec::new();
                    if !id.is_empty() {
                        picks.push(Pick::AsAll);
                    }
                    picks.push(Pick::Nothing);
                    picks.extend(names.iter().cloned().map(Pick::Named));
                    egui::ComboBox::from_id_salt(("signature-choice", id.as_str(), key)).selected_text(now.shown()).width(150.0).show_ui(ui, |ui| {
                        for p in picks {
                            if ui.selectable_label(p == *now, p.shown()).clicked() && p != *now {
                                chosen = Some(match p {
                                    Pick::AsAll => json!({ "account": id, "own": false }),
                                    _ => json!({ "account": id, key: p.value() }),
                                });
                            }
                        }
                    });
                }
                ui.end_row();
            }
        });
        if let Some(p) = chosen {
            self.act("signatures.choose", p);
        }
    }
}
