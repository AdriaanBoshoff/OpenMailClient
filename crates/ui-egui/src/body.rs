//! Draws a message body that was read into a formatted document.
//!
//! A link shows where it points when the pointer rests on it and is never followed. A picture is
//! drawn only when the message carries it, as an attachment or written into the body, and it
//! could be decoded ([`Pictures`]); any other picture is an empty frame of the size it asks for,
//! or a labelled placeholder when it asks for none. A picture stands in the line of text it is
//! written in, or at one side when it asks to, with what follows beside it until it ends. An
//! attachment that stands in the text is a small card there with our own mark, its name and its
//! size; a click on it asks where to save it, and nothing is opened or run. A
//! border a style asks for is drawn round its paragraphs, its table or its cell. Text is drawn in the font it
//! names when that font, or an open one of the same measures, is installed ([`Faces`]). Colours
//! a message asks for are kept only where they stay readable on what is behind them. Sizes follow what Outlook Classic
//! was seen to do with the same message (`docs/ui-parity.md`, "Message bodies").

use crate::faces::Faces;
use crate::icons::{self, Icon};
use crate::pictures::{Pictures, Shown};
use crate::theme::Tokens;
use crate::{bold, clip_text, font, text_width};
use egui::text::{LayoutJob, TextFormat};
use egui::text_selection::LabelSelectionState;
use egui::{Align, Color32, FontFamily, FontId, Rect, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use openmailclient_fonts::script;
use openmailclient_formats::rich::{
    self, Block, Doc, Edge, Edges, Filed, Float, Frame, Line, Para, ParaKind, Picture, Run, Script, Table, TableWidth, VAlign, any_edge,
};
use std::cell::{Cell, RefCell};
use std::sync::Arc;

/// Size of text the message gives no size for: 12 points.
const BODY_PX: f32 = 16.0;
/// One level of indent: a list level, a block set in, half an inch of RTF indent.
const INDENT: f32 = 40.0;
const QUOTE: f32 = 14.0;
/// More columns than this are drawn one cell under another.
const MAX_COLUMNS: usize = 12;
/// Tables nested deeper than this are drawn one cell under another.
const MAX_TABLE_DEPTH: usize = 6;
/// Largest cell padding and spacing drawn, whatever the message asks for.
const MAX_CELL_ROOM: u16 = 64;
/// A picture that is not drawn gets a frame when it asks for at least this size one way. The
/// way it does not say is as tall as a line of its words, or as wide as a short name.
const MIN_FRAME: u16 = 24;
const FRAME_TALL: f32 = 28.0;
const FRAME_WIDE: f32 = 160.0;
const MIN_COLUMN: f32 = 24.0;
/// Rows of a table, and blocks of a cell, that are measured to size its columns.
const MEASURED_ROWS: usize = 100;
const MEASURED_BLOCKS: usize = 200;
/// Width a column is taken to want when what it holds is not measured.
const UNMEASURED: f32 = 240.0;
/// Widest and tallest a picture is drawn, whatever it asks for.
const MAX_PICTURE_WIDTH: f32 = 4096.0;
const MAX_PICTURE_HEIGHT: f32 = 4096.0;
/// Room kept under a picture in a line of text, so that it stands on the line the letters stand
/// on and not on the line their tails reach down to.
const BASELINE: f32 = 4.0;
/// Largest font size given to the space that holds a picture's place.
const MAX_HOLDER_PX: f32 = 20_000.0;
/// Room between a picture at one side of a paragraph and the text beside it.
const FLOAT_GAP: f32 = 8.0;
/// A picture stands at one side only when this much width is left for the text beside it.
const MIN_BESIDE: f32 = 120.0;
/// Runs of a paragraph looked through for a picture that stands at one side.
const MAX_FLOAT_SEARCH: usize = 256;
/// Widest border line drawn, and most dashes drawn along one side of a border.
const MAX_EDGE: u8 = 12;
const MAX_DASHES: usize = 2048;
/// Tabs of one paragraph whose room is filled with marks.
const MAX_LEADERS: usize = 64;
/// A border colour this close to what is behind it is drawn in the reader's own.
const EDGE_CONTRAST: f32 = 1.3;
/// A cell fill at least this light is paper: on the dark pane it is the pane.
const PALE_FILL: f32 = 0.82;
/// What is said under a link's address when the pointer rests on it.
const LINK_HINT: &str = "Click to be asked before it is opened.";
/// The card of an attachment that stands in the text: how tall it is, the room its mark and its
/// edges take beside the words, and the narrowest it is drawn.
const CARD_TALL: f32 = 22.0;
const CARD_ROOM: f32 = 32.0;
const MIN_CARD: f32 = 56.0;

/// What drawing needs besides the document: the theme, the decoded pictures, and the colour
/// behind the text being drawn (the pane, or a filled table cell).
#[derive(Clone, Copy)]
struct Look<'a> {
    t: &'a Tokens,
    pictures: &'a Pictures,
    faces: &'a Faces,
    on: Color32,
    /// The attachment a click in the text asked for, by its number in the message.
    picked: &'a Cell<Option<usize>>,
    /// The link a click in the text was on.
    link: &'a RefCell<Option<Arc<str>>>,
    /// The reader asked, on a picture not shown, for the message's pictures to be downloaded.
    download: &'a Cell<bool>,
}

/// What a click in the body asked for.
#[derive(Default)]
pub(crate) struct Picked {
    /// An attachment standing in the text, by its number: the reader asks for it to be saved.
    pub file: Option<usize>,
    /// A link: the reader is asked whether to open it.
    pub link: Option<Arc<str>>,
    /// The message's pictures on a server: the reader asked for them to be downloaded.
    pub download: bool,
}

impl Look<'_> {
    /// The colour of text that asks for none.
    fn text(&self) -> Color32 {
        if self.on == self.t.surface { self.t.text } else { plain_on(self.on) }
    }
}

/// Draw the body. Returns what a click in it asked for. Nothing is opened from here.
pub(crate) fn draw(ui: &mut Ui, doc: &Doc, t: &Tokens, pictures: &Pictures, faces: &Faces) -> Picked {
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let picked = Cell::new(None);
    let link = RefCell::new(None);
    let download = Cell::new(false);
    // A body that asks for a colour behind it is read against that colour.
    let on = page(doc).unwrap_or(t.surface);
    blocks(ui, &doc.blocks, Look { t, pictures, faces, on, picked: &picked, link: &link, download: &download }, 0);
    Picked { file: picked.get(), link: link.into_inner(), download: download.get() }
}

/// The colour a body asks to be read on, for whoever paints the pane behind it.
pub(crate) fn page(doc: &Doc) -> Option<Color32> {
    doc.fill.map(|c| Color32::from_rgb(c[0], c[1], c[2]))
}

/// A picture standing at one side, with room beside it still to fill.
#[derive(Clone, Copy)]
struct Aside {
    left: bool,
    /// Where text beside it may come to: the picture's far edge and the gap, across the pane.
    edge: f32,
    bottom: f32,
}

/// Pictures left standing at once, and those one paragraph sets at its sides.
const MAX_ASIDES: usize = 16;
const MAX_OWN_ASIDES: usize = 8;

/// The room across that `standing` leaves of `full` for what starts at `top`: a picture that
/// ends at or above there takes none.
fn between_asides(full: Rect, standing: &[Aside], top: f32) -> (f32, f32) {
    let live = standing.iter().filter(|a| a.bottom > top + 0.5);
    let left = live.clone().filter(|a| a.left).map(|a| a.edge).fold(full.left(), f32::max);
    let right = live.filter(|a| !a.left).map(|a| a.edge).fold(full.right(), f32::min);
    (left.min(full.right()), right.max(left + 8.0))
}

/// Where the lowest of the pictures still standing ends.
fn below_asides(standing: &[Aside]) -> Option<f32> {
    standing.iter().map(|a| a.bottom).reduce(f32::max)
}

/// Paragraphs that follow one another inside one border: the border and how many they are.
type Group<'a> = Option<(&'a Arc<Frame>, usize)>;

fn blocks(ui: &mut Ui, list: &[Block], look: Look, depth: usize) {
    let mut standing: Vec<Aside> = Vec::new();
    let mut i = 0usize;
    while let Some(block) = list.get(i) {
        let group: Group = rich::boxed_run(list.get(i..).unwrap_or_default(), 0);
        // A picture at the side ends where it ends, or where a paragraph asks to start below it.
        let top = ui.cursor().top();
        standing.retain(|a| a.bottom > top + 0.5);
        if let (Some(bottom), Block::Para(p)) = (below_asides(&standing), block)
            && p.clear
        {
            ui.add_space((bottom - top).max(0.0));
            standing.clear();
        }
        match (block, group) {
            // A paragraph stands beside the pictures that stand, and may set its own.
            (Block::Para(p), None) => para(ui, p, look, i > 0, &mut standing),
            _ if standing.is_empty() => one(ui, list, i, group, look, depth),
            _ => {
                // What follows a picture's paragraph stands beside the picture too.
                let top = ui.cursor().top();
                let full = ui.available_rect_before_wrap();
                let (left, right) = between_asides(full, &standing, top);
                let room = Rect::from_min_max(pos2(left, top), pos2(right, top));
                let mut child = ui.new_child(UiBuilder::new().id_salt(("beside", i)).max_rect(room));
                child.spacing_mut().item_spacing = vec2(0.0, 0.0);
                one(&mut child, list, i, group, look, depth);
                ui.allocate_rect(Rect::from_min_max(pos2(full.left(), top), pos2(full.right(), child.min_rect().bottom().max(top))), Sense::hover());
            }
        }
        i += group.map_or(1, |(_, count)| count.max(1));
    }
    if let Some(bottom) = below_asides(&standing) {
        ui.add_space((bottom - ui.cursor().top()).max(0.0));
    }
}

/// Draw the block at `i`, or the paragraphs that share its border.
fn one(ui: &mut Ui, list: &[Block], i: usize, group: Group, look: Look, depth: usize) {
    match (list.get(i), group) {
        (Some(_), Some((frame, count))) => framed(ui, list.get(i..i.saturating_add(count)).unwrap_or_default(), frame, look, (i, 0, depth)),
        (Some(Block::Para(p)), None) => contained(ui, p, look, i > 0),
        (Some(Block::Rule), _) => {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 13.0), Sense::hover());
            ui.painter().hline(rect.x_range(), rect.center().y, (1.0, look.t.text_faint));
        }
        (Some(Block::Table(table)), _) => {
            if i > 0 && !table.snug {
                ui.add_space(6.0);
            }
            self::table(ui, table, look, depth);
        }
        (None, _) => {}
    }
}

/// Room a border takes inside its box: left and top, then right and bottom.
fn frame_room(frame: &Frame) -> (Vec2, Vec2) {
    let side = |i: usize| frame.edges.get(i).map_or(0.0, |e| f32::from(e.width.min(MAX_EDGE)));
    let [above, right, below, left] = frame.padding.map(|v| f32::from(v.min(MAX_CELL_ROOM)));
    // Text does not touch a line beside it.
    let clear = |w: f32, pad: f32| if w > 0.0 { w + pad.max(2.0) } else { pad };
    (vec2(clear(side(3), left), side(0) + above), vec2(clear(side(1), right), side(2) + below))
}

