//! Writing with a look: the body of a message being written and a signature, each in its own
//! fonts, sizes and colours, with pictures standing in the text, lists and alignment. The Format
//! Text bar above it, Ctrl+B, Ctrl+I and Ctrl+U, and copy and paste with the look kept.
//!
//! What is written is the engine's [`Writing`]: everything done to it is a `compose.*` command
//! (the signature editor runs the same ones on a writing of its own, `write_formatted`), so the
//! window only draws it and says what was done.
//!
//! The clipboard carries HTML and pictures as well as text. HTML pasted is read as any message
//! is, nothing it names on a server fetched; the only files it may bring in are the pictures a
//! program copying it put in the system's folder for temporary files, as Outlook and Word do
//! (`file:///…/Temp/msohtmlclip…/clip_image001.png`).

use crate::faces::Faces;
use crate::line_edit::{Look, State};
use crate::text_area::{Lay, Side, Styled, TextArea};
use crate::theme::{Tokens, contrast};
use base64::Engine as _;
use egui::{Color32, FontId, Id, Stroke, TextFormat, TextureHandle, Ui, Vec2, vec2};
use openmailclient_engine::Writing;
use openmailclient_model::format::{Align, List, Mark, Picture};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

/// Pictures kept decoded for writing in; past them the oldest are let go.
const MAX_KEPT: usize = 64;
/// The largest picture file taken from the clipboard or from the temporary folder.
const MAX_FILE: usize = 16 << 20;
/// The most pixels of a picture copied, before it is written as PNG.
const MAX_PIXELS: usize = 40_000_000;
/// The highest a picture stands in the text.
const MAX_HIGH: f32 = 1200.0;
/// The size a picture is drawn while it is being decoded, when the text gives none.
const UNKNOWN: Vec2 = vec2(96.0, 72.0);

/// Fonts offered by name: the open ones a reader without them is given, and the common ones.
const FONTS: &[&str] = &["Arial", "Calibri", "Cambria", "Courier New", "Georgia", "Segoe UI", "Tahoma", "Times New Roman", "Trebuchet MS", "Verdana"];
const SIZES: &[f32] = &[8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 36.0, 48.0, 72.0];
/// Colours offered for text and for highlighting.
const COLOURS: &[(&str, &str)] = &[
    ("Black", "#000000"),
    ("Dark Blue", "#1f3864"),
    ("Blue", "#0563c1"),
    ("Teal", "#00808a"),
    ("Green", "#2e7d32"),
    ("Dark Red", "#a4262c"),
    ("Red", "#d32f2f"),
    ("Orange", "#e36c09"),
    ("Purple", "#7030a0"),
    ("Grey", "#7f7f7f"),
];
const MARKERS: &[(&str, &str)] =
    &[("Yellow", "#ffff00"), ("Bright Green", "#00ff00"), ("Turquoise", "#00ffff"), ("Pink", "#ff00ff"), ("Light Grey", "#c0c0c0")];

/// What was done to a writing in a frame, in the order it was done.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Did {
    /// The text typed to this, the change beginning at the character given or after.
    Body(String, usize),
    /// A Format Text command, by id, with its params.
    Run(&'static str, Value),
    /// A picture file to be chosen and put in.
    PictureFile,
}

/// A picture being decoded, decoded, or that could not be.
enum Slot {
    Pending,
    /// The picture, its copies at each half of its size, and its own size.
    Ready(TextureHandle, Vec<TextureHandle>, Vec2),
    Failed,
}

/// Pictures of what is being written, decoded on a thread of their own, never on the one that
/// draws the window; until one is ready its room is drawn as an empty frame.
#[derive(Clone, Default)]
pub(crate) struct Inline(Arc<Mutex<HashMap<u64, Slot>>>);

/// What tells one picture file from another without going through all of it every frame.
fn key_of(bytes: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.len().hash(&mut hasher);
    bytes.get(..bytes.len().min(4096)).hash(&mut hasher);
    bytes.get(bytes.len().saturating_sub(4096)..).hash(&mut hasher);
    hasher.finish()
}

