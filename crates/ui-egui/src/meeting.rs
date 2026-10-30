//! Meetings in the window: who is asked, typed by role in the appointment window, what each has
//! answered (Tracking), and the bar above a meeting's request, answer or cancellation in the
//! reading pane, from which it is answered. Everything is done by the `calendar.*` commands.

use crate::MailUi;
use openmailclient_model::meeting::{self, Attendee, Meeting, Response, Role};
use openmailclient_model::pid;
use openmailclient_model::time::{self, Civil};
use serde_json::{Value, json};

/// A person as typed in a line of addresses: `"Name" <address>`, or the address alone.
pub(crate) fn person_text(p: &Attendee) -> String {
    let name: String = p.name.chars().filter(|c| !matches!(c, '"' | '<' | '>') && !c.is_control()).collect();
    let name = name.trim();
    if name.is_empty() || name.eq_ignore_ascii_case(&p.address) {
        p.address.clone()
    } else if name.contains([',', ';', '@', '(', ')']) {
        format!("\"{name}\" <{}>", p.address)
    } else {
        format!("{name} <{}>", p.address)
    }
}

/// Those asked, as the lines by role the window types them in: required, optional, resources.
pub(crate) fn lines(m: Option<&Meeting>) -> [String; 3] {
    let line =
        |role: Role| m.map(|m| m.attendees.iter().filter(|p| p.role == role).map(person_text).collect::<Vec<_>>().join("; ")).unwrap_or_default();
    [line(Role::Required), line(Role::Optional), line(Role::Resource)]
}

/// The lines as `calendar.new` and `calendar.change` take them.
pub(crate) fn lines_param(asked: &[String; 3]) -> Value {
    json!({ "required": asked[0].trim(), "optional": asked[1].trim(), "resources": asked[2].trim() })
}

/// What has come back, in a line: "2 accepted, 1 declined, 3 no response".
pub(crate) fn tally_words(m: &Meeting) -> String {
    let [accepted, tentative, declined, none] = m.tally();
    let mut out: Vec<String> = Vec::new();
    for (n, what) in [(accepted, "accepted"), (tentative, "tentative"), (declined, "declined"), (none, "no response")] {
        if n > 0 {
            out.push(format!("{n} {what}"));
        }
    }
    if out.is_empty() { "No one is asked".to_string() } else { out.join(", ") }
}

/// One's own answer, as the window says it.
pub(crate) fn answer_words(r: Response) -> &'static str {
    match r {
        Response::Accepted => "You accepted",
        Response::Tentative => "You tentatively accepted",
        Response::Declined => "You declined",
        Response::Organized => "You are the organizer",
        Response::None | Response::NotResponded => "You have not answered",
    }
}

fn verb(r: &str) -> &'static str {
    match r {
        "accepted" => "accepted",
        "tentative" => "tentatively accepted",
        "declined" => "declined",
        _ => "answered",
    }
}

/// The longest comment the window takes; the command cuts it to its own limit too.
const MAX_COMMENT: usize = 10_000;

/// An answer to the open invitation being written: which, with what words, and whether it is
/// sent. Beyond Outlook, the answer can still be changed here, and whether it goes is chosen
/// in the same window instead of in a menu before it.
#[derive(Clone, Debug)]
pub(crate) struct Answer {
    /// "accept", "tentative" or "decline".
    pub response: &'static str,
    pub comment: String,
    pub send: bool,
    /// The invitation it answers, known by `answer_key`, so that one opened meanwhile is not
    /// answered in its place.
    pub of: String,
    pub subject: String,
    pub error: String,
}

impl MailUi {
    /// What names the open invitation, to know it again: the message's Message-ID, subject and
    /// date, and the meeting's UID and sequence, any of which a stranger may give two alike.
    fn answer_key(&mut self) -> Option<String> {
        let item = self.session.open.as_ref()?;
        let head = format!("{}|{}|{}", item.props.str(pid::INTERNET_MESSAGE_ID).unwrap_or_default(), item.subject(), item.time());
        let v = self.session.run("calendar.invitation", &Value::Null).ok()?;
        let said = &v["untrusted_content"];
        Some(format!("{head}|{}|{}", said["uid"].as_str().unwrap_or_default(), said["sequence"]))
    }
}

