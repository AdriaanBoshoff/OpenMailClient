//! SVG pictures, drawn to pixels.
//!
//! A drawing in a message is hostile input like the rest of it. Nothing in one is run: there
//! is no script engine here. Nothing is read from disk or fetched for one: a picture a drawing
//! refers to, by address or written into it, is left out. Its text is set in the fonts
//! installed on this machine. The reader and the painter are not ours (`resvg`), so a drawing
//! is drawn on a thread of its own, with room for the deepest drawing the reader takes, and is
//! given up when it takes too long; the picture then stays a placeholder.
//!
//! In a browser (M6.6) there is no thread to draw on and a panic cannot be caught, so a
//! drawing is first looked over (`shallow`): one nested deeper than [`MAX_WEB_DEPTH`], or with
//! more than [`MAX_WEB_USES`] references to its own parts, is not drawn there, and what is
//! drawn is kept smaller. The page's stack is made large enough for what passes
//! (`.cargo/config.toml`).
//!
//! A compressed drawing (`.svgz`, gzip as RFC 1952 has it) is unpacked first, to at most
//! [`MAX_FILE`] bytes, so that a small file cannot become a large one in memory; one written in
//! UTF-16 is read as UTF-8.

use std::borrow::Cow;

use crate::pictures::Pixels;

/// Largest drawing that is read.
const MAX_FILE: usize = 4 << 20;
/// How far into a file the opening `<svg` is looked for.
const MAX_PROLOGUE: usize = 4096;

/// Whether `data` begins as an SVG file does: XML with an `svg` element near its top, in UTF-8
/// or UTF-16, or compressed.
pub(crate) fn looks_like(data: &[u8]) -> bool {
    if gzipped(data) {
        return true;
    }
    if let Some(text) = utf16(data, MAX_PROLOGUE) {
        return plain_looks_like(text.as_bytes());
    }
    plain_looks_like(data)
}

/// Whether UTF-8 `data` begins as an SVG file does.
fn plain_looks_like(data: &[u8]) -> bool {
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    let from = data.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(data.len());
    let top = data.get(from..data.len().min(from.saturating_add(MAX_PROLOGUE))).unwrap_or(&[]);
    top.first() == Some(&b'<') && top.windows(4).any(|w| w == b"<svg")
}

/// An SVG file → pixels. `None` for one that is over the limits, cannot be read, draws nothing,
/// or takes too long.
pub(crate) fn decode(data: &[u8]) -> Option<Pixels> {
    if data.len() > MAX_FILE {
        return None;
    }
    let data = plain(data)?;
    if !plain_looks_like(&data) {
        return None;
    }
    drawn(&data)
}

/// `data` as UTF-8 XML: unpacked when compressed, read as UTF-8 when written in UTF-16. `None`
/// when it cannot be, or comes to more than [`MAX_FILE`].
pub(crate) fn plain(data: &[u8]) -> Option<Cow<'_, [u8]>> {
    if gzipped(data) {
        let unpacked = gunzip(data)?;
        // A drawing compressed twice is not unpacked twice.
        if gzipped(&unpacked) {
            return None;
        }
        return Some(Cow::Owned(match utf16(&unpacked, MAX_FILE) {
            Some(text) => text.into_bytes(),
            None => unpacked,
        }));
    }
    match utf16(data, MAX_FILE) {
        Some(text) => (text.len() <= MAX_FILE).then(|| Cow::Owned(text.into_bytes())),
        None => Some(Cow::Borrowed(data)),
    }
}

fn gzipped(data: &[u8]) -> bool {
    data.starts_with(&[0x1f, 0x8b, 8])
}

