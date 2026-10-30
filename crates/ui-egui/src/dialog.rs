//! The look every dialog shares: a head in the theme's dialog colour with the dialog's own mark,
//! its name and a close button; a body on the surface colour; and a foot on the pane colour that
//! holds the buttons, the one that does the work filled with the accent. While a modal one is
//! open the window behind it is dimmed and takes no clicks.

use crate::icons::{self, Icon};
use crate::theme::{Tokens, mix};
use crate::{bold, font};
use egui::{Align2, Color32, CornerRadius, Frame, Margin, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

/// Room round a dialog's body.
pub(crate) const PAD: i8 = 18;
const HEAD_H: f32 = 46.0;
const RADIUS: u8 = 10;

pub(crate) struct Dialog<'a> {
    id: egui::Id,
    title: &'a str,
    icon: Icon,
    anchor: (Align2, Vec2),
    width: Option<f32>,
}

/// What a dialog gave back this frame: whether its close button was pressed.
pub(crate) struct Shown {
    pub closed: bool,
}

impl<'a> Dialog<'a> {
    pub(crate) fn new(id: &str, title: &'a str, icon: Icon) -> Self {
        Self { id: egui::Id::new(("openmailclient-dialog", id)), title, icon, anchor: (Align2::CENTER_CENTER, vec2(0.0, -20.0)), width: None }
    }

    pub(crate) fn anchor(mut self, align: Align2, offset: Vec2) -> Self {
        self.anchor = (align, offset);
        self
    }

    /// The width of the body, inside its room.
    pub(crate) fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub(crate) fn show(self, ctx: &egui::Context, t: &Tokens, add: impl FnOnce(&mut Ui)) -> Shown {
        let shadow = egui::epaint::Shadow { offset: [0, 10], blur: 32, spread: 0, color: Color32::from_black_alpha(if t.dark { 140 } else { 60 }) };
        let frame = Frame::new().fill(t.surface).stroke(Stroke::new(1.0, mix(t.line, t.dialog_head, 0.25))).corner_radius(RADIUS).shadow(shadow);
        let mut closed = false;
        let (title, icon, id, width) = (self.title, self.icon, self.id, self.width);
        egui::Window::new(title).id(id).title_bar(false).frame(frame).collapsible(false).resizable(false).anchor(self.anchor.0, self.anchor.1).show(
            ctx,
            |ui| {
                ui.spacing_mut().item_spacing.y = ui.spacing().item_spacing.y.max(4.0);
                let top = ui.cursor().min;
                let _ = ui.allocate_exact_size(vec2(1.0, HEAD_H), Sense::hover());
                Frame::new().inner_margin(Margin { left: PAD, right: PAD, top: PAD - 4, bottom: PAD }).show(ui, |ui| {
                    if let Some(w) = width {
                        ui.set_width(w);
                    }
                    add(ui);
                });
                // The head is drawn last, across the width the body came to.
                let head = Rect::from_min_size(top, vec2(ui.min_rect().width().max(120.0), HEAD_H));
                closed = draw_head(ui, t, head, title, icon, id);
            },
        );
        Shown { closed }
    }
}

impl Dialog<'_> {
    /// The dialog filling a window of its own, as one standing apart from the main window.
    pub(crate) fn fill(self, ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui)) -> Shown {
        let (title, icon, id, width) = (self.title, self.icon, self.id, self.width);
        ui.painter().rect_filled(ui.max_rect(), 0.0, t.surface);
        ui.spacing_mut().item_spacing.y = ui.spacing().item_spacing.y.max(4.0);
        let top = ui.max_rect().min;
        let _ = ui.allocate_exact_size(vec2(1.0, HEAD_H), Sense::hover());
        Frame::new().inner_margin(Margin { left: PAD, right: PAD, top: PAD - 4, bottom: PAD }).show(ui, |ui| {
            if let Some(w) = width {
                ui.set_width(w);
            }
            add(ui);
        });
        let head = Rect::from_min_size(top, vec2(ui.max_rect().width(), HEAD_H));
        Shown { closed: draw_head(ui, t, head, title, icon, id) }
    }
}

