//! Calendars on a CalDAV server ([RFC 4791]), as Nextcloud, Radicale, Fastmail and others keep
//! them: found from where the account says, or from its address ([RFC 6764]), each one's
//! changes asked for by its sync token ([RFC 6578]) or, on a server that keeps none, by the
//! ETag of each event; events read many at a time (calendar-multiget, [RFC 4791] 7.9), and put
//! or deleted only while the server still has what was last read of them (`If-Match` and
//! `If-None-Match`, [RFC 9110] 13.1).
//!
//! Over HTTPS only, signed in with Basic ([RFC 7617]) on that connection alone. What the
//! server says is hostile: the head, the body, how deep and how many its XML elements are,
//! each href, the responses and the calendars are all capped, and an href that leads to
//! another server is not followed.
//!
//! [RFC 4791]: https://www.rfc-editor.org/rfc/rfc4791
//! [RFC 6578]: https://www.rfc-editor.org/rfc/rfc6578
//! [RFC 6764]: https://www.rfc-editor.org/rfc/rfc6764
//! [RFC 7617]: https://www.rfc-editor.org/rfc/rfc7617
//! [RFC 9110]: https://www.rfc-editor.org/rfc/rfc9110

use crate::conn::Conn;
use crate::dns::{Kind, Record};
use crate::{Connector, Endpoint, Fingerprint, Login, NetError, Protocol, Security, http, said};

/// The largest answer read: a list of a calendar's events, or the events themselves.
pub const MAX_BODY: usize = 16 << 20;
/// Lines of an answer's head, and the longest address or href taken.
const MAX_HEAD_LINES: usize = 100;
const MAX_URL: usize = 2048;
/// Redirects followed while a server's calendars are found.
const MAX_REDIRECTS: usize = 5;
/// Nodes of one answer's XML, and elements inside one another.
const MAX_NODES: u32 = 4_000_000;
const MAX_DEPTH: usize = 40;
/// Calendars taken of one server, and responses of one answer.
pub const MAX_CALENDARS: usize = 100;
pub const MAX_RESPONSES: usize = 50_000;
/// Events asked for in one calendar-multiget.
pub const MULTIGET: usize = 50;
/// Rounds of a sync the server cut short ([RFC 6578] 3.6), before what is left waits.
const MAX_ROUNDS: usize = 20;
/// Longest name, colour, ETag and sync token kept.
const MAX_NAME: usize = 200;
const MAX_TAG: usize = 1000;

/// What is asked of a calendar to list it.
/// What is asked of an address book ([RFC 6352] 6).
const BOOK_PROPS: &str = "<d:resourcetype/><d:displayname/><cs:getctag/><d:sync-token/><d:current-user-privilege-set/>";
const CALENDAR_PROPS: &str = "<d:resourcetype/><d:displayname/><cs:getctag/><d:sync-token/><c:supported-calendar-component-set/><ic:calendar-color/><d:current-user-privilege-set/>";

const DAV: &str = "DAV:";
const CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
const CARDDAV: &str = "urn:ietf:params:xml:ns:carddav";
const CS: &str = "http://calendarserver.org/ns/";
const APPLE: &str = "http://apple.com/ns/ical/";

/// Where a CalDAV server is: an `https:` address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    pub host: String,
    pub port: u16,
    /// The path, as sent in the request line.
    pub path: String,
}

impl Url {
    /// Read an `https:` address ([RFC 9110] 4.2.2). Anything else, and an address with a name
    /// or password in it, is refused. A server on this machine or the local network is taken:
    /// the owner named it.
    pub fn parse(url: &str) -> Result<Self, NetError> {
        let url = url.trim();
        let refuse = |why: &str| NetError::Unsupported(format!("{} is not a server's address: {why}", said(url)));
        if url.len() > MAX_URL || url.chars().any(|c| c.is_control() || c == ' ') {
            return Err(refuse("it is too long or holds spaces"));
        }
        let (scheme, rest) = url.split_once("://").ok_or_else(|| refuse("it does not start with https://"))?;
        if !scheme.eq_ignore_ascii_case("https") {
            return Err(refuse("only HTTPS is used"));
        }
        let cut = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = rest.get(..cut).unwrap_or("");
        let mut path: String = rest.get(cut..).unwrap_or("").split('#').next().unwrap_or("").to_string();
        if path.is_empty() || path.starts_with('?') {
            path.insert(0, '/');
        }
        if !path.is_ascii() {
            return Err(refuse("its path is not plain ASCII"));
        }
        if authority.contains('@') {
            return Err(refuse("it holds a name and password"));
        }
        let (host, port) = match authority.strip_prefix('[') {
            Some(inner) => {
                let (host, after) = inner.split_once(']').ok_or_else(|| refuse("it is not whole"))?;
                (host, after.strip_prefix(':'))
            }
            None => match authority.rsplit_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            },
        };
        let port = match port {
            Some(p) => p.parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| refuse("its port is not one"))?,
            None => 443,
        };
        let host = host.to_ascii_lowercase();
        if host.is_empty() || host.len() > 253 || !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':')) {
            return Err(refuse("its server's name is not one"));
        }
        Ok(Self { host, port, path })
    }

    /// The address as text.
    pub fn text(&self) -> String {
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        if self.port == 443 { format!("https://{host}{}", self.path) } else { format!("https://{host}:{}{}", self.port, self.path) }
    }

    fn same_server(&self, other: &Url) -> bool {
        self.host == other.host && self.port == other.port
    }

    /// The same address, at another path of the server.
    fn at(&self, path: &str) -> Self {
        Self { host: self.host.clone(), port: self.port, path: path.to_string() }
    }
}

