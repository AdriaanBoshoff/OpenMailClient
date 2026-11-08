//! Just enough HTTP/1.1 (RFC 9112) for web access: requests read with every size bounded, and
//! responses written with the headers every page of ours carries. Anyone on the network can send
//! these bytes, so nothing here trusts a length, a count or an encoding it is given.

use std::io::{BufRead, Write};

/// The longest request line or header line.
pub const MAX_LINE: usize = 8 * 1024;
/// The most header lines.
pub const MAX_HEADERS: usize = 64;
/// The most bytes of all header lines together.
pub const MAX_HEAD: usize = 32 * 1024;
/// The largest body a request may have; ours are sign-in forms.
pub const MAX_BODY: usize = 16 * 1024;

/// A request as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// The path without its query.
    pub path: String,
    pub query: String,
    /// Header names in lower case, in the order sent.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// The first header named `name` (lower case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// Whether the client asked for the connection to end after this request.
    pub fn closes(&self) -> bool {
        self.header("connection").is_some_and(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case("close")))
    }

    /// The value of the cookie named `name`.
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .filter(|(n, _)| n == "cookie")
            .flat_map(|(_, v)| v.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v)
    }

    /// The body read as `application/x-www-form-urlencoded`, its field `name`.
    pub fn form(&self, name: &str) -> Option<String> {
        let body = std::str::from_utf8(&self.body).ok()?;
        body.split('&')
            .filter_map(|pair| pair.split_once('=').or(Some((pair, ""))))
            .find(|(n, _)| percent_decode(n).as_deref() == Some(name))
            .and_then(|(_, v)| percent_decode(v))
    }
}

/// Why a request could not be read; each is answered with the status it names, or with nothing.
#[derive(Debug, PartialEq, Eq)]
pub enum Bad {
    /// The connection ended or went quiet before a request began; nothing is answered.
    Gone,
    /// Malformed; 400.
    Malformed(&'static str),
    /// A line or the headers too long; 431.
    TooLarge,
    /// A body too long; 413.
    BodyTooLarge,
    /// A transfer coding we do not take; 501.
    Unsupported,
}

impl Bad {
    pub fn status(&self) -> Option<u16> {
        match self {
            Bad::Gone => None,
            Bad::Malformed(_) => Some(400),
            Bad::TooLarge => Some(431),
            Bad::BodyTooLarge => Some(413),
            Bad::Unsupported => Some(501),
        }
    }
}

/// Read one line ending in CRLF (or LF), at most [`MAX_LINE`] bytes, without the line end.
fn line(input: &mut impl BufRead, budget: &mut usize) -> Result<Option<String>, Bad> {
    let mut bytes = Vec::new();
    loop {
        let chunk = match input.fill_buf() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(Bad::Gone),
        };
        if chunk.is_empty() {
            return if bytes.is_empty() { Ok(None) } else { Err(Bad::Gone) };
        }
        let (take, done) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        if bytes.len().saturating_add(take) > MAX_LINE || take > *budget {
            return Err(Bad::TooLarge);
        }
        bytes.extend_from_slice(chunk.get(..take).unwrap_or(&[]));
        *budget -= take;
        input.consume(take);
        if done {
            break;
        }
    }
    while bytes.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        bytes.pop();
    }
    String::from_utf8(bytes).map(Some).map_err(|_| Bad::Malformed("a line that is not UTF-8"))
}

