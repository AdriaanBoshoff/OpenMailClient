use crate::html::parse;
use crate::rich::{Align, Block, Doc, Float, Line, Para, ParaKind, Picture, Script};
use std::sync::Arc;

fn paras(doc: &Doc) -> Vec<&Para> {
    doc.blocks
        .iter()
        .filter_map(|b| match b {
            Block::Para(p) => Some(p),
            _ => None,
        })
        .collect()
}

#[test]
fn paragraphs_runs_and_inline_styles() {
    let doc = parse(
        "<html><head><title>x</title><style>p{color:red}</style></head><body><p>Hi <b>bold</b> and <i>it<u>al</u>ic</i>.</p><div>Next&nbsp;&amp; last</div></body></html>",
    );
    let p = paras(&doc);
    assert_eq!(p.len(), 2);
    assert_eq!(p[0].text(), "Hi bold and italic.");
    assert!(p[0].gap);
    let styles: Vec<(&str, bool, bool, bool)> =
        p[0].runs.iter().map(|r| (r.text.as_str(), r.style.bold, r.style.italic, r.style.underline)).collect();
    assert_eq!(
        styles,
        [
            ("Hi ", false, false, false),
            ("bold", true, false, false),
            (" and ", false, false, false),
            ("it", false, true, false),
            ("al", false, true, true),
            ("ic", false, true, false),
            (".", false, false, false)
        ]
    );
    assert_eq!(p[1].text(), "Next\u{a0}& last");
    assert_eq!(doc.text(), "Hi bold and italic.\nNext\u{a0}& last");
}

#[test]
fn inline_css_and_font_tags() {
    let doc = parse(
        "<p style=\"text-align:center; margin:0\"><span style=\"color:#C00000;font-weight:700;font-size:14pt\">Red</span>\
         <font color=navy size=5 face=\"Courier New\">mono</font><span style='background-color: rgb(255, 255, 0); text-decoration: line-through'>hl</span>\
         <a href=\"https://example.org/a?b=1&amp;c=2\" style=\"text-decoration:none\">link</a><sup>2</sup></p>",
    );
    let p = paras(&doc);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].align, Align::Center);
    assert!(!p[0].gap, "a zero margin leaves no gap");
    let r = &p[0].runs;
    assert_eq!((r[0].style.color, r[0].style.bold, r[0].style.size), (Some([0xC0, 0, 0]), true, Some(14.0)));
    assert_eq!((r[1].style.color, r[1].style.size, r[1].style.mono), (Some([0, 0, 128]), Some(18.0), true));
    assert_eq!((r[2].style.background, r[2].style.strike), (Some([255, 255, 0]), true));
    assert_eq!(r[3].style.link.as_deref(), Some("https://example.org/a?b=1&c=2"));
    assert!(!r[3].style.underline);
    assert_eq!(r[4].style.script, Script::Super);
}

#[test]
fn lists_headings_quotes_and_rules() {
    let doc = parse(
        "<h2>Title</h2><ul><li>One<li>Two<ol start=3><li>Three</li><li>Four</li></ol></li></ul><blockquote type=cite>Quoted<blockquote style=\"border-left:2px solid #ccc\">Deeper</blockquote></blockquote><blockquote>Set in</blockquote><hr><pre>a\n  b\tc</pre>",
    );
    let p = paras(&doc);
    let got: Vec<(String, ParaKind, u8, u8)> = p.iter().map(|p| (p.text(), p.kind.clone(), p.indent, p.quote)).collect();
    assert_eq!(
        got,
        [
            ("Title".to_string(), ParaKind::Heading(2), 0, 0),
            ("One".to_string(), ParaKind::Item("•".to_string()), 1, 0),
            ("Two".to_string(), ParaKind::Item("•".to_string()), 1, 0),
            ("Three".to_string(), ParaKind::Item("3.".to_string()), 2, 0),
            ("Four".to_string(), ParaKind::Item("4.".to_string()), 2, 0),
            ("Quoted".to_string(), ParaKind::Body, 0, 1),
            ("Deeper".to_string(), ParaKind::Body, 0, 2),
            ("Set in".to_string(), ParaKind::Body, 1, 0),
            ("a\n  b    c".to_string(), ParaKind::Pre, 0, 0),
        ]
    );
    assert!(p[0].runs[0].style.bold);
    assert!(doc.blocks.iter().any(|b| matches!(b, Block::Rule)));
    assert!(p[8].runs[0].style.mono);
}

#[test]
fn line_breaks_and_blank_lines() {
    let doc = parse(
        "<div>one<br>two<br></div><div><br></div><div>three</div><p class=MsoNormal>four<o:p></o:p></p><p class=MsoNormal><o:p>&nbsp;</o:p></p>",
    );
    let p = paras(&doc);
    let texts: Vec<String> = p.iter().map(|p| p.text()).collect();
    assert_eq!(texts, ["one\ntwo", "\u{a0}", "three", "four", "\u{a0}"]);
    assert!(p.iter().all(|p| !p.gap), "div and word-processor paragraphs have no margins");
}

#[test]
fn tables_keep_cells_and_layout_tables_flatten() {
    let doc = parse(
        "<table border=1><tr><th>Item</th><th>Qty</th></tr><tr><td>Flyers</td><td align=right>500</td></tr><tr><td colspan=2>Total <b>500</b></td></tr></table>",
    );
    let Some(Block::Table(t)) = doc.blocks.first() else { panic!("expected a table, got {:?}", doc.blocks) };
    assert!(t.border);
    assert_eq!(t.columns(), 2);
    assert_eq!(t.rows.len(), 3);
    assert!(t.rows[0][0].header);
    assert_eq!(t.rows[2][0].span, 2);
    assert_eq!(doc.text(), "Item\tQty\nFlyers\t500\nTotal 500");
    let Some(Block::Para(qty)) = t.rows[1][1].blocks.first() else { panic!("expected text in the cell") };
    assert_eq!(qty.align, Align::Right);

    // One column, nested: reads as ordinary paragraphs.
    let doc = parse("<table><tr><td><table><tr><td><p>Hello</p></td></tr><tr><td>World</td></tr></table></td></tr></table><p>After</p>");
    assert_eq!(doc.blocks.len(), 3);
    assert!(doc.blocks.iter().all(|b| matches!(b, Block::Para(_))));
    assert_eq!(doc.text(), "Hello\nWorld\nAfter");

    // Markup left open inside a cell does not leak out of it.
    let doc = parse("<table><tr><td><b>bold<td>plain</table>after");
    let Some(Block::Table(t)) = doc.blocks.first() else { panic!("expected a table") };
    let Some(Block::Para(plain)) = t.rows[0][1].blocks.first() else { panic!("expected text") };
    assert!(!plain.runs[0].style.bold);
    assert_eq!(paras(&doc).last().map(|p| (p.text(), p.runs[0].style.bold)), Some(("after".to_string(), false)));
}

#[test]
fn nothing_active_survives_and_remote_content_is_only_counted() {
    let doc = parse(
        "<p onclick=\"evil()\">Text<script>alert('x')</script><style>p{}</style><iframe src=\"https://x.example/\">within</iframe>\
         <object data=x>embedded</object><img src=\"https://t.example/pixel.gif\" width=1 height=1><img src=\"https://x.example/a.png\" alt=\"Logo\">\
         <img src=\"cid:part1@example\" alt=chart><img src=\"data:image/png;base64,AAAA\"><span style=\"display:none\">hidden preheader</span>\
         <span style=\"font-size:0\">also hidden</span><!-- comment <b>not text</b> --><![if !mso]>shown<![endif]></p>\
         <div style=\"background:url(https://x.example/bg.png)\">bg</div>",
    );
    assert_eq!(doc.text(), "TextLogochartshown\nbg");
    assert_eq!(doc.remote_blocked, 3, "tracker, logo and background");
    let p = paras(&doc);
    let pictures: Vec<&Picture> = p[0].runs.iter().filter_map(|r| r.picture.as_ref()).collect();
    assert_eq!(
        pictures,
        [&Picture::Remote("https://x.example/a.png".to_string()), &Picture::Attached("part1@example".to_string()), &Picture::Inline(Some(0))]
    );
    let all = format!("{doc:?}");
    for gone in ["evil", "alert", "within", "embedded", "hidden", "comment", "AAAA"] {
        assert!(!all.contains(gone), "{gone} must not be in the document");
    }
}

#[test]
fn near_white_backgrounds_fill_their_cells_and_not_the_text() {
    let doc = parse(
        "<body bgcolor=\"#FFFFFF\"><table><tr><td bgcolor=\"#1F4E79\"><font color=white>Banner</font></td><td style=\"background:#fafafa\">Side</td></tr></table></body>",
    );
    let Some(Block::Table(t)) = doc.blocks.first() else { panic!("expected a table") };
    let run = |c: usize| match t.rows[0][c].blocks.first() {
        Some(Block::Para(p)) => p.runs[0].style.clone(),
        _ => panic!("expected text"),
    };
    assert_eq!((run(0).background, run(0).color), (None, Some([255, 255, 255])), "the colour fills the cell, not the text");
    assert_eq!((t.rows[0][0].background, t.rows[0][1].background), (Some([0x1F, 0x4E, 0x79]), Some([0xfa, 0xfa, 0xfa])));
    assert_eq!(run(1).background, None);
}

#[test]
fn hostile_markup_never_panics_and_is_capped() {
    for s in [
        "<",
        "<<<>>>",
        "</",
        "</>",
        "<a href=",
        "<a href='unterminated>text",
        "<p style=\"font-size:nanpt;color:#zzz;margin:;:;\">x",
        "<font size=99999999999 color=#12>x",
        "&#xFFFFFFFF;&;&#0;",
        "<script>never closed",
        "<!-- open",
        "<table><tr><td>",
        "</td></tr></table></table></p></b>",
        "<td>cell without table</td>",
        "<ol start=4294967295><li>a<li>b",
        "<img>",
        "<img src=cid:>",
        "<p\u{0} =\"\" ='' \"'>é<é>é",
        "a < b and c > d",
        "<table><table><table><tr><tr><td><td></table>x",
    ] {
        let _ = parse(s);
    }
    assert_eq!(parse("a < b and c > d").text(), "a < b and c > d");

    // Deep nesting: bounded element stack, bounded tables, text still read.
    let deep = format!("{}text{}", "<div><b><table><tr><td>".repeat(3000), "</td></tr></table></b></div>".repeat(3000));
    assert!(parse(&deep).text().contains("text"));
    let wide = "<table><tr>".to_string() + &"<td>c</td>".repeat(5000) + "</tr></table>";
    assert_eq!(parse(&wide).text().matches('c').count(), 5000);

    // Oversized input is cut, and says so.
    let big = "<p>word word word</p>".repeat(300_000);
    let doc = parse(&big);
    assert!(doc.truncated);
    assert!(doc.blocks.len() <= 50_000);
    let multibyte = "é".repeat(3 << 20);
    assert!(parse(&multibyte).truncated);
}

const RTF: &[u8] = b"{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0\\fswiss Arial;}{\\f1\\fmodern Courier New;}}{\\colortbl;\\red0\\green102\\blue153;\\red255\\green242\\blue204;}\
    {\\*\\generator Ours;}\\pard\\qc\\b\\fs32\\cf1 Title\\b0\\fs20\\cf0\\par\
    \\pard Code {\\f1 RP-1}, {\\i it}{\\v hidden}{\\super 2}{\\strike old}\\line next\\par\\par\
    \\trowd\\clbrdrt\\brdrs\\cellx3000\\cellx6000\\intbl\\b Item\\cell Amount\\b0\\cell\\row\
    \\trowd\\cellx3000\\cellx6000\\intbl {\\highlight2 Total}\\cell 62.00\\cell\\row\
    \\pard\\li1440 Indented {\\pict\\wmetafile8 010203}\\par\
    \\pard See {\\field{\\*\\fldinst{HYPERLINK \"https://example.net/help\"}}{\\fldrslt{\\ul\\cf1 the help page}}} now\\par\\par\\par}";

#[test]
fn rtf_keeps_styles_paragraphs_tables_and_links() {
    let doc = crate::rtf::parse(RTF);
    assert!(!doc.truncated);
    assert_eq!(doc.text(), "Title\nCode RP-1, it2old\nnext\n\u{a0}\nItem\tAmount\nTotal\t62.00\nIndented \nSee the help page now");
    let p = paras(&doc);
    assert_eq!(p[0].align, Align::Center);
    let title = &p[0].runs[0].style;
    assert_eq!((title.bold, title.size, title.color), (true, Some(16.0), Some([0, 102, 153])));
    assert_eq!(p[1].align, Align::Left);
    let by_text = |para: &Para, text: &str| para.runs.iter().find(|r| r.text == text).map(|r| r.style.clone()).unwrap();
    assert!(by_text(p[1], "RP-1").mono);
    assert!(by_text(p[1], "it").italic);
    assert_eq!(by_text(p[1], "2").script, Script::Super);
    assert!(by_text(p[1], "old").strike);
    assert!(!by_text(p[1], "Code ").bold, "the title's bold ended with \\b0");
    let table = doc.blocks.iter().find_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).unwrap();
    // Only the line the row asks for: above its first cell.
    assert!(!table.border && table.snug);
    assert_eq!(table.rows[0][0].edges.map(|e| e.map(|e| e.width)), Some([1, 0, 0, 0]));
    assert!(table.rows[0][1].edges.is_none() && table.rows[1][0].edges.is_none());
    assert_eq!((table.rows.len(), table.columns()), (2, 2));
    let Block::Para(total) = &table.rows[1][0].blocks[0] else { panic!("a paragraph") };
    assert_eq!(total.runs[0].style.background, Some([255, 255, 0]));
    assert_eq!(p[3].indent, 2);
    assert_eq!(p[3].runs[1].picture, Some(Picture::Inline(None)));
    let link = by_text(p[4], "the help page");
    assert_eq!(link.link.as_deref(), Some("https://example.net/help"));
    assert!(link.underline);
    assert_eq!(by_text(p[4], " now").link, None);
}

#[test]
fn rtf_that_wraps_html_or_text_is_read_as_what_it_wraps() {
    let html = b"{\\rtf1\\ansi\\fromhtml1 {\\*\\htmltag64 <p>}\\htmlrtf {\\htmlrtf0 Hello {\\*\\htmltag84 <b>}you{\\*\\htmltag92 </b>}\\htmlrtf }\\htmlrtf0{\\*\\htmltag72 </p>}\
        {\\*\\htmltag4 <script>}alert(1){\\*\\htmltag12 </script>}{\\*\\htmltag4 <img src=\"http://example.com/p.gif\" width=40 height=40>}}";
    let doc = crate::rtf::parse(html);
    assert_eq!(doc.text(), "Hello you");
    assert!(paras(&doc)[0].runs[1].style.bold);
    assert_eq!(doc.remote_blocked, 1, "what the wrapped HTML points at is still not fetched");
    let text = crate::rtf::parse(b"{\\rtf1\\ansi\\fromtext \\pard One\\par\\par Three\\par}");
    assert_eq!(paras(&text).iter().map(|p| p.text()).collect::<Vec<_>>(), ["One", "\u{a0}", "Three"]);
}

#[test]
fn hostile_rtf_gives_a_bounded_document() {
    let junk: [&[u8]; 7] = [
        b"",
        b"{",
        b"}",
        b"\\",
        b"{\\rtf1\\cell\\row\\cell\\row}",
        b"{\\rtf1\\cf99999\\highlight-4 x\\fs-9 y\\fs999999 z\\li-5 q\\li99999999 r}",
        b"{\\field{\\*\\fldinst HYPERLINK}{\\fldrslt x}}",
    ];
    for rtf in junk {
        let _ = crate::rtf::parse(rtf);
    }
    let deep = [b"{\\rtf1 ".to_vec(), b"{\\b ".repeat(5000), b"deep".to_vec(), vec![b'}'; 5001]].concat();
    assert_eq!(crate::rtf::parse(&deep).text(), "deep");
    let many = [b"{\\rtf1 ".to_vec(), b"x\\par ".repeat(80_000), b"}".to_vec()].concat();
    let doc = crate::rtf::parse(&many);
    assert!(doc.truncated && doc.blocks.len() <= 50_000);
    let cells = [b"{\\rtf1 ".to_vec(), b"\\intbl c\\cell ".repeat(20_000), b"\\row}".to_vec()].concat();
    assert!(matches!(crate::rtf::parse(&cells).blocks.first(), Some(Block::Table(t)) if t.columns() == 20_000));
    let colors = [b"{\\rtf1{\\colortbl".to_vec(), b";".repeat(100_000), b"}\\cf5000 x}".to_vec()].concat();
    assert_eq!(crate::rtf::parse(&colors).text(), "x");
}

#[test]
fn a_picture_keeps_the_size_the_body_asks_for() {
    let doc = parse(
        "<p><img src=cid:a width=240 height=\"96px\"><img src=cid:b width=\"50%\"><img src=cid:c height=99999999>\
         <img src=cid:d width=-7 height=nan><img src=cid:e width=1e39></p>",
    );
    let extents: Vec<(u16, u16)> = paras(&doc)[0].runs.iter().filter(|r| r.picture.is_some()).map(|r| r.extent).collect();
    assert_eq!(extents, [(240, 96), (0, 0), (0, 4096), (0, 0)], "a size below nothing drops the picture; one beyond any number is no size");
}

#[test]
fn tables_keep_their_width_their_place_and_their_cell_sizes() {
    use crate::rich::TableWidth;
    let table = |html: &str| match parse(html).blocks.into_iter().next() {
        Some(Block::Table(t)) => t,
        other => panic!("expected a table, got {other:?}"),
    };
    let t = table("<table width=600 align=center><tr><td bgcolor=#1F4E79>Banner<tr><td>Body</table>");
    assert_eq!((t.width, t.align, t.columns(), t.rows.len()), (TableWidth::Pixels(600), Align::Center, 1, 2));
    let t = table("<table style=\"width:100%;MAX-WIDTH: 480px !important; margin:0 auto\"><tr><td width=120>a<td width=\"30%\">b</table>");
    assert_eq!((t.width, t.align), (TableWidth::Pixels(480), Align::Center));
    assert_eq!((t.rows[0][0].width, t.rows[0][1].width), (120, 0));
    let t = table("<table width=\"100%\"><tr><td>a<td>b</table>");
    assert_eq!((t.width, t.align), (TableWidth::Fill, Align::Left));
    let t = table("<table width=\"50%\" style=\"margin-left:auto\"><tr><td width=99999999>a<td width=-3>b</table>");
    assert_eq!((t.width, t.align, t.rows[0][0].width, t.rows[0][1].width), (TableWidth::Percent(50), Align::Left, 4096, 0));
    // One column and nothing to draw: still only layout.
    assert!(matches!(parse("<table width=\"100%\"><tr><td>a<tr><td>b</table>").blocks.first(), Some(Block::Para(_))));
    // A cell's height is the least it takes, capped; one in a unit or none is kept as pixels.
    let t = table("<table border=1><tr><td height=40>a<td style=\"height:3em\">b<td height=900000>c<td height=\"50%\">d</table>");
    let tall: Vec<u16> = t.rows[0].iter().map(|c| c.tall).collect();
    assert_eq!((tall[0], tall[1], tall[2]), (40, 48, 2_000), "{tall:?}");

    let rtf = crate::rtf::parse(
        b"{\\rtf1 \\trowd\\cellx4200\\cellx6000\\intbl a\\cell b\\cell\\row\\trowd\\cellx-50\\cellx99999999\\intbl c\\cell d\\cell e\\cell\\row}",
    );
    let Some(Block::Table(t)) = rtf.blocks.first() else { panic!("expected a table") };
    let widths: Vec<Vec<u16>> = t.rows.iter().map(|r| r.iter().map(|c| c.width).collect()).collect();
    assert_eq!(widths, [vec![280, 120], vec![0, 4096, 0]]);
}

#[test]
fn style_sheets_pick_elements_by_name_first_class_and_id() {
    let doc = parse(
        "<html><head><title>t</title><style><!--\n\
         @import url(https://x.example/a.css); /* a note */\n\
         p.note, h2 { color:#b00020; font-weight:bold }\n\
         #lead { font-size:18pt }\n\
         .plain { color:#006400 }\n\
         .box td, p > b, p.two.marks, p[lang], * { color:#00ff00 }\n\
         @media screen { p { color:#0000ff } .gone { display:block } }\n\
         .gone { display:none }\n\
         p { margin:0; color:#444488 }\n\
         a:hover { color:#ff0000 } a:link { color:#008080 }\n\
         --></style></head><body>\
         <p id=LEAD>lead</p><p class=\"Note other\">note</p><p class=\"other plain\">second</p><p class=gone>hidden</p>\
         <h2>head</h2><p class=plain style=\"color:#101010\">own</p><p class=two>x <b>b</b> <a href=\"https://x.example/\">link</a></p>",
    );
    let p = paras(&doc);
    assert_eq!(p.iter().map(|p| p.text()).collect::<Vec<_>>(), ["lead", "note", "second", "head", "own", "x b link"]);
    let color = |i: usize, run: usize| p[i].runs[run].style.color;
    assert_eq!((p[0].runs[0].style.size, color(0, 0)), (Some(18.0), Some([0x44, 0x44, 0x88])), "the id is matched whatever its case");
    assert_eq!((color(1, 0), p[1].runs[0].style.bold), (Some([0xb0, 0x00, 0x20]), true), "the first class picks the rule");
    assert_eq!(color(2, 0), Some([0x44, 0x44, 0x88]), "a second class picks nothing");
    assert_eq!(color(3, 0), Some([0xb0, 0x00, 0x20]));
    assert_eq!(color(4, 0), Some([0x10, 0x10, 0x10]), "the element's own style comes last");
    assert_eq!(color(5, 1), Some([0x44, 0x44, 0x88]), "rules with other selectors, and rules inside at-rules, are skipped");
    assert_eq!(color(5, 3), Some([0x00, 0x80, 0x80]));
    assert!(!p[1].gap && !p[2].gap && p[3].gap, "the sheet takes the paragraphs' margins away, not the heading's");

    // A link has the link colour, whatever is around it, unless the link itself is given one.
    let doc = parse(
        "<body text=#333333><p><font color=red><a href=\"https://x.example/\">a</a></font> <a href=\"https://x.example/\" style=\"color:red\">b</a></p>",
    );
    let runs = &paras(&doc)[0].runs;
    assert_eq!((runs[0].style.color, runs[2].style.color), (None, Some([255, 0, 0])));
}

#[test]
fn cells_keep_their_room_their_fills_and_their_places() {
    use crate::rich::{TableWidth, VAlign};
    let table = |html: &str| match parse(html).blocks.into_iter().next() {
        Some(Block::Table(t)) => t,
        other => panic!("expected a table, got {other:?}"),
    };
    let t = table("<table><tr><td>a<td>b</table>");
    assert_eq!((t.padding, t.spacing), (1, 2));
    let t = table(
        "<table cellpadding=12 cellspacing=\"0px\"><tr><td>a<td style=\"padding:16px 32px 4px\">b<td style=\"padding:6pt\">c<td style=\"padding:1em\">d</table>",
    );
    assert_eq!((t.padding, t.spacing), (12, 0));
    assert_eq!(t.rows[0].iter().map(|c| c.padding).collect::<Vec<_>>(), [None, Some([16, 32, 4, 32]), Some([8, 8, 8, 8]), None]);
    let t = table("<table cellpadding=99999 cellspacing=-4 style=\"border-collapse:collapse\"><tr><td>a<td>b</table>");
    assert_eq!((t.padding, t.spacing), (64, 0));
    let t = table("<table cellspacing=1e39><tr><td>a<td>b</table>");
    assert_eq!(t.spacing, 2, "a size beyond any number is no size");

    // A row's background is its cells', unless a cell has its own.
    let t = table(
        "<style>tr.band { background-color:#ffd966 }</style>\
         <table><tr bgcolor=#1F4E79><td>a<td bgcolor=#aa0000>b</tr><tr><td>c<td>d</tr><tr class=band><td>e<td>f</table>",
    );
    let fills: Vec<Vec<Option<[u8; 3]>>> = t.rows.iter().map(|r| r.iter().map(|c| c.background).collect()).collect();
    assert_eq!(fills[0], [Some([0x1F, 0x4E, 0x79]), Some([0xaa, 0, 0])]);
    assert_eq!(fills[1], [None, None]);
    assert_eq!(fills[2], [Some([0xff, 0xd9, 0x66]); 2]);
    assert!(
        t.rows.iter().flatten().all(|c| c.blocks.iter().all(|b| matches!(b, Block::Para(p) if p.runs.iter().all(|r| r.style.background.is_none()))))
    );

    // What a cell holds stands in the middle unless the cell or its row says otherwise.
    let t = table(
        "<table><tr><td>a<td valign=top>b<td valign=BOTTOM>c<td style=\"vertical-align:top\">d</tr>\
         <tr valign=top><td>e<td valign=middle>f<td valign=sideways>g</table>",
    );
    let places: Vec<Vec<VAlign>> = t.rows.iter().map(|r| r.iter().map(|c| c.valign).collect()).collect();
    assert_eq!(places[0], [VAlign::Middle, VAlign::Top, VAlign::Bottom, VAlign::Top]);
    assert_eq!(places[1], [VAlign::Top, VAlign::Middle, VAlign::Top]);

    // Widths as shares, of the room and of the table.
    let t = table("<table style=\"width:50%\"><tr><td width=\"25%\">a<td width=\"75%\">b<td width=\"900%\">c<td width=\"0%\">d</table>");
    assert_eq!(t.width, TableWidth::Percent(50));
    assert_eq!(t.rows[0].iter().map(|c| (c.share, c.width)).collect::<Vec<_>>(), [(25, 0), (75, 0), (100, 0), (0, 0)]);
    let t = table("<style>table.wide { width:400px }</style><table class=wide><tr><td>a<td>b</table>");
    assert_eq!(t.width, TableWidth::Pixels(400));
}

#[test]
fn hostile_style_sheets_are_read_without_harm() {
    let sheets = [
        "p {".repeat(5000),
        "@media x {".repeat(5000),
        "}".repeat(5000) + "p{color:red}",
        ".a".repeat(5000) + "{color:red}",
        (0..3000).map(|i| format!("p.c{i}, #i{i}, p {{ color:#{:06x} }}", i)).collect::<String>(),
        "p { color:red; ".to_string() + &"x:y;".repeat(100_000) + "}",
        "/* never closed p { color:red }".to_string(),
        "\u{e9}\u{e9}.\u{e9}#\u{e9} { color:red } .\u{202e} {} # {} . {} : {} ,,, {}".to_string(),
        "a:link:hover, a::before, :link, p:LINK { color:red }".to_string(),
    ];
    for sheet in &sheets {
        let doc = parse(&format!("<style>{sheet}</style><p class=\"c1 a\" id=i1>text</p><table><tr class=c2><td id=i2>a<td class=a>b</table>"));
        assert!(doc.text().contains("text"), "{:?}", doc.text());
    }
    // A sheet that is never closed takes the rest of the message with it, as a browser has it.
    assert!(parse("<p>before</p><style>p { color:red } <p>after</p>").text().contains("before"));
    // Sheets past the size limit are not read.
    let big = format!("<style>{}</style><style>p {{ color:red }}</style><p>x</p>", " ".repeat(600 << 10));
    assert_eq!(paras(&parse(&big))[0].runs[0].style.color, None);
}

#[test]
fn pictures_written_into_the_body_are_kept_as_bytes_once_each() {
    let doc = parse(
        "<p><img src=\"data:image/png;base64,AQID\"><img src=\"DATA:image/png;charset=x;BASE64, AQ\r\n ID \"><img src=\"data:image/gif;base64,BAUG-_8=\">\
         <img src=\"data:image/png,AQID\"><img src=\"data:image/png;base64,AQ*D\"><img src=\"data:image/png;base64,\"><img src=\"data:;base64\">\
         <img src=\"data:image/png;base64,====\"></p>",
    );
    let kept: Vec<Option<usize>> =
        paras(&doc)[0].runs.iter().filter_map(|r| if let Some(Picture::Inline(i)) = r.picture { Some(i) } else { None }).collect();
    assert_eq!(kept, [Some(0), Some(0), Some(1), None, None, None, None, None], "the same bytes are kept once; what is not base64 is not kept");
    assert_eq!(doc.written.iter().map(|w| w.to_vec()).collect::<Vec<_>>(), [vec![1, 2, 3], vec![4, 5, 6, 0xfb, 0xff]]);
    assert_eq!(doc.remote_blocked, 0, "nothing was asked of anyone");
}

#[test]
fn hostile_written_pictures_are_capped() {
    // More pictures than one body may keep: each differs, and the rest are placeholders.
    let many: String =
        (0..200usize).map(|i| format!("<img src=\"data:image/png;base64,{}\">", ["AAAA", "AAAB", "AAAC", "AAAD"][i % 4].repeat(1 + i / 4))).collect();
    let doc = parse(&many);
    assert_eq!(doc.written.len(), 64);
    // An address longer than is read is cut short, and one body keeps only so many bytes.
    let long = format!("<img src=\"data:image/png;base64,{}\">", "QUJD".repeat(1 << 20));
    let doc = parse(&long);
    assert!(doc.written.iter().map(|w| w.len()).sum::<usize>() <= 3 << 20);
    let heavy: String =
        (0..8u8).map(|i| format!("<img src=\"data:image/png;base64,{}{}\">", char::from(b'A' + i), "A".repeat((3 << 20) - 64))).collect();
    let doc = parse(&heavy);
    assert!(doc.written.iter().map(|w| w.len()).sum::<usize>() <= 12 << 20);
    for src in
        ["data:", "data:,", "data:;base64,", "data:image/png;base64", "data:\u{0};base64,\u{202e}", &format!("data:{};base64,AAAA", "x".repeat(4000))]
    {
        let doc = parse(&format!("<img src=\"{src}\" alt=a>"));
        assert!(doc.written.is_empty(), "{src:?}");
    }
}

