//! An in-process mail server for tests: IMAP, POP3 and SMTP over an in-memory [`pipe`], with
//! mailboxes held in memory. No socket is opened and nothing leaves the process; this is what
//! the tests of this crate and the engine talk to, never a real server.
//!
//! It speaks only what the clients here say, and checks it: a command it does not know is
//! answered as an error. [`Fake::script`] instead plays back fixed bytes, for hostile-server
//! tests.

mod dav;
mod http;
mod imap;
mod pop3;
mod smtp;

use crate::{About, Connector, End, Endpoint, Fingerprint, NetError, Protocol, Security, Seen, Stream, pipe};
pub use dav::{DavCalendar, DavObject, HOME as DAV_HOME, PRINCIPAL as DAV_PRINCIPAL};
use std::sync::{Arc, Mutex, MutexGuard};

/// A message held by the fake server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub uid: u32,
    pub flags: Vec<String>,
    pub data: Vec<u8>,
    /// When it last changed, as CONDSTORE counts ([RFC 7162] 3.1).
    pub modseq: u64,
}

/// A folder of the fake server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mailbox {
    /// The name as the server knows it (modified UTF-7).
    pub name: String,
    pub attributes: Vec<String>,
    pub uid_validity: u32,
    pub uid_next: u32,
    pub messages: Vec<Message>,
    /// The folder's highest mod-sequence, raised by every change ([RFC 7162] 3.1.1).
    pub modseq: u64,
    /// The server's own id for the folder, which stays with it when it is renamed ([RFC 8474]).
    pub id: String,
    /// Messages taken away, and the mod-sequence they went at, for QRESYNC.
    pub vanished: Vec<(u32, u64)>,
}

impl Mailbox {
    pub fn new(name: &str, attributes: &[&str]) -> Self {
        Self {
            name: name.to_string(),
            attributes: attributes.iter().map(|a| a.to_string()).collect(),
            uid_validity: 1,
            uid_next: 1,
            messages: Vec::new(),
            modseq: 1,
            vanished: Vec::new(),
            id: format!("F{}", name.bytes().map(|b| format!("{b:02x}")).collect::<String>()),
        }
    }

    pub fn add(&mut self, data: &[u8], flags: &[&str]) -> u32 {
        let uid = self.uid_next;
        self.uid_next += 1;
        self.modseq += 1;
        self.messages.push(Message { uid, flags: flags.iter().map(|f| f.to_string()).collect(), data: data.to_vec(), modseq: self.modseq });
        uid
    }

    /// A message's flags changed, as a client elsewhere changes them.
    pub fn set_flags(&mut self, uid: u32, flags: &[&str]) {
        self.modseq += 1;
        let at = self.modseq;
        if let Some(m) = self.messages.iter_mut().find(|m| m.uid == uid) {
            m.flags = flags.iter().map(|f| f.to_string()).collect();
            m.modseq = at;
        }
    }

    /// A message taken away, as a client elsewhere expunges it; where it stood, from 1.
    pub fn remove(&mut self, uid: u32) -> Option<usize> {
        let at = self.messages.iter().position(|m| m.uid == uid)?;
        self.messages.remove(at);
        self.modseq += 1;
        self.vanished.push((uid, self.modseq));
        Some(at + 1)
    }
}

/// A message handed to the fake SMTP server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submitted {
    pub from: String,
    pub to: Vec<String>,
    /// As it came, dots taken off, CRLF line ends.
    pub data: Vec<u8>,
}

