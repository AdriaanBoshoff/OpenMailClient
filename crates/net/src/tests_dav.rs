//! Calendars on a CalDAV server, against the fake one: found, synced by token and by ETag,
//! events put and deleted only while nobody else changed them, and hostile answers refused.

use crate::dav::{Calendar, Dav, Deleted, Put, Sync, Url, starts};
use crate::dns::Record;
use crate::fake::{DAV_HOME, Fake, State};
use crate::{Login, NetError, Secret};

const PERSONAL: &str = "/dav/calendars/pat/personal/";

fn login() -> Login {
    let s = State::default();
    Login { user: s.user, password: Secret::new(s.password) }
}

fn event(uid: &str, summary: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTART:20261020T090000Z\r\nDTEND:20261020T100000Z\r\nSUMMARY:{summary}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
}

fn calendars(fake: &Fake, at: &str) -> Result<Vec<Calendar>, NetError> {
    let login = login();
    Dav::new(fake, &login, url(at), None).calendars()
}

#[test]
fn only_https_addresses_are_taken() {
    assert_eq!(url("https://Dav.Example.test"), Url { host: "dav.example.test".into(), port: 443, path: "/".into() });
    assert_eq!(
        url("https://dav.example.test:8443/remote.php/dav/?x#y"),
        Url { host: "dav.example.test".into(), port: 8443, path: "/remote.php/dav/?x".into() }
    );
    assert_eq!(url("https://[::1]:5232/").host, "::1", "a server of the owner's own, named by them");
    assert_eq!(url("https://dav.example.test:8443/a/").text(), "https://dav.example.test:8443/a/");
    for refused in [
        "http://dav.example.test/",
        "dav.example.test",
        "https://pat:pw@dav.example.test/",
        "https://dav.example.test:0/",
        "https://dav.example.test/a b",
        "https://dav.example.test/caf\u{e9}/",
        "https://dav_x.example.test/",
        "https:///",
    ] {
        assert!(Url::parse(refused).is_err(), "{refused}");
    }
}

