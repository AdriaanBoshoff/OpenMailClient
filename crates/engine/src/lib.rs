//! OpenMailClient engine: session, stores, command registry, Outlook feature catalog, sample mailbox.
//!
//! Everything a person can do is a command with an id (`cmd::registry`). The ribbon, shortcuts,
//! the CLI and agents all go through [`Session::run`]; nothing here opens a dialog, and mail
//! servers are reached only through the connector the host gives (`Session::connector`), on a
//! thread of its own (`jobs`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod accounts;
pub mod arrange;
pub mod arrival;
pub mod attachments;
pub mod autocomplete;
pub mod catalog;
pub mod cmd;
pub mod io;
pub mod jobs;
pub mod links;
pub mod outgoing;
pub mod outlook;
pub mod pictures;
pub mod print;
pub mod printers;
pub mod reminders;
pub mod remote;
pub mod rules;
pub mod sample;
pub mod search;
pub mod signatures;
mod svg;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_accounts;
#[cfg(test)]
mod tests_arrival;
#[cfg(test)]
mod tests_autocomplete;
#[cfg(test)]
mod tests_caldav;
#[cfg(test)]
mod tests_caldav_tasks;
#[cfg(test)]
mod tests_calendar;
#[cfg(test)]
mod tests_calendar_file;
#[cfg(test)]
mod tests_carddav;
#[cfg(test)]
mod tests_import;
#[cfg(test)]
mod tests_meeting;
#[cfg(test)]
mod tests_names;
#[cfg(test)]
mod tests_people;
#[cfg(test)]
mod tests_pictures;
#[cfg(test)]
mod tests_print;
#[cfg(test)]
mod tests_receive;
#[cfg(test)]
mod tests_reminders;
#[cfg(test)]
mod tests_remote;
#[cfg(test)]
mod tests_rules;
#[cfg(test)]
mod tests_send;
#[cfg(test)]
mod tests_signatures;
#[cfg(test)]
mod tests_sync;
#[cfg(test)]
mod tests_task_request;
#[cfg(test)]
mod tests_tasks;
#[cfg(test)]
mod tests_todo_bar;
#[cfg(test)]
mod tests_update;
pub mod undo;
pub mod update;
pub mod watch;

use openmailclient_model::zone::Zone;
use openmailclient_model::{Attachment, FolderId, FolderInfo, FolderKind, Item, ItemId, Store, StoreError, Summary};
use serde_json::{Value, json};

pub use cmd::{CommandSpec, registry};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command: {0}")]
    UnknownCommand(String),
    #[error("{0} is not available right now")]
    Disabled(String),
    #[error("bad parameters: {0}")]
    BadParams(String),
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Format(String),
    #[error("internal error in {0}; nothing was changed on disk")]
    Internal(String),
}

pub type CmdResult = Result<Value, EngineError>;

/// The four Outlook modules of the navigation bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Module {
    Mail,
    Calendar,
    People,
    Tasks,
}