fn decoded(ctx: &egui::Context, bytes: &[u8]) -> Slot {
    let Some(pixels) = openmailclient_engine::pictures::decode(bytes) else { return Slot::Failed };
    let (Ok(w), Ok(h)) = (usize::try_from(pixels.width), usize::try_from(pixels.height)) else { return Slot::Failed };
    let Some((texture, smaller, _)) = crate::pictures::textures(ctx, "written-picture", [w, h], &pixels.rgba) else { return Slot::Failed };
    let natural = vec2(f32::from(pixels.natural.0), f32::from(pixels.natural.1));
    Slot::Ready(texture, smaller, natural)
}

impl Inline {
    /// The picture decoded, with its copies and its own size; `None` while it is decoded or if
    /// it cannot be.
    fn get(&self, ctx: &egui::Context, p: &Picture) -> Option<(TextureHandle, Vec<TextureHandle>, Vec2)> {
        let key = key_of(&p.bytes);
        let Ok(mut slots) = self.0.lock() else { return None };
        match slots.get(&key) {
            Some(Slot::Ready(texture, smaller, size)) => return Some((texture.clone(), smaller.clone(), *size)),
            Some(_) => return None,
            None => {}
        }
        if slots.len() >= MAX_KEPT {
            slots.retain(|_, slot| matches!(slot, Slot::Pending));
        }
        slots.insert(key, Slot::Pending);
        drop(slots);
        let (shared, bytes, ctx2) = (self.0.clone(), p.bytes.clone(), ctx.clone());
        let work = move || {
            let slot = decoded(&ctx2, &bytes);
            if let Ok(mut slots) = shared.lock() {
                slots.insert(key, slot);
            }
            ctx2.request_repaint();
        };
        // Where there are no threads, as in a browser, it is decoded here.
        let spawned = std::thread::Builder::new().name("written-picture".to_string()).spawn(work.clone());
        if spawned.is_err() {
            work();
        }
        None
    }
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Text that is drawn on `paper` in `wanted` where it can be read there, else in `ink`.
fn readable(wanted: Option<Color32>, paper: Color32, ink: Color32) -> Color32 {
    wanted.filter(|c| contrast(*c, paper) >= 2.0).unwrap_or(ink)
}

/// How text of the look `m` is drawn.
fn text_format(m: &Mark, faces: &Faces, base: &FontId, ink: Color32, paper: Color32, link: Color32) -> TextFormat {
    let px = m.size.map_or(base.size, |pt| pt * 4.0 / 3.0).clamp(6.0, 96.0);
    let named = m.family.as_deref().and_then(|f| faces.pick(f, m.bold, m.italic));
    let italics = named.as_ref().map_or(m.italic, |picked| picked.slant);
    let font_id = match (named, m.bold) {
        (Some(picked), _) => FontId::new(px, picked.family),
        (None, true) => crate::bold(px),
        (None, false) => crate::font(px),
    };
    let mut format = TextFormat { italics, font_id, ..TextFormat::default() };
    let behind = m.highlight.map(rgb);
    let on = behind.unwrap_or(paper);
    let own = if m.link.is_some() && m.color.is_none() { Some(link) } else { m.color.map(rgb) };
    let plain = if behind.is_some() && contrast(ink, on) < 2.0 { Color32::from_rgb(0x1a, 0x1a, 0x1a) } else { ink };
    format.color = readable(own, on, plain);
    format.background = behind.unwrap_or(Color32::TRANSPARENT);
    if m.underline || m.link.is_some() {
        format.underline = Stroke::new(1.0, format.color);
    }
    if m.strike {
        format.strikethrough = Stroke::new(1.0, format.color);
    }
    format
}

/// The size a picture stands at: as the text says, else its own, no wider than `room`.
fn picture_size(p: &Picture, natural: Option<Vec2>, room: f32) -> Vec2 {
    let own = natural.filter(|n| n.x >= 1.0 && n.y >= 1.0);
    let (w, h) = (f32::from(p.width), f32::from(p.height));
    let size = match (w > 0.0, h > 0.0, own) {
        (true, true, _) => vec2(w, h),
        (true, false, Some(n)) => vec2(w, w * n.y / n.x),
        (false, true, Some(n)) => vec2(h * n.x / n.y, h),
        (_, _, Some(n)) => n,
        (true, false, None) => vec2(w, w * 0.75),
        (false, true, None) => vec2(h / 0.75, h),
        (false, false, None) => UNKNOWN,
    };
    let room = room.max(16.0);
    let scale = (room / size.x).min(MAX_HIGH / size.y).min(1.0);
    vec2((size.x * scale).max(1.0), (size.y * scale).max(1.0))
}

/// How `w` is drawn in a box `room` wide, its own text in `base`.
pub(crate) fn styled(ctx: &egui::Context, faces: &mut Faces, inline: &Inline, w: &Writing, base: &FontId, t: &Tokens, room: f32) -> Styled {
    let mut f = w.format.clone();
    f.fit(&w.body);
    // The fonts it names, loaded for it to be drawn in.
    let mut asked: Vec<(Arc<str>, bool, bool)> = Vec::new();
    for (_, m) in &f.runs {
        if let Some(family) = &m.family
            && !asked.iter().any(|(a, b, i)| a == family && *b == m.bold && *i == m.italic)
            && asked.len() < 32
        {
            asked.push((family.clone(), m.bold, m.italic));
        }
    }
    if !asked.is_empty() {
        faces.want(ctx, &asked);
    }
    let (ink, paper, link) = (t.text, t.surface, if t.dark { t.accent } else { t.accent_dark });
    let mut styled = Styled { looks: vec![Look::Text(TextFormat::simple(base.clone(), ink))], ..Styled::default() };
    let mut marks: Vec<Mark> = vec![Mark::default()];
    for (n, m) in &f.runs {
        let index = match marks.iter().position(|known| known == m) {
            Some(index) => index,
            None => {
                let look = match m.picture.and_then(|id| f.pictures.iter().find(|p| p.id == id)) {
                    Some(p) => {
                        let ready = inline.get(ctx, p);
                        let size = picture_size(p, ready.as_ref().map(|(_, _, s)| *s), room - 8.0);
                        if let Some((texture, smaller, _)) = &ready {
                            let px = size * ctx.pixels_per_point();
                            styled.pictures.insert(styled.looks.len(), crate::pictures::sharpest(texture, smaller, px));
                        }
                        Look::Room(size.x, size.y)
                    }
                    None if m.picture.is_some() => Look::Room(UNKNOWN.x, UNKNOWN.y),
                    None => Look::Text(text_format(m, faces, base, ink, paper, link)),
                };
                marks.push(m.clone());
                styled.looks.push(look);
                styled.looks.len() - 1
            }
        };
        styled.of.extend(std::iter::repeat_n(index, *n));
    }
    // Each paragraph's place, an item of a numbered list counted among those before it at its
    // depth.
    let mut counts: Vec<(u8, usize)> = Vec::new();
    for para in &f.paras {
        counts.retain(|(depth, _)| *depth <= para.indent);
        let marker = match para.list {
            List::None => {
                counts.clear();
                None
            }
            List::Bullet => Some(if para.indent % 2 == 0 { "•" } else { "◦" }.to_string()),
            List::Number => {
                let n = match counts.iter_mut().find(|(depth, _)| *depth == para.indent) {
                    Some((_, n)) => {
                        *n += 1;
                        *n
                    }
                    None => {
                        counts.push((para.indent, 1));
                        1
                    }
                };
                Some(format!("{n}."))
            }
        };
        let indent = u16::from(para.indent) + u16::from(para.list != List::None);
        let align = match para.align {
            Align::Left | Align::Justify => Side::Start,
            Align::Center => Side::Center,
            Align::Right => Side::End,
        };
        styled.paras.push(Lay { indent, align, marker });
    }
    styled
}

/// How many characters `a` and `b` begin with alike.
fn alike(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

/// The text of `w` in a box with its look and the Format Text bar above it: what was done is
/// returned for the caller to run, on a message or on a writing of its own.
#[allow(clippy::too_many_arguments)]
pub(crate) fn writing_box(ui: &mut Ui, t: &Tokens, faces: &mut Faces, inline: &Inline, w: &Writing, id: Id, rows: usize, limit: usize) -> Vec<Did> {
    let mut did = Vec::new();
    bar(ui, t, w, id, &mut did);
    // Ctrl+B, Ctrl+I and Ctrl+U while the text is being written in.
    if !w.plain && ui.memory(|m| m.has_focus(id)) {
        for (key, what) in [(egui::Key::B, "bold"), (egui::Key::I, "italic"), (egui::Key::U, "underline")] {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, key)) {
                did.push(Did::Run("compose.format", json!({ what: "toggle" })));
            }
        }
    }
    // A selection the engine moved, as a paste or a picture put in does: the box takes it.
    let synced = id.with("synced");
    if ui.data(|d| d.get_temp::<(usize, usize)>(synced)) != Some(w.selection) {
        let mut state: State = ui.data(|d| d.get_temp(id)).unwrap_or_default();
        (state.anchor, state.cursor) = w.selection;
        ui.data_mut(|d| {
            d.insert_temp(id, state);
            d.insert_temp(synced, w.selection);
        });
    }
    let font = crate::font(13.5);
    let mut body = w.body.clone();
    let response = if w.plain {
        ui.add(TextArea::new(&mut body, id).font(font).bare().desired_rows(rows).char_limit(limit))
    } else {
        let look = styled(ui.ctx(), faces, inline, w, &font, t, ui.available_width());
        ui.add(TextArea::new(&mut body, id).font(font).bare().desired_rows(rows).char_limit(limit).styled(&look))
    };
    if response.changed() {
        let at = alike(&w.body, &body);
        did.push(Did::Body(body.clone(), at));
    }
    let state: State = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let now = (state.anchor, state.cursor);
    if now != w.selection {
        did.push(Did::Run("compose.select", json!({ "from": now.0, "to": now.1 })));
        ui.data_mut(|d| d.insert_temp(synced, now));
    }
    let asked = crate::text_area::asked(ui.ctx(), id);
    if let Some((from, to)) = asked.copied {
        let mut f = w.format.clone();
        f.fit(&w.body);
        let (text, piece) = f.cut(&w.body, from, to);
        let plain = piece.to_plain(&text);
        let html = openmailclient_engine::signatures::keep_html(&text, &piece);
        if !clipboard::put(&html, &plain) {
            ui.ctx().copy_text(plain);
        }
    }
    if let Some(text) = asked.paste {
        let rich = if w.plain { None } else { clipboard::take() };
        did.push(Did::Run("compose.paste", rich.unwrap_or_else(|| json!({ "text": text }))));
    }
    did
}

