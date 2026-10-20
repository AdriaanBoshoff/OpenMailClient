//! Contacts and contact groups: what an item of a Contacts folder says of a person or of a list
//! of people, read from and written to the properties [MS-OXOCNTC] gives them.
//!
//! A contact's names, company, telephone numbers and home and other addresses are tagged
//! properties ([MS-OXOCNTC] 2.2.1.1 to 2.2.1.4); its e-mail addresses, how it is filed, its work
//! address and its web page are named properties of the address property set (2.2.1.2,
//! 2.2.1.3). A contact group is an item of its own class whose members are kept as one-off
//! entry ids, a name and an address each ([MS-OXOCNTC] 2.2.2, [MS-OXCDATA] 2.2.5.1).

use crate::calendar::{bool_of, i32_of, named, remove_named, set_named, time_of};
use crate::props::{NameId, NamedKey, PropValue, pid, pset};
use crate::zone::Offsets;
use crate::{Attachment, Item, time};

/// PSETID_Address {00062004-0000-0000-C000-000000000046} ([MS-OXPROPS] 1.3.2).
pub const PSETID_ADDRESS: [u8; 16] = [0x04, 0x20, 0x06, 0x00, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];

/// The message class of a contact ([MS-OXOCNTC] 2.2.1).
pub const CLASS: &str = "IPM.Contact";
/// The message class of a contact group ([MS-OXOCNTC] 2.2.2).
pub const GROUP_CLASS: &str = "IPM.DistList";

/// E-mail addresses a contact keeps, as Outlook keeps three.
pub const MAX_EMAILS: usize = 3;
/// Members a group keeps at most; more are left out when it is read.
pub const MAX_MEMBERS: usize = 10_000;
/// Longest value of any one field kept, in characters.
pub const MAX_FIELD: usize = 4_000;
/// Longest notes kept, in characters.
pub const MAX_NOTES: usize = 200_000;
/// Largest picture kept, in bytes.
pub const MAX_PICTURE: usize = 4 << 20;

/// Tagged properties of a contact ([MS-OXOCNTC] 2.2.1, [MS-OXPROPS]).
mod tag {
    pub const DISPLAY_NAME: u16 = 0x3001;
    pub const GENERATION: u16 = 0x3A05;
    pub const GIVEN_NAME: u16 = 0x3A06;
    pub const BUSINESS_PHONE: u16 = 0x3A08;
    pub const HOME_PHONE: u16 = 0x3A09;
    pub const INITIALS: u16 = 0x3A0A;
    pub const SURNAME: u16 = 0x3A11;
    pub const COMPANY: u16 = 0x3A16;
    pub const TITLE: u16 = 0x3A17;
    pub const DEPARTMENT: u16 = 0x3A18;
    pub const OFFICE: u16 = 0x3A19;
    pub const PRIMARY_PHONE: u16 = 0x3A1A;
    pub const BUSINESS2_PHONE: u16 = 0x3A1B;
    pub const MOBILE_PHONE: u16 = 0x3A1C;
    pub const CAR_PHONE: u16 = 0x3A1E;
    pub const OTHER_PHONE: u16 = 0x3A1F;
    pub const PAGER: u16 = 0x3A21;
    pub const BUSINESS_FAX: u16 = 0x3A24;
    pub const HOME_FAX: u16 = 0x3A25;
    pub const MAIL_COUNTRY: u16 = 0x3A26;
    pub const MAIL_CITY: u16 = 0x3A27;
    pub const MAIL_STATE: u16 = 0x3A28;
    pub const MAIL_STREET: u16 = 0x3A29;
    pub const MAIL_POSTAL: u16 = 0x3A2A;
    pub const MAIL_PO_BOX: u16 = 0x3A2B;
    pub const ASSISTANT_PHONE: u16 = 0x3A2E;
    pub const HOME2_PHONE: u16 = 0x3A2F;
    pub const ASSISTANT: u16 = 0x3A30;
    pub const ANNIVERSARY: u16 = 0x3A41;
    pub const BIRTHDAY: u16 = 0x3A42;
    pub const MIDDLE_NAME: u16 = 0x3A44;
    pub const PREFIX: u16 = 0x3A45;
    pub const PROFESSION: u16 = 0x3A46;
    pub const SPOUSE: u16 = 0x3A48;
    pub const MANAGER: u16 = 0x3A4E;
    pub const NICKNAME: u16 = 0x3A4F;
    pub const HOME_PAGE: u16 = 0x3A50;
    pub const BUSINESS_HOME_PAGE: u16 = 0x3A51;
    pub const HOME_CITY: u16 = 0x3A59;
    pub const HOME_COUNTRY: u16 = 0x3A5A;
    pub const HOME_POSTAL: u16 = 0x3A5B;
    pub const HOME_STATE: u16 = 0x3A5C;
    pub const HOME_STREET: u16 = 0x3A5D;
    pub const HOME_PO_BOX: u16 = 0x3A5E;
    pub const OTHER_CITY: u16 = 0x3A5F;
    pub const OTHER_COUNTRY: u16 = 0x3A60;
    pub const OTHER_POSTAL: u16 = 0x3A61;
    pub const OTHER_STATE: u16 = 0x3A62;
    pub const OTHER_STREET: u16 = 0x3A63;
    pub const OTHER_PO_BOX: u16 = 0x3A64;
    /// PidTagAttachmentContactPhoto: the attachment is the contact's picture ([MS-OXOCNTC] 2.2.1.8).
    pub const CONTACT_PHOTO: u16 = 0x7FFF;
}

