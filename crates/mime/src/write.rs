//! Writing an item as an RFC 5322 / MIME message.

use base64::Engine as _;
use openmailclient_model::{Attachment, Item, PropBag, RECIP_BCC, RECIP_CC, RECIP_TO, pid, recipient_email, recipient_label, time};

/// Write one message. Text is quoted-printable UTF-8, attachments are base64 ([RFC 2045] 6).
pub fn write(item: &Item) -> Vec<u8> {
    let mut out = String::new();
    let from_name = item.sender_name();
    let from_email = item.sender_email();
    if !from_name.is_empty() || !from_email.is_empty() {
        header(&mut out, "From", &mailbox(&from_name, &from_email));
    }
    for (kind, name) in [(RECIP_TO, "To"), (RECIP_CC, "Cc"), (RECIP_BCC, "Bcc")] {
        let list: Vec<String> =
            item.recipients.iter().filter(|r| r.i32(pid::RECIPIENT_TYPE).unwrap_or(RECIP_TO) == kind).map(recipient_mailbox).collect();
        if !list.is_empty() {
            header(&mut out, name, &list.join(", "));
        }
    }
    header(&mut out, "Subject", &encode_text(&item.subject()));
    if item.time() > 0 {
        header(&mut out, "Date", &format_date(item.time()));
    }
    for (name, id) in [("Message-ID", pid::INTERNET_MESSAGE_ID), ("In-Reply-To", pid::IN_REPLY_TO_ID), ("References", pid::INTERNET_REFERENCES)] {
        if let Some(v) = item.props.str(id).filter(|v| !v.is_empty() && v.is_ascii()) {
            header(&mut out, name, v);
        }
    }
    // When it stops being of use ([RFC 4021] 2.1, Expires). When a held message may go is ours
    // alone and is not told to anyone.
    if item.expires() > 0 {
        header(&mut out, "Expires", &format_date(item.expires()));
    }
    match item.importance() {
        2 => header(&mut out, "Importance", "high"),
        0 => header(&mut out, "Importance", "low"),
        _ => {}
    }
    if !item.categories.is_empty() {
        header(&mut out, "Keywords", &encode_text(&item.categories.join(", ")));
    }
    if item.is_unsent() {
        header(&mut out, "X-Unsent", "1");
    }
    header(&mut out, "MIME-Version", "1.0");

    let html = item.body_html();
    let text =
        item.props.str(pid::BODY).map(|s| s.replace("\r\n", "\n")).unwrap_or_else(|| if html.is_some() { item.body_text() } else { String::new() });
    let is_inline = |a: &&Attachment| matches!((a.content_id(), &html), (Some(cid), Some(h)) if !cid.is_empty() && h.contains(&format!("cid:{cid}")));
    // A meeting's invitation, reply or cancellation carries its calendar as another way of
    // reading the message, with the method it is (RFC 6047 2.4, [MS-OXCICAL] 2.1.3.4).
    let invite = invitation(item);
    let is_invite = |a: &&Attachment| invite.is_some_and(|(i, _)| std::ptr::eq(*a, i));
    let inline: Vec<&Attachment> = item.attachments.iter().filter(is_inline).collect();
    let files: Vec<&Attachment> = item.attachments.iter().filter(|a| !is_inline(a) && !is_invite(a)).collect();
    let seed = fnv(item.subject().as_bytes()) ^ (item.approx_size() as u64).rotate_left(17) ^ item.time().unsigned_abs();

    let calendar = invite.map(|(a, method)| calendar_part(&a.data, method));
    let mut body = match &html {
        Some(h) => {
            let mut ways = vec![text_part("plain", &text), text_part("html", h)];
            ways.extend(calendar.clone());
            let alt = multipart("alternative", seed ^ 1, &ways);
            if inline.is_empty() {
                alt
            } else {
                let mut parts = vec![alt];
                parts.extend(inline.iter().map(|a| attachment_part(a, true)));
                multipart("related", seed ^ 2, &parts)
            }
        }
        None => match calendar {
            Some(c) => multipart("alternative", seed ^ 1, &[text_part("plain", &text), c]),
            None => text_part("plain", &text),
        },
    };
    if !files.is_empty() {
        let mut parts = vec![body];
        parts.extend(files.iter().map(|a| attachment_part(a, false)));
        body = multipart("mixed", seed ^ 3, &parts);
    }
    out.push_str(&body);
    out.into_bytes()
}