#[test]
fn pictures_stand_at_a_side_only_when_align_says_so() {
    let doc = parse(
        "<p><img src=cid:a align=left><img src=cid:b align=\" RIGHT \"><img src=cid:c align=middle><img src=cid:d style=\"float:left\">\
         <img src=cid:e align=\"leftward\"><img src=cid:f>text</p>",
    );
    let floats: Vec<Float> = paras(&doc)[0].runs.iter().filter(|r| r.picture.is_some()).map(|r| r.float).collect();
    assert_eq!(floats, [Float::Left, Float::Right, Float::None, Float::None, Float::None, Float::None]);
}

#[test]
fn fonts_are_kept_by_name_as_the_body_lists_them() {
    let doc = parse(
        "<style>p.note { font-family: Verdana, sans-serif } #code { font-family: 'Courier New' }</style>\
         <p><font face=\"Arial\">a</font><span style=\"font-family: 'Nosuch Font', Georgia , serif\">b</span>c\
         <span style=\"font-family:inherit\">d</span><font face=\"\">e</font></p><p class=note>f<b>g</b></p><p id=code>h</p>",
    );
    let p = paras(&doc);
    let family =
        |para: &Para, text: &str| para.runs.iter().find(|r| r.text.contains(text)).and_then(|r| r.style.family.as_deref().map(str::to_lowercase));
    assert_eq!(family(p[0], "a").as_deref(), Some("arial"));
    assert_eq!(family(p[0], "b").as_deref(), Some("'nosuch font', georgia , serif"));
    assert_eq!(family(p[0], "c"), None);
    assert_eq!(family(p[0], "d"), None);
    assert_eq!(family(p[0], "e"), None);
    assert_eq!(family(p[1], "f").as_deref(), Some("verdana, sans-serif"));
    assert_eq!(family(p[1], "g").as_deref(), Some("verdana, sans-serif"), "bold text keeps the paragraph's font");
    assert!(p[2].runs[0].style.mono && p[2].runs[0].style.family.is_some());

    // A name is hostile text: it is kept on one line and no longer than a list may be.
    let hostile = format!("<p style=\"font-family:{}\">x</p><font face=\"a\u{0}b\nc\u{1b}[31m\">y</font>", "N".repeat(100_000));
    let doc = parse(&hostile);
    for run in paras(&doc).iter().flat_map(|p| &p.runs) {
        let name = run.style.family.as_deref().unwrap();
        assert!(name.chars().count() <= 160 && !name.chars().any(char::is_control), "{name:?}");
    }
}

#[test]
fn rtf_text_has_the_fonts_its_font_table_names() {
    let doc = crate::rtf::parse(RTF);
    let p = paras(&doc);
    let family = |para: &Para, text: &str| para.runs.iter().find(|r| r.text == text).and_then(|r| r.style.family.as_deref().map(str::to_string));
    assert_eq!(family(p[0], "Title").as_deref(), Some("Arial"), "text that names no font has the default one");
    assert_eq!(family(p[1], "RP-1").as_deref(), Some("Courier New"));
    assert_eq!(family(p[1], "it").as_deref(), Some("Arial"));

    // Other names of a font and its data are not its name; a font the table lacks is no font.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi{\\fonttbl{\\f0\\froman{\\*\\panose 02020603050405020304}{\\*\\falt Other}Georgia;}{\\f1 {\\fontemb 0102}Verdana;extra}{\\f2 ;}}\
          \\pard\\f0 a\\f1 b\\f2 c\\f9 d\\f-3 e\\par}",
    );
    let fonts: Vec<(&str, Option<&str>)> = paras(&doc)[0].runs.iter().map(|r| (r.text.as_str(), r.style.family.as_deref())).collect();
    assert_eq!(fonts, [("a", Some("Georgia")), ("b", Some("Verdana")), ("cde", None)]);

    // A font table without end, names without end, a default that is not there.
    let mut table = b"{\\rtf1\\deff2147483647{\\fonttbl".to_vec();
    for i in 0..5000 {
        table.extend_from_slice(format!("{{\\f{i} {}\u{1};}}", "n".repeat(300)).as_bytes());
    }
    table.extend_from_slice(b"}\\pard\\f4999 x\\f3 y\\par}");
    let doc = crate::rtf::parse(&table);
    assert_eq!(doc.text(), "xy");
    for run in paras(&doc).iter().flat_map(|p| &p.runs) {
        assert!(run.style.family.as_deref().is_none_or(|n| n.chars().count() <= 64 && !n.chars().any(char::is_control)));
    }
}

fn tables(doc: &Doc) -> Vec<&crate::rich::Table> {
    doc.blocks.iter().filter_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).collect()
}

#[test]
fn cells_that_reach_down_push_the_cells_under_them_aside() {
    use crate::rich::any_edge;
    // HTML 4.01 section 11.2.6.1: a cell stands in the first column no cell from above covers.
    let doc = parse(
        "<table border=1><tr><td rowspan=2>a<td>b<td>c<tr><td>d<td rowspan=2>e<tr><td>f<td>g</table>\
         <table border=1><tr><td rowspan=3 colspan=2>a<td>b<tr><td>c<tr><td>d<tr><td>e<td>f<td>g</table>",
    );
    let t = tables(&doc);
    assert_eq!(t[0].places(), [vec![0, 1, 2], vec![1, 2], vec![0, 1]]);
    assert_eq!(t[0].columns(), 3);
    assert_eq!(t[0].rows[0][0].rows, 2);
    assert_eq!(t[1].places(), [vec![0, 2], vec![2], vec![2], vec![0, 1, 2]]);
    assert_eq!(t[1].columns(), 3);
    assert!(t[0].border && any_edge(&t[0].frame), "a border attribute frames the table");

    // Spans without end, of nothing, and of what is not a number: every cell still has a place
    // in a table of bounded width.
    let mut html = String::from("<table><tr><td rowspan=0>a<td rowspan=99999999999 colspan=60000>b<td rowspan=-4>c<td rowspan=x>d");
    for _ in 0..300 {
        html.push_str("<tr><td rowspan=1000 colspan=900>e<td>f");
    }
    html.push_str("</table>");
    let doc = parse(&html);
    for table in tables(&doc) {
        let places = table.places();
        assert_eq!(places.len(), table.rows.len());
        for (row, starts) in table.rows.iter().zip(&places) {
            assert_eq!(row.len(), starts.len());
            assert!(row.iter().all(|c| c.rows <= 1000));
            assert!(starts.windows(2).all(|w| w[0] <= w[1]), "cells keep their order: {starts:?}");
        }
        assert!(table.columns() <= 1024 + 60000);
    }
}

#[test]
fn borders_set_in_styles_are_kept_for_paragraphs_tables_and_cells() {
    use crate::rich::{Line, any_edge};
    let doc = parse(
        "<p style=\"border:2px solid #cc0000\">a</p>\
         <p style=\"border-left:4px solid rgb(31,78,121); padding-left:8px\">b</p>\
         <div style=\"border:1px dashed #2e7d32; padding:10px\">c<p>d</p></div>\
         <p style=\"border-bottom:3px double #000000\">e</p>\
         <p style=\"border:none\">f</p><p style=\"border:5px\">g</p><p>h</p>",
    );
    let p = paras(&doc);
    let frame = |i: usize| p[i].frame.as_deref();
    let a = frame(0).expect("a border");
    assert!(a.edges.iter().all(|e| e.width == 2 && e.color == Some([0xcc, 0, 0]) && e.line == Line::Solid));
    let b = frame(1).expect("a border at the left");
    assert_eq!(b.edges.iter().map(|e| e.width).collect::<Vec<_>>(), [0, 0, 0, 4]);
    assert_eq!(b.edges[3].color, Some([31, 78, 121]));
    // Paragraphs inside one bordered block share its border, so that it is drawn round them once.
    let (c, d) = (p[2].frame.as_ref().expect("c"), p[3].frame.as_ref().expect("d"));
    assert!(std::sync::Arc::ptr_eq(c, d));
    assert_eq!((c.edges[0].line, c.padding), (Line::Dashed, [10; 4]));
    assert!(!std::sync::Arc::ptr_eq(p[0].frame.as_ref().expect("a"), p[1].frame.as_ref().expect("b")));
    let e = frame(4).expect("a line under");
    assert_eq!((e.edges[2].width, e.edges[2].line, e.edges[0].width), (3, Line::Double, 0));
    // No kind of line is no line.
    assert!(frame(5).is_none() && frame(6).is_none() && frame(7).is_none());

    let doc = parse(
        "<div style=\"border:1px solid red\"><table style=\"border:3px solid #cc0000\"><tr><td>a<td>b</table></div>\
         <table><tr><td style=\"border:1px solid #1f4e79\">c<td style=\"border-bottom:2px solid #2e7d32\">d<td>e</table>\
         <table border=1 bordercolor=\"#cc0000\"><tr><td>f</table>\
         <table border=4 cellspacing=0><tr><td>g</table>",
    );
    let t = tables(&doc);
    assert!(t[0].frame.iter().all(|e| e.width == 3 && e.color == Some([0xcc, 0, 0])) && !t[0].border);
    // A border round a table is not a border round each paragraph in it.
    assert!(t[0].rows.iter().flatten().flat_map(|c| &c.blocks).all(|b| !matches!(b, Block::Para(p) if p.frame.is_some())));
    let row = &t[1].rows[0];
    assert!(row[0].edges.is_some_and(|e| e.iter().all(|e| e.width == 1)));
    assert_eq!(row[1].edges.map(|e| [e[0].width, e[1].width, e[2].width, e[3].width]), Some([0, 0, 2, 0]));
    assert!(row[2].edges.is_none() && !any_edge(&t[1].frame));
    assert_eq!(t[2].line, Some([0xcc, 0, 0]));
    assert!(t[3].frame.iter().all(|e| e.width == 4));
}

#[test]
fn a_break_that_clears_starts_below_a_picture_at_the_side() {
    let doc = parse("<p><img src=cid:a align=left>a</p><p>b</p><br clear=all><p>c</p><p style=\"clear:both\">d</p><p>e<br clear=none>f</p>");
    let clear: Vec<(String, bool)> = paras(&doc).iter().map(|p| (p.runs.iter().map(|r| r.text.as_str()).collect::<String>(), p.clear)).collect();
    let of = |text: &str| clear.iter().find(|(t, _)| t.contains(text)).map(|(_, c)| *c);
    assert_eq!((of("a"), of("b"), of("c"), of("d")), (Some(false), Some(false), Some(true), Some(true)));
    assert_eq!(of("e"), Some(false), "a break that clears nothing is a line break: {clear:?}");
}

#[test]
fn hostile_borders_are_capped() {
    let mut html = String::from(
        "<p style=\"border:99999px solid; padding:99999px\">a</p><p style=\"border:-5px dashed #zzz; padding:-1px\">b</p>\
         <p style=\"border:1e39pt double rgb(999,-1,x)\">c</p><table border=99999 bordercolor=nothing style=\"border:NaNpx solid\"><tr><td style=\"border:inf solid\">d</table>\
         <br clear=\"\u{202e}\">",
    );
    html.push_str(&"<div style=\"border:1px solid red;padding:4000px\">".repeat(400));
    html.push_str("deep");
    let doc = parse(&html);
    fn check(blocks: &[Block], depth: usize) {
        assert!(depth < 64);
        for block in blocks {
            match block {
                Block::Para(p) => {
                    if let Some(frame) = &p.frame {
                        assert!(frame.edges.iter().all(|e| e.width <= 12), "{:?}", frame.edges);
                    }
                }
                Block::Table(t) => {
                    assert!(t.frame.iter().all(|e| e.width <= 12));
                    for cell in t.rows.iter().flatten() {
                        assert!(cell.edges.is_none_or(|e| e.iter().all(|e| e.width <= 12)));
                        check(&cell.blocks, depth + 1);
                    }
                }
                Block::Rule => {}
            }
        }
    }
    check(&doc.blocks, 0);
    assert!(doc.text().contains("deep"));
}

/// A picture file's first bytes as RTF writes them: two hex digits to the byte.
fn hexed(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn rtf_pictures_are_kept_as_bytes_at_the_size_they_ask_for() {
    let png = [0x89u8, b'P', b'N', b'G', 13, 10, 26, 10, 1, 2, 3];
    let hex = hexed(&png);
    let (head, tail) = hex.split_at(7);
    let rtf = format!(
        "{{\\rtf1\\ansi\\pard a{{\\pict\\pngblip\\picw96\\pich64 {head}\r\n{tail}}}\
         b{{\\*\\shppict{{\\pict{{\\*\\picprop{{\\sp{{\\sn x}}{{\\sv 4142}}}}}}\\jpegblip\\picwgoal2880\\pichgoal960\\picscalex50\\picscaley200 ffd8ff}}}}\
         {{\\nonshppict{{\\pict\\wmetafile8 0102}}}}\
         c{{\\pict\\wmetafile8\\picwgoal1500\\pichgoal750 010203}}d{{\\pict\\pngblip {hex}}}{{\\v{{\\pict\\pngblip {hex}}}}}\\par}}"
    );
    let doc = crate::rtf::parse(rtf.as_bytes());
    assert_eq!(doc.text(), "abcd");
    let runs: Vec<_> = paras(&doc)[0].runs.iter().filter_map(|r| r.picture.clone().map(|p| (p, r.extent))).collect();
    assert_eq!(
        runs,
        [
            // At its own size: the size in pixels a picture file says it has is the file's to say.
            (Picture::Inline(Some(0)), (0, 0)),
            // Twips to pixels, scaled as asked.
            (Picture::Inline(Some(1)), (96, 128)),
            // A metafile is not read: a place of the size asked for.
            (Picture::Inline(None), (100, 50)),
            // The same picture again is kept once.
            (Picture::Inline(Some(0)), (0, 0)),
        ]
    );
    assert_eq!(doc.written.len(), 2);
    assert_eq!(&*doc.written[0], &png[..]);
    assert_eq!(&*doc.written[1], &[0xff, 0xd8, 0xff][..]);

    // Data that is not hex digits, half a byte, none: a place, and no bytes kept.
    for data in ["89504e4x", "89504", ""] {
        let doc = crate::rtf::parse(format!("{{\\rtf1 a{{\\pict\\pngblip {data}}}b\\par}}").as_bytes());
        assert!(doc.written.is_empty(), "{data}");
        assert_eq!(paras(&doc)[0].runs.iter().filter_map(|r| r.picture.clone()).collect::<Vec<_>>(), [Picture::Inline(None)], "{data}");
    }
}

/// A bitmap of two by two pixels, three bytes to the pixel, without a file's header. The first
/// pixel kept (the lower left one) is blue.
fn small_dib() -> Vec<u8> {
    let mut dib = Vec::new();
    for n in [40u32, 2, 2] {
        dib.extend_from_slice(&n.to_le_bytes());
    }
    dib.extend_from_slice(&[1, 0, 24, 0]);
    for n in [0u32, 16, 0, 0, 0, 0] {
        dib.extend_from_slice(&n.to_le_bytes());
    }
    dib.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0]);
    dib
}

/// A Windows metafile of these records (what each does, and what follows that).
fn wmf(records: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut file = vec![1, 0, 9, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    for (function, body) in records.iter().chain(&[(0u16, Vec::new())]) {
        file.extend_from_slice(&u32::try_from((6 + body.len()) / 2).unwrap().to_le_bytes());
        file.extend_from_slice(&function.to_le_bytes());
        file.extend_from_slice(body);
    }
    file
}

/// The record that draws `dib` stretched (META_STRETCHDIB), copied as it is.
fn stretch(dib: &[u8]) -> (u16, Vec<u8>) {
    let mut body = vec![0x20, 0, 0xCC, 0, 0, 0];
    body.extend_from_slice(&[0; 16]);
    body.extend_from_slice(dib);
    (0x0F43, body)
}

/// An enhanced metafile that draws `dib` (EMR_STRETCHDIBITS) and nothing else.
fn emf(dib: &[u8]) -> Vec<u8> {
    let mut file = vec![0u8; 88];
    file[0] = 1;
    file[4] = 88;
    file[40..44].copy_from_slice(b" EMF");
    let (header, pixels) = dib.split_at(40);
    let mut record = vec![0u8; 80];
    let size = u32::try_from(80 + dib.len()).unwrap();
    for (at, n) in [(0, 81), (4, size), (48, 80), (52, 40), (56, 120), (60, u32::try_from(pixels.len()).unwrap()), (68, 0x00CC_0020)] {
        record[at..at + 4].copy_from_slice(&n.to_le_bytes());
    }
    file.extend_from_slice(&record);
    file.extend_from_slice(header);
    file.extend_from_slice(pixels);
    file.extend_from_slice(&[14, 0, 0, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20, 0, 0, 0]);
    file
}

#[test]
fn a_bitmap_and_a_metafile_that_holds_only_one_become_a_picture_file() {
    use crate::metafile::{dib_file, emf_bitmap, wmf_bitmap};
    let dib = small_dib();
    let file = dib_file(&dib).unwrap();
    assert_eq!(file.len(), 14 + dib.len());
    // "BM", the file's length, and where its pixels begin: after both headers.
    assert_eq!(&file[..14], &[b'B', b'M', 70, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0]);
    assert_eq!(&file[14..], &dib[..]);
    // Eight bits to the pixel: a table of 256 colours stands before the pixels.
    let mut indexed = dib.clone();
    indexed[14] = 8;
    indexed.resize(40 + 1024 + 8, 0);
    assert_eq!(dib_file(&indexed).unwrap()[10..14], (14u32 + 40 + 1024).to_le_bytes());

    // A metafile that sets things up and draws the one bitmap, placeable or not.
    let plain = wmf(&[(0x0103, vec![8, 0]), stretch(&dib)]);
    assert_eq!(wmf_bitmap(&plain), Some(file.clone()));
    let mut placeable = vec![0xD7, 0xCD, 0xC6, 0x9A];
    placeable.resize(22, 0);
    placeable.extend_from_slice(&plain);
    assert_eq!(wmf_bitmap(&placeable), Some(file.clone()));
    assert_eq!(emf_bitmap(&emf(&dib)), Some(file));

    // Anything else drawn (an ellipse), a second bitmap, a bitmap that is not copied as it is,
    // or no bitmap at all: not read.
    assert_eq!(wmf_bitmap(&wmf(&[stretch(&dib), (0x0418, vec![0; 8])])), None);
    assert_eq!(wmf_bitmap(&wmf(&[stretch(&dib), stretch(&dib)])), None);
    let mut inverted = stretch(&dib);
    inverted.1[0] = 0x21;
    assert_eq!(wmf_bitmap(&wmf(&[inverted])), None);
    assert_eq!(wmf_bitmap(&wmf(&[(0x0103, vec![8, 0])])), None);
}

#[test]
fn hostile_bitmaps_and_metafiles_are_not_read() {
    use crate::metafile::{dib_file, emf_bitmap, wmf_bitmap};
    let dib = small_dib();
    let files = [dib.clone(), wmf(&[stretch(&dib)]), emf(&dib)];
    for file in &files {
        // Cut short anywhere, and with any one byte turned over: no picture, or one that is
        // all there.
        for end in 0..file.len() {
            let _ = (dib_file(&file[..end]), wmf_bitmap(&file[..end]), emf_bitmap(&file[..end]));
            let mut changed = file.clone();
            changed[end] ^= 0xFF;
            for read in [dib_file(&changed), wmf_bitmap(&changed), emf_bitmap(&changed)].into_iter().flatten() {
                assert!(read.len() <= file.len() + 14);
            }
        }
    }
    assert_eq!(wmf_bitmap(&wmf(&[stretch(&dib)])[..40]), None);
    // A record of no length would never end; sizes beyond the data; a header that says its
    // colours fill more than there is.
    let mut endless = wmf(&[]);
    endless[18] = 0;
    assert_eq!(wmf_bitmap(&endless), None);
    let mut far = emf(&dib);
    far[88 + 48..88 + 52].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(emf_bitmap(&far), None);
    let mut greedy = dib.clone();
    greedy[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(dib_file(&greedy), None);
    assert_eq!(dib_file(&[0xFF; 64]), None);
}

#[test]
fn rtf_keeps_bitmaps_metafile_bitmaps_and_the_places_of_attachments() {
    let dib = small_dib();
    let rtf = format!(
        "{{\\rtf1\\ansi\\pard a{{\\pict\\dibitmap0\\picwgoal300\\pichgoal300 {}}}b{{\\pict\\wmetafile8 {}}}c{{\\pict\\emfblip {}}}\
         d{{\\pict\\wbitmap0 {}}}\\par e\\objattph\\'20f\\objattph\\par}}",
        hexed(&dib),
        hexed(&wmf(&[stretch(&dib)])),
        hexed(&emf(&dib)),
        hexed(&dib)
    );
    let doc = crate::rtf::parse(rtf.as_bytes());
    assert_eq!(doc.text(), "abcd\ne f");
    let pictures: Vec<_> = paras(&doc).iter().flat_map(|p| &p.runs).filter_map(|r| r.picture.clone().map(|p| (p, r.extent))).collect();
    assert_eq!(
        pictures,
        [
            (Picture::Inline(Some(0)), (20, 20)),
            // The same bitmap out of a metafile is the same picture, kept once.
            (Picture::Inline(Some(0)), (0, 0)),
            (Picture::Inline(Some(0)), (0, 0)),
            // A bitmap made for one device is not read.
            (Picture::Inline(None), (0, 0)),
            (Picture::Object(0), (0, 0)),
            (Picture::Object(1), (0, 0)),
        ]
    );
    assert_eq!(doc.written.len(), 1);
    assert_eq!(crate::metafile::dib_file(&dib).as_deref(), Some(&*doc.written[0]));
    assert_eq!(doc.objects, 2);

    // More places than are kept: the runs stop, the count goes on.
    let many = format!("{{\\rtf1 {}\\par}}", "\\objattph ".repeat(1000));
    let doc = crate::rtf::parse(many.as_bytes());
    assert_eq!(doc.objects, 1000);
    assert_eq!(paras(&doc).iter().flat_map(|p| &p.runs).filter(|r| r.picture.is_some()).count(), 256);
}

#[test]
fn a_large_rtf_picture_does_not_cut_the_text_after_it_short() {
    // More hex digits than the cap on text: they are data, and the text after them is read.
    let mut rtf = String::from("{\\rtf1\\ansi\\pard before{\\pict\\pngblip ");
    rtf.push_str(&"89504e47".repeat(400_000));
    rtf.push_str("}{\\*\\themedata ");
    rtf.push_str(&"504b0304".repeat(300_000));
    rtf.push_str("} after\\par}");
    let doc = crate::rtf::parse(rtf.as_bytes());
    assert!(!doc.truncated);
    assert_eq!(doc.text(), "before after");
    assert_eq!(doc.written.first().map(|p| p.len()), Some(1_600_000));
}

#[test]
fn rtf_cells_keep_their_lines_fills_places_and_merges() {
    use crate::rich::{Line, VAlign};
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi{\\colortbl;\\red204\\green0\\blue0;\\red255\\green230\\blue153;}\
          \\trowd\\trgaph108\\clbrdrt\\brdrs\\brdrw60\\clbrdrl\\brdrs\\clbrdrb\\brdrdb\\brdrw15\\clbrdrr\\brdrdash\\brdrw15\\brdrcf1\\clcbpat2\\clvertalb\\cellx2000\
          \\clbrdrl\\brdrdot\\clbrdrt\\brdrnone\\clbrdrb\\brdrs\\brdrw9999\\clvertalc\\cellx4000\\intbl a\\cell b\\cell\\row\
          \\trowd\\clbrdrt\\brdrs\\clbrdrb\\brdrs\\clvmgf\\cellx2000\\clbrdrt\\brdrs\\cellx4000\\intbl c\\cell d\\cell\\row\
          \\trowd\\clbrdrb\\brdrs\\brdrw30\\clvmrg\\cellx2000\\cellx4000\\intbl\\cell e\\cell\\row\
          \\trowd\\clvmrg\\cellx2000\\clvmrg\\cellx4000\\intbl f\\cell g\\cell\\row\\pard\\brdrb\\brdrs\\brdrw90 after\\par}",
    );
    let t = tables(&doc)[0];
    let a = &t.rows[0][0];
    let edges = a.edges.expect("lines");
    assert_eq!(edges.map(|e| e.width), [4, 1, 3, 1], "twips to pixels; a double line is three wide");
    assert_eq!(edges.map(|e| e.line), [Line::Solid, Line::Dashed, Line::Double, Line::Solid]);
    assert_eq!((edges[1].color, edges[0].color), (Some([204, 0, 0]), None));
    assert_eq!((a.background, a.valign, a.padding), (Some([255, 230, 153]), VAlign::Bottom, Some([1, 7, 1, 7])));
    // The line between two cells is drawn once, and no line is no line; widths are capped.
    let b = &t.rows[0][1];
    assert_eq!(b.edges.map(|e| e.map(|e| e.width)), Some([0, 0, 12, 0]));
    assert_eq!((b.valign, b.padding), (VAlign::Middle, Some([1, 7, 1, 7])));
    // A line under every cell of a row is the line above the next one.
    assert_eq!(t.rows[1][0].edges.map(|e| e.map(|e| e.width)), Some([0, 0, 2, 0]), "top shared; bottom is the last part's");
    assert_eq!(t.rows[1][1].edges, None, "its only line was the one it shares: {:?}", t.rows[1][1].edges);
    // The merged cell covers three rows and the cells under it are gone; a cell that continues
    // nothing is a cell of its own.
    assert_eq!(t.rows[1][0].rows, 3);
    assert_eq!(t.rows.iter().map(Vec::len).collect::<Vec<_>>(), [2, 2, 1, 1]);
    assert_eq!(t.places(), [vec![0, 1], vec![0, 1], vec![1], vec![1]]);
    assert_eq!(doc.text().lines().last(), Some("after"));
    // A paragraph's border is not the border of the cell defined last.
    assert!(t.rows[3][0].edges.is_none());
}

#[test]
fn hostile_rtf_pictures_and_tables_are_capped() {
    let mut rtf = String::from("{\\rtf1\\ansi{\\colortbl;\\red1\\green2\\blue3;}");
    // More pictures than are kept, each different, with sizes and scales out of range.
    for i in 0..300 {
        rtf.push_str(&format!("{{\\pict\\pngblip\\picwgoal2147483647\\pichgoal-5\\picscalex2147483647\\picscaley-2147483648 89504e47{i:08x}}}"));
    }
    // Rows of cells merged down from nothing, without end, with borders of every width.
    for i in 0..2000 {
        rtf.push_str("\\trowd\\trgaph2147483647");
        for c in 0..80 {
            rtf.push_str(&format!("\\clbrdrt\\brdrdb\\brdrw{}\\brdrcf9999\\clbrdrl\\brdrw-7\\clvmrg\\clcbpat-1\\cellx{}", i * 97, c * 100 - 300));
        }
        rtf.push_str("\\intbl x\\cell y\\cell\\row");
    }
    rtf.push_str("\\trowd\\clvmgf\\cellx-2147483648\\clvmgf\\cellx2147483647\\intbl a\\cell b\\cell\\row{\\pict\\pngblip 8950");
    let doc = crate::rtf::parse(rtf.as_bytes());
    assert!(doc.written.len() <= 64);
    fn check(blocks: &[Block]) {
        for block in blocks {
            match block {
                Block::Para(p) => assert!(p.runs.iter().all(|r| r.extent.0 <= 4096 && r.extent.1 <= 4096)),
                Block::Table(t) => {
                    assert_eq!(t.places().len(), t.rows.len());
                    for cell in t.rows.iter().flatten() {
                        assert!(cell.rows <= 1000 && cell.width <= 4096);
                        assert!(cell.edges.is_none_or(|e| e.iter().all(|e| e.width <= 12)));
                        assert!(cell.padding.is_some_and(|p| p.iter().all(|v| *v <= 64)));
                        check(&cell.blocks);
                    }
                }
                Block::Rule => {}
            }
        }
    }
    check(&doc.blocks);
    // One cell merged down a thousand rows and more stays within the cap.
    let mut rtf = String::from("{\\rtf1\\trowd\\clvmgf\\cellx1000\\cellx2000\\intbl a\\cell b\\cell\\row");
    rtf.push_str(&"\\trowd\\clvmrg\\cellx1000\\cellx2000\\intbl\\cell c\\cell\\row".repeat(1500));
    let doc = crate::rtf::parse(rtf.as_bytes());
    assert!(tables(&doc).iter().all(|t| t.rows.iter().flatten().all(|c| c.rows <= 1000)));
}

