//! The sample mailbox: invented people at example.org writing invented mail. Every word here is
//! ours; nothing comes from a real mailbox.

use openmailclient_model::{Attachment, Flag, FolderKind, Item, MSGFLAG_READ, MSGFLAG_UNSENT, MemStore, PropValue, RECIP_CC, RECIP_TO, pid, rtf};

const MINUTE: i64 = 60 * 10_000_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

const ME: (&str, &str) = ("Alex Morgan", "alex.morgan@example.org");

struct Mail {
    from: (&'static str, &'static str),
    to: &'static [(&'static str, &'static str)],
    cc: &'static [(&'static str, &'static str)],
    subject: &'static str,
    body: &'static str,
    html: Option<&'static str>,
    age: i64,
    read: bool,
}

impl Mail {
    fn item(&self, now: i64) -> Item {
        let mut it = Item::new_mail();
        it.set_subject(self.subject);
        it.set_body_text(self.body);
        if let Some(html) = self.html {
            it.props.set(pid::HTML, PropValue::Binary(html.as_bytes().to_vec()));
            it.props.set(pid::INTERNET_CODEPAGE, PropValue::Int32(65001));
        }
        it.set_sender(self.from.0, self.from.1);
        for (name, email) in self.to {
            it.add_recipient(RECIP_TO, name, email);
        }
        for (name, email) in self.cc {
            it.add_recipient(RECIP_CC, name, email);
        }
        let when = now.saturating_sub(self.age);
        it.props.set(pid::CLIENT_SUBMIT_TIME, PropValue::Time(when.saturating_sub(MINUTE)));
        it.props.set(pid::MESSAGE_DELIVERY_TIME, PropValue::Time(when));
        it.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(if self.read { MSGFLAG_READ } else { 0 }));
        it
    }
}

const PRIYA: (&str, &str) = ("Priya Raman", "priya.raman@example.org");
const TOMAS: (&str, &str) = ("Tomás Lindqvist", "tomas.lindqvist@example.org");
const JUNE: (&str, &str) = ("June Okafor", "june.okafor@example.org");
const FACILITIES: (&str, &str) = ("Facilities Desk", "facilities@example.org");
const DEPOT: (&str, &str) = ("Northgate Print Depot", "orders@printdepot.example.com");
const HELENA: (&str, &str) = ("Helena Brandt", "helena.brandt@example.org");
const TRAVEL: (&str, &str) = ("Travel Bookings", "bookings@travel.example.net");
const PARKING: (&str, &str) = ("Riverside Car Park", "permits@parking.example.net");

