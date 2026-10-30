//! Options: every setting of the program in one window, a page for each part down its side.
//! Accounts, themes, time zones and updates were windows of their own before; they are pages
//! here, and the ribbon's buttons for them open this window at their page. Every change is
//! made through the command that changes it, and is kept with the settings at once.

use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::{MailUi, RibbonMode, bold, font};
use egui::{Rect, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};

/// The pages, in the order they are listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Mail,
    Calendar,
    Reminders,
    Saving,
    Accounts,
    Themes,
    Updates,
}

impl Page {
    pub const ALL: [Page; 8] =
        [Page::General, Page::Mail, Page::Calendar, Page::Reminders, Page::Saving, Page::Accounts, Page::Themes, Page::Updates];

    pub fn name(self) -> &'static str {
        match self {
            Page::General => "general",
            Page::Mail => "mail",
            Page::Calendar => "calendar",
            Page::Reminders => "reminders",
            Page::Saving => "saving",
            Page::Accounts => "accounts",
            Page::Themes => "themes",
            Page::Updates => "updates",
        }
    }

    pub fn of(name: &str) -> Option<Page> {
        Page::ALL.into_iter().find(|p| p.name() == name)
    }

    fn label(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Mail => "Mail",
            Page::Calendar => "Calendar",
            Page::Reminders => "Reminders",
            Page::Saving => "Saving",
            Page::Accounts => "Accounts",
            Page::Themes => "Themes",
            Page::Updates => "Updates",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Page::General => Icon::Settings,
            Page::Mail => Icon::Mail,
            Page::Calendar => Icon::Clock,
            Page::Reminders => Icon::Bell,
            Page::Saving => Icon::Save,
            Page::Accounts => Icon::People,
            Page::Themes => Icon::Palette,
            Page::Updates => Icon::Update,
        }
    }

    /// What the page holds, under its name.
    fn about(self) -> &'static str {
        match self {
            Page::General => "How the window looks and how messages are read.",
            Page::Mail => "When mail is sent and received, and how new mail is told of.",
            Page::Calendar => "The time zone dates are shown in, and a second one beside it.",
            Page::Reminders => "What happens when an appointment, a task or flagged mail is due.",
            Page::Saving => "How changes reach a data file, and when they are saved on their own.",
            Page::Accounts => "Mail is sent and received through these accounts, each with a data file of its own.",
            Page::Themes => "Colours for the window and its dialogs, for the light set and the dark set each. A change shows at once.",
            Page::Updates => "This version, the newest release, and how new ones are looked for.",
        }
    }
}

/// How wide the list of pages stands.
const NAV_W: f32 = 168.0;
/// How wide a page is: the widest, Themes, fits.
const PAGE_W: f32 = 776.0;
const ROW_H: f32 = 34.0;

/// A row of buttons at the foot of a page, under a line: what a page's own foot held when it
/// was a window.
pub(crate) fn bar<R>(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(10.0);
    let y = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, t.line));
    ui.add_space(10.0);
    ui.horizontal(add).inner
}

impl MailUi {
    /// Open Options at `page`, making what the page keeps while it is open.
    pub(crate) fn open_options(&mut self, page: Page) -> Result<(), String> {
        match page {
            Page::Accounts if self.account_window.is_none() => self.open_accounts(None, false)?,
            Page::Themes if self.themes_window.is_none() => self.themes_window = Some(crate::themes::ThemesWindow::default()),
            Page::Updates => {
                // Opened on nothing found yet: asked at once, on a thread of its own.
                let asking = self.session.running().iter().any(|r| r.what == openmailclient_engine::update::CHECKING);
                if !self.session.update.checked && !asking && self.session.connector.is_some() {
                    let _ = self.session.run("help.checkForUpdates", &json!({ "wait": false }));
                }
            }
            _ => {}
        }
        self.options = Some(page);
        Ok(())
    }

