//! The Mail Merge window (`people.mailMerge`): a subject and a text with fields, written to the
//! contacts shown or the one chosen, looked at for the first few before anything is made, and
//! kept in Drafts or put in the Outbox.
//!
//! Beyond Outlook, which hands a merge to Word: the fields are put in from a list where the
//! cursor is, the first messages are shown as they will be while the text is written, and a
//! wrong field is said at once. What a preview shows comes from contacts that may be a
//! stranger's card: drawn, never acted on.

use crate::icons::Icon;
use crate::{MailUi, bold, font, one_line};
use serde_json::{Value, json};

/// What the Mail Merge window holds while it is open.
#[derive(Clone, Debug)]
pub(crate) struct Merge {
    pub subject: String,
    pub body: String,
    /// "shown" or "selected".
    pub who: &'static str,
    /// "drafts" or "outbox".
    pub deliver: &'static str,
    /// The words and fields the previews are of, and what they found.
    pub looked: Option<(String, String, &'static str)>,
    pub previews: Vec<(String, String, String)>,
    pub count: usize,
    pub skipped: usize,
    pub fields: Vec<String>,
    pub error: String,
}

impl Default for Merge {
    fn default() -> Self {
        Self {
            subject: String::new(),
            body: "Dear «First Name»,\n\n".into(),
            who: "shown",
            deliver: "drafts",
            looked: None,
            previews: Vec::new(),
            count: 0,
            skipped: 0,
            fields: Vec::new(),
            error: String::new(),
        }
    }
}

impl Merge {
    fn params(&self, deliver: &str) -> Value {
        json!({ "subject": self.subject, "body": self.body, "contacts": self.who, "deliver": deliver })
    }
}

impl MailUi {
    fn merge_preview(&mut self) {
        let Some(m) = self.merge.as_ref() else { return };
        let key = (m.subject.clone(), m.body.clone(), m.who);
        if m.looked.as_ref() == Some(&key) {
            return;
        }
        let got = self.session.run("people.mailMerge", &m.params("preview"));
        let Some(m) = self.merge.as_mut() else { return };
        m.looked = Some(key);
        match got {
            Ok(v) => {
                let text = |p: &Value, k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                m.previews = v["untrusted_content"]["previews"]
                    .as_array()
                    .map(|a| a.iter().map(|p| (one_line(&text(p, "to")), one_line(&text(p, "subject")), text(p, "body"))).collect())
                    .unwrap_or_default();
                m.count = v["to"].as_array().map_or(0, Vec::len);
                m.skipped = v["skipped"].as_array().map_or(0, Vec::len);
                m.fields = v["fields"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
                m.error.clear();
            }
            Err(e) => {
                m.previews.clear();
                m.count = 0;
                m.error = e.to_string();
            }
        }
    }

    pub(crate) fn mail_merge_window(&mut self, ctx: &egui::Context) {
        if self.merge.is_none() {
            return;
        }
        self.merge_preview();
        let t = self.tokens.clone();
        let Some(mut m) = self.merge.clone() else { return };
        let mut run = false;
        let mut close = false;
        let shown = crate::dialog::Dialog::new("mail-merge", "Mail Merge", Icon::Mail).width(620.0).show(ctx, &t, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("To").font(font(12.5)).color(t.text_dim));
                ui.radio_value(&mut m.who, "shown", "Everyone shown");
                ui.radio_value(&mut m.who, "selected", "The one chosen");
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Subject").font(font(12.5)).color(t.text_dim));
                ui.add(crate::line_edit::LineEdit::new(&mut m.subject, egui::Id::new("merge-subject")).font(font(13.0)).char_limit(1_000));
            });
            let body_id = egui::Id::new("merge-body");
            ui.add(egui::TextEdit::multiline(&mut m.body).id(body_id).desired_rows(6).desired_width(f32::INFINITY).char_limit(1_000_000));
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Insert field").font(font(12.0)).color(t.text_dim));
                for f in m.fields.clone() {
                    if ui.small_button(&f).clicked() {
                        // Where the cursor is in the text, else at its end.
                        let at = egui::TextEdit::load_state(ui.ctx(), body_id)
                            .and_then(|s| s.cursor.char_range())
                            .map_or(m.body.chars().count(), |r| r.primary.index.into());
                        let byte = m.body.char_indices().nth(at).map_or(m.body.len(), |(b, _)| b);
                        m.body.insert_str(byte, &format!("«{f}»"));
                    }
                }
            });
            ui.add_space(6.0);
            if m.error.is_empty() {
                let words = format!(
                    "{} message{} will be written{}",
                    m.count,
                    if m.count == 1 { "" } else { "s" },
                    if m.skipped > 0 { format!("; {} without an e-mail address left out", m.skipped) } else { String::new() }
                );
                ui.label(egui::RichText::new(words).font(bold(12.5)).color(t.text));
                egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (to, subject, body) in &m.previews {
                        crate::dialog::card(ui, &t, |ui| {
                            ui.label(egui::RichText::new(format!("To: {to}")).font(font(12.0)).color(t.text_dim));
                            if !subject.trim().is_empty() {
                                ui.label(egui::RichText::new(subject).font(bold(12.5)).color(t.text));
                            }
                            let short: String = body.trim_end().chars().take(400).collect();
                            ui.label(egui::RichText::new(short).font(font(12.5)).color(t.text));
                        });
                    }
                });
            } else {
                crate::dialog::callout(ui, &t, t.danger, |ui| {
                    ui.label(egui::RichText::new(&m.error).font(font(12.5)).color(t.text));
                });
            }
            crate::dialog::foot(ui, &t, |ui| {
                ui.radio_value(&mut m.deliver, "drafts", "Keep in Drafts");
                ui.radio_value(&mut m.deliver, "outbox", "Put in the Outbox");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(crate::dialog::plain("Cancel")).clicked() {
                        close = true;
                    }
                    if ui.add_enabled(m.error.is_empty() && m.count > 0, crate::dialog::primary(&t, "Write Messages")).clicked() {
                        run = true;
                    }
                });
            });
        });
        let params = m.params(m.deliver);
        self.merge = Some(m);
        if run {
            match self.session.run("people.mailMerge", &params) {
                Ok(_) => close = true,
                Err(e) => {
                    if let Some(m) = self.merge.as_mut() {
                        m.error = e.to_string();
                    }
                }
            }
        }
        if shown.closed || close {
            self.merge = None;
        }
    }
}
