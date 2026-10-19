//! What the search box understands.
//!
//! Words are all looked for, in the subject, the sender, the recipients, the preview and the
//! categories; `"quoted words"` are looked for together. A word may be kept to one field or ask
//! about the message itself, by the keywords Outlook's search box has and some shorter ones:
//! `from:` `to:` `subject:` `category:`, `has:attachment` (`hasattachments:yes`), `is:unread`
//! (`read:no`), `is:flagged` (`isflagged:yes`), `is:important` (`importance:high`), `before:`
//! `after:` `on:` (`received:`) with a day, a month or a year (`2026-10-05`, `2026-10`, `2026`),
//! and `size:>2mb`. `-word` and `NOT word` leave messages out; `OR` between words takes either
//! side.
//!
//! Where it goes past the search it follows: letters match without their accents and in either
//! case ("zoe" finds "Zöe"), nothing needs an index, and a keyword that is not understood is
//! looked for as the text it is, so a query never fails and never quietly finds everything.

use openmailclient_model::zone::Zone;
use openmailclient_model::{Flag, Summary, time};

/// Clauses one query may have; what is typed past them is left out.
const MAX_CLAUSES: usize = 32;
/// Ticks (100 ns) in a day.
const DAY: i64 = 864_000_000_000;

/// Letters U+00C0 to U+017F without their accents and in lower case, from the Unicode
/// decompositions; a letter that has none (æ, ø, ß, ł) stands for itself.
const PLAIN: &str = "aaaaaaæceeeeiiiiðnooooo×øuuuuyþßaaaaaaæceeeeiiiiðnooooo÷øuuuuyþyaaaaaaccccccccddđđeeeeeeeeeegggggggghhħħiiiiiiiiiıĳĳjjkkĸllllllŀŀłłnnnnnnŉŋŋooooooœœrrrrrrssssssssttttŧŧuuuuuuuuuuuuwwyyyzzzzzzſ";

/// Text as it is compared: lower case, Latin letters without accents, accents that stand alone
/// (U+0300 to U+036F) dropped.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\u{300}'..='\u{36F}' => {}
            '\u{C0}'..='\u{17F}' => {
                let plain = usize::try_from(u32::from(c) - 0xC0).ok().and_then(|n| PLAIN.chars().nth(n));
                out.push(plain.unwrap_or(c));
            }
            _ => out.extend(c.to_lowercase()),
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
enum Test {
    Text(String),
    From(String),
    To(String),
    Subject(String),
    Category(String),
    Attachment(bool),
    Read(bool),
    Flagged(bool),
    Complete,
    Importance(i32),
    /// Received before this moment; at or after it; within the two.
    Before(i64),
    After(i64),
    Within(i64, i64),
    Larger(u64),
    Smaller(u64),
}

#[derive(Clone, Debug, PartialEq)]
struct Clause {
    not: bool,
    test: Test,
}

/// A query as understood: a message matches when all clauses of any one alternative hold.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    any: Vec<Vec<Clause>>,
}

struct Token {
    /// As typed, less the quotation marks and a leading `-`.
    text: String,
    /// `key:value` with the key outside quotation marks.
    keyed: Option<(String, String)>,
    not: bool,
    quoted: bool,
}

fn tokens(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(first) = chars.peek().copied() {
        if first.is_whitespace() {
            chars.next();
            continue;
        }
        let (mut word, mut quoted, mut inside, mut colon) = (String::new(), false, false, None);
        while let Some(c) = chars.peek().copied() {
            if c == '"' {
                (quoted, inside) = (true, !inside);
            } else if c.is_whitespace() && !inside {
                break;
            } else {
                if c == ':' && !quoted && colon.is_none() {
                    colon = Some(word.len());
                }
                word.push(c);
            }
            chars.next();
        }
        // A `-` before a word leaves it out; one alone, or before quotation marks only, is text.
        let not = word.len() > 1 && word.starts_with('-');
        let (word, colon) = if not { (word.get(1..).unwrap_or_default().to_string(), colon.map(|at| at.saturating_sub(1))) } else { (word, colon) };
        let keyed = colon.and_then(|at| {
            let (key, value) = (word.get(..at)?, word.get(at.checked_add(1)?..)?);
            (!key.is_empty() && key.chars().all(|c| c.is_ascii_alphabetic())).then(|| (key.to_ascii_lowercase(), value.to_string()))
        });
        if !word.is_empty() {
            out.push(Token { text: word, keyed, not, quoted });
        }
    }
    out
}