/// A receipt kept only as RTF, the way older mail in a data file often is: no plain-text and no
/// HTML body beside it.
const PERMIT_RTF: &str = "{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0\\fswiss Arial;}{\\f1\\fmodern Courier New;}}\
{\\colortbl;\\red0\\green102\\blue153;\\red255\\green242\\blue204;}\r\n\
{\\*\\listtable{\\list{\\listlevel\\levelnfc0\\levelstartat1{\\leveltext\\'02\\'00.;}{\\levelnumbers\\'01;}}\\listid1}}\
{\\*\\listoverridetable{\\listoverride\\listid1\\listoverridecount0\\ls1}}\r\n\
\\pard\\qc\\b\\fs32\\cf1 Riverside Car Park\\b0\\fs20\\cf0\\par\r\n\
\\pard Receipt for permit {\\f1 RP-20418}, paid by card. Thank you.\\par\\par\r\n\
\\trowd\\clbrdrt\\brdrs\\clbrdrb\\brdrs\\cellx4200\\clbrdrt\\brdrs\\clbrdrb\\brdrs\\cellx6000\\intbl\\b Item\\cell Amount\\b0\\cell\\row\r\n\
\\trowd\\clmgf\\clbrdrt\\brdrs\\clbrdrb\\brdrs\\cellx4200\\clmrg\\clbrdrt\\brdrs\\clbrdrb\\brdrs\\cellx6000\\intbl\\i October, car park B\\i0\\cell\\cell\\row\r\n\
\\trowd\\clbrdrt\\brdrs\\clbrdrb\\brdrs\\cellx4200\\clbrdrt\\brdrs\\clbrdrb\\brdrs\\cellx6000\\intbl Monthly permit, bay 14\\cell 62.00\\cell\\row\r\n\
\\trowd\\clbrdrt\\brdrs\\clbrdrb\\brdrs\\cellx4200\\clbrdrt\\brdrs\\clbrdrb\\brdrs\\cellx6000\\intbl {\\highlight2 Total paid}\\cell {\\b 62.00}\\cell\\row\r\n\
\\trowd\\clbrdrt\\brdrs\\clbrdrb\\brdrs\\cellx4200\\clbrdrt\\brdrs\\clbrdrb\\brdrs\\cellx6000\\pard\\intbl Paid:\\par\r\n\
\\pard\\intbl\\itap2 By\\nestcell card ending 4417\\nestcell{\\*\\nesttableprops\\trowd\\clbrdrb\\brdrs\\cellx900\\clbrdrb\\brdrs\\cellx3600\\nestrow}{\\nonesttables\\par}\r\n\
\\pard\\intbl\\itap2 On\\nestcell 3 October\\nestcell{\\*\\nesttableprops\\trowd\\cellx900\\cellx3600\\nestrow}{\\nonesttables\\par}\r\n\
\\pard\\intbl\\cell\\pard\\intbl\\cell\\row\r\n\
\\pard\\sb240\\sa120\\li720\\brdrl\\brdrs\\brdrw45\\brdrcf1\\brsp120 Keep this receipt: the barrier reads the permit number, not the number plate.\\par\r\n\
\\pard To use the permit:\\par\r\n\
{\\listtext\\pard\\plain 1.\\tab}\\pard\\ls1\\fi-360\\li720\\sl360\\slmult1 Print the card below.\\par\r\n\
Leave it where it can be read through the windscreen.\\par\r\n\
\\pard\\sb120\\tx2100\\tqdec\\tldot\\tx4800 Next payment\\tab 1 November\\tab 62.00\\par\r\n\
\\pard\\tx2100\\tqdec\\tldot\\tx4800 Late fee\\tab after the 5th\\tab 7.50\\par\r\n\
\\pard\\par Your permit card, to print and keep in the car: \\objattph\\'20\\par\r\n\
\\pard\\par\\qc \\objattph\\'20\\par\r\n\
\\pard\\par Bay 14 is the green one, third from the way in:\\par\r\n\
\\pard\\qc {\\pict\\wmetafile8\\picw6350\\pich3175\\picwgoal3600\\pichgoal1800 @MAP@}\\par\r\n\
\\pard\\par Questions? See {\\field{\\*\\fldinst HYPERLINK \"https://parking.example.net/help\"}{\\fldrslt{\\ul\\cf1 parking.example.net/help}}}.\\par}";

