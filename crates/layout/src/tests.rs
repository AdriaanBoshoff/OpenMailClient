use std::sync::Arc;

use openmailclient_fonts::{Face, Library, boxes};
use openmailclient_formats::rich::{Align, Block, Cell, Doc, Edge, Frame, Para, ParaKind, Picture, Run, Script, Style, Table, TableWidth, VAlign};

use super::*;

/// Every glyph is a box half as wide as the size, a space a quarter; a line is as tall as the size.
const CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789:[].- ";

/// A sheet 200 wide with 160 of room: 26 body characters to the line.
const SMALL: Paper = Paper { width: 200.0, height: 300.0, margin: 20.0 };

fn print_on(library: &Library, memo_: &Memo, paper: impl Into<Setup>) -> Result<Printed, LayoutError> {
    let mut fonts = Fonts::new(library);
    for family in ["Segoe UI", "Times New Roman", "Calibri"] {
        fonts.give(family, false, false, Face { data: boxes::font(CHARS), index: 0 });
    }
    memo(memo_, paper, &mut fonts)
}

fn print(body: &Doc, paper: Paper) -> Printed {
    print_on(&Library::empty(), &Memo { owner: "Owner", fields: &[], body, plain: false, sizes: &|_| None }, paper).unwrap()
}

fn run(text: &str) -> Run {
    Run { text: text.into(), ..Run::default() }
}

fn para(text: &str) -> Para {
    Para { runs: vec![run(text)], ..Para::default() }
}

fn doc(blocks: Vec<Block>) -> Doc {
    Doc { blocks, ..Doc::default() }
}

fn cell(text: &str) -> Cell {
    Cell { blocks: vec![Block::Para(para(text))], span: 1, rows: 1, ..Cell::default() }
}

/// The lines of a page without the owner's name at its head and the number at its foot.
fn body_lines(page: &Page) -> Vec<String> {
    let text = page.text();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines.pop();
    if lines.first().is_some_and(|l| l == "Owner") {
        lines.remove(0);
    }
    lines
}

fn within(printed: &Printed, paper: Paper) {
    for page in &printed.pages {
        for text in &page.texts {
            assert!(text.x >= paper.margin - 0.01 && text.x + text.width <= paper.width - paper.margin + 0.01, "{text:?}");
            assert!(text.y > paper.margin && text.y < paper.height - paper.margin + 0.01, "{text:?}");
            assert!(text.face < printed.faces.len());
        }
    }
}

#[test]
fn text_wraps_at_spaces_and_long_words_are_cut() {
    // A word of four is 24 wide and a space 3: six words fill 159 of the 160.
    let words = ["abcd"; 14].join(" ");
    let printed = print(&doc(vec![Block::Para(para(&words)), Block::Para(para(&"x".repeat(60)))]), SMALL);
    let lines = body_lines(&printed.pages[0]);
    let six = ["abcd"; 6].join(" ");
    assert_eq!(lines, [six.clone(), six, "abcd abcd".into(), "x".repeat(26), "x".repeat(26), "x".repeat(8)]);
    within(&printed, SMALL);
    // The lines stand a line apart.
    let ys: Vec<f32> = printed.pages[0].texts.iter().filter(|t| t.glyphs[0].1 == 'x').map(|t| t.y).collect();
    assert!((ys[1] - ys[0] - 12.0).abs() < 0.01 && (ys[2] - ys[1] - 12.0).abs() < 0.01, "{ys:?}");
}

#[test]
fn a_long_message_goes_on_to_more_pages_each_with_its_number() {
    let text: String = (0..100).map(|i| format!("line{i}\n")).collect();
    let body = Doc::plain(&text);
    let printed = print_on(
        &Library::empty(),
        &Memo { owner: "Owner", fields: &[("To", "someone".into()), ("Cc", " ".into())], body: &body, plain: true, sizes: &|_| None },
        SMALL,
    )
    .unwrap();
    assert!(printed.pages.len() > 3 && !printed.truncated);
    within(&printed, SMALL);
    let mut seen = Vec::new();
    for (n, page) in printed.pages.iter().enumerate() {
        assert_eq!((page.width, page.height), (200.0, 300.0));
        assert_eq!(page.text().lines().last().unwrap(), format!("{} of {}", n + 1, printed.pages.len()));
        // The number stands under everything else.
        let foot = page.texts.last().unwrap().y;
        assert!(page.texts.iter().filter(|t| t.y != foot).all(|t| t.y < foot - 8.0));
        seen.extend(body_lines(page));
    }
    // The field with a value is listed, the empty one is not.
    assert_eq!(seen[..2], ["To:someone".to_string(), "line0".into()]);
    assert_eq!(seen.len(), 101);
    assert_eq!(seen.last().unwrap(), "line99");
    // The owner's name stands over a thick line.
    assert!(printed.pages[0].rules.iter().any(|r| r.width > 2.0 && r.from.1 == r.to.1 && r.to.0 - r.from.0 > 150.0));
}

#[test]
fn looks_are_carried_onto_the_page() {
    let styled = |text: &str, style: Style| Run { text: text.into(), style, ..Run::default() };
    let body = doc(vec![
        Block::Para(Para {
            runs: vec![
                styled("under", Style { underline: true, color: Some([200, 0, 0]), ..Style::default() }),
                styled("back", Style { background: Some([255, 255, 0]), bold: true, italic: true, ..Style::default() }),
                styled("big", Style { size: Some(24.0), ..Style::default() }),
                styled("up", Style { script: Script::Super, ..Style::default() }),
                styled("link", Style { link: Some("https://example.org/".into()), ..Style::default() }),
            ],
            ..Para::default()
        }),
        Block::Para(Para { runs: vec![run("right")], align: Align::Right, ..Para::default() }),
        Block::Para(Para { runs: vec![run("item")], kind: ParaKind::Item("1.".into()), indent: 1, ..Para::default() }),
        Block::Para(Para { runs: vec![run("quoted")], quote: 2, ..Para::default() }),
        Block::Rule,
        Block::Para(Para {
            runs: vec![Run { text: "a cat".into(), picture: Some(Picture::Remote(String::new())), ..Run::default() }],
            ..Para::default()
        }),
        Block::Para(Para { runs: vec![run("a\tb\u{a0}c\u{200b}d\u{7}")], ..Para::default() }),
    ]);
    let printed = print(&body, Paper::A4);
    let page = &printed.pages[0];
    let find = |text: &str| {
        page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap_or_else(|| panic!("{text}: {}", page.text()))
    };
    let under = find("under");
    assert_eq!(under.color, [200, 0, 0]);
    assert!(page.rules.iter().any(|r| r.color == [200, 0, 0] && r.from.0 == under.x && r.from.1 > under.y));
    // The face has no bold or italic of its own.
    let back = find("back");
    assert!(back.bold && back.slant);
    assert!(page.fills.iter().any(|f| f.color == [255, 255, 0] && f.x == back.x && (f.width - back.width).abs() < 0.01));
    // All of a line stands on one baseline, the raised text above it.
    let (big, up) = (find("big"), find("up"));
    assert_eq!((big.size, big.y), (24.0, under.y));
    assert!(up.y < under.y && up.size < 12.0);
    assert_eq!(find("link").color, [0, 84, 166]);
    let right = find("right");
    assert!((right.x + right.width - (Paper::A4.width - 36.0)).abs() < 0.01);
    let item = find("item");
    assert!((item.x - 66.0).abs() < 0.01);
    let dot = find("1.");
    assert!(dot.x + dot.width < item.x && dot.y == item.y);
    let quoted = find("quoted");
    assert_eq!(page.rules.iter().filter(|r| r.from.0 == r.to.0 && r.from.1 < quoted.y && r.to.1 > quoted.y).count(), 2);
    assert!(page.rules.iter().any(|r| r.from.1 == r.to.1 && (r.to.0 - r.from.0 - (Paper::A4.width - 72.0)).abs() < 0.01));
    assert_eq!(find("[picture not downloaded: a cat]").size, 9.0);
    // A tab is room up to the next stop (every half inch where the paragraph has none of its
    // own), a hard space a space; marks that draw nothing are not there.
    let (a, rest) = (find("a"), find("b cd"));
    assert!((rest.x - (a.x + 36.0)).abs() < 0.01);
}

#[test]
fn a_table_stands_in_columns_inside_its_lines() {
    let mut wide = cell("wide");
    wide.span = 2;
    wide.background = Some([200, 220, 255]);
    let mut low = cell("low");
    low.valign = VAlign::Bottom;
    let tall =
        Cell { blocks: vec![Block::Para(para("one")), Block::Para(para("two")), Block::Para(para("three"))], span: 1, rows: 1, ..Cell::default() };
    let table = Table { rows: vec![vec![cell("a"), cell("bbbb")], vec![wide], vec![low, tall]], border: true, padding: 4, ..Table::default() };
    let printed = print(&doc(vec![Block::Para(para("before")), Block::Table(table), Block::Para(para("after"))]), Paper::A4);
    let page = &printed.pages[0];
    let find = |text: &str| page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap();
    let (a, b, wide, low, three) = (find("a"), find("bbbb"), find("wide"), find("low"), find("three"));
    assert!(a.y == b.y && b.x > a.x + a.width && wide.y > a.y && wide.x == a.x);
    assert_eq!(low.y, three.y);
    assert!(find("before").y < a.y && find("after").y > three.y);
    // The second column starts where it does in every row.
    assert_eq!(find("one").x, b.x);
    // Five cells, each inside four lines.
    assert_eq!(page.rules.iter().filter(|r| r.width == 0.75).count(), 20);
    let fill = page.fills.iter().find(|f| f.color == [200, 220, 255]).unwrap();
    assert!(fill.x < wide.x && fill.x + fill.width > b.x + b.width && fill.y < wide.y && fill.y + fill.height > wide.y);
    // As wide as it needs to be and no wider: padding at both sides of both columns.
    assert!((fill.width - (18.0 + 30.0 + 4.0 * 3.0)).abs() < 0.01, "{}", fill.width);

    // A table that fills the page gives a column that asks for a width that width.
    let mut named = cell("n");
    named.width = 100;
    let full = Table { rows: vec![vec![named, cell("value")]], width: TableWidth::Fill, ..Table::default() };
    let printed = print(&doc(vec![Block::Table(full)]), Paper::A4);
    let page = &printed.pages[0];
    let x = |ch: char| page.texts.iter().find(|t| t.glyphs[0].1 == ch).unwrap().x;
    assert!((x('v') - x('n') - 75.0).abs() < 0.01);
}

#[test]
fn a_table_longer_than_a_page_goes_on_with_its_lines_and_frames() {
    let rows: Vec<Vec<Cell>> = (0..60).map(|i| vec![cell(&format!("r{i}")), cell("v")]).collect();
    let edge = Edge { width: 4, color: Some([0, 0, 90]), line: Line::Double };
    let frame = Arc::new(Frame { edges: [edge; 4], padding: [4; 4], indent: 0, between: Edge::default(), fill: None, within: None, radius: 0 });
    let framed = |text: &str| Block::Para(Para { frame: Some(frame.clone()), ..para(text) });
    let body = doc(vec![Block::Table(Table { rows, border: true, frame: [edge; 4], ..Table::default() }), framed("in"), framed("frame")]);
    let printed = print(&body, SMALL);
    assert!(printed.pages.len() > 2);
    within(&printed, SMALL);
    let all: Vec<String> = printed.pages.iter().flat_map(body_lines).collect();
    let rows: Vec<&String> = all.iter().filter(|l| l.starts_with('r')).collect();
    assert_eq!(rows.len(), 60);
    assert!(rows.iter().enumerate().all(|(i, l)| **l == format!("r{i}v")));
    assert_eq!(all[60..], ["in".to_string(), "frame".into()]);
    for page in &printed.pages {
        assert!(page.rules.iter().all(|r| r.from.1 >= 19.0 && r.to.1 <= 281.0 && r.from.0 >= 19.0 && r.to.0 <= 181.0), "lines stay on the page");
    }
    // A double line is two thin ones; one frame goes round both paragraphs.
    let last = printed.pages.last().unwrap();
    assert!(last.rules.iter().filter(|r| r.color == [0, 0, 90] && r.width == 1.0).count() >= 8);
}

fn nested(depth: usize) -> Block {
    let mut block = Block::Para(para("deep"));
    for _ in 0..depth {
        let cell =
            Cell { blocks: vec![block], span: 60_000, rows: 60_000, width: u16::MAX, share: 255, padding: Some([u16::MAX; 4]), ..Cell::default() };
        block = Block::Table(Table {
            rows: vec![vec![cell]],
            border: true,
            padding: u16::MAX,
            spacing: u16::MAX,
            width: TableWidth::Pixels(u16::MAX),
            ..Table::default()
        });
    }
    block
}

#[test]
fn hostile_bodies_never_panic_and_stay_within_the_limits() {
    let wild = Edge { width: 255, color: None, line: Line::Double };
    let frame =
        Arc::new(Frame { edges: [wild; 4], padding: [u16::MAX; 4], indent: u8::MAX, between: wild, fill: Some([1, 2, 3]), within: None, radius: 0 });
    let odd = |size: f32| Run {
        text: "odd \u{10ffff}\u{0}\n\n\t".repeat(20),
        style: Style { size: Some(size), family: Some(",,,;\"".into()), ..Style::default() },
        ..Run::default()
    };
    let many: Vec<Cell> =
        (0..300).map(|i| Cell { span: i as u16, rows: 300 - i as u16, share: i as u8, width: i as u16 * 200, ..cell("c") }).collect();
    let blocks = vec![
        nested(300),
        Block::Para(Para {
            runs: [f32::NAN, f32::INFINITY, -1.0, 0.0, 1e30].map(odd).to_vec(),
            indent: 255,
            quote: 255,
            frame: Some(frame),
            ..Para::default()
        }),
        Block::Table(Table { rows: vec![many.clone(), Vec::new(), many], width: TableWidth::Percent(255), frame: [wild; 4], ..Table::default() }),
        Block::Table(Table { rows: vec![Vec::new(); 50], ..Table::default() }),
        Block::Para(Para { kind: ParaKind::Item("\u{0}".repeat(1000)), ..Para::default() }),
    ];
    let body = doc(blocks);
    for paper in [
        SMALL,
        Paper::A4,
        Paper { width: f32::NAN, height: f32::INFINITY, margin: -1e30 },
        Paper { width: 1.0, height: 1.0, margin: 1e30 },
        Paper { width: 1e30, height: 0.0, margin: f32::NAN },
    ] {
        let printed = print(&body, paper);
        assert!(!printed.pages.is_empty() && printed.pages.len() <= MAX_PAGES);
        for page in &printed.pages {
            assert!(page.width.is_finite() && page.height.is_finite());
            assert!(page.texts.iter().all(|t| t.x.is_finite() && t.y.is_finite() && t.width.is_finite() && t.face < printed.faces.len()));
            assert!(page.fills.iter().all(|f| f.x.is_finite() && f.y.is_finite() && f.width.is_finite() && f.height.is_finite()));
            assert!(page.rules.iter().all(|r| r.from.0.is_finite()
                && r.from.1.is_finite()
                && r.to.0.is_finite()
                && r.to.1.is_finite()
                && r.width.is_finite()));
        }
    }

    // More than the pages there may be: the rest is left out, and said to be.
    let endless = Doc::plain(&"a\n".repeat(60_000));
    let printed = print(&endless, Paper { width: 100.0, height: 100.0, margin: 10.0 });
    assert_eq!(printed.pages.len(), MAX_PAGES);
    assert!(printed.truncated);
}