/// Blocks inside one border: paragraphs, tables, and the boxes that stand inside this one.
/// `place` is which block of its list the first is, how many boxes this one stands inside, and
/// how deep in tables it is.
fn framed(ui: &mut Ui, paras: &[Block], frame: &Frame, look: Look, place: (usize, usize, usize)) {
    let (at, level, depth) = place;
    match paras.first() {
        Some(Block::Para(p)) if at > 0 => ui.add_space(f32::from(p.lead())),
        Some(Block::Table(t)) if at > 0 && !t.snug => ui.add_space(6.0),
        _ => {}
    }
    let top = ui.cursor().top();
    let mut full = ui.available_rect_before_wrap();
    // Set in on the side its paragraphs start at.
    let inset = (f32::from(frame.indent.min(12)) * INDENT).min(full.width() * 0.6);
    if matches!(paras.first(), Some(Block::Para(p)) if p.rtl) {
        full.max.x -= inset;
    } else {
        full.min.x += inset;
    }
    let (near, far) = frame_room(frame);
    let left = (full.left() + near.x).min(full.right());
    let inside = Rect::from_min_max(pos2(left, top + near.y), pos2((full.right() - far.x).max(left + 8.0).min(full.right().max(left)), top + near.y));
    // The fill goes behind what the box holds, and what it holds is read against the fill.
    let behind = ui.painter().add(egui::Shape::Noop);
    let fill = frame.fill.map(|c| Color32::from_rgb(c[0], c[1], c[2]));
    let look = Look { on: fill.unwrap_or(look.on), ..look };
    let mut child = ui.new_child(UiBuilder::new().id_salt(("framed", at, level)).max_rect(inside));
    child.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let between = f32::from(frame.between.width.min(MAX_EDGE));
    let mut k = 0usize;
    while let Some(block) = paras.get(k) {
        // The line between one paragraph and the next, with the room the border keeps.
        if k > 0 && between > 0.0 {
            let [above, _, below, _] = frame.padding.map(|v| f32::from(v.min(MAX_CELL_ROOM)));
            child.add_space(below);
            let y = child.cursor().top();
            let line = Rect::from_min_max(pos2(full.left(), y), pos2(full.right(), y + between));
            child.add_space(between + above);
            let only = [frame.between, Edge::default(), Edge::default(), Edge::default()];
            edges(ui, line, &only, look, look.text());
        }
        let rest = paras.get(k..).unwrap_or_default();
        k += match (rich::boxed_run(rest, level + 1), block) {
            // A box inside this one, with all that stands in it.
            (Some((inner, count)), _) => {
                framed(&mut child, rest.get(..count).unwrap_or_default(), inner, look, (if between > 0.0 { 0 } else { k }, level + 1, depth));
                count.max(1)
            }
            // A picture at the side stays inside the border.
            (None, Block::Para(p)) => {
                contained(&mut child, p, look, k > 0 && between == 0.0);
                1
            }
            (None, _) => {
                one(&mut child, paras, k, None, look, depth);
                1
            }
        };
    }
    let rect = Rect::from_min_max(pos2(full.left(), top), pos2(full.right(), child.min_rect().bottom().max(inside.top()) + far.y));
    ui.allocate_rect(rect, Sense::hover());
    let round = corners(&frame.edges, frame.radius, rect);
    if let Some(fill) = fill {
        ui.painter().set(behind, egui::Shape::rect_filled(rect, round, fill));
    }
    border(ui, rect, &frame.edges, round, look, look.text());
}

/// How far the corners of a box are rounded: by what it asks for, by no more than half its
/// shorter side, and not at all when its border is not one unbroken line all the way round
/// (sides that differ, dashes and double lines keep square corners).
pub(crate) fn corners(list: &Edges, radius: u16, rect: Rect) -> f32 {
    let whole = !any_edge(list) || rich::one_edge(list).is_some_and(|e| e.line == Line::Solid);
    if !whole || !rect.is_positive() {
        return 0.0;
    }
    f32::from(radius.min(rich::MAX_RADIUS)).min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0)
}

/// A border just inside `rect`: one line with rounded corners when `round` is more than
/// nothing, and the four sides each on their own otherwise.
fn border(ui: &Ui, rect: Rect, list: &Edges, round: f32, look: Look, unset: Color32) {
    match rich::one_edge(list) {
        Some(edge) if round > 0.0 => {
            if !ui.is_rect_visible(rect) {
                return;
            }
            let w = f32::from(edge.width.min(MAX_EDGE)).min(rect.width() / 2.0).min(rect.height() / 2.0);
            let color = edge.color.map_or(unset, |c| ink(c, look));
            ui.painter().rect_stroke(rect, round, (w, color), egui::StrokeKind::Inside);
        }
        _ => edges(ui, rect, list, look, unset),
    }
}

/// Where the borders round text within a line are drawn (`Style::edge`): one box for each
/// stretch of a line with the same border. A box is open at the side where its text goes on
/// in the line before or the line after. `pos` is the text's own corner.
pub(crate) fn outlines(galley: &egui::Galley, runs: &[Run], ends: &[usize], pos: egui::Pos2) -> Vec<(Rect, Edges)> {
    let mut out: Vec<(Rect, Edges)> = Vec::new();
    if !runs.iter().any(|r| r.style.edge.is_some()) {
        return out;
    }
    // Each glyph's border, in the order the text is stored.
    let mut marks: Vec<(usize, f32, f32, Option<Edge>)> = Vec::new();
    let mut chars = 0usize;
    for (n, row) in galley.rows.iter().enumerate() {
        for glyph in &row.glyphs {
            let run = ends.partition_point(|end| *end <= chars);
            let edge = runs.get(run).filter(|r| r.picture.is_none()).and_then(|r| r.style.edge).filter(|e| e.width > 0);
            let left = row.pos.x + glyph.pos.x;
            marks.push((n, left, left + glyph.advance_width, edge));
            chars = chars.saturating_add(1);
        }
        chars = chars.saturating_add(usize::from(row.ends_with_newline));
    }
    let none = Edge::default();
    let mut i = 0usize;
    while let Some(&(n, left, _, edge)) = marks.get(i) {
        let mut j = i + 1;
        let mut right = marks.get(i).map_or(left, |m| m.2);
        while let Some(&(m, _, to, other)) = marks.get(j) {
            if m != n || other != edge {
                break;
            }
            right = right.max(to);
            j += 1;
        }
        if let (Some(edge), Some(row)) = (edge, galley.rows.get(n)) {
            // Open where the same border goes on in another line.
            let before = i.checked_sub(1).and_then(|k| marks.get(k)).is_some_and(|m| m.3 == Some(edge));
            let after = marks.get(j).is_some_and(|m| m.3 == Some(edge));
            let rect = Rect::from_min_max(pos2(left.floor(), row.pos.y), pos2(right.ceil(), row.pos.y + row.size.y)).translate(pos.to_vec2());
            out.push((rect, [edge, if after { none } else { edge }, edge, if before { none } else { edge }]));
        }
        i = j.max(i + 1);
    }
    out
}

/// The lines of a border, drawn just inside `rect`. A side that names no colour, or one that
/// would not be seen on what is behind it, is drawn in `unset`.
fn edges(ui: &Ui, rect: Rect, edges: &Edges, look: Look, unset: Color32) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let seen = ui.clip_rect();
    let painter = ui.painter();
    for (side, edge) in edges.iter().enumerate() {
        let w = f32::from(edge.width.min(MAX_EDGE)).min(rect.width()).min(rect.height());
        if w <= 0.0 {
            continue;
        }
        let color = edge.color.map_or(unset, |c| ink(c, look));
        // The strip the line fills, and whether it runs across.
        let (strip, across) = match side {
            0 => (Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + w)), true),
            1 => (Rect::from_min_max(pos2(rect.right() - w, rect.top()), rect.max), false),
            2 => (Rect::from_min_max(pos2(rect.left(), rect.bottom() - w), rect.max), true),
            _ => (Rect::from_min_max(rect.min, pos2(rect.left() + w, rect.bottom())), false),
        };
        match edge.line {
            Line::Double if w >= 3.0 => {
                // Two lines with as much room between them.
                let thin = (w / 3.0).floor().max(1.0);
                let (first, second) = if across {
                    (
                        Rect::from_min_max(strip.min, pos2(strip.right(), strip.top() + thin)),
                        Rect::from_min_max(pos2(strip.left(), strip.bottom() - thin), strip.max),
                    )
                } else {
                    (
                        Rect::from_min_max(strip.min, pos2(strip.left() + thin, strip.bottom())),
                        Rect::from_min_max(pos2(strip.right() - thin, strip.top()), strip.max),
                    )
                };
                painter.rect_filled(first, 0.0, color);
                painter.rect_filled(second, 0.0, color);
            }
            Line::Dashed | Line::Dotted => {
                // A mark one pixel square is too small to be drawn: dots are two long at the least.
                let mark = if edge.line == Line::Dashed { w * 3.0 } else { w.max(2.0) };
                let step = mark * 2.0;
                let (start, end) = if across { (strip.left(), strip.right()) } else { (strip.top(), strip.bottom()) };
                let (from, to) = if across { (seen.left(), seen.right()) } else { (seen.top(), seen.bottom()) };
                // Only the marks that can be seen, keeping their places along the line.
                let skipped = ((from - start) / step).floor().max(0.0);
                // Marks start on a whole pixel, or the smallest of them fade away.
                let mut at = (start + skipped * step).round();
                let mut drawn = 0usize;
                while at < end.min(to) && drawn < MAX_DASHES && step > 0.0 {
                    let upto = (at + mark).min(end);
                    let dash = if across {
                        Rect::from_min_max(pos2(at, strip.top()), pos2(upto, strip.bottom()))
                    } else {
                        Rect::from_min_max(pos2(strip.left(), at), pos2(strip.right(), upto))
                    };
                    painter.rect_filled(dash, 0.0, color);
                    at += step;
                    drawn += 1;
                }
            }
            Line::Solid | Line::Double => {
                painter.rect_filled(strip, 0.0, color);
            }
        }
    }
}

/// What stands on the page where a run has a picture.
#[derive(Clone, Copy)]
enum Stand<'a> {
    Drawn(&'a Shown),
    /// The picture is not here to draw, but the body says how large it is.
    Frame,
    /// An attachment that stands in the text as a file.
    File(&'a Filed),
}

impl Stand<'_> {
    /// The size the picture has before the body's wishes and the room are taken into account.
    fn natural(&self, run: &Run) -> Vec2 {
        match self {
            Stand::Drawn(picture) => picture.size,
            Stand::Frame => {
                let given = |v: u16, unset: f32| if v > 0 { f32::from(v) } else { unset };
                vec2(given(run.extent.0, FRAME_WIDE), given(run.extent.1, FRAME_TALL))
            }
            Stand::File(_) => vec2(MIN_CARD, CARD_TALL),
        }
    }
}

/// The picture a run stands for, or the frame that takes its place; `None` for text, and for a
/// picture that is shown as a line of text.
fn shown<'a>(run: &'a Run, pictures: &'a Pictures) -> Option<Stand<'a>> {
    let drawn = match run.picture.as_ref()? {
        Picture::Attached(id) => pictures.get(id),
        Picture::Inline(Some(index)) => pictures.written(*index),
        Picture::Remote(address) => pictures.remote(address),
        Picture::Inline(None) | Picture::Object(_) => None,
        Picture::File(filed) => return Some(Stand::File(filed)),
    };
    if let Some(drawn) = drawn {
        return Some(Stand::Drawn(drawn));
    }
    let fits = |v: u16| v == 0 || v >= MIN_FRAME;
    (run.extent.0.max(run.extent.1) >= MIN_FRAME && fits(run.extent.0) && fits(run.extent.1)).then_some(Stand::Frame)
}

/// What a picture that is not drawn is called.
fn missing(picture: &Picture) -> &'static str {
    match picture {
        Picture::Remote(_) => "Picture not downloaded",
        Picture::Attached(_) | Picture::Inline(_) => "Picture",
        Picture::Object(_) | Picture::File(_) => "Attachment",
    }
}

/// A picture's alternative text, as it is shown.
fn alt(run: &Run) -> String {
    run.text.chars().filter(|c| !c.is_control()).take(120).collect()
}

/// The colour of notes that stand in for content.
fn faint(look: Look) -> Color32 {
    if look.on == look.t.surface { look.t.text_faint } else { look.text().gamma_multiply(0.75) }
}

