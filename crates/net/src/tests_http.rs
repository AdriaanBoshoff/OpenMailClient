//! Pictures fetched from a web server, against the fake one: nothing leaves the process.

use crate::NetError;
use crate::fake::{Fake, State};
use crate::http::{Address, address, get};

fn fake(pages: &[(&str, &[u8])]) -> Fake {
    let state = State { pages: pages.iter().map(|(at, answer)| (at.to_string(), answer.to_vec())).collect(), ..State::default() };
    Fake::new(state)
}

#[test]
fn only_https_addresses_off_this_machine_are_fetched() {
    let at = |host: &str, port: u16, target: &str| Address { host: host.into(), port, target: target.into() };
    assert_eq!(address("https://Pictures.Example.com/a/b.png?x=1#top").ok(), Some(at("pictures.example.com", 443, "/a/b.png?x=1")));
    assert_eq!(address("HTTPS://pictures.example.com:8443").ok(), Some(at("pictures.example.com", 8443, "/")));
    assert_eq!(address("https://[2001:db8::1]:444/p.gif").ok(), Some(at("2001:db8::1", 444, "/p.gif")));
    for refused in [
        "http://pictures.example.com/a.png",
        "ftp://pictures.example.com/a.png",
        "/a.png",
        "https://localhost/a.png",
        "https://127.0.0.1/a.png",
        "https://10.1.2.3/a.png",
        "https://192.168.1.1/a.png",
        "https://169.254.1.1/a.png",
        "https://[::1]/a.png",
        "https://[fd00::1]/a.png",
        "https://[::ffff:127.0.0.1]/a.png",
        "https://printer/a.png",
        "https://router.local/a.png",
        "https://pat:secret@pictures.example.com/a.png",
        "https://pictures.example.com:0/a.png",
        "https://pictures.example.com:99999/a.png",
        "https://pictures.example.com/a b.png",
        "https://pictures.example.com/a\r\nHost: evil.test",
        "https://pictures.example.com/caf\u{e9}.png",
        "https://pic_tures.example.com/a.png",
        "https://[2001:db8::1/a.png",
    ] {
        assert!(address(refused).is_err(), "{refused} must not be fetched");
    }
    assert!(address(&format!("https://pictures.example.com/{}", "a".repeat(3000))).is_err());
}

