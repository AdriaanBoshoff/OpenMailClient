//! Serving from a console or as a service (M7.1, M7.5): what `openmailclient serve` and
//! `openmailclient-cli web serve` share. [`run`] starts the server, says where it is reached
//! and how its certificate is checked, and runs until Ctrl+C or the service manager's stop
//! (SIGTERM), when the session saves what is running and its journals keep the rest, as the
//! window does when it closes. [`Hosted`] is the owner's own mailbox, opened from the settings as
//! the window opens it and keeping the settings as it changes them.

use crate::server;
use openmailclient_engine::{Session, places};
use openmailclient_mcp::{Backend, Headless};
use openmailclient_net::sealed::Choice;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// What the console asked for.
#[derive(Default, Debug, Clone)]
pub struct Plan {
    /// `address:port`; 127.0.0.1:8443 when not given.
    pub listen: Option<String>,
    pub names: Vec<String>,
    pub web_dir: Option<PathBuf>,
    pub new_certificate: bool,
}

/// The port web access listens on unless told otherwise.
pub const PORT: u16 = 8443;

/// The listening address asked for, or this machine only.
pub fn listen_of(text: Option<&str>) -> Result<std::net::SocketAddr, String> {
    let text = text.unwrap_or("127.0.0.1:8443");
    text.parse().map_err(|e| format!("{text}: {e}; give it as 127.0.0.1:{PORT}, 0.0.0.0:{PORT} or [::]:{PORT}"))
}

/// Serve until stopped. `about` is said once it is listening, after where it is reached.
pub fn run<B: Backend + 'static>(
    dir: &Path,
    plan: Plan,
    make: impl FnOnce() -> Result<B, String> + Send + 'static,
    about: &[String],
) -> Result<(), String> {
    let listen = listen_of(plan.listen.as_deref())?;
    if let Some(web) = &plan.web_dir
        && !web.join("index.html").is_file()
    {
        return Err(format!("{}: there is no index.html; `cargo xtask web` builds it", web.display()));
    }
    // Ctrl+C at a console, and the stop of systemd or another service manager (SIGTERM).
    let (stop_tx, stop_rx) = std::sync::mpsc::sync_channel::<()>(1);
    ctrlc::set_handler(move || {
        let _ = stop_tx.try_send(());
    })
    .map_err(|e| format!("cannot be told to stop: {e}"))?;
    let options = server::Options { listen, names: plan.names, dir: dir.to_path_buf(), web_dir: plan.web_dir, new_certificate: plan.new_certificate };
    let running = server::serve(options, make)?;
    for line in announce(&running.started, dir) {
        println!("{line}");
    }
    for line in about {
        println!("{line}");
    }
    println!("Stop with Ctrl+C.");
    let _ = stop_rx.recv();
    println!("Stopping: what is being saved is finished first.");
    running.stop();
    println!("Stopped.");
    Ok(())
}

/// The owner's own mailbox in the settings' folder `dir`, served until stopped: what
/// `openmailclient serve` does. It holds the folder, so no window opens the same data files
/// meanwhile, and keeps mail account passwords as `passwords` says.
pub fn serve_profile(dir: &Path, plan: Plan, passwords: Choice) -> Result<(), String> {
    places::name_this_run("openmailclient serve");
    let _hold = places::hold(dir)?;
    let vault = std::sync::Arc::new(places::vault(dir, passwords));
    let about = vec![format!("Mail account passwords are kept {}.", vault.says())];
    let settings = dir.join("settings.json");
    let make = move || {
        let mut session = Session::new();
        if let Err(e) = session.use_this_system() {
            eprintln!("openmailclient: mail servers cannot be reached: {e}");
        }
        session.vault = vault;
        session.set_system_zone(openmailclient_engine::local_zone());
        Hosted::open(session, settings, true)
    };
    run(dir, plan, make, &about)
}

