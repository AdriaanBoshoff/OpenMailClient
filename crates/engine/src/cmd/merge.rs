//! Mail Merge: one message for each contact, its fields put into the subject and the text, kept
//! in Drafts to be looked over or put in the Outbox to go at the next Send/Receive.
//!
//! A field is written `«First Name»`, as a merge field shows in a letter, or `{{First Name}}`,
//! as is easier to type; its name is matched without regard to case, spaces or underscores.
//!
//! Beyond Outlook, which merges through Word: the merge is looked at before anything is made
//! (`"deliver": "preview"`), a field name that is not one is refused before a message is made
//! instead of being left in hundreds of them, a contact without an address is named rather than
//! passed over in silence, a group is merged as its members and no one gets two.

use super::people::{card_of, people_folder};
use super::{CommandSpec, p_str};
use crate::{CmdResult, Draft, EngineError, Session};
use openmailclient_formats::vcf::Card;
use openmailclient_model::contact::{Address, Contact, Email, Phone, Place};
use openmailclient_model::{FolderKind, ItemId, StoreError};
use serde_json::{Value, json};
use std::collections::HashSet;

/// Messages one merge makes, and how long its subject and text may be.
const MAX_MESSAGES: usize = 5_000;
const MAX_SUBJECT: usize = 1_000;
const MAX_BODY: usize = 1024 * 1024;
/// Messages shown by a preview.
const PREVIEWS: usize = 3;
/// Fields one text may hold.
const MAX_FIELDS: usize = 1_000;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "people.mailMerge",
        label: "Mail Merge",
        tab: "Home",
        group: "Actions",
        shortcut: "",
        params: r#"{"subject", "body": text with fields written «Field» or {{Field}} (see "fields"), "contacts": "shown" (the default: those of the Contacts folder shown, as searched) | "selected" | [ids], "deliver": "drafts" (the default) | "outbox" | "preview": make nothing and show the first three, "account": the account they go from}; one message each, to the address a contact has first (a group: each member once); returns {"made", "to", "ids", "fields": the names a field may have, "skipped": [{"name", "why"}], "untrusted_content": {"previews": [{"to", "subject", "body"}]}}"#,
        enabled: has_contacts,
        run: mail_merge,
    }]
}

fn has_contacts(s: &Session) -> bool {
    people_folder(s).is_some()
}

/// The fields a merge knows, by the name they are written with.
const FIELDS: &[&str] = &[
    "Title",
    "First Name",
    "Middle Name",
    "Last Name",
    "Suffix",
    "Full Name",
    "Nickname",
    "File As",
    "Company",
    "Department",
    "Job Title",
    "Email",
    "Business Phone",
    "Home Phone",
    "Mobile Phone",
    "Business Address",
    "Home Address",
    "Mailing Address",
    "Street",
    "City",
    "State",
    "Postal Code",
    "Country",
    "Web Page",
];

/// A field's name as it is compared: lower case, without spaces, underscores or dashes.
fn key(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, ' ' | '_' | '-')).flat_map(char::to_lowercase).collect()
}

/// The field a written name stands for, with the other names it is known by.
fn field_of(name: &str) -> Option<&'static str> {
    let k = key(name);
    let alias = match k.as_str() {
        "prefix" => "Title",
        "given" | "givenname" | "firstname" | "first" => "First Name",
        "surname" | "lastname" | "familyname" | "last" => "Last Name",
        "name" | "displayname" => "Full Name",
        "emailaddress" | "address" | "email1" => "Email",
        "zip" | "zipcode" | "postcode" => "Postal Code",
        "organization" | "organisation" => "Company",
        "title2" | "jobtitle" => "Job Title",
        "mobile" | "cellphone" => "Mobile Phone",
        _ => "",
    };
    if !alias.is_empty() {
        return Some(alias);
    }
    FIELDS.iter().copied().find(|f| key(f) == k)
}

