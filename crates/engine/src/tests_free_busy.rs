//! The Scheduling Assistant against the in-process fake: one's own times from the calendar,
//! an attendee's from the account's scheduling server, another's from the free/busy address
//! their contact names, one known nowhere, the times suggested, and work time.

use crate::Session;
use openmailclient_model::time;
use openmailclient_net::fake::Fake;
use serde_json::{Value, json};
use std::sync::Arc;

fn account(fake: &Fake, name: &str) -> (Session, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("omc-free-busy-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session::new();
    s.now = time::from_civil(2026, 10, 1, 9, 0, 0, 0);
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.zone = openmailclient_model::zone::Zone::fixed(0);
    s.connector = Some(Arc::new(fake.clone()));
    s.account_dir = Some(dir.join("Accounts"));
    s.journal_dir = Some(dir.join("journals"));
    s.run(
        "account.add",
        &json!({ "email": "pat@example.test", "kind": "imap",
                 "incoming": { "host": "mail.example.test", "security": "starttls" },
                 "outgoing": { "host": "mail.example.test", "security": "starttls" },
                 "caldav": "https://dav.example.test/", "password": "not a real password" }),
    )
    .unwrap();
    (s, dir)
}

fn done(mut s: Session, dir: std::path::PathBuf) {
    let _ = s.run("file.closeStore", &json!({ "store": 1, "discard": true }));
    let _ = std::fs::remove_dir_all(&dir);
}

fn page(body: &str) -> Vec<u8> {
    format!("HTTP/1.1 200 OK\r\nContent-Type: text/calendar\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

fn person<'a>(got: &'a Value, address: &str) -> &'a Value {
    got["untrusted_content"]["people"].as_array().unwrap().iter().find(|p| p["address"] == json!(address)).unwrap()
}

#[test]
fn free_and_busy_times_are_looked_up_and_times_suggested() {
    let fake = Fake::default();
    let reply = "BEGIN:VCALENDAR\r\nMETHOD:REPLY\r\nBEGIN:VFREEBUSY\r\nATTENDEE:mailto:sam@example.test\r\nFREEBUSY:20261012T100000Z/PT1H\r\nEND:VFREEBUSY\r\nEND:VCALENDAR\r\n";
    fake.state().free_busy.push(("sam@example.test".into(), reply.into()));
    let published = "BEGIN:VCALENDAR\r\nMETHOD:PUBLISH\r\nBEGIN:VFREEBUSY\r\nORGANIZER:mailto:ola@example.org\r\nFREEBUSY;FBTYPE=BUSY-TENTATIVE:20261012T110000Z/20261012T120000Z\r\nEND:VFREEBUSY\r\nEND:VCALENDAR\r\n";
    fake.state().pages.push(("fb.example.org/ola.ifb".into(), page(published)));
    let (mut s, dir) = account(&fake, "look");

    s.run("nav.module", &json!({ "module": "people" })).unwrap();
    s.run("people.new", &json!({ "given": "Ola", "emails": ["ola@example.org"], "freeBusy": "https://fb.example.org/ola.ifb" })).unwrap();
    s.run("nav.module", &json!({ "module": "calendar" })).unwrap();
    s.run("calendar.new", &json!({ "subject": "Planning", "start": "2026-10-12 08:00", "end": "2026-10-12 09:00" })).unwrap();

    let attendees = json!([
        "sam@example.test",
        "ola@example.org",
        { "address": "nobody@example.test", "role": "optional" },
        "pat@example.test",
    ]);
    let got = s.run("calendar.freeBusy", &json!({ "attendees": attendees, "start": "2026-10-12", "days": 1, "length": 60 })).unwrap();
    let people = got["untrusted_content"]["people"].as_array().unwrap();
    assert_eq!(people.len(), 4, "oneself, and three others; one's own address is not asked of: {got}");
    assert_eq!((&people[0]["organizer"], &people[0]["source"]), (&json!(true), &json!("calendar")));
    assert!(
        people[0]["busy"].as_array().unwrap().iter().any(|b| b["start"] == json!("2026-10-12 08:00") && b["end"] == json!("2026-10-12 09:00")),
        "{got}"
    );
    let sam = person(&got, "sam@example.test");
    assert_eq!(sam["source"], json!("server"));
    assert_eq!(sam["busy"], json!([{ "start": "2026-10-12 10:00", "end": "2026-10-12 11:00", "kind": "busy" }]));
    let ola = person(&got, "ola@example.org");
    assert_eq!(ola["source"], json!("internet"), "{got}");
    assert_eq!(ola["busy"], json!([{ "start": "2026-10-12 11:00", "end": "2026-10-12 12:00", "kind": "tentative" }]));
    let nobody = person(&got, "nobody@example.test");
    assert_eq!((&nobody["source"], &nobody["busy"], &nobody["required"]), (&json!("none"), &Value::Null, &json!(false)));
    assert!(!nobody["said"].as_str().unwrap().is_empty());

    // Every time suggested is free for everyone who must come, the best first.
    let suggested = got["suggested"].as_array().unwrap();
    assert!(!suggested.is_empty(), "{got}");
    for g in suggested {
        let start = g["start"].as_str().unwrap();
        assert!(!["2026-10-12 08:", "2026-10-12 10:", "2026-10-12 11:"].iter().any(|t| start.starts_with(t)), "{got}");
        assert!(g["end"].as_str().unwrap() <= "2026-10-12 17:00");
    }
    assert_eq!(suggested[0]["free"], suggested[0]["known"]);
    assert_eq!(s.scheduling.as_ref().map(|x| x.people.len()), Some(4));
    assert!(fake.state().heard.iter().any(|l| l.starts_with("POST /dav/calendars/pat/outbox/")));

    // A meeting's own attendees and length, the meeting itself not counted against oneself.
    let made = s
        .run(
            "calendar.newMeeting",
            &json!({ "subject": "Review", "start": "2026-10-12 13:00", "end": "2026-10-12 14:30", "attendees": "sam@example.test" }),
        )
        .unwrap();
    let id = made["id"].as_u64().unwrap();
    let got = s.run("calendar.freeBusy", &json!({ "id": id, "start": "2026-10-12", "days": 1 })).unwrap();
    assert_eq!(got["length"], json!(90));
    assert_eq!(got["untrusted_content"]["people"].as_array().unwrap().len(), 2);
    assert!(person(&got, "sam@example.test")["source"] == json!("server"));
    let mine = &got["untrusted_content"]["people"][0]["busy"];
    assert!(!mine.as_array().unwrap().iter().any(|b| b["start"] == json!("2026-10-12 13:00")), "{got}");
    assert!(got["suggested"].as_array().unwrap().iter().any(|g| g["start"] == json!("2026-10-12 13:00")), "{got}");

    // A server that schedules nothing: each falls back to their free/busy address, or none.
    fake.state().dav_outbox = false;
    let got = s.run("calendar.freeBusy", &json!({ "attendees": ["sam@example.test", "ola@example.org"], "start": "2026-10-12", "days": 1 })).unwrap();
    assert_eq!(person(&got, "sam@example.test")["source"], json!("none"));
    assert_eq!(person(&got, "ola@example.org")["source"], json!("internet"));
    done(s, dir);
}

#[test]
fn work_time_is_kept_and_suggestions_follow_it() {
    let fake = Fake::default();
    let (mut s, dir) = account(&fake, "work");
    assert_eq!(
        s.run("calendar.workTime", &json!({})).unwrap(),
        json!({ "start": "08:00", "end": "17:00", "days": ["mon", "tue", "wed", "thu", "fri"] })
    );
    let w = s.run("calendar.workTime", &json!({ "start": "10:00", "end": "12:00", "days": ["Saturday"] })).unwrap();
    assert_eq!(w, json!({ "start": "10:00", "end": "12:00", "days": ["sat"] }));
    // 2026-10-10 is a Saturday: one person alone, free from ten to noon.
    let got = s.run("calendar.freeBusy", &json!({ "attendees": [], "start": "2026-10-05", "days": 7, "length": 60 })).unwrap();
    let starts: Vec<&str> = got["suggested"].as_array().unwrap().iter().map(|g| g["start"].as_str().unwrap()).collect();
    assert_eq!(starts, ["2026-10-10 10:00", "2026-10-10 10:30", "2026-10-10 11:00"], "{got}");
    let saved = crate::cmd::settings::current(&s, None);
    assert_eq!(saved["workTime"]["days"], json!(["sat"]));

    for bad in [
        json!({ "start": "12:00", "end": "10:00" }),
        json!({ "start": "25:00" }),
        json!({ "start": "ten" }),
        json!({ "days": [] }),
        json!({ "days": ["someday"] }),
        json!({ "days": "mon" }),
        json!({ "end": 5 }),
    ] {
        assert!(s.run("calendar.workTime", &bad).is_err(), "{bad}");
    }
    assert_eq!(s.work.days, 1 << 6, "a refused change leaves it as it was");
    for bad in [
        json!({ "days": 0 }),
        json!({ "days": 63 }),
        json!({ "days": "x" }),
        json!({ "length": 0 }),
        json!({ "length": 1441 }),
        json!({ "start": "someday" }),
        json!({ "attendees": 5 }),
        json!({ "id": 999_999 }),
        json!({ "workTime": "all day" }),
    ] {
        assert!(s.run("calendar.freeBusy", &bad).is_err(), "{bad}");
    }
    done(s, dir);
}
