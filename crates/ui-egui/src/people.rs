//! The People module: a Contacts folder's contacts and groups in the five views Outlook's Current
//! View offers, found as they are typed for, each opened in a window to be made or changed, and
//! vCard files shown card by card before anything is added.
//!
//! Better than Outlook's: the search looks in every field, notes and numbers too, as it is typed;
//! a card opened again brings its contact up to date instead of making a second one, keeping what
//! the card does not say; and a contact's picture is decoded on a thread of its own.

use crate::dialog::{self, Dialog};
use crate::icons::{self, Icon};
use crate::line_edit::LineEdit;
use crate::theme::{self, Tokens};
use crate::{MailUi, bold, clip_text, font, one_line};
use egui::{Align2, Color32, Painter, Rect, Sense, TextureHandle, Ui, pos2, vec2};
use openmailclient_engine::cmd::people::View;
use openmailclient_formats::vcf::Card;
use openmailclient_model::contact::{Address, Contact, Group, Phone, Place};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// The kinds of number the contact window has a field for, in its order.
const PHONES: [Phone; 5] = [Phone::Business, Phone::Home, Phone::Mobile, Phone::BusinessFax, Phone::Other];
const PLACES: [Place; 2] = [Place::Business, Place::Home];

fn initials(name: &str) -> String {
    let mut out: String =
        name.split_whitespace().filter_map(|w| w.chars().find(|c| c.is_alphanumeric())).take(2).flat_map(char::to_uppercase).collect();
    if out.is_empty() {
        out.push('?');
    }
    out
}

fn label_of(card: &Card) -> String {
    let name = one_line(&card.name());
    if name.is_empty() { "(No name)".to_string() } else { name }
}

/// The line under a name: where they work, else their first address; a group's size.
fn second_line(card: &Card) -> String {
    match card {
        Card::Contact(c) => {
            let work = [c.job_title.trim(), c.company.trim()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
            if work.is_empty() { c.emails.first().map(|e| e.address.clone()).unwrap_or_default() } else { one_line(&work) }
        }
        Card::Group(g) => match g.members.len() {
            1 => "Contact group, 1 member".to_string(),
            n => format!("Contact group, {n} members"),
        },
    }
}

fn day_text(day: Option<i64>) -> String {
    day.map(|d| {
        let c = openmailclient_model::time::civil(openmailclient_model::time::from_unix(d.saturating_mul(86_400)), 0);
        format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
    })
    .unwrap_or_default()
}

/// A contact's picture, decoded apart from the thread that draws the window.
#[derive(Default)]
pub(crate) struct Face {
    /// Whose it is, and a hash of the picture, so a changed one is decoded again.
    key: Option<(u64, u64)>,
    texture: Option<TextureHandle>,
    waiting: Option<Receiver<Option<egui::ColorImage>>>,
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01B3))
}

fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let pixels = openmailclient_engine::pictures::decode(bytes)?;
    let size = [usize::try_from(pixels.width).ok()?, usize::try_from(pixels.height).ok()?];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, &pixels.rgba))
}

impl Face {
    /// The picture of contact `id`, once it is decoded; asked for when it is not.
    fn of(&mut self, ctx: &egui::Context, id: u64, picture: Option<&[u8]>) -> Option<egui::TextureId> {
        let Some(bytes) = picture else {
            *self = Self::default();
            return None;
        };
        let key = (id, hash(bytes));
        if self.key != Some(key) {
            *self = Self { key: Some(key), ..Self::default() };
            let (tx, rx) = channel();
            let data = bytes.to_vec();
            let work = move || {
                let _ = tx.send(decode(&data));
            };
            if cfg!(target_arch = "wasm32") {
                work();
            } else if std::thread::Builder::new().name("contact-picture".into()).spawn(work).is_err() {
                return None;
            }
            self.waiting = Some(rx);
        }
        if let Some(rx) = &self.waiting {
            match rx.try_recv() {
                Ok(image) => {
                    self.texture = image.map(|i| ctx.load_texture(format!("contact-{id}"), i, egui::TextureOptions::LINEAR));
                    self.waiting = None;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(50)),
                Err(TryRecvError::Disconnected) => self.waiting = None,
            }
        }
        self.texture.as_ref().map(TextureHandle::id)
    }
}

