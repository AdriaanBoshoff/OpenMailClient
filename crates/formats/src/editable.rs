//! A formatted document read for writing in: its text and its [`Format`], as a compose window
//! or the signature editor keeps them. HTML pasted from elsewhere, a draft kept as HTML and an
//! Outlook signature (`.htm`) come in this way, read by [`crate::html::parse`] first, so what
//! is read is as inert as anything the reading pane shows.
//!
//! What writing in keeps is less than what reading shows: each paragraph is a line of text with
//! its alignment, its list and its indent, and each stretch of a line keeps its look and its
//! link. A table's cells come in as paragraphs one after another, a rule as an empty line, and
//! a picture comes in only where its bytes are in hand: written into the body (RFC 2397), or an
//! attachment of the message that `attached` gives by its content id (RFC 2392). A picture on
//! a server is left out, never fetched; its alternative text stands in its place.

use crate::rich::{self, Block, Doc, ParaKind, Picture as Source};
use openmailclient_model::format::{self, Align, Format, List, MAX_INDENT, Mark, PICTURE, Para, Picture};
use std::sync::Arc;

/// The most characters taken in; what is past them is left out.
pub const MAX_CHARS: usize = 4 << 20;

/// A picture's bytes, its media type and its file name, given by its content id.
pub type Attached<'a> = &'a dyn Fn(&str) -> Option<(Arc<[u8]>, String, String)>;

/// What is written in so far.
struct Out {
    text: String,
    chars: usize,
    runs: Vec<(usize, Mark)>,
    paras: Vec<Para>,
    pictures: Vec<Picture>,
    /// A paragraph has been begun: the next one needs a line feed before it.
    begun: bool,
}

impl Out {
    fn full(&self) -> bool {
        self.chars >= MAX_CHARS
    }

    fn put(&mut self, s: &str, mark: &Mark) {
        let room = MAX_CHARS.saturating_sub(self.chars);
        // A no-break space is how HTML keeps blanks from being run together; written on, it is
        // a blank like any other, and is written so again where it is needed.
        let piece: String =
            s.chars().filter(|c| *c != PICTURE && (*c == '\t' || !c.is_control())).map(|c| if c == '\u{a0}' { ' ' } else { c }).take(room).collect();
        let n = piece.chars().count();
        if n > 0 {
            self.text.push_str(&piece);
            self.chars += n;
            self.runs.push((n, mark.clone()));
        }
    }

    /// A new paragraph of the look `para`.
    fn paragraph(&mut self, para: Para) {
        if self.full() {
            return;
        }
        if self.begun {
            self.text.push('\n');
            self.chars += 1;
            self.runs.push((1, Mark::default()));
            self.paras.push(para);
        } else if let Some(first) = self.paras.first_mut() {
            *first = para;
        } else {
            self.paras.push(para);
        }
        self.begun = true;
    }

    fn picture(&mut self, bytes: Arc<[u8]>, kind: String, name: String, extent: (u16, u16), mark: &Mark) -> bool {
        if self.full() || self.pictures.len() >= format::MAX_PICTURES || bytes.is_empty() || bytes.len() > format::MAX_PICTURE_BYTES {
            return false;
        }
        let id = u32::try_from(self.pictures.len()).unwrap_or(u32::MAX).saturating_add(1);
        self.pictures.push(Picture { id, bytes, kind, name, width: extent.0, height: extent.1 });
        self.text.push(PICTURE);
        self.chars += 1;
        self.runs.push((1, Mark { picture: Some(id), link: mark.link.clone(), ..Mark::default() }));
        true
    }
}

/// The text and formatting of `doc`, its pictures with them.
pub fn editable(doc: &Doc, attached: Attached) -> (String, Format) {
    let mut out = Out { text: String::new(), chars: 0, runs: Vec::new(), paras: vec![Para::default()], pictures: Vec::new(), begun: false };
    blocks(&mut out, doc, &doc.blocks, attached, 0);
    let mut f = Format { runs: out.runs, paras: out.paras, pictures: out.pictures };
    f.fit(&out.text);
    (out.text, f)
}

/// HTML read for writing in.
pub fn of_html(html: &str, attached: Attached) -> (String, Format) {
    editable(&crate::html::parse(html), attached)
}

fn blocks(out: &mut Out, doc: &Doc, list: &[Block], attached: Attached, depth: usize) {
    // Tables inside tables are read only so deep.
    if depth > 8 {
        return;
    }
    for block in list {
        if out.full() {
            return;
        }
        match block {
            Block::Para(p) => para(out, doc, p, attached),
            Block::Rule => out.paragraph(Para::default()),
            Block::Table(t) => {
                for row in &t.rows {
                    for cell in row {
                        blocks(out, doc, &cell.blocks, attached, depth + 1);
                    }
                }
            }
        }
    }
}