/// A shop's order mail the way such mail is built: a layout table, a banner, a data table, a
/// remote logo and a tracking pixel (neither is ever fetched), a picture sent along with the
/// message (`cid:`) and a hidden preview line.
const DEPOT_HTML: &str = "<html><head><style>td{font-family:sans-serif}</style></head><body bgcolor=\"#ffffff\">\
<span style=\"display:none\">Order 48213 is ready at the front counter.</span>\
<table width=\"600\" align=\"center\" cellpadding=\"0\" cellspacing=\"0\"><tr><td bgcolor=\"#1F4E79\" align=\"center\">\
<img src=\"https://printdepot.example.com/logo.png\" alt=\"Northgate Print Depot\" width=\"180\" height=\"40\">\
<font color=\"#ffffff\" size=\"5\"><b>&nbsp;Your order is ready&nbsp;</b></font></td></tr>\
<tr><td><h2>Order 48213</h2>\
<p>Hello Alex Morgan,</p>\
<p>Everything in your order has been printed and checked. You can collect it from the <b>front counter</b>, \
weekdays <span style=\"color:#2E7D32;font-weight:bold\">8:30 to 17:00</span>.</p>\
<table border=\"1\" cellpadding=\"4\"><tr><th align=\"left\">Item</th><th align=\"right\">Quantity</th><th align=\"right\">Price</th></tr>\
<tr><td>Folded leaflets, A5, full colour</td><td align=\"right\">200</td><td align=\"right\">64.00</td></tr>\
<tr><td>Roll-up banner, 85 &times; 200 cm</td><td align=\"right\">2</td><td align=\"right\">118.00</td></tr>\
<tr><td colspan=\"2\"><b>Total</b></td><td align=\"right\"><b>182.00</b></td></tr></table>\
<p>The proof of your banner, as printed:<br><img src=\"cid:proof-48213@printdepot.example.com\" alt=\"Proof of the roll-up banner\" width=\"240\" height=\"96\"></p>\
<p>Please bring this message with you. <a href=\"https://printdepot.example.com/orders/48213\">View the order online</a> \
or <a href=\"mailto:orders@printdepot.example.com\">write to us</a> if anything looks wrong.</p>\
<blockquote><i>Uncollected orders are kept for <u>30 days</u>.</i></blockquote>\
<hr><p align=\"center\"><small>Northgate Print Depot &middot; 14 Canal Row &middot; <s>Unit 3</s> Unit 5</small></p>\
<img src=\"https://printdepot.example.com/t/open.gif\" width=\"1\" height=\"1\"></td></tr></table></body></html>";

const LAUNCH_HTML: &str = "<html><body style=\"font-family:Aptos,Calibri,sans-serif;font-size:11pt\">\
<p>Hi Alex,</p>\
<p>The <b>launch checklist</b> is in good shape. Three things are still open:</p>\
<ul><li>Final sign-off on the pricing page</li><li>Support rota for the first week</li><li>The <i>welcome</i> email copy</li></ul>\
<p>Can you take the first one before <span style=\"color:#C00000\">Thursday noon</span>? I have attached the current timeline.</p>\
<div style=\"background:#DCEBF7;border-left:4px solid #1F4E79;padding:8px 14px 8px 12px\"><b>Reminder</b><br>\
The rehearsal is on Wednesday at 15:00 in the small studio.</div>\
<p>Thanks,<br>Priya</p></body></html>";

const LURE_HTML: &str = r#"<html><body style="font-family:Arial,sans-serif;font-size:11pt">
<p>Your mailbox has reached <b>98%</b> of its quota.</p>
<p>Confirm your account within 24 hours to keep receiving mail:</p>
<p><a href="https://mail.example.org.account-check.example.net/quota">https://mail.example.org/quota</a></p>
<p>Or sign in at <a href="https://mail.example.org@sign-in.example.net/">mail.example.org</a>,
or use <a href="http://192.0.2.44/owa">the web version</a>.</p>
<p>Questions go to <a href="mailto:helpdesk@mailbox-notice.example.net">helpdesk@example.org</a>.
Our real help pages are at <a href="https://example.org/help">example.org/help</a>.</p>
<p>Mail Administrator</p>
</body></html>"#;

