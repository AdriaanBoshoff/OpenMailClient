//! A field of one line of text that is typed into: the search box, a folder's new name, the
//! address and subject lines of a message being written, what stands at a printed page's head.
//!
//! It is our own because the toolkit's field draws text as it is stored: Hebrew and Arabic
//! mixed with other writing came out in the wrong order, and the cursor stood at the wrong end
//! of them. Here the line is put in the order it is read (UAX #9, as everywhere else: see
//! `seen_line`), and the cursor, the selection and a click all go by where each character is
//! seen. The cursor moves through the text as it is stored; the arrow keys are turned in a
//! line that is read from the right, so that "left" is always the way the eye goes.

use crate::body;
use egui::text::LayoutJob;
use egui::{Color32, CursorIcon, Event, EventFilter, FontId, Id, ImeEvent, Key, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use openmailclient_fonts::script;

/// What a field takes when it is not told: more is never a name, an address or a search.
const MAX_CHARS: usize = 4000;
const UNDO_STEPS: usize = 100;

/// What a field remembers from one frame to the next.
#[derive(Clone, Default)]
pub(crate) struct State {
    /// Where the next character goes: how many characters of the text are before it.
    pub(crate) cursor: usize,
    /// Where the selection started; the cursor when nothing is selected.
    pub(crate) anchor: usize,
    /// Where the cursor is drawn, from the start of the line.
    pub(crate) caret_x: f32,
    /// How far the line is moved to the left for the cursor to be seen.
    scroll: f32,
    /// What an input method is putting together, shown at the cursor until it is done.
    pub(crate) preedit: String,
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
    /// True while one character is typed after another: they are taken back together.
    typing: bool,
    /// When something was last done, for the cursor to blink from.
    pub(crate) touched: f64,
    /// Where the cursor is wanted across a text of many lines, while it goes up and down.
    pub(crate) goal: Option<f32>,
}

/// What a field remembers, for a test to look at.
#[cfg(test)]
pub(crate) fn state(ctx: &egui::Context, id: Id) -> Option<State> {
    ctx.data(|d| d.get_temp::<State>(id))
}

pub(crate) struct LineEdit<'a> {
    text: &'a mut String,
    id: Id,
    font: FontId,
    color: Option<Color32>,
    hint: &'a str,
    hint_color: Option<Color32>,
    limit: usize,
    frame: bool,
    width: f32,
    secret: bool,
}

impl<'a> LineEdit<'a> {
    pub(crate) fn new(text: &'a mut String, id: Id) -> Self {
        Self {
            text,
            id,
            font: crate::font(13.0),
            color: None,
            hint: "",
            hint_color: None,
            limit: MAX_CHARS,
            frame: true,
            width: f32::INFINITY,
            secret: false,
        }
    }

    pub(crate) fn font(mut self, font: FontId) -> Self {
        self.font = font;
        self
    }

    pub(crate) fn text_color(mut self, color: Color32) -> Self {
        self.color = Some(color);
        self
    }

    /// What is shown, dimmed, while the field is empty.
    pub(crate) fn hint(mut self, hint: &'a str, color: Option<Color32>) -> Self {
        (self.hint, self.hint_color) = (hint, color);
        self
    }

    /// How many characters the field takes at most.
    pub(crate) fn char_limit(mut self, limit: usize) -> Self {
        self.limit = limit.min(MAX_CHARS);
        self
    }

    /// Without its own ground and edge: for a field that is drawn into something else.
    pub(crate) fn bare(mut self) -> Self {
        self.frame = false;
        self
    }

    pub(crate) fn desired_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// For a password: each character is shown as a dot, nothing is copied out of it, no
    /// earlier text is remembered to be gone back to, and what it holds is told to nobody,
    /// readers of the screen included.
    pub(crate) fn password(mut self) -> Self {
        self.secret = true;
        self
    }
}

/// A character as a field shows it. A mark that turns the text after it round is shown as a
/// dot and turns nothing: what is being typed over cannot be disguised, and can be seen to be
/// taken out. Anything else that has no shape is a blank.
pub(crate) fn shown(c: char) -> char {
    match c {
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' => '\u{00B7}',
        c if c.is_control() => ' ',
        c => c,
    }
}

/// What is typed or pasted, as a field takes it: one line, or many when `lines` (each ended
/// by a line feed alone), and none of the marks that turn text round.
pub(crate) fn taken(text: &str, room: usize, lines: bool) -> Vec<char> {
    let text = if lines { text.replace("\r\n", "\n").replace('\r', "\n") } else { text.to_string() };
    text.chars()
        .filter_map(|c| match c {
            '\n' | '\t' if lines => Some(c),
            '\n' | '\t' => Some(' '),
            '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' => None,
            c if c.is_control() => None,
            c => Some(c),
        })
        .take(room)
        .collect()
}