/// An empty frame where a picture would be: an outline, our own mark for "no picture", and what
/// the message says the picture shows.
fn frame(ui: &Ui, run: &Run, at: Rect, look: Look) {
    let color = faint(look);
    let painter = ui.painter().with_clip_rect(at.intersect(ui.clip_rect()));
    painter.rect_stroke(at, 0.0, (1.0, color.gamma_multiply(0.6)), egui::StrokeKind::Inside);
    // The mark: a small picture frame with a line struck through it.
    let mark = Rect::from_min_size(at.min + vec2(7.0, 7.0), vec2(14.0, 11.0));
    painter.rect_stroke(mark, 1.5, (1.0, color), egui::StrokeKind::Inside);
    painter.line_segment([mark.left_bottom() + vec2(-2.0, 2.0), mark.right_top() + vec2(2.0, -2.0)], (1.2, color));
    // A frame too narrow for a word of its name has only the mark; the name is in its tip.
    if at.width() < 80.0 {
        return;
    }
    let what = run.picture.as_ref().map_or("Picture", missing);
    let alt = alt(run);
    // A frame too short for two lines says what the picture is called, or else what is missing.
    let label = match (alt.is_empty(), at.height() >= 40.0) {
        (true, _) => what.to_string(),
        (false, true) => format!("{what}\n{alt}"),
        (false, false) => alt,
    };
    let galley = painter.layout(label, font(11.5), color, (at.width() - 36.0).max(8.0));
    painter.galley(at.min + vec2(28.0, 5.0), galley, color);
}

/// What the card of an attachment says: its name, what kind of object it is, and its size.
fn card_words(filed: &Filed) -> String {
    let mut words: String = filed.name.chars().filter(|c| !c.is_control()).take(120).collect();
    if let Some(kind) = &filed.kind {
        let kind: String = kind.chars().filter(|c| !c.is_control()).take(60).collect();
        words = format!("{words} ({kind})");
    }
    if filed.bytes > 0 {
        words = format!("{words}  ({})", openmailclient_engine::attachments::size_words(filed.bytes));
    }
    words
}

/// The size of an attachment's card in a line no wider than `wrap`.
fn card_size(ui: &Ui, filed: &Filed, wrap: f32) -> Vec2 {
    let wide = text_width(ui.painter(), &card_words(filed), font(12.0)) + CARD_ROOM;
    let most = if wrap.is_finite() { wrap.max(MIN_CARD) } else { MAX_PICTURE_WIDTH };
    vec2(wide.clamp(MIN_CARD, most).ceil(), CARD_TALL)
}

/// What is said about an attachment in the text when the pointer rests on it.
fn card_tip(filed: &Filed) -> String {
    let mut lines = vec![card_words(filed)];
    lines.extend(filed.caution.iter().map(|words| words.chars().take(300).collect::<String>()));
    lines.push(match filed.number {
        Some(_) => "Click to save this attachment to a file. Nothing is opened or run from here.".to_string(),
        None => "An object of another program that cannot be shown here.".to_string(),
    });
    lines.join("\n")
}

/// An attachment where it stands in the text: our own mark for a file, or for one to think
/// twice about, then its name and size. A click asks for it to be saved.
fn card(ui: &Ui, filed: &Filed, at: Rect, look: Look) {
    let plain = look.on == look.t.surface;
    let response = filed.number.map(|number| ui.interact(at, ui.id().with(("file in text", number)), Sense::click()));
    let hovered = response.as_ref().is_some_and(|r| r.hovered());
    let painter = ui.painter().with_clip_rect(at.intersect(ui.clip_rect()));
    let ink = look.text();
    let fill = match (plain, hovered) {
        (true, true) => look.t.hover,
        (true, false) => look.t.pane,
        (false, true) => ink.gamma_multiply(0.15),
        (false, false) => Color32::TRANSPARENT,
    };
    painter.rect_filled(at, 3.0, fill);
    // One a reader should think twice about says so before it is pointed at.
    let (line, icon, tint) = match filed.caution {
        Some(_) => ((1.5, look.t.danger), Icon::Importance, look.t.danger),
        None => ((1.0, if plain { look.t.line } else { faint(look) }), Icon::Page, faint(look)),
    };
    painter.rect_stroke(at, 3.0, line, egui::StrokeKind::Inside);
    icons::paint(&painter, Rect::from_center_size(pos2(at.left() + 12.0, at.center().y), vec2(13.0, 13.0)), icon, tint);
    // A long name is cut in its middle: the ending is what tells the kind of file.
    let room = at.width() - CARD_ROOM + 4.0;
    let words = card_words(filed);
    let words = if text_width(&painter, &words, font(12.0)) > room { crate::middle_cut(&painter, &words, room, font(12.0)) } else { words };
    clip_text(&painter, pos2(at.left() + 23.0, at.center().y - 8.0), room, &words, font(12.0), ink);
    if let Some(response) = response {
        if response.clicked() {
            look.picked.set(filed.number);
        }
        response.on_hover_text(card_tip(filed));
    }
}

/// Draw a paragraph with the room under any picture it sets at its side: nothing of it is left
/// standing for what follows.
fn contained(ui: &mut Ui, p: &Para, look: Look, after_another: bool) {
    let mut standing = Vec::new();
    para(ui, p, look, after_another, &mut standing);
    if let Some(bottom) = below_asides(&standing) {
        ui.add_space((bottom - ui.cursor().top()).max(0.0));
    }
}

/// Draw a paragraph. `standing` are the pictures already standing at the sides: the text is set
/// beside them, line by line as wide as they leave it, and under each from where it ends. The
/// pictures of the paragraph's own that ask to stand at a side do, one beside another while
/// there is room for text between them, and are added to `standing` for what follows.
fn para(ui: &mut Ui, para: &Para, look: Look, after_another: bool, standing: &mut Vec<Aside>) {
    if after_another {
        ui.add_space(f32::from(para.lead()));
    }
    let top = ui.cursor().top();
    standing.retain(|a| a.bottom > top + 0.5);
    let asks = |run: &Run| run.float != Float::None && run.picture.is_some();
    if standing.is_empty() && !para.runs.iter().take(MAX_FLOAT_SEARCH).any(asks) {
        let (_, width) = room(ui, para);
        piece(ui, para, &para.runs, look, Spot { left: 0.0, width, min_height: 0.0, marker: true });
        return;
    }
    // Those that stood before this paragraph narrow the room it is set in; its own stand
    // inside that room, and narrow its text.
    let others = standing.len();
    let full = ui.available_rect_before_wrap();
    let (room_left, room_right) = between_asides(full, standing, top);
    let (inset, width) = room_of(room_right - room_left, para);
    let text_left = room_left + if para.rtl { 0.0 } else { inset };
    let (mut left, mut right) = (text_left, text_left + width);
    let mut rest: Vec<Run> = Vec::with_capacity(para.runs.len());
    for (at, run) in para.runs.iter().enumerate() {
        let stand = (at < MAX_FLOAT_SEARCH && asks(run) && standing.len() < others.saturating_add(MAX_OWN_ASIDES).min(MAX_ASIDES))
            .then(|| shown(run, look.pictures))
            .flatten();
        let placed = stand.and_then(|stand| {
            let size = picture_size(run.extent, stand.natural(run), width);
            (right - left - size.x - FLOAT_GAP >= MIN_BESIDE).then_some((stand, size))
        });
        let Some((stand, size)) = placed else {
            rest.push(run.clone());
            continue;
        };
        let at_left = run.float == Float::Left;
        let x = if at_left { left } else { right - size.x };
        if at_left {
            left = x + size.x + FLOAT_GAP;
        } else {
            right = x - FLOAT_GAP;
        }
        standing.push(Aside { left: at_left, edge: if at_left { left } else { right }, bottom: top + size.y + FLOAT_GAP / 2.0 });
        let spot = Rect::from_min_size(pos2(x.round(), top.round()), size);
        if ui.is_rect_visible(spot) {
            picture(ui, run, stand, spot, look);
            let id = ui.id().with(("aside", at, spot.min.x.to_bits(), spot.min.y.to_bits()));
            let tip = tip(run, stand);
            let response = ui.interact(spot, id, if not_downloaded(run, stand) { Sense::click() } else { Sense::hover() });
            offer_download(&response, run, stand, look);
            if !tip.is_empty() {
                response.on_hover_text_at_pointer(tip);
            }
        }
    }

    // The text, in as many pieces as there are heights at which a picture ends.
    let mut marker = true;
    for band in 0..=MAX_ASIDES {
        let now = ui.cursor().top();
        let full = ui.available_rect_before_wrap();
        let (earlier, own) = standing.split_at(others.min(standing.len()));
        let (room_left, room_right) = between_asides(full, earlier, now);
        let (inset, width) = room_of(room_right - room_left, para);
        let text_left = room_left + if para.rtl { 0.0 } else { inset };
        let (left, right) = between_asides(Rect::from_min_max(pos2(text_left, now), pos2(text_left + width, now)), own, now);
        let until = standing.iter().map(|a| a.bottom).filter(|bottom| *bottom > now + 0.5).reduce(f32::min).filter(|_| band < MAX_ASIDES);
        let mut child = ui.new_child(
            UiBuilder::new().id_salt(("band", now.to_bits(), band)).max_rect(Rect::from_min_max(pos2(room_left, now), pos2(room_right, now))),
        );
        child.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let spot = |min_height: f32| Spot { left: left - text_left, width: (right - left).max(8.0), min_height, marker };
        let cut = until.and_then(|until| {
            let (job, ends, _) = job(&child, para, &rest, look, (right - left).max(8.0));
            let galley = child.painter().layout_job(job);
            lines_beside(&galley, until - now).map(|chars| (split(&rest, &ends, chars), until))
        });
        let done = match cut {
            Some(((head, tail), until)) => {
                piece(&mut child, para, &head, look, spot(until - now));
                rest = tail;
                false
            }
            None => {
                piece(&mut child, para, &rest, look, spot(0.0));
                true
            }
        };
        ui.allocate_rect(Rect::from_min_max(pos2(full.left(), now), pos2(full.right(), child.min_rect().bottom().max(now))), Sense::hover());
        if done {
            break;
        }
        marker = false;
    }
}

/// How many characters of a laid-out text stand in lines that start above `tall`; `None` when
/// all of them do. The first line always does.
fn lines_beside(galley: &egui::Galley, tall: f32) -> Option<usize> {
    let mut chars = 0usize;
    for (i, row) in galley.rows.iter().enumerate() {
        if i > 0 && row.pos.y >= tall {
            return Some(chars);
        }
        chars = chars.saturating_add(row.glyphs.len()).saturating_add(usize::from(row.ends_with_newline));
    }
    None
}

/// Runs cut in two after `chars` characters of the text they are drawn as; `ends` is where each
/// run ends in that text. Only plain text is cut; a picture or its label stays whole.
pub(crate) fn split(runs: &[Run], ends: &[usize], chars: usize) -> (Vec<Run>, Vec<Run>) {
    let (mut head, mut tail): (Vec<Run>, Vec<Run>) = (Vec::new(), Vec::new());
    let mut start = 0usize;
    for (run, end) in runs.iter().zip(ends) {
        if *end <= chars {
            head.push(run.clone());
        } else if start >= chars {
            tail.push(run.clone());
        } else if run.picture.is_some() {
            head.push(run.clone());
        } else {
            let cut = run.text.char_indices().nth(chars - start).map_or(run.text.len(), |(i, _)| i);
            let (a, b) = run.text.split_at_checked(cut).unwrap_or((run.text.as_str(), ""));
            head.push(Run { text: a.to_string(), ..run.clone() });
            tail.push(Run { text: b.to_string(), ..run.clone() });
        }
        start = *end;
    }
    // The line break that ended the last line beside the picture is not drawn twice.
    if let Some(last) = head.last_mut()
        && last.picture.is_none()
        && last.text.ends_with('\n')
    {
        last.text.pop();
    }
    (head, tail)
}

/// Room a paragraph leaves on its left.
fn inset(para: &Para) -> f32 {
    f32::from(para.indent.min(12)) * INDENT + f32::from(para.quote.min(8)) * QUOTE
}

/// Room a paragraph leaves on its left here, and the width left for what it holds.
fn room(ui: &Ui, para: &Para) -> (f32, f32) {
    room_of(ui.available_width(), para)
}

/// [`room`] in a width that is given.
fn room_of(available: f32, para: &Para) -> (f32, f32) {
    let inset = inset(para).min(available * 0.6);
    (inset, (available - inset).max(40.0))
}

fn quote_bars(ui: &Ui, para: &Para, rect: Rect, t: &Tokens) {
    for level in 0..para.quote.min(8) {
        let x = f32::from(level) * QUOTE + 4.0;
        let x = if para.rtl { rect.right() - x } else { rect.left() + x };
        ui.painter().vline(x, rect.y_range(), (2.0, t.line.lerp_to_gamma(t.text_faint, 0.6)));
    }
}