/// A writable in-memory mailbox dated around `now` (UTC FILETIME ticks).
pub fn sample_store(now: i64) -> MemStore {
    let mut s = MemStore::with_default_folders("alex.morgan@example.org");
    let folder = |s: &MemStore, kind| s.folder_by_kind(kind);
    let (Some(inbox), Some(sent), Some(drafts), Some(deleted)) =
        (folder(&s, FolderKind::Inbox), folder(&s, FolderKind::Sent), folder(&s, FolderKind::Drafts), folder(&s, FolderKind::Deleted))
    else {
        return s;
    };
    let projects = s.push_folder(Some(inbox), "Projects", "IPF.Note");
    let receipts = s.push_folder(Some(inbox), "Receipts", "IPF.Note");

    let permit = Mail { from: PARKING, to: &[ME], cc: &[], subject: "Your parking permit receipt", body: "", html: None, age: 12 * DAY, read: true };
    let mut permit = permit.item(now);
    permit.props.remove(pid::BODY);
    permit.props.set(pid::RTF_COMPRESSED, PropValue::Binary(rtf::store(PERMIT_RTF.replace("@MAP@", &hex(&bay_map())).as_bytes())));
    permit.props.set(pid::RTF_IN_SYNC, PropValue::Bool(true));
    // An attachment that has its place in the body's text, as RTF mail keeps them.
    let mut card = Attachment::file("permit-RP-20418.txt", "text/plain", b"Riverside Car Park\r\nPermit RP-20418, bay 14\r\n".to_vec());
    card.props.set(pid::RENDERING_POSITION, PropValue::Int32(0));
    permit.attachments.push(card);
    // And an object another program would have made: here a picture of the card, drawn below.
    if let Ok(object) = openmailclient_msg::ole::picture_object("Permit card", &card_bitmap()) {
        let mut picture = Attachment::file("Permit card", "", object);
        picture.props.set(pid::ATTACH_METHOD, PropValue::Int32(6));
        picture.props.set(pid::RENDERING_POSITION, PropValue::Int32(1));
        permit.attachments.push(picture);
    }
    permit.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(0x11));
    s.push_item(receipts, permit);

    let mut launch = Mail {
        from: PRIYA,
        to: &[ME],
        cc: &[TOMAS],
        subject: "Launch checklist: three items left",
        body: "Hi Alex,\n\nThe launch checklist is in good shape. Three things are still open:\n\n  - Final sign-off on the pricing page\n  - Support rota for the first week\n  - The welcome email copy\n\nCan you take the first one before Thursday noon? I have attached the current timeline.\n\nThanks,\nPriya",
        html: Some(LAUNCH_HTML),
        age: 25 * MINUTE,
        read: false,
    }
    .item(now);
    launch.attachments.push(Attachment::file(
        "launch-timeline.csv",
        "text/csv",
        b"milestone,owner,due\nPricing page sign-off,Alex,Thursday\nSupport rota,June,Friday\nWelcome email,Priya,Friday\n".to_vec(),
    ));
    launch.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(0x10));
    launch.props.set(pid::IMPORTANCE, PropValue::Int32(2));
    launch.set_flag(Flag::Flagged);
    launch.categories = vec!["Red category".to_string()];
    s.push_item(inbox, launch);

    let inbox_mail = [
        Mail {
            from: TOMAS,
            to: &[ME, PRIYA],
            cc: &[],
            subject: "RE: Pricing page wording",
            body: "Works for me. I would drop the second paragraph entirely: it repeats the table above it.\n\nIf we keep it, \"per seat\" needs to be \"per person\" to match the rest of the site.\n\nTomás",
            html: None,
            age: 2 * HOUR + 10 * MINUTE,
            read: false,
        },
        Mail {
            from: FACILITIES,
            to: &[ME],
            cc: &[],
            subject: "Lift maintenance on Friday morning",
            body: "The east lift is out of service on Friday from 7:00 to 10:00 for its yearly inspection. The west lift and the stairs stay open.\n\nDeliveries that need the goods lift should be booked for the afternoon.\n\nFacilities Desk",
            html: None,
            age: 5 * HOUR,
            read: true,
        },
        Mail {
            from: JUNE,
            to: &[ME],
            cc: &[],
            subject: "Support rota draft",
            body: "Alex,\n\nFirst pass at the rota for launch week is below. I have you down for Tuesday and Thursday mornings; shout if that clashes with anything.\n\nMon: June, Tomás\nTue: Alex, Helena\nWed: Priya, June\nThu: Alex, Tomás\nFri: Helena, Priya\n\nJune",
            html: None,
            age: DAY + 3 * HOUR,
            read: false,
        },
        Mail {
            from: DEPOT,
            to: &[ME],
            cc: &[],
            subject: "Your order 48213 is ready for collection",
            body: "Hello Alex Morgan,\n\nYour order 48213 (200 folded leaflets, 2 roll-up banners) is ready. Collection is from the front counter, weekdays 8:30 to 17:00. Please bring this message with you.\n\nNorthgate Print Depot",
            html: Some(DEPOT_HTML),
            age: DAY + 7 * HOUR,
            read: true,
        },
        Mail {
            from: HELENA,
            to: &[ME],
            cc: &[JUNE],
            subject: "Notes from the customer call",
            body: "Short version: they like the import tool, they are worried about the migration window, and they want a named contact for the first month.\n\nI promised a written plan by the end of next week. Can we find thirty minutes on Monday to split it up?\n\nHelena",
            html: None,
            age: 2 * DAY + 2 * HOUR,
            read: true,
        },
        Mail {
            from: TRAVEL,
            to: &[ME],
            cc: &[],
            subject: "Itinerary: Rotterdam, two nights",
            body: "Booking reference KX7Q2M\n\nOutbound: Tuesday 07:42, platform to be announced, seat 54 coach 8\nReturn: Thursday 18:15, seat 31 coach 4\nHotel: Maaskade Rooms, two nights, breakfast included\n\nChanges are free until 24 hours before departure.",
            html: None,
            age: 4 * DAY,
            read: true,
        },
        Mail {
            from: PRIYA,
            to: &[ME],
            cc: &[],
            subject: "Welcome email copy, second draft",
            body: "Shorter this time, as you asked. The opening line now says what the product does instead of how excited we are.\n\nStill unsure about the sign-off. \"The team\" feels anonymous; a name might be better.\n\nPriya",
            html: None,
            age: 8 * DAY + 4 * HOUR,
            read: true,
        },
        Mail {
            from: TOMAS,
            to: &[ME],
            cc: &[],
            subject: "Budget numbers for the offsite",
            body: "Venue 1,800, catering 950, travel about 1,200 depending on who drives. That leaves a little over 500 of the 4,500 for anything we forgot.\n\nTomás",
            html: None,
            age: 16 * DAY,
            read: true,
        },
    ];
    for (i, m) in inbox_mail.iter().enumerate() {
        let mut it = m.item(now);
        if m.from == DEPOT {
            let mut proof = Attachment::file("banner-proof.png", "image/png", banner_proof());
            proof.props.set_str(pid::ATTACH_CONTENT_ID, "proof-48213@printdepot.example.com");
            it.attachments.push(proof);
        }
        match i {
            2 => it.categories = vec!["Blue category".to_string()],
            4 => it.set_flag(Flag::Complete),
            _ => {}
        }
        s.push_item(inbox, it);
    }

    let project_mail = [
        Mail {
            from: HELENA,
            to: &[ME],
            cc: &[],
            subject: "Import tool: field mapping",
            body: "The mapping table is done for contacts and companies. Deals are next; the stage names differ between the two systems, so that one needs a decision from you.\n\nHelena",
            html: None,
            age: 3 * DAY,
            read: true,
        },
        Mail {
            from: JUNE,
            to: &[ME, HELENA],
            cc: &[],
            subject: "Import tool: test results",
            body: "Ran the importer against the three sample exports. Two went through clean. The third stops at row 4,112 on a date written as text.\n\nJune",
            html: None,
            age: 6 * DAY,
            read: false,
        },
    ];
    for m in &project_mail {
        s.push_item(projects, m.item(now));
    }

    for (to, subject, body, age) in [
        (PRIYA, "RE: Launch checklist: three items left", "I will take the pricing page. Sign-off by Thursday 11:00.\n\nAlex", 10 * MINUTE),
        (HELENA, "RE: Notes from the customer call", "Monday 10:00 works. I will bring a first outline.\n\nAlex", 2 * DAY),
    ] {
        let mut it = Mail { from: ME, to: &[], cc: &[], subject, body, html: None, age, read: true }.item(now);
        it.add_recipient(RECIP_TO, to.0, to.1);
        it.props.remove(pid::MESSAGE_DELIVERY_TIME);
        it.props.set(pid::CLIENT_SUBMIT_TIME, PropValue::Time(now.saturating_sub(age)));
        s.push_item(sent, it);
    }

    let mut draft = Item::new_mail();
    draft.set_subject("Offsite agenda");
    draft.set_body_text("Morning: where we are. Afternoon: what we stop doing.\n\n(to finish)");
    draft.set_sender(ME.0, ME.1);
    draft.add_recipient(RECIP_TO, TOMAS.0, TOMAS.1);
    draft.props.set(pid::LAST_MODIFICATION_TIME, PropValue::Time(now.saturating_sub(DAY)));
    draft.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(MSGFLAG_READ | MSGFLAG_UNSENT));
    s.push_item(drafts, draft);

    let old = Mail {
        from: FACILITIES,
        to: &[ME],
        cc: &[],
        subject: "Car park barrier fixed",
        body: "The barrier at the rear car park works again. Thank you for your patience.\n\nFacilities Desk",
        html: None,
        age: 9 * DAY,
        read: true,
    };
    s.push_item(deleted, old.item(now));

    // Mail of the kind that is after a password: every link in it says one place and goes to
    // another, each in its own way. The names are made up and resolve nowhere.
    if let Some(junk) = s.folder_by_kind(FolderKind::Junk) {
        let lure = Mail {
            from: ("Mail Administrator", "no-reply@mailbox-notice.example.net"),
            to: &[ME],
            cc: &[],
            subject: "Your mailbox is almost full",
            body: "Your mailbox has reached 98% of its quota. Confirm your account to keep receiving mail.",
            html: Some(LURE_HTML),
            age: 2 * DAY,
            read: false,
        };
        let mut lure = lure.item(now);
        lure.attachments.push(Attachment::file(
            "Mailbox-quota.pdf.htm",
            "text/html",
            b"<html><body><p>Sign in to keep your mailbox.</p><form><input name=\"user\"><input type=\"password\" name=\"pass\"></form></body></html>".to_vec(),
        ));
        lure.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(0x10));
        s.push_item(junk, lure);
    }
    let calendar = s.push_folder(None, "Calendar", "IPF.Appointment");
    for item in appointments(now) {
        s.push_item(calendar, item);
    }
    s
}

