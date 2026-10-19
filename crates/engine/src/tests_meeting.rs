//! Meetings: invitations sent, answered and called off through the in-process fake server, both
//! as the organizer and as one asked. No socket is opened and nothing really goes; every address
//! is synthetic.

use crate::Session;
use openmailclient_net::fake::Fake;
use serde_json::{Value, json};
use std::sync::Arc;

fn session(fake: &Fake) -> Session {
    let mut s = Session::new();
    s.now = openmailclient_model::time::from_civil(2026, 10, 7, 14, 30, 0, 0);
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.connector = Some(Arc::new(fake.clone()));
    s.run(
        "account.add",
        &json!({
            "email": "pat@example.test", "displayName": "Pat Lee", "kind": "imap",
            "incoming": { "host": "mail.example.test", "security": "starttls" },
            "outgoing": { "host": "mail.example.test", "security": "starttls" },
            "password": "not a real password",
        }),
    )
    .unwrap();
    s
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.run(id, &params).unwrap_or_else(|e| panic!("{id} failed: {e}"))
}

/// The text of the `n`th message that went, its calendar part decoded.
fn went(fake: &Fake, n: usize) -> (Vec<String>, String, String) {
    let state = fake.state();
    let m = state.submitted.get(n).unwrap_or_else(|| panic!("only {} went", state.submitted.len()));
    let item = openmailclient_mime::parse(&m.data).unwrap();
    let cal = item
        .attachments
        .iter()
        .find(|a| a.mime() == "text/calendar")
        .map(|a| String::from_utf8_lossy(&a.data).replace("\r\n ", ""))
        .unwrap_or_default();
    (m.to.clone(), item.subject(), cal)
}

/// A message from `from` carrying a calendar of `method` about the event `body` says.
fn comes(fake: &Fake, from: &str, subject: &str, method: &str, body: &str) {
    let msg = format!(
        "From: {from}\r\nTo: pat@example.test\r\nSubject: {subject}\r\nDate: Tue, 13 Oct 2026 09:00:00 +0000\r\nMessage-ID: <{}@example.test>\r\nContent-Type: text/calendar; method={method}; charset=utf-8\r\n\r\nBEGIN:VCALENDAR\r\nMETHOD:{method}\r\nBEGIN:VEVENT\r\n{body}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        subject.replace(' ', "-")
    );
    fake.state().mailbox("INBOX").unwrap().add(msg.as_bytes(), &[]);
}

fn line_of<'a>(cal: &'a str, name: &str) -> &'a str {
    cal.split("\r\n").find(|l| l.starts_with(name)).unwrap_or("")
}

