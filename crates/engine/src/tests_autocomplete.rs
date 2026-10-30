//! The AutoComplete list: added to by each send, suggested first, taken off one by one or all
//! at once, kept between runs, and brought from a data file Outlook kept one in.

use crate::Session;
use openmailclient_model::{FolderId, FolderInfo, FolderKind, Item, ItemId, MemStore, PropValue, Store, StoreError, Summary, pid};
use serde_json::{Value, json};

/// The sample, with Outlook's AutoComplete message hidden in its Inbox.
struct Kept(MemStore, Vec<u8>);

impl Store for Kept {
    fn display_name(&self) -> String {
        self.0.display_name()
    }
    fn folders(&mut self) -> Result<Vec<FolderInfo>, StoreError> {
        self.0.folders()
    }
    fn list(&mut self, folder: FolderId) -> Result<Vec<Summary>, StoreError> {
        self.0.list(folder)
    }
    fn load(&mut self, folder: FolderId, id: ItemId) -> Result<Item, StoreError> {
        self.0.load(folder, id)
    }
    fn hidden(&mut self, folder: FolderId) -> Result<Vec<(ItemId, Result<Item, StoreError>)>, StoreError> {
        let inbox = self.0.folders()?.into_iter().find(|f| f.kind == FolderKind::Inbox).map(|f| f.id);
        if inbox != Some(folder) {
            return Ok(Vec::new());
        }
        let mut views = Item::default();
        views.props.set_str(pid::MESSAGE_CLASS, "IPM.Microsoft.FolderDesign.NamedView");
        let mut list = Item::default();
        list.props.set_str(pid::MESSAGE_CLASS, "IPM.Configuration.Autocomplete");
        list.props.set(0x7C09, PropValue::Binary(self.1.clone()));
        Ok(vec![(ItemId(1), Ok(views)), (ItemId(2), Err(StoreError::NoFolder)), (ItemId(3), Ok(list))])
    }
}

fn names(got: &Value) -> Vec<String> {
    got["untrusted_content"]["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| format!("{} {}", n["kind"].as_str().unwrap(), n["address"].as_str().unwrap()))
        .collect()
}

#[test]
fn what_is_sent_is_suggested_first_and_can_be_forgotten() {
    let mut s = Session::with_sample();
    s.run("mail.new", &Value::Null).unwrap();
    s.run("compose.setField", &json!({ "field": "to", "value": "Pat Rivera <pat.rivera@example.net>, june.okafor@example.org" })).unwrap();
    s.run("compose.setField", &json!({ "field": "subject", "value": "Hello" })).unwrap();
    s.run("compose.send", &json!({ "anyway": true })).unwrap();
    let list = s.run("people.autoComplete", &Value::Null).unwrap();
    assert_eq!(list["total"], 2, "{list}");
    // One written to before comes before the contacts, and is not shown twice.
    let got = s.run("people.suggest", &json!({ "text": "pa" })).unwrap();
    assert_eq!(names(&got)[0], "recent pat.rivera@example.net", "{got}");
    let got = s.run("people.suggest", &json!({ "text": "june" })).unwrap();
    assert_eq!(names(&got), ["recent june.okafor@example.org"], "{got}");

    s.run("people.forgetName", &json!({ "address": "PAT.RIVERA@example.net" })).unwrap();
    assert!(!names(&s.run("people.suggest", &json!({ "text": "pat r" })).unwrap()).iter().any(|n| n.contains("rivera")));
    assert!(s.run("people.forgetName", &json!({ "address": "nobody@example.net" })).is_err());

    let dir = std::env::temp_dir().join(format!("omc-autocomplete-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json").display().to_string();
    s.run("settings.save", &json!({ "path": path })).unwrap();
    let mut again = Session::new();
    again.run("settings.load", &json!({ "path": path })).unwrap();
    assert_eq!(again.autocomplete, s.autocomplete);
    assert_eq!(s.run("people.emptyAutoComplete", &Value::Null).unwrap()["removed"], 1);
    assert!(s.autocomplete.list.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn outlooks_list_is_brought_from_a_data_file() {
    let stream = crate::autocomplete::tests::stream(&[
        &[(0x3001, "Robin Shaw"), (0x3002, "SMTP"), (0x3003, "robin@example.net")],
        &[(0x3001, "Priya"), (0x3002, "SMTP"), (0x3003, "priya.raman@example.org")],
    ]);
    let mut s = Session::new();
    s.add_store(Box::new(Kept(crate::sample::sample_store(s.now), stream)));
    s.autocomplete.learn("Priya Raman", "priya.raman@example.org", 5);
    let got = s.run("people.bringAutoComplete", &Value::Null).unwrap();
    assert_eq!((got["found"].as_u64(), got["added"].as_u64()), (Some(2), Some(1)), "{got}");
    // The name it had already is kept.
    assert!(s.autocomplete.list.iter().any(|n| n.name == "Priya Raman"));
    assert_eq!(names(&s.run("people.suggest", &json!({ "text": "rob" })).unwrap())[0], "recent robin@example.net");
    assert!(s.run("people.bringAutoComplete", &json!({ "store": 4 })).is_err());
    // A data file without one adds nothing.
    let mut plain = Session::with_sample();
    assert_eq!(plain.run("people.bringAutoComplete", &Value::Null).unwrap()["found"], 0);
}
