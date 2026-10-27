//! RFC 5322 / MIME (`.eml`) reader and writer.
//!
//! Reading is tolerant: mail in the wild breaks every rule, so a malformed part becomes text or an
//! attachment instead of an error. Sizes, depths and counts that come from the message are capped.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod header;
mod write;

use base64::Engine as _;
use header::{Headers, decode_charset, parse_params};
use openmailclient_model::{Attachment, Item, MSGFLAG_READ, MSGFLAG_UNSENT, PropValue, RECIP_BCC, RECIP_CC, RECIP_TO, pid};

pub use header::{parse_addresses, parse_date};
pub use write::{calendar_method, write};

/// Nesting deeper than this is kept as an attachment instead of being walked.
const MAX_DEPTH: usize = 32;
/// Parts beyond this many are ignored.
const MAX_PARTS: usize = 10_000;

#[derive(Debug, thiserror::Error)]
pub enum MimeError {
    #[error("the file is empty")]
    Empty,
    #[error("this does not look like an email message")]
    NotMail,
}

/// Read one message ([RFC 5322], MIME [RFC 2045]–[RFC 2049]).
pub fn parse(data: &[u8]) -> Result<Item, MimeError> {
    if data.iter().all(u8::is_ascii_whitespace) {
        return Err(MimeError::Empty);
    }
    let (head, _) = split_head(data);
    let headers = Headers::parse(head);
    if headers.is_empty() {
        return Err(MimeError::NotMail);
    }
    let mut item = Item::default();
    item.props.set_str(pid::MESSAGE_CLASS, "IPM.Note");
    item.props.set_str(pid::TRANSPORT_MESSAGE_HEADERS, &header::bytes_to_string(head));
    message_headers(&headers, &mut item);
    let mut ctx = Ctx { item, parts: 0, have_text: false, have_html: false, classed: false };
    walk(data, 0, &mut ctx);
    let lone_calendar = ctx.classed && !ctx.have_text && !ctx.have_html;
    let mut item = ctx.item;
    // A meeting sent as its calendar alone, as Outlook sends one, says what it says in the
    // calendar's DESCRIPTION (RFC 5545 3.8.1.5): that is its body.
    if lone_calendar
        && let Some(text) = item.attachments.iter().find(|a| a.mime() == "text/calendar").and_then(|a| write::calendar_text(&a.data, "DESCRIPTION"))
    {
        item.set_body_text(&text);
    }
    let has_attach = item.visible_attachments().next().is_some();
    item.props.set(pid::HASATTACH, PropValue::Bool(has_attach));
    Ok(item)
}

fn message_headers(h: &Headers, item: &mut Item) {
    let subject = h.text("subject").unwrap_or_default();
    // The field's own ends are trimmed already; what an encoded word holds is kept as written.
    item.set_subject(&subject);
    let from = h.text("from").map(|v| parse_addresses(&v)).unwrap_or_default();
    if let Some((name, email)) = from.first() {
        item.set_sender(if name.is_empty() { email } else { name }, email);
    }
    if let Some((name, email)) = h.text("sender").map(|v| parse_addresses(&v)).unwrap_or_default().first() {
        item.props.set_str(pid::SENDER_NAME, if name.is_empty() { email } else { name });
        item.props.set_str(pid::SENDER_EMAIL_ADDRESS, email);
        item.props.set_str(pid::SENDER_SMTP_ADDRESS, email);
    }
    for (name, kind) in [("to", RECIP_TO), ("cc", RECIP_CC), ("bcc", RECIP_BCC)] {
        for (n, e) in h.all_text(name).iter().flat_map(|v| parse_addresses(v)).take(MAX_PARTS) {
            item.add_recipient(kind, &n, &e);
        }
    }
    item.refresh_display_recipients();
    if let Some(t) = h.text("date").and_then(|d| parse_date(&d)) {
        item.props.set(pid::CLIENT_SUBMIT_TIME, PropValue::Time(t));
        item.props.set(pid::MESSAGE_DELIVERY_TIME, PropValue::Time(t));
    }
    // "Expires", or the X.400 "Expiry-Date" ([RFC 4021] 2.1).
    if let Some(t) = h.text("expires").or_else(|| h.text("expiry-date")).and_then(|d| parse_date(&d)).filter(|&t| t > 0) {
        item.props.set(pid::EXPIRY_TIME, PropValue::Time(t));
    }
    for (name, id) in [("message-id", pid::INTERNET_MESSAGE_ID), ("in-reply-to", pid::IN_REPLY_TO_ID), ("references", pid::INTERNET_REFERENCES)] {
        if let Some(v) = h.text(name) {
            item.props.set_str(id, v.trim());
        }
    }
    let importance = match h.text("importance").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("high") => 2,
        Some("low") => 0,
        Some(_) => 1,
        None => match h.text("x-priority").and_then(|v| v.trim().chars().next()) {
            Some('1' | '2') => 2,
            Some('4' | '5') => 0,
            _ => 1,
        },
    };
    item.props.set(pid::IMPORTANCE, PropValue::Int32(importance));
    // "X-Unsent: 1" is how Outlook marks a saved draft in an .eml.
    let unsent = h.text("x-unsent").is_some_and(|v| v.trim() == "1");
    item.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(MSGFLAG_READ | if unsent { MSGFLAG_UNSENT } else { 0 }));
    if let Some(k) = h.text("keywords") {
        item.categories = k.split([',', ';']).map(str::trim).filter(|c| !c.is_empty()).take(100).map(str::to_string).collect();
    }
}

