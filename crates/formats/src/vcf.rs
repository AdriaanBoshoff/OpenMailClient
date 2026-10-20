//! vCard files (RFC 6350 for 4.0, RFC 2426 for 3.0 and the 2.1 Outlook still writes): cards
//! read into contacts and contact groups, and contacts written as cards the way [MS-OXVCARD]
//! maps the one onto the other.
//!
//! A card comes from anyone: the file, every line, every list and the number of cards are held
//! to a limit, and nothing in a card is fetched or run. A picture is kept only when it is in the
//! card itself; one named by a link is said in a note and left where it is.

use openmailclient_model::contact::{self, Address, Contact, Email, Group, MAX_EMAILS, MAX_MEMBERS, MAX_PICTURE, Member, Phone, Place};
use openmailclient_model::{calendar, codepage, time};

/// Largest file read, in bytes.
pub const MAX_BYTES: usize = 32 << 20;
/// Most cards taken from one file.
pub const MAX_CARDS: usize = 20_000;
/// Longest line kept once unfolded, in bytes; a picture is the longest a card has.
const MAX_LINE: usize = 8 << 20;
/// Most properties read of one card, and parameters of one property.
const MAX_PROPS: usize = 2_000;
const MAX_PARAMS: usize = 64;
/// Most values of one list kept (CATEGORIES, NICKNAME…).
const MAX_LIST: usize = 200;
/// Longest field kept, in characters, and longest note.
const MAX_TEXT: usize = 4_000;
const MAX_NOTE: usize = 200_000;
/// Most notes kept of what could not be read.
const MAX_NOTES: usize = 200;
/// Deepest nesting of cards (a 2.1 AGENT holds one) passed over.
const MAX_DEPTH: usize = 8;

/// What a card is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Card {
    Contact(Box<Contact>),
    Group(Group),
}

impl Card {
    /// The UID it is known by; empty when it has none.
    pub fn uid(&self) -> &str {
        match self {
            Self::Contact(c) => &c.uid,
            Self::Group(g) => &g.uid,
        }
    }

    /// The name it is shown by.
    pub fn name(&self) -> String {
        match self {
            Self::Contact(c) => c.display_name(),
            Self::Group(g) => g.name.clone(),
        }
    }
}

/// What a file holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cards {
    pub cards: Vec<Card>,
    /// What could not be kept, said once each.
    pub notes: Vec<String>,
}

fn note(notes: &mut Vec<String>, text: impl Into<String>) {
    let text = text.into();
    if notes.len() < MAX_NOTES && !notes.contains(&text) {
        notes.push(text);
    }
}

/// `s` cut to at most `most` characters.
fn cut(s: &str, most: usize) -> String {
    match s.char_indices().nth(most) {
        Some((at, _)) => s.get(..at).unwrap_or_default().to_string(),
        None => s.to_string(),
    }
}

/// One content line: `group.NAME;PARAM=a,b;BARE:value`.
#[derive(Debug, Default)]
struct Prop {
    name: String,
    /// Each parameter's name and values; a bare 2.1 parameter (`TEL;WORK;FAX`) is a TYPE.
    params: Vec<(String, Vec<String>)>,
    value: String,
}

impl Prop {
    fn param(&self, name: &str) -> impl Iterator<Item = &str> {
        self.params.iter().filter(move |(n, _)| n == name).flat_map(|(_, v)| v.iter().map(String::as_str))
    }

    /// The types it is given, lowercase.
    /// The types it is given, lowercase; a quoted list (`TYPE="voice,home"`) is split too.
    fn types(&self) -> Vec<String> {
        self.param("TYPE").flat_map(|v| v.split(',')).map(|v| v.trim().to_ascii_lowercase()).filter(|v| !v.is_empty()).collect()
    }

    fn has_type(&self, t: &str) -> bool {
        self.types().iter().any(|v| v == t) || t == "pref" && self.param("PREF").next().is_some()
    }

    fn encoding(&self) -> String {
        self.param("ENCODING").next().unwrap_or_default().to_ascii_lowercase()
    }
}

/// Whether a line's property is quoted-printable (vCard 2.1), which folds by ending a line
/// with `=` rather than starting the next with a space.
fn is_qp(line: &[u8]) -> bool {
    let head = line.iter().position(|b| *b == b':').and_then(|at| line.get(..at)).unwrap_or(line);
    head.to_ascii_uppercase().windows(16).any(|w| w == b"QUOTED-PRINTABLE")
}

/// The lines of a file with folded lines joined again (RFC 6350 3.2, and the soft line breaks
/// of 2.1's quoted-printable), each cut to `MAX_LINE` bytes, read as UTF-8 or, where that
/// fails, as Windows-1252.
fn unfold(bytes: &[u8]) -> Vec<String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let mut lines: Vec<Vec<u8>> = Vec::new();
    let mut soft = false;
    for raw in bytes.split(|b| *b == b'\n') {
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        let more = match (raw.first(), lines.last_mut()) {
            (_, Some(last)) if soft => Some((last, raw)),
            (Some(b' ' | b'\t'), Some(last)) => Some((last, raw.get(1..).unwrap_or_default())),
            _ => None,
        };
        match more {
            Some((last, more)) => {
                if soft {
                    last.pop();
                }
                let room = MAX_LINE.saturating_sub(last.len());
                last.extend_from_slice(more.get(..more.len().min(room)).unwrap_or_default());
            }
            None if raw.is_empty() => {}
            None => lines.push(raw.get(..raw.len().min(MAX_LINE)).unwrap_or_default().to_vec()),
        }
        soft = lines.last().is_some_and(|l| l.ends_with(b"=") && is_qp(l)) && !raw.is_empty();
    }
    lines
        .into_iter()
        .map(|l| match String::from_utf8(l) {
            Ok(s) => s,
            Err(e) => codepage::decode(1252, e.as_bytes()),
        })
        .collect()
}