/// A button of the bar, pressed in while `on`.
fn press(ui: &mut Ui, text: egui::RichText, on: bool, tip: &str) -> bool {
    ui.add(egui::Button::new(text).selected(on).min_size(vec2(26.0, 24.0))).on_hover_text(tip).clicked()
}

/// What a paragraph button shows, drawn in lines of our own.
#[derive(Clone, Copy, PartialEq)]
enum Lines {
    Bullets,
    Numbers,
    Less,
    More,
    Left,
    Center,
    Right,
}

/// A paragraph button: four lines of text as the paragraph would set them.
fn lines_button(ui: &mut Ui, t: &Tokens, drawn: Lines, on: bool, tip: &str) -> bool {
    let response = ui.add(egui::Button::new("").selected(on).min_size(vec2(28.0, 24.0))).on_hover_text(tip);
    let r = egui::Rect::from_center_size(response.rect.center(), vec2(14.0, 11.0));
    let painter = ui.painter();
    let ink = Stroke::new(1.3, t.text);
    let lengths = [1.0, 0.7, 1.0, 0.55];
    for (n, wide) in lengths.iter().enumerate() {
        let y = r.top() + n as f32 * r.height() / 3.0;
        let (mut left, mut right) = (r.left(), r.left() + r.width() * wide);
        match drawn {
            Lines::Center => {
                let half = r.width() * wide / 2.0;
                (left, right) = (r.center().x - half, r.center().x + half);
            }
            Lines::Right => (left, right) = (r.right() - r.width() * wide, r.right()),
            Lines::Bullets | Lines::Numbers => {
                (left, right) = (r.left() + 5.0, r.right());
                if n % 2 == 0 && drawn == Lines::Bullets {
                    painter.circle_filled(egui::pos2(r.left() + 1.0, y), 1.4, t.text);
                }
            }
            Lines::Less | Lines::More => {
                if n == 1 || n == 2 {
                    (left, right) = (r.left() + 6.0, r.right());
                } else {
                    right = r.right();
                }
            }
            Lines::Left => {}
        }
        painter.line_segment([egui::pos2(left, y), egui::pos2(right, y)], ink);
    }
    // An arrow for the indent buttons, and numbers' first stroke.
    let mid = r.top() + r.height() / 2.0;
    match drawn {
        Lines::More => {
            painter.line_segment([egui::pos2(r.left(), mid - 2.5), egui::pos2(r.left() + 3.0, mid)], ink);
            painter.line_segment([egui::pos2(r.left() + 3.0, mid), egui::pos2(r.left(), mid + 2.5)], ink);
        }
        Lines::Less => {
            painter.line_segment([egui::pos2(r.left() + 3.0, mid - 2.5), egui::pos2(r.left(), mid)], ink);
            painter.line_segment([egui::pos2(r.left(), mid), egui::pos2(r.left() + 3.0, mid + 2.5)], ink);
        }
        Lines::Numbers => {
            painter.line_segment(
                [
                    egui::pos2(r.left() + 1.0, r.top() + r.height() * 2.0 / 3.0 - 2.0),
                    egui::pos2(r.left() + 1.0, r.top() + r.height() * 2.0 / 3.0 + 2.0),
                ],
                ink,
            );
        }
        _ => {}
    }
    response.clicked()
}

