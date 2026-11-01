//! Attachment Preview: an attachment shown in the reading pane on a click, read here and
//! never handed to another program. What it is is told from its bytes before its name, as a
//! name may lie: a picture by the marks it begins with, a message by its header, text by
//! being text. A program or script is never previewed, nor is anything we cannot read
//! ourselves; that is said in words instead.
//!
//! What a preview shows is the attachment's content, written by a stranger: it is data,
//! drawn as a message's body is (no script, nothing fetched), and a command that returns it
//! marks it as `untrusted_content`.

use crate::attachments;
use openmailclient_model::time::Region;
use openmailclient_model::zone::Zone;
use openmailclient_model::{Attachment, Item, PropValue, pid};

/// At most this much of a text file is shown; the rest is said to be left out.
const MAX_TEXT: usize = 512 * 1024;
/// At most this many events, to-dos or cards of a calendar or contacts file are listed.
const MAX_LISTED: usize = 200;
/// The content id the picture of a picture preview is placed by.
pub const PICTURE_ID: &str = "attachment-preview-picture";

/// What an attachment is shown as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Picture,
    Text,
    Page,
    /// Values with commas or tabs between them, set out as a table.
    Table,
    Message,
    Calendar,
    Contacts,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Picture => "picture",
            Kind::Text => "text",
            Kind::Page => "page",
            Kind::Table => "table",
            Kind::Message => "message",
            Kind::Calendar => "calendar",
            Kind::Contacts => "contacts",
        }
    }
}

/// An attachment as the reading pane shows it: lines of a header over it (for an attached
/// message, who sent it, when and to whom), and an item whose body is what is shown, with the
/// pictures that body places among its attachments.
#[derive(Clone, Debug)]
pub struct Preview {
    pub kind: Kind,
    pub head: Vec<(&'static str, String)>,
    pub item: Item,
    /// Whether more was in the file than is shown.
    pub cut: bool,
}

/// `attachment` as it is previewed, or the words that say why it is not.
pub fn preview(attachment: &Attachment, zone: &Zone, region: &Region) -> Result<Preview, String> {
    let name = attachments::file_name(attachment);
    if let Some(inner) = &attachment.embedded {
        return Ok(message((**inner).clone(), zone, region));
    }
    let data = attachment.data.as_slice();
    let ending = attachments::ending(&name).unwrap_or_default();
    let mime = attachment.mime().to_ascii_lowercase();
    // A program or script is not shown, even as its text: it is only saved.
    if matches!(attachments::caution(&name, data), Some(attachments::Caution::Program { .. } | attachments::Caution::Runs { .. })) {
        return Err(format!("{name} is a program or script. It is not previewed; double-click it to save it."));
    }
    if data.is_empty() {
        return Err(format!("{name} is empty."));
    }
    if is_picture(data) {
        let mut shown = attachment.clone();
        shown.props.set_str(pid::ATTACH_CONTENT_ID, PICTURE_ID);
        let mut item = Item::default();
        item.props.set_str(pid::HTML, &format!("<p><img src=\"cid:{PICTURE_ID}\" alt=\"{}\" style=\"max-width:100%\"></p>", escape(&name)));
        item.attachments.push(shown);
        return Ok(Preview { kind: Kind::Picture, head: Vec::new(), item, cut: false });
    }
    match ending.as_str() {
        "msg" => {
            return openmailclient_msg::read(data).map(|m| message(m, zone, region)).map_err(|_| format!("{name} could not be read as a message."));
        }
        "eml" => {
            return openmailclient_mime::parse(data).map(|m| message(m, zone, region)).map_err(|_| format!("{name} could not be read as a message."));
        }
        _ => {}
    }
    if mime == "message/rfc822"
        && let Ok(m) = openmailclient_mime::parse(data)
    {
        return Ok(message(m, zone, region));
    }
    if ending == "ics" || mime == "text/calendar" {
        return openmailclient_formats::ics::read(data, zone)
            .map(|c| calendar(&c, zone, region))
            .map_err(|_| format!("{name} could not be read as a calendar."));
    }
    if ending == "vcf" || mime == "text/vcard" || mime == "text/x-vcard" {
        return openmailclient_formats::vcf::read(data).map(|c| contacts(&c)).map_err(|_| format!("{name} could not be read as contact cards."));
    }
    let (text, cut) = text_of(data).ok_or_else(|| format!("There is no preview for {name}. Double-click it to open or save it."))?;
    let mut item = Item::default();
    if matches!(ending.as_str(), "htm" | "html" | "xhtml") || mime == "text/html" {
        item.props.set_str(pid::HTML, &text);
        return Ok(Preview { kind: Kind::Page, head: Vec::new(), item, cut });
    }
    let sep = match ending.as_str() {
        "csv" => Some(','),
        "tsv" | "tab" => Some('\t'),
        _ if mime == "text/csv" => Some(','),
        _ if mime == "text/tab-separated-values" => Some('\t'),
        _ => None,
    };
    if let Some(sep) = sep {
        let (rows, more) = cells(&text, sep);
        if rows.iter().any(|r| r.len() > 1) {
            let mut head = vec![("Rows", counted(rows.len().saturating_sub(1), "row", "rows"))];
            if more {
                head.push(("Shown", format!("the first {MAX_ROWS} rows")));
            }
            item.props.set_str(pid::HTML, &table(&rows));
            return Ok(Preview { kind: Kind::Table, head, item, cut: cut || more });
        }
    }
    item.props.set_str(pid::BODY, &text);
    Ok(Preview { kind: Kind::Text, head: Vec::new(), item, cut })
}

const MAX_ROWS: usize = 1000;
const MAX_COLUMNS: usize = 64;

/// The rows of a file of values, as RFC 4180 2 has them: a value in double quotes may hold
/// the separator, line breaks and a doubled quote. Bounded in rows and columns; the second
/// value says whether rows were left out.
pub fn cells(text: &str, sep: char) -> (Vec<Vec<String>>, bool) {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    cell.push('"');
                }
                '"' => quoted = false,
                _ => cell.push(c),
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => quoted = true,
            '\n' => {
                if row.len() < MAX_COLUMNS {
                    row.push(std::mem::take(&mut cell));
                }
                cell.clear();
                rows.push(std::mem::take(&mut row));
                if rows.len() > MAX_ROWS {
                    rows.truncate(MAX_ROWS + 1);
                    return (rows, true);
                }
            }
            '\r' => {}
            c if c == sep => {
                if row.len() < MAX_COLUMNS {
                    row.push(std::mem::take(&mut cell));
                }
                cell.clear();
            }
            _ => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        if row.len() < MAX_COLUMNS {
            row.push(cell);
        }
        rows.push(row);
    }
    (rows, false)
}

