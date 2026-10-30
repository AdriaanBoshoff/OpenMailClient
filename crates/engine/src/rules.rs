//! Rules (Home > Rules > Manage Rules & Alerts): what is done to a message as it comes into an
//! Inbox, or to the messages of a folder when Run Rules Now asks, by the conditions it meets and
//! the exceptions it does not. Each rule is tried in turn, as Outlook tries them, until one says
//! to stop; what they say to do is put together and done once (`cmd/rules.rs`).
//!
//! Going beyond Outlook: a rule never deletes a message for good, only puts it in Deleted Items,
//! so a rule that catches more than was meant loses nothing; and Run Rules Now can first show
//! what it would do, changing nothing.
//!
//! Rules are kept with the settings. What is read is hostile: names, words and lists are capped,
//! and a part that makes no sense is left out and counted.

use openmailclient_model::{Item, RECIP_BCC, RECIP_CC, RECIP_TO, meeting, pid};
use serde_json::{Value, json};

pub const MAX_RULES: usize = 256;
pub const MAX_NAME: usize = 255;
/// Words a condition looks for, and characters in each.
pub const MAX_WORDS: usize = 64;
pub const MAX_WORD: usize = 255;
/// Conditions, exceptions and actions in one rule, each.
pub const MAX_PARTS: usize = 32;
/// A folder's path, in characters.
const MAX_PATH: usize = 1024;

/// What a message is tested for. Words are looked for anywhere in what is named, any case, and
/// any one of them is enough.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Test {
    /// From someone whose name or address has one of these.
    From(Vec<String>),
    /// Sent to someone (To or Cc) whose name or address has one of these.
    SentTo(Vec<String>),
    Subject(Vec<String>),
    Body(Vec<String>),
    SubjectOrBody(Vec<String>),
    /// The message's internet header has one of these.
    Header(Vec<String>),
    /// Where an address of the owner's is in the To box.
    ToMe,
    CcMe,
    ToOrCcMe,
    /// Sent to the owner alone.
    OnlyToMe,
    HasAttachment,
    /// Of this importance: 0 low, 1 normal, 2 high.
    Importance(i32),
    /// Of a size between these, in kilobytes; 0 for no bound.
    Size {
        min: u64,
        max: u64,
    },
    /// A meeting's request, answer or cancellation.
    Meeting,
}

/// What is done to a message a rule takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Moved to the folder of this path in its data file ("Inbox/Projects").
    Move(String),
    Copy(String),
    /// Put in Deleted Items.
    Delete,
    MarkRead,
    /// Flagged for follow-up today.
    Flag,
    ClearFlag,
    Category(String),
    Importance(i32),
    /// No rule after this one is tried.
    Stop,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rule {
    pub name: String,
    pub on: bool,
    /// Only mail that comes through this account, by its id; every account's where `None`.
    pub account: Option<String>,
    /// Every one of these is met; a rule with none takes every message.
    pub when: Vec<Test>,
    /// None of these is met.
    pub except: Vec<Test>,
    pub then: Vec<Act>,
}

/// What the rules that take a message say to do, put together.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// The names of the rules that took it, in order.
    pub rules: Vec<String>,
    pub read: bool,
    /// Flagged (true) or its flag cleared (false).
    pub flag: Option<bool>,
    pub categories: Vec<String>,
    pub importance: Option<i32>,
    /// Where it goes: the first move asked for. A later rule's move is a copy, as Outlook does.
    pub move_to: Option<String>,
    pub copies: Vec<String>,
    /// Put in Deleted Items, whatever else was asked.
    pub delete: bool,
}

/// What a message is and who it went to, lowered once for every rule.
struct Seen {
    from: String,
    sent_to: String,
    subject: String,
    body: String,
    header: String,
}

/// The addresses of a message's recipients of one kind.
fn addresses(item: &Item, kind: i32) -> Vec<String> {
    item.recipients
        .iter()
        .filter(|r| r.i32(pid::RECIPIENT_TYPE).unwrap_or(RECIP_TO) == kind)
        .map(|r| r.str(pid::SMTP_ADDRESS).or_else(|| r.str(pid::EMAIL_ADDRESS)).unwrap_or("").trim().to_ascii_lowercase())
        .collect()
}

