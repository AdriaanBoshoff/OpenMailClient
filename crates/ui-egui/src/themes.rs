//! Themes: the built-in ones and those a person makes, each with its own colours for the light
//! set and the dark set. They are chosen and changed while the window shows the change, from the
//! Themes window or by command (`ui.themes`, `ui.useTheme`, `ui.themeColor`, `ui.newTheme`,
//! `ui.renameTheme`, `ui.deleteTheme`), and kept in the settings with the rest of the window.
//!
//! Better than Outlook, which offers four fixed Office themes: any colour of either set can be
//! changed, one accent can bring the colours that follow it along, every pair of text and its
//! ground is checked for contrast as it changes, and the window may follow the system's light or
//! dark setting.

use crate::dialog;
use crate::icons::{self, Icon};
use crate::line_edit::LineEdit;
use crate::theme::{self, COLORS, MAX_THEMES, Theme, Tokens};
use crate::{MailUi, bold, font, one_line};
use egui::{Color32, Rect, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use std::collections::HashMap;

/// The Themes window: the colour codes being typed. The set it changes is always the one the
/// window shows, so what is changed is seen at once.
#[derive(Default)]
pub(crate) struct ThemesWindow {
    /// The name being typed for the theme, while its field has the keyboard.
    pub name: String,
    pub hex: HashMap<String, String>,
    /// Delete was pressed once; a second press deletes.
    pub deleting: bool,
}

/// Accents to start from, our own choices.
const ACCENTS: [(u8, u8, u8); 10] = [
    (0x12, 0x62, 0xB3),
    (0x0B, 0x7A, 0x75),
    (0x2F, 0x6E, 0x3A),
    (0x6B, 0x3F, 0x8F),
    (0xA8, 0x2B, 0x5A),
    (0xB2, 0x3A, 0x1E),
    (0x9A, 0x5B, 0x00),
    (0x4A, 0x55, 0x60),
    (0x1F, 0x4E, 0x79),
    (0x00, 0x7A, 0xA8),
];

impl MailUi {
    /// The built-in themes and then the person's own.
    pub(crate) fn all_themes(&self) -> Vec<Theme> {
        let mut all = theme::built_in();
        all.extend(self.themes.iter().cloned());
        all
    }

    fn find_theme(&self, name: &str) -> Option<Theme> {
        self.all_themes().into_iter().find(|t| t.name.eq_ignore_ascii_case(name.trim()))
    }

    pub(crate) fn current_theme(&self) -> Theme {
        self.find_theme(&self.theme_name).unwrap_or_else(|| theme::built_in().into_iter().next().unwrap_or_default())
    }

    /// Draw with the chosen theme, light or dark.
    pub(crate) fn set_dark(&mut self, dark: bool) {
        self.tokens = self.current_theme().tokens(dark);
    }

    /// A name no theme has yet, from `base`.
    fn free_name(&self, base: &str) -> String {
        let base = theme::clean_name(base).unwrap_or_else(|| "My theme".to_string());
        let taken = |n: &str| self.all_themes().iter().any(|t| t.name.eq_ignore_ascii_case(n));
        if !taken(&base) {
            return base;
        }
        (2..=MAX_THEMES + 2)
            .map(|n| {
                let tail = format!(" {n}");
                let head: String = base.chars().take(theme::MAX_NAME.saturating_sub(tail.len())).collect();
                format!("{}{tail}", head.trim_end())
            })
            .find(|n| !taken(n))
            .unwrap_or(base)
    }

    /// The person's theme being changed: the chosen one when it is theirs, else a copy of it made
    /// theirs and chosen.
    fn own_index(&mut self) -> Result<usize, String> {
        if let Some(at) = self.themes.iter().position(|t| t.name.eq_ignore_ascii_case(&self.theme_name)) {
            return Ok(at);
        }
        if self.themes.len() >= MAX_THEMES {
            return Err(format!("At most {MAX_THEMES} themes are kept; delete one first"));
        }
        let from = self.current_theme();
        let name = self.free_name(&format!("{} (mine)", from.name));
        self.themes.push(Theme { name: name.clone(), built_in: false, ..from });
        self.theme_name = name;
        Ok(self.themes.len() - 1)
    }

    fn themes_list(&self) -> Value {
        json!({
            "current": self.theme_name,
            "mode": if self.follow_system { "system" } else if self.tokens.dark { "dark" } else { "light" },
            "themes": self.all_themes().iter().map(|t| json!({ "name": t.name, "builtIn": t.built_in })).collect::<Vec<_>>(),
            "unreadable": self.tokens.unreadable().iter().map(|(what, ratio)| json!({ "pair": what, "contrast": (ratio * 10.0).round() / 10.0 })).collect::<Vec<_>>(),
        })
    }

    pub(crate) fn theme_command(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        let text = |key: &str| params.get(key).and_then(Value::as_str);
        let dark = self.tokens.dark;
        match id {
            "ui.themes" => self.options_page_command(crate::options::Page::Themes, params)?,
            "ui.useTheme" => {
                let name = text("name").ok_or("\"name\" is missing")?;
                let found = self.find_theme(name).ok_or_else(|| format!("There is no theme called {}", one_line(name)))?;
                self.theme_name = found.name;
                self.set_dark(dark);
            }
            "ui.newTheme" => {
                if self.themes.len() >= MAX_THEMES {
                    return Err(format!("At most {MAX_THEMES} themes are kept; delete one first"));
                }
                let from = match text("from") {
                    Some(from) => self.find_theme(from).ok_or_else(|| format!("There is no theme called {}", one_line(from)))?,
                    None => self.current_theme(),
                };
                let name = match text("name") {
                    Some(name) => {
                        let name = theme::clean_name(name).ok_or("A theme needs a name")?;
                        if self.find_theme(&name).is_some() {
                            return Err(format!("There is a theme called {name} already"));
                        }
                        name
                    }
                    None => self.free_name("My theme"),
                };
                self.themes.push(Theme { name: name.clone(), built_in: false, ..from });
                self.theme_name = name;
                self.set_dark(dark);
            }
            "ui.renameTheme" => {
                let to = theme::clean_name(text("to").ok_or("\"to\" is missing")?).ok_or("A theme needs a name")?;
                let name = text("name").map_or_else(|| self.theme_name.clone(), str::to_string);
                let at = self
                    .themes
                    .iter()
                    .position(|t| t.name.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| format!("{} is not one of your themes; only those can be renamed", one_line(&name)))?;
                if self.find_theme(&to).is_some_and(|t| !t.name.eq_ignore_ascii_case(&name)) {
                    return Err(format!("There is a theme called {to} already"));
                }
                let was_current = self.theme_name.eq_ignore_ascii_case(&name);
                if let Some(t) = self.themes.get_mut(at) {
                    t.name = to.clone();
                }
                if was_current {
                    self.theme_name = to;
                }
            }
            "ui.deleteTheme" => {
                let name = text("name").map_or_else(|| self.theme_name.clone(), str::to_string);
                let at = self
                    .themes
                    .iter()
                    .position(|t| t.name.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| format!("{} is not one of your themes; only those can be deleted", one_line(&name)))?;
                self.themes.remove(at);
                if self.theme_name.eq_ignore_ascii_case(&name) {
                    self.theme_name = "Blue".to_string();
                }
                self.set_dark(dark);
            }
            "ui.themeColor" => {
                let mode = match text("mode") {
                    None => dark,
                    Some("light") => false,
                    Some("dark") => true,
                    Some(_) => return Err("\"mode\" must be light or dark".to_string()),
                };
                let key = text("color").ok_or("\"color\" is missing")?;
                if !COLORS.iter().any(|(k, ..)| *k == key) {
                    return Err(format!("{} is not a colour a theme has", one_line(key)));
                }
                let value = match params.get("value") {
                    Some(Value::Null) => None,
                    Some(Value::String(s)) => Some(theme::parse_hex(s).ok_or("\"value\" must be a colour as #RRGGBB")?),
                    _ => return Err("\"value\" must be a colour as #RRGGBB, or null for the theme's own".to_string()),
                };
                let follow = params.get("follow").and_then(Value::as_bool) == Some(true);
                if follow && key != "accent" {
                    return Err("only the accent brings other colours along".to_string());
                }
                let at = self.own_index()?;
                if let Some(own) = self.themes.get_mut(at) {
                    match value {
                        Some(c) if follow => own.set_accent(mode, c),
                        Some(c) => {
                            own.changes_mut(mode).insert(key.to_string(), c);
                        }
                        None => {
                            own.changes_mut(mode).remove(key);
                        }
                    }
                }
                self.set_dark(dark);
            }
            _ => return Err(format!("unknown command {}", one_line(id))),
        }
        Ok(self.themes_list())
    }

    /// The person's themes and the chosen one, as a settings file has them.
    pub(crate) fn themes_settings(&self) -> (Value, Value) {
        (json!(self.theme_name), Value::Array(self.themes.iter().map(Theme::to_json).collect()))
    }

    /// Take the themes a settings file keeps: at most `MAX_THEMES`, none called as a built-in one
    /// or as one taken before it.
    pub(crate) fn apply_theme_settings(&mut self, ui: &Value) {
        if let Some(list) = ui.get("themes").and_then(Value::as_array) {
            self.themes.clear();
            for one in list.iter().take(MAX_THEMES * 2) {
                let Some(t) = Theme::from_json(one) else { continue };
                if self.themes.len() < MAX_THEMES && self.find_theme(&t.name).is_none() {
                    self.themes.push(t);
                }
            }
        }
        if let Some(name) = ui.get("themeName").and_then(Value::as_str)
            && let Some(found) = self.find_theme(name)
        {
            self.theme_name = found.name;
        }
        let dark = self.tokens.dark;
        self.set_dark(dark);
    }

    /// The Themes page of Options.
    pub(crate) fn themes_page(&mut self, ui: &mut Ui) {
        let Some(mut window) = self.themes_window.take() else { return };
        let t = self.tokens.clone();
        let all = self.all_themes();
        let current = self.current_theme();
        let editing_dark = t.dark;
        let editing = current.tokens(editing_dark);
        let mut run: Vec<(&'static str, Value)> = Vec::new();
        {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(220.0);
                    // How the window chooses between the sets.
                    dialog::heading(ui, &t, "Show");
                    ui.horizontal(|ui| {
                        for (label, mode) in [("Light", "light"), ("Dark", "dark"), ("As the system", "system")] {
                            let on = match mode {
                                "system" => self.follow_system,
                                "dark" => !self.follow_system && t.dark,
                                _ => !self.follow_system && !t.dark,
                            };
                            if ui.selectable_label(on, label).clicked() && !on {
                                run.push(("ui.theme", json!({ "theme": mode })));
                            }
                        }
                    });
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical().id_salt("theme-list").max_height(440.0).show(ui, |ui| {
                        dialog::heading(ui, &t, "Built in");
                        for one in all.iter().filter(|x| x.built_in) {
                            if theme_card(ui, &t, one, one.name == current.name).clicked() {
                                run.push(("ui.useTheme", json!({ "name": one.name })));
                            }
                        }
                        ui.add_space(6.0);
                        dialog::heading(ui, &t, "Yours");
                        let mut none = true;
                        for one in all.iter().filter(|x| !x.built_in) {
                            none = false;
                            if theme_card(ui, &t, one, one.name == current.name).clicked() {
                                run.push(("ui.useTheme", json!({ "name": one.name })));
                            }
                        }
                        if none {
                            ui.label(
                                egui::RichText::new("Change any colour and a copy of the theme becomes yours.").font(font(11.5)).color(t.text_faint),
                            );
                        }
                        ui.add_space(4.0);
                        if ui.add(dialog::plain("+ New theme")).on_hover_text("A copy of the chosen theme, to change").clicked() {
                            run.push(("ui.newTheme", Value::Null));
                        }
                    });
                });
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.set_width(520.0);
                    // The theme's name: typed in place for one's own.
                    ui.horizontal(|ui| {
                        if current.built_in {
                            ui.label(egui::RichText::new(&current.name).font(bold(17.0)).color(t.text));
                            ui.label(egui::RichText::new("built in: a change makes a copy that is yours").font(font(11.5)).color(t.text_faint));
                        } else {
                            let id = egui::Id::new("theme-name");
                            let typing = ui.memory(|m| m.has_focus(id));
                            if !typing {
                                window.name = current.name.clone();
                            }
                            let field = ui.add(LineEdit::new(&mut window.name, id).desired_width(300.0).char_limit(theme::MAX_NAME).font(bold(15.0)));
                            if field.lost_focus() && window.name.trim() != current.name {
                                run.push(("ui.renameTheme", json!({ "to": window.name })));
                            }
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Changing").color(t.text_dim));
                        // Changing a set shows it, as Show does: a change is never made out of sight.
                        for (label, dark) in [("Light set", false), ("Dark set", true)] {
                            if ui.selectable_label(editing_dark == dark, label).clicked() && editing_dark != dark {
                                run.push(("ui.theme", json!({ "theme": if dark { "dark" } else { "light" } })));
                            }
                        }
                    });
                    ui.add_space(6.0);
                    preview(ui, &editing, &current.name);
                    ui.add_space(8.0);
                    let mode = if editing_dark { "dark" } else { "light" };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new("Accent").color(t.text_dim)).on_hover_text("The colours that go with it follow");
                        for (r, g, b) in ACCENTS {
                            let base = Color32::from_rgb(r, g, b);
                            // The dark set takes a lighter one of each, to stand out on it.
                            let c = if editing_dark { theme::mix(base, Color32::WHITE, 0.45) } else { base };
                            let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                            let on = editing.accent == c;
                            ui.painter().circle_filled(rect.center(), 10.0, c);
                            if on || resp.hovered() {
                                ui.painter().circle_stroke(rect.center(), 11.5, egui::Stroke::new(2.0, t.text));
                            }
                            if resp.on_hover_text(theme::hex(c)).clicked() {
                                run.push(("ui.themeColor", json!({ "mode": mode, "color": "accent", "value": theme::hex(c), "follow": true })));
                            }
                        }
                        let mut c = editing.accent;
                        if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                            run.push(("ui.themeColor", json!({ "mode": mode, "color": "accent", "value": theme::hex(c), "follow": true })));
                        }
                    });
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().id_salt("theme-colours").max_height(250.0).auto_shrink([false, true]).show(ui, |ui| {
                        let mut group = "";
                        egui::Grid::new("theme-colour-grid").num_columns(4).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                            for (key, label, in_group) in COLORS {
                                if in_group != group {
                                    group = in_group;
                                    ui.label(egui::RichText::new(in_group).font(bold(12.5)).color(t.text));
                                    ui.end_row();
                                }
                                let Some(now) = editing.color(key) else { continue };
                                ui.label(egui::RichText::new(label).color(t.text_dim));
                                let mut c = now;
                                if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                                    run.push(("ui.themeColor", json!({ "mode": mode, "color": key, "value": theme::hex(c) })));
                                }
                                let id = egui::Id::new(("theme-hex", key));
                                let typing = ui.memory(|m| m.has_focus(id));
                                let text = window.hex.entry(key.to_string()).or_default();
                                if !typing {
                                    *text = theme::hex(now);
                                }
                                let field = ui.add(LineEdit::new(text, id).desired_width(80.0).char_limit(7).font(egui::FontId::monospace(12.0)));
                                if field.changed()
                                    && let Some(c) = theme::parse_hex(text)
                                    && c != now
                                {
                                    run.push(("ui.themeColor", json!({ "mode": mode, "color": key, "value": theme::hex(c) })));
                                }
                                let changed = current.changes(editing_dark).contains_key(key) && !current.built_in;
                                if ui
                                    .add_enabled(changed, egui::Button::new("Undo").small())
                                    .on_hover_text("Back to the colour the theme started from")
                                    .clicked()
                                {
                                    run.push(("ui.themeColor", json!({ "mode": mode, "color": key, "value": null })));
                                }
                                ui.end_row();
                            }
                        });
                    });
                    ui.add_space(8.0);
                    // Contrast, checked as it changes.
                    let low = editing.unreadable();
                    if low.is_empty() {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                            icons::paint(ui.painter(), r, Icon::Check, t.ok);
                            ui.label(egui::RichText::new("All text here is easy to read on what is behind it.").font(font(12.0)).color(t.text_dim));
                        });
                    }
                    for (what, ratio) in low {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                            icons::paint(ui.painter(), r, Icon::Importance, t.danger);
                            ui.label(egui::RichText::new(format!("{what} is hard to read: {ratio:.1} to 1")).font(font(12.0)).color(t.danger));
                        });
                    }
                });
            });
            // A built-in theme has nothing to delete, so no row for it.
            if !current.built_in {
                crate::options::bar(ui, &t, |ui| {
                    let label = if window.deleting { "Delete it" } else { "Delete theme…" };
                    let button = if window.deleting { dialog::danger(&t, label) } else { dialog::plain(label) };
                    if ui.add(button).clicked() {
                        if window.deleting {
                            run.push(("ui.deleteTheme", Value::Null));
                        }
                        window.deleting = !window.deleting;
                    }
                });
            }
        }
        self.themes_window = Some(window);
        for (id, params) in run {
            if let Err(e) = self.execute(id, &params) {
                self.session.status = e;
            }
        }
    }
}

