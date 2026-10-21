//! The clients against the in-process fake server, and against servers that break the rules.

use crate::fake::{Fake, State};
use crate::imap::{self, Imap, Resp, Tok};
use crate::pop3::Pop3;
use crate::smtp::{Smtp, stuffed};
use crate::{Endpoint, Login, NetError, Protocol, Secret, Security, decode_mailbox_name, encode_mailbox_name};

fn login() -> Login {
    let s = State::default();
    Login { user: s.user, password: Secret::new(s.password) }
}

fn tls(protocol: Protocol) -> Endpoint {
    Endpoint { security: Security::Tls, port: protocol.port(Security::Tls), ..Fake::endpoint(protocol) }
}

const MESSAGE: &[u8] =
    b"From: Pat <pat@example.test>\r\nTo: Sam <sam@example.test>\r\nSubject: Hello\r\n\r\nFirst line\r\n.starts with a dot\r\n.\r\nbare\nends\n";

#[test]
fn a_message_is_submitted_after_starttls_and_sign_in() {
    let fake = Fake::default();
    let mut s = Smtp::open(&fake, &Fake::endpoint(Protocol::Smtp), Some(&login())).unwrap();
    assert!(s.is_secure());
    let sent = s.send("pat@example.test", &["sam@example.test".into(), "kim@refused.test".into()], MESSAGE).unwrap();
    s.quit().unwrap();
    assert_eq!(sent.accepted, vec!["sam@example.test".to_string()]);
    assert_eq!(sent.refused.len(), 1);
    let state = fake.state();
    assert_eq!((state.secured, state.logins), (1, 1));
    let got = &state.submitted[0];
    assert_eq!(got.from, "pat@example.test");
    assert_eq!(got.to, vec!["sam@example.test".to_string()]);
    // Dots are put back as they were and every line ends in CRLF.
    let want = String::from_utf8_lossy(MESSAGE).replace("bare\nends\n", "bare\r\nends\r\n");
    assert_eq!(String::from_utf8_lossy(&got.data), want);
    // The size was said, and the password never went before TLS: STARTTLS came first.
    let heard = &state.heard;
    let at = |what: &str| heard.iter().position(|l| l.starts_with(what)).unwrap();
    assert!(at("STARTTLS") < at("AUTH PLAIN"));
    assert!(heard.iter().any(|l| l.starts_with("MAIL FROM:<pat@example.test> SIZE=")));
}

#[test]
fn no_password_goes_to_a_server_that_cannot_be_made_secure() {
    let mut state = State::default();
    state.smtp_ext.retain(|e| e != "STARTTLS");
    state.pop_caps.retain(|c| c != "STLS");
    state.imap_without.push("STARTTLS".into());
    let fake = Fake::new(state);
    assert!(matches!(Smtp::open(&fake, &Fake::endpoint(Protocol::Smtp), Some(&login())), Err(NetError::NotSecure)));
    assert!(matches!(Pop3::open(&fake, &Fake::endpoint(Protocol::Pop3), &login()), Err(NetError::NotSecure)));
    let plain = |p| Endpoint { security: Security::Plain, ..Fake::endpoint(p) };
    assert!(matches!(Pop3::open(&fake, &plain(Protocol::Pop3), &login()), Err(NetError::NotSecure)));
    assert!(matches!(Imap::open(&fake, &plain(Protocol::Imap), &login()), Err(NetError::NotSecure)));
    assert!(matches!(Smtp::open(&fake, &plain(Protocol::Smtp), Some(&login())), Err(NetError::NotSecure)));
    let state = fake.state();
    assert_eq!(state.logins, 0);
    assert!(!state.heard.iter().any(|l| l.contains(&state.password) || l.starts_with("AUTH") || l.contains("LOGIN")));
}

#[test]
fn smtp_signs_in_with_login_when_plain_is_not_offered_and_says_a_wrong_password() {
    let mut state = State::default();
    for e in &mut state.smtp_ext {
        if e.starts_with("AUTH") {
            *e = "AUTH LOGIN".into();
        }
    }
    let fake = Fake::new(state);
    // The fake takes only PLAIN, so LOGIN being tried shows in what it heard.
    let r = Smtp::open(&fake, &tls(Protocol::Smtp), Some(&login()));
    assert!(matches!(r, Err(NetError::Refused(_) | NetError::Auth(_))));
    assert!(fake.state().heard.iter().any(|l| l == "AUTH LOGIN"));
    let wrong = Login { password: Secret::new("wrong"), ..login() };
    assert!(matches!(Smtp::open(&Fake::default(), &tls(Protocol::Smtp), Some(&wrong)), Err(NetError::Auth(_))));
}

