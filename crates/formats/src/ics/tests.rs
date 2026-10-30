use super::*;

fn at(text: &str, off: i32) -> i64 {
    calendar::parse_local(text, off).unwrap()
}

fn day(text: &str) -> i64 {
    calendar::day_of(at(text, 0), 0)
}

/// A file as a calendar service publishes one: a zone with daylight saving, a weekly series
/// with a day taken out and one occurrence moved, an all-day event, and one in UTC with an
/// alarm. Synthetic, written for this test.
const PUBLISHED: &[u8] = b"BEGIN:VCALENDAR\r
PRODID:-//Example//Test Calendar//EN\r
VERSION:2.0\r
METHOD:PUBLISH\r
X-WR-CALNAME:Team\r
BEGIN:VTIMEZONE\r
TZID:Europe/Berlin\r
BEGIN:DAYLIGHT\r
TZOFFSETFROM:+0100\r
TZOFFSETTO:+0200\r
DTSTART:19700329T020000\r
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r
END:DAYLIGHT\r
BEGIN:STANDARD\r
TZOFFSETFROM:+0200\r
TZOFFSETTO:+0100\r
DTSTART:19701025T030000\r
RRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r
END:STANDARD\r
END:VTIMEZONE\r
BEGIN:VEVENT\r
UID:standup-1@example.test\r
DTSTAMP:20261101T120000Z\r
DTSTART;TZID=Europe/Berlin:20261102T091500\r
DTEND;TZID=Europe/Berlin:20261102T093000\r
RRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=8\r
EXDATE;TZID=Europe/Berlin:20261104T091500\r
SUMMARY:Stand-up\r
LOCATION:Room 4\\, second floor\r
DESCRIPTION:Bring what you did yesterday\\nand what is next. Caf\xC3\r
 \xA9 is upstairs.\r
CATEGORIES:Team,Daily\\, short\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:standup-1@example.test\r
RECURRENCE-ID;TZID=Europe/Berlin:20261109T091500\r
DTSTART;TZID=Europe/Berlin:20261109T110000\r
DTEND;TZID=Europe/Berlin:20261109T111500\r
SUMMARY:Stand-up (late)\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:holiday@example.test\r
DTSTART;VALUE=DATE:20261224\r
DTEND;VALUE=DATE:20261227\r
SUMMARY:Holidays\r
TRANSP:TRANSPARENT\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:call@example.test\r
DTSTART:20260710T150000Z\r
DURATION:PT45M\r
SUMMARY:Call\r
X-MICROSOFT-CDO-BUSYSTATUS:OOF\r
BEGIN:VALARM\r
ACTION:DISPLAY\r
TRIGGER:-PT30M\r
END:VALARM\r
END:VEVENT\r
END:VCALENDAR\r
";

#[test]
fn a_published_calendar_is_read_with_its_zone_its_series_and_its_changed_occurrences() {
    let cal = read(PUBLISHED, &Zone::fixed(0)).unwrap();
    assert_eq!(cal.name.as_deref(), Some("Team"));
    assert_eq!(cal.method.as_deref(), Some("PUBLISH"));
    assert!(cal.notes.is_empty(), "{:?}", cal.notes);
    assert_eq!(cal.events.len(), 3);
    let standup = &cal.events[0];
    assert!(standup.notes.is_empty(), "{:?}", standup.notes);
    assert_eq!(standup.uid, "standup-1@example.test");
    let a = &standup.appointment;
    assert_eq!(a.subject, "Stand-up");
    assert_eq!(a.location, "Room 4, second floor");
    // November in Berlin is an hour ahead of UTC.
    assert_eq!(a.start, at("2026-11-02 09:15", 60));
    assert_eq!(a.minutes(), 15);
    assert_eq!(standup.description, "Bring what you did yesterday\nand what is next. Café is upstairs.");
    assert_eq!(standup.categories, vec!["Team".to_string(), "Daily, short".to_string()]);
    let r = a.recurrence.as_ref().unwrap();
    assert_eq!((r.freq, &r.rule, r.period, r.end), (Freq::Weekly, &Rule::Week { days: 0b0000_1010 }, 1, End::After(8)));
    assert_eq!(a.series_offset(), 60);
    let all = a.occurrences(at("2026-11-01 00:00", 0), at("2027-01-01 00:00", 0));
    // Eight planned, one taken out.
    assert_eq!(all.len(), 7);
    assert!(!all.iter().any(|o| o.occurrence == Some(day("2026-11-04 00:00"))));
    let late = all.iter().find(|o| o.occurrence == Some(day("2026-11-09 00:00"))).unwrap();
    assert_eq!((late.subject.as_str(), late.start, late.location.as_str()), ("Stand-up (late)", at("2026-11-09 11:00", 60), "Room 4, second floor"));
    let holidays = &cal.events[1].appointment;
    assert!(holidays.all_day);
    assert_eq!((holidays.start, holidays.end, holidays.busy), (at("2026-12-24 00:00", 0), at("2026-12-27 00:00", 0), Busy::Free));
    let call = &cal.events[2].appointment;
    assert_eq!((call.start, call.minutes(), call.busy, call.reminder), (at("2026-07-10 15:00", 0), 45, Busy::OutOfOffice, Some(30)));
    // Read where we are two hours ahead, an all-day event keeps its days here.
    let east = read(PUBLISHED, &Zone::fixed(120)).unwrap();
    assert_eq!(east.events[1].appointment.start, at("2026-12-24 00:00", 120));
    assert_eq!(east.events[0].appointment.start, a.start);
}