impl Module {
    pub fn name(self) -> &'static str {
        match self {
            Self::Mail => "Mail",
            Self::Calendar => "Calendar",
            Self::People => "People",
            Self::Tasks => "Tasks",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "mail" => Some(Self::Mail),
            "calendar" => Some(Self::Calendar),
            "people" | "contacts" => Some(Self::People),
            "tasks" => Some(Self::Tasks),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Date,
    From,
    To,
    Subject,
    Size,
    Importance,
    Flag,
    Attachments,
}

impl SortKey {
    pub fn name(self) -> &'static str {
        match self {
            Self::Date => "Date",
            Self::From => "From",
            Self::To => "To",
            Self::Subject => "Subject",
            Self::Size => "Size",
            Self::Importance => "Importance",
            Self::Flag => "Flag",
            Self::Attachments => "Attachments",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "date" => Self::Date,
            "from" => Self::From,
            "to" => Self::To,
            "subject" => Self::Subject,
            "size" => Self::Size,
            "importance" => Self::Importance,
            "flag" => Self::Flag,
            "attachments" => Self::Attachments,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingPane {
    Right,
    Bottom,
    Off,
}

/// A message being written in a compose window.
#[derive(Clone, Debug, Default)]
pub struct Draft {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    pub attachments: Vec<Attachment>,
    pub show_bcc: bool,
    /// "New", "Reply", "Reply All" or "Forward": what opened this window.
    pub kind: &'static str,
    /// The message this answers reached its writer as a hidden recipient, and it answers all.
    pub was_hidden: bool,
    /// Its writer has put something into it.
    pub touched: bool,
    /// The id of the account it goes from; empty for the first account.
    pub account: String,
    /// What it answers, as the headers that keep a conversation together ([RFC 5322] 3.6.4).
    pub in_reply_to: String,
    pub references: String,
    /// Delay Delivery: it waits in the Outbox until then (store time); 0 when it goes at once.
    pub deliver_after: i64,
    /// Expires After: when it stops being of use (store time); 0 when it does not.
    pub expires: i64,
    /// Where it is kept in Drafts, once saved or when it was opened from there: the next save
    /// takes that one's place, and sending it takes it out of Drafts.
    pub filed: Option<Filed>,
    /// The signature put in it, as it was put in, so another can take its place.
    pub signature: Option<String>,
    /// That signature was put in as the account's choice, not by hand: it changes with the
    /// account the message goes from.
    pub signature_auto: bool,
    /// The name of the signature put in, as the signature menu marks it.
    pub signature_name: Option<String>,
}

/// A draft kept in a data file's Drafts folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filed {
    /// The data file by its path, so that another opened in its place since is never touched.
    pub path: String,
    pub store: usize,
    pub folder: FolderId,
    pub item: ItemId,
}

/// A save of a data file that is being done on a thread of its own.
pub struct Saving {
    /// Which data file, by its place in `Session::stores`; no data file is closed meanwhile.
    pub store: usize,
    pub name: String,
    pub progress: std::sync::Arc<openmailclient_model::Progress>,
    /// True when it was started by saving on its own (`Session::autosave`), not asked for.
    pub auto: bool,
    done: std::sync::mpsc::Receiver<Result<openmailclient_model::SaveDone, StoreError>>,
}

impl Saving {
    pub(crate) fn new(
        store: usize,
        name: String,
        progress: std::sync::Arc<openmailclient_model::Progress>,
        done: std::sync::mpsc::Receiver<Result<openmailclient_model::SaveDone, StoreError>>,
    ) -> Self {
        Self { store, name, progress, auto: false, done }
    }
}

impl Drop for Saving {
    fn drop(&mut self) {
        // Nobody is left to end it: it stops, and writes nothing over the data file.
        self.progress.stop();
    }
}

/// An export of a data file that is being done on a thread of its own.
pub struct Exporting {
    /// Which data file, by its place in `Session::stores`; no data file is closed meanwhile.
    pub store: usize,
    /// The file being exported to.
    pub to: std::path::PathBuf,
    /// The file beside it that is written first.
    tmp: std::path::PathBuf,
    pub progress: std::sync::Arc<openmailclient_model::Progress>,
    done: std::sync::mpsc::Receiver<Result<io::Saved, EngineError>>,
}

impl Exporting {
    pub(crate) fn new(
        store: usize,
        to: std::path::PathBuf,
        tmp: std::path::PathBuf,
        progress: std::sync::Arc<openmailclient_model::Progress>,
        done: std::sync::mpsc::Receiver<Result<io::Saved, EngineError>>,
    ) -> Self {
        Self { store, to, tmp, progress, done }
    }
}

impl Drop for Exporting {
    fn drop(&mut self) {
        // Nobody is left to end it: it stops, and what it wrote beside the target goes. It is
        // waited for, so that no file is finished after this has looked.
        self.progress.stop();
        if self.done.recv().is_ok_and(|done| done.is_ok()) {
            let _ = std::fs::remove_file(&self.tmp);
        }
    }
}

/// How long nothing must be done before a data file is saved on its own.
const QUIET_MS: u64 = 5_000;

/// What saving on its own keeps between calls of `Session::autosave`.
#[derive(Default)]
pub(crate) struct AutoSave {
    /// Commands run so far, and as many as the last call had seen: when they differ, something
    /// was just done.
    runs: u64,
    seen: u64,
    /// When something was last done, on the host's clock.
    active_at: u64,
    /// Since when data files have waited to be saved on their own; `None` while none waits.
    due_from: Option<u64>,
    /// Data files saved in this run: once a file has been saved, later saves may be made on
    /// their own.
    pub(crate) saved_here: std::collections::HashSet<std::path::PathBuf>,
}

pub struct Session {
    /// Mail accounts, as the owner set them up; their passwords are in `vault`.
    pub accounts: Vec<accounts::Account>,
    /// Where passwords are kept. Kept in memory until the host gives the system's store.
    pub vault: std::sync::Arc<dyn openmailclient_net::Vault>,
    /// How mail servers are reached; `None` until the host gives a way.
    pub connector: Option<std::sync::Arc<dyn openmailclient_net::Connector>>,
    /// Certificates servers showed in this run that were not trusted: the only ones the owner
    /// may trust (`account.trustCertificate`).
    pub certificates: accounts::Shown,
    /// Work done on a thread of its own, such as talking to a server (`jobs`).
    pub jobs: Vec<jobs::Job>,
    /// The last `Job::id` given.
    pub(crate) job_seq: u64,
    /// What the parts of the receive that runs came to, account by account.
    pub(crate) parts: Vec<cmd::receive::PartsOf>,
    /// What the jobs that ended last came to, the newest last.
    pub finished: Vec<Value>,
    /// The Outbox is to be sent again once the send under way ends: more was put in it.
    pub send_again: bool,
    /// Saves of data files that are being done away from the caller: `file.save` with
    /// `"wait": false` starts them, `Session::poll` ends them.
    pub saves: Vec<Saving>,
    /// Exports that are being done away from the caller, likewise (`file.export`).
    pub exports: Vec<Exporting>,
    pub stores: Vec<Box<dyn Store>>,
    /// Folder lists per store, cached; refreshed after every command that changes a store.
    pub folders: Vec<Vec<FolderInfo>>,
    pub module: Module,
    pub current: Option<(usize, FolderId)>,
    /// The current folder's rows after search, filter and sort: what the message list shows.
    pub rows: Vec<Summary>,
    /// Previews of the current folder's messages read from the messages themselves, by id,
    /// where the folder's table has none, as Outlook's data files have none
    /// ([`Session::fill_previews`]). Each is kept with the message's time and size, so that a
    /// message changed since is read again.
    previews: std::collections::HashMap<u64, Read>,
    /// How dates and times are written, as the system's region settings say. Set by the host.
    pub region: openmailclient_model::time::Region,
    pub selected: Option<u64>,
    pub open: Option<Item>,
    pub sort: SortKey,
    pub sort_desc: bool,
    pub search: String,
    pub unread_only: bool,
    pub reading_pane: ReadingPane,
    pub folder_pane: bool,
    /// Whether pictures that move in a message are played (`view.animations`); else each
    /// stands still on its first frame.
    pub animate: bool,
    /// Pictures on a server, fetched on the reader's word (`mail.downloadPictures`).
    pub downloaded: remote::Downloaded,
    /// Whether the file as it was stays beside a data file the first time it is saved
    /// (`file.keepOriginal`). It does until the owner says otherwise.
    pub keep_original: bool,
    /// Whether a save writes what changed into the data file where it is when that can be
    /// done (`file.saveInPlace`), and not the whole file anew.
    pub save_in_place: bool,
    /// After how many minutes unsaved changes to a data file are saved on their own
    /// (`file.autoSave`); 0 saves only when asked.
    pub autosave_min: u32,
    pub(crate) auto: AutoSave,
    /// Every how many minutes mail is sent and received on its own while the host runs
    /// (`mail.sendReceiveEvery`); 0 only when asked.
    pub send_receive_min: u32,
    pub(crate) timer: arrival::Timer,
    /// Whether each IMAP account's Inbox is watched, so new mail comes as it arrives
    /// (`mail.newMailAsItComes`).
    pub new_mail_as_it_comes: bool,
    /// The Inboxes being watched (`watch`).
    pub(crate) watches: Vec<watch::Watch>,
    /// Accounts whose Inbox changed and that are to be received from, the first first.
    pub(crate) watch_pending: Vec<String>,
    /// Given by the host so that a watch that heard something wakes it.
    pub waker: Option<watch::Waker>,
    /// Messages that came and were not told of or opened yet.
    pub arrived: arrival::Arrivals,
    /// Reminders of appointments, tasks and flagged mail, and how they are shown.
    pub reminders: reminders::Reminders,
    /// Signatures, and those chosen for new messages and for replies (`signatures.*`).
    pub signatures: signatures::Signatures,
    /// Rules, tried in turn on mail as it comes and on Run Rules Now (`rules.*`).
    pub rules: Vec<rules::Rule>,
    /// The names and addresses written to before, suggested first (`people.autoComplete`).
    pub autocomplete: autocomplete::AutoComplete,
    /// Outlook Classic's folder of signatures, read to import them; set by the host.
    pub outlook_signatures: Option<std::path::PathBuf>,
    /// How new mail is told of (`mail.newMailAlerts`).
    pub alerts: arrival::Alerts,
    pub compose: Option<Draft>,
    pub status: String,
    /// The time zone dates are shown in and read in, and written into reply headers: the one
    /// chosen (`calendar.timeZone`), or the system's.
    pub zone: Zone,
    /// The system's zone, as the host finds it ([`Zone::from_system`]).
    pub system_zone: Zone,
    /// The zone chosen in place of the system's, by name; `None` to follow the system.
    pub zone_chosen: Option<String>,
    /// A second zone whose hours the Calendar's day and week views show beside its own
    /// (`calendar.secondTimeZone`).
    pub second_zone: Option<Zone>,
    /// "Now" as UTC FILETIME ticks. Set by the host; tests pin it.
    pub now: i64,
    pub user_name: String,
    pub user_email: String,
    /// What was done to messages and can be taken back, the newest last.
    pub undo: Vec<undo::Step>,
    /// Messages that were being written and were discarded or made way for another, the newest
    /// last: `compose.restore` brings one back.
    pub set_aside: Vec<Draft>,
    /// How pages are set up for printing: `file.pageSetup` changes it.
    pub page_setup: openmailclient_layout::Setup,
    /// What is printed: the open message, or the folder's list. `file.pageSetup` changes it.
    pub print_style: print::Style,
    /// What Table Style prints of a list: its columns, its headings, which rows.
    pub print_list: print::Listing,
    /// How printers are found and printed on; none until the host gives this system's.
    pub spooler: std::sync::Arc<dyn printers::Spooler>,
    /// The printers as `file.printers` last found them.
    pub printers: Vec<printers::Printer>,
    /// The printer chosen to print on; `None` is the system's default.
    pub printer: Option<String>,
    /// A folder of the program's own where the journal of a data file's unsaved changes is
    /// kept when it cannot be kept beside the file. Set by the host; `None` keeps none there.
    pub journal_dir: Option<std::path::PathBuf>,
    /// A folder of the program's own where each account's data file is made. Set by the
    /// host; `None` (tests, a run that keeps no settings) puts an account's mail where new
    /// mail goes.
    pub account_dir: Option<std::path::PathBuf>,
    /// Where the messages of IMAP accounts are on their servers, as read before.
    pub places: cmd::imap_sync::Places,
    /// How Outlook Classic's profiles are read to be imported (`outlook::system_profiles`).
    /// Set by the host; `None` (tests, other systems) reads none.
    pub outlook: Option<std::sync::Arc<dyn Fn() -> Vec<outlook::RawProfile> + Send + Sync>>,
    /// What the Calendar module shows.
    pub calendar: cmd::calendar::Shown,
    /// What the People module shows.
    pub people: cmd::people::Shown,
    /// What the Tasks module shows.
    pub tasks: cmd::tasks::Shown,
    /// The parts of the To-Do Bar each module shows, in `cmd::todo_bar::MODULES` order.
    pub todo_bar: [cmd::todo_bar::Parts; 4],
    /// What the To-Do Bar shows (`Session::todo_bar_due`).
    pub todo: cmd::todo_bar::Shown,
    /// The addresses marked as favourites, in lower case, in the order they were marked.
    pub favorites: Vec<String>,
    /// Where updates of the program stand (`help.checkForUpdates`).
    pub update: update::Updates,
    /// A folder of the program's own where updates are brought down. Set by the host; `None`
    /// brings none down.
    pub update_dir: Option<std::path::PathBuf>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// A preview read from a message, for the row it was read for.
#[derive(Clone, Debug)]
struct Read {
    time: i64,
    size: u32,
    preview: String,
    whole: bool,
}

impl Read {
    /// True when it was read for this row as it is now.
    fn of(&self, row: &Summary) -> bool {
        (self.time, self.size) == (row.time, row.size)
    }
}

impl Session {
    pub fn new() -> Self {
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        Self {
            accounts: Vec::new(),
            vault: std::sync::Arc::new(openmailclient_net::MemoryVault::default()),
            connector: None,
            jobs: Vec::new(),
            job_seq: 0,
            parts: Vec::new(),
            finished: Vec::new(),
            send_again: false,
            saves: Vec::new(),
            exports: Vec::new(),
            stores: Vec::new(),
            folders: Vec::new(),
            module: Module::Mail,
            current: None,
            rows: Vec::new(),
            selected: None,
            open: None,
            sort: SortKey::Date,
            sort_desc: true,
            search: String::new(),
            unread_only: false,
            reading_pane: ReadingPane::Right,
            folder_pane: true,
            animate: true,
            downloaded: remote::Downloaded::default(),
            keep_original: true,
            save_in_place: true,
            autosave_min: 5,
            auto: AutoSave::default(),
            send_receive_min: 30,
            new_mail_as_it_comes: true,
            watches: Vec::new(),
            watch_pending: Vec::new(),
            waker: None,
            timer: arrival::Timer::default(),
            arrived: arrival::Arrivals::default(),
            reminders: reminders::Reminders::default(),
            alerts: arrival::Alerts::default(),
            compose: None,
            status: String::new(),
            zone: Zone::utc(),
            system_zone: Zone::utc(),
            zone_chosen: None,
            second_zone: None,
            region: openmailclient_model::time::Region::default(),
            previews: std::collections::HashMap::new(),
            now: openmailclient_model::time::from_unix(i64::try_from(unix).unwrap_or(0)),
            user_name: "Alex Morgan".to_string(),
            user_email: "alex.morgan@example.org".to_string(),
            undo: Vec::new(),
            set_aside: Vec::new(),
            page_setup: openmailclient_layout::Setup::default(),
            spooler: std::sync::Arc::new(printers::NoPrinters),
            printers: Vec::new(),
            printer: None,
            journal_dir: None,
            account_dir: None,
            outlook: None,
            signatures: signatures::Signatures::default(),
            rules: Vec::new(),
            autocomplete: autocomplete::AutoComplete::default(),
            outlook_signatures: None,
            places: cmd::imap_sync::Places::default(),
            calendar: cmd::calendar::Shown::default(),
            people: cmd::people::Shown::default(),
            tasks: cmd::tasks::Shown::default(),
            todo_bar: [cmd::todo_bar::Parts::default(); 4],
            todo: cmd::todo_bar::Shown::default(),
            favorites: Vec::new(),
            update: update::Updates::new(),
            update_dir: None,
            certificates: accounts::Shown::default(),
            print_style: print::Style::default(),
            print_list: print::Listing::default(),
        }
    }

