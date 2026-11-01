//! Printing the Calendar and People, in the styles Outlook Classic offers for them: Daily,
//! Weekly Agenda, Monthly and Calendar Details Style for appointments, and Card and Phone
//! Directory Style for contacts and groups. Each is laid out as tables by
//! `openmailclient_layout::sheets`, a day, a week or a month to a page.

use std::collections::BTreeMap;
use std::sync::Arc;

use openmailclient_fonts::Library;
use openmailclient_formats::rich::{Align, Block, Cell, Edge, Frame, Line, Para, Run, Style as Look, Table, TableWidth, VAlign};
use openmailclient_formats::vcf::Card;
use openmailclient_layout::{Fonts, LayoutError};
use openmailclient_model::ItemId;
use openmailclient_model::calendar::{self, Appointment, View};
use openmailclient_model::time::{self, Civil, WEEKDAYS};
use serde_json::Value;

use crate::print::{Job, Laid, Pictures, Style};
use crate::{EngineError, Module, Session};

/// The most days one print of the Calendar covers.
const MAX_DAYS: i64 = 400;
/// Characters of one line of a print, and of an appointment's notes.
const MAX_TEXT: usize = 300;
const MAX_NOTES: usize = 2_000;
/// Lines of notes printed for one appointment or one contact.
const MAX_NOTE_LINES: usize = 40;
/// Appointments whose notes are read for Calendar Details, and people printed.
const MAX_NOTED: usize = 400;
const MAX_PEOPLE: usize = 5_000;
/// The hours a Daily page shows at the least: from seven in the morning to seven at night.
const FIRST_HOUR: u32 = 7;
const LAST_HOUR: u32 = 19;
/// A day's head, the grid's lines and the days of another month.
const SHADE: [u8; 3] = [232, 232, 232];
const LINES: [u8; 3] = [140, 140, 140];
const DIM: [u8; 3] = [120, 120, 120];
/// Of the room a page has, what a grid fills: a little is left over, so that rounding never
/// sends its last row on to a page of its own.
const FILLED: f32 = 0.94;

/// What a print of the Calendar is made of: the days it covers and their appointments.
#[derive(Clone, Debug, Default)]
pub struct Gathered {
    pub style: Style,
    /// The days printed, first to last, as local day numbers.
    pub days: Vec<i64>,
    /// The appointments of those days, each occurrence of a series on its own, earliest first.
    pub items: Vec<(u64, Appointment)>,
    /// What each appointment's notes say, by item id: read for Calendar Details alone.
    pub notes: BTreeMap<u64, String>,
}

/// The day `day` (days since 1970-01-01) as a date.
fn civil_of(day: i64) -> Civil {
    time::civil(time::from_unix(day.saturating_mul(86_400)), 0)
}

/// The first and last days of the month `day` is in.
fn month_of(day: i64) -> (i64, i64) {
    let c = civil_of(day);
    let first = day.saturating_sub(i64::from(c.day.saturating_sub(1)));
    (first, first.saturating_add(i64::from(calendar::days_in(c.year, c.month)).saturating_sub(1)))
}

/// The days a print in `style` covers: those `"start"` and `"end"` give, or else the days the
/// Calendar shows (in a month's view, the month chosen), widened to whole weeks for Weekly
/// Agenda and whole months for Monthly.
fn span(s: &Session, style: Style, p: &Value) -> Result<(i64, i64), EngineError> {
    let from = crate::cmd::calendar::date_param(s, p, "start")?;
    let to = crate::cmd::calendar::date_param(s, p, "end")?;
    let chosen = if s.calendar.day == 0 { s.today() } else { s.calendar.day };
    let shown = s.calendar_days();
    let (mut first, mut last) = match (from, to) {
        (Some(from), Some(to)) => (from, to),
        (Some(day), None) | (None, Some(day)) => (day, day),
        (None, None) if s.calendar.view == View::Month => month_of(chosen),
        (None, None) => (shown.first().copied().unwrap_or(chosen), shown.last().copied().unwrap_or(chosen)),
    };
    if last < first {
        return Err(EngineError::BadParams("\"end\" is before \"start\"".into()));
    }
    match style {
        Style::WeeklyAgenda => {
            first = s.region.week_begun(&civil_of(first));
            last = s.region.week_begun(&civil_of(last)).saturating_add(6);
        }
        Style::Monthly => {
            first = month_of(first).0;
            last = month_of(last).1;
        }
        _ => {}
    }
    if last.saturating_sub(first) >= MAX_DAYS {
        return Err(EngineError::BadParams(format!("a print of the Calendar covers at most {MAX_DAYS} days")));
    }
    Ok((first, last))
}

