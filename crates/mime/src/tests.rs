use super::*;
use openmailclient_model::time;

const SIMPLE: &str = "From: \"Ana Costa\" <ana@example.org>\r\nTo: Bo Lind <bo@example.org>, cy@example.org\r\nCc: =?UTF-8?B?WsO2ZQ==?= <zoe@example.org>\r\nSubject: =?iso-8859-1?Q?Caf=E9?=\r\n =?iso-8859-1?Q?_menu?=\r\nDate: Mon, 5 Oct 2026 09:14:00 +0200\r\nMessage-ID: <1@example.org>\r\nImportance: high\r\n\r\nHello =\r\nthere\r\n";

#[test]
fn reads_headers_and_plain_body() {
    let it = parse(SIMPLE.as_bytes()).unwrap();
    assert_eq!(it.subject(), "Café menu");
    assert_eq!(it.sender_name(), "Ana Costa");
    assert_eq!(it.sender_email(), "ana@example.org");
    assert_eq!(it.display_recipients(RECIP_TO), "Bo Lind; cy@example.org");
    assert_eq!(it.display_recipients(RECIP_CC), "Zöe");
    assert_eq!(it.importance(), 2);
    assert_eq!(it.time(), time::from_civil(2026, 10, 5, 7, 14, 0, 0));
    assert!(it.body_text().starts_with("Hello ="));
}

#[test]
fn reads_multipart_with_attachment_and_inline_picture() {
    let eml = "From: a@example.org\r\nSubject: Parts\r\nContent-Type: multipart/mixed; boundary=\"XX\"\r\n\r\npreamble\r\n--XX\r\nContent-Type: multipart/related; boundary=YY\r\n\r\n--YY\r\nContent-Type: multipart/alternative; boundary=ZZ\r\n\r\n--ZZ\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nna=C3=AFve=\r\n text\r\n--ZZ\r\nContent-Type: text/html; charset=iso-8859-1\r\n\r\n<p>na\u{ef}ve <img src=\"cid:pic1\"></p>\r\n--ZZ--\r\n--YY\r\nContent-Type: image/png\r\nContent-ID: <pic1>\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0K\r\nGgo=\r\n--YY--\r\n--XX\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERg==\r\n--XX--\r\nepilogue\r\n";
    // The HTML part is Latin-1 on the wire.
    let bytes: Vec<u8> = eml.chars().map(|c| if c == '\u{ef}' { 0xEF } else { c as u8 }).collect();
    let it = parse(&bytes).unwrap();
    assert_eq!(it.body_text(), "naïve text");
    assert!(it.body_html().unwrap().contains("naïve"));
    assert_eq!(it.attachments.len(), 2);
    assert_eq!(it.attachments[0].data, b"\x89PNG\r\n\x1a\n");
    assert_eq!(it.attachments[1].name(), "résumé.pdf");
    assert_eq!(it.attachments[1].data, b"%PDF");
    assert_eq!(it.visible_attachments().count(), 1);
}

#[test]
fn write_then_parse_round_trips() {
    let mut it = Item::new_mail();
    it.set_subject("Größe — quarterly numbers and a long subject line that has to be folded somewhere");
    it.set_sender("Alex Morgan", "alex.morgan@example.org");
    it.add_recipient(RECIP_TO, "Ana \"AC\" Costa", "ana@example.org");
    it.add_recipient(RECIP_TO, "", "bo@example.org");
    it.add_recipient(RECIP_CC, "Zöe Ångström", "zoe@example.org");
    let body = format!("Line one\nA long line: {}\n=equals and trailing space \n\nLast – line", "word ".repeat(40));
    it.set_body_text(&body);
    it.props.set(pid::HTML, PropValue::Binary("<p>Hi <img src=\"cid:logo\"> ✓</p>".as_bytes().to_vec()));
    it.props.set(pid::CLIENT_SUBMIT_TIME, PropValue::Time(time::from_civil(2026, 10, 5, 9, 14, 30, 0)));
    it.props.set(pid::IMPORTANCE, PropValue::Int32(0));
    it.categories = vec!["Red Category".to_string(), "Projekt Ä".to_string()];
    let mut logo = Attachment::file("logo.png", "image/png", vec![0, 1, 2, 255]);
    logo.props.set_str(pid::ATTACH_CONTENT_ID, "logo");
    it.attachments.push(logo);
    it.attachments.push(Attachment::file("Bericht für März.bin", "application/octet-stream", (0..=255u8).cycle().take(1000).collect()));

    let back = parse(&write(&it)).unwrap();
    assert_eq!(back.subject(), it.subject());
    assert_eq!(back.sender_name(), "Alex Morgan");
    assert_eq!(back.display_recipients(RECIP_TO), "Ana \"AC\" Costa; bo@example.org");
    assert_eq!(back.display_recipients(RECIP_CC), "Zöe Ångström");
    assert_eq!(back.body_text(), body);
    assert_eq!(back.body_html(), it.body_html());
    assert_eq!(back.time(), it.time());
    assert_eq!(back.importance(), 0);
    assert!(back.is_unsent());
    assert_eq!(back.categories, it.categories);
    assert_eq!(back.attachments.len(), 2);
    assert_eq!(back.attachments[0].content_id(), Some("logo"));
    assert_eq!(back.attachments[1].name(), "Bericht für März.bin");
    assert_eq!(back.attachments[1].data, it.attachments[1].data);
    for line in String::from_utf8(write(&it)).unwrap().lines() {
        assert!(line.len() <= 78, "line too long: {line}");
    }
}

