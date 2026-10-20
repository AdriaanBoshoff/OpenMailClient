//! HTML message bodies read into a [`Doc`].
//!
//! The reader is tolerant, because mail HTML breaks every rule, and it is inert: scripts,
//! frames, objects and forms are dropped, nothing is fetched, and a link is kept only as the text
//! of its address. Every size, depth and count that comes from the message is capped.
//!
//! Element and attribute meanings follow the HTML Living Standard (sections 4 and 15, "Rendering");
//! only the presentational subset mail uses is understood. Style comes from inline `style`
//! declarations and from the rules of `<style>` sheets that pick elements by name, class or id
//! (CSS 2.1 section 5, type, class and ID selectors); other selectors and at-rules are skipped.
//! A picture written into the body (a `data:` address, RFC 2397, in base64, RFC 4648 section 4)
//! is kept as bytes for the reader to decode; nothing else about it is trusted.

use crate::rich::{
    Align, Block, Cell, Doc, Edge, Edges, Float, Frame, LEADING_PX, LEADING_TIMES, Leading, Line, MAX_BOXES, MAX_RADIUS, Para, ParaKind, Picture,
    Rgb, Run, Script, Style, Table, TableWidth, VAlign, any_edge,
};
use openmailclient_model::html::entity;
use std::collections::HashMap;
use std::sync::Arc;

/// HTML beyond this many bytes is not read.
const MAX_INPUT: usize = 4 << 20;
/// Text beyond this many bytes is not kept.
const MAX_TEXT: usize = 2 << 20;
const MAX_BLOCKS: usize = 50_000;
const MAX_RUNS: usize = 200_000;
/// Open elements beyond this depth are not tracked.
const MAX_DEPTH: usize = 256;
/// Tables nested deeper than this are read into the innermost table kept.
const MAX_TABLE_DEPTH: usize = 24;
const MAX_ATTRS: usize = 32;
const MAX_ATTR_LEN: usize = 2048;
const MAX_ALT_LEN: usize = 200;
/// The longest address of a picture on a server that is kept.
const MAX_REMOTE_ADDRESS: usize = 2048;
/// Longest list of font names kept for a run.
const MAX_FAMILY_LEN: usize = 160;
/// A picture written into the body: the longest address read, and how many pictures and bytes
/// of them one body may keep.
const MAX_DATA_ADDRESS: usize = 3 << 20;
pub(crate) const MAX_WRITTEN: usize = 64;
pub(crate) const MAX_WRITTEN_BYTES: usize = 12 << 20;
/// Largest width or height, in pixels, a picture may ask to be shown at.
const MAX_PICTURE_SIDE: f32 = 4096.0;
/// Relative sizes (`em`, `%`, `<small>`, headings) start from this many points when no size is
/// set: the size Outlook Classic shows unstyled HTML text at.
const BASE_PT: f32 = 12.0;
/// Style sheet text beyond this many bytes, and rules beyond this many, are not read.
const MAX_SHEET: usize = 512 << 10;
const MAX_RULES: usize = 1024;
/// Rules that may apply to one element.
const MAX_MATCHES: usize = 64;
/// Largest cell padding or spacing, in pixels.
const MAX_CELL_ROOM: f32 = 64.0;
/// Cell padding and spacing of a table that sets none (HTML 15.3.10, tables).
const CELL_PADDING: u16 = 1;
/// Widest border line kept, in pixels, and most rows one cell may cover.
const MAX_EDGE: f32 = 12.0;
const MAX_ROW_SPAN: u16 = 1000;
const CELL_SPACING: u16 = 2;

/// Elements whose content is dropped without being read.
const RAW: &[&str] = &["script", "style", "title", "iframe", "object", "embed", "svg", "template", "select", "textarea", "noframes"];
const VOID: &[&str] = &["br", "hr", "img", "meta", "link", "input", "col", "wbr", "area", "base", "param", "source", "track"];
const BLOCK: &[&str] = &[
    "p",
    "div",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "menu",
    "dir",
    "li",
    "dl",
    "dt",
    "dd",
    "blockquote",
    "pre",
    "center",
    "section",
    "article",
    "header",
    "footer",
    "address",
    "figure",
    "figcaption",
    "form",
    "fieldset",
    "main",
    "nav",
    "aside",
    "body",
    "html",
];
/// Blocks that have a margin above and below.
const GAP: &[&str] = &["p", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "pre", "dl", "figure"];

/// Read an HTML body. Never fails: what cannot be understood is skipped.
pub fn parse(html: &str) -> Doc {
    let mut b = Builder { sinks: vec![Vec::new()], ..Builder::default() };
    let mut end = html.len().min(MAX_INPUT);
    while end > 0 && !html.is_char_boundary(end) {
        end -= 1;
    }
    b.doc.truncated = end < html.len();
    let mut rest = html.get(..end).unwrap_or("");
    while !rest.is_empty() && !b.full {
        let Some(lt) = rest.find('<') else {
            b.text(rest);
            break;
        };
        b.text(rest.get(..lt).unwrap_or(""));
        let after = rest.get(lt + 1..).unwrap_or("");
        if let Some(comment) = after.strip_prefix("!--") {
            rest = comment.find("-->").and_then(|i| comment.get(i + 3..)).unwrap_or("");
            continue;
        }
        match after.chars().next() {
            Some('!' | '?') => {
                rest = after.find('>').and_then(|i| after.get(i + 1..)).unwrap_or("");
                continue;
            }
            Some(c) if c.is_ascii_alphabetic() || c == '/' => {}
            _ => {
                b.text("<");
                rest = after;
                continue;
            }
        }
        let Some(gt) = tag_end(after) else { break };
        let tag = after.get(..gt).unwrap_or("");
        rest = after.get(gt + 1..).unwrap_or("");
        let closing = tag.starts_with('/');
        let body = tag.trim_start_matches('/');
        let name_len = body.find(|c: char| !(c.is_ascii_alphanumeric() || c == ':' || c == '-')).unwrap_or(body.len());
        let name = body.get(..name_len).unwrap_or("").to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        if closing {
            b.close_tag(&name);
            continue;
        }
        if RAW.contains(&name.as_str()) {
            let end_tag = format!("</{name}");
            let close = find_ci(rest, &end_tag);
            if name == "style" {
                b.sheet.read(rest.get(..close.unwrap_or(rest.len())).unwrap_or(""));
            }
            rest = close.and_then(|i| rest.get(i..)).and_then(|from| from.find('>').and_then(|i| from.get(i + 1..))).unwrap_or("");
            continue;
        }
        let attrs = attributes(body.get(name_len..).unwrap_or(""));
        b.open_tag(&name, &attrs, body.trim_end().ends_with('/'));
    }
    b.finish()
}

/// What the current position inherits: the inline style and the block settings.
#[derive(Clone, Default)]
struct Fmt {
    style: Style,
    align: Align,
    indent: u8,
    quote: u8,
    heading: u8,
    pre: bool,
    hidden: bool,
    /// The border the paragraphs here stand inside.
    frame: Option<Arc<Frame>>,
    leading: Leading,
    /// Written from right to left (`dir`, CSS `direction`).
    rtl: bool,
    /// `dir="auto"`: each paragraph is written the way its first letter is.
    by_text: bool,
}

struct Open {
    name: String,
    saved: Fmt,
    /// The element pushed a list level.
    list: bool,
    /// The element leaves a margin after it.
    gap: bool,
    /// The marks that begin and end what the element says of the way its text runs (see
    /// `turning`).
    turn: Option<(char, char)>,
    /// The paragraph the opening mark was last written in; 0 for none.
    said: u64,
}

struct TableBuild {
    /// Written from the right: `dir` on the table or on what it stands in.
    rtl: bool,
    rows: Vec<Vec<Cell>>,
    border: bool,
    width: TableWidth,
    align: Align,
    padding: u16,
    spacing: u16,
    frame: Edges,
    line: Option<Rgb>,
    /// The box the table stands inside.
    inside: Option<Arc<Frame>>,
    /// The table's own background, behind its cells.
    fill: Option<Rgb>,
    radius: u16,
    /// Background of the row being read; its cells start from it.
    row_fill: Option<Rgb>,
    row_valign: Option<VAlign>,
    /// Length of the element stack just after the table's own entry.
    open_len: usize,
    cell: Option<CellBuild>,
}

struct CellBuild {
    saved: Fmt,
    open_len: usize,
    header: bool,
    span: u16,
    background: Option<Rgb>,
    width: u16,
    padding: Option<[u16; 4]>,
    share: u8,
    valign: VAlign,
    rows: u16,
    edges: Option<Edges>,
    radius: u16,
}

struct ParaHead {
    kind: ParaKind,
    align: Align,
    indent: u8,
    quote: u8,
    gap: bool,
    frame: Option<Arc<Frame>>,
    clear: bool,
    leading: Leading,
    rtl: bool,
    by_text: bool,
}

