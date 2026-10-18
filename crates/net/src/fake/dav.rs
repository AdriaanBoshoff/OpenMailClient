//! The fake CalDAV server: one request a connection, as `dav` sends them. It keeps calendars of
//! events by name, an ETag for each and a counter every change raises, from which its sync
//! tokens are made ([RFC 6578]); it answers PROPFIND as a principal, a calendar home and a
//! calendar would, REPORT for a sync and a calendar-multiget, and PUT and DELETE only while
//! `If-Match` and `If-None-Match` hold.
//!
//! [RFC 6578]: https://www.rfc-editor.org/rfc/rfc6578

use super::{State, lock};
use crate::{Conn, End, NetError};
use std::sync::Mutex;

/// An event of a fake calendar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DavObject {
    /// Its name in the calendar: `<uid>.ics`.
    pub name: String,
    pub etag: String,
    pub data: String,
    /// The change it was last put at.
    pub changed: u64,
}

/// A calendar of the fake server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DavCalendar {
    /// Its path, ending in `/`.
    pub href: String,
    pub name: String,
    pub color: String,
    pub objects: Vec<DavObject>,
    /// Events deleted, and the change they went at.
    pub removed: Vec<(String, u64)>,
    /// Only read may be done in it.
    pub read_only: bool,
}

impl DavCalendar {
    pub fn new(href: &str, name: &str) -> Self {
        Self { href: href.into(), name: name.into(), color: "#3366CCFF".into(), objects: Vec::new(), removed: Vec::new(), read_only: false }
    }
}

/// Where the fake's principal and calendar home are.
pub const PRINCIPAL: &str = "/dav/principals/pat/";
pub const HOME: &str = "/dav/calendars/pat/";

impl State {
    /// A calendar of the fake CalDAV server by its path.
    pub fn dav_calendar(&mut self, href: &str) -> Option<&mut DavCalendar> {
        self.calendars.iter_mut().find(|c| c.href == href)
    }

    /// An event put in a calendar, as a client elsewhere puts it; its new ETag.
    pub fn dav_put(&mut self, calendar: &str, name: &str, data: &str) -> Option<String> {
        self.dav_changes += 1;
        let at = self.dav_changes;
        let etag = format!("\"e{at}\"");
        let cal = self.calendars.iter_mut().find(|c| c.href == calendar)?;
        cal.removed.retain(|(n, _)| n != name);
        match cal.objects.iter_mut().find(|o| o.name == name) {
            Some(o) => {
                o.data = data.to_string();
                o.etag = etag.clone();
                o.changed = at;
            }
            None => cal.objects.push(DavObject { name: name.into(), etag: etag.clone(), data: data.into(), changed: at }),
        }
        Some(etag)
    }

    /// An event deleted from a calendar, as a client elsewhere deletes it.
    pub fn dav_remove(&mut self, calendar: &str, name: &str) -> bool {
        self.dav_changes += 1;
        let at = self.dav_changes;
        let Some(cal) = self.calendars.iter_mut().find(|c| c.href == calendar) else { return false };
        let had = cal.objects.len();
        cal.objects.retain(|o| o.name != name);
        if cal.objects.len() == had {
            return false;
        }
        cal.removed.push((name.to_string(), at));
        true
    }

    /// The calendar's sync token now.
    fn token(&self) -> String {
        format!("http://dav.example.test/sync/{}", self.dav_changes)
    }
}

struct Request {
    method: String,
    path: String,
    depth: String,
    if_match: Option<String>,
    if_none_match: Option<String>,
    authorized: bool,
    body: String,
}

fn read_request(conn: &mut Conn, state: &Mutex<State>) -> Result<Request, NetError> {
    use base64::Engine as _;
    let line = String::from_utf8_lossy(&conn.line()?).into_owned();
    let mut parts = line.split(' ');
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let (mut length, mut depth, mut if_match, mut if_none_match, mut auth) = (0usize, String::new(), None, None, String::new());
    loop {
        let head = String::from_utf8_lossy(&conn.line()?).into_owned();
        if head.is_empty() {
            break;
        }
        let Some((name, value)) = head.split_once(':') else { continue };
        let value = value.trim().to_string();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => length = value.parse().unwrap_or(0),
            "depth" => depth = value,
            "if-match" => if_match = Some(value),
            "if-none-match" => if_none_match = Some(value),
            "authorization" => auth = value,
            _ => {}
        }
    }
    let body = String::from_utf8_lossy(&conn.exact(length.min(16 << 20))?).into_owned();
    let mut state = lock(state);
    state.heard.push(line.clone());
    let want = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", state.user, state.password));
    let authorized = auth.strip_prefix("Basic ").is_some_and(|a| a == want);
    if authorized {
        state.logins += 1;
    }
    Ok(Request { method, path, depth, if_match, if_none_match, authorized, body })
}

