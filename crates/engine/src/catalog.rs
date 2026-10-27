//! The Outlook Classic feature catalog: what its ribbon and menus offer, module by module, and
//! which OpenMailClient command covers each one. Parity is the share of entries whose command exists.
//!
//! Entries are feature names as a person sees them in the product; nothing else is taken from it.

use crate::cmd::registry;

pub struct Feature {
    pub module: &'static str,
    pub tab: &'static str,
    pub group: &'static str,
    pub name: &'static str,
    /// The command that does it; empty when there is none yet.
    pub command: &'static str,
}

const fn f(module: &'static str, tab: &'static str, group: &'static str, name: &'static str, command: &'static str) -> Feature {
    Feature { module, tab, group, name, command }
}

pub const FEATURES: &[Feature] = &[
    // Mail: Home
    f("Mail", "Home", "New", "New Email", "mail.new"),
    f("Mail", "Home", "New", "New Items", ""),
    f("Mail", "Home", "Delete", "Ignore", ""),
    f("Mail", "Home", "Delete", "Clean Up", ""),
    f("Mail", "Home", "Delete", "Junk", "mail.junk"),
    f("Mail", "Home", "Delete", "Delete", "mail.delete"),
    f("Mail", "Home", "Delete", "Archive", "mail.archive"),
    f("Mail", "Home", "Respond", "Reply", "mail.reply"),
    f("Mail", "Home", "Respond", "Reply All", "mail.replyAll"),
    f("Mail", "Home", "Respond", "Forward", "mail.forward"),
    f("Mail", "Home", "Respond", "Meeting", "mail.replyWithMeeting"),
    f("Mail", "Home", "Respond", "Forward as Attachment", ""),
    f("Mail", "Home", "Quick Steps", "Quick Steps", ""),
    f("Mail", "Home", "Move", "Move", "mail.move"),
    f("Mail", "Home", "Move", "Rules", ""),
    f("Mail", "Home", "Tags", "Unread/ Read", "mail.toggleRead"),
    f("Mail", "Home", "Tags", "Categorize", "mail.categorize"),
    f("Mail", "Home", "Tags", "Follow Up", "mail.flag"),
    f("Mail", "Home", "Tags", "Mark Complete", "mail.flagComplete"),
    f("Mail", "Home", "Tags", "Clear Flag", "mail.clearFlag"),
    f("Mail", "Home", "Find", "Search People", "people.search"),
    f("Mail", "Home", "Find", "Address Book", ""),
    f("Mail", "Home", "Find", "Filter Email: Unread", "view.filterUnread"),
    f("Mail", "Home", "Find", "Filter Email: Has Attachments", ""),
    f("Mail", "Home", "Find", "Filter Email: Flagged", ""),
    f("Mail", "Home", "Speech", "Read Aloud", ""),
    // Mail: Send / Receive
    f("Mail", "Send / Receive", "Send & Receive", "Send/Receive All Folders", "mail.sendReceive"),
    f("Mail", "Send / Receive", "Send & Receive", "Send/Receive Groups", "mail.sendReceiveEvery"),
    f("Mail", "Send / Receive", "Send & Receive", "Update Folder", ""),
    f("Mail", "Send / Receive", "Send & Receive", "Send All", "mail.sendAll"),
    f("Mail", "Send / Receive", "Download", "Show Progress", "mail.showProgress"),
    f("Mail", "Send / Receive", "Preferences", "Work Offline", ""),
    // Mail: Folder
    f("Mail", "Folder", "New", "New Folder", "folder.new"),
    f("Mail", "Folder", "New", "New Search Folder", ""),
    f("Mail", "Folder", "Actions", "Rename Folder", "folder.rename"),
    f("Mail", "Folder", "Actions", "Copy Folder", ""),
    f("Mail", "Folder", "Actions", "Move Folder", "folder.move"),
    f("Mail", "Folder", "Actions", "Delete Folder", "folder.delete"),
    f("Mail", "Folder", "Clean Up", "Mark All as Read", "folder.markAllRead"),
    f("Mail", "Folder", "Clean Up", "Run Rules Now", ""),
    f("Mail", "Folder", "Clean Up", "Clean Up Folder", ""),
    f("Mail", "Folder", "Clean Up", "Empty Folder", "folder.empty"),
    f("Mail", "Folder", "Clean Up", "Recover Deleted Items", ""),
    f("Mail", "Folder", "Favorites", "Add to Favorites", ""),
    f("Mail", "Folder", "AutoArchive", "AutoArchive Settings", ""),
    f("Mail", "Folder", "Properties", "Folder Properties", "folder.properties"),
    // Mail: View
    f("Mail", "View", "Current View", "Change View", ""),
    f("Mail", "View", "Current View", "View Settings", ""),
    f("Mail", "View", "Current View", "Reset View", ""),
    f("Mail", "View", "Messages", "Show as Conversations", ""),
    f("Mail", "View", "Arrangement", "Arrange By", "view.sortBy"),
    f("Mail", "View", "Arrangement", "Reverse Sort", "view.reverseSort"),
    f("Mail", "View", "Arrangement", "Add Columns", ""),
    f("Mail", "View", "Arrangement", "Message Preview", ""),
    f("Mail", "View", "Layout", "Folder Pane", "view.folderPane"),
    f("Mail", "View", "Layout", "Reading Pane", "view.readingPane"),
    f("Mail", "View", "Layout", "To-Do Bar", ""),
    f("Mail", "View", "Window", "Reminders Window", "view.reminders"),
    f("Mail", "View", "Window", "Open in New Window", ""),
    // Mail: search and navigation
    f("Mail", "Search", "", "Search Current Folder", "search.query"),
    f("Mail", "Search", "Scope", "Search All Mailboxes", ""),
    f("Mail", "Search", "Refine", "Refine by From, Subject, Attachments", ""),
    f("Mail", "Search", "Close", "Close Search", "search.clear"),
    f("Mail", "Quick Access", "", "Undo", "edit.undo"),
    f("Mail", "Navigation", "", "Go to Folder", "folder.select"),
    f("Mail", "Navigation", "", "Switch Module", "nav.module"),
    f("Mail", "Navigation", "", "Next Item", "mail.next"),
    f("Mail", "Navigation", "", "Previous Item", "mail.previous"),
    // File (backstage)
    f("File", "Info", "", "Account Settings", "account.list"),
    f("File", "Info", "", "Add Account", "account.add"),
    f("File", "Info", "", "Automatic Replies", ""),
    f("File", "Info", "", "Mailbox Settings: Tools", ""),
    f("File", "Info", "", "Manage Rules & Alerts", ""),
    f("File", "Open & Export", "", "Open Outlook Data File", "file.open"),
    f("File", "Open & Export", "", "Open Calendar", "calendar.import"),
    f("File", "Open & Export", "", "Import/Export", "file.export"),
    f("File", "Save", "", "Save", "file.save"),
    f("File", "Save As", "", "Save As", "file.saveAs"),
    f("File", "Save Attachments", "", "Save Attachments", "file.saveAllAttachments"),
    f("File", "Print", "", "Print", "file.print"),
    f("File", "Print", "", "Page Setup", "file.pageSetup"),
    f("File", "Print", "", "Printer", "file.printers"),
    f("File", "Options", "", "Options", ""),
    f("File", "Info", "", "Close Data File", "file.closeStore"),
    // Message window
    f("Message", "Message", "", "Send", "compose.send"),
    f("Message", "Message", "", "Save", "compose.saveDraft"),
    f("Message", "Message", "Clipboard", "Paste", ""),
    f("Message", "Message", "Basic Text", "Font, Bold, Italic, Underline", ""),
    f("Message", "Message", "Basic Text", "Bullets and Numbering", ""),
    f("Message", "Message", "Names", "Address Book", ""),
    f("Message", "Message", "Names", "Check Names", ""),
    f("Message", "Message", "Include", "Attach File", "compose.attach"),
    f("Message", "Message", "Include", "Signature", ""),
    f("Message", "Message", "Tags", "High Importance", ""),
    f("Message", "Message", "Tags", "Low Importance", ""),
    f("Message", "Insert", "", "Pictures, Tables, Links", ""),
    f("Message", "Options", "Show Fields", "Bcc", "compose.showBcc"),
    f("Message", "Options", "Tracking", "Request a Delivery or Read Receipt", ""),
    f("Message", "Options", "More Options", "Delay Delivery", "compose.delayDelivery"),
    f("Message", "Options", "More Options", "Expires After", "compose.expires"),
    f("Message", "Format Text", "Format", "HTML, Plain Text, Rich Text", ""),
    f("Message", "Review", "Proofing", "Spelling & Grammar", ""),
    // Calendar
    f("Calendar", "Home", "New", "New Appointment", "calendar.new"),
    f("Calendar", "Home", "New", "New Meeting", "calendar.newMeeting"),
    f("Calendar", "Home", "Go To", "Today", "calendar.today"),
    f("Calendar", "Home", "Arrange", "Day, Work Week, Week, Month", "calendar.view"),
    f("Calendar", "Home", "Arrange", "Schedule View", ""),
    f("Calendar", "Home", "Manage Calendars", "Open Calendar", "calendar.import"),
    f("Calendar", "Home", "Share", "E-mail Calendar", "calendar.email"),
    f("Calendar", "File", "Save As", "Save Calendar", "calendar.saveAs"),
    f("Calendar", "Appointment", "Options", "Recurrence", "calendar.recurrence"),
    f("Calendar", "Appointment", "Options", "Reminder", "calendar.change"),
    f("Calendar", "Appointment", "Options", "Time Zones", "calendar.change"),
    f("Calendar", "File", "Options", "Time Zone", "calendar.timeZone"),
    f("Calendar", "File", "Options", "Show a Second Time Zone", "calendar.secondTimeZone"),
    f("Calendar", "Meeting", "Respond", "Accept, Tentative, Decline", "calendar.respond"),
    f("Calendar", "Meeting", "Attendees", "Invite Attendees", "calendar.change"),
    f("Calendar", "Meeting", "Actions", "Send, Send Update", "calendar.sendInvitations"),
    f("Calendar", "Meeting", "Actions", "Cancel Meeting", "calendar.cancelMeeting"),
    f("Calendar", "Meeting", "Show", "Tracking", "calendar.attendees"),
    f("Calendar", "Meeting", "Show", "Scheduling Assistant", ""),
    // People
    f("People", "Home", "New", "New Contact", "people.new"),
    f("People", "Home", "New", "New Contact Group", "people.newGroup"),
    f("People", "Home", "Current View", "People, Business Card, Card, Phone, List", "people.view"),
    f("People", "Home", "Actions", "Mail Merge", ""),
    f("People", "Home", "Share", "Forward Contact", "people.forward"),
    // Tasks
    f("Tasks", "Home", "New", "New Task", "tasks.new"),
    f("Tasks", "Home", "Delete", "Delete", "tasks.delete"),
    f("Tasks", "Home", "Manage Task", "Mark Complete", "tasks.markComplete"),
    f("Tasks", "Home", "Manage Task", "Remove from List", "tasks.removeFromList"),
    f("Tasks", "Home", "Follow Up", "Today, Tomorrow, This Week", "tasks.followUp"),
    f("Tasks", "Home", "Current View", "To-Do List, Detailed, Simple List", "tasks.view"),
    f("Tasks", "Home", "Find", "Search Tasks", "tasks.search"),
    f("Tasks", "Task", "Manage Task", "Assign Task", ""),
];