/// The bar above a meeting's message, worked out once for each message shown.
#[derive(Clone, Debug, Default)]
pub(crate) struct Bar {
    pub(crate) words: String,
    /// It asks a warning's colour: out of date, or not from whom it has to come.
    pub(crate) warn: bool,
    /// Accept, Tentative and Decline are offered.
    pub(crate) answers: bool,
    /// A task request's Accept and Decline are offered.
    pub(crate) task_answers: bool,
    /// Remove from Calendar: the appointment a cancellation names.
    pub(crate) remove: Option<u64>,
    /// Show in Calendar: the day it is on, YYYY-MM-DD.
    pub(crate) day: Option<String>,
}

impl MailUi {
    /// The bar for the open message, when it is a meeting's; kept until another is open or
    /// something is done.
    pub(crate) fn meeting_bar(&mut self) -> Option<Bar> {
        let item = self.session.open.as_ref()?;
        let task_kind = openmailclient_model::task::request::Kind::of(item);
        if !meeting::is_meeting_message(item) && task_kind.is_none() {
            return None;
        }
        let key = format!(
            "{}|{}|{}|{}",
            item.props.str(pid::INTERNET_MESSAGE_ID).unwrap_or_default(),
            item.subject(),
            item.time(),
            self.session.calendar.items.len()
        );
        if let Some((k, bar)) = &self.invitation
            && *k == key
        {
            return bar.clone();
        }
        let sender = item.sender_email();
        let bar = match task_kind {
            Some(_) => self.session.run("tasks.request", &Value::Null).ok().map(|v| task_bar(&v, &item_sender(self))),
            None => self.session.run("calendar.invitation", &Value::Null).ok().map(|v| self.bar_of(&v, &sender)),
        };
        self.invitation = Some((key, bar.clone()));
        bar
    }

    fn bar_of(&mut self, v: &Value, sender: &str) -> Bar {
        let text = |v: &Value| v.as_str().map(|s| s.chars().filter(|c| !c.is_control()).take(200).collect::<String>()).unwrap_or_default();
        match v["method"].as_str().unwrap_or_default() {
            "REPLY" => {
                let who = &v["attendee"]["untrusted_content"];
                let name = if text(&who["name"]).is_empty() { text(&who["address"]) } else { text(&who["name"]) };
                let response = v["attendee"]["response"].as_str().unwrap_or_default();
                let from_them = text(&who["address"]).eq_ignore_ascii_case(sender.trim());
                if from_them {
                    Bar { words: format!("{name} has {} this meeting.", verb(response)), ..Bar::default() }
                } else {
                    Bar {
                        words: format!("This answer, for {name}, was sent by someone else, so the meeting was left as it is."),
                        warn: true,
                        ..Bar::default()
                    }
                }
            }
            "CANCEL" => {
                let here = v["inCalendar"].as_u64();
                let organizer = here.and_then(|id| self.session.meeting(id).ok().flatten()).map(|m| m.organizer.address);
                match (here, organizer) {
                    (Some(id), Some(o)) if o.eq_ignore_ascii_case(sender.trim()) => Bar {
                        words: "The organizer has called this meeting off. It is marked Canceled in your calendar.".into(),
                        remove: Some(id),
                        ..Bar::default()
                    },
                    (Some(_), _) => Bar {
                        words: "This cancellation was not sent by the meeting's organizer, so your calendar was left as it is.".into(),
                        warn: true,
                        ..Bar::default()
                    },
                    (None, _) => Bar { words: "This meeting has been called off. It is not in your calendar.".into(), ..Bar::default() },
                }
            }
            _ => {
                let day = v["start"].as_str().and_then(|s| s.get(..10)).map(str::to_string);
                if v["outOfDate"].as_bool() == Some(true) {
                    return Bar {
                        words: "This meeting request is out of date: your calendar has a later one.".into(),
                        warn: true,
                        day,
                        ..Bar::default()
                    };
                }
                let answered = v["inCalendar"].as_u64().is_some();
                let when = self.when_words(v);
                let place = text(&v["untrusted_content"]["location"]);
                let mut words = if place.is_empty() { format!("{when}. ") } else { format!("{when}, {place}. ") };
                words.push_str(match v["response"].as_str() {
                    Some("accepted") if answered => "You accepted this meeting.",
                    Some("tentative") if answered => "You tentatively accepted this meeting.",
                    _ => "Please respond. It is not in your calendar until you do.",
                });
                let clashes: Vec<String> = v["conflicts"]
                    .as_array()
                    .map(|c| c.iter().take(3).map(|c| text(&c["untrusted_content"]["subject"])).collect())
                    .unwrap_or_default();
                let more = v["conflicts"].as_array().map_or(0, Vec::len).saturating_sub(clashes.len());
                if clashes.is_empty() {
                    words.push_str(" Nothing else is planned then.");
                } else {
                    words.push_str(&format!(
                        " At the same time: {}{}.",
                        clashes.join(", "),
                        if more > 0 { format!(" and {more} more") } else { String::new() }
                    ));
                }
                Bar { words, warn: !clashes.is_empty(), answers: true, day, ..Bar::default() }
            }
        }
    }

