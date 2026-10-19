//! Meetings: an appointment with attendees, who are invited, answer and are told when it
//! changes or is cancelled. What is kept of one is what [MS-OXOCAL] gives it.
//!
//! A meeting is an appointment whose PidLidAppointmentStateFlags has asfMeeting
//! ([MS-OXOCAL] 2.2.1.10); asfReceived marks one that came from someone else, asfCanceled one
//! called off. Its attendees are its recipients ([MS-OXOCAL] 2.2.4.10): those who must come
//! as primary recipients, those who may as carbon copies and rooms or things as blind ones,
//! the organizer among them marked by recipOrganizer in PidTagRecipientFlags, and each
//! attendee's answer in PidTagRecipientTrackStatus. One's own answer is PidLidResponseStatus
//! (2.2.1.11) and how many times it was changed and sent again PidLidAppointmentSequence
//! (2.2.1.1).

use crate::calendar::{PSETID_APPOINTMENT, bool_of, i32_of, named, remove_named, set_named, time_of};
use crate::props::{PropBag, PropValue, pid};
use crate::{Item, recipient_email, recipient_label};

/// Property ids of the appointment property set used by meetings ([MS-OXOCAL] 2.2.1).
mod lid {
    pub const SEQUENCE: u32 = 0x8201;
    pub const STATE_FLAGS: u32 = 0x8217;
    pub const RESPONSE_STATUS: u32 = 0x8218;
    pub const REPLY_TIME: u32 = 0x8220;
    pub const FINVITED: u32 = 0x8229;
    pub const REPLY_NAME: u32 = 0x8230;
    pub const TO_ATTENDEES: u32 = 0x823B;
    pub const CC_ATTENDEES: u32 = 0x823C;
}

/// PidLidAppointmentStateFlags ([MS-OXOCAL] 2.2.1.10).
pub const AS_MEETING: i32 = 0x1;
pub const AS_RECEIVED: i32 = 0x2;
pub const AS_CANCELED: i32 = 0x4;

/// PidTagRecipientFlags, PidTagRecipientTrackStatus and PidTagRecipientTrackStatusTime
/// ([MS-OXOCAL] 2.2.4.10.1 to 2.2.4.10.3).
const RECIPIENT_FLAGS: u16 = 0x5FFD;
const TRACK_STATUS: u16 = 0x5FFF;
const TRACK_STATUS_TIME: u16 = 0x5FFB;
/// recipSendable and recipOrganizer.
const RECIP_SENDABLE: i32 = 0x1;
const RECIP_ORGANIZER: i32 = 0x2;

/// The most attendees one meeting keeps; more are read from no file or command.
pub const MAX_ATTENDEES: usize = 500;
/// The longest name or address kept of one person.
const MAX_NAME: usize = 320;

/// Message classes of the messages a meeting is sent and answered with ([MS-OXOCAL] 2.2.6).
pub mod class {
    pub const REQUEST: &str = "IPM.Schedule.Meeting.Request";
    pub const CANCELED: &str = "IPM.Schedule.Meeting.Canceled";
    pub const ACCEPTED: &str = "IPM.Schedule.Meeting.Resp.Pos";
    pub const TENTATIVE: &str = "IPM.Schedule.Meeting.Resp.Tent";
    pub const DECLINED: &str = "IPM.Schedule.Meeting.Resp.Neg";
}

/// A meeting's request, answer or cancellation marked as about the meeting `global_id` names,
/// as Outlook marks one ([MS-OXOCAL] 2.2.6.1): its GlobalObjectId, that it is a meeting (and one
/// received), its sequence and the answer it stands at. What else it holds is left.
pub fn dress(item: &mut Item, global_id: &[u8], sequence: i32, received: bool, response: Response) {
    let a = PSETID_APPOINTMENT;
    crate::calendar::set_global_id(item, global_id);
    set_named(item, a, lid::STATE_FLAGS, PropValue::Int32(AS_MEETING | if received { AS_RECEIVED } else { 0 }));
    set_named(item, a, lid::SEQUENCE, PropValue::Int32(sequence.max(0)));
    set_named(item, a, lid::RESPONSE_STATUS, PropValue::Int32(response.code()));
}

/// Whether an item is a meeting's request, answer or cancellation, by its class.
pub fn is_meeting_message(item: &Item) -> bool {
    item.message_class().to_ascii_lowercase().starts_with("ipm.schedule.meeting.")
}

/// How an attendee is asked to a meeting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Role {
    #[default]
    Required,
    Optional,
    /// A room or a thing, booked rather than asked.
    Resource,
}

impl Role {
    /// The recipient type it is kept as ([MS-OXOCAL] 2.2.4.10).
    pub fn recipient_type(self) -> i32 {
        match self {
            Self::Required => crate::RECIP_TO,
            Self::Optional => crate::RECIP_CC,
            Self::Resource => crate::RECIP_BCC,
        }
    }