/// Gather what a print of the Calendar in `style` shows, for the days `p` asks for, into
/// [`Session::print_calendar`]. A style of another module needs nothing gathered.
pub fn gather(s: &mut Session, style: Style, p: &Value) -> Result<(), EngineError> {
    if style.module() != Module::Calendar {
        return Ok(());
    }
    let (first, last) = span(s, style, p)?;
    let from = calendar::day_start(first, &s.zone);
    let to = calendar::day_start(last.saturating_add(1), &s.zone);
    let (items, _) = s.appointments_between(from, to)?;
    let mut notes = BTreeMap::new();
    if style == Style::CalendarDetails
        && let Some((store, folder)) = crate::cmd::calendar::calendar_folder(s)
        && let Some(st) = s.stores.get_mut(store)
    {
        let mut ids: Vec<u64> = items.iter().map(|(id, _)| *id).collect();
        ids.sort_unstable();
        ids.dedup();
        for id in ids.into_iter().take(MAX_NOTED) {
            if let Ok(item) = st.load(folder, ItemId(id)) {
                let text: String = item.body_text().chars().take(MAX_NOTES).collect();
                if !text.trim().is_empty() {
                    notes.insert(id, text);
                }
            }
        }
    }
    s.print_calendar = Some(Gathered { style, days: (first..=last).collect(), items, notes });
    Ok(())
}

/// What a print in `style` is called: the name of the folder it is printed from.
pub fn title(s: &Session, style: Style) -> String {
    let named = |at: Option<(usize, openmailclient_model::FolderId)>| {
        at.and_then(|(store, folder)| s.folders.get(store).and_then(|list| list.iter().find(|f| f.id == folder)).map(|f| f.name.clone()))
    };
    match style.module() {
        Module::Calendar => named(crate::cmd::calendar::calendar_folder(s)).unwrap_or_else(|| "Calendar".into()),
        _ => named(s.people.folder).unwrap_or_else(|| "Contacts".into()),
    }
}

/// Lay out what the Calendar or People shows in `style`, in the fonts of `library`.
pub fn layout(s: &Session, style: Style, job: &Job, library: &Library) -> Result<Laid, EngineError> {
    let now = job.zone.civil(job.now);
    let mut setup = job.setup.clone();
    let title = title(s, style);
    setup.words = vec![
        ("Date".to_string(), job.region.date(&now)),
        ("Time".to_string(), job.region.time(&now)),
        ("User".to_string(), job.owner.to_string()),
        ("Folder".to_string(), title.clone()),
        ("Subject".to_string(), title.clone()),
    ];
    let mut fonts = Fonts::new(library);
    let printed = if style.module() == Module::Calendar {
        // What was gathered for this style, or else the days the Calendar shows.
        let shown;
        let gathered = match &s.print_calendar {
            Some(g) if g.style == style => g,
            _ => {
                shown = Gathered { style, days: s.calendar_days(), items: s.calendar.items.clone(), notes: BTreeMap::new() };
                &shown
            }
        };
        let make = |room: f32| calendar_sheets(s, style, gathered, room);
        openmailclient_layout::sheets(&make, setup, &mut fonts)
    } else {
        let make = |_: f32| vec![people_sheet(s, style, &title)];
        openmailclient_layout::sheets(&make, setup, &mut fonts)
    };
    let printed = printed.map_err(|e| match e {
        LayoutError::NoFont => EngineError::Format(e.to_string()),
    })?;
    Ok(Laid { links: 0, printed, pictures: Pictures::default() })
}

/// One line of a print: its text without control characters, cut short.
fn clean(text: &str, most: usize) -> String {
    let line: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).take(most).collect();
    line.trim().to_string()
}

/// A run: a space at either end is kept, one of them, to stand between it and the next.
fn run(text: &str, bold: bool, size: Option<f32>) -> Run {
    let inner = clean(text, MAX_TEXT);
    let before = if text.starts_with(char::is_whitespace) && !inner.is_empty() { " " } else { "" };
    let after = if text.ends_with(char::is_whitespace) && !inner.is_empty() { " " } else { "" };
    Run { text: format!("{before}{inner}{after}"), style: Look { bold, size, ..Look::default() }, ..Run::default() }
}

