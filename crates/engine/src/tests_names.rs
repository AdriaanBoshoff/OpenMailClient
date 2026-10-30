//! The Address Book, names suggested as they are typed, and Check Names, on the sample's
//! people: Priya Raman, Tomás Lindqvist, June Okafor, Sam Patel and the Launch team.

use crate::Session;
use serde_json::{Value, json};

fn writing(to: &str) -> Session {
    let mut s = Session::with_sample();
    s.run("mail.new", &Value::Null).unwrap();
    s.run("compose.setField", &json!({ "field": "to", "value": to })).unwrap();
    s
}

fn to(s: &Session) -> String {
    s.compose.as_ref().unwrap().to.clone()
}

#[test]
fn the_address_book_lists_everyone_of_every_contacts_folder_and_finds_by_any_word() {
    let mut s = Session::with_sample();
    let all = s.run("people.addressBook", &Value::Null).unwrap();
    assert_eq!(all["total"], json!(5), "four people and a group: {all}");
    assert_eq!(all["books"][0]["name"], json!("Contacts"));
    let entries = all["untrusted_content"]["entries"].as_array().unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["June Okafor", "Launch team", "Priya Raman", "Sam Patel", "Tomás Lindqvist"], "in the order they are filed");
    let team = entries.iter().find(|e| e["kind"] == json!("group")).unwrap();
    assert_eq!(team["members"].as_array().unwrap().len(), 2);
    assert_eq!(s.people.book, Some(String::new()), "the window is opened");

    let found = s.run("people.addressBook", &json!({ "query": "north", "open": false })).unwrap();
    assert_eq!(found["total"], json!(2), "by company: {found}");
    let found = s.run("people.addressBook", &json!({ "query": "okafor" })).unwrap();
    assert_eq!(found["untrusted_content"]["entries"][0]["address"], json!("june.okafor@example.org"));
    assert_eq!(s.run("people.addressBook", &json!({ "query": "nobody here" })).unwrap()["total"], json!(0));

    for bad in [json!({ "store": 0 }), json!({ "store": 9, "folder": 1 }), json!({ "for": "subject" }), json!({ "store": -1, "folder": 1 })] {
        assert!(s.run("people.addressBook", &bad).is_err(), "{bad}");
    }
    assert_eq!(s.run("people.addressBook", &json!({ "for": "cc" })).unwrap()["total"], json!(5));
    assert_eq!(s.people.book.as_deref(), Some("cc"));
}

#[test]
fn names_are_suggested_for_what_is_being_typed_the_best_first() {
    let mut s = Session::with_sample();
    let got = s.run("people.suggest", &json!({ "text": "priya.raman@example.org; to" })).unwrap();
    assert_eq!(got["for"], json!("to"));
    let first = &got["untrusted_content"]["suggestions"][0];
    assert_eq!(first["name"], json!("Tomás Lindqvist"), "its name begins with it: {got}");
    assert_eq!(first["as"], json!("Tomás Lindqvist <tomas.lindqvist@example.org>"));
    let got = s.run("people.suggest", &json!({ "text": "lau" })).unwrap();
    assert_eq!(got["untrusted_content"]["suggestions"][0]["kind"], json!("group"));
    assert!(got["untrusted_content"]["suggestions"][0]["as"].as_str().unwrap().contains("priya.raman@example.org"));
    // Nothing yet, or an address already written out, asks for nothing.
    for text in ["", "Ana <a@x.test>", "a@x.test, "] {
        assert_eq!(s.run("people.suggest", &json!({ "text": text })).unwrap()["untrusted_content"]["suggestions"], json!([]), "{text}");
    }
    assert!(s.run("people.suggest", &Value::Null).is_err());
    let long = "a".repeat(100_000);
    assert!(s.run("people.suggest", &json!({ "text": long })).is_ok());
}

#[test]
fn check_names_puts_in_what_the_address_book_has_once_and_offers_the_rest() {
    let mut s = writing("June; sam.patel@example.org; Launch team; north; Nobody Known; \"Kept, As\" <kept@example.test>");
    let got = s.run("compose.checkNames", &Value::Null).unwrap();
    assert_eq!(got["resolved"], json!(2), "{got}");
    assert_eq!(got["ambiguous"], json!(1), "both at Northwind Studio: {got}");
    assert_eq!(got["unknown"], json!(1));
    let line = &got["untrusted_content"]["to"];
    assert_eq!(line[0]["state"], json!("resolved"));
    assert_eq!(line[1]["state"], json!("address"));
    assert_eq!(line[1]["as"], json!("Sam Patel <sam.patel@example.org>"), "an address is given its contact's name");
    assert!(line[3]["choices"].as_array().unwrap().len() >= 2);
    assert_eq!(
        to(&s),
        "June Okafor <june.okafor@example.org>; Sam Patel <sam.patel@example.org>; Priya Raman <priya.raman@example.org>; Tomás Lindqvist <tomas.lindqvist@example.org>; north; Nobody Known; \"Kept, As\" <kept@example.test>",
        "a group goes in as its members"
    );
    // Checked again, nothing more changes.
    let again = s.run("compose.checkNames", &json!({ "field": "to" })).unwrap();
    assert_eq!(again["resolved"], json!(0));

    // Looked at without changing the line.
    let mut s = writing("priya");
    let got = s.run("compose.checkNames", &json!({ "apply": false })).unwrap();
    assert_eq!(got["resolved"], json!(1));
    assert_eq!(to(&s), "priya");
    assert!(s.run("compose.checkNames", &json!({ "field": "subject" })).is_err());
    let mut idle = Session::with_sample();
    assert!(idle.run("compose.checkNames", &Value::Null).is_err(), "nothing is being written");
}