/// What the fake server holds and how it behaves.
#[derive(Clone, Debug)]
pub struct State {
    pub user: String,
    pub password: String,
    pub mailboxes: Vec<Mailbox>,
    pub submitted: Vec<Submitted>,
    /// IMAP capabilities beyond IMAP4rev1, STARTTLS and AUTH=PLAIN, which the server
    /// always offers unless they are taken out in `imap_without`.
    pub imap_caps: Vec<String>,
    pub imap_without: Vec<String>,
    /// SMTP extensions offered after EHLO.
    pub smtp_ext: Vec<String>,
    /// POP3 capabilities offered by CAPA.
    pub pop_caps: Vec<String>,
    /// Times a connection was made secure, and sign-ins taken.
    pub secured: usize,
    pub logins: usize,
    /// How many times a client began to wait in IDLE.
    pub idling: usize,
    /// Every command line the server was sent, passwords and all: for the tests to look at.
    pub heard: Vec<String>,
    /// What the fake web server answers, by host and path: the whole answer, head and body.
    pub pages: Vec<(String, Vec<u8>)>,
    /// A certificate of the server's own making, which no system trusts: a connection made
    /// secure is refused unless the endpoint trusts exactly this one.
    pub own_certificate: Option<Fingerprint>,
    /// The UIDVALIDITY the next folder made is given: each folder made has one of its own.
    pub next_validity: u32,
    /// The mail servers that answer, as `host:port`; any other refuses the connection. All
    /// answer when this is `None`.
    pub answers_only: Option<Vec<String>>,
    /// What the fake name server has, by name: records of every kind.
    pub records: Vec<(String, crate::dns::Record)>,
    /// The host the fake CalDAV server answers at; other HTTPS hosts get `pages`.
    pub dav_host: String,
    /// Its calendars, and the counter each change raises, of which sync tokens are made.
    pub calendars: Vec<DavCalendar>,
    pub dav_changes: u64,
    /// It keeps sync tokens ([RFC 6578]); a token older than `dav_forget_before` is forgotten.
    pub dav_sync: bool,
    pub dav_forget_before: u64,
    /// A sync lists no more than this many changes at once, and says there are more (507).
    pub dav_page: Option<usize>,
    /// `/.well-known/caldav` sends the client on to `/dav/` ([RFC 6764] 5).
    pub dav_well_known: bool,
    /// A PUT is answered without the new ETag, as some servers answer it.
    pub dav_no_etag: bool,
}

impl Default for State {
    fn default() -> Self {
        let mut inbox = Mailbox::new("INBOX", &[]);
        inbox.uid_validity = 7;
        Self {
            user: "pat@example.test".into(),
            password: "not a real password".into(),
            mailboxes: vec![inbox, Mailbox::new("Sent", &["\\Sent"]), Mailbox::new("Drafts", &["\\Drafts"]), Mailbox::new("Trash", &["\\Trash"])],
            submitted: Vec::new(),
            imap_caps: vec!["SPECIAL-USE".into(), "UIDPLUS".into()],
            imap_without: Vec::new(),
            idling: 0,
            smtp_ext: vec!["PIPELINING".into(), "SIZE 10485760".into(), "8BITMIME".into(), "STARTTLS".into(), "AUTH PLAIN LOGIN".into()],
            pop_caps: vec!["USER".into(), "UIDL".into(), "STLS".into(), "SASL PLAIN".into()],
            secured: 0,
            logins: 0,
            heard: Vec::new(),
            pages: Vec::new(),
            own_certificate: None,
            next_validity: 100,
            answers_only: None,
            records: Vec::new(),
            dav_host: "dav.example.test".into(),
            calendars: vec![DavCalendar::new("/dav/calendars/pat/personal/", "Personal")],
            dav_changes: 1,
            dav_sync: true,
            dav_forget_before: 0,
            dav_page: None,
            dav_well_known: true,
            dav_no_etag: false,
        }
    }
}

impl State {
    pub fn mailbox(&mut self, name: &str) -> Option<&mut Mailbox> {
        self.mailboxes.iter_mut().find(|m| m.name.eq_ignore_ascii_case(name) || (name.eq_ignore_ascii_case("INBOX") && m.name == "INBOX"))
    }
}

#[derive(Clone)]
enum Mode {
    Serve,
    Script(Vec<u8>),
}

/// The fake server, as a connector the clients use.
#[derive(Clone)]
pub struct Fake {
    state: Arc<Mutex<State>>,
    mode: Mode,
}

impl Default for Fake {
    fn default() -> Self {
        Self::new(State::default())
    }
}

impl Fake {
    pub fn new(state: State) -> Self {
        Self { state: Arc::new(Mutex::new(state)), mode: Mode::Serve }
    }