/// The size a picture is drawn at: what the body asks for, else its own, never wider than `room`.
pub(crate) fn picture_size(asked: (u16, u16), natural: Vec2, room: f32) -> Vec2 {
    let natural = vec2(natural.x.max(1.0), natural.y.max(1.0));
    let (w, h) = (f32::from(asked.0), f32::from(asked.1));
    let mut size = match (w > 0.0, h > 0.0) {
        (true, true) => vec2(w, h),
        (true, false) => vec2(w, natural.y * w / natural.x),
        (false, true) => vec2(natural.x * h / natural.y, h),
        (false, false) => natural,
    };
    if size.x > room {
        size *= room.max(1.0) / size.x;
    }
    vec2(size.x.clamp(1.0, MAX_PICTURE_WIDTH), size.y.clamp(1.0, MAX_PICTURE_HEIGHT))
}

/// What is laid behind a dark picture that is seen through, on the dark pane: paper, a little
/// wider than the picture.
pub(crate) const BACKING: Color32 = Color32::from_rgb(0xF4, 0xF4, 0xF2);
const BACKING_MARGIN: f32 = 3.0;
/// A picture no lighter than this is taken to be drawn for a light page.
const DARK_INK: f32 = 0.42;

/// Whether a picture is given a backing: one that is seen through and is dark, where what is
/// behind it is the dark pane itself. A light one is drawn for a dark page and needs none; where
/// the message chose what is behind, that is left as chosen.
fn backed(drawn: &Shown, look: Look) -> bool {
    look.t.dark && look.on == look.t.surface && drawn.ink.is_some_and(|ink| ink <= DARK_INK)
}

/// Draw a picture, or the frame that takes its place.
fn picture(ui: &Ui, run: &Run, stand: Stand, at: Rect, look: Look) {
    match stand {
        Stand::Drawn(drawn) => {
            if backed(drawn, look) {
                ui.painter().rect_filled(at.expand(BACKING_MARGIN), 3.0, BACKING);
            }
            ui.painter().image(
                drawn.frame(ui.ctx(), look.pictures.moving, at.size()),
                at,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        Stand::Frame => frame(ui, run, at, look),
        // A card is set as low as the tails of the letters beside it, so that its words stand on
        // their line.
        Stand::File(filed) => card(ui, filed, at.translate(vec2(0.0, BASELINE)), look),
    }
}

/// A picture on a server, shown as the frame that takes its place.
fn not_downloaded(run: &Run, stand: Stand) -> bool {
    matches!((stand, &run.picture), (Stand::Frame, Some(Picture::Remote(_))))
}

/// A right-click on a picture not downloaded offers to download the message's pictures, as
/// nothing is fetched by itself.
fn offer_download(response: &egui::Response, run: &Run, stand: Stand, look: Look) {
    if !not_downloaded(run, stand) {
        return;
    }
    response.context_menu(|ui| {
        if ui.button("Download Pictures").clicked() {
            look.download.set(true);
            ui.close();
        }
    });
}

/// What is said about a picture when the pointer rests on it.
fn tip(run: &Run, stand: Stand) -> String {
    if let Stand::File(filed) = stand {
        return card_tip(filed);
    }
    let mut lines: Vec<String> = Vec::new();
    if let (Stand::Frame, Some(picture)) = (stand, &run.picture) {
        lines.push(missing(picture).to_string());
    }
    lines.push(alt(run));
    if let Some(link) = run.style.link.as_deref() {
        lines.push(link.chars().take(300).collect());
        lines.push(LINK_HINT.to_string());
    }
    lines.retain(|line| !line.is_empty());
    lines.join("\n")
}

/// A picture that stands in a line of text, and the size it is drawn at.
struct Placed<'a> {
    run: &'a Run,
    stand: Stand<'a>,
    size: Vec2,
    /// The character that holds its place in the text, and the line height given to it.
    holder: (char, f32),
}

/// What holds a picture's place in a line of text: a space set so large that it is as wide as
/// the picture, on a line as tall as the picture. `None` when the fonts have no such space.
fn holder(ui: &Ui, size: Vec2) -> Option<(char, TextFormat)> {
    const PROBE: f32 = 64.0;
    let probe = FontId::new(PROBE, FontFamily::Proportional);
    let (space, px) = ui.ctx().fonts_mut(|fonts| {
        // An em space is as wide as its font is tall; a plain space does where there is none.
        [('\u{2003}', true), (' ', false)].into_iter().find_map(|(c, check)| {
            if check && !fonts.has_glyph(&probe, c) {
                return None;
            }
            let advance = fonts.glyph_width(&probe, c);
            (advance.is_finite() && advance > 1.0).then(|| (c, size.x * PROBE / advance))
        })
    })?;
    let format = TextFormat {
        font_id: FontId::new(px.clamp(1.0, MAX_HOLDER_PX), FontFamily::Proportional),
        line_height: Some(size.y + BASELINE),
        color: Color32::TRANSPARENT,
        valign: Align::BOTTOM,
        ..TextFormat::default()
    };
    Some((space, format))
}

/// Runs laid out as one piece of text no wider than `wrap`; the character offset where each run
/// ends; and the pictures that stand in the text, in order.
fn job<'a>(ui: &Ui, para: &Para, runs: &'a [Run], look: Look<'a>, wrap: f32) -> (LayoutJob, Vec<usize>, Vec<Placed<'a>>) {
    let (job, ends, placed, _) = job_and_leaders(ui, para, runs, look, wrap);
    (job, ends, placed)
}

/// A tab whose room is filled with marks.
struct Lead {
    /// Which of the text's tabs it is, counted from the first.
    nth: usize,
    with: rich::Leader,
    /// The room before the tab, and the size of the letters beside it.
    room: f32,
    size: f32,
}

/// As [`job`], and the tabs whose room is filled.
fn job_and_leaders<'a>(ui: &Ui, para: &Para, runs: &'a [Run], look: Look<'a>, wrap: f32) -> (LayoutJob, Vec<usize>, Vec<Placed<'a>>, Vec<Lead>) {
    let mut leaders: Vec<Lead> = Vec::new();
    let mut tabs = 0usize;
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    // Where the text stands in its line, kept only where a tab will ask.
    let tabbed = runs.iter().any(|r| r.picture.is_none() && r.text.contains('\t'));
    let mut x = 0.0f32;
    job.halign = match para.align {
        rich::Align::Left => Align::LEFT,
        rich::Align::Center => Align::Center,
        rich::Align::Right => Align::RIGHT,
    };
    let mut ends: Vec<usize> = Vec::with_capacity(runs.len());
    let mut placed: Vec<Placed> = Vec::new();
    let mut chars = 0usize;
    // A line told how tall to be: each piece of text asks for that height, counted from
    // its own.
    let told = |format: &mut TextFormat| {
        if para.leading != rich::Leading::Own {
            let own = ui.ctx().fonts_mut(|fonts| fonts.row_height(&format.font_id));
            format.line_height = Some(para.leading.tall(own, 1.0).max(1.0));
        }
    };
    for (at, run) in runs.iter().enumerate() {
        let held = shown(run, look.pictures).and_then(|stand| {
            let size = match stand {
                Stand::File(filed) => card_size(ui, filed, wrap),
                _ => picture_size(run.extent, stand.natural(run), wrap),
            };
            holder(ui, size).map(|(space, format)| (stand, size, space, format))
        });
        let wide = held.as_ref().map(|(_, size, ..)| size.x);
        let (text, format) = match held {
            Some((stand, size, space, format)) => {
                placed.push(Placed { run, stand, size, holder: (space, size.y + BASELINE) });
                (space.to_string(), format)
            }
            None => {
                let (text, mut format) = section(run, look);
                // A link that is not what its text says is marked before anyone points at it.
                if run.picture.is_none() && openmailclient_formats::links::concern_at(runs, at).is_some() {
                    format.underline = Stroke::new(2.0, look.t.danger);
                }
                told(&mut format);
                (text, format)
            }
        };
        chars = chars.saturating_add(text.chars().count());
        ends.push(chars);
        match wide {
            _ if !tabbed => job.append(&text, 0.0, format),
            Some(wide) => {
                x += wide;
                job.append(&text, 0.0, format);
            }
            None => {
                let mut rest = text.as_str();
                loop {
                    let (before, after) = match rest.split_once('\t') {
                        Some((before, after)) => (before, Some(after)),
                        None => (rest, None),
                    };
                    if !before.is_empty() {
                        let measure = |text: &str| text_width(ui.painter(), text, format.font_id.clone());
                        x = match before.rsplit_once('\n') {
                            Some((_, last)) => measure(last),
                            None => x + measure(before),
                        };
                        job.append(before, 0.0, format.clone());
                    }
                    let Some(after) = after else { break };
                    let (tab, own) = tab_format(ui, &format);
                    // Preformatted text has a stop every eight characters.
                    let step = if matches!(para.kind, ParaKind::Pre) {
                        8.0 * text_width(ui.painter(), " ", format.font_id.clone())
                    } else {
                        f32::from(rich::TAB_STEP)
                    };
                    // A tab past the end of the first line stands where nothing here can say.
                    let room = if x < wrap {
                        let (ahead, point) = ahead(ui, runs, look, at, after, &format.font_id);
                        let room = rich::tab_room(&para.tabs, 1.0, step, x, ahead, point);
                        if let Some((_, stop)) = rich::tab_stop(&para.tabs, 1.0, x)
                            && stop.lead != rich::Leader::None
                            && leaders.len() < MAX_LEADERS
                        {
                            leaders.push(Lead { nth: tabs, with: stop.lead, room: (room - own).max(0.0), size: format.font_id.size });
                        }
                        room
                    } else {
                        step
                    };
                    tabs = tabs.saturating_add(1);
                    job.append("\t", (room - own).max(0.0), tab);
                    x += room.max(own);
                    rest = after;
                }
            }
        }
    }
    (job, ends, placed, leaders)
}

/// The section each character of a text to lay out belongs to.
fn sections_of(job: &LayoutJob) -> Vec<usize> {
    let mut out = vec![0usize; job.text.chars().count()];
    let mut starts = job.text.char_indices().map(|(byte, _)| byte).enumerate().peekable();
    for (n, section) in job.sections.iter().enumerate() {
        let end: usize = section.byte_range.end.into();
        while let Some((at, _)) = starts.next_if(|(_, byte)| *byte < end) {
            if let Some(slot) = out.get_mut(at) {
                *slot = n;
            }
        }
    }
    out
}

/// `chars` with the letters of each Arabic word that changes its look part way (bold, colour,
/// a link) given the shapes they have where they stand. The text shaper is handed one look at
/// a time and would end each part as if the word ended there; the letters of such a word are
/// joined here, each to a shape that every look of the word can draw, and the shaper draws
/// them as they come. As many characters come back as went in.
pub(crate) fn joined_across(ui: &Ui, job: &LayoutJob, chars: &[char], sections: &[usize]) -> Vec<char> {
    let mut out = chars.to_vec();
    let mut at = 0usize;
    while at < chars.len() {
        let from = at;
        while chars.get(at).is_some_and(|c| script::joins(*c)) {
            at += 1;
        }
        if at == from {
            at += 1;
            continue;
        }
        let looks: Vec<usize> = {
            let mut looks: Vec<usize> = sections.get(from..at).unwrap_or_default().to_vec();
            looks.dedup();
            looks
        };
        if looks.len() < 2 {
            continue;
        }
        let fonts: Vec<FontId> = looks.iter().filter_map(|n| job.sections.get(*n)).map(|s| s.format.font_id.clone()).collect();
        let run = chars.get(from..at).unwrap_or_default();
        let shapes = ui.ctx().fonts_mut(|f| script::joined(run, &mut |c| fonts.iter().all(|font| f.has_glyph(font, c))));
        if let Some(slots) = out.get_mut(from..at) {
            for (slot, c) in slots.iter_mut().zip(shapes) {
                *slot = c;
            }
        }
    }
    out
}

/// The most characters and the most words of a paragraph whose order is worked out; a longer
/// one is drawn as it is stored.
const MAX_TURNED_CHARS: usize = 60_000;
const MAX_TURNED_WORDS: usize = 8_000;

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Word,
    Blank,
    Break,
}

