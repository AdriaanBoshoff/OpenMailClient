//! What Outlook Classic has set up on this machine, read to be imported: its profiles, the
//! mail accounts in each with their servers, and the data files that hold each account's mail,
//! calendar, contacts and tasks, and the other data files the profile has open.
//!
//! Outlook keeps a profile in the registry, under
//! `Software\Microsoft\Office\<version>\Outlook\Profiles\<profile>`. That layout is not in a
//! public specification (Outlook 2010 and those before it kept them under
//! `Software\Microsoft\Windows NT\CurrentVersion\Windows Messaging Subsystem\Profiles`, read
//! too): what is read here was seen on a machine of our own (black-box, the
//! names of keys and values only) and is read as hostile input, as anything from outside is.
//! Each mail account is a key under the subkey `9375CFF0413111d3B88A00104B2A6676`, with values
//! named in words (`Email`, `IMAP Server`, `SMTP Port`…). Each data file is a section of the
//! profile whose values are named by the MAPI property they hold, as `001f6700` for
//! PidTagPstPath ([MS-OXPROPS]: tag 0x6700, type 0x001F, a UTF-16 string). An account's
//! `Service UID` names the section of its data file.
//!
//! Passwords are never read: Outlook keeps them sealed by the system for itself, and the owner
//! is asked for each. Nothing here writes to the registry or to Outlook's files.

use crate::accounts::Kind;
use std::path::PathBuf;

/// Profiles read at most.
pub const MAX_PROFILES: usize = 16;
/// Sections of one profile read at most.
pub const MAX_SECTIONS: usize = 512;
/// Accounts of one profile read at most.
pub const MAX_ACCOUNTS: usize = 64;
/// Values of one section read at most.
pub const MAX_VALUES: usize = 256;
/// The largest value read; anything larger is passed over.
pub const MAX_VALUE: usize = 64 * 1024;

/// The key under a profile that holds its mail accounts.
pub const ACCOUNTS_KEY: &str = "9375CFF0413111d3B88A00104B2A6676";

/// A value as the registry holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Raw {
    Number(u32),
    Text(String),
    Bytes(Vec<u8>),
}

/// A key of a profile and its values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub values: Vec<(String, Raw)>,
}

impl Section {
    fn get(&self, name: &str) -> Option<&Raw> {
        self.values.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v)
    }

    /// A value of words, kept as words or as UTF-16 bytes, as Outlook keeps it in either way.
    fn text(&self, name: &str) -> Option<String> {
        let t = match self.get(name)? {
            Raw::Text(t) => t.clone(),
            Raw::Bytes(b) => utf16(b),
            Raw::Number(_) => return None,
        };
        let t = t.trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string();
        (!t.is_empty() && t.chars().count() <= 1024 && !t.chars().any(char::is_control)).then_some(t)
    }

    /// A number, kept as a DWORD or as its four bytes.
    fn number(&self, name: &str) -> Option<u32> {
        match self.get(name)? {
            Raw::Number(n) => Some(*n),
            Raw::Bytes(b) => b.get(..4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes),
            Raw::Text(t) => t.trim().parse().ok(),
        }
    }

    fn bytes(&self, name: &str) -> Option<&[u8]> {
        match self.get(name)? {
            Raw::Bytes(b) => Some(b),
            _ => None,
        }
    }
}

/// UTF-16 little-endian bytes as words, up to the first NUL; an odd byte at the end is left out.
pub fn utf16(b: &[u8]) -> String {
    let units: Vec<u16> = b.chunks_exact(2).filter_map(|c| <[u8; 2]>::try_from(c).ok()).map(u16::from_le_bytes).take_while(|u| *u != 0).collect();
    String::from_utf16_lossy(&units)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A profile as the registry holds it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawProfile {
    pub name: String,
    /// The profile Outlook opens when it starts.
    pub default: bool,
    /// The profile's own keys, by name.
    pub sections: Vec<Section>,
    /// The keys under `ACCOUNTS_KEY`.
    pub accounts: Vec<Section>,
}

/// One server of an account as Outlook has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub host: String,
    pub port: Option<u16>,
    /// "tls" or "starttls": OpenMailClient signs in over nothing else.
    pub security: &'static str,
}

/// A mail account found in a profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub id: String,
    pub name: String,
    pub email: String,
    pub display_name: String,
    pub kind: Kind,
    pub user: String,
    pub incoming: Server,
    pub outgoing: Option<Server>,
    pub sign_in_to_send: bool,
    /// POP3: whether Outlook leaves a copy of each message on the server; `None` where it
    /// does not say.
    pub leave_on_server: Option<bool>,
    /// The signatures the account chose for new messages and for replies and forwards, by
    /// name ("New Signature" and "Reply-Forward Signature").
    pub new_signature: Option<String>,
    pub reply_signature: Option<String>,
    /// The data file Outlook keeps the account's mail, calendar, contacts and tasks in.
    pub data_file: Option<PathBuf>,
    /// What differs from what Outlook does, in words.
    pub notes: Vec<String>,
}