#[derive(Default)]
struct Builder {
    doc: Doc,
    sheet: Sheet,
    /// Block lists being filled: the document, then one per open table cell.
    sinks: Vec<Vec<Block>>,
    open: Vec<Open>,
    tables: Vec<TableBuild>,
    /// Open lists: ordered, and the last number given out.
    lists: Vec<(bool, u32)>,
    fmt: Fmt,
    head: Option<ParaHead>,
    runs: Vec<Run>,
    last_space: bool,
    marker: Option<String>,
    gap: bool,
    blocks: usize,
    run_count: usize,
    /// How many paragraphs have been started.
    para_no: u64,
    /// The paragraph being written has a mark in it that turns letters round one by one.
    overridden: bool,
    bytes: usize,
    written_bytes: usize,
    full: bool,
    /// The next paragraph starts below any picture standing at a side.
    clear: bool,
    /// The border and the padding the element being opened asks for in its style.
    css_border: Option<Edges>,
    css_padding: Option<[u16; 4]>,
    /// The background the element being opened asks for, as it is written.
    css_fill: Option<Rgb>,
    /// How far the element being opened asks for its corners to be rounded, in pixels.
    css_radius: u16,
}

impl Builder {
    fn finish(mut self) -> Doc {
        while !self.tables.is_empty() {
            self.close_table();
        }
        self.end_para();
        self.doc.blocks = self.sinks.drain(..).next().unwrap_or_default();
        self.doc
    }

    fn push_block(&mut self, block: Block) {
        self.blocks += 1;
        if self.blocks > MAX_BLOCKS {
            self.full = true;
            self.doc.truncated = true;
            return;
        }
        match self.sinks.last_mut() {
            Some(sink) => sink.push(block),
            None => self.sinks.push(vec![block]),
        }
    }

    /// Elements below this index were opened outside the current cell or table and cannot be
    /// closed from inside it.
    fn floor(&self) -> usize {
        match self.tables.last() {
            Some(t) => t.cell.as_ref().map_or(t.open_len, |c| c.open_len),
            None => 0,
        }
    }

    /// Text and pictures directly inside a table, outside any cell, get a cell of their own.
    fn ensure_cell(&mut self) {
        if self.tables.last().is_some_and(|t| t.cell.is_none()) {
            self.open_cell(false, 1);
        }
    }

    fn start_para(&mut self) {
        if self.head.is_some() {
            return;
        }
        let kind = match self.marker.take() {
            Some(marker) => ParaKind::Item(marker),
            None if self.fmt.heading > 0 => ParaKind::Heading(self.fmt.heading),
            None if self.fmt.pre => ParaKind::Pre,
            None => ParaKind::Body,
        };
        self.head = Some(ParaHead {
            kind,
            align: self.fmt.align,
            indent: self.fmt.indent,
            quote: self.fmt.quote,
            gap: std::mem::take(&mut self.gap),
            frame: self.fmt.frame.clone(),
            clear: std::mem::take(&mut self.clear),
            leading: self.fmt.leading,
            rtl: self.fmt.rtl,
            by_text: self.fmt.by_text,
        });
        self.para_no = self.para_no.saturating_add(1);
        self.overridden = false;
        self.last_space = true;
    }

    fn end_para(&mut self) {
        let Some(head) = self.head.take() else { return };
        let mut runs = std::mem::take(&mut self.runs);
        self.last_space = true;
        // A line break just before the end of a block does not make a line of its own.
        let mut newline_left = true;
        while let Some(last) = runs.last_mut() {
            if last.picture.is_some() {
                break;
            }
            let mut t = last.text.trim_end_matches(' ');
            if newline_left && let Some(s) = t.strip_suffix('\n') {
                t = s.trim_end_matches(' ');
                newline_left = false;
            }
            let n = t.len();
            last.text.truncate(n);
            if last.text.is_empty() {
                runs.pop();
            } else {
                break;
            }
        }
        if runs.is_empty() {
            if newline_left {
                return;
            }
            // Only a line break: an empty line.
            runs.push(Run { text: "\u{a0}".to_string(), ..Run::default() });
        }
        let para = Para {
            runs,
            kind: head.kind,
            align: head.align,
            indent: head.indent,
            quote: head.quote,
            gap: head.gap,
            frame: head.frame,
            clear: head.clear,
            above: 0,
            tabs: Vec::new(),
            leading: head.leading,
            rtl: head.rtl && !head.by_text,
        };
        // `dir="auto"`: the way the paragraph's first letter is written, and its side with it.
        let para = if head.by_text {
            let align = if head.rtl && para.align == Align::Right { Align::Left } else { para.align };
            Para { align, ..para }.by_its_text()
        } else {
            para
        };
        self.push_block(Block::Para(para));
    }

