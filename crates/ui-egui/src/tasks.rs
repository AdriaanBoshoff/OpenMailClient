//! The Tasks module: a Tasks folder's tasks, and on the To-Do List the mail flagged for
//! follow-up beside them, in the views Outlook's Current View offers; each opened in a window
//! to be made or changed, and ticked off in the list.
//!
//! Better than Outlook's: a task is added by typing its subject on the line above the list,
//! with "today", "tomorrow" or a date at its end to give it a due day; the To-Do List is put
//! under headings by when each is due, late ones first and in red; how far along each is
//! shows as a bar; the search looks in notes, owners and companies too as it is typed; and the
//! task window says how long there is left, or how late it is.

use crate::dialog::{self, Dialog};
use crate::icons::{self, Icon};
use crate::line_edit::LineEdit;
use crate::{MailUi, bold, clip_text, font, one_line};
use egui::{Align2, Color32, Rect, Sense, Ui, pos2, vec2};
use openmailclient_engine::cmd::tasks::{Row, View};
use openmailclient_model::FolderId;
use openmailclient_model::task::{MAX_WORK, Status, Task};
use openmailclient_model::time;
use serde_json::{Value, json};

const STATUSES: [Status; 5] = [Status::NotStarted, Status::InProgress, Status::Complete, Status::Waiting, Status::Deferred];
const IMPORTANCE: [(i32, &str); 3] = [(0, "Low"), (1, "Normal"), (2, "High")];

type Key = (usize, FolderId, u64);

fn key_json((store, folder, id): Key) -> Value {
    json!({ "id": id, "store": store, "folder": folder.0 })
}

/// A day number as `YYYY-MM-DD`, as the commands take it.
fn iso(day: Option<i64>) -> String {
    day.map(|d| {
        let c = time::civil(time::from_unix(d.saturating_mul(86_400)), 0);
        format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
    })
    .unwrap_or_default()
}

/// Work in minutes as the window shows it: in hours, days of eight hours or weeks of forty, as
/// Outlook counts them.
fn work_text(minutes: i32) -> String {
    match minutes {
        0 => String::new(),
        m if m % (40 * 60) == 0 => format!("{} weeks", m / (40 * 60)),
        m if m % (8 * 60) == 0 => format!("{} days", m / (8 * 60)),
        m if m % 60 == 0 => format!("{} hours", m / 60),
        m => format!("{m} minutes"),
    }
}

/// Work typed in the window: a number with "minutes", "hours", "days" (of eight hours) or
/// "weeks" (of forty) after it, hours when nothing is; minutes, or why not.
pub(crate) fn parse_work(text: &str) -> Result<i32, String> {
    let t = text.trim().to_ascii_lowercase();
    if t.is_empty() {
        return Ok(0);
    }
    let split = t.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).unwrap_or(t.len());
    let (number, unit) = t.split_at(split);
    let n: f64 = number.replace(',', ".").parse().map_err(|_| format!("\"{}\" is not an amount of work", one_line(text)))?;
    let per = match unit.trim() {
        "" | "h" | "hr" | "hrs" | "hour" | "hours" => 60.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 1.0,
        "d" | "day" | "days" => 8.0 * 60.0,
        "w" | "wk" | "wks" | "week" | "weeks" => 40.0 * 60.0,
        _ => return Err(format!("\"{}\" is not an amount of work: say hours, days or weeks", one_line(text))),
    };
    let minutes = (n * per).round();
    if !minutes.is_finite() || !(0.0..=f64::from(MAX_WORK)).contains(&minutes) {
        return Err("That is more work than a task holds".to_string());
    }
    Ok(minutes as i32)
}

/// What is typed on the line above the list made a task: its subject, and a due day when it
/// ends with "today", "tomorrow" or a date written YYYY-MM-DD.
pub(crate) fn quick_task(text: &str, today: i64) -> Value {
    let text = one_line(text);
    let mut subject = text.trim().to_string();
    let mut due = None;
    for (word, day) in [(" today", today), (" tomorrow", today.saturating_add(1))] {
        // The subject's own last letters are looked at, so a cut is always between letters.
        let cut = subject.len().saturating_sub(word.len());
        if cut > 0 && subject.get(cut..).is_some_and(|tail| tail.eq_ignore_ascii_case(word)) {
            due = Some(iso(Some(day)));
            subject.truncate(cut);
            break;
        }
    }
    if due.is_none()
        && let Some((head, tail)) = subject.rsplit_once(' ')
        && tail.len() == 10
        && tail.chars().all(|c| c.is_ascii_digit() || c == '-')
        && !head.trim().is_empty()
    {
        due = Some(tail.to_string());
        subject = head.trim_end().to_string();
    }
    let mut p = json!({ "subject": subject.trim() });
    if let Some(d) = due {
        p["due"] = json!(d);
        p["start"] = json!(d);
    }
    p
}

/// The heading a task stands under on the To-Do List, by when it is due.
pub(crate) fn heading(t: &Task, today: i64, week_start: u32) -> &'static str {
    openmailclient_engine::cmd::todo_bar::heading(t.due, today, week_start)
}

