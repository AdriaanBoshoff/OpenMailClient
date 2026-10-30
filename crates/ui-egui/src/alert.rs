//! New mail told of: an alert at the corner of the screen, the taskbar button, and a count the
//! host plays its sound for, as `mail.newMailAlerts` says (`openmailclient_engine::arrival`).
//!
//! The alert stands on its own above other windows and never takes the keyboard, so typing
//! elsewhere goes on; a click on it opens the message, × or a few seconds put it away, and the
//! pointer over it holds it. What it shows (sender, subject, start of the text) is a stranger's
//! text, drawn and never acted on.

use crate::icons::{self, Icon};
use crate::{MailUi, bold, clip_text, font};
use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use serde_json::json;

/// How long an alert stands, in seconds, while the pointer is not over it.
const STANDS_S: f32 = 8.0;
const SIZE: Vec2 = Vec2::new(372.0, 104.0);
/// From the screen's edges; more from the bottom, where the taskbar usually is.
const MARGIN: Vec2 = Vec2::new(16.0, 60.0);

/// The alert being shown.
#[derive(Clone, Debug)]
pub(crate) struct Shown {
    pub seq: u64,
    pub account: String,
    pub from: String,
    pub subject: String,
    pub preview: String,
    /// Others that came with it or since, not shown.
    pub more: usize,
    /// Seconds left while the pointer is not over it.
    pub left: f32,
}

enum Act {
    Open,
    Close,
}

impl MailUi {
    /// Note what came since the last frame, and tell of it as the owner chose.
    pub(crate) fn new_mail(&mut self, ctx: &egui::Context) {
        // A rule asked for the sound, whatever the new-mail options say.
        let rang = self.session.arrived.sounds > self.sounds_heard;
        self.sounds_heard = self.session.arrived.sounds;
        let last = self.session.arrived.last;
        if last <= self.told_seq {
            if rang {
                self.chimes = self.chimes.saturating_add(1);
            }
            return;
        }
        let told = self.told_seq;
        self.told_seq = last;
        let fresh: Vec<_> = self.session.arrived.list.iter().filter(|a| a.seq > told).collect();
        let Some(newest) = fresh.last() else { return };
        let alerts = self.session.alerts;
        if alerts.sound || rang {
            self.chimes = self.chimes.saturating_add(1);
        }
        if alerts.flash && !ctx.input(|i| i.viewport().focused.unwrap_or(true)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
        }
        if alerts.alert {
            let before = self.alert.as_ref().map_or(0, |a| a.more.saturating_add(1));
            self.alert = Some(Shown {
                seq: newest.seq,
                account: newest.account.clone(),
                from: newest.from.clone(),
                subject: newest.subject.clone(),
                preview: newest.preview.clone(),
                more: fresh.len().saturating_sub(1).saturating_add(before),
                left: STANDS_S,
            });
        }
    }

    /// The host plays its sound this many times (at most once at a time); taking them clears them.
    pub fn take_chimes(&mut self) -> u32 {
        std::mem::take(&mut self.chimes)
    }