impl Seen {
    fn of(item: &Item) -> Self {
        let mut from = vec![item.sender_name(), item.sender_email()];
        for id in [pid::SENT_REPRESENTING_NAME, pid::SENT_REPRESENTING_EMAIL_ADDRESS, pid::SENDER_SMTP_ADDRESS, pid::SENT_REPRESENTING_SMTP_ADDRESS] {
            from.push(item.props.str(id).unwrap_or("").to_string());
        }
        let mut to: Vec<String> = Vec::new();
        for r in item.recipients.iter().filter(|r| r.i32(pid::RECIPIENT_TYPE).unwrap_or(RECIP_TO) != RECIP_BCC) {
            for id in [pid::DISPLAY_NAME, pid::SMTP_ADDRESS, pid::EMAIL_ADDRESS] {
                to.push(r.str(id).unwrap_or("").to_string());
            }
        }
        to.push(item.display_recipients(RECIP_TO));
        to.push(item.display_recipients(RECIP_CC));
        Seen {
            from: from.join("\n").to_lowercase(),
            sent_to: to.join("\n").to_lowercase(),
            subject: item.subject().to_lowercase(),
            body: item.body_text().to_lowercase(),
            header: item.props.str(pid::TRANSPORT_MESSAGE_HEADERS).unwrap_or("").to_lowercase(),
        }
    }
}

fn has_any(text: &str, words: &[String]) -> bool {
    words.iter().any(|w| !w.is_empty() && text.contains(&w.to_lowercase()))
}

impl Test {
    fn met(&self, item: &Item, seen: &Seen, me: &[String]) -> bool {
        let mine = |kind: i32| addresses(item, kind).iter().any(|a| me.iter().any(|m| m.eq_ignore_ascii_case(a)));
        match self {
            Test::From(w) => has_any(&seen.from, w),
            Test::SentTo(w) => has_any(&seen.sent_to, w),
            Test::Subject(w) => has_any(&seen.subject, w),
            Test::Body(w) => has_any(&seen.body, w),
            Test::SubjectOrBody(w) => has_any(&seen.subject, w) || has_any(&seen.body, w),
            Test::Header(w) => has_any(&seen.header, w),
            Test::ToMe => mine(RECIP_TO),
            Test::CcMe => mine(RECIP_CC),
            Test::ToOrCcMe => mine(RECIP_TO) || mine(RECIP_CC),
            Test::OnlyToMe => {
                let to = addresses(item, RECIP_TO);
                to.len() == 1 && addresses(item, RECIP_CC).is_empty() && mine(RECIP_TO)
            }
            Test::HasAttachment => item.visible_attachments().next().is_some(),
            Test::Importance(level) => item.importance() == *level,
            Test::Size { min, max } => {
                let kb = u64::try_from(item.approx_size()).unwrap_or(u64::MAX) / 1024;
                kb >= *min && (*max == 0 || kb <= *max)
            }
            Test::Meeting => meeting::is_meeting_message(item),
        }
    }
}

/// What the rules that are on say to do with `item`, come through the account `account`, for
/// an owner whose addresses are `me`.
pub fn plan(rules: &[Rule], item: &Item, account: Option<&str>, me: &[String]) -> Plan {
    let seen = Seen::of(item);
    let mut plan = Plan::default();
    for rule in rules.iter().filter(|r| r.on) {
        if rule.account.as_deref().is_some_and(|a| Some(a) != account) {
            continue;
        }
        if !rule.when.iter().all(|t| t.met(item, &seen, me)) || rule.except.iter().any(|t| t.met(item, &seen, me)) {
            continue;
        }
        plan.rules.push(rule.name.clone());
        let mut stop = false;
        for act in &rule.then {
            match act {
                Act::Move(f) if plan.move_to.is_none() => plan.move_to = Some(f.clone()),
                Act::Move(f) | Act::Copy(f) => {
                    if plan.move_to.as_deref() != Some(f.as_str()) && !plan.copies.contains(f) {
                        plan.copies.push(f.clone());
                    }
                }
                Act::Delete => plan.delete = true,
                Act::MarkRead => plan.read = true,
                Act::Flag => plan.flag = Some(true),
                Act::ClearFlag => plan.flag = Some(false),
                Act::Category(c) => {
                    if !plan.categories.iter().any(|x| x.eq_ignore_ascii_case(c)) {
                        plan.categories.push(c.clone());
                    }
                }
                Act::Importance(l) => plan.importance = Some(*l),
                Act::Stop => stop = true,
            }
        }
        if stop {
            break;
        }
    }
    plan
}

fn level_name(level: i32) -> &'static str {
    match level {
        0 => "low",
        2 => "high",
        _ => "normal",
    }
}

fn level_of(v: &Value) -> Result<i32, String> {
    match v.get("level").and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() {
        Some("low") => Ok(0),
        Some("normal") => Ok(1),
        Some("high") => Ok(2),
        _ => Err("\"level\" is low, normal or high".into()),
    }
}

