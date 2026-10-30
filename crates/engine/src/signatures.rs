//! Signatures: text of the owner's own put at the foot of what they write, one chosen for new
//! messages and one for replies and forwards, for each account or for all of them, as Outlook's
//! Signatures and Stationery has it. Better than Outlook's, one signature serves several
//! accounts: `{name}` and `{email}` in it are filled with those of the account the message goes
//! from.
//!
//! Signatures are kept in the settings. Outlook keeps each as files of one name in
//! `%APPDATA%\Microsoft\Signatures` (`.htm`, `.rtf` and `.txt`), and the choices of each
//! account as its "New Signature" and "Reply-Forward Signature" values; those are read by
//! [`read_outlook`] to be imported.

use serde_json::{Value, json};
use std::path::Path;

/// Signatures kept at most.
pub const MAX_SIGNATURES: usize = 64;
/// The longest name of one, in characters.
pub const MAX_NAME: usize = 100;
/// The longest text of one, in bytes.
pub const MAX_TEXT: usize = 32 * 1024;
/// Accounts whose choices are kept at most.
const MAX_CHOICES: usize = 64;
/// A file of Outlook's larger than this is not a signature.
const MAX_FILE: u64 = 1 << 20;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Signature {
    pub name: String,
    pub text: String,
}

/// The signatures chosen for new messages and for replies and forwards, by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    pub new: Option<String>,
    pub replies: Option<String>,
}

impl Choice {
    fn to_json(&self) -> Value {
        json!({ "new": self.new, "replies": self.replies })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Signatures {
    pub list: Vec<Signature>,
    /// What is chosen for mail from an account with no choice of its own, and where there are
    /// no accounts.
    pub default: Choice,
    /// What each account chose for itself, by its id, "none" included; one not here takes the
    /// default.
    pub by_account: Vec<(String, Choice)>,
}

/// A name as it is kept: trimmed, of one line, not empty and not too long.
pub fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= MAX_NAME && !name.chars().any(char::is_control)).then(|| name.to_string())
}

/// Text as it is kept: lines ended by `\n` alone, no trailing space or blank lines, no other
/// control characters but tabs, cut at `MAX_TEXT`.
pub fn clean_text(text: &str) -> String {
    let lines: Vec<String> = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|l| l.chars().filter(|c| *c == '\t' || !c.is_control()).collect::<String>().trim_end().to_string())
        .collect();
    let mut out = lines.join("\n").trim_matches('\n').to_string();
    if out.len() > MAX_TEXT {
        let mut end = MAX_TEXT;
        while !out.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        out.truncate(end);
    }
    out
}

impl Signatures {
    pub fn get(&self, name: &str) -> Option<&Signature> {
        self.list.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// What applies to mail from the account `id` (empty where there are no accounts).
    pub fn choice_for(&self, id: &str) -> &Choice {
        self.by_account.iter().find(|(a, _)| a == id).map_or(&self.default, |(_, c)| c)
    }

    /// Set what an account chooses for itself (`""` for the default); `None` to have it take
    /// the default.
    pub fn choose(&mut self, id: &str, choice: Option<Choice>) {
        if id.is_empty() {
            self.default = choice.unwrap_or_default();
            return;
        }
        let room = self.by_account.len() < MAX_CHOICES;
        match (self.by_account.iter_mut().find(|(a, _)| a == id), choice) {
            (Some((_, had)), Some(c)) => *had = c,
            (Some(_), None) => self.by_account.retain(|(a, _)| a != id),
            (None, Some(c)) if room => self.by_account.push((id.to_string(), c)),
            (None, _) => {}
        }
    }

    /// Put a signature in, taking the place of one of that name. Says whether there was room.
    pub fn put(&mut self, sig: Signature) -> bool {
        if let Some(had) = self.list.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&sig.name)) {
            *had = sig;
            return true;
        }
        if self.list.len() >= MAX_SIGNATURES {
            return false;
        }
        self.list.push(sig);
        true
    }

    /// Rename one, and every choice of it with it.
    pub fn rename(&mut self, from: &str, to: &str) {
        let same = |n: &Option<String>| n.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(from));
        for c in std::iter::once(&mut self.default).chain(self.by_account.iter_mut().map(|(_, c)| c)) {
            if same(&c.new) {
                c.new = Some(to.to_string());
            }
            if same(&c.replies) {
                c.replies = Some(to.to_string());
            }
        }
        if let Some(s) = self.list.iter_mut().find(|s| s.name.eq_ignore_ascii_case(from)) {
            s.name = to.to_string();
        }
    }

    /// Take one away, and every choice of it with it. Says whether there was one.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.list.len();
        self.list.retain(|s| !s.name.eq_ignore_ascii_case(name));
        let same = |n: &Option<String>| n.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name));
        for c in std::iter::once(&mut self.default).chain(self.by_account.iter_mut().map(|(_, c)| c)) {
            if same(&c.new) {
                c.new = None;
            }
            if same(&c.replies) {
                c.replies = None;
            }
        }
        self.list.len() != before
    }

    /// A name no signature has yet: `name`, else "name (2)"…
    pub fn free_name(&self, name: &str) -> Option<String> {
        (1..=MAX_SIGNATURES + 1).map(|n| if n == 1 { name.to_string() } else { format!("{name} ({n})") }).find(|n| self.get(n).is_none())
    }

    /// As the settings keep them.
    pub fn to_json(&self) -> Value {
        json!({
            "list": self.list.iter().map(|s| json!({ "name": s.name, "text": s.text })).collect::<Vec<_>>(),
            "default": self.default.to_json(),
            "accounts": self.by_account.iter().map(|(a, c)| json!({ "account": a, "new": c.new, "replies": c.replies })).collect::<Vec<_>>(),
        })
    }

    /// As the settings keep them, and how many parts made no sense and were left out; `None`
    /// where it is not an object.
    pub fn of_json(v: &Value) -> Option<(Signatures, u32)> {
        let o = v.as_object()?;
        let mut out = Signatures::default();
        let mut ignored = 0u32;
        for s in o.get("list").and_then(Value::as_array).into_iter().flatten() {
            let name = s.get("name").and_then(Value::as_str).and_then(clean_name);
            let text = s.get("text").and_then(Value::as_str).map(clean_text);
            match (name, text) {
                (Some(name), Some(text)) if out.get(&name).is_none() => {
                    if !out.put(Signature { name, text }) {
                        ignored += 1;
                    }
                }
                _ => ignored += 1,
            }
        }
        // A choice of a signature that is not kept is no choice.
        let named = |out: &Signatures, v: Option<&Value>| v.and_then(Value::as_str).and_then(|n| out.get(n)).map(|s| s.name.clone());
        let choice = |out: &Signatures, v: &Value| Choice { new: named(out, v.get("new")), replies: named(out, v.get("replies")) };
        if let Some(d) = o.get("default") {
            out.default = choice(&out, d);
        }
        for a in o.get("accounts").and_then(Value::as_array).into_iter().flatten() {
            match a.get("account").and_then(Value::as_str).filter(|id| !id.is_empty() && id.len() <= 64) {
                Some(id) => {
                    let c = choice(&out, a);
                    out.choose(id, Some(c));
                }
                None => ignored += 1,
            }
        }
        Some((out, ignored))
    }
}

