//! The Reminders window: every reminder due, for appointments, tasks and messages flagged for
//! follow-up, with Dismiss All, Open Item, Dismiss and Snooze, as Outlook's (`reminders.*`,
//! `openmailclient_engine::reminders`). It comes up of its own when one comes due, with the
//! sound and the taskbar button, as `reminders.options` says, and stands beside the mailbox,
//! which stays in use while it is open. One that comes due while the main window is minimized
//! or behind another comes up in a window of its own above every other, until the main window
//! is in front again; the sound is our own chime or a WAVE file of the owner's.
//!
//! Beyond Outlook: what is chosen shows where and when in full, an appointment can be snoozed
//! to so many minutes before it starts only while that is still to come, and the window says
//! when the next one comes once none is left. Subjects and locations may be a stranger's text
//! (a meeting asked for, a message flagged): drawn, never acted on.

use crate::icons::{self, Icon};
use crate::{MailUi, bold, font, one_line};
use egui::{Align2, Rect, Sense, Stroke, StrokeKind, vec2};
use openmailclient_engine::reminders::{Key, Pending};
use openmailclient_model::calendar::MINUTE;
use openmailclient_model::reminder::{Due, Kind};
use openmailclient_model::time;
use serde_json::json;

const ROW_H: f32 = 40.0;
/// Rows shown before the list scrolls.
const ROWS: f32 = 6.0;

/// A sound the host is to play for a reminder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sound {
    /// Our own chime.
    Chime,
    /// A WAVE file of the owner's; the chime if it cannot be played.
    File(String),
}

/// Whether the Reminders window stands in a window of its own: from when one comes due (`came`)
/// with the main window minimized or behind another, until the main window is in front again.
pub(crate) fn stands_apart(was: bool, came: bool, minimized: bool, focused: bool) -> bool {
    if focused && !minimized {
        return false;
    }
    was || came
}

/// When a snooze ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Minutes(i64),
    /// So many minutes before an appointment starts.
    Before(i64),
}

impl Default for Choice {
    fn default() -> Self {
        Choice::Minutes(5)
    }
}

const MINUTES: [(i64, &str); 13] = [
    (5, "5 minutes"),
    (10, "10 minutes"),
    (15, "15 minutes"),
    (30, "30 minutes"),
    (60, "1 hour"),
    (120, "2 hours"),
    (240, "4 hours"),
    (720, "0.5 days"),
    (1440, "1 day"),
    (2880, "2 days"),
    (4320, "3 days"),
    (5760, "4 days"),
    (10080, "1 week"),
];
const BEFORE: [(i64, &str); 4] =
    [(15, "15 minutes before start"), (10, "10 minutes before start"), (5, "5 minutes before start"), (0, "0 minutes before start")];

/// What the snooze can be set to for `p` at `now`: before its start only while that is to come.
fn choices(p: Option<&Pending>, now: i64) -> Vec<(Choice, &'static str)> {
    let mut all = Vec::new();
    if let Some(Pending { reminder: r, .. }) = p
        && r.kind == Kind::Appointment
        && let Due::At(start) = r.due
    {
        for (m, label) in BEFORE {
            if start.saturating_sub(m.saturating_mul(MINUTE)) > now {
                all.push((Choice::Before(m), label));
            }
        }
    }
    all.extend(MINUTES.iter().map(|(m, label)| (Choice::Minutes(*m), *label)));
    all
}

fn kind_icon(kind: Kind) -> Icon {
    match kind {
        Kind::Appointment => Icon::Calendar,
        Kind::Task => Icon::Tasks,
        Kind::Message => Icon::Flag,
    }
}

enum Act {
    Dismiss(Key),
    DismissAll,
    Open(Key),
    Snooze(Key, Choice),
}

impl MailUi {
    /// Note a reminder that came due since the last frame, and tell of it as the owner chose.
    pub(crate) fn new_reminders(&mut self, ctx: &egui::Context) {
        let fresh = self.session.reminders.fresh;
        if fresh == self.reminders_told {
            return;
        }
        self.reminders_told = fresh;
        if self.session.reminders.due.is_empty() {
            return;
        }
        let (minimized, focused) = ctx.input(|i| (i.viewport().minimized.unwrap_or(false), i.viewport().focused.unwrap_or(true)));
        if self.session.reminders.show {
            self.session.reminders.window = true;
            self.reminders_apart = stands_apart(self.reminders_apart, true, minimized, focused);
            // The one that came is chosen, so Dismiss and Snooze are about it.
            self.reminder_pick = self.session.reminders.due.last().map(Pending::key);
        }
        if self.session.reminders.sound {
            self.reminder_sounds = self.reminder_sounds.saturating_add(1);
        }
        if !focused {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
        }
    }

    /// The sound the host is to play for a reminder, if one is due; taking it clears it.
    pub fn take_reminder_sound(&mut self) -> Option<Sound> {
        (std::mem::take(&mut self.reminder_sounds) > 0).then(|| self.session.reminders.sound_file.clone().map_or(Sound::Chime, Sound::File))
    }

