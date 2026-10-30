//! The ribbon, its drop-down menus, the name prompt and the File page.

use crate::icons::{self, Icon};
use crate::{MailUi, Menu, Tab, bold, clip_text, font, text_width, theme};
use egui::{Align2, Color32, Rect, Sense, Ui, pos2, vec2};
use openmailclient_model::{FolderId, FolderInfo, FolderKind};
use serde_json::{Value, json};

struct Btn {
    icon: Icon,
    label: &'static str,
    /// A command id, `menu:<name>` for a drop-down, or "" for a feature that is not built yet.
    cmd: &'static str,
    params: &'static str,
    large: bool,
}

const fn big(icon: Icon, label: &'static str, cmd: &'static str) -> Btn {
    Btn { icon, label, cmd, params: "", large: true }
}

const fn small(icon: Icon, label: &'static str, cmd: &'static str) -> Btn {
    Btn { icon, label, cmd, params: "", large: false }
}

const fn with(mut b: Btn, params: &'static str) -> Btn {
    b.params = params;
    b
}

type Group = (&'static str, &'static [Btn]);

const HOME: &[Group] = &[
    ("New", &[big(Icon::NewMail, "New\nEmail", "mail.new")]),
    (
        "Delete",
        &[big(Icon::Delete, "Delete", "mail.delete"), small(Icon::Archive, "Archive", "mail.archive"), small(Icon::Junk, "Junk", "mail.junk")],
    ),
    (
        "Respond",
        &[
            big(Icon::Reply, "Reply", "mail.reply"),
            big(Icon::ReplyAll, "Reply\nAll", "mail.replyAll"),
            big(Icon::Forward, "Forward", "mail.forward"),
            small(Icon::NewMeeting, "Meeting", "ui.replyWithMeeting"),
        ],
    ),
    ("Move", &[small(Icon::Move, "Move", "menu:move"), small(Icon::Rules, "Rules", "menu:rules")]),
    (
        "Tags",
        &[
            small(Icon::MailOpen, "Unread/ Read", "mail.toggleRead"),
            small(Icon::Category, "Categorize", "menu:categorize"),
            small(Icon::Flag, "Follow Up", "menu:followup"),
        ],
    ),
    ("Find", &[small(Icon::Filter, "Filter Unread", "view.filterUnread"), small(Icon::People, "Address Book", "people.addressBook")]),
];

