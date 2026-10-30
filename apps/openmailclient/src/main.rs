//! The OpenMailClient desktop app.
//!
//! ```text
//! openmailclient [--sample] [--dark] [--classic-ribbon] [--no-settings] [file.pst | file.ost | file.msg | file.eml | file.oft]...
//! ```
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod chime;
mod region;

use openmailclient_engine::Session;
use openmailclient_ui_egui::{MailUi, UiRequest};
use serde_json::{Value, json};
use std::process::ExitCode;

struct App {
    ui: MailUi,
    /// Where the settings are kept; `None` when they are not (`--no-settings`).
    settings: Option<std::path::PathBuf>,
    /// The settings as they were last written or read.
    kept: Value,
    /// When they were last written: a window being dragged changes them every frame, and
    /// the file is written no more than once a second.
    written: Option<std::time::Instant>,
    /// When the journals of unsaved changes were last forced to disk.
    settled: Option<std::time::Instant>,
    /// When the app started: the clock saving on its own goes by.
    started: std::time::Instant,
}

impl App {
    fn run(&mut self, id: &str, params: Value) {
        if let Err(e) = self.ui.execute(id, &params) {
            self.ui.session.status = e;
        }
    }

    /// Force the journals of unsaved changes to disk, no more than once a second: a power
    /// cut then costs the last second of changes at most.
    fn settle(&mut self) {
        if self.settled.is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(1)) {
            return;
        }
        if self.ui.session.settle() {
            self.settled = Some(std::time::Instant::now());
        }
    }

    /// Write the settings when they are not what the file has: when they change, not only on
    /// the way out, so that a run that ends badly keeps them too.
    /// Returns true when a change waits to be written.
    fn keep_settings(&mut self, at_once: bool) -> bool {
        let Some(path) = self.settings.clone() else { return false };
        let now = self.ui.session.settings(Some(&self.ui.settings()));
        if now == self.kept {
            return false;
        }
        if !at_once && self.written.is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(1)) {
            return true;
        }
        self.written = Some(std::time::Instant::now());
        // The status line is for what the person did, not for housekeeping: a failure to
        // write settings is not shown over it, and is tried again at the next change.
        let status = self.ui.session.status.clone();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = self.ui.execute("settings.save", &json!({ "path": path.display().to_string() }));
        self.ui.session.status = status;
        self.kept = now;
        false
    }

    /// Native dialogs for what the UI asked for. Only a user action queues these.
    fn serve(&mut self, ctx: &egui::Context) {
        for request in std::mem::take(&mut self.ui.requests) {
            match request {
                UiRequest::OpenFile => {
                    let picked = rfd::FileDialog::new()
                        .set_title("Open data file or message")
                        .add_filter("Mail files", &["pst", "ost", "msg", "eml", "oft"])
                        .add_filter("Outlook data files", &["pst", "ost"])
                        .add_filter("Messages", &["msg", "eml", "oft"])
                        .add_filter("All files", &["*"])
                        .pick_files();
                    for path in picked.unwrap_or_default() {
                        self.run("file.open", json!({ "path": path }));
                    }
                }
                UiRequest::Export => {
                    let session = &self.ui.session;
                    let name =
                        session.current.and_then(|(i, _)| session.stores.get(i)).map(|s| safe_file_name(&s.display_name())).unwrap_or_default();
                    let picked = rfd::FileDialog::new()
                        .set_title("Export to a data file")
                        .set_file_name(format!("{name}.pst"))
                        .add_filter("Outlook data file", &["pst"])
                        .save_file();
                    if let Some(path) = picked {
                        self.run("file.export", json!({ "path": path, "wait": false }));
                    }
                }
                UiRequest::SaveAs => {
                    let name = self.ui.session.open.as_ref().map(|it| safe_file_name(&it.subject())).unwrap_or_default();
                    let picked = rfd::FileDialog::new()
                        .set_title("Save message as")
                        .set_file_name(format!("{name}.msg"))
                        .add_filter("Outlook message", &["msg"])
                        .add_filter("Internet message", &["eml"])
                        .add_filter("Outlook template", &["oft"])
                        .save_file();
                    if let Some(path) = picked {
                        self.run("file.saveAs", json!({ "path": path }));
                    }
                }
                UiRequest::Print => {
                    let session = &self.ui.session;
                    let name = safe_file_name(&openmailclient_engine::print::title(session, session.print_style));
                    let picked = rfd::FileDialog::new()
                        .set_title("Print to PDF")
                        .set_file_name(format!("{name}.pdf"))
                        .add_filter("PDF document", &["pdf"])
                        .save_file();
                    if let Some(path) = picked {
                        self.run("file.print", json!({ "path": path }));
                    }
                }
                UiRequest::SaveAttachment(index) => {
                    let name = self
                        .ui
                        .session
                        .open
                        .as_ref()
                        .and_then(|it| it.attachments.get(index))
                        .map(openmailclient_engine::attachments::file_name)
                        .unwrap_or_default();
                    if let Some(path) = rfd::FileDialog::new().set_title("Save attachment").set_file_name(name).save_file() {
                        self.run("file.saveAttachment", json!({ "index": index, "path": path }));
                    }
                }
                UiRequest::SaveAllAttachments => {
                    if let Some(folder) = rfd::FileDialog::new().set_title("Save all attachments in").pick_folder() {
                        self.run("file.saveAllAttachments", json!({ "folder": folder }));
                    }
                }
                UiRequest::Attach => {
                    for path in rfd::FileDialog::new().set_title("Attach file").pick_files().unwrap_or_default() {
                        self.run("compose.attach", json!({ "path": path }));
                    }
                }
                UiRequest::OpenCalendar => {
                    let picked = rfd::FileDialog::new()
                        .set_title("Open calendar")
                        .add_filter("iCalendar file", &["ics", "ical", "ifb"])
                        .add_filter("All files", &["*"])
                        .pick_file();
                    if let Some(path) = picked
                        && let Err(e) = self.ui.execute("ui.openCalendar", &json!({ "path": path }))
                    {
                        self.ui.session.status = e;
                    }
                }
                UiRequest::SaveCalendar => {
                    let picked = rfd::FileDialog::new()
                        .set_title("Save calendar")
                        .set_file_name("Calendar.ics")
                        .add_filter("iCalendar file", &["ics"])
                        .save_file();
                    if let Some(path) = picked {
                        self.run("calendar.saveAs", json!({ "path": path }));
                    }
                }
                UiRequest::OpenCards => {
                    let picked = rfd::FileDialog::new()
                        .set_title("Open vCard file")
                        .add_filter("vCard file", &["vcf", "vcard"])
                        .add_filter("All files", &["*"])
                        .pick_file();
                    if let Some(path) = picked
                        && let Err(e) = self.ui.execute("ui.openCards", &json!({ "path": path }))
                    {
                        self.ui.session.status = e;
                    }
                }
                UiRequest::SaveCards(ids) => {
                    // One card is named after its contact; several after the folder.
                    let named = match ids.as_slice() {
                        [id] => self.ui.session.person(*id).ok().map(|c| c.name()),
                        _ => None,
                    };
                    let stem: String = named
                        .unwrap_or_else(|| "Contacts".to_string())
                        .chars()
                        .map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
                        .take(80)
                        .collect();
                    let stem = if stem.trim().is_empty() { "Contact".to_string() } else { stem.trim().to_string() };
                    let picked = rfd::FileDialog::new()
                        .set_title("Save as vCard")
                        .set_file_name(format!("{stem}.vcf"))
                        .add_filter("vCard file", &["vcf"])
                        .save_file();
                    if let Some(path) = picked {
                        let params = if ids.is_empty() { json!({ "path": path, "all": true }) } else { json!({ "path": path, "ids": ids }) };
                        self.run("people.saveAs", params);
                    }
                }
                UiRequest::ContactPicture => {
                    let picked = rfd::FileDialog::new()
                        .set_title("Add contact picture")
                        .add_filter("Picture", &["jpg", "jpeg", "png", "gif", "bmp"])
                        .add_filter("All files", &["*"])
                        .pick_file();
                    if let Some(path) = picked {
                        self.run("ui.contactPicture", json!({ "path": path }));
                    }
                }
                UiRequest::SaveMergeLetters => {
                    let picked = rfd::FileDialog::new()
                        .set_title("Save letters")
                        .set_file_name("Letters.pdf")
                        .add_filter("PDF document", &["pdf"])
                        .save_file();
                    if let Some(path) = picked {
                        self.run("ui.mergeLetters", json!({ "path": path }));
                    }
                }
                UiRequest::OpenTheme => {
                    let ext = openmailclient_ui_egui::THEME_FILE_EXT;
                    let picked = rfd::FileDialog::new()
                        .set_title("Open theme file")
                        .add_filter("Theme file", &[ext])
                        .add_filter("All files", &["*"])
                        .pick_file();
                    if let Some(path) = picked {
                        self.run("ui.openTheme", json!({ "path": path }));
                    }
                }
                UiRequest::ReminderSound => {
                    let picked = rfd::FileDialog::new()
                        .set_title("Reminder sound")
                        .add_filter("WAVE sound", &["wav", "wave"])
                        .add_filter("All files", &["*"])
                        .pick_file();
                    if let Some(path) = picked {
                        self.run("reminders.options", json!({ "soundFile": path }));
                    }
                }
                UiRequest::SaveTheme(name) => {
                    let ext = openmailclient_ui_egui::THEME_FILE_EXT;
                    let stem: String = name.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).take(80).collect();
                    let picked = rfd::FileDialog::new()
                        .set_title("Save theme")
                        .set_file_name(format!("{}.{ext}", stem.trim()))
                        .add_filter("Theme file", &[ext])
                        .save_file();
                    if let Some(path) = picked {
                        self.run("ui.saveTheme", json!({ "path": path, "name": name }));
                    }
                }
                UiRequest::OpenLink(address) => {
                    self.ui.session.status = match open_in_browser(&address) {
                        Ok(()) => "Opened in the browser".to_string(),
                        Err(e) => format!("The browser could not be started: {e}"),
                    };
                }
                UiRequest::OpenAttachment(path) => {
                    self.ui.session.status = match open_copy(&path) {
                        Ok(()) => format!("Opened {}", std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy()).unwrap_or_default()),
                        Err(e) => format!("The attachment could not be opened: {e}"),
                    };
                }
                UiRequest::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    }
}

