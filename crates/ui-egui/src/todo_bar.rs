//! The To-Do Bar: a strip at the right of any module with a month to choose a day from and the
//! appointments still to come, favourite people, and the To-Do List with a box to add a task.
//! Which parts each module shows is the engine's (`view.todoBar`), and what they show is read
//! by `Session::todo_bar_due` — again a moment after something was done, never on every frame.

use crate::calendar::{Picker, month_picker};
use crate::icons::{self, Icon};
use crate::line_edit::LineEdit;
use crate::{MailUi, bold, clip_text, font, one_line};
use egui::{Align2, Rect, Sense, Ui, pos2, vec2};
use openmailclient_engine::cmd::todo_bar::heading;
use openmailclient_model::calendar;
use openmailclient_model::time;
use serde_json::{Value, json};

/// How wide the bar is; the module beside it has the rest.
pub(crate) const WIDTH: f32 = 264.0;
/// The head of each part: its name and the × that hides it.
const HEAD: f32 = 26.0;
const ROW: f32 = 22.0;

/// A day number as `YYYY-MM-DD`, as the commands take it.
fn iso(day: i64) -> String {
    let c = time::civil(time::from_unix(day.saturating_mul(86_400)), 0);
    format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
}

/// A task or a flagged message dragged from the To-Do List, to be given a time in the
/// Calendar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TaskDrag {
    pub(crate) store: usize,
    pub(crate) folder: u64,
    pub(crate) id: u64,
    pub(crate) subject: String,
}

/// Where a task dragged from the To-Do List is held over `rect`, while it is.
pub(crate) fn task_over(ui: &Ui, rect: Rect) -> Option<egui::Pos2> {
    if !egui::DragAndDrop::has_payload_of_type::<TaskDrag>(ui.ctx()) {
        return None;
    }
    ui.ctx().pointer_latest_pos().filter(|at| rect.contains(*at))
}

/// A task dragged from the To-Do List let go over `rect` now, and where; it is taken, so only
/// one place has it.
pub(crate) fn task_dropped(ui: &Ui, rect: Rect) -> Option<(TaskDrag, egui::Pos2)> {
    let at = task_over(ui, rect)?;
    if !ui.input(|i| i.pointer.any_released()) {
        return None;
    }
    let d = egui::DragAndDrop::take_payload::<TaskDrag>(ui.ctx())?;
    Some(((*d).clone(), at))
}

/// One line of a part's list.
enum Line {
    Head(String),
    Row(usize),
}