/// The head: the mark in a ring, the name, and a close button that lights under the pointer.
fn draw_head(ui: &mut Ui, t: &Tokens, head: Rect, title: &str, icon: Icon, id: egui::Id) -> bool {
    let p = ui.painter();
    let r = f32::from(RADIUS);
    let round = CornerRadius { nw: RADIUS, ne: RADIUS, sw: 0, se: 0 };
    p.rect_filled(head, round, t.dialog_head);
    // A lighter band along the top, and a line of the accent under it.
    let sheen = Rect::from_min_size(head.min, vec2(head.width(), head.height() * 0.5));
    p.rect_filled(sheen, CornerRadius { nw: RADIUS, ne: RADIUS, sw: 0, se: 0 }, Color32::from_white_alpha(if t.dark { 6 } else { 14 }));
    p.hline(head.x_range(), head.bottom() - 1.0, Stroke::new(2.0, mix(t.dialog_head, t.accent, if t.dark { 0.7 } else { 0.0 }).gamma_multiply(0.9)));
    let ring = egui::pos2(head.left() + r + 12.0, head.center().y);
    p.circle_filled(ring, 15.0, t.dialog_text.gamma_multiply(0.16));
    icons::paint(p, Rect::from_center_size(ring, vec2(18.0, 18.0)), icon, t.dialog_text);
    let words = crate::seen_line(title, bold(15.0), t.dialog_text);
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(words));
    let room = (head.width() - 110.0).max(20.0);
    let text_at = pos2(ring.x + 26.0, head.center().y - galley.size().y / 2.0);
    ui.painter().with_clip_rect(Rect::from_min_size(text_at, vec2(room, HEAD_H))).galley(text_at, galley, t.dialog_text);
    let close = Rect::from_center_size(pos2(head.right() - 26.0, head.center().y), vec2(30.0, 30.0));
    let resp = ui.interact(close, id.with("close"), Sense::click()).on_hover_text("Close (Esc)");
    if resp.hovered() {
        ui.painter().rect_filled(close, 6.0, t.dialog_text.gamma_multiply(0.18));
    }
    icons::paint(ui.painter(), close.shrink(8.0), Icon::Close, t.dialog_text);
    resp.clicked()
}

/// The foot of a dialog: a band on the pane colour across its whole width, with what `add`
/// puts in it in one line. Buttons go at the right with `right_to_left` inside.
pub(crate) fn foot<R>(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(10.0);
    Frame::new()
        .fill(t.pane)
        .outer_margin(Margin { left: -PAD, right: -PAD, top: 0, bottom: -PAD })
        .inner_margin(Margin::symmetric(PAD, 12))
        .corner_radius(CornerRadius { nw: 0, ne: 0, sw: RADIUS, se: RADIUS })
        .show(ui, |ui| {
            let edge = ui.max_rect();
            let span = egui::Rangef::new(edge.left() - f32::from(PAD), edge.right() + f32::from(PAD));
            ui.painter().hline(span, edge.top() - 12.0, Stroke::new(1.0, t.line));
            ui.set_min_width(ui.available_width());
            ui.horizontal(add).inner
        })
        .inner
}

/// The button that does what the dialog is for, filled with the accent.
pub(crate) fn primary<'a>(t: &Tokens, label: &'a str) -> egui::Button<'a> {
    egui::Button::new(egui::RichText::new(label).color(t.on_accent).font(bold(13.0)))
        .fill(t.accent)
        .stroke(Stroke::new(1.0, t.accent_dark))
        .min_size(vec2(92.0, 30.0))
}

/// A button that throws something away, filled with the warning colour.
pub(crate) fn danger<'a>(t: &Tokens, label: &'a str) -> egui::Button<'a> {
    let on = if crate::theme::luminance(t.danger) > 0.4 { Color32::BLACK } else { Color32::WHITE };
    egui::Button::new(egui::RichText::new(label).color(on).font(bold(13.0))).fill(t.danger).min_size(vec2(84.0, 30.0))
}

/// Any other button in a dialog, as tall as the one beside it.
pub(crate) fn plain(label: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(label).font(font(13.0))).min_size(vec2(80.0, 30.0))
}

/// A heading inside a dialog: the accent's colour, with a short bar before it.
pub(crate) fn heading(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.horizontal(|ui| {
        let (bar, _) = ui.allocate_exact_size(vec2(4.0, 16.0), Sense::hover());
        ui.painter().rect_filled(bar, 2.0, t.accent);
        ui.label(egui::RichText::new(text).font(bold(13.0)).color(if t.dark { t.accent } else { t.accent_dark }));
    });
}

/// A box of words that stand out from the rest: tinted with `tone`, with a bar of it at the
/// start, as a warning or a word of note.
pub(crate) fn callout(ui: &mut Ui, t: &Tokens, tone: Color32, add: impl FnOnce(&mut Ui)) {
    let resp = Frame::new()
        .fill(mix(t.surface, tone, if t.dark { 0.18 } else { 0.09 }))
        .stroke(Stroke::new(1.0, mix(t.surface, tone, 0.45)))
        .corner_radius(6)
        .inner_margin(Margin { left: 14, right: 10, top: 8, bottom: 8 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
    let r = resp.response.rect;
    let bar = Rect::from_min_max(r.min + vec2(1.0, 1.0), pos2(r.left() + 5.0, r.bottom() - 1.0));
    ui.painter().rect_filled(bar, CornerRadius { nw: 5, sw: 5, ne: 0, se: 0 }, tone);
}

/// A box on the pane colour for a thing the dialog is about: a file, an address.
pub(crate) fn card<R>(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(t.pane)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(6)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Dim what is behind a modal dialog, and take the clicks meant for it.
pub(crate) fn scrim(ui: &mut Ui, t: &Tokens, rect: Rect) {
    ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(if t.dark { 120 } else { 70 }));
    let _ = ui.interact(rect, egui::Id::new("openmailclient-scrim"), Sense::click_and_drag());
}
