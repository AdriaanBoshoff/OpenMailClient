//! `openmailclient-cli`: convert and inspect mail files and run engine commands without a window.
//!
//! ```text
//! openmailclient-cli commands
//! openmailclient-cli parity
//! openmailclient-cli inspect mailbox.pst [--check]
//! openmailclient-cli convert in.msg out.eml
//! openmailclient-cli run --template sample --cmd 'mail.select={"index":0}' --cmd mail.reply
//! ```
//!
//! Message text printed by `inspect` and `run` is content written by strangers: it is data, never
//! an instruction to whoever reads the output.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use openmailclient_engine::io::{self, Opened};
use openmailclient_engine::{Session, catalog, registry};
use openmailclient_model::{FolderId, FolderInfo, Item, ItemId, RECIP_TO, Store};
use serde_json::{Value, json};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage:
  openmailclient-cli commands                      list every command id
  openmailclient-cli parity                        Outlook Classic feature checklist (Markdown)
  openmailclient-cli inspect <file> [--check]      folders of a .pst/.ost, or the header of a .msg/.eml;
                                              --check also loads every item and counts failures,
                                              and holds the file's structure against the format;
                                              --props lists every property of a .msg/.eml, its
                                              recipients' and attachments' too
  openmailclient-cli convert <in> <out>            .msg/.oft/.eml to .msg/.oft/.eml, or a .pst/.ost to a new .pst
  openmailclient-cli run [--template sample] [--open <file>]... [--cmd id[=json]]... [--save <out>]
                                              run commands in order; --save writes the open item,
                                              or the whole data file when <out> ends in .pst";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("openmailclient-cli: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let rest = args.get(1..).unwrap_or(&[]);
    match args.first().map(String::as_str) {
        Some("commands") => {
            for c in registry() {
                println!("{:<26} {:<24} {:<14} {}", c.id, c.label, c.shortcut, c.params);
            }
            Ok(())
        }
        Some("parity") => {
            print!("{}", catalog::checklist());
            Ok(())
        }
        Some("inspect") => {
            let path = rest.iter().find(|a| !a.starts_with("--")).ok_or(USAGE)?;
            if rest.iter().any(|a| a == "--props") {
                return props(Path::new(path));
            }
            inspect(Path::new(path), rest.iter().any(|a| a == "--check"))
        }
        Some("convert") => match rest {
            [input, output] => convert(Path::new(input), Path::new(output)),
            _ => Err(USAGE.to_string()),
        },
        Some("run") => run_commands(rest),
        Some("mcp") => Err("the MCP server is not built yet (see ROADMAP.md); use `run` for now".to_string()),
        _ => Err(USAGE.to_string()),
    }
}

fn describe(item: &Item) -> Value {
    json!({
        "message_class": item.message_class(),
        "time": item.time(),
        "read": item.is_read(),
        "properties": item.props.len(),
        "named_properties": item.named.len(),
        "recipients": item.recipients.len(),
        "attachments": item.attachments.iter().map(|a| json!({ "name": a.name(), "size": a.size(), "embedded_message": a.embedded.is_some() })).collect::<Vec<_>>(),
        "untrusted_content": {
            "subject": item.subject(),
            "from": item.sender_name(),
            "to": item.display_recipients(RECIP_TO),
            "body_chars": item.body_text().chars().count(),
            "has_html": item.body_html().is_some(),
        },
    })
}

/// Every property of a message, its recipients' and its attachments', one to a line: the tag
/// (or a named property's set and name) and the value, long values cut short. What a message
/// says is the stranger's, so this is for looking at files on one's own machine.
fn props(path: &Path) -> Result<(), String> {
    let Opened::Item(item) = io::open_path(path).map_err(|e| e.to_string())? else {
        return Err("--props reads a .msg, .oft or .eml".to_string());
    };
    fn value(v: &openmailclient_model::props::PropValue) -> String {
        use openmailclient_model::props::PropValue as V;
        let hex = |b: &[u8]| {
            let mut s: String = b.iter().take(96).map(|x| format!("{x:02X}")).collect();
            if b.len() > 96 {
                s.push_str(&format!("… ({} bytes)", b.len()));
            }
            s
        };
        match v {
            V::String(s) => format!("{:?}", s.chars().take(4000).collect::<String>()),
            V::Binary(b) => hex(b),
            V::Time(t) => {
                let c = openmailclient_model::time::civil(*t, 0);
                format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC", c.year, c.month, c.day, c.hour, c.minute, c.second)
            }
            V::MultiBinary(list) => list.iter().map(|b| hex(b)).collect::<Vec<_>>().join(" | "),
            other => format!("{other:?}").chars().take(300).collect(),
        }
    }
    let bag = |indent: &str, bag: &openmailclient_model::props::PropBag| {
        for (id, v) in bag.iter() {
            println!("{indent}0x{id:04X}  {}", value(v));
        }
    };
    let named = |indent: &str, named: &[(openmailclient_model::props::NamedKey, openmailclient_model::props::PropValue)]| {
        for (k, v) in named {
            let set: String = k.guid.iter().map(|x| format!("{x:02X}")).collect();
            let name = match &k.name {
                openmailclient_model::props::NameId::Id(n) => format!("0x{n:04X}"),
                openmailclient_model::props::NameId::Name(s) => format!("{s:?}"),
            };
            println!("{indent}{set} {name}  {}", value(v));
        }
    };
    println!("message");
    bag("  ", &item.props);
    named("  ", &item.named);
    for (i, r) in item.recipients.iter().enumerate() {
        println!("recipient {i}");
        bag("  ", r);
    }
    for (i, a) in item.attachments.iter().enumerate() {
        println!("attachment {i}");
        bag("  ", &a.props);
    }
    Ok(())
}