    /// Options closed: what its pages kept goes, and a password typed on the Accounts page is
    /// wiped as the page's state is dropped.
    pub(crate) fn close_options(&mut self) {
        self.options = None;
        self.account_window = None;
        self.themes_window = None;
    }

    /// `ui.options`: open at a page, switch between pages, or close.
    pub(crate) fn options_command(&mut self, params: &Value) -> Result<(), String> {
        if params.get("close").and_then(Value::as_bool) == Some(true) {
            self.close_options();
            return Ok(());
        }
        let page = match params.get("page") {
            None | Some(Value::Null) => self.options.unwrap_or(Page::General),
            Some(Value::String(name)) => Page::of(name).ok_or_else(|| {
                format!(r#""page" is one of {}"#, Page::ALL.iter().map(|p| format!("\"{}\"", p.name())).collect::<Vec<_>>().join(", "))
            })?,
            Some(_) => return Err(r#""page" is a page's name"#.to_string()),
        };
        self.open_options(page)
    }

    /// A page's own command, `{"open": true or false}` or turned the other way when not given:
    /// Options at that page, or Options closed when it is the page shown.
    pub(crate) fn options_page_command(&mut self, page: Page, params: &Value) -> Result<(), String> {
        let open = match params.get("open") {
            None | Some(Value::Null) => self.options != Some(page),
            Some(Value::Bool(open)) => *open,
            Some(_) => return Err("\"open\" is true or false".to_string()),
        };
        if open {
            self.open_options(page)
        } else {
            if self.options == Some(page) {
                self.close_options();
            }
            Ok(())
        }
    }

    pub(crate) fn options_window(&mut self, ctx: &egui::Context) {
        let Some(page) = self.options else { return };
        let t = self.tokens.clone();
        let mut chosen = page;
        let mut close = false;
        // The window keeps one height from page to page, as far as the screen has room.
        let tall = (ctx.content_rect().height() - 250.0).clamp(120.0, 520.0);
        let shown = crate::dialog::Dialog::new("options", "Options", Icon::Settings).width(NAV_W + 18.0 + PAGE_W).show(ctx, &t, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(NAV_W);
                    for p in Page::ALL {
                        if nav_row(ui, &t, p, p == page).clicked() {
                            chosen = p;
                        }
                    }
                });
                ui.add_space(8.0);
                let line_x = ui.cursor().left();
                ui.add_space(9.0);
                // Accounts, Themes and Updates scroll their own long parts and keep their
                // buttons in sight; the other pages are short.
                ui.vertical(|ui| {
                    ui.set_width(PAGE_W);
                    ui.set_min_height(tall);
                    ui.label(egui::RichText::new(page.label()).font(bold(18.0)).color(t.text));
                    ui.label(egui::RichText::new(page.about()).font(font(12.0)).color(t.text_dim));
                    ui.add_space(10.0);
                    match page {
                        Page::General => self.general_page(ui),
                        Page::Mail => self.mail_page(ui),
                        Page::Calendar => self.time_zones_page(ui),
                        Page::Reminders => self.reminders_page(ui),
                        Page::Saving => self.saving_page(ui),
                        Page::Accounts => self.accounts_page(ui),
                        Page::Themes => self.themes_page(ui),
                        Page::Updates => self.updates_page(ui),
                    }
                });
                let span = ui.min_rect().y_range();
                ui.painter().vline(line_x, span, egui::Stroke::new(1.0, t.line));
            });
            crate::dialog::foot(ui, &t, |ui| {
                ui.label(egui::RichText::new("Every change is kept between runs as it is made.").font(font(11.5)).color(t.text_faint));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(crate::dialog::primary(&t, "Close")).clicked() {
                        close = true;
                    }
                });
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) && !ui.memory(|m| m.focused().is_some()) {
                close = true;
            }
        });
        // A page may have closed the window itself (Restart and Install).
        if self.options.is_none() {
            return;
        }
        if shown.closed || close {
            self.close_options();
        } else if chosen != page
            && let Err(e) = self.open_options(chosen)
        {
            self.session.status = e;
        }
    }

    fn general_page(&mut self, ui: &mut Ui) {
        let t = self.tokens.clone();
        crate::dialog::heading(ui, &t, "Ribbon");
        ui.horizontal(|ui| {
            for (label, mode, name) in [("Classic", RibbonMode::Classic, "classic"), ("Simplified", RibbonMode::Simplified, "simplified")] {
                if ui.radio(self.ribbon_mode == mode, label).clicked() && self.ribbon_mode != mode {
                    self.act("ui.ribbonMode", json!({ "mode": name }));
                }
            }
        });
        ui.add_space(8.0);
        crate::dialog::heading(ui, &t, "Reading");
        let mut plain = self.plain_bodies;
        if ui
            .checkbox(&mut plain, "Read every message as plain text")
            .on_hover_text("Formatting, pictures and links are shown as text only")
            .changed()
        {
            self.act("ui.bodyFormat", json!({ "format": if plain { "plain" } else { "formatted" } }));
        }
        let mut animate = self.session.animate;
        if ui.checkbox(&mut animate, "Play pictures that move").on_hover_text("Off, each stands still on its first frame").changed() {
            self.act("view.animations", json!({ "on": animate }));
        }
        ui.add_space(8.0);
        crate::dialog::heading(ui, &t, "Colours");
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Light, dark or as the system, and the theme's own colours, are on").font(font(12.5)).color(t.text_dim));
            if ui.link("Themes").clicked() {
                self.act("ui.options", json!({ "page": "themes" }));
            }
        });
    }

    fn mail_page(&mut self, ui: &mut Ui) {
        let t = self.tokens.clone();
        let s = &self.session;
        let (every, coming, alerts) = (s.send_receive_min, s.new_mail_as_it_comes, s.alerts);
        crate::dialog::heading(ui, &t, "Send/Receive");
        ui.horizontal(|ui| {
            let mut on = every > 0;
            let mut minutes = if every > 0 { every } else { 30 };
            let ticked = ui.checkbox(&mut on, "Send and receive every").changed();
            let dragged = ui.add_enabled(on, egui::DragValue::new(&mut minutes).range(1..=1440).suffix(" minutes")).changed();
            if ticked || dragged {
                self.act("mail.sendReceiveEvery", json!({ "minutes": if on { minutes } else { 0 } }));
            }
        });
        let mut coming = coming;
        if ui
            .checkbox(&mut coming, "Receive new mail the moment it comes")
            .on_hover_text("Each IMAP Inbox whose server can tell is watched on a connection of its own")
            .changed()
        {
            self.act("mail.newMailAsItComes", json!({ "on": coming }));
        }
        ui.add_space(8.0);
        crate::dialog::heading(ui, &t, "When new mail comes");
        let mut a = alerts;
        let mut changed = ui.checkbox(&mut a.alert, "Show an alert at the corner of the screen").changed();
        changed |= ui.checkbox(&mut a.sound, "Play a sound").changed();
        changed |= ui.checkbox(&mut a.flash, "Flash the taskbar button").changed();
        if changed {
            self.act("mail.newMailAlerts", json!({ "alert": a.alert, "sound": a.sound, "flash": a.flash }));
        }
        ui.add_space(8.0);
        crate::dialog::heading(ui, &t, "Accounts");
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Servers, addresses and passwords are on").font(font(12.5)).color(t.text_dim));
            if ui.link("Accounts").clicked() {
                self.act("ui.options", json!({ "page": "accounts" }));
            }
        });
    }

    fn reminders_page(&mut self, ui: &mut Ui) {
        let t = self.tokens.clone();
        let (mut show, mut sound) = (self.session.reminders.show, self.session.reminders.sound);
        crate::dialog::heading(ui, &t, "When a reminder is due");
        let mut changed = ui.checkbox(&mut show, "Bring up the Reminders window").changed();
        changed |= ui.checkbox(&mut sound, "Play a sound").changed();
        if changed {
            self.act("reminders.options", json!({ "show": show, "sound": sound }));
        }
        // The sound: our own chime, or a WAVE file of the owner's.
        ui.add_space(6.0);
        let file = self.session.reminders.sound_file.clone();
        let name = file
            .as_deref()
            .map_or("Our own chime".to_string(), |f| std::path::Path::new(f).file_name().map_or(f.to_string(), |n| n.to_string_lossy().into_owned()));
        ui.add_enabled_ui(sound, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Sound:").color(t.text_dim));
                let shown = ui.add(egui::Label::new(egui::RichText::new(&name).color(t.text)).truncate());
                if let Some(f) = &file {
                    shown.on_hover_text(f);
                }
            });
            ui.horizontal(|ui| {
                if ui.add(crate::dialog::plain("Browse...")).on_hover_text("Choose a WAVE (.wav) file").clicked() {
                    self.requests.push(crate::UiRequest::ReminderSound);
                }
                if file.is_some() && ui.add(crate::dialog::plain("Use our own chime")).clicked() {
                    self.act("reminders.options", json!({ "soundFile": null }));
                }
                if ui.add(crate::dialog::plain("Play")).on_hover_text("Hear it now").clicked() {
                    self.try_reminder_sound();
                }
            });
        });
    }

    fn saving_page(&mut self, ui: &mut Ui) {
        let t = self.tokens.clone();
        let s = &self.session;
        let (auto, mut keep, mut in_place) = (s.autosave_min, s.keep_original, s.save_in_place);
        crate::dialog::heading(ui, &t, "Saving on its own");
        ui.horizontal(|ui| {
            let mut on = auto > 0;
            let mut minutes = if auto > 0 { auto } else { 5 };
            let ticked = ui.checkbox(&mut on, "Save changes that have waited").changed();
            let dragged = ui.add_enabled(on, egui::DragValue::new(&mut minutes).range(1..=240).suffix(" minutes")).changed();
            if ticked || dragged {
                self.act("file.autoSave", json!({ "minutes": if on { minutes } else { 0 } }));
            }
        });
        ui.label(
            egui::RichText::new("Only at a quiet moment, and never a data file's first save: that waits for Save Changes.")
                .font(font(12.0))
                .color(t.text_dim),
        );
        ui.add_space(8.0);
        crate::dialog::heading(ui, &t, "Saving a data file");
        if ui
            .checkbox(&mut in_place, "Write only what changed, into the file where it is")
            .on_hover_text("Off, every save writes the whole file anew; either way a save that fails leaves the file as it was")
            .changed()
        {
            self.act("file.saveInPlace", json!({ "on": in_place }));
        }
        if ui
            .checkbox(&mut keep, "Keep the file as it was beside it on its first save")
            .on_hover_text("Under a name ending in .before-openmailclient.pst")
            .changed()
        {
            self.act("file.keepOriginal", json!({ "on": keep }));
        }
    }
}

/// One page in the list: its mark and name, lit when shown.
fn nav_row(ui: &mut Ui, t: &Tokens, page: Page, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(NAV_W, ROW_H), Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 6.0, t.selected);
        p.rect_filled(Rect::from_min_size(rect.min + vec2(0.0, 7.0), vec2(3.0, ROW_H - 14.0)), 1.5, t.accent);
    } else if resp.hovered() {
        p.rect_filled(rect, 6.0, t.hover);
    }
    let color = if on { t.text } else { t.text_dim };
    icons::paint(p, Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), vec2(16.0, 16.0)), page.icon(), color);
    p.text(pos2(rect.left() + 40.0, rect.center().y), egui::Align2::LEFT_CENTER, page.label(), if on { bold(13.0) } else { font(13.0) }, color);
    // Read by name, as a button is.
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, page.label()));
    resp
}