    pub fn of_type(kind: i32) -> Option<Self> {
        match kind {
            crate::RECIP_TO => Some(Self::Required),
            crate::RECIP_CC => Some(Self::Optional),
            crate::RECIP_BCC => Some(Self::Resource),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
            Self::Resource => "resource",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Required, Self::Optional, Self::Resource].into_iter().find(|r| r.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// An answer to a meeting ([MS-OXOCAL] 2.2.1.11 and 2.2.4.10.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Response {
    /// Not asked, or not said.
    #[default]
    None,
    /// The one who called it.
    Organized,
    Tentative,
    Accepted,
    Declined,
    /// Asked, and no answer yet.
    NotResponded,
}

impl Response {
    pub fn code(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Organized => 1,
            Self::Tentative => 2,
            Self::Accepted => 3,
            Self::Declined => 4,
            Self::NotResponded => 5,
        }
    }

    pub fn of(code: i32) -> Self {
        match code {
            1 => Self::Organized,
            2 => Self::Tentative,
            3 => Self::Accepted,
            4 => Self::Declined,
            5 => Self::NotResponded,
            _ => Self::None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Organized => "organizer",
            Self::Tentative => "tentative",
            Self::Accepted => "accepted",
            Self::Declined => "declined",
            Self::NotResponded => "notResponded",
        }
    }

    /// As a person reads it in a list of attendees.
    pub fn words(self) -> &'static str {
        match self {
            Self::None | Self::NotResponded => "No response",
            Self::Organized => "Meeting organizer",
            Self::Tentative => "Tentative",
            Self::Accepted => "Accepted",
            Self::Declined => "Declined",
        }
    }

    /// The answer a word asks for: accept, tentative or decline.
    pub fn answer(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "accept" | "accepted" => Some(Self::Accepted),
            "tentative" | "tentatively" => Some(Self::Tentative),
            "decline" | "declined" => Some(Self::Declined),
            _ => None,
        }
    }

    /// Whether it is an answer given: accepted, tentative or declined.
    pub fn answered(self) -> bool {
        matches!(self, Self::Tentative | Self::Accepted | Self::Declined)
    }

    /// The class of the message that gives this answer.
    pub fn class(self) -> &'static str {
        match self {
            Self::Declined => class::DECLINED,
            Self::Tentative => class::TENTATIVE,
            _ => class::ACCEPTED,
        }
    }

    /// The word put before the subject of a message that gives this answer.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Declined => "Declined",
            Self::Tentative => "Tentative",
            _ => "Accepted",
        }
    }
}

/// A person a meeting is about: its organizer or one of those asked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attendee {
    pub name: String,
    pub address: String,
    pub role: Role,
    pub response: Response,
    /// When the answer came, UTC FILETIME ticks; 0 when none did.
    pub replied: i64,
}

impl Attendee {
    pub fn new(name: &str, address: &str, role: Role) -> Self {
        Self { name: clean(name), address: clean(address), role, response: Response::None, replied: 0 }
    }

    /// The name shown: the name, else the address.
    pub fn label(&self) -> &str {
        if self.name.is_empty() { &self.address } else { &self.name }
    }

    /// Whether it is the one at `address`, by address alone and whatever its case.
    pub fn is(&self, address: &str) -> bool {
        let a = address.trim();
        !a.is_empty() && self.address.trim().eq_ignore_ascii_case(a)
    }
}

/// A name or address as kept: one line, no control characters, not too long.
pub fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect::<String>().trim().to_string()
}

/// What makes an appointment a meeting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meeting {
    pub organizer: Attendee,
    /// Those asked, the organizer not among them.
    pub attendees: Vec<Attendee>,
    /// One's own answer: Organized for the organizer.
    pub response: Response,
    /// How many times it was changed and sent again (RFC 5545 SEQUENCE).
    pub sequence: i32,
    /// It came from someone else: one is an attendee, not the organizer.
    pub received: bool,
    pub cancelled: bool,
    /// Its invitations were sent.
    pub invited: bool,
    /// When one's own answer was sent, UTC FILETIME ticks; 0 when it was not.
    pub replied: i64,
}

