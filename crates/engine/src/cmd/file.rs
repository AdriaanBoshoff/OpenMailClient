//! File commands: opening data files and loose messages, saving a message, closing a data file.

use super::{CommandSpec, always, has_selection, need_str, ok, p_bool, p_str, p_u64};
use crate::io::{self, Opened};
use crate::{CmdResult, EngineError, Session, attachments};
use openmailclient_model::{FolderId, MemStore, Recovery, StoreError};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;

/// Loose `.msg`/`.eml` files open into this in-memory store.
pub const OPENED_FILES: &str = "Opened Files";

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "file.open",
            label: "Open Outlook Data File",
            tab: "File",
            group: "Open & Export",
            shortcut: "Ctrl+O",
            params: r#"{"path": a .pst, .ost, .msg, .oft or .eml file, "recover": false to leave unsaved changes of an earlier run where they are}; nothing is written to the file until file.save; changes a run that ended without saving left in the journal beside a data file are brought back, unsaved as they were; a journal that cannot be kept beside the file is kept in a folder of the program's own; returns {"store", "recovered", "lost", "lostMore": true when more than "lost" may be missing, "staleJournal"}"#,
            enabled: always,
            run: open,
        },
        CommandSpec {
            id: "file.openSample",
            label: "Open Sample Mailbox",
            tab: "File",
            group: "Open & Export",
            shortcut: "",
            params: "",
            enabled: always,
            run: open_sample,
        },
        CommandSpec {
            id: "file.saveAs",
            label: "Save As",
            tab: "File",
            group: "Save",
            shortcut: "F12",
            params: r#"{"path": target ending in .msg, .oft or .eml, or a folder to save it in under its subject without replacing anything; "format": "msg" (default), "eml" or "oft" for a folder}"#,
            enabled: has_selection,
            run: save_as,
        },
        CommandSpec {
            id: "file.save",
            label: "Save Changes",
            tab: "File",
            group: "Save",
            shortcut: "Ctrl+S",
            params: r#"{"store": index, "wait": false to come back at once and do the writing on a thread of its own}; default: every data file with unsaved changes, and waiting. What changed is written into the data file where it is, with what is written over kept beside it until the save is checked and read back, and put back if anything goes wrong; the first save that keeps the file as it was, a file that cannot be written into and one with much unused space are written anew beside themselves, checked, read back and only then put in the old one's place. A save that is not waited for takes the mailbox as it stands when asked; until it ends the data file is read as before and takes no changes, and file.stopSave stops it; returns {"saved": how many, "inPlace": how many of them went into the file where it is, "started": how many are being saved, "kept": where each file as it first was is kept}"#,
            enabled: can_save,
            run: save,
        },
        CommandSpec {
            id: "file.keepOriginal",
            label: "Keep the File as It Was on First Save",
            tab: "File",
            group: "Save",
            shortcut: "",
            params: r#"{"on": bool}; default: the other way. On (as it is at first), the first save of a data file leaves the file as it was beside it, under a name ending in .before-openmailclient.pst; off, it does not. A file kept before stays. Kept with the settings; returns {"on"}"#,
            enabled: always,
            run: keep_original,
        },
        CommandSpec {
            id: "file.saveInPlace",
            label: "Save Changes into the Data File",
            tab: "File",
            group: "Save",
            shortcut: "",
            params: r#"{"on": bool}; default: the other way. On (as it is at first), a save writes what changed into the data file where it is, which takes as long as the changes are large and not as long as the mailbox is; off, every save writes the whole file anew and puts it in the old one's place. Either way a save that fails leaves the file as it was. Kept with the settings; returns {"on"}"#,
            enabled: always,
            run: save_in_place,
        },
        CommandSpec {
            id: "file.autoSave",
            label: "Save Changes on Their Own",
            tab: "File",
            group: "Save",
            shortcut: "",
            params: r#"{"minutes": 0 to 240}; default: 5 as it is at first, or off when on and on when off. Unsaved changes to a data file are saved on their own once they have waited this many minutes, when nothing has been done for a few seconds (in a mailbox in use all along, after three times as long), on a thread of their own and checked as Save Changes does; 0 saves only when asked. A data file is never saved for the first time on its own: one that was not saved here before waits for Save Changes. Kept with the settings; returns {"minutes"}"#,
            enabled: always,
            run: auto_save,
        },
        CommandSpec {
            id: "file.stopSave",
            label: "Stop Saving",
            tab: "File",
            group: "Save",
            shortcut: "",
            params: r#"{"store": index}; default: every save, every export and all work with mail servers that is running. A data file being saved stays as it was and its changes stay unsaved; a file being exported to is not written; returns {"stopping": how many}"#,
            enabled: is_saving,
            run: stop_save,
        },
        CommandSpec {
            id: "file.export",
            label: "Export to a Data File",
            tab: "File",
            group: "Open & Export",
            shortcut: "",
            params: r#"{"path": target ending in .pst, "store": index (default: the current one), "kind": "unicode" or "ansi" (the kind older programs read, of at most 2 GB; text is 8-bit where the code page of its message has all of it and Unicode text where it has not, so nothing is lost) or "4k" (pages of 4096 bytes and blocks of up to 64 KB, the kind newer versions of Outlook make; its layout is in no specification and was measured on files Outlook wrote) (default: the kind the source is when it is copied as stored, Unicode otherwise), "wait": false to come back at once ({"started": true}) and write the new file on a thread of its own, from the mailbox as it stands when asked; file.stopSave stops it}; every folder and item of the data file goes into a new .pst, which is checked before it takes the target's place. A Unicode .pst is copied as it is stored, unsaved changes with it: hidden items, a folder's own properties and properties this program does not know go along ("copied": true); any other source, and one with damage, gives what could be read; returns {"folders", "items", "unreadable", "copied", "bytes", "untrusted_content": {"path"}}"#,
            enabled: has_store,
            run: export,
        },
        CommandSpec {
            id: "file.saveAttachment",
            label: "Save Attachment",
            tab: "File",
            group: "Save",
            shortcut: "",
            params: r#"{"index": attachment number, "path": target file, or a folder to save it in under its own name}; returns {"untrusted_content": {"path"}}"#,
            enabled: has_selection,
            run: save_attachment,
        },
        CommandSpec {
            id: "file.saveAllAttachments",
            label: "Save All Attachments",
            tab: "File",
            group: "Save",
            shortcut: "",
            params: r#"{"folder": an existing folder, "inline": also pictures placed in the body (default false)}; no file is replaced; returns {"count", "untrusted_content": {"paths"}}"#,
            enabled: has_selection,
            run: save_all_attachments,
        },
        CommandSpec {
            id: "file.openAttachment",
            label: "Open Attachment",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"index": attachment number, "confirm": true, as the reader's answer to being asked}; puts a copy of the attachment, made read-only and marked as come from the internet, in a folder of the program's own, cleared when it ends; a program, a script or a file whose name hides what it is is not opened, only saved; nothing is started here: returns {"inside": true for an attached message, which the window opens itself with file.open, "untrusted_content": {"open": the copy's path}} for the window to hand to the program the system opens such files with"#,
            enabled: has_selection,
            run: open_attachment,
        },
        CommandSpec {
            id: "file.print",
            label: "Print",
            tab: "File",
            group: "Print",
            shortcut: "Ctrl+P",
            params: r#"{"path": target ending in .pdf, or "printer": the name of a printer file.printers lists, to print on it, with "copies" (1 to 99, default 1), "from" and "to" (the pages printed, from 1; default all of them) and "wait": false to print on a thread of its own (the window does); "links": which links are numbered and listed after the body: "concerns" (default: those that are not what their text says), "all" or "none"; and, for this print only, anything file.pageSetup takes: "style", "paper", "orientation", "margins", "header", "footer", "details", "headerFooter", "bodyOnly"}; "style" is "memo" (the open message) or "table" (the folder's list as it is shown, a message to a row, with the names of the columns on every page, and with "columns", "groups" and "rows" as file.pageSetup takes them); returns {"path", "pages", "truncated", "links", "style"}, or with a printer {"printer", "pages", "copies", ...} once it is sent, or {"started"} when it does not wait"#,
            enabled: printable,
            run: print,
        },
        CommandSpec {
            id: "file.printers",
            label: "Printer",
            tab: "File",
            group: "Print",
            shortcut: "",
            params: r#"{"choose": the name of a listed printer to print on from now, or null for the system's default}; looks again for this system's printers; returns {"printers": [{"name", "default"}], "printer": the one printed on (the chosen one while it is there, otherwise the default), "chosen"}"#,
            enabled: always,
            run: printers,
        },
        CommandSpec {
            id: "file.printPreview",
            label: "Print Preview",
            tab: "File",
            group: "Print",
            shortcut: "",
            params: r#"{"page": which page, from 1 (default 1), "path": where to write that page as a picture, ending in .png (left out: nothing is written), "dpi": pixels to the inch of the picture, 24 to 600 (default 96), "links" as file.print; and, for this look only, anything file.pageSetup takes}; the page is drawn from what file.print writes; returns {"pages", "page", "truncated", "links", "style", "setup"} and with a path {"path", "width", "height"} in pixels"#,
            enabled: printable,
            run: print_preview,
        },
        CommandSpec {
            id: "file.pageSetup",
            label: "Page Setup",
            tab: "File",
            group: "Print",
            shortcut: "",
            params: r#"{"paper": "a4", "letter", "legal", "a3", "a5", "b5", "executive" or "tabloid", "orientation": "portrait" or "landscape", "margins": millimetres, one number for every edge or {"top", "right", "bottom", "left"}, "header" and "footer": a text for the middle, or {"left", "center", "right"}; in them [Page], [Pages], [Page of Pages], [Date], [Time], [User] and [Subject] are filled in, "details": false to leave out the owner's name and the message's From, Sent, To and Subject at the head (in Table Style the folder's name), "headerFooter": false to print no header or footer (what they say is kept), "bodyOnly": true for both, "style": "memo" or "table"; for Table Style "columns": "default" or the columns from the left, of who (From, or To in a folder of outgoing mail), from, to, subject, date, size, notes, categories, importance, flag, attachment, preview; "groups": false to leave out the heading over each group of the list as it is arranged; "rows": "all" or "selected" for the messages selected (not kept between runs); "reset": true to start from the default}; nothing asked for changes nothing; returns the setup"#,
            enabled: always,
            run: page_setup,
        },
        CommandSpec {
            id: "file.closeStore",
            label: "Close Data File",
            tab: "Folder",
            group: "Actions",
            shortcut: "",
            params: r#"{"store": index (default: the current one), "save": true or "discard": true when it has unsaved changes}"#,
            enabled: has_store,
            run: close_store,
        },
    ]
}

