//! The server end to end on this machine: a TLS client of our own that trusts only the
//! certificate's fingerprint, a backend of our own with words a stranger might write, and a
//! settings folder in the temporary folder.

use super::*;
use rustls::DigitallySignedStruct;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};

const PASSWORD: &str = "lantern orchard pebble violin";

/// A backend whose mailbox holds words meant to break out of the page.
struct Fake;

impl Backend for Fake {
    fn commands(&mut self) -> Result<Value, String> {
        Ok(json!([{ "id": "mail.folders" }]))
    }
    fn execute(&mut self, command: &str, params: &Value) -> Result<Value, String> {
        let result = match command {
            "mail.folders" => json!({ "folders": [
                { "store": 0, "file": "Test <file>", "id": 7, "path": "Inbox", "name": "Inbox <b>", "items": 2, "unread": 1 },
            ]}),
            "mail.items" => {
                assert_eq!(params.get("folderId").and_then(Value::as_u64), Some(7));
                json!({ "path": "Inbox", "total": 1, "items": [
                    { "id": 3, "read": false, "time": "Mon 10:00", "attachments": true,
                      "untrusted_content": { "from": "Mallory", "subject": "<script>alert(1)</script>", "preview": "\"><img src=x>" } },
                ]})
            }
            "mail.read" => json!({ "id": 3, "time": "Mon 10:00", "cut": false, "untrusted_content": {
                "from": "Mallory", "fromAddress": "m@example.invalid", "subject": "Hi", "to": "You", "cc": "",
                "body": "</pre><script>steal()</script>", "attachments": [{ "name": "a<b>.txt" }] } }),
            _ => return Err(format!("{command}: not in the fake")),
        };
        Ok(json!({ "result": result, "note": "untrusted" }))
    }
    fn state(&mut self) -> Result<Value, String> {
        Ok(json!({ "fake": true }))
    }
}

/// Trusts the one certificate whose fingerprint it was given.
#[derive(Debug)]
struct Pinned(String);

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let d = ring::digest::digest(&ring::digest::SHA256, cert.as_ref());
        let seen = d.as_ref().iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":");
        if seen == self.0 { Ok(ServerCertVerified::assertion()) } else { Err(rustls::Error::General("another certificate".into())) }
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &rustls::crypto::ring::default_provider().signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &rustls::crypto::ring::default_provider().signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes()
    }
}

struct Test {
    dir: PathBuf,
    running: Option<Running>,
    client: Arc<rustls::ClientConfig>,
}

impl Test {
    fn new(name: &str, before: impl FnOnce(&Path)) -> Test {
        let dir = std::env::temp_dir().join(format!("omc-serve-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        password::set(&dir, PASSWORD, crate::now_ms()).unwrap();
        before(&dir);
        let options = Options {
            listen: "127.0.0.1:0".parse().unwrap(),
            names: vec!["mail.home.arpa".into()],
            dir: dir.clone(),
            web_dir: None,
            new_certificate: false,
        };
        let running = serve(options, || Ok(Fake)).unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let client = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Pinned(running.started.fingerprint.clone())))
            .with_no_client_auth();
        Test { dir, running: Some(running), client: Arc::new(client) }
    }

    fn port(&self) -> u16 {
        self.running.as_ref().unwrap().started.listen.port()
    }

    fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port())
    }

    fn tls(&self) -> rustls::StreamOwned<rustls::ClientConnection, TcpStream> {
        let tcp = TcpStream::connect(("127.0.0.1", self.port())).unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        let conn = rustls::ClientConnection::new(self.client.clone(), ServerName::try_from("localhost").unwrap()).unwrap();
        rustls::StreamOwned::new(conn, tcp)
    }

    /// One request on a connection of its own; the status, the head and the body.
    fn send(&self, request: &str) -> (u16, String, String) {
        let mut s = self.tls();
        s.write_all(request.as_bytes()).unwrap();
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        split(&out)
    }

    fn get(&self, path: &str, cookie: Option<&str>) -> (u16, String, String) {
        let cookie = cookie.map_or(String::new(), |c| format!("Cookie: {COOKIE}={c}\r\n"));
        self.send(&format!("GET {path} HTTP/1.1\r\nHost: {}\r\n{cookie}Connection: close\r\n\r\n", self.host()))
    }

    fn post(&self, path: &str, origin: &str, cookie: Option<&str>, body: &str) -> (u16, String, String) {
        let cookie = cookie.map_or(String::new(), |c| format!("Cookie: {COOKIE}={c}\r\n"));
        self.send(&format!(
            "POST {path} HTTP/1.1\r\nHost: {}\r\nOrigin: {origin}\r\n{cookie}Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.host(),
            body.len()
        ))
    }

    fn origin(&self) -> String {
        format!("https://{}", self.host())
    }

    fn sign_in(&self) -> String {
        let (status, head, _) = self.post("/sign-in", &self.origin(), None, &format!("password={}", PASSWORD.replace(' ', "+")));
        assert_eq!(status, 303, "{head}");
        cookie_of(&head).unwrap()
    }
}

