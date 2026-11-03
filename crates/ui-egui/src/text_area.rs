//! Text of many lines that is typed into: the body of a message being written, and the notes of
//! an appointment, a task or a contact.
//!
//! It is our own for the reason `line_edit` is: the toolkit's box draws text as it is stored,
//! so Hebrew and Arabic came out in the wrong order while they were typed, and the cursor stood
//! at the wrong end of them. Here each paragraph (what a line feed ends) is wrapped to the room
//! there is and read the way its first letter that has a way says (UAX #9 P2 and P3): a
//! paragraph written from the right stands at the right, and each of its lines is put in the
//! order it is read, as a field of one line puts it. The cursor moves through the text as it is
//! stored; the arrows are turned on a line read from the right; up and down keep to where the
//! cursor was seen to be.
//!
//! A box can be written in with a look ([`Styled`]): each character in its own font, size and
//! colour, pictures standing in the text, and each paragraph aligned, set in and marked as an
//! item of a list. Its lines are then as high as what is tallest on them.

use crate::line_edit::{self, Edit, Line, Look, State};
use egui::{
    Color32, CursorIcon, Event, EventFilter, FontId, Id, ImeEvent, Key, Rect, Response, Sense, Stroke, StrokeKind, TextureId, Ui, pos2, vec2,
};
use openmailclient_fonts::script;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// What a box takes when it is not told: more than any message written by hand.
const MAX_CHARS: usize = 4_000_000;
/// The narrowest a line is wrapped to, so that a box squeezed to nothing still has words on
/// its lines rather than a letter on each.
const NARROWEST: f32 = 24.0;
/// How many lines Page Up and Page Down go when the box cannot say how many are seen.
const PAGE: usize = 20;

/// What a box remembers, for a test to look at.
#[cfg(test)]
pub(crate) fn state(ctx: &egui::Context, id: Id) -> Option<State> {
    ctx.data(|d| d.get_temp::<State>(id))
}

/// How a box written in with a look draws its text.
#[derive(Clone, Default)]
pub(crate) struct Styled {
    /// For each character of the text, which of `looks` it is drawn in; one past them is
    /// drawn in the first.
    pub(crate) of: Vec<usize>,
    /// The first is the text's own.
    pub(crate) looks: Vec<Look>,
    /// For a look that is room for a picture, the picture drawn there once it is ready.
    pub(crate) pictures: HashMap<usize, TextureId>,
    /// Each paragraph's place: one for each line feed and one more.
    pub(crate) paras: Vec<Lay>,
}

/// Where a paragraph stands.
#[derive(Clone, Debug, Default, PartialEq, Hash)]
pub(crate) struct Lay {
    /// How far in from the margin it starts, a list's marker before it.
    pub(crate) indent: u16,
    pub(crate) align: Side,
    /// What marks it as an item of a list: a bullet, or its number.
    pub(crate) marker: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Side {
    #[default]
    Start,
    Center,
    End,
}

impl Styled {
    fn look_of(&self, at: usize) -> usize {
        self.of.get(at).copied().unwrap_or(0)
    }

    /// What tells one way of drawing from another, for what was wrapped to be kept by.
    fn fingerprint(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.looks.hash(&mut hasher);
        hasher.finish()
    }
}

/// What a box written in with a look was asked for in this frame, for its writer to do: the
/// toolkit's clipboard carries only text.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Asked {
    /// The characters copied or cut, from and to.
    pub(crate) copied: Option<(usize, usize)>,
    /// Paste was pressed: with the text the toolkit has for it.
    pub(crate) paste: Option<String>,
}

/// What a box written in with a look was asked for in this frame.
pub(crate) fn asked(ctx: &egui::Context, id: Id) -> Asked {
    ctx.data_mut(|d| d.remove_temp::<Asked>(id.with("asked"))).unwrap_or_default()
}

pub(crate) struct TextArea<'a> {
    text: &'a mut String,
    styled: Option<&'a Styled>,
    id: Id,
    font: FontId,
    color: Option<Color32>,
    rows: usize,
    limit: usize,
    width: f32,
    frame: bool,
    hint: &'a str,
}