#[test]
fn the_zone_in_effect_follows_daylight_saving_through_the_year() {
    let cal = read(PUBLISHED, &Zone::fixed(0)).unwrap();
    let (roots, _) = components(PUBLISHED);
    let zone = roots[0].children.iter().find(|c| c.name == "VTIMEZONE").and_then(FileZone::of).unwrap();
    let local = |t: &str| time::to_unix(at(t, 0));
    assert_eq!(zone.offset_at(local("2026-01-15 12:00")), 60);
    assert_eq!(zone.offset_at(local("2026-07-15 12:00")), 120);
    // 29 March 2026 is the last Sunday of March, 25 October the last of October.
    assert_eq!(zone.offset_at(local("2026-03-29 01:59")), 60);
    assert_eq!(zone.offset_at(local("2026-03-29 03:00")), 120);
    assert_eq!(zone.offset_at(local("2026-10-25 04:00")), 60);
    assert_eq!(zone.offset_at(local("1960-01-01 00:00")), 60);
    assert_eq!(cal.events.len(), 3);
}

fn appt(subject: &str, start: i64, minutes: i64) -> Appointment {
    Appointment {
        subject: subject.into(),
        location: String::new(),
        start,
        end: start + minutes * MINUTE,
        all_day: false,
        busy: Busy::Busy,
        reminder: Some(15),
        recurring: false,
        meeting: false,
        recurrence: None,
        occurrence: None,
        zone: None,
    }
}

fn series(mut a: Appointment, freq: Freq, rule: Rule, period: u32, end: End, off: i64) -> Appointment {
    let local = a.start.div_euclid(MINUTE) + off;
    let d = recur::day_of_minutes(local);
    let so = u32::try_from(local - recur::minutes_of_day(d)).unwrap();
    let r = Recurrence::new(freq, rule, period, end, d, so, so + u32::try_from(a.minutes()).unwrap(), 0);
    let first = recur::minutes_of_day(r.start_day) + i64::from(r.start_offset) - off;
    let length = a.end - a.start;
    a.start = first * MINUTE;
    a.end = a.start + length;
    a.recurrence = Some(r);
    a.recurring = true;
    a
}

#[test]
fn appointments_written_as_a_calendar_file_are_read_back_as_they_were() {
    let off = 120;
    let mut weekly =
        series(appt("Planning", at("2026-10-19 10:00", off), 60), Freq::Weekly, Rule::Week { days: 0b0010_0010 }, 2, End::After(6), i64::from(off));
    weekly.location = "Room; 7, east\\wing".into();
    let moved = weekly.occurrence_of(day("2026-11-02 00:00")).unwrap();
    let mut to = moved.clone();
    to.start += 90 * MINUTE;
    to.end += 90 * MINUTE;
    to.subject = "Planning, moved".into();
    to.reminder = None;
    assert!(weekly.change_occurrence(day("2026-11-02 00:00"), &to));
    assert!(weekly.delete_occurrence(day("2026-11-06 00:00")));
    let last_thursday = series(
        appt("Review", at("2026-10-29 16:00", off), 30),
        Freq::Monthly,
        Rule::MonthNth { days: 1 << 4, nth: 5 },
        1,
        End::On(day("2027-06-30 00:00")),
        i64::from(off),
    );
    let day_31 = series(appt("Rent", at("2026-10-31 09:00", off), 15), Freq::Monthly, Rule::Month { day: 31 }, 1, End::Never, i64::from(off));
    let yearly =
        series(appt("Anniversary", at("2026-11-07 00:00", off), 24 * 60), Freq::Yearly, Rule::Month { day: 7 }, 12, End::Never, i64::from(off));
    let mut yearly = yearly;
    yearly.all_day = true;
    yearly.reminder = None;
    let daily = series(appt("Walk", at("2026-10-20 07:00", 0), 20), Freq::Daily, Rule::Day, 3, End::After(10), 0);
    let mut single = appt("Dentist", at("2026-10-21 08:30", off), 45);
    single.busy = Busy::Tentative;
    single.location = "12 High Street".into();
    let mut away = appt("Away", at("2026-12-28 00:00", off), 3 * 24 * 60);
    away.all_day = true;
    away.busy = Busy::OutOfOffice;
    away.reminder = None;
    let all = [weekly, last_thursday, day_31, yearly, daily, single, away];
    let items: Vec<Item> = all
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let mut it = a.item();
            it.set_body_text(&format!("Line one of {i}\nline two, with a comma; and a semicolon"));
            it.categories = vec!["Work".into(), format!("Item {i}")];
            calendar::set_global_id(&mut it, &calendar::new_global_id(at("2026-10-01 00:00", 0), i as u64));
            it
        })
        .collect();
    let text = write(&items, &Zone::fixed(off), at("2026-10-18 12:00", 0), Some("Calendar"));
    for l in text.split("\r\n") {
        assert!(l.len() <= 75, "{l}");
    }
    let cal = read(text.as_bytes(), &Zone::fixed(off)).unwrap();
    assert_eq!(cal.name.as_deref(), Some("Calendar"));
    assert!(cal.notes.is_empty(), "{:?}", cal.notes);
    assert_eq!(cal.events.len(), all.len());
    for ((e, a), it) in cal.events.iter().zip(&all).zip(&items) {
        assert!(e.notes.is_empty(), "{}: {:?}", a.subject, e.notes);
        assert_eq!(&e.appointment, a, "{}", a.subject);
        assert_eq!(e.uid, uid(it));
        assert_eq!(e.description, it.body_text());
        assert_eq!(e.categories, it.categories);
        // The item made of it is known by the same id.
        assert_eq!(calendar::global_id(&e.item()), calendar::global_id(it));
    }
    // Read where the clock is elsewhere, the series keep their own time.
    let west = read(text.as_bytes(), &Zone::fixed(-300)).unwrap();
    assert_eq!(west.events[0].appointment.start, all[0].start);
    assert_eq!(west.events[0].appointment.recurrence, all[0].recurrence);
}