fn para(text: &str, bold: bool, size: Option<f32>) -> Para {
    Para { runs: vec![run(text, bold, size)], ..Para::default() }
}

fn dim(mut p: Para) -> Para {
    for r in &mut p.runs {
        r.style.color = Some(DIM);
    }
    p
}

/// A page's heading over a thick line, with a line under it in smaller letters.
fn heading(text: &str, under: &str) -> Vec<Block> {
    let line = Edge { width: 3, color: Some([0, 0, 0]), line: Line::Solid };
    let frame = Arc::new(Frame { edges: [Edge::default(), Edge::default(), line, Edge::default()], padding: [2, 0, 4, 0], ..Frame::default() });
    let mut head = para(text, true, Some(16.0));
    head.frame = Some(frame.clone());
    let mut blocks = vec![Block::Para(head)];
    if !under.is_empty() {
        let mut sub = dim(para(under, false, Some(9.0)));
        sub.frame = Some(frame);
        blocks.push(Block::Para(sub));
    }
    blocks
}

/// A cell of a grid: what it holds at its top, at least `tall` pixels tall.
fn cell(blocks: Vec<Block>, tall: u16) -> Cell {
    Cell { blocks, span: 1, rows: 1, valign: VAlign::Top, padding: Some([3, 4, 3, 4]), tall, ..Cell::default() }
}

fn grid(rows: Vec<Vec<Cell>>) -> Table {
    Table { rows, border: true, width: TableWidth::Fill, padding: 3, line: Some(LINES), ..Table::default() }
}

/// Pixels of height each of `count` rows has, sharing `room` less what stands above them.
fn share(room: f32, above: f32, count: usize) -> u16 {
    let each = (room - above).max(0.0) * FILLED / count.max(1) as f32;
    each.clamp(16.0, 2_000.0) as u16
}

/// The first and last days `a` is on, where dates are shown.
fn days_of(a: &Appointment, s: &Session) -> (i64, i64) {
    let first = calendar::day_of(a.start, &s.zone);
    (first, calendar::day_of(a.end.saturating_sub(1).max(a.start), &s.zone).max(first))
}

/// An appointment that takes up whole days: an all-day event, or one that goes on past a day.
fn whole_days(a: &Appointment, s: &Session) -> bool {
    let (first, last) = days_of(a, s);
    a.all_day || last > first
}

fn on<'a>(items: &'a [(u64, Appointment)], day: i64, s: &Session) -> Vec<&'a Appointment> {
    items
        .iter()
        .map(|(_, a)| a)
        .filter(|a| {
            let (first, last) = days_of(a, s);
            first <= day && day <= last
        })
        .collect()
}

fn subject(a: &Appointment) -> String {
    let subject = clean(&a.subject, MAX_TEXT);
    let subject = if subject.is_empty() { "(No subject)".to_string() } else { subject };
    let location = clean(&a.location, MAX_TEXT);
    if location.is_empty() { subject } else { format!("{subject} ({location})") }
}

fn clock(at: i64, s: &Session) -> String {
    s.region.time(&s.zone.civil(at))
}

/// When `a` is on `day`, as a print of that day says it: its start, or that it goes on from
/// before.
fn starts(a: &Appointment, day: i64, s: &Session) -> String {
    if a.all_day {
        String::new()
    } else if days_of(a, s).0 == day {
        clock(a.start, s)
    } else {
        "Continued".to_string()
    }
}

/// A line for `a` on `day`: its time in bold, then what it is.
fn line(a: &Appointment, day: i64, s: &Session, size: Option<f32>) -> Para {
    let when = starts(a, day, s);
    let mut runs = Vec::new();
    if !when.is_empty() {
        runs.push(run(&format!("{when} "), true, size));
    }
    runs.push(run(&subject(a), false, size));
    Para { runs, ..Para::default() }
}

