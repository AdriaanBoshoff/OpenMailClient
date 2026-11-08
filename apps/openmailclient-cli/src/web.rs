//! `openmailclient-cli web …`: web access served and looked after from the host's console (M7.1,
//! M7.2, M7.4), so a forgotten password is set again over SSH and an owner locked out of their own
//! address can let it back in. Only `web serve` listens; nothing here reaches out.

use openmailclient_engine::Session;
use openmailclient_net::sealed::Choice;
use openmailclient_serve::guard::{self, Guard, Net};
use openmailclient_serve::{host, password, server, tls};
use serde_json::{Value, json};
use std::io::{BufRead, IsTerminal};
use std::path::{Path, PathBuf};

pub const USAGE: &str = "  openmailclient-cli web serve [--listen <address:port>] [--name <host>]... [--web-dir <folder>]
                          [--new-certificate] [--dir <folder>]
                          (--profile [--passwords system|sealed|either]
                           | [--template sample] [--open <file>]... [--reach-out])
                                              serve a mailbox to browsers over HTTPS, on
                                              127.0.0.1:8443 unless another address is named
                                              (0.0.0.0:8443 for every network this machine is on);
                                              --name adds a name browsers reach it by; --profile
                                              serves the owner's own, as `openmailclient serve`
  openmailclient-cli web password [--dir <folder>]
                                              set the web password, or set it again when it is
                                              forgotten (asked for twice, never on the command line;
                                              read from standard input when that is not a console);
                                              every browser signed in with the old one is signed out
  openmailclient-cli web status [--dir <folder>]  whether a password is set, the bans, whether it
                                              is serving and the certificate's fingerprint
  openmailclient-cli web certificate [--dir <folder>]
                                              the certificate as PEM, to trust on a phone by hand
  openmailclient-cli web log [--dir <folder>]     the last sign-ins and failures
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
    let mut serve = Serve::default();
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--dir" => dir = Some(PathBuf::from(value()?)),
            "--remove" => remove = true,
            "--listen" => serve.listen = Some(value()?),
            "--name" => serve.names.push(value()?),
            "--template" => serve.template = Some(value()?),
            "--open" => serve.open.push(value()?),
            "--web-dir" => serve.web_dir = Some(PathBuf::from(value()?)),
            "--reach-out" => serve.reach_out = true,
            "--new-certificate" => serve.new_certificate = true,
            "--profile" => serve.profile = true,
            "--passwords" => {
                let v = value()?;
                serve.passwords = Choice::parse(&v).ok_or_else(|| format!("--passwords {v}: say system, sealed or either"))?;
            }
            other if other.starts_with("--") => return Err(format!("{other}: not an option of `web`\n{USAGE}")),
            other => words.push(other),
        }
    }
    let dir = dir.or(default_dir).ok_or("there is no folder for settings here; name one with --dir")?;
    match words.as_slice() {
        ["serve"] => run_server(dir, serve),
        ["password"] => set_password(&dir),
        ["status"] => status(&dir),
        ["certificate"] => certificate(&dir),
        ["log"] => log(&dir),
        ["bans"] => bans(&dir),
        ["unban", address] => unban(&dir, address),
        ["never-ban"] if !remove => never_ban_list(&dir),
        ["never-ban", net] => never_ban(&dir, net, remove),
        _ => Err(format!("usage:\n{USAGE}")),
    }
}

fn now_ms() -> u64 {
    openmailclient_serve::now_ms()
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
        println!("The web password is set. Start web access with `openmailclient-cli web serve`.");
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
    match tls::load(dir) {
        Ok(Some(kept)) => {
            println!("Certificate: made {} ago for {}.", span(now.saturating_sub(kept.made_ms)), kept.names.join(", "));
            println!("  SHA-256 fingerprint: {}", kept.fingerprint());
        }
        Ok(None) => println!("Certificate: none yet; one is made the first time `openmailclient-cli web serve` runs."),
        Err(e) => println!("Certificate: {e}"),
    }
    println!("{}", serving(dir));
    Ok(())
}

