//! The Address Book window (`people.addressBook`): everyone of every Contacts folder open,
//! searched as it is typed, and put in the To, Cc or Bcc line of the message being written, or
//! in a new one. Names suggested under an address line while it is typed (`people.suggest`).
//!
//! Beyond Outlook: the search finds any word of a name, an address or a company as it is
//! typed, with no Go button and no choice of "name only"; a group shows who is in it; the
//! window says which data file each book is of. Names and addresses may come from a stranger's
//! card: drawn, never acted on.

use crate::icons::Icon;
use crate::{MailUi, bold, font, one_line};
use egui::{Rect, Sense, Stroke, StrokeKind, vec2};
use serde_json::{Value, json};

const ROW_H: f32 = 30.0;
/// Rows shown before the list scrolls.
const ROWS: f32 = 12.0;

/// One entry, as `people.addressBook` and `people.suggest` give it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Row {
    pub name: String,
    pub address: String,
    pub company: String,
    pub group: bool,
    /// A group's members, as mailboxes.
    pub members: Vec<String>,
    /// What it is put in an address line as.
    pub as_: String,
}

impl Row {
    fn of(v: &Value) -> Self {
        let text = |k: &str| v.get(k).and_then(Value::as_str).map(one_line).unwrap_or_default();
        let members: Vec<String> =
            v.get("members").and_then(Value::as_array).map(|m| m.iter().filter_map(Value::as_str).map(one_line).collect()).unwrap_or_default();
        let group = v.get("kind").and_then(Value::as_str) == Some("group");
        let as_ = match v.get("as").and_then(Value::as_str) {
            Some(a) => one_line(a),
            None if group => members.join("; "),
            None => mailbox(&text("name"), &text("address")),
        };
        Self { name: text("name"), address: text("address"), company: text("company"), group, members, as_ }
    }
}

/// `name <address>`, quoted where the name needs it, as the engine writes it.
fn mailbox(name: &str, address: &str) -> String {
    let name = name.trim().replace(['"', '\\'], "");
    if name.is_empty() || name.eq_ignore_ascii_case(address) {
        address.to_string()
    } else if name.chars().any(|c| ",;<>@()[]:.".contains(c)) {
        format!("\"{name}\" <{address}>")
    } else {
        format!("{name} <{address}>")
    }
}

/// What the Address Book window shows: what is looked for and what it found.
#[derive(Clone, Debug, Default)]
pub(crate) struct Book {
    pub query: String,
    /// The query the rows are of; `None` before the first look.
    pub looked: Option<String>,
    pub rows: Vec<Row>,
    pub total: usize,
    /// Each book: its name and data file, and how many it has.
    pub books: Vec<String>,
    pub pick: Option<usize>,
}

/// Names suggested under one address line of the message being written.
#[derive(Clone, Debug, Default)]
pub(crate) struct Suggest {
    /// "to", "cc" or "bcc".
    pub field: String,
    /// The line they are for, as it was typed.
    pub line: String,
    pub rows: Vec<Row>,
    pub pick: usize,
}

/// `line` with `add` after what it has, one piece of it.
pub(crate) fn added(line: &str, add: &str) -> String {
    let kept = line.trim().trim_end_matches([';', ',', ' ']);
    if kept.is_empty() { format!("{add}; ") } else { format!("{kept}; {add}; ") }
}

/// `line` with what follows its last comma or semicolon (outside quotes and angles) made `add`.
pub(crate) fn completed(line: &str, add: &str) -> String {
    let (mut quoted, mut angle, mut at) = (false, 0usize, 0usize);
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '<' if !quoted => angle = angle.saturating_add(1),
            '>' if !quoted => angle = angle.saturating_sub(1),
            ',' | ';' if !quoted && angle == 0 => at = i + c.len_utf8(),
            _ => {}
        }
    }
    added(line.get(..at).unwrap_or(""), add)
}