/// A content line read. `None` when it has no colon outside quotes.
fn content_line(line: &str) -> Option<Prop> {
    let b = line.as_bytes();
    let mut i = 0;
    while b.get(i).is_some_and(|c| *c != b';' && *c != b':') {
        i += 1;
    }
    let mut name = line.get(..i)?.trim().to_ascii_uppercase();
    // A group's prefix (`item1.EMAIL`) says nothing kept here.
    if let Some(dot) = name.rfind('.') {
        name = name.get(dot + 1..)?.to_string();
    }
    let mut params: Vec<(String, Vec<String>)> = Vec::new();
    while b.get(i) == Some(&b';') {
        let start = i + 1;
        let mut j = start;
        while b.get(j).is_some_and(|c| *c != b'=' && *c != b':' && *c != b';') {
            j += 1;
        }
        let pname = line.get(start..j)?.trim().to_ascii_uppercase();
        let mut values = Vec::new();
        if b.get(j) == Some(&b'=') {
            j += 1;
            loop {
                if b.get(j) == Some(&b'"') {
                    let open = j + 1;
                    let close = (open..b.len()).find(|k| b.get(*k) == Some(&b'"'))?;
                    values.push(line.get(open..close)?.to_string());
                    j = close + 1;
                } else {
                    let open = j;
                    while b.get(j).is_some_and(|c| *c != b',' && *c != b':' && *c != b';') {
                        j += 1;
                    }
                    values.push(line.get(open..j)?.to_string());
                }
                if b.get(j) == Some(&b',') && values.len() < MAX_LIST {
                    j += 1;
                } else {
                    break;
                }
            }
        }
        if params.len() < MAX_PARAMS {
            match pname.as_str() {
                "" => {}
                // 2.1 names an encoding or character set bare too.
                "QUOTED-PRINTABLE" | "BASE64" | "B" if values.is_empty() => params.push(("ENCODING".into(), vec![pname])),
                _ if values.is_empty() => params.push(("TYPE".into(), vec![pname])),
                _ => params.push((pname, values)),
            }
        }
        i = j;
    }
    if b.get(i) != Some(&b':') {
        return None;
    }
    Some(Prop { name, params, value: line.get(i + 1..)?.to_string() })
}

/// Quoted-printable undone (RFC 2045 6.7); a broken escape is kept as it is written.
fn unquote(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'=' {
            let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(byte) = hex {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// A code page for a character set's name.
fn code_page(charset: &str) -> u32 {
    let c = charset.to_ascii_lowercase();
    if let Some(n) = c.strip_prefix("windows-").and_then(|n| n.parse::<u32>().ok()) {
        return n;
    }
    if let Some(n) = c.strip_prefix("iso-8859-").and_then(|n| n.parse::<u32>().ok()) {
        return if n == 1 { 1252 } else { 28590 + n };
    }
    match c.as_str() {
        "us-ascii" | "ascii" => 1252,
        "shift_jis" | "shift-jis" | "sjis" => 932,
        "gb2312" | "gbk" | "gb18030" => 936,
        "big5" => 950,
        "euc-kr" => 949,
        "euc-jp" => 51932,
        "koi8-r" => 20866,
        "koi8-u" => 21866,
        _ => 65001,
    }
}

/// A property's value as text, with its 2.1 encoding and character set undone.
fn raw_text(p: &Prop) -> String {
    if p.encoding() == "quoted-printable" {
        let cp = p.param("CHARSET").next().map_or(65001, code_page);
        codepage::decode(cp, &unquote(&p.value))
    } else {
        p.value.clone()
    }
}

/// TEXT unescaped (RFC 6350 3.4).
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            if c != '\r' {
                out.push(c);
            }
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// TEXT escaped (RFC 6350 3.4); in a component `;` is escaped too.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c if c.is_control() && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// Values split at a separator that is not escaped, each unescaped and cut.
fn split(s: &str, sep: char, most: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut part = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                part.push('\\');
                part.extend(chars.next());
            }
            c if c == sep => {
                out.push(std::mem::take(&mut part));
                if out.len() >= most {
                    return out.into_iter().map(|p| cut(unescape(&p).trim(), MAX_TEXT)).collect();
                }
            }
            c => part.push(c),
        }
    }
    out.push(part);
    out.into_iter().map(|p| cut(unescape(&p).trim(), MAX_TEXT)).collect()
}

fn text(p: &Prop) -> String {
    cut(unescape(&raw_text(p)).trim(), MAX_TEXT)
}

