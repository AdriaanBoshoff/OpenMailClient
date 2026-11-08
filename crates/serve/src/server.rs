//! The web access server (M7.1): HTTPS only, from this machine, to browsers signed in with the
//! web password.
//!
//! - **TLS** with a certificate of its own ([`crate::tls`]); a browser that speaks plain HTTP to
//!   the port is told to come back over HTTPS and nothing else.
//! - **Names:** a request's `Host` must be one this server answers to: a name it was given, or the
//!   very address the connection came in on. A web page elsewhere that points a name of its own at
//!   this machine (DNS rebinding) gets 421 and nothing more.
//! - **Origin:** every form posted and every WebSocket opened must come from a page of this
//!   server's own; the session cookie is `SameSite=Strict` besides.
//! - **Sign-in:** [`crate::guard`] is asked before the password is checked, waits are served on
//!   the connection's own thread, and at most two Argon2 checks run at once so a flood of
//!   sign-ins cannot take the machine's memory.
//! - **Mail:** a session of its own on one thread ([`openmailclient_mcp::Backend`]), reached from
//!   connections by a channel; the pages here are drawn on the host with every word from a
//!   message escaped, and `/ws` carries the same commands as the control channel and MCP.
//! - **Bounds:** connections, connections per address, requests per connection, header and body
//!   sizes, and every wait.

use crate::guard::{self, Failed, Guard, Verdict};
use crate::http::{self, Bad, Request, Response, escape};
use crate::sessions::{COOKIE, Sessions};
use crate::{password, tls, ws};
use openmailclient_mcp::Backend;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

/// The most connections at once, and from one address.
pub const MAX_CONNECTIONS: usize = 64;
pub const MAX_PER_ADDRESS: usize = 16;
/// The most requests on one connection before it is closed.
const MAX_REQUESTS: usize = 200;
/// How long a TLS handshake may take, and how long a connection may wait between requests.
const HANDSHAKE: Duration = Duration::from_secs(10);
const IDLE: Duration = Duration::from_secs(30);
/// How long a write may wait on a browser that reads nothing.
const WRITE: Duration = Duration::from_secs(30);
/// A WebSocket is pinged after this long without a word, and closed after four times it.
const WS_QUIET: Duration = Duration::from_secs(20);
/// How long a command may take before the browser is told it is still running.
const COMMAND_WAIT: Duration = Duration::from_secs(120);
/// The most Argon2 checks at once (each takes about 19 MiB).
const VERIFYING: usize = 2;
/// The sign-in log grows to this, then is moved aside once.
const LOG_MAX: u64 = 1 << 20;
/// The most of one file of the web build served.
const MAX_STATIC: u64 = 64 << 20;
/// Messages listed on one page of a folder.
const PAGE: u64 = 50;

/// How the server is started.
#[derive(Clone, Debug)]
pub struct Options {
    /// Where it listens; `127.0.0.1:8443` unless the owner names another.
    pub listen: SocketAddr,
    /// Names and addresses it answers to besides `localhost` and its own addresses.
    pub names: Vec<String>,
    /// The settings' folder, where the password, the bans, the certificate and the log are.
    pub dir: PathBuf,
    /// The web build (`cargo xtask web`), served under `/app/` when given.
    pub web_dir: Option<PathBuf>,
    /// Make a new certificate even when the one kept would do.
    pub new_certificate: bool,
}

/// What `serve` tells its caller once it is listening.
#[derive(Clone, Debug)]
pub struct Started {
    pub listen: SocketAddr,
    pub names: Vec<String>,
    pub fingerprint: String,
    pub new_certificate: bool,
}

/// The file that says the server is running, where and with which certificate.
pub fn serving_file(dir: &Path) -> PathBuf {
    dir.join("web-serving.json")
}

/// The sign-in log.
pub fn log_file(dir: &Path) -> PathBuf {
    dir.join("web-sign-ins.log")
}

/// A question for the session's own thread.
enum Ask {
    Commands,
    Execute(String, Value),
    State,
}

struct Job {
    ask: Ask,
    reply: SyncSender<Result<Value, String>>,
}

/// A count that waits when full.
struct Slots {
    used: Mutex<usize>,
    freed: Condvar,
    most: usize,
}

impl Slots {
    fn take(&self) -> SlotGuard<'_> {
        let mut used = lock(&self.used);
        while *used >= self.most {
            used = match self.freed.wait(used) {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
        *used += 1;
        SlotGuard(self)
    }
}

struct SlotGuard<'a>(&'a Slots);

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        let mut used = lock(&self.0.used);
        *used = used.saturating_sub(1);
        self.0.freed.notify_one();
    }
}

/// A lock that a panic elsewhere does not poison for good: what it guards is plain data.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// The password's setting, read again when its file changes.
#[derive(Default)]
struct PasswordSeen {
    modified: Option<SystemTime>,
    kept: Option<password::Kept>,
}

struct Shared {
    dir: PathBuf,
    port: u16,
    /// `Host` values answered, lower case, with and without the port.
    hosts: Vec<String>,
    tls: Arc<rustls::ServerConfig>,
    web_dir: Option<PathBuf>,
    sessions: Mutex<Sessions>,
    guard: Mutex<Guard>,
    bans_seen: Mutex<Option<SystemTime>>,
    password: Mutex<PasswordSeen>,
    verifying: Slots,
    jobs: SyncSender<Job>,
    connections: AtomicUsize,
    per_address: Mutex<HashMap<IpAddr, usize>>,
    log: Mutex<()>,
    stop: AtomicBool,
}