    /// When a meeting is, as the region writes dates and times: "Wed 10/7/2026 2:00 PM to
    /// 3:00 PM"; the pattern of a series.
    fn when_words(&self, v: &Value) -> String {
        if let Some(p) = v["pattern"].as_str().filter(|p| !p.is_empty()) {
            return p.chars().filter(|c| !c.is_control()).take(200).collect();
        }
        // "YYYY-MM-DD HH:MM", as the engine writes it, read back.
        let at = |key: &str| -> Option<Civil> {
            let s = v[key].as_str()?;
            let n = |r: std::ops::Range<usize>| s.get(r).and_then(|x| x.parse::<u32>().ok());
            let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
            let (h, mi) = (n(11..13).unwrap_or(0), n(14..16).unwrap_or(0));
            Some(time::civil(time::from_civil(i64::from(y), m, d, h, mi, 0, 0), 0))
        };
        let r = &self.session.region;
        let day = |c: &Civil| format!("{} {}", c.weekday_name().get(..3).unwrap_or(""), r.date(c));
        match (at("start"), at("end")) {
            (Some(s), _) if v["allDay"].as_bool() == Some(true) => format!("{}, all day", day(&s)),
            (Some(s), Some(e)) if (s.year, s.month, s.day) == (e.year, e.month, e.day) => format!("{} {} to {}", day(&s), r.time(&s), r.time(&e)),
            (Some(s), Some(e)) => format!("{} {} to {} {}", day(&s), r.time(&s), day(&e), r.time(&e)),
            _ => "When it is could not be read".to_string(),
        }
    }