    fn text(&mut self, raw: &str) {
        if self.fmt.hidden || raw.is_empty() {
            return;
        }
        let mut buf = String::new();
        let mut last_space = self.head.is_none() || self.last_space;
        let mut rest = raw;
        while let Some(c) = rest.chars().next() {
            if c == '&'
                && let Some((decoded, len)) = entity(rest)
            {
                for d in decoded.chars().filter(|d| !d.is_control()) {
                    buf.push(d);
                    last_space = false;
                }
                rest = rest.get(len..).unwrap_or("");
                continue;
            }
            rest = rest.get(c.len_utf8()..).unwrap_or("");
            if self.fmt.pre {
                match c {
                    '\n' => buf.push('\n'),
                    '\t' => buf.push_str("    "),
                    c if c.is_control() => {}
                    c => buf.push(c),
                }
                last_space = false;
            } else if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c') {
                if !last_space {
                    buf.push(' ');
                    last_space = true;
                }
            } else if !c.is_control() {
                buf.push(c);
                last_space = false;
            }
        }
        if self.fmt.style.link.is_some() {
            buf.retain(|c| !OVERRIDES.contains(&c));
        }
        if buf.is_empty() {
            return;
        }
        if self.tables.last().is_some_and(|t| t.cell.is_none()) {
            if buf.trim().is_empty() {
                return;
            }
            self.ensure_cell();
        }
        self.append(buf, None);
        self.last_space = last_space;
    }

    fn append(&mut self, text: String, picture: Option<Picture>) {
        self.append_sized(text, picture, (0, 0), Float::None);
    }

    fn append_sized(&mut self, mut text: String, picture: Option<Picture>, extent: (u16, u16), float: Float) {
        self.bytes = self.bytes.saturating_add(text.len());
        if self.bytes > MAX_TEXT || self.run_count >= MAX_RUNS {
            self.full = true;
            self.doc.truncated = true;
            return;
        }
        self.start_para();
        // An element that says which way its text runs says so in every paragraph it has text
        // in, before the first of it.
        let (floor, para) = (self.floor(), self.para_no);
        let mut marks = String::new();
        for o in self.open.iter_mut().skip(floor) {
            if let Some((opener, _)) = o.turn
                && o.said != para
            {
                o.said = para;
                marks.push(opener);
            }
        }
        self.overridden |= text.contains(OVERRIDES) || marks.contains(OVERRIDES);
        if !marks.is_empty() {
            if picture.is_none() {
                text.insert_str(0, &marks);
            } else {
                self.run_count += 1;
                self.runs.push(Run { text: marks, style: self.fmt.style.clone(), ..Run::default() });
            }
        }
        if picture.is_none()
            && let Some(last) = self.runs.last_mut()
            && last.picture.is_none()
            && last.style == self.fmt.style
        {
            last.text.push_str(&text);
            return;
        }
        self.run_count += 1;
        self.runs.push(Run { text, style: self.fmt.style.clone(), picture, extent, float });
    }

    fn line_break(&mut self) {
        if self.fmt.hidden || self.tables.last().is_some_and(|t| t.cell.is_none()) {
            return;
        }
        self.append("\n".to_string(), None);
        self.last_space = true;
    }

    fn picture(&mut self, attrs: &[(String, String)]) {
        if self.fmt.hidden {
            return;
        }
        let src = attr(attrs, "src").unwrap_or("").trim();
        let lower: String = src.chars().take(8).collect::<String>().to_ascii_lowercase();
        let picture = if let Some(cid) = lower.strip_prefix("cid:").and(src.get(4..)) {
            Picture::Attached(cid.to_string())
        } else if lower.starts_with("data:") {
            Picture::Inline(self.written(src))
        } else {
            if !src.is_empty() {
                self.doc.remote_blocked += 1;
            }
            Picture::Remote(if src.len() <= MAX_REMOTE_ADDRESS { src.to_string() } else { String::new() })
        };
        // One-pixel pictures are trackers or spacers; there is nothing to show for them.
        let asked = |name: &str| attr(attrs, name).and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok()).filter(|v| v.is_finite());
        let tiny = |name: &str| asked(name).is_some_and(|v| v <= 2.0);
        if src.is_empty() || tiny("width") || tiny("height") {
            return;
        }
        // A percentage is not a pixel size: it is left to the reader.
        let pixels = |name: &str| asked(name).map_or(0, |v| v.round().clamp(0.0, MAX_PICTURE_SIDE) as u16);
        let extent = (pixels("width"), pixels("height"));
        let alt: String = attr(attrs, "alt").unwrap_or("").chars().filter(|c| !c.is_control()).take(MAX_ALT_LEN).collect();
        // `align` puts a picture at one side with the text beside it (HTML 15.4.3, images).
        let float = match attr(attrs, "align").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("left") => Float::Left,
            Some("right") => Float::Right,
            _ => Float::None,
        };
        self.ensure_cell();
        self.append_sized(alt.trim().to_string(), Some(picture), extent, float);
        self.last_space = false;
    }

    /// Keep the bytes of a picture the body spells out in a `data:` address
    /// (RFC 2397: `data:[<type>][;base64],<data>`). Only base64 data is read.
    fn written(&mut self, src: &str) -> Option<usize> {
        let (head, data) = src.get(5..)?.split_once(',')?;
        if head.len() > 128 || !head.split(';').any(|part| part.trim().eq_ignore_ascii_case("base64")) {
            return None;
        }
        let bytes = base64(data)?;
        if bytes.is_empty() {
            return None;
        }
        if let Some(same) = self.doc.written.iter().position(|kept| **kept == *bytes) {
            return Some(same);
        }
        if self.doc.written.len() >= MAX_WRITTEN || self.written_bytes.saturating_add(bytes.len()) > MAX_WRITTEN_BYTES {
            return None;
        }
        self.written_bytes += bytes.len();
        self.doc.written.push(Arc::from(bytes));
        Some(self.doc.written.len() - 1)
    }

    fn open_tag(&mut self, name: &str, attrs: &[(String, String)], self_closing: bool) {
        match name {
            "br" => {
                // A break that clears ends the paragraph: what follows starts below any picture
                // standing at a side (HTML 4.01 section 15.1.3.1).
                if attr(attrs, "clear").is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "all" | "left" | "right" | "both"))
                    && !self.fmt.hidden
                {
                    self.end_para();
                    self.clear = true;
                    return;
                }
                return self.line_break();
            }
            "hr" => {
                self.end_para();
                if !self.fmt.hidden && !self.tables.last().is_some_and(|t| t.cell.is_none()) {
                    self.push_block(Block::Rule);
                }
                self.gap = true;
                return;
            }
            "img" => return self.picture(attrs),
            "table" => return self.open_table(attrs),
            "tr" => {
                if let Some(t) = self.tables.last()
                    && (t.cell.is_some() || t.rows.last().is_none_or(|r| !r.is_empty()))
                {
                    self.close_cell();
                    if let Some(t) = self.tables.last_mut() {
                        t.rows.push(Vec::new());
                    }
                }
                let css = self.sheet.declarations(name, attrs);
                let fill = css_value(&css, "background-color")
                    .or_else(|| css_value(&css, "background"))
                    .and_then(|v| color(&v).or_else(|| v.split_whitespace().find_map(color)))
                    .or_else(|| attr(attrs, "bgcolor").and_then(color));
                let place = css_value(&css, "vertical-align").as_deref().and_then(valign).or_else(|| attr(attrs, "valign").and_then(valign));
                if let Some(t) = self.tables.last_mut() {
                    t.row_fill = fill;
                    t.row_valign = place;
                }
                return;
            }
            "td" | "th" => {
                if self.tables.is_empty() {
                    self.end_para();
                    return;
                }
                self.close_cell();
                let row_fill = self.tables.last().and_then(|t| t.row_fill);
                let row_valign = self.tables.last().and_then(|t| t.row_valign);
                let span = attr(attrs, "colspan").and_then(|v| v.trim().parse::<u16>().ok()).unwrap_or(1).clamp(1, 64);
                // `rowspan=0` reaches to the table's last row (HTML 4.01 section 11.2.6).
                let rows = match attr(attrs, "rowspan").and_then(|v| v.trim().parse::<u32>().ok()) {
                    Some(0) => MAX_ROW_SPAN,
                    Some(n) => u16::try_from(n).unwrap_or(MAX_ROW_SPAN).min(MAX_ROW_SPAN),
                    None => 1,
                };
                self.open_cell(name == "th", span);
                if name == "th" {
                    self.fmt.style.bold = true;
                }
                self.common(name, attrs);
                // A background set on the cell or its row is the cell's and not its text's. A pale
                // one is kept as it is: the cell is drawn on it, as it is on paper.
                let inherited = self.fmt.style.background.take();
                let background = self.css_fill.take().or(row_fill).or(inherited);
                let css = self.sheet.declarations(name, attrs);
                let css_width = css_value(&css, "width");
                let asked = css_width.as_deref().or_else(|| attr(attrs, "width")).unwrap_or("");
                // Its padding, all four sides or each of them (CSS 2.1 section 8.4).
                let padding = self.css_padding.take();
                // What a cell holds stands in the middle of it unless the cell or its row says otherwise.
                let place = css_value(&css, "vertical-align").as_deref().and_then(valign).or_else(|| attr(attrs, "valign").and_then(valign));
                let edges = self.css_border.take().filter(any_edge);
                let radius = std::mem::take(&mut self.css_radius);
                if let Some(cell) = self.tables.last_mut().and_then(|t| t.cell.as_mut()) {
                    cell.rows = rows;
                    cell.edges = edges;
                    cell.radius = radius;
                    cell.background = background;
                    cell.width = pixels(asked).unwrap_or(0);
                    cell.share = percent(asked).unwrap_or(0);
                    cell.padding = padding;
                    cell.valign = place.or(row_valign).unwrap_or(VAlign::Middle);
                }
                return;
            }
            "tbody" | "thead" | "tfoot" | "caption" | "colgroup" => return,
            _ => {}
        }
        if VOID.contains(&name) || self_closing {
            return;
        }
        let block = BLOCK.contains(&name);
        if block {
            // A block ends an open paragraph element (HTML 13.1.2.4, optional end tags).
            let floor = self.floor();
            if let Some(i) = self.open.iter().rposition(|o| o.name == "p").filter(|i| *i >= floor)
                && self.open.get(i + 1..).is_some_and(|above| above.iter().all(|o| !BLOCK.contains(&o.name.as_str())))
            {
                self.end_para();
                self.pop_to(i);
            }
            if name == "li" {
                let list = self.open.iter().rposition(|o| o.list).map_or(floor, |i| (i + 1).max(floor));
                if let Some(i) = self.open.iter().rposition(|o| o.name == "li").filter(|i| *i >= list) {
                    self.end_para();
                    self.pop_to(i);
                }
            }
            self.end_para();
        }
        if self.open.len() >= MAX_DEPTH {
            return;
        }
        self.open.push(Open { name: name.to_string(), saved: self.fmt.clone(), list: false, gap: false, turn: None, said: 0 });
        let mut gap = GAP.contains(&name);
        let style = &mut self.fmt.style;
        match name {
            "b" | "strong" => style.bold = true,
            "i" | "em" | "cite" | "var" | "dfn" => style.italic = true,
            "u" | "ins" => style.underline = true,
            "s" | "strike" | "del" => style.strike = true,
            "code" | "tt" | "kbd" | "samp" => style.mono = true,
            "sup" => style.script = Script::Super,
            "sub" => style.script = Script::Sub,
            "small" => style.size = Some((style.size.unwrap_or(BASE_PT) * 0.83).clamp(6.0, 72.0)),
            "big" => style.size = Some((style.size.unwrap_or(BASE_PT) * 1.2).clamp(6.0, 72.0)),
            "mark" => style.background = Some([255, 235, 120]),
            "a" => {
                if let Some(href) = attr(attrs, "href") {
                    let href: String = href.trim().chars().filter(|c| !c.is_control()).collect();
                    if !href.is_empty() && !href.starts_with('#') {
                        style.link = Some(Arc::from(href));
                        // A link has the link colour unless a colour is set on the link itself,
                        // and is underlined unless its style says `text-decoration: none`, on the
                        // link or on the text inside it, as Outlook draws Word's signatures.
                        style.color = None;
                        style.underline = true;
                    }
                }
            }
            "font" => {
                if let Some(c) = attr(attrs, "color").and_then(color) {
                    style.color = Some(c);
                }
                if let Some(size) = attr(attrs, "size").and_then(font_size_attr) {
                    style.size = Some(size);
                }
                if let Some(face) = attr(attrs, "face") {
                    if is_mono_family(face) {
                        style.mono = true;
                    }
                    if let Some(family) = family(face) {
                        style.family = Some(family);
                    }
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = name.as_bytes().get(1).map_or(1, |b| b.saturating_sub(b'0')).clamp(1, 6);
                self.fmt.heading = level;
                style.bold = true;
                style.size = Some(BASE_PT * [2.0, 1.5, 1.17, 1.0, 0.83, 0.67].get(usize::from(level) - 1).copied().unwrap_or(1.0));
            }
            "center" => self.fmt.align = Align::Center,
            // A quotation of earlier mail is marked as one (a bar); any other is only set in.
            "blockquote" => {
                let has = |name: &str, what: &str| attr(attrs, name).is_some_and(|v| v.to_ascii_lowercase().contains(what));
                if has("type", "cite") || has("class", "quote") || self.sheet.declarations(name, attrs).to_ascii_lowercase().contains("border-left") {
                    self.fmt.quote = self.fmt.quote.saturating_add(1).min(8);
                } else {
                    self.fmt.indent = self.fmt.indent.saturating_add(1).min(12);
                }
            }
            "dd" => self.fmt.indent = self.fmt.indent.saturating_add(1).min(12),
            "pre" | "xmp" | "listing" | "plaintext" => {
                self.fmt.pre = true;
                style.mono = true;
            }
            "ul" | "ol" | "menu" | "dir" => {
                let start = attr(attrs, "start").and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(1);
                gap = self.lists.is_empty();
                self.lists.push((name == "ol", start.saturating_sub(1)));
                self.fmt.indent = self.fmt.indent.saturating_add(1).min(12);
                if let Some(o) = self.open.last_mut() {
                    o.list = true;
                }
            }
            "li" => {
                self.marker = Some(match self.lists.last_mut() {
                    Some((true, n)) => {
                        *n = n.saturating_add(1);
                        format!("{n}.")
                    }
                    _ => "•".to_string(),
                });
            }
            "body" => {
                if let Some(c) = attr(attrs, "text").and_then(color) {
                    style.color = Some(c);
                }
            }
            _ => {}
        }
        let margin = self.common(name, attrs);
        // A border on a block is drawn round the paragraphs in it. On a quotation, the line at
        // its left is the quotation's bar.
        let edges = self.css_border.take().filter(any_edge).filter(|_| name != "blockquote");
        let fill = self.css_fill.take();
        let padding = self.css_padding.take().filter(|p| p.iter().any(|v| *v > 0));
        let radius = std::mem::take(&mut self.css_radius);
        if matches!(name, "body" | "html") {
            // A background on the body is behind all of the message (CSS 2.1 section 14.2),
            // and is not a highlight of each of its words. White is the page itself.
            if let Some(c) = fill {
                self.fmt.style.background = None;
                if self.tables.is_empty() {
                    self.doc.fill = page_or(c);
                }
            }
        } else if block {
            // A background on a block is behind all of it (CSS 2.1 section 14.2), and is not a
            // highlight of its text. On a colour, white is a colour like any other.
            let within = self.fmt.frame.clone();
            let behind = within.as_ref().and_then(|f| f.behind()).or(self.doc.fill);
            if fill.is_some() {
                self.fmt.style.background = None;
            }
            let fill = fill.and_then(|c| if behind.is_some() { Some(c) } else { page_or(c) });
            // A box stands inside the box round it, as many deep as are ever drawn; one
            // deeper than that is not a box of its own.
            let deep = within.as_ref().map_or(0, |f| f.depth() + 1);
            if (edges.is_some() || fill.is_some() || padding.is_some()) && deep < MAX_BOXES {
                self.fmt.frame = Some(Arc::new(Frame {
                    edges: edges.unwrap_or_default(),
                    padding: padding.unwrap_or_default(),
                    indent: 0,
                    between: Edge::default(),
                    fill,
                    within,
                    radius,
                }));
            }
        } else if let Some(edge) = edges.as_ref().and_then(crate::rich::one_edge) {
            // A border on text within a line (CSS 2.1 section 8.5) is drawn round that text.
            // One that is not the same on all four sides is not drawn: a line under text is
            // what `text-decoration` is for.
            self.fmt.style.edge = Some(edge);
        }
        // Word-processor paragraphs ("MsoNormal" and friends) are written with no margins.
        if margin == Some(false) || (name == "p" && attr(attrs, "class").is_some_and(|c| c.trim_start().to_ascii_lowercase().starts_with("mso"))) {
            gap = false;
        }
        if gap {
            self.gap = true;
        }
        let mut turn = turning(name, block, attrs, &self.sheet.declarations(name, attrs), self.fmt.rtl);
        // The words of a link are read as they are written: nothing turns their letters round
        // one by one, so a link cannot be made to show another name than the one it has. What
        // is set apart starts with nothing turned (UAX #9, X5a to X5c).
        if self.fmt.style.link.is_some() {
            let floor = self.floor();
            let turned = self.overridden || self.open.iter().skip(floor).any(|o| o.turn.is_some_and(|(opener, _)| OVERRIDES.contains(&opener)));
            turn = match turn {
                Some((opener, _)) if OVERRIDES.contains(&opener) => None,
                None if name == "a" && turned => Some(('\u{2068}', '\u{2069}')),
                turn => turn,
            };
        }
        if let Some(o) = self.open.last_mut() {
            o.gap = gap;
            o.turn = turn;
        }
    }

    /// Attributes any element may carry. Returns whether the style asks for a margin.
    fn common(&mut self, name: &str, attrs: &[(String, String)]) -> Option<bool> {
        // The direction first: what it does to the side text stands at, `align` may undo.
        if let Some(dir) = attr(attrs, "dir") {
            self.direction(dir);
        }
        if let Some(a) = attr(attrs, "align").and_then(|v| self.align(v))
            && name != "table"
            && name != "img"
        {
            self.fmt.align = a;
        }
        self.css_fill = None;
        if let Some(c) = attr(attrs, "bgcolor").and_then(color) {
            self.fmt.style.background = page_or(c);
            self.css_fill = Some(c);
        }
        if attr(attrs, "background").is_some_and(is_remote) {
            self.doc.remote_blocked += 1;
        }
        if attr(attrs, "hidden").is_some() {
            self.fmt.hidden = true;
        }
        self.css_border = None;
        self.css_padding = None;
        self.css_radius = 0;
        let css = self.sheet.declarations(name, attrs);
        if css.is_empty() { None } else { self.css(&css) }
    }

    /// `dir` and CSS `direction` (HTML 3.2.6.4, CSS Writing Modes 3, 2.1). Text that is not told a
    /// side stands at the start of its direction.
    fn direction(&mut self, dir: &str) {
        let was = self.fmt.rtl;
        match dir.trim().to_ascii_lowercase().as_str() {
            "rtl" => (self.fmt.rtl, self.fmt.by_text) = (true, false),
            "ltr" => (self.fmt.rtl, self.fmt.by_text) = (false, false),
            "auto" => self.fmt.by_text = true,
            _ => return,
        }
        match (was, self.fmt.rtl, self.fmt.align) {
            (false, true, Align::Left) => self.fmt.align = Align::Right,
            (true, false, Align::Right) => self.fmt.align = Align::Left,
            _ => {}
        }
    }

    fn align(&self, v: &str) -> Option<Align> {
        align(v).map(|a| sided(a, &v.to_ascii_lowercase(), self.fmt.rtl))
    }

    /// Style declarations, in the order they apply (CSS 2.1 and CSS Text Decoration property names).
    fn css(&mut self, css: &str) -> Option<bool> {
        // The direction first, wherever it is written: `text-align` beside it has the last word.
        if let Some(dir) = css_value(css, "direction") {
            self.direction(&dir);
        }
        let mut zero_top = None;
        let mut zero_bottom = None;
        let mut edges: Option<Edges> = None;
        for decl in css.split(';').take(MAX_DECLARATIONS) {
            let Some((prop, value)) = decl.split_once(':') else { continue };
            let prop = prop.trim().to_ascii_lowercase();
            let value = value.trim().to_ascii_lowercase();
            let v = value.trim_end_matches("!important").trim();
            let style = &mut self.fmt.style;
            match prop.as_str() {
                "color" => {
                    if let Some(c) = color(v) {
                        style.color = Some(c);
                    }
                }
                "background-color" | "background" => {
                    if v.contains("url(") && is_remote(v.split("url(").nth(1).unwrap_or("").trim_start_matches(['"', '\'', ' '])) {
                        self.doc.remote_blocked += 1;
                    }
                    if let Some(c) = color(v).or_else(|| v.split_whitespace().find_map(color)) {
                        style.background = page_or(c);
                        self.css_fill = Some(c);
                    }
                }
                "font-weight" => match v {
                    "bold" | "bolder" => style.bold = true,
                    "normal" | "lighter" => style.bold = false,
                    _ => {
                        if let Ok(n) = v.parse::<u32>() {
                            style.bold = n >= 600;
                        }
                    }
                },
                "font-style" => style.italic = v.starts_with("italic") || v.starts_with("oblique"),
                "text-decoration" | "text-decoration-line" => {
                    if v.contains("none") {
                        style.underline = false;
                        style.strike = false;
                    }
                    if v.contains("underline") {
                        style.underline = true;
                    }
                    if v.contains("line-through") {
                        style.strike = true;
                    }
                }
                "font-size" => match font_size(v, style.size.unwrap_or(BASE_PT)) {
                    Some(pt) if pt < 1.0 => self.fmt.hidden = true,
                    Some(pt) => style.size = Some(pt.clamp(6.0, 72.0)),
                    None => {}
                },
                "font-family" => {
                    if is_mono_family(v) {
                        style.mono = true;
                    }
                    if let Some(family) = family(v) {
                        style.family = Some(family);
                    }
                }
                "text-align" => {
                    let rtl = self.fmt.rtl;
                    if let Some(a) = align(v).map(|a| sided(a, v, rtl)) {
                        self.fmt.align = a;
                    }
                }
                "line-height" => {
                    if let Some(leading) = line_height(v) {
                        self.fmt.leading = leading;
                    }
                }
                "display" if v.starts_with("none") => self.fmt.hidden = true,
                "visibility" if v.starts_with("hidden") => self.fmt.hidden = true,
                "mso-hide" if v.starts_with("all") => self.fmt.hidden = true,
                "white-space" => self.fmt.pre = v.starts_with("pre"),
                "margin" => {
                    let parts: Vec<&str> = v.split_whitespace().take(4).collect();
                    zero_top = parts.first().map(|p| is_zero(p));
                    zero_bottom = parts.get(2).or(parts.first()).map(|p| is_zero(p));
                }
                "margin-top" => zero_top = Some(is_zero(v)),
                "margin-bottom" => zero_bottom = Some(is_zero(v)),
                // Borders (CSS 2.1 section 8.5): all four sides, or one of them.
                "border" => edges = Some([edge(v); 4]),
                "border-top" | "border-right" | "border-bottom" | "border-left" => {
                    let side = ["border-top", "border-right", "border-bottom", "border-left"].iter().position(|p| *p == prop).unwrap_or(0);
                    if let Some(e) = edges.get_or_insert_with(Edges::default).get_mut(side) {
                        *e = edge(v);
                    }
                }
                // Rounded corners (CSS Backgrounds and Borders 3, section 5.1): the first length
                // is taken for all four; a share of the box's size is not.
                "border-radius" => self.css_radius = v.split_whitespace().next().and_then(pad_length).unwrap_or(0).min(MAX_RADIUS),
                // Padding (CSS 2.1 section 8.4): all four sides, or one of them.
                "padding" => self.css_padding = block_padding(v),
                "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
                    let side = ["padding-top", "padding-right", "padding-bottom", "padding-left"].iter().position(|p| *p == prop).unwrap_or(0);
                    if let (Some(room), Some(slot)) = (pad_length(v), self.css_padding.get_or_insert_with(Default::default).get_mut(side)) {
                        *slot = room;
                    }
                }
                "clear" if matches!(v, "both" | "left" | "right" | "all") => {
                    self.end_para();
                    self.clear = true;
                }
                _ => {}
            }
        }
        if edges.is_some() {
            self.css_border = edges;
        }
        match (zero_top, zero_bottom) {
            (Some(true), Some(true)) => Some(false),
            (None, None) => None,
            _ => Some(true),
        }
    }

    fn close_tag(&mut self, name: &str) {
        match name {
            "table" => {
                if self.tables.is_empty() {
                    self.end_para();
                } else {
                    self.close_table();
                }
                return;
            }
            "tr" | "td" | "th" => {
                if self.tables.is_empty() {
                    self.end_para();
                } else {
                    self.close_cell();
                }
                if name == "tr"
                    && let Some(t) = self.tables.last_mut()
                {
                    t.row_fill = None;
                    t.row_valign = None;
                }
                return;
            }
            "tbody" | "thead" | "tfoot" | "caption" | "colgroup" => return,
            // A stray </br> stands for a line break (HTML 13.2.6.4.7).
            "br" => return self.line_break(),
            _ => {}
        }
        let floor = self.floor();
        let Some(i) = self.open.iter().rposition(|o| o.name == name).filter(|i| *i >= floor) else {
            if BLOCK.contains(&name) {
                self.end_para();
            }
            return;
        };
        let block = BLOCK.contains(&name);
        if block {
            self.end_para();
        }
        let gap = self.open.get(i).is_some_and(|o| o.gap);
        self.pop_to(i);
        if gap {
            self.gap = true;
        }
    }

    /// Close every element from index `len` up, restoring what each one changed.
    fn pop_to(&mut self, len: usize) {
        while self.open.len() > len {
            let Some(o) = self.open.pop() else { break };
            if o.list {
                self.lists.pop();
            }
            // Only in the paragraph it was begun in: the end of a paragraph ends whatever was
            // begun in it (UAX #9, X8).
            if let Some((_, closer)) = o.turn
                && o.said == self.para_no
                && self.head.is_some()
            {
                self.append(closer.to_string(), None);
            }
            self.fmt = o.saved;
        }
    }

    fn open_table(&mut self, attrs: &[(String, String)]) {
        self.end_para();
        if self.tables.len() >= MAX_TABLE_DEPTH || self.open.len() >= MAX_DEPTH {
            return;
        }
        self.ensure_cell();
        self.open.push(Open { name: "table".to_string(), saved: self.fmt.clone(), list: false, gap: false, turn: None, said: 0 });
        // Alignment of the table itself is not inherited by its text, which stands at the side
        // it is read from. The table stands at the side the text around it is read from.
        let around = self.fmt.rtl;
        // A table in a centred cell or block stands in its middle, as a word there would.
        let centred = self.fmt.align == Align::Center;
        self.fmt.align = if around { Align::Right } else { Align::Left };
        let thick = attr(attrs, "border").and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok()).filter(|n| n.is_finite() && *n > 0.0);
        let border = thick.is_some();
        // What stands outside the table does not put its border round the table's paragraphs:
        // the table as a whole stands inside it.
        let inside = self.fmt.frame.take();
        self.common("table", attrs);
        // Its background is behind its cells, not a highlight of the words in them.
        let fill = self.css_fill.take();
        if fill.is_some() {
            self.fmt.style.background = None;
        }
        let radius = std::mem::take(&mut self.css_radius);
        let line = attr(attrs, "bordercolor").and_then(color);
        // The table's own border: its style's, or as thick as `border` says.
        let frame = self.css_border.take().filter(any_edge).unwrap_or_else(|| {
            let width = thick.map_or(0, |n| n.round().clamp(1.0, MAX_EDGE) as u8);
            [Edge { width, color: line, line: Line::Solid }; 4]
        });
        let css = self.sheet.declarations("table", attrs);
        let auto = |prop: &str| css_value(&css, prop).is_some_and(|v| v.split_whitespace().any(|part| part == "auto"));
        let room = |name: &str, unset: u16| attr(attrs, name).and_then(cell_room).unwrap_or(unset);
        // Collapsed borders leave no room between cells (CSS 2.1 section 17.6.2).
        let collapsed = css_value(&css, "border-collapse").is_some_and(|v| v.starts_with("collapse"));
        let table_align = match attr(attrs, "align").and_then(align) {
            Some(a) => a,
            None if auto("margin") || (auto("margin-left") && auto("margin-right")) => Align::Center,
            None if centred => Align::Center,
            None if around => Align::Right,
            None => Align::Left,
        };
        self.tables.push(TableBuild {
            rtl: self.fmt.rtl,
            rows: Vec::new(),
            border,
            width: table_width(attrs, &css),
            align: table_align,
            padding: room("cellpadding", CELL_PADDING),
            spacing: if collapsed { 0 } else { room("cellspacing", CELL_SPACING) },
            frame,
            line,
            inside,
            fill,
            radius,
            row_fill: None,
            row_valign: None,
            open_len: self.open.len(),
            cell: None,
        });
    }

    fn open_cell(&mut self, header: bool, span: u16) {
        self.end_para();
        let saved = self.fmt.clone();
        // A box round the table is not a box round each paragraph in its cells.
        self.fmt.frame = None;
        let open_len = self.open.len();
        let Some(t) = self.tables.last_mut() else { return };
        if t.rows.is_empty() {
            t.rows.push(Vec::new());
        }
        t.cell = Some(CellBuild {
            saved,
            open_len,
            header,
            span,
            background: None,
            width: 0,
            padding: None,
            share: 0,
            valign: VAlign::Middle,
            rows: 1,
            edges: None,
            radius: 0,
        });
        self.sinks.push(Vec::new());
        self.gap = false;
    }

    fn close_cell(&mut self) {
        let Some(cell) = self.tables.last_mut().and_then(|t| t.cell.take()) else { return };
        self.end_para();
        self.pop_to(cell.open_len);
        self.fmt = cell.saved;
        self.marker = None;
        self.gap = false;
        // The document's own list is never popped: a cell always has a sink above it.
        let blocks = if self.sinks.len() > 1 { self.sinks.pop().unwrap_or_default() } else { Vec::new() };
        if let Some(row) = self.tables.last_mut().and_then(|t| t.rows.last_mut()) {
            row.push(Cell {
                blocks,
                header: cell.header,
                span: cell.span,
                background: cell.background,
                width: cell.width,
                padding: cell.padding,
                share: cell.share,
                valign: cell.valign,
                rows: cell.rows,
                edges: cell.edges,
                radius: cell.radius,
            });
        }
    }

    fn close_table(&mut self) {
        self.close_cell();
        let Some(t) = self.tables.pop() else { return };
        self.pop_to(t.open_len.saturating_sub(1));
        // An empty row is dropped, unless cells that cover several rows count on it.
        let reaches = t.rows.iter().flatten().any(|c| c.rows > 1);
        let mut rows: Vec<Vec<Cell>> = t.rows.into_iter().filter(|r| reaches || r.iter().any(|c| !c.blocks.is_empty())).collect();
        if rows.iter().flatten().all(|c| c.blocks.is_empty()) {
            return;
        }
        if rows.is_empty() {
            return;
        }
        // A width, a place on the page, or a cell filled or padded is something to draw; without them
        // a table with one column is only layout, and its content reads as ordinary blocks.
        let shaped = matches!(t.width, TableWidth::Pixels(_) | TableWidth::Percent(_))
            || t.align != Align::Left
            || any_edge(&t.frame)
            || t.fill.is_some()
            || rows.iter().flatten().any(|c| c.background.is_some() || c.edges.is_some() || c.padding.is_some_and(|p| p.iter().any(|v| *v > 0)));
        if !shaped && rows.iter().all(|r| r.iter().filter(|c| !c.blocks.is_empty()).count() <= 1) {
            // What it held stands in the box the table stood in, where it has none of its own.
            for cell in rows.drain(..).flatten() {
                for mut block in cell.blocks {
                    match &mut block {
                        Block::Para(p) if p.frame.is_none() => p.frame = t.inside.clone(),
                        Block::Table(inner) if inner.inside.is_none() => inner.inside = t.inside.clone(),
                        _ => {}
                    }
                    self.push_block(block);
                }
            }
        } else {
            // A table has no margin of its own (CSS 2.1 section 17.4): one right under another
            // touches it, as a column of tables in a message is drawn.
            let snug = matches!(self.sinks.last().and_then(|s| s.last()), Some(Block::Table(_)));
            self.push_block(Block::Table(Table {
                rows,
                border: t.border,
                width: t.width,
                align: t.align,
                padding: t.padding,
                spacing: t.spacing,
                frame: t.frame,
                line: t.line,
                fill: t.fill,
                radius: t.radius,
                snug,
                repeat: false,
                rtl: t.rtl,
                inside: t.inside,
            }));
        }
        self.gap = true;
    }
}