#[test]
fn recurrence_rules_are_kept_as_outlook_keeps_them_or_said_to_be_not_kept() {
    let file = |rule: &str, start: &str| {
        format!(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x\r\nDTSTART:{start}\r\nDTEND:{start}\r\nRRULE:{rule}\r\nSUMMARY:S\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        )
    };
    let rule_of = |rule: &str, start: &str| {
        let cal = read(file(rule, start).as_bytes(), &Zone::fixed(0)).unwrap();
        let e = cal.events.into_iter().next().unwrap();
        (e.appointment.recurrence.map(|r| (r.freq, r.rule, r.period, r.end, r.start_day)), e.notes)
    };
    let mon = "20261019T090000Z";
    let d = |t: &str| day(t);
    let cases: Vec<(&str, (Freq, Rule, u32, End))> = vec![
        ("FREQ=DAILY;INTERVAL=3", (Freq::Daily, Rule::Day, 3, End::Never)),
        ("FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR", (Freq::Weekly, Rule::Week { days: 0x3E }, 1, End::Never)),
        ("FREQ=WEEKLY", (Freq::Weekly, Rule::Week { days: 0b10 }, 1, End::Never)),
        ("FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,TH;COUNT=4", (Freq::Weekly, Rule::Week { days: 0b1_0100 }, 2, End::After(4))),
        ("FREQ=MONTHLY", (Freq::Monthly, Rule::Month { day: 19 }, 1, End::Never)),
        ("FREQ=MONTHLY;BYDAY=-1FR", (Freq::Monthly, Rule::MonthNth { days: 1 << 5, nth: 5 }, 1, End::Never)),
        ("FREQ=MONTHLY;BYDAY=+2TU", (Freq::Monthly, Rule::MonthNth { days: 1 << 2, nth: 2 }, 1, End::Never)),
        ("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1", (Freq::Monthly, Rule::MonthNth { days: 0x3E, nth: 5 }, 1, End::Never)),
        ("FREQ=MONTHLY;INTERVAL=3;BYMONTHDAY=28,29,30;BYSETPOS=-1", (Freq::Monthly, Rule::Month { day: 30 }, 3, End::Never)),
        ("FREQ=MONTHLY;BYMONTHDAY=-1;UNTIL=20270101T000000Z", (Freq::Monthly, Rule::MonthEnd, 1, End::On(d("2026-12-31 00:00")))),
        // Until a time before the day's occurrence: the day before is the last.
        ("FREQ=DAILY;UNTIL=20261025T080000Z", (Freq::Daily, Rule::Day, 1, End::On(d("2026-10-24 00:00")))),
        ("FREQ=DAILY;UNTIL=20261025T090000Z", (Freq::Daily, Rule::Day, 1, End::On(d("2026-10-25 00:00")))),
        ("FREQ=DAILY;UNTIL=20261025", (Freq::Daily, Rule::Day, 1, End::On(d("2026-10-25 00:00")))),
        ("FREQ=YEARLY", (Freq::Yearly, Rule::Month { day: 19 }, 12, End::Never)),
        ("FREQ=YEARLY;INTERVAL=2", (Freq::Monthly, Rule::Month { day: 19 }, 24, End::Never)),
        ("FREQ=YEARLY;BYMONTH=10;BYDAY=3MO", (Freq::Yearly, Rule::MonthNth { days: 0b10, nth: 3 }, 12, End::Never)),
        ("FREQ=WEEKLY;COUNT=5000", (Freq::Weekly, Rule::Week { days: 0b10 }, 1, End::After(999))),
    ];
    for (rule, want) in cases {
        let (got, _) = rule_of(rule, mon);
        let (freq, r, period, end, _) = got.unwrap_or_else(|| panic!("{rule}"));
        assert_eq!((freq, r, period, end), want, "{rule}");
    }
    // A yearly series in another month than it is written to start in begins in that month.
    let (got, _) = rule_of("FREQ=YEARLY;BYMONTH=3;BYMONTHDAY=5", mon);
    assert_eq!(got.map(|g| g.4), Some(d("2027-03-05 00:00")));
    // A day past 28 alone falls on the last day of shorter months here, and says so.
    let (_, notes) = rule_of("FREQ=MONTHLY;BYMONTHDAY=31", "20261031T090000Z");
    assert!(notes.iter().any(|n| n.contains("last day")), "{notes:?}");
    for rule in [
        "FREQ=HOURLY",
        "FREQ=WEEKLY;BYWEEKNO=20",
        "FREQ=MONTHLY;BYDAY=MO",
        "FREQ=YEARLY;BYMONTH=1,7",
        "FREQ=DAILY;INTERVAL=0",
        "FREQ=WEEKLY;BYDAY=XX",
        "FREQ=WEEKLY;COUNT=2;UNTIL=20270101",
        "INTERVAL=2",
        "FREQ=DAILY;INTERVAL=5000",
    ] {
        let (got, notes) = rule_of(rule, mon);
        assert!(got.is_none(), "{rule}");
        assert!(notes.iter().any(|n| n.contains("not kept")), "{rule}: {notes:?}");
    }
}

