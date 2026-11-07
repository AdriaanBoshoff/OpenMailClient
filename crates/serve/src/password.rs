//! The web password (M7.2): one for the whole host, kept only as an Argon2id hash (RFC 9106) in a
//! file of its own beside the settings, never in them and never sent anywhere. Setting it again
//! is the reset; each setting has a number one higher than the last, so sessions signed in with
//! an older one can be told apart and ended (M7.3).
//!
//! Weak passwords are refused before they are kept: fewer than [`MIN_LEN`] characters, or made
//! mostly of the words most often guessed, keyboard runs, counting, one character again and
//! again, or the program's own name, however spelt with digits for letters. The list is our own
//! and lives in the binary; nothing is looked up anywhere.

use argon2::password_hash::{PasswordHash, SaltString};
use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};
use serde_json::{Value, json};
use std::path::Path;

/// The fewest characters a web password may have.
pub const MIN_LEN: usize = 12;
/// The most bytes of a password looked at; a longer one is refused, not cut.
pub const MAX_LEN: usize = 1024;
/// The most memory a kept hash may ask of a check, in KiB; a file asking more is not ours.
const MAX_MEMORY_KIB: u32 = 256 * 1024;
/// The most passes a kept hash may ask of a check.
const MAX_PASSES: u32 = 16;
/// The most lanes a kept hash may ask of a check.
const MAX_LANES: u32 = 16;

/// The web password as kept: its hash, when it was set and which setting it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    /// The PHC string of the Argon2id hash, salt and parameters in it.
    pub hash: String,
    /// When it was set, in milliseconds since 1970.
    pub set_ms: u64,
    /// One more than the setting before it; sessions carry the one they were signed in with.
    pub generation: u64,
}

/// Why a password would not do, said to the one choosing it; `None` when it will.
pub fn weakness(password: &str) -> Option<String> {
    let chars = password.chars().count();
    if password.len() > MAX_LEN {
        return Some(format!("a password may have at most {MAX_LEN} bytes"));
    }
    if chars < MIN_LEN {
        return Some(format!("a password needs at least {MIN_LEN} characters; this one has {chars}"));
    }
    if password.trim() != password && password.trim().chars().count() < MIN_LEN {
        return Some(format!("a password needs at least {MIN_LEN} characters besides spaces at its ends"));
    }
    let plain = normal(password);
    let distinct: std::collections::BTreeSet<char> = plain.iter().map(|(c, _)| *c).collect();
    if distinct.len() < 5 {
        return Some("a password needs more than four different characters".into());
    }
    let (rest, pieces) = leftover(&plain);
    if rest < 4 && pieces < 4 {
        return Some("this password is mostly a word or pattern that is often guessed; add words of your own".into());
    }
    None
}

/// The password folded the way guessers fold it, each character beside the one it was: lower
/// case, digits and signs that stand for letters read as those letters, spaces and dashes
/// between words left out.
fn normal(password: &str) -> Vec<(char, char)> {
    password
        .chars()
        .flat_map(|c| c.to_lowercase().map(move |l| (l, c)))
        .filter(|(c, _)| !matches!(c, ' ' | '-' | '_' | '.'))
        .map(|(c, was)| {
            let folded = match c {
                '0' => 'o',
                '1' | '!' | '|' => 'i',
                '3' => 'e',
                '4' | '@' => 'a',
                '5' | '$' => 's',
                '7' | '+' => 't',
                '8' => 'b',
                '9' => 'g',
                other => other,
            };
            (folded, was)
        })
        .collect()
}