impl<'a> TextArea<'a> {
    pub(crate) fn new(text: &'a mut String, id: Id) -> Self {
        Self { text, styled: None, id, font: crate::font(13.0), color: None, rows: 4, limit: MAX_CHARS, width: f32::INFINITY, frame: true, hint: "" }
    }

    /// Drawn with a look of its own. Copy and paste are then left to its writer: see [`asked`].
    pub(crate) fn styled(mut self, styled: &'a Styled) -> Self {
        self.styled = Some(styled);
        self
    }

    pub(crate) fn font(mut self, font: FontId) -> Self {
        self.font = font;
        self
    }

    /// How many lines high it is at least.
    pub(crate) fn desired_rows(mut self, rows: usize) -> Self {
        self.rows = rows.clamp(1, 200);
        self
    }

    pub(crate) fn char_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub(crate) fn desired_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// What is shown, dimmed, while nothing is written.
    pub(crate) fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }

    /// Without the box drawn round it: the body of a message being written.
    pub(crate) fn bare(mut self) -> Self {
        self.frame = false;
        self
    }
}

/// One line as it is seen: where it is in the text, and how it is drawn.
#[derive(Clone)]
pub(crate) struct Row {
    /// The characters of the text it holds, a line feed that ends it left out.
    pub(crate) from: usize,
    pub(crate) to: usize,
    pub(crate) seen: Arc<Line>,
    /// How wide what it holds is.
    pub(crate) width: f32,
    /// True when its paragraph is read from the right: it stands at the right of the box.
    pub(crate) rtl: bool,
    /// True when a line feed ends it rather than the paragraph going on below.
    pub(crate) ends: bool,
    /// Where it is from the top of the text, and how high.
    pub(crate) top: f32,
    pub(crate) height: f32,
    /// How far its paragraph is set in, and how its lines stand.
    indent: f32,
    side: Side,
    /// What marks its paragraph as an item of a list, on the paragraph's first line.
    marker: Option<String>,
}

impl Row {
    /// Where the line starts in a box `room` wide.
    pub(crate) fn left(&self, room: f32) -> f32 {
        let free = (room - self.indent - self.width).max(0.0);
        match (self.side, self.rtl) {
            (Side::Center, _) => self.indent + free / 2.0,
            (Side::Start, false) | (Side::End, true) => self.indent,
            (Side::End, false) | (Side::Start, true) => free,
        }
    }

    /// The last place on it the cursor can be put by a click or by End: on a line the paragraph
    /// goes on from, before the blank it was broken at, which is where the next line starts.
    fn last(&self) -> usize {
        if self.ends || self.to <= self.from { self.to } else { self.to - 1 }
    }
}

/// How wide a line is: as far as its furthest character goes.
fn breadth(line: &Line) -> f32 {
    line.edges.iter().fold(0.0f32, |most, (a, b)| most.max(*a).max(*b))
}

/// A paragraph wrapped to `room`: each line as far as it goes, counted from the paragraph's
/// start, and how it is seen. A line is broken after a blank; a word wider than the room is
/// broken where it no longer fits. A paragraph with nothing in it is one empty line.
pub(crate) fn wrap(
    chars: &[char],
    room: f32,
    font: &FontId,
    color: Color32,
    mut measure: impl FnMut(&str) -> f32,
) -> Vec<(usize, usize, Line, bool)> {
    let shown = |from: usize, to: usize| -> String { chars.get(from..to).unwrap_or_default().iter().map(|c| line_edit::shown(*c)).collect() };
    let breaks = breaks(chars, room, &mut |from, to| measure(&shown(from, to)));
    let rtl = script::first_strong(&shown(0, chars.len())).unwrap_or(false);
    let count = breaks.len();
    breaks
        .into_iter()
        .enumerate()
        .map(|(n, (from, to))| {
            let line = line_edit::line_as(chars.get(from..to).unwrap_or_default(), font, color, &mut measure, Some(rtl));
            (from, to, line, n + 1 == count)
        })
        .collect()
}