/// Index of the `>` that ends a tag, skipping quoted attribute values.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = 0u8;
    let mut after_eq = false;
    for (i, b) in s.bytes().enumerate() {
        if quote != 0 {
            if b == quote {
                quote = 0;
            }
            continue;
        }
        match b {
            b'>' => return Some(i),
            b'"' | b'\'' if after_eq => quote = b,
            b'=' => {
                after_eq = true;
                continue;
            }
            b' ' | b'\t' | b'\n' | b'\r' => continue,
            _ => {}
        }
        after_eq = false;
    }
    s.find('>')
}

/// Attributes of a tag: lower-case names, entity-decoded values.
fn attributes(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    while out.len() < MAX_ATTRS {
        rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
        if rest.is_empty() {
            break;
        }
        let name_len = rest.find(|c: char| c.is_ascii_whitespace() || c == '=' || c == '/').unwrap_or(rest.len());
        let name = rest.get(..name_len).unwrap_or("").to_ascii_lowercase();
        rest = rest.get(name_len..).unwrap_or("").trim_start();
        let mut value = "";
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            match after.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let inner = after.get(1..).unwrap_or("");
                    let end = inner.find(q).unwrap_or(inner.len());
                    value = inner.get(..end).unwrap_or("");
                    rest = inner.get(end + 1..).unwrap_or("");
                }
                _ => {
                    let end = after.find(|c: char| c.is_ascii_whitespace()).unwrap_or(after.len());
                    value = after.get(..end).unwrap_or("");
                    rest = after.get(end..).unwrap_or("");
                }
            }
        }
        if name.is_empty() {
            // A stray "=" or quote: step over one character so the loop always moves on.
            let step = rest.chars().next().map_or(0, char::len_utf8);
            rest = rest.get(step..).unwrap_or("");
            continue;
        }
        // The address of a picture written into the body is the picture itself, so it may be long.
        let written = name == "src" && value.trim_start().get(..5).is_some_and(|scheme| scheme.eq_ignore_ascii_case("data:"));
        out.push((name, decode(value, if written { MAX_DATA_ADDRESS } else { MAX_ATTR_LEN })));
    }
    out
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// Entity-decode an attribute value, keeping at most `max` characters.
fn decode(value: &str, max: usize) -> String {
    let mut out = String::new();
    let mut rest = value;
    let mut count = 0;
    while let Some(c) = rest.chars().next() {
        if count >= max {
            break;
        }
        count += 1;
        if c == '&'
            && let Some((decoded, len)) = entity(rest)
        {
            out.push_str(&decoded);
            rest = rest.get(len..).unwrap_or("");
            continue;
        }
        out.push(c);
        rest = rest.get(c.len_utf8()..).unwrap_or("");
    }
    out
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h.get(i..i + n.len()).is_some_and(|w| w.eq_ignore_ascii_case(n)))
}