#[test]
fn embedded_message_is_an_item() {
    let inner = "From: in@example.org\r\nSubject: Inner\r\n\r\ninner body\r\n";
    let eml = format!(
        "From: out@example.org\r\nSubject: Outer\r\nContent-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\nContent-Type: text/plain\r\n\r\nouter\r\n--B\r\nContent-Type: message/rfc822\r\n\r\n{inner}\r\n--B--\r\n"
    );
    let it = parse(eml.as_bytes()).unwrap();
    assert_eq!(it.body_text(), "outer");
    let a = &it.attachments[0];
    assert_eq!(a.name(), "Inner.eml");
    assert_eq!(a.embedded.as_ref().unwrap().body_text().trim(), "inner body");
    // And it survives being written again.
    let back = parse(&write(&it)).unwrap();
    assert_eq!(back.attachments[0].embedded.as_ref().unwrap().subject(), "Inner");
}

#[test]
fn dates_in_old_and_new_forms() {
    let want = time::from_civil(2026, 10, 5, 13, 14, 0, 0);
    assert_eq!(parse_date("Mon, 5 Oct 2026 09:14:00 -0400 (EDT)"), Some(want));
    assert_eq!(parse_date("5 Oct 2026 09:14 EDT"), Some(want));
    assert_eq!(parse_date("Mon, 05 Oct 26 13:14:00 GMT"), Some(want));
    assert_eq!(parse_date("yesterday"), None);
    assert_eq!(parse_date("99 Oct 2026 09:14"), None);
    assert_eq!(parse_date("5 Oct 99999999999999999999 09:14"), None);
}

#[test]
fn address_lists() {
    assert_eq!(
        parse_addresses("\"Costa, Ana\" <ana@example.org>; bo@example.org (Bo), Team: cy@example.org;"),
        vec![
            ("Costa, Ana".to_string(), "ana@example.org".to_string()),
            ("Bo".to_string(), "bo@example.org".to_string()),
            (String::new(), "cy@example.org".to_string()),
        ]
    );
    assert_eq!(parse_addresses("undisclosed-recipients:;"), vec![]);
}

#[test]
fn hostile_input_never_panics() {
    assert!(matches!(parse(b""), Err(MimeError::Empty)));
    assert!(matches!(parse(b"   \r\n"), Err(MimeError::Empty)));
    assert!(matches!(parse(b"no header here"), Err(MimeError::NotMail)));
    // Nested far past the depth cap.
    let mut deep = String::from("Subject: deep\r\n");
    for i in 0..200 {
        deep.push_str(&format!("Content-Type: multipart/mixed; boundary=b{i}\r\n\r\n--b{i}\r\n"));
    }
    deep.push_str("Content-Type: text/plain\r\n\r\nbottom\r\n");
    assert!(parse(deep.as_bytes()).is_ok());
    // A boundary that never closes, empty boundary, broken encodings and parameters.
    for eml in [
        "Subject: x\r\nContent-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\n\r\npart never ends",
        "Subject: x\r\nContent-Type: multipart/mixed; boundary=\"\"\r\n\r\n--\r\n--\r\n",
        "Subject: =?utf-8?B?!!!?= =?x?Q?=ZZ?= =?\r\nContent-Type: text/plain; charset=nope; name*0*=''%; name*1=\"\r\nContent-Transfer-Encoding: base64\r\n\r\n=====\r\n",
        "Subject: x\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n=\r\n=4\r\n=",
        "To: <<<<>>>>,,,;;;\"\\\r\nDate: , , ,\r\n\r\n",
        ":\r\n: :\r\n\r\n",
    ] {
        let _ = parse(eml.as_bytes());
    }
    // Every truncation and a byte-flipped copy of a real message.
    let good = write(&parse(SIMPLE.as_bytes()).unwrap());
    for n in 0..good.len() {
        let _ = parse(&good[..n]);
        let mut bad = good.clone();
        bad[n] = bad[n].wrapping_mul(31).wrapping_add(n as u8);
        let _ = parse(&bad);
    }
}