/// The sample calendar: a few days of our own invented plans around `now`, two of them at the
/// same time so they stand side by side, events that last all day, and a stand-up every
/// weekday.
fn appointments(now: i64) -> Vec<Item> {
    use openmailclient_model::calendar::{Appointment, Busy};
    let today = now.div_euclid(DAY).saturating_mul(DAY);
    let at = |day: i64, hour: i64, minute: i64| today.saturating_add(day * DAY + hour * HOUR + minute * MINUTE);
    let plan = |subject: &str, location: &str, start: i64, end: i64, busy: Busy| Appointment {
        subject: subject.to_string(),
        location: location.to_string(),
        start,
        end,
        all_day: false,
        busy,
        reminder: Some(15),
        recurring: false,
        meeting: false,
        recurrence: None,
        occurrence: None,
    };
    // Every weekday from a week ago, at the same time; the sample's times are UTC.
    use openmailclient_model::recur::{self, End, Freq, Recurrence, Rule};
    let week_ago = recur::day_of_minutes(at(-7, 0, 0).div_euclid(MINUTE));
    let pattern = Recurrence::new(Freq::Weekly, Rule::Week { days: 0x3E }, 1, End::Never, week_ago, 9 * 60 + 30, 9 * 60 + 45, 1);
    let first = recur::minutes_of_day(pattern.start_day).saturating_add(9 * 60 + 30).saturating_mul(MINUTE);
    let mut standup = plan("Team stand-up", "Room 2", first, first.saturating_add(15 * MINUTE), Busy::Busy);
    standup.recurring = true;
    standup.recurrence = Some(pattern);
    let mut list = vec![
        standup,
        plan("Review the spring catalogue", "Studio", at(0, 14, 0), at(0, 15, 30), Busy::Busy),
        plan("Call with the print shop", "Phone", at(0, 14, 30), at(0, 15, 0), Busy::Tentative),
        plan("Lunch with Priya", "Harbour Cafe", at(-1, 12, 30), at(-1, 13, 30), Busy::OutOfOffice),
        plan("Dentist", "Bridge Street Clinic", at(1, 8, 0), at(1, 9, 0), Busy::OutOfOffice),
        plan("Budget planning", "Room 4", at(2, 10, 0), at(2, 12, 0), Busy::Busy),
        plan("Working from the north office", "", at(3, 9, 0), at(3, 17, 0), Busy::WorkingElsewhere),
        plan("Volunteer shift at the library", "Riverside Library", at(5, 10, 0), at(5, 13, 0), Busy::Free),
    ];
    let mut fair = plan("Book fair", "Exhibition Hall", at(1, 0, 0), at(4, 0, 0), Busy::Free);
    fair.all_day = true;
    fair.reminder = None;
    let mut birthday = plan("Sam's birthday", "", at(-2, 0, 0), at(-1, 0, 0), Busy::Free);
    birthday.all_day = true;
    birthday.reminder = None;
    list.push(fair);
    list.push(birthday);
    // Each with its id for life, as one made here has.
    list.iter()
        .enumerate()
        .map(|(i, a)| {
            let mut item = a.item();
            openmailclient_model::calendar::set_global_id(&mut item, &openmailclient_model::calendar::new_global_id(now, i as u64));
            item
        })
        .collect()
}