impl MailUi {
    /// The names suggested for what was just typed in `field`, read again when it changed.
    pub(crate) fn suggest_for(&mut self, field: &str, line: &str) {
        if self.suggest.as_ref().is_some_and(|s| s.field == field && s.line == line) {
            return;
        }
        let rows: Vec<Row> = match self.session.run("people.suggest", &json!({ "text": line, "limit": 8 })) {
            Ok(v) => v["untrusted_content"]["suggestions"].as_array().map(|a| a.iter().map(Row::of).collect()).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        self.suggest = Some(Suggest { field: field.to_string(), line: line.to_string(), rows, pick: 0 });
    }

    fn look(&mut self) {
        let query = self.book.query.clone();
        match self.session.run("people.addressBook", &json!({ "query": query, "open": false, "limit": 2000 })) {
            Ok(v) => {
                self.book.rows = v["untrusted_content"]["entries"].as_array().map(|a| a.iter().map(Row::of).collect()).unwrap_or_default();
                self.book.total = v["total"].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
                self.book.books = v["books"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|b| {
                                let name = b["name"].as_str().map(one_line).unwrap_or_default();
                                let file = b["file"].as_str().map(one_line).unwrap_or_default();
                                format!("{name} ({file}): {}", b["entries"].as_u64().unwrap_or(0))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            }
            Err(e) => {
                self.book.rows.clear();
                self.book.total = 0;
                self.session.status = e.to_string();
            }
        }
        self.book.pick = (!self.book.rows.is_empty()).then_some(0);
        self.book.looked = Some(query);
    }

    /// `row` put in the line `field` of the message being written; a new one when none is.
    pub(crate) fn put_in(&mut self, field: &str, row: &Row) {
        if self.session.compose.is_none() && self.session.run("mail.new", &Value::Null).is_err() {
            return;
        }
        let Some(d) = self.session.compose.as_ref() else { return };
        let line = match field {
            "cc" => d.cc.clone(),
            "bcc" => d.bcc.clone(),
            _ => d.to.clone(),
        };
        let field = if matches!(field, "cc" | "bcc") { field } else { "to" };
        if let Err(e) = self.session.run("compose.setField", &json!({ "field": field, "value": added(&line, &row.as_) })) {
            self.session.status = e.to_string();
        }
    }

    pub(crate) fn address_book_window(&mut self, ctx: &egui::Context) {
        let Some(target) = self.session.people.book.clone() else {
            self.book.looked = None;
            return;
        };
        if self.book.looked.as_deref() != Some(self.book.query.as_str()) {
            self.look();
        }
        let t = self.tokens.clone();
        let writing = self.session.compose.is_some();
        let mut query = self.book.query.clone();
        let rows = self.book.rows.clone();
        let mut pick = self.book.pick;
        let mut put: Option<(&'static str, usize)> = None;
        let mut close = false;
        let default_field: &'static str = match target.as_str() {
            "cc" => "cc",
            "bcc" => "bcc",
            _ => "to",
        };
        let shown = crate::dialog::Dialog::new("address-book", "Address Book", Icon::People).width(560.0).show(ctx, &t, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Search").font(font(12.5)).color(t.text_dim));
                let edit = crate::line_edit::LineEdit::new(&mut query, egui::Id::new("address-book-query"))
                    .font(font(13.0))
                    .hint("a name, an address or a company", None)
                    .char_limit(200);
                ui.add(edit);
            });
            for b in &self.book.books {
                ui.label(egui::RichText::new(b).font(font(11.5)).color(t.text_dim));
            }
            ui.add_space(6.0);
            let width = ui.available_width();
            let (head, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(head, 0.0, t.pane);
            p.text(head.left_center() + vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, "Name", bold(12.0), t.text_dim);
            p.text(head.left_center() + vec2(width * 0.42, 0.0), egui::Align2::LEFT_CENTER, "Email Address", bold(12.0), t.text_dim);
            p.text(head.left_center() + vec2(width * 0.78, 0.0), egui::Align2::LEFT_CENTER, "Company", bold(12.0), t.text_dim);
            egui::ScrollArea::vertical().max_height(ROW_H * ROWS).auto_shrink([false, true]).show(ui, |ui| {
                if rows.is_empty() {
                    let words = if query.trim().is_empty() { "No one is in the address book." } else { "No one is found." };
                    ui.label(egui::RichText::new(words).font(font(12.5)).color(t.text_dim));
                }
                for (i, row) in rows.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
                    let painter = ui.painter_at(rect);
                    if pick == Some(i) {
                        painter.rect_filled(rect, 3.0, t.selected);
                        painter.rect_stroke(rect, 3.0, Stroke::new(1.0, t.accent), StrokeKind::Inside);
                    } else if resp.hovered() {
                        painter.rect_filled(rect, 3.0, t.hover);
                    }
                    let cell = |x0: f32, x1: f32, text: &str, f: egui::FontId, color| {
                        let room = Rect::from_min_max(rect.min + vec2(x0, 0.0), rect.min + vec2(x1, ROW_H));
                        let mut job = crate::seen_line(text, f, color);
                        job.wrap = egui::text::TextWrapping::truncate_at_width((room.width() - 8.0).max(1.0));
                        let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
                        painter.galley(room.left_center() - vec2(0.0, galley.size().y / 2.0), galley, color);
                    };
                    let name = if row.group { format!("{} (group)", row.name) } else { row.name.clone() };
                    cell(10.0, width * 0.42, &name, if row.group { bold(13.0) } else { font(13.0) }, t.text);
                    let address = if row.group { format!("{} members", row.members.len()) } else { row.address.clone() };
                    cell(width * 0.42, width * 0.78, &address, font(12.5), t.text_dim);
                    cell(width * 0.78, width, &row.company, font(12.5), t.text_dim);
                    let resp = if row.group { resp.on_hover_text(row.members.join("\n")) } else { resp };
                    if resp.clicked() {
                        pick = Some(i);
                    }
                    if resp.double_clicked() {
                        put = Some((default_field, i));
                    }
                }
            });
            if self.book.total > rows.len() {
                ui.label(
                    egui::RichText::new(format!("{} of {} shown; search to find the others.", rows.len(), self.book.total))
                        .font(font(11.5))
                        .color(t.text_dim),
                );
            }
            crate::dialog::foot(ui, &t, |ui| {
                if writing {
                    for (label, field) in [("To →", "to"), ("Cc →", "cc"), ("Bcc →", "bcc")] {
                        if ui.add_enabled(pick.is_some(), crate::dialog::plain(label)).clicked()
                            && let Some(i) = pick
                        {
                            put = Some((field, i));
                        }
                    }
                } else if ui.add_enabled(pick.is_some(), crate::dialog::plain("New Email")).clicked()
                    && let Some(i) = pick
                {
                    put = Some(("to", i));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(crate::dialog::primary(&t, "Close")).clicked() {
                        close = true;
                    }
                });
            });
        });
        self.book.query = query;
        self.book.pick = pick;
        if let Some((field, i)) = put
            && let Some(row) = rows.get(i)
        {
            let new = self.session.compose.is_none();
            self.put_in(field, row);
            // A new message is written in at once, as Outlook's New Message to does.
            if new {
                close = true;
            }
        }
        if shown.closed || close {
            self.session.people.book = None;
        }
    }
}
