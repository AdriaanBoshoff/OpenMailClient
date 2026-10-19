//! Settings kept between runs: the page setup, how the window is arranged, which data files
//! were open, and the mail accounts (never their passwords, which are in the credential store). The host says where the file is; nothing here chooses a place.
//!
//! The file is read as anything else from disk is: it may be damaged or written by someone
//! else. What it says is taken a part at a time, and a part that makes no sense is left out.

use super::{CommandSpec, always, need_str, p_bool};
use crate::{CmdResult, EngineError, ReadingPane, Session};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::Path;

/// A settings file larger than this is not ours.
const MAX_FILE: u64 = 1 << 20;
/// What the window keeps for itself is carried as it is, up to this size.
const MAX_UI: usize = 16 << 10;
/// Data files opened again on start.
const MAX_DATA_FILES: usize = 32;
const VERSION: u64 = 1;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "settings.save",
            label: "Save Settings",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"path": a file ending in .json, "ui": what the window keeps for itself (an object, carried as it is)}; written beside the target and put in its place in one step; returns {"untrusted_content": {"path"}}"#,
            enabled: always,
            run: save,
        },
        CommandSpec {
            id: "settings.load",
            label: "Load Settings",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"path": a file ending in .json, "reopen": true to open the data files it lists}; a file that is not there changes nothing; returns {"found", "applied": parts taken, "ignored": parts that made no sense, "opened", "missing": data files that could not be opened, "ui"}"#,
            enabled: always,
            run: load,
        },
    ]
}

fn json_path(p: &Value) -> Result<&Path, EngineError> {
    let path = Path::new(need_str(p, "path")?);
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
        return Err(EngineError::BadParams("settings are kept in a file ending in .json".into()));
    }
    Ok(path)
}

/// The settings of a session as they are written.
pub fn current(s: &Session, ui: Option<&Value>) -> Value {
    let mut setup = crate::print::setup_json(&s.page_setup);
    setup["style"] = json!(s.print_style.name());
    // Which rows are printed is of the moment, and is not kept.
    setup["columns"] = json!(s.print_list.columns.iter().map(|f| f.name()).collect::<Vec<_>>());
    setup["groups"] = json!(s.print_list.groups);
    setup["printer"] = json!(s.printer);
    let files: Vec<String> = s.stores.iter().filter_map(|st| st.path()).take(MAX_DATA_FILES).map(|p| p.display().to_string()).collect();
    let mut out = json!({
        "version": VERSION,
        "pageSetup": setup,
        "readingPane": match s.reading_pane {
            ReadingPane::Right => "right",
            ReadingPane::Bottom => "bottom",
            ReadingPane::Off => "off",
        },
        "folderPane": s.folder_pane,
        "animations": s.animate,
        "keepOriginal": s.keep_original,
        "saveInPlace": s.save_in_place,
        "autoSaveMinutes": s.autosave_min,
        "sendReceiveMinutes": s.send_receive_min,
        "newMailAsItComes": s.new_mail_as_it_comes,
        "newMail": s.alerts.to_json(),
        "timeZone": s.zone_chosen,
        "secondTimeZone": s.second_zone.as_ref().map(|z| z.name().to_string()),
        "dataFiles": files,
        "accounts": s.accounts.iter().map(crate::accounts::Account::to_settings).collect::<Vec<_>>(),
    });
    if let Some(ui) = ui.filter(|ui| ui.is_object() && ui.to_string().len() <= MAX_UI) {
        out["ui"] = ui.clone();
    }
    out
}

fn save(s: &mut Session, p: &Value) -> CmdResult {
    let path = json_path(p)?;
    let text = serde_json::to_string_pretty(&current(s, p.get("ui"))).map_err(|e| EngineError::Format(e.to_string()))?;
    let name = path.file_name().ok_or_else(|| EngineError::BadParams("the path has no file name".into()))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = path.with_file_name(tmp_name);
    let written = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(EngineError::Io(e));
    }
    Ok(json!({"untrusted_content": {"path": path.display().to_string()}}))
}

