//! Pictures a message names on a web server, fetched only when the reader asks: one `GET`
//! over HTTPS ([RFC 9110], [RFC 9112]), through the same connector the mail clients use. The
//! same `GET` asks a web service what it knows (`get_as`) and brings a large file down a piece
//! at a time into wherever it is put (`download`), as an update of the program is.
//!
//! What the request says about the reader is as little as can be: no cookies, no referrer, a
//! plain user agent and no language. Plain HTTP is not used, so what is fetched cannot be
//! read or changed on the way, and nothing is asked of this machine or of the local network
//! by a name or address a message gave. What the server says is hostile: the head, the body,
//! each chunk and the number of redirects are all capped.

use crate::conn::{Conn, MAX_LINE};
use crate::{Connector, Endpoint, NetError, Protocol, Security, said};

/// The largest picture fetched.
pub const MAX_PICTURE: usize = 8 * 1024 * 1024;
/// Redirects followed for one picture.
const MAX_REDIRECTS: usize = 5;
/// Lines of a response's head, and the longest address followed.
const MAX_HEAD_LINES: usize = 100;
const MAX_URL: usize = 2048;
/// Chunks of a chunked body.
const MAX_CHUNKS: usize = 100_000;
/// The largest file downloaded, such as an update of the program.
pub const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
/// The longest address followed to a download: a release's file is handed out at a long
/// signed address.
const MAX_LONG_URL: usize = 8192;
/// Bytes read and handed on at a time while a file comes down.
const PIECE: usize = 256 * 1024;

/// A picture as a server sent it.
#[derive(Debug)]
pub struct Fetched {
    pub bytes: Vec<u8>,
    /// What the server said it is (`Content-Type`), without its parameters.
    pub kind: String,
}

/// Where a `https:` address points: the server and what is asked of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub host: String,
    pub port: u16,
    /// The path and query, as sent in the request line.
    pub target: String,
}

/// Read a `https:` address ([RFC 9110] 4.2.2). Anything else, an address with a user name or
/// password in it, and a server on this machine or the local network, is refused.
pub fn address(url: &str) -> Result<Address, NetError> {
    address_within(url, MAX_URL)
}

fn address_within(url: &str, longest: usize) -> Result<Address, NetError> {
    let url = url.trim();
    if url.len() > longest || url.chars().any(|c| c.is_control() || c == ' ') {
        return Err(NetError::Unsupported("the address of the picture is not one that is fetched".into()));
    }
    let scheme_end = url.find("://").ok_or_else(|| NetError::Unsupported("the picture has no web address".into()))?;
    let scheme = url.get(..scheme_end).unwrap_or("");
    if !scheme.eq_ignore_ascii_case("https") {
        return Err(NetError::Unsupported(format!("only pictures over HTTPS are fetched, not {}", said(scheme))));
    }
    let rest = url.get(scheme_end + 3..).unwrap_or("");
    let cut = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest.get(..cut).unwrap_or("");
    let mut target: String = rest.get(cut..).unwrap_or("").split('#').next().unwrap_or("").to_string();
    if target.is_empty() || target.starts_with('?') {
        target.insert(0, '/');
    }
    if !target.is_ascii() {
        return Err(NetError::Unsupported("the address of the picture is not plain ASCII".into()));
    }
    if authority.contains('@') {
        return Err(NetError::Unsupported("an address with a name and password in it is not fetched".into()));
    }
    // An IPv6 address stands in brackets, its port after them ([RFC 3986] 3.2.2).
    let (host, port) = match authority.strip_prefix('[') {
        Some(inner) => {
            let (host, after) = inner.split_once(']').ok_or_else(|| NetError::Unsupported("the picture's address is not whole".into()))?;
            (host, after.strip_prefix(':'))
        }
        None => match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };
    let port = match port {
        Some(port) => port
            .parse::<u16>()
            .ok()
            .filter(|p| *p > 0)
            .ok_or_else(|| NetError::Unsupported("the picture's address has a port that is not one".into()))?,
        None => 443,
    };
    let host = host.to_ascii_lowercase();
    if host.is_empty() || host.len() > 253 || !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':')) {
        return Err(NetError::Unsupported("the picture's server has a name that is not looked up".into()));
    }
    if is_local(&host) {
        return Err(NetError::Unsupported("a picture on this machine or the local network is not fetched".into()));
    }
    Ok(Address { host, port, target })
}

/// A name or address of this machine or of a private network.
pub(crate) fn is_local(host: &str) -> bool {
    use std::net::IpAddr;
    let name = host.trim_end_matches('.');
    if name == "localhost" || name.ends_with(".localhost") || name.ends_with(".local") || name.ends_with(".internal") || !name.contains(['.', ':']) {
        return true;
    }
    name.parse::<IpAddr>().is_ok_and(local_ip)
}

/// An address of this machine or of a private network. A name a message gave is looked up
/// only to be refused here if it leads to one, so no name can be used to reach them.
pub(crate) fn local_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
        }
        IpAddr::V6(v6) => {
            let first = v6.segments().first().copied().unwrap_or(0);
            v6.is_loopback()
                || v6.is_unspecified()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || v6.to_ipv4_mapped().is_some_and(|v4| local_ip(IpAddr::V4(v4)))
        }
    }
}

