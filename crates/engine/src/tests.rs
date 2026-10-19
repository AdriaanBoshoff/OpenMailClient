use crate::{Draft, EngineError, Session, catalog, registry};
use openmailclient_model::{FolderId, FolderKind, RECIP_CC, RECIP_TO, time};
use serde_json::{Value, json};

/// 2026-10-07 14:30 UTC, a Wednesday.
fn now() -> i64 {
    time::from_civil(2026, 10, 7, 14, 30, 0, 0)
}

fn session() -> Session {
    let mut s = Session::new();
    s.now = now();
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.run(id, &params).unwrap_or_else(|e| panic!("{id} failed: {e}"))
}

fn folder(s: &Session, name: &str) -> (u32, u32) {
    let f = s.folders[0].iter().find(|f| f.name == name).unwrap();
    (f.total, f.unread)
}

#[test]
fn command_ids_are_unique_and_described() {
    let mut ids: Vec<&str> = registry().iter().map(|c| c.id).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "two commands share an id");
    for c in registry() {
        assert!(!c.label.is_empty() && c.id.contains('.'), "{} needs a label and a dotted id", c.id);
    }
}

#[test]
fn catalog_names_real_commands_and_parity_only_rises() {
    for f in catalog::FEATURES {
        assert!(f.command.is_empty() || f.implemented(), "{} points at a command that does not exist: {}", f.name, f.command);
    }
    let (done, total) = catalog::summary();
    // The floor only goes up: raise it when features land.
    assert!(done >= 56, "parity fell to {done} of {total}");
    assert!(catalog::checklist().contains("- [x] New: New Email"));
}

/// Every command, with every kind of junk for parameters, on a session with and without a
/// selection and with a compose window open: it may refuse, it may not panic.
#[test]
fn hostile_params_never_panic() {
    let junk = [
        Value::Null,
        json!(42),
        json!("text"),
        json!([1, 2, 3]),
        json!({}),
        json!({"index": -1, "id": -5, "store": -1, "folderId": -9}),
        json!({"index": 1e300, "id": 1.5, "store": 1e99, "folderId": 2.5}),
        json!({"index": u64::MAX, "id": u64::MAX, "store": u64::MAX, "folderId": u64::MAX}),
        json!({"index": "0", "id": [], "store": {}, "folderId": null}),
        json!({"path": 7, "folder": 7, "name": 7, "field": 7, "value": 7, "key": 7, "module": 7, "position": 7, "level": 7, "category": 7, "text": 7}),
        json!({"path": "", "folder": "", "name": "", "field": "", "value": "", "key": "", "module": "", "position": "", "level": "", "category": "", "text": ""}),
        json!({"path": "\u{0}", "folder": "a/b/c/d/../..", "name": "x/y", "field": "body", "value": "é".repeat(40_000), "key": "\u{202e}", "category": "c".repeat(4000), "text": "\u{0}\u{ffff}"}),
        json!({"path": "Z:\\no\\such\\dir\\file.msg", "folder": "/", "name": "\u{7}", "field": "to", "value": "<<<>>>;;;,,,\"", "index": 99999}),
        json!({"show": "yes", "on": 3, "descending": "no", "clear": "true"}),
        json!({"address": 7, "confirm": 7}),
        json!({"style": "table", "page": 2, "dpi": 24}),
        json!({"style": {"table": 1}, "reset": "yes"}),
        json!({"page": 0, "dpi": 1e9, "path": "Z:/no/such/dir/x.png"}),
        json!({"page": 1e300, "dpi": "x", "path": "Z:/no/such/dir/con.png", "margins": 149}),
        json!({"margins": 1e308, "orientation": 7, "reset": "x", "header": {"left": "[".repeat(70_000)}, "footer": []}),
        json!({"margins": {"top": null, "left": -1e300}, "header": 7, "footer": {"center": "[Page".repeat(20_000)}}),
        json!({"margins": 0, "paper": "tabloid", "orientation": "landscape", "header": "[Subject][Date][Time][User]", "reset": true}),
        json!({"address": "file:///C:/a.exe", "confirm": true, "index": 0}),
        json!({"address": "h".repeat(70_000), "confirm": "x"}),
    ];
    let setups: [fn(&mut Session); 4] = [
        |_| {},
        |s| {
            let _ = s.run("mail.select", &json!({"index": 0}));
        },
        |s| {
            let _ = s.run("mail.select", &json!({"index": 1}));
            let _ = s.run("mail.replyAll", &Value::Null);
        },
        |s| {
            while !s.stores.is_empty() {
                let _ = s.run("file.closeStore", &Value::Null);
            }
        },
    ];
    for setup in setups {
        for c in registry() {
            for p in &junk {
                let mut s = session();
                setup(&mut s);
                let result = s.run(c.id, p);
                assert!(!matches!(result, Err(EngineError::Internal(_))), "{} panicked on {p}", c.id);
                // Whatever happened, the session still answers.
                let _ = s.inspect();
                assert!(!matches!(s.run("session.inspect", &Value::Null), Err(EngineError::Internal(_))));
            }
        }
    }
    assert!(matches!(session().run("no.such.command", &Value::Null), Err(EngineError::UnknownCommand(_))));
}

#[test]
fn opens_on_the_inbox_newest_first() {
    let s = session();
    assert_eq!(s.current_folder().unwrap().kind, FolderKind::Inbox);
    assert_eq!(s.rows.len(), 9);
    assert_eq!(s.rows[0].subject, "Launch checklist: three items left");
    assert!(s.rows.windows(2).all(|w| w[0].time >= w[1].time));
    assert_eq!(folder(&s, "Inbox"), (9, 3));
    assert!(s.selected.is_none() && s.open.is_none());
}

#[test]
fn select_can_go_to_a_folder_first() {
    let mut s = session();
    run(&mut s, "mail.select", json!({ "folder": "Sent Items", "index": 0 }));
    assert_eq!(s.current_folder().unwrap().kind, FolderKind::Sent);
    assert_eq!(s.open.as_ref().unwrap().subject(), "RE: Launch checklist: three items left");
    assert!(s.run("mail.select", &json!({ "folder": "No/Such", "index": 0 })).is_err());
    assert_eq!(s.current_folder().unwrap().kind, FolderKind::Sent);
    // A folder alone goes there and opens nothing.
    run(&mut s, "mail.select", json!({ "folder": "Inbox" }));
    assert_eq!(s.current_folder().unwrap().kind, FolderKind::Inbox);
    assert!(s.open.is_none());
    assert!(s.run("mail.select", &json!({})).is_err());
}

#[test]
fn selecting_marks_read_and_delete_moves_to_deleted_items() {
    let mut s = session();
    run(&mut s, "mail.select", json!({"index": 0}));
    assert_eq!(folder(&s, "Inbox"), (9, 2), "the reading pane marks the message read");
    assert!(s.rows[0].read);
    run(&mut s, "mail.markUnread", Value::Null);
    assert_eq!(folder(&s, "Inbox"), (9, 3));

    let second = s.rows[1].id;
    run(&mut s, "mail.delete", Value::Null);
    assert_eq!(folder(&s, "Inbox"), (8, 2));
    assert_eq!(folder(&s, "Deleted Items").0, 2);
    assert_eq!(s.selected, Some(second), "selection moves to the next message");
    assert!(!s.rows[0].read, "moving on does not mark the neighbour read");

    // Deleting inside Deleted Items removes for good.
    run(&mut s, "folder.select", json!({"folder": "Deleted Items"}));
    run(&mut s, "mail.select", json!({"index": 0}));
    run(&mut s, "mail.delete", Value::Null);
    assert_eq!(folder(&s, "Deleted Items").0, 1);
    run(&mut s, "folder.empty", Value::Null);
    assert_eq!(folder(&s, "Deleted Items").0, 0);
}

#[test]
fn flags_categories_and_moves() {
    let mut s = session();
    run(&mut s, "mail.select", json!({"index": 1}));
    run(&mut s, "mail.flag", Value::Null);
    run(&mut s, "mail.categorize", json!({"category": "Green category"}));
    let row = &s.rows[1];
    assert_eq!((row.flag, row.categories.as_slice()), (openmailclient_model::Flag::Flagged, ["Green category".to_string()].as_slice()));
    run(&mut s, "mail.categorize", json!({"category": "green CATEGORY"}));
    assert!(s.rows[1].categories.is_empty(), "choosing a category again removes it");
    run(&mut s, "mail.move", json!({"folder": "Inbox/Projects"}));
    assert_eq!(folder(&s, "Projects").0, 3);
    assert!(s.run("mail.move", &json!({"folder": "Nowhere"})).is_err());
}

#[test]
fn reply_reply_all_and_forward_follow_outlook() {
    let mut s = session();
    run(&mut s, "mail.select", json!({"index": 0}));
    run(&mut s, "mail.reply", Value::Null);
    let d = s.compose.clone().unwrap();
    assert_eq!(
        (d.kind, d.to.as_str(), d.cc.as_str(), d.subject.as_str()),
        ("Reply", "Priya Raman <priya.raman@example.org>", "", "RE: Launch checklist: three items left")
    );
    assert!(d.attachments.is_empty(), "a reply leaves attachments behind");
    let header = "\n\n-----Original Message-----\nFrom: Priya Raman <priya.raman@example.org>\nSent: Wednesday, October 7, 2026 2:05 PM\nTo: Alex Morgan\nCc: Tomás Lindqvist\nSubject: Launch checklist: three items left\n\nHi Alex,";
    assert!(d.body.starts_with(header), "{}", d.body);

    run(&mut s, "mail.replyAll", Value::Null);
    let d = s.compose.clone().unwrap();
    assert_eq!(
        (d.to.as_str(), d.cc.as_str()),
        ("Priya Raman <priya.raman@example.org>", "Tomás Lindqvist <tomas.lindqvist@example.org>"),
        "the user's own address is left out"
    );

    run(&mut s, "mail.forward", Value::Null);
    let d = s.compose.clone().unwrap();
    assert_eq!((d.to.as_str(), d.subject.as_str(), d.attachments.len()), ("", "FW: Launch checklist: three items left", 1));

    // "RE:" is not stacked.
    run(&mut s, "mail.select", json!({"index": 1}));
    run(&mut s, "mail.reply", Value::Null);
    assert_eq!(s.compose.as_ref().unwrap().subject, "RE: Pricing page wording");
}

#[test]
fn send_files_in_the_outbox_and_never_leaves_the_machine() {
    let mut s = session();
    run(&mut s, "mail.new", Value::Null);
    assert!(s.run("compose.send", &Value::Null).is_err(), "a message with no recipient is not sent");
    run(&mut s, "compose.setField", json!({"field": "to", "value": "June Okafor <june.okafor@example.org>; tomas.lindqvist@example.org"}));
    run(&mut s, "compose.setField", json!({"field": "cc", "value": "helena.brandt@example.org"}));
    run(&mut s, "compose.setField", json!({"field": "subject", "value": "Rota\r\nlooks good"}));
    run(&mut s, "compose.setField", json!({"field": "body", "value": "Thanks, June.\nAlex"}));
    run(&mut s, "compose.send", Value::Null);
    assert!(s.compose.is_none());
    assert_eq!(folder(&s, "Outbox").0, 1);
    run(&mut s, "folder.select", json!({"folder": "Outbox"}));
    run(&mut s, "mail.select", json!({"index": 0}));
    let it = s.open.clone().unwrap();
    assert_eq!(it.subject(), "Rota looks good");
    assert_eq!(it.display_recipients(RECIP_TO), "June Okafor; tomas.lindqvist@example.org");
    assert_eq!(it.display_recipients(RECIP_CC), "helena.brandt@example.org");
    assert_eq!((it.sender_email().as_str(), it.is_unsent(), it.body_text().as_str()), ("alex.morgan@example.org", false, "Thanks, June.\nAlex"));

    run(&mut s, "mail.new", Value::Null);
    run(&mut s, "compose.setField", json!({"field": "subject", "value": "Later"}));
    run(&mut s, "compose.saveDraft", Value::Null);
    assert_eq!(folder(&s, "Drafts").0, 2);
}

#[test]
fn a_message_is_looked_over_before_it_goes() {
    use crate::outgoing::{Caution, cautions, sendable};
    for (address, want) in [
        ("june.okafor@example.org", true),
        ("a@b.co", true),
        ("june@localhost", false),
        ("june", false),
        ("@example.org", false),
        ("june@", false),
        ("june@@example.org", false),
        ("a@b@example.org", false),
        ("june okafor@example.org", false),
        ("june@example..org", false),
        ("june@.example.org", false),
        ("june@-example.org", false),
        ("", false),
    ] {
        assert_eq!(sendable(address), want, "{address:?}");
    }

    let draft = |to: &str, subject: &str, body: &str| Draft { to: to.into(), subject: subject.into(), body: body.into(), ..Draft::default() };
    assert!(cautions(&draft("june.okafor@example.org", "Rota", "Thanks.")).is_empty());
    assert_eq!(cautions(&draft("june.okafor@example.org", " ", "Thanks.")), [Caution::NoSubject]);
    assert_eq!(cautions(&draft("a@example.org", "Rota", "The rota is ATTACHED.")), [Caution::NothingAttached]);
    assert_eq!(cautions(&draft("a@example.org", "Rota", "See the attachment")), [Caution::NothingAttached]);
    // Only the new text is read, and only whole words.
    assert!(cautions(&draft("a@example.org", "Rota", "Thanks.\n\n-----Original Message-----\nI attached it")).is_empty());
    assert!(cautions(&draft("a@example.org", "Rota", "An unattached, reattaching sort of day")).is_empty());
    assert_eq!(
        cautions(&Draft {
            cc: "A@Example.org; b@example.org".into(),
            bcc: "a@example.org".into(),
            ..draft("a@example.org; june; b@example.org", "Rota", "")
        }),
        [Caution::NotAnAddress("june".into()), Caution::Twice("A@Example.org".into()), Caution::Twice("b@example.org".into())]
    );
    assert!(Caution::NotAnAddress("june".into()).words().contains("june"));

    // Sending says what it found and leaves the message open, until it is told to send anyway.
    let mut s = session();
    run(&mut s, "mail.new", Value::Null);
    run(&mut s, "compose.setField", json!({"field": "to", "value": "June Okafor <june.okafor@example.org>; JUNE.OKAFOR@example.org"}));
    run(&mut s, "compose.setField", json!({"field": "cc", "value": "june.okafor@example.org; helena.brandt@example.org"}));
    run(&mut s, "compose.setField", json!({"field": "body", "value": "Rota attached."}));
    let told = run(&mut s, "compose.check", Value::Null);
    assert_eq!(told["cautions"].as_array().unwrap().len(), 3, "{told}");
    assert_eq!(s.inspect()["composing"]["cautions"].as_array().unwrap().len(), 3);
    let refused = s.run("compose.send", &Value::Null).unwrap_err().to_string();
    assert!(refused.contains("no subject") && refused.contains("nothing is attached") && refused.contains("more than once"), "{refused}");
    assert!(s.compose.is_some(), "what was written is kept");
    assert_eq!(folder(&s, "Outbox").0, 0);
    run(&mut s, "compose.send", json!({"anyway": true}));
    assert_eq!(folder(&s, "Outbox").0, 1);

    // An answer to one's own message goes to those it went to, each of them once.
    run(&mut s, "folder.select", json!({"folder": "Outbox"}));
    run(&mut s, "mail.select", json!({"index": 0}));
    run(&mut s, "mail.reply", Value::Null);
    assert_eq!(s.compose.as_ref().unwrap().to, "June Okafor <june.okafor@example.org>");
    run(&mut s, "mail.replyAll", Value::Null);
    let d = s.compose.clone().unwrap();
    assert_eq!((d.to.as_str(), d.cc.as_str(), d.was_hidden), ("June Okafor <june.okafor@example.org>", "helena.brandt@example.org", false));

    // A hidden recipient who answers everyone is told so; one who answers the sender is not.
    let mut s = session();
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    s.user_email = "someone.else@example.org".to_string();
    run(&mut s, "mail.reply", Value::Null);
    assert!(!s.compose.as_ref().unwrap().was_hidden);
    run(&mut s, "mail.replyAll", Value::Null);
    let d = s.compose.clone().unwrap();
    assert!(d.was_hidden && cautions(&d).contains(&Caution::WasHidden));
    assert!(s.run("compose.send", &Value::Null).unwrap_err().to_string().contains("hidden"));

    // Hostile: whatever is in the boxes, looking it over does not panic and says little.
    for wild in ["@".repeat(5000), "a@b.c;".repeat(3000), "\u{202e}<\u{0}>,;\"".repeat(900), "\u{301}attach\u{301}".repeat(4000), "x".repeat(100_000)]
    {
        let found =
            cautions(&Draft { to: wild.clone(), cc: wild.clone(), bcc: wild.clone(), subject: wild.clone(), body: wild.clone(), ..Draft::default() });
        assert!(found.len() <= 12 && found.iter().all(|c| c.words().chars().count() <= 200), "{}", found.len());
    }
}

#[test]
fn what_was_done_to_a_message_can_be_taken_back() {
    let mut s = session();
    assert!(!s.is_enabled("edit.undo"));
    let state = |s: &Session| {
        let it = s.open.as_ref().unwrap();
        (it.subject(), it.is_read(), it.flag(), it.categories.clone(), it.props.i32(openmailclient_model::pid::IMPORTANCE))
    };
    run(&mut s, "mail.select", json!({"index": 1}));
    let (id, before) = (s.selected.unwrap(), state(&s));
    let counts = (folder(&s, "Inbox"), folder(&s, "Deleted Items"), folder(&s, "Archive"));

    // Changes to the message, taken back newest first.
    run(&mut s, "mail.flag", Value::Null);
    run(&mut s, "mail.categorize", json!({"category": "Launch"}));
    run(&mut s, "mail.importance", json!({"level": "high"}));
    run(&mut s, "mail.markUnread", Value::Null);
    assert_eq!(s.inspect()["undo"], "Mark as Unread");
    for (what, left) in [("Mark as Unread", 3), ("Importance", 2), ("Categorize", 1), ("Follow Up", 0)] {
        assert_eq!(run(&mut s, "edit.undo", Value::Null), json!({"undone": what, "left": left}));
    }
    assert_eq!((s.selected, state(&s)), (Some(id), before.clone()));
    assert!(!s.is_enabled("edit.undo") && s.inspect()["undo"].is_null());

    // A change, then two moves: all three come back, though the message's number changed twice.
    run(&mut s, "mail.flag", Value::Null);
    run(&mut s, "mail.archive", Value::Null);
    run(&mut s, "folder.select", json!({"folder": "Archive"}));
    let archived = s.rows.iter().find(|r| r.subject == before.0).unwrap().id;
    run(&mut s, "mail.select", json!({"id": archived}));
    run(&mut s, "mail.delete", Value::Null);
    run(&mut s, "folder.select", json!({"folder": "Drafts"}));
    assert_eq!(run(&mut s, "edit.undo", Value::Null)["undone"], "Delete");
    assert_eq!(
        (s.current_folder().unwrap().name.as_str(), s.open.as_ref().unwrap().subject()),
        ("Archive", before.0.clone()),
        "it is shown where it is again"
    );
    assert_eq!(run(&mut s, "edit.undo", Value::Null)["undone"], "Archive");
    assert_eq!(s.current_folder().unwrap().name, "Inbox");
    assert_eq!(run(&mut s, "edit.undo", Value::Null)["undone"], "Follow Up");
    assert_eq!(state(&s), before);
    assert_eq!((folder(&s, "Inbox"), folder(&s, "Deleted Items"), folder(&s, "Archive")), counts);

    // An unread message that comes back is still unread.
    run(&mut s, "view.filterUnread", json!({"on": false}));
    let unread = s.rows.iter().find(|r| !r.read).unwrap().id;
    run(&mut s, "mail.select", json!({"id": unread}));
    run(&mut s, "mail.markUnread", Value::Null);
    run(&mut s, "mail.junk", Value::Null);
    run(&mut s, "edit.undo", Value::Null);
    assert!(!s.open.as_ref().unwrap().is_read() && folder(&s, "Inbox") == counts.0);
    run(&mut s, "edit.undo", Value::Null);

    // Deleted for good, and taken back whole.
    run(&mut s, "folder.select", json!({"folder": "Deleted Items"}));
    run(&mut s, "mail.select", json!({"index": 0}));
    let whole = s.open.clone().unwrap();
    run(&mut s, "mail.delete", Value::Null);
    assert_eq!(folder(&s, "Deleted Items").0, counts.1.0 - 1);
    assert_eq!(run(&mut s, "edit.undo", Value::Null)["undone"], "Delete");
    let back = s.open.clone().unwrap();
    assert_eq!(
        (back.subject(), back.body_text(), back.attachments.len(), back.recipients.len()),
        (whole.subject(), whole.body_text(), whole.attachments.len(), whole.recipients.len())
    );
    assert_eq!(folder(&s, "Deleted Items"), counts.1);

    // A step whose message is gone says so, is dropped, and leaves the rest alone.
    run(&mut s, "folder.select", json!({"folder": "Inbox"}));
    run(&mut s, "mail.select", json!({"index": 0}));
    run(&mut s, "mail.flag", Value::Null);
    run(&mut s, "mail.delete", Value::Null);
    run(&mut s, "folder.select", json!({"folder": "Deleted Items"}));
    run(&mut s, "folder.empty", Value::Null);
    let refused = s.run("edit.undo", &Value::Null).unwrap_err().to_string();
    assert!(refused.contains("Delete can no longer be undone"), "{refused}");
    assert!(s.run("edit.undo", &Value::Null).unwrap_err().to_string().contains("Follow Up can no longer be undone"));
    assert!(!s.is_enabled("edit.undo"));

    // Only so many steps are kept, and none while a message is being written.
    run(&mut s, "folder.select", json!({"folder": "Inbox"}));
    run(&mut s, "mail.select", json!({"index": 0}));
    for _ in 0..crate::undo::MAX_STEPS + 20 {
        run(&mut s, "mail.flag", Value::Null);
    }
    assert_eq!(s.undo.len(), crate::undo::MAX_STEPS);
    run(&mut s, "mail.new", Value::Null);
    assert!(!s.is_enabled("edit.undo"));
    run(&mut s, "compose.discard", Value::Null);
    assert!(s.is_enabled("edit.undo"));
    run(&mut s, "file.closeStore", Value::Null);
    assert!(s.undo.is_empty(), "the steps of a closed data file go with it");
}