fn has_store(s: &Session) -> bool {
    !s.stores.is_empty()
}

fn can_save(s: &Session) -> bool {
    s.stores.iter().any(|st| st.unsaved() && !st.saving())
}

fn is_saving(s: &Session) -> bool {
    !s.saves.is_empty() || !s.exports.is_empty() || !s.jobs.is_empty()
}

/// Ask saves that are running to stop. They end when their work notices.
fn stop_save(s: &mut Session, p: &Value) -> CmdResult {
    let only = p_u64(p, "store").map(|i| usize::try_from(i).unwrap_or(usize::MAX));
    let mut stopping = 0;
    for saving in s.saves.iter().filter(|saving| only.is_none_or(|i| i == saving.store)) {
        saving.progress.stop();
        stopping += 1;
    }
    for export in s.exports.iter().filter(|export| only.is_none_or(|i| i == export.store)) {
        export.progress.stop();
        stopping += 1;
    }
    // Work with a server stops at the next step it comes to; what it did by then is kept.
    for job in s.jobs.iter().filter(|_| only.is_none()) {
        job.progress.stop();
        stopping += 1;
    }
    Ok(json!({ "stopping": stopping }))
}

fn keep_original(s: &mut Session, p: &Value) -> CmdResult {
    s.keep_original = p_bool(p, "on").unwrap_or(!s.keep_original);
    s.status = if s.keep_original {
        "The first save of a data file keeps the file as it was beside it".to_string()
    } else {
        "A save keeps no copy of the data file as it was".to_string()
    };
    Ok(json!({ "on": s.keep_original }))
}