#[test]
fn an_address_cannot_carry_a_command_in_with_it() {
    let fake = Fake::default();
    let mut s = Smtp::open(&fake, &tls(Protocol::Smtp), Some(&login())).unwrap();
    for bad in ["sam@example.test>\r\nRCPT TO:<eve@example.test", "sam@example.test> NOTIFY=NEVER", "", "no-at-sign", "a<b@example.test"] {
        assert!(matches!(s.send("pat@example.test", &[bad.to_string()], MESSAGE), Err(NetError::Unsupported(_))), "{bad:?}");
        assert!(matches!(s.send(bad, &["sam@example.test".into()], MESSAGE), Err(NetError::Unsupported(_))), "{bad:?}");
    }
    let refused = s.send("pat@example.test", &["kim@refused.test".into()], MESSAGE);
    assert!(matches!(refused, Err(NetError::Refused(_))));
    assert!(fake.state().submitted.is_empty());
    assert!(!fake.state().heard.iter().any(|l| l == "DATA"));
}

#[test]
fn a_message_too_large_for_the_server_is_not_sent() {
    let fake = Fake::default();
    let mut s = Smtp::open(&fake, &tls(Protocol::Smtp), Some(&login())).unwrap();
    let big = vec![b'a'; 10_485_761];
    assert!(matches!(s.send("pat@example.test", &["sam@example.test".into()], &big), Err(NetError::Refused(_))));
    assert!(fake.state().submitted.is_empty());
}

#[test]
fn dot_stuffing_follows_rfc_5321() {
    assert_eq!(stuffed(b""), b".\r\n");
    assert_eq!(stuffed(b"a"), b"a\r\n.\r\n");
    assert_eq!(stuffed(b".\r\n..x\r\n"), b"..\r\n...x\r\n.\r\n");
    assert_eq!(stuffed(b"a\rb\n.c"), b"a\r\nb\r\n..c\r\n.\r\n");
}

fn with_inbox(messages: &[&[u8]]) -> Fake {
    let mut state = State::default();
    if let Some(inbox) = state.mailbox("INBOX") {
        for m in messages {
            inbox.add(m, &[]);
        }
    }
    Fake::new(state)
}

#[test]
fn pop3_lists_retrieves_and_deletes() {
    let fake = with_inbox(&[MESSAGE, b"Subject: two\r\n\r\nx\r\n"]);
    let mut p = Pop3::open(&fake, &Fake::endpoint(Protocol::Pop3), &login()).unwrap();
    let listed = p.list().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].uid, "uid-1");
    assert_eq!(listed[0].size, MESSAGE.len() as u64);
    let got = p.retrieve(1).unwrap();
    assert_eq!(String::from_utf8_lossy(&got), String::from_utf8_lossy(MESSAGE).replace("bare\nends\n", "bare\r\nends\r\n"));
    p.delete(1).unwrap();
    assert!(p.retrieve(1).is_err());
    p.quit().unwrap();
    let mut state = fake.state();
    assert_eq!(state.mailbox("INBOX").unwrap().messages.len(), 1);
    assert_eq!(state.secured, 1);
    // Signed in with SASL PLAIN, which the server offered: no password on a line of its own.
    assert!(state.heard.iter().any(|l| l.starts_with("AUTH PLAIN ")));
    assert!(!state.heard.iter().any(|l| l.starts_with("PASS")));
}

#[test]
fn pop3_signs_in_with_user_and_pass_without_sasl() {
    let mut state = State::default();
    state.pop_caps.retain(|c| !c.starts_with("SASL"));
    let fake = Fake::new(state);
    let mut p = Pop3::open(&fake, &tls(Protocol::Pop3), &login()).unwrap();
    assert!(p.list().unwrap().is_empty());
    p.quit().unwrap();
    assert!(fake.state().heard.iter().any(|l| l.starts_with("PASS ")));
    let wrong = Login { password: Secret::new("wrong"), ..login() };
    assert!(matches!(Pop3::open(&fake, &tls(Protocol::Pop3), &wrong), Err(NetError::Auth(_))));
}

