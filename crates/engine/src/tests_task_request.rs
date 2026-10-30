//! Assign Task: a task given by mail, accepted or declined, and kept up to date from the one it
//! was given to, between two sessions through the in-process fake server. No socket is opened
//! and nothing really goes; every address is synthetic.

use crate::Session;
use openmailclient_net::fake::Fake;
use serde_json::{Value, json};
use std::sync::Arc;

fn session(fake: &Fake, email: &str, name: &str) -> Session {
    let mut s = Session::new();
    s.now = openmailclient_model::time::from_civil(2026, 10, 7, 14, 30, 0, 0);
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.connector = Some(Arc::new(fake.clone()));
    // Each side's server knows its own user.
    fake.state().user = email.to_string();
    s.run(
        "account.add",
        &json!({
            "email": email, "displayName": name, "kind": "imap",
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

/// The `n`th message that went from `from`, as the other side's server gets it.
fn went(from: &Fake, n: usize) -> Vec<u8> {
    // The server's state is taken once: asked again while held, it would wait for itself.
    let state = from.state();
    let count = state.submitted.len();
    state.submitted.get(n).unwrap_or_else(|| panic!("only {count} went")).data.clone()
}

fn deliver(to: &Fake, data: &[u8]) {
    to.state().mailbox("INBOX").unwrap().add(data, &[]);
}

fn open(s: &mut Session, subject: &str) {
    run(s, "folder.select", json!({ "folder": "Inbox" }));
    let at = s
        .rows
        .iter()
        .position(|r| r.subject == subject)
        .unwrap_or_else(|| panic!("no {subject} in the Inbox: {:?} {}", s.rows.iter().map(|r| r.subject.clone()).collect::<Vec<_>>(), s.status));
    run(s, "mail.select", json!({ "index": at }));
}

fn task_row(s: &mut Session, subject: &str) -> Value {
    let items = run(s, "tasks.items", json!({ "view": "simpleList" }));
    items["items"].as_array().unwrap().iter().find(|i| i["untrusted_content"]["subject"] == subject).cloned().unwrap_or(Value::Null)
}

#[test]
fn a_task_is_given_accepted_and_kept_up_to_date_by_the_one_it_was_given_to() {
    let (pat_server, june_server) = (Fake::default(), Fake::default());
    let mut pat = session(&pat_server, "pat@example.test", "Pat Lee");
    let mut june = session(&june_server, "june@example.test", "June Okafor");

    let made = run(&mut pat, "tasks.new", json!({ "subject": "Quarterly figures", "due": "2026-10-20", "notes": "The usual tables." }));
    let id = made["id"].as_u64().unwrap();
    assert!(pat.run("tasks.assign", &json!({ "id": id, "to": "pat@example.test" })).is_err(), "not to oneself");
    let sent = run(&mut pat, "tasks.assign", json!({ "id": id, "to": "June Okafor <june@example.test>", "comment": "By Tuesday, please." }));
    assert_eq!((sent["sent"].as_u64(), sent["shared"].as_bool()), (Some(1), Some(false)), "{}", pat.status);
    let mine = task_row(&mut pat, "Quarterly figures");
    assert_eq!(mine["assigned"], json!({ "copy": "assigner", "answer": "waiting" }));
    assert_eq!(mine["untrusted_content"]["owner"], "June Okafor");
    assert!(pat.run("tasks.assign", &json!({ "id": id, "to": "sam@example.test" })).is_err(), "given once");

    // As it goes: a task request, with a winmail.dat Outlook reads and a body anyone can.
    let request = went(&pat_server, 0);
    let text = String::from_utf8_lossy(&request);
    assert!(text.contains("application/ms-tnef") && text.contains("winmail.dat"), "{text}");
    let got = openmailclient_mime::parse(&request).unwrap();
    assert_eq!(got.message_class(), "IPM.TaskRequest");
    assert_eq!(got.subject(), "Task Request: Quarterly figures");
    assert!(got.body_text().contains("By Tuesday, please.") && got.body_text().contains("Due date: 2026-10-20"), "{}", got.body_text());

    // June has it, and accepts it.
    deliver(&june_server, &request);
    run(&mut june, "account.receive", Value::Null);
    open(&mut june, "Task Request: Quarterly figures");
    let asked = run(&mut june, "tasks.request", Value::Null);
    assert_eq!((asked["kind"].as_str(), asked["from"].as_str()), (Some("request"), Some("pat@example.test")));
    assert_eq!(asked["due"], "2026-10-20");
    assert_eq!(asked["untrusted_content"]["notes"], "The usual tables.");
    assert_eq!(asked["assigners"][0]["address"], "pat@example.test");
    let done = run(&mut june, "tasks.respond", json!({ "response": "accept" }));
    assert_eq!(done["sent"], true);
    let theirs = task_row(&mut june, "Quarterly figures");
    assert_eq!(theirs["assigned"], json!({ "copy": "assignee", "answer": "accepted" }));
    assert_eq!(theirs["due"], "2026-10-20");
    assert!(!june.rows.iter().any(|r| r.subject == "Task Request: Quarterly figures"), "the request answered went to Deleted Items");

    let accepted = went(&june_server, 0);
    let back = openmailclient_mime::parse(&accepted).unwrap();
    assert_eq!(back.message_class(), "IPM.TaskRequest.Accept");
    assert_eq!(back.subject(), "Task Accepted: Quarterly figures");
    deliver(&pat_server, &accepted);
    let got = run(&mut pat, "account.receive", Value::Null);
    assert_eq!(got["meetings"], json!(["June Okafor accepted Quarterly figures"]), "{got}");
    assert_eq!(task_row(&mut pat, "Quarterly figures")["assigned"]["answer"], "accepted");

    // June gets half way; an update goes to Pat by itself. One who is not June says it is
    // done, and is not believed.
    let jid = theirs["id"].as_u64().unwrap();
    run(&mut june, "tasks.change", json!({ "id": jid, "percent": 50 }));
    let update = went(&june_server, 1);
    assert_eq!(openmailclient_mime::parse(&update).unwrap().message_class(), "IPM.TaskRequest.Update");
    let forged = String::from_utf8_lossy(&update).replacen("From: \"June Okafor\" <june@example.test>", "From: Mallory <mallory@example.test>", 1);
    assert_ne!(forged.as_bytes(), update.as_slice(), "the forgery names someone else");
    deliver(&pat_server, forged.as_bytes());
    let got = run(&mut pat, "account.receive", Value::Null);
    assert_eq!(got["meetings"], json!([]), "{got}");
    assert_eq!(task_row(&mut pat, "Quarterly figures")["percent"], 0);
    deliver(&pat_server, &update);
    run(&mut pat, "account.receive", Value::Null);
    let mine = task_row(&mut pat, "Quarterly figures");
    assert_eq!((mine["percent"].as_u64(), mine["status"].as_str()), (Some(50), Some("inProgress")), "{mine}");

    // Done: the report on completing goes too, and Pat's copy says so.
    run(&mut june, "tasks.markComplete", json!({ "id": jid }));
    let report = went(&june_server, 2);
    assert_eq!(openmailclient_mime::parse(&report).unwrap().subject(), "Task Completed: Quarterly figures");
    deliver(&pat_server, &report);
    run(&mut pat, "account.receive", Value::Null);
    assert_eq!(task_row(&mut pat, "Quarterly figures")["status"], "complete");
    // An update older than what Pat has is not taken.
    deliver(&pat_server, &update);
    run(&mut pat, "account.receive", Value::Null);
    assert_eq!(task_row(&mut pat, "Quarterly figures")["status"], "complete");

    // A status report, by hand.
    run(&mut pat, "tasks.sendStatusReport", json!({ "id": id }));
    let status = openmailclient_mime::parse(&went(&pat_server, 1)).unwrap();
    assert_eq!(status.subject(), "Task Status Report: Quarterly figures");
}

#[test]
fn a_declined_task_is_the_giver_s_own_again_and_a_shared_one_is_not_answered() {
    let (pat_server, june_server) = (Fake::default(), Fake::default());
    let mut pat = session(&pat_server, "pat@example.test", "Pat Lee");
    let mut june = session(&june_server, "june@example.test", "June Okafor");
    let id = run(&mut pat, "tasks.new", json!({ "subject": "Order paper" }))["id"].as_u64().unwrap();
    run(&mut pat, "tasks.assign", json!({ "id": id, "to": "june@example.test", "keepCopy": true }));
    deliver(&june_server, &went(&pat_server, 0));
    run(&mut june, "account.receive", Value::Null);
    open(&mut june, "Task Request: Order paper");
    let done = run(&mut june, "tasks.respond", json!({ "response": "decline", "comment": "Not this week." }));
    assert_eq!((done["id"].as_u64(), done["sent"].as_bool()), (None, Some(true)));
    assert_eq!(task_row(&mut june, "Order paper"), Value::Null, "a declined task is not kept");
    let declined = went(&june_server, 0);
    let back = openmailclient_mime::parse(&declined).unwrap();
    assert_eq!(back.message_class(), "IPM.TaskRequest.Decline");
    assert!(back.body_text().contains("Not this week."));
    deliver(&pat_server, &declined);
    run(&mut pat, "account.receive", Value::Null);
    let mine = task_row(&mut pat, "Order paper");
    assert_eq!(mine["assigned"]["answer"], "declined");
    // Declined, it may be given again.
    run(&mut pat, "tasks.assign", json!({ "id": id, "to": "June <june@example.test>; Sam <sam@example.test>" }));

    // Given to two, it is theirs to share, and neither answers.
    deliver(&june_server, &went(&pat_server, 1));
    run(&mut june, "account.receive", Value::Null);
    open(&mut june, "Task Request: Order paper");
    let done = run(&mut june, "tasks.respond", json!({ "response": "accept" }));
    assert_eq!(done["sent"], false);
    assert_eq!(june_server.state().submitted.len(), 1);

    // Without a copy kept, the task goes once it is sent.
    let other = run(&mut pat, "tasks.new", json!({ "subject": "Book the room" }))["id"].as_u64().unwrap();
    run(&mut pat, "tasks.assign", json!({ "id": other, "to": "june@example.test", "keepCopy": false }));
    assert_eq!(task_row(&mut pat, "Book the room"), Value::Null);
}

#[test]
fn hostile_task_request_params_are_errors_that_change_nothing() {
    let fake = Fake::default();
    let mut s = session(&fake, "pat@example.test", "Pat Lee");
    for (id, p) in [
        ("tasks.assign", json!({})),
        ("tasks.assign", json!({ "id": 999_999, "to": "x@example.test" })),
        ("tasks.assign", json!({ "to": ["no address", 4, null] })),
        ("tasks.respond", json!({ "response": "accept" })),
        ("tasks.respond", json!({ "response": 7 })),
        ("tasks.request", json!({})),
        ("tasks.sendStatusReport", json!({ "id": "seven" })),
    ] {
        assert!(s.run(id, &p).is_err(), "{id} {p}");
    }
    assert!(fake.state().submitted.is_empty());
}
