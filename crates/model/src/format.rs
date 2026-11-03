//! The formatting of text being written: a message's body or a signature. The text itself stays
//! a plain string, as every command that writes into a body already has it; what is kept here
//! goes beside it, counted in its characters: stretches of one look ([`Mark`]: bold, italic,
//! underline, struck through, a font, a size, a colour, a highlight, a link), each paragraph's
//! alignment, list and indent ([`Para`]), and the pictures that stand in the text, each as one
//! object replacement character (U+FFFC) whose mark names it.
//!
//! When the text is changed by anything, [`Format::follow`] puts the formatting back over it:
//! what was kept keeps its look, and what came in takes the look it was typed in. Whatever the
//! formatting holds, [`Format::fit`] makes it agree with the text, so formatting that was kept
//! wrong, or read from a hostile file, is never trusted to count right.
//!
//! [`Format::to_html`] writes it as HTML for sending, the pictures referred to by content id
//! ([RFC 2392]) so they go as parts of the message ([RFC 2387]).

use std::sync::Arc;

/// The character a picture stands as in the text (Unicode's OBJECT REPLACEMENT CHARACTER).
pub const PICTURE: char = '\u{FFFC}';
/// Pictures kept in one text at most.
pub const MAX_PICTURES: usize = 64;
/// The largest picture kept, in bytes.
pub const MAX_PICTURE_BYTES: usize = 16 << 20;
/// The largest size of text, in points.
pub const MAX_POINTS: f32 = 400.0;
/// How deep a paragraph is indented at most.
pub const MAX_INDENT: u8 = 8;
/// The longest link kept, and the longest font name.
const MAX_LINK: usize = 2048;
const MAX_FAMILY: usize = 100;

/// Red, green, blue.
pub type Rgb = [u8; 3];

/// The look of a stretch of text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mark {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// A font by its name; `None` is the text's own.
    pub family: Option<Arc<str>>,
    /// Points; `None` is the text's own.
    pub size: Option<f32>,
    pub color: Option<Rgb>,
    /// The colour behind the text, as a highlighter marks it.
    pub highlight: Option<Rgb>,
    /// Where it links to.
    pub link: Option<Arc<str>>,
    /// The picture this character is, by its [`Picture::id`].
    pub picture: Option<u32>,
}

impl Mark {
    /// Whether it looks like text with no formatting at all.
    pub fn is_plain(&self) -> bool {
        *self == Mark::default()
    }

    /// The look text typed after this takes: a picture and a link stop where they are.
    pub fn typing(&self) -> Mark {
        Mark { link: None, picture: None, ..self.clone() }
    }

    /// The same, with what is too long or makes no sense left out.
    fn clean(mut self) -> Mark {
        self.size = self.size.filter(|s| s.is_finite() && *s >= 1.0).map(|s| s.min(MAX_POINTS));
        self.family = self.family.filter(|f| !f.trim().is_empty() && f.chars().count() <= MAX_FAMILY && !f.chars().any(char::is_control));
        self.link = self.link.filter(|l| !l.trim().is_empty() && l.len() <= MAX_LINK && !l.chars().any(char::is_control));
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum List {
    #[default]
    None,
    Bullet,
    Number,
}

/// The look of a paragraph: what a line feed ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Para {
    pub align: Align,
    pub list: List,
    /// Steps in from the margin, each half an inch.
    pub indent: u8,
}

/// A picture that stands in the text.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub id: u32,
    /// The picture file as it is, PNG, JPEG, GIF and the like.
    pub bytes: Arc<[u8]>,
    /// Its media type, `image/png` and the like.
    pub kind: String,
    /// Its file name, for the part it is sent as.
    pub name: String,
    /// The size it is shown at, in pixels; 0 is its own.
    pub width: u16,
    pub height: u16,
}

/// The formatting of one text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Format {
    /// Stretches of one look, how many characters each, in order.
    pub runs: Vec<(usize, Mark)>,
    /// Each paragraph's look, one for each line feed and one more.
    pub paras: Vec<Para>,
    pub pictures: Vec<Picture>,
}

/// How many characters `s` has.
fn count(s: &str) -> usize {
    s.chars().count()
}

