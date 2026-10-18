//! An account's servers found from its address, as Auto Account Setup finds them, in this
//! order:
//!
//! 1. what is known of large providers, kept here (`KNOWN`);
//! 2. the settings the address's domain publishes for mail programs, an XML file at
//!    `https://autoconfig.<domain>/mail/config-v1.1.xml` or under
//!    `https://<domain>/.well-known/autoconfig/`, fetched over HTTPS only;
//! 3. the servers the domain names in its DNS SRV records for mail programs ([RFC 6186],
//!    with the implicit-TLS services of [RFC 8314] first);
//! 4. the provider that takes the domain's mail, known by its mail exchangers (MX): a domain
//!    whose mail goes to Google, Microsoft 365, Yahoo, Fastmail, Zoho, iCloud or Yandex is
//!    given that provider's servers, which are kept here, never names read from the answer;
//! 5. the usual names of a domain's mail servers (`imap.`, `mail.`, `smtp.` and the domain
//!    itself), each greeted over TLS, or made secure with STARTTLS, and signed in to by none.
//!
//! The address itself is sent to no one: only its domain is named, in the server names asked
//! for. What a web server sends is hostile: its size is capped, it is read as XML without
//! anything fetched for it, and a name or port in it that is not one is passed over. A server
//! that offers no secure connection is never chosen. DNS answers are not signed and can be
//! changed on the way, so what SRV records name comes with a note to check it; it is only
//! ever filled into the form, for the owner to keep or change, and its certificate is checked
//! as any server's is.
//!
//! [RFC 6186]: https://www.rfc-editor.org/rfc/rfc6186
//! [RFC 8314]: https://www.rfc-editor.org/rfc/rfc8314

use crate::dns::{Kind, Record};
use crate::{Connector, Endpoint, NetError, Protocol, Security, http};

/// The settings file is no larger than this.
const MAX_SETTINGS: usize = 64 * 1024;
/// Nodes read of it.
const MAX_NODES: u32 = 4096;
/// Elements inside one another: the reader goes down each with a call of its own, so a file
/// nested deeper than this is not read at all. A settings file needs four.
const MAX_DEPTH: usize = 32;

/// Where the settings were found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// What is known here of a large provider.
    Known,
    /// The settings the domain publishes.
    Published,
    /// The servers the domain's SRV records name.
    Records,
    /// The provider its mail exchangers (MX) belong to.
    Exchanger,
    /// The usual names, tried: each server chosen greeted us.
    Tried,
}

impl Source {
    pub fn word(self) -> &'static str {
        match self {
            Self::Known => "known",
            Self::Published => "published",
            Self::Records => "records",
            Self::Exchanger => "exchanger",
            Self::Tried => "tried",
        }
    }
}

/// A server, as an account names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub host: String,
    pub port: u16,
    pub security: Security,
}

/// The settings found for an address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// IMAP or POP3.
    pub protocol: Protocol,
    pub incoming: Server,
    pub outgoing: Server,
    /// The name to sign in with.
    pub user: String,
    pub from: Source,
    /// What the owner should know: that a provider asks for an app password, say.
    pub note: Option<&'static str>,
}

