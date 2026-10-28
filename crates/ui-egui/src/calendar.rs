//! The Calendar module: a month to choose days from, the days shown hour by hour or as a month,
//! and the window an appointment is made or changed in. What is shown is the engine's
//! (`Session::calendar`); every change goes through the `calendar.*` commands.

use crate::icons::{self, Icon};
use crate::line_edit::LineEdit;
use crate::{MailUi, bold, clip_text, font, one_line, text_width};
use egui::{Align2, Color32, Painter, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use openmailclient_model::calendar::{self, Appointment, Busy, DAY, MINUTE, View};
use openmailclient_model::meeting::{Meeting, Role};
use openmailclient_model::recur::{self, End, Freq, Rule};
use openmailclient_model::time::{self, Civil};
use openmailclient_model::zone::Zone;
use serde_json::{Value, json};

/// The height of an hour in the day views; half hours are marked between.
const HOUR_H: f32 = 44.0;
/// The column of hours at the left of the day views.
const GUTTER: f32 = 58.0;
/// The row naming each day.
const DAY_HEAD: f32 = 44.0;
/// One row of the band of all-day events.
const BAND_ROW: f32 = 22.0;
/// All-day events shown above the hours before the rest are counted.
const BAND_ROWS: usize = 4;
/// The hours the working day is shaded for, and the first shown.
const WORK: (u32, u32) = (8, 17);
/// One line of an appointment in a month's day.
const MONTH_LINE: f32 = 18.0;

/// The date a local day number is.
fn date_of(day: i64) -> Civil {
    time::civil(calendar::day_start(day, 0), 0)
}

/// Short weekday names, Sunday first.
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// How often an appointment being made or changed repeats, as chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Repeats {
    Never,
    Daily,
    Weekdays,
    Weekly,
    Monthly,
    Yearly,
}

const REPEATS: &[(Repeats, &str)] = &[
    (Repeats::Never, "Does not repeat"),
    (Repeats::Daily, "Daily"),
    (Repeats::Weekdays, "Every weekday"),
    (Repeats::Weekly, "Weekly"),
    (Repeats::Monthly, "Monthly"),
    (Repeats::Yearly, "Yearly"),
];

/// Which day of a month a monthly or yearly series falls on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MonthBy {
    /// Day `day` of the month.
    Date,
    /// The `nth` of `nth_days`, as "the second Tuesday".
    Weekday,
    LastDay,
}

/// When a series ends, as chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ends {
    Never,
    After,
    On,
}

/// Which days the nth of a month counts: any day, weekdays, weekend days or one weekday.
const NTH_DAYS: &[(u8, &str)] = &[
    (0x7F, "day"),
    (0x3E, "weekday"),
    (0x41, "weekend day"),
    (1, "Sunday"),
    (2, "Monday"),
    (4, "Tuesday"),
    (8, "Wednesday"),
    (16, "Thursday"),
    (32, "Friday"),
    (64, "Saturday"),
];
const NTH: &[(u32, &str)] = &[(1, "first"), (2, "second"), (3, "third"), (4, "fourth"), (5, "last")];
const DAY_NAMES: [&str; 7] = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];

/// How an appointment repeats, as typed in its window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RepeatForm {
    pub(crate) repeats: Repeats,
    /// Days, weeks or months between.
    pub(crate) interval: u32,
    /// A weekly series' days, bit 0 being Sunday.
    pub(crate) days: u8,
    pub(crate) month_by: MonthBy,
    pub(crate) day: u32,
    pub(crate) nth: u32,
    pub(crate) nth_days: u8,
    pub(crate) ends: Ends,
    pub(crate) count: u32,
    pub(crate) until: String,
}

impl RepeatForm {
    /// How `a` repeats, or how it would by default from its start.
    fn of(a: &Appointment, zone: &Zone) -> Self {
        let c = zone.civil(a.start);
        let weekday = 1u8 << (c.weekday % 7);
        let mut f = Self {
            repeats: Repeats::Never,
            interval: 1,
            days: weekday,
            month_by: MonthBy::Date,
            day: c.day,
            nth: (c.day.saturating_sub(1) / 7 + 1).min(5),
            nth_days: weekday,
            ends: Ends::Never,
            count: 10,
            until: ymd(&c),
        };
        let Some(r) = &a.recurrence else { return f };
        f.interval = r.period.max(1);
        f.repeats = match (r.freq, &r.rule) {
            (_, Rule::Day) => Repeats::Daily,
            (_, Rule::Week { days }) if *days == 0x3E && r.period <= 1 => Repeats::Weekdays,
            (_, Rule::Week { .. }) => Repeats::Weekly,
            (Freq::Yearly, _) => Repeats::Yearly,
            (_, Rule::Other { .. }) => Repeats::Never,
            _ => Repeats::Monthly,
        };
        match &r.rule {
            Rule::Week { days } => f.days = *days,
            Rule::Month { day } => (f.month_by, f.day) = (MonthBy::Date, *day),
            Rule::MonthNth { days, nth } => (f.month_by, f.nth_days, f.nth) = (MonthBy::Weekday, *days, (*nth).clamp(1, 5)),
            Rule::MonthEnd => f.month_by = MonthBy::LastDay,
            Rule::Day | Rule::Other { .. } => {}
        }
        match r.end {
            End::Never => {}
            End::After(n) => (f.ends, f.count) = (Ends::After, n),
            End::On(day) => (f.ends, f.until) = (Ends::On, ymd(&date_of(day))),
        }
        f
    }

    /// The "repeat" `calendar.new`, `calendar.change` and `calendar.recurrence` take.
    pub(crate) fn param(&self) -> Value {
        let names = |mask: u8| -> Vec<&str> { (0..7).filter(|i| mask & (1 << i) != 0).filter_map(|i| DAY_NAMES.get(i).copied()).collect() };
        let mut p = match self.repeats {
            Repeats::Never => return Value::Null,
            Repeats::Daily => json!({ "every": "day", "interval": self.interval }),
            Repeats::Weekdays => json!({ "every": "weekday" }),
            Repeats::Weekly => json!({ "every": "week", "interval": self.interval, "on": names(self.days) }),
            Repeats::Monthly => json!({ "every": "month", "interval": self.interval }),
            Repeats::Yearly => json!({ "every": "year" }),
        };
        if matches!(self.repeats, Repeats::Monthly | Repeats::Yearly) {
            match self.month_by {
                MonthBy::Date => p["day"] = json!(self.day),
                MonthBy::Weekday => {
                    p["nth"] = if self.nth >= 5 { json!("last") } else { json!(self.nth) };
                    p["on"] = json!(names(self.nth_days));
                }
                MonthBy::LastDay => p["lastDay"] = json!(true),
            }
        }
        match self.ends {
            Ends::Never => {}
            Ends::After => p["count"] = json!(self.count),
            Ends::On => p["until"] = json!(self.until.trim()),
        }
        p
    }
}

/// An appointment being made or changed, as typed.
#[derive(Clone, Debug)]
pub(crate) struct AppointmentForm {
    pub(crate) id: Option<u64>,
    pub(crate) subject: String,
    pub(crate) location: String,
    pub(crate) start_date: String,
    pub(crate) start_time: String,
    pub(crate) end_date: String,
    pub(crate) end_time: String,
    pub(crate) all_day: bool,
    pub(crate) busy: Busy,
    pub(crate) reminder: Option<i32>,
    /// What went wrong with the last Save.
    pub(crate) said: Option<String>,
    pub(crate) focus: bool,
    /// One occurrence of a series alone: the day it was first planned on.
    pub(crate) occurrence: Option<i64>,
    /// It is a series, or one of one.
    pub(crate) recurring: bool,
    /// The form a window opened on one occurrence can be switched to for the whole series, or
    /// back; kept as typed.
    pub(crate) other: Option<Box<AppointmentForm>>,
    pub(crate) repeat: RepeatForm,
    /// How it repeated when the window opened: the pattern is sent only when it was changed,
    /// so that occurrences changed on their own stay so.
    pub(crate) repeat_was: RepeatForm,
    /// The pattern in words, as it is written for the series.
    pub(crate) pattern: String,
    /// The time zone it is kept in and its times are typed in, by name; empty for none, when
    /// they are typed where we are. One occurrence alone is always typed where we are.
    pub(crate) zone: String,
    /// The meeting it is, as kept when the window opened; `None` for an appointment.
    pub(crate) meeting: Option<Meeting>,
    /// Those asked, typed by role: required, optional, resources; and as they were.
    pub(crate) asked: [String; 3],
    pub(crate) asked_was: [String; 3],
    /// Those asked are shown to be typed in: a meeting one organizes, or Invite Attendees.
    pub(crate) inviting: bool,
}

const REMINDERS: &[(Option<i32>, &str)] = &[
    (None, "None"),
    (Some(0), "0 minutes"),
    (Some(5), "5 minutes"),
    (Some(10), "10 minutes"),
    (Some(15), "15 minutes"),
    (Some(30), "30 minutes"),
    (Some(60), "1 hour"),
    (Some(120), "2 hours"),
    (Some(1440), "1 day"),
    (Some(10080), "1 week"),
];

const SHOW_AS: &[(Busy, &str)] = &[
    (Busy::Free, "Free"),
    (Busy::WorkingElsewhere, "Working Elsewhere"),
    (Busy::Tentative, "Tentative"),
    (Busy::Busy, "Busy"),
    (Busy::OutOfOffice, "Out of Office"),
];

fn show_as(b: Busy) -> &'static str {
    SHOW_AS.iter().find(|(x, _)| *x == b).map_or("Busy", |(_, n)| n)
}

fn reminder_words(r: Option<i32>) -> String {
    match REMINDERS.iter().find(|(x, _)| *x == r) {
        Some((_, n)) => (*n).to_string(),
        None => format!("{} minutes", r.unwrap_or(0)),
    }
}

fn ymd(c: &Civil) -> String {
    format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
}

fn hm(c: &Civil) -> String {
    format!("{:02}:{:02}", c.hour, c.minute)
}