/// A piece of a text: words as they are, or a field.
#[derive(Debug, PartialEq, Eq)]
enum Piece {
    Words(String),
    Field(&'static str),
}

/// A text read into its words and fields; a field that is not one is an error that names it.
fn parse(text: &str) -> Result<Vec<Piece>, EngineError> {
    let mut out = Vec::new();
    let mut rest = text;
    let mut fields = 0usize;
    loop {
        let next =
            [("«", "»"), ("{{", "}}")].iter().filter_map(|(open, close)| rest.find(open).map(|at| (at, *open, *close))).min_by_key(|(at, _, _)| *at);
        let Some((at, open, close)) = next else {
            out.push(Piece::Words(rest.to_string()));
            return Ok(out);
        };
        let after = rest.get(at + open.len()..).unwrap_or("");
        let Some(end) = after.find(close) else {
            out.push(Piece::Words(rest.to_string()));
            return Ok(out);
        };
        let name = after.get(..end).unwrap_or("");
        out.push(Piece::Words(rest.get(..at).unwrap_or("").to_string()));
        let field = field_of(name).ok_or_else(|| {
            let shown: String = name.chars().take(60).collect();
            EngineError::BadParams(format!("{open}{shown}{close} is not a field; the fields are {}", FIELDS.join(", ")))
        })?;
        fields = fields.saturating_add(1);
        if fields > MAX_FIELDS {
            return Err(EngineError::BadParams(format!("a text may hold at most {MAX_FIELDS} fields")));
        }
        out.push(Piece::Field(field));
        rest = after.get(end + close.len()..).unwrap_or("");
    }
}

/// An address on one line.
fn one_line(a: &Address) -> String {
    let city =
        [a.city.as_str(), a.state.as_str(), a.postal_code.as_str()].iter().filter(|p| !p.trim().is_empty()).copied().collect::<Vec<_>>().join(" ");
    [a.street.replace(['\r', '\n'], ", ").as_str(), city.as_str(), a.country.as_str()]
        .iter()
        .filter(|p| !p.trim().is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(", ")
}

/// What `field` is for `c`, written to `email`.
fn value(c: &Contact, email: &str, field: &str) -> String {
    let mailing = c.mailing.and_then(|p| c.address(p)).or_else(|| [Place::Business, Place::Home, Place::Other].iter().find_map(|p| c.address(*p)));
    let part = |f: fn(&Address) -> &String| mailing.map(|a| f(a).clone()).unwrap_or_default();
    match field {
        "Title" => c.prefix.clone(),
        "First Name" => c.given.clone(),
        "Middle Name" => c.middle.clone(),
        "Last Name" => c.surname.clone(),
        "Suffix" => c.suffix.clone(),
        "Full Name" => c.display_name(),
        "Nickname" => c.nickname.clone(),
        "File As" => c.filed_as(),
        "Company" => c.company.clone(),
        "Department" => c.department.clone(),
        "Job Title" => c.job_title.clone(),
        "Email" => email.to_string(),
        "Business Phone" => c.phone(Phone::Business).unwrap_or_default().to_string(),
        "Home Phone" => c.phone(Phone::Home).unwrap_or_default().to_string(),
        "Mobile Phone" => c.phone(Phone::Mobile).unwrap_or_default().to_string(),
        "Business Address" => c.address(Place::Business).map(one_line).unwrap_or_default(),
        "Home Address" => c.address(Place::Home).map(one_line).unwrap_or_default(),
        "Mailing Address" => mailing.map(one_line).unwrap_or_default(),
        "Street" => part(|a| &a.street),
        "City" => part(|a| &a.city),
        "State" => part(|a| &a.state),
        "Postal Code" => part(|a| &a.postal_code),
        "Country" => part(|a| &a.country),
        "Web Page" => c.web_page.clone(),
        _ => String::new(),
    }
}

/// The text with each field filled in for `c`; on one line when `line`.
fn fill(pieces: &[Piece], c: &Contact, email: &str, line: bool, max: usize) -> String {
    let mut out = String::new();
    for p in pieces {
        match p {
            Piece::Words(w) => out.push_str(w),
            Piece::Field(f) => {
                let v = value(c, email, f);
                out.push_str(&if line { v.replace(['\r', '\n'], " ") } else { v });
            }
        }
        if out.len() > max {
            break;
        }
    }
    let mut end = out.len().min(max);
    while !out.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    out.truncate(end);
    out
}

/// Who a merge writes to: each contact with the address it has first, and those it cannot.
fn recipients(s: &mut Session, p: &Value) -> Result<(Vec<(Contact, String)>, Vec<Value>), EngineError> {
    let (store, folder) = people_folder(s).ok_or_else(|| EngineError::BadParams("no data file open has a Contacts folder".into()))?;
    let ids: Vec<u64> = match p.get("contacts") {
        None | Some(Value::Null) => {
            s.reload_people()?;
            s.people.items.iter().map(|(id, _)| *id).collect()
        }
        Some(Value::String(w)) if w == "shown" => {
            s.reload_people()?;
            s.people.items.iter().map(|(id, _)| *id).collect()
        }
        Some(Value::String(w)) if w == "selected" => s.people.selected.into_iter().collect(),
        Some(Value::Array(a)) => {
            let ids: Vec<u64> = a.iter().filter_map(Value::as_u64).take(MAX_MESSAGES.saturating_add(1)).collect();
            if ids.len() != a.len().min(MAX_MESSAGES.saturating_add(1)) {
                return Err(EngineError::BadParams("\"contacts\" are ids, as people.items gives them".into()));
            }
            ids
        }
        Some(_) => return Err(EngineError::BadParams("\"contacts\" is \"shown\", \"selected\" or a list of ids".into())),
    };
    if ids.is_empty() {
        return Err(EngineError::BadParams("there is no one to write to: no contact is shown or chosen".into()));
    }
    let mut cards = Vec::new();
    for id in ids {
        let item = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(folder, ItemId(id))?;
        let card = card_of(&item, s).ok_or_else(|| EngineError::BadParams(format!("{id} is not a contact or a group")))?;
        cards.push(card);
    }
    // Members of a group that are contacts here are merged with what their contact says.
    let known: Vec<Contact> = cards.iter().filter_map(|c| if let Card::Contact(c) = c { Some((**c).clone()) } else { None }).collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for card in cards {
        let people: Vec<(Contact, String)> = match card {
            Card::Contact(c) => match c.emails.iter().find(|e| e.address.contains('@')).map(|e| e.address.clone()) {
                Some(email) => vec![(*c, email)],
                None => {
                    skipped.push(json!({ "name": c.display_name(), "why": "has no e-mail address" }));
                    Vec::new()
                }
            },
            Card::Group(g) => {
                g.members
                    .iter()
                    .filter(|m| m.address.contains('@'))
                    .map(|m| {
                        let c = known.iter().find(|k| k.emails.iter().any(|e| e.address.eq_ignore_ascii_case(&m.address))).cloned().unwrap_or_else(
                            || Contact {
                                full_name: m.name.clone(),
                                emails: vec![Email { address: m.address.clone(), name: String::new() }],
                                ..Contact::default()
                            },
                        );
                        (c, m.address.clone())
                    })
                    .collect()
            }
        };
        for (c, email) in people {
            if seen.insert(email.to_lowercase()) {
                out.push((c, email));
            }
        }
    }
    if out.len() > MAX_MESSAGES {
        return Err(EngineError::BadParams(format!("a merge writes at most {MAX_MESSAGES} messages")));
    }
    Ok((out, skipped))
}

fn mail_merge(s: &mut Session, p: &Value) -> CmdResult {
    let subject = parse(p_str(p, "subject").unwrap_or(""))?;
    let body = parse(p_str(p, "body").ok_or_else(|| EngineError::BadParams("\"body\" (text) is required".into()))?)?;
    let deliver = match p_str(p, "deliver").unwrap_or("drafts") {
        "drafts" => FolderKind::Drafts,
        "outbox" => FolderKind::Outbox,
        "preview" => FolderKind::Other,
        _ => return Err(EngineError::BadParams("\"deliver\" is \"drafts\", \"outbox\" or \"preview\"".into())),
    };
    let account = match p_str(p, "account").map(str::trim).filter(|a| !a.is_empty()) {
        None => String::new(),
        Some(wanted) => s
            .accounts
            .iter()
            .find(|a| a.id == wanted || a.email.eq_ignore_ascii_case(wanted))
            .map(|a| a.id.clone())
            .ok_or_else(|| EngineError::BadParams("there is no such account".into()))?,
    };
    let (people, skipped) = recipients(s, p)?;
    let drafts: Vec<Draft> = people
        .iter()
        .map(|(c, email)| Draft {
            to: super::names::mailbox(&c.display_name(), email),
            subject: fill(&subject, c, email, true, MAX_SUBJECT),
            body: fill(&body, c, email, false, MAX_BODY),
            kind: "New",
            account: account.clone(),
            ..Draft::default()
        })
        .collect();
    let previews: Vec<Value> = drafts.iter().take(PREVIEWS).map(|d| json!({ "to": d.to, "subject": d.subject, "body": d.body })).collect();
    let to: Vec<&str> = drafts.iter().map(|d| d.to.as_str()).collect();
    let mut ids = Vec::new();
    if deliver != FolderKind::Other && !drafts.is_empty() {
        let store = match deliver {
            FolderKind::Drafts => super::compose::drafts_store(s, drafts.first().unwrap_or(&Draft::default()))?,
            _ => s.writable_store(),
        }
        .ok_or_else(|| EngineError::Format("there is no writable data file to keep the messages in".into()))?;
        let folder = s.find_folder(store, deliver).ok_or_else(|| EngineError::Format(format!("this data file has no {deliver:?} folder")))?;
        for d in &drafts {
            let item = super::compose::build(s, d, deliver == FolderKind::Outbox);
            ids.push(s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.add(folder, item)?.0);
        }
        s.refresh_folders(store);
        s.reload_rows()?;
        s.status = format!(
            "{} message{} {} {}",
            ids.len(),
            if ids.len() == 1 { "" } else { "s" },
            if ids.len() == 1 { "is" } else { "are" },
            if deliver == FolderKind::Drafts { "in Drafts, to be looked over and sent" } else { "in the Outbox; they go at the next Send/Receive" }
        );
    }
    Ok(json!({
        "made": ids.len(),
        "to": to,
        "ids": ids,
        "fields": FIELDS,
        "skipped": skipped,
        "untrusted_content": { "previews": previews },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_read_in_either_form_and_a_wrong_one_is_named() {
        let pieces = parse("Dear «First Name», {{ last_name }} {{Zip}}").unwrap();
        assert_eq!(
            pieces,
            vec![
                Piece::Words("Dear ".into()),
                Piece::Field("First Name"),
                Piece::Words(", ".into()),
                Piece::Field("Last Name"),
                Piece::Words(" ".into()),
                Piece::Field("Postal Code"),
                Piece::Words(String::new()),
            ]
        );
        let e = parse("Hi «Frist Name»").unwrap_err().to_string();
        assert!(e.contains("«Frist Name» is not a field"), "{e}");
        assert_eq!(parse("an «open one and {{ another").unwrap().len(), 1, "an unclosed one is words");
        assert!(parse(&"{{City}}".repeat(MAX_FIELDS + 1)).is_err());
        let c = Contact { given: "Ana".into(), ..Contact::default() };
        assert_eq!(fill(&parse("«First Name»é".repeat(10).as_str()).unwrap(), &c, "a@x", true, 7), "AnaéAn", "cut at a character");
    }
}