/// How long there is left, or how late it is, said as Outlook's info bar says it.
pub(crate) fn due_words(t: &Task, today: i64) -> String {
    if t.is_complete() {
        return match t.completed {
            Some(d) => format!("Completed on {}.", iso(Some(d))),
            None => "Completed.".to_string(),
        };
    }
    match t.due.map(|d| d.saturating_sub(today)) {
        None => String::new(),
        Some(0) => "Due today.".to_string(),
        Some(1) => "Due tomorrow.".to_string(),
        Some(n) if n > 1 => format!("Due in {n} days."),
        Some(-1) => "Overdue by a day.".to_string(),
        Some(n) => format!("Overdue by {} days.", n.saturating_neg()),
    }
}

/// The task window: a task being made or changed, or the follow-up of a flagged message.
#[derive(Clone, Default)]
pub(crate) struct TaskForm {
    pub key: Option<Key>,
    pub flagged_mail: bool,
    pub subject: String,
    subject_was: String,
    pub start: String,
    pub due: String,
    pub status: Status,
    pub percent: u8,
    pub importance: i32,
    pub reminder_on: bool,
    pub reminder: String,
    pub owner: String,
    pub categories: String,
    pub notes: String,
    pub total_work: String,
    pub actual_work: String,
    pub mileage: String,
    pub billing: String,
    pub companies: String,
    pub private: bool,
    /// What the window says of when it is due, worked out when it was opened.
    due_words: String,
    pub said: Option<String>,
    focus: bool,
}

impl TaskForm {
    pub(crate) fn new(today: i64) -> Self {
        Self { importance: 1, focus: true, reminder: format!("{} 08:00", iso(Some(today))), ..Self::default() }
    }

    pub(crate) fn of(key: Key, t: &Task, reminder: String, today: i64) -> Self {
        Self {
            key: Some(key),
            flagged_mail: t.flagged_mail,
            subject: t.subject.clone(),
            subject_was: t.subject.clone(),
            start: iso(t.start),
            due: iso(t.due),
            status: t.status,
            percent: t.percent,
            importance: t.importance,
            reminder_on: t.reminder.is_some(),
            reminder: if reminder.is_empty() { format!("{} 08:00", iso(t.due.or(Some(today)))) } else { reminder },
            owner: t.owner.clone(),
            categories: t.categories.join(", "),
            notes: t.notes.clone(),
            total_work: work_text(t.total_work),
            actual_work: work_text(t.actual_work),
            mileage: t.mileage.clone(),
            billing: t.billing.clone(),
            companies: t.companies.join(", "),
            private: t.private,
            due_words: due_words(t, today),
            said: None,
            focus: true,
        }
    }

    /// What `tasks.new` or `tasks.change` is given.
    pub(crate) fn params(&self) -> Result<Value, String> {
        let day = |d: &str| if d.trim().is_empty() { Value::Null } else { json!(d.trim()) };
        let mut p = json!({
            "subject": self.subject,
            "start": day(&self.start),
            "due": day(&self.due),
            "status": self.status.name(),
            "percent": self.percent,
            "reminder": if self.reminder_on { json!(self.reminder.trim()) } else { Value::Null },
        });
        if !self.flagged_mail {
            let list = |s: &str| s.split([',', ';']).map(str::trim).filter(|c| !c.is_empty()).map(str::to_string).collect::<Vec<_>>();
            p["importance"] = json!(IMPORTANCE.iter().find(|(i, _)| *i == self.importance).map_or("normal", |(_, n)| n).to_lowercase());
            p["owner"] = json!(self.owner);
            p["categories"] = json!(list(&self.categories));
            p["notes"] = json!(self.notes);
            p["totalWork"] = json!(parse_work(&self.total_work)?);
            p["actualWork"] = json!(parse_work(&self.actual_work)?);
            p["mileage"] = json!(self.mileage);
            p["billing"] = json!(self.billing);
            p["companies"] = json!(list(&self.companies));
            p["private"] = json!(self.private);
        }
        if let Some(key) = self.key {
            for (k, v) in key_json(key).as_object().into_iter().flatten() {
                p[k] = v.clone();
            }
        }
        Ok(p)
    }

    /// Status and how far along it is kept in step as they are changed, as Outlook keeps them.
    fn status_changed(&mut self) {
        if self.status == Status::Complete {
            self.percent = 100;
        } else if self.percent == 100 {
            self.percent = 0;
        }
    }

    fn percent_changed(&mut self) {
        if self.percent == 100 {
            self.status = Status::Complete;
        } else if self.status == Status::Complete || (self.percent > 0 && self.status == Status::NotStarted) {
            self.status = Status::InProgress;
        }
    }
}

enum Ask {
    Close,
    Save,
    Complete,
    Delete,
}

