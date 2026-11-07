//! `openmailclient-cli web …`: web access looked after from the host's console (M7.2, M7.4), so
//! a forgotten password is set again over SSH and an owner locked out of their own address can
//! let it back in. Nothing here touches the network.

use openmailclient_serve::guard::{self, Guard, Net};
use openmailclient_serve::password;
use std::io::{BufRead, IsTerminal};
use std::path::{Path, PathBuf};

pub const USAGE: &str = "  openmailclient-cli web password [--dir <folder>]
                                              set the web password, or set it again when it is
                                              forgotten (asked for twice, never on the command line;
                                              read from standard input when that is not a console);
                                              every browser signed in with the old one is signed out
  openmailclient-cli web status [--dir <folder>]  whether a password is set, and the bans
  openmailclient-cli web bans [--dir <folder>]    the addresses banned for failing to sign in
  openmailclient-cli web unban <address> [--dir <folder>]
                                              lift a ban (an IPv6 address lifts its whole /64)
  openmailclient-cli web never-ban [<network>] [--remove] [--dir <folder>]
                                              list, add or remove networks never banned, such as
                                              192.168.1.0/24; they are still slowed";

/// Run `web …` with `rest` after `web`; `default_dir` is the settings' folder.
pub fn web(rest: &[String], default_dir: Option<PathBuf>) -> Result<(), String> {
    let mut words = Vec::new();
    let mut dir = None;
    let mut remove = false;
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dir" => dir = Some(PathBuf::from(args.next().ok_or("--dir needs a folder")?)),
            "--remove" => remove = true,
            other if other.starts_with("--") => return Err(format!("{other}: not an option of `web`\n{USAGE}")),
            other => words.push(other),
        }
    }
    let dir = dir.or(default_dir).ok_or("there is no folder for settings here; name one with --dir")?;
    match words.as_slice() {
        ["password"] => set_password(&dir),
        ["status"] => status(&dir),
        ["bans"] => bans(&dir),
        ["unban", address] => unban(&dir, address),
        ["never-ban"] if !remove => never_ban_list(&dir),
        ["never-ban", net] => never_ban(&dir, net, remove),
        _ => Err(format!("usage:\n{USAGE}")),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The password, asked for twice at a console, or read from standard input's first line.
fn ask() -> Result<String, String> {
    if !std::io::stdin().is_terminal() {
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).map_err(|e| format!("standard input: {e}"))?;
        return Ok(line.trim_end_matches(['\r', '\n']).to_string());
    }
    println!("The web password needs at least {} characters. A few words of your own are best.", password::MIN_LEN);
    loop {
        let first = rpassword::prompt_password("New web password: ").map_err(|e| format!("the console: {e}"))?;
        if let Some(why) = password::weakness(&first) {
            println!("{why}.");
            continue;
        }
        let again = rpassword::prompt_password("The same again: ").map_err(|e| format!("the console: {e}"))?;
        if again == first {
            return Ok(first);
        }
        println!("The two were not the same; start again.");
    }
}

fn set_password(dir: &Path) -> Result<(), String> {
    let was = password::load(dir).ok().flatten();
    let chosen = ask()?;
    let kept = password::set(dir, &chosen, now_ms())?;
    if was.is_some() {
        println!("The web password is set again (setting {}). Every browser signed in with the old one is signed out.", kept.generation);
    } else {
        println!("The web password is set. Browsers can sign in with it once web access is turned on.");
    }
    println!("Only its Argon2id hash is kept, in {}.", openmailclient_serve::password_file(dir).display());
    Ok(())
}

/// `ms` from now said in words: "3 minutes", "2 hours", "5 days".
fn span(ms: u64) -> String {
    if ms < 60_000 {
        return "under a minute".to_string();
    }
    let minutes = ms.div_ceil(60_000);
    let (n, unit) = match minutes {
        0..=89 => (minutes, "minute"),
        90..=2879 => (minutes.div_ceil(60), "hour"),
        _ => (minutes / 1440, "day"),
    };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

fn status(dir: &Path) -> Result<(), String> {
    let now = now_ms();
    match password::load(dir) {
        Ok(Some(kept)) => println!("Web password: set {} ago (setting {}).", span(now.saturating_sub(kept.set_ms)), kept.generation),
        Ok(None) => println!("Web password: not set. Web access cannot be turned on until `openmailclient-cli web password` sets it."),
        Err(e) => println!("Web password: cannot be read ({e}). Set it again with `openmailclient-cli web password`."),
    }
    let g = Guard::load(dir)?;
    println!("Banned addresses: {}.", g.bans(now).len());
    let spared: Vec<String> = g.spared().iter().map(Net::to_string).collect();
    println!("Never banned: {}.", if spared.is_empty() { "none".to_string() } else { spared.join(", ") });
    println!("Serving: not yet; the server comes with M7.1.");
    Ok(())
}

fn bans(dir: &Path) -> Result<(), String> {
    let now = now_ms();
    let g = Guard::load(dir)?;
    let list = g.bans(now);
    if list.is_empty() {
        println!("No address is banned.");
    }
    for (addr, ban) in list {
        let until = match ban.until_ms {
            Some(u) => format!("for {} more", span(u.saturating_sub(now))),
            None => "until lifted".to_string(),
        };
        let level = if ban.level == 0 { "by hand".to_string() } else { format!("ban {} in a row", ban.level) };
        println!("{:<44} {until}, since {} ago, {level}", guard::shown(addr), span(now.saturating_sub(ban.since_ms)));
    }
    Ok(())
}

fn unban(dir: &Path, address: &str) -> Result<(), String> {
    let addr = guard::parse_address(address).ok_or_else(|| format!("{address}: not an IPv4 or IPv6 address"))?;
    let mut g = Guard::load(dir)?;
    if g.unban(addr) {
        g.save(dir)?;
        println!("{} may sign in again.", guard::shown(addr));
    } else {
        println!("{} was not banned.", guard::shown(addr));
    }
    Ok(())
}

fn never_ban_list(dir: &Path) -> Result<(), String> {
    let g = Guard::load(dir)?;
    if g.spared().is_empty() {
        println!("No network is kept from bans.");
    }
    for net in g.spared() {
        println!("{net}");
    }
    Ok(())
}

fn never_ban(dir: &Path, text: &str, remove: bool) -> Result<(), String> {
    let net = Net::parse(text).ok_or_else(|| format!("{text}: not an address or a network such as 192.168.1.0/24"))?;
    let mut g = Guard::load(dir)?;
    let mut nets: Vec<Net> = g.spared().to_vec();
    if remove {
        let before = nets.len();
        nets.retain(|n| *n != net);
        if nets.len() == before {
            println!("{net} was not kept from bans.");
            return Ok(());
        }
    } else if nets.contains(&net) {
        println!("{net} is already kept from bans.");
        return Ok(());
    } else if nets.len() >= guard::MAX_NEVER {
        return Err(format!("at most {} networks can be kept from bans", guard::MAX_NEVER));
    } else {
        if net.prefix == 0 {
            return Err(format!("{net} is every address; keeping it from bans would turn bans off"));
        }
        nets.push(net);
    }
    g.never_ban(nets);
    g.save(dir)?;
    println!("{net} {}.", if remove { "may be banned again" } else { "is kept from bans; it is still slowed" });
    Ok(())
}
