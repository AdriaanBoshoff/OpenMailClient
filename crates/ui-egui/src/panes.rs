//! The window body: navigation strip, folder pane, message list, reading pane and the inline
//! compose form.

use crate::icons::{self, Icon};
use crate::{MailUi, body, bold, clip_text, font, one_line, text_width, theme};
use egui::{Align2, Rect, ScrollArea, Sense, Ui, UiBuilder, pos2, vec2};
use openmailclient_engine::{Module, ReadingPane, SortKey};
use openmailclient_model::zone::Zone;
use openmailclient_model::{Flag, FolderId, FolderInfo, FolderKind, RECIP_CC, RECIP_TO, Summary, time};
use serde_json::{Value, json};

const ROW_H: f32 = 64.0;
/// Half the width of the strip at a pane's edge that takes hold of it.
const GRIP: f32 = 3.0;

/// An edge between panes: where the pane it sizes begins (and, for the list over the reading
/// pane, the height the two share).
#[derive(Clone, Copy)]
enum Edge {
    Folders(f32),
    List(f32),
    ListBottom(f32, f32),
}
const GROUP_H: f32 = 24.0;
/// Body text beyond this is not laid out (a message body can be hundreds of megabytes).
const MAX_BODY_CHARS: usize = 200_000;

enum Entry {
    Group(usize),
    Row(usize),
}

fn folder_icon(kind: FolderKind) -> Icon {
    match kind {
        FolderKind::Inbox => Icon::Mail,
        FolderKind::Sent | FolderKind::Outbox => Icon::Send,
        FolderKind::Drafts => Icon::Rename,
        FolderKind::Deleted => Icon::Delete,
        FolderKind::Junk => Icon::Junk,
        FolderKind::Archive => Icon::Archive,
        FolderKind::Calendar => Icon::Calendar,
        FolderKind::Contacts => Icon::People,
        FolderKind::Tasks => Icon::Tasks,
        FolderKind::Search => Icon::Search,
        _ => Icon::Folder,
    }
}

fn initials(name: &str) -> String {
    let mut out: String =
        name.split_whitespace().filter_map(|w| w.chars().find(|c| c.is_alphanumeric())).take(2).flat_map(char::to_uppercase).collect();
    if out.is_empty() {
        out.push('?');
    }
    out
}

fn human_size(bytes: usize) -> String {
    openmailclient_engine::attachments::size_words(bytes)
}

impl MailUi {
    pub(crate) fn body(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        // The To-Do Bar stands at the right of whichever module is shown, when it shows a part.
        let rest = if self.session.todo_bar_parts().any() && rect.width() > 2.0 * crate::todo_bar::WIDTH {
            let w = crate::todo_bar::WIDTH;
            self.todo_bar(ui, Rect::from_min_max(pos2(rect.right() - w, rect.top()), rect.max));
            Rect::from_min_max(rect.min, pos2(rect.right() - w, rect.bottom()))
        } else {
            rect
        };
        if self.session.module == Module::Calendar {
            self.calendar_body(ui, rest);
            return;
        }
        if self.session.module == Module::People {
            self.people_body(ui, rest);
            return;
        }
        if self.session.module == Module::Tasks {
            self.tasks_body(ui, rest);
            return;
        }
        let mut left = rest.left();
        // The edges between the panes, to be dragged: where each is, and what it changes.
        let mut edges: Vec<(Rect, Edge)> = Vec::new();
        if self.session.folder_pane {
            let w = self.folder_w.min(rest.width() * 0.4);
            self.folder_pane(ui, Rect::from_min_size(pos2(left, rest.top()), vec2(w, rest.height())));
            left += w;
            edges.push((Rect::from_min_max(pos2(left - GRIP, rest.top()), pos2(left + GRIP, rest.bottom())), Edge::Folders(rest.left())));
        }
        let area = Rect::from_min_max(pos2(left, rest.top()), rest.max);
        let composing = self.session.compose.is_some();
        let pane = if composing && self.session.reading_pane == ReadingPane::Off { ReadingPane::Right } else { self.session.reading_pane };
        let (list, reading) = match pane {
            ReadingPane::Off => (area, None),
            ReadingPane::Right => {
                let w = self.list_w.min(area.width() * 0.7);
                edges.push((
                    Rect::from_min_max(pos2(area.left() + w - GRIP, area.top()), pos2(area.left() + w + GRIP, area.bottom())),
                    Edge::List(area.left()),
                ));
                (Rect::from_min_size(area.min, vec2(w, area.height())), Some(Rect::from_min_max(pos2(area.left() + w, area.top()), area.max)))
            }
            ReadingPane::Bottom => {
                let h = (area.height() * self.list_share).max(120.0).min(area.height());
                edges.push((
                    Rect::from_min_max(pos2(area.left(), area.top() + h - GRIP), pos2(area.right(), area.top() + h + GRIP)),
                    Edge::ListBottom(area.top(), area.height()),
                ));
                (Rect::from_min_size(area.min, vec2(area.width(), h)), Some(Rect::from_min_max(pos2(area.left(), area.top() + h), area.max)))
            }
        };
        self.message_list(ui, list);
        if let Some(r) = reading {
            let p = ui.painter_at(r);
            p.rect_filled(r, 0.0, t.surface);
            if pane == ReadingPane::Bottom {
                p.hline(r.x_range(), r.top() + 0.5, (1.0, t.line));
            } else {
                p.vline(r.left() + 0.5, r.y_range(), (1.0, t.line));
            }
            if composing {
                self.compose_form(ui, r.shrink2(vec2(1.0, 1.0)));
            } else {
                self.reading_pane(ui, r);
            }
        }
        // Last, so that an edge is over what the panes drew beside it.
        for (grip, edge) in edges {
            self.pane_edge(ui, grip, edge);
        }
    }