impl Format {
    /// Whether nothing in it is formatted: text sent as it is.
    pub fn is_plain(&self) -> bool {
        self.runs.iter().all(|(_, m)| m.is_plain()) && self.paras.iter().all(|p| *p == Para::default())
    }

    /// How many characters it covers.
    pub fn chars(&self) -> usize {
        self.runs.iter().fold(0usize, |n, (l, _)| n.saturating_add(*l))
    }

    /// Make it agree with `text`: as many characters as it has, a paragraph for each line feed
    /// and one more, a picture only on a picture's character and only one it holds, stretches
    /// of one look joined, and what is out of bounds clamped.
    pub fn fit(&mut self, text: &str) {
        let chars: Vec<char> = text.chars().collect();
        let mut runs: Vec<(usize, Mark)> = Vec::with_capacity(self.runs.len());
        let mut at = 0usize;
        let ids: Vec<u32> = self.pictures.iter().map(|p| p.id).collect();
        for (len, mark) in std::mem::take(&mut self.runs) {
            let len = len.min(chars.len().saturating_sub(at));
            if len == 0 {
                continue;
            }
            let mark = mark.clean();
            match mark.picture {
                // A picture is one character, and only a picture's character.
                Some(id) => {
                    for c in chars.get(at..at + len).unwrap_or_default() {
                        let own = *c == PICTURE && ids.contains(&id);
                        push(&mut runs, 1, Mark { picture: own.then_some(id), ..mark.clone() });
                    }
                }
                None => push(&mut runs, len, mark),
            }
            at += len;
        }
        if at < chars.len() {
            let last = runs.last().map(|(_, m)| m.typing()).unwrap_or_default();
            push(&mut runs, chars.len() - at, last);
        }
        self.runs = runs;
        let paragraphs = chars.iter().filter(|c| **c == '\n').count() + 1;
        let last = self.paras.last().copied().unwrap_or_default();
        self.paras.resize(paragraphs, last);
        for p in &mut self.paras {
            p.indent = p.indent.min(MAX_INDENT);
        }
        // Pictures no character names any more are let go.
        let used: Vec<u32> = self.runs.iter().filter_map(|(_, m)| m.picture).collect();
        self.pictures.retain(|p| used.contains(&p.id));
        self.pictures.truncate(MAX_PICTURES);
    }

    /// Put the formatting back over the text after it was changed from `old` to `new`: what
    /// both have at their start and their end keeps its look, what came in between takes
    /// `typed`, and the paragraph it came into lends its look to any paragraph it begins.
    pub fn follow(&mut self, old: &str, new: &str, typed: &Mark) {
        self.follow_from(old, new, typed, usize::MAX);
    }

    /// The same, the change known to begin no later than character `start`, where the caret
    /// was: typing " there" after "hello" in "hello world" is then put after "hello", in its
    /// look, and not after the blank that both texts also have there.
    pub fn follow_from(&mut self, old: &str, new: &str, typed: &Mark, start: usize) {
        if old == new {
            self.fit(new);
            return;
        }
        self.fit(old);
        let a: Vec<char> = old.chars().collect();
        let b: Vec<char> = new.chars().collect();
        let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count().min(start);
        let tail = a.iter().rev().zip(b.iter().rev()).take(a.len().min(b.len()) - head).take_while(|(x, y)| x == y).count();
        let (gone, came) = (head..a.len() - tail, head..b.len() - tail);
        // Paragraphs: those the lines feeds taken away ended go, and the one it all happens in
        // lends its look to each a line feed put in begins.
        let first = a.get(..head).map_or(0, |s| s.iter().filter(|c| **c == '\n').count());
        let lost = a.get(gone.clone()).map_or(0, |s| s.iter().filter(|c| **c == '\n').count());
        let made = b.get(came.clone()).map_or(0, |s| s.iter().filter(|c| **c == '\n').count());
        let lent = self.paras.get(first).copied().unwrap_or_default();
        let end = (first + 1 + lost).min(self.paras.len());
        if first < self.paras.len() {
            self.paras.splice(first + 1..end.max(first + 1), std::iter::repeat_n(lent, made));
        }
        self.splice(gone.start, gone.end, came.len(), &typed.typing());
        self.fit(new);
    }