/// The banner proof in the print shop's mail: our own drawing, made here pixel by pixel. A dark
/// field, a lighter slanted band, a pale disc and three bars standing in for lines of type.
fn banner_proof() -> Vec<u8> {
    const W: u32 = 240;
    const H: u32 = 96;
    png(W, H, |x, y| {
        let (dx, dy) = (i64::from(x) - 48, i64::from(y) - 48);
        let bar = |top: u32, right: u32| (top..top + 8).contains(&y) && (96..right).contains(&x);
        if dx * dx + dy * dy <= 26 * 26 {
            [0xF4, 0xD3, 0x5E]
        } else if bar(28, 212) || bar(46, 188) || bar(64, 150) {
            [0xFF, 0xFF, 0xFF]
        } else if (x + y / 2) % 120 < 26 {
            [0x2E, 0x6F, 0xA8]
        } else {
            [0x1F, 0x4E, 0x79]
        }
    })
}

/// A PNG file of `width` × `height` pixels, 8-bit RGB, with the pixel data stored uncompressed
/// (PNG specification, W3C Recommendation, 2nd edition: §5 datastream structure, §11.2.2 IHDR;
/// RFC 1950 for the zlib wrapper and RFC 1951 §3.2.4 for stored blocks).
fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let mut raw = Vec::new();
    for y in 0..height {
        raw.push(0); // filter type: none
        for x in 0..width {
            raw.extend_from_slice(&pixel(x, y));
        }
    }
    let mut zlib = vec![0x78, 0x01];
    let mut blocks = raw.chunks(0xFFFF).peekable();
    while let Some(block) = blocks.next() {
        let len = u16::try_from(block.len()).unwrap_or(u16::MAX);
        zlib.push(u8::from(blocks.peek().is_none()));
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in &raw {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
        let mut crc = !0u32;
        for byte in kind.iter().chain(data) {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&(!crc).to_be_bytes());
    };
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]); // bit depth, colour type RGB, deflate, filter, no interlace
    chunk(b"IHDR", &header);
    chunk(b"IDAT", &zlib);
    chunk(b"IEND", &[]);
    out
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// The map of the sample receipt: a Windows metafile that is a drawing ([MS-WMF] 2.3), written
/// here. A deck with a row of bays, one of them green, a pillar, and an arrow at the way in.
pub(crate) fn bay_map() -> Vec<u8> {
    let words = |list: &[i16]| list.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>();
    let pen = |style: u16, width: i16, [r, g, b]: [u8; 3]| {
        let mut body = style.to_le_bytes().to_vec();
        body.extend(words(&[width, 0]));
        body.extend([r, g, b, 0]);
        (0x02FAu16, body)
    };
    let brush = |[r, g, b]: [u8; 3]| (0x02FCu16, vec![0, 0, r, g, b, 0, 0, 0]);
    let select = |object: i16| (0x012Du16, words(&[object]));
    // What each record does, and its parameters: the last one written first.
    let mut records = vec![
        (0x0103, words(&[8])),
        (0x020B, words(&[0, 0])),
        (0x020C, words(&[120, 240])),
        // 0: the deck's edge; 1: the deck; 2: a bay's lines; 3: an empty bay; 4: bay 14;
        // 5: no line; 6: the arrow and the pillar.
        pen(0, 3, [0x5F, 0x5F, 0x5F]),
        brush([0xEC, 0xEC, 0xE6]),
        pen(0, 1, [0x8A, 0x8A, 0x8A]),
        brush([0xFF, 0xFF, 0xFF]),
        brush([0x2E, 0x7A, 0x4F]),
        pen(5, 0, [0, 0, 0]),
        brush([0x1F, 0x5A, 0xA0]),
        select(0),
        select(1),
        (0x061C, words(&[20, 20, 114, 234, 6, 6])),
        select(2),
    ];
    for bay in 0..6i16 {
        let left = 54 + bay * 28;
        records.push(select(if bay == 2 { 4 } else { 3 }));
        records.push((0x041B, words(&[60, left + 28, 16, left])));
    }
    records.extend([
        select(5),
        select(6),
        // The arrow: a shaft and a head, pointing in from the left.
        (0x0324, words(&[7, 14, 80, 34, 80, 34, 72, 50, 86, 34, 100, 34, 92, 14, 92])),
        (0x0418, words(&[104, 206, 84, 186])),
        select(0),
        // The lane the arrow leads into.
        (0x0214, words(&[72, 58])),
        (0x0213, words(&[72, 176])),
    ]);
    let longest = records.iter().map(|(_, body)| 3 + body.len() / 2).max().unwrap_or(3);
    let total = 9 + records.iter().map(|(_, body)| 3 + body.len() / 2).sum::<usize>() + 3;
    // [MS-WMF] 2.3.2.2, META_HEADER: kept in memory, nine words, version 3, the file's length
    // in words, how many objects, and the longest record.
    let mut file = vec![1, 0, 9, 0, 0, 3];
    file.extend((total as u32).to_le_bytes());
    file.extend(7u16.to_le_bytes());
    file.extend((longest as u32).to_le_bytes());
    file.extend([0, 0]);
    for (function, body) in records.iter().chain(&[(0u16, Vec::new())]) {
        file.extend(((3 + body.len() / 2) as u32).to_le_bytes());
        file.extend(function.to_le_bytes());
        file.extend(body);
    }
    file
}