    /// `ui.answerInvitation`: the window that answers the open invitation with a comment.
    pub(crate) fn answer_invitation(&mut self, params: &Value) -> Result<(), String> {
        let response = match params.get("response").and_then(Value::as_str) {
            Some("accept") => "accept",
            Some("tentative") => "tentative",
            Some("decline") => "decline",
            _ => return Err(r#""response" is "accept", "tentative" or "decline""#.to_string()),
        };
        if !self.meeting_bar().is_some_and(|b| b.answers) {
            return Err("The message open is not an invitation that can be answered".to_string());
        }
        let of = self.answer_key().ok_or("No invitation is open")?;
        let subject = self.session.open.as_ref().map(|i| crate::one_line(&i.subject()).chars().take(200).collect()).unwrap_or_default();
        self.answer = Some(Answer { response, comment: String::new(), send: true, of, subject, error: String::new() });
        Ok(())
    }

    pub(crate) fn answer_window(&mut self, ctx: &egui::Context) {
        let Some(mut a) = self.answer.clone() else { return };
        let t = self.tokens.clone();
        let mut go = false;
        let mut close = false;
        let title = match a.response {
            "accept" => "Accept",
            "tentative" => "Tentative",
            _ => "Decline",
        };
        let shown = crate::dialog::Dialog::new("answer-invitation", title, crate::icons::Icon::NewMeeting).width(460.0).show(ctx, &t, |ui| {
            ui.label(egui::RichText::new(&a.subject).font(crate::bold(13.0)).color(t.text));
            ui.horizontal(|ui| {
                ui.radio_value(&mut a.response, "accept", "Accept");
                ui.radio_value(&mut a.response, "tentative", "Tentative");
                ui.radio_value(&mut a.response, "decline", "Decline");
            });
            ui.add_space(4.0);
            ui.label(egui::RichText::new("A few words to the organizer").font(crate::font(12.5)).color(t.text_dim));
            ui.add_enabled(
                a.send,
                egui::TextEdit::multiline(&mut a.comment)
                    .id(egui::Id::new("answer-comment"))
                    .desired_rows(5)
                    .desired_width(f32::INFINITY)
                    .char_limit(MAX_COMMENT),
            );
            if !a.error.is_empty() {
                crate::dialog::callout(ui, &t, t.danger, |ui| {
                    ui.label(egui::RichText::new(&a.error).font(crate::font(12.5)).color(t.text));
                });
            }
            crate::dialog::foot(ui, &t, |ui| {
                ui.checkbox(&mut a.send, "Send the answer").on_hover_text("Unticked, your calendar is changed and the organizer is not told");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(crate::dialog::plain("Cancel")).clicked() {
                        close = true;
                    }
                    if ui.add(crate::dialog::primary(&t, if a.send { "Send" } else { "Answer" })).clicked() {
                        go = true;
                    }
                });
            });
        });
        if go {
            a.error = self.send_answer(&a).err().unwrap_or_default();
            close |= a.error.is_empty();
        }
        self.answer = if shown.closed || close { None } else { Some(a) };
    }

    /// The answer written given, to the invitation it was written for only.
    pub(crate) fn send_answer(&mut self, a: &Answer) -> Result<(), String> {
        if self.answer_key().as_deref() != Some(a.of.as_str()) {
            return Err("The invitation is no longer open: open it again to answer it".to_string());
        }
        let mut p = json!({ "response": a.response, "send": a.send });
        if a.send && !a.comment.trim().is_empty() {
            p["comment"] = json!(a.comment);
        }
        self.session.run("calendar.respond", &p).map_err(|e| e.to_string())?;
        self.invitation = None;
        Ok(())
    }

    /// The bar drawn: its words, and what it offers, as commands to run.
    pub(crate) fn meeting_bar_ui(&self, ui: &mut egui::Ui, bar: &Bar, acts: &mut Vec<(&'static str, Value)>) {
        let t = &self.tokens;
        egui::Frame::NONE.fill(if bar.warn { t.info_bar } else { t.surface }).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(&bar.words).font(crate::font(12.0)).color(t.text));
            });
            ui.horizontal(|ui| {
                if bar.task_answers {
                    for (label, response, hint) in [
                        ("Accept", "accept", "Put it in your tasks and tell the one who gave it"),
                        ("Decline", "decline", "Tell the one who gave it you will not do it"),
                    ] {
                        if ui.add(crate::dialog::plain(label)).on_hover_text(hint).clicked() {
                            acts.push(("tasks.respond", json!({ "response": response })));
                        }
                    }
                }
                if bar.answers {
                    for (label, response, hint) in [
                        ("Accept", "accept", "Put it in your calendar and tell the organizer"),
                        ("Tentative", "tentative", "Put it in your calendar as tentative and tell the organizer"),
                        ("Decline", "decline", "Tell the organizer you will not come; it stays out of your calendar"),
                    ] {
                        // As Outlook asks: a comment first, the answer at once, or no answer
                        // sent, on a click or a right-click alike.
                        // Room at the right for a chevron drawn in, as the ribbon's menus have.
                        let r = ui.add(crate::dialog::plain(&format!("{label}    "))).on_hover_text(hint);
                        let at = egui::Rect::from_center_size(egui::pos2(r.rect.right() - 13.0, r.rect.center().y), egui::vec2(9.0, 9.0));
                        crate::icons::paint(ui.painter(), at, crate::icons::Icon::ChevronDown, self.tokens.text_dim);
                        let choices = |ui: &mut egui::Ui, acts: &mut Vec<(&'static str, Value)>| {
                            if ui.button("Edit the Response before Sending…").clicked() {
                                acts.push(("ui.answerInvitation", json!({ "response": response })));
                                ui.close();
                            }
                            if ui.button("Send the Response Now").clicked() {
                                acts.push(("calendar.respond", json!({ "response": response })));
                                ui.close();
                            }
                            if ui.button("Do Not Send a Response").clicked() {
                                acts.push(("calendar.respond", json!({ "response": response, "send": false })));
                                ui.close();
                            }
                        };
                        // One menu for both buttons: egui's own context menu shares its place
                        // and would shut it on the click that opened it.
                        let toggle = (r.clicked() || r.secondary_clicked()).then_some(egui::SetOpenCommand::Toggle);
                        egui::Popup::menu(&r).open_memory(toggle).show(|ui| choices(ui, acts));
                    }
                }
                if let Some(id) = bar.remove
                    && ui.add(crate::dialog::plain("Remove from Calendar")).clicked()
                {
                    acts.push(("calendar.delete", json!({ "id": id, "series": true })));
                }
                if let Some(day) = &bar.day
                    && ui.add(crate::dialog::plain("Show in Calendar")).on_hover_text("The day it is on, with what else is planned").clicked()
                {
                    acts.push(("nav.module", json!({ "module": "calendar" })));
                    acts.push(("calendar.goTo", json!({ "date": day })));
                }
            });
        });
    }
}

