//! Header fields: unfolding, encoded words, parameters, address lists, dates.

use openmailclient_model::time;

/// Header lines beyond this many are ignored.
const MAX_HEADERS: usize = 2_000;

pub(crate) struct Headers(Vec<(String, Vec<u8>)>);

impl Headers {
    /// Unfold and split a header block ([RFC 5322] 2.2). Names are lower-cased.
    pub(crate) fn parse(head: &[u8]) -> Self {
        let mut out: Vec<(String, Vec<u8>)> = Vec::new();
        for line in head.split(|&b| b == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                continue;
            }
            if line.first().is_some_and(|b| *b == b' ' || *b == b'\t') {
                // Unfolding takes away the line break alone; the white space after it stays.
                if let Some((_, v)) = out.last_mut() {
                    v.extend_from_slice(line);
                }
                continue;
            }
            let Some(colon) = line.iter().position(|&b| b == b':') else { continue };
            let name = line.get(..colon).unwrap_or(&[]);
            if name.is_empty() || name.iter().any(|b| !b.is_ascii_graphic()) {
                continue;
            }
            if out.len() >= MAX_HEADERS {
                break;
            }
            out.push((String::from_utf8_lossy(name).to_ascii_lowercase(), line.get(colon + 1..).unwrap_or(&[]).trim_ascii().to_vec()));
        }
        Self(out)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The first field of this name, with encoded words decoded.
    pub(crate) fn text(&self, name: &str) -> Option<String> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, v)| decode_words(&bytes_to_string(v)))
    }

    pub(crate) fn all_text(&self, name: &str) -> Vec<String> {
        self.0.iter().filter(|(n, _)| n == name).map(|(_, v)| decode_words(&bytes_to_string(v))).collect()
    }
}

/// Bytes with no declared charset: UTF-8 when valid, else Windows-1252 as mail clients assume.
pub(crate) fn bytes_to_string(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::WINDOWS_1252.decode(b).0.into_owned(),
    }
}

pub(crate) fn decode_charset(bytes: &[u8], label: &str) -> String {
    match encoding_rs::Encoding::for_label(label.trim().as_bytes()) {
        Some(enc) => enc.decode(bytes).0.into_owned(),
        None => bytes_to_string(bytes),
    }
}

/// Decode `=?charset?B|Q?text?=` words ([RFC 2047]). White space between two words is dropped.
pub(crate) fn decode_words(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    let mut after_word = false;
    while let Some(start) = rest.find("=?") {
        let (before, cand) = rest.split_at(start);
        match parse_word(cand) {
            Some((text, len)) => {
                if !(after_word && before.chars().all(char::is_whitespace)) {
                    out.push_str(before);
                }
                out.push_str(&text);
                rest = cand.get(len..).unwrap_or("");
                after_word = true;
            }
            None => {
                out.push_str(before);
                out.push_str("=?");
                rest = cand.get(2..).unwrap_or("");
                after_word = false;
            }
        }
    }
    out.push_str(rest);
    out
}

/// One encoded word at the start of `s`: its text and its length in bytes.
fn parse_word(s: &str) -> Option<(String, usize)> {
    let mut it = s.get(2..)?.splitn(3, '?');
    let (charset, enc, rest) = (it.next()?, it.next()?, it.next()?);
    let end = rest.find("?=")?;
    let payload = rest.get(..end)?;
    if charset.is_empty() || charset.contains(char::is_whitespace) || payload.contains(char::is_whitespace) {
        return None;
    }
    let bytes = match enc {
        "B" | "b" => crate::decode_base64(payload.as_bytes()),
        "Q" | "q" => crate::decode_qp(payload.replace('_', " ").as_bytes()),
        _ => return None,
    };
    // "charset*language" ([RFC 2231] 5).
    let charset = charset.split('*').next().unwrap_or(charset);
    Some((decode_charset(&bytes, charset), s.len().checked_sub(rest.len())?.checked_add(end)?.checked_add(2)?))
}