    /// Characters `from..to` replaced by `n` of the look `mark`.
    fn splice(&mut self, from: usize, to: usize, n: usize, mark: &Mark) {
        let mut runs = Vec::with_capacity(self.runs.len() + 2);
        let mut at = 0usize;
        let mut put = false;
        for (len, m) in std::mem::take(&mut self.runs) {
            let (start, end) = (at, at + len);
            at = end;
            if start < from {
                push(&mut runs, from.min(end) - start, m.clone());
            }
            if !put && end >= from {
                push(&mut runs, n, mark.clone());
                put = true;
            }
            if end > to {
                push(&mut runs, end - to.max(start), m);
            }
        }
        if !put {
            push(&mut runs, n, mark.clone());
        }
        self.runs = runs;
    }

    /// The look of the character at `at`, or of the one before the end.
    pub fn mark_at(&self, at: usize) -> Mark {
        let mut start = 0usize;
        for (len, m) in &self.runs {
            if at < start + len {
                return m.clone();
            }
            start += len;
        }
        self.runs.last().map(|(_, m)| m.clone()).unwrap_or_default()
    }

    /// The look text typed at `at` takes: that of the character before it.
    pub fn typing_at(&self, at: usize) -> Mark {
        match at.checked_sub(1) {
            Some(before) => self.mark_at(before).typing(),
            None => self.mark_at(0).typing(),
        }
    }

    /// Whether every character of `from..to` is so; for an empty stretch, the one before it.
    pub fn all(&self, from: usize, to: usize, test: impl Fn(&Mark) -> bool) -> bool {
        if from >= to {
            return test(&self.typing_at(from));
        }
        let mut start = 0usize;
        for (len, m) in &self.runs {
            let end = start + len;
            if end > from && start < to && !test(m) {
                return false;
            }
            start = end;
        }
        true
    }

    /// Change the look of characters `from..to`.
    pub fn apply(&mut self, from: usize, to: usize, change: impl Fn(&mut Mark)) {
        let mut runs = Vec::with_capacity(self.runs.len() + 2);
        let mut at = 0usize;
        for (len, m) in std::mem::take(&mut self.runs) {
            let (start, end) = (at, at + len);
            at = end;
            let (a, b) = (from.clamp(start, end), to.clamp(start, end));
            push(&mut runs, a - start, m.clone());
            if b > a {
                let mut changed = m.clone();
                change(&mut changed);
                // A picture stays a picture whatever is done to its look.
                changed.picture = m.picture;
                push(&mut runs, b - a, changed.clean());
            }
            push(&mut runs, end - b, m);
        }
        self.runs = runs;
    }

    /// The paragraphs characters `from..to` of `text` are in, by their number.
    pub fn paras_of(text: &str, from: usize, to: usize) -> std::ops::RangeInclusive<usize> {
        let (from, to) = (from.min(to), from.max(to));
        let mut first = 0usize;
        let mut last = 0usize;
        for (n, c) in text.chars().enumerate() {
            if c == '\n' {
                if n < from {
                    first += 1;
                }
                if n < to {
                    last += 1;
                }
            }
        }
        first..=last.max(first)
    }

    /// Change the look of the paragraphs characters `from..to` of `text` are in.
    pub fn set_paras(&mut self, text: &str, from: usize, to: usize, change: impl Fn(&mut Para)) {
        self.fit(text);
        for n in Self::paras_of(text, from, to) {
            if let Some(p) = self.paras.get_mut(n) {
                change(p);
                p.indent = p.indent.min(MAX_INDENT);
            }
        }
    }

    /// A number for a new picture, one none of them has.
    fn free_id(&self) -> u32 {
        self.pictures.iter().map(|p| p.id).max().map_or(1, |n| n.saturating_add(1))
    }