/// The Format Text bar: font, size, bold, italic, underline, struck through, colour and
/// highlight, lists, indent and alignment, a link, a picture, and HTML or plain text.
fn bar(ui: &mut Ui, t: &Tokens, w: &Writing, id: Id, did: &mut Vec<Did>) {
    let look = w.look();
    let on = |key: &str| look.get(key).and_then(Value::as_bool).unwrap_or(false);
    let said = |key: &str| look.get(key).and_then(Value::as_str).map(str::to_string);
    let run = |did: &mut Vec<Did>, command: &'static str, p: Value| did.push(Did::Run(command, p));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
        if !w.plain {
            let font = said("font");
            egui::ComboBox::from_id_salt(id.with("font"))
                .width(120.0)
                .selected_text(font.clone().unwrap_or_else(|| "(Body font)".to_string()))
                .show_ui(ui, |ui| {
                    if ui.selectable_label(font.is_none(), "(Body font)").clicked() {
                        run(did, "compose.format", json!({ "font": null }));
                    }
                    for name in FONTS {
                        if ui.selectable_label(font.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(name)), *name).clicked() {
                            run(did, "compose.format", json!({ "font": name }));
                        }
                    }
                });
            let size = look.get("size").and_then(Value::as_f64);
            let shown = size.map_or_else(|| "11".to_string(), |s| format!("{}", (s * 2.0).round() / 2.0));
            egui::ComboBox::from_id_salt(id.with("size")).width(44.0).selected_text(shown).show_ui(ui, |ui| {
                for s in SIZES {
                    if ui.selectable_label(size.is_some_and(|now| (now - f64::from(*s)).abs() < 0.01), format!("{s}")).clicked() {
                        run(did, "compose.format", json!({ "size": s }));
                    }
                }
            });
            if press(ui, egui::RichText::new("A+").font(crate::font(12.5)), false, "Grow Font") {
                run(did, "compose.format", json!({ "size": "grow" }));
            }
            if press(ui, egui::RichText::new("A−").font(crate::font(11.0)), false, "Shrink Font") {
                run(did, "compose.format", json!({ "size": "shrink" }));
            }
            ui.separator();
            for (label, key, tip) in [
                ("B", "bold", "Bold (Ctrl+B)"),
                ("I", "italic", "Italic (Ctrl+I)"),
                ("U", "underline", "Underline (Ctrl+U)"),
                ("S", "strike", "Strikethrough"),
            ] {
                let mut text = egui::RichText::new(label).font(if key == "bold" { crate::bold(13.0) } else { crate::font(13.0) });
                text = match key {
                    "italic" => text.italics(),
                    "underline" => text.underline(),
                    "strike" => text.strikethrough(),
                    _ => text,
                };
                if press(ui, text, on(key), tip) {
                    run(did, "compose.format", json!({ key: "toggle" }));
                }
            }
            for (label, key, choices, none) in [("A", "color", COLOURS, "Automatic"), ("ab", "highlight", MARKERS, "No Colour")] {
                let now = said(key);
                let tip = if key == "color" { "Font Colour" } else { "Text Highlight Colour" };
                let shown = egui::RichText::new(label)
                    .font(crate::font(12.5))
                    .color(now.as_deref().and_then(colour_of).filter(|_| key == "color").unwrap_or(t.text));
                ui.menu_button(shown, |ui| {
                    if ui.button(none).clicked() {
                        run(did, "compose.format", json!({ key: null }));
                        ui.close();
                    }
                    for (name, hex) in choices {
                        let swatch = colour_of(hex).unwrap_or(t.text);
                        let picked = ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                            ui.painter().rect(r, 2.0, swatch, Stroke::new(1.0, t.line), egui::StrokeKind::Inside);
                            ui.button(egui::RichText::new(*name).color(t.text)).on_hover_text(*hex).clicked()
                        });
                        if picked.inner {
                            run(did, "compose.format", json!({ key: hex }));
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text(tip);
            }
            ui.separator();
            let list = said("list").unwrap_or_default();
            let align = said("align").unwrap_or_default();
            for (drawn, on, tip, p) in [
                (Lines::Bullets, list == "bullet", "Bullets", json!({ "list": "bullet" })),
                (Lines::Numbers, list == "number", "Numbering", json!({ "list": "number" })),
                (Lines::Less, false, "Decrease Indent", json!({ "indent": "less" })),
                (Lines::More, false, "Increase Indent", json!({ "indent": "more" })),
                (Lines::Left, align == "left", "Align Left", json!({ "align": "left" })),
                (Lines::Center, align == "center", "Center", json!({ "align": "center" })),
                (Lines::Right, align == "right", "Align Right", json!({ "align": "right" })),
            ] {
                if lines_button(ui, t, drawn, on, tip) {
                    run(did, "compose.paragraph", p);
                }
            }
            ui.separator();
            link_menu(ui, w, id, did);
            if press(
                ui,
                egui::RichText::new("Picture…").font(crate::font(12.5)),
                false,
                "Insert a picture from a file, shown where it stands and sent in the message",
            ) {
                did.push(Did::PictureFile);
            }
            if press(ui, egui::RichText::new("Clear").font(crate::font(12.5)), false, "Clear All Formatting") {
                run(did, "compose.format", json!({ "clear": true }));
            }
            ui.separator();
        }
        for (label, format, plain) in [("HTML", "html", false), ("Plain Text", "plain", true)] {
            if ui.add(egui::Button::new(egui::RichText::new(label).font(crate::font(12.5))).selected(w.plain == plain)).clicked() && w.plain != plain
            {
                run(did, "compose.textFormat", json!({ "format": format }));
            }
        }
    });
}

