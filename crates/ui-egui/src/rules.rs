//! The Rules page of Options (Home > Move > Rules): the rules in the order they are tried,
//! each turned on and off, moved up and down and deleted; one rule written on at a time, its
//! conditions, exceptions and actions; and Run Rules Now on the current folder, looked at
//! first. Every change goes through its `rules.*` command.

use crate::MailUi;
use crate::line_edit::LineEdit;
use egui::Ui;
use openmailclient_engine::rules::{MAX_NAME, MAX_PARTS, MAX_WORD};
use openmailclient_model::FolderKind as Kind;
use serde_json::{Value, json};

/// What a part asks to be filled in with.
#[derive(Clone, Copy, PartialEq)]
enum Fill {
    Nothing,
    /// Words to look for, separated by semicolons.
    Words,
    Level,
    /// Kilobytes, at least and at most.
    Size,
    Folder,
    Name,
    /// Days, on or after and on or before, YYYY-MM-DD.
    Days,
    /// Addresses, separated by semicolons.
    Addresses,
    /// Words of mine, on several lines.
    Text,
    Minutes,
}

const TESTS: &[(&str, &str, Fill)] = &[
    ("from", "From someone with", Fill::Words),
    ("sentTo", "Sent to someone with", Fill::Words),
    ("subject", "Subject has", Fill::Words),
    ("body", "Body has", Fill::Words),
    ("subjectOrBody", "Subject or body has", Fill::Words),
    ("header", "Message header has", Fill::Words),
    ("toMe", "Where my address is in the To box", Fill::Nothing),
    ("ccMe", "Where my address is in the Cc box", Fill::Nothing),
    ("toOrCcMe", "Where my address is in To or Cc", Fill::Nothing),
    ("onlyToMe", "Sent only to me", Fill::Nothing),
    ("hasAttachment", "Which has an attachment", Fill::Nothing),
    ("importance", "Marked as importance", Fill::Level),
    ("size", "With a size in KB", Fill::Size),
    ("meeting", "Which is a meeting request or answer", Fill::Nothing),
    ("senderAddress", "With words in the sender's address", Fill::Words),
    ("recipientAddress", "With words in the recipient's address", Fill::Words),
    ("received", "Received in a span of days", Fill::Days),
];

const ACTS: &[(&str, &str, Fill)] = &[
    ("move", "Move it to", Fill::Folder),
    ("copy", "Copy it to", Fill::Folder),
    ("delete", "Put it in Deleted Items", Fill::Nothing),
    ("markRead", "Mark it as read", Fill::Nothing),
    ("flag", "Flag it for follow-up today", Fill::Nothing),
    ("clearFlag", "Clear its flag", Fill::Nothing),
    ("category", "Put it in the category", Fill::Name),
    ("importance", "Mark it as importance", Fill::Level),
    ("stop", "Stop processing more rules", Fill::Nothing),
    ("forward", "Forward it to", Fill::Addresses),
    ("reply", "Reply to the sender with", Fill::Text),
    ("sound", "Play a sound", Fill::Nothing),
    ("defer", "Defer delivery by minutes", Fill::Minutes),
];

const LEVELS: [&str; 3] = ["low", "normal", "high"];

/// Whether an action may be asked of a rule on mail as it is sent (`sent`) or as it arrives:
/// what went stays in Sent Items and answers no one, and only what is sent is deferred.
fn allowed(kind: &str, sent: bool) -> bool {
    if sent { !matches!(kind, "move" | "delete" | "markRead" | "forward" | "reply") } else { kind != "defer" }
}

/// One condition, exception or action as it is written on.
#[derive(Clone, Default)]
struct Part {
    kind: String,
    text: String,
    level: usize,
    min: String,
    max: String,
}

impl Part {
    fn of(v: &Value, key: &str) -> Part {
        let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let words: Vec<&str> = v.get("words").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        let n = |k: &str| v.get(k).and_then(Value::as_u64).filter(|&n| n > 0).map(|n| n.to_string()).unwrap_or_default();
        let kind = text(key);
        if kind == "received" {
            return Part { kind, min: text("after"), max: text("before"), level: 1, ..Part::default() };
        }
        if kind == "defer" {
            return Part { kind, min: n("minutes"), level: 1, ..Part::default() };
        }
        let to: Vec<&str> = v.get("to").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        let filled = match kind.as_str() {
            "forward" => to.join("; "),
            "reply" => text("words"),
            "category" => text("name"),
            _ if words.is_empty() => text("folder"),
            _ => words.join("; "),
        };
        Part { kind, text: filled, level: LEVELS.iter().position(|l| *l == text("level")).unwrap_or(1), min: n("min"), max: n("max") }
    }