/// A line as it is seen.
#[derive(Clone)]
pub(crate) struct Line {
    pub(crate) job: LayoutJob,
    /// For each character of the text, where it starts and where it ends as it is read, from
    /// the start of the line: the start is at the right of a letter written from the right.
    pub(crate) edges: Vec<(f32, f32)>,
}

impl Line {
    /// Where the cursor stands when `cursor` characters are before it: after the one it
    /// follows, which is where the next one typed will be seen to go on from.
    pub(crate) fn caret(&self, cursor: usize) -> f32 {
        match cursor.checked_sub(1).and_then(|before| self.edges.get(before)) {
            Some((_, end)) => *end,
            None => self.edges.first().map_or(0.0, |(start, _)| *start),
        }
    }

    /// The cursor that is nearest to `x`.
    pub(crate) fn at(&self, x: f32) -> usize {
        let mut best = (0, f32::INFINITY);
        for cursor in 0..=self.edges.len() {
            let away = (self.caret(cursor) - x).abs();
            if away < best.1 {
                best = (cursor, away);
            }
        }
        best.0
    }
}

/// Put a line in the order it is seen in and say where each of its characters is.
/// `measure` gives the width of a piece of text as the toolkit draws it.
pub(crate) fn line(chars: &[char], font: &FontId, color: Color32, measure: impl FnMut(&str) -> f32) -> Line {
    line_as(chars, font, color, measure, None)
}

/// As `line`, read from the right when `rtl` says so, or as its first letter that has a way
/// says when it does not: a line of a paragraph wrapped is read the way its paragraph is.
pub(crate) fn line_as(chars: &[char], font: &FontId, color: Color32, mut measure: impl FnMut(&str) -> f32, rtl: Option<bool>) -> Line {
    let chars: Vec<char> = chars.iter().map(|c| shown(*c)).collect();
    let text: String = chars.iter().collect();
    let rtl = rtl.unwrap_or_else(|| script::first_strong(&text).unwrap_or(false));
    let levels = script::levels(&text, Some(rtl));
    // Pieces: characters that stand equally deep and are of one kind, as in `seen_line`.
    let mut pieces: Vec<(usize, usize, u8, u8)> = Vec::new();
    for (at, c) in chars.iter().enumerate() {
        let level = levels.get(at).copied().unwrap_or(0);
        let kind = if c.is_whitespace() {
            0
        } else if script::is_rtl(*c) {
            1
        } else {
            2
        };
        match pieces.last_mut() {
            Some(last) if last.2 == level && last.3 == kind => last.1 = at + 1,
            _ => pieces.push((at, at + 1, level, kind)),
        }
    }
    let stand_ins: Vec<char> = pieces.iter().map(|p| if p.3 == 0 { ' ' } else { 'a' }).collect();
    let depth: Vec<u8> = pieces.iter().map(|p| p.2).collect();
    let format = egui::TextFormat::simple(font.clone(), color);
    let (mut job, mut edges, mut x) = (LayoutJob::default(), vec![(0.0, 0.0); chars.len()], 0.0f32);
    for i in script::order(&stand_ins, &depth, u8::from(rtl)) {
        let Some((from, to, level, _)) = pieces.get(i).copied() else { continue };
        let Some(piece) = chars.get(from..to) else { continue };
        let whole = body::spelled(piece, level);
        let wide = measure(&whole);
        // Where each character ends is how wide the piece is up to it.
        let mut before = 0.0f32;
        for (n, edge) in edges.iter_mut().skip(from).take(to - from).enumerate() {
            let upto = if n + 1 == piece.len() { wide } else { measure(&body::spelled(piece.get(..=n).unwrap_or(piece), level)).clamp(before, wide) };
            *edge = if level % 2 == 1 { (x + wide - before, x + wide - upto) } else { (x + before, x + upto) };
            before = upto;
        }
        body::section_of_its_own(&mut job, &whole, 0.0, format.clone());
        x += wide;
    }
    Line { job, edges }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where the word before `at` starts.
pub(crate) fn word_back(chars: &[char], at: usize) -> usize {
    let mut at = at.min(chars.len());
    while at > 0 && !chars.get(at - 1).is_some_and(|c| is_word(*c)) {
        at -= 1;
    }
    while at > 0 && chars.get(at - 1).is_some_and(|c| is_word(*c)) {
        at -= 1;
    }
    at
}

/// Where the word after `at` ends.
pub(crate) fn word_on(chars: &[char], at: usize) -> usize {
    let mut at = at.min(chars.len());
    while at < chars.len() && !chars.get(at).is_some_and(|c| is_word(*c)) {
        at += 1;
    }
    while at < chars.len() && chars.get(at).is_some_and(|c| is_word(*c)) {
        at += 1;
    }
    at
}

/// The word that `at` is in or just after: where it starts and where it ends.
pub(crate) fn word_at(chars: &[char], at: usize) -> (usize, usize) {
    let (mut from, mut to) = (at.min(chars.len()), at.min(chars.len()));
    while from > 0 && chars.get(from - 1).is_some_and(|c| is_word(*c)) {
        from -= 1;
    }
    while to < chars.len() && chars.get(to).is_some_and(|c| is_word(*c)) {
        to += 1;
    }
    (from, to)
}

/// The text of a field while it is worked on.
pub(crate) struct Edit<'s> {
    pub(crate) chars: Vec<char>,
    pub(crate) state: &'s mut State,
    pub(crate) limit: usize,
    pub(crate) changed: bool,
    /// True when it is text of many lines.
    pub(crate) lines: bool,
}