    pub(crate) fn alert_window(&mut self, ctx: &egui::Context) {
        let Some(shown) = self.alert.clone() else { return };
        // Opened or dismissed elsewhere meanwhile: nothing more to tell of.
        if shown.more == 0 && !self.session.arrived.list.iter().any(|a| a.seq == shown.seq) {
            self.alert = None;
            return;
        }
        let act = if ctx.embed_viewports() {
            // Where windows cannot stand on their own (tests, headless pictures), it is drawn
            // at the corner of the main window.
            let at = ctx.content_rect().max - SIZE - vec2(16.0, self.tokens.status_h + 16.0);
            egui::Area::new(egui::Id::new("openmailclient-new-mail"))
                .order(egui::Order::Foreground)
                .fixed_pos(at)
                .show(ctx, |ui| {
                    let (rect, _) = ui.allocate_exact_size(SIZE, Sense::hover());
                    self.alert_body(ui, rect)
                })
                .inner
        } else {
            let builder = egui::ViewportBuilder::default()
                .with_title("New mail")
                .with_inner_size(SIZE)
                .with_resizable(false)
                .with_decorations(false)
                .with_window_level(egui::WindowLevel::AlwaysOnTop)
                .with_taskbar(false)
                .with_active(false)
                .with_position(corner(ctx));
            ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("openmailclient-new-mail"), builder, |ui, _| {
                if ui.input(|i| i.viewport().close_requested()) {
                    return Some(Act::Close);
                }
                let rect = ui.ctx().content_rect();
                self.alert_body(ui, rect)
            })
        };
        match act {
            Some(Act::Open) => {
                self.alert = None;
                if let Err(e) = self.session.run("mail.openArrived", &json!({ "seq": shown.seq })) {
                    self.session.status = e.to_string();
                }
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
            }
            Some(Act::Close) => {
                self.alert = None;
                let _ = self.session.run("mail.dismissArrived", &json!({ "seq": shown.seq }));
            }
            None => {
                if self.alert.as_ref().is_some_and(|a| a.left <= 0.0) {
                    // Gone from the corner, still new: the status bar goes on telling of it.
                    self.alert = None;
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
    }

    fn alert_body(&mut self, ui: &mut Ui, rect: Rect) -> Option<Act> {
        let t = self.tokens.clone();
        let shown = self.alert.as_mut()?;
        let p = ui.painter_at(rect);
        let whole = ui.interact(rect, egui::Id::new("new-mail-whole"), Sense::click());
        let close = Rect::from_min_size(pos2(rect.right() - 30.0, rect.top() + 6.0), vec2(24.0, 24.0));
        let x_resp = ui.interact(close, egui::Id::new("new-mail-close"), Sense::click());
        if !whole.hovered() && !x_resp.hovered() {
            shown.left -= ui.input(|i| i.stable_dt).clamp(0.0, 0.5);
        }
        let fill = if whole.hovered() { t.hover } else { t.surface };
        p.rect_filled(rect, CornerRadius::same(8), fill);
        p.rect_stroke(rect.shrink(0.5), CornerRadius::same(8), Stroke::new(1.0, t.line), StrokeKind::Inside);
        // A band of the accent at the left says where it is from at a glance.
        p.rect_filled(
            Rect::from_min_size(rect.min + vec2(1.0, 1.0), vec2(4.0, rect.height() - 2.0)),
            CornerRadius { nw: 7, sw: 7, ne: 0, se: 0 },
            t.accent,
        );
        icons::paint(&p, Rect::from_min_size(rect.min + vec2(16.0, 16.0), vec2(24.0, 24.0)), Icon::Mail, t.accent);
        if x_resp.hovered() {
            p.rect_filled(close, 4.0, t.selected);
        }
        let c = close.center();
        for (a, b) in [(vec2(-5.0, -5.0), vec2(5.0, 5.0)), (vec2(-5.0, 5.0), vec2(5.0, -5.0))] {
            p.line_segment([c + a, c + b], Stroke::new(1.4, t.text_dim));
        }

        let left = rect.left() + 52.0;
        let width = rect.right() - 36.0 - left;
        let from = if shown.from.is_empty() { "(unknown sender)" } else { shown.from.as_str() };
        clip_text(&p, pos2(left, rect.top() + 12.0), width, from, bold(13.5), t.text);
        let subject = if shown.subject.is_empty() { "(no subject)" } else { shown.subject.as_str() };
        clip_text(&p, pos2(left, rect.top() + 32.0), rect.right() - 12.0 - left, subject, font(12.5), t.text);
        clip_text(&p, pos2(left, rect.top() + 51.0), rect.right() - 12.0 - left, &shown.preview, font(12.0), t.text_dim);
        let foot = match shown.more {
            0 => shown.account.clone(),
            1 => format!("{} · and 1 more new message", shown.account),
            n => format!("{} · and {n} more new messages", shown.account),
        };
        clip_text(&p, pos2(left, rect.bottom() - 24.0), rect.right() - 12.0 - left, &foot, font(11.5), t.text_faint);
        // How long it stands, as a thin line that shortens; held while the pointer is over it.
        let share = (shown.left / STANDS_S).clamp(0.0, 1.0);
        let track = Rect::from_min_size(pos2(rect.left() + 8.0, rect.bottom() - 4.0), vec2((rect.width() - 16.0) * share, 2.0));
        p.rect_filled(track, 1.0, t.accent.gamma_multiply(0.5));

        if x_resp.clicked() {
            Some(Act::Close)
        } else if whole.clicked() {
            Some(Act::Open)
        } else {
            whole.on_hover_cursor(egui::CursorIcon::PointingHand);
            None
        }
    }
}

/// The bottom right of the screen the window is on, as far as this system says.
fn corner(ctx: &egui::Context) -> Pos2 {
    let (monitor, outer) = ctx.input(|i| (i.viewport().monitor_size, i.viewport().outer_rect));
    match (monitor, outer) {
        // A window on a second screen to the right is past the first: the corner is taken
        // from the screen it is on, counted in screens of this size.
        (Some(m), Some(o)) if m.x > 0.0 && m.y > 0.0 => {
            let across = (o.center().x / m.x).floor().max(0.0);
            pos2(across * m.x + m.x - SIZE.x - MARGIN.x, m.y - SIZE.y - MARGIN.y)
        }
        (Some(m), None) => (m - SIZE - MARGIN).to_pos2(),
        (_, Some(o)) => o.max - SIZE - vec2(MARGIN.x, MARGIN.x),
        _ => pos2(100.0, 100.0),
    }
}