/// A signature's text for mail from `name` at `email`: `{name}` and `{email}` filled.
pub fn fill(text: &str, name: &str, email: &str) -> String {
    text.replace("{name}", name).replace("{email}", email)
}

/// The text of a file Outlook wrote: UTF-16 with its byte-order mark, as Outlook writes them,
/// UTF-8, or else bytes of the Windows Latin page.
pub fn decode(bytes: &[u8]) -> String {
    let words = |b: &[u8], big: bool| {
        let units: Vec<u16> = b
            .chunks_exact(2)
            .filter_map(|c| <[u8; 2]>::try_from(c).ok())
            .map(|c| if big { u16::from_be_bytes(c) } else { u16::from_le_bytes(c) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => words(rest, false),
        [0xFE, 0xFF, rest @ ..] => words(rest, true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => match std::str::from_utf8(bytes) {
            Ok(t) => t.to_string(),
            // Latin-1 for the bytes of Windows-1252 that it shares; the others are rare in text.
            Err(_) => bytes.iter().map(|b| char::from(*b)).collect(),
        },
    }
}

/// A signature as Outlook keeps it, found to be imported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub text: String,
    /// It has pictures or formatting its plain text leaves out.
    pub formatted: bool,
}

/// The signatures in Outlook's folder of them, by name, each from its `.txt`, else as the text
/// of its `.htm`. Only read; none where the folder is not there.
pub fn read_outlook(dir: &Path) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = Vec::new();
    for e in entries.flatten().take(MAX_SIGNATURES.saturating_mul(8)) {
        let path = e.path();
        let ext = path.extension().and_then(|x| x.to_str()).map(str::to_ascii_lowercase);
        if !matches!(ext.as_deref(), Some("txt" | "htm" | "html")) {
            continue;
        }
        if let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned())
            && !names.iter().any(|n| n.eq_ignore_ascii_case(&stem))
        {
            names.push(stem);
        }
    }
    names.sort_by_key(|n| n.to_lowercase());
    let read = |name: &str, ext: &str| -> Option<String> {
        let path = dir.join(format!("{name}.{ext}"));
        let meta = std::fs::metadata(&path).ok()?;
        if !meta.is_file() || meta.len() > MAX_FILE {
            return None;
        }
        std::fs::read(&path).ok().map(|b| decode(&b))
    };
    let mut out = Vec::new();
    for stem in names {
        let Some(name) = clean_name(&stem) else { continue };
        let html = read(&stem, "htm").or_else(|| read(&stem, "html"));
        let pictures = dir.join(format!("{stem}_files")).is_dir();
        let text = match read(&stem, "txt") {
            Some(t) => clean_text(&t),
            None => match &html {
                Some(h) => clean_text(&openmailclient_formats::html::parse(h).text()),
                None => continue,
            },
        };
        out.push(Found { name, text, formatted: pictures || html.is_some() });
        if out.len() >= MAX_SIGNATURES {
            break;
        }
    }
    out
}

