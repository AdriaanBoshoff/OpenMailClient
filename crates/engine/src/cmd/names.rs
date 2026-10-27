//! The Address Book and Check Names: the people of every Contacts folder open, looked through
//! by name or address, and the names typed in To, Cc and Bcc made into addresses from them.
//!
//! A piece of an address line ([RFC 5322] 3.4) that is an address stays as typed (given its
//! contact's name when it has none); a name is matched against the address book: one match is
//! put in its place, several are offered, none is left as typed. A group is put in as its
//! members, so the writer sees who the message goes to.

use super::people::card_of;
use super::{CommandSpec, always, p_bool, p_str, p_u64};
use crate::{CmdResult, EngineError, Session};
use openmailclient_formats::vcf::Card;
use openmailclient_model::{FolderKind, ItemId};
use serde_json::{Value, json};

/// Items of one folder looked at, entries kept, members a group is put in as, choices offered.
const MAX_ITEMS: usize = 20_000;
const MAX_ENTRIES: usize = 50_000;
const MAX_MEMBERS: usize = 1_000;
const MAX_CHOICES: usize = 50;
/// What is typed in an address line is looked at in at most so many pieces, each so long.
const MAX_PIECES: usize = 500;
const MAX_TEXT: usize = 512;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "people.addressBook",
            label: "Address Book",
            tab: "Home",
            group: "Find",
            shortcut: "Ctrl+Shift+B",
            params: r#"{"query": words each of which begins a word of the name or the address (default: all), "store" and "folder": one Contacts folder alone (default: every one open), "for": "to" | "cc" | "bcc": the message being written's line a choice goes to, "open": false to look without opening the window, "limit": at most so many (default 500, at most 5000)}; returns {"books": [{"store", "folder", "name", "file", "entries"}], "total", "untrusted_content": {"entries": [{"name", "address", "kind": "contact" | "group", "store", "folder", "id", "company", "members": a group's, as addresses}]}}"#,
            enabled: always,
            run: address_book,
        },
        CommandSpec {
            id: "people.suggest",
            label: "Suggested Names",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"text": an address line as typed; what follows its last comma or semicolon is looked for, "limit" (default 10, at most 50)}; returns {"for": what was looked for, "untrusted_content": {"suggestions": [{"name", "address", "kind", "as": what goes in the line}]}}, those whose name begins with it first"#,
            enabled: always,
            run: suggest,
        },
        CommandSpec {
            id: "compose.checkNames",
            label: "Check Names",
            tab: "Message",
            group: "Names",
            shortcut: "Ctrl+K",
            params: r#"{"field": "to" | "cc" | "bcc" (default: all three), "apply": false to look without changing the lines}; each name in them that one entry of the address book has is put in as that entry's address, a group as its members; returns {"resolved", "ambiguous", "unknown", "untrusted_content": {"to" | "cc" | "bcc": [{"typed", "state": "address" | "resolved" | "ambiguous" | "unknown", "as": what stands in the line, "choices": [{"name", "address", "as"}]}]}}"#,
            enabled: composing,
            run: check_names,
        },
    ]
}

fn composing(s: &Session) -> bool {
    s.compose.is_some()
}

/// One way to write to someone the address book has: a contact's address, or a group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub name: String,
    /// Empty for a group.
    pub address: String,
    pub company: String,
    pub store: usize,
    pub folder: u64,
    pub id: u64,
    /// A group's members, as mailboxes; empty for a contact.
    pub members: Vec<String>,
}

impl Entry {
    fn is_group(&self) -> bool {
        self.address.is_empty()
    }

    /// What it is put in an address line as.
    pub(crate) fn mailbox(&self) -> String {
        if self.is_group() { self.members.join("; ") } else { mailbox(&self.name, &self.address) }
    }

    fn json(&self) -> Value {
        json!({
            "name": self.name,
            "address": self.address,
            "kind": if self.is_group() { "group" } else { "contact" },
            "store": self.store,
            "folder": self.folder,
            "id": self.id,
            "company": self.company,
            "members": self.members,
        })
    }