/// Read one request.
pub fn read(input: &mut impl BufRead) -> Result<Request, Bad> {
    let mut budget = MAX_HEAD;
    // Empty lines before a request are let pass (RFC 9112 §2.2), a few of them.
    let mut first = String::new();
    for _ in 0..4 {
        match line(input, &mut budget)? {
            None => return Err(Bad::Gone),
            Some(l) if l.is_empty() => continue,
            Some(l) => {
                first = l;
                break;
            }
        }
    }
    let mut parts = first.split(' ');
    let (Some(method), Some(target), Some(version), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(Bad::Malformed("a request line that is not three parts"));
    };
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(Bad::Malformed("a method that is not a word"));
    }
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(Bad::Malformed("a version other than HTTP/1.x"));
    }
    if !target.starts_with('/') || target.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return Err(Bad::Malformed("a target that is not a path"));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut headers = Vec::new();
    loop {
        let Some(l) = line(input, &mut budget)? else { return Err(Bad::Gone) };
        if l.is_empty() {
            break;
        }
        if headers.len() >= MAX_HEADERS {
            return Err(Bad::TooLarge);
        }
        let Some((name, value)) = l.split_once(':') else { return Err(Bad::Malformed("a header without a colon")) };
        if name.is_empty() || name.bytes().any(|b| !(b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))) {
            return Err(Bad::Malformed("a header name that is not a token"));
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
    }
    let request = Request { method: method.to_string(), path: path.to_string(), query: query.to_string(), headers, body: Vec::new() };
    if request.header("transfer-encoding").is_some() {
        return Err(Bad::Unsupported);
    }
    let lengths: Vec<&str> = request.headers.iter().filter(|(n, _)| n == "content-length").map(|(_, v)| v.as_str()).collect();
    let length = match lengths.as_slice() {
        [] => 0,
        [one] if !one.is_empty() && one.bytes().all(|b| b.is_ascii_digit()) && one.len() <= 9 => {
            one.parse::<usize>().map_err(|_| Bad::Malformed("a length"))?
        }
        _ => return Err(Bad::Malformed("a length that is not one number")),
    };
    if length > MAX_BODY {
        return Err(Bad::BodyTooLarge);
    }
    let mut body = vec![0u8; length];
    input.read_exact(&mut body).map_err(|_| Bad::Gone)?;
    Ok(Request { body, ..request })
}

