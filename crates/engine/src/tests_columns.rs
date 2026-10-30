//! The columns of the message list: chosen, moved and taken away, printed by Table Style as
//! they are shown, and kept between runs.

use crate::Session;
use openmailclient_model::time;
use serde_json::{Value, json};

fn session() -> Session {
    let mut s = Session::new();
    s.now = time::from_civil(2026, 10, 7, 14, 30, 0, 0);
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.run(id, &params).unwrap_or_else(|e| panic!("{id} failed: {e}"))
}

fn names(v: &Value) -> Vec<&str> {
    v["columns"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect()
}

#[test]
fn columns_are_shown_moved_and_hidden_and_table_style_prints_the_same() {
    let mut s = session();
    let first = run(&mut s, "view.columns", Value::Null);
    assert_eq!(names(&first), ["importance", "attachment", "who", "subject", "date", "size", "categories", "flag"]);
    let got = run(&mut s, "view.columns", json!({ "hide": "size" }));
    assert!(!names(&got).contains(&"size"));
    let got = run(&mut s, "view.columns", json!({ "show": "preview", "at": 4 }));
    assert_eq!(names(&got)[4], "preview");
    // Shown again where it already is: moved, not there twice.
    let got = run(&mut s, "view.columns", json!({ "show": "preview" }));
    assert_eq!((names(&got).last().copied(), names(&got).iter().filter(|c| **c == "preview").count()), (Some("preview"), 1));
    let got = run(&mut s, "view.columns", json!({ "move": "subject", "at": 0 }));
    assert_eq!(names(&got)[0], "subject");
    let got = run(&mut s, "view.columns", json!({ "move": "flag", "at": 999 }));
    assert_eq!(names(&got).last().copied(), Some("flag"));

    // Table Style prints the columns as the list shows them.
    let setup = run(&mut s, "file.pageSetup", Value::Null);
    assert_eq!(names(&setup), names(&got));

    let got = run(&mut s, "view.columns", json!({ "columns": ["received", "subject", "subject"] }));
    assert_eq!(names(&got), ["date", "subject"]);
    run(&mut s, "view.columns", json!({ "hide": "date" }));
    assert!(s.run("view.columns", &json!({ "hide": "subject" })).is_err(), "one column stays");
    assert!(s.run("view.columns", &json!({ "move": "size", "at": 0 })).is_err(), "not a column of the list");
    for junk in [json!({ "show": "nonsense" }), json!({ "show": 3 }), json!({ "columns": [] }), json!({ "show": "size", "at": -1 })] {
        assert!(s.run("view.columns", &junk).is_err(), "{junk}");
    }
    assert_eq!(names(&run(&mut s, "view.columns", Value::Null)), ["subject"], "nothing changed by what was refused");
    let got = run(&mut s, "view.columns", json!({ "columns": "default" }));
    assert_eq!(names(&got), names(&first));
}

#[test]
fn the_columns_are_kept_between_runs() {
    let mut s = session();
    run(&mut s, "view.columns", json!({ "columns": ["who", "subject", "date", "preview"] }));
    let kept = crate::cmd::settings::current(&s, None);
    assert_eq!(kept["listColumns"], json!(["who", "subject", "date", "preview"]));
    let mut next = session();
    let path = std::env::temp_dir().join(format!("omc-columns-{}.json", std::process::id()));
    std::fs::write(&path, serde_json::to_string(&kept).unwrap()).unwrap();
    run(&mut next, "settings.load", json!({ "path": path.display().to_string() }));
    assert_eq!(names(&run(&mut next, "view.columns", Value::Null)), ["who", "subject", "date", "preview"]);

    // A list of columns that makes no sense leaves the list as it was.
    let mut bad = kept.clone();
    bad["listColumns"] = json!(["nonsense"]);
    std::fs::write(&path, serde_json::to_string(&bad).unwrap()).unwrap();
    let mut third = session();
    run(&mut third, "settings.load", json!({ "path": path.display().to_string() }));
    assert_eq!(third.list_columns, crate::print::Field::LIST.to_vec());
    let _ = std::fs::remove_file(&path);
}