    /// How well `text` (lower case, trimmed) finds it: 0 not at all, 1 a word of the name or
    /// the address begins with each word of it, 2 the name begins with it, 3 it is the name or
    /// the address.
    fn score(&self, text: &str) -> u8 {
        let name = self.name.to_lowercase();
        let address = self.address.to_lowercase();
        // Of an address, the part before the @ alone: every address of a domain ends alike.
        let local = address.split('@').next().unwrap_or("");
        if text.is_empty() {
            return 1;
        }
        if name == text || (!address.is_empty() && address == text) {
            return 3;
        }
        if name.starts_with(text) || (!address.is_empty() && address.starts_with(text)) {
            return 2;
        }
        let words: Vec<&str> =
            name.split(|c: char| !c.is_alphanumeric()).chain(local.split(|c: char| !c.is_alphanumeric())).filter(|w| !w.is_empty()).collect();
        let company = self.company.to_lowercase();
        let all = text.split_whitespace().all(|t| {
            let t = t.trim_matches(|c: char| !c.is_alphanumeric());
            t.is_empty() || words.iter().any(|w| w.starts_with(t)) || company.split_whitespace().any(|w| w.starts_with(t))
        });
        u8::from(all)
    }
}

/// `name <address>`, quoted where the name needs it ([RFC 5322] 3.4, 3.2.4).
pub(crate) fn mailbox(name: &str, address: &str) -> String {
    let name = name.trim().replace(['"', '\\', '\r', '\n'], "");
    if name.is_empty() || name.eq_ignore_ascii_case(address) {
        address.to_string()
    } else if name.chars().any(|c| ",;<>@()[]:.".contains(c)) {
        format!("\"{name}\" <{address}>")
    } else {
        format!("{name} <{address}>")
    }
}

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

impl Session {
    /// Every entry of the Contacts folders open (or of one), in the order they are filed.
    pub(crate) fn address_entries(&mut self, only: Option<(usize, u64)>) -> Vec<Entry> {
        let mut folders = Vec::new();
        for (store, list) in self.folders.iter().enumerate() {
            for f in list.iter().filter(|f| f.kind == FolderKind::Contacts) {
                if only.is_none_or(|o| o == (store, f.id.0)) {
                    folders.push((store, f.id));
                }
            }
        }
        let mut out = Vec::new();
        for (store, folder) in folders {
            let Some(rows) = self.stores.get_mut(store).and_then(|st| st.list(folder).ok()) else { continue };
            for row in rows.into_iter().take(MAX_ITEMS) {
                if out.len() >= MAX_ENTRIES {
                    return out;
                }
                let Some(item) = self.stores.get_mut(store).and_then(|st| st.load(folder, ItemId(row.id)).ok()) else { continue };
                let Some(card) = card_of(&item, self) else { continue };
                match card {
                    Card::Contact(c) => {
                        let name = c.display_name();
                        for e in c.emails.iter().filter(|e| e.address.contains('@')) {
                            let shown = if e.name.trim().is_empty() { name.clone() } else { e.name.clone() };
                            out.push(Entry {
                                name: cut(&shown, MAX_TEXT),
                                address: cut(&e.address, MAX_TEXT),
                                company: cut(&c.company, MAX_TEXT),
                                store,
                                folder: folder.0,
                                id: row.id,
                                members: Vec::new(),
                            });
                        }
                    }
                    Card::Group(g) => {
                        let members: Vec<String> = g
                            .members
                            .iter()
                            .filter(|m| m.address.contains('@'))
                            .take(MAX_MEMBERS)
                            .map(|m| mailbox(&cut(&m.name, MAX_TEXT), &cut(&m.address, MAX_TEXT)))
                            .collect();
                        if !members.is_empty() {
                            out.push(Entry {
                                name: cut(&g.name, MAX_TEXT),
                                address: String::new(),
                                company: String::new(),
                                store,
                                folder: folder.0,
                                id: row.id,
                                members,
                            });
                        }
                    }
                }
            }
        }
        out.sort_by_cached_key(|e| (e.name.to_lowercase(), e.address.to_lowercase(), e.store, e.id));
        out
    }
}