/// The day a date is on: `1990-01-31`, `19900131`, with a time after it or not. A date with no
/// year (`--0131`) has no day here.
fn date(v: &str) -> Option<i64> {
    let v = v.trim();
    let v = v.split('T').next().unwrap_or(v);
    if v.starts_with("--") {
        return None;
    }
    let digits: String = v.chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 8 || v.chars().any(|c| !c.is_ascii_digit() && c != '-') {
        return None;
    }
    let year = digits.get(..4)?.parse::<i64>().ok()?;
    let month = digits.get(4..6)?.parse::<u32>().ok()?;
    let day = digits.get(6..8)?.parse::<u32>().ok()?;
    if !(1601..=9999).contains(&year) || !(1..=12).contains(&month) || day < 1 || day > calendar::days_in(year, month) {
        return None;
    }
    Some(time::to_unix(time::from_civil(year, month, day, 0, 0, 0, 0)).div_euclid(86_400))
}

fn date_text(day: i64) -> String {
    let c = time::civil(time::from_unix(day.saturating_mul(86_400)), 0);
    format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
}

/// The value of a URI property without its scheme (`tel:`, `mailto:`, `xmpp:`…).
fn without_scheme<'a>(v: &'a str, schemes: &[&str]) -> &'a str {
    for s in schemes {
        if let Some(rest) = v.get(..s.len()).filter(|h| h.eq_ignore_ascii_case(s)).and_then(|_| v.get(s.len()..)) {
            return rest;
        }
    }
    v
}

/// The bytes of a picture in the card itself: base64 (3.0 `ENCODING=b`, 2.1 `ENCODING=BASE64`)
/// or a `data:` URI (4.0). `Err` with a note for a link or anything else.
fn picture(p: &Prop) -> Result<Vec<u8>, &'static str> {
    let v = p.value.trim();
    let bytes = if matches!(p.encoding().as_str(), "b" | "base64") {
        crate::html::base64(v)
    } else if let Some(data) = v.get(..5).filter(|h| h.eq_ignore_ascii_case("data:")).and_then(|_| v.get(5..)) {
        let (head, body) = data.split_once(',').ok_or("a picture in a card could not be read")?;
        if !head.to_ascii_lowercase().ends_with(";base64") {
            return Err("a picture in a card could not be read");
        }
        crate::html::base64(body)
    } else {
        return Err("a picture named by a link was not fetched");
    };
    let bytes = bytes.ok_or("a picture in a card could not be read")?;
    if bytes.is_empty() {
        return Err("a picture in a card could not be read");
    }
    if bytes.len() > MAX_PICTURE {
        return Err("a picture larger than 4 MB was left out");
    }
    Ok(bytes)
}

/// The kind of telephone number a TEL is, among those not taken yet.
fn phone_kind(p: &Prop, taken: &[(Phone, String)]) -> Option<Phone> {
    let t = p.types();
    let has = |s: &str| t.iter().any(|x| x == s);
    let free = |k: Phone| !taken.iter().any(|(p, _)| *p == k);
    let pick = |ks: &[Phone]| ks.iter().copied().find(|k| free(*k));
    if has("fax") {
        return if has("home") { pick(&[Phone::HomeFax, Phone::BusinessFax]) } else { pick(&[Phone::BusinessFax, Phone::HomeFax]) };
    }
    if has("cell") || has("mobile") || has("iphone") {
        return pick(&[Phone::Mobile, Phone::Other]);
    }
    if has("pager") {
        return pick(&[Phone::Pager, Phone::Other]);
    }
    if has("car") {
        return pick(&[Phone::Car, Phone::Other]);
    }
    if has("x-assistant") || has("x-ms-assistant") {
        return pick(&[Phone::Assistant, Phone::Other]);
    }
    if has("work") {
        return pick(&[Phone::Business, Phone::Business2, Phone::Other]);
    }
    if has("home") {
        return pick(&[Phone::Home, Phone::Home2, Phone::Other]);
    }
    if has("pref") && t.len() <= 2 {
        return pick(&[Phone::Primary, Phone::Other, Phone::Business2, Phone::Home2]);
    }
    pick(&[Phone::Other, Phone::Primary, Phone::Business2, Phone::Home2])
}

fn place_of(p: &Prop) -> Place {
    let t = p.types();
    if t.iter().any(|x| x == "work") {
        Place::Business
    } else if t.iter().any(|x| x == "home") {
        Place::Home
    } else {
        Place::Other
    }
}

