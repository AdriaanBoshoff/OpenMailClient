//! A release's notes as GitHub shows them: the Markdown a release is written in read for its
//! headings, lists, paragraphs, code and rules, and within a line for bold, italic, code and
//! links ([CommonMark] 0.31, only that much of it). Nothing in the notes is run or fetched; a link
//! is shown, and handed on only when clicked and only when it is `https://`.

use crate::theme::Tokens;
use crate::{bold, font, one_line};
use egui::{FontFamily, FontId, RichText};

/// How many blocks of notes are read; what is past them is left out.
const MAX_BLOCKS: usize = 400;

/// A stretch of a line with one look.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Block {
    /// A heading of level 1 to 6.
    Heading(u8, Vec<Span>),
    Paragraph(Vec<Span>),
    /// A list item, how deep it stands (0 at the margin) and its number if it is numbered.
    Item(usize, Option<String>, Vec<Span>),
    Code(String),
    Rule,
}

/// The notes read as blocks (CommonMark 4 and 5, the common cases).
pub(crate) fn blocks(text: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    // The lines of the paragraph or list item being read, and which it is.
    let mut open: Vec<String> = Vec::new();
    let mut item: Option<(usize, Option<String>)> = None;
    let mut code: Option<String> = None;
    let flush = |out: &mut Vec<Block>, open: &mut Vec<String>, item: &mut Option<(usize, Option<String>)>| {
        if open.is_empty() && item.is_none() {
            return;
        }
        let spans = spans(&open.join(" "));
        out.push(match item.take() {
            Some((depth, number)) => Block::Item(depth, number, spans),
            None => Block::Paragraph(spans),
        });
        open.clear();
    };
    for raw in text.lines() {
        if out.len() >= MAX_BLOCKS {
            break;
        }
        let line = raw.trim_end();
        if let Some(body) = code.as_mut() {
            if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
                out.push(Block::Code(std::mem::take(body)));
                code = None;
            } else {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(line);
            }
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let rest = line.trim_start();
        if rest.is_empty() {
            flush(&mut out, &mut open, &mut item);
            continue;
        }
        if rest.starts_with("```") || rest.starts_with("~~~") {
            flush(&mut out, &mut open, &mut item);
            code = Some(String::new());
            continue;
        }
        if indent < 4 && is_rule(rest) {
            flush(&mut out, &mut open, &mut item);
            out.push(Block::Rule);
            continue;
        }
        if indent < 4
            && let Some((level, words)) = heading(rest)
        {
            flush(&mut out, &mut open, &mut item);
            out.push(Block::Heading(level, spans(words)));
            continue;
        }
        if let Some((number, words)) = marker(rest) {
            flush(&mut out, &mut open, &mut item);
            item = Some((indent / 2, number));
            open.push(words.to_string());
            continue;
        }
        // A line that goes on the paragraph or item before it.
        open.push(rest.to_string());
    }
    if let Some(body) = code
        && out.len() < MAX_BLOCKS
    {
        out.push(Block::Code(body));
    }
    if out.len() < MAX_BLOCKS {
        flush(&mut out, &mut open, &mut item);
    }
    out
}

/// Three or more of one of `-`, `*` or `_`, with nothing else but spaces (CommonMark 4.1).
fn is_rule(line: &str) -> bool {
    let marks: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    marks.len() >= 3 && marks.first().is_some_and(|m| matches!(m, '-' | '*' | '_') && marks.iter().all(|c| c == m))
}

/// `#` to `######` and a space (CommonMark 4.2), with closing `#`s taken off.
fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let after = line.get(hashes..)?;
    if !after.is_empty() && !after.starts_with(' ') {
        return None;
    }
    Some((hashes as u8, after.trim().trim_end_matches('#').trim_end()))
}