impl Feature {
    pub fn implemented(&self) -> bool {
        !self.command.is_empty() && registry().iter().any(|c| c.id == self.command)
    }
}

/// (features with a working command, all features).
pub fn summary() -> (usize, usize) {
    (FEATURES.iter().filter(|f| f.implemented()).count(), FEATURES.len())
}

/// `docs/parity-checklist.md`.
pub fn checklist() -> String {
    let (done, total) = summary();
    let mut out = format!(
        "# Outlook Classic parity checklist\n\nGenerated by `cargo xtask parity` from `crates/engine/src/catalog.rs`. Do not edit by hand.\n\n**{done} of {total} catalogued features have a command** ({}%).\n",
        done * 100 / total.max(1)
    );
    let mut section = ("", "");
    for f in FEATURES {
        if (f.module, f.tab) != section {
            section = (f.module, f.tab);
            out.push_str(&format!("\n## {} — {}\n\n", f.module, f.tab));
        }
        let mark = if f.implemented() { "x" } else { " " };
        let group = if f.group.is_empty() { String::new() } else { format!("{}: ", f.group) };
        let cmd = if f.command.is_empty() { String::new() } else { format!(" — `{}`", f.command) };
        out.push_str(&format!("- [{mark}] {group}{}{cmd}\n", f.name));
    }
    out
}