/// How long, in words: "30 minutes", "1 hour 30 minutes", "3 days".
fn length_words(minutes: i64) -> String {
    let (d, h, m) = (minutes / 1440, minutes % 1440 / 60, minutes % 60);
    let part = |n: i64, one: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") };
    let mut out: Vec<String> = Vec::new();
    if d > 0 {
        out.push(part(d, "day"));
    }
    if h > 0 {
        out.push(part(h, "hour"));
    }
    if m > 0 || out.is_empty() {
        out.push(part(m, "minute"));
    }
    out.join(" ")
}

impl AppointmentForm {
    /// A new appointment from `start` (FILETIME), half an hour long, or all day.
    fn new_at(start: i64, all_day: bool, here: &Zone) -> Self {
        let end = if all_day { start.saturating_add(DAY) } else { start.saturating_add(30 * MINUTE) };
        let a = Appointment {
            subject: String::new(),
            location: String::new(),
            start,
            end,
            all_day,
            busy: if all_day { Busy::Free } else { Busy::Busy },
            reminder: if all_day { None } else { Some(15) },
            recurring: false,
            meeting: false,
            recurrence: None,
            occurrence: None,
            // Made where we are, when that is a zone known by name.
            zone: (!all_day && !here.name().is_empty()).then(|| here.clone()),
        };
        Self::of(None, &a, here)
    }

    pub(crate) fn of(id: Option<u64>, a: &Appointment, here: &Zone) -> Self {
        // Its times as they are in its own zone, as they were typed; one occurrence alone, or
        // an all-day event, as they are here.
        let own = a.zone.as_ref().filter(|z| a.occurrence.is_none() && !a.all_day && !z.name().is_empty());
        let zone = own.unwrap_or(here);
        let s = zone.civil(a.start);
        // An all-day event's last day is the one before the midnight it ends at.
        let e = zone.civil(if a.all_day { a.end.saturating_sub(DAY).max(a.start) } else { a.end });
        Self {
            id,
            subject: one_line(&a.subject),
            location: one_line(&a.location),
            start_date: ymd(&s),
            start_time: hm(&s),
            end_date: ymd(&e),
            end_time: hm(&e),
            all_day: a.all_day,
            busy: a.busy,
            reminder: a.reminder,
            said: None,
            focus: true,
            occurrence: a.occurrence,
            recurring: a.recurrence.is_some() || a.recurring,
            other: None,
            repeat: RepeatForm::of(a, zone),
            repeat_was: RepeatForm::of(a, zone),
            pattern: a.recurrence.as_ref().map(|_| a.pattern_words()).unwrap_or_default(),
            zone: a.zone.as_ref().filter(|_| a.occurrence.is_none()).map(|z| z.name().to_string()).unwrap_or_default(),
            meeting: None,
            asked: Default::default(),
            asked_was: Default::default(),
            inviting: false,
        }
    }

    /// The form made to show `m`, the meeting it is.
    pub(crate) fn with_meeting(mut self, m: Option<Meeting>) -> Self {
        self.asked = crate::meeting::lines(m.as_ref());
        self.asked_was = self.asked.clone();
        self.inviting = m.as_ref().is_some_and(|m| !m.received);
        self.meeting = m;
        self
    }

    /// One asked to it by someone else: its organizer says who comes.
    pub(crate) fn received(&self) -> bool {
        self.meeting.as_ref().is_some_and(|m| m.received)
    }

    /// Someone is typed in as asked.
    pub(crate) fn asks_anyone(&self) -> bool {
        self.inviting && self.asked.iter().any(|l| l.contains('@'))
    }

    /// The parameters `calendar.new` and `calendar.change` take.
    pub(crate) fn params(&self) -> Value {
        let at = |d: &str, t: &str| if self.all_day { d.trim().to_string() } else { format!("{} {}", d.trim(), t.trim()) };
        let mut p = json!({
            "subject": self.subject,
            "location": self.location,
            "start": at(&self.start_date, &self.start_time),
            "end": at(&self.end_date, &self.end_time),
            "allDay": self.all_day,
            "busy": self.busy.name(),
            "reminder": self.reminder,
        });
        if let Some(id) = self.id {
            p["id"] = json!(id);
        }
        match self.occurrence {
            Some(day) => p["occurrence"] = json!(ymd(&date_of(day))),
            None if self.id.is_some() => p["series"] = json!(true),
            None => {}
        }
        if self.occurrence.is_none() && self.repeat != self.repeat_was {
            p["repeat"] = self.repeat.param();
        }
        if self.occurrence.is_none() {
            p["zone"] = if self.zone.is_empty() { Value::Null } else { json!(self.zone) };
        }
        // Those asked, when they were typed or changed; the whole series' alone.
        if self.occurrence.is_none() && self.inviting && !self.received() && (self.asked != self.asked_was || self.id.is_none()) {
            p["attendees"] = crate::meeting::lines_param(&self.asked);
        }
        p
    }

    /// The zone its times are typed in: its own, or `here`.
    pub(crate) fn typed_in(&self, here: &Zone) -> Zone {
        match self.occurrence {
            None if !self.zone.is_empty() && !self.all_day => Zone::named(&self.zone).unwrap_or_else(|| here.clone()),
            _ => here.clone(),
        }
    }

    /// What Delete in its window deletes: the occurrence alone, or the whole appointment.
    pub(crate) fn delete_params(&self) -> Value {
        match self.occurrence {
            Some(day) => json!({ "id": self.id, "occurrence": ymd(&date_of(day)) }),
            None => json!({ "id": self.id, "series": true }),
        }
    }

    /// The window switched between the occurrence and the whole series.
    pub(crate) fn switched(mut self) -> Self {
        match self.other.take() {
            Some(mut other) => {
                self.focus = false;
                other.other = Some(Box::new(self));
                other.focus = false;
                *other
            }
            None => self,
        }
    }

    /// Start and end as typed, when both read as times.
    fn span(&self, here: &Zone) -> Option<(i64, i64)> {
        let offset = &self.typed_in(here);
        let start =
            calendar::parse_local(&format!("{} {}", self.start_date.trim(), if self.all_day { "00:00" } else { self.start_time.trim() }), offset)?;
        let end = if self.all_day {
            calendar::parse_local(self.end_date.trim(), offset)?.saturating_add(DAY)
        } else {
            calendar::parse_local(&format!("{} {}", self.end_date.trim(), self.end_time.trim()), offset)?
        };
        Some((start, end))
    }
}

/// What a click in the appointment window asked for.
enum Ask {
    Save,
    Delete,
    Close,
    /// Saved, and its invitations or update sent.
    Send,
    /// Everyone asked told it is off, and it goes.
    CancelMeeting,
    /// One asked answers: accept, tentative or decline.
    Respond(&'static str),
}

impl MailUi {
    /// The Calendar module's whole body.
    pub(crate) fn calendar_body(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        ui.painter_at(rect).rect_filled(rect, 0.0, t.surface);
        let mut left = rect.left();
        if self.session.folder_pane {
            let w = self.folder_w.clamp(200.0, 280.0).min(rect.width() * 0.4);
            self.calendar_side(ui, Rect::from_min_size(rect.min, vec2(w, rect.height())));
            left += w;
            ui.painter_at(rect).vline(left + 0.5, rect.y_range(), (1.0, t.line));
        }
        let area = Rect::from_min_max(pos2(left + 1.0, rect.top()), rect.max);
        let head = Rect::from_min_size(area.min, vec2(area.width(), 40.0));
        self.calendar_head(ui, head);
        let days = self.session.calendar_days();
        let grid = Rect::from_min_max(pos2(area.left(), head.bottom()), area.max);
        if self.session.calendar.view == View::Month {
            self.month_grid(ui, grid, &days);
        } else {
            self.time_grid(ui, grid, &days);
        }
    }

    /// The month to choose a day from, and the calendars there are.
    fn calendar_side(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.pane);
        let s = &self.session;
        let today = s.today();
        let chosen = if s.calendar.day == 0 { today } else { s.calendar.day };
        let shown = s.calendar_days();
        let week_start = s.region.week_start;
        let picked = month_picker(
            ui,
            &t,
            &mut self.cal_nav,
            Picker { rect, top: rect.top() + 12.0, today, chosen, shown: &shown, week_start, salt: "cal-nav" },
        );
        let y0 = picked.bottom;
        if let Some(day) = picked.day {
            self.cal_nav = None;
            self.go_to_day(day);
        }

        // Every calendar of the data files open, those kept in step with a server in its
        // colour; a click shows that one.
        let y = y0 + 22.0;
        p.text(pos2(rect.left() + 14.0, y), Align2::LEFT_CENTER, "My Calendars", bold(12.5), t.text);
        let calendars = openmailclient_engine::cmd::calendar::calendars(&self.session);
        let mut y = y + 14.0;
        if calendars.is_empty() {
            clip_text(&p, pos2(rect.left() + 26.0, y - 1.0), rect.width() - 36.0, "No calendar in the data files open", font(12.0), t.text_dim);
            y += 20.0;
        }
        // The data file each is in, only where they are in more than one.
        let files = calendars.first().is_some_and(|f| calendars.iter().any(|c| c["store"] != f["store"]));
        let mut open = None;
        for c in &calendars {
            if y + 16.0 > rect.bottom() {
                break;
            }
            let (Some(store), Some(folder)) = (c["store"].as_u64(), c["folder"].as_u64()) else { continue };
            let shown = c["shown"].as_bool() == Some(true);
            let row = Rect::from_min_size(pos2(rect.left() + 18.0, y - 3.0), vec2(rect.width() - 26.0, 18.0));
            let resp = ui.interact(row, egui::Id::new(("cal-list", store, folder)), Sense::click());
            if resp.hovered() {
                p.rect_filled(row, 3.0, t.hover);
            }
            let (color, mark) = match c["color"].as_str().and_then(hex_color) {
                Some(c) => (c, if luminance(c) > 0.5 { Color32::BLACK } else { Color32::WHITE }),
                None => (t.accent, t.on_accent),
            };
            let r = Rect::from_min_size(pos2(rect.left() + 26.0, y), vec2(12.0, 12.0));
            if shown {
                p.rect_filled(r, 2.0, color);
                icons::paint(&p, r.shrink(1.5), Icon::Check, mark);
            } else {
                p.rect_stroke(r, 2.0, (1.5, color), egui::StrokeKind::Inside);
            }
            let name = format!("{} - {}", one_line(c["name"].as_str().unwrap_or("")), one_line(c["file"].as_str().unwrap_or("")));
            let words = if files { name.clone() } else { one_line(c["name"].as_str().unwrap_or("")) };
            clip_text(
                &p,
                pos2(r.right() + 8.0, r.top() - 1.0),
                rect.right() - r.right() - 18.0,
                &words,
                font(12.0),
                if shown { t.text } else { t.text_dim },
            );
            let read_only = c["readOnly"].as_bool() == Some(true);
            let hover = match (c["account"].is_string(), read_only) {
                (true, true) => format!(
                    "{name}
Kept in step with a calendar server, which lets it be read only"
                ),
                (true, false) => format!(
                    "{name}
Kept in step with a calendar server at each Send/Receive"
                ),
                _ => name,
            };
            if resp.on_hover_text(hover).clicked() && !shown {
                open = Some((store, folder));
            }
            y += 20.0;
        }
        if let Some((store, folder)) = open {
            self.act("calendar.open", json!({ "store": store, "folder": folder }));
        }
        if self.session.calendar.unreadable > 0 {
            let words = format!("{} items could not be read", self.session.calendar.unreadable);
            clip_text(&p, pos2(rect.left() + 26.0, y + 2.0), rect.width() - 36.0, &words, font(11.0), t.danger);
        }
    }

