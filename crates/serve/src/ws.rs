//! Just enough WebSocket (RFC 6455) for one channel of commands between a browser and the host.
//! Frames are taken from a buffer the connection fills, so a read that times out halfway through
//! a frame loses nothing; every length is bounded before anything is kept.

use crate::http::Request;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

/// The largest message, whole, from a browser.
pub const MAX_MESSAGE: usize = 1 << 20;
/// The most bytes kept waiting to make up frames.
const MAX_BUFFER: usize = MAX_MESSAGE + 16;

/// RFC 6455 §1.3: the key every server adds to the browser's before hashing it.
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// The `Sec-WebSocket-Accept` for a browser's `Sec-WebSocket-Key` (§4.2.2).
pub fn accept_key(key: &str) -> String {
    let joined = format!("{key}{GUID}");
    STANDARD.encode(ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, joined.as_bytes()))
}

/// The browser's key when `request` asks for a WebSocket as §4.2.1 says it must.
pub fn upgrade_key(request: &Request) -> Result<&str, &'static str> {
    let has = |name: &str, token: &str| request.header(name).is_some_and(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case(token)));
    if request.method != "GET" {
        return Err("a WebSocket is asked for with GET");
    }
    if !has("upgrade", "websocket") || !has("connection", "upgrade") {
        return Err("not a WebSocket request");
    }
    if request.header("sec-websocket-version") != Some("13") {
        return Err("only WebSocket version 13 is spoken");
    }
    let key = request.header("sec-websocket-key").ok_or("no Sec-WebSocket-Key")?;
    match STANDARD.decode(key) {
        Ok(bytes) if bytes.len() == 16 => Ok(key),
        _ => Err("a Sec-WebSocket-Key that is not 16 bytes of base64"),
    }
}

/// One message from the browser.
#[derive(Debug, PartialEq, Eq)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong,
    /// The browser closes; answered with a close of our own.
    Close,
}

/// Why the channel is closed on the browser: the close code to send (§7.4.1).
#[derive(Debug, PartialEq, Eq)]
pub enum Broken {
    /// 1002: not WebSocket as the RFC has it.
    Protocol(&'static str),
    /// 1007: text that is not UTF-8.
    NotText,
    /// 1009: too large.
    TooLarge,
}

impl Broken {
    pub fn code(&self) -> u16 {
        match self {
            Broken::Protocol(_) => 1002,
            Broken::NotText => 1007,
            Broken::TooLarge => 1009,
        }
    }
}

/// Frames taken from bytes as they come.
#[derive(Default)]
pub struct Reader {
    buffer: Vec<u8>,
    /// A message sent in pieces: its opcode and what has come of it.
    partial: Option<(u8, Vec<u8>)>,
}

impl Reader {
    pub fn new() -> Reader {
        Reader::default()
    }

    /// Bytes read from the connection.
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), Broken> {
        if self.buffer.len().saturating_add(bytes.len()) > MAX_BUFFER {
            return Err(Broken::TooLarge);
        }
        self.buffer.extend_from_slice(bytes);
        Ok(())
    }

    /// The next whole message, or `None` until more bytes come.
    pub fn message(&mut self) -> Result<Option<Message>, Broken> {
        loop {
            let Some((fin, opcode, payload, used)) = self.frame()? else { return Ok(None) };
            self.buffer.drain(..used.min(self.buffer.len()));
            match opcode {
                0x8 => return Ok(Some(Message::Close)),
                0x9 => return Ok(Some(Message::Ping(payload))),
                0xA => return Ok(Some(Message::Pong)),
                0x0 => {
                    let Some((first, mut so_far)) = self.partial.take() else { return Err(Broken::Protocol("a continuation of nothing")) };
                    if so_far.len().saturating_add(payload.len()) > MAX_MESSAGE {
                        return Err(Broken::TooLarge);
                    }
                    so_far.extend_from_slice(&payload);
                    if fin {
                        return whole(first, so_far).map(Some);
                    }
                    self.partial = Some((first, so_far));
                }
                0x1 | 0x2 => {
                    if self.partial.is_some() {
                        return Err(Broken::Protocol("a new message before the last one ended"));
                    }
                    if fin {
                        return whole(opcode, payload).map(Some);
                    }
                    self.partial = Some((opcode, payload));
                }
                _ => return Err(Broken::Protocol("an opcode WebSocket does not have")),
            }
        }
    }

    /// One frame from the front of the buffer, unmasked: whether it ends its message, its opcode,
    /// its payload and how many bytes it took (§5.2).
    fn frame(&self) -> Result<Option<(bool, u8, Vec<u8>, usize)>, Broken> {
        let b = &self.buffer;
        let (Some(&b0), Some(&b1)) = (b.first(), b.get(1)) else { return Ok(None) };
        if b0 & 0x70 != 0 {
            return Err(Broken::Protocol("reserved bits set"));
        }
        let (fin, opcode) = (b0 & 0x80 != 0, b0 & 0x0F);
        if b1 & 0x80 == 0 {
            return Err(Broken::Protocol("a browser's frame must be masked"));
        }
        let (len, mut at) = match b1 & 0x7F {
            126 => match b.get(2..4).and_then(|n| <[u8; 2]>::try_from(n).ok()) {
                Some(n) => (u64::from(u16::from_be_bytes(n)), 4),
                None => return Ok(None),
            },
            127 => match b.get(2..10).and_then(|n| <[u8; 8]>::try_from(n).ok()) {
                Some(n) => (u64::from_be_bytes(n), 10),
                None => return Ok(None),
            },
            n => (u64::from(n), 2),
        };
        if opcode >= 0x8 && (!fin || len > 125) {
            return Err(Broken::Protocol("a control frame in pieces or longer than 125 bytes"));
        }
        let len = usize::try_from(len).ok().filter(|n| *n <= MAX_MESSAGE).ok_or(Broken::TooLarge)?;
        let Some(mask) = b.get(at..at + 4).and_then(|m| <[u8; 4]>::try_from(m).ok()) else { return Ok(None) };
        at += 4;
        let Some(masked) = b.get(at..at + len) else { return Ok(None) };
        let payload = masked.iter().zip(mask.iter().cycle()).map(|(byte, m)| byte ^ m).collect();
        Ok(Some((fin, opcode, payload, at + len)))
    }
}