/// Where a paragraph is broken into lines `room` wide, `measure` giving how wide its
/// characters from one to another are.
fn breaks(chars: &[char], room: f32, measure: &mut dyn FnMut(usize, usize) -> f32) -> Vec<(usize, usize)> {
    let room = if room.is_nan() { f32::INFINITY } else { room.max(NARROWEST) };
    // Words, each with the blanks after it.
    let mut words: Vec<(usize, usize)> = Vec::new();
    for (at, c) in chars.iter().enumerate() {
        let after_blank = at > 0 && chars.get(at - 1).is_some_and(|b| b.is_whitespace()) && !c.is_whitespace();
        match words.last_mut() {
            Some(last) if !after_blank => last.1 = at + 1,
            _ => words.push((at, at + 1)),
        }
    }
    let mut breaks: Vec<(usize, usize)> = Vec::new();
    let (mut start, mut wide) = (0usize, 0.0f32);
    for (from, to) in words {
        // What is measured is the word without its blanks: they may hang past the edge.
        let solid = chars.get(from..to).unwrap_or_default().iter().rposition(|c| !c.is_whitespace()).map_or(from, |n| from + n + 1);
        let word = measure(from, solid);
        if from > start && wide + word > room {
            breaks.push((start, from));
            (start, wide) = (from, 0.0);
        }
        if word > room {
            // Broken where it no longer fits, a letter at least on each line.
            let mut at = from;
            while at < solid {
                let mut end = at + 1;
                while end < solid && wide + measure(at, end + 1) <= room {
                    end += 1;
                }
                if end < solid {
                    breaks.push((start, end));
                    (start, wide) = (end, 0.0);
                } else {
                    wide += measure(at, end);
                }
                at = end;
            }
            wide += measure(solid, to);
            continue;
        }
        wide += measure(from, to);
    }
    breaks.push((start, chars.len()));
    breaks
}

/// A line wrapped once: where it is in its paragraph, how it is seen, how wide and how high
/// it is, and whether its paragraph ends with it.
#[derive(Clone)]
struct Made {
    from: usize,
    to: usize,
    seen: Arc<Line>,
    width: f32,
    height: f32,
    ends: bool,
}

/// What was wrapped last frame, kept by what it was made from, so that only a paragraph that
/// changed is wrapped again.
#[derive(Clone, Default)]
struct Wrapped(Arc<HashMap<u64, Arc<Vec<Made>>>>);

/// How far in each step of a paragraph's indent sets it: half an inch at 96 to the inch.
pub(crate) const INDENT_STEP: f32 = 48.0;
/// How far before an item of a list its marker stands.
const MARKER_ROOM: f32 = 22.0;