/// Large providers: their domains, IMAP and SMTP servers, and a note. From what each provider
/// publishes for mail programs; our own list.
const KNOWN: &[(&[&str], (&str, u16, Security), (&str, u16, Security), Option<&str>)] = &[
    (
        &["gmail.com", "googlemail.com"],
        ("imap.gmail.com", 993, Security::Tls),
        ("smtp.gmail.com", 465, Security::Tls),
        Some("Google signs in mail programs with an app password made in the Google account, or with Google's own sign-in (later)"),
    ),
    (
        &["outlook.com", "hotmail.com", "live.com", "msn.com", "hotmail.co.uk", "outlook.co.uk", "live.co.uk"],
        ("outlook.office365.com", 993, Security::Tls),
        ("smtp-mail.outlook.com", 587, Security::StartTls),
        Some("Microsoft signs in mail programs with its own sign-in (later); a password alone may be refused"),
    ),
    (
        &["yahoo.com", "ymail.com", "rocketmail.com", "yahoo.co.uk"],
        ("imap.mail.yahoo.com", 993, Security::Tls),
        ("smtp.mail.yahoo.com", 465, Security::Tls),
        Some("Yahoo signs in mail programs with an app password made in the Yahoo account"),
    ),
    (
        &["aol.com"],
        ("imap.aol.com", 993, Security::Tls),
        ("smtp.aol.com", 465, Security::Tls),
        Some("AOL signs in mail programs with an app password made in the AOL account"),
    ),
    (
        &["icloud.com", "me.com", "mac.com"],
        ("imap.mail.me.com", 993, Security::Tls),
        ("smtp.mail.me.com", 587, Security::StartTls),
        Some("Apple signs in mail programs with an app-specific password made in the Apple account"),
    ),
    (
        &["fastmail.com", "fastmail.fm"],
        ("imap.fastmail.com", 993, Security::Tls),
        ("smtp.fastmail.com", 465, Security::Tls),
        Some("Fastmail signs in mail programs with an app password"),
    ),
    (&["gmx.com", "gmx.net", "gmx.de"], ("imap.gmx.com", 993, Security::Tls), ("mail.gmx.com", 587, Security::StartTls), None),
    (&["zoho.com", "zohomail.com"], ("imap.zoho.com", 993, Security::Tls), ("smtp.zoho.com", 465, Security::Tls), None),
    (
        &["yandex.com", "yandex.ru"],
        ("imap.yandex.com", 993, Security::Tls),
        ("smtp.yandex.com", 465, Security::Tls),
        Some("Yandex signs in mail programs with an app password"),
    ),
];

/// Providers known by the names of their mail exchangers: what each name ends with, and the
/// provider's IMAP and SMTP servers for a domain of its customer's own.
const EXCHANGERS: &[(&[&str], (&str, u16, Security), (&str, u16, Security), Option<&str>)] = &[
    (
        &["google.com", "googlemail.com"],
        ("imap.gmail.com", 993, Security::Tls),
        ("smtp.gmail.com", 465, Security::Tls),
        Some("Google Workspace signs in mail programs with an app password made in the Google account, or with Google's own sign-in (later)"),
    ),
    (
        &["outlook.com"],
        ("outlook.office365.com", 993, Security::Tls),
        ("smtp.office365.com", 587, Security::StartTls),
        Some("Microsoft 365 signs in mail programs with its own sign-in (later); a password alone may be refused"),
    ),
    (
        &["yahoodns.net"],
        ("imap.mail.yahoo.com", 993, Security::Tls),
        ("smtp.mail.yahoo.com", 465, Security::Tls),
        Some("Yahoo signs in mail programs with an app password made in the Yahoo account"),
    ),
    (
        &["messagingengine.com"],
        ("imap.fastmail.com", 993, Security::Tls),
        ("smtp.fastmail.com", 465, Security::Tls),
        Some("Fastmail signs in mail programs with an app password"),
    ),
    (&["zoho.com"], ("imap.zoho.com", 993, Security::Tls), ("smtp.zoho.com", 465, Security::Tls), None),
    (&["zoho.eu"], ("imap.zoho.eu", 993, Security::Tls), ("smtp.zoho.eu", 465, Security::Tls), None),
    (
        &["icloud.com"],
        ("imap.mail.me.com", 993, Security::Tls),
        ("smtp.mail.me.com", 587, Security::StartTls),
        Some("Apple signs in mail programs with an app-specific password made in the Apple account"),
    ),
    (
        &["yandex.net"],
        ("imap.yandex.com", 993, Security::Tls),
        ("smtp.yandex.com", 465, Security::Tls),
        Some("Yandex signs in mail programs with an app password"),
    ),
];

/// What the owner is told of servers named by DNS records.
const RECORDS_NOTE: &str =
    "These servers are named in the domain's DNS records, which can be changed on the way to this computer: check them before signing in";

