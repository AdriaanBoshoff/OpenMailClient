//! The formatted document a message body is read into: blocks of styled runs, with tables.
//! It is what the reading pane draws; it holds no markup, no scripts and no fetched content.
//! A picture the body itself spells out is kept as the bytes the body gave; they are not decoded
//! here.

use std::sync::Arc;

/// Red, green, blue.
pub type Rgb = [u8; 3];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Doc {
    pub blocks: Vec<Block>,
    /// The body was longer than the reader's limits; the rest is not in the document.
    pub truncated: bool,
    /// Pictures and backgrounds that point at a server. None of them was fetched.
    pub remote_blocked: usize,
    /// Pictures written into the body itself, each as the bytes of a picture file.
    pub written: Vec<Arc<[u8]>>,
    /// Places the body keeps for attachments ([`Picture::Object`]), counted even where no
    /// run was kept for one.
    pub objects: usize,
    /// The colour the body asks to be read on, behind all of it; `None` is the reader's own.
    pub fill: Option<Rgb>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Para(Para),
    /// A horizontal line.
    Rule,
    Table(Table),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Para {
    pub runs: Vec<Run>,
    pub kind: ParaKind,
    pub align: Align,
    /// List and definition nesting.
    pub indent: u8,
    /// Quotation nesting; each level is drawn with a bar.
    pub quote: u8,
    /// The paragraph has a margin above it.
    pub gap: bool,
    /// The border the paragraph stands inside. Paragraphs that follow one another and share one
    /// frame stand inside one border.
    pub frame: Option<Arc<Frame>>,
    /// The paragraph starts below any picture standing at a side.
    pub clear: bool,
    /// Pixels of room asked for above the paragraph, beside any margin it has.
    pub above: u8,
    /// Where text after a tab goes on to, from the nearest on. Past the last of them, and
    /// where there are none, a tab goes on to the next of the stops every paragraph has.
    pub tabs: Vec<Tab>,
    /// How tall the paragraph's lines are.
    pub leading: Leading,
    /// The paragraph is written from right to left: its first word stands at the right, and
    /// left-to-right words inside it keep their own order (UAX #9, the paragraph level).
    pub rtl: bool,
}

/// How tall a paragraph's lines are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Leading {
    /// As tall as what stands in them.
    #[default]
    Own,
    /// Hundredths of their own height.
    Times(u16),
    /// Their own height, and no less than this many pixels.
    AtLeast(u16),
    /// This many pixels, whatever stands in them.
    Exactly(u16),
}

/// The least and the most a line's height may be multiplied by, in hundredths.
pub const LEADING_TIMES: (u16, u16) = (50, 1000);
/// The most pixels a line may be told to be tall.
pub const LEADING_PX: u16 = 1024;

impl Leading {
    /// The height of a line whose own height is `own`; `scale` is the measure of `own` to
    /// the pixel. Never less than nothing, and `own` where the numbers are none.
    pub fn tall(self, own: f32, scale: f32) -> f32 {
        let own = if own.is_finite() { own.max(0.0) } else { 0.0 };
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        let px = |v: u16| f32::from(v.clamp(1, LEADING_PX)) * scale;
        let tall = match self {
            Leading::Own => own,
            Leading::Times(n) => own * f32::from(n.clamp(LEADING_TIMES.0, LEADING_TIMES.1)) / 100.0,
            Leading::AtLeast(v) => own.max(px(v)),
            Leading::Exactly(v) => px(v),
        };
        if tall.is_finite() { tall } else { own }
    }
}

/// A place text after a tab goes on to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tab {
    /// Pixels from where the paragraph's text starts.
    pub at: u16,
    pub kind: TabKind,
    /// What fills the room the tab takes.
    pub lead: Leader,
}

/// What fills the room a tab takes up to its stop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Leader {
    #[default]
    None,
    Dots,
    /// Dots at the middle of the letters.
    MiddleDots,
    Hyphens,
    Line,
    ThickLine,
    Equals,
}

/// How the text after a tab stands at its stop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabKind {
    /// It starts there.
    #[default]
    Left,
    /// Its middle is there.
    Center,
    /// It ends there.
    Right,
    /// Its decimal point is there.
    Decimal,
}

/// Pixels between the stops every paragraph has: half an inch.
pub const TAB_STEP: u16 = 48;

