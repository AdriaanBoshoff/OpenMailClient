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
    assert!(cal.notes.iter().any(|n| n.contains("tasks")));
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