/// A card's properties made into a contact or group.
fn card_of(props: &[Prop], notes: &mut Vec<String>) -> Card {
    let mut c = Contact::default();
    let mut group = false;
    let mut members = Vec::new();
    let mut member_names: Vec<String> = Vec::new();
    let mut mailing = None;
    let mut sort_as = String::new();
    for p in props {
        match p.name.as_str() {
            "FN" => c.full_name = text(p),
            "N" => {
                let parts = split(&raw_text(p), ';', 5);
                let at = |i: usize| parts.get(i).map(|s| split(s, ',', MAX_LIST).join(" ")).unwrap_or_default();
                (c.surname, c.given, c.middle, c.prefix, c.suffix) = (at(0), at(1), at(2), at(3), at(4));
                if let Some(s) = p.param("SORT-AS").next() {
                    sort_as = cut(&unescape(s), MAX_TEXT);
                }
            }
            "NICKNAME" => c.nickname = split(&raw_text(p), ',', MAX_LIST).into_iter().find(|s| !s.is_empty()).unwrap_or_default(),
            "ORG" => {
                let parts = split(&raw_text(p), ';', 2);
                c.company = parts.first().cloned().unwrap_or_default();
                c.department = parts.get(1).cloned().unwrap_or_default();
            }
            "TITLE" => c.job_title = text(p),
            "ROLE" => c.profession = text(p),
            "EMAIL" => {
                let address = without_scheme(&text(p), &["mailto:"]).trim().to_string();
                if address.is_empty() || c.emails.iter().any(|e| e.address.eq_ignore_ascii_case(&address)) {
                    continue;
                }
                let email = Email { address, name: String::new() };
                // The preferred one first.
                if p.has_type("pref") || p.param("PREF").any(|v| v == "1") {
                    c.emails.insert(0, email);
                } else {
                    c.emails.push(email);
                }
                if c.emails.len() > MAX_EMAILS {
                    c.emails.truncate(MAX_EMAILS);
                    note(notes, "a card had more than three e-mail addresses; the others were left out");
                }
            }
            "TEL" => {
                let number = without_scheme(&text(p), &["tel:"]).trim().to_string();
                if number.is_empty() {
                    continue;
                }
                match phone_kind(p, &c.phones) {
                    Some(kind) => c.set_phone(kind, &number),
                    None => note(notes, "a card had more telephone numbers of one kind than a contact keeps; the others were left out"),
                }
            }
            "ADR" => {
                let parts = split(&raw_text(p), ';', 7);
                let at = |i: usize| parts.get(i).map(|s| split(s, ',', MAX_LIST).join(", ")).unwrap_or_default();
                let street = [at(2), at(1)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n");
                let a = Address { po_box: at(0), street, city: at(3), state: at(4), postal_code: at(5), country: at(6) };
                let place = place_of(p);
                if a.is_empty() || c.address(place).is_some() {
                    continue;
                }
                if p.has_type("pref") {
                    mailing = Some(place);
                }
                c.set_address(place, a);
            }
            "URL" => {
                let url = text(p);
                if p.has_type("home") && c.home_page.is_empty() {
                    c.home_page = url;
                } else if c.web_page.is_empty() {
                    c.web_page = url;
                } else if c.home_page.is_empty() {
                    c.home_page = url;
                }
            }
            "BDAY" => match date(&text(p)) {
                Some(d) => c.birthday = Some(d),
                None => note(notes, "a birthday with no year, or not a date, was left out"),
            },
            "ANNIVERSARY" | "X-MS-ANNIVERSARY" | "X-ANNIVERSARY" | "X-EVOLUTION-ANNIVERSARY" => {
                if let Some(d) = date(&text(p)) {
                    c.anniversary = Some(d);
                }
            }
            "NOTE" => {
                let n = cut(unescape(&raw_text(p)).trim(), MAX_NOTE);
                if c.notes.is_empty() {
                    c.notes = n;
                } else if c.notes.chars().count() < MAX_NOTE {
                    c.notes = cut(&format!("{}\n{n}", c.notes), MAX_NOTE);
                }
            }
            "CATEGORIES" => {
                for cat in split(&raw_text(p), ',', MAX_LIST) {
                    if !cat.is_empty() && !c.categories.contains(&cat) && c.categories.len() < MAX_LIST {
                        c.categories.push(cat);
                    }
                }
            }
            "PHOTO" if c.picture.is_none() => match picture(p) {
                Ok(bytes) => c.picture = Some(bytes),
                Err(why) => note(notes, why),
            },
            "X-MS-IMADDRESS" | "IMPP" | "X-JABBER" | "X-AIM" | "X-SKYPE" | "X-MSN" | "X-ICQ" | "X-YAHOO" | "X-GOOGLE-TALK" if c.im.is_empty() => {
                c.im = without_scheme(&text(p), &["xmpp:", "sip:", "im:", "aim:", "skype:", "msnim:", "ymsgr:"]).to_string();
            }
            "X-MS-MANAGER" | "X-MANAGER" => c.manager = text(p),
            "X-MS-ASSISTANT" | "X-ASSISTANT" => c.assistant = text(p),
            "X-MS-SPOUSE" | "X-SPOUSE" => c.spouse = text(p),
            "X-MS-OL-DEFAULT-POSTAL-ADDRESS" => {
                mailing = match text(p).as_str() {
                    "1" => Some(Place::Home),
                    "2" => Some(Place::Business),
                    "3" => Some(Place::Other),
                    _ => mailing,
                };
            }
            "SORT-STRING" => sort_as = text(p),
            "UID" => c.uid = cut(text(p).as_str(), 255),
            "KIND" | "X-ADDRESSBOOKSERVER-KIND" => group = text(p).eq_ignore_ascii_case("group"),
            "MEMBER" | "X-ADDRESSBOOKSERVER-MEMBER" => {
                let v = text(p);
                let address = without_scheme(&v, &["mailto:"]);
                if address.len() == v.len() || !address.contains('@') {
                    note(notes, "a group member named only by a link to another card was left out");
                } else if members.len() < MAX_MEMBERS {
                    members.push(address.trim().to_string());
                    member_names
                        .push(p.param("X-CN").next().or_else(|| p.param("CN").next()).map(|n| cut(&unescape(n), MAX_TEXT)).unwrap_or_default());
                }
            }
            _ => {}
        }
    }
    c.mailing = mailing.filter(|m| c.address(*m).is_some());
    if !sort_as.is_empty() && sort_as != c.default_file_as() {
        c.file_as = sort_as;
    }
    if group {
        let name = if c.full_name.is_empty() { c.display_name() } else { c.full_name.clone() };
        let members = members
            .into_iter()
            .zip(member_names)
            .map(|(address, name)| Member { name: if name.is_empty() { address.clone() } else { name }, address })
            .collect();
        return Card::Group(Group { name, members, notes: c.notes, categories: c.categories, uid: c.uid });
    }
    Card::Contact(Box::new(c))
}

/// Reads every card of a vCard file. A card that is cut off at the end is kept as far as it
/// goes; a file with no card at all is an error.
pub fn read(bytes: &[u8]) -> Result<Cards, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("the file is larger than {} MB", MAX_BYTES >> 20));
    }
    let mut out = Cards::default();
    let mut props: Vec<Prop> = Vec::new();
    let mut depth = 0usize;
    let mut seen = false;
    for line in unfold(bytes) {
        let Some(p) = content_line(&line) else { continue };
        match p.name.as_str() {
            "BEGIN" if p.value.trim().eq_ignore_ascii_case("VCARD") => {
                if depth == 0 {
                    props.clear();
                }
                depth = depth.saturating_add(1).min(MAX_DEPTH + 1);
                seen = true;
            }
            "END" if p.value.trim().eq_ignore_ascii_case("VCARD") && depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    if out.cards.len() >= MAX_CARDS {
                        note(&mut out.notes, format!("only the first {MAX_CARDS} cards were read"));
                        break;
                    }
                    out.cards.push(card_of(&props, &mut out.notes));
                    props.clear();
                }
            }
            _ if depth == 1 && props.len() < MAX_PROPS => props.push(p),
            _ => {}
        }
    }
    if depth > 0 && !props.is_empty() && out.cards.len() < MAX_CARDS {
        note(&mut out.notes, "the file ended inside a card; it was read as far as it went");
        out.cards.push(card_of(&props, &mut out.notes));
    }
    if !seen {
        return Err("there is no card in the file".into());
    }
    Ok(out)
}