/// The deflated data inside a gzip member, unpacked to at most [`MAX_FILE`] bytes (RFC 1952
/// section 2.3: a ten-byte header, then what its flags say follows).
fn gunzip(data: &[u8]) -> Option<Vec<u8>> {
    let flags = *data.get(3)?;
    let mut at = 10usize;
    if flags & 0x04 != 0 {
        // FEXTRA: two bytes of length, then that many.
        let len = u16::from_le_bytes([*data.get(at)?, *data.get(at.checked_add(1)?)?]);
        at = at.checked_add(2)?.checked_add(usize::from(len))?;
    }
    for flag in [0x08u8, 0x10] {
        // FNAME and FCOMMENT: text ended by a zero byte.
        if flags & flag != 0 {
            let end = data.get(at..)?.iter().position(|b| *b == 0)?;
            at = at.checked_add(end)?.checked_add(1)?;
        }
    }
    if flags & 0x02 != 0 {
        // FHCRC: two bytes.
        at = at.checked_add(2)?;
    }
    miniz_oxide::inflate::decompress_to_vec_with_limit(data.get(at..)?, MAX_FILE).ok()
}

/// UTF-16 text, as its byte-order mark says, read into UTF-8 up to `max` bytes of it; `None`
/// for text with no such mark. What is not UTF-16 becomes U+FFFD.
fn utf16(data: &[u8], max: usize) -> Option<String> {
    let big = if data.starts_with(&[0xFE, 0xFF]) {
        true
    } else if data.starts_with(&[0xFF, 0xFE]) {
        false
    } else {
        return None;
    };
    let units = data.get(2..)?.chunks_exact(2).map(|p| match p {
        [a, b] if big => u16::from_be_bytes([*a, *b]),
        [a, b] => u16::from_le_bytes([*a, *b]),
        _ => 0xFFFD,
    });
    let mut text = String::new();
    for c in char::decode_utf16(units) {
        let c = c.unwrap_or(char::REPLACEMENT_CHARACTER);
        if text.len().saturating_add(c.len_utf8()) > max {
            break;
        }
        text.push(c);
    }
    Some(text)
}

/// Elements inside one another a drawing may have to be drawn in a browser.
pub(crate) const MAX_WEB_DEPTH: usize = 48;
/// `<use>` elements, and references into the drawing, a drawing may have to be drawn in a
/// browser: each may lead the reader deeper.
pub(crate) const MAX_WEB_USES: usize = 64;
/// Longest side of a drawing kept in a browser, where it is drawn in place.
#[cfg(target_arch = "wasm32")]
const MAX_WEB_KEPT_SIDE: f32 = 1024.0;

/// In the web build, in place, once the drawing was looked over.
#[cfg(target_arch = "wasm32")]
fn drawn(data: &[u8]) -> Option<Pixels> {
    if !shallow(data) {
        return None;
    }
    let fonts = crate::pictures::lettering().library().shared();
    draw::draw(data, fonts, MAX_WEB_KEPT_SIDE)
}