    /// The reminder sound played once, to hear it (Options).
    pub(crate) fn try_reminder_sound(&mut self) {
        self.reminder_sounds = self.reminder_sounds.saturating_add(1);
    }

    pub(crate) fn reminders_window(&mut self, ctx: &egui::Context) {
        let (minimized, focused) = ctx.input(|i| (i.viewport().minimized.unwrap_or(false), i.viewport().focused.unwrap_or(true)));
        self.reminders_apart = stands_apart(self.reminders_apart, false, minimized, focused);
        if !self.session.reminders.window {
            return;
        }
        let t = self.tokens.clone();
        let now = self.session.time_now();
        let today = self.session.today();
        let due = self.session.reminders.due.clone();
        let next = self.session.reminders.next;
        let pick = self.reminder_pick.filter(|k| due.iter().any(|p| p.key() == *k)).or_else(|| due.first().map(Pending::key));
        let chosen = pick.and_then(|k| due.iter().find(|p| p.key() == k));
        let offered = choices(chosen, now);
        let snooze = offered.iter().find(|(c, _)| *c == self.snooze_pick).map_or(Choice::default(), |(c, _)| *c);
        let title = match due.len() {
            0 => "Reminders".to_string(),
            1 => "1 Reminder".to_string(),
            n => format!("{n} Reminders"),
        };
        let mut act = None;
        let mut picked = pick;
        let mut snooze_to = snooze;
        let mut close = false;
        let dialog =
            crate::dialog::Dialog::new("reminders", &title, Icon::Bell).width(440.0).anchor(Align2::RIGHT_BOTTOM, vec2(-24.0, -(t.status_h + 24.0)));
        let body = |ui: &mut egui::Ui| {
            // What is chosen, in full.
            match chosen {
                Some(p) => {
                    let r = &p.reminder;
                    let subject = if r.subject.trim().is_empty() { "(No subject)".to_string() } else { one_line(&r.subject) };
                    ui.add(egui::Label::new(crate::seen_line(&subject, bold(14.0), t.text)).truncate());
                    let when = self.reminder_when(p);
                    if !when.is_empty() {
                        ui.label(egui::RichText::new(when).font(font(12.5)).color(t.text_dim));
                    }
                    if !r.location.trim().is_empty() {
                        ui.add(
                            egui::Label::new(crate::seen_line(&format!("Location: {}", one_line(&r.location)), font(12.5), t.text_dim)).truncate(),
                        );
                    }
                }
                None => {
                    ui.label(egui::RichText::new("No reminders are due.").font(font(13.0)).color(t.text));
                    if let Some(at) = next {
                        let c = self.session.zone.civil(at);
                        let when = format!("The next comes {} {}.", self.session.region.date(&c), self.session.region.time(&c));
                        ui.label(egui::RichText::new(when).font(font(12.5)).color(t.text_dim));
                    }
                }
            }
            ui.add_space(8.0);
            // The list: what each is of, its subject, and how long until it is due.
            let width = ui.available_width();
            let (head, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(head, 0.0, t.pane);
            p.text(head.left_center() + vec2(32.0, 0.0), Align2::LEFT_CENTER, "Subject", bold(12.0), t.text_dim);
            p.text(head.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, "Due in", bold(12.0), t.text_dim);
            egui::ScrollArea::vertical().max_height(ROW_H * ROWS).auto_shrink([false, true]).show(ui, |ui| {
                if due.is_empty() {
                    ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
                }
                for p in &due {
                    let key = p.key();
                    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
                    let painter = ui.painter_at(rect);
                    if picked == Some(key) {
                        painter.rect_filled(rect, 3.0, t.selected);
                        painter.rect_stroke(rect, 3.0, Stroke::new(1.0, t.accent), StrokeKind::Inside);
                    } else if resp.hovered() {
                        painter.rect_filled(rect, 3.0, t.hover);
                    }
                    let r = &p.reminder;
                    icons::paint(
                        &painter,
                        Rect::from_center_size(rect.left_center() + vec2(16.0, 0.0), vec2(16.0, 16.0)),
                        kind_icon(r.kind),
                        t.accent,
                    );
                    let due_in = openmailclient_model::reminder::due_in(r.due, now, today);
                    let late = due_in.ends_with("overdue");
                    let due_w = 120.0;
                    painter.text(
                        rect.right_center() - vec2(10.0, 0.0),
                        Align2::RIGHT_CENTER,
                        &due_in,
                        font(12.5),
                        if late { t.danger } else { t.text },
                    );
                    let subject = if r.subject.trim().is_empty() { "(No subject)".to_string() } else { one_line(&r.subject) };
                    let room = Rect::from_min_max(rect.min + vec2(32.0, 3.0), rect.max - vec2(due_w, 3.0));
                    let mut job = crate::seen_line(&subject, font(13.0), t.text);
                    job.wrap = egui::text::TextWrapping::truncate_at_width(room.width());
                    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
                    painter.galley(room.left_center() - vec2(0.0, galley.size().y / 2.0), galley, t.text);
                    if resp.clicked() {
                        picked = Some(key);
                    }
                    if resp.double_clicked() {
                        act = Some(Act::Open(key));
                    }
                }
            });
            ui.add_space(8.0);
            let any = !due.is_empty();
            ui.horizontal(|ui| {
                if ui.add_enabled(any, crate::dialog::plain("Dismiss All")).clicked() {
                    act = Some(Act::DismissAll);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(k) = picked {
                        if ui.add(crate::dialog::plain("Dismiss")).clicked() {
                            act = Some(Act::Dismiss(k));
                        }
                        if ui.add(crate::dialog::plain("Open Item")).clicked() {
                            act = Some(Act::Open(k));
                        }
                    }
                });
            });
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Click Snooze to be reminded again in:").font(font(12.5)).color(t.text_dim));
            crate::dialog::foot(ui, &t, |ui| {
                let label = offered.iter().find(|(c, _)| *c == snooze_to).map_or("5 minutes", |(_, l)| *l);
                egui::ComboBox::from_id_salt("reminders-snooze").width(220.0).selected_text(label).show_ui(ui, |ui| {
                    for (c, l) in &offered {
                        ui.selectable_value(&mut snooze_to, *c, *l);
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if any {
                        if ui.add_enabled(picked.is_some(), crate::dialog::primary(&t, "Snooze")).clicked()
                            && let Some(k) = picked
                        {
                            act = Some(Act::Snooze(k, snooze_to));
                        }
                    } else if ui.add(crate::dialog::primary(&t, "Close")).clicked() {
                        close = true;
                    }
                });
            });
        };
        let shown = if self.reminders_apart && !ctx.embed_viewports() {
            // Above every other window, in one of its own, while the main window is not in front.
            let builder = egui::ViewportBuilder::default()
                .with_title(title.clone())
                .with_inner_size(vec2(476.0, 476.0))
                .with_resizable(false)
                .with_window_level(egui::WindowLevel::AlwaysOnTop);
            let mut once = Some((dialog, body));
            ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("openmailclient-reminders"), builder, |ui, _| {
                let asked = ui.input(|i| i.viewport().close_requested());
                let shown = once.take().map(|(d, b)| d.fill(ui, &t, b));
                crate::dialog::Shown { closed: asked || shown.is_some_and(|s| s.closed) }
            })
        } else {
            dialog.show(ctx, &t, body)
        };
        self.reminder_pick = picked;
        self.snooze_pick = snooze_to;
        if shown.closed || close {
            self.session.reminders.window = false;
        }
        if let Some(act) = act {
            let at = |k: Key| json!({ "store": k.0, "folder": k.1.0, "id": k.2 });
            let result = match act {
                Act::Dismiss(k) => self.session.run("reminders.dismiss", &at(k)),
                Act::DismissAll => self.session.run("reminders.dismissAll", &json!({})),
                Act::Open(k) => self.session.run("reminders.open", &at(k)),
                Act::Snooze(k, c) => {
                    let mut p = at(k);
                    match c {
                        Choice::Minutes(m) => p["minutes"] = json!(m),
                        Choice::Before(m) => p["beforeStart"] = json!(m),
                    }
                    self.session.run("reminders.snooze", &p)
                }
            };
            match result {
                // With none left the window goes, as Outlook's does.
                Ok(_) if self.session.reminders.due.is_empty() => self.session.reminders.window = false,
                Ok(_) => {}
                Err(e) => self.session.status = e.to_string(),
            }
        }
        // "In 5 minutes" counts down while it is open.
        ctx.request_repaint_after(std::time::Duration::from_secs(15));
    }

