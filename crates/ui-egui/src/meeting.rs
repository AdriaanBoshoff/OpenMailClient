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

/// The bar above a meeting's message, worked out once for each message shown.
#[derive(Clone, Debug, Default)]
pub(crate) struct Bar {
    pub(crate) words: String,
    /// It asks a warning's colour: out of date, or not from whom it has to come.
    pub(crate) warn: bool,
    /// Accept, Tentative and Decline are offered.
    pub(crate) answers: bool,
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
        if !meeting::is_meeting_message(item) {
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
        let bar = self.session.run("calendar.invitation", &Value::Null).ok().map(|v| self.bar_of(&v, &sender));
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

    /// The bar drawn: its words, and what it offers, as commands to run.
    pub(crate) fn meeting_bar_ui(&self, ui: &mut egui::Ui, bar: &Bar, acts: &mut Vec<(&'static str, Value)>) {
        let t = &self.tokens;
        egui::Frame::NONE.fill(if bar.warn { t.info_bar } else { t.surface }).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(&bar.words).font(crate::font(12.0)).color(t.text));
            });
            ui.horizontal(|ui| {
                if bar.answers {
                    for (label, response, hint) in [
                        ("Accept", "accept", "Put it in your calendar and tell the organizer"),
                        ("Tentative", "tentative", "Put it in your calendar as tentative and tell the organizer"),
                        ("Decline", "decline", "Tell the organizer you will not come; it stays out of your calendar"),
                    ] {
                        let r = ui.add(crate::dialog::plain(label)).on_hover_text(hint);
                        if r.clicked() {
                            acts.push(("calendar.respond", json!({ "response": response })));
                        }
                        r.context_menu(|ui| {
                            if ui.button("Send the Response Now").clicked() {
                                acts.push(("calendar.respond", json!({ "response": response })));
                                ui.close();
                            }
                            if ui.button("Do Not Send a Response").clicked() {
                                acts.push(("calendar.respond", json!({ "response": response, "send": false })));
                                ui.close();
                            }
                        });
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