/// Hand a web address to the system's browser. The address has been checked by the engine
/// (`mail.openLink`): `http` or `https`, plain characters only. It is checked once more here,
/// because this is where it leaves the program, and it is passed as one argument, never through
/// a shell.
fn open_in_browser(address: &str) -> std::io::Result<()> {
    use openmailclient_formats::links::{Target, target};
    let refused = |why: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, why.to_string());
    let address = match target(address) {
        Ok(Target::Page(address)) => address,
        Ok(Target::Mailbox(_)) => return Err(refused("not a web address")),
        Err(why) => return Err(refused(why)),
    };
    let mut command = if cfg!(target_os = "windows") {
        let mut command = std::process::Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };
    command.arg(address).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    command.spawn().map(|_| ())
}

/// Hand a copy of an attachment to the program the system opens such files with. The copy was
/// made by the engine (`file.openAttachment`), which refuses programs and scripts; it is checked
/// once more here to be a file in the program's own folder of copies, and is passed as one
/// argument, never through a shell.
fn open_copy(path: &str) -> std::io::Result<()> {
    let refused = |why: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, why.to_string());
    let path = std::path::Path::new(path);
    let folder = openmailclient_engine::attachments::opened_folder().ok_or_else(|| refused("there is no folder for copies"))?;
    if !path.starts_with(&folder) || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) || !path.is_file() {
        return Err(refused("not a copy this program made"));
    }
    let mut command = if cfg!(target_os = "windows") {
        let mut command = std::process::Command::new("rundll32");
        command.arg("shell32.dll,ShellExec_RunDLL");
        command
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };
    command.arg(path).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    command.spawn().map(|_| ())
}