/// A theme to choose: its name, and its light and dark sets side by side as small windows.
fn theme_card(ui: &mut Ui, t: &Tokens, one: &Theme, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(206.0, 46.0), Sense::click());
    let p = ui.painter();
    let fill = if on {
        t.selected
    } else if resp.hovered() {
        t.hover
    } else {
        t.surface
    };
    p.rect_filled(rect, 6.0, fill);
    p.rect_stroke(rect, 6.0, egui::Stroke::new(if on { 2.0 } else { 1.0 }, if on { t.accent } else { t.line }), egui::StrokeKind::Inside);
    for (n, dark) in [false, true].into_iter().enumerate() {
        let k = one.tokens(dark);
        let r = Rect::from_min_size(pos2(rect.left() + 8.0 + n as f32 * 30.0, rect.top() + 9.0), vec2(26.0, 28.0));
        p.rect_filled(r, 3.0, k.window);
        p.rect_filled(Rect::from_min_size(r.min, vec2(r.width(), 7.0)), egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, k.title_bg);
        p.rect_filled(Rect::from_min_size(r.min + vec2(4.0, 11.0), vec2(18.0, 5.0)), 1.0, k.surface);
        p.rect_filled(Rect::from_min_size(r.min + vec2(4.0, 19.0), vec2(11.0, 5.0)), 1.0, k.accent);
        p.rect_stroke(r, 3.0, egui::Stroke::new(1.0, t.line), egui::StrokeKind::Inside);
    }
    let words = crate::seen_line(&one.name, if on { bold(13.0) } else { font(13.0) }, t.text);
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(words));
    let at = pos2(rect.left() + 74.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().with_clip_rect(rect.shrink(4.0)).galley(at, galley, t.text);
    resp
}