    /// Put `other`, the formatting of `piece`, into `text` at character `at`, `piece` with it:
    /// pasted text or a signature with its look and pictures. The paragraph it goes into keeps
    /// its own look for the first line of it, and each line after takes the look `other` gives.
    /// It says where the piece ends; nothing is put in past the limit of pictures.
    pub fn insert(&mut self, text: &mut String, at: usize, piece: &str, other: &Format) -> usize {
        self.fit(text);
        let mut other = other.clone();
        other.fit(piece);
        // Its pictures take numbers of this text's own, as many as there is room for.
        let mut next = self.free_id();
        let room = MAX_PICTURES.saturating_sub(self.pictures.len());
        let mut renumber: Vec<(u32, u32)> = Vec::new();
        for p in other.pictures.iter().take(room) {
            renumber.push((p.id, next));
            self.pictures.push(Picture { id: next, ..p.clone() });
            next = next.saturating_add(1);
        }
        let chars: Vec<char> = text.chars().collect();
        let at = at.min(chars.len());
        let byte = text.char_indices().nth(at).map_or(text.len(), |(b, _)| b);
        let first = chars.get(..at).map_or(0, |s| s.iter().filter(|c| **c == '\n').count());
        let mut runs = Vec::new();
        let mut start = 0usize;
        for (len, m) in std::mem::take(&mut self.runs) {
            let end = start + len;
            push(&mut runs, at.min(end).saturating_sub(start), m.clone());
            if start <= at && at < end || (end == at && start == at) {
                // Its stretch is split where the piece goes in.
            }
            if end > at {
                if start <= at {
                    for (l, om) in &other.runs {
                        let picture = om.picture.and_then(|id| renumber.iter().find(|(from, _)| *from == id).map(|(_, to)| *to));
                        push(&mut runs, *l, Mark { picture, ..om.clone() });
                    }
                }
                push(&mut runs, end - at.max(start), m);
            }
            start = end;
        }
        if start <= at {
            for (l, om) in &other.runs {
                let picture = om.picture.and_then(|id| renumber.iter().find(|(from, _)| *from == id).map(|(_, to)| *to));
                push(&mut runs, *l, Mark { picture, ..om.clone() });
            }
        }
        self.runs = runs;
        let later: Vec<Para> = other.paras.iter().skip(1).copied().collect();
        let after = (first + 1).min(self.paras.len());
        self.paras.splice(after..after, later);
        text.insert_str(byte, piece);
        self.fit(text);
        at + count(piece)
    }

    /// Characters `from..to` of `text` and their formatting, as a piece of their own: what is
    /// copied out of a body.
    pub fn cut(&self, text: &str, from: usize, to: usize) -> (String, Format) {
        let (from, to) = (from.min(to), from.max(to));
        let piece: String = text.chars().skip(from).take(to - from).collect();
        let mut runs = Vec::new();
        let mut start = 0usize;
        for (len, m) in &self.runs {
            let end = start + len;
            let (a, b) = (from.clamp(start, end), to.clamp(start, end));
            push(&mut runs, b - a, m.clone());
            start = end;
        }
        let paras: Vec<Para> = Self::paras_of(text, from, to).filter_map(|n| self.paras.get(n).copied()).collect();
        let used: Vec<u32> = runs.iter().filter_map(|(_, m): &(usize, Mark)| m.picture).collect();
        let pictures = self.pictures.iter().filter(|p| used.contains(&p.id)).cloned().collect();
        let mut out = Format { runs, paras, pictures };
        out.fit(&piece);
        (piece, out)
    }

    /// A picture put in at `at`: its character goes into `text`. `None` when there is no room
    /// for another, or it is too large.
    pub fn put_picture(&mut self, text: &mut String, at: usize, picture: Picture) -> Option<usize> {
        if picture.bytes.len() > MAX_PICTURE_BYTES || picture.bytes.is_empty() {
            return None;
        }
        self.fit(text);
        if self.pictures.len() >= MAX_PICTURES {
            return None;
        }
        let id = self.free_id();
        let mark = Mark { picture: Some(id), ..self.typing_at(at) };
        let piece = Format { runs: vec![(1, mark)], paras: vec![Para::default()], pictures: vec![Picture { id, ..picture }] };
        Some(self.insert(text, at, &PICTURE.to_string(), &piece))
    }

    pub fn picture(&self, id: u32) -> Option<&Picture> {
        self.pictures.iter().find(|p| p.id == id)
    }