/// Every line of the text as it is seen in a box `room` wide, drawn as `styled` says where it
/// is given.
pub(crate) fn rows(ui: &Ui, cache: Id, chars: &[char], room: f32, font: &FontId, color: Color32, styled: Option<&Styled>) -> Vec<Row> {
    let old: Wrapped = ui.data(|d| d.get_temp(cache)).unwrap_or_default();
    let mut kept: HashMap<u64, Arc<Vec<Made>>> = HashMap::new();
    let mut rows = Vec::new();
    let mut start = 0usize;
    let row_h = ui.ctx().fonts_mut(|fonts| fonts.row_height(font)).max(1.0);
    // How high each look stands, and what tells the looks from another set of them.
    let heights: Vec<f32> = styled.map_or_else(Vec::new, |styled| {
        styled
            .looks
            .iter()
            .map(|look| match look {
                Look::Text(format) => ui.ctx().fonts_mut(|fonts| fonts.row_height(&format.font_id)).max(1.0),
                Look::Room(_, high) => high.max(1.0),
            })
            .collect()
    });
    let fingerprint = styled.map_or(0, Styled::fingerprint);
    let mut top = 0.0f32;
    for (n, paragraph) in chars.split(|c| *c == '\n').enumerate() {
        let lay = styled.and_then(|styled| styled.paras.get(n)).cloned().unwrap_or_default();
        let indent = (f32::from(lay.indent) * INDENT_STEP).min(room / 2.0);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (paragraph, room.to_bits(), font.size.to_bits(), color, fingerprint, lay.indent).hash(&mut hasher);
        format!("{:?}", font.family).hash(&mut hasher);
        if let Some(styled) = styled {
            styled.of.get(start..start.saturating_add(paragraph.len())).hash(&mut hasher);
        }
        let key = hasher.finish();
        let lines = match kept.get(&key).or_else(|| old.0.get(&key)) {
            Some(lines) => lines.clone(),
            None => {
                let made = match styled {
                    None => {
                        let measure = |piece: &str| ui.painter().layout_no_wrap(piece.to_string(), font.clone(), color).size().x;
                        wrap(paragraph, room - indent, font, color, measure)
                            .into_iter()
                            .map(|(from, to, line, ends)| Made { from, to, width: breadth(&line), seen: Arc::new(line), height: row_h, ends })
                            .collect()
                    }
                    Some(styled) => wrap_styled(ui, paragraph, start, room - indent, styled, &heights, row_h),
                };
                Arc::new(made)
            }
        };
        let rtl = script::first_strong(&paragraph.iter().map(|c| line_edit::shown(*c)).collect::<String>()).unwrap_or(false);
        for (k, line) in lines.iter().enumerate() {
            let marker = if k == 0 { lay.marker.clone() } else { None };
            rows.push(Row {
                from: start + line.from,
                to: start + line.to,
                seen: line.seen.clone(),
                width: line.width,
                rtl,
                ends: line.ends,
                top,
                height: line.height,
                indent,
                side: lay.align,
                marker,
            });
            top += line.height;
        }
        kept.insert(key, lines);
        start = start.saturating_add(paragraph.len()).saturating_add(1);
    }
    ui.data_mut(|d| d.insert_temp(cache, Wrapped(Arc::new(kept))));
    rows
}

/// A paragraph that starts at `start` of the text wrapped to `room`, drawn as `styled` says.
fn wrap_styled(ui: &Ui, paragraph: &[char], start: usize, room: f32, styled: &Styled, heights: &[f32], row_h: f32) -> Vec<Made> {
    let look = |n: usize| styled.look_of(start.saturating_add(n));
    let plain = egui::TextFormat::default();
    let mut measure_in =
        |piece: &str, format: &egui::TextFormat| ui.painter().layout_no_wrap(piece.to_string(), format.font_id.clone(), Color32::WHITE).size().x;
    // How wide the characters from one to another are: each stretch of one look measured in it.
    let mut measure = |from: usize, to: usize| {
        let (mut wide, mut at) = (0.0f32, from);
        while at < to {
            let seen = look(at);
            let mut end = at + 1;
            while end < to && look(end) == seen {
                end += 1;
            }
            wide += match styled.looks.get(seen) {
                Some(Look::Room(w, _)) => w * (end - at) as f32,
                Some(Look::Text(format)) => {
                    measure_in(&paragraph.get(at..end).unwrap_or_default().iter().map(|c| line_edit::shown(*c)).collect::<String>(), format)
                }
                None => measure_in(" ", &plain),
            };
            at = end;
        }
        wide
    };
    let breaks = breaks(paragraph, room, &mut measure);
    let rtl = script::first_strong(&paragraph.iter().map(|c| line_edit::shown(*c)).collect::<String>()).unwrap_or(false);
    let count = breaks.len();
    breaks
        .into_iter()
        .enumerate()
        .map(|(n, (from, to))| {
            let line =
                line_edit::line_in(paragraph.get(from..to).unwrap_or_default(), &|k| look(from + k), &styled.looks, &mut measure_in, Some(rtl));
            // As high as what is tallest on it; an empty line as high as the look it is in.
            let height = (from..to.max(from + 1)).map(|k| heights.get(look(k)).copied().unwrap_or(row_h)).fold(0.0f32, f32::max).max(1.0);
            Made { from, to, width: breadth(&line), seen: Arc::new(line), height, ends: n + 1 == count }
        })
        .collect()
}

