//! The Scheduling Assistant in the meeting window: each person's free and busy times as bars
//! across the working day of the meeting, the meeting's own time as a band over them, where
//! each person's times came from, and the times suggested, any of which moves the meeting
//! there. What it shows is what `calendar.freeBusy` last found (`Session::scheduling`).

use crate::theme::Tokens;
use crate::{font, one_line};
use egui::{Align2, Color32, Rect, Sense, Ui, pos2, vec2};
use openmailclient_engine::cmd::free_busy::{Scheduling, Source};
use openmailclient_model::calendar::{Busy, MINUTE};
use openmailclient_model::free_busy::{Block, takes};
use openmailclient_model::zone::Zone;

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// Suggested times offered as buttons, at most.
const SHOWN: usize = 6;
/// People drawn, at most; the rest are counted.
const ROWS: usize = 24;

/// What a click in the Scheduling Assistant asked for.
pub(crate) enum Pick {
    /// Look up everyone's times again.
    Look,
    /// Move the meeting to these times (UTC ticks).
    Use(i64, i64),
}

/// The Scheduling Assistant for a meeting now typed to run `span` (UTC ticks), when it can be read.
pub(crate) fn assistant(ui: &mut Ui, t: &Tokens, sch: Option<&Scheduling>, span: Option<(i64, i64)>, here: &Zone, looking: bool) -> Option<Pick> {
    let mut pick = None;
    ui.horizontal(|ui| {
        let label = if sch.is_some() { "Look Up Again" } else { "Look Up Free and Busy Times" };
        let r = ui.add_enabled(!looking, crate::dialog::plain(label)).on_hover_text(
            "Asks your calendar server, and the free/busy address each contact gives, when everyone asked is free; the times found stay on this machine",
        );
        if r.clicked() {
            pick = Some(Pick::Look);
        }
        if looking {
            ui.spinner();
            ui.label(egui::RichText::new("Looking…").font(font(12.0)).color(t.text_dim));
        }
    });
    let Some(sch) = sch else { return pick };
    // The day the meeting is on, or the first one looked at; within the span looked at.
    let day =
        span.map(|(s, _)| here.day_of(s)).filter(|d| (here.day_of(sch.from)..here.day_of(sch.to.saturating_sub(1)).saturating_add(1)).contains(d));
    match day {
        Some(day) => rows(ui, t, sch, day, span, here),
        None => {
            ui.label(egui::RichText::new("The meeting is now on a day not looked up; look up again to see it").font(font(12.0)).color(t.text_dim));
        }
    }
    if !sch.suggested.is_empty() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Suggested times").font(font(12.0)).color(t.text_dim));
        ui.horizontal_wrapped(|ui| {
            for g in sch.suggested.iter().take(SHOWN) {
                let (a, b) = (here.civil(g.start), here.civil(g.end));
                let weekday = WEEKDAYS.get(a.weekday as usize).copied().unwrap_or("");
                let words = format!("{weekday} {} {:02}:{:02}–{:02}:{:02}", a.day, a.hour, a.minute, b.hour, b.minute);
                let free =
                    if g.free == g.known { "Everyone whose times are known is free".to_string() } else { format!("{} of {} free", g.free, g.known) };
                let chosen = span == Some((g.start, g.end));
                if ui.selectable_label(chosen, words).on_hover_text(free).clicked() {
                    pick = Some(Pick::Use(g.start, g.end));
                }
            }
        });
    } else if span.is_some() {
        ui.label(egui::RichText::new("No time in the days looked up suits everyone who must come").font(font(12.0)).color(t.text_dim));
    }
    pick
}

