//! Headless window screenshots: draws the real UI off-screen (no window, no focus stealing) and
//! writes PNGs.
//!
//! ```text
//! cargo run --release -p openmailclient-ui-egui --example ui_shot -- script.jsonl
//! ```
//!
//! The script is JSON lines, run in order:
//!
//! ```text
//! {"size": [1280, 800]}                      window size in points (before the first shot)
//! {"sample": true}                           open the sample mailbox
//! {"command": "mail.select", "params": {"index": 0}}
//!                                            a param "$id" stands for the "id" the last
//!                                            command gave back
//! {"shot": "plan/shots/inbox.png"}
//! {"click": [640, 18]}                       press and let go at a place in the window
//! {"type": "words"}                          type into what has the keyboard
//! {"fake": {"ownCertificate": "AB:CD:…"}}    reach the in-process fake server (with a
//!                                            certificate of its own) instead of none
//! {"outlook": {"name": "Outlook", "accounts": [{"Account Name": "Work", "IMAP Port": 993, …}],
//!   "sections": [{"name": "1111…", "001f6700": "C:\…\a.pst"}], "importTo": "plan/try"}}
//!                                            read this profile, written by hand, as Outlook's
//!                                            settings (numbers as numbers, "hex:…" as bytes,
//!                                            the rest as words)
//! {"arrive": "raw RFC 5322 text"}           put a message in the fake server's Inbox
//! {"settle": true}                           draw until the jobs under way have ended
//! {"region": ["yyyy/MM/dd", "dddd, d MMMM yyyy", "HH:mm", 1]}
//!                                            write dates as a region does: short and long
//!                                            date, time, and the first day of the week
//!                                            (0 Sunday)
//! {"now": [2026, 10, 16, 12, 0]}             the time it is, in UTC
//! {"printers": ["Hall Laser", "Desk Inkjet"]}
//!                                            printers made up, the first the default, that
//!                                            keep what they are sent: nothing is printed
//! ```
//!
//! Without a script it opens the sample mailbox and writes `plan/shots/ui_shot.png`.