/// Split on `sep` characters that are outside quotes, angle brackets and comments.
fn split_top(s: &str, seps: &[char], angles: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut escaped, mut angle, mut paren) = (false, false, 0u32, 0u32);
    for c in s.chars() {
        if escaped {
            escaped = false;
        } else if quoted {
            match c {
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }
        } else {
            match c {
                '"' => quoted = true,
                '<' if angles => angle = angle.saturating_add(1),
                '>' if angles => angle = angle.saturating_sub(1),
                '(' if angles => paren = paren.saturating_add(1),
                ')' if angles => paren = paren.saturating_sub(1),
                c if angle == 0 && paren == 0 && seps.contains(&c) => {
                    out.push(std::mem::take(&mut cur));
                    continue;
                }
                _ => {}
            }
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    let Some(inner) = s.strip_prefix('"') else { return s.to_string() };
    let inner = inner.strip_suffix('"').unwrap_or(inner);
    let mut out = String::with_capacity(inner.len());
    let mut escaped = false;
    for c in inner.chars() {
        if c == '\\' && !escaped {
            escaped = true;
        } else {
            out.push(c);
            escaped = false;
        }
    }
    out
}

/// `type/subtype; name=value; …` → (lower-cased value, parameters with lower-cased names).
/// Handles quoted values, encoded words, and [RFC 2231] continuations and charsets.
pub(crate) fn parse_params(value: &str) -> (String, Vec<(String, String)>) {
    let mut pieces = split_top(value, &[';'], false).into_iter();
    let main = pieces.next().unwrap_or_default().trim().to_ascii_lowercase();
    let mut plain: Vec<(String, String)> = Vec::new();
    // name → (index, extended, raw value)
    let mut segments: Vec<(String, u32, bool, String)> = Vec::new();
    for piece in pieces.take(64) {
        let Some((name, val)) = piece.split_once('=') else { continue };
        let name = name.trim().to_ascii_lowercase();
        let val = unquote(val);
        match name.split_once('*') {
            None => plain.push((name, decode_words(&val))),
            Some((base, tail)) => {
                let extended = tail.ends_with('*') || tail.is_empty();
                let index = tail.trim_end_matches('*').parse::<u32>().unwrap_or(0);
                segments.push((base.to_string(), index, extended, val));
            }
        }
    }
    segments.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
    let mut i = 0;
    while let Some((base, _, _, _)) = segments.get(i) {
        let base = base.clone();
        let mut bytes: Vec<u8> = Vec::new();
        let mut charset = String::new();
        let mut first = true;
        while let Some((b, _, extended, val)) = segments.get(i) {
            if *b != base {
                break;
            }
            let mut v = val.as_str();
            if *extended {
                if first && let Some((cs, rest)) = v.split_once('\'') {
                    charset = cs.to_string();
                    v = rest.split_once('\'').map_or(rest, |(_, r)| r);
                }
                bytes.extend(percent_decode(v));
            } else {
                bytes.extend_from_slice(v.as_bytes());
            }
            first = false;
            i += 1;
        }
        let text = decode_charset(&bytes, &charset);
        // The extended form wins over a plain fallback of the same name.
        plain.retain(|(n, _)| *n != base);
        plain.push((base, text));
    }
    (main, plain)
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'%'
            && let (Some(hi), Some(lo)) = (b.get(i + 1).copied().and_then(crate::hex), b.get(i + 2).copied().and_then(crate::hex))
        {
            out.push(hi << 4 | lo);
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// An address list ([RFC 5322] 3.4) → (display name, address) pairs. Either may be empty.
pub fn parse_addresses(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for piece in split_top(s, &[',', ';'], true) {
        let mut piece = piece.trim();
        // "Group name: a@x, b@x;" — drop the group label.
        if let Some((label, rest)) = piece.split_once(':')
            && !label.contains(['<', '@', '"'])
        {
            piece = rest.trim();
        }
        if piece.is_empty() {
            continue;
        }
        let (name, email) = match (piece.rfind('<'), piece.rfind('>')) {
            (Some(lt), Some(gt)) if lt < gt => (unquote(piece.get(..lt).unwrap_or("")), piece.get(lt + 1..gt).unwrap_or("").trim().to_string()),
            _ => match piece.split_once('(') {
                // "a@x (Name)", the old comment form.
                Some((addr, comment)) if addr.contains('@') => (comment.trim_end_matches(')').trim().to_string(), addr.trim().to_string()),
                _ if piece.contains('@') && !piece.contains(char::is_whitespace) => (String::new(), piece.to_string()),
                _ => (unquote(piece), String::new()),
            },
        };
        let name = if name == email { String::new() } else { name.trim().trim_matches('\'').to_string() };
        if !name.is_empty() || !email.is_empty() {
            out.push((name, email));
        }
        if out.len() >= 5_000 {
            break;
        }
    }
    out
}

/// A `Date:` value ([RFC 5322] 3.3, with the obsolete forms) → UTC FILETIME ticks.
pub fn parse_date(s: &str) -> Option<i64> {
    // Comments such as "(UTC)" carry nothing.
    let s: String = split_top(s, &['('], false).into_iter().next().unwrap_or_default();
    let mut tokens = s.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).peekable();
    if tokens.peek().is_some_and(|t| t.chars().all(|c| c.is_ascii_alphabetic())) {
        tokens.next();
    }
    let day: u32 = tokens.next()?.parse().ok()?;
    let month_name = tokens.next()?.to_ascii_lowercase();
    let month = u32::try_from(
        ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"].iter().position(|m| month_name.starts_with(m))?,
    )
    .ok()?
        + 1;
    let year_raw = tokens.next()?;
    let mut year: i64 = year_raw.parse().ok()?;
    if year_raw.len() <= 2 {
        year += if year < 50 { 2000 } else { 1900 };
    } else if year_raw.len() == 3 {
        year += 1900;
    }
    let mut hms = tokens.next()?.split(':');
    let hour: u32 = hms.next()?.parse().ok()?;
    let minute: u32 = hms.next()?.parse().ok()?;
    let second: u32 = hms.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    if !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 || !(1601..=9999).contains(&year) {
        return None;
    }
    let offset = match tokens.next() {
        Some(z) if z.starts_with(['+', '-']) && z.len() == 5 => {
            let n: i32 = z.get(1..)?.parse().ok()?;
            let minutes = (n / 100).min(23) * 60 + (n % 100).min(59);
            if z.starts_with('-') { -minutes } else { minutes }
        }
        Some(z) => match z.to_ascii_uppercase().as_str() {
            "EDT" => -240,
            "EST" | "CDT" => -300,
            "CST" | "MDT" => -360,
            "MST" | "PDT" => -420,
            "PST" => -480,
            _ => 0,
        },
        None => 0,
    };
    Some(time::from_civil(year, month, day, hour, minute, second.min(59), offset))
}