#[test]
fn a_link_that_is_not_what_its_text_says_is_told_apart() {
    use crate::links::{Concern, Place, concern, concern_at, place, text_around};
    let at = |link: &str| place(link).map(|p| p.host);
    for (link, host) in [
        ("https://Example.ORG/path?x=1#y", Some("example.org")),
        ("http://www.example.org.", Some("example.org")),
        ("https://example.org:8443/a@b", Some("example.org")),
        ("https://user:secret@example.org/", Some("example.org")),
        ("https://[2001:db8::1]:443/", Some("2001:db8::1")),
        ("https:\\\\example.org\\path", Some("example.org")),
        ("ht\ttps://exa\nmple.org/", Some("example.org")),
        ("mailto:Ana.Costa@Example.org?subject=Hi", Some("example.org")),
        ("ftp://files.example.net", Some("files.example.net")),
        ("#top", None),
        ("page.html", None),
        ("tel:+15550100", None),
        ("cid:logo@example.org", None),
        ("https://", None),
        ("https:///path", None),
        ("mailto:nobody", None),
        ("", None),
        (":", None),
        ("1http://example.org", None),
    ] {
        assert_eq!(at(link).as_deref(), host, "{link:?}");
    }
    assert_eq!(place("https://bank.example@evil.test/"), Some(Place { scheme: "https".into(), host: "evil.test".into(), disguised: true }));

    let elsewhere = |says: &str, goes: &str| Some(Concern::Elsewhere { says: says.into(), goes: goes.into() });
    for (text, link, want) in [
        // What it seems.
        ("our help pages", "https://parking.example.net/help", None),
        ("parking.example.net/help", "https://parking.example.net/help", None),
        ("www.example.org", "https://example.org/welcome", None),
        ("example.org", "https://login.example.org/", None),
        ("https://login.example.org/start", "https://example.org/", None),
        ("See https://example.org/a.", "http://EXAMPLE.org/b", None),
        ("ana@example.org", "mailto:ana@example.org", None),
        ("Ana Costa", "mailto:ana@example.org", None),
        ("report.pdf", "https://files.example.net/report.pdf", None),
        ("Read more at once.", "https://news.example.net/", None),
        ("v1.2", "https://example.org/releases", None),
        ("ana@example.org", "https://example.net/people/ana", None),
        ("here", "#section", None),
        ("call us", "tel:+15550100", None),
        // Not what it seems.
        ("https://bank.example/login", "https://bank.example.evil.test/login", elsewhere("bank.example", "bank.example.evil.test")),
        ("www.bank.example", "http://evil.test/bank.example", elsewhere("bank.example", "evil.test")),
        ("bank.example", "https://evil.test/", elsewhere("bank.example", "evil.test")),
        ("Sign in at https://bank.example today", "https://notbank.example/", elsewhere("bank.example", "notbank.example")),
        ("help@bank.example", "mailto:help@evil.test", elsewhere("bank.example", "evil.test")),
        ("bank.example", "https://bank.example@evil.test/", Some(Concern::Disguised { goes: "evil.test".into() })),
        ("your account", "https://xn--bnk-5ma.example/", Some(Concern::OtherLetters { goes: "xn--bnk-5ma.example".into() })),
        ("your account", "https://b\u{430}nk.example/", Some(Concern::OtherLetters { goes: "b\u{430}nk.example".into() })),
        ("your account", "http://192.0.2.7/login", Some(Concern::Number { goes: "192.0.2.7".into() })),
        ("your account", "http://0xC0.0.2.7/", Some(Concern::Number { goes: "0xc0.0.2.7".into() })),
        ("your account", "http://3221225991/", Some(Concern::Number { goes: "3221225991".into() })),
        ("your account", "http://[2001:db8::1]/", Some(Concern::Number { goes: "2001:db8::1".into() })),
        ("abc.de", "https://abc.de/", None),
        ("click", "javascript:alert(1)", Some(Concern::NotAnAddress { scheme: "javascript".into() })),
        ("click", " Java\tScript:alert(1)", Some(Concern::NotAnAddress { scheme: "javascript".into() })),
        ("the file", "file:///C:/Windows/system.ini", Some(Concern::NotAnAddress { scheme: "file".into() })),
        ("open", "ms-word:ofe|u|https://example.org/x.docx", Some(Concern::NotAnAddress { scheme: "ms-word".into() })),
    ] {
        assert_eq!(concern(text, link), want, "{text:?} -> {link:?}");
    }
    assert_eq!(elsewhere("a.example", "b.example").unwrap().words(), "The text names a.example, but the link goes to b.example.");

    // A link's text is all the runs that go to the same address, however they are styled.
    let doc = parse(r#"<p>Go to <a href="https://evil.test/">www.<b>bank</b>.example</a> or <a href="https://bank.example/">bank.example</a>.</p>"#);
    let Some(Block::Para(para)) = doc.blocks.first() else { panic!("a paragraph") };
    let told: Vec<(String, bool)> = (0..para.runs.len()).map(|i| (text_around(&para.runs, i), concern_at(&para.runs, i).is_some())).collect();
    let links: Vec<&(String, bool)> = told.iter().filter(|(text, _)| !text.is_empty()).collect();
    assert_eq!(links.len(), 4, "three runs of the first link and one of the second");
    assert!(links[..3].iter().all(|l| **l == ("www.bank.example".to_string(), true)));
    assert_eq!(*links[3], ("bank.example".to_string(), false));

    // Hostile: nothing here panics, whatever stands in an address or a text.
    let pieces = [
        "", ":", "//", "@", "[", "]", "\\", "?", "#", ".", "..", "xn--", "http", "https://", "mailto:", "é", "\u{0}", "\u{202e}", "www.", "a.b", "0x",
    ];
    for a in pieces {
        for b in pieces {
            for c in pieces {
                let joined = format!("{a}{b}{c}");
                let _ = concern(&joined, &joined);
                let _ = concern(c, &joined);
                let _ = place(&joined);
            }
        }
    }
    let long = "a.".repeat(100_000);
    let _ = concern(&long, &format!("https://{long}@{long}/"));
    assert_eq!(text_around(&[], 3), "");
    assert_eq!(concern_at(&[], 0), None);
}

/// The words of a record's parameters.
fn words(list: &[i16]) -> Vec<u8> {
    list.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// The colour of one pixel of a BMP file of 24 bits to the pixel.
fn pixel(bmp: &[u8], x: usize, y: usize) -> [u8; 3] {
    let (w, h) = bmp_size(bmp);
    let at = 54 + (h - 1 - y) * ((w * 3 + 3) & !3) + x * 3;
    [bmp[at + 2], bmp[at + 1], bmp[at]]
}

fn bmp_size(bmp: &[u8]) -> (usize, usize) {
    let n = |at: usize| u32::from_le_bytes(bmp[at..at + 4].try_into().unwrap()) as usize;
    assert_eq!((&bmp[..2], n(2), n(10)), (&b"BM"[..], bmp.len(), 54));
    (n(18), n(22))
}

/// Records that set a window of a hundred units each way, and a red brush.
fn red_window() -> Vec<(u16, Vec<u8>)> {
    vec![(0x020B, words(&[0, 0])), (0x020C, words(&[100, 100])), (0x02FC, vec![0, 0, 255, 0, 0, 0, 0, 0]), (0x012D, words(&[0]))]
}

const RED: [u8; 3] = [255, 0, 0];
const WHITE: [u8; 3] = [255; 3];

#[test]
fn a_metafile_that_is_a_drawing_is_drawn() {
    use crate::metafile::wmf_picture;
    // A red box with the pen's black line round it, drawn at twice the size it is shown at.
    let mut records = red_window();
    records.push((0x041B, words(&[80, 80, 20, 20])));
    let bmp = wmf_picture(&wmf(&records), Some((50, 50))).unwrap();
    assert_eq!(bmp_size(&bmp), (100, 100));
    assert_eq!(pixel(&bmp, 50, 50), RED);
    assert_eq!(pixel(&bmp, 5, 5), WHITE);
    assert_eq!(pixel(&bmp, 90, 50), WHITE);
    let edge = pixel(&bmp, 20, 50);
    assert!(edge != RED && edge != WHITE && edge[1] == 0, "{edge:?}");

    // No size asked for: a placeable metafile says its own (an inch by half an inch).
    let mut placeable = vec![0xD7, 0xCD, 0xC6, 0x9A, 0, 0];
    placeable.extend(words(&[0, 0, 1440, 720, 1440]));
    placeable.resize(22, 0);
    placeable.extend(wmf(&[(0x0418, words(&[720, 1440, 0, 0]))]));
    let bmp = wmf_picture(&placeable, None).unwrap();
    assert_eq!(bmp_size(&bmp), (96, 48));
    // The ellipse is filled with the white brush and lined in black: its ends are dark.
    assert_eq!(pixel(&bmp, 48, 24), WHITE);
    assert!(pixel(&bmp, 0, 24)[0] < 200 && pixel(&bmp, 95, 24)[0] < 200);
    assert_eq!(pixel(&bmp, 1, 1), WHITE);
    // A plain one that says nothing has its shape, at a size that can be looked at.
    let wide = wmf(&[(0x020C, words(&[50, 200])), (0x0213, words(&[50, 200]))]);
    assert_eq!(bmp_size(&wmf_picture(&wide, None).unwrap()), (600, 150));

    // One square inside another, both drawn the same way round: the inner one is a hole when
    // every other part is filled, and filled when all that is wound round is.
    let nested = |mode: i16| {
        let mut records = red_window();
        records.push((0x0106, words(&[mode])));
        records.push((0x0538, words(&[2, 4, 4, 10, 10, 90, 10, 90, 90, 10, 90, 40, 40, 60, 40, 60, 60, 40, 60])));
        wmf_picture(&wmf(&records), Some((50, 50))).unwrap()
    };
    assert_eq!((pixel(&nested(1), 50, 50), pixel(&nested(1), 25, 50)), (WHITE, RED));
    assert_eq!((pixel(&nested(2), 50, 50), pixel(&nested(2), 25, 50)), (RED, RED));

    // A pen that draws nothing, a pie, and a state that is saved and put back: what is cut
    // off inside it is whole again after.
    let mut records = red_window();
    records.extend([
        (0x02FA, vec![5, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        (0x012D, words(&[1])),
        (0x001E, Vec::new()),
        (0x0416, words(&[50, 50, 0, 0])),
        (0x041B, words(&[100, 100, 0, 0])),
        (0x0127, words(&[-1])),
        (0x081A, words(&[100, 75, 75, 100, 100, 100, 50, 50])),
    ]);
    let bmp = wmf_picture(&wmf(&records), Some((50, 50))).unwrap();
    assert_eq!((pixel(&bmp, 20, 20), pixel(&bmp, 80, 20), pixel(&bmp, 20, 80)), (RED, WHITE, WHITE));
    // The pie runs from the right of its ellipse round to the bottom, against the clock: all
    // of it but the quarter at the lower right.
    assert_eq!((pixel(&bmp, 65, 65), pixel(&bmp, 85, 85)), (RED, WHITE));
    assert_eq!((pixel(&bmp, 65, 85), pixel(&bmp, 85, 65)), (RED, RED));
}

#[test]
fn a_metafile_that_cannot_be_drawn_whole_is_not_drawn() {
    use crate::metafile::wmf_picture;
    let boxed = (0x041B, words(&[80, 80, 20, 20]));
    let with = |extra: (u16, Vec<u8>)| {
        let mut records = red_window();
        records.extend([boxed.clone(), extra]);
        wmf_picture(&wmf(&records), None)
    };
    assert!(with((0x0102, words(&[1]))).is_some());
    // Text, a pen that inverts, a brush of a kind nobody here knows that is filled with, a
    // hatching that is none of the six, a colour that is a number in a palette.
    assert_eq!(with((0x0A32, words(&[10, 10, 1, 0, 0x41]))), None);
    assert_eq!(with((0x0104, words(&[6]))), None);
    for brush in [vec![3, 0, 255, 0, 0, 0, 0, 0], vec![2, 0, 255, 0, 0, 0, 6, 0]] {
        assert_eq!(wmf_picture(&wmf(&[(0x020C, words(&[100, 100])), (0x02FC, brush), (0x012D, words(&[0])), boxed.clone()]), None), None);
    }
    // A bitmap that goes to a box of no size draws nothing, and is no reason to draw nothing.
    assert!(with(stretch(&small_dib())).is_some());
    assert_eq!(wmf_picture(&wmf(&[(0x020C, words(&[100, 100])), (0x02FC, vec![0, 0, 5, 0, 0, 1, 0, 0]), boxed.clone()]), None), None);
    // Nothing drawn, no window to draw in, a window of no size, an object that was never made,
    // a state put back that was never saved.
    assert_eq!(wmf_picture(&wmf(&red_window()), None), None);
    assert_eq!(wmf_picture(&wmf(std::slice::from_ref(&boxed)), None), None);
    assert_eq!(wmf_picture(&wmf(&[(0x020C, words(&[0, 100])), boxed.clone()]), None), None);
    assert_eq!(with((0x012D, words(&[7]))), None);
    assert_eq!(with((0x0127, words(&[-1]))), None);
}

#[test]
fn hostile_drawings_never_panic() {
    use crate::metafile::wmf_picture;
    let mut records = red_window();
    records.extend([
        (0x02FA, vec![0, 0, 9, 0, 0, 0, 0, 0, 255, 0]),
        (0x012D, words(&[1])),
        (0x001E, Vec::new()),
        (0x0416, words(&[90, 90, 10, 10])),
        (0x0410, words(&[1, 2, 1, 2])),
        (0x020F, words(&[5, 5])),
        (0x0214, words(&[10, 10])),
        (0x0213, words(&[90, 90])),
        (0x0325, words(&[3, 0, 0, 50, 50, 100, 0])),
        (0x0324, words(&[3, 0, 0, 50, 50, 100, 0])),
        (0x0538, words(&[1, 3, 0, 0, 50, 50, 100, 0])),
        (0x0418, words(&[80, 80, 20, 20])),
        (0x061C, words(&[20, 20, 80, 80, 20, 20])),
        (0x0817, words(&[0, 50, 50, 0, 100, 100, 0, 0])),
        (0x0830, words(&[0, 50, 50, 0, 100, 100, 0, 0])),
        (0x041F, vec![0, 255, 0, 0, 30, 0, 30, 0]),
        (0x0127, words(&[1])),
        (0x01F0, words(&[0])),
    ]);
    let file = wmf(&records);
    assert!(wmf_picture(&file, Some((40, 40))).is_some());
    // Cut short anywhere, and with any one byte turned over or made the largest it can be.
    for end in 0..file.len() {
        let _ = wmf_picture(&file[..end], Some((24, 24)));
        for with in [file[end] ^ 0xFF, 0xFF, 0x80, 0] {
            let mut changed = file.clone();
            changed[end] = with;
            for bmp in [wmf_picture(&changed, Some((24, 24))), wmf_picture(&changed, Some((u32::MAX, 1)))].into_iter().flatten() {
                let (w, h) = bmp_size(&bmp);
                assert!(w <= 800 && h <= 800);
            }
        }
    }
    // More points than a record holds; more polygons than there are; a great many objects.
    assert_eq!(wmf_picture(&wmf(&[(0x020C, words(&[100, 100])), (0x0324, words(&[-1, 0, 0]))]), None), None);
    assert_eq!(wmf_picture(&wmf(&[(0x020C, words(&[100, 100])), (0x0538, words(&[-1, 3]))]), None), None);
    let many: Vec<_> = (0..5000).map(|_| (0x02FBu16, Vec::new())).collect();
    assert_eq!(wmf_picture(&wmf(&many), None), None);
    // A drawing that would take too long: a great many long lines of the widest pen.
    let mut slow = vec![(0x020C, words(&[100, 100])), (0x02FA, vec![0, 0, 100, 0, 0, 0, 0, 0, 0, 0]), (0x012D, words(&[0]))];
    let mut zigzag = vec![2000i16];
    zigzag.extend((0..2000).flat_map(|i| [if i % 2 == 0 { 0 } else { 100 }, i % 100]));
    slow.extend((0..200).map(|_| (0x0325, words(&zigzag))));
    let started = std::time::Instant::now();
    let _ = wmf_picture(&wmf(&slow), Some((400, 400)));
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}

#[test]
fn rtf_shows_a_metafile_that_is_a_drawing() {
    let mut records = red_window();
    records.push((0x041B, words(&[80, 80, 20, 20])));
    let rtf = format!("{{\\rtf1\\ansi\\pard a{{\\pict\\wmetafile8\\picwgoal600\\pichgoal300 {}}}b\\par}}", hexed(&wmf(&records)));
    let doc = crate::rtf::parse(rtf.as_bytes());
    let run = paras(&doc)[0].runs.iter().find(|r| r.picture.is_some()).unwrap();
    assert_eq!((run.picture.clone(), run.extent), (Some(Picture::Inline(Some(0))), (40, 20)));
    assert_eq!(bmp_size(&doc.written[0]), (80, 40));
    assert_eq!(pixel(&doc.written[0], 40, 20), RED);
}

/// The long numbers of a record.
fn longs(list: &[i32]) -> Vec<u8> {
    list.iter().flat_map(|n| n.to_le_bytes()).collect()
}

/// An enhanced metafile of these records (what each is, and what follows its size): a picture
/// a hundred pixels each way on a screen of 96 to the inch.
fn emf_of(records: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut file = vec![0u8; 88];
    for (at, n) in [(0, 1), (4, 88), (16, 99), (20, 99), (32, 2646), (36, 2646), (72, 1920), (76, 1080), (80, 508), (84, 286)] {
        file[at..at + 4].copy_from_slice(&i32::to_le_bytes(n));
    }
    file[40..44].copy_from_slice(b" EMF");
    for (kind, body) in records.iter().chain(&[(14u32, longs(&[0, 0, 20]))]) {
        file.extend(kind.to_le_bytes());
        file.extend(u32::try_from(8 + body.len()).unwrap().to_le_bytes());
        file.extend(body);
    }
    file
}

/// Records that make a red brush and select it, and select the pen that draws nothing.
fn red_brush() -> Vec<(u32, Vec<u8>)> {
    let mut brush = longs(&[1, 0]);
    brush.extend([255, 0, 0, 0, 0, 0, 0, 0]);
    vec![(39, brush), (37, longs(&[1])), (37, 0x8000_0008u32.to_le_bytes().to_vec())]
}

#[test]
fn an_enhanced_metafile_that_is_a_drawing_is_drawn() {
    use crate::metafile::emf_picture;
    let draw = |more: Vec<(u32, Vec<u8>)>, want| {
        let mut records = red_brush();
        records.extend(more);
        emf_picture(&emf_of(&records), want)
    };
    let at = |bmp: &[u8], points: [(usize, usize); 2]| points.map(|(x, y)| pixel(bmp, x, y));
    // A box, at twice the size it is shown at; with no size asked for, at the frame's own.
    let bmp = draw(vec![(43, longs(&[20, 20, 80, 80]))], Some((50, 50))).unwrap();
    assert_eq!(bmp_size(&bmp), (100, 100));
    assert_eq!(at(&bmp, [(50, 50), (10, 10)]), [RED, WHITE]);
    assert_eq!(bmp_size(&draw(vec![(43, longs(&[20, 20, 80, 80]))], None).unwrap()), (100, 100));

    // A path: a triangle that is closed and filled.
    let path = vec![
        (59, Vec::new()),
        (27, longs(&[10, 10])),
        (54, longs(&[90, 10])),
        (54, longs(&[50, 90])),
        (61, Vec::new()),
        (60, Vec::new()),
        (62, longs(&[0, 0, 99, 99])),
    ];
    assert_eq!(at(&draw(path.clone(), Some((50, 50))).unwrap(), [(50, 30), (12, 80)]), [RED, WHITE]);
    // A path that is only begun draws nothing.
    assert_eq!(draw(path[..6].to_vec(), None), None);

    // The world turned: half the size, and moved to the middle.
    let turn: Vec<u8> = [0.5f32, 0.0, 0.0, 0.5, 50.0, 50.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    let bmp = draw(vec![(35, turn.clone()), (43, longs(&[0, 0, 100, 100]))], Some((50, 50))).unwrap();
    assert_eq!(at(&bmp, [(75, 75), (25, 25)]), [RED, WHITE]);
    // The same turn put after one that was there, and a state put back: the box is whole again.
    let mut after = turn.clone();
    after.extend(longs(&[3]));
    let bmp = draw(vec![(33, Vec::new()), (36, after), (34, longs(&[-1])), (43, longs(&[0, 0, 100, 100]))], Some((50, 50))).unwrap();
    assert_eq!(at(&bmp, [(75, 75), (25, 25)]), [RED, RED]);

    // A window of a thousand units onto a hundred of the device's.
    let window = vec![(17, longs(&[8])), (9, longs(&[1000, 1000])), (11, longs(&[100, 100])), (43, longs(&[200, 200, 800, 800]))];
    assert_eq!(at(&draw(window, Some((50, 50))).unwrap(), [(50, 50), (10, 10)]), [RED, WHITE]);

    // Points of short numbers: a polygon, and a curve that runs on from where the pen is.
    let mut polygon = longs(&[0, 0, 99, 99, 3]);
    polygon.extend(words(&[10, 10, 90, 10, 50, 90]));
    assert_eq!(at(&draw(vec![(86, polygon)], Some((50, 50))).unwrap(), [(50, 30), (12, 80)]), [RED, WHITE]);
    let mut curve = longs(&[0, 0, 99, 99, 3]);
    curve.extend(words(&[10, 90, 90, 90, 90, 10]));
    let bmp = draw(vec![(37, 0x8000_0007u32.to_le_bytes().to_vec()), (27, longs(&[10, 10])), (88, curve)], Some((50, 50))).unwrap();
    // It hangs down from both ends to seventy in the middle.
    assert!(pixel(&bmp, 50, 70)[0] < 200 && pixel(&bmp, 50, 30) == WHITE && pixel(&bmp, 50, 85) == WHITE);

    // A path that is one box is cut to; a region of no size brings the whole page back.
    let cut = vec![(59, Vec::new()), (43, longs(&[0, 0, 50, 50])), (60, Vec::new()), (67, longs(&[5])), (43, longs(&[0, 0, 100, 100]))];
    assert_eq!(at(&draw(cut.clone(), Some((50, 50))).unwrap(), [(25, 25), (75, 75)]), [RED, WHITE]);
    let mut back = cut;
    back.extend([(75, longs(&[0, 5])), (43, longs(&[0, 0, 100, 100]))]);
    assert_eq!(at(&draw(back, Some((50, 50))).unwrap(), [(25, 25), (75, 75)]), [RED, RED]);

    // A pie from the right of its ellipse round to the bottom, against the clock and with it.
    let pie = |way: i32| draw(vec![(57, longs(&[way])), (47, longs(&[0, 0, 100, 100, 100, 50, 50, 100]))], Some((50, 50))).unwrap();
    assert_eq!(at(&pie(1), [(30, 30), (70, 70)]), [RED, WHITE]);
    assert_eq!(at(&pie(2), [(30, 30), (70, 70)]), [WHITE, RED]);
}

#[test]
fn an_enhanced_metafile_that_cannot_be_drawn_whole_is_not_drawn() {
    use crate::metafile::emf_picture;
    let with = |extra: (u32, Vec<u8>)| {
        let mut records = red_brush();
        records.extend([(43, longs(&[20, 20, 80, 80])), extra]);
        emf_picture(&emf_of(&records), None)
    };
    assert!(with((18, longs(&[1]))).is_some());
    // Text, a bitmap whose record is cut short, a region that is not there, a pen that
    // inverts, a mapping mode in parts of a millimetre, a drawing from right to left.
    assert_eq!(with(emf_text((10, 40), "A", 0)), None);
    assert_eq!(with((81, vec![0; 40])), None);
    assert_eq!(with((71, vec![0; 24])), None);
    assert_eq!(with((20, longs(&[6]))), None);
    assert_eq!(with((17, longs(&[2]))), None);
    assert_eq!(with((115, longs(&[1]))), None);
    // A bitmap that goes to a box of no size draws nothing, and is no reason to draw nothing.
    assert!(with((81, vec![0; 72])).is_some());
    // A brush of a pattern that is not there, filled with; an object that was never made; a
    // state put back that was never saved; a path that is cut to and was never made.
    let mut hatched = longs(&[2, 3]);
    hatched.extend([0; 8]);
    assert_eq!(emf_picture(&emf_of(&[(39, hatched), (37, longs(&[2])), (43, longs(&[20, 20, 80, 80]))]), None), None);
    assert_eq!(with((37, longs(&[9]))), None);
    assert_eq!(with((34, longs(&[-1]))), None);
    assert_eq!(with((67, longs(&[5]))), None);
    // Nothing drawn: only what is said beside the drawing, as in a file whose drawing is all
    // in its comments. No frame and no box that was drawn in.
    assert_eq!(emf_picture(&emf_of(&[(70, longs(&[4, 0x2B46_4D45]))]), None), None);
    let mut empty = emf_of(&[(43, longs(&[20, 20, 80, 80]))]);
    empty[8..40].copy_from_slice(&longs(&[0, 0, -1, -1, 0, 0, 0, 0]));
    assert_eq!(emf_picture(&empty, None), None);
}

#[test]
fn hostile_enhanced_drawings_never_panic() {
    use crate::metafile::emf_picture;
    let mut records = red_brush();
    let turn: Vec<u8> = [0.9f32, 0.1, -0.1, 0.9, 5.0, 5.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut pen = longs(&[2, 0, 3, 0]);
    pen.extend([0, 0, 255, 0]);
    let mut wide = longs(&[3, 0, 0, 0, 0, 0x0001_0000, 4, 0]);
    wide.extend([0, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut many = longs(&[0, 0, 99, 99, 2, 6, 3, 3]);
    many.extend(longs(&[10, 10, 90, 10, 50, 90, 20, 20, 80, 20, 50, 80]));
    let mut region = longs(&[48, 1, 32, 1, 1, 16, 0, 0, 90, 90]);
    region.extend(longs(&[5, 5, 90, 90]));
    records.extend([
        (38, pen),
        (37, longs(&[2])),
        (95, wide),
        (37, longs(&[3])),
        (33, Vec::new()),
        (75, region),
        (30, longs(&[10, 10, 90, 90])),
        (8, many.clone()),
        (7, many),
        (36, turn.iter().copied().chain(longs(&[2])).collect()),
        (42, longs(&[20, 20, 80, 80])),
        (44, longs(&[20, 20, 80, 80, 10, 10])),
        (45, longs(&[0, 0, 100, 100, 50, 0, 0, 50])),
        (46, longs(&[0, 0, 100, 100, 50, 0, 0, 50])),
        (55, longs(&[0, 0, 100, 100, 50, 0, 0, 50])),
        (59, Vec::new()),
        (27, longs(&[10, 10])),
        (6, longs(&[0, 0, 99, 99, 2, 90, 10, 50, 90])),
        (5, longs(&[0, 0, 99, 99, 3, 40, 80, 20, 60, 10, 10])),
        (61, Vec::new()),
        (60, Vec::new()),
        (63, longs(&[0, 0, 99, 99])),
        (15, longs(&[30, 30, 0x00FF_0000])),
        (34, longs(&[1])),
        (40, longs(&[1])),
    ]);
    let file = emf_of(&records);
    assert!(emf_picture(&file, Some((40, 40))).is_some());
    // Cut short anywhere, and with any one byte turned over or made the largest it can be.
    for end in 0..file.len() {
        let _ = emf_picture(&file[..end], Some((24, 24)));
        for with in [file[end] ^ 0xFF, 0xFF, 0x80, 0] {
            let mut changed = file.clone();
            changed[end] = with;
            for bmp in [emf_picture(&changed, Some((24, 24))), emf_picture(&changed, None)].into_iter().flatten() {
                let (w, h) = bmp_size(&bmp);
                assert!(w <= 800 && h <= 800);
            }
        }
    }
    // More points than a record holds; more polygons than there are; an object far out.
    assert_eq!(emf_picture(&emf_of(&[(3, longs(&[0, 0, 99, 99, -1, 0, 0]))]), None), None);
    assert_eq!(emf_picture(&emf_of(&[(8, longs(&[0, 0, 99, 99, -1, 3, 3]))]), None), None);
    assert_eq!(emf_picture(&emf_of(&[(39, longs(&[i32::MAX, 0, 0, 0])), (43, longs(&[20, 20, 80, 80]))]), None), None);
    // A turn that is no number.
    let nan: Vec<u8> = [f32::NAN, 0.0, 0.0, 1.0, 0.0, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(emf_picture(&emf_of(&[(35, nan), (43, longs(&[20, 20, 80, 80]))]), None), None);
}

#[test]
fn rtf_shows_an_enhanced_metafile_that_is_a_drawing() {
    let mut records = red_brush();
    records.push((43, longs(&[20, 20, 80, 80])));
    let rtf = format!("{{\\rtf1\\ansi\\pard a{{\\pict\\emfblip\\picwgoal600\\pichgoal600 {}}}b\\par}}", hexed(&emf_of(&records)));
    let doc = crate::rtf::parse(rtf.as_bytes());
    let run = paras(&doc)[0].runs.iter().find(|r| r.picture.is_some()).unwrap();
    assert_eq!((run.picture.clone(), run.extent), (Some(Picture::Inline(Some(0))), (40, 40)));
    assert_eq!(bmp_size(&doc.written[0]), (80, 80));
    assert_eq!(pixel(&doc.written[0], 40, 40), RED);
}

/// A font of boxes, which stands for whatever font a drawing names: every letter is a box
/// from 0.05 to 0.45 of the em across and 0.7 of it high, half an em from the next.
fn boxes(chars: &str) -> openmailclient_fonts::Face {
    openmailclient_fonts::Face { data: openmailclient_fonts::boxes::font(chars), index: 0 }
}

const BLUE: [u8; 3] = [0, 0, 255];

/// META_CREATEFONTINDIRECT for a font this high, turned so many tenths of a degree, with these
/// four bytes for italic, underlined, struck out and its character set.
fn wmf_font(height: i16, turn: i16, ways: [u8; 4]) -> (u16, Vec<u8>) {
    let mut body = words(&[height, 0, turn, 0, 400]);
    body.extend(ways);
    body.extend([0; 4]);
    body.extend(b"Box\0");
    (0x02FB, body)
}

/// A window of a hundred units each way, a font twenty units high that is selected, red
/// letters and nothing filled in behind them.
fn lettered(font: (u16, Vec<u8>)) -> Vec<(u16, Vec<u8>)> {
    vec![(0x020B, words(&[0, 0])), (0x020C, words(&[100, 100])), font, (0x012D, words(&[0])), (0x0209, vec![255, 0, 0, 0]), (0x0102, words(&[1]))]
}

/// META_EXTTEXTOUT of `text` at a point, with these options and what follows the text.
fn ext_text(at: (i16, i16), text: &[u8], options: i16, after: &[i16]) -> (u16, Vec<u8>) {
    let mut body = words(&[at.1, at.0, i16::try_from(text.len()).unwrap(), options]);
    body.extend(text);
    if text.len() % 2 == 1 {
        body.push(0);
    }
    body.extend(words(after));
    (0x0A32, body)
}

#[test]
fn text_in_a_metafile_is_drawn() {
    use crate::metafile::{wmf_drawn, wmf_picture};
    let face = boxes("AB ");
    let draw = |font: (u16, Vec<u8>), more: Vec<(u16, Vec<u8>)>| {
        let mut records = lettered(font);
        records.extend(more);
        wmf_drawn(&wmf(&records), Some((50, 50)), Some(&face))
    };
    let plain = || wmf_font(-20, 0, [0, 0, 0, 0]);

    // Two letters whose top left corner is the point given: boxes fourteen high that stand on
    // the line sixteen below it.
    let drawn = draw(plain(), vec![ext_text((10, 40), b"AB", 0, &[])]).unwrap();
    assert_eq!((bmp_size(&drawn.file), drawn.words.as_str()), ((100, 100), "AB"));
    for (x, y, colour) in [(15, 50, RED), (25, 50, RED), (20, 50, WHITE), (15, 40, WHITE), (15, 58, WHITE), (35, 50, WHITE)] {
        assert_eq!(pixel(&drawn.file, x, y), colour, "{x} {y}");
    }
    // With nobody to ask for the letters the drawing is not made at all.
    let mut records = lettered(plain());
    records.push(ext_text((10, 40), b"AB", 0, &[]));
    assert_eq!(wmf_picture(&wmf(&records), Some((50, 50))), None);

    // The colour behind the letters is filled in unless the drawing says not.
    let drawn = draw(plain(), vec![(0x0201, vec![0, 0, 255, 0]), (0x0102, words(&[2])), ext_text((10, 40), b"AB", 0, &[])]).unwrap();
    assert_eq!((pixel(&drawn.file, 20, 50), pixel(&drawn.file, 15, 50), pixel(&drawn.file, 20, 62)), (BLUE, RED, WHITE));

    // The drawing's own spacing is kept: the second letter thirty units on.
    let drawn = draw(plain(), vec![ext_text((10, 40), b"AB", 0, &[30, 30])]).unwrap();
    assert_eq!((pixel(&drawn.file, 45, 50), pixel(&drawn.file, 25, 50)), (RED, WHITE));

    // Set by its middle and by the line it stands on.
    let drawn = draw(plain(), vec![(0x012E, words(&[6 | 24])), ext_text((50, 50), b"AB", 0, &[])]).unwrap();
    assert_eq!((pixel(&drawn.file, 45, 45), pixel(&drawn.file, 55, 45), pixel(&drawn.file, 45, 52)), (RED, RED, WHITE));
    // Set by its right end and its bottom.
    let drawn = draw(plain(), vec![(0x012E, words(&[2 | 8])), ext_text((50, 50), b"AB", 0, &[])]).unwrap();
    assert_eq!((pixel(&drawn.file, 35, 40), pixel(&drawn.file, 45, 40), pixel(&drawn.file, 55, 40)), (RED, RED, WHITE));

    // A line turned a quarter round runs up the page.
    let drawn = draw(wmf_font(-20, 900, [0, 0, 0, 0]), vec![ext_text((50, 80), b"A", 0, &[])]).unwrap();
    assert_eq!((pixel(&drawn.file, 60, 75), pixel(&drawn.file, 45, 75), pixel(&drawn.file, 60, 85)), (RED, WHITE, WHITE));

    // A font given by the height of its lines, underlined and struck out.
    let drawn = draw(wmf_font(20, 0, [0, 1, 1, 0]), vec![ext_text((10, 40), b"A B", 0, &[])]).unwrap();
    assert_eq!(
        (pixel(&drawn.file, 15, 50), pixel(&drawn.file, 22, 58), pixel(&drawn.file, 22, 50), pixel(&drawn.file, 22, 54)),
        (RED, RED, RED, WHITE)
    );
    assert_eq!(drawn.words, "A B");

    // META_TEXTOUT, and lines that each start where the last one ended.
    let text_out = |text: &[u8; 1], at: (i16, i16)| {
        let mut body = words(&[1]);
        body.extend([text[0], 0]);
        body.extend(words(&[at.1, at.0]));
        (0x0521, body)
    };
    let drawn = draw(plain(), vec![text_out(b"A", (10, 40)), text_out(b"B", (60, 10))]).unwrap();
    assert_eq!((pixel(&drawn.file, 15, 50), pixel(&drawn.file, 65, 20), drawn.words.as_str()), (RED, RED, "A B"));
    let kept = vec![(0x0214, words(&[40, 10])), (0x012E, words(&[1])), text_out(b"A", (0, 0)), text_out(b"B", (0, 0))];
    let drawn = draw(plain(), kept).unwrap();
    assert_eq!((pixel(&drawn.file, 15, 50), pixel(&drawn.file, 25, 50), pixel(&drawn.file, 5, 10)), (RED, RED, WHITE));

    // The box of a line is filled, and the letters are cut to it.
    let boxed = vec![(0x0201, vec![0, 0, 255, 0]), ext_text((10, 40), b"AB", 2 | 4, &[])];
    let mut boxed_line = boxed.clone();
    boxed_line[1] = {
        let mut body = words(&[40, 10, 2, 2 | 4, 5, 35, 17, 65]);
        body.extend(b"AB");
        (0x0A32, body)
    };
    let drawn = draw(plain(), boxed_line).unwrap();
    assert_eq!(
        (pixel(&drawn.file, 7, 37), pixel(&drawn.file, 14, 50), pixel(&drawn.file, 18, 50), pixel(&drawn.file, 25, 50)),
        (BLUE, RED, WHITE, WHITE)
    );
    // A box with no text is only filled, and needs nobody who knows letters.
    let mut records = lettered(plain());
    records.push((0x0201, vec![0, 0, 255, 0]));
    records.push((0x0A32, words(&[0, 0, 0, 2, 20, 20, 80, 80])));
    let bmp = wmf_picture(&wmf(&records), Some((50, 50))).unwrap();
    assert_eq!((pixel(&bmp, 50, 50), pixel(&bmp, 10, 10)), (BLUE, WHITE));
}

#[test]
fn text_that_cannot_be_drawn_whole_is_not_drawn() {
    use crate::metafile::wmf_drawn;
    let face = boxes("AB ");
    let draw = |font: Option<(u16, Vec<u8>)>, more: Vec<(u16, Vec<u8>)>| {
        let mut records = lettered(font.clone().unwrap_or_else(|| (0x02FA, vec![0, 0, 1, 0, 0, 0, 0, 0, 0, 0])));
        records.extend(more);
        wmf_drawn(&wmf(&records), Some((50, 50)), Some(&face))
    };
    let plain = |set: u8| Some(wmf_font(-20, 0, [0, 0, 0, set]));
    assert!(draw(plain(0), vec![ext_text((10, 40), b"AB", 0, &[])]).is_some());
    // No font was chosen; a letter the font lacks; a font of symbols; a character set nobody
    // knows; a font of no height.
    assert_eq!(draw(None, vec![ext_text((10, 40), b"AB", 0, &[])]), None);
    assert_eq!(draw(plain(0), vec![ext_text((10, 40), b"AZ", 0, &[])]), None);
    assert_eq!(draw(plain(2), vec![ext_text((10, 40), b"AB", 0, &[])]), None);
    assert_eq!(draw(plain(77), vec![ext_text((10, 40), b"A\xE9", 0, &[])]), None);
    assert_eq!(draw(Some(wmf_font(0, 0, [0; 4])), vec![ext_text((10, 40), b"AB", 0, &[])]), None);
    // Letters that are joined or run right to left (Hebrew and Arabic, by their sets); a line
    // said to read right to left; numbers of glyphs in place of characters.
    assert_eq!(draw(plain(177), vec![ext_text((10, 40), b"\xE0\xE1", 0, &[])]), None);
    assert_eq!(draw(plain(178), vec![ext_text((10, 40), b"\xC7\xC8", 0, &[])]), None);
    assert_eq!(draw(plain(0), vec![(0x012E, words(&[256])), ext_text((10, 40), b"AB", 0, &[])]), None);
    assert_eq!(draw(plain(0), vec![ext_text((10, 40), b"AB", 0x10, &[])]), None);
    assert_eq!(draw(plain(0), vec![ext_text((10, 40), b"AB", 0x80, &[])]), None);
    // More text than the record holds.
    assert_eq!(draw(plain(0), vec![(0x0A32, words(&[40, 10, 300, 0, 0x4141]))]), None);
    assert_eq!(draw(plain(0), vec![(0x0521, words(&[300, 0x4141, 40, 10]))]), None);
}

/// EMR_EXTCREATEFONTINDIRECTW for object 2: a font this high, turned so many tenths of a
/// degree.
fn emf_font(height: i32, turn: i32) -> (u32, Vec<u8>) {
    let mut body = longs(&[2, height, 0, turn, 0, 400]);
    body.extend([0; 8]);
    let mut name: Vec<u8> = "Box".encode_utf16().flat_map(u16::to_le_bytes).collect();
    name.resize(64, 0);
    body.extend(name);
    (82, body)
}

/// EMR_EXTTEXTOUTW of `text` at a point, each character ten units from the next.
fn emf_text(at: (i32, i32), text: &str, options: i32) -> (u32, Vec<u8>) {
    let units: Vec<u16> = text.encode_utf16().collect();
    let count = i32::try_from(units.len()).unwrap();
    let after = 76 + 2 * count + (2 * count) % 4;
    let mut body = longs(&[0, 0, 0, 0, 1, 0, 0, at.0, at.1, count, 76, options, 5, 35, 17, 65, after]);
    body.extend(units.iter().flat_map(|unit| unit.to_le_bytes()));
    body.resize(usize::try_from(after).unwrap() - 8, 0);
    body.extend(longs(&vec![10; units.len()]));
    (84, body)
}

/// The font chosen, red letters and nothing filled in behind them.
fn emf_lettered(font: (u32, Vec<u8>)) -> Vec<(u32, Vec<u8>)> {
    vec![font, (37, longs(&[2])), (24, vec![255, 0, 0, 0]), (18, longs(&[1]))]
}

#[test]
fn text_in_an_enhanced_metafile_is_drawn() {
    use crate::metafile::{emf_drawn, emf_picture};
    let face = boxes("AB ");
    let draw = |font: (u32, Vec<u8>), more: Vec<(u32, Vec<u8>)>| {
        let mut records = emf_lettered(font);
        records.extend(more);
        emf_drawn(&emf_of(&records), Some((50, 50)), Some(&face))
    };
    let drawn = draw(emf_font(-20, 0), vec![emf_text((10, 40), "AB", 0)]).unwrap();
    assert_eq!((bmp_size(&drawn.file), drawn.words.as_str()), ((100, 100), "AB"));
    for (x, y, colour) in [(15, 50, RED), (25, 50, RED), (20, 50, WHITE), (15, 40, WHITE), (15, 58, WHITE)] {
        assert_eq!(pixel(&drawn.file, x, y), colour, "{x} {y}");
    }
    let mut records = emf_lettered(emf_font(-20, 0));
    records.push(emf_text((10, 40), "AB", 0));
    assert_eq!(emf_picture(&emf_of(&records), Some((50, 50))), None);

    // Text in bytes, in the font's character set.
    let mut bytes = longs(&[0, 0, 0, 0, 1, 0, 0, 10, 40, 2, 76, 0, 0, 0, 0, 0, 80]);
    bytes.extend(b"AB\0\0");
    bytes.extend(longs(&[30, 30]));
    let drawn = draw(emf_font(-20, 0), vec![(83, bytes)]).unwrap();
    assert_eq!((pixel(&drawn.file, 15, 50), pixel(&drawn.file, 45, 50), pixel(&drawn.file, 25, 50)), (RED, RED, WHITE));

    // The colour behind, the box filled and the letters cut to it.
    let drawn = draw(emf_font(-20, 0), vec![(25, vec![0, 0, 255, 0]), emf_text((10, 40), "AB", 2 | 4)]).unwrap();
    assert_eq!(
        (pixel(&drawn.file, 7, 37), pixel(&drawn.file, 14, 50), pixel(&drawn.file, 18, 50), pixel(&drawn.file, 25, 50)),
        (BLUE, RED, WHITE, WHITE)
    );
    let drawn = draw(emf_font(-20, 0), vec![(25, vec![0, 0, 255, 0]), (18, longs(&[2])), emf_text((10, 40), "AB", 0)]).unwrap();
    assert_eq!((pixel(&drawn.file, 20, 50), pixel(&drawn.file, 15, 50)), (BLUE, RED));

    // Set by its middle and the line it stands on; turned a quarter round.
    let drawn = draw(emf_font(-20, 0), vec![(22, longs(&[6 | 24])), emf_text((50, 50), "AB", 0)]).unwrap();
    assert_eq!((pixel(&drawn.file, 45, 45), pixel(&drawn.file, 55, 45), pixel(&drawn.file, 45, 52)), (RED, RED, WHITE));
    let drawn = draw(emf_font(-20, 900), vec![emf_text((50, 80), "A", 0)]).unwrap();
    assert_eq!((pixel(&drawn.file, 60, 75), pixel(&drawn.file, 45, 75)), (RED, WHITE));

    // The world made twice as large: letters twice the size, twice as far along.
    let twice: Vec<u8> = [2f32, 0.0, 0.0, 2.0, 0.0, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    let drawn = draw(emf_font(-20, 0), vec![(35, twice), emf_text((5, 10), "AB", 0)]).unwrap();
    assert_eq!(
        (pixel(&drawn.file, 20, 40), pixel(&drawn.file, 40, 40), pixel(&drawn.file, 30, 40), pixel(&drawn.file, 20, 22)),
        (RED, RED, WHITE, WHITE)
    );

    // Not drawn: a world that is turned; a letter the font lacks; right to left; numbers of
    // glyphs; text in a path; one of the device's own fonts; more text than the record holds.
    let turned: Vec<u8> = [0f32, 1.0, -1.0, 0.0, 0.0, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(draw(emf_font(-20, 0), vec![(35, turned), emf_text((10, 40), "AB", 0)]), None);
    assert_eq!(draw(emf_font(-20, 0), vec![emf_text((10, 40), "AZ", 0)]), None);
    assert_eq!(draw(emf_font(-20, 0), vec![emf_text((10, 40), "\u{5d0}\u{5d1}", 0)]), None);
    assert_eq!(draw(emf_font(-20, 0), vec![emf_text((10, 40), "AB", 0x10)]), None);
    assert_eq!(draw(emf_font(-20, 0), vec![(59, Vec::new()), emf_text((10, 40), "AB", 0)]), None);
    assert_eq!(draw(emf_font(-20, 0), vec![(37, 0x8000_000Du32.to_le_bytes().to_vec()), emf_text((10, 40), "AB", 0)]), None);
    let mut long = emf_text((10, 40), "AB", 0);
    long.1[36..40].copy_from_slice(&5000i32.to_le_bytes());
    assert_eq!(draw(emf_font(-20, 0), vec![long]), None);
}

#[test]
fn hostile_text_in_drawings_never_panics() {
    use crate::metafile::{emf_drawn, wmf_drawn};
    let face = boxes("AB ");
    let mut records = lettered(wmf_font(-20, 450, [1, 1, 1, 0]));
    records.extend([
        (0x0201, vec![0, 0, 255, 0]),
        (0x012E, words(&[6 | 24])),
        ext_text((50, 50), b"AB", 0, &[]),
        (0x0A32, {
            let mut body = words(&[40, 10, 3, 2 | 4, 5, 35, 17, 65]);
            body.extend(b"A B\0");
            body.extend(words(&[9, 9, 9]));
            body
        }),
        (0x0521, words(&[2, 0x4241, 40, 10])),
    ]);
    let file = wmf(&records);
    assert!(wmf_drawn(&file, Some((40, 40)), Some(&face)).is_some());
    for end in 0..file.len() {
        let _ = wmf_drawn(&file[..end], Some((24, 24)), Some(&face));
        for with in [file[end] ^ 0xFF, 0xFF, 0x80, 0] {
            let mut changed = file.clone();
            changed[end] = with;
            if let Some(drawn) = wmf_drawn(&changed, Some((24, 24)), Some(&face)) {
                let (w, h) = bmp_size(&drawn.file);
                assert!(w <= 800 && h <= 800 && drawn.words.chars().count() <= 2000);
            }
        }
    }
    let mut records = emf_lettered(emf_font(-20, 450));
    records.extend([(25, vec![0, 0, 255, 0]), (22, longs(&[6 | 24])), emf_text((50, 50), "A B", 2 | 4), emf_text((10, 40), "AB", 0)]);
    let file = emf_of(&records);
    assert!(emf_drawn(&file, Some((40, 40)), Some(&face)).is_some());
    for end in 0..file.len() {
        let _ = emf_drawn(&file[..end], Some((24, 24)), Some(&face));
        for with in [file[end] ^ 0xFF, 0xFF, 0x80, 0] {
            let mut changed = file.clone();
            changed[end] = with;
            if let Some(drawn) = emf_drawn(&changed, Some((24, 24)), Some(&face)) {
                let (w, h) = bmp_size(&drawn.file);
                assert!(w <= 800 && h <= 800 && drawn.words.chars().count() <= 2000);
            }
        }
    }
    // A great many lines of text say no more than is kept.
    let mut records = lettered(wmf_font(-20, 0, [0; 4]));
    records.extend((0..2000).map(|_| ext_text((10, 40), b"AB AB", 0, &[])));
    let drawn = wmf_drawn(&wmf(&records), Some((24, 24)), Some(&face)).unwrap();
    assert!(drawn.words.chars().count() <= 2000 && drawn.words.starts_with("AB AB AB"));
    // A font that is not one, and one cut short, give no letters.
    for bad in [&b"not a font"[..], &face.data[..face.data.len() / 2], &[]] {
        let broken = openmailclient_fonts::Face { data: bad.to_vec(), index: 0 };
        let mut records = lettered(wmf_font(-20, 0, [0; 4]));
        records.push(ext_text((10, 40), b"AB", 0, &[]));
        assert_eq!(wmf_drawn(&wmf(&records), Some((24, 24)), Some(&broken)), None);
    }
}

#[test]
fn rtf_shows_the_text_of_a_drawing_and_says_what_it_says() {
    let face = boxes("Bay14 ");
    let mut records = lettered(wmf_font(-20, 0, [0; 4]));
    records.push(ext_text((10, 40), b"Bay 14", 0, &[]));
    let rtf = format!("{{\\rtf1\\ansi\\pard a{{\\pict\\wmetafile8\\picwgoal750\\pichgoal750 {}}}b\\par}}", hexed(&wmf(&records)));
    let doc = crate::rtf::parse_with(rtf.as_bytes(), Some(&face));
    let run = paras(&doc)[0].runs.iter().find(|r| r.picture.is_some()).unwrap();
    assert_eq!((run.picture.clone(), run.text.as_str()), (Some(Picture::Inline(Some(0))), "Bay 14"));
    assert_eq!(pixel(&doc.written[0], 15, 50), RED);
    // With nobody who knows letters the picture keeps its place and says nothing.
    let doc = crate::rtf::parse(rtf.as_bytes());
    let run = paras(&doc)[0].runs.iter().find(|r| r.picture.is_some()).unwrap();
    assert_eq!((run.picture.clone(), run.text.as_str(), doc.written.len()), (Some(Picture::Inline(None)), "", 0));
}

#[test]
fn rtf_pictures_written_as_bytes_are_kept() {
    // Bytes that would end the group, start a control word or end the file if they were text.
    let mut png = vec![0x89u8, b'P', b'N', b'G', 13, 10, 26, 10];
    png.extend_from_slice(b"}{\\par \\bin9 \0\r\n}}");
    let body = |data: &[u8]| {
        let mut rtf = b"{\\rtf1\\ansi\\pard a{\\pict\\pngblip\\picwgoal1500\\pichgoal750".to_vec();
        rtf.extend_from_slice(data);
        rtf.extend_from_slice(b"}b\\par}");
        rtf
    };
    let bin = |bytes: &[u8]| [format!("\\bin{} ", bytes.len()).as_bytes(), bytes].concat();
    let pictures = |doc: &Doc| paras(doc)[0].runs.iter().filter_map(|r| r.picture.clone().map(|p| (p, r.extent))).collect::<Vec<_>>();

    // All of it as bytes; and part as hex digits, part as bytes, part as hex digits again.
    let (head, rest) = png.split_at(5);
    let (middle, tail) = rest.split_at(9);
    let mixed = [format!(" {}\r\n", hexed(head)).into_bytes(), bin(middle), hexed(tail).into_bytes()].concat();
    for data in [bin(&png), mixed] {
        let doc = crate::rtf::parse(&body(&data));
        assert_eq!(doc.text(), "ab");
        assert_eq!(pictures(&doc), [(Picture::Inline(Some(0)), (100, 50))]);
        assert_eq!(&*doc.written[0], &png[..]);
    }
    // No bytes at all are no picture, and take nothing after them.
    let doc = crate::rtf::parse(&body(b"\\bin0 "));
    assert_eq!((doc.text().as_str(), pictures(&doc)), ("ab", vec![(Picture::Inline(None), (100, 50))]));

    // Fewer bytes than promised, half a byte of hex before the bytes, a number that is no
    // count: a place, nothing kept, and nothing of the bytes read as text.
    let short = b"{\\rtf1 a{\\pict\\pngblip \\bin500 \x89PNG}b\\par}".to_vec();
    let split = body(&[b" 8".to_vec(), bin(&png)].concat());
    let negative = body(b"\\bin-4 ");
    let huge = body(b"\\bin2147483647 \x89PNG");
    for rtf in [short, split, negative, huge] {
        let doc = crate::rtf::parse(&rtf);
        assert!(doc.written.is_empty(), "{:?}", String::from_utf8_lossy(&rtf));
        assert!(!doc.text().contains("PNG"), "{:?}", doc.text());
        assert!(paras(&doc)[0].runs.iter().any(|r| r.picture == Some(Picture::Inline(None))));
    }

    // Bytes outside a picture are never text, and what follows them is read as before.
    let mut rtf = b"{\\rtf1\\ansi\\pard a\\bin4 }{\\b".to_vec();
    rtf.extend_from_slice(b" b{\\*\\unknown\\bin3 }}}c}\\par}");
    let doc = crate::rtf::parse(&rtf);
    assert_eq!(doc.text(), "a b");
    assert!(doc.written.is_empty());
    // The same goes for the readers that only want the words.
    assert_eq!(openmailclient_model::rtf::to_text(&rtf), "a b");

    // A bitmap written as bytes becomes a picture file like one written as hex digits.
    let dib = small_dib();
    let mut rtf = b"{\\rtf1 {\\pict\\dibitmap0".to_vec();
    rtf.extend_from_slice(&bin(&dib));
    rtf.extend_from_slice(b"}\\par}");
    let doc = crate::rtf::parse(&rtf);
    assert_eq!(doc.written.len(), 1);
    assert_eq!(&doc.written[0][..2], b"BM");
}

#[test]
fn rtf_cells_merged_across_are_one_cell() {
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi\
          \\trowd\\clmgf\\clbrdrl\\brdrs\\cellx1500\\clmrg\\cellx3000\\clmrg\\clbrdrr\\brdrs\\brdrw30\\cellx4500\\cellx6000\\intbl head\\cell\\cell\\cell x\\cell\\row\
          \\trowd\\cellx1500\\cellx3000\\cellx4500\\cellx6000\\intbl a\\cell b\\cell c\\cell d\\cell\\row\
          \\trowd\\clmrg\\cellx1500\\clvmgf\\clmgf\\cellx3000\\clmrg\\cellx4500\\cellx6000\\intbl e\\cell f\\cell\\cell g\\cell\\row\
          \\trowd\\cellx1500\\clvmrg\\clmgf\\cellx3000\\clmrg\\cellx4500\\cellx6000\\intbl h\\cell\\cell\\cell i\\cell\\row\\pard after\\par}",
    );
    let t = tables(&doc)[0];
    // Three cells are one that is three columns wide, with the line at the right of the last.
    let head = &t.rows[0][0];
    assert_eq!((head.span, head.width), (3, 300));
    assert_eq!(head.edges.map(|e| e.map(|e| e.width)), Some([0, 2, 0, 1]));
    assert_eq!(
        t.rows.iter().map(|r| r.iter().map(|c| c.span).collect::<Vec<_>>()).collect::<Vec<_>>(),
        [vec![3, 1], vec![1, 1, 1, 1], vec![1, 2, 1], vec![1, 1]]
    );
    assert_eq!(t.columns(), 4);
    // A cell that continues nothing (the first of a row) is a cell of its own.
    assert_eq!(paras_text(&t.rows[2][0]), "e");
    // A cell merged across and down is one cell over both.
    assert_eq!((t.rows[2][1].span, t.rows[2][1].rows), (2, 2));
    assert_eq!(t.places(), [vec![0, 3], vec![0, 1, 2, 3], vec![0, 1, 3], vec![0, 3]]);
    assert_eq!(doc.text(), "head\tx\na\tb\tc\td\ne\tf\tg\nh\ti\nafter");
}

fn paras_text(cell: &crate::rich::Cell) -> String {
    Doc { blocks: cell.blocks.clone(), ..Doc::default() }.text()
}

#[test]
fn rtf_tables_stand_in_the_cells_of_tables() {
    // As Word writes it (RTF 1.9.1, "Nested Tables"): the cells of the inner table end with
    // \nestcell, its row is defined after them, and a reader that knows none of this is given
    // a paragraph end in \nonesttables.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi{\\colortbl;\\red204\\green0\\blue0;}\
          \\trowd\\trgaph108\\clbrdrt\\brdrs\\cellx3000\\cellx6000\
          \\pard\\intbl before\\par\
          \\pard\\intbl\\itap2 one\\nestcell two\\nestcell\\nestcell\
          {\\*\\nesttableprops\\trowd\\trgaph30\\clcbpat1\\clbrdrb\\brdrs\\brdrw30\\cellx1000\\clmgf\\cellx2000\\clmrg\\cellx2500\\nestrow}{\\nonesttables\\par}\
          \\pard\\intbl\\itap2 three\\nestcell four\\nestcell\\nestcell\
          {\\*\\nesttableprops\\trowd\\cellx1000\\cellx2000\\cellx2500\\nestrow}{\\nonesttables\\par}\
          \\pard\\intbl after\\cell right\\cell\\row\
          \\pard below\\par}",
    );
    let outer = tables(&doc)[0];
    assert_eq!(outer.rows.len(), 1);
    assert_eq!(doc.text(), "before one\ttwo three\tfour\t after\tright\nbelow");
    // The row of the outer table keeps its own settings: the inner rows did not take them.
    let cell = &outer.rows[0][0];
    assert_eq!((cell.width, cell.padding, cell.edges.map(|e| e.map(|e| e.width))), (200, Some([1, 7, 1, 7]), Some([1, 0, 0, 0])));
    assert_eq!(outer.rows[0][1].width, 200);
    // What the cell holds, in the order it was written: a paragraph, the table, a paragraph.
    let kinds: Vec<&str> = cell.blocks.iter().map(|b| if matches!(b, Block::Table(_)) { "table" } else { "para" }).collect();
    assert_eq!(kinds, ["para", "table", "para"]);
    let Some(Block::Table(inner)) = cell.blocks.get(1) else { panic!("{cell:?}") };
    // The inner rows are as they were defined after their cells.
    assert_eq!(
        inner.rows.iter().map(|r| r.iter().map(|c| (c.span, c.width)).collect::<Vec<_>>()).collect::<Vec<_>>(),
        [vec![(1, 66), (2, 99)], vec![(1, 66), (1, 66), (1, 33)]]
    );
    let one = &inner.rows[0][0];
    assert_eq!((one.background, one.padding, one.edges.map(|e| e.map(|e| e.width))), (Some([204, 0, 0]), Some([1, 2, 1, 2]), Some([0, 0, 2, 0])));

    // Three deep, and a table that ends with its cell where nothing says so.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\trowd\\cellx3000\\cellx6000\\pard\\intbl\\itap3 deep\\nestcell deeper\\nestcell\
          {\\*\\nesttableprops\\trowd\\cellx500\\cellx900\\nestrow}\
          \\pard\\intbl\\itap2 mid\\nestcell dle\\nestcell{\\*\\nesttableprops\\trowd\\cellx1000\\cellx2000\\nestrow}\
          \\pard\\intbl\\itap2 open\\nestcell ended\\cell last\\cell\\row}",
    );
    let outer = tables(&doc)[0];
    let Some(Block::Table(mid)) = outer.rows[0][0].blocks.first() else { panic!("{outer:?}") };
    assert!(matches!(mid.rows[0][0].blocks.first(), Some(Block::Table(deep)) if deep.rows[0].len() == 2), "{mid:?}");
    assert_eq!(paras_text(&mid.rows[0][0]), "deep\tdeeper\nmid");
    assert_eq!(mid.rows.len(), 2, "the row that was never defined is still a row: {mid:?}");
    assert_eq!(doc.text(), "deep\tdeeper mid\tdle open ended\tlast");
}

#[test]
fn hostile_rtf_nested_and_merged_tables_never_panic() {
    let mut bodies: Vec<String> = vec![
        // Ends without their beginnings, a definition with nothing to define, levels out of any range.
        "{\\rtf1\\nestcell\\nestrow{\\*\\nesttableprops\\trowd\\cellx5\\nestrow}\\itap-5\\itap2147483647 a\\nestcell\\par}".into(),
        "{\\rtf1\\trowd\\clmrg\\cellx1\\clmrg\\cellx2\\intbl\\cell\\cell\\cell\\row\\clmrg\\clmgf\\nestrow\\row\\cell}".into(),
        // A definition that never closes, and one inside another.
        "{\\rtf1\\intbl\\itap2 a\\nestcell{\\*\\nesttableprops\\trowd\\cellx9{\\*\\nesttableprops\\trowd\\cellx1\\nestrow} b\\cell".into(),
        // The outer row is defined inside the definition of an inner one.
        "{\\rtf1{\\*\\nesttableprops\\trowd\\cellx100\\cellx200}\\intbl a\\cell b\\cell\\row}".into(),
    ];
    // More cells merged into one than a cell may span, and more cells waiting than a row has.
    let mut wide = String::from("{\\rtf1\\trowd\\clmgf\\cellx10");
    for i in 0..500 {
        wide.push_str(&format!("\\clmrg\\cellx{}", 20 + i));
    }
    wide.push_str("\\intbl a");
    wide.push_str(&"\\cell x".repeat(500));
    wide.push_str("\\row\\pard\\intbl\\itap2 ");
    wide.push_str(&"n\\nestcell ".repeat(5000));
    wide.push_str("{\\*\\nesttableprops\\trowd\\cellx1\\nestrow}\\cell\\row}");
    bodies.push(wide);
    // Deeper and deeper, then shallower, with nothing ever closed.
    let mut deep = String::from("{\\rtf1 ");
    for i in 0..3000 {
        deep.push_str(&format!("\\intbl\\itap{} t\\nestcell\\par ", i % 400));
    }
    bodies.push(deep);
    for body in &bodies {
        let doc = crate::rtf::parse(body.as_bytes());
        let _ = doc.text();
        fn check(blocks: &[Block], depth: usize) {
            assert!(depth <= 9, "tables stand no deeper than they may");
            for block in blocks {
                if let Block::Table(t) = block {
                    for cell in t.rows.iter().flatten() {
                        assert!((1..=64).contains(&cell.span));
                        check(&cell.blocks, depth + 1);
                    }
                    let _ = (t.columns(), t.places());
                }
            }
        }
        check(&doc.blocks, 0);
    }
    // The widest merged cell is as wide as a cell may be, and the cells after it are cells.
    let doc = crate::rtf::parse(bodies[4].as_bytes());
    assert_eq!(tables(&doc)[0].rows[0][0].span, 64);
}

#[test]
fn rtf_paragraphs_have_borders_and_room_above() {
    // RTF 1.9.1, "Paragraph Borders" and "Spacing".
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi{\\colortbl;\\red0\\green102\\blue153;}\
          \\pard plain\\par\
          \\pard\\sb240\\sa120\\box\\brdrdb\\brdrw30\\brdrcf1\\brsp80 one\\par two\\par\
          \\pard\\sb60\\brdrl\\brdrs\\brdrw45\\brdrb\\brdrdot left\\par\
          \\pard\\brdrl\\brdrs\\brdrw45\\brdrb\\brdrdot\\brdrt\\brdrnone again\\par\
          \\pard\\brdrt\\brdrs\\brdrbtw\\brdrs\\brdrw90 top\\par\
          \\pard\\sa30000 none\\par\\pard last\\par}",
    );
    let p = paras(&doc);
    assert_eq!(doc.text(), "plain\none\ntwo\nleft\nagain\ntop\nnone\nlast");
    assert_eq!(p[0].frame, None);
    // All four sides at once, and the room between the lines and the text.
    let boxed = p[1].frame.as_ref().unwrap();
    assert!(boxed.edges.iter().all(|e| (e.width, e.color, e.line) == (2, Some([0, 102, 153]), Line::Double)));
    assert_eq!(boxed.padding, [5; 4]);
    // Paragraphs that follow one another with the same border stand inside one.
    assert!(Arc::ptr_eq(boxed, p[2].frame.as_ref().unwrap()));
    // Sides of their own; where nothing is said of the room, the text still does not touch.
    let left = p[3].frame.as_ref().unwrap();
    assert_eq!(left.edges.map(|e| (e.width, e.line)), [(0, Line::Solid), (0, Line::Solid), (1, Line::Dotted), (3, Line::Solid)]);
    assert_eq!(left.padding, [1, 4, 1, 4]);
    // The same border written again is the same border: a side that is said to have no line
    // has none, and the line between paragraphs is not a side.
    assert!(Arc::ptr_eq(left, p[4].frame.as_ref().unwrap()));
    assert_eq!(p[5].frame.as_ref().unwrap().edges.map(|e| e.width), [1, 0, 0, 0]);
    assert_eq!(p[6].frame, None, "\\pard takes the border away");
    // Room above is what the paragraph asks for above it and the one before asked for below.
    assert_eq!(p.iter().map(|p| p.above).collect::<Vec<_>>(), [0, 16, 24, 12, 0, 0, 0, 96]);
    assert_eq!((p[1].lead(), p[0].lead()), (16, 0));

    // A cell's lines are not its paragraphs', and a paragraph's are not its cell's.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\trowd\\clbrdrt\\brdrs\\brdrw30\\cellx3000\\cellx6000\\pard\\intbl\\brdrb\\brdrs\\brdrw60 a\\cell\\pard\\intbl b\\cell\\row}",
    );
    let t = tables(&doc)[0];
    assert_eq!(t.rows[0][0].edges.map(|e| e.map(|e| e.width)), Some([2, 0, 0, 0]));
    let Some(Block::Para(a)) = t.rows[0][0].blocks.first() else { panic!("{t:?}") };
    assert_eq!(a.frame.as_ref().unwrap().edges.map(|e| e.width), [0, 0, 4, 0]);
    let Some(Block::Para(b)) = t.rows[0][1].blocks.first() else { panic!("{t:?}") };
    assert_eq!(b.frame, None);

    // A border stands where the paragraph's indent puts it, with the text just inside.
    let doc = crate::rtf::parse(b"{\\rtf1\\pard\\li1440\\brdrl\\brdrs in\\par\\pard\\li1440 bare\\par}");
    let p = paras(&doc);
    assert_eq!((p[0].indent, p[0].frame.as_ref().unwrap().indent), (0, 2));
    assert_eq!((p[1].indent, &p[1].frame), (2, &None));
}

#[test]
fn hostile_rtf_paragraph_borders_and_spacing_never_panic() {
    let mut body = String::from("{\\rtf1\\brdrw-5\\brdrcf99\\brdrs\\brsp-7\\sb-2147483648\\sa2147483647 a\\par");
    body.push_str("\\box\\brdrw2147483647\\brsp2147483647\\brdrcf-1 b\\par\\brdrnone\\brdrnone c\\par");
    for i in 0..3000 {
        body.push_str(&format!("\\pard\\brdrt\\brdrw{}\\sb{} p\\par", i * 7919, i * 104_729));
    }
    let doc = crate::rtf::parse(body.as_bytes());
    for p in paras(&doc) {
        assert!(p.above <= 96 && p.lead() <= 108);
        if let Some(frame) = &p.frame {
            assert!(frame.edges.iter().all(|e| e.width <= 12) && frame.padding.iter().all(|v| *v <= 64));
        }
    }
}

#[test]
fn rtf_list_paragraphs_have_their_numbers_and_bullets() {
    // As Word writes a list (RTF 1.9.1, "List Text"): the number worked out, then the text.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi{\\fonttbl{\\f0 Arial;}{\\f3\\fnil\\fcharset2 Symbol;}}\
          \\pard before\\par\
          \\pard{\\listtext\\pard\\plain\\f0 1.\\tab}\\ls1\\fi-360\\li720 first\\par\
          {\\listtext\\pard\\plain\\f0 2.\\tab}\\ls1\\fi-360\\li720 second {\\b bold}\\par\
          {\\listtext\\pard\\plain\\f3 \\'b7\\tab}\\ls2\\fi-360\\li1440 round\\par\
          {\\listtext\\pard\\plain o\\tab}\\ls2\\ilvl1\\fi-360\\li1440 hollow\\par\
          \\pard{\\pntext\\pard\\plain iv)\\tab}{\\*\\pn\\pnlvlbody\\pnlcrm{\\pntxta )}}\\fi-360 old\\par\
          {\\listtext 3.\\tab}\\par\
          {\\listtext \\tab}\\pard after\\par}",
    );
    let p = paras(&doc);
    let kinds: Vec<(Option<&str>, u8)> =
        p.iter().map(|p| (if let ParaKind::Item(m) = &p.kind { Some(m.as_str()) } else { None }, p.indent)).collect();
    assert_eq!(
        kinds,
        [
            (None, 0),
            (Some("1."), 1),
            (Some("2."), 1),
            (Some("\u{2022}"), 2),
            (Some("\u{25e6}"), 2),
            // A number with no indent still has room to stand in.
            (Some("iv)"), 1),
            // A number with nothing after it is still a paragraph of the list.
            (Some("3."), 1),
            // A marker that is only room is none.
            (None, 0)
        ]
    );
    // The number is not written twice, and the words of the paragraph keep their looks.
    assert_eq!(p[2].text(), "second bold");
    assert!(p[2].runs.last().is_some_and(|r| r.style.bold));
    assert_eq!(doc.text(), "before\n1. first\n2. second bold\n\u{2022} round\n\u{25e6} hollow\niv) old\n3. \u{a0}\nafter");
    // A reader that only wants the words still has the numbers the writer left.
    assert!(openmailclient_model::rtf::to_text(b"{\\rtf1{\\listtext 1.\\tab}first\\par}").starts_with("1.\tfirst"));

    // Hostile: a number without end, markers inside markers, one never closed, one in a cell.
    let mut body = String::from("{\\rtf1{\\listtext ");
    body.push_str(&"9".repeat(5000));
    body.push_str(
        "}x\\par{\\listtext{\\listtext{\\pntext a}b}c}y\\par\\trowd\\cellx900\\cellx1800\\intbl{\\listtext 1.}in\\cell two\\cell\\row{\\listtext z",
    );
    let doc = crate::rtf::parse(body.as_bytes());
    let p = paras(&doc);
    assert!(matches!(&p[0].kind, ParaKind::Item(m) if m.chars().count() == 12));
    assert!(matches!(&p[1].kind, ParaKind::Item(m) if m == "abc"), "{:?}", p[1].kind);
    let Some(Block::Para(cell)) = tables(&doc)[0].rows[0][0].blocks.first() else { panic!() };
    assert_eq!(cell.kind, ParaKind::Item("1.".into()));
}

#[test]
fn rtf_paragraphs_have_tab_stops() {
    use crate::rich::{Tab, TabKind, tab_room};
    // RTF 1.9.1, "Tabs": how a stop holds its text comes before where it is.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi\
          \\pard\\tx1440\\tqr\\tx4320\\li720\\tqdec\\tx6000\\tqc\\tx3000\\tb5000\\tx720\\tx300 a\\tab b\\tab c\\par \
          same\\par\
          \\pard none\\tab here\\par\
          \\pard\\tqr\\tx9000{\\tx100 in\\tab side\\par}out\\tab side\\par}",
    );
    let p = paras(&doc);
    let stop = |at, kind| Tab { at, kind, lead: Default::default() };
    // From where the text starts, the nearest first; a stop at or before the start is none,
    // and a bar tab is no stop.
    assert_eq!(p[0].tabs, [stop(48, TabKind::Left), stop(152, TabKind::Center), stop(240, TabKind::Right), stop(352, TabKind::Decimal)]);
    // A paragraph with no tab in it carries none; one with a tab and no stops has the usual ones.
    assert!(p[1].tabs.is_empty() && p[2].tabs.is_empty());
    assert_eq!(p[2].text(), "none\there");
    // Stops are the paragraph's: a group keeps its own and gives them back.
    assert_eq!(p[3].tabs, [stop(6, TabKind::Left), stop(600, TabKind::Right)]);
    assert_eq!(p[4].tabs, [stop(600, TabKind::Right)]);

    // The room a tab takes. With no stop ahead: to the next of the stops every paragraph has.
    assert_eq!(tab_room(&[], 1.0, 48.0, 10.0, 0.0, 0.0), 38.0);
    assert_eq!(tab_room(&[], 1.0, 48.0, 48.0, 0.0, 0.0), 48.0);
    let one = |kind| [stop(100, kind)];
    assert_eq!(tab_room(&one(TabKind::Left), 1.0, 48.0, 10.0, 30.0, 12.0), 90.0);
    assert_eq!(tab_room(&one(TabKind::Right), 1.0, 48.0, 10.0, 30.0, 12.0), 60.0);
    assert_eq!(tab_room(&one(TabKind::Center), 1.0, 48.0, 10.0, 30.0, 12.0), 75.0);
    assert_eq!(tab_room(&one(TabKind::Decimal), 1.0, 48.0, 10.0, 30.0, 12.0), 78.0);
    // Text too long to end at its stop starts where the tab is.
    assert_eq!(tab_room(&one(TabKind::Right), 1.0, 48.0, 10.0, 200.0, 0.0), 0.0);
    // Past the last stop, and in another measure (points, three to four pixels).
    assert_eq!(tab_room(&one(TabKind::Left), 1.0, 48.0, 120.0, 0.0, 0.0), 24.0);
    assert_eq!(tab_room(&one(TabKind::Left), 0.75, 36.0, 15.0, 0.0, 0.0), 60.0);
    // Numbers that are none: always some room, never a panic.
    for wild in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -5.0, f32::MAX] {
        for kind in [TabKind::Left, TabKind::Center, TabKind::Right, TabKind::Decimal] {
            for room in [
                tab_room(&one(kind), 1.0, wild, wild, wild, wild),
                tab_room(&one(kind), wild, 48.0, 10.0, wild, wild),
                tab_room(&[], 1.0, wild, 10.0, 0.0, 0.0),
            ] {
                assert!(room.is_finite() && room >= 0.0, "{wild} {kind:?} {room}");
            }
        }
    }

    // Hostile: more stops than a paragraph may have, and places that are none.
    let mut body = String::from("{\\rtf1\\pard\\tx-5\\tx2147483647\\tqr\\tx-2147483648\\li2147483647\\tx99999 ");
    for i in 0..2000 {
        body.push_str(&format!("\\tx{} ", i * 40));
    }
    body.push_str("a\\tab b\\par}");
    let doc = crate::rtf::parse(body.as_bytes());
    let tabs = &paras(&doc)[0].tabs;
    assert!(tabs.len() <= 32 && tabs.iter().all(|t| (1..=4096).contains(&t.at)) && tabs.windows(2).all(|w| w[0].at < w[1].at));
}