/// A data file of a profile that is no account's own: an archive, a file opened beside them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundFile {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
}

/// What a profile holds that is not imported, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub name: String,
    pub why: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub default: bool,
    pub accounts: Vec<Found>,
    pub files: Vec<FoundFile>,
    pub skipped: Vec<Skipped>,
}

/// A data file's path as a section names it: whole, ending in `.pst` or `.ost`.
fn data_path(section: &Section) -> Option<PathBuf> {
    // PidTagPstPath; an offline file's own path (0x6610) when it is one.
    ["001f6610", "001f6700", "001e6610", "001e6700"].iter().filter_map(|n| section.text(n)).map(PathBuf::from).find(|p| {
        p.is_absolute() && p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pst") || e.eq_ignore_ascii_case("ost"))
    })
}

fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
}

/// A server as an account's values name it, by the words that start their names ("IMAP",
/// "POP3", "SMTP").
fn server(a: &Section, of: &str, notes: &mut Vec<String>) -> Option<Server> {
    let host = a.text(&format!("{of} Server"))?.to_ascii_lowercase();
    let port = a.number(&format!("{of} Port")).and_then(|p| u16::try_from(p).ok()).filter(|p| *p > 0);
    // "Secure Connection" is 0 for none, 1 for SSL/TLS, 2 for STARTTLS and 3 for whichever the
    // server offers; "Use SSL" alone is 1 for SSL/TLS.
    let tls_port = matches!(port, Some(993 | 995 | 465));
    let security = match (a.number(&format!("{of} Secure Connection")), a.number(&format!("{of} Use SSL"))) {
        (Some(1), _) => "tls",
        (Some(2), _) => "starttls",
        (Some(3), _) | (None, Some(1)) if tls_port || port.is_none() => "tls",
        (Some(3), _) | (None, Some(1)) => "starttls",
        _ => {
            notes.push(format!("Outlook signs in to {host} without TLS; OpenMailClient asks it for STARTTLS"));
            "starttls"
        }
    };
    Some(Server { host, port, security })
}

/// Read what a profile holds. `at` numbers it among the profiles read, for the ids.
pub fn read(raw: &RawProfile, at: usize) -> Profile {
    let mut out = Profile { name: raw.name.chars().take(200).collect(), default: raw.default, ..Profile::default() };
    // The data files of the profile, each once.
    let mut files: Vec<(PathBuf, &Section)> = Vec::new();
    for s in raw.sections.iter().take(MAX_SECTIONS) {
        if let Some(p) = data_path(s)
            && !files.iter().any(|(q, _)| same_path(q, &p))
        {
            files.push((p, s));
        }
    }
    let mut owned: Vec<PathBuf> = Vec::new();
    for (n, a) in raw.accounts.iter().take(MAX_ACCOUNTS).enumerate() {
        let name_of = a.text("Account Name").unwrap_or_else(|| a.name.clone());
        let Some(email) = a.text("Email") else {
            // The address book and other services that are not mail accounts.
            if a.text("Account Name").is_some() && (a.text("IMAP Server").is_some() || a.text("POP3 Server").is_some()) {
                out.skipped.push(Skipped { name: name_of, why: "it has no address to send from".into() });
            }
            continue;
        };
        let mut notes = Vec::new();
        // "Leave on Server": its lowest bit is set where a copy is left.
        let leave_on_server = a.number("Leave on Server").map(|n| n & 1 != 0);
        let (kind, incoming) = match (server(a, "IMAP", &mut notes), server(a, "POP3", &mut notes)) {
            (Some(s), _) => (Kind::Imap, s),
            (None, Some(s)) => {
                notes.push(match leave_on_server {
                    Some(true) => "a copy of each message is left on the server, as Outlook leaves one".into(),
                    Some(false) => "each message is taken off the server, as Outlook takes it, and only once it is saved here".into(),
                    None => "a copy of each message is left on the server, as is safest; this can be changed in Account Settings".into(),
                });
                (Kind::Pop3, s)
            }
            (None, None) => {
                out.skipped.push(Skipped {
                    name: name_of,
                    why: "it is not an IMAP or POP3 account (an Exchange or Microsoft 365 account is signed in to as M8 comes)".into(),
                });
                continue;
            }
        };
        let user_key = if kind == Kind::Imap { "IMAP User" } else { "POP3 User" };
        // The section of its data file, named by its Service UID.
        let service = a.bytes("Service UID").filter(|b| b.len() == 16).map(hex);
        let data_file = service.and_then(|uid| {
            raw.sections.iter().take(MAX_SECTIONS).find(|s| s.name.eq_ignore_ascii_case(&uid)).and_then(data_path).or_else(|| {
                // Or a section of a provider of that service (PidTagServiceUid, 0x3D0C).
                raw.sections.iter().take(MAX_SECTIONS).filter(|s| s.bytes("01023d0c").is_some_and(|b| hex(b) == uid)).find_map(data_path)
            })
        });
        if let Some(p) = &data_file {
            owned.push(p.clone());
        }
        out.accounts.push(Found {
            id: format!("p{at}a{n}"),
            name: name_of,
            display_name: a.text("Display Name").unwrap_or_default(),
            user: a.text(user_key).unwrap_or_else(|| email.clone()),
            outgoing: server(a, "SMTP", &mut notes),
            sign_in_to_send: a.number("SMTP Use Auth").is_none_or(|n| n != 0),
            leave_on_server: leave_on_server.filter(|_| kind == Kind::Pop3),
            new_signature: a.text("New Signature").and_then(|n| crate::signatures::clean_name(&n)),
            reply_signature: a.text("Reply-Forward Signature").and_then(|n| crate::signatures::clean_name(&n)),
            email,
            kind,
            incoming,
            data_file,
            notes,
        });
    }
    for (n, (path, s)) in files.into_iter().enumerate() {
        if owned.iter().any(|o| same_path(o, &path)) {
            continue;
        }
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        // PidTagDisplayName of the data file, else its file's name.
        let name = s.text("001f3001").or_else(|| s.text("001e3001")).unwrap_or(stem);
        out.files.push(FoundFile { id: format!("p{at}f{n}"), name, path });
    }
    out
}