    /// The text as plain text: pictures left out, list items marked as a reader of plain text
    /// would write them.
    pub fn to_plain(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut number = 0usize;
        for (n, line) in text.split('\n').enumerate() {
            if n > 0 {
                out.push('\n');
            }
            let para = self.paras.get(n).copied().unwrap_or_default();
            out.push_str(&"    ".repeat(usize::from(para.indent.min(MAX_INDENT))));
            match para.list {
                List::Bullet => out.push_str("• "),
                List::Number => {
                    number += 1;
                    out.push_str(&format!("{number}. "));
                }
                List::None => {}
            }
            if para.list != List::Number {
                number = 0;
            }
            out.extend(line.chars().filter(|c| *c != PICTURE));
        }
        out
    }

    /// The text as the body of an HTML message, in `base`'s font and size; each picture is
    /// shown from the address `src` gives it: `cid:` and its content id for a message
    /// ([RFC 2392]), or `data:` and its bytes where it is kept whole ([RFC 2397]).
    pub fn to_html(&self, text: &str, base: &Mark, src: impl Fn(&Picture) -> String) -> String {
        let mut f = self.clone();
        f.fit(text);
        let chars: Vec<char> = text.chars().collect();
        let mut body = String::with_capacity(text.len() * 2);
        // Open lists, the kind and indent of each.
        let mut lists: Vec<(List, u8)> = Vec::new();
        let mut at = 0usize;
        let mut run = 0usize;
        let mut used = 0usize;
        let lines: Vec<&[char]> = chars.split(|c| *c == '\n').collect();
        for (n, line) in lines.iter().enumerate() {
            let para = f.paras.get(n).copied().unwrap_or_default();
            // Lists closed and opened to stand as deep as this paragraph does.
            let want: Option<(List, u8)> = (para.list != List::None).then_some((para.list, para.indent));
            while let Some(&(kind, depth)) = lists.last() {
                let keep = want.is_some_and(|(k, d)| d > depth || (d == depth && k == kind));
                if keep {
                    break;
                }
                body.push_str(if kind == List::Number { "</ol>" } else { "</ul>" });
                lists.pop();
            }
            if let Some((kind, depth)) = want
                && lists.last().is_none_or(|(_, d)| *d < depth)
            {
                body.push_str(if kind == List::Number {
                    "<ol style=\"margin-top:0;margin-bottom:0\">"
                } else {
                    "<ul style=\"margin-top:0;margin-bottom:0\">"
                });
                lists.push((kind, depth));
            }
            let mut style = String::from("margin:0");
            match para.align {
                Align::Left => {}
                Align::Center => style.push_str(";text-align:center"),
                Align::Right => style.push_str(";text-align:right"),
                Align::Justify => style.push_str(";text-align:justify"),
            }
            if want.is_none() && para.indent > 0 {
                style.push_str(&format!(";margin-left:{}pt", u32::from(para.indent) * 36));
            }
            body.push_str(if want.is_some() { "<li style=\"" } else { "<p style=\"" });
            body.push_str(&style);
            body.push_str("\">");
            if line.is_empty() {
                body.push_str("&nbsp;");
            }
            // The stretches of the line, each in its look.
            let end = at + line.len();
            while at < end {
                let Some((len, mark)) = f.runs.get(run) else { break };
                let start_of_run: usize = f.runs.get(..run).map_or(0, |r| r.iter().map(|(l, _)| *l).sum());
                let upto = (start_of_run + len).min(end);
                let piece: String = chars.get(at..upto).unwrap_or_default().iter().collect();
                write_run(&mut body, &piece, mark, &f, &src);
                at = upto;
                if upto >= start_of_run + len {
                    run += 1;
                }
                used += 1;
                if used > 1_000_000 {
                    break;
                }
            }
            body.push_str(if want.is_some() { "</li>" } else { "</p>" });
            body.push('\n');
            // The line feed.
            at += 1;
            while let Some((len, _)) = f.runs.get(run) {
                let start_of_run: usize = f.runs.get(..run).map_or(0, |r| r.iter().map(|(l, _)| *l).sum());
                if start_of_run + len <= at { run += 1 } else { break }
            }
        }
        for (kind, _) in lists.iter().rev() {
            body.push_str(if *kind == List::Number { "</ol>" } else { "</ul>" });
        }
        let mut base_style = String::new();
        if let Some(family) = &base.family {
            base_style.push_str(&format!("font-family:{};", css_family(family)));
        }
        if let Some(size) = base.size {
            base_style.push_str(&format!("font-size:{}pt;", points(size)));
        }
        if let Some(c) = base.color {
            base_style.push_str(&format!("color:{};", hex(c)));
        }
        format!(
            "<html>\n<head>\n<meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\">\n</head>\n<body>\n<div style=\"{}\">\n{body}</div>\n</body>\n</html>\n",
            escape(&base_style)
        )
    }
}

