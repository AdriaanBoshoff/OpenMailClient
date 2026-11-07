//! Files held in memory, as in a browser (M6.6): data files and messages opened from bytes,
//! a data file and a message given back as bytes, settings kept as text.

use super::*;

fn sample() -> Session {
    let mut s = Session::new();
    s.run("file.openSample", &Value::Null).unwrap();
    s
}

fn counts(s: &mut Session, store: usize) -> (usize, u64) {
    let folders = s.run("mail.folders", &json!({})).unwrap();
    let mine: Vec<&Value> = folders["folders"].as_array().unwrap().iter().filter(|f| f["store"] == store).collect();
    (mine.len(), mine.iter().map(|f| f["items"].as_u64().unwrap()).sum())
}

#[test]
fn a_data_file_goes_out_as_bytes_and_comes_back_whole_with_its_changes() {
    let mut s = sample();
    // A change not saved goes with it.
    s.run("mail.select", &json!({ "folder": "Inbox", "index": 0 })).unwrap();
    s.run("mail.delete", &Value::Null).unwrap();
    let before = counts(&mut s, 0);
    let (name, data) = s.store_bytes().unwrap();
    assert!(name.ends_with(".pst"), "{name}");
    assert!(data.starts_with(b"!BDN"));
    let mut back = Session::new();
    let opened = back.open_bytes("Mailbox from elsewhere.pst", data).unwrap();
    let idx = opened["store"].as_u64().unwrap() as usize;
    assert_eq!(counts(&mut back, idx), before);
    // It takes changes in memory, and goes out again.
    back.run("mail.select", &json!({ "folder": "Inbox", "index": 0 })).unwrap();
    back.run("mail.markRead", &Value::Null).unwrap();
    assert!(back.store_bytes().is_ok());
}

#[test]
fn a_message_goes_out_as_bytes_and_comes_back_in_opened_files() {
    let mut s = sample();
    assert!(s.item_bytes("msg").is_err(), "nothing is open");
    s.run("mail.select", &json!({ "folder": "Inbox", "index": 0 })).unwrap();
    let subject = s.open.as_ref().unwrap().subject();
    for format in ["msg", "eml", "oft"] {
        let (name, data) = s.item_bytes(format).unwrap();
        assert!(name.ends_with(&format!(".{format}")), "{name}");
        assert!(!name.contains('/') && !name.contains(char::from(92)), "{name}");
        let mut back = Session::new();
        back.open_bytes(&name, data).unwrap();
        assert_eq!(back.open.as_ref().unwrap().subject(), subject, "{format}");
    }
    assert!(s.item_bytes("exe").is_err());
}

#[test]
fn bytes_that_are_not_a_file_of_ours_are_an_error_not_a_crash() {
    let mut s = Session::new();
    for (name, data) in [
        ("x.pst", b"!BDN".to_vec()),
        ("x.pst", vec![0u8; 4096]),
        ("x.msg", vec![0xD0, 0xCF, 0x11, 0xE0, 0, 0, 0, 0]),
        ("no ending", b"!BDN\x00\x01".to_vec()),
        ("", Vec::new()),
    ] {
        assert!(s.open_bytes(name, data).is_err(), "{name}");
    }
    // A name with no ending and the bytes of a message is read as one.
    assert!(s.open_bytes("noted", b"Subject: hello\r\n\r\nbody\r\n".to_vec()).is_ok());
    // Nothing shown: nothing to give back.
    assert!(Session::new().store_bytes().is_err());
}

#[test]
fn settings_kept_as_text_come_back() {
    let mut s = sample();
    s.run("view.readingPane", &json!({ "position": "bottom" })).ok();
    let text = serde_json::to_string(&s.settings(Some(&json!({ "theme": "dark" })))).unwrap();
    let mut back = Session::new();
    let found = back.run("settings.load", &json!({ "text": text })).unwrap();
    assert_eq!(found["found"], true);
    assert_eq!(found["ui"]["theme"], "dark");
    assert_eq!(back.reading_pane, s.reading_pane);
    // None kept, too long, not JSON, or not text.
    assert_eq!(back.run("settings.load", &json!({ "text": null })).unwrap()["found"], false);
    assert_eq!(back.run("settings.load", &json!({ "text": "x".repeat(2 << 20) })).unwrap()["applied"], 0);
    assert_eq!(back.run("settings.load", &json!({ "text": "{not json" })).unwrap()["applied"], 0);
    assert!(back.run("settings.load", &json!({ "text": 5 })).is_err());
}
