//! Memo Style: the owner's name over a thick line, the message's fields as a list of names and
//! values, then the body, and each page's number at its foot, of how many.

use std::sync::Arc;

use openmailclient_formats::rich::{Block, Cell, Doc, Edge, Frame, Line, Para, Picture, Run, Style, Table, TableWidth};

use crate::flow::{Base, FOOT, Flow, HEAD};
use crate::fonts::Fonts;
use crate::{LayoutError, Page, Printed, Setup};

/// Fields a header shows, and characters of one field.
const MAX_FIELDS: usize = 32;
const MAX_FIELD_CHARS: usize = 20_000;
/// Width of the column of field names, in pixels.
const NAMES: u16 = 110;
/// Characters of one part of a header or footer, and names looked through for it.
const MAX_EDGE_CHARS: usize = 400;
const MAX_WORDS: usize = 32;

pub(crate) const HEAD_FONT: &str = "Segoe UI, Arial, Liberation Sans, sans-serif";
const BODY_FONT: &str = "Times New Roman, Liberation Serif, serif";
const PLAIN_FONT: &str = "Calibri, Carlito, Arial, sans-serif";

/// A message to print.
pub struct Memo<'a> {
    /// Whose mailbox it is printed from; heads the first page.
    pub owner: &'a str,
    /// Names and values, in the order they are listed. A value that is empty is left out.
    pub fields: &'a [(&'a str, String)],
    pub body: &'a Doc,
    /// The body is plain text, not a formatted one.
    pub plain: bool,
    /// The width and height in pixels of a picture that can be printed; `None` for one that
    /// cannot, whose place is then named in words.
    pub sizes: &'a dyn Fn(&Picture) -> Option<(u16, u16)>,
}

pub(crate) fn text(text: &str, bold: bool, size: Option<f32>) -> Para {
    let text: String = text.chars().filter(|c| !c.is_control()).take(MAX_FIELD_CHARS).collect();
    Para { runs: vec![Run { text, style: Style { bold, size, ..Style::default() }, ..Run::default() }], ..Para::default() }
}

/// The owner's name over a thick line: what heads the first page of a print.
pub(crate) fn owner(name: &str) -> Para {
    let line = Edge { width: 3, color: Some([0, 0, 0]), line: Line::Solid };
    let frame = Arc::new(Frame {
        edges: [Edge::default(), Edge::default(), line, Edge::default()],
        padding: [3, 0, 3, 0],
        indent: 0,
        between: Edge::default(),
        fill: None,
        within: None,
        radius: 0,
    });
    let mut owner = text(name, true, Some(12.0));
    owner.frame = Some(frame);
    owner
}

fn head(memo: &Memo) -> Vec<Block> {
    let owner = owner(memo.owner);
    let cell =
        |para: Para, width: u16| Cell { blocks: vec![Block::Para(para)], span: 1, rows: 1, width, padding: Some([1, 0, 1, 0]), ..Cell::default() };
    let rows: Vec<Vec<Cell>> = memo
        .fields
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .take(MAX_FIELDS)
        .map(|(name, value)| vec![cell(text(&format!("{name}:"), true, None), NAMES), cell(text(value, false, None), 0)])
        .collect();
    let mut blocks = vec![Block::Para(owner), Block::Para(Para::default())];
    if !rows.is_empty() {
        blocks.push(Block::Table(Table { rows, width: TableWidth::Fill, snug: true, ..Table::default() }));
        blocks.push(Block::Para(Para::default()));
    }
    blocks
}

/// A part of a header or footer with the names in it filled in, as [`Setup`] says.
fn filled(template: &str, page: usize, pages: usize, words: &[(String, String)]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some((before, after)) = rest.split_once('[') {
        out.push_str(before);
        let found = after.split_once(']').and_then(|(name, then)| {
            let name = name.trim();
            let value = if name.eq_ignore_ascii_case("page") {
                Some(page.saturating_add(1).to_string())
            } else if name.eq_ignore_ascii_case("pages") {
                Some(pages.to_string())
            } else if name.eq_ignore_ascii_case("page of pages") {
                // With how many there are, a loose page tells whether the rest is with it.
                Some(if pages > 1 { format!("{} of {pages}", page.saturating_add(1)) } else { page.saturating_add(1).to_string() })
            } else {
                words.iter().take(MAX_WORDS).find(|(word, _)| word.eq_ignore_ascii_case(name)).map(|(_, value)| value.clone())
            };
            value.map(|value| (value, then))
        });
        match found {
            Some((value, then)) => {
                out.extend(value.chars().take(MAX_EDGE_CHARS));
                rest = then;
            }
            // No name of ours: the bracket stands as it is.
            None => {
                out.push('[');
                rest = after;
            }
        }
        if out.len() > MAX_EDGE_CHARS * 4 {
            break;
        }
    }
    out.push_str(rest);
    out.chars().filter(|c| !c.is_control()).take(MAX_EDGE_CHARS).collect()
}

