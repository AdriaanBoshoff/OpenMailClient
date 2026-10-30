//! Rules and Alerts (Home > Move > Rules): rules made, changed, turned on and off, put in
//! order and deleted; Create Rule from the selected message; and Run Rules Now on a folder,
//! with a look first at what would be done. What a rule does is done here, by `apply`, for a
//! message as it comes into an Inbox (`cmd/receive.rs`) and for each one Run Rules Now takes.
//!
//! What a rule moves goes within its own data file: removed and added again, as a move by hand
//! is, so an IMAP account's server follows it on the next Send/Receive. A rule never deletes
//! for good; a message it deletes is in Deleted Items.

use super::{CommandSpec, always, clock, has_selection, ok, p_bool, p_str, p_u64};
use crate::rules::{self, Act, Plan, Rule, Test};
use crate::{CmdResult, EngineError, Session};
use openmailclient_model::task::{self, FollowUp};
use openmailclient_model::{FolderId, FolderKind, ItemId, PropValue, pid};
use serde_json::{Value, json};

/// Messages Run Rules Now tells of when it only looks.
const MAX_SHOWN: usize = 500;
/// Folders Run Rules Now goes into, with their subfolders.
const MAX_FOLDERS: usize = 1024;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "rules.list",
            label: "Manage Rules & Alerts",
            tab: "Home",
            group: "Move",
            shortcut: "",
            params: r#"{}; returns {"rules": [{"name", "on", "account", "when": [conditions], "except": [conditions], "then": [actions], "words": what it does, in words}]}, in the order they are tried. A condition is {"test": "from"|"sentTo"|"subject"|"body"|"subjectOrBody"|"header"|"senderAddress"|"recipientAddress", "words": [..]}, {"test": "received", "after": "YYYY-MM-DD", "before": "YYYY-MM-DD"}, {"test": "toMe"|"ccMe"|"toOrCcMe"|"onlyToMe"|"hasAttachment"|"meeting"}, {"test": "importance", "level": "low"|"normal"|"high"} or {"test": "size", "min": KB, "max": KB}; an action {"do": "move"|"copy", "folder": a path in its data file}, {"do": "delete"|"markRead"|"flag"|"clearFlag"|"stop"}, {"do": "category", "name"}, {"do": "importance", "level"}, {"do": "forward", "to": [addresses]}, {"do": "reply", "words"} or {"do": "sound"}; a forward or reply is filed in the Outbox, never made of mail a machine wrote, and a sender is answered once"#,
            enabled: always,
            run: list,
        },
        CommandSpec {
            id: "rules.save",
            label: "Save Rule",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"rule": {"name", "on", "account": an account's id or null for all, "when", "except", "then"}, "index": the rule it takes the place of, or none for a new one at the end}; returns {"index"}"#,
            enabled: always,
            run: save,
        },
        CommandSpec {
            id: "rules.delete",
            label: "Delete Rule",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"index"}"#,
            enabled: |s| !s.rules.is_empty(),
            run: delete,
        },
        CommandSpec {
            id: "rules.move",
            label: "Move Rule",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"index", "to": its place in the order}"#,
            enabled: |s| s.rules.len() > 1,
            run: move_rule,
        },
        CommandSpec {
            id: "rules.turn",
            label: "Turn Rule On or Off",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"index", "on"}"#,
            enabled: |s| !s.rules.is_empty(),
            run: turn,
        },
        CommandSpec {
            id: "rules.create",
            label: "Create Rule",
            tab: "Home",
            group: "Move",
            shortcut: "",
            params: r#"{}; returns {"rule"}: one made from the selected message, from its sender and with its subject, that moves it nowhere yet; nothing is kept until rules.save"#,
            enabled: has_selection,
            run: create,
        },
        CommandSpec {
            id: "rules.runNow",
            label: "Run Rules Now",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"folder": a path in the current data file (the current folder where none, or the Inbox), "rules": [names] (every rule that is on where none; a rule turned off may be named), "subfolders": true to go into them too, "unread": true for unread messages alone, "preview": true to change nothing and say what would be done}; returns {"looked", "matched", "moved", "deleted", "messages": [{"folder", "subject", "from", "rules", "does"}] (when previewed), "trouble": [..]}"#,
            enabled: |s| !s.rules.is_empty() && !s.stores.is_empty(),
            run: run_now,
        },
    ]
}