#[test]
fn imap_lists_folders_reads_flags_and_appends() {
    let mut state = State::default();
    let mut drafts = crate::fake::Mailbox::new("Entw&APw-rfe", &["\\HasNoChildren"]);
    drafts.uid_validity = 9;
    state.mailboxes.push(drafts);
    if let Some(inbox) = state.mailbox("INBOX") {
        inbox.add(MESSAGE, &["\\Seen"]);
        inbox.add(b"Subject: (two) {5}\r\n\r\n\"quoted\" )(\r\n", &[]);
    }
    let fake = Fake::new(state);
    let mut i = Imap::open(&fake, &Fake::endpoint(Protocol::Imap), &login()).unwrap();
    assert!(i.has("UIDPLUS"));
    // Signing in was offered only before it: capabilities are asked again afterwards.
    assert!(!i.has("AUTH=PLAIN"));
    let folders = i.folders().unwrap();
    let names: Vec<&str> = folders.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Inbox", "Sent", "Drafts", "Trash", "Entwürfe"]);
    assert!(folders[1].has("\\Sent"));
    assert_eq!(folders[4].raw, "Entw&APw-rfe");
    let sel = i.select("INBOX").unwrap();
    assert_eq!((sel.exists, sel.uid_validity, sel.uid_next, sel.read_only), (2, Some(7), Some(3), false));
    let list = i.list(1).unwrap();
    assert_eq!(list.iter().map(|s| (s.uid, s.size)).collect::<Vec<_>>(), [(1, MESSAGE.len() as u64), (2, 35)]);
    assert_eq!(list[0].flags, ["\\Seen"]);
    // Nothing new after the last: the last is not taken for new.
    assert!(i.list(3).unwrap().is_empty());
    assert_eq!(i.fetch(2).unwrap().unwrap(), b"Subject: (two) {5}\r\n\r\n\"quoted\" )(\r\n");
    assert_eq!(i.fetch(1).unwrap().unwrap(), MESSAGE);
    assert_eq!(i.fetch(99).unwrap(), None);
    i.flag(2, true, &["\\Seen", "\\Flagged"]).unwrap();
    i.flag(1, false, &["\\Seen"]).unwrap();
    assert!(i.flag(1, true, &["\\Seen) (x"]).is_err());
    let list = i.list(1).unwrap();
    assert_eq!((list[0].flags.len(), list[1].flags.len()), (0, 2));
    // An append waits to be told to go on, then sends the message as a literal.
    assert_eq!(i.append("Sent", &["\\Seen"], MESSAGE).unwrap(), Some(1));
    assert!(matches!(i.select("Nowhere"), Err(NetError::Refused(_))));
    i.logout().unwrap();
    let mut state = fake.state();
    assert_eq!(state.mailbox("Sent").unwrap().messages[0].data, MESSAGE);
    assert_eq!(state.mailbox("Sent").unwrap().messages[0].flags, ["\\Seen"]);
    assert_eq!(state.secured, 1);
    assert!(state.heard.iter().any(|l| l.contains("BODY.PEEK[]")));
}

#[test]
fn imap_uses_literal_plus_and_login_when_offered() {
    let mut state = State::default();
    state.imap_caps.push("LITERAL+".into());
    state.imap_without.push("AUTH=PLAIN".into());
    state.password = "pässword \"with\" quotes\\".into();
    let user = Login { user: state.user.clone(), password: Secret::new(state.password.clone()) };
    let fake = Fake::new(state);
    let mut i = Imap::open(&fake, &tls(Protocol::Imap), &user).unwrap();
    assert_eq!(i.append("INBOX", &[], b"Subject: x\r\n\r\ny\r\n").unwrap(), Some(1));
    i.logout().unwrap();
    let state = fake.state();
    assert!(state.heard.iter().any(|l| l.contains(" LOGIN ")));
    assert!(state.heard.iter().any(|l| l.contains("{17+}")));
    assert_eq!(state.secured, 0);
}

