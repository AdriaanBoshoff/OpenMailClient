//! Flows blocks of a formatted document down pages: paragraphs into lines, tables into rows
//! of cells, each moving on to the next page where it runs out of room.
//!
//! Measures in a document are in pixels, 96 to the inch (CSS Values Level 3, section 5.2);
//! on the page they are points, 72 to the inch.

use std::sync::Arc;

use openmailclient_formats::rich::{
    Align, Block, Cell, Edge, Edges, Float, Frame, Leader, Line, MAX_RADIUS, Para, ParaKind, Picture, Rgb, Run, Script, Style, TAB_STEP, Tab, Table,
    TableWidth, VAlign, any_edge, boxed_run, one_edge as rich_one_edge, tab_room, tab_stop,
};

use crate::fonts::{Fonts, Pick};
use crate::{Fill, MAX_PAGES, Page, Paper, Placed, Rule, Said, Setup, Text};
use openmailclient_fonts::script;

/// Points to the pixel.
const PX: f32 = 0.75;
/// As the reading pane has them, in pixels.
const INDENT: f32 = 40.0;
const QUOTE: f32 = 14.0;
const MAX_EDGE: u8 = 12;
const MAX_CELL_ROOM: u16 = 64;
/// Room between a picture at one side of a paragraph and the text beside it, in points.
const FLOAT_GAP: f32 = 8.0 * PX;
/// A picture stands at one side only when this much width is left for the text beside it.
const MIN_BESIDE: f32 = 120.0 * PX;
/// Runs of a paragraph looked through for a picture that stands at one side.
const MAX_FLOAT_SEARCH: usize = 64;

const MAX_COLUMNS: usize = 64;
/// Tables inside tables that are still set as tables; deeper ones are set cell under cell.
const MAX_TABLE_DEPTH: usize = 8;
/// Nesting of any kind that is still followed.
const MAX_DEPTH: usize = 64;
/// Narrowest column, in points.
const MIN_COLUMN: f32 = 12.0;
/// Rows whose cells are measured to share out the columns.
const MEASURED_ROWS: usize = 200;
/// Glyphs one layout may place or measure.
const MAX_GLYPHS: usize = 4_000_000;
/// Glyphs in one paragraph.
const MAX_PARA_GLYPHS: usize = 400_000;
/// Characters of a line the print sets itself: a page's head or foot, a list's marker.
const MAX_WORDS_CHARS: usize = 400;
/// Room kept at the foot of each page for its number.
pub(crate) const FOOT: f32 = 16.0;
/// Room over the body for the header, in points.
pub(crate) const HEAD: f32 = 16.0;
/// The least height of body a page keeps under a header.
const MIN_BODY: f32 = 24.0;

const BLACK: Rgb = [0, 0, 0];
const GREY: Rgb = [128, 128, 128];
const FAINT: Rgb = [110, 110, 110];
const LINK: Rgb = [0, 84, 166];

/// What text is set in when it does not say.
#[derive(Clone)]
pub(crate) struct Base {
    pub family: Arc<str>,
    pub size: f32,
}

/// A place on the pages: the page and how far down it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct At {
    pub page: usize,
    pub y: f32,
}

fn later(a: At, b: At) -> At {
    if (b.page, b.y) > (a.page, a.y) { b } else { a }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Ink,
    Space,
    Break,
}

/// One character as it will be set.
#[derive(Clone, Copy)]
struct Glyph {
    ch: char,
    pick: Pick,
    glyph: u16,
    advance: f32,
    /// What of the advance is kerning with the glyph after it.
    kern: f32,
    look: usize,
    kind: Kind,
    /// A picture: its place among the paragraph's pictures. Its width is the advance.
    picture: Option<usize>,
    /// A picture's height; nothing for text.
    tall: f32,
    /// A tab: what fills the room it takes.
    lead: Leader,
    /// Which of the paragraph's characters it is, and how deep in right-to-left text it
    /// stands (UAX #9): odd is right to left.
    at: usize,
    level: u8,
    /// How many of the paragraph's characters it stands for: more than one for letters drawn
    /// as one glyph, none for a glyph that is drawn with the one before it for one character.
    span: u16,
    /// How far from its place the font's tables put it: to the right, and up.
    dx: f32,
    dy: f32,
}

/// How a run's glyphs look.
#[derive(Clone, Copy, PartialEq)]
struct Look {
    size: f32,
    color: Rgb,
    under: bool,
    strike: bool,
    back: Option<Rgb>,
    /// How far above the baseline the glyphs stand.
    rise: f32,
    /// The border drawn round the text, where it has one.
    edge: Option<Edge>,
}

/// A picture standing at one side, with room beside it still to fill.
#[derive(Clone, Copy)]
struct Aside {
    left: bool,
    /// Where text beside it may come to: the picture's far edge and the gap, across the page.
    edge: f32,
    page: usize,
    bottom: f32,
}

/// Pictures left standing at once, and those one paragraph sets at its sides.
const MAX_ASIDES: usize = 16;
const MAX_OWN_ASIDES: usize = 8;

/// How many fills, lines, texts and pictures a page held at some moment.
type Mark = (usize, usize, usize, usize);

pub(crate) struct Flow<'f, 'l> {
    pub fonts: &'f mut Fonts<'l>,
    /// The sheet as it lies. Its `margin` is not looked at: `margins` has the four.
    pub paper: Paper,
    /// Room left free at the top, the right, the bottom and the left.
    pub(crate) margins: [f32; 4],
    /// Room the header and the footer take inside the margins; nothing where there is none.
    pub(crate) head: f32,
    pub(crate) foot: f32,
    pub pages: Vec<Page>,
    pub base: Base,
    /// The limits were reached: nothing more is set.
    pub full: bool,
    /// No font at all could be found for some text.
    pub no_font: bool,
    glyphs_left: usize,
    sizes: &'f dyn Fn(&Picture) -> Option<(u16, u16)>,
    /// The pictures that stand at the sides of the blocks being set.
    asides: Vec<Aside>,
    /// How far what is being set lies over the fills of the boxes it stands inside.
    lift: u16,
    /// How far the corners of the next box drawn are rounded, in pixels; taken by `boxed`.
    round: u16,
}

fn px(v: u16) -> f32 {
    f32::from(v) * PX
}

/// Give each tab the room that takes what follows it to its stop. A tab past the end of the
/// first line stands where nothing here can say, and keeps the room it had.
fn set_tabs(glyphs: &mut [Glyph], tabs: &[Tab], pre: bool, room: f32) {
    let mut x = 0.0f32;
    for i in 0..glyphs.len() {
        let Some(g) = glyphs.get(i).copied() else { break };
        if g.kind == Kind::Break {
            x = 0.0;
            continue;
        }
        if g.ch == '\t' && x < room {
            // The text up to the next tab or the end of the line, and up to its decimal point.
            let (mut after, mut point) = (0.0f32, None);
            for next in glyphs.get(i + 1..).unwrap_or_default() {
                if next.kind == Kind::Break || next.ch == '\t' {
                    break;
                }
                if next.ch == '.' && point.is_none() {
                    point = Some(after);
                }
                after += next.advance;
            }
            // Preformatted text has a stop every eight characters; a tab starts as four spaces.
            let step = if pre { g.advance * 2.0 } else { px(TAB_STEP) };
            let wide = tab_room(tabs, PX, step, x, after, point.unwrap_or(after));
            if let Some(g) = glyphs.get_mut(i) {
                g.advance = wide;
                g.lead = tab_stop(tabs, PX, x).map_or(Leader::None, |(_, stop)| stop.lead);
            }
            x += wide;
            continue;
        }
        x += g.advance;
    }
}

/// How many letters beside a run its face is shown when it shapes the run.
const CONTEXT: usize = 4;

/// The most characters a piece of text says it stands for.
const MAX_SAID: usize = 4_000;

/// A space that does not end a line is found and copied as a space.
fn plain(ch: char) -> char {
    if ch == '\u{a0}' { ' ' } else { ch }
}

/// The character a glyph is said to be in a piece of text. In text written from the right
/// that is the shape that is seen: a bracket that opens is drawn as one that closes, and a
/// reader of the file that turns the glyphs round turns the brackets with them.
fn shown(ch: char, rtl: bool) -> char {
    if rtl { script::mirrored(plain(ch)) } else { plain(ch) }
}

/// Which glyphs of a piece are not one character each, and what they stand for together.
/// Each glyph comes with its character, where that stands in the paragraph and how many
/// characters the glyph stands for; glyphs of one place in the paragraph are read together.
/// Text written from the right says nothing more for being that: its glyphs are in the order
/// they are seen in, which is what a reader of the file expects and turns round itself.
fn said(run: &[(u16, char, usize, u16)], stored: &[char]) -> Vec<Said> {
    let mut out = Vec::new();
    let (mut from, mut room) = (0usize, MAX_SAID);
    while let Some((_, ch, at, _)) = run.get(from).copied() {
        let count = run.get(from..).unwrap_or_default().iter().take_while(|g| g.2 == at).count().max(1);
        let span = run.get(from..from + count).unwrap_or_default().iter().map(|g| usize::from(g.3)).max().unwrap_or(0);
        let chars = stored.get(at..at.saturating_add(span)).unwrap_or_default();
        if count != 1 || chars.len() != 1 || chars.first().map(|c| plain(*c)) != Some(plain(ch)) {
            let text: String = chars.iter().take(room).map(|c| plain(*c)).collect();
            room = room.saturating_sub(chars.len());
            out.push(Said { from, count, text });
        }
        from += count;
    }
    out
}