/// A server running on threads of its own.
pub struct Running {
    pub started: Started,
    shared: Arc<Shared>,
    accept: Option<std::thread::JoinHandle<()>>,
}

impl Running {
    /// Stop listening; connections open end as their browsers leave.
    pub fn stop(mut self) {
        self.halt();
    }

    /// Wait until the server stops (it does not on its own).
    pub fn wait(mut self) {
        if let Some(t) = self.accept.take() {
            let _ = t.join();
        }
    }

    fn halt(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // Wake the accepting thread.
        let wake = match self.started.listen.ip() {
            ip if ip.is_unspecified() && ip.is_ipv4() => SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), self.started.listen.port()),
            ip if ip.is_unspecified() => SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), self.started.listen.port()),
            _ => self.started.listen,
        };
        let _ = TcpStream::connect_timeout(&wake, Duration::from_secs(1));
        if let Some(t) = self.accept.take() {
            let _ = t.join();
        }
        let _ = std::fs::remove_file(serving_file(&self.shared.dir));
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if self.accept.is_some() {
            self.halt();
        }
    }
}

/// The addresses this machine would send from, found by asking the system for a route to an
/// address kept for documentation (RFC 5737, RFC 3849); a UDP socket that is only connected
/// sends nothing.
pub fn own_addresses() -> Vec<IpAddr> {
    let mut out = Vec::new();
    for (bind, to) in [("0.0.0.0:0", "192.0.2.1:9"), ("[::]:0", "[2001:db8::1]:9")] {
        if let Ok(socket) = UdpSocket::bind(bind)
            && socket.connect(to).is_ok()
            && let Ok(local) = socket.local_addr()
            && !local.ip().is_unspecified()
        {
            out.push(local.ip());
        }
    }
    out
}

/// Whether `ip` is reached only from this machine or a home or office network.
pub fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local() || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64),
        IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80,
    }
}

/// The names the certificate is made for and `Host` is answered to.
fn names_for(options: &Options) -> Vec<String> {
    let mut names = vec!["localhost".to_string(), "127.0.0.1".to_string(), "::1".to_string()];
    let ip = options.listen.ip();
    if ip.is_unspecified() {
        names.extend(own_addresses().iter().map(IpAddr::to_string));
    } else if !ip.is_loopback() {
        names.push(ip.to_string());
    }
    for n in &options.names {
        let n = n.trim().trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
        if !n.is_empty() && !names.contains(&n) {
            names.push(n);
        }
    }
    names
}

fn host_values(names: &[String], port: u16) -> Vec<String> {
    let mut out = Vec::new();
    for n in names {
        let shown = if n.contains(':') { format!("[{n}]") } else { n.clone() };
        out.push(format!("{shown}:{port}"));
        if port == 443 {
            out.push(shown);
        }
    }
    out
}