/// Words as they are kept: each trimmed, of one line and not empty, at most `MAX_WORDS`.
fn words_of(v: &Value) -> Result<Vec<String>, String> {
    let list = v.get("words").and_then(Value::as_array).ok_or("\"words\" is a list of words to look for")?;
    let words: Vec<String> = list
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|w| !w.is_empty() && !w.chars().any(char::is_control))
        .map(|w| w.chars().take(MAX_WORD).collect())
        .take(MAX_WORDS)
        .collect();
    if words.is_empty() {
        return Err("a condition looks for one word at least".into());
    }
    Ok(words)
}

fn quoted(words: &[String]) -> String {
    words.iter().map(|w| format!("\"{w}\"")).collect::<Vec<_>>().join(" or ")
}

impl Test {
    pub fn of_json(v: &Value) -> Result<Test, String> {
        let kind = v.get("test").and_then(Value::as_str).ok_or("each condition has its \"test\"")?;
        Ok(match kind {
            "from" => Test::From(words_of(v)?),
            "sentTo" => Test::SentTo(words_of(v)?),
            "subject" => Test::Subject(words_of(v)?),
            "body" => Test::Body(words_of(v)?),
            "subjectOrBody" => Test::SubjectOrBody(words_of(v)?),
            "header" => Test::Header(words_of(v)?),
            "toMe" => Test::ToMe,
            "ccMe" => Test::CcMe,
            "toOrCcMe" => Test::ToOrCcMe,
            "onlyToMe" => Test::OnlyToMe,
            "hasAttachment" => Test::HasAttachment,
            "importance" => Test::Importance(level_of(v)?),
            "size" => {
                let kb = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0).min(u64::from(u32::MAX));
                let (min, max) = (kb("min"), kb("max"));
                if max != 0 && max < min {
                    return Err("a size's \"max\" is not below its \"min\"".into());
                }
                Test::Size { min, max }
            }
            "meeting" => Test::Meeting,
            other => return Err(format!("\"{}\" is not a condition rules know", other.chars().take(32).collect::<String>())),
        })
    }

    pub fn to_json(&self) -> Value {
        let w = |test: &str, words: &[String]| json!({ "test": test, "words": words });
        match self {
            Test::From(x) => w("from", x),
            Test::SentTo(x) => w("sentTo", x),
            Test::Subject(x) => w("subject", x),
            Test::Body(x) => w("body", x),
            Test::SubjectOrBody(x) => w("subjectOrBody", x),
            Test::Header(x) => w("header", x),
            Test::ToMe => json!({ "test": "toMe" }),
            Test::CcMe => json!({ "test": "ccMe" }),
            Test::ToOrCcMe => json!({ "test": "toOrCcMe" }),
            Test::OnlyToMe => json!({ "test": "onlyToMe" }),
            Test::HasAttachment => json!({ "test": "hasAttachment" }),
            Test::Importance(l) => json!({ "test": "importance", "level": level_name(*l) }),
            Test::Size { min, max } => json!({ "test": "size", "min": min, "max": max }),
            Test::Meeting => json!({ "test": "meeting" }),
        }
    }

    /// As a rule's description says it.
    pub fn words(&self) -> String {
        match self {
            Test::From(w) => format!("from {}", quoted(w)),
            Test::SentTo(w) => format!("sent to {}", quoted(w)),
            Test::Subject(w) => format!("with {} in the subject", quoted(w)),
            Test::Body(w) => format!("with {} in the body", quoted(w)),
            Test::SubjectOrBody(w) => format!("with {} in the subject or body", quoted(w)),
            Test::Header(w) => format!("with {} in the message header", quoted(w)),
            Test::ToMe => "where my address is in the To box".into(),
            Test::CcMe => "where my address is in the Cc box".into(),
            Test::ToOrCcMe => "where my address is in the To or Cc box".into(),
            Test::OnlyToMe => "sent only to me".into(),
            Test::HasAttachment => "which has an attachment".into(),
            Test::Importance(l) => format!("marked as {} importance", level_name(*l)),
            Test::Size { min, max: 0 } => format!("of {min} KB or more"),
            Test::Size { min, max } => format!("of {min} to {max} KB"),
            Test::Meeting => "which is a meeting's request, answer or cancellation".into(),
        }
    }
}

fn path_of(v: &Value) -> Result<String, String> {
    let p = v.get("folder").and_then(Value::as_str).map(|p| p.trim().trim_matches('/')).unwrap_or("");
    if p.is_empty() || p.chars().count() > MAX_PATH || p.chars().any(char::is_control) {
        return Err("\"folder\" is a folder's path in its data file, as \"Inbox/Projects\"".into());
    }
    Ok(p.to_string())
}