/// The longest wait `file.autoSave` takes: four hours.
pub(crate) const MAX_AUTOSAVE_MIN: u64 = 240;

fn auto_save(s: &mut Session, p: &Value) -> CmdResult {
    s.autosave_min = match p.get("minutes") {
        None | Some(Value::Null) if s.autosave_min == 0 => 5,
        None | Some(Value::Null) => 0,
        Some(v) => match v.as_u64() {
            Some(m) if m <= MAX_AUTOSAVE_MIN => u32::try_from(m).unwrap_or(5),
            _ => return Err(EngineError::BadParams(format!("\"minutes\" is a whole number from 0 to {MAX_AUTOSAVE_MIN}"))),
        },
    };
    s.status = match s.autosave_min {
        0 => "Changes are saved only when asked".to_string(),
        1 => "Unsaved changes are saved on their own after a minute".to_string(),
        m => format!("Unsaved changes are saved on their own after {m} minutes"),
    };
    Ok(json!({ "minutes": s.autosave_min }))
}

fn save_in_place(s: &mut Session, p: &Value) -> CmdResult {
    s.save_in_place = p_bool(p, "on").unwrap_or(!s.save_in_place);
    s.status = if s.save_in_place {
        "A save writes what changed into the data file where it is".to_string()
    } else {
        "Every save writes the whole data file anew".to_string()
    };
    Ok(json!({ "on": s.save_in_place }))
}

/// Start the long part of a save on a thread of its own. False when this store has no such
/// part, or no thread can be had: it is saved the plain way then.
fn save_elsewhere(s: &mut Session, idx: usize, id: [u8; 16]) -> Result<bool, EngineError> {
    if cfg!(target_arch = "wasm32") {
        return Ok(false);
    }
    let Some(store) = s.stores.get_mut(idx) else { return Ok(false) };
    let Some(work) = store.save_begin(id)? else { return Ok(false) };
    let progress = std::sync::Arc::new(openmailclient_model::Progress::default());
    let (tx, done) = std::sync::mpsc::channel();
    let theirs = progress.clone();
    let spawned = std::thread::Builder::new().name("save".to_string()).spawn(move || {
        // A panic in the work becomes an error here; nobody waits for a save that says nothing.
        let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(theirs)))
            .unwrap_or_else(|_| Err(StoreError::Format("the save failed inside; nothing was replaced".to_string())));
        let _ = tx.send(done);
    });
    if spawned.is_err() {
        // The work went with the thread that never was: the store is told, and saves plainly.
        let _ = store.save_end(Err(StoreError::Format("no thread could be had".to_string())));
        return Ok(false);
    }
    let name = store.display_name();
    s.saves.push(crate::Saving::new(idx, name, progress, done));
    Ok(true)
}

