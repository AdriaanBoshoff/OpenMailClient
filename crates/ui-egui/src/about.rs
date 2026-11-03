//! About OpenMailClient: the program's own icon, its version and where it is made, its licence,
//! and that it is not Microsoft's. Check for Updates and the project's page are a click away.

use crate::icons::Icon;
use crate::{MailUi, UiRequest, bold, font};
use openmailclient_engine::update;

/// How large the icon stands at the top.
const ICON: f32 = 64.0;

impl MailUi {
    pub(crate) fn about_window(&mut self, ctx: &egui::Context) {
        if !self.about {
            return;
        }
        let t = self.tokens.clone();
        let texture = self
            .about_icon
            .get_or_insert_with(|| {
                let side = (ICON * ctx.pixels_per_point()).ceil().clamp(16.0, 256.0) as u32;
                let image = egui::ColorImage::from_rgba_unmultiplied([side as usize, side as usize], &crate::app_icon::rgba(side));
                ctx.load_texture("about-icon", image, egui::TextureOptions::LINEAR)
            })
            .clone();
        let page = format!("https://github.com/{}", update::REPOSITORY);
        let mut close = false;
        let mut go: Option<String> = None;
        let mut updates = false;
        let shown = crate::dialog::Dialog::new("about", "About OpenMailClient", Icon::Info).width(420.0).show(ctx, &t, |ui| {
            ui.vertical_centered(|ui| {
                ui.add(egui::Image::new((texture.id(), egui::vec2(ICON, ICON))));
                ui.add_space(6.0);
                ui.label(egui::RichText::new("OpenMailClient").font(bold(18.0)).color(t.text));
                ui.label(egui::RichText::new(format!("Version {}", update::current())).font(font(13.0)).color(t.text_dim));
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("An open-source mail, calendar, contacts and tasks client, written in Rust.").font(font(12.5)).color(t.text),
                );
                ui.label(egui::RichText::new("Licensed under MIT or Apache-2.0.").font(font(12.5)).color(t.text));
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("An independent project, not affiliated with or endorsed by Microsoft.").font(font(11.5)).color(t.text_dim),
                );
                ui.add_space(4.0);
                if ui.link(egui::RichText::new(&page).font(font(12.0))).clicked() {
                    go = Some(page.clone());
                }
            });
            crate::dialog::foot(ui, &t, |ui| {
                if ui.add(crate::dialog::plain("Check for Updates")).clicked() {
                    updates = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(crate::dialog::primary(&t, "OK")).clicked() {
                        close = true;
                    }
                });
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)) {
            close = true;
        }
        if let Some(link) = go {
            self.requests.push(UiRequest::OpenLink(link));
        }
        if updates {
            close = true;
            if let Err(e) = self.execute("ui.updates", &serde_json::Value::Null) {
                self.session.status = e;
            }
        }
        if shown.closed || close {
            self.about = false;
        }
    }
}