/// Where a server may send us while its calendars are found: the same server, or one of the
/// same domain (`example.org` to `cloud.example.org`, and back), over HTTPS; never from a
/// server off this machine to one on it or on its network.
fn may_follow(from: &Url, to: &Url) -> bool {
    if from.same_server(to) {
        return true;
    }
    let near = to.host.ends_with(&format!(".{}", from.host)) || from.host.ends_with(&format!(".{}", to.host));
    near && (http::is_local(&from.host) || !http::is_local(&to.host))
}

/// A calendar of the server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Calendar {
    /// Its path on the server, ending in `/`.
    pub href: String,
    /// What the owner called it.
    pub name: String,
    /// Its colour, `#RRGGBB`, when the server keeps one.
    pub color: String,
    /// What changes when anything in it does (getctag); empty when the server keeps none.
    pub ctag: String,
    /// Its sync token now ([RFC 6578] 4); empty when the server keeps none.
    pub token: String,
    /// It holds events: it says so, or says nothing of what it holds.
    pub events: bool,
    /// What is changed here may be put there.
    pub writable: bool,
}

/// An event of a calendar as listed: where it is and its ETag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub href: String,
    pub etag: String,
}

/// What changed in a calendar since a sync token.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub changed: Vec<Entry>,
    pub removed: Vec<String>,
    /// The token to ask with next time.
    pub token: String,
    /// The server cut the list short more often than is followed: what is left comes next time.
    pub partial: bool,
}

/// How asking for a calendar's changes came out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sync {
    Changes(Changes),
    /// The token is one the server no longer knows ([RFC 6578] 3.2): the whole list is read.
    Lost,
    /// The server does not keep sync tokens for this calendar.
    NotKept,
}

/// An event as the server sent it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub href: String,
    pub etag: String,
    pub data: String,
}

/// How putting an event came out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Put {
    /// Put; with its new ETag when the server said it.
    Done(Option<String>),
    /// The server's copy changed since it was read, or one by that name is there already.
    Conflict,
}

/// How deleting an event came out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deleted {
    Done,
    /// The server's copy changed since it was read: it was not deleted.
    Conflict,
    /// The server had it no more.
    Gone,
}

/// What the server's collections hold: calendars of events ([RFC 4791]) or address books of
/// contacts ([RFC 6352]). The same WebDAV sync is spoken to both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Holds {
    #[default]
    Events,
    Cards,
}

impl Holds {
    /// What the server is called in what is said of it.
    pub fn server(self) -> &'static str {
        match self {
            Self::Events => "the calendar server",
            Self::Cards => "the address book server",
        }
    }

    /// One of its collections.
    pub fn collection(self) -> &'static str {
        match self {
            Self::Events => "calendar",
            Self::Cards => "address book",
        }
    }

    /// One thing in a collection.
    fn item(self) -> &'static str {
        match self {
            Self::Events => "event",
            Self::Cards => "contact",
        }
    }

    /// Where a server of a domain says its collections are ([RFC 6764] 5).
    fn well_known(self) -> &'static str {
        match self {
            Self::Events => "/.well-known/caldav",
            Self::Cards => "/.well-known/carddav",
        }
    }
}

/// An answer of the server.
#[derive(Debug)]
struct Reply {
    code: u16,
    etag: Option<String>,
    location: Option<String>,
    body: Vec<u8>,
}

/// A connection to a CalDAV or CardDAV server, signed in.
pub struct Dav<'a> {
    with: &'a dyn Connector,
    /// What its collections hold.
    pub holds: Holds,
    login: &'a Login,
    /// The server, and the path the account names.
    pub at: Url,
    /// A certificate the owner trusted for this server, though this system does not.
    pub trusted: Option<Fingerprint>,
}