#[test]
fn times_in_a_zone_the_file_does_not_define_are_taken_as_here_and_said_to_be() {
    let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:a\nDTSTART;TZID=Mars/Olympus:20261020T100000\nSUMMARY:A\nEND:VEVENT\nBEGIN:VEVENT\nUID:b\nDTSTART:20261020T100000\nSUMMARY:Floating\nEND:VEVENT\nEND:VCALENDAR\n";
    let cal = read(text.as_bytes(), &Zone::fixed(120)).unwrap();
    assert_eq!(cal.events[0].appointment.start, at("2026-10-20 10:00", 120));
    assert!(cal.events[0].notes.iter().any(|n| n.contains("Mars/Olympus")));
    assert_eq!(cal.events[1].appointment.start, at("2026-10-20 10:00", 120));
    assert!(cal.events[1].notes.is_empty());
}

#[test]
fn cancelled_events_and_orphaned_occurrences_are_handled_and_said() {
    let text = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:gone\r\nDTSTART:20261020T100000Z\r\nSTATUS:CANCELLED\r\nSUMMARY:Off\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:lonely\r\nRECURRENCE-ID:20261021T100000Z\r\nDTSTART:20261021T120000Z\r\nSUMMARY:Moved one\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:No start\r\nEND:VEVENT\r\nBEGIN:VTODO\r\nSUMMARY:A task\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
    let cal = read(text.as_bytes(), &Zone::fixed(0)).unwrap();
    assert_eq!(cal.events.len(), 1);
    assert_eq!(cal.events[0].appointment.subject, "Moved one");
    assert_ne!(cal.events[0].uid, "lonely");
    assert!(cal.notes.iter().any(|n| n.contains("cancelled")));
    assert!(cal.notes.iter().any(|n| n.contains("no start")));
    // A to-do is no longer left out with a note: it is read as a task.
    assert!(!cal.notes.iter().any(|n| n.contains("tasks")));
    assert_eq!(cal.todos.iter().map(|t| t.task.subject.as_str()).collect::<Vec<_>>(), ["A task"]);
}

#[test]
fn long_lines_are_folded_between_characters_and_unfolded_again() {
    let mut a = appt("Ünïcödé ", at("2026-10-20 10:00", 0), 30);
    a.subject = "Ünïcödé — ".repeat(40);
    let text = write(&[a.item()], &Zone::fixed(0), at("2026-10-18 12:00", 0), None);
    assert!(text.split("\r\n").all(|l| l.len() <= 75));
    assert!(text.ends_with("END:VCALENDAR\r\n"));
    let back = read(text.as_bytes(), &Zone::fixed(0)).unwrap();
    assert_eq!(back.events[0].appointment.subject, a.subject);
}

