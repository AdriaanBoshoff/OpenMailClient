//! Mail kept on an IMAP server ([RFC 9051], and [RFC 3501], which most servers still speak):
//! folders, the messages in them and their flags. Made secure with STARTTLS or TLS from the
//! start; signed in with SASL PLAIN ([RFC 4616]) or LOGIN.

use crate::conn::{Conn, MAX_LINE};
use crate::{Connector, Endpoint, Login, MAX_MESSAGE, NetError, Security, one_line, said, sasl_plain};

/// Lists inside lists in one response.
const MAX_DEPTH: usize = 32;
/// Literals in one response.
const MAX_LITERALS: usize = 1_000;
/// Untagged responses kept while one command runs.
const MAX_UNTAGGED: usize = 1_000_000;
/// Items in one list.
const MAX_ITEMS: usize = 100_000;
/// Folders taken from LIST.
pub const MAX_FOLDERS: usize = 100_000;

/// A piece of what a server sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tok {
    Atom(String),
    /// A quoted string or a literal.
    Str(Vec<u8>),
    List(Vec<Tok>),
    Nil,
}

impl Tok {
    pub fn atom(&self) -> Option<&str> {
        match self {
            Self::Atom(a) => Some(a),
            _ => None,
        }
    }

    pub fn number(&self) -> Option<u64> {
        self.atom().and_then(|a| a.parse().ok())
    }

    /// An astring or nstring as text.
    pub fn text(&self) -> Option<String> {
        match self {
            Self::Atom(a) => Some(a.clone()),
            Self::Str(s) => Some(String::from_utf8_lossy(s).into_owned()),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ok,
    No,
    Bad,
    Bye,
    Preauth,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resp {
    /// A status response; `tag` is `None` for an untagged one.
    Status { tag: Option<String>, kind: Kind, code: Option<String>, text: String },
    /// Untagged data: `* 3 EXISTS` has the number 3 and the name EXISTS.
    Data { number: Option<u64>, name: String, rest: Vec<Tok> },
    /// `+`: go on.
    Go(String),
}

/// One response, read whole: a line, and while it ends in a literal's count, that many bytes
/// and the line after them.
pub fn read(c: &mut Conn) -> Result<Resp, NetError> {
    parse(&read_raw(c, None)?)
}

/// A response's bytes, literals in place. `go`, when given, is said before each literal is
/// read (the fake server's `+` to a client's synchronizing literal).
pub(crate) fn read_raw(c: &mut Conn, go: Option<&[u8]>) -> Result<Vec<u8>, NetError> {
    let mut raw = Vec::new();
    let mut literals = 0;
    loop {
        let line = c.line()?;
        raw.extend_from_slice(&line);
        let Some(n) = literal_at_end(&line) else { break };
        literals += 1;
        if literals > MAX_LITERALS {
            return Err(NetError::TooLarge("a response from the server"));
        }
        let n = usize::try_from(n)
            .ok()
            .filter(|n| raw.len().saturating_add(*n) <= MAX_MESSAGE + MAX_LINE)
            .ok_or(NetError::TooLarge("a literal from the server"))?;
        raw.extend_from_slice(b"\r\n");
        if let Some(go) = go
            && !line.ends_with(b"+}")
        {
            c.send(go)?;
        }
        raw.extend_from_slice(&c.exact(n)?);
    }
    Ok(raw)
}

/// The count of a literal a line ends with: `{12}` or `{12+}`.
fn literal_at_end(line: &[u8]) -> Option<u64> {
    let body = line.strip_suffix(b"}")?;
    let open = body.iter().rposition(|b| *b == b'{')?;
    let digits = body.get(open + 1..)?;
    let digits = digits.strip_suffix(b"+").unwrap_or(digits);
    if digits.is_empty() || digits.len() > 19 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

/// One response from its bytes, literals in place.
pub fn parse(raw: &[u8]) -> Result<Resp, NetError> {
    let mut p = Parser { b: raw, at: 0 };
    if raw.first() == Some(&b'+') {
        return Ok(Resp::Go(p.rest_text(1)));
    }
    let tag = p.word();
    if tag.is_empty() {
        return Err(NetError::protocol("an empty response"));
    }
    p.space();
    let second = p.word();
    let kind = match second.to_ascii_uppercase().as_str() {
        "OK" => Some(Kind::Ok),
        "NO" => Some(Kind::No),
        "BAD" => Some(Kind::Bad),
        "BYE" => Some(Kind::Bye),
        "PREAUTH" => Some(Kind::Preauth),
        _ => None,
    };
    if let Some(kind) = kind {
        p.space();
        let (code, text) = p.status_text();
        return Ok(Resp::Status { tag: (tag != "*").then_some(tag), kind, code, text });
    }
    if tag != "*" {
        return Err(NetError::protocol(format!("a tagged response that is not a status: {tag} {second}")));
    }
    let (number, name) = match second.parse::<u64>() {
        Ok(n) => {
            p.space();
            (Some(n), p.word().to_ascii_uppercase())
        }
        Err(_) => (None, second.to_ascii_uppercase()),
    };
    let rest = p.items(0, false)?;
    Ok(Resp::Data { number, name, rest })
}

/// The items of a line a client sent, for the fake server.
#[cfg(feature = "fake")]
pub(crate) fn tokens(raw: &[u8]) -> Result<Vec<Tok>, NetError> {
    Parser { b: raw, at: 0 }.items(0, false)
}

struct Parser<'a> {
    b: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.at).copied()
    }

    fn space(&mut self) {
        while self.peek() == Some(b' ') {
            self.at += 1;
        }
    }

    fn word(&mut self) -> String {
        let start = self.at;
        while self.peek().is_some_and(|b| b != b' ') {
            self.at += 1;
        }
        String::from_utf8_lossy(self.b.get(start..self.at).unwrap_or_default()).into_owned()
    }

    fn rest_text(&mut self, from: usize) -> String {
        let text = String::from_utf8_lossy(self.b.get(from.max(self.at)..).unwrap_or_default()).trim().to_string();
        self.at = self.b.len();
        text
    }

    /// `[CODE ...] text`, the code without its brackets.
    fn status_text(&mut self) -> (Option<String>, String) {
        if self.peek() == Some(b'[') {
            let start = self.at + 1;
            let mut depth = 0usize;
            while let Some(b) = self.peek() {
                self.at += 1;
                match b {
                    b'[' => depth += 1,
                    b']' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            let code = String::from_utf8_lossy(self.b.get(start..self.at - 1).unwrap_or_default()).into_owned();
                            return (Some(code), self.rest_text(self.at));
                        }
                    }
                    _ => {}
                }
            }
            self.at = start - 1;
        }
        (None, self.rest_text(self.at))
    }

    /// Items up to the end, or to the `)` that closes a list.
    fn items(&mut self, depth: usize, in_list: bool) -> Result<Vec<Tok>, NetError> {
        if depth > MAX_DEPTH {
            return Err(NetError::TooLarge("lists inside lists"));
        }
        let mut out = Vec::new();
        loop {
            self.space();
            match self.peek() {
                None if in_list => return Err(NetError::protocol("a list that is not closed")),
                None => return Ok(out),
                Some(b')') if in_list => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b')') => return Err(NetError::protocol("a list closed that was not opened")),
                Some(b'(') => {
                    self.at += 1;
                    out.push(Tok::List(self.items(depth + 1, true)?));
                }
                Some(b'"') => out.push(Tok::Str(self.quoted()?)),
                Some(b'{') => out.push(Tok::Str(self.literal()?)),
                Some(_) => {
                    let a = self.atom()?;
                    out.push(if a.eq_ignore_ascii_case("NIL") { Tok::Nil } else { Tok::Atom(a) });
                }
            }
            if out.len() > MAX_ITEMS {
                return Err(NetError::TooLarge("a list from the server"));
            }
        }
    }

    fn quoted(&mut self) -> Result<Vec<u8>, NetError> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None | Some(b'\r' | b'\n') => return Err(NetError::protocol("a quoted string that is not closed")),
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    if let Some(b) = self.peek() {
                        out.push(b);
                        self.at += 1;
                    }
                }
                Some(b) => {
                    out.push(b);
                    self.at += 1;
                }
            }
        }
    }

    fn literal(&mut self) -> Result<Vec<u8>, NetError> {
        let close =
            self.b.get(self.at..).and_then(|r| r.iter().position(|b| *b == b'}')).ok_or_else(|| NetError::protocol("a literal without its count"))?;
        let count = self.b.get(self.at + 1..self.at + close).unwrap_or_default();
        let count = count.strip_suffix(b"+").unwrap_or(count);
        let n: usize = std::str::from_utf8(count)
            .ok()
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| NetError::protocol("a literal's count that is not a number"))?;
        self.at += close + 1;
        if self.b.get(self.at..self.at + 2) != Some(b"\r\n") {
            return Err(NetError::protocol("a literal not on a line of its own"));
        }
        self.at += 2;
        let end = self.at.checked_add(n).filter(|e| *e <= self.b.len()).ok_or_else(|| NetError::protocol("a literal shorter than its count"))?;
        let out = self.b.get(self.at..end).unwrap_or_default().to_vec();
        self.at = end;
        Ok(out)
    }

    /// An atom, with a `[section]` and a `<partial>` kept in it (`BODY[HEADER.FIELDS (A B)]<0>`).
    fn atom(&mut self) -> Result<String, NetError> {
        let start = self.at;
        let mut depth = 0usize;
        while let Some(b) = self.peek() {
            match b {
                b'[' => depth += 1,
                b']' => depth = depth.saturating_sub(1),
                b' ' | b'(' | b')' | b'\r' | b'\n' if depth == 0 => break,
                b'\r' | b'\n' => return Err(NetError::protocol("a section that is not closed")),
                _ => {}
            }
            self.at += 1;
        }
        if self.at == start {
            return Err(NetError::protocol("an empty atom"));
        }
        Ok(String::from_utf8_lossy(self.b.get(start..self.at).unwrap_or_default()).into_owned())
    }
}