/// The sample mailbox, in which nothing can be put into one folder.
struct Refusing {
    inner: openmailclient_model::MemStore,
    full: Option<FolderId>,
}

impl openmailclient_model::Store for Refusing {
    fn display_name(&self) -> String {
        self.inner.display_name()
    }
    fn writable(&self) -> bool {
        true
    }
    fn folders(&mut self) -> Result<Vec<openmailclient_model::FolderInfo>, openmailclient_model::StoreError> {
        self.inner.folders()
    }
    fn list(&mut self, folder: FolderId) -> Result<Vec<openmailclient_model::Summary>, openmailclient_model::StoreError> {
        self.inner.list(folder)
    }
    fn load(&mut self, folder: FolderId, id: openmailclient_model::ItemId) -> Result<openmailclient_model::Item, openmailclient_model::StoreError> {
        self.inner.load(folder, id)
    }
    fn update(
        &mut self,
        folder: FolderId,
        id: openmailclient_model::ItemId,
        item: openmailclient_model::Item,
    ) -> Result<(), openmailclient_model::StoreError> {
        self.inner.update(folder, id, item)
    }
    fn add(&mut self, folder: FolderId, item: openmailclient_model::Item) -> Result<openmailclient_model::ItemId, openmailclient_model::StoreError> {
        if Some(folder) == self.full {
            return Err(openmailclient_model::StoreError::Format("the folder is full".into()));
        }
        self.inner.add(folder, item)
    }
    fn remove(&mut self, folder: FolderId, id: openmailclient_model::ItemId) -> Result<openmailclient_model::Item, openmailclient_model::StoreError> {
        self.inner.remove(folder, id)
    }
}

#[test]
fn a_message_that_cannot_be_put_where_it_is_going_stays_where_it_was() {
    let mut s = Session::new();
    s.now = now();
    let inner = crate::sample::sample_store(s.now);
    let full = inner.folder_by_kind(FolderKind::Archive);
    s.add_store(Box::new(Refusing { inner, full }));
    run(&mut s, "mail.select", json!({"index": 0}));
    let subject = s.open.as_ref().unwrap().subject();
    let before = (folder(&s, "Inbox"), folder(&s, "Archive"));
    assert!(s.run("mail.archive", &Value::Null).unwrap_err().to_string().contains("full"));
    assert_eq!((folder(&s, "Inbox"), folder(&s, "Archive")), before, "it is in neither folder twice, and in one of them once");
    assert!(s.rows.iter().any(|r| r.subject == subject) && s.undo.is_empty());

    // The same when taking a move back: what cannot return stays where it is.
    let mut s = Session::new();
    s.now = now();
    let inner = crate::sample::sample_store(s.now);
    let full = inner.folder_by_kind(FolderKind::Inbox);
    s.add_store(Box::new(Refusing { inner, full }));
    run(&mut s, "mail.select", json!({"index": 0}));
    run(&mut s, "mail.archive", Value::Null);
    let had = (folder(&s, "Inbox").0, folder(&s, "Archive").0);
    assert!(s.run("edit.undo", &Value::Null).unwrap_err().to_string().contains("can no longer be undone"));
    assert_eq!((folder(&s, "Inbox").0, folder(&s, "Archive").0), had);
}

