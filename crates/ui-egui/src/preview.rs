//! The print window: the pages as they will be printed, beside how they are set up.
//!
//! The pages are drawn from what `file.print` writes, so what is seen here is what the file
//! holds. Every control runs `file.pageSetup`; a print on a printer is `file.print` with the
//! printer `file.printers` chose, and a PDF file is asked of the host.

use egui::{Align2, Color32, ColorImage, Key, Rect, Sense, TextureHandle, TextureOptions, pos2, vec2};
use openmailclient_engine::print::{self, Field, Laid, Links, Listing, Rows, Style};
use openmailclient_layout::{MM, Paper, Setup};
use openmailclient_model::FolderId;
use serde_json::{Value, json};

use crate::{MailUi, UiRequest, font};

/// The room the page is shown in, in points of the screen.
const ROOM: egui::Vec2 = vec2(470.0, 560.0);
/// The widest a page is drawn, in pixels.
const MAX_PIXELS: f32 = 2400.0;
/// The sizes a page is stepped through, in hundredths of its size on paper, and the screen's
/// points to a point of the page at 100.
const ZOOMS: [f32; 8] = [25.0, 50.0, 75.0, 100.0, 150.0, 200.0, 300.0, 400.0];
const ACTUAL: f32 = 96.0 / 72.0;

/// The print window while it is open.
#[derive(Default)]
pub(crate) struct Preview {
    /// The page looked at; the first is 0.
    pub page: usize,
    /// What the pages were laid out for: the message, the folder and its list, the style and
    /// the setup.
    of: Option<Of>,
    /// The pages, or why there are none.
    pub laid: Option<Result<Laid, String>>,
    /// The drawn page, with which page it is and how many pixels wide.
    shown: Option<(TextureHandle, usize, usize)>,
    /// How large the page is shown, in hundredths of its size on paper; `None` is as large
    /// as the room lets the whole page be.
    pub zoom: Option<f32>,
    /// What that came to when the page was last drawn to fit, in the same measure.
    fit: f32,
    /// This system's printers and the one printed on, as `file.printers` says; looked for when
    /// the window opens.
    printers: Option<Value>,
    /// Copies printed, 1 to 99 (0 is 1), and the pages printed, from 1: all of them when not set.
    copies: u32,
    range: Option<(usize, usize)>,
    /// The Print button was pressed by a command: the pages go when next laid out.
    print: bool,
}

/// What a preview was laid out for; the last part is what the Calendar or People shows.
type Of = (Option<u64>, Option<(usize, FolderId)>, (usize, usize), Style, Setup, Listing, String);

impl Preview {
    /// Show the page at the size `asked`: "fit", "in" or "out" for a step, or hundredths of
    /// its size on paper, 25 to 400.
    pub(crate) fn zoom_to(&mut self, asked: &Value) -> Result<(), String> {
        let (least, most) = (ZOOMS[0], ZOOMS[ZOOMS.len() - 1]);
        let now = self.zoom.unwrap_or(if self.fit > 0.0 { self.fit } else { 100.0 });
        let bad = || format!("\"zoom\" is \"fit\", \"in\", \"out\" or a number from {least} to {most}");
        self.zoom = match asked {
            Value::String(name) => match name.trim().to_ascii_lowercase().as_str() {
                "fit" => None,
                "in" => Some(ZOOMS.iter().copied().find(|z| *z > now + 0.5).unwrap_or(most)),
                "out" => Some(ZOOMS.iter().rev().copied().find(|z| *z < now - 0.5).unwrap_or(least)),
                _ => return Err(bad()),
            },
            Value::Number(n) => Some(n.as_f64().filter(|z| (f64::from(least)..=f64::from(most)).contains(z)).ok_or_else(bad)? as f32),
            _ => return Err(bad()),
        };
        Ok(())
    }

    /// Take the copies, the pages and whether to print from `ui.printPreview`'s parameters.
    pub(crate) fn ask(&mut self, p: &Value) -> Result<(), String> {
        if let Some(copies) = p.get("copies") {
            let most = u64::from(openmailclient_engine::printers::MAX_COPIES);
            self.copies = copies.as_u64().filter(|n| (1..=most).contains(n)).ok_or(format!("\"copies\" is from 1 to {most}"))? as u32;
        }
        match p.get("pages") {
            None => {}
            Some(Value::String(all)) if all == "all" => self.range = None,
            Some(Value::Array(two)) => {
                let page = |at: usize| two.get(at).and_then(Value::as_u64).filter(|n| *n >= 1).and_then(|n| usize::try_from(n).ok());
                let (from, to) = page(0).zip(page(1)).ok_or("\"pages\" is [from, to], from 1")?;
                self.range = Some((from, to.max(from)));
            }
            Some(_) => return Err("\"pages\" is [from, to] or \"all\"".into()),
        }
        if p.get("print").and_then(Value::as_bool) == Some(true) {
            self.print = true;
        }
        Ok(())
    }