/// Where the line that starts at `start` ends when it has `width` of room, and where the next
/// one starts. A line ends at the last space it has room for; a word longer than a line is cut
/// where it must be. A paragraph of nothing has one line; `None` when no line is left.
fn line_end(glyphs: &[Glyph], start: usize, width: f32) -> Option<(usize, usize)> {
    if start >= glyphs.len() {
        return (start == 0).then_some((0, 0));
    }
    let (mut wide, mut i) = (0.0f32, start);
    let mut after_space: Option<usize> = None;
    while let Some(g) = glyphs.get(i) {
        match g.kind {
            Kind::Break => return Some((i, i + 1)),
            Kind::Space => {
                wide += g.advance;
                after_space = Some(i + 1);
                i += 1;
            }
            Kind::Ink if wide + g.advance > width + 0.01 && i > start => {
                let cut = after_space.filter(|at| *at > start).unwrap_or(i);
                return Some((cut, cut));
            }
            Kind::Ink => {
                wide += g.advance;
                i += 1;
            }
        }
    }
    Some((glyphs.len(), glyphs.len()))
}

/// Room a border takes inside its box, in points: left and top, then right and bottom.
fn frame_room(frame: &Frame) -> ((f32, f32), (f32, f32)) {
    let side = |i: usize| frame.edges.get(i).map_or(0.0, |e| px(u16::from(e.width.min(MAX_EDGE))));
    let [above, right, below, left] = frame.padding.map(|v| px(v.min(MAX_CELL_ROOM)));
    // Text does not touch a line beside it.
    let clear = |w: f32, pad: f32| if w > 0.0 { w + pad.max(1.5) } else { pad };
    ((clear(side(3), left), side(0) + above), (clear(side(1), right), side(2) + below))
}

/// Room between a cell's edge and what it holds, in points: above, right, below and left.
fn padding(table: &Table, cell: &Cell) -> [f32; 4] {
    let [top, right, bottom, left] = cell.padding.unwrap_or([table.padding; 4]);
    // A cell's own lines are drawn inside it: what they take past a hair line is not the text's.
    let line = |a: usize, b: usize| {
        let wide = |i: usize| cell.edges.and_then(|e| e.get(i).map(|e| e.width.min(MAX_EDGE))).unwrap_or(0);
        px(u16::from(wide(a).max(wide(b)).saturating_sub(1)))
    };
    let room = |v: u16| px(v.min(MAX_CELL_ROOM));
    [room(top) + line(0, 2), room(right) + line(1, 3), room(bottom) + line(0, 2), room(left) + line(1, 3)]
}

/// Give the columns that ask for a share of the table's width that share of `room`, and the
/// room that is left to the others.
fn shares(table: &Table, places: &[Vec<usize>], widths: &mut [f32], room: f32) {
    let mut asked = vec![0.0f32; widths.len()];
    for (row, starts) in table.rows.iter().zip(places).take(MEASURED_ROWS) {
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
    let total: f32 = widths.iter().sum();
    if total > 0.0 && (others <= 0.0 || left <= 0.0) {
        widths.iter_mut().for_each(|w| *w *= room / total);
    }
}

/// A cell that is set and waits for the row it ends in.
struct Waiting<'a> {
    cell: &'a Cell,
    x: f32,
    width: f32,
    start: At,
    end: At,
    last_row: usize,
    before: Mark,
    after: Mark,
}

impl<'f, 'l> Flow<'f, 'l> {
    pub fn new(fonts: &'f mut Fonts<'l>, setup: &Setup, base: Base, sizes: &'f dyn Fn(&Picture) -> Option<(u16, u16)>) -> Self {
        let sane = |v: f32, least: f32, most: f32| if v.is_finite() { v.clamp(least, most) } else { least };
        let (width, height) = (sane(setup.paper.width, 72.0, 14_400.0), sane(setup.paper.height, 72.0, 14_400.0));
        let (width, height) = if setup.landscape && width < height { (height, width) } else { (width, height) };
        // A third of the sheet is the most an edge keeps free, so that a third is left.
        let [top, right, bottom, left] = setup.margins;
        let margins = [sane(top, 0.0, height / 3.0), sane(right, 0.0, width / 3.0), sane(bottom, 0.0, height / 3.0), sane(left, 0.0, width / 3.0)];
        let asked = |parts: &[String; 3]| setup.edges && parts.iter().any(|part| !part.trim().is_empty());
        let foot = if asked(&setup.footer) { FOOT } else { 0.0 };
        let room = height - margins[0] - margins[2] - foot;
        let head = if asked(&setup.header) && room - HEAD >= MIN_BODY { HEAD } else { 0.0 };
        Self {
            fonts,
            paper: Paper { width, height, margin: 0.0 },
            margins,
            head,
            foot,
            pages: Vec::new(),
            base,
            full: false,
            no_font: false,
            glyphs_left: MAX_GLYPHS,
            sizes,
            asides: Vec::new(),
            lift: 0,
            round: 0,
        }
    }

    pub fn top(&self) -> f32 {
        self.margins[0] + self.head
    }

    fn bottom(&self) -> f32 {
        self.paper.height - self.margins[2] - self.foot
    }

    pub fn left(&self) -> f32 {
        self.margins[3]
    }

    pub fn width(&self) -> f32 {
        self.paper.width - self.margins[1] - self.margins[3]
    }

    pub fn start(&mut self) -> At {
        self.room(At { page: 0, y: self.top() }, 0.0)
    }

    /// The top of the page after the one `at` is on. Makes the page.
    pub fn next_page(&mut self, at: At) -> At {
        self.room(At { page: at.page.saturating_add(1), y: self.top() }, 0.0)
    }

    /// The height of the room for the body on a page.
    pub fn tall(&self) -> f32 {
        (self.bottom() - self.top()).max(0.0)
    }

    fn page(&mut self, page: usize) -> Option<&mut Page> {
        self.pages.get_mut(page)
    }

    /// A place with `need` of room below it: `at`, or the top of the next page. Makes the page.
    fn room(&mut self, at: At, need: f32) -> At {
        let mut at = at;
        if at.y + need > self.bottom() + 0.01 && at.y > self.top() + 0.01 {
            at = At { page: at.page.saturating_add(1), y: self.top() };
        }
        while self.pages.len() <= at.page {
            if self.pages.len() >= MAX_PAGES {
                self.full = true;
                return At { page: self.pages.len().saturating_sub(1), y: self.bottom() };
            }
            self.pages.push(Page { width: self.paper.width, height: self.paper.height, ..Page::default() });
        }
        at
    }

    /// Where what does not stand beside a picture goes on: `at`, or under the lowest of the
    /// pictures that stand at the sides. No picture stands there afterwards.
    fn below_aside(&mut self, at: At) -> At {
        let lowest = self.asides.drain(..).filter(|a| a.page == at.page && at.y < a.bottom).map(|a| a.bottom).reduce(f32::max);
        match lowest {
            Some(bottom) => At { page: at.page, y: bottom.min(self.bottom()) },
            None => at,
        }
    }

    /// The room across that the pictures standing at the sides leave, of the room from `left`
    /// that is `room` wide, for a line that starts at `at`.
    fn between_asides(&self, left: f32, room: f32, at: At) -> (f32, f32) {
        let live = self.asides.iter().filter(|a| a.page == at.page && at.y < a.bottom - 0.01);
        let from = live.clone().filter(|a| a.left).map(|a| a.edge).fold(left, f32::max);
        let to = live.filter(|a| !a.left).map(|a| a.edge).fold(left + room, f32::min);
        (from.min(left + room), to)
    }

    fn mark(&self, page: usize) -> Mark {
        self.pages.get(page).map_or((0, 0, 0, 0), |p| (p.fills.len(), p.rules.len(), p.texts.len(), p.pictures.len()))
    }

    /// Move what was put on a page between two moments down by `by`.
    fn lower(&mut self, page: usize, from: Mark, to: Mark, by: f32) {
        let Some(page) = self.page(page) else { return };
        for fill in page.fills.get_mut(from.0..to.0).unwrap_or_default() {
            fill.y += by;
        }
        for rule in page.rules.get_mut(from.1..to.1).unwrap_or_default() {
            rule.from.1 += by;
            rule.to.1 += by;
        }
        for text in page.texts.get_mut(from.2..to.2).unwrap_or_default() {
            text.y += by;
        }
        for picture in page.pictures.get_mut(from.3..to.3).unwrap_or_default() {
            picture.y += by;
        }
    }

    fn rule(&mut self, page: usize, from: (f32, f32), to: (f32, f32), width: f32, color: Rgb, line: Line) {
        if let Some(page) = self.page(page) {
            page.rules.push(Rule { from, to, width, color, line });
        }
    }

    /// The size a picture is printed at, in points: what the body asks for, else its own, never
    /// wider than `room` or taller than a page.
    fn picture_size(&self, asked: (u16, u16), natural: (u16, u16), room: f32) -> (f32, f32) {
        let natural = (f32::from(natural.0.max(1)), f32::from(natural.1.max(1)));
        let (w, h) = (f32::from(asked.0), f32::from(asked.1));
        let (w, h) = match (w > 0.0, h > 0.0) {
            (true, true) => (w, h),
            (true, false) => (w, natural.1 * w / natural.0),
            (false, true) => (natural.0 * h / natural.1, h),
            (false, false) => natural,
        };
        let (w, h) = (w * PX, h * PX);
        let page = (self.bottom() - self.top()).max(1.0);
        let fit = (room.max(1.0) / w).min(page / h).min(1.0);
        ((w * fit).max(0.1), (h * fit).max(0.1))
    }

