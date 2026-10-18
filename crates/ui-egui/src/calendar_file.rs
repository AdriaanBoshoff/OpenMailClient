//! Open Calendar: what a calendar file holds shown before anything is put in the calendar, each
//! event chosen or left, then added by `calendar.import`.
//!
//! Better than Outlook's, which adds a file's events without showing them, or opens the file as
//! a calendar of its own beside yours: here each event is shown with when it is, how it repeats
//! and what of it could not be kept, an event the calendar has already is said to be brought up
//! to date rather than added twice, and the whole of it is taken back by one Undo.

use crate::dialog::{self, Dialog};
use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::{MailUi, bold, font, one_line};
use egui::{Sense, Ui, vec2};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Default)]
pub(crate) struct CalendarFileWindow {
    /// Where the file is: `{"path"}` or `{"attachment"}`, as `calendar.import` takes it.
    pub source: Value,
    /// What `calendar.import` showed of it.
    pub preview: Value,
    /// The numbers of the events to add.
    pub chosen: BTreeSet<usize>,
    /// What went wrong, when adding them did not work.
    pub said: Option<String>,
}

fn events(preview: &Value) -> impl Iterator<Item = &Value> {
    preview["events"].as_array().into_iter().flatten()
}

fn text(v: &Value) -> String {
    one_line(v.as_str().unwrap_or_default())
}

/// When an event is, in words: its day for an all-day event, else its start and end.
fn when(e: &Value) -> String {
    let (start, end) = (text(&e["start"]), text(&e["end"]));
    if e["allDay"] == true {
        let first = start.get(..10).unwrap_or(&start).to_string();
        let last = text(&e["lastDay"]);
        return if e["recurring"] == true || first >= last { format!("{first}, all day") } else { format!("{first} to {last}, all day") };
    }
    match (start.get(..10), end.get(..10), end.get(11..)) {
        (Some(a), Some(b), Some(t)) if a == b => format!("{start} – {t}"),
        _ => format!("{start} – {end}"),
    }
}

impl MailUi {
    /// `ui.openCalendar`: open the window on a calendar file, add what is chosen, or close it.
    pub(crate) fn calendar_file_command(&mut self, params: &Value) -> Result<Value, String> {
        if params.get("close").and_then(Value::as_bool) == Some(true) {
            self.calendar_file = None;
            return Ok(Value::Null);
        }
        if params.get("import").and_then(Value::as_bool) == Some(true) {
            let Some(mut window) = self.calendar_file.take() else { return Err("Open Calendar is not open".to_string()) };
            if let Some(only) = params.get("only").and_then(Value::as_array) {
                window.chosen = only.iter().filter_map(Value::as_u64).filter_map(|n| usize::try_from(n).ok()).collect();
            }
            return match self.add_chosen(&window) {
                Ok(v) => Ok(v),
                Err(e) => {
                    window.said = Some(e.clone());
                    self.calendar_file = Some(window);
                    Err(e)
                }
            };
        }
        let source = match (params.get("path").and_then(Value::as_str), params.get("attachment").and_then(Value::as_u64)) {
            (Some(path), None) => json!({ "path": path }),
            (None, Some(index)) => json!({ "attachment": index }),
            _ => return Err("\"path\" or \"attachment\" is required".to_string()),
        };
        let mut asked = source.clone();
        asked["preview"] = json!(true);
        let preview = self.execute("calendar.import", &asked)?;
        let chosen = (0..events(&preview).count()).collect();
        let shown = json!({ "events": events(&preview).count() });
        self.calendar_file = Some(CalendarFileWindow { source, preview, chosen, said: None });
        Ok(shown)
    }

    /// Add the events chosen, and show the calendar at the first of them.
    fn add_chosen(&mut self, window: &CalendarFileWindow) -> Result<Value, String> {
        if window.chosen.is_empty() {
            return Err("No event is chosen".to_string());
        }
        let mut asked = window.source.clone();
        asked["only"] = json!(window.chosen.iter().collect::<Vec<_>>());
        let done = self.execute("calendar.import", &asked)?;
        let status = self.session.status.clone();
        let _ = self.execute("nav.module", &json!({ "module": "calendar" }));
        self.session.status = status;
        Ok(done)
    }