/// Named properties of the address property set ([MS-OXOCNTC] 2.2.1, 2.2.2).
mod lid {
    pub const FILE_UNDER: u32 = 0x8005;
    pub const FILE_UNDER_ID: u32 = 0x8006;
    pub const HAS_PICTURE: u32 = 0x8015;
    pub const HOME_ADDRESS: u32 = 0x801A;
    pub const WORK_ADDRESS: u32 = 0x801B;
    pub const OTHER_ADDRESS: u32 = 0x801C;
    pub const POSTAL_ADDRESS_ID: u32 = 0x8022;
    pub const EMAIL_LIST: u32 = 0x8028;
    pub const ARRAY_TYPE: u32 = 0x8029;
    pub const HTML: u32 = 0x802B;
    pub const WORK_STREET: u32 = 0x8045;
    pub const WORK_CITY: u32 = 0x8046;
    pub const WORK_STATE: u32 = 0x8047;
    pub const WORK_POSTAL: u32 = 0x8048;
    pub const WORK_COUNTRY: u32 = 0x8049;
    pub const WORK_PO_BOX: u32 = 0x804A;
    pub const CHECKSUM: u32 = 0x804C;
    pub const LIST_NAME: u32 = 0x8053;
    pub const ONE_OFF_MEMBERS: u32 = 0x8054;
    pub const MEMBERS: u32 = 0x8055;
    pub const IM_ADDRESS: u32 = 0x8062;
    pub const BIRTHDAY_LOCAL: u32 = 0x80DE;
    pub const ANNIVERSARY_LOCAL: u32 = 0x80DF;
    /// The first property of each e-mail address: Email1 at 0x8080, Email2 at 0x8090, Email3 at
    /// 0x80A0; the display name, then the address type at +2, the address at +3, the original
    /// display name at +4 and the original entry id at +5 ([MS-OXOCNTC] 2.2.1.2).
    pub const EMAIL_BASE: [u32; 3] = [0x8080, 0x8090, 0x80A0];
}

/// The vCard UID a contact or group was read with or written as, kept so that a card read
/// again is known as the same one (RFC 6350 6.7.6). Outlook has no property of its own for it;
/// a named one of the public strings is kept by it as it is.
fn uid_key() -> NamedKey {
    NamedKey { guid: pset::PS_PUBLIC_STRINGS, name: NameId::Name("vCardUID".to_string()) }
}

/// The vCard UID kept on an item, when it has one.
pub fn uid_of(item: &Item) -> Option<String> {
    let want = uid_key();
    match item.named.iter().find(|(k, _)| *k == want).map(|(_, v)| v) {
        Some(PropValue::String(s)) if !s.trim().is_empty() => Some(cut(s, 255)),
        _ => None,
    }
}

/// Keeps `uid` on an item as its vCard UID, or takes it away when it is empty.
pub fn set_uid(item: &mut Item, uid: &str) {
    let want = uid_key();
    item.named.retain(|(k, _)| *k != want);
    let uid = cut(uid, 255);
    if !uid.is_empty() {
        item.named.push((want, PropValue::String(uid)));
    }
}

/// PidLidFileUnderId: filed as "Surname, Given Middle" ([MS-OXOCNTC] 2.2.1.1.12).
const FILED_BY_NAME: i32 = 0x8017;
/// PidLidFileUnderId: filed by company.
const FILED_BY_COMPANY: i32 = 0x3A16;

/// The telephone numbers a contact can have, as Outlook offers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phone {
    Business,
    Business2,
    BusinessFax,
    Home,
    Home2,
    HomeFax,
    Mobile,
    Pager,
    Car,
    Assistant,
    Primary,
    Other,
}

impl Phone {
    pub const ALL: [Self; 12] = [
        Self::Business,
        Self::Business2,
        Self::BusinessFax,
        Self::Home,
        Self::Home2,
        Self::HomeFax,
        Self::Mobile,
        Self::Pager,
        Self::Car,
        Self::Assistant,
        Self::Primary,
        Self::Other,
    ];

    fn tag(self) -> u16 {
        match self {
            Self::Business => tag::BUSINESS_PHONE,
            Self::Business2 => tag::BUSINESS2_PHONE,
            Self::BusinessFax => tag::BUSINESS_FAX,
            Self::Home => tag::HOME_PHONE,
            Self::Home2 => tag::HOME2_PHONE,
            Self::HomeFax => tag::HOME_FAX,
            Self::Mobile => tag::MOBILE_PHONE,
            Self::Pager => tag::PAGER,
            Self::Car => tag::CAR_PHONE,
            Self::Assistant => tag::ASSISTANT_PHONE,
            Self::Primary => tag::PRIMARY_PHONE,
            Self::Other => tag::OTHER_PHONE,
        }
    }

    /// The name commands give it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Business => "business",
            Self::Business2 => "business2",
            Self::BusinessFax => "businessFax",
            Self::Home => "home",
            Self::Home2 => "home2",
            Self::HomeFax => "homeFax",
            Self::Mobile => "mobile",
            Self::Pager => "pager",
            Self::Car => "car",
            Self::Assistant => "assistant",
            Self::Primary => "primary",
            Self::Other => "other",
        }
    }

    /// The name the window shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::Business => "Business",
            Self::Business2 => "Business 2",
            Self::BusinessFax => "Business Fax",
            Self::Home => "Home",
            Self::Home2 => "Home 2",
            Self::HomeFax => "Home Fax",
            Self::Mobile => "Mobile",
            Self::Pager => "Pager",
            Self::Car => "Car",
            Self::Assistant => "Assistant",
            Self::Primary => "Primary",
            Self::Other => "Other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(s))
    }
}

/// Where an address is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Place {
    Business,
    Home,
    Other,
}

impl Place {
    pub const ALL: [Self; 3] = [Self::Business, Self::Home, Self::Other];

    pub fn name(self) -> &'static str {
        match self {
            Self::Business => "business",
            Self::Home => "home",
            Self::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Business => "Business",
            Self::Home => "Home",
            Self::Other => "Other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(s))
    }

    /// PidLidPostalAddressId: which address mail goes to ([MS-OXOCNTC] 2.2.1.3.9).
    fn postal_id(self) -> i32 {
        match self {
            Self::Home => 1,
            Self::Business => 2,
            Self::Other => 3,
        }
    }
}

/// A postal address.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Address {
    pub street: String,
    pub city: String,
    pub state: String,
    pub postal_code: String,
    pub country: String,
    pub po_box: String,
}

impl Address {
    pub fn is_empty(&self) -> bool {
        [&self.street, &self.city, &self.state, &self.postal_code, &self.country, &self.po_box].iter().all(|s| s.trim().is_empty())
    }

    /// The address as it is written on an envelope: the street, then the city, state and code
    /// on one line, then the country.
    pub fn lines(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        for l in self.street.lines() {
            if !l.trim().is_empty() {
                lines.push(l.trim().to_string());
            }
        }
        if !self.po_box.trim().is_empty() {
            lines.push(self.po_box.trim().to_string());
        }
        let place: Vec<&str> = [self.city.trim(), self.state.trim(), self.postal_code.trim()].into_iter().filter(|s| !s.is_empty()).collect();
        if !place.is_empty() {
            lines.push(place.join(" "));
        }
        if !self.country.trim().is_empty() {
            lines.push(self.country.trim().to_string());
        }
        lines.join("\n")
    }
}