/// A day's appointments, those of whole days first, as lines; at most `most` of them, the
/// last saying how many more there are.
fn lines(list: &[&Appointment], day: i64, s: &Session, size: Option<f32>, most: usize) -> Vec<Block> {
    let mut sorted: Vec<&&Appointment> = list.iter().collect();
    sorted.sort_by_key(|a| (!whole_days(a, s), a.start));
    let most = most.max(1);
    let mut out: Vec<Block> = Vec::new();
    for (n, a) in sorted.iter().enumerate() {
        if n + 1 == most && sorted.len() > most {
            out.push(Block::Para(dim(para(&format!("{} more", sorted.len() - n), false, size))));
            break;
        }
        let mut p = line(a, day, s, size);
        if whole_days(a, s) {
            for r in &mut p.runs {
                r.style.background = Some(SHADE);
            }
        }
        out.push(Block::Para(p));
    }
    out
}

/// "November 2026".
fn month_words(c: &Civil) -> String {
    format!("{} {}", c.month_name(), c.year)
}

/// "November 2 – 8, 2026", or with months and years named where they change.
fn week_words(first: &Civil, last: &Civil) -> String {
    if first.year != last.year {
        format!("{} {}, {} – {} {}, {}", first.month_name(), first.day, first.year, last.month_name(), last.day, last.year)
    } else if first.month != last.month {
        format!("{} {} – {} {}, {}", first.month_name(), first.day, last.month_name(), last.day, last.year)
    } else {
        format!("{} {} – {}, {}", first.month_name(), first.day, last.day, last.year)
    }
}

/// The pages of a print of the Calendar: one for each day, week or month, or for Calendar
/// Details one long list. `room` is the height a page has for them, in pixels.
fn calendar_sheets(s: &Session, style: Style, g: &Gathered, room: f32) -> Vec<Vec<Block>> {
    let (Some(first), Some(last)) = (g.days.first().copied(), g.days.last().copied()) else { return Vec::new() };
    match style {
        Style::Daily => g.days.iter().map(|day| daily(s, &g.items, *day, room)).collect(),
        Style::WeeklyAgenda => {
            let mut out = Vec::new();
            let mut week = s.region.week_begun(&civil_of(first));
            while week <= last {
                out.push(weekly(s, &g.items, week, room));
                week = week.saturating_add(7);
            }
            out
        }
        Style::Monthly => {
            let mut out = Vec::new();
            let mut month = month_of(first).0;
            while month <= last {
                out.push(monthly(s, &g.items, month, room));
                month = month_of(month).1.saturating_add(1);
            }
            out
        }
        _ => vec![details(s, g, first, last)],
    }
}

/// Daily Style: the day's whole-day appointments at its head, then its hours, an hour to a
/// row, each appointment in the hour it starts in.
fn daily(s: &Session, items: &[(u64, Appointment)], day: i64, room: f32) -> Vec<Block> {
    let c = civil_of(day);
    let mut out = heading(&s.region.long_date(&c), "");
    let list = on(items, day, s);
    let (whole, timed): (Vec<&Appointment>, Vec<&Appointment>) = list.iter().partition(|a| whole_days(a, s));
    let mut used = 48.0;
    if !whole.is_empty() {
        let blocks = lines(&whole, day, s, None, usize::MAX);
        used += 12.0 + 18.0 * blocks.len() as f32;
        out.push(Block::Table(grid(vec![vec![Cell { background: Some(SHADE), ..cell(blocks, 0) }]])));
    }
    let hour = |at: i64| s.zone.civil(at).hour;
    let earliest = timed.iter().map(|a| hour(a.start)).min().unwrap_or(FIRST_HOUR).min(FIRST_HOUR);
    let latest = timed
        .iter()
        .map(|a| {
            let end = s.zone.civil(a.end);
            if calendar::day_of(a.end, &s.zone) > day { 24 } else { end.hour + u32::from(end.minute > 0) }
        })
        .max()
        .unwrap_or(LAST_HOUR)
        .clamp(LAST_HOUR, 24);
    let hours = latest.saturating_sub(earliest).max(1);
    let tall = share(room, used, hours as usize);
    let rows = (earliest..earliest + hours)
        .map(|h| {
            let at = Civil { hour: h, minute: 0, second: 0, ..c };
            let starting: Vec<&Appointment> = timed.iter().copied().filter(|a| hour(a.start) == h && days_of(a, s).0 == day).collect();
            let mut time = cell(vec![Block::Para(para(&s.region.time(&at), true, None))], tall);
            time.share = 14;
            time.background = Some(SHADE);
            let mut what = cell(lines(&starting, day, s, None, usize::MAX), tall);
            what.share = 86;
            vec![time, what]
        })
        .collect();
    out.push(Block::Table(Table { snug: true, ..grid(rows) }));
    out
}