/// Whether a server started from this folder is listening now.
fn serving(dir: &Path) -> String {
    let no = || "Serving: no.".to_string();
    let Ok(text) = std::fs::read_to_string(server::serving_file(dir)) else { return no() };
    let Ok(value) = serde_json::from_str::<Value>(&text) else { return no() };
    let Some(listen) = value.get("listen").and_then(Value::as_str).and_then(|l| l.parse::<std::net::SocketAddr>().ok()) else { return no() };
    // Asked of this machine only: the address it listens on, or this machine's own for every address.
    let mut ask = listen;
    if ask.ip().is_unspecified() {
        ask.set_ip(if ask.is_ipv4() { std::net::Ipv4Addr::LOCALHOST.into() } else { std::net::Ipv6Addr::LOCALHOST.into() });
    }
    if std::net::TcpStream::connect_timeout(&ask, std::time::Duration::from_secs(1)).is_err() {
        return format!("Serving: no (it last listened on {listen} and has stopped).");
    }
    let names: Vec<&str> = value.get("names").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let port = listen.port();
    let urls: Vec<String> =
        names.iter().map(|n| if n.contains(':') { format!("https://[{n}]:{port}/") } else { format!("https://{n}:{port}/") }).collect();
    let since = now_ms().saturating_sub(value.get("started").and_then(Value::as_u64).unwrap_or(0));
    format!("Serving: yes, on {listen}, for {}; reach it at {}", span(since), urls.join("  "))
}

fn certificate(dir: &Path) -> Result<(), String> {
    let kept = tls::load(dir)?.ok_or("there is no certificate yet; one is made the first time `openmailclient-cli web serve` runs")?;
    print!("{}", kept.pem());
    eprintln!("SHA-256 fingerprint: {}", kept.fingerprint());
    Ok(())
}

fn log(dir: &Path) -> Result<(), String> {
    let path = server::log_file(dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("No one has tried to sign in yet.");
            return Ok(());
        }
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let lines: Vec<&str> = text.lines().collect();
    for line in lines.iter().skip(lines.len().saturating_sub(40)) {
        println!("{line}");
    }
    Ok(())
}

#[derive(Default)]
struct Serve {
    listen: Option<String>,
    names: Vec<String>,
    template: Option<String>,
    open: Vec<String>,
    web_dir: Option<PathBuf>,
    reach_out: bool,
    new_certificate: bool,
    profile: bool,
    passwords: Choice,
}

fn run_server(dir: PathBuf, serve: Serve) -> Result<(), String> {
    if let Some(t) = &serve.template
        && t != "sample"
    {
        return Err(format!("unknown template \"{t}\"; there is: sample"));
    }
    if serve.profile && (serve.template.is_some() || !serve.open.is_empty()) {
        return Err("--profile serves the owner's own mailbox; it is not given with --template or --open".to_string());
    }
    let plan = host::Plan { listen: serve.listen, names: serve.names, web_dir: serve.web_dir, new_certificate: serve.new_certificate };
    if serve.profile {
        return host::serve_profile(&dir, plan, serve.passwords);
    }
    let (template, open, reach_out) = (serve.template, serve.open, serve.reach_out);
    let make = move || {
        let mut session = Session::new();
        // Without --reach-out nothing could use the network, so none is given.
        if reach_out && let Err(e) = session.use_this_system() {
            eprintln!("openmailclient-cli: mail servers cannot be reached: {e}");
        }
        if template.is_some() {
            session.run("file.openSample", &Value::Null).map_err(|e| e.to_string())?;
        }
        for path in open {
            session.run("file.open", &json!({ "path": path })).map_err(|e| e.to_string())?;
        }
        Ok(openmailclient_mcp::Headless { session, reach_out })
    };
    host::run(&dir, plan, make, &[])
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
