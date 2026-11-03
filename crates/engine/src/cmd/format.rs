//! Format Text: the look of what is written in a message, as the compose window's Basic Text
//! and Format Text groups give it. Each works on the selection the writer last gave
//! (`compose.select`): with text selected it changes that text, and with none it sets the look
//! the next typing takes, as Outlook's buttons do. Pictures and pasted HTML come in through
//! [`openmailclient_formats::editable`], as inert as the reading pane shows them: nothing is
//! fetched, and a link is kept only to a web page, a mailbox or a telephone number.

use super::compose::{MAX_BODY, clamp};
use super::{CommandSpec, need_str, ok, p_str, p_u64};
use crate::{CmdResult, Draft, EngineError, Session};
use base64::Engine as _;
use openmailclient_formats::editable;
use openmailclient_model::format::{self, Align, Format, List, MAX_PICTURE_BYTES, Mark, PICTURE, Para, Picture, Rgb};
use serde_json::{Value, json};
use std::sync::Arc;

/// The sizes Grow Font and Shrink Font step through, in points.
const SIZES: [f32; 16] = [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0, 72.0];
/// The size of text that names none: what a new message is written in.
const OWN_SIZE: f32 = 11.0;
/// The most a pasted or inserted piece may hold, in characters, and the longest picture
/// dimension kept.
const MAX_PASTE: usize = 4 << 20;
const MAX_EXTENT: u64 = 8192;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "compose.select",
            label: "Select Text",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"from", "to"}: characters of the body, counted from 0; the stretch the next Format Text command works on; returns its look as compose.format does"#,
            enabled: composing,
            run: on_draft_select,
        },
        CommandSpec {
            id: "compose.format",
            label: "Font",
            tab: "Format Text",
            group: "Font",
            shortcut: "Ctrl+D",
            params: r##"{"bold", "italic", "underline", "strike": true, false or "toggle"; "font": a font's name or null for the message's own; "size": points, "grow", "shrink" or null; "color", "highlight": "#rrggbb" or null; "clear": true takes all of it off} on the selection, or for what is typed next when nothing is selected; with none of them, only tells; returns {"bold", "italic", "underline", "strike", "font", "size", "color", "highlight", "link", "align", "list", "indent"} of the selection"##,
            enabled: composing,
            run: on_draft_set_format,
        },
        CommandSpec {
            id: "compose.paragraph",
            label: "Paragraph",
            tab: "Format Text",
            group: "Paragraph",
            shortcut: "",
            params: r#"{"align": "left", "center", "right" or "justify"; "list": "bullet", "number" or "none" (the kind the paragraphs have already takes it off); "indent": "more" or "less"} for the paragraphs the selection is in; returns the look as compose.format does"#,
            enabled: composing,
            run: on_draft_paragraph,
        },
        CommandSpec {
            id: "compose.link",
            label: "Link",
            tab: "Insert",
            group: "Links",
            shortcut: "Ctrl+K",
            params: r#"{"address": a web page, a mailbox or a telephone number (www.… is taken as https, a@b as a mailbox), empty to take a link off; "text": what it shows in place of the selection, the address itself when nothing is selected}"#,
            enabled: composing,
            run: on_draft_link,
        },
        CommandSpec {
            id: "compose.insertPicture",
            label: "Pictures",
            tab: "Insert",
            group: "Illustrations",
            shortcut: "",
            params: r#"{"path": a picture file (PNG, JPEG, GIF, BMP, WebP) or "data": its bytes in base64; "name"; "width", "height": pixels it is shown at} in place of the selection; it is sent in the message, shown where it stands; returns {"id"}"#,
            enabled: composing,
            run: on_draft_insert_picture,
        },
        CommandSpec {
            id: "compose.paste",
            label: "Paste",
            tab: "Message",
            group: "Clipboard",
            shortcut: "Ctrl+V",
            params: r#"{"html": formatted text as copied from a page, a message or a signature, its pictures written in it; or "text"; or "image": a picture's bytes in base64} in place of the selection; nothing it names on a server is fetched; a message in plain text takes only the text; returns {"at": where the caret goes}"#,
            enabled: composing,
            run: on_draft_paste,
        },
        CommandSpec {
            id: "compose.textFormat",
            label: "Format",
            tab: "Format Text",
            group: "Format",
            shortcut: "",
            params: r#"{"format": "html" (or "rich", written as HTML) or "plain"}: plain text takes every look and picture off the body; returns {"format"}"#,
            enabled: composing,
            run: on_draft_text_format,
        },
    ]
}