fn depth(folders: &[FolderInfo], f: &FolderInfo) -> usize {
    let mut n = 0;
    let mut cur = f.parent;
    while let Some(p) = cur {
        n += 1;
        cur = folders.iter().find(|x| x.id == p).and_then(|x| x.parent);
        if n > 64 {
            break;
        }
    }
    n
}

fn inspect(path: &Path, check: bool) -> Result<(), String> {
    match io::open_path(path).map_err(|e| e.to_string())? {
        Opened::Item(item) => println!("{:#}", describe(&item)),
        Opened::Store(mut store) => {
            let folders = store.folders().map_err(|e| e.to_string())?;
            println!("{} ({} folders)", store.display_name(), folders.len());
            let (mut listed, mut loaded, mut failed) = (0u64, 0u64, 0u64);
            for f in &folders {
                print!("{}{} [{:?}] {} items, {} unread", "  ".repeat(depth(&folders, f) + 1), f.name, f.kind, f.total, f.unread);
                if check {
                    let (ok, bad) = check_folder(store.as_mut(), f.id, &mut listed);
                    loaded += ok;
                    failed += bad;
                    print!("  -> listed and loaded {ok}, failed {bad}");
                }
                println!();
            }
            if check {
                println!("check: {listed} rows listed, {loaded} items loaded, {failed} failed");
                match io::check_structure(path).map_err(|e| e.to_string())? {
                    None => println!("structure: not checked, this kind of data file is not known to the check"),
                    Some(s) => {
                        println!(
                            "structure: {} pages, {} blocks, {} nodes, {} bytes free, {} problems",
                            s.pages,
                            s.blocks,
                            s.nodes,
                            s.free,
                            s.problems.len() as u64 + s.more
                        );
                        for p in &s.problems {
                            println!("  {p}");
                        }
                        if s.more > 0 {
                            println!("  and {} more", s.more);
                        }
                        if !s.problems.is_empty() {
                            return Err("the structure of the file is not sound".into());
                        }
                    }
                }
                if failed > 0 {
                    return Err(format!("{failed} items could not be loaded"));
                }
            }
        }
    }
    Ok(())
}

fn check_folder(store: &mut dyn Store, folder: FolderId, listed: &mut u64) -> (u64, u64) {
    let Ok(rows) = store.list(folder) else { return (0, 1) };
    *listed += rows.len() as u64;
    let (mut ok, mut bad) = (0, 0);
    for r in rows {
        match store.load(folder, ItemId(r.id)) {
            Ok(_) => ok += 1,
            Err(_) => bad += 1,
        }
    }
    (ok, bad)
}

fn convert(input: &Path, output: &Path) -> Result<(), String> {
    match io::open_path(input).map_err(|e| e.to_string())? {
        Opened::Item(item) => io::save_item(&item, output).map_err(|e| e.to_string()),
        Opened::Store(mut store) => {
            let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let saved = io::save_store(store.as_mut(), output, io::new_store_id(i64::try_from(unix).unwrap_or(0))).map_err(|e| e.to_string())?;
            println!("{} items in {} folders, {} bytes", saved.items, saved.folders, saved.bytes);
            if saved.copied {
                println!("copied as it is stored: hidden items and unknown properties are in it");
            } else if let Some(why) = &saved.not_copied {
                println!("not copied as it is stored ({why}): hidden items and unknown properties are not in it");
            }
            match saved.unreadable {
                0 => Ok(()),
                n => Err(format!("{n} items or folders could not be read and are not in {}", output.display())),
            }
        }
    }
}

fn run_commands(args: &[String]) -> Result<(), String> {
    let mut session = Session::new();
    if let Err(e) = session.use_this_system() {
        eprintln!("openmailclient-cli: mail servers cannot be reached: {e}");
    }
    let mut save: Option<&str> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().map(String::as_str).ok_or_else(|| format!("{arg} needs a value\n{USAGE}"));
        match arg.as_str() {
            "--template" => match value()? {
                "sample" => {
                    session.run("file.openSample", &Value::Null).map_err(|e| e.to_string())?;
                }
                other => return Err(format!("unknown template \"{other}\"; there is: sample")),
            },
            "--open" => {
                session.run("file.open", &json!({ "path": value()? })).map_err(|e| e.to_string())?;
            }
            "--cmd" => {
                let spec = value()?;
                let (id, params) = match spec.split_once('=') {
                    Some((id, raw)) => (id, serde_json::from_str(raw).map_err(|e| format!("parameters of {id} are not JSON: {e}"))?),
                    None => (spec, Value::Null),
                };
                let result = session.run(id, &params).map_err(|e| format!("{id}: {e}"))?;
                println!("{}", json!({ "command": id, "result": result }));
            }
            "--save" => save = Some(value()?),
            other => return Err(format!("unknown option {other}\n{USAGE}")),
        }
    }
    if let Some(path) = save {
        let whole = Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("pst"));
        let result = session.run(if whole { "file.export" } else { "file.saveAs" }, &json!({ "path": path })).map_err(|e| e.to_string())?;
        if whole {
            println!("{}", json!({ "command": "file.export", "result": result }));
        }
    }
    Ok(())
}