/// A word, the blanks between two words, or the end of a line: characters of one look that
/// stand equally deep in right-to-left text.
struct Word {
    /// The characters it is, of the text as it is stored.
    chars: (usize, usize),
    section: usize,
    level: u8,
    /// Whether its letters are ones written from the right: the text shaper turns those round.
    rtl: bool,
    part: Part,
    wide: f32,
}

/// Adds `text` to a text to lay out as a section of its own. The text shaper works section by
/// section and takes each to run one way; `LayoutJob::append` would make one section of
/// neighbours that look alike.
pub(crate) fn section_of_its_own(job: &mut LayoutJob, text: &str, lead: f32, format: TextFormat) {
    let start = job.text.len();
    job.text.push_str(text);
    job.sections.push(egui::text::LayoutSection {
        leading_space: lead,
        byte_range: egui::text::ByteIndex(start)..egui::text::ByteIndex(job.text.len()),
        format,
    });
}

/// A paragraph with right-to-left writing in it, as it is drawn.
pub(crate) struct Turned {
    /// The text with its lines broken and each in the order it is seen in.
    pub(crate) job: LayoutJob,
    /// For each line, what stands in it from the left: where it starts and ends, counted from
    /// where the line starts, and which characters of the stored text it is.
    pub(crate) rows: Vec<Vec<(f32, f32, (usize, usize))>>,
    /// The pictures in the order they are met line by line from the left: for each, which of
    /// the paragraph's pictures it is.
    pictures: Vec<usize>,
    /// The tabs in the order they are met line by line from the left: for each, which of the
    /// text's tabs it is.
    pub(crate) tabs: Vec<usize>,
    /// For each piece of each line, true when its characters are seen from the right: the
    /// first of them at its right.
    pub(crate) backwards: Vec<Vec<bool>>,
}

/// Where each character of a paragraph drawn in another order than it is stored is seen,
/// counted from where the paragraph is drawn: the line it is on, and its left and right. A
/// piece whose letters were each drawn as one glyph goes by the glyphs; one whose letters were
/// joined into fewer is shared out evenly.
pub(crate) fn char_spots(galley: &egui::Galley, turned: &Turned) -> Vec<Option<(usize, f32, f32)>> {
    let count = turned.rows.iter().flatten().map(|(.., (_, to))| *to).max().unwrap_or(0).min(MAX_TURNED_CHARS);
    let mut out = vec![None; count];
    for (n, (row, pieces)) in galley.rows.iter().zip(&turned.rows).enumerate() {
        for (i, (from, to, (first, end))) in pieces.iter().enumerate() {
            let len = end.saturating_sub(*first);
            if len == 0 {
                continue;
            }
            let backwards = turned.backwards.get(n).and_then(|row| row.get(i)).copied().unwrap_or(false);
            let glyphs: Vec<(f32, f32)> = row
                .glyphs
                .iter()
                .map(|g| (row.pos.x + g.pos.x, row.pos.x + g.pos.x + g.advance_width))
                .filter(|(l, r)| (l + r) / 2.0 >= *from - 0.01 && (l + r) / 2.0 < *to + 0.01)
                .collect();
            let even = (to - from) / len as f32;
            for k in 0..len {
                let seen = if backwards { len - 1 - k } else { k };
                let spot = match glyphs.get(seen).filter(|_| glyphs.len() == len) {
                    Some(at) => *at,
                    None => (from + even * seen as f32, from + even * (seen + 1) as f32),
                };
                if let Some(slot) = out.get_mut(first + k) {
                    *slot = Some((n, spot.0, spot.1));
                }
            }
        }
    }
    out
}

/// The place between two characters nearest to `at`, counted from where the paragraph is
/// drawn: how many of the stored characters are before it. A click in the half of a letter
/// that is read first is before it; one past either end of a line is at the line's start or
/// its end as its paragraph (`rtl` when from the right) is read.
pub(crate) fn turned_hit(galley: &egui::Galley, spots: &[Option<(usize, f32, f32)>], turned: &Turned, rtl: bool, at: Vec2) -> Option<usize> {
    let n = galley.rows.iter().position(|row| at.y < row.pos.y + row.size.y).unwrap_or(galley.rows.len().checked_sub(1)?);
    let on_row = || spots.iter().enumerate().filter_map(|(i, s)| s.filter(|s| s.0 == n).map(|s| (i, s.1, s.2)));
    let (first, last) = (on_row().map(|s| s.0).min()?, on_row().map(|s| s.0).max()?);
    let (most_left, most_right) =
        (on_row().map(|s| s.1.min(s.2)).fold(f32::INFINITY, f32::min), on_row().map(|s| s.1.max(s.2)).fold(f32::NEG_INFINITY, f32::max));
    if at.x < most_left {
        return Some(if rtl { last + 1 } else { first });
    }
    if at.x > most_right {
        return Some(if rtl { first } else { last + 1 });
    }
    let mut best: Option<(usize, f32)> = None;
    for (index, spot) in spots.iter().enumerate() {
        let Some((row, left, right)) = spot else { continue };
        if *row != n {
            continue;
        }
        let away = if at.x < *left {
            left - at.x
        } else if at.x > *right {
            at.x - right
        } else {
            0.0
        };
        if best.is_none_or(|(_, b)| away < b) {
            let backwards = turned_backwards(turned, n, index);
            let before = (at.x < (left + right) / 2.0) != backwards;
            best = Some((if before { index } else { index + 1 }, away));
        }
    }
    best.map(|(index, _)| index)
}

/// Whether the character `index` of line `n` is in a piece seen from the right.
fn turned_backwards(turned: &Turned, n: usize, index: usize) -> bool {
    let Some(pieces) = turned.rows.get(n) else { return false };
    pieces
        .iter()
        .position(|(.., (from, to))| index >= *from && index < *to)
        .and_then(|i| turned.backwards.get(n).and_then(|row| row.get(i)))
        .copied()
        .unwrap_or(false)
}

/// What is selected in a paragraph drawn in another order than it is stored: which paragraph,
/// and from where to where in its stored text. One at a time, in the whole window.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct TurnedPick {
    pub(crate) para: egui::Id,
    pub(crate) anchor: usize,
    pub(crate) cursor: usize,
}

impl TurnedPick {
    pub(crate) fn id() -> egui::Id {
        egui::Id::new("openmailclient-turned-selection")
    }

    pub(crate) fn range(&self) -> (usize, usize) {
        (self.anchor.min(self.cursor), self.anchor.max(self.cursor))
    }
}

/// Whether the text shaper takes `text` to be written from right to left: it goes by the
/// first letter that belongs to one script, and is not told what stands around the text.
fn shaped_from_the_right(text: &[char]) -> bool {
    text.iter().find(|c| c.is_alphanumeric() && !c.is_ascii_digit() && **c != '\u{0640}').is_some_and(|c| script::is_rtl(*c))
}

/// A word as it is handed to the text shaper so that it is drawn the way `level` says. The
/// shaper turns a word of right-to-left letters round, and its brackets with it; whatever else
/// is to be turned is turned here.
pub(crate) fn spelled(text: &[char], level: u8) -> String {
    match (level % 2 == 1, shaped_from_the_right(text)) {
        (true, true) | (false, false) => text.iter().collect(),
        (true, false) => text.iter().rev().map(|c| script::mirrored(*c)).collect(),
        // Turned here, and turned back by the shaper: digits written with Arabic letters.
        (false, true) => text.iter().rev().collect(),
    }
}

/// A text with right-to-left writing in it, as it is drawn: its lines broken at its words and
/// each put in the order it is seen in (UAX #9, L1 and L2). The text shaper is handed one
/// piece of a word at a time, letters written from the right apart from everything else,
/// because it takes all it is handed at once to run one way. `None` for text that is drawn as
/// it is stored.
pub(crate) fn turned(ui: &Ui, para: &Para, job: &LayoutJob) -> Option<Turned> {
    if !para.rtl && !script::has_rtl(&job.text) {
        return None;
    }
    let chars: Vec<char> = job.text.chars().collect();
    if chars.len() > MAX_TURNED_CHARS {
        return None;
    }
    let levels = script::levels(&job.text, Some(para.rtl));
    let sections = sections_of(job);
    let shaped = joined_across(ui, job, &chars, &sections);
    let part = |c: char| match c {
        '\n' => Part::Break,
        c if c.is_whitespace() => Part::Blank,
        _ => Part::Word,
    };
    let mut words: Vec<Word> = Vec::new();
    for (at, c) in chars.iter().enumerate() {
        let (section, level, part) = (sections.get(at).copied()?, levels.get(at).copied()?, part(*c));
        let rtl = part == Part::Word && script::is_rtl(*c);
        match words.last_mut() {
            Some(last) if part != Part::Break && last.part == part && last.section == section && last.level == level && last.rtl == rtl => {
                last.chars.1 = at + 1
            }
            _ => words.push(Word { chars: (at, at + 1), section, level, rtl, part, wide: 0.0 }),
        }
        if words.len() > MAX_TURNED_WORDS {
            return None;
        }
    }
    // The room before a section stands before the first of its words.
    let mut led = vec![false; job.sections.len()];
    let mut texts: Vec<(String, f32)> = Vec::with_capacity(words.len());
    for word in &mut words {
        let from = job.sections.get(word.section)?;
        let first = led.get_mut(word.section).is_some_and(|led| !std::mem::replace(led, true));
        let lead = if first { from.leading_space } else { 0.0 };
        if word.part == Part::Break {
            texts.push(("\n".to_string(), 0.0));
            continue;
        }
        let text = spelled(shaped.get(word.chars.0..word.chars.1)?, word.level);
        let mut alone = LayoutJob::default();
        alone.append(&text, lead, from.format.clone());
        word.wide = ui.painter().layout_job(alone).size().x;
        texts.push((text, lead));
    }
    // Lines break before the word there is no room for; blanks stay where they are. A word is
    // all its pieces up to the next blank.
    let wrap = job.wrap.max_width;
    let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
    let mut x = 0.0f32;
    for (n, word) in words.iter().enumerate() {
        let has_words = lines.last().is_some_and(|line| !line.is_empty());
        let starts =
            word.part == Part::Word && !n.checked_sub(1).and_then(|before| words.get(before)).is_some_and(|before| before.part == Part::Word);
        let whole: f32 =
            if starts { words.get(n..).unwrap_or_default().iter().take_while(|w| w.part == Part::Word).map(|w| w.wide).sum() } else { 0.0 };
        if word.part == Part::Break || (starts && has_words && x + whole > wrap + 0.5) {
            lines.push(Vec::new());
            x = 0.0;
        }
        if word.part != Part::Break
            && let Some(line) = lines.last_mut()
        {
            line.push(n);
            x += word.wide;
        }
    }
    let holders: Vec<usize> = words.iter().enumerate().filter(|(_, word)| holds_picture(job, word.section)).map(|(n, _)| n).collect();
    let mut out = LayoutJob { text: String::new(), sections: Vec::new(), ..job.clone() };
    out.wrap.max_width = f32::INFINITY;
    let mut rows = Vec::with_capacity(lines.len());
    let mut backwards = Vec::with_capacity(lines.len());
    let mut pictures = Vec::new();
    let mut tabs = Vec::new();
    // A tab is a piece of its own: it has a look of its own (`tab_format`).
    let is_tab = |word: &Word| word.part == Part::Blank && chars.get(word.chars.0) == Some(&'\t');
    let tab_of: Vec<usize> = {
        let mut met = 0usize;
        words
            .iter()
            .map(|word| {
                if !is_tab(word) {
                    return usize::MAX;
                }
                met += 1;
                met - 1
            })
            .collect()
    };
    let mut last_section = 0usize;
    for (n, line) in lines.iter().enumerate() {
        let stand_in = |w: &usize| match words.get(*w) {
            Some(word) if is_tab(word) => '\t',
            Some(word) if word.part == Part::Blank => ' ',
            _ => 'a',
        };
        let stand_ins: Vec<char> = line.iter().map(stand_in).collect();
        let depth: Vec<u8> = line.iter().map(|w| words.get(*w).map_or(0, |word| word.level)).collect();
        let wide: f32 = line.iter().filter_map(|w| words.get(*w)).map(|word| word.wide).sum();
        let mut x = match job.halign {
            Align::Min => 0.0,
            Align::Center => -wide / 2.0,
            Align::Max => -wide,
        };
        let mut row = Vec::with_capacity(line.len());
        let mut back = Vec::with_capacity(line.len());
        for i in script::order(&stand_ins, &depth, u8::from(para.rtl)) {
            let at = line.get(i).copied()?;
            let (word, (text, lead)) = (words.get(at)?, texts.get(at)?);
            section_of_its_own(&mut out, text, *lead, job.sections.get(word.section)?.format.clone());
            row.push((x, x + word.wide, word.chars));
            back.push(word.level % 2 == 1);
            x += word.wide;
            last_section = word.section;
            if let Ok(nth) = holders.binary_search(&at) {
                pictures.push(nth);
            }
            if is_tab(word) {
                tabs.push(tab_of.get(at).copied()?);
            }
        }
        rows.push(row);
        backwards.push(back);
        if n + 1 < lines.len() {
            section_of_its_own(&mut out, "\n", 0.0, job.sections.get(last_section)?.format.clone());
        }
    }
    Some(Turned { job: out, rows, pictures, tabs, backwards })
}