#[test]
fn printing_needs_a_font() {
    let body = doc(vec![Block::Para(para("text"))]);
    let library = Library::empty();
    let mut fonts = Fonts::new(&library);
    assert_eq!(
        memo(&Memo { owner: "o", fields: &[], body: &body, plain: false, sizes: &|_| None }, Paper::A4, &mut fonts).err(),
        Some(LayoutError::NoFont)
    );
    // A font that may not be put into a document is none.
    let mut fonts = Fonts::new(&library);
    fonts.give("Arial", false, false, Face { data: boxes::font_with(CHARS, 0x0002), index: 0 });
    assert_eq!(
        memo(&Memo { owner: "o", fields: &[], body: &body, plain: false, sizes: &|_| None }, Paper::A4, &mut fonts).err(),
        Some(LayoutError::NoFont)
    );
    // Any font that is there will do for a character none has.
    let mut fonts = Fonts::new(&library);
    fonts.give("Arial", false, false, Face { data: boxes::font("o"), index: 0 });
    let printed = memo(&Memo { owner: "o", fields: &[], body: &body, plain: false, sizes: &|_| None }, Paper::A4, &mut fonts).unwrap();
    assert_eq!(printed.pages[0].texts[1].glyphs, [(0, 't'), (0, 'e'), (0, 'x'), (0, 't')]);
    assert!(printed.pages[0].texts[0].bold && !printed.pages[0].texts[1].bold);
}

#[test]
fn pictures_stand_in_the_line_at_the_size_asked_for() {
    let shown =
        |text: &str, id: &str, extent: (u16, u16)| Run { text: text.into(), picture: Some(Picture::Attached(id.into())), extent, ..Run::default() };
    let in_cell = Cell {
        blocks: vec![Block::Para(Para { runs: vec![shown("", "small", (0, 0))], ..Para::default() })],
        span: 1,
        rows: 1,
        valign: VAlign::Bottom,
        ..Cell::default()
    };
    let tall = Cell {
        blocks: vec![Block::Para(para("a")), Block::Para(para("b")), Block::Para(para("c")), Block::Para(para("d"))],
        span: 1,
        rows: 1,
        ..Cell::default()
    };
    let body = doc(vec![
        Block::Para(Para { runs: vec![run("ab"), shown("alt", "small", (0, 0)), run("cd")], ..Para::default() }),
        Block::Para(Para { runs: vec![shown("", "small", (80, 0))], align: Align::Right, ..Para::default() }),
        Block::Para(Para { runs: vec![shown("", "huge", (0, 0))], ..Para::default() }),
        Block::Para(Para { runs: vec![shown("gone", "missing", (40, 40))], ..Para::default() }),
        Block::Table(Table { rows: vec![vec![in_cell, tall]], ..Table::default() }),
    ]);
    let sizes = |picture: &Picture| match picture {
        Picture::Attached(id) if id == "small" => Some((40, 20)),
        Picture::Attached(id) if id == "huge" => Some((u16::MAX, u16::MAX)),
        _ => None,
    };
    let printed = print_on(&Library::empty(), &Memo { owner: "Owner", fields: &[], body: &body, plain: false, sizes: &sizes }, SMALL).unwrap();
    let all: Vec<&Placed> = printed.pages.iter().flat_map(|p| &p.pictures).collect();
    assert_eq!(all.len(), 4);
    for placed in &all {
        assert!(placed.x >= 19.99 && placed.x + placed.width <= 180.01 && placed.y >= 19.99 && placed.y + placed.height <= 264.01, "{placed:?}");
    }
    // 40 by 20 pixels are 30 by 15 points; the picture stands on the baseline between the words.
    let page = &printed.pages[0];
    let (ab, cd) = (page.texts.iter().find(|t| t.glyphs[0].1 == 'a').unwrap(), page.texts.iter().find(|t| t.glyphs[0].1 == 'c').unwrap());
    let first = all[0];
    assert_eq!((first.width, first.height), (30.0, 15.0));
    assert!((first.x - (ab.x + 12.0)).abs() < 0.01 && (cd.x - (first.x + 30.0)).abs() < 0.01);
    assert!((first.y + first.height - ab.y).abs() < 0.01 && ab.y == cd.y);
    // A width asked for keeps the picture's shape; the paragraph's place is the picture's.
    assert_eq!((all[1].width, all[1].height), (60.0, 30.0));
    assert!((all[1].x + 60.0 - 180.0).abs() < 0.01 && all[1].y >= first.y + first.height);
    // One larger than the page is made to fit it, and goes to a page with room.
    assert!((all[2].width - 160.0).abs() < 0.01 && (all[2].height - 160.0).abs() < 0.01);
    // One that cannot be printed is named.
    assert!(printed.pages.iter().any(|p| p.text().contains("[picture: gone]")));
    // In a cell it is moved with what else the cell holds: to the foot, beside the last line.
    let last_page = printed.pages.last().unwrap();
    let d = last_page.texts.iter().find(|t| t.glyphs[0].1 == 'd').unwrap();
    assert!(all[3].y + all[3].height <= d.y + 3.01 && all[3].y + all[3].height > d.y - 12.0, "{:?} {}", all[3], d.y);
}

#[test]
fn text_after_a_tab_stands_at_its_stop() {
    use openmailclient_formats::rich::{Tab, TabKind};
    let stop = |at, kind| Tab { at, kind, lead: Default::default() };
    // Stops are in pixels, four to three points: 30, 90 and 120 points from the start.
    let tabs = vec![stop(40, TabKind::Left), stop(120, TabKind::Right), stop(160, TabKind::Decimal)];
    let body = doc(vec![
        Block::Para(Para { tabs: tabs.clone(), ..para("a\tbb\tcc\t1.5") }),
        // Past the last stop, and where there are none: the stops every paragraph has.
        Block::Para(Para { tabs: vec![stop(4, TabKind::Left)], ..para("dddd\tee") }),
        // Text too long to end at its stop starts where the tab is.
        Block::Para(Para { tabs: vec![stop(16, TabKind::Right)], ..para("f\tgggggg") }),
        // Every line of a paragraph starts again.
        Block::Para(Para { tabs, ..para("h\tii\nj\tkk") }),
    ]);
    let printed = print(&body, Paper::A4);
    let page = &printed.pages[0];
    let find = |text: &str| {
        page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap_or_else(|| panic!("{text}: {}", page.text()))
    };
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    let (a, bb, cc, num) = (find("a"), find("bb"), find("cc"), find("1.5"));
    assert!(near(bb.x, a.x + 30.0), "{} {}", bb.x, a.x);
    assert!(near(cc.x + cc.width, a.x + 90.0));
    assert!(near(num.x + num.width / 3.0, a.x + 120.0));
    assert!(a.y == bb.y && bb.y == cc.y && cc.y == num.y);
    let (d, e) = (find("dddd"), find("ee"));
    assert!(near(e.x, d.x + 36.0), "{} {}", e.x, d.x + d.width);
    let (f, g) = (find("f"), find("gggggg"));
    assert!(near(g.x, f.x + f.width));
    let (h, ii, j, kk) = (find("h"), find("ii"), find("j"), find("kk"));
    assert!(near(ii.x, h.x + 30.0) && near(kk.x, j.x + 30.0) && near(h.x, j.x) && j.y > h.y);

    // Hostile: stops out of any range, a tab at every character, tabs past the end of a line.
    let wild = vec![stop(0, TabKind::Right), stop(u16::MAX, TabKind::Decimal), stop(u16::MAX, TabKind::Center)];
    let body = doc(vec![
        Block::Para(Para { tabs: wild.clone(), ..para(&"a\t".repeat(3000)) }),
        Block::Para(Para { tabs: wild, kind: ParaKind::Pre, ..para(&format!("{}\t\t\tb.\t.\t", "word ".repeat(200))) }),
    ]);
    within(&print(&body, SMALL), SMALL);
}

#[test]
fn lines_are_as_tall_as_their_paragraph_says() {
    use openmailclient_formats::rich::Leading;
    let two = |leading| Block::Para(Para { leading, ..para("aa\nbb") });
    // Four paragraphs of two lines: the step from the first line to the second.
    let body = doc(vec![two(Leading::Own), two(Leading::Times(200)), two(Leading::Exactly(40)), two(Leading::AtLeast(4)), two(Leading::Exactly(4))]);
    let printed = print(&body, Paper::A4);
    let page = &printed.pages[0];
    let ys = |text: &str| page.texts.iter().filter(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).map(|t| t.y).collect::<Vec<f32>>();
    let (a, b) = (ys("aa"), ys("bb"));
    assert_eq!((a.len(), b.len()), (5, 5));
    let step: Vec<f32> = a.iter().zip(&b).map(|(a, b)| b - a).collect();
    let own = step[0];
    assert!(own > 1.0);
    assert!((step[1] - own * 2.0).abs() < 0.01, "{step:?}");
    assert!((step[2] - 30.0).abs() < 0.01, "{step:?}");
    assert!((step[3] - own).abs() < 0.01, "{step:?}");
    assert!((step[4] - 3.0).abs() < 0.01, "{step:?}");
    // What a line gains stands below its text: the paragraph after a doubled one starts lower.
    let gap = |n: usize| a[n] - b[n - 1];
    assert!((gap(2) - gap(1) - own).abs() < 0.01 && (gap(3) - gap(1) - (30.0 - own)).abs() < 0.01);

    // Hostile: heights out of any range, on a small sheet, with more lines than a page.
    for leading in [Leading::Times(0), Leading::Times(u16::MAX), Leading::AtLeast(u16::MAX), Leading::Exactly(u16::MAX), Leading::Exactly(0)] {
        let body = doc(vec![Block::Para(Para { leading, ..para(&"word ".repeat(400)) })]);
        within(&print(&body, SMALL), SMALL);
    }
}

#[test]
fn leaders_fill_the_room_of_tabs_and_lines_stand_between_paragraphs() {
    use openmailclient_formats::rich::{Leader, Line, Tab, TabKind};
    let stop = |at, lead| Tab { at, kind: TabKind::Right, lead };
    let edge = Edge { width: 1, color: None, line: Line::Solid };
    let between = Edge { width: 2, color: Some([200, 0, 0]), line: Line::Solid };
    let frame = Arc::new(Frame { edges: [edge; 4], padding: [4; 4], indent: 0, between, fill: None, within: None, radius: 0 });
    let body = doc(vec![
        Block::Para(Para { tabs: vec![stop(160, Leader::Dots)], ..para("aa\t1") }),
        Block::Para(Para { tabs: vec![stop(160, Leader::None)], ..para("bb\t2") }),
        Block::Para(Para { tabs: vec![stop(160, Leader::Equals)], ..para("cc\t3") }),
        Block::Para(Para { frame: Some(frame.clone()), ..para("dd") }),
        Block::Para(Para { frame: Some(frame), ..para("ee") }),
    ]);
    let printed = print(&body, Paper::A4);
    let page = &printed.pages[0];
    let find = |text: &str| page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap();
    // On the line of the letters, between the text before the tab and the text after it.
    let on_line = |before: &str, after: &str| {
        let (a, b) = (find(before), find(after));
        page.rules
            .iter()
            .filter(|r| r.from.1 == r.to.1 && r.from.0 > a.x + a.width && r.to.0 < b.x && (r.from.1 - a.y).abs() < a.size * 0.5)
            .collect::<Vec<_>>()
    };
    let dots = on_line("aa", "1");
    assert_eq!(dots.len(), 1);
    assert_eq!(dots[0].line, Line::Dotted);
    assert!(dots[0].to.0 - dots[0].from.0 > 80.0);
    assert!(on_line("bb", "2").is_empty());
    assert_eq!(on_line("cc", "3").len(), 2);

    // The line between two paragraphs in one border: across the border, between their texts.
    let (d, e) = (find("dd"), find("ee"));
    let lines: Vec<_> = page.rules.iter().filter(|r| r.color == [200, 0, 0]).collect();
    assert_eq!(lines.len(), 1);
    let line = lines[0];
    assert!(line.from.1 == line.to.1 && line.from.1 > d.y && line.to.1 < e.y - e.size * 0.5, "{line:?} {} {}", d.y, e.y);
    assert!(line.from.0 < d.x && line.to.0 > 400.0 && (line.width - 1.5).abs() < 0.01);

    // Hostile: lines of any width between many paragraphs, leaders with no room to stand in.
    let wild = Edge { width: u8::MAX, color: None, line: Line::Double };
    let frame =
        Arc::new(Frame { edges: [wild; 4], padding: [u16::MAX; 4], indent: u8::MAX, between: wild, fill: Some([1, 2, 3]), within: None, radius: 0 });
    let mut blocks: Vec<Block> = (0..300).map(|_| Block::Para(Para { frame: Some(frame.clone()), ..para("a") })).collect();
    blocks.push(Block::Para(Para { tabs: (1..40).map(|n| stop(n, Leader::ThickLine)).collect(), ..para(&"a\t".repeat(500)) }));
    blocks.push(Block::Para(Para { tabs: vec![stop(u16::MAX, Leader::Hyphens)], ..para("a\tb\t\t") }));
    within(&print(&doc(blocks), SMALL), SMALL);
}

#[test]
fn a_filled_box_is_behind_its_paragraphs_with_its_room_on_each_side() {
    let frame = Arc::new(Frame {
        edges: Default::default(),
        padding: [6, 10, 14, 30],
        indent: 0,
        between: Edge::default(),
        fill: Some([0, 51, 102]),
        within: None,
        radius: 0,
    });
    let boxed = |text: &str| Block::Para(Para { frame: Some(frame.clone()), ..para(text) });
    let body = doc(vec![Block::Para(para("before")), boxed("one"), boxed("two"), Block::Para(para("after"))]);
    let printed = print(&body, Paper::A4);
    let page = &printed.pages[0];
    let find = |text: &str| page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap();
    let fills: Vec<_> = page.fills.iter().filter(|f| f.color == [0, 51, 102]).collect();
    assert_eq!(fills.len(), 1, "one fill behind both paragraphs");
    let fill = fills[0];
    let (plain, one, two, after) = (find("before"), find("one"), find("two"), find("after"));
    // The box is as wide as the text's room; its text stands in from the left by the room asked.
    assert!((fill.x - plain.x).abs() < 0.01 && (one.x - fill.x - 30.0 * 0.75).abs() < 0.01, "{} {} {}", fill.x, plain.x, one.x);
    assert!(fill.y < one.y - one.size * 0.5 && fill.y + fill.height > two.y + 14.0 * 0.75 - 0.01);
    assert!(fill.y > plain.y && fill.y + fill.height < after.y);

    // A box longer than a page is filled on each page it is on, inside the page's room.
    let many: Vec<Block> = (0..80).map(|i| boxed(&format!("line {i}"))).collect();
    let printed = print(&doc(many), SMALL);
    assert!(printed.pages.len() > 2);
    within(&printed, SMALL);
    for page in &printed.pages {
        let fills: Vec<_> = page.fills.iter().filter(|f| f.color == [0, 51, 102]).collect();
        assert_eq!(fills.len(), 1);
        assert!(fills[0].y >= 19.9 && fills[0].y + fills[0].height <= 280.1 && fills[0].height > 0.0);
    }
}

fn print_in(font: Vec<u8>, body: &Doc, paper: Paper) -> Printed {
    let library = Library::empty();
    let mut fonts = Fonts::new(&library);
    for family in ["Segoe UI", "Times New Roman", "Calibri"] {
        fonts.give(family, false, false, Face { data: font.clone(), index: 0 });
    }
    memo(&Memo { owner: "Owner", fields: &[], body, plain: false, sizes: &|_| None }, paper, &mut fonts).unwrap()
}