fn load(s: &mut Session, p: &Value) -> CmdResult {
    let path = json_path(p)?;
    let nothing = |found: bool| json!({"found": found, "applied": 0, "ignored": 0, "opened": 0, "missing": 0});
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        // The first run: there is nothing to take.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(nothing(false)),
        Err(e) => return Err(EngineError::Io(e)),
    };
    let mut text = String::new();
    // Not text, or longer than any settings of ours: nothing is taken from it.
    if file.take(MAX_FILE + 1).read_to_string(&mut text).is_err() || text.len() as u64 > MAX_FILE {
        return Ok(nothing(true));
    }
    let Ok(Value::Object(all)) = serde_json::from_str::<Value>(&text) else { return Ok(nothing(true)) };
    let (mut applied, mut ignored) = (0u32, 0u32);
    let mut took = |ok: bool| if ok { applied += 1 } else { ignored += 1 };

    if let Some(setup) = all.get("pageSetup") {
        // Each part by itself: one that makes no sense does not cost the others.
        for key in ["paper", "orientation", "margins", "header", "footer", "details", "headerFooter"] {
            if let Some(v) = setup.get(key) {
                let part = crate::print::setup_with(&s.page_setup, &json!({ key: v }));
                took(part.is_ok());
                if let Ok(part) = part {
                    s.page_setup = part;
                }
            }
        }
        if setup.get("style").is_some() {
            let style = crate::print::style_with(s.print_style, setup);
            took(style.is_ok());
            if let Ok(style) = style {
                s.print_style = style;
            }
        }
        // The printer chosen last; whether it is still there is seen when printing.
        if let Some(v) = setup.get("printer") {
            let name = if v.is_null() { Some(None) } else { v.as_str().and_then(crate::printers::clean_name).map(Some) };
            took(name.is_some());
            if let Some(name) = name {
                s.printer = name;
            }
        }
        for key in ["columns", "groups"] {
            if let Some(v) = setup.get(key) {
                let listing = crate::print::listing_with(&s.print_list, &json!({ key: v }));
                took(listing.is_ok());
                if let Ok(listing) = listing {
                    s.print_list = listing;
                }
            }
        }
    }
    if let Some(v) = all.get("readingPane") {
        let pane = match v.as_str() {
            Some("right") => Some(ReadingPane::Right),
            Some("bottom") => Some(ReadingPane::Bottom),
            Some("off") => Some(ReadingPane::Off),
            _ => None,
        };
        took(pane.is_some());
        if let Some(pane) = pane {
            s.reading_pane = pane;
        }
    }
    if let Some(v) = all.get("folderPane") {
        took(v.is_boolean());
        if let Some(show) = v.as_bool() {
            s.folder_pane = show;
        }
    }
    if let Some(v) = all.get("animations") {
        took(v.is_boolean());
        if let Some(on) = v.as_bool() {
            s.animate = on;
        }
    }
    if let Some(v) = all.get("keepOriginal") {
        took(v.is_boolean());
        if let Some(keep) = v.as_bool() {
            s.keep_original = keep;
        }
    }
    if let Some(v) = all.get("saveInPlace") {
        took(v.is_boolean());
        if let Some(on) = v.as_bool() {
            s.save_in_place = on;
        }
    }
    if let Some(v) = all.get("autoSaveMinutes") {
        let minutes = v.as_u64().filter(|m| *m <= super::file::MAX_AUTOSAVE_MIN).and_then(|m| u32::try_from(m).ok());
        took(minutes.is_some());
        if let Some(m) = minutes {
            s.autosave_min = m;
        }
    }
    if let Some(v) = all.get("sendReceiveMinutes") {
        let minutes = v.as_u64().filter(|m| *m <= crate::arrival::MAX_EVERY_MIN).and_then(|m| u32::try_from(m).ok());
        took(minutes.is_some());
        if let Some(m) = minutes {
            s.send_receive_min = m;
        }
    }
    if let Some(v) = all.get("newMailAsItComes") {
        took(v.is_boolean());
        if let Some(on) = v.as_bool() {
            s.new_mail_as_it_comes = on;
        }
    }
    if let Some(v) = all.get("timeZone") {
        let ok = match v {
            Value::Null => s.choose_zone(None).is_ok(),
            Value::String(name) => s.choose_zone(Some(name)).is_ok(),
            _ => false,
        };
        took(ok);
    }
    if let Some(v) = all.get("secondTimeZone") {
        let zone = match v {
            Value::Null => Some(None),
            Value::String(name) => openmailclient_model::zone::Zone::named(name).map(Some),
            _ => None,
        };
        took(zone.is_some());
        if let Some(z) = zone {
            s.second_zone = z;
        }
    }
    if let Some(v) = all.get("newMail") {
        let alerts = v.is_object().then(|| s.alerts.with(v)).flatten();
        took(alerts.is_some());
        if let Some(a) = alerts {
            s.alerts = a;
        }
    }
    if let Some(v) = all.get("accounts") {
        let (kept, left_out) = crate::accounts::from_settings(v);
        took(v.is_array());
        ignored += left_out;
        if v.is_array() {
            s.accounts = kept;
            for e in super::account::open_account_stores(s) {
                s.status = format!("A data file of an account could not be opened: {e}");
            }
        }
    }
    let (mut opened, mut missing) = (0u32, 0u32);
    if p_bool(p, "reopen") == Some(true) {
        let files = all.get("dataFiles").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        for path in files.iter().take(MAX_DATA_FILES) {
            // Only data files are opened again, and only those that are still there.
            let is_data_file = |p: &&str| Path::new(p).extension().is_some_and(|e| e.eq_ignore_ascii_case("pst") || e.eq_ignore_ascii_case("ost"));
            match path.as_str().filter(is_data_file).map(|p| super::file::open_data_file(s, p)) {
                Some(Ok(())) => opened += 1,
                _ => missing += 1,
            }
        }
    }
    let mut out = json!({"found": true, "applied": applied, "ignored": ignored, "opened": opened, "missing": missing});
    if let Some(ui) = all.get("ui").filter(|ui| ui.is_object() && ui.to_string().len() <= MAX_UI) {
        out["ui"] = ui.clone();
    }
    if missing > 0 {
        s.status = format!("{missing} of the data files that were open last time could not be opened");
    }
    Ok(out)
}