fn composing(s: &Session) -> bool {
    s.compose.is_some()
}

/// Text being written with its look: the body of a message, or a signature in its editor. A
/// message's is lent out of its [`Draft`] for each command and given back after it.
#[derive(Clone, Debug, Default)]
pub struct Writing {
    pub body: String,
    pub format: Format,
    /// The selection, as characters of the body: where it was begun and where it is now.
    pub selection: (usize, usize),
    /// The look the next typing takes, where one was chosen with nothing selected.
    pub typing: Option<Mark>,
    /// Written in plain text: no look and no pictures.
    pub plain: bool,
    /// Something was changed.
    pub touched: bool,
}

impl Writing {
    pub fn new(body: String, format: Format) -> Self {
        Writing { body, format, ..Writing::default() }
    }

    /// The selection, its start first and held to the text.
    pub fn selected(&self) -> (usize, usize) {
        let n = self.body.chars().count();
        let (a, b) = (self.selection.0.min(n), self.selection.1.min(n));
        (a.min(b), a.max(b))
    }

    /// The text changed by the writer to `new`, the change beginning at `at` or after: what was
    /// typed takes the look chosen for typing, or that of the text before it.
    pub fn rewrite(&mut self, new: String, at: usize) {
        let typed = self.typing.take().unwrap_or_else(|| self.format.typing_at(at));
        self.format.follow_from(&self.body, &new, &typed, at);
        self.body = new;
        let n = self.body.chars().count();
        self.selection = (self.selection.0.min(n), self.selection.1.min(n));
        self.touched = true;
    }

    /// What is being written in the message `d`, as it is now: for the window to draw.
    pub fn of(d: &Draft) -> Writing {
        Writing { body: d.body.clone(), format: d.format.clone(), selection: d.selection, typing: d.typing.clone(), plain: d.plain, touched: false }
    }

    /// The look of what is selected, as compose.format returns it.
    pub fn look(&self) -> Value {
        look(self)
    }

    fn lent(d: &mut Draft) -> Writing {
        Writing {
            body: std::mem::take(&mut d.body),
            format: std::mem::take(&mut d.format),
            selection: d.selection,
            typing: d.typing.take(),
            plain: d.plain,
            touched: false,
        }
    }

    fn give_back(self, d: &mut Draft) {
        d.body = self.body;
        d.format = self.format;
        d.selection = self.selection;
        d.typing = self.typing;
        d.plain = self.plain;
        d.touched |= self.touched;
    }
}

/// Run a Format Text command on `w`, as on a message being written: for the signature editor,
/// whose text is not a message's.
pub fn write(w: &mut Writing, command: &str, p: &Value) -> CmdResult {
    let op: Op = match command {
        "compose.select" => select,
        "compose.format" => set_format,
        "compose.paragraph" => paragraph,
        "compose.link" => link,
        "compose.insertPicture" => insert_picture,
        "compose.paste" => paste,
        "compose.textFormat" => text_format,
        _ => return Err(EngineError::UnknownCommand(command.to_string())),
    };
    op(w, p)
}

type Op = fn(&mut Writing, &Value) -> CmdResult;

/// `op` on the message being written, `what` naming it where none is.
fn on_draft(s: &mut Session, p: &Value, what: &str, op: Op) -> CmdResult {
    let d = s.compose.as_mut().ok_or_else(|| EngineError::Disabled(what.into()))?;
    let mut w = Writing::lent(d);
    let done = op(&mut w, p);
    w.give_back(d);
    done
}