/// One line of the list: a heading with how many stand under it, or a row.
enum Line {
    Head(&'static str, usize),
    Row(usize),
}

impl MailUi {
    /// `ui.task`: the task window on a new task, or on the row named (by "id", "store" and
    /// "folder"), or on the one chosen when "open" is true.
    pub(crate) fn task_command(&mut self, params: &Value) -> Result<Value, String> {
        let today = self.session.today();
        let wanted = if params.get("open").and_then(Value::as_bool) == Some(true) {
            Some(self.session.tasks.selected.ok_or("No task is chosen")?)
        } else {
            params.get("id").and_then(Value::as_u64).map(|id| {
                let shown = self.session.tasks.folder;
                let store = params.get("store").and_then(Value::as_u64).and_then(|s| usize::try_from(s).ok()).or(shown.map(|(s, _)| s)).unwrap_or(0);
                let folder = params.get("folder").and_then(Value::as_u64).map(FolderId).or(shown.map(|(_, f)| f)).unwrap_or(FolderId(0));
                (store, folder, id)
            })
        };
        match wanted {
            Some(key) => {
                // Shown in the Tasks module, or on the To-Do Bar beside any module.
                let listed = self.session.tasks.rows.iter().any(|r| r.key() == key);
                let row = self
                    .session
                    .tasks
                    .rows
                    .iter()
                    .chain(&self.session.todo.tasks)
                    .find(|r| r.key() == key)
                    .cloned()
                    .ok_or("That task is not shown")?;
                let reminder = row.task.reminder.map(|r| self.local_stamp(r)).unwrap_or_default();
                self.task = Some(TaskForm::of(key, &row.task, reminder, today));
                if listed {
                    self.act("tasks.select", key_json(key));
                }
            }
            None if self.session.is_enabled("tasks.new") => self.task = Some(TaskForm::new(today)),
            None => return Err("No data file open can be written".to_string()),
        }
        Ok(Value::Null)
    }

    /// A UTC time as `YYYY-MM-DD HH:MM` where the session is.
    fn local_stamp(&self, utc: i64) -> String {
        let c = self.session.zone.civil(utc);
        format!("{:04}-{:02}-{:02} {:02}:{:02}", c.year, c.month, c.day, c.hour, c.minute)
    }

    fn day_shown(&self, day: Option<i64>) -> String {
        day.map(|d| {
            let c = time::civil(time::from_unix(d.saturating_mul(86_400)), 0);
            let name = c.weekday_name();
            format!("{} {}", name.get(..3).unwrap_or(name), self.session.region.date(&c))
        })
        .unwrap_or_else(|| "None".to_string())
    }

    /// The Tasks module's whole body.
    pub(crate) fn tasks_body(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        ui.painter_at(rect).rect_filled(rect, 0.0, t.surface);
        let mut left = rect.left();
        if self.session.folder_pane {
            let w = self.folder_w.clamp(200.0, 280.0).min(rect.width() * 0.4);
            self.tasks_side(ui, Rect::from_min_size(rect.min, vec2(w, rect.height())));
            left += w;
            ui.painter_at(rect).vline(left + 0.5, rect.y_range(), (1.0, t.line));
        }
        let area = Rect::from_min_max(pos2(left + 1.0, rect.top()), rect.max);
        let head = Rect::from_min_size(area.min, vec2(area.width(), 44.0));
        self.tasks_head(ui, head);
        let rest = Rect::from_min_max(pos2(area.left(), head.bottom()), area.max);
        let reading = rest.width() > 760.0 && self.session.tasks.selected.is_some();
        let list = if reading { Rect::from_min_size(rest.min, vec2((rest.width() * 0.58).max(440.0), rest.height())) } else { rest };
        let add = Rect::from_min_size(list.min, vec2(list.width(), 34.0));
        self.tasks_add_line(ui, add);
        let body = Rect::from_min_max(pos2(list.left(), add.bottom()), list.max);
        if self.session.tasks.rows.is_empty() {
            let p = ui.painter_at(body);
            let words = if !self.session.tasks.query.is_empty() {
                "Nothing was found"
            } else if self.session.tasks.folder.is_none() && self.session.tasks.view != View::ToDoList {
                "No data file open has a Tasks folder; type above to make one"
            } else {
                "There are no items to show in this view"
            };
            icons::paint(&p, Rect::from_center_size(body.center() - vec2(0.0, 30.0), vec2(44.0, 44.0)), Icon::Tasks, t.disabled);
            p.text(body.center() + vec2(0.0, 14.0), Align2::CENTER_CENTER, words, font(13.5), t.text_dim);
        } else {
            self.tasks_list(ui, body);
        }
        if reading {
            ui.painter_at(rest).vline(list.right() + 0.5, rest.y_range(), (1.0, t.line));
            self.task_card(ui, Rect::from_min_max(pos2(list.right() + 1.0, rest.top()), rest.max));
        }
    }