    /// Reach mail servers over TCP and TLS, checked against the roots this system trusts, and
    /// keep passwords in the system's credential store. The host calls it as it starts; tests
    /// do not, and use the fake server. Says why when servers cannot be reached.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn use_this_system(&mut self) -> Result<(), String> {
        self.vault = std::sync::Arc::new(openmailclient_net::SystemVault);
        self.spooler = std::sync::Arc::new(printers::System);
        let connector = openmailclient_net::TcpConnector::new().map_err(|e| e.to_string())?;
        self.connector = Some(std::sync::Arc::new(connector));
        Ok(())
    }

    /// A session showing the built-in sample mailbox.
    pub fn with_sample() -> Self {
        let mut s = Self::new();
        let store = sample::sample_store(s.now);
        s.add_store(Box::new(store));
        s
    }

    /// Run a command by id. An escaped panic becomes an error (never-crash safety net).
    pub fn run(&mut self, id: &str, params: &Value) -> CmdResult {
        self.auto.runs = self.auto.runs.wrapping_add(1);
        let spec = registry().iter().find(|c| c.id == id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        if !(spec.enabled)(self) {
            return Err(EngineError::Disabled(spec.label.to_string()));
        }
        let run = spec.run;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(self, params)));
        match result {
            Ok(r) => {
                if let Err(e) = &r {
                    self.status = e.to_string();
                }
                r
            }
            Err(_) => {
                // Views are rebuilt from the stores, so a half-finished view change can't linger.
                self.refresh_all();
                let e = EngineError::Internal(id.to_string());
                self.status = e.to_string();
                Err(e)
            }
        }
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        registry().iter().find(|c| c.id == id).is_some_and(|c| (c.enabled)(self))
    }