    /// A server that sends these bytes whatever it is told, hears what comes, and hangs up
    /// once nothing has come for a moment.
    pub fn script(bytes: &[u8]) -> Self {
        Self { state: Arc::new(Mutex::new(State::default())), mode: Mode::Script(bytes.to_vec()) }
    }

    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// An endpoint of this server for a protocol, secured with STARTTLS.
    pub fn endpoint(protocol: Protocol) -> Endpoint {
        Endpoint { protocol, host: "mail.example.test".into(), port: protocol.port(Security::StartTls), security: Security::StartTls, trusted: None }
    }

    /// The certificate of the server's own making refused, as a TLS connector refuses it.
    fn check(&self, to: &Endpoint) -> Result<(), NetError> {
        let Some(own) = self.state().own_certificate else { return Ok(()) };
        if to.trusted == Some(own) {
            return Ok(());
        }
        let about = About {
            subject: to.host.clone(),
            issuer: to.host.clone(),
            names: vec![to.host.clone()],
            not_before: "2026-01-01".into(),
            not_after: "2036-01-01".into(),
        };
        let reason = "it was not signed by anyone this system trusts; a server can make one of its own like this".to_string();
        Err(NetError::Untrusted(Box::new(Seen {
            host: to.host.clone(),
            fingerprint: own,
            reason,
            trustable: true,
            changed: to.trusted.is_some(),
            about,
        })))
    }
}

impl Connector for Fake {
    fn connect(&self, to: &Endpoint) -> Result<Box<dyn Stream>, NetError> {
        let at = format!("{}:{}", to.host, to.port);
        if to.protocol != Protocol::Https && lock(&self.state).answers_only.as_ref().is_some_and(|only| !only.contains(&at)) {
            return Err(NetError::Io(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, format!("nothing answers at {at}"))));
        }
        if to.security == Security::Tls {
            self.check(to)?;
        }
        let (client, server) = pipe();
        let state = self.state.clone();
        let mode = self.mode.clone();
        let protocol = to.protocol;
        let tls = to.security == Security::Tls;
        let host = to.host.clone();
        let dav = protocol == Protocol::Https && host == lock(&self.state).dav_host;
        std::thread::spawn(move || {
            let _ = match mode {
                Mode::Script(bytes) => script(server, &bytes),
                Mode::Serve => match protocol {
                    Protocol::Imap => imap::serve(server, &state, tls),
                    Protocol::Pop3 => pop3::serve(server, &state, tls),
                    Protocol::Smtp => smtp::serve(server, &state, tls),
                    Protocol::Https if dav => dav::serve(server, &state),
                    Protocol::Https => http::serve(server, &state, &host),
                },
            };
        });
        Ok(Box::new(client))
    }

    fn lookup(&self, name: &str, kind: crate::dns::Kind) -> Result<Vec<crate::dns::Record>, NetError> {
        use crate::dns::{Kind, Record};
        let mut state = lock(&self.state);
        state.heard.push(format!("DNS {kind:?} {name}"));
        let name = name.trim_end_matches('.').to_ascii_lowercase();
        let of_kind = |r: &Record| matches!((r, kind), (Record::Mx { .. }, Kind::Mx) | (Record::Srv { .. }, Kind::Srv));
        Ok(state.records.iter().filter(|(at, r)| *at == name && of_kind(r)).map(|(_, r)| r.clone()).collect())
    }

    /// The fake has no TLS: the same pipe goes on, and the server counts that it was asked.
    fn secure(&self, plain: Box<dyn Stream>, to: &Endpoint) -> Result<Box<dyn Stream>, NetError> {
        self.check(to)?;
        Ok(plain)
    }
}

fn script(mut end: End, bytes: &[u8]) -> Result<(), NetError> {
    use std::io::{Read, Write};
    end.write_all(bytes)?;
    let _ = end.set_wait(std::time::Duration::from_millis(150));
    let mut sink = [0u8; 4096];
    while end.read(&mut sink)? > 0 {}
    Ok(())
}

/// Locks the state for one step of a session.
fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
