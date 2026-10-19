//! OpenMailClient item model: MAPI-style property bags, folders, stores.
//!
//! Outlook keeps every item (message, appointment, contact, task) as a bag of typed properties
//! ([MS-OXPROPS], [MS-OXCDATA] 2.11). OpenMailClient keeps the same shape so a data file round-trips
//! without loss: a property this code does not understand is carried, not dropped.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod calendar;
pub mod codepage;
pub mod html;
pub mod meeting;
pub mod memstore;
pub mod props;
pub mod recur;
pub mod rtf;
pub mod time;
pub mod zone;

pub use memstore::MemStore;
pub use props::{NameId, NamedKey, PropBag, PropValue, pid, pset, ptype};

/// A folder's identity inside one store (a node id in a `.pst`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FolderId(pub u64);

/// An item's identity inside one store (a node id in a `.pst`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(pub u64);

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("this data file is open read-only")]
    ReadOnly,
    #[error("folder not found")]
    NoFolder,
    #[error("item not found")]
    NoItem,
    #[error("{0}")]
    Format(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// The long part of a save, to be run once, anywhere. It reports how far it is and stops
/// early when asked to.
pub type SaveWork = Box<dyn FnOnce(std::sync::Arc<Progress>) -> Result<SaveDone, StoreError> + Send>;
/// What the long part of a save gives back to the store that began it.
pub type SaveDone = Box<dyn std::any::Any + Send>;

/// The mailbox as it stands, to be had on another thread: run there, it gives a store of
/// its own that the first one's later changes do not reach. What that store does at
/// length, it reports to the progress it was given, and stops when that says so.
pub type Apart = Box<dyn FnOnce(std::sync::Arc<Progress>) -> Result<Box<dyn Store>, StoreError> + Send>;

/// How far a long piece of work is, read by whoever shows it while it runs elsewhere.
#[derive(Debug, Default)]
pub struct Progress {
    done: std::sync::atomic::AtomicU64,
    total: std::sync::atomic::AtomicU64,
    stop: std::sync::atomic::AtomicBool,
}

impl Progress {
    /// Say how many steps there are in all, and start counting from none.
    pub fn begin(&self, total: u64) {
        self.done.store(0, std::sync::atomic::Ordering::Relaxed);
        self.total.store(total, std::sync::atomic::Ordering::Relaxed);
    }

    /// One more step is done.
    pub fn step(&self) {
        self.done.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// This part of all the steps is done as well: 4 for a quarter of them.
    pub fn part(&self, of: u64) {
        let total = self.total.load(std::sync::atomic::Ordering::Relaxed);
        self.done.fetch_add(total / of.max(1), std::sync::atomic::Ordering::Relaxed);
    }

    /// How much is done, from 0 to 1; 0 while it is not known how much there is.
    pub fn fraction(&self) -> f32 {
        let total = self.total.load(std::sync::atomic::Ordering::Relaxed);
        let done = self.done.load(std::sync::atomic::Ordering::Relaxed).min(total);
        if total == 0 { 0.0 } else { (done as f64 / total as f64) as f32 }
    }

    /// Ask the work to stop where it is.
    pub fn stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// What a folder is for; decides its icon and its place in the folder pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderKind {
    Root,
    Inbox,
    Drafts,
    Sent,
    Deleted,
    Junk,
    Outbox,
    Archive,
    Calendar,
    Contacts,
    Tasks,
    Notes,
    Journal,
    Search,
    Mail,
    Other,
}

impl FolderKind {
    /// Guess from the container class ([MS-OXOSFLD]) and, for mail folders, the well-known English name.
    pub fn guess(name: &str, container_class: &str) -> Self {
        let class = container_class.to_ascii_lowercase();
        if class.starts_with("ipf.appointment") {
            return Self::Calendar;
        }
        if class.starts_with("ipf.contact") {
            return Self::Contacts;
        }
        if class.starts_with("ipf.task") {
            return Self::Tasks;
        }
        if class.starts_with("ipf.stickynote") {
            return Self::Notes;
        }
        if class.starts_with("ipf.journal") {
            return Self::Journal;
        }
        match name.to_ascii_lowercase().as_str() {
            "inbox" => Self::Inbox,
            "drafts" => Self::Drafts,
            "sent items" | "sent" | "sent mail" => Self::Sent,
            "deleted items" | "trash" | "bin" => Self::Deleted,
            "junk email" | "junk e-mail" | "junk" | "spam" => Self::Junk,
            "outbox" => Self::Outbox,
            "archive" => Self::Archive,
            "search folders" | "search root" => Self::Search,
            _ if class.is_empty() || class.starts_with("ipf.note") => Self::Mail,
            _ => Self::Other,
        }
    }

    /// Outlook lists the special mail folders first, in this order, then the rest by name.
    pub fn sort_rank(self) -> u8 {
        match self {
            Self::Inbox => 0,
            Self::Drafts => 1,
            Self::Sent => 2,
            Self::Deleted => 3,
            Self::Archive => 4,
            Self::Junk => 6,
            Self::Outbox => 7,
            Self::Search => 9,
            _ => 5,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FolderInfo {
    pub id: FolderId,
    pub parent: Option<FolderId>,
    pub name: String,
    pub kind: FolderKind,
    pub container_class: String,
    pub total: u32,
    pub unread: u32,
    pub has_children: bool,
    /// A folder of the mail program's own that a folder pane does not show (PidTagAttributeHidden,
    /// [MS-OXPROPS] 2.609): kept and saved as any other.
    pub hidden: bool,
}

/// Follow-up flag ([MS-OXOFLAG] PidTagFlagStatus).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Flag {
    #[default]
    None,
    Complete,
    Flagged,
}

/// Characters of a body a message list's preview holds, as many as Outlook keeps (PidTagPreview).
pub const PREVIEW_CHARS: usize = 255;

/// One row of a folder's message list: enough to draw the list without loading the item.
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub id: u64,
    pub subject: String,
    pub from: String,
    pub to: String,
    /// UTC FILETIME ticks (100 ns since 1601-01-01); 0 when unknown.
    pub time: i64,
    pub size: u32,
    pub read: bool,
    pub has_attachments: bool,
    /// 0 low, 1 normal, 2 high.
    pub importance: i32,
    pub flag: Flag,
    pub message_class: String,
    pub preview: String,
    /// The preview is the whole body: the list says so after it, as Outlook does.
    pub preview_whole: bool,
    pub categories: Vec<String>,
    /// When a message held in the Outbox may go; 0 when it is not held.
    pub deliver_after: i64,
    /// When it stops being of use; 0 when it does not.
    pub expires: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attachment {
    pub props: PropBag,
    pub data: Vec<u8>,
    /// An attached Outlook item (attach method 5), when the reader could load it.
    pub embedded: Option<Box<Item>>,
}

impl Attachment {
    pub fn file(name: &str, mime: &str, data: Vec<u8>) -> Self {
        let mut props = PropBag::default();
        props.set_str(pid::ATTACH_LONG_FILENAME, name);
        props.set_str(pid::ATTACH_FILENAME, name);
        props.set_str(pid::DISPLAY_NAME, name);
        if !mime.is_empty() {
            props.set_str(pid::ATTACH_MIME_TAG, mime);
        }
        props.set(pid::ATTACH_METHOD, PropValue::Int32(1));
        props.set(pid::ATTACH_SIZE, PropValue::Int32(i32::try_from(data.len()).unwrap_or(i32::MAX)));
        Self { props, data, embedded: None }
    }

    pub fn name(&self) -> String {
        for id in [pid::ATTACH_LONG_FILENAME, pid::ATTACH_FILENAME, pid::DISPLAY_NAME] {
            if let Some(s) = self.props.str(id)
                && !s.is_empty()
            {
                return s.to_string();
            }
        }
        "attachment".to_string()
    }

    pub fn mime(&self) -> &str {
        self.props.str(pid::ATTACH_MIME_TAG).unwrap_or("application/octet-stream")
    }

    pub fn content_id(&self) -> Option<&str> {
        self.props.str(pid::ATTACH_CONTENT_ID)
    }

    pub fn size(&self) -> usize {
        if !self.data.is_empty() {
            return self.data.len();
        }
        usize::try_from(self.props.i32(pid::ATTACH_SIZE).unwrap_or(0)).unwrap_or(0)
    }
}

/// Recipient type ([MS-OXCMSG] PidTagRecipientType).
pub const RECIP_TO: i32 = 1;
pub const RECIP_CC: i32 = 2;
pub const RECIP_BCC: i32 = 3;

pub const MSGFLAG_READ: i32 = 0x01;
pub const MSGFLAG_UNSENT: i32 = 0x08;
pub const MSGFLAG_HASATTACH: i32 = 0x10;

/// A message, appointment, contact or task: a property bag plus recipient and attachment tables.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub props: PropBag,
    pub recipients: Vec<PropBag>,
    pub attachments: Vec<Attachment>,
    /// PidNameKeywords, kept apart because its property id differs per store.
    pub categories: Vec<String>,
    /// Every other named property, by name ([MS-OXCDATA] 2.6.1).
    pub named: Vec<(NamedKey, PropValue)>,
}

impl Item {
    /// A new, unsent mail message.
    pub fn new_mail() -> Self {
        let mut it = Self::default();
        it.props.set_str(pid::MESSAGE_CLASS, "IPM.Note");
        it.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(MSGFLAG_UNSENT | MSGFLAG_READ));
        it.props.set(pid::IMPORTANCE, PropValue::Int32(1));
        it
    }

    pub fn message_class(&self) -> &str {
        self.props.str(pid::MESSAGE_CLASS).unwrap_or("IPM.Note")
    }

    /// The subject as shown, without the two-character prefix marker some stores keep ([MS-PST] 2.5.3.1.1.1).
    pub fn subject(&self) -> String {
        clean_subject(self.props.str(pid::SUBJECT).unwrap_or(""))
    }

    pub fn set_subject(&mut self, s: &str) {
        self.props.set_str(pid::SUBJECT, s);
        self.props.set_str(pid::NORMALIZED_SUBJECT, strip_reply_prefix(s));
        self.props.set_str(pid::CONVERSATION_TOPIC, strip_reply_prefix(s));
    }

    pub fn sender_name(&self) -> String {
        for id in [pid::SENT_REPRESENTING_NAME, pid::SENDER_NAME] {
            if let Some(s) = self.props.str(id)
                && !s.is_empty()
            {
                return s.to_string();
            }
        }
        self.sender_email()
    }

    pub fn sender_email(&self) -> String {
        for id in [pid::SENDER_SMTP_ADDRESS, pid::SENT_REPRESENTING_SMTP_ADDRESS, pid::SENDER_EMAIL_ADDRESS, pid::SENT_REPRESENTING_EMAIL_ADDRESS] {
            if let Some(s) = self.props.str(id)
                && s.contains('@')
            {
                return s.to_string();
            }
        }
        self.props.str(pid::SENDER_EMAIL_ADDRESS).unwrap_or("").to_string()
    }

    pub fn set_sender(&mut self, name: &str, email: &str) {
        for id in [pid::SENDER_NAME, pid::SENT_REPRESENTING_NAME] {
            self.props.set_str(id, name);
        }
        for id in [pid::SENDER_EMAIL_ADDRESS, pid::SENT_REPRESENTING_EMAIL_ADDRESS, pid::SENDER_SMTP_ADDRESS] {
            self.props.set_str(id, email);
        }
        for id in [pid::SENDER_ADDRESS_TYPE, pid::SENT_REPRESENTING_ADDRESS_TYPE] {
            self.props.set_str(id, "SMTP");
        }
    }

    /// Received time, or failing that the sent, modified or created time. UTC FILETIME ticks.
    pub fn time(&self) -> i64 {
        for id in [pid::MESSAGE_DELIVERY_TIME, pid::CLIENT_SUBMIT_TIME, pid::LAST_MODIFICATION_TIME, pid::CREATION_TIME] {
            if let Some(t) = self.props.time(id)
                && t > 0
            {
                return t;
            }
        }
        0
    }

    pub fn flags(&self) -> i32 {
        self.props.i32(pid::MESSAGE_FLAGS).unwrap_or(0)
    }

    pub fn is_read(&self) -> bool {
        self.flags() & MSGFLAG_READ != 0
    }

    pub fn set_read(&mut self, read: bool) {
        let f = self.flags();
        self.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(if read { f | MSGFLAG_READ } else { f & !MSGFLAG_READ }));
    }

    /// When a message held in the Outbox may go (Delay Delivery); 0 when it is not held.
    pub fn deliver_after(&self) -> i64 {
        self.props.time(pid::DEFERRED_DELIVERY_TIME).filter(|&t| t > 0).unwrap_or(0)
    }

    /// When it stops being of use (Expires After); 0 when it does not.
    pub fn expires(&self) -> i64 {
        self.props.time(pid::EXPIRY_TIME).filter(|&t| t > 0).unwrap_or(0)
    }

    pub fn is_unsent(&self) -> bool {
        self.flags() & MSGFLAG_UNSENT != 0
    }

    pub fn importance(&self) -> i32 {
        self.props.i32(pid::IMPORTANCE).unwrap_or(1)
    }

    pub fn flag(&self) -> Flag {
        match self.props.i32(pid::FLAG_STATUS).unwrap_or(0) {
            1 => Flag::Complete,
            2 => Flag::Flagged,
            _ => Flag::None,
        }
    }

    pub fn set_flag(&mut self, flag: Flag) {
        match flag {
            Flag::None => self.props.remove(pid::FLAG_STATUS),
            Flag::Complete => self.props.set(pid::FLAG_STATUS, PropValue::Int32(1)),
            Flag::Flagged => self.props.set(pid::FLAG_STATUS, PropValue::Int32(2)),
        }
    }

    /// The plain-text body; derived from the HTML or RTF body when the item has no plain one.
    pub fn body_text(&self) -> String {
        if let Some(s) = self.props.str(pid::BODY)
            && !s.trim().is_empty()
        {
            return s.replace("\r\n", "\n");
        }
        if let Some(h) = self.html_property() {
            return html::to_text(&h);
        }
        self.body_rtf().map(|r| rtf::to_text(&r)).unwrap_or_default()
    }

    /// The HTML body: PidTagHtml, or else the HTML an RTF body encapsulates ([MS-OXRTFEX]).
    pub fn body_html(&self) -> Option<String> {
        self.html_property().or_else(|| match rtf::encapsulated(&self.body_rtf()?)? {
            rtf::Wrapped::Html(h) => Some(h),
            rtf::Wrapped::Text(_) => None,
        })
    }

    fn html_property(&self) -> Option<String> {
        match self.props.get(pid::HTML) {
            Some(PropValue::Binary(b)) => Some(String::from_utf8_lossy(b).into_owned()),
            Some(PropValue::String(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// The RTF body (PidTagRtfCompressed, [MS-OXRTFCP]), decompressed.
    pub fn body_rtf(&self) -> Option<Vec<u8>> {
        match self.props.get(pid::RTF_COMPRESSED) {
            Some(PropValue::Binary(b)) => rtf::decompress(b),
            _ => None,
        }
    }

    /// Make the HTML body UTF-8 and say so in PidTagInternetCodepage, so every reader of the
    /// item can treat the bytes alike. Readers call this once after loading.
    pub fn normalise_html(&mut self) {
        let cp = self.props.i32(pid::INTERNET_CODEPAGE).or_else(|| self.props.i32(pid::MESSAGE_CODEPAGE)).unwrap_or(0);
        let utf8 = match self.props.get(pid::HTML) {
            Some(PropValue::Binary(b)) if cp != 65001 || std::str::from_utf8(b).is_err() => codepage::decode(u32::try_from(cp).unwrap_or(0), b),
            _ => return,
        };
        self.props.set(pid::HTML, PropValue::Binary(utf8.into_bytes()));
        self.props.set(pid::INTERNET_CODEPAGE, PropValue::Int32(65001));
    }

    pub fn set_body_text(&mut self, s: &str) {
        self.props.set_str(pid::BODY, &s.replace("\r\n", "\n").replace('\n', "\r\n"));
    }

    /// Display names of the recipients of one type, "; "-separated, as in the To/Cc lines.
    pub fn display_recipients(&self, kind: i32) -> String {
        let from_table: Vec<String> =
            self.recipients.iter().filter(|r| r.i32(pid::RECIPIENT_TYPE).unwrap_or(RECIP_TO) == kind).map(recipient_label).collect();
        if !from_table.is_empty() {
            return from_table.join("; ");
        }
        let id = match kind {
            RECIP_CC => pid::DISPLAY_CC,
            RECIP_BCC => pid::DISPLAY_BCC,
            _ => pid::DISPLAY_TO,
        };
        self.props.str(id).unwrap_or("").to_string()
    }

    pub fn add_recipient(&mut self, kind: i32, name: &str, email: &str) {
        let mut r = PropBag::default();
        r.set(pid::RECIPIENT_TYPE, PropValue::Int32(kind));
        r.set_str(pid::DISPLAY_NAME, if name.is_empty() { email } else { name });
        r.set_str(pid::EMAIL_ADDRESS, email);
        r.set_str(pid::SMTP_ADDRESS, email);
        r.set_str(pid::ADDRESS_TYPE, "SMTP");
        self.recipients.push(r);
        self.refresh_display_recipients();
    }

    /// Recompute PidTagDisplayTo/Cc/Bcc from the recipient table.
    pub fn refresh_display_recipients(&mut self) {
        for (kind, id) in [(RECIP_TO, pid::DISPLAY_TO), (RECIP_CC, pid::DISPLAY_CC), (RECIP_BCC, pid::DISPLAY_BCC)] {
            let names: Vec<String> =
                self.recipients.iter().filter(|r| r.i32(pid::RECIPIENT_TYPE).unwrap_or(RECIP_TO) == kind).map(recipient_label).collect();
            self.props.set_str(id, &names.join("; "));
        }
    }

    /// Attachments a person would count: not the inline pictures an HTML body refers to, nor
    /// the calendar a meeting's message carries, which is the meeting itself.
    pub fn visible_attachments(&self) -> impl Iterator<Item = &Attachment> {
        let html = self.body_html().unwrap_or_default();
        let meeting = meeting::is_meeting_message(self);
        self.attachments.iter().filter(move |a| {
            !(meeting && a.mime().eq_ignore_ascii_case("text/calendar"))
                && match a.content_id() {
                    Some(cid) if !cid.is_empty() => !html.contains(&format!("cid:{cid}")),
                    _ => true,
                }
        })
    }

    pub fn summary(&self, id: u64) -> Summary {
        let (preview, preview_whole) = self.preview();
        Summary {
            id,
            subject: self.subject(),
            from: self.sender_name(),
            to: self.display_recipients(RECIP_TO),
            time: self.time(),
            size: u32::try_from(self.approx_size()).unwrap_or(u32::MAX),
            read: self.is_read(),
            has_attachments: self.visible_attachments().next().is_some(),
            importance: self.importance(),
            flag: self.flag(),
            message_class: self.message_class().to_string(),
            preview,
            preview_whole,
            categories: self.categories.clone(),
            deliver_after: self.deliver_after(),
            expires: self.expires(),
        }
    }

    /// The start of the body on one line, as the message list shows it, and whether that is
    /// all of it. A message without a body says what Outlook kept of it (PidTagPreview).
    pub fn preview(&self) -> (String, bool) {
        let body = self.body_text();
        let mut words = body.split_whitespace();
        let mut preview = String::new();
        let mut whole = true;
        for word in words.by_ref() {
            if preview.chars().count().saturating_add(word.chars().count()) > PREVIEW_CHARS {
                // A word longer than the whole preview, as a long link is, is cut.
                if preview.is_empty() {
                    preview.extend(word.chars().take(PREVIEW_CHARS));
                }
                whole = false;
                break;
            }
            if !preview.is_empty() {
                preview.push(' ');
            }
            preview.push_str(word);
        }
        if preview.is_empty() && whole {
            let kept = self.props.str(pid::PREVIEW).unwrap_or("");
            let kept: String = kept.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(PREVIEW_CHARS).collect();
            // What Outlook kept may have been cut short: only an empty body is known whole.
            return (kept.clone(), kept.is_empty());
        }
        (preview, whole)
    }

    pub fn approx_size(&self) -> usize {
        if let Some(n) = self.props.i32(pid::MESSAGE_SIZE)
            && n > 0
        {
            return usize::try_from(n).unwrap_or(0);
        }
        let body = self.props.str(pid::BODY).map_or(0, str::len);
        let html = match self.props.get(pid::HTML) {
            Some(PropValue::Binary(b)) => b.len(),
            _ => 0,
        };
        body.saturating_add(html).saturating_add(self.attachments.iter().map(Attachment::size).sum::<usize>()).saturating_add(512)
    }
}

/// "Name" for a recipient row, falling back to its address.
pub fn recipient_label(r: &PropBag) -> String {
    match r.str(pid::DISPLAY_NAME) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => recipient_email(r),
    }
}

pub fn recipient_email(r: &PropBag) -> String {
    for id in [pid::SMTP_ADDRESS, pid::EMAIL_ADDRESS] {
        if let Some(s) = r.str(id)
            && s.contains('@')
        {
            return s.to_string();
        }
    }
    r.str(pid::EMAIL_ADDRESS).unwrap_or("").to_string()
}

/// Drop the `\x01\xNN` subject-prefix marker a `.pst` keeps in front of some subjects.
pub fn clean_subject(s: &str) -> String {
    let mut chars = s.chars();
    if chars.next() == Some('\u{1}') {
        chars.next();
        return chars.as_str().to_string();
    }
    s.to_string()
}

/// The subject without leading "RE:" / "FW:" / "FWD:" prefixes (the conversation topic).
pub fn strip_reply_prefix(s: &str) -> &str {
    let mut rest = s.trim_start();
    loop {
        let lower: String = rest.chars().take(4).collect::<String>().to_ascii_lowercase();
        let cut = if lower.starts_with("re:") || lower.starts_with("fw:") {
            3
        } else if lower == "fwd:" {
            4
        } else {
            return rest;
        };
        rest = rest.get(cut..).unwrap_or("").trim_start();
    }
}

/// What playing back a journal of unsaved changes did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Recovery {
    /// Changes brought back.
    pub applied: u32,
    /// Changes that were damaged, cut short or could not be made again, and those after them
    /// that could still be counted.
    pub lost: u32,
    /// True when more than `lost` may not have been brought back: what follows the last
    /// record that could be counted cannot be told into records.
    pub more: bool,
    /// True when a journal was found that is not for the file as it is now; it is left alone.
    pub stale: bool,
    /// True when another running program holds the journal.
    pub held: bool,
}

/// A mail store: an Outlook data file, a sample mailbox, later a server account's cache.
/// What `Store::copy_to` put in the new file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Copied {
    pub folders: u64,
    pub items: u64,
}

pub trait Store {
    fn display_name(&self) -> String;
    /// Where the file lives, when it is a file.
    fn path(&self) -> Option<&std::path::Path> {
        None
    }
    fn writable(&self) -> bool {
        false
    }
    /// True when changes were made that `save` has not yet put where the store is kept.
    fn unsaved(&self) -> bool {
        false
    }
    /// Bring back the changes a run that ended without saving left in a journal. They are
    /// unsaved again, as they were; the data file is not written.
    fn recover(&mut self) -> Result<Recovery, StoreError> {
        Ok(Recovery::default())
    }

    /// Throw the unsaved changes away, and their journal with them.
    fn discard(&mut self) {}

    /// A folder of the program's own to keep the journal of unsaved changes in when it
    /// cannot be kept beside the data file. Said before `recover`.
    fn journal_elsewhere(&mut self, _dir: &std::path::Path) {}

    /// Have the disk keep what was journalled so far. True when there was something to keep.
    fn settle(&mut self) -> bool {
        false
    }

    /// Why changes are held in memory only, when they are not also journalled.
    fn journal_trouble(&self) -> Option<String> {
        None
    }

    /// Put the changes made so far where the store is kept. `seed` is 16 unpredictable bytes
    /// for the ids of what is new. A store that keeps nothing anywhere has nothing to do.
    fn save(&mut self, _seed: [u8; 16]) -> Result<(), StoreError> {
        Ok(())
    }

    /// Begin a save whose long part can be done away from the caller: the work comes back to
    /// be run anywhere (another thread, say) and what it gives is handed to `save_end`. Until
    /// then the store is read as before and takes no changes. `None` when this store has no
    /// such work: `save` is the way then.
    fn save_begin(&mut self, _seed: [u8; 16]) -> Result<Option<SaveWork>, StoreError> {
        Ok(None)
    }

    /// End a save begun with `save_begin`, with what its work gave. Whatever that was, the
    /// store takes changes again afterwards; when it was an error, nothing was saved.
    fn save_end(&mut self, _done: Result<SaveDone, StoreError>) -> Result<(), StoreError> {
        Ok(())
    }

    /// True between `save_begin` and `save_end`.
    fn saving(&self) -> bool {
        false
    }

    /// Write the store as it stands, unsaved changes with it, to a new file at `to`, with
    /// all that is stored and the model does not hold: hidden items, a folder's own
    /// properties, properties this program does not know. The store itself is only read.
    /// `None` when this store cannot (the caller then writes what the model gives).
    fn copy_to(&mut self, _to: &std::path::Path, _seed: [u8; 16]) -> Result<Option<Copied>, StoreError> {
        Ok(None)
    }
    /// Whether the file as it was stays beside a data file the first time a save replaces
    /// it. It does unless this says otherwise; a store that is in no file has nothing to keep.
    fn keep_original(&mut self, _keep: bool) {}
    /// Whether a save writes what changed into the data file where it is when that can be
    /// done, or always writes a whole new file. It does unless this says otherwise.
    fn save_in_place(&mut self, _on: bool) {}
    /// How many saves of this store went into its data file where it is.
    fn saved_in_place(&self) -> u64 {
        0
    }
    /// The mailbox as it stands when this is asked, for a thread of its own: a data file is
    /// read there through a handle of its own, with the changes as they are now, so this
    /// store goes on being read and changed meanwhile. `None` when this store cannot.
    fn apart(&mut self) -> Result<Option<Apart>, StoreError> {
        Ok(None)
    }
    /// A folder's hidden (associated) items: views, rules, forms and the like that other
    /// programs keep beside the folder's messages, each with its id. Their order is the
    /// store's; one that cannot be read is given as the error it was. None in a store that
    /// keeps no such items.
    fn hidden(&mut self, _folder: FolderId) -> Result<Vec<(ItemId, Result<Item, StoreError>)>, StoreError> {
        Ok(Vec::new())
    }
    /// A folder's own properties as they are kept, those the model has no place for among
    /// them. Empty in a store that keeps none.
    fn folder_props(&mut self, _folder: FolderId) -> Result<PropBag, StoreError> {
        Ok(PropBag::default())
    }
    /// The id the store gives itself, which its entry ids carry ([MS-OXCDATA] 2.2.4.1),
    /// where it has one.
    fn record_key(&mut self) -> Option<[u8; 16]> {
        None
    }
    /// Every folder under the visible root (the "IPM subtree"), parents before children.
    fn folders(&mut self) -> Result<Vec<FolderInfo>, StoreError>;
    fn list(&mut self, folder: FolderId) -> Result<Vec<Summary>, StoreError>;
    fn load(&mut self, folder: FolderId, id: ItemId) -> Result<Item, StoreError>;

    fn update(&mut self, _folder: FolderId, _id: ItemId, _item: Item) -> Result<(), StoreError> {
        Err(StoreError::ReadOnly)
    }
    fn add(&mut self, _folder: FolderId, _item: Item) -> Result<ItemId, StoreError> {
        Err(StoreError::ReadOnly)
    }
    fn remove(&mut self, _folder: FolderId, _id: ItemId) -> Result<Item, StoreError> {
        Err(StoreError::ReadOnly)
    }
    fn create_folder(&mut self, _parent: FolderId, _name: &str) -> Result<FolderId, StoreError> {
        Err(StoreError::ReadOnly)
    }
    /// A mail folder at the top of the data file, beside the Inbox.
    fn create_top_folder(&mut self, _name: &str) -> Result<FolderId, StoreError> {
        Err(StoreError::ReadOnly)
    }
    fn rename_folder(&mut self, _folder: FolderId, _name: &str) -> Result<(), StoreError> {
        Err(StoreError::ReadOnly)
    }
    fn delete_folder(&mut self, _folder: FolderId) -> Result<(), StoreError> {
        Err(StoreError::ReadOnly)
    }
    /// Put `folder`, with all it holds, under `parent`. Never into itself or a folder inside it.
    fn move_folder(&mut self, _folder: FolderId, _parent: FolderId) -> Result<(), StoreError> {
        Err(StoreError::ReadOnly)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_prefix_marker_is_dropped() {
        assert_eq!(clean_subject("\u{1}\u{5}RE: Budget"), "RE: Budget");
        assert_eq!(clean_subject("Budget"), "Budget");
        assert_eq!(clean_subject("\u{1}"), "");
    }

    #[test]
    fn reply_prefixes_are_stripped() {
        assert_eq!(strip_reply_prefix("RE: FW: re: Budget"), "Budget");
        assert_eq!(strip_reply_prefix("Fwd: Budget"), "Budget");
        assert_eq!(strip_reply_prefix("Regarding"), "Regarding");
        assert_eq!(strip_reply_prefix("RE:"), "");
    }

    #[test]
    fn display_to_follows_recipient_table() {
        let mut it = Item::new_mail();
        it.add_recipient(RECIP_TO, "Ana Costa", "ana@example.org");
        it.add_recipient(RECIP_TO, "", "bo@example.org");
        it.add_recipient(RECIP_CC, "Cy", "cy@example.org");
        assert_eq!(it.display_recipients(RECIP_TO), "Ana Costa; bo@example.org");
        assert_eq!(it.props.str(pid::DISPLAY_CC), Some("Cy"));
    }

    #[test]
    fn inline_pictures_are_not_counted_as_attachments() {
        let mut it = Item::new_mail();
        it.props.set(pid::HTML, PropValue::Binary(b"<img src=\"cid:logo1\">".to_vec()));
        let mut a = Attachment::file("logo.png", "image/png", vec![1, 2, 3]);
        a.props.set_str(pid::ATTACH_CONTENT_ID, "logo1");
        it.attachments.push(a);
        assert_eq!(it.visible_attachments().count(), 0);
        it.attachments.push(Attachment::file("report.pdf", "application/pdf", vec![1]));
        assert_eq!(it.visible_attachments().count(), 1);
    }

    #[test]
    fn a_preview_is_the_start_of_the_body_on_one_line_and_says_when_it_is_all_of_it() {
        let mut it = Item::default();
        it.props.set_str(pid::BODY, "\r\n\r\n  test\r\n");
        assert_eq!(it.preview(), ("test".to_string(), true));
        let long = "word ".repeat(100);
        it.props.set_str(pid::BODY, &long);
        let (preview, whole) = it.preview();
        assert!(!whole && preview.chars().count() <= PREVIEW_CHARS && preview.ends_with("word"));
        // One word longer than the preview, as a long link is, is cut.
        it.props.set_str(pid::BODY, &"x".repeat(1000));
        assert_eq!(it.preview(), ("x".repeat(PREVIEW_CHARS), false));
        // Without a body, what Outlook kept stands, and may have been cut short.
        let mut kept = Item::default();
        kept.props.set_str(pid::PREVIEW, " Kept\r\nby Outlook ");
        assert_eq!(kept.preview(), ("Kept by Outlook".to_string(), false));
        assert_eq!(Item::default().preview(), (String::new(), true));
        assert!(it.summary(1).preview == "x".repeat(PREVIEW_CHARS));
    }
}