/// Selecting in a paragraph drawn in another order than it is stored: a press starts it, a
/// drag carries it on (with Shift a press carries it on too), a double-click takes a word and
/// a triple-click the paragraph. What is selected is lit where each letter is seen, and is
/// copied in the order it is stored, which is the order it is read in.
fn select_turned(ui: &Ui, response: &egui::Response, galley: &egui::Galley, turned: &Turned, stored: &[char], rtl: bool, pos: egui::Pos2) {
    let id = TurnedPick::id();
    let before: Option<TurnedPick> = ui.data(|d| d.get_temp(id));
    let mut pick = before;
    let spots = char_spots(galley, turned);
    let mine = pick.is_some_and(|p| p.para == response.id);
    let pressed = ui.input(|i| i.pointer.any_pressed());
    let shift = ui.input(|i| i.modifiers.shift);
    if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
        let hit = || turned_hit(galley, &spots, turned, rtl, pointer - pos);
        if pressed && response.hovered() {
            if let Some(at) = hit() {
                pick = Some(match pick.filter(|_| mine && shift) {
                    Some(p) => TurnedPick { cursor: at, ..p },
                    None => TurnedPick { para: response.id, anchor: at, cursor: at },
                });
            }
        } else if pressed && mine && !shift {
            // A press anywhere else lets go of it.
            pick = None;
        } else if response.dragged()
            && mine
            && let (Some(p), Some(at)) = (pick.as_mut(), hit())
        {
            p.cursor = at;
        }
    }
    if response.double_clicked()
        && let Some(p) = pick.as_mut().filter(|p| p.para == response.id)
    {
        let at = p.cursor.min(stored.len());
        let word = |c: &char| c.is_alphanumeric() || *c == '_' || *c == '\u{0640}';
        let from = stored.get(..at).unwrap_or_default().iter().rposition(|c| !word(c)).map_or(0, |n| n + 1);
        let to = stored.get(at..).unwrap_or_default().iter().position(|c| !word(c)).map_or(stored.len(), |n| at + n);
        (p.anchor, p.cursor) = (from, to);
    }
    if response.triple_clicked()
        && let Some(p) = pick.as_mut().filter(|p| p.para == response.id)
    {
        (p.anchor, p.cursor) = (0, stored.len());
    }
    if let Some(p) = pick.filter(|p| p.para == response.id) {
        let (from, to) = p.range();
        let fill = ui.visuals().selection.bg_fill;
        for (row, left, right) in spots.iter().skip(from).take(to.saturating_sub(from)).flatten() {
            if let Some(line) = galley.rows.get(*row) {
                let rect = Rect::from_min_max(pos2(pos.x + left, pos.y + line.pos.y), pos2(pos.x + right, pos.y + line.pos.y + line.size.y));
                ui.painter().rect_filled(rect, 0.0, fill);
            }
        }
        if from < to && ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy | egui::Event::Cut))) {
            let text: String = stored.get(from..to.min(stored.len())).unwrap_or_default().iter().collect();
            ui.ctx().copy_text(text);
        }
    }
    if pick != before {
        ui.data_mut(|d| match pick {
            Some(p) => {
                d.insert_temp(id, p);
            }
            None => d.remove::<TurnedPick>(id),
        });
    }
}

/// Whether a section of a text to lay out is the space that holds a picture's place
/// ([`holder`]).
fn holds_picture(job: &LayoutJob, section: usize) -> bool {
    job.sections
        .get(section)
        .is_some_and(|s| s.format.color == Color32::TRANSPARENT && s.format.valign == Align::BOTTOM && s.format.line_height.is_some())
}

/// As [`spots`], for a text drawn in another order than it is stored: `order` says which of
/// `placed` each picture met is.
fn spots_turned(galley: &egui::Galley, placed: &[Placed], order: &[usize]) -> Vec<Rect> {
    let mut out = vec![Rect::NOTHING; placed.len()];
    let mut next = order.iter();
    let mut want = next.next();
    'rows: for row in &galley.rows {
        for glyph in &row.glyphs {
            let Some(p) = want.and_then(|nth| placed.get(*nth)) else { break 'rows };
            if glyph.chr == p.holder.0 && (glyph.line_height - p.holder.1).abs() < 0.75 {
                let bottom = row.pos.y + row.size.y - BASELINE;
                if let Some(slot) = want.and_then(|nth| out.get_mut(*nth)) {
                    *slot = Rect::from_min_size(pos2((row.pos.x + glyph.pos.x).round(), (bottom - p.size.y).round()), p.size);
                }
                want = next.next();
            }
        }
    }
    out
}

/// What fills the room of the tabs of a laid out text: marks from the end of what is before
/// each tab to the tab, on the line of the letters. `pos` is the text's own corner. `order`
/// says which of the text's tabs each tab met is, for a text drawn in another order than it
/// is stored.
fn draw_leaders(ui: &Ui, galley: &egui::Galley, leaders: &[Lead], order: Option<&[usize]>, pos: egui::Pos2, color: Color32) {
    if leaders.is_empty() {
        return;
    }
    let painter = ui.painter();
    let mut nth = 0usize;
    for row in &galley.rows {
        for glyph in &row.glyphs {
            if glyph.chr != '\t' {
                continue;
            }
            let this = match order {
                Some(order) => order.get(nth).copied().unwrap_or(usize::MAX),
                None => nth,
            };
            nth = nth.saturating_add(1);
            let Some(lead) = leaders.iter().find(|l| l.nth == this) else { continue };
            let (room, size) = (lead.room, lead.size.clamp(8.0, 64.0));
            let end = pos.x + row.pos.x + glyph.pos.x - size * 0.2;
            let start = end - room + size * 0.4;
            if !(start.is_finite() && end.is_finite()) || end - start < size * 0.5 {
                continue;
            }
            let base = (pos.y + row.pos.y + row.size.y - size * 0.3).round() - 1.0;
            let line = |y: f32, thick: f32| {
                painter.rect_filled(Rect::from_min_max(pos2(start.round(), y), pos2(end.round(), y + thick)), 0.0, color);
            };
            let marks = |y: f32, mark: f32, step: f32| {
                let mut at = start.round();
                let mut drawn = 0usize;
                while at + mark <= end && drawn < MAX_DASHES {
                    painter.rect_filled(Rect::from_min_size(pos2(at, y), vec2(mark, (size / 12.0).round().max(1.0))), 0.0, color);
                    at += step;
                    drawn += 1;
                }
            };
            let thin = (size / 14.0).round().max(1.0);
            match lead.with {
                rich::Leader::None => {}
                rich::Leader::Dots => marks(base, thin.max(2.0), (size * 0.3).round().max(4.0)),
                rich::Leader::MiddleDots => marks(base - (size * 0.25).round(), thin.max(2.0), (size * 0.3).round().max(4.0)),
                rich::Leader::Hyphens => marks(base - (size * 0.22).round(), (size * 0.25).round().max(2.0), (size * 0.45).round().max(4.0)),
                rich::Leader::Line => line(base + 1.0, thin),
                rich::Leader::ThickLine => line(base + 1.0, thin * 2.0),
                rich::Leader::Equals => {
                    line(base - (size * 0.18).round(), thin);
                    line(base - (size * 0.18).round() - thin * 2.0 - 1.0, thin);
                }
            }
        }
    }
}

/// How a tab is set: the tab itself in letters too small to take room, so that the room
/// before it is all the room it takes. Gives its format and the room it takes of itself.
fn tab_format(ui: &Ui, format: &TextFormat) -> (TextFormat, f32) {
    let font_id = FontId::new(1.0, format.font_id.family.clone());
    let own = ui.ctx().fonts_mut(|fonts| fonts.glyph_width(&font_id, '\t'));
    let tab = TextFormat { font_id, underline: Stroke::NONE, strikethrough: Stroke::NONE, background: Color32::TRANSPARENT, ..format.clone() };
    (tab, if own.is_finite() { own.max(0.0) } else { 0.0 })
}

/// The width of the text that follows a tab, up to the next tab or the end of the line, and
/// its width up to its decimal point (all of it where it has none). `rest` is what follows
/// the tab in run `at`, set in `font`.
fn ahead(ui: &Ui, runs: &[Run], look: Look, at: usize, rest: &str, font: &FontId) -> (f32, f32) {
    const RUNS_AHEAD: usize = 64;
    let (mut wide, mut point) = (0.0f32, None);
    // Gives whether the line or the room up to the next tab ended in `text`.
    let mut take = |text: &str, font: &FontId| {
        let end = text.find(['\t', '\n']);
        let part = text.get(..end.unwrap_or(text.len())).unwrap_or_default();
        if point.is_none()
            && let Some(dot) = part.find('.')
        {
            point = Some(wide + text_width(ui.painter(), part.get(..dot).unwrap_or_default(), font.clone()));
        }
        wide += text_width(ui.painter(), part, font.clone());
        end.is_some()
    };
    if !take(rest, font) {
        for run in runs.iter().skip(at.saturating_add(1)).take(RUNS_AHEAD) {
            if run.picture.is_some() {
                continue;
            }
            let (text, format) = section(run, look);
            if take(&text, &format.font_id) {
                break;
            }
        }
    }
    (wide, point.unwrap_or(wide))
}

/// Where each picture of `placed` landed in the laid out text, from the text's own corner.
fn spots(galley: &egui::Galley, placed: &[Placed]) -> Vec<Rect> {
    let mut out = Vec::with_capacity(placed.len());
    let mut next = placed.iter();
    let mut want = next.next();
    'rows: for row in &galley.rows {
        for glyph in &row.glyphs {
            let Some(p) = want else { break 'rows };
            // Pictures come in the order of the text, each held by its space on its own tall line.
            if glyph.chr == p.holder.0 && (glyph.line_height - p.holder.1).abs() < 0.75 {
                let bottom = row.pos.y + row.size.y - BASELINE;
                out.push(Rect::from_min_size(pos2((row.pos.x + glyph.pos.x).round(), (bottom - p.size.y).round()), p.size));
                want = next.next();
            }
        }
    }
    out
}

/// Where a piece of a paragraph's text goes in the room the paragraph has.
struct Spot {
    /// Room left free before the text, for a picture at the paragraph's left.
    left: f32,
    width: f32,
    min_height: f32,
    /// Draw the list marker beside it.
    marker: bool,
}