/// The entries `text` finds, best first.
fn found<'a>(entries: &'a [Entry], text: &str) -> Vec<(u8, &'a Entry)> {
    let text = text.trim().to_lowercase();
    let mut out: Vec<(u8, &Entry)> = entries.iter().map(|e| (e.score(&text), e)).filter(|(n, _)| *n > 0).collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.0));
    out
}

fn limit(p: &Value, default: usize, most: usize) -> usize {
    p_u64(p, "limit").and_then(|n| usize::try_from(n).ok()).unwrap_or(default).clamp(1, most)
}

fn address_book(s: &mut Session, p: &Value) -> CmdResult {
    let only = match (p_u64(p, "store"), p_u64(p, "folder")) {
        (Some(store), Some(folder)) => {
            let store = usize::try_from(store).map_err(|_| EngineError::BadParams("\"store\" is a data file's number".into()))?;
            let known = s.folders.get(store).is_some_and(|l| l.iter().any(|f| f.id.0 == folder && f.kind == FolderKind::Contacts));
            if !known {
                return Err(EngineError::BadParams("that is not a Contacts folder of a data file open".into()));
            }
            Some((store, folder))
        }
        (None, None) => None,
        _ => return Err(EngineError::BadParams("\"store\" and \"folder\" are given together, as people.folders gives them".into())),
    };
    let target = match p_str(p, "for").map(str::to_ascii_lowercase).as_deref() {
        None | Some("") => String::new(),
        Some(f @ ("to" | "cc" | "bcc")) => f.to_string(),
        Some(_) => return Err(EngineError::BadParams("\"for\" is \"to\", \"cc\" or \"bcc\"".into())),
    };
    let query = cut(p_str(p, "query").unwrap_or(""), MAX_TEXT);
    let entries = s.address_entries(only);
    let hits = found(&entries, &query);
    let mut books = Vec::new();
    for (store, list) in s.folders.iter().enumerate() {
        let file = s.stores.get(store).map(|st| st.display_name()).unwrap_or_default();
        for f in list.iter().filter(|f| f.kind == FolderKind::Contacts && only.is_none_or(|o| o == (store, f.id.0))) {
            let n = entries.iter().filter(|e| e.store == store && e.folder == f.id.0).count();
            books.push(json!({ "store": store, "folder": f.id.0, "name": f.name, "file": file, "entries": n }));
        }
    }
    // Found by the query, they keep the order they are filed in.
    let mut shown: Vec<&Entry> = hits.iter().map(|(_, e)| *e).collect();
    shown.sort_by_cached_key(|e| (e.name.to_lowercase(), e.address.to_lowercase()));
    let total = shown.len();
    shown.truncate(limit(p, 500, 5_000));
    if p_bool(p, "open") != Some(false) {
        s.people.book = Some(target);
    }
    Ok(json!({
        "books": books,
        "total": total,
        "untrusted_content": { "entries": shown.iter().map(|e| e.json()).collect::<Vec<_>>() },
    }))
}

/// What follows the last comma or semicolon of an address line, outside quotes and angles.
fn last_piece(text: &str) -> &str {
    let (mut quoted, mut angle, mut at) = (false, 0usize, 0usize);
    for (i, c) in text.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '<' if !quoted => angle = angle.saturating_add(1),
            '>' if !quoted => angle = angle.saturating_sub(1),
            ',' | ';' if !quoted && angle == 0 => at = i + c.len_utf8(),
            _ => {}
        }
    }
    text.get(at..).unwrap_or("").trim()
}