#[test]
fn pairs_a_face_kerns_stand_as_close_as_it_says() {
    // "AV" stands a tenth of the size closer, "Va" a twentieth, "ab" further apart.
    let pairs = [('A', 'V', -100), ('V', 'a', -50), ('a', 'b', 30), ('V', 'A', 0)];
    let body = doc(vec![Block::Para(para("AVab VA")), Block::Para(Para { align: Align::Right, ..para("xAV") }), Block::Para(para("A\tV"))]);
    let plain = print(&body, Paper::A4);
    for gpos in [false, true] {
        let printed = print_in(boxes::font_kerned(CHARS, &pairs, gpos), &body, Paper::A4);
        let page = &printed.pages[0];
        let find = |page: &Page, text: &str| page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap().clone();
        let (set, was) = (find(page, "AVab VA"), find(&plain.pages[0], "AVab VA"));
        // The body is set at 12 points: a box is 6 wide, and a thousandth of the size 0.012.
        let near = |a: f32, b: f32| (a - b).abs() < 0.001;
        assert!(was.kerns.is_empty() && near(was.width, 6.0 * 6.0 + 3.0));
        assert_eq!(set.kerns.len(), 7, "gpos {gpos}");
        let want = [-1.2, -0.6, 0.36, 0.0, 0.0, 0.0, 0.0];
        assert!(set.kerns.iter().zip(want).all(|(k, w)| near(*k, w)), "gpos {gpos}: {:?}", set.kerns);
        assert!(near(set.width, was.width - 1.2 - 0.6 + 0.36), "{}", set.width);
        // Text set to the right ends where the room ends, kerned or not.
        let (right, right_was) = (find(page, "xAV"), find(&plain.pages[0], "xAV"));
        assert!(near(right.x + right.width, right_was.x + right_was.width) && near(right.width, right_was.width - 1.2));
        // Nothing is kerned across a tab.
        assert!(page.texts.iter().filter(|t| t.glyphs.len() == 1 && matches!(t.glyphs[0].1, 'A' | 'V')).all(|t| t.kerns.is_empty()));
        within(&printed, Paper::A4);
    }

    // More fits on a line that is kerned tighter: 26 boxes fill the small sheet, 27 with pairs.
    let tight: Vec<(char, char, i16)> = vec![('A', 'V', -200), ('V', 'A', -200)];
    let word = "AV".repeat(14);
    let body = doc(vec![Block::Para(para(&word))]);
    assert_eq!(body_lines(&print(&body, SMALL).pages[0]).len(), 2);
    let printed = print_in(boxes::font_kerned(CHARS, &tight, true), &body, SMALL);
    assert_eq!(body_lines(&printed.pages[0]), std::slice::from_ref(&word));
    within(&printed, SMALL);

    // A pair in two sizes or two looks is not one pair.
    let two = Para { runs: vec![run("A"), Run { style: Style { size: Some(20.0), ..Style::default() }, ..run("V") }], ..Para::default() };
    let printed = print_in(boxes::font_kerned(CHARS, &pairs, false), &doc(vec![Block::Para(two)]), Paper::A4);
    assert!(printed.pages[0].texts.iter().all(|t| t.kerns.is_empty()));
}

#[test]
fn hostile_kerning_tables_move_nothing_far() {
    let body = doc(vec![Block::Para(para("AVAVAV abc VA")), Block::Para(para(&"AV".repeat(400)))]);
    let whole = boxes::font_kerned(CHARS, &[('A', 'V', i16::MIN), ('V', 'A', i16::MAX)], true);
    // Amounts beyond reason are held to half an em, and no glyph moves back past its start.
    let printed = print_in(whole.clone(), &body, SMALL);
    within(&printed, SMALL);
    for text in printed.pages.iter().flat_map(|p| &p.texts) {
        assert!(text.width >= -0.001 && text.kerns.iter().all(|k| k.abs() <= text.size / 2.0 + 0.001), "{text:?}");
    }
    // Tables that are cut short, or filled with anything, are read as far as they hold.
    let kern = boxes::font_kerned(CHARS, &[('A', 'V', -100)], false);
    for font in [&whole, &kern] {
        let tag: &[u8; 4] = if std::ptr::eq(font, &whole) { b"GPOS" } else { b"kern" };
        let record = (0..usize::from(u16::from_be_bytes([font[4], font[5]]))).map(|i| 12 + i * 16).find(|at| &font[*at..*at + 4] == tag).unwrap();
        let start = u32::from_be_bytes(font[record + 8..record + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(font[record + 12..record + 16].try_into().unwrap()) as usize;
        for cut in 0..len {
            let mut short = font.clone();
            short[record + 12..record + 16].copy_from_slice(&(cut as u32).to_be_bytes());
            within(&print_in(short, &body, SMALL), SMALL);
        }
        for (step, fill) in [(1usize, 0xFFu8), (2, 0x00), (3, 0x7F), (5, 0x01), (7, 0x80)] {
            let mut wild = font.clone();
            for at in (start..start + len).step_by(step) {
                wild[at] = fill;
            }
            within(&print_in(wild, &body, SMALL), SMALL);
        }
    }
    for junk in [vec![], vec![0u8; 3], vec![0xFF; 64], vec![0, 1, 0, 0, 0, 10, 0, 10, 0, 10, 0xFF, 0xFF], (0..=255u8).collect::<Vec<u8>>()] {
        for tag in [*b"GPOS", *b"kern"] {
            within(&print_in(boxes::font_and(CHARS, vec![(tag, junk.clone())]), &body, SMALL), SMALL);
        }
    }
}

#[test]
fn text_stands_beside_a_picture_at_one_side() {
    use openmailclient_formats::rich::Float;
    let side = |id: &str, float: Float| Run { picture: Some(Picture::Attached(id.into())), float, ..Run::default() };
    let sizes = |picture: &Picture| match picture {
        // 80 by 40 pixels are 60 by 30 points; with its gap a picture takes 66 of the width.
        Picture::Attached(id) if id == "small" => Some((80, 40)),
        Picture::Attached(id) if id == "wide" => Some((100, 40)),
        Picture::Attached(id) if id == "tall" => Some((80, 160)),
        Picture::Attached(id) if id == "huge" => Some((u16::MAX, u16::MAX)),
        _ => None,
    };
    let print = |body: &Doc, paper: Paper| {
        print_on(&Library::empty(), &Memo { owner: "Owner", fields: &[], body, plain: false, sizes: &sizes }, paper).unwrap()
    };
    let words = |n: usize| vec!["word"; n].join(" ");
    let (left, right) = (36.0f32, 595.28f32 - 36.0);
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    let lines_of = |page: &Page| -> Vec<Text> { page.texts.iter().filter(|t| t.glyphs.first().is_some_and(|g| g.1 == 'w')).cloned().collect() };

    for float in [Float::Left, Float::Right] {
        let body = doc(vec![Block::Para(Para { runs: vec![side("small", float), run(&words(90))], ..Para::default() }), Block::Para(para("after"))]);
        let printed = print(&body, Paper::A4);
        let page = &printed.pages[0];
        assert_eq!(page.pictures.len(), 1);
        let picture = &page.pictures[0];
        let lines = lines_of(page);
        assert!(lines.len() >= 5, "{}", lines.len());
        // The picture is at its side, its top at the top of the first line (12 points tall,
        // with its baseline 9.6 down).
        assert_eq!((picture.width, picture.height), (60.0, 30.0));
        assert!(near(picture.x, if float == Float::Left { left } else { right - 60.0 }), "{picture:?}");
        assert!(near(picture.y, lines[0].y - 9.6), "{} {}", picture.y, lines[0].y);
        // Three lines start above its foot and are shorter by its width and the gap; the
        // fourth is under it, as wide as the page's room.
        for (n, line) in lines.iter().enumerate() {
            let (from, to) = match (n < 3, float) {
                (true, Float::Left) => (left + 66.0, right),
                (true, _) => (left, right - 66.0),
                (false, _) => (left, right),
            };
            assert!(near(line.x, from) && line.x + line.width <= to + 0.01, "line {n} of {float:?}: {} {}", line.x, line.width);
            // As many words as there is room for: one more would not fit.
            assert!(n + 1 == lines.len() || line.width + 27.0 > to - from, "line {n} of {float:?} is short");
        }
        assert!(lines[3].y > picture.y + picture.height);
        within(&printed, Paper::A4);
    }

    // A picture taller than its paragraph: what follows stands beside it too, until a
    // paragraph asks to start below it, or something that is no plain paragraph comes.
    let tall = |then: Block| {
        doc(vec![
            Block::Para(Para { runs: vec![side("tall", Float::Left), run("one")], ..Para::default() }),
            Block::Para(para("two")),
            then,
            Block::Para(para("end")),
        ])
    };
    let find = |page: &Page, text: &str| page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap().clone();
    // A paragraph that asks to start below it goes under it; a table or a bordered block
    // stands beside it while there is room, as Outlook wraps them.
    for (n, then) in [
        Block::Para(Para { clear: true, ..para("below") }),
        Block::Table(Table { rows: vec![vec![cell("below")]], ..Table::default() }),
        Block::Para(Para {
            frame: Some(Arc::new(Frame {
                edges: Default::default(),
                padding: [0; 4],
                indent: 0,
                between: Edge::default(),
                fill: Some([1, 2, 3]),
                within: None,
                radius: 0,
            })),
            ..para("below")
        }),
    ]
    .into_iter()
    .enumerate()
    {
        let printed = print(&tall(then), Paper::A4);
        let page = &printed.pages[0];
        let picture = &page.pictures[0];
        assert_eq!((picture.width, picture.height), (60.0, 120.0));
        let (one, two, below, end) = (find(page, "one"), find(page, "two"), find(page, "below"), find(page, "end"));
        assert!(near(one.x, left + 66.0) && near(two.x, left + 66.0) && two.y > one.y && two.y < picture.y + 120.0);
        if n == 0 {
            assert!(below.x < left + 20.0 && below.y - 9.6 >= picture.y + 120.0 - 0.01, "{} {}", below.x, below.y);
            assert!(near(end.x, left) && end.y > below.y);
        } else {
            // Beside it, under "two", and what follows still beside it while it lasts.
            assert!(below.x >= left + 66.0 - 0.01 && below.y > two.y && below.y < picture.y + 120.0, "{n}: {} {}", below.x, below.y);
            assert!(end.y > below.y && (end.y - 9.6 >= picture.y + 120.0 - 0.01 || end.x >= left + 66.0 - 0.01), "{n}: {} {}", end.x, end.y);
        }
    }
    // On a page too narrow beside it, the table goes below the picture.
    let wide = doc(vec![
        Block::Para(Para { runs: vec![side("huge", Float::Left), run("one")], ..Para::default() }),
        Block::Table(Table { rows: vec![vec![cell("under")]], ..Table::default() }),
    ]);
    let printed = print(&wide, Paper::A4);
    for page in &printed.pages {
        if let (Some(picture), Some(under)) =
            (page.pictures.first(), page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == "under"))
        {
            assert!(under.y - 9.6 >= picture.y + picture.height - 0.01 || under.x < left + 20.0, "{} {}", under.x, under.y);
        }
    }
    within(&printed, Paper::A4);
    // Blocks that end beside a picture end under it: a cell is as tall as its picture, and
    // the next cell's text does not stand beside a picture that is not its own.
    let floated = Cell {
        blocks: vec![Block::Para(Para { runs: vec![side("tall", Float::Right), run("in")], ..Para::default() })],
        span: 1,
        rows: 1,
        ..Cell::default()
    };
    let body = doc(vec![
        Block::Table(Table { rows: vec![vec![floated, cell("next")]], width: TableWidth::Percent(100), ..Table::default() }),
        Block::Para(para("after")),
    ]);
    let printed = print(&body, Paper::A4);
    let page = &printed.pages[0];
    let (picture, next, after) = (&page.pictures[0], find(page, "next"), find(page, "after"));
    assert!(after.y - 9.6 >= picture.y + 120.0 - 0.01 && near(after.x, left));
    assert!(next.x >= picture.x + picture.width - 0.01 && next.x + next.width <= right + 0.01, "{picture:?} {} {}", next.x, next.width);

    // Where too little room would be left beside it, a picture stands in the line.
    let body = doc(vec![Block::Para(Para { runs: vec![side("wide", Float::Left), run("ab cd")], ..Para::default() })]);
    let printed = print(&body, SMALL);
    let (picture, ab) = (&printed.pages[0].pictures[0], find(&printed.pages[0], "ab cd"));
    assert!(near(picture.x, 20.0) && near(ab.x, 20.0 + 75.0) && near(picture.y + picture.height, ab.y));
    // With room for it, it stands at the side on the small sheet too.
    let body = doc(vec![Block::Para(Para { runs: vec![side("small", Float::Right), run(&words(12))], ..Para::default() })]);
    let printed = print(&body, SMALL);
    assert!(near(printed.pages[0].pictures[0].x, 180.0 - 60.0));
    assert!(lines_of(&printed.pages[0])[0].width <= 94.01);
    within(&printed, SMALL);

    // A picture that has no room left on its page goes to the next with its text.
    let mut blocks: Vec<Block> = (0..17).map(|i| Block::Para(para(&format!("line {i}")))).collect();
    blocks.push(Block::Para(Para { runs: vec![side("small", Float::Left), run("beside it")], ..Para::default() }));
    let printed = print(&doc(blocks), SMALL);
    assert_eq!(printed.pages.len(), 2);
    assert!(printed.pages[0].pictures.is_empty());
    let (picture, beside) = (&printed.pages[1].pictures[0], find(&printed.pages[1], "beside it"));
    assert!(near(picture.y, 20.0) && near(beside.x, 20.0 + 66.0) && near(beside.y, 20.0 + 9.6));

    // Hostile: pictures of any size at the sides of everything, nested, by the thousand.
    let mut blocks: Vec<Block> = Vec::new();
    for i in 0..400u16 {
        let id = ["small", "wide", "tall", "huge", "none"][usize::from(i % 5)];
        let float = if i % 2 == 0 { Float::Left } else { Float::Right };
        let runs = vec![
            run("x"),
            Run { extent: (i.wrapping_mul(977), i.wrapping_mul(331)), ..side(id, float) },
            side("tall", float),
            run(&words(usize::from(i % 7))),
        ];
        let p = Para {
            runs,
            clear: i % 11 == 0,
            indent: (i % 9) as u8,
            quote: (i % 4) as u8,
            align: if i % 3 == 0 { Align::Right } else { Align::Left },
            ..Para::default()
        };
        blocks.push(if i % 6 == 0 {
            Block::Table(Table {
                rows: vec![vec![Cell { blocks: vec![Block::Para(p.clone()), Block::Para(p)], span: 1, rows: 1, ..Cell::default() }, cell("c")]],
                ..Table::default()
            })
        } else {
            Block::Para(p)
        });
    }
    for paper in [SMALL, Paper::A4, Paper { width: 80.0, height: 90.0, margin: 30.0 }] {
        let printed = print(&doc(blocks.clone()), paper);
        assert!(!printed.pages.is_empty());
        for page in &printed.pages {
            for placed in &page.pictures {
                assert!(placed.width > 0.0 && placed.height > 0.0 && placed.x.is_finite() && placed.y.is_finite());
                assert!(placed.y + placed.height <= page.height + 0.01 && placed.y >= -0.01, "{placed:?}");
            }
        }
    }
}

#[test]
fn pages_are_set_up_as_asked() {
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    let words = |n: usize| vec!["word"; n].join(" ");
    // The glyphs on one baseline, with where they start and end.
    let on = |page: &Page, y: f32| -> Vec<(f32, f32, char)> {
        let mut out = Vec::new();
        for t in page.texts.iter().filter(|t| (t.y - y).abs() < 0.01) {
            let mut x = t.x;
            for (_, ch) in &t.glyphs {
                let wide = f32::from(if *ch == ' ' { boxes::SPACE } else { boxes::ADVANCE }) / f32::from(boxes::UNITS) * t.size;
                out.push((x, x + wide, *ch));
                x += wide;
            }
        }
        out
    };
    let said = |line: &[(f32, f32, char)]| line.iter().map(|g| g.2).collect::<String>();
    let set = |setup: &Setup, body: &Doc| {
        print_on(&Library::empty(), &Memo { owner: "Owner", fields: &[], body, plain: false, sizes: &|_| None }, setup).unwrap()
    };
    let part = |s: &str| s.to_string();
    let long = doc(vec![Block::Para(para(&words(900)))]);

    // A sheet on its long side, with a margin of its own at each edge.
    let setup = Setup {
        landscape: true,
        margins: [72.0, 50.0, 60.0, 40.0],
        header: [part("[Subject]"), part("[date] x"), part("p[ Page ].[PAGES]")],
        footer: [part("[User]"), String::new(), part("[Page of Pages]")],
        words: vec![(part("Subject"), part("[Page] of [Pages]")), (part("Date"), part("d")), (part("User"), part("me"))],
        ..Setup::from(Paper::A4)
    };
    let printed = set(&setup, &long);
    assert!(printed.pages.len() > 1);
    for (n, page) in printed.pages.iter().enumerate() {
        assert_eq!((page.width, page.height), (841.89, 595.28));
        // The header is one line in the room over the body: its parts at the left, in the
        // middle and at the right. What a name gives is not looked through for names.
        let head = on(page, 72.0 + 4.0 + 6.4);
        let pages = printed.pages.len();
        assert_eq!(said(&head), format!("[Page] of [Pages]d xp{}.{pages}", n + 1));
        let (left, middle, right) = (&head[..17], &head[17..20], &head[20..]);
        assert!(near(left[0].0, 40.0), "{left:?}");
        assert!(near(right.last().unwrap().1, 841.89 - 50.0), "{right:?}");
        assert!(near(middle[0].0 - 40.0, 841.89 - 50.0 - middle[2].1), "{middle:?}");
        let foot = on(page, 595.28 - 60.0 - 4.0);
        assert_eq!(said(&foot), format!("me{} of {pages}", n + 1));
        assert!(near(foot[0].0, 40.0) && near(foot.last().unwrap().1, 841.89 - 50.0));
        // The body keeps inside the margins, under the header and over the footer.
        for text in page.texts.iter().filter(|t| t.size > 8.0) {
            assert!(text.x >= 40.0 - 0.01 && text.x + text.width <= 841.89 - 50.0 + 0.01, "{text:?}");
            assert!(text.y > 72.0 + 16.0 && text.y <= 595.28 - 60.0 - 16.0 + 0.01, "{text:?}");
        }
    }

    // With no header and no footer the body has their room: it starts at the top margin and
    // goes down to the bottom one.
    let bare = Setup { header: Default::default(), footer: Default::default(), ..setup.clone() };
    let printed = set(&bare, &doc(vec![Block::Para(para(&words(3000)))]));
    let page = &printed.pages[0];
    assert!(page.texts.iter().all(|t| t.size > 8.0));
    let (top, bottom) = page.texts.iter().fold((f32::MAX, 0.0f32), |(a, b), t| (a.min(t.y), b.max(t.y)));
    assert!(top < 72.0 + 16.0 && bottom > 595.28 - 60.0 - 16.0 && bottom <= 595.28 - 60.0 + 0.01, "{top} {bottom}");
    // A sheet that is wider than tall already lies as it is, and upright is as given.
    let wide = Paper { width: 300.0, height: 200.0, margin: 20.0 };
    for landscape in [false, true] {
        let page = &set(&Setup { landscape, ..Setup::from(wide) }, &long).pages[0];
        assert_eq!((page.width, page.height), (300.0, 200.0));
    }
    assert_eq!(set(&Setup { landscape: true, ..Setup::from(SMALL) }, &long).pages[0].width, 300.0);

    // A paper alone is the sheet upright with its number at the foot, as before.
    assert_eq!(
        Setup::from(Paper::LETTER),
        Setup {
            paper: Paper::LETTER,
            landscape: false,
            margins: [36.0; 4],
            header: Default::default(),
            footer: [String::new(), part("[Page of Pages]"), String::new()],
            details: true,
            edges: true,
            words: vec![]
        }
    );
    assert_eq!(Setup::default().paper, Paper::A4);
    for name in Paper::NAMES {
        let paper = Paper::named(&name.to_uppercase()).unwrap();
        assert_eq!(paper.name(), Some(name));
        assert!(paper.height > paper.width && paper.margin == 36.0);
    }
    assert!(Paper::named("foolscap").is_none() && SMALL.name().is_none());

    // Brackets that name nothing of ours stand as they are; a part longer than its share of
    // the line is cut, and the parts do not run into each other.
    let odd = Setup {
        header: [part("[nothing] [ [Page"), part("a]b[]"), part("[Page]]")],
        footer: [words(40), words(40), words(40)],
        ..Setup::from(SMALL)
    };
    let printed = set(&odd, &doc(vec![Block::Para(para("body"))]));
    let page = &printed.pages[0];
    assert!(said(&on(page, 20.0 + 4.0 + 6.4)).starts_with("[nothing]"));
    let roomy = set(&Setup { paper: Paper::A4, margins: [36.0; 4], ..odd.clone() }, &doc(vec![Block::Para(para("body"))]));
    assert_eq!(said(&on(&roomy.pages[0], 36.0 + 4.0 + 6.4)), "[nothing] [ [Pagea]b[]1]");
    let foot = on(page, 300.0 - 20.0 - 4.0);
    assert!(foot.len() > 6 && foot.windows(2).all(|w| w[1].0 >= w[0].1 - 0.01), "{foot:?}");
    assert!(near(foot[0].0, 20.0) && foot.last().unwrap().1 <= 180.01);
    within(&printed, SMALL);

    // Hostile: margins and sheets that are no sizes, headers of any length, names without end.
    let wild = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1e30, 1e30, 0.0, 5000.0];
    let brackets = "[".repeat(60_000) + &"]".repeat(60_000);
    let many: Vec<(String, String)> = (0..5000).map(|i| (format!("n{i}"), "[n0]".repeat(5000))).collect();
    for (i, a) in wild.iter().enumerate() {
        for b in wild {
            let setup = Setup {
                paper: Paper { width: *a, height: b, margin: *a },
                landscape: i % 2 == 0,
                margins: [*a, b, b, *a],
                header: [brackets.clone(), "[n0][n4999][Page]".repeat(4000), "\u{0}\n\t[Pages]".into()],
                footer: ["[n1]".repeat(9000), brackets.clone(), "x".repeat(100_000)],
                details: i % 3 == 0,
                edges: i % 2 == 1,
                words: many.clone(),
            };
            let printed = set(&setup, &doc(vec![Block::Para(para(&words(60)))]));
            assert!(!printed.pages.is_empty());
            for page in &printed.pages {
                assert!(page.width >= 72.0 && page.height >= 72.0 && page.width <= 14_400.0 && page.height <= 14_400.0);
                for text in &page.texts {
                    assert!(text.x.is_finite() && text.y.is_finite() && text.x >= -0.01 && text.x + text.width <= page.width + 0.01, "{text:?}");
                    assert!(text.y >= 0.0 && text.y <= page.height, "{text:?}");
                }
            }
        }
    }
}