/// The services a domain may name for reading mail ([RFC 6186] 3.2, [RFC 8314] 5.1), the
/// implicit-TLS ones first, then those made secure with STARTTLS; and for sending (3.1).
const INCOMING_SERVICES: &[(&str, Protocol, Security)] = &[
    ("_imaps._tcp", Protocol::Imap, Security::Tls),
    ("_imap._tcp", Protocol::Imap, Security::StartTls),
    ("_pop3s._tcp", Protocol::Pop3, Security::Tls),
    ("_pop3._tcp", Protocol::Pop3, Security::StartTls),
];
const OUTGOING_SERVICES: &[(&str, Security)] = &[("_submissions._tcp", Security::Tls), ("_submission._tcp", Security::StartTls)];

/// The server SRV records name first: the lowest priority, and of that priority the highest
/// weight ([RFC 2782]; the weight shares a load among servers, and one is chosen here). A
/// target of `.` says the service is not offered ([RFC 6186] 3.4), and a name or port that is
/// not one is passed over.
///
/// [RFC 2782]: https://www.rfc-editor.org/rfc/rfc2782
pub fn first_named(records: &[Record]) -> Option<(String, u16)> {
    records
        .iter()
        .filter_map(|r| match r {
            Record::Srv { priority, weight, port, target } if *port != 0 && is_host(target) => Some((*priority, *weight, target, *port)),
            _ => None,
        })
        .min_by_key(|(priority, weight, ..)| (*priority, std::cmp::Reverse(*weight)))
        .map(|(.., target, port)| (target.clone(), port))
}

/// The servers `domain`'s SRV records name; `None` unless both reading and sending are named.
fn by_records(with: &dyn Connector, email: &str, domain: &str, stopped: &dyn Fn() -> bool) -> Result<Option<Found>, NetError> {
    let named = |service: &str| -> Result<Option<(String, u16)>, NetError> {
        if stopped() {
            return Err(NetError::Unsupported("stopped".into()));
        }
        Ok(with.lookup(&format!("{service}.{domain}"), Kind::Srv).ok().and_then(|records| first_named(&records)))
    };
    let mut incoming = None;
    for (service, protocol, security) in INCOMING_SERVICES {
        if let Some((host, port)) = named(service)? {
            incoming = Some((*protocol, Server { host, port, security: *security }));
            break;
        }
    }
    let Some((protocol, incoming)) = incoming else { return Ok(None) };
    for (service, security) in OUTGOING_SERVICES {
        if let Some((host, port)) = named(service)? {
            let outgoing = Server { host, port, security: *security };
            return Ok(Some(Found { protocol, incoming, outgoing, user: email.trim().to_string(), from: Source::Records, note: Some(RECORDS_NOTE) }));
        }
    }
    Ok(None)
}

/// The provider whose mail exchanger comes first among `records`, the lowest preference; its
/// servers are those kept here.
pub fn exchanger(records: &[Record], email: &str) -> Option<Found> {
    let first = records
        .iter()
        .filter_map(|r| match r {
            Record::Mx { preference, exchange } => Some((*preference, exchange)),
            _ => None,
        })
        .min_by_key(|(preference, _)| *preference)?
        .1;
    let ends = |suffix: &str| first == suffix || first.strip_suffix(suffix).is_some_and(|rest| rest.ends_with('.'));
    let (_, inc, out, note) = EXCHANGERS.iter().find(|(suffixes, ..)| suffixes.iter().any(|s| ends(s)))?;
    let server = |(host, port, security): (&str, u16, Security)| Server { host: host.to_string(), port, security };
    Some(Found {
        protocol: Protocol::Imap,
        incoming: server(*inc),
        outgoing: server(*out),
        user: email.trim().to_string(),
        from: Source::Exchanger,
        note: *note,
    })
}

/// An address's local part and domain, the domain in lower case; `None` for what is not one.
pub fn parts(email: &str) -> Option<(&str, String)> {
    let email = email.trim();
    let (local, domain) = email.rsplit_once('@')?;
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    (!local.is_empty() && local.len() <= 64 && is_host(&domain) && domain.contains('.')).then_some((local, domain))
}