    /// Add a store and show its Inbox (or first folder) when nothing is shown yet.
    pub fn add_store(&mut self, store: Box<dyn Store>) -> usize {
        self.stores.push(store);
        self.folders.push(Vec::new());
        let idx = self.stores.len() - 1;
        self.refresh_folders(idx);
        if self.current.is_none() {
            self.select_default_folder(idx);
        }
        idx
    }

    pub fn select_default_folder(&mut self, store: usize) {
        let list = self.folders.get(store).cloned().unwrap_or_default();
        let pick = list
            .iter()
            .find(|f| f.kind == FolderKind::Inbox)
            .or_else(|| list.iter().find(|f| f.kind == FolderKind::Mail && f.total > 0))
            .or_else(|| list.first());
        if let Some(f) = pick {
            let id = f.id;
            let _ = self.select_folder(store, id);
        }
    }

    pub fn refresh_folders(&mut self, store: usize) {
        let list = match self.stores.get_mut(store) {
            Some(s) => s.folders().unwrap_or_else(|e| {
                log::warn!("folder list failed: {e}");
                Vec::new()
            }),
            None => return,
        };
        if let Some(slot) = self.folders.get_mut(store) {
            *slot = list;
        }
    }

    pub fn refresh_all(&mut self) {
        for i in 0..self.stores.len() {
            self.refresh_folders(i);
        }
        let _ = self.reload_rows();
    }

    pub fn folder_info(&self, store: usize, folder: FolderId) -> Option<&FolderInfo> {
        self.folders.get(store)?.iter().find(|f| f.id == folder)
    }