/// A file name suggestion from untrusted text: no path separators, reserved characters or dots
/// at the ends, and a sensible length.
fn safe_file_name(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .take(80)
        .map(|c| if c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() { "message".to_string() } else { trimmed.to_string() }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui.ui(ui);
        let ctx = ui.ctx().clone();
        // Closing the window with unsaved changes asks once before they are thrown away.
        if ctx.input(|i| i.viewport().close_requested()) && !self.ui.may_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        let ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        // Asked to close while mail was coming: closed as soon as it has stopped.
        if self.ui.close_now(ms) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if self.ui.leaving() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        self.serve(&ctx);
        self.settle();
        // Unsaved changes that have waited long enough are saved on their own.
        if self.ui.session.autosave(ms) {
            ctx.request_repaint();
        }
        self.ui.session.tick_clock();
        // The newest release is asked after on its own once a day; that only ever tells.
        if self.ui.session.update_due() {
            self.run("help.checkForUpdates", json!({ "auto": true, "wait": false }));
        }
        // Mail is sent and received on its own every so often, counted from the last time.
        if let Some(left) = self.ui.session.send_receive_due(ms) {
            ctx.request_repaint_after(std::time::Duration::from_millis(left.clamp(50, 60_000)));
        }
        // Reminders are looked for once a minute, soon after something was done, and when the
        // next one comes due.
        if let Some(left) = self.ui.session.reminders_due(ms) {
            ctx.request_repaint_after(std::time::Duration::from_millis(left));
        }
        if let Some(sound) = self.ui.take_reminder_sound() {
            chime::play_sound(sound);
        } else if self.ui.take_chimes() > 0 {
            chime::play();
        }
        if self.ui.session.unsaved() {
            // A change made a moment after the last forcing is forced a second later.
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        if self.keep_settings(false) {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }

    fn on_exit(&mut self) {
        // Work on threads of its own is stopped, not waited out.
        self.ui.session.leave_jobs(std::time::Duration::from_secs(2));
        self.ui.session.finish_saves();
        self.ui.session.settle();
        self.keep_settings(true);
        // The copies of attachments opened this run go with it.
        openmailclient_engine::attachments::clear_opened(true);
        // An update asked to be installed is run last, once everything is saved or kept: it
        // closes what is left of this run and starts the program again when it is done.
        if let (Some(ready), Some(dir)) = (self.ui.session.update.install.take(), self.ui.session.update_dir.clone())
            && let Err(e) = openmailclient_engine::update::launch(&ready, &dir)
        {
            eprintln!("openmailclient: {e}");
        }
    }
}

/// Where the settings are kept: the per-user configuration folder of the system.
fn settings_path() -> Option<std::path::PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    let dir = if cfg!(target_os = "windows") {
        var("APPDATA")?.join("OpenMailClient")
    } else if cfg!(target_os = "macos") {
        var("HOME")?.join("Library").join("Application Support").join("OpenMailClient")
    } else {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|home| home.join(".config")))?.join("openmailclient")
    };
    Some(dir.join("settings.json"))
}