    /// My Tasks: the To-Do List and every Tasks folder of the data files open.
    fn tasks_side(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.pane);
        p.text(pos2(rect.left() + 14.0, rect.top() + 20.0), Align2::LEFT_CENTER, "My Tasks", bold(12.5), t.text);
        let found = self.session.run("tasks.folders", &Value::Null).ok();
        let folders: Vec<Value> = found.as_ref().and_then(|v| v["folders"].as_array()).cloned().unwrap_or_default();
        let files = folders.first().is_some_and(|f| folders.iter().any(|c| c["store"] != f["store"]));
        let to_do = self.session.tasks.view == View::ToDoList;
        let mut y = rect.top() + 36.0;
        let mut act: Option<(&str, Value)> = None;
        let entry = |ui: &mut Ui, y: f32, key: (&str, u64, u64), icon: Icon, name: &str, count: String, on: bool| {
            let r = Rect::from_min_size(pos2(rect.left() + 4.0, y), vec2(rect.width() - 8.0, 28.0));
            let resp = ui.interact(r, egui::Id::new(("tasks-side", key)), Sense::click());
            if on {
                p.rect_filled(r, 3.0, t.selected);
            } else if resp.hovered() {
                p.rect_filled(r, 3.0, t.hover);
            }
            icons::paint(&p, Rect::from_center_size(pos2(r.left() + 14.0, r.center().y), vec2(14.0, 14.0)), icon, t.text_dim);
            let cw = crate::text_width(&p, &count, font(11.5));
            clip_text(&p, pos2(r.left() + 28.0, r.center().y - 8.0), r.width() - 40.0 - cw, name, font(12.5), t.text);
            p.text(pos2(r.right() - 8.0, r.center().y), Align2::RIGHT_CENTER, count, font(11.5), t.text_faint);
            resp.clicked()
        };
        if entry(ui, y, ("todo", 0, 0), Icon::Flag, "To-Do List", String::new(), to_do) {
            act = Some(("tasks.view", json!({ "view": "toDoList" })));
        }
        y += 30.0;
        for f in &folders {
            if y + 28.0 > rect.bottom() {
                break;
            }
            let mut name = f["name"].as_str().unwrap_or("").to_string();
            if files {
                name = format!("{name} - {}", f["file"].as_str().unwrap_or(""));
            }
            let count = f["items"].as_u64().unwrap_or(0).to_string();
            let key = ("folder", f["store"].as_u64().unwrap_or(0), f["folder"].as_u64().unwrap_or(0));
            if entry(ui, y, key, Icon::Tasks, &name, count, !to_do && f["shown"] == true) {
                act = Some(("ui.tasksFolder", json!({ "store": f["store"], "folder": f["folder"] })));
            }
            y += 30.0;
        }
        if folders.is_empty() {
            clip_text(&p, pos2(rect.left() + 14.0, y + 4.0), rect.width() - 24.0, "No Tasks folder in the data files open", font(12.0), t.text_dim);
            y += 30.0;
        }
        // Every view, where it can always be reached, however narrow the ribbon.
        y += 14.0;
        if y + 40.0 < rect.bottom() {
            p.text(pos2(rect.left() + 14.0, y + 6.0), Align2::LEFT_CENTER, "Current View", bold(12.5), t.text);
            y += 22.0;
        }
        let shown = self.session.tasks.view;
        for view in View::ALL.into_iter().filter(|v| *v != View::ToDoList) {
            if y + 28.0 > rect.bottom() {
                break;
            }
            let on = shown == view;
            let r = Rect::from_min_size(pos2(rect.left() + 4.0, y), vec2(rect.width() - 8.0, 26.0));
            let resp = ui.interact(r, egui::Id::new(("tasks-view", view.name())), Sense::click());
            if on {
                p.rect_filled(r, 3.0, t.selected);
            } else if resp.hovered() {
                p.rect_filled(r, 3.0, t.hover);
            }
            let dot = pos2(r.left() + 14.0, r.center().y);
            p.circle_stroke(dot, 5.0, (1.0, if on { t.accent } else { t.text_dim }));
            if on {
                p.circle_filled(dot, 2.5, t.accent);
            }
            clip_text(&p, pos2(r.left() + 28.0, r.center().y - 8.0), r.width() - 36.0, view.label(), font(12.5), t.text);
            if resp.clicked() {
                act = Some(("tasks.view", json!({ "view": view.name() })));
            }
            y += 27.0;
        }
        if let Some((id, params)) = act {
            if id == "ui.tasksFolder" {
                self.act("tasks.open", params);
                if to_do {
                    self.act("tasks.view", json!({ "view": "simpleList" }));
                }
            } else {
                self.act(id, params);
            }
        }
    }

    /// The search box, how many there are, and the view.
    fn tasks_head(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.hline(rect.x_range(), rect.bottom() - 0.5, (1.0, t.line));
        let shown = self.session.tasks.rows.len();
        let view = self.session.tasks.view;
        let place = if view == View::ToDoList { "To-Do List".to_string() } else { format!("Tasks  ·  {}", view.label()) };
        let mut title = format!("{place}  ·  {shown}");
        if self.session.tasks.left_out > 0 {
            title.push_str(&format!(" (and {} older flagged messages)", self.session.tasks.left_out));
        }
        clip_text(&p, pos2(rect.left() + 14.0, rect.center().y - 9.0), rect.width() * 0.5, &title, bold(14.0), t.text);
        let w = (rect.width() * 0.36).clamp(160.0, 300.0);
        let field = Rect::from_min_size(pos2(rect.right() - w - 12.0, rect.center().y - 14.0), vec2(w, 28.0));
        p.rect_filled(field, 4.0, t.pane);
        p.rect_stroke(field, 4.0, (1.0, t.line), egui::StrokeKind::Inside);
        icons::paint(&p, Rect::from_center_size(pos2(field.left() + 14.0, field.center().y), vec2(13.0, 13.0)), Icon::Search, t.text_dim);
        let inner = Rect::from_min_max(pos2(field.left() + 26.0, field.top() + 2.0), pos2(field.right() - 6.0, field.bottom() - 2.0));
        let hint = if view == View::ToDoList { "Search To-Do List" } else { "Search Tasks" };
        let resp =
            ui.put(inner, LineEdit::new(&mut self.tasks_query, egui::Id::new("tasks-search")).bare().char_limit(200).hint(hint, Some(t.text_faint)));
        if std::mem::take(&mut self.tasks_search_focus) {
            resp.request_focus();
        }
        if self.tasks_query != self.session.tasks.query {
            let q = self.tasks_query.clone();
            self.act("tasks.search", json!({ "query": q }));
        }
    }

