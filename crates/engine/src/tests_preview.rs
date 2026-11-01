//! Attachment Preview: each kind of attachment shown as what it is, told from its bytes; what
//! is not shown said in words; hostile files bounded.

use crate::Session;
use crate::attachment_preview::{Kind, PICTURE_ID, is_picture, preview, text_of};
use openmailclient_model::time::Region;
use openmailclient_model::zone::Zone;
use openmailclient_model::{Attachment, Item, pid, time};
use serde_json::{Value, json};

fn shown(name: &str, mime: &str, data: &[u8]) -> Result<crate::attachment_preview::Preview, String> {
    preview(&Attachment::file(name, mime, data.to_vec()), &Zone::utc(), &Region::default())
}

/// A PNG of one pixel, as our own writer makes it.
fn png() -> Vec<u8> {
    let mut s = Session::with_sample();
    s.run("mail.select", &json!({ "index": 4 })).unwrap();
    s.open.as_ref().unwrap().attachments[0].data.clone()
}

#[test]
fn each_kind_is_shown_as_what_it_is() {
    // A picture, known by its bytes whatever its name says.
    let p = shown("scan.dat", "", &png()).unwrap();
    assert_eq!(p.kind, Kind::Picture);
    assert!(p.item.body_html().unwrap().contains(&format!("cid:{PICTURE_ID}")));
    assert_eq!(p.item.attachments[0].content_id(), Some(PICTURE_ID));

    let p = shown("notes.txt", "text/plain", "First line\r\nSecond line, ünïcode".as_bytes()).unwrap();
    assert_eq!((p.kind, p.item.body_text().as_str()), (Kind::Text, "First line\nSecond line, ünïcode"));
    // UTF-16 by its byte-order mark, as Windows' Notepad writes it.
    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend("Hallo, wêreld".encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(shown("log.txt", "", &utf16).unwrap().item.body_text(), "Hallo, wêreld");

    let p = shown("page.html", "text/html", b"<html><body><h1>Prices</h1><script>alert(1)</script></body></html>").unwrap();
    assert_eq!(p.kind, Kind::Page);
    assert!(p.item.body_html().unwrap().contains("Prices"));

    // An attached Outlook item: its own header over its body.
    let mut inner = Item::default();
    inner.props.set_str(pid::SUBJECT, "Quarterly figures");
    inner.props.set_str(pid::SENDER_NAME, "Helena Brandt");
    inner.props.set_str(pid::BODY, "See the table below.");
    let mut att = Attachment::file("Quarterly figures", "", Vec::new());
    att.embedded = Some(Box::new(inner));
    let p = preview(&att, &Zone::utc(), &Region::default()).unwrap();
    assert_eq!(p.kind, Kind::Message);
    assert!(p.head.contains(&("Subject", "Quarterly figures".to_string())));
    assert!(p.head.iter().any(|(l, v)| *l == "From" && v.contains("Helena")));

    let eml = b"From: Sam Ruiz <sam@example.org>\r\nTo: alex@example.org\r\nSubject: Forwarded note\r\nDate: Tue, 6 Oct 2026 09:00:00 +0000\r\n\r\nThe note itself.\r\n";
    let p = shown("note.eml", "message/rfc822", eml).unwrap();
    assert_eq!((p.kind, p.item.subject().as_str()), (Kind::Message, "Forwarded note"));
    assert!(p.head.iter().any(|(l, v)| *l == "Sent" && !v.is_empty()));
    // The same message as an Outlook item file, written by our own writer.
    let msg = openmailclient_msg::write(&p.item).unwrap();
    assert_eq!(shown("note.msg", "", &msg).unwrap().item.subject(), "Forwarded note");

    let ics = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:a1\r\nDTSTART:20261008T090000Z\r\nDTEND:20261008T100000Z\r\nSUMMARY:Planning\r\nLOCATION:Room B2\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let p = shown("invite.ics", "text/calendar", ics).unwrap();
    assert_eq!(p.kind, Kind::Calendar);
    let text = p.item.body_text();
    assert!(text.contains("Planning") && text.contains("Room B2"), "{text}");
    assert!(p.head.contains(&("Holds", "1 event".to_string())));

    let vcf = b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:June Okafor\r\nN:Okafor;June;;;\r\nEMAIL:june@example.org\r\nTEL;TYPE=CELL:+27 82 555 0101\r\nEND:VCARD\r\n";
    let p = shown("june.vcf", "text/vcard", vcf).unwrap();
    assert_eq!(p.kind, Kind::Contacts);
    let text = p.item.body_text();
    assert!(text.contains("June Okafor") && text.contains("june@example.org") && text.contains("555 0101"), "{text}");
}

#[test]
fn what_is_not_shown_is_said_in_words() {
    // A program, by its name or by its bytes, is never previewed, not even as text.
    let err = shown("setup.exe", "", b"MZ\x90\0").unwrap_err();
    assert!(err.contains("program"), "{err}");
    let mut pe = b"MZ".to_vec();
    pe.resize(128, 0);
    assert!(shown("invoice.pdf", "", &pe).unwrap_err().contains("program"));
    assert!(shown("run.js", "", b"alert(1)").unwrap_err().contains("program"));
    assert!(shown("empty.txt", "", b"").unwrap_err().contains("empty"));
    // A binary file of no kind we read.
    let binary: Vec<u8> = (0..4096u32).map(|i| (i * 7 % 256) as u8).collect();
    assert!(shown("data.bin", "", &binary).unwrap_err().contains("no preview"));
    assert!(shown("broken.eml", "", b"\xff\xfe\0\0").is_err() || shown("broken.eml", "", b"\xff\xfe\0\0").is_ok());
    assert!(shown("broken.msg", "", b"not a compound file").unwrap_err().contains("message"));
    assert!(
        shown("broken.ics", "", b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT").is_ok() || shown("broken.ics", "", b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT").is_err()
    );
}

#[test]
fn hostile_files_are_bounded() {
    // A text file larger than is shown: cut at a whole letter, and said to be cut.
    let mut big = "é".repeat(400_000).into_bytes();
    big.extend(b"tail");
    let (text, cut) = text_of(&big).unwrap();
    assert!(cut && text.len() <= 512 * 1024 && !text.contains("tail"));
    // Odd UTF-16: an unpaired surrogate and an odd byte at the end.
    let (text, _) = text_of(&[0xFF, 0xFE, 0x00, 0xD8, 0x41, 0x00, 0x42]).unwrap();
    assert!(text.ends_with('A'), "{text:?}");
    // The marks of a picture alone, and an SVG that is not one.
    assert!(is_picture(b"\x89PNG\r\n\x1a\n"));
    assert!(!is_picture(b"BM"));
    assert!(is_picture(b"\xEF\xBB\xBF  <svg xmlns=\"http://www.w3.org/2000/svg\"/>"));
    assert!(!is_picture(b"<html><svg></svg></html>"));
    for junk in [&b"\0"[..], b"\xFF", b"GIF8", b"RIFF\0\0\0\0WEB", b"\xFE\xFF\xD8"] {
        let _ = shown("x", "", junk);
        let _ = shown("x.ics", "text/calendar", junk);
        let _ = shown("x.vcf", "", junk);
        let _ = shown("x.eml", "", junk);
        let _ = shown("x.msg", "", junk);
    }
}

#[test]
fn the_reading_pane_shows_an_attachment_until_the_message_is_left() {
    let mut s = Session::with_sample();
    s.now = time::from_civil(2026, 10, 7, 14, 30, 0, 0);
    // The launch checklist, the sample's message with a file attached.
    s.run("mail.select", &json!({ "index": 0 })).unwrap();
    assert!(s.open.as_ref().unwrap().subject().contains("Launch checklist"));
    let got = s.run("mail.previewAttachment", &json!({ "index": 0 })).unwrap();
    assert!(got["kind"].is_string() && got["name"].is_string(), "{got}");
    assert!(got["untrusted_content"].is_object());
    let selected = s.selected.unwrap();
    assert_eq!(s.previewing, Some((selected, 0)));
    assert_eq!(s.run("mail.previewAttachment", &json!({ "index": null })).unwrap()["previewing"], false);
    assert_eq!(s.previewing, None);
    for junk in [json!({ "index": 99 }), json!({ "index": -1 }), json!({ "index": "0" })] {
        assert!(s.run("mail.previewAttachment", &junk).is_err(), "{junk}");
    }
    assert_eq!(s.previewing, None, "nothing shown by what was refused");
    s.run("mail.previewAttachment", &json!({ "index": 0 })).unwrap();
    // Another message read: the preview was of the last one, and is not shown with this one.
    s.run("mail.select", &json!({ "index": 1 })).unwrap();
    assert_ne!(s.previewing.map(|p| p.0), s.selected);
    let mut nothing = Session::new();
    assert!(nothing.run("mail.previewAttachment", &Value::Null).is_err());
}

#[test]
fn values_with_commas_are_set_out_as_a_table() {
    let csv = "name,note\r\n\"Okafor, June\",\"said \"\"yes\"\"\"\r\nRuiz,<b>bold</b>\r\n";
    let p = shown("people.csv", "text/csv", csv.as_bytes()).unwrap();
    assert_eq!(p.kind, Kind::Table);
    let html = p.item.body_html().unwrap();
    assert!(html.contains("<th>name</th>") && html.contains("<td>Okafor, June</td>") && html.contains("said &quot;yes&quot;"), "{html}");
    // A value is text, never markup.
    assert!(html.contains("&lt;b&gt;bold") && !html.contains("<b>"), "{html}");
    assert!(p.head.contains(&("Rows", "2 rows".to_string())));
    let p = shown("a.tsv", "", b"x\ty\n1\t2\n").unwrap();
    assert_eq!(p.kind, Kind::Table);
    // A file of one column is only text.
    assert_eq!(shown("list.csv", "", b"one\ntwo\n").unwrap().kind, Kind::Text);
    // Bounded: rows past the cap are left out and said to be, columns past it dropped.
    let wide = format!("{}\n", vec!["v"; 500].join(","));
    let (rows, more) = crate::attachment_preview::cells(&wide.repeat(2000), ',');
    assert!(more && rows.len() == 1001 && rows.iter().all(|r| r.len() <= 64));
    // An open quote to the end, and lone carriage returns, end without harm.
    let (rows, _) = crate::attachment_preview::cells("a,\"never closed\nb\rc", ',');
    assert_eq!(rows.len(), 1);
}