fn on_draft_select(s: &mut Session, p: &Value) -> CmdResult {
    on_draft(s, p, "Select Text", select)
}

fn on_draft_set_format(s: &mut Session, p: &Value) -> CmdResult {
    on_draft(s, p, "Font", set_format)
}

fn on_draft_paragraph(s: &mut Session, p: &Value) -> CmdResult {
    on_draft(s, p, "Paragraph", paragraph)
}

fn on_draft_link(s: &mut Session, p: &Value) -> CmdResult {
    on_draft(s, p, "Link", link)
}

fn on_draft_insert_picture(s: &mut Session, p: &Value) -> CmdResult {
    on_draft(s, p, "Pictures", insert_picture)
}

fn on_draft_paste(s: &mut Session, p: &Value) -> CmdResult {
    on_draft(s, p, "Paste", paste)
}

fn on_draft_text_format(s: &mut Session, p: &Value) -> CmdResult {
    on_draft(s, p, "Format", text_format)
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// The look of the selection, or of what is typed at the caret.
fn look(d: &Writing) -> Value {
    let (from, to) = d.selected();
    let f = &d.format;
    let mark = if from < to { f.mark_at(from) } else { d.typing.clone().unwrap_or_else(|| f.typing_at(from)) };
    let all = |test: fn(&Mark) -> bool| if from < to { f.all(from, to, test) } else { test(&mark) };
    let mut fit = f.clone();
    fit.fit(&d.body);
    let first = *Format::paras_of(&d.body, from, to).start();
    let para = fit.paras.get(first).copied().unwrap_or_default();
    json!({
        "bold": all(|m| m.bold),
        "italic": all(|m| m.italic),
        "underline": all(|m| m.underline),
        "strike": all(|m| m.strike),
        "font": mark.family.as_deref(),
        "size": mark.size,
        "color": mark.color.map(hex),
        "highlight": mark.highlight.map(hex),
        "link": mark.link.as_deref(),
        "align": match para.align { Align::Left => "left", Align::Center => "center", Align::Right => "right", Align::Justify => "justify" },
        "list": match para.list { List::None => "none", List::Bullet => "bullet", List::Number => "number" },
        "indent": para.indent,
        "plain": d.plain,
    })
}

fn select(d: &mut Writing, p: &Value) -> CmdResult {
    let n = d.body.chars().count();
    let at = |key: &str| p_u64(p, key).and_then(|v| usize::try_from(v).ok()).map(|v| v.min(n));
    let from = at("from").unwrap_or(0);
    let to = at("to").unwrap_or(from);
    if (from, to) != d.selection {
        // A look chosen for typing holds only where it was chosen.
        d.typing = None;
    }
    d.selection = (from, to);
    Ok(look(d))
}

/// A switch given as true, false or "toggle": what it is to become, `on_now` being whether it
/// is on over all of the selection.
fn switch(p: &Value, key: &str, on_now: bool) -> Result<Option<bool>, EngineError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(Value::String(t)) if t == "toggle" => Ok(Some(!on_now)),
        Some(_) => Err(EngineError::BadParams(format!("\"{key}\" is true, false or \"toggle\""))),
    }
}

/// A colour as "#rrggbb" (or without its "#"), or null for none.
fn colour(p: &Value, key: &str) -> Result<Option<Option<Rgb>>, EngineError> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(t)) => {
            let t = t.trim().trim_start_matches('#');
            let byte = |i: usize| t.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
            match (t.len(), byte(0), byte(2), byte(4)) {
                (6, Some(r), Some(g), Some(b)) => Ok(Some(Some([r, g, b]))),
                _ => Err(EngineError::BadParams(format!("\"{key}\" is a colour as #rrggbb"))),
            }
        }
        Some(_) => Err(EngineError::BadParams(format!("\"{key}\" is a colour as #rrggbb, or null"))),
    }
}

/// What "size" asks for: `Some(None)` the message's own.
enum Size {
    Keep,
    Set(Option<f32>),
    Grow,
    Shrink,
}