/// A stretch added to `runs`, joined to the one before when they look alike.
fn push(runs: &mut Vec<(usize, Mark)>, len: usize, mark: Mark) {
    if len == 0 {
        return;
    }
    match runs.last_mut() {
        Some((n, m)) if *m == mark && mark.picture.is_none() => *n = n.saturating_add(len),
        _ => runs.push((len, mark)),
    }
}

/// One stretch of text, or a picture, as HTML.
fn write_run(out: &mut String, piece: &str, mark: &Mark, f: &Format, src: &impl Fn(&Picture) -> String) {
    if let Some(picture) = mark.picture.and_then(|id| f.picture(id)) {
        let mut size = String::new();
        if picture.width > 0 {
            size.push_str(&format!(" width=\"{}\"", picture.width));
        }
        if picture.height > 0 {
            size.push_str(&format!(" height=\"{}\"", picture.height));
        }
        // Only an address of the two kinds is written: nothing that would be fetched.
        let address = src(picture);
        let lower = address.get(..5).unwrap_or_default().to_ascii_lowercase();
        if lower != "data:" && !lower.starts_with("cid:") {
            return;
        }
        out.push_str(&format!("<img src=\"{}\"{size} alt=\"{}\">", escape(&address), escape(&picture.name)));
        return;
    }
    let mut style = String::new();
    if mark.bold {
        style.push_str("font-weight:bold;");
    }
    if mark.italic {
        style.push_str("font-style:italic;");
    }
    match (mark.underline, mark.strike) {
        (true, true) => style.push_str("text-decoration:underline line-through;"),
        (true, false) => style.push_str("text-decoration:underline;"),
        (false, true) => style.push_str("text-decoration:line-through;"),
        (false, false) => {}
    }
    if let Some(family) = &mark.family {
        style.push_str(&format!("font-family:{};", css_family(family)));
    }
    if let Some(size) = mark.size {
        style.push_str(&format!("font-size:{}pt;", points(size)));
    }
    if let Some(c) = mark.color {
        style.push_str(&format!("color:{};", hex(c)));
    }
    if let Some(c) = mark.highlight {
        style.push_str(&format!("background:{};", hex(c)));
    }
    // Runs of blanks keep their width, as they were typed.
    let mut words = String::with_capacity(piece.len());
    let mut blank_before = true;
    for c in piece.chars().filter(|c| *c != PICTURE) {
        match c {
            ' ' if blank_before => words.push_str("&nbsp;"),
            '\t' => words.push_str("&nbsp;&nbsp;&nbsp;&nbsp;"),
            _ => words.push_str(&escape(&c.to_string())),
        }
        blank_before = c == ' ' || c == '\t';
    }
    if words.is_empty() {
        return;
    }
    let open_link = mark.link.as_deref().filter(|l| safe_link(l));
    if let Some(link) = open_link {
        out.push_str(&format!("<a href=\"{}\">", escape(link)));
    }
    if style.is_empty() {
        out.push_str(&words);
    } else {
        out.push_str(&format!("<span style=\"{}\">{words}</span>", escape(&style)));
    }
    if open_link.is_some() {
        out.push_str("</a>");
    }
}

/// A link that is written into what is sent: one to the web, to a mailbox, or to a telephone,
/// never one that runs something.
pub fn safe_link(link: &str) -> bool {
    let lower = link.trim().to_ascii_lowercase();
    ["http://", "https://", "mailto:", "tel:"].iter().any(|s| lower.starts_with(s))
}