/// One of a contact's e-mail addresses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Email {
    pub address: String,
    /// What a message to it shows as the recipient's name; empty for the contact's own.
    pub name: String,
}

/// What a contact says of a person.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contact {
    pub prefix: String,
    pub given: String,
    pub middle: String,
    pub surname: String,
    pub suffix: String,
    /// The name it is shown by; made from the parts when it is empty.
    pub full_name: String,
    /// The name it is filed and sorted by; "Surname, Given" when it is empty.
    pub file_as: String,
    pub nickname: String,
    pub company: String,
    pub department: String,
    pub job_title: String,
    pub office: String,
    pub profession: String,
    pub manager: String,
    pub assistant: String,
    pub spouse: String,
    pub emails: Vec<Email>,
    /// Each kind once, in the order of [`Phone::ALL`].
    pub phones: Vec<(Phone, String)>,
    /// Each place once, in the order of [`Place::ALL`].
    pub addresses: Vec<(Place, Address)>,
    /// The address mail goes to, when one is chosen.
    pub mailing: Option<Place>,
    pub web_page: String,
    pub home_page: String,
    pub im: String,
    /// Day numbers (days since 1970-01-01) of the birthday and anniversary.
    pub birthday: Option<i64>,
    pub anniversary: Option<i64>,
    pub notes: String,
    pub categories: Vec<String>,
    /// The picture, as a file's bytes (JPEG, PNG…), when it has one.
    pub picture: Option<Vec<u8>>,
    /// The vCard UID it is known by; empty when it has none.
    pub uid: String,
}

fn cut(s: &str, most: usize) -> String {
    let s = s.trim_matches(|c: char| c.is_whitespace() && c != '\n');
    if s.chars().count() <= most { s.to_string() } else { s.chars().take(most).collect() }
}

fn text(item: &Item, id: u16) -> String {
    cut(item.props.str(id).unwrap_or(""), MAX_FIELD)
}

fn named_text(item: &Item, id: u32) -> String {
    match named(item, PSETID_ADDRESS, id) {
        Some(PropValue::String(s)) => cut(s, MAX_FIELD),
        _ => String::new(),
    }
}

/// A string property set, or taken away when it is empty, so that what is cleared here is gone.
fn put(item: &mut Item, id: u16, value: &str) {
    if value.is_empty() {
        item.props.remove(id);
    } else {
        item.props.set_str(id, value);
    }
}

fn put_named(item: &mut Item, id: u32, value: &str) {
    if value.is_empty() {
        remove_named(item, PSETID_ADDRESS, id);
    } else {
        set_named(item, PSETID_ADDRESS, id, PropValue::String(value.to_string()));
    }
}

/// A day number at UTC midnight, as PidLidBirthdayLocal keeps it.
fn utc_midnight(day: i64) -> i64 {
    time::from_unix(day.saturating_mul(86_400))
}

/// The day a UTC-midnight time is on.
fn day_at_midnight(t: i64) -> i64 {
    time::to_unix(t).div_euclid(86_400)
}

/// Whether an item is a contact, by its class.
pub fn is_contact(item: &Item) -> bool {
    let class = item.message_class().to_ascii_lowercase();
    class == "ipm.contact" || class.starts_with("ipm.contact.")
}

/// Whether an item is a contact group, by its class.
pub fn is_group(item: &Item) -> bool {
    let class = item.message_class().to_ascii_lowercase();
    class == "ipm.distlist" || class.starts_with("ipm.distlist.")
}

const PLACE_TAGS: [(Place, [u16; 6]); 2] = [
    (Place::Home, [tag::HOME_STREET, tag::HOME_CITY, tag::HOME_STATE, tag::HOME_POSTAL, tag::HOME_COUNTRY, tag::HOME_PO_BOX]),
    (Place::Other, [tag::OTHER_STREET, tag::OTHER_CITY, tag::OTHER_STATE, tag::OTHER_POSTAL, tag::OTHER_COUNTRY, tag::OTHER_PO_BOX]),
];
const WORK_LIDS: [u32; 6] = [lid::WORK_STREET, lid::WORK_CITY, lid::WORK_STATE, lid::WORK_POSTAL, lid::WORK_COUNTRY, lid::WORK_PO_BOX];
const MAIL_TAGS: [u16; 6] = [tag::MAIL_STREET, tag::MAIL_CITY, tag::MAIL_STATE, tag::MAIL_POSTAL, tag::MAIL_COUNTRY, tag::MAIL_PO_BOX];

fn address_of(f: impl Fn(usize) -> String) -> Address {
    Address { street: f(0), city: f(1), state: f(2), postal_code: f(3), country: f(4), po_box: f(5) }
}

fn address_parts(a: &Address) -> [&str; 6] {
    [&a.street, &a.city, &a.state, &a.postal_code, &a.country, &a.po_box]
}