/// Weekly Agenda Style: the week's seven days in two columns, each with its appointments.
fn weekly(s: &Session, items: &[(u64, Appointment)], week: i64, room: f32) -> Vec<Block> {
    let mut out = heading(&week_words(&civil_of(week), &civil_of(week.saturating_add(6))), "");
    let tall = share(room, 48.0, 4);
    let day_cell = |day: i64| {
        let c = civil_of(day);
        let mut head = para(&format!("{} {}", c.weekday_name(), c.day), true, Some(10.0));
        head.runs.push(run(&format!("  {}", c.month_name()), false, Some(8.0)));
        let line = Edge { width: 1, color: Some(LINES), line: Line::Solid };
        head.frame =
            Some(Arc::new(Frame { edges: [Edge::default(), Edge::default(), line, Edge::default()], padding: [0, 0, 2, 0], ..Frame::default() }));
        let mut blocks = vec![Block::Para(head)];
        blocks.extend(lines(&on(items, day, s), day, s, None, usize::MAX));
        Cell { share: 50, ..cell(blocks, tall) }
    };
    let rows = (0..4)
        .map(|r| {
            let left = day_cell(week.saturating_add(r));
            let right = if r < 3 { day_cell(week.saturating_add(r + 4)) } else { Cell { share: 50, ..cell(Vec::new(), tall) } };
            vec![left, right]
        })
        .collect();
    out.push(Block::Table(grid(rows)));
    out
}

/// Monthly Style: the month's weeks as a grid under the names of the days, each day with as
/// many of its appointments as there is room for.
fn monthly(s: &Session, items: &[(u64, Appointment)], month: i64, room: f32) -> Vec<Block> {
    let (first, last) = month_of(month);
    let c = civil_of(first);
    let mut out = heading(&month_words(&c), "");
    let start = s.region.week_begun(&c);
    let weeks = usize::try_from((last.saturating_sub(start)).div_euclid(7).saturating_add(1)).unwrap_or(6).clamp(4, 6);
    let tall = share(room, 70.0, weeks);
    // An appointment's line, in the size the grid writes them, is about thirteen pixels.
    let fit = (usize::from(tall).saturating_sub(22) / 13).max(1);
    let mut names = Vec::with_capacity(7);
    for n in 0..7u32 {
        let weekday = (s.region.week_start + n) % 7;
        let mut p = para(WEEKDAYS.get(weekday as usize).copied().unwrap_or(""), true, Some(8.0));
        p.align = Align::Center;
        names.push(Cell { header: true, background: Some(SHADE), share: 14, ..cell(vec![Block::Para(p)], 0) });
    }
    let mut rows = vec![names];
    for w in 0..weeks {
        let row = (0..7i64)
            .map(|d| {
                let day = start.saturating_add(i64::try_from(w).unwrap_or(0).saturating_mul(7)).saturating_add(d);
                let inside = (first..=last).contains(&day);
                let number = para(&civil_of(day).day.to_string(), inside, Some(9.0));
                let mut blocks = vec![Block::Para(if inside { number } else { dim(number) })];
                blocks.extend(lines(&on(items, day, s), day, s, Some(7.5), fit));
                Cell { share: 14, ..cell(blocks, tall) }
            })
            .collect();
        rows.push(row);
    }
    out.push(Block::Table(grid(rows)));
    out
}

