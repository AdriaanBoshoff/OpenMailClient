//! The Mail Merge window (`people.mailMerge`): a subject and a text with fields, written to the
//! contacts shown or the one chosen, looked at for the first few before anything is made, and
//! kept in Drafts, put in the Outbox, or printed as letters and saved as one PDF document.
//!
//! Beyond Outlook, which hands a merge to Word: the fields are put in from a list where the
//! cursor is, the first messages or letters are shown as they will be while the text is
//! written, a wrong field is said at once, and the contacts are chosen by a filter of their
//! fields here, with how many it leaves out. What a preview shows comes from contacts that may
//! be a stranger's card: drawn, never acted on.

use crate::icons::Icon;
use crate::{MailUi, UiRequest, bold, font, one_line};
use serde_json::{Value, json};

/// Conditions the window's filter may hold, as the engine takes them.
const MAX_CONDITIONS: usize = 20;

/// How a condition compares, as the engine names it, with the words the window shows.
const OPS: [(&str, &str); 7] = [
    ("is", "is"),
    ("isNot", "is not"),
    ("contains", "contains"),
    ("notContains", "does not contain"),
    ("startsWith", "starts with"),
    ("empty", "is empty"),
    ("notEmpty", "is not empty"),
];

/// One condition of the filter: a field, how it compares, and with what.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Condition {
    pub field: String,
    pub op: &'static str,
    pub value: String,
}

/// What the Mail Merge window holds while it is open.
#[derive(Clone, Debug)]
pub(crate) struct Merge {
    pub subject: String,
    pub body: String,
    /// "shown" or "selected".
    pub who: &'static str,
    /// "drafts", "outbox" or "letters".
    pub deliver: &'static str,
    pub filter: Vec<Condition>,
    /// A contact meeting any condition is merged, rather than one meeting all.
    pub any: bool,
    /// The parameters the previews are of.
    pub looked: Option<String>,
    pub previews: Vec<(String, String, String)>,
    pub count: usize,
    pub skipped: usize,
    pub filtered: usize,
    /// Letters for those without a postal address.
    pub no_address: usize,
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
            filter: Vec::new(),
            any: false,
            looked: None,
            previews: Vec::new(),
            count: 0,
            skipped: 0,
            filtered: 0,
            no_address: 0,
            fields: Vec::new(),
            error: String::new(),
        }
    }
}

impl Merge {
    /// The parameters of a merge delivered as `deliver`; letters are looked at, not laid out.
    pub(crate) fn params(&self, deliver: &str) -> Value {
        let filter: Vec<Value> = self.filter.iter().map(|c| json!({ "field": c.field, "op": c.op, "value": c.value })).collect();
        let mut p = json!({ "subject": self.subject, "body": self.body, "contacts": self.who, "filter": filter, "filterAny": self.any });
        if deliver == "preview" && self.deliver == "letters" {
            p["deliver"] = json!("letters");
            p["preview"] = json!(true);
        } else {
            p["deliver"] = json!(deliver);
        }
        p
    }
}