    fn json(&self, key: &str, fill: Fill) -> Value {
        let mut v = json!({ key: self.kind });
        match fill {
            Fill::Nothing => {}
            Fill::Words => v["words"] = json!(self.text.split(';').map(str::trim).filter(|w| !w.is_empty()).collect::<Vec<_>>()),
            Fill::Level => v["level"] = json!(LEVELS.get(self.level).copied().unwrap_or("normal")),
            Fill::Size => {
                v["min"] = json!(self.min.trim().parse::<u64>().unwrap_or(0));
                v["max"] = json!(self.max.trim().parse::<u64>().unwrap_or(0));
            }
            Fill::Folder => v["folder"] = json!(self.text),
            Fill::Name => v["name"] = json!(self.text.trim()),
            Fill::Days => {
                v["after"] = json!(self.min.trim());
                v["before"] = json!(self.max.trim());
            }
            Fill::Addresses => v["to"] = json!(self.text.split([';', ',']).map(str::trim).filter(|a| !a.is_empty()).collect::<Vec<_>>()),
            Fill::Text => v["words"] = json!(self.text),
            Fill::Minutes => v["minutes"] = json!(self.min.trim().parse::<u64>().unwrap_or(0)),
        }
        v
    }
}

/// The rule being written on, and what Run Rules Now last said.
#[derive(Default)]
pub(crate) struct RuleEdit {
    /// The rule's place in the order; none for a new one.
    index: Option<usize>,
    name: String,
    on: bool,
    account: Option<String>,
    /// On mail as it is sent, not as it arrives.
    sent: bool,
    when: Vec<Part>,
    except: Vec<Part>,
    then: Vec<Part>,
    /// Why the last save was refused.
    refused: Option<String>,
    /// Run Rules Now's look, or what it did.
    run: Option<Value>,
    subfolders: bool,
}

impl RuleEdit {
    fn of(index: Option<usize>, v: &Value) -> RuleEdit {
        let list = |key: &str, kind: &str| v.get(key).and_then(Value::as_array).into_iter().flatten().map(|p| Part::of(p, kind)).collect();
        RuleEdit {
            index,
            name: v.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            on: v.get("on").and_then(Value::as_bool).unwrap_or(true),
            account: v.get("account").and_then(Value::as_str).map(str::to_string),
            sent: v.get("sent").and_then(Value::as_bool).unwrap_or(false),
            when: list("when", "test"),
            except: list("except", "test"),
            then: list("then", "do"),
            ..RuleEdit::default()
        }
    }

    fn json(&self) -> Value {
        let parts = |list: &[Part], key: &str, table: &[(&str, &str, Fill)]| -> Vec<Value> {
            list.iter().map(|p| p.json(key, table.iter().find(|(k, _, _)| *k == p.kind).map_or(Fill::Nothing, |t| t.2))).collect()
        };
        json!({
            "name": self.name,
            "on": self.on,
            "account": self.account,
            "sent": self.sent,
            "when": parts(&self.when, "test", TESTS),
            "except": parts(&self.except, "test", TESTS),
            "then": parts(&self.then, "do", ACTS),
        })
    }
}