/// Calendar Details Style: each day that has appointments, and under it each of them with
/// its times, where it is, how it repeats and what its notes say.
fn details(s: &Session, g: &Gathered, first: i64, last: i64) -> Vec<Block> {
    let range = if first == last {
        s.region.long_date(&civil_of(first))
    } else {
        format!("{} – {}", s.region.long_date(&civil_of(first)), s.region.long_date(&civil_of(last)))
    };
    let mut out = heading("Calendar Details", &range);
    let mut any = false;
    for day in first..=last {
        let list = on(&g.items, day, s);
        if list.is_empty() {
            continue;
        }
        any = true;
        let line = Edge { width: 1, color: Some(LINES), line: Line::Solid };
        let mut head = para(&s.region.long_date(&civil_of(day)), true, Some(11.0));
        head.frame =
            Some(Arc::new(Frame { edges: [Edge::default(), Edge::default(), line, Edge::default()], padding: [8, 0, 2, 0], ..Frame::default() }));
        out.push(Block::Para(head));
        let mut rows = Vec::new();
        let mut sorted = list;
        sorted.sort_by_key(|a| (!whole_days(a, s), a.start));
        for a in sorted {
            let when = if a.all_day {
                "All day".to_string()
            } else {
                let (from, to) = days_of(a, s);
                let start = if from == day { clock(a.start, s) } else { String::new() };
                let end = if to == day { clock(a.end, s) } else { String::new() };
                format!("{start} – {end}").trim().to_string()
            };
            let mut what = vec![Block::Para(para(&subject(a), true, None))];
            if a.recurring {
                what.push(Block::Para(dim(para(&format!("Recurrence: {}", a.pattern_words()), false, Some(8.0)))));
            }
            if a.meeting {
                what.push(Block::Para(dim(para("Meeting", false, Some(8.0)))));
            }
            if let Some(notes) = g.items.iter().find(|(_, b)| std::ptr::eq(a, b)).and_then(|(id, _)| g.notes.get(id)) {
                for text in notes.lines().map(str::trim_end).take(MAX_NOTE_LINES) {
                    what.push(Block::Para(para(text, false, Some(8.5))));
                }
            }
            let mut time = cell(vec![Block::Para(para(&when, false, None))], 0);
            time.share = 25;
            let mut said = cell(what, 0);
            said.share = 75;
            rows.push(vec![time, said]);
        }
        out.push(Block::Table(Table { border: false, ..grid(rows) }));
    }
    if !any {
        out.push(Block::Para(dim(para("There are no appointments on these days.", false, None))));
    }
    out
}

/// The letter a name is filed under, or "#" for one that starts with no letter.
fn letter(name: &str) -> String {
    match name.trim().chars().next() {
        Some(c) if c.is_alphabetic() => c.to_uppercase().collect(),
        _ => "#".to_string(),
    }
}

/// A row that names the letter the names under it are filed by.
fn letter_row(letter: &str, columns: u16) -> Vec<Cell> {
    vec![Cell { span: columns, background: Some(SHADE), ..cell(vec![Block::Para(para(letter, true, Some(14.0)))], 0) }]
}

/// A name and what it says, for a card: "Business: 555 0100"; without a name, a line that goes
/// on from the one above.
fn field(out: &mut Vec<Block>, label: &str, value: &str) {
    let value = clean(value, MAX_TEXT);
    if !value.is_empty() {
        let mut p = Para::default();
        if !label.is_empty() {
            let mut name = run(&format!("{label}: "), false, Some(8.5));
            name.style.color = Some(DIM);
            p.runs.push(name);
        }
        p.runs.push(run(&value, false, Some(8.5)));
        out.push(Block::Para(p));
    }
}

/// What a card says of a contact or a group, under its name in a bar.
fn card(entry: &Card, s: &Session) -> Vec<Block> {
    let bar = |name: &str| {
        let mut p = para(name, true, Some(10.0));
        p.frame = Some(Arc::new(Frame { fill: Some(SHADE), padding: [2, 3, 2, 3], ..Frame::default() }));
        Block::Para(p)
    };
    match entry {
        Card::Contact(c) => {
            let mut out = vec![bar(&c.filed_as())];
            if c.display_name() != c.filed_as() {
                field(&mut out, "Full Name", &c.display_name());
            }
            field(&mut out, "Job Title", &c.job_title);
            field(&mut out, "Company", &c.company);
            field(&mut out, "Department", &c.department);
            for (place, address) in &c.addresses {
                for (n, text) in address.lines().lines().enumerate() {
                    field(&mut out, &if n == 0 { format!("{} Address", place.label()) } else { String::new() }, text);
                }
            }
            for (kind, number) in &c.phones {
                field(&mut out, kind.label(), number);
            }
            for (n, email) in c.emails.iter().enumerate() {
                field(&mut out, &if n == 0 { "E-mail".to_string() } else { format!("E-mail {}", n + 1) }, &email.address);
            }
            field(&mut out, "Web Page", &c.web_page);
            field(&mut out, "IM Address", &c.im);
            for (label, day) in [("Birthday", c.birthday), ("Anniversary", c.anniversary)] {
                if let Some(day) = day {
                    field(&mut out, label, &s.region.date(&civil_of(day)));
                }
            }
            field(&mut out, "Categories", &c.categories.join(", "));
            for text in c.notes.lines().take(MAX_NOTE_LINES) {
                if !text.trim().is_empty() {
                    out.push(Block::Para(para(text, false, Some(8.0))));
                }
            }
            out
        }
        Card::Group(g) => {
            let mut out = vec![bar(&g.name)];
            field(&mut out, "Members", &g.members.len().to_string());
            for m in &g.members {
                let shown = if m.name.trim().is_empty() || m.name == m.address { m.address.clone() } else { format!("{} <{}>", m.name, m.address) };
                out.push(Block::Para(para(&shown, false, Some(8.5))));
            }
            out
        }
    }
}

