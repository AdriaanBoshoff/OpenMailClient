//! The Calendar's Schedule View: one day across, each calendar of the data files open a row of
//! it, and above them a row of when anyone is taken up. What is shown is the engine's
//! (`Session::calendar.rows`); every change goes through the `calendar.*` commands, and a
//! calendar not shown is opened (`calendar.open`) before anything is done in it.

use crate::calendar::{AppointmentForm, WORK, date_of, hex_color, hm, select_params, ymd};
use crate::todo_bar::{TaskDrag, task_dropped, task_over};
use crate::{MailUi, bold, clip_text, font, one_line};
use egui::{Align2, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use openmailclient_engine::cmd::calendar::busy_of;
use openmailclient_model::calendar::{self, MINUTE};
use serde_json::json;

/// The width of an hour.
const HOUR_W: f32 = 72.0;
/// The column naming each calendar.
const NAMES: f32 = 176.0;
/// The row of hours along the top.
const HEAD: f32 = 30.0;
/// The row of when anyone is taken up.
const BUSY_H: f32 = 24.0;
/// One calendar's row.
const ROW_H: f32 = 50.0;
/// Appointments at the same time stand one above another, this many at most.
const LANES: usize = 3;

/// What a click asks for, done once the view is drawn; rows are named by their place.
enum Asked {
    Select(usize, u64, Option<i64>),
    Open(usize, u64, Option<i64>),
    New(usize, i64),
    Show(usize),
    Nothing(usize),
    Task(TaskDrag, String),
}

impl MailUi {
    /// The day across, a calendar to each row, scrolled to the working day when first shown.
    pub(crate) fn schedule_grid(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let off = self.session.zone.clone();
        let today = self.session.today();
        let day = self.session.calendar_days().first().copied().unwrap_or(today);
        let from = calendar::day_start(day, &off);
        let to = calendar::day_start(day.saturating_add(1), &off).max(from.saturating_add(MINUTE));
        let rows = self.session.calendar.rows.clone();
        let shown = self.session.calendar.folder;
        let (selected, occurrence) = (self.session.calendar.selected, self.session.calendar.occurrence);
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.surface);
        if rows.is_empty() {
            p.text(rect.center(), Align2::CENTER_CENTER, "No calendar in the data files open", font(13.0), t.text_dim);
            return;
        }
        let busy = busy_of(&rows, from, to);
        let minutes = to.saturating_sub(from) / MINUTE;
        let width = HOUR_W * minutes as f32 / 60.0;
        let is_shown = |i: usize| rows.get(i).is_some_and(|r| shown == Some((r.store, r.folder)));
        let scroll_to = std::mem::take(&mut self.cal_scroll);
        let workday = (1..=5).contains(&date_of(day).weekday);
        let now = self.session.time_now();
        let mut asked: Option<Asked> = None;

        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            ScrollArea::both().id_salt("cal-schedule").auto_shrink(false).show(ui, |ui| {
                let height = (HEAD + BUSY_H + ROW_H * rows.len() as f32).max(rect.height() - 14.0);
                let (canvas, _) = ui.allocate_exact_size(vec2(NAMES + width + 12.0, height), Sense::hover());
                let clip = ui.clip_rect();
                let p = ui.painter_at(canvas.intersect(clip));
                let left = canvas.left() + NAMES;
                let x_of = |at: i64| left + (at.clamp(from, to).saturating_sub(from) / MINUTE) as f32 / 60.0 * HOUR_W;
                // The half hour a point along the day is in.
                let half_at = |x: f32| {
                    let last = (minutes / 30).saturating_sub(1).max(0) as f32;
                    from.saturating_add(((x - left) / (HOUR_W / 2.0)).floor().clamp(0.0, last) as i64 * 30 * MINUTE)
                };
                let busy_top = canvas.top() + HEAD;
                let rows_top = busy_top + BUSY_H;
                let row_rect = |i: usize| Rect::from_min_size(pos2(left, rows_top + ROW_H * i as f32), vec2(width, ROW_H));

                // The day: out of hours darker, the working day as the day views shade it.
                p.rect_filled(Rect::from_min_max(pos2(left, busy_top), pos2(left + width, canvas.bottom())), 0.0, t.pane);
                if workday {
                    let (a, b) = (from + i64::from(WORK.0) * 60 * MINUTE, from + i64::from(WORK.1) * 60 * MINUTE);
                    p.rect_filled(Rect::from_min_max(pos2(x_of(a), busy_top), pos2(x_of(b), canvas.bottom())), 0.0, t.surface);
                }
                let hours = minutes / 60;
                for h in 0..=hours {
                    let x = left + HOUR_W * h as f32;
                    p.vline(x + 0.5, busy_top..=canvas.bottom(), (1.0, t.line));
                    if h < hours {
                        p.vline(x + HOUR_W / 2.0 + 0.5, rows_top..=canvas.bottom(), (1.0, t.line.gamma_multiply(0.45)));
                    }
                }
                p.hline(left..=left + width, rows_top - 0.5, (1.0, t.line));
                for i in 0..=rows.len() {
                    p.hline(left..=left + width, rows_top + ROW_H * i as f32 - 0.5, (1.0, t.line));
                }

                // When anyone is taken up, across every calendar.
                for (a, b) in &busy {
                    let r = Rect::from_min_max(pos2(x_of(*a), busy_top + 5.0), pos2(x_of(*b).max(x_of(*a) + 3.0), rows_top - 5.0));
                    p.rect_filled(r, 2.0, t.accent.gamma_multiply(0.55));
                }

                // Each calendar's row: a click where nothing is there, then its appointments.
                for (i, row) in rows.iter().enumerate() {
                    let r = row_rect(i);
                    if is_shown(i) {
                        p.rect_filled(r, 0.0, t.accent.gamma_multiply(0.06));
                    }
                    let resp = ui.interact(r, egui::Id::new(("cal-schedule-row", row.store, row.folder.0)), Sense::click());
                    if let Some(at) = resp.interact_pointer_pos() {
                        if resp.double_clicked() {
                            asked = Some(Asked::New(i, half_at(at.x)));
                        } else if resp.clicked() {
                            asked = Some(Asked::Nothing(i));
                        }
                    }
                    // A task let go on the calendar shown is an appointment from that half hour.
                    if is_shown(i) {
                        if let Some(at) = task_over(ui, r) {
                            let x = x_of(half_at(at.x));
                            p.rect_filled(
                                Rect::from_min_size(pos2(x + 1.0, r.top() + 2.0), vec2(HOUR_W / 2.0 - 2.0, ROW_H - 4.0)),
                                2.0,
                                t.accent.gamma_multiply(0.3),
                            );
                        }
                        if let Some((d, at)) = task_dropped(ui, r) {
                            let start = off.civil(half_at(at.x));
                            asked = Some(Asked::Task(d, format!("{} {}", ymd(&start), hm(&start))));
                        }
                    }
                    let spans: Vec<(i64, i64)> = row.items.iter().map(|(_, a)| (a.start.max(from), a.end.min(to))).collect();
                    let lanes = calendar::columns(&spans, 15 * MINUTE);
                    for (((id, a), (s, e)), (lane, of)) in row.items.iter().zip(&spans).zip(&lanes) {
                        if *lane >= LANES {
                            continue;
                        }
                        let h = (ROW_H - 6.0) / (*of).clamp(1, LANES) as f32;
                        let (x0, y0) = (x_of(*s), r.top() + 3.0 + h * *lane as f32);
                        let b = Rect::from_min_max(pos2(x0 + 1.0, y0), pos2(x_of(*e).max(x0 + 10.0) - 1.0, y0 + h - 1.0));
                        let chosen = is_shown(i) && selected == Some(*id) && (occurrence.is_none() || a.occurrence == occurrence);
                        let resp = self.appointment_box(ui, &p, b, *id, a, chosen, &format!("schedule-{i}"));
                        if resp.double_clicked() {
                            asked = Some(Asked::Open(i, *id, a.occurrence));
                        } else if resp.clicked() {
                            asked = Some(Asked::Select(i, *id, a.occurrence));
                        }
                    }
                    if row.unreadable > 0 {
                        let words = format!("{} could not be read", row.unreadable);
                        p.text(pos2(left + width - 6.0, r.bottom() - 8.0), Align2::RIGHT_CENTER, words, font(10.0), t.danger);
                    }
                }

                // Now, on today.
                if day == today && (from..to).contains(&now) {
                    let x = x_of(now);
                    p.vline(x, busy_top..=canvas.bottom(), Stroke::new(1.5, t.danger));
                    p.circle_filled(pos2(x, busy_top), 4.0, t.danger);
                }

                // The hours along the top and the calendars' names at the left stay in sight
                // as the day is scrolled.
                let top = clip.top().max(canvas.top());
                let side = clip.left().max(canvas.left());
                // An hour's name is cut where the names begin, and not begun under them.
                let words = p.with_clip_rect(Rect::from_min_max(pos2(side + NAMES, top), pos2(clip.right(), top + HEAD)).intersect(p.clip_rect()));
                let head = Rect::from_min_max(pos2(left, top), pos2(left + width, top + HEAD));
                p.rect_filled(head, 0.0, t.surface);
                p.hline(head.x_range(), head.bottom() - 0.5, (1.0, t.line));
                for h in 0..hours {
                    let at = off.civil(from.saturating_add(h * 60 * MINUTE));
                    let x = left + HOUR_W * h as f32;
                    if x + 6.0 < side + NAMES {
                        continue;
                    }
                    words.text(pos2(x + 6.0, head.center().y), Align2::LEFT_CENTER, self.clock_name(at.hour, at.minute), font(11.0), t.text_dim);
                }
                let names = Rect::from_min_max(pos2(side, canvas.top()), pos2(side + NAMES, canvas.bottom()));
                p.rect_filled(names, 0.0, t.pane);
                p.vline(names.right() - 0.5, names.y_range(), (1.0, t.line));
                let corner = Rect::from_min_size(pos2(side, top), vec2(NAMES, HEAD));
                p.rect_filled(corner, 0.0, t.pane);
                p.hline(corner.x_range(), corner.bottom() - 0.5, (1.0, t.line));
                p.text(pos2(side + 12.0, corner.center().y), Align2::LEFT_CENTER, self.session.region.long_date(&date_of(day)), bold(11.5), t.text);
                p.text(pos2(side + 12.0, busy_top + BUSY_H / 2.0), Align2::LEFT_CENTER, "Anyone busy", font(11.0), t.text_dim);
                for (i, row) in rows.iter().enumerate() {
                    let r = Rect::from_min_size(pos2(side, rows_top + ROW_H * i as f32), vec2(NAMES, ROW_H));
                    p.hline(r.x_range(), r.bottom() - 0.5, (1.0, t.line));
                    let color = row.color.as_deref().and_then(hex_color).unwrap_or(t.accent);
                    p.rect_filled(Rect::from_min_size(pos2(r.left() + 10.0, r.center().y - 6.0), vec2(12.0, 12.0)), 2.0, color);
                    let file = self.session.stores.get(row.store).map(|st| st.display_name()).unwrap_or_default();
                    let (name, file) = (one_line(&row.name), one_line(&file));
                    let shown_here = is_shown(i);
                    clip_text(
                        &p,
                        pos2(r.left() + 30.0, r.center().y - 15.0),
                        NAMES - 38.0,
                        &name,
                        if shown_here { bold(12.0) } else { font(12.0) },
                        t.text,
                    );
                    clip_text(&p, pos2(r.left() + 30.0, r.center().y + 1.0), NAMES - 38.0, &file, font(10.5), t.text_dim);
                    if shown_here {
                        p.rect_filled(Rect::from_min_size(r.min, vec2(3.0, r.height())), 0.0, t.accent);
                    }
                    let resp = ui.interact(r, egui::Id::new(("cal-schedule-name", row.store, row.folder.0)), Sense::click());
                    let resp = resp.on_hover_text(if shown_here {
                        format!("{name} - {file}\nNew appointments go in this calendar")
                    } else {
                        format!("{name} - {file}\nClick to write in this calendar")
                    });
                    if resp.clicked() {
                        asked = Some(Asked::Show(i));
                    }
                }
                if scroll_to {
                    let x = x_of(from.saturating_add((i64::from(WORK.0) * 60 - 30) * MINUTE)) - NAMES;
                    ui.scroll_to_rect(Rect::from_min_size(pos2(x, canvas.top()), vec2(1.0, 1.0)), Some(egui::Align::Min));
                }
            });
        });

        let Some(asked) = asked else { return };
        // The calendar of row `i` made the one shown, at once: what is asked next is of it.
        let show = |ui: &mut MailUi, i: usize| -> bool {
            if is_shown(i) {
                return true;
            }
            let Some(row) = rows.get(i) else { return false };
            match ui.execute("calendar.open", &json!({ "store": row.store, "folder": row.folder.0 })) {
                Ok(_) => true,
                Err(e) => {
                    ui.session.status = e;
                    false
                }
            }
        };
        match asked {
            Asked::Select(i, id, occurrence) => {
                if show(self, i) {
                    self.act("calendar.select", select_params(id, occurrence));
                }
            }
            Asked::Open(i, id, occurrence) => {
                if show(self, i) {
                    self.open_appointment(id, occurrence);
                }
            }
            Asked::New(i, start) => {
                if show(self, i) {
                    self.appointment = Some(AppointmentForm::new_at(start, false, &off));
                }
            }
            Asked::Show(i) => {
                show(self, i);
            }
            Asked::Nothing(i) => {
                if is_shown(i) {
                    self.act("calendar.select", json!({ "id": null }));
                } else {
                    show(self, i);
                }
            }
            Asked::Task(d, start) => self.schedule_task(&d, &start),
        }
    }
}