#[test]
fn send_puts_in_a_name_the_address_book_has_once_and_cautions_of_the_others() {
    let mut s = writing("Priya Raman");
    s.run("compose.setField", &json!({ "field": "subject", "value": "Plans" })).unwrap();
    s.run("compose.send", &Value::Null).unwrap();
    assert!(s.compose.is_none(), "sent to the Outbox");

    let mut s = writing("north");
    s.run("compose.setField", &json!({ "field": "subject", "value": "Plans" })).unwrap();
    let e = s.run("compose.send", &Value::Null).unwrap_err().to_string();
    assert!(e.contains("not an address"), "{e}");
    assert_eq!(to(&s), "north", "a name of several is left as typed");
}

#[test]
fn hostile_lines_are_bounded() {
    let mut s = writing(&"x; ".repeat(10_000));
    let got = s.run("compose.checkNames", &Value::Null).unwrap();
    assert!(got["untrusted_content"]["to"].as_array().unwrap().len() <= 500);
    let mut s = writing(&format!("{}\"<<<>>>;;,,", "\"<".repeat(5_000)));
    assert!(s.run("compose.checkNames", &Value::Null).is_ok());
}

#[test]
fn mail_merge_writes_one_message_to_each_with_their_fields_and_no_one_twice() {
    let mut s = Session::with_sample();
    let p = json!({ "subject": "Plans for «Company»", "body": "Dear {{first name}},\nwe meet in «City».", "deliver": "preview" });
    let got = s.run("people.mailMerge", &p).unwrap();
    assert_eq!(got["made"], json!(0), "a preview makes nothing: {got}");
    assert_eq!(
        got["to"],
        json!([
            "Priya Raman <priya.raman@example.org>",
            "Tomás Lindqvist <tomas.lindqvist@example.org>",
            "June Okafor <june.okafor@example.org>",
            "Sam Patel <sam.patel@example.org>"
        ]),
        "the group's members once each, as their contacts say"
    );
    let previews = got["untrusted_content"]["previews"].as_array().unwrap();
    assert_eq!(previews.len(), 3);
    assert_eq!(previews[2]["subject"], json!("Plans for Riverside Print"));
    assert_eq!(previews[1]["body"], json!("Dear Tomás,\nwe meet in ."), "a field it lacks is left empty");
    assert_eq!(previews[0]["body"], json!("Dear Priya,\nwe meet in Port Ellis."), "the mailing address");

    let drafts = s.find_folder(s.writable_store().unwrap(), openmailclient_model::FolderKind::Drafts).unwrap();
    let got = s.run("people.mailMerge", &json!({ "subject": "Hi «First Name»", "body": "«Full Name», «Mailing Address»" })).unwrap();
    assert_eq!(got["made"], json!(4), "{got}");
    let store = s.writable_store().unwrap();
    let id = got["ids"][3].as_u64().unwrap();
    let item = s.stores[store].load(drafts, openmailclient_model::ItemId(id)).unwrap();
    assert_eq!(item.subject(), "Hi Sam");
    assert_eq!(item.body_text().trim_end(), "Sam Patel, 4 Willow Lane, Port Ellis 40118", "the home address when it is the only one");

    // The one chosen, to the Outbox; and someone without an address is named, not passed over.
    let none = s.run("people.new", &json!({ "given": "Ola", "surname": "Nowak" })).unwrap();
    let ola = none["id"].as_u64().unwrap();
    let got = s.run("people.mailMerge", &json!({ "body": "x", "contacts": [ola], "deliver": "outbox" })).unwrap();
    assert_eq!(got["made"], json!(0));
    assert_eq!(got["skipped"][0]["why"], json!("has no e-mail address"));
    s.reload_people().unwrap();
    let sam = s.people.items.iter().find(|(_, c)| matches!(c, openmailclient_formats::vcf::Card::Contact(c) if c.given == "Sam")).unwrap().0;
    s.people.selected = Some(sam);
    let got = s.run("people.mailMerge", &json!({ "body": "x", "contacts": "selected", "deliver": "outbox" })).unwrap();
    assert_eq!(got["made"], json!(1));
    assert!(s.status.contains("Outbox"), "{}", s.status);

    for bad in [
        json!({ "body": "Hi «Frist Name»" }),
        json!({ "subject": "x" }),
        json!({ "body": "x", "deliver": "fax" }),
        json!({ "body": "x", "contacts": "everyone" }),
        json!({ "body": "x", "contacts": ["a"] }),
        json!({ "body": "x", "contacts": [] }),
        json!({ "body": "x", "account": "nobody" }),
    ] {
        assert!(s.run("people.mailMerge", &bad).is_err(), "{bad}");
    }
    let e = s.run("people.mailMerge", &json!({ "body": "Hi «Frist Name»" })).unwrap_err().to_string();
    assert!(e.contains("First Name"), "the fields are named: {e}");
}