/// Whether a drawing is nested no deeper than [`MAX_WEB_DEPTH`] and refers to its own parts at
/// most [`MAX_WEB_USES`] times, read as XML is read, without building anything. What cannot be
/// made out is not shallow. Comments, processing instructions, `CDATA` and quoted attribute
/// values are stepped over; a document type is not shallow, as entities can stand for more.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn shallow(data: &[u8]) -> bool {
    let (mut depth, mut uses, mut i) = (0usize, 0usize, 0usize);
    let at = |i: usize| data.get(i).copied();
    let starts = |i: usize, s: &[u8]| data.get(i..).is_some_and(|d| d.starts_with(s));
    let find = |from: usize, s: &[u8]| data.get(from..).and_then(|d| d.windows(s.len()).position(|w| w == s)).map(|p| p.saturating_add(from));
    while let Some(lt) = data.get(i..).and_then(|d| d.iter().position(|b| *b == b'<')).map(|p| p.saturating_add(i)) {
        let after = lt.saturating_add(1);
        if starts(after, b"!--") {
            let Some(end) = find(after, b"-->") else { return false };
            i = end.saturating_add(3);
            continue;
        }
        if starts(after, b"![CDATA[") {
            let Some(end) = find(after, b"]]>") else { return false };
            i = end.saturating_add(3);
            continue;
        }
        if starts(after, b"!") {
            return false;
        }
        if starts(after, b"?") {
            let Some(end) = find(after, b"?>") else { return false };
            i = end.saturating_add(2);
            continue;
        }
        let closing = at(after) == Some(b'/');
        if starts(after, b"use") || starts(after, b"svg:use") {
            uses = uses.saturating_add(1);
        }
        // To the end of the tag, over quoted values.
        let mut j = after;
        let mut quote: Option<u8> = None;
        let empty;
        loop {
            let Some(b) = at(j) else { return false };
            match quote {
                Some(q) if b == q => quote = None,
                Some(_) => {}
                None if b == b'"' || b == QUOTE => quote = Some(b),
                None if b == b'>' => {
                    empty = j > after && at(j.saturating_sub(1)) == Some(b'/');
                    break;
                }
                None => {}
            }
            // A reference inside a value, `href="#a"` or `url(#a)`, may lead the reader deeper.
            let opens_with_hash = quote.is_some() && at(j.saturating_add(1)) == Some(b'#') && matches!(at(j), Some(b'"' | QUOTE));
            if opens_with_hash || starts(j, b"url(#") {
                uses = uses.saturating_add(1);
            }
            j = j.saturating_add(1);
        }
        if closing {
            depth = depth.saturating_sub(1);
        } else if !empty {
            depth = depth.saturating_add(1);
        }
        if depth > MAX_WEB_DEPTH || uses > MAX_WEB_USES {
            return false;
        }
        i = j.saturating_add(1);
    }
    true
}

/// A single quote, as a byte.
const QUOTE: u8 = 0x27;

#[cfg(not(target_arch = "wasm32"))]
fn drawn(data: &[u8]) -> Option<Pixels> {
    native::drawn(data)
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    use crate::pictures::Pixels;

    /// How long one drawing may take.
    const MAX_WAIT: Duration = Duration::from_secs(3);
    /// Room for the reader's deepest nesting (it stops at 1024 elements inside one another).
    const STACK: usize = 64 << 20;
    /// Drawings given up that may still be running. With this many, no other is begun.
    const MAX_LEFT: usize = 2;
    /// The longest side kept.
    const MAX_KEPT_SIDE: f32 = 2048.0;

    static LEFT: AtomicUsize = AtomicUsize::new(0);

    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Running,
        Done,
        GivenUp,
    }

    pub(super) fn drawn(data: &[u8]) -> Option<Pixels> {
        if LEFT.load(Ordering::SeqCst) >= MAX_LEFT {
            return None;
        }
        let data = data.to_vec();
        let fonts = crate::pictures::lettering().library().shared();
        let state = Arc::new(Mutex::new(State::Running));
        let theirs = Arc::clone(&state);
        let (send, receive) = mpsc::channel();
        std::thread::Builder::new()
            .name("svg".into())
            .stack_size(STACK)
            .spawn(move || {
                let pixels = catch_unwind(AssertUnwindSafe(|| super::draw::draw(&data, fonts, MAX_KEPT_SIDE))).ok().flatten();
                let _ = send.send(pixels);
                if let Ok(mut state) = theirs.lock() {
                    if *state == State::GivenUp {
                        LEFT.fetch_sub(1, Ordering::SeqCst);
                    }
                    *state = State::Done;
                }
            })
            .ok()?;
        match receive.recv_timeout(MAX_WAIT) {
            Ok(pixels) => pixels,
            Err(_) => {
                let mut state = state.lock().ok()?;
                if *state == State::Running {
                    *state = State::GivenUp;
                    LEFT.fetch_add(1, Ordering::SeqCst);
                }
                None
            }
        }
    }
}

/// Reading and painting a drawing, wherever that is done.
mod draw {
    use std::sync::Arc;

    use resvg::{tiny_skia, usvg};