/// An ETag or sync token as a server gives it: printable ASCII, not too long.
fn kept_tag(t: &str, max: usize) -> Option<String> {
    let t = t.trim();
    (!t.is_empty() && t.len() <= max && t.bytes().all(|b| (0x20..0x7F).contains(&b))).then(|| t.to_string())
}

/// Text put inside an XML element.
fn escape(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// A path fit for a request line: ASCII with no space, control or line break.
fn kept_path(p: &str) -> Option<String> {
    (p.starts_with('/') && p.len() <= MAX_URL && p.bytes().all(|b| (0x21..0x7F).contains(&b))).then(|| p.to_string())
}

impl<'a> Dav<'a> {
    pub fn new(with: &'a dyn Connector, login: &'a Login, at: Url, trusted: Option<Fingerprint>) -> Self {
        Self { with, login, at, trusted, holds: Holds::Events }
    }

    /// The same server, its address books asked for instead of its calendars.
    pub fn holding(mut self, holds: Holds) -> Self {
        self.holds = holds;
        self
    }

    fn who(&self) -> &'static str {
        self.holds.server()
    }

    fn endpoint(&self) -> Endpoint {
        Endpoint { protocol: Protocol::Https, host: self.at.host.clone(), port: self.at.port, security: Security::Tls, trusted: self.trusted }
    }

    /// An href of an answer as a path of this server; one of another server is not taken.
    fn path_of(&self, href: &str) -> Option<String> {
        let href = href.trim();
        if href.starts_with('/') {
            return kept_path(href);
        }
        let url = Url::parse(href).ok()?;
        self.at.same_server(&url).then_some(url.path).and_then(|p| kept_path(&p))
    }

    /// One request and its answer. The name and password go only over this connection, which
    /// is TLS from its first byte.
    fn send(&self, method: &str, path: &str, depth: Option<&str>, headers: &[(&str, &str)], body: Option<(&str, &[u8])>) -> Result<Reply, NetError> {
        use base64::Engine as _;
        let path = kept_path(path).ok_or_else(|| NetError::Unsupported(format!("an address on {} that is not asked for", self.who())))?;
        if self.login.user.contains(':') || self.login.user.chars().any(char::is_control) {
            return Err(NetError::Unsupported(format!("a user name with a colon in it cannot sign in to {}", self.who())));
        }
        let mut conn = Conn::new(self.with.connect(&self.endpoint())?);
        let name = if self.at.host.contains(':') { format!("[{}]", self.at.host) } else { self.at.host.clone() };
        let host = if self.at.port == 443 { name } else { format!("{name}:{}", self.at.port) };
        let auth = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", self.login.user, self.login.password.expose()));
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: OpenMailClient\r\nAuthorization: Basic {auth}\r\nAccept-Encoding: identity\r\nConnection: close\r\n"
        );
        if let Some(depth) = depth {
            head.push_str(&format!("Depth: {depth}\r\n"));
        }
        for (name, value) in headers {
            if value.bytes().any(|b| b < 0x20 || b == 0x7F) {
                return Err(NetError::Unsupported("a value that holds a line break is not sent".into()));
            }
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        let body = body.unwrap_or(("", &[]));
        if !body.0.is_empty() {
            head.push_str(&format!("Content-Type: {}\r\n", body.0));
        }
        head.push_str(&format!("Content-Length: {}\r\n\r\n", body.1.len()));
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(body.1);
        conn.send(&bytes)?;
        let reply = read_reply(&mut conn, method == "HEAD")?;
        if reply.code == 401 {
            return Err(NetError::Auth(format!("{} did not take them", self.who())));
        }
        Ok(reply)
    }

    /// PROPFIND, following redirects to where the server's calendars are when `follow`.
    fn propfind(&mut self, path: &str, depth: &str, props: &str, follow: bool) -> Result<(String, Reply), NetError> {
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<d:propfind xmlns:d=\"DAV:\" xmlns:c=\"{CALDAV}\" xmlns:card=\"{CARDDAV}\" xmlns:cs=\"{CS}\" xmlns:ic=\"{APPLE}\"><d:prop>{props}</d:prop></d:propfind>"
        );
        let mut path = path.to_string();
        for _ in 0..=MAX_REDIRECTS {
            let reply = self.send("PROPFIND", &path, Some(depth), &[], Some(("application/xml; charset=utf-8", body.as_bytes())))?;
            match (reply.code, &reply.location) {
                (301 | 302 | 303 | 307 | 308, Some(to)) if follow => {
                    let to = to.trim();
                    let next = if to.starts_with('/') { self.at.at(to) } else { Url::parse(to)? };
                    if !may_follow(&self.at, &next) {
                        return Err(NetError::Refused(format!("{} sent us on to {}, which is not followed", self.who(), said(&next.host))));
                    }
                    self.at = next.at(&self.at.path);
                    path = next.path;
                }
                _ => return Ok((path, reply)),
            }
        }
        Err(NetError::Refused(format!("{} sent us on too many times", self.who())))
    }

    /// The server's calendars, found from the path the account names: the principal it
    /// belongs to ([RFC 5397]), that principal's calendar homes ([RFC 4791] 6.2.1), and the
    /// calendars in each. Where the path gives no principal, `/.well-known/caldav` is asked
    /// ([RFC 6764] 5). A path that is a calendar's own is taken as that calendar's home.
    /// Holding cards, the same is done for address books: the principal's address book homes
    /// ([RFC 6352] 7.1.1) and `/.well-known/carddav`.
    pub fn calendars(&mut self) -> Result<Vec<Calendar>, NetError> {
        let start = self.at.path.clone();
        // What the path is: a calendar is the one listed; else whose it is.
        let props = format!("{}<d:current-user-principal/>", self.props());
        let (_, reply) = self.propfind(&start, "0", &props, start == "/")?;
        let mut principal = None;
        if reply.code == 207 {
            let (all, _) = multistatus(&reply.body)?;
            if let Some(one) = all.iter().find_map(|r| self.calendar_of(r)) {
                return Ok(vec![one]);
            }
            principal = all.into_iter().find_map(|r| r.props.principal).and_then(|h| self.path_of(&h));
        }
        if principal.is_none() {
            principal = self.principal(self.holds.well_known(), true).ok().flatten();
        }
        let mut homes = match &principal {
            Some(p) => self.homes(p)?,
            None => Vec::new(),
        };
        if homes.is_empty() {
            homes.push(start.clone());
        }
        let mut out: Vec<Calendar> = Vec::new();
        for home in homes.iter().take(10) {
            for c in self.in_home(home)? {
                if out.len() < MAX_CALENDARS && !out.iter().any(|o| o.href == c.href) {
                    out.push(c);
                }
            }
        }
        Ok(out)
    }

    /// What is asked of a collection.
    fn props(&self) -> &'static str {
        match self.holds {
            Holds::Events => CALENDAR_PROPS,
            Holds::Cards => BOOK_PROPS,
        }
    }

    fn principal(&mut self, path: &str, follow: bool) -> Result<Option<String>, NetError> {
        let (_, reply) = self.propfind(path, "0", "<d:current-user-principal/>", follow)?;
        if reply.code != 207 {
            return Ok(None);
        }
        let (all, _) = multistatus(&reply.body)?;
        Ok(all.into_iter().find_map(|r| r.props.principal).and_then(|h| self.path_of(&h)))
    }

    fn homes(&mut self, principal: &str) -> Result<Vec<String>, NetError> {
        let home = match self.holds {
            Holds::Events => "<c:calendar-home-set/>",
            Holds::Cards => "<card:addressbook-home-set/>",
        };
        let (_, reply) = self.propfind(principal, "0", home, false)?;
        if reply.code != 207 {
            return Err(NetError::Refused(format!("{} answered {} when asked where the {}s are", self.who(), reply.code, self.holds.collection())));
        }
        let (all, _) = multistatus(&reply.body)?;
        Ok(all
            .into_iter()
            .flat_map(|r| if self.holds == Holds::Cards { r.props.book_homes } else { r.props.homes })
            .filter_map(|h| self.path_of(&h))
            .take(10)
            .collect())
    }

    /// The calendars in one home (Depth 1).
    fn in_home(&mut self, home: &str) -> Result<Vec<Calendar>, NetError> {
        let (_, reply) = self.propfind(home, "1", self.props(), false)?;
        if reply.code != 207 {
            return Err(NetError::Refused(format!("{} answered {} when its {}s were listed", self.who(), reply.code, self.holds.collection())));
        }
        let (all, _) = multistatus(&reply.body)?;
        Ok(all.iter().filter_map(|r| self.calendar_of(r)).take(MAX_CALENDARS).collect())
    }

    /// A response as a calendar, when it is one of this server.
    fn calendar_of(&self, r: &Response) -> Option<Calendar> {
        let wanted = match self.holds {
            Holds::Events => r.props.calendar,
            Holds::Cards => r.props.book,
        };
        let href = self.path_of(&r.href).filter(|_| wanted)?;
        {
            let href = if href.ends_with('/') { href } else { format!("{href}/") };
            let name = r.props.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| {
                let last = href.trim_end_matches('/').rsplit('/').next().unwrap_or(self.holds.collection());
                percent_decoded(last)
            });
            let events = self.holds == Holds::Cards || !r.props.comps_given || r.props.comps.iter().any(|c| c.eq_ignore_ascii_case("VEVENT"));
            let writable = !r.props.privileges_given || r.props.privileges.iter().any(|p| matches!(p.as_str(), "all" | "write" | "write-content"));
            Some(Calendar {
                href,
                name: name.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect(),
                color: r.props.color.as_deref().and_then(color).unwrap_or_default(),
                ctag: r.props.ctag.clone().unwrap_or_default(),
                token: r.props.token.clone().unwrap_or_default(),
                events,
                writable,
            })
        }
    }

    /// What changed in a calendar since `token` ([RFC 6578] 3.2); with an empty token, every
    /// event in it. A list the server cut short is asked for again from where it stopped.
    pub fn changes(&self, calendar: &str, token: &str) -> Result<Sync, NetError> {
        let mut out = Changes::default();
        let mut token = token.to_string();
        for round in 0..MAX_ROUNDS {
            let body = format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<d:sync-collection xmlns:d=\"DAV:\"><d:sync-token>{}</d:sync-token><d:sync-level>1</d:sync-level><d:prop><d:getetag/></d:prop></d:sync-collection>",
                escape(&token)
            );
            let reply = self.send("REPORT", calendar, None, &[], Some(("application/xml; charset=utf-8", body.as_bytes())))?;
            match reply.code {
                207 => {}
                403 | 409 if contains(&reply.body, b"valid-sync-token") => return Ok(if token.is_empty() { Sync::NotKept } else { Sync::Lost }),
                404 => return Err(self.gone()),
                400 | 403 | 405 | 415 | 422 | 501 => return Ok(if round == 0 { Sync::NotKept } else { Sync::Lost }),
                code => return Err(NetError::Refused(format!("{} answered {code} when asked what changed", self.who()))),
            }
            let (all, next) = multistatus(&reply.body)?;
            let mut cut = false;
            for r in all {
                let Some(href) = self.path_of(&r.href) else { continue };
                if same_collection(&href, calendar) {
                    cut |= r.status == Some(507);
                    continue;
                }
                match (r.status, r.props.etag) {
                    (Some(404), _) => push_removed(&mut out, href),
                    (_, Some(etag)) => push_changed(&mut out, Entry { href, etag }),
                    _ => {}
                }
            }
            let Some(next) = next.and_then(|t| kept_tag(&t, MAX_TAG)) else {
                return Ok(if token.is_empty() && out.changed.is_empty() { Sync::NotKept } else { Sync::Lost });
            };
            token = next;
            if !cut {
                out.token = token;
                return Ok(Sync::Changes(out));
            }
        }
        out.token = token;
        out.partial = true;
        Ok(Sync::Changes(out))
    }

    /// Every event of a calendar with its ETag (PROPFIND, Depth 1): for a server that keeps no
    /// sync tokens, or one that forgot the token given.
    pub fn list(&self, calendar: &str) -> Result<Vec<Entry>, NetError> {
        let body =
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<d:propfind xmlns:d=\"DAV:\"><d:prop><d:resourcetype/><d:getetag/></d:prop></d:propfind>";
        let reply = self.send("PROPFIND", calendar, Some("1"), &[], Some(("application/xml; charset=utf-8", body.as_bytes())))?;
        match reply.code {
            207 => {}
            404 => return Err(self.gone()),
            code => return Err(NetError::Refused(format!("{} answered {code} when its {}s were listed", self.who(), self.holds.item()))),
        }
        let (all, _) = multistatus(&reply.body)?;
        let mut out: Vec<Entry> = Vec::new();
        for r in all {
            let Some(href) = self.path_of(&r.href) else { continue };
            if same_collection(&href, calendar) || r.props.collection {
                continue;
            }
            if let Some(etag) = r.props.etag
                && !out.iter().any(|e| e.href == href)
            {
                out.push(Entry { href, etag });
            }
        }
        Ok(out)
    }

    /// The events at these hrefs of a calendar (calendar-multiget, [RFC 4791] 7.9), at most
    /// `MULTIGET` of them; those the server has no more are left out.
    pub fn fetch(&self, calendar: &str, hrefs: &[String]) -> Result<Vec<Object>, NetError> {
        let mut asked = String::new();
        for h in hrefs.iter().take(MULTIGET) {
            asked.push_str(&format!("<d:href>{}</d:href>", escape(h)));
        }
        // [RFC 6352] 8.7 asks for cards as [RFC 4791] 7.9 asks for events.
        let body = match self.holds {
            Holds::Events => format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<c:calendar-multiget xmlns:d=\"DAV:\" xmlns:c=\"{CALDAV}\"><d:prop><d:getetag/><c:calendar-data/></d:prop>{asked}</c:calendar-multiget>"
            ),
            Holds::Cards => format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<card:addressbook-multiget xmlns:d=\"DAV:\" xmlns:card=\"{CARDDAV}\"><d:prop><d:getetag/><card:address-data/></d:prop>{asked}</card:addressbook-multiget>"
            ),
        };
        let reply = self.send("REPORT", calendar, Some("1"), &[], Some(("application/xml; charset=utf-8", body.as_bytes())))?;
        if reply.code != 207 {
            return Err(NetError::Refused(format!("{} answered {} when {}s were asked for", self.who(), reply.code, self.holds.item())));
        }
        let (all, _) = multistatus(&reply.body)?;
        Ok(all
            .into_iter()
            .filter_map(|r| {
                let href = self.path_of(&r.href)?;
                hrefs.contains(&href).then_some(())?;
                Some(Object { href, etag: r.props.etag.unwrap_or_default(), data: r.props.data? })
            })
            .collect())
    }

    /// Put an event: over the one with `etag` there, or as a new one when `etag` is None.
    pub fn put(&self, href: &str, ics: &[u8], etag: Option<&str>) -> Result<Put, NetError> {
        let condition = match etag {
            Some(tag) => ("If-Match", tag),
            None => ("If-None-Match", "*"),
        };
        let kind = match self.holds {
            Holds::Events => "text/calendar; charset=utf-8",
            Holds::Cards => "text/vcard; charset=utf-8",
        };
        let reply = self.send("PUT", href, None, &[condition], Some((kind, ics)))?;
        match reply.code {
            200 | 201 | 204 => Ok(Put::Done(reply.etag.and_then(|t| kept_tag(&t, MAX_NAME)))),
            412 => Ok(Put::Conflict),
            code => Err(NetError::Refused(format!("{} answered {code} when {} {} was put there", self.who(), self.a(), self.holds.item()))),
        }
    }

    /// Delete an event, only while it is the one with `etag`.
    pub fn delete(&self, href: &str, etag: &str) -> Result<Deleted, NetError> {
        let condition: &[(&str, &str)] = if etag.is_empty() { &[] } else { &[("If-Match", etag)] };
        let reply = self.send("DELETE", href, None, condition, None)?;
        match reply.code {
            200 | 202 | 204 => Ok(Deleted::Done),
            404 | 410 => Ok(Deleted::Gone),
            412 => Ok(Deleted::Conflict),
            code => Err(NetError::Refused(format!("{} answered {code} when {} {} was deleted", self.who(), self.a(), self.holds.item()))),
        }
    }

    fn a(&self) -> &'static str {
        match self.holds {
            Holds::Events => "an",
            Holds::Cards => "a",
        }
    }

    fn gone(&self) -> NetError {
        NetError::Refused(format!("the {} is not on the server any more", self.holds.collection()))
    }
}