fn rule_json(s: &Session, r: &Rule) -> Value {
    let account = r.account.as_deref().map(|id| s.accounts.iter().find(|a| a.id == id).map_or(id, |a| a.name.as_str()));
    let mut v = r.to_json();
    v["words"] = json!(r.words(account));
    v
}

fn list(s: &mut Session, _: &Value) -> CmdResult {
    Ok(json!({ "rules": s.rules.iter().map(|r| rule_json(s, r)).collect::<Vec<_>>() }))
}

fn index_of(s: &Session, p: &Value, key: &str) -> Result<usize, EngineError> {
    let i = p_u64(p, key).ok_or_else(|| EngineError::BadParams(format!("\"{key}\" (a rule's place, from 0) is required")))?;
    usize::try_from(i).ok().filter(|&i| i < s.rules.len()).ok_or_else(|| EngineError::BadParams("there is no rule at that place".into()))
}

fn save(s: &mut Session, p: &Value) -> CmdResult {
    let rule = Rule::of_json(p.get("rule").ok_or_else(|| EngineError::BadParams("\"rule\" is required".into()))?).map_err(EngineError::BadParams)?;
    if let Some(a) = &rule.account
        && !s.accounts.iter().any(|x| x.id == *a)
    {
        return Err(EngineError::BadParams("there is no such account".into()));
    }
    let index = match p.get("index").filter(|v| !v.is_null()) {
        Some(_) => {
            let i = index_of(s, p, "index")?;
            if let Some(slot) = s.rules.get_mut(i) {
                *slot = rule;
            }
            i
        }
        None => {
            if s.rules.len() >= rules::MAX_RULES {
                return Err(EngineError::Format(format!("no more than {} rules are kept", rules::MAX_RULES)));
            }
            s.rules.push(rule);
            s.rules.len() - 1
        }
    };
    Ok(json!({ "index": index }))
}

fn delete(s: &mut Session, p: &Value) -> CmdResult {
    let i = index_of(s, p, "index")?;
    s.rules.remove(i);
    ok()
}

fn move_rule(s: &mut Session, p: &Value) -> CmdResult {
    let i = index_of(s, p, "index")?;
    let to = index_of(s, p, "to")?;
    let r = s.rules.remove(i);
    s.rules.insert(to.min(s.rules.len()), r);
    ok()
}

fn turn(s: &mut Session, p: &Value) -> CmdResult {
    let i = index_of(s, p, "index")?;
    let on = p_bool(p, "on").ok_or_else(|| EngineError::BadParams("\"on\" (true or false) is required".into()))?;
    if let Some(r) = s.rules.get_mut(i) {
        r.on = on;
    }
    ok()
}

/// A rule's words, from text of a message: of one line and cut to what a rule keeps.
fn word(text: &str) -> Option<String> {
    let w: String = text.chars().filter(|c| !c.is_control()).take(rules::MAX_WORD).collect();
    let w = w.trim();
    (!w.is_empty()).then(|| w.to_string())
}

fn create(s: &mut Session, _: &Value) -> CmdResult {
    let item = s.open.as_ref().ok_or_else(|| EngineError::BadParams("no message is selected".into()))?;
    let mut when = Vec::new();
    let email = item.sender_email();
    let sender = if email.contains('@') { email.clone() } else { item.sender_name() };
    if let Some(from) = word(&sender) {
        when.push(Test::From(vec![from]));
    }
    if let Some(subject) = word(openmailclient_model::strip_reply_prefix(&item.subject())) {
        when.push(Test::Subject(vec![subject]));
    }
    let name = word(&item.sender_name()).or_else(|| word(&email)).unwrap_or_else(|| "New rule".into());
    let rule = Rule { name, on: true, account: None, when, except: Vec::new(), then: vec![Act::MarkRead] };
    Ok(json!({ "rule": rule_json(s, &rule) }))
}

/// Every address the owner has: each account's and the one set as theirs.
pub(crate) fn me(s: &Session) -> Vec<String> {
    let mut me: Vec<String> = s.accounts.iter().map(|a| a.email.trim().to_ascii_lowercase()).filter(|e| !e.is_empty()).collect();
    let own = s.user_email.trim().to_ascii_lowercase();
    if !own.is_empty() && !me.contains(&own) {
        me.push(own);
    }
    me
}