fn header(out: &mut String, name: &str, value: &str) {
    // Fold at white space so no line passes 78 columns where that is possible ([RFC 5322] 2.1.1).
    let clean: String = value.chars().map(|c| if c == '\r' || c == '\n' { ' ' } else { c }).collect();
    let mut line = format!("{name}:");
    for word in clean.split(' ').filter(|w| !w.is_empty()) {
        if line.len() + 1 + word.len() > 78 && line.len() > name.len() + 1 {
            out.push_str(&line);
            out.push_str("\r\n");
            line = String::new();
        }
        line.push(' ');
        line.push_str(word);
    }
    out.push_str(&line);
    out.push_str("\r\n");
}

/// An unstructured value: as is when plain ASCII, else UTF-8 base64 encoded words ([RFC 2047]).
fn encode_text(s: &str) -> String {
    if s.is_ascii() && !s.contains("=?") {
        return s.to_string();
    }
    // 39 bytes of text make 52 characters of base64: a word fits a 78-column line after a header name.
    let mut words = Vec::new();
    let mut chunk = String::new();
    for c in s.chars() {
        if chunk.len() + c.len_utf8() > 39 {
            words.push(std::mem::take(&mut chunk));
        }
        chunk.push(c);
    }
    words.push(chunk);
    words.iter().map(|w| format!("=?UTF-8?B?{}?=", base64::engine::general_purpose::STANDARD.encode(w))).collect::<Vec<_>>().join(" ")
}

fn mailbox(name: &str, email: &str) -> String {
    let email: String = email.chars().filter(|c| c.is_ascii_graphic() && !matches!(c, '<' | '>')).collect();
    if name.is_empty() || name == email {
        return if email.is_empty() { String::new() } else { format!("<{email}>") };
    }
    let shown = if name.is_ascii() { format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\"")) } else { encode_text(name) };
    if email.is_empty() { shown } else { format!("{shown} <{email}>") }
}

fn recipient_mailbox(r: &PropBag) -> String {
    mailbox(&recipient_label(r), &recipient_email(r))
}

/// "Mon, 5 Oct 2026 09:14:00 +0000" ([RFC 5322] 3.3). Always UTC.
fn format_date(t: i64) -> String {
    let c = time::civil(t, 0);
    let wd = c.weekday_name().get(..3).unwrap_or("Mon");
    let mon = c.month_name().get(..3).unwrap_or("Jan");
    format!("{wd}, {} {mon} {} {:02}:{:02}:{:02} +0000", c.day, c.year, c.hour, c.minute, c.second)
}

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| (h ^ u64::from(x)).wrapping_mul(0x0000_0100_0000_01b3))
}

fn multipart(kind: &str, seed: u64, parts: &[String]) -> String {
    // The boundary must not occur in a part; bump it until it does not.
    let mut n = seed;
    let boundary = loop {
        let b = format!("----=_OpenMailClient_{n:016x}");
        if !parts.iter().any(|p| p.contains(&b)) {
            break b;
        }
        n = n.wrapping_add(1);
    };
    let mut out = format!("Content-Type: multipart/{kind};\r\n boundary=\"{boundary}\"\r\n\r\n");
    for p in parts {
        out.push_str(&format!("--{boundary}\r\n{p}\r\n"));
    }
    out.push_str(&format!("--{boundary}--\r\n"));
    out
}

fn text_part(subtype: &str, text: &str) -> String {
    format!("Content-Type: text/{subtype}; charset=\"utf-8\"\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n{}", encode_qp(text))
}