/// What is said once it listens: where it is reached and how to check the certificate.
pub fn announce(s: &server::Started, dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if s.new_certificate {
        out.push("A new certificate was made; browsers will ask about it once.".to_string());
    }
    out.push(format!("Web access is on, listening on {}.", s.listen));
    let port = s.listen.port();
    for n in &s.names {
        out.push(format!("  https://{}:{port}/", if n.contains(':') { format!("[{n}]") } else { n.clone() }));
    }
    out.push("The browser will warn that it does not know the certificate. Go on only if it shows this fingerprint:".to_string());
    out.push(format!("  SHA-256 {}", s.fingerprint));
    let ip = s.listen.ip();
    if ip.is_loopback() {
        out.push(format!("Only this machine can reach it. To reach it from other devices, start it with --listen 0.0.0.0:{port}."));
    } else if ip.is_unspecified() {
        out.push(
            "Every network this machine is on can reach it. A port forwarded to it from the internet makes the sign-in page public.".to_string(),
        );
    } else if !server::is_private(ip) {
        out.push(format!(
            "WARNING: {ip} is a public address. Anyone on the internet can reach the sign-in page; only the password and the bans keep them out."
        ));
    }
    out.push(format!("Sign-ins are logged in {}.", server::log_file(dir).display()));
    out
}

/// The owner's mailbox as the window opens it, kept in the settings as it changes.
pub struct Hosted {
    pub inner: Headless,
    settings: PathBuf,
    ui: Value,
    kept: Value,
    written_ms: Option<u64>,
}

/// Settings changed are written at most this often, and at once when it stops.
const KEEP_EVERY_MS: u64 = 5_000;

impl Hosted {
    /// Open the settings at `settings` (`settings.json` in the settings' folder) into
    /// `session`, with every data file of last time and each account's own: the session's
    /// journals and accounts' data files are kept beside them, as the window keeps them.
    pub fn open(mut session: Session, settings: PathBuf, reach_out: bool) -> Result<Hosted, String> {
        let dir = settings.parent().map(Path::to_path_buf).ok_or("the settings are in no folder")?;
        session.journal_dir = Some(dir.join("journals"));
        session.account_dir = Some(dir.join("Accounts"));
        let loaded = session.run("settings.load", &json!({ "path": settings.display().to_string(), "reopen": true })).map_err(|e| e.to_string())?;
        // What the window keeps for itself is kept as it was.
        let ui = loaded.get("ui").cloned().unwrap_or(Value::Null);
        let kept = session.settings(Some(&ui));
        Ok(Hosted { inner: Headless { session, reach_out }, settings, ui, kept, written_ms: None })
    }

    fn keep(&mut self, now_ms: Option<u64>) {
        let now = self.inner.session.settings(Some(&self.ui));
        if now == self.kept {
            return;
        }
        if let (Some(at), Some(was)) = (now_ms, self.written_ms)
            && at.saturating_sub(was) < KEEP_EVERY_MS
        {
            return;
        }
        self.written_ms = now_ms;
        let s = &mut self.inner.session;
        let status = s.status.clone();
        if let Some(dir) = self.settings.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = s.run("settings.save", &json!({ "path": self.settings.display().to_string(), "ui": self.ui }));
        s.status = status;
        self.kept = now;
    }
}

impl Backend for Hosted {
    fn commands(&mut self) -> Result<Value, String> {
        self.inner.commands()
    }

    fn execute(&mut self, command: &str, params: &Value) -> Result<Value, String> {
        self.inner.execute(command, params)
    }

    fn state(&mut self) -> Result<Value, String> {
        self.inner.state()
    }

    fn tick(&mut self, now_ms: u64) {
        self.inner.tick(now_ms);
        self.keep(Some(now_ms));
    }

    fn finish(&mut self) {
        self.inner.finish();
        self.keep(None);
        // The copies of attachments opened this run go with it.
        openmailclient_engine::attachments::clear_opened(true);
    }
}
