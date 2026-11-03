//! The Signatures page of Options: signatures made, changed, renamed and deleted, and the one
//! each account puts in new messages and in replies and forwards, or the one every account
//! takes. Every change goes through its `signatures.*` command.
//!
//! A signature is written as a message is, with its fonts, colours, links and pictures, and the
//! Format Text bar above it (`crate::writing`); it is kept as HTML with its pictures written
//! into it, or as text alone where it has no look.

use crate::MailUi;
use crate::line_edit::LineEdit;
use egui::Ui;
use openmailclient_engine::signatures::{self, MAX_NAME, Signature};
use openmailclient_engine::{Writing, write_formatted};
use serde_json::{Value, json};

/// The signature being written on the page.
#[derive(Default)]
pub(crate) struct SignatureEdit {
    /// The name it had when it was opened; none for a new one.
    was: Option<String>,
    name: String,
    /// What is written, with its look.
    pub(crate) w: Writing,
    /// A save asked for and not yet looked at: the name it was saved under, and the text it
    /// reads as.
    saving: Option<(String, String)>,
}

impl SignatureEdit {
    pub(crate) fn of(g: &Signature) -> SignatureEdit {
        let w = match &g.html {
            Some(html) => {
                let (text, format) = openmailclient_formats::editable::of_html(html, &|_| None);
                Writing::new(text, format)
            }
            None => Writing::new(g.text.clone(), Default::default()),
        };
        SignatureEdit { was: Some(g.name.clone()), name: g.name.clone(), w, saving: None }
    }
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
    /// What was done to the signature being written, done to it.
    pub(crate) fn signature_writing(&mut self, did: Vec<crate::writing::Did>) {
        let w = &mut self.signature_edit.w;
        for d in did {
            match d {
                crate::writing::Did::Body(text, at) => {
                    let text: String = text.chars().take(signatures::MAX_TEXT).collect();
                    w.rewrite(text, at);
                }
                crate::writing::Did::Run(command, p) => {
                    if let Err(e) = write_formatted(w, command, &p) {
                        self.session.status = e.to_string();
                    }
                }
                crate::writing::Did::PictureFile => {
                    self.picture_for_signature = true;
                    self.requests.push(crate::UiRequest::InsertPicture);
                }
            }
        }
    }

    /// `ui.insertPicture` for the signature being written: the picture put in where its caret is.
    pub(crate) fn signature_picture(&mut self, p: &Value) -> Result<Value, String> {
        write_formatted(&mut self.signature_edit.w, "compose.insertPicture", p).map_err(|e| e.to_string())
    }

    pub(crate) fn signatures_page(&mut self, ui: &mut Ui) {
        let t = self.tokens.clone();
        // A save asked for last frame: kept, it is the one written on now; refused, it stays new.
        if let Some((name, kept)) = self.signature_edit.saving.take()
            && let Some(g) = self.session.signatures.get(&name).filter(|g| g.text == kept)
        {
            self.signature_edit.was = Some(g.name.clone());
            self.signature_edit.name = g.name.clone();
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
                            self.signature_edit = SignatureEdit::of(g);
                        }
                    }
                });
                if ui.add(crate::dialog::plain("New")).clicked() {
                    self.signature_edit = SignatureEdit::default();
                }
            });
            let mut did = Vec::new();
            ui.vertical(|ui| {
                let e = &mut self.signature_edit;
                ui.add(
                    LineEdit::new(&mut e.name, egui::Id::new("signature-name")).desired_width(f32::INFINITY).char_limit(MAX_NAME).hint("Name", None),
                );
                ui.add_space(4.0);
                egui::Frame::new().stroke(egui::Stroke::new(1.0, t.line)).fill(t.surface).inner_margin(6.0).corner_radius(3.0).show(ui, |ui| {
                    did = crate::writing::writing_box(
                        ui,
                        &t,
                        &mut self.faces,
                        &self.inline,
                        &e.w,
                        egui::Id::new("signature-text"),
                        6,
                        signatures::MAX_TEXT,
                    );
                });
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
                        let kept = Signature::of(name.clone(), &e.w.body, &e.w.format);
                        let mut p = match (&kept.html, e.w.plain) {
                            (Some(html), false) => json!({ "name": name, "html": html }),
                            _ => json!({ "name": name, "text": kept.text }),
                        };
                        if let Some(was) = &e.was {
                            p["was"] = json!(was);
                        }
                        self.signature_edit.saving = Some((name, kept.text));
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
            self.signature_writing(did);
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