impl Meeting {
    /// What `item` says of itself as a meeting; `None` when it is not one.
    pub fn of(item: &Item) -> Option<Self> {
        let a = PSETID_APPOINTMENT;
        let flags = i32_of(named(item, a, lid::STATE_FLAGS)).unwrap_or(0);
        if flags & AS_MEETING == 0 {
            return None;
        }
        let mut organizer = None;
        let mut attendees = Vec::new();
        for r in &item.recipients {
            let role = Role::of_type(r.i32(pid::RECIPIENT_TYPE).unwrap_or(crate::RECIP_TO));
            let address = clean(&recipient_email(r));
            let name = clean(&recipient_label(r));
            let person = Attendee {
                // A name that is only the address again is no name.
                name: if name.eq_ignore_ascii_case(&address) { String::new() } else { name },
                address,
                role: role.unwrap_or_default(),
                response: Response::of(r.i32(TRACK_STATUS).unwrap_or(0)),
                replied: r.time(TRACK_STATUS_TIME).unwrap_or(0),
            };
            if r.i32(RECIPIENT_FLAGS).unwrap_or(0) & RECIP_ORGANIZER != 0 {
                organizer.get_or_insert(person);
            } else if role.is_some() && attendees.len() < MAX_ATTENDEES {
                attendees.push(person);
            }
        }
        // One that does not list its organizer among its recipients names it as its sender.
        let organizer = organizer.unwrap_or_else(|| Attendee {
            name: clean(&item.sender_name()),
            address: clean(&item.sender_email()),
            role: Role::Required,
            response: Response::Organized,
            replied: 0,
        });
        // The organizer's own row is not an attendee too.
        attendees.retain(|p| !(p.is(&organizer.address) && !organizer.address.is_empty()));
        let received = flags & AS_RECEIVED != 0;
        let response = Response::of(i32_of(named(item, a, lid::RESPONSE_STATUS)).unwrap_or(if received { 5 } else { 1 }));
        Some(Self {
            organizer,
            attendees,
            response,
            sequence: i32_of(named(item, a, lid::SEQUENCE)).unwrap_or(0).max(0),
            received,
            cancelled: flags & AS_CANCELED != 0,
            invited: bool_of(named(item, a, lid::FINVITED)).unwrap_or(false),
            replied: time_of(named(item, a, lid::REPLY_TIME)).unwrap_or(0),
        })
    }

    /// `item` made to say this meeting: its state, its answer and its recipients. A recipient
    /// kept before keeps what else it carried (properties this program does not know).
    pub fn write(&self, item: &mut Item) {
        let a = PSETID_APPOINTMENT;
        let mut flags = AS_MEETING;
        if self.received {
            flags |= AS_RECEIVED;
        }
        if self.cancelled {
            flags |= AS_CANCELED;
        }
        set_named(item, a, lid::STATE_FLAGS, PropValue::Int32(flags));
        set_named(item, a, lid::RESPONSE_STATUS, PropValue::Int32(self.response.code()));
        set_named(item, a, lid::SEQUENCE, PropValue::Int32(self.sequence.max(0)));
        set_named(item, a, lid::FINVITED, PropValue::Bool(self.invited));
        if self.replied > 0 {
            set_named(item, a, lid::REPLY_TIME, PropValue::Time(self.replied));
            set_named(item, a, lid::REPLY_NAME, PropValue::String(self.organizer.label().to_string()));
        } else {
            remove_named(item, a, lid::REPLY_TIME);
        }
        let mut was = std::mem::take(&mut item.recipients);
        let mut row = |p: &Attendee, flags: i32| -> PropBag {
            let mut r = match was.iter().position(|r| p.is(&recipient_email(r))) {
                Some(i) => was.remove(i),
                None => PropBag::default(),
            };
            r.set(pid::RECIPIENT_TYPE, PropValue::Int32(p.role.recipient_type()));
            r.set_str(pid::DISPLAY_NAME, p.label());
            r.set_str(pid::EMAIL_ADDRESS, &p.address);
            r.set_str(pid::SMTP_ADDRESS, &p.address);
            r.set_str(pid::ADDRESS_TYPE, "SMTP");
            r.set(RECIPIENT_FLAGS, PropValue::Int32(flags));
            r.set(TRACK_STATUS, PropValue::Int32(p.response.code()));
            if p.replied > 0 {
                r.set(TRACK_STATUS_TIME, PropValue::Time(p.replied));
            }
            r
        };
        let mut rows = Vec::with_capacity(self.attendees.len().saturating_add(1));
        if !self.organizer.address.is_empty() {
            let mut o = self.organizer.clone();
            o.role = Role::Required;
            o.response = Response::Organized;
            rows.push(row(&o, RECIP_SENDABLE | RECIP_ORGANIZER));
        }
        for p in self.attendees.iter().take(MAX_ATTENDEES) {
            rows.push(row(p, RECIP_SENDABLE));
        }
        item.recipients = rows;
        item.refresh_display_recipients();
        let names = |role: Role| self.attendees.iter().filter(|p| p.role == role).map(Attendee::label).collect::<Vec<_>>().join("; ");
        set_named(item, a, lid::TO_ATTENDEES, PropValue::String(names(Role::Required)));
        set_named(item, a, lid::CC_ATTENDEES, PropValue::String(names(Role::Optional)));
    }