fn answer_of(s: &mut Session, id: u64, address: &str) -> String {
    let m = run(s, "calendar.attendees", json!({ "id": id }));
    m["attendees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["untrusted_content"]["address"] == address)
        .map(|p| p["response"].as_str().unwrap().to_string())
        .unwrap()
}

#[test]
fn a_meeting_is_sent_answered_updated_and_called_off_by_its_organizer() {
    let fake = Fake::default();
    let mut s = session(&fake);
    let made = run(
        &mut s,
        "calendar.newMeeting",
        json!({
            "subject": "Rota planning", "start": "2026-10-14 10:00", "end": "2026-10-14 11:00", "location": "Room 2",
            "attendees": [
                "June Okafor <june@example.test>",
                { "address": "tomas@example.test", "role": "optional" },
                { "address": "Pat@Example.test" },
                { "address": "room2@example.test", "name": "Room 2", "role": "resource" },
            ],
        }),
    );
    let id = made["id"].as_u64().unwrap();
    let m = run(&mut s, "calendar.attendees", json!({ "id": id }));
    assert_eq!(m["organizer"]["untrusted_content"]["address"], "pat@example.test");
    assert_eq!(m["attendees"].as_array().unwrap().len(), 3, "one's own address is not asked: {m}");
    assert_eq!((m["invited"].as_bool(), m["organized"].as_bool()), (Some(false), Some(true)));
    assert!(fake.state().submitted.is_empty(), "nothing goes until Send");

    let sent = run(&mut s, "calendar.sendInvitations", json!({ "id": id }));
    assert_eq!((sent["sent"].as_u64(), sent["update"].as_bool()), (Some(3), Some(false)), "{sent}");
    let (to, subject, cal) = went(&fake, 0);
    assert_eq!(to, ["june@example.test", "room2@example.test", "tomas@example.test"]);
    assert_eq!(subject, "Rota planning");
    assert!(cal.contains("METHOD:REQUEST\r\n") && cal.contains("SEQUENCE:0\r\n"), "{cal}");
    assert!(line_of(&cal, "ORGANIZER").ends_with("mailto:pat@example.test"));
    assert!(cal.contains("ROLE=OPT-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:tomas@example.test"), "{cal}");
    let uid = line_of(&cal, "UID:").trim_start_matches("UID:").to_string();
    assert!(!uid.is_empty());
    assert_eq!(run(&mut s, "calendar.attendees", json!({ "id": id }))["invited"], true);

    // June accepts; one who is not Tomas says Tomas declines, and is not believed.
    let event = format!("UID:{uid}\r\nDTSTART:20261014T100000Z\r\nSEQUENCE:0");
    comes(
        &fake,
        "June Okafor <june@example.test>",
        "Accepted: Rota planning",
        "REPLY",
        &format!("{event}\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:june@example.test"),
    );
    comes(
        &fake,
        "Mallory <mallory@example.test>",
        "Declined: Rota planning",
        "REPLY",
        &format!("{event}\r\nATTENDEE;PARTSTAT=DECLINED:mailto:tomas@example.test"),
    );
    let got = run(&mut s, "account.receive", Value::Null);
    assert_eq!(got["meetings"], json!(["June Okafor accepted"]), "{got}");
    assert!(s.status.contains("June Okafor accepted"), "{}", s.status);
    assert_eq!(answer_of(&mut s, id, "june@example.test"), "accepted");
    assert_eq!(answer_of(&mut s, id, "tomas@example.test"), "none");
    let tally = run(&mut s, "calendar.attendees", json!({ "id": id }))["tally"].clone();
    assert_eq!(tally, json!({ "accepted": 1, "tentative": 0, "declined": 0, "none": 2 }));

    // Changed and sent again: an update, its sequence one on; June's answer is kept.
    run(&mut s, "calendar.change", json!({ "id": id, "start": "2026-10-14 15:00" }));
    let sent = run(&mut s, "calendar.sendInvitations", json!({ "id": id }));
    assert_eq!((sent["update"].as_bool(), sent["sequence"].as_i64()), (Some(true), Some(1)));
    assert!(went(&fake, 1).2.contains("SEQUENCE:1\r\n"));
    assert_eq!(answer_of(&mut s, id, "june@example.test"), "accepted");

    // Called off: everyone told, and the meeting goes to Deleted Items.
    let done = run(&mut s, "calendar.cancelMeeting", json!({ "id": id, "comment": "Moved to next month" }));
    assert_eq!(done["told"], 3);
    let (to, subject, cal) = went(&fake, 2);
    assert_eq!(to.len(), 3);
    assert_eq!(subject, "Canceled: Rota planning");
    assert!(cal.contains("METHOD:CANCEL\r\n") && cal.contains("STATUS:CANCELLED\r\n") && cal.contains("SEQUENCE:2\r\n"), "{cal}");
    assert!(s.run("calendar.attendees", &json!({ "id": id })).is_err(), "it is no longer in the calendar");
}

#[test]
fn an_invitation_is_answered_and_only_its_organizer_calls_it_off() {
    let fake = Fake::default();
    let mut s = session(&fake);
    // Something is planned then already.
    run(&mut s, "calendar.new", json!({ "subject": "Dentist", "start": "2026-10-15 09:30", "end": "2026-10-15 10:30" }));
    let event = "UID:review-1@example.test\r\nDTSTART:20261015T090000Z\r\nDTEND:20261015T100000Z\r\nSUMMARY:Quarterly review\r\nORGANIZER;CN=Sam Reyes:mailto:sam@example.test\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:pat@example.test\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:june@example.test\r\nSEQUENCE:0";
    comes(&fake, "Sam Reyes <sam@example.test>", "Quarterly review", "REQUEST", event);
    run(&mut s, "account.receive", Value::Null);
    let items = run(&mut s, "calendar.items", json!({ "from": "2026-10-15", "to": "2026-10-16" }));
    let in_calendar = |v: &Value, subject: &str| v["items"].as_array().unwrap().iter().any(|i| i["untrusted_content"]["subject"] == subject);
    assert!(!in_calendar(&items, "Quarterly review"), "an invitation is not in the calendar until it is answered: {items}");

    run(&mut s, "folder.select", json!({ "folder": "Inbox" }));
    let at = s.rows.iter().position(|r| r.subject == "Quarterly review").unwrap();
    run(&mut s, "mail.select", json!({ "index": at }));
    assert_eq!(s.open.as_ref().unwrap().message_class(), "IPM.Schedule.Meeting.Request");
    let asked = run(&mut s, "calendar.invitation", Value::Null);
    assert_eq!(asked["method"], "REQUEST");
    assert_eq!(asked["response"], "notResponded");
    assert_eq!(asked["inCalendar"], Value::Null);
    // The dentist and the sample's daily stand-up both fall within it.
    assert_eq!(asked["conflicts"].as_array().unwrap().len(), 2, "{asked}");
    assert!(asked["conflicts"].as_array().unwrap().iter().any(|c| c["untrusted_content"]["subject"] == "Dentist"));

    let done = run(&mut s, "calendar.respond", json!({ "response": "tentative", "comment": "If the dentist is quick" }));
    let id = done["id"].as_u64().unwrap();
    assert_eq!(done["sent"], true);
    let (to, subject, cal) = went(&fake, 0);
    assert_eq!(to, ["sam@example.test"]);
    assert_eq!(subject, "Tentative: Quarterly review");
    assert!(cal.contains("METHOD:REPLY\r\n") && line_of(&cal, "ATTENDEE").contains("PARTSTAT=TENTATIVE"), "{cal}");
    assert!(line_of(&cal, "ATTENDEE").ends_with("mailto:pat@example.test"));
    assert!(cal.contains("COMMENT:If the dentist is quick"));
    assert!(!s.rows.iter().any(|r| r.subject == "Quarterly review"), "the invitation answered went to Deleted Items");
    let items = run(&mut s, "calendar.items", json!({ "from": "2026-10-15", "to": "2026-10-16" }));
    let review = items["items"].as_array().unwrap().iter().find(|i| i["id"] == id).unwrap().clone();
    assert_eq!((review["busy"].as_str(), review["meeting"].as_bool()), (Some("tentative"), Some(true)));
    let m = run(&mut s, "calendar.attendees", json!({ "id": id }));
    assert_eq!((m["response"].as_str(), m["organized"].as_bool()), (Some("tentative"), Some(false)));
    assert!(s.run("calendar.sendInvitations", &json!({ "id": id })).is_err(), "only its organizer sends it");
    assert!(s.run("calendar.cancelMeeting", &json!({ "id": id })).is_err());

    // Answered again from the calendar: accepted, busy now.
    run(&mut s, "calendar.respond", json!({ "id": id, "response": "accept", "send": false }));
    assert_eq!(fake.state().submitted.len(), 1, "not told when asked not to");
    assert_eq!(run(&mut s, "calendar.attendees", json!({ "id": id }))["response"], "accepted");

    // Called off by someone else: nothing changes. By its organizer: marked, and free.
    let off = "UID:review-1@example.test\r\nDTSTART:20261015T090000Z\r\nSTATUS:CANCELLED\r\nSEQUENCE:1";
    comes(&fake, "June <june@example.test>", "Canceled: Quarterly review", "CANCEL", off);
    run(&mut s, "account.receive", Value::Null);
    assert_eq!(run(&mut s, "calendar.attendees", json!({ "id": id }))["cancelled"], false);
    comes(&fake, "Sam Reyes <sam@example.test>", "Canceled: Quarterly review (2)", "CANCEL", off);
    let got = run(&mut s, "account.receive", Value::Null);
    assert_eq!(got["meetings"], json!(["Quarterly review was called off"]), "{got}");
    assert_eq!(run(&mut s, "calendar.attendees", json!({ "id": id }))["cancelled"], true);
    let items = run(&mut s, "calendar.items", json!({ "from": "2026-10-15", "to": "2026-10-16" }));
    let review = items["items"].as_array().unwrap().iter().find(|i| i["id"] == id).unwrap().clone();
    assert_eq!(review["busy"], "free");
    assert_eq!(review["untrusted_content"]["subject"], "Canceled: Quarterly review");
    assert!(s.run("calendar.respond", &json!({ "id": id, "response": "accept" })).is_err(), "a meeting called off is not answered");
}

#[test]
fn a_declined_invitation_stays_out_of_the_calendar_and_a_reply_with_meeting_asks_everyone() {
    let fake = Fake::default();
    let mut s = session(&fake);
    let event = "UID:lunch@example.test\r\nDTSTART:20261016T120000Z\r\nDTEND:20261016T130000Z\r\nSUMMARY:Lunch\r\nORGANIZER:mailto:sam@example.test\r\nATTENDEE:mailto:pat@example.test\r\nSEQUENCE:0";
    comes(&fake, "sam@example.test", "Lunch", "REQUEST", event);
    run(&mut s, "account.receive", Value::Null);
    run(&mut s, "folder.select", json!({ "folder": "Inbox" }));
    let at = s.rows.iter().position(|r| r.subject == "Lunch").unwrap();
    run(&mut s, "mail.select", json!({ "index": at }));
    let done = run(&mut s, "calendar.respond", json!({ "response": "decline" }));
    assert_eq!(done["id"], Value::Null);
    assert_eq!(went(&fake, 0).1, "Declined: Lunch");
    assert!(went(&fake, 0).2.contains("PARTSTAT=DECLINED"));
    let items = run(&mut s, "calendar.items", json!({ "from": "2026-10-16", "to": "2026-10-17" }));
    assert!(!items["items"].as_array().unwrap().iter().any(|i| i["untrusted_content"]["subject"] == "Lunch"));

    // Reply with Meeting: the sender and those it went to, oneself left out.
    let at = s.rows.iter().position(|r| r.subject != "Lunch").unwrap();
    run(&mut s, "mail.select", json!({ "index": at }));
    let subject = s.open.as_ref().unwrap().subject();
    let sender = s.open.as_ref().unwrap().sender_email();
    let made = run(&mut s, "mail.replyWithMeeting", json!({ "start": "2026-10-20 09:00" }));
    let m = run(&mut s, "calendar.attendees", json!({ "id": made["id"] }));
    let asked: Vec<&str> = m["attendees"].as_array().unwrap().iter().map(|p| p["untrusted_content"]["address"].as_str().unwrap()).collect();
    assert!(asked.contains(&sender.as_str()), "{asked:?}");
    assert!(!asked.iter().any(|a| a.eq_ignore_ascii_case("pat@example.test")));
    let items = run(&mut s, "calendar.items", json!({ "from": "2026-10-20", "to": "2026-10-21" }));
    assert!(items["items"].as_array().unwrap().iter().any(|i| i["untrusted_content"]["subject"] == subject.as_str()));
}

#[test]
fn hostile_meeting_params_are_errors_that_change_nothing() {
    let fake = Fake::default();
    let mut s = session(&fake);
    for p in [
        json!({ "subject": "x", "start": "2026-10-14 10:00" }),
        json!({ "subject": "x", "start": "2026-10-14 10:00", "attendees": [] }),
        json!({ "subject": "x", "start": "2026-10-14 10:00", "attendees": "no address here" }),
        json!({ "subject": "x", "start": "2026-10-14 10:00", "attendees": [{ "address": "a@example.test", "role": "boss" }] }),
        json!({ "subject": "x", "start": "2026-10-14 10:00", "attendees": [{ "name": "No address" }] }),
        json!({ "subject": "x", "start": "2026-10-14 10:00", "attendees": [7] }),
        json!({ "subject": "x", "start": "2026-10-14 10:00", "attendees": "pat@example.test" }),
    ] {
        assert!(s.run("calendar.newMeeting", &p).is_err(), "{p}");
    }
    let many: Vec<String> = (0..5_000).map(|i| format!("p{i}@example.test")).collect();
    let made = run(&mut s, "calendar.newMeeting", json!({ "subject": "Everyone", "start": "2026-10-14 10:00", "attendees": many.join("; ") }));
    let m = run(&mut s, "calendar.attendees", json!({ "id": made["id"] }));
    assert_eq!(m["attendees"].as_array().unwrap().len(), openmailclient_model::meeting::MAX_ATTENDEES);
    for p in
        [json!({}), json!({ "response": "maybe" }), json!({ "response": "accept", "id": 999_999 }), json!({ "response": "accept", "id": made["id"] })]
    {
        assert!(s.run("calendar.respond", &p).is_err(), "{p}");
    }
    assert!(s.run("calendar.sendInvitations", &json!({ "id": 999_999 })).is_err());
    assert!(fake.state().submitted.is_empty());
}

/// A meeting's id as Outlook writes it for a UID: its GlobalObjectId in hexadecimal.
const OUTLOOK_UID: &str = "040000008200E00074C5B7101A82E00800000000B0206F3BB95FDD010000000000000000100000004C54A336181ADC4B955995B304583205";

/// A request in the shape Outlook sends one: the calendar alone with no other way of reading it,
/// lines folded with a tab, the zone named as Windows names it and given by its standard time
/// alone, the UID a GlobalObjectId and the words in DESCRIPTION. Written by hand from that shape;
/// every name, address and word in it is ours.
fn outlook_request() -> String {
    let (uid_a, uid_b) = OUTLOOK_UID.split_at(73);
    [
        "From: \"Sam Reyes\" <sam@example.test>",
        "To: <pat@example.test>",
        "Subject: Planning",
        "Date: Mon, 19 Oct 2026 11:01:46 +0200",
        "Message-ID: <000401dd5fa8$7a65cfa0$6f316ee0$@example.test>",
        "MIME-Version: 1.0",
        "Content-Type: text/calendar; method=REQUEST;",
        "\tcharset=\"utf-8\"",
        "Content-Transfer-Encoding: 7bit",
        "",
        "BEGIN:VCALENDAR",
        "PRODID:-//Example//Example Calendar 1.0//EN",
        "VERSION:2.0",
        "METHOD:REQUEST",
        "BEGIN:VTIMEZONE",
        "TZID:South Africa Standard Time",
        "BEGIN:STANDARD",
        "DTSTART:16010101T000000",
        "TZOFFSETFROM:+0200",
        "TZOFFSETTO:+0200",
        "END:STANDARD",
        "END:VTIMEZONE",
        "BEGIN:VEVENT",
        "ATTENDEE;CN=pat@example.test;RSVP=TRUE:mailto:pat@exam",
        "\tple.test",
        "CLASS:PUBLIC",
        "CREATED:20261019T090925Z",
        "DESCRIPTION:Plans for the next quarter\\n\\n",
        "DTEND;TZID=\"South Africa Standard Time\":20261019T120000",
        "DTSTAMP:20261019T090146Z",
        "DTSTART;TZID=\"South Africa Standard Time\":20261019T113000",
        "LAST-MODIFIED:20261019T090925Z",
        "ORGANIZER;CN=\"Sam Reyes\":mailto:sam@example.test",
        "PRIORITY:5",
        "SEQUENCE:0",
        "SUMMARY;LANGUAGE=en-za:Planning",
        "TRANSP:OPAQUE",
        &format!("UID:{uid_a}"),
        &format!("\t{uid_b}"),
        "X-ALT-DESC;FMTTYPE=text/html:<html><body><p>Plans for the next quarter</p></b",
        "\tody></html>",
        "X-MICROSOFT-CDO-BUSYSTATUS:BUSY",
        "X-MICROSOFT-DISALLOW-COUNTER:FALSE",
        "END:VEVENT",
        "END:VCALENDAR",
        "",
    ]
    .join("\r\n")
}

#[test]
fn a_request_from_outlook_is_read_answered_and_kept_as_outlook_keeps_one() {
    use openmailclient_model::calendar::{self, Appointment};
    use openmailclient_model::meeting::{self, Meeting, Response};
    let fake = Fake::default();
    let mut s = session(&fake);
    fake.state().mailbox("INBOX").unwrap().add(outlook_request().as_bytes(), &[]);
    run(&mut s, "account.receive", Value::Null);
    run(&mut s, "folder.select", json!({ "folder": "Inbox" }));
    let at = s.rows.iter().position(|r| r.subject == "Planning").unwrap();
    run(&mut s, "mail.select", json!({ "index": at }));
    let open = s.open.clone().unwrap();
    assert_eq!(open.message_class(), meeting::class::REQUEST);
    // What it says is in its calendar alone, and is its body.
    assert_eq!(open.body_text().trim(), "Plans for the next quarter");
    // Kept as Outlook keeps a request: the meeting's id, when it is, received and not answered.
    assert_eq!(calendar::global_id(&open).unwrap(), calendar::global_id_of_uid(OUTLOOK_UID));
    let a = Appointment::of(&open).unwrap();
    assert_eq!(a.start, openmailclient_model::time::from_civil(2026, 10, 19, 9, 30, 0, 0), "11:30 South Africa time");
    assert_eq!(a.end, openmailclient_model::time::from_civil(2026, 10, 19, 10, 0, 0, 0));
    let m = Meeting::of(&open).unwrap();
    assert!(m.received && m.response == Response::NotResponded, "{m:?}");
    let asked = run(&mut s, "calendar.invitation", Value::Null);
    assert_eq!(asked["method"], "REQUEST");
    assert_eq!(asked["meeting"]["organizer"]["untrusted_content"]["address"], "sam@example.test", "{asked}");

    // As Outlook keeps it in a data file or a .msg: no calendar, its properties alone.
    let mut stored = open.clone();
    stored.attachments.clear();
    s.open = Some(stored);
    let again = run(&mut s, "calendar.invitation", Value::Null);
    for key in ["method", "start", "end", "response"] {
        assert_eq!(again[key], asked[key], "{key}: {again}");
    }
    assert_eq!(again["meeting"]["organizer"]["untrusted_content"], asked["meeting"]["organizer"]["untrusted_content"]);
    assert_eq!(again["untrusted_content"]["description"], "Plans for the next quarter");

    // Answered: the reply names the meeting by Outlook's own id, so Outlook knows it again.
    run(&mut s, "mail.select", json!({ "index": at }));
    run(&mut s, "calendar.respond", json!({ "response": "accept" }));
    let (to, subject, cal) = went(&fake, 0);
    assert_eq!(to, ["sam@example.test"]);
    assert_eq!(subject, "Accepted: Planning");
    assert!(cal.contains("METHOD:REPLY\r\n") && cal.contains("PARTSTAT=ACCEPTED"), "{cal}");
    assert_eq!(line_of(&cal, "UID:"), format!("UID:{OUTLOOK_UID}"));
}

#[test]
fn answers_and_cancellations_kept_by_outlook_are_read_from_their_properties() {
    use openmailclient_model::calendar;
    use openmailclient_model::meeting::{self, Response};
    let fake = Fake::default();
    let mut s = session(&fake);
    let id = calendar::global_id_of_uid(OUTLOOK_UID);
    for (class, method) in [
        (meeting::class::ACCEPTED, "REPLY"),
        (meeting::class::TENTATIVE, "REPLY"),
        (meeting::class::DECLINED, "REPLY"),
        (meeting::class::CANCELED, "CANCEL"),
    ] {
        let mut msg = openmailclient_model::Item::new_mail();
        msg.props.set_str(openmailclient_model::pid::MESSAGE_CLASS, class);
        msg.set_sender("June Okafor", "june@example.test");
        msg.set_body_text("See you there");
        meeting::dress(&mut msg, &id, 2, true, Response::None);
        s.open = Some(msg);
        let got = run(&mut s, "calendar.invitation", Value::Null);
        assert_eq!(got["method"], method, "{class}: {got}");
        assert_eq!(got["untrusted_content"]["uid"], OUTLOOK_UID, "{class}");
        if method == "REPLY" {
            assert_eq!(got["attendee"]["untrusted_content"]["address"], "june@example.test");
            assert_eq!(got["untrusted_content"]["comment"], "See you there");
        }
    }
    // One with no meeting's id in it is no meeting that can be read.
    let mut bare = openmailclient_model::Item::new_mail();
    bare.props.set_str(openmailclient_model::pid::MESSAGE_CLASS, meeting::class::REQUEST);
    s.open = Some(bare);
    assert!(s.run("calendar.invitation", &Value::Null).is_err());
}

#[test]
fn an_invitation_we_send_is_kept_with_the_properties_outlook_reads() {
    use openmailclient_model::calendar;
    use openmailclient_model::meeting::Meeting;
    let fake = Fake::default();
    let mut s = session(&fake);
    let made = run(
        &mut s,
        "calendar.newMeeting",
        json!({ "subject": "Review", "start": "2026-10-20 10:00", "end": "2026-10-20 11:00", "attendees": ["june@example.test"] }),
    );
    run(&mut s, "calendar.sendInvitations", json!({ "id": made["id"] }));
    let (_, _, cal) = went(&fake, 0);
    let uid = line_of(&cal, "UID:").trim_start_matches("UID:").to_string();
    // The message kept in Sent Items carries the meeting's id and is marked a meeting, not received.
    run(&mut s, "folder.select", json!({ "folder": "Sent Items" }));
    let at = s.rows.iter().position(|r| r.subject == "Review").unwrap();
    run(&mut s, "mail.select", json!({ "index": at }));
    let sent = s.open.clone().unwrap();
    assert_eq!(calendar::global_id(&sent).unwrap(), calendar::global_id_of_uid(&uid));
    let m = Meeting::of(&sent).unwrap();
    assert!(!m.received, "{m:?}");
}

#[test]
fn an_account_s_data_file_made_without_a_calendar_is_given_one_and_takes_the_meeting() {
    use openmailclient_model::{FolderKind, MemStore};
    let dir = std::env::temp_dir().join(format!("omc-no-calendar-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fake = Fake::default();
    let mut s = Session::new();
    s.now = openmailclient_model::time::from_civil(2026, 10, 7, 14, 30, 0, 0);
    s.connector = Some(Arc::new(fake.clone()));
    run(
        &mut s,
        "account.add",
        json!({
            "email": "pat@example.test", "displayName": "Pat Lee", "kind": "imap",
            "incoming": { "host": "mail.example.test", "security": "starttls" },
            "outgoing": { "host": "mail.example.test", "security": "starttls" },
            "password": "not a real password",
        }),
    );
    // Its data file as accounts' files were made before they had a calendar: mail folders only.
    let path = dir.join("Accounts").join(crate::accounts::data_file_name(&s.accounts[0]));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    crate::io::save_store(&mut MemStore::with_default_folders("Pat Lee"), &path, crate::io::new_store_id(s.now)).unwrap();
    s.account_dir = Some(dir.join("Accounts"));
    s.journal_dir = Some(dir.join("journals"));
    assert!(s.stores.iter().all(|st| st.path().is_none_or(|p| p != path)));

    // The meeting comes into it, and it is given what it lacks.
    fake.state().mailbox("INBOX").unwrap().add(outlook_request().as_bytes(), &[]);
    run(&mut s, "account.receive", Value::Null);
    let at = s.stores.iter().position(|st| st.path() == Some(path.as_path())).unwrap();
    for kind in [FolderKind::Calendar, FolderKind::Contacts, FolderKind::Tasks] {
        assert!(s.find_folder(at, kind).is_some(), "{kind:?}");
    }
    let count = |s: &mut Session| s.stores[at].folders().unwrap().len();
    let folders = count(&mut s);
    // Opened again, nothing more is made.
    run(&mut s, "account.receive", Value::Null);
    assert_eq!(count(&mut s), folders);

    // Accepted into the calendar that is now there.
    run(&mut s, "folder.select", json!({ "folder": "Inbox", "store": at }));
    let row = s.rows.iter().position(|r| r.subject == "Planning").unwrap();
    run(&mut s, "mail.select", json!({ "index": row }));
    run(&mut s, "calendar.respond", json!({ "response": "accept" }));
    assert_eq!(went(&fake, 0).1, "Accepted: Planning");
    let cal = s.find_folder(at, FolderKind::Calendar).unwrap();
    assert_eq!(s.stores[at].list(cal).unwrap().len(), 1);

    // Saved, the folders are there when the file is opened again.
    run(&mut s, "file.save", Value::Null);
    drop(s);
    let mut again = crate::io::open_path(&path)
        .ok()
        .and_then(|o| match o {
            crate::io::Opened::Store(st) => Some(st),
            crate::io::Opened::Item(_) => None,
        })
        .unwrap();
    let kinds: Vec<_> = again.folders().unwrap().into_iter().map(|f| f.kind).collect();
    for kind in [FolderKind::Calendar, FolderKind::Contacts, FolderKind::Tasks] {
        assert_eq!(kinds.iter().filter(|k| **k == kind).count(), 1, "{kind:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
