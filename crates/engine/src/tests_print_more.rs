//! The Calendar printed a day, a week or a month to a page and in Calendar Details Style, and
//! People as cards and as a telephone directory; each module keeping the style it prints in.

use crate::print::Style;
use crate::{Module, Session};
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

fn calendar() -> Session {
    let mut s = session();
    run(&mut s, "nav.module", json!({"module": "calendar"}));
    run(&mut s, "calendar.goTo", json!({"date": "2026-10-07"}));
    s
}

fn pages(s: &mut Session, p: Value) -> usize {
    let out = run(s, "file.printPreview", p);
    out["pages"].as_u64().unwrap() as usize
}

#[test]
fn the_calendar_prints_as_its_view_until_a_style_is_chosen() {
    let mut s = calendar();
    run(&mut s, "calendar.view", json!({"view": "week"}));
    assert_eq!(crate::print::style_now(&s), Style::WeeklyAgenda);
    let out = run(&mut s, "file.printPreview", Value::Null);
    assert_eq!((out["style"].as_str(), out["pages"].as_u64()), (Some("weeklyAgenda"), Some(1)));
    run(&mut s, "calendar.view", json!({"view": "day"}));
    assert_eq!(crate::print::style_now(&s), Style::Daily);
    run(&mut s, "calendar.view", json!({"view": "month"}));
    assert_eq!(crate::print::style_now(&s), Style::Monthly);

    // A style chosen is kept for the Calendar alone, whatever its view.
    run(&mut s, "file.pageSetup", json!({"style": "Calendar Details Style"}));
    run(&mut s, "calendar.view", json!({"view": "day"}));
    assert_eq!(crate::print::style_now(&s), Style::CalendarDetails);
    assert_eq!(s.print_style, Style::Memo);
    run(&mut s, "nav.module", json!({"module": "mail"}));
    assert_eq!(crate::print::style_now(&s), Style::Memo);
    // A reset lets the view choose again.
    run(&mut s, "nav.module", json!({"module": "calendar"}));
    run(&mut s, "file.pageSetup", json!({"reset": true}));
    assert_eq!(crate::print::style_now(&s), Style::Daily);
}

#[test]
fn a_day_a_week_and_a_month_to_a_page() {
    let mut s = calendar();
    run(&mut s, "calendar.view", json!({"view": "week"}));
    // The week shown, a day to a page.
    assert_eq!(pages(&mut s, json!({"style": "daily"})), 7);
    // Two days asked for.
    assert_eq!(pages(&mut s, json!({"style": "daily", "start": "2026-10-07", "end": "2026-10-08"})), 2);
    // Weeks are whole: three days across a week's end are two weeks.
    assert_eq!(pages(&mut s, json!({"style": "weeklyAgenda", "start": "2026-10-09", "end": "2026-10-12"})), 2);
    // Months are whole: two months from the middle of one.
    assert_eq!(pages(&mut s, json!({"style": "monthly", "start": "2026-10-20", "end": "2026-11-03"})), 2);
    run(&mut s, "calendar.view", json!({"view": "month"}));
    assert_eq!(pages(&mut s, json!({"style": "monthly"})), 1);
    // The details of the week, on as many pages as they take.
    run(&mut s, "calendar.view", json!({"view": "week"}));
    let out = run(&mut s, "file.printPreview", json!({"style": "calendarDetails"}));
    assert!(out["pages"].as_u64().unwrap() >= 1);
    // The gathered days are those printed.
    let g = s.print_calendar.as_ref().unwrap();
    assert_eq!((g.style, g.days.len()), (Style::CalendarDetails, 7));
    assert!(g.items.iter().any(|(_, a)| a.subject == "Team stand-up"));
    // And a PDF of a month.
    let pdf = std::env::temp_dir().join(format!("omc-print-more-{}.pdf", std::process::id()));
    run(&mut s, "file.print", json!({"style": "monthly", "path": pdf}));
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
    std::fs::remove_file(&pdf).unwrap();
}