#[test]
fn what_is_being_written_is_not_lost_and_files_are_not_replaced() {
    use crate::outgoing::{Caution, HEAVY, cautions};
    use openmailclient_model::Attachment;
    let mut s = session();
    assert!(!s.is_enabled("compose.restore"));

    // An untouched message leaves nothing behind; a touched one is set aside by whatever closes it.
    run(&mut s, "mail.new", Value::Null);
    run(&mut s, "compose.discard", Value::Null);
    assert!(s.set_aside.is_empty() && !s.is_enabled("edit.undo"));
    run(&mut s, "mail.new", Value::Null);
    run(&mut s, "compose.setField", json!({"field": "body", "value": "First thoughts"}));
    run(&mut s, "mail.select", json!({"index": 0}));
    run(&mut s, "mail.reply", Value::Null);
    assert_eq!((s.set_aside.len(), s.inspect()["setAside"].as_u64()), (1, Some(1)), "a reply does not write over a message in progress");
    run(&mut s, "compose.setField", json!({"field": "body", "value": "Second thoughts"}));
    run(&mut s, "compose.discard", Value::Null);
    assert!(s.compose.is_none() && s.status.contains("Undo"));
    assert_eq!(run(&mut s, "edit.undo", Value::Null)["undone"], "Discard");
    let d = s.compose.clone().unwrap();
    assert_eq!((d.kind, d.body.as_str(), d.subject.as_str()), ("Reply", "Second thoughts", "RE: Launch checklist: three items left"));
    // Bringing the other one back sets this one aside in its turn.
    assert_eq!(run(&mut s, "compose.restore", Value::Null)["left"], 1);
    assert_eq!(s.compose.as_ref().unwrap().body, "First thoughts");
    run(&mut s, "compose.restore", Value::Null);
    assert_eq!(s.compose.as_ref().unwrap().body, "Second thoughts");
    // A message that is sent or saved is not set aside as well.
    run(&mut s, "compose.saveDraft", Value::Null);
    assert_eq!(s.set_aside.len(), 1);
    // Only so many are kept.
    for n in 0..30 {
        run(&mut s, "mail.new", Value::Null);
        run(&mut s, "compose.setField", json!({"field": "subject", "value": format!("Note {n}")}));
    }
    run(&mut s, "compose.discard", Value::Null);
    assert_eq!(s.set_aside.len(), 20);
    run(&mut s, "compose.restore", Value::Null);
    assert_eq!(s.compose.as_ref().unwrap().subject, "Note 29");

    // Attaching: not a folder, not the same file twice, and it says what the message now carries.
    let dir = std::env::temp_dir().join(format!("openmailclient-m221-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("rota.txt"), b"Monday: June").unwrap();
    std::fs::write(dir.join("empty.txt"), b"").unwrap();
    std::fs::write(dir.join("setup.bat"), b"echo hello").unwrap();
    assert!(s.run("compose.attach", &json!({"path": dir})).unwrap_err().to_string().contains("folder"));
    let told = run(&mut s, "compose.attach", json!({"path": dir.join("rota.txt")}));
    assert_eq!((told["index"].as_u64(), told["size"].as_u64(), told["total"].as_u64()), (Some(0), Some(12), Some(12)));
    assert!(s.run("compose.attach", &json!({"path": dir.join("rota.txt")})).unwrap_err().to_string().contains("attached already"));
    run(&mut s, "compose.attach", json!({"path": dir.join("empty.txt")}));
    run(&mut s, "compose.attach", json!({"path": dir.join("setup.bat")}));
    let found = cautions(s.compose.as_ref().unwrap());
    assert!(found.contains(&Caution::Empty("empty.txt".into())) && found.contains(&Caution::Refused("setup.bat".into())), "{found:?}");
    let heavy = Draft {
        to: "a@example.org".into(),
        subject: "Big".into(),
        attachments: vec![Attachment::file("film.bin", "", vec![7; HEAVY + 1])],
        ..Draft::default()
    };
    let found = cautions(&heavy);
    assert!(matches!(found.as_slice(), [Caution::Heavy(_)]) && found[0].words().contains("20 MB"), "{found:?}");
    let many = Draft { attachments: (0..50).map(|n| Attachment::file(&format!("{n}.exe"), "", Vec::new())).collect(), ..heavy };
    assert!(cautions(&many).len() <= 6);
    run(&mut s, "compose.discard", Value::Null);

    // Saving a message into a folder: under its subject made safe, replacing nothing.
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    let first = run(&mut s, "file.saveAs", json!({"path": dir}))["untrusted_content"]["path"].as_str().unwrap().to_string();
    let second = run(&mut s, "file.saveAs", json!({"path": dir}))["untrusted_content"]["path"].as_str().unwrap().to_string();
    let eml = run(&mut s, "file.saveAs", json!({"path": dir, "format": "EML"}))["untrusted_content"]["path"].as_str().unwrap().to_string();
    assert!(
        first.ends_with("Launch checklist_ three items left.msg") && second.ends_with("Launch checklist_ three items left (2).msg"),
        "{first} {second}"
    );
    assert!(eml.ends_with("Launch checklist_ three items left.eml") && std::fs::metadata(&eml).unwrap().len() > 0);
    assert!(s.run("file.saveAs", &json!({"path": dir, "format": "exe"})).is_err());
    // A file that was saved opens again as the message it was.
    let stores = s.stores.len();
    run(&mut s, "file.open", json!({"path": first}));
    assert_eq!((s.stores.len(), s.open.as_ref().unwrap().subject().as_str()), (stores + 1, "Launch checklist: three items left"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sort_search_and_filter() {
    let mut s = session();
    run(&mut s, "view.sortBy", json!({"key": "from"}));
    let from: Vec<&str> = s.rows.iter().map(|r| r.from.as_str()).collect();
    assert_eq!((from[0], from[8]), ("Facilities Desk", "Travel Bookings"));
    run(&mut s, "view.sortBy", json!({"key": "from"}));
    assert_eq!(s.rows[0].from, "Travel Bookings", "arranging by the same column again reverses it");
    run(&mut s, "view.sortBy", json!({"key": "date"}));

    run(&mut s, "search.query", json!({"text": "ROTA"}));
    assert_eq!(s.rows.iter().map(|r| r.subject.as_str()).collect::<Vec<_>>(), ["Launch checklist: three items left", "Support rota draft"]);
    run(&mut s, "search.clear", Value::Null);
    run(&mut s, "view.filterUnread", Value::Null);
    assert_eq!(s.rows.len(), 3);
    run(&mut s, "view.filterUnread", json!({"on": false}));
    assert_eq!(s.rows.len(), 9);
}

#[test]
fn rows_are_arranged_as_a_reader_orders_them_and_stand_in_groups() {
    use crate::SortKey;
    use crate::arrange::{groups, natural, sort};
    use openmailclient_model::{Flag, Summary, time};
    use std::cmp::Ordering::{Equal, Greater, Less};
    for (a, b, want) in [
        ("invoice 9", "invoice 10", Less),
        ("invoice 10", "invoice 9", Greater),
        ("invoice 010", "invoice 10", Equal),
        ("a2b", "a10", Less),
        ("", "a", Less),
        ("a", "", Greater),
        ("abc", "abc", Equal),
        ("1", "a", Less),
        ("99999999999999999999999999999998", "99999999999999999999999999999999", Less),
        ("000", "0", Equal),
    ] {
        assert_eq!(natural(a, b), want, "{a:?} {b:?}");
    }

    let now = time::from_civil(2026, 10, 11, 12, 0, 0, 0);
    let at = |y, m, d| time::from_civil(y, m, d, 9, 0, 0, 0);
    let row = |id: u64, from: &str, subject: &str, time: i64| Summary {
        id,
        from: from.into(),
        subject: subject.into(),
        time,
        read: true,
        importance: 1,
        ..Summary::default()
    };
    let rows = vec![
        row(1, "Zoe Adams", "Invoice 10", at(2026, 10, 11)),
        row(2, "\u{c9}lodie Martin", "RE: invoice 9", at(2026, 10, 10)),
        Summary { read: false, size: 2 << 20, ..row(3, "elodie martin", "Invoice 9", at(2026, 10, 7)) },
        Summary {
            has_attachments: true,
            flag: Flag::Flagged,
            importance: 2,
            size: 50_000,
            ..row(4, "Aaron Bell", "FW: RE: Agenda", at(2026, 10, 2))
        },
        Summary { read: false, ..row(5, "aaron bell", "agenda", at(2026, 9, 20)) },
        Summary { flag: Flag::Complete, importance: 0, ..row(6, "", "", at(2025, 12, 24)) },
        row(7, "Zoe Adams", "Invoice 100", at(2025, 3, 1)),
        row(8, "Bo", "\u{c5}ngstr\u{f6}m notes", 0),
    ];
    let ids = |key, descending| {
        let mut sorted = rows.clone();
        sort(&mut sorted, key, descending);
        sorted.iter().map(|r| r.id).collect::<Vec<_>>()
    };
    // Names without their case and accents; those that are equal, newest first both ways.
    assert_eq!(ids(SortKey::From, false), [6, 4, 5, 8, 2, 3, 1, 7]);
    assert_eq!(ids(SortKey::From, true), [1, 7, 2, 3, 8, 4, 5, 6]);
    // Subjects without their prefixes, numbers by their value.
    assert_eq!(ids(SortKey::Subject, false), [6, 4, 5, 8, 2, 3, 1, 7]);
    assert_eq!(ids(SortKey::Subject, true), [7, 1, 2, 3, 8, 4, 5, 6]);
    assert_eq!(ids(SortKey::Date, true), [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(ids(SortKey::Date, false), [8, 7, 6, 5, 4, 3, 2, 1]);
    assert_eq!(ids(SortKey::Size, true), [3, 4, 1, 2, 5, 6, 7, 8]);
    assert_eq!(ids(SortKey::Importance, true), [4, 1, 2, 3, 5, 7, 8, 6]);
    assert_eq!(ids(SortKey::Flag, true), [4, 6, 1, 2, 3, 5, 7, 8]);
    assert_eq!(ids(SortKey::Attachments, true), [4, 1, 2, 3, 5, 6, 7, 8]);

    let told = |key, descending| {
        let mut sorted = rows.clone();
        sort(&mut sorted, key, descending);
        groups(&sorted, key, now, &openmailclient_model::zone::Zone::utc(), 0)
            .into_iter()
            .map(|g| (g.label, g.start, g.count, g.unread))
            .collect::<Vec<_>>()
    };
    let g = |label: &str, start, count, unread| (label.to_string(), start, count, unread);
    assert_eq!(
        told(SortKey::Date, true),
        [
            g("Today", 0, 1, 0),
            g("Yesterday", 1, 1, 0),
            g("Last Week", 2, 1, 1),
            g("Two Weeks Ago", 3, 1, 0),
            g("Three Weeks Ago", 4, 1, 1),
            g("Older", 5, 2, 0),
            g("No date", 7, 1, 0)
        ]
    );
    // As Outlook heads them where weeks begin on a Monday: Sunday the 11th ends the week, so the
    // Wednesday before is this week, by its name.
    let mut sorted = rows.clone();
    sort(&mut sorted, SortKey::Date, true);
    let labels = |now, start| {
        groups(&sorted, SortKey::Date, now, &openmailclient_model::zone::Zone::utc(), start).into_iter().map(|g| g.label).collect::<Vec<_>>()
    };
    assert_eq!(labels(now, 1), ["Today", "Yesterday", "Wednesday", "Last Week", "Three Weeks Ago", "Older", "No date"]);
    // Seen from November: the month before, and those before it.
    let november = time::from_civil(2026, 11, 28, 12, 0, 0, 0);
    assert_eq!(labels(november, 0), ["Last Month", "Older", "No date"]);
    // A day still to come is told forward.
    let ahead = |y, m, d| groups(&[row(1, "a", "b", at(y, m, d))], SortKey::Date, now, &openmailclient_model::zone::Zone::utc(), 0)[0].label.clone();
    assert_eq!(
        [
            ahead(2026, 10, 12),
            ahead(2026, 10, 14),
            ahead(2026, 10, 19),
            ahead(2026, 10, 26),
            ahead(2026, 11, 2),
            ahead(2026, 11, 20),
            ahead(2027, 1, 1)
        ],
        ["Tomorrow", "Wednesday", "Next Week", "Two Weeks Away", "Three Weeks Away", "Next Month", "Beyond Next Month"]
    );
    assert_eq!(
        told(SortKey::From, false),
        [g("No sender", 0, 1, 0), g("Aaron Bell", 1, 2, 1), g("Bo", 3, 1, 0), g("\u{c9}lodie Martin", 4, 2, 1), g("Zoe Adams", 6, 2, 0)]
    );
    assert_eq!(
        told(SortKey::Subject, false),
        [
            g("No subject", 0, 1, 0),
            g("Agenda", 1, 2, 1),
            g("\u{c5}ngstr\u{f6}m notes", 3, 1, 0),
            g("invoice 9", 4, 2, 1),
            g("Invoice 10", 6, 1, 0),
            g("Invoice 100", 7, 1, 0)
        ]
    );
    assert_eq!(told(SortKey::Size, true), [g("1 MB to 10 MB", 0, 1, 1), g("10 KB to 100 KB", 1, 1, 0), g("Under 10 KB", 2, 6, 1)]);
    assert_eq!(told(SortKey::Importance, true), [g("High importance", 0, 1, 0), g("Normal importance", 1, 6, 2), g("Low importance", 7, 1, 0)]);
    assert_eq!(told(SortKey::Flag, true), [g("Flagged", 0, 1, 0), g("Completed", 1, 1, 0), g("Not flagged", 2, 6, 2)]);
    assert_eq!(told(SortKey::Attachments, true), [g("With attachments", 0, 1, 0), g("No attachments", 1, 7, 2)]);
    // Earlier in the month than the last two weeks.
    let late = time::from_civil(2026, 10, 30, 12, 0, 0, 0);
    assert_eq!(
        groups(&[row(1, "a", "b", at(2026, 10, 2))], SortKey::Date, late, &openmailclient_model::zone::Zone::utc(), 0)[0].label,
        "Earlier This Month"
    );

    // Through the session: the groups are told with the rows, and their headings are marked as
    // what the messages say.
    let mut s = session();
    assert_eq!(s.groups().iter().map(|g| g.count).sum::<usize>(), s.rows.len());
    run(&mut s, "view.sortBy", json!({"key": "from"}));
    let told = s.inspect();
    let first = &told["groups"][0];
    assert_eq!((first["start"].as_u64(), first["untrusted_content"]["label"].as_str()), (Some(0), Some("Facilities Desk")));
    assert_eq!(told["groups"].as_array().unwrap().iter().map(|g| g["count"].as_u64().unwrap()).sum::<u64>(), 9);

    // Hostile: nothing here panics, whatever the rows hold.
    let wild = |n: u64| Summary {
        id: n,
        from: ["\u{0}", "9".repeat(5000).as_str(), "\u{202e}", "", "\u{301}"][n as usize % 5].to_string(),
        subject: "re: ".repeat(n as usize % 7 * 500),
        time: [i64::MIN, i64::MAX, 0, -1, 1][n as usize % 5],
        size: [0, u32::MAX][n as usize % 2],
        importance: [i32::MIN, i32::MAX, 0][n as usize % 3],
        ..Summary::default()
    };
    let mut many: Vec<Summary> = (0..200).map(wild).collect();
    for key in [SortKey::Date, SortKey::From, SortKey::To, SortKey::Subject, SortKey::Size, SortKey::Importance, SortKey::Flag, SortKey::Attachments]
    {
        for descending in [false, true] {
            sort(&mut many, key, descending);
            for (now, tz) in [(i64::MIN, i32::MIN), (i64::MAX, i32::MAX), (0, 0)] {
                let found = groups(&many, key, now, &openmailclient_model::zone::Zone::fixed(tz), u32::MAX);
                assert_eq!(found.iter().map(|g| g.count).sum::<usize>(), 200);
                assert!(found.iter().all(|g| g.label.chars().count() <= 80));
            }
        }
    }
    assert!(groups(&[], SortKey::Date, now, &openmailclient_model::zone::Zone::utc(), 0).is_empty());
}

#[test]
fn search_understands_fields_states_dates_and_sizes() {
    use crate::search::{fold, parse};
    use openmailclient_model::{Flag, Summary, time};
    assert_eq!(fold("Zöe ÅNGSTRÖM, Tomás; cafe\u{301} Ærø Łódź ß"), "zoe angstrom, tomas; cafe ærø łodz ß");

    let at = |y, m, d, h| time::from_civil(y, m, d, h, 0, 0, 120);
    let row = |subject: &str, from: &str, to: &str, time: i64| Summary {
        subject: subject.into(),
        from: from.into(),
        to: to.into(),
        time,
        read: true,
        importance: 1,
        ..Summary::default()
    };
    let rows = [
        Summary {
            has_attachments: true,
            size: 3 << 20,
            categories: vec!["Red category".into()],
            ..row("Launch checklist", "Priya Raman", "Alex Morgan", at(2026, 10, 5, 9))
        },
        Summary {
            read: false,
            flag: Flag::Flagged,
            size: 900,
            ..row("RE: Pricing page wording", "Tomás Okafor", "Alex Morgan; Zöe Ångström", at(2026, 10, 4, 23))
        },
        Summary {
            importance: 2,
            flag: Flag::Complete,
            preview: "The lift is out until noon: use the stairs".into(),
            size: 2048,
            ..row("Lift maintenance", "Facilities Desk", "Everyone", at(2026, 9, 30, 8))
        },
        Summary { importance: 0, ..row("from: a subject with a colon, 10:30", "June Park", "Alex Morgan", 0) },
    ];
    // Dates are the user's days: here two hours east of UTC.
    let find = |text: &str| -> Vec<usize> {
        let query = parse(text, &openmailclient_model::zone::Zone::fixed(120));
        rows.iter().enumerate().filter(|(_, r)| query.matches(r)).map(|(n, _)| n).collect()
    };
    let all = vec![0, 1, 2, 3];
    for (text, want) in [
        ("", all.clone()),
        ("   ", all.clone()),
        // Words: all of them, in any field, in either case, with or without accents.
        ("alex", vec![0, 1, 3]),
        ("ALEX pricing", vec![1]),
        ("tomas", vec![1]),
        ("Tomás", vec![1]),
        ("zoe angstrom", vec![1]),
        ("stairs", vec![2]),
        ("red", vec![0]),
        ("\"page wording\"", vec![1]),
        ("\"wording page\"", vec![]),
        ("page wording", vec![1]),
        // Leaving out, and either side.
        ("alex -pricing", vec![0, 3]),
        ("alex NOT pricing", vec![0, 3]),
        ("lift OR launch", vec![0, 2]),
        ("lift OR", vec![2]),
        ("OR", all.clone()),
        ("alex AND pricing", vec![1]),
        ("\"OR\"", vec![0, 1, 3]),
        ("or", vec![0, 1, 3]),
        ("-", vec![]),
        // Fields.
        ("from:priya", vec![0]),
        ("from:\"facilities desk\"", vec![2]),
        ("to:alex", vec![0, 1, 3]),
        ("to:alex -from:june", vec![0, 1]),
        ("subject:lift", vec![2]),
        ("subject:stairs", vec![]),
        ("category:red", vec![0]),
        ("-category:red", vec![1, 2, 3]),
        // What a message is.
        ("has:attachment", vec![0]),
        ("hasattachments:yes", vec![0]),
        ("hasattachments:no", vec![1, 2, 3]),
        ("is:unread", vec![1]),
        ("read:no", vec![1]),
        ("is:read", vec![0, 2, 3]),
        ("is:flagged", vec![1]),
        ("isflagged:yes", vec![1]),
        ("is:complete", vec![2]),
        ("is:important", vec![2]),
        ("importance:high", vec![2]),
        ("importance:low", vec![3]),
        ("IS:Unread", vec![1]),
        // Days, months and years; a message with no time is on none of them.
        ("on:2026-10-05", vec![0]),
        ("received:2026-10-04", vec![1]),
        ("on:2026-10", vec![0, 1]),
        ("on:2026", vec![0, 1, 2]),
        ("before:2026-10-05", vec![1, 2]),
        ("after:2026-10-04", vec![0]),
        ("after:2026-09", vec![0, 1]),
        ("before:2026", vec![]),
        ("after:2026-12", vec![]),
        ("-on:2026", vec![3]),
        // Sizes.
        ("size:>2mb", vec![0]),
        ("size:<1kb", vec![1, 3]),
        ("size:1kb", vec![0, 2]),
        ("size:>=2048", vec![0]),
        // What is not understood is looked for as it stands: nothing is found, not everything.
        ("colour:blue", vec![]),
        ("is:purple", vec![]),
        ("on:someday", vec![]),
        ("on:2026-13", vec![]),
        ("on:2026-10-32", vec![]),
        ("size:big", vec![]),
        ("size:>99999999999999999999mb", vec![]),
        ("from:", vec![3]),
        ("10:30", vec![3]),
        ("\"from: a subject\"", vec![3]),
    ] {
        assert_eq!(find(text), want, "{text:?}");
    }

    // Through the command, on the sample mailbox.
    let mut s = session();
    let found = run(&mut s, "search.query", json!({"text": "from:priya has:attachment"}));
    assert_eq!(found, json!({"count": 1}));
    assert_eq!((s.rows[0].subject.as_str(), s.status.as_str()), ("Launch checklist: three items left", "1 item found"));
    assert_eq!(run(&mut s, "search.query", json!({"text": "from:tomas OR from:helena"}))["count"], json!(s.rows.len()));
    assert!(s.rows.len() >= 2 && s.rows.iter().all(|r| r.from.starts_with("Tomás") || r.from.starts_with("Helena")));
    run(&mut s, "search.query", json!({"text": "no such words anywhere"}));
    assert_eq!((s.rows.len(), s.status.as_str()), (0, "Nothing found"));
    run(&mut s, "search.clear", Value::Null);
    assert_eq!(s.rows.len(), 9);
}

#[test]
fn hostile_queries_never_panic() {
    use crate::search::parse;
    let row = openmailclient_model::Summary { subject: "é\u{301}\u{0}".into(), time: i64::MAX, size: u32::MAX, ..Default::default() };
    let pieces = [
        "\"",
        "-",
        ":",
        "--",
        "OR",
        "NOT",
        "AND",
        "on:",
        "9999-12-31",
        "0000",
        "size:",
        ">",
        "<=",
        "é",
        "\u{0}",
        "\u{202e}",
        "\u{17f}",
        "\u{c0}",
        "\u{180}",
        "-\"",
        "a:b:c",
        "from:\"",
        "1601",
        "99999999999999999999",
    ];
    for a in pieces {
        for b in pieces {
            for text in [
                format!("{a}{b}"),
                format!("{a} {b}"),
                format!("on:{a}{b}"),
                format!("size:{a}{b}"),
                format!("-{a}:{b}"),
                format!("before:{a}-{b} after:{b}-{a}"),
            ] {
                for tz in [0, i32::MIN, i32::MAX] {
                    let _ = parse(&text, &openmailclient_model::zone::Zone::fixed(tz)).matches(&row);
                }
            }
        }
    }
    // Far more clauses than are kept: those past the cap are left out, and it still answers.
    let long = "word ".repeat(10_000);
    assert!(
        parse(&long, &openmailclient_model::zone::Zone::utc())
            .matches(&openmailclient_model::Summary { subject: "a word".into(), ..Default::default() })
    );
    let _ = parse(&"\"".repeat(100_001), &openmailclient_model::zone::Zone::utc());
    let _ = parse(&"OR ".repeat(50_000), &openmailclient_model::zone::Zone::utc());
}

#[test]
fn folders_can_be_made_renamed_and_deleted_but_not_the_inbox() {
    let mut s = session();
    assert!(!s.is_enabled("folder.rename") && !s.is_enabled("folder.delete"), "the Inbox is not the user's to rename or delete");
    let id = run(&mut s, "folder.new", json!({"name": "Clients"}))["folderId"].as_u64().unwrap();
    assert!(s.run("folder.new", &json!({"name": "clients"})).is_err(), "names are unique inside a folder");
    run(&mut s, "folder.select", json!({"folderId": id}));
    run(&mut s, "folder.rename", json!({"name": "Customers"}));
    assert_eq!(s.current_folder().unwrap().name, "Customers");
    run(&mut s, "folder.delete", json!({"permanently": true}));
    assert_eq!(s.current_folder().unwrap().name, "Inbox");
    assert!(s.folders[0].iter().all(|f| f.name != "Customers"));
}

#[test]
fn a_deleted_folder_can_be_taken_back_and_folders_say_what_they_hold() {
    let mut s = session();
    let names = |s: &Session, parent: &str| {
        let parent = s.folder_by_path(0, parent);
        s.folders[0].iter().filter(|f| f.parent == parent).map(|f| f.name.clone()).collect::<Vec<_>>()
    };
    let told = run(&mut s, "folder.properties", Value::Null);
    assert_eq!((told["total"].as_u64(), told["subfolders"].as_u64(), told["untrusted_content"]["path"].as_str()), (Some(9), Some(2), Some("Inbox")));
    let with = told["withSubfolders"]["total"].as_u64().unwrap();
    assert!(
        with > 9 && told["withSubfolders"]["size"].as_u64() > told["size"].as_u64() && told["oldest"].as_i64() < told["newest"].as_i64(),
        "{told}"
    );

    // Names are one name whatever the case of their letters, in a rename as in a new folder.
    run(&mut s, "folder.select", json!({"folder": "Inbox/Projects"}));
    assert!(s.run("folder.rename", &json!({"name": "RECEIPTS"})).is_err());
    run(&mut s, "folder.rename", json!({"name": "projects"}));
    run(&mut s, "folder.rename", json!({"name": "Projects"}));
    let held = run(&mut s, "folder.properties", Value::Null)["total"].as_u64().unwrap();
    assert!(held > 0);

    // Moving: not into itself, not onto a name that is taken, and with all it holds.
    let inner = run(&mut s, "folder.new", json!({"name": "Old"}))["folderId"].as_u64().unwrap();
    assert!(s.run("folder.move", &json!({"toId": inner})).is_err(), "not into a folder inside it");
    assert!(s.run("folder.move", &json!({"to": "Inbox/Projects"})).is_err(), "not into itself");
    assert!(s.run("folder.move", &json!({"to": "Nowhere"})).is_err() && s.run("folder.move", &Value::Null).is_err());
    run(&mut s, "folder.move", json!({"to": "Archive"}));
    assert_eq!(run(&mut s, "folder.properties", Value::Null)["untrusted_content"]["path"], "Archive/Projects");
    assert_eq!(names(&s, "Inbox"), ["Receipts"]);
    assert_eq!(s.folder_by_path(0, "Archive/Projects/Old"), Some(FolderId(inner)));
    let order: Vec<_> = s.folders[0].iter().map(|f| f.id).collect();
    assert!(
        s.folders[0].iter().all(|f| f.parent.is_none_or(|p| order.iter().position(|id| *id == p) < order.iter().position(|id| *id == f.id))),
        "parents first"
    );
    run(&mut s, "folder.move", json!({"to": "Inbox"}));

    // Deleting puts the folder with the deleted mail, whole, and says what went with it.
    let went = run(&mut s, "folder.delete", Value::Null);
    assert_eq!((went["permanently"].as_bool(), went["messages"].as_u64(), went["folders"].as_u64()), (Some(false), Some(held), Some(2)));
    assert_eq!(s.current_folder().unwrap().name, "Inbox");
    assert_eq!(s.folder_by_path(0, "Deleted Items/Projects/Old"), Some(FolderId(inner)));
    // A second folder of the same name gets a number there, not an error.
    run(&mut s, "folder.new", json!({"name": "Projects"}));
    run(&mut s, "folder.select", json!({"folder": "Inbox/Projects"}));
    run(&mut s, "folder.delete", Value::Null);
    assert_eq!(names(&s, "Deleted Items"), ["Projects", "Projects (2)"]);
    // Taken back with what it held.
    run(&mut s, "folder.select", json!({"folder": "Deleted Items/Projects"}));
    run(&mut s, "folder.move", json!({"to": "Inbox"}));
    assert_eq!(run(&mut s, "folder.properties", Value::Null)["total"].as_u64(), Some(held));

    // Marking a folder read with the folders inside it.
    run(&mut s, "folder.select", json!({"folder": "Inbox"}));
    let unread = run(&mut s, "folder.properties", Value::Null)["withSubfolders"]["unread"].as_u64().unwrap();
    assert_eq!(run(&mut s, "folder.markAllRead", json!({"subfolders": true}))["marked"].as_u64(), Some(unread));
    assert_eq!(run(&mut s, "folder.properties", Value::Null)["withSubfolders"]["unread"].as_u64(), Some(0));

    // A folder among the deleted mail goes for good; emptying takes the folders with the mail.
    run(&mut s, "folder.select", json!({"folder": "Inbox/Projects"}));
    run(&mut s, "folder.delete", Value::Null);
    run(&mut s, "folder.select", json!({"folder": "Deleted Items/Projects (2)"}));
    assert_eq!(run(&mut s, "folder.delete", Value::Null)["permanently"], true);
    assert_eq!(names(&s, "Deleted Items"), ["Projects"]);
    assert_eq!(s.current_folder().unwrap().name, "Deleted Items");
    let before = folder(&s, "Deleted Items").0 as u64;
    assert!(s.run("folder.empty", &json!({"olderThanDays": "soon"})).is_err());
    // Only what is older than asked goes, and the folders stay.
    let aged = run(&mut s, "folder.empty", json!({"olderThanDays": 100_000}));
    assert_eq!((aged["removed"].as_u64(), aged["kept"].as_u64(), aged["folders"].as_u64()), (Some(0), Some(before), Some(0)));
    let aged = run(&mut s, "folder.empty", json!({"olderThanDays": u64::MAX}));
    assert_eq!(aged["removed"].as_u64(), Some(0));
    let gone = run(&mut s, "folder.empty", Value::Null);
    assert_eq!((gone["removed"].as_u64(), gone["folders"].as_u64(), gone["kept"].as_u64()), (Some(before + held), Some(2), Some(0)));
    assert!(names(&s, "Deleted Items").is_empty());
}

#[test]
fn an_attached_object_is_shown_by_the_best_picture_it_has() {
    use openmailclient_formats::rich::{Block, Picture};
    use openmailclient_model::{Attachment, Item, PropValue, pid, rtf};
    use std::io::Write;
    let mut dib = Vec::new();
    for n in [40u32, 2, 2] {
        dib.extend_from_slice(&n.to_le_bytes());
    }
    dib.extend_from_slice(&[1, 0, 24, 0]);
    dib.extend_from_slice(&[0; 24]);
    dib.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0]);
    let object = |kind: &str, bitmap: &[u8]| {
        let mut a = Attachment::file("Card", "", openmailclient_msg::ole::picture_object(kind, bitmap).unwrap());
        a.props.set(pid::ATTACH_METHOD, PropValue::Int32(6));
        a.props.set(pid::RENDERING_POSITION, PropValue::Int32(0));
        a
    };
    let show = |a: Attachment| {
        let mut holder = Item::default();
        holder.props.set(pid::RTF_COMPRESSED, PropValue::Binary(rtf::store(b"{\\rtf1\\ansi\\pard \\objattph\\'20\\par}")));
        let mut doc = openmailclient_formats::rtf::parse(&holder.body_rtf().unwrap());
        crate::pictures::settle(&mut doc, &Item { attachments: vec![a], ..Item::default() });
        let Some(Block::Para(para)) = doc.blocks.first() else { panic!("a paragraph") };
        let run = para.runs.iter().find(|r| r.picture.is_some() || r.text.contains("Card")).unwrap().clone();
        let file = match run.picture {
            Some(Picture::Inline(Some(n))) => Some(doc.written[n].to_vec()),
            _ => None,
        };
        (run.text, file)
    };
    // The picture the object keeps of itself.
    let (text, file) = show(object("Permit card", &dib));
    assert_eq!(text, "Card");
    let pixels = crate::pictures::decode(&file.unwrap()).unwrap();
    assert_eq!((pixels.width, pixels.height), (2, 2));
    // One that cannot be read: the attachment is named, with what the object calls itself.
    let (text, file) = show(object("Permit\u{7} card", &dib[..30]));
    assert_eq!((text.as_str(), file), ("[Attachment: Card (Permit card)]", None));
    // The object's own data, when that is a picture file, goes before what it keeps for showing:
    // here a bitmap file of one pixel beside a presentation of two by two.
    let mut bmp = b"BM".to_vec();
    bmp.extend_from_slice(&[58, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0]);
    for n in [40u32, 1, 1] {
        bmp.extend_from_slice(&n.to_le_bytes());
    }
    bmp.extend_from_slice(&[1, 0, 24, 0]);
    bmp.extend_from_slice(&[0; 24]);
    bmp.extend_from_slice(&[0, 0, 255, 0]);
    let mut a = object("Bitmap", &dib);
    let mut cf = cfb::CompoundFile::open(std::io::Cursor::new(std::mem::take(&mut a.data))).unwrap();
    let mut native = u32::try_from(bmp.len()).unwrap().to_le_bytes().to_vec();
    native.extend_from_slice(&bmp);
    cf.create_stream("/\u{1}Ole10Native").unwrap().write_all(&native).unwrap();
    cf.flush().unwrap();
    a.data = cf.into_inner().into_inner();
    let (_, file) = show(a);
    assert_eq!(file.as_deref(), Some(&bmp[..]));
    // Data that only claims to be a picture is passed over for the next one that reads.
    let mut a = object("Bitmap", &dib);
    let mut cf = cfb::CompoundFile::open(std::io::Cursor::new(std::mem::take(&mut a.data))).unwrap();
    cf.create_stream("/\u{1}Ole10Native").unwrap().write_all(&[4, 0, 0, 0, b'B', b'M', 1, 2]).unwrap();
    cf.flush().unwrap();
    a.data = cf.into_inner().into_inner();
    let pixels = crate::pictures::decode(&show(a).1.unwrap()).unwrap();
    assert_eq!((pixels.width, pixels.height), (2, 2));
}

#[test]
fn attachments_stand_in_their_places_in_an_rtf_body() {
    use openmailclient_formats::rich::{Block, Filed, Picture};
    use openmailclient_model::{Attachment, Item, PropValue, pid, rtf};
    // A bitmap of two by two pixels, the lower left one blue, drawn by a metafile (META_STRETCHDIB).
    let mut dib = Vec::new();
    for n in [40u32, 2, 2] {
        dib.extend_from_slice(&n.to_le_bytes());
    }
    dib.extend_from_slice(&[1, 0, 24, 0]);
    dib.extend_from_slice(&[0; 24]);
    dib.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0]);
    let mut wmf = vec![1, 0, 9, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    wmf.extend_from_slice(&u32::try_from((28 + dib.len()) / 2).unwrap().to_le_bytes());
    wmf.extend_from_slice(&[0x43, 0x0F, 0x20, 0, 0xCC, 0, 0, 0]);
    wmf.extend_from_slice(&[0; 16]);
    wmf.extend_from_slice(&dib);
    wmf.extend_from_slice(&[3, 0, 0, 0, 0, 0]);

    let at = |mut a: Attachment, place: i32| {
        a.props.set(pid::RENDERING_POSITION, PropValue::Int32(place));
        a
    };
    let mut chart = at(Attachment::file("chart", "", Vec::new()), 20);
    chart.props.set(pid::ATTACH_METHOD, PropValue::Int32(6));
    chart.props.set(pid::ATTACH_RENDERING, PropValue::Binary(wmf));
    let mut hidden = at(Attachment::file("hidden.png", "image/png", vec![1]), 1);
    hidden.props.set(pid::ATTACHMENT_HIDDEN, PropValue::Bool(true));
    let attachments = vec![
        at(Attachment::file("notes\u{7}.txt", "text/plain", b"notes".to_vec()), 500),
        chart,
        hidden,
        at(Attachment::file("unplaced.txt", "text/plain", vec![2]), -1),
        Attachment::file("from-mime.txt", "text/plain", vec![3]),
    ];
    let mut item = Item { attachments, ..Item::default() };
    let names: Vec<String> = crate::pictures::standing(&item).iter().map(|a| a.name()).collect();
    assert_eq!(names, ["chart", "notes\u{7}.txt"], "by rendering position; hidden ones and those with no place are left out");

    let read = |places: usize, item: &Item| {
        let body = format!("{{\\rtf1\\ansi\\pard See {}\\par}}", "\\objattph\\'20".repeat(places));
        let mut holder = Item::default();
        holder.props.set(pid::RTF_COMPRESSED, PropValue::Binary(rtf::store(body.as_bytes())));
        let mut doc = openmailclient_formats::rtf::parse(&holder.body_rtf().unwrap());
        crate::pictures::settle(&mut doc, item);
        doc
    };
    let doc = read(2, &item);
    let Some(Block::Para(para)) = doc.blocks.first() else { panic!("{doc:?}") };
    let stands: Vec<(Option<Picture>, &str)> =
        para.runs.iter().map(|r| (r.picture.clone(), r.text.as_str())).filter(|(p, t)| p.is_some() || t.contains('[')).collect();
    // A file of its own stands as a file: what it is saved as, its number in the message and its size.
    let notes = Filed { number: Some(0), name: "notes.txt".into(), kind: None, bytes: 5, caution: None };
    assert_eq!(stands, [(Some(Picture::Inline(Some(0))), "chart"), (Some(Picture::File(notes)), "[Attachment: notes.txt]")]);
    // The object's own picture is a picture file the reading pane and print can decode.
    let pixels = crate::pictures::decode(&doc.written[0]).unwrap();
    assert_eq!((pixels.width, pixels.height), (2, 2));
    assert_eq!(&pixels.rgba[8..12], &[0, 0, 255, 255]);

    // The body and the attachments do not agree: the places are given up, nothing is shown there.
    for places in [1, 3, 300] {
        let doc = read(places, &item);
        assert!(doc.written.is_empty());
        assert_eq!(doc.text().trim(), "See", "{places}");
        let Some(Block::Para(para)) = doc.blocks.first() else { panic!("{doc:?}") };
        assert!(para.runs.iter().all(|r| r.picture.is_none()));
    }
    // A rendering that is not a metafile of one bitmap: the object is named.
    item.attachments[1].props.set(pid::ATTACH_RENDERING, PropValue::Binary(vec![0xFF; 64]));
    let doc = read(2, &item);
    assert!(doc.text().contains("[Attachment: chart]"));
    // It is no file to save: it is named and no more.
    let Some(Block::Para(para)) = doc.blocks.first() else { panic!("{doc:?}") };
    let files: Vec<&Filed> = para.runs.iter().filter_map(|r| if let Some(Picture::File(f)) = &r.picture { Some(f) } else { None }).collect();
    assert_eq!(files.first().map(|f| (f.number, f.name.as_str())), Some((None, "chart")));

    // A file to think twice about says so where it stands, under the name it really has.
    item.attachments[0] = at(Attachment::file("invoice\u{202e}fdp.exe", "application/pdf", b"MZ".to_vec()), 500);
    let doc = read(2, &item);
    let Some(Block::Para(para)) = doc.blocks.first() else { panic!("{doc:?}") };
    let risky = para.runs.iter().find_map(|r| if let Some(Picture::File(f)) = &r.picture { f.number.map(|_| f) } else { None }).unwrap();
    assert_eq!(risky.name, "invoicefdp.exe");
    assert!(risky.caution.is_some(), "{risky:?}");
    assert!(doc.text().contains("[Attachment: invoicefdp.exe]"));
}

#[test]
fn a_message_prints_to_a_pdf_file() {
    let dir = std::env::temp_dir().join(format!("openmailclient-engine-print-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = session();
    // Nothing is open: there is nothing to print.
    assert!(!Session::new().is_enabled("file.print"));
    run(&mut s, "mail.select", json!({"index": 0}));
    // The target and the paper are checked before anything is laid out or written.
    let wrong = dir.join("print.msg");
    assert!(matches!(s.run("file.print", &json!({"path": wrong})), Err(EngineError::BadParams(_))));
    assert!(matches!(s.run("file.print", &json!({"path": dir.join("p.pdf"), "paper": "foolscap"})), Err(EngineError::BadParams(_))));
    assert!(!wrong.exists() && !dir.join("p.pdf").exists());

    // With the test font the pages are known; the installed fonts differ from machine to machine.
    let item = s.open.clone().unwrap();
    let library = openmailclient_fonts::Library::empty();
    assert!(matches!(
        crate::print::layout(
            &item,
            &crate::print::Job {
                owner: "Alex Morgan",
                zone: &openmailclient_model::zone::Zone::utc(),
                now: 0,
                setup: &openmailclient_layout::Setup::default(),
                links: crate::print::Links::All,
                listing: &crate::print::Listing::default(),
                region: &openmailclient_model::time::Region::default(),
            },
            &library
        )
        .map(|_| ()),
        Err(EngineError::Format(_))
    ));
    let printed = {
        let mut fonts = openmailclient_layout::Fonts::new(&library);
        let chars: String = (' '..='~').chain(['\u{e1}', '\u{2022}']).collect();
        fonts.give("Arial", false, false, openmailclient_fonts::Face { data: openmailclient_fonts::boxes::font(&chars), index: 0 });
        let fields = [("Subject", item.subject())];
        let body = openmailclient_formats::html::parse(&item.body_html().unwrap());
        openmailclient_layout::memo(
            &openmailclient_layout::Memo { owner: "Alex Morgan", fields: &fields, body: &body, plain: false, sizes: &|_| None },
            openmailclient_layout::Paper::A4,
            &mut fonts,
        )
        .unwrap()
    };
    let text = printed.pages[0].text();
    assert!(text.starts_with("Alex Morgan\nSubject:Launch checklist: three items left\nHi Alex,"), "{text}");
    assert!(text.contains("Support rota for the first week") && text.ends_with("\n1"));
    let file = crate::print::write(&printed, &crate::print::Pictures::default(), &item.subject()).unwrap();
    assert!(file.starts_with(b"%PDF-1.7") && file.ends_with(b"%%EOF\n"));

    // A picture the message carries is printed at the size the body asks for; one on a server
    // is named, and nothing is fetched for it.
    run(&mut s, "mail.select", json!({"index": 4}));
    let item = s.open.clone().unwrap();
    let body = openmailclient_formats::html::parse(&item.body_html().unwrap());
    let pictures = crate::print::Pictures::of(&body, &item);
    let printed = {
        let mut fonts = openmailclient_layout::Fonts::new(&library);
        fonts.give("Arial", false, false, openmailclient_fonts::Face { data: openmailclient_fonts::boxes::font("ab "), index: 0 });
        let sizes = |picture: &openmailclient_layout::Picture| pictures.get(picture).map(|p| p.natural);
        let memo = openmailclient_layout::Memo { owner: "a", fields: &[], body: &body, plain: false, sizes: &sizes };
        openmailclient_layout::memo(&memo, openmailclient_layout::Paper::A4, &mut fonts).unwrap()
    };
    let placed: Vec<_> = printed.pages.iter().flat_map(|p| &p.pictures).collect();
    assert_eq!(placed.len(), 1);
    assert_eq!((placed[0].width, placed[0].height), (180.0, 72.0));
    let file = crate::print::write(&printed, &pictures, "t").unwrap();
    let has = |needle: &[u8]| file.windows(needle.len()).any(|w| w == needle);
    assert!(has(b"/Subtype /Image /Width 240 /Height 96") && has(b"/XObject << /Im0 "));
    assert!(!crate::print::write(&printed, &crate::print::Pictures::default(), "t").unwrap().windows(15).any(|w| w == b"/Subtype /Image"));
    run(&mut s, "mail.select", json!({"index": 0}));

    // Through the command: a file, or the word that no font is installed. Never a half-written one.
    let path = dir.join("launch.pdf");
    match s.run("file.print", &json!({"path": path, "paper": "Letter"})) {
        Ok(result) => {
            assert_eq!(result["pages"], 1);
            assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF-1.7"));
        }
        Err(e) => {
            assert!(matches!(e, EngineError::Format(_)), "{e}");
            assert!(!path.exists());
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn print_lists_where_links_go() {
    use crate::print::{Links, note_links};
    use openmailclient_formats::rich::{Block, Doc};
    assert_eq!(
        (Links::named("ALL"), Links::named("concerns"), Links::named("none"), Links::named("some")),
        (Some(Links::All), Some(Links::Concerns), Some(Links::None), None)
    );
    fn text(blocks: &[Block]) -> Vec<String> {
        let mut out = Vec::new();
        for block in blocks {
            match block {
                Block::Para(para) => out.push(para.runs.iter().map(|r| r.text.as_str()).collect()),
                Block::Rule => out.push("--".to_string()),
                Block::Table(table) => out.extend(table.rows.iter().flatten().flat_map(|cell| text(&cell.blocks))),
            }
        }
        out
    }
    let mut s = session();
    run(&mut s, "mail.select", json!({"folder": "Junk Email", "index": 0}));
    let html = s.open.as_ref().unwrap().body_html().unwrap();

    // As Outlook prints it: the body alone.
    let mut doc = openmailclient_formats::html::parse(&html);
    let before = doc.clone();
    assert_eq!(note_links(&mut doc, Links::None), 0);
    assert_eq!(doc, before);

    // The four links that are not what they say are numbered where they stand and listed.
    let mut doc = openmailclient_formats::html::parse(&html);
    assert_eq!(note_links(&mut doc, Links::Concerns), 4);
    let lines = text(&doc.blocks);
    assert!(lines.contains(&"https://mail.example.org/quota[1]".to_string()), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("mail.example.org[2], or use the web version[3].")), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("helpdesk@example.org[4]. Our real help pages are at example.org/help.")), "{lines:?}");
    let at = lines.iter().position(|l| l == "--").unwrap();
    assert_eq!(
        lines[at + 1..],
        [
            "Links that are not what they say",
            "[1] \u{201c}https://mail.example.org/quota\u{201d} goes to https://mail.example.org.account-check.example.net/quota  The text names mail.example.org, but the link goes to mail.example.org.account-check.example.net.",
            "[2] \u{201c}mail.example.org\u{201d} goes to https://mail.example.org@sign-in.example.net/  The address is written to look like another place: it goes to sign-in.example.net.",
            "[3] \u{201c}the web version\u{201d} goes to http://192.0.2.44/owa  The link goes to a number, not a name: 192.0.2.44.",
            "[4] \u{201c}helpdesk@example.org\u{201d} goes to mailto:helpdesk@mailbox-notice.example.net  The text names example.org, but the link goes to mailbox-notice.example.net.",
        ]
    );

    // All of them: the honest one too, which only says where it goes.
    let mut doc = openmailclient_formats::html::parse(&html);
    assert_eq!(note_links(&mut doc, Links::All), 5);
    let lines = text(&doc.blocks);
    assert_eq!(lines[lines.len() - 6], "Links in this message");
    assert_eq!(lines[lines.len() - 1], "[5] \u{201c}example.org/help\u{201d} goes to https://example.org/help");

    // A link written out as its own address, one inside the message and a picture of the message
    // need no line; the same link twice has one number; links in tables are found.
    let mut doc = openmailclient_formats::html::parse(
        r##"<p><a href="https://example.org/a">https://example.org/a</a> <a href="#top">top</a> <a href="cid:x@y">x</a>
        <a href="https://example.org/b">the <b>plan</b></a> and again <a href="https://example.org/b">the plan</a></p>
        <table><tr><td><a href="https://example.org/c">in a table</a></td></tr></table>"##,
    );
    assert_eq!(note_links(&mut doc, Links::All), 2);
    let lines = text(&doc.blocks);
    assert!(lines[0].contains("https://example.org/a #top") || lines[0].contains("https://example.org/a top"), "{lines:?}");
    assert!(lines[0].contains("the plan[1] and again the plan[1]"), "{lines:?}");
    assert!(lines.contains(&"in a table[2]".to_string()), "{lines:?}");

    // Hostile: far more links than are listed, with addresses far longer than are shown.
    let many: String = (0..1000).map(|n| format!(r#"<a href="https://example.org/{n}/{}">link {n}</a> "#, "x".repeat(1000))).collect();
    let mut doc = openmailclient_formats::html::parse(&many);
    assert_eq!(note_links(&mut doc, Links::All), 200);
    let lines = text(&doc.blocks);
    assert!(lines.last().unwrap().starts_with("[200] ") && lines.last().unwrap().chars().count() < 420);
    assert_eq!(note_links(&mut Doc::default(), Links::All), 0);

    // Through the command.
    let dir = std::env::temp_dir().join(format!("openmailclient-engine-print-links-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(matches!(s.run("file.print", &json!({"path": dir.join("p.pdf"), "links": "some"})), Err(EngineError::BadParams(_))));
    for (links, count) in [(Value::Null, 4), (json!("all"), 5), (json!("none"), 0)] {
        let mut params = json!({"path": dir.join("lure.pdf")});
        if !links.is_null() {
            params["links"] = links;
        }
        match s.run("file.print", &params) {
            Ok(result) => assert_eq!(result["links"], count),
            Err(e) => assert!(matches!(e, EngineError::Format(_)), "{e}"),
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn messages_save_and_reopen_as_msg_and_eml() {
    let dir = std::env::temp_dir().join(format!("openmailclient-engine-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = session();
    run(&mut s, "mail.select", json!({"index": 0}));
    for ext in ["msg", "eml", "oft"] {
        let path = dir.join(format!("launch.{ext}"));
        run(&mut s, "file.saveAs", json!({"path": path}));
        let mut other = Session::new();
        let opened = run(&mut other, "file.open", json!({"path": path}));
        assert_eq!(opened["store"], 0);
        let it = other.open.clone().unwrap();
        assert_eq!(it.subject(), "Launch checklist: three items left", "{ext}");
        assert_eq!(it.sender_name(), "Priya Raman", "{ext}");
        assert!(it.body_text().contains("Support rota for the first week"), "{ext}");
        assert_eq!(it.attachments.len(), 1, "{ext}");
        assert_eq!(it.attachments[0].name(), "launch-timeline.csv", "{ext}");
        assert!(!other.stores[0].display_name().is_empty());
    }
    // A refused save leaves the existing file as it was.
    let keep = dir.join("keep.pst");
    std::fs::write(&keep, b"original").unwrap();
    assert!(s.run("file.saveAs", &json!({"path": keep})).is_err());
    assert_eq!(std::fs::read(&keep).unwrap(), b"original");
    run(&mut s, "file.saveAttachment", json!({"index": 0, "path": dir.join("timeline.csv")}));
    assert!(std::fs::read_to_string(dir.join("timeline.csv")).unwrap().starts_with("milestone,owner,due"));
    assert!(matches!(s.run("file.open", &json!({"path": dir.join("missing.msg")})), Err(EngineError::Io(_))));
    std::fs::write(dir.join("junk.msg"), b"not a compound file").unwrap();
    assert!(matches!(s.run("file.open", &json!({"path": dir.join("junk.msg")})), Err(EngineError::Format(_))));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_attachment_is_opened_only_on_the_reader_s_word_and_never_a_program() {
    use openmailclient_model::{Attachment, Item};
    let mut s = Session::new();
    let mut item = Item::new_mail();
    item.attachments.push(Attachment::file("notes.txt", "text/plain", b"plain words".to_vec()));
    item.attachments.push(Attachment::file("setup.exe", "application/octet-stream", b"MZ".to_vec()));
    item.attachments.push(Attachment::file("invoice.pdf.js", "text/javascript", b"x".to_vec()));
    let mut inner = Item::new_mail();
    inner.set_subject("Inside");
    let mut message = Attachment::file("Inside", "message/rfc822", Vec::new());
    message.embedded = Some(Box::new(inner));
    item.attachments.push(message);
    s.open = Some(item);
    s.selected = Some(1);
    // Asked without the reader's answer, nothing is written.
    assert!(s.run("file.openAttachment", &json!({"index": 0})).is_err());
    assert!(s.run("file.openAttachment", &json!({"index": 9, "confirm": true})).is_err());
    // A program or a script under a name that hides it is only ever saved.
    for index in [1, 2] {
        let refused = s.run("file.openAttachment", &json!({"index": index, "confirm": true})).unwrap_err().to_string();
        assert!(refused.contains("not opened"), "{refused}");
    }
    let opened = run(&mut s, "file.openAttachment", json!({"index": 0, "confirm": true}));
    assert_eq!(opened["inside"], false);
    let path = std::path::PathBuf::from(opened["untrusted_content"]["open"].as_str().unwrap());
    assert!(path.starts_with(crate::attachments::opened_folder().unwrap()), "{}", path.display());
    assert_eq!(std::fs::read(&path).unwrap(), b"plain words");
    assert!(std::fs::metadata(&path).unwrap().permissions().readonly());
    if cfg!(windows) {
        let mut zone = path.clone().into_os_string();
        zone.push(":Zone.Identifier");
        assert!(std::fs::read_to_string(zone).unwrap().contains("ZoneId=3"));
    }
    // Opened again while the first copy stands, it takes a name of its own.
    let again = run(&mut s, "file.openAttachment", json!({"index": 0, "confirm": true}));
    assert_ne!(again["untrusted_content"]["open"], opened["untrusted_content"]["open"]);
    // An attached message is opened in the program itself.
    let message = run(&mut s, "file.openAttachment", json!({"index": 3, "confirm": true}));
    assert_eq!(message["inside"], true);
    let copy = message["untrusted_content"]["open"].as_str().unwrap().to_string();
    assert!(copy.ends_with(".msg"), "{copy}");
    let mut other = Session::new();
    run(&mut other, "file.open", json!({"path": copy}));
    assert_eq!(other.open.as_ref().unwrap().subject(), "Inside");
    // The copies go when the program ends, read-only as they are.
    crate::attachments::clear_opened(true);
    assert!(!path.exists());
}

#[test]
fn attachment_names_are_made_safe_and_told_apart() {
    use crate::attachments::{Caution, caution, ending, safe_name};
    for (name, want) in [
        ("report.pdf", "report.pdf"),
        ("Report Final.PDF", "Report Final.PDF"),
        ("", "attachment"),
        ("   ", "attachment"),
        ("...", "attachment"),
        ("\u{0}\u{7}", "attachment"),
        // One name, never a path.
        ("..\\..\\Windows\\system.ini", "_.._Windows_system.ini"),
        ("../../etc/passwd", "_.._etc_passwd"),
        ("C:\\Users\\me\\x.txt", "C__Users_me_x.txt"),
        ("a<b>c:d\"e|f?g*h.txt", "a_b_c_d_e_f_g_h.txt"),
        ("/", "_"),
        // Nothing Windows drops or hides.
        ("notes.txt. . .", "notes.txt"),
        (".hidden", "hidden"),
        ("  spaced   out  .txt ", "spaced out.txt"),
        ("line\r\nbreak\t.txt", "linebreak.txt"),
        ("invoice.pdf                                        .exe", "invoice.pdf.exe"),
        // Device names are not opened as devices.
        ("CON", "_CON"),
        ("nul.txt", "_nul.txt"),
        ("Com1.tar.gz", "_Com1.tar.gz"),
        ("LPT9", "_LPT9"),
        ("console.txt", "console.txt"),
        // Characters that reverse or hide are left out: the name is what it is.
        ("invoice\u{202e}fdp.exe", "invoicefdp.exe"),
        ("a\u{200b}b\u{feff}.txt", "ab.txt"),
    ] {
        assert_eq!(safe_name(name), want, "{name:?}");
    }
    // A long name is cut in its stem; its ending stays.
    let long = safe_name(&format!("{}.docx", "long name ".repeat(40)));
    assert!(long.chars().count() <= 120 && long.ends_with(".docx") && long.starts_with("long name long"), "{long}");
    let long = safe_name(&"é".repeat(5000));
    assert_eq!(long.chars().count(), 119);
    assert_eq!(safe_name(&format!("a.{}", "x".repeat(300))).chars().count(), 119, "an ending too long to be one is part of the name");

    for (name, want) in
        [("a.PDF", Some("pdf")), ("a.tar.gz", Some("gz")), ("a.exe. .", Some("exe")), ("a", None), (".txt", None), ("a.", None), ("a.b c", None)]
    {
        assert_eq!(ending(name).as_deref(), want, "{name:?}");
    }

    let program: Vec<u8> = b"MZ".iter().copied().chain(std::iter::repeat_n(0, 100)).collect();
    let runs = |e: &str| Some(Caution::Runs { ending: e.into() });
    for (name, data, want) in [
        ("report.pdf", &b"%PDF-1.7"[..], None),
        ("notes.txt", b"MZ is how this note starts", None),
        ("photo.jpeg", b"", None),
        ("archive.tar.gz", b"", None),
        ("v1.2 notes", b"", None),
        ("setup.exe", &program[..], runs("exe")),
        ("Setup.EXE", b"", runs("exe")),
        ("run.bat. .", b"", runs("bat")),
        ("shortcut.lnk", b"", runs("lnk")),
        ("script.js", b"", runs("js")),
        ("budget.xlsm", b"", Some(Caution::Macros { ending: "xlsm".into() })),
        ("form.html", b"", Some(Caution::Page { ending: "html".into() })),
        ("drawing.svg", b"", Some(Caution::Page { ending: "svg".into() })),
        ("invoice.pdf.exe", b"", Some(Caution::TwoEndings { reads: "pdf".into(), is: "exe".into() })),
        ("invoice.pdf          .scr", b"", Some(Caution::TwoEndings { reads: "pdf".into(), is: "scr".into() })),
        ("statement.docx.htm", b"", Some(Caution::TwoEndings { reads: "docx".into(), is: "htm".into() })),
        ("photos.zip.docm", b"", Some(Caution::TwoEndings { reads: "zip".into(), is: "docm".into() })),
        ("my.setup.exe", b"", runs("exe")),
        ("invoice\u{202e}fdp.exe", b"", Some(Caution::Reversed { is: Some("exe".into()) })),
        ("\u{202e}", b"", Some(Caution::Reversed { is: None })),
        ("report.pdf", &program[..], Some(Caution::Program { named: Some("pdf".into()) })),
        ("readme", &program[..], Some(Caution::Program { named: None })),
        ("tool.dll", &program[..], runs("dll")),
    ] {
        assert_eq!(caution(name, data), want, "{name:?}");
    }
    assert_eq!(Caution::TwoEndings { reads: "pdf".into(), is: "exe".into() }.words(), "The name reads as a .pdf file, but it is a .exe file.");

    // Hostile: nothing here panics, and a safe name is always one that can be written.
    let pieces =
        ["", ".", "..", "/", "\\", ":", " ", "\u{0}", "\u{202e}", "\u{200b}", "CON", "nul.", "é", "a", ".exe", ".pdf", "$", "\u{1f600}", "\r\n"];
    for a in pieces {
        for b in pieces {
            for c in pieces {
                let name = format!("{a}{b}{c}");
                let safe = safe_name(&name);
                assert!(!safe.is_empty() && safe.chars().count() <= 120, "{name:?}");
                assert!(
                    !safe.contains(['/', '\\', ':', '\u{0}', '\u{202e}']) && !safe.starts_with(['.', ' ']) && !safe.ends_with(['.', ' ']),
                    "{name:?} -> {safe:?}"
                );
                assert_eq!(safe_name(&safe), safe, "safe names stay as they are: {name:?}");
                let _ = caution(&name, name.as_bytes());
                let _ = ending(&name);
            }
        }
    }
    let _ = caution(&".exe".repeat(100_000), b"MZ");
}

#[test]
fn saving_all_attachments_replaces_no_file() {
    use openmailclient_model::Attachment;
    let dir = std::env::temp_dir().join(format!("openmailclient-engine-attachments-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = session();
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    let mut inner = openmailclient_model::Item::default();
    inner.props.set_str(openmailclient_model::pid::SUBJECT, "A note: inside");
    {
        let item = s.open.as_mut().unwrap();
        assert_eq!(item.attachments.len(), 1, "the sample's launch timeline");
        item.attachments.push(Attachment::file("launch-timeline.csv", "text/csv", b"second".to_vec()));
        item.attachments.push(Attachment::file("LAUNCH-TIMELINE.CSV", "text/csv", b"third".to_vec()));
        item.attachments.push(Attachment::file("..\\..\\escape.txt", "text/plain", b"stays here".to_vec()));
        item.attachments.push(Attachment::file("nul", "", b"not a device".to_vec()));
        let mut placed = Attachment::file("logo.png", "image/png", b"picture".to_vec());
        placed.props.set_str(openmailclient_model::pid::ATTACH_CONTENT_ID, "logo@example.org");
        item.attachments.push(placed);
        let mut note = Attachment::file("A note: inside", "", Vec::new());
        note.embedded = Some(Box::new(inner));
        item.attachments.push(note);
    }
    // A file that was there before stays as it was.
    std::fs::write(dir.join("launch-timeline.csv"), b"mine").unwrap();
    let saved = run(&mut s, "file.saveAllAttachments", json!({"folder": dir}));
    assert_eq!(saved["count"], 6);
    assert_eq!(s.status, format!("Saved 6 attachments to {}", dir.display()));
    let names: Vec<String> = saved["untrusted_content"]["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| std::path::Path::new(p.as_str().unwrap()).file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        ["launch-timeline (2).csv", "launch-timeline (3).csv", "LAUNCH-TIMELINE (4).CSV", "_.._escape.txt", "_nul", "A note_ inside.msg"]
    );
    assert_eq!(std::fs::read(dir.join("launch-timeline.csv")).unwrap(), b"mine");
    assert!(std::fs::read_to_string(dir.join("launch-timeline (2).csv")).unwrap().starts_with("milestone,owner,due"));
    assert_eq!(std::fs::read(dir.join("LAUNCH-TIMELINE (4).CSV")).unwrap(), b"third");
    assert_eq!(std::fs::read(dir.join("_.._escape.txt")).unwrap(), b"stays here");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 7, "nothing was written anywhere else");
    let mut other = Session::new();
    run(&mut other, "file.open", json!({"path": dir.join("A note_ inside.msg")}));
    assert_eq!(other.open.as_ref().unwrap().subject(), "A note: inside");

    // Pictures placed in the body come along when asked for; again nothing is replaced.
    assert_eq!(run(&mut s, "file.saveAllAttachments", json!({"folder": dir, "inline": true}))["count"], 7);
    assert_eq!(std::fs::read(dir.join("logo.png")).unwrap(), b"picture");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 14);

    // One attachment into a folder goes under its own name; to a path, where it is told to.
    let one = run(&mut s, "file.saveAttachment", json!({"index": 3, "path": dir}));
    assert!(one["untrusted_content"]["path"].as_str().unwrap().ends_with("_.._escape (3).txt"), "{one}");
    run(&mut s, "file.saveAttachment", json!({"index": 1, "path": dir.join("chosen.csv")}));
    assert_eq!(std::fs::read(dir.join("chosen.csv")).unwrap(), b"second");

    assert!(matches!(s.run("file.saveAllAttachments", &json!({"folder": dir.join("missing")})), Err(EngineError::BadParams(_))));
    assert!(matches!(s.run("file.saveAllAttachments", &json!({"folder": dir.join("chosen.csv")})), Err(EngineError::BadParams(_))));
    assert!(s.run("file.saveAllAttachments", &Value::Null).is_err());

    // A message without attachments saves none and says so.
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 1}));
    assert_eq!(run(&mut s, "file.saveAllAttachments", json!({"folder": dir}))["count"], 0);
    assert_eq!(s.status, "This message has no attachments to save");

    // What a reader should know is said with the attachment.
    run(&mut s, "mail.select", json!({"folder": "Junk Email", "index": 0}));
    let told = s.inspect();
    let attachment = &told["open"]["attachments"][0];
    assert_eq!(attachment["name"], "Mailbox-quota.pdf.htm");
    assert_eq!(attachment["caution"], "The name reads as a .pdf file, but it is a .htm file.");
    assert_eq!(attachment["file"], true);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn closing_data_files_keeps_the_session_usable() {
    let mut s = session();
    run(&mut s, "file.openSample", Value::Null);
    assert_eq!((s.stores.len(), s.current.unwrap().0), (2, 1));
    run(&mut s, "file.closeStore", json!({"store": 0}));
    assert_eq!((s.stores.len(), s.current.unwrap().0), (1, 0));
    run(&mut s, "file.closeStore", Value::Null);
    assert!(s.stores.is_empty() && s.current.is_none() && s.rows.is_empty());
    assert!(!s.is_enabled("mail.select") && s.is_enabled("file.open"));
}

#[test]
fn inspect_marks_message_text_as_untrusted() {
    let mut s = session();
    run(&mut s, "mail.select", json!({"index": 0}));
    let v = run(&mut s, "session.inspect", Value::Null);
    assert!(v["open"]["untrusted_content"]["body"].as_str().unwrap().contains("launch checklist"));
    assert!(v["open"].get("body").is_none() && v["rows"][0].get("subject").is_none());
}

#[test]
fn a_message_kept_only_as_rtf_has_text_a_preview_and_survives_as_html() {
    use openmailclient_model::{Item, PropValue, pid, rtf};
    let mut s = session();
    run(&mut s, "mail.select", json!({ "folder": "Inbox/Receipts", "index": 0 }));
    let item = s.open.as_ref().unwrap();
    assert!(item.props.get(pid::BODY).is_none() && item.props.get(pid::HTML).is_none());
    // Without this Outlook Classic shows an RTF-only message with an empty body.
    assert_eq!(item.props.get(pid::RTF_IN_SYNC), Some(&PropValue::Bool(true)));
    let text = item.body_text();
    assert!(text.starts_with("Riverside Car Park\nReceipt for permit RP-20418, paid by card."), "{text}");
    assert!(text.contains("Monthly permit, bay 14\t62.00"));
    assert!(!text.contains("HYPERLINK") && !text.contains("Arial"));
    assert!(s.rows[0].preview.starts_with("Riverside Car Park Receipt for permit"));
    assert_eq!(item.body_html(), None, "ordinary RTF is not HTML");

    // RTF that wraps HTML ([MS-OXRTFEX]) is the message's HTML body wherever one is asked for.
    let mut wrapped = Item::new_mail();
    let body = b"{\\rtf1\\ansi\\fromhtml1 {\\*\\htmltag64 <p>}\\htmlrtf {\\htmlrtf0 Hello {\\*\\htmltag84 <b>}you{\\*\\htmltag92 </b>}\\htmlrtf }\\htmlrtf0{\\*\\htmltag72 </p>}}";
    wrapped.props.set(pid::RTF_COMPRESSED, PropValue::Binary(rtf::store(body)));
    assert_eq!(wrapped.body_html().as_deref(), Some("<p>Hello <b>you</b></p>"));
    assert_eq!(wrapped.body_text(), "Hello you");
    // Damaged or foreign bytes in the property are no body, not an error.
    wrapped.props.set(pid::RTF_COMPRESSED, PropValue::Binary(vec![0xAB; 40]));
    assert_eq!((wrapped.body_html(), wrapped.body_text()), (None, String::new()));
}

#[test]
fn a_picture_sent_with_a_message_is_an_attachment_the_body_shows_not_a_listed_one() {
    let mut s = session();
    run(&mut s, "mail.select", json!({ "index": 4 }));
    let item = s.open.as_ref().unwrap();
    assert_eq!(item.attachments.len(), 1);
    let proof = &item.attachments[0];
    assert_eq!((proof.name().as_str(), proof.mime()), ("banner-proof.png", "image/png"));
    assert!(proof.data.starts_with(b"\x89PNG\r\n\x1a\n") && proof.data.ends_with(b"IEND\xAE\x42\x60\x82"));
    assert!(item.body_html().unwrap().contains(&format!("cid:{}", proof.content_id().unwrap())));
    assert_eq!(item.visible_attachments().count(), 0, "the body shows it, so the attachment list does not");
}

#[test]
fn the_sample_receipt_has_a_drawing_that_is_drawn() {
    use openmailclient_formats::metafile::wmf_picture;
    // The map is a metafile of shapes and no bitmap: it is drawn, at its own shape.
    let map = crate::sample::bay_map();
    assert_eq!(openmailclient_formats::metafile::wmf_bitmap(&map), None);
    let bmp = wmf_picture(&map, Some((240, 120))).unwrap();
    assert_eq!((&bmp[..2], &bmp[18..22], &bmp[22..26]), (&b"BM"[..], &480u32.to_le_bytes()[..], &240u32.to_le_bytes()[..]));
    // And it stands in the receipt's body as a picture that was read, beside the card.
    let mut s = session();
    run(&mut s, "mail.select", json!({ "folder": "Inbox/Receipts", "index": 0 }));
    let rtf = s.open.as_ref().unwrap().props.bin(openmailclient_model::pid::RTF_COMPRESSED).and_then(openmailclient_model::rtf::decompress).unwrap();
    let doc = openmailclient_formats::rtf::parse(&rtf);
    assert_eq!(doc.written.len(), 1);
    assert_eq!(doc.written[0][..], bmp[..]);
}

#[test]
fn a_link_is_followed_only_when_it_is_the_message_s_and_safe_to_follow() {
    let mut s = session();
    // Nothing is open: there are no links to list or to follow.
    assert!(s.run("mail.links", &Value::Null).is_err() && s.run("mail.openLink", &json!({"index": 0})).is_err());

    run(&mut s, "mail.select", json!({"folder": "Junk Email", "index": 0}));
    let listed = run(&mut s, "mail.links", Value::Null);
    assert_eq!(listed["count"], 5);
    let links = listed["untrusted_content"]["links"].as_array().unwrap();
    let goes: Vec<&str> = links.iter().map(|l| l["goes"].as_str().unwrap()).collect();
    assert_eq!(
        goes,
        ["mail.example.org.account-check.example.net", "sign-in.example.net", "192.0.2.44", "mailbox-notice.example.net", "example.org"]
    );
    assert_eq!(links[1]["text"], "mail.example.org");
    assert_eq!(links[1]["address"], "https://mail.example.org@sign-in.example.net/");
    // Four of them are not what they say; the last one is.
    assert!(links[..4].iter().all(|l| l["concern"].is_string()) && links[4]["concern"].is_null());

    // A link with a concern is not followed without being told to.
    let refused = s.run("mail.openLink", &json!({"index": 0})).unwrap_err().to_string();
    assert!(refused.contains("account-check.example.net") && refused.contains("confirm"), "{refused}");
    assert!(s.run("mail.openLink", &json!({"index": 0, "confirm": "yes"})).is_err());
    let told = run(&mut s, "mail.openLink", json!({"index": 0, "confirm": true}));
    assert_eq!(told["untrusted_content"]["open"], "https://mail.example.org.account-check.example.net/quota");
    // One that is what it says needs no telling, by number or by address.
    let told = run(&mut s, "mail.openLink", json!({"index": 4}));
    assert_eq!(told["untrusted_content"], json!({"open": "https://example.org/help", "goes": "example.org"}));
    assert_eq!(run(&mut s, "mail.openLink", json!({"address": "https://example.org/help"}))["untrusted_content"]["open"], "https://example.org/help");
    // An address the message does not have is not followed, whatever it is.
    for other in ["https://example.com/", "file:///C:/Windows/System32/calc.exe", "", "https://example.org/help "] {
        assert!(s.run("mail.openLink", &json!({"address": other, "confirm": true})).is_err(), "{other}");
    }
    assert!(s.run("mail.openLink", &json!({"index": 5})).is_err() && s.run("mail.openLink", &json!({"index": u64::MAX})).is_err());
    assert!(s.run("mail.openLink", &Value::Null).is_err());
    // The engine itself has started nothing: no message is being written yet.
    assert!(s.compose.is_none());

    // A mailbox starts a message to it, once the concern is confirmed.
    assert!(s.run("mail.openLink", &json!({"index": 3})).is_err() && s.compose.is_none());
    let told = run(&mut s, "mail.openLink", json!({"index": 3, "confirm": true}));
    assert_eq!(told["untrusted_content"]["mailbox"], "helpdesk@mailbox-notice.example.net");
    let draft = s.compose.as_ref().unwrap();
    assert_eq!((draft.to.as_str(), draft.kind, draft.subject.as_str()), ("helpdesk@mailbox-notice.example.net", "New", ""));

    // The receipt's link, in an RTF body, is found the same way.
    run(&mut s, "compose.discard", Value::Null);
    run(&mut s, "mail.select", json!({"folder": "Inbox/Receipts", "index": 0}));
    let listed = run(&mut s, "mail.links", Value::Null);
    assert_eq!(listed["untrusted_content"]["links"][0]["address"], "https://parking.example.net/help");
}

#[test]
fn links_of_hostile_bodies_are_listed_within_bounds() {
    use openmailclient_model::{Item, PropValue, pid};
    let mut html = String::from("<html><body>");
    for i in 0..3000 {
        html.push_str(&format!("<a href=\"javascript:{i}//{}\">{}</a> <a href=\"\">none</a> ", "x".repeat(5000), "t".repeat(3000)));
    }
    for _ in 0..200 {
        html.push_str("<table><tr><td>");
    }
    html.push_str("<a href=\"https://example.org/\u{0}\u{202e}\">deep</a>");
    let mut item = Item::default();
    item.props.set(pid::HTML, PropValue::Binary(html.into_bytes()));
    let found = crate::links::of(&item);
    assert_eq!(found.len(), crate::links::MAX_LINKS);
    assert!(found.iter().all(|l| l.text.chars().count() <= 200 && l.address.chars().count() <= 2048 && !l.address.chars().any(char::is_control)));
    // None of them is one to follow.
    let mut s = session();
    s.open = Some(item);
    for index in [0, 1, 499] {
        assert!(s.run("mail.openLink", &json!({"index": index, "confirm": true})).is_err());
    }
}

#[test]
fn page_setup_is_kept_and_printed_with() {
    use openmailclient_layout::{MM, Paper, Setup};
    let mut s = session();
    // It can be asked about and changed with no message open.
    assert!(s.is_enabled("file.pageSetup"));
    let was = run(&mut s, "file.pageSetup", json!({}));
    assert_eq!(
        was,
        json!({"style": "memo", "columns": ["who", "subject", "date", "size", "notes"], "groups": true, "rows": "all",
            "paper": "a4", "width_mm": 210.0, "height_mm": 297.0, "orientation": "portrait",
            "margins": {"top": 12.7, "right": 12.7, "bottom": 12.7, "left": 12.7},
            "header": {"left": "", "center": "", "right": ""}, "footer": {"left": "", "center": "[Page of Pages]", "right": ""},
            "details": true, "headerFooter": true, "bodyOnly": false})
    );
    assert_eq!(s.page_setup, Setup::default());

    // What is asked for changes; the rest stays.
    let now = run(
        &mut s,
        "file.pageSetup",
        json!({"paper": "Legal", "orientation": "Landscape", "margins": {"top": 25, "left": 20.5}, "header": "[Subject]"}),
    );
    assert_eq!(now["paper"], "legal");
    assert_eq!(now["orientation"], "landscape");
    assert_eq!(now["margins"], json!({"top": 25.0, "right": 12.7, "bottom": 12.7, "left": 20.5}));
    assert_eq!(now["header"], json!({"left": "", "center": "[Subject]", "right": ""}));
    assert_eq!(now["footer"], was["footer"]);
    assert_eq!(s.status, "Page setup changed");
    let now = run(&mut s, "file.pageSetup", json!({"margins": 10, "header": {"right": "[Date]\u{7}\n"}, "footer": {"left": "[User]", "centre": ""}}));
    assert_eq!(now["margins"], json!({"top": 10.0, "right": 10.0, "bottom": 10.0, "left": 10.0}));
    assert_eq!(now["header"], json!({"left": "", "center": "[Subject]", "right": "[Date]"}));
    assert_eq!(now["footer"], json!({"left": "[User]", "center": "", "right": ""}));
    assert_eq!(s.page_setup.paper, Paper::LEGAL);
    assert!(s.page_setup.landscape && (s.page_setup.margins[0] - 10.0 * MM).abs() < 0.001);
    // A part is no longer than may stand on a line of any sheet.
    let now = run(&mut s, "file.pageSetup", json!({"footer": "f".repeat(5000)}));
    assert_eq!(now["footer"]["center"].as_str().unwrap().len(), 400);

    // What is no setup changes nothing.
    let kept = s.page_setup.clone();
    for bad in [
        json!({"paper": "foolscap"}),
        json!({"orientation": "sideways"}),
        json!({"margins": -1}),
        json!({"margins": 150.5}),
        json!({"margins": "wide"}),
        json!({"margins": {"up": 1}}),
        json!({"margins": {"top": "a"}}),
        json!({"margins": {"top": 5, "left": 1e300}}),
        json!({"header": 7}),
        json!({"header": {"left": 7}}),
        json!({"footer": {"under": "x"}}),
        json!({"paper": "a5", "footer": []}),
        json!({"details": "no"}),
        json!({"headerFooter": 0}),
        json!({"bodyOnly": null}),
    ] {
        assert!(matches!(s.run("file.pageSetup", &bad), Err(EngineError::BadParams(_))), "{bad}");
        assert_eq!(s.page_setup, kept, "{bad}");
    }

    // The names a header or footer may ask for: the day and time of printing, where the
    // owner is, whose mailbox it is, and the message's subject.
    run(&mut s, "file.pageSetup", json!({"reset": true, "header": {"left": "[Subject]"}}));
    assert_eq!(s.page_setup, Setup { header: ["[Subject]".into(), String::new(), String::new()], ..Setup::default() });
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    let item = s.open.clone().unwrap();
    let listing = crate::print::Listing::default();
    let region = openmailclient_model::time::Region::default();
    let (utc, east_zone) = (openmailclient_model::zone::Zone::utc(), openmailclient_model::zone::Zone::fixed(120));
    let job = |zone| crate::print::Job {
        owner: "Alex Morgan",
        zone,
        now: openmailclient_model::time::from_civil(2026, 10, 12, 23, 5, 0, 0),
        setup: &kept,
        links: crate::print::Links::default(),
        listing: &listing,
        region: &region,
    };
    let words = crate::print::words(&item, &job(&utc));
    let word = |words: &[(String, String)], name: &str| words.iter().find(|w| w.0 == name).unwrap().1.clone();
    assert_eq!((word(&words, "Date"), word(&words, "Time")), ("10/12/2026".to_string(), "11:05 PM".to_string()));
    assert_eq!((word(&words, "User"), word(&words, "Subject")), ("Alex Morgan".to_string(), item.subject()));
    let east = crate::print::words(&item, &job(&east_zone));
    assert_eq!((word(&east, "Date"), word(&east, "Time")), ("10/13/2026".to_string(), "1:05 AM".to_string()));

    // Printing uses the setup, and what a print asks for is for that print only. Through the
    // command there is a file, or the word that no font is installed.
    let dir = std::env::temp_dir().join(format!("openmailclient-setup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let has = |file: &[u8], needle: &str| file.windows(needle.len()).any(|w| w == needle.as_bytes());
    let kept = s.page_setup.clone();
    for (params, sheet) in [
        (json!({}), "/MediaBox [0 0 595.28 841.89]"),
        (json!({"paper": "letter", "orientation": "landscape", "margins": 30, "footer": ""}), "/MediaBox [0 0 792 612]"),
        (json!({"paper": "a5"}), "/MediaBox [0 0 419.53 595.28]"),
    ] {
        let path = dir.join("setup.pdf");
        let mut params = params;
        params["path"] = json!(path);
        match s.run("file.print", &params) {
            Ok(_) => {
                let file = std::fs::read(&path).unwrap();
                assert!(has(&file, sheet), "{sheet}");
                std::fs::remove_file(&path).unwrap();
            }
            Err(e) => assert!(matches!(e, EngineError::Format(_)) && !path.exists(), "{e}"),
        }
        assert_eq!(s.page_setup, kept);
    }
    assert!(matches!(s.run("file.print", &json!({"path": dir.join("p.pdf"), "margins": -3})), Err(EngineError::BadParams(_))));
    assert!(!dir.join("p.pdf").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_page_can_be_looked_at_before_it_is_printed() {
    let mut s = session();
    assert!(!Session::new().is_enabled("file.printPreview"));
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    let dir = std::env::temp_dir().join(format!("openmailclient-preview-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("page.png");

    // What is no page, no picture file or no density is refused before anything is laid out.
    for bad in [
        json!({"path": dir.join("page.jpg")}),
        json!({"path": dir.join("page")}),
        json!({"page": 0}),
        json!({"page": -1}),
        json!({"page": "one"}),
        json!({"dpi": 10}),
        json!({"dpi": 601}),
        json!({"dpi": "fine"}),
        json!({"links": "some"}),
        json!({"margins": -1}),
    ] {
        assert!(matches!(s.run("file.printPreview", &bad), Err(EngineError::BadParams(_))), "{bad}");
    }
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none());

    // The page that is drawn is the page that is printed: with the test font both are known.
    let item = s.open.clone().unwrap();
    let library = openmailclient_fonts::Library::empty();
    let laid = {
        let mut fonts = openmailclient_layout::Fonts::new(&library);
        let chars: String = (' '..='~').chain(['\u{e1}', '\u{2022}']).collect();
        fonts.give("Arial", false, false, openmailclient_fonts::Face { data: openmailclient_fonts::boxes::font(&chars), index: 0 });
        let body = openmailclient_formats::html::parse(&item.body_html().unwrap());
        let memo = openmailclient_layout::Memo { owner: "Alex Morgan", fields: &[], body: &body, plain: false, sizes: &|_| None };
        let printed = openmailclient_layout::memo(&memo, openmailclient_layout::Paper::A4, &mut fonts).unwrap();
        crate::print::Laid { links: 0, printed, pictures: crate::print::Pictures::default() }
    };
    let image = crate::print::raster(&laid, 0, 2.0).unwrap();
    assert_eq!((image.width, image.height), (1191, 1684));
    // Ink where the owner's name stands (its first box), and where each piece of text does.
    let seen = |x: f32, y: f32| {
        let at = ((y * 2.0) as usize * image.width + (x * 2.0) as usize) * 3;
        [image.rgb[at], image.rgb[at + 1], image.rgb[at + 2]]
    };
    let texts = &laid.printed.pages[0].texts;
    assert!(texts.len() > 20);
    for text in texts.iter().filter(|t| t.glyphs.first().is_some_and(|g| g.1 != ' ')) {
        assert_eq!(seen(text.x + text.size * 0.25, text.y - text.size * 0.3), text.color, "{text:?}");
    }
    assert!(seen(10.0, 10.0) == [255; 3] && seen(300.0, 830.0) == [255; 3]);
    assert!(crate::print::raster(&laid, 1, 2.0).is_none());
    let file = crate::print::png(&image).unwrap();
    assert!(file.starts_with(b"\x89PNG\r\n\x1a\n"));
    let back = image::load_from_memory(&file).unwrap().to_rgb8();
    assert_eq!((back.width(), back.height()), (1191, 1684));
    assert_eq!(back.into_raw(), image.rgb);

    // Through the command, in the installed fonts: how many pages, and a picture of one when
    // asked for; or the word that no font is installed. Nothing is kept of the setup.
    let kept = s.page_setup.clone();
    match s.run("file.printPreview", &json!({})) {
        Ok(result) => {
            assert_eq!(result["pages"], 1);
            assert_eq!(result["page"], 1);
            assert_eq!(result["setup"]["paper"], "a4");
            assert!(result.get("path").is_none());
            let result = run(&mut s, "file.printPreview", json!({"path": path, "dpi": 48, "paper": "a5", "orientation": "landscape"}));
            assert_eq!((result["width"].as_u64(), result["height"].as_u64()), (Some(397), Some(280)));
            assert_eq!(result["setup"]["orientation"], "landscape");
            let seen = image::load_from_memory(&std::fs::read(&path).unwrap()).unwrap().to_rgb8();
            assert_eq!((seen.width(), seen.height()), (397, 280));
            // A page with text on it is neither blank nor black.
            let dark = seen.pixels().filter(|p| p.0[0] < 128).count();
            assert!(dark > 200 && dark < 397 * 280 / 4, "{dark}");
            let pages = result["pages"].as_u64().unwrap();
            assert!(matches!(
                s.run("file.printPreview", &json!({"page": pages + 1, "paper": "a5", "orientation": "landscape"})),
                Err(EngineError::BadParams(_))
            ));
        }
        Err(e) => assert!(matches!(e, EngineError::Format(_)), "{e}"),
    }
    assert_eq!(s.page_setup, kept);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_folders_list_is_printed_as_a_table() {
    let mut s = session();
    assert!(!Session::new().is_enabled("file.print"));
    // With a folder shown there is a list to print, though no message is open.
    assert!(s.open.is_none() && s.is_enabled("file.print") && s.is_enabled("file.printPreview"));
    let dir = std::env::temp_dir().join(format!("openmailclient-table-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(matches!(s.run("file.print", &json!({"path": dir.join("memo.pdf")})), Err(EngineError::Store(_))));
    for bad in [json!({"style": "grid"}), json!({"style": 7}), json!({"style": ""}), json!({"style": ["table"]})] {
        assert!(matches!(s.run("file.printPreview", &bad), Err(EngineError::BadParams(_))), "{bad}");
        assert!(matches!(s.run("file.pageSetup", &bad), Err(EngineError::BadParams(_))), "{bad}");
    }
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none());

    // The list as it is shown, in the test font: a row to a message, in the list's order, the
    // unread in bold, under the owner, the folder and what is listed.
    let library = openmailclient_fonts::Library::empty();
    let laid = |s: &Session| {
        let setup = s.page_setup.clone();
        let job = crate::print::job(s, &setup, crate::print::Links::default());
        // The layout asks the library for its fonts; the test font stands in for them all.
        let folder = s.current_folder().unwrap();
        let columns = [openmailclient_layout::Column { name: "From", share: 100, right: false }];
        let rows: Vec<openmailclient_layout::Row> = Vec::new();
        let listed = openmailclient_layout::Listed { owner: job.owner, title: &folder.name, note: "", columns: &columns, rows: &rows };
        let mut fonts = openmailclient_layout::Fonts::new(&library);
        assert!(openmailclient_layout::list(&listed, openmailclient_layout::Paper::A4, &mut fonts).is_err(), "no font, no print");
        crate::print::list_layout(s, &job, &library)
    };
    assert!(matches!(laid(&s), Err(EngineError::Format(_))));

    // Through the commands, in the installed fonts (or the word that none is installed).
    match s.run("file.printPreview", &json!({"style": "Table Style"})) {
        Ok(told) => {
            assert_eq!((told["style"].as_str(), told["pages"].as_u64(), told["links"].as_u64()), (Some("table"), Some(1), Some(0)));
            assert_eq!(s.print_style, crate::print::Style::Memo, "a look keeps nothing");
            let kept = run(&mut s, "file.pageSetup", json!({"style": "table", "header": "[Folder]"}));
            assert_eq!(kept["style"], "table");
            let laid = crate::print::laid_as(&s, s.print_style, &s.page_setup, &s.print_list, crate::print::Links::default()).unwrap();
            let text = laid.printed.pages[0].text();
            let lines: Vec<&str> = text.lines().collect();
            assert_eq!(lines[..4], ["Alex Morgan", "Inbox", "9 items, 3 unread; by Date", "FromSubjectReceivedSizeNotes"], "{text}");
            // Each row is there, in the order of the list, and the folder's name heads the page.
            let mut from = 0;
            for row in &s.rows {
                let at = text[from..].find(&row.subject).unwrap_or_else(|| panic!("{} after {from} in {text}", row.subject));
                from += at;
            }
            assert!(lines.iter().filter(|l| **l == "Inbox").count() == 2, "{text}");
            let bold =
                |words: &str| laid.printed.pages[0].texts.iter().any(|t| t.bold && t.glyphs.iter().map(|g| g.1).collect::<String>().contains(words));
            let unread = s.rows.iter().find(|r| !r.read).unwrap();
            let read = s.rows.iter().find(|r| r.read).unwrap();
            let face = |words: &str| {
                let text = laid.printed.pages[0].texts.iter().find(|t| t.glyphs.iter().map(|g| g.1).collect::<String>().contains(words)).unwrap();
                (text.face, text.bold)
            };
            assert!(face(&unread.subject) != face(&read.subject) || bold(&unread.subject));
            // The list is printed as it is shown: searched and ordered.
            run(&mut s, "search.query", json!({"text": "rota"}));
            let found = s.rows.len();
            assert!(found > 0 && found < 9);
            let laid = crate::print::laid_as(&s, s.print_style, &s.page_setup, &s.print_list, crate::print::Links::default()).unwrap();
            let text = laid.printed.pages[0].text();
            assert!(text.contains("search: rota") && !text.contains("Budget numbers"), "{text}");
            // To a file, with the style kept; a call may still ask for the other.
            let told = run(&mut s, "file.print", json!({"path": dir.join("list.pdf")}));
            assert_eq!((told["style"].as_str(), told["pages"].as_u64()), (Some("table"), Some(1)));
            let file = std::fs::read(dir.join("list.pdf")).unwrap();
            assert!(file.starts_with(b"%PDF-") && file.len() > 2000);
            assert!(matches!(s.run("file.print", &json!({"path": dir.join("memo.pdf"), "style": "memo"})), Err(EngineError::Store(_))));
            assert_eq!(run(&mut s, "file.pageSetup", json!({"reset": true}))["style"], "memo");
        }
        Err(e) => assert!(matches!(e, EngineError::Format(_)), "{e}"),
    }
    assert_eq!(crate::print::Style::named(" TABLE style "), Some(crate::print::Style::Table));
    assert_eq!(crate::print::Style::named("memostyle"), Some(crate::print::Style::Memo));
    assert_eq!(crate::print::Style::named("style"), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_mailbox_is_exported_to_a_pst_and_opens_again() {
    let dir = std::env::temp_dir().join(format!("openmailclient-export-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = session();
    let path = dir.join("sample.pst");
    let done = run(&mut s, "file.export", json!({"path": path}));
    assert_eq!(done["unreadable"], 0);
    assert!(done["items"].as_u64().unwrap() > 5 && done["folders"].as_u64().unwrap() >= 7);
    assert_eq!(done["bytes"].as_u64().unwrap(), std::fs::metadata(&path).unwrap().len());
    assert!(s.status.starts_with("Exported "));
    // Nothing is left beside it.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    // The file passes the structure check, and holds what the mailbox held.
    let found = crate::io::check_structure(&path).unwrap().unwrap();
    assert!(found.problems.is_empty(), "{:?}", found.problems);

    let mut other = Session::new();
    run(&mut other, "file.open", json!({"path": path}));
    assert_eq!(other.stores[0].display_name(), s.stores[0].display_name());
    let names = |s: &mut Session| {
        let mut all: Vec<(String, u32, u32)> = s.stores[0].folders().unwrap().into_iter().map(|f| (f.name, f.total, f.unread)).collect();
        all.sort();
        all
    };
    assert_eq!(names(&mut other), names(&mut s));
    assert_eq!(other.folders[0].iter().find(|f| f.kind == FolderKind::Inbox).map(|f| f.name.as_str()), Some("Inbox"));
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    run(&mut other, "mail.select", json!({"folder": "Inbox", "index": 0}));
    let (a, b) = (s.open.clone().unwrap(), other.open.clone().unwrap());
    assert_eq!(
        (b.subject(), b.sender_name(), b.body_text(), b.categories.clone()),
        (a.subject(), a.sender_name(), a.body_text(), a.categories.clone())
    );
    assert_eq!(
        b.attachments.iter().map(|x| (x.name(), x.data.clone())).collect::<Vec<_>>(),
        a.attachments.iter().map(|x| (x.name(), x.data.clone())).collect::<Vec<_>>()
    );
    assert_eq!(b.recipients.len(), a.recipients.len());

    // A data file read from disk goes out again, and over a file that was there.
    let again = dir.join("again.pst");
    std::fs::write(&again, b"what was here before").unwrap();
    let second = run(&mut other, "file.export", json!({"path": again, "store": 0}));
    assert_eq!((&second["items"], &second["folders"]), (&done["items"], &done["folders"]));
    // The sample mailbox goes through the model; a data file is copied as it is stored.
    assert_eq!((&done["copied"], &second["copied"]), (&json!(false), &json!(true)));
    assert!(other.status.ends_with("as it is stored"), "{}", other.status);
    assert!(crate::io::check_structure(&again).unwrap().unwrap().problems.is_empty());

    // Refused: over itself, over a data file that is open here, a name that is not .pst, a
    // data file that is not there. Each leaves what was on disk as it was.
    let before = std::fs::read(&path).unwrap();
    assert!(other.run("file.export", &json!({"path": path})).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(s.run("file.export", &json!({"path": dir.join("mail.ost")})).is_err());
    assert!(s.run("file.export", &json!({"path": dir.join("mail.zip")})).is_err());
    assert!(s.run("file.export", &json!({"path": dir.join("other.pst"), "store": 7})).is_err());
    assert!(s.run("file.saveAs", &json!({"path": dir.join("one.pst")})).is_err());
    assert!(!dir.join("other.pst").exists() && !dir.join("one.pst").exists());
    // A target that cannot be replaced: the export fails, and its temporary file is gone.
    let blocked = dir.join("folder.pst");
    std::fs::create_dir_all(blocked.join("inside")).unwrap();
    assert!(s.run("file.export", &json!({"path": blocked})).is_err());
    assert!(blocked.join("inside").is_dir());
    drop(other);
    assert!(std::fs::read_dir(&dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_save_that_is_not_waited_for_leaves_the_session_to_be_used() {
    let dir = std::env::temp_dir().join(format!("openmailclient-save-elsewhere-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("box.pst");
    run(&mut session(), "file.export", json!({"path": path}));
    let first = std::fs::read(&path).unwrap();
    let mut s = Session::new();
    s.now = now();
    run(&mut s, "file.open", json!({"path": path}));
    assert!(!s.is_enabled("file.stopSave") && s.saving().is_none());
    let (total, _) = folder(&s, "Inbox");
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    run(&mut s, "mail.delete", Value::Null);
    let until_ended = |s: &mut Session| {
        for _ in 0..2000 {
            if s.poll() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the save did not end");
    };

    // A save that is stopped: the file is as it was and the changes are still there.
    let done = run(&mut s, "file.save", json!({"wait": false}));
    assert_eq!((&done["saved"], &done["started"]), (&json!(0), &json!(1)));
    assert!(s.saving().is_some() && s.is_enabled("file.stopSave") && !s.is_enabled("file.save"));
    assert_eq!(run(&mut s, "file.stopSave", Value::Null)["stopping"], 1);
    // While it runs the mailbox is read, and is not changed or closed.
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    assert!(s.open.is_some());
    let asked_while_saving = [s.run("mail.delete", &Value::Null).is_err(), s.run("file.closeStore", &json!({"discard": true})).is_err()];
    until_ended(&mut s);
    // A save this small may be done before it hears it is to stop: either way nothing is half done.
    if s.unsaved() {
        assert!(s.status.contains("was stopped"), "{}", s.status);
        assert_eq!(std::fs::read(&path).unwrap(), first);
        assert_eq!(asked_while_saving, [true, true]);
        assert!(s.saves.is_empty() && s.is_enabled("file.save"));
        let done = run(&mut s, "file.save", json!({"wait": false}));
        assert_eq!(done["started"], 1);
        assert!(s.run("file.save", &Value::Null).is_err(), "a save that is running is not started again");
        until_ended(&mut s);
    }
    assert!(s.status.starts_with("Saved "), "{}", s.status);
    assert!(!s.unsaved() && s.saves.is_empty() && s.saving().is_none());
    assert_eq!(folder(&s, "Inbox").0, total - 1);
    assert!(crate::io::check_structure(&path).unwrap().unwrap().problems.is_empty());
    assert!(std::fs::read_dir(&dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));

    // Leaving waits for a save that runs, and it is ended.
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    run(&mut s, "mail.delete", Value::Null);
    run(&mut s, "file.save", json!({"wait": false}));
    s.finish_saves();
    assert!(!s.unsaved() && s.saves.is_empty());
    let mut other = Session::new();
    run(&mut other, "file.open", json!({"path": path}));
    assert_eq!(folder(&other, "Inbox").0, total - 2);
    // A store that keeps nothing in a file is saved the plain way whatever is asked.
    assert!(session().run("file.save", &json!({"wait": false})).is_err());
    drop((s, other));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_export_that_is_not_waited_for_copies_the_mailbox_as_it_stood() {
    let dir = std::env::temp_dir().join(format!("openmailclient-export-elsewhere-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("box.pst");
    run(&mut session(), "file.export", json!({"path": path}));
    let first = std::fs::read(&path).unwrap();
    let mut s = Session::new();
    s.now = now();
    run(&mut s, "file.open", json!({"path": path}));
    let (total, _) = folder(&s, "Inbox");
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    run(&mut s, "mail.delete", Value::Null);
    let until_ended = |s: &mut Session| {
        for _ in 0..2000 {
            if s.poll() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the export did not end");
    };
    let no_tmp = |dir: &std::path::Path| std::fs::read_dir(dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp"));
    let inbox_of = |path: &std::path::Path| {
        let mut other = Session::new();
        run(&mut other, "file.open", json!({"path": path}));
        folder(&other, "Inbox").0
    };

    // An export that is stopped: nothing is where it was to go.
    let stopped = dir.join("stopped.pst");
    assert!(!s.is_enabled("file.stopSave") && s.working().is_none());
    let done = run(&mut s, "file.export", json!({"path": stopped, "wait": false}));
    assert_eq!(done["started"], true);
    assert!(s.is_enabled("file.stopSave") && s.working().is_some_and(|(what, _)| what == "Exporting to stopped.pst"));
    assert!(s.run("file.export", &json!({"path": stopped, "wait": false})).is_err(), "one export to a file at a time");
    assert!(s.run("file.closeStore", &json!({"discard": true})).is_err());
    assert_eq!(run(&mut s, "file.stopSave", Value::Null)["stopping"], 1);
    until_ended(&mut s);
    // An export this small may be done before it hears it is to stop.
    if s.status.contains("was stopped") {
        assert!(!stopped.exists());
    } else {
        assert!(s.status.starts_with("Exported "), "{}", s.status);
    }
    assert!(s.exports.is_empty() && s.working().is_none() && no_tmp(&dir));

    // One that runs to its end: the mailbox goes on being changed meanwhile, and the file
    // holds it as it stood when the export was asked for.
    let out = dir.join("out.pst");
    assert_eq!(run(&mut s, "file.export", json!({"path": out, "wait": false}))["started"], true);
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    run(&mut s, "mail.delete", Value::Null);
    assert_eq!(folder(&s, "Inbox").0, total - 2);
    until_ended(&mut s);
    assert!(s.status.starts_with("Exported ") && s.status.ends_with(", as it is stored"), "{}", s.status);
    assert!(s.unsaved(), "an export saves nothing");
    assert_eq!(std::fs::read(&path).unwrap(), first);
    let found = crate::io::check_structure(&out).unwrap().unwrap();
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    assert_eq!(inbox_of(&out), total - 1);

    // Leaving waits for an export that runs.
    let last = dir.join("last.pst");
    run(&mut s, "file.export", json!({"path": last, "wait": false}));
    s.finish_saves();
    assert!(s.exports.is_empty() && s.status.starts_with("Exported "), "{}", s.status);
    assert_eq!(inbox_of(&last), total - 2);

    // A session that goes while an export runs leaves no half of a file behind.
    let left = dir.join("left.pst");
    run(&mut s, "file.export", json!({"path": left, "wait": false}));
    drop(s);
    for _ in 0..2000 {
        if no_tmp(&dir) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(no_tmp(&dir) && !left.exists());

    // A mailbox that is in no file is written from what is read, on a thread of its own as
    // well, and as it stood when the export was asked for.
    let plain = dir.join("plain.pst");
    let mut sample = session();
    let (all, _) = folder(&sample, "Inbox");
    assert_eq!(run(&mut sample, "file.export", json!({"path": plain, "wait": false}))["started"], true);
    assert!(sample.working().is_some_and(|(what, _)| what == "Exporting to plain.pst"));
    run(&mut sample, "mail.select", json!({"folder": "Inbox", "index": 0}));
    run(&mut sample, "mail.delete", Value::Null);
    until_ended(&mut sample);
    assert!(sample.status.starts_with("Exported ") && !sample.status.contains("as it is stored"), "{}", sample.status);
    assert_eq!(folder(&sample, "Inbox").0, all - 1);
    assert_eq!(inbox_of(&plain), all);
    assert!(crate::io::check_structure(&plain).unwrap().unwrap().problems.is_empty());

    // So is a data file that is only read (an `.ost`), through a handle of its own.
    let ost = dir.join("box.ost");
    std::fs::write(&ost, &first).unwrap();
    let mut s = Session::new();
    s.now = now();
    run(&mut s, "file.open", json!({"path": ost}));
    let from_ost = dir.join("from-ost.pst");
    assert_eq!(run(&mut s, "file.export", json!({"path": from_ost, "wait": false}))["started"], true);
    until_ended(&mut s);
    assert!(s.status.starts_with("Exported ") && !s.status.contains("as it is stored"), "{}", s.status);
    assert_eq!(inbox_of(&from_ost), total);
    assert_eq!(std::fs::read(&ost).unwrap(), first);
    // One that is told to stop before it has begun writes nothing, and is not tried again
    // another way.
    let never = dir.join("never.pst");
    run(&mut s, "file.export", json!({"path": never, "wait": false}));
    run(&mut s, "file.stopSave", Value::Null);
    until_ended(&mut s);
    if s.status.contains("was stopped") {
        assert!(!never.exists());
    } else {
        assert!(s.status.starts_with("Exported "), "{}", s.status);
    }
    assert!(no_tmp(&dir));
    drop(s);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_file_as_it_was_is_kept_beside_a_first_save_unless_asked_not_to() {
    let dir = std::env::temp_dir().join(format!("openmailclient-keep-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for keep in [true, false] {
        let path = dir.join(if keep { "kept.pst" } else { "alone.pst" });
        run(&mut session(), "file.export", json!({"path": path}));
        let first = std::fs::read(&path).unwrap();
        let mut s = Session::new();
        s.now = now();
        assert!(s.keep_original);
        assert_eq!(run(&mut s, "file.keepOriginal", json!({"on": keep}))["on"], keep);
        run(&mut s, "file.open", json!({"path": path}));
        run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
        run(&mut s, "mail.delete", Value::Null);
        run(&mut s, "file.save", Value::Null);
        let beside = openmailclient_pst::original_beside(&path);
        assert_eq!(beside.exists(), keep);
        // Nothing else is left beside the file, however it was saved.
        assert!(!openmailclient_pst::undo_beside(&path).exists() && !openmailclient_pst::journal_beside(&path).exists());
        if keep {
            assert_eq!(std::fs::read(&beside).unwrap(), first);
        }
        assert_ne!(std::fs::read(&path).unwrap(), first);
    }
    // Two data files, and the one file kept.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_save_goes_into_the_data_file_unless_asked_not_to() {
    let dir = std::env::temp_dir().join(format!("openmailclient-onto-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("box.pst");
    run(&mut session(), "file.export", json!({"path": path}));
    let mut s = Session::new();
    s.now = now();
    assert!(s.save_in_place);
    run(&mut s, "file.open", json!({"path": path}));
    let change = |s: &mut Session| {
        run(s, "mail.select", json!({"folder": "Inbox", "index": 0}));
        run(s, "mail.delete", Value::Null);
        run(s, "file.save", Value::Null)
    };
    // The first save keeps the file as it was, and so writes a new one; the next goes into it.
    let done = change(&mut s);
    assert_eq!((done["saved"].clone(), done["inPlace"].clone()), (json!(1), json!(0)));
    let done = change(&mut s);
    assert_eq!((done["saved"].clone(), done["inPlace"].clone()), (json!(1), json!(1)));
    assert_eq!(run(&mut s, "file.saveInPlace", Value::Null)["on"], false);
    assert_eq!(s.settings(None)["saveInPlace"], false);
    let done = change(&mut s);
    assert_eq!((done["saved"].clone(), done["inPlace"].clone()), (json!(1), json!(0)));
    assert_eq!(run(&mut s, "file.saveInPlace", json!({"on": true}))["on"], true);
    // One that is not waited for goes into the file as well.
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    run(&mut s, "mail.delete", Value::Null);
    assert_eq!(run(&mut s, "file.save", json!({"wait": false}))["started"], 1);
    s.finish_saves();
    assert!(s.saves.is_empty() && s.inspect()["stores"][0]["unsaved"] == false, "{}", s.status);
    // The data file, the file as it first was, and nothing else.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    let mut again = Session::new();
    run(&mut again, "file.open", json!({"path": path}));
    assert_eq!(folder(&again, "Inbox").0, folder(&s, "Inbox").0);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn changes_to_an_open_pst_are_saved_into_it() {
    let dir = std::env::temp_dir().join(format!("openmailclient-save-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("box.pst");
    run(&mut session(), "file.export", json!({"path": path}));
    let first = std::fs::read(&path).unwrap();

    let mut s = Session::new();
    s.now = now();
    assert!(!s.is_enabled("file.save"));
    run(&mut s, "file.open", json!({"path": path}));
    assert!(s.status.ends_with("(changes are kept when saved)"), "{}", s.status);
    assert_eq!(s.inspect()["stores"][0]["unsaved"], false);
    let (total, _) = folder(&s, "Inbox");

    // Changes through the same commands as anywhere else: flag, move, delete, a new folder.
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    let flagged = s.selected.unwrap();
    let flag_was = s.open.as_ref().unwrap().flag();
    run(&mut s, "mail.flag", Value::Null);
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 1}));
    let moved = s.selected.unwrap();
    run(&mut s, "mail.move", json!({"folder": "Sent Items"}));
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 1}));
    run(&mut s, "mail.delete", Value::Null);
    run(&mut s, "folder.new", json!({"name": "Kept here"}));
    assert!(s.is_enabled("file.save") && s.unsaved());
    assert_eq!(s.inspect()["stores"][0]["unsaved"], true);
    assert_eq!(std::fs::read(&path).unwrap(), first, "nothing is written before it is asked for");

    // Closing does not throw the changes away, nor write them, unless told which.
    assert!(s.run("file.closeStore", &Value::Null).is_err());
    assert_eq!((s.stores.len(), std::fs::read(&path).unwrap() == first), (1, true));

    let done = run(&mut s, "file.save", Value::Null);
    assert_eq!(done["saved"], 1);
    assert!(!s.unsaved() && !s.is_enabled("file.save"));
    assert!(s.status.starts_with("Saved "));
    // The file as it was is kept beside the new one, and nothing else is.
    let kept = std::path::PathBuf::from(done["kept"][0].as_str().unwrap());
    assert_eq!(std::fs::read(&kept).unwrap(), first);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    assert!(crate::io::check_structure(&path).unwrap().unwrap().problems.is_empty());
    assert!(s.run("file.save", &Value::Null).is_err(), "there is nothing to save");
    // The session goes on from what it showed.
    assert_eq!(folder(&s, "Inbox").0, total - 2);
    let kept_folders: Vec<String> = s.folders[0].iter().map(|f| f.name.clone()).collect();

    let mut other = Session::new();
    run(&mut other, "file.open", json!({"path": path}));
    assert_eq!(other.folders[0].iter().map(|f| f.name.clone()).collect::<Vec<_>>(), kept_folders);
    assert_eq!(folder(&other, "Inbox").0, total - 2);
    let inbox = other.folders[0].iter().find(|f| f.name == "Inbox").unwrap().id;
    let sent = other.folders[0].iter().find(|f| f.name == "Sent Items").unwrap().id;
    let again = other.stores[0].load(inbox, openmailclient_model::ItemId(flagged)).unwrap();
    assert_ne!(again.flag(), flag_was);
    assert!(again.is_read());
    assert!(other.stores[0].list(sent).unwrap().iter().any(|m| m.id == moved));
    drop(other);

    // A change, then closing with it saved; and one thrown away.
    run(&mut s, "folder.new", json!({"name": "Second"}));
    run(&mut s, "file.closeStore", json!({"save": true}));
    assert!(s.stores.is_empty());
    run(&mut s, "file.open", json!({"path": path}));
    assert!(s.folders[0].iter().any(|f| f.name == "Second"));
    let before = std::fs::read(&path).unwrap();
    run(&mut s, "folder.new", json!({"name": "Third"}));
    run(&mut s, "file.closeStore", json!({"discard": true}));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::read(&kept).unwrap(), first, "the file as it first was is not written over");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn settings_are_kept_between_runs() {
    let dir = std::env::temp_dir().join(format!("openmailclient-settings-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("settings.json");
    let at = |path: &std::path::Path| json!({"path": path});

    // The first run: no file, nothing changes, and that is not an error.
    let mut s = Session::new();
    let fresh = s.settings(None);
    assert_eq!(run(&mut s, "settings.load", at(&file))["found"], false);
    assert_eq!(s.settings(None), fresh);

    // What was set is written, with what the window keeps for itself, and nothing is left beside it.
    let pst = dir.join("kept.pst");
    run(&mut session(), "file.export", json!({"path": pst}));
    run(&mut s, "file.open", json!({"path": pst}));
    run(
        &mut s,
        "file.pageSetup",
        json!({"paper": "letter", "orientation": "landscape", "margins": {"top": 12.5, "left": 30}, "footer": "[Page]", "details": false}),
    );
    run(&mut s, "view.readingPane", json!({"position": "bottom"}));
    run(&mut s, "view.folderPane", json!({"show": false}));
    assert_eq!(run(&mut s, "file.keepOriginal", Value::Null)["on"], false);
    assert_eq!(run(&mut s, "file.autoSave", json!({"minutes": 12}))["minutes"], 12);
    let mut save = at(&file);
    save["ui"] = json!({"theme": "dark"});
    run(&mut s, "settings.save", save);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    let kept = s.settings(Some(&json!({"theme": "dark"})));
    assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(&file).unwrap()).unwrap(), kept);
    assert_eq!(kept["dataFiles"].as_array().unwrap().len(), 1);

    // The next run takes all of it; the data file only when it is asked to open them again.
    let mut next = Session::new();
    let took = run(&mut next, "settings.load", at(&file));
    assert_eq!((took["found"].clone(), took["ignored"].clone(), took["opened"].clone()), (json!(true), json!(0), json!(0)));
    assert_eq!(took["ui"], json!({"theme": "dark"}));
    assert!(next.stores.is_empty());
    assert_eq!((&next.page_setup, next.reading_pane, next.folder_pane), (&s.page_setup, crate::ReadingPane::Bottom, false));
    assert!(!next.keep_original);
    assert_eq!(next.autosave_min, 12);
    let mut again = at(&file);
    again["reopen"] = json!(true);
    let took = run(&mut next, "settings.load", again.clone());
    assert_eq!((took["opened"].clone(), took["missing"].clone()), (json!(1), json!(0)));
    assert_eq!(next.settings(Some(&json!({"theme": "dark"}))), kept);
    assert!(!next.unsaved());

    // A data file that is gone is counted and said, and costs nothing else. Only data files are
    // opened again, whatever the list names.
    let mut all: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    all["dataFiles"] = json!([dir.join("gone.pst"), file, 7, pst]);
    std::fs::write(&file, all.to_string()).unwrap();
    let mut third = Session::new();
    let took = run(&mut third, "settings.load", again.clone());
    assert_eq!((took["opened"].clone(), took["missing"].clone()), (json!(1), json!(3)));
    assert_eq!(third.stores.len(), 1);
    assert!(third.status.starts_with("3 of the data files"), "{}", third.status);

    // A part that makes no sense is left out; the others are taken.
    std::fs::write(
        &file,
        json!({
            "version": 99,
            "pageSetup": {"paper": "papyrus", "orientation": "landscape", "margins": {"top": -4}, "header": [1, 2], "style": 9},
            "readingPane": "ceiling",
            "folderPane": false,
            "dataFiles": "everything",
            "ui": [1, 2, 3],
        })
        .to_string(),
    )
    .unwrap();
    let mut odd = Session::new();
    let default = odd.page_setup.clone();
    let took = run(&mut odd, "settings.load", again.clone());
    assert!(took["ignored"].as_u64().unwrap() >= 3 && took["applied"].as_u64().unwrap() >= 2, "{took}");
    assert!(took.get("ui").is_none());
    assert!(odd.page_setup.landscape && !odd.folder_pane);
    assert_eq!(
        (odd.page_setup.paper.name(), odd.page_setup.margins, odd.reading_pane),
        (default.paper.name(), default.margins, crate::ReadingPane::Right)
    );

    // A file that is not ours: nothing is taken, nothing fails, and it is not touched.
    let big = format!("{{\"folderPane\": false, \"x\": \"{}\"}}", "a".repeat(1 << 20));
    for junk in [&b""[..], b"{", b"[1, 2]", b"\xff\xfe\x00junk", b"null", big.as_bytes()] {
        std::fs::write(&file, junk).unwrap();
        let mut s = Session::new();
        let took = run(&mut s, "settings.load", again.clone());
        assert_eq!((took["found"].clone(), took["applied"].clone()), (json!(true), json!(0)));
        assert_eq!(s.settings(None), fresh);
        assert_eq!(std::fs::read(&file).unwrap(), junk);
    }

    // Settings go to a .json file and nowhere else, and a failed write leaves no file behind.
    let mut s = Session::new();
    for path in [json!(pst), json!(dir.join("settings")), json!(7), Value::Null] {
        for id in ["settings.save", "settings.load"] {
            assert!(matches!(s.run(id, &json!({"path": path})), Err(EngineError::BadParams(_))), "{id} {path}");
        }
    }
    assert!(matches!(s.run("settings.save", &at(&dir.join("no-such-folder").join("s.json"))), Err(EngineError::Io(_))));
    // What the window keeps is carried only when it is an object of a sensible size.
    for ui in [json!("dark"), json!({"x": "y".repeat(20_000)})] {
        let mut save = at(&file);
        save["ui"] = ui;
        run(&mut s, "settings.save", save);
        assert!(serde_json::from_slice::<Value>(&std::fs::read(&file).unwrap()).unwrap().get("ui").is_none());
    }
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn unsaved_changes_come_back_when_the_data_file_is_opened_again() {
    let dir = std::env::temp_dir().join(format!("openmailclient-journal-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("box.pst");
    run(&mut session(), "file.export", json!({"path": path}));
    let first = std::fs::read(&path).unwrap();
    let names = |s: &Session| s.folders[0].iter().map(|f| (f.name.clone(), f.total, f.unread)).collect::<Vec<_>>();

    let mut s = Session::new();
    s.now = now();
    let opened = run(&mut s, "file.open", json!({"path": path}));
    assert_eq!((opened["recovered"].clone(), opened["lost"].clone(), opened["staleJournal"].clone()), (json!(0), json!(0), json!(false)));
    assert_eq!(s.inspect()["stores"][0]["journalled"], false, "nothing is unsaved yet");
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    run(&mut s, "mail.flag", Value::Null);
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 1}));
    run(&mut s, "mail.move", json!({"folder": "Sent Items"}));
    run(&mut s, "folder.new", json!({"name": "Kept here"}));
    assert!(s.unsaved() && !s.unjournalled());
    assert_eq!(s.inspect()["stores"][0]["journalled"], true);
    let want = names(&s);
    // The run ends here without a save: the data file is as it was.
    drop(s);
    assert_eq!(std::fs::read(&path).unwrap(), first);

    // Opened without asking for them, the changes stay where they are.
    let mut cold = Session::new();
    let opened = run(&mut cold, "file.open", json!({"path": path, "recover": false}));
    assert_eq!(opened["recovered"], 0);
    assert!(!cold.unsaved() && names(&cold) != want);
    drop(cold);

    let mut next = Session::new();
    next.now = now();
    let opened = run(&mut next, "file.open", json!({"path": path}));
    assert!(opened["recovered"].as_u64().unwrap() >= 3 && opened["lost"] == 0, "{opened}");
    assert!(next.status.contains("were brought back"), "{}", next.status);
    assert!(next.unsaved() && next.is_enabled("file.save"));
    assert_eq!(names(&next), want);
    assert_eq!(std::fs::read(&path).unwrap(), first, "bringing changes back writes nothing to the data file");

    // Thrown away, they are gone for good: the next opening finds nothing.
    run(&mut next, "file.closeStore", json!({"discard": true}));
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    let opened = run(&mut next, "file.open", json!({"path": path}));
    assert_eq!(opened["recovered"], 0);
    assert!(!next.unsaved() && names(&next) != want);

    // Saved, they are in the file and the journal is gone.
    run(&mut next, "folder.new", json!({"name": "Saved this time"}));
    let want = names(&next);
    run(&mut next, "file.save", Value::Null);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    drop(next);
    let mut last = Session::new();
    assert_eq!(run(&mut last, "file.open", json!({"path": path}))["recovered"], 0);
    assert_eq!(names(&last), want);
    drop(last);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_damaged_data_file_is_exported_as_far_as_it_reads() {
    let dir = std::env::temp_dir().join(format!("openmailclient-export-damage-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let good = dir.join("good.pst");
    run(&mut session(), "file.export", json!({"path": good}));
    let bytes = std::fs::read(&good).unwrap();
    // Damage in one place after another: the file opens or it does not, and when it opens
    // the export is a copy as stored or, failing that, what the model could read.
    let (bad, out) = (dir.join("bad.pst"), dir.join("out.pst"));
    let (mut copied, mut read) = (0, 0);
    for at in (0x4400..bytes.len().saturating_sub(64)).step_by(4096) {
        let mut hurt = bytes.clone();
        for b in &mut hurt[at..at + 64] {
            *b = !*b;
        }
        std::fs::write(&bad, &hurt).unwrap();
        let mut s = Session::new();
        if s.run("file.open", &json!({"path": bad, "recover": false})).is_err() {
            continue;
        }
        let _ = std::fs::remove_file(&out);
        match s.run("file.export", &json!({"path": out})) {
            Ok(done) if done["copied"] == true => copied += 1,
            Ok(done) => {
                read += 1;
                assert!(s.status.contains("could not be copied as it is stored"), "{}", s.status);
                assert!(done["folders"].as_u64().unwrap() >= 1);
                assert!(crate::io::check_structure(&out).unwrap().unwrap().problems.is_empty());
            }
            Err(_) => assert!(!out.exists()),
        }
        assert_eq!(std::fs::read(&bad).unwrap(), hurt, "the source is only read");
    }
    assert!(read > 0, "no damage made the copy fall back ({copied} copied)");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn unsaved_changes_are_journalled_elsewhere_when_the_folder_cannot_be_written() {
    let dir = std::env::temp_dir().join(format!("openmailclient-journal-elsewhere-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (path, ours) = (dir.join("box.pst"), dir.join("ours"));
    run(&mut session(), "file.export", json!({"path": path}));
    // Nothing can be written where the journal would be: a folder stands in its place.
    std::fs::create_dir(dir.join("box.pst.openmailclient-journal")).unwrap();

    let mut s = Session::new();
    s.now = now();
    s.journal_dir = Some(ours.clone());
    assert!(!s.settle(), "nothing is open");
    run(&mut s, "file.open", json!({"path": path}));
    run(&mut s, "folder.new", json!({"name": "Kept elsewhere"}));
    assert!(s.unsaved() && !s.unjournalled(), "{}", s.status);
    assert_eq!(std::fs::read_dir(&ours).unwrap().count(), 1);
    assert!(s.settle() && !s.settle());
    drop(s);

    let mut next = Session::new();
    next.now = now();
    next.journal_dir = Some(ours.clone());
    let opened = run(&mut next, "file.open", json!({"path": path}));
    assert_eq!((opened["recovered"].clone(), opened["lost"].clone(), opened["lostMore"].clone()), (json!(1), json!(0), json!(false)));
    assert!(next.folders[0].iter().any(|f| f.name == "Kept elsewhere"));
    run(&mut next, "file.closeStore", json!({"discard": true}));
    assert_eq!(std::fs::read_dir(&ours).unwrap().count(), 0);
    drop(next);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_mailbox_is_exported_as_an_ansi_file_and_such_a_file_stays_one() {
    let dir = std::env::temp_dir().join(format!("openmailclient-export-ansi-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // HEADER, wVer ([MS-PST] 2.2.2.6): 14 an ANSI file, 23 a Unicode one.
    let kind_of = |path: &std::path::Path| {
        let bytes = std::fs::read(path).unwrap();
        u16::from_le_bytes([bytes[10], bytes[11]])
    };
    let sound = |path: &std::path::Path| {
        let found = crate::io::check_structure(path).unwrap().unwrap();
        assert!(found.problems.is_empty(), "{:?}", found.problems);
    };
    let mut s = session();
    let old = dir.join("old.pst");
    let done = run(&mut s, "file.export", json!({"path": old, "kind": "ansi"}));
    assert_eq!((&done["copied"], &done["unreadable"]), (&json!(false), &json!(0)));
    assert_eq!(kind_of(&old), 14);
    sound(&old);
    // With 4K pages (wVer 36, measured on files Outlook wrote), and back again: a copy of it
    // as it is stored keeps the kind, one asked for as Unicode is not a copy.
    let big = dir.join("big.pst");
    assert_eq!(run(&mut s, "file.export", json!({"path": big, "kind": "4k"}))["copied"], false);
    assert_eq!(kind_of(&big), 36);
    sound(&big);
    let mut from_big = Session::new();
    from_big.now = now();
    run(&mut from_big, "file.open", json!({"path": big}));
    assert_eq!(run(&mut from_big, "file.export", json!({"path": dir.join("big2.pst")}))["copied"], true);
    assert_eq!(kind_of(&dir.join("big2.pst")), 36);
    assert_eq!(run(&mut from_big, "file.export", json!({"path": dir.join("wide.pst"), "kind": "unicode"}))["copied"], false);
    assert_eq!(kind_of(&dir.join("wide.pst")), 23);
    sound(&dir.join("big2.pst"));
    assert!(s.run("file.export", &json!({"path": dir.join("no.pst"), "kind": "8k"})).is_err());
    assert!(s.run("file.export", &json!({"path": dir.join("no.pst"), "kind": 14})).is_ok_and(|v| v["copied"] == false));
    assert_eq!(kind_of(&dir.join("no.pst")), 23, "a kind that is not text is no kind named");

    // It opens with what the sample has, takes changes, and is saved as the kind it is.
    let mut other = Session::new();
    other.now = now();
    run(&mut other, "file.open", json!({"path": old}));
    assert!(other.stores[0].writable());
    assert_eq!(folder(&other, "Inbox"), folder(&s, "Inbox"));
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    run(&mut other, "mail.select", json!({"folder": "Inbox", "index": 0}));
    let (a, b) = (s.open.clone().unwrap(), other.open.clone().unwrap());
    assert_eq!((b.subject(), b.sender_name(), b.body_text(), b.body_html()), (a.subject(), a.sender_name(), a.body_text(), a.body_html()));
    assert_eq!(
        b.attachments.iter().map(|x| (x.name(), x.data.clone())).collect::<Vec<_>>(),
        a.attachments.iter().map(|x| (x.name(), x.data.clone())).collect::<Vec<_>>()
    );
    let (total, _) = folder(&other, "Inbox");
    run(&mut other, "mail.delete", Value::Null);
    assert_eq!(run(&mut other, "file.save", Value::Null)["saved"], 1);
    assert_eq!(kind_of(&old), 14);
    sound(&old);
    assert_eq!(folder(&other, "Inbox").0, total - 1);

    // Exported with no kind named it is copied as it is stored, and so an ANSI file; named
    // the other kind it is written from what is read, on a thread of its own as well.
    let same = dir.join("same.pst");
    assert_eq!(run(&mut other, "file.export", json!({"path": same}))["copied"], true);
    assert_eq!(run(&mut other, "file.export", json!({"path": same, "kind": "ansi"}))["copied"], true);
    assert_eq!(kind_of(&same), 14);
    sound(&same);
    let wide = dir.join("wide.pst");
    assert_eq!(run(&mut other, "file.export", json!({"path": wide, "kind": "unicode"}))["copied"], false);
    assert_eq!(kind_of(&wide), 23);
    sound(&wide);
    let narrow = dir.join("narrow.pst");
    let mut third = Session::new();
    run(&mut third, "file.open", json!({"path": wide}));
    assert_eq!(run(&mut third, "file.export", json!({"path": narrow, "kind": "ansi", "wait": false}))["started"], true);
    for _ in 0..2000 {
        if third.poll() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(third.status.starts_with("Exported ") && !third.status.contains("as it is stored"), "{}", third.status);
    assert_eq!(kind_of(&narrow), 14);
    sound(&narrow);
    let mut fourth = Session::new();
    run(&mut fourth, "file.open", json!({"path": narrow}));
    assert_eq!(folder(&fourth, "Inbox").0, total - 1);
    drop((other, third, fourth));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn table_style_prints_the_columns_the_headings_and_the_rows_asked_for() {
    use crate::print::{Field, Listing, Rows};
    let mut s = session();
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 2}));
    let chosen = s.rows[2].clone();
    // What is asked for is kept by the setup, told back, and refused when it is not known.
    let told = run(&mut s, "file.pageSetup", json!({"style": "table", "columns": ["Subject", "from", "received", "subject", "categories"]}));
    assert_eq!(told["columns"], json!(["subject", "from", "date", "categories"]), "a column named twice is there once");
    assert_eq!(s.print_list.columns, [Field::Subject, Field::From, Field::Date, Field::Categories]);
    for bad in [
        json!({"columns": []}),
        json!({"columns": ["colour"]}),
        json!({"columns": "all"}),
        json!({"columns": [7]}),
        json!({"groups": "yes"}),
        json!({"rows": "some"}),
        json!({"rows": 2}),
    ] {
        assert!(matches!(s.run("file.pageSetup", &bad), Err(EngineError::BadParams(_))), "{bad}");
        assert!(matches!(s.run("file.printPreview", &bad), Err(EngineError::BadParams(_)) | Err(EngineError::Format(_))), "{bad}");
    }
    assert!(s.run("file.pageSetup", &json!({"columns": vec!["size"; 65]})).is_err());
    assert_eq!(s.print_list.columns.len(), 4, "a setup that is refused changes nothing");
    assert_eq!(Field::ALL.map(|f| Field::named(f.name())), Field::ALL.map(Some));

    // In the installed fonts, or the word that none is installed.
    let text_of = |s: &Session, listing: &Listing| {
        crate::print::laid_as(s, crate::print::Style::Table, &s.page_setup, listing, crate::print::Links::default())
            .map(|laid| laid.printed.pages.iter().map(|page| page.text()).collect::<Vec<_>>().join("\n"))
    };
    let kept = s.print_list.clone();
    match text_of(&s, &kept) {
        Ok(text) => {
            let lines: Vec<&str> = text.lines().collect();
            assert_eq!(lines[3], "SubjectFromReceivedCategories", "{text}");
            // A heading stands over each group of the list as it is arranged, before its
            // first row and after the last of the group before it.
            let groups = s.groups();
            assert!(groups.len() > 1);
            let mut from = 0;
            for group in &groups {
                let mut says = format!("{}: {} item{}", group.label, group.count, if group.count == 1 { "" } else { "s" });
                if group.unread > 0 {
                    says.push_str(&format!(", {} unread", group.unread));
                }
                let at = text[from..].find(&says).unwrap_or_else(|| panic!("{says} after {from} in {text}"));
                let first = text[from + at..].find(&s.rows[group.start].subject).unwrap();
                from += at + first;
            }
            // Arranged another way, the headings are those of that arrangement.
            run(&mut s, "view.sortBy", json!({"key": "from"}));
            let by_name = text_of(&s, &kept).unwrap();
            assert!(by_name.contains(&format!("{}: ", s.groups()[0].label)) && !by_name.contains("Today: "), "{by_name}");
            run(&mut s, "view.sortBy", json!({"key": "date", "descending": true}));
            // Without them, the rows alone.
            let plain = text_of(&s, &Listing { groups: false, ..kept.clone() }).unwrap();
            assert!(!plain.contains(&format!("{}: ", groups[0].label)) && plain.lines().count() < lines.len(), "{plain}");
            // The selected message alone: its row, no heading, and the page says which it is.
            let one = text_of(&s, &Listing { rows: Rows::Selected, ..kept.clone() }).unwrap();
            assert!(one.contains(&chosen.subject) && one.contains("The selected message, 3 of 9"), "{one}");
            assert!(s.rows.iter().filter(|row| one.contains(&row.subject)).count() == 1 && !one.contains(" items"), "{one}");
            // Every column there is, at once, on a sheet with room for them: none is lost.
            let all = Listing { columns: Field::ALL.to_vec(), ..Listing::default() };
            run(&mut s, "file.pageSetup", json!({"paper": "a3", "orientation": "landscape"}));
            let wide = text_of(&s, &all).unwrap();
            assert!(
                wide.replace(
                    "
", ""
                )
                .contains("FromFromToSubjectReceivedSizeNotesCategoriesImportanceFlagAttachmentPreview"),
                "{wide}"
            );
            run(&mut s, "file.pageSetup", json!({"paper": "a4", "orientation": "portrait"}));
            // Through the commands: for one print, and kept.
            let told = run(&mut s, "file.printPreview", json!({"rows": "selected", "columns": ["subject"], "groups": false}));
            assert_eq!((told["pages"].as_u64(), told["style"].as_str()), (Some(1), Some("table")));
            assert_eq!(s.print_list, kept, "a look keeps nothing");
        }
        Err(e) => assert!(matches!(e, EngineError::Format(_)), "{e}"),
    }
    // With no message selected there is no selected row to print, and it says so.
    run(&mut s, "file.pageSetup", json!({"rows": "selected"}));
    assert_eq!(s.print_list.rows, Rows::Selected);
    s.selected = None;
    assert!(matches!(text_of(&s, &s.print_list), Err(EngineError::BadParams(_))));
    // The columns and the headings are kept with the settings; which rows is of the moment.
    run(&mut s, "file.pageSetup", json!({"groups": false}));
    let settings = crate::cmd::settings::current(&s, None);
    assert_eq!(
        (&settings["pageSetup"]["columns"], &settings["pageSetup"]["groups"]),
        (&json!(["subject", "from", "date", "categories"]), &json!(false))
    );
    assert!(settings["pageSetup"].get("rows").is_none());
    assert_eq!(run(&mut s, "file.pageSetup", json!({"reset": true}))["columns"], json!(["who", "subject", "date", "size", "notes"]));
    assert_eq!(s.print_list, Listing::default());
}

/// Unsaved changes are saved on their own once they have waited, at a quiet moment, and never
/// for the first time.
#[test]
fn unsaved_changes_are_saved_on_their_own_but_never_first() {
    let dir = std::env::temp_dir().join(format!("openmailclient-autosave-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("box.pst");
    run(&mut session(), "file.export", json!({"path": path}));
    let inbox = |s: &Session| s.folders[0].iter().find(|f| f.kind == FolderKind::Inbox).unwrap().total;
    let mut s = Session::new();
    s.now = now();
    assert_eq!(s.autosave_min, 5);
    run(&mut s, "file.open", json!({"path": path}));
    let was = inbox(&s);
    let delete = |s: &mut Session| {
        run(s, "mail.select", json!({"folder": "Inbox", "index": 0}));
        run(s, "mail.delete", Value::Null);
    };
    const MIN: u64 = 60_000;

    // A file never saved here is not saved on its own, however long it waits.
    delete(&mut s);
    assert!(!s.autosave(0));
    assert!(!s.autosave(60 * MIN));
    assert!(s.unsaved() && !openmailclient_pst::original_beside(&path).exists());

    // Once saved, the next changes are saved on their own after five minutes and a quiet moment.
    run(&mut s, "file.save", Value::Null);
    let t = 70 * MIN;
    delete(&mut s);
    assert!(!s.autosave(t));
    assert!(!s.autosave(t + 5 * MIN - 1));
    assert!(s.autosave(t + 5 * MIN));
    assert!(s.is_enabled("file.stopSave"));
    s.finish_saves();
    assert!(!s.unsaved());
    assert!(s.status.ends_with("on its own"), "{}", s.status);
    // Nothing waits now.
    assert!(!s.autosave(t + 20 * MIN));

    // In use all along: not while something was done a moment ago, but at three times as long,
    // at a pause of a second.
    let t = 100 * MIN;
    delete(&mut s);
    assert!(!s.autosave(t));
    for k in 1..=14u64 {
        run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
        assert!(!s.autosave(t + k * MIN));
    }
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    assert!(!s.autosave(t + 15 * MIN));
    assert!(!s.autosave(t + 15 * MIN + 999));
    assert!(s.autosave(t + 15 * MIN + 1000));
    s.finish_saves();
    assert!(!s.unsaved());

    // Off: nothing is saved on its own.
    assert_eq!(run(&mut s, "file.autoSave", json!({"minutes": 0}))["minutes"], 0);
    delete(&mut s);
    assert!(!s.autosave(200 * MIN) && !s.autosave(300 * MIN));
    assert_eq!(run(&mut s, "file.autoSave", Value::Null)["minutes"], 5);
    assert!(s.run("file.autoSave", &json!({"minutes": 241})).is_err());
    assert!(s.run("file.autoSave", &json!({"minutes": -1})).is_err());
    assert_eq!(s.autosave_min, 5);
    run(&mut s, "file.save", Value::Null);
    drop(s);

    // The next run: the file as it was is beside it from the first save, so it is saved on its own.
    assert!(openmailclient_pst::original_beside(&path).exists());
    let mut s = Session::new();
    s.now = now();
    run(&mut s, "file.open", json!({"path": path}));
    assert_eq!(inbox(&s), was - 4);
    delete(&mut s);
    assert!(!s.autosave(0));
    assert!(s.autosave(5 * MIN));
    s.finish_saves();
    drop(s);
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    assert_eq!(inbox(&s), was - 5);
    drop(s);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_data_files_rows_get_their_preview_from_the_message_and_sent_mail_its_recipients() {
    let dir = std::env::temp_dir().join(format!("openmailclient-preview-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = session();
    let path = dir.join("sample.pst");
    run(&mut s, "file.export", json!({"path": path}));
    let inbox = |s: &Session| s.folders[0].iter().find(|f| f.kind == openmailclient_model::FolderKind::Inbox).unwrap().id;
    let at = inbox(&s);
    s.select_folder(0, at).unwrap();
    let want: Vec<(u64, String, bool)> = s.rows.iter().map(|r| (r.time as u64, r.preview.clone(), r.preview_whole)).collect();
    assert!(want.iter().all(|(_, p, _)| !p.is_empty()));

    let mut other = Session::new();
    other.now = s.now;
    run(&mut other, "file.open", json!({"path": path}));
    let at = inbox(&other);
    other.select_folder(0, at).unwrap();
    // A folder's table holds no body: the rows are listed without one, then read a few at a time.
    assert!(other.rows.iter().all(|r| r.preview.is_empty()));
    let ids: Vec<u64> = other.rows.iter().map(|r| r.id).collect();
    let mut rounds = 0;
    while other.rows.iter().any(|r| r.preview.is_empty()) {
        assert!(other.fill_previews(&ids));
        rounds += 1;
        assert!(rounds < 10);
    }
    let got: Vec<(u64, String, bool)> = other.rows.iter().map(|r| (r.time as u64, r.preview.clone(), r.preview_whole)).collect();
    assert_eq!(got, want);
    // What was read is kept: the rows read again from the file have it at once, and nothing is
    // read twice.
    other.reload_rows().unwrap();
    assert!(other.rows.iter().all(|r| !r.preview.is_empty()));
    assert!(!other.fill_previews(&ids));
    // Ids not listed, or asked for again and again, change nothing.
    assert!(!other.fill_previews(&[u64::MAX, 0, u64::MAX]));
    drop(other);
    let _ = std::fs::remove_dir_all(&dir);

    // In Sent Items each row says whom the message went to: the list shows `to`, which the
    // sample's sent mail has.
    let sent = s.folders[0].iter().find(|f| f.kind == openmailclient_model::FolderKind::Sent).unwrap().id;
    s.select_folder(0, sent).unwrap();
    assert!(!s.rows.is_empty() && s.rows.iter().all(|r| !r.to.is_empty()));
}

/// A message is printed as its body alone when asked: no name and fields at its head, no
/// header or footer; and each can be left out by itself.
#[test]
fn a_message_is_printed_as_its_body_alone() {
    use crate::print::{Links, laid_as};
    let mut s = session();
    run(&mut s, "mail.select", json!({"folder": "Receipts", "index": 0}));
    let text = |s: &Session| {
        let laid = laid_as(s, s.print_style, &s.page_setup, &s.print_list, Links::default()).unwrap();
        laid.printed.pages.iter().map(|page| page.text()).collect::<Vec<_>>().join(
            "
",
        )
    };
    let all = text(&s);
    assert!(all.contains("From:") && all.contains("Subject:") && all.contains("Riverside Car Park"), "{all}");
    let now = run(&mut s, "file.pageSetup", json!({"bodyOnly": true}));
    assert_eq!((&now["details"], &now["headerFooter"], &now["bodyOnly"]), (&json!(false), &json!(false), &json!(true)));
    let body = text(&s);
    assert!(!body.contains("From:") && !body.contains("Subject:") && body.contains("Riverside Car Park"), "{body}");
    // What the footer says is kept, only not printed; it comes back with it.
    assert_eq!(now["footer"]["center"], "[Page of Pages]");
    let now = run(&mut s, "file.pageSetup", json!({"headerFooter": true}));
    assert_eq!((&now["details"], &now["headerFooter"], &now["bodyOnly"]), (&json!(false), &json!(true), &json!(false)));
    assert!(!text(&s).contains("From:"));
    // A print asked for once with it does not change the setup.
    run(&mut s, "file.pageSetup", json!({"reset": true}));
    let dir = std::env::temp_dir().join(format!("openmailclient-body-only-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = dir.join("body.pdf");
    run(&mut s, "file.print", json!({"path": pdf, "bodyOnly": true}));
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
    assert!(s.page_setup.details && s.page_setup.edges);
    std::fs::remove_dir_all(&dir).unwrap();
}