struct Ctx {
    item: Item,
    parts: usize,
    have_text: bool,
    have_html: bool,
    /// Classed as a meeting's message by a calendar it carries.
    classed: bool,
}

/// Blank line between headers and body ([RFC 5322] 2.1). Returns (headers, body).
fn split_head(data: &[u8]) -> (&[u8], &[u8]) {
    if let Some(rest) = data.strip_prefix(b"\r\n").or_else(|| data.strip_prefix(b"\n")) {
        return (&[], rest);
    }
    let mut from = 0;
    while let Some(i) = data.get(from..).and_then(|d| d.iter().position(|&b| b == b'\n')) {
        let after = from.saturating_add(i).saturating_add(1);
        let rest = data.get(after..).unwrap_or(&[]);
        if let Some(body) = rest.strip_prefix(b"\r\n").or_else(|| rest.strip_prefix(b"\n")) {
            return (data.get(..after).unwrap_or(&[]), body);
        }
        from = after;
    }
    (data, &[])
}

fn walk(data: &[u8], depth: usize, ctx: &mut Ctx) {
    ctx.parts = ctx.parts.saturating_add(1);
    if ctx.parts > MAX_PARTS {
        return;
    }
    let (head, body) = split_head(data);
    let h = Headers::parse(head);
    let (mime, ct_params) = parse_params(&h.text("content-type").unwrap_or_else(|| "text/plain".to_string()));
    let mime = if mime.contains('/') { mime } else { "text/plain".to_string() };
    let (disposition, cd_params) = parse_params(&h.text("content-disposition").unwrap_or_default());
    let param = |list: &[(String, String)], key: &str| list.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let filename = param(&cd_params, "filename").or_else(|| param(&ct_params, "name")).map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    let encoding = h.text("content-transfer-encoding").unwrap_or_default().trim().to_ascii_lowercase();
    let is_attachment = disposition == "attachment" || (filename.is_some() && disposition != "inline" && !mime.starts_with("text/"));

    if mime.starts_with("multipart/") && depth < MAX_DEPTH {
        if let Some(boundary) = param(&ct_params, "boundary").filter(|b| !b.is_empty()) {
            let parts = split_multipart(body, &boundary);
            if !parts.is_empty() {
                for p in parts {
                    walk(p, depth + 1, ctx);
                }
                return;
            }
        }
        // No usable boundary: show what is there as text rather than lose it.
        if !ctx.have_text {
            ctx.item.set_body_text(&header::bytes_to_string(body));
            ctx.have_text = true;
        }
        return;
    }

    let decoded = decode_body(body, &encoding);
    let charset = param(&ct_params, "charset").unwrap_or_default();

    if mime == "message/rfc822" && depth > 0 && depth < MAX_DEPTH {
        let inner = parse(&decoded).ok();
        let name = filename.unwrap_or_else(|| {
            let subject = inner.as_ref().map(Item::subject).filter(|s| !s.is_empty()).unwrap_or_else(|| "message".to_string());
            format!(
                "{}.eml",
                subject.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).take(80).collect::<String>()
            )
        });
        let mut a = Attachment::file(&name, "message/rfc822", decoded);
        if let Some(inner) = inner {
            a.props.set(pid::ATTACH_METHOD, PropValue::Int32(5));
            a.embedded = Some(Box::new(inner));
        }
        ctx.item.attachments.push(a);
        return;
    }
    if !is_attachment && mime == "text/plain" && !ctx.have_text {
        ctx.item.set_body_text(&decode_charset(&decoded, &charset));
        ctx.have_text = true;
        return;
    }
    if !is_attachment && mime == "text/html" && !ctx.have_html {
        // Kept as UTF-8 bytes with the code page saying so ([MS-OXCMSG] 2.2.1.56.9).
        ctx.item.props.set(pid::HTML, PropValue::Binary(decode_charset(&decoded, &charset).into_bytes()));
        ctx.item.props.set(pid::INTERNET_CODEPAGE, PropValue::Int32(65001));
        ctx.have_html = true;
        return;
    }
    if depth == 0 && decoded.iter().all(u8::is_ascii_whitespace) {
        return;
    }
    // A meeting's invitation, answer or cancellation, by the calendar it carries (RFC 6047 2.4):
    // classed as Outlook classes one ([MS-OXOCAL] 2.2.6.1, 2.2.7.1, 2.2.8.1). Whether it is to be
    // believed, and what is done with it, is the engine's to say.
    if mime == "text/calendar"
        && !ctx.classed
        && let Some(class) = meeting_class(&decoded)
    {
        ctx.item.props.set_str(pid::MESSAGE_CLASS, class);
        ctx.classed = true;
    }
    let n = ctx.item.attachments.len() + 1;
    let name = filename.unwrap_or_else(|| format!("ATT{n:05}.{}", extension_for(&mime)));
    let mut a = Attachment::file(&name, &mime, decoded);
    if let Some(cid) = h.text("content-id") {
        let cid = cid.trim().trim_start_matches('<').trim_end_matches('>');
        if !cid.is_empty() {
            a.props.set_str(pid::ATTACH_CONTENT_ID, cid);
        }
    }
    ctx.item.attachments.push(a);
}