fn size(p: &Value) -> Result<Size, EngineError> {
    match p.get("size") {
        None => Ok(Size::Keep),
        Some(Value::Null) => Ok(Size::Set(None)),
        Some(Value::String(t)) if t == "grow" => Ok(Size::Grow),
        Some(Value::String(t)) if t == "shrink" => Ok(Size::Shrink),
        Some(v) => match v.as_f64() {
            #[allow(clippy::cast_possible_truncation)]
            Some(n) if n.is_finite() && n >= 1.0 && n <= f64::from(format::MAX_POINTS) => Ok(Size::Set(Some(n as f32))),
            _ => Err(EngineError::BadParams(format!("\"size\" is points from 1 to {}, \"grow\", \"shrink\" or null", format::MAX_POINTS))),
        },
    }
}

/// The next of the sizes up or down from `now`, as Grow Font and Shrink Font step.
fn stepped(now: Option<f32>, up: bool) -> Option<f32> {
    let now = now.unwrap_or(OWN_SIZE);
    let next = if up { SIZES.iter().copied().find(|s| *s > now) } else { SIZES.iter().rev().copied().find(|s| *s < now) };
    Some(next.unwrap_or(now))
}

/// The changes one compose.format asks for, made to a look.
struct Change {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    font: Option<Option<Arc<str>>>,
    size: Size,
    color: Option<Option<Rgb>>,
    highlight: Option<Option<Rgb>>,
    clear: bool,
}

impl Change {
    fn to(&self, m: &mut Mark) {
        if self.clear {
            *m = Mark { link: m.link.clone(), picture: m.picture, ..Mark::default() };
        }
        let set = |field: &mut bool, to: Option<bool>| {
            if let Some(v) = to {
                *field = v;
            }
        };
        set(&mut m.bold, self.bold);
        set(&mut m.italic, self.italic);
        set(&mut m.underline, self.underline);
        set(&mut m.strike, self.strike);
        if let Some(f) = &self.font {
            m.family = f.clone();
        }
        match self.size {
            Size::Keep => {}
            Size::Set(v) => m.size = v,
            Size::Grow => m.size = stepped(m.size, true),
            Size::Shrink => m.size = stepped(m.size, false),
        }
        if let Some(c) = self.color {
            m.color = c;
        }
        if let Some(c) = self.highlight {
            m.highlight = c;
        }
    }
}

fn set_format(d: &mut Writing, p: &Value) -> CmdResult {
    let (from, to) = d.selected();
    let typing = d.typing.clone().unwrap_or_else(|| d.format.typing_at(from));
    let on = |test: fn(&Mark) -> bool| if from < to { d.format.all(from, to, test) } else { test(&typing) };
    let font = match p.get("font") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(t)) if t.trim().is_empty() => Some(None),
        Some(Value::String(t)) => Some(Some(Arc::from(clamp(t.trim(), 400).as_str()))),
        Some(_) => return Err(EngineError::BadParams("\"font\" is a font's name, or null".into())),
    };
    let change = Change {
        bold: switch(p, "bold", on(|m| m.bold))?,
        italic: switch(p, "italic", on(|m| m.italic))?,
        underline: switch(p, "underline", on(|m| m.underline))?,
        strike: switch(p, "strike", on(|m| m.strike))?,
        font,
        size: size(p)?,
        color: colour(p, "color")?,
        highlight: colour(p, "highlight")?,
        clear: p.get("clear").and_then(Value::as_bool).unwrap_or(false),
    };
    let asked = change.bold.is_some()
        || change.italic.is_some()
        || change.underline.is_some()
        || change.strike.is_some()
        || change.font.is_some()
        || !matches!(change.size, Size::Keep)
        || change.color.is_some()
        || change.highlight.is_some()
        || change.clear;
    if asked {
        if from < to {
            d.format.fit(&d.body);
            d.format.apply(from, to, |m| change.to(m));
            d.touched = true;
        } else {
            let mut next = typing;
            change.to(&mut next);
            d.typing = Some(next.typing());
        }
    }
    Ok(look(d))
}