/// A part of a command: text, or bytes sent as a literal.
enum Part {
    Text(String),
    Lit(Vec<u8>),
}

/// A string as a command carries it: quoted when it can be, as a literal otherwise.
fn astring(s: &[u8]) -> Part {
    if s.iter().all(|b| (0x20..0x7F).contains(b)) {
        let mut q = String::with_capacity(s.len() + 2);
        q.push('"');
        for &b in s {
            if b == b'"' || b == b'\\' {
                q.push('\\');
            }
            q.push(char::from(b));
        }
        q.push('"');
        Part::Text(q)
    } else {
        Part::Lit(s.to_vec())
    }
}

/// A folder on the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    /// The name as it reads.
    pub name: String,
    /// The name as the server knows it, for commands.
    pub raw: String,
    pub delimiter: Option<char>,
    /// `\Noselect`, `\HasChildren`, and the special uses `\Sent`, `\Drafts`, `\Trash`,
    /// `\Junk`, `\Archive` ([RFC 6154]).
    pub attributes: Vec<String>,
}

impl Folder {
    pub fn has(&self, attribute: &str) -> bool {
        self.attributes.iter().any(|a| a.eq_ignore_ascii_case(attribute))
    }

    pub fn selectable(&self) -> bool {
        !self.has("\\Noselect") && !self.has("\\NonExistent")
    }
}