impl Contact {
    /// What `item` says of itself as a contact. `here` is where a birthday kept only as a time
    /// is read (Outlook keeps it as the local midnight, [MS-OXOCNTC] 2.2.1.5.1).
    pub fn of(item: &Item, here: impl Offsets + Copy) -> Self {
        let mut c = Self {
            prefix: text(item, tag::PREFIX),
            given: text(item, tag::GIVEN_NAME),
            middle: text(item, tag::MIDDLE_NAME),
            surname: text(item, tag::SURNAME),
            suffix: text(item, tag::GENERATION),
            full_name: text(item, tag::DISPLAY_NAME),
            file_as: named_text(item, lid::FILE_UNDER),
            nickname: text(item, tag::NICKNAME),
            company: text(item, tag::COMPANY),
            department: text(item, tag::DEPARTMENT),
            job_title: text(item, tag::TITLE),
            office: text(item, tag::OFFICE),
            profession: text(item, tag::PROFESSION),
            manager: text(item, tag::MANAGER),
            assistant: text(item, tag::ASSISTANT),
            spouse: text(item, tag::SPOUSE),
            web_page: named_text(item, lid::HTML),
            home_page: text(item, tag::HOME_PAGE),
            im: named_text(item, lid::IM_ADDRESS),
            notes: cut(&item.body_text(), MAX_NOTES),
            categories: item.categories.clone(),
            uid: uid_of(item).unwrap_or_default(),
            ..Self::default()
        };
        if c.web_page.is_empty() {
            c.web_page = text(item, tag::BUSINESS_HOME_PAGE);
        }
        if c.full_name.is_empty() {
            c.full_name = cut(&item.subject(), MAX_FIELD);
        }
        for base in lid::EMAIL_BASE {
            let address = named_text(item, base + 3);
            if !address.is_empty() {
                let name = named_text(item, base);
                // Outlook shows "Name (address)" as an address's name; that is no name of its own.
                let own = name.is_empty() || name == address || name.ends_with(&format!("({address})"));
                c.emails.push(Email { address, name: if own { String::new() } else { name } });
            }
        }
        for phone in Phone::ALL {
            let number = text(item, phone.tag());
            if !number.is_empty() {
                c.phones.push((phone, number));
            }
        }
        let work = address_of(|i| WORK_LIDS.get(i).map_or_else(String::new, |&id| named_text(item, id)));
        let mut work = work;
        let mailing = match i32_of(named(item, PSETID_ADDRESS, lid::POSTAL_ADDRESS_ID)) {
            Some(1) => Some(Place::Home),
            Some(2) => Some(Place::Business),
            Some(3) => Some(Place::Other),
            _ => None,
        };
        // An item written by another program may keep its work address only as the mailing one.
        if work.is_empty() && mailing.is_none_or(|m| m == Place::Business) {
            work = address_of(|i| MAIL_TAGS.get(i).map_or_else(String::new, |&id| text(item, id)));
        }
        if !work.is_empty() {
            c.addresses.push((Place::Business, work));
        }
        for (place, tags) in PLACE_TAGS {
            let a = address_of(|i| tags.get(i).map_or_else(String::new, |&id| text(item, id)));
            if !a.is_empty() {
                c.addresses.push((place, a));
            }
        }
        c.mailing = mailing.filter(|m| c.addresses.iter().any(|(p, _)| p == m));
        let day = |local: u32, tagged: u16| {
            time_of(named(item, PSETID_ADDRESS, local))
                .map(day_at_midnight)
                .or_else(|| item.props.time(tagged).map(|t| crate::calendar::day_of(t, here)))
                // No date before 1601 or after 9999.
                .filter(|d| (-134_774..=2_932_896).contains(d))
        };
        c.birthday = day(lid::BIRTHDAY_LOCAL, tag::BIRTHDAY);
        c.anniversary = day(lid::ANNIVERSARY_LOCAL, tag::ANNIVERSARY);
        if bool_of(named(item, PSETID_ADDRESS, lid::HAS_PICTURE)) != Some(false) {
            c.picture = item
                .attachments
                .iter()
                .find(|a| a.props.bool(tag::CONTACT_PHOTO) == Some(true))
                .map(|a| a.data.clone())
                .filter(|d| !d.is_empty() && d.len() <= MAX_PICTURE);
        }
        c
    }

    /// The name it is shown by: the full name, else the parts of the name, else the company,
    /// else its first address.
    pub fn display_name(&self) -> String {
        if !self.full_name.trim().is_empty() {
            return self.full_name.trim().to_string();
        }
        let made = self.name_from_parts();
        if !made.is_empty() {
            return made;
        }
        if !self.company.trim().is_empty() {
            return self.company.trim().to_string();
        }
        self.emails.first().map(|e| e.address.clone()).unwrap_or_default()
    }