    pub(crate) fn go_to_day(&mut self, day: i64) {
        let c = date_of(day);
        self.act("calendar.goTo", json!({ "date": ymd(&c) }));
        self.cal_scroll = true;
    }

    /// Back and forward, and which days these are.
    fn calendar_head(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.hline(rect.x_range(), rect.bottom() - 0.5, (1.0, t.line));
        let mut x = rect.left() + 12.0;
        for (back, cmd) in [(true, "calendar.previous"), (false, "calendar.next")] {
            let r = Rect::from_min_size(pos2(x, rect.center().y - 13.0), vec2(26.0, 26.0));
            let resp = ui.interact(r, egui::Id::new(("cal-step", back)), Sense::click());
            if resp.hovered() {
                p.rect_filled(r, 4.0, t.hover);
            }
            chevron(&p, r.center(), back, t.text);
            if resp.clicked() {
                self.act(cmd, Value::Null);
            }
            resp.on_hover_text(if back { "Back (Alt+Up)" } else { "Forward (Alt+Down)" });
            x += 28.0;
        }
        let title = self.range_title();
        p.text(pos2(x + 10.0, rect.center().y), Align2::LEFT_CENTER, title, font(18.0), t.text);
    }

    /// The days shown, in words: "October 4 - 10, 2026", "October 2026".
    fn range_title(&self) -> String {
        let s = &self.session;
        let days = s.calendar_days();
        let (Some(first), Some(last)) = (days.first().copied(), days.last().copied()) else { return String::new() };
        let (a, b) = (date_of(first), date_of(last));
        match s.calendar.view {
            View::Day => s.region.long_date(&a),
            View::Month => {
                let c = date_of(if s.calendar.day == 0 { s.today() } else { s.calendar.day });
                format!("{} {}", c.month_name(), c.year)
            }
            _ if a.year != b.year => format!("{} {}, {} - {} {}, {}", a.month_name(), a.day, a.year, b.month_name(), b.day, b.year),
            _ if a.month != b.month => format!("{} {} - {} {}, {}", a.month_name(), a.day, b.month_name(), b.day, b.year),
            _ => format!("{} {} - {}, {}", a.month_name(), a.day, b.day, b.year),
        }
    }

    /// An hour as the gutter names it: "8 AM", or "08:00" where times are written on 24 hours.
    fn hour_name(&self, hour: u32) -> String {
        self.clock_name(hour, 0)
    }

    /// A time as the gutter names it: "8 AM" or "8:30 AM", or "08:30" on 24 hours.
    pub(crate) fn clock_name(&self, hour: u32, minute: u32) -> String {
        let r = &self.session.region;
        let (hour, minute) = (hour % 24, minute % 60);
        if r.short_time.contains('h') {
            let h = if hour.is_multiple_of(12) { 12 } else { hour % 12 };
            let ampm = if hour < 12 { &r.am } else { &r.pm };
            if minute == 0 { format!("{h} {ampm}") } else { format!("{h}:{minute:02} {ampm}") }
        } else {
            format!("{hour:02}:{minute:02}")
        }
    }