/// The line the cursor is on: the last that starts at it or before, so that where a paragraph
/// is wrapped the cursor stands at the start of the line below.
pub(crate) fn row_of(rows: &[Row], cursor: usize) -> usize {
    rows.partition_point(|r| r.from <= cursor).saturating_sub(1)
}

/// Where the cursor is seen across a line, from the box's left.
pub(crate) fn caret(rows: &[Row], cursor: usize, room: f32) -> (usize, f32) {
    let n = row_of(rows, cursor);
    match rows.get(n) {
        Some(row) => (n, row.left(room) + row.seen.caret(cursor.saturating_sub(row.from))),
        None => (0, 0.0),
    }
}

/// The cursor nearest to `x` on line `n`.
pub(crate) fn hit(rows: &[Row], n: usize, x: f32, room: f32) -> usize {
    match rows.get(n) {
        Some(row) => (row.from + row.seen.at(x - row.left(room))).min(row.last()),
        None => 0,
    }
}

/// Up or down `by` lines, to where the cursor was seen when it started going up and down; past
/// the first line is the start of the text and past the last its end.
fn vertical(edit: &mut Edit, lines: &[Row], room: f32, by: isize, select: bool) {
    let (n, x) = caret(lines, edit.state.cursor, room);
    let goal = *edit.state.goal.get_or_insert(x);
    match n.checked_add_signed(by).filter(|t| *t < lines.len()) {
        Some(t) => edit.go(hit(lines, t, goal, room), select),
        None if by < 0 => edit.go(0, select),
        None => edit.go(usize::MAX, select),
    }
    edit.state.goal = Some(goal);
}

/// Where the paragraph that `at` is in starts and ends.
fn paragraph_at(chars: &[char], at: usize) -> (usize, usize) {
    let at = at.min(chars.len());
    let from = chars.get(..at).unwrap_or_default().iter().rposition(|c| *c == '\n').map_or(0, |n| n + 1);
    let to = chars.get(at..).unwrap_or_default().iter().position(|c| *c == '\n').map_or(chars.len(), |n| at + n);
    (from, to)
}