#[test]
fn paragraphs_say_how_tall_their_lines_are() {
    use crate::rich::Leading;
    // RTF 1.9.1, "Spacing": 240ths of a line with `\slmult1`, otherwise twips; a negative
    // height is all the line is, a positive one the least it is.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi\\pard\\sl360\\slmult1 a\\par b\\par \
          \\pard\\sl-300\\slmult0 c\\par \\pard\\sl300 d\\par \
          \\pard\\sl240\\slmult1 e\\par \\pard\\sl480\\slmult1{\\sl-15 f\\par}g\\par \
          \\pard h\\par \\pard\\sl2147483647\\slmult1 i\\par \\pard\\sl-2147483648 j\\par \
          \\pard\\sl-2147483648\\slmult1 k\\par \\pard\\sl1\\slmult1 l\\par \\pard\\sl2147483647 m\\par}",
    );
    let got: Vec<Leading> = paras(&doc).iter().map(|p| p.leading).collect();
    assert_eq!(
        got,
        [
            Leading::Times(150),
            Leading::Times(150),
            Leading::Exactly(20),
            Leading::AtLeast(20),
            Leading::Own,
            Leading::Times(50),
            Leading::Times(200),
            Leading::Own,
            Leading::Times(1000),
            Leading::Exactly(1024),
            Leading::Times(1000),
            Leading::Times(50),
            Leading::AtLeast(1024),
        ]
    );

    // CSS 2.1, 10.8.1: a number or a percentage of the size of the text, or a length.
    let html = |style: &str| paras(&parse(&format!("<p style=\"line-height:{style}\">a</p><p>b</p>"))).iter().map(|p| p.leading).collect::<Vec<_>>();
    assert_eq!(html("1.15"), [Leading::Times(100), Leading::Own]);
    assert_eq!(html("200%"), [Leading::Times(174), Leading::Own]);
    assert_eq!(html("2.3em"), [Leading::Times(200), Leading::Own]);
    assert_eq!(html("20px"), [Leading::Exactly(20), Leading::Own]);
    assert_eq!(html("15pt"), [Leading::Exactly(20), Leading::Own]);
    assert_eq!(html("normal"), [Leading::Own, Leading::Own]);
    for junk in ["", "-3", "0", "nan", "inf", "1e99", "99999999999999999999px", "0.0001", "3furlongs", "\u{e9}px", "9999999pt"] {
        let got = html(junk);
        assert!(matches!(got[0], Leading::Own | Leading::Times(50..=1000) | Leading::Exactly(1..=1024)), "{junk}: {got:?}");
    }
    // What a paragraph says holds for the paragraphs inside it, and stops with it.
    let doc = parse("<div style=\"line-height:30px\"><p>a</p><p style=\"line-height:normal\">b</p></div><p>c</p>");
    assert_eq!(paras(&doc).iter().map(|p| p.leading).collect::<Vec<_>>(), [Leading::Exactly(30), Leading::Own, Leading::Own]);

    // The height itself, in the measure of the line's own.
    assert_eq!(Leading::Own.tall(16.0, 1.0), 16.0);
    assert_eq!(Leading::Times(150).tall(16.0, 1.0), 24.0);
    assert_eq!(Leading::AtLeast(20).tall(16.0, 1.0), 20.0);
    assert_eq!(Leading::AtLeast(20).tall(30.0, 1.0), 30.0);
    assert_eq!(Leading::Exactly(20).tall(30.0, 0.75), 15.0);
    for wild in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -4.0, f32::MAX, 0.0] {
        for leading in [
            Leading::Own,
            Leading::Times(0),
            Leading::Times(u16::MAX),
            Leading::AtLeast(0),
            Leading::AtLeast(u16::MAX),
            Leading::Exactly(0),
            Leading::Exactly(u16::MAX),
        ] {
            for tall in [leading.tall(wild, 1.0), leading.tall(12.0, wild), leading.tall(wild, wild)] {
                assert!(tall.is_finite() && tall >= 0.0, "{wild} {leading:?} {tall}");
            }
        }
    }
}