#[test]
fn imap_refuses_a_session_signed_in_before_it_was_made_secure() {
    let fake = Fake::script(b"* PREAUTH welcome\r\n");
    assert!(matches!(Imap::open(&fake, &Fake::endpoint(Protocol::Imap), &login()), Err(NetError::NotSecure)));
}

#[test]
fn what_was_said_in_the_clear_after_starttls_is_refused() {
    // Someone between us and the server adds an answer behind the server's go-ahead.
    let fake = Fake::script(b"220 hi\r\n250-x\r\n250 STARTTLS\r\n220 go\r\n250 injected\r\n");
    let r = Smtp::open(&fake, &Fake::endpoint(Protocol::Smtp), Some(&login()));
    assert!(matches!(r, Err(NetError::Protocol(ref w)) if w.contains("before the connection was made secure")), "{:?}", r.err());
    let fake = Fake::script(b"* OK [CAPABILITY IMAP4rev1 STARTTLS] hi\r\nA0001 OK go\r\n* OK injected\r\n");
    assert!(matches!(Imap::open(&fake, &Fake::endpoint(Protocol::Imap), &login()), Err(NetError::Protocol(_))));
}

#[test]
fn mailbox_names_in_modified_utf7() {
    for (raw, name) in [
        ("INBOX", "INBOX"),
        ("Entw&APw-rfe", "Entwürfe"),
        ("&-", "&"),
        ("~peter/mail/&U,BTFw-/&ZeVnLIqe-", "~peter/mail/台北/日本語"),
        ("a&2D3eAA-b", "a😀b"),
    ] {
        assert_eq!(decode_mailbox_name(raw), name);
        assert_eq!(encode_mailbox_name(name), raw);
    }
    // What does not decode is kept as it came.
    for raw in ["&", "&%%-", "a&AAA-", "&2D3-"] {
        let _ = decode_mailbox_name(raw);
    }
    assert_eq!(decode_mailbox_name("&"), "&");
}

#[test]
fn a_password_never_prints() {
    let l = login();
    assert!(!format!("{l:?}").contains(l.password.expose()));
    assert_eq!(format!("{:?}", Secret::new("x")), "Secret(..)");
}

#[test]
fn imap_responses_parse() {
    let r = imap::parse(b"* 12 FETCH (UID 4 FLAGS (\\Seen $Junk) BODY[HEADER.FIELDS (SUBJECT)] {5}\r\nhello RFC822.SIZE 99)").unwrap();
    let Resp::Data { number, name, rest } = r else { panic!() };
    assert_eq!((number, name.as_str()), (Some(12), "FETCH"));
    let Tok::List(items) = &rest[0] else { panic!() };
    assert_eq!(items[4], Tok::Atom("BODY[HEADER.FIELDS (SUBJECT)]".into()));
    assert_eq!(items[5], Tok::Str(b"hello".to_vec()));
    let r = imap::parse(b"A0001 OK [APPENDUID 3 9] done [really]").unwrap();
    assert_eq!(r, Resp::Status { tag: Some("A0001".into()), kind: imap::Kind::Ok, code: Some("APPENDUID 3 9".into()), text: "done [really]".into() });
    let r = imap::parse(b"* LIST (\\HasNoChildren) NIL \"a \\\"b\\\"\"").unwrap();
    let Resp::Data { rest, .. } = r else { panic!() };
    assert_eq!(rest[1], Tok::Nil);
    assert_eq!(rest[2], Tok::Str(b"a \"b\"".to_vec()));
}

/// Bytes that look like a server's, made from a seed.
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let parts: [&[u8]; 24] = [
        b"* ",
        b"OK ",
        b"A0001 ",
        b"+ ",
        b"(",
        b")",
        b"{",
        b"}",
        b"\r\n",
        b"\"",
        b"\\",
        b"[",
        b"]",
        b"250-",
        b"250 ",
        b"220 ",
        b"+OK",
        b"-ERR",
        b".\r\n",
        b"{999999999999}\r\n",
        b"FETCH ",
        b"UID ",
        b"BODY[] ",
        b"\xff\xfe",
    ];
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut out = Vec::new();
    while out.len() < len {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.extend_from_slice(parts[(x % 24) as usize]);
        if x.is_multiple_of(5) {
            out.push((x >> 8) as u8);
        }
    }
    out
}