/// Put the changes made to data files into them.
fn save(s: &mut Session, p: &Value) -> CmdResult {
    let which: Vec<usize> = match p_u64(p, "store") {
        Some(i) => {
            let idx = usize::try_from(i).unwrap_or(usize::MAX);
            if idx >= s.stores.len() {
                return Err(EngineError::BadParams(format!("there is no data file {idx}")));
            }
            vec![idx]
        }
        None => (0..s.stores.len()).collect(),
    };
    let wait = p_bool(p, "wait") != Some(false);
    let (mut saved, mut kept) = (Vec::new(), Vec::new());
    let (mut started, mut in_place) = (0, 0);
    for idx in which {
        let id = io::new_store_id(s.now);
        match s.stores.get(idx) {
            Some(st) if st.saving() && wait => {
                return Err(EngineError::BadParams(format!("{} is being saved already", st.display_name())));
            }
            Some(st) if st.unsaved() && !st.saving() => {}
            _ => continue,
        }
        let (keep, onto) = (s.keep_original, s.save_in_place);
        if let Some(store) = s.stores.get_mut(idx) {
            store.keep_original(keep);
            store.save_in_place(onto);
        }
        if !wait && save_elsewhere(s, idx, id)? {
            started += 1;
            continue;
        }
        let Some(store) = s.stores.get_mut(idx) else { continue };
        let before = store.saved_in_place();
        store.save(id)?;
        if let Some(path) = store.path() {
            s.auto.saved_here.insert(path.to_path_buf());
        }
        in_place += u32::from(store.saved_in_place() > before);
        saved.push(store.display_name());
        if let Some(original) = store.path().map(openmailclient_pst::original_beside).filter(|o| o.exists()) {
            kept.push(original.display().to_string());
        }
        s.refresh_folders(idx);
    }
    s.status = match saved.as_slice() {
        // What is being saved says so itself (`Session::saving`); the status has nothing to add.
        [] if started > 0 => String::new(),
        [] if !s.saves.is_empty() => "The data file is being saved".to_string(),
        [] => "There are no unsaved changes".to_string(),
        [one] => format!("Saved {one}"),
        many => format!("Saved {} data files", many.len()),
    };
    Ok(json!({ "saved": saved.len(), "inPlace": in_place, "started": started, "kept": kept }))
}

/// Save one data file as `file.save` does, on a thread of its own where it can be.
pub(super) fn save_in_background(s: &mut Session, store: usize) -> CmdResult {
    save(s, &json!({ "store": store, "wait": false }))
}

/// Open a data file as `file.open` does.
pub(super) fn open_data_file(s: &mut Session, path: &str) -> Result<(), EngineError> {
    open(s, &json!({ "path": path })).map(|_| ())
}

fn open(s: &mut Session, p: &Value) -> CmdResult {
    let path = Path::new(need_str(p, "path")?);
    // A data file that is open already is gone to, not opened a second time beside itself.
    if let Ok(real) = std::fs::canonicalize(path)
        && let Some(idx) = s.stores.iter().position(|st| st.path().and_then(|p| std::fs::canonicalize(p).ok()).is_some_and(|p| p == real))
    {
        s.select_default_folder(idx);
        return Ok(json!({ "store": idx, "already": true }));
    }
    match io::open_path(path)? {
        Opened::Store(mut store) => {
            let name = store.display_name();
            // What a run that ended without saving left in the journal comes back, unsaved
            // as it was. A journal that cannot be read does not keep the file from opening.
            if let Some(dir) = &s.journal_dir {
                store.journal_elsewhere(dir);
            }
            let found = if p_bool(p, "recover") == Some(false) { Ok(Recovery::default()) } else { store.recover() };
            let idx = s.add_store(store);
            s.select_default_folder(idx);
            let mode = if s.stores.get(idx).is_some_and(|st| st.writable()) { "changes are kept when saved" } else { "read-only" };
            s.status = format!("Opened {name} ({mode})");
            let found = match found {
                Ok(found) => found,
                Err(e) => {
                    s.status = format!("Opened {name}; the journal of unsaved changes beside it could not be read ({e})");
                    Recovery::default()
                }
            };
            if found.applied > 0 {
                s.status = format!("Opened {name}: {} unsaved changes of last time were brought back; Save Changes keeps them", found.applied);
            }
            if found.lost > 0 {
                // The count is said as far as it is known, and no further.
                let more = if found.more { " or more" } else { "" };
                s.status.push_str(&format!("; {}{more} could not be, and the journal as it was found is kept", found.lost));
            } else if found.stale {
                s.status.push_str("; a journal of unsaved changes beside it is for the file as it once was, and was left alone");
            } else if found.held {
                s.status.push_str("; another running OpenMailClient has unsaved changes to it, so changes made here are not journalled");
            }
            Ok(json!({ "store": idx, "recovered": found.applied, "lost": found.lost, "lostMore": found.more, "staleJournal": found.stale }))
        }
        Opened::Item(item) => {
            let idx = match s.stores.iter().position(|st| st.display_name() == OPENED_FILES && st.writable()) {
                Some(i) => i,
                None => {
                    let mut mem = MemStore::new(OPENED_FILES);
                    mem.push_folder(None, "Files", "IPF.Note");
                    s.add_store(Box::new(mem))
                }
            };
            let folder = s.folders.get(idx).and_then(|f| f.first()).map(|f| f.id).ok_or(StoreError::NoFolder)?;
            let id = s.stores.get_mut(idx).ok_or(StoreError::NoFolder)?.add(folder, *item)?;
            s.refresh_folders(idx);
            s.select_folder(idx, folder)?;
            // Opening a file is not reading it: keep the read state the file had.
            let item = s.stores.get_mut(idx).ok_or(StoreError::NoFolder)?.load(folder, id)?;
            s.selected = Some(id.0);
            s.open = Some(item);
            Ok(json!({ "store": idx, "id": id.0 }))
        }
    }
}