const CALENDAR_HOME: &[Group] = &[
    ("New", &[big(Icon::NewAppointment, "New\nAppointment", "ui.appointment"), big(Icon::NewMeeting, "New\nMeeting", "ui.meeting")]),
    ("Go To", &[big(Icon::Today, "Today", "calendar.today")]),
    ("Time Zones", &[big(Icon::Clock, "Time\nZones", "ui.timeZones")]),
    (
        "Arrange",
        &[
            with(big(Icon::DayView, "Day", "calendar.view"), r#"{"view":"day"}"#),
            with(big(Icon::WorkWeekView, "Work\nWeek", "calendar.view"), r#"{"view":"workWeek"}"#),
            with(big(Icon::WeekView, "Week", "calendar.view"), r#"{"view":"week"}"#),
            with(big(Icon::MonthView, "Month", "calendar.view"), r#"{"view":"month"}"#),
        ],
    ),
    ("Manage Calendars", &[big(Icon::Calendar, "Open\nCalendar", "ui.openCalendarDialog")]),
    ("Share", &[big(Icon::Mail, "E-mail\nCalendar", "calendar.email"), small(Icon::Save, "Save Calendar", "ui.saveCalendarDialog")]),
    ("Actions", &[small(Icon::Open, "Open", "ui.openAppointment"), small(Icon::Delete, "Delete", "calendar.delete")]),
];

const PEOPLE_HOME: &[Group] = &[
    (
        "New",
        &[
            big(
                Icon::People,
                "New
Contact",
                "ui.contact",
            ),
            big(
                Icon::People,
                "New Contact
Group",
                "ui.contactGroup",
            ),
        ],
    ),
    ("Delete", &[big(Icon::Delete, "Delete", "people.delete")]),
    ("Communicate", &[big(Icon::Mail, "E-mail", "people.email")]),
    (
        "Current View",
        &[
            with(big(Icon::People, "People", "people.view"), r#"{"view":"people"}"#),
            with(
                big(
                    Icon::Page,
                    "Business
Card",
                    "people.view",
                ),
                r#"{"view":"businessCard"}"#,
            ),
            with(big(Icon::Panes, "Card", "people.view"), r#"{"view":"card"}"#),
            with(big(Icon::Sort, "Phone", "people.view"), r#"{"view":"phone"}"#),
            with(big(Icon::Sidebar, "List", "people.view"), r#"{"view":"list"}"#),
        ],
    ),
    (
        "Share",
        &[
            big(
                Icon::Forward,
                "Forward
Contact",
                "people.forward",
            ),
            small(Icon::Save, "Save as vCard", "ui.saveCardsDialog"),
        ],
    ),
    ("Find", &[small(Icon::Search, "Search People", "ui.searchPeople"), small(Icon::Import, "Open vCard", "ui.openCardsDialog")]),
    ("Actions", &[small(Icon::Open, "Open", "ui.openContact"), small(Icon::Mail, "Mail Merge", "ui.mailMerge")]),
];

const TASKS_HOME: &[Group] = &[
    (
        "New",
        &[big(
            Icon::Tasks,
            "New
Task",
            "ui.task",
        )],
    ),
    ("Delete", &[big(Icon::Delete, "Delete", "tasks.delete")]),
    (
        "Manage Task",
        &[
            big(
                Icon::Check,
                "Mark
Complete",
                "tasks.markComplete",
            ),
            big(
                Icon::Close,
                "Remove
from List",
                "tasks.removeFromList",
            ),
        ],
    ),
    (
        "Follow Up",
        &[
            with(small(Icon::Flag, "Today", "tasks.followUp"), r#"{"when":"today"}"#),
            with(small(Icon::Flag, "Tomorrow", "tasks.followUp"), r#"{"when":"tomorrow"}"#),
            with(small(Icon::Flag, "This Week", "tasks.followUp"), r#"{"when":"thisWeek"}"#),
            with(small(Icon::Flag, "Next Week", "tasks.followUp"), r#"{"when":"nextWeek"}"#),
            with(small(Icon::Flag, "No Date", "tasks.followUp"), r#"{"when":"noDate"}"#),
        ],
    ),
    (
        "Current View",
        &[
            with(
                big(
                    Icon::Flag,
                    "To-Do
List",
                    "tasks.view",
                ),
                r#"{"view":"toDoList"}"#,
            ),
            with(
                big(
                    Icon::Sidebar,
                    "Simple
List",
                    "tasks.view",
                ),
                r#"{"view":"simpleList"}"#,
            ),
            with(big(Icon::Panes, "Detailed", "tasks.view"), r#"{"view":"detailed"}"#),
            with(big(Icon::Importance, "Prioritized", "tasks.view"), r#"{"view":"prioritized"}"#),
            with(small(Icon::Tasks, "Active", "tasks.view"), r#"{"view":"active"}"#),
            with(small(Icon::Check, "Completed", "tasks.view"), r#"{"view":"completed"}"#),
            with(small(Icon::Today, "Today", "tasks.view"), r#"{"view":"today"}"#),
            with(small(Icon::WeekView, "Next 7 Days", "tasks.view"), r#"{"view":"next7Days"}"#),
            with(small(Icon::Clock, "Overdue", "tasks.view"), r#"{"view":"overdue"}"#),
        ],
    ),
    ("Find", &[small(Icon::Search, "Search Tasks", "ui.searchTasks")]),
    ("Actions", &[small(Icon::Open, "Open", "ui.openTask")]),
];

const SEND_RECEIVE: &[Group] = &[
    (
        "Send & Receive",
        &[
            with(big(Icon::Sync, "Send/Receive\nAll Folders", "mail.sendReceive"), r#"{"wait":false}"#),
            small(Icon::Sync, "Update Folder", ""),
            small(Icon::Sort, "Send/Receive Groups", "menu:groups"),
            with(small(Icon::Send, "Send All", "mail.sendAll"), r#"{"wait":false}"#),
            small(Icon::People, "Account Settings", "ui.accountSettings"),
        ],
    ),
    ("Download", &[small(Icon::Sync, "Show Progress", "ui.showProgress")]),
];

const FOLDER: &[Group] = &[
    ("New", &[with(big(Icon::NewFolder, "New\nFolder", "ui.prompt"), r#"{"command":"folder.new"}"#)]),
    (
        "Actions",
        &[
            with(small(Icon::Rename, "Rename Folder", "ui.prompt"), r#"{"command":"folder.rename"}"#),
            small(Icon::Delete, "Delete Folder", "folder.delete"),
        ],
    ),
    ("Clean Up", &[small(Icon::MailOpen, "Mark All as Read", "folder.markAllRead"), small(Icon::Broom, "Empty Folder", "folder.empty")]),
    ("Properties", &[big(Icon::Info, "Folder\nProperties", "folder.properties")]),
];

const VIEW: &[Group] = &[
    ("Arrangement", &[small(Icon::Sort, "Arrange By", "menu:arrange"), small(Icon::Swap, "Reverse Sort", "view.reverseSort")]),
    (
        "Layout",
        &[
            small(Icon::Sidebar, "Folder Pane", "view.folderPane"),
            small(Icon::Panes, "Reading Pane", "menu:reading"),
            small(Icon::Tasks, "To-Do Bar", "menu:todobar"),
        ],
    ),
    ("Message", &[small(Icon::Info, "Properties", "mail.properties"), small(Icon::MailOpen, "Plain Text", "ui.bodyFormat")]),
    (
        "Window",
        &[
            small(Icon::Panes, "Simplified Ribbon", "ui.ribbonMode"),
            small(Icon::Swap, "Dark Mode", "ui.theme"),
            small(Icon::Palette, "Themes", "ui.themes"),
        ],
    ),
];

/// Commands the single-line ribbon shows as an icon alone; the label is the hover text.
const ICON_ONLY: &[&str] = &[
    "mail.delete",
    "mail.archive",
    "mail.junk",
    "mail.reply",
    "mail.replyAll",
    "mail.forward",
    "menu:categorize",
    "menu:followup",
    "view.reverseSort",
];

const HELP: &[Group] = &[("Help", &[big(Icon::Info, "About\nOpenMailClient", "ui.about"), big(Icon::Update, "Check for\nUpdates", "ui.updates")])];

fn menu_of(cmd: &str) -> Option<&str> {
    cmd.strip_prefix("menu:")
}

struct MenuItem {
    label: String,
    cmd: &'static str,
    params: Value,
    swatch: Option<Color32>,
    checked: bool,
}

fn item(label: &str, cmd: &'static str, params: Value) -> MenuItem {
    MenuItem { label: label.to_string(), cmd, params, swatch: None, checked: false }
}

impl MailUi {
    fn available(&self, cmd: &str) -> bool {
        match cmd {
            "" => false,
            "ui.prompt" => true,
            "ui.openAppointment" => self.session.calendar.selected.is_some(),
            "ui.appointment" => self.session.is_enabled("calendar.new"),
            "ui.meeting" => self.session.is_enabled("calendar.newMeeting"),
            "ui.replyWithMeeting" => self.session.is_enabled("mail.replyWithMeeting"),
            "ui.saveCalendarDialog" => self.session.is_enabled("calendar.saveAs"),
            "ui.contact" => self.session.is_enabled("people.new"),
            "ui.contactGroup" => self.session.is_enabled("people.newGroup"),
            "ui.mailMerge" => self.session.is_enabled("people.mailMerge"),
            "ui.openContact" | "ui.saveCardsDialog" => self.session.people.selected.is_some(),
            "ui.task" => self.session.is_enabled("tasks.new"),
            "ui.openTask" => self.session.tasks.selected.is_some(),
            c if c.starts_with("ui.") => true,
            c => match menu_of(c) {
                Some("categorize" | "followup") => self.session.is_enabled("mail.flag"),
                Some("move") => self.session.is_enabled("mail.move"),
                Some("arrange") => self.session.is_enabled("view.sortBy"),
                Some(_) => true,
                None => self.session.is_enabled(c),
            },
        }
    }

    /// A button that stands for what is shown now, such as the Calendar's view.
    fn checked(&self, b: &Btn) -> bool {
        match b.cmd {
            "calendar.view" => b.params.contains(&format!("\"{}\"", self.session.calendar.view.name())),
            "people.view" => b.params.contains(&format!("\"{}\"", self.session.people.view.name())),
            "tasks.view" => b.params.contains(&format!("\"{}\"", self.session.tasks.view.name())),
            _ => false,
        }
    }

    fn press(&mut self, b: &Btn, below: egui::Pos2) {
        if b.cmd == "ui.openAppointment" {
            if let Some(id) = self.session.calendar.selected {
                self.act("ui.appointment", json!({ "id": id }));
            }
            return;
        }
        if b.cmd == "ui.openTask" {
            self.act("ui.task", json!({ "open": true }));
            return;
        }
        if b.cmd == "ui.openContact" {
            if let Some(id) = self.session.people.selected {
                self.act("ui.contact", json!({ "id": id }));
            }
            return;
        }
        if let Some(menu) = menu_of(b.cmd) {
            self.act("ui.menu", json!({ "menu": menu, "x": below.x, "y": below.y }));
        } else {
            self.act(b.cmd, serde_json::from_str(b.params).unwrap_or(Value::Null));
        }
    }

    fn groups(&self) -> &'static [Group] {
        match self.tab {
            Tab::Home if self.session.module == openmailclient_engine::Module::Calendar => CALENDAR_HOME,
            Tab::Home if self.session.module == openmailclient_engine::Module::People => PEOPLE_HOME,
            Tab::Home if self.session.module == openmailclient_engine::Module::Tasks => TASKS_HOME,
            Tab::Home => HOME,
            Tab::SendReceive => SEND_RECEIVE,
            Tab::Folder => FOLDER,
            Tab::View => VIEW,
            Tab::Help | Tab::File => HELP,
        }
    }

    /// The single-line ribbon: one rounded panel, every command of the tab in a row.
    pub(crate) fn ribbon_simple(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect.expand(1.0));
        p.rect_filled(rect, 6.0, t.surface);
        p.rect_stroke(rect, 6.0, (1.0, t.line), egui::StrokeKind::Inside);
        let mut x = rect.left() + 8.0;
        let (top, h) = (rect.center().y - 16.0, 32.0);
        'groups: for (n, (name, buttons)) in self.groups().iter().enumerate() {
            if n > 0 {
                p.vline(x + 3.0, (rect.top() + 10.0)..=(rect.bottom() - 10.0), (1.0, t.line));
                x += 8.0;
            }
            for b in *buttons {
                let label = b.label.replace('\n', " ");
                let icon_only = ICON_ONLY.contains(&b.cmd);
                let menu = menu_of(b.cmd).is_some();
                let text_w = if icon_only { 0.0 } else { text_width(&p, &label, font(12.5)) + 8.0 };
                let w = 34.0 + text_w + if menu { 12.0 } else { 0.0 };
                if x + w > rect.right() - 8.0 {
                    break 'groups;
                }
                let r = Rect::from_min_size(pos2(x, top), vec2(w, h));
                let on = self.available(b.cmd);
                let resp = ui.interact(r, egui::Id::new(("ribbon1", *name, b.label)), if on { Sense::click() } else { Sense::hover() });
                if self.checked(b) {
                    p.rect_filled(r, 4.0, t.selected);
                }
                if on && resp.hovered() {
                    p.rect_filled(r, 4.0, t.hover);
                }
                let tint = if on { t.accent } else { t.disabled };
                icons::paint(&p, Rect::from_center_size(pos2(r.left() + 17.0, r.center().y), vec2(18.0, 18.0)), b.icon, tint);
                if !icon_only {
                    p.text(pos2(r.left() + 32.0, r.center().y), Align2::LEFT_CENTER, &label, font(12.5), if on { t.text } else { t.disabled });
                }
                if menu {
                    let c = if on { t.text_dim } else { t.disabled };
                    icons::paint(&p, Rect::from_center_size(pos2(r.right() - 9.0, r.center().y), vec2(8.0, 8.0)), Icon::ChevronDown, c);
                }
                if on && resp.clicked() {
                    self.press(b, pos2(r.left(), rect.bottom()));
                }
                resp.on_hover_text(if b.cmd.is_empty() { format!("{label} (not built yet)") } else { label });
                x += w + 2.0;
            }
        }
    }

    pub(crate) fn ribbon(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect.expand(1.0));
        p.rect_filled(rect, 6.0, t.surface);
        p.rect_stroke(rect, 6.0, (1.0, t.line), egui::StrokeKind::Inside);
        let groups = self.groups();
        let top = rect.top() + 5.0;
        let label_y = rect.bottom() - 11.0;
        let mut x = rect.left() + 8.0;
        for (name, buttons) in groups {
            let start = x;
            let mut i = 0;
            while let Some(b) = buttons.get(i) {
                if b.large {
                    let lines: Vec<&str> = b.label.split('\n').collect();
                    let w = lines.iter().map(|l| text_width(&p, l, font(11.5))).fold(40.0, f32::max) + 12.0;
                    let r = Rect::from_min_size(pos2(x, top), vec2(w, 66.0));
                    let on = self.available(b.cmd);
                    let resp = ui.interact(r, egui::Id::new(("ribbon", *name, b.label)), if on { Sense::click() } else { Sense::hover() });
                    if self.checked(b) {
                        p.rect_filled(r, 3.0, t.selected);
                    }
                    if on && resp.hovered() {
                        p.rect_filled(r, 3.0, t.hover);
                    }
                    let color = if on { t.accent } else { t.disabled };
                    icons::paint(&p, Rect::from_center_size(pos2(r.center().x, r.top() + 19.0), vec2(26.0, 26.0)), b.icon, color);
                    for (n, line) in lines.iter().enumerate() {
                        let y = r.top() + 38.0 + n as f32 * 13.0 + if lines.len() == 1 { 6.0 } else { 0.0 };
                        p.text(pos2(r.center().x, y), Align2::CENTER_TOP, line, font(11.5), if on { t.text } else { t.disabled });
                    }
                    if on && resp.clicked() {
                        self.press(b, r.left_bottom());
                    }
                    if b.cmd.is_empty() {
                        resp.on_hover_text("Not built yet");
                    }
                    x += w + 2.0;
                    i += 1;
                } else {
                    // Up to three small buttons share a column.
                    let column: Vec<&Btn> = buttons.iter().skip(i).take_while(|b| !b.large).take(3).collect();
                    let w = column.iter().map(|b| text_width(&p, b.label, font(11.5))).fold(30.0, f32::max) + 36.0;
                    for (row, b) in column.iter().enumerate() {
                        let r = Rect::from_min_size(pos2(x, top + row as f32 * 22.0), vec2(w, 22.0));
                        let on = self.available(b.cmd);
                        let resp = ui.interact(r, egui::Id::new(("ribbon", *name, b.label)), if on { Sense::click() } else { Sense::hover() });
                        if on && resp.hovered() {
                            p.rect_filled(r, 3.0, t.hover);
                        }
                        icons::paint(
                            &p,
                            Rect::from_center_size(pos2(r.left() + 12.0, r.center().y), vec2(15.0, 15.0)),
                            b.icon,
                            if on { t.accent } else { t.disabled },
                        );
                        p.text(pos2(r.left() + 24.0, r.center().y), Align2::LEFT_CENTER, b.label, font(11.5), if on { t.text } else { t.disabled });
                        if menu_of(b.cmd).is_some() {
                            icons::paint(
                                &p,
                                Rect::from_center_size(pos2(r.right() - 7.0, r.center().y), vec2(8.0, 8.0)),
                                Icon::ChevronDown,
                                if on { t.text_dim } else { t.disabled },
                            );
                        }
                        if on && resp.clicked() {
                            self.press(b, r.left_bottom());
                        }
                        if b.cmd.is_empty() {
                            resp.on_hover_text("Not built yet");
                        }
                    }
                    x += w + 2.0;
                    i += column.len().max(1);
                }
            }
            p.text(pos2((start + x) / 2.0, label_y), Align2::CENTER_CENTER, name, font(10.5), t.text_faint);
            x += 5.0;
            p.vline(x, (rect.top() + 6.0)..=(rect.bottom() - 6.0), (1.0, t.line));
            x += 6.0;
        }
    }

    fn menu_items(&self, menu: Menu) -> Vec<MenuItem> {
        let s = &self.session;
        match menu {
            Menu::Categorize => {
                let have: Vec<String> = s.open.as_ref().map(|it| it.categories.clone()).unwrap_or_default();
                let mut items: Vec<MenuItem> =
                    ["Red category", "Orange category", "Yellow category", "Green category", "Blue category", "Purple category"]
                        .into_iter()
                        .map(|name| MenuItem {
                            swatch: Some(theme::category_color(name)),
                            checked: have.iter().any(|c| c.eq_ignore_ascii_case(name)),
                            ..item(name, "mail.categorize", json!({ "category": name }))
                        })
                        .collect();
                items.push(item("Clear All Categories", "mail.categorize", json!({ "clear": true })));
                items
            }
            Menu::FollowUp => vec![
                MenuItem { swatch: Some(self.tokens.flag), ..item("Today", "mail.followUp", json!({ "when": "today" })) },
                MenuItem { swatch: Some(self.tokens.flag), ..item("Tomorrow", "mail.followUp", json!({ "when": "tomorrow" })) },
                MenuItem { swatch: Some(self.tokens.flag), ..item("This Week", "mail.followUp", json!({ "when": "thisWeek" })) },
                MenuItem { swatch: Some(self.tokens.flag), ..item("Next Week", "mail.followUp", json!({ "when": "nextWeek" })) },
                MenuItem { swatch: Some(self.tokens.flag), ..item("No Date", "mail.followUp", json!({ "when": "noDate" })) },
                item("Mark Complete", "mail.flagComplete", Value::Null),
                item("Clear Flag", "mail.clearFlag", Value::Null),
            ],
            Menu::Move => {
                let Some((store, current)) = s.current else { return Vec::new() };
                let folders: &[FolderInfo] = s.folders.get(store).map_or(&[], Vec::as_slice);
                folders
                    .iter()
                    .filter(|f| {
                        f.id != current && !matches!(f.kind, FolderKind::Calendar | FolderKind::Contacts | FolderKind::Tasks | FolderKind::Search)
                    })
                    .take(40)
                    .map(|f| item(&folder_path(folders, f.id), "mail.move", json!({ "folderId": f.id.0 })))
                    .collect()
            }
            Menu::Arrange => ["date", "from", "to", "subject", "size", "importance", "flag", "attachments"]
                .into_iter()
                .map(|key| {
                    let label: String = key.chars().take(1).flat_map(char::to_uppercase).chain(key.chars().skip(1)).collect();
                    MenuItem { checked: s.sort.name() == key, ..item(&label, "view.sortBy", json!({ "key": key })) }
                })
                .collect(),
            Menu::Groups => {
                let every = s.send_receive_min;
                let mut items: Vec<MenuItem> = [0u32, 5, 10, 15, 30, 60]
                    .into_iter()
                    .map(|m| {
                        let label = if m == 0 { "Only when asked (F9)".to_string() } else { format!("Every {m} minutes") };
                        MenuItem { checked: every == m, ..item(&label, "mail.sendReceiveEvery", json!({ "minutes": m })) }
                    })
                    .collect();
                if ![0, 5, 10, 15, 30, 60].contains(&every) {
                    items.push(MenuItem {
                        checked: true,
                        ..item(&format!("Every {every} minutes"), "mail.sendReceiveEvery", json!({ "minutes": every }))
                    });
                }
                let a = s.alerts;
                for (label, key, on) in [
                    ("New mail: alert on the desktop", "alert", a.alert),
                    ("New mail: a sound", "sound", a.sound),
                    ("New mail: taskbar button", "flash", a.flash),
                ] {
                    items.push(MenuItem { checked: on, ..item(label, "mail.newMailAlerts", json!({ key: !on })) });
                }
                items
            }
            Menu::Reading => [("Right", "right"), ("Bottom", "bottom"), ("Off", "off")]
                .into_iter()
                .map(|(label, position)| MenuItem {
                    checked: format!("{:?}", s.reading_pane).eq_ignore_ascii_case(position),
                    ..item(label, "view.readingPane", json!({ "position": position }))
                })
                .collect(),
            Menu::TodoBar => {
                let shown = s.todo_bar_parts();
                let mut items: Vec<MenuItem> =
                    [("Calendar", "calendar", shown.calendar), ("People", "people", shown.people), ("Tasks", "tasks", shown.tasks)]
                        .into_iter()
                        .map(|(label, part, on)| MenuItem { checked: on, ..item(label, "view.todoBar", json!({ "part": part, "show": !on })) })
                        .collect();
                items.push(MenuItem { checked: !shown.any(), ..item("Off", "view.todoBar", json!({ "part": "off" })) });
                items
            }
            Menu::Rules => {
                let mut items = Vec::new();
                if s.is_enabled("rules.create") {
                    items.push(item("Create Rule…", "ui.options", json!({ "page": "rules", "create": true })));
                }
                items.push(item("Manage Rules & Alerts…", "ui.options", json!({ "page": "rules" })));
                if s.is_enabled("rules.runNow") {
                    items.push(item("Run Rules Now…", "ui.options", json!({ "page": "rules", "runNow": true })));
                }
                items
            }
        }
    }

    pub(crate) fn menu_popup(&mut self, ctx: &egui::Context) {
        let Some((menu, pos)) = self.menu else { return };
        let t = self.tokens.clone();
        let items = self.menu_items(menu);
        let mut chosen: Option<usize> = None;
        let area = egui::Area::new(egui::Id::new("openmailclient-menu")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            let p = ui.painter().clone();
            let width = items.iter().map(|i| text_width(&p, &i.label, font(12.0))).fold(120.0, f32::max).min(360.0) + 52.0;
            let height = (items.len().max(1) as f32) * 24.0 + 8.0;
            let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            p.rect_filled(rect.translate(vec2(1.0, 2.0)), 4.0, Color32::from_black_alpha(40));
            p.rect_filled(rect, 4.0, t.surface);
            p.rect_stroke(rect, 4.0, (1.0, t.line), egui::StrokeKind::Inside);
            if items.is_empty() {
                p.text(rect.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, "Nothing to choose here", font(12.0), t.text_faint);
            }
            for (n, it) in items.iter().enumerate() {
                let r = Rect::from_min_size(pos2(rect.left() + 3.0, rect.top() + 4.0 + n as f32 * 24.0), vec2(width - 6.0, 24.0));
                let resp = ui.interact(r, egui::Id::new(("menu-item", n)), Sense::click());
                if resp.hovered() {
                    p.rect_filled(r, 3.0, t.hover);
                }
                if it.checked {
                    icons::paint(&p, Rect::from_center_size(pos2(r.left() + 11.0, r.center().y), vec2(11.0, 11.0)), Icon::Check, t.accent);
                }
                if let Some(c) = it.swatch {
                    p.rect_filled(Rect::from_center_size(pos2(r.left() + 28.0, r.center().y), vec2(11.0, 11.0)), 2.0, c);
                }
                clip_text(&p, pos2(r.left() + 40.0, r.center().y - 8.0), width - 50.0, &it.label, font(12.0), t.text);
                if resp.clicked() {
                    chosen = Some(n);
                }
            }
        });
        if let Some(it) = chosen.and_then(|n| items.into_iter().nth(n)) {
            self.act(it.cmd, it.params);
            self.menu = None;
        } else if !self.menu_fresh && ctx.input(|i| i.pointer.any_click()) && !area.response.contains_pointer() {
            self.menu = None;
        }
        self.menu_fresh = false;
    }

    /// Ask about a link before anything is opened: where it really goes, what is wrong with
    /// it, and the whole address. Only "Open" here ever hands an address to the system.
    pub(crate) fn link_window(&mut self, ctx: &egui::Context) {
        let Some(ask) = self.link_ask.as_ref() else { return };
        let t = self.tokens.clone();
        // What to do: open (true), or only close (false).
        let mut done: Option<bool> = None;
        let title = if ask.mailbox { "Write to this mailbox?" } else { "Open this link?" };
        let icon = if ask.mailbox { Icon::Mail } else { Icon::Link };
        let shown = crate::dialog::Dialog::new("link", title, icon).width(460.0).show(ctx, &t, |ui| {
            // What a message says of itself is put in the order it is read, like every line taken from one.
            let words = |text: &str, size: f32, color: Color32| egui::Label::new(crate::seen_line(text, font(size), color)).wrap();
            if !ask.text.is_empty() {
                ui.add(words(&format!("The text says: {}", ask.text), 12.5, t.text_dim));
                ui.add_space(4.0);
            }
            if let Some(goes) = &ask.goes {
                ui.add(words(if ask.mailbox { "It writes to a mailbox at" } else { "It goes to" }, 12.5, t.text_dim));
                ui.add(egui::Label::new(crate::seen_line(goes, bold(17.0), t.text)).wrap());
                ui.add_space(4.0);
            }
            if let Some(concern) = &ask.concern {
                crate::dialog::callout(ui, &t, t.danger, |ui| {
                    ui.add(words(concern, 13.0, t.text));
                });
                ui.add_space(4.0);
            }
            // The whole address, in a box of its own so where it starts and ends is plain.
            let shown: String = ask.address.chars().take(600).collect();
            crate::dialog::card(ui, &t, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(shown).font(egui::FontId::monospace(12.0)).color(t.text_dim)).wrap());
            });
            if let Some(refused) = &ask.refused {
                ui.add_space(4.0);
                crate::dialog::callout(ui, &t, t.flag, |ui| {
                    ui.add(words(refused, 13.0, t.text));
                });
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
            crate::dialog::foot(ui, &t, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // The safe answer is the one a key press gives.
                    let cancel = ui.add(crate::dialog::plain("Cancel"));
                    if !cancel.has_focus() && ui.memory(|m| m.focused().is_none()) {
                        cancel.request_focus();
                    }
                    if cancel.clicked() {
                        done = Some(false);
                    }
                    if ui.add(crate::dialog::plain("Copy address")).clicked() {
                        ui.ctx().copy_text(ask.address.clone());
                        done = Some(false);
                    }
                    let open = if ask.mailbox { "Write a message" } else { "Open in browser" };
                    if ui.add_enabled(ask.refused.is_none(), crate::dialog::primary(&t, open)).clicked() {
                        done = Some(true);
                    }
                });
            });
        });
        if shown.closed {
            done = Some(false);
        }
        let Some(open) = done else { return };
        let Some(ask) = self.link_ask.take().filter(|_| open) else { return };
        // The engine checks the address again; the person has seen the concern and said yes.
        match self.execute("mail.openLink", &json!({ "address": ask.address, "confirm": true })) {
            Ok(result) => {
                if let Some(address) = result.pointer("/untrusted_content/open").and_then(Value::as_str) {
                    self.requests.push(crate::UiRequest::OpenLink(address.to_string()));
                }
            }
            Err(e) => self.session.status = e,
        }
    }

    /// Ask what to do with an attachment, as Outlook does on a double-click: open it, save it,
    /// or neither. What the reader should know of it is said first, a program or a script is
    /// only ever saved, and the answer a key press gives is the safe one.
    pub(crate) fn attachment_window(&mut self, ctx: &egui::Context) {
        let Some(ask) = self.attachment_ask.as_ref() else { return };
        let t = self.tokens.clone();
        #[derive(Clone, Copy)]
        enum Answer {
            Open,
            Save,
            Cancel,
            AddToCalendar,
        }
        // A calendar file is opened here, its events shown before they are added.
        let calendar = ask.name.to_ascii_lowercase().ends_with(".ics") && ask.refused.is_none();
        let mut done: Option<Answer> = None;
        let shown = crate::dialog::Dialog::new("attachment", "Opening an attachment", Icon::Clip).width(440.0).show(ctx, &t, |ui| {
            let words = |text: &str, size: f32, color: Color32| egui::Label::new(crate::seen_line(text, font(size), color)).wrap();
            ui.add(words("Do you want to open this file or save it?", 12.5, t.text_dim));
            ui.add_space(4.0);
            // The file, as a card: its mark, its name and its size.
            crate::dialog::card(ui, &t, |ui| {
                ui.horizontal(|ui| {
                    let (mark, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
                    ui.painter().rect_filled(mark, 6.0, t.accent_soft);
                    icons::paint(ui.painter(), mark.shrink(7.0), Icon::Clip, t.accent);
                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(crate::seen_line(&ask.name, bold(15.0), t.text)).wrap());
                        ui.add(words(&openmailclient_engine::attachments::size_words(ask.size), 12.0, t.text_dim));
                    });
                });
            });
            ui.add_space(4.0);
            if let Some(caution) = &ask.caution {
                crate::dialog::callout(ui, &t, t.danger, |ui| {
                    ui.add(words(caution, 13.0, t.text));
                });
                ui.add_space(4.0);
            }
            match &ask.refused {
                Some(refused) => ui.add(words(refused, 13.0, t.text)),
                None if ask.message => ui.add(words("The attached message opens here, as a message.", 12.0, t.text_dim)),
                None if calendar => ui.add(words("Its events are shown first, and you choose which are added to your calendar.", 12.0, t.text_dim)),
                None => ui.add(words(
                    "It opens in the program your system uses for this kind of file, as a read-only copy that is cleared away when OpenMailClient closes. Only open files from people you trust.",
                    12.0,
                    t.text_dim,
                )),
            };
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(Answer::Cancel);
            }
            crate::dialog::foot(ui, &t, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cancel = ui.add(crate::dialog::plain("Cancel"));
                    if !cancel.has_focus() && ui.memory(|m| m.focused().is_none()) {
                        cancel.request_focus();
                    }
                    if cancel.clicked() {
                        done = Some(Answer::Cancel);
                    }
                    if ui.add(crate::dialog::plain("Save As…")).clicked() {
                        done = Some(Answer::Save);
                    }
                    if calendar {
                        if ui.add(crate::dialog::primary(&t, "Add to Calendar…")).clicked() {
                            done = Some(Answer::AddToCalendar);
                        }
                    } else if ui.add_enabled(ask.refused.is_none(), crate::dialog::primary(&t, "Open")).clicked() {
                        done = Some(Answer::Open);
                    }
                });
            });
        });
        if shown.closed {
            done = Some(Answer::Cancel);
        }
        let Some(answer) = done else { return };
        let Some(ask) = self.attachment_ask.take() else { return };
        match answer {
            Answer::Cancel => {}
            Answer::Save => self.requests.push(crate::UiRequest::SaveAttachment(ask.index)),
            Answer::AddToCalendar => {
                if let Err(e) = self.calendar_file_command(&json!({ "attachment": ask.index })) {
                    self.session.status = e;
                }
            }
            // The engine makes the copy and checks the attachment again; the person said yes.
            Answer::Open => match self.execute("file.openAttachment", &json!({ "index": ask.index, "confirm": true })) {
                Ok(result) => {
                    let path = result.pointer("/untrusted_content/open").and_then(Value::as_str).map(str::to_string);
                    match (path, result["inside"].as_bool() == Some(true)) {
                        (Some(path), true) => {
                            if let Err(e) = self.execute("file.open", &json!({ "path": path })) {
                                self.session.status = e;
                            }
                        }
                        (Some(path), false) => self.requests.push(crate::UiRequest::OpenAttachment(path)),
                        (None, _) => {}
                    }
                }
                Err(e) => self.session.status = e,
            },
        }
    }

    pub(crate) fn prompt_window(&mut self, ctx: &egui::Context) {
        let t = self.tokens.clone();
        let Some(prompt) = self.prompt.as_mut() else { return };
        let mut done: Option<bool> = None;
        let shown = crate::dialog::Dialog::new("prompt", prompt.title, Icon::Folder).width(320.0).show(ctx, &t, |ui| {
            ui.label(egui::RichText::new("Name").font(font(12.0)).color(t.text_dim));
            let edit = ui.add(
                crate::line_edit::LineEdit::new(&mut prompt.value, egui::Id::new("openmailclient-prompt"))
                    .desired_width(f32::INFINITY)
                    .char_limit(255),
            );
            if prompt.focus {
                edit.request_focus();
                prompt.focus = false;
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
            let named = !prompt.value.trim().is_empty();
            crate::dialog::foot(ui, &t, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(crate::dialog::plain("Cancel")).clicked() {
                        done = Some(false);
                    }
                    if ui.add_enabled(named, crate::dialog::primary(&t, "OK")).clicked() {
                        done = Some(true);
                    }
                });
            });
        });
        if shown.closed {
            done = Some(false);
        }
        if let Some(accept) = done
            && let Some(p) = self.prompt.take().filter(|p| accept && !p.value.trim().is_empty())
        {
            self.act(p.command, json!({ "name": p.value.trim() }));
        }
    }

    /// The File page: open, save and close data files.
    pub(crate) fn backstage(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        let side = Rect::from_min_size(rect.min, vec2(SIDE_W, rect.height()));
        p.rect_filled(rect, 0.0, t.surface);
        p.rect_filled(side, 0.0, t.accent);
        // The pages of the File tab, then what is done at once: Print, Options and Exit.
        let entries: [(Icon, &str, Side); 7] = [
            (Icon::Back, "Back", Side::Act("ui.ribbonTab", json!({ "tab": "home" }))),
            (Icon::Info, "Info", Side::Page(FilePage::Info)),
            (Icon::Open, "Open & Export", Side::Page(FilePage::Open)),
            (Icon::Save, "Save", Side::Page(FilePage::Save)),
            (Icon::Print, "Print", Side::Act("ui.printPreview", Value::Null)),
            (Icon::Settings, "Options", Side::Act("ui.options", Value::Null)),
            (Icon::Junk, "Exit", Side::Act("ui.quit", Value::Null)),
        ];
        let mut y = side.top() + 10.0;
        for (n, (icon, label, what)) in entries.into_iter().enumerate() {
            // A gap between the pages and what is done at once.
            if n == 4 {
                p.hline(side.left() + 18.0..=side.right() - 18.0, y + 6.0, egui::Stroke::new(1.0, t.on_accent.gamma_multiply(0.3)));
                y += 14.0;
            }
            let r = Rect::from_min_size(pos2(side.left(), y), vec2(side.width(), 40.0));
            y += 42.0;
            let on = match &what {
                Side::Act("ui.printPreview", _) => self.session.is_enabled("file.print"),
                _ => true,
            };
            let shown = matches!(what, Side::Page(page) if page == self.file_page);
            let resp = ui.interact(r, egui::Id::new(("backstage", label)), if on { Sense::click() } else { Sense::hover() });
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, on, shown, label));
            if shown {
                p.rect_filled(r, 0.0, t.accent_dark);
                p.rect_filled(Rect::from_min_size(r.min, vec2(4.0, r.height())), 0.0, t.on_accent);
            } else if on && resp.hovered() {
                p.rect_filled(r, 0.0, t.accent_dark);
            }
            let color = if on { t.on_accent } else { t.on_accent.gamma_multiply(0.45) };
            icons::paint(&p, Rect::from_center_size(pos2(r.left() + 26.0, r.center().y), vec2(16.0, 16.0)), icon, color);
            p.text(pos2(r.left() + 48.0, r.center().y), Align2::LEFT_CENTER, label, if shown { bold(13.5) } else { font(13.5) }, color);
            if on && resp.clicked() {
                match what {
                    Side::Page(page) => self.file_page = page,
                    Side::Act(cmd, params) => {
                        self.act(cmd, params);
                        if cmd != "ui.ribbonTab" && cmd != "ui.quit" {
                            self.act("ui.ribbonTab", json!({ "tab": "home" }));
                        }
                    }
                }
            }
        }

        let x = side.right() + 40.0;
        let width = (rect.right() - x - 30.0).max(40.0);
        let mut y = rect.top() + 26.0;
        let (title, tiles): (&str, Vec<Tile>) = match self.file_page {
            FilePage::Info => ("Info", self.info_tiles()),
            FilePage::Open => ("Open & Export", self.open_tiles()),
            FilePage::Save => ("Save", self.save_tiles()),
        };
        p.text(pos2(x, y), Align2::LEFT_TOP, title, font(26.0), t.text);
        y += 50.0;
        if self.file_page == FilePage::Info {
            y = self.data_files(&p, x, y, width);
        }
        y = self.tiles(ui, &p, tiles, x, y, width);
        if self.file_page == FilePage::Info {
            y += 10.0;
            for line in [
                "A data file is not written to until Save Changes; the first save keeps the file as it was beside it.",
                "Accounts, themes, time zones, reminders, saving and updates are all set in Options.",
            ] {
                clip_text(&p, pos2(x, y), width, line, font(12.0), t.text_faint);
                y += 20.0;
            }
        }
    }

    /// The data files open, each with where it is, how many folders it has and whether it
    /// can be written; returns where the next thing goes.
    fn data_files(&self, p: &egui::Painter, x: f32, mut y: f32, width: f32) -> f32 {
        let t = &self.tokens;
        p.text(pos2(x, y), Align2::LEFT_TOP, "Data files", bold(14.0), t.text);
        y += 28.0;
        if self.session.stores.is_empty() {
            clip_text(
                p,
                pos2(x, y),
                width,
                "Nothing is open. Open an Outlook data file (.pst, .ost), a message (.msg, .eml, .oft) or the sample mailbox.",
                font(13.0),
                t.text_dim,
            );
            y += 28.0;
        }
        for (i, store) in self.session.stores.iter().enumerate() {
            let folders = self.session.folders.get(i).map_or(0, Vec::len);
            p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(4.0, 40.0)), 2.0, t.accent);
            clip_text(p, pos2(x + 14.0, y), width, &store.display_name(), bold(14.0), t.text);
            let location = store.path().map_or_else(|| "In memory (not saved to disk)".to_string(), |path| path.display().to_string());
            let mode = match (store.writable(), store.unsaved()) {
                (_, true) if store.journal_trouble().is_some() => "unsaved changes, in memory only",
                (_, true) if store.path().is_some() => "unsaved changes, journalled beside the file",
                (_, true) => "unsaved changes",
                (true, false) => "editable",
                (false, _) => "read-only",
            };
            clip_text(p, pos2(x + 14.0, y + 20.0), width, &format!("{location}  ·  {folders} folders  ·  {mode}"), font(12.0), t.text_dim);
            y += 54.0;
        }
        y + 10.0
    }

    fn info_tiles(&self) -> Vec<Tile> {
        let s = &self.session;
        vec![
            Tile::new(
                Icon::Save,
                "Save Changes",
                "Write what changed into the data files, checked as it goes",
                "file.save",
                json!({ "wait": false }),
            )
            .on(s.is_enabled("file.save")),
            Tile::new(Icon::Close, "Close Data File", "Close the data file chosen in the folder pane", "file.closeStore", Value::Null)
                .on(s.is_enabled("file.closeStore")),
            Tile::new(Icon::People, "Account Settings", "Add, change, test and remove mail accounts", "ui.options", json!({ "page": "accounts" })),
            Tile::new(Icon::Settings, "Options", "Every setting of OpenMailClient, in one place", "ui.options", Value::Null),
        ]
    }

    fn open_tiles(&self) -> Vec<Tile> {
        let s = &self.session;
        vec![
            Tile::new(
                Icon::Open,
                "Open Data File or Message",
                "A .pst or .ost data file, or a .msg, .eml or .oft message",
                "ui.openDialog",
                Value::Null,
            ),
            Tile::new(Icon::Mail, "Open Sample Mailbox", "A mailbox of our own, to try things on", "file.openSample", Value::Null)
                .on(s.is_enabled("file.openSample")),
            Tile::new(
                Icon::Import,
                "Import from Outlook",
                "Outlook's accounts and data files, copied without changing them",
                "ui.importOutlook",
                Value::Null,
            )
            .on(s.is_enabled("file.importOutlook")),
            Tile::new(Icon::Save, "Export to a Data File", "The data file chosen, written as a new .pst", "ui.exportDialog", Value::Null)
                .on(s.is_enabled("file.export")),
            Tile::new(Icon::Calendar, "Open Calendar", "Events from an .ics file, shown before they are added", "ui.openCalendarDialog", Value::Null),
            Tile::new(Icon::Calendar, "Save Calendar", "The calendar as an .ics file", "ui.saveCalendarDialog", Value::Null)
                .on(s.is_enabled("calendar.saveAs")),
            Tile::new(Icon::People, "Open vCard File", "Contacts from a .vcf file, shown before they are added", "ui.openCardsDialog", Value::Null),
            Tile::new(Icon::People, "Save Contacts as vCard", "Every contact as one .vcf file", "ui.saveCardsDialog", json!({ "all": true }))
                .on(s.is_enabled("people.saveAs") && s.people.total > 0),
        ]
    }

    fn save_tiles(&self) -> Vec<Tile> {
        let s = &self.session;
        let attached = s.is_enabled("file.saveAttachment") && s.open.as_ref().is_some_and(|it| !it.attachments.is_empty());
        vec![
            Tile::new(
                Icon::Save,
                "Save Changes",
                "Write what changed into the data files, checked as it goes",
                "file.save",
                json!({ "wait": false }),
            )
            .on(s.is_enabled("file.save")),
            Tile::new(Icon::Save, "Save Message As", "The message open, as .msg, .oft or .eml", "ui.saveAsDialog", Value::Null)
                .on(s.is_enabled("file.saveAs")),
            Tile::new(Icon::Clip, "Save Attachment", "The open message's first attachment", "ui.saveAttachmentDialog", json!({ "index": 0 }))
                .on(attached),
            Tile::new(
                Icon::Clip,
                "Save All Attachments",
                "Every attachment of the open message, into a folder",
                "ui.saveAllAttachmentsDialog",
                Value::Null,
            )
            .on(attached),
        ]
    }

    /// Tiles two to a row where there is room, one where there is not; returns where the next
    /// thing goes.
    fn tiles(&mut self, ui: &mut Ui, p: &egui::Painter, tiles: Vec<Tile>, x: f32, y: f32, width: f32) -> f32 {
        let t = self.tokens.clone();
        let across = if width >= 2.0 * TILE_W + 16.0 { 2 } else { 1 };
        let w = if across == 2 { TILE_W } else { width.min(TILE_W) };
        let mut bottom = y;
        for (n, tile) in tiles.into_iter().enumerate() {
            let (row, col) = (n / across, n % across);
            let r = Rect::from_min_size(pos2(x + col as f32 * (TILE_W + 16.0), y + row as f32 * (TILE_H + 12.0)), vec2(w, TILE_H));
            bottom = r.bottom() + 12.0;
            let resp = ui.interact(r, egui::Id::new(("backstage-tile", tile.label)), if tile.on { Sense::click() } else { Sense::hover() });
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, tile.on, tile.label));
            let lit = tile.on && resp.hovered();
            p.rect_filled(r, 8.0, if lit { t.hover } else { t.window });
            p.rect_stroke(r, 8.0, egui::Stroke::new(1.0, if lit { t.accent } else { t.line }), egui::StrokeKind::Inside);
            let (mark, words, dim) = if tile.on { (t.accent, t.text, t.text_dim) } else { (t.text_faint, t.text_faint, t.text_faint) };
            let ring = pos2(r.left() + 30.0, r.center().y);
            p.circle_filled(ring, 18.0, mark.gamma_multiply(0.12));
            icons::paint(p, Rect::from_center_size(ring, vec2(20.0, 20.0)), tile.icon, mark);
            clip_text(p, pos2(r.left() + 60.0, r.top() + 13.0), r.width() - 72.0, tile.label, bold(14.0), words);
            clip_text(p, pos2(r.left() + 60.0, r.top() + 34.0), r.width() - 72.0, tile.about, font(12.0), dim);
            if tile.on && resp.clicked() {
                self.act(tile.cmd, tile.params);
                self.act("ui.ribbonTab", json!({ "tab": "home" }));
            }
        }
        bottom
    }
}

/// How wide the File tab's side stands.
const SIDE_W: f32 = 220.0;
const TILE_W: f32 = 380.0;
const TILE_H: f32 = 64.0;

/// The pages of the File tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum FilePage {
    #[default]
    Info,
    Open,
    Save,
}

/// What an entry at the side of the File tab does: show a page, or run a command.
enum Side {
    Page(FilePage),
    Act(&'static str, Value),
}

/// One thing the File tab offers: its mark, its name, a line of what it does, and its command.
struct Tile {
    icon: Icon,
    label: &'static str,
    about: &'static str,
    cmd: &'static str,
    params: Value,
    on: bool,
}

impl Tile {
    fn new(icon: Icon, label: &'static str, about: &'static str, cmd: &'static str, params: Value) -> Self {
        Self { icon, label, about, cmd, params, on: true }
    }

    fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }
}

/// "Inbox/Projects" for a folder, walking up at most 32 parents.
pub(crate) fn folder_path(folders: &[FolderInfo], id: FolderId) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut cur = Some(id);
    while let Some(f) = cur.and_then(|c| folders.iter().find(|f| f.id == c)) {
        parts.push(&f.name);
        cur = f.parent;
        if parts.len() >= 32 {
            break;
        }
    }
    parts.reverse();
    parts.join("/")
}