/// The account whose own data file is the store `store`, as `account::account_store` keeps one.
pub(crate) fn account_of_store(s: &Session, store: usize) -> Option<String> {
    let real = s.stores.get(store)?.path().and_then(|p| std::fs::canonicalize(p).ok())?;
    s.accounts
        .iter()
        .find(|a| super::account::account_path(s, a).and_then(|p| std::fs::canonicalize(p).ok()).as_ref() == Some(&real))
        .map(|a| a.id.clone())
}

/// What `plan` says done to the message `id` of `folder` in the store `store`, and where it is
/// now. What cannot be done is told of in `trouble`; what can still is done. A message that
/// cannot be put where it was going stays where it was.
pub(crate) fn apply(
    s: &mut Session,
    store: usize,
    folder: FolderId,
    id: u64,
    plan: &Plan,
    trouble: &mut Vec<String>,
    undo: Option<&mut Vec<crate::undo::Ruled>>,
    answered: &mut rules::Answered,
) -> Result<(FolderId, u64), EngineError> {
    if plan.rules.is_empty() {
        return Ok((folder, id));
    }
    let named = plan.rules.join(", ");
    let (today, now, zone) = (s.today(), clock(s), s.zone.clone());
    let (start, due) = FollowUp::Today.days(today);
    let mut copies = Vec::new();
    for path in &plan.copies {
        match s.folder_by_path(store, path) {
            Some(f) if f != folder => copies.push(f),
            Some(_) => {}
            None => trouble.push(format!("rule {named}: there is no folder {path} to copy to")),
        }
    }
    let to = if plan.delete {
        match s.find_folder(store, FolderKind::Deleted) {
            Some(f) => Some(f),
            None => {
                trouble.push(format!("rule {named}: this data file has no Deleted Items"));
                None
            }
        }
    } else {
        plan.move_to.as_deref().and_then(|path| {
            let f = s.folder_by_path(store, path);
            if f.is_none() {
                trouble.push(format!("rule {named}: there is no folder {path} to move to"));
            }
            f
        })
    };
    let mut item = s.stores.get_mut(store).ok_or(openmailclient_model::StoreError::NoFolder)?.load(folder, ItemId(id))?;
    let before = item.clone();
    answer(s, &before, plan, answered, trouble);
    let st = s.stores.get_mut(store).ok_or(openmailclient_model::StoreError::NoFolder)?;
    if plan.read {
        item.set_read(true);
    }
    match plan.flag {
        Some(true) => task::follow_up(&mut item, start, due, now, &zone),
        Some(false) => task::clear_flag(&mut item),
        None => {}
    }
    for c in &plan.categories {
        if !item.categories.iter().any(|x| x.eq_ignore_ascii_case(c)) {
            item.categories.push(c.clone());
        }
    }
    if let Some(level) = plan.importance {
        item.props.set(pid::IMPORTANCE, PropValue::Int32(level));
    }
    if item != before {
        st.update(folder, ItemId(id), item.clone())?;
    }
    let mut made = Vec::new();
    for f in copies {
        let mut copy = item.clone();
        // A copy is a message of its own, not the one the server has.
        super::imap_sync::Place::clear(&mut copy);
        match st.add(f, copy) {
            Ok(c) => made.push((f, c.0)),
            Err(e) => trouble.push(format!("rule {named}: a copy could not be kept: {e}")),
        }
    }
    let mut at = (folder, id);
    if let Some(to) = to.filter(|&t| t != folder) {
        let taken = st.remove(folder, ItemId(id))?;
        match st.add(to, taken.clone()) {
            Ok(new) => at = (to, new.0),
            Err(e) => {
                trouble.push(format!("rule {named}: it could not be moved: {e}"));
                let back = st.add(folder, taken)?;
                at = (folder, back.0);
            }
        }
    }
    if let Some(undo) = undo {
        undo.push(crate::undo::Ruled { from: folder, before: Box::new(before), at: at.0, id: at.1, copies: made });
    }
    Ok(at)
}