    /// "Prefix Given Middle Surname Suffix", as Outlook makes a full name.
    pub fn name_from_parts(&self) -> String {
        [&self.prefix, &self.given, &self.middle, &self.surname, &self.suffix]
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// "Surname, Given Middle", as Outlook files a contact unless told otherwise.
    pub fn default_file_as(&self) -> String {
        let first: String = [&self.given, &self.middle].iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
        match (self.surname.trim(), first.as_str()) {
            ("", "") if !self.company.trim().is_empty() => self.company.trim().to_string(),
            ("", "") => self.display_name(),
            ("", f) => f.to_string(),
            (s, "") => s.to_string(),
            (s, f) => format!("{s}, {f}"),
        }
    }

    /// The name it is filed under.
    pub fn filed_as(&self) -> String {
        if self.file_as.trim().is_empty() { self.default_file_as() } else { self.file_as.trim().to_string() }
    }

    /// What it is sorted by: the name it is filed under, without case.
    pub fn sort_key(&self) -> String {
        self.filed_as().to_lowercase()
    }

    pub fn phone(&self, kind: Phone) -> Option<&str> {
        self.phones.iter().find(|(p, _)| *p == kind).map(|(_, n)| n.as_str())
    }

    pub fn address(&self, place: Place) -> Option<&Address> {
        self.addresses.iter().find(|(p, _)| *p == place).map(|(_, a)| a)
    }

    /// Sets a telephone number, or takes it away when `number` is empty.
    pub fn set_phone(&mut self, kind: Phone, number: &str) {
        self.phones.retain(|(p, _)| *p != kind);
        let number = cut(number, MAX_FIELD);
        if !number.is_empty() {
            self.phones.push((kind, number));
            self.phones.sort_by_key(|(p, _)| *p);
        }
    }

    pub fn set_address(&mut self, place: Place, address: Address) {
        self.addresses.retain(|(p, _)| *p != place);
        if !address.is_empty() {
            self.addresses.push((place, address));
            self.addresses.sort_by_key(|(p, _)| *p);
        }
    }

    /// Whether every word of `query` is somewhere in its names, company, addresses, numbers,
    /// categories or notes, without case.
    pub fn matches(&self, query: &str) -> bool {
        let mut hay = String::new();
        for s in [
            &self.full_name,
            &self.file_as,
            &self.given,
            &self.middle,
            &self.surname,
            &self.nickname,
            &self.company,
            &self.department,
            &self.job_title,
            &self.office,
            &self.web_page,
            &self.im,
            &self.notes,
        ] {
            hay.push_str(s);
            hay.push('\n');
        }
        for e in &self.emails {
            hay.push_str(&e.address);
            hay.push('\n');
            hay.push_str(&e.name);
            hay.push('\n');
        }
        for (_, n) in &self.phones {
            hay.push_str(n);
            hay.push('\n');
            // A number is found however its digits are spaced.
            hay.extend(n.chars().filter(char::is_ascii_digit));
            hay.push('\n');
        }
        for (_, a) in &self.addresses {
            hay.push_str(&a.lines());
            hay.push('\n');
        }
        for c in &self.categories {
            hay.push_str(c);
            hay.push('\n');
        }
        let hay = hay.to_lowercase();
        query.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
    }

    /// Brought up to date from `newer` (a card read again): each field it has takes the place
    /// of this one's, and what it lacks is kept, so nothing written here is lost to a card that
    /// does not know of it. E-mail addresses and categories are joined, newer first.
    pub fn merge_from(&mut self, newer: &Contact) {
        for (mine, theirs) in [
            (&mut self.prefix, &newer.prefix),
            (&mut self.given, &newer.given),
            (&mut self.middle, &newer.middle),
            (&mut self.surname, &newer.surname),
            (&mut self.suffix, &newer.suffix),
            (&mut self.full_name, &newer.full_name),
            (&mut self.file_as, &newer.file_as),
            (&mut self.nickname, &newer.nickname),
            (&mut self.company, &newer.company),
            (&mut self.department, &newer.department),
            (&mut self.job_title, &newer.job_title),
            (&mut self.office, &newer.office),
            (&mut self.profession, &newer.profession),
            (&mut self.manager, &newer.manager),
            (&mut self.assistant, &newer.assistant),
            (&mut self.spouse, &newer.spouse),
            (&mut self.web_page, &newer.web_page),
            (&mut self.home_page, &newer.home_page),
            (&mut self.im, &newer.im),
            (&mut self.notes, &newer.notes),
            (&mut self.uid, &newer.uid),
        ] {
            if !theirs.trim().is_empty() {
                mine.clone_from(theirs);
            }
        }
        let mut emails = newer.emails.clone();
        for e in &self.emails {
            if !emails.iter().any(|n| n.address.eq_ignore_ascii_case(&e.address)) {
                emails.push(e.clone());
            }
        }
        emails.truncate(MAX_EMAILS);
        self.emails = emails;
        for (kind, number) in &newer.phones {
            self.set_phone(*kind, number);
        }
        for (place, address) in &newer.addresses {
            self.set_address(*place, address.clone());
        }
        if newer.mailing.is_some() {
            self.mailing = newer.mailing;
        }
        self.birthday = newer.birthday.or(self.birthday);
        self.anniversary = newer.anniversary.or(self.anniversary);
        for c in &newer.categories {
            if !self.categories.contains(c) {
                self.categories.push(c.clone());
            }
        }
        if newer.picture.is_some() {
            self.picture.clone_from(&newer.picture);
        }
    }

    /// Writes it into `item`, keeping every property it does not speak of. `here` is where its
    /// birthday's local midnight is.
    pub fn write(&self, item: &mut Item, here: impl Offsets + Copy) {
        if !is_contact(item) {
            item.props.set_str(pid::MESSAGE_CLASS, CLASS);
        }
        let display = self.display_name();
        item.set_subject(&display);
        put(item, tag::DISPLAY_NAME, &display);
        put(item, tag::PREFIX, &self.prefix);
        put(item, tag::GIVEN_NAME, &self.given);
        put(item, tag::MIDDLE_NAME, &self.middle);
        put(item, tag::SURNAME, &self.surname);
        put(item, tag::GENERATION, &self.suffix);
        let initials: String =
            [&self.given, &self.middle, &self.surname].iter().filter_map(|s| s.trim().chars().next()).map(|c| format!("{c}.")).collect();
        put(item, tag::INITIALS, &initials);
        put(item, tag::NICKNAME, &self.nickname);
        put(item, tag::COMPANY, &self.company);
        put(item, tag::DEPARTMENT, &self.department);
        put(item, tag::TITLE, &self.job_title);
        put(item, tag::OFFICE, &self.office);
        put(item, tag::PROFESSION, &self.profession);
        put(item, tag::MANAGER, &self.manager);
        put(item, tag::ASSISTANT, &self.assistant);
        put(item, tag::SPOUSE, &self.spouse);
        put(item, tag::HOME_PAGE, &self.home_page);
        put(item, tag::BUSINESS_HOME_PAGE, &self.web_page);
        put_named(item, lid::HTML, &self.web_page);
        put_named(item, lid::IM_ADDRESS, &self.im);
        // Filed as asked; Outlook knows the usual choices by an id of their own.
        let filed = self.filed_as();
        put_named(item, lid::FILE_UNDER, &filed);
        let a = PSETID_ADDRESS;
        if filed == self.default_file_as() && !self.surname.trim().is_empty() {
            set_named(item, a, lid::FILE_UNDER_ID, PropValue::Int32(FILED_BY_NAME));
        } else if filed == self.company.trim() && !filed.is_empty() {
            set_named(item, a, lid::FILE_UNDER_ID, PropValue::Int32(FILED_BY_COMPANY));
        } else {
            remove_named(item, a, lid::FILE_UNDER_ID);
        }
        // E-mail addresses, each with the names and one-off entry id Outlook sends to.
        let mut listed = Vec::new();
        let mut array = 0i32;
        for (n, base) in lid::EMAIL_BASE.into_iter().enumerate() {
            match self.emails.get(n).filter(|e| !e.address.trim().is_empty()) {
                Some(e) => {
                    let name = if e.name.trim().is_empty() { format!("{display} ({})", e.address.trim()) } else { e.name.trim().to_string() };
                    set_named(item, a, base, PropValue::String(name.clone()));
                    set_named(item, a, base + 2, PropValue::String("SMTP".into()));
                    set_named(item, a, base + 3, PropValue::String(e.address.trim().to_string()));
                    set_named(item, a, base + 4, PropValue::String(e.address.trim().to_string()));
                    set_named(item, a, base + 5, PropValue::Binary(one_off(&name, e.address.trim())));
                    listed.push(i32::try_from(n).unwrap_or(0));
                    array |= 1 << n;
                }
                None => {
                    for at in [0, 2, 3, 4, 5] {
                        remove_named(item, a, base + at);
                    }
                }
            }
        }
        if listed.is_empty() {
            remove_named(item, a, lid::EMAIL_LIST);
            remove_named(item, a, lid::ARRAY_TYPE);
        } else {
            set_named(item, a, lid::EMAIL_LIST, PropValue::MultiInt32(listed));
            set_named(item, a, lid::ARRAY_TYPE, PropValue::Int32(array));
        }
        for phone in Phone::ALL {
            put(item, phone.tag(), self.phone(phone).unwrap_or(""));
        }
        // Addresses: the work one named, home and other tagged, the mailing one copied to the
        // tags every program reads ([MS-OXOCNTC] 2.2.1.3).
        let empty = Address::default();
        let work = self.address(Place::Business).unwrap_or(&empty);
        for (id, v) in WORK_LIDS.into_iter().zip(address_parts(work)) {
            put_named(item, id, v);
        }
        for (place, tags) in PLACE_TAGS {
            let at = self.address(place).unwrap_or(&empty);
            for (id, v) in tags.into_iter().zip(address_parts(at)) {
                put(item, id, v);
            }
        }
        for (place, id) in [(Place::Home, lid::HOME_ADDRESS), (Place::Business, lid::WORK_ADDRESS), (Place::Other, lid::OTHER_ADDRESS)] {
            put_named(item, id, &self.address(place).map(Address::lines).unwrap_or_default());
        }
        let mailing = self.mailing.filter(|m| self.address(*m).is_some()).or_else(|| self.addresses.first().map(|(p, _)| *p));
        let mail = mailing.and_then(|m| self.address(m)).unwrap_or(&empty);
        for (id, v) in MAIL_TAGS.into_iter().zip(address_parts(mail)) {
            put(item, id, v);
        }
        match mailing {
            Some(m) => set_named(item, a, lid::POSTAL_ADDRESS_ID, PropValue::Int32(m.postal_id())),
            None => remove_named(item, a, lid::POSTAL_ADDRESS_ID),
        }
        // Dates: the local midnight as a time, and the day at UTC midnight beside it.
        for (day, tagged, local) in
            [(self.birthday, tag::BIRTHDAY, lid::BIRTHDAY_LOCAL), (self.anniversary, tag::ANNIVERSARY, lid::ANNIVERSARY_LOCAL)]
        {
            match day {
                Some(d) => {
                    item.props.set(tagged, PropValue::Time(crate::calendar::day_start(d, here)));
                    set_named(item, a, local, PropValue::Time(utc_midnight(d)));
                }
                None => {
                    item.props.remove(tagged);
                    remove_named(item, a, local);
                }
            }
        }
        item.set_body_text(&self.notes);
        item.categories = self.categories.clone();
        set_uid(item, &self.uid);
        // The picture is an attachment marked as it ([MS-OXOCNTC] 2.2.1.8).
        item.attachments.retain(|at| at.props.bool(tag::CONTACT_PHOTO) != Some(true));
        match self.picture.as_ref().filter(|p| !p.is_empty() && p.len() <= MAX_PICTURE) {
            Some(bytes) => {
                let (ext, mime) = picture_kind(bytes);
                let mut at = Attachment::file(&format!("ContactPicture.{ext}"), mime, bytes.clone());
                at.props.set(tag::CONTACT_PHOTO, PropValue::Bool(true));
                // Hidden from the attachments a reader lists (PidTagAttachmentHidden).
                at.props.set(0x7FFE, PropValue::Bool(true));
                item.attachments.push(at);
                set_named(item, a, lid::HAS_PICTURE, PropValue::Bool(true));
            }
            None => set_named(item, a, lid::HAS_PICTURE, PropValue::Bool(false)),
        }
    }

    /// A new contact item.
    pub fn item(&self, here: impl Offsets + Copy) -> Item {
        let mut it = Item::default();
        it.props.set_str(pid::MESSAGE_CLASS, CLASS);
        it.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(crate::MSGFLAG_READ));
        self.write(&mut it, here);
        it
    }
}