    pub fn current_folder(&self) -> Option<&FolderInfo> {
        let (s, f) = self.current?;
        self.folder_info(s, f)
    }

    pub fn current_writable(&self) -> bool {
        self.current.and_then(|(s, _)| self.stores.get(s)).is_some_and(|s| s.writable())
    }

    pub fn find_folder(&self, store: usize, kind: FolderKind) -> Option<FolderId> {
        self.folders.get(store)?.iter().find(|f| f.kind == kind).map(|f| f.id)
    }

    /// "Inbox/Projects"-style path lookup, case-insensitive; a bare name matches anywhere.
    pub fn folder_by_path(&self, store: usize, path: &str) -> Option<FolderId> {
        let list = self.folders.get(store)?;
        let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
        let (last, parents) = parts.split_last()?;
        list.iter()
            .filter(|f| f.name.eq_ignore_ascii_case(last))
            .find(|f| {
                let mut cur = f.parent;
                for want in parents.iter().rev() {
                    match cur.and_then(|p| list.iter().find(|x| x.id == p)) {
                        Some(pf) if pf.name.eq_ignore_ascii_case(want) => cur = pf.parent,
                        _ => return false,
                    }
                }
                true
            })
            .map(|f| f.id)
    }

    pub fn select_folder(&mut self, store: usize, folder: FolderId) -> Result<(), EngineError> {
        if self.folder_info(store, folder).is_none() {
            return Err(StoreError::NoFolder.into());
        }
        self.current = Some((store, folder));
        self.selected = None;
        self.open = None;
        self.search.clear();
        self.previews.clear();
        self.reload_rows()
    }

    /// Give the rows `ids` that have no preview the one their message has: a data file's table
    /// of a folder does not hold the body, so the rows on screen are read a few at a time.
    /// Returns true when any row changed. Each message is read once while the folder is open.
    pub fn fill_previews(&mut self, ids: &[u64]) -> bool {
        /// Messages read at a time, so that a long list scrolled fast stays smooth.
        const AT_A_TIME: usize = 12;
        /// Previews kept: a folder looked through from end to end starts again past this.
        const MAX_KEPT: usize = 20_000;
        let Some((s, f)) = self.current else { return false };
        if self.previews.len() >= MAX_KEPT {
            self.previews.clear();
        }
        let wanted: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| self.rows.iter().any(|r| r.id == *id && r.preview.is_empty() && self.previews.get(id).is_none_or(|p| !p.of(r))))
            .take(AT_A_TIME)
            .collect();
        let Some(store) = self.stores.get_mut(s) else { return false };
        let mut changed = false;
        for id in wanted {
            let (preview, whole) = store.load(f, ItemId(id)).map(|item| item.preview()).unwrap_or_default();
            let Some(row) = self.rows.iter_mut().find(|r| r.id == id) else { continue };
            changed |= !preview.is_empty();
            (row.preview, row.preview_whole) = (preview.clone(), whole);
            self.previews.insert(id, Read { time: row.time, size: row.size, preview, whole });
        }
        changed
    }

    /// Re-read the current folder and apply search, filter and sort.
    pub fn reload_rows(&mut self) -> Result<(), EngineError> {
        let Some((s, f)) = self.current else {
            self.rows.clear();
            return Ok(());
        };
        let Some(store) = self.stores.get_mut(s) else {
            self.rows.clear();
            self.current = None;
            return Ok(());
        };
        let mut rows = match store.list(f) {
            Ok(r) => r,
            Err(e) => {
                self.rows.clear();
                return Err(e.into());
            }
        };
        for row in rows.iter_mut().filter(|r| r.preview.is_empty()) {
            if let Some(read) = self.previews.get(&row.id).filter(|p| p.of(row)) {
                (row.preview, row.preview_whole) = (read.preview.clone(), read.whole);
            }
        }
        let query = search::parse(&self.search, &self.zone);
        if !query.is_empty() {
            rows.retain(|r| query.matches(r));
        }
        if self.unread_only {
            rows.retain(|r| !r.read);
        }
        arrange::sort(&mut rows, self.sort, self.sort_desc);
        self.rows = rows;
        if let Some(sel) = self.selected
            && !self.rows.iter().any(|r| r.id == sel)
        {
            self.selected = None;
            self.open = None;
        }
        Ok(())
    }

    /// The groups the listed rows stand in, as they are arranged.
    pub fn groups(&self) -> Vec<arrange::Group> {
        arrange::groups(&self.rows, self.sort, self.now, &self.zone, self.region.week_start)
    }

    pub fn selected_index(&self) -> Option<usize> {
        let sel = self.selected?;
        self.rows.iter().position(|r| r.id == sel)
    }

    /// Select and load a row. Unread mail is marked read when the store allows it, as the
    /// reading pane does in Outlook.
    pub fn select_item(&mut self, id: u64) -> Result<(), EngineError> {
        let (s, f) = self.current.ok_or(StoreError::NoFolder)?;
        let store = self.stores.get_mut(s).ok_or(StoreError::NoFolder)?;
        let mut item = store.load(f, ItemId(id))?;
        // While its data file is being saved a message is read without being marked read:
        // reading is never held up by a save.
        if !item.is_read() && store.writable() && !store.saving() {
            item.set_read(true);
            store.update(f, ItemId(id), item.clone())?;
            if let Some(r) = self.rows.iter_mut().find(|r| r.id == id) {
                r.read = true;
            }
            self.refresh_folders(s);
        }
        self.selected = Some(id);
        self.open = Some(item);
        Ok(())
    }