fn yes(value: &str) -> Option<bool> {
    match value {
        "yes" | "true" | "y" | "1" => Some(true),
        "no" | "false" | "n" | "0" => Some(false),
        _ => None,
    }
}

/// A day, a month or a year, as the span of time it covers where the user is.
fn span(value: &str, tz: &Zone) -> Option<(i64, i64)> {
    let mut parts = value.split('-');
    let number = |part: Option<&str>, digits: usize| -> Option<Option<u32>> {
        match part {
            None => Some(None),
            Some(p) if !p.is_empty() && p.len() <= digits && p.bytes().all(|b| b.is_ascii_digit()) => Some(p.parse().ok()),
            Some(_) => None,
        }
    };
    let year = i64::from(number(parts.next(), 4)??);
    let (month, day) = (number(parts.next(), 2)?, number(parts.next(), 2)?);
    if parts.next().is_some() || !(1601..=9999).contains(&year) || month.is_some_and(|m| !(1..=12).contains(&m)) {
        return None;
    }
    let start = |y: i64, m: u32, d: u32| tz.utc_of(time::from_civil(y, m, d, 0, 0, 0, 0));
    Some(match (month, day) {
        (Some(m), Some(d)) => {
            if !(1..=31).contains(&d) {
                return None;
            }
            let from = start(year, m, d);
            (from, from.saturating_add(DAY))
        }
        (Some(12), None) => (start(year, 12, 1), start(year + 1, 1, 1)),
        (Some(m), None) => (start(year, m, 1), start(year, m + 1, 1)),
        (None, _) => (start(year, 1, 1), start(year + 1, 1, 1)),
    })
}

/// `>2mb`, `<10kb`, `500` (larger than): a count of bytes and the side of it asked for.
fn size(value: &str) -> Option<Test> {
    let (larger, rest) = match value.strip_prefix('<') {
        Some(rest) => (false, rest),
        None => (true, value.strip_prefix('>').unwrap_or(value)),
    };
    let rest = rest.strip_prefix('=').unwrap_or(rest);
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (number, unit) = (rest.get(..digits)?, rest.get(digits..)?);
    if number.is_empty() || number.len() > 12 {
        return None;
    }
    let scale: u64 = match unit {
        "" | "b" => 1,
        "k" | "kb" => 1 << 10,
        "m" | "mb" => 1 << 20,
        "g" | "gb" => 1 << 30,
        _ => return None,
    };
    let bytes = number.parse::<u64>().ok()?.saturating_mul(scale);
    Some(if larger { Test::Larger(bytes) } else { Test::Smaller(bytes) })
}