    /// The glyphs of a paragraph that has `room` across, the looks they point at, and the
    /// pictures among them. The runs at `aside` stand at the sides and are not among them.
    fn glyphs<'p>(&mut self, para: &'p Para, room: f32, aside: &[usize]) -> (Vec<Glyph>, Vec<Look>, Vec<&'p Picture>, Vec<char>) {
        let mut stored: Vec<char> = Vec::new();
        let (mut out, looks, pictures) = self.glyphs_stored(para, room, aside, &mut stored);
        self.directed(para.rtl, &mut out, &stored, &looks);
        let out = self.shaped(out, &stored, &looks);
        (out, looks, pictures, stored)
    }

    /// The glyphs of a paragraph as the fonts' own tables shape them: each stretch of one
    /// face, one look and one direction is handed to the face whole, so letters that are
    /// drawn as one glyph are, letters that join their neighbours do, marks stand where the
    /// font puts them and pairs stand as close as it says. A stretch its face does not shape
    /// stays as it was set, glyph for character.
    fn shaped(&mut self, glyphs: Vec<Glyph>, stored: &[char], looks: &[Look]) -> Vec<Glyph> {
        let plain = |g: &Glyph| g.picture.is_none() && g.kind != Kind::Break && g.ch != '\t';
        let mut out: Vec<Glyph> = Vec::with_capacity(glyphs.len());
        let mut i = 0usize;
        while let Some(first) = glyphs.get(i) {
            let same = |g: &Glyph| plain(g) && g.pick == first.pick && g.look == first.look && g.level == first.level;
            let count = if plain(first) { glyphs.get(i..).unwrap_or_default().iter().take_while(|g| same(g)).count().max(1) } else { 1 };
            let run = glyphs.get(i..i.saturating_add(count)).unwrap_or_default();
            i += count;
            let before = out.len();
            if !plain(first) || self.shape_run(run, stored, looks, &mut out).is_none() || out.len() > MAX_PARA_GLYPHS {
                out.truncate(before);
                out.extend_from_slice(run);
            }
        }
        out
    }

    /// One stretch of glyphs shaped by its face, added to `out` in the order it is read.
    /// `None` when the face does not shape it; what was added by then is not to be kept.
    fn shape_run(&mut self, run: &[Glyph], stored: &[char], looks: &[Look], out: &mut Vec<Glyph>) -> Option<()> {
        let (first, last) = (run.first()?, run.last()?);
        let size = looks.get(first.look)?.size;
        let rtl = first.level % 2 == 1;
        // What stands between two of the glyphs and has none of its own is read with them: a
        // joiner, a letter another took into itself.
        let end = last.at.checked_add(usize::from(last.span.max(1)))?;
        let text: Vec<(usize, char)> = (first.at..end).filter_map(|at| stored.get(at).map(|c| (at, *c))).filter(|(_, c)| !c.is_control()).collect();
        // The letters beside the run, as far as the word goes.
        let letter = |c: &&char| !c.is_control() && !c.is_whitespace() && **c != '\u{fffc}';
        let before: String =
            stored.get(..first.at).unwrap_or_default().iter().rev().take_while(letter).take(CONTEXT).collect::<Vec<_>>().into_iter().rev().collect();
        let after: String = stored.get(end..).unwrap_or_default().iter().take_while(letter).take(CONTEXT).collect();
        let mut shaped = self.fonts.shape(first.pick.face, &text, rtl, &before, &after)?;
        if shaped.is_empty() || shaped.len() > text.len().saturating_mul(4).saturating_add(16) {
            return None;
        }
        // The face was picked for the shapes the letters were given, and may not have the
        // letters themselves: then the shapes stay.
        if shaped.iter().any(|s| s.glyph == 0) && run.iter().all(|g| g.glyph != 0) {
            return None;
        }
        // The shaper gives the glyphs from the left; here they are kept in the order read.
        if rtl {
            shaped.reverse();
        }
        if shaped.windows(2).any(|pair| matches!(pair, [a, b] if a.cluster > b.cluster)) {
            return None;
        }
        let face = self.fonts.face(first.pick.face)?;
        for (n, s) in shaped.iter().enumerate() {
            // The glyph the character had says what it is: a space, or ink.
            let had = run.get(run.partition_point(|g| g.at <= s.cluster).saturating_sub(1)).copied()?;
            let starts = n.checked_sub(1).and_then(|before| shaped.get(before)).is_none_or(|before| before.cluster != s.cluster);
            let next = shaped.get(n + 1..).unwrap_or_default().iter().find(|after| after.cluster != s.cluster).map_or(end, |after| after.cluster);
            let span = if starts { u16::try_from(next.saturating_sub(s.cluster)).unwrap_or(u16::MAX) } else { 0 };
            let advance = s.advance * size;
            // No glyph is put further from its place than two of its own size.
            let (dx, dy) = (s.dx.clamp(-2.0, 2.0) * size, s.dy.clamp(-2.0, 2.0) * size);
            if !(advance.is_finite() && dx.is_finite() && dy.is_finite()) || advance < 0.0 {
                return None;
            }
            out.push(Glyph {
                ch: stored.get(s.cluster).copied().unwrap_or(had.ch),
                glyph: s.glyph,
                advance,
                // What the face's tables changed of the width the glyph has by itself.
                kern: Some(advance - face.advance(s.glyph, size)).filter(|kern| kern.abs() > size / 10_000.0).unwrap_or(0.0),
                kind: if had.at == s.cluster { had.kind } else { Kind::Ink },
                at: s.cluster,
                span,
                dx,
                dy,
                ..had
            });
        }
        Some(())
    }

    /// Say how deep in right-to-left text each glyph stands, and turn round the brackets that
    /// stand in it. `stored` is the paragraph's characters in the order they are read.
    fn directed(&mut self, rtl: bool, glyphs: &mut [Glyph], stored: &[char], looks: &[Look]) {
        if !rtl && !stored.iter().any(|c| script::is_rtl(*c)) {
            return;
        }
        let text: String = stored.iter().collect();
        let levels = script::levels(&text, Some(rtl));
        for g in glyphs.iter_mut() {
            g.level = levels.get(g.at).copied().unwrap_or(u8::from(rtl));
            if g.level % 2 == 0 || g.picture.is_some() {
                continue;
            }
            // Kerning is between a glyph and the one read after it, which here stands before it.
            g.advance -= g.kern;
            g.kern = 0.0;
            let turned = script::mirrored(g.ch);
            if turned == g.ch {
                continue;
            }
            let glyph = self.fonts.glyph(g.pick.face, turned);
            let size = looks.get(g.look).map_or(self.base.size, |l| l.size);
            if glyph != 0
                && let Some(advance) = self.fonts.face(g.pick.face).map(|f| f.advance(glyph, size))
            {
                (g.glyph, g.advance) = (glyph, advance);
            }
        }
    }

    /// Arabic letters as the shapes they have beside their neighbours, run by run; nothing for
    /// a paragraph with none. A letter's neighbours may stand in another run.
    fn shapes(&mut self, para: &Para) -> Vec<Vec<char>> {
        let arabic = |c: char| matches!(c, '\u{0600}'..='\u{06FF}');
        if !para.runs.iter().any(|run| run.picture.is_none() && run.text.chars().any(arabic)) {
            return Vec::new();
        }
        let mut all: Vec<char> = Vec::new();
        let mut counts: Vec<usize> = Vec::new();
        for run in &para.runs {
            let before = all.len();
            match run.picture {
                // A picture parts the letters at its sides.
                Some(_) => all.push('\u{fffc}'),
                None => all.extend(run.text.chars().take(MAX_PARA_GLYPHS.saturating_sub(before))),
            }
            counts.push(all.len() - before);
        }
        let mut chain = self.fonts.chain(&self.base.family.clone(), false, false, false);
        let shaped = script::joined(&all, &mut |c| self.fonts.resolve(&mut chain, c).is_some_and(|(_, glyph)| glyph != 0));
        let mut rest = shaped.as_slice();
        counts
            .into_iter()
            .map(|count| {
                let (own, after) = rest.split_at_checked(count).unwrap_or((rest, &[]));
                rest = after;
                own.to_vec()
            })
            .collect()
    }

    fn glyphs_stored<'p>(&mut self, para: &'p Para, room: f32, aside: &[usize], stored: &mut Vec<char>) -> (Vec<Glyph>, Vec<Look>, Vec<&'p Picture>) {
        let shapes = self.shapes(para);
        let mut out: Vec<Glyph> = Vec::new();
        let mut looks: Vec<Look> = Vec::new();
        let mut pictures: Vec<&'p Picture> = Vec::new();
        for (n, run) in para.runs.iter().enumerate() {
            if aside.contains(&n) {
                continue;
            }
            let s = &run.style;
            let mut size = s.size.filter(|v| v.is_finite()).unwrap_or(self.base.size).clamp(4.0, 96.0);
            let mut rise = 0.0;
            match s.script {
                Script::None => {}
                Script::Super => {
                    rise = size * 0.33;
                    size *= 0.7;
                }
                Script::Sub => {
                    rise = -size * 0.12;
                    size *= 0.7;
                }
            }
            let link = s.link.is_some();
            let mut look = Look {
                size,
                color: s.color.unwrap_or(if link { LINK } else { BLACK }),
                under: s.underline,
                strike: s.strike,
                back: s.background,
                rise,
                edge: s.edge.filter(|e| e.width > 0),
            };
            // A picture that can be printed stands in the line like one tall glyph.
            if let Some(picture) = &run.picture
                && let Some(natural) = (self.sizes)(picture)
            {
                let mut chain = self.fonts.chain(&self.base.family.clone(), false, false, false);
                if let Some((pick, glyph)) = self.fonts.resolve(&mut chain, ' ')
                    && out.len() < MAX_PARA_GLYPHS
                    && self.glyphs_left > 0
                {
                    self.glyphs_left -= 1;
                    let (wide, tall) = self.picture_size(run.extent, natural, room);
                    let plain = Look { under: false, strike: false, back: None, rise: 0.0, edge: None, ..look };
                    let at = looks.iter().position(|l| *l == plain).unwrap_or_else(|| {
                        looks.push(plain);
                        looks.len() - 1
                    });
                    stored.push('\u{fffc}');
                    out.push(Glyph {
                        at: stored.len() - 1,
                        level: 0,
                        span: 1,
                        dx: 0.0,
                        dy: 0.0,
                        ch: '\u{fffc}',
                        pick,
                        glyph,
                        advance: wide,
                        kern: 0.0,
                        look: at,
                        kind: Kind::Ink,
                        picture: Some(pictures.len()),
                        tall,
                        lead: Leader::None,
                    });
                    pictures.push(picture);
                    continue;
                }
            }
            // One that cannot is named in words, as the reading pane names one it cannot show.
            let named;
            let text: &str = match &run.picture {
                // An attachment that stands in the text is named there, in the text's own look.
                Some(Picture::File(_)) => &run.text,
                Some(picture) => {
                    let what = match picture {
                        Picture::Remote(_) => "picture not downloaded",
                        _ => "picture",
                    };
                    let alt: String = run.text.chars().filter(|c| !c.is_control()).take(80).collect();
                    named = if alt.trim().is_empty() { format!("[{what}]") } else { format!("[{what}: {}]", alt.trim()) };
                    look = Look { size: size.min(9.0), color: FAINT, under: false, strike: false, back: None, rise: 0.0, edge: None };
                    &named
                }
                None => &run.text,
            };
            let at = looks.iter().position(|l| *l == look).unwrap_or_else(|| {
                looks.push(look);
                looks.len() - 1
            });
            let family = s.family.clone().unwrap_or_else(|| self.base.family.clone());
            let mut chain = self.fonts.chain(&family, s.mono, s.bold, s.italic);
            // The shapes are of the run's own text, not of words that stand for a picture.
            let shaped = shapes.get(n).filter(|_| run.picture.is_none());
            let mut taken = false;
            for (k, ch) in text.chars().enumerate() {
                stored.push(ch);
                // An alef that the lam before it took into itself.
                if std::mem::take(&mut taken) {
                    continue;
                }
                let joined = shaped.and_then(|s| s.get(k)).copied().filter(|j| *j != ch && *j != script::TAKEN);
                let joined = joined.filter(|j| self.fonts.resolve(&mut chain, *j).is_some_and(|(_, glyph)| glyph != 0));
                taken = joined.is_some() && shaped.and_then(|s| s.get(k + 1)) == Some(&script::TAKEN);
                if out.len() >= MAX_PARA_GLYPHS || self.glyphs_left == 0 {
                    self.full = true;
                    return (out, looks, pictures);
                }
                let (shown, kind, times) = match ch {
                    '\n' => (' ', Kind::Break, 1.0),
                    '\t' => (' ', Kind::Space, 4.0),
                    ' ' => (' ', Kind::Space, 1.0),
                    // A space that does not end a line.
                    '\u{a0}' => (' ', Kind::Ink, 1.0),
                    // Marks that take no room and draw nothing.
                    '\u{ad}'
                    | '\u{200b}'..='\u{200f}'
                    | '\u{202a}'..='\u{202e}'
                    | '\u{2060}'..='\u{2064}'
                    | '\u{2066}'..='\u{2069}'
                    | '\u{fe00}'..='\u{fe0f}'
                    | '\u{feff}' => {
                        continue;
                    }
                    c if c.is_control() => continue,
                    c => (joined.unwrap_or(c), Kind::Ink, 1.0),
                };
                let Some((pick, glyph)) = self.fonts.resolve(&mut chain, shown) else {
                    self.no_font = true;
                    self.full = true;
                    return (out, looks, pictures);
                };
                self.glyphs_left -= 1;
                let advance = self.fonts.face(pick.face).map_or(0.0, |f| f.advance(glyph, look.size)) * times;
                // A pair the face kerns stands as close as the face says.
                if kind != Kind::Break
                    && ch != '\t'
                    && let Some(before) = out.last_mut()
                    && before.picture.is_none()
                    && before.kind != Kind::Break
                    && before.ch != '\t'
                    && before.pick == pick
                    && before.look == at
                {
                    let kern = self.fonts.kern(pick.face, before.glyph, glyph, look.size).max(-before.advance);
                    if kern.is_finite() {
                        before.kern = kern;
                        before.advance += kern;
                    }
                }
                out.push(Glyph {
                    at: stored.len() - 1,
                    level: 0,
                    // A lam drawn as one glyph with the alef after it stands for both.
                    span: if taken { 2 } else { 1 },
                    dx: 0.0,
                    dy: 0.0,
                    ch: if kind == Kind::Break { '\n' } else { ch },
                    pick,
                    glyph,
                    advance,
                    kern: 0.0,
                    look: at,
                    kind,
                    picture: None,
                    tall: 0.0,
                    lead: Leader::None,
                });
            }
        }
        (out, looks, pictures)
    }

    /// Height, depth and line room of text in the base font at `size`.
    fn base_line(&mut self, size: f32) -> (f32, f32, f32) {
        let mut chain = self.fonts.chain(&self.base.family.clone(), false, false, false);
        match self.fonts.resolve(&mut chain, ' ').and_then(|(pick, _)| self.fonts.face(pick.face)) {
            Some(face) => face.line(size),
            None => (size * 0.8, size * 0.2, 0.0),
        }
    }

    fn inset(para: &Para, width: f32) -> f32 {
        ((f32::from(para.indent.min(12)) * INDENT + f32::from(para.quote.min(8)) * QUOTE) * PX).min(width * 0.6)
    }

    /// Set a paragraph in the room from `x`, `width` wide, starting at `at`. Gives where it ends.
    fn para(&mut self, para: &Para, x: f32, width: f32, at: At, first: bool) -> At {
        let mut at = at;
        if self.full {
            return at;
        }
        if !first && at.y > self.top() + 0.01 {
            at.y += px(para.lead());
        }
        let inset = Self::inset(para, width);
        // What a paragraph is set in by, it is set in by on the side it starts at: the right
        // for one written from the right.
        let (left, room) = (if para.rtl { x } else { x + inset }, (width - inset).max(24.0));
        // A picture standing at one side ends where it ends, or where a paragraph asks to
        // start below it.
        if para.clear {
            at = self.below_aside(at);
        }
        self.asides.retain(|a| a.page == at.page && at.y < a.bottom - 0.01);
        // The pictures that ask to stand at a side do, one beside another, while room for text
        // is left between them and those that stand already.
        let (mut from, mut to) = self.between_asides(left, room, at);
        let mut stand: Vec<(usize, &Picture, Float, f32, f32)> = Vec::new();
        for (n, run) in para.runs.iter().enumerate().take(MAX_FLOAT_SEARCH) {
            if stand.len() >= MAX_OWN_ASIDES.min(MAX_ASIDES.saturating_sub(self.asides.len())) {
                break;
            }
            let Some(picture) = run.picture.as_ref().filter(|_| run.float != Float::None) else { continue };
            let Some(size) = (self.sizes)(picture) else { continue };
            let (wide, tall) = self.picture_size(run.extent, size, room);
            if to - from - wide - FLOAT_GAP >= MIN_BESIDE {
                if run.float == Float::Left {
                    from += wide + FLOAT_GAP;
                } else {
                    to -= wide + FLOAT_GAP;
                }
                stand.push((n, picture, run.float, wide, tall));
            }
        }
        if let Some(tallest) = stand.iter().map(|s| s.4).reduce(f32::max) {
            // On a page that has room for the tallest of them. On a new page none of those
            // that stood before stands, so there is no less room for these.
            at = self.room(at, tallest);
            if self.full {
                return at;
            }
            let (mut from, mut to) = self.between_asides(left, room, at);
            for (_, picture, float, wide, tall) in &stand {
                let at_left = *float == Float::Left;
                let x = if at_left { from } else { to - wide };
                if at_left {
                    from = x + wide + FLOAT_GAP;
                } else {
                    to = x - FLOAT_GAP;
                }
                let placed = Placed { x, y: at.y, width: *wide, height: *tall, picture: (*picture).clone() };
                if let Some(page) = self.page(at.page) {
                    page.pictures.push(placed);
                }
                self.asides.push(Aside {
                    left: at_left,
                    edge: if at_left { from } else { to },
                    page: at.page,
                    bottom: at.y + tall + FLOAT_GAP / 2.0,
                });
            }
        }
        let standing: Vec<usize> = stand.iter().map(|s| s.0).collect();
        let (mut glyphs, looks, pictures, stored) = self.glyphs(para, room, &standing);
        set_tabs(&mut glyphs, &para.tabs, matches!(para.kind, ParaKind::Pre), room);
        let own_size = looks.first().map_or(self.base.size, |l| l.size);
        let (mut from, mut n) = (0usize, 0usize);
        loop {
            // A line beside pictures that stand at the sides is shorter.
            let (left, room) = if self.asides.is_empty() {
                (left, room)
            } else {
                let (from, to) = self.between_asides(left, room, at);
                (from, (to - from).max(24.0))
            };
            let Some((to, next)) = line_end(&glyphs, from, room) else { break };
            let line = glyphs.get(from..to).unwrap_or_default();
            // Spaces at the end of a line take no room.
            let seen = line.iter().rposition(|g| g.kind != Kind::Space).map_or(0, |last| last + 1);
            let line = line.get(..seen).unwrap_or_default();
            // A line with right-to-left text in it is drawn in another order than it is read
            // (UAX #9, L2).
            let drawn: Vec<Glyph>;
            let turned = para.rtl || line.iter().any(|g| g.level > 0);
            let line = if turned {
                let chars: Vec<char> = line.iter().map(|g| g.ch).collect();
                let levels: Vec<u8> = line.iter().map(|g| g.level).collect();
                drawn = script::order(&chars, &levels, u8::from(para.rtl)).into_iter().filter_map(|i| line.get(i).copied()).collect();
                drawn.as_slice()
            } else {
                line
            };
            let (mut ascent, mut descent, mut gap) = (0.0f32, 0.0f32, 0.0f32);
            for g in line {
                if g.picture.is_some() {
                    ascent = ascent.max(g.tall);
                    continue;
                }
                let size = looks.get(g.look).map_or(own_size, |l| l.size);
                if let Some((a, d, l)) = self.fonts.face(g.pick.face).map(|f| f.line(size)) {
                    (ascent, descent, gap) = (ascent.max(a), descent.max(d), gap.max(l));
                }
            }
            if line.is_empty() {
                (ascent, descent, gap) = self.base_line(own_size);
            }
            // A line told how tall to be has what it gains or loses below its text, as it has
            // on screen; a picture is never cut into.
            let own = ascent + descent + gap;
            let tall = match line.iter().filter(|g| g.picture.is_some()).map(|g| g.tall).reduce(f32::max) {
                Some(picture) => para.leading.tall(own, PX).max(picture),
                // No line is taller than a page has room for.
                None => para.leading.tall(own, PX).min((self.bottom() - self.top()).max(own)),
            };
            at = self.room(at, tall);
            if self.full {
                break;
            }
            let baseline = at.y + gap / 2.0 + ascent;
            // The last glyph of a line has nothing after it to stand closer to.
            let wide: f32 = line.iter().map(|g| g.advance).sum::<f32>() - line.last().map_or(0.0, |g| g.kern);
            let mut pen = left
                + match para.align {
                    Align::Left => 0.0,
                    // Never past the room there is: a narrow table cell has less than the
                    // least a text wraps at.
                    Align::Center => ((room.min(width - inset) - wide) / 2.0).max(0.0),
                    Align::Right => (room.min(width - inset) - wide).max(0.0),
                };
            // Glyphs that follow one another in one face, one look and one direction make one
            // piece. The pieces of the line, each with the first of the paragraph's characters
            // it stands for.
            let mut set: Vec<(usize, Text)> = Vec::new();
            let mut i = 0usize;
            while let Some(first) = line.get(i) {
                if let Some(picture) = first.picture.and_then(|n| pictures.get(n)) {
                    let placed = Placed { x: pen, y: baseline - first.tall, width: first.advance, height: first.tall, picture: (*picture).clone() };
                    if let Some(page) = self.page(at.page) {
                        page.pictures.push(placed);
                    }
                    pen += first.advance;
                    i += 1;
                    continue;
                }
                let same = |g: &Glyph| g.pick == first.pick && g.look == first.look && g.level == first.level && g.picture.is_none();
                let piece = line.get(i..).unwrap_or_default();
                let count = piece.iter().take_while(|g| same(g)).count().max(1);
                let piece = piece.get(..count).unwrap_or_default();
                let piece_wide: f32 = piece.iter().map(|g| g.advance).sum();
                if let Some(look) = looks.get(first.look).copied() {
                    let page = at.page;
                    if let Some(color) = look.back
                        && let Some(page) = self.page(page)
                    {
                        page.fills.push(Fill { x: pen, y: at.y, width: piece_wide, height: tall, color, depth: u16::MAX, radius: 0.0, ring: 0.0 });
                    }
                    if let Some(edge) = look.edge {
                        // A border round text within a line (CSS 2.1 section 8.5): over and
                        // under each piece, and at a side only where the border ends there. It
                        // is open where its text goes on in another line.
                        let of = |g: Option<&Glyph>| g.and_then(|g| looks.get(g.look)).and_then(|l| l.edge);
                        let before = match i.checked_sub(1) {
                            Some(k) => of(line.get(k)),
                            None if !turned => of(from.checked_sub(1).and_then(|k| glyphs.get(k))),
                            None => None,
                        };
                        let after = match line.get(i.saturating_add(count)) {
                            Some(g) => of(Some(g)),
                            None if !turned => of(glyphs.get(next)),
                            None => None,
                        };
                        let thick = px(u16::from(edge.width.min(MAX_EDGE))).min(tall / 2.0);
                        let half = thick / 2.0;
                        let drawn = Edge { color: Some(edge.color.unwrap_or(look.color)), ..edge };
                        let (x0, x1, y0, y1) = (pen, pen + piece_wide, at.y, at.y + tall);
                        self.edge(page, (x0, y0 + half), (x1, y0 + half), thick, &drawn, true);
                        self.edge(page, (x0, y1 - half), (x1, y1 - half), thick, &drawn, true);
                        if before != Some(edge) {
                            self.edge(page, (x0 + half, y0), (x0 + half, y1), thick, &drawn, false);
                        }
                        if after != Some(edge) {
                            self.edge(page, (x1 - half, y0), (x1 - half, y1), thick, &drawn, false);
                        }
                    }
                    let thick = (look.size / 16.0).max(0.4);
                    if look.under {
                        let y = baseline + look.size * 0.12;
                        self.rule(page, (pen, y), (pen + piece_wide, y), thick, look.color, Line::Solid);
                    }
                    if look.strike {
                        let y = baseline - look.rise - look.size * 0.28;
                        self.rule(page, (pen, y), (pen + piece_wide, y), thick, look.color, Line::Solid);
                    }
                    // A tab is drawn as room, not as glyphs.
                    let mut x = pen;
                    let mut run: Vec<(u16, char, usize, u16)> = Vec::new();
                    let mut kerns: Vec<f32> = Vec::new();
                    let mut run_x = pen;
                    let mut leads: Vec<(f32, f32, Leader)> = Vec::new();
                    let mut close = |run: &mut Vec<(u16, char, usize, u16)>, kerns: &mut Vec<f32>, from: f32, to: f32, lift: f32| {
                        let moved = kerns.iter().any(|k| *k != 0.0);
                        let kerns = std::mem::take(kerns);
                        let run = std::mem::take(run);
                        let Some(starts) = run.iter().map(|(_, _, at, _)| *at).min() else { return };
                        let glyphs: Vec<(u16, char)> = run.iter().map(|(glyph, ch, ..)| (*glyph, shown(*ch, first.level % 2 == 1))).collect();
                        set.push((
                            starts,
                            Text {
                                x: from,
                                y: baseline - look.rise,
                                face: first.pick.face,
                                size: look.size,
                                color: look.color,
                                actual: said(&run, &stored),
                                rtl: first.level % 2 == 1,
                                glyphs,
                                width: to - from,
                                kerns: if moved { kerns } else { Vec::new() },
                                bold: first.pick.bold,
                                slant: first.pick.slant,
                                lift,
                            },
                        ));
                    };
                    for g in piece {
                        if g.ch == '\t' {
                            close(&mut run, &mut kerns, run_x, x, 0.0);
                            leads.push((x, g.advance, g.lead));
                            run_x = x + g.advance;
                        } else if g.dx != 0.0 || g.dy != 0.0 {
                            // A glyph the font puts somewhere else than where the line has got
                            // to is a piece of its own.
                            close(&mut run, &mut kerns, run_x, x, 0.0);
                            run.push((g.glyph, g.ch, g.at, g.span));
                            kerns.push(0.0);
                            // Never outside the room the line has.
                            let from = (x + g.dx).min(left + room - g.advance).max(left);
                            close(&mut run, &mut kerns, from, from + g.advance, g.dy);
                            run_x = x + g.advance;
                        } else {
                            run.push((g.glyph, if g.ch == '\u{a0}' { ' ' } else { g.ch }, g.at, g.span));
                            kerns.push(g.kern);
                        }
                        x += g.advance;
                    }
                    close(&mut run, &mut kerns, run_x, x, 0.0);
                    for (from, wide, lead) in leads {
                        self.leader(page, from, wide, baseline, look, lead);
                    }
                }
                pen += piece_wide;
                i += count;
            }
            // In the file the pieces of a line follow one another as they are read, wherever
            // each is drawn, so that text copied out of it is in that order.
            if turned {
                set.sort_by_key(|(starts, _)| *starts);
            }
            if let Some(page) = self.page(at.page) {
                page.texts.extend(set.into_iter().map(|(_, text)| text));
            }
            for level in 0..para.quote.min(8) {
                let bar = (f32::from(level) * QUOTE + 4.0) * PX;
                let bar = if para.rtl { x + width - bar } else { x + bar };
                self.rule(at.page, (bar, at.y), (bar, at.y + tall), 1.5, [170, 170, 170], Line::Solid);
            }
            if n == 0
                && let ParaKind::Item(marker) = &para.kind
            {
                let edge = if para.rtl { left + room.min(width - inset) + 5.0 } else { left - 5.0 };
                self.marker(marker, edge, para.rtl, baseline, own_size, at.page);
            }
            at.y += tall;
            if next <= from {
                break;
            }
            (from, n) = (next, n + 1);
        }
        at
    }

    /// What fills the room a tab takes: a line of marks from `from`, `wide` long, standing
    /// clear of the text at both ends.
    fn leader(&mut self, page: usize, from: f32, wide: f32, baseline: f32, look: Look, lead: Leader) {
        let clear = look.size * 0.15;
        let (a, b) = (from + clear, from + wide - clear);
        if !(a.is_finite() && b.is_finite()) || b - a < look.size * 0.5 {
            return;
        }
        let thin = (look.size / 14.0).max(0.4);
        let mut draw = |y: f32, width: f32, line: Line| self.rule(page, (a, y), (b, y), width, look.color, line);
        match lead {
            Leader::None => {}
            Leader::Dots => draw(baseline - thin, thin * 1.4, Line::Dotted),
            Leader::MiddleDots => draw(baseline - look.size * 0.3, thin * 1.4, Line::Dotted),
            Leader::Hyphens => draw(baseline - look.size * 0.27, thin, Line::Dashed),
            Leader::Line => draw(baseline + thin, thin, Line::Solid),
            Leader::ThickLine => draw(baseline + thin, thin * 2.0, Line::Solid),
            Leader::Equals => {
                draw(baseline - look.size * 0.22, thin, Line::Solid);
                draw(baseline - look.size * 0.36, thin, Line::Solid);
            }
        }
    }

    /// A list item's marker, ending at `right`.
    /// The marker of a list's item, ending at `edge`; for an item written from the right it
    /// stands at the right of the item and starts there.
    fn marker(&mut self, marker: &str, edge: f32, rtl: bool, baseline: f32, size: f32, page: usize) {
        let marker: String = marker.chars().take(12).collect();
        let family = self.base.family.clone();
        // A marker longer than the room before its item (the room from the edge of the page
        // inside the margin) is cut as a line of the head of a page is: it never hangs off
        // the sheet.
        let sheet = self.paper.width;
        if rtl {
            // Read from the right, as its item is: the stop after a number stands at its left.
            let marker = format!("\u{200f}{marker}");
            let room = (self.left() + self.width() - edge).max(size);
            self.words(page, &marker, &family, size, Some(room), baseline, |wide| edge.min(sheet - wide).max(0.0));
        } else {
            let room = (edge - self.left()).max(size);
            self.words(page, &marker, &family, size, Some(room), baseline, |wide| (edge - wide).max(0.0));
        }
    }

    /// One line the print sets itself (what stands at the head and the foot of a page, the
    /// marker of a list's item), set as a paragraph's text is: shaped by its font's own
    /// tables, and drawn in the order it is seen when it has right-to-left writing in it, its
    /// direction that of its first letter. With `room`, a line too long for it loses the end
    /// of what it says, as that is read, and a mark says there was more. `place` is told how
    /// wide the line came out and says where it starts.
    pub(crate) fn words(
        &mut self,
        page: usize,
        words: &str,
        family: &str,
        size: f32,
        room: Option<f32>,
        baseline: f32,
        place: impl FnOnce(f32) -> f32,
    ) {
        // A character no font has is left out, and the mark that says there was more with it.
        let mut chain = self.fonts.chain(family, false, false, false);
        let mut has = |flow: &mut Self, c: char| matches!(c, '\u{200b}'..='\u{200f}') || flow.fonts.resolve(&mut chain, c).is_some();
        let chars: Vec<char> = words.chars().filter(|c| !c.is_control()).take(MAX_WORDS_CHARS).filter(|c| has(self, *c)).collect();
        let more = has(self, '\u{2026}').then_some('\u{2026}');
        // What the body left of the page's allowance is not this line's to use up or to want,
        // and a body that filled its pages still has its pages' heads and feet set.
        let (left, no_font, full) = (self.glyphs_left, self.no_font, self.full);
        (self.glyphs_left, self.full) = (MAX_WORDS_CHARS * 16, false);
        let mut set = self.words_cut(&chars, chars.len(), more, family, size);
        if let Some(room) = room.filter(|room| set.1 > *room + 0.01) {
            let (mut fits, mut too_long) = (0usize, chars.len());
            while too_long.saturating_sub(fits) > 1 {
                let middle = fits + (too_long - fits) / 2;
                if self.words_cut(&chars, middle, more, family, size).1 <= room + 0.01 {
                    fits = middle;
                } else {
                    too_long = middle;
                }
            }
            set = self.words_cut(&chars, fits, more, family, size);
        }
        (self.glyphs_left, self.no_font, self.full) = (left, no_font, full);
        let (glyphs, wide, stored) = set;
        let turned = glyphs.iter().any(|g| g.level > 0);
        let mut pen = place(wide);
        // The pieces of the line, each with the first of its characters it stands for.
        let mut pieces: Vec<(usize, Text)> = Vec::new();
        let mut i = 0usize;
        while let Some(first) = glyphs.get(i) {
            let rest = glyphs.get(i..).unwrap_or_default();
            let count = rest.iter().take_while(|g| g.pick == first.pick && g.level == first.level).count().max(1);
            i += count;
            let mut run: Vec<(u16, char, usize, u16)> = Vec::new();
            let mut kerns: Vec<f32> = Vec::new();
            let mut from = pen;
            let mut close = |run: &mut Vec<(u16, char, usize, u16)>, kerns: &mut Vec<f32>, from: f32, to: f32, lift: f32| {
                let moved = kerns.iter().any(|k| *k != 0.0);
                let kerns = std::mem::take(kerns);
                let run = std::mem::take(run);
                let Some(starts) = run.iter().map(|(_, _, at, _)| *at).min() else { return };
                pieces.push((
                    starts,
                    Text {
                        x: from,
                        y: baseline,
                        face: first.pick.face,
                        size,
                        color: BLACK,
                        actual: said(&run, &stored),
                        rtl: first.level % 2 == 1,
                        glyphs: run.iter().map(|(glyph, ch, ..)| (*glyph, shown(*ch, first.level % 2 == 1))).collect(),
                        width: to - from,
                        kerns: if moved { kerns } else { Vec::new() },
                        bold: first.pick.bold,
                        slant: first.pick.slant,
                        lift,
                    },
                ));
            };
            for g in rest.get(..count).unwrap_or_default() {
                if g.dx != 0.0 || g.dy != 0.0 {
                    // A glyph the font puts somewhere else than where the line has got to is
                    // a piece of its own.
                    close(&mut run, &mut kerns, from, pen, 0.0);
                    run.push((g.glyph, g.ch, g.at, g.span));
                    kerns.push(0.0);
                    close(&mut run, &mut kerns, pen + g.dx, pen + g.dx + g.advance, g.dy);
                    from = pen + g.advance;
                } else {
                    run.push((g.glyph, if g.ch == '\u{a0}' { ' ' } else { g.ch }, g.at, g.span));
                    kerns.push(g.kern);
                }
                pen += g.advance;
            }
            close(&mut run, &mut kerns, from, pen, 0.0);
        }
        // In the file the pieces follow one another as they are read, wherever each is drawn.
        if turned {
            pieces.sort_by_key(|(starts, _)| *starts);
        }
        if let Some(page) = self.page(page) {
            page.texts.extend(pieces.into_iter().map(|(_, text)| text));
        }
    }

    /// The first `keep` of a line's characters, with a mark after them when that is not all:
    /// their glyphs in the order they are drawn, how wide they are together, and the
    /// characters they stand for.
    fn words_cut(&mut self, chars: &[char], keep: usize, more: Option<char>, family: &str, size: f32) -> (Vec<Glyph>, f32, Vec<char>) {
        let mut text: String = chars.iter().take(keep).collect();
        if keep < chars.len() {
            text = text.trim_end().to_string();
            text.extend(more);
        }
        let rtl = script::first_strong(&text).unwrap_or(false);
        let style = Style { family: Some(family.into()), size: Some(size), ..Style::default() };
        let para = Para { runs: vec![Run { text, style, ..Run::default() }], rtl, ..Para::default() };
        let (glyphs, _, _, stored) = self.glyphs(&para, self.width(), &[]);
        let glyphs: Vec<Glyph> = glyphs.into_iter().filter(|g| g.picture.is_none() && g.kind != Kind::Break && g.ch != '\t').collect();
        let glyphs = if rtl || glyphs.iter().any(|g| g.level > 0) {
            let chars: Vec<char> = glyphs.iter().map(|g| g.ch).collect();
            let levels: Vec<u8> = glyphs.iter().map(|g| g.level).collect();
            script::order(&chars, &levels, u8::from(rtl)).into_iter().filter_map(|i| glyphs.get(i).copied()).collect()
        } else {
            glyphs
        };
        // The last glyph of a line has nothing after it to stand closer to.
        let wide = glyphs.iter().map(|g| g.advance).sum::<f32>() - glyphs.last().map_or(0.0, |g| g.kern);
        (glyphs, wide, stored)
    }

    /// The narrowest and the widest a paragraph can usefully be: its longest word, and its
    /// longest line when nothing is wrapped.
    fn para_wants(&mut self, para: &Para) -> (f32, f32) {
        let (glyphs, ..) = self.glyphs(para, f32::MAX, &[]);
        let (mut word, mut line, mut least, mut most) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for g in &glyphs {
            match g.kind {
                // A picture is made smaller where there is less room than it wants.
                Kind::Ink if g.picture.is_some() => {
                    word += g.advance.min(MIN_COLUMN);
                    line += g.advance;
                }
                Kind::Ink => {
                    word += g.advance;
                    line += g.advance;
                }
                Kind::Space => {
                    least = least.max(word);
                    word = 0.0;
                    line += g.advance;
                }
                Kind::Break => {
                    (least, most) = (least.max(word), most.max(line));
                    (word, line) = (0.0, 0.0);
                }
            }
        }
        let inset = Self::inset(para, f32::MAX)
            + para.frame.as_ref().map_or(0.0, |f| {
                let (near, far) = frame_room(f);
                near.0 + far.0 + f32::from(f.indent.min(12)) * INDENT * PX
            });
        (least.max(word) + inset, most.max(line) + inset)
    }

    /// The narrowest and the widest some blocks can usefully be.
    fn wants(&mut self, list: &[Block], depth: usize) -> (f32, f32) {
        let (mut least, mut most) = (0.0f32, 0.0f32);
        if depth > MAX_DEPTH {
            return (least, most);
        }
        for block in list {
            if self.full {
                break;
            }
            let (a, b) = match block {
                Block::Para(para) => self.para_wants(para),
                Block::Rule => (0.0, 0.0),
                Block::Table(table) => {
                    let columns = table.columns();
                    if columns == 0 || columns > MAX_COLUMNS || depth >= MAX_TABLE_DEPTH {
                        let mut both = (0.0f32, 0.0f32);
                        for cell in table.rows.iter().take(MEASURED_ROWS).flatten() {
                            let (a, b) = self.wants(&cell.blocks, depth + 1);
                            both = (both.0.max(a), both.1.max(b));
                        }
                        both
                    } else {
                        let (mins, maxs, _) = self.columns(table, &table.places(), columns, depth);
                        let (rim, gap) = Self::rim_and_gap(table);
                        let around = rim * 2.0 + gap * (columns as f32 + 1.0);
                        let least: f32 = mins.iter().sum::<f32>() + around;
                        let most = match table.width {
                            TableWidth::Pixels(w) => px(w).max(least),
                            _ => maxs.iter().sum::<f32>() + around,
                        };
                        (least, most)
                    }
                }
            };
            (least, most) = (least.max(a), most.max(b));
        }
        (least, most)
    }

    /// Room a table's own border takes on each side, and the room between its cells, in points.
    fn rim_and_gap(table: &Table) -> (f32, f32) {
        let rim = table.frame.iter().map(|e| px(u16::from(e.width.min(MAX_EDGE)))).fold(0.0, f32::max);
        ((rim - PX).max(0.0), px(table.spacing.min(MAX_CELL_ROOM)))
    }

    /// For each column: the least it can be, what it wants, and the width it asks for (0 for none).
    fn columns(&mut self, table: &Table, places: &[Vec<usize>], columns: usize, depth: usize) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let mut mins = vec![MIN_COLUMN; columns];
        let mut maxs = vec![MIN_COLUMN; columns];
        let mut asked = vec![0.0f32; columns];
        let mut wide: Vec<(usize, usize, f32, f32)> = Vec::new();
        for (row, starts) in table.rows.iter().zip(places).take(MEASURED_ROWS) {
            for (cell, col) in row.iter().zip(starts) {
                let (least, most) = self.wants(&cell.blocks, depth + 1);
                let pad = padding(table, cell);
                let pad = pad[1] + pad[3];
                let (least, most, ask) = (least + pad, most + pad, px(cell.width));
                if cell.span <= 1 {
                    if let (Some(a), Some(b), Some(c)) = (mins.get_mut(*col), maxs.get_mut(*col), asked.get_mut(*col)) {
                        (*a, *b, *c) = (a.max(least), b.max(most), c.max(ask));
                    }
                } else {
                    wide.push((*col, usize::from(cell.span), least, most.max(ask)));
                }
            }
        }
        for ((least, most), ask) in mins.iter().zip(maxs.iter_mut()).zip(&asked) {
            if *ask > 0.0 {
                *most = ask.max(*least);
            }
        }
        // A cell over several columns that wants more than they have widens the last of them.
        for (col, span, least, most) in wide {
            let last = col.saturating_add(span).saturating_sub(1).min(columns.saturating_sub(1));
            for (list, want) in [(&mut mins, least), (&mut maxs, most)] {
                let have: f32 = list.iter().skip(col).take(span).sum();
                if want > have
                    && let Some(slot) = list.get_mut(last)
                {
                    *slot += want - have;
                }
            }
        }
        for (least, most) in mins.iter().zip(maxs.iter_mut()) {
            *most = most.max(*least);
        }
        (mins, maxs, asked)
    }

    /// A colour behind all that was set from `from` to `to`, under every other fill.
    pub fn behind(&mut self, from: At, to: At, x: f32, width: f32, fill: Rgb) {
        self.boxed(from, to, x, width, &Edges::default(), Some(fill), 0, false);
    }

    /// The lines and the fill of a box that starts at `from` and ends at `to`, on every page it
    /// is on. Lines stand on the box's edge when `on_edge`, else just inside it.
    fn boxed(&mut self, from: At, to: At, x: f32, width: f32, edges: &Edges, fill: Option<Rgb>, depth: u16, on_edge: bool) {
        let (top, bottom) = (self.top(), self.bottom());
        // Rounded corners are for a box that is whole: on one page, with no border or with one
        // unbroken line all the way round. Any other keeps square corners.
        let asked = std::mem::take(&mut self.round);
        let line = rich_one_edge(edges).filter(|e| e.line == Line::Solid);
        let whole = from.page == to.page && asked > 0 && (line.is_some() || !any_edge(edges));
        let radius = if whole { px(asked.min(MAX_RADIUS)).min(width / 2.0).min((to.y - from.y) / 2.0).max(0.0) } else { 0.0 };
        let radius = if radius.is_finite() { radius } else { 0.0 };
        for page in from.page..=to.page.min(from.page.saturating_add(MAX_PAGES)) {
            let (y0, y1) = (if page == from.page { from.y } else { top }, if page == to.page { to.y } else { bottom });
            if y1 <= y0 {
                continue;
            }
            if let Some(color) = fill
                && let Some(page) = self.page(page)
            {
                page.fills.push(Fill { x, y: y0, width, height: y1 - y0, color, depth, radius, ring: 0.0 });
            }
            if let Some(edge) = line.filter(|_| radius > 0.0) {
                // The border is a band that follows the corners, over the fill it is round.
                let ring = px(u16::from(edge.width.min(MAX_EDGE))).min(width / 2.0).min((y1 - y0) / 2.0);
                if let Some(page) = self.page(page) {
                    page.fills.push(Fill { x, y: y0, width, height: y1 - y0, color: edge.color.unwrap_or(BLACK), depth, radius, ring });
                }
                continue;
            }
            for (side, edge) in edges.iter().enumerate() {
                if edge.width == 0 || (side == 0 && page != from.page) || (side == 2 && page != to.page) {
                    continue;
                }
                let thick = px(u16::from(edge.width.min(MAX_EDGE)));
                let inside = if on_edge { 0.0 } else { thick / 2.0 };
                let (a, b) = match side {
                    0 => ((x, y0 + inside), (x + width, y0 + inside)),
                    1 => ((x + width - inside, y0), (x + width - inside, y1)),
                    2 => ((x, y1 - inside), (x + width, y1 - inside)),
                    _ => ((x + inside, y0), (x + inside, y1)),
                };
                self.edge(page, a, b, thick, edge, side % 2 == 0);
            }
        }
    }

    fn edge(&mut self, page: usize, a: (f32, f32), b: (f32, f32), thick: f32, edge: &Edge, level: bool) {
        let color = edge.color.unwrap_or(BLACK);
        if edge.line == Line::Double && thick >= 2.0 {
            // Two lines a third as thick, with as much room between them.
            let off = thick / 3.0;
            for sign in [-1.0, 1.0] {
                let (dx, dy) = if level { (0.0, off * sign) } else { (off * sign, 0.0) };
                self.rule(page, (a.0 + dx, a.1 + dy), (b.0 + dx, b.1 + dy), off, color, Line::Solid);
            }
        } else {
            self.rule(page, a, b, thick, color, if edge.line == Line::Double { Line::Solid } else { edge.line });
        }
    }

    /// Which of the fills on a page one lies among: over those of the tables and boxes it
    /// stands inside, `over` more than a table that deep has for its own cells.
    fn layer(&self, depth: usize, over: u16) -> u16 {
        u16::try_from(depth).unwrap_or(u16::MAX - 1).saturating_add(over).saturating_add(self.lift).min(u16::MAX - 1)
    }

    /// Blocks inside one border: paragraphs, tables, and the boxes that stand inside this one.
    /// `level` is how many boxes this one stands inside.
    #[allow(clippy::too_many_arguments)]
    fn framed(&mut self, paras: &[Block], frame: &Frame, x: f32, width: f32, at: At, first: bool, depth: usize, level: usize) -> At {
        let mut at = at;
        match paras.first() {
            Some(Block::Para(p)) if !first && at.y > self.top() + 0.01 => at.y += px(p.lead()),
            Some(Block::Table(t)) if !first && !t.snug && at.y > self.top() + 0.01 => at.y += 6.0 * PX,
            _ => {}
        }
        let (near, far) = frame_room(frame);
        let inset = (f32::from(frame.indent.min(12)) * INDENT * PX).min(width * 0.6).min((width - 24.0).max(0.0));
        // Set in on the side its paragraphs start at.
        let rtl = matches!(paras.first(), Some(Block::Para(p)) if p.rtl);
        let (x, width) = (if rtl { x } else { x + inset }, width - inset);
        let start = self.room(at, near.1 + self.base.size * 1.3);
        let mut inner = At { page: start.page, y: start.y + near.1 };
        // What it holds never stands outside it, however much room boxes inside one another
        // ask for.
        let room = (width - near.0 - far.0).max(width.min(24.0)).max(0.0);
        let left = (x + near.0).min(x + width - room).max(x);
        // The line between one paragraph and the next, with the room the border keeps.
        let between = f32::from(frame.between.width.min(MAX_EDGE)) * PX;
        // Over the fill of a cell it stands in, under any highlight of its text; what it
        // holds lies over its own fill.
        let over = self.layer(depth, 2);
        let lift = self.lift;
        self.lift = lift.saturating_add(2);
        let mut k = 0usize;
        while let Some(block) = paras.get(k) {
            if self.full {
                break;
            }
            if k > 0 && between > 0.0 {
                let [above, _, below, _] = frame.padding.map(|v| f32::from(v.min(MAX_CELL_ROOM)) * PX);
                inner = self.below_aside(inner);
                inner = self.room(At { page: inner.page, y: inner.y + below }, between + above);
                let y = inner.y + between / 2.0;
                self.edge(inner.page, (x, y), (x + width, y), between, &frame.between, true);
                inner.y += between + above;
            }
            let start_of = k == 0 || between > 0.0;
            let rest = paras.get(k..).unwrap_or_default();
            k += match (boxed_run(rest, level + 1), block) {
                // A box inside this one, with all that stands in it.
                (Some((within, count)), _) => {
                    inner = self.below_aside(inner);
                    inner = self.framed(rest.get(..count).unwrap_or_default(), within, left, room, inner, start_of, depth, level + 1);
                    count.max(1)
                }
                (None, Block::Para(p)) => {
                    inner = self.para(p, left, room, inner, start_of);
                    1
                }
                (None, Block::Table(table)) => {
                    inner = self.below_aside(inner);
                    if !start_of && !table.snug && inner.y > self.top() + 0.01 {
                        inner.y += 6.0 * PX;
                    }
                    inner = self.table(table, left, room, inner, depth);
                    1
                }
                (None, Block::Rule) => 1,
            };
        }
        self.lift = lift;
        // A picture at one side of the last paragraph is inside the border too.
        inner = self.below_aside(inner);
        let end = At { page: inner.page, y: inner.y + far.1 };
        self.round = frame.radius;
        self.boxed(start, end, x, width, &frame.edges, frame.fill, over, false);
        end
    }

    /// Set blocks one under another. Gives where they end.
    pub fn blocks(&mut self, list: &[Block], x: f32, width: f32, at: At, depth: usize) -> At {
        let mut at = at;
        if depth > MAX_DEPTH {
            return at;
        }
        // A picture at one side belongs to the blocks it is among: these start with none.
        let outer = std::mem::take(&mut self.asides);
        let mut i = 0usize;
        while let Some(block) = list.get(i) {
            if self.full {
                break;
            }
            let step = 1usize;
            // A plain paragraph's lines go round a picture at one side. A table, a rule or a
            // bordered block stands beside it whole, in the room it leaves, as in the window,
            // when that room is wide enough; else it starts below the picture.
            let (mut bx, mut bw) = (x, width);
            if !matches!(block, Block::Para(p) if p.frame.is_none()) {
                let (from, to) = self.between_asides(x, width, at);
                let narrower = from > x + 0.01 || to < x + width - 0.01;
                if narrower && to - from >= MIN_BESIDE.max(width * 0.3) {
                    (bx, bw) = (from, to - from);
                } else {
                    at = self.below_aside(at);
                }
            }
            // What stands beside the pictures leaves them standing for what comes after it.
            let beside = (bx, bw) != (x, width);
            let standing = if beside { std::mem::take(&mut self.asides) } else { Vec::new() };
            if let Some((frame, count)) = boxed_run(list.get(i..).unwrap_or_default(), 0) {
                at = self.framed(list.get(i..i.saturating_add(count)).unwrap_or_default(), frame, bx, bw, at, i == 0, depth, 0);
                i += count.max(1);
                if beside {
                    self.asides = standing;
                }
                continue;
            }
            match block {
                Block::Para(para) => at = self.para(para, bx, bw, at, i == 0),
                Block::Rule => {
                    let tall = 13.0 * PX;
                    at = self.room(at, tall);
                    let y = at.y + tall / 2.0;
                    self.rule(at.page, (bx, y), (bx + bw, y), PX, GREY, Line::Solid);
                    at.y += tall;
                }
                Block::Table(table) => {
                    if i > 0 && !table.snug && at.y > self.top() + 0.01 {
                        at.y += 6.0 * PX;
                    }
                    at = self.table(table, bx, bw, at, depth);
                }
            }
            if beside {
                self.asides = standing;
            }
            i += step;
        }
        at = self.below_aside(at);
        self.asides = outer;
        at
    }

    /// Set a table's first row once more, at `at`, in the columns the table has. Gives where
    /// the row after it starts.
    fn first_row_again(&mut self, table: &Table, places: &[Vec<usize>], columns: (&[f32], &[f32], f32), at: At, depth: usize) -> At {
        let (lefts, widths, gap) = columns;
        let (Some(row), Some(starts)) = (table.rows.first(), places.first()) else { return at };
        let lined = Edge { width: 1, color: Some(table.line.unwrap_or(GREY)), line: Line::Solid };
        let fill_depth = self.layer(depth, 1);
        let mut set: Vec<(&Cell, f32, f32)> = Vec::new();
        let mut end = at;
        for (cell, col) in row.iter().zip(starts) {
            let span = usize::from(cell.span.max(1)).min(lefts.len().saturating_sub(*col));
            let Some(left) = lefts.get(table.left_column(*col, span, lefts.len())).copied() else { continue };
            let wide: f32 = widths.iter().skip(*col).take(span).sum::<f32>() + gap * (span.saturating_sub(1) as f32);
            let pad = padding(table, cell);
            let below =
                self.blocks(&cell.blocks, left + pad[3], (wide - pad[1] - pad[3]).max(6.0), At { page: at.page, y: at.y + pad[0] }, depth + 1);
            end = later(end, At { page: below.page, y: below.y + pad[2] });
            set.push((cell, left, wide));
        }
        for (cell, left, wide) in set {
            let edges = cell.edges.unwrap_or(if table.border { [lined; 4] } else { [Edge::default(); 4] });
            self.round = cell.radius;
            self.boxed(at, end, left, wide, &edges, cell.background, fill_depth, true);
        }
        At { page: end.page, y: end.y + gap }
    }

    fn table(&mut self, table: &Table, x: f32, width: f32, at: At, depth: usize) -> At {
        let columns = table.columns();
        if columns == 0 || columns > MAX_COLUMNS || depth >= MAX_TABLE_DEPTH || width / (columns as f32) < MIN_COLUMN {
            // Too many columns or too little room: one cell under another, in reading order.
            let mut at = at;
            for cell in table.rows.iter().flatten() {
                at = self.blocks(&cell.blocks, x, width, at, depth + 1);
            }
            return at;
        }
        let places = table.places();
        let (mins, maxs, asked) = self.columns(table, &places, columns, depth);
        let (least, most): (f32, f32) = (mins.iter().sum(), maxs.iter().sum());
        let (rim, gap) = Self::rim_and_gap(table);
        let gaps = columns as f32 + 1.0;
        let target = match table.width {
            TableWidth::Pixels(w) => px(w).min(width),
            TableWidth::Percent(share) => width * f32::from(share.clamp(1, 100)) / 100.0,
            TableWidth::Fill => width,
            TableWidth::Content => (most + gap * gaps + rim * 2.0).min(width),
        };
        let rim = rim.min(target / 4.0);
        // Spacing never takes more than half the table.
        let gap = gap.min((target - rim * 2.0) / gaps / 2.0).max(0.0);
        let cells = (target - rim * 2.0 - gap * gaps).max(1.0);
        let mut widths: Vec<f32> = if most <= cells {
            // Room to spare goes to the columns that ask for no width of their own.
            let spare = cells - most;
            let free: f32 = maxs.iter().zip(&asked).filter(|(_, a)| **a <= 0.0).map(|(m, _)| *m).sum();
            maxs.iter()
                .zip(&asked)
                .map(|(m, a)| match (free > 0.0, *a <= 0.0) {
                    (true, true) => m + spare * m / free,
                    (true, false) => *m,
                    (false, _) => m * cells / most.max(0.01),
                })
                .collect()
        } else if least >= cells {
            mins.iter().map(|m| m * cells / least.max(0.01)).collect()
        } else {
            let part = (cells - least) / (most - least).max(0.01);
            mins.iter().zip(&maxs).map(|(a, b)| a + (b - a) * part).collect()
        };
        if table.width != TableWidth::Content {
            shares(table, &places, &mut widths, cells);
        }
        let x = x + match table.align {
            Align::Left => 0.0,
            Align::Center => ((width - target) / 2.0).max(0.0),
            Align::Right => (width - target).max(0.0),
        };
        let lefts = table.lefts(&widths, x + rim + gap, x + target - rim - gap, gap);

        let lined = Edge { width: 1, color: Some(table.line.unwrap_or(GREY)), line: Line::Solid };
        let fill_depth = self.layer(depth, 1);
        let line_tall = self.base.size * 1.3;
        let start = self.room(at, rim + gap + line_tall);
        let mut at = At { page: start.page, y: start.y + rim + gap };
        let mut waiting: Vec<Waiting> = Vec::new();
        let rows = table.rows.len();
        // The page the first row last stood on, for a table that repeats it.
        let mut headed = start.page;
        for (r, (row, starts)) in table.rows.iter().zip(&places).enumerate() {
            if self.full {
                break;
            }
            let mut row_start = self.room(at, line_tall);
            if r == 0 {
                headed = row_start.page;
            } else if table.repeat && row_start.page > headed {
                headed = row_start.page;
                row_start = self.first_row_again(table, &places, (&lefts, &widths, gap), row_start, depth);
                // A first row that fills its page leaves the rest to go on without it.
                headed = headed.max(row_start.page);
            }
            for (cell, col) in row.iter().zip(starts) {
                let span = usize::from(cell.span.max(1)).min(columns - (*col).min(columns));
                let Some(left) = lefts.get(table.left_column(*col, span, columns)).copied() else { continue };
                let wide: f32 = widths.iter().skip(*col).take(span).sum::<f32>() + gap * (span.saturating_sub(1) as f32);
                let pad = padding(table, cell);
                let before = self.mark(row_start.page);
                let inner = At { page: row_start.page, y: row_start.y + pad[0] };
                let end = self.blocks(&cell.blocks, left + pad[3], (wide - pad[1] - pad[3]).max(6.0), inner, depth + 1);
                let after = self.mark(row_start.page);
                let mut end = At { page: end.page, y: end.y + pad[2] };
                // A cell that asks for a height has it, as far as the page goes.
                if cell.tall > 0 && end.page == row_start.page {
                    end.y = end.y.max((row_start.y + px(cell.tall)).min(self.bottom()));
                }
                waiting.push(Waiting {
                    cell,
                    x: left,
                    width: wide,
                    start: row_start,
                    end,
                    last_row: r.saturating_add(usize::from(cell.rows.max(1)) - 1).min(rows.saturating_sub(1)),
                    before,
                    after,
                });
            }
            let mut row_end = row_start;
            for w in waiting.iter().filter(|w| w.last_row <= r) {
                row_end = later(row_end, w.end);
            }
            let mut still: Vec<Waiting> = Vec::new();
            for w in waiting {
                if w.last_row > r {
                    still.push(w);
                    continue;
                }
                // What a cell holds stands at its top, middle or bottom, when all of it is on one page.
                let spare = row_end.y - w.end.y;
                if w.cell.valign != VAlign::Top && w.start.page == w.end.page && w.end.page == row_end.page && spare > 0.01 {
                    self.lower(w.start.page, w.before, w.after, if w.cell.valign == VAlign::Middle { spare / 2.0 } else { spare });
                }
                let edges = w.cell.edges.unwrap_or(if table.border { [lined; 4] } else { [Edge::default(); 4] });
                self.round = w.cell.radius;
                self.boxed(w.start, row_end, w.x, w.width, &edges, w.cell.background, fill_depth, true);
            }
            waiting = still;
            at = At { page: row_end.page, y: row_end.y + gap };
        }
        let end = At { page: at.page, y: at.y + rim };
        self.round = table.radius;
        self.boxed(start, end, x, target, &table.frame, table.fill, 0, false);
        end
    }
}