/// A folder once selected.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selected {
    pub exists: u64,
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub read_only: bool,
    /// The folder's highest mod-sequence, where the server keeps them ([RFC 7162] 3.1.2.1);
    /// none where it does not, or says `NOMODSEQ`.
    pub highest_modseq: Option<u64>,
    /// Selected with what was known of it (QRESYNC, [RFC 7162] 3.2.5): the messages the server
    /// says went since, as it says them, and those whose flags changed.
    pub since: Option<Since>,
    /// The server's own id for the folder, which stays with it when it is renamed or moved
    /// ([RFC 8474] 4.1), where the server gives one.
    pub mailbox_id: Option<String>,
}

/// What STATUS says of a folder that is not selected ([RFC 9051] 6.3.11).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub uid_validity: Option<u32>,
    /// The server's own id for the folder ([RFC 8474] 4.2), where it gives one.
    pub mailbox_id: Option<String>,
}

/// A folder's id as a server gives it ([RFC 8474] 4: objectid, 1 to 255 letters, digits, `-`
/// and `_`); anything else is no id.
fn object_id(text: &str) -> Option<String> {
    let id = text.trim().trim_start_matches('(').trim_end_matches(')').trim();
    (!id.is_empty() && id.len() <= 255 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')).then(|| id.to_string())
}

/// What a folder selected with QRESYNC says changed since the mod-sequence given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Since {
    /// The ids the server says went. It may name ids that never were; that a message named is
    /// gone is to be made sure of before anything goes here ([`Imap::still_there`]).
    pub vanished: UidSet,
    /// Messages whose flags changed, with their flags now.
    pub changed: Vec<Summary>,
    /// More was said than is kept: what went is not known from this.
    pub too_much: bool,
}

/// What a folder watched with IDLE said ([RFC 2177]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Woke {
    /// How many messages it holds now, when it said so.
    pub exists: Option<u64>,
    /// A message came, went or had its flags changed.
    pub changed: bool,
}

impl Woke {
    fn hear(&mut self, r: &Resp) {
        match r {
            Resp::Data { number: Some(n), name, .. } if name == "EXISTS" => {
                self.exists = Some(*n);
                self.changed = true;
            }
            Resp::Data { name, .. } if matches!(name.as_str(), "EXPUNGE" | "FETCH" | "VANISHED") => self.changed = true,
            _ => {}
        }
    }
}

/// Responses heard while watching that change nothing, before the server is taken to be
/// saying nonsense.
const MAX_IDLE_CHATTER: usize = 10_000;

/// A set of message ids as a server writes it, `3,5:9,20:*`, kept as its runs: a run of four
/// thousand million ids takes no more room than one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UidSet {
    runs: Vec<(u32, u32)>,
}

/// Runs of one set kept at most.
const MAX_RUNS: usize = 100_000;

impl UidSet {
    /// As a server writes it; none when it is not one, or has more runs than are kept.
    pub fn parse(text: &str) -> Option<Self> {
        let mut runs = Vec::new();
        for part in text.split(',') {
            let n = |t: &str| if t == "*" { Some(u32::MAX) } else { t.parse::<u32>().ok().filter(|n| *n > 0) };
            let (lo, hi) = match part.split_once(':') {
                Some((a, b)) => (n(a)?, n(b)?),
                None => (n(part)?, n(part)?),
            };
            runs.push((lo.min(hi), lo.max(hi)));
            if runs.len() > MAX_RUNS {
                return None;
            }
        }
        Some(Self { runs })
    }

    pub fn contains(&self, uid: u32) -> bool {
        self.runs.iter().any(|(lo, hi)| (*lo..=*hi).contains(&uid))
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    fn add(&mut self, other: Self) -> bool {
        self.runs.extend(other.runs);
        self.runs.len() <= MAX_RUNS
    }
}

/// A message as listed: its id, flags and size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub uid: u32,
    pub flags: Vec<String>,
    pub size: u64,
}

pub struct Imap {
    c: Conn,
    tag: u32,
    caps: Vec<String>,
    /// QRESYNC turned on ([RFC 7162] 3.2.3).
    qresync: bool,
}

impl Imap {
    pub fn open(with: &dyn Connector, to: &Endpoint, login: &Login) -> Result<Self, NetError> {
        let (mut i, preauth) = Self::reach(with, to)?;
        if !preauth {
            i.sign_in(login)?;
        }
        i.capability()?;
        if i.has("QRESYNC") {
            i.enable_qresync()?;
        }
        Ok(i)
    }

    /// Whether the server greets us as an IMAP server and the connection is made secure;
    /// nothing is signed in to, and the connection is left with LOGOUT.
    pub fn greets(with: &dyn Connector, to: &Endpoint) -> Result<(), NetError> {
        Self::reach(with, to)?.0.logout()
    }

    /// The greeting, and the connection made secure; whether the server signed us in already.
    fn reach(with: &dyn Connector, to: &Endpoint) -> Result<(Self, bool), NetError> {
        let mut i = Self { c: Conn::new(with.connect(to)?), tag: 0, caps: Vec::new(), qresync: false };
        let preauth = match read(&mut i.c)? {
            Resp::Status { tag: None, kind: Kind::Ok, code, .. } => {
                i.caps_from(code.as_deref());
                false
            }
            Resp::Status { tag: None, kind: Kind::Preauth, .. } => true,
            Resp::Status { tag: None, kind: Kind::Bye, text, .. } => return Err(NetError::Refused(text)),
            _ => return Err(NetError::protocol("not a greeting")),
        };
        let mut secure = to.security == Security::Tls;
        if to.security == Security::StartTls {
            if preauth {
                // Signed in before it could be made secure: nothing about the connection can
                // be trusted ([RFC 9051] 7.1.4).
                return Err(NetError::NotSecure);
            }
            if i.caps.is_empty() {
                i.capability()?;
            }
            if !i.has("STARTTLS") {
                return Err(NetError::NotSecure);
            }
            i.run(vec![Part::Text("STARTTLS".into())])?;
            let Self { c, tag, .. } = i;
            i = Self { c: c.secure(with, to)?, tag, caps: Vec::new(), qresync: false };
            secure = true;
        }
        if !secure {
            return Err(NetError::NotSecure);
        }
        Ok((i, preauth))
    }