/// How many characters are left once the words, runs and repeats a guesser tries first, and
/// the numbers (a year, a PIN) and signs put after them, are taken out of `plain`, and how many
/// such pieces there were.
fn leftover(plain: &[(char, char)]) -> (usize, usize) {
    let folded: Vec<char> = plain.iter().map(|(c, _)| *c).collect();
    let (mut rest, mut pieces, mut i) = (0usize, 0usize, 0usize);
    while i < folded.len() {
        let tail = folded.get(i..).unwrap_or(&[]);
        let was = plain.get(i..).unwrap_or(&[]);
        let guessed = longest_word(tail).max(run(tail)).max(repeat(tail));
        let guessed = if guessed >= 3 { guessed } else { 0 };
        let number = was.iter().take_while(|(_, w)| w.is_ascii_digit()).count().min(4);
        let signs = was.iter().take_while(|(_, w)| w.is_ascii_punctuation()).count().min(3);
        let taken = guessed.max(number).max(signs);
        if taken > 0 {
            pieces += 1;
            i += taken;
        } else {
            rest += 1;
            i += 1;
        }
    }
    (rest, pieces)
}

/// The length of the longest common word `tail` starts with, or 0.
fn longest_word(tail: &[char]) -> usize {
    COMMON
        .iter()
        .filter(|w| w.len() >= 3 && w.len() <= tail.len() && w.chars().zip(tail).all(|(a, b)| a == *b))
        .map(|w| w.chars().count())
        .max()
        .unwrap_or(0)
}

/// The length of the run of counting (`abcd`, `4321`) or of keys side by side (`qwer`, `lkjh`)
/// that `tail` starts with, or 0. Digits were read as letters by [`normal`], so the rows here
/// are spelt as they were folded.
fn run(tail: &[char]) -> usize {
    const ROWS: &[&str] = &["abcdefghijklmnopqrstuvwxyz", "qwertyuiop", "asdfghjkl", "zxcvbnm", "i2eas6tbgo", "qazwsxedcrfvtgbyhnujmikolp"];
    let mut best = 0;
    for row in ROWS {
        let row: Vec<char> = row.chars().collect();
        for backwards in [false, true] {
            let mut ordered = row.clone();
            if backwards {
                ordered.reverse();
            }
            for start in 0..ordered.len() {
                let n = ordered.get(start..).unwrap_or(&[]).iter().zip(tail).take_while(|(a, b)| a == b).count();
                best = best.max(n);
            }
        }
    }
    best
}

/// How many times the first character of `tail` is said again at its start, or 0 for once.
fn repeat(tail: &[char]) -> usize {
    match tail.first() {
        Some(first) => tail.iter().take_while(|c| *c == first).count(),
        None => 0,
    }
}

/// A new hash of `password`, with a salt of 16 random bytes and Argon2id's recommended costs
/// (19 MiB, two passes, one lane; RFC 9106 §4 and OWASP's guidance).
pub fn hash(password: &str) -> Result<String, String> {
    if password.len() > MAX_LEN {
        return Err(format!("a password may have at most {MAX_LEN} bytes"));
    }
    let salt = SaltString::encode_b64(&crate::random::<16>()?).map_err(|e| format!("salt: {e}"))?;
    let hashed = argon2().hash_password(password.as_bytes(), &salt).map_err(|e| format!("hash: {e}"))?;
    Ok(hashed.to_string())
}

fn argon2() -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default())
}

/// Whether `password` is the one `hashed` was made from. The check costs the same whether it is
/// right or wrong (the hash is made again and compared in constant time); a hash that is not an
/// Argon2id one of ours, or asks for more memory or passes than ours would, is never matched.
pub fn verify(password: &str, hashed: &str) -> bool {
    if password.len() > MAX_LEN {
        return false;
    }
    let Ok(parsed) = PasswordHash::new(hashed) else { return false };
    if parsed.algorithm.as_str() != "argon2id" {
        return false;
    }
    let Ok(params) = Params::try_from(&parsed) else { return false };
    if params.m_cost() > MAX_MEMORY_KIB || params.t_cost() > MAX_PASSES || params.p_cost() > MAX_LANES {
        return false;
    }
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params).verify_password(password.as_bytes(), &parsed).is_ok()
}