fn points(size: f32) -> String {
    let s = format!("{:.1}", size.clamp(1.0, MAX_POINTS));
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// Fonts by their names as CSS takes them, first choice first: each quoted, but for the kinds
/// of font CSS names itself, with what could end the quotes left out.
fn css_family(family: &str) -> String {
    let names: Vec<String> = family
        .split(',')
        .map(|n| n.chars().filter(|c| !matches!(c, '"' | '\'' | ';' | '\\' | '<' | '>' | '{' | '}')).collect::<String>().trim().to_string())
        .filter(|n| !n.is_empty())
        .take(8)
        .map(|n| if matches!(n.to_ascii_lowercase().as_str(), "serif" | "sans-serif" | "monospace" | "cursive") { n } else { format!("'{n}'") })
        .collect();
    names.join(",")
}

/// Text as HTML writes it.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold() -> Mark {
        Mark { bold: true, ..Mark::default() }
    }

    #[test]
    fn typing_keeps_the_look_around_it_and_takes_its_own() {
        let mut f = Format::default();
        f.fit("hello world");
        f.apply(0, 5, |m| m.bold = true);
        assert_eq!(f.runs, vec![(5, bold()), (6, Mark::default())]);
        // Typed after "hello", in bold as the character before it is.
        let typed = f.typing_at(5);
        f.follow_from("hello world", "hello there world", &typed, 5);
        assert_eq!(f.runs, vec![(11, bold()), (6, Mark::default())]);
        // Deleted across the two looks.
        f.follow("hello there world", "help world", &Mark::default());
        assert_eq!(f.runs, vec![(3, bold()), (7, Mark::default())]);
        assert!(f.all(0, 3, |m| m.bold) && !f.all(0, 4, |m| m.bold));
    }

    #[test]
    fn paragraphs_follow_line_feeds() {
        let mut f = Format::default();
        f.fit("one\ntwo");
        f.set_paras("one\ntwo", 5, 5, |p| p.align = Align::Center);
        assert_eq!(f.paras, vec![Para::default(), Para { align: Align::Center, ..Para::default() }]);
        // A line feed in the centred paragraph begins another centred one.
        f.follow("one\ntwo", "one\ntw\no", &Mark::default());
        assert_eq!(f.paras.len(), 3);
        assert_eq!(f.paras[2].align, Align::Center);
        // Joined again: the first of them keeps its look.
        f.follow("one\ntw\no", "onetw\no", &Mark::default());
        assert_eq!(f.paras, vec![Para::default(), Para { align: Align::Center, ..Para::default() }]);
    }

    #[test]
    fn a_picture_is_one_character_and_goes_with_it() {
        let mut text = String::from("ab");
        let mut f = Format::default();
        let pic = Picture { id: 0, bytes: Arc::from(&b"\x89PNG"[..]), kind: "image/png".into(), name: "logo.png".into(), width: 0, height: 0 };
        assert_eq!(f.put_picture(&mut text, 1, pic), Some(2));
        assert_eq!(text, "a\u{FFFC}b");
        assert_eq!(f.pictures.len(), 1);
        let html = f.to_html(&text, &Mark::default(), |p| format!("cid:pic{}", p.id));
        assert!(html.contains("<img src=\"cid:pic1\""), "{html}");
        assert_eq!(f.to_plain(&text), "ab");
        // Typing over it lets it go.
        f.follow(&text, "axb", &Mark::default());
        assert!(f.pictures.is_empty() && f.runs.iter().all(|(_, m)| m.picture.is_none()));
    }

    #[test]
    fn pasted_text_brings_its_look_and_pictures() {
        let mut text = String::from("Hi there");
        let mut f = Format::default();
        let mut piece_f = Format::default();
        let mut piece = String::from("Regards\nPat ");
        piece_f.fit(&piece);
        piece_f.apply(0, 7, |m| m.color = Some([0x80, 0, 0]));
        piece_f.set_paras(&piece, 9, 9, |p| p.align = Align::Right);
        let pic = Picture { id: 7, bytes: Arc::from(&b"GIF89a"[..]), kind: "image/gif".into(), name: "a.gif".into(), width: 20, height: 10 };
        piece_f.put_picture(&mut piece, 12, pic).unwrap();
        let end = f.insert(&mut text, 2, &piece, &piece_f);
        assert_eq!(text, "HiRegards\nPat \u{FFFC} there");
        assert_eq!(end, 15);
        assert_eq!(f.mark_at(2).color, Some([0x80, 0, 0]));
        assert_eq!(f.paras, vec![Para::default(), Para { align: Align::Right, ..Para::default() }]);
        assert_eq!(f.pictures.len(), 1);
        assert_eq!(f.mark_at(14).picture, Some(f.pictures[0].id));
        // Cut out again, it is the same.
        let (back, back_f) = f.cut(&text, 2, 15);
        assert_eq!(back, piece);
        assert_eq!(back_f.runs.len(), piece_f.runs.len());
    }

    #[test]
    fn html_is_written_with_lists_and_escaped() {
        let text = "Title\none\ntwo\n<b>&";
        let mut f = Format::default();
        f.fit(text);
        f.apply(0, 5, |m| {
            m.bold = true;
            m.size = Some(14.0);
            m.family = Some(Arc::from("Arial\"><script>"));
        });
        f.set_paras(text, 6, 12, |p| p.list = List::Bullet);
        f.apply(14, 17, |m| m.link = Some(Arc::from("javascript:alert(1)")));
        let html = f.to_html(text, &Mark { size: Some(11.0), family: Some(Arc::from("Calibri, sans-serif")), ..Mark::default() }, |_| String::new());
        assert!(html.contains("font-family:&#39;Calibri&#39;,sans-serif;"), "{html}");
        assert!(html.contains("<ul") && html.contains("<li") && html.contains("</ul>"), "{html}");
        assert!(html.contains("font-weight:bold;") && html.contains("font-size:14pt;"));
        assert!(!html.contains("<script>") && !html.contains("javascript:"), "{html}");
        assert!(html.contains("&lt;b&gt;&amp;"));
        assert_eq!(f.to_plain(text), "Title\n• one\n• two\n<b>&");
    }

    #[test]
    fn formatting_kept_wrong_is_made_to_agree() {
        let mut f = Format {
            runs: vec![(usize::MAX, bold()), (3, Mark { picture: Some(9), ..Mark::default() }), (0, Mark::default())],
            paras: vec![Para { indent: 200, ..Para::default() }; 40],
            pictures: Vec::new(),
        };
        f.fit("abc\nd");
        assert_eq!(f.runs, vec![(5, bold())]);
        assert_eq!(f.paras.len(), 2);
        assert_eq!(f.paras[0].indent, MAX_INDENT);
        f.fit("");
        assert!(f.runs.is_empty() && f.paras.len() == 1);
        for (old, new) in [("", "x"), ("x", ""), ("\n\n", "\n"), ("é\u{FFFC}", "\u{FFFC}é\n"), ("abc", "abcabc")] {
            f.follow(old, new, &bold());
            assert_eq!(f.chars(), new.chars().count());
        }
    }

    #[test]
    fn any_edit_keeps_it_agreeing_with_the_text() {
        // A small generator of its own, so the edits are the same each run.
        let mut seed = 0x2545_f491_u32;
        let mut next = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as usize % n.max(1)
        };
        let alphabet = ['a', 'b', ' ', '\n', PICTURE, 'é', '\u{05D0}'];
        let mut text = String::from("start");
        let mut f = Format::default();
        f.fit(&text);
        for _ in 0..2000 {
            let chars: Vec<char> = text.chars().collect();
            let from = next(chars.len() + 1);
            let to = (from + next(4)).min(chars.len());
            match next(4) {
                0 => f.apply(from, to + next(3), |m| m.bold = !m.bold),
                1 => f.set_paras(&text, from, to, |p| p.indent = p.indent.saturating_add(3)),
                _ => {
                    let put: Vec<char> = (0..next(4)).map(|_| alphabet[next(alphabet.len())]).collect();
                    let new: String = chars[..from].iter().chain(&put).chain(&chars[to..]).collect();
                    let typed = f.typing_at(from);
                    f.follow_from(&text, &new, &typed, from);
                    text = new;
                }
            }
            f.fit(&text);
            assert_eq!(f.chars(), text.chars().count());
            assert_eq!(f.paras.len(), text.matches('\n').count() + 1);
            let _ = f.to_html(&text, &Mark::default(), |p| format!("cid:{}", p.id));
        }
    }
}