/// Parts of one list, each with its own fields and a way to take it away, and a menu to add one.
fn parts(ui: &mut Ui, t: &crate::theme::Tokens, salt: &str, list: &mut Vec<Part>, table: &[(&str, &str, Fill)], folders: &[String]) {
    let mut gone = None;
    for (i, p) in list.iter_mut().enumerate() {
        let Some(&(_, label, fill)) = table.iter().find(|(k, _, _)| *k == p.kind) else { continue };
        ui.horizontal(|ui| {
            if ui.small_button("×").on_hover_text("Take this away").clicked() {
                gone = Some(i);
            }
            ui.label(egui::RichText::new(label).color(t.text));
            let id = egui::Id::new((salt, i));
            match fill {
                Fill::Nothing => {}
                Fill::Words => {
                    ui.add(
                        LineEdit::new(&mut p.text, id)
                            .desired_width(220.0)
                            .char_limit(MAX_WORD.saturating_mul(4))
                            .hint("words; separated; by semicolons", None),
                    );
                }
                Fill::Name => {
                    ui.add(LineEdit::new(&mut p.text, id).desired_width(160.0).char_limit(255).hint("Category", None));
                }
                Fill::Level => {
                    egui::ComboBox::from_id_salt(id).selected_text(LEVELS.get(p.level).copied().unwrap_or("normal")).width(90.0).show_ui(ui, |ui| {
                        for (n, l) in LEVELS.iter().enumerate() {
                            ui.selectable_value(&mut p.level, n, *l);
                        }
                    });
                }
                Fill::Size => {
                    ui.add(LineEdit::new(&mut p.min, id.with("min")).desired_width(60.0).char_limit(10).hint("at least", None));
                    ui.label(egui::RichText::new("to").color(t.text_dim));
                    ui.add(LineEdit::new(&mut p.max, id.with("max")).desired_width(60.0).char_limit(10).hint("any", None));
                }
                Fill::Minutes => {
                    ui.add(LineEdit::new(&mut p.min, id).desired_width(60.0).char_limit(3).hint("1 to 120", None));
                }
                Fill::Days => {
                    ui.add(LineEdit::new(&mut p.min, id.with("after")).desired_width(90.0).char_limit(10).hint("from YYYY-MM-DD", None));
                    ui.label(egui::RichText::new("to").color(t.text_dim));
                    ui.add(LineEdit::new(&mut p.max, id.with("before")).desired_width(90.0).char_limit(10).hint("any day", None));
                }
                Fill::Addresses => {
                    ui.add(LineEdit::new(&mut p.text, id).desired_width(260.0).char_limit(2000).hint("name@example.org; another@example.org", None));
                }
                Fill::Text => {
                    ui.add(
                        egui::TextEdit::multiline(&mut p.text)
                            .id(id)
                            .desired_width(260.0)
                            .desired_rows(3)
                            .char_limit(8000)
                            .hint_text("What the reply says"),
                    );
                }
                Fill::Folder => {
                    let shown = if p.text.is_empty() { "Choose a folder" } else { p.text.as_str() };
                    egui::ComboBox::from_id_salt(id).selected_text(shown).width(200.0).show_ui(ui, |ui| {
                        for f in folders {
                            ui.selectable_value(&mut p.text, f.clone(), f);
                        }
                    });
                }
            }
        });
    }
    if let Some(i) = gone {
        list.remove(i);
    }
    if list.len() < MAX_PARTS {
        ui.menu_button(egui::RichText::new("Add…").color(t.accent), |ui| {
            for (kind, label, _) in table {
                if ui.button(*label).clicked() {
                    list.push(Part { kind: (*kind).to_string(), level: 1, ..Part::default() });
                    ui.close();
                }
            }
        });
    }
}

impl MailUi {
    /// Options at the Rules page, with a rule made from the selected message where `create`
    /// is asked, or what Run Rules Now would do where `runNow` is.
    pub(crate) fn rules_command(&mut self, params: &Value) -> Result<(), String> {
        if params.get("create").and_then(Value::as_bool) == Some(true) {
            let made = self.execute("rules.create", &Value::Null)?;
            self.rule_edit = RuleEdit::of(None, made.get("rule").unwrap_or(&Value::Null));
        }
        if params.get("runNow").and_then(Value::as_bool) == Some(true) {
            let subfolders = self.rule_edit.subfolders;
            self.rule_edit.run = Some(self.execute("rules.runNow", &json!({ "preview": true, "subfolders": subfolders }))?);
        }
        Ok(())
    }

