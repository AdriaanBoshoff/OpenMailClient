//! Check for Updates: the newest release asked after on GitHub, its installer brought down and
//! checked on a thread of its own, and run once the program has closed (`crate::update`).

use super::{CommandSpec, always, p_bool};
use crate::update::{self, Ready, Release};
use crate::{CmdResult, EngineError, Session};
use openmailclient_net::http;
use serde_json::{Value, json};

use crate::update::{CHECKING, DOWNLOADING};

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "help.checkForUpdates",
            label: "Check for Updates",
            tab: "Help",
            group: "Help",
            shortcut: "",
            params: r#"{"wait": false to ask on a thread of its own, "auto": true to ask only when a day has gone by since the last answer and updates are asked after on their own}; asks GitHub for this program's releases; returns {"current", "newer", "untrusted_content": {"release": {"version", "name", "notes", "prerelease", "page", "published", "size"}}}"#,
            enabled: always,
            run: check,
        },
        CommandSpec {
            id: "help.downloadUpdate",
            label: "Download Update",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"wait": false to bring it down on a thread of its own}; brings down the installer of the release Check for Updates found and checks it against the sums published with it; nothing is run"#,
            enabled: |s| s.update.found.as_ref().is_some_and(|r| r.setup.is_some()),
            run: download,
        },
        CommandSpec {
            id: "help.installUpdate",
            label: "Install Update",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"cancel": true to take the asking back}; the update brought down is installed when the program closes, after every change is saved or kept; Windows only"#,
            enabled: |s| s.update.ready.is_some(),
            run: install,
        },
        CommandSpec {
            id: "help.updateOptions",
            label: "Update Options",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"prereleases": true to be offered pre-releases too, "automatic": false to ask after updates only when told}"#,
            enabled: always,
            run: options,
        },
    ]
}

/// What is said of a release; its words are the project's, given as content.
fn told(r: &Release) -> Value {
    json!({
        "version": r.version.to_string(),
        "name": r.name,
        "notes": r.notes,
        "prerelease": r.prerelease,
        "page": r.page,
        "published": r.published,
        "size": r.setup.as_ref().map(|a| a.size),
        "installer": r.setup.is_some(),
    })
}

fn unix_now(s: &Session) -> i64 {
    openmailclient_model::time::to_unix(s.now)
}

impl Session {
    /// Whether the newest release is to be asked after on its own now: once a day while that
    /// is on, an hour after a try that failed, and not while it is being asked or brought down.
    pub fn update_due(&self) -> bool {
        let now = unix_now(self);
        self.update.automatic
            && self.connector.is_some()
            && now.saturating_sub(self.update.last_checked) >= update::CHECK_EVERY_SECS
            && self.update.last_tried.is_none_or(|t| now.saturating_sub(t) >= update::RETRY_SECS)
            && !self.jobs.iter().any(|j| j.what == CHECKING || j.what == DOWNLOADING)
    }
}

fn check(s: &mut Session, p: &Value) -> CmdResult {
    let auto = p_bool(p, "auto") == Some(true);
    if auto && !s.update_due() {
        return Ok(json!({ "started": false }));
    }
    if s.jobs.iter().any(|j| j.what == CHECKING) {
        return Ok(json!({ "started": false, "said": "updates are being checked for" }));
    }
    let connector = s.connector.clone().ok_or_else(|| EngineError::Disabled("Check for Updates (no network)".into()))?;
    s.update.last_tried = Some(unix_now(s));
    let pre = s.update.prereleases;
    s.begin(CHECKING.to_string(), p_bool(p, "wait") != Some(false), move |_| {
        let got = http::get_as(&*connector, update::RELEASES, update::MAX_LIST, "application/vnd.github+json")
            .map_err(|e| e.to_string())
            .and_then(|f| update::read_releases(&f.bytes));
        Box::new(move |s: &mut Session| {
            let releases = match got {
                Ok(releases) => releases,
                Err(e) => {
                    let said = format!("Updates could not be checked for: {e}");
                    s.update.failed = Some(said.clone());
                    // Asked on its own: the status line is for what the person did.
                    let status = if auto { s.status.clone() } else { said };
                    return (status, json!({ "ok": false, "said": e }));
                }
            };
            s.update.last_checked = unix_now(s);
            s.update.checked = true;
            s.update.failed = None;
            let current = update::current();
            let found = update::newest(&releases, &current, pre).cloned();
            let status = match &found {
                Some(r) => format!("OpenMailClient {} is available: Help, Check for Updates", r.version),
                None if auto => s.status.clone(),
                None => format!("OpenMailClient {current} is the newest"),
            };
            // A newer find makes way for an installer brought down for an older one.
            if s.update.ready.as_ref().is_some_and(|r| found.as_ref().is_some_and(|f| f.version != r.version)) {
                s.update.ready = None;
            }
            let value = json!({
                "current": current.to_string(),
                "newer": found.is_some(),
                "untrusted_content": { "release": found.as_ref().map(told) },
            });
            s.update.found = found;
            (status, value)
        })
    })
}