use egui_kittest::Harness;
use openmailclient_engine::Session;
use openmailclient_ui_egui::MailUi;
use serde_json::{Value, json};
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ui_shot: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let steps: Vec<Value> = match std::env::args().nth(1) {
        Some(path) => {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
            text.lines()
                .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
                .map(|l| serde_json::from_str(l).map_err(|e| format!("{path}: {e}: {l}")))
                .collect::<Result<_, _>>()?
        }
        None => vec![
            json!({ "sample": true }),
            json!({ "command": "mail.select", "params": { "index": 0 } }),
            json!({ "shot": "plan/shots/ui_shot.png" }),
        ],
    };
    let size = steps.iter().find_map(|s| s.get("size")).and_then(Value::as_array).map_or((1280.0, 800.0), |a| {
        let at = |i: usize| a.get(i).and_then(Value::as_f64).map_or(800.0, |v| v.clamp(320.0, 4096.0) as f32);
        (at(0), at(1))
    });
    let scale = steps.iter().find_map(|s| s.get("scale")).and_then(Value::as_f64).map_or(1.0, |v| v.clamp(0.5, 3.0) as f32);
    let mut fake_server: Option<openmailclient_net::fake::Fake> = None;
    let mut harness = Harness::builder()
        .with_size(size)
        .with_pixels_per_point(scale)
        .wgpu()
        .build_ui_state(|ui, app: &mut MailUi| app.ui(ui), MailUi::new(Session::new()));
    // A picture is taken with every picture of the message in it.
    harness.state_mut().wait_for_pictures(true);
    harness.run_steps(3);
    // The "id" the last command gave back, put where a param says "$id".
    let mut last_id = Value::Null;
    for step in &steps {
        if step.get("sample").and_then(Value::as_bool) == Some(true) {
            harness.state_mut().execute("file.openSample", &Value::Null)?;
        } else if let Some(id) = step.get("command").and_then(Value::as_str) {
            let mut params = step.get("params").cloned().unwrap_or(Value::Null);
            if let Some(map) = params.as_object_mut() {
                for v in map.values_mut().filter(|v| v.as_str() == Some("$id")) {
                    *v = last_id.clone();
                }
            }
            match harness.state_mut().execute(id, &params) {
                Ok(got) => {
                    if let Some(new) = got.get("id").filter(|v| !v.is_null()) {
                        last_id = new.clone();
                    }
                }
                Err(e) => eprintln!("ui_shot: {id}: {e}"),
            }
        } else if let Some(r) = step.get("region").and_then(Value::as_array) {
            let text = |i: usize| r.get(i).and_then(Value::as_str).unwrap_or("");
            let start = r.get(3).and_then(Value::as_u64).map_or(0, |n| (n % 7) as u32);
            harness.state_mut().session.region = openmailclient_model::time::Region::of(text(0), text(1), text(2), "", "", start);
        } else if let Some(at) = step.get("now").and_then(Value::as_array) {
            let n = |i: usize| at.get(i).and_then(Value::as_u64).unwrap_or(0);
            harness.state_mut().session.now =
                openmailclient_model::time::from_civil(n(0) as i64, n(1) as u32, n(2) as u32, n(3) as u32, n(4) as u32, 0, 0);
        } else if let Some(at) = step.get("click").and_then(Value::as_array) {
            let part = |i: usize| at.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
            let at = egui::pos2(part(0), part(1));
            harness.hover_at(at);
            harness.run_steps(1);
            harness.drag_at(at);
            harness.run_steps(1);
            harness.drop_at(at);
        } else if let Some(names) = step.get("printers").and_then(Value::as_array) {
            let listed = names
                .iter()
                .filter_map(Value::as_str)
                .enumerate()
                .map(|(at, name)| openmailclient_engine::printers::Printer { name: name.to_string(), default: at == 0 })
                .collect();
            let recorder = openmailclient_engine::printers::Recorder { listed, ..Default::default() };
            harness.state_mut().session.spooler = std::sync::Arc::new(std::sync::Arc::new(recorder));
        } else if let Some(fake) = step.get("fake") {
            let server = openmailclient_net::fake::Fake::default();
            server.state().own_certificate = fake.get("ownCertificate").and_then(Value::as_str).and_then(openmailclient_net::Fingerprint::parse);
            harness.state_mut().session.connector = Some(std::sync::Arc::new(server.clone()));
            fake_server = Some(server);
        } else if let Some(profile) = step.get("outlook") {
            let raw = outlook_profile(profile);
            harness.state_mut().session.outlook = Some(std::sync::Arc::new(move || vec![raw.clone()]));
            if let Some(dir) = profile.get("importTo").and_then(Value::as_str) {
                harness.state_mut().session.account_dir = Some(Path::new(dir).join("Accounts"));
            }
        } else if let Some(raw) = step.get("arrive").and_then(Value::as_str) {
            if let Some(server) = &fake_server
                && let Some(mailbox) = server.state().mailbox("INBOX")
            {
                mailbox.add(raw.as_bytes(), &[]);
            }
        } else if step.get("settle").and_then(Value::as_bool) == Some(true) {
            for _ in 0..500 {
                if harness.state().session.jobs.is_empty() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
                harness.run_steps(1);
            }
        } else if let Some(text) = step.get("type").and_then(Value::as_str) {
            harness.event(egui::Event::Text(text.to_string()));
        } else if let Some(path) = step.get("shot").and_then(Value::as_str) {
            harness.run_steps(4);
            let image = harness.render()?;
            if let Some(dir) = Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            image.save(path).map_err(|e| format!("{path}: {e}"))?;
            println!("wrote {path} ({}x{})", image.width(), image.height());
        }
        harness.run_steps(2);
    }
    Ok(())
}

/// A profile as a script writes it: each section an object of values, `name` its name.
fn outlook_profile(v: &Value) -> openmailclient_engine::outlook::RawProfile {
    use openmailclient_engine::outlook::{Raw, RawProfile, Section};
    let sections = |key: &str| -> Vec<Section> {
        let all = v.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object);
        all.enumerate()
            .map(|(n, o)| Section {
                name: o.get("name").and_then(Value::as_str).map_or_else(|| format!("{n:08}"), str::to_string),
                values: o
                    .iter()
                    .filter(|(k, _)| k.as_str() != "name")
                    .map(|(k, v)| {
                        let raw = match v {
                            Value::Number(n) => Raw::Number(n.as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0)),
                            Value::String(s) => match s.strip_prefix("hex:") {
                                Some(hex) => Raw::Bytes(
                                    (0..hex.len() / 2)
                                        .filter_map(|i| hex.get(i * 2..i * 2 + 2))
                                        .filter_map(|b| u8::from_str_radix(b, 16).ok())
                                        .collect(),
                                ),
                                None => Raw::Text(s.clone()),
                            },
                            other => Raw::Text(other.to_string()),
                        };
                        (k.clone(), raw)
                    })
                    .collect(),
            })
            .collect()
    };
    RawProfile {
        name: v.get("name").and_then(Value::as_str).unwrap_or("Outlook").to_string(),
        default: true,
        sections: sections("sections"),
        accounts: sections("accounts"),
    }
}
