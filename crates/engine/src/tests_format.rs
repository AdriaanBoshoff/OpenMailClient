//! Format Text: a message written with its look and pictures, sent as HTML with each picture a
//! part of its own it refers to, kept as a draft and written on again, and a signature with a
//! picture put in and taken out again.

use crate::Session;
use base64::Engine as _;
use openmailclient_model::format::{List, PICTURE};
use openmailclient_model::pid;
use serde_json::{Value, json};

/// The start of a PNG file: enough for it to be known as one.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01";

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.run(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// Open the message of `folder` whose subject is `subject`.
fn open(s: &mut Session, folder: &str, subject: &str) {
    run(s, "folder.select", json!({ "folder": folder }));
    for index in 0..s.rows.len() {
        run(s, "mail.select", json!({ "index": index }));
        if s.open.as_ref().is_some_and(|i| i.subject() == subject) {
            return;
        }
    }
    panic!("no {subject} in {folder}");
}

#[test]
fn a_formatted_message_goes_as_html_with_its_pictures() {
    let mut s = Session::with_sample();
    run(&mut s, "mail.new", Value::Null);
    run(&mut s, "compose.setField", json!({ "field": "to", "value": "june.okafor@example.org" }));
    run(&mut s, "compose.setField", json!({ "field": "subject", "value": "Look" }));
    run(&mut s, "compose.setField", json!({ "field": "body", "value": "Hello there\nfirst\nsecond", "at": 0 }));
    // "Hello" bold, and the two lines after it a list.
    let look = run(&mut s, "compose.select", json!({ "from": 0, "to": 5 }));
    assert_eq!(look["bold"], false);
    let look = run(&mut s, "compose.format", json!({ "bold": "toggle", "color": "#1F3864" }));
    assert_eq!((look["bold"].as_bool(), look["color"].as_str()), (Some(true), Some("#1f3864")));
    run(&mut s, "compose.select", json!({ "from": 13, "to": 20 }));
    run(&mut s, "compose.paragraph", json!({ "list": "bullet" }));
    // Typed after "Hello", the bold goes on; past the blank it does not.
    run(&mut s, "compose.select", json!({ "from": 5, "to": 5 }));
    run(&mut s, "compose.setField", json!({ "field": "body", "value": "Hello!! there\nfirst\nsecond", "at": 5 }));
    let d = s.compose.as_ref().unwrap();
    assert!(d.format.mark_at(6).bold && !d.format.mark_at(8).bold);
    assert_eq!(d.format.paras.iter().map(|p| p.list).collect::<Vec<_>>(), [List::None, List::Bullet, List::Bullet]);
    // A picture at the end, and a link to a mailbox.
    run(&mut s, "compose.select", json!({ "from": 26, "to": 26 }));
    let put = run(&mut s, "compose.insertPicture", json!({ "data": b64(PNG), "name": "dot.png", "width": 20, "height": 20 }));
    assert_eq!(put["id"], 1);
    run(&mut s, "compose.link", json!({ "address": "june.okafor@example.org", "text": " mail me" }));
    assert!(s.run("compose.link", &json!({ "address": "javascript:alert(1)" })).is_err());
    assert_eq!(s.compose.as_ref().unwrap().body, format!("Hello!! there\nfirst\nsecond{PICTURE} mail me"));

    run(&mut s, "compose.saveDraft", Value::Null);
    run(&mut s, "compose.send", json!({ "anyway": true }));
    open(&mut s, "Outbox", "Look");
    let it = s.open.clone().unwrap();
    let html = it.body_html().unwrap();
    assert!(html.contains("<b>") || html.contains("font-weight:bold"), "{html}");
    assert!(html.contains("<ul") && html.contains("mailto:june.okafor@example.org"), "{html}");
    // The picture is a part the body names by content id, and not one of the attachments shown.
    let part = it.attachments.iter().find(|a| a.content_id().is_some()).expect("the picture's part");
    let cid = part.content_id().unwrap();
    assert!(html.contains(&format!("cid:{cid}")), "{html}");
    assert_eq!((part.data.as_slice(), part.name().as_str(), part.mime()), (PNG, "dot.png", "image/png"));
    assert_eq!(part.props.bool(pid::ATTACHMENT_HIDDEN), Some(true));
    // And as text beside it, the list marked as text writes it.
    assert_eq!(it.body_text(), "Hello!! there\n• first\n• second mail me");
}

#[test]
fn a_formatted_draft_is_written_on_again_as_it_was() {
    let mut s = Session::with_sample();
    run(&mut s, "mail.new", Value::Null);
    run(&mut s, "compose.setField", json!({ "field": "subject", "value": "Again" }));
    run(&mut s, "compose.setField", json!({ "field": "body", "value": "Big words", "at": 0 }));
    run(&mut s, "compose.select", json!({ "from": 0, "to": 3 }));
    run(&mut s, "compose.format", json!({ "size": 20, "italic": true }));
    run(&mut s, "compose.select", json!({ "from": 9, "to": 9 }));
    run(&mut s, "compose.paste", json!({ "image": b64(PNG) }));
    run(&mut s, "compose.saveDraft", Value::Null);
    run(&mut s, "compose.discard", Value::Null);
    open(&mut s, "Drafts", "Again");
    run(&mut s, "compose.editDraft", Value::Null);
    let d = s.compose.as_ref().unwrap();
    assert_eq!(d.body, format!("Big words{PICTURE}"));
    let big = d.format.mark_at(0);
    assert!(big.italic && big.size == Some(20.0), "{big:?}");
    assert!(!d.format.mark_at(5).italic);
    assert_eq!(d.format.pictures.first().map(|p| p.bytes.as_ref()), Some(PNG));
    // The picture is in the body again, not an attachment of its own as well.
    assert!(d.attachments.is_empty(), "{:?}", d.attachments.iter().map(|a| a.name()).collect::<Vec<_>>());
}

#[test]
fn pasted_html_keeps_its_look_and_fetches_nothing() {
    let mut s = Session::with_sample();
    run(&mut s, "mail.new", Value::Null);
    let html = format!(
        r#"<p><b>Pat</b> <a href="https://example.org">site</a> <a href="javascript:x()">bad</a><img src="https://tracker.example/p.gif" alt=""><img src="data:image/png;base64,{}"></p>"#,
        b64(PNG)
    );
    run(&mut s, "compose.paste", json!({ "html": html }));
    let d = s.compose.as_ref().unwrap();
    assert_eq!(d.body, format!("Pat site bad{PICTURE}"));
    assert!(d.format.mark_at(0).bold);
    assert_eq!(d.format.mark_at(4).link.as_deref(), Some("https://example.org"));
    assert_eq!(d.format.mark_at(9).link, None);
    assert_eq!(d.format.pictures.len(), 1);
    // Plain text takes all of it off, and pastes only text after.
    run(&mut s, "compose.textFormat", json!({ "format": "plain" }));
    let d = s.compose.as_ref().unwrap();
    assert_eq!((d.body.as_str(), d.format.is_plain(), d.format.pictures.len()), ("Pat site bad", true, 0));
    run(&mut s, "compose.paste", json!({ "html": "<p><i>more</i></p>" }));
    assert!(s.compose.as_ref().unwrap().format.is_plain());
    assert!(s.run("compose.insertPicture", &json!({ "data": b64(PNG) })).is_err());
}

#[test]
fn a_signature_with_a_picture_is_put_in_and_taken_out() {
    let mut s = Session::with_sample();
    let html = format!(
        r#"<p style="margin:0"><b>Pat Example</b></p><p style="margin:0"><img src="data:image/png;base64,{}" width="40" height="10" alt="logo"></p>"#,
        b64(PNG)
    );
    let saved = run(&mut s, "signatures.save", json!({ "name": "Logo", "text": "Pat Example\n", "html": html }));
    assert_eq!(saved["formatted"], true);
    run(&mut s, "signatures.save", json!({ "name": "Plain", "text": "— Pat" }));
    run(&mut s, "mail.new", Value::Null);
    run(&mut s, "compose.setField", json!({ "field": "body", "value": "Hello.", "at": 0 }));
    run(&mut s, "compose.signature", json!({ "name": "Logo" }));
    let d = s.compose.as_ref().unwrap();
    assert_eq!(d.body, format!("Hello.\n\nPat Example\n{PICTURE}"));
    assert!(d.format.mark_at(8).bold && !d.format.mark_at(0).bold);
    assert_eq!(d.format.pictures.first().map(|p| (p.width, p.height)), Some((40, 10)));
    // Another in its place: the picture goes with the one it came with.
    run(&mut s, "compose.signature", json!({ "name": "Plain" }));
    let d = s.compose.as_ref().unwrap();
    assert_eq!(d.body, "Hello.\n\n— Pat");
    let mut f = d.format.clone();
    f.fit(&d.body);
    assert!(f.pictures.is_empty() && f.is_plain(), "{f:?}");
}

#[test]
fn format_commands_refuse_junk() {
    let mut s = Session::with_sample();
    assert!(s.run("compose.format", &json!({ "bold": true })).is_err(), "no message is being written");
    run(&mut s, "mail.new", Value::Null);
    for (id, p) in [
        ("compose.format", json!({ "bold": 3 })),
        ("compose.format", json!({ "color": "#12" })),
        ("compose.format", json!({ "size": -4 })),
        ("compose.format", json!({ "size": 1e9 })),
        ("compose.paragraph", json!({ "align": "up" })),
        ("compose.insertPicture", json!({ "data": b64(b"not a picture") })),
        ("compose.insertPicture", json!({ "data": "!!!" })),
        ("compose.paste", json!({})),
        ("compose.textFormat", json!({ "format": "doc" })),
    ] {
        assert!(s.run(id, &p).is_err(), "{id} {p}");
    }
    // A selection past the end is held to it.
    let look = run(&mut s, "compose.select", json!({ "from": 999, "to": 5 }));
    assert_eq!(look["bold"], false);
    assert_eq!(s.compose.as_ref().unwrap().selected(), (0, 0));
}