fn is_remote(url: &str) -> bool {
    let u: String = url.trim().chars().take(8).collect::<String>().to_ascii_lowercase();
    u.starts_with("http:") || u.starts_with("https:") || u.starts_with("//") || u.starts_with("ftp:")
}

/// The list of font names a body asks for, as it is kept: one line, of bounded length.
fn family(v: &str) -> Option<Arc<str>> {
    let v: String = v.chars().filter(|c| !c.is_control()).take(MAX_FAMILY_LEN).collect();
    let v = v.trim();
    (!v.is_empty() && !v.eq_ignore_ascii_case("inherit") && !v.eq_ignore_ascii_case("initial")).then(|| Arc::from(v))
}

/// Base64 (RFC 4648 section 4, either alphabet) to bytes. White space is passed over and the
/// data ends at the first `=`; anything else that is not base64 means there is no picture.
pub(crate) fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut held, mut bits) = (0u32, 0u32);
    for byte in text.bytes() {
        let six = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ if byte.is_ascii_whitespace() => continue,
            _ => return None,
        };
        held = ((held & 0xfff) << 6) | u32::from(six);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((held >> bits) & 0xff).unwrap_or(0));
        }
    }
    Some(out)
}

fn is_mono_family(v: &str) -> bool {
    let v = v.to_ascii_lowercase();
    ["monospace", "courier", "consolas", "menlo", "monaco", "lucida console"].iter().any(|m| v.contains(m))
}