#[test]
fn a_meeting_message_carries_its_calendar_as_another_way_to_read_it() {
    let mut item = Item::default();
    item.props.set_str(pid::MESSAGE_CLASS, "IPM.Schedule.Meeting.Request");
    item.props.set_str(pid::SUBJECT, "Planning");
    item.set_body_text("When: Tuesday");
    let ics = b"BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:p\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n".to_vec();
    item.attachments.push(Attachment::file("invite.ics", "text/calendar", ics.clone()));
    item.attachments.push(Attachment::file("agenda.txt", "text/plain", b"1. Plans".to_vec()));
    let eml = String::from_utf8(write(&item)).unwrap();
    assert!(eml.contains("multipart/alternative"));
    assert!(eml.contains("Content-Type: text/calendar; charset=\"utf-8\"; method=REQUEST\r\n"));
    assert!(!eml.contains("invite.ics"), "not an attachment as well");
    let back = parse(eml.as_bytes()).unwrap();
    assert_eq!(back.message_class(), "IPM.Schedule.Meeting.Request");
    assert_eq!(back.body_text().trim(), "When: Tuesday");
    let cal = back.attachments.iter().find(|a| a.mime() == "text/calendar").unwrap();
    assert_eq!(cal.data, ics);
    assert!(back.attachments.iter().any(|a| a.name() == "agenda.txt"));

    // Answers are classed by what they say; a calendar that is only published is not a meeting's.
    for (body, class) in [
        ("METHOD:REPLY\r\nATTENDEE;PARTSTAT=DECLINED:mailto:a@example.com", "IPM.Schedule.Meeting.Resp.Neg"),
        ("METHOD:REPLY\r\nATTENDEE;PARTSTAT=TENTATIVE:mailto:a@example.com", "IPM.Schedule.Meeting.Resp.Tent"),
        ("METHOD:CANCEL", "IPM.Schedule.Meeting.Canceled"),
        ("METHOD:PUBLISH", "IPM.Note"),
        ("METHOD:REPLY", "IPM.Note"),
    ] {
        let eml = format!("Subject: x\r\nContent-Type: text/calendar; method=X\r\n\r\nBEGIN:VCALENDAR\r\n{body}\r\nEND:VCALENDAR\r\n");
        assert_eq!(parse(eml.as_bytes()).unwrap().message_class(), class, "{body}");
    }
    // A plain message that is not a meeting's keeps its calendar as an attachment.
    item.props.set_str(pid::MESSAGE_CLASS, "IPM.Note");
    assert!(String::from_utf8(write(&item)).unwrap().contains("invite.ics"));
}

#[test]
fn a_meeting_sent_as_its_calendar_alone_says_what_it_says_in_its_body() {
    // As Outlook sends one: no other way of reading it than the calendar, lines folded with tabs.
    let eml = "From: Sam <sam@example.test>\r\nTo: pat@example.test\r\nSubject: Planning\r\nContent-Type: text/calendar; method=REQUEST;\r\n\tcharset=\"utf-8\"\r\n\r\nBEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nDESCRIPTION;LANGUAGE=en:Plans for the\r\n\t next quarter\\, in Room 2\\n\\nSam\\n\r\nSUMMARY:Planning\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let item = parse(eml.as_bytes()).unwrap();
    assert_eq!(item.message_class(), "IPM.Schedule.Meeting.Request");
    assert_eq!(item.body_text().trim_end(), "Plans for the next quarter, in Room 2\n\nSam");
    // A quoted colon in a parameter is not where the value starts.
    assert_eq!(crate::write::calendar_text(b"DESCRIPTION;ALTREP=\"cid:a:b\":Words\r\n", "DESCRIPTION").as_deref(), Some("Words"));
    assert_eq!(crate::write::calendar_text(b"DESCRIPTIONS:no\r\nSUMMARY:x\r\n", "DESCRIPTION"), None);
    assert_eq!(crate::write::calendar_text(b"DESCRIPTION:\\", "DESCRIPTION"), None);
    assert_eq!(crate::write::calendar_text(b"", "DESCRIPTION"), None);
    assert_eq!(crate::write::calendar_text(&[0xFF; 300], "DESCRIPTION"), None);
    // Long words are cut, not kept whole.
    let long = format!("DESCRIPTION:{}\r\n", "é".repeat(50_000));
    assert_eq!(crate::write::calendar_text(long.as_bytes(), "DESCRIPTION").unwrap().chars().count(), 20_000);
}

#[test]
fn a_subject_keeps_its_white_space_through_a_message_file() {
    for subject in ["Subject with trailing space ", " leading", "two  spaces", "a\ttab", "plain words stay plain", "  "] {
        let mut it = Item::new_mail();
        it.set_subject(subject);
        it.set_sender("Alex Morgan", "alex.morgan@example.org");
        let written = write::write(&it);
        let text = String::from_utf8_lossy(&written);
        let field = text.lines().find(|l| l.starts_with("Subject:")).unwrap();
        // Only what folding would change is encoded.
        assert_eq!(field.contains("=?UTF-8?"), subject != "plain words stay plain", "{field}");
        assert_eq!(parse(&written).unwrap().subject(), subject);
    }
    // A field folded by someone else keeps the white space after the break.
    let folded = "Subject: one\r\n  two\r\n\tthree\r\n\r\nbody";
    assert_eq!(parse(folded.as_bytes()).unwrap().subject(), "one  two\tthree");
}
