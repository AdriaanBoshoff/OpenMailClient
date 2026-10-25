//! A connection read a line or a counted run of bytes at a time, with every read capped.

use crate::{Connector, NetError, Stream};
use std::io::ErrorKind;

/// The longest line read; a server that sends a longer one is broken or hostile.
pub const MAX_LINE: usize = 64 * 1024;
/// Bytes asked of the stream at a time.
const CHUNK: usize = 16 * 1024;

/// An error of the stream, as this crate says it: waiting too long is timing out.
pub(crate) fn io_error(e: std::io::Error) -> NetError {
    match e.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => NetError::TimedOut,
        _ => NetError::Io(e),
    }
}

pub struct Conn {
    s: Box<dyn Stream>,
    buf: Vec<u8>,
    at: usize,
}

impl Conn {
    pub fn new(s: Box<dyn Stream>) -> Self {
        Self { s, buf: Vec::new(), at: 0 }
    }

    fn fill(&mut self) -> Result<(), NetError> {
        if self.at > 0 {
            self.buf.drain(..self.at);
            self.at = 0;
        }
        let start = self.buf.len();
        self.buf.resize(start + CHUNK, 0);
        let n = loop {
            match self.s.read(self.buf.get_mut(start..).unwrap_or_default()) {
                Ok(n) => break n,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.buf.truncate(start);
                    return Err(io_error(e));
                }
            }
        };
        self.buf.truncate(start + n);
        if n == 0 {
            return Err(NetError::Closed);
        }
        Ok(())
    }

    fn pending(&self) -> &[u8] {
        self.buf.get(self.at..).unwrap_or_default()
    }

    /// One line, without its CRLF (a bare LF is taken as the end too). Longer than `MAX_LINE`
    /// is an error.
    pub fn line(&mut self) -> Result<Vec<u8>, NetError> {
        let mut looked = 0;
        loop {
            if let Some(i) = self.pending().get(looked..).and_then(|p| p.iter().position(|b| *b == b'\n')) {
                let end = looked + i;
                let mut line = self.pending().get(..end).unwrap_or_default().to_vec();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                self.at += end + 1;
                return Ok(line);
            }
            looked = self.pending().len();
            if looked > MAX_LINE {
                return Err(NetError::TooLarge("a line from the server"));
            }
            self.fill()?;
        }
    }

    /// Exactly `n` bytes; `n` is checked against the cap by the caller.
    pub fn exact(&mut self, n: usize) -> Result<Vec<u8>, NetError> {
        let mut out = Vec::with_capacity(n.min(CHUNK * 64));
        while out.len() < n {
            if self.pending().is_empty() {
                self.fill()?;
            }
            let take = (n - out.len()).min(self.pending().len());
            out.extend_from_slice(self.pending().get(..take).unwrap_or_default());
            self.at += take;
        }
        Ok(out)
    }

    /// What comes until the other end closes, no more than `limit` bytes of it.
    pub fn until_closed(&mut self, limit: usize) -> Result<Vec<u8>, NetError> {
        loop {
            if self.pending().len() > limit {
                return Err(NetError::TooLarge("what the server sent"));
            }
            match self.fill() {
                Ok(()) => {}
                Err(NetError::Closed) => break,
                Err(e) => return Err(e),
            }
        }
        let out = self.pending().to_vec();
        self.at = self.buf.len();
        Ok(out)
    }

    /// What has come or comes next, no more than `max` bytes of it; nothing once the other end
    /// has closed.
    pub fn some(&mut self, max: usize) -> Result<Vec<u8>, NetError> {
        if self.pending().is_empty() {
            match self.fill() {
                Ok(()) => {}
                Err(NetError::Closed) => return Ok(Vec::new()),
                Err(e) => return Err(e),
            }
        }
        let take = max.max(1).min(self.pending().len());
        let out = self.pending().get(..take).unwrap_or_default().to_vec();
        self.at += take;
        Ok(out)
    }

    /// How long a read waits from now on; what it waited before, where the stream can say.
    pub(crate) fn set_wait(&mut self, wait: std::time::Duration) -> Result<Option<std::time::Duration>, NetError> {
        Ok(self.s.set_wait(wait)?)
    }

    pub fn send(&mut self, bytes: &[u8]) -> Result<(), NetError> {
        self.s.write_all(bytes).map_err(|e| match e.kind() {
            ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted => NetError::Closed,
            ErrorKind::TimedOut | ErrorKind::WouldBlock => NetError::TimedOut,
            _ => e.into(),
        })?;
        self.s.flush()?;
        Ok(())
    }

    /// TLS over this connection (after STARTTLS or STLS). Anything the server sent after its
    /// answer and before TLS began is refused: it was said in the clear and could have been
    /// put there by someone between us and the server ([RFC 3207] 4.2, [RFC 9051] 6.2.1).
    pub fn secure(self, with: &dyn Connector, to: &crate::Endpoint) -> Result<Self, NetError> {
        if !self.pending().is_empty() {
            return Err(NetError::protocol("the server said more before the connection was made secure"));
        }
        Ok(Self::new(with.secure(self.s, to)?))
    }
}