fn is_zero(v: &str) -> bool {
    v.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%').parse::<f32>().is_ok_and(|n| n == 0.0)
}

/// `start` and `end` are sides of the way the text is written (CSS Text 3, 7.1).
/// The marks after which every letter runs one way, whatever it is (UAX #9, 2.2).
const OVERRIDES: [char; 2] = ['\u{202D}', '\u{202E}'];

/// The marks that stand for an element that says which way the text in it runs: the one that
/// begins it and the one that ends it. `bdo`, `bdi` and `dir` on an element inside a line are
/// what HTML says they are (HTML 15.3.5, "Bidirectional text": `[dir]` is `unicode-bidi:
/// isolate`, `bdo` `isolate-override`, `bdi` and `dir=auto` isolate by their own first letter),
/// and `unicode-bidi` is CSS Writing Modes 3, 2.2, whose values are defined by the characters
/// of UAX #9, 2.1 to 2.4, that are written here. `rtl` is the direction the element has come to.
fn turning(name: &str, block: bool, attrs: &[(String, String)], css: &str, rtl: bool) -> Option<(char, char)> {
    let dir = attr(attrs, "dir").map(|v| v.trim().to_ascii_lowercase());
    let bidi = css_value(css, "unicode-bidi").map(|v| v.trim().to_ascii_lowercase());
    let apart = |rtl: Option<bool>| {
        let opener = match rtl {
            Some(true) => '\u{2067}',
            Some(false) => '\u{2066}',
            // The way its own first letter is written.
            None => '\u{2068}',
        };
        Some((opener, '\u{2069}'))
    };
    let over = Some((if rtl { '\u{202E}' } else { '\u{202D}' }, '\u{202C}'));
    match bidi.as_deref() {
        // Every letter runs the way that is said, whatever it is.
        Some("bidi-override" | "isolate-override") => over,
        Some("normal") => None,
        // A block's own direction is its paragraphs': there is nothing to set apart.
        _ if block => None,
        Some("embed") => Some((if rtl { '\u{202B}' } else { '\u{202A}' }, '\u{202C}')),
        Some("plaintext") => apart(None),
        Some("isolate") => apart(Some(rtl)),
        _ if name == "bdo" => over,
        _ => match dir.as_deref() {
            Some("rtl") => apart(Some(true)),
            Some("ltr") => apart(Some(false)),
            Some("auto") => apart(None),
            // A name that may be in any writing is kept apart from what is around it.
            _ if name == "bdi" => apart(None),
            _ => None,
        },
    }
}

fn sided(a: Align, v: &str, rtl: bool) -> Align {
    match (v.trim(), rtl) {
        ("start", true) => Align::Right,
        ("end", true) => Align::Left,
        _ => a,
    }
}