/// A list item's marker, `-`, `*` or `+` or a number and `.` or `)`, and what follows it
/// (CommonMark 5.2).
fn marker(line: &str) -> Option<(Option<String>, &str)> {
    for bullet in ["- ", "* ", "+ "] {
        if let Some(words) = line.strip_prefix(bullet) {
            return Some((None, words.trim_start()));
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if (1..=9).contains(&digits) {
        let after = line.get(digits..)?;
        if let Some(words) = after.strip_prefix(". ").or_else(|| after.strip_prefix(") ")) {
            return Some((Some(format!("{}.", line.get(..digits)?)), words.trim_start()));
        }
    }
    None
}

/// A line's stretches: `**bold**` and `__bold__`, `*italic*` and `_italic_`, `` `code` ``,
/// `[text](address)` and `\` before a mark to keep it as it is (CommonMark 6, the common cases).
pub(crate) fn spans(line: &str) -> Vec<Span> {
    let chars: Vec<char> = line.chars().collect();
    let mut out: Vec<Span> = Vec::new();
    let mut now = Span::default();
    let mut i = 0;
    let push = |out: &mut Vec<Span>, now: &mut Span| {
        if !now.text.is_empty() {
            let next = Span { text: String::new(), ..now.clone() };
            out.push(std::mem::replace(now, next));
        }
    };
    let find = |from: usize, what: &[char]| -> Option<usize> { (from..chars.len()).find(|&j| chars.get(j..j + what.len()) == Some(what)) };
    while let Some(&c) = chars.get(i) {
        let next = chars.get(i + 1).copied();
        if c == '\\' && next.is_some_and(|n| n.is_ascii_punctuation()) {
            now.text.extend(next);
            i += 2;
            continue;
        }
        if c == '`' {
            let ticks = chars.get(i..).map_or(0, |s| s.iter().take_while(|c| **c == '`').count());
            let fence = vec!['`'; ticks];
            if let Some(end) = find(i + ticks, &fence) {
                push(&mut out, &mut now);
                let inner: String = chars.get(i + ticks..end).unwrap_or_default().iter().collect();
                out.push(Span { text: inner.trim().to_string(), code: true, ..Span::default() });
                i = end + ticks;
                continue;
            }
            now.text.extend(std::iter::repeat_n('`', ticks));
            i += ticks;
            continue;
        }
        if c == '['
            && let Some(close) = find(i + 1, &[']', '('])
            && let Some(end) = find(close + 2, &[')'])
        {
            let words: String = chars.get(i + 1..close).unwrap_or_default().iter().collect();
            let address: String = chars.get(close + 2..end).unwrap_or_default().iter().collect();
            push(&mut out, &mut now);
            for mut s in spans(&words) {
                s.bold |= now.bold;
                s.italic |= now.italic;
                s.link = Some(address.trim().to_string());
                out.push(s);
            }
            i = end + 1;
            continue;
        }
        if (c == '*' || c == '_') && next == Some(c) {
            push(&mut out, &mut now);
            now.bold = !now.bold;
            i += 2;
            continue;
        }
        if c == '*' || c == '_' {
            // Only a mark that opens before a letter or closes after one; `_` inside a word is
            // a letter of it, as in snake_case (CommonMark 6.2).
            let before = i.checked_sub(1).and_then(|b| chars.get(b)).copied();
            let opens = !now.italic && next.is_some_and(|n| !n.is_whitespace());
            let closes = now.italic && before.is_some_and(|b| !b.is_whitespace());
            let in_word = c == '_' && before.is_some_and(char::is_alphanumeric) && next.is_some_and(char::is_alphanumeric);
            if (opens || closes) && !in_word {
                push(&mut out, &mut now);
                now.italic = !now.italic;
                i += 1;
                continue;
            }
        }
        now.text.push(c);
        i += 1;
    }
    push(&mut out, &mut now);
    out
}

/// What was clicked in the notes: an address to hand on.
pub(crate) fn show(ui: &mut egui::Ui, t: &Tokens, text: &str) -> Option<String> {
    let mut clicked = None;
    for block in blocks(text) {
        match block {
            Block::Heading(level, spans) => {
                ui.add_space(if level <= 2 { 8.0 } else { 6.0 });
                let size = match level {
                    1 => 17.0,
                    2 => 15.0,
                    3 => 13.5,
                    _ => 12.5,
                };
                line(ui, t, &spans, size, true, &mut clicked);
                ui.add_space(2.0);
            }
            Block::Paragraph(spans) => {
                line(ui, t, &spans, 12.5, false, &mut clicked);
                ui.add_space(4.0);
            }
            Block::Item(depth, number, spans) => {
                ui.horizontal_top(|ui| {
                    ui.add_space(4.0 + 16.0 * depth.min(6) as f32);
                    let mark = number.unwrap_or_else(|| if depth == 0 { "•".into() } else { "◦".into() });
                    ui.add_sized([14.0, 16.0], egui::Label::new(RichText::new(mark).font(font(12.5)).color(t.text_dim)));
                    ui.vertical(|ui| line(ui, t, &spans, 12.5, false, &mut clicked));
                });
                ui.add_space(1.0);
            }
            Block::Code(body) => {
                egui::Frame::new().fill(t.pane).corner_radius(4).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let shown: String = body.chars().filter(|c| *c == '\n' || !c.is_control()).take(4000).collect();
                    ui.add(egui::Label::new(RichText::new(shown).font(FontId::new(12.0, FontFamily::Monospace)).color(t.text)).wrap());
                });
                ui.add_space(4.0);
            }
            Block::Rule => {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
            }
        }
    }
    clicked
}

/// One block's stretches, wrapped together across the width.
fn line(ui: &mut egui::Ui, t: &Tokens, spans: &[Span], size: f32, heading: bool, clicked: &mut Option<String>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for s in spans {
            let words = one_line(&s.text);
            let mut rich = if s.code {
                RichText::new(words).font(FontId::new(size - 1.0, FontFamily::Monospace)).background_color(t.pane)
            } else if s.bold || heading {
                RichText::new(words).font(bold(size))
            } else {
                RichText::new(words).font(font(size))
            };
            if s.italic {
                rich = rich.italics();
            }
            match &s.link {
                Some(address) if address.starts_with("https://") => {
                    let address = one_line(address);
                    if ui.link(rich.color(t.accent)).on_hover_text(&address).clicked() {
                        *clicked = Some(address);
                    }
                }
                Some(address) => {
                    ui.label(rich.color(t.text)).on_hover_text(one_line(address));
                }
                None => {
                    ui.label(rich.color(t.text));
                }
            }
        }
    });
}