fn open_sample(s: &mut Session, _: &Value) -> CmdResult {
    let idx = s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.select_default_folder(idx);
    Ok(json!({ "store": idx }))
}

fn save_as(s: &mut Session, p: &Value) -> CmdResult {
    let path = Path::new(need_str(p, "path")?);
    let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
    // A folder takes the message under its subject, made safe, beside what is there.
    let target = if path.is_dir() {
        let kind = match p_str(p, "format").map(str::to_ascii_lowercase).as_deref() {
            None | Some("msg") => "msg",
            Some("eml") => "eml",
            Some("oft") => "oft",
            Some(other) => return Err(EngineError::BadParams(format!("\"{other}\" is not msg, eml or oft"))),
        };
        let subject = item.subject();
        let stem = attachments::safe_name(if subject.trim().is_empty() { "No subject" } else { subject.trim() });
        // A subject's last dot is not a file ending.
        let name = format!("{}.{kind}", stem.replace('.', "_"));
        attachments::free_path(path, &name, &mut HashSet::new()).ok_or_else(|| full(path))?
    } else {
        path.to_path_buf()
    };
    io::save_item(item, &target)?;
    s.status = format!("Saved {}", target.display());
    Ok(json!({"untrusted_content": {"path": target.display().to_string()}}))
}

/// Save a whole data file as a new `.pst`.
fn export(s: &mut Session, p: &Value) -> CmdResult {
    let path = Path::new(need_str(p, "path")?);
    let idx = match p_u64(p, "store") {
        Some(i) => usize::try_from(i).unwrap_or(usize::MAX),
        None => s.current.map(|(i, _)| i).unwrap_or(0),
    };
    // A data file that is open here is not written over: its reader holds it.
    if let Ok(real) = std::fs::canonicalize(path)
        && s.stores.iter().any(|st| st.path().and_then(|p| std::fs::canonicalize(p).ok()).is_some_and(|p| p == real))
    {
        return Err(EngineError::BadParams("that data file is open here; close it first or choose another name".into()));
    }
    if s.exports.iter().any(|e| e.to == path) {
        return Err(EngineError::BadParams("that data file is being written by an export already".into()));
    }
    let kind = io::PstKind::named(p_str(p, "kind"))?;
    let id = io::new_store_id(s.now);
    let store = s.stores.get_mut(idx).ok_or_else(|| EngineError::BadParams(format!("there is no data file {idx}")))?;
    if p_bool(p, "wait") == Some(false)
        && let Some((work, tmp)) = io::export_begin(store.as_mut(), path, id, kind)?
    {
        let progress = std::sync::Arc::new(openmailclient_model::Progress::default());
        let (tx, done) = std::sync::mpsc::channel();
        let (theirs, beside) = (progress.clone(), tmp.clone());
        let spawned = std::thread::Builder::new().name("export".to_string()).spawn(move || {
            let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(theirs)))
                .unwrap_or_else(|_| Err(EngineError::Format("the export failed inside; nothing was written".to_string())));
            // Nobody is left to put the file in its place: it goes.
            if tx.send(done).is_err() {
                let _ = std::fs::remove_file(&beside);
            }
        });
        if spawned.is_ok() {
            s.exports.push(crate::Exporting::new(idx, path.to_path_buf(), tmp, progress, done));
            s.status = String::new();
            return Ok(json!({"started": true, "untrusted_content": {"path": path.display().to_string()}}));
        }
        // No thread could be had: the work went with it, and the export is done here.
    }
    let saved = io::save_store_as(store.as_mut(), path, id, kind)?;
    s.status = saved.says(path);
    Ok(json!({
        "started": false,
        "copied": saved.copied,
        "folders": saved.folders,
        "items": saved.items,
        "unreadable": saved.unreadable,
        "bytes": saved.bytes,
        "untrusted_content": {"path": path.display().to_string()},
    }))
}

fn save_attachment(s: &mut Session, p: &Value) -> CmdResult {
    let path = Path::new(need_str(p, "path")?);
    let index = usize::try_from(p_u64(p, "index").unwrap_or(0)).unwrap_or(usize::MAX);
    let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
    let att = item
        .attachments
        .get(index)
        .ok_or_else(|| EngineError::BadParams(format!("there is no attachment {index}; the message has {}", item.attachments.len())))?;
    // A folder takes the attachment under its own name, made safe, beside what is there.
    let target = if path.is_dir() {
        attachments::free_path(path, &attachments::file_name(att), &mut HashSet::new()).ok_or_else(|| full(path))?
    } else {
        path.to_path_buf()
    };
    save_one(att, &target)?;
    s.status = format!("Saved {}", target.display());
    Ok(json!({"untrusted_content": {"path": target.display().to_string()}}))
}