/// The kept password in `dir`, or `None` when none was set.
pub fn load(dir: &Path) -> Result<Option<Kept>, String> {
    let path = crate::password_file(dir);
    let Some(text) = crate::read_small(&path)? else { return Ok(None) };
    let value: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let hash = value.get("hash").and_then(Value::as_str).filter(|h| h.len() <= 512 && h.starts_with("$argon2id$"));
    let Some(hash) = hash else { return Err(format!("{}: no Argon2id hash in it", path.display())) };
    Ok(Some(Kept {
        hash: hash.to_string(),
        set_ms: value.get("set").and_then(Value::as_u64).unwrap_or(0),
        generation: value.get("generation").and_then(Value::as_u64).unwrap_or(0),
    }))
}

/// Set the web password in `dir` (the reset when it was set before), after [`weakness`] has
/// found nothing. What was kept before, if anything, is replaced whole.
pub fn set(dir: &Path, password: &str, now_ms: u64) -> Result<Kept, String> {
    if let Some(why) = weakness(password) {
        return Err(why);
    }
    let before = load(dir).ok().flatten().map_or(0, |k| k.generation);
    let kept = Kept { hash: hash(password)?, set_ms: now_ms, generation: before.saturating_add(1) };
    let text = json!({
        "about": "The OpenMailClient web password, as an Argon2id hash only. Set it again with `openmailclient-cli web password`.",
        "hash": kept.hash,
        "set": kept.set_ms,
        "generation": kept.generation,
    });
    crate::write_private(&crate::password_file(dir), &format!("{text:#}\n"))?;
    Ok(kept)
}

