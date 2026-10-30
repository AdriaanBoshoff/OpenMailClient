//! Signatures (File > Options > Mail > Signatures, and Message > Include > Signature in the
//! compose window): made, changed, renamed and deleted, and chosen for new messages and for
//! replies and forwards, for each account or for all. What is chosen is put in each message as
//! it is started (`cmd/compose.rs`), and changed with the account it goes from.

use super::{CommandSpec, always, need_str, ok};
use crate::signatures::{self, Choice, Signature};
use crate::{CmdResult, EngineError, Session};
use serde_json::{Value, json};

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "signatures.list",
            label: "Signatures",
            tab: "File",
            group: "Options",
            shortcut: "",
            params: r#"{}; returns {"signatures": [{"name", "text"}], "default": {"new", "replies"}, "accounts": [{"account", "name", "new", "replies", "own": it has a choice of its own}]}; {name} and {email} in a signature's text are filled with those of the account a message goes from"#,
            enabled: always,
            run: list,
        },
        CommandSpec {
            id: "signatures.save",
            label: "Save Signature",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"name", "text", "was": the name it had, to rename it}; makes one or changes one of that name; returns {"name"}"#,
            enabled: always,
            run: save,
        },
        CommandSpec {
            id: "signatures.delete",
            label: "Delete Signature",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"name"}; it is no longer chosen for anything either"#,
            enabled: |s| !s.signatures.list.is_empty(),
            run: delete,
        },
        CommandSpec {
            id: "signatures.choose",
            label: "Choose Default Signature",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"account": its id, or "" or none for every account with no choice of its own, "new": a signature's name or null for none, "replies": the same for replies and forwards, "own": false to have the account take the default again}; what is not given stays as it was"#,
            enabled: always,
            run: choose,
        },
    ]
}

fn list(s: &mut Session, _: &Value) -> CmdResult {
    let sigs = &s.signatures;
    let accounts: Vec<Value> = s
        .accounts
        .iter()
        .map(|a| {
            let c = sigs.choice_for(&a.id);
            let own = sigs.by_account.iter().any(|(id, _)| *id == a.id);
            json!({ "account": a.id, "name": a.name, "new": c.new, "replies": c.replies, "own": own })
        })
        .collect();
    Ok(json!({
        "signatures": sigs.list.iter().map(|g| json!({ "name": g.name, "text": g.text })).collect::<Vec<_>>(),
        "default": { "new": sigs.default.new, "replies": sigs.default.replies },
        "accounts": accounts,
    }))
}

fn name_of(p: &Value, key: &str) -> Result<String, EngineError> {
    signatures::clean_name(need_str(p, key)?)
        .ok_or_else(|| EngineError::BadParams(format!("a signature's name is one line of 1 to {} characters", signatures::MAX_NAME)))
}

fn save(s: &mut Session, p: &Value) -> CmdResult {
    let name = name_of(p, "name")?;
    let text = need_str(p, "text")?;
    if text.len() > signatures::MAX_TEXT.saturating_mul(4) {
        return Err(EngineError::BadParams("this signature is longer than any is kept".into()));
    }
    let text = signatures::clean_text(text);
    if p.get("was").is_some_and(|v| !v.is_null()) {
        let was = name_of(p, "was")?;
        if s.signatures.get(&was).is_none() {
            return Err(EngineError::BadParams(format!("there is no signature called {was}")));
        }
        if !was.eq_ignore_ascii_case(&name) && s.signatures.get(&name).is_some() {
            return Err(EngineError::BadParams(format!("there is a signature called {name} already")));
        }
        s.signatures.rename(&was, &name);
    }
    if !s.signatures.put(Signature { name: name.clone(), text }) {
        return Err(EngineError::Format(format!("no more than {} signatures are kept", signatures::MAX_SIGNATURES)));
    }
    Ok(json!({ "name": name }))
}

fn delete(s: &mut Session, p: &Value) -> CmdResult {
    let name = name_of(p, "name")?;
    if !s.signatures.remove(&name) {
        return Err(EngineError::BadParams(format!("there is no signature called {name}")));
    }
    ok()
}

fn choose(s: &mut Session, p: &Value) -> CmdResult {
    let account = match p.get("account") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(id)) if id.is_empty() => String::new(),
        Some(Value::String(id)) => {
            s.accounts.iter().find(|a| a.id == *id).map(|a| a.id.clone()).ok_or_else(|| EngineError::BadParams("there is no such account".into()))?
        }
        Some(_) => return Err(EngineError::BadParams("\"account\" is an account's id".into())),
    };
    let pick = |key: &str, was: Option<String>| -> Result<Option<String>, EngineError> {
        match p.get(key) {
            None => Ok(was),
            Some(Value::Null) => Ok(None),
            Some(Value::String(n)) => s.signatures.get(n).map(|g| Some(g.name.clone())).ok_or_else(|| {
                EngineError::BadParams(format!("there is no signature called {}", n.chars().take(signatures::MAX_NAME).collect::<String>()))
            }),
            Some(_) => Err(EngineError::BadParams(format!("\"{key}\" is a signature's name, or null for none"))),
        }
    };
    // What the account chose itself, not what it takes from the default.
    let was = s.signatures.choice_for(&account).clone();
    if p.get("own").and_then(Value::as_bool) == Some(false) {
        s.signatures.choose(&account, None);
        return ok();
    }
    let choice = Choice { new: pick("new", was.new)?, replies: pick("replies", was.replies)? };
    s.signatures.choose(&account, Some(choice));
    ok()
}