    /// The day, work week and week views: a column of hours for each day, all-day events above.
    fn time_grid(&mut self, ui: &mut Ui, rect: Rect, days: &[i64]) {
        let t = self.tokens.clone();
        let off = &self.session.zone.clone();
        let today = self.session.today();
        let n = days.len().max(1);
        // A second zone's hours stand in a column of their own, left of ours.
        let second = self.session.second_zone.clone().filter(|z| z.name() != off.name());
        let gutter = if second.is_some() { GUTTER * 2.0 } else { GUTTER };
        let col_w = ((rect.width() - gutter - 14.0) / n as f32).max(20.0);
        let col_x = |i: usize| rect.left() + gutter + col_w * i as f32;
        let items = self.session.calendar.items.clone();
        let (selected, occurrence) = (self.session.calendar.selected, self.session.calendar.occurrence);
        let chosen = |id: u64, a: &Appointment| selected == Some(id) && (occurrence.is_none() || a.occurrence == occurrence);
        let first = days.first().copied().unwrap_or(today);
        let last = days.last().copied().unwrap_or(first);
        let shown_from = calendar::day_start(first, off);
        let shown_to = calendar::day_start(last.saturating_add(1), off);

        // The band: all-day events and anything a day or longer, each in the first free row.
        let banded: Vec<&(u64, Appointment)> = items.iter().filter(|(_, a)| a.all_day || a.end.saturating_sub(a.start) >= DAY).collect();
        let mut rows: Vec<Vec<(usize, usize)>> = Vec::new();
        let mut placed: Vec<(usize, usize, usize, &(u64, Appointment))> = Vec::new();
        let mut hidden = vec![0usize; n];
        for item in &banded {
            let a = &item.1;
            let from = calendar::day_of(a.start.max(shown_from), off);
            let to = calendar::day_of(a.end.min(shown_to).saturating_sub(1).max(a.start.max(shown_from)), off);
            let (Some(c0), Some(c1)) = (days.iter().position(|d| *d == from), days.iter().position(|d| *d == to).or(Some(n - 1))) else {
                continue;
            };
            let row = rows.iter().position(|r| r.iter().all(|&(s, e)| c1 < s || c0 > e)).unwrap_or(rows.len());
            if row >= BAND_ROWS {
                for h in hidden.iter_mut().take(c1 + 1).skip(c0) {
                    *h += 1;
                }
                continue;
            }
            if row == rows.len() {
                rows.push(Vec::new());
            }
            if let Some(r) = rows.get_mut(row) {
                r.push((c0, c1));
            }
            placed.push((row, c0, c1, item));
        }
        let band_h = BAND_ROW * rows.len().max(1) as f32 + 6.0 + if hidden.iter().any(|h| *h > 0) { 14.0 } else { 0.0 };

        let p = ui.painter_at(rect);
        // The days' names.
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), DAY_HEAD));
        for (i, day) in days.iter().enumerate() {
            let c = date_of(*day);
            let x = col_x(i);
            let is_today = *day == today;
            let name = WEEKDAYS.get(usize::try_from(c.weekday).unwrap_or(0)).copied().unwrap_or("");
            p.text(pos2(x + 8.0, head.top() + 12.0), Align2::LEFT_CENTER, name, font(11.5), if is_today { t.accent } else { t.text_dim });
            p.text(
                pos2(x + 8.0, head.top() + 30.0),
                Align2::LEFT_CENTER,
                c.day.to_string(),
                if is_today { bold(17.0) } else { font(17.0) },
                if is_today { t.accent } else { t.text },
            );
            if is_today {
                p.rect_filled(Rect::from_min_size(pos2(x + 1.0, head.bottom() - 3.0), vec2(col_w - 2.0, 3.0)), 1.0, t.accent);
            }
            p.vline(x + 0.5, head.top() + 6.0..=head.bottom() + band_h, (1.0, t.line));
            let resp =
                ui.interact(Rect::from_min_size(pos2(x, head.top()), vec2(col_w, DAY_HEAD)), egui::Id::new(("cal-day-head", *day)), Sense::click());
            if resp.clicked() && self.session.calendar.view != View::Day {
                // A day's name opens that day alone.
                self.act("calendar.view", json!({ "view": "day" }));
                self.go_to_day(*day);
            }
        }
        if let Some(z) = &second {
            // Which column is which zone's, named as the place or the offset.
            let at = calendar::day_start(days.first().copied().unwrap_or(today), off);
            for (x, zone) in [(rect.left() + GUTTER - 8.0, z), (rect.left() + gutter - 8.0, off)] {
                let name: String = zone.short(at).chars().take(12).collect();
                p.text(pos2(x, head.bottom() - 10.0), Align2::RIGHT_CENTER, name, font(10.0), t.text_dim);
            }
        }
        let band = Rect::from_min_size(pos2(rect.left(), head.bottom()), vec2(rect.width(), band_h));
        p.hline(rect.x_range(), band.top() + 0.5, (1.0, t.line));
        p.hline(rect.x_range(), band.bottom() - 0.5, (1.0, t.line));
        let mut band_open: Option<(u64, Option<i64>)> = None;
        let mut band_select: Option<(u64, Option<i64>)> = None;
        for (row, c0, c1, (id, a)) in &placed {
            let r = Rect::from_min_max(
                pos2(col_x(*c0) + 3.0, band.top() + 3.0 + BAND_ROW * *row as f32),
                pos2(col_x(*c1 + 1) - 3.0, band.top() + 1.0 + BAND_ROW * (*row as f32 + 1.0)),
            );
            let resp = self.appointment_box(ui, &p, r, *id, a, chosen(*id, a), "band");
            if resp.double_clicked() {
                band_open = Some((*id, a.occurrence));
            } else if resp.clicked() {
                band_select = Some((*id, a.occurrence));
            }
        }
        for (i, h) in hidden.iter().enumerate() {
            if *h > 0 {
                p.text(pos2(col_x(i) + 6.0, band.bottom() - 9.0), Align2::LEFT_CENTER, format!("{h} more"), font(10.5), t.text_dim);
            }
        }

        // The hours.
        let hours = Rect::from_min_max(pos2(rect.left(), band.bottom()), rect.max);
        let scroll_to = std::mem::take(&mut self.cal_scroll);
        let mut new_at: Option<i64> = None;
        let mut clicked_empty = false;
        let mut open: Option<(u64, Option<i64>)> = None;
        let mut select: Option<(u64, Option<i64>)> = None;
        ui.scope_builder(UiBuilder::new().max_rect(hours), |ui| {
            ScrollArea::vertical().id_salt("cal-hours").auto_shrink(false).show(ui, |ui| {
                let (canvas, resp) = ui.allocate_exact_size(vec2(hours.width().max(0.0), HOUR_H * 24.0), Sense::click());
                let p = ui.painter_at(canvas.intersect(ui.clip_rect()));
                for (i, day) in days.iter().enumerate() {
                    let c = date_of(*day);
                    let x = canvas.left() + gutter + col_w * i as f32;
                    let workday = (1..=5).contains(&c.weekday);
                    let work =
                        Rect::from_min_max(pos2(x, canvas.top() + HOUR_H * WORK.0 as f32), pos2(x + col_w, canvas.top() + HOUR_H * WORK.1 as f32));
                    p.rect_filled(Rect::from_min_size(pos2(x, canvas.top()), vec2(col_w, canvas.height())), 0.0, t.pane);
                    if workday {
                        p.rect_filled(work, 0.0, t.surface);
                    }
                    p.vline(x + 0.5, canvas.y_range(), (1.0, t.line));
                }
                for hour in 0..24u32 {
                    let y = canvas.top() + HOUR_H * hour as f32;
                    p.hline(canvas.left() + gutter..=canvas.right(), y + 0.5, (1.0, t.line));
                    p.hline(canvas.left() + gutter..=canvas.right(), y + HOUR_H / 2.0 + 0.5, (1.0, t.line.gamma_multiply(0.45)));
                    if hour > 0 {
                        p.text(pos2(canvas.left() + gutter - 8.0, y), Align2::RIGHT_CENTER, self.hour_name(hour), font(10.5), t.text_dim);
                        if let Some(z) = &second {
                            // The same moment there, on the first day shown; half hours too.
                            let local = calendar::day_start(first, off).saturating_add(i64::from(hour) * 60 * MINUTE);
                            let there = z.civil(local);
                            p.text(
                                pos2(canvas.left() + GUTTER - 8.0, y),
                                Align2::RIGHT_CENTER,
                                self.clock_name(there.hour, there.minute),
                                font(10.5),
                                t.text_dim,
                            );
                        }
                    }
                }
                if scroll_to {
                    let y = canvas.top() + HOUR_H * (WORK.0 as f32 - 0.5);
                    ui.scroll_to_rect(Rect::from_min_size(pos2(canvas.left(), y), vec2(1.0, HOUR_H)), Some(egui::Align::TOP));
                }
                // Each day's appointments, those at the same time side by side.
                for (i, day) in days.iter().enumerate() {
                    let from = calendar::day_start(*day, off);
                    let to = from.saturating_add(DAY);
                    let timed: Vec<&(u64, Appointment)> =
                        items.iter().filter(|(_, a)| !(a.all_day || a.end.saturating_sub(a.start) >= DAY) && a.overlaps(from, to)).collect();
                    let spans: Vec<(i64, i64)> = timed.iter().map(|(_, a)| (a.start.max(from), a.end.min(to))).collect();
                    let cols = calendar::columns(&spans, 15 * MINUTE);
                    let x = canvas.left() + gutter + col_w * i as f32;
                    for ((item, (s, e)), (col, of)) in timed.iter().zip(&spans).zip(&cols) {
                        let (id, a) = (item.0, &item.1);
                        let y0 = canvas.top() + (s.saturating_sub(from) / MINUTE) as f32 / 60.0 * HOUR_H;
                        let y1 = (canvas.top() + (e.saturating_sub(from) / MINUTE) as f32 / 60.0 * HOUR_H).max(y0 + 18.0);
                        let w = (col_w - 8.0) / (*of).max(1) as f32;
                        let r =
                            Rect::from_min_max(pos2(x + 2.0 + w * *col as f32, y0 + 1.0), pos2(x + 2.0 + w * (*col as f32 + 1.0) - 2.0, y1 - 1.0));
                        let resp = self.appointment_box(ui, &p, r, id, a, chosen(id, a), "hours");
                        if resp.double_clicked() {
                            open = Some((id, a.occurrence));
                        } else if resp.clicked() {
                            select = Some((id, a.occurrence));
                        }
                    }
                    // Now, on today.
                    if *day == today {
                        let now = self.session.time_now();
                        let y = canvas.top() + (now.saturating_sub(from) / MINUTE) as f32 / 60.0 * HOUR_H;
                        p.hline(x..=x + col_w, y, Stroke::new(1.5, t.danger));
                        p.circle_filled(pos2(x, y), 4.0, t.danger);
                    }
                }
                // A double click where nothing is makes an appointment at that half hour.
                if let Some(at) = resp.interact_pointer_pos().filter(|_| resp.double_clicked() || resp.clicked()) {
                    let i = ((at.x - canvas.left() - gutter) / col_w).floor();
                    let half = ((at.y - canvas.top()) / (HOUR_H / 2.0)).floor().clamp(0.0, 47.0) as i64;
                    if i >= 0.0
                        && let Some(day) = days.get(i as usize)
                    {
                        if resp.double_clicked() {
                            new_at = Some(calendar::day_start(*day, off).saturating_add(half * 30 * MINUTE));
                        } else {
                            clicked_empty = true;
                        }
                    }
                }
            });
        });
        if let Some((id, occurrence)) = open.or(band_open) {
            self.open_appointment(id, occurrence);
        } else if let Some((id, occurrence)) = select.or(band_select) {
            self.act("calendar.select", select_params(id, occurrence));
        } else if let Some(start) = new_at {
            self.appointment = Some(AppointmentForm::new_at(start, false, off));
        } else if clicked_empty {
            self.act("calendar.select", json!({ "id": null }));
        }
    }

    /// One appointment: its time, coloured as it is shown as, with its subject and place.
    #[allow(clippy::too_many_arguments)]
    fn appointment_box(&self, ui: &mut Ui, p: &Painter, r: Rect, id: u64, a: &Appointment, chosen: bool, place: &str) -> egui::Response {
        let t = &self.tokens;
        let (fill, bar) = match a.busy {
            Busy::OutOfOffice => (t.away.gamma_multiply(0.22), t.away),
            Busy::Free => (t.surface, t.accent),
            Busy::Tentative => (t.surface, t.accent),
            _ => (t.accent.gamma_multiply(0.2), t.accent),
        };
        // Opaque first, so the hour lines do not show through.
        p.rect_filled(r, 3.0, t.surface);
        p.rect_filled(r, 3.0, fill);
        let bar_r = Rect::from_min_size(r.min, vec2(4.0, r.height()));
        match a.busy {
            // Tentative time is striped, free time an outline, time elsewhere dotted.
            Busy::Tentative => {
                let mut y = bar_r.top();
                while y < bar_r.bottom() {
                    p.rect_filled(Rect::from_min_max(pos2(bar_r.left(), y), pos2(bar_r.right(), (y + 3.0).min(bar_r.bottom()))), 0.0, bar);
                    y += 6.0;
                }
            }
            Busy::Free => {
                p.rect_stroke(bar_r, 0.0, (1.0, bar), egui::StrokeKind::Inside);
            }
            Busy::WorkingElsewhere => {
                let mut y = bar_r.top() + 2.0;
                while y < bar_r.bottom() {
                    p.circle_filled(pos2(bar_r.center().x, y), 1.4, bar);
                    y += 4.0;
                }
            }
            _ => {
                p.rect_filled(bar_r, 0.0, bar);
            }
        }
        p.rect_stroke(
            r,
            3.0,
            if chosen { Stroke::new(2.0, t.accent_dark) } else { Stroke::new(1.0, bar.gamma_multiply(0.5)) },
            egui::StrokeKind::Inside,
        );
        let inner = p.with_clip_rect(r.shrink(2.0).intersect(p.clip_rect()));
        let subject = if a.subject.trim().is_empty() { "(No subject)".to_string() } else { one_line(&a.subject) };
        let w = r.width() - 12.0;
        let x = r.left() + 8.0;
        let one_row = r.height() < 34.0;
        if one_row {
            let mut line = subject;
            if !a.location.is_empty() {
                line = format!("{line}; {}", one_line(&a.location));
            }
            clip_text(&inner, pos2(x, r.center().y - 8.0), w, &line, bold(11.5), t.text);
        } else {
            clip_text(&inner, pos2(x, r.top() + 3.0), w, &subject, bold(11.5), t.text);
            if !a.location.is_empty() {
                clip_text(&inner, pos2(x, r.top() + 19.0), w, &one_line(&a.location), font(11.0), t.text_dim);
            }
        }
        // At its bottom right: a loop when it repeats (struck through when this one was changed
        // on its own), and a bell when it has a reminder.
        let mut x_icon = r.right() - 16.0;
        let y_icon = if one_row { r.center().y - 5.5 } else { r.bottom() - 16.0 };
        if a.recurring && r.width() > 48.0 {
            icons::paint(&inner, Rect::from_min_size(pos2(x_icon, y_icon), vec2(11.0, 11.0)), recur_icon(a), t.text_dim);
            x_icon -= 15.0;
        }
        if a.reminder.is_some() && r.width() > 60.0 && !one_row {
            icons::paint(&inner, Rect::from_min_size(pos2(x_icon, y_icon), vec2(11.0, 11.0)), Icon::Bell, t.text_faint);
        }
        let resp = ui.interact(r, egui::Id::new(("cal-appt", place, id, a.occurrence)), Sense::click());
        let off = &self.session.zone.clone();
        let (s, e) = (off.civil(a.start), off.civil(a.end));
        let r = &self.session.region;
        let when = if a.all_day { "All day".to_string() } else { format!("{} - {}", r.time(&s), r.time(&e)) };
        let place = if a.location.is_empty() { String::new() } else { format!("\n{}", one_line(&a.location)) };
        let repeats = if a.recurrence.is_some() { format!("\n{}", a.pattern_words()) } else { String::new() };
        resp.on_hover_text(format!("{when}\n{}{place}\nShow as: {}{repeats}", one_line(&a.subject), show_as(a.busy)))
    }

    /// The month view: six weeks, each day with what is on it.
    fn month_grid(&mut self, ui: &mut Ui, rect: Rect, days: &[i64]) {
        let t = self.tokens.clone();
        let off = &self.session.zone.clone();
        let today = self.session.today();
        let chosen = date_of(if self.session.calendar.day == 0 { today } else { self.session.calendar.day });
        let p = ui.painter_at(rect);
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), 26.0));
        let w = (rect.width() - 8.0) / 7.0;
        let h = ((rect.height() - head.height() - 4.0) / 6.0).max(40.0);
        let ws = usize::try_from(self.session.region.week_start % 7).unwrap_or(0);
        for i in 0..7 {
            let name = WEEKDAYS.get((ws + i) % 7).copied().unwrap_or("");
            p.text(pos2(rect.left() + w * i as f32 + 8.0, head.center().y), Align2::LEFT_CENTER, name, font(11.5), t.text_dim);
        }
        let items = self.session.calendar.items.clone();
        let (selected, occurrence) = (self.session.calendar.selected, self.session.calendar.occurrence);
        let mut open: Option<(u64, Option<i64>)> = None;
        let mut select: Option<Option<(u64, Option<i64>)>> = None;
        let mut new_on: Option<i64> = None;
        let mut go: Option<i64> = None;
        for (i, day) in days.iter().enumerate() {
            let (row, col) = (i / 7, i % 7);
            let cell = Rect::from_min_size(pos2(rect.left() + w * col as f32, head.bottom() + h * row as f32), vec2(w, h));
            let c = date_of(*day);
            let in_month = c.month == chosen.month;
            p.rect_filled(cell, 0.0, if in_month { t.surface } else { t.pane });
            p.rect_stroke(cell, 0.0, (1.0, t.line), egui::StrokeKind::Inside);
            let cell_resp = ui.interact(cell, egui::Id::new(("cal-month-day", *day)), Sense::click());
            let label = if c.day == 1 { format!("{} {}", c.month_name().chars().take(3).collect::<String>(), c.day) } else { c.day.to_string() };
            let is_today = *day == today;
            if is_today {
                let lw = text_width(&p, &label, bold(12.0)) + 10.0;
                p.rect_filled(Rect::from_min_size(pos2(cell.left() + 3.0, cell.top() + 3.0), vec2(lw, 17.0)), 8.0, t.accent);
            }
            p.text(
                pos2(cell.left() + 8.0, cell.top() + 11.5),
                Align2::LEFT_CENTER,
                &label,
                if is_today || c.day == 1 { bold(12.0) } else { font(12.0) },
                if is_today {
                    t.on_accent
                } else if in_month {
                    t.text
                } else {
                    t.text_faint
                },
            );
            let from = calendar::day_start(*day, off);
            let on: Vec<&(u64, Appointment)> = items.iter().filter(|(_, a)| a.overlaps(from, from.saturating_add(DAY))).collect();
            let room = (((cell.height() - 24.0) / MONTH_LINE).floor().max(0.0)) as usize;
            let fits = if on.len() > room { room.saturating_sub(1) } else { on.len() };
            for (k, (id, a)) in on.iter().take(fits).enumerate() {
                let r = Rect::from_min_size(
                    pos2(cell.left() + 3.0, cell.top() + 22.0 + MONTH_LINE * k as f32),
                    vec2(cell.width() - 6.0, MONTH_LINE - 2.0),
                );
                let resp = ui.interact(r, egui::Id::new(("cal-month-appt", *day, *id, a.occurrence)), Sense::click());
                let whole = a.all_day || a.end.saturating_sub(a.start) >= DAY;
                let chosen = selected == Some(*id) && (occurrence.is_none() || a.occurrence == occurrence);
                if whole {
                    p.rect_filled(r, 2.0, if a.busy == Busy::OutOfOffice { t.away.gamma_multiply(0.3) } else { t.accent.gamma_multiply(0.2) });
                } else if resp.hovered() {
                    p.rect_filled(r, 2.0, t.hover);
                }
                if chosen {
                    p.rect_stroke(r, 2.0, (1.5, t.accent_dark), egui::StrokeKind::Inside);
                }
                let mut x = r.left() + 4.0;
                if !whole {
                    let s = off.civil(a.start.max(from));
                    let when = self.session.region.time(&s);
                    p.text(pos2(x, r.center().y), Align2::LEFT_CENTER, &when, font(11.0), t.text_dim);
                    x += text_width(&p, &when, font(11.0)) + 5.0;
                }
                let subject = if a.subject.trim().is_empty() { "(No subject)".to_string() } else { one_line(&a.subject) };
                let mut right = r.right() - 2.0;
                if a.recurring && r.width() > 80.0 {
                    right -= 14.0;
                    icons::paint(&p, Rect::from_min_size(pos2(right + 2.0, r.center().y - 5.0), vec2(10.0, 10.0)), recur_icon(a), t.text_dim);
                }
                clip_text(&p, pos2(x, r.center().y - 7.5), right - x, &subject, font(11.5), t.text);
                if resp.double_clicked() {
                    open = Some((*id, a.occurrence));
                } else if resp.clicked() {
                    select = Some(Some((*id, a.occurrence)));
                }
            }
            if on.len() > fits {
                let more = format!("{} more", on.len() - fits);
                let r = Rect::from_min_size(
                    pos2(cell.left() + 3.0, cell.top() + 22.0 + MONTH_LINE * fits as f32),
                    vec2(cell.width() - 6.0, MONTH_LINE - 2.0),
                );
                let resp = ui.interact(r, egui::Id::new(("cal-month-more", *day)), Sense::click());
                p.text(
                    pos2(r.left() + 4.0, r.center().y),
                    Align2::LEFT_CENTER,
                    &more,
                    bold(11.0),
                    if resp.hovered() { t.accent } else { t.text_dim },
                );
                if resp.clicked() {
                    go = Some(*day);
                }
            }
            if cell_resp.double_clicked() {
                new_on = Some(from);
            } else if cell_resp.clicked() {
                select = Some(None);
            }
        }
        if let Some((id, occurrence)) = open {
            self.open_appointment(id, occurrence);
        } else if let Some(day) = go {
            self.act("calendar.view", json!({ "view": "day" }));
            self.go_to_day(day);
        } else if let Some(sel) = select {
            match sel {
                Some((id, occurrence)) => self.act("calendar.select", select_params(id, occurrence)),
                None => self.act("calendar.select", json!({ "id": null })),
            }
        } else if let Some(start) = new_on {
            // A day of the month makes an all-day event, as Outlook's does.
            self.appointment = Some(AppointmentForm::new_at(start, true, off));
        }
    }

    /// Open the appointment `id` in its window: one occurrence of a series, which the window
    /// can switch to the whole series, or the whole appointment.
    pub(crate) fn open_appointment(&mut self, id: u64, occurrence: Option<i64>) {
        let off = &self.session.zone.clone();
        let is = |(i, a): &&(u64, Appointment)| *i == id && (occurrence.is_none() || a.occurrence == occurrence);
        // Shown in the Calendar, or on the To-Do Bar beside any module.
        let listed = self.session.calendar.items.iter().any(|item| is(&item));
        let shown = self.session.calendar.items.iter().chain(&self.session.todo.appointments).find(is).map(|(_, a)| a.clone());
        let Some(a) = shown else { return };
        let m = if a.meeting { self.session.meeting(id).ok().flatten() } else { None };
        let mut form = AppointmentForm::of(Some(id), &a, off).with_meeting(m.clone());
        if a.occurrence.is_some() {
            match self.session.appointment(id) {
                Ok(series) => form.other = Some(Box::new(AppointmentForm::of(Some(id), &series, off).with_meeting(m))),
                Err(e) => self.session.status = e.to_string(),
            }
        }
        self.appointment = Some(form);
        if listed {
            self.act("calendar.select", select_params(id, a.occurrence));
        }
    }

    /// A new appointment at the next half hour of the day chosen.
    pub(crate) fn new_appointment(&mut self) {
        let s = &self.session;
        let off = &s.zone.clone();
        let now = s.time_now();
        let day = if s.calendar.day == 0 { s.today() } else { s.calendar.day };
        let into_day = now.saturating_sub(calendar::day_start(s.today(), off));
        let half = (into_day / (30 * MINUTE)).saturating_add(1).clamp(0, 47);
        let start = calendar::day_start(day, off).saturating_add(half * 30 * MINUTE);
        self.appointment = Some(AppointmentForm::new_at(start, false, off));
    }

    /// A new meeting: an appointment with its attendees to type in, sent rather than saved.
    pub(crate) fn new_meeting(&mut self) {
        self.new_appointment();
        if let Some(f) = self.appointment.as_mut() {
            f.inviting = true;
        }
    }

    /// A meeting in reply to the open message: its sender and those it went to asked, its
    /// subject, at the next whole hour. Nothing is kept until it is sent or saved.
    pub(crate) fn reply_with_meeting(&mut self) -> Result<(), String> {
        let Some(item) = self.session.open.as_ref() else { return Err("No message is open".to_string()) };
        let subject = one_line(&item.subject());
        let asked = self.session.reply_attendees();
        let off = &self.session.zone.clone();
        let now = self.session.time_now();
        let hour = 60 * MINUTE;
        let start = now.saturating_sub(now.rem_euclid(hour)).saturating_add(hour);
        let mut form = AppointmentForm::new_at(start, false, off);
        form.end_time = hm(&off.civil(start.saturating_add(hour)));
        form.end_date = ymd(&off.civil(start.saturating_add(hour)));
        form.subject = subject;
        let m = Meeting { attendees: asked, ..Meeting::default() };
        form.asked = crate::meeting::lines(Some(&m));
        form.inviting = true;
        self.appointment = Some(form);
        Ok(())
    }

    /// The window an appointment is made or changed in.
    pub(crate) fn appointment_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.appointment.take() else { return };
        let t = self.tokens.clone();
        let off = &self.session.zone.clone();
        let mut ask: Option<Ask> = None;
        // What else is at that time, named, where Outlook only says there is a conflict.
        let clashes: Vec<String> = form
            .span(off)
            .map(|(s, e)| {
                self.session
                    .calendar
                    .items
                    .iter()
                    .filter(|(id, a)| Some(*id) != form.id && a.busy != Busy::Free && !a.all_day && e > s && a.overlaps(s, e))
                    .take(3)
                    .map(|(_, a)| one_line(&a.subject))
                    .collect()
            })
            .unwrap_or_default();
        // The pattern as typed, in words, with the days it first falls on; or why it cannot be.
        let preview = self.repeat_preview(&form);
        let zones = known_zones();
        let here_label = off.label();
        let is_meeting = form.meeting.is_some() || form.inviting;
        let title = match (form.id, form.occurrence, is_meeting) {
            (None, _, false) => "New Appointment",
            (None, _, true) => "New Meeting",
            (Some(_), Some(_), false) => "Appointment Occurrence",
            (Some(_), Some(_), true) => "Meeting Occurrence",
            (Some(_), None, false) if form.recurring => "Appointment Series",
            (Some(_), None, true) if form.recurring => "Meeting Series",
            (Some(_), None, false) => "Appointment",
            (Some(_), None, true) => "Meeting",
        };
        let icon = if is_meeting { Icon::NewMeeting } else { Icon::NewAppointment };
        let mut switch = false;
        let shown = crate::dialog::Dialog::new("appointment", title, icon).show(ctx, &t, |ui| {
            // Opened on one occurrence: this one alone, or the whole series, with no question first.
            if form.other.is_some() || form.occurrence.is_some() {
                ui.horizontal(|ui| {
                    let alone = form.occurrence.is_some();
                    if ui.selectable_label(alone, "This occurrence").clicked() && !alone {
                        switch = true;
                    }
                    if ui.selectable_label(!alone, "The whole series").clicked() && alone {
                        switch = true;
                    }
                });
                ui.add_space(4.0);
            }
            if form.occurrence.is_some() && !form.pattern.is_empty() {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Recur, t.text_dim);
                    ui.label(
                        egui::RichText::new(format!("One occurrence of a series that {}", lower_first(&form.pattern)))
                            .font(font(12.0))
                            .color(t.text_dim),
                    );
                });
                ui.add_space(4.0);
            }
            if let Some(m) = form.meeting.as_ref().filter(|m| m.received) {
                let organizer = one_line(m.organizer.label());
                let tone = if m.cancelled { t.danger } else { t.accent };
                crate::dialog::callout(ui, &t, tone, |ui| {
                    let words = if m.cancelled {
                        format!("{organizer} has called this meeting off.")
                    } else {
                        format!("{organizer} asked you to this meeting. {}.", crate::meeting::answer_words(m.response))
                    };
                    ui.label(egui::RichText::new(words).font(font(12.0)).color(t.text));
                    if !m.cancelled {
                        ui.horizontal(|ui| {
                            for (label, response) in [("Accept", "accept"), ("Tentative", "tentative"), ("Decline", "decline")] {
                                if ui.add(crate::dialog::plain(label)).on_hover_text("Answer, and tell the organizer").clicked() {
                                    ask = Some(Ask::Respond(response));
                                }
                            }
                        });
                    }
                });
                ui.add_space(6.0);
            }
            if !clashes.is_empty() {
                let words = format!("At the same time: {}", clashes.join(", "));
                crate::dialog::callout(ui, &t, t.flag, |ui| {
                    ui.label(egui::RichText::new(words).font(font(12.0)).color(t.text));
                });
                ui.add_space(6.0);
            }
            egui::Grid::new("appointment-form").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                ui.label(egui::RichText::new("Subject").color(t.text_dim));
                let subject = ui.add(LineEdit::new(&mut form.subject, egui::Id::new("appt-subject")).desired_width(380.0).char_limit(255));
                if std::mem::take(&mut form.focus) {
                    subject.request_focus();
                }
                ui.end_row();
                ui.label(egui::RichText::new("Location").color(t.text_dim));
                ui.add(LineEdit::new(&mut form.location, egui::Id::new("appt-location")).desired_width(380.0).char_limit(255));
                ui.end_row();
                if let Some(m) = form.meeting.as_ref().filter(|m| m.received) {
                    let people: Vec<String> = m.attendees.iter().take(12).map(|p| one_line(p.label())).collect();
                    let more = m.attendees.len().saturating_sub(people.len());
                    ui.label(egui::RichText::new("Organizer").color(t.text_dim));
                    ui.label(egui::RichText::new(crate::meeting::person_text(&m.organizer)).color(t.text));
                    ui.end_row();
                    ui.label(egui::RichText::new("Attendees").color(t.text_dim));
                    let mut line = people.join("; ");
                    if more > 0 {
                        line.push_str(&format!(" and {more} more"));
                    }
                    ui.add(egui::Label::new(egui::RichText::new(line).color(t.text)).wrap());
                    ui.end_row();
                } else if form.inviting {
                    // The whole series' attendees: one occurrence alone has those of its series.
                    let alone = form.occurrence.is_some();
                    for (n, (label, hint)) in
                        [("Required", "Those who need to come"), ("Optional", "Those welcome to come"), ("Resources", "Rooms and things booked")]
                            .into_iter()
                            .enumerate()
                    {
                        let Some(line) = form.asked.get_mut(n) else { continue };
                        ui.label(egui::RichText::new(label).color(t.text_dim));
                        ui.add_enabled(
                            !alone,
                            LineEdit::new(line, egui::Id::new(("appt-asked", n)))
                                .desired_width(380.0)
                                .char_limit(16_000)
                                .hint("Name <address>; …", None),
                        )
                        .on_hover_text(hint)
                        .on_disabled_hover_text("Those asked are changed for the whole series");
                        ui.end_row();
                    }
                }
                for (label, date, clock, key) in
                    [("Start time", &mut form.start_date, &mut form.start_time, "start"), ("End time", &mut form.end_date, &mut form.end_time, "end")]
                {
                    ui.label(egui::RichText::new(label).color(t.text_dim));
                    ui.horizontal(|ui| {
                        ui.add(LineEdit::new(date, egui::Id::new(("appt-date", key))).desired_width(110.0).char_limit(10).hint("YYYY-MM-DD", None));
                        if !form.all_day {
                            ui.add(LineEdit::new(clock, egui::Id::new(("appt-time", key))).desired_width(64.0).char_limit(5).hint("HH:MM", None));
                        }
                    });
                    ui.end_row();
                }
                ui.label("");
                ui.checkbox(&mut form.all_day, "All day event");
                ui.end_row();
                if form.occurrence.is_none() && !form.all_day {
                    ui.label(egui::RichText::new("Time zone").color(t.text_dim));
                    zone_choice(ui, &mut form.zone, zones, &here_label);
                    ui.end_row();
                }
                ui.label(egui::RichText::new("Show As").color(t.text_dim));
                egui::ComboBox::from_id_salt("appt-busy").selected_text(show_as(form.busy)).width(180.0).show_ui(ui, |ui| {
                    for (b, name) in SHOW_AS {
                        ui.selectable_value(&mut form.busy, *b, *name);
                    }
                });
                ui.end_row();
                ui.label(egui::RichText::new("Reminder").color(t.text_dim));
                egui::ComboBox::from_id_salt("appt-reminder").selected_text(reminder_words(form.reminder)).width(180.0).show_ui(ui, |ui| {
                    for (r, name) in REMINDERS {
                        ui.selectable_value(&mut form.reminder, *r, *name);
                    }
                });
                ui.end_row();
                if form.occurrence.is_none() {
                    let week_start = self.session.region.week_start;
                    repeat_rows(ui, &t, &mut form.repeat, week_start, preview.as_ref());
                }
            });
            if let Some(m) = form.meeting.as_ref().filter(|m| !m.received && m.invited && !m.attendees.is_empty()) {
                ui.add_space(6.0);
                egui::CollapsingHeader::new(
                    egui::RichText::new(format!("Tracking: {}", crate::meeting::tally_words(m))).font(font(12.0)).color(t.text),
                )
                .id_salt("appt-tracking")
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                        egui::Grid::new("appt-tracking-grid").num_columns(3).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                            for p in m.attendees.iter().take(openmailclient_model::meeting::MAX_ATTENDEES) {
                                ui.label(egui::RichText::new(one_line(p.label())).color(t.text));
                                ui.label(
                                    egui::RichText::new(match p.role {
                                        Role::Required => "Required",
                                        Role::Optional => "Optional",
                                        Role::Resource => "Resource",
                                    })
                                    .color(t.text_dim),
                                );
                                ui.label(egui::RichText::new(p.response.words()).color(t.text));
                                ui.end_row();
                            }
                        });
                    });
                });
            }
            ui.add_space(8.0);
            match form.span(off) {
                Some((s, e)) if e > s => {
                    let mut words = format!("Lasts {}", length_words(e.saturating_sub(s) / MINUTE));
                    // Typed in another zone than ours: when that is here, too.
                    let typed_in = form.typed_in(off);
                    if typed_in.offset_at(s) != off.offset_at(s) || typed_in.offset_at(e) != off.offset_at(e) {
                        let (a, b) = (off.civil(s), off.civil(e));
                        words.push_str(&format!(
                            " · {} {} to {} {} where you are",
                            ymd(&a),
                            hm(&a),
                            if ymd(&a) == ymd(&b) { String::new() } else { ymd(&b) },
                            hm(&b)
                        ));
                        words = words.replace("to  ", "to ");
                    }
                    ui.label(egui::RichText::new(words).font(font(12.0)).color(t.text_dim));
                }
                Some(_) => {
                    ui.label(egui::RichText::new("It ends before it starts").font(font(12.0)).color(t.danger));
                }
                None => {
                    ui.label(egui::RichText::new("Dates are written YYYY-MM-DD and times HH:MM").font(font(12.0)).color(t.text_dim));
                }
            }
            if let Some(said) = &form.said {
                crate::dialog::callout(ui, &t, t.danger, |ui| {
                    ui.label(egui::RichText::new(one_line(said)).font(font(12.0)).color(t.text));
                });
            }
            let invited = form.meeting.as_ref().is_some_and(|m| m.invited && !m.received && !m.cancelled);
            let sends = form.asks_anyone() && !form.received();
            crate::dialog::foot(ui, &t, |ui| {
                let deletes = if form.occurrence.is_some() { "Deletes this occurrence alone" } else { "Moves it to Deleted Items" };
                if invited && form.occurrence.is_none() {
                    if ui
                        .add(crate::dialog::danger(&t, "Cancel Meeting"))
                        .on_hover_text("Tells everyone asked it is off, and moves it to Deleted Items")
                        .clicked()
                    {
                        ask = Some(Ask::CancelMeeting);
                    }
                } else if form.id.is_some() && ui.add(crate::dialog::danger(&t, "Delete")).on_hover_text(deletes).clicked() {
                    ask = Some(Ask::Delete);
                }
                if !form.inviting && form.meeting.is_none() && form.occurrence.is_none() && ui.add(crate::dialog::plain("Invite Attendees")).clicked()
                {
                    form.inviting = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(crate::dialog::plain("Cancel")).clicked() {
                        ask = Some(Ask::Close);
                    }
                    if sends {
                        let label = if invited { "Send Update" } else { "Send" };
                        if ui.add(crate::dialog::primary(&t, label)).on_hover_text("Saves it and sends it to everyone asked").clicked() {
                            ask = Some(Ask::Send);
                        }
                        if ui.add(crate::dialog::plain("Save & Close")).on_hover_text("Keeps it in the calendar without sending it yet").clicked() {
                            ask = Some(Ask::Save);
                        }
                    } else if ui.add(crate::dialog::primary(&t, "Save & Close")).clicked() {
                        ask = Some(Ask::Save);
                    }
                });
            });
        });
        if shown.closed || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ask = Some(Ask::Close);
        }
        if switch && ask.is_none() {
            self.appointment = Some(form.switched());
            return;
        }
        let result = match ask {
            None => {
                self.appointment = Some(form);
                return;
            }
            Some(Ask::Close) => return,
            Some(Ask::Save) => self.execute(if form.id.is_some() { "calendar.change" } else { "calendar.new" }, &form.params()),
            Some(Ask::Delete) => self.execute("calendar.delete", &form.delete_params()),
            Some(Ask::Send) => self.execute(if form.id.is_some() { "calendar.change" } else { "calendar.new" }, &form.params()).and_then(|saved| {
                // Saved: what is typed is kept, so a send that fails is tried again from here.
                let id = form.id.or_else(|| saved["id"].as_u64()).ok_or_else(|| "The meeting was not kept".to_string())?;
                form.id = Some(id);
                form.asked_was = form.asked.clone();
                self.execute("calendar.sendInvitations", &json!({ "id": id }))
            }),
            Some(Ask::CancelMeeting) => self.execute("calendar.cancelMeeting", &json!({ "id": form.id })),
            Some(Ask::Respond(response)) => {
                let id = form.id;
                self.execute("calendar.respond", &json!({ "id": id, "response": response }))
            }
        };
        if let Err(e) = result {
            form.said = Some(e);
            self.appointment = Some(form);
        }
    }
}