/// Start serving: a certificate is made or read, the session is made on a thread of its own by
/// `make`, and connections are taken on another. The web password must be set first.
pub fn serve<B: Backend + 'static>(options: Options, make: impl FnOnce() -> Result<B, String> + Send + 'static) -> Result<Running, String> {
    if password::load(&options.dir)?.is_none() {
        return Err("there is no web password yet; set one with `openmailclient-cli web password` first".to_string());
    }
    let names = names_for(&options);
    let now = crate::now_ms();
    let (kept, new_certificate) = tls::ensure(&options.dir, &names, options.new_certificate, now)?;
    let config = kept.server_config()?;
    let listener = TcpListener::bind(options.listen).map_err(|e| format!("{}: {e}", options.listen))?;
    let listen = listener.local_addr().map_err(|e| e.to_string())?;
    let guard = Guard::load(&options.dir)?;
    let bans_seen = modified(&crate::bans_file(&options.dir));

    // The session lives on one thread and is reached by a channel, so nothing of it is shared.
    let (jobs, inbox) = sync_channel::<Job>(64);
    let (made_tx, made_rx) = sync_channel::<Result<(), String>>(1);
    std::thread::Builder::new()
        .name("web-session".into())
        .spawn(move || match make() {
            Ok(backend) => {
                let _ = made_tx.send(Ok(()));
                session_thread(backend, inbox);
            }
            Err(e) => {
                let _ = made_tx.send(Err(e));
            }
        })
        .map_err(|e| format!("a thread for the session: {e}"))?;
    made_rx.recv().map_err(|_| "the session could not be made".to_string())??;

    let shared = Arc::new(Shared {
        dir: options.dir.clone(),
        port: listen.port(),
        hosts: host_values(&names, listen.port()),
        tls: config,
        web_dir: options.web_dir.clone(),
        sessions: Mutex::new(Sessions::new()),
        guard: Mutex::new(guard),
        bans_seen: Mutex::new(bans_seen),
        password: Mutex::new(PasswordSeen::default()),
        verifying: Slots { used: Mutex::new(0), freed: Condvar::new(), most: VERIFYING },
        jobs,
        connections: AtomicUsize::new(0),
        per_address: Mutex::new(HashMap::new()),
        log: Mutex::new(()),
        stop: AtomicBool::new(false),
    });
    let started = Started { listen, names: names.clone(), fingerprint: kept.fingerprint(), new_certificate };
    let serving = json!({
        "about": "OpenMailClient web access is running (removed when it stops).",
        "listen": listen.to_string(),
        "names": names,
        "fingerprint": started.fingerprint,
        "pid": std::process::id(),
        "started": now,
    });
    let _ = crate::write_private(&serving_file(&options.dir), &serving.to_string());
    let s = shared.clone();
    let accept = std::thread::Builder::new()
        .name("web-accept".into())
        .spawn(move || accept_loop(&s, &listener))
        .map_err(|e| format!("a thread to take connections: {e}"))?;
    Ok(Running { started, shared, accept: Some(accept) })
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn session_thread<B: Backend>(mut backend: B, inbox: Receiver<Job>) {
    let began = Instant::now();
    let mut ticked = Instant::now();
    loop {
        match inbox.recv_timeout(Duration::from_secs(1)) {
            Ok(job) => {
                let answer = match job.ask {
                    Ask::Commands => backend.commands(),
                    Ask::Execute(command, params) => backend.execute(&command, &params),
                    Ask::State => backend.state(),
                };
                let _ = job.reply.send(answer);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if ticked.elapsed() >= Duration::from_secs(1) {
            ticked = Instant::now();
            backend.tick(u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX));
        }
    }
}

fn accept_loop(shared: &Arc<Shared>, listener: &TcpListener) {
    for stream in listener.incoming() {
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else {
            // Out of descriptors or a connection that went before it was taken: wait a moment.
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        let Ok(peer) = stream.peer_addr() else { continue };
        let ip = plain_ip(peer.ip());
        if shared.connections.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            continue;
        }
        {
            let mut per = lock(&shared.per_address);
            let n = per.entry(ip).or_insert(0);
            if *n >= MAX_PER_ADDRESS {
                continue;
            }
            *n += 1;
        }
        shared.connections.fetch_add(1, Ordering::SeqCst);
        let s = shared.clone();
        let spawned = std::thread::Builder::new().name("web-connection".into()).spawn(move || {
            connection(&s, stream, ip);
            let mut per = lock(&s.per_address);
            if let Some(n) = per.get_mut(&ip) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    per.remove(&ip);
                }
            }
            drop(per);
            s.connections.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            shared.connections.fetch_sub(1, Ordering::SeqCst);
            let mut per = lock(&shared.per_address);
            if let Some(n) = per.get_mut(&ip) {
                *n = n.saturating_sub(1);
            }
        }
    }
}

/// An IPv4 address carried in IPv6 read as the IPv4 address it is.
fn plain_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

type Tls = rustls::StreamOwned<rustls::ServerConnection, TcpStream>;

fn connection(shared: &Shared, tcp: TcpStream, peer: IpAddr) {
    let _ = tcp.set_nodelay(true);
    let _ = tcp.set_read_timeout(Some(HANDSHAKE));
    let _ = tcp.set_write_timeout(Some(WRITE));
    let local = tcp.local_addr().map(|a| plain_ip(a.ip())).ok();
    // A TLS connection begins with a handshake record (22); a letter is a browser speaking plain
    // HTTP, which is sent to HTTPS.
    let mut first = [0u8; 1];
    match tcp.peek(&mut first) {
        Ok(1) if first[0] == 22 => {}
        Ok(1) if first[0].is_ascii_uppercase() => return to_https(shared, tcp, local),
        _ => return,
    }
    let Ok(conn) = rustls::ServerConnection::new(shared.tls.clone()) else { return };
    let mut tls = rustls::StreamOwned::new(conn, tcp);
    let began = Instant::now();
    while tls.conn.is_handshaking() {
        if began.elapsed() > HANDSHAKE || tls.conn.complete_io(&mut tls.sock).is_err() {
            return;
        }
    }
    let _ = tls.sock.set_read_timeout(Some(IDLE));
    let mut reader = BufReader::new(tls);
    for _ in 0..MAX_REQUESTS {
        let request = match http::read(&mut reader) {
            Ok(r) => r,
            Err(bad) => {
                if let Some(status) = bad.status() {
                    let _ = page(status, "Not understood", &format!("<p>{}</p>", escape(&say_bad(&bad)))).write(reader.get_mut(), true);
                }
                return;
            }
        };
        let Some(host) = known_host(shared, &request, local) else {
            let _ = Response::text(421, "This server does not answer to that name.").write(reader.get_mut(), true);
            return;
        };
        if request.path == "/ws" {
            return websocket(shared, reader, &request, &host);
        }
        let response = route(shared, &request, &host, peer);
        let close = request.closes() || shared.stop.load(Ordering::SeqCst);
        if response.write(reader.get_mut(), close).is_err() || close {
            return;
        }
    }
}

fn say_bad(bad: &Bad) -> String {
    match bad {
        Bad::Malformed(why) => format!("The request was not HTTP as this server reads it: {why}."),
        Bad::TooLarge => "The request's headers were too long.".into(),
        Bad::BodyTooLarge => "The request was too long.".into(),
        Bad::Unsupported => "The request was sent in a way this server does not take.".into(),
        Bad::Gone => String::new(),
    }
}

/// Plain HTTP on the HTTPS port: one answer sending the browser to HTTPS, for names this server
/// answers to only.
fn to_https(shared: &Shared, tcp: TcpStream, local: Option<IpAddr>) {
    let mut reader = BufReader::new(tcp);
    let Ok(request) = http::read(&mut reader) else { return };
    let response = match known_host(shared, &request, local) {
        Some(host) => Response::text(301, "This server speaks HTTPS only.").with("Location", &format!("https://{host}/")),
        None => Response::text(421, "This server does not answer to that name."),
    };
    // Strict-Transport-Security is ignored by browsers over plain HTTP, and is harmless.
    let _ = response.write(reader.get_mut(), true);
}

/// The request's `Host` when it is one this server answers to: a name it was given, or the very
/// address the connection came in on (a page elsewhere cannot make a browser send that while
/// talking to a name of its own).
fn known_host(shared: &Shared, request: &Request, local: Option<IpAddr>) -> Option<String> {
    known_among(&shared.hosts, shared.port, request, local)
}

fn known_among(hosts: &[String], served_port: u16, request: &Request, local: Option<IpAddr>) -> Option<String> {
    let host = request.header("host")?.trim().to_ascii_lowercase();
    if host.len() > 255 {
        return None;
    }
    if hosts.contains(&host) {
        return Some(host);
    }
    let local = local?;
    // `[v6]:port`, `[v6]`, `v4:port`, `name:port`, or a bare address.
    let (addr, port) = if let Some(rest) = host.strip_prefix('[') {
        let (a, after) = rest.split_once(']')?;
        (a, after.strip_prefix(':'))
    } else {
        match host.split_once(':') {
            Some((a, p)) if !p.contains(':') => (a, Some(p)),
            Some(_) => (host.as_str(), None),
            None => (host.as_str(), None),
        }
    };
    let port_ok = match port {
        Some(p) => p.parse::<u16>().ok() == Some(served_port),
        None => served_port == 443,
    };
    let ip: IpAddr = addr.parse().ok()?;
    (port_ok && plain_ip(ip) == local).then_some(host)
}

/// Whether a form or a WebSocket comes from a page of this server's.
fn same_origin(request: &Request, host: &str) -> bool {
    request.header("origin").is_some_and(|o| o.eq_ignore_ascii_case(&format!("https://{host}")))
}

impl Shared {
    /// The password's setting now, read again when its file changed.
    fn password(&self) -> Option<password::Kept> {
        let path = crate::password_file(&self.dir);
        let now = modified(&path);
        let mut seen = lock(&self.password);
        if seen.modified != now || (now.is_none() && seen.kept.is_some()) {
            seen.kept = password::load(&self.dir).ok().flatten();
            seen.modified = now;
        }
        seen.kept.clone()
    }

    fn generation(&self) -> u64 {
        self.password().map_or(0, |k| k.generation)
    }

    /// The guard, with bans changed at the console taken in first.
    fn guard(&self) -> MutexGuard<'_, Guard> {
        let mut guard = lock(&self.guard);
        let now = modified(&crate::bans_file(&self.dir));
        let mut seen = lock(&self.bans_seen);
        if *seen != now {
            if let Ok(kept) = Guard::load(&self.dir) {
                guard.take_kept(kept);
            }
            *seen = now;
        }
        drop(seen);
        guard
    }

    fn save_bans(&self, guard: &Guard) {
        let _ = guard.save(&self.dir);
        *lock(&self.bans_seen) = modified(&crate::bans_file(&self.dir));
    }

    /// The session a request's cookie names, when it is open.
    fn signed_in(&self, request: &Request) -> Option<String> {
        let token = request.cookie(COOKIE)?;
        let generation = self.generation();
        lock(&self.sessions).check(token, crate::now_ms(), generation).then(|| token.to_string())
    }

    fn ask(&self, ask: Ask) -> Result<Value, String> {
        let (reply, answer) = sync_channel(1);
        self.jobs.try_send(Job { ask, reply }).map_err(|_| "the mailbox is busy; try again in a moment".to_string())?;
        answer.recv_timeout(COMMAND_WAIT).map_err(|_| "the mailbox took too long to answer".to_string())?
    }

    /// A command's own result, without the note the backend puts beside it.
    fn run(&self, command: &str, params: Value) -> Result<Value, String> {
        let answer = self.ask(Ask::Execute(command.to_string(), params))?;
        Ok(answer.get("result").cloned().unwrap_or(answer))
    }

    /// One line in the sign-in log: when, from where, and what came of it. Never a password.
    fn log(&self, peer: IpAddr, what: &str) {
        let _held = lock(&self.log);
        let path = log_file(&self.dir);
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_MAX) {
            let mut old = path.as_os_str().to_owned();
            old.push(".old");
            let _ = std::fs::rename(&path, PathBuf::from(old));
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        if let Ok(mut file) = options.open(&path) {
            let _ = writeln!(file, "{}  {:<40}  {what}", crate::stamp(crate::now_ms()), guard::shown(guard::counted_as(peer)));
        }
    }
}