    /// QRESYNC is turned on where the server has it; one that refuses leaves it off.
    fn enable_qresync(&mut self) -> Result<(), NetError> {
        let data = match self.run(vec![Part::Text("ENABLE QRESYNC".into())]) {
            Ok((data, _)) => data,
            Err(NetError::Refused(_)) => return Ok(()),
            Err(NetError::Protocol(_)) if !self.has("ENABLE") => return Ok(()),
            Err(e) => return Err(e),
        };
        self.qresync = data.iter().any(|d| {
            matches!(d, Resp::Data { name, rest, .. } if name == "ENABLED" && rest.iter().any(|t| t.atom().is_some_and(|a| a.eq_ignore_ascii_case("QRESYNC"))))
        });
        Ok(())
    }

    /// The server keeps a mod-sequence for every change ([RFC 7162] CONDSTORE).
    pub fn has_modseqs(&self) -> bool {
        self.qresync || self.has("CONDSTORE")
    }

    /// QRESYNC is on: a folder selected with what was known of it says what went since.
    pub fn qresync(&self) -> bool {
        self.qresync
    }

    fn caps_from(&mut self, code: Option<&str>) {
        if let Some(rest) = code.and_then(|c| c.strip_prefix("CAPABILITY ")) {
            self.caps = rest.split_whitespace().map(|c| c.to_ascii_uppercase()).collect();
        }
    }

    pub fn capabilities(&self) -> &[String] {
        &self.caps
    }

    pub fn has(&self, cap: &str) -> bool {
        self.caps.iter().any(|c| c.eq_ignore_ascii_case(cap))
    }

    fn capability(&mut self) -> Result<(), NetError> {
        let (data, _) = self.run(vec![Part::Text("CAPABILITY".into())])?;
        for d in data {
            if let Resp::Data { name, rest, .. } = d
                && name == "CAPABILITY"
            {
                self.caps = rest.iter().filter_map(|t| t.atom()).map(|c| c.to_ascii_uppercase()).collect();
            }
        }
        Ok(())
    }

    fn sign_in(&mut self, login: &Login) -> Result<(), NetError> {
        one_line("user name", &login.user)?;
        one_line("password", login.password.expose())?;
        let done = if self.has("AUTH=PLAIN") {
            if self.has("SASL-IR") {
                self.run(vec![Part::Text(format!("AUTHENTICATE PLAIN {}", sasl_plain(login)))])
            } else {
                self.authenticate(&sasl_plain(login))
            }
        } else if self.has("LOGINDISABLED") {
            return Err(NetError::Unsupported("the server signs in only in ways this version does not speak".into()));
        } else {
            self.run(vec![
                Part::Text("LOGIN ".into()),
                astring(login.user.as_bytes()),
                Part::Text(" ".into()),
                astring(login.password.expose().as_bytes()),
            ])
        };
        match done {
            Ok((_, code)) => {
                self.caps_from(code.as_deref());
                Ok(())
            }
            Err(NetError::Refused(w)) => Err(NetError::Auth(w)),
            Err(e) => Err(e),
        }
    }

    /// AUTHENTICATE PLAIN without an initial response: the server asks, we answer.
    fn authenticate(&mut self, answer: &str) -> Result<(Vec<Resp>, Option<String>), NetError> {
        let tag = self.next_tag();
        self.c.send(format!("{tag} AUTHENTICATE PLAIN\r\n").as_bytes())?;
        loop {
            match read(&mut self.c)? {
                Resp::Go(_) => {
                    self.c.send(format!("{answer}\r\n").as_bytes())?;
                    return self.finish(&tag, Vec::new());
                }
                Resp::Status { tag: Some(t), kind, text, .. } if t == tag => return Err(status_error(kind, text)),
                _ => {}
            }
        }
    }

    fn next_tag(&mut self) -> String {
        self.tag = self.tag.wrapping_add(1);
        format!("A{:04}", self.tag)
    }

    /// Send a command and read up to its tagged answer: the untagged responses on the way,
    /// and the code of the answer.
    fn run(&mut self, parts: Vec<Part>) -> Result<(Vec<Resp>, Option<String>), NetError> {
        let tag = self.next_tag();
        let literal_plus = self.has("LITERAL+");
        let mut line = format!("{tag} ").into_bytes();
        let mut data = Vec::new();
        for part in parts {
            match part {
                Part::Text(t) => line.extend_from_slice(t.as_bytes()),
                Part::Lit(bytes) => {
                    if literal_plus || (self.has("LITERAL-") && bytes.len() <= 4096) {
                        line.extend_from_slice(format!("{{{}+}}\r\n", bytes.len()).as_bytes());
                    } else {
                        line.extend_from_slice(format!("{{{}}}\r\n", bytes.len()).as_bytes());
                        self.c.send(&line)?;
                        line.clear();
                        // Wait to be told to go on.
                        loop {
                            match read(&mut self.c)? {
                                Resp::Go(_) => break,
                                Resp::Status { tag: Some(t), kind, text, .. } if t == tag => return Err(status_error(kind, text)),
                                Resp::Status { tag: None, kind: Kind::Bye, text, .. } => return Err(NetError::Refused(text)),
                                other => keep(&mut data, other)?,
                            }
                        }
                    }
                    line.extend_from_slice(&bytes);
                }
            }
        }
        line.extend_from_slice(b"\r\n");
        self.c.send(&line)?;
        self.finish(&tag, data)
    }