    use crate::pictures::Pixels;

    /// Largest width or height a drawing is said to have.
    const MAX_SIDE: f32 = 8192.0;
    /// A drawing is kept this many times as large as it is, where that fits: it has no pixels
    /// of its own, and is then sharp on a dense screen and on paper.
    const SHARP: f32 = 2.0;

    /// `data` read and painted, its longest side at most `max_kept` pixels.
    pub(super) fn draw(data: &[u8], fonts: Arc<usvg::fontdb::Database>, max_kept: f32) -> Option<Pixels> {
        let options = usvg::Options {
            resources_dir: None,
            fontdb: fonts,
            // No picture is read for a drawing, from a file, an address or its own text.
            image_href_resolver: usvg::ImageHrefResolver { resolve_data: Box::new(|_, _, _| None), resolve_string: Box::new(|_, _| None) },
            ..usvg::Options::default()
        };
        let tree = usvg::Tree::from_data(data, &options).ok()?;
        if !tree.root().has_children() {
            return None;
        }
        let (w, h) = (tree.size().width(), tree.size().height());
        if !(w.is_finite() && h.is_finite()) || w < 0.5 || h < 0.5 {
            return None;
        }
        let longer = w.max(h);
        let said = (MAX_SIDE / longer).min(1.0);
        let natural = (((w * said).ceil() as u16).max(1), ((h * said).ceil() as u16).max(1));
        let by = (longer * SHARP).clamp(1.0, max_kept) / longer;
        let side = |n: f32| ((n * by).ceil() as u32).clamp(1, max_kept as u32);
        let mut page = tiny_skia::Pixmap::new(side(w), side(h))?;
        resvg::render(&tree, tiny_skia::Transform::from_scale(by, by), &mut page.as_mut());
        // The painter keeps colour already multiplied by cover; pictures here are kept plain.
        let mut rgba = Vec::with_capacity(page.pixels().len().saturating_mul(4));
        for pixel in page.pixels() {
            let plain = pixel.demultiply();
            rgba.extend_from_slice(&[plain.red(), plain.green(), plain.blue(), plain.alpha()]);
        }
        Some(Pixels { width: page.width(), height: page.height(), rgba, natural })
    }
}

#[cfg(test)]
mod tests_web {
    use super::*;

    fn nested(n: usize) -> Vec<u8> {
        let mut s = String::from("<svg xmlns='http://www.w3.org/2000/svg'>");
        s.push_str(&"<g>".repeat(n));
        s.push_str("<rect width='4' height='4'/>");
        s.push_str(&"</g>".repeat(n));
        s.push_str("</svg>");
        s.into_bytes()
    }

    #[test]
    fn a_drawing_is_looked_over_before_it_is_drawn_in_a_browser() {
        assert!(shallow(&nested(10)));
        assert!(shallow(&nested(MAX_WEB_DEPTH - 1)));
        assert!(!shallow(&nested(MAX_WEB_DEPTH + 1)));
        // Siblings are not depth, and empty elements close themselves.
        let wide = format!("<svg>{}</svg>", "<rect width='1' height='1'/><g></g>".repeat(500));
        assert!(shallow(wide.as_bytes()));
        // What looks like a tag inside a value, a comment or CDATA is not one.
        let hidden =
            format!("<svg><desc a='{}'/><!-- {} --><style><![CDATA[ {} ]]></style></svg>", "<g>".repeat(99), "<g>".repeat(99), "<g>".repeat(99));
        assert!(shallow(hidden.as_bytes()));
        // References into the drawing are counted, by <use>, href and url().
        let uses = format!("<svg>{}</svg>", "<use href='#a'/>".repeat(MAX_WEB_USES));
        assert!(!shallow(uses.as_bytes()));
        let fills = format!("<svg>{}</svg>", "<rect fill='url(#p)'/>".repeat(MAX_WEB_USES + 1));
        assert!(!shallow(fills.as_bytes()));
        assert!(shallow(b"<svg><rect fill='url(#p)'/><use href='#r'/></svg>"));
        // A document type, and what ends early, are not looked into.
        assert!(!shallow(b"<!DOCTYPE svg [<!ENTITY a 'b'>]><svg>&a;</svg>"));
        assert!(!shallow(b"<svg><g a='"));
        assert!(!shallow(b"<svg><!-- "));
        assert!(shallow(b""));
    }
}