fn colour_of(hex: &str) -> Option<Color32> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    let n = u32::from_str_radix(hex.get(..6)?, 16).ok()?;
    let [_, r, g, b] = n.to_be_bytes();
    Some(Color32::from_rgb(r, g, b))
}

/// Insert > Link: where the selection goes to, typed in.
fn link_menu(ui: &mut Ui, w: &Writing, id: Id, did: &mut Vec<Did>) {
    let typed = id.with("link");
    let now = w.look().get("link").and_then(Value::as_str).map(str::to_string);
    ui.menu_button(egui::RichText::new("Link…").font(crate::font(12.5)), |ui| {
        let mut address: String = ui.data(|d| d.get_temp(typed)).unwrap_or_else(|| now.clone().unwrap_or_default());
        ui.label("Address");
        ui.add(
            crate::line_edit::LineEdit::new(&mut address, typed.with("field"))
                .desired_width(260.0)
                .char_limit(2000)
                .hint("https://… or someone@example.org", None),
        );
        ui.horizontal(|ui| {
            if ui.add_enabled(!address.trim().is_empty(), egui::Button::new("Insert Link")).clicked() {
                did.push(Did::Run("compose.link", json!({ "address": address.trim() })));
                address.clear();
                ui.close();
            }
            if now.is_some() && ui.button("Remove Link").clicked() {
                did.push(Did::Run("compose.link", json!({ "address": "" })));
                ui.close();
            }
        });
        ui.data_mut(|d| d.insert_temp(typed, address));
    })
    .response
    .on_hover_text("Link (Ctrl+K)");
}