    fn finish(&mut self, tag: &str, mut data: Vec<Resp>) -> Result<(Vec<Resp>, Option<String>), NetError> {
        loop {
            match read(&mut self.c)? {
                Resp::Status { tag: Some(t), kind, code, text } if t == tag => {
                    return match kind {
                        Kind::Ok => Ok((data, code)),
                        kind => Err(status_error(kind, text)),
                    };
                }
                Resp::Status { tag: Some(t), .. } => return Err(NetError::protocol(format!("an answer to a command not sent: {t}"))),
                Resp::Status { tag: None, kind: Kind::Bye, text, .. } if !tag.is_empty() => {
                    // BYE comes before the answer to LOGOUT; to anything else it ends the session.
                    data.push(Resp::Status { tag: None, kind: Kind::Bye, code: None, text });
                }
                Resp::Go(_) => return Err(NetError::protocol("asked to go on when nothing waits")),
                other => keep(&mut data, other)?,
            }
        }
    }

    /// Every folder, with its special use when the server says it ([RFC 6154]).
    pub fn folders(&mut self) -> Result<Vec<Folder>, NetError> {
        let command = if self.has("SPECIAL-USE") { "LIST \"\" \"*\" RETURN (SPECIAL-USE)" } else { "LIST \"\" \"*\"" };
        let (data, _) = self.run(vec![Part::Text(command.into())])?;
        let mut out = Vec::new();
        for d in data {
            let Resp::Data { name, rest, .. } = d else { continue };
            if name != "LIST" {
                continue;
            }
            let [Tok::List(attrs), delimiter, raw, ..] = rest.as_slice() else {
                return Err(NetError::protocol("a LIST response that is not one"));
            };
            let Some(raw) = raw.text() else { return Err(NetError::protocol("a folder without a name")) };
            let delimiter = delimiter.text().and_then(|d| d.chars().next());
            let attributes = attrs.iter().filter_map(|a| a.atom()).map(str::to_string).collect();
            let name = if raw.eq_ignore_ascii_case("INBOX") { "Inbox".to_string() } else { crate::utf7::decode(&raw) };
            out.push(Folder { name, raw, delimiter, attributes });
            if out.len() > MAX_FOLDERS {
                return Err(NetError::TooLarge("the list of folders"));
            }
        }
        Ok(out)
    }

    pub fn select(&mut self, raw: &str) -> Result<Selected, NetError> {
        self.select_since(raw, None)
    }

    /// Select a folder; with its UIDVALIDITY and mod-sequence as last seen, where QRESYNC is on,
    /// the server says too what went and what changed since ([RFC 7162] 3.2.5).
    pub fn select_since(&mut self, raw: &str, known: Option<(u32, u64)>) -> Result<Selected, NetError> {
        self.open_folder("SELECT ", raw, known)
    }

    /// Open a folder only to look at it: nothing in it changes for being looked at, not even
    /// which messages are new ([RFC 9051] 6.3.3).
    pub fn examine(&mut self, raw: &str) -> Result<Selected, NetError> {
        self.open_folder("EXAMINE ", raw, None)
    }

    fn open_folder(&mut self, verb: &str, raw: &str, known: Option<(u32, u64)>) -> Result<Selected, NetError> {
        let mut parts = vec![Part::Text(verb.into()), astring(raw.as_bytes())];
        let resync = known.filter(|(v, m)| self.qresync && *v > 0 && *m > 0);
        match resync {
            Some((v, m)) => parts.push(Part::Text(format!(" (QRESYNC ({v} {m}))"))),
            None if self.has("CONDSTORE") && !self.qresync => parts.push(Part::Text(" (CONDSTORE)".into())),
            None => {}
        }
        let (data, code) = self.run(parts)?;
        let mut s = Selected { read_only: code.as_deref().is_some_and(|c| c.eq_ignore_ascii_case("READ-ONLY")), ..Selected::default() };
        let mut since = Since::default();
        for d in data {
            match d {
                Resp::Data { number: Some(n), name, .. } if name == "EXISTS" => s.exists = n,
                Resp::Data { name, rest, .. } if name == "VANISHED" => {
                    let set = rest.iter().filter_map(Tok::atom).find(|a| !a.eq_ignore_ascii_case("(EARLIER)")).and_then(UidSet::parse);
                    if !set.is_some_and(|set| since.vanished.add(set)) {
                        since.too_much = true;
                    }
                }
                Resp::Status { tag: None, kind: Kind::Ok, code: Some(code), .. } => {
                    let mut words = code.split_whitespace();
                    let key = words.next().unwrap_or_default().to_ascii_uppercase();
                    let value = words.next().and_then(|v| v.parse::<u64>().ok());
                    match key.as_str() {
                        "UIDVALIDITY" => s.uid_validity = value.and_then(|v| u32::try_from(v).ok()),
                        "UIDNEXT" => s.uid_next = value.and_then(|v| u32::try_from(v).ok()),
                        // A mod-sequence is at most 2^63 - 1 ([RFC 7162] 7).
                        "HIGHESTMODSEQ" => s.highest_modseq = value.filter(|v| *v > 0 && *v < 1 << 63),
                        "NOMODSEQ" => s.highest_modseq = None,
                        "MAILBOXID" => s.mailbox_id = code.get("MAILBOXID".len()..).and_then(object_id),
                        _ => {}
                    }
                }
                other => {
                    if let Some(m) = summary_of(&other) {
                        since.changed.push(m);
                    }
                }
            }
        }
        if resync.is_some() {
            since.changed.sort_by_key(|m| m.uid);
            since.changed.dedup_by_key(|m| m.uid);
            s.since = Some(since);
        }
        Ok(s)
    }

    /// The messages of the selected folder whose flags changed after a mod-sequence, with their
    /// flags now ([RFC 7162] 3.1.4.1).
    pub fn changed_since(&mut self, modseq: u64) -> Result<Vec<Summary>, NetError> {
        let (data, _) = self.run(vec![Part::Text(format!("UID FETCH 1:* (UID FLAGS) (CHANGEDSINCE {modseq})"))])?;
        let mut out: Vec<Summary> = data.iter().filter_map(summary_of).collect();
        out.sort_by_key(|m| m.uid);
        out.dedup_by_key(|m| m.uid);
        Ok(out)
    }