fn escape(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn answer(code: u16, extra: &str, body: &str) -> Vec<u8> {
    let reason = match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        207 => "Multi-Status",
        301 => "Moved Permanently",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        412 => "Precondition Failed",
        _ => "Other",
    };
    let mut head = format!("HTTP/1.1 {code} {reason}\r\n{extra}Content-Length: {}\r\n", body.len());
    if !body.is_empty() {
        head.push_str("Content-Type: application/xml; charset=utf-8\r\n");
    }
    head.push_str("\r\n");
    let mut out = head.into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

fn multistatus(inner: &str, token: Option<&str>) -> String {
    let token = token.map(|t| format!("<d:sync-token>{}</d:sync-token>", escape(t))).unwrap_or_default();
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<d:multistatus xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\" xmlns:cs=\"http://calendarserver.org/ns/\" xmlns:ic=\"http://apple.com/ns/ical/\">{inner}{token}</d:multistatus>"
    )
}

fn ok(href: &str, props: &str) -> String {
    format!(
        "<d:response><d:href>{}</d:href><d:propstat><d:prop>{props}</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
        escape(href)
    )
}

fn status(href: &str, code: &str) -> String {
    format!("<d:response><d:href>{}</d:href><d:status>HTTP/1.1 {code}</d:status></d:response>", escape(href))
}

pub fn serve(end: End, state: &Mutex<State>) -> Result<(), NetError> {
    let mut conn = Conn::new(Box::new(end));
    let r = read_request(&mut conn, state)?;
    let reply = if r.authorized { respond(&r, &mut lock(state)) } else { answer(401, "WWW-Authenticate: Basic realm=\"fake\"\r\n", "") };
    conn.send(&reply)
}

fn respond(r: &Request, s: &mut State) -> Vec<u8> {
    let path = r.path.as_str();
    match r.method.as_str() {
        "PROPFIND" if path == "/.well-known/caldav" => {
            if s.dav_well_known {
                answer(301, "Location: /dav/\r\n", "")
            } else {
                answer(404, "", "")
            }
        }
        "PROPFIND" if path == "/dav/" || path == PRINCIPAL => {
            let props = format!(
                "<d:current-user-principal><d:href>{PRINCIPAL}</d:href></d:current-user-principal><c:calendar-home-set><d:href>{HOME}</d:href></c:calendar-home-set>"
            );
            answer(207, "", &multistatus(&ok(path, &props), None))
        }
        "PROPFIND" if path == HOME => {
            let mut inner = ok(HOME, "<d:resourcetype><d:collection/></d:resourcetype>");
            if r.depth == "1" {
                let token = s.token();
                for c in &s.calendars {
                    inner.push_str(&ok(&c.href, &calendar_props(c, s, &token)));
                }
            }
            answer(207, "", &multistatus(&inner, None))
        }
        "PROPFIND" => match s.calendars.iter().find(|c| c.href == path) {
            Some(c) => {
                let mut inner = ok(&c.href, &calendar_props(c, s, &s.token()));
                for o in c.objects.iter().filter(|_| r.depth == "1") {
                    inner.push_str(&ok(&format!("{}{}", c.href, o.name), &format!("<d:resourcetype/><d:getetag>{}</d:getetag>", escape(&o.etag))));
                }
                answer(207, "", &multistatus(&inner, None))
            }
            None => answer(404, "", ""),
        },
        "REPORT" => report(r, s),
        "PUT" => put(r, s),
        "DELETE" => delete(r, s),
        _ => answer(405, "", ""),
    }
}

/// What a calendar is, as a PROPFIND of it says.
fn calendar_props(c: &DavCalendar, s: &State, token: &str) -> String {
    let privileges = if c.read_only {
        "<d:privilege><d:read/></d:privilege>"
    } else {
        "<d:privilege><d:read/></d:privilege><d:privilege><d:write/></d:privilege>"
    };
    let sync = if s.dav_sync { format!("<d:sync-token>{token}</d:sync-token>") } else { String::new() };
    format!(
        "<d:resourcetype><d:collection/><c:calendar/></d:resourcetype><d:displayname>{}</d:displayname><cs:getctag>{}</cs:getctag>{sync}<c:supported-calendar-component-set><c:comp name=\"VEVENT\"/></c:supported-calendar-component-set><ic:calendar-color>{}</ic:calendar-color><d:current-user-privilege-set>{privileges}</d:current-user-privilege-set>",
        escape(&c.name),
        s.dav_changes,
        escape(&c.color)
    )
}