#[test]
fn rtf_tabs_have_leaders_and_borders_a_line_between() {
    use crate::rich::{Leader, Tab, TabKind, tab_stop};
    // RTF 1.9.1, "Tabs": what fills the room comes before the stop, and is that stop's alone.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi\\pard\\tldot\\tx1440\\tqr\\tlul\\tx2880\\tx4320\\tlhyph\\tb5000\\tlmdot\\tx5760\\tlth\\tx7200\\tleq\\tx8640 \
          a\\tab b\\tab c\\par \\pard\\tx1440 d\\tab e\\par}",
    );
    let p = paras(&doc);
    let leads: Vec<Leader> = p[0].tabs.iter().map(|t| t.lead).collect();
    assert_eq!(leads, [Leader::Dots, Leader::Line, Leader::None, Leader::MiddleDots, Leader::ThickLine, Leader::Equals]);
    assert_eq!(p[0].tabs[1].kind, TabKind::Right);
    assert_eq!(p[1].tabs, [Tab { at: 96, kind: TabKind::Left, lead: Leader::None }]);

    // The stop a tab goes on to, in the measure it is asked in.
    let tabs = &p[0].tabs;
    assert_eq!(tab_stop(tabs, 1.0, 0.0).map(|(at, t)| (at, t.lead)), Some((96.0, Leader::Dots)));
    assert_eq!(tab_stop(tabs, 1.0, 96.0).map(|(at, t)| (at, t.lead)), Some((192.0, Leader::Line)));
    assert_eq!(tab_stop(tabs, 0.75, 71.9).map(|(at, _)| at), Some(144.0));
    assert!(tab_stop(tabs, 1.0, 5000.0).is_none() && tab_stop(&[], 1.0, 0.0).is_none());
    for wild in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, f32::MAX] {
        assert!(tab_stop(tabs, 1.0, wild).is_none(), "{wild}");
        assert!(tab_stop(tabs, wild, 0.0).is_none_or(|(at, _)| at.is_finite()), "{wild}");
    }

    // RTF 1.9.1, "Paragraph Borders": `\brdrbtw` is the line between paragraphs in one border.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi{\\colortbl;\\red200\\green0\\blue0;}\
          \\pard\\box\\brdrs\\brdrw15\\brdrbtw\\brdrdot\\brdrw30\\brdrcf1 a\\par b\\par \
          \\pard\\box\\brdrs\\brdrw15 c\\par \\pard\\brdrbtw\\brdrs\\brdrw2147483647 d\\par}",
    );
    let p = paras(&doc);
    let frame = p[0].frame.as_ref().unwrap();
    assert!(Arc::ptr_eq(frame, p[1].frame.as_ref().unwrap()));
    assert_eq!((frame.between.width, frame.between.line, frame.between.color), (2, Line::Dotted, Some([200, 0, 0])));
    assert!(frame.edges.iter().all(|e| e.width == 1 && e.line == Line::Solid && e.color.is_none()));
    // A border without one has none, and a line between is no border of its own.
    assert_eq!(p[2].frame.as_ref().unwrap().between.width, 0);
    assert!(!Arc::ptr_eq(frame, p[2].frame.as_ref().unwrap()));
    assert!(p[3].frame.is_none());
}