    /// Which of these ids the selected folder still has: what the server says went is made sure
    /// of before a message goes here for it.
    pub fn still_there(&mut self, uids: &[u32]) -> Result<std::collections::HashSet<u32>, NetError> {
        let mut out = std::collections::HashSet::new();
        for chunk in uids.chunks(500) {
            let set: Vec<String> = chunk.iter().map(u32::to_string).collect();
            let (data, _) = self.run(vec![Part::Text(format!("UID FETCH {} (UID)", set.join(",")))])?;
            out.extend(
                data.iter().filter_map(fetched).filter_map(|items| item(items, "UID").and_then(Tok::number)).filter_map(|u| u32::try_from(u).ok()),
            );
        }
        Ok(out)
    }

    /// The messages of the selected folder from `from` on: id, flags and size.
    pub fn list(&mut self, from: u32) -> Result<Vec<Summary>, NetError> {
        let from = from.max(1);
        let (data, _) = self.run(vec![Part::Text(format!("UID FETCH {from}:* (UID FLAGS RFC822.SIZE)"))])?;
        let mut out = Vec::new();
        for d in data {
            let Some(items) = fetched(&d) else { continue };
            let Some(uid) = item(items, "UID").and_then(Tok::number).and_then(|u| u32::try_from(u).ok()) else { continue };
            // `n:*` takes in the last message even when its id is below n ([RFC 9051] 6.4.9).
            if uid < from {
                continue;
            }
            let flags = match item(items, "FLAGS") {
                Some(Tok::List(f)) => f.iter().filter_map(|t| t.atom()).map(str::to_string).collect(),
                _ => Vec::new(),
            };
            let size = item(items, "RFC822.SIZE").and_then(Tok::number).unwrap_or(0);
            out.push(Summary { uid, flags, size });
        }
        out.sort_by_key(|s| s.uid);
        out.dedup_by_key(|s| s.uid);
        Ok(out)
    }

    /// One message of the selected folder, whole, without marking it read.
    pub fn fetch(&mut self, uid: u32) -> Result<Option<Vec<u8>>, NetError> {
        let (data, _) = self.run(vec![Part::Text(format!("UID FETCH {uid} (UID BODY.PEEK[])"))])?;
        for d in data {
            let Some(items) = fetched(&d) else { continue };
            if item(items, "UID").and_then(Tok::number) != Some(u64::from(uid)) {
                continue;
            }
            if let Some(Tok::Str(body)) = item(items, "BODY[]") {
                return Ok(Some(body.clone()));
            }
        }
        Ok(None)
    }