/// Where a calendar server of an address's domain may be ([RFC 6764] 6): what its
/// `_caldavs._tcp` SRV record names, then the domain itself, each at `/.well-known/caldav`.
pub fn starts(with: &dyn Connector, email: &str) -> Vec<Url> {
    starts_for(with, email, Holds::Events)
}

/// The same for what `holds` names: `_carddavs._tcp` and `/.well-known/carddav` for address
/// books ([RFC 6352] 11).
pub fn starts_for(with: &dyn Connector, email: &str, holds: Holds) -> Vec<Url> {
    let (srv_name, well_known) = match holds {
        Holds::Events => ("_caldavs._tcp", holds.well_known()),
        Holds::Cards => ("_carddavs._tcp", holds.well_known()),
    };
    let Some((_, domain)) = email.trim().rsplit_once('@') else { return Vec::new() };
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if !crate::find::is_host(&domain) {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Ok(records) = with.lookup(&format!("{srv_name}.{domain}"), Kind::Srv) {
        let mut srv: Vec<(u16, u16, u16, String)> = records
            .into_iter()
            .filter_map(|r| match r {
                Record::Srv { priority, weight, port, target } if port > 0 && crate::find::is_host(target.trim_end_matches('.')) => {
                    Some((priority, u16::MAX - weight, port, target.trim_end_matches('.').to_ascii_lowercase()))
                }
                _ => None,
            })
            .collect();
        srv.sort();
        out.extend(srv.into_iter().take(3).map(|(_, _, port, host)| Url { host, port, path: well_known.into() }));
    }
    out.push(Url { host: domain, port: 443, path: well_known.into() });
    out
}

/// Whether two paths are the same collection, with or without the `/` at the end.
fn same_collection(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

fn push_changed(out: &mut Changes, e: Entry) {
    out.removed.retain(|r| *r != e.href);
    if let Some(was) = out.changed.iter_mut().find(|c| c.href == e.href) {
        *was = e;
    } else if out.changed.len() < MAX_RESPONSES {
        out.changed.push(e);
    } else {
        out.partial = true;
    }
}

fn push_removed(out: &mut Changes, href: String) {
    out.changed.retain(|c| c.href != href);
    if !out.removed.contains(&href) && out.removed.len() < MAX_RESPONSES {
        out.removed.push(href);
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// `%XX` in a path segment read as the bytes they stand for, as text.
fn percent_decoded(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (c, hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A calendar's colour as `#RRGGBB`: Apple's `#RRGGBBAA` loses its alpha.
fn color(c: &str) -> Option<String> {
    let c = c.trim();
    let hex = c.strip_prefix('#')?;
    let hex = hex.get(..6).filter(|_| hex.len() == 6 || hex.len() == 8)?;
    hex.bytes().all(|b| b.is_ascii_hexdigit()).then(|| format!("#{}", hex.to_ascii_uppercase()))
}

/// The answer to a request: status line, head and body, no more than `MAX_BODY` of it.
fn read_reply(conn: &mut Conn, head_only: bool) -> Result<Reply, NetError> {
    // A 1xx answer is passed over for the one after it ([RFC 9110] 15.2).
    for _ in 0..5 {
        let status = String::from_utf8_lossy(&conn.line()?).into_owned();
        let mut parts = status.split(' ');
        let version = parts.next().unwrap_or("");
        let code = parts.next().and_then(|c| c.parse::<u16>().ok()).filter(|_| version.starts_with("HTTP/1."));
        let Some(code) = code else { return Err(NetError::protocol(format!("not a web server's answer: {status}"))) };
        let (mut length, mut chunked, mut etag, mut location) = (None::<usize>, false, None::<String>, None::<String>);
        for n in 0.. {
            let line = conn.line()?;
            if line.is_empty() {
                break;
            }
            if n >= MAX_HEAD_LINES {
                return Err(NetError::TooLarge("the head of the server's answer"));
            }
            let line = String::from_utf8_lossy(&line).into_owned();
            let Some((name, value)) = line.split_once(':') else { continue };
            let value = value.trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = Some(value.parse::<usize>().map_err(|_| NetError::protocol("a length that is not a number"))?),
                "transfer-encoding" => chunked = value.to_ascii_lowercase().split(',').any(|v| v.trim() == "chunked"),
                "etag" => etag = kept_tag(value, MAX_NAME),
                "location" => location = Some(value.chars().take(MAX_URL + 1).collect()),
                _ => {}
            }
        }
        if (100..200).contains(&code) {
            continue;
        }
        let body = if head_only || code == 204 || code == 304 {
            Vec::new()
        } else if chunked {
            http::chunks(conn, MAX_BODY, "the server's answer")?
        } else if let Some(length) = length {
            if length > MAX_BODY {
                return Err(NetError::TooLarge("the server's answer"));
            }
            conn.exact(length)?
        } else {
            conn.until_closed(MAX_BODY)?
        };
        return Ok(Reply { code, etag, location, body });
    }
    Err(NetError::protocol("the server sent only interim answers"))
}

/// What a response of a multistatus says of one href.
#[derive(Clone, Debug, Default)]
struct Response {
    href: String,
    /// The status of the whole response, where it has one (as a sync's removed events do).
    status: Option<u16>,
    props: Props,
}

/// The properties a server gave with a 2xx status.
#[derive(Clone, Debug, Default)]
struct Props {
    etag: Option<String>,
    name: Option<String>,
    ctag: Option<String>,
    token: Option<String>,
    color: Option<String>,
    calendar: bool,
    /// An address book ([RFC 6352] 5.2).
    book: bool,
    collection: bool,
    comps: Vec<String>,
    comps_given: bool,
    privileges: Vec<String>,
    privileges_given: bool,
    principal: Option<String>,
    homes: Vec<String>,
    /// Address book homes, kept apart: a server that answers both is asked for one.
    book_homes: Vec<String>,
    data: Option<String>,
}

/// The number of a status line: `HTTP/1.1 404 Not Found` is 404.
fn status_code(text: &str) -> Option<u16> {
    text.split_whitespace().nth(1)?.parse().ok()
}

/// The first child of `n` by namespace and name.
fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, ns: &str, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.has_tag_name((ns, name)))
}

/// The responses of a multistatus ([RFC 4918] 13), and its sync token when it has one.
fn multistatus(body: &[u8]) -> Result<(Vec<Response>, Option<String>), NetError> {
    let bad = |why: &str| NetError::protocol(format!("the server's answer is not one that is read: {why}"));
    let text = std::str::from_utf8(body).map_err(|_| bad("it is not UTF-8"))?;
    let text = text.trim_start_matches('\u{feff}');
    if !crate::find::shallow(text.as_bytes(), MAX_DEPTH) {
        return Err(bad("its elements are nested too deeply"));
    }
    let options = roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: MAX_NODES, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(text, options).map_err(|e| bad(&e.to_string()))?;
    let root = doc.root_element();
    if !root.has_tag_name((DAV, "multistatus")) {
        return Err(bad("it is not a multistatus"));
    }
    let text_of = |n: roxmltree::Node<'_, '_>| n.text().map(str::trim).unwrap_or("").to_string();
    let hrefs = |n: roxmltree::Node<'_, '_>| -> Vec<String> {
        n.children().filter(|c| c.has_tag_name((DAV, "href"))).map(&text_of).filter(|h| !h.is_empty() && h.len() <= MAX_URL).take(10).collect()
    };
    let token = child(root, DAV, "sync-token").map(text_of);
    let mut out = Vec::new();
    for r in root.children().filter(|c| c.has_tag_name((DAV, "response"))).take(MAX_RESPONSES) {
        let Some(href) = child(r, DAV, "href").map(text_of).filter(|h| !h.is_empty() && h.len() <= MAX_URL) else { continue };
        let status = child(r, DAV, "status").and_then(|s| status_code(&text_of(s)));
        let mut props = Props::default();
        for ps in r.children().filter(|c| c.has_tag_name((DAV, "propstat"))) {
            let ok = child(ps, DAV, "status").and_then(|s| status_code(&text_of(s))).is_some_and(|c| (200..300).contains(&c));
            let Some(prop) = child(ps, DAV, "prop").filter(|_| ok) else { continue };
            for p in prop.children().filter(roxmltree::Node::is_element) {
                let (ns, name) = (p.tag_name().namespace().unwrap_or(""), p.tag_name().name());
                match (ns, name) {
                    (DAV, "getetag") => props.etag = kept_tag(&text_of(p), MAX_NAME),
                    (DAV, "displayname") => props.name = Some(text_of(p).chars().take(MAX_NAME).collect()),
                    (DAV, "sync-token") => props.token = kept_tag(&text_of(p), MAX_TAG),
                    (CS, "getctag") => props.ctag = kept_tag(&text_of(p), MAX_TAG),
                    (APPLE, "calendar-color") => props.color = Some(text_of(p).chars().take(16).collect()),
                    (DAV, "resourcetype") => {
                        props.calendar = child(p, CALDAV, "calendar").is_some();
                        props.book = child(p, CARDDAV, "addressbook").is_some();
                        props.collection = child(p, DAV, "collection").is_some();
                    }
                    (CALDAV, "supported-calendar-component-set") => {
                        props.comps_given = true;
                        props.comps = p
                            .children()
                            .filter(|c| c.has_tag_name((CALDAV, "comp")))
                            .filter_map(|c| c.attribute("name"))
                            .map(|n| n.chars().take(20).collect())
                            .take(20)
                            .collect();
                    }
                    (DAV, "current-user-privilege-set") => {
                        props.privileges_given = true;
                        props.privileges = p
                            .descendants()
                            .filter(|c| c.has_tag_name((DAV, "privilege")))
                            .flat_map(|c| c.children().filter(roxmltree::Node::is_element).map(|n| n.tag_name().name().to_string()))
                            .take(50)
                            .collect();
                    }
                    (DAV, "current-user-principal") => props.principal = hrefs(p).into_iter().next(),
                    (CALDAV, "calendar-home-set") => props.homes = hrefs(p),
                    (CALDAV, "calendar-data") | (CARDDAV, "address-data") => props.data = p.text().map(str::to_string),
                    (CARDDAV, "addressbook-home-set") => props.book_homes = hrefs(p),
                    _ => {}
                }
            }
        }
        out.push(Response { href, status, props });
    }
    Ok((out, token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_and_names_are_read_as_they_are_kept() {
        assert_eq!(color("#1e88e5ff").as_deref(), Some("#1E88E5"));
        assert_eq!(color("#1e88e5").as_deref(), Some("#1E88E5"));
        assert_eq!(color("blue"), None);
        assert_eq!(color("#12345"), None);
        assert_eq!(percent_decoded("Work%20%26%20Home"), "Work & Home");
        assert_eq!(percent_decoded("bad%zz%"), "bad%zz%");
        assert_eq!(status_code("HTTP/1.1 404 Not Found"), Some(404));
    }
}
