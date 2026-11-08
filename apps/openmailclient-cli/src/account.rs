//! `openmailclient-cli account …`: mail accounts looked after from the host's console (M7.5),
//! for a server with no window: an account's password set over SSH, asked for twice and never on
//! the command line, kept where `openmailclient serve` finds it (the system's credential store,
//! or sealed beside the settings where there is none). Nothing here reaches a mail server.

use openmailclient_engine::{Session, places};
use openmailclient_net::sealed::Choice;
use serde_json::{Value, json};
use std::io::{BufRead, IsTerminal};
use std::path::{Path, PathBuf};

pub const USAGE: &str = "  openmailclient-cli account list [--dir <folder>]
                                              the accounts in the settings, and whether each has
                                              its password kept
  openmailclient-cli account password <account> [--forget] [--passwords system|sealed|either]
                          [--dir <folder>]
                                              set an account's password (named by its address,
                                              name or id), asked for twice and never on the
                                              command line, or read from standard input when that
                                              is not a console; --forget forgets it";

/// Run `account …` with `rest` after `account`; `default_dir` is the settings' folder.
pub fn account(rest: &[String], default_dir: Option<PathBuf>) -> Result<(), String> {
    let mut words = Vec::new();
    let (mut dir, mut forget, mut passwords) = (None, false, Choice::Either);
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--dir" => dir = Some(PathBuf::from(value()?)),
            "--forget" => forget = true,
            "--passwords" => {
                let v = value()?;
                passwords = Choice::parse(&v).ok_or_else(|| format!("--passwords {v}: say system, sealed or either"))?;
            }
            other if other.starts_with("--") => return Err(format!("{other}: not an option of `account`\n{USAGE}")),
            other => words.push(other),
        }
    }
    let dir = dir.or(default_dir).ok_or("there is no folder for settings here; name one with --dir")?;
    match words.as_slice() {
        ["list"] => list(&dir, passwords),
        ["password", which] => password(&dir, which, forget, passwords),
        _ => Err(format!("usage:\n{USAGE}")),
    }
}

/// The settings' accounts in a session that opens no data file.
fn session(dir: &Path, passwords: Choice) -> Result<(Session, String), String> {
    let settings = dir.join("settings.json");
    if !settings.is_file() {
        return Err(format!("{}: there are no settings here yet", settings.display()));
    }
    let mut s = Session::new();
    let vault = places::vault(dir, passwords);
    let says = vault.says();
    s.vault = std::sync::Arc::new(vault);
    s.run("settings.load", &json!({ "path": settings.display().to_string(), "reopen": false })).map_err(|e| e.to_string())?;
    Ok((s, says))
}

fn accounts(s: &mut Session) -> Result<Vec<Value>, String> {
    let listed = s.run("account.list", &Value::Null).map_err(|e| e.to_string())?;
    Ok(listed.get("accounts").and_then(Value::as_array).cloned().unwrap_or_default())
}

fn list(dir: &Path, passwords: Choice) -> Result<(), String> {
    let (mut s, says) = session(dir, passwords)?;
    let all = accounts(&mut s)?;
    if all.is_empty() {
        println!("There is no account yet.");
    }
    let field = |a: &Value, k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    for a in &all {
        let kept = if a.get("hasPassword").and_then(Value::as_bool).unwrap_or(false) { "password kept" } else { "no password kept" };
        println!("{:<36} {:<8} {kept}  (id {})", field(a, "email"), field(a, "kind"), field(a, "id"));
    }
    println!("Passwords are kept {says}.");
    Ok(())
}

/// The password, asked for twice at a console, or read from standard input's first line.
fn ask(account: &str) -> Result<String, String> {
    if !std::io::stdin().is_terminal() {
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).map_err(|e| format!("standard input: {e}"))?;
        return Ok(line.trim_end_matches(['\r', '\n']).to_string());
    }
    loop {
        let first = rpassword::prompt_password(format!("Password for {account}: ")).map_err(|e| format!("the console: {e}"))?;
        if first.is_empty() {
            return Err("no password was given; nothing was changed".to_string());
        }
        let again = rpassword::prompt_password("The same again: ").map_err(|e| format!("the console: {e}"))?;
        if again == first {
            return Ok(first);
        }
        println!("The two were not the same; start again.");
    }
}

fn password(dir: &Path, which: &str, forget: bool, passwords: Choice) -> Result<(), String> {
    let (mut s, says) = session(dir, passwords)?;
    let all = accounts(&mut s)?;
    let field = |a: &Value, k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let found: Vec<&Value> = all
        .iter()
        .filter(|a| field(a, "id") == which || field(a, "email").eq_ignore_ascii_case(which) || field(a, "name").eq_ignore_ascii_case(which))
        .collect();
    let account = match found.as_slice() {
        [one] => *one,
        [] => return Err(format!("no account is {which}; `openmailclient-cli account list` lists them")),
        _ => return Err(format!("more than one account is {which}; name it by its id")),
    };
    let (id, shown) = (field(account, "id"), field(account, "email"));
    let chosen = if forget { String::new() } else { ask(&shown)? };
    if !forget && chosen.is_empty() {
        return Err("no password was given; nothing was changed".to_string());
    }
    s.run("account.setPassword", &json!({ "id": id, "password": chosen })).map_err(|e| e.to_string())?;
    if forget {
        println!("The password of {shown} is forgotten.");
    } else {
        println!("The password of {shown} is kept {says}.");
    }
    Ok(())
}