    /// The Open Calendar window, while it is open.
    pub(crate) fn calendar_file_window(&mut self, ctx: &egui::Context) {
        let Some(mut window) = self.calendar_file.take() else { return };
        let t = self.tokens.clone();
        let mut close = false;
        let mut add = false;
        let total = events(&window.preview).count();
        let room = (ctx.content_rect().height() - 300.0).clamp(160.0, 520.0);
        let shown = Dialog::new("open-calendar", "Open Calendar", Icon::Calendar).width(600.0).show(ctx, &t, |ui| {
            let name = text(&window.preview["untrusted_content"]["name"]);
            let from = match window.source.get("path").and_then(Value::as_str) {
                Some(path) => std::path::Path::new(path).file_name().map(|n| one_line(&n.to_string_lossy())).unwrap_or_default(),
                None => "the attachment".to_string(),
            };
            let head = match (name.is_empty(), total) {
                (true, 1) => format!("One event in {from}."),
                (true, n) => format!("{n} events in {from}."),
                (false, 1) => format!("One event of the calendar {name}, in {from}."),
                (false, n) => format!("{n} events of the calendar {name}, in {from}."),
            };
            ui.label(egui::RichText::new(head).font(font(13.0)).color(t.text));
            ui.label(
                egui::RichText::new("Chosen events are added to your calendar. One that is there already is brought up to date instead of being added twice. Undo takes all of it back.")
                    .font(font(12.0))
                    .color(t.text_dim),
            );
            let method = text(&window.preview["method"]).to_ascii_uppercase();
            if matches!(method.as_str(), "REQUEST" | "REPLY" | "CANCEL" | "COUNTER") {
                ui.add_space(6.0);
                dialog::callout(ui, &t, t.accent, |ui| {
                    ui.label(
                        egui::RichText::new("This file is part of a meeting invitation. Its events are added as appointments; nobody is answered.")
                            .font(font(12.5))
                            .color(t.text),
                    );
                });
            }
            ui.add_space(8.0);
            egui::ScrollArea::vertical().id_salt("open-calendar-list").max_height(room).show(ui, |ui| {
                for e in events(&window.preview) {
                    event_card(ui, &t, e, &mut window.chosen);
                }
                if total == 0 {
                    ui.label(egui::RichText::new("The file holds no events.").font(font(13.0)).color(t.text));
                }
                for n in window.preview["untrusted_content"]["notes"].as_array().into_iter().flatten() {
                    small_line(ui, &t, Icon::Info, &text(n), false);
                }
            });
            if let Some(said) = &window.said {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(one_line(said)).font(font(12.5)).color(t.danger));
            }
            dialog::foot(ui, &t, |ui| {
                if total > 1 {
                    let all = window.chosen.len() == total;
                    if ui.add(dialog::plain(if all { "Choose None" } else { "Choose All" })).clicked() {
                        window.chosen = if all { BTreeSet::new() } else { (0..total).collect() };
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let n = window.chosen.len();
                    let label = match n {
                        1 => "Add 1 Event".to_string(),
                        n => format!("Add {n} Events"),
                    };
                    if ui.add_enabled(n > 0, dialog::primary(&t, &label)).clicked() {
                        add = true;
                    }
                    if ui.add(dialog::plain("Cancel")).clicked() {
                        close = true;
                    }
                });
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
        if shown.closed || close {
            return;
        }
        if add {
            match self.add_chosen(&window) {
                Ok(_) => return,
                Err(e) => window.said = Some(e),
            }
        }
        self.calendar_file = Some(window);
    }
}

/// One event: chosen or not, what it is, when, how it repeats and what of it is not kept.
fn event_card(ui: &mut Ui, t: &Tokens, e: &Value, chosen: &mut BTreeSet<usize>) {
    let Some(index) = e["index"].as_u64().and_then(|n| usize::try_from(n).ok()) else { return };
    let seen = &e["untrusted_content"];
    dialog::card(ui, t, |ui| {
        ui.horizontal_top(|ui| {
            let mut on = chosen.contains(&index);
            if ui.add(egui::Checkbox::without_text(&mut on)).changed() {
                if on {
                    chosen.insert(index)
                } else {
                    chosen.remove(&index)
                };
            }
            ui.vertical(|ui| {
                let subject = text(&seen["subject"]);
                let subject = if subject.is_empty() { "(No subject)".to_string() } else { subject };
                ui.add(egui::Label::new(crate::seen_line(&subject, bold(13.5), t.text)).truncate());
                let mut line = when(e);
                let location = text(&seen["location"]);
                if !location.is_empty() {
                    line = format!("{line} · {location}");
                }
                ui.add(egui::Label::new(crate::seen_line(&line, font(12.0), t.text_dim)).truncate());
                if e["recurring"] == true {
                    small_line(ui, t, Icon::Sync, &text(&e["pattern"]), false);
                }
                if e["replaces"].is_u64() {
                    small_line(ui, t, Icon::Check, "In your calendar already: it is brought up to date", false);
                }
                for note in seen["notes"].as_array().into_iter().flatten() {
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

fn small_line(ui: &mut Ui, t: &Tokens, icon: Icon, words: &str, warn: bool) {
    let color = if warn { t.danger } else { t.text_dim };
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(13.0, 13.0), Sense::hover());
        icons::paint(ui.painter(), r, icon, color);
        ui.add(egui::Label::new(egui::RichText::new(one_line(words)).font(font(12.0)).color(color)).wrap());
    });
}