/// Runs of one paragraph drawn as text, with the pictures that stand in it. Returns the room
/// it took.
fn piece(ui: &mut Ui, para: &Para, runs: &[Run], look: Look, spot: Spot) -> Rect {
    let (inset, _) = room(ui, para);
    let (job, ends, placed, leaders) = job_and_leaders(ui, para, runs, look, spot.width);
    // Right-to-left writing is drawn in another order than it is stored.
    let turned = turned(ui, para, &job).map(|turned| (ui.painter().layout_job(turned.job.clone()), turned));
    let stored: Option<Vec<char>> = turned.as_ref().map(|_| job.text.chars().collect());
    let (galley, turned) = match turned {
        // One line drawn for each that was made, or the text is drawn as it is stored.
        Some((galley, turned)) if galley.rows.len() == turned.rows.len() => (galley, Some(turned)),
        _ => (ui.painter().layout_job(job), None),
    };

    let height = (galley.size().y + 2.0).max(spot.min_height);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag() - Sense::FOCUSABLE);
    if !ui.is_rect_visible(rect) {
        return rect;
    }
    // What a paragraph is set in by, it is set in by on the side it starts at: the right for
    // one written from the right.
    let text_left = rect.left() + spot.left + if para.rtl { 0.0 } else { inset };
    // Never past the room there is: a narrow table cell has less than the least a text wraps at.
    let wide = spot.width.min((rect.right() - text_left).max(0.0));
    let pos = match para.align {
        rich::Align::Left => pos2(text_left, rect.top()),
        rich::Align::Center => pos2(text_left + wide / 2.0, rect.top()),
        rich::Align::Right => pos2(text_left + wide, rect.top()),
    };
    quote_bars(ui, para, rect, look.t);
    if spot.marker
        && let ParaKind::Item(marker) = &para.kind
    {
        let marker: String = marker.chars().take(12).collect();
        // In the letters of the text it stands before, so that it sits on the same line.
        let like = runs.iter().find(|r| r.picture.is_none()).map_or_else(|| font(BODY_PX), |r| section(r, look).1.font_id);
        if para.rtl {
            // Read from the right, as its item is: the stop after a number stands at its left.
            let galley = ui.painter().layout_job(crate::seen_line_from(&marker, Some(true), like, look.text()));
            ui.painter().galley(pos2(rect.right() - inset + 7.0, rect.top()), galley, look.text());
        } else {
            ui.painter().text(pos2(rect.left() + inset - 7.0, rect.top()), egui::Align2::RIGHT_TOP, marker, like, look.text());
        }
    }

    let spots = match &turned {
        Some(turned) => spots_turned(&galley, &placed, &turned.pictures),
        None => spots(&galley, &placed),
    };
    let spots: Vec<Rect> = spots.into_iter().map(|at| at.translate(pos.to_vec2())).collect();
    // A picture not downloaded is a place of its own, that a right-click on offers to download.
    for (n, (p, at)) in placed.iter().zip(&spots).enumerate() {
        if not_downloaded(p.run, p.stand) && ui.is_rect_visible(*at) {
            let spot = ui.interact(*at, response.id.with(("not downloaded", n)), Sense::click());
            offer_download(&spot, p.run, p.stand, look);
            let tip = tip(p.run, p.stand);
            if !tip.is_empty() {
                spot.on_hover_text_at_pointer(tip);
            }
        }
    }
    // Where the pointer is: on a picture, or in the text, where a link shows its address and
    // nothing else happens.
    if let Some(pointer) = response.hover_pos() {
        if let Some((p, _)) = placed.iter().zip(&spots).find(|(_, at)| at.contains(pointer)) {
            let mut tip = tip(p.run, p.stand);
            if let Some(concern) = runs.iter().position(|r| std::ptr::eq(r, p.run)).and_then(|at| openmailclient_formats::links::concern_at(runs, at))
            {
                tip = format!("{}\n{tip}", concern.words());
            }
            if !tip.is_empty() {
                response.clone().on_hover_text_at_pointer(tip);
            }
            if let Some(link) = &p.run.style.link {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if response.clicked() {
                    look.link.replace(Some(link.clone()));
                }
            }
        } else {
            // The character under the pointer, as the text stores it.
            let under = match &turned {
                Some(turned) => {
                    let (x, y) = (pointer.x - pos.x, pointer.y - pos.y);
                    let row = galley.rows.iter().position(|row| y >= row.pos.y && y < row.pos.y + row.size.y);
                    row.and_then(|n| turned.rows.get(n))
                        .and_then(|row| row.iter().find(|(from, to, _)| x >= *from && x < *to))
                        .map(|(.., chars)| chars.0)
                }
                None => {
                    let cursor = galley.cursor_from_pos(pointer - pos);
                    let at = galley.pos_from_cursor(cursor).translate(pos.to_vec2());
                    ((pointer.x - at.center().x).abs() < 16.0 && at.y_range().contains(pointer.y)).then_some(cursor.index.0)
                }
            };
            if let Some(index) = under {
                let at = ends.iter().position(|end| index < *end);
                if let Some(link) = at.and_then(|i| runs.get(i)).and_then(|r| r.style.link.as_ref()) {
                    let shown: String = link.chars().filter(|c| !c.is_control()).take(300).collect();
                    let concern =
                        at.and_then(|i| openmailclient_formats::links::concern_at(runs, i)).map(|c| format!("{}\n", c.words())).unwrap_or_default();
                    response.clone().on_hover_text_at_pointer(format!("{concern}{shown}\n{LINK_HINT}"));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    if response.clicked() {
                        look.link.replace(Some(link.clone()));
                    }
                }
            }
        }
    }
    if turned.is_none() {
        // Borders round text within a line. Text drawn in another order than it is stored has
        // none yet.
        for (at, sides) in outlines(&galley, runs, &ends, pos) {
            edges(ui, at, &sides, look, look.text());
        }
    }
    if let (Some(turned), Some(stored)) = (&turned, &stored) {
        draw_leaders(ui, &galley, &leaders, Some(&turned.tabs), pos, look.text());
        select_turned(ui, &response, &galley, turned, stored, para.rtl, pos);
        ui.painter().galley(pos, galley, look.text());
    } else {
        draw_leaders(ui, &galley, &leaders, None, pos, look.text());
        LabelSelectionState::label_text_selection(ui, &response, pos, galley, look.text(), Stroke::NONE);
    }
    for (p, at) in placed.iter().zip(&spots) {
        if ui.is_rect_visible(*at) {
            picture(ui, p.run, p.stand, *at, look);
        }
    }
    rect
}

/// The text and format of one run as it is drawn.
fn section(run: &Run, look: Look) -> (String, TextFormat) {
    let s = &run.style;
    let mut px = s.size.map_or(BODY_PX, |pt| pt * 4.0 / 3.0).clamp(8.0, 64.0);
    let mut format = TextFormat::default();
    match s.script {
        Script::None => {}
        Script::Super => {
            px *= 0.75;
            format.valign = Align::TOP;
        }
        Script::Sub => {
            px *= 0.75;
            format.valign = Align::BOTTOM;
        }
    }
    // The font the run names, when it is installed; otherwise the reader's own.
    let named = s.family.as_deref().and_then(|list| look.faces.pick(list, s.bold, s.italic));
    // A font's own italic face is drawn as it is; without one, the upright face is slanted.
    format.italics = named.as_ref().map_or(s.italic, |picked| picked.slant);
    format.font_id = match (named, s.mono, s.bold) {
        (Some(picked), ..) => FontId::new(px, picked.family),
        (None, true, _) => FontId::new(px, FontFamily::Monospace),
        (None, false, true) => bold(px),
        (None, false, false) => font(px),
    };
    let (color, background) = colors(s.color, s.background, look);
    format.color = color;
    format.background = background;
    if let Some(Picture::File(_)) = &run.picture {
        // Where there is no room for its card, an attachment is named in words.
        format.color = faint(look);
        format.background = Color32::TRANSPARENT;
        return (alt(run), format);
    }
    if let Some(picture) = &run.picture {
        let what = missing(picture);
        let alt = alt(run);
        format.color = faint(look);
        format.background = Color32::TRANSPARENT;
        format.font_id = font(px.min(13.5));
        return (if alt.is_empty() { format!("[{what}]") } else { format!("[{what}: {alt}]") }, format);
    }
    if s.link.is_some() && s.color.is_none() && s.background.is_none() && contrast(look.t.accent_dark, look.on) >= 3.0 {
        format.color = look.t.accent_dark;
    }
    // A link is underlined as its style says: Word's signatures turn it off.
    if s.underline {
        format.underline = Stroke::new(1.0, format.color);
    }
    if s.strike {
        format.strikethrough = Stroke::new(1.0, format.color);
    }
    (run.text.clone(), format)
}

/// Text that asks for no colour, on a colour the message chose.
fn plain_on(background: Color32) -> Color32 {
    if luminance(background) > 0.4 { Color32::from_rgb(0x1A, 0x1A, 0x1A) } else { Color32::WHITE }
}

/// Text and highlight colours for a run. A message is written for a white page; here a colour is
/// kept only if it can be read on what is behind it, otherwise it is moved toward the plain text
/// colour there.
fn colors(color: Option<rich::Rgb>, background: Option<rich::Rgb>, look: Look) -> (Color32, Color32) {
    let rgb = |c: rich::Rgb| Color32::from_rgb(c[0], c[1], c[2]);
    match background.map(rgb) {
        Some(bg) => {
            let fg = color.map(rgb).filter(|c| contrast(*c, bg) >= 2.0).unwrap_or(plain_on(bg));
            (fg, bg)
        }
        None => {
            let plain = look.text();
            let Some(c) = color.map(rgb) else { return (plain, Color32::TRANSPARENT) };
            let readable =
                [0.0, 0.4, 0.7, 1.0].into_iter().map(|k| c.lerp_to_gamma(plain, k)).find(|c| contrast(*c, look.on) >= 3.0).unwrap_or(plain);
            (readable, Color32::TRANSPARENT)
        }
    }
}

/// Relative luminance and contrast ratio (WCAG 2.1, "relative luminance" and "contrast ratio").
fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// A border's own colour, made only as much darker or lighter as it needs to be seen on what is
/// behind it: a pale line on white stays a pale line of its own hue, not the colour of text.
fn ink(c: [u8; 3], look: Look) -> Color32 {
    let own = Color32::from_rgb(c[0], c[1], c[2]);
    let toward = look.text();
    let mut seen = own;
    for step in 1..=10u8 {
        if contrast(seen, look.on) >= EDGE_CONTRAST {
            return seen;
        }
        seen = own.lerp_to_gamma(toward, f32::from(step) / 10.0);
    }
    seen
}

fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The width blocks would take with all the room they could want.
fn natural_width(ui: &Ui, list: &[Block], look: Look, depth: usize) -> f32 {
    let mut widest = 0.0f32;
    for block in list.iter().take(MEASURED_BLOCKS) {
        let width = match block {
            Block::Para(p) => {
                let (mut job, ..) = job(ui, p, &p.runs, look, f32::INFINITY);
                job.halign = Align::LEFT;
                let border = p.frame.as_ref().map_or(0.0, |frame| {
                    let (near, far) = frame_room(frame);
                    near.x + far.x + f32::from(frame.indent.min(12)) * INDENT
                });
                inset(p) + ui.painter().layout_job(job).size().x + 2.0 + border
            }
            Block::Rule => 0.0,
            Block::Table(t) => match t.width {
                TableWidth::Pixels(w) => f32::from(w),
                TableWidth::Percent(_) | TableWidth::Fill => UNMEASURED,
                _ if depth < MAX_TABLE_DEPTH && t.columns() <= MAX_COLUMNS => {
                    let columns = natural_columns(ui, t, look, depth);
                    columns.iter().sum::<f32>() + spacing(t) * (columns.len() as f32 + 1.0) + rim(t) * 2.0
                }
                _ => UNMEASURED,
            },
        };
        if width.is_finite() {
            widest = widest.max(width);
        }
    }
    widest
}

/// What a cell is filled with. A message is written for white paper, and a pale fill is drawn
/// as it is on the light pane; on the dark pane, where nothing else is behind it, it is the pane.
fn cell_fill(cell: &rich::Cell, look: Look) -> Option<Color32> {
    fill_on(cell.background?, look)
}

/// A fill the message chose, as it is drawn on what is behind it.
fn fill_on(c: rich::Rgb, look: Look) -> Option<Color32> {
    let c = Color32::from_rgb(c[0], c[1], c[2]);
    let pale = luminance(c) >= PALE_FILL;
    (!(pale && look.t.dark && look.on == look.t.surface)).then_some(c)
}

/// The room a padding takes across: at the right and at the left.
fn sides(pad: [f32; 4]) -> f32 {
    pad[1] + pad[3]
}