    /// An edge between two panes: dragging it sizes the pane before it, and a double click
    /// gives that pane the size it started with.
    fn pane_edge(&mut self, ui: &mut Ui, grip: Rect, edge: Edge) {
        let (name, across) = match edge {
            Edge::Folders(_) => ("edge-folders", true),
            Edge::List(_) => ("edge-list", true),
            Edge::ListBottom(..) => ("edge-list-bottom", false),
        };
        let found = ui.interact(grip, ui.id().with(name), Sense::click_and_drag());
        if found.hovered() || found.dragged() {
            ui.ctx().set_cursor_icon(if across { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
            let line = (2.0, self.tokens.accent);
            let p = ui.painter_at(grip);
            if across {
                p.vline(grip.center().x, grip.y_range(), line);
            } else {
                p.hline(grip.x_range(), grip.center().y, line);
            }
        }
        let first = crate::Tokens::default();
        let size = if found.double_clicked() {
            Some(match edge {
                Edge::Folders(_) => f64::from(first.folder_w),
                Edge::List(_) => f64::from(first.list_w),
                Edge::ListBottom(..) => f64::from(crate::FIRST_LIST_SHARE),
            })
        } else if found.dragged() {
            found.interact_pointer_pos().map(|at| match edge {
                Edge::Folders(from) | Edge::List(from) => f64::from(at.x - from),
                Edge::ListBottom(from, height) => f64::from((at.y - from) / height.max(1.0)),
            })
        } else {
            None
        };
        if let Some(size) = size {
            let key = match edge {
                Edge::Folders(_) => "folders",
                Edge::List(_) => "list",
                Edge::ListBottom(..) => "listShare",
            };
            self.actions.push(("ui.layout".to_string(), serde_json::json!({ key: size })));
        }
    }

    pub(crate) fn nav_strip(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.pane);
        p.vline(rect.right() - 0.5, rect.y_range(), (1.0, t.line));
        let modules = [
            (Module::Mail, Icon::Mail, "Mail"),
            (Module::Calendar, Icon::Calendar, "Calendar"),
            (Module::People, Icon::People, "People"),
            (Module::Tasks, Icon::Tasks, "Tasks"),
        ];
        let pitch = 48.0;
        for (n, (module, icon, label)) in modules.into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(rect.left(), rect.top() + 8.0 + n as f32 * pitch), vec2(rect.width() - 1.0, 40.0));
            let resp = ui.interact(r, egui::Id::new(("nav", label)), Sense::click());
            let active = self.session.module == module;
            if resp.hovered() || active {
                p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 4.0, if active { t.selected } else { t.hover });
            }
            if active {
                p.rect_filled(Rect::from_min_size(pos2(r.left() + 1.0, r.top() + 9.0), vec2(3.0, 22.0)), 1.5, t.accent);
            }
            icons::paint(&p, Rect::from_center_size(r.center(), vec2(20.0, 20.0)), icon, if active { t.accent } else { t.text_dim });
            if resp.clicked() {
                self.act("nav.module", json!({ "module": label.to_ascii_lowercase() }));
            }
            resp.on_hover_text(label);
        }
    }

    fn folder_pane(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        ui.painter_at(rect).rect_filled(rect, 0.0, t.pane);
        ui.painter_at(rect).vline(rect.right() - 0.5, rect.y_range(), (1.0, t.line));
        // Flatten the tree first: (store, depth, folder) rows in display order.
        struct Line {
            store: usize,
            depth: usize,
            folder: Option<FolderInfo>,
            title: String,
            open: bool,
            parent: bool,
        }
        let mut lines: Vec<Line> = Vec::new();
        for (store, folders) in self.session.folders.iter().enumerate() {
            let title = self.session.stores.get(store).map(|s| s.display_name()).unwrap_or_default();
            let open = !self.collapsed.contains(&(store, u64::MAX));
            lines.push(Line { store, depth: 0, folder: None, title, open, parent: true });
            if !open {
                continue;
            }
            // Depth-first with an explicit stack; a folder is visited once even if parents loop.
            let mut seen: Vec<FolderId> = Vec::new();
            let mut stack: Vec<(usize, &FolderInfo)> = children(folders, None).into_iter().rev().map(|f| (1, f)).collect();
            while let Some((depth, f)) = stack.pop() {
                if seen.contains(&f.id) || lines.len() > 20_000 {
                    continue;
                }
                seen.push(f.id);
                let kids = children(folders, Some(f.id));
                let open = !self.collapsed.contains(&(store, f.id.0));
                lines.push(Line { store, depth, folder: Some(f.clone()), title: f.name.clone(), open, parent: !kids.is_empty() });
                if open && depth < 32 {
                    stack.extend(kids.into_iter().rev().map(|k| (depth + 1, k)));
                }
            }
        }
        let current = self.session.current;
        let mut acts: Vec<(&str, Value)> = Vec::new();
        ui.scope_builder(UiBuilder::new().max_rect(rect.shrink2(vec2(1.0, 0.0))), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ScrollArea::vertical().id_salt("folders").auto_shrink(false).show_rows(ui, 26.0, lines.len(), |ui, range| {
                for line in lines.iter().skip(range.start).take(range.len()) {
                    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                    let p = ui.painter();
                    let folder_id = line.folder.as_ref().map(|f| f.id);
                    let selected = folder_id.is_some_and(|id| current == Some((line.store, id)));
                    if selected {
                        p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 3.0, t.selected);
                    } else if resp.hovered() {
                        p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 3.0, t.hover);
                    }
                    let x = r.left() + 8.0 + line.depth as f32 * 14.0;
                    let chevron = Rect::from_center_size(pos2(x + 5.0, r.center().y), vec2(9.0, 9.0));
                    let toggle = line.parent && resp.clicked() && resp.interact_pointer_pos().is_some_and(|pos| pos.x < chevron.right() + 6.0);
                    if line.parent {
                        icons::paint(p, chevron, if line.open { Icon::ChevronDown } else { Icon::ChevronRight }, t.text_dim);
                    }
                    let mut right = r.right() - 10.0;
                    match &line.folder {
                        None => {
                            clip_text(p, pos2(x + 16.0, r.center().y - 8.5), right - x - 16.0, &line.title, bold(12.5), t.text);
                            if resp.clicked() {
                                acts.push(("ui.toggleFolder", json!({ "store": line.store, "folderId": u64::MAX })));
                            }
                        }
                        Some(f) => {
                            let count = match f.kind {
                                FolderKind::Drafts | FolderKind::Outbox if f.total > 0 => Some((format!("[{}]", f.total), t.text_dim)),
                                _ if f.unread > 0 => Some((f.unread.to_string(), t.accent)),
                                _ => None,
                            };
                            if let Some((text, color)) = count {
                                right -= text_width(p, &text, bold(12.0));
                                p.text(pos2(right, r.center().y), Align2::LEFT_CENTER, &text, bold(12.0), color);
                                right -= 6.0;
                            }
                            icons::paint(
                                p,
                                Rect::from_center_size(pos2(x + 23.0, r.center().y), vec2(14.0, 14.0)),
                                folder_icon(f.kind),
                                if selected { t.accent } else { t.text_dim },
                            );
                            let name_font = if f.unread > 0 { bold(12.5) } else { font(12.5) };
                            clip_text(p, pos2(x + 36.0, r.center().y - 8.5), right - x - 36.0, &line.title, name_font, t.text);
                            if toggle {
                                acts.push(("ui.toggleFolder", json!({ "store": line.store, "folderId": f.id.0 })));
                            } else if resp.clicked() {
                                acts.push(("folder.select", json!({ "store": line.store, "folderId": f.id.0 })));
                            }
                        }
                    }
                }
            });
        });
        for (id, params) in acts {
            self.act(id, params);
        }
    }

    fn message_list(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.surface);
        let header = Rect::from_min_size(rect.min, vec2(rect.width(), 34.0));
        p.hline(rect.x_range(), header.bottom() - 0.5, (1.0, t.line));

        // "All | Unread" on the left, the arrangement on the right.
        let mut x = header.left() + 14.0;
        for (label, unread_only) in [("All", false), ("Unread", true)] {
            let active = self.session.unread_only == unread_only;
            let w = text_width(&p, label, bold(13.0));
            let r = Rect::from_min_size(pos2(x - 4.0, header.top() + 4.0), vec2(w + 8.0, 26.0));
            let resp = ui.interact(r, egui::Id::new(("list-filter", label)), Sense::click());
            p.text(
                pos2(x, header.center().y),
                Align2::LEFT_CENTER,
                label,
                if active { bold(13.0) } else { font(13.0) },
                if active { t.accent } else { t.text_dim },
            );
            if active {
                p.rect_filled(Rect::from_min_size(pos2(x, header.bottom() - 5.0), vec2(w, 2.5)), 1.0, t.accent);
            }
            if resp.clicked() && !active {
                self.act("view.filterUnread", json!({ "on": unread_only }));
            }
            x += w + 18.0;
        }
        let sort_name = match self.session.sort {
            SortKey::Date => "Date",
            SortKey::From => "From",
            SortKey::To => "To",
            SortKey::Subject => "Subject",
            SortKey::Size => "Size",
            SortKey::Importance => "Importance",
            SortKey::Flag => "Flag",
            SortKey::Attachments => "Attachments",
        };
        let sort_label = format!("By {sort_name}  {}", if self.session.sort_desc { "↓" } else { "↑" });
        let sort_w = text_width(&p, &sort_label, font(12.0));
        let sort_rect = Rect::from_min_size(pos2(header.right() - sort_w - 16.0, header.top() + 4.0), vec2(sort_w + 12.0, 26.0));
        let sort_resp = ui.interact(sort_rect, egui::Id::new("list-sort"), Sense::click());
        if sort_resp.hovered() {
            p.rect_filled(sort_rect, 3.0, t.hover);
        }
        p.text(sort_rect.center(), Align2::CENTER_CENTER, &sort_label, font(12.0), t.text_dim);
        if sort_resp.clicked() {
            self.act("ui.menu", json!({ "menu": "arrange", "x": sort_rect.left(), "y": sort_rect.bottom() }));
        }

        let body = Rect::from_min_max(header.left_bottom(), rect.max);
        let s = &self.session;
        if s.rows.is_empty() {
            let message = if s.stores.is_empty() {
                "Open a data file from the File tab"
            } else if !s.search.is_empty() {
                "Nothing matches the search"
            } else {
                "There is nothing to show here"
            };
            p.text(pos2(body.center().x, body.top() + 40.0), Align2::CENTER_CENTER, message, font(12.5), t.text_faint);
            return;
        }

        // Lay the rows out once, each group under its heading, however the list is arranged.
        let groups = s.groups();
        let now = s.now;
        let now_day = s.zone.civil(now).day_number;
        let mut entries: Vec<(f32, Entry)> = Vec::with_capacity(s.rows.len() + groups.len());
        let mut y = 0.0;
        let mut next = 0;
        for i in 0..s.rows.len() {
            if groups.get(next).is_some_and(|g| g.start == i) {
                entries.push((y, Entry::Group(next)));
                y += GROUP_H;
                next += 1;
            }
            entries.push((y, Entry::Row(i)));
            y += ROW_H;
        }
        let total = y;
        let selected = s.selected;
        let scroll_to = std::mem::take(&mut self.scroll_to_selected);
        // In a folder of mail going out, a row says whom it went to, as Outlook's do.
        let outgoing = s.current_folder().is_some_and(|f| matches!(f.kind, FolderKind::Sent | FolderKind::Drafts | FolderKind::Outbox));
        let shown = Shown { now, now_day, tz: &s.zone, region: &s.region, outgoing };
        let rows = &s.rows;
        let mut unread_previews: Vec<u64> = Vec::new();
        let mut clicked: Option<u64> = None;
        let mut opened = false;
        ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ScrollArea::vertical().id_salt("messages").auto_shrink(false).show_viewport(ui, |ui, viewport| {
                let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), total), Sense::hover());
                let p = ui.painter().clone();
                let first = entries.partition_point(|(y, _)| *y + ROW_H < viewport.top());
                for (y, entry) in entries.iter().skip(first) {
                    if *y > viewport.bottom() && !scroll_to {
                        break;
                    }
                    match entry {
                        Entry::Group(at) => {
                            let Some(group) = groups.get(*at) else { continue };
                            let r = Rect::from_min_size(pos2(area.left(), area.top() + y), vec2(area.width(), GROUP_H));
                            icons::paint(
                                &p,
                                Rect::from_center_size(pos2(r.left() + 14.0, r.center().y), vec2(8.0, 8.0)),
                                Icon::ChevronDown,
                                t.text_dim,
                            );
                            // How many the group holds, and how many of them wait to be read.
                            let count = match group.unread {
                                0 => group.count.to_string(),
                                unread => format!("{unread} unread of {}", group.count),
                            };
                            let counted = p.text(pos2(r.right() - 12.0, r.center().y), Align2::RIGHT_CENTER, count, font(11.0), t.text_faint);
                            clip_text(
                                &p,
                                pos2(r.left() + 24.0, r.center().y - 8.0),
                                (counted.left() - r.left() - 32.0).max(0.0),
                                &group.label,
                                bold(11.5),
                                t.text_dim,
                            );
                        }
                        Entry::Row(i) => {
                            let Some(row) = rows.get(*i) else { continue };
                            let r = Rect::from_min_size(pos2(area.left(), area.top() + y), vec2(area.width(), ROW_H));
                            let is_selected = selected == Some(row.id);
                            if is_selected && scroll_to {
                                ui.scroll_to_rect(r, None);
                            }
                            if *y > viewport.bottom() {
                                continue;
                            }
                            let resp = ui.interact(r, egui::Id::new(("row", row.id)), Sense::click());
                            if is_selected {
                                p.rect_filled(r, 0.0, t.selected);
                            } else if resp.hovered() {
                                p.rect_filled(r, 0.0, t.hover);
                            }
                            draw_row(&p, r, row, &shown, &t);
                            if row.preview.is_empty() {
                                unread_previews.push(row.id);
                            }
                            if resp.clicked() {
                                clicked = Some(row.id);
                            }
                            if resp.double_clicked() {
                                clicked = Some(row.id);
                                opened = true;
                            }
                        }
                    }
                }
            });
        });
        // The rows on screen whose folder's table holds no preview get theirs from the
        // message, a few each frame.
        if !unread_previews.is_empty() && self.session.fill_previews(&unread_previews) {
            ui.ctx().request_repaint();
        }
        if let Some(id) = clicked {
            self.act("mail.select", json!({ "id": id }));
            if opened {
                self.act("ui.openSelected", Value::Null);
            }
        }
    }

    fn reading_pane(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let doc = self.body_doc();
        // A meeting's request, answer or cancellation: what it is, and answered from here.
        let meeting_bar = self.meeting_bar();
        let mut meeting_acts: Vec<(&'static str, Value)> = Vec::new();
        if let (Some(doc), Some((key, _)), Some(item)) = (&doc, &self.body_doc, self.session.open.as_ref()) {
            self.pictures.load(ui.ctx(), *key, doc, item, &self.session.downloaded);
            self.pictures.moving = self.session.animate;
            self.faces.load(ui.ctx(), *key, doc);
        }
        let p = ui.painter_at(rect);
        let Some(item) = self.session.open.as_ref() else {
            icons::paint(&p, Rect::from_center_size(rect.center() - vec2(0.0, 26.0), vec2(52.0, 52.0)), Icon::Mail, t.disabled);
            p.text(rect.center() + vec2(0.0, 22.0), Align2::CENTER_CENTER, "Select an item to read", font(14.0), t.text_dim);
            return;
        };
        let pad = 18.0;
        let left = rect.left() + pad;
        let width = rect.width() - pad * 2.0;
        let mut y = rect.top() + 14.0;
        clip_text(&p, pos2(left, y), width, &item.subject(), font(18.0), t.text);
        y += 34.0;

        // Reply / Reply All / Forward on the right of the sender line.
        let mut bx = rect.right() - pad;
        let mut acts: Vec<&'static str> = Vec::new();
        for (icon, label, cmd) in
            [(Icon::Forward, "Forward", "mail.forward"), (Icon::ReplyAll, "Reply All", "mail.replyAll"), (Icon::Reply, "Reply", "mail.reply")]
        {
            let w = text_width(&p, label, font(12.0)) + 32.0;
            bx -= w;
            let r = Rect::from_min_size(pos2(bx, y), vec2(w - 4.0, 26.0));
            let resp = ui.interact(r, egui::Id::new(("reading-action", cmd)), Sense::click());
            p.rect_filled(r, 3.0, if resp.hovered() { t.hover } else { t.surface });
            p.rect_stroke(r, 3.0, (1.0, t.line), egui::StrokeKind::Inside);
            icons::paint(&p, Rect::from_center_size(pos2(r.left() + 13.0, r.center().y), vec2(14.0, 14.0)), icon, t.accent);
            p.text(pos2(r.left() + 24.0, r.center().y), Align2::LEFT_CENTER, label, font(12.0), t.text);
            if resp.clicked() {
                acts.push(cmd);
            }
        }
        let sender = item.sender_name();
        let email = item.sender_email();
        p.circle_filled(pos2(left + 18.0, y + 18.0), 18.0, theme::avatar_color(&sender));
        p.text(pos2(left + 18.0, y + 18.0), Align2::CENTER_CENTER, initials(&sender), bold(13.0), t.on_accent);
        let text_x = left + 46.0;
        let text_w = bx - text_x - 10.0;
        let from = if email.is_empty() || email == sender { sender.clone() } else { format!("{sender} <{email}>") };
        clip_text(&p, pos2(text_x, y - 1.0), text_w, &from, bold(13.0), t.text);
        let to = item.display_recipients(RECIP_TO);
        clip_text(&p, pos2(text_x, y + 18.0), text_w, &format!("To  {to}"), font(12.0), t.text_dim);
        y += 38.0;
        let cc = item.display_recipients(RECIP_CC);
        if !cc.is_empty() {
            clip_text(&p, pos2(text_x, y), width - 46.0, &format!("Cc  {cc}"), font(12.0), t.text_dim);
            y += 18.0;
        }
        let stamp = self.session.region.stamp(&self.session.zone.civil(item.time()));
        clip_text(&p, pos2(text_x, y), width - 46.0, &stamp, font(12.0), t.text_faint);
        y += 24.0;

        // Flag, importance and categories.
        let mut tx = left;
        if item.flag() != Flag::None {
            let done = item.flag() == Flag::Complete;
            icons::paint(
                &p,
                Rect::from_min_size(pos2(tx, y + 2.0), vec2(13.0, 13.0)),
                if done { Icon::Check } else { Icon::Flag },
                if done { t.ok } else { t.flag },
            );
            tx += 18.0;
            tx += clip_text(&p, pos2(tx, y), 200.0, if done { "Completed" } else { "Follow up" }, font(12.0), t.text_dim) + 14.0;
        }
        if item.importance() == 2 {
            icons::paint(&p, Rect::from_min_size(pos2(tx, y + 2.0), vec2(13.0, 13.0)), Icon::Importance, t.danger);
            tx += 16.0;
            tx += clip_text(&p, pos2(tx, y), 200.0, "High importance", font(12.0), t.text_dim) + 14.0;
        }
        for category in item.categories.iter().take(8) {
            let name = one_line(category);
            let w = text_width(&p, &name, font(11.5)).min(180.0) + 14.0;
            if tx + w > rect.right() - pad {
                break;
            }
            let chip = Rect::from_min_size(pos2(tx, y), vec2(w, 18.0));
            p.rect_filled(chip, 3.0, theme::category_color(category).gamma_multiply(0.25));
            p.rect_filled(Rect::from_min_size(chip.min, vec2(4.0, 18.0)), 2.0, theme::category_color(category));
            clip_text(&p, pos2(chip.left() + 8.0, chip.top() + 1.0), w - 10.0, &name, font(11.5), t.text);
            tx += w + 6.0;
        }
        if tx > left {
            y += 26.0;
        }

        // Attachments: a click picks one, a double-click or Enter asks whether to open or save it,
        // and a right-click offers the same. Nothing is opened or run without that answer.
        // The name is shown as it would be saved, so nothing in it is hidden or reversed.
        // A meeting's calendar is the meeting, answered from its bar, not a file to open.
        let meeting = openmailclient_model::meeting::is_meeting_message(item);
        let files: Vec<(usize, String, usize, Option<String>)> = openmailclient_engine::attachments::files(item)
            .filter(|(_, a)| !(meeting && a.mime().eq_ignore_ascii_case("text/calendar")))
            .map(|(i, a)| {
                (i, openmailclient_engine::attachments::file_name(a), a.size(), openmailclient_engine::attachments::caution_of(a).map(|c| c.words()))
            })
            .take(32)
            .collect();
        let mut save: Option<usize> = None;
        let mut ask: Option<usize> = None;
        let picked_id = egui::Id::new(("attachment-picked", self.session.selected));
        let mut picked: Option<usize> = ui.data(|d| d.get_temp(picked_id));
        let mut link: Option<std::sync::Arc<str>> = None;
        let mut save_all = false;
        let mut download = false;
        let mut write_on = false;
        if !files.is_empty() {
            let mut ax = left;
            for (index, name, size, caution) in &files {
                let label = format!("{}  ({})", one_line(name), human_size(*size));
                let w = (text_width(&p, &label, font(12.0)) + 36.0).min(width);
                if ax + w > rect.right() - pad && ax > left {
                    ax = left;
                    y += 32.0;
                }
                let chip = Rect::from_min_size(pos2(ax, y), vec2(w, 28.0));
                let resp = ui.interact(chip, egui::Id::new(("attachment", *index)), Sense::click());
                let is_picked = picked == Some(*index);
                p.rect_filled(
                    chip,
                    3.0,
                    if is_picked {
                        t.selected
                    } else if resp.hovered() {
                        t.hover
                    } else {
                        t.pane
                    },
                );
                // An attachment a reader should think twice about says so before it is pointed at.
                let (line, icon, tint) = match caution {
                    Some(_) => ((1.5, t.danger), Icon::Importance, t.danger),
                    None => ((1.0, t.line), Icon::Clip, t.text_dim),
                };
                p.rect_stroke(chip, 3.0, line, egui::StrokeKind::Inside);
                icons::paint(&p, Rect::from_center_size(pos2(chip.left() + 14.0, chip.center().y), vec2(14.0, 14.0)), icon, tint);
                // A long name is cut in its middle: the ending is what tells the kind of file.
                let room = w - 32.0;
                let label = if text_width(&p, &label, font(12.0)) > room { crate::middle_cut(&p, &label, room, font(12.0)) } else { label };
                clip_text(&p, pos2(chip.left() + 26.0, chip.center().y - 8.0), room, &label, font(12.0), t.text);
                if resp.clicked() {
                    picked = Some(*index);
                }
                if resp.double_clicked() {
                    ask = Some(*index);
                }
                let resp = resp.on_hover_text(match caution {
                    Some(words) => format!("{words}\nDouble-click to open or save it. Nothing is opened or run without your answer."),
                    None => "Double-click to open or save it. Nothing is opened or run without your answer.".to_string(),
                });
                resp.context_menu(|ui| {
                    picked = Some(*index);
                    if ui.button("Open…").clicked() {
                        ask = Some(*index);
                        ui.close();
                    }
                    if ui.button("Save As…").clicked() {
                        save = Some(*index);
                        ui.close();
                    }
                    if files.len() > 1 && ui.button("Save All Attachments…").clicked() {
                        save_all = true;
                        ui.close();
                    }
                });
                ax += w + 8.0;
            }
            // Enter asks about the picked one, as a double-click does.
            if let Some(index) = picked.filter(|i| files.iter().any(|f| f.0 == *i))
                && ui.memory(|m| m.focused().is_none())
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
            {
                ask = Some(index);
            }
            if files.len() > 1 {
                let label = "Save all…";
                let w = text_width(&p, label, font(12.0)) + 20.0;
                if ax + w > rect.right() - pad && ax > left {
                    ax = left;
                    y += 32.0;
                }
                let chip = Rect::from_min_size(pos2(ax, y), vec2(w, 28.0));
                let resp = ui.interact(chip, egui::Id::new("attachments-all"), Sense::click());
                if resp.hovered() {
                    p.rect_filled(chip, 3.0, t.hover);
                }
                p.text(chip.center(), egui::Align2::CENTER_CENTER, label, font(12.0), t.accent);
                save_all |= resp.clicked();
                resp.on_hover_text("Save every attachment into a folder. No file there is replaced.");
            }
            y += 38.0;
        }
        // Says only what was held back; a formatted body needs no notice of its own.
        // Pictures on a server not yet downloaded are downloaded from the bar, on the reader's word.
        let (named, waiting) = doc.as_ref().map_or((0, 0), |d| {
            let named = openmailclient_engine::remote::addresses(d);
            let waiting = named.iter().filter(|a| !self.session.downloaded.contains(a)).count();
            (named.len(), waiting)
        });
        let offer = waiting > 0 && !doc.as_ref().is_some_and(|d| d.truncated);
        let notice = match &doc {
            Some(d) if d.truncated => Some("This message is very long; the rest is not shown. Remote content is never downloaded."),
            Some(_) if offer => {
                Some("Pictures in this message were not downloaded, to keep you private. Click here, or right-click a picture, to download them.")
            }
            Some(d) if d.remote_blocked > 0 && named > 0 => {
                Some("The pictures were downloaded as you asked. Other remote content is not downloaded.")
            }
            Some(d) if d.remote_blocked > 0 => Some("Pictures and other remote content in this message were not downloaded."),
            Some(_) => None,
            None if item.body_html().is_some() || item.body_rtf().is_some() => {
                Some("Shown as plain text. Pictures and remote content are never downloaded.")
            }
            None => None,
        };
        // A draft says so, and opens to be written on from here as from a double-click.
        if item.is_unsent() {
            let bar = Rect::from_min_size(pos2(rect.left() + 1.0, y), vec2(rect.width() - 1.0, 24.0));
            p.rect_filled(bar, 0.0, t.info_bar);
            let resp = ui.interact(bar, egui::Id::new("edit-draft-bar"), Sense::click());
            if resp.hovered() {
                p.rect_filled(bar, 0.0, t.hover);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            write_on |= resp.clicked();
            icons::paint(&p, Rect::from_center_size(pos2(bar.left() + pad, bar.center().y), vec2(13.0, 13.0)), Icon::Info, t.text_dim);
            let words = "A draft: it has not been sent. Click here, or double-click it in the list, to go on writing it.";
            clip_text(&p, pos2(bar.left() + pad + 12.0, bar.center().y - 8.0), bar.width() - 40.0, words, font(11.5), t.text_dim);
            y += 30.0;
        }
        if let Some(bar) = &meeting_bar {
            let room = Rect::from_min_max(pos2(rect.left() + 1.0, y), pos2(rect.right(), rect.bottom()));
            let drawn =
                ui.scope_builder(UiBuilder::new().max_rect(room).id_salt("meeting-bar"), |ui| self.meeting_bar_ui(ui, bar, &mut meeting_acts));
            y += drawn.response.rect.height() + 6.0;
        }
        if let Some(notice) = notice {
            let bar = Rect::from_min_size(pos2(rect.left() + 1.0, y), vec2(rect.width() - 1.0, 24.0));
            p.rect_filled(bar, 0.0, t.info_bar);
            if offer {
                let resp = ui.interact(bar, egui::Id::new("download-pictures-bar"), Sense::click());
                if resp.hovered() {
                    p.rect_filled(bar, 0.0, t.hover);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                download |= resp.clicked();
                resp.context_menu(|ui| {
                    if ui.button("Download Pictures").clicked() {
                        download = true;
                        ui.close();
                    }
                });
            }
            icons::paint(&p, Rect::from_center_size(pos2(bar.left() + pad, bar.center().y), vec2(13.0, 13.0)), Icon::Info, t.text_dim);
            clip_text(&p, pos2(bar.left() + pad + 12.0, bar.center().y - 8.0), bar.width() - 40.0, notice, font(11.5), t.text_dim);
            y += 30.0;
        }
        p.hline((rect.left() + 1.0)..=rect.right(), y, (1.0, t.line));
        y += 1.0;

        let plain = match &doc {
            Some(_) => String::new(),
            None => {
                let full = item.body_text();
                let cut = full.char_indices().nth(MAX_BODY_CHARS).map(|(i, _)| i);
                let mut body: String = full.get(..cut.unwrap_or(full.len())).unwrap_or("").replace('\t', "    ");
                body.retain(|c| c == '\n' || !c.is_control());
                if cut.is_some() {
                    body.push_str("\n\n[The rest of this message is not shown.]");
                }
                body
            }
        };
        let body_rect = Rect::from_min_max(pos2(rect.left() + 1.0, y), rect.max);
        // A body that asks for a colour behind it has it behind all of the pane it is read in.
        if let Some(fill) = doc.as_ref().and_then(|doc| body::page(doc)) {
            ui.painter().rect_filled(body_rect, 0.0, fill);
        }
        ui.scope_builder(UiBuilder::new().max_rect(body_rect), |ui| {
            ScrollArea::vertical().id_salt(("body", self.session.selected)).auto_shrink(false).show(ui, |ui| {
                egui::Frame::NONE.inner_margin(egui::Margin::symmetric(pad as i8, 12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    match &doc {
                        // An attachment that stands in the text is asked about like one in the list above.
                        Some(doc) => {
                            let chosen = body::draw(ui, doc, &t, &self.pictures, &self.faces);
                            ask = chosen.file.or(ask);
                            link = chosen.link;
                            download |= chosen.download;
                        }
                        None => {
                            ui.add(egui::Label::new(egui::RichText::new(plain).font(font(13.5)).color(t.text)).wrap().selectable(true));
                        }
                    }
                });
            });
        });
        for cmd in acts {
            self.act(cmd, Value::Null);
        }
        for (cmd, params) in meeting_acts {
            self.act(cmd, params);
        }
        ui.data_mut(|d| match picked {
            Some(index) => {
                d.insert_temp(picked_id, index);
            }
            None => d.remove::<usize>(picked_id),
        });
        if let Some(index) = save {
            self.act("ui.saveAttachmentDialog", json!({ "index": index }));
        }
        if let Some(index) = ask {
            self.act("ui.attachmentDialog", json!({ "index": index }));
        }
        if let Some(link) = link {
            self.act("ui.linkDialog", json!({ "address": link.chars().filter(|c| !c.is_control()).collect::<String>() }));
        }
        if save_all {
            self.act("ui.saveAllAttachmentsDialog", Value::Null);
        }
        if download {
            self.act("mail.downloadPictures", json!({ "wait": false }));
        }
        if write_on {
            self.act("ui.openSelected", Value::Null);
        }
    }

    fn compose_form(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.tokens.clone();
        let Some(draft) = self.session.compose.as_ref() else { return };
        let mut fields = [
            ("to", "To", draft.to.clone(), true),
            ("cc", "Cc", draft.cc.clone(), true),
            ("bcc", "Bcc", draft.bcc.clone(), draft.show_bcc),
            ("subject", "Subject", draft.subject.clone(), true),
        ];
        let mut body = draft.body.clone();
        let attachments: Vec<(String, usize)> = draft.attachments.iter().map(|a| (a.name(), a.size())).collect();
        let show_bcc = draft.show_bcc;
        let signed = draft.signature_name.clone();
        let signature_names: Vec<String> = self.session.signatures.list.iter().map(|g| g.name.clone()).collect();
        let (go_at, expire_at) = (draft.deliver_after, draft.expires);
        let go_shown = (go_at > 0).then(|| self.session.time_shown(go_at));
        let expire_shown = (expire_at > 0).then(|| self.session.time_shown(expire_at));
        let (now, tz) = (self.session.now, self.session.zone.clone());
        let mut go_text = std::mem::take(&mut self.go_at_text);
        let mut expire_text = std::mem::take(&mut self.expire_at_text);
        // What the writer should know is on show while the message can still be changed.
        let cautions: Vec<String> = openmailclient_engine::outgoing::cautions(draft).iter().map(|c| c.words()).collect();
        let mut acts: Vec<(&str, Value)> = Vec::new();
        let suggest = self.suggest.clone();
        // A line typed in, whose names are looked for; a suggestion taken.
        let mut typed: Option<(&str, String)> = None;
        let mut took = false;
        ui.scope_builder(UiBuilder::new().max_rect(rect.shrink2(vec2(14.0, 10.0))), |ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
            // The buttons go on to a second line where the pane is narrow.
            ui.horizontal_wrapped(|ui| {
                let send = ui.add(
                    egui::Button::new(
                        egui::RichText::new(if cautions.is_empty() { "Send" } else { "Send Anyway" }).font(bold(13.0)).color(t.on_accent),
                    )
                    .fill(t.accent)
                    .min_size(vec2(72.0, 30.0)),
                );
                if send
                    .on_hover_text(
                        "Sends it through its account's server; it waits in the Outbox until it has gone, and a copy is kept in Sent Items.",
                    )
                    .clicked()
                {
                    acts.push(("compose.send", json!({ "anyway": !cautions.is_empty(), "wait": false })));
                }
                for (label, cmd, params) in [
                    ("Attach File…", "ui.attachDialog", Value::Null),
                    ("Check Names", "compose.checkNames", Value::Null),
                    (if show_bcc { "Hide Bcc" } else { "Bcc" }, "compose.showBcc", Value::Null),
                    ("Save Draft", "compose.saveDraft", Value::Null),
                    ("Discard", "compose.discard", Value::Null),
                ] {
                    if ui.add(egui::Button::new(egui::RichText::new(label).font(font(12.5))).min_size(vec2(0.0, 30.0))).clicked() {
                        acts.push((cmd, params));
                    }
                }
                ui.menu_button(egui::RichText::new("Delivery…").font(font(12.5)), |ui| {
                    delivery_menu(ui, &t, now, &tz, (&mut go_text, &mut expire_text), (go_at > 0, expire_at > 0), &mut acts);
                });
                // Message > Include > Signature: one put in, in place of the one there.
                ui.menu_button(egui::RichText::new("Signature").font(font(12.5)), |ui| {
                    for name in &signature_names {
                        if ui.selectable_label(signed.as_deref() == Some(name.as_str()), name).clicked() {
                            acts.push(("compose.signature", json!({ "name": name })));
                            ui.close();
                        }
                    }
                    if ui.selectable_label(signed.is_none(), "(none)").clicked() {
                        acts.push(("compose.signature", json!({ "name": null })));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Signatures…").clicked() {
                        acts.push(("ui.options", json!({ "page": "signatures" })));
                        ui.close();
                    }
                });
            });
            // What is set for when it goes and when it expires, each with a way to undo it.
            for (shown, words, cmd) in
                [(&go_shown, "Waits in the Outbox until", "compose.delayDelivery"), (&expire_shown, "Expires", "compose.expires")]
            {
                let Some(shown) = shown else { continue };
                ui.horizontal(|ui| {
                    let (mark, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
                    icons::paint(ui.painter(), Rect::from_center_size(mark.center(), vec2(14.0, 14.0)), Icon::Send, t.text_dim);
                    ui.label(egui::RichText::new(format!("{words} {shown}")).font(font(12.0)).color(t.text));
                    if ui.small_button("×").on_hover_text("Undo this").clicked() {
                        acts.push((cmd, json!({ "off": true })));
                    }
                });
            }
            for caution in &cautions {
                ui.horizontal(|ui| {
                    let (mark, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
                    icons::paint(ui.painter(), Rect::from_center_size(mark.center(), vec2(14.0, 14.0)), Icon::Importance, t.danger);
                    ui.add(egui::Label::new(crate::seen_line(caution, font(12.0), t.text)).wrap());
                });
            }
            ui.add_space(4.0);
            for (key, label, value, shown) in &mut fields {
                if !*shown {
                    continue;
                }
                let id = egui::Id::new(("compose", *key));
                let address_line = *key != "subject";
                // Suggestions under this line, while it is being typed in.
                let offered = suggest
                    .as_ref()
                    .filter(|s| address_line && s.field == *key && s.line == *value && !s.rows.is_empty() && ui.memory(|m| m.has_focus(id)));
                let mut pick = offered.map_or(0, |s| s.pick.min(s.rows.len().saturating_sub(1)));
                let mut take = false;
                if let Some(s) = offered {
                    ui.input_mut(|i| {
                        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                            pick = (pick + 1).min(s.rows.len().saturating_sub(1));
                        }
                        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                            pick = pick.saturating_sub(1);
                        }
                        if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) {
                            take = true;
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if address_line {
                        // As Outlook's To… button: the Address Book, choosing for this line.
                        let button = egui::Button::new(egui::RichText::new(format!("{label}…")).font(font(12.5))).min_size(vec2(58.0, 24.0));
                        if ui.add(button).on_hover_text("Choose from the Address Book").clicked() {
                            acts.push(("people.addressBook", json!({ "for": key })));
                        }
                    } else {
                        ui.add_sized(vec2(58.0, 24.0), egui::Label::new(egui::RichText::new(*label).font(font(12.5)).color(t.text_dim)));
                    }
                    let edit = crate::line_edit::LineEdit::new(value, id).font(font(13.0));
                    if ui.add(edit).changed() {
                        acts.push(("compose.setField", json!({ "field": key, "value": value })));
                        if address_line {
                            typed = Some((*key, value.clone()));
                        }
                    }
                });
                if let Some(s) = offered {
                    ui.horizontal(|ui| {
                        ui.add_space(66.0);
                        egui::Frame::new().fill(t.surface).stroke(egui::Stroke::new(1.0, t.line)).corner_radius(4.0).inner_margin(4.0).show(
                            ui,
                            |ui| {
                                ui.vertical(|ui| {
                                    for (i, row) in s.rows.iter().enumerate() {
                                        let words =
                                            if row.group { format!("{} (group)", row.name) } else { format!("{}  {}", row.name, row.address) };
                                        let chosen = i == pick;
                                        let r = ui.add(egui::Button::selectable(chosen, crate::seen_line(&words, font(12.5), t.text)));
                                        if r.clicked() {
                                            pick = i;
                                            take = true;
                                        }
                                    }
                                });
                            },
                        );
                    });
                    if take && let Some(row) = s.rows.get(pick) {
                        let now = crate::address_book::completed(value, &row.as_);
                        acts.push(("compose.setField", json!({ "field": key, "value": now })));
                        took = true;
                    } else if pick != s.pick {
                        typed = None;
                        acts.push(("ui.suggestPick", json!({ "pick": pick })));
                    }
                }
            }
            if !attachments.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for (i, (name, size)) in attachments.iter().enumerate() {
                        let label = format!("{}  ({})  ✕", one_line(name), human_size(*size));
                        if ui.add(egui::Button::new(crate::seen_line(&label, font(12.0), t.text))).on_hover_text("Remove this attachment").clicked() {
                            acts.push(("compose.removeAttachment", json!({ "index": i })));
                        }
                    }
                });
            }
            ui.separator();
            ScrollArea::vertical().id_salt("compose-body").auto_shrink(false).show(ui, |ui| {
                let edit = egui::TextEdit::multiline(&mut body)
                    .font(font(13.5))
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY)
                    .desired_rows(16)
                    .id(egui::Id::new(("compose", "body")));
                if ui.add(edit).changed() {
                    acts.push(("compose.setField", json!({ "field": "body", "value": body })));
                }
            });
        });
        self.go_at_text = go_text;
        self.expire_at_text = expire_text;
        for (id, params) in acts {
            if id == "ui.suggestPick" {
                if let (Some(s), Some(n)) = (self.suggest.as_mut(), params["pick"].as_u64().and_then(|n| usize::try_from(n).ok())) {
                    s.pick = n;
                }
                continue;
            }
            self.act(id, params);
        }
        if took {
            self.suggest = None;
        } else if let Some((key, line)) = typed {
            self.suggest_for(key, &line);
        }
    }
}