fn list_on(listed: &Listed, paper: impl Into<Setup>) -> Result<Printed, LayoutError> {
    let library = Library::empty();
    let mut fonts = Fonts::new(&library);
    fonts.give("Segoe UI", false, false, Face { data: boxes::font(CHARS), index: 0 });
    list(listed, paper, &mut fonts)
}

#[test]
fn a_list_is_a_table_whose_names_stand_on_every_page() {
    let columns = [
        Column { name: "From", share: 30, right: false },
        Column { name: "Subject", share: 50, right: false },
        Column { name: "Size", share: 20, right: true },
    ];
    let rows: Vec<Row> =
        (0..40).map(|n| Row { cells: vec![format!("who{n}"), format!("about{n}"), format!("{n}")], bold: n == 3, heading: false }).collect();
    let listed = Listed { owner: "Owner", title: "Inbox", note: "40 items", columns: &columns, rows: &rows };
    let printed = list_on(&listed, SMALL).unwrap();
    within(&printed, SMALL);
    assert!(printed.pages.len() > 2 && !printed.truncated, "{} pages", printed.pages.len());

    // The first page is headed by the owner, the folder and the note; every page has the names
    // of the columns over its rows, and every row is on some page once, in order.
    let lines: Vec<Vec<String>> = printed.pages.iter().map(|p| p.text().lines().map(str::to_string).collect()).collect();
    assert_eq!(lines[0][..4], ["Owner", "Inbox", "40 items", "FromSubjectSize"]);
    let mut seen = Vec::new();
    for (n, page) in lines.iter().enumerate() {
        let names = page.iter().position(|l| l == "FromSubjectSize").unwrap_or_else(|| panic!("page {n}: {page:?}"));
        assert_eq!(page.iter().filter(|l| *l == "FromSubjectSize").count(), 1);
        assert_eq!(names, if n == 0 { 3 } else { 0 }, "page {n}");
        seen.extend(page[names + 1..page.len() - 1].iter().cloned());
        assert_eq!(page.last().unwrap(), &format!("{} of {}", n + 1, lines.len()));
    }
    assert_eq!(seen, (0..40).map(|n| format!("who{n}about{n}{n}")).collect::<Vec<_>>());

    // Columns stand at the same places on every page: at their shares of the 160 of room.
    let at =
        |page: &Page, first: char| page.texts.iter().filter(|t| t.glyphs.first().is_some_and(|g| g.1 == first)).map(|t| t.x).fold(f32::MAX, f32::min);
    for page in &printed.pages {
        assert!((at(page, 'F') - at(&printed.pages[0], 'F')).abs() < 0.01);
        assert!((at(page, 'w') - at(page, 'F')).abs() < 0.01);
        assert!((at(page, 'a') - at(page, 'F') - 48.0).abs() < 0.01, "{}", at(page, 'a') - at(page, 'F'));
    }
    // Sizes end at the right edge of their column, and the unread row is thickened.
    let first = &printed.pages[0];
    let ends: Vec<f32> =
        first.texts.iter().filter(|t| t.size > 8.2 && t.glyphs.iter().all(|g| g.1.is_ascii_digit())).map(|t| t.x + t.width).collect();
    assert!(ends.len() > 2 && ends.iter().all(|e| (e - ends[0]).abs() < 0.01), "{ends:?}");
    let bold = |word: &str| {
        printed.pages.iter().flat_map(|p| &p.texts).any(|t| t.bold && t.glyphs.iter().map(|g| g.1).collect::<String>().starts_with(word))
    };
    assert!(bold("who3") && bold("From") && bold("Inbox") && !bold("who4"));
    // A line under each row: black under the names, grey under the rest.
    assert!(first.rules.iter().any(|r| r.color == [0, 0, 0] && r.width < 1.0));
    assert!(first.rules.iter().filter(|r| r.color == [200, 200, 200]).count() >= 3);

    // The setup is followed, and the folder's name can stand in a header.
    let mut setup = Setup::from(SMALL);
    setup.header = [String::new(), "[Folder]".into(), String::new()];
    setup.words = vec![("Folder".into(), "Inbox".into())];
    let headed = list_on(&listed, setup).unwrap();
    for (n, page) in headed.pages.iter().enumerate() {
        assert_eq!(page.text().lines().filter(|l| *l == "Inbox").count(), if n == 0 { 2 } else { 1 }, "page {n}");
    }
}

#[test]
fn a_heading_in_a_list_stands_across_every_column() {
    let columns = [
        Column { name: "From", share: 30, right: false },
        Column { name: "Subject", share: 50, right: false },
        Column { name: "Size", share: 20, right: true },
    ];
    let mut rows: Vec<Row> = Vec::new();
    for n in 0..12 {
        if n % 4 == 0 {
            // More texts than the first are not printed in a heading; none is an empty one.
            rows.push(Row {
                cells: vec![format!("group{n} has a name longer than the first column is wide"), "not this".into()],
                bold: false,
                heading: true,
            });
        }
        rows.push(Row { cells: vec![format!("who{n}"), format!("about{n}"), format!("{n}")], bold: false, heading: false });
    }
    rows.push(Row { cells: Vec::new(), bold: false, heading: true });
    let listed = Listed { owner: "Owner", title: "Inbox", note: "", columns: &columns, rows: &rows };
    let printed = list_on(&listed, Paper::A4).unwrap();
    within(&printed, Paper::A4);
    let page = &printed.pages[0];
    let text = page.text();
    let lines: Vec<&str> = text.lines().collect();
    // Each heading is a line of its own, whole, before the rows of its group.
    let at = |want: &str| lines.iter().position(|l| *l == want).unwrap_or_else(|| panic!("{want} in {text}"));
    for n in [0, 4, 8] {
        let head = at(&format!("group{n} has a name longer than the first column is wide"));
        assert_eq!(lines[head + 1], format!("who{n}about{n}{n}"), "{text}");
    }
    assert!(!text.contains("not this"));
    // It is bold, begins where the first column does, and has a fill behind it as wide as
    // the table: the rows have none.
    let head = page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>().starts_with("group4")).unwrap();
    let who = page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>().starts_with("who4")).unwrap();
    assert!(head.bold && !who.bold && (head.x - who.x).abs() < 0.01);
    let fills: Vec<_> = page.fills.iter().filter(|f| f.color == [238, 238, 238]).collect();
    assert_eq!(fills.len(), 4, "three groups and the empty heading");
    let room = Paper::A4.width - 2.0 * 36.0;
    assert!(
        fills.iter().all(|f| f.width > room * 0.9 && f.y < head.y + 40.0 || f.width > room * 0.9),
        "{:?}",
        fills.iter().map(|f| f.width).collect::<Vec<_>>()
    );
    assert!(fills.iter().any(|f| f.y <= head.y && head.y <= f.y + f.height));
}