/// The extension and media type of a picture, by its first bytes.
pub fn picture_kind(bytes: &[u8]) -> (&'static str, &'static str) {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        ("png", "image/png")
    } else if bytes.starts_with(b"GIF8") {
        ("gif", "image/gif")
    } else if bytes.starts_with(b"BM") {
        ("bmp", "image/bmp")
    } else {
        ("jpg", "image/jpeg")
    }
}

/// The provider id that begins every one-off entry id ([MS-OXCDATA] 2.2.5.1).
const ONE_OFF_PROVIDER: [u8; 16] = [0x81, 0x2B, 0x1F, 0xA4, 0xBE, 0xA3, 0x10, 0x19, 0x9D, 0x6E, 0x00, 0xDD, 0x01, 0x0F, 0x54, 0x02];

/// A one-off entry id ([MS-OXCDATA] 2.2.5.1): a name, the address type SMTP and an address, in
/// UTF-16, with no rich text asked for.
pub fn one_off(name: &str, address: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(28 + 2 * (name.len() + address.len() + 8));
    v.extend_from_slice(&[0u8; 4]);
    v.extend_from_slice(&ONE_OFF_PROVIDER);
    v.extend_from_slice(&[0, 0]);
    // M (no rich information) in the first byte, U (Unicode) in the second.
    v.extend_from_slice(&[0x01, 0x90]);
    for s in [name, "SMTP", address] {
        for u in s.chars().take(MAX_FIELD).collect::<String>().encode_utf16() {
            v.extend_from_slice(&u.to_le_bytes());
        }
        v.extend_from_slice(&[0, 0]);
    }
    v
}

/// The name, address type and address of a one-off entry id; `None` for any other entry id.
pub fn read_one_off(b: &[u8]) -> Option<(String, String, String)> {
    if b.get(4..20)? != ONE_OFF_PROVIDER {
        return None;
    }
    let unicode = b.get(23)? & 0x80 != 0;
    let mut rest = b.get(24..)?;
    let mut parts = Vec::with_capacity(3);
    for _ in 0..3 {
        if unicode {
            let mut units = Vec::new();
            let mut end = None;
            for (i, pair) in rest.chunks_exact(2).enumerate() {
                let u = u16::from_le_bytes([pair[0], pair[1]]);
                if u == 0 {
                    end = Some(i);
                    break;
                }
                units.push(u);
            }
            let end = end?;
            parts.push(cut(&String::from_utf16_lossy(&units), MAX_FIELD));
            rest = rest.get((end + 1) * 2..)?;
        } else {
            let end = rest.iter().position(|&c| c == 0)?;
            parts.push(cut(&crate::codepage::decode(1252, rest.get(..end)?), MAX_FIELD));
            rest = rest.get(end + 1..)?;
        }
    }
    let mut parts = parts.into_iter();
    Some((parts.next()?, parts.next()?, parts.next()?))
}

