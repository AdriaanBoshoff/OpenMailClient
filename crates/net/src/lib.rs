//! Mail on the network: IMAP ([RFC 9051], [RFC 3501]), POP3 ([RFC 1939]) and SMTP submission
//! ([RFC 5321], [RFC 6409]) clients.
//!
//! The clients speak over a [`Stream`] a [`Connector`] gives them, so the protocols are the same
//! code whether the bytes go to a server over TLS or to the in-process fake server the tests use
//! (`fake`, behind the feature of that name). Nothing in this crate outside a connector opens a
//! socket.
//!
//! What a server says is hostile: every line, literal, list and count it sends is capped before
//! anything is allocated for it, and a reply that breaks the protocol is an error, never a panic.
//! Passwords are held in a [`Secret`], which never prints and is never put in an error.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod cert;
mod conn;
pub mod dav;
pub mod dns;
pub mod find;
pub mod http;
pub mod imap;
mod pipe;
pub mod pop3;
pub mod smtp;
pub mod utf7;
mod vault;

#[cfg(not(target_arch = "wasm32"))]
mod tcp;
#[cfg(not(target_arch = "wasm32"))]
pub use tcp::TcpConnector;

#[cfg(feature = "fake")]
pub mod fake;

pub use cert::{About, Fingerprint, Seen};
pub use conn::Conn;
pub use pipe::{End, pipe};
pub use utf7::{decode as decode_mailbox_name, encode as encode_mailbox_name};
pub use vault::{MemoryVault, SystemVault, Vault};

use std::io::{Read, Write};

/// The largest message taken from a server or handed to one.
pub const MAX_MESSAGE: usize = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("could not reach the server: {0}")]
    Io(#[from] std::io::Error),
    #[error("the server closed the connection")]
    Closed,
    #[error("the server did not answer in time")]
    TimedOut,
    #[error("the server broke the protocol: {0}")]
    Protocol(String),
    #[error("the server refused: {0}")]
    Refused(String),
    #[error("the user name or password was not accepted: {0}")]
    Auth(String),
    #[error("the server does not offer a secure connection, and this account asks for one")]
    NotSecure,
    #[error("the server's certificate was not accepted: {0}")]
    Certificate(String),
    /// A certificate this system does not trust, which the owner may trust for this server.
    #[error("the certificate of {} is not trusted: {}", .0.host, .0.reason)]
    Untrusted(Box<Seen>),
    #[error("the system's credential store could not be used: {0}")]
    Vault(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("{0} is too large")]
    TooLarge(&'static str),
}

impl NetError {
    pub(crate) fn protocol(what: impl Into<String>) -> Self {
        Self::Protocol(said(&what.into()))
    }
}

/// What a server said, fit to be shown: no control characters, and no more than 200 of them.
pub(crate) fn said(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(200).collect()
}

/// How a connection is kept from being read by others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    /// TLS from the first byte (IMAP on 993, POP3 on 995, SMTP on 465).
    Tls,
    /// A plain connection made secure before anything is said that matters (STARTTLS, STLS);
    /// a server that does not offer it is not used.
    StartTls,
    /// No TLS at all. For a server on this machine, or the fake one.
    Plain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Imap,
    Pop3,
    Smtp,
    /// A web server, for a picture a message names. Only over TLS.
    Https,
}

impl Protocol {
    /// The port a protocol uses with a kind of security, when none is given.
    pub fn port(self, security: Security) -> u16 {
        match (self, security) {
            (Self::Imap, Security::Tls) => 993,
            (Self::Imap, _) => 143,
            (Self::Pop3, Security::Tls) => 995,
            (Self::Pop3, _) => 110,
            (Self::Smtp, Security::Tls) => 465,
            (Self::Smtp, _) => 587,
            (Self::Https, _) => 443,
        }
    }
}

/// A server to talk to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub security: Security,
    /// A certificate the owner trusted for this server, though this system does not: the
    /// server is taken to be itself when it shows exactly that one.
    pub trusted: Option<Fingerprint>,
}

/// A password or token. It never prints, so it does not end up in a log or an error.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The secret itself, for the one place it is said to the server.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// Who signs in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Login {
    pub user: String,
    pub password: Secret,
}

/// Bytes both ways to a server.
pub trait Stream: Read + Write + Send {
    /// How long a read waits before it times out, from now on; what it waited before, where
    /// the stream can say. A folder watched with IDLE is read a second at a time, so that
    /// watching can be stopped, and then read as before.
    fn set_wait(&mut self, _wait: std::time::Duration) -> std::io::Result<Option<std::time::Duration>> {
        Ok(None)
    }
}

impl Stream for std::net::TcpStream {
    fn set_wait(&mut self, wait: std::time::Duration) -> std::io::Result<Option<std::time::Duration>> {
        let was = self.read_timeout()?;
        self.set_read_timeout(Some(wait))?;
        Ok(was)
    }
}

impl Stream for End {
    fn set_wait(&mut self, wait: std::time::Duration) -> std::io::Result<Option<std::time::Duration>> {
        Ok(Some(End::set_wait(self, wait)))
    }
}

impl Stream for Box<dyn Stream> {
    fn set_wait(&mut self, wait: std::time::Duration) -> std::io::Result<Option<std::time::Duration>> {
        (**self).set_wait(wait)
    }
}

/// Opens connections, and makes a plain one secure.
pub trait Connector: Send + Sync {
    /// A connection to the endpoint, made secure from the first byte when it asks for TLS.
    fn connect(&self, to: &Endpoint) -> Result<Box<dyn Stream>, NetError>;
    /// The same connection after STARTTLS: TLS over it, the certificate checked as `connect`
    /// checks it for that endpoint.
    fn secure(&self, plain: Box<dyn Stream>, to: &Endpoint) -> Result<Box<dyn Stream>, NetError>;
    /// The records of `kind` the DNS has for `name`; none when the name does not exist.
    fn lookup(&self, _name: &str, _kind: dns::Kind) -> Result<Vec<dns::Record>, NetError> {
        Err(NetError::Unsupported("names cannot be looked up from here".into()))
    }
}

/// SASL PLAIN ([RFC 4616]): authorisation identity left empty, the user and the password.
pub(crate) fn sasl_plain(login: &Login) -> String {
    use base64::Engine as _;
    let raw = format!("\0{}\0{}", login.user, login.password.expose());
    base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
}

/// Text with no line breaks or NULs, which could end a command early and start another.
pub(crate) fn one_line(what: &'static str, text: &str) -> Result<(), NetError> {
    if text.chars().any(|c| matches!(c, '\r' | '\n' | '\0')) {
        return Err(NetError::Unsupported(format!("the {what} holds a line break")));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_cert;
#[cfg(test)]
mod tests_dav;
#[cfg(test)]
mod tests_dns;
#[cfg(test)]
mod tests_find;
#[cfg(test)]
mod tests_http;
#[cfg(test)]
mod tests_tls;