impl Drop for Test {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn split(bytes: &[u8]) -> (u16, String, String) {
    let text = String::from_utf8_lossy(bytes).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head.get(9..12).and_then(|s| s.parse().ok()).unwrap_or(0);
    (status, head.to_string(), body.to_string())
}

fn cookie_of(head: &str) -> Option<String> {
    let line = head.lines().find(|l| l.starts_with("Set-Cookie: "))?;
    let value = line.trim_start_matches("Set-Cookie: ").split(';').next()?;
    value.split_once('=').map(|(_, v)| v.to_string()).filter(|v| !v.is_empty())
}

#[test]
fn signing_in_reading_and_signing_out() {
    let t = Test::new("flow", |_| {});
    // Nothing without a session but the sign-in page.
    let (status, head, _) = t.get("/", None);
    assert_eq!(status, 303);
    assert!(head.contains("Location: /sign-in"));
    let (status, head, body) = t.get("/sign-in", None);
    assert_eq!(status, 200);
    assert!(body.contains("type=\"password\""));
    for h in ["Strict-Transport-Security", "Content-Security-Policy", "X-Frame-Options: DENY"] {
        assert!(head.contains(h), "{h}");
    }

    // A form from elsewhere is refused before the password is looked at.
    let (status, _, _) = t.post("/sign-in", "https://evil.example", None, &format!("password={PASSWORD}"));
    assert_eq!(status, 403);
    let (status, _, body) = t.post("/sign-in", &t.origin(), None, "password=not+it+at+all+no");
    assert_eq!(status, 401);
    assert!(body.contains("not the password"));

    let cookie = t.sign_in();
    let (status, _, body) = t.get("/", Some(&cookie));
    assert_eq!(status, 200);
    assert!(body.contains("Inbox &lt;b&gt;") && body.contains("Test &lt;file&gt;"), "{body}");
    let (status, _, body) = t.get("/folder?store=0&id=7", Some(&cookie));
    assert_eq!(status, 200);
    assert!(!body.contains("<script>alert") && body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"), "{body}");
    assert!(!body.contains("<img src=x>"));
    let (status, _, body) = t.get("/message?store=0&folder=7&id=3", Some(&cookie));
    assert_eq!(status, 200);
    assert!(!body.contains("<script>steal") && body.contains("&lt;/pre&gt;&lt;script&gt;steal()"), "{body}");
    assert!(body.contains("a&lt;b&gt;.txt"));

    // A name this server does not answer to: DNS rebinding gets nothing.
    let (status, _, body) =
        t.send(&format!("GET / HTTP/1.1\r\nHost: evil.example:{}\r\nCookie: {COOKIE}={cookie}\r\nConnection: close\r\n\r\n", t.port()));
    assert_eq!(status, 421);
    assert!(!body.contains("Inbox"));
    let (status, _, _) =
        t.send(&format!("GET / HTTP/1.1\r\nHost: mail.home.arpa:{}\r\nCookie: {COOKIE}={cookie}\r\nConnection: close\r\n\r\n", t.port()));
    assert_eq!(status, 200, "a name it was given");

    // Signing out ends the session.
    let (status, head, _) = t.post("/sign-out", &t.origin(), Some(&cookie), "");
    assert_eq!(status, 303);
    assert!(head.contains("Max-Age=0"));
    assert_eq!(t.get("/", Some(&cookie)).0, 303);

    // A new password ends every session open.
    let cookie = t.sign_in();
    assert_eq!(t.get("/", Some(&cookie)).0, 200);
    std::thread::sleep(Duration::from_millis(20));
    password::set(&t.dir, "another quite long passphrase", crate::now_ms()).unwrap();
    assert_eq!(t.get("/", Some(&cookie)).0, 303);

    let log = std::fs::read_to_string(log_file(&t.dir)).unwrap();
    assert!(log.contains("wrong password") && log.contains("signed in") && log.contains("signed out"));
    assert!(!log.contains(PASSWORD) && !log.contains("not+it"), "the log never holds a password");
    assert!(serving_file(&t.dir).exists());
}

#[test]
fn the_websocket_carries_commands_for_a_session_only() {
    let t = Test::new("ws", |_| {});
    let cookie = t.sign_in();
    let upgrade = |origin: &str, cookie: &str| {
        format!(
            "GET /ws HTTP/1.1\r\nHost: {}\r\nOrigin: {origin}\r\nCookie: {COOKIE}={cookie}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
            t.host()
        )
    };
    let (status, _, _) = t.send(&upgrade("https://evil.example", &cookie));
    assert_eq!(status, 403, "a page elsewhere cannot open it");
    let (status, _, _) = t.send(&upgrade(&t.origin(), "nobody"));
    assert_eq!(status, 401);

    let mut s = t.tls();
    s.write_all(upgrade(&t.origin(), &cookie).as_bytes()).unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        s.read_exact(&mut byte).unwrap();
        head.push(byte[0]);
    }
    let head = String::from_utf8(head).unwrap();
    assert!(head.starts_with("HTTP/1.1 101") && head.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="), "{head}");
    let ask = |s: &mut rustls::StreamOwned<rustls::ClientConnection, TcpStream>, text: &str| {
        let mask = [1u8, 2, 3, 4];
        let mut frame = vec![0x81, 0x80 | text.len() as u8];
        frame.extend_from_slice(&mask);
        frame.extend(text.bytes().zip(mask.iter().cycle()).map(|(b, m)| b ^ m));
        s.write_all(&frame).unwrap();
        let mut two = [0u8; 2];
        s.read_exact(&mut two).unwrap();
        assert_eq!(two[0], 0x81);
        let len = match two[1] {
            126 => {
                let mut n = [0u8; 2];
                s.read_exact(&mut n).unwrap();
                usize::from(u16::from_be_bytes(n))
            }
            n => usize::from(n),
        };
        let mut payload = vec![0u8; len];
        s.read_exact(&mut payload).unwrap();
        serde_json::from_slice::<Value>(&payload).unwrap()
    };
    let got = ask(&mut s, r#"{"id":1,"method":"engine.execute","params":{"command":"mail.folders"}}"#);
    assert_eq!(got["id"], 1);
    assert_eq!(got["result"]["result"]["folders"][0]["id"], 7);
    let got = ask(&mut s, r#"{"id":2,"method":"ui.screenshot"}"#);
    assert!(got["error"].is_string());
    let got = ask(&mut s, "not json");
    assert_eq!(got["error"], "not JSON");
}

#[test]
fn a_ban_is_kept_and_lifted_from_the_console_without_a_restart() {
    let t = Test::new("ban", |dir| {
        let mut g = Guard::new();
        g.ban_by_hand("127.0.0.1".parse().unwrap(), crate::now_ms(), None);
        g.save(dir).unwrap();
    });
    let (status, _, body) = t.post("/sign-in", &t.origin(), None, &format!("password={}", PASSWORD.replace(' ', "+")));
    assert_eq!(status, 403, "banned even with the right password");
    assert!(body.contains("unban"));
    // `openmailclient-cli web unban 127.0.0.1` writes the file; the server reads it again.
    std::thread::sleep(Duration::from_millis(20));
    let mut g = Guard::load(&t.dir).unwrap();
    assert!(g.unban("127.0.0.1".parse().unwrap()));
    g.save(&t.dir).unwrap();
    t.sign_in();
}

#[test]
fn plain_http_and_hostile_bytes_get_nothing_of_the_mailbox() {
    let t = Test::new("plain", |_| {});
    let mut tcp = TcpStream::connect(("127.0.0.1", t.port())).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    tcp.write_all(format!("GET / HTTP/1.1\r\nHost: {}\r\n\r\n", t.host()).as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = tcp.read_to_end(&mut out);
    let (status, head, _) = split(&out);
    assert_eq!(status, 301);
    assert!(head.contains(&format!("Location: https://{}/", t.host())));
    // Garbage before TLS, and junk after it, end the connection and nothing else.
    for junk in [&b"\x00\x01\x02"[..], b"\x16\x03\x01\xff\xff garbage"] {
        let mut tcp = TcpStream::connect(("127.0.0.1", t.port())).unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let _ = tcp.write_all(junk);
        let _ = tcp.shutdown(std::net::Shutdown::Write);
        let mut out = Vec::new();
        let _ = tcp.read_to_end(&mut out);
        assert!(!String::from_utf8_lossy(&out).contains("Inbox"));
    }
    let (status, _, _) = t.send(&format!("GET / HTTP/1.1\r\nHost: {}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n", t.host()));
    assert_eq!(status, 501);
    // The server still answers.
    assert_eq!(t.get("/sign-in", None).0, 200);
}

#[test]
fn hosts_are_known_by_name_or_by_the_address_dialled() {
    let local: IpAddr = "192.168.1.20".parse().unwrap();
    let names = vec!["localhost".to_string(), "::1".to_string()];
    let request = |host: &str| Request {
        method: "GET".into(),
        path: "/".into(),
        query: String::new(),
        headers: vec![("host".into(), host.into())],
        body: Vec::new(),
    };
    let hosts = host_values(&names, 8443);
    assert_eq!(hosts, vec!["localhost:8443", "[::1]:8443"]);
    let check = |host: &str| known_among(&hosts, 8443, &request(host), Some(local)).is_some();
    assert!(check("LOCALHOST:8443"));
    assert!(check("192.168.1.20:8443"));
    assert!(!check("192.168.1.21:8443"));
    assert!(!check("evil.example:8443"));
    assert!(!check("192.168.1.20:9999"));
    assert!(!check("192.168.1.20"));
    assert!(!check("[192.168.1.20]:8443x"));
    let v6: IpAddr = "fd00::5".parse().unwrap();
    assert!(known_among(&hosts, 8443, &request("[fd00::5]:8443"), Some(v6)).is_some());
    assert!(known_among(&hosts, 8443, &request("[fd00::6]:8443"), Some(v6)).is_none());
    assert!(known_among(&hosts, 8443, &request(&"a".repeat(300)), Some(v6)).is_none());
    assert!(is_private("192.168.1.20".parse().unwrap()) && is_private("100.100.1.1".parse().unwrap()) && is_private("fd00::1".parse().unwrap()));
    assert!(!is_private("8.8.8.8".parse().unwrap()));
}