/// The compose window's Delivery menu: when it may go (Delay Delivery) and when it expires,
/// each from a few choices that are usually wanted or typed in.
fn delivery_menu(
    ui: &mut Ui,
    t: &theme::Tokens,
    now: i64,
    tz: &Zone,
    (go_text, expire_text): (&mut String, &mut String),
    (going_later, expiring): (bool, bool),
    acts: &mut Vec<(&'static str, Value)>,
) {
    const DAY: i64 = 864_000_000_000;
    // "2026-10-17 08:00": `days` from today, at `hour`, on this machine's clock.
    let day_at = |days: i64, hour: u32| {
        let c = tz.civil(now.saturating_add(days.saturating_mul(DAY)));
        format!("{}-{:02}-{:02} {hour:02}:00", c.year, c.month, c.day)
    };
    let today = tz.civil(now);
    // Monday next, never today.
    let to_monday = match (8 - i64::from(today.weekday.min(6))) % 7 {
        0 => 7,
        n => n,
    };
    ui.set_min_width(250.0);
    let heading = |ui: &mut Ui, words: &str| {
        ui.label(egui::RichText::new(words).font(bold(12.0)).color(t.text_dim));
    };
    heading(ui, "Do not deliver before");
    let mut picks: Vec<(&str, Value)> = vec![
        ("In 1 hour", json!({ "inMinutes": 60 })),
        ("Tomorrow at 8:00 AM", json!({ "at": day_at(1, 8) })),
        ("Monday at 8:00 AM", json!({ "at": day_at(to_monday, 8) })),
    ];
    if going_later {
        picks.push(("Send at once", json!({ "off": true })));
    }
    for (label, params) in picks {
        if ui.button(label).clicked() {
            acts.push(("compose.delayDelivery", params));
            ui.close();
        }
    }
    typed_time(ui, t, go_text, egui::Id::new("deliver-at"), "compose.delayDelivery", acts);
    ui.separator();
    heading(ui, "Expires after");
    let mut picks: Vec<(&str, Value)> = Vec::new();
    if today.hour < 17 {
        picks.push(("Today at 5:00 PM", json!({ "at": day_at(0, 17) })));
    }
    picks.extend([("In 1 day", json!({ "inMinutes": 24 * 60 })), ("In 1 week", json!({ "inMinutes": 7 * 24 * 60 }))]);
    if expiring {
        picks.push(("Never", json!({ "off": true })));
    }
    for (label, params) in picks {
        if ui.button(label).clicked() {
            acts.push(("compose.expires", params));
            ui.close();
        }
    }
    typed_time(ui, t, expire_text, egui::Id::new("expire-at"), "compose.expires", acts);
}

/// A date and time typed in, set with Enter or Set.
fn typed_time(ui: &mut Ui, t: &theme::Tokens, text: &mut String, id: egui::Id, cmd: &'static str, acts: &mut Vec<(&'static str, Value)>) {
    ui.horizontal(|ui| {
        let edit = crate::line_edit::LineEdit::new(text, id)
            .font(font(12.5))
            .hint("2026-10-17 09:00", Some(t.text_faint))
            .char_limit(32)
            .desired_width(150.0);
        let resp = ui.add(edit);
        let entered = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.add_enabled(!text.trim().is_empty(), egui::Button::new("Set")).clicked() || entered) && !text.trim().is_empty() {
            acts.push((cmd, json!({ "at": text.trim() })));
            text.clear();
            ui.close();
        }
    });
}