/// A copy of an attachment for the program the system opens it with, on the reader's word.
fn open_attachment(s: &mut Session, p: &Value) -> CmdResult {
    let index = usize::try_from(p_u64(p, "index").unwrap_or(0)).unwrap_or(usize::MAX);
    let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
    let att = item
        .attachments
        .get(index)
        .ok_or_else(|| EngineError::BadParams(format!("there is no attachment {index}; the message has {}", item.attachments.len())))?;
    if p_bool(p, "confirm") != Some(true) {
        return Err(EngineError::BadParams("an attachment is opened only when the reader says so: give \"confirm\": true".into()));
    }
    if let Some(why) = attachments::not_opened(att) {
        return Err(EngineError::Format(why));
    }
    let folder = attachments::opened_folder().ok_or_else(|| EngineError::Disabled("Opening attachments (no file system)".into()))?;
    std::fs::create_dir_all(&folder)?;
    let target = attachments::free_path(&folder, &attachments::file_name(att), &mut HashSet::new()).ok_or_else(|| full(&folder))?;
    save_one(att, &target)?;
    // Marked as come from the internet, so the program it opens in is careful with it, as it is
    // with a download (Office opens it in Protected View); and read-only, so a change to it is
    // not taken for a change to the attachment.
    if cfg!(windows) {
        let mut zone = target.clone().into_os_string();
        zone.push(":Zone.Identifier");
        let _ = std::fs::write(
            zone,
            "[ZoneTransfer]

ZoneId=3

",
        );
    }
    if let Ok(meta) = std::fs::metadata(&target) {
        let mut perms = meta.permissions();
        perms.set_readonly(true);
        let _ = std::fs::set_permissions(&target, perms);
    }
    // An attached message is opened here, as a message, not handed to another mail program.
    let inside = att.embedded.is_some();
    s.status = format!("Opening {}", attachments::file_name(att));
    Ok(json!({"inside": inside, "untrusted_content": {"open": target.display().to_string()}}))
}

fn full(folder: &Path) -> EngineError {
    EngineError::BadParams(format!("{} has too many files of that name", folder.display()))
}

fn save_one(att: &openmailclient_model::Attachment, path: &Path) -> Result<(), EngineError> {
    match &att.embedded {
        Some(inner) => io::save_item(inner, path),
        None => io::write_atomic(path, &att.data),
    }
}

/// Save every attachment of the open message into a folder, each under its own name made safe.
/// No file is replaced: a name that is taken gets a number.
fn save_all_attachments(s: &mut Session, p: &Value) -> CmdResult {
    let folder = Path::new(need_str(p, "folder")?);
    if !folder.is_dir() {
        return Err(EngineError::BadParams(format!("{} is not a folder", folder.display())));
    }
    let inline = p_bool(p, "inline").unwrap_or(false);
    let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
    let mut taken = HashSet::new();
    let mut paths = Vec::new();
    for att in item.attachments.iter().filter(|a| inline || attachments::is_file(a)) {
        let target = attachments::free_path(folder, &attachments::file_name(att), &mut taken).ok_or_else(|| full(folder))?;
        save_one(att, &target)?;
        paths.push(target.display().to_string());
    }
    s.status = match paths.len() {
        0 => "This message has no attachments to save".to_string(),
        1 => format!("Saved 1 attachment to {}", folder.display()),
        n => format!("Saved {n} attachments to {}", folder.display()),
    };
    Ok(json!({"count": paths.len(), "untrusted_content": {"paths": paths}}))
}

/// There is something to print: a message that is open, or a folder whose list is shown.
fn printable(s: &Session) -> bool {
    has_selection(s) || s.current.is_some()
}

/// Which links a print lists, as the parameters ask.
fn print_links(p: &Value) -> Result<crate::print::Links, EngineError> {
    match p_str(p, "links") {
        Some(name) => {
            crate::print::Links::named(name).ok_or_else(|| EngineError::BadParams(format!("unknown links {name:?}: none, concerns or all")))
        }
        None => Ok(crate::print::Links::default()),
    }
}

/// Lay the open message out as it would be printed and say how many pages it takes; with a
/// path, write one page as a picture.
fn print_preview(s: &mut Session, p: &Value) -> CmdResult {
    let path = match p_str(p, "path") {
        Some(path) if Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) => Some(Path::new(path)),
        Some(_) => return Err(EngineError::BadParams("a page is written as a PNG picture: the path must end in .png".into())),
        None => None,
    };
    let page = match p.get("page") {
        None => 1,
        Some(v) => v
            .as_u64()
            .filter(|n| *n >= 1)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| EngineError::BadParams("\"page\" is a page's number, from 1".into()))?,
    };
    let dpi = match p.get("dpi") {
        None => 96.0,
        Some(v) => {
            v.as_f64().filter(|d| (24.0..=600.0).contains(d)).ok_or_else(|| EngineError::BadParams("\"dpi\" is a number from 24 to 600".into()))?
        }
    };
    let setup = crate::print::setup_with(&s.page_setup, p)?;
    let style = crate::print::style_with(s.print_style, p)?;
    let listing = crate::print::listing_with(&s.print_list, p)?;
    let links = print_links(p)?;
    let laid = crate::print::laid_as(s, style, &setup, &listing, links)?;
    let pages = laid.printed.pages.len();
    if page > pages {
        return Err(EngineError::BadParams(format!("there is no page {page}: there are {pages}")));
    }
    let mut out = json!({"pages": pages, "page": page, "truncated": laid.printed.truncated, "links": laid.links, "style": style.name(), "setup": crate::print::setup_json(&setup)});
    if let Some(path) = path {
        let image =
            crate::print::raster(&laid, page - 1, dpi as f32 / 72.0).ok_or_else(|| EngineError::Format("the page could not be drawn".into()))?;
        io::write_atomic(path, &crate::print::png(&image)?)?;
        out["path"] = json!(path.display().to_string());
        out["width"] = json!(image.width);
        out["height"] = json!(image.height);
        s.status = format!("Page {page} of {pages} written to {}", path.display());
    }
    Ok(out)
}