fn download(s: &mut Session, p: &Value) -> CmdResult {
    let release = s.update.found.clone().ok_or_else(|| EngineError::BadParams("no newer release has been found; check for updates first".into()))?;
    let setup = release
        .setup
        .clone()
        .ok_or_else(|| EngineError::Disabled(format!("There is no installer for Windows in {}; see {}", release.version, release.page)))?;
    let sums = release.sums.clone().ok_or_else(|| EngineError::Disabled(format!("{} has no sums to check its installer by", release.version)))?;
    if s.update.ready.as_ref().is_some_and(|r| r.version == release.version && r.path.is_file()) {
        return Ok(json!({ "started": false, "said": "the update is ready" }));
    }
    if s.jobs.iter().any(|j| j.what == DOWNLOADING) {
        return Ok(json!({ "started": false, "said": "the update is being downloaded" }));
    }
    if setup.size > update::MAX_SETUP {
        return Err(EngineError::Format("the installer is larger than any of ours".into()));
    }
    let dir = s.update_dir.clone().ok_or_else(|| EngineError::Disabled("Downloading updates (no folder for them)".into()))?;
    let connector = s.connector.clone().ok_or_else(|| EngineError::Disabled("Downloading updates (no network)".into()))?;
    s.begin(DOWNLOADING.to_string(), p_bool(p, "wait") != Some(false), move |progress| {
        progress.begin(setup.size.max(1));
        let got = (|| -> Result<Ready, String> {
            std::fs::create_dir_all(&dir).map_err(|e| format!("the folder for updates could not be made: {e}"))?;
            // What earlier runs brought down goes: only the newest is kept.
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten().take(1000) {
                    let path = entry.path();
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                    if name.starts_with("OpenMailClient-") && (name.ends_with("-setup.exe") || name.ends_with(".part")) {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
            let mut text = Vec::new();
            http::download(&*connector, &sums.url, update::MAX_SUMS, &mut text, &mut |_, _| !progress.stopped()).map_err(|e| e.to_string())?;
            let want = update::sum_for(&String::from_utf8_lossy(&text), &setup.name)
                .ok_or("the sums published with the release do not name its installer")?;
            let path = dir.join(&setup.name);
            let part = dir.join(format!("{}.part", setup.name));
            let file = std::fs::File::create(&part).map_err(|e| format!("the update could not be written: {e}"))?;
            let mut into = update::Summed { file, hasher: sha2::Digest::new() };
            let came = http::download(&*connector, &setup.url, update::MAX_SETUP, &mut into, &mut |n, _| {
                progress.reach(n);
                !progress.stopped()
            });
            let finish = (|| {
                let n = came.map_err(|e| e.to_string())?;
                into.file.sync_all().map_err(|e| e.to_string())?;
                if setup.size > 0 && n != setup.size {
                    return Err(format!("the installer came to {n} bytes, not the {} GitHub said", setup.size));
                }
                let sum: [u8; 32] = sha2::Digest::finalize(into.hasher).into();
                if sum != want {
                    return Err("the installer is not the one the release's sums name; it was thrown away".into());
                }
                Ok(sum)
            })();
            let sum = match finish {
                Ok(sum) => sum,
                Err(e) => {
                    let _ = std::fs::remove_file(&part);
                    return Err(e);
                }
            };
            std::fs::rename(&part, &path).map_err(|e| {
                let _ = std::fs::remove_file(&part);
                format!("the update could not be kept: {e}")
            })?;
            Ok(Ready { path, sha256: sum, version: release.version.clone() })
        })();
        Box::new(move |s: &mut Session| match got {
            Ok(ready) => {
                let status = format!("OpenMailClient {} is downloaded and checked, and is installed when the program closes", ready.version);
                let value = json!({ "ready": true, "version": ready.version.to_string(), "sha256": update::hex(&ready.sha256) });
                s.update.ready = Some(ready);
                s.update.failed = None;
                (status, value)
            }
            Err(e) => {
                let said = format!("The update could not be downloaded: {e}");
                s.update.failed = Some(said.clone());
                (said, json!({ "ok": false, "said": e }))
            }
        })
    })
}

fn install(s: &mut Session, p: &Value) -> CmdResult {
    if p_bool(p, "cancel") == Some(true) {
        s.update.install = None;
        s.status = "The update is not installed when the program closes".into();
        return Ok(json!({ "installOnClose": false }));
    }
    if !cfg!(windows) {
        return Err(EngineError::Disabled("Install Update (only on Windows; other systems update from the release page)".into()));
    }
    let ready = s.update.ready.clone().ok_or_else(|| EngineError::BadParams("no update has been downloaded".into()))?;
    let dir = s.update_dir.clone().ok_or_else(|| EngineError::Disabled("Install Update (no folder for updates)".into()))?;
    if !update::is_ours(&ready.path, &dir) {
        return Err(EngineError::BadParams("that is not an update this program brought down".into()));
    }
    // Checked again here and once more as it is run: what is run is what was checked.
    if update::sum_of_file(&ready.path).ok() != Some(ready.sha256) {
        s.update.ready = None;
        return Err(EngineError::Format("the update has changed since it was checked; download it again".into()));
    }
    s.status = format!("OpenMailClient {} is installed when the program closes", ready.version);
    let version = ready.version.to_string();
    s.update.install = Some(ready);
    Ok(json!({ "installOnClose": true, "version": version }))
}

fn options(s: &mut Session, p: &Value) -> CmdResult {
    if let Some(v) = p.get("prereleases") {
        let pre = v.as_bool().ok_or_else(|| EngineError::BadParams("\"prereleases\" must be true or false".into()))?;
        if pre != s.update.prereleases {
            s.update.prereleases = pre;
            // What was found was found for the other choice.
            s.update.found = None;
            s.update.checked = false;
        }
    }
    if let Some(v) = p.get("automatic") {
        s.update.automatic = v.as_bool().ok_or_else(|| EngineError::BadParams("\"automatic\" must be true or false".into()))?;
    }
    Ok(json!({ "prereleases": s.update.prereleases, "automatic": s.update.automatic }))
}