/// Outlook's folder of signatures for the person signed in, where it runs.
pub fn outlook_dir() -> Option<std::path::PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    std::env::var_os("APPDATA").map(|d| Path::new(&d).join("Microsoft").join("Signatures"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_and_read_back_as_the_settings_keep_them() {
        let mut s = Signatures::default();
        assert!(s.put(Signature { name: "Work".into(), text: "Alex Morgan\nExample Ltd".into() }));
        assert!(s.put(Signature { name: "Short".into(), text: "— {name}".into() }));
        s.default = Choice { new: Some("Work".into()), replies: Some("Short".into()) };
        s.choose("a2", Some(Choice { new: Some("Short".into()), replies: None }));
        let (back, ignored) = Signatures::of_json(&s.to_json()).unwrap();
        assert_eq!((back.clone(), ignored), (s.clone(), 0));
        assert_eq!(back.choice_for("a2").new.as_deref(), Some("Short"));
        assert_eq!(back.choice_for("a1").new.as_deref(), Some("Work"), "an account with no choice of its own takes the default");
        // Renamed, the choices follow; taken away, they go with it.
        s.rename("Short", "Brief");
        assert_eq!(s.default.replies.as_deref(), Some("Brief"));
        assert_eq!(s.choice_for("a2").new.as_deref(), Some("Brief"));
        assert!(s.remove("brief"));
        assert_eq!(s.default.replies, None);
        assert_eq!(s.choice_for("a2"), &Choice::default(), "its own choice stays, of none");
        s.choose("a2", None);
        assert!(s.by_account.is_empty());
        assert_eq!(s.free_name("Work").as_deref(), Some("Work (2)"));
    }

    #[test]
    fn what_makes_no_sense_in_the_settings_is_left_out() {
        let v = json!({
            "list": [{"name": "A", "text": "one\r\ntwo  \r\n\r\n"}, {"name": "a", "text": "again"}, {"name": "", "text": "x"}, {"name": "B"}, 7,
                     {"name": "C\u{7}", "text": "y"}],
            "default": {"new": "A", "replies": "gone"},
            "accounts": [{"account": "", "new": "A"}, {"account": "x".repeat(65)}, {"account": "a1", "new": "nothing"}],
        });
        let (s, ignored) = Signatures::of_json(&v).unwrap();
        assert_eq!(s.list, vec![Signature { name: "A".into(), text: "one\ntwo".into() }]);
        assert_eq!(s.default, Choice { new: Some("A".into()), replies: None });
        assert_eq!(s.by_account, vec![("a1".to_string(), Choice::default())], "a choice of nothing kept is a choice of none");
        assert_eq!(ignored, 7);
        assert!(Signatures::of_json(&json!("no")).is_none());
        let long = "é".repeat(MAX_TEXT);
        assert!(clean_text(&long).len() <= MAX_TEXT);
        assert_eq!(fill("{name} <{email}>", "Alex", "alex@example.org"), "Alex <alex@example.org>");
    }

    #[test]
    fn outlooks_files_are_read_as_it_writes_them() {
        let dir = std::env::temp_dir().join(format!("omc-signatures-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Formal_files")).unwrap();
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain("\r\nRegards,\r\nAlex Morgan\r\n".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        std::fs::write(dir.join("Formal.txt"), &utf16).unwrap();
        std::fs::write(dir.join("Formal.htm"), "<p>Regards</p>").unwrap();
        std::fs::write(dir.join("Formal.rtf"), "{\\rtf1 Regards}").unwrap();
        std::fs::write(dir.join("Web only.htm"), "<html><body><p>Cheers,</p><p><b>Sam</b></p></body></html>").unwrap();
        std::fs::write(dir.join("Latin.txt"), [0x43, 0x61, 0x66, 0xE9]).unwrap();
        std::fs::write(dir.join("notes.doc"), "not one").unwrap();
        let found = read_outlook(&dir);
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Formal", "Latin", "Web only"]);
        assert_eq!(found[0].text, "Regards,\nAlex Morgan");
        assert!(found[0].formatted);
        assert_eq!((found[1].text.as_str(), found[1].formatted), ("Café", false));
        assert!(found[2].text.contains("Cheers,") && found[2].text.contains("Sam"), "{:?}", found[2].text);
        assert!(read_outlook(&dir.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        // Garbage of any kind decodes to something.
        for junk in [&[0xFF, 0xFE, 0x00][..], &[0xFE, 0xFF, 0xD8, 0x00], &[0xEF, 0xBB, 0xBF, 0xFF], &[0x80; 7]] {
            let _ = decode(junk);
        }
    }
}