/// A content line folded at 75 bytes (RFC 6350 3.2).
fn line(out: &mut String, text: &str) {
    let mut used = 0;
    for c in text.chars() {
        let n = c.len_utf8();
        if used + n > 75 {
            out.push_str("\r\n ");
            used = 1;
        }
        out.push(c);
        used += n;
    }
    out.push_str("\r\n");
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(A.get(((n >> shift) & 63) as usize).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn pair(out: &mut String, name: &str, value: &str) {
    if !value.trim().is_empty() {
        line(out, &format!("{name}:{}", escape(value.trim())));
    }
}

/// A contact written as a vCard 3.0 card, as Outlook reads them ([MS-OXVCARD] 2.1.3). `uid`
/// is the one it is written with when it has none of its own; `now` (FILETIME) is its REV.
pub fn write_contact(out: &mut String, c: &Contact, uid: &str, now: i64) {
    line(out, "BEGIN:VCARD");
    line(out, "VERSION:3.0");
    let n = [&c.surname, &c.given, &c.middle, &c.prefix, &c.suffix].map(|s| escape(s.trim())).join(";");
    line(out, &format!("N:{n}"));
    line(out, &format!("FN:{}", escape(&c.display_name())));
    if !c.file_as.trim().is_empty() && c.file_as.trim() != c.default_file_as() {
        pair(out, "SORT-STRING", &c.file_as);
    }
    pair(out, "NICKNAME", &c.nickname);
    if !c.company.trim().is_empty() || !c.department.trim().is_empty() {
        line(out, &format!("ORG:{};{}", escape(c.company.trim()), escape(c.department.trim())));
    }
    pair(out, "TITLE", &c.job_title);
    pair(out, "ROLE", &c.profession);
    for (n, e) in c.emails.iter().take(MAX_EMAILS).enumerate() {
        let pref = if n == 0 { ",PREF" } else { "" };
        line(out, &format!("EMAIL;TYPE=INTERNET{pref}:{}", escape(e.address.trim())));
    }
    for (kind, number) in &c.phones {
        let types = match kind {
            Phone::Business | Phone::Business2 => "WORK,VOICE",
            Phone::BusinessFax => "WORK,FAX",
            Phone::Home | Phone::Home2 => "HOME,VOICE",
            Phone::HomeFax => "HOME,FAX",
            Phone::Mobile => "CELL,VOICE",
            Phone::Pager => "PAGER",
            Phone::Car => "CAR,VOICE",
            Phone::Assistant => "VOICE,X-ASSISTANT",
            Phone::Primary => "PREF,VOICE",
            Phone::Other => "VOICE",
        };
        line(out, &format!("TEL;TYPE={types}:{}", escape(number.trim())));
    }
    for (place, a) in &c.addresses {
        let kind = match place {
            Place::Business => "WORK",
            Place::Home => "HOME",
            Place::Other => "POSTAL",
        };
        let pref = if c.mailing == Some(*place) { ",PREF" } else { "" };
        let parts = [&a.po_box, &String::new(), &a.street, &a.city, &a.state, &a.postal_code, &a.country].map(|s| escape(s.trim())).join(";");
        line(out, &format!("ADR;TYPE={kind}{pref}:{parts}"));
    }
    if let Some(m) = c.mailing {
        let id = match m {
            Place::Home => 1,
            Place::Business => 2,
            Place::Other => 3,
        };
        line(out, &format!("X-MS-OL-DEFAULT-POSTAL-ADDRESS:{id}"));
    }
    if !c.web_page.trim().is_empty() {
        line(out, &format!("URL;TYPE=WORK:{}", escape(c.web_page.trim())));
    }
    if !c.home_page.trim().is_empty() {
        line(out, &format!("URL;TYPE=HOME:{}", escape(c.home_page.trim())));
    }
    pair(out, "X-MS-IMADDRESS", &c.im);
    if let Some(d) = c.birthday {
        line(out, &format!("BDAY:{}", date_text(d)));
    }
    if let Some(d) = c.anniversary {
        line(out, &format!("X-MS-ANNIVERSARY:{}", date_text(d)));
    }
    pair(out, "X-MS-MANAGER", &c.manager);
    pair(out, "X-MS-ASSISTANT", &c.assistant);
    pair(out, "X-MS-SPOUSE", &c.spouse);
    if !c.categories.is_empty() {
        line(out, &format!("CATEGORIES:{}", c.categories.iter().map(|s| escape(s)).collect::<Vec<_>>().join(",")));
    }
    pair(out, "NOTE", &c.notes);
    if let Some(p) = c.picture.as_ref().filter(|p| !p.is_empty() && p.len() <= MAX_PICTURE) {
        let kind = match contact::picture_kind(p).0 {
            "png" => "PNG",
            "gif" => "GIF",
            "bmp" => "BMP",
            _ => "JPEG",
        };
        line(out, &format!("PHOTO;ENCODING=b;TYPE={kind}:{}", base64(p)));
    }
    let uid = if c.uid.trim().is_empty() { uid } else { c.uid.trim() };
    pair(out, "UID", uid);
    line(out, &format!("REV:{}", stamp(now)));
    line(out, "END:VCARD");
}

/// A group written as a vCard 4.0 card (RFC 6350 6.1.4, 6.6.5): Outlook does not write its
/// groups as cards, so they are written as the standard has them, each member by its address
/// with its name beside it.
pub fn write_group(out: &mut String, g: &Group, uid: &str, now: i64) {
    line(out, "BEGIN:VCARD");
    line(out, "VERSION:4.0");
    line(out, "KIND:group");
    line(out, &format!("FN:{}", escape(g.name.trim())));
    for m in g.members.iter().take(MAX_MEMBERS) {
        let name = m.name.trim().replace(['"', '\r', '\n'], "");
        let param = if name.is_empty() || name == m.address.trim() { String::new() } else { format!(";X-CN=\"{name}\"") };
        line(out, &format!("MEMBER{param}:mailto:{}", m.address.trim().replace([',', ';', '\r', '\n'], "")));
    }
    if !g.categories.is_empty() {
        line(out, &format!("CATEGORIES:{}", g.categories.iter().map(|s| escape(s)).collect::<Vec<_>>().join(",")));
    }
    pair(out, "NOTE", &g.notes);
    let uid = if g.uid.trim().is_empty() { uid } else { g.uid.trim() };
    pair(out, "UID", uid);
    line(out, &format!("REV:{}", stamp(now)));
    line(out, "END:VCARD");
}

fn stamp(t: i64) -> String {
    let c = time::civil(t, 0);
    format!("{:04}{:02}{:02}T{:02}{:02}{:02}Z", c.year, c.month, c.day, c.hour, c.minute, c.second)
}

/// Cards written one after another into one file.
pub fn write(cards: &[(Card, String)], now: i64) -> String {
    let mut out = String::new();
    for (card, uid) in cards {
        match card {
            Card::Contact(c) => write_contact(&mut out, c, uid, now),
            Card::Group(g) => write_group(&mut out, g, uid, now),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Contact {
        match read(text.as_bytes()).unwrap().cards.into_iter().next().unwrap() {
            Card::Contact(c) => *c,
            Card::Group(_) => panic!("a group"),
        }
    }

    #[test]
    fn a_contact_written_is_read_back_as_it_was() {
        let mut c = Contact {
            prefix: "Dr.".into(),
            given: "Zoë".into(),
            middle: "Q".into(),
            surname: "Example; Jr".into(),
            company: "Acme, Inc.".into(),
            department: "R&D".into(),
            job_title: "Lead".into(),
            nickname: "Zed".into(),
            emails: vec![
                Email { address: "zoe@example.com".into(), name: String::new() },
                Email { address: "z@example.org".into(), name: String::new() },
            ],
            birthday: Some(7_000),
            anniversary: Some(12_000),
            notes: "Line one\nLine two, with a comma; and a semicolon".into(),
            categories: vec!["Work".into(), "VIP, really".into()],
            web_page: "https://example.com/zoe".into(),
            home_page: "https://example.org".into(),
            im: "zoe@chat.example".into(),
            manager: "Boss".into(),
            picture: Some((0u8..=255).cycle().take(3000).collect()),
            uid: "abc-123".into(),
            file_as: "Zed!".into(),
            full_name: "Dr. Zoë Q Example; Jr".into(),
            ..Contact::default()
        };
        c.set_phone(Phone::Mobile, "+1 555 0100");
        c.set_phone(Phone::BusinessFax, "+1 555 0101");
        c.set_phone(Phone::Business, "+1 555 0102");
        c.set_phone(Phone::Home2, "+1 555 0103");
        c.set_address(
            Place::Business,
            Address {
                street: "1 Long Road\nSuite 2".into(),
                city: "Town".into(),
                state: "ST".into(),
                postal_code: "12345".into(),
                country: "Land".into(),
                po_box: "PO 9".into(),
            },
        );
        c.set_address(Place::Home, Address { street: "3 Home Way".into(), ..Address::default() });
        c.mailing = Some(Place::Home);
        let mut out = String::new();
        write_contact(&mut out, &c, "unused", time::from_unix(0));
        assert!(out.lines().all(|l| l.len() <= 76));
        let back = one(&out);
        let mut want = c.clone();
        // A second home number is read as the first, as the card does not tell them apart.
        want.set_phone(Phone::Home2, "");
        want.set_phone(Phone::Home, "+1 555 0103");
        assert_eq!(back, want);
    }

    #[test]
    fn an_outlook_style_2_1_card_is_read() {
        let card = "BEGIN:VCARD\r\nVERSION:2.1\r\nN;LANGUAGE=en-us:Smith;Jane\r\nFN:Jane Smith\r\nORG:Widgets\r\nTEL;WORK;VOICE:555-1000\r\nTEL;CELL;VOICE:555-2000\r\nTEL;WORK;FAX:555-3000\r\nADR;WORK;PREF:;;1 Main St;Springfield;IL;62701;USA\r\nEMAIL;PREF;INTERNET:jane@example.com\r\nNOTE;ENCODING=QUOTED-PRINTABLE;CHARSET=utf-8:Caf=C3=A9 notes=0D=0Aon two =\r\nlines\r\nBDAY:19800215\r\nX-MS-OL-DEFAULT-POSTAL-ADDRESS:2\r\nEND:VCARD\r\n";
        let c = one(card);
        assert_eq!((c.given.as_str(), c.surname.as_str(), c.company.as_str()), ("Jane", "Smith", "Widgets"));
        assert_eq!(c.phone(Phone::Business), Some("555-1000"));
        assert_eq!(c.phone(Phone::Mobile), Some("555-2000"));
        assert_eq!(c.phone(Phone::BusinessFax), Some("555-3000"));
        assert_eq!(c.address(Place::Business).map(|a| a.city.as_str()), Some("Springfield"));
        assert_eq!(c.mailing, Some(Place::Business));
        assert_eq!(c.emails[0].address, "jane@example.com");
        assert_eq!(c.notes, "Café notes\r\non two lines".replace('\r', ""));
        assert_eq!(c.birthday.map(date_text).as_deref(), Some("1980-02-15"));
    }

    #[test]
    fn a_4_0_card_with_uris_and_a_data_picture_is_read() {
        let card = "BEGIN:VCARD\nVERSION:4.0\nFN:Ann\nTEL;VALUE=uri;TYPE=\"voice,home\":tel:+1-555-0200\nEMAIL:ann@example.com\nIMPP:xmpp:ann@chat.example\nPHOTO:data:image/png;base64,iVBORw0KGgo=\nANNIVERSARY:2001-06-01\nBDAY:--0203\nitem1.URL:https://ann.example\nEND:VCARD\n";
        let r = read(card.as_bytes()).unwrap();
        let Card::Contact(c) = &r.cards[0] else { panic!() };
        assert_eq!(c.phone(Phone::Home), Some("+1-555-0200"));
        assert_eq!(c.im, "ann@chat.example");
        assert_eq!(c.picture.as_deref().map(|p| &p[..4]), Some(&[0x89, b'P', b'N', b'G'][..]));
        assert_eq!(c.anniversary.map(date_text).as_deref(), Some("2001-06-01"));
        assert_eq!(c.birthday, None);
        assert_eq!(c.web_page, "https://ann.example");
        assert!(r.notes.iter().any(|n| n.contains("no year")));
    }

    #[test]
    fn a_picture_named_by_a_link_is_not_fetched() {
        let r = read(b"BEGIN:VCARD\nVERSION:4.0\nFN:L\nPHOTO:https://tracker.example/p.jpg\nEND:VCARD\n").unwrap();
        let Card::Contact(c) = &r.cards[0] else { panic!() };
        assert!(c.picture.is_none());
        assert!(r.notes.iter().any(|n| n.contains("not fetched")));
    }

    #[test]
    fn a_group_is_written_and_read_with_its_members() {
        let g = Group {
            name: "Team, A".into(),
            members: vec![
                Member { name: "Ada \"A\" L".into(), address: "ada@example.com".into() },
                Member { name: "bob@example.com".into(), address: "bob@example.com".into() },
            ],
            notes: "n".into(),
            categories: vec![],
            uid: String::new(),
        };
        let mut out = String::new();
        write_group(&mut out, &g, "g-1", 0);
        let r = read(out.as_bytes()).unwrap();
        let Card::Group(back) = &r.cards[0] else { panic!() };
        assert_eq!(back.name, "Team, A");
        assert_eq!(back.members[0], Member { name: "Ada A L".into(), address: "ada@example.com".into() });
        assert_eq!(back.members[1], g.members[1]);
        assert_eq!(back.uid, "g-1");
        // An Apple group's members named by card links are said, not guessed.
        let apple =
            read(b"BEGIN:VCARD\nVERSION:3.0\nFN:G\nX-ADDRESSBOOKSERVER-KIND:group\nX-ADDRESSBOOKSERVER-MEMBER:urn:uuid:1\nEND:VCARD\n").unwrap();
        assert!(matches!(&apple.cards[0], Card::Group(g) if g.members.is_empty()));
        assert!(!apple.notes.is_empty());
    }

    #[test]
    fn several_cards_and_a_card_cut_off_are_read() {
        let r = read(b"BEGIN:VCARD\nFN:One\nEND:VCARD\nBEGIN:VCARD\nFN:Two\nAGENT:BEGIN:VCARD\nBEGIN:VCARD\nFN:Inner\nEND:VCARD\nEND:VCARD\nBEGIN:VCARD\nFN:Three").unwrap();
        let names: Vec<String> = r.cards.iter().map(Card::name).collect();
        assert_eq!(names, ["One", "Two", "Three"]);
        assert!(read(b"not a card").is_err());
        assert!(read(b"").is_err());
    }

    #[test]
    fn hostile_cards_are_read_without_a_panic_and_within_bounds() {
        let mut big = String::from("BEGIN:VCARD\n");
        for i in 0..5000 {
            big.push_str(&format!("EMAIL:a{i}@x\nTEL;WORK:{i}\nCATEGORIES:{}\n", ",x".repeat(50)));
        }
        let r = read(big.as_bytes()).unwrap();
        let Card::Contact(c) = &r.cards[0] else { panic!() };
        assert_eq!(c.emails.len(), MAX_EMAILS);
        assert!(c.phones.len() <= Phone::ALL.len());
        // Truncated anywhere, garbage, odd escapes and quotes, broken encodings.
        let samples: [&[u8]; 8] = [
            b"BEGIN:VCARD\nN;SORT-AS=\"unclosed:x\nEND:VCARD",
            b"BEGIN:VCARD\nNOTE;ENCODING=QUOTED-PRINTABLE:=\n=Z=\n\nEND:VCARD",
            b"BEGIN:VCARD\nPHOTO;ENCODING=b:!!!!\nPHOTO:data:,x\nPHOTO:data:image/png;base64,\nEND:VCARD",
            b"BEGIN:VCARD\nN:\\\nADR:;;;;;;;;;;;;\\\nBDAY:99999999\nBDAY:2023-02-30\nEND:VCARD",
            b"BEGIN:VCARD\n\xff\xfe\x00:\x80\nFN:\xe9t\xe9\nEND:VCARD",
            b"END:VCARD\nEND:VCARD\nBEGIN:VCARD\nMEMBER:mailto:\nKIND:group\nEND:VCARD",
            b"BEGIN:VCARD\n;;;:\n:\n.:\nTEL:\nEMAIL:\nEND:VCARD",
            b"BEGIN:VCARD\nTEL;TYPE=a,b,c,d:1\n TEL;a;b;c;d;e;f;g;h:2\nEND:VCARD",
        ];
        for s in samples {
            for n in 0..=s.len() {
                let _ = read(&s[..n]);
            }
        }
        let latin = read(b"BEGIN:VCARD\nFN:\xe9t\xe9\nEND:VCARD").unwrap();
        assert_eq!(latin.cards[0].name(), "été");
        // Deep nesting and many cards.
        let deep = "BEGIN:VCARD\n".repeat(100) + &"END:VCARD\n".repeat(100);
        assert_eq!(read(deep.as_bytes()).unwrap().cards.len(), 1);
        let many = "BEGIN:VCARD\nFN:x\nEND:VCARD\n".repeat(MAX_CARDS + 5);
        let r = read(many.as_bytes()).unwrap();
        assert_eq!(r.cards.len(), MAX_CARDS);
        assert!(read(&vec![b'a'; MAX_BYTES + 1]).is_err());
    }

    #[test]
    fn base64_is_as_the_reader_takes_it() {
        for n in 0..10 {
            let data: Vec<u8> = (0..n).map(|i| (i * 37) as u8).collect();
            assert_eq!(crate::html::base64(&base64(&data)), Some(data));
        }
    }
}