impl egui::Widget for TextArea<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let Self { text, styled, id, font, color, rows: at_least, limit, width, frame, hint } = self;
        let pad = if frame { vec2(5.0, 4.0) } else { vec2(0.0, 0.0) };
        let row_h = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font)).max(1.0);
        let wide = if width.is_finite() { width } else { ui.available_width() }.max(32.0);
        let room = (wide - 2.0 * pad.x).max(1.0);
        let color = color.unwrap_or_else(|| ui.visuals().text_color());
        let mut state = ui.data(|d| d.get_temp::<State>(id)).unwrap_or_default();
        let before = text.clone();
        let now = ui.input(|i| i.time);
        let cache = id.with("wrapped");

        let mut edit = Edit { chars: text.chars().collect(), state: &mut state, limit, changed: false, lines: true };
        edit.state.cursor = edit.state.cursor.min(edit.chars.len());
        edit.state.anchor = edit.state.anchor.min(edit.chars.len());
        // The lines as they were when the keys were pressed, for the keys to go by.
        let first = rows(ui, cache, &edit.chars, room, &font, color, styled);
        let lines_high = first.last().map_or(0.0, |r| r.top + r.height);
        let height = lines_high.max(at_least as f32 * row_h) + 2.0 * pad.y;
        let (_, rect) = ui.allocate_space(vec2(wide, height));
        let mut response = ui.interact(rect, id, Sense::click_and_drag());
        let inner = rect.shrink2(pad);
        if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }
        let pointer = response.is_pointer_button_down_on();
        if pointer {
            response.request_focus();
        }
        let focused = response.has_focus() || pointer;
        // How many lines are seen at once, for Page Up and Page Down.
        let page = ((ui.clip_rect().height() / row_h).floor() as usize).saturating_sub(1).clamp(1, 200);
        let page = if ui.clip_rect().height().is_finite() { page } else { PAGE };

        let mut copied: Option<String> = None;
        let mut asked = Asked::default();
        let mut keep_goal = false;
        if focused {
            ui.memory_mut(|m| m.set_focus_lock_filter(id, EventFilter { horizontal_arrows: true, vertical_arrows: true, ..EventFilter::default() }));
            let mut lines = first.clone();
            for event in ui.input(|i| i.events.clone()) {
                let was = (edit.state.cursor, edit.state.anchor, edit.changed);
                let line = row_of(&lines, edit.state.cursor);
                let rtl = lines.get(line).is_some_and(|r| r.rtl);
                match event {
                    // What is pasted into a box with a look is its writer's to put in, with
                    // the look the clipboard has for it.
                    Event::Paste(new) if styled.is_some() => asked.paste = Some(new),
                    Event::Text(new) | Event::Paste(new) => edit.insert(&new),
                    Event::Copy => {
                        copied = edit.copy().or(copied);
                        asked.copied = Some(edit.selected()).filter(|(a, b)| a < b).or(asked.copied);
                    }
                    Event::Cut => {
                        if let Some(text) = edit.copy() {
                            copied = Some(text);
                            asked.copied = Some(edit.selected());
                            let (from, to) = edit.selected();
                            edit.delete(from, to);
                        }
                    }
                    Event::Ime(ImeEvent::Preedit { text: new, .. }) => {
                        let (from, to) = edit.selected();
                        if !new.is_empty() {
                            edit.delete(from, to);
                        }
                        edit.state.preedit = line_edit::taken(&new, 200, false).into_iter().collect();
                    }
                    Event::Ime(ImeEvent::Commit(new)) => {
                        edit.state.preedit.clear();
                        edit.insert(&new);
                    }
                    Event::Ime(ImeEvent::DeleteSurrounding { before_chars, after_chars }) => {
                        let at = edit.state.cursor;
                        edit.delete(at.saturating_sub(before_chars), at.saturating_add(after_chars));
                    }
                    Event::Key { key, pressed: true, modifiers, .. } => {
                        let (word, select) = (modifiers.ctrl || modifiers.alt, modifiers.shift);
                        let (from, to) = edit.selected();
                        let at = edit.state.cursor;
                        let vertical = |edit: &mut Edit, by: isize| vertical(edit, &lines, room, by, select);
                        match key {
                            Key::ArrowLeft => edit.walk(rtl, word, select),
                            Key::ArrowRight => edit.walk(!rtl, word, select),
                            Key::ArrowUp => {
                                vertical(&mut edit, -1);
                                keep_goal = true;
                            }
                            Key::ArrowDown => {
                                vertical(&mut edit, 1);
                                keep_goal = true;
                            }
                            Key::PageUp => {
                                vertical(&mut edit, -(page as isize));
                                keep_goal = true;
                            }
                            Key::PageDown => {
                                vertical(&mut edit, page as isize);
                                keep_goal = true;
                            }
                            Key::Home if modifiers.command => edit.go(0, select),
                            Key::End if modifiers.command => edit.go(usize::MAX, select),
                            Key::Home => edit.go(lines.get(line).map_or(0, |r| r.from), select),
                            Key::End => edit.go(lines.get(line).map_or(usize::MAX, Row::last), select),
                            Key::Enter => edit.insert("\n"),
                            Key::Backspace if from != to => edit.delete(from, to),
                            Key::Delete if from != to => edit.delete(from, to),
                            Key::Backspace if word => edit.delete(line_edit::word_back(&edit.chars, at), at),
                            Key::Backspace => edit.delete(at.saturating_sub(1), at),
                            Key::Delete if word => edit.delete(at, line_edit::word_on(&edit.chars, at)),
                            Key::Delete => edit.delete(at, at.saturating_add(1)),
                            Key::A if modifiers.command => {
                                edit.go(0, false);
                                edit.go(usize::MAX, true);
                            }
                            Key::Z if modifiers.command && !modifiers.shift => edit.swap(true),
                            Key::Z | Key::Y if modifiers.command => edit.swap(false),
                            _ => {}
                        }
                    }
                    _ => {}
                }
                if was != (edit.state.cursor, edit.state.anchor, edit.changed) {
                    edit.state.touched = now;
                }
                if edit.changed {
                    // What was typed is wrapped again before the next key goes by the lines.
                    lines = rows(ui, cache, &edit.chars, room, &font, color, styled);
                }
            }
        } else {
            edit.state.preedit.clear();
        }
        let total = edit.chars.len();
        if !keep_goal {
            edit.state.goal = None;
        }

        // What is drawn: the text, with what an input method is putting together at the cursor.
        let held: Vec<char> = edit.state.preedit.chars().collect();
        let at = edit.state.cursor.min(edit.chars.len());
        let mut drawn = edit.chars.clone();
        drawn.splice(at..at, held.iter().copied());
        let seen = rows(ui, cache, &drawn, room, &font, color, styled);
        let in_drawn = |n: usize| if n >= at { n + held.len() } else { n };
        let in_stored = |n: usize| if n >= at + held.len() { n - held.len() } else { n.min(at) };
        let line_at = |y: f32| seen.partition_point(|r| r.top <= y - inner.top()).saturating_sub(1).min(seen.len().saturating_sub(1));

        if pointer && let Some(pos) = response.interact_pointer_pos() {
            let hit = hit(&seen, line_at(pos.y), pos.x - inner.left(), room);
            let hit = if hit >= at + held.len() { hit - held.len() } else { hit.min(at) };
            let pressed = ui.input(|i| i.pointer.any_pressed());
            edit.go(hit, !pressed || ui.input(|i| i.modifiers.shift));
            edit.state.touched = now;
        }
        if response.double_clicked() {
            (edit.state.anchor, edit.state.cursor) = line_edit::word_at(&edit.chars, edit.state.cursor);
        }
        if response.triple_clicked() {
            (edit.state.anchor, edit.state.cursor) = paragraph_at(&edit.chars, edit.state.cursor);
        }

        let changed = edit.changed;
        if changed {
            *text = edit.text();
        }
        let cursor = edit.state.cursor.min(edit.chars.len());
        let (from, to) = edit.selected();
        let moved = edit.state.touched == now;
        let (line, caret_x) = caret(&seen, in_drawn(cursor), room);
        state.caret_x = caret_x;

        let (cursor_top, cursor_h) = seen.get(line).map_or((0.0, row_h), |r| (r.top, r.height));
        let cursor_top = inner.top() + cursor_top;
        let cursor_rect = Rect::from_min_max(pos2(inner.left() + caret_x, cursor_top), pos2(inner.left() + caret_x + 1.0, cursor_top + cursor_h));
        if focused && moved {
            ui.scroll_to_rect(cursor_rect.expand2(vec2(0.0, 2.0)), None);
        }
        if ui.is_rect_visible(rect) {
            if frame {
                let visuals = ui.visuals();
                let edge = if focused { visuals.selection.stroke } else { visuals.widgets.noninteractive.bg_stroke };
                ui.painter().rect_filled(rect, 3.0, visuals.text_edit_bg_color());
                ui.painter().rect_stroke(rect, 3.0, edge, StrokeKind::Inside);
            }
            let clip = ui.clip_rect();
            let painter = ui.painter().with_clip_rect(rect.intersect(clip));
            let selection = ui.visuals().selection.bg_fill;
            for row in &seen {
                let (top, row_h) = (inner.top() + row.top, row.height);
                if top > clip.bottom() || top + row_h < clip.top() {
                    continue;
                }
                let left = inner.left() + row.left(room);
                let span = |c: usize| row.seen.edges.get(c.checked_sub(row.from)?).map(|(a, b)| (left + a.min(*b), left + a.max(*b)));
                if focused && from != to && held.is_empty() {
                    for (l, r) in (from.max(row.from)..to.min(row.to)).filter_map(span) {
                        painter.rect_filled(Rect::from_min_max(pos2(l, top), pos2(r, top + row_h)), 0.0, selection);
                    }
                    // A line feed that is selected is a narrow box at the line's end.
                    if row.ends && from <= row.to && row.to < to && row.to < total {
                        let x = if row.rtl { left - 4.0 } else { left + row.width };
                        painter.rect_filled(Rect::from_min_max(pos2(x, top), pos2(x + 4.0, top + row_h)), 0.0, selection);
                    }
                }
                let galley = ui.painter().layout_job(row.seen.job.clone());
                // Text stands on the foot of a line a picture makes high.
                let lift = if styled.is_some() { row_h - galley.size().y } else { (row_h - galley.size().y) / 2.0 };
                painter.galley(pos2(left, top + lift), galley, color);
                if let Some(styled) = styled {
                    for c in row.from..row.to {
                        let Some(Look::Room(w, h)) = styled.looks.get(styled.look_of(in_stored(c))) else { continue };
                        let Some((l, _)) = span(c) else { continue };
                        let at = Rect::from_min_size(pos2(l, top + row_h - h), vec2(*w, *h));
                        match styled.pictures.get(&styled.look_of(in_stored(c))) {
                            Some(texture) => {
                                painter.image(*texture, at, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                            }
                            None => {
                                painter.rect_stroke(at.shrink(0.5), 0.0, Stroke::new(1.0, ui.visuals().weak_text_color()), StrokeKind::Inside);
                            }
                        }
                    }
                }
                if let Some(marker) = &row.marker {
                    let job = crate::seen_line(marker, font.clone(), color);
                    let mark = ui.painter().layout_job(job);
                    let x = if row.rtl { left + row.width + MARKER_ROOM - mark.size().x } else { left - MARKER_ROOM };
                    painter.galley(pos2(x, top + row_h - mark.size().y), mark, color);
                }
                for (l, r) in (at..at + held.len()).filter_map(span) {
                    painter.hline(l..=r, top + row_h - 1.0, Stroke::new(1.0, color));
                }
            }
            if drawn.is_empty() && !hint.is_empty() {
                let dim = ui.visuals().weak_text_color();
                for (n, line) in hint.lines().take(at_least).enumerate() {
                    let job = crate::seen_line(line, font.clone(), dim);
                    let galley = ui.painter().layout_job(job);
                    let rtl = script::first_strong(line).unwrap_or(false);
                    let x = if rtl { inner.right() - galley.size().x } else { inner.left() };
                    painter.galley(pos2(x, inner.top() + n as f32 * row_h), galley, dim);
                }
            }
            if focused {
                // The cursor is seen for half of each second after the last thing done.
                let since = (now - state.touched).max(0.0);
                if since % 1.0 < 0.5 {
                    painter.vline(cursor_rect.left().round() + 0.5, cursor_rect.y_range(), Stroke::new(1.0, color));
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(0.5 - since % 0.5));
                let to_global = ui.ctx().layer_transform_to_global(ui.layer_id()).unwrap_or_default();
                ui.ctx().output_mut(|o| {
                    o.ime = Some(egui::output::IMEOutput {
                        purpose: egui::IMEPurpose::Normal,
                        rect: to_global * rect,
                        cursor_rect: to_global * cursor_rect,
                        should_interrupt_composition: false,
                    });
                });
            }
        }

        if styled.is_some() {
            if asked != Asked::default() {
                ui.data_mut(|d| d.insert_temp(id.with("asked"), asked));
            }
        } else if let Some(text) = copied {
            ui.ctx().copy_text(text);
        }
        if changed {
            response.mark_changed();
        }
        response.widget_info(|| egui::WidgetInfo::text_edit(true, &before, &*text, hint));
        ui.data_mut(|d| d.insert_temp(id, state));
        response
    }
}