/// The CRC [MS-OXOCNTC] 2.2.2.1.3 asks for over a group's members: CRC-32 as [MS-OXRTFCP]
/// 2.1.3.2 reckons it, starting at 0 with nothing turned at the end.
fn checksum(parts: &[Vec<u8>]) -> u32 {
    let mut crc = 0u32;
    for b in parts.iter().flatten() {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    crc
}

/// One of a group's members.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub address: String,
}

/// A contact group: a name for a list of addresses, which a message can be sent to as one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub members: Vec<Member>,
    pub notes: String,
    pub categories: Vec<String>,
    /// The vCard UID it is known by; empty when it has none.
    pub uid: String,
}

impl Group {
    /// What `item` says of itself as a group: its members from their one-off entry ids, or
    /// from the members' own where those are one-offs too. A member kept as a link to a contact
    /// and nothing else is left out.
    pub fn of(item: &Item) -> Self {
        let mut name = named_text(item, lid::LIST_NAME);
        if name.is_empty() {
            name = text(item, tag::DISPLAY_NAME);
        }
        if name.is_empty() {
            name = cut(&item.subject(), MAX_FIELD);
        }
        let list = |id| match named(item, PSETID_ADDRESS, id) {
            Some(PropValue::MultiBinary(v)) => v.iter().take(MAX_MEMBERS).filter_map(|b| read_one_off(b)).collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        let mut found = list(lid::ONE_OFF_MEMBERS);
        if found.is_empty() {
            found = list(lid::MEMBERS);
        }
        let members = found.into_iter().filter(|(_, _, address)| !address.is_empty()).map(|(name, _, address)| Member { name, address }).collect();
        Self { name, members, notes: cut(&item.body_text(), MAX_NOTES), categories: item.categories.clone(), uid: uid_of(item).unwrap_or_default() }
    }

    /// Writes it into `item`, keeping every property it does not speak of.
    pub fn write(&self, item: &mut Item) {
        if !is_group(item) {
            item.props.set_str(pid::MESSAGE_CLASS, GROUP_CLASS);
        }
        let name = self.name.trim();
        item.set_subject(name);
        put(item, tag::DISPLAY_NAME, name);
        put_named(item, lid::LIST_NAME, name);
        put_named(item, lid::FILE_UNDER, name);
        let ids: Vec<Vec<u8>> = self
            .members
            .iter()
            .take(MAX_MEMBERS)
            .map(|m| one_off(if m.name.trim().is_empty() { m.address.trim() } else { m.name.trim() }, m.address.trim()))
            .collect();
        let a = PSETID_ADDRESS;
        set_named(item, a, lid::CHECKSUM, PropValue::Int32(i32::from_ne_bytes(checksum(&ids).to_ne_bytes())));
        set_named(item, a, lid::MEMBERS, PropValue::MultiBinary(ids.clone()));
        set_named(item, a, lid::ONE_OFF_MEMBERS, PropValue::MultiBinary(ids));
        item.set_body_text(&self.notes);
        item.categories = self.categories.clone();
        set_uid(item, &self.uid);
    }

    /// A new group item.
    pub fn item(&self) -> Item {
        let mut it = Item::default();
        it.props.set_str(pid::MESSAGE_CLASS, GROUP_CLASS);
        it.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(crate::MSGFLAG_READ));
        self.write(&mut it);
        it
    }