    /// When the chosen one is: an appointment's start, a task's or message's day.
    fn reminder_when(&self, p: &Pending) -> String {
        let region = &self.session.region;
        match p.reminder.due {
            Due::At(at) => {
                let c = self.session.zone.civil(at);
                let lead = if p.reminder.kind == Kind::Appointment { "Start time" } else { "Due" };
                format!("{lead}: {} {}", region.long_date(&c), region.time(&c))
            }
            Due::Day(d) => {
                let c = time::civil(time::from_unix(d.saturating_mul(86_400)), 0);
                format!("Due: {}", region.long_date(&c))
            }
            Due::Unknown => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn before_its_start_is_offered_only_while_it_is_to_come() {
        use openmailclient_engine::reminders::Pending;
        use openmailclient_model::FolderId;
        use openmailclient_model::reminder::Reminder;
        let start = time::from_civil(2030, 10, 10, 8, 0, 0, 0);
        let p = Pending {
            store: 0,
            folder: FolderId(1),
            id: 1,
            reminder: Reminder {
                kind: Kind::Appointment,
                subject: String::new(),
                location: String::new(),
                signal: start - 15 * MINUTE,
                due: Due::At(start),
                occurrence: None,
            },
        };
        let at = |now| choices(Some(&p), now).iter().filter(|(c, _)| matches!(c, Choice::Before(_))).count();
        assert_eq!(at(start - 15 * MINUTE), 3, "15 minutes before is now, so not offered");
        assert_eq!(at(start - 7 * MINUTE), 2);
        assert_eq!(at(start), 0);
        assert_eq!(choices(None, start).len(), MINUTES.len());
    }
}