/// A round face: the picture when there is one, else initials on the name's colour.
fn paint_face(p: &Painter, center: egui::Pos2, radius: f32, name: &str, texture: Option<egui::TextureId>, t: &Tokens) {
    match texture {
        Some(tex) => {
            let r = Rect::from_center_size(center, vec2(radius * 2.0, radius * 2.0));
            p.add(egui::epaint::RectShape::filled(r, radius, Color32::WHITE).with_texture(tex, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))));
        }
        None => {
            p.circle_filled(center, radius, theme::avatar_color(name));
            p.text(center, Align2::CENTER_CENTER, initials(name), bold((radius * 0.75).clamp(10.0, 30.0)), t.on_accent);
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct AddressForm {
    pub street: String,
    pub city: String,
    pub state: String,
    pub postal_code: String,
    pub country: String,
}

impl AddressForm {
    fn of(a: Option<&Address>) -> Self {
        a.map(|a| Self {
            street: a.street.clone(),
            city: a.city.clone(),
            state: a.state.clone(),
            postal_code: a.postal_code.clone(),
            country: a.country.clone(),
        })
        .unwrap_or_default()
    }

    fn param(&self) -> Value {
        if [&self.street, &self.city, &self.state, &self.postal_code, &self.country].iter().all(|f| f.trim().is_empty()) {
            return Value::Null;
        }
        json!({ "street": self.street, "city": self.city, "state": self.state, "postalCode": self.postal_code, "country": self.country })
    }
}

/// The contact window: a contact being made or changed.
#[derive(Clone, Default)]
pub(crate) struct ContactForm {
    pub id: Option<u64>,
    pub full_name: String,
    /// The name as it was, so the parts are worked out again only when it is changed.
    full_name_was: String,
    pub company: String,
    pub job_title: String,
    pub department: String,
    pub emails: [String; 3],
    /// What a message to each address shows, kept while the address is not changed.
    email_names: [(String, String); 3],
    pub phones: [String; 5],
    pub addresses: [AddressForm; 2],
    pub mailing: Option<Place>,
    pub web_page: String,
    pub im: String,
    pub birthday: String,
    pub anniversary: String,
    pub categories: String,
    pub notes: String,
    pub said: Option<String>,
    focus: bool,
}

impl ContactForm {
    pub(crate) fn new() -> Self {
        Self { focus: true, ..Self::default() }
    }

    pub(crate) fn of(id: u64, c: &Contact) -> Self {
        let name = c.display_name();
        Self {
            id: Some(id),
            full_name: name.clone(),
            full_name_was: name,
            company: c.company.clone(),
            job_title: c.job_title.clone(),
            department: c.department.clone(),
            emails: std::array::from_fn(|i| c.emails.get(i).map(|e| e.address.clone()).unwrap_or_default()),
            email_names: std::array::from_fn(|i| c.emails.get(i).map(|e| (e.address.clone(), e.name.clone())).unwrap_or_default()),
            phones: PHONES.map(|k| c.phone(k).unwrap_or_default().to_string()),
            addresses: PLACES.map(|at| AddressForm::of(c.address(at))),
            mailing: c.mailing,
            web_page: c.web_page.clone(),
            im: c.im.clone(),
            birthday: day_text(c.birthday),
            anniversary: day_text(c.anniversary),
            categories: c.categories.join(", "),
            notes: c.notes.clone(),
            said: None,
            focus: true,
        }
    }

    /// What `people.new` or `people.change` is given.
    pub(crate) fn params(&self) -> Value {
        let mut p = json!({
            "company": self.company,
            "jobTitle": self.job_title,
            "department": self.department,
            "emails": self.emails.iter().map(|e| e.trim()).filter(|e| !e.is_empty()).map(|e| {
                let name = self.email_names.iter().find(|(a, _)| a.eq_ignore_ascii_case(e)).map(|(_, n)| n.as_str()).unwrap_or("");
                json!({ "address": e, "name": name })
            }).collect::<Vec<_>>(),
            "phones": PHONES.iter().zip(&self.phones).map(|(k, n)| (k.name().to_string(), if n.trim().is_empty() { Value::Null } else { json!(n) })).collect::<serde_json::Map<_, _>>(),
            "addresses": PLACES.iter().zip(&self.addresses).map(|(at, a)| (at.name().to_string(), a.param())).collect::<serde_json::Map<_, _>>(),
            "webPage": self.web_page,
            "im": self.im,
            "birthday": if self.birthday.trim().is_empty() { Value::Null } else { json!(self.birthday.trim()) },
            "anniversary": if self.anniversary.trim().is_empty() { Value::Null } else { json!(self.anniversary.trim()) },
            "categories": self.categories.split([',', ';']).map(str::trim).filter(|c| !c.is_empty()).collect::<Vec<_>>(),
            "notes": self.notes,
        });
        // The place mail goes to, only while there is an address there.
        let mailing =
            self.mailing.filter(|at| PLACES.iter().position(|p| p == at).and_then(|i| self.addresses.get(i)).is_some_and(|a| !a.param().is_null()));
        p["mailing"] = mailing.map_or(Value::Null, |m| json!(m.name()));
        let name = self.full_name.trim();
        if self.id.is_none() || name != self.full_name_was.trim() {
            // "Surname, Given" or "Given Middle Surname", as a name is written.
            let (given, surname) = match name.split_once(',') {
                Some((surname, given)) => (given.trim().to_string(), surname.trim().to_string()),
                None => match name.rsplit_once(' ') {
                    Some((given, surname)) => (given.trim().to_string(), surname.trim().to_string()),
                    None => (name.to_string(), String::new()),
                },
            };
            p["given"] = json!(given);
            p["surname"] = json!(surname);
            p["middle"] = json!("");
            p["fullName"] = json!(if name.contains(',') { format!("{given} {surname}").trim().to_string() } else { name.to_string() });
            p["fileAs"] = json!("");
        }
        if let Some(id) = self.id {
            p["id"] = json!(id);
        }
        p
    }
}

/// The contact group window.
#[derive(Clone, Default)]
pub(crate) struct GroupForm {
    pub id: Option<u64>,
    pub name: String,
    /// One member a line, "Name <address>" or an address.
    pub members: String,
    pub notes: String,
    pub said: Option<String>,
    focus: bool,
}

impl GroupForm {
    pub(crate) fn new() -> Self {
        Self { focus: true, ..Self::default() }
    }

    pub(crate) fn of(id: u64, g: &Group) -> Self {
        let members = g
            .members
            .iter()
            .map(|m| {
                let name = m.name.replace(['"', '\\', '\r', '\n'], "");
                if name.trim().is_empty() || name.eq_ignore_ascii_case(&m.address) {
                    m.address.clone()
                } else {
                    format!("\"{name}\" <{}>", m.address)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        Self { id: Some(id), name: g.name.clone(), members, notes: g.notes.clone(), said: None, focus: true }
    }

    pub(crate) fn params(&self) -> Value {
        let mut p = json!({
            "name": self.name,
            "members": self.members.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>(),
            "notes": self.notes,
        });
        if let Some(id) = self.id {
            p["id"] = json!(id);
        }
        p
    }
}

/// Open vCard File: what a file holds, shown before anything is added.
#[derive(Default)]
pub(crate) struct CardsWindow {
    pub source: Value,
    pub preview: Value,
    pub chosen: BTreeSet<usize>,
    pub said: Option<String>,
}

fn cards(preview: &Value) -> impl Iterator<Item = &Value> {
    preview["cards"].as_array().into_iter().flatten()
}

fn text(v: &Value) -> String {
    one_line(v.as_str().unwrap_or_default())
}

enum Ask {
    Close,
    Save,
    Delete,
}

impl MailUi {
    /// `ui.contact`: the contact window on a new contact, or on the one named.
    pub(crate) fn contact_command(&mut self, params: &Value) -> Result<Value, String> {
        match params.get("id").and_then(Value::as_u64) {
            Some(id) => match self.session.person(id).map_err(|e| e.to_string())? {
                Card::Contact(c) => {
                    self.contact = Some(ContactForm::of(id, &c));
                    self.act("people.select", json!({ "id": id }));
                }
                Card::Group(g) => {
                    self.group = Some(GroupForm::of(id, &g));
                    self.act("people.select", json!({ "id": id }));
                }
            },
            None if self.session.is_enabled("people.new") => self.contact = Some(ContactForm::new()),
            None => return Err("No data file open has a Contacts folder that can be written".to_string()),
        }
        Ok(Value::Null)
    }

    /// `ui.contactGroup`: the group window on a new group.
    pub(crate) fn group_command(&mut self) -> Result<Value, String> {
        if !self.session.is_enabled("people.newGroup") {
            return Err("No data file open has a Contacts folder that can be written".to_string());
        }
        self.group = Some(GroupForm::new());
        Ok(Value::Null)
    }

    /// The People module's whole body.
    pub(crate) fn people_body(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        ui.painter_at(rect).rect_filled(rect, 0.0, t.surface);
        let mut left = rect.left();
        if self.session.folder_pane {
            let w = self.folder_w.clamp(200.0, 280.0).min(rect.width() * 0.4);
            self.people_side(ui, Rect::from_min_size(rect.min, vec2(w, rect.height())));
            left += w;
            ui.painter_at(rect).vline(left + 0.5, rect.y_range(), (1.0, t.line));
        }
        let area = Rect::from_min_max(pos2(left + 1.0, rect.top()), rect.max);
        let head = Rect::from_min_size(area.min, vec2(area.width(), 44.0));
        self.people_head(ui, head);
        let rest = Rect::from_min_max(pos2(area.left(), head.bottom()), area.max);
        if self.session.people.folder.is_none() {
            let p = ui.painter_at(rest);
            icons::paint(&p, Rect::from_center_size(rest.center() - vec2(0.0, 30.0), vec2(48.0, 48.0)), Icon::People, t.disabled);
            p.text(rest.center() + vec2(0.0, 16.0), Align2::CENTER_CENTER, "No data file open has a Contacts folder", font(14.0), t.text_dim);
            return;
        }
        if self.session.people.items.is_empty() {
            let p = ui.painter_at(rest);
            let words = if self.session.people.query.is_empty() { "There are no contacts here yet" } else { "Nothing was found" };
            p.text(rest.center(), Align2::CENTER_CENTER, words, font(14.0), t.text_dim);
            return;
        }
        match self.session.people.view {
            View::People => {
                let w = (rest.width() * 0.38).clamp(220.0, 340.0);
                let list = Rect::from_min_size(rest.min, vec2(w, rest.height()));
                self.people_list(ui, list);
                ui.painter_at(rest).vline(list.right() + 0.5, rest.y_range(), (1.0, t.line));
                self.person_card(ui, Rect::from_min_max(pos2(list.right() + 1.0, rest.top()), rest.max));
            }
            View::BusinessCard | View::Card => self.card_grid(ui, rest),
            View::Phone | View::List => self.people_table(ui, rest),
        }
    }

    /// My Contacts: every Contacts folder of the data files open; a click shows that one.
    fn people_side(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.pane);
        p.text(pos2(rect.left() + 14.0, rect.top() + 20.0), Align2::LEFT_CENTER, "My Contacts", bold(12.5), t.text);
        let found = self.session.run("people.folders", &Value::Null).ok();
        let folders: Vec<Value> = found.as_ref().and_then(|v| v["folders"].as_array()).cloned().unwrap_or_default();
        let files = folders.first().is_some_and(|f| folders.iter().any(|c| c["store"] != f["store"]));
        let mut y = rect.top() + 36.0;
        let mut open = None;
        for f in &folders {
            if y + 28.0 > rect.bottom() {
                break;
            }
            let r = Rect::from_min_size(pos2(rect.left() + 4.0, y), vec2(rect.width() - 8.0, 28.0));
            let resp = ui.interact(r, egui::Id::new(("people-folder", f["store"].as_u64(), f["folder"].as_u64())), Sense::click());
            if f["shown"] == true {
                p.rect_filled(r, 3.0, t.selected);
            } else if resp.hovered() {
                p.rect_filled(r, 3.0, t.hover);
            }
            icons::paint(&p, Rect::from_center_size(pos2(r.left() + 14.0, r.center().y), vec2(14.0, 14.0)), Icon::People, t.text_dim);
            let mut name = text(&f["name"]);
            if files {
                name = format!("{name} - {}", text(&f["file"]));
            }
            let count = f["items"].as_u64().unwrap_or(0).to_string();
            let cw = crate::text_width(&p, &count, font(11.5));
            clip_text(&p, pos2(r.left() + 28.0, r.center().y - 8.0), r.width() - 40.0 - cw, &name, font(12.5), t.text);
            p.text(pos2(r.right() - 8.0, r.center().y), Align2::RIGHT_CENTER, count, font(11.5), t.text_faint);
            if resp.clicked() {
                open = Some(json!({ "store": f["store"], "folder": f["folder"] }));
            }
            y += 30.0;
        }
        if folders.is_empty() {
            clip_text(&p, pos2(rect.left() + 14.0, y), rect.width() - 24.0, "No Contacts folder in the data files open", font(12.0), t.text_dim);
        }
        if let Some(params) = open {
            self.act("people.open", params);
        }
    }

    /// The search box, how many there are, and the view.
    fn people_head(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.hline(rect.x_range(), rect.bottom() - 0.5, (1.0, t.line));
        let shown = self.session.people.items.len();
        let total = self.session.people.total;
        let count = if shown == total { format!("{total}") } else { format!("{shown} of {total}") };
        let title = format!("Contacts  ·  {count}  ·  {}", self.session.people.view.label());
        clip_text(&p, pos2(rect.left() + 14.0, rect.center().y - 9.0), rect.width() * 0.45, &title, bold(14.0), t.text);
        let w = (rect.width() * 0.4).clamp(160.0, 320.0);
        let field = Rect::from_min_size(pos2(rect.right() - w - 12.0, rect.center().y - 14.0), vec2(w, 28.0));
        p.rect_filled(field, 4.0, t.pane);
        p.rect_stroke(field, 4.0, (1.0, t.line), egui::StrokeKind::Inside);
        icons::paint(&p, Rect::from_center_size(pos2(field.left() + 14.0, field.center().y), vec2(13.0, 13.0)), Icon::Search, t.text_dim);
        let inner = Rect::from_min_max(pos2(field.left() + 26.0, field.top() + 2.0), pos2(field.right() - 6.0, field.bottom() - 2.0));
        let resp = ui.put(
            inner,
            LineEdit::new(&mut self.people_query, egui::Id::new("people-search")).bare().char_limit(200).hint("Search People", Some(t.text_faint)),
        );
        if std::mem::take(&mut self.people_search_focus) {
            resp.request_focus();
        }
        if self.people_query != self.session.people.query {
            let q = self.people_query.clone();
            self.act("people.search", json!({ "query": q }));
        }
    }

    /// The People view's list of names.
    fn people_list(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let row = 52.0;
        let mut chosen = None;
        let mut opened = None;
        let scroll_to = std::mem::take(&mut self.people_scroll);
        let selected = self.session.people.selected;
        let items = &self.session.people.items;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        egui::ScrollArea::vertical().id_salt("people-list").auto_shrink([false, false]).show_rows(&mut child, row, items.len(), |ui, range| {
            for i in range {
                let Some((id, card)) = items.get(i) else { continue };
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row), Sense::click());
                let p = ui.painter();
                let on = selected == Some(*id);
                if on {
                    p.rect_filled(r, 0.0, t.selected);
                    p.rect_filled(Rect::from_min_size(r.min, vec2(3.0, r.height())), 0.0, t.accent);
                } else if resp.hovered() {
                    p.rect_filled(r, 0.0, t.hover);
                }
                let name = label_of(card);
                match card {
                    Card::Group(_) => {
                        p.circle_filled(pos2(r.left() + 30.0, r.center().y), 18.0, t.accent_soft);
                        icons::paint(p, Rect::from_center_size(pos2(r.left() + 30.0, r.center().y), vec2(18.0, 18.0)), Icon::People, t.accent);
                    }
                    Card::Contact(_) => paint_face(p, pos2(r.left() + 30.0, r.center().y), 18.0, &name, None, &t),
                }
                let x = r.left() + 58.0;
                let w = r.right() - x - 10.0;
                clip_text(p, pos2(x, r.top() + 8.0), w, &name, bold(13.0), t.text);
                clip_text(p, pos2(x, r.top() + 28.0), w, &second_line(card), font(12.0), t.text_dim);
                p.hline(r.x_range(), r.bottom() - 0.5, (1.0, t.line));
                if on && scroll_to {
                    resp.scroll_to_me(None);
                }
                if resp.clicked() {
                    chosen = Some(*id);
                }
                if resp.double_clicked() {
                    opened = Some(*id);
                }
            }
        });
        self.after_click(chosen, opened);
    }

    fn after_click(&mut self, chosen: Option<u64>, opened: Option<u64>) {
        if let Some(id) = chosen.filter(|id| self.session.people.selected != Some(*id)) {
            self.act("people.select", json!({ "id": id }));
        }
        if let Some(id) = opened {
            self.act("ui.contact", json!({ "id": id }));
        }
    }

    /// The chosen contact or group, as a card beside the list.
    fn person_card(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let ctx = ui.ctx().clone();
        let Some(id) = self.session.people.selected else {
            ui.painter_at(rect).text(rect.center(), Align2::CENTER_CENTER, "Choose someone to see what is known of them", font(13.0), t.text_dim);
            return;
        };
        let Some(card) = self.session.people.items.iter().find(|(i, _)| *i == id).map(|(_, c)| c.clone()) else { return };
        let picture = match &card {
            Card::Contact(c) => c.picture.as_deref(),
            Card::Group(_) => None,
        };
        let face = self.people_face.of(&ctx, id, picture);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(24.0, 16.0))));
        let mut act: Option<(&str, Value)> = None;
        egui::ScrollArea::vertical().id_salt(("person-card", id)).auto_shrink([false, false]).show(&mut child, |ui| {
            let name = label_of(&card);
            let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), Sense::hover());
            let p = ui.painter();
            match &card {
                Card::Contact(_) => paint_face(p, pos2(head.left() + 40.0, head.center().y), 40.0, &name, face, &t),
                Card::Group(_) => {
                    p.circle_filled(pos2(head.left() + 40.0, head.center().y), 40.0, t.accent_soft);
                    icons::paint(p, Rect::from_center_size(pos2(head.left() + 40.0, head.center().y), vec2(36.0, 36.0)), Icon::People, t.accent);
                }
            }
            clip_text(p, pos2(head.left() + 98.0, head.center().y - 24.0), head.width() - 100.0, &name, bold(20.0), t.text);
            clip_text(p, pos2(head.left() + 98.0, head.center().y + 6.0), head.width() - 100.0, &second_line(&card), font(13.0), t.text_dim);
            ui.horizontal(|ui| {
                let can_mail = match &card {
                    Card::Contact(c) => !c.emails.is_empty(),
                    Card::Group(g) => !g.members.is_empty(),
                };
                if ui.add_enabled(can_mail, dialog::primary(&t, "Email")).clicked() {
                    act = Some(("people.email", json!({ "id": id })));
                }
                if ui.add(dialog::plain("Edit")).clicked() {
                    act = Some(("ui.contact", json!({ "id": id })));
                }
                if ui.add(dialog::plain("Forward")).on_hover_text("A new message with this as a vCard").clicked() {
                    act = Some(("people.forward", json!({ "id": id })));
                }
            });
            ui.add_space(12.0);
            let row = |ui: &mut Ui, label: &str, value: &str| {
                if value.trim().is_empty() {
                    return;
                }
                ui.horizontal_top(|ui| {
                    ui.add_sized(vec2(110.0, 18.0), egui::Label::new(egui::RichText::new(label).font(font(12.0)).color(t.text_dim)));
                    ui.add(egui::Label::new(crate::seen_line(value, font(13.0), t.text)).wrap());
                });
            };
            match &card {
                Card::Contact(c) => {
                    section(ui, &t, "Contact");
                    for (n, e) in c.emails.iter().enumerate() {
                        let label = if c.emails.len() > 1 { format!("E-mail {}", n + 1) } else { "E-mail".to_string() };
                        row(ui, &label, &e.address);
                    }
                    for (kind, number) in &c.phones {
                        row(ui, kind.label(), number);
                    }
                    row(ui, "IM", &c.im);
                    row(ui, "Web page", &c.web_page);
                    if c.addresses.iter().any(|(_, a)| !a.is_empty()) {
                        section(ui, &t, "Addresses");
                        for (at, a) in &c.addresses {
                            let label = if c.mailing == Some(*at) { format!("{} (mail)", at.label()) } else { at.label().to_string() };
                            ui.horizontal_top(|ui| {
                                ui.add_sized(vec2(110.0, 18.0), egui::Label::new(egui::RichText::new(label).font(font(12.0)).color(t.text_dim)));
                                ui.vertical(|ui| {
                                    for line in a.lines().lines() {
                                        ui.label(crate::seen_line(line, font(13.0), t.text));
                                    }
                                });
                            });
                        }
                    }
                    let work =
                        !(c.company.is_empty() && c.department.is_empty() && c.office.is_empty() && c.manager.is_empty() && c.assistant.is_empty());
                    if work {
                        section(ui, &t, "Work");
                        row(ui, "Company", &c.company);
                        row(ui, "Department", &c.department);
                        row(ui, "Office", &c.office);
                        row(ui, "Manager", &c.manager);
                        row(ui, "Assistant", &c.assistant);
                    }
                    if c.birthday.is_some() || c.anniversary.is_some() || !c.spouse.is_empty() || !c.nickname.is_empty() {
                        section(ui, &t, "Personal");
                        row(ui, "Nickname", &c.nickname);
                        row(ui, "Birthday", &day_text(c.birthday));
                        row(ui, "Anniversary", &day_text(c.anniversary));
                        row(ui, "Spouse/Partner", &c.spouse);
                    }
                    if !c.categories.is_empty() {
                        section(ui, &t, "Categories");
                        ui.label(egui::RichText::new(one_line(&c.categories.join(", "))).font(font(13.0)).color(t.text));
                    }
                    if !c.notes.trim().is_empty() {
                        section(ui, &t, "Notes");
                        ui.add(
                            egui::Label::new(egui::RichText::new(c.notes.chars().take(20_000).collect::<String>()).font(font(13.0)).color(t.text))
                                .wrap(),
                        );
                    }
                }
                Card::Group(g) => {
                    section(ui, &t, "Members");
                    for m in g.members.iter().take(2_000) {
                        let line = if m.name.is_empty() || m.name == m.address {
                            m.address.clone()
                        } else {
                            format!("{}  <{}>", one_line(&m.name), m.address)
                        };
                        ui.label(crate::seen_line(&line, font(13.0), t.text));
                    }
                    if g.members.len() > 2_000 {
                        ui.label(egui::RichText::new(format!("and {} more", g.members.len() - 2_000)).font(font(12.0)).color(t.text_dim));
                    }
                    if !g.notes.trim().is_empty() {
                        section(ui, &t, "Notes");
                        ui.add(
                            egui::Label::new(egui::RichText::new(g.notes.chars().take(20_000).collect::<String>()).font(font(13.0)).color(t.text))
                                .wrap(),
                        );
                    }
                }
            }
        });
        if let Some((id, params)) = act {
            self.act(id, params);
        }
    }

    /// Business Card and Card views: a card for each, row after row.
    fn card_grid(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let business = self.session.people.view == View::BusinessCard;
        let (cw, ch) = if business { (280.0, 150.0) } else { (240.0, 170.0) };
        let gap = 12.0;
        let inner = rect.shrink(gap);
        let per_row = ((inner.width() + gap) / (cw + gap)).floor().max(1.0) as usize;
        let items = &self.session.people.items;
        let rows = items.len().div_ceil(per_row);
        let selected = self.session.people.selected;
        let scroll_to = std::mem::take(&mut self.people_scroll);
        let (mut chosen, mut opened) = (None, None);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        egui::ScrollArea::vertical().id_salt("people-cards").auto_shrink([false, false]).show_rows(&mut child, ch + gap, rows, |ui, range| {
            for r in range {
                let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), ch + gap), Sense::hover());
                for c in 0..per_row {
                    let Some((id, card)) = items.get(r * per_row + c) else { break };
                    let at = Rect::from_min_size(pos2(line.left() + c as f32 * (cw + gap), line.top()), vec2(cw, ch));
                    let resp = ui.interact(at, egui::Id::new(("people-card", *id)), Sense::click());
                    let p = ui.painter_at(at.expand(2.0));
                    let on = selected == Some(*id);
                    p.rect_filled(at, 4.0, if resp.hovered() && !on { t.hover } else { t.pane });
                    p.rect_stroke(at, 4.0, (if on { 2.0 } else { 1.0 }, if on { t.accent } else { t.line }), egui::StrokeKind::Inside);
                    if business {
                        business_card(&p, at, card, &t);
                    } else {
                        plain_card(&p, at, card, &t);
                    }
                    if on && scroll_to {
                        resp.scroll_to_me(None);
                    }
                    if resp.clicked() {
                        chosen = Some(*id);
                    }
                    if resp.double_clicked() {
                        opened = Some(*id);
                    }
                }
            }
        });
        self.after_click(chosen, opened);
    }

    /// Phone and List views: a row for each, in columns.
    fn people_table(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let phone = self.session.people.view == View::Phone;
        let columns: &[(&str, f32)] = if phone {
            &[("Full Name", 0.24), ("Company", 0.2), ("File As", 0.2), ("Business Phone", 0.12), ("Mobile Phone", 0.12), ("Home Phone", 0.12)]
        } else {
            &[("Full Name", 0.22), ("Job Title", 0.16), ("Company", 0.18), ("E-mail", 0.28), ("Categories", 0.16)]
        };
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), 26.0));
        let p = ui.painter_at(head);
        p.rect_filled(head, 0.0, t.pane);
        p.hline(head.x_range(), head.bottom() - 0.5, (1.0, t.line));
        let mut x = head.left() + 12.0;
        for (name, share) in columns {
            clip_text(&p, pos2(x, head.center().y - 7.0), rect.width() * share - 10.0, name, bold(12.0), t.text_dim);
            x += rect.width() * share;
        }
        let body = Rect::from_min_max(pos2(rect.left(), head.bottom()), rect.max);
        let row_h = 26.0;
        let items = &self.session.people.items;
        let selected = self.session.people.selected;
        let scroll_to = std::mem::take(&mut self.people_scroll);
        let (mut chosen, mut opened) = (None, None);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
        egui::ScrollArea::vertical().id_salt("people-table").auto_shrink([false, false]).show_rows(&mut child, row_h, items.len(), |ui, range| {
            for i in range {
                let Some((id, card)) = items.get(i) else { continue };
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                let p = ui.painter();
                if selected == Some(*id) {
                    p.rect_filled(r, 0.0, t.selected);
                } else if resp.hovered() {
                    p.rect_filled(r, 0.0, t.hover);
                }
                let cells: Vec<String> = match card {
                    Card::Contact(c) if phone => vec![
                        c.display_name(),
                        c.company.clone(),
                        c.filed_as(),
                        c.phone(Phone::Business).unwrap_or_default().to_string(),
                        c.phone(Phone::Mobile).unwrap_or_default().to_string(),
                        c.phone(Phone::Home).unwrap_or_default().to_string(),
                    ],
                    Card::Contact(c) => vec![
                        c.display_name(),
                        c.job_title.clone(),
                        c.company.clone(),
                        c.emails.first().map(|e| e.address.clone()).unwrap_or_default(),
                        c.categories.join(", "),
                    ],
                    Card::Group(g) if phone => vec![g.name.clone(), String::new(), g.name.clone()],
                    Card::Group(g) => vec![g.name.clone(), "Contact group".to_string(), String::new(), second_line(card), g.categories.join(", ")],
                };
                let mut x = r.left() + 12.0;
                for ((_, share), cell) in columns.iter().zip(cells) {
                    clip_text(p, pos2(x, r.center().y - 7.5), r.width() * share - 10.0, &one_line(&cell), font(12.5), t.text);
                    x += r.width() * share;
                }
                p.hline(r.x_range(), r.bottom() - 0.5, (1.0, t.line));
                if selected == Some(*id) && scroll_to {
                    resp.scroll_to_me(None);
                }
                if resp.clicked() {
                    chosen = Some(*id);
                }
                if resp.double_clicked() {
                    opened = Some(*id);
                }
            }
        });
        self.after_click(chosen, opened);
    }

    /// The one chosen moved `by` places in the order shown, as the arrow keys do.
    pub(crate) fn people_step(&mut self, by: isize) {
        let items = &self.session.people.items;
        if items.is_empty() {
            return;
        }
        let at = self.session.people.selected.and_then(|id| items.iter().position(|(i, _)| *i == id));
        let next = match at {
            Some(i) => i.saturating_add_signed(by).min(items.len() - 1),
            None => 0,
        };
        if let Some((id, _)) = items.get(next) {
            let id = *id;
            self.people_scroll = true;
            self.act("people.select", json!({ "id": id }));
        }
    }

    /// The contact window, while it is open.
    pub(crate) fn contact_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.contact.take() else { return };
        let t = self.tokens.clone();
        let mut ask = None;
        let title = if form.id.is_some() {
            let name = one_line(&form.full_name_was);
            if name.is_empty() { "Contact".to_string() } else { format!("{name} - Contact") }
        } else {
            "Untitled - Contact".to_string()
        };
        let room = (ctx.content_rect().height() - 300.0).clamp(160.0, 600.0);
        let shown = Dialog::new("contact", &title, Icon::People).width(720.0).show(ctx, &t, |ui| {
            egui::ScrollArea::vertical().id_salt("contact-form").max_height(room).show(ui, |ui| {
                ui.columns(2, |cols| {
                    let [left, right] = cols else { return };
                    egui::Grid::new("contact-left").num_columns(2).spacing(vec2(10.0, 6.0)).show(left, |ui| {
                        let field = |ui: &mut Ui, label: &str, value: &mut String, id: &str, hint: &str| {
                            ui.label(egui::RichText::new(label).color(t.text_dim));
                            let r =
                                ui.add(LineEdit::new(value, egui::Id::new(("contact", id))).desired_width(230.0).char_limit(500).hint(hint, None));
                            ui.end_row();
                            r
                        };
                        let name = field(ui, "Full Name", &mut form.full_name, "name", "Given Surname");
                        if std::mem::take(&mut form.focus) {
                            name.request_focus();
                        }
                        field(ui, "Company", &mut form.company, "company", "");
                        field(ui, "Job Title", &mut form.job_title, "title", "");
                        field(ui, "Department", &mut form.department, "department", "");
                        for (n, e) in form.emails.iter_mut().enumerate() {
                            let label = if n == 0 { "E-mail".to_string() } else { format!("E-mail {}", n + 1) };
                            field(ui, &label, e, &format!("email-{n}"), "name@example.org");
                        }
                        field(ui, "Web Page", &mut form.web_page, "web", "https://");
                        field(ui, "IM Address", &mut form.im, "im", "");
                        for (kind, number) in PHONES.iter().zip(form.phones.iter_mut()) {
                            let label = if *kind == Phone::BusinessFax { kind.label().to_string() } else { format!("{} Phone", kind.label()) };
                            field(ui, &label, number, kind.name(), "");
                        }
                        field(ui, "Birthday", &mut form.birthday, "birthday", "YYYY-MM-DD");
                        field(ui, "Anniversary", &mut form.anniversary, "anniversary", "YYYY-MM-DD");
                        field(ui, "Categories", &mut form.categories, "categories", "Friends, Work");
                    });
                    for (n, (at, a)) in PLACES.iter().zip(form.addresses.iter_mut()).enumerate() {
                        right.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("{} Address", at.label())).font(bold(12.5)).color(t.text));
                            let mut mail = form.mailing == Some(*at);
                            if ui.checkbox(&mut mail, "Mail goes here").changed() {
                                form.mailing = if mail { Some(*at) } else { None };
                            }
                        });
                        egui::Grid::new(("contact-address", n)).num_columns(2).spacing(vec2(10.0, 6.0)).show(right, |ui| {
                            for (label, value, key) in [
                                ("Street", &mut a.street, "street"),
                                ("City", &mut a.city, "city"),
                                ("State/Province", &mut a.state, "state"),
                                ("ZIP/Postal Code", &mut a.postal_code, "postal"),
                                ("Country/Region", &mut a.country, "country"),
                            ] {
                                ui.label(egui::RichText::new(label).color(t.text_dim));
                                ui.add(LineEdit::new(value, egui::Id::new(("contact-address", n, key))).desired_width(200.0).char_limit(500));
                                ui.end_row();
                            }
                        });
                        right.add_space(8.0);
                    }
                    right.label(egui::RichText::new("Notes").font(bold(12.5)).color(t.text));
                    right.add(egui::TextEdit::multiline(&mut form.notes).desired_rows(6).desired_width(f32::INFINITY).char_limit(200_000));
                });
            });
            if let Some(said) = &form.said {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(one_line(said)).font(font(12.5)).color(t.danger));
            }
            dialog::foot(ui, &t, |ui| {
                if form.id.is_some() && ui.add(dialog::danger(&t, "Delete")).on_hover_text("Moves it to Deleted Items").clicked() {
                    ask = Some(Ask::Delete);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(dialog::plain("Cancel")).clicked() {
                        ask = Some(Ask::Close);
                    }
                    if ui.add(dialog::primary(&t, "Save & Close")).clicked() {
                        ask = Some(Ask::Save);
                    }
                });
            });
        });
        if shown.closed || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ask = Some(Ask::Close);
        }
        let result = match ask {
            None => {
                self.contact = Some(form);
                return;
            }
            Some(Ask::Close) => return,
            Some(Ask::Save) => self.execute(if form.id.is_some() { "people.change" } else { "people.new" }, &form.params()),
            Some(Ask::Delete) => self.execute("people.delete", &json!({ "id": form.id })),
        };
        match result {
            Ok(_) => self.people_scroll = true,
            Err(e) => {
                form.said = Some(e);
                self.contact = Some(form);
            }
        }
    }

    /// The contact group window, while it is open.
    pub(crate) fn group_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.group.take() else { return };
        let t = self.tokens.clone();
        let mut ask = None;
        let title = if form.id.is_some() { "Contact Group" } else { "Untitled - Contact Group" };
        let shown = Dialog::new("contact-group", title, Icon::People).width(520.0).show(ctx, &t, |ui| {
            egui::Grid::new("group-form").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                ui.label(egui::RichText::new("Name").color(t.text_dim));
                let name = ui.add(LineEdit::new(&mut form.name, egui::Id::new("group-name")).desired_width(380.0).char_limit(255));
                if std::mem::take(&mut form.focus) {
                    name.request_focus();
                }
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Members, one a line: an address, or Name <address>").font(font(12.0)).color(t.text_dim));
            ui.add(egui::TextEdit::multiline(&mut form.members).desired_rows(8).desired_width(f32::INFINITY).char_limit(1_000_000));
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Notes").font(font(12.0)).color(t.text_dim));
            ui.add(egui::TextEdit::multiline(&mut form.notes).desired_rows(3).desired_width(f32::INFINITY).char_limit(200_000));
            if let Some(said) = &form.said {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(one_line(said)).font(font(12.5)).color(t.danger));
            }
            dialog::foot(ui, &t, |ui| {
                if form.id.is_some()
                    && ui.add(dialog::danger(&t, "Delete Group")).on_hover_text("Moves it to Deleted Items; its members stay").clicked()
                {
                    ask = Some(Ask::Delete);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(dialog::plain("Cancel")).clicked() {
                        ask = Some(Ask::Close);
                    }
                    if ui.add(dialog::primary(&t, "Save & Close")).clicked() {
                        ask = Some(Ask::Save);
                    }
                });
            });
        });
        if shown.closed || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ask = Some(Ask::Close);
        }
        let result = match ask {
            None => {
                self.group = Some(form);
                return;
            }
            Some(Ask::Close) => return,
            Some(Ask::Save) => self.execute(if form.id.is_some() { "people.changeGroup" } else { "people.newGroup" }, &form.params()),
            Some(Ask::Delete) => self.execute("people.delete", &json!({ "id": form.id })),
        };
        if let Err(e) = result {
            form.said = Some(e);
            self.group = Some(form);
        }
    }

    /// `ui.openCards`: open the window on a vCard file, add what is chosen, or close it.
    pub(crate) fn cards_command(&mut self, params: &Value) -> Result<Value, String> {
        if params.get("close").and_then(Value::as_bool) == Some(true) {
            self.cards_file = None;
            return Ok(Value::Null);
        }
        if params.get("import").and_then(Value::as_bool) == Some(true) {
            let Some(mut window) = self.cards_file.take() else { return Err("Open vCard File is not open".to_string()) };
            if let Some(only) = params.get("only").and_then(Value::as_array) {
                window.chosen = only.iter().filter_map(Value::as_u64).filter_map(|n| usize::try_from(n).ok()).collect();
            }
            return match self.add_cards(&window) {
                Ok(v) => Ok(v),
                Err(e) => {
                    window.said = Some(e.clone());
                    self.cards_file = Some(window);
                    Err(e)
                }
            };
        }
        let source = match (params.get("path").and_then(Value::as_str), params.get("attachment").and_then(Value::as_u64)) {
            (Some(path), None) => json!({ "path": path }),
            (None, Some(index)) => json!({ "attachment": index }),
            _ => return Err("\"path\" or \"attachment\" is required".to_string()),
        };
        let mut asked = source.clone();
        asked["preview"] = json!(true);
        let preview = self.execute("people.import", &asked)?;
        let chosen = (0..cards(&preview).count()).collect();
        let shown = json!({ "cards": cards(&preview).count() });
        self.cards_file = Some(CardsWindow { source, preview, chosen, said: None });
        Ok(shown)
    }

    fn add_cards(&mut self, window: &CardsWindow) -> Result<Value, String> {
        if window.chosen.is_empty() {
            return Err("No card is chosen".to_string());
        }
        let mut asked = window.source.clone();
        asked["only"] = json!(window.chosen.iter().collect::<Vec<_>>());
        let done = self.execute("people.import", &asked)?;
        let status = self.session.status.clone();
        let _ = self.execute("nav.module", &json!({ "module": "people" }));
        self.session.status = status;
        Ok(done)
    }

    /// The Open vCard File window, while it is open.
    pub(crate) fn cards_file_window(&mut self, ctx: &egui::Context) {
        let Some(mut window) = self.cards_file.take() else { return };
        let t = self.tokens.clone();
        let (mut close, mut add) = (false, false);
        let total = cards(&window.preview).count();
        let room = (ctx.content_rect().height() - 300.0).clamp(160.0, 520.0);
        let shown = Dialog::new("open-cards", "Open vCard File", Icon::People).width(560.0).show(ctx, &t, |ui| {
            let from = match window.source.get("path").and_then(Value::as_str) {
                Some(path) => std::path::Path::new(path).file_name().map(|n| one_line(&n.to_string_lossy())).unwrap_or_default(),
                None => "the attachment".to_string(),
            };
            let head = if total == 1 { format!("One card in {from}.") } else { format!("{total} cards in {from}.") };
            ui.label(egui::RichText::new(head).font(font(13.0)).color(t.text));
            ui.label(
                egui::RichText::new("Chosen cards are added to your contacts. Someone you have already is brought up to date, keeping what the card does not say. Undo takes all of it back.")
                    .font(font(12.0))
                    .color(t.text_dim),
            );
            ui.add_space(8.0);
            egui::ScrollArea::vertical().id_salt("open-cards-list").max_height(room).show(ui, |ui| {
                for c in cards(&window.preview) {
                    let Some(index) = c["index"].as_u64().and_then(|n| usize::try_from(n).ok()) else { continue };
                    let seen = &c["untrusted_content"];
                    dialog::card(ui, &t, |ui| {
                        ui.horizontal_top(|ui| {
                            let mut on = window.chosen.contains(&index);
                            if ui.add(egui::Checkbox::without_text(&mut on)).changed() {
                                if on {
                                    window.chosen.insert(index)
                                } else {
                                    window.chosen.remove(&index)
                                };
                            }
                            ui.vertical(|ui| {
                                let name = text(&seen["name"]);
                                let name = if name.is_empty() { "(No name)".to_string() } else { name };
                                ui.add(egui::Label::new(crate::seen_line(&name, bold(13.5), t.text)).truncate());
                                let line = if c["kind"] == "group" {
                                    format!("Contact group, {} members", seen["members"].as_u64().unwrap_or(0))
                                } else {
                                    let emails: Vec<String> = seen["emails"].as_array().into_iter().flatten().map(text).collect();
                                    [text(&seen["company"]), emails.join(", ")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
                                };
                                if !line.is_empty() {
                                    ui.add(egui::Label::new(crate::seen_line(&line, font(12.0), t.text_dim)).truncate());
                                }
                                if c["replaces"].is_u64() {
                                    ui.label(egui::RichText::new("In your contacts already: brought up to date").font(font(12.0)).color(t.text_dim));
                                }
                            });
                        });
                    });
                    ui.add_space(4.0);
                }
                if total == 0 {
                    ui.label(egui::RichText::new("The file holds no cards.").font(font(13.0)).color(t.text));
                }
                for n in window.preview["untrusted_content"]["notes"].as_array().into_iter().flatten() {
                    ui.label(egui::RichText::new(text(n)).font(font(12.0)).color(t.text_dim));
                }
            });
            if let Some(said) = &window.said {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(one_line(said)).font(font(12.5)).color(t.danger));
            }
            dialog::foot(ui, &t, |ui| {
                if total > 1 {
                    let all = window.chosen.len() == total;
                    if ui.add(dialog::plain(if all { "Choose None" } else { "Choose All" })).clicked() {
                        window.chosen = if all { BTreeSet::new() } else { (0..total).collect() };
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let n = window.chosen.len();
                    let label = if n == 1 { "Add 1 Card".to_string() } else { format!("Add {n} Cards") };
                    if ui.add_enabled(n > 0, dialog::primary(&t, &label)).clicked() {
                        add = true;
                    }
                    if ui.add(dialog::plain("Cancel")).clicked() {
                        close = true;
                    }
                });
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
        if shown.closed || close {
            return;
        }
        if add {
            match self.add_cards(&window) {
                Ok(_) => return,
                Err(e) => window.said = Some(e),
            }
        }
        self.cards_file = Some(window);
    }
}

fn section(ui: &mut Ui, t: &Tokens, title: &str) {
    ui.add_space(10.0);
    ui.label(egui::RichText::new(title).font(bold(12.5)).color(t.accent));
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, (1.0, t.line));
    ui.add_space(4.0);
}

/// A business card: a band in the name's colour with the face, and what they do and how to reach them.
fn business_card(p: &Painter, r: Rect, card: &Card, t: &Tokens) {
    let name = label_of(card);
    let band = Rect::from_min_size(r.min, vec2(84.0, r.height()));
    let color = theme::avatar_color(&name);
    p.rect_filled(band.shrink(1.0), egui::CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 }, color.gamma_multiply(0.18));
    match card {
        Card::Contact(_) => paint_face(p, band.center(), 28.0, &name, None, t),
        Card::Group(_) => icons::paint(p, Rect::from_center_size(band.center(), vec2(32.0, 32.0)), Icon::People, color),
    }
    let x = band.right() + 10.0;
    let w = r.right() - x - 8.0;
    let mut y = r.top() + 10.0;
    clip_text(p, pos2(x, y), w, &name, bold(13.5), t.text);
    y += 22.0;
    let lines: Vec<String> = match card {
        Card::Contact(c) => {
            let mut v = vec![c.company.clone(), c.job_title.clone()];
            v.extend(c.phones.iter().take(2).map(|(k, n)| format!("{}: {n}", k.label())));
            v.extend(c.emails.first().map(|e| e.address.clone()));
            v
        }
        Card::Group(g) => {
            let mut v = vec![second_line(card)];
            v.extend(g.members.iter().take(4).map(|m| one_line(&m.name)));
            v
        }
    };
    for line in lines.into_iter().filter(|l| !l.trim().is_empty()).take(5) {
        clip_text(p, pos2(x, y), w, &one_line(&line), font(12.0), t.text_dim);
        y += 19.0;
    }
}

/// A plain card, as Outlook's Card view: the file-as name over labelled lines.
fn plain_card(p: &Painter, r: Rect, card: &Card, t: &Tokens) {
    let head = Rect::from_min_size(r.min, vec2(r.width(), 28.0));
    p.rect_filled(head.shrink(1.0), egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, t.accent_soft);
    let title = match card {
        Card::Contact(c) => c.filed_as(),
        Card::Group(g) => g.name.clone(),
    };
    clip_text(p, pos2(r.left() + 10.0, head.center().y - 8.0), r.width() - 20.0, &one_line(&title), bold(13.0), t.text);
    let lines: Vec<(String, String)> = match card {
        Card::Contact(c) => {
            let mut v: Vec<(String, String)> = Vec::new();
            if let Some(at) = c.mailing.or(c.addresses.first().map(|(p, _)| *p))
                && let Some(a) = c.address(at)
            {
                v.push((at.label().to_string(), a.lines().replace('\n', ", ")));
            }
            v.extend(c.phones.iter().take(3).map(|(k, n)| (k.label().to_string(), n.clone())));
            v.extend(c.emails.iter().take(2).map(|e| ("E-mail".to_string(), e.address.clone())));
            v
        }
        Card::Group(g) => g.members.iter().take(6).map(|m| ("Member".to_string(), m.address.clone())).collect(),
    };
    let mut y = head.bottom() + 8.0;
    for (label, value) in lines.into_iter().take(6) {
        if y + 16.0 > r.bottom() {
            break;
        }
        clip_text(p, pos2(r.left() + 10.0, y), 74.0, &label, font(11.5), t.text_faint);
        clip_text(p, pos2(r.left() + 88.0, y), r.width() - 96.0, &one_line(&value), font(12.0), t.text);
        y += 19.0;
    }
}