/// Print the open message, in Memo Style, to a PDF file.
fn print(s: &mut Session, p: &Value) -> CmdResult {
    if p.get("printer").is_some() {
        return print_on(s, p);
    }
    let path = Path::new(need_str(p, "path")?);
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        return Err(EngineError::BadParams("printing writes a PDF file: the path must end in .pdf".into()));
    }
    let setup = crate::print::setup_with(&s.page_setup, p)?;
    let style = crate::print::style_with(s.print_style, p)?;
    let listing = crate::print::listing_with(&s.print_list, p)?;
    let links = print_links(p)?;
    let printed = crate::print::pdf_as(s, style, &setup, &listing, links)?;
    io::write_atomic(path, &printed.file)?;
    s.status = format!("Printed {} page{} to {}", printed.pages, if printed.pages == 1 { "" } else { "s" }, path.display());
    Ok(
        json!({"path": path.display().to_string(), "pages": printed.pages, "truncated": printed.truncated, "links": printed.links, "style": style.name()}),
    )
}

/// Look for the system's printers again, and keep them.
fn find_printers(s: &mut Session) -> Result<(), EngineError> {
    s.printers = s.spooler.printers().map_err(EngineError::Format)?;
    s.printers.truncate(crate::printers::MAX_PRINTERS);
    Ok(())
}

/// The printer printed on: the chosen one while it is listed, otherwise the default.
fn printer_now(s: &Session) -> Option<&crate::printers::Printer> {
    let chosen = s.printer.as_deref().and_then(|name| s.printers.iter().find(|p| p.name == name));
    chosen.or_else(|| s.printers.iter().find(|p| p.default)).or_else(|| s.printers.first())
}

fn printers_json(s: &Session) -> Value {
    json!({
        "printers": s.printers.iter().map(|p| json!({"name": p.name, "default": p.default})).collect::<Vec<_>>(),
        "printer": printer_now(s).map(|p| p.name.clone()),
        "chosen": s.printer,
    })
}

/// Look for this system's printers, and choose the one to print on.
fn printers(s: &mut Session, p: &Value) -> CmdResult {
    find_printers(s)?;
    if let Some(choose) = p.get("choose") {
        if choose.is_null() {
            s.printer = None;
        } else {
            let name = choose.as_str().ok_or_else(|| EngineError::BadParams("\"choose\" is a printer's name, or null".into()))?;
            let found =
                s.printers.iter().find(|p| p.name == name).ok_or_else(|| EngineError::BadParams(format!("there is no printer {name:?} here")))?;
            s.printer = Some(found.name.clone());
        }
    }
    Ok(printers_json(s))
}

/// A page number of the parameters, from 1, as an index from 0.
fn page_param(p: &Value, key: &str) -> Result<Option<usize>, EngineError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .filter(|n| *n >= 1)
            .map(|n| Some(usize::try_from(n - 1).unwrap_or(usize::MAX)))
            .ok_or_else(|| EngineError::BadParams(format!("\"{key}\" is a page's number, from 1"))),
    }
}

/// Print what the session shows on a printer of this system.
fn print_on(s: &mut Session, p: &Value) -> CmdResult {
    let setup = crate::print::setup_with(&s.page_setup, p)?;
    let style = crate::print::style_with(s.print_style, p)?;
    let listing = crate::print::listing_with(&s.print_list, p)?;
    let links = print_links(p)?;
    // The printer is known to be one before anything is laid out.
    printer_named(s, p)?;
    let laid = crate::print::laid_as(s, style, &setup, &listing, links)?;
    let title = crate::print::title(s, style);
    print_laid(s, p, laid, title, style.name())
}

/// The printer `"printer"` names, when the system lists it: nothing else reaches its commands.
pub(super) fn printer_named(s: &mut Session, p: &Value) -> Result<String, EngineError> {
    let name = p.get("printer").and_then(Value::as_str).ok_or_else(|| EngineError::BadParams("\"printer\" is a printer's name".into()))?;
    if !s.printers.iter().any(|known| known.name == name) {
        find_printers(s)?;
    }
    let printer = s.printers.iter().find(|known| known.name == name).map(|known| known.name.clone());
    printer.ok_or_else(|| EngineError::BadParams(format!("there is no printer {name:?} here")))
}