impl Act {
    pub fn of_json(v: &Value) -> Result<Act, String> {
        let kind = v.get("do").and_then(Value::as_str).ok_or("each action has its \"do\"")?;
        Ok(match kind {
            "move" => Act::Move(path_of(v)?),
            "copy" => Act::Copy(path_of(v)?),
            "delete" => Act::Delete,
            "markRead" => Act::MarkRead,
            "flag" => Act::Flag,
            "clearFlag" => Act::ClearFlag,
            "category" => {
                let c = v.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("");
                if c.is_empty() || c.chars().count() > 255 || c.chars().any(char::is_control) {
                    return Err("a category's \"name\" is 1 to 255 characters".into());
                }
                Act::Category(c.to_string())
            }
            "importance" => Act::Importance(level_of(v)?),
            "stop" => Act::Stop,
            other => return Err(format!("\"{}\" is not an action rules know", other.chars().take(32).collect::<String>())),
        })
    }

    pub fn to_json(&self) -> Value {
        match self {
            Act::Move(f) => json!({ "do": "move", "folder": f }),
            Act::Copy(f) => json!({ "do": "copy", "folder": f }),
            Act::Delete => json!({ "do": "delete" }),
            Act::MarkRead => json!({ "do": "markRead" }),
            Act::Flag => json!({ "do": "flag" }),
            Act::ClearFlag => json!({ "do": "clearFlag" }),
            Act::Category(c) => json!({ "do": "category", "name": c }),
            Act::Importance(l) => json!({ "do": "importance", "level": level_name(*l) }),
            Act::Stop => json!({ "do": "stop" }),
        }
    }

    pub fn words(&self) -> String {
        match self {
            Act::Move(f) => format!("move it to {f}"),
            Act::Copy(f) => format!("copy it to {f}"),
            Act::Delete => "put it in Deleted Items".into(),
            Act::MarkRead => "mark it as read".into(),
            Act::Flag => "flag it for follow-up today".into(),
            Act::ClearFlag => "clear its flag".into(),
            Act::Category(c) => format!("put it in the {c} category"),
            Act::Importance(l) => format!("mark it as {} importance", level_name(*l)),
            Act::Stop => "stop processing more rules".into(),
        }
    }
}

/// A rule's name as it is kept: trimmed, of one line, not empty and not too long.
pub fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= MAX_NAME && !name.chars().any(char::is_control)).then(|| name.to_string())
}

fn list_of<T>(v: &Value, key: &str, one: impl Fn(&Value) -> Result<T, String>) -> Result<Vec<T>, String> {
    let Some(list) = v.get(key).filter(|l| !l.is_null()) else { return Ok(Vec::new()) };
    let list = list.as_array().ok_or_else(|| format!("\"{key}\" is a list"))?;
    if list.len() > MAX_PARTS {
        return Err(format!("a rule has no more than {MAX_PARTS} in \"{key}\""));
    }
    list.iter().map(one).collect()
}

impl Rule {
    /// A rule as a command or the settings give it; what makes no sense is an error saying why.
    pub fn of_json(v: &Value) -> Result<Rule, String> {
        let name = v
            .get("name")
            .and_then(Value::as_str)
            .and_then(clean_name)
            .ok_or(format!("a rule's \"name\" is one line of 1 to {MAX_NAME} characters"))?;
        let account = match v.get("account") {
            None | Some(Value::Null) => None,
            Some(Value::String(a)) if a.is_empty() => None,
            Some(Value::String(a)) if a.len() <= 64 => Some(a.clone()),
            Some(_) => return Err("\"account\" is an account's id".into()),
        };
        let then = list_of(v, "then", Act::of_json)?;
        if then.iter().all(|a| *a == Act::Stop) {
            return Err("a rule does something: \"then\" has an action".into());
        }
        Ok(Rule {
            name,
            on: v.get("on").and_then(Value::as_bool).unwrap_or(true),
            account,
            when: list_of(v, "when", Test::of_json)?,
            except: list_of(v, "except", Test::of_json)?,
            then,
        })
    }

    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "on": self.on,
            "account": self.account,
            "when": self.when.iter().map(Test::to_json).collect::<Vec<_>>(),
            "except": self.except.iter().map(Test::to_json).collect::<Vec<_>>(),
            "then": self.then.iter().map(Act::to_json).collect::<Vec<_>>(),
        })
    }

    /// What the rule does, in words, as the Rules and Alerts window describes it.
    pub fn words(&self, account: Option<&str>) -> String {
        let mut out = String::from("Apply this rule after the message arrives");
        if let Some(a) = account {
            out.push_str(&format!(" through the {a} account"));
        }
        for t in &self.when {
            out.push_str(&format!(", {}", t.words()));
        }
        let acts: Vec<String> = self.then.iter().map(Act::words).collect();
        out.push_str(&format!(": {}", acts.join(", ")));
        if !self.except.is_empty() {
            let except: Vec<String> = self.except.iter().map(Test::words).collect();
            out.push_str(&format!("; except {}", except.join(" or ")));
        }
        out
    }
}