#[test]
fn a_servers_calendars_are_found_from_its_address_or_its_well_known_place() {
    let fake = Fake::default();
    fake.state().calendars.push({
        let mut shared = crate::fake::DavCalendar::new("/dav/calendars/pat/team/", "Team & Co");
        shared.read_only = true;
        shared.color = "#ff8800".into();
        shared
    });
    for at in [
        "https://dav.example.test/",
        "https://dav.example.test/dav/",
        "https://dav.example.test/dav/principals/pat/",
        "https://dav.example.test/dav/calendars/pat/",
    ] {
        let found = calendars(&fake, at).unwrap();
        assert_eq!(found.len(), 2, "{at}");
        let (mine, team) = (&found[0], &found[1]);
        assert_eq!((mine.href.as_str(), mine.name.as_str(), mine.color.as_str()), (PERSONAL, "Personal", "#3366CC"));
        assert!(mine.writable && mine.events && !mine.token.is_empty() && !mine.ctag.is_empty());
        assert_eq!((team.name.as_str(), team.color.as_str(), team.writable), ("Team & Co", "#FF8800", false));
    }
    // A server that keeps address books too names a home for each: the calendars are asked
    // for in theirs and the books in theirs, from the same address.
    fake.state().calendars.push(crate::fake::DavCalendar::book("/dav/addressbooks/pat/friends/", "Friends"));
    for at in ["https://dav.example.test/", "https://dav.example.test/dav/principals/pat/"] {
        assert_eq!(calendars(&fake, at).unwrap().len(), 2, "{at}: the calendars alone");
        let login = login();
        let books = Dav::new(&fake, &login, url(at), None).holding(crate::dav::Holds::Cards).calendars().unwrap();
        assert_eq!(books.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Friends"], "{at}");
    }
    // Named by its own path, a calendar is the one listed.
    let one = calendars(&fake, "https://dav.example.test/dav/calendars/pat/team/").unwrap();
    assert_eq!(one.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Team & Co"]);
    // No password goes anywhere but in the head of each request over TLS.
    assert!(fake.state().heard.iter().all(|h| !h.contains("not a real password")));
    // A wrong password is said to be one.
    let wrong = Login { user: "pat@example.test".into(), password: Secret::new("nope") };
    let err = Dav::new(&fake, &wrong, url("https://dav.example.test/"), None).calendars().unwrap_err();
    assert!(matches!(err, NetError::Auth(_)), "{err}");
}

#[test]
fn events_are_put_changed_and_deleted_only_as_last_read() {
    let fake = Fake::default();
    let login = login();
    let dav = Dav::new(&fake, &login, url("https://dav.example.test/dav/"), None);
    let href = format!("{PERSONAL}one.ics");
    let Put::Done(Some(etag)) = dav.put(&href, event("one", "Dentist").as_bytes(), None).unwrap() else { panic!("not put") };
    // One by that name is there already: a new one is not put over it.
    assert_eq!(dav.put(&href, event("one", "Other").as_bytes(), None).unwrap(), Put::Conflict);
    // Changed elsewhere, the copy read here is stale and is not put over it.
    fake.state().dav_put(PERSONAL, "one.ics", &event("one", "Dentist (moved)"));
    assert_eq!(dav.put(&href, event("one", "Mine").as_bytes(), Some(&etag)).unwrap(), Put::Conflict);
    assert_eq!(dav.delete(&href, &etag).unwrap(), Deleted::Conflict);
    let now = dav.list(PERSONAL).unwrap();
    assert_eq!(now.len(), 1);
    let got = dav.fetch(PERSONAL, &[href.clone(), format!("{PERSONAL}none.ics")]).unwrap();
    assert_eq!(got.len(), 1);
    assert!(got[0].data.contains("SUMMARY:Dentist (moved)") && got[0].etag == now[0].etag);
    let Put::Done(Some(newer)) = dav.put(&href, event("one", "Mine").as_bytes(), Some(&now[0].etag)).unwrap() else { panic!("not put") };
    assert_eq!(fake.state().dav_calendar(PERSONAL).unwrap().objects[0].data, event("one", "Mine"));
    assert_eq!(dav.delete(&href, &newer).unwrap(), Deleted::Done);
    assert_eq!(dav.delete(&href, &newer).unwrap(), Deleted::Gone);
    // A server that does not say the new ETag: put, and it is read again by the caller.
    fake.state().dav_no_etag = true;
    assert_eq!(dav.put(&format!("{PERSONAL}two.ics"), event("two", "x").as_bytes(), None).unwrap(), Put::Done(None));
    // A read-only calendar takes nothing.
    fake.state().calendars[0].read_only = true;
    assert!(dav.put(&format!("{PERSONAL}three.ics"), event("three", "x").as_bytes(), None).is_err());
}

#[test]
fn changes_are_asked_for_by_token_and_the_whole_list_read_when_it_is_lost() {
    let fake = Fake::default();
    let login = login();
    let dav = Dav::new(&fake, &login, url("https://dav.example.test/dav/"), None);
    for n in 0..5 {
        fake.state().dav_put(PERSONAL, &format!("e{n}.ics"), &event(&format!("e{n}"), "x"));
    }
    let Sync::Changes(first) = dav.changes(PERSONAL, "").unwrap() else { panic!("no sync") };
    assert_eq!((first.changed.len(), first.removed.len(), first.partial), (5, 0, false));
    // Nothing new: nothing listed.
    let Sync::Changes(none) = dav.changes(PERSONAL, &first.token).unwrap() else { panic!("no sync") };
    assert!(none.changed.is_empty() && none.removed.is_empty() && none.token == first.token);
    fake.state().dav_put(PERSONAL, "e1.ics", &event("e1", "changed"));
    fake.state().dav_remove(PERSONAL, "e3.ics");
    fake.state().dav_put(PERSONAL, "e9.ics", &event("e9", "new"));
    let Sync::Changes(next) = dav.changes(PERSONAL, &first.token).unwrap() else { panic!("no sync") };
    let mut changed: Vec<_> = next.changed.iter().map(|e| e.href.rsplit('/').next().unwrap().to_string()).collect();
    changed.sort();
    assert_eq!(changed, ["e1.ics", "e9.ics"]);
    assert_eq!(next.removed, [format!("{PERSONAL}e3.ics")]);

    // A list the server cuts short is asked for again from where it stopped.
    fake.state().dav_page = Some(2);
    let Sync::Changes(paged) = dav.changes(PERSONAL, "").unwrap() else { panic!("no sync") };
    assert_eq!((paged.changed.len(), paged.partial), (5, false));
    assert_eq!(paged.token, next.token);

    // A token the server forgot: the whole list is read instead.
    fake.state().dav_forget_before = 1_000;
    assert_eq!(dav.changes(PERSONAL, &first.token).unwrap(), Sync::Lost);
    // A server that keeps no tokens.
    fake.state().dav_sync = false;
    assert_eq!(dav.changes(PERSONAL, "").unwrap(), Sync::NotKept);
    assert_eq!(dav.list(PERSONAL).unwrap().len(), 5);
}

#[test]
fn a_server_is_found_from_the_address_domain() {
    let fake = Fake::default();
    fake.state()
        .records
        .push(("_caldavs._tcp.example.test".into(), Record::Srv { priority: 10, weight: 5, port: 8443, target: "dav.example.test.".into() }));
    fake.state().records.push(("_caldavs._tcp.example.test".into(), Record::Srv { priority: 0, weight: 0, port: 0, target: ".".into() }));
    let found = starts(&fake, "pat@Example.Test");
    assert_eq!(found, [url("https://dav.example.test:8443/.well-known/caldav"), url("https://example.test/.well-known/caldav")]);
    assert!(starts(&fake, "no address").is_empty());
    assert!(starts(&fake, "pat@bad_domain").is_empty());
}

/// The fake as a server that sends these bytes, whatever it is asked.
fn hostile(bytes: &[u8]) -> Result<Vec<Calendar>, NetError> {
    let fake = Fake::script(bytes);
    let login = login();
    Dav::new(&fake, &login, url("https://dav.example.test/dav/calendars/pat/"), None).calendars()
}

#[test]
fn hostile_answers_are_errors_not_crashes() {
    let wrap = |body: &str| format!("HTTP/1.1 207 Multi-Status\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes();
    // Too large, by its length or chunked.
    assert!(matches!(hostile(b"HTTP/1.1 207 x\r\nContent-Length: 99999999999\r\n\r\n"), Err(NetError::TooLarge(_))));
    assert!(hostile(b"HTTP/1.1 207 x\r\nTransfer-Encoding: chunked\r\n\r\nFFFFFFFFFFFF\r\n").is_err());
    assert!(hostile(b"HTTP/1.1 207 x\r\nContent-Length: nine\r\n\r\n").is_err());
    // A head that does not end.
    assert!(hostile(&b"HTTP/1.1 207 x\r\n".iter().chain(b"X: y\r\n".repeat(500).iter()).copied().collect::<Vec<_>>()).is_err());
    assert!(hostile(b"SMTP 220 hello\r\n\r\n").is_err());
    assert!(hostile(&b"HTTP/1.1 100 Continue\r\n\r\n".repeat(10)).is_err());
    // XML too deep, with a DTD, not a multistatus or not UTF-8.
    let deep = format!("<d:multistatus xmlns:d=\"DAV:\">{}{}</d:multistatus>", "<a>".repeat(100), "</a>".repeat(100));
    assert!(hostile(&wrap(&deep)).is_err());
    assert!(hostile(&wrap("<!DOCTYPE x [<!ENTITY a \"aaaa\">]><d:multistatus xmlns:d=\"DAV:\">&a;</d:multistatus>")).is_err());
    assert!(hostile(&wrap("<html>hello</html>")).is_err());
    assert!(hostile(b"HTTP/1.1 207 x\r\nContent-Length: 2\r\n\r\n\xff\xfe").is_err());
    // A calendar on another server, or with a path that could not be sent, is not taken.
    let elsewhere = wrap(
        "<d:multistatus xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\">\
         <d:response><d:href>https://evil.test/cal/</d:href><d:propstat><d:prop><d:resourcetype><c:calendar/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>\
         <d:response><d:href>/cal/a b/</d:href><d:propstat><d:prop><d:resourcetype><c:calendar/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>\
         <d:response><d:href>/cal/ok/</d:href><d:propstat><d:prop><d:resourcetype><c:calendar/></d:resourcetype><d:displayname>a\u{7}b</d:displayname></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>\
         </d:multistatus>",
    );
    let fake = Fake::script(&elsewhere);
    let login = login();
    let mut dav = Dav::new(&fake, &login, url("https://dav.example.test/dav/calendars/pat/"), None);
    // The script answers every request alike: ask its home's calendars alone.
    let found = dav.calendars().unwrap_or_default();
    assert!(found.iter().all(|c| c.href == "/cal/ok/" && c.name == "ab"), "{found:?}");
    // A redirect to a server on this machine, or of another domain, is not followed.
    for to in ["https://127.0.0.1/dav/", "https://evil.test/dav/", "http://dav.example.test/dav/"] {
        let fake = Fake::script(format!("HTTP/1.1 301 Moved\r\nLocation: {to}\r\nContent-Length: 0\r\n\r\n").as_bytes());
        let mut dav = Dav::new(&fake, &login, url("https://dav.example.test/"), None);
        assert!(dav.calendars().is_err(), "{to}");
    }
    // Hrefs and tokens with line breaks are never sent.
    let fake = Fake::default();
    let dav = Dav::new(&fake, &login, url("https://dav.example.test/"), None);
    assert!(dav.put("/dav/x\r\nHost: evil.test", b"x", None).is_err());
    assert!(dav.delete(&format!("{PERSONAL}a.ics"), "\"a\"\r\nX: y").is_err());
    assert!(dav.changes(PERSONAL, "<inject/>").is_ok_and(|s| s == Sync::Lost), "a token is put in the XML escaped");
    let _ = DAV_HOME;
}