    /// "Click here to add a new task": a subject typed and Enter makes it.
    fn tasks_add_line(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.hline(rect.x_range(), rect.bottom() - 0.5, (1.0, t.line));
        icons::paint(&p, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(14.0, 14.0)), Icon::Tasks, t.text_faint);
        let inner = Rect::from_min_max(pos2(rect.left() + 38.0, rect.top() + 5.0), pos2(rect.right() - 10.0, rect.bottom() - 5.0));
        let resp = ui.put(
            inner,
            LineEdit::new(&mut self.tasks_add, egui::Id::new("tasks-add"))
                .bare()
                .char_limit(500)
                .hint("Type a new task, then Enter  (end with today, tomorrow or YYYY-MM-DD for a due day)", Some(t.text_faint)),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.tasks_add.trim().is_empty() {
            let params = quick_task(&self.tasks_add, self.session.today());
            match self.execute("tasks.new", &params) {
                Ok(_) => {
                    self.tasks_add.clear();
                    self.tasks_scroll = true;
                    resp.request_focus();
                }
                Err(e) => self.session.status = e,
            }
        }
    }

    /// The tasks shown, under headings by when they are due on the To-Do List.
    fn tasks_list(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let today = self.session.today();
        let view = self.session.tasks.view;
        let detailed = matches!(view, View::Detailed | View::Prioritized | View::Completed);
        let columns: &[(&str, f32)] = if detailed {
            &[("Task Subject", 0.33), ("Status", 0.19), ("Due Date", 0.18), ("% Complete", 0.15), ("Categories", 0.15)]
        } else {
            &[("Task Subject", 0.58), ("Due Date", 0.22), ("In Folder", 0.20)]
        };
        // The box to tick it off, its importance and its flag stand before the columns.
        let lead = 50.0;
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), 26.0));
        let p = ui.painter_at(head);
        p.rect_filled(head, 0.0, t.pane);
        p.hline(head.x_range(), head.bottom() - 0.5, (1.0, t.line));
        icons::paint(&p, Rect::from_center_size(pos2(head.left() + 14.0, head.center().y), vec2(12.0, 12.0)), Icon::Check, t.text_dim);
        icons::paint(&p, Rect::from_center_size(pos2(head.left() + 36.0, head.center().y), vec2(12.0, 12.0)), Icon::Importance, t.text_dim);
        let width = rect.width() - lead - 28.0;
        let mut x = head.left() + lead;
        for (name, share) in columns {
            clip_text(&p, pos2(x, head.center().y - 7.0), width * share - 10.0, name, bold(12.0), t.text_dim);
            x += width * share;
        }
        icons::paint(&p, Rect::from_center_size(pos2(head.right() - 16.0, head.center().y), vec2(12.0, 12.0)), Icon::Flag, t.text_dim);

        let rows = &self.session.tasks.rows;
        let mut lines = Vec::with_capacity(rows.len() + 8);
        if view == View::ToDoList {
            let mut at = 0;
            while at < rows.len() {
                let week_start = self.session.region.week_start;
                let name = rows.get(at).map_or("", |r| heading(&r.task, today, week_start));
                let count = rows.get(at..).unwrap_or(&[]).iter().take_while(|r| heading(&r.task, today, week_start) == name).count();
                lines.push(Line::Head(name, count));
                lines.extend((at..at + count).map(Line::Row));
                at += count.max(1);
            }
        } else {
            lines.extend((0..rows.len()).map(Line::Row));
        }
        let body = Rect::from_min_max(pos2(rect.left(), head.bottom()), rect.max);
        let row_h = 28.0;
        let selected = self.session.tasks.selected;
        let scroll_to = std::mem::take(&mut self.tasks_scroll);
        let (mut chosen, mut opened, mut ticked) = (None, None, None);
        let folder_names: Vec<String> =
            rows.iter().map(|r| self.session.folder_info(r.store, r.folder).map(|f| f.name.clone()).unwrap_or_default()).collect();
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
        egui::ScrollArea::vertical().id_salt("tasks-list").auto_shrink([false, false]).show_rows(&mut child, row_h, lines.len(), |ui, range| {
            for line in lines.get(range).unwrap_or(&[]) {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                let p = ui.painter();
                let row = match line {
                    Line::Head(name, count) => {
                        let color = if *name == "Overdue" { t.danger } else { t.accent };
                        p.text(pos2(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, format!("{name}  ({count})"), bold(12.5), color);
                        p.hline((r.left() + 12.0)..=(r.right() - 12.0), r.bottom() - 1.5, (1.0, color.gamma_multiply(0.5)));
                        continue;
                    }
                    Line::Row(i) => *i,
                };
                let Some(item) = rows.get(row) else { continue };
                let task = &item.task;
                let on = selected == Some(item.key());
                if on {
                    p.rect_filled(r, 0.0, t.selected);
                } else if resp.hovered() {
                    p.rect_filled(r, 0.0, t.hover);
                }
                let done = task.is_complete();
                let late = task.is_overdue(today);
                let color = if done {
                    t.text_faint
                } else if late {
                    t.danger
                } else {
                    t.text
                };
                // The box: a click ticks it off, or takes the tick back.
                let tick = Rect::from_center_size(pos2(r.left() + 14.0, r.center().y), vec2(14.0, 14.0));
                let tick_resp = ui.interact(tick.expand(3.0), egui::Id::new(("task-tick", item.key())), Sense::click());
                p.rect_stroke(tick, 2.0, (1.0, if tick_resp.hovered() { t.accent } else { t.text_dim }), egui::StrokeKind::Inside);
                if done {
                    p.rect_filled(tick, 2.0, t.accent);
                    icons::paint(p, tick.shrink(2.0), Icon::Check, t.on_accent);
                }
                if tick_resp.clicked() {
                    ticked = Some((item.key(), done));
                }
                match task.importance {
                    2 => {
                        icons::paint(p, Rect::from_center_size(pos2(r.left() + 36.0, r.center().y), vec2(12.0, 12.0)), Icon::Importance, t.danger);
                    }
                    0 => {
                        p.text(pos2(r.left() + 36.0, r.center().y), Align2::CENTER_CENTER, "↓", bold(13.0), t.accent);
                    }
                    _ => {}
                }
                let mut x = r.left() + lead;
                let cells: Vec<String> = if detailed {
                    vec![task.subject.clone(), task.status.label().to_string(), self.day_shown(task.due), String::new(), task.categories.join(", ")]
                } else {
                    vec![task.subject.clone(), self.day_shown(task.due), folder_names.get(row).cloned().unwrap_or_default()]
                };
                for (n, ((_, share), cell)) in columns.iter().zip(cells).enumerate() {
                    let w = width * share - 10.0;
                    if detailed && n == 3 {
                        // How far along it is, as a bar with the number beside it.
                        let bar = Rect::from_min_size(pos2(x, r.center().y - 4.0), vec2((w - 36.0).max(10.0), 8.0));
                        p.rect_filled(bar, 4.0, t.line);
                        let filled = bar.width() * f32::from(task.percent.min(100)) / 100.0;
                        p.rect_filled(Rect::from_min_size(bar.min, vec2(filled, bar.height())), 4.0, if done { t.ok } else { t.accent });
                        p.text(pos2(bar.right() + 4.0, r.center().y), Align2::LEFT_CENTER, format!("{}%", task.percent), font(11.5), t.text_dim);
                    } else {
                        let font = if n == 0 && !done { bold(12.5) } else { font(12.5) };
                        let drawn = clip_text(p, pos2(x, r.center().y - 7.5), w, &one_line(&cell), font, color);
                        if n == 0 && done {
                            p.hline(x..=(x + drawn.min(w)), r.center().y, (1.0, t.text_faint));
                        }
                    }
                    x += width * share;
                }
                let flag = Rect::from_center_size(pos2(r.right() - 16.0, r.center().y), vec2(13.0, 13.0));
                if done {
                    icons::paint(p, flag, Icon::Check, t.ok);
                } else {
                    icons::paint(p, flag, Icon::Flag, if task.flagged_mail { t.flag } else { t.flag.gamma_multiply(0.6) });
                }
                p.hline(r.x_range(), r.bottom() - 0.5, (1.0, t.line));
                if on && scroll_to {
                    resp.scroll_to_me(None);
                }
                if resp.clicked() {
                    chosen = Some(item.key());
                }
                if resp.double_clicked() {
                    opened = Some(item.key());
                }
            }
        });
        if let Some((key, done)) = ticked {
            if done {
                let mut p = key_json(key);
                p["status"] = json!("notStarted");
                self.act("tasks.change", p);
            } else {
                self.act("tasks.markComplete", key_json(key));
            }
        }
        if let Some(key) = chosen.filter(|k| self.session.tasks.selected != Some(*k)) {
            self.act("tasks.select", key_json(key));
        }
        if let Some(key) = opened {
            self.act("ui.task", key_json(key));
        }
    }

    /// The chosen task beside the list: what it is, how far along, and its notes.
    fn task_card(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let today = self.session.today();
        let Some(row) = self.session.tasks.chosen_row().cloned() else { return };
        let task = &row.task;
        let p = ui.painter_at(rect);
        let mut y = rect.top() + 16.0;
        let x = rect.left() + 18.0;
        let w = rect.width() - 36.0;
        clip_text(&p, pos2(x, y), w, &one_line(&task.subject), bold(17.0), t.text);
        y += 32.0;
        let words = due_words(task, today);
        if !words.is_empty() {
            let bar = Rect::from_min_size(pos2(x, y), vec2(w, 26.0));
            let late = task.is_overdue(today);
            p.rect_filled(bar, 3.0, if late { t.danger.gamma_multiply(0.15) } else { t.info_bar });
            p.text(pos2(bar.left() + 10.0, bar.center().y), Align2::LEFT_CENTER, &words, font(12.5), if late { t.danger } else { t.text });
            y += 36.0;
        }
        let mut field = |label: &str, value: String, color: Color32| {
            if value.is_empty() {
                return;
            }
            p.text(pos2(x, y + 8.0), Align2::LEFT_CENTER, label, font(12.0), t.text_dim);
            clip_text(&p, pos2(x + 110.0, y), w - 110.0, &one_line(&value), font(12.5), color);
            y += 24.0;
        };
        if task.flagged_mail {
            let folder = self.session.folder_info(row.store, row.folder).map(|f| f.name.clone()).unwrap_or_default();
            field("Flagged message", format!("in {folder}"), t.flag);
        }
        field("Start date", self.day_shown(task.start), t.text);
        field("Due date", self.day_shown(task.due), if task.is_overdue(today) { t.danger } else { t.text });
        field("Status", task.status.label().to_string(), t.text);
        if !task.flagged_mail {
            field("Priority", IMPORTANCE.iter().find(|(i, _)| *i == task.importance).map_or("Normal", |(_, n)| n).to_string(), t.text);
        }
        field("% Complete", format!("{}%", task.percent), t.text);
        field("Reminder", task.reminder.map(|r| self.local_stamp(r)).unwrap_or_default(), t.text);
        field("Owner", task.owner.clone(), t.text);
        field("Categories", task.categories.join(", "), t.text);
        field("Total work", work_text(task.total_work), t.text);
        field("Actual work", work_text(task.actual_work), t.text);
        field("Companies", task.companies.join(", "), t.text);
        let bar = Rect::from_min_size(pos2(x, y + 4.0), vec2(w, 6.0));
        p.rect_filled(bar, 3.0, t.line);
        p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * f32::from(task.percent.min(100)) / 100.0, 6.0)), 3.0, t.accent);
        y += 22.0;
        if !task.notes.is_empty() {
            p.hline(x..=(x + w), y, (1.0, t.line));
            let notes = Rect::from_min_max(pos2(x, y + 10.0), pos2(x + w, rect.bottom() - 10.0));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(notes));
            egui::ScrollArea::vertical().id_salt("task-notes").auto_shrink([false, false]).show(&mut child, |ui| {
                ui.label(egui::RichText::new(&task.notes).font(font(12.5)).color(t.text));
            });
        }
    }

    /// The one chosen moved `by` places in the order shown, as the arrow keys do.
    pub(crate) fn tasks_step(&mut self, by: isize) {
        let rows: &[Row] = &self.session.tasks.rows;
        if rows.is_empty() {
            return;
        }
        let at = self.session.tasks.selected.and_then(|k| rows.iter().position(|r| r.key() == k));
        let next = match at {
            Some(i) => i.saturating_add_signed(by).min(rows.len() - 1),
            None => 0,
        };
        if let Some(r) = rows.get(next) {
            let key = r.key();
            self.tasks_scroll = true;
            self.act("tasks.select", key_json(key));
        }
    }

    /// The task window, while it is open.
    pub(crate) fn task_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.task.take() else { return };
        let t = self.tokens.clone();
        let mut ask = None;
        let title = match (form.key, form.flagged_mail) {
            (Some(_), true) => format!("{} - Follow Up", one_line(&form.subject_was)),
            (Some(_), false) if !form.subject_was.trim().is_empty() => format!("{} - Task", one_line(&form.subject_was)),
            _ => "Untitled - Task".to_string(),
        };
        let room = (ctx.content_rect().height() - 320.0).clamp(160.0, 560.0);
        let shown = Dialog::new("task", &title, Icon::Tasks).width(640.0).show(ctx, &t, |ui| {
            if !form.due_words.is_empty() {
                ui.label(egui::RichText::new(&form.due_words).font(font(12.5)).color(t.text_dim));
                ui.add_space(4.0);
            }
            egui::ScrollArea::vertical().id_salt("task-form").max_height(room).show(ui, |ui| {
                egui::Grid::new("task-grid").num_columns(4).spacing(vec2(10.0, 7.0)).show(ui, |ui| {
                    let dim = |ui: &mut Ui, s: &str| ui.label(egui::RichText::new(s).color(t.text_dim));
                    dim(ui, if form.flagged_mail { "To-do title" } else { "Subject" });
                    let subject = ui.add(LineEdit::new(&mut form.subject, egui::Id::new("task-subject")).desired_width(470.0).char_limit(4000));
                    if std::mem::take(&mut form.focus) {
                        subject.request_focus();
                    }
                    ui.label("");
                    ui.label("");
                    ui.end_row();
                    dim(ui, "Start date");
                    ui.add(LineEdit::new(&mut form.start, egui::Id::new("task-start")).desired_width(150.0).char_limit(20).hint("YYYY-MM-DD", None));
                    dim(ui, "Status");
                    let before = form.status;
                    egui::ComboBox::from_id_salt("task-status").selected_text(form.status.label()).width(190.0).show_ui(ui, |ui| {
                        for s in STATUSES {
                            ui.selectable_value(&mut form.status, s, s.label());
                        }
                    });
                    if form.status != before {
                        form.status_changed();
                    }
                    ui.end_row();
                    dim(ui, "Due date");
                    ui.add(LineEdit::new(&mut form.due, egui::Id::new("task-due")).desired_width(150.0).char_limit(20).hint("YYYY-MM-DD", None));
                    dim(ui, "% Complete");
                    let before = form.percent;
                    ui.add(egui::Slider::new(&mut form.percent, 0..=100).step_by(5.0).suffix("%"));
                    if form.percent != before {
                        form.percent_changed();
                    }
                    ui.end_row();
                    dim(ui, "Reminder");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut form.reminder_on, "");
                        ui.add_enabled(
                            form.reminder_on,
                            LineEdit::new(&mut form.reminder, egui::Id::new("task-reminder"))
                                .desired_width(130.0)
                                .char_limit(20)
                                .hint("YYYY-MM-DD HH:MM", None),
                        );
                    });
                    if !form.flagged_mail {
                        dim(ui, "Priority");
                        egui::ComboBox::from_id_salt("task-priority")
                            .selected_text(IMPORTANCE.iter().find(|(i, _)| *i == form.importance).map_or("Normal", |(_, n)| n))
                            .width(190.0)
                            .show_ui(ui, |ui| {
                                for (i, name) in IMPORTANCE {
                                    ui.selectable_value(&mut form.importance, i, name);
                                }
                            });
                    }
                    ui.end_row();
                    if !form.flagged_mail {
                        dim(ui, "Owner");
                        ui.add(LineEdit::new(&mut form.owner, egui::Id::new("task-owner")).desired_width(150.0).char_limit(500));
                        dim(ui, "Categories");
                        ui.add(
                            LineEdit::new(&mut form.categories, egui::Id::new("task-categories"))
                                .desired_width(190.0)
                                .char_limit(1000)
                                .hint("Red category", None),
                        );
                        ui.end_row();
                    }
                });
                if !form.flagged_mail {
                    ui.add_space(8.0);
                    ui.add(
                        egui::TextEdit::multiline(&mut form.notes)
                            .desired_rows(7)
                            .desired_width(f32::INFINITY)
                            .char_limit(200_000)
                            .hint_text("Notes"),
                    );
                    ui.add_space(6.0);
                    egui::CollapsingHeader::new(egui::RichText::new("Details").font(bold(12.5)).color(t.text)).id_salt("task-details").show(
                        ui,
                        |ui| {
                            egui::Grid::new("task-details-grid").num_columns(4).spacing(vec2(10.0, 7.0)).show(ui, |ui| {
                                let dim = |ui: &mut Ui, s: &str| ui.label(egui::RichText::new(s).color(t.text_dim));
                                dim(ui, "Total work");
                                ui.add(
                                    LineEdit::new(&mut form.total_work, egui::Id::new("task-total"))
                                        .desired_width(130.0)
                                        .char_limit(40)
                                        .hint("4 hours", None),
                                );
                                dim(ui, "Actual work");
                                ui.add(
                                    LineEdit::new(&mut form.actual_work, egui::Id::new("task-actual"))
                                        .desired_width(130.0)
                                        .char_limit(40)
                                        .hint("2 days", None),
                                );
                                ui.end_row();
                                dim(ui, "Mileage");
                                ui.add(LineEdit::new(&mut form.mileage, egui::Id::new("task-mileage")).desired_width(130.0).char_limit(500));
                                dim(ui, "Billing");
                                ui.add(LineEdit::new(&mut form.billing, egui::Id::new("task-billing")).desired_width(130.0).char_limit(500));
                                ui.end_row();
                                dim(ui, "Companies");
                                ui.add(LineEdit::new(&mut form.companies, egui::Id::new("task-companies")).desired_width(130.0).char_limit(2000));
                                ui.checkbox(&mut form.private, "Private");
                                ui.end_row();
                            });
                        },
                    );
                }
            });
            if let Some(said) = &form.said {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(one_line(said)).font(font(12.5)).color(t.danger));
            }
            dialog::foot(ui, &t, |ui| {
                if form.key.is_some()
                    && !form.flagged_mail
                    && ui.add(dialog::danger(&t, "Delete")).on_hover_text("Moves it to Deleted Items").clicked()
                {
                    ask = Some(Ask::Delete);
                }
                if form.key.is_some() && form.status != Status::Complete && ui.add(dialog::plain("Mark Complete")).clicked() {
                    ask = Some(Ask::Complete);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(dialog::plain("Cancel")).clicked() {
                        ask = Some(Ask::Close);
                    }
                    if ui.add(dialog::primary(&t, "Save & Close")).clicked() {
                        ask = Some(Ask::Save);
                    }
                });
            });
        });
        if shown.closed || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ask = Some(Ask::Close);
        }
        let result = match ask {
            None => {
                self.task = Some(form);
                return;
            }
            Some(Ask::Close) => return,
            Some(Ask::Save) => match form.params() {
                Ok(p) => self.execute(if form.key.is_some() { "tasks.change" } else { "tasks.new" }, &p),
                Err(e) => Err(e),
            },
            Some(Ask::Complete) => match form.params() {
                Ok(p) => self.execute("tasks.change", &p).and_then(|_| self.execute("tasks.markComplete", &form.key.map_or(Value::Null, key_json))),
                Err(e) => Err(e),
            },
            Some(Ask::Delete) => self.execute("tasks.delete", &form.key.map_or(Value::Null, key_json)),
        };
        match result {
            Ok(_) => self.tasks_scroll = true,
            Err(e) => {
                form.said = Some(e);
                self.task = Some(form);
            }
        }
    }
}