fn align(v: &str) -> Option<Align> {
    match v.trim().to_ascii_lowercase().as_str() {
        "left" | "justify" | "start" => Some(Align::Left),
        "center" | "middle" => Some(Align::Center),
        "right" | "end" => Some(Align::Right),
        _ => None,
    }
}

/// A length in pixels, as an attribute or a CSS value gives it. A percentage is not one.
fn pixels(v: &str) -> Option<u16> {
    let n = v.trim().trim_end_matches("px").trim_end().parse::<f32>().ok().filter(|n| n.is_finite() && *n >= 1.0)?;
    Some(n.round().min(MAX_PICTURE_SIDE) as u16)
}

/// A share of the room in hundredths, 1 to 100, as `25%` gives it.
fn percent(v: &str) -> Option<u8> {
    let n = v.trim().strip_suffix('%')?.trim().parse::<f32>().ok().filter(|n| n.is_finite() && *n >= 1.0)?;
    Some(n.round().min(100.0) as u8)
}

fn valign(v: &str) -> Option<VAlign> {
    match v.trim().to_ascii_lowercase().as_str() {
        "top" | "baseline" | "text-top" => Some(VAlign::Top),
        "middle" | "center" => Some(VAlign::Middle),
        "bottom" | "text-bottom" => Some(VAlign::Bottom),
        _ => None,
    }
}

/// One side of a border as a style gives it: a width, a kind of line and a colour, in any order
/// (CSS 2.1 section 8.5.4). Without a kind of line there is no line.
fn edge(v: &str) -> Edge {
    let (mut width, mut line, mut colour) = (None, None, None);
    // A colour written as a function has spaces of its own.
    let mut rest = v.to_string();
    if let Some(start) = rest.find("rgb")
        && let Some(len) = rest.get(start..).and_then(|r| r.find(')'))
    {
        colour = rest.get(start..=start + len).and_then(color);
        rest.replace_range(start..=start + len, " ");
    }
    for part in rest.split_whitespace().take(8) {
        match part {
            "none" | "hidden" => line = Some(None),
            "solid" | "groove" | "ridge" | "inset" | "outset" => line = Some(Some(Line::Solid)),
            "dashed" => line = Some(Some(Line::Dashed)),
            "dotted" => line = Some(Some(Line::Dotted)),
            "double" => line = Some(Some(Line::Double)),
            "thin" => width = Some(1.0),
            "medium" => width = Some(3.0),
            "thick" => width = Some(5.0),
            _ => {
                let (number, scale) = match part.strip_suffix("pt") {
                    Some(points) => (points, 4.0 / 3.0),
                    None => (part.trim_end_matches("px"), 1.0),
                };
                match number.parse::<f32>().ok().filter(|n| n.is_finite() && *n >= 0.0) {
                    Some(n) => width = Some(n * scale),
                    None => colour = colour.or_else(|| color(part)),
                }
            }
        }
    }
    let Some(Some(line)) = line else { return Edge::default() };
    let width = width.unwrap_or(3.0);
    // A line thinner than a pixel is still a line.
    let width = if width <= 0.0 { 0 } else { width.round().clamp(1.0, MAX_EDGE) as u8 };
    Edge { width, color: colour, line }
}

/// Cell padding or spacing as a table's attribute gives it, in pixels.
fn cell_room(v: &str) -> Option<u16> {
    let n = v.trim().trim_end_matches("px").trim_end().parse::<f32>().ok().filter(|n| n.is_finite() && *n >= 0.0)?;
    Some(n.round().min(MAX_CELL_ROOM) as u16)
}

/// One length of a `padding`, in pixels.
/// Every absolute unit is read (CSS 2.1 section 4.3.2): Word writes `0cm 5.4pt`, and one
/// length not understood would lose all four. A length without a unit is only allowed as 0,
/// but is read as pixels, as browsers do in mail.
fn pad_length(part: &str) -> Option<u16> {
    let split = part.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(part.len());
    let n = part.get(..split)?.parse::<f32>().ok().filter(|n| n.is_finite() && *n >= 0.0)?;
    let scale = match part.get(split..)?.to_ascii_lowercase().as_str() {
        "px" | "" => 1.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        "q" => 96.0 / 101.6,
        _ => return None,
    };
    let px = n * scale;
    px.is_finite().then(|| px.round().min(MAX_CELL_ROOM) as u16)
}

/// A block's `padding`: above, right, below, left, from one to four lengths (CSS 2.1 section
/// 8.4). Lengths that depend on the text size, and percentages, are not understood.
fn block_padding(v: &str) -> Option<[u16; 4]> {
    let mut given = [0u16; 4];
    let mut count = 0usize;
    for part in v.split_whitespace().take(5) {
        // A fifth length makes it no padding at all.
        *given.get_mut(count)? = pad_length(part)?;
        count += 1;
    }
    let [a, b, c, d] = given;
    match count {
        1 => Some([a; 4]),
        2 => Some([a, b, a, b]),
        3 => Some([a, b, c, b]),
        4 => Some([a, b, c, d]),
        _ => None,
    }
}

/// The value a list of declarations ends up giving one property, lower case.
fn css_value(css: &str, prop: &str) -> Option<String> {
    let (_, value) =
        css.split(';').take(MAX_DECLARATIONS).filter_map(|d| d.split_once(':')).filter(|(p, _)| p.trim().eq_ignore_ascii_case(prop)).last()?;
    Some(value.to_ascii_lowercase().trim().trim_end_matches("!important").trim().to_string())
}

/// Declarations of one element that are read: its rules' and its own.
const MAX_DECLARATIONS: usize = 256;

/// The rules of the message's style sheets that pick elements by name, class or id.
#[derive(Default)]
struct Sheet {
    rules: Vec<Rule>,
    /// Rule numbers by what picks them out: the id, else the class, else the element name.
    by_id: HashMap<String, Vec<usize>>,
    by_class: HashMap<String, Vec<usize>>,
    by_tag: HashMap<String, Vec<usize>>,
    bytes: usize,
}

/// One selector of a rule; empty parts match anything.
struct Rule {
    tag: String,
    class: String,
    id: String,
    declarations: Arc<str>,
}

impl Sheet {
    /// Read the text of one `<style>` element (CSS 2.1 section 4.1, syntax).
    fn read(&mut self, css: &str) {
        let mut end = css.len().min(MAX_SHEET.saturating_sub(self.bytes));
        while end > 0 && !css.is_char_boundary(end) {
            end -= 1;
        }
        self.bytes = self.bytes.saturating_add(end);
        // Sheets in mail are often wrapped in an HTML comment.
        let css = css.get(..end).unwrap_or("").replace("<!--", " ").replace("-->", " ");
        let mut clean = String::with_capacity(css.len());
        let mut rest = css.as_str();
        while let Some(i) = rest.find("/*") {
            clean.push_str(rest.get(..i).unwrap_or(""));
            rest = rest.get(i + 2..).and_then(|r| r.find("*/").and_then(|j| r.get(j + 2..))).unwrap_or("");
        }
        clean.push_str(rest);

        let mut rest = clean.as_str();
        while self.rules.len() < MAX_RULES {
            let Some(open) = rest.find('{') else { break };
            let prelude = rest.get(..open).unwrap_or("");
            // An at-rule without a block (`@import ...;`) ends at its semicolon.
            let prelude = prelude.rfind(';').and_then(|i| prelude.get(i + 1..)).unwrap_or(prelude).trim();
            let after = rest.get(open + 1..).unwrap_or("");
            if prelude.starts_with('@') {
                // `@media`, `@font-face` and the like: skipped with everything inside.
                let mut depth = 1usize;
                let close = after.bytes().position(|b| {
                    match b {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    depth == 0
                });
                rest = close.and_then(|i| after.get(i + 1..)).unwrap_or("");
                continue;
            }
            let close = after.find('}').unwrap_or(after.len());
            let declarations: Arc<str> = Arc::from(after.get(..close).unwrap_or("").chars().take(MAX_ATTR_LEN).collect::<String>());
            rest = after.get(close + 1..).unwrap_or("");
            for selector in prelude.split(',').take(32) {
                self.add(selector, &declarations);
            }
        }
    }

    /// Keep one selector if it is an element name, a class, an id, or a name with one of each.
    fn add(&mut self, selector: &str, declarations: &Arc<str>) {
        let selector = selector.trim();
        // A link that was not followed is every link here; other states never apply.
        let (simple, state) = selector.split_once(':').unwrap_or((selector, ""));
        if !(state.is_empty() || state.eq_ignore_ascii_case("link")) || self.rules.len() >= MAX_RULES {
            return;
        }
        if simple.is_empty() || !simple.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '#' | '-' | '_')) {
            return;
        }
        let tag_len = simple.find(['.', '#']).unwrap_or(simple.len());
        let tag = simple.get(..tag_len).unwrap_or("").to_ascii_lowercase();
        let (mut class, mut id) = (String::new(), String::new());
        let mut marks = simple.get(tag_len..).unwrap_or("");
        while let Some(kind) = marks.chars().next() {
            let body = marks.get(1..).unwrap_or("");
            let len = body.find(['.', '#']).unwrap_or(body.len());
            let slot = if kind == '.' { &mut class } else { &mut id };
            if len == 0 || !slot.is_empty() {
                return;
            }
            *slot = body.get(..len).unwrap_or("").to_ascii_lowercase();
            marks = body.get(len..).unwrap_or("");
        }
        let index = self.rules.len();
        let (map, key) = if !id.is_empty() {
            (&mut self.by_id, id.clone())
        } else if !class.is_empty() {
            (&mut self.by_class, class.clone())
        } else {
            (&mut self.by_tag, tag.clone())
        };
        map.entry(key).or_default().push(index);
        self.rules.push(Rule { tag, class, id, declarations: declarations.clone() });
    }

    /// Every declaration that applies to an element, in the order it applies: rules by element
    /// name, then by class, then by id, each in the order written, then the element's own `style`
    /// (CSS 2.1 section 6.4.3, specificity).
    fn declarations(&self, name: &str, attrs: &[(String, String)]) -> String {
        let own = attr(attrs, "style").unwrap_or("");
        if self.rules.is_empty() {
            return own.to_string();
        }
        let id = attr(attrs, "id").map(|v| v.trim().to_ascii_lowercase()).unwrap_or_default();
        // Only the first class an element names picks rules: that is what Outlook Classic was seen to do.
        let class = attr(attrs, "class").unwrap_or("").split_ascii_whitespace().next().unwrap_or("").to_ascii_lowercase();
        let lists = [self.by_tag.get(name), self.by_id.get(&id).filter(|_| !id.is_empty()), self.by_class.get(&class).filter(|_| !class.is_empty())];
        let mut found: Vec<(u8, usize)> = Vec::new();
        for &index in lists.into_iter().flatten().flat_map(|list| list.iter().take(MAX_MATCHES)) {
            let Some(rule) = self.rules.get(index) else { continue };
            if (rule.tag.is_empty() || rule.tag == name) && (rule.class.is_empty() || rule.class == class) {
                let weight = u8::from(!rule.id.is_empty()) * 4 + u8::from(!rule.class.is_empty()) * 2 + u8::from(!rule.tag.is_empty());
                found.push((weight, index));
            }
        }
        found.sort_unstable();
        found.dedup();
        let mut all = String::new();
        for (_, index) in found.into_iter().take(MAX_MATCHES) {
            if let Some(rule) = self.rules.get(index) {
                all.push_str(&rule.declarations);
                all.push(';');
            }
        }
        all.push_str(own);
        all
    }
}