#[test]
fn hostile_calendar_files_never_panic_and_are_read_in_bounded_time() {
    let started = std::time::Instant::now();
    let mut deep = String::from("BEGIN:VCALENDAR\n");
    deep.push_str(&"BEGIN:VEVENT\n".repeat(100_000));
    let mut many = String::from("BEGIN:VCALENDAR\n");
    for i in 0..30_000 {
        many.push_str(&format!(
            "BEGIN:VEVENT\nUID:{}\nDTSTART:20261020T100000Z\nRRULE:FREQ=DAILY\nEXDATE:20261021T100000Z,20261022T100000Z\nEND:VEVENT\n",
            i % 3
        ));
    }
    let overrides = {
        let mut s = String::from("BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:x\nDTSTART:20261020T100000Z\nRRULE:FREQ=DAILY\nEND:VEVENT\n");
        for i in 0..3_000 {
            s.push_str(&format!(
                "BEGIN:VEVENT\nUID:x\nRECURRENCE-ID:{}\nDTSTART:20261020T110000Z\nEND:VEVENT\n",
                date_text(day("2026-10-20 00:00") + i) + "T100000Z"
            ));
        }
        s
    };
    let cases: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"garbage".to_vec(),
        b"BEGIN:VCALENDAR".to_vec(),
        b"BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:2026\nEND:VEVENT".to_vec(),
        b"BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:a\nDTSTART:20261340T250000Z\nDTEND:99999999T999999\nEND:VEVENT\nEND:VCALENDAR".to_vec(),
        b"BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:a\nDTSTART;TZID=\"unterminated:20261020T100000\nEND:VEVENT\nEND:VCALENDAR".to_vec(),
        b"BEGIN:VCALENDAR\nBEGIN:VTIMEZONE\nTZID:Z1\nBEGIN:STANDARD\nDTSTART:16010101T000000\nTZOFFSETTO:+9999\nTZOFFSETFROM:-99\nRRULE:FREQ=YEARLY;BYMONTH=13;BYDAY=-99SU\nRDATE:1,2,3\nEND:STANDARD\nEND:VTIMEZONE\nBEGIN:VEVENT\nUID:a\nDTSTART;TZID=Z1:20261020T100000\nDURATION:P99999999999W\nRRULE:FREQ=MONTHLY;BYMONTHDAY=0;BYSETPOS=9\nEND:VEVENT\nEND:VCALENDAR".to_vec(),
        b"BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:a\nDTSTART:20261020T100000Z\nRRULE:FREQ=DAILY;UNTIL=16010101\nBEGIN:VALARM\nTRIGGER;VALUE=DATE-TIME:00000000T000000Z\nEND:VALARM\nBEGIN:VALARM\nTRIGGER:PX\nEND:VALARM\nEND:VEVENT\nEND:VCALENDAR".to_vec(),
        b"BEGIN:VCALENDAR\n continuation first\n\0\xFF\xFE\nEND:VEVENT\nEND:VCALENDAR\nEND:VCALENDAR".to_vec(),
        format!("BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:a\nDTSTART:20261020T100000Z\nSUMMARY:{}\n{}\nEND:VEVENT\nEND:VCALENDAR", "x".repeat(MAX_LINE * 2), " y".repeat(10_000)).into_bytes(),
        format!("BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:a\nDTSTART:20261020T100000Z\nRRULE:FREQ=WEEKLY;BYDAY={}\nCATEGORIES:{}\nEND:VEVENT\nEND:VCALENDAR", "MO,".repeat(5_000), "a,".repeat(5_000)).into_bytes(),
        format!("BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:a\nDTSTART:20261020T100000Z{}\nEND:VEVENT\nEND:VCALENDAR", ";X=\"a\",b".repeat(5_000)).into_bytes(),
        deep.into_bytes(),
        many.into_bytes(),
        overrides.into_bytes(),
    ];
    for (i, case) in cases.iter().enumerate() {
        if let Ok(cal) = read(case, &Zone::fixed(0)) {
            assert!(cal.events.len() <= MAX_EVENTS, "{i}");
            assert!(cal.notes.len() <= MAX_NOTES, "{i}");
            for e in &cal.events {
                assert!(e.appointment.end >= e.appointment.start, "{i}");
                assert!(e.appointment.subject.chars().count() <= MAX_TEXT, "{i}");
                // What is read is written again without a panic.
                let _ = write(&[e.item()], &Zone::fixed(0), 0, None);
            }
        }
    }
    let many_events = read(cases[13].as_slice(), &Zone::fixed(0)).unwrap();
    assert_eq!(many_events.events.len(), 3, "one series for each UID");
    let changed = read(cases[14].as_slice(), &Zone::fixed(0)).unwrap();
    assert!(changed.events[0].appointment.recurrence.as_ref().unwrap().exceptions.len() <= recur::MAX_COUNT as usize);
    assert!(read(&vec![b'a'; MAX_BYTES + 1], &Zone::fixed(0)).is_err());
    assert!(started.elapsed().as_secs() < 60, "{:?}", started.elapsed());
}

/// An invitation as another calendar program sends one: an organizer, three attendees (one
/// who must come, one who may and has said yes, and a room) and a SEQUENCE. Synthetic.
const INVITATION: &str = "BEGIN:VCALENDAR\r
METHOD:REQUEST\r
BEGIN:VEVENT\r
UID:plan-7@example.com\r
DTSTAMP:20261101T120000Z\r
DTSTART:20261103T140000Z\r
DTEND:20261103T150000Z\r
SUMMARY:Planning\r
ORGANIZER;CN=\"Lee, Sam\":mailto:sam@example.com\r
ATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;CN=Alex Morgan:MAILTO:alex.morgan@example.org\r
ATTENDEE;ROLE=OPT-PARTICIPANT;PARTSTAT=ACCEPTED:mailto:priya@example.net\r
ATTENDEE;CUTYPE=ROOM;CN=Room 4:mailto:room4@example.org\r
ATTENDEE;CN=Twice:mailto:PRIYA@example.net\r
ATTENDEE;CN=No address:urn:uuid:1234\r
SEQUENCE:3\r
END:VEVENT\r
END:VCALENDAR\r
";