/// `%XX` and `+` read back, as forms send them; `None` when it is not UTF-8 or a `%` is not followed by two hex digits.
pub fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        match b {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            other => out.push(other),
        }
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// Text made safe to put in HTML.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// The policy every page of ours is served with: nothing from elsewhere, no inline scripts, no
/// framing. The web build's own scripts and WebAssembly come from this host alone.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// A response to be written.
#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Response {
        Response { status, headers: vec![("Content-Type".into(), content_type.into())], body: body.into() }
    }

    pub fn html(status: u16, body: String) -> Response {
        Response::new(status, "text/html; charset=utf-8", body)
    }

    pub fn text(status: u16, body: &str) -> Response {
        Response::new(status, "text/plain; charset=utf-8", body)
    }

    /// See other: go to `to` with a GET.
    pub fn redirect(to: &str) -> Response {
        Response::text(303, "").with("Location", to)
    }

    pub fn with(mut self, name: &str, value: &str) -> Response {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// Write it, with the headers every response of ours carries; `close` ends the connection after.
    pub fn write(&self, out: &mut impl Write, close: bool) -> std::io::Result<()> {
        let mut head = format!("HTTP/1.1 {} {}\r\n", self.status, reason(self.status));
        for (n, v) in &self.headers {
            // A header value of ours never holds a line end; one that would is left out.
            if !v.contains(['\r', '\n']) && !n.contains(['\r', '\n', ':']) {
                head.push_str(&format!("{n}: {v}\r\n"));
            }
        }
        let has = |name: &str| self.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name));
        if !has("Cache-Control") {
            head.push_str("Cache-Control: no-store\r\n");
        }
        head.push_str(&format!("Content-Length: {}\r\n", self.body.len()));
        head.push_str("Strict-Transport-Security: max-age=31536000\r\n");
        head.push_str(&format!("Content-Security-Policy: {CONTENT_SECURITY_POLICY}\r\n"));
        head.push_str("X-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\n");
        head.push_str("Cross-Origin-Opener-Policy: same-origin\r\nCross-Origin-Resource-Policy: same-origin\r\n");
        head.push_str("Permissions-Policy: camera=(), microphone=(), geolocation=(), payment=()\r\n");
        if close {
            head.push_str("Connection: close\r\n");
        }
        head.push_str("\r\n");
        out.write_all(head.as_bytes())?;
        out.write_all(&self.body)?;
        out.flush()
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        101 => "Switching Protocols",
        200 => "OK",
        301 => "Moved Permanently",
        303 => "See Other",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Content Too Large",
        421 => "Misdirected Request",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(bytes: &[u8]) -> Result<Request, Bad> {
        read(&mut std::io::BufReader::new(bytes))
    }

    #[test]
    fn a_request_is_read() {
        let r = parse(b"POST /sign-in?x=1 HTTP/1.1\r\nHost: 127.0.0.1:8443\r\nCookie: a=1; __Host-omc=abc\r\nContent-Length: 26\r\n\r\npassword=a+b%21c&keep=on&z").unwrap();
        assert_eq!((r.method.as_str(), r.path.as_str(), r.query.as_str()), ("POST", "/sign-in", "x=1"));
        assert_eq!(r.header("host"), Some("127.0.0.1:8443"));
        assert_eq!(r.cookie("__Host-omc"), Some("abc"));
        assert_eq!(r.form("password").as_deref(), Some("a b!c"));
        assert_eq!(r.form("keep").as_deref(), Some("on"));
        assert_eq!(r.form("nothing"), None);
    }

    #[test]
    fn hostile_requests_are_refused_by_their_size_or_shape() {
        let long = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(MAX_LINE));
        assert_eq!(parse(long.as_bytes()), Err(Bad::TooLarge));
        let many: String = (0..=MAX_HEADERS).map(|i| format!("X-{i}: y\r\n")).collect();
        assert_eq!(parse(format!("GET / HTTP/1.1\r\n{many}\r\n").as_bytes()), Err(Bad::TooLarge));
        let wide: String = (0..40).map(|i| format!("X-{i}: {}\r\n", "y".repeat(1000))).collect();
        assert_eq!(parse(format!("GET / HTTP/1.1\r\n{wide}\r\n").as_bytes()), Err(Bad::TooLarge));
        assert_eq!(parse(b"POST / HTTP/1.1\r\nContent-Length: 99999999\r\n\r\n"), Err(Bad::BodyTooLarge));
        assert_eq!(parse(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n"), Err(Bad::Unsupported));
        for junk in [
            &b"GET / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nab"[..],
            b"GET / HTTP/1.1\r\nContent-Length: -1\r\n\r\n",
            b"GET / HTTP/1.1\r\nContent-Length: 1e3\r\n\r\n",
            b"GET / HTTP/2\r\n\r\n",
            b"get / HTTP/1.1\r\n\r\n",
            b"GET http://elsewhere/ HTTP/1.1\r\n\r\n",
            b"GET / HTTP/1.1 extra\r\n\r\n",
            b"GET / HTTP/1.1\r\nNo colon here\r\n\r\n",
            b"GET / HTTP/1.1\r\nBad Name: x\r\n\r\n",
            b"GET /\xff HTTP/1.1\r\n\r\n",
        ] {
            assert!(matches!(parse(junk), Err(Bad::Malformed(_))), "{:?}", String::from_utf8_lossy(junk));
        }
        assert_eq!(parse(b""), Err(Bad::Gone));
        assert_eq!(parse(b"GET / HTTP/1.1\r\nHost: x"), Err(Bad::Gone));
        assert_eq!(parse(b"POST / HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc"), Err(Bad::Gone));
    }

    #[test]
    fn garbage_never_panics() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for n in 0..2000 {
            let len = n % 300;
            let bytes: Vec<u8> = (0..len)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    b"GET /POST HTTP/1.1\r\n:%+= Content-Length: 9\r\n"[(seed % 44) as usize]
                })
                .collect();
            let _ = parse(&bytes);
        }
        for bad in ["%", "%4", "%zz", "%ff", "a%2"] {
            assert_eq!(percent_decode(bad), None, "{bad}");
        }
    }

    #[test]
    fn responses_carry_our_headers_and_never_a_split_line() {
        let mut out = Vec::new();
        Response::text(200, "hi").with("X-Evil", "a\r\nSet-Cookie: x=1").write(&mut out, true).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(!text.contains("Set-Cookie"));
        for h in [
            "Content-Security-Policy: default-src 'self'",
            "Strict-Transport-Security",
            "X-Frame-Options: DENY",
            "Content-Length: 2",
            "Connection: close",
            "Cache-Control: no-store",
        ] {
            assert!(text.contains(h), "{h}");
        }
        assert!(text.ends_with("\r\n\r\nhi"));
        assert_eq!(escape("<a href=\"x\">&'"), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
    }
}