    pub fn matches(&self, query: &str) -> bool {
        let mut hay = self.name.to_lowercase();
        for m in &self.members {
            hay.push('\n');
            hay.push_str(&m.name.to_lowercase());
            hay.push('\n');
            hay.push_str(&m.address.to_lowercase());
        }
        query.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Contact {
        let mut c = Contact {
            prefix: "Dr.".into(),
            given: "Ada".into(),
            middle: "B.".into(),
            surname: "Lovelace".into(),
            company: "Analytical Engines".into(),
            job_title: "Engineer".into(),
            emails: vec![
                Email { address: "ada@example.com".into(), name: String::new() },
                Email { address: "ada.home@example.org".into(), name: "Ada at home".into() },
            ],
            birthday: Some(-56_000),
            anniversary: Some(10),
            notes: "Met at the\nconference.".into(),
            categories: vec!["Friends".into()],
            web_page: "https://example.com".into(),
            im: "ada@chat.example".into(),
            picture: Some(vec![0x89, b'P', b'N', b'G', 1, 2, 3]),
            uid: "urn:uuid:1234".into(),
            ..Contact::default()
        };
        c.set_phone(Phone::Mobile, "+1 555 0100");
        c.set_phone(Phone::Business, "555 0199");
        c.set_address(
            Place::Home,
            Address {
                street: "1 Main St".into(),
                city: "Springfield".into(),
                postal_code: "12345".into(),
                country: "Elsewhere".into(),
                ..Address::default()
            },
        );
        c.set_address(Place::Business, Address { street: "2 Work Rd".into(), city: "Capital".into(), ..Address::default() });
        c.mailing = Some(Place::Home);
        c
    }

    #[test]
    fn a_contact_comes_back_from_its_item_as_it_was_written() {
        for here in [0i32, 120, -300] {
            let c = sample();
            let item = c.item(here);
            assert!(is_contact(&item));
            assert_eq!(item.subject(), "Dr. Ada B. Lovelace");
            let back = Contact::of(&item, here);
            assert_eq!(back.full_name, "Dr. Ada B. Lovelace");
            assert_eq!(back.file_as, "Lovelace, Ada B.");
            assert_eq!(back.emails, c.emails);
            assert_eq!(back.phones, c.phones);
            assert_eq!(back.addresses, c.addresses);
            assert_eq!(back.mailing, Some(Place::Home));
            assert_eq!(back.birthday, c.birthday);
            assert_eq!(back.anniversary, c.anniversary);
            assert_eq!(back.notes, c.notes);
            assert_eq!(back.categories, c.categories);
            assert_eq!(back.web_page, c.web_page);
            assert_eq!(back.im, c.im);
            assert_eq!(back.picture, c.picture);
            assert_eq!(back.uid, c.uid);
            assert_eq!(item.attachments.len(), 1);
            assert_eq!(item.attachments[0].name(), "ContactPicture.png");
        }
    }

    #[test]
    fn a_card_read_again_fills_in_and_keeps_what_it_does_not_know() {
        let mut mine = sample();
        mine.office = "Room 7".into();
        let newer = Contact {
            given: "Ada".into(),
            surname: "King".into(),
            emails: vec![Email { address: "new@example.com".into(), name: String::new() }],
            ..Contact::default()
        };
        mine.merge_from(&newer);
        assert_eq!((mine.surname.as_str(), mine.office.as_str(), mine.company.as_str()), ("King", "Room 7", "Analytical Engines"));
        let addresses: Vec<&str> = mine.emails.iter().map(|e| e.address.as_str()).collect();
        assert_eq!(addresses, ["new@example.com", "ada@example.com", "ada.home@example.org"]);
        assert!(mine.picture.is_some() && mine.birthday.is_some());
    }

    #[test]
    fn a_birthday_kept_only_as_a_local_midnight_is_read_on_its_day() {
        let c = Contact { given: "B".into(), birthday: Some(20_000), ..Contact::default() };
        let mut item = c.item(180);
        remove_named(&mut item, PSETID_ADDRESS, lid::BIRTHDAY_LOCAL);
        assert_eq!(Contact::of(&item, 180).birthday, Some(20_000));
    }

    #[test]
    fn what_is_cleared_is_gone_and_what_is_not_spoken_of_stays() {
        let mut item = sample().item(0);
        item.props.set_str(0x3A4D, "kept");
        let mut c = Contact::of(&item, 0);
        c.set_phone(Phone::Mobile, "");
        c.emails.clear();
        c.picture = None;
        c.birthday = None;
        c.write(&mut item, 0);
        let back = Contact::of(&item, 0);
        assert_eq!(back.phone(Phone::Mobile), None);
        assert!(back.emails.is_empty());
        assert!(named(&item, PSETID_ADDRESS, 0x8083).is_none());
        assert!(back.picture.is_none() && item.attachments.is_empty());
        assert_eq!(back.birthday, None);
        assert_eq!(item.props.str(0x3A4D), Some("kept"));
    }

    #[test]
    fn a_contact_is_filed_and_found() {
        let c = sample();
        assert_eq!(c.filed_as(), "Lovelace, Ada B.");
        assert!(c.matches("ada engines"));
        assert!(c.matches("5550100"));
        assert!(c.matches("springfield"));
        assert!(!c.matches("babbage"));
        let company = Contact { company: "Acme".into(), ..Contact::default() };
        assert_eq!(company.display_name(), "Acme");
        assert_eq!(company.filed_as(), "Acme");
    }

    #[test]
    fn a_one_off_entry_id_is_read_as_it_is_written() {
        let id = one_off("Zoë Example", "zoe@example.com");
        assert_eq!(read_one_off(&id), Some(("Zoë Example".into(), "SMTP".into(), "zoe@example.com".into())));
        // An ANSI one.
        let mut ansi = id[..22].to_vec();
        ansi.extend_from_slice(&[0x01, 0x00]);
        ansi.extend_from_slice(b"Bob\0SMTP\0bob@example.com\0");
        assert_eq!(read_one_off(&ansi).map(|p| p.2), Some("bob@example.com".into()));
    }

    #[test]
    fn hostile_entry_ids_are_refused_without_a_panic() {
        let id = one_off("Name", "a@b.c");
        for n in 0..id.len() {
            let _ = read_one_off(&id[..n]);
        }
        assert_eq!(read_one_off(&id[..id.len() - 1]), None);
        assert_eq!(read_one_off(&[0xFF; 64]), None);
        let mut odd = id.clone();
        odd.push(7);
        assert!(read_one_off(&odd).is_some());
        // A one-off with no ends at all.
        let mut open = id[..24].to_vec();
        open.extend(std::iter::repeat_n(0x41, 4001));
        assert_eq!(read_one_off(&open), None);
    }

    #[test]
    fn a_group_keeps_its_members() {
        let g = Group {
            name: "Team".into(),
            members: vec![
                Member { name: "Ada".into(), address: "ada@example.com".into() },
                Member { name: String::new(), address: "bob@example.com".into() },
            ],
            notes: "Weekly".into(),
            categories: vec![],
            uid: "team-1".into(),
        };
        let item = g.item();
        assert!(is_group(&item) && !is_contact(&item));
        let back = Group::of(&item);
        assert_eq!(back.name, "Team");
        assert_eq!(back.members[0], g.members[0]);
        assert_eq!(back.members[1], Member { name: "bob@example.com".into(), address: "bob@example.com".into() });
        assert!(back.matches("bob"));
        assert_eq!(back.uid, "team-1");
        // The checksum is over the members as they are kept.
        let ids = match named(&item, PSETID_ADDRESS, lid::MEMBERS) {
            Some(PropValue::MultiBinary(v)) => v.clone(),
            _ => Vec::new(),
        };
        assert_eq!(i32_of(named(&item, PSETID_ADDRESS, lid::CHECKSUM)), Some(i32::from_ne_bytes(checksum(&ids).to_ne_bytes())));
    }

    #[test]
    fn a_contact_from_another_program_is_read_by_its_mailing_address_and_subject() {
        let mut item = Item::default();
        item.props.set_str(pid::MESSAGE_CLASS, "IPM.Contact");
        item.set_subject("Only A Subject");
        item.props.set_str(tag::MAIL_STREET, "9 Post Lane");
        item.props.set_str(tag::MAIL_CITY, "Town");
        // A birthday out of every calendar is left out.
        item.props.set(tag::BIRTHDAY, PropValue::Time(i64::MAX));
        let c = Contact::of(&item, 0);
        assert_eq!(c.display_name(), "Only A Subject");
        assert_eq!(c.address(Place::Business).map(|a| a.city.as_str()), Some("Town"));
        assert_eq!(c.birthday, None);
    }
}