impl MailUi {
    /// The bar in `rect`, each part shown for the module there one under the other.
    pub(crate) fn todo_bar(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let ms = (ui.input(|i| i.time) * 1000.0).max(0.0) as u64;
        if let Some(wait) = self.session.todo_bar_due(ms) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(wait.clamp(100, 60_000)));
        }
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.pane);
        p.vline(rect.left() + 0.5, rect.y_range(), (1.0, t.line));
        let parts = self.session.todo_bar_parts();
        let mut y = rect.top();
        if parts.calendar {
            // The month and the next appointments take what is left when nothing else is shown,
            // and about half of it when the tasks are below.
            let room = if parts.tasks || parts.people { (rect.bottom() - y) * 0.5 } else { rect.bottom() - y };
            y = self.todo_calendar(ui, Rect::from_min_max(pos2(rect.left() + 1.0, y), pos2(rect.right(), y + room.max(HEAD))));
        }
        if parts.people {
            let room = if parts.tasks { ((self.session.todo.people.len() as f32 + 1.0) * ROW + HEAD + 40.0).min(200.0) } else { rect.bottom() - y };
            y = self.todo_people(ui, Rect::from_min_max(pos2(rect.left() + 1.0, y), pos2(rect.right(), (y + room).min(rect.bottom()))));
        }
        if parts.tasks && y + HEAD < rect.bottom() {
            self.todo_tasks(ui, Rect::from_min_max(pos2(rect.left() + 1.0, y), rect.max));
        }
    }

    /// A part's name and the × that hides it; the rest of `rect` is below it.
    fn todo_head(&mut self, ui: &mut Ui, rect: Rect, name: &str, part: &str) -> Rect {
        let t = self.tokens.clone();
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), HEAD));
        let p = ui.painter_at(head);
        p.hline(head.x_range(), head.bottom() - 0.5, (1.0, t.line));
        p.text(pos2(head.left() + 10.0, head.center().y), Align2::LEFT_CENTER, name, bold(12.5), t.text);
        let close = Rect::from_center_size(pos2(head.right() - 14.0, head.center().y), vec2(18.0, 18.0));
        let resp = ui.interact(close, egui::Id::new(("todo-close", part)), Sense::click()).on_hover_text(format!("Hide {name}"));
        if resp.hovered() {
            p.rect_filled(close, 3.0, t.hover);
        }
        icons::paint(&p, close.shrink(5.0), Icon::Close, t.text_dim);
        if resp.clicked() {
            self.act("view.todoBar", json!({ "part": part, "show": false }));
        }
        Rect::from_min_max(pos2(rect.left(), head.bottom()), rect.max)
    }

    /// The month, and below it the appointments of today and the next days not ended yet. A day
    /// clicked is shown in the Calendar; an appointment double-clicked is opened where it is.
    fn todo_calendar(&mut self, ui: &mut Ui, rect: Rect) -> f32 {
        let t = self.tokens.clone();
        let body = self.todo_head(ui, rect, "Calendar", "calendar");
        let today = self.session.today();
        let week_start = self.session.region.week_start;
        let picked = month_picker(
            ui,
            &t,
            &mut self.todo_nav,
            Picker { rect: body, top: body.top() + 8.0, today, chosen: today, shown: &[], week_start, salt: "todo-nav" },
        );
        if let Some(day) = picked.day {
            self.todo_nav = None;
            self.act("nav.module", json!({ "module": "calendar" }));
            self.go_to_day(day);
        }
        let list = Rect::from_min_max(pos2(body.left(), picked.bottom + 6.0), body.max);
        if list.height() < ROW {
            return rect.bottom();
        }
        let zone = self.session.zone.clone();
        let now = self.session.now;
        let items = &self.session.todo.appointments;
        let mut lines = Vec::with_capacity(items.len() + 8);
        let mut last = None;
        for (n, (_, a)) in items.iter().enumerate() {
            let day = calendar::day_of(a.start.max(calendar::day_start(today, &zone)), &zone);
            if last != Some(day) {
                let name = match day - today {
                    0 => "Today".to_string(),
                    1 => "Tomorrow".to_string(),
                    _ => {
                        let c = time::civil(time::from_unix(day.saturating_mul(86_400)), 0);
                        format!("{} {}", c.weekday_name(), self.session.region.date(&c))
                    }
                };
                lines.push(Line::Head(name));
                last = Some(day);
            }
            lines.push(Line::Row(n));
        }
        let p = ui.painter_at(list);
        if lines.is_empty() {
            p.text(pos2(list.left() + 12.0, list.top() + 12.0), Align2::LEFT_CENTER, "No upcoming appointments", font(12.0), t.text_dim);
            return (list.top() + 30.0).min(rect.bottom());
        }
        let mut opened = None;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
        egui::ScrollArea::vertical().id_salt("todo-appointments").auto_shrink([false, false]).show_rows(&mut child, ROW, lines.len(), |ui, range| {
            for line in lines.get(range).unwrap_or(&[]) {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                let p = ui.painter();
                let n = match line {
                    Line::Head(name) => {
                        p.text(pos2(r.left() + 10.0, r.center().y + 2.0), Align2::LEFT_CENTER, name, bold(12.0), t.accent);
                        continue;
                    }
                    Line::Row(n) => *n,
                };
                let Some((id, a)) = items.get(n) else { continue };
                if resp.hovered() {
                    p.rect_filled(r, 0.0, t.hover);
                }
                // The one under way now has the accent's bar at its side.
                let running = a.start <= now && now < a.end;
                if running {
                    p.rect_filled(Rect::from_min_size(pos2(r.left() + 4.0, r.top() + 3.0), vec2(3.0, r.height() - 6.0)), 1.0, t.accent);
                }
                let when = if a.all_day {
                    "All day".to_string()
                } else {
                    let c = zone.civil(a.start);
                    self.clock_name(c.hour, c.minute)
                };
                clip_text(p, pos2(r.left() + 12.0, r.center().y - 7.0), 62.0, &when, font(12.0), t.text_dim);
                let mut subject = one_line(&a.subject);
                if !a.location.trim().is_empty() {
                    subject = format!("{subject}  ({})", one_line(&a.location));
                }
                clip_text(
                    p,
                    pos2(r.left() + 78.0, r.center().y - 7.0),
                    r.width() - 86.0,
                    &subject,
                    if running { bold(12.0) } else { font(12.0) },
                    t.text,
                );
                if resp.double_clicked() {
                    opened = Some((*id, a.occurrence));
                }
            }
        });
        if let Some((id, occurrence)) = opened {
            self.open_appointment(id, occurrence);
        }
        rect.bottom()
    }

    /// Favourite people by their contacts' names; a click writes to one, the × takes one off,
    /// and a name or an address typed below adds one.
    fn todo_people(&mut self, ui: &mut Ui, rect: Rect) -> f32 {
        let t = self.tokens.clone();
        let body = self.todo_head(ui, rect, "People", "people");
        let people = self.session.todo.people.clone();
        let mut y = body.top() + 4.0;
        let (mut write, mut off, mut shown) = (None, None, None);
        for (n, f) in people.iter().enumerate() {
            let tall = if f.heard.is_some() { ROW + 16.0 } else { ROW };
            if y + tall > body.bottom() - 30.0 {
                break;
            }
            let r = Rect::from_min_size(pos2(body.left(), y), vec2(body.width(), ROW));
            let resp = ui.interact(r, egui::Id::new(("todo-person", n)), Sense::click()).on_hover_text(&f.address);
            let p = ui.painter_at(r);
            if resp.hovered() {
                p.rect_filled(r, 0.0, t.hover);
            }
            icons::paint(
                &p,
                Rect::from_center_size(pos2(r.left() + 18.0, r.center().y), vec2(13.0, 13.0)),
                Icon::People,
                if f.known { t.accent } else { t.text_faint },
            );
            clip_text(&p, pos2(r.left() + 32.0, r.center().y - 7.0), r.width() - 60.0, &one_line(&f.name), font(12.0), t.text);
            let x = Rect::from_center_size(pos2(r.right() - 14.0, r.center().y), vec2(16.0, 16.0));
            let x_resp = ui.interact(x, egui::Id::new(("todo-unfavorite", n)), Sense::click()).on_hover_text("Remove from Favorites");
            if resp.hovered() || x_resp.hovered() {
                icons::paint(&p, x.shrink(4.0), Icon::Close, if x_resp.hovered() { t.danger } else { t.text_faint });
            }
            if x_resp.clicked() {
                off = Some(f.address.clone());
            } else if resp.clicked() {
                write = Some(f.clone());
            }
            // The last they sent, and how many of theirs wait to be read; a click shows it.
            if let Some(h) = &f.heard {
                let line = Rect::from_min_size(pos2(body.left(), y + ROW - 4.0), vec2(body.width(), 18.0));
                let line_resp = ui.interact(line, egui::Id::new(("todo-heard", n)), Sense::click()).on_hover_text("Show the message");
                let p = ui.painter_at(line);
                if line_resp.hovered() {
                    p.rect_filled(line, 0.0, t.hover);
                }
                // Today's by the time, others by the day.
                let c = self.session.zone.civil(h.time);
                let today = calendar::day_of(h.time, &self.session.zone) == self.session.today();
                let when = if today { self.session.region.time(&c) } else { self.session.region.date(&c) };
                let mut right = line.right() - 8.0;
                if h.unread > 0 {
                    let count = h.unread.min(999).to_string();
                    p.text(pos2(right, line.center().y), Align2::RIGHT_CENTER, &count, bold(11.0), t.accent);
                    right -= crate::text_width(&p, &count, bold(11.0)) + 6.0;
                }
                p.text(pos2(right, line.center().y), Align2::RIGHT_CENTER, &when, font(10.5), t.text_faint);
                right -= crate::text_width(&p, &when, font(10.5)) + 6.0;
                let subject = if h.subject.trim().is_empty() { "(No subject)".to_string() } else { one_line(&h.subject) };
                let weight = if h.read { font(11.0) } else { bold(11.0) };
                clip_text(&p, pos2(line.left() + 32.0, line.center().y - 6.5), (right - line.left() - 32.0).max(0.0), &subject, weight, t.text_dim);
                if line_resp.clicked() {
                    shown = Some(h.clone());
                }
            }
            y += tall;
        }
        if people.is_empty() {
            ui.painter_at(body).text(pos2(body.left() + 12.0, y + 9.0), Align2::LEFT_CENTER, "No favourites yet", font(12.0), t.text_dim);
            y += ROW;
        }
        let line = Rect::from_min_size(pos2(body.left() + 8.0, y + 4.0), vec2(body.width() - 16.0, 24.0));
        let p = ui.painter_at(line.expand(1.0));
        p.rect_stroke(line, 3.0, (1.0, t.line), egui::StrokeKind::Inside);
        let resp = ui.put(
            line.shrink2(vec2(6.0, 3.0)),
            LineEdit::new(&mut self.todo_person, egui::Id::new("todo-person-add"))
                .bare()
                .char_limit(320)
                .hint("Add a favourite: a name or address", Some(t.text_faint)),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.todo_person.trim().is_empty() {
            match self.favorite_typed() {
                Ok(()) => self.todo_person.clear(),
                Err(e) => self.session.status = e,
            }
        }
        if let Some(address) = off {
            self.act("people.favorite", json!({ "address": address, "on": false }));
        }
        if let Some(f) = write {
            let to = if f.known { format!("{} <{}>", f.name, f.address) } else { f.address.clone() };
            self.act("mail.new", Value::Null);
            self.act("compose.setField", json!({ "field": "to", "value": to }));
        }
        if let Some(h) = shown {
            self.act("mail.showMessage", json!({ "store": h.store, "folder": h.folder.0, "id": h.id }));
        }
        (line.bottom() + 8.0).min(rect.bottom())
    }

    /// A task dragged from the To-Do List given a time in the Calendar: `start` as
    /// `calendar.fromTask` takes it.
    pub(crate) fn schedule_task(&mut self, d: &TaskDrag, start: &str) {
        let asked = json!({ "id": d.id, "store": d.store, "folder": d.folder, "start": start });
        if let Err(e) = self.execute("calendar.fromTask", &asked) {
            self.session.status = e;
        }
    }

    /// What was typed in the People part made a favourite: an address as it is, a name as the
    /// first contact it names.
    fn favorite_typed(&mut self) -> Result<(), String> {
        let typed = one_line(&self.todo_person);
        let typed = typed.trim();
        let address = if typed.contains('@') {
            typed.trim_matches(['<', '>']).to_string()
        } else {
            let found = self.execute("people.suggest", &json!({ "text": typed, "limit": 10 }))?;
            found["untrusted_content"]["suggestions"]
                .as_array()
                .and_then(|all| all.iter().find(|s| s["kind"] == "contact"))
                .and_then(|s| s["address"].as_str())
                .filter(|a| !a.is_empty())
                .map(str::to_string)
                .ok_or_else(|| format!("No contact is named {typed}"))?
        };
        self.execute("people.favorite", &json!({ "address": address })).map(|_| ())
    }

    /// The To-Do List under its headings, a box to tick each off and a line to add one, due today
    /// when no day is said.
    fn todo_tasks(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let body = self.todo_head(ui, rect, "Tasks", "tasks");
        let line = Rect::from_min_size(pos2(body.left() + 8.0, body.top() + 6.0), vec2(body.width() - 16.0, 24.0));
        let p = ui.painter_at(line.expand(1.0));
        p.rect_stroke(line, 3.0, (1.0, t.line), egui::StrokeKind::Inside);
        let resp = ui.put(
            line.shrink2(vec2(6.0, 3.0)),
            LineEdit::new(&mut self.todo_add, egui::Id::new("todo-task-add")).bare().char_limit(500).hint("Type a new task", Some(t.text_faint)),
        );
        let today = self.session.today();
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.todo_add.trim().is_empty() {
            let mut params = crate::tasks::quick_task(&self.todo_add, today);
            if params.get("due").is_none() {
                params["due"] = json!(iso(today));
                params["start"] = json!(iso(today));
            }
            match self.execute("tasks.new", &params) {
                Ok(_) => {
                    self.todo_add.clear();
                    resp.request_focus();
                }
                Err(e) => self.session.status = e,
            }
        }
        let list = Rect::from_min_max(pos2(body.left(), line.bottom() + 6.0), body.max);
        let week_start = self.session.region.week_start;
        let rows = &self.session.todo.tasks;
        let mut lines = Vec::with_capacity(rows.len() + 8);
        let mut last = None;
        for (n, r) in rows.iter().enumerate() {
            let name = heading(r.task.due, today, week_start);
            if last != Some(name) {
                lines.push(Line::Head(name.to_string()));
                last = Some(name);
            }
            lines.push(Line::Row(n));
        }
        if lines.is_empty() {
            ui.painter_at(list).text(
                pos2(list.left() + 12.0, list.top() + 12.0),
                Align2::LEFT_CENTER,
                "Nothing on the To-Do List",
                font(12.0),
                t.text_dim,
            );
            return;
        }
        let (mut ticked, mut opened) = (None, None);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
        egui::ScrollArea::vertical().id_salt("todo-tasks").auto_shrink([false, false]).show_rows(&mut child, ROW + 2.0, lines.len(), |ui, range| {
            for line in lines.get(range).unwrap_or(&[]) {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW + 2.0), Sense::click_and_drag());
                let p = ui.painter();
                let n = match line {
                    Line::Head(name) => {
                        let color = if name == "Overdue" { t.danger } else { t.accent };
                        p.text(pos2(r.left() + 10.0, r.center().y + 2.0), Align2::LEFT_CENTER, name, bold(12.0), color);
                        continue;
                    }
                    Line::Row(n) => *n,
                };
                let Some(row) = rows.get(n) else { continue };
                // Dragged onto the Calendar, it is given a time there.
                let (store, folder, id) = row.key();
                resp.dnd_set_drag_payload(TaskDrag { store, folder: folder.0, id, subject: one_line(&row.task.subject) });
                if resp.hovered() {
                    p.rect_filled(r, 0.0, t.hover);
                }
                let tick = Rect::from_center_size(pos2(r.left() + 18.0, r.center().y), vec2(13.0, 13.0));
                let tick_resp = ui.interact(tick.expand(3.0), egui::Id::new(("todo-tick", row.key())), Sense::click()).on_hover_text("Mark Complete");
                p.rect_stroke(tick, 2.0, (1.0, if tick_resp.hovered() { t.accent } else { t.text_dim }), egui::StrokeKind::Inside);
                if tick_resp.clicked() {
                    ticked = Some(row.key());
                }
                let color = if row.task.is_overdue(today) { t.danger } else { t.text };
                clip_text(p, pos2(r.left() + 32.0, r.center().y - 7.0), r.width() - 58.0, &one_line(&row.task.subject), font(12.0), color);
                let flag = Rect::from_center_size(pos2(r.right() - 14.0, r.center().y), vec2(12.0, 12.0));
                icons::paint(p, flag, if row.task.flagged_mail { Icon::Mail } else { Icon::Flag }, t.flag);
                if resp.double_clicked() && !tick_resp.hovered() {
                    opened = Some(row.key());
                }
            }
        });
        // What is being dragged, by the pointer, above everything.
        if let (Some(d), Some(at)) = (egui::DragAndDrop::payload::<TaskDrag>(ui.ctx()), ui.ctx().pointer_latest_pos()) {
            let p = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("todo-task-drag")));
            let words = if d.subject.trim().is_empty() { "(No subject)".to_string() } else { d.subject.clone() };
            let r = Rect::from_min_size(at + vec2(14.0, 8.0), vec2(220.0, ROW));
            p.rect_filled(r, 3.0, t.surface);
            p.rect_stroke(r, 3.0, (1.0, t.accent), egui::StrokeKind::Inside);
            clip_text(&p, pos2(r.left() + 8.0, r.center().y - 7.0), r.width() - 16.0, &words, font(12.0), t.text);
        }
        if let Some((store, folder, id)) = ticked {
            self.act("tasks.markComplete", json!({ "id": id, "store": store, "folder": folder.0 }));
        }
        if let Some((store, folder, id)) = opened {
            self.act("ui.task", json!({ "id": id, "store": store, "folder": folder.0 }));
        }
    }
}