    /// Change the selected item in its store and refresh the views.
    /// `what` is the command's label, for taking it back.
    pub fn edit_selected(&mut self, what: &'static str, edit: impl FnOnce(&mut Item)) -> Result<(), EngineError> {
        let (s, f) = self.current.ok_or(StoreError::NoFolder)?;
        let id = self.selected.ok_or(StoreError::NoItem)?;
        let store = self.stores.get_mut(s).ok_or(StoreError::NoFolder)?;
        let mut item = store.load(f, ItemId(id))?;
        let before = Box::new(item.clone());
        edit(&mut item);
        store.update(f, ItemId(id), item.clone())?;
        self.open = Some(item);
        self.did(undo::Step::Edited { what, store: s, folder: f, id, before });
        self.refresh_folders(s);
        self.reload_rows()
    }

    /// Move the selected item to another folder of the same store; selection moves to the next row.
    /// `what` is the command's label, for taking it back.
    pub fn move_selected(&mut self, what: &'static str, to: FolderId) -> Result<(), EngineError> {
        let (s, f) = self.current.ok_or(StoreError::NoFolder)?;
        let id = self.selected.ok_or(StoreError::NoItem)?;
        if to == f {
            return Ok(());
        }
        if self.folder_info(s, to).is_none() {
            return Err(StoreError::NoFolder.into());
        }
        let next = self.selected_index().and_then(|i| self.rows.get(i + 1).or_else(|| i.checked_sub(1).and_then(|p| self.rows.get(p)))).map(|r| r.id);
        let store = self.stores.get_mut(s).ok_or(StoreError::NoFolder)?;
        let item = store.remove(f, ItemId(id))?;
        let moved = match store.add(to, item.clone()) {
            Ok(moved) => moved.0,
            Err(e) => {
                // A message that could not be put where it was going goes back where it was.
                store.add(f, item)?;
                self.refresh_folders(s);
                self.reload_rows()?;
                return Err(e.into());
            }
        };
        self.did(undo::Step::Moved { what, store: s, from: f, was: id, to, id: moved });
        self.selected = None;
        self.open = None;
        self.refresh_folders(s);
        self.reload_rows()?;
        if let Some(n) = next {
            // Keep the unread state of the neighbour: moving on must not mark it read.
            let store = self.stores.get_mut(s).ok_or(StoreError::NoFolder)?;
            if let Ok(item) = store.load(f, ItemId(n)) {
                self.selected = Some(n);
                self.open = Some(item);
            }
        }
        Ok(())
    }

    /// The system's zone is `zone`, as the host found it; dates are shown in it unless another
    /// was chosen.
    pub fn set_system_zone(&mut self, zone: Zone) {
        self.system_zone = zone;
        self.zone = self.zone_chosen.as_deref().and_then(Zone::named).unwrap_or_else(|| self.system_zone.clone());
    }

    /// Dates are shown in the zone known as `name`, or in the system's again with `None`. An
    /// error, changing nothing, when no zone is known by that name.
    pub fn choose_zone(&mut self, name: Option<&str>) -> Result<(), EngineError> {
        match name {
            None => self.zone_chosen = None,
            Some(n) => {
                let zone =
                    Zone::named(n).ok_or_else(|| EngineError::BadParams("that is not a time zone known; calendar.timeZones lists them".into()))?;
                // Kept by the name it is known by, whatever it was asked for by.
                self.zone_chosen = Some(zone.name().to_string());
            }
        }
        self.set_system_zone(self.system_zone.clone());
        Ok(())
    }

    /// The settings kept between runs, as `settings.save` writes them. `ui` is what the
    /// window keeps for itself.
    pub fn settings(&self, ui: Option<&Value>) -> Value {
        cmd::settings::current(self, ui)
    }

    /// True when unsaved changes to a data file are in memory only: not also in a journal
    /// that would bring them back after a run that ends badly.
    pub fn unjournalled(&self) -> bool {
        self.stores.iter().any(|s| s.unsaved() && s.journal_trouble().is_some())
    }

    /// Have the disk keep what the journals of unsaved changes hold so far. The host asks for
    /// it now and then (it has the clock): a journal is otherwise forced to disk every so
    /// many changes only. True when there was something to keep.
    pub fn settle(&mut self) -> bool {
        // Every one of them, not only up to the first that had something.
        let mut any = false;
        for store in &mut self.stores {
            any |= store.settle();
        }
        any
    }

    /// Save on their own the data files whose changes have waited `autosave_min` minutes,
    /// at a moment when nothing has been done for a few seconds; a mailbox in use all along is
    /// saved after three times as long, at the first pause of a second. Each is saved as
    /// `file.save` with `"wait": false` does it, checked and read back, on a thread of its own.
    /// A data file is never saved for the first time on its own: it must have been saved here
    /// once, or have the file as it was kept beside it from a first save of an earlier run. A
    /// save that fails is tried again after as long once more. `now_ms` is the host's clock,
    /// which only goes forward. True when a save was begun.
    pub fn autosave(&mut self, now_ms: u64) -> bool {
        if self.auto.runs != self.auto.seen {
            self.auto.seen = self.auto.runs;
            self.auto.active_at = now_ms;
        }
        let due: Vec<usize> = (0..self.stores.len()).filter(|&i| self.may_save_alone(i)).collect();
        if self.autosave_min == 0 || due.is_empty() {
            self.auto.due_from = None;
            return false;
        }
        let from = *self.auto.due_from.get_or_insert(now_ms);
        let every = u64::from(self.autosave_min).saturating_mul(60_000);
        let (waited, quiet) = (now_ms.saturating_sub(from), now_ms.saturating_sub(self.auto.active_at));
        let ready = waited >= every && (quiet >= QUIET_MS || (waited >= every.saturating_mul(3) && quiet >= 1_000));
        if !ready || !self.saves.is_empty() || !self.exports.is_empty() {
            return false;
        }
        // From now: what is not saved this time waits as long again.
        self.auto.due_from = Some(now_ms);
        let (before, status) = (self.saves.len(), std::mem::take(&mut self.status));
        let (mut begun, mut said) = (false, None);
        for idx in due {
            let name = self.stores.get(idx).map(|st| st.display_name()).unwrap_or_default();
            match self.run("file.save", &json!({ "store": idx, "wait": false })) {
                Ok(done) if done["started"].as_u64().unwrap_or(0) > 0 => begun = true,
                // Where no thread can be had it was saved there and then.
                Ok(done) if done["saved"].as_u64().unwrap_or(0) > 0 => {
                    begun = true;
                    said = Some(format!("Saved {name} on its own"));
                }
                Ok(_) => {}
                Err(e) => said = Some(format!("{name} was not saved on its own: {e}")),
            }
        }
        for saving in self.saves.iter_mut().skip(before) {
            saving.auto = true;
        }
        // Saving on its own is not something the owner did.
        self.auto.seen = self.auto.runs;
        self.status = said.unwrap_or(status);
        begun
    }