fn paragraph(d: &mut Writing, p: &Value) -> CmdResult {
    let (from, to) = d.selected();
    let align = match p_str(p, "align") {
        None => None,
        Some("left") => Some(Align::Left),
        Some("center") => Some(Align::Center),
        Some("right") => Some(Align::Right),
        Some("justify") => Some(Align::Justify),
        Some(_) => return Err(EngineError::BadParams("\"align\" is left, center, right or justify".into())),
    };
    let list = match p_str(p, "list") {
        None => None,
        Some("none") => Some(List::None),
        Some("bullet") => Some(List::Bullet),
        Some("number") => Some(List::Number),
        Some(_) => return Err(EngineError::BadParams("\"list\" is bullet, number or none".into())),
    };
    let indent = match p_str(p, "indent") {
        None => 0i8,
        Some("more") => 1,
        Some("less") => -1,
        Some(_) => return Err(EngineError::BadParams("\"indent\" is more or less".into())),
    };
    d.format.fit(&d.body);
    // Bullets pressed again over paragraphs that all have them take them off.
    let list = list.map(|kind| {
        let all = Format::paras_of(&d.body, from, to).all(|n| d.format.paras.get(n).is_some_and(|p| p.list == kind));
        if all && kind != List::None { List::None } else { kind }
    });
    d.format.set_paras(&d.body, from, to, |para: &mut Para| {
        if let Some(a) = align {
            para.align = a;
        }
        if let Some(l) = list {
            para.list = l;
        }
        para.indent = match indent {
            1 => para.indent.saturating_add(1),
            -1 => para.indent.saturating_sub(1),
            _ => para.indent,
        };
    });
    if align.is_some() || list.is_some() || indent != 0 {
        d.touched = true;
    }
    Ok(look(d))
}

/// The selection taken out of the body, the caret left where it was.
fn take_selection(d: &mut Writing) -> usize {
    let (from, to) = d.selected();
    if from < to {
        let new: String = d.body.chars().take(from).chain(d.body.chars().skip(to)).collect();
        d.format.follow_from(&d.body, &new, &Mark::default(), from);
        d.body = new;
    }
    d.selection = (from, from);
    from
}

/// `piece` in its look put in place of the selection; the caret goes after it.
fn put(d: &mut Writing, piece: &str, look: &Format) -> Result<usize, EngineError> {
    if piece.is_empty() {
        return Ok(d.selected().0);
    }
    let (from, to) = d.selected();
    let taken = d.body.chars().skip(from).take(to.saturating_sub(from)).map(char::len_utf8).sum::<usize>();
    if d.body.len().saturating_sub(taken).saturating_add(piece.len()) > MAX_BODY {
        return Err(EngineError::Format("the message cannot hold that much more".into()));
    }
    let at = take_selection(d);
    let mut body = std::mem::take(&mut d.body);
    let end = d.format.insert(&mut body, at, piece, look);
    d.body = body;
    d.selection = (end, end);
    d.typing = None;
    d.touched = true;
    Ok(end)
}

/// An address as a link takes it: a bare web address is a secure one, and an address with an
/// at sign and nothing before a colon is a mailbox.
fn link_address(address: &str) -> Option<String> {
    let a = address.trim();
    let lower = a.to_ascii_lowercase();
    let full = if lower.starts_with("www.") {
        format!("https://{a}")
    } else if a.contains('@') && !a.contains(':') && !a.contains('/') {
        format!("mailto:{a}")
    } else {
        a.to_string()
    };
    format::safe_link(&full).then_some(full)
}