fn para(out: &mut Out, doc: &Doc, p: &rich::Para, attached: Attached) {
    let align = match p.align {
        rich::Align::Left => Align::Left,
        rich::Align::Center => Align::Center,
        rich::Align::Right => Align::Right,
    };
    let (list, indent) = match &p.kind {
        ParaKind::Item(marker) => {
            let numbered = marker.trim_end().ends_with('.') || marker.trim_end().ends_with(')');
            (if numbered { List::Number } else { List::Bullet }, p.indent.saturating_sub(1))
        }
        _ => (List::None, p.indent.saturating_add(p.quote)),
    };
    let look = Para { align, list, indent: indent.min(MAX_INDENT) };
    // What comes after a line break in a list item goes on under it, unmarked.
    let after_break = Para { list: List::None, indent: if list == List::None { look.indent } else { (look.indent + 1).min(MAX_INDENT) }, ..look };
    out.paragraph(look);
    let base = match p.kind {
        ParaKind::Heading(level) => Mark { bold: true, size: Some(heading_size(level)), ..Mark::default() },
        ParaKind::Pre => Mark { family: Some(Arc::from("Courier New")), ..Mark::default() },
        _ => Mark::default(),
    };
    for run in &p.runs {
        if out.full() {
            return;
        }
        let mark = mark_of(&run.style, &base);
        if let Some(source) = &run.picture {
            let found = match source {
                Source::Inline(Some(i)) => doc.written.get(*i).map(|b| (b.clone(), String::new(), String::new())),
                Source::Attached(cid) => attached(cid.trim_start_matches('<').trim_end_matches('>')),
                _ => None,
            };
            let put = found.and_then(|(bytes, kind, name)| {
                let kind = if kind.starts_with("image/") { kind } else { sniff(&bytes)?.to_string() };
                let name = if name.trim().is_empty() { format!("image{:03}.{}", out.pictures.len() + 1, extension(&kind)) } else { name };
                Some(out.picture(bytes, kind, name, run.extent, &mark))
            });
            if put != Some(true) && !run.text.trim().is_empty() {
                out.put(&run.text, &mark);
            }
            continue;
        }
        let mut lines = run.text.split('\n');
        if let Some(first) = lines.next() {
            out.put(first, &mark);
        }
        for line in lines {
            out.paragraph(after_break);
            out.put(line, &mark);
        }
    }
}

fn heading_size(level: u8) -> f32 {
    match level {
        1 => 24.0,
        2 => 18.0,
        3 => 14.0,
        _ => 12.0,
    }
}

/// A look read from a body's style, over `base`, a heading's or the like.
fn mark_of(style: &rich::Style, base: &Mark) -> Mark {
    // Only the first of the fonts a body lists is kept, without its quotes.
    let family = style
        .family
        .as_deref()
        .and_then(|f| f.split(',').next())
        .map(|f| f.trim().trim_matches(['"', '\'']).trim())
        .filter(|f| !f.is_empty())
        .map(Arc::from)
        .or_else(|| style.mono.then(|| Arc::from("Courier New")))
        .or_else(|| base.family.clone());
    Mark {
        bold: style.bold || base.bold,
        italic: style.italic,
        underline: style.underline && style.link.is_none(),
        strike: style.strike,
        family,
        size: style.size.or(base.size),
        color: style.color,
        highlight: style.background,
        link: style.link.clone().filter(|l| format::safe_link(l)),
        picture: None,
    }
}

/// The media type of a picture file, known by its first bytes; `None` for what is not one we
/// can show and send.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    let starts = |magic: &[u8]| bytes.starts_with(magic);
    if starts(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if starts(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        Some("image/gif")
    } else if starts(b"BM") {
        Some("image/bmp")
    } else if bytes.len() >= 12 && starts(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

/// The file name ending a media type is written with.
pub fn extension(kind: &str) -> &'static str {
    match kind {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/webp" => "webp",
        _ => "png",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn a_signature_comes_in_with_its_look_and_pictures() {
        let html = r#"<p style="margin:0"><b><span style="font-family:'Segoe UI',sans-serif;font-size:14pt;color:#1F3864">Pat Lee</span></b><br>Support lead</p>
            <p style="margin:0;text-align:center"><a href="https://example.org">example.org</a> <img src="cid:logo@01" width="80" height="20" alt="logo"></p>
            <p><img src="https://tracker.example/x.png" alt="far"></p>
            <ul><li>one</li><li>two</li></ul>"#;
        let attached = |cid: &str| (cid == "logo@01").then(|| (Arc::from(PNG), "image/png".to_string(), "logo.png".to_string()));
        let (text, f) = of_html(html, &attached);
        assert_eq!(text, "Pat Lee\nSupport lead\nexample.org \u{FFFC}\nfar\none\ntwo");
        let name = f.mark_at(0);
        assert!(name.bold && name.size == Some(14.0) && name.color == Some([0x1f, 0x38, 0x64]));
        assert!(name.family.as_deref().is_some_and(|f| f.eq_ignore_ascii_case("Segoe UI")));
        assert_eq!(f.paras[2].align, Align::Center);
        assert_eq!(f.mark_at(21).link.as_deref(), Some("https://example.org"));
        assert_eq!(f.pictures.len(), 1);
        assert_eq!((f.pictures[0].width, f.pictures[0].height, f.pictures[0].name.as_str()), (80, 20, "logo.png"));
        assert_eq!(f.paras[4].list, List::Bullet);
        // Written out again, the picture goes by its content id.
        let html = f.to_html(&text, &Mark::default(), |p| format!("cid:p{}", p.id));
        assert!(html.contains("cid:p1") && !html.contains("tracker.example"), "{html}");
    }

    #[test]
    fn pictures_written_in_the_body_come_in() {
        let (text, f) = of_html(r#"<p>a<img src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==">b</p>"#, &|_| None);
        assert_eq!(text, "a\u{FFFC}b");
        assert_eq!(f.pictures.first().map(|p| p.kind.as_str()), Some("image/png"));
    }

    #[test]
    fn junk_comes_in_without_harm() {
        let deep = "<table><tr><td>".repeat(200) + "x";
        for html in ["", "<", "<p>\u{FFFC}\u{0}\u{7}</p>", "<img src=cid:>", deep.as_str(), "<a href=javascript:alert(1)>x</a>"] {
            let (text, f) = of_html(html, &|_| None);
            assert_eq!(f.chars(), text.chars().count());
            assert!(!text.contains('\u{FFFC}') || !f.pictures.is_empty());
            assert!(f.runs.iter().all(|(_, m)| m.link.as_deref().is_none_or(format::safe_link)));
        }
    }
}