#[test]
fn hostile_servers_are_errors_never_panics() {
    let long_line = vec![b'a'; crate::conn::MAX_LINE + 10];
    let deep = [b"* 1 FETCH ".as_slice(), &[b'('; 200], b"\r\n"].concat();
    let many_lines: Vec<u8> = b"220 hi\r\n".iter().copied().chain(std::iter::repeat_n(b"250-x\r\n".iter().copied(), 1_100).flatten()).collect();
    let scripts: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"garbage\r\n".to_vec(),
        long_line,
        b"* OK hi\r\n* 1 FETCH (BODY[] {99999999999999999}\r\n".to_vec(),
        b"* OK hi\r\n* 1 FETCH (BODY[] {18446744073709551615}\r\n".to_vec(),
        [b"* OK hi\r\n".as_slice(), &deep].concat(),
        b"* OK hi\r\nA0001 OK x\r\n* LIST (\"unclosed\r\n".to_vec(),
        many_lines,
        b"+OK hi\r\n+OK\r\nSASL PLAIN\r\nSTLS\r\n.\r\n+OK\r\n".to_vec(),
        b"+OK\r\n+OK\r\n.\r\n+OK\r\n+OK\r\nnot a number uid\r\n.\r\n".to_vec(),
        b"999 nope\r\n".to_vec(),
    ];
    let ends = [Protocol::Imap, Protocol::Pop3, Protocol::Smtp].map(tls);
    let run = |bytes: &[u8]| {
        for to in &ends {
            let fake = Fake::script(bytes);
            match to.protocol {
                Protocol::Imap => {
                    if let Ok(mut i) = Imap::open(&fake, to, &login()) {
                        let _ = i.folders();
                        let _ = i.select("INBOX");
                        let _ = i.select_since("INBOX", Some((7, 3)));
                        let _ = i.changed_since(1);
                        let _ = i.still_there(&[1, 2]);
                        let _ = i.idle(std::time::Duration::from_millis(5), &mut || false);
                        let _ = i.list(1);
                        let _ = i.fetch(1);
                    }
                }
                Protocol::Pop3 => {
                    if let Ok(mut p) = Pop3::open(&fake, to, &login()) {
                        let _ = p.list();
                        let _ = p.retrieve(1);
                    }
                }
                Protocol::Smtp => {
                    if let Ok(mut s) = Smtp::open(&fake, to, Some(&login())) {
                        let _ = s.send("a@b.test", &["c@d.test".into()], b"x");
                    }
                }
                Protocol::Https => {}
            }
        }
    };
    for s in &scripts {
        run(s);
        let _ = crate::http::get(&Fake::script(s), "https://pictures.example.com/a.png", 4096);
    }
    for seed in 0..150 {
        run(&noise(seed, 64 + (seed as usize % 7) * 300));
    }
    for seed in 0..300 {
        let _ = imap::parse(&noise(seed, 200));
    }
}

#[test]
fn sets_of_ids_are_read_as_servers_write_them_and_nonsense_is_none() {
    let set = imap::UidSet::parse("3,5:9,20:*").unwrap();
    assert!([3, 5, 7, 9, 20, 4_000_000_000].iter().all(|u| set.contains(*u)));
    assert!(![1, 4, 10, 19].iter().any(|u| set.contains(*u)));
    // Written high to low is the same run.
    assert!(imap::UidSet::parse("9:5").unwrap().contains(6));
    for bad in ["", ",", "0", "1:0", "a", "1,,2", "-1", "1:2:3", "4294967296", "1 2", "*:x"] {
        assert_eq!(imap::UidSet::parse(bad), None, "{bad:?}");
    }
    // A run of every id there is costs no more than one; more runs than are kept is none.
    assert!(imap::UidSet::parse("1:4294967295").unwrap().contains(u32::MAX));
    let many = vec!["1"; 100_001].join(",");
    assert_eq!(imap::UidSet::parse(&many), None);
}