/// The forward and the reply a plan asks for, filed in the Outbox. Nothing written by a machine
/// is answered or forwarded, no sender is answered twice, nothing goes back to one of my own
/// addresses, and forwards stop for the hour after [`rules::MAX_FORWARDS_AN_HOUR`].
fn answer(s: &mut Session, item: &openmailclient_model::Item, plan: &Plan, answered: &mut rules::Answered, trouble: &mut Vec<String>) {
    if plan.forward.is_empty() && plan.reply.is_none() {
        return;
    }
    let named = plan.rules.join(", ");
    if rules::automatic(item) {
        trouble.push(format!("rule {named}: a message written by a machine is not answered or forwarded"));
        return;
    }
    let me = me(s);
    let mine = |a: &str| me.iter().any(|m| m.eq_ignore_ascii_case(a.trim()));
    let to: Vec<&str> = plan.forward.iter().map(String::as_str).filter(|a| !mine(a)).collect();
    if !to.is_empty() {
        if answered.may_forward(clock(s)) {
            if let Err(e) = super::compose::by_rule(s, item, true, &to.join(", "), "") {
                trouble.push(format!("rule {named}: it could not be forwarded: {e}"));
            }
        } else {
            trouble.push(format!("rule {named}: {} forwards were made in the last hour; no more are made", rules::MAX_FORWARDS_AN_HOUR));
        }
    }
    if let Some(words) = &plan.reply {
        let sender = item.sender_email();
        if sender.contains('@') && !mine(&sender) && answered.may_reply(&sender) {
            let words = format!("{}\n\n", words.trim_end());
            if let Err(e) = super::compose::by_rule(s, item, false, &sender, &words) {
                trouble.push(format!("rule {named}: it could not be answered: {e}"));
            }
        }
    }
}

/// The folder Run Rules Now starts at, and those under it where asked.
fn folders_of(s: &Session, store: usize, start: FolderId, subfolders: bool) -> Vec<FolderId> {
    let mut out = vec![start];
    if subfolders {
        let list = s.folders.get(store).map(Vec::as_slice).unwrap_or_default();
        let mut i = 0;
        while let Some(&f) = out.get(i) {
            for c in list.iter().filter(|c| c.parent == Some(f) && !c.hidden) {
                if out.len() < MAX_FOLDERS && !out.contains(&c.id) {
                    out.push(c.id);
                }
            }
            i += 1;
        }
    }
    out
}

fn does(plan: &Plan) -> String {
    let mut out = Vec::new();
    if plan.delete {
        out.push("put in Deleted Items".to_string());
    } else if let Some(f) = &plan.move_to {
        out.push(format!("moved to {f}"));
    }
    out.extend(plan.copies.iter().map(|c| format!("copied to {c}")));
    if plan.read {
        out.push("marked read".into());
    }
    match plan.flag {
        Some(true) => out.push("flagged".into()),
        Some(false) => out.push("flag cleared".into()),
        None => {}
    }
    out.extend(plan.categories.iter().map(|c| format!("in {c}")));
    if !plan.forward.is_empty() {
        out.push(format!("forwarded to {}", plan.forward.join(", ")));
    }
    if plan.reply.is_some() {
        out.push("answered".into());
    }
    if let Some(l) = plan.importance {
        out.push(["low", "normal", "high"].get(usize::try_from(l).unwrap_or(1)).map_or("normal", |l| l).to_string() + " importance");
    }
    out.join(", ")
}