/// The parameters `calendar.select` takes for an appointment or one occurrence of it.
fn select_params(id: u64, occurrence: Option<i64>) -> Value {
    match occurrence {
        Some(day) => json!({ "id": id, "occurrence": ymd(&date_of(day)) }),
        None => json!({ "id": id }),
    }
}

/// The mark of a series, struck through on an occurrence changed on its own.
fn recur_icon(a: &Appointment) -> Icon {
    let changed = a.occurrence.is_some_and(|d| a.recurrence.as_ref().is_some_and(|r| r.exception_on(d).is_some()));
    if changed { Icon::RecurChanged } else { Icon::Recur }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_lowercase().chain(c).collect(),
        None => String::new(),
    }
}

/// A day as "Mon 12 Oct".
fn short_day(day: i64) -> String {
    let c = date_of(day);
    let name = WEEKDAYS.get(usize::try_from(c.weekday).unwrap_or(0)).copied().unwrap_or("");
    format!("{name} {} {}", c.day, c.month_name().chars().take(3).collect::<String>())
}

/// What the Repeats rows say under the pattern: in words with its first days, or the reason
/// it cannot be saved.
type Preview = Result<Option<(String, Vec<i64>)>, String>;

impl MailUi {
    /// The pattern of `form` as the engine would make it, not saved.
    pub(crate) fn repeat_preview(&self, form: &AppointmentForm) -> Option<Preview> {
        if form.occurrence.is_some() || form.repeat.repeats == Repeats::Never {
            return None;
        }
        let off = &self.session.zone.clone();
        let (start, end) = form.span(off)?;
        let typed_in = form.typed_in(off);
        let a = Appointment {
            subject: String::new(),
            location: String::new(),
            start,
            end: end.max(start),
            all_day: form.all_day,
            busy: form.busy,
            reminder: None,
            recurring: false,
            meeting: false,
            recurrence: None,
            occurrence: None,
            zone: Some(typed_in).filter(|z| !z.name().is_empty()),
        };
        Some(match self.session.repeat_preview(&form.repeat.param(), &a) {
            Ok(Some(r)) => {
                let first = r.start_day;
                let next: Vec<i64> = r.days(first, first.saturating_add(800)).into_iter().take(4).collect();
                let mut words = format!("Occurs {}", r.words());
                match r.end {
                    End::Never => {}
                    End::After(n) => words.push_str(&format!(", {n} times")),
                    End::On(day) => words.push_str(&format!(", until {}", short_day(day))),
                }
                Ok(Some((words, next)))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(e.to_string()),
        })
    }
}

impl MailUi {
    /// The Time Zones window: the zone dates are shown in, the system's or one chosen, and a
    /// second one whose hours stand beside ours in the day and week views; the time now in each.
    pub(crate) fn time_zones_window(&mut self, ctx: &egui::Context) {
        if !self.time_zones_open {
            return;
        }
        let t = self.tokens.clone();
        let zones = known_zones();
        let now = self.session.now;
        let system = self.session.system_zone.clone();
        let mut chosen = self.session.zone_chosen.clone().unwrap_or_default();
        let mut second = self.session.second_zone.as_ref().map(|z| z.name().to_string()).unwrap_or_default();
        let (chosen_was, second_was) = (chosen.clone(), second.clone());
        let at = |zone: &Zone| {
            let c = zone.civil(now);
            format!("{} {}", self.clock_name(c.hour, c.minute), zone.offset_text_at(now))
        };
        let (here_now, there_now) = (at(&self.session.zone), self.session.second_zone.as_ref().map(at));
        let mut close = false;
        let shown = crate::dialog::Dialog::new("time-zones", "Time Zones", Icon::Clock).width(460.0).show(ctx, &t, |ui| {
            egui::Grid::new("time-zones-form").num_columns(2).spacing(vec2(12.0, 10.0)).show(ui, |ui| {
                ui.label(egui::RichText::new("Show dates in").color(t.text_dim));
                let label = |name: &str, none: &str| zones.iter().find(|(n, _)| n == name).map_or_else(|| none.to_string(), |(_, l)| l.clone());
                let system_words = format!("The system's ({})", system.label());
                egui::ComboBox::from_id_salt("tz-shown").selected_text(label(&chosen, &system_words)).width(300.0).height(320.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut chosen, String::new(), system_words.as_str());
                    for (name, l) in zones {
                        ui.selectable_value(&mut chosen, name.clone(), l.as_str());
                    }
                });
                ui.end_row();
                ui.label("");
                ui.label(egui::RichText::new(format!("It is {here_now} there now.")).font(font(12.0)).color(t.text_dim));
                ui.end_row();
                ui.label(egui::RichText::new("Second time zone").color(t.text_dim));
                egui::ComboBox::from_id_salt("tz-second").selected_text(label(&second, "None")).width(300.0).height(320.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut second, String::new(), "None");
                    for (name, l) in zones {
                        ui.selectable_value(&mut second, name.clone(), l.as_str());
                    }
                });
                ui.end_row();
                if let Some(there) = &there_now {
                    ui.label("");
                    ui.label(
                        egui::RichText::new(format!("It is {there} there now; its hours stand beside ours.")).font(font(12.0)).color(t.text_dim),
                    );
                    ui.end_row();
                }
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("An appointment made in a zone keeps its hour there when summer time begins or ends.")
                    .font(font(12.0))
                    .color(t.text_dim),
            );
            crate::dialog::foot(ui, &t, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(crate::dialog::primary(&t, "Close")).clicked() {
                        close = true;
                    }
                });
            });
        });
        let name = |n: String| if n.is_empty() { Value::Null } else { Value::String(n) };
        if chosen != chosen_was {
            self.act("calendar.timeZone", json!({ "zone": name(chosen) }));
        }
        if second != second_was {
            self.act("calendar.secondTimeZone", json!({ "zone": name(second) }));
        }
        if shown.closed || close {
            self.time_zones_open = false;
        }
    }
}