/// Send what was laid out to the printer `"printer"` names, with the `"copies"`, `"from"`,
/// `"to"` and `"wait"` the parameters ask for; `kind` is said back as `"style"`.
pub(super) fn print_laid(s: &mut Session, p: &Value, laid: crate::print::Laid, title: String, kind: &'static str) -> CmdResult {
    let printer = printer_named(s, p)?;
    let copies = match p.get("copies") {
        None | Some(Value::Null) => 1,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=u64::from(crate::printers::MAX_COPIES)).contains(n))
            .ok_or_else(|| EngineError::BadParams(format!("\"copies\" is from 1 to {}", crate::printers::MAX_COPIES)))? as u32,
    };
    let (from, to) = (page_param(p, "from")?, page_param(p, "to")?);
    let wait = p_bool(p, "wait") != Some(false);
    let pages = laid.printed.pages.len();
    let last = pages.checked_sub(1).ok_or_else(|| EngineError::Format("there is nothing to print".into()))?;
    let (from, to) = (from.unwrap_or(0), to.unwrap_or(last).min(last));
    if from > to {
        return Err(EngineError::BadParams(format!(
            "there {} {pages} page{}: \"from\" is past \"to\" or the last",
            if pages == 1 { "is" } else { "are" },
            if pages == 1 { "" } else { "s" }
        )));
    }
    let pdf = crate::print::write(&laid.printed, &laid.pictures, &title)?;
    let truncated = laid.printed.truncated;
    s.printer = Some(printer.clone());
    let spooler = std::sync::Arc::clone(&s.spooler);
    let count = to - from + 1;
    let what = format!("Printing {title:?} on {printer}");
    let sheets = crate::printers::Sheets { printer: printer.clone(), title, copies, from, to, laid, pdf };
    s.begin(what, wait, move |progress| {
        let sent = spooler.send(sheets, progress);
        Box::new(move |_s: &mut Session| match sent {
            Ok(()) => (
                format!(
                    "Sent {count} page{} to {printer}{}",
                    if count == 1 { "" } else { "s" },
                    if copies > 1 { format!(", {copies} copies") } else { String::new() }
                ),
                json!({"printer": printer, "pages": count, "copies": copies, "truncated": truncated, "style": kind}),
            ),
            Err(why) => (format!("{printer} did not take the print: {why}"), json!({"ok": false, "printer": printer, "error": why})),
        })
    })
}

/// Change how pages are set up for printing, and say how they are.
fn page_setup(s: &mut Session, p: &Value) -> CmdResult {
    let reset = p_bool(p, "reset") == Some(true);
    let from = if reset { openmailclient_layout::Setup::default() } else { s.page_setup.clone() };
    let setup = crate::print::setup_with(&from, p)?;
    let style = crate::print::style_with(if reset { crate::print::Style::default() } else { s.print_style }, p)?;
    let listing = crate::print::listing_with(&if reset { crate::print::Listing::default() } else { s.print_list.clone() }, p)?;
    if setup != s.page_setup || style != s.print_style || listing != s.print_list {
        s.page_setup = setup;
        s.print_style = style;
        s.print_list = listing;
        s.status = "Page setup changed".to_string();
    }
    let mut out = crate::print::setup_json(&s.page_setup);
    out["style"] = json!(s.print_style.name());
    crate::print::listing_json(&s.print_list, &mut out);
    Ok(out)
}

fn close_store(s: &mut Session, p: &Value) -> CmdResult {
    let idx = match p_u64(p, "store") {
        Some(i) => usize::try_from(i).unwrap_or(usize::MAX),
        None => s.current.map(|(i, _)| i).unwrap_or(0),
    };
    if idx >= s.stores.len() {
        return Err(EngineError::BadParams(format!("there is no data file {idx}")));
    }
    if !s.saves.is_empty() || !s.exports.is_empty() || !s.jobs.is_empty() {
        return Err(EngineError::BadParams(
            "a data file is being saved or exported, or mail is being sent; close when that is done, or stop it with file.stopSave".into(),
        ));
    }
    // Changes are not thrown away, and a file is not written, without being asked.
    if s.stores.get(idx).is_some_and(|st| st.unsaved()) {
        if p_bool(p, "save").unwrap_or(false) {
            save(s, &json!({ "store": idx }))?;
        } else if !p_bool(p, "discard").unwrap_or(false) {
            return Err(EngineError::BadParams(
                r#"the data file has unsaved changes: file.save first, or close it with {"save": true} or {"discard": true}"#.into(),
            ));
        } else if let Some(store) = s.stores.get_mut(idx) {
            // Thrown away for good: the journal of them goes too, so they do not come back.
            store.discard();
        }
    }
    s.stores.remove(idx);
    s.arrived.closed(idx);
    s.reminders.closed();
    s.calendar.chosen = match s.calendar.chosen {
        Some((c, f)) if c < idx => Some((c, f)),
        Some((c, f)) if c > idx => Some((c - 1, f)),
        _ => None,
    };
    // The steps name data files by their place in the list, which has just changed.
    s.undo.clear();
    if idx < s.folders.len() {
        s.folders.remove(idx);
    }
    let keep: Option<(usize, FolderId)> = match s.current {
        Some((c, f)) if c < idx => Some((c, f)),
        Some((c, f)) if c > idx => Some((c - 1, f)),
        _ => None,
    };
    match keep {
        Some(cur) => s.current = Some(cur),
        None => {
            s.current = None;
            s.selected = None;
            s.open = None;
            s.rows.clear();
            if !s.stores.is_empty() {
                s.select_default_folder(0);
            }
        }
    }
    ok()
}