#[test]
fn mail_merge_chooses_by_a_filter_and_prints_letters_a_page_each_to_one_document() {
    let mut s = Session::with_sample();
    // A filter: only those in Port Ellis, compared without regard to case.
    let p = json!({ "body": "x", "deliver": "preview", "filter": [{ "field": "City", "op": "is", "value": "port ellis" }] });
    let got = s.run("people.mailMerge", &p).unwrap();
    assert_eq!(got["to"], json!(["Priya Raman <priya.raman@example.org>", "Sam Patel <sam.patel@example.org>"]), "{got}");
    assert_eq!(got["filtered"], json!(2));
    let p = json!({ "body": "x", "deliver": "preview", "filterAny": true, "filter": [
        { "field": "First Name", "op": "startsWith", "value": "T" },
        { "field": "company", "op": "contains", "value": "PRINT" },
    ] });
    let got = s.run("people.mailMerge", &p).unwrap();
    assert_eq!(got["to"], json!(["Tomás Lindqvist <tomas.lindqvist@example.org>", "June Okafor <june.okafor@example.org>"]), "{got}");
    let got = s.run("people.mailMerge", &json!({ "body": "x", "deliver": "preview", "filter": [{ "field": "Company", "op": "empty" }] })).unwrap();
    assert!(got["to"].as_array().unwrap().iter().all(|t| !t.as_str().unwrap().contains("June")), "{got}");

    // Letters: one each, someone without an e-mail address too, each on a page of its own.
    let ola = s.run("people.new", &json!({ "given": "Ola", "surname": "Nowak", "prefix": "Ms." })).unwrap()["id"].as_u64().unwrap();
    let dir = std::env::temp_dir().join(format!("omc-merge-letters-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = dir.join("letters.pdf");
    let body = "«Address Block»\n\n«Greeting Line»\n\nOur shop opens on Monday.";
    let got = s.run("people.mailMerge", &json!({ "body": body, "deliver": "letters", "path": pdf })).unwrap();
    assert_eq!(got["letters"], json!(5), "{got}");
    assert_eq!(got["pages"], json!(5), "a page each");
    assert_eq!(got["noAddress"], json!(["Tomás Lindqvist", "Ms. Ola Nowak", "June Okafor"]), "named, still written to");
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF"));
    let previews = got["untrusted_content"]["previews"].as_array().unwrap();
    assert_eq!(
        previews[0]["body"],
        json!("Priya Raman\nNorthwind Studio\n12 Harbour Road\nPort Ellis 40112\nExample\n\nDear Priya,\n\nOur shop opens on Monday."),
        "{got}"
    );
    let got = s.run("people.mailMerge", &json!({ "body": "«Greeting Line»", "deliver": "letters", "path": pdf, "contacts": [ola] })).unwrap();
    assert_eq!(got["untrusted_content"]["previews"][0]["body"], json!("Dear Ms. Nowak,"));
    assert!(s.status.contains("1 letter on 1 page"), "{}", s.status);
    let got = s
        .run(
            "people.mailMerge",
            &json!({ "body": "«Greeting Line»", "deliver": "letters", "preview": true, "filter": [{ "field": "City", "op": "notEmpty" }] }),
        )
        .unwrap();
    assert_eq!((got["letters"].clone(), got.get("pages").is_none()), (json!(2), true), "looked at, nothing laid out: {got}");
    // In a message's subject an address block is one line.
    let got = s.run("people.mailMerge", &json!({ "subject": "«Address Block»", "body": "x", "deliver": "preview", "contacts": "shown" })).unwrap();
    assert!(!got["untrusted_content"]["previews"][0]["subject"].as_str().unwrap().contains('\n'));

    for bad in [
        json!({ "body": "x", "deliver": "letters" }),
        json!({ "body": "x", "deliver": "letters", "path": dir.join("letters.txt") }),
        json!({ "body": "x", "deliver": "letters", "printer": "No Such Printer Here" }),
        json!({ "body": "x", "deliver": "letters", "path": pdf, "filter": [{ "field": "City", "value": "Nowhere" }] }),
        json!({ "body": "x", "filter": "City" }),
        json!({ "body": "x", "filter": [{ "field": "Shoe Size", "value": "9" }] }),
        json!({ "body": "x", "filter": [{ "field": "City", "op": "like", "value": "a" }] }),
        json!({ "body": "x", "filter": [{ "field": "City", "op": "is" }] }),
        json!({ "body": "x", "filter": [{ "field": "City", "value": ["a"] }] }),
        json!({ "body": "x", "filter": vec![json!({ "field": "City", "op": "empty" }); 21] }),
    ] {
        assert!(s.run("people.mailMerge", &bad).is_err(), "{bad}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