#[test]
fn an_invitation_is_read_with_its_organizer_and_attendees_and_written_again() {
    let cal = read(INVITATION.as_bytes(), &Zone::utc()).unwrap();
    assert_eq!(cal.method.as_deref(), Some("REQUEST"));
    let e = &cal.events[0];
    assert!(e.notes.is_empty(), "{:?}", e.notes);
    assert!(e.appointment.meeting);
    assert_eq!(e.sequence, 3);
    let organizer = e.organizer.as_ref().unwrap();
    assert_eq!((organizer.name.as_str(), organizer.address.as_str()), ("Lee, Sam", "sam@example.com"));
    let people: Vec<(&str, Role, Response)> = e.attendees.iter().map(|p| (p.address.as_str(), p.role, p.response)).collect();
    assert_eq!(
        people,
        [
            ("alex.morgan@example.org", Role::Required, Response::NotResponded),
            ("priya@example.net", Role::Optional, Response::Accepted),
            ("room4@example.org", Role::Resource, Response::NotResponded),
        ],
        "an address given twice is one attendee; a link that is no address is none"
    );
    // Seen by one who is asked: received, not yet answered.
    let mine = e.meeting(&["Alex.Morgan@example.org"]).unwrap();
    assert!(mine.received);
    assert_eq!(mine.response, Response::NotResponded);
    // Seen by its organizer: theirs.
    let theirs = e.meeting(&["sam@example.com"]).unwrap();
    assert!(!theirs.received);
    assert_eq!(theirs.response, Response::Organized);
    let mut item = e.appointment.item();
    e.write_as(&mut item, &["sam@example.com"]);
    let folded = write_as(std::slice::from_ref(&item), &Zone::utc(), at("2026-11-01 12:00", 0), None, &Method::Request);
    assert!(folded.split("\r\n").all(|l| l.len() <= 75), "lines are folded");
    let text = folded.replace("\r\n ", "");
    assert!(text.contains("METHOD:REQUEST\r\n"));
    assert!(text.contains("ORGANIZER;CN=\"Lee, Sam\":mailto:sam@example.com\r\n"));
    assert!(text.contains("PARTSTAT=NEEDS-ACTION;RSVP=TRUE;CN=\"Alex Morgan\":mailto:alex.morgan@example.org\r\n"), "{text}");
    assert!(text.contains("ROLE=NON-PARTICIPANT;CUTYPE=RESOURCE"));
    assert!(text.contains("SEQUENCE:3\r\n") && text.contains("STATUS:CONFIRMED\r\n"));
    let back = read(text.as_bytes(), &Zone::utc()).unwrap();
    assert_eq!(back.events[0].attendees, e.attendees);
    assert_eq!(back.events[0].organizer, e.organizer);
}

#[test]
fn a_reply_gives_its_answer_and_a_cancellation_its_meeting() {
    let cal = read(INVITATION.as_bytes(), &Zone::utc()).unwrap();
    let mut item = cal.events[0].appointment.item();
    cal.events[0].write_as(&mut item, &["alex.morgan@example.org"]);
    item.set_body_text("The agenda");
    let mut me = Attendee::new("Alex Morgan", "alex.morgan@example.org", Role::Required);
    me.response = Response::Tentative;
    let now = at("2026-11-01 13:00", 0);
    let reply = write_as(std::slice::from_ref(&item), &Zone::utc(), now, None, &Method::Reply { from: &me, comment: "Late by ten minutes" });
    assert!(reply.contains("METHOD:REPLY\r\n"));
    assert_eq!(reply.matches("ATTENDEE").count(), 1, "a reply names the one who answers alone");
    assert!(!reply.contains("DESCRIPTION") && !reply.contains("VALARM"));
    let back = read(reply.as_bytes(), &Zone::utc()).unwrap();
    assert_eq!(back.answers.len(), 1);
    let answer = &back.answers[0];
    assert_eq!(answer.uid, "plan-7@example.com");
    assert_eq!((answer.attendee.address.as_str(), answer.attendee.response), ("alex.morgan@example.org", Response::Tentative));
    assert_eq!(answer.comment, "Late by ten minutes");
    assert_eq!(answer.sequence, 3);

    let cancel = write_as(std::slice::from_ref(&item), &Zone::utc(), now, None, &Method::Cancel);
    assert!(cancel.contains("STATUS:CANCELLED\r\n"));
    let back = read(cancel.as_bytes(), &Zone::utc()).unwrap();
    assert!(back.events.is_empty(), "a meeting called off is not one to add");
    assert_eq!(back.cancelled.len(), 1);
    assert_eq!(back.cancelled[0].uid, "plan-7@example.com");
    assert_eq!(back.cancelled[0].organizer.as_ref().map(|o| o.address.as_str()), Some("sam@example.com"));
    // A reply or cancellation of one occurrence alone says which.
    let one = "BEGIN:VCALENDAR\r\nMETHOD:CANCEL\r\nBEGIN:VEVENT\r\nUID:s\r\nRECURRENCE-ID:20261110T090000Z\r\nDTSTART:20261110T090000Z\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let back = read(one.as_bytes(), &Zone::utc()).unwrap();
    assert_eq!(back.cancelled[0].occurrence, Some(at("2026-11-10 09:00", 0)));
}