#[test]
fn rtf_list_numbers_are_counted_where_the_writer_left_none() {
    // RTF 1.9.1, "List Table" and "List Override Table": a paragraph names a list (`\ls`) and
    // a level (`\ilvl`); the level says how its number is written and where it starts.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi\
          {\\*\\listtable{\\list\\listtemplateid1\
          {\\listlevel\\levelnfc0\\levelstartat3{\\leveltext\\'02\\'00.;}{\\levelnumbers\\'01;}}\
          {\\listlevel\\levelnfc4\\levelstartat1{\\leveltext\\'03\\'00.\\'01;}{\\levelnumbers\\'01\\'03;}}\
          {\\listname Numbers;}\\listid7}\
          {\\list{\\listlevel\\levelnfc23{\\leveltext\\'01\\u-3913 ?;}{\\levelnumbers;}}\\listid9}\
          {\\list{\\listlevel\\levelnfc1}\\listid11}}\
          {\\*\\listoverridetable{\\listoverride\\listid7\\listoverridecount0\\ls1}\
          {\\listoverride\\listid9\\listoverridecount0\\ls2}{\\listoverride\\listid11\\ls3}}\
          \\pard\\ls1\\ilvl0 one\\par two\\par \
          \\pard\\ls1\\ilvl1 deep\\par deeper\\par \
          \\pard\\ls1\\ilvl0 three\\par \
          \\pard\\ls1\\ilvl1 again\\par \
          \\pard\\ls2 dot\\par \
          \\pard plain\\par \
          \\pard\\ls1{\\listtext\\pard\\plain X.\\tab}written\\par \
          \\pard\\ls1 after\\par \
          \\pard\\ls99 none\\par \
          \\pard\\ls3 roman\\par \\pard\\ls3\\par \\pard\\ls3 more\\par}",
    );
    let kinds: Vec<String> = paras(&doc)
        .iter()
        .map(|p| match &p.kind {
            ParaKind::Item(marker) => marker.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(kinds, ["3.", "4.", "4.a", "4.b", "5.", "5.a", "\u{2022}", "Body", "X.", "7.", "Body", "I.", "II.", "III."]);
    assert!(paras(&doc).iter().all(|p| !p.text().contains("Numbers")));
    assert!(paras(&doc).iter().filter(|p| matches!(p.kind, ParaKind::Item(_))).all(|p| p.indent >= 1));

    // Hostile: more lists and levels than are kept, numbers at the edge of what can be counted,
    // words that say they are longer than they are, levels and lists that are not there.
    let mut body = String::from("{\\rtf1\\ansi{\\*\\listtable");
    for i in 0..600 {
        body.push_str("{\\list");
        for _ in 0..20 {
            body.push_str("{\\listlevel\\levelnfc3\\levelstartat2147483647{\\leveltext\\'ff\\'00\\'08\\'07\\'01.;}}");
        }
        body.push_str(&format!("\\listid{i}}}"));
    }
    body.push_str(
        "{\\list\\listid-1}{\\list{\\listlevel\\levelnfc-2147483648\\levelstartat-2147483648{\\leveltext}}\\listid600}}{\\*\\listoverridetable",
    );
    for i in 0..3000 {
        body.push_str(&format!("{{\\listoverride\\listid{}\\ls{i}}}", i % 602 - 1));
    }
    body.push('}');
    for i in 0..3000 {
        body.push_str(&format!("\\pard\\ls{}\\ilvl{} a\\par ", i % 700, i % 13 - 2));
    }
    body.push_str("\\pard\\ls-2147483648\\ilvl2147483647 b\\par\\pard\\ls601 c\\par c\\par}");
    let doc = crate::rtf::parse(body.as_bytes());
    for p in paras(&doc) {
        if let ParaKind::Item(marker) = &p.kind {
            assert!(!marker.is_empty() && marker.chars().count() <= 12 && !marker.chars().any(char::is_control), "{marker:?}");
        }
    }
    assert!(paras(&doc).len() > 100);
}

#[test]
fn blocks_have_fills_and_room_on_each_side() {
    // CSS 2.1 section 14.2: a block's background is behind all of it. Section 8.4: padding.
    let doc = parse(
        "<div style=\"background:#036;padding:4px 8px 12px 16px;color:#fff\"><h2>Title</h2><p>Text <span style=\"background:#ff0\">marked</span></p>\
         <div style=\"background-color:#ffffff;padding-left:9pt\">card</div></div>\
         <p style=\"padding:6px 10px\">roomy</p>\
         <p bgcolor=\"#cc0000\" style=\"border:1px solid #000;padding-top:3px;padding-bottom:5px\">both</p>\
         <div style=\"background:#fefefe\">page</div>\
         <div style=\"border:1px solid #000\"><p style=\"padding:20px\">inside</p></div>\
         <blockquote style=\"background:#ddd;border-left:3px solid #999\">quoted</blockquote>\
         <div style=\"background:#036\"><table border=1><tr><td>cell</td><td>two</td></tr></table>after</div>\
         <p style=\"padding:1px 2px 3px 4px 5px\">five</p><p style=\"padding:0\">none</p><p style=\"padding:2em\">em</p>",
    );
    let p = paras(&doc);
    let frame = |n: usize| p[n].frame.as_ref().unwrap();
    assert_eq!(p[0].text(), "Title");
    assert_eq!((frame(0).fill, frame(0).padding), (Some([0, 51, 102]), [4, 8, 12, 16]));
    assert!(!crate::rich::any_edge(&frame(0).edges));
    // The paragraphs of one block stand in one box, and their text is not highlighted.
    assert!(Arc::ptr_eq(frame(0), frame(1)));
    assert!(p[0].runs.iter().all(|r| r.style.background.is_none() && r.style.color == Some([255, 255, 255])));
    assert_eq!(p[1].runs.iter().map(|r| r.style.background).collect::<Vec<_>>(), [None, Some([255, 255, 0])]);
    // Inside a filled block, white is a colour: the card is white, with its own room.
    assert_eq!((frame(2).fill, frame(2).padding), (Some([255, 255, 255]), [0, 0, 0, 12]));
    // Room alone is a box with no lines and no fill.
    assert_eq!((frame(3).fill, frame(3).padding, crate::rich::any_edge(&frame(3).edges)), (None, [6, 10, 6, 10], false));
    assert_eq!((frame(4).fill, frame(4).padding, frame(4).edges[0].width), (Some([204, 0, 0]), [3, 0, 5, 0], 1));
    // On the page itself, white is no fill.
    assert_eq!((p[5].text(), p[5].frame.is_none()), ("page".to_string(), true));
    // Room asked for inside a box that is there already is a box of its own inside that one.
    assert_eq!((frame(6).edges[0].width, frame(6).padding, frame(6).depth()), (0, [20; 4], 1));
    let outer = frame(6).within.as_ref().unwrap();
    assert_eq!((outer.edges[0].width, outer.padding, outer.within.is_none()), (1, [0; 4], true));
    // The card stands inside the block it was written in, and keeps its own fill only.
    assert!(Arc::ptr_eq(frame(2).within.as_ref().unwrap(), frame(0)));
    // A quotation's line at the left is its bar; its fill is a fill.
    assert_eq!((p[7].quote, frame(7).fill, crate::rich::any_edge(&frame(7).edges)), (1, Some([221, 221, 221]), false));
    // A table inside a filled block: its cells' paragraphs are not boxes of their own.
    let Some(Block::Table(table)) = doc.blocks.iter().find(|b| matches!(b, Block::Table(_))) else { panic!("no table") };
    assert!(matches!(&table.rows[0][0].blocks[0], Block::Para(q) if q.frame.is_none()));
    // The table as a whole stands in that block, with what follows it.
    assert!(Arc::ptr_eq(table.inside.as_ref().unwrap(), frame(8)));
    assert_eq!((p[8].text(), frame(8).fill), ("after".to_string(), Some([0, 51, 102])));
    // More lengths than four, none, and lengths that are not understood.
    assert!(p[9].frame.is_none() && p[10].frame.is_none() && p[11].frame.is_none());

    // RTF 1.9.1, "Paragraph Shading": `\cbpat` is the colour behind the paragraph.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi{\\colortbl;\\red255\\green242\\blue204;}\\pard\\cbpat1 shaded\\par same\\par \\pard plain\\par \
          \\pard\\cbpat9 none\\par \\pard\\box\\brdrs\\cbpat1 both\\par}",
    );
    let p = paras(&doc);
    assert_eq!(p[0].frame.as_ref().unwrap().fill, Some([255, 242, 204]));
    assert!(Arc::ptr_eq(p[0].frame.as_ref().unwrap(), p[1].frame.as_ref().unwrap()));
    assert!(p[2].frame.is_none() && p[3].frame.is_none());
    let both = p[4].frame.as_ref().unwrap();
    assert_eq!((both.fill, both.edges[0].width > 0), (Some([255, 242, 204]), true));

    // Hostile: thousands of filled blocks inside one another, and room of any size.
    let mut body = String::new();
    for i in 0..3000 {
        body.push_str(&format!("<div style=\"background:#{:06x};padding:{}px 99999999px -4px 1e39px;padding-left:{}pt\">x", i * 5000, i, u64::MAX));
    }
    let doc = parse(&body);
    for p in paras(&doc) {
        if let Some(frame) = &p.frame {
            assert!(frame.padding.iter().all(|v| *v <= 64));
        }
    }
}

#[test]
fn only_web_pages_and_mailboxes_are_links_to_follow() {
    use crate::links::{Target, target};
    let page = |address: &str| Ok(Target::Page(address.to_string()));
    assert_eq!(target("https://example.org/help?a=1&b=%20#top"), page("https://example.org/help?a=1&b=%20#top"));
    assert_eq!(target("HTTP://Example.org"), page("HTTP://Example.org"));
    // White space and control characters inside an address are dropped, as a browser would.
    assert_eq!(target(" https://exa\tmple.org/\r\n a\u{7}"), page("https://example.org/a"));
    assert_eq!(target("mailto:help@example.org?subject=Hi&body=x"), Ok(Target::Mailbox("help@example.org".to_string())));

    // Anything else would start a program, open a file, or go nowhere.
    for other in [
        "file:///C:/Windows/System32/calc.exe",
        "javascript:alert(1)",
        "data:text/html,<script>1</script>",
        "ms-msdt:/id PCWDiagnostic",
        "search-ms:query=x",
        "ftp://example.org/a",
        "tel:+15550100",
        "cid:part1@example.org",
        "\\\\server\\share\\a.exe",
        "C:\\Windows\\notepad.exe",
        "#top",
        "/relative/path",
        "example.org",
        "",
        "http:",
        "http://",
        "https:///path",
        "https://",
    ] {
        assert!(target(other).is_err(), "{other}");
    }
    // An address in characters an address does not have is not handed on: quotes, brackets,
    // backslashes, a pipe, letters outside the plain ones.
    for odd in [
        "https://example.org/\"&calc",
        "https://example.org/a b|c",
        "https://example.org/<x>",
        "https:\\\\example.org\\a",
        "https://example.org/^a`{}",
        "https://ex\u{430}mple.org/",
        "https://example.org/\u{202e}gpj.exe",
    ] {
        assert!(
            target(odd).is_err() || matches!(target(odd), Ok(Target::Page(p)) if p.is_ascii() && !p.contains(['"', '|', '<', '\\', ' '])),
            "{odd}"
        );
    }
    assert!(target("https://example.org/\"x").is_err() && target("https:\\\\example.org").is_err() && target("https://ex\u{430}mple.org/").is_err());
    // One plain mailbox, or none.
    for odd in
        ["mailto:", "mailto:a", "mailto:a@b", "mailto:a@b.org,c@d.org", "mailto:a%40b.org", "mailto:\"x\"@b.org", "mailto:a@b@c.org", "mailto:@b.org"]
    {
        assert!(target(odd).is_err(), "{odd}");
    }
    // Hostile: any length, any characters.
    let long = format!("https://example.org/{}", "a".repeat(100_000));
    assert!(target(&long).is_err());
    let wild: String = (0..5000u32).filter_map(|n| char::from_u32(n * 37 % 0x11_0000)).collect();
    for cut in [0, 1, 7, 64, 1000] {
        let _ = target(&wild.chars().skip(cut).collect::<String>());
        let _ = target(&format!("https://{}", wild.chars().skip(cut).collect::<String>()));
        let _ = target(&format!("mailto:{}", wild.chars().skip(cut).collect::<String>()));
    }
}

/// A bitmap without a file's header: so wide and high, of so many bits to the pixel, packed
/// this way, with so many colours said to be in its table; then what follows the header (masks,
/// the table) and the rows.
fn dib_of((w, h): (i32, i32), bits: u16, packed: u32, used: u32, after: &[u8], rows: &[u8]) -> Vec<u8> {
    let mut dib = Vec::new();
    dib.extend(40u32.to_le_bytes());
    dib.extend(w.to_le_bytes());
    dib.extend(h.to_le_bytes());
    dib.extend([1, 0]);
    dib.extend(bits.to_le_bytes());
    dib.extend(packed.to_le_bytes());
    dib.extend([0; 12]);
    dib.extend(used.to_le_bytes());
    dib.extend([0; 4]);
    dib.extend_from_slice(after);
    dib.extend_from_slice(rows);
    dib
}

/// META_STRETCHDIB of `dib`: the part taken (high, wide, row, column) and the box it goes to
/// (high, wide, top, left).
fn stretched(dib: &[u8], from: [i16; 4], to: [i16; 4]) -> (u16, Vec<u8>) {
    let mut body = vec![0x20, 0, 0xCC, 0, 0, 0];
    body.extend(words(&from));
    body.extend(words(&to));
    body.extend_from_slice(dib);
    (0x0F43, body)
}

/// EMR_STRETCHDIBITS of `dib`: where it goes and how large it is made, and the part taken.
fn emf_stretched(dib: &[u8], to: [i32; 4], from: [i32; 4]) -> (u32, Vec<u8>) {
    let pixels = i32::try_from(dib.len() - 40).unwrap();
    let mut body = longs(&[0, 0, 0, 0, to[0], to[1], from[0], from[1], from[2], from[3], 80, 40, 120, pixels, 0, 0x00CC_0020, to[2], to[3]]);
    body.extend_from_slice(dib);
    (81, body)
}

const GREEN: [u8; 3] = [0, 255, 0];

#[test]
fn a_bitmaps_pixels_are_read_in_every_plain_way_they_are_kept() {
    use crate::metafile::dib_pixels;
    let read = |dib: &[u8]| dib_pixels(dib, 1_000_000);
    let colours = |dib: &[u8]| read(dib).map(|(size, rgba)| (size, rgba.chunks(4).map(|p| [p[0], p[1], p[2]]).collect::<Vec<_>>()));
    // Three bytes to the pixel, the bottom row first: red and white over blue and green, and
    // every pixel covers all of its place.
    let ((w, h), rgba) = read(&small_dib()).unwrap();
    assert_eq!((w, h, rgba.len()), (2, 2, 16));
    assert!(rgba.chunks(4).all(|p| p[3] == 255));
    assert_eq!(colours(&small_dib()).unwrap().1, [RED, WHITE, BLUE, GREEN]);

    // Numbers in a table of colours, each kept blue first: eight bits, four, and one.
    let table = [10, 20, 30, 0, 1, 2, 3, 0];
    assert_eq!(colours(&dib_of((2, 1), 8, 0, 2, &table, &[1, 0, 0, 0])).unwrap(), ((2, 1), vec![[3, 2, 1], [30, 20, 10]]));
    // A number the table does not go up to is black.
    assert_eq!(colours(&dib_of((2, 1), 8, 0, 2, &table, &[5, 1, 0, 0])).unwrap().1, [[0; 3], [3, 2, 1]]);
    let greys: Vec<u8> = (0..16u8).flat_map(|n| [n, n, n, 0]).collect();
    assert_eq!(colours(&dib_of((3, 1), 4, 0, 0, &greys, &[0x12, 0x30, 0, 0])).unwrap().1, [[1; 3], [2; 3], [3; 3]]);
    let two = [0, 0, 0, 0, 255, 255, 255, 0];
    let nine = colours(&dib_of((9, 1), 1, 0, 0, &two, &[0x80, 0x80, 0, 0])).unwrap().1;
    assert_eq!((nine.len(), nine[0], nine[8]), (9, WHITE, WHITE));
    assert!(nine[1..8].iter().all(|p| *p == [0; 3]));

    // Sixteen bits: five to a colour, or the fields the bitmap names (five, six and five).
    assert_eq!(colours(&dib_of((1, 1), 16, 0, 0, &[], &[0x00, 0x7C, 0, 0])).unwrap().1, [RED]);
    let fields: Vec<u8> = [0xF800u32, 0x07E0, 0x001F].iter().flat_map(|m| m.to_le_bytes()).collect();
    assert_eq!(colours(&dib_of((2, 1), 16, 3, 0, &fields, &[0xE0, 0x07, 0x1F, 0x00])).unwrap().1, [GREEN, BLUE]);
    // Thirty-two bits, from the bottom up and (a height below nothing) from the top down.
    let rows = [1, 2, 3, 0, 4, 5, 6, 0];
    assert_eq!(colours(&dib_of((1, 2), 32, 0, 0, &[], &rows)).unwrap().1, [[6, 5, 4], [3, 2, 1]]);
    assert_eq!(colours(&dib_of((1, -2), 32, 0, 0, &[], &rows)).unwrap(), ((1, 2), vec![[3, 2, 1], [6, 5, 4]]));
    // The oldest header, of twelve bytes.
    assert_eq!(colours(&[12, 0, 0, 0, 1, 0, 1, 0, 1, 0, 24, 0, 9, 8, 7, 0]).unwrap(), ((1, 1), vec![[7, 8, 9]]));

    // Packed in runs: two pixels of the second colour.
    assert_eq!(colours(&dib_of((2, 1), 8, 1, 2, &table, &[2, 1, 0, 0])).unwrap().1, [[3, 2, 1]; 2]);
    // Not read: bits to the pixel nobody keeps, fields for a bitmap that has none, too large
    // a side, more pixels than may be had, no pixels, rows that are not there.
    assert_eq!(read(&dib_of((2, 1), 2, 0, 0, &table, &[0; 4])), None);
    assert_eq!(read(&dib_of((1, 1), 24, 3, 0, &fields, &[0; 4])), None);
    assert_eq!(read(&dib_of((5000, 1), 1, 0, 0, &two, &[0; 628])), None);
    assert_eq!(dib_pixels(&small_dib(), 3), None);
    assert!(dib_pixels(&small_dib(), 4).is_some());
    for size in [(0, 1), (1, 0), (-1, 1), (1, i32::MIN), (i32::MAX, i32::MAX)] {
        assert_eq!(read(&dib_of(size, 24, 0, 0, &[], &[0; 8])), None, "{size:?}");
    }
    assert_eq!(read(&dib_of((2, 2), 24, 0, 0, &[], &[0; 15])), None);
    assert_eq!(read(&dib_of((1, 1), 16, 3, 0, &fields[..8], &[])), None);
    // Fields of no bits, of all of them, and with holes in them.
    let odd: Vec<u8> = [0u32, u32::MAX, 0x0000_0505].iter().flat_map(|m| m.to_le_bytes()).collect();
    assert_eq!(colours(&dib_of((1, 1), 32, 3, 0, &odd, &[0xFF; 4])).unwrap().1, [[0, 255, 255]]);
}

#[test]
fn a_bitmap_beside_a_drawing_is_drawn_with_it() {
    use crate::metafile::{wmf_bitmap, wmf_picture};
    let dib = small_dib();
    let all = [2, 2, 0, 0];
    let at = |bmp: &[u8], points: [(usize, usize); 4]| points.map(|(x, y)| pixel(bmp, x, y));
    let quarters = [(20, 20), (40, 20), (20, 40), (40, 40)];
    // A red box, and then the bitmap over its corner: each of its four pixels fills a quarter
    // of the box it is laid over, and the red box shows beside it.
    let mut records = red_window();
    records.push((0x012D, words(&[0])));
    records.push((0x041B, words(&[90, 90, 30, 30])));
    records.push(stretched(&dib, all, [40, 40, 10, 10]));
    let file = wmf(&records);
    assert_eq!(wmf_bitmap(&file), None);
    let bmp = wmf_picture(&file, Some((50, 50))).unwrap();
    assert_eq!(bmp_size(&bmp), (100, 100));
    assert_eq!(at(&bmp, quarters), [RED, WHITE, BLUE, GREEN]);
    assert_eq!((pixel(&bmp, 70, 70), pixel(&bmp, 5, 5), pixel(&bmp, 55, 20)), (RED, WHITE, WHITE));
    // What is drawn after it lies over it.
    records.push((0x041B, words(&[45, 45, 35, 35])));
    assert_eq!(pixel(&wmf_picture(&wmf(&records), Some((50, 50))).unwrap(), 40, 40), RED);

    // Two bitmaps and nothing else, written the other two ways: stretched without the colours'
    // way, and copied as large as it is (two units, which are two pixels here).
    let mut second = vec![0x20, 0, 0xCC, 0];
    second.extend(words(&[2, 2, 0, 0, 40, 40, 10, 10]));
    second.extend_from_slice(&dib);
    let mut third = vec![0x20, 0, 0xCC, 0];
    third.extend(words(&[0, 0, 2, 2, 60, 60]));
    third.extend_from_slice(&dib);
    let window = vec![(0x020B, words(&[0, 0])), (0x020C, words(&[100, 100]))];
    let both = |second: Vec<u8>, third: Vec<u8>| {
        let mut records = window.clone();
        records.extend([(0x0B41, second), (0x0940, third)]);
        wmf_picture(&wmf(&records), Some((50, 50)))
    };
    let bmp = both(second.clone(), third.clone()).unwrap();
    assert_eq!(at(&bmp, quarters), [RED, WHITE, BLUE, GREEN]);
    assert_eq!(at(&bmp, [(60, 60), (61, 60), (60, 61), (61, 61)]), [RED, WHITE, BLUE, GREEN]);
    assert_eq!((pixel(&bmp, 62, 62), pixel(&bmp, 59, 59)), (WHITE, WHITE));

    // It stays inside what the drawing is cut to (a line of no length makes it a drawing: a
    // metafile of one bitmap and nothing else is that bitmap, as it is).
    let mut records = window.clone();
    records.extend([(0x0213, words(&[0, 0])), (0x0416, words(&[50, 100, 0, 0])), stretched(&dib, all, [100, 100, 0, 0])]);
    let bmp = wmf_picture(&wmf(&records), Some((50, 50))).unwrap();
    assert_eq!(at(&bmp, [(25, 25), (75, 25), (25, 75), (75, 75)]), [RED, WHITE, WHITE, WHITE]);

    // A part of the bitmap, its rows counted from the bottom as the bitmap keeps them: the
    // bottom row, the top one, the right-hand column. A part alone is a drawing too.
    let one = |record: (u16, Vec<u8>)| {
        let mut records = window.clone();
        records.extend([(0x0213, words(&[0, 0])), record]);
        wmf_picture(&wmf(&records), Some((50, 50)))
    };
    let to = [40, 40, 10, 10];
    let mut alone = window.clone();
    alone.push(stretched(&dib, [1, 2, 0, 0], to));
    assert_eq!(at(&wmf_picture(&wmf(&alone), Some((50, 50))).unwrap(), quarters), [BLUE, GREEN, BLUE, GREEN]);
    assert_eq!(at(&one(stretched(&dib, [1, 2, 0, 0], to)).unwrap(), quarters), [BLUE, GREEN, BLUE, GREEN]);
    assert_eq!(at(&one(stretched(&dib, [1, 2, 1, 0], to)).unwrap(), quarters), [RED, WHITE, RED, WHITE]);
    assert_eq!(at(&one(stretched(&dib, [2, 1, 0, 1], to)).unwrap(), quarters), [WHITE, WHITE, GREEN, GREEN]);
    // A width or a height below nothing mirrors it.
    assert_eq!(at(&one(stretched(&dib, all, [40, -40, 10, 50])).unwrap(), quarters), [WHITE, RED, GREEN, BLUE]);
    assert_eq!(at(&one(stretched(&dib, all, [-40, 40, 50, 10])).unwrap(), quarters), [BLUE, GREEN, RED, WHITE]);
    // Pixels packed in runs; a part that starts one column in is the rest of the row.
    let runs = dib_of((2, 1), 8, 1, 2, &[0, 0, 0, 0, 0, 255, 0, 0], &[2, 1, 0, 0]);
    assert_eq!(at(&one(stretched(&runs, [1, 2, 0, 0], to)).unwrap(), quarters), [GREEN; 4]);
    let mut moved = third.clone();
    moved[6] = 1;
    let bmp = both(second.clone(), moved).unwrap();
    assert_eq!(at(&bmp, [(60, 60), (61, 60), (60, 61), (61, 61)]), [WHITE, WHITE, GREEN, WHITE]);

    // Not drawn, and so no picture: colours that are numbers in a palette, a bitmap that is
    // cut short, and no window to draw in.
    let mut numbered = stretched(&dib, all, to);
    numbered.1[4] = 1;
    assert_eq!(one(numbered), None);
    assert_eq!(one(stretched(&dib[..dib.len() - 2], all, to)), None);
    assert_eq!(wmf_picture(&wmf(&[stretched(&dib, [1, 2, 0, 0], to), stretched(&dib, all, to)]), Some((50, 50))), None);
}

#[test]
fn a_bitmap_beside_an_enhanced_drawing_is_drawn_with_it() {
    use crate::metafile::{emf_bitmap, emf_picture};
    let dib = small_dib();
    let all = [0, 0, 2, 2];
    let draw = |more: Vec<(u32, Vec<u8>)>| {
        let mut records = red_brush();
        records.extend(more);
        emf_picture(&emf_of(&records), Some((50, 50)))
    };
    let at = |bmp: &[u8], points: [(usize, usize); 4]| points.map(|(x, y)| pixel(bmp, x, y));
    let quarters = [(20, 20), (40, 20), (20, 40), (40, 40)];
    let boxed = (43, longs(&[30, 30, 90, 90]));
    let placed = emf_stretched(&dib, [10, 10, 40, 40], all);
    assert_eq!(emf_bitmap(&emf_of(&[boxed.clone(), placed.clone()])), None);
    let bmp = draw(vec![boxed.clone(), placed.clone()]).unwrap();
    assert_eq!(bmp_size(&bmp), (100, 100));
    assert_eq!(at(&bmp, quarters), [RED, WHITE, BLUE, GREEN]);
    assert_eq!((pixel(&bmp, 70, 70), pixel(&bmp, 5, 5), pixel(&bmp, 55, 20)), (RED, WHITE, WHITE));
    // Two bitmaps and nothing else; what is drawn after one lies over it.
    let bmp = draw(vec![placed.clone(), emf_stretched(&dib, [60, 60, 20, 20], all)]).unwrap();
    assert_eq!(at(&bmp, quarters), [RED, WHITE, BLUE, GREEN]);
    assert_eq!(at(&bmp, [(65, 65), (75, 65), (65, 75), (75, 75)]), [RED, WHITE, BLUE, GREEN]);
    assert_eq!(pixel(&draw(vec![placed.clone(), (43, longs(&[35, 35, 45, 45]))]).unwrap(), 40, 40), RED);

    // The world made half as large and moved to the middle: so is the bitmap. And it stays
    // inside the box the drawing is cut to.
    let turn: Vec<u8> = [0.5f32, 0.0, 0.0, 0.5, 50.0, 50.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    let bmp = draw(vec![(54, longs(&[0, 0])), (35, turn), emf_stretched(&dib, [0, 0, 100, 100], all)]).unwrap();
    assert_eq!(at(&bmp, [(60, 60), (90, 60), (60, 90), (90, 90)]), [RED, WHITE, BLUE, GREEN]);
    assert_eq!(pixel(&bmp, 40, 40), WHITE);
    let bmp = draw(vec![(54, longs(&[0, 0])), (30, longs(&[0, 0, 100, 50])), emf_stretched(&dib, [0, 0, 100, 100], all)]).unwrap();
    assert_eq!(at(&bmp, [(25, 25), (75, 25), (25, 75), (75, 75)]), [RED, WHITE, WHITE, WHITE]);

    // A world that is turned a quarter round: so is the bitmap, its top now at the right.
    let turned: Vec<u8> = [0.0f32, 1.0, -1.0, 0.0, 100.0, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    let bmp = draw(vec![(54, longs(&[0, 0])), (35, turned), placed.clone()]).unwrap();
    assert_eq!(at(&bmp, [(80, 20), (80, 40), (60, 20), (60, 40)]), [RED, WHITE, BLUE, GREEN]);
    assert_eq!(pixel(&bmp, 20, 20), WHITE);
    // A part of the bitmap (the bottom row, counted from the bottom; the right-hand column),
    // one mirrored, and one whose pixels are packed in runs. A part alone is a drawing too.
    let to = [10, 10, 40, 40];
    assert_eq!(at(&draw(vec![emf_stretched(&dib, to, [0, 0, 2, 1])]).unwrap(), quarters), [BLUE, GREEN, BLUE, GREEN]);
    assert_eq!(at(&draw(vec![emf_stretched(&dib, to, [0, 1, 2, 1])]).unwrap(), quarters), [RED, WHITE, RED, WHITE]);
    assert_eq!(at(&draw(vec![emf_stretched(&dib, to, [1, 0, 1, 2])]).unwrap(), quarters), [WHITE, WHITE, GREEN, GREEN]);
    assert_eq!(at(&draw(vec![emf_stretched(&dib, [50, 10, -40, 40], all)]).unwrap(), quarters), [WHITE, RED, GREEN, BLUE]);
    let runs = dib_of((2, 1), 8, 1, 2, &[0, 0, 0, 0, 0, 255, 0, 0], &[2, 1, 0, 0]);
    assert_eq!(at(&draw(vec![(54, longs(&[0, 0])), emf_stretched(&runs, to, [0, 0, 2, 1])]).unwrap(), quarters), [GREEN; 4]);

    // Not drawn, and so no picture: colours that are numbers in a palette, and parts that
    // are not in the record.
    let change = |at: usize, to: i32| {
        let mut record = placed.clone();
        record.1[at - 8..at - 4].copy_from_slice(&to.to_le_bytes());
        draw(vec![boxed.clone(), record])
    };
    assert!(change(64, 0).is_some());
    assert_eq!(change(64, 1), None);
    assert_eq!(change(48, 4000), None);
    assert_eq!(change(60, 4000), None);
    // A copy that is not a plain one: each pixel is what was there or the bitmap's, not both.
    let bmp = change(68, 0x0066_0046).unwrap();
    assert_eq!(at(&bmp, quarters), [[0, 255, 255], [0; 3], [255, 255, 0], [255, 255, 0]]);
}

#[test]
fn hostile_bitmaps_in_drawings_never_panic() {
    use crate::metafile::{dib_pixels, emf_picture, wmf_picture};
    let dib = small_dib();
    let fields: Vec<u8> = [0xF800u32, 0x07E0, 0x001F].iter().flat_map(|m| m.to_le_bytes()).collect();
    let kinds = [
        dib.clone(),
        dib_of((9, 2), 1, 0, 0, &[0, 0, 0, 0, 255, 255, 255, 0], &[0xAA; 8]),
        dib_of((3, 2), 4, 0, 3, &[9; 12], &[0x12; 8]),
        dib_of((2, 2), 8, 0, 2, &[7; 8], &[1; 8]),
        dib_of((2, -2), 16, 3, 0, &fields, &[0x5A; 8]),
        dib_of((1, 2), 32, 0, 0, &[], &[0x33; 8]),
    ];
    for dib in &kinds {
        assert!(dib_pixels(dib, 1000).is_some());
        for end in 0..dib.len() {
            let _ = dib_pixels(&dib[..end], 1000);
            for with in [dib[end] ^ 0xFF, 0xFF, 0x80, 0] {
                let mut changed = dib.clone();
                changed[end] = with;
                if let Some(((w, h), rgba)) = dib_pixels(&changed, 1000) {
                    assert!(w * h <= 1000 && rgba.len() == w * h * 4);
                }
            }
        }
    }
    // In a drawing of each kind: cut short anywhere, and with any one byte turned over or made
    // the largest it can be.
    let mut records = red_window();
    records.extend([(0x041B, words(&[90, 90, 30, 30])), stretched(&dib, [2, 2, 0, 0], [40, 40, 10, 10])]);
    let old = wmf(&records);
    let new = emf_of(&[(43, longs(&[30, 30, 90, 90])), emf_stretched(&dib, [10, 10, 40, 40], [0, 0, 2, 2])]);
    assert!(wmf_picture(&old, Some((24, 24))).is_some() && emf_picture(&new, Some((24, 24))).is_some());
    for (file, read) in [(&old, wmf_picture as fn(&[u8], Option<(u32, u32)>) -> Option<Vec<u8>>), (&new, emf_picture)] {
        for end in 0..file.len() {
            let _ = read(&file[..end], Some((24, 24)));
            for with in [file[end] ^ 0xFF, 0xFF, 0x80, 0] {
                let mut changed = file.clone();
                changed[end] = with;
                for bmp in [read(&changed, Some((24, 24))), read(&changed, None)].into_iter().flatten() {
                    let (w, h) = bmp_size(&bmp);
                    assert!(w <= 800 && h <= 800);
                }
            }
        }
    }
    // A drawing of a great many large bitmaps is given up, and soon: the pixels of all its
    // bitmaps are counted together.
    let large = dib_of((2048, 2048), 1, 0, 0, &[0, 0, 0, 0, 255, 255, 255, 0], &vec![0x55; 256 * 2048]);
    let mut records = vec![(0x020Bu16, words(&[0, 0])), (0x020C, words(&[100, 100]))];
    records.extend((0..6).map(|_| stretched(&large, [2048, 2048, 0, 0], [100, 100, 0, 0])));
    let started = std::time::Instant::now();
    assert_eq!(wmf_picture(&wmf(&records), Some((40, 40))), None);
    let many: Vec<_> = (0..6).map(|_| emf_stretched(&large, [0, 0, 100, 100], [0, 0, 2048, 2048])).collect();
    assert_eq!(emf_picture(&emf_of(&many), Some((40, 40))), None);
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
    // One of them alone is drawn.
    assert!(wmf_picture(&wmf(&records[..3]), Some((40, 40))).is_some());
}

/// Which way each paragraph of a document is written, and the side it stands at.
fn ways(doc: &Doc) -> Vec<(bool, crate::rich::Align)> {
    doc.blocks.iter().filter_map(|b| if let Block::Para(p) = b { Some((p.rtl, p.align)) } else { None }).collect()
}

#[test]
fn a_paragraph_says_which_way_it_is_written() {
    use crate::rich::Align::{Center, Left, Right};
    // HTML: `dir`, and `direction` in a style; a right-to-left paragraph starts at the right
    // unless it is told a side, and `start` and `end` are sides of the way it is written.
    let doc = crate::html::parse(concat!(
        "<p>a</p><p dir=rtl>b</p><p dir=RTL align=left>c</p><p style=\"direction:rtl\">d</p>",
        "<p dir=rtl style=\"text-align:end\">e</p><p dir=rtl style=\"text-align:start\">f</p>",
        "<p style=\"text-align:end\">g</p><p dir=rtl align=center>h</p><p dir=rtl style=\"direction:ltr\">i</p>",
        "<div dir=rtl><p>j</p><p dir=ltr>k</p></div>",
    ));
    assert_eq!(
        ways(&doc),
        [
            (false, Left),
            (true, Right),
            (true, Left),
            (true, Right),
            (true, Left),
            (true, Right),
            (false, Right),
            (true, Center),
            (false, Left),
            (true, Right),
            (false, Left)
        ]
    );
    // `dir=auto`: by the first letter that has a direction; digits and signs have none.
    let doc = crate::html::parse("<p dir=auto>12 (\u{5d0}) b</p><p dir=auto>12 b \u{5d0}</p><p dir=auto>12</p>");
    assert_eq!(ways(&doc), [(true, Right), (false, Left), (false, Left)]);
    // RTF: `\rtlpar` and `\ltrpar`, until `\pard`; `\ql` keeps a right-to-left paragraph left.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi \\pard\\rtlpar one\\par two\\par\\ql three\\par\\pard four\\par\\rtlpar\\ltrpar five\\par\\pard\\rtlpar\\qc six\\par}",
    );
    assert_eq!(ways(&doc), [(true, Right), (true, Right), (true, Left), (false, Left), (false, Left), (true, Center)]);
    // Plain text says nothing: each paragraph goes by its own first letter.
    let doc = Doc::plain("hello \u{5d0}\n\u{627}\u{644} hello\n123");
    assert_eq!(ways(&doc), [(false, Left), (true, Right), (false, Left)]);
}

#[test]
fn hostile_directions_never_panic() {
    let deep = "<div dir=rtl>".repeat(5_000) + "\u{5d0}";
    let long = format!("<p dir=\"{}\">x</p><p style=\"direction:{}\">y</p>", "rtl".repeat(100_000), "\u{202e}".repeat(50_000));
    for html in [
        deep.as_str(),
        long.as_str(),
        "<p dir>",
        "<p dir=>a",
        "<p dir=auto>",
        "<p dir=auto>\u{202e}\u{202e}",
        "<td dir=rtl>x",
        "<p style=\"direction\">a<p style=\"direction:;text-align:start;text-align:end\">b",
    ] {
        let doc = crate::html::parse(html);
        assert!(ways(&doc).len() <= 5_001);
    }
    let many = [b"{\\rtf1 ".as_slice(), &b"\\rtlpar\\ltrpar".repeat(50_000), b" x\\par}"].concat();
    assert_eq!(crate::rtf::parse(&many).text().trim(), "x");
    for rtf in [b"{\\rtf1\\rtlpar".as_slice(), b"{\\rtf1\\rtlpar-99999999999 a}", b"{\\rtf1\\ltrpar\\qr\\rtlpar}"] {
        let _ = crate::rtf::parse(rtf);
    }
    let plain = "\u{5d0}\n".repeat(50_000) + &"\u{202e}".repeat(300_000);
    assert!(!Doc::plain(&plain).blocks.is_empty());
}

#[test]
fn an_element_that_says_which_way_its_text_runs_says_so_in_the_text() {
    let texts = |html: &str| -> Vec<String> { paras(&parse(html)).iter().map(|p| p.text()).collect() };
    // Every letter the way that is said, whatever it is.
    assert_eq!(texts("<p>abc <bdo dir=rtl>def</bdo> ghi</p>"), ["abc \u{202e}def\u{202c} ghi"]);
    assert_eq!(texts("<p>\u{5d0} <BDO DIR=LTR>\u{5d1}\u{5d2}</BDO></p>"), ["\u{5d0} \u{202d}\u{5d1}\u{5d2}\u{202c}"]);
    // With no direction of its own it has the one it stands in.
    assert_eq!(texts("<p dir=rtl><bdo>ab</bdo></p><p><bdo>ab</bdo></p>"), ["\u{202e}ab\u{202c}", "\u{202d}ab\u{202c}"]);
    // A name that may be in any writing is kept apart from what is around it.
    assert_eq!(texts("<p><bdi>\u{5d0}</bdi>: 3</p>"), ["\u{2068}\u{5d0}\u{2069}: 3"]);
    assert_eq!(
        texts("<p>a <span dir=rtl>b</span> <span dir=ltr>c</span> <span dir=auto>d</span> <span dir=up>e</span></p>"),
        ["a \u{2067}b\u{2069} \u{2066}c\u{2069} \u{2068}d\u{2069} e"]
    );
    assert_eq!(
        texts(concat!(
            "<p>a <span style=\"unicode-bidi:embed;direction:rtl\">b</span> <span style=\"unicode-bidi: Bidi-Override\">c</span>",
            " <i style=\"unicode-bidi:isolate;direction:rtl\">d</i> <u style=\"unicode-bidi:plaintext\">e</u>",
            " <b style=\"unicode-bidi:isolate-override;direction:rtl\">f</b></p>"
        )),
        ["a \u{202b}b\u{202c} \u{202d}c\u{202c} \u{2067}d\u{2069} \u{2068}e\u{2069} \u{202e}f\u{202c}"]
    );
    // A direction alone turns nothing inside a line, and `normal` takes back what an element is.
    assert_eq!(texts("<p>a <span style=\"direction:rtl\">b</span> <bdo dir=rtl style=\"unicode-bidi:normal\">c</bdo></p>"), ["a b c"]);
    // A paragraph's direction is its own: no marks. Told to run one way, all of it does.
    assert_eq!(texts("<p dir=rtl>b</p><div dir=auto>c</div>"), ["b", "c"]);
    assert_eq!(
        texts("<div style=\"unicode-bidi:bidi-override;direction:rtl\"><p>one</p><p>two</p></div><p>three</p>"),
        ["\u{202e}one", "\u{202e}two", "three"]
    );
    // It is said again in every paragraph the element has text in, and ended where it ends.
    assert_eq!(texts("<span dir=rtl>one<p>two</p>three</span> four"), ["\u{2067}one", "\u{2067}two", "\u{2067}three\u{2069} four"]);
    // One inside another, and a picture first.
    assert_eq!(texts("<p><bdo dir=rtl>a<bdi>b</bdi></bdo>c</p>"), ["\u{202e}a\u{2068}b\u{2069}\u{202c}c"]);
    let pictured = parse("<p>a<bdo dir=rtl><img src=\"cid:x\" alt=\"pic\">b</bdo></p>");
    let p = paras(&pictured);
    assert_eq!(
        p[0].runs.iter().map(|r| (r.text.as_str(), r.picture.is_some())).collect::<Vec<_>>(),
        [("a", false), ("\u{202e}", false), ("pic", true), ("b\u{202c}", false)]
    );
    // Nothing is written for an element with no text, or with text that is not shown.
    assert_eq!(texts("<p>a<bdo dir=rtl></bdo>b<bdo dir=rtl style=\"display:none\">x</bdo>c</p><bdo dir=rtl> </bdo>"), ["abc"]);
    // A cell is its own: what is around the table does not reach into it.
    let table = parse("<bdo dir=rtl>a<table border=1><tr><td>x<bdi>y</bdi></td></tr></table></bdo>");
    let cell = table.blocks.iter().find_map(|b| if let Block::Table(t) = b { t.rows.first()?.first() } else { None }).unwrap();
    let Some(Block::Para(inside)) = cell.blocks.first() else { panic!("a cell with a paragraph") };
    assert_eq!(inside.text(), "x\u{2068}y\u{2069}");
    // Hostile: far too deep, never closed, closed twice.
    let deep = parse(&("<bdo dir=rtl><bdi>".repeat(5000) + "x" + &"</bdi></bdo>".repeat(6000) + "y"));
    assert!(deep.text().contains('x') && deep.text().contains('y'));
    assert!(deep.text().chars().count() < 2000);
    parse(&"<bdo dir=rtl><p>a".repeat(3000));
    parse("<bdo dir><bdi dir=><span dir=auto style=\"unicode-bidi:;unicode-bidi\"></p></bdo></bdo>x");
}

#[test]
fn the_words_of_a_link_are_not_turned_letter_by_letter() {
    let texts = |html: &str| -> Vec<String> { paras(&parse(html)).iter().map(|p| p.text()).collect() };
    // Turned inside the link: by an element, by a style, by the mark itself.
    assert_eq!(texts("<p><a href=\"https://x.example/\"><bdo dir=rtl>moc.knab</bdo></a></p>"), ["moc.knab"]);
    assert_eq!(
        texts("<p><a href=\"https://x.example/\"><span style=\"unicode-bidi:bidi-override;direction:rtl\">moc.knab</span></a></p>"),
        ["moc.knab"]
    );
    assert_eq!(texts("<p><a href=\"https://x.example/\">\u{202e}moc.knab\u{202c} &#x202E;x</a></p>"), ["moc.knab\u{202c} x"]);
    assert_eq!(texts("<p><a href=\"https://x.example/\" style=\"unicode-bidi:bidi-override;direction:rtl\">moc.knab</a></p>"), ["moc.knab"]);
    // Turned from outside it: the link is set apart, and what is set apart starts unturned.
    assert_eq!(
        texts("<p><bdo dir=rtl>ab <a href=\"https://x.example/\">moc.knab</a> cd</bdo></p>"),
        ["\u{202e}ab \u{2068}moc.knab\u{2069} cd\u{202c}"]
    );
    assert_eq!(texts("<p>\u{202e}ab <a href=\"https://x.example/\">moc.knab</a></p>"), ["\u{202e}ab \u{2068}moc.knab\u{2069}"]);
    // A link keeps what sets its words apart or says which way they are read, and a link in
    // text that nothing turns is as it was.
    assert_eq!(
        texts("<p>a <a href=\"https://x.example/\" dir=rtl>b <bdi>c</bdi></a> <a href=\"https://x.example/\">d</a></p>"),
        ["a \u{2067}b \u{2068}c\u{2069}\u{2069} d"]
    );
    // Text that is no link is turned as it asks to be.
    assert_eq!(texts("<p><a name=here><bdo dir=rtl>ab</bdo></a></p>"), ["\u{202e}ab\u{202c}"]);
}

#[test]
fn a_table_written_from_the_right_says_so() {
    use crate::rich::Table;
    let table =
        |html: &str| -> Table { parse(html).blocks.into_iter().find_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).expect("a table") };
    let side = |t: &Table| -> Vec<(bool, Align)> {
        let paras = t.rows.iter().flatten().flat_map(|c| &c.blocks);
        paras.filter_map(|b| if let Block::Para(p) = b { Some((p.rtl, p.align)) } else { None }).collect()
    };
    let cells = "<tr><td>a<td>b</table>";
    let t = table(&format!("<table border=1>{cells}"));
    assert_eq!((t.rtl, t.align, side(&t)), (false, Align::Left, vec![(false, Align::Left); 2]));
    // Its own direction turns its columns; it stands where the text around it starts.
    let t = table(&format!("<table border=1 dir=rtl>{cells}"));
    assert_eq!((t.rtl, t.align, side(&t)), (true, Align::Left, vec![(true, Align::Right); 2]));
    let t = table(&format!("<div dir=rtl><table border=1>{cells}</div>"));
    assert_eq!((t.rtl, t.align, side(&t)), (true, Align::Right, vec![(true, Align::Right); 2]));
    let t = table(&format!("<body style=\"direction:rtl\"><table border=1 dir=ltr>{cells}"));
    assert_eq!((t.rtl, t.align, side(&t)), (false, Align::Right, vec![(false, Align::Left); 2]));
    // Told where to stand, it stands there.
    let t = table(&format!("<div dir=rtl><table border=1 align=left>{cells}</div>"));
    assert_eq!((t.rtl, t.align), (true, Align::Left));
    let t = table(&format!("<div dir=rtl><table border=1 style=\"margin:0 auto\">{cells}</div>"));
    assert_eq!((t.rtl, t.align), (true, Align::Center));
    // A table inside a cell goes by what it stands in.
    let t = table(&format!("<table border=1 dir=rtl><tr><td><table border=1>{cells}<td>c</table>"));
    let Some(Block::Table(inner)) = t.rows[0][0].blocks.first() else { panic!("a table in the cell") };
    assert!(t.rtl && inner.rtl && inner.align == Align::Right);

    // Where each column starts, and which of them a cell's left edge is.
    let (ltr, rtl) = (Table::default(), Table { rtl: true, ..Table::default() });
    assert_eq!(ltr.lefts(&[10.0, 20.0, 30.0], 5.0, 75.0, 5.0), [5.0, 20.0, 45.0]);
    assert_eq!(rtl.lefts(&[10.0, 20.0, 30.0], 5.0, 75.0, 5.0), [65.0, 40.0, 5.0]);
    assert!(rtl.lefts(&[], 0.0, 0.0, 0.0).is_empty());
    assert_eq!((ltr.left_column(1, 2, 3), rtl.left_column(1, 2, 3), rtl.left_column(0, 1, 3)), (1, 2, 0));
    // Hostile: spans past the columns, no columns.
    assert_eq!((rtl.left_column(2, usize::MAX, 3), rtl.left_column(usize::MAX, usize::MAX, 3), rtl.left_column(0, 0, 0)), (2, 2, 0));

    // RTF: `\rtlrow` on the first row.
    let rtf = |row: &str| -> bool {
        let text = format!(
            "{{\\rtf1 \\trowd{row}\\cellx3000\\cellx6000\\intbl a\\cell b\\cell\\row\\trowd\\cellx3000\\cellx6000\\intbl c\\cell d\\cell\\row}}"
        );
        let doc = crate::rtf::parse(text.as_bytes());
        let Some(Block::Table(t)) = doc.blocks.first() else { panic!("a table") };
        t.rtl
    };
    assert!(rtf("\\rtlrow") && !rtf("") && !rtf("\\ltrrow") && !rtf("\\rtlrow\\ltrrow"));
}

#[test]
fn boxes_stand_inside_one_another_and_the_body_has_a_colour_behind_it() {
    use crate::rich::{MAX_BOXES, box_at, boxed_run};
    let doc = parse(
        "<body bgcolor=\"#102030\" style=\"color:#fff\"><div style=\"border:2px solid #f00;padding:10px\"><p>a</p>         <div style=\"background:#fff;border:1px solid #00f\"><p>b</p><div style=\"padding:5px\">c</div><p>d</p></div><p>e</p></div><p>f</p>         <div style=\"background:#ffffff\">white</div></body>",
    );
    // CSS 2.1 section 14.2: the body's background is behind the whole message, not behind
    // each word of it.
    assert_eq!(doc.fill, Some([16, 32, 48]));
    let p = paras(&doc);
    assert!(p.iter().all(|q| q.runs.iter().all(|r| r.style.background.is_none())));
    let frame = |n: usize| p[n].frame.as_ref().unwrap();
    assert_eq!(p.iter().map(|q| q.frame.as_ref().map_or(0, |f| f.depth() + 1)).collect::<Vec<_>>(), [1, 2, 3, 2, 1, 0, 1]);
    assert!(Arc::ptr_eq(frame(0), frame(4)) && Arc::ptr_eq(frame(1), frame(3)));
    assert!(Arc::ptr_eq(frame(1).within.as_ref().unwrap(), frame(0)) && Arc::ptr_eq(frame(2).within.as_ref().unwrap(), frame(1)));
    // The colour behind what a box holds is the nearest one round it.
    assert_eq!((frame(0).behind(), frame(1).behind(), frame(2).fill, frame(2).behind()), (None, Some([255; 3]), None, Some([255; 3])));
    // On a body with a colour, white is a colour.
    assert_eq!(frame(6).fill, Some([255; 3]));
    // Outermost first; none past the innermost.
    assert!(Arc::ptr_eq(box_at(frame(2), 0).unwrap(), frame(0)) && Arc::ptr_eq(box_at(frame(2), 2).unwrap(), frame(2)));
    assert!(box_at(frame(2), 3).is_none() && box_at(frame(0), 1).is_none());
    // Five blocks in the outer box, three of them in the one inside it, one in the innermost.
    assert_eq!(boxed_run(&doc.blocks, 0).map(|r| r.1), Some(5));
    assert_eq!(boxed_run(&doc.blocks[1..], 1).map(|r| r.1), Some(3));
    assert_eq!(boxed_run(&doc.blocks[2..], 2).map(|r| r.1), Some(1));
    assert!(boxed_run(&doc.blocks, 1).is_none() && boxed_run(&doc.blocks[5..], 0).is_none() && boxed_run(&[], 0).is_none());

    // White behind the body is the page itself; a colour set inside a table is that cell's.
    assert_eq!(parse("<body style=\"background:#fff\">x</body>").fill, None);
    assert_eq!(parse("<html style=\"background-color:#ddd\"><body>x</body></html>").fill, Some([221; 3]));
    assert_eq!(parse("<table><tr><td><body bgcolor=#123456>x</body></table>").fill, None);

    // A layout table of one column is taken away; what it held stands in the box it stood in.
    let doc = parse("<div style=\"background:#036\"><table><tr><td><p>one</p></td></tr><tr><td><p>two</p></td></tr></table></div>");
    let p = paras(&doc);
    assert!(p.len() == 2 && Arc::ptr_eq(p[0].frame.as_ref().unwrap(), p[1].frame.as_ref().unwrap()));
    assert_eq!(p[0].frame.as_ref().unwrap().fill, Some([0, 51, 102]));

    // Hostile: far more boxes inside one another than are ever kept.
    let deep = format!("{}x{}", "<div style=\"border:1px solid red;background:#ccc\">".repeat(5000), "</div>".repeat(4000));
    let doc = parse(&deep);
    let p = paras(&doc);
    let innermost = p.iter().filter_map(|q| q.frame.as_ref()).map(|f| f.depth()).max().unwrap();
    assert_eq!(innermost, MAX_BOXES - 1);
    for q in &p {
        if let Some(f) = &q.frame {
            assert!(box_at(f, MAX_BOXES).is_none() && f.behind().is_some());
        }
    }
    // A chain made by hand that is longer than any reader makes is walked only so far.
    let mut chain = Arc::new(crate::rich::Frame::default());
    for _ in 0..10_000 {
        chain = Arc::new(crate::rich::Frame { within: Some(chain), ..crate::rich::Frame::default() });
    }
    assert_eq!(chain.depth(), MAX_BOXES * 2);
    assert!(chain.behind().is_none() && box_at(&chain, MAX_BOXES).is_none());
    let list = vec![Block::Para(Para { frame: Some(chain), ..Para::default() })];
    assert!(boxed_run(&list, 0).is_some_and(|r| r.1 == 1) && boxed_run(&list, MAX_BOXES).is_none());
    // Taking the chain apart one box at a time, so that dropping it is not as deep as it is long.
    let Some(Block::Para(mut para)) = list.into_iter().next() else { panic!("no paragraph") };
    let mut at = para.frame.take();
    while let Some(frame) = at {
        at = Arc::try_unwrap(frame).ok().and_then(|f| f.within);
    }
}

#[test]
fn text_within_a_line_has_a_border_and_boxes_have_rounded_corners() {
    use crate::rich::{Edge, MAX_RADIUS, one_edge};
    let doc = parse(
        "<p>a <span style=\"border:1px solid #f00\">b</span> <span style=\"border-bottom:1px solid #f00\">c</span> \
         <code style=\"border:2px dashed #00f;border-radius:4px\">d</code> e</p>\
         <div style=\"border:1px solid #00f;border-radius:12px;padding:4px\">f</div>\
         <div style=\"background:#ddd;border-radius:9999px\">g</div>\
         <table><tr><td style=\"border-radius:6px 2px;background:#ccc\">h</td><td>i</td></tr></table>\
         <p style=\"border:1px solid red;border-radius:50%\">j</p><div style=\"border-radius:8px\">k</div>",
    );
    let p = paras(&doc);
    // CSS 2.1 section 8.5: a border on text within a line is round that text and no other.
    let edge = |text: &str| p[0].runs.iter().find(|r| r.text.trim() == text).unwrap().style.edge;
    assert_eq!(edge("b"), Some(Edge { width: 1, color: Some([255, 0, 0]), line: Line::Solid }));
    assert_eq!(edge("d"), Some(Edge { width: 2, color: Some([0, 0, 255]), line: Line::Dashed }));
    // A line on one side only is not a border round the text.
    assert_eq!((edge("a"), edge("c"), edge("e")), (None, None, None));
    // A block's border is the block's: its text has none of its own.
    assert!(p.iter().skip(1).all(|q| q.runs.iter().all(|r| r.style.edge.is_none())));
    // The first length rounds all four corners, never by more than is drawn; a share of the
    // box's size is not read.
    let radius = |n: usize| p[n].frame.as_ref().map(|f| f.radius);
    assert_eq!((radius(1), radius(2), radius(3)), (Some(12), Some(MAX_RADIUS), Some(0)));
    // Rounded corners alone, with no border, fill or room, make no box.
    assert!(p[4].frame.is_none());
    let table = doc
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!((table.rows[0][0].radius, table.rows[0][1].radius), (6, 0));

    // All four sides the same line that can be seen, or none.
    let line = Edge { width: 1, color: None, line: Line::Solid };
    assert_eq!(one_edge(&[line; 4]), Some(line));
    assert_eq!(one_edge(&[line, line, line, Edge { width: 2, ..line }]), None);
    assert_eq!(one_edge(&[Edge::default(); 4]), None);

    // Nothing that is written for a radius or a border is too much.
    for junk in ["-5px", "1e99px", "", "nanpx", "999999999999999999999px", "\u{0}", "4px/8px", "inherit"] {
        let doc = parse(&format!("<div style=\"border:1px solid;border-radius:{junk}\">x<td style=\"border-radius:{junk}\">y</div>"));
        assert!(paras(&doc).iter().all(|q| q.frame.as_ref().is_none_or(|f| f.radius <= MAX_RADIUS)), "{junk}");
    }
    let deep = format!("<p>{}x{}</p>", "<span style=\"border:300px double red;border-radius:9px\">".repeat(3000), "</span>".repeat(2990));
    let doc = parse(&deep);
    assert!(paras(&doc).iter().any(|q| q.runs.iter().any(|r| r.style.edge.is_some())));
}

#[test]
fn rtf_highlights_are_marker_colours_and_list_numbers_follow_their_overrides() {
    // RTF 1.9.1, "Highlighting": the highlight is a place in the colour table. It is shown as
    // one of sixteen colours; a colour behind text that is not a highlight is kept as it is.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi{\\colortbl;\\red255\\green242\\blue204;\\red0\\green128\\blue128;\\red250\\green250\\blue250;\
          \\red100\\green104\\blue110;\\red30\\green60\\blue140;\\red255\\green150\\blue180;\\red10\\green10\\blue10;}\
          {\\highlight1 a}{\\highlight2 b}{\\highlight3 c}{\\highlight4 d}{\\highlight5 e}{\\highlight6 f}{\\highlight7 g}\
          {\\cb1 h}{\\chcbpat6 i}{\\highlight0 j}{\\highlight99 k}\\par}",
    );
    let p = paras(&doc);
    let behind: Vec<Option<[u8; 3]>> =
        "abcdefghijk".chars().map(|c| p[0].runs.iter().find(|r| r.text.contains(c)).unwrap().style.background).collect();
    assert_eq!(
        behind,
        [
            // A pale yellow is the yellow marker, as Outlook shows it.
            Some([255, 255, 0]),
            // One of the sixteen is itself.
            Some([0, 128, 128]),
            // Nearly no colour: a grey, by how light it is.
            Some([255, 255, 255]),
            Some([128, 128, 128]),
            // Else the pure colour of its hue, dark or bright.
            Some([0, 0, 128]),
            Some([255, 0, 0]),
            Some([0, 0, 0]),
            Some([255, 242, 204]),
            Some([255, 150, 180]),
            None,
            None,
        ]
    );

    // RTF 1.9.1, "List Override Table": a number that paragraphs name may start a level of
    // its list somewhere else (`\listoverridestartat`), or write it another way
    // (`\listoverrideformat`). The first `\lfolevel` is for the first level.
    let doc = crate::rtf::parse(
        b"{\\rtf1\\ansi\
          {\\*\\listtable{\\list\
          {\\listlevel\\levelnfc0\\levelstartat1{\\leveltext\\'02\\'00.;}{\\levelnumbers\\'01;}}\
          {\\listlevel\\levelnfc4\\levelstartat1{\\leveltext\\'03\\'00.\\'01;}{\\levelnumbers\\'01\\'03;}}\\listid7}}\
          {\\*\\listoverridetable{\\listoverride\\listid7\\listoverridecount0\\ls1}\
          {\\listoverride\\listid7\\listoverridecount1{\\lfolevel\\listoverridestartat\\levelstartat5}\\ls2}\
          {\\listoverride\\listid7\\listoverridecount2{\\lfolevel\\listoverridestartat\\listoverrideformat\
          {\\listlevel\\levelnfc1\\levelstartat3{\\leveltext\\'02\\'00);}{\\levelnumbers\\'01;}}}\
          {\\lfolevel\\listoverrideformat{\\listlevel\\levelnfc0\\levelstartat9{\\leveltext\\'03\\'00-\\'01;}{\\levelnumbers\\'01\\'03;}}}\\ls3}\
          {\\listoverride\\listid7\\listoverridecount0\\ls4}}\
          \\pard\\ls1 one\\par two\\par \
          \\pard\\ls4 three\\par \
          \\pard\\ls2 five\\par six\\par \
          \\pard\\ls2\\ilvl1 six a\\par \
          \\pard\\ls1 seven\\par \
          \\pard\\ls2 five again\\par \
          \\pard\\ls3 three\\par four\\par \
          \\pard\\ls3\\ilvl1 four one\\par four two\\par \
          \\pard\\ls4 five\\par}",
    );
    let kinds: Vec<String> = paras(&doc)
        .iter()
        .map(|p| match &p.kind {
            ParaKind::Item(marker) => marker.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    // One list counts on through every number that names it; a number that starts a level
    // somewhere begins there each time it takes over. A level written anew keeps the list's
    // start unless the override says it starts too.
    assert_eq!(kinds, ["1.", "2.", "3.", "5.", "6.", "6.a", "7.", "5.", "III)", "IV)", "IV-1", "IV-2", "5."]);

    // Hostile: more levels than a list has, changes with nothing in them, words outside any
    // level, numbers at the edge, and more overrides than are kept.
    let mut body =
        String::from("{\\rtf1\\ansi{\\*\\listtable{\\list{\\listlevel\\levelnfc0{\\leveltext\\'02\\'00.;}}\\listid1}}{\\*\\listoverridetable");
    body.push_str("\\listoverridestartat\\levelstartat7\\listlevel\\levelnfc3{\\leveltext\\'ff\\'08\\'07}\\ls-5");
    for i in 0..1500 {
        body.push_str(&format!("{{\\listoverride\\listid{}\\listoverridecount2147483647", i % 3));
        for _ in 0..12 {
            body.push_str("{\\lfolevel\\listoverridestartat\\levelstartat2147483647\\listoverrideformat{\\listlevel\\levelnfc-7\\levelstartat-2147483648{\\leveltext\\'ff\\'00\\'08\\'07\\'01.;}}}");
        }
        body.push_str(&format!("{{\\lfolevel}}{{\\lfolevel\\listoverrideformat}}\\ls{i}}}"));
    }
    body.push('}');
    for i in 0..1600 {
        body.push_str(&format!("\\pard\\ls{}\\ilvl{} x\\par ", i % 1550 - 6, i % 11));
    }
    body.push('}');
    let doc = crate::rtf::parse(body.as_bytes());
    assert_eq!(paras(&doc).len(), 1600);
    assert!(paras(&doc).iter().all(|p| match &p.kind {
        ParaKind::Item(marker) => marker.chars().count() <= 400,
        _ => true,
    }));
}

/// META_PATBLT: the brush copied onto a box (left, top, wide, high), mixed the way `how` says.
fn pat(how: u32, [left, top, wide, high]: [i16; 4]) -> (u16, Vec<u8>) {
    let mut body = how.to_le_bytes().to_vec();
    body.extend(words(&[high, wide, top, left]));
    (0x061D, body)
}

/// META_CREATEREGION of these scans: top, bottom, and the ends of each line (left, right).
fn wmf_region(scans: &[(i16, i16, &[i16])]) -> (u16, Vec<u8>) {
    let mut body = words(&[0, 6, 0, 0, 0, i16::try_from(scans.len()).unwrap(), 0, 0, 0, 100, 100]);
    for (top, bottom, ends) in scans {
        let count = i16::try_from(ends.len()).unwrap();
        body.extend(words(&[count, *top, *bottom]));
        body.extend(words(ends));
        body.extend(words(&[count]));
    }
    (0x06FF, body)
}

/// A region's data in an enhanced metafile: a header, and boxes (left, top, right, bottom).
fn emf_region(boxes: &[[i32; 4]]) -> Vec<u8> {
    let count = i32::try_from(boxes.len()).unwrap();
    let mut data = longs(&[32, 1, count, 16 * count, 0, 0, 100, 100]);
    for one in boxes {
        data.extend(longs(one));
    }
    data
}

/// EMR_BITBLT (or, with the size of the part taken, EMR_STRETCHBLT) of `dib` onto a box.
fn emf_blt(dib: Option<&[u8]>, to: [i32; 4], how: u32, from: (i32, i32), part: Option<(i32, i32)>) -> (u32, Vec<u8>) {
    let start = if part.is_some() { 108 } else { 100 };
    let mut body = longs(&[0, 0, 0, 0, to[0], to[1], to[2], to[3], how.cast_signed(), from.0, from.1]);
    body.extend([1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0].iter().flat_map(|v| v.to_le_bytes()));
    let (header, pixels) = dib.map_or((0, 0), |dib| (40, i32::try_from(dib.len() - 40).unwrap()));
    body.extend(longs(&[0, 0, if dib.is_some() { start } else { 0 }, header, if dib.is_some() { start + 40 } else { 0 }, pixels]));
    if let Some((wide, high)) = part {
        body.extend(longs(&[wide, high]));
    }
    body.extend_from_slice(dib.unwrap_or(&[]));
    (if part.is_some() { 77 } else { 76 }, body)
}

const BLACK: [u8; 3] = [0; 3];
const CYAN: [u8; 3] = [0, 255, 255];
const MAGENTA: [u8; 3] = [255, 0, 255];
const YELLOW: [u8; 3] = [255, 255, 0];
const PATCOPY: u32 = 0x00F0_0021;

#[test]
fn pixels_packed_in_runs_are_unpacked() {
    use crate::metafile::dib_pixels;
    let table = [0, 0, 0, 0, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0];
    let colours = |dib: &[u8]| dib_pixels(dib, 10_000).map(|(size, rgba)| (size, rgba.chunks(4).map(|p| [p[0], p[1], p[2]]).collect::<Vec<_>>()));
    let read = |size, bits, packed, runs: &[u8]| colours(&dib_of(size, bits, packed, 4, &table, runs));
    // Eight bits: the bottom row is three reds, a step to the next, then two numbers as they
    // are (green, blue) made up to a word, and the end. What no run reaches is the first colour.
    let runs = [3, 1, 0, 0, 0, 3, 2, 3, 1, 0, 0, 1];
    assert_eq!(read((3, 2), 8, 1, &runs).unwrap(), ((3, 2), vec![GREEN, BLUE, RED, RED, RED, RED]));
    assert_eq!(read((3, 2), 8, 1, &[1, 2, 0, 1]).unwrap().1, [BLACK, BLACK, BLACK, GREEN, BLACK, BLACK]);
    // A step to the right and up, and then a run there.
    assert_eq!(read((3, 2), 8, 1, &[0, 2, 1, 1, 2, 3, 0, 1]).unwrap().1, [BLACK, BLUE, BLUE, BLACK, BLACK, BLACK]);
    // Four bits: a run is two numbers by turns; numbers as they are come two to a byte.
    assert_eq!(read((4, 1), 4, 2, &[4, 0x12, 0, 1]).unwrap().1, [RED, GREEN, RED, GREEN]);
    assert_eq!(read((4, 1), 4, 2, &[0, 3, 0x32, 0x10, 1, 0x20, 0, 1]).unwrap().1, [BLUE, GREEN, RED, GREEN]);
    // A run that goes past the side is cut off there, and one that ends early leaves the rest.
    assert_eq!(read((2, 1), 8, 1, &[200, 1]).unwrap().1, [RED, RED]);
    assert_eq!(read((2, 2), 8, 1, &[]).unwrap().1, [BLACK; 4]);
    // Not read: numbers as they are that are not all there, a step that is cut short, runs for
    // a bitmap kept from the top down, runs for bits that are not packed that way.
    assert_eq!(read((3, 1), 8, 1, &[0, 3, 1]), None);
    assert_eq!(read((3, 1), 8, 1, &[0, 2, 1]), None);
    assert_eq!(read((2, -1), 8, 1, &[2, 1]), None);
    assert_eq!(read((2, 1), 4, 1, &[2, 1]), None);
    assert_eq!(read((2, 1), 8, 2, &[2, 1]), None);
    // Steps and runs that would go anywhere: nothing is written outside the bitmap.
    for runs in [&[0u8, 2, 255, 255, 255, 1][..], &[255, 1, 255, 2, 0, 0, 255, 3], &[0, 255], &[0, 2], &[0; 64], &[255; 64]] {
        for bits in [8, 4] {
            let _ = read((3, 2), bits, if bits == 8 { 1 } else { 2 }, runs);
        }
    }
}

#[test]
fn a_bitmap_is_mixed_with_what_is_there_and_with_the_brush() {
    use crate::canvas::{mix_needs, mixed};
    use crate::metafile::wmf_picture;
    // The table of a way of mixing has a bit for each of the eight things three bits can be.
    assert_eq!((mixed(0xCC, 9, 0x5A, 7), mixed(0xF0, 9, 0x5A, 7), mixed(0xAA, 9, 0x5A, 7)), (0x5A, 9, 7));
    assert_eq!((mixed(0x66, 0, 0xF0, 0xFF), mixed(0x88, 0, 0xF0, 0x3C), mixed(0xEE, 0, 0xF0, 0x0F)), (0x0F, 0x30, 0xFF));
    assert_eq!((mixed(0, 1, 2, 3), mixed(0xFF, 1, 2, 3), mixed(0x55, 1, 2, 0xF0)), (0, 255, 0x0F));
    assert_eq!(
        [0xCC, 0xF0, 0x55, 0xC0, 0, 0xFF].map(mix_needs),
        [(true, false), (false, true), (false, false), (true, true), (false, false), (false, false)]
    );

    let dib = small_dib();
    let at = |bmp: &[u8], points: [(usize, usize); 4]| points.map(|(x, y)| pixel(bmp, x, y));
    let quarters = [(20, 20), (40, 20), (20, 40), (40, 40)];
    // A red page (the brush copied over all of it), and the bitmap laid on it each way.
    let over = |how: u32| {
        let mut record = stretched(&dib, [2, 2, 0, 0], [40, 40, 10, 10]);
        record.1[..4].copy_from_slice(&how.to_le_bytes());
        let mut records = red_window();
        records.extend([pat(PATCOPY, [0, 0, 100, 100]), record]);
        let bmp = wmf_picture(&wmf(&records), Some((50, 50))).unwrap();
        assert_eq!(pixel(&bmp, 70, 70), RED, "{how:#x}");
        at(&bmp, quarters)
    };
    assert_eq!(over(0x00CC_0020), [RED, WHITE, BLUE, GREEN]);
    assert_eq!(over(0x0066_0046), [BLACK, CYAN, MAGENTA, YELLOW]);
    assert_eq!(over(0x0088_00C6), [RED, RED, BLACK, BLACK]);
    assert_eq!(over(0x00EE_0086), [RED, WHITE, MAGENTA, YELLOW]);
    assert_eq!(over(0x0033_0008), [CYAN, BLACK, YELLOW, MAGENTA]);
    // With the brush too: the bitmap where the brush is.
    assert_eq!(over(0x00C0_00CA), [RED, RED, BLACK, BLACK]);

    // No bitmap: the brush, what is there turned over, black, white, the brush or what is
    // there but not both. A record of a bitmap that has none does the same.
    let on = |records: Vec<(u16, Vec<u8>)>| {
        let mut all = red_window();
        all.push(pat(PATCOPY, [0, 0, 50, 100]));
        all.extend(records);
        let bmp = wmf_picture(&wmf(&all), Some((50, 50))).unwrap();
        [pixel(&bmp, 25, 50), pixel(&bmp, 75, 50), pixel(&bmp, 25, 95), pixel(&bmp, 75, 95)]
    };
    let band = [0, 0, 100, 90];
    assert_eq!(on(vec![]), [RED, WHITE, RED, WHITE]);
    assert_eq!(on(vec![pat(0x0055_0009, band)]), [CYAN, BLACK, RED, WHITE]);
    assert_eq!(on(vec![pat(0x0000_0042, band)]), [BLACK, BLACK, RED, WHITE]);
    assert_eq!(on(vec![pat(0x00FF_0062, band)]), [WHITE, WHITE, RED, WHITE]);
    assert_eq!(on(vec![pat(0x005A_0049, band)]), [BLACK, CYAN, RED, WHITE]);
    let mut bare = 0x0055_0009u32.to_le_bytes().to_vec();
    bare.extend(words(&[0, 0, 0, 0, 0, 90, 100, 0, 0]));
    assert_eq!(on(vec![(0x0B41, bare)]), [CYAN, BLACK, RED, WHITE]);
    let mut bare = 0x0055_0009u32.to_le_bytes().to_vec();
    bare.extend(words(&[0, 0, 0, 90, 100, 0, 0]));
    assert_eq!(on(vec![(0x0940, bare)]), [CYAN, BLACK, RED, WHITE]);
    // A way that needs a bitmap, with none; and one that needs a brush, with one nobody knows.
    let mut records = red_window();
    records.push(pat(0x00CC_0020, band));
    assert_eq!(wmf_picture(&wmf(&records), Some((50, 50))), None);
    let unknown = vec![(0x020C, words(&[100, 100])), (0x02FC, vec![3, 0, 0, 0, 0, 0, 0, 0]), (0x012D, words(&[0])), pat(PATCOPY, band)];
    assert_eq!(wmf_picture(&wmf(&unknown), Some((50, 50))), None);
}

#[test]
fn brushes_of_a_hatching_and_of_a_pattern_are_filled_with() {
    use crate::metafile::{emf_picture, wmf_picture};
    let all = [0, 0, 100, 100];
    // Lines across, red, drawn at twice the size: every eighth pair of rows. Between them is
    // the paper's colour, or (when what is behind shows through) what was there.
    let hatched = |kind: u8, through: bool| {
        let mut records = vec![(0x020B, words(&[0, 0])), (0x020C, words(&[100, 100]))];
        records.extend([(0x02FC, vec![0, 0, 0, 0, 255, 0, 0, 0]), (0x02FC, vec![2, 0, 255, 0, 0, 0, kind, 0])]);
        records.extend([(0x012D, words(&[0])), pat(PATCOPY, all), (0x012D, words(&[1])), (0x0102, words(&[if through { 1 } else { 2 }]))]);
        records.push(pat(PATCOPY, all));
        wmf_picture(&wmf(&records), Some((50, 50))).unwrap()
    };
    let bmp = hatched(0, false);
    assert_eq!([pixel(&bmp, 50, 6), pixel(&bmp, 50, 7), pixel(&bmp, 50, 10), pixel(&bmp, 50, 22)], [RED, RED, WHITE, RED]);
    let bmp = hatched(0, true);
    assert_eq!([pixel(&bmp, 50, 6), pixel(&bmp, 50, 10)], [RED, BLUE]);
    // Lines down, lines that fall and that rise to the right, and the two grids.
    let bmp = hatched(1, true);
    assert_eq!([pixel(&bmp, 6, 50), pixel(&bmp, 10, 50), pixel(&bmp, 22, 50)], [RED, BLUE, RED]);
    assert_eq!([pixel(&hatched(2, true), 4, 4), pixel(&hatched(2, true), 4, 10)], [RED, BLUE]);
    assert_eq!([pixel(&hatched(3, true), 4, 10), pixel(&hatched(3, true), 4, 4)], [RED, BLUE]);
    let bmp = hatched(4, true);
    assert_eq!([pixel(&bmp, 6, 50), pixel(&bmp, 50, 6), pixel(&bmp, 10, 10)], [RED, RED, BLUE]);
    let bmp = hatched(5, true);
    assert_eq!([pixel(&bmp, 4, 4), pixel(&bmp, 4, 10), pixel(&bmp, 8, 2)], [RED, RED, BLUE]);

    // A bitmap laid side by side, each of its pixels two of the page's each way.
    let mut tile = words(&[5, 0]);
    tile.extend_from_slice(&small_dib());
    let window = vec![(0x020B, words(&[0, 0])), (0x020C, words(&[100, 100]))];
    let mut records = window.clone();
    records.extend([(0x0142, tile.clone()), (0x012D, words(&[0])), pat(PATCOPY, all)]);
    let bmp = wmf_picture(&wmf(&records), Some((50, 50))).unwrap();
    assert_eq!(
        [pixel(&bmp, 0, 0), pixel(&bmp, 1, 1), pixel(&bmp, 2, 0), pixel(&bmp, 0, 2), pixel(&bmp, 2, 2), pixel(&bmp, 4, 0)],
        [RED, RED, WHITE, BLUE, GREEN, RED]
    );
    // A shape is filled with it as a box is.
    let mut records = window.clone();
    records.extend([(0x0142, tile.clone()), (0x012D, words(&[0])), (0x041B, words(&[80, 80, 20, 20]))]);
    let bmp = wmf_picture(&wmf(&records), Some((50, 50))).unwrap();
    assert_eq!([pixel(&bmp, 40, 40), pixel(&bmp, 42, 40), pixel(&bmp, 40, 42), pixel(&bmp, 10, 10)], [RED, WHITE, BLUE, WHITE]);
    // The oldest kind of pattern: a bit to the pixel, set for white.
    let mut old = words(&[0, 8, 2, 2]);
    old.extend([1, 1]);
    old.extend([0; 22]);
    old.extend([0xF0, 0, 0x0F, 0]);
    let mut records = window.clone();
    records.extend([(0x01F9, old), (0x012D, words(&[0])), pat(PATCOPY, all)]);
    let bmp = wmf_picture(&wmf(&records), Some((50, 50))).unwrap();
    assert_eq!(
        [pixel(&bmp, 2, 0), pixel(&bmp, 10, 0), pixel(&bmp, 2, 2), pixel(&bmp, 10, 2), pixel(&bmp, 10, 4)],
        [WHITE, BLACK, BLACK, WHITE, BLACK]
    );
    // A pattern whose colours are numbers in a palette, or that is cut short, is not known.
    for bad in [words(&[5, 1]).into_iter().chain(small_dib()).collect::<Vec<u8>>(), tile[..tile.len() - 3].to_vec()] {
        let mut records = window.clone();
        records.extend([(0x0142, bad), (0x012D, words(&[0])), pat(PATCOPY, all)]);
        assert_eq!(wmf_picture(&wmf(&records), Some((50, 50))), None);
    }

    // Enhanced: lines down in green on the paper's white, and a bitmap side by side.
    let mut brush = longs(&[2, 2]);
    brush.extend([0, 255, 0, 0, 1, 0, 0, 0]);
    let fill = vec![(37, longs(&[2])), (37, 0x8000_0008u32.to_le_bytes().to_vec()), (43, longs(&[0, 0, 100, 100]))];
    let mut records = vec![(39, brush)];
    records.extend(fill.clone());
    let bmp = emf_picture(&emf_of(&records), Some((50, 50))).unwrap();
    assert_eq!([pixel(&bmp, 3, 50), pixel(&bmp, 4, 50), pixel(&bmp, 11, 50)], [GREEN, WHITE, GREEN]);
    let dib = small_dib();
    let mut pattern = longs(&[2, 0, 32, 40, 72, 16]);
    pattern.extend_from_slice(&dib);
    let mut records = vec![(94, pattern.clone())];
    records.extend(fill.clone());
    let bmp = emf_picture(&emf_of(&records), Some((50, 50))).unwrap();
    assert_eq!(
        [pixel(&bmp, 10, 10), pixel(&bmp, 11, 10), pixel(&bmp, 10, 11), pixel(&bmp, 11, 11), pixel(&bmp, 12, 12)],
        [RED, WHITE, BLUE, GREEN, RED]
    );
    // One whose bitmap is not in the record is not known, and nothing is filled with it.
    pattern[12..16].copy_from_slice(&4000i32.to_le_bytes());
    let mut records = vec![(94, pattern)];
    records.extend(fill);
    assert_eq!(emf_picture(&emf_of(&records), Some((50, 50))), None);
}

#[test]
fn regions_are_filled_framed_turned_over_and_cut_to() {
    use crate::metafile::{emf_picture, wmf_picture};
    // A box at the top left, and under it one line of two boxes.
    let region = wmf_region(&[(10, 30, &[10, 30]), (50, 70, &[10, 30, 60, 90])]);
    let spots = [(20, 20), (20, 60), (75, 60), (45, 60), (20, 40)];
    let draw = |more: Vec<(u16, Vec<u8>)>| {
        let mut records = red_window();
        records.push(region.clone());
        records.extend(more);
        wmf_picture(&wmf(&records), Some((50, 50)))
    };
    let at = |bmp: &[u8]| spots.map(|(x, y)| pixel(bmp, x, y));
    // Painted with the brush there is; filled with one that is named; turned over.
    assert_eq!(at(&draw(vec![(0x012B, words(&[1]))]).unwrap()), [RED, RED, RED, WHITE, WHITE]);
    let blue = (0x02FC, vec![0, 0, 0, 0, 255, 0, 0, 0]);
    assert_eq!(at(&draw(vec![blue.clone(), (0x0228, words(&[1, 2]))]).unwrap()), [BLUE, BLUE, BLUE, WHITE, WHITE]);
    assert_eq!(at(&draw(vec![(0x012A, words(&[1]))]).unwrap()), [BLACK, BLACK, BLACK, WHITE, WHITE]);
    // Framed: a rim two units wide and high, inside it.
    let bmp = draw(vec![(0x0429, words(&[1, 0, 2, 2]))]).unwrap();
    assert_eq!(
        [pixel(&bmp, 11, 20), pixel(&bmp, 20, 11), pixel(&bmp, 28, 20), pixel(&bmp, 20, 20), pixel(&bmp, 61, 60), pixel(&bmp, 8, 20)],
        [RED, RED, RED, WHITE, RED, WHITE]
    );
    // Cut to: the brush copied over all the page stays inside the region, by either record.
    for select in [0x012C, 0x012D] {
        let bmp = draw(vec![(select, words(&[1])), pat(PATCOPY, [0, 0, 100, 100])]).unwrap();
        assert_eq!(at(&bmp), [RED, RED, RED, WHITE, WHITE], "{select:#x}");
        assert_eq!([pixel(&bmp, 5, 5), pixel(&bmp, 95, 95)], [WHITE, WHITE]);
    }
    // A box that is kept out of; and one kept out of what was a box already.
    let bmp = draw(vec![(0x0415, words(&[60, 60, 40, 40])), pat(PATCOPY, [0, 0, 100, 100])]).unwrap();
    assert_eq!([pixel(&bmp, 50, 50), pixel(&bmp, 20, 20), pixel(&bmp, 80, 50)], [WHITE, RED, RED]);
    let bmp = draw(vec![(0x0416, words(&[100, 50, 0, 0])), (0x0415, words(&[60, 60, 40, 40])), pat(PATCOPY, [0, 0, 100, 100])]).unwrap();
    assert_eq!([pixel(&bmp, 45, 50), pixel(&bmp, 20, 20), pixel(&bmp, 80, 50)], [WHITE, RED, WHITE]);
    // A state that is saved and put back has the page whole again.
    let bmp = draw(vec![(0x001E, Vec::new()), (0x012C, words(&[1])), (0x0127, words(&[-1])), pat(PATCOPY, [0, 0, 100, 100])]).unwrap();
    assert_eq!(pixel(&bmp, 45, 60), RED);
    // A region that was never made, an object that is no region, a brush that is none.
    assert_eq!(draw(vec![(0x012B, words(&[5]))]), None);
    assert_eq!(draw(vec![(0x012B, words(&[0]))]), None);
    assert_eq!(draw(vec![(0x0228, words(&[1, 1]))]), None);

    // Enhanced: the same region, in the drawing's units.
    let data = emf_region(&[[10, 10, 30, 30], [10, 50, 30, 70], [60, 50, 90, 70]]);
    let size = i32::try_from(data.len()).unwrap();
    let with = |head: &[i32]| {
        let mut body = longs(&[0, 0, 100, 100, size]);
        body.extend(longs(head));
        body.extend_from_slice(&data);
        body
    };
    let draw = |more: Vec<(u32, Vec<u8>)>| {
        let mut records = red_brush();
        records.extend(more);
        emf_picture(&emf_of(&records), Some((50, 50)))
    };
    assert_eq!(at(&draw(vec![(74, with(&[]))]).unwrap()), [RED, RED, RED, WHITE, WHITE]);
    assert_eq!(at(&draw(vec![(73, with(&[]))]).unwrap()), [BLACK, BLACK, BLACK, WHITE, WHITE]);
    assert_eq!(at(&draw(vec![(71, with(&[0x8000_0004u32.cast_signed()]))]).unwrap()), [BLACK, BLACK, BLACK, WHITE, WHITE]);
    let bmp = draw(vec![(72, with(&[1, 2, 2]))]).unwrap();
    assert_eq!([pixel(&bmp, 11, 20), pixel(&bmp, 20, 11), pixel(&bmp, 20, 20)], [RED, RED, WHITE]);
    assert!(pixel(&bmp, 61, 60)[1] < 8 && pixel(&bmp, 64, 60) == WHITE);
    // Cut to, in the device's units; then a box kept out of, and one joined to it.
    let cut = |mode: i32| {
        let mut body = longs(&[size, mode]);
        body.extend_from_slice(&data);
        (75, body)
    };
    let page = (43, longs(&[0, 0, 100, 100]));
    assert_eq!(at(&draw(vec![cut(5), page.clone()]).unwrap()), [RED, RED, RED, WHITE, WHITE]);
    let bmp = draw(vec![cut(5), (29, longs(&[15, 15, 25, 25])), page.clone()]).unwrap();
    assert_eq!([pixel(&bmp, 20, 20), pixel(&bmp, 12, 12), pixel(&bmp, 20, 60)], [WHITE, RED, RED]);
    // Either (the half page or the region), one and not the other, the half page without it.
    let half = (30, longs(&[0, 0, 50, 100]));
    let joined = |mode: i32| {
        let bmp = draw(vec![half.clone(), cut(mode), page.clone()]).unwrap();
        [pixel(&bmp, 20, 20), pixel(&bmp, 40, 40), pixel(&bmp, 75, 60), pixel(&bmp, 75, 20)]
    };
    assert_eq!(joined(1), [RED, WHITE, WHITE, WHITE]);
    assert_eq!(joined(2), [RED, RED, RED, WHITE]);
    assert_eq!(joined(3), [WHITE, RED, RED, WHITE]);
    assert_eq!(joined(4), [WHITE, RED, WHITE, WHITE]);
    assert_eq!(draw(vec![half.clone(), cut(6), page.clone()]), None);
    // No region at all is the whole page again.
    let bmp = draw(vec![cut(5), (75, longs(&[0, 5])), page.clone()]).unwrap();
    assert_eq!(pixel(&bmp, 45, 60), RED);
    // A path that is no box is cut to as it would be filled: a triangle.
    let path = vec![(59, Vec::new()), (27, longs(&[10, 10])), (54, longs(&[90, 10])), (54, longs(&[50, 90])), (61, Vec::new()), (60, Vec::new())];
    let mut records = path.clone();
    records.extend([(67, longs(&[1])), page.clone()]);
    let bmp = draw(records).unwrap();
    assert_eq!([pixel(&bmp, 50, 30), pixel(&bmp, 12, 80), pixel(&bmp, 88, 80), pixel(&bmp, 50, 5)], [RED, WHITE, WHITE, WHITE]);
    // More boxes than a region may have, and boxes that are not in the record.
    let mut many = longs(&[0, 0, 100, 100, 32, 32, 1, 5000, 0, 0, 0, 100, 100]);
    many.extend(vec![0; 64]);
    assert_eq!(draw(vec![(74, many)]), None);
    let mut short = with(&[]);
    short.truncate(short.len() - 4);
    assert_eq!(draw(vec![(74, short)]), None);
}

#[test]
fn an_enhanced_drawing_copies_bitmaps_from_one_place_to_another() {
    use crate::metafile::emf_picture;
    let dib = small_dib();
    let draw = |more: Vec<(u32, Vec<u8>)>| {
        let mut records = red_brush();
        records.extend(more);
        emf_picture(&emf_of(&records), Some((50, 50)))
    };
    let at = |bmp: &[u8], points: [(usize, usize); 4]| points.map(|(x, y)| pixel(bmp, x, y));
    // A pixel of the bitmap to one of the device: its rows are counted from the top.
    let bmp = draw(vec![emf_blt(Some(&dib), [10, 10, 2, 2], 0x00CC_0020, (0, 0), None)]).unwrap();
    assert_eq!(at(&bmp, [(10, 10), (11, 10), (10, 11), (11, 11)]), [RED, WHITE, BLUE, GREEN]);
    assert_eq!([pixel(&bmp, 12, 12), pixel(&bmp, 9, 9)], [WHITE, WHITE]);
    // Stretched: all of it, and its bottom row alone.
    let quarters = [(20, 20), (40, 20), (20, 40), (40, 40)];
    let bmp = draw(vec![emf_blt(Some(&dib), [10, 10, 40, 40], 0x00CC_0020, (0, 0), Some((2, 2)))]).unwrap();
    assert_eq!(at(&bmp, quarters), [RED, WHITE, BLUE, GREEN]);
    let bmp = draw(vec![emf_blt(Some(&dib), [10, 10, 40, 40], 0x00CC_0020, (0, 1), Some((2, 1)))]).unwrap();
    assert_eq!(at(&bmp, quarters), [BLUE, GREEN, BLUE, GREEN]);
    // No bitmap: the brush, and what is there turned over.
    let bmp = draw(vec![emf_blt(None, [0, 0, 50, 100], PATCOPY, (0, 0), None), emf_blt(None, [0, 0, 100, 50], 0x0055_0009, (0, 0), None)]).unwrap();
    assert_eq!(at(&bmp, [(25, 25), (75, 25), (25, 75), (75, 75)]), [CYAN, BLACK, RED, WHITE]);
    // Mixed with what is there: the bitmap where the red is.
    let bmp =
        draw(vec![emf_blt(None, [0, 0, 100, 100], PATCOPY, (0, 0), None), emf_blt(Some(&dib), [10, 10, 40, 40], 0x0088_00C6, (0, 0), Some((2, 2)))])
            .unwrap();
    assert_eq!(at(&bmp, quarters), [RED, RED, BLACK, BLACK]);
    // Not drawn: a way that needs a bitmap, with none; colours that are numbers in a palette;
    // the bitmap's own units turned round; a record that is cut short.
    assert_eq!(draw(vec![emf_blt(None, [0, 0, 50, 50], 0x00CC_0020, (0, 0), None)]), None);
    let whole = emf_blt(Some(&dib), [10, 10, 40, 40], 0x00CC_0020, (0, 0), Some((2, 2)));
    let change = |at: usize, bytes: [u8; 4]| {
        let mut record = whole.clone();
        record.1[at - 8..at - 4].copy_from_slice(&bytes);
        draw(vec![record])
    };
    assert!(change(80, [0; 4]).is_some());
    assert_eq!(change(80, 1u32.to_le_bytes()), None);
    assert_eq!(change(56, 1.0f32.to_le_bytes()), None);
    assert_eq!(change(84, 4000u32.to_le_bytes()), None);
    assert_eq!(draw(vec![(77, whole.1[..60].to_vec())]), None);
}

#[test]
fn hostile_brushes_regions_and_copies_never_panic() {
    use crate::metafile::{emf_picture, wmf_picture};
    // Every record this reads, cut to every length and filled with every kind of junk: 13
    // kinds, 12 lengths, 4 fillings, each on a page of a hundred pixels a side.
    let fillings = [0u8, 1, 0x7F, 0xFF];
    for function in [0x0F43u16, 0x0B41, 0x0940, 0x061D, 0x06FF, 0x012C, 0x0228, 0x012B, 0x012A, 0x0429, 0x0415, 0x0142, 0x01F9] {
        for len in [0usize, 2, 4, 8, 10, 16, 18, 22, 28, 40, 64, 120] {
            for fill in fillings {
                let mut records = red_window();
                records.extend([
                    (0x041B, words(&[80, 80, 20, 20])),
                    (function, vec![fill; len]),
                    (0x012D, words(&[1])),
                    pat(PATCOPY, [0, 0, 100, 100]),
                ]);
                let _ = wmf_picture(&wmf(&records), Some((50, 50)));
            }
        }
    }
    for kind in [29u32, 30, 39, 67, 71, 72, 73, 74, 75, 76, 77, 81, 93, 94] {
        for len in [0usize, 4, 8, 16, 24, 32, 40, 56, 72, 92, 100, 160] {
            for fill in fillings {
                let mut records = red_brush();
                records.extend([(43, longs(&[20, 20, 80, 80])), (kind, vec![fill; len]), (37, longs(&[2])), (43, longs(&[0, 0, 100, 100]))]);
                let _ = emf_picture(&emf_of(&records), Some((50, 50)));
            }
        }
    }
    // Boxes and bitmaps as far out and as large as their numbers go, mirrored and not.
    let dib = small_dib();
    for far in [i16::MIN, -1, 0, 1, i16::MAX] {
        let mut records = red_window();
        records.extend([
            stretched(&dib, [far, far, far, far], [40, 40, 10, 10]),
            stretched(&dib, [2, 2, 0, 0], [far, far, far, far]),
            pat(0x005A_0049, [far, far, far, far]),
        ]);
        records.extend([
            wmf_region(&[(far, far, &[far, far]), (far, 0, &[0, far, far, 0])]),
            (0x012B, words(&[1])),
            (0x0429, words(&[1, 0, far, far])),
        ]);
        let _ = wmf_picture(&wmf(&records), Some((50, 50)));
    }
    for far in [i32::MIN, -1, 0, 1, 70_000, i32::MAX] {
        let data = emf_region(&[[far, far, far, far], [0, far, far, 0]]);
        let mut cut = longs(&[i32::try_from(data.len()).unwrap(), 2]);
        cut.extend_from_slice(&data);
        let mut frame = longs(&[0, 0, 0, 0, 0, 1, far, far]);
        frame.extend_from_slice(&data);
        let turn: Vec<u8> = [far as f32, 1.0e30, f32::NAN, -0.0, f32::INFINITY, 0.5].iter().flat_map(|v| v.to_le_bytes()).collect();
        let records = vec![
            emf_stretched(&dib, [far, far, far, far], [0, 0, 2, 2]),
            emf_stretched(&dib, [10, 10, 40, 40], [far, far, far, far]),
            emf_blt(Some(&dib), [far, far, far, far], 0x0066_0046, (far, far), Some((far, far))),
            emf_blt(Some(&dib), [10, 10, far, far], 0x00CC_0020, (far, far), None),
            (75, cut),
            (72, frame),
            (29, longs(&[far, far, far, far])),
            (35, turn),
            emf_stretched(&dib, [10, 10, 40, 40], [0, 0, 2, 2]),
            (29, longs(&[0, 0, 10, 10])),
        ];
        let _ = emf_picture(&emf_of(&records), Some((50, 50)));
    }
    // What is cut to, over and over: each costs the page once, and the work runs out.
    let mut records = red_window();
    records.extend(std::iter::repeat_n((0x0415, words(&[60, 60, 40, 40])), 3000));
    records.push(pat(PATCOPY, [0, 0, 100, 100]));
    let _ = wmf_picture(&wmf(&records), Some((50, 50)));
}

#[test]
fn a_pale_padded_column_in_a_centred_cell_keeps_its_fill_its_padding_and_its_place() {
    let doc = crate::html::parse(
        r##"<table width="100%"><tr><td align="center"><table width="640" bgcolor="#faf9f5"><tr><td style="padding:18px 36px 36px">Welcome</td></tr></table></td></tr></table>"##,
    );
    let Some(Block::Table(t)) = doc.blocks.iter().find(|b| matches!(b, Block::Table(_))) else { panic!("no table: {:?}", doc.blocks) };
    assert_eq!((t.width, t.align), (crate::rich::TableWidth::Pixels(640), Align::Center));
    let cell = &t.rows[0][0];
    assert_eq!((t.fill, cell.background, cell.padding), (Some([0xfa, 0xf9, 0xf5]), None, Some([18, 36, 36, 36])));
}

#[test]
fn a_table_keeps_its_fill_and_round_corners_and_stands_flush_under_another() {
    let doc = crate::html::parse(
        r##"<table width="100%" bgcolor="#faf9f5"><tr><td>Top</td></tr></table><table width="100%" style="background-color:#FFFFFF;border:1px solid #E8E5DB;border-radius:12px"><tr><td>Code</td></tr></table>"##,
    );
    let tables: Vec<&crate::rich::Table> = doc.blocks.iter().filter_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).collect();
    assert_eq!(tables.len(), 2, "{:?}", doc.blocks);
    assert_eq!((tables[0].fill, tables[0].snug, tables[0].rows[0][0].background), (Some([0xfa, 0xf9, 0xf5]), false, None));
    assert_eq!((tables[1].fill, tables[1].radius, tables[1].snug), (Some([255, 255, 255]), 12, true));
}

#[test]
fn a_cell_padded_on_one_side_is_a_table_and_not_only_layout() {
    let doc = crate::html::parse(r#"<table width="100%"><tr><td style="padding:0;padding-top:12px;padding-right:24px">Steps</td></tr></table>"#);
    let Some(Block::Table(t)) = doc.blocks.iter().find(|b| matches!(b, Block::Table(_))) else { panic!("no table: {:?}", doc.blocks) };
    assert_eq!(t.rows[0][0].padding, Some([12, 24, 0, 0]));
}

/// Every run of text in blocks, tables and all, with whether it is a link and underlined.
fn links_of(blocks: &[Block], out: &mut Vec<(String, bool, bool)>) {
    for b in blocks {
        match b {
            Block::Para(p) => {
                out.extend(p.runs.iter().filter(|r| !r.text.trim().is_empty()).map(|r| (r.text.clone(), r.style.link.is_some(), r.style.underline)))
            }
            Block::Table(t) => t.rows.iter().flatten().for_each(|c| links_of(&c.blocks, out)),
            _ => {}
        }
    }
}

#[test]
fn a_word_signature_keeps_its_padding_in_centimetres_and_points_and_its_links_not_underlined() {
    // Written as Word writes a signature: lengths in cm and pt, and the underline turned off
    // on the text inside each link.
    let doc = parse(
        r#"<table border=0 cellspacing=0 cellpadding=0><tr><td style='padding:0cm 12.0pt 0cm 0cm'><p>Logo</p></td><td style='border:none;border-left:solid #E53935 2.25pt;padding:0cm 0cm 0cm 12.0pt'><p><a href="mailto:sam@example.test"><span style='color:#E53935;text-decoration:none'>sam@example.test</span></a></p><p><a href="https://example.test">example.test</a> <a href="https://example.test/b" style="text-decoration:none">B</a></p></td></tr></table>"#,
    );
    let Some(Block::Table(t)) = doc.blocks.iter().find(|b| matches!(b, Block::Table(_))) else { panic!("no table: {:?}", doc.blocks) };
    assert_eq!(t.rows[0][0].padding, Some([0, 16, 0, 0]));
    assert_eq!(t.rows[0][1].padding, Some([0, 0, 0, 16]));
    let mut runs = Vec::new();
    links_of(&doc.blocks, &mut runs);
    let get = |text: &str| runs.iter().find(|r| r.0 == text).map(|r| (r.1, r.2));
    assert_eq!(get("sam@example.test"), Some((true, false)), "{runs:?}");
    // A link left as it is is underlined, as in a browser.
    assert_eq!(get("example.test"), Some((true, true)), "{runs:?}");
    assert_eq!(get("B"), Some((true, false)), "{runs:?}");
}

#[test]
fn padding_is_read_in_every_absolute_unit_and_a_length_not_understood_is_no_padding() {
    let cell = |padding: &str| {
        let doc = parse(&format!(r#"<table width="100%" border="1"><tr><td style="padding:{padding}">x</td><td>y</td></tr></table>"#));
        match doc.blocks.iter().find(|b| matches!(b, Block::Table(_))) {
            Some(Block::Table(t)) => t.rows[0][0].padding,
            _ => panic!("no table"),
        }
    };
    assert_eq!(cell("0"), Some([0, 0, 0, 0]));
    // An inch is 96 pixels, more than a cell is given: 64 at most.
    assert_eq!(cell("1in 1pc 1cm 10mm"), Some([64, 16, 38, 38]));
    assert_eq!(cell("6PT"), Some([8, 8, 8, 8]));
    assert_eq!(cell("1em"), None);
    assert_eq!(cell("-3px"), None);
    assert_eq!(cell("1e30in"), None);
    assert_eq!(cell("99999999999999999999999999999999999999999in"), None);
    assert_eq!(cell("9999999in"), Some([64, 64, 64, 64]));
}