fn link(d: &mut Writing, p: &Value) -> CmdResult {
    let address = need_str(p, "address")?.to_string();
    let (from, to) = d.selected();
    if address.trim().is_empty() {
        d.format.fit(&d.body);
        d.format.apply(from, to, |m| m.link = None);
        d.touched = true;
        return ok();
    }
    let target = link_address(&address)
        .ok_or_else(|| EngineError::BadParams("a link goes to a web page (http or https), a mailbox or a telephone number".into()))?;
    let text = p_str(p, "text").map(|t| t.replace(['\r', '\n'], " ")).filter(|t| !t.trim().is_empty());
    if from < to && text.is_none() {
        d.format.fit(&d.body);
        d.format.apply(from, to, |m| m.link = Some(Arc::from(target.as_str())));
        d.touched = true;
        return ok();
    }
    let shown = clamp(&text.unwrap_or_else(|| address.trim().to_string()), 2048);
    let mark = Mark { link: Some(Arc::from(target.as_str())), ..d.typing.clone().unwrap_or_else(|| d.format.typing_at(from)) };
    let piece = Format { runs: vec![(shown.chars().count(), mark)], paras: vec![Para::default()], pictures: Vec::new() };
    let end = put(d, &shown, &piece)?;
    Ok(json!({ "at": end }))
}

fn base64_bytes(p: &Value, key: &str) -> Result<Option<Vec<u8>>, EngineError> {
    let Some(text) = p_str(p, key) else { return Ok(None) };
    // Base64 is four characters for three bytes: a longer text is a larger picture than is taken.
    if text.len() / 4 * 3 > MAX_PICTURE_BYTES {
        return Err(EngineError::Format("that picture is too large to put in a message".into()));
    }
    let clean: String = text
        .trim()
        .trim_start_matches(|c| c != ',' && text.contains(";base64,"))
        .trim_start_matches(',')
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(clean.as_bytes())
        .map(Some)
        .map_err(|_| EngineError::BadParams(format!("\"{key}\" is not base64")))
}

fn extent(p: &Value, key: &str) -> u16 {
    u16::try_from(p_u64(p, key).unwrap_or(0).min(MAX_EXTENT)).unwrap_or(0)
}

/// A picture's bytes, made one to put in: only a picture of a kind that is shown and sent.
fn picture_of(bytes: Vec<u8>, name: Option<&str>, width: u16, height: u16) -> Result<Picture, EngineError> {
    if bytes.is_empty() || bytes.len() > MAX_PICTURE_BYTES {
        return Err(EngineError::Format("that picture is too large to put in a message".into()));
    }
    let kind =
        editable::sniff(&bytes).ok_or_else(|| EngineError::Format("that is not a picture this can show (PNG, JPEG, GIF, BMP or WebP)".into()))?;
    let name = name
        .map(|n| clamp(n.trim(), 255))
        .filter(|n| !n.is_empty() && !n.chars().any(|c| c.is_control() || matches!(c, '/' | '\\')))
        .unwrap_or_else(|| format!("image.{}", editable::extension(kind)));
    Ok(Picture { id: 0, bytes: Arc::from(bytes), kind: kind.to_string(), name, width, height })
}

/// A picture put in place of the selection; says its number.
fn put_picture(d: &mut Writing, picture: Picture) -> Result<u32, EngineError> {
    if d.plain {
        return Err(EngineError::Disabled("Pictures in a message written in plain text".into()));
    }
    if d.body.len().saturating_add(4) > MAX_BODY {
        return Err(EngineError::Format("the message cannot hold that much more".into()));
    }
    let at = take_selection(d);
    let mut body = std::mem::take(&mut d.body);
    let end = d.format.put_picture(&mut body, at, picture);
    d.body = body;
    let end = end.ok_or_else(|| EngineError::Format(format!("a message holds at most {} pictures", format::MAX_PICTURES)))?;
    d.selection = (end, end);
    d.touched = true;
    Ok(d.format.mark_at(at).picture.unwrap_or(0))
}