fn route(shared: &Shared, request: &Request, host: &str, peer: IpAddr) -> Response {
    let method = request.method.as_str();
    let path = request.path.as_str();
    match (method, path) {
        ("GET", "/sign-in") => {
            if shared.signed_in(request).is_some() {
                return Response::redirect("/");
            }
            sign_in_page(200, None)
        }
        ("POST", "/sign-in") => {
            if !same_origin(request, host) {
                return page(403, "Refused", "<p>A sign-in must come from this server's own page.</p>");
            }
            sign_in(shared, request, peer)
        }
        ("POST", "/sign-out") => {
            if !same_origin(request, host) {
                return page(403, "Refused", "<p>A sign-out must come from this server's own page.</p>");
            }
            if let Some(token) = request.cookie(COOKIE) {
                lock(&shared.sessions).close(token);
            }
            shared.log(peer, "signed out");
            Response::redirect("/sign-in").with("Set-Cookie", &format!("{COOKIE}=; Secure; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"))
        }
        (_, "/sign-in" | "/sign-out") => {
            Response::text(405, "Not with that method.").with("Allow", if path == "/sign-in" { "GET, POST" } else { "POST" })
        }
        ("GET", "/favicon.ico") => Response::text(404, ""),
        ("GET", "/style.css") => Response::new(200, "text/css; charset=utf-8", STYLE).with("Cache-Control", "max-age=3600"),
        _ => {
            if shared.signed_in(request).is_none() {
                return if method == "GET" { Response::redirect("/sign-in") } else { Response::text(401, "Sign in first.") };
            }
            if method != "GET" {
                return Response::text(405, "Not with that method.").with("Allow", "GET");
            }
            match path {
                "/" => home(shared),
                "/folder" => folder(shared, request),
                "/message" => message(shared, request),
                p if p.starts_with("/app/") || p == "/app" => static_file(shared, p),
                _ => page(404, "Not here", "<p>There is nothing at that address.</p><p><a href=\"/\">Folders</a></p>"),
            }
        }
    }
}

