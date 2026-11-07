//! The command registry. Every user-visible action is one [`CommandSpec`]; the ribbon, shortcuts,
//! command search, the CLI and agents all dispatch by id.

mod account;
mod arrival;
pub(crate) mod caldav;
pub mod calendar;
mod calendar_file;
pub(crate) mod compose;
pub(crate) mod file;
mod folder;
pub(crate) mod format;
pub mod free_busy;
pub(crate) mod imap_sync;
mod import;
mod link;
mod mail;
mod meeting;
mod merge;
pub(crate) mod names;
pub mod people;
mod picture;
mod progress;
mod reading;
pub(crate) mod receive;
mod reminders;
pub(crate) mod rules;
mod send;
pub(crate) mod settings;
mod signatures;
mod task_request;
pub mod tasks;
pub mod todo_bar;
mod update;
mod view;

use crate::{CmdResult, EngineError, Session};
pub(crate) use account::Reach;
pub(crate) use receive::RECEIVING;
pub(crate) use send::{SENDING, clock, held_in_outbox};
use serde_json::Value;
use std::sync::OnceLock;

pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Ribbon tab and group this command lives in; empty for commands with no button.
    pub tab: &'static str,
    pub group: &'static str,
    /// Outlook Classic's default shortcut, as shown in its tooltips.
    pub shortcut: &'static str,
    /// One line describing the JSON parameters.
    pub params: &'static str,
    pub enabled: fn(&Session) -> bool,
    pub run: fn(&mut Session, &Value) -> CmdResult,
}

/// Commands that reach beyond this machine: they send mail or answers, talk to a server, change
/// what an account is, or bring down and run an update. Whatever drives the window from outside
/// it (the control channel, the MCP server) runs them only where the owner has allowed it.
pub const REACHES_OUT: &[&str] = &[
    "compose.send",
    "mail.sendAll",
    "mail.sendReceive",
    "mail.sendReceiveEvery",
    "mail.newMailAsItComes",
    "mail.downloadPictures",
    "account.add",
    "account.edit",
    "account.remove",
    "account.test",
    "account.find",
    "account.receive",
    "account.trustCertificate",
    "account.setPassword",
    "calendar.sendInvitations",
    "calendar.cancelMeeting",
    "calendar.respond",
    "calendar.sync",
    "people.sync",
    "people.mailMerge",
    "tasks.respond",
    "tasks.sendStatusReport",
    "tasks.sync",
    "help.checkForUpdates",
    "help.downloadUpdate",
    "help.installUpdate",
];

/// Commands that carry a secret in their parameters: never run from outside the window, so a
/// password never passes through a channel, a log or an agent.
pub const CARRIES_SECRET: &[&str] = &["account.setPassword"];

/// Whether a command reaches beyond this machine ([`REACHES_OUT`]).
pub fn reaches_out(id: &str) -> bool {
    REACHES_OUT.contains(&id)
}

pub fn registry() -> &'static [CommandSpec] {
    static REG: OnceLock<Vec<CommandSpec>> = OnceLock::new();
    REG.get_or_init(|| {
        let mut v = Vec::new();
        v.extend(file::commands());
        v.extend(import::commands());
        v.extend(mail::commands());
        v.extend(link::commands());
        v.extend(picture::commands());
        v.extend(compose::commands());
        v.extend(format::commands());
        v.extend(signatures::commands());
        v.extend(rules::commands());
        v.extend(folder::commands());
        v.extend(view::commands());
        v.extend(settings::commands());
        v.extend(account::commands());
        v.extend(send::commands());
        v.extend(receive::commands());
        v.extend(arrival::commands());
        v.extend(progress::commands());
        v.extend(calendar::commands());
        v.extend(people::commands());
        v.extend(reading::commands());
        v.extend(names::commands());
        v.extend(merge::commands());
        v.extend(tasks::commands());
        v.extend(task_request::commands());
        v.extend(update::commands());
        v.extend(calendar_file::commands());
        v.extend(meeting::commands());
        v.extend(free_busy::commands());
        v.extend(caldav::commands());
        v.extend(reminders::commands());
        v.extend(todo_bar::commands());
        v
    })
}

pub(crate) fn always(_: &Session) -> bool {
    true
}

pub(crate) fn has_selection(s: &Session) -> bool {
    s.selected.is_some() && s.open.is_some()
}

pub(crate) fn has_selection_writable(s: &Session) -> bool {
    has_selection(s) && s.current_writable()
}

pub(crate) fn has_folder(s: &Session) -> bool {
    s.current.is_some()
}

pub(crate) fn has_folder_writable(s: &Session) -> bool {
    s.current.is_some() && s.current_writable()
}

pub(crate) fn p_str<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Value::as_str)
}

pub(crate) fn need_str<'a>(p: &'a Value, key: &str) -> Result<&'a str, EngineError> {
    p_str(p, key).ok_or_else(|| EngineError::BadParams(format!("\"{key}\" (text) is required")))
}

pub(crate) fn p_u64(p: &Value, key: &str) -> Option<u64> {
    p.get(key).and_then(Value::as_u64)
}

pub(crate) fn p_bool(p: &Value, key: &str) -> Option<bool> {
    p.get(key).and_then(Value::as_bool)
}

pub(crate) fn ok() -> CmdResult {
    Ok(Value::Null)
}