/// A host name as DNS has them ([RFC 1123] 2.1): labels of letters, digits and hyphens.
pub fn is_host(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// What is known here of the address's provider.
pub fn known(email: &str) -> Option<Found> {
    let (_, domain) = parts(email)?;
    let (_, inc, out, note) = KNOWN.iter().find(|(domains, ..)| domains.contains(&domain.as_str()))?;
    let server = |(host, port, security): (&str, u16, Security)| Server { host: host.to_string(), port, security };
    Some(Found {
        protocol: Protocol::Imap,
        incoming: server(*inc),
        outgoing: server(*out),
        user: email.trim().to_string(),
        from: Source::Known,
        note: *note,
    })
}

/// The settings for `email` in a published settings file, IMAP before POP3, secure servers
/// only. `None` when it holds none that can be used.
pub fn published(xml: &[u8], email: &str) -> Option<Found> {
    let (local, domain) = parts(email)?;
    if xml.len() > MAX_SETTINGS {
        return None;
    }
    let text = std::str::from_utf8(xml).ok()?;
    if !shallow(text.as_bytes(), MAX_DEPTH) {
        return None;
    }
    let options = roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: MAX_NODES, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(text, options).ok()?;
    let fill = |s: &str| s.trim().replace("%EMAILADDRESS%", email.trim()).replace("%EMAILLOCALPART%", local).replace("%EMAILDOMAIN%", &domain);
    let read = |node: roxmltree::Node<'_, '_>| -> Option<(Server, String)> {
        let child = |name: &str| node.children().find(|c| c.has_tag_name(name)).and_then(|c| c.text()).map(fill);
        let host = child("hostname")?.to_ascii_lowercase();
        let port = child("port")?.parse::<u16>().ok().filter(|p| *p != 0)?;
        let security = match child("socketType")?.to_ascii_uppercase().as_str() {
            "SSL" | "TLS" => Security::Tls,
            "STARTTLS" => Security::StartTls,
            _ => return None,
        };
        let user = child("username")
            .filter(|u| !u.is_empty() && u.len() <= 256 && !u.chars().any(char::is_control))
            .unwrap_or_else(|| email.trim().to_string());
        is_host(&host).then_some((Server { host, port, security }, user))
    };
    let servers = |tag: &str, kind: &str| {
        doc.descendants()
            .filter(move |n| n.has_tag_name(tag) && n.attribute("type").is_some_and(|t| t.eq_ignore_ascii_case(kind)))
            .filter_map(read)
            .next()
    };
    let (protocol, (incoming, user)) = match servers("incomingServer", "imap") {
        Some(found) => (Protocol::Imap, found),
        None => (Protocol::Pop3, servers("incomingServer", "pop3")?),
    };
    let (outgoing, _) = servers("outgoingServer", "smtp")?;
    Some(Found { protocol, incoming, outgoing, user, from: Source::Published, note: None })
}

/// Whether XML nests elements no deeper than `most`. Comments, CDATA, processing
/// instructions and declarations are passed over whole, and quoted attribute values are read as
/// such, so nothing in them can hide an element; where it cannot tell, it counts one more.
pub(crate) fn shallow(xml: &[u8], most: usize) -> bool {
    let after = |at: usize, end: &[u8]| xml.get(at..).and_then(|rest| rest.windows(end.len()).position(|w| w == end)).map(|n| at + n + end.len());
    let (mut at, mut depth) = (0usize, 0usize);
    while let Some(n) = xml.get(at..).and_then(|rest| rest.iter().position(|b| *b == b'<')) {
        let open = at + n;
        let rest = xml.get(open..).unwrap_or_default();
        let skip = if rest.starts_with(b"<!--") {
            after(open + 4, b"-->")
        } else if rest.starts_with(b"<![CDATA[") {
            after(open + 9, b"]]>")
        } else if rest.starts_with(b"<?") {
            after(open + 2, b"?>")
        } else {
            None
        };
        if let Some(next) = skip {
            at = next;
            continue;
        }
        // A tag: to its `>`, quotes read as quotes.
        let (mut quote, mut close) = (None::<u8>, None::<usize>);
        for (i, b) in rest.iter().enumerate().skip(1) {
            match (quote, *b) {
                (Some(q), b) if b == q => quote = None,
                (Some(_), _) => {}
                (None, b'"' | b'\'') => quote = Some(*b),
                (None, b'>') => {
                    close = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(close) = close else { return depth < most };
        let tag = rest.get(..=close).unwrap_or_default();
        if tag.starts_with(b"</") {
            depth = depth.saturating_sub(1);
        } else if !tag.starts_with(b"<!") && !tag.ends_with(b"/>") {
            depth += 1;
            if depth > most {
                return false;
            }
        }
        at = open + close + 1;
    }
    true
}

/// Where a domain's settings may be published, in the order they are asked for.
pub fn settings_addresses(domain: &str) -> [String; 2] {
    [format!("https://autoconfig.{domain}/mail/config-v1.1.xml"), format!("https://{domain}/.well-known/autoconfig/mail/config-v1.1.xml")]
}

/// The usual servers of a domain, in the order they are tried: incoming (IMAP), then
/// outgoing.
pub fn usual(domain: &str) -> (Vec<Server>, Vec<Server>) {
    let at = |prefix: &str, port: u16, security: Security| Server { host: format!("{prefix}{domain}"), port, security };
    let incoming = vec![
        at("imap.", 993, Security::Tls),
        at("mail.", 993, Security::Tls),
        at("", 993, Security::Tls),
        at("imap.", 143, Security::StartTls),
        at("mail.", 143, Security::StartTls),
    ];
    let outgoing = vec![
        at("smtp.", 465, Security::Tls),
        at("smtp.", 587, Security::StartTls),
        at("mail.", 465, Security::Tls),
        at("mail.", 587, Security::StartTls),
        at("", 465, Security::Tls),
        at("", 587, Security::StartTls),
    ];
    (incoming, outgoing)
}

/// Whether a server greets us as its protocol does, made secure; nothing is signed in to.
pub fn greets(with: &dyn Connector, protocol: Protocol, server: &Server) -> Result<(), NetError> {
    let to = Endpoint { protocol, host: server.host.clone(), port: server.port, security: server.security, trusted: None };
    match protocol {
        Protocol::Imap => crate::imap::Imap::greets(with, &to),
        Protocol::Pop3 => crate::pop3::Pop3::greets(with, &to),
        Protocol::Smtp => crate::smtp::Smtp::open(with, &to, None)?.quit(),
        Protocol::Https => Err(NetError::Unsupported("not a mail server".into())),
    }
}

/// Find the settings for `email`. `stopped` is asked between one server and the next.
pub fn find(with: &dyn Connector, email: &str, stopped: &dyn Fn() -> bool) -> Result<Found, NetError> {
    let Some((_, domain)) = parts(email) else { return Err(NetError::Unsupported("that is not an email address".into())) };
    if let Some(found) = known(email) {
        return Ok(found);
    }
    for address in settings_addresses(&domain) {
        if stopped() {
            return Err(NetError::Unsupported("stopped".into()));
        }
        if let Ok(got) = http::get_settings(with, &address, MAX_SETTINGS)
            && let Some(found) = published(&got.bytes, email)
        {
            return Ok(found);
        }
    }
    if let Some(found) = by_records(with, email, &domain, stopped)? {
        return Ok(found);
    }
    if stopped() {
        return Err(NetError::Unsupported("stopped".into()));
    }
    if let Some(found) = with.lookup(&domain, Kind::Mx).ok().and_then(|records| exchanger(&records, email)) {
        return Ok(found);
    }
    let (incoming, outgoing) = usual(&domain);
    let first = |protocol: Protocol, servers: Vec<Server>| -> Result<Option<Server>, NetError> {
        for server in servers {
            if stopped() {
                return Err(NetError::Unsupported("stopped".into()));
            }
            if greets(with, protocol, &server).is_ok() {
                return Ok(Some(server));
            }
        }
        Ok(None)
    };
    let Some(incoming) = first(Protocol::Imap, incoming)? else {
        return Err(NetError::Unsupported(format!(
            "no settings were found for {domain}: it publishes none, and none of the usual servers answered securely"
        )));
    };
    let Some(outgoing) = first(Protocol::Smtp, outgoing)? else {
        return Err(NetError::Unsupported(format!("{} answered for incoming mail, but none of the usual servers for sending did", incoming.host)));
    };
    Ok(Found { protocol: Protocol::Imap, incoming, outgoing, user: email.trim().to_string(), from: Source::Tried, note: None })
}