fn sign_in(shared: &Shared, request: &Request, peer: IpAddr) -> Response {
    let now = crate::now_ms();
    let verdict = shared.guard().check(peer, now);
    let wait_ms = match verdict {
        Verdict::Banned { until_ms } => {
            shared.log(peer, "refused: banned");
            let until = until_ms.map_or("until it is lifted at the host".to_string(), |u| format!("until {}", crate::stamp(u)));
            return page(
                403,
                "Not now",
                &format!(
                    "<p>Too many wrong passwords came from this address, so it may not sign in {}.</p><p>At the host, <code>openmailclient-cli web unban &lt;address&gt;</code> lifts it.</p>",
                    escape(&until)
                ),
            );
        }
        Verdict::Try { wait_ms } => wait_ms,
    };
    if wait_ms > 0 {
        std::thread::sleep(Duration::from_millis(wait_ms));
    }
    let Some(kept) = shared.password() else {
        return page(503, "Not set up", "<p>There is no web password. At the host, <code>openmailclient-cli web password</code> sets one.</p>");
    };
    let given = request.form("password").unwrap_or_default();
    let keep = request.form("keep").is_some_and(|v| v == "on");
    let right = given.len() <= password::MAX_LEN && {
        let _slot = shared.verifying.take();
        password::verify(&given, &kept.hash)
    };
    let now = crate::now_ms();
    if right {
        shared.guard().succeeded(peer);
        let token = match lock(&shared.sessions).open(now, keep, kept.generation) {
            Ok(t) => t,
            Err(e) => return page(503, "Not now", &format!("<p>{}</p>", escape(&e))),
        };
        shared.log(peer, if keep { "signed in (kept signed in)" } else { "signed in" });
        let age = if keep { format!("; Max-Age={}", crate::sessions::KEPT_MS / 1000) } else { String::new() };
        return Response::redirect("/").with("Set-Cookie", &format!("{COOKIE}={token}; Secure; HttpOnly; SameSite=Strict; Path=/{age}"));
    }
    let mut guard = shared.guard();
    let failed = guard.failed(peer, now);
    let said = match &failed {
        Failed::Banned(ban) => {
            shared.save_bans(&guard);
            let until = ban.until_ms.map_or("until it is lifted at the host".to_string(), |u| format!("until {}", crate::stamp(u)));
            shared.log(peer, &format!("wrong password; banned {until}"));
            format!("That was not the password, and too many have come from this address: it may not sign in {until}.")
        }
        Failed::Counted { .. } | Failed::Spared => {
            shared.log(peer, "wrong password");
            "That was not the password.".to_string()
        }
    };
    drop(guard);
    sign_in_page(401, Some(&said))
}