#[test]
fn attendees_from_a_stranger_are_capped_and_cannot_break_a_line() {
    let mut text = String::from("BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:x\r\nDTSTART:20261103T140000Z\r\n");
    for i in 0..1_200 {
        text.push_str(&format!("ATTENDEE;CN=\"a\r\n b\";PARTSTAT=WHATEVER:mailto:p{i}@example.com\r\n"));
    }
    text.push_str("ORGANIZER:mailto:o@example.com;\"x\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
    let cal = read(text.as_bytes(), &Zone::utc()).unwrap();
    let e = &cal.events[0];
    assert_eq!(e.attendees.len(), meeting::MAX_ATTENDEES);
    assert!(e.notes.iter().any(|n| n.contains("attendees")));
    assert_eq!(e.organizer.as_ref().unwrap().address, "o@example.comx");
    let mut item = e.appointment.item();
    e.write_as(&mut item, &[]);
    let mut evil = Meeting::of(&item).unwrap();
    evil.attendees[0].name = "Bad\"\r\nATTENDEE:mailto:x@evil".into();
    evil.attendees[0].address = "y@example.com\r\nX:1".into();
    evil.write(&mut item);
    let out = write_as(std::slice::from_ref(&item), &Zone::utc(), 0, None, &Method::Request);
    assert!(!out.split("\r\n").any(|l| l.starts_with("ATTENDEE:mailto:x@evil") || l.starts_with("X:1")));
}

/// To-dos as calendar services and other programs write them. Synthetic, written for this test.
const TODOS: &[u8] = b"BEGIN:VCALENDAR\r
VERSION:2.0\r
PRODID:-//Example//Tasks//EN\r
BEGIN:VTODO\r
UID:todo-1@example.test\r
SUMMARY:Send the print proofs\r
DTSTART;VALUE=DATE:20261102\r
DUE;VALUE=DATE:20261105\r
STATUS:IN-PROCESS\r
PERCENT-COMPLETE:40\r
PRIORITY:1\r
CLASS:CONFIDENTIAL\r
CATEGORIES:Launch,Print\r
DESCRIPTION:Both sizes\\, as agreed.\r
BEGIN:VALARM\r
ACTION:DISPLAY\r
TRIGGER;RELATED=END:-PT12H\r
END:VALARM\r
END:VTODO\r
BEGIN:VTODO\r
UID:todo-2@example.test\r
SUMMARY:Book the room\r
DUE:20261103T170000Z\r
COMPLETED:20261101T090000Z\r
PRIORITY:9\r
RRULE:FREQ=WEEKLY\r
END:VTODO\r
BEGIN:VTODO\r
UID:todo-2@example.test\r
SUMMARY:Book the room again\r
END:VTODO\r
BEGIN:VTODO\r
SUMMARY:Called off\r
STATUS:CANCELLED\r
DTSTART;VALUE=DATE:20261102\r
DURATION:P3D\r
END:VTODO\r
BEGIN:VJOURNAL\r
SUMMARY:A note\r
END:VJOURNAL\r
END:VCALENDAR\r
";

#[test]
fn to_dos_are_read_as_tasks_and_tasks_written_as_to_dos() {
    use openmailclient_model::task::Status;
    let cal = read(TODOS, &Zone::utc()).unwrap();
    assert!(cal.events.is_empty());
    assert_eq!(cal.todos.len(), 3, "{:?}", cal.notes);
    assert!(cal.notes.iter().any(|n| n.contains("journal")), "{:?}", cal.notes);
    assert!(cal.notes.iter().any(|n| n.contains("given twice")), "{:?}", cal.notes);
    let proofs = &cal.todos[0];
    assert_eq!(proofs.uid, "todo-1@example.test");
    let t = &proofs.task;
    assert_eq!(t.subject, "Send the print proofs");
    assert_eq!((t.start, t.due), (Some(day("2026-11-02 00:00")), Some(day("2026-11-05 00:00"))));
    assert_eq!((t.status, t.percent, t.importance, t.private), (Status::InProgress, 40, 2, true));
    assert_eq!(t.categories, vec!["Launch", "Print"]);
    assert_eq!(t.notes, "Both sizes, as agreed.");
    assert_eq!(t.reminder, Some(at("2026-11-04 12:00", 0)), "twelve hours before the day it is due");
    let room = &cal.todos[1];
    assert_eq!((room.task.status, room.task.percent, room.task.importance), (Status::Complete, 100, 0), "done, as COMPLETED says");
    assert_eq!(room.task.completed, Some(day("2026-11-01 00:00")));
    assert!(room.notes.iter().any(|n| n.contains("repeats")));
    let off = &cal.todos[2];
    assert_eq!(off.task.status, Status::Deferred);
    assert_eq!(off.task.due, Some(day("2026-11-05 00:00")), "its start and how long it lasts");
    assert!(!off.uid.is_empty());

    // Written and read again, each comes back as it was; a status RFC 5545 lacks too.
    let now = at("2026-10-27 10:00", 0);
    let mut waiting = cal.todos[0].clone();
    waiting.uid = "todo-3@example.test".into();
    waiting.task.status = Status::Waiting;
    waiting.task.total_work = 90;
    let items: Vec<Item> = [&cal.todos[0], &cal.todos[1], &waiting].iter().map(|t| t.item(day("2026-10-27 00:00"), now, &Zone::utc())).collect();
    let text = write(&items, &Zone::utc(), now, Some("Tasks"));
    assert!(text.contains("BEGIN:VTODO") && !text.contains("BEGIN:VEVENT"), "{text}");
    let again = read(text.as_bytes(), &Zone::utc()).unwrap();
    assert_eq!(again.todos.len(), 3);
    for (a, b) in again.todos.iter().zip([&cal.todos[0], &cal.todos[1], &waiting]) {
        assert_eq!(a.uid, b.uid);
        assert_eq!(a.task, b.task);
    }
    // A meeting's invitation carries no tasks.
    assert!(!write_as(&items, &Zone::utc(), now, None, &Method::Request).contains("VTODO"));
}

#[test]
fn hostile_to_dos_are_bounded() {
    let mut text = String::from("BEGIN:VCALENDAR\r\n");
    for i in 0..3 {
        text.push_str(&format!("BEGIN:VTODO\r\nUID:x{i}\r\nDUE:99999999T999999Z\r\nDTSTART;VALUE=DATE:00000000\r\nPERCENT-COMPLETE:9999\r\nPRIORITY:-4\r\nDURATION:P99999999999D\r\nX-OPENMAILCLIENT-TOTAL-WORK:-77\r\nBEGIN:VALARM\r\nTRIGGER:-P99999999999W\r\nEND:VALARM\r\nEND:VTODO\r\n"));
    }
    text.push_str("END:VCALENDAR\r\n");
    let cal = read(text.as_bytes(), &Zone::utc()).unwrap();
    assert_eq!(cal.todos.len(), 3);
    for t in &cal.todos {
        assert!(t.task.percent <= 100 && t.task.total_work >= 0);
    }
}

/// A zone as written and read again, through its VTIMEZONE alone.
fn zone_through_a_file(zone: &Zone) -> (FileZone, Option<Zone>, String) {
    let mut text = String::from("BEGIN:VCALENDAR\r\n");
    write_zone(&mut text, zone);
    text.push_str("END:VCALENDAR\r\n");
    let (roots, _) = components(text.as_bytes());
    let file = roots[0].children.iter().find(|c| c.name == "VTIMEZONE").and_then(FileZone::of).unwrap();
    let model = file.model();
    (file, model, text)
}

#[test]
fn a_zone_whose_rules_changed_is_written_with_each_era_and_read_back() {
    for id in ["Europe/Moscow", "America/New_York", "Asia/Jerusalem", "Africa/Casablanca", "Europe/Berlin"] {
        let zone = Zone::named(id).unwrap();
        let (file, model, text) = zone_through_a_file(&zone);
        assert_eq!(model.as_ref(), Some(&zone), "{id}:\n{text}");
        // The file's own offsets are the zone's, year by year.
        for y in (1990..=2035).filter(|y| zone.one_rule_in(*y)) {
            for m in 1..=12u32 {
                let local = time::to_unix(at(&format!("{y}-{m:02}-10 12:00"), 0));
                let utc = zone.utc_of(time::from_unix(local));
                assert_eq!(file.offset_at(local), i64::from(zone.offset_at(utc)), "{id} {y}-{m}");
            }
        }
    }
    // Moscow's eras: summer time ending in 2010, four hours to 2014, three since.
    let (_, _, text) = zone_through_a_file(&Zone::named("Europe/Moscow").unwrap());
    assert!(text.contains("UNTIL=20101231T235959Z"), "{text}");
    assert!(text.contains("TZOFFSETTO:+0400"), "{text}");
    // Morocco's moving days as the days they fell on.
    let (_, _, text) = zone_through_a_file(&Zone::named("Africa/Casablanca").unwrap());
    assert!(text.contains("RDATE:") && text.contains("20250223T030000"), "{text}");
    // Under a name not known, the eras that ended are kept as the file gives them.
    let mut own = Zone::named("America/Chicago").unwrap();
    own.id = "Somewhere/Plains".into();
    own.key.clear();
    let (_, model, _) = zone_through_a_file(&own);
    let model = model.unwrap();
    assert_eq!((model.bias, model.daylight, model.earlier.len()), (own.bias, own.daylight, 1));
    assert!(model.keeps_as(&own));
}