/// The permit card of the sample receipt: a bitmap without a file's header ([MS-WMF] 2.2.2.9),
/// drawn here. A cream card with a green edge, a blue band, and a "P" in a square.
fn card_bitmap() -> Vec<u8> {
    const W: usize = 192;
    const H: usize = 112;
    let inside = |x: usize, y: usize, x0: usize, y0: usize, x1: usize, y1: usize| (x0..x1).contains(&x) && (y0..y1).contains(&y);
    let mut dib = Vec::with_capacity(40 + W * H * 3);
    for field in [40, W as u32, H as u32, 1 | 24 << 16, 0, (W * H * 3) as u32, 3780, 3780, 0, 0] {
        dib.extend_from_slice(&field.to_le_bytes());
    }
    // Rows run from the bottom up, each pixel blue, green, red.
    for y in (0..H).rev() {
        for x in 0..W {
            let letter = inside(x, y, 34, 30, 42, 82) || inside(x, y, 34, 30, 62, 38) || inside(x, y, 34, 52, 62, 60) || inside(x, y, 56, 30, 64, 60);
            let color: [u8; 3] = if !inside(x, y, 4, 4, W - 4, H - 4) {
                [0x4F, 0x7A, 0x2E]
            } else if inside(x, y, 20, 20, 76, 92) {
                if letter { [0xFF, 0xFF, 0xFF] } else { [0xA0, 0x5A, 0x1F] }
            } else if inside(x, y, 88, 28, W - 16, 40) {
                [0xA0, 0x5A, 0x1F]
            } else if inside(x, y, 88, 52, W - 40, 58) || inside(x, y, 88, 68, W - 64, 74) || inside(x, y, 88, 84, W - 28, 90) {
                [0x9A, 0x9A, 0x8C]
            } else {
                [0xE4, 0xF3, 0xFA]
            };
            dib.extend_from_slice(&color);
        }
    }
    dib
}