/// The passwords most often guessed, and words guessers put in them, folded as [`normal`]
/// folds; our own list, made for this program.
const COMMON: &[&str] = &[
    "password",
    "passwort",
    "passw",
    "pass",
    "secret",
    "letmein",
    "welcome",
    "admin",
    "administrator",
    "root",
    "login",
    "user",
    "guest",
    "default",
    "changeme",
    "qwerty",
    "azerty",
    "iloveyou",
    "love",
    "lovely",
    "monkey",
    "dragon",
    "master",
    "shadow",
    "sunshine",
    "princess",
    "football",
    "baseball",
    "soccer",
    "hockey",
    "batman",
    "superman",
    "trustno",
    "starwars",
    "whatever",
    "freedom",
    "computer",
    "internet",
    "mustang",
    "michael",
    "jennifer",
    "jordan",
    "hunter",
    "ranger",
    "buster",
    "tigger",
    "charlie",
    "robert",
    "thomas",
    "daniel",
    "andrew",
    "joshua",
    "matthew",
    "jessica",
    "ashley",
    "amanda",
    "nicole",
    "summer",
    "winter",
    "spring",
    "autumn",
    "flower",
    "cookie",
    "chocolate",
    "cheese",
    "pepper",
    "ginger",
    "banana",
    "orange",
    "purple",
    "silver",
    "golden",
    "diamond",
    "angel",
    "heaven",
    "killer",
    "hello",
    "hallo",
    "test",
    "testing",
    "demo",
    "sample",
    "server",
    "ubuntu",
    "debian",
    "linux",
    "windows",
    "apple",
    "google",
    "mail",
    "email",
    "inbox",
    "outlook",
    "openmailclient",
    "openmail",
    "mailclient",
    "client",
    "access",
    "web",
    "home",
    "family",
    "house",
    "office",
    "work",
    "money",
    "lucky",
    "happy",
    "smile",
    "abc",
    "abcd",
    "abcdef",
    "abcabc",
    "aaa",
    "ooo",
    "iii",
    "zaq",
    "zxc",
    "asdf",
    "qaz",
    "wsx",
    "pussy",
    "fuck",
    "shit",
    "bitch",
    "sex",
    "god",
    "jesus",
    "christ",
    "blessed",
    "baby",
    "mother",
    "father",
    "sister",
    "brother",
    "friend",
    "friends",
    "january",
    "february",
    "march",
    "april",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "year",
    "date",
    "birthday",
    "wifi",
    "router",
    "network",
    "phone",
    "mobile",
    "samsung",
    "iphone",
    "android",
    "music",
    "player",
    "gamer",
    "game",
    "games",
    "minecraft",
    "pokemon",
    "naruto",
    "matrix",
    "zelda",
    "mario",
    "liverpool",
    "chelsea",
    "arsenal",
    "barcelona",
    "madrid",
    "united",
    "london",
    "paris",
    "berlin",
    "america",
    "canada",
    "africa",
    "australia",
    "india",
    "china",
    "europe",
    "secure",
    "security",
    "private",
    "privat",
    "open",
    "sesame",
    "opensesame",
    "nothing",
    "none",
    "blank",
    "empty",
    "temp",
    "temporary",
    "new",
    "old",
    "first",
    "last",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "zero",
    "hundred",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_check_the_password_and_nothing_else() {
        let hashed = hash("a long and unusual phrase of ours").unwrap();
        assert!(hashed.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert!(verify("a long and unusual phrase of ours", &hashed));
        assert!(!verify("a long and unusual phrase of ourz", &hashed));
        assert!(!verify("", &hashed));
        // Two hashes of one password differ by their salts.
        assert_ne!(hashed, hash("a long and unusual phrase of ours").unwrap());
    }

    #[test]
    fn a_hash_not_ours_is_never_matched() {
        for junk in [
            "",
            "$argon2id$",
            "plain text",
            "$argon2i$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaA",
            "$argon2id$v=19$m=4194304,t=2,p=1$c2FsdHNhbHRzYWx0$aGFzaGhhc2hoYXNoaGFzaA",
        ] {
            assert!(!verify("anything at all here", junk), "{junk}");
        }
        assert!(!verify(&"x".repeat(MAX_LEN + 1), "$argon2id$"));
    }

    #[test]
    fn weak_passwords_are_refused() {
        for weak in [
            "short",
            "Password1234",
            "P@ssw0rd!2024",
            "qwertyuiop12",
            "123456789012",
            "aaaaaaaaaaaaaa",
            "abababababab",
            "letmein letmein",
            "OpenMailClient2026",
            "Summer2026!!!",
            "zxcvbnm,./1234",
        ] {
            assert!(weakness(weak).is_some(), "{weak} was let through");
        }
    }

    #[test]
    fn passwords_of_ones_own_are_kept() {
        for fine in ["kettle orbit plum vane", "Xy7#mQ2!pLw9", "the gate by the old mill", "trams drip on velvet", "summer dragon monkey kettle"] {
            assert_eq!(weakness(fine), None, "{fine} was refused");
        }
    }

    #[test]
    fn the_password_is_kept_beside_the_settings_and_reset_by_setting_it_again() {
        let dir = std::env::temp_dir().join(format!("omc-web-password-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load(&dir).unwrap(), None);
        assert!(set(&dir, "short", 1).is_err());
        let first = set(&dir, "kettle orbit plum vane", 1000).unwrap();
        assert_eq!(first.generation, 1);
        assert_eq!(load(&dir).unwrap(), Some(first.clone()));
        let text = std::fs::read_to_string(crate::password_file(&dir)).unwrap();
        assert!(!text.contains("kettle"), "the password itself was written");
        let second = set(&dir, "trams drip on velvet", 2000).unwrap();
        assert_eq!(second.generation, 2);
        let kept = load(&dir).unwrap().unwrap();
        assert!(verify("trams drip on velvet", &kept.hash));
        assert!(!verify("kettle orbit plum vane", &kept.hash));
        std::fs::write(crate::password_file(&dir), "{not json").unwrap();
        assert!(load(&dir).is_err());
        std::fs::write(crate::password_file(&dir), r#"{"hash":"$2b$10$bcrypt"}"#).unwrap();
        assert!(load(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