/// Room between a cell's edges and what it holds: above, right, below and left.
fn padding(table: &Table, cell: &rich::Cell) -> [f32; 4] {
    let [top, right, bottom, left] = cell.padding.unwrap_or([table.padding; 4]);
    // A cell's own lines are drawn inside it: what they take past a hair line is not the text's.
    let line = |a: usize, b: usize| {
        let wide = |i: usize| cell.edges.and_then(|e| e.get(i).map(|e| e.width.min(MAX_EDGE))).unwrap_or(0);
        f32::from(wide(a).max(wide(b)).saturating_sub(1))
    };
    let room = |v: u16| f32::from(v.min(MAX_CELL_ROOM));
    [room(top) + line(0, 2), room(right) + line(1, 3), room(bottom) + line(0, 2), room(left) + line(1, 3)]
}

/// Room a table's own border takes on each side when it is thicker than a hair line.
fn rim(table: &Table) -> f32 {
    (table.frame.iter().map(|e| f32::from(e.width.min(MAX_EDGE))).fold(0.0, f32::max) - 1.0).max(0.0)
}

/// Room between the cells of a table.
fn spacing(table: &Table) -> f32 {
    f32::from(table.spacing.min(MAX_CELL_ROOM))
}

/// Give the columns that ask for a share of the table's width that share of `room`, and the
/// room that is left to the others.
fn shares(table: &Table, widths: &mut [f32], room: f32) {
    let mut asked = vec![0.0f32; widths.len()];
    let places = table.places();
    for (row, starts) in table.rows.iter().zip(&places).take(MEASURED_ROWS) {
        for (cell, col) in row.iter().zip(starts) {
            if cell.span <= 1
                && let Some(a) = asked.get_mut(*col)
            {
                *a = a.max(f32::from(cell.share.min(100)) / 100.0);
            }
        }
    }
    let claimed: f32 = asked.iter().sum();
    if claimed <= 0.0 {
        return;
    }
    // Shares that add up to more than everything are cut down together.
    let scale = if claimed > 1.0 { 1.0 / claimed } else { 1.0 };
    let others: f32 = widths.iter().zip(&asked).filter(|(_, a)| **a <= 0.0).map(|(w, _)| *w).sum();
    let left = (room - room * claimed * scale).max(0.0);
    for (w, a) in widths.iter_mut().zip(&asked) {
        if *a > 0.0 {
            *w = room * a * scale;
        } else if others > 0.0 {
            *w *= left / others;
        }
    }
    // With no other column to take what is left, the sharing columns take it.
    let total: f32 = widths.iter().sum();
    if total > 0.0 && (others <= 0.0 || left <= 0.0) {
        widths.iter_mut().for_each(|w| *w *= room / total);
    }
}

/// The width each column asks for, or else wants for what it holds.
fn natural_columns(ui: &Ui, table: &Table, look: Look, depth: usize) -> Vec<f32> {
    let columns = table.columns();
    let mut asked = vec![0.0f32; columns];
    let mut wanted = vec![MIN_COLUMN; columns];
    let mut wide: Vec<(usize, usize, f32)> = Vec::new();
    let places = table.places();
    for (row, starts) in table.rows.iter().zip(&places).take(MEASURED_ROWS) {
        for (cell, col) in row.iter().zip(starts) {
            let col = *col;
            let span = usize::from(cell.span.max(1));
            let on = cell_fill(cell, look).unwrap_or(look.on);
            let want = natural_width(ui, &cell.blocks, Look { on, ..look }, depth + 1) + sides(padding(table, cell));
            if span == 1 {
                if let Some(a) = asked.get_mut(col) {
                    *a = a.max(f32::from(cell.width));
                }
                if let Some(w) = wanted.get_mut(col) {
                    *w = w.max(want);
                }
            } else {
                wide.push((col, span, want.max(f32::from(cell.width))));
            }
        }
    }
    if table.rows.len() > MEASURED_ROWS {
        wanted.iter_mut().for_each(|w| *w = w.max(UNMEASURED));
    }
    let mut widths: Vec<f32> = asked.iter().zip(&wanted).map(|(a, w)| if *a > 0.0 { a.max(MIN_COLUMN) } else { *w }).collect();
    // A cell over several columns that wants more than they have widens the last of them.
    for (col, span, want) in wide {
        let have: f32 = widths.iter().skip(col).take(span).sum();
        if want > have
            && let Some(last) = widths.get_mut(col.saturating_add(span).saturating_sub(1))
        {
            *last += want - have;
        }
    }
    widths
}

fn table(ui: &mut Ui, table: &Table, look: Look, depth: usize) {
    let columns = table.columns();
    let available = ui.available_width();
    if columns == 0 || columns > MAX_COLUMNS || depth >= MAX_TABLE_DEPTH || available / (columns as f32) < 48.0 {
        // Too many columns or too little room: one cell under another, in reading order.
        for cell in table.rows.iter().flatten() {
            blocks(ui, &cell.blocks, look, depth + 1);
        }
        return;
    }
    // A table is as wide as it asks to be, or as what it holds, and never wider than the room.
    // Its cells stand apart by the table's spacing, which also runs round the outside, inside
    // the table's own border.
    let mut widths = natural_columns(ui, table, look, depth);
    let total: f32 = widths.iter().sum();
    let gaps = columns as f32 + 1.0;
    let rim = rim(table);
    let target = match table.width {
        TableWidth::Pixels(w) => f32::from(w).min(available),
        TableWidth::Percent(share) => available * f32::from(share.clamp(1, 100)) / 100.0,
        TableWidth::Fill => available,
        TableWidth::Content => (total + spacing(table) * gaps + rim * 2.0).min(available),
    };
    let rim = rim.min(target / 4.0);
    // Spacing never takes more than half the table.
    let gap = spacing(table).min((target - rim * 2.0) / gaps / 2.0).max(0.0);
    let cells = (target - rim * 2.0 - gap * gaps).max(0.0);
    if total > 0.0 && total.is_finite() {
        widths.iter_mut().for_each(|w| *w *= cells / total);
    } else {
        widths = vec![cells / columns as f32; columns];
    }
    if table.width != TableWidth::Content {
        shares(table, &mut widths, cells);
    }
    let shift = match table.align {
        rich::Align::Left => 0.0,
        rich::Align::Center => ((available - target) / 2.0).max(0.0),
        rich::Align::Right => (available - target).max(0.0),
    };
    let room_left = ui.cursor().left();
    let left = room_left + shift;
    let table_top = ui.cursor().top();
    let lefts = table.lefts(&widths, left + rim + gap, left + target - rim - gap, gap);

    // Where each cell stands: a cell may cover several columns, and several rows.
    struct Slot<'a> {
        row: usize,
        /// The last row the cell covers.
        upto: usize,
        number: usize,
        cell: &'a rich::Cell,
        x: f32,
        w: f32,
        /// Its padding: above, right, below and left.
        pad: [f32; 4],
        /// How tall the cell is with what it holds.
        tall: f32,
    }
    let count = table.rows.len();
    let places = table.places();
    let mut slots: Vec<Slot> = Vec::new();
    for (r, (row, starts)) in table.rows.iter().zip(&places).enumerate() {
        for (c, (cell, col)) in row.iter().zip(starts).enumerate() {
            let span = usize::from(cell.span.max(1)).min(columns.saturating_sub(*col));
            let Some(x) = lefts.get(table.left_column(*col, span, columns)).copied() else { continue };
            let covered = widths.iter().skip(*col).take(usize::from(cell.span.max(1)));
            let w: f32 = covered.clone().sum::<f32>() + gap * (covered.count().max(1) - 1) as f32;
            if w <= 0.0 {
                continue;
            }
            let upto = r.saturating_add(usize::from(cell.rows.max(1)) - 1).min(count.saturating_sub(1));
            slots.push(Slot { row: r, upto, number: c, cell, x, w, pad: padding(table, cell), tall: 0.0 });
        }
    }
    // What the table is filled with is behind each of its cells that has no fill of its own.
    let fill = table.fill.and_then(|c| fill_on(c, look));
    let inner = Look { on: fill.unwrap_or(look.on), ..look };
    let on = |cell: &rich::Cell| cell_fill(cell, inner).unwrap_or(inner.on);
    let inside =
        |slot: &Slot, top: f32| Rect::from_min_size(pos2(slot.x + slot.pad[3], top + slot.pad[0]), vec2((slot.w - sides(slot.pad)).max(8.0), 0.0));

    // Every cell is laid out once unseen, to learn how tall it is: a row is as tall as its
    // tallest cell, and a cell over several rows makes the last of them tall enough for it.
    for slot in &mut slots {
        let mut child = ui.new_child(UiBuilder::new().id_salt(("cell size", slot.row, slot.number)).max_rect(inside(slot, table_top)).invisible());
        child.spacing_mut().item_spacing = vec2(0.0, 0.0);
        blocks(&mut child, &slot.cell.blocks, Look { on: on(slot.cell), ..look }, depth + 1);
        slot.tall = child.min_rect().height() + slot.pad[0] + slot.pad[2];
    }
    let mut heights = vec![0.0f32; count];
    for slot in slots.iter().filter(|s| s.upto == s.row) {
        if let Some(h) = heights.get_mut(slot.row) {
            *h = h.max(slot.tall);
        }
    }
    let mut reaching: Vec<&Slot> = slots.iter().filter(|s| s.upto > s.row).collect();
    reaching.sort_by_key(|s| s.upto);
    let mut reaching = reaching.into_iter().peekable();
    // Where each row starts, below the first one's top.
    let mut tops = vec![0.0f32; count];
    let mut y = 0.0f32;
    for r in 0..count {
        if let Some(top) = tops.get_mut(r) {
            *top = y;
        }
        while let Some(slot) = reaching.next_if(|s| s.upto <= r) {
            let start = tops.get(slot.row).copied().unwrap_or(y);
            if let Some(h) = heights.get_mut(r) {
                *h = h.max(slot.tall - (y - start));
            }
        }
        y += heights.get(r).copied().unwrap_or(0.0) + gap;
    }
    let first = table_top + rim + gap;
    let bottom = first + y + rim;

    let whole = Rect::from_min_max(pos2(room_left, table_top), pos2(room_left + available, bottom));
    if ui.is_visible() && ui.is_rect_visible(whole) {
        let lines = table.line.map_or(look.t.text_faint, |c| ink(c, inner));
        let outer = Rect::from_min_max(pos2(left, table_top), pos2(left + target, bottom));
        let round = corners(&table.frame, table.radius, outer);
        if let Some(fill) = fill {
            ui.painter().rect_filled(outer, round, fill);
        }
        for slot in &slots {
            let top = first + tops.get(slot.row).copied().unwrap_or(0.0);
            let end = first + tops.get(slot.upto).copied().unwrap_or(0.0) + heights.get(slot.upto).copied().unwrap_or(0.0);
            let rect = Rect::from_min_max(pos2(slot.x, top), pos2(slot.x + slot.w, end.max(top)));
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let within = Look { on: on(slot.cell), ..inner };
            let own = slot.cell.edges.unwrap_or_default();
            let round = corners(&own, slot.cell.radius, rect);
            if cell_fill(slot.cell, look).is_some() {
                ui.painter().rect_filled(rect, round, within.on);
            }
            let spare = (rect.height() - slot.tall).max(0.0);
            let down = match slot.cell.valign {
                VAlign::Top => 0.0,
                VAlign::Middle => (spare / 2.0).floor(),
                VAlign::Bottom => spare,
            };
            let mut child = ui.new_child(UiBuilder::new().id_salt(("cell", slot.row, slot.number)).max_rect(inside(slot, top + down)));
            child.spacing_mut().item_spacing = vec2(0.0, 0.0);
            blocks(&mut child, &slot.cell.blocks, within, depth + 1);
            match &slot.cell.edges {
                Some(own) => border(ui, rect, own, round, within, within.text()),
                None if table.border => {
                    ui.painter().rect_stroke(rect, round, (1.0, lines), egui::StrokeKind::Middle);
                }
                None => {}
            }
        }
        // The table's own border: seen round cells that stand apart, or when it is a border of
        // its own making.
        if any_edge(&table.frame) && (gap > 0.0 || rim > 0.0 || !table.border) {
            border(ui, outer, &table.frame, round, look, if table.border { lines } else { look.text() });
        }
    }
    ui.allocate_rect(whole, Sense::hover());
}