/// Children of `parent` in pane order: special folders first, then by name.
fn children(folders: &[FolderInfo], parent: Option<FolderId>) -> Vec<&FolderInfo> {
    // A hidden folder is the mail program's own (quick steps, conversation actions), as Outlook
    // leaves it out of its folder pane.
    let mut out: Vec<&FolderInfo> = folders.iter().filter(|f| f.parent == parent && f.kind != FolderKind::Root && !f.hidden).collect();
    out.sort_by(|a, b| a.kind.sort_rank().cmp(&b.kind.sort_rank()).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

/// What every row of the message list is drawn with.
struct Shown<'a> {
    now: i64,
    now_day: i64,
    tz: &'a Zone,
    region: &'a time::Region,
    /// The folder holds mail going out: a row says whom it went to.
    outgoing: bool,
}

fn draw_row(p: &egui::Painter, r: Rect, row: &Summary, shown: &Shown, t: &theme::Tokens) {
    let Shown { now, now_day, tz, region, outgoing } = *shown;
    // Of no use any more: struck through, as recipients' mail programs show it too.
    let expired = row.expires > 0 && row.expires <= now;
    // Waiting in the Outbox for its time (Delay Delivery): when it goes stands where the date is.
    let held = row.deliver_after > now;
    p.hline(r.x_range(), r.bottom() - 0.5, (1.0, t.line.gamma_multiply(0.6)));
    if !row.read {
        p.rect_filled(Rect::from_min_size(r.min + vec2(0.0, 1.0), vec2(3.0, r.height() - 2.0)), 0.0, t.accent);
    }
    let left = r.left() + 14.0;
    let mut right = r.right() - 10.0;

    // Line 1: who, with the status icons on the right.
    if row.flag != Flag::None {
        let done = row.flag == Flag::Complete;
        right -= 14.0;
        icons::paint(
            p,
            Rect::from_min_size(pos2(right, r.top() + 9.0), vec2(13.0, 13.0)),
            if done { Icon::Check } else { Icon::Flag },
            if done { t.ok } else { t.flag },
        );
        right -= 5.0;
    }
    if row.has_attachments {
        right -= 14.0;
        icons::paint(p, Rect::from_min_size(pos2(right, r.top() + 9.0), vec2(13.0, 13.0)), Icon::Clip, t.text_dim);
        right -= 5.0;
    }
    if row.importance == 2 {
        right -= 12.0;
        icons::paint(p, Rect::from_min_size(pos2(right, r.top() + 9.0), vec2(13.0, 13.0)), Icon::Importance, t.danger);
        right -= 5.0;
    }
    let who = if (outgoing && !row.to.trim().is_empty()) || row.from.is_empty() { &row.to } else { &row.from };
    let who_w = clip_text(p, pos2(left, r.top() + 6.0), right - left, who, if row.read { font(13.5) } else { bold(13.5) }, t.text);
    let strike = |y: f32, w: f32| {
        if expired && w > 0.0 {
            p.hline(left..=left + w, r.top() + y, (1.0, t.text_dim));
        }
    };
    strike(15.0, who_w);

    // Line 2: subject and when.
    let shown_time = if held { row.deliver_after } else { row.time };
    let c = tz.civil(shown_time);
    let when = if shown_time <= 0 {
        String::new()
    } else if held {
        let day = match c.day_number.saturating_sub(now_day) {
            i64::MIN..=0 => String::new(),
            1 => "tomorrow ".to_string(),
            2..=6 => format!("{} ", c.weekday_name().get(..3).unwrap_or("")),
            _ => format!("{} ", region.date(&c)),
        };
        format!("Goes {day}{}", region.time(&c))
    } else {
        match now_day.saturating_sub(c.day_number) {
            i64::MIN..=0 => region.time(&c),
            1..=6 => format!("{} {}", c.weekday_name().get(..3).unwrap_or(""), region.time(&c)),
            _ => region.date(&c),
        }
    };
    let when_w = crate::text_width(p, &when, font(11.5));
    p.text(pos2(r.right() - 10.0, r.top() + 34.0), Align2::RIGHT_CENTER, &when, font(11.5), if row.read { t.text_faint } else { t.accent });
    let subject = if row.subject.trim().is_empty() { "(no subject)" } else { &row.subject };
    let subject_w = clip_text(
        p,
        pos2(left, r.top() + 25.0),
        r.right() - 22.0 - when_w - left,
        subject,
        if row.read { font(12.5) } else { bold(12.5) },
        if row.read { t.text } else { t.accent },
    );
    strike(34.0, subject_w);

    // Line 3: preview, with category squares on the right.
    let mut cat_x = r.right() - 10.0;
    for category in row.categories.iter().take(4) {
        cat_x -= 11.0;
        p.rect_filled(Rect::from_min_size(pos2(cat_x, r.top() + 46.0), vec2(9.0, 9.0)), 2.0, theme::category_color(category));
        cat_x -= 3.0;
    }
    // A body that the preview holds whole ends with an end mark, as Outlook's list shows.
    let preview_w = clip_text(p, pos2(left, r.top() + 43.0), cat_x - left - 4.0, &row.preview, font(12.0), t.text_faint);
    let end_room = cat_x - 4.0 - (left + preview_w + 4.0);
    if row.preview_whole && !row.preview.is_empty() && end_room >= text_width(p, "<end>", font(12.0)) {
        p.text(pos2(left + preview_w + 4.0, r.top() + 43.0), Align2::LEFT_TOP, "<end>", font(12.0), t.text_faint);
    }
}