#[test]
fn a_print_range_is_checked() {
    let mut s = calendar();
    for bad in [
        json!({"style": "daily", "start": "2026-10-09", "end": "2026-10-08"}),
        json!({"style": "daily", "start": "2026-01-01", "end": "2027-12-31"}),
        json!({"style": "daily", "start": "not a day"}),
        json!({"style": "monthly", "end": 7}),
        json!({"style": "weekly agendas"}),
    ] {
        assert!(s.run("file.printPreview", &bad).is_err(), "{bad}");
    }
    // An empty range of days is a page that says so.
    let out = run(&mut s, "file.printPreview", json!({"style": "calendarDetails", "start": "2030-01-01", "end": "2030-01-02"}));
    assert_eq!(out["pages"], 1);
}

#[test]
fn people_print_as_cards_and_as_a_directory() {
    let mut s = session();
    run(&mut s, "nav.module", json!({"module": "people"}));
    assert_eq!(s.module, Module::People);
    assert_eq!(crate::print::style_now(&s), Style::Card);
    let cards = run(&mut s, "file.printPreview", Value::Null);
    assert_eq!(cards["style"], "card");
    assert!(cards["pages"].as_u64().unwrap() >= 1);
    run(&mut s, "people.new", json!({"given": "Zed", "surname": "Nophone", "emails": ["zed@example.org"]}));
    run(&mut s, "people.new", json!({"given": "Ola", "surname": "Ring", "phones": {"mobile": "+1 555 0100 222"}}));
    let directory = run(&mut s, "file.printPreview", json!({"style": "phone directory"}));
    assert_eq!(directory["style"], "phoneDirectory");
    let laid = crate::print::laid_as(&s, Style::PhoneDirectory, &s.page_setup, &s.print_list, Default::default()).unwrap();
    // What is printed, with no spaces: words may be set apart.
    let text: String =
        laid.printed.pages.iter().flat_map(|p| &p.texts).flat_map(|t| t.glyphs.iter().map(|g| g.1)).filter(|c| !c.is_whitespace()).collect();
    let has = |words: &str| text.contains(&words.replace(' ', ""));
    assert!(has("Ring, Ola"), "{text}");
    assert!(has("+1 555 0100 222"), "{text}");
    assert!(!has("Nophone"), "{text}");
    assert!(has("without a telephone number left out"), "{text}");
    // A Calendar style is not one People prints in.
    run(&mut s, "file.pageSetup", json!({"style": "phoneDirectory"}));
    assert_eq!(s.print_people_style, Style::PhoneDirectory);
    assert_eq!(s.print_style, Style::Memo);
}

#[test]
fn each_module_s_style_is_kept_between_runs() {
    let mut s = calendar();
    run(&mut s, "file.pageSetup", json!({"style": "monthly"}));
    run(&mut s, "nav.module", json!({"module": "people"}));
    run(&mut s, "file.pageSetup", json!({"style": "phoneDirectory"}));
    run(&mut s, "nav.module", json!({"module": "mail"}));
    run(&mut s, "file.pageSetup", json!({"style": "table"}));
    let kept = s.settings(None);
    let setup = &kept["pageSetup"];
    assert_eq!(
        (setup["style"].as_str(), setup["calendarStyle"].as_str(), setup["peopleStyle"].as_str()),
        (Some("table"), Some("monthly"), Some("phoneDirectory"))
    );

    let dir = std::env::temp_dir().join(format!("omc-print-more-settings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("settings.json");
    std::fs::write(&file, serde_json::to_vec(&kept).unwrap()).unwrap();
    let mut next = Session::new();
    run(&mut next, "settings.load", json!({"path": file}));
    assert_eq!((next.print_style, next.print_calendar_style, next.print_people_style), (Style::Table, Some(Style::Monthly), Style::PhoneDirectory));

    // A style of another module's is not taken for one's own.
    let mut wrong = kept.clone();
    wrong["pageSetup"]["style"] = json!("monthly");
    wrong["pageSetup"]["calendarStyle"] = json!("card");
    wrong["pageSetup"]["peopleStyle"] = json!(null);
    std::fs::write(&file, serde_json::to_vec(&wrong).unwrap()).unwrap();
    let mut other = Session::new();
    let took = run(&mut other, "settings.load", json!({"path": file}));
    assert!(took["ignored"].as_u64().unwrap() >= 3, "{took}");
    assert_eq!((other.print_style, other.print_calendar_style, other.print_people_style), (Style::Memo, None, Style::Card));
    std::fs::remove_dir_all(&dir).unwrap();
}