#[test]
fn hostile_lists_are_printed_within_the_limits() {
    let wide: Vec<Column> = (0..40).map(|_| Column { name: "N", share: 255, right: true }).collect();
    let none: [Column; 0] = [];
    let zero = [Column { name: "", share: 0, right: false }];
    let long = "x".repeat(100_000);
    let rows = vec![
        Row { cells: Vec::new(), bold: true, heading: false },
        Row { cells: vec![long.clone(); 50], bold: false, heading: false },
        Row { cells: vec!["\u{0}\u{7}\n\t".into(), "a b c".into()], bold: false, heading: false },
    ];
    for columns in [&wide[..], &none[..], &zero[..]] {
        let printed = list_on(&Listed { owner: &long, title: &long, note: "\n", columns, rows: &rows }, SMALL).unwrap();
        within(&printed, SMALL);
        assert!(!printed.pages.is_empty() && printed.pages.len() < 200, "{}", printed.pages.len());
        // No cell holds more than its 300 characters.
        let glyphs: usize = printed.pages.iter().flat_map(|p| &p.texts).filter(|t| t.size < 9.0).map(|t| t.glyphs.len()).sum();
        assert!(glyphs <= 12 * 300 * 2 + 400, "{glyphs}");
    }
    // More rows than may be printed: the list says so and stops.
    let many: Vec<Row> = (0..MAX_ROWS + 5).map(|n| Row { cells: vec![format!("{n}")], bold: false, heading: false }).collect();
    let columns = [Column { name: "N", share: 100, right: false }];
    let printed = list_on(&Listed { owner: "Owner", title: "Inbox", note: "", columns: &columns, rows: &many }, Paper::A4).unwrap();
    assert!(printed.truncated);
    let text: String = printed.pages.iter().map(|p| p.text()).collect::<Vec<_>>().join("\n");
    assert!(text.lines().any(|l| l == (MAX_ROWS - 1).to_string()) && !text.lines().any(|l| l == MAX_ROWS.to_string()));
    assert!(printed.pages.len() < MAX_PAGES);
}

/// Latin, Hebrew and Arabic letters, and the shapes Arabic letters take; beh has all four, lam
/// with alef its one.
const BOTH_WAYS: &str =
    "abcdefgh12() \u{5d0}\u{5d1}\u{5d2}\u{5d3}\u{5d4}\u{5d5}\u{627}\u{628}\u{644}\u{fe8e}\u{fe8f}\u{fe90}\u{fe91}\u{fe92}\u{fefb}";

/// The glyphs of the first line of the body, from the left as they are drawn.
fn glyphs_of(printed: &Printed) -> Vec<u16> {
    let page = &printed.pages[0];
    let y = page.texts.iter().map(|t| t.y).find(|y| *y > SMALL.margin + 20.0).unwrap();
    let mut line: Vec<&Text> = page.texts.iter().filter(|t| (t.y - y).abs() < 0.01).collect();
    line.sort_by(|a, b| a.x.total_cmp(&b.x));
    line.iter().flat_map(|t| t.glyphs.iter().map(|(glyph, _)| *glyph)).collect()
}

/// The lines of the body as they are seen: the characters of the glyphs from the left.
fn seen_lines(page: &Page) -> Vec<String> {
    let mut lines: Vec<(f32, Vec<&Text>)> = Vec::new();
    for text in &page.texts {
        match lines.last_mut() {
            Some((y, line)) if (*y - text.y).abs() < 0.01 => line.push(text),
            _ => lines.push((text.y, vec![text])),
        }
    }
    let mut seen: Vec<String> = Vec::new();
    for (_, mut line) in lines {
        line.sort_by(|a, b| a.x.total_cmp(&b.x));
        seen.push(line.iter().flat_map(|t| t.glyphs.iter().map(|(_, ch)| *ch)).collect());
    }
    seen.pop();
    if seen.first().is_some_and(|l| l == "Owner") {
        seen.remove(0);
    }
    seen
}

#[test]
fn right_to_left_text_is_printed_from_the_right() {
    let rtl = |text: &str| Block::Para(Para { rtl: true, align: Align::Right, ..para(text) });
    let body = doc(vec![
        // Hebrew in a left-to-right paragraph: the word is turned, the line is not.
        Block::Para(para("ab \u{5d0}\u{5d1}\u{5d2} cd")),
        // A right-to-left paragraph: the first word at the right, Latin words and the digits after
        // them as they are read.
        rtl("\u{5d0}\u{5d1} ab 12 \u{5d2}\u{5d3}"),
        // A bracket opens towards what it holds.
        rtl("\u{5d0}(\u{5d1})"),
        // Every line is turned by itself: the first words stay on the first line.
        rtl(
            "\u{5d0}\u{5d0}\u{5d0}\u{5d0}\u{5d0}\u{5d0}\u{5d0}\u{5d0} \u{5d1}\u{5d1}\u{5d1}\u{5d1}\u{5d1}\u{5d1}\u{5d1}\u{5d1} \u{5d2}\u{5d2}\u{5d2}\u{5d2}\u{5d2}\u{5d2}\u{5d2}\u{5d2} \u{5d3}\u{5d3}\u{5d3}\u{5d3}\u{5d3}\u{5d3}\u{5d3}\u{5d3}",
        ),
        Block::Para(para("plain")),
    ]);
    let printed = print_in(boxes::font(BOTH_WAYS), &body, SMALL);
    within(&printed, SMALL);
    // What a page says is what is read, whichever way it is drawn: text copied from the page
    // or looked for in it is in that order.
    let read = body_lines(&printed.pages[0]);
    assert_eq!(read[0], "ab \u{5d0}\u{5d1}\u{5d2} cd");
    assert_eq!(read[1], "\u{5d0}\u{5d1} ab 12 \u{5d2}\u{5d3}");
    assert_eq!(read[2], "\u{5d0}(\u{5d1})");
    assert_eq!(read[5], "plain");
    let lines = seen_lines(&printed.pages[0]);
    assert_eq!(lines[0], "ab \u{5d2}\u{5d1}\u{5d0} cd");
    assert_eq!(lines[1], "\u{5d3}\u{5d2} ab 12 \u{5d1}\u{5d0}");
    // A bracket is said as it is seen, as its glyph is: a reader of the file that turns the
    // glyphs round turns the brackets with them. Read as the page reads it, it is the one stored.
    assert_eq!(lines[2], "(\u{5d1})\u{5d0}");
    // Text written from the right says nothing more than its glyphs for being that: a reader
    // that was tried turned a line round twice when the line said how it is read.
    let turned: Vec<&Text> = printed.pages[0].texts.iter().filter(|t| t.rtl).collect();
    assert!(turned.len() >= 4 && turned.iter().all(|t| t.actual.is_empty()), "{turned:?}");
    assert!(turned.iter().any(|t| t.said() == "\u{5d0}(\u{5d1})"));
    assert_eq!(
        lines[3],
        "\u{5d2}\u{5d2}\u{5d2}\u{5d2}\u{5d2}\u{5d2}\u{5d2}\u{5d2} \u{5d1}\u{5d1}\u{5d1}\u{5d1}\u{5d1}\u{5d1}\u{5d1}\u{5d1} \u{5d0}\u{5d0}\u{5d0}\u{5d0}\u{5d0}\u{5d0}\u{5d0}\u{5d0}"
    );
    assert_eq!(lines[4], "\u{5d3}\u{5d3}\u{5d3}\u{5d3}\u{5d3}\u{5d3}\u{5d3}\u{5d3}");
    assert_eq!(lines[5], "plain");
    let page = &printed.pages[0];
    let (open, close) = (BOTH_WAYS.chars().position(|c| c == '(').unwrap(), BOTH_WAYS.chars().position(|c| c == ')').unwrap());
    let brackets: Vec<(u16, char)> = page.texts.iter().flat_map(|t| t.glyphs.iter().copied()).filter(|(_, c)| matches!(c, '(' | ')')).collect();
    let glyph_of = |c: char| brackets.iter().find(|(_, ch)| *ch == c).unwrap().0;
    // The glyph that is drawn is the one of the bracket that is seen.
    assert_eq!(i32::from(glyph_of(')')) - i32::from(glyph_of('(')), close as i32 - open as i32);
    // A right-to-left line ends at the right margin.
    let has = |t: &&Text| t.glyphs.iter().any(|(_, c)| *c == '\u{5d3}');
    let end_of = |y: f32| page.texts.iter().filter(|t| (t.y - y).abs() < 0.01).map(|t| t.x + t.width).fold(0.0, f32::max);
    let ends: Vec<f32> = page.texts.iter().filter(has).map(|t| end_of(t.y)).collect();
    assert!(ends.iter().all(|end| (end - (SMALL.width - SMALL.margin)).abs() < 0.01), "{ends:?}");
}

#[test]
fn arabic_letters_are_printed_joined_where_the_font_has_the_shapes() {
    let one = |text: &str, font: &str| glyphs_of(&print_in(boxes::font(font), &doc(vec![Block::Para(Para { rtl: true, ..para(text) })]), SMALL));
    // Beh three times is its shape joined to the letter after, to both and to the one before:
    // drawn from the right, so the first read stands last.
    assert_eq!(one("\u{628}\u{628}\u{628}", BOTH_WAYS), one("\u{fe91}\u{fe92}\u{fe90}", BOTH_WAYS));
    assert_eq!(one("\u{628}\u{628}\u{628}", BOTH_WAYS).len(), 3);
    // Lam and alef are one glyph.
    assert_eq!(one("\u{644}\u{627}", BOTH_WAYS), one("\u{fefb}", BOTH_WAYS));
    // The letters join across a change of look.
    let bold = Run { style: Style { bold: true, ..Style::default() }, ..run("\u{628}") };
    let mixed = doc(vec![Block::Para(Para { runs: vec![run("\u{628}"), bold], rtl: true, ..Para::default() })]);
    assert_eq!(glyphs_of(&print_in(boxes::font(BOTH_WAYS), &mixed, SMALL)), one("\u{fe91}\u{fe90}", BOTH_WAYS));
    // A font without the shapes prints the letters as they are stored.
    let bare = "ab \u{627}\u{628}\u{644}";
    let plain = one("\u{628}\u{644}\u{627}", bare);
    assert_eq!(plain.len(), 3);
    assert_eq!(plain.iter().collect::<std::collections::HashSet<_>>().len(), 3);
}

#[test]
fn hostile_right_to_left_text_never_panics() {
    let turns = "\u{202e}".repeat(300) + "ab\u{5d0}" + &"\u{202c}".repeat(400);
    let body = doc(vec![
        Block::Para(Para { rtl: true, ..para(&turns) }),
        Block::Para(Para { rtl: true, ..para("") }),
        Block::Para(Para { rtl: true, tabs: Vec::new(), ..para("\t\u{5d0}\t\n\n\u{644}") }),
        Block::Para(Para {
            rtl: true,
            kind: ParaKind::Item("1.".into()),
            indent: 3,
            quote: 2,
            ..para("\u{644}\u{644}\u{644}\u{627}\u{627} \u{200f}\u{200e}(((")
        }),
        Block::Para(Para {
            runs: vec![run("\u{644}"), Run { text: "x".into(), picture: Some(Picture::Remote(String::new())), ..Run::default() }, run("\u{627}")],
            rtl: true,
            ..Para::default()
        }),
        Block::Para(para(&"\u{5d0}\u{628} ".repeat(3000))),
    ]);
    let printed = print_in(boxes::font(BOTH_WAYS), &body, SMALL);
    within(&printed, SMALL);
    assert!(!printed.pages.is_empty());
}

#[test]
fn text_at_the_right_of_a_narrow_cell_stays_inside_it() {
    // A cell narrower than the least a text wraps at: its text ends where the cell does, not
    // where that least would.
    let right = |text: &str| Cell { blocks: vec![Block::Para(Para { align: Align::Right, ..para(text) })], span: 1, rows: 1, ..Cell::default() };
    let table = Table { rows: vec![vec![right("on"), right("tw"), cell("three")]], border: true, padding: 1, ..Table::default() };
    let printed = print(&doc(vec![Block::Table(table)]), Paper::A4);
    let page = &printed.pages[0];
    let find = |text: &str| page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap();
    let (one, two, three) = (find("on"), find("tw"), find("three"));
    assert!(one.x + one.width <= two.x && two.x + two.width <= three.x, "{one:?} {two:?} {three:?}");
}

/// Letters, two glyphs that stand for more than one letter ("fi" is drawn as the glyph of `1`,
/// "ffl" as that of `2`), an accent the font lifts over its letter, and beh with its shapes.
const SHAPED: &str = "abcdefil12 \u{301}\u{5d0}\u{5d1}\u{5d2}\u{628}\u{fe8f}\u{fe90}\u{fe91}\u{fe92}";

fn shaped_font() -> Vec<u8> {
    boxes::font_shaped(
        SHAPED,
        &[
            ("liga", "ffl", '2'),
            ("liga", "fi", '1'),
            ("isol", "\u{628}", '\u{fe8f}'),
            ("fina", "\u{628}", '\u{fe90}'),
            ("init", "\u{628}", '\u{fe91}'),
            ("medi", "\u{628}", '\u{fe92}'),
        ],
        &[('\u{301}', -300, 150)],
    )
}

fn glyph_in(chars: &str, ch: char) -> u16 {
    chars.chars().position(|c| c == ch).unwrap() as u16 + 1
}

#[test]
fn letters_are_drawn_with_the_glyphs_the_font_s_own_tables_give() {
    let printed = print_in(shaped_font(), &doc(vec![Block::Para(para("afib baffle"))]), SMALL);
    within(&printed, SMALL);
    // "fi" is one glyph and "ffl" is one: seven glyphs and two spaces... of eleven characters.
    let glyphs = glyphs_of(&printed);
    let of = |text: &str| -> Vec<u16> { text.chars().map(|c| glyph_in(SHAPED, c)).collect() };
    assert_eq!(glyphs, of("a1b ba2e"));
    // The line is as wide as its glyphs, not as its characters.
    let page = &printed.pages[0];
    let line: Vec<&Text> = page.texts.iter().filter(|t| t.glyphs.iter().any(|(g, _)| *g == glyph_in(SHAPED, '2'))).collect();
    assert_eq!(line.len(), 1);
    let size = line[0].size;
    let wide = (7.0 * f32::from(boxes::ADVANCE) + f32::from(boxes::SPACE)) / f32::from(boxes::UNITS) * size;
    assert!((line[0].width - wide).abs() < 0.01, "{} {wide}", line[0].width);
    // What the page says is what was written.
    assert_eq!(body_lines(page)[0], "afib baffle");
    // Only the glyphs that are several letters say so, each for itself.
    let said: Vec<(usize, usize, &str)> = line[0].actual.iter().map(|s| (s.from, s.count, s.text.as_str())).collect();
    assert_eq!(said, [(1, 1, "fi"), (6, 1, "ffl")]);
    assert!(!line[0].rtl);
    // A line with nothing of the kind says nothing more than its glyphs do.
    let printed = print_in(shaped_font(), &doc(vec![Block::Para(para("abc def"))]), SMALL);
    assert!(printed.pages[0].texts.iter().all(|t| t.actual.is_empty() && t.lift == 0.0));
    assert_eq!(body_lines(&printed.pages[0])[0], "abc def");

    // A word too long for a line is still cut, between glyphs.
    let long = "fi".repeat(400);
    let printed = print_in(shaped_font(), &doc(vec![Block::Para(para(&long))]), SMALL);
    within(&printed, SMALL);
    let read: String = printed.pages.iter().flat_map(body_lines).collect();
    assert_eq!(read, long);
}

#[test]
fn a_glyph_stands_where_the_font_puts_it() {
    let printed = print_in(shaped_font(), &doc(vec![Block::Para(para("ae\u{301}b"))]), SMALL);
    within(&printed, SMALL);
    let page = &printed.pages[0];
    let accent = page.texts.iter().find(|t| t.glyphs.iter().any(|(g, _)| *g == glyph_in(SHAPED, '\u{301}'))).unwrap();
    let letters: Vec<&Text> = page.texts.iter().filter(|t| (t.y - accent.y).abs() < 0.01 && !std::ptr::eq(*t, accent)).collect();
    let (size, left) = (accent.size, letters.iter().map(|t| t.x).fold(f32::MAX, f32::min));
    // Three tenths of the size back from where the line had got to after "ae", and lifted.
    assert!((accent.x - (left + size - 0.3 * size)).abs() < 0.01, "{} {left} {size}", accent.x);
    assert!((accent.lift - 0.15 * size).abs() < 0.01);
    assert_eq!(accent.glyphs.len(), 1);
    // The text is read once, in order, whatever pieces it is drawn in.
    assert_eq!(body_lines(page)[0], "ae\u{301}b");
}