    /// The attendee at `address`.
    pub fn attendee(&self, address: &str) -> Option<&Attendee> {
        self.attendees.iter().find(|p| p.is(address))
    }

    pub fn attendee_mut(&mut self, address: &str) -> Option<&mut Attendee> {
        self.attendees.iter_mut().find(|p| p.is(address))
    }

    /// How many attendees gave each answer: accepted, tentative, declined, none.
    pub fn tally(&self) -> [usize; 4] {
        let mut t = [0usize; 4];
        for p in &self.attendees {
            let i = match p.response {
                Response::Accepted => 0,
                Response::Tentative => 1,
                Response::Declined => 2,
                _ => 3,
            };
            if let Some(n) = t.get_mut(i) {
                *n += 1;
            }
        }
        t
    }
}

/// A meeting's state taken off an item, as it is when it is no meeting any more.
pub fn unmake(item: &mut Item) {
    let a = PSETID_APPOINTMENT;
    set_named(item, a, lid::STATE_FLAGS, PropValue::Int32(0));
    for id in [lid::RESPONSE_STATUS, lid::FINVITED, lid::REPLY_TIME, lid::REPLY_NAME, lid::TO_ATTENDEES, lid::CC_ATTENDEES] {
        remove_named(item, a, id);
    }
    item.recipients.clear();
    item.refresh_display_recipients();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{Appointment, Busy};

    fn meeting() -> Meeting {
        Meeting {
            organizer: Attendee::new("Alex Morgan", "alex.morgan@example.org", Role::Required),
            attendees: vec![
                Attendee::new("Sam Lee", "sam@example.com", Role::Required),
                Attendee::new("", "priya@example.net", Role::Optional),
                Attendee::new("Room 4", "room4@example.org", Role::Resource),
            ],
            response: Response::Organized,
            sequence: 2,
            invited: true,
            ..Meeting::default()
        }
    }

    #[test]
    fn a_meeting_is_kept_in_its_items_recipients_and_read_back() {
        let a = Appointment { subject: "Planning".into(), start: 1, end: 2, busy: Busy::Busy, ..Appointment::default() };
        let mut item = a.item();
        let mut m = meeting();
        m.organizer.response = Response::Organized;
        m.attendees[0].response = Response::Accepted;
        m.attendees[0].replied = 99;
        m.write(&mut item);
        assert_eq!(item.recipients.len(), 4);
        let back = Meeting::of(&item).unwrap();
        assert_eq!(back, m);
        assert_eq!(item.display_recipients(crate::RECIP_TO), "Alex Morgan; Sam Lee");
        assert_eq!(back.tally(), [1, 0, 0, 2]);
        // Still an appointment, and a meeting as Appointment reads it.
        assert!(Appointment::of(&item).unwrap().meeting);
        // A recipient's own properties are kept when it is written again.
        item.recipients[1].set(0x3A00, PropValue::String("kept".into()));
        m.attendees[0].response = Response::Declined;
        m.write(&mut item);
        let sam = item.recipients.iter().find(|r| recipient_email(r) == "sam@example.com").unwrap();
        assert_eq!(sam.str(0x3A00), Some("kept"));
        assert_eq!(Meeting::of(&item).unwrap().attendees[0].response, Response::Declined);
        unmake(&mut item);
        assert!(Meeting::of(&item).is_none());
        assert!(item.recipients.is_empty());
    }

    #[test]
    fn a_received_meeting_names_its_organizer_by_its_sender_when_its_recipients_do_not() {
        let mut item = Appointment { start: 1, end: 2, ..Appointment::default() }.item();
        set_named(&mut item, PSETID_APPOINTMENT, lid::STATE_FLAGS, PropValue::Int32(AS_MEETING | AS_RECEIVED));
        item.set_sender("Sam Lee", "sam@example.com");
        item.add_recipient(crate::RECIP_TO, "Me", "me@example.org");
        item.add_recipient(crate::RECIP_TO, "Sam Lee", "SAM@example.com");
        let m = Meeting::of(&item).unwrap();
        assert!(m.received);
        assert_eq!(m.organizer.address, "sam@example.com");
        assert_eq!(m.attendees.len(), 1, "the organizer is not an attendee too");
        assert_eq!(m.response, Response::NotResponded);
    }

    #[test]
    fn names_from_a_stranger_are_one_line_and_capped() {
        let p = Attendee::new("Line\r\nTwo\u{7}", &"a".repeat(5_000), Role::Required);
        assert_eq!(p.name, "LineTwo");
        assert_eq!(p.address.chars().count(), MAX_NAME);
        assert_eq!(Response::answer(" Accept "), Some(Response::Accepted));
        assert_eq!(Response::answer("maybe"), None);
        assert_eq!(Role::parse("RESOURCE"), Some(Role::Resource));
    }
}