#[cfg(test)]
mod tests_plain {
    use super::*;

    const DRAWING: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='8' height='6'><rect width='8' height='6' fill='#c00'/></svg>";

    /// A gzip member around `data`, with a name in its header when `named`.
    fn gzip(data: &[u8], named: bool) -> Vec<u8> {
        let mut out = vec![0x1f, 0x8b, 8, if named { 0x08 } else { 0 }, 0, 0, 0, 0, 0, 3];
        if named {
            out.extend_from_slice(b"drawing.svg\0");
        }
        out.extend(miniz_oxide::deflate::compress_to_vec(data, 6));
        // The CRC and length after it are not looked at.
        out.extend_from_slice(&[0; 8]);
        out
    }

    fn utf16(text: &str, big: bool) -> Vec<u8> {
        let mut out = if big { vec![0xFE, 0xFF] } else { vec![0xFF, 0xFE] };
        for unit in text.encode_utf16() {
            out.extend(if big { unit.to_be_bytes() } else { unit.to_le_bytes() });
        }
        out
    }

    #[test]
    fn compressed_and_utf16_drawings_are_drawn() {
        let plain = decode(DRAWING.as_bytes()).map(|p| (p.width, p.height));
        assert!(plain.is_some());
        for data in [gzip(DRAWING.as_bytes(), false), gzip(DRAWING.as_bytes(), true), utf16(DRAWING, false), utf16(DRAWING, true)] {
            assert!(looks_like(&data));
            assert_eq!(decode(&data).map(|p| (p.width, p.height)), plain);
        }
        // With the declaration UTF-16 files carry, which is then not what they are.
        let declared = format!("<?xml version='1.0' encoding='UTF-16'?>{DRAWING}");
        assert_eq!(decode(&utf16(&declared, false)).map(|p| (p.width, p.height)), plain);
        // Compressed UTF-16, too.
        assert_eq!(decode(&gzip(&utf16(DRAWING, false), false)).map(|p| (p.width, p.height)), plain);
    }

    #[test]
    fn a_compressed_drawing_that_unpacks_too_large_or_twice_is_not_drawn() {
        // A small file that would become more than the limit.
        let mut huge = String::from("<svg xmlns='http://www.w3.org/2000/svg' width='8' height='6'>");
        huge.push_str(&" ".repeat(MAX_FILE + 1));
        huge.push_str("</svg>");
        let bomb = gzip(huge.as_bytes(), false);
        assert!(bomb.len() < 64 << 10, "{}", bomb.len());
        assert!(plain(&bomb).is_none());
        assert!(decode(&bomb).is_none());
        assert!(decode(&gzip(&gzip(DRAWING.as_bytes(), false), false)).is_none());
        // Headers that end early or point past the end; deflate that is not.
        for data in [
            &[0x1f, 0x8b, 8][..],
            &[0x1f, 0x8b, 8, 0x04, 0, 0, 0, 0, 0, 0, 0xff, 0xff],
            &[0x1f, 0x8b, 8, 0x08, 0, 0, 0, 0, 0, 0, b'a'],
            &[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff],
        ] {
            assert!(decode(data).is_none());
        }
        // UTF-16 that is cut in a pair, or not a drawing.
        let mut cut = utf16(DRAWING, false);
        cut.pop();
        let _ = decode(&cut);
        assert!(!looks_like(&utf16("hello", true)));
        assert!(decode(&[0xFF, 0xFE, 0x00, 0xD8]).is_none());
    }
}