    pub(crate) fn pages(&self) -> usize {
        self.laid.as_ref().and_then(|laid| laid.as_ref().ok()).map_or(0, |laid| laid.printed.pages.len())
    }
}

fn field_label(field: Field) -> &'static str {
    match field {
        Field::Who => "From, or To in outgoing mail",
        Field::Date => "Date",
        other => other.label(false),
    }
}

fn paper_label(name: &str) -> &'static str {
    match name {
        "a4" => "A4",
        "letter" => "Letter",
        "legal" => "Legal",
        "a3" => "A3",
        "a5" => "A5",
        "b5" => "B5",
        "executive" => "Executive",
        "tabloid" => "Tabloid",
        _ => "Custom",
    }
}

impl MailUi {
    pub(crate) fn preview_window(&mut self, ctx: &egui::Context) {
        let Some(mut pv) = self.preview.take() else { return };
        // With no message open and no folder shown there is nothing to print.
        if !self.session.is_enabled("file.print") {
            return;
        }
        let t = self.tokens.clone();
        let style = print::style_now(&self.session);
        let listed = (self.session.rows.len(), self.session.rows.iter().filter(|row| !row.read).count());
        let listing = self.session.print_list.clone();
        let s = &self.session;
        let elsewhere = format!(
            "{:?} {} {:?} {} {}",
            s.module,
            s.calendar.day,
            s.calendar.view,
            s.calendar.items.iter().map(|(id, a)| id ^ a.start as u64 ^ a.end as u64).fold(0u64, |a, b| a.rotate_left(5) ^ b),
            s.people.items.len()
        );
        let of = (self.session.selected, self.session.current, listed, style, self.session.page_setup.clone(), listing.clone(), elsewhere);
        if pv.of.as_ref() != Some(&of) {
            pv.of = Some(of);
            pv.laid = None;
            pv.shown = None;
        }
        if pv.laid.is_none() {
            // The Calendar's days are read for the print before it is laid out.
            let gathered = openmailclient_engine::print_more::gather(&mut self.session, style, &Value::Null);
            let s = &self.session;
            pv.laid =
                Some(gathered.and_then(|()| print::laid_as(s, style, &s.page_setup, &s.print_list, Links::default())).map_err(|e| e.to_string()));
        }
        let pages = pv.pages();
        pv.page = pv.page.min(pages.saturating_sub(1));

        if pv.printers.is_none() {
            pv.printers = Some(self.execute("file.printers", &Value::Null).unwrap_or(Value::Null));
        }
        let listed: Vec<String> = pv
            .printers
            .as_ref()
            .and_then(|p| p["printers"].as_array())
            .map(|all| all.iter().filter_map(|p| p["name"].as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let printer = pv.printers.as_ref().and_then(|p| p["printer"].as_str()).map(str::to_string);
        pv.copies = pv.copies.clamp(1, openmailclient_engine::printers::MAX_COPIES);
        let most = pages.max(1);
        pv.range = pv.range.map(|(from, to)| (from.clamp(1, most), to.clamp(1, most))).map(|(from, to)| (from, to.max(from)));

        let setup = self.session.page_setup.clone();
        // What the person asked for this frame: changes to the setup, then print or close.
        let mut changes: Vec<Value> = Vec::new();
        let (mut print_now, mut close, mut choose, mut on_paper) = (false, false, None::<String>, false);
        let shown = crate::dialog::Dialog::new("print", "Print", crate::icons::Icon::Print).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(
            ctx,
            &t,
            |ui| {
                let dim = |text: &str| egui::Label::new(egui::RichText::new(text).font(font(12.0)).color(t.text_dim));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(236.0);
                        crate::dialog::heading(ui, &t, "Printer");
                        if listed.is_empty() {
                            ui.add(dim("No printer was found; the pages can be saved as a PDF file.").wrap());
                        } else {
                            let now = printer.clone().unwrap_or_default();
                            egui::ComboBox::from_id_salt("print-printer").selected_text(&now).width(226.0).show_ui(ui, |ui| {
                                for name in &listed {
                                    if ui.selectable_label(*name == now, name).clicked() && *name != now {
                                        choose = Some(name.clone());
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.add(dim("Copies"));
                                ui.add(egui::DragValue::new(&mut pv.copies).range(1..=openmailclient_engine::printers::MAX_COPIES).speed(0.1));
                            });
                            ui.horizontal(|ui| {
                                ui.add(dim("Pages"));
                                if ui.selectable_label(pv.range.is_none(), "All").clicked() {
                                    pv.range = None;
                                }
                                if ui.selectable_label(pv.range.is_some(), "From").clicked() && pv.range.is_none() {
                                    pv.range = Some((pv.page + 1, pv.page + 1));
                                }
                                if let Some((from, to)) = pv.range.as_mut() {
                                    ui.add(egui::DragValue::new(from).range(1..=most).speed(0.1));
                                    ui.add(dim("to"));
                                    ui.add(egui::DragValue::new(to).range(1..=most).speed(0.1));
                                }
                            });
                        }
                        ui.add_space(6.0);
                        crate::dialog::heading(ui, &t, "Include");
                        let mut details = setup.details;
                        let about = "The owner's name and From, Sent, To and Subject over a message; the folder's name over a list";
                        if ui.checkbox(&mut details, "Name and message details").on_hover_text(about).changed() {
                            changes.push(json!({ "details": details }));
                        }
                        let mut edges = setup.edges;
                        if ui.checkbox(&mut edges, "Page header and footer").changed() {
                            changes.push(json!({ "headerFooter": edges }));
                        }
                        let alone = !setup.details && !setup.edges;
                        let only = egui::Button::new("Message body only");
                        if ui.add_enabled(!alone, only).on_hover_text("Neither of the two: what the message says, and nothing more").clicked() {
                            changes.push(json!({ "bodyOnly": true }));
                        }
                        ui.add_space(6.0);
                        crate::dialog::heading(ui, &t, "What to print");
                        ui.horizontal(|ui| {
                            for one in Style::of(self.session.module).iter().copied() {
                                if ui.selectable_label(style == one, one.label()).clicked() && style != one {
                                    changes.push(json!({ "style": one.name() }));
                                }
                            }
                        });
                        if style == Style::Table {
                            ui.horizontal(|ui| {
                                for (label, rows) in [("All rows", Rows::All), ("Selected row", Rows::Selected)] {
                                    if ui.selectable_label(listing.rows == rows, label).clicked() && listing.rows != rows {
                                        changes.push(json!({ "rows": rows.name() }));
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                let mut groups = listing.groups;
                                if ui.checkbox(&mut groups, "Group headings").changed() {
                                    changes.push(json!({ "groups": groups }));
                                }
                                ui.menu_button("Columns", |ui| {
                                    for field in Field::ALL {
                                        let mut on = listing.columns.contains(&field);
                                        if ui.checkbox(&mut on, field_label(field)).changed() {
                                            let mut columns = listing.columns.clone();
                                            if on {
                                                columns.push(field);
                                            } else {
                                                columns.retain(|f| *f != field);
                                            }
                                            // The last column stays: a table has one at least.
                                            if !columns.is_empty() {
                                                changes.push(json!({ "columns": columns.iter().map(|f| f.name()).collect::<Vec<_>>() }));
                                            }
                                        }
                                    }
                                });
                            });
                        }
                        ui.add_space(6.0);
                        crate::dialog::heading(ui, &t, "Paper");
                        let now = setup.paper.name().unwrap_or("");
                        egui::ComboBox::from_id_salt("print-paper").selected_text(paper_label(now)).width(150.0).show_ui(ui, |ui| {
                            for name in Paper::NAMES {
                                if ui.selectable_label(name == now, paper_label(name)).clicked() && name != now {
                                    changes.push(json!({ "paper": name }));
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            for (label, landscape) in [("Portrait", false), ("Landscape", true)] {
                                if ui.selectable_label(setup.landscape == landscape, label).clicked() && setup.landscape != landscape {
                                    changes.push(json!({ "orientation": label.to_ascii_lowercase() }));
                                }
                            }
                        });
                    });
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.set_width(236.0);
                        crate::dialog::heading(ui, &t, "Margins (mm)");
                        egui::Grid::new("print-margins").num_columns(4).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
                            for (row, edges) in [[("Top", 0usize), ("Bottom", 2)], [("Left", 3), ("Right", 1)]].into_iter().enumerate() {
                                for (label, at) in edges {
                                    let was = (f64::from(setup.margins.get(at).copied().unwrap_or(0.0) / MM) * 10.0).round() / 10.0;
                                    let mut mm = was;
                                    ui.add(dim(label));
                                    let field = egui::DragValue::new(&mut mm).range(0.0..=150.0).speed(0.5).max_decimals(1);
                                    if ui.add(field).changed() && (mm - was).abs() > 0.04 {
                                        changes.push(json!({ "margins": { label.to_ascii_lowercase(): mm } }));
                                    }
                                }
                                if row == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                        ui.add_space(8.0);
                        for (label, key, parts) in [("Header", "header", &setup.header), ("Footer", "footer", &setup.footer)] {
                            // Kept while they are not printed, and changed only where they are.
                            ui.add_enabled_ui(setup.edges, |ui| {
                                ui.add(dim(label));
                                for (part, hint) in parts.iter().zip(["left", "center", "right"]) {
                                    let mut text = part.clone();
                                    let field = crate::line_edit::LineEdit::new(&mut text, egui::Id::new(("page-setup", key, hint)))
                                        .hint(hint, None)
                                        .desired_width(226.0)
                                        .font(font(12.5));
                                    if ui.add(field).changed() {
                                        changes.push(json!({ key: { hint: text } }));
                                    }
                                }
                                ui.add_space(4.0);
                            });
                        }
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new("[Page]  [Pages]  [Page of Pages]  [Date]  [Time]  [User]  [Subject]  [Folder]")
                                    .font(font(11.0))
                                    .color(t.text_faint),
                            )
                            .wrap(),
                        );
                    });
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        let (room, over) = ui.allocate_exact_size(ROOM, Sense::hover());
                        ui.painter().rect_filled(room, 2.0, t.pane);
                        // Ctrl and the wheel, or a pinch, over the page makes it larger or smaller.
                        let pinch = ui.input(|i| i.zoom_delta());
                        if over.hovered() && (pinch - 1.0).abs() > 0.001 && pinch.is_finite() {
                            let now = pv.zoom.unwrap_or(if pv.fit > 0.0 { pv.fit } else { 100.0 });
                            pv.zoom = Some((now * pinch).clamp(ZOOMS[0], ZOOMS[ZOOMS.len() - 1]));
                        }
                        match pv.laid.as_ref() {
                            Some(Ok(laid)) => {
                                if let Some(page) = laid.printed.pages.get(pv.page) {
                                    // The page as large as the room lets it be, a little in from its edges.
                                    let fit = ((room.width() - 24.0) / page.width.max(1.0)).min((room.height() - 24.0) / page.height.max(1.0));
                                    pv.fit = fit / ACTUAL * 100.0;
                                    let scale = pv.zoom.map_or(fit, |zoom| zoom / 100.0 * ACTUAL);
                                    let size = vec2(page.width * scale, page.height * scale);
                                    // No wider or taller than a picture may be here: a larger one
                                    // is refused by what draws it. It is stretched to its place.
                                    let side = (ui.ctx().input(|i| i.max_texture_side) as f32 - 2.0).max(16.0);
                                    let widest = MAX_PIXELS.min(side).min(side * page.width / page.height.max(1.0)).max(16.0);
                                    let pixels = (size.x * ui.ctx().pixels_per_point()).clamp(16.0, widest).floor() as usize;
                                    if !pv.shown.as_ref().is_some_and(|(_, page, wide)| *page == pv.page && *wide == pixels) {
                                        let drawn = print::raster(laid, pv.page, pixels as f32 / page.width.max(1.0));
                                        pv.shown = drawn.filter(|image| image.width.max(image.height) as f32 <= side + 2.0).map(|image| {
                                            let picture = ColorImage::from_rgb([image.width, image.height], &image.rgb);
                                            (ui.ctx().load_texture("print-preview", picture, TextureOptions::LINEAR), pv.page, pixels)
                                        });
                                    }
                                    let texture = pv.shown.as_ref().map(|(texture, ..)| texture.id());
                                    let draw = |ui: &egui::Ui, sheet: Rect| {
                                        ui.painter().rect_filled(sheet.translate(vec2(2.0, 2.0)), 0.0, Color32::from_black_alpha(40));
                                        ui.painter().rect_filled(sheet, 0.0, Color32::WHITE);
                                        if let Some(texture) = texture {
                                            ui.painter().image(texture, sheet, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                                        }
                                    };
                                    if size.x <= room.width() - 24.0 && size.y <= room.height() - 24.0 {
                                        draw(ui, Rect::from_center_size(room.center(), size));
                                    } else {
                                        // Larger than the room: the page is moved about under it.
                                        ui.scope_builder(egui::UiBuilder::new().max_rect(room), |ui| {
                                            egui::ScrollArea::both().id_salt("print-page").auto_shrink(false).show(ui, |ui| {
                                                let (all, _) = ui.allocate_exact_size(size + vec2(24.0, 24.0), Sense::hover());
                                                draw(ui, Rect::from_center_size(all.center(), size));
                                            });
                                        });
                                    }
                                }
                            }
                            Some(Err(why)) => {
                                let words = egui::Label::new(crate::seen_line(why, font(13.0), t.text)).wrap();
                                ui.put(room.shrink(40.0), words);
                            }
                            None => {}
                        }
                        ui.horizontal(|ui| {
                            if ui.add_enabled(pv.page > 0, egui::Button::new("Previous")).clicked() {
                                pv.page -= 1;
                            }
                            if ui.add_enabled(pv.page + 1 < pages, egui::Button::new("Next")).clicked() {
                                pv.page += 1;
                            }
                            if pages > 0 {
                                ui.add(dim(&format!("Page {} of {pages}", pv.page + 1)));
                                ui.add_space(10.0);
                                let now = pv.zoom.unwrap_or(pv.fit);
                                if ui.add_enabled(now > ZOOMS[0] + 0.5, egui::Button::new("-")).on_hover_text("Smaller").clicked() {
                                    let _ = pv.zoom_to(&json!("out"));
                                }
                                ui.add(dim(&format!("{}%", now.round())));
                                if ui.add_enabled(now < ZOOMS[ZOOMS.len() - 1] - 0.5, egui::Button::new("+")).on_hover_text("Larger").clicked() {
                                    let _ = pv.zoom_to(&json!("in"));
                                }
                                if ui.add_enabled(pv.zoom.is_some(), egui::Button::new("Whole page")).clicked() {
                                    pv.zoom = None;
                                }
                            }
                            if pv.laid.as_ref().is_some_and(|laid| laid.as_ref().is_ok_and(|laid| laid.printed.truncated)) {
                                ui.add(egui::Label::new(
                                    egui::RichText::new("The message is longer than may be printed.").font(font(12.0)).color(t.danger),
                                ));
                            }
                        });
                    });
                });
                // Keys turn the pages when nothing is being typed in.
                if ui.memory(|m| m.focused().is_none()) {
                    let hit = |key: Key| ui.input(|i| i.key_pressed(key));
                    if (hit(Key::PageDown) || hit(Key::ArrowRight)) && pv.page + 1 < pages {
                        pv.page += 1;
                    }
                    if (hit(Key::PageUp) || hit(Key::ArrowLeft)) && pv.page > 0 {
                        pv.page -= 1;
                    }
                    if hit(Key::Home) {
                        pv.page = 0;
                    }
                    if hit(Key::End) {
                        pv.page = pages.saturating_sub(1);
                    }
                }
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    close = true;
                }
                crate::dialog::foot(ui, &t, |ui| {
                    let changed = setup != Setup::default() || style != Style::default() || listing != Listing::default();
                    if ui.add_enabled(changed, crate::dialog::plain("Defaults")).on_hover_text("The page setup and style it started with").clicked() {
                        changes.push(json!({ "reset": true }));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(crate::dialog::plain("Close")).clicked() {
                            close = true;
                        }
                        if printer.is_some() {
                            if ui.add_enabled(pages > 0, crate::dialog::primary(&t, "Print")).clicked() {
                                on_paper = true;
                            }
                            if ui.add_enabled(pages > 0, crate::dialog::plain("Save as PDF…")).clicked() {
                                print_now = true;
                            }
                        } else if ui.add_enabled(pages > 0, crate::dialog::primary(&t, "Save as PDF…")).clicked() {
                            print_now = true;
                        }
                    });
                });
            },
        );
        if shown.closed {
            close = true;
        }
        for change in changes {
            if let Err(e) = self.execute("file.pageSetup", &change) {
                self.session.status = e;
            }
        }
        if let Some(name) = choose {
            match self.execute("file.printers", &json!({ "choose": name })) {
                Ok(now) => pv.printers = Some(now),
                Err(e) => self.session.status = e,
            }
        }
        if print_now {
            self.requests.push(UiRequest::Print);
        }
        // Asked for by a command: only once there are pages, and only on a printer there is.
        if std::mem::take(&mut pv.print) {
            if pages > 0 && printer.is_some() {
                on_paper = true;
            } else {
                self.session.status = "There is no printer to print on".to_string();
            }
        }
        if on_paper && let Some(name) = printer {
            let (from, to) = pv.range.unwrap_or((1, most));
            // On a thread of its own: it is seen in Show Progress and the window goes on.
            let asked = json!({ "printer": name, "copies": pv.copies, "from": from, "to": to, "wait": false });
            if let Err(e) = self.execute("file.print", &asked) {
                self.session.status = e;
                on_paper = false;
            }
        }
        if !close && !print_now && !on_paper {
            self.preview = Some(pv);
        }
    }
}