/// The system's clipboard, as HTML and pictures.
mod clipboard {
    use serde_json::Value;

    /// What the clipboard holds that has a look: HTML, or a picture; `None` for text alone,
    /// which the toolkit has already.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn take() -> Option<Value> {
        use base64::Engine as _;
        let mut clipboard = arboard::Clipboard::new().ok()?;
        if let Ok(html) = clipboard.get().html()
            && !html.trim().is_empty()
        {
            return Some(serde_json::json!({ "html": super::with_copied_pictures(&html) }));
        }
        let image = clipboard.get_image().ok()?;
        let png = super::png_of(image.width, image.height, &image.bytes)?;
        Some(serde_json::json!({ "image": base64::engine::general_purpose::STANDARD.encode(png) }))
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn take() -> Option<Value> {
        None
    }

    /// Put `html` on the clipboard with `text` beside it; false where it could not be.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn put(html: &str, text: &str) -> bool {
        arboard::Clipboard::new().and_then(|mut c| c.set().html(html, Some(text))).is_ok()
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn put(_html: &str, _text: &str) -> bool {
        false
    }
}

/// A picture copied, `rgba` four bytes a pixel, written as PNG.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn png_of(width: usize, height: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    let pixels = width.checked_mul(height)?;
    if pixels == 0 || pixels > MAX_PIXELS || rgba.len() != pixels.checked_mul(4)? {
        return None;
    }
    let image = image::RgbaImage::from_raw(u32::try_from(width).ok()?, u32::try_from(height).ok()?, rgba.to_vec())?;
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// `html` with each picture it names in the system's temporary folder written into it: how a
/// program copying formatted text hands over its pictures. Any other file it names is left
/// as it is, and so is never read.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn with_copied_pictures(html: &str) -> String {
    let Ok(temp) = std::env::temp_dir().canonicalize() else { return html.to_string() };
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let mut taken = 0usize;
    while let Some(at) = rest.find("file:///") {
        let (before, from) = rest.split_at(at);
        out.push_str(before);
        let end = from.find(['"', '\'', ' ', '>']).unwrap_or(from.len());
        let (address, after) = from.split_at(end);
        match (taken < openmailclient_model::format::MAX_PICTURES).then(|| written(address, &temp)).flatten() {
            Some(data) => {
                out.push_str(&data);
                taken += 1;
            }
            None => out.push_str(address),
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// A `file:///` address of a picture in `temp` as a `data:` one (RFC 2397).
fn written(address: &str, temp: &std::path::Path) -> Option<String> {
    let path = unescaped(address.strip_prefix("file:///")?)?;
    // On Windows the address starts at the drive; elsewhere at the root.
    let path = if cfg!(windows) { std::path::PathBuf::from(path) } else { std::path::PathBuf::from(format!("/{path}")) };
    let path = path.canonicalize().ok()?;
    if !path.starts_with(temp) || std::fs::metadata(&path).ok()?.len() > MAX_FILE as u64 {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    let kind = openmailclient_formats::editable::sniff(&bytes)?;
    Some(format!("data:{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)))
}

/// An address's `%xx` escapes undone.
fn unescaped(s: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(s.len());
    let mut it = s.bytes();
    while let Some(b) = it.next() {
        if b == b'%' {
            let hi = char::from(it.next()?).to_digit(16)?;
            let lo = char::from(it.next()?).to_digit(16)?;
            bytes.push(u8::try_from(hi * 16 + lo).ok()?);
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_copied_come_only_from_the_temporary_folder() {
        let temp = std::env::temp_dir().join(format!("omc-clip-{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap();
        let png = temp.join("clip image001.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
        let address = format!("file:///{}", png.display().to_string().replace('\\', "/").trim_start_matches('/').replace(' ', "%20"));
        let html = format!(r#"<p><img src="{address}"><img src="file:///C:/Windows/win.ini"><img src='file:///etc/passwd'></p>"#);
        let out = with_copied_pictures(&html);
        assert!(out.contains("data:image/png;base64,"), "{out}");
        assert!(out.contains("file:///C:/Windows/win.ini") && out.contains("file:///etc/passwd"), "{out}");
        let _ = std::fs::remove_dir_all(&temp);
        for junk in ["file:///", "file:///%", "file:///%zz", "<img src=\"file:///\u{0}\">"] {
            let _ = with_copied_pictures(junk);
        }
    }

    #[test]
    fn pictures_copied_are_written_as_png_within_bounds() {
        assert!(png_of(2, 1, &[255; 8]).is_some_and(|p| p.starts_with(b"\x89PNG")));
        assert!(png_of(2, 1, &[255; 7]).is_none());
        assert!(png_of(usize::MAX, 2, &[]).is_none());
        assert!(png_of(0, 0, &[]).is_none());
    }

    #[test]
    fn a_picture_is_drawn_at_its_size_within_the_room() {
        let p = |w: u16, h: u16| Picture { id: 1, bytes: Arc::from(&b""[..]), kind: "image/png".into(), name: "a.png".into(), width: w, height: h };
        assert_eq!(picture_size(&p(80, 20), None, 500.0), vec2(80.0, 20.0));
        assert_eq!(picture_size(&p(0, 0), Some(vec2(1000.0, 500.0)), 500.0), vec2(500.0, 250.0));
        assert_eq!(picture_size(&p(100, 0), Some(vec2(50.0, 25.0)), 500.0), vec2(100.0, 50.0));
        assert_eq!(picture_size(&p(0, 0), None, 500.0), UNKNOWN);
    }

    #[test]
    fn a_link_typed_is_unescaped_safely() {
        assert_eq!(unescaped("a%20b").as_deref(), Some("a b"));
        assert_eq!(unescaped("a%2"), None);
        assert_eq!(unescaped("%ff"), None);
    }
}