/// The profiles of Outlook Classic on this machine, read from the registry of the person
/// signed in; none where it is not installed. Read-only.
#[cfg(windows)]
pub fn system_profiles() -> Vec<RawProfile> {
    use windows_registry::{CURRENT_USER, Type};
    let mut out = Vec::new();
    // Outlook 2016, 2019, 2021 and Microsoft 365 keep version 16.0; 2013 kept 15.0; 2010 and
    // those before it kept profiles with the system's mail settings, which name the default.
    let places = [
        (r"Software\Microsoft\Office\16.0\Outlook\Profiles".to_string(), r"Software\Microsoft\Office\16.0\Outlook".to_string()),
        (r"Software\Microsoft\Office\15.0\Outlook\Profiles".to_string(), r"Software\Microsoft\Office\15.0\Outlook".to_string()),
        (
            r"Software\Microsoft\Windows NT\CurrentVersion\Windows Messaging Subsystem\Profiles".to_string(),
            r"Software\Microsoft\Windows NT\CurrentVersion\Windows Messaging Subsystem\Profiles".to_string(),
        ),
    ];
    for (place, named) in places {
        let Ok(profiles) = CURRENT_USER.open(&place) else { continue };
        let default = CURRENT_USER.open(&named).and_then(|k| k.get_string("DefaultProfile")).unwrap_or_default();
        let read_section = |key: &windows_registry::Key, name: String| -> Section {
            let mut values = Vec::new();
            if let Ok(all) = key.values() {
                for (n, v) in all.take(MAX_VALUES) {
                    if v.len() > MAX_VALUE {
                        continue;
                    }
                    let raw = match v.ty() {
                        Type::U32 => v.get(..4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes).map(Raw::Number),
                        Type::String | Type::ExpandString => Some(Raw::Text(utf16(&v))),
                        Type::Bytes => Some(Raw::Bytes(v.to_vec())),
                        _ => None,
                    };
                    if let Some(raw) = raw {
                        values.push((n, raw));
                    }
                }
            }
            Section { name, values }
        };
        let names: Vec<String> = profiles.keys().map(|k| k.take(MAX_PROFILES).collect()).unwrap_or_default();
        for name in names {
            if out.len() >= MAX_PROFILES || out.iter().any(|p: &RawProfile| p.name.eq_ignore_ascii_case(&name)) {
                continue;
            }
            let Ok(profile) = profiles.open(&name) else { continue };
            let mut raw = RawProfile { default: name.eq_ignore_ascii_case(&default), name: name.clone(), ..RawProfile::default() };
            let keys: Vec<String> = profile.keys().map(|k| k.take(MAX_SECTIONS).collect()).unwrap_or_default();
            for key in keys {
                let Ok(k) = profile.open(&key) else { continue };
                if key.eq_ignore_ascii_case(ACCOUNTS_KEY) {
                    let accounts: Vec<String> = k.keys().map(|a| a.take(MAX_ACCOUNTS).collect()).unwrap_or_default();
                    for a in accounts {
                        if let Ok(ak) = k.open(&a) {
                            raw.accounts.push(read_section(&ak, a));
                        }
                    }
                } else {
                    raw.sections.push(read_section(&k, key));
                }
            }
            out.push(raw);
        }
    }
    out
}

/// Outlook Classic runs only on Windows: elsewhere there is nothing of it to read.
#[cfg(not(windows))]
pub fn system_profiles() -> Vec<RawProfile> {
    Vec::new()
}