/// The width a table asks for: a limit in pixels first, then its width.
fn table_width(attrs: &[(String, String)], css: &str) -> TableWidth {
    let asked = [css_value(css, "max-width"), css_value(css, "width"), attr(attrs, "width").map(str::to_string)];
    for v in asked.into_iter().flatten() {
        match percent(&v) {
            Some(100) => return TableWidth::Fill,
            Some(share) => return TableWidth::Percent(share),
            None if v.trim().ends_with('%') => {}
            None => {
                if let Some(px) = pixels(&v) {
                    return TableWidth::Pixels(px);
                }
            }
        }
    }
    TableWidth::Content
}

/// A near-white background is the page itself, not a highlight.
fn page_or(c: Rgb) -> Option<Rgb> {
    let [r, g, b] = c;
    let luma = (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000;
    (luma < 236).then_some(c)
}

/// A CSS font size in points. Zero means the text is hidden.
/// A CSS `line-height` (CSS 2.1 section 10.8.1): a number or a percentage multiplies the
/// lines' own height, a length is the height itself.
fn line_height(v: &str) -> Option<Leading> {
    let v = v.trim();
    if v == "normal" {
        return Some(Leading::Own);
    }
    let split = v.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(v.len());
    let n = v.get(..split)?.parse::<f32>().ok()?;
    if !n.is_finite() || n <= 0.0 {
        return None;
    }
    // A line of text is about 1.15 times its size tall: that is what "1" would cut into.
    const OWN: f32 = 1.15;
    // `as` saturates, and the numbers are clamped where they are used.
    let times = |of_size: f32| Leading::Times((of_size / OWN * 100.0).round().clamp(f32::from(LEADING_TIMES.0), f32::from(LEADING_TIMES.1)) as u16);
    let exactly = |px: f32| Leading::Exactly(px.round().clamp(1.0, f32::from(LEADING_PX)) as u16);
    Some(match v.get(split..).unwrap_or("").trim() {
        "" => times(n),
        "%" => times(n / 100.0),
        "em" | "rem" => times(n),
        "px" => exactly(n),
        "pt" => exactly(n * 4.0 / 3.0),
        "pc" => exactly(n * 16.0),
        "in" => exactly(n * 96.0),
        "cm" => exactly(n * 37.8),
        "mm" => exactly(n * 3.78),
        _ => return None,
    })
}

fn font_size(v: &str, current: f32) -> Option<f32> {
    let v = v.trim();
    let keyword = match v {
        "xx-small" => Some(6.75),
        "x-small" => Some(7.5),
        "small" => Some(9.75),
        "medium" => Some(12.0),
        "large" => Some(13.5),
        "x-large" => Some(18.0),
        "xx-large" => Some(24.0),
        "smaller" => Some(current * 0.83),
        "larger" => Some(current * 1.2),
        _ => None,
    };
    if keyword.is_some() {
        return keyword;
    }
    let split = v.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+')).unwrap_or(v.len());
    let n = v.get(..split)?.parse::<f32>().ok()?;
    if !n.is_finite() || n < 0.0 {
        return None;
    }
    let pt = match v.get(split..).unwrap_or("").trim() {
        "pt" => n,
        "px" | "" => n * 0.75,
        "em" | "rem" => n * current,
        "%" => n / 100.0 * current,
        "pc" => n * 12.0,
        "in" => n * 72.0,
        "cm" => n * 28.35,
        "mm" => n * 2.835,
        _ => return None,
    };
    pt.is_finite().then_some(pt)
}

/// `<font size>`: 1 to 7, or relative to 3.
fn font_size_attr(v: &str) -> Option<f32> {
    let v = v.trim();
    let n = v.parse::<i32>().ok()?;
    let step = if v.starts_with(['+', '-']) { n.saturating_add(3) } else { n }.clamp(1, 7);
    [7.5, 10.0, 12.0, 13.5, 18.0, 24.0, 36.0].get(usize::try_from(step - 1).ok()?).copied()
}

/// A CSS colour: `#rgb`, `#rrggbb`, `rgb()`/`rgba()` or a common name.
fn color(v: &str) -> Option<Rgb> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(hex) = v.strip_prefix('#') {
        if !hex.is_ascii() {
            return None;
        }
        let nibble = |i: usize| hex.get(i..i + 1).and_then(|d| u8::from_str_radix(d, 16).ok());
        let byte = |i: usize| hex.get(i..i + 2).and_then(|d| u8::from_str_radix(d, 16).ok());
        return match hex.len() {
            3 | 4 => Some([nibble(0)? * 17, nibble(1)? * 17, nibble(2)? * 17]),
            6 | 8 => Some([byte(0)?, byte(2)?, byte(4)?]),
            _ => None,
        };
    }
    if let Some(args) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")) {
        let mut parts = args.trim_end_matches(')').split(|c: char| c == ',' || c == '/' || c.is_whitespace()).filter(|p| !p.is_empty());
        let mut channel = || {
            let p = parts.next()?;
            let n = match p.strip_suffix('%') {
                Some(pc) => pc.parse::<f32>().ok()? * 2.55,
                None => p.parse::<f32>().ok()?,
            };
            // `as` saturates, and NaN becomes 0.
            n.is_finite().then_some(n.clamp(0.0, 255.0) as u8)
        };
        return Some([channel()?, channel()?, channel()?]);
    }
    Some(match v.as_str() {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "blue" => [0, 0, 255],
        "gray" | "grey" => [128, 128, 128],
        "darkgray" | "darkgrey" => [169, 169, 169],
        "lightgray" | "lightgrey" => [211, 211, 211],
        "dimgray" | "dimgrey" => [105, 105, 105],
        "silver" => [192, 192, 192],
        "maroon" => [128, 0, 0],
        "navy" => [0, 0, 128],
        "purple" => [128, 0, 128],
        "teal" => [0, 128, 128],
        "olive" => [128, 128, 0],
        "lime" => [0, 255, 0],
        "aqua" | "cyan" => [0, 255, 255],
        "fuchsia" | "magenta" => [255, 0, 255],
        "yellow" => [255, 255, 0],
        "orange" => [255, 165, 0],
        "pink" => [255, 192, 203],
        "brown" => [165, 42, 42],
        "gold" => [255, 215, 0],
        "darkred" => [139, 0, 0],
        "darkgreen" => [0, 100, 0],
        "darkblue" => [0, 0, 139],
        "steelblue" => [70, 130, 180],
        "royalblue" => [65, 105, 225],
        "crimson" => [220, 20, 60],
        "tomato" => [255, 99, 71],
        "indigo" => [75, 0, 130],
        "violet" => [238, 130, 238],
        _ => return None,
    })
}