/// Every zone known, by name and as it is labelled, west to east; made once.
fn known_zones() -> &'static [(String, String)] {
    static ZONES: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
    ZONES.get_or_init(|| Zone::known().iter().map(|z| (z.name().to_string(), z.label())).collect())
}

/// The appointment window's choice of time zone: none, so that it is where we are wherever
/// that is, or one known by name.
fn zone_choice(ui: &mut Ui, zone: &mut String, zones: &[(String, String)], here: &str) {
    let shown = zones.iter().find(|(name, _)| name == zone).map_or_else(|| format!("Where you are ({here})"), |(_, label)| label.clone());
    egui::ComboBox::from_id_salt("appt-zone").selected_text(shown).width(300.0).height(320.0).show_ui(ui, |ui| {
        ui.selectable_value(zone, String::new(), format!("Where you are ({here})"));
        for (name, label) in zones {
            ui.selectable_value(zone, name.clone(), label.as_str());
        }
    });
}

/// The rows of the appointment window's grid that say how it repeats.
fn repeat_rows(ui: &mut Ui, t: &crate::theme::Tokens, f: &mut RepeatForm, week_start: u32, preview: Option<&Preview>) {
    let label = |ui: &mut Ui, text: &str| {
        ui.label(egui::RichText::new(text).color(t.text_dim));
    };
    label(ui, "Repeats");
    let name = REPEATS.iter().find(|(r, _)| *r == f.repeats).map_or("", |(_, n)| *n);
    egui::ComboBox::from_id_salt("appt-repeats").selected_text(name).width(180.0).show_ui(ui, |ui| {
        for (r, name) in REPEATS {
            ui.selectable_value(&mut f.repeats, *r, *name);
        }
    });
    ui.end_row();
    if f.repeats == Repeats::Never {
        return;
    }
    let unit = match f.repeats {
        Repeats::Daily => Some(("day", "days", 999)),
        Repeats::Weekly => Some(("week", "weeks", 99)),
        Repeats::Monthly => Some(("month", "months", 99)),
        _ => None,
    };
    if let Some((one, many, most)) = unit {
        label(ui, "Every");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut f.interval).range(1..=most).speed(0.1));
            ui.label(if f.interval == 1 { one } else { many });
        });
        ui.end_row();
    }
    if f.repeats == Repeats::Weekly {
        label(ui, "On");
        ui.horizontal(|ui| {
            for i in 0..7u32 {
                let d = (week_start + i) % 7;
                let bit = 1u8 << d;
                let on = f.days & bit != 0;
                let name = WEEKDAYS.get(d as usize).copied().unwrap_or("");
                // A week has at least one day: the last one cannot be taken off.
                if ui.selectable_label(on, name).clicked() && (!on || f.days != bit) {
                    f.days ^= bit;
                }
            }
        });
        ui.end_row();
    }
    if matches!(f.repeats, Repeats::Monthly | Repeats::Yearly) {
        label(ui, "On");
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut f.month_by, MonthBy::Date, "Day");
                ui.add_enabled(f.month_by == MonthBy::Date, egui::DragValue::new(&mut f.day).range(1..=31).speed(0.1));
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut f.month_by, MonthBy::Weekday, "The");
                ui.add_enabled_ui(f.month_by == MonthBy::Weekday, |ui| {
                    let nth = NTH.iter().find(|(n, _)| *n == f.nth).map_or("first", |(_, w)| *w);
                    egui::ComboBox::from_id_salt("appt-nth").selected_text(nth).width(80.0).show_ui(ui, |ui| {
                        for (n, w) in NTH {
                            ui.selectable_value(&mut f.nth, *n, *w);
                        }
                    });
                    let which = NTH_DAYS.iter().find(|(m, _)| *m == f.nth_days).map_or("day", |(_, w)| *w);
                    egui::ComboBox::from_id_salt("appt-nth-days").selected_text(which).width(110.0).show_ui(ui, |ui| {
                        for (m, w) in NTH_DAYS {
                            ui.selectable_value(&mut f.nth_days, *m, *w);
                        }
                    });
                });
            });
            ui.radio_value(&mut f.month_by, MonthBy::LastDay, "The last day of the month");
        });
        ui.end_row();
    }
    label(ui, "Ends");
    ui.vertical(|ui| {
        ui.radio_value(&mut f.ends, Ends::Never, "Never");
        ui.horizontal(|ui| {
            ui.radio_value(&mut f.ends, Ends::After, "After");
            ui.add_enabled(f.ends == Ends::After, egui::DragValue::new(&mut f.count).range(1..=recur::MAX_COUNT).speed(0.2));
            ui.label("times");
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut f.ends, Ends::On, "On");
            ui.add_enabled(
                f.ends == Ends::On,
                LineEdit::new(&mut f.until, egui::Id::new("appt-until")).desired_width(110.0).char_limit(10).hint("YYYY-MM-DD", None),
            );
        });
    });
    ui.end_row();
    match preview {
        Some(Ok(Some((words, next)))) => {
            ui.label("");
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(words).font(font(12.0)).color(t.text));
                if !next.is_empty() {
                    let days: Vec<String> = next.iter().map(|d| short_day(*d)).collect();
                    ui.label(egui::RichText::new(format!("First on {}", days.join(", "))).font(font(12.0)).color(t.text_dim));
                }
            });
            ui.end_row();
        }
        Some(Err(e)) => {
            ui.label("");
            ui.label(egui::RichText::new(one_line(e)).font(font(12.0)).color(t.danger));
            ui.end_row();
        }
        Some(Ok(None)) | None => {}
    }
}