#[test]
fn arabic_letters_are_joined_by_the_font_s_own_tables() {
    let font = shaped_font;
    let one = |body: &Doc| glyphs_of(&print_in(font(), body, SMALL));
    let of = |text: &str| -> Vec<u16> { text.chars().map(|c| glyph_in(SHAPED, c)).collect() };
    // Drawn from the right: the shape that ends a word stands first from the left.
    let three = doc(vec![Block::Para(Para { rtl: true, ..para("\u{628}\u{628}\u{628}") })]);
    assert_eq!(one(&three), of("\u{fe90}\u{fe92}\u{fe91}"));
    assert_eq!(one(&doc(vec![Block::Para(Para { rtl: true, ..para("\u{628}") })])), of("\u{fe8f}"));
    // The letters join across a change of look: each piece is shaped knowing its neighbour.
    let bold = Run { style: Style { bold: true, ..Style::default() }, ..run("\u{628}") };
    let mixed = doc(vec![Block::Para(Para { runs: vec![run("\u{628}"), bold, run("\u{628}")], rtl: true, ..Para::default() })]);
    assert_eq!(one(&mixed), of("\u{fe90}\u{fe92}\u{fe91}"));
    // What is read is the letters as they were written, not the shapes.
    let printed = print_in(font(), &three, SMALL);
    assert_eq!(body_lines(&printed.pages[0])[0], "\u{628}\u{628}\u{628}");
    // Hebrew in the same font is turned as before.
    let printed = print_in(font(), &doc(vec![Block::Para(para("ab \u{5d0}\u{5d1}\u{5d2} cd"))]), SMALL);
    assert_eq!(seen_lines(&printed.pages[0])[0], "ab \u{5d2}\u{5d1}\u{5d0} cd");
    assert_eq!(body_lines(&printed.pages[0])[0], "ab \u{5d0}\u{5d1}\u{5d2} cd");
}

#[test]
fn hostile_shaping_tables_never_panic() {
    let body = doc(vec![
        Block::Para(para(&("fi\u{301}".repeat(2000) + &"\u{301}".repeat(3000)))),
        Block::Para(Para { rtl: true, ..para(&"\u{628}\u{301}fi \u{200d}\u{200c}".repeat(500)) }),
        Block::Para(para("\u{301}")),
        Block::Para(para("f\ti\nf\u{ad}i")),
    ]);
    let whole = shaped_font();
    let good = print_in(whole.clone(), &body, SMALL);
    within(&good, SMALL);
    // Bytes turned over anywhere inside the two tables text is shaped by.
    let table = |tag: &[u8; 4]| {
        let entry = whole.windows(4).position(|w| w == tag).unwrap();
        let word = |at: usize| u32::from_be_bytes(whole[at..at + 4].try_into().unwrap()) as usize;
        word(entry + 8)..word(entry + 8) + word(entry + 12)
    };
    let tables = [table(b"GSUB"), table(b"GPOS")];
    let mut state = 0x2545_f491u32;
    for round in 0..300usize {
        let mut font = whole.clone();
        let inside = &tables[round % 2];
        for _ in 0..=(round % 5) {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let at = inside.start + (state >> 8) as usize % inside.len();
            font[at] = (state >> 3) as u8;
        }
        let printed = print_in(font, &doc(vec![Block::Para(para("afib\u{301} baffle \u{628}\u{628}\u{5d0}"))]), SMALL);
        within(&printed, SMALL);
    }
    // Moves that are as far as a font can say.
    let far = boxes::font_shaped(SHAPED, &[("liga", "fi", '1')], &[('a', i16::MAX, i16::MIN), ('b', i16::MIN, i16::MAX)]);
    let printed = print_in(far, &doc(vec![Block::Para(para(&"ab fi ".repeat(200)))]), SMALL);
    assert!(!printed.pages.is_empty());
}

#[test]
fn a_character_the_italic_face_lacks_comes_from_the_plain_face_leant() {
    let library = Library::empty();
    let mut fonts = Fonts::new(&library);
    for family in ["Segoe UI", "Times New Roman", "Calibri"] {
        fonts.give(family, false, false, Face { data: boxes::font("ab \u{5d0}"), index: 0 });
        fonts.give(family, false, true, Face { data: boxes::font("ab "), index: 0 });
    }
    let italic = Run { style: Style { italic: true, ..Style::default() }, ..run("a\u{5d0}") };
    let body = doc(vec![Block::Para(Para { runs: vec![italic], ..Para::default() })]);
    let printed = memo(&Memo { owner: "Owner", fields: &[], body: &body, plain: false, sizes: &|_| None }, SMALL, &mut fonts).unwrap();
    let page = &printed.pages[0];
    let of = |ch: char| page.texts.iter().find(|t| t.glyphs.iter().any(|(_, c)| *c == ch)).unwrap();
    // The letter the italic face has is drawn with it, as it is.
    assert!(!of('a').slant && of('a').glyphs.iter().all(|(glyph, _)| *glyph != 0));
    // The one it lacks is no box: the plain face has it, and is leant.
    let hebrew = of('\u{5d0}');
    assert!(hebrew.slant && hebrew.face != of('a').face);
    assert!(hebrew.glyphs.iter().all(|(glyph, _)| *glyph != 0));
}

#[test]
fn what_stands_at_a_page_s_head_and_foot_and_before_an_item_is_shaped_as_the_body_is() {
    let of = |text: &str| -> Vec<u16> { text.chars().map(|c| glyph_in(SHAPED, c)).collect() };
    let part = |s: &str| s.to_string();
    let set = |setup: &Setup, body: &Doc| {
        let library = Library::empty();
        let mut fonts = Fonts::new(&library);
        fonts.give("Segoe UI", false, false, Face { data: shaped_font(), index: 0 });
        fonts.give("Times New Roman", false, false, Face { data: shaped_font(), index: 0 });
        memo(&Memo { owner: "abc", fields: &[], body, plain: false, sizes: &|_| None }, setup, &mut fonts).unwrap()
    };
    // The texts on the line over the body and under it, from the left.
    let line = |page: &Page, over: bool| -> Vec<Text> {
        let y = if over { page.texts.iter().map(|t| t.y).fold(f32::MAX, f32::min) } else { page.texts.iter().map(|t| t.y).fold(0.0, f32::max) };
        let mut line: Vec<Text> = page.texts.iter().filter(|t| (t.y - y).abs() < 0.01).cloned().collect();
        line.sort_by(|a, b| a.x.total_cmp(&b.x));
        line
    };
    let glyphs = |line: &[Text]| -> Vec<u16> { line.iter().flat_map(|t| t.glyphs.iter().map(|(g, _)| *g)).collect() };
    let read = |line: &[Text]| -> String { line.iter().map(Text::said).collect() };
    let body = doc(vec![Block::Para(para("abc"))]);
    let paper = Paper { width: 400.0, height: 300.0, margin: 20.0 };

    // Letters drawn as one glyph are; Arabic letters join; Hebrew is drawn from the right in
    // a line that is not; a line that starts with it runs from the right.
    let setup = Setup {
        header: [part("baffle fi"), part("\u{628}\u{628}\u{628}"), part("ab \u{5d0}\u{5d1}\u{5d2} cd")],
        footer: [part("\u{5d0}\u{5d1} ab [Page]"), String::new(), String::new()],
        ..Setup::from(paper)
    };
    let printed = set(&setup, &body);
    let head = line(&printed.pages[0], true);
    assert_eq!(glyphs(&head), of("ba2e 1\u{fe90}\u{fe92}\u{fe91}ab \u{5d2}\u{5d1}\u{5d0} cd"), "{head:?}");
    // What the page says is what was asked for, in the order it is read.
    let texts = &printed.pages[0].texts;
    let says = |what: &str| texts.iter().any(|t| t.said() == what);
    assert!(says("baffle fi") && says("\u{628}\u{628}\u{628}"), "{texts:?}");
    let foot = line(&printed.pages[0], false);
    // A number after Latin letters is read with them (UAX #9, W7).
    assert_eq!(glyphs(&foot), of("ab 1 \u{5d1}\u{5d0}"));
    // Each part stands where it stood: at the left, in the middle, at the right.
    let right = head.last().unwrap();
    assert!((right.x + right.width - 380.0).abs() < 0.01 && (head[0].x - 20.0).abs() < 0.01, "{head:?}");

    // Too long for its share of the line, a part loses its end as it is read, whichever way
    // it is drawn, and stays in its share.
    let long_ltr = "fi ".repeat(60);
    let long_rtl = "\u{5d0}\u{5d1}\u{5d2} ".repeat(60);
    let setup = Setup {
        header: [long_ltr.clone(), String::new(), long_rtl.clone()],
        footer: [String::new(), String::new(), String::new()],
        ..Setup::from(paper)
    };
    let printed = set(&setup, &body);
    let head = line(&printed.pages[0], true);
    let (left, right): (Vec<Text>, Vec<Text>) = head.into_iter().partition(|t| t.x < 200.0);
    assert!(left.iter().all(|t| t.x >= 19.99 && t.x + t.width <= 200.0) && right.iter().all(|t| t.x >= 200.0 && t.x + t.width <= 380.01));
    let start = read(&left);
    assert!(start.ends_with('\u{2026}') && long_ltr.starts_with(start.trim_end_matches('\u{2026}')) && start.len() > 20, "{start}");
    // The start of the Hebrew is what is kept, and it stands at the right edge.
    let kept: Vec<u16> = glyphs(&right);
    assert!(kept.len() > 20 && kept.ends_with(&of("\u{5d2}\u{5d1}\u{5d0}")), "{kept:?}");
    let last = right.iter().map(|t| t.x + t.width).fold(0.0, f32::max);
    assert!((last - 380.0).abs() < 0.01, "{last}");

    // A marker is set the same way, and ends where it ended.
    let item = |marker: &str| Para { kind: ParaKind::Item(marker.into()), ..para("abc") };
    let printed = set(&Setup::from(paper), &doc(vec![Block::Para(item("fi.")), Block::Para(item("\u{5d0}\u{5d1}"))]));
    let page = &printed.pages[0];
    let with = |glyph: char| page.texts.iter().find(|t| t.glyphs.iter().any(|(g, _)| *g == glyph_in(SHAPED, glyph))).unwrap();
    let (one, two) = (with('1'), with('\u{5d0}'));
    assert_eq!((one.said(), one.actual.len()), ("fi.".to_string(), 1));
    assert_eq!(two.glyphs.iter().map(|(g, _)| *g).collect::<Vec<u16>>(), of("\u{5d1}\u{5d0}"));
    assert!((two.x + two.width - (one.x + one.width)).abs() < 0.3, "{one:?} {two:?}");

    // A body that filled what a print may hold still has its pages numbered, and junk at the
    // head of a page is set or left out, never more.
    for junk in [
        "\u{202e}\u{202e}a\u{5d0}\u{202c}",
        "\u{7}\n\t\u{0}",
        "\u{10ffff}\u{e000}zzz",
        &"\u{628}\u{301}".repeat(5000),
        "[Page][Pages][Page of Pages][nothing]",
    ] {
        for size in [Paper { width: 60.0, height: 90.0, margin: 20.0 }, paper] {
            let setup =
                Setup { header: [part(junk), part(junk), part(junk)], footer: [part(junk), String::new(), part("[Page]")], ..Setup::from(size) };
            let printed = set(&setup, &doc(vec![Block::Para(item(junk))]));
            for page in &printed.pages {
                assert!(
                    page.texts.iter().all(|t| t.x.is_finite()
                        && t.width.is_finite()
                        && (size.width < 100.0 || (t.x >= -0.01 && t.x + t.width <= size.width + 0.01))),
                    "{junk:?} {:?}",
                    page.texts.iter().map(|t| (t.x, t.width, t.y, t.glyphs.len())).collect::<Vec<_>>()
                );
            }
        }
    }
}

#[test]
fn a_paragraph_written_from_the_right_is_set_in_at_the_right() {
    let paper = Paper { width: 400.0, height: 300.0, margin: 20.0 };
    let item = |rtl: bool| Para {
        rtl,
        align: if rtl { Align::Right } else { Align::Left },
        indent: 1,
        quote: 2,
        kind: ParaKind::Item("1.".into()),
        ..para(&"word ".repeat(40))
    };
    let set = |rtl: bool| print(&doc(vec![Block::Para(item(rtl))]), paper);
    let (ltr, rtl) = (set(false), set(true));
    // The lines of the item, its marker and its bars, without the head and foot of the page.
    let parts = |printed: &Printed, rtl: bool| {
        let page = &printed.pages[0];
        let said = |t: &Text| t.glyphs.iter().map(|(_, c)| *c).collect::<String>();
        let lines: Vec<(f32, f32)> = page.texts.iter().filter(|t| said(t).starts_with("word")).map(|t| (t.x, t.x + t.width)).collect();
        // Beside an item read from the right, the stop stands at the left of its number: a
        // piece of its own.
        let of = |what: &str| page.texts.iter().find(|t| said(t) == what).map(|t| (t.x, t.x + t.width, t.y));
        let marker = if rtl {
            let (stop, number) = (of(".").unwrap(), of("1").unwrap());
            assert!((stop.1 - number.0).abs() < 0.01 && stop.2 == number.2, "{stop:?} {number:?}");
            (stop.0, number.1, stop.2)
        } else {
            of("1.").unwrap()
        };
        let first = page.texts.iter().find(|t| said(t).starts_with("word")).unwrap().y;
        let bars: Vec<f32> = page.rules.iter().filter(|r| r.from.0 == r.to.0).map(|r| r.from.0).collect();
        // A bar is drawn line by line.
        let mut bars = bars;
        bars.sort_by(f32::total_cmp);
        bars.dedup();
        (lines, marker, first, bars)
    };
    let (l_lines, l_marker, l_first, l_bars) = parts(&ltr, false);
    let (r_lines, r_marker, r_first, r_bars) = parts(&rtl, true);
    assert!(l_lines.len() > 1 && l_lines.len() == r_lines.len());
    // Set in by the same room, on the other side: the picture in a mirror.
    let inset = l_lines[0].0 - 20.0;
    assert!(inset > 30.0);
    assert!(l_lines.iter().all(|l| (l.0 - 20.0 - inset).abs() < 0.01 && l.1 <= 380.01));
    assert!(r_lines.iter().all(|l| l.0 >= 19.99 && l.1 <= 380.01 - inset), "{r_lines:?}");
    assert!((r_lines[0].1 - (380.0 - inset)).abs() < 0.3, "{r_lines:?}");
    // The marker stands outside the text on the side the item starts at, on its first line.
    assert!(l_marker.1 < l_lines[0].0 && r_marker.0 > r_lines[0].1 && r_marker.1 <= 380.01, "{r_marker:?}");
    assert!((l_lines[0].0 - l_marker.1 - (r_marker.0 - r_lines[0].1)).abs() < 0.3);
    assert_eq!((l_marker.2, r_marker.2), (l_first, r_first));
    // And so do the bars of a quotation.
    assert_eq!((l_bars.len(), r_bars.len()), (2, 2));
    for (l, r) in l_bars.iter().zip(r_bars.iter().rev()) {
        assert!((l - 20.0 - (380.0 - r)).abs() < 0.01, "{l_bars:?} {r_bars:?}");
    }
    // In a cell too narrow for any of it, and with a marker too long, nothing leaves the sheet.
    let long = Para { kind: ParaKind::Item("abcdefghijkl".into()), indent: 12, quote: 8, ..item(true) };
    let narrow = Table {
        rows: vec![vec![Cell { blocks: vec![Block::Para(long.clone())], span: 1, rows: 1, width: 4, ..Cell::default() }, cell("a")]],
        ..Table::default()
    };
    let printed = print(&doc(vec![Block::Para(long), Block::Table(narrow)]), paper);
    for page in &printed.pages {
        assert!(page.texts.iter().all(|t| t.x >= -0.01 && t.x + t.width <= 400.01 && t.x.is_finite()));
    }
}