/// The rules as the settings keep them.
pub fn to_json(rules: &[Rule]) -> Value {
    Value::Array(rules.iter().map(Rule::to_json).collect())
}

/// The rules the settings keep, and how many made no sense and were left out.
pub fn of_json(v: &Value) -> (Vec<Rule>, u32) {
    let mut ignored = 0u32;
    let mut out = Vec::new();
    for r in v.as_array().into_iter().flatten() {
        match Rule::of_json(r) {
            Ok(rule) if out.len() < MAX_RULES => out.push(rule),
            _ => ignored = ignored.saturating_add(1),
        }
    }
    (out, ignored)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(from: &str, to: &[&str], subject: &str, body: &str) -> Item {
        let mut m = Item::new_mail();
        m.props.set_str(pid::SENDER_NAME, from);
        m.props.set_str(pid::SENDER_EMAIL_ADDRESS, from);
        m.props.set_str(pid::SUBJECT, subject);
        m.set_body_text(body);
        for t in to {
            m.add_recipient(RECIP_TO, "", t);
        }
        m
    }

    fn rule(json: Value) -> Rule {
        Rule::of_json(&json).unwrap()
    }

    #[test]
    fn rules_are_tried_in_turn_until_one_stops() {
        let me = vec!["pat@example.test".to_string()];
        let rules = vec![
            rule(
                json!({ "name": "News", "when": [{ "test": "from", "words": ["news@"] }], "then": [{ "do": "move", "folder": "Inbox/News" }, { "do": "markRead" }] }),
            ),
            rule(
                json!({ "name": "Copy", "when": [{ "test": "subjectOrBody", "words": ["Launch"] }], "then": [{ "do": "move", "folder": "Launch" }, { "do": "stop" }] }),
            ),
            rule(json!({ "name": "Never", "then": [{ "do": "delete" }] })),
        ];
        let m = message("news@example.test", &["pat@example.test"], "Launch day", "");
        let p = plan(&rules, &m, None, &me);
        assert_eq!(p.rules, ["News", "Copy"]);
        assert_eq!((p.move_to.as_deref(), p.copies.as_slice(), p.read, p.delete), (Some("Inbox/News"), &["Launch".to_string()][..], true, false));
        // The last takes everything else.
        let other = plan(&rules, &message("kim@example.test", &["x@example.test"], "Hi", ""), None, &me);
        assert_eq!((other.rules.as_slice(), other.delete), (&["Never".to_string()][..], true));
        // An exception, a rule turned off, and one for another account.
        let mut rules = rules;
        rules[0].except = vec![Test::OnlyToMe];
        rules[1].on = false;
        rules[2].account = Some("a2".into());
        assert_eq!(plan(&rules, &m, Some("a1"), &me), Plan::default());
        assert_eq!(plan(&rules, &m, Some("a2"), &me).rules, ["Never"]);
    }

    #[test]
    fn rules_kept_and_read_back_and_hostile_ones_left_out() {
        let r = rule(json!({ "name": " Big ", "account": "a1", "when": [{ "test": "size", "min": 100 }, { "test": "importance", "level": "HIGH" }],
            "except": [{ "test": "ccMe" }], "then": [{ "do": "category", "name": "Red" }, { "do": "flag" }] }));
        assert_eq!(r.name, "Big");
        assert_eq!(Rule::of_json(&r.to_json()).unwrap(), r);
        assert!(r.words(Some("Work")).contains("of 100 KB or more, marked as high importance: put it in the Red category"));
        let (back, ignored) = of_json(
            &json!([r.to_json(), { "name": "x" }, { "name": "", "then": [{ "do": "delete" }] }, 7, { "name": "y", "then": [{ "do": "stop" }] },
            { "name": "z", "when": [{ "test": "from", "words": [] }], "then": [{ "do": "delete" }] }, { "name": "w", "then": [{ "do": "move", "folder": "" }] }]),
        );
        assert_eq!((back, ignored), (vec![r], 6));
        let many: Vec<Value> = (0..MAX_RULES + 5).map(|n| json!({ "name": format!("r{n}"), "then": [{ "do": "markRead" }] })).collect();
        assert_eq!(of_json(&Value::Array(many)).0.len(), MAX_RULES);
    }
}