/// What a card or a line of the directory is filed under.
fn filed(entry: &Card) -> String {
    match entry {
        Card::Contact(c) => c.filed_as(),
        Card::Group(g) => g.name.clone(),
    }
}

/// Card and Phone Directory Style: the people shown, in the order they are filed, under the
/// letters they are filed by; cards two to a row, or each name with its numbers.
fn people_sheet(s: &Session, style: Style, title: &str) -> Vec<Block> {
    let entries: Vec<&Card> = s.people.items.iter().map(|(_, c)| c).take(MAX_PEOPLE).collect();
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut last_letter = String::new();
    let mut left_out = 0usize;
    let mut pending: Option<Cell> = None;
    for entry in &entries {
        let name = filed(entry);
        let phones: Vec<(String, String)> = match entry {
            Card::Contact(c) => c.phones.iter().filter(|(_, n)| !n.trim().is_empty()).map(|(k, n)| (k.label().to_string(), n.clone())).collect(),
            Card::Group(_) => Vec::new(),
        };
        if style == Style::PhoneDirectory && phones.is_empty() {
            left_out += 1;
            continue;
        }
        let first = letter(&name);
        if first != last_letter {
            if let Some(cell) = pending.take() {
                rows.push(vec![cell, Cell { share: 50, ..cell_of(Vec::new()) }]);
            }
            rows.push(letter_row(&first, 2));
            last_letter = first;
        }
        if style == Style::PhoneDirectory {
            let mut who = cell_of(vec![Block::Para(para(&name, false, Some(9.0)))]);
            who.share = 55;
            let numbers = phones
                .iter()
                .map(|(label, number)| {
                    let mut p = para(&format!("{label}  "), false, Some(8.0));
                    p.runs.first_mut().into_iter().for_each(|r| r.style.color = Some(DIM));
                    p.runs.push(run(number, false, Some(9.0)));
                    p.align = Align::Right;
                    Block::Para(p)
                })
                .collect();
            let mut said = cell_of(numbers);
            said.share = 45;
            let under = Edge { width: 1, color: Some(SHADE), line: Line::Solid };
            for c in [&mut who, &mut said] {
                c.edges = Some([Edge::default(), Edge::default(), under, Edge::default()]);
            }
            rows.push(vec![who, said]);
        } else {
            let one = Cell { share: 50, ..cell_of(card(entry, s)) };
            match pending.take() {
                Some(left) => rows.push(vec![left, one]),
                None => pending = Some(one),
            }
        }
    }
    if let Some(cell) = pending.take() {
        rows.push(vec![cell, Cell { share: 50, ..cell_of(Vec::new()) }]);
    }
    let shown = entries.len().saturating_sub(left_out);
    let mut note = format!("{shown} {}", if shown == 1 { "entry" } else { "entries" });
    if left_out > 0 {
        note.push_str(&format!("; {left_out} without a telephone number left out"));
    }
    let mut out = heading(title, &note);
    if rows.is_empty() {
        out.push(Block::Para(dim(para("There is no one to print.", false, None))));
    } else {
        let spacing = if style == Style::Card { 6 } else { 0 };
        out.push(Block::Table(Table { rows, border: false, width: TableWidth::Fill, padding: 2, spacing, ..Table::default() }));
    }
    out
}

fn cell_of(blocks: Vec<Block>) -> Cell {
    cell(blocks, 0)
}