impl MailUi {
    fn merge_preview(&mut self) {
        let Some(m) = self.merge.as_ref() else { return };
        let params = m.params("preview");
        let key = params.to_string();
        if m.looked.as_ref() == Some(&key) {
            return;
        }
        let got = self.session.run("people.mailMerge", &params);
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
                m.filtered = v["filtered"].as_u64().map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX));
                m.no_address = v["noAddress"].as_array().map_or(0, Vec::len);
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

    /// `ui.mergeLetters`: the letters of the merge open in its window saved as one PDF document
    /// (`{"path"}`) or printed (`{"printer"}`); the window closes once they are.
    pub(crate) fn merge_letters(&mut self, params: &Value) -> Result<Value, String> {
        let m = self.merge.as_ref().ok_or("Mail Merge is not open")?;
        let mut p = m.params("letters");
        match (params.get("path").and_then(Value::as_str), params.get("printer").and_then(Value::as_str)) {
            (Some(path), _) => p["path"] = json!(path),
            (None, Some(printer)) => {
                p["printer"] = json!(printer);
                p["wait"] = json!(false);
            }
            (None, None) => return Err("\"path\" (a .pdf file) or \"printer\" is required".to_string()),
        }
        match self.session.run("people.mailMerge", &p) {
            Ok(v) => {
                self.merge = None;
                Ok(v)
            }
            Err(e) => {
                let said = e.to_string();
                if let Some(m) = self.merge.as_mut() {
                    m.error = said.clone();
                }
                Err(said)
            }
        }
    }

    /// The filter's conditions, each a row: a field, how it compares, and a value.
    fn merge_filter(ui: &mut egui::Ui, t: &crate::theme::Tokens, m: &mut Merge) {
        let mut gone = None;
        for (at, c) in m.filter.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(("merge-field", at)).width(150.0).selected_text(c.field.as_str()).show_ui(ui, |ui| {
                    for f in &m.fields {
                        ui.selectable_value(&mut c.field, f.clone(), f.as_str());
                    }
                });
                let shown = OPS.iter().find(|(id, _)| *id == c.op).map_or("is", |(_, w)| *w);
                egui::ComboBox::from_id_salt(("merge-op", at)).width(120.0).selected_text(shown).show_ui(ui, |ui| {
                    for (id, words) in OPS {
                        ui.selectable_value(&mut c.op, id, words);
                    }
                });
                if !matches!(c.op, "empty" | "notEmpty") {
                    ui.add(crate::line_edit::LineEdit::new(&mut c.value, egui::Id::new(("merge-value", at))).desired_width(150.0).char_limit(500));
                }
                if ui.add(crate::dialog::plain("Remove")).clicked() {
                    gone = Some(at);
                }
            });
        }
        if let Some(at) = gone {
            m.filter.remove(at);
        }
        ui.horizontal(|ui| {
            if m.filter.len() < MAX_CONDITIONS
                && ui.add(crate::dialog::plain("Add Condition")).on_hover_text("Merge only the contacts whose fields say so").clicked()
            {
                m.filter.push(Condition { field: "City".to_string(), op: "is", value: String::new() });
            }
            if m.filter.len() > 1 {
                ui.label(egui::RichText::new("Merge those meeting").font(font(12.0)).color(t.text_dim));
                ui.radio_value(&mut m.any, false, "every condition");
                ui.radio_value(&mut m.any, true, "any one");
            }
        });
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
        let mut save_letters = false;
        let mut print_letters = false;
        let letters = m.deliver == "letters";
        let shown = crate::dialog::Dialog::new("mail-merge", "Mail Merge", Icon::Mail).width(660.0).show(ctx, &t, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("To").font(font(12.5)).color(t.text_dim));
                ui.radio_value(&mut m.who, "shown", "Everyone shown");
                ui.radio_value(&mut m.who, "selected", "The one chosen");
            });
            Self::merge_filter(ui, &t, &mut m);
            if !letters {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Subject").font(font(12.5)).color(t.text_dim));
                    ui.add(crate::line_edit::LineEdit::new(&mut m.subject, egui::Id::new("merge-subject")).font(font(13.0)).char_limit(1_000));
                });
            }
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
                let what = if letters { "letter" } else { "message" };
                let mut words =
                    format!("{} {what}{} will be {}", m.count, if m.count == 1 { "" } else { "s" }, if letters { "printed" } else { "written" });
                if m.filtered > 0 {
                    words.push_str(&format!("; {} left out by the filter", m.filtered));
                }
                if m.skipped > 0 && !letters {
                    words.push_str(&format!("; {} without an e-mail address left out", m.skipped));
                }
                if m.no_address > 0 && letters {
                    words.push_str(&format!("; {} without a postal address", m.no_address));
                }
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
                ui.radio_value(&mut m.deliver, "letters", "Letters");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(crate::dialog::plain("Cancel")).clicked() {
                        close = true;
                    }
                    let ready = m.error.is_empty() && m.count > 0;
                    if letters {
                        if ui.add_enabled(ready, crate::dialog::plain("Print")).on_hover_text("On the printer chosen in Print").clicked() {
                            print_letters = true;
                        }
                        if ui.add_enabled(ready, crate::dialog::primary(&t, "Save as PDF…")).clicked() {
                            save_letters = true;
                        }
                    } else if ui.add_enabled(ready, crate::dialog::primary(&t, "Write Messages")).clicked() {
                        run = true;
                    }
                });
            });
        });
        let params = m.params(m.deliver);
        self.merge = Some(m);
        let mut ask: Option<(&str, Value)> = None;
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
        if save_letters {
            ask = Some(("ui.mergeLettersDialog", Value::Null));
        }
        if print_letters {
            let printer = self.session.run("file.printers", &json!({})).ok().and_then(|v| v["printer"].as_str().map(str::to_string));
            match printer {
                Some(printer) => ask = Some(("ui.mergeLetters", json!({ "printer": printer }))),
                None => {
                    if let Some(m) = self.merge.as_mut() {
                        m.error = "There is no printer on this system to print on".to_string();
                    }
                }
            }
        }
        if let Some((id, params)) = ask
            && let Err(e) = self.execute(id, &params)
            && let Some(m) = self.merge.as_mut()
        {
            m.error = e;
        }
        if shown.closed || close {
            self.merge = None;
        }
    }

    /// `ui.mergeLettersDialog`: ask where the letters of the open merge are saved.
    pub(crate) fn merge_letters_dialog(&mut self) -> Result<(), String> {
        if self.merge.is_none() {
            return Err("Mail Merge is not open".to_string());
        }
        self.requests.push(UiRequest::SaveMergeLetters);
        Ok(())
    }
}