const STYLE: &str = "
:root{--bg:#f4f5f7;--card:#fff;--ink:#1d2330;--soft:#5c6575;--line:#d9dde4;--accent:#1f5fbf;--accent-ink:#fff;--unread:#1d2330;--hover:#eef2f9}
@media (prefers-color-scheme:dark){:root{--bg:#16181d;--card:#1f2229;--ink:#e6e8ec;--soft:#9aa3b2;--line:#323743;--accent:#5b9bff;--accent-ink:#0d1117;--unread:#fff;--hover:#272c36}}
*{box-sizing:border-box}html{-webkit-text-size-adjust:100%}
body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.45 system-ui,-apple-system,'Segoe UI',Roboto,'Noto Sans',sans-serif}
a{color:var(--accent);text-decoration:none}a:hover{text-decoration:underline}
header{display:flex;align-items:center;gap:12px;padding:10px 16px;background:var(--card);border-bottom:1px solid var(--line);position:sticky;top:0}
header h1{font-size:16px;margin:0;flex:1;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
main{max-width:960px;margin:0 auto;padding:16px}
.card{background:var(--card);border:1px solid var(--line);border-radius:8px;overflow:hidden}
.row{display:flex;gap:12px;align-items:baseline;padding:10px 14px;border-top:1px solid var(--line);color:var(--ink)}
.row:first-child{border-top:0}.row:hover{background:var(--hover);text-decoration:none}
.grow{flex:1;min-width:0}.soft{color:var(--soft)}.small{font-size:13px}
.one{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.unread .who,.unread .what{font-weight:600;color:var(--unread)}
.count{font-variant-numeric:tabular-nums;color:var(--soft)}
.badge{background:var(--accent);color:var(--accent-ink);border-radius:10px;padding:0 7px;font-size:12px;font-weight:600}
.file{margin:18px 0 6px;font-size:13px;color:var(--soft);text-transform:uppercase;letter-spacing:.04em}
pre.body{white-space:pre-wrap;word-wrap:break-word;font:inherit;margin:0;padding:16px}
.meta{padding:14px 16px;border-bottom:1px solid var(--line)}.meta h2{margin:0 0 6px;font-size:18px}
.pager{display:flex;justify-content:space-between;margin:12px 2px}
button,.button{font:inherit;border:1px solid var(--line);background:var(--card);color:var(--ink);border-radius:6px;padding:6px 12px;cursor:pointer}
button.primary{background:var(--accent);border-color:var(--accent);color:var(--accent-ink);width:100%;padding:10px}
form.inline{margin:0}
.signin{max-width:360px;margin:12vh auto 0;padding:24px}
.signin h1{font-size:20px;margin:0 0 4px}
.signin input[type=password]{width:100%;font:inherit;padding:10px;border:1px solid var(--line);border-radius:6px;background:var(--bg);color:var(--ink);margin:12px 0}
.signin label.keep{display:flex;gap:8px;align-items:center;margin:0 0 16px;color:var(--soft)}
.said{background:#fdecea;color:#7a1b12;border-radius:6px;padding:8px 10px;margin:12px 0 0}
@media (prefers-color-scheme:dark){.said{background:#3b1d1a;color:#ffb4a8}}
.note{border-left:3px solid var(--line);padding:4px 10px;margin:0 0 12px;color:var(--soft);font-size:13px}
";

fn page(status: u16, title: &str, body: &str) -> Response {
    Response::html(
        status,
        format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><meta name=\"robots\" content=\"noindex\"><title>{} · OpenMailClient</title><link rel=\"stylesheet\" href=\"/style.css\"></head><body>{body}</body></html>",
            escape(title)
        ),
    )
}

fn signed_in_page(title: &str, back: Option<(&str, &str)>, body: &str) -> Response {
    let back = back.map_or(String::new(), |(href, label)| format!("<a href=\"{}\">‹ {}</a>", escape(href), escape(label)));
    page(
        200,
        title,
        &format!(
            "<header>{back}<h1>{}</h1><form class=\"inline\" method=\"post\" action=\"/sign-out\"><button>Sign out</button></form></header><main>{body}</main>",
            escape(title)
        ),
    )
}

fn sign_in_page(status: u16, said: Option<&str>) -> Response {
    let said = said.map_or(String::new(), |s| format!("<p class=\"said\" role=\"alert\">{}</p>", escape(s)));
    page(
        status,
        "Sign in",
        &format!(
            "<form class=\"card signin\" method=\"post\" action=\"/sign-in\"><h1>OpenMailClient</h1><div class=\"soft small\">Web access to the mailbox on this machine</div>{said}\
             <input type=\"hidden\" name=\"username\" value=\"openmailclient\" autocomplete=\"username\">\
             <input type=\"password\" name=\"password\" placeholder=\"Web password\" aria-label=\"Web password\" autocomplete=\"current-password\" required autofocus maxlength=\"1024\">\
             <label class=\"keep\"><input type=\"checkbox\" name=\"keep\"> Keep me signed in on this device</label>\
             <button class=\"primary\">Sign in</button>\
             <p class=\"soft small\">Forgot it? At the host, <code>openmailclient-cli web password</code> sets it again.</p></form>"
        ),
    )
}

fn error_page(e: &str) -> Response {
    let mut r = signed_in_page("Not now", Some(("/", "Folders")), &format!("<p>{}</p>", escape(e)));
    r.status = 503;
    r
}

fn home(shared: &Shared) -> Response {
    let folders = match shared.run("mail.folders", json!({})) {
        Ok(v) => v,
        Err(e) => return error_page(&e),
    };
    let list = folders.get("folders").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut body = String::new();
    if shared.web_dir.is_some() {
        body.push_str("<p class=\"note\"><a href=\"/app/index.html\">The whole window in the browser</a> (a preview: until M7.6 it shows the sample mailbox, not this one).</p>");
    }
    if list.is_empty() {
        body.push_str("<p class=\"soft\">No data file is open.</p>");
    }
    let mut last_file = None;
    for f in &list {
        let file = f.get("file").and_then(Value::as_str).unwrap_or("");
        if last_file != Some(file) {
            if last_file.is_some() {
                body.push_str("</div>");
            }
            body.push_str(&format!("<div class=\"file\">{}</div><div class=\"card\">", escape(file)));
            last_file = Some(file);
        }
        let depth = f.get("path").and_then(Value::as_str).map_or(0, |p| p.matches('/').count()).min(8);
        let unread = f.get("unread").and_then(Value::as_u64).unwrap_or(0);
        let items = f.get("items").and_then(Value::as_u64).unwrap_or(0);
        body.push_str(&format!(
            "<a class=\"row{}\" href=\"/folder?store={}&amp;id={}\"><span class=\"grow one what\" style=\"padding-left:{}px\">{}</span>{}<span class=\"count small\">{items}</span></a>",
            if unread > 0 { " unread" } else { "" },
            f.get("store").and_then(Value::as_u64).unwrap_or(0),
            f.get("id").and_then(Value::as_u64).unwrap_or(0),
            depth * 16,
            escape(f.get("name").and_then(Value::as_str).unwrap_or("")),
            if unread > 0 { format!("<span class=\"badge\">{unread}</span>") } else { String::new() },
        ));
    }
    if last_file.is_some() {
        body.push_str("</div>");
    }
    signed_in_page("Folders", None, &body)
}

/// A number from the query, or `None`.
fn query_u64(request: &Request, name: &str) -> Option<u64> {
    request.query.split('&').filter_map(|p| p.split_once('=')).find(|(n, _)| *n == name).and_then(|(_, v)| v.parse().ok())
}

fn untrusted<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get("untrusted_content").and_then(|u| u.get(key)).and_then(Value::as_str).unwrap_or("")
}

fn folder(shared: &Shared, request: &Request) -> Response {
    let (Some(store), Some(id)) = (query_u64(request, "store"), query_u64(request, "id")) else {
        return page(404, "Not here", "<p>That is not a folder.</p><p><a href=\"/\">Folders</a></p>");
    };
    let offset = query_u64(request, "offset").unwrap_or(0).min(10_000_000);
    let listed = match shared.run("mail.items", json!({ "store": store, "folderId": id, "offset": offset, "limit": PAGE })) {
        Ok(v) => v,
        Err(e) => return error_page(&e),
    };
    let name = listed.get("path").and_then(Value::as_str).map_or("Folder", |p| p.rsplit('/').next().unwrap_or(p)).to_string();
    let total = listed.get("total").and_then(Value::as_u64).unwrap_or(0);
    let mut body = String::from("<div class=\"card\">");
    let items = listed.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    if items.is_empty() {
        body.push_str("<div class=\"row soft\">There are no items in this folder.</div>");
    }
    for item in &items {
        let read = item.get("read").and_then(Value::as_bool).unwrap_or(true);
        let subject = untrusted(item, "subject");
        let who = if untrusted(item, "from").is_empty() { untrusted(item, "to") } else { untrusted(item, "from") };
        body.push_str(&format!(
            "<a class=\"row{}\" href=\"/message?store={store}&amp;folder={id}&amp;id={}\"><span class=\"grow\"><span class=\"one who\" style=\"display:block\">{}</span><span class=\"one what\" style=\"display:block\">{}</span><span class=\"one soft small\" style=\"display:block\">{}</span></span><span class=\"soft small\" style=\"white-space:nowrap\">{}{}</span></a>",
            if read { "" } else { " unread" },
            item.get("id").and_then(Value::as_u64).unwrap_or(0),
            escape(who),
            if subject.is_empty() { "(no subject)".to_string() } else { escape(subject) },
            escape(untrusted(item, "preview")),
            if item.get("attachments").and_then(Value::as_bool).unwrap_or(false) { "📎 " } else { "" },
            escape(item.get("time").and_then(Value::as_str).unwrap_or("")),
        ));
    }
    body.push_str("</div><div class=\"pager small\">");
    body.push_str(&if offset > 0 {
        format!("<a href=\"/folder?store={store}&amp;id={id}&amp;offset={}\">‹ Newer</a>", offset.saturating_sub(PAGE))
    } else {
        "<span></span>".into()
    });
    let shown_to = offset.saturating_add(items.len() as u64);
    body.push_str(&format!("<span class=\"soft\">{} to {shown_to} of {total}</span>", if items.is_empty() { 0 } else { offset + 1 }));
    body.push_str(&if shown_to < total {
        format!("<a href=\"/folder?store={store}&amp;id={id}&amp;offset={shown_to}\">Older ›</a>")
    } else {
        "<span></span>".into()
    });
    body.push_str("</div>");
    signed_in_page(&name, Some(("/", "Folders")), &body)
}

fn message(shared: &Shared, request: &Request) -> Response {
    let (Some(store), Some(folder), Some(id)) = (query_u64(request, "store"), query_u64(request, "folder"), query_u64(request, "id")) else {
        return page(404, "Not here", "<p>That is not a message.</p><p><a href=\"/\">Folders</a></p>");
    };
    let read = match shared.run("mail.read", json!({ "store": store, "folderId": folder, "id": id, "maxChars": 200_000 })) {
        Ok(v) => v,
        Err(e) => return error_page(&e),
    };
    let subject = untrusted(&read, "subject");
    let from = untrusted(&read, "from");
    let address = untrusted(&read, "fromAddress");
    let mut meta = format!("<div class=\"meta\"><h2>{}</h2><div>{}", escape(if subject.is_empty() { "(no subject)" } else { subject }), escape(from));
    if !address.is_empty() && address != from {
        meta.push_str(&format!(" <span class=\"soft\">&lt;{}&gt;</span>", escape(address)));
    }
    meta.push_str("</div>");
    for (label, key) in [("To", "to"), ("Cc", "cc")] {
        let v = untrusted(&read, key);
        if !v.is_empty() {
            meta.push_str(&format!("<div class=\"small\"><span class=\"soft\">{label}:</span> {}</div>", escape(v)));
        }
    }
    meta.push_str(&format!("<div class=\"soft small\">{}</div>", escape(read.get("time").and_then(Value::as_str).unwrap_or(""))));
    let attachments: Vec<String> = read
        .get("untrusted_content")
        .and_then(|u| u.get("attachments"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.get("name").and_then(Value::as_str)).map(escape).collect())
        .unwrap_or_default();
    if !attachments.is_empty() {
        meta.push_str(&format!("<div class=\"small\">📎 {}</div>", attachments.join(", ")));
    }
    meta.push_str("</div>");
    let mut text = escape(untrusted(&read, "body"));
    if read.get("cut").and_then(Value::as_bool).unwrap_or(false) {
        text.push_str("\n\n[…the rest is not shown here]");
    }
    let back = format!("/folder?store={store}&id={folder}");
    // The words are the stranger's; they are shown as text, never as HTML, and nothing in them is fetched.
    signed_in_page(
        if subject.is_empty() { "(no subject)" } else { subject },
        Some((&back, "Back")),
        &format!("<div class=\"card\">{meta}<pre class=\"body\">{text}</pre></div>"),
    )
}

fn static_file(shared: &Shared, path: &str) -> Response {
    let Some(dir) = &shared.web_dir else { return page(404, "Not here", "<p>The web build is not served here.</p>") };
    let name = path.trim_start_matches("/app").trim_start_matches('/');
    let name = if name.is_empty() { "index.html" } else { name };
    // A plain file name only: nothing above the folder, nothing hidden.
    if name.starts_with('.') || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) {
        return page(404, "Not here", "<p>There is nothing at that address.</p>");
    }
    let file = dir.join(name);
    let kind = match file.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        _ => return page(404, "Not here", "<p>There is nothing at that address.</p>"),
    };
    match std::fs::metadata(&file) {
        Ok(m) if m.is_file() && m.len() <= MAX_STATIC => match std::fs::read(&file) {
            Ok(bytes) => Response::new(200, kind, bytes),
            Err(_) => page(404, "Not here", "<p>There is nothing at that address.</p>"),
        },
        _ => page(404, "Not here", "<p>There is nothing at that address.</p>"),
    }
}

/// `/ws`: one channel of commands, `{"id", "method", "params"}` answered `{"id", "result"}` or
/// `{"id", "error"}`, the methods those of the control channel that need no window.
fn websocket(shared: &Shared, mut reader: BufReader<Tls>, request: &Request, host: &str) {
    let refuse = |reader: &mut BufReader<Tls>, status: u16, why: &str| {
        let _ = Response::text(status, why).write(reader.get_mut(), true);
    };
    if !same_origin(request, host) {
        return refuse(&mut reader, 403, "A WebSocket must come from this server's own page.");
    }
    let Some(token) = shared.signed_in(request) else { return refuse(&mut reader, 401, "Sign in first.") };
    let key = match ws::upgrade_key(request) {
        Ok(k) => k,
        Err(why) => return refuse(&mut reader, 400, why),
    };
    let head = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        ws::accept_key(key)
    );
    if reader.get_mut().write_all(head.as_bytes()).and_then(|()| reader.get_mut().flush()).is_err() {
        return;
    }
    let _ = reader.get_ref().sock.set_read_timeout(Some(WS_QUIET));
    let mut frames = ws::Reader::new();
    let mut buf = vec![0u8; 16 * 1024];
    let mut quiet = 0u32;
    let send = |reader: &mut BufReader<Tls>, bytes: &[u8]| reader.get_mut().write_all(bytes).and_then(|()| reader.get_mut().flush()).is_ok();
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            let _ = send(&mut reader, &ws::close(1001));
            return;
        }
        match reader.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                quiet = 0;
                if let Err(broken) = frames.push(buf.get(..n).unwrap_or(&[])) {
                    let _ = send(&mut reader, &ws::close(broken.code()));
                    return;
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                quiet += 1;
                if quiet >= 4 || !send(&mut reader, &ws::ping()) {
                    let _ = send(&mut reader, &ws::close(1001));
                    return;
                }
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        }
        loop {
            let message = match frames.message() {
                Ok(Some(m)) => m,
                Ok(None) => break,
                Err(broken) => {
                    let _ = send(&mut reader, &ws::close(broken.code()));
                    return;
                }
            };
            let text = match message {
                ws::Message::Close => {
                    let _ = send(&mut reader, &ws::close(1000));
                    return;
                }
                ws::Message::Ping(p) => {
                    if !send(&mut reader, &ws::pong(&p)) {
                        return;
                    }
                    continue;
                }
                ws::Message::Pong => continue,
                ws::Message::Binary(_) => {
                    let _ = send(&mut reader, &ws::close(1003));
                    return;
                }
                ws::Message::Text(t) => t,
            };
            // The session is looked at again for each message: a sign-out elsewhere or a new
            // password ends this channel too.
            let still = lock(&shared.sessions).check(&token, crate::now_ms(), shared.generation());
            if !still {
                let _ = send(&mut reader, &ws::close(1008));
                return;
            }
            let answer = answer(shared, &text);
            if !send(&mut reader, &ws::text(&answer.to_string())) {
                return;
            }
        }
    }
}

/// The answer to one message of the WebSocket.
fn answer(shared: &Shared, text: &str) -> Value {
    let Ok(message) = serde_json::from_str::<Value>(text) else { return json!({ "id": null, "error": "not JSON" }) };
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let asked = match message.get("method").and_then(Value::as_str) {
        Some("engine.commands") => Ask::Commands,
        Some("engine.state") => Ask::State,
        Some("engine.execute") => match params.get("command").and_then(Value::as_str) {
            Some(command) if command.len() <= 128 => Ask::Execute(command.to_string(), params.get("params").cloned().unwrap_or(Value::Null)),
            _ => return json!({ "id": id, "error": "engine.execute takes {\"command\", \"params\"}" }),
        },
        Some("control.ping") => return json!({ "id": id, "result": "pong" }),
        _ => return json!({ "id": id, "error": "methods: control.ping, engine.commands, engine.execute, engine.state" }),
    };
    match shared.ask(asked) {
        Ok(result) => json!({ "id": id, "result": result }),
        Err(e) => json!({ "id": id, "error": e }),
    }
}

#[cfg(test)]
mod tests;