/// The room a tab takes. `x` is where it stands in its line, `step` the distance between the
/// stops every paragraph has, `after` the width of the text up to the next tab or the end of
/// the line and `point` the width of that text up to its decimal point, all in one measure;
/// `scale` is that measure to the pixel.
pub fn tab_room(tabs: &[Tab], scale: f32, step: f32, x: f32, after: f32, point: f32) -> f32 {
    let step = if step.is_finite() { step.max(1.0) } else { 1.0 };
    if !x.is_finite() || x < 0.0 {
        return step;
    }
    let ahead = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
    let room = match tab_stop(tabs, scale, x) {
        None => {
            let on = ((x / step).floor() + 1.0) * step - x;
            if on > 0.0 { on } else { step }
        }
        Some((at, tab)) => match tab.kind {
            TabKind::Left => at - x,
            TabKind::Center => at - x - ahead(after) / 2.0,
            TabKind::Right => at - x - ahead(after),
            TabKind::Decimal => at - x - ahead(point),
        },
    };
    // A place too far along for the arithmetic to hold still takes a step.
    if room.is_finite() { room.max(0.0) } else { step }
}

/// The stop of its own that a tab standing at `x` goes on to, and where that is in the
/// measure of `x`; `scale` is that measure to the pixel. None where the paragraph has no
/// stop ahead.
pub fn tab_stop(tabs: &[Tab], scale: f32, x: f32) -> Option<(f32, Tab)> {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    if !x.is_finite() || x < 0.0 {
        return None;
    }
    tabs.iter().map(|t| (f32::from(t.at) * scale, *t)).find(|(at, _)| at.is_finite() && *at > x + 0.5)
}

/// One side of a border.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edge {
    /// Pixels; 0 is no line.
    pub width: u8,
    /// `None` is the colour of the text.
    pub color: Option<Rgb>,
    pub line: Line,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Line {
    #[default]
    Solid,
    Dashed,
    Dotted,
    Double,
}

/// The four sides of a border: top, right, bottom, left.
pub type Edges = [Edge; 4];

/// Whether any side has a line.
pub fn any_edge(edges: &Edges) -> bool {
    edges.iter().any(|e| e.width > 0)
}

/// A border round one or more paragraphs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    pub edges: Edges,
    /// Room between the border and what it holds: above, right, below, left.
    pub padding: [u16; 4],
    /// How far in the border itself stands, in the levels a paragraph's indent is counted in.
    pub indent: u8,
    /// The line between one paragraph and the next inside the border.
    pub between: Edge,
    /// The colour the box is filled with, behind what it holds.
    pub fill: Option<Rgb>,
    /// The box this one stands inside.
    pub within: Option<Arc<Frame>>,
    /// How far its corners are rounded, in pixels; 0 is square.
    pub radius: u16,
}

/// The most a corner is rounded by, in pixels.
pub const MAX_RADIUS: u16 = 64;

/// One line that is all four sides of `edges`, where they are the same line and can be seen.
pub fn one_edge(edges: &Edges) -> Option<Edge> {
    let first = edges.first().copied().filter(|e| e.width > 0)?;
    edges.iter().all(|e| *e == first).then_some(first)
}

/// How many boxes may stand inside one another.
pub const MAX_BOXES: usize = 8;

impl Frame {
    /// How many boxes this one stands inside; no more than are ever looked at.
    pub fn depth(&self) -> usize {
        let (mut depth, mut at) = (0usize, self.within.as_ref());
        while let Some(outer) = at.filter(|_| depth < MAX_BOXES * 2) {
            depth += 1;
            at = outer.within.as_ref();
        }
        depth
    }

    /// The colour behind what the box holds: its own, or that of the nearest box round it
    /// that has one.
    pub fn behind(&self) -> Option<Rgb> {
        let (mut left, mut at) = (MAX_BOXES * 2, Some(self));
        while let Some(frame) = at.filter(|_| left > 0) {
            if frame.fill.is_some() {
                return frame.fill;
            }
            left -= 1;
            at = frame.within.as_deref();
        }
        None
    }
}