#[test]
fn mod_sequences_are_read_and_qresync_says_what_went_and_changed() {
    let mut state = State::default();
    state.imap_caps.extend(["CONDSTORE", "QRESYNC", "ENABLE"].map(String::from));
    if let Some(inbox) = state.mailbox("INBOX") {
        for _ in 0..4 {
            inbox.add(MESSAGE, &[]);
        }
    }
    let fake = Fake::new(state);
    let mut i = Imap::open(&fake, &Fake::endpoint(Protocol::Imap), &login()).unwrap();
    assert!(i.qresync() && i.has_modseqs());
    let first = i.select_since("INBOX", None).unwrap();
    let at = first.highest_modseq.unwrap();
    assert_eq!(first.since, None);
    {
        let mut state = fake.state();
        let inbox = state.mailbox("INBOX").unwrap();
        inbox.set_flags(2, &["\\Seen"]);
        inbox.remove(3);
    }
    let again = i.select_since("INBOX", Some((first.uid_validity.unwrap(), at))).unwrap();
    assert!(again.highest_modseq.unwrap() > at);
    let since = again.since.unwrap();
    assert!(since.vanished.contains(3) && !since.vanished.contains(2));
    assert_eq!(since.changed.iter().map(|m| (m.uid, m.flags.clone())).collect::<Vec<_>>(), [(2, vec!["\\Seen".to_string()])]);
    assert_eq!(i.still_there(&[1, 3, 4]).unwrap(), [1, 4].into_iter().collect());
    assert!(i.changed_since(again.highest_modseq.unwrap()).unwrap().is_empty());
    i.logout().unwrap();

    // Where QRESYNC is not offered it is not turned on, and CONDSTORE is asked for by SELECT.
    let mut state = State::default();
    state.imap_caps.push("CONDSTORE".into());
    let fake = Fake::new(state);
    let mut i = Imap::open(&fake, &Fake::endpoint(Protocol::Imap), &login()).unwrap();
    assert!(!i.qresync() && i.has_modseqs());
    let s = i.select_since("INBOX", Some((7, 3))).unwrap();
    assert!(s.highest_modseq.is_some() && s.since.is_none());
    assert!(fake.state().heard.iter().any(|c| c.ends_with("SELECT \"INBOX\" (CONDSTORE)")));
}

#[test]
fn mod_sequences_that_cannot_be_are_none() {
    for (said, kept) in [("5", Some(5)), ("0", None), ("9223372036854775808", None), ("99999999999999999999999", None), ("-1", None), ("x", None)] {
        let script = format!(
            "* OK hi\r\nA0001 OK in\r\n* CAPABILITY IMAP4rev1 CONDSTORE\r\nA0002 OK caps\r\n* 3 EXISTS\r\n* OK [HIGHESTMODSEQ {said}] x\r\n* VANISHED (EARLIER) 0:{said}\r\nA0003 OK [READ-WRITE] in\r\n"
        );
        let fake = Fake::script(script.as_bytes());
        let mut i = Imap::open(&fake, &tls(Protocol::Imap), &login()).unwrap();
        let s = i.select_since("INBOX", Some((7, 3))).unwrap();
        assert_eq!((s.exists, s.highest_modseq), (3, kept), "{said}");
    }
}

#[test]
fn idle_waits_until_the_folder_changes_and_stops_when_asked() {
    use std::time::Duration;
    let mut state = State::default();
    state.imap_caps.push("IDLE".into());
    let fake = Fake::new(state);
    let mut i = Imap::open(&fake, &Fake::endpoint(Protocol::Imap), &login()).unwrap();
    assert_eq!(i.examine("INBOX").unwrap().exists, 0);
    // Nothing changes: it waits as long as it is told to, then says DONE.
    let mut asked = 0;
    let woke = i
        .idle(Duration::from_millis(10), &mut || {
            asked += 1;
            asked < 3
        })
        .unwrap();
    assert_eq!((woke, asked), (imap::Woke::default(), 3));
    // A message that comes while it waits wakes it, with how many there are now.
    let elsewhere = fake.clone();
    let comes = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        elsewhere.state().mailbox("INBOX").unwrap().add(MESSAGE, &[]);
    });
    let woke = i.idle(Duration::from_millis(10), &mut || true).unwrap();
    comes.join().unwrap();
    assert_eq!(woke, imap::Woke { exists: Some(1), changed: true });
    // A flag changed elsewhere wakes it too.
    let elsewhere = fake.clone();
    let comes = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        elsewhere.state().mailbox("INBOX").unwrap().set_flags(1, &["\\Seen"]);
    });
    assert!(i.idle(Duration::from_millis(10), &mut || true).unwrap().changed);
    comes.join().unwrap();
    // The session goes on as before: commands are answered at their own pace again.
    assert_eq!(i.list(1).unwrap().len(), 1);
    i.logout().unwrap();
    let state = fake.state();
    assert_eq!(state.idling, 3);
    assert_eq!(state.heard.iter().filter(|l| l.as_str() == "DONE").count(), 3);

    // A server without IDLE is not waited on.
    let fake = Fake::default();
    let mut i = Imap::open(&fake, &Fake::endpoint(Protocol::Imap), &login()).unwrap();
    i.examine("INBOX").unwrap();
    assert!(matches!(i.idle(Duration::from_millis(10), &mut || true), Err(NetError::Unsupported(_))));
}