impl Edit<'_> {
    pub(crate) fn selected(&self) -> (usize, usize) {
        (self.state.cursor.min(self.state.anchor), self.state.cursor.max(self.state.anchor))
    }

    pub(crate) fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub(crate) fn copy(&self) -> Option<String> {
        let (from, to) = self.selected();
        self.chars.get(from..to).filter(|s| !s.is_empty()).map(|s| s.iter().collect())
    }

    /// Remember the text as it is, to be gone back to.
    pub(crate) fn step(&mut self, typing: bool) {
        if !(typing && self.state.typing) {
            self.state.undo.push((self.text(), self.state.cursor));
            if self.state.undo.len() > UNDO_STEPS {
                self.state.undo.remove(0);
            }
        }
        self.state.redo.clear();
        self.state.typing = typing;
    }

    fn cut(&mut self, from: usize, to: usize) {
        let (from, to) = (from.min(self.chars.len()), to.min(self.chars.len()));
        if from < to {
            self.chars.drain(from..to);
            self.changed = true;
        }
        (self.state.cursor, self.state.anchor) = (from, from);
    }

    pub(crate) fn insert(&mut self, text: &str) {
        let (from, to) = self.selected();
        let room = self.limit.saturating_sub(self.chars.len().saturating_sub(to - from));
        let new = taken(text, room, self.lines);
        if new.is_empty() && from == to {
            return;
        }
        self.step(from == to && new.len() == 1 && new.first().is_some_and(|c| !c.is_whitespace()));
        self.cut(from, to);
        let at = from.min(self.chars.len());
        let count = new.len();
        self.chars.splice(at..at, new);
        (self.state.cursor, self.state.anchor) = (at + count, at + count);
        self.changed = true;
    }

    pub(crate) fn delete(&mut self, from: usize, to: usize) {
        if from < to {
            self.step(false);
            self.cut(from, to);
        }
    }

    pub(crate) fn go(&mut self, to: usize, select: bool) {
        self.state.cursor = to.min(self.chars.len());
        if !select {
            self.state.anchor = self.state.cursor;
        }
        self.state.typing = false;
    }

    /// One step through the text as it is stored: `on` is towards its end.
    pub(crate) fn walk(&mut self, on: bool, word: bool, select: bool) {
        let (from, to) = self.selected();
        let cursor = self.state.cursor;
        let target = match (from != to && !select, on, word) {
            // A selection that is let go of leaves the cursor at the end that was asked for.
            (true, true, _) => to,
            (true, false, _) => from,
            (false, true, true) => word_on(&self.chars, cursor),
            (false, false, true) => word_back(&self.chars, cursor),
            (false, true, false) => cursor.saturating_add(1),
            (false, false, false) => cursor.saturating_sub(1),
        };
        self.go(target, select);
    }

    pub(crate) fn swap(&mut self, back: bool) {
        let now = (self.text(), self.state.cursor);
        let (take, give) = if back { (&mut self.state.undo, &mut self.state.redo) } else { (&mut self.state.redo, &mut self.state.undo) };
        let Some((text, cursor)) = take.pop() else { return };
        give.push(now);
        self.chars = text.chars().collect();
        (self.state.cursor, self.state.anchor) = (cursor.min(self.chars.len()), cursor.min(self.chars.len()));
        (self.state.typing, self.changed) = (false, true);
    }
}