/// A chevron pointing left (`back`) or right, centred on `c`.
/// Where a month to choose a day from is drawn, and what it shows.
pub(crate) struct Picker<'a> {
    pub rect: Rect,
    pub top: f32,
    pub today: i64,
    /// The day whose month it opens on.
    pub chosen: i64,
    /// The days marked as shown.
    pub shown: &'a [i64],
    pub week_start: u32,
    /// Its widgets' ids are made from it, so two can stand in one window.
    pub salt: &'static str,
}

/// What a month drawn came to: where it ends, and the day clicked.
pub(crate) struct Picked {
    pub bottom: f32,
    pub day: Option<i64>,
}

/// A month to choose a day from, with chevrons to go to the month before and after, which
/// `nav` keeps while it is not the chosen day's (the Calendar's date navigator, and the To-Do
/// Bar's).
pub(crate) fn month_picker(ui: &mut Ui, t: &crate::theme::Tokens, nav: &mut Option<(i64, u32)>, k: Picker<'_>) -> Picked {
    let rect = k.rect;
    let p = ui.painter_at(rect);
    let (year, month) = nav.unwrap_or_else(|| {
        let c = date_of(k.chosen);
        (c.year, c.month)
    });
    let first = calendar::day_of(time::from_civil(year, month, 1, 0, 0, 0, 0), 0);
    let grid = View::Month.days(first, k.week_start);
    let top = k.top;
    let title = format!("{} {}", date_of(first).month_name(), year);
    p.text(pos2(rect.center().x, top + 10.0), Align2::CENTER_CENTER, &title, bold(13.0), t.text);
    for (n, x) in [(-1i32, rect.left() + 22.0), (1, rect.right() - 22.0)] {
        let r = Rect::from_center_size(pos2(x, top + 10.0), vec2(22.0, 22.0));
        let resp = ui.interact(r, egui::Id::new((k.salt, "month", n)), Sense::click());
        if resp.hovered() {
            p.rect_filled(r, 3.0, t.hover);
        }
        chevron(&p, r.center(), n < 0, t.text_dim);
        if resp.clicked() {
            let (mut y, mut m) = (year, i64::from(month) + i64::from(n));
            if m < 1 {
                (y, m) = (y - 1, 12);
            } else if m > 12 {
                (y, m) = (y + 1, 1);
            }
            *nav = Some((y.clamp(1601, 9999), u32::try_from(m).unwrap_or(1)));
        }
    }
    let cell = ((rect.width() - 20.0) / 7.0).min(30.0);
    let x0 = rect.center().x - cell * 3.5;
    let y0 = top + 30.0;
    let week_start = usize::try_from(k.week_start % 7).unwrap_or(0);
    for i in 0..7 {
        let name = WEEKDAYS.get((week_start + i) % 7).copied().unwrap_or("");
        let letter: String = name.chars().take(2).collect();
        p.text(pos2(x0 + cell * (i as f32 + 0.5), y0 + 8.0), Align2::CENTER_CENTER, letter, font(11.0), t.text_faint);
    }
    let mut go: Option<i64> = None;
    for (i, day) in grid.iter().enumerate() {
        let (row, col) = (i / 7, i % 7);
        let r = Rect::from_min_size(pos2(x0 + cell * col as f32, y0 + 18.0 + cell * row as f32), vec2(cell, cell));
        let c = date_of(*day);
        let in_month = c.month == month;
        if k.shown.contains(day) {
            p.rect_filled(r.shrink(1.0), 2.0, t.selected);
        }
        let resp = ui.interact(r, egui::Id::new((k.salt, "day", *day)), Sense::click());
        if resp.hovered() {
            p.rect_filled(r.shrink(1.0), 2.0, t.hover);
        }
        let color = if *day == k.today {
            p.circle_filled(r.center(), cell * 0.42, t.accent);
            t.on_accent
        } else if in_month {
            t.text
        } else {
            t.text_faint
        };
        p.text(r.center(), Align2::CENTER_CENTER, c.day.to_string(), if *day == k.today { bold(11.5) } else { font(11.5) }, color);
        if resp.clicked() {
            go = Some(*day);
        }
    }
    Picked { bottom: y0 + 18.0 + cell * 6.0, day: go }
}

fn chevron(p: &Painter, c: egui::Pos2, back: bool, color: Color32) {
    let d = if back { -1.0 } else { 1.0 };
    let s = Stroke::new(1.6, color);
    p.line_segment([pos2(c.x - 3.0 * d, c.y - 5.5), pos2(c.x + 2.5 * d, c.y)], s);
    p.line_segment([pos2(c.x + 2.5 * d, c.y), pos2(c.x - 3.0 * d, c.y + 5.5)], s);
}

/// `#RRGGBB` as a colour; a server's colour may carry alpha after it (`#RRGGBBAA`), which is left out.
pub(crate) fn hex_color(text: &str) -> Option<Color32> {
    let hex = text.trim().strip_prefix('#')?;
    let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    (hex.len() == 6 || hex.len() == 8).then_some(())?;
    Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// How light a colour is, 0 to 1, near enough to choose a mark's colour over it.
fn luminance(c: Color32) -> f32 {
    (0.2126 * f32::from(c.r()) + 0.7152 * f32::from(c.g()) + 0.0722 * f32::from(c.b())) / 255.0
}