    /// True when the data file `idx` has changes that may be saved on their own.
    fn may_save_alone(&self, idx: usize) -> bool {
        let Some(st) = self.stores.get(idx) else { return false };
        st.writable()
            && st.unsaved()
            && !st.saving()
            && st.path().is_some_and(|p| self.auto.saved_here.contains(p) || openmailclient_pst::original_beside(p).exists())
    }

    /// True when a data file has changes that are not in the file yet.
    pub fn unsaved(&self) -> bool {
        self.stores.iter().any(|s| s.unsaved())
    }

    /// End the saves whose work is done: each new file takes its data file's place, or the
    /// status says why not. The host asks every so often while `saves` is not empty. True
    /// when one ended.
    pub fn poll(&mut self) -> bool {
        let saved = self.end_saves(false);
        let jobs = self.end_jobs(false);
        self.end_exports(false) || saved || jobs
    }

    /// Wait for every save and export that is running, and end it. The host asks before it
    /// leaves.
    pub fn finish_saves(&mut self) {
        self.end_saves(true);
        self.end_exports(true);
        self.end_jobs(true);
    }

    fn end_exports(&mut self, wait: bool) -> bool {
        let mut ended = false;
        let mut at = 0;
        while let Some(export) = self.exports.get(at) {
            let lost = || Err(EngineError::Format("the export ended without saying how".to_string()));
            let done = if wait {
                export.done.recv().unwrap_or_else(|_| lost())
            } else {
                match export.done.try_recv() {
                    Ok(done) => done,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        at += 1;
                        continue;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => lost(),
                }
            };
            let export = self.exports.remove(at);
            ended = true;
            let to = export.to.clone();
            // A data file that was opened here meanwhile is not written over.
            let open_here = std::fs::canonicalize(&to)
                .is_ok_and(|real| self.stores.iter().any(|st| st.path().and_then(|p| std::fs::canonicalize(p).ok()).is_some_and(|p| p == real)));
            let done = match done {
                Ok(_) if open_here => {
                    let _ = std::fs::remove_file(&export.tmp);
                    Err(EngineError::BadParams("that data file was opened here meanwhile".into()))
                }
                Ok(saved) => io::export_end(&export.tmp, &to).map(|()| saved),
                // The work tried both ways of writing the mailbox: what it says is how it is.
                Err(e) => Err(e),
            };
            let ok = done.is_ok();
            self.status = match done {
                Ok(saved) => saved.says(&to),
                Err(_) if export.progress.stopped() => format!("The export to {} was stopped; nothing was written there", to.display()),
                Err(e) => format!("Nothing was exported to {}: {e}", to.display()),
            };
            let name = to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let said = self.status.clone();
            self.ended(json!({ "job": format!("Exporting to {name}"), "said": said, "ok": ok, "stopped": export.progress.stopped() }));
        }
        ended
    }

    /// What is being saved or exported on a thread of its own, in words, and how far it is
    /// from 0 to 1; `None` when nothing is.
    pub fn working(&self) -> Option<(String, f32)> {
        if let Some(done) = self.saving() {
            return Some((format!("Saving {}", self.saves.first().map_or("", |s| s.name.as_str())), done));
        }
        if let Some(job) = self.jobs.first().filter(|_| self.exports.is_empty()) {
            return Some((job.what.clone(), job.progress.fraction().clamp(0.0, 1.0)));
        }
        let export = self.exports.first()?;
        let name = export.to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Some((format!("Exporting to {name}"), export.progress.fraction().clamp(0.0, 1.0)))
    }

    fn end_saves(&mut self, wait: bool) -> bool {
        let mut ended = false;
        let mut at = 0;
        while let Some(saving) = self.saves.get(at) {
            let lost = || Err(StoreError::Format("the save ended without saying how; nothing was replaced".to_string()));
            let done = if wait {
                saving.done.recv().unwrap_or_else(|_| lost())
            } else {
                match saving.done.try_recv() {
                    Ok(done) => done,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        at += 1;
                        continue;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => lost(),
                }
            };
            let saving = self.saves.remove(at);
            ended = true;
            let Some(store) = self.stores.get_mut(saving.store) else { continue };
            let alone = if saving.auto { " on its own" } else { "" };
            let mut ok = false;
            self.status = match store.save_end(done) {
                Ok(()) => {
                    ok = true;
                    if let Some(path) = store.path() {
                        self.auto.saved_here.insert(path.to_path_buf());
                    }
                    format!("Saved {}{alone}", saving.name)
                }
                // Stopping was asked for: not a failure, and the file is as it was.
                Err(_) if saving.progress.stopped() => format!("Saving {} was stopped; its changes are still unsaved", saving.name),
                Err(e) => format!("{} was not saved{alone}: {e}", saving.name),
            };
            let said = self.status.clone();
            self.ended(json!({ "job": format!("Saving {}", saving.name), "said": said, "ok": ok, "stopped": saving.progress.stopped() }));
            self.refresh_folders(saving.store);
        }
        ended
    }