fn whole(opcode: u8, payload: Vec<u8>) -> Result<Message, Broken> {
    if opcode == 0x1 { String::from_utf8(payload).map(Message::Text).map_err(|_| Broken::NotText) } else { Ok(Message::Binary(payload)) }
}

/// A frame from the server: never masked, never in pieces (§5.1).
pub fn frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 10);
    out.push(0x80 | (opcode & 0x0F));
    match payload.len() {
        n if n < 126 => out.push(n as u8),
        n if n <= usize::from(u16::MAX) => {
            out.push(126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
    out
}

pub fn text(payload: &str) -> Vec<u8> {
    frame(0x1, payload.as_bytes())
}

/// A close frame with its code.
pub fn close(code: u16) -> Vec<u8> {
    frame(0x8, &code.to_be_bytes())
}

pub fn pong(payload: &[u8]) -> Vec<u8> {
    frame(0xA, payload.get(..125).unwrap_or(payload))
}

pub fn ping() -> Vec<u8> {
    frame(0x9, b"")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame as a browser sends it, masked.
    fn masked(b0: u8, payload: &[u8]) -> Vec<u8> {
        let mask = [0x37, 0xfa, 0x21, 0x3d];
        let mut out = vec![b0];
        match payload.len() {
            n if n < 126 => out.push(0x80 | n as u8),
            n if n <= 0xffff => {
                out.push(0x80 | 126);
                out.extend_from_slice(&(n as u16).to_be_bytes());
            }
            n => {
                out.push(0x80 | 127);
                out.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        out.extend_from_slice(&mask);
        out.extend(payload.iter().zip(mask.iter().cycle()).map(|(b, m)| b ^ m));
        out
    }

    #[test]
    fn the_accept_key_is_the_rfcs() {
        // RFC 6455 §1.3's own example.
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn messages_come_whole_in_pieces_and_byte_by_byte() {
        let mut r = Reader::new();
        let mut bytes = masked(0x01, b"Hel");
        bytes.extend(masked(0x89, b"hi"));
        bytes.extend(masked(0x80, b"lo"));
        bytes.extend(masked(0x82, &[1u8; 300]));
        bytes.extend(masked(0x88, &1000u16.to_be_bytes()));
        let mut got = Vec::new();
        for b in bytes {
            r.push(&[b]).unwrap();
            while let Some(m) = r.message().unwrap() {
                got.push(m);
            }
        }
        assert_eq!(got, vec![Message::Ping(b"hi".to_vec()), Message::Text("Hello".into()), Message::Binary(vec![1; 300]), Message::Close]);
        assert_eq!(frame(0x1, &[0; 70_000]).len(), 70_010);
    }

    #[test]
    fn hostile_frames_close_the_channel() {
        let cases: Vec<(Vec<u8>, u16)> = vec![
            (vec![0x81, 0x02, b'h', b'i'], 1002),                                     // not masked
            (masked(0xC1, b"x"), 1002),                                               // reserved bit
            (masked(0x83, b"x"), 1002),                                               // no such opcode
            (masked(0x80, b"x"), 1002),                                               // continuation of nothing
            (masked(0x09, b"x"), 1002),                                               // a ping in pieces
            (masked(0x89, &[0; 126]), 1002),                                          // a long ping
            (masked(0x81, &[0xff, 0xfe]), 1007),                                      // not UTF-8
            (vec![0x82, 0xFF, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff], 1009), // 2^63 bytes
            ([masked(0x01, b"a"), masked(0x01, b"b")].concat(), 1002),                // two at once
        ];
        for (bytes, code) in cases {
            let mut r = Reader::new();
            let got = r.push(&bytes).and_then(|()| {
                loop {
                    match r.message() {
                        Ok(Some(_)) => continue,
                        other => break other.map(|_| ()),
                    }
                }
            });
            assert_eq!(got.map_err(|e| e.code()), Err(code), "{bytes:?}");
        }
        let mut r = Reader::new();
        assert_eq!(r.push(&vec![0; MAX_BUFFER + 1]), Err(Broken::TooLarge));
        // A message in many pieces adds up to no more than the largest.
        let mut r = Reader::new();
        r.push(&masked(0x01, &[b'a'; 60_000])).unwrap();
        assert_eq!(r.message(), Ok(None));
        let mut over = Ok(None);
        for _ in 0..20 {
            r.push(&masked(0x00, &[b'a'; 60_000])).unwrap();
            over = r.message();
            if over.is_err() {
                break;
            }
        }
        assert_eq!(over, Err(Broken::TooLarge));
    }

    #[test]
    fn garbage_never_panics() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        for n in 0..3000 {
            let mut r = Reader::new();
            let bytes: Vec<u8> = (0..n % 64)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    seed as u8
                })
                .collect();
            if r.push(&bytes).is_ok() {
                for _ in 0..8 {
                    if !matches!(r.message(), Ok(Some(_))) {
                        break;
                    }
                }
            }
        }
    }
}