    /// Several messages of the selected folder, whole, in one command, without marking them
    /// read: each by its id, in the order asked for; one that is gone by now is left out. The
    /// caller keeps the ids few and the messages small enough together to be held at once.
    pub fn fetch_many(&mut self, uids: &[u32]) -> Result<Vec<(u32, Vec<u8>)>, NetError> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        if uids.len() > MAX_LITERALS {
            return Err(NetError::TooLarge("messages asked for at once"));
        }
        let set = uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let (data, _) = self.run(vec![Part::Text(format!("UID FETCH {set} (UID BODY.PEEK[])"))])?;
        let mut by: std::collections::HashMap<u32, Vec<u8>> = std::collections::HashMap::new();
        for d in data {
            let Some(items) = fetched(&d) else { continue };
            let Some(uid) = item(items, "UID").and_then(Tok::number).and_then(|u| u32::try_from(u).ok()) else { continue };
            if let Some(Tok::Str(body)) = item(items, "BODY[]")
                && uids.contains(&uid)
            {
                by.insert(uid, body.clone());
            }
        }
        Ok(uids.iter().filter_map(|u| by.remove(u).map(|b| (*u, b))).collect())
    }

    /// Puts flags on a message of the selected folder, or takes them off.
    pub fn flag(&mut self, uid: u32, on: bool, flags: &[&str]) -> Result<(), NetError> {
        if flags.iter().any(|f| f.is_empty() || f.chars().any(|c| c.is_control() || matches!(c, ' ' | '(' | ')' | '"' | '{'))) {
            return Err(NetError::Unsupported("not a flag".into()));
        }
        let sign = if on { '+' } else { '-' };
        self.run(vec![Part::Text(format!("UID STORE {uid} {sign}FLAGS.SILENT ({})", flags.join(" ")))]).map(|_| ())
    }

    /// The Message-ID of each message of the selected folder up to `upto`, by its id; read
    /// without marking anything read. Messages without one are left out.
    pub fn message_ids(&mut self, upto: u32) -> Result<Vec<(u32, String)>, NetError> {
        if upto == 0 {
            return Ok(Vec::new());
        }
        let (data, _) = self.run(vec![Part::Text(format!("UID FETCH 1:{upto} (UID BODY.PEEK[HEADER.FIELDS (MESSAGE-ID)])"))])?;
        let mut out = Vec::new();
        for d in data {
            let Some(items) = fetched(&d) else { continue };
            let Some(uid) = item(items, "UID").and_then(Tok::number).and_then(|u| u32::try_from(u).ok()) else { continue };
            // The section is named back as the server writes it ([RFC 9051] 7.5.2).
            let header = items.chunks(2).find_map(|pair| match pair {
                [Tok::Atom(name), Tok::Str(value)] if name.to_ascii_uppercase().starts_with("BODY[HEADER.FIELDS") => Some(value),
                _ => None,
            });
            if let Some(id) = header.and_then(|h| message_id_in(h))
                && uid <= upto
            {
                out.push((uid, id));
            }
        }
        out.sort_by_key(|m| m.0);
        out.dedup_by_key(|m| m.0);
        Ok(out)
    }

    /// Moves a message of the selected folder to another: with MOVE where the server has it
    /// ([RFC 6851]), otherwise copied and marked deleted, to be expunged. Its id and the folder's
    /// UIDVALIDITY there, when the server says them ([RFC 4315] COPYUID).
    pub fn move_to(&mut self, uid: u32, to: &str) -> Result<Moved, NetError> {
        let verb = if self.has("MOVE") { "UID MOVE" } else { "UID COPY" };
        let (data, code) = self.run(vec![Part::Text(format!("{verb} {uid} ")), astring(to.as_bytes())])?;
        // MOVE says COPYUID before it has done; COPY in its answer.
        let codes = data.iter().filter_map(|d| match d {
            Resp::Status { tag: None, kind: Kind::Ok, code: Some(c), .. } => Some(c.as_str()),
            _ => None,
        });
        let there = codes.chain(code.as_deref()).find_map(copy_uid);
        let left = !self.has("MOVE");
        if left {
            self.flag(uid, true, &["\\Deleted"])?;
        }
        Ok(Moved { there, left })
    }

    /// Takes away the messages of the selected folder marked deleted: only these where the server
    /// can be told which ([RFC 4315] UID EXPUNGE), otherwise all that are so marked.
    pub fn expunge(&mut self, uids: &[u32]) -> Result<(), NetError> {
        if uids.is_empty() {
            return Ok(());
        }
        if self.has("UIDPLUS") {
            for chunk in uids.chunks(500) {
                let set: Vec<String> = chunk.iter().map(u32::to_string).collect();
                self.run(vec![Part::Text(format!("UID EXPUNGE {}", set.join(",")))])?;
            }
            Ok(())
        } else {
            self.run(vec![Part::Text("EXPUNGE".into())]).map(|_| ())
        }
    }

    /// Adds a message to a folder; its id there when the server says it ([RFC 4315] UIDPLUS).
    pub fn append(&mut self, raw: &str, flags: &[&str], message: &[u8]) -> Result<Option<u32>, NetError> {
        if message.len() > MAX_MESSAGE {
            return Err(NetError::TooLarge("the message"));
        }
        let mut parts = vec![Part::Text("APPEND ".into()), astring(raw.as_bytes())];
        if !flags.is_empty() {
            parts.push(Part::Text(format!(" ({})", flags.join(" "))));
        }
        parts.push(Part::Text(" ".into()));
        parts.push(Part::Lit(message.to_vec()));
        let (_, code) = self.run(parts)?;
        Ok(code.and_then(|c| {
            let mut w = c.split_whitespace();
            (w.next()?.eq_ignore_ascii_case("APPENDUID")).then_some(())?;
            w.nth(1)?.parse().ok()
        }))
    }

    /// Make a folder on the server ([RFC 9051] 6.3.4), as an APPEND that was answered
    /// `[TRYCREATE]` asks for.
    pub fn create(&mut self, raw: &str) -> Result<(), NetError> {
        self.run(vec![Part::Text("CREATE ".into()), astring(raw.as_bytes())]).map(|_| ())
    }

    /// Rename a folder on the server, or move it under another ([RFC 9051] 6.3.6). The folders
    /// inside it go with it, and its messages keep their ids.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), NetError> {
        self.run(vec![Part::Text("RENAME ".into()), astring(from.as_bytes()), Part::Text(" ".into()), astring(to.as_bytes())]).map(|_| ())
    }

    /// Delete a folder on the server, with the messages in it ([RFC 9051] 6.3.5).
    pub fn delete(&mut self, raw: &str) -> Result<(), NetError> {
        self.run(vec![Part::Text("DELETE ".into()), astring(raw.as_bytes())]).map(|_| ())
    }

    /// What a folder is without selecting it: its UIDVALIDITY, and its own id where the server
    /// keeps one (`OBJECTID`, [RFC 8474]).
    pub fn status(&mut self, raw: &str) -> Result<Status, NetError> {
        let items = if self.has("OBJECTID") { " (UIDVALIDITY MAILBOXID)" } else { " (UIDVALIDITY)" };
        let (data, _) = self.run(vec![Part::Text("STATUS ".into()), astring(raw.as_bytes()), Part::Text(items.into())])?;
        let mut out = Status::default();
        for d in data {
            let Resp::Data { name, rest, .. } = d else { continue };
            if name != "STATUS" {
                continue;
            }
            let Some(Tok::List(pairs)) = rest.get(1) else { continue };
            let mut at = 0;
            while let (Some(key), Some(value)) = (pairs.get(at).and_then(Tok::atom), pairs.get(at + 1)) {
                match key.to_ascii_uppercase().as_str() {
                    "UIDVALIDITY" => out.uid_validity = value.number().and_then(|v| u32::try_from(v).ok()).filter(|v| *v > 0),
                    "MAILBOXID" => {
                        out.mailbox_id = match value {
                            Tok::List(id) => id.first().and_then(Tok::text).as_deref().and_then(object_id),
                            other => other.text().as_deref().and_then(object_id),
                        }
                    }
                    _ => {}
                }
                at += 2;
            }
        }
        Ok(out)
    }

    /// Wait on the folder selected until the server says something in it changed ([RFC 2177]):
    /// IDLE, then DONE once it said so or `keep_on` says to wait no longer. The server is
    /// listened to `poll` at a time, and `keep_on` asked after each quiet one, so watching can
    /// be stopped within about that long; it says too when IDLE is to be begun again, as a
    /// server may end one that lasts more than 29 minutes.
    pub fn idle(&mut self, poll: std::time::Duration, keep_on: &mut dyn FnMut() -> bool) -> Result<Woke, NetError> {
        if !self.has("IDLE") {
            return Err(NetError::Unsupported("the server does not tell of changes as they come (IDLE)".into()));
        }
        let tag = self.next_tag();
        self.c.send(format!("{tag} IDLE\r\n").as_bytes())?;
        let mut woke = Woke::default();
        let mut chatter = 0usize;
        loop {
            match read(&mut self.c)? {
                Resp::Go(_) => break,
                Resp::Status { tag: Some(t), kind, text, .. } if t == tag => return Err(status_error(kind, text)),
                Resp::Status { tag: Some(t), .. } => return Err(NetError::protocol(format!("an answer to a command not sent: {t}"))),
                Resp::Status { tag: None, kind: Kind::Bye, text, .. } => return Err(NetError::Refused(said(&text))),
                other => {
                    woke.hear(&other);
                    chatter += 1;
                    if chatter > MAX_IDLE_CHATTER {
                        return Err(NetError::TooLarge("what the server said before it waited"));
                    }
                }
            }
        }
        let was = self.c.set_wait(poll)?;
        let heard = self.listen(&tag, &mut woke, keep_on);
        if let Some(was) = was {
            self.c.set_wait(was)?;
        }
        // The server ended the wait itself: nothing is left to say DONE to.
        if heard? {
            return Ok(woke);
        }
        self.c.send(b"DONE\r\n")?;
        let (data, _) = self.finish(&tag, Vec::new())?;
        data.iter().for_each(|d| woke.hear(d));
        Ok(woke)
    }

    /// What the server says while it is waited on, until something changed or `keep_on` says
    /// no more; true when the server answered the IDLE itself.
    fn listen(&mut self, tag: &str, woke: &mut Woke, keep_on: &mut dyn FnMut() -> bool) -> Result<bool, NetError> {
        let mut chatter = 0usize;
        while !woke.changed {
            match read(&mut self.c) {
                Ok(Resp::Status { tag: Some(t), kind, text, .. }) if t == tag => {
                    return match kind {
                        Kind::Ok => Ok(true),
                        kind => Err(status_error(kind, text)),
                    };
                }
                Ok(Resp::Status { tag: Some(t), .. }) => return Err(NetError::protocol(format!("an answer to a command not sent: {t}"))),
                Ok(Resp::Status { tag: None, kind: Kind::Bye, text, .. }) => return Err(NetError::Refused(said(&text))),
                Ok(Resp::Go(_)) => return Err(NetError::protocol("asked to go on when nothing waits")),
                Ok(other) => {
                    woke.hear(&other);
                    chatter += 1;
                    // A server that says something all the time is still asked after now and
                    // then, and one that says only nonsense is left.
                    if chatter > MAX_IDLE_CHATTER {
                        return Err(NetError::TooLarge("what the server said while it was watched"));
                    }
                    if chatter.is_multiple_of(100) && !keep_on() {
                        return Ok(false);
                    }
                }
                Err(NetError::TimedOut) => {
                    if !keep_on() {
                        return Ok(false);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Ok(false)
    }

    pub fn logout(mut self) -> Result<(), NetError> {
        self.run(vec![Part::Text("LOGOUT".into())]).map(|_| ())
    }
}

/// What a move came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moved {
    /// The UIDVALIDITY of the folder it went to and its id there, when the server said them.
    pub there: Option<(u32, u32)>,
    /// It was copied and marked deleted where it was, and is there until it is expunged.
    pub left: bool,
}

/// `COPYUID <validity> <from> <to>` for one message: the validity and its new id.
fn copy_uid(code: &str) -> Option<(u32, u32)> {
    let mut w = code.split_whitespace();
    if !w.next()?.eq_ignore_ascii_case("COPYUID") {
        return None;
    }
    let validity = w.next()?.parse().ok()?;
    let _from = w.next()?;
    let to = w.next()?;
    // One message was moved, so one id; a range would be another server's business.
    Some((validity, to.parse().ok()?))
}

/// The Message-ID in header lines, unfolded and trimmed, as at most 998 characters.
fn message_id_in(header: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(header);
    let mut value: Option<String> = None;
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        match &mut value {
            Some(v) if line.starts_with([' ', '\t']) => v.push_str(line.trim()),
            Some(_) => break,
            None => {
                if let Some((name, rest)) = line.split_once(':')
                    && name.trim().eq_ignore_ascii_case("Message-ID")
                {
                    value = Some(rest.trim().to_string());
                }
            }
        }
    }
    value.map(|v| v.chars().filter(|c| !c.is_control()).take(998).collect::<String>()).filter(|v| !v.is_empty())
}

fn status_error(kind: Kind, text: String) -> NetError {
    match kind {
        Kind::Bad => NetError::protocol(format!("the server did not understand: {text}")),
        _ => NetError::Refused(said(&text)),
    }
}

fn keep(data: &mut Vec<Resp>, r: Resp) -> Result<(), NetError> {
    if data.len() >= MAX_UNTAGGED {
        return Err(NetError::TooLarge("the answer to one command"));
    }
    data.push(r);
    Ok(())
}

/// A FETCH response as a message's id and flags, its size when it says it.
fn summary_of(r: &Resp) -> Option<Summary> {
    let items = fetched(r)?;
    let uid = item(items, "UID").and_then(Tok::number).and_then(|u| u32::try_from(u).ok())?;
    let flags = match item(items, "FLAGS") {
        Some(Tok::List(f)) => f.iter().filter_map(|t| t.atom()).map(str::to_string).collect(),
        _ => return None,
    };
    let size = item(items, "RFC822.SIZE").and_then(Tok::number).unwrap_or(0);
    Some(Summary { uid, flags, size })
}

/// The items of a FETCH response.
fn fetched(r: &Resp) -> Option<&[Tok]> {
    match r {
        Resp::Data { name, rest, .. } if name == "FETCH" => match rest.first() {
            Some(Tok::List(items)) => Some(items),
            _ => None,
        },
        _ => None,
    }
}

/// The value after a name in a FETCH response's list.
fn item<'a>(items: &'a [Tok], name: &str) -> Option<&'a Tok> {
    items.chunks(2).find(|pair| pair.first().and_then(Tok::atom).is_some_and(|a| a.eq_ignore_ascii_case(name))).and_then(|pair| pair.get(1))
}