#[test]
fn a_server_that_ends_a_wait_or_chatters_is_dealt_with() {
    use std::time::Duration;
    let head = "* OK hi\r\nA0001 OK in\r\n* CAPABILITY IMAP4rev1 IDLE\r\nA0002 OK caps\r\n";
    // It answers the IDLE itself: nothing is said DONE to.
    let fake = Fake::script(format!("{head}+ idling\r\n* 4 EXISTS\r\nA0003 OK done\r\n").as_bytes());
    let mut i = Imap::open(&fake, &tls(Protocol::Imap), &login()).unwrap();
    assert_eq!(i.idle(Duration::from_millis(10), &mut || true).unwrap().exists, Some(4));
    let fake = Fake::script(format!("{head}+ idling\r\nA0003 OK enough\r\n").as_bytes());
    let mut i = Imap::open(&fake, &tls(Protocol::Imap), &login()).unwrap();
    assert_eq!(i.idle(Duration::from_millis(10), &mut || true).unwrap(), imap::Woke::default());
    // It goes away, or refuses.
    for end in ["+ idling\r\n* BYE going\r\n", "A0003 NO not now\r\n", "* BYE going\r\n", "+ idling\r\nA0009 OK what\r\n"] {
        let fake = Fake::script(format!("{head}{end}").as_bytes());
        let mut i = Imap::open(&fake, &tls(Protocol::Imap), &login()).unwrap();
        assert!(i.idle(Duration::from_millis(10), &mut || true).is_err(), "{end:?}");
    }
    // It says nothing that matters, without end: it is asked after, and left in the end.
    let chatter = "* OK still here\r\n".repeat(10_050);
    let fake = Fake::script(format!("{head}+ idling\r\n{chatter}").as_bytes());
    let mut i = Imap::open(&fake, &tls(Protocol::Imap), &login()).unwrap();
    let mut asked = 0;
    assert!(
        i.idle(Duration::from_millis(10), &mut || {
            asked += 1;
            true
        })
        .is_err()
    );
    assert!(asked >= 100, "{asked}");
}

#[test]
fn several_messages_are_fetched_in_one_command_in_the_order_asked_and_gone_ones_left_out() {
    let mut state = State::default();
    if let Some(inbox) = state.mailbox("INBOX") {
        inbox.add(b"Subject: one\r\n\r\n1\r\n", &[]);
        inbox.add(b"Subject: two\r\n\r\n2\r\n", &[]);
        inbox.add(b"Subject: three\r\n\r\n3\r\n", &[]);
    }
    let fake = Fake::new(state);
    let mut i = Imap::open(&fake, &Fake::endpoint(Protocol::Imap), &login()).unwrap();
    i.select("INBOX").unwrap();
    let got = i.fetch_many(&[3, 99, 1]).unwrap();
    assert_eq!(got.iter().map(|(u, _)| *u).collect::<Vec<_>>(), [3, 1]);
    assert_eq!(got[1].1, b"Subject: one\r\n\r\n1\r\n");
    assert!(i.fetch_many(&[]).unwrap().is_empty());
    assert!(matches!(i.fetch_many(&vec![1; 100_000]), Err(NetError::TooLarge(_))));
    let fetches = fake.state().heard.iter().filter(|l| l.contains("UID FETCH 3,99,1")).count();
    assert_eq!(fetches, 1, "one command for all of them");
}