/// What a `key:value` asks. `None` when the key is not one of ours or the value makes no sense
/// for it: the words are then looked for as text.
fn keyed(key: &str, value: &str, tz: &Zone) -> Option<Test> {
    let folded = fold(value);
    let value = folded.as_str();
    let text = |make: fn(String) -> Test| (!value.is_empty()).then(|| make(value.to_string()));
    match key {
        "from" => text(Test::From),
        "to" => text(Test::To),
        "subject" => text(Test::Subject),
        "category" | "categories" => text(Test::Category),
        "has" | "hasattachment" | "hasattachments" => match value {
            "attachment" | "attachments" => Some(Test::Attachment(true)),
            _ => yes(value).map(Test::Attachment),
        },
        "is" => match value {
            "unread" => Some(Test::Read(false)),
            "read" => Some(Test::Read(true)),
            "flagged" => Some(Test::Flagged(true)),
            "complete" | "completed" | "done" => Some(Test::Complete),
            "important" | "high" => Some(Test::Importance(2)),
            "low" => Some(Test::Importance(0)),
            _ => None,
        },
        "read" => yes(value).map(Test::Read),
        "flagged" | "isflagged" => yes(value).map(Test::Flagged),
        "importance" => match value {
            "high" => Some(Test::Importance(2)),
            "normal" => Some(Test::Importance(1)),
            "low" => Some(Test::Importance(0)),
            _ => None,
        },
        "before" => span(value, tz).map(|(from, _)| Test::Before(from)),
        "after" => span(value, tz).map(|(_, to)| Test::After(to)),
        "on" | "received" | "date" => span(value, tz).map(|(from, to)| Test::Within(from, to)),
        "size" => size(value),
        _ => None,
    }
}

/// Understand what was typed. `tz` is the user's time zone, for the dates.
pub fn parse(text: &str, tz: &Zone) -> Query {
    let mut any: Vec<Vec<Clause>> = vec![Vec::new()];
    let (mut negate, mut count) = (false, 0usize);
    for token in tokens(text) {
        if !token.quoted && !token.not {
            match token.text.as_str() {
                "OR" => {
                    any.push(Vec::new());
                    negate = false;
                    continue;
                }
                "AND" => continue,
                "NOT" => {
                    negate = true;
                    continue;
                }
                _ => {}
            }
        }
        if count >= MAX_CLAUSES {
            break;
        }
        let test = token.keyed.as_ref().and_then(|(key, value)| keyed(key, value, tz)).unwrap_or_else(|| Test::Text(fold(&token.text)));
        if let Some(all) = any.last_mut() {
            all.push(Clause { not: token.not != negate, test });
            count += 1;
        }
        negate = false;
    }
    // An `OR` with nothing on one side is only the other side.
    any.retain(|all| !all.is_empty());
    Query { any }
}

impl Query {
    /// Nothing was asked: every message matches.
    pub fn is_empty(&self) -> bool {
        self.any.is_empty()
    }

    pub fn matches(&self, row: &Summary) -> bool {
        if self.any.is_empty() {
            return true;
        }
        let (subject, from, to) = (fold(&row.subject), fold(&row.from), fold(&row.to));
        let categories: Vec<String> = row.categories.iter().map(|c| fold(c)).collect();
        let preview = fold(&row.preview);
        let holds = |test: &Test| match test {
            Test::Text(t) => [&subject, &from, &to, &preview].into_iter().chain(&categories).any(|field| field.contains(t.as_str())),
            Test::From(t) => from.contains(t.as_str()),
            Test::To(t) => to.contains(t.as_str()),
            Test::Subject(t) => subject.contains(t.as_str()),
            Test::Category(t) => categories.iter().any(|c| c.contains(t.as_str())),
            Test::Attachment(on) => row.has_attachments == *on,
            Test::Read(on) => row.read == *on,
            Test::Flagged(on) => (row.flag == Flag::Flagged) == *on,
            Test::Complete => row.flag == Flag::Complete,
            Test::Importance(level) => row.importance == *level,
            // A message with no time is on no day.
            Test::Before(at) => row.time != 0 && row.time < *at,
            Test::After(at) => row.time != 0 && row.time >= *at,
            Test::Within(from, to) => row.time != 0 && (*from..*to).contains(&row.time),
            Test::Larger(bytes) => u64::from(row.size) > *bytes,
            Test::Smaller(bytes) => u64::from(row.size) < *bytes,
        };
        self.any.iter().any(|all| all.iter().all(|clause| holds(&clause.test) != clause.not))
    }
}