#[test]
fn text_told_which_way_to_run_is_printed_that_way() {
    let body = doc(vec![
        // Letters written from the left, told to run from the right.
        Block::Para(para("ab \u{202e}cde\u{202c} fg")),
        // Hebrew told to run from the left stands as it is stored; the word before it is turned.
        Block::Para(para("\u{5d0}\u{5d1} \u{202d}\u{5d2}\u{5d3}\u{202c}")),
        // What is set apart takes no room and leaves no mark of its own on the page.
        Block::Para(para("a\u{2068}b\u{2069}c\u{2066}d\u{2069}\u{2067}\u{2069}")),
        // Set apart, the digits do not follow the Hebrew before them as they otherwise would.
        Block::Para(para("\u{5d0} 12")),
        Block::Para(para("\u{2068}\u{5d0}\u{2069} 12")),
    ]);
    let printed = print_in(boxes::font(BOTH_WAYS), &body, SMALL);
    within(&printed, SMALL);
    let lines = seen_lines(&printed.pages[0]);
    assert_eq!(lines[0], "ab edc fg");
    assert_eq!(lines[1], "\u{5d1}\u{5d0} \u{5d2}\u{5d3}");
    assert_eq!(lines[2], "abcd");
    assert_eq!(lines[3], "12 \u{5d0}");
    assert_eq!(lines[4], "\u{5d0} 12");
    // What is read from the page is the text without the marks.
    let read = body_lines(&printed.pages[0]);
    assert_eq!(read[0], "ab cde fg");
    assert_eq!(read[2], "abcd");
}

#[test]
fn what_stands_in_a_paragraph_written_from_the_right_is_counted_from_the_right() {
    use openmailclient_formats::rich::{Leader, Line, Tab, TabKind};
    let paper = Paper { width: 400.0, height: 300.0, margin: 20.0 };
    let said = |t: &Text| t.glyphs.iter().map(|(_, c)| *c).collect::<String>();
    let set = |blocks: Vec<Block>| print_in(boxes::font(BOTH_WAYS), &doc(blocks), paper);
    // Where a piece of text stands: its left and its right.
    let of = |printed: &Printed, what: &str| {
        let text = printed.pages[0].texts.iter().find(|t| said(t) == what).unwrap_or_else(|| panic!("{what} is on the page"));
        (text.x, text.x + text.width, text.y)
    };
    let near = |a: f32, b: f32| (a - b).abs() < 0.05;

    // A tab stop is as far from the right as it is from the left in a paragraph written from
    // the left, and what fills its room stands between the two texts.
    let tabbed = |rtl: bool, text: &str| {
        let stop = Tab { at: 160, kind: TabKind::Left, lead: Leader::Dots };
        Block::Para(Para { rtl, align: if rtl { Align::Right } else { Align::Left }, tabs: vec![stop], ..para(text) })
    };
    let (ltr, rtl) = (set(vec![tabbed(false, "ab\tcd")]), set(vec![tabbed(true, "ab\tcd")]));
    let (first, second) = (of(&ltr, "ab"), of(&ltr, "cd"));
    let (first_r, second_r) = (of(&rtl, "ab"), of(&rtl, "cd"));
    assert!(near(first.0, paper.margin) && near(second.0, paper.margin + 120.0), "{first:?} {second:?}");
    assert!(near(first_r.1, paper.width - first.0), "{first_r:?}");
    assert!(near(second_r.1, paper.width - second.0), "{second_r:?}");
    let dots = |printed: &Printed| printed.pages[0].rules.iter().filter(|r| r.line == Line::Dotted).map(|r| (r.from.0, r.to.0)).collect::<Vec<_>>();
    let (led, led_r) = (dots(&ltr), dots(&rtl));
    assert_eq!((led.len(), led_r.len()), (1, 1));
    assert!(led[0].0 > first.1 && led[0].1 < second.0, "{led:?}");
    assert!(led_r[0].0 > second_r.1 && led_r[0].1 < first_r.0, "{led_r:?}");
    // Hebrew on both sides of a tab in a paragraph written from the left: the first stays first.
    let mixed = set(vec![tabbed(false, "\u{5d0}\u{5d1}\t\u{5d2}\u{5d3}")]);
    let (first_h, second_h) = (of(&mixed, "\u{5d1}\u{5d0}"), of(&mixed, "\u{5d3}\u{5d2}"));
    assert!(near(first_h.0, paper.margin) && near(second_h.0, paper.margin + 120.0), "{first_h:?} {second_h:?}");

    // The columns of a table written from the right are counted from the right, and a cell
    // that covers several stands over the same ones.
    let table = |rtl: bool| {
        let wide = Cell { span: 2, ..cell("gh") };
        let rows = vec![vec![cell("ab"), cell("cd"), cell("ef")], vec![wide, cell("12")]];
        Block::Table(Table { rows, border: true, width: TableWidth::Fill, rtl, ..Table::default() })
    };
    let (ltr, rtl) = (set(vec![table(false)]), set(vec![table(true)]));
    let x = |printed: &Printed, what: &str| of(printed, what).0;
    assert!(x(&ltr, "ab") < x(&ltr, "cd") && x(&ltr, "cd") < x(&ltr, "ef"));
    assert!(x(&rtl, "ab") > x(&rtl, "cd") && x(&rtl, "cd") > x(&rtl, "ef"));
    assert!(near(x(&ltr, "gh"), x(&ltr, "ab")) && near(x(&ltr, "12"), x(&ltr, "ef")));
    assert!(near(x(&rtl, "gh"), x(&rtl, "cd")) && near(x(&rtl, "12"), x(&rtl, "ef")));
    // The lines of the table are where they were: only what stands between them changed sides.
    let uprights = |printed: &Printed| {
        let mut at: Vec<i32> = printed.pages[0].rules.iter().filter(|r| r.from.0 == r.to.0).map(|r| (r.from.0 * 10.0).round() as i32).collect();
        at.sort_unstable();
        at.dedup();
        at
    };
    let mirrored: Vec<i32> = uprights(&ltr).iter().rev().map(|x| (paper.width * 10.0).round() as i32 - x).collect();
    assert_eq!(uprights(&rtl), mirrored);
    within(&rtl, paper);

    // A border set in is set in from the side its paragraphs start at.
    let boxed = |rtl: bool| {
        let edge = Edge { width: 1, color: None, line: Line::Solid };
        let frame = Arc::new(Frame { edges: [edge; 4], padding: [4; 4], indent: 2, between: Edge::default(), fill: None, within: None, radius: 0 });
        Block::Para(Para { rtl, frame: Some(frame), ..para("ab") })
    };
    let sides = |printed: &Printed| {
        let xs: Vec<f32> = printed.pages[0].rules.iter().filter(|r| r.from.0 == r.to.0).map(|r| r.from.0).collect();
        (xs.iter().copied().fold(f32::MAX, f32::min), xs.iter().copied().fold(f32::MIN, f32::max))
    };
    let (ltr, rtl) = (sides(&set(vec![boxed(false)])), sides(&set(vec![boxed(true)])));
    assert!(ltr.0 > rtl.0 + 10.0 && near(ltr.0 - rtl.0, ltr.1 - rtl.1), "{ltr:?} {rtl:?}");
    assert!(near(rtl.0 - paper.margin, paper.width - paper.margin - ltr.1), "{ltr:?} {rtl:?}");

    // Hostile: a table written from the right with spans past its columns, and none.
    let wild = |span: u16, rows: u16| Cell { span, rows, ..cell("a") };
    let rows = vec![vec![wild(u16::MAX, 1), wild(0, u16::MAX), wild(3, 2)], vec![], vec![wild(2, 0), wild(1, 1), wild(9, 9), wild(1, 1)]];
    let hostile = set(vec![
        Block::Table(Table { rows, border: true, rtl: true, repeat: true, ..Table::default() }),
        Block::Table(Table { rtl: true, ..Table::default() }),
        tabbed(true, &"\t\u{5d0}\t ab \t".repeat(200)),
    ]);
    within(&hostile, paper);
}

#[test]
fn several_pictures_stand_at_the_sides_of_one_paragraph() {
    use openmailclient_formats::rich::Float;
    let side = |id: &str, float: Float| Run { picture: Some(Picture::Attached(id.into())), float, ..Run::default() };
    // In points: small is 60 by 30, tall 60 by 120, broad 300 by 30; each takes 6 more as its gap.
    let sizes = |picture: &Picture| match picture {
        Picture::Attached(id) if id == "small" => Some((80, 40)),
        Picture::Attached(id) if id == "tall" => Some((80, 160)),
        Picture::Attached(id) if id == "broad" => Some((400, 40)),
        _ => None,
    };
    let print =
        |body: &Doc| print_on(&Library::empty(), &Memo { owner: "Owner", fields: &[], body, plain: false, sizes: &sizes }, Paper::A4).unwrap();
    let words = |n: usize| vec!["word"; n].join(" ");
    let (left, right) = (36.0f32, 595.28f32 - 36.0);
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    let lines_of = |page: &Page| -> Vec<Text> { page.texts.iter().filter(|t| t.glyphs.first().is_some_and(|g| g.1 == 'w')).cloned().collect() };
    let find = |page: &Page, text: &str| page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap().clone();

    // One at each side, of different heights: lines are short at both ends while both stand,
    // at one while one does, and as wide as the page under both.
    let body =
        doc(vec![Block::Para(Para { runs: vec![side("small", Float::Left), side("tall", Float::Right), run(&words(400))], ..Para::default() })]);
    let printed = print(&body);
    let page = &printed.pages[0];
    assert_eq!(page.pictures.len(), 2);
    let (small, tall) = (&page.pictures[0], &page.pictures[1]);
    assert!(near(small.x, left) && near(tall.x, right - 60.0) && near(small.y, tall.y), "{small:?} {tall:?}");
    let lines = lines_of(page);
    assert!(lines.len() > 13, "{}", lines.len());
    for (n, line) in lines.iter().enumerate() {
        let (from, to) = match n {
            0..=2 => (left + 66.0, right - 66.0),
            3..=10 => (left, right - 66.0),
            _ => (left, right),
        };
        assert!(near(line.x, from) && line.x + line.width <= to + 0.01, "line {n}: {} {}", line.x, line.width);
        assert!(n + 1 == lines.len() || line.width + 27.0 > to - from, "line {n} is short");
    }
    within(&printed, Paper::A4);

    // Two at one side stand beside one another; from the right they are set from the right.
    for float in [Float::Left, Float::Right] {
        let body = doc(vec![Block::Para(Para { runs: vec![side("small", float), side("tall", float), run(&words(60))], ..Para::default() })]);
        let printed = print(&body);
        let page = &printed.pages[0];
        let (first, second, line) = (&page.pictures[0], &page.pictures[1], &lines_of(page)[0]);
        if float == Float::Left {
            assert!(near(first.x, left) && near(second.x, left + 66.0) && near(line.x, left + 132.0));
        } else {
            assert!(
                near(first.x, right - 60.0) && near(second.x, right - 126.0) && near(line.x, left) && line.x + line.width <= right - 132.0 + 0.01
            );
        }
    }

    // One set while another still stands is set beside it, and what follows stands beside both
    // for as long as both stand.
    let body = doc(vec![
        Block::Para(Para { runs: vec![side("tall", Float::Left), run("one")], ..Para::default() }),
        Block::Para(Para { runs: vec![side("small", Float::Left), run("two")], ..Para::default() }),
        Block::Para(para("three")),
        Block::Para(Para { clear: true, ..para("below") }),
    ]);
    let printed = print(&body);
    let page = &printed.pages[0];
    let (first, second) = (&page.pictures[0], &page.pictures[1]);
    let (one, two, three, below) = (find(page, "one"), find(page, "two"), find(page, "three"), find(page, "below"));
    assert!(near(first.x, left) && near(second.x, left + 66.0) && second.y > first.y, "{first:?} {second:?}");
    assert!(near(one.x, left + 66.0) && near(two.x, left + 132.0) && near(three.x, left + 132.0), "{} {} {}", one.x, two.x, three.x);
    assert!(near(below.x, left) && below.y - 9.6 >= first.y + 120.0 - 0.01);

    // One for which no room is left between the others stands in the text.
    let body = doc(vec![Block::Para(Para {
        runs: vec![side("broad", Float::Left), side("small", Float::Right), side("broad", Float::Left), run("ab")],
        ..Para::default()
    })]);
    let printed = print(&body);
    let page = &printed.pages[0];
    assert_eq!(page.pictures.len(), 3);
    let inline = page.pictures.iter().filter(|p| p.x > left + 300.0 && p.x < right - 66.0).count();
    assert_eq!(inline, 1, "{:?}", page.pictures);
    within(&printed, Paper::A4);

    // Hostile: far more of them than are ever set at the sides, of every size, everywhere.
    let mut blocks: Vec<Block> = Vec::new();
    for i in 0..60u16 {
        let mut runs: Vec<Run> = (0..40u16)
            .map(|k| Run {
                extent: (i.wrapping_mul(977).wrapping_add(k * 31) % 700, k.wrapping_mul(331) % 1500),
                ..side(["small", "tall", "broad", "none"][usize::from((i + k) % 4)], if (i + k) % 3 == 0 { Float::Left } else { Float::Right })
            })
            .collect();
        runs.push(run(&words(usize::from(i % 9))));
        blocks.push(Block::Para(Para { runs, rtl: i % 4 == 1, clear: i % 11 == 5, indent: (i % 14) as u8, ..Para::default() }));
        if i % 13 == 7 {
            blocks.push(Block::Table(Table {
                rows: vec![vec![cell("in"), Cell { blocks: blocks.clone(), span: 1, rows: 1, ..Cell::default() }]],
                ..Table::default()
            }));
        }
    }
    for paper in [Paper::A4, SMALL] {
        within(
            &print_on(&Library::empty(), &Memo { owner: "Owner", fields: &[], body: &doc(blocks.clone()), plain: false, sizes: &sizes }, paper)
                .unwrap(),
            paper,
        );
    }
}