fn insert_picture(d: &mut Writing, p: &Value) -> CmdResult {
    let (bytes, file) = match (p_str(p, "path"), base64_bytes(p, "data")?) {
        (Some(path), _) => {
            let path = std::path::Path::new(path);
            let about = std::fs::metadata(path)?;
            if about.is_dir() || about.len() > MAX_PICTURE_BYTES as u64 {
                return Err(EngineError::Format("that is not a picture file of a size a message takes".into()));
            }
            (std::fs::read(path)?, path.file_name().map(|n| n.to_string_lossy().into_owned()))
        }
        (None, Some(bytes)) => (bytes, None),
        (None, None) => return Err(EngineError::BadParams("\"path\" or \"data\" is required".into())),
    };
    let name = p_str(p, "name").map(str::to_string).or(file);
    let picture = picture_of(bytes, name.as_deref(), extent(p, "width"), extent(p, "height"))?;
    let id = put_picture(d, picture)?;
    Ok(json!({ "id": id }))
}

fn paste(d: &mut Writing, p: &Value) -> CmdResult {
    if let Some(bytes) = base64_bytes(p, "image")? {
        let picture = picture_of(bytes, p_str(p, "name"), 0, 0)?;
        put_picture(d, picture)?;
        return Ok(json!({ "at": d.selected().0 }));
    }
    let (text, look) = match (p_str(p, "html"), p_str(p, "text")) {
        (Some(html), _) if !d.plain => {
            // Pasted HTML has no attachments to give pictures by content id.
            let (text, look) = editable::of_html(&clamp(html, MAX_PASTE.saturating_mul(4)), &|_| None);
            (text, look)
        }
        (Some(html), _) => {
            let (text, look) = editable::of_html(&clamp(html, MAX_PASTE.saturating_mul(4)), &|_| None);
            plain_piece(&look.to_plain(&text), Mark::default())
        }
        (None, Some(text)) => {
            let typing = d.typing.clone().unwrap_or_else(|| d.format.typing_at(d.selected().0));
            plain_piece(text, typing)
        }
        (None, None) => return Err(EngineError::BadParams("\"html\", \"text\" or \"image\" is required".into())),
    };
    let at = put(d, &text, &look)?;
    Ok(json!({ "at": at }))
}

/// Text to paste as it is, in one look: line ends made line feeds, and no picture's character.
fn plain_piece(text: &str, mark: Mark) -> (String, Format) {
    let text: String = clamp(text, MAX_PASTE.saturating_mul(4))
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|c| *c != PICTURE && (*c == '\n' || *c == '\t' || !c.is_control()))
        .take(MAX_PASTE)
        .collect();
    let mut look = Format { runs: vec![(text.chars().count(), mark.typing())], paras: Vec::new(), pictures: Vec::new() };
    look.fit(&text);
    (text, look)
}

fn text_format(d: &mut Writing, p: &Value) -> CmdResult {
    let wanted = need_str(p, "format")?.to_ascii_lowercase();
    match wanted.as_str() {
        "html" | "rich" => d.plain = false,
        "plain" => {
            // As in Outlook, plain text keeps the words and lets go of everything else.
            let text = d.format.to_plain(&d.body);
            d.format = Format::default();
            d.format.fit(&text);
            d.body = text;
            d.typing = None;
            let n = d.body.chars().count();
            d.selection = (d.selection.0.min(n), d.selection.1.min(n));
            d.plain = true;
        }
        _ => return Err(EngineError::BadParams("\"format\" is html, rich or plain".into())),
    }
    d.touched = true;
    Ok(json!({ "format": if d.plain { "plain" } else { "html" } }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_become_links_only_to_safe_places() {
        assert_eq!(link_address("www.example.org").as_deref(), Some("https://www.example.org"));
        assert_eq!(link_address("pat@example.org").as_deref(), Some("mailto:pat@example.org"));
        assert_eq!(link_address("javascript:alert(1)"), None);
        assert_eq!(link_address("file:///c:/x"), None);
    }

    #[test]
    fn sizes_step_as_grow_and_shrink_font() {
        assert_eq!(stepped(None, true), Some(12.0));
        assert_eq!(stepped(Some(12.0), false), Some(11.0));
        assert_eq!(stepped(Some(72.0), true), Some(72.0));
        assert_eq!(stepped(Some(13.0), true), Some(14.0));
    }
}