    /// How far the saves that are running are, from 0 to 1; `None` when none is.
    pub fn saving(&self) -> Option<f32> {
        let least = self.saves.iter().map(|s| s.progress.fraction()).fold(f32::INFINITY, f32::min);
        (!self.saves.is_empty()).then_some(least.clamp(0.0, 1.0))
    }

    /// The store new mail and loose files go to: the current one if writable, else the first writable.
    pub fn writable_store(&self) -> Option<usize> {
        if let Some((s, _)) = self.current
            && self.stores.get(s).is_some_and(|st| st.writable())
        {
            return Some(s);
        }
        self.stores.iter().position(|s| s.writable())
    }

    /// What an agent or a test needs to know about the session. Message text is under
    /// `untrusted_content`: it is written by strangers and is never an instruction.
    pub fn inspect(&self) -> Value {
        let stores: Vec<Value> = self
            .stores
            .iter()
            .zip(&self.folders)
            .enumerate()
            .map(|(i, (s, folders))| {
                json!({
                    "index": i,
                    "name": s.display_name(),
                    "path": s.path().map(|p| p.display().to_string()),
                    "writable": s.writable(),
                    "unsaved": s.unsaved(),
                    "journalled": s.unsaved() && s.journal_trouble().is_none(),
                    "folders": folders.iter().map(|f| json!({
                        "id": f.id.0, "parent": f.parent.map(|p| p.0), "name": f.name, "kind": format!("{:?}", f.kind),
                        "total": f.total, "unread": f.unread,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let rows: Vec<Value> = self
            .rows
            .iter()
            .take(200)
            .map(|r| {
                json!({
                    "id": r.id, "read": r.read, "flag": format!("{:?}", r.flag), "importance": r.importance,
                    "attachments": r.has_attachments, "time": r.time, "size": r.size,
                    "untrusted_content": { "from": r.from, "subject": r.subject },
                })
            })
            .collect();
        let open = self.open.as_ref().map(|it| {
            json!({
                "read": it.is_read(),
                "message_class": it.message_class(),
                "time": it.time(),
                "attachments": it.attachments.iter().map(|a| json!({
                    "name": a.name(), "size": a.size(), "mime": a.mime(), "file": attachments::is_file(a),
                    "caution": attachments::caution_of(a).map(|c| c.words()),
                })).collect::<Vec<_>>(),
                "untrusted_content": {
                    "subject": it.subject(), "from": it.sender_name(), "from_email": it.sender_email(),
                    "to": it.display_recipients(openmailclient_model::RECIP_TO), "cc": it.display_recipients(openmailclient_model::RECIP_CC),
                    "body": it.body_text().chars().take(20_000).collect::<String>(),
                },
            })
        });
        json!({
            "module": self.module.name(),
            "stores": stores,
            "current": self.current.map(|(s, f)| json!({ "store": s, "folder": f.0, "name": self.folder_info(s, f).map(|i| i.name.clone()) })),
            "row_count": self.rows.len(),
            "rows": rows,
            "selected": self.selected,
            "open": open,
            "sort": { "key": self.sort.name(), "descending": self.sort_desc },
            "groups": self.groups().iter().map(|g| json!({
                "start": g.start, "count": g.count, "unread": g.unread, "untrusted_content": { "label": g.label },
            })).collect::<Vec<_>>(),
            "search": self.search,
            "unread_only": self.unread_only,
            "undo": self.undo.last().map(|step| step.what()),
            "setAside": self.set_aside.len(),
            "composing": self.compose.as_ref().map(|d| json!({
                "kind": d.kind, "to": d.to, "cc": d.cc, "subject": d.subject, "attachments": d.attachments.len(),
                "cautions": outgoing::cautions(d).iter().map(|c| c.words()).collect::<Vec<_>>(),
            })),
            "accounts": self.accounts.iter().map(accounts::Account::to_json).collect::<Vec<_>>(),
            "jobs": { "running": self.jobs.iter().map(|j| j.what.clone()).collect::<Vec<_>>(), "finished": self.finished },
            "arrived": self.arrived.list.iter().map(arrival::Arrived::to_json).collect::<Vec<_>>(),
            "status": self.status,
        })
    }
}

/// The time zone this machine keeps; hosts give it to [`Session::set_system_zone`]. Its rules
/// are found from the offsets the system keeps through this year, and it is known by the name
/// the system gives it.
pub fn local_zone() -> openmailclient_model::zone::Zone {
    use chrono::TimeZone;
    use openmailclient_model::{time, zone::Zone};
    let now = time::from_unix(chrono::Utc::now().timestamp());
    let offset_at = |t: i64| chrono::Local.timestamp_opt(time::to_unix(t), 0).single().map_or(0, |d| d.offset().local_minus_utc() / 60);
    let found = Zone::from_system(now, offset_at);
    match system_zone_name() {
        // One known by that name, where it keeps the rules the system does.
        Some(name) => match Zone::named(&name) {
            // Its rules this year are what the system says; those of other years, the tz database's.
            Some(known) if known.bias == found.bias && known.same_in(&found, time::civil(now, found.offset_at(now)).year) => known,
            // Else what the system keeps, by the name it gives it.
            _ => Zone { id: name, ..found },
        },
        None => found,
    }
}

/// The tz database name the system gives its zone, when it is a plain one.
#[cfg(not(target_arch = "wasm32"))]
fn system_zone_name() -> Option<String> {
    let name = iana_time_zone::get_timezone().ok()?;
    let plain = !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'+'));
    plain.then_some(name)
}

#[cfg(target_arch = "wasm32")]
fn system_zone_name() -> Option<String> {
    None
}
