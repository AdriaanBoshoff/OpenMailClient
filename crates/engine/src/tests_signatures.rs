//! Signatures: chosen for each account, put in new messages, replies and forwards, changed with
//! the account a message goes from and by hand, kept between runs and brought from Outlook.

use crate::Session;
use crate::accounts::Account;
use crate::outlook::{Raw, RawProfile, Section};
use serde_json::{Value, json};
use std::sync::Arc;

fn account(s: &mut Session, email: &str, display: &str) -> String {
    let mut a = Account::from_json(
        &json!({ "email": email, "displayName": display, "kind": "imap", "incoming": { "host": "mail.example.test", "port": 993, "security": "tls" } }),
        "x",
        None,
    )
    .unwrap();
    a.id = crate::accounts::new_id(&s.accounts);
    let id = a.id.clone();
    s.accounts.push(a);
    id
}

fn body(s: &Session) -> String {
    s.compose.as_ref().unwrap().body.clone()
}

#[test]
fn signatures_are_put_in_as_each_account_chose_them() {
    let mut s = Session::with_sample();
    let work = account(&mut s, "pat@example.test", "Pat Example");
    let home = account(&mut s, "pat.home@example.test", "");
    s.run("signatures.save", &json!({ "name": "Work", "text": "Regards,\r\n{name}\r\n{email}\r\n\r\n" })).unwrap();
    s.run("signatures.save", &json!({ "name": "Short", "text": "— Pat" })).unwrap();
    s.run("signatures.choose", &json!({ "new": "Work", "replies": "Short" })).unwrap();
    s.run("signatures.choose", &json!({ "account": home, "new": null, "replies": null })).unwrap();

    // A new message from the first account: its own name and address filled in, at the foot.
    s.run("mail.new", &Value::Null).unwrap();
    assert_eq!(body(&s), "\n\nRegards,\nPat Example\npat@example.test");
    // Sent from the other, which chose none: it goes.
    s.run("compose.setField", &json!({ "field": "from", "value": home })).unwrap();
    assert_eq!(body(&s), "");
    // And back: the first account's again.
    s.run("compose.setField", &json!({ "field": "body", "value": "Hello." })).unwrap();
    s.run("compose.setField", &json!({ "field": "from", "value": work })).unwrap();
    assert_eq!(body(&s), "Hello.\n\nRegards,\nPat Example\npat@example.test");
    // Chosen by hand, it takes the place of the one there, and stays when the account changes.
    s.run("compose.signature", &json!({ "name": "short" })).unwrap();
    assert_eq!(body(&s), "Hello.\n\n— Pat");
    assert_eq!(s.compose.as_ref().unwrap().signature_name.as_deref(), Some("Short"), "as the menu marks it");
    s.run("compose.setField", &json!({ "field": "from", "value": home })).unwrap();
    assert_eq!(body(&s), "Hello.\n\n— Pat");
    s.run("compose.signature", &json!({ "name": null })).unwrap();
    assert_eq!(body(&s), "Hello.");
    assert_eq!(s.compose.as_ref().unwrap().signature_name, None);
    assert!(s.run("compose.signature", &json!({ "name": "Nothing" })).is_err());
    assert!(s.run("compose.signature", &json!({ "name": 7 })).is_err());

    // A reply has the reply signature above the quoted text.
    s.run("compose.discard", &Value::Null).unwrap();
    s.run("mail.select", &json!({ "folder": "Inbox", "index": 0 })).unwrap();
    s.run("mail.reply", &Value::Null).unwrap();
    let reply = body(&s);
    assert!(reply.starts_with("\n\n— Pat\n\n-----Original Message-----"), "{reply:?}");
    s.run("compose.signature", &json!({ "name": "Work" })).unwrap();
    let reply = body(&s);
    assert!(reply.starts_with("\n\nRegards,\n") && reply.matches("-----Original Message-----").count() == 1, "{reply:?}");
    assert!(!reply.contains("— Pat"));

    // The list says what each account takes; renaming and deleting follow the choices.
    let listed = s.run("signatures.list", &Value::Null).unwrap();
    assert_eq!(listed["signatures"].as_array().unwrap().len(), 2);
    assert_eq!((listed["accounts"][0]["new"].as_str(), listed["accounts"][0]["own"].as_bool()), (Some("Work"), Some(false)));
    assert_eq!((listed["accounts"][1]["new"].clone(), listed["accounts"][1]["own"].as_bool()), (Value::Null, Some(true)));
    s.run("signatures.save", &json!({ "name": "Formal", "text": "Yours,\n{name}", "was": "Work" })).unwrap();
    assert_eq!(s.signatures.default.new.as_deref(), Some("Formal"));
    assert!(s.run("signatures.save", &json!({ "name": "Short", "text": "x", "was": "Formal" })).is_err(), "a name taken");
    assert!(s.run("signatures.save", &json!({ "name": "", "text": "x" })).is_err());
    s.run("signatures.delete", &json!({ "name": "Formal" })).unwrap();
    assert_eq!(s.signatures.default.new, None);
    assert!(s.run("signatures.choose", &json!({ "new": "Formal" })).is_err());
    assert!(s.run("signatures.choose", &json!({ "account": "nobody", "new": null })).is_err());
    s.run("signatures.choose", &json!({ "account": home, "own": false })).unwrap();
    assert!(s.signatures.by_account.is_empty());

    // Kept between runs.
    let dir = std::env::temp_dir().join(format!("omc-signatures-settings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json").display().to_string();
    s.run("settings.save", &json!({ "path": path })).unwrap();
    let mut again = Session::new();
    again.run("settings.load", &json!({ "path": path })).unwrap();
    assert_eq!(again.signatures, s.signatures);
    let _ = std::fs::remove_dir_all(&dir);
}

fn utf16_file(text: &str) -> Vec<u8> {
    [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect()
}

fn section(name: &str, values: Vec<(&str, Raw)>) -> Section {
    Section { name: name.to_string(), values: values.into_iter().map(|(n, v)| (n.to_string(), v)).collect() }
}

#[test]
fn signatures_are_brought_from_outlook_with_each_accounts_choice() {
    let dir = std::env::temp_dir().join(format!("omc-signatures-import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let theirs = dir.join("Signatures");
    std::fs::create_dir_all(&theirs).unwrap();
    std::fs::write(theirs.join("Work.txt"), utf16_file("\r\nRegards,\r\nPat\r\n")).unwrap();
    std::fs::write(theirs.join("Work.htm"), "<p>Regards,</p>").unwrap();
    std::fs::write(theirs.join("Home.txt"), utf16_file("Cheers")).unwrap();
    let before: Vec<(String, Vec<u8>)> =
        std::fs::read_dir(&theirs).unwrap().map(|e| e.unwrap().path()).map(|p| (p.display().to_string(), std::fs::read(&p).unwrap())).collect();
    let profile = RawProfile {
        name: "Outlook".into(),
        default: true,
        sections: Vec::new(),
        accounts: vec![
            section(
                "00000002",
                vec![
                    ("Account Name", Raw::Text("Mail".into())),
                    ("Email", Raw::Text("kim@example.test".into())),
                    ("POP3 Server", Raw::Text("pop.example.test".into())),
                    ("POP3 Port", Raw::Number(995)),
                    ("POP3 Use SSL", Raw::Number(1)),
                    ("Leave on Server", Raw::Number(0)),
                    ("New Signature", Raw::Text("Work".into())),
                    ("Reply-Forward Signature", Raw::Text("Home".into())),
                ],
            ),
            section(
                "00000003",
                vec![
                    ("Account Name", Raw::Text("Other".into())),
                    ("Email", Raw::Text("lee@example.test".into())),
                    ("POP3 Server", Raw::Text("pop.example.test".into())),
                    ("Leave on Server", Raw::Number(0x000E_0001)),
                    ("New Signature", Raw::Text("Gone".into())),
                ],
            ),
        ],
    };
    let mut s = Session::new();
    s.account_dir = Some(dir.join("app").join("Accounts"));
    s.outlook = Some(Arc::new(move || vec![profile.clone()]));
    s.outlook_signatures = Some(theirs.clone());
    // One of Outlook's names is here already, with other words.
    s.run("signatures.save", &json!({ "name": "Home", "text": "Bye" })).unwrap();

    let preview = s.run("file.importOutlook", &Value::Null).unwrap();
    let sigs = preview["signatures"].as_array().unwrap();
    assert_eq!(sigs.iter().map(|g| g["name"].as_str().unwrap()).collect::<Vec<_>>(), ["Home", "Work"]);
    assert_eq!(
        (sigs[1]["text"].as_str(), sigs[1]["formatted"].as_bool(), sigs[1]["here"].as_bool()),
        (Some("Regards,\nPat"), Some(true), Some(false))
    );
    let kim = &preview["profiles"][0]["accounts"][0];
    assert_eq!(kim["signatures"], json!({ "new": "Work", "replies": "Home" }));
    assert_eq!(kim["leaveOnServer"], json!(false));
    assert_eq!(preview["profiles"][0]["accounts"][1]["leaveOnServer"], json!(true));

    let ids: Vec<String> = sigs.iter().map(|g| g["id"].as_str().unwrap().to_string()).chain(["p0a0".to_string(), "p0a1".to_string()]).collect();
    let done = s.run("file.importOutlook", &json!({ "import": ids, "withData": false })).unwrap();
    assert_eq!(done["signatures"], json!(2), "{done}");
    assert!(s.status.contains("2 signatures"), "{}", s.status);
    // Outlook's Home comes in under a name of its own; ours stays.
    assert_eq!(s.signatures.get("Home").unwrap().text, "Bye");
    assert_eq!(s.signatures.get("Home (2)").unwrap().text, "Cheers");
    assert_eq!(s.signatures.get("Work").unwrap().text, "Regards,\nPat");
    let kim = s.accounts.iter().find(|a| a.email == "kim@example.test").unwrap().clone();
    assert!(!kim.leave_on_server, "as Outlook had it");
    assert!(s.accounts.iter().find(|a| a.email == "lee@example.test").unwrap().leave_on_server);
    let c = s.signatures.choice_for(&kim.id);
    assert_eq!((c.new.as_deref(), c.replies.as_deref()), (Some("Work"), Some("Home (2)")));
    let lee = s.accounts.iter().find(|a| a.email == "lee@example.test").unwrap().id.clone();
    assert!(s.signatures.by_account.iter().all(|(id, _)| *id != lee), "a signature not brought is not chosen");

    // Brought again, nothing is doubled.
    let again = s.run("file.importOutlook", &json!({ "import": ["s1"] })).unwrap();
    assert_eq!(s.signatures.list.len(), 3, "{again}");
    assert!(s.run("file.importOutlook", &json!({ "import": ["s9"] })).is_err());

    // Outlook's files are as they were.
    for (path, bytes) in before {
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "{path}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