fn suggest(s: &mut Session, p: &Value) -> CmdResult {
    let text = p_str(p, "text").ok_or_else(|| EngineError::BadParams("\"text\" (text) is required".into()))?;
    let text: String = text.chars().rev().take(4 * MAX_TEXT).collect::<Vec<_>>().into_iter().rev().collect();
    let piece = last_piece(&text).to_string();
    // A piece that is already an address, or nothing yet, is not looked for.
    if piece.is_empty() || piece.contains('<') {
        return Ok(json!({ "for": piece, "untrusted_content": { "suggestions": [] } }));
    }
    let entries = s.address_entries(None);
    let shown: Vec<Value> = found(&entries, &piece)
        .into_iter()
        .take(limit(p, 10, 50))
        .map(|(_, e)| json!({ "name": e.name, "address": e.address, "kind": if e.is_group() { "group" } else { "contact" }, "as": e.mailbox() }))
        .collect();
    Ok(json!({ "for": piece, "untrusted_content": { "suggestions": shown } }))
}

/// What became of one piece of an address line.
struct Checked {
    typed: String,
    state: &'static str,
    /// What stands in the line for it.
    now: String,
    choices: Vec<Entry>,
}

/// Each piece of an address line, as Check Names makes it.
fn check_line(line: &str, entries: &[Entry]) -> Vec<Checked> {
    let mut out = Vec::new();
    for piece in split_line(line).into_iter().take(MAX_PIECES) {
        let addresses = openmailclient_mime::parse_addresses(&piece);
        let named = addresses.first().map(|(n, a)| (n.clone(), a.clone())).unwrap_or_default();
        if addresses.len() == 1 && named.1.contains('@') {
            // An address: given its contact's name when it has none.
            let now = if named.0.is_empty() {
                entries
                    .iter()
                    .find(|e| !e.is_group() && e.address.eq_ignore_ascii_case(&named.1))
                    .map_or_else(|| piece.clone(), |e| mailbox(&e.name, &named.1))
            } else {
                piece.clone()
            };
            out.push(Checked { typed: piece, state: "address", now, choices: Vec::new() });
            continue;
        }
        let hits = found(entries, &piece);
        // The one that is exactly what was typed is taken over others that begin with it.
        let best = hits.first().map_or(0, |(n, _)| *n);
        let top: Vec<Entry> = hits.iter().filter(|(n, _)| *n == best).map(|(_, e)| (*e).clone()).collect();
        match top.as_slice() {
            [] => out.push(Checked { now: piece.clone(), typed: piece, state: "unknown", choices: Vec::new() }),
            [one] => out.push(Checked { now: one.mailbox(), typed: piece, state: "resolved", choices: Vec::new() }),
            _ => {
                let choices = hits.iter().take(MAX_CHOICES).map(|(_, e)| (*e).clone()).collect();
                out.push(Checked { now: piece.clone(), typed: piece, state: "ambiguous", choices });
            }
        }
    }
    out
}

/// The pieces of an address line, split at commas and semicolons outside quotes and angles.
fn split_line(line: &str) -> Vec<String> {
    let (mut quoted, mut angle) = (false, 0usize);
    let mut pieces = Vec::new();
    let mut now = String::new();
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            '<' if !quoted => angle = angle.saturating_add(1),
            '>' if !quoted => angle = angle.saturating_sub(1),
            ',' | ';' if !quoted && angle == 0 => {
                pieces.push(std::mem::take(&mut now));
                continue;
            }
            _ => {}
        }
        now.push(c);
    }
    pieces.push(now);
    pieces.into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