#[test]
fn boxes_stand_inside_one_another_on_paper_over_the_colour_behind_the_body() {
    use crate::Fill;
    use openmailclient_formats::rich::{Edge, Frame};
    let line = |width: u8| Edge { width, color: Some([0, 0, 0]), line: Line::Solid };
    let outer = Arc::new(Frame {
        edges: [line(1); 4],
        padding: [10; 4],
        indent: 0,
        between: Edge::default(),
        fill: Some([200, 0, 0]),
        within: None,
        radius: 0,
    });
    let inner = Arc::new(Frame {
        edges: [line(2); 4],
        padding: [4; 4],
        indent: 0,
        between: Edge::default(),
        fill: Some([0, 0, 200]),
        within: Some(outer.clone()),
        radius: 0,
    });
    let boxed = |text: &str, frame: &Arc<Frame>| Block::Para(Para { frame: Some(frame.clone()), ..para(text) });
    let shaded = Cell { background: Some([0, 200, 0]), ..cell("cell") };
    let table = Block::Table(Table { rows: vec![vec![shaded, cell("two")]], inside: Some(inner.clone()), ..Table::default() });
    let blocks =
        vec![Block::Para(para("before")), boxed("first", &outer), boxed("second", &inner), table, boxed("last", &outer), Block::Para(para("after"))];
    let body = Doc { fill: Some([10, 20, 30]), ..doc(blocks) };
    let printed = print_on(&Library::empty(), &Memo { owner: "Owner", fields: &[], body: &body, plain: false, sizes: &|_| None }, Paper::A4).unwrap();
    let page = &printed.pages[0];
    let find = |text: &str| page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap().clone();
    let fill = |color: [u8; 3]| *page.fills.iter().find(|f| f.color == color).unwrap();
    let (behind, red, blue, green) = (fill([10, 20, 30]), fill([200, 0, 0]), fill([0, 0, 200]), fill([0, 200, 0]));
    // Each lies over the one it stands in: the body's colour, the outer box, the inner box,
    // the cell.
    assert!(
        behind.depth < red.depth && red.depth < blue.depth && blue.depth < green.depth,
        "{} {} {} {}",
        behind.depth,
        red.depth,
        blue.depth,
        green.depth
    );
    let inside = |a: &Fill, b: &Fill| {
        a.x >= b.x - 0.01 && a.y >= b.y - 0.01 && a.x + a.width <= b.x + b.width + 0.01 && a.y + a.height <= b.y + b.height + 0.01
    };
    assert!(inside(&green, &blue) && inside(&blue, &red) && inside(&red, &behind), "{green:?} {blue:?} {red:?} {behind:?}");
    // The inner box is set in by the outer one's line and room: 11 pixels are 8.25 points.
    assert!((blue.x - red.x - 8.25).abs() < 0.01 && (red.x + red.width - blue.x - blue.width - 8.25).abs() < 0.01, "{red:?} {blue:?}");
    // What stands in which: text and table in the inner box, the first and last in the outer only.
    let over = |text: &str, f: &Fill| {
        let t = find(text);
        t.x >= f.x && t.x + t.width <= f.x + f.width && t.y > f.y && t.y <= f.y + f.height
    };
    assert!(over("second", &blue) && over("cell", &blue) && over("two", &blue));
    assert!(over("first", &red) && over("last", &red) && !over("first", &blue) && !over("last", &blue));
    assert!(over("before", &behind) && over("after", &behind) && !over("before", &red) && !over("after", &red));
    assert!(find("last").y > blue.y + blue.height && find("second").y < find("cell").y);
    within(&printed, Paper::A4);

    // A body longer than a page has its colour on every page; one with none has none.
    let long =
        Doc { fill: Some([10, 20, 30]), ..doc((0..200).map(|i| boxed(&format!("line {i}"), if i % 3 == 0 { &inner } else { &outer })).collect()) };
    let printed = print_on(&Library::empty(), &Memo { owner: "Owner", fields: &[], body: &long, plain: false, sizes: &|_| None }, Paper::A4).unwrap();
    assert!(printed.pages.len() > 1);
    assert!(printed.pages.iter().all(|p| p.fills.iter().any(|f| f.color == [10, 20, 30] && f.depth == 0)));
    within(&printed, Paper::A4);
    let none = print_on(
        &Library::empty(),
        &Memo { owner: "Owner", fields: &[], body: &doc(vec![Block::Para(para("x"))]), plain: false, sizes: &|_| None },
        Paper::A4,
    )
    .unwrap();
    assert!(none.pages[0].fills.is_empty());

    // Hostile: boxes far deeper than are drawn, tables in boxes in cells in boxes, small paper.
    let mut frame = outer.clone();
    let mut blocks = Vec::new();
    for i in 0..40u8 {
        frame = Arc::new(Frame {
            edges: [line(i % 9); 4],
            padding: [u16::from(i) * 9; 4],
            indent: i % 20,
            between: line(i % 3),
            fill: Some([i; 3]),
            within: Some(frame),
            radius: 0,
        });
        blocks.push(boxed("dp", &frame));
        let held = Cell { blocks: if i % 10 == 3 { blocks.clone() } else { vec![boxed("hd", &frame)] }, span: 1, rows: 1, ..Cell::default() };
        blocks.push(Block::Table(Table { rows: vec![vec![cell("in"), held]], inside: Some(frame.clone()), ..Table::default() }));
        if blocks.len() > 60 {
            blocks.drain(..30);
        }
    }
    for paper in [Paper::A4, SMALL] {
        within(
            &print_on(&Library::empty(), &Memo { owner: "Owner", fields: &[], body: &doc(blocks.clone()), plain: false, sizes: &|_| None }, paper)
                .unwrap(),
            paper,
        );
    }
}

#[test]
fn text_within_a_line_is_printed_in_its_border_and_boxes_have_rounded_corners() {
    let blue = Edge { width: 2, color: Some([0, 0, 200]), line: Line::Solid };
    let edged = |text: &str, edge: Edge| Run { text: text.into(), style: Style { edge: Some(edge), ..Style::default() }, ..Run::default() };
    let body = doc(vec![Block::Para(Para { runs: vec![run("ab "), edged("cd", blue), run(" ef")], ..Para::default() })]);
    let printed = print(&body, Paper::A4);
    let page = &printed.pages[0];
    let find = |page: &Page, text: &str| page.texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>() == text).unwrap().clone();
    let word = find(page, "cd");
    let lines: Vec<_> = page.rules.iter().filter(|r| r.color == [0, 0, 200]).collect();
    // Over, under and at both sides of the word, a pixel and a half thick, round its letters.
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(lines.iter().all(|r| (r.width - 1.5).abs() < 0.01));
    let (level, upright): (Vec<&Rule>, Vec<&Rule>) = lines.iter().copied().partition(|r| (r.from.1 - r.to.1).abs() < 0.01);
    assert_eq!((level.len(), upright.len()), (2, 2));
    for r in &level {
        assert!((r.from.0 - word.x).abs() < 0.01 && (r.to.0 - word.x - word.width).abs() < 0.01, "{r:?} {word:?}");
    }
    let (top, bottom) = (level[0].from.1.min(level[1].from.1), level[0].from.1.max(level[1].from.1));
    assert!(top < word.y - word.size * 0.6 && bottom > word.y, "{top} {bottom} {}", word.y);
    for r in &upright {
        assert!(r.from.0 > word.x - 0.01 && r.from.0 < word.x + word.width + 0.01);
        assert!((r.from.1.min(r.to.1) - (top - 0.75)).abs() < 0.01 && (r.from.1.max(r.to.1) - (bottom + 0.75)).abs() < 0.01);
    }

    // A border that names no colour has the colour of its text.
    let own = Edge { color: None, ..blue };
    let red = Run { style: Style { edge: Some(own), color: Some([200, 0, 0]), ..Style::default() }, ..run("cd") };
    let printed = print(&doc(vec![Block::Para(Para { runs: vec![run("ab "), red], ..Para::default() })]), Paper::A4);
    assert_eq!(printed.pages[0].rules.iter().filter(|r| r.color == [200, 0, 0]).count(), 4);

    // Text in a border that goes on over three lines: a line over and under each, and a side
    // only where the border starts and where it ends. Bold letters in it are in the same border.
    let bold = Run { style: Style { edge: Some(blue), bold: true, ..Style::default() }, ..run("ef gh ") };
    let long = vec![run("ab "), edged("ab cd ", blue), bold, edged(&"ij kl mn op ".repeat(5), blue), run("end")];
    let printed = print(&doc(vec![Block::Para(Para { runs: long, ..Para::default() })]), SMALL);
    let rules: Vec<_> = printed.pages[0].rules.iter().filter(|r| r.color == [0, 0, 200]).collect();
    let upright = rules.iter().filter(|r| (r.from.0 - r.to.0).abs() < 0.01).count();
    let rows: std::collections::BTreeSet<i32> = rules.iter().filter(|r| (r.from.1 - r.to.1).abs() < 0.01).map(|r| (r.from.1 * 10.0) as i32).collect();
    assert_eq!(upright, 2, "{rules:?}");
    assert!(rows.len() >= 6, "{rows:?}");
    within(&printed, SMALL);

    // A box with rounded corners: its fill is rounded, and its border is a band that follows
    // the corners, over the fill, in place of four straight lines.
    let line = |width: u8| Edge { width, color: Some([0, 0, 0]), line: Line::Solid };
    let frame = |edges, radius| {
        Arc::new(Frame { edges, padding: [4; 4], indent: 0, between: Edge::default(), fill: Some([0, 51, 102]), within: None, radius })
    };
    let boxed = |text: &str, frame: &Arc<Frame>| Block::Para(Para { frame: Some(frame.clone()), ..para(text) });
    let round = frame([line(2); 4], 8);
    let printed = print(&doc(vec![boxed("one", &round), boxed("two", &round)]), Paper::A4);
    let page = &printed.pages[0];
    // The line under the head of the page is thicker than any of these.
    let lines = |page: &Page| page.rules.iter().filter(|r| r.width < 2.0).count();
    assert_eq!(lines(page), 0, "{:?}", page.rules);
    let (fill, band) = (page.fills.iter().find(|f| f.color == [0, 51, 102]).unwrap(), page.fills.iter().find(|f| f.ring > 0.0).unwrap());
    assert!((fill.radius - 6.0).abs() < 0.01 && fill.ring == 0.0, "{fill:?}");
    assert!((band.radius - 6.0).abs() < 0.01 && (band.ring - 1.5).abs() < 0.01 && band.color == [0, 0, 0]);
    assert_eq!((band.x, band.y, band.width, band.height, band.depth), (fill.x, fill.y, fill.width, fill.height, fill.depth));
    let (at_fill, at_band) = (page.fills.iter().position(|f| f == fill).unwrap(), page.fills.iter().position(|f| f == band).unwrap());
    assert!(at_band > at_fill, "the border is drawn after the fill");

    // Sides that differ, and dashes, keep square corners and straight lines.
    for edges in [[line(2), line(2), line(2), line(1)], [Edge { line: Line::Dashed, ..line(2) }; 4]] {
        let printed = print(&doc(vec![boxed("one", &frame(edges, 8))]), Paper::A4);
        let page = &printed.pages[0];
        assert!(page.fills.iter().all(|f| f.radius == 0.0 && f.ring == 0.0) && lines(page) == 4);
    }
    // A box with no border is rounded too, and a cell with a colour.
    let printed = print(&doc(vec![boxed("one", &frame([Edge::default(); 4], 8))]), Paper::A4);
    assert!(printed.pages[0].fills.iter().all(|f| (f.radius - 6.0).abs() < 0.01 && f.ring == 0.0) && printed.pages[0].fills.len() == 1);
    let shaded = Cell { background: Some([0, 200, 0]), radius: 4, ..cell("cell") };
    let printed = print(&doc(vec![Block::Table(Table { rows: vec![vec![shaded, cell("two")]], ..Table::default() })]), Paper::A4);
    assert!(printed.pages[0].fills.iter().any(|f| f.color == [0, 200, 0] && (f.radius - 3.0).abs() < 0.01));

    // A box that goes on over pages is cut where the page ends: square, with its lines.
    let many: Vec<Block> = (0..80).map(|i| boxed(&format!("line {i}"), &round)).collect();
    let printed = print(&doc(many), SMALL);
    assert!(printed.pages.len() > 2);
    assert!(printed.pages.iter().all(|p| p.fills.iter().all(|f| f.radius == 0.0 && f.ring == 0.0) && lines(p) > 0));

    // Any radius and any border on small paper: every corner is rounded by a number, by no
    // more than half of its box, and the text stays on the page.
    let wild = Edge { width: u8::MAX, color: None, line: Line::Solid };
    let huge = frame([wild; 4], u16::MAX);
    let cells = vec![Cell { radius: u16::MAX, background: Some([1, 2, 3]), edges: Some([wild; 4]), ..cell("in a cell") }, cell("two")];
    let blocks = vec![
        boxed("in a box", &huge),
        Block::Table(Table { rows: vec![cells], inside: Some(huge.clone()), ..Table::default() }),
        Block::Para(Para { runs: vec![edged(&"wide words ".repeat(40), wild), edged("\u{5d0}\u{5d1} ab", wild)], rtl: true, ..Para::default() }),
    ];
    for paper in [SMALL, Paper { width: 60.0, height: 80.0, margin: 20.0 }, Paper::A4] {
        let printed = print(&doc(blocks.clone()), paper);
        for page in &printed.pages {
            for f in &page.fills {
                assert!(f.radius.is_finite() && f.ring.is_finite() && f.radius >= 0.0 && f.ring >= 0.0);
                assert!(f.radius <= f.width / 2.0 + 0.01 && f.radius <= f.height / 2.0 + 0.01 && f.radius <= 48.01, "{f:?}");
                assert!(f.ring <= f.width / 2.0 + 0.01 && f.ring <= f.height / 2.0 + 0.01, "{f:?}");
            }
            assert!(page.rules.iter().all(|r| r.width.is_finite() && r.from.0.is_finite() && r.to.1.is_finite()));
        }
    }
}

#[test]
fn the_body_alone_is_printed_without_details_header_or_footer() {
    let body = doc(vec![Block::Para(para("Only this"))]);
    let fields = [("From".to_string(), "Sam".to_string()), ("Subject".to_string(), "Hello".to_string())];
    let fields: Vec<(&str, String)> = fields.iter().map(|(n, v)| (n.as_str(), v.clone())).collect();
    let memo_ = Memo { owner: "Owner", fields: &fields, body: &body, plain: false, sizes: &|_| None };
    let setup = Setup { header: [String::new(), "Head".into(), String::new()], ..Setup::from(Paper::A4) };
    let all = print_on(&Library::empty(), &memo_, setup.clone()).unwrap().pages[0].text();
    for word in ["Owner", "From:", "Sam", "Subject:", "Head", "Only this", "1"] {
        assert!(all.contains(word), "{word} in {all:?}");
    }
    // Neither the details nor the edges: the body, from the top margin.
    let bare = Setup { details: false, edges: false, ..setup.clone() };
    let printed = print_on(&Library::empty(), &memo_, bare.clone()).unwrap();
    assert_eq!(printed.pages[0].text().trim(), "Only this");
    let top = printed.pages[0].texts.iter().map(|t| t.y).fold(f32::MAX, f32::min);
    assert!(top < 36.0 + 16.0, "{top}");
    // Each by itself.
    let edges_only = print_on(&Library::empty(), &memo_, Setup { details: false, ..setup.clone() }).unwrap().pages[0].text();
    assert!(edges_only.contains("Head") && !edges_only.contains("Owner") && !edges_only.contains("Sam"));
    let details_only = print_on(&Library::empty(), &memo_, Setup { edges: false, ..setup.clone() }).unwrap().pages[0].text();
    assert!(details_only.contains("Owner") && details_only.contains("Sam") && !details_only.contains("Head"));
    // A list without its details is the table alone.
    let columns = [Column { name: "Subject", share: 100, right: false }];
    let rows = [Row { cells: vec!["Row one".into()], bold: false, heading: false }];
    let listed = Listed { owner: "Owner", title: "Inbox", note: "1 item", columns: &columns, rows: &rows };
    let table = list_on(&listed, bare).unwrap().pages[0].text();
    assert!(!table.contains("Owner") && !table.contains("Inbox") && table.contains("Row one"), "{table:?}");
}