/// Each person's row: their name, and their day as a bar of busy times.
fn rows(ui: &mut Ui, t: &Tokens, sch: &Scheduling, day: i64, span: Option<(i64, i64)>, here: &Zone) {
    let midnight = here.day_start(day);
    let mut open = midnight.saturating_add(i64::from(sch.work.start) * MINUTE);
    let mut close = midnight.saturating_add(i64::from(sch.work.end) * MINUTE);
    // The meeting shown whole, though it be outside working hours.
    if let Some((s, e)) = span {
        open = open.min(s.saturating_sub(s.saturating_sub(midnight).rem_euclid(60 * MINUTE)));
        close = close.max(e);
    }
    let length = close.saturating_sub(open).max(MINUTE);
    let name_w = 170.0;
    let bar_w = (ui.available_width() - name_w - 8.0).max(160.0);
    let x_of = |at: i64, r: Rect| r.left() + bar_w * ((at.clamp(open, close) - open) as f32 / length as f32);

    // The hours along the top.
    let (head, _) = ui.allocate_exact_size(vec2(name_w + 8.0 + bar_w, 14.0), Sense::hover());
    let bar_head = Rect::from_min_size(pos2(head.left() + name_w + 8.0, head.top()), vec2(bar_w, 14.0));
    let p = ui.painter();
    let mut hour = open.saturating_sub(open.saturating_sub(midnight).rem_euclid(60 * MINUTE));
    while hour <= close {
        if hour >= open {
            let c = here.civil(hour);
            p.text(pos2(x_of(hour, bar_head), bar_head.center().y), Align2::LEFT_CENTER, format!("{:02}", c.hour), font(10.0), t.text_faint);
        }
        hour = hour.saturating_add(60 * MINUTE);
    }
    for person in sch.people.iter().take(ROWS) {
        let (row, response) = ui.allocate_exact_size(vec2(name_w + 8.0 + bar_w, 20.0), Sense::hover());
        let p = ui.painter();
        let mut name = one_line(if person.name.trim().is_empty() { &person.address } else { &person.name });
        if person.organizer {
            name.push_str(" (you)");
        } else if !person.required {
            name.push_str(" (optional)");
        }
        let name_r = Rect::from_min_size(row.min, vec2(name_w, row.height()));
        crate::clip_text(p, name_r.left_center() - vec2(0.0, 7.0), name_w, &name, font(12.0), t.text);
        let bar = Rect::from_min_size(pos2(row.left() + name_w + 8.0, row.top() + 3.0), vec2(bar_w, row.height() - 6.0));
        p.rect_filled(bar, 2.0, t.pane);
        p.rect_stroke(bar, 2.0, (1.0, t.line), egui::StrokeKind::Inside);
        let tip = match (&person.blocks, person.source) {
            (None, _) => format!("{}: not known, {}", person.address, one_line(&person.said)),
            (Some(_), Source::Calendar) => "From your calendar".to_string(),
            (Some(_), Source::Server) => format!("{}: from your calendar server", person.address),
            (Some(_), Source::Internet) => format!("{}: from the free/busy address their contact gives", person.address),
            (Some(_), Source::None) => person.address.clone(),
        };
        match &person.blocks {
            None => {
                // Not known: hatched, so it is not read as free.
                let mut x = bar.left();
                while x < bar.right() {
                    p.line_segment([pos2(x, bar.bottom()), pos2((x + bar.height()).min(bar.right()), bar.top())], (1.0, t.text_faint));
                    x += 6.0;
                }
            }
            Some(blocks) => {
                for b in blocks.iter().filter(|b| b.end > open && b.start < close) {
                    let r =
                        Rect::from_min_max(pos2(x_of(b.start, bar), bar.top()), pos2(x_of(b.end, bar).max(x_of(b.start, bar) + 2.0), bar.bottom()));
                    paint_block(p, r, b, t);
                }
            }
        }
        response.on_hover_text(tip);
    }
    if sch.people.len() > ROWS {
        ui.label(egui::RichText::new(format!("and {} more", sch.people.len() - ROWS)).font(font(12.0)).color(t.text_dim));
    }
    // The meeting's time over every row, red where someone who must come is taken.
    if let Some((s, e)) = span.filter(|(s, e)| e > s) {
        let top = head.bottom();
        let bottom = ui.min_rect().bottom();
        let band = Rect::from_min_max(pos2(x_of(s, bar_head), top), pos2(x_of(e, bar_head).max(x_of(s, bar_head) + 2.0), bottom));
        let clash =
            sch.people.iter().any(|p| p.required && p.blocks.as_deref().is_some_and(|b| b.iter().any(|b| b.start < e && b.end > s && takes(b.busy))));
        let edge = if clash { t.danger } else { t.ok };
        ui.painter().rect_filled(band, 0.0, Color32::from_rgba_unmultiplied(edge.r(), edge.g(), edge.b(), 28));
        ui.painter().rect_stroke(band, 0.0, (1.5, edge), egui::StrokeKind::Outside);
    }
    ui.horizontal(|ui| {
        for (busy, words) in [(Busy::Busy, "Busy"), (Busy::Tentative, "Tentative"), (Busy::OutOfOffice, "Out of Office")] {
            let (r, _) = ui.allocate_exact_size(vec2(18.0, 10.0), Sense::hover());
            paint_block(ui.painter(), r, &Block { start: 0, end: 1, busy }, t);
            ui.label(egui::RichText::new(words).font(font(11.0)).color(t.text_dim));
        }
    });
}

/// A busy time, coloured as the calendar colours appointments shown so.
fn paint_block(p: &egui::Painter, r: Rect, b: &Block, t: &Tokens) {
    match b.busy {
        Busy::Tentative => {
            p.rect_filled(r, 1.0, t.accent.gamma_multiply(0.25));
            let mut x = r.left();
            while x < r.right() {
                p.line_segment([pos2(x, r.bottom()), pos2((x + r.height()).min(r.right()), r.top())], (1.5, t.accent));
                x += 5.0;
            }
        }
        Busy::OutOfOffice => {
            p.rect_filled(r, 1.0, t.away);
        }
        Busy::Free | Busy::WorkingElsewhere => {}
        Busy::Busy => {
            p.rect_filled(r, 1.0, t.accent);
        }
    }
}
