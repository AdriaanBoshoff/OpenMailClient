//! The People module: the sample's contacts listed and found, contacts and groups made, changed,
//! deleted and taken back, vCard files saved and opened again without adding anyone twice, a
//! contact sent on and written to, a sender added, and hostile files and parameters.

use crate::{Module, Session};
use openmailclient_formats::vcf::{self, Card};
use openmailclient_model::{Item, MemStore, time};
use serde_json::{Value, json};
use std::path::PathBuf;

fn session() -> Session {
    let mut s = Session::new();
    s.now = time::from_civil(2026, 10, 7, 14, 30, 0, 0);
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.run(id, &params).unwrap_or_else(|e| panic!("{id} failed: {e}"))
}

fn names(s: &Session) -> Vec<String> {
    s.people.items.iter().map(|(_, c)| c.name()).collect()
}

fn id_of(s: &Session, name: &str) -> u64 {
    s.people.items.iter().find(|(_, c)| c.name() == name).map(|(id, _)| *id).unwrap_or_else(|| panic!("{name} is not shown"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("omc-people-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn people_opens_on_the_sample_contacts_filed_by_surname() {
    let mut s = session();
    run(&mut s, "nav.module", json!({"module": "people"}));
    assert_eq!(s.module, Module::People);
    // Filed as "Surname, Given"; the group by its name.
    assert_eq!(names(&s), ["Launch team", "Tomás Lindqvist", "June Okafor", "Sam Patel", "Priya Raman"]);
    let listed = run(&mut s, "people.items", json!({}));
    assert_eq!(listed["total"], 5);
    let priya = listed["items"].as_array().unwrap().iter().find(|i| i["fileAs"] == "Raman, Priya").unwrap();
    assert_eq!(priya["kind"], "contact");
    let c = &priya["untrusted_content"];
    assert_eq!(c["company"], "Northwind Studio");
    assert_eq!(c["phones"]["mobile"], "+1 555 0142 778");
    assert_eq!(c["addresses"]["business"]["city"], "Port Ellis");
    assert_eq!(c["mailing"], "business");
    let june = listed["items"].as_array().unwrap().iter().find(|i| i["fileAs"] == "Okafor, June").unwrap();
    assert_eq!(june["untrusted_content"]["birthday"], "1990-03-14");
    let team = listed["items"].as_array().unwrap().iter().find(|i| i["kind"] == "group").unwrap();
    assert_eq!(team["untrusted_content"]["members"].as_array().unwrap().len(), 2);

    run(&mut s, "people.search", json!({"query": "northwind"}));
    assert_eq!(names(&s), ["Tomás Lindqvist", "Priya Raman"]);
    run(&mut s, "people.search", json!({"query": "0190"}));
    assert_eq!(names(&s), ["June Okafor"]);
    run(&mut s, "people.search", json!({"query": ""}));
    assert_eq!(names(&s).len(), 5);
    for view in ["businessCard", "card", "phone", "list", "people"] {
        run(&mut s, "people.view", json!({"view": view}));
        assert_eq!(s.people.view.name(), view);
    }
    assert!(s.run("people.view", &json!({"view": "timeline"})).is_err());
}

#[test]
fn a_contact_made_changed_deleted_and_taken_back() {
    let mut s = session();
    run(&mut s, "nav.module", json!({"module": "people"}));
    let made = run(
        &mut s,
        "people.new",
        json!({
            "given": "Ana", "surname": "Silva", "company": "Example Books",
            "emails": ["ana.silva@example.org", {"address": "ana@example.net", "name": "Ana at home"}],
            "phones": {"mobile": "+1 555 0101 010"},
            "addresses": {"home": {"street": "1 Quay Street\nFlat 2", "city": "Port Ellis"}},
            "mailing": "home",
            "birthday": "1985-07-01",
            "categories": ["Friends", "Friends"],
        }),
    );
    let id = made["id"].as_u64().unwrap();
    assert_eq!(s.people.selected, Some(id));
    let Card::Contact(c) = s.person(id).unwrap() else { panic!("not a contact") };
    assert_eq!((c.display_name().as_str(), c.filed_as().as_str()), ("Ana Silva", "Silva, Ana"));
    assert_eq!(c.emails.len(), 2);
    assert_eq!(c.categories, ["Friends"]);
    assert!(!c.uid.is_empty());
    assert_eq!(c.address(openmailclient_model::contact::Place::Home).unwrap().street, "1 Quay Street\nFlat 2");

    run(&mut s, "people.change", json!({"jobTitle": "Editor", "phones": {"mobile": null, "business": "+1 555 0101 999"}, "birthday": null}));
    let Card::Contact(c) = s.person(id).unwrap() else { panic!("not a contact") };
    assert_eq!(c.job_title, "Editor");
    assert_eq!(c.phones, [(openmailclient_model::contact::Phone::Business, "+1 555 0101 999".to_string())]);
    assert_eq!(c.birthday, None);
    // Changing the name's parts makes the whole again.
    run(&mut s, "people.change", json!({"given": "Anabela"}));
    assert_eq!(s.person(id).unwrap().name(), "Anabela Silva");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.person(id).unwrap().name(), "Ana Silva");

    let at = s.people.items.iter().position(|(i, _)| *i == id).unwrap();
    run(&mut s, "people.delete", json!({}));
    assert!(!names(&s).contains(&"Ana Silva".to_string()));
    // The one that took its place is chosen, as in Outlook.
    let next = s.people.items.get(at.min(s.people.items.len() - 1)).map(|(i, _)| *i);
    assert_eq!(s.people.selected, next);
    run(&mut s, "edit.undo", json!({}));
    assert!(names(&s).contains(&"Ana Silva".to_string()));

    // A contact needs something to be called by; an address must be one.
    assert!(s.run("people.new", &json!({"notes": "nobody"})).is_err());
    assert!(s.run("people.new", &json!({"given": "X", "emails": ["not an address"]})).is_err());
    assert!(s.run("people.new", &json!({"given": "X", "emails": ["a@x.org", "b@x.org", "c@x.org", "d@x.org"]})).is_err());
    assert!(s.run("people.new", &json!({"given": "X", "phones": {"satellite": "1"}})).is_err());
    assert!(s.run("people.new", &json!({"given": "X", "mailing": "home"})).is_err());
}

#[test]
fn a_group_made_from_addresses_and_contacts_and_changed() {
    let mut s = session();
    run(&mut s, "nav.module", json!({"module": "people"}));
    let june = id_of(&s, "June Okafor");
    let made = run(
        &mut s,
        "people.newGroup",
        json!({"name": "Printers", "members": ["Ops <ops@example.org>", {"contact": june}, {"address": "ops@example.org", "name": "twice"}]}),
    );
    let id = made["id"].as_u64().unwrap();
    let Card::Group(g) = s.person(id).unwrap() else { panic!("not a group") };
    let addresses: Vec<&str> = g.members.iter().map(|m| m.address.as_str()).collect();
    assert_eq!(addresses, ["ops@example.org", "june.okafor@example.org"]);
    assert_eq!(g.members[1].name, "June Okafor");

    run(&mut s, "people.changeGroup", json!({"add": ["sam.patel@example.org"], "remove": ["OPS@example.org"]}));
    let Card::Group(g) = s.person(id).unwrap() else { panic!("not a group") };
    let addresses: Vec<&str> = g.members.iter().map(|m| m.address.as_str()).collect();
    assert_eq!(addresses, ["june.okafor@example.org", "sam.patel@example.org"]);

    // A message to the group goes to each member once.
    run(&mut s, "people.email", json!({}));
    let draft = s.compose.as_ref().unwrap();
    assert!(draft.to.contains("june.okafor@example.org") && draft.to.contains("sam.patel@example.org"));
    assert!(s.run("people.newGroup", &json!({"members": ["a@example.org"]})).is_err());
    assert!(s.run("people.change", &json!({"id": id, "company": "x"})).is_err());
}

#[test]
fn saved_as_a_vcard_and_opened_again_nothing_is_added_twice() {
    let mut s = session();
    run(&mut s, "nav.module", json!({"module": "people"}));
    let dir = scratch("again");
    let path = dir.join("all.vcf");
    let saved = run(&mut s, "people.saveAs", json!({"path": path.to_str().unwrap(), "all": true}));
    assert_eq!(saved["cards"], 5);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("VERSION:3.0") && text.contains("KIND:group"));

    // Opened in a mailbox that has none of them, all are added, the group with its members.
    let mut other = Session::new();
    other.now = s.now;
    other.add_store(Box::new(MemStore::with_default_folders("me@example.org")));
    assert!(other.run("people.import", &json!({"path": path.to_str().unwrap()})).is_err(), "there is no Contacts folder");

    // Opened again where they came from: each is known, and what the file does not say stays.
    let priya = id_of(&s, "Priya Raman");
    run(&mut s, "people.change", json!({"id": priya, "notes": "Met at the fair."}));
    let mut card = vcf::read(text.as_bytes()).unwrap();
    assert_eq!(card.cards.len(), 5);
    if let Some(Card::Contact(c)) = card.cards.iter_mut().find(|c| c.name() == "Priya Raman") {
        c.job_title = "Head of Product".into();
        c.notes.clear();
    }
    let changed = dir.join("changed.vcf");
    let list: Vec<(Card, String)> = card.cards.into_iter().map(|c| (c.clone(), c.uid().to_string())).collect();
    std::fs::write(&changed, vcf::write(&list, s.now)).unwrap();
    let preview = run(&mut s, "people.import", json!({"path": changed.to_str().unwrap(), "preview": true}));
    assert!(preview["cards"].as_array().unwrap().iter().all(|c| c["replaces"].is_u64()));
    assert_eq!(names(&s).len(), 5);
    let done = run(&mut s, "people.import", json!({"path": changed.to_str().unwrap()}));
    assert_eq!((done["added"].as_u64(), done["updated"].as_u64()), (Some(0), Some(5)));
    assert_eq!(names(&s).len(), 5);
    let Card::Contact(c) = s.person(priya).unwrap() else { panic!("not a contact") };
    assert_eq!(c.job_title, "Head of Product");
    assert_eq!(c.notes, "Met at the fair.");
    run(&mut s, "edit.undo", json!({}));
    let Card::Contact(c) = s.person(priya).unwrap() else { panic!("not a contact") };
    assert_eq!(c.job_title, "Product Lead");

    // A card known only by an address brings that contact up to date.
    let lone = dir.join("lone.vcf");
    std::fs::write(&lone, "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Tom L.\r\nEMAIL:TOMAS.LINDQVIST@example.org\r\nTEL;TYPE=HOME:+1 555 0177 000\r\nEND:VCARD\r\nBEGIN:VCARD\r\nVERSION:3.0\r\nFN:Kai New\r\nEMAIL:kai@example.org\r\nEND:VCARD\r\n").unwrap();
    let only = run(&mut s, "people.import", json!({"path": lone.to_str().unwrap(), "only": [0]}));
    assert_eq!((only["added"].as_u64(), only["updated"].as_u64()), (Some(0), Some(1)));
    let tomas = id_of(&s, "Tom L.");
    let Card::Contact(c) = s.person(tomas).unwrap() else { panic!("not a contact") };
    assert_eq!(c.company, "Northwind Studio");
    assert_eq!(c.phone(openmailclient_model::contact::Phone::Home), Some("+1 555 0177 000"));
    let rest = run(&mut s, "people.import", json!({"path": lone.to_str().unwrap()}));
    assert_eq!((rest["added"].as_u64(), rest["updated"].as_u64()), (Some(1), Some(1)));
    assert_eq!(names(&s).len(), 6);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_contact_sent_on_as_a_card_and_a_sender_added() {
    let mut s = session();
    run(&mut s, "nav.module", json!({"module": "people"}));
    let june = id_of(&s, "June Okafor");
    run(&mut s, "people.select", json!({"id": june}));
    run(&mut s, "people.forward", json!({}));
    let draft = s.compose.as_ref().unwrap();
    assert_eq!(draft.subject, "FW: June Okafor");
    assert!(draft.body.contains("Riverside Print"));
    let att = draft.attachments.first().unwrap();
    let back = vcf::read(&att.data).unwrap();
    assert_eq!(back.cards.first().map(Card::name).as_deref(), Some("June Okafor"));

    // A message from someone new: they are made a contact; one known is only chosen.
    let mut message = Item::default();
    message.props.set_str(openmailclient_model::props::pid::SENDER_NAME, "Lee, Morgan");
    message.props.set_str(openmailclient_model::props::pid::SENDER_EMAIL_ADDRESS, "morgan.lee@example.org");
    s.open = Some(message);
    let added = run(&mut s, "mail.addSenderToContacts", json!({}));
    assert_eq!(added["added"], true);
    let Card::Contact(c) = s.person(added["id"].as_u64().unwrap()).unwrap() else { panic!("not a contact") };
    assert_eq!((c.given.as_str(), c.surname.as_str(), c.display_name().as_str()), ("Morgan", "Lee", "Morgan Lee"));
    let again = run(&mut s, "mail.addSenderToContacts", json!({}));
    assert_eq!((again["added"].as_bool(), again["id"].as_u64()), (Some(false), added["id"].as_u64()));
}

#[test]
fn hostile_cards_and_parameters_are_turned_away() {
    let mut s = session();
    run(&mut s, "nav.module", json!({"module": "people"}));
    let dir = scratch("hostile");
    for (name, bytes) in [
        ("empty.vcf", Vec::new()),
        ("garbage.vcf", (0..4096u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect()),
        ("unclosed.vcf", b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Half".to_vec()),
        ("nested.vcf", "BEGIN:VCARD\r\n".repeat(50).into_bytes()),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        // Read or turned away, never a crash; nothing that is not a card is added.
        let _ = s.run("people.import", &json!({"path": path.to_str().unwrap()}));
    }
    assert!(s.run("people.import", &json!({"path": dir.to_str().unwrap()})).is_err());
    assert!(s.run("people.import", &json!({"attachment": 0})).is_err());
    assert!(s.run("people.saveAs", &json!({"path": dir.join("x.txt").to_str().unwrap(), "all": true})).is_err());
    let picture = dir.join("not-a-picture.png");
    std::fs::write(&picture, b"<script>").unwrap();
    assert!(s.run("people.new", &json!({"given": "P", "picture": picture.to_str().unwrap()})).is_err());
    assert!(s.run("people.select", &json!({"id": 999_999})).is_err());
    assert!(s.run("people.open", &json!({"store": 0, "folder": 1})).is_err());
    let too_many: Vec<String> = (0..10_001).map(|i| format!("m{i}@example.org")).collect();
    assert!(s.run("people.newGroup", &json!({"name": "Huge", "members": too_many})).is_err());
    std::fs::remove_dir_all(&dir).ok();
}