/// The header (`over`) or the footer of a page: three parts on one line, at the left, in the
/// middle and at the right. A part longer than its share of the line is cut.
fn edge(flow: &mut Flow, parts: &[String; 3], words: &[(String, String)], page: usize, pages: usize, over: bool) {
    const SIZE: f32 = 8.0;
    let y = if over { flow.margins[0] + (HEAD - SIZE) / 2.0 + SIZE * 0.8 } else { flow.paper.height - flow.margins[2] - (FOOT - SIZE) / 2.0 };
    let (left, width) = (flow.left(), flow.width());
    let asked = parts.iter().filter(|part| !part.trim().is_empty()).count().max(1);
    let share = width / asked as f32 - if asked > 1 { SIZE } else { 0.0 };
    for (n, part) in parts.iter().enumerate() {
        let words = filled(part, page, pages, words);
        flow.words(page, words.trim(), HEAD_FONT, SIZE, Some(share), y, |wide| match n {
            0 => left,
            1 => left + (width - wide) / 2.0,
            _ => left + width - wide,
        });
    }
}

/// Lay a message out in Memo Style, on pages set up as `setup` says: a [`Setup`], or a
/// [`crate::Paper`] for the sheet upright with its margin and each page's number at its foot.
pub fn memo(memo: &Memo, setup: impl Into<Setup>, fonts: &mut Fonts) -> Result<Printed, LayoutError> {
    let setup: Setup = setup.into();
    let mut flow = Flow::new(fonts, &setup, Base { family: HEAD_FONT.into(), size: 8.0 }, memo.sizes);
    let (x, width) = (flow.left(), flow.width());
    let start = flow.start();
    // The body alone, when the details are not asked for.
    let at = if setup.details { flow.blocks(&head(memo), x, width, start, 0) } else { start };
    flow.base = if memo.plain { Base { family: PLAIN_FONT.into(), size: 11.0 } } else { Base { family: BODY_FONT.into(), size: 12.0 } };
    let end = flow.blocks(&memo.body.blocks, x, width, at, 0);
    // A body that asks for a colour behind it has it behind all of it, on every page it is on:
    // text chosen to be read on that colour is not lost on the paper.
    if let Some(fill) = memo.body.fill {
        flow.behind(at, end, x, width, fill);
    }
    let (pages, full) = finish(flow, &setup)?;
    Ok(Printed { pages, faces: fonts.faces().to_vec(), truncated: full || memo.body.truncated })
}

/// Sheets laid out one after another, each starting on a page of its own: a day, a week or a
/// month of a calendar, each on its page. `sheets` is given the height of the room on a page
/// for the body, in pixels, so that what it makes can fill the page.
pub fn sheets(sheets: &dyn Fn(f32) -> Vec<Vec<Block>>, setup: impl Into<Setup>, fonts: &mut Fonts) -> Result<Printed, LayoutError> {
    let setup: Setup = setup.into();
    let none = |_: &Picture| None;
    let mut flow = Flow::new(fonts, &setup, Base { family: HEAD_FONT.into(), size: 9.0 }, &none);
    let (x, width) = (flow.left(), flow.width());
    let mut at = flow.start();
    // From points to pixels: the layout's units are points, and a cell's height is in pixels.
    let room = (flow.tall() * 96.0 / 72.0).max(0.0);
    for (n, sheet) in sheets(room).iter().enumerate() {
        if flow.full {
            break;
        }
        if n > 0 {
            at = flow.next_page(at);
        }
        at = flow.blocks(sheet, x, width, at, 0);
    }
    let (pages, full) = finish(flow, &setup)?;
    Ok(Printed { pages, faces: fonts.faces().to_vec(), truncated: full })
}

/// The pages of a flow that is set, each with its header and footer; and whether there was
/// more than may be printed.
pub(crate) fn finish(mut flow: Flow, setup: &Setup) -> Result<(Vec<Page>, bool), LayoutError> {
    if flow.no_font {
        return Err(LayoutError::NoFont);
    }
    let pages = flow.pages.len();
    for page in 0..pages {
        if flow.head > 0.0 {
            edge(&mut flow, &setup.header, &setup.words, page, pages, true);
        }
        if flow.foot > 0.0 {
            edge(&mut flow, &setup.footer, &setup.words, page, pages, false);
        }
    }
    let full = flow.full;
    let mut pages = std::mem::take(&mut flow.pages);
    for page in &mut pages {
        // A fill deeper in tables lies over a shallower one.
        page.fills.sort_by_key(|fill| fill.depth);
    }
    Ok((pages, full))
}