/// A small window in the colours being changed: its bars, a list with a row selected, a dialog's
/// head and a button, so each colour can be seen where it goes.
fn preview(ui: &mut Ui, k: &Tokens, name: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(520.0, 150.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6.0, k.window);
    let title = Rect::from_min_size(rect.min, vec2(rect.width(), 24.0));
    p.rect_filled(title, egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, k.title_bg);
    p.text(title.left_center() + vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, "Inbox", font(12.0), k.title_text);
    p.rect_filled(Rect::from_center_size(title.center(), vec2(160.0, 16.0)), 4.0, k.title_field);
    let status = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 16.0), rect.max);
    p.rect_filled(status, egui::CornerRadius { nw: 0, ne: 0, sw: 6, se: 6 }, k.bar_bg);
    p.text(status.left_center() + vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, "Items: 9", font(10.5), k.bar_text);
    // Folders on the pane, the list on the surface.
    let folders = Rect::from_min_max(pos2(rect.left(), title.bottom()), pos2(rect.left() + 110.0, status.top()));
    p.rect_filled(folders, 0.0, k.pane);
    p.rect_filled(Rect::from_min_size(folders.min + vec2(6.0, 8.0), vec2(98.0, 20.0)), 3.0, k.selected);
    p.text(folders.min + vec2(14.0, 18.0), egui::Align2::LEFT_CENTER, "Inbox", font(11.5), k.text);
    p.text(folders.min + vec2(14.0, 42.0), egui::Align2::LEFT_CENTER, "Sent Items", font(11.5), k.text);
    p.text(folders.min + vec2(14.0, 64.0), egui::Align2::LEFT_CENTER, "Flagged", font(11.5), k.flag);
    let list = Rect::from_min_max(pos2(folders.right() + 1.0, title.bottom()), pos2(rect.left() + 300.0, status.top()));
    p.rect_filled(list, 0.0, k.surface);
    p.vline(folders.right(), list.y_range(), egui::Stroke::new(1.0, k.line));
    for (n, (who, what)) in [("Priya", "Launch notes"), ("Tomas", "RE: Plans"), ("Facilities", "Lift maintenance")].into_iter().enumerate() {
        let row = Rect::from_min_size(pos2(list.left(), list.top() + n as f32 * 36.0), vec2(list.width(), 36.0));
        if n == 0 {
            p.rect_filled(row, 0.0, k.selected);
            p.rect_filled(Rect::from_min_size(row.min, vec2(3.0, row.height())), 0.0, k.accent);
        }
        p.text(row.min + vec2(10.0, 11.0), egui::Align2::LEFT_CENTER, who, bold(11.5), k.text);
        p.text(row.min + vec2(10.0, 26.0), egui::Align2::LEFT_CENTER, what, font(11.0), if n == 0 { k.accent } else { k.text_dim });
        p.hline(row.x_range(), row.bottom(), egui::Stroke::new(1.0, k.line));
    }
    // A dialog standing over the reading pane.
    let dlg = Rect::from_min_size(pos2(list.right() + 16.0, title.bottom() + 12.0), vec2(rect.right() - list.right() - 28.0, 82.0));
    p.rect_filled(dlg, 6.0, k.surface);
    p.rect_stroke(dlg, 6.0, egui::Stroke::new(1.0, k.line), egui::StrokeKind::Inside);
    let head = Rect::from_min_size(dlg.min, vec2(dlg.width(), 22.0));
    p.rect_filled(head, egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, k.dialog_head);
    p.text(head.left_center() + vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, one_line(name), bold(11.0), k.dialog_text);
    p.text(dlg.min + vec2(8.0, 34.0), egui::Align2::LEFT_CENTER, "Quieter words", font(10.5), k.text_dim);
    let button = Rect::from_min_size(pos2(dlg.right() - 62.0, dlg.bottom() - 26.0), vec2(54.0, 20.0));
    p.rect_filled(button, 4.0, k.accent);
    p.text(button.center(), egui::Align2::CENTER_CENTER, "Save", bold(10.5), k.on_accent);
    let other = Rect::from_min_size(pos2(button.left() - 60.0, button.top()), vec2(54.0, 20.0));
    p.rect_filled(other, 4.0, k.hover);
    p.text(other.center(), egui::Align2::CENTER_CENTER, "Cancel", font(10.5), k.text);
    p.rect_stroke(rect, 6.0, egui::Stroke::new(1.0, k.line), egui::StrokeKind::Inside);
}