/// The names of the message being written checked, and put in as addresses when `apply`;
/// how many were resolved, ambiguous and unknown, and what came of each, line by line.
pub(crate) fn resolve_draft(s: &mut Session, fields: &[&'static str], apply: bool) -> Result<(usize, usize, usize, Value), EngineError> {
    let lines: Vec<(&'static str, String)> = {
        let d = s.compose.as_ref().ok_or_else(|| EngineError::Disabled("Check Names".into()))?;
        fields
            .iter()
            .map(|f| {
                (
                    *f,
                    match *f {
                        "cc" => d.cc.clone(),
                        "bcc" => d.bcc.clone(),
                        _ => d.to.clone(),
                    },
                )
            })
            .collect()
    };
    // The address book is read only when some piece is a name or lacks one.
    let entries = if lines.iter().any(|(_, l)| !l.trim().is_empty()) { s.address_entries(None) } else { Vec::new() };
    let (mut resolved, mut ambiguous, mut unknown) = (0usize, 0usize, 0usize);
    let mut shown = serde_json::Map::new();
    for (field, line) in lines {
        let checked = check_line(&line, &entries);
        for c in &checked {
            match c.state {
                "resolved" => resolved = resolved.saturating_add(1),
                "ambiguous" => ambiguous = ambiguous.saturating_add(1),
                "unknown" => unknown = unknown.saturating_add(1),
                _ => {}
            }
        }
        let now = checked.iter().map(|c| c.now.as_str()).collect::<Vec<_>>().join("; ");
        shown.insert(
            field.into(),
            json!(
                checked
                    .iter()
                    .map(|c| json!({
                        "typed": c.typed,
                        "state": c.state,
                        "as": c.now,
                        "choices": c.choices.iter().map(|e| json!({ "name": e.name, "address": e.address, "as": e.mailbox() })).collect::<Vec<_>>(),
                    }))
                    .collect::<Vec<_>>()
            ),
        );
        if apply
            && now != line.trim()
            && !checked.is_empty()
            && let Some(d) = s.compose.as_mut()
        {
            let now = cut(&now, 32 * 1024);
            match field {
                "cc" => d.cc = now,
                "bcc" => d.bcc = now,
                _ => d.to = now,
            }
            d.touched = true;
        }
    }
    Ok((resolved, ambiguous, unknown, Value::Object(shown)))
}

fn check_names(s: &mut Session, p: &Value) -> CmdResult {
    let fields: &[&'static str] = match p_str(p, "field").map(str::to_ascii_lowercase).as_deref() {
        None | Some("") => &["to", "cc", "bcc"],
        Some("to") => &["to"],
        Some("cc") => &["cc"],
        Some("bcc") => &["bcc"],
        Some(_) => return Err(EngineError::BadParams("\"field\" is \"to\", \"cc\" or \"bcc\"".into())),
    };
    let (resolved, ambiguous, unknown, shown) = resolve_draft(s, fields, p_bool(p, "apply") != Some(false))?;
    s.status = match (resolved, ambiguous, unknown) {
        (0, 0, 0) => "Every name is an address".to_string(),
        (r, 0, 0) => format!("{r} name{} found in the address book", if r == 1 { "" } else { "s" }),
        (_, a, u) => format!(
            "{}{}",
            if a > 0 {
                format!("{a} name{} more than one person in the address book. ", if a == 1 { " matches" } else { "s match" })
            } else {
                String::new()
            },
            if u > 0 { format!("{u} name{} not in the address book.", if u == 1 { " is" } else { "s are" }) } else { String::new() }
        )
        .trim()
        .to_string(),
    };
    Ok(json!({ "resolved": resolved, "ambiguous": ambiguous, "unknown": unknown, "untrusted_content": shown }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_split_outside_quotes_and_angles() {
        assert_eq!(split_line("\"Silva, Ana\" <a@x.test>; Ben,, ;"), vec!["\"Silva, Ana\" <a@x.test>", "Ben"]);
        assert_eq!(last_piece("Ana <a@x.test>, be"), "be");
        assert_eq!(last_piece("\"a, b"), "\"a, b");
        assert_eq!(split_line(&"x;".repeat(10_000)).len(), 10_000);
    }

    #[test]
    fn a_name_scores_by_how_it_is_found() {
        let e = Entry {
            name: "Ana Silva".into(),
            address: "ana@example.org".into(),
            company: "Fabrikam".into(),
            store: 0,
            folder: 1,
            id: 2,
            members: Vec::new(),
        };
        assert_eq!(e.score("ana silva"), 3);
        assert_eq!(e.score("ana@example.org"), 3);
        assert_eq!(e.score("an"), 2);
        assert_eq!(e.score("silva"), 1);
        assert_eq!(e.score("sil an"), 1);
        assert_eq!(e.score("fab"), 1);
        assert_eq!(e.score("bob"), 0);
        assert_eq!(e.score("org"), 0, "not by its domain");
        assert_eq!(mailbox("Silva, Ana", "a@x"), "\"Silva, Ana\" <a@x>");
    }
}