/// Fetch a picture, following up to five redirects to other HTTPS addresses. At most `limit`
/// bytes of it are taken; a larger one is an error.
pub fn get(with: &dyn Connector, url: &str, limit: usize) -> Result<Fetched, NetError> {
    let limit = limit.min(MAX_PICTURE);
    let mut at = address(url)?;
    for _ in 0..=MAX_REDIRECTS {
        match once(with, &at, limit, "image/*")? {
            Answer::Got(fetched) => return Ok(fetched),
            Answer::Moved(to) => at = address(&joined(&at, &to))?,
        }
    }
    Err(NetError::Refused("the picture was moved too many times".into()))
}

/// Where a `Location` points, taken against the address it came from ([RFC 9110] 10.2.2).
fn joined(from: &Address, to: &str) -> String {
    let to = to.trim();
    if to.contains("://") {
        return to.to_string();
    }
    let origin = if from.port == 443 { format!("https://{}", from.host) } else { format!("https://{}:{}", from.host, from.port) };
    if let Some(rest) = to.strip_prefix("//") {
        return format!("https://{rest}");
    }
    if to.starts_with('/') {
        return format!("{origin}{to}");
    }
    let base = from.target.split('?').next().unwrap_or("/");
    let dir = base.rsplit_once('/').map_or("", |(dir, _)| dir);
    format!("{origin}{dir}/{to}")
}

enum Answer {
    Got(Fetched),
    Moved(String),
}

/// Fetch a settings file a domain publishes for mail programs, as `get` fetches a picture.
pub fn get_settings(with: &dyn Connector, url: &str, limit: usize) -> Result<Fetched, NetError> {
    get_as(with, url, limit, "text/xml, application/xml")
}

/// Fetch what a web service answers, saying what is wanted (`Accept`), as `get` fetches a
/// picture: such as what a project's releases are.
pub fn get_as(with: &dyn Connector, url: &str, limit: usize, accept: &str) -> Result<Fetched, NetError> {
    let accept: String = accept.chars().filter(|c| c.is_ascii_graphic() || *c == ' ').take(200).collect();
    let mut at = address(url)?;
    for _ in 0..=MAX_REDIRECTS {
        match once(with, &at, limit.min(MAX_PICTURE), &accept)? {
            Answer::Got(fetched) => return Ok(fetched),
            Answer::Moved(to) => at = address(&joined(&at, &to))?,
        }
    }
    Err(NetError::Refused("what was asked for was moved too many times".into()))
}

/// Bring a file down into `into` a piece at a time, following up to five redirects to other
/// HTTPS addresses, so that however large it is no more than a piece of it is held. `heard`
/// is told how many bytes have come and how many there are when the server says, and stops
/// the download by answering false. A file larger than `limit` is an error before or as it
/// comes. Gives back how many bytes came.
pub fn download(
    with: &dyn Connector,
    url: &str,
    limit: u64,
    into: &mut dyn std::io::Write,
    heard: &mut dyn FnMut(u64, Option<u64>) -> bool,
) -> Result<u64, NetError> {
    let limit = limit.min(MAX_DOWNLOAD);
    let mut at = address_within(url, MAX_LONG_URL)?;
    for _ in 0..=MAX_REDIRECTS {
        let (mut conn, head) = ask(with, &at, "application/octet-stream")?;
        match head.code {
            200 => return body_into(&mut conn, &head, limit, into, heard),
            301 | 302 | 303 | 307 | 308 => {
                let to = head.location.ok_or_else(|| NetError::protocol("moved, but not said where"))?;
                at = address_within(&joined(&at, &to), MAX_LONG_URL)?;
            }
            code => return Err(NetError::Refused(format!("the web server answered {code}"))),
        }
    }
    Err(NetError::Refused("the file was moved too many times".into()))
}

