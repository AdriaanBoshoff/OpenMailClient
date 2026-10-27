//! A message on its way out, looked over before it goes: what its writer would want to know
//! while it can still be changed. Nothing here sends, resolves or looks an address up.

use std::collections::HashSet;

use crate::Draft;
use crate::attachments;
use crate::search::fold;

/// Characters of the new text that are read for a word about an attachment.
const MAX_READ: usize = 20_000;
/// Cautions about addresses that are told, and characters of an address that are said back.
const MAX_TOLD: usize = 8;
const MAX_SAID: usize = 80;
/// More than this in attachments and many mail servers refuse the message.
pub const HEAVY: usize = 20 * 1024 * 1024;
/// Cautions about single attachments that are told.
const MAX_FILES_TOLD: usize = 4;

/// The line a reply or a forward puts above the text it answers.
pub const ORIGINAL: &str = "-----Original Message-----";

/// Words that say something is attached.
const ATTACHING: &[&str] = &["attach", "attached", "attaching", "attachment", "attachments", "enclosed", "enclosing"];

/// Something about a message its writer should know before it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caution {
    NoSubject,
    /// The text speaks of an attachment and there is none.
    NothingAttached,
    /// Not an address mail can be sent to.
    NotAnAddress(String),
    /// The same address stands in more than one place.
    Twice(String),
    /// The writer was a hidden recipient of the message this answers, and answers everyone.
    WasHidden,
    /// The attachments come to so many bytes.
    Heavy(usize),
    /// An attachment that holds nothing.
    Empty(String),
    /// An attachment of a kind mail servers refuse to carry.
    Refused(String),
}

impl Caution {
    /// The caution in a sentence.
    pub fn words(&self) -> String {
        match self {
            Self::NoSubject => "The message has no subject.".to_string(),
            Self::NothingAttached => "The text speaks of an attachment, but nothing is attached.".to_string(),
            // A name alone may be one the address book has: Check Names (or Send) puts it in.
            Self::NotAnAddress(text) if !text.contains('@') => format!(
                "\u{201c}{text}\u{201d} is not an address mail can be sent to; Check Names finds it in the Address Book when it is there once."
            ),
            Self::NotAnAddress(text) => format!("\u{201c}{text}\u{201d} is not an address mail can be sent to."),
            Self::Twice(address) => format!("{address} is named more than once."),
            Self::Heavy(bytes) => {
                format!("The attachments come to {}; many mail servers refuse more than about 20 MB.", attachments::size_words(*bytes))
            }
            Self::Empty(name) => format!("\u{201c}{name}\u{201d} is empty."),
            Self::Refused(name) => format!("\u{201c}{name}\u{201d} is a program or a script; most mail servers refuse to carry one."),
            Self::WasHidden => {
                "You were a hidden (Bcc) recipient of the message this answers: a reply to all shows the others that you had it.".to_string()
            }
        }
    }
}

fn said(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(MAX_SAID).collect()
}

/// An address mail can be sent to ([RFC 5321] 4.1.2, loosely): one `@`, something before it, a
/// name with a dot after it, and no spaces.
pub fn sendable(address: &str) -> bool {
    let Some((local, domain)) = address.rsplit_once('@') else { return false };
    let label = |part: &str| !part.is_empty() && !part.starts_with('-') && !part.ends_with('-');
    !local.is_empty()
        && !local.contains('@')
        && address.chars().count() <= 254
        && !address.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | ',' | ';'))
        && domain.contains('.')
        && domain.split('.').all(label)
}

/// The text the writer wrote: what stands above the message being answered.
fn new_text(body: &str) -> &str {
    body.find(ORIGINAL).and_then(|at| body.get(..at)).unwrap_or(body)
}

/// What the writer of `draft` should know before it goes, most telling first.
pub fn cautions(draft: &Draft) -> Vec<Caution> {
    let mut found = Vec::new();
    if draft.was_hidden {
        found.push(Caution::WasHidden);
    }
    let (mut seen, mut twice) = (HashSet::new(), HashSet::new());
    let mut told = 0;
    for line in [&draft.to, &draft.cc, &draft.bcc] {
        for (name, address) in openmailclient_mime::parse_addresses(line) {
            if told >= MAX_TOLD {
                break;
            }
            let address = address.trim();
            if !sendable(address) {
                found.push(Caution::NotAnAddress(said(if address.is_empty() { name.trim() } else { address })));
                told += 1;
            } else if !seen.insert(address.to_lowercase()) && twice.insert(address.to_lowercase()) {
                found.push(Caution::Twice(said(address)));
                told += 1;
            }
        }
    }
    if draft.subject.trim().is_empty() {
        found.push(Caution::NoSubject);
    }
    let total = draft.attachments.iter().fold(0usize, |sum, a| sum.saturating_add(a.size()));
    if total > HEAVY {
        found.push(Caution::Heavy(total));
    }
    let files = draft.attachments.iter().filter_map(|a| {
        if a.embedded.is_none() && a.data.is_empty() {
            return Some(Caution::Empty(said(&attachments::safe_name(&a.name()))));
        }
        match attachments::caution_of(a) {
            Some(attachments::Caution::Runs { .. } | attachments::Caution::Program { .. }) => {
                Some(Caution::Refused(said(&attachments::safe_name(&a.name()))))
            }
            _ => None,
        }
    });
    found.extend(files.take(MAX_FILES_TOLD));
    if draft.attachments.is_empty() {
        let text: String = new_text(&draft.body).chars().take(MAX_READ).collect();
        if fold(&text).split(|c: char| !c.is_alphanumeric()).any(|word| ATTACHING.contains(&word)) {
            found.push(Caution::NothingAttached);
        }
    }
    found
}