/// Rows as a table: the first row as its headings, each value as text and never as markup.
fn table(rows: &[Vec<String>]) -> String {
    let mut html = String::from("<table border=\"1\" cellpadding=\"4\" style=\"border-collapse:collapse\">");
    for (i, row) in rows.iter().enumerate() {
        html.push_str("<tr>");
        let tag = if i == 0 { "th" } else { "td" };
        for value in row {
            html.push_str(&format!("<{tag}>{}</{tag}>", escape(value)));
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");
    html
}

/// An attached message: its own header over its body.
fn message(m: Item, zone: &Zone, region: &Region) -> Preview {
    let mut head = Vec::new();
    let sender = m.sender_name();
    let email = m.sender_email();
    let from = if email.is_empty() || email == sender { sender } else { format!("{sender} <{email}>") };
    head.push(("From", from));
    if m.time() > 0 {
        head.push(("Sent", region.stamp(&zone.civil(m.time()))));
    }
    for (label, kind) in [("To", openmailclient_model::RECIP_TO), ("Cc", openmailclient_model::RECIP_CC)] {
        let who = m.display_recipients(kind);
        if !who.is_empty() {
            head.push((label, who));
        }
    }
    head.push(("Subject", m.subject()));
    if let Some(names) = attached_names(&m) {
        head.push(("Attachments", names));
    }
    Preview { kind: Kind::Message, head, item: m, cut: false }
}

/// The files attached to an attached message, by name.
fn attached_names(m: &Item) -> Option<String> {
    let names: Vec<String> = attachments::files(m).map(|(_, a)| attachments::file_name(a)).take(20).collect();
    (!names.is_empty()).then(|| names.join("; "))
}

/// A calendar file: each event with its time and place, and each to-do.
fn calendar(c: &openmailclient_formats::ics::Calendar, zone: &Zone, region: &Region) -> Preview {
    let mut lines = Vec::new();
    for e in c.events.iter().take(MAX_LISTED) {
        let a = &e.appointment;
        let (start, end) = (zone.civil(a.start), zone.civil(a.end));
        let when = if a.all_day {
            region.date(&start)
        } else if start.day_number == end.day_number {
            format!("{} {} to {}", region.date(&start), region.time(&start), region.time(&end))
        } else {
            format!("{} {} to {} {}", region.date(&start), region.time(&start), region.date(&end), region.time(&end))
        };
        let subject = if a.subject.trim().is_empty() { "(no subject)" } else { a.subject.as_str() };
        lines.push(format!("{subject}\n    {when}"));
        if !a.location.trim().is_empty() {
            lines.push(format!("    {}", a.location));
        }
    }
    for t in c.todos.iter().take(MAX_LISTED) {
        lines.push(format!("To-do: {}", t.task.subject));
    }
    let mut head = Vec::new();
    if let Some(name) = c.name.as_ref().filter(|n| !n.trim().is_empty()) {
        head.push(("Calendar", name.clone()));
    }
    head.push(("Holds", counted(c.events.len(), "event", "events") + &todos(c.todos.len())));
    let cut = c.events.len() > MAX_LISTED || c.todos.len() > MAX_LISTED;
    Preview { kind: Kind::Calendar, head, item: plain(&lines.join("\n")), cut }
}

fn todos(n: usize) -> String {
    if n == 0 { String::new() } else { format!(" and {}", counted(n, "to-do", "to-dos")) }
}

/// Contact cards: each with its addresses and numbers, and each group with its members.
fn contacts(c: &openmailclient_formats::vcf::Cards) -> Preview {
    let mut lines = Vec::new();
    for card in c.cards.iter().take(MAX_LISTED) {
        match card {
            openmailclient_formats::vcf::Card::Contact(p) => {
                lines.push(p.display_name());
                let work = [p.job_title.as_str(), p.company.as_str()].into_iter().filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join(", ");
                if !work.is_empty() {
                    lines.push(format!("    {work}"));
                }
                for e in p.emails.iter().filter(|e| !e.address.is_empty()) {
                    lines.push(format!("    {}", e.address));
                }
                for (kind, number) in p.phones.iter().filter(|(_, n)| !n.trim().is_empty()) {
                    lines.push(format!("    {}: {number}", kind.name()));
                }
            }
            openmailclient_formats::vcf::Card::Group(g) => {
                lines.push(format!("{} (group of {})", g.name, g.members.len()));
            }
        }
    }
    let head = vec![("Holds", counted(c.cards.len(), "card", "cards"))];
    Preview { kind: Kind::Contacts, head, item: plain(&lines.join("\n")), cut: c.cards.len() > MAX_LISTED }
}

fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn plain(text: &str) -> Item {
    let mut item = Item::default();
    item.props.set(pid::BODY, PropValue::String(text.to_string()));
    item
}

/// Whether `data` begins as a picture we draw does: PNG, JPEG, GIF, BMP, WebP, TIFF, or SVG.
/// The bytes are looked at, not the name; the picture is decoded on a thread of its own.
pub fn is_picture(data: &[u8]) -> bool {
    let starts = |mark: &[u8]| data.starts_with(mark);
    if starts(b"\x89PNG\r\n\x1a\n") || starts(&[0xFF, 0xD8, 0xFF]) || starts(b"GIF87a") || starts(b"GIF89a") || starts(b"II*\0") || starts(b"MM\0*") {
        return true;
    }
    if starts(b"BM") && data.len() > 26 {
        return true;
    }
    if starts(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        return true;
    }
    // An SVG drawing: XML whose first element is <svg.
    let head = data.get(..data.len().min(1024)).unwrap_or_default();
    let head = String::from_utf8_lossy(head);
    let head = head.trim_start_matches('\u{FEFF}').trim_start();
    head.starts_with("<svg") || (head.starts_with("<?xml") && head.contains("<svg"))
}

/// `data` as text, when it is text: UTF-8, or UTF-16 by its byte-order mark. Bytes that are
/// mostly not letters (a binary file) give none. The second is whether it was cut.
pub fn text_of(data: &[u8]) -> Option<(String, bool)> {
    let shown = data.get(..data.len().min(MAX_TEXT)).unwrap_or_default();
    let cut = data.len() > MAX_TEXT;
    let text = if let Some(rest) = shown.strip_prefix(&[0xFF, 0xFE]) {
        utf16(rest, u16::from_le_bytes)
    } else if let Some(rest) = shown.strip_prefix(&[0xFE, 0xFF]) {
        utf16(rest, u16::from_be_bytes)
    } else {
        let rest = shown.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(shown);
        // A cut may fall inside a letter: what is whole before it is kept.
        match std::str::from_utf8(rest) {
            Ok(s) => s.to_string(),
            Err(e) if cut && e.error_len().is_none() => String::from_utf8_lossy(rest.get(..e.valid_up_to()).unwrap_or_default()).into_owned(),
            Err(_) => {
                // Not UTF-8: Latin-1 when it is still mostly text.
                rest.iter().map(|b| char::from(*b)).collect()
            }
        }
    };
    let total = text.chars().take(4096).count();
    let odd = text.chars().take(4096).filter(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\u{0C}')).count();
    // More than one in thirty characters a control code: a binary file, not text.
    if total == 0 || odd.saturating_mul(30) > total {
        return None;
    }
    Some((text.replace("\r\n", "\n"), cut))
}

fn utf16(bytes: &[u8], word: fn([u8; 2]) -> u16) -> String {
    let words = bytes.chunks_exact(2).map(|c| word([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)]));
    char::decode_utf16(words).map(|c| c.unwrap_or('\u{FFFD}')).collect()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