/// The message class of a message carrying this calendar, when it is a meeting's.
fn meeting_class(calendar: &[u8]) -> Option<&'static str> {
    Some(match write::calendar_method(calendar)? {
        "REQUEST" => "IPM.Schedule.Meeting.Request",
        "CANCEL" => "IPM.Schedule.Meeting.Canceled",
        "REPLY" => {
            let text = String::from_utf8_lossy(calendar.get(..calendar.len().min(64 << 10)).unwrap_or_default()).to_ascii_uppercase();
            if text.contains("PARTSTAT=ACCEPTED") {
                "IPM.Schedule.Meeting.Resp.Pos"
            } else if text.contains("PARTSTAT=TENTATIVE") {
                "IPM.Schedule.Meeting.Resp.Tent"
            } else if text.contains("PARTSTAT=DECLINED") {
                "IPM.Schedule.Meeting.Resp.Neg"
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

fn extension_for(mime: &str) -> &'static str {
    match mime {
        "text/plain" => "txt",
        "text/html" => "htm",
        "text/calendar" => "ics",
        "text/vcard" | "text/x-vcard" => "vcf",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "application/pdf" => "pdf",
        _ => "bin",
    }
}

/// The body parts between `--boundary` lines ([RFC 2046] 5.1.1). An unterminated last part is kept.
fn split_multipart<'a>(body: &'a [u8], boundary: &str) -> Vec<&'a [u8]> {
    let delim = format!("--{boundary}").into_bytes();
    let mut parts = Vec::new();
    let mut part_start: Option<usize> = None;
    let mut pos = 0usize;
    while pos < body.len() && parts.len() < MAX_PARTS {
        let rest = body.get(pos..).unwrap_or(&[]);
        let line_len = rest.iter().position(|&b| b == b'\n').map_or(rest.len(), |i| i + 1);
        let line = rest.get(..line_len).unwrap_or(&[]);
        if let Some(tail) = line.strip_prefix(delim.as_slice()) {
            let tail = tail.trim_ascii();
            if tail.is_empty() || tail == b"--" {
                if let Some(s) = part_start {
                    let mut e = pos;
                    // The line break before a delimiter belongs to the delimiter.
                    if e > s && body.get(e - 1) == Some(&b'\n') {
                        e -= 1;
                        if e > s && body.get(e - 1) == Some(&b'\r') {
                            e -= 1;
                        }
                    }
                    parts.push(body.get(s..e).unwrap_or(&[]));
                }
                part_start = if tail.is_empty() { Some(pos.saturating_add(line_len)) } else { None };
                if part_start.is_none() {
                    break;
                }
            }
        }
        pos = pos.saturating_add(line_len.max(1));
    }
    if let Some(s) = part_start
        && let Some(last) = body.get(s..)
        && !last.iter().all(u8::is_ascii_whitespace)
    {
        parts.push(last);
    }
    parts
}

fn decode_body(body: &[u8], encoding: &str) -> Vec<u8> {
    match encoding {
        "base64" => decode_base64(body),
        "quoted-printable" => decode_qp(body),
        _ => body.to_vec(),
    }
}

/// Base64 that forgives line breaks, stray characters, missing padding and a ragged end.
pub(crate) fn decode_base64(data: &[u8]) -> Vec<u8> {
    let mut clean: Vec<u8> = data.iter().copied().filter(|b| b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/').collect();
    if clean.len() % 4 == 1 {
        clean.pop();
    }
    let cfg = base64::engine::GeneralPurposeConfig::new()
        .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true);
    base64::engine::GeneralPurpose::new(&base64::alphabet::STANDARD, cfg).decode(&clean).unwrap_or_default()
}

/// Quoted-printable ([RFC 2045] 6.7); a broken escape is kept as written.
pub(crate) fn decode_qp(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while let Some(&b) = data.get(i) {
        if b != b'=' {
            out.push(b);
            i += 1;
            continue;
        }
        match (data.get(i + 1).copied(), data.get(i + 2).copied()) {
            (Some(b'\r'), Some(b'\n')) => i += 3,
            (Some(b'\n'), _) => i += 2,
            (Some(a), Some(c)) => match (hex(a), hex(c)) {
                (Some(hi), Some(lo)) => {
                    out.push(hi << 4 | lo);
                    i += 3;
                }
                _ => {
                    out.push(b);
                    i += 1;
                }
            },
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

pub(crate) fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