/// Who sent the open message, by name where it gives one.
fn item_sender(ui: &MailUi) -> String {
    ui.session.open.as_ref().map(|m| Some(m.sender_name()).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| m.sender_email())).unwrap_or_default()
}

/// The bar of a task request, acceptance, rejection or update: what it says, in words.
fn task_bar(v: &Value, sender: &str) -> Bar {
    let clean = |s: &str| s.chars().filter(|c| !c.is_control()).take(200).collect::<String>();
    let who = clean(sender);
    let due = v["due"].as_str().map(|d| format!(" Due {d}.")).unwrap_or_default();
    let words = match v["kind"].as_str() {
        Some("request") if v["inTasks"].is_u64() => format!("{who} asked you to do this task; it is in your tasks.{due}"),
        Some("request") => format!("{who} asked you to do this task.{due}"),
        Some("accept") => format!("{who} accepted the task."),
        Some("decline") => format!("{who} declined the task; it is yours again."),
        _ => format!("{who} sent how far along the task is: {}% complete.", v["percent"].as_u64().unwrap_or(0)),
    };
    let readable = v.get("readable").is_none();
    Bar { words, task_answers: readable && v["kind"] == "request", ..Bar::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn people_are_written_so_that_they_read_back() {
        let plain = Attendee::new("June Okafor", "june@example.test", Role::Required);
        assert_eq!(person_text(&plain), "June Okafor <june@example.test>");
        let comma = Attendee::new("Okafor, June \"J\"", "june@example.test", Role::Required);
        assert_eq!(person_text(&comma), "\"Okafor, June J\" <june@example.test>");
        assert_eq!(person_text(&Attendee::new("", "a@example.test", Role::Optional)), "a@example.test");
    }
}