    pub(crate) fn rules_page(&mut self, ui: &mut Ui) {
        let t = self.tokens.clone();
        let listed = self.session.run("rules.list", &Value::Null).unwrap_or_default();
        let rules: Vec<Value> = listed.get("rules").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut ask: Option<(&'static str, Value)> = None;
        let mut open: Option<usize> = None;

        crate::dialog::heading(ui, &t, "Rules, in the order they are tried");
        egui::ScrollArea::vertical().id_salt("rules-list").max_height(150.0).show(ui, |ui| {
            if rules.is_empty() {
                ui.label(egui::RichText::new("No rules yet. A rule moves, flags or sorts mail as it comes in.").color(t.text_faint));
            }
            for (i, r) in rules.iter().enumerate() {
                let name = r.get("name").and_then(Value::as_str).unwrap_or("");
                let mut on = r.get("on").and_then(Value::as_bool).unwrap_or(true);
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut on, "").on_hover_text("Turn this rule on or off").changed() {
                        ask = Some(("rules.turn", json!({ "index": i, "on": on })));
                    }
                    let chosen = self.rule_edit.index == Some(i);
                    if ui.selectable_label(chosen, name).clicked() {
                        open = Some(i);
                    }
                });
                if let Some(words) = r.get("words").and_then(Value::as_str) {
                    ui.add(egui::Label::new(egui::RichText::new(words).font(crate::font(11.0)).color(t.text_dim)).wrap());
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.add(crate::dialog::plain("New Rule")).clicked() {
                self.rule_edit = RuleEdit { on: true, then: vec![Part { kind: "move".into(), level: 1, ..Part::default() }], ..RuleEdit::default() };
            }
            if ui.add_enabled(self.session.is_enabled("rules.create"), crate::dialog::plain("From Selected Message")).clicked() {
                ask = Some(("ui.options", json!({ "page": "rules", "create": true })));
            }
            if let Some(i) = self.rule_edit.index {
                if ui.add_enabled(i > 0, crate::dialog::plain("Move Up")).clicked() {
                    ask = Some(("rules.move", json!({ "index": i, "to": i - 1 })));
                    self.rule_edit.index = Some(i - 1);
                }
                if ui.add_enabled(i + 1 < rules.len(), crate::dialog::plain("Move Down")).clicked() {
                    ask = Some(("rules.move", json!({ "index": i, "to": i + 1 })));
                    self.rule_edit.index = Some(i + 1);
                }
                if ui.add(crate::dialog::plain("Delete")).clicked() {
                    ask = Some(("rules.delete", json!({ "index": i })));
                    self.rule_edit = RuleEdit::default();
                }
            }
        });
        if let Some(i) = open
            && let Some(r) = rules.get(i)
        {
            let run = self.rule_edit.run.take();
            self.rule_edit = RuleEdit { run, ..RuleEdit::of(Some(i), r) };
        }

        let writing = self.rule_edit.index.is_some() || !self.rule_edit.then.is_empty() || !self.rule_edit.name.is_empty();
        if writing {
            ui.add_space(8.0);
            let folders: Vec<String> = match self.session.current {
                Some((store, _)) => {
                    let all = self.session.folders.get(store).map(Vec::as_slice).unwrap_or_default();
                    all.iter()
                        .filter(|f| {
                            !f.hidden
                                && !matches!(
                                    f.kind,
                                    Kind::Root
                                        | Kind::Calendar
                                        | Kind::Contacts
                                        | Kind::Tasks
                                        | Kind::Notes
                                        | Kind::Journal
                                        | Kind::Search
                                        | Kind::Other
                                )
                        })
                        .map(|f| crate::ribbon::folder_path(all, f.id))
                        .collect()
                }
                None => Vec::new(),
            };
            let accounts: Vec<(String, String)> = self.session.accounts.iter().map(|a| (a.id.clone(), a.name.clone())).collect();
            crate::dialog::card(ui, &t, |ui| {
                let e = &mut self.rule_edit;
                ui.horizontal(|ui| {
                    ui.add(LineEdit::new(&mut e.name, egui::Id::new("rule-name")).desired_width(240.0).char_limit(MAX_NAME).hint("Rule name", None));
                    let shown = e.account.as_ref().and_then(|id| accounts.iter().find(|(a, _)| a == id)).map_or("Every account", |(_, n)| n.as_str());
                    egui::ComboBox::from_id_salt("rule-account").selected_text(shown).width(170.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut e.account, None, "Every account");
                        for (id, name) in &accounts {
                            ui.selectable_value(&mut e.account, Some(id.clone()), name);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    let was = e.sent;
                    ui.radio_value(&mut e.sent, false, "After the message arrives");
                    ui.radio_value(&mut e.sent, true, "After I send the message");
                    if e.sent != was {
                        let sent = e.sent;
                        e.then.retain(|p| allowed(&p.kind, sent));
                    }
                });
                ui.add_space(4.0);
                let when = if e.sent {
                    "When I send a message (every one, where nothing is asked)"
                } else {
                    "When a message comes in (every one, where nothing is asked)"
                };
                ui.label(egui::RichText::new(when).font(crate::bold(12.0)).color(t.text));
                parts(ui, &t, "rule-when", &mut e.when, TESTS, &folders);
                ui.label(egui::RichText::new("Do this").font(crate::bold(12.0)).color(t.text));
                let sent = e.sent;
                let acts: Vec<(&str, &str, Fill)> = ACTS.iter().filter(|(k, _, _)| allowed(k, sent)).copied().collect();
                parts(ui, &t, "rule-then", &mut e.then, &acts, &folders);
                ui.label(egui::RichText::new("Except where").font(crate::bold(12.0)).color(t.text));
                parts(ui, &t, "rule-except", &mut e.except, TESTS, &folders);
            });
            if let Some(why) = self.rule_edit.refused.clone() {
                crate::dialog::callout(ui, &t, t.danger, |ui| {
                    ui.label(egui::RichText::new(why).color(t.text));
                });
            }
            ui.horizontal(|ui| {
                if ui.add(crate::dialog::primary(&t, "Save Rule")).clicked() {
                    let mut p = json!({ "rule": self.rule_edit.json() });
                    if let Some(i) = self.rule_edit.index {
                        p["index"] = json!(i);
                    }
                    match self.execute("rules.save", &p) {
                        Ok(saved) => {
                            self.rule_edit.index = saved.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok());
                            self.rule_edit.refused = None;
                        }
                        Err(why) => self.rule_edit.refused = Some(why),
                    }
                }
                ui.label(
                    egui::RichText::new("A rule never deletes for good: what it deletes is in Deleted Items.")
                        .font(crate::font(11.0))
                        .color(t.text_faint),
                );
            });
        }

        ui.add_space(8.0);
        crate::dialog::heading(ui, &t, "Run Rules Now");
        let here = self.session.current_folder().map(|f| f.name.clone()).unwrap_or_else(|| "the Inbox".into());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("On {here}")).color(t.text));
            ui.checkbox(&mut self.rule_edit.subfolders, "and its subfolders");
            let can = self.session.is_enabled("rules.runNow");
            if ui.add_enabled(can, crate::dialog::plain("Look First")).clicked() {
                ask = Some(("ui.options", json!({ "page": "rules", "runNow": true })));
            }
            if ui.add_enabled(can, crate::dialog::primary(&t, "Run")).clicked() {
                let subfolders = self.rule_edit.subfolders;
                self.rule_edit.run =
                    Some(self.execute("rules.runNow", &json!({ "subfolders": subfolders })).unwrap_or_else(|e| json!({ "trouble": [e] })));
            }
        });
        if let Some(run) = &self.rule_edit.run {
            let n = |k: &str| run.get(k).and_then(Value::as_u64).unwrap_or(0);
            let previewed = run.get("messages").is_some();
            let said = if previewed {
                format!("The rules would take {} of the {} messages looked at; nothing is changed until Run.", n("matched"), n("looked"))
            } else {
                format!("The rules took {} of {} messages, and moved {}.", n("matched"), n("looked"), n("moved"))
            };
            ui.label(egui::RichText::new(said).color(t.text_dim));
            egui::ScrollArea::vertical().id_salt("rules-run").max_height(120.0).show(ui, |ui| {
                for m in run.get("messages").and_then(Value::as_array).into_iter().flatten() {
                    let s = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("");
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(egui::RichText::new(s("subject")).color(t.text)).truncate());
                        ui.label(egui::RichText::new(format!("— {}", s("does"))).font(crate::font(11.0)).color(t.text_dim));
                    });
                }
                for why in run.get("trouble").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                    ui.label(egui::RichText::new(why).font(crate::font(11.0)).color(t.danger));
                }
            });
        }
        if let Some((id, p)) = ask {
            self.act(id, p);
        }
    }
}