/// Where updates are brought down: the per-user folder for what can be made again, as the
/// installer itself is kept.
fn updates_path() -> Option<std::path::PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    if cfg!(target_os = "windows") {
        Some(var("LOCALAPPDATA")?.join("OpenMailClient").join("Updates"))
    } else {
        settings_path().and_then(|p| p.parent().map(|dir| dir.join("Updates")))
    }
}

fn main() -> ExitCode {
    let mut session = Session::new();
    if let Err(e) = session.use_this_system() {
        eprintln!("openmailclient: mail servers cannot be reached: {e}");
    }
    session.set_system_zone(openmailclient_ui_egui::local_zone());
    session.region = region::of_this_system();
    // Copies of attachments an earlier run opened and could not clear away, a day on.
    openmailclient_engine::attachments::clear_opened(false);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let settings = if args.iter().any(|a| a == "--no-settings") { None } else { settings_path() };
    // Journals that cannot be kept beside their data file go beside the settings.
    session.journal_dir = settings_path().and_then(|p| p.parent().map(|dir| dir.join("journals")));
    // Each account's mail is kept in a data file of its own beside the settings, as Outlook
    // keeps one per account, apart from any data file opened.
    session.account_dir = settings_path().and_then(|p| p.parent().map(|dir| dir.join("Accounts")));
    session.update_dir = updates_path();
    // Outlook's profiles are read only when an import from Outlook is asked for.
    session.outlook = Some(std::sync::Arc::new(openmailclient_engine::outlook::system_profiles));
    session.outlook_signatures = openmailclient_engine::signatures::outlook_dir();
    let mut app = App { ui: MailUi::new(session), settings, kept: Value::Null, written: None, settled: None, started: std::time::Instant::now() };
    if let Some(path) = app.settings.clone() {
        // The data files of last time are opened again unless this run names what to open.
        let reopen = !args.iter().any(|a| a == "--sample" || !a.starts_with("--"));
        app.run("settings.load", json!({ "path": path.display().to_string(), "reopen": reopen }));
        app.kept = app.ui.session.settings(Some(&app.ui.settings()));
    }
    for arg in args {
        match arg.as_str() {
            "--no-settings" => {}
            "--sample" => app.run("file.openSample", Value::Null),
            "--dark" => app.run("ui.theme", json!({ "theme": "dark" })),
            "--classic-ribbon" => app.run("ui.ribbonMode", json!({ "mode": "classic" })),
            "--help" | "-h" => {
                println!(
                    "openmailclient [--sample] [--dark] [--classic-ribbon] [--no-settings] [file.pst | file.ost | file.msg | file.eml | file.oft]..."
                );
                return ExitCode::SUCCESS;
            }
            flag if flag.starts_with("--") => {
                eprintln!("openmailclient: unknown option {flag}");
                return ExitCode::FAILURE;
            }
            path => app.run("file.open", json!({ "path": path })),
        }
    }
    // The window stands where it stood last time; the first frames see that it can be reached.
    let place = app.ui.window;
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("OpenMailClient")
        .with_inner_size(place.map_or(openmailclient_ui_egui::FIRST_WINDOW, |w| w.size))
        .with_min_inner_size(openmailclient_ui_egui::MIN_WINDOW)
        .with_maximized(place.is_some_and(|w| w.maximized))
        .with_icon(egui::IconData { rgba: openmailclient_ui_egui::app_icon::rgba(64), width: 64, height: 64 });
    if let Some(pos) = place.and_then(|w| w.pos) {
        viewport = viewport.with_position(pos);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    let creator = move |cc: &eframe::CreationContext<'_>| -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
        // A watched Inbox that heard of new mail wakes the window, which receives it at once.
        let ctx = cc.egui_ctx.clone();
        app.ui.session.waker = Some(std::sync::Arc::new(move || ctx.request_repaint()));
        Ok(Box::new(app))
    };
    match eframe::run_native("OpenMailClient", options, Box::new(creator)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("openmailclient: {e}");
            ExitCode::FAILURE
        }
    }
}