fn run_now(s: &mut Session, p: &Value) -> CmdResult {
    let store = s.current.map_or(0, |(st, _)| st);
    if s.stores.get(store).is_none() {
        return Err(EngineError::BadParams("no data file is open".into()));
    }
    let start = match p_str(p, "folder") {
        Some(path) => s.folder_by_path(store, path).ok_or_else(|| EngineError::BadParams("there is no such folder in this data file".into()))?,
        None => match s.current.map(|(_, f)| f).or_else(|| s.find_folder(store, FolderKind::Inbox)) {
            Some(f) => f,
            None => return Err(EngineError::BadParams("\"folder\" is required".into())),
        },
    };
    let chosen: Vec<Rule> = match p.get("rules").filter(|v| !v.is_null()) {
        Some(Value::Array(names)) => {
            let mut out = Vec::new();
            for n in names {
                let n = n.as_str().ok_or_else(|| EngineError::BadParams("\"rules\" is a list of rules' names".into()))?;
                let r = s.rules.iter().find(|r| r.name.eq_ignore_ascii_case(n)).ok_or_else(|| {
                    EngineError::BadParams(format!("there is no rule called {}", n.chars().take(rules::MAX_NAME).collect::<String>()))
                })?;
                out.push(Rule { on: true, ..r.clone() });
            }
            out
        }
        Some(_) => return Err(EngineError::BadParams("\"rules\" is a list of rules' names".into())),
        None => s.rules.iter().filter(|r| r.on).cloned().collect(),
    };
    let preview = p_bool(p, "preview").unwrap_or(false);
    let unread_only = p_bool(p, "unread").unwrap_or(false);
    if !preview && !s.stores.get(store).is_some_and(|st| st.writable()) {
        return Err(EngineError::Format("this data file cannot be changed".into()));
    }
    let account = account_of_store(s, store);
    let me = me(s);
    let folders = folders_of(s, store, start, p_bool(p, "subfolders").unwrap_or(false));
    // Each folder's messages as they are before any is moved, so none is taken twice.
    let mut each = Vec::new();
    for f in folders {
        let ids: Vec<(u64, bool)> =
            s.stores.get_mut(store).and_then(|st| st.list(f).ok()).unwrap_or_default().iter().map(|m| (m.id, m.read)).collect();
        each.push((f, ids));
    }
    let (mut looked, mut matched, mut moved, mut deleted) = (0usize, 0usize, 0usize, 0usize);
    let mut done = Vec::new();
    let mut answered = rules::Answered::default();
    let mut shown = Vec::new();
    let mut trouble = Vec::new();
    for (f, ids) in each {
        let name = s.folder_info(store, f).map(|i| i.name.clone()).unwrap_or_default();
        for (id, read) in ids {
            if unread_only && read {
                continue;
            }
            let Some(item) = s.stores.get_mut(store).and_then(|st| st.load(f, ItemId(id)).ok()) else { continue };
            looked += 1;
            let plan = rules::plan(&chosen, &item, account.as_deref(), &me, &s.zone);
            if plan.rules.is_empty() {
                continue;
            }
            matched += 1;
            deleted += usize::from(plan.delete);
            if preview {
                if shown.len() < MAX_SHOWN {
                    shown.push(
                        json!({ "folder": name, "subject": item.subject(), "from": item.sender_name(), "rules": plan.rules, "does": does(&plan) }),
                    );
                }
                continue;
            }
            match apply(s, store, f, id, &plan, &mut trouble, Some(&mut done), &mut answered) {
                Ok((now_in, _)) => moved += usize::from(now_in != f),
                Err(e) => trouble.push(format!("{}: {e}", item.subject())),
            }
            if trouble.len() > MAX_SHOWN {
                trouble.truncate(MAX_SHOWN);
            }
        }
    }
    if !done.is_empty() {
        s.did(crate::undo::Step::Ruled { what: "Run Rules Now", store, done });
    }
    if !preview && matched > 0 {
        s.refresh_folders(store);
        s.reload_rows()?;
        // The message that was open may have been moved away.
        if s.selected.is_some_and(|id| !s.rows.iter().any(|r| r.id == id)) {
            s.selected = None;
            s.open = None;
        }
    }
    s.status = if preview {
        format!("The rules would take {matched} of {looked} messages")
    } else {
        format!("The rules took {matched} of {looked} messages")
    };
    let mut out = json!({ "looked": looked, "matched": matched, "moved": moved, "deleted": deleted, "trouble": trouble });
    if preview {
        out["messages"] = json!(shown);
    }
    Ok(out)
}

/// Rules run on a message that came into the Inbox `folder` of `store` through the account
/// `account`; where it is now, and whether it is still to be told of as new.
pub(crate) fn on_arrival(
    s: &mut Session,
    store: usize,
    folder: FolderId,
    id: u64,
    account: &str,
    trouble: &mut Vec<String>,
) -> (FolderId, u64, bool) {
    if s.rules.iter().all(|r| !r.on) {
        return (folder, id, true);
    }
    let Some(item) = s.stores.get_mut(store).and_then(|st| st.load(folder, ItemId(id)).ok()) else { return (folder, id, true) };
    let me = me(s);
    let plan = rules::plan(&s.rules, &item, Some(account), &me, &s.zone);
    if plan.rules.is_empty() {
        return (folder, id, true);
    }
    if plan.sound {
        s.arrived.sounds = s.arrived.sounds.saturating_add(1);
    }
    let mut answered = std::mem::take(&mut s.arrived.answered);
    let done = apply(s, store, folder, id, &plan, trouble, None, &mut answered);
    s.arrived.answered = answered;
    match done {
        Ok((f, new)) => (f, new, !plan.read && !plan.delete),
        Err(e) => {
            trouble.push(format!("the rules could not be run on a message: {e}"));
            (folder, id, true)
        }
    }
}