fn report(r: &Request, s: &mut State) -> Vec<u8> {
    let Ok(doc) = roxmltree::Document::parse(&r.body) else { return answer(400, "", "") };
    let root = doc.root_element();
    let token_now = s.token();
    let (sync, page, forget) = (s.dav_sync, s.dav_page, s.dav_forget_before);
    let Some(cal) = s.calendars.iter().find(|c| c.href == r.path) else { return answer(404, "", "") };
    if root.has_tag_name(("DAV:", "sync-collection")) {
        if !sync {
            return answer(403, "", "<?xml version=\"1.0\"?><d:error xmlns:d=\"DAV:\"><d:supported-report/></d:error>");
        }
        let given = root.descendants().find(|n| n.has_tag_name(("DAV:", "sync-token"))).and_then(|n| n.text()).unwrap_or("").trim().to_string();
        let since = if given.is_empty() {
            0
        } else {
            match given.strip_prefix("http://dav.example.test/sync/").and_then(|n| n.parse::<u64>().ok()) {
                Some(n) if n >= forget => n,
                _ => return answer(403, "", "<?xml version=\"1.0\"?><d:error xmlns:d=\"DAV:\"><d:valid-sync-token/></d:error>"),
            }
        };
        // Every change after `since`, oldest first; an initial sync lists no removed events.
        let mut all: Vec<(u64, String)> = cal
            .objects
            .iter()
            .filter(|o| o.changed > since)
            .map(|o| (o.changed, ok(&format!("{}{}", cal.href, o.name), &format!("<d:getetag>{}</d:getetag>", escape(&o.etag)))))
            .collect();
        if since > 0 {
            all.extend(
                cal.removed.iter().filter(|(_, at)| *at > since).map(|(name, at)| (*at, status(&format!("{}{name}", cal.href), "404 Not Found"))),
            );
        }
        all.sort_by_key(|(at, _)| *at);
        let (shown, token) = match page {
            Some(n) if all.len() > n => {
                let last = all.get(n.saturating_sub(1)).map(|(at, _)| *at).unwrap_or(since);
                let mut inner: String = all.iter().take(n).map(|(_, x)| x.as_str()).collect();
                inner.push_str(&status(&cal.href, "507 Insufficient Storage"));
                (inner, format!("http://dav.example.test/sync/{last}"))
            }
            _ => (all.iter().map(|(_, x)| x.as_str()).collect(), token_now),
        };
        return answer(207, "", &multistatus(&shown, Some(&token)));
    }
    if root.has_tag_name(("urn:ietf:params:xml:ns:caldav", "calendar-multiget")) {
        let mut inner = String::new();
        for href in root.children().filter(|n| n.has_tag_name(("DAV:", "href"))).filter_map(|n| n.text()) {
            match cal.objects.iter().find(|o| format!("{}{}", cal.href, o.name) == href) {
                Some(o) => inner.push_str(&ok(
                    href,
                    &format!("<d:getetag>{}</d:getetag><c:calendar-data>{}</c:calendar-data>", escape(&o.etag), escape(&o.data)),
                )),
                None => inner.push_str(&status(href, "404 Not Found")),
            }
        }
        return answer(207, "", &multistatus(&inner, None));
    }
    answer(400, "", "")
}

/// The calendar an event's path is in, and its name there.
fn split(path: &str) -> Option<(String, String)> {
    let (cal, name) = path.rsplit_once('/')?;
    (!name.is_empty()).then(|| (format!("{cal}/"), name.to_string()))
}

fn put(r: &Request, s: &mut State) -> Vec<u8> {
    let Some((href, name)) = split(&r.path) else { return answer(405, "", "") };
    let Some(cal) = s.calendars.iter().find(|c| c.href == href) else { return answer(404, "", "") };
    if cal.read_only {
        return answer(403, "", "");
    }
    let there = cal.objects.iter().find(|o| o.name == name).map(|o| o.etag.clone());
    let refused = match (&r.if_none_match, &r.if_match, &there) {
        (Some(n), _, Some(_)) if n == "*" => true,
        (_, Some(want), Some(etag)) => want != etag,
        (_, Some(_), None) => true,
        _ => false,
    };
    if refused {
        return answer(412, "", "");
    }
    let no_etag = s.dav_no_etag;
    let Some(etag) = s.dav_put(&href, &name, &r.body) else { return answer(404, "", "") };
    let head = if no_etag { String::new() } else { format!("ETag: {etag}\r\n") };
    answer(if there.is_some() { 204 } else { 201 }, &head, "")
}

fn delete(r: &Request, s: &mut State) -> Vec<u8> {
    let Some((href, name)) = split(&r.path) else { return answer(405, "", "") };
    let Some(cal) = s.calendars.iter().find(|c| c.href == href) else { return answer(404, "", "") };
    if cal.read_only {
        return answer(403, "", "");
    }
    let Some(etag) = cal.objects.iter().find(|o| o.name == name).map(|o| o.etag.clone()) else { return answer(404, "", "") };
    if r.if_match.as_ref().is_some_and(|want| *want != etag) {
        return answer(412, "", "");
    }
    s.dav_remove(&href, &name);
    answer(204, "", "")
}