/// The calendar a meeting message carries, and the method it gives (RFC 5546 1.4): the first
/// `text/calendar` attachment of an `IPM.Schedule.Meeting.*` message.
fn invitation(item: &Item) -> Option<(&Attachment, &'static str)> {
    if !item.message_class().to_ascii_lowercase().starts_with("ipm.schedule.meeting.") {
        return None;
    }
    let a = item.attachments.iter().find(|a| a.embedded.is_none() && a.mime().eq_ignore_ascii_case("text/calendar"))?;
    Some((a, calendar_method(&a.data)?))
}

/// The METHOD a calendar gives, one of those a message may carry; `None` for any other.
pub fn calendar_method(data: &[u8]) -> Option<&'static str> {
    let head = data.get(..data.len().min(4096)).unwrap_or_default();
    let text = String::from_utf8_lossy(head);
    let line = text.lines().find(|l| l.len() > 7 && l.get(..7).is_some_and(|h| h.eq_ignore_ascii_case("METHOD:")))?;
    let method = line.get(7..)?.trim().to_ascii_uppercase();
    ["PUBLISH", "REQUEST", "REPLY", "CANCEL", "COUNTER", "DECLINECOUNTER", "REFRESH", "ADD"].into_iter().find(|m| *m == method)
}

fn calendar_part(data: &[u8], method: &str) -> String {
    let mut out = format!("Content-Type: text/calendar; charset=\"utf-8\"; method={method}\r\nContent-Transfer-Encoding: base64\r\n\r\n");
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    for line in b64.as_bytes().chunks(76) {
        out.push_str(std::str::from_utf8(line).unwrap_or(""));
        out.push_str("\r\n");
    }
    out
}

fn attachment_part(a: &Attachment, inline: bool) -> String {
    let name = a.name();
    let mime = if a.embedded.is_some() { "message/rfc822" } else { a.mime() };
    let mime: String = mime.chars().filter(|c| c.is_ascii_graphic() && !matches!(c, ';' | '"')).collect();
    let data = match (&a.embedded, a.data.is_empty()) {
        (Some(inner), true) => write(inner),
        _ => a.data.clone(),
    };
    let mut out =
        format!("Content-Type: {};\r\n {}\r\n", if mime.contains('/') { &mime } else { "application/octet-stream" }, name_param("name", &name));
    out.push_str("Content-Transfer-Encoding: base64\r\n");
    out.push_str(&format!("Content-Disposition: {};\r\n {}\r\n", if inline { "inline" } else { "attachment" }, name_param("filename", &name)));
    if let Some(cid) = a.content_id().filter(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_graphic() && !matches!(ch, '<' | '>'))) {
        out.push_str(&format!("Content-ID: <{cid}>\r\n"));
    }
    out.push_str("\r\n");
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    for line in b64.as_bytes().chunks(76) {
        out.push_str(std::str::from_utf8(line).unwrap_or(""));
        out.push_str("\r\n");
    }
    out
}

/// `name="file.txt"`, or the [RFC 2231] `name*=UTF-8''…` form when the name is not plain ASCII.
fn name_param(key: &str, name: &str) -> String {
    if name.chars().all(|c| c.is_ascii() && !c.is_ascii_control() && c != '"' && c != '\\') {
        return format!("{key}=\"{name}\"");
    }
    let mut enc = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
            enc.push(b as char);
        } else {
            enc.push_str(&format!("%{b:02X}"));
        }
    }
    format!("{key}*=UTF-8''{enc}")
}

/// Quoted-printable with CRLF line ends and soft breaks at 76 columns ([RFC 2045] 6.7).
fn encode_qp(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    for (n, line) in text.replace("\r\n", "\n").split('\n').enumerate() {
        if n > 0 {
            out.push_str("\r\n");
        }
        let bytes = line.as_bytes();
        let mut col = 0;
        for (i, &b) in bytes.iter().enumerate() {
            let last = i + 1 == bytes.len();
            let literal = (b == b' ' || b == b'\t') && !last || (b.is_ascii_graphic() && b != b'=');
            let width = if literal { 1 } else { 3 };
            if col + width > 75 {
                out.push_str("=\r\n");
                col = 0;
            }
            if literal {
                out.push(b as char);
            } else {
                out.push_str(&format!("={b:02X}"));
            }
            col += width;
        }
    }
    out
}