/// Of the boxes `frame` stands in and itself, the outermost first, the one `level` deep.
/// `None` past the innermost, and past the [`MAX_BOXES`] that are ever drawn.
pub fn box_at(frame: &Arc<Frame>, level: usize) -> Option<&Arc<Frame>> {
    let depth = frame.depth();
    if level > depth || level >= MAX_BOXES {
        return None;
    }
    let mut at = frame;
    for _ in level..depth {
        at = at.within.as_ref()?;
    }
    Some(at)
}

/// The box that is `level` deep round the first of `list`, and how many blocks from the first
/// on stand in that same box. `None` where the first block stands in no box that deep.
pub fn boxed_run(list: &[Block], level: usize) -> Option<(&Arc<Frame>, usize)> {
    let frame = box_at(list.first()?.boxed()?, level)?;
    let same = |b: &Block| b.boxed().and_then(|f| box_at(f, level)).is_some_and(|f| Arc::ptr_eq(f, frame));
    Some((frame, 1 + list.get(1..).unwrap_or_default().iter().take_while(|b| same(b)).count()))
}

impl Block {
    /// The innermost box the block stands in.
    pub fn boxed(&self) -> Option<&Arc<Frame>> {
        match self {
            Block::Para(p) => p.frame.as_ref(),
            Block::Table(t) => t.inside.as_ref(),
            Block::Rule => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ParaKind {
    #[default]
    Body,
    /// Heading level 1 to 6.
    Heading(u8),
    /// Preformatted: line breaks and spaces are kept.
    Pre,
    /// A list item with its marker ("•" or "3.").
    Item(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    /// For a picture this is its alternative text.
    pub text: String,
    pub style: Style,
    pub picture: Option<Picture>,
    /// Width and height the body asks a picture to be shown at, in pixels; 0 where it gives none.
    pub extent: (u16, u16),
    /// A picture that stands at one side with the paragraph's text beside it.
    pub float: Float,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Float {
    #[default]
    None,
    Left,
    Right,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub mono: bool,
    /// The fonts asked for, by name, as the body lists them (first choice first, separated by
    /// commas). They are names only: whether one is installed is the reader's business.
    pub family: Option<Arc<str>>,
    pub script: Script,
    /// Font size in points; `None` is the reader's default.
    pub size: Option<f32>,
    pub color: Option<Rgb>,
    pub background: Option<Rgb>,
    /// Where a link points. It is shown, never followed by the reader.
    pub link: Option<Arc<str>>,
    /// A line drawn round the text where it stands in its line.
    pub edge: Option<Edge>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Script {
    #[default]
    None,
    Super,
    Sub,
}

/// Where a picture's data is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Picture {
    /// An attachment of the same message, by content id.
    Attached(String),
    /// Data written into the body itself: its place in [`Doc::written`], or `None` where the
    /// data was not kept (not readable, too large, or of a kind that is not read).
    Inline(Option<usize>),
    /// On a server, by its address. Fetched only when the reader asks; an address too long
    /// to keep is empty.
    Remote(String),
    /// The place an RTF body keeps for an attachment (`\objattph`), by its number among such
    /// places. Which attachment stands there is for whoever has the message to say.
    Object(usize),
    /// An attachment that stands in the text as a file of its own. The run's text names it in
    /// words, for whatever reads the body as text.
    File(Filed),
}

/// What is known of an attachment that stands in the text, for showing it there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filed {
    /// Its number among the message's attachments, when it can be saved as a file.
    pub number: Option<usize>,
    /// Its name, as it would be saved: nothing in it is hidden or reversed.
    pub name: String,
    /// What an object of another program calls itself.
    pub kind: Option<String>,
    /// Its size in bytes.
    pub bytes: usize,
    /// What a reader should know before saving it, in a sentence.
    pub caution: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub rows: Vec<Vec<Cell>>,
    pub border: bool,
    pub width: TableWidth,
    /// Where the table stands when it is narrower than the room it has.
    pub align: Align,
    /// Room between a cell's edge and what it holds, in pixels, for cells that set none.
    pub padding: u16,
    /// Room between cells, and between the cells and the table's edge, in pixels.
    pub spacing: u16,
    /// The table's own border, round all its cells.
    pub frame: Edges,
    /// The colour of the lines `border` asks for; `None` is the reader's.
    pub line: Option<Rgb>,
    /// The table's own background, behind its cells and the room between them.
    pub fill: Option<Rgb>,
    /// Rounded corners of its own border and background, in pixels.
    pub radius: u16,
    /// The table stands right under what is before it, with no room left between them.
    pub snug: bool,
    /// On paper, the first row stands again at the top of each page the table goes on to.
    pub repeat: bool,
    /// The table is written from the right: its first column stands at the right.
    pub rtl: bool,
    /// The box the table stands inside, as a paragraph stands inside its `frame`.
    pub inside: Option<Arc<Frame>>,
}

/// How wide a table asks to be.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableWidth {
    /// As wide as what it holds.
    #[default]
    Content,
    Pixels(u16),
    /// This many hundredths of the room there is, 1 to 99.
    Percent(u8),
    /// All the room there is.
    Fill,
}

/// Where what a cell holds stands when the cell is taller than it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    pub blocks: Vec<Block>,
    pub header: bool,
    /// Columns the cell covers, at least 1.
    pub span: u16,
    /// Fills the whole cell, behind its text.
    pub background: Option<Rgb>,
    /// Width the cell asks for, in pixels; 0 where it gives none.
    pub width: u16,
    /// The cell's own padding, above, right, below and left; `None` is the table's.
    pub padding: Option<[u16; 4]>,
    /// Width the cell asks for as hundredths of the table's, 1 to 100; 0 where it gives none.
    pub share: u8,
    pub valign: VAlign,
    /// Rows the cell covers, at least 1.
    pub rows: u16,
    /// How far the cell's corners are rounded, in pixels; 0 is square.
    pub radius: u16,
    /// The cell's own border; `None` is what the table gives its cells.
    pub edges: Option<Edges>,
    /// The least height it stands in, in pixels, padding and all; 0 where it asks for none and
    /// is as tall as what it holds.
    pub tall: u16,
}

impl Doc {
    /// The document's text, one line per paragraph; cells of a row are separated by tabs.
    pub fn text(&self) -> String {
        let mut out = String::new();
        blocks_text(&self.blocks, &mut out, 0);
        out.trim_end().to_string()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Plain text as a document: a paragraph for each of its lines.
    pub fn plain(text: &str) -> Doc {
        /// Lines and characters one plain body may have.
        const MAX_LINES: usize = 50_000;
        const MAX_CHARS: usize = 4_000_000;
        let mut doc = Doc::default();
        let mut left = MAX_CHARS;
        for line in text.lines() {
            if doc.blocks.len() >= MAX_LINES || left == 0 {
                doc.truncated = true;
                break;
            }
            let kept: String = line.chars().filter(|c| *c == '\t' || !c.is_control()).take(left).collect();
            left = left.saturating_sub(kept.chars().count().max(1));
            let runs = if kept.is_empty() { Vec::new() } else { vec![Run { text: kept, ..Run::default() }] };
            doc.blocks.push(Block::Para(Para { runs, ..Para::default() }.by_its_text()));
        }
        doc
    }
}

impl Para {
    /// The paragraph with the direction of the first of its letters that has one, and at the
    /// right if that is right to left: for text that does not say which way it is written.
    pub fn by_its_text(mut self) -> Para {
        let first = self.runs.iter().filter(|run| run.picture.is_none()).find_map(|run| openmailclient_fonts::script::first_strong(&run.text));
        if first == Some(true) {
            self.rtl = true;
            if self.align == Align::Left {
                self.align = Align::Right;
            }
        }
        self
    }

    /// Pixels of room between this paragraph and one above it.
    pub fn lead(&self) -> u16 {
        let margin = match (self.gap, &self.kind) {
            (false, _) => 0,
            (true, ParaKind::Heading(_)) => 12,
            (true, _) => 9,
        };
        margin + u16::from(self.above)
    }

    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

impl Table {
    /// Where each column starts, for columns `widths` wide that stand `gap` apart between
    /// `left` and `right`: the first at the left, or at the right in a table written from the
    /// right (HTML 4.01 section 11.2.1, `dir` on a table: "columns are laid out right to left").
    pub fn lefts(&self, widths: &[f32], left: f32, right: f32, gap: f32) -> Vec<f32> {
        let mut lefts = Vec::with_capacity(widths.len());
        let mut pen = if self.rtl { right } else { left };
        for wide in widths {
            if self.rtl {
                pen -= wide;
                lefts.push(pen);
                pen -= gap;
            } else {
                lefts.push(pen);
                pen += wide + gap;
            }
        }
        lefts
    }

    /// Which column a cell's left edge is the left edge of: the cell starts in column `col` and
    /// covers `span` of the table's `columns`.
    pub fn left_column(&self, col: usize, span: usize, columns: usize) -> usize {
        if self.rtl { col.saturating_add(span.max(1) - 1).min(columns.saturating_sub(1)) } else { col }
    }

    /// Columns in the widest row, counting spans and the cells that reach down from rows above.
    pub fn columns(&self) -> usize {
        if !self.reaches_down() {
            return self.rows.iter().map(|r| r.iter().map(|c| usize::from(c.span.max(1))).sum::<usize>()).max().unwrap_or(0);
        }
        self.walk(|_, _, _| {})
    }

    /// The column each cell starts in, row by row. A cell that covers several rows takes its
    /// columns in the rows below it too, and the cells of those rows stand after it
    /// (HTML 4.01 section 11.2.6.1).
    pub fn places(&self) -> Vec<Vec<usize>> {
        let mut places: Vec<Vec<usize>> = self.rows.iter().map(|row| Vec::with_capacity(row.len())).collect();
        self.walk(|row, _, column| {
            if let Some(known) = places.get_mut(row) {
                known.push(column);
            }
        });
        places
    }

    fn reaches_down(&self) -> bool {
        self.rows.iter().flatten().any(|c| c.rows > 1)
    }

    /// Go through the cells in order, telling `place` each one's row, number in the row and
    /// column. Returns the number of columns.
    fn walk(&self, mut place: impl FnMut(usize, usize, usize)) -> usize {
        // Rows each column is still covered for from above. Columns past the limit are not
        // tracked: a cell there covers its own row only.
        const TRACKED: usize = 1024;
        let mut down: Vec<u16> = Vec::new();
        let mut live = 0usize;
        let mut columns = 0usize;
        for (r, row) in self.rows.iter().enumerate() {
            let mut col = 0usize;
            for (i, cell) in row.iter().enumerate() {
                while down.get(col).is_some_and(|rows| *rows > 0) {
                    col += 1;
                }
                place(r, i, col);
                let span = usize::from(cell.span.max(1));
                if cell.rows > 1 {
                    for k in col..col.saturating_add(span).min(TRACKED) {
                        if down.len() <= k {
                            down.resize(k + 1, 0);
                        }
                        if let Some(rows) = down.get_mut(k) {
                            live += usize::from(*rows == 0);
                            *rows = cell.rows;
                        }
                    }
                }
                col = col.saturating_add(span);
            }
            columns = columns.max(col);
            if live > 0 {
                for rows in down.iter_mut().filter(|rows| **rows > 0) {
                    *rows -= 1;
                    live -= usize::from(*rows == 0);
                }
            }
        }
        columns.max(down.len())
    }
}

/// Characters of text in `blocks`, counted up to `cap`. Used to share out column widths.
pub fn text_len(blocks: &[Block], cap: usize) -> usize {
    let mut n = 0usize;
    for block in blocks {
        if n >= cap {
            break;
        }
        match block {
            Block::Para(p) => n = n.saturating_add(p.runs.iter().map(|r| r.text.len()).sum::<usize>()),
            Block::Rule => {}
            Block::Table(t) => {
                for cell in t.rows.iter().flatten() {
                    n = n.saturating_add(text_len(&cell.blocks, cap.saturating_sub(n)));
                }
            }
        }
    }
    n.min(cap)
}

fn blocks_text(blocks: &[Block], out: &mut String, depth: usize) {
    for block in blocks {
        match block {
            Block::Para(p) => {
                if let ParaKind::Item(marker) = &p.kind {
                    out.push_str(marker);
                    out.push(' ');
                }
                out.push_str(&p.text());
                out.push('\n');
            }
            Block::Rule => out.push_str("----\n"),
            Block::Table(t) => {
                for row in &t.rows {
                    let mut cells = Vec::new();
                    for cell in row {
                        let mut s = String::new();
                        if depth < 64 {
                            blocks_text(&cell.blocks, &mut s, depth + 1);
                        }
                        cells.push(s.trim_end().replace('\n', " "));
                    }
                    out.push_str(&cells.join("\t"));
                    out.push('\n');
                }
            }
        }
    }
}