impl egui::Widget for LineEdit<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let Self { text, id, font, color, hint, hint_color, limit, frame, width, secret } = self;
        let pad = if frame { vec2(5.0, 3.0) } else { vec2(0.0, 0.0) };
        let row = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font));
        let wide = if width.is_finite() { width } else { ui.available_width() }.max(16.0);
        let (_, rect) = ui.allocate_space(vec2(wide, (row + 2.0 * pad.y).max(ui.spacing().interact_size.y.min(row + 6.0))));
        let mut response = ui.interact(rect, id, Sense::click_and_drag());
        let inner = rect.shrink2(pad);
        let color = color.unwrap_or_else(|| ui.visuals().text_color());
        let mut state = ui.data(|d| d.get_temp::<State>(id)).unwrap_or_default();
        let before = text.clone();
        let now = ui.input(|i| i.time);

        if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }
        let pointer = response.is_pointer_button_down_on();
        if pointer {
            response.request_focus();
        }
        let focused = response.has_focus() || pointer;
        let mut edit = Edit { chars: text.chars().collect(), state: &mut state, limit, changed: false, lines: false };
        edit.state.cursor = edit.state.cursor.min(edit.chars.len());
        edit.state.anchor = edit.state.anchor.min(edit.chars.len());
        if response.gained_focus() && !pointer {
            // Come to with the keyboard: what is typed takes the place of what is there.
            (edit.state.anchor, edit.state.cursor) = (0, edit.chars.len());
            edit.state.touched = now;
        }

        let mut copied: Option<String> = None;
        if focused {
            ui.memory_mut(|m| m.set_focus_lock_filter(id, EventFilter { horizontal_arrows: true, ..EventFilter::default() }));
            // Which way the arrows go is the way the line was read when they were pressed.
            let rtl = script::first_strong(&edit.chars.iter().map(|c| shown(*c)).collect::<String>()).unwrap_or(false);
            for event in ui.input(|i| i.events.clone()) {
                let was = (edit.state.cursor, edit.state.anchor, edit.changed);
                match event {
                    Event::Text(new) | Event::Paste(new) => edit.insert(&new),
                    Event::Copy | Event::Cut if secret => {}
                    Event::Copy => copied = edit.copy().or(copied),
                    Event::Cut => {
                        if let Some(text) = edit.copy() {
                            copied = Some(text);
                            let (from, to) = edit.selected();
                            edit.delete(from, to);
                        }
                    }
                    Event::Ime(ImeEvent::Preedit { text: new, .. }) => {
                        let (from, to) = edit.selected();
                        if !new.is_empty() {
                            edit.delete(from, to);
                        }
                        edit.state.preedit = taken(&new, 200, false).into_iter().collect();
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
                        match key {
                            Key::ArrowLeft => edit.walk(rtl, word, select),
                            Key::ArrowRight => edit.walk(!rtl, word, select),
                            Key::Home => edit.go(0, select),
                            Key::End => edit.go(usize::MAX, select),
                            Key::Backspace if from != to => edit.delete(from, to),
                            Key::Delete if from != to => edit.delete(from, to),
                            Key::Backspace if word => edit.delete(word_back(&edit.chars, at), at),
                            Key::Backspace => edit.delete(at.saturating_sub(1), at),
                            Key::Delete if word => edit.delete(at, word_on(&edit.chars, at)),
                            Key::Delete => edit.delete(at, at.saturating_add(1)),
                            Key::A if modifiers.command => {
                                edit.go(0, false);
                                edit.go(usize::MAX, true);
                            }
                            Key::Z | Key::Y if secret && modifiers.command => {}
                            Key::Z if modifiers.command && !modifiers.shift => edit.swap(true),
                            Key::Z | Key::Y if modifiers.command => edit.swap(false),
                            Key::Enter => ui.memory_mut(|m| m.surrender_focus(id)),
                            _ => {}
                        }
                    }
                    _ => {}
                }
                if was != (edit.state.cursor, edit.state.anchor, edit.changed) {
                    edit.state.touched = now;
                }
            }
        } else {
            edit.state.preedit.clear();
        }

        // What is drawn: the text, with what an input method is putting together at the cursor.
        let held: Vec<char> = edit.state.preedit.chars().collect();
        let at = edit.state.cursor.min(edit.chars.len());
        let mut drawn = if secret { vec!['•'; edit.chars.len()] } else { edit.chars.clone() };
        drawn.splice(at..at, held.iter().copied());
        let measure = |piece: &str| ui.painter().layout_no_wrap(piece.to_string(), font.clone(), color).size().x;
        let seen = line(&drawn, &font, color, measure);

        if pointer && let Some(pos) = response.interact_pointer_pos() {
            let hit = seen.at(pos.x - inner.left() + edit.state.scroll);
            // Counted in the text itself, without what an input method holds.
            let hit = if hit >= at + held.len() { hit - held.len() } else { hit.min(at) };
            let pressed = ui.input(|i| i.pointer.any_pressed());
            edit.go(hit, !pressed || ui.input(|i| i.modifiers.shift));
            edit.state.touched = now;
        }
        if response.double_clicked() {
            // The words of a password are not to be told apart.
            (edit.state.anchor, edit.state.cursor) = if secret { (0, edit.chars.len()) } else { word_at(&edit.chars, edit.state.cursor) };
        }
        if response.triple_clicked() {
            (edit.state.anchor, edit.state.cursor) = (0, edit.chars.len());
        }

        let changed = edit.changed;
        if changed {
            *text = edit.text();
        }
        let cursor = edit.state.cursor.min(edit.chars.len());
        let (from, to) = edit.selected();
        // Where things are in what is drawn: after what the input method holds, moved on by it.
        let in_drawn = |n: usize| if n >= at { n + held.len() } else { n };
        let caret = seen.caret(in_drawn(cursor));
        state.caret_x = caret;
        let galley = ui.painter().layout_job(seen.job.clone());
        let room = inner.width().max(1.0);
        state.scroll = state.scroll.min((galley.size().x - room + 1.0).max(0.0)).min(caret).max(caret - room + 1.0).max(0.0);
        if !focused {
            state.scroll = 0.0;
        }

        if ui.is_rect_visible(rect) {
            if frame {
                let visuals = ui.visuals();
                let edge = if focused { visuals.selection.stroke } else { visuals.widgets.noninteractive.bg_stroke };
                ui.painter().rect_filled(rect, 3.0, visuals.text_edit_bg_color());
                ui.painter().rect_stroke(rect, 3.0, edge, StrokeKind::Inside);
            }
            let painter = ui.painter().with_clip_rect(inner.expand2(vec2(1.0, pad.y)).intersect(ui.clip_rect()));
            let origin = pos2(inner.left() - state.scroll, inner.center().y - galley.size().y / 2.0);
            let (top, bottom) = (inner.center().y - row / 2.0, inner.center().y + row / 2.0);
            let span = |n: usize| seen.edges.get(n).map(|(a, b)| (origin.x + a.min(*b), origin.x + a.max(*b)));
            if focused && from != to && held.is_empty() {
                for (left, right) in (from..to).filter_map(span) {
                    painter.rect_filled(Rect::from_min_max(pos2(left, top), pos2(right, bottom)), 0.0, ui.visuals().selection.bg_fill);
                }
            }
            if drawn.is_empty() {
                let hint_color = hint_color.unwrap_or_else(|| ui.visuals().weak_text_color());
                let hint = ui.painter().layout_job(crate::seen_line(hint, font.clone(), hint_color));
                painter.galley(pos2(origin.x, inner.center().y - hint.size().y / 2.0), hint, hint_color);
            } else {
                painter.galley(origin, galley, color);
            }
            for (left, right) in (at..at + held.len()).filter_map(span) {
                painter.hline(left..=right, bottom - 1.0, Stroke::new(1.0, color));
            }
            if focused {
                // The cursor is seen for half of each second after the last thing done.
                let since = (now - state.touched).max(0.0);
                if since % 1.0 < 0.5 {
                    painter.vline((origin.x + caret).round() + 0.5, top..=bottom, Stroke::new(1.0, color));
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(0.5 - since % 0.5));
                let to_global = ui.ctx().layer_transform_to_global(ui.layer_id()).unwrap_or_default();
                let cursor_rect = Rect::from_min_max(pos2(origin.x + caret, top), pos2(origin.x + caret + 1.0, bottom));
                ui.ctx().output_mut(|o| {
                    o.ime = Some(egui::output::IMEOutput {
                        purpose: if secret { egui::IMEPurpose::Password } else { egui::IMEPurpose::Normal },
                        rect: to_global * rect,
                        cursor_rect: to_global * cursor_rect,
                        should_interrupt_composition: false,
                    });
                });
            }
        }

        if let Some(text) = copied {
            ui.ctx().copy_text(text);
        }
        if changed {
            response.mark_changed();
        }
        if secret {
            state.undo.clear();
            state.redo.clear();
            response.widget_info(|| egui::WidgetInfo::text_edit(true, "", "", hint));
        } else {
            response.widget_info(|| egui::WidgetInfo::text_edit(true, &before, &*text, hint));
        }
        ui.data_mut(|d| d.insert_temp(id, state));
        response
    }
}