/// The body of an answer, handed on a piece at a time.
fn body_into(
    conn: &mut Conn,
    head: &Head,
    limit: u64,
    into: &mut dyn std::io::Write,
    heard: &mut dyn FnMut(u64, Option<u64>) -> bool,
) -> Result<u64, NetError> {
    let mut got: u64 = 0;
    let mut take = |piece: &[u8], got: &mut u64, total: Option<u64>| -> Result<(), NetError> {
        into.write_all(piece)?;
        *got = got.saturating_add(piece.len() as u64);
        if heard(*got, total) { Ok(()) } else { Err(NetError::Refused("the download was stopped".into())) }
    };
    if head.chunked {
        for _ in 0..MAX_CHUNKS {
            let line = conn.line()?;
            let size = String::from_utf8_lossy(&line);
            let size = size.split(';').next().unwrap_or("").trim();
            let size = u64::from_str_radix(size, 16).map_err(|_| NetError::protocol("a chunk size that is not a number"))?;
            if size == 0 {
                return Ok(got);
            }
            if size > limit.saturating_sub(got) {
                return Err(NetError::TooLarge("the file"));
            }
            let mut left = size;
            while left > 0 {
                let n = usize::try_from(left).unwrap_or(PIECE).min(PIECE);
                let piece = conn.exact(n)?;
                take(&piece, &mut got, None)?;
                left = left.saturating_sub(n as u64);
            }
            conn.line()?;
        }
        return Err(NetError::TooLarge("the number of chunks of the file"));
    }
    if let Some(length) = head.length {
        if length > limit {
            return Err(NetError::TooLarge("the file"));
        }
        let mut left = length;
        while left > 0 {
            let n = usize::try_from(left).unwrap_or(PIECE).min(PIECE);
            let piece = conn.exact(n)?;
            take(&piece, &mut got, Some(length))?;
            left = left.saturating_sub(n as u64);
        }
    } else {
        loop {
            let piece = conn.some(PIECE)?;
            if piece.is_empty() {
                break;
            }
            if piece.len() as u64 > limit.saturating_sub(got) {
                return Err(NetError::TooLarge("the file"));
            }
            take(&piece, &mut got, None)?;
        }
    }
    Ok(got)
}

/// What the head of a web server's answer says.
struct Head {
    code: u16,
    length: Option<u64>,
    chunked: bool,
    kind: String,
    location: Option<String>,
}

fn once(with: &dyn Connector, at: &Address, limit: usize, accept: &str) -> Result<Answer, NetError> {
    let (mut conn, head) = ask(with, at, accept)?;
    match head.code {
        200 => {}
        301 | 302 | 303 | 307 | 308 => {
            return head.location.map(Answer::Moved).ok_or_else(|| NetError::protocol("moved, but not said where"));
        }
        code => return Err(NetError::Refused(format!("the web server answered {code}"))),
    }
    let bytes = if head.chunked {
        chunks(&mut conn, limit, "the picture")?
    } else if let Some(length) = head.length {
        let length = usize::try_from(length).map_err(|_| NetError::TooLarge("the picture"))?;
        if length > limit {
            return Err(NetError::TooLarge("the picture"));
        }
        conn.exact(length)?
    } else {
        conn.until_closed(limit)?
    };
    Ok(Answer::Got(Fetched { bytes, kind: head.kind }))
}

/// Send the request, and read the status line and the head after it.
fn ask(with: &dyn Connector, at: &Address, accept: &str) -> Result<(Conn, Head), NetError> {
    let to = Endpoint { protocol: Protocol::Https, host: at.host.clone(), port: at.port, security: Security::Tls, trusted: None };
    let mut conn = Conn::new(with.connect(&to)?);
    let host = if at.port == 443 { at.host.clone() } else { format!("{}:{}", at.host, at.port) };
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: OpenMailClient\r\nAccept: {accept}\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
        at.target
    );
    conn.send(request.as_bytes())?;
    // The status line, and the head after it.
    let status = String::from_utf8_lossy(&conn.line()?).into_owned();
    let mut parts = status.split(' ');
    let version = parts.next().unwrap_or("");
    let code = parts.next().and_then(|c| c.parse::<u16>().ok()).filter(|_| version.starts_with("HTTP/1."));
    let Some(code) = code else { return Err(NetError::protocol(format!("not a web server's answer: {status}"))) };
    let (mut length, mut chunked, mut kind, mut location) = (None::<u64>, false, String::new(), None::<String>);
    for n in 0.. {
        let line = conn.line()?;
        if line.is_empty() {
            break;
        }
        if n >= MAX_HEAD_LINES {
            return Err(NetError::TooLarge("the head of the web server's answer"));
        }
        let line = String::from_utf8_lossy(&line).into_owned();
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => length = Some(value.parse::<u64>().map_err(|_| NetError::protocol("a length that is not a number"))?),
            "transfer-encoding" => chunked = value.to_ascii_lowercase().split(',').any(|v| v.trim() == "chunked"),
            "content-type" => kind = value.split(';').next().unwrap_or("").trim().to_ascii_lowercase().chars().take(100).collect(),
            "location" => location = Some(value.chars().take(MAX_LONG_URL + 1).collect()),
            _ => {}
        }
    }
    Ok((conn, Head { code, length, chunked, kind, location }))
}

/// A chunked body ([RFC 9112] 7.1): each chunk's size in hexadecimal, then the chunk.
pub(crate) fn chunks(conn: &mut Conn, limit: usize, what: &'static str) -> Result<Vec<u8>, NetError> {
    let mut out = Vec::new();
    for _ in 0..MAX_CHUNKS {
        let line = conn.line()?;
        let size = String::from_utf8_lossy(&line);
        let size = size.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size, 16).map_err(|_| NetError::protocol("a chunk size that is not a number"))?;
        if size == 0 {
            return Ok(out);
        }
        if size > limit.saturating_sub(out.len()) || size > MAX_LINE * 1024 {
            return Err(NetError::TooLarge(what));
        }
        out.extend(conn.exact(size)?);
        // The line end after the chunk.
        conn.line()?;
    }
    Err(NetError::TooLarge("the number of chunks of the picture"))
}