#[test]
fn a_picture_is_fetched_with_its_length_in_chunks_or_until_the_end() {
    let f = fake(&[
        ("pictures.example.com/a.png", b"HTTP/1.1 200 OK\r\nContent-Type: image/png; q=1\r\nContent-Length: 5\r\n\r\nhello"),
        ("pictures.example.com/b.png", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\n"),
        ("pictures.example.com/c.png", b"HTTP/1.0 200 OK\r\nContent-Type: image/gif\r\n\r\nto the end"),
    ]);
    let a = get(&f, "https://pictures.example.com/a.png", 1024).unwrap();
    assert_eq!((a.bytes.as_slice(), a.kind.as_str()), (&b"hello"[..], "image/png"));
    assert_eq!(get(&f, "https://pictures.example.com/b.png", 1024).unwrap().bytes, b"abcde");
    assert_eq!(get(&f, "https://pictures.example.com/c.png", 1024).unwrap().bytes, b"to the end");
    // The request says nothing of the reader: no cookies, no referrer.
    let heard = f.state().heard.join("\n");
    assert!(heard.contains("GET /a.png HTTP/1.1"), "{heard}");
    assert!(!heard.to_ascii_lowercase().contains("cookie") && !heard.to_ascii_lowercase().contains("referer"));
}

#[test]
fn redirects_are_followed_a_few_times_and_only_to_https() {
    let f = fake(&[
        ("pictures.example.com/old.png", b"HTTP/1.1 301 Moved\r\nLocation: new/one.png\r\nContent-Length: 0\r\n\r\n"),
        ("pictures.example.com/new/one.png", b"HTTP/1.1 302 Found\r\nLocation: https://cdn.example.net/x.png\r\n\r\n"),
        ("cdn.example.net/x.png", b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"),
        ("pictures.example.com/loop.png", b"HTTP/1.1 302 Found\r\nLocation: /loop.png\r\n\r\n"),
        ("pictures.example.com/plain.png", b"HTTP/1.1 302 Found\r\nLocation: http://cdn.example.net/x.png\r\n\r\n"),
        ("pictures.example.com/home.png", b"HTTP/1.1 302 Found\r\nLocation: https://127.0.0.1/x.png\r\n\r\n"),
    ]);
    assert_eq!(get(&f, "https://pictures.example.com/old.png", 1024).unwrap().bytes, b"ok");
    assert!(matches!(get(&f, "https://pictures.example.com/loop.png", 1024), Err(NetError::Refused(_))));
    assert!(matches!(get(&f, "https://pictures.example.com/plain.png", 1024), Err(NetError::Unsupported(_))));
    assert!(matches!(get(&f, "https://pictures.example.com/home.png", 1024), Err(NetError::Unsupported(_))));
    assert!(matches!(get(&f, "https://pictures.example.com/missing.png", 1024), Err(NetError::Refused(_))));
}

#[test]
fn hostile_web_servers_are_capped() {
    let f = fake(&[
        ("pictures.example.com/big.png", b"HTTP/1.1 200 OK\r\nContent-Length: 99999999999\r\n\r\n"),
        ("pictures.example.com/bigger.png", b"HTTP/1.1 200 OK\r\nContent-Length: 2000\r\n\r\n"),
        ("pictures.example.com/chunk.png", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nFFFFFFFFFFFFFFFF\r\n"),
        ("pictures.example.com/odd.png", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n"),
        ("pictures.example.com/end.png", b"HTTP/1.1 200 OK\r\n\r\n0123456789"),
        ("pictures.example.com/length.png", b"HTTP/1.1 200 OK\r\nContent-Length: -4\r\n\r\n"),
        ("pictures.example.com/short.png", b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\nshort"),
        ("pictures.example.com/gopher.png", b"SPDY 200 OK\r\n\r\n"),
    ]);
    for path in ["big", "bigger", "chunk", "odd", "end", "length", "short", "gopher"] {
        assert!(get(&f, &format!("https://pictures.example.com/{path}.png"), 8).is_err(), "{path}");
    }
    let mut many = b"HTTP/1.1 200 OK\r\n".to_vec();
    for n in 0..500 {
        many.extend(format!("X-{n}: y\r\n").as_bytes());
    }
    let f = fake(&[("pictures.example.com/head.png", &many)]);
    assert!(matches!(get(&f, "https://pictures.example.com/head.png", 1024), Err(NetError::TooLarge(_))));
}

/// A large file comes down a piece at a time, by its length, in chunks or to the end, after
/// a redirect to a long signed address; how far it is is heard, and it can be stopped.
#[test]
fn a_file_comes_down_in_pieces_and_can_be_stopped() {
    use crate::http::download;
    let body: Vec<u8> = (0..700_000u32).map(|n| (n % 251) as u8).collect();
    let mut sized = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    sized.extend(&body);
    let mut chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    for part in body.chunks(300_000) {
        chunked.extend(format!("{:x}\r\n", part.len()).as_bytes());
        chunked.extend(part);
        chunked.extend(b"\r\n");
    }
    chunked.extend(b"0\r\n\r\n");
    let mut ended = b"HTTP/1.0 200 OK\r\n\r\n".to_vec();
    ended.extend(&body);
    let signed = format!("/f?sig={}", "a".repeat(3000));
    let moved = format!("HTTP/1.1 302 Found\r\nLocation: https://files.example.com{signed}\r\n\r\n").into_bytes();
    let f = fake(&[
        ("files.example.com/sized", &sized),
        ("files.example.com/chunked", &chunked),
        ("files.example.com/ended", &ended),
        ("releases.example.com/moved", &moved),
        (&format!("files.example.com{signed}"), &sized),
        ("files.example.com/plain", b"HTTP/1.1 302 Found\r\nLocation: http://files.example.com/sized\r\n\r\n"),
    ]);
    for path in [
        "https://files.example.com/sized",
        "https://files.example.com/chunked",
        "https://files.example.com/ended",
        "https://releases.example.com/moved",
    ] {
        let mut out = Vec::new();
        let mut heard = Vec::new();
        let got = download(&f, path, 1 << 20, &mut out, &mut |n, total| {
            heard.push((n, total));
            true
        })
        .unwrap();
        assert_eq!(got, body.len() as u64, "{path}");
        assert!(out == body, "{path}");
        assert!(heard.len() >= 3, "heard in pieces: {path}");
        assert!(heard.windows(2).all(|w| w[0].0 < w[1].0));
    }
    // Stopped part way: an error, and no more than was heard is written.
    let mut out = Vec::new();
    let stopped = download(&f, "https://files.example.com/sized", 1 << 20, &mut out, &mut |n, _| n < 300_000);
    assert!(matches!(stopped, Err(NetError::Refused(_))));
    assert!(out.len() < body.len());
    // Larger than asked for, by its length, in chunks or to the end: refused as it comes.
    for path in ["https://files.example.com/sized", "https://files.example.com/chunked", "https://files.example.com/ended"] {
        let mut out = Vec::new();
        assert!(matches!(download(&f, path, 100_000, &mut out, &mut |_, _| true), Err(NetError::TooLarge(_))), "{path}");
        assert!(out.len() <= 100_000);
    }
    // Never over plain HTTP, never from this machine.
    assert!(download(&f, "https://files.example.com/plain", 1 << 20, &mut Vec::new(), &mut |_, _| true).is_err());
    assert!(download(&f, "https://127.0.0.1/sized", 1 << 20, &mut Vec::new(), &mut |_, _| true).is_err());
    assert!(download(&f, &format!("https://files.example.com/{}", "a".repeat(9000)), 1 << 20, &mut Vec::new(), &mut |_, _| true).is_err());
}
