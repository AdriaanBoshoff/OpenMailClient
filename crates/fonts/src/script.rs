//! Text that is written from right to left, and Arabic letters that join.
//!
//! A line is stored in the order it is read and drawn in another: [`levels`] says how deep in
//! right-to-left and left-to-right text each character stands (the Unicode Bidirectional
//! Algorithm, UAX #9, rules P2 to I2), [`order`] puts the characters of one line in the order
//! they are drawn (rules L1 and L2), and [`mirrored`] turns a bracket round where it stands in
//! right-to-left text (rule L4).
//!
//! [`joined`] gives each Arabic letter the shape it has beside its neighbours (The Unicode
//! Standard, section 9.2, "Arabic Joining"; joining types as in `ArabicShaping.txt`), by the
//! characters Unicode keeps for those shapes (Arabic Presentation Forms-A and -B). It is not
//! shaping by a font's own tables: marks are not placed and only lam with alef becomes one
//! letter.
//!
//! The text comes from a message and is hostile: nothing here indexes past what it was given,
//! and a text longer than [`MAX_CHARS`] is left in the order it is stored.

use unicode_bidi::{BidiInfo, Level};

/// Characters of one text whose order is worked out.
pub const MAX_CHARS: usize = 200_000;

/// What stands in for the alef that lam has taken into itself: it has no width and no glyph.
pub const TAKEN: char = '\u{200B}';

/// Whether `c` is written from right to left, or says that what follows is.
pub fn is_rtl(c: char) -> bool {
    matches!(u32::from(c),
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x1_0800..=0x1_0FFF | 0x1_E800..=0x1_EFFF | 0x200F | 0x202B | 0x202E | 0x2067)
}

/// Whether any of `text` is written from right to left: only then need its order be worked out.
pub fn has_rtl(text: &str) -> bool {
    text.chars().any(is_rtl)
}

/// The direction of the first character of `text` that has one (UAX #9, P2 and P3): `Some(true)`
/// for right to left. Digits and marks have none.
pub fn first_strong(text: &str) -> Option<bool> {
    text.chars().take(MAX_CHARS).find_map(|c| {
        if is_rtl(c) && !matches!(c, '\u{0660}'..='\u{0669}' | '\u{06F0}'..='\u{06F9}') {
            Some(true)
        } else if c.is_alphabetic() {
            Some(false)
        } else {
            None
        }
    })
}

/// How deep each character of `text` stands: an even number is left to right, an odd one right
/// to left. `rtl` is the direction of the paragraph; `None` takes it from the first character
/// that has one. One number for each character; all the paragraph's own for a text too long.
pub fn levels(text: &str, rtl: Option<bool>) -> Vec<u8> {
    let base = rtl.map(|rtl| if rtl { Level::rtl() } else { Level::ltr() });
    let count = text.chars().count();
    if count > MAX_CHARS || !has_rtl(text) && rtl != Some(true) {
        return vec![u8::from(rtl == Some(true)); count];
    }
    let info = BidiInfo::new(text, base);
    text.char_indices().map(|(at, _)| info.levels.get(at).map_or(0, |level| level.number())).collect()
}

fn blank(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// The order one line is drawn in: for each place from the left, which of the line's characters
/// stands there. `levels` are those of the line's characters and `base` the paragraph's (0 or
/// 1). Blanks at the end of the line stand at the paragraph's end, whatever they followed, and
/// a tab with the blanks before it stands between what is before it and what is after it the
/// way the paragraph is read, whatever is written on either side.
pub fn order(chars: &[char], levels: &[u8], base: u8) -> Vec<usize> {
    let count = chars.len().min(levels.len());
    let mut depth: Vec<u8> = levels.iter().take(count).copied().collect();
    // L1: a tab, the blanks before a tab and the blanks at the end of the line take the
    // paragraph's level.
    let mut ends = true;
    for (level, c) in depth.iter_mut().zip(chars.iter().take(count)).rev() {
        ends = *c == '\t' || (ends && blank(*c));
        if ends {
            *level = base;
        }
    }
    let mut places: Vec<usize> = (0..count).collect();
    let highest = depth.iter().copied().max().unwrap_or(0);
    let lowest_odd = depth.iter().copied().filter(|level| level % 2 == 1).min().unwrap_or(u8::MAX);
    // L2: from the deepest level down to the shallowest odd one, turn round every run of
    // characters that stand at that level or deeper.
    let mut level = highest;
    while level >= lowest_odd && level > 0 {
        let mut at = 0usize;
        while at < count {
            let deep = |i: usize| places.get(i).and_then(|p| depth.get(*p)).is_some_and(|d| *d >= level);
            if !deep(at) {
                at += 1;
                continue;
            }
            let start = at;
            while at < count && deep(at) {
                at += 1;
            }
            if let Some(run) = places.get_mut(start..at) {
                run.reverse();
            }
        }
        level -= 1;
    }
    places
}

/// Pairs that are each other's mirror (UAX #9, L4; `BidiMirroring.txt`), the ones mail is
/// written with.
const MIRRORS: [(char, char); 9] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>'), ('«', '»'), ('‹', '›'), ('≤', '≥'), ('⟨', '⟩'), ('⁅', '⁆')];

/// `c` as it is drawn in right-to-left text: an opening bracket opens to the left.
pub fn mirrored(c: char) -> char {
    MIRRORS
        .iter()
        .find_map(|(a, b)| {
            if *a == c {
                Some(*b)
            } else if *b == c {
                Some(*a)
            } else {
                None
            }
        })
        .unwrap_or(c)
}

/// How a letter joins its neighbours (`ArabicShaping.txt`, joining type).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Join {
    /// Not at all.
    None,
    /// To the letter before it only.
    Before,
    /// To both.
    Both,
    /// It makes its neighbours join and has one shape itself (tatweel, the zero-width joiner).
    Causing,
    /// A mark: the letters on either side join through it.
    Through,
}

/// A letter, the first of the characters that are its shapes, and how many it has: one (alone),
/// two (alone, joined to the letter before) or four (and then: joined to the letter after, to
/// both). Arabic Presentation Forms-B for the Arabic alphabet, Forms-A for letters of Persian
/// and Urdu.
const SHAPES: [(u32, u32, u8); 52] = [
    (0x0621, 0xFE80, 1),
    (0x0622, 0xFE81, 2),
    (0x0623, 0xFE83, 2),
    (0x0624, 0xFE85, 2),
    (0x0625, 0xFE87, 2),
    (0x0626, 0xFE89, 4),
    (0x0627, 0xFE8D, 2),
    (0x0628, 0xFE8F, 4),
    (0x0629, 0xFE93, 2),
    (0x062A, 0xFE95, 4),
    (0x062B, 0xFE99, 4),
    (0x062C, 0xFE9D, 4),
    (0x062D, 0xFEA1, 4),
    (0x062E, 0xFEA5, 4),
    (0x062F, 0xFEA9, 2),
    (0x0630, 0xFEAB, 2),
    (0x0631, 0xFEAD, 2),
    (0x0632, 0xFEAF, 2),
    (0x0633, 0xFEB1, 4),
    (0x0634, 0xFEB5, 4),
    (0x0635, 0xFEB9, 4),
    (0x0636, 0xFEBD, 4),
    (0x0637, 0xFEC1, 4),
    (0x0638, 0xFEC5, 4),
    (0x0639, 0xFEC9, 4),
    (0x063A, 0xFECD, 4),
    (0x0641, 0xFED1, 4),
    (0x0642, 0xFED5, 4),
    (0x0643, 0xFED9, 4),
    (0x0644, 0xFEDD, 4),
    (0x0645, 0xFEE1, 4),
    (0x0646, 0xFEE5, 4),
    (0x0647, 0xFEE9, 4),
    (0x0648, 0xFEED, 2),
    (0x0649, 0xFEEF, 2),
    (0x064A, 0xFEF1, 4),
    (0x0671, 0xFB50, 2),
    (0x0679, 0xFB66, 4),
    (0x067E, 0xFB56, 4),
    (0x0686, 0xFB7A, 4),
    (0x0688, 0xFB88, 2),
    (0x0691, 0xFB8C, 2),
    (0x0698, 0xFB8A, 2),
    (0x06A4, 0xFB6A, 4),
    (0x06A9, 0xFB8E, 4),
    (0x06AF, 0xFB92, 4),
    (0x06BA, 0xFB9E, 2),
    (0x06BE, 0xFBAA, 4),
    (0x06C1, 0xFBA6, 4),
    (0x06CC, 0xFBFC, 4),
    (0x06D2, 0xFBAE, 2),
    (0x06D3, 0xFBB0, 2),
];

/// Lam with each alef it takes into itself: the alef, and the first of the two shapes (alone,
/// joined to the letter before).
const LAM_ALEF: [(u32, u32); 4] = [(0x0622, 0xFEF5), (0x0623, 0xFEF7), (0x0625, 0xFEF9), (0x0627, 0xFEFB)];
const LAM: char = '\u{0644}';

fn shapes_of(c: char) -> Option<(u32, u8)> {
    SHAPES.iter().find(|(letter, ..)| *letter == u32::from(c)).map(|(_, first, count)| (*first, *count))
}

fn join(c: char) -> Join {
    match u32::from(c) {
        0x0640 | 0x200D => Join::Causing,
        0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC | 0x06DF..=0x06E4 | 0x06E7 | 0x06E8 | 0x06EA..=0x06ED => Join::Through,
        _ => match shapes_of(c) {
            Some((_, 4)) => Join::Both,
            Some((_, 2)) => Join::Before,
            Some(_) => Join::None,
            // A letter of the Arabic script that is not listed is taken to join both ways.
            None if matches!(u32::from(c), 0x0620 | 0x063B..=0x063F | 0x066E..=0x06D3 | 0x06FA..=0x06FF | 0x0750..=0x077F) => Join::Both,
            None => Join::None,
        },
    }
}

/// Whether `c` takes part in Arabic joining: a letter that joins, a mark it is joined
/// through, or tatweel and the zero-width joiner that join what stands beside them.
pub fn joins(c: char) -> bool {
    !matches!(join(c), Join::None)
}

/// `text` with each Arabic letter as the shape it has where it stands, for the letters whose
/// shapes `has` says can be drawn; the others are left as they are. As many characters come
/// back as went in: an alef that lam takes into itself becomes [`TAKEN`].
pub fn joined(text: &[char], has: &mut dyn FnMut(char) -> bool) -> Vec<char> {
    let mut out = text.to_vec();
    if text.len() > MAX_CHARS {
        return out;
    }
    // The nearest letter before each place and after it, marks passed over.
    let near = |from: usize, back: bool| -> Join {
        let mut at = from;
        loop {
            at = match if back { at.checked_sub(1) } else { at.checked_add(1) } {
                Some(at) => at,
                None => return Join::None,
            };
            match text.get(at).map(|c| join(*c)) {
                Some(Join::Through) => {}
                Some(found) => return found,
                None => return Join::None,
            }
        }
    };
    let mut skip = false;
    for (at, c) in text.iter().enumerate() {
        if std::mem::take(&mut skip) {
            continue;
        }
        let own = join(*c);
        if !matches!(own, Join::Both | Join::Before) {
            // A letter with one shape, where the font keeps it apart from the letter itself.
            continue;
        }
        let before = matches!(near(at, true), Join::Both | Join::Causing);
        let after = own == Join::Both && matches!(near(at, false), Join::Both | Join::Before | Join::Causing);
        if *c == LAM
            && let Some((_, first)) = text.get(at + 1).and_then(|next| LAM_ALEF.iter().find(|(alef, _)| *alef == u32::from(*next)))
            && let Some(one) = char::from_u32(first + u32::from(before))
            && has(one)
        {
            if let (Some(lam), Some(alef)) = (out.get_mut(at), text.get(at + 1).map(|_| TAKEN)) {
                *lam = one;
                if let Some(next) = out.get_mut(at + 1) {
                    *next = alef;
                }
            }
            skip = true;
            continue;
        }
        let Some((first, count)) = shapes_of(*c) else { continue };
        // Alone, joined to the letter before, to the letter after, to both.
        let which = match (before, after) {
            (false, false) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (true, true) => 3,
        };
        if which >= u32::from(count) {
            continue;
        }
        if let Some(shape) = char::from_u32(first + which).filter(|shape| has(*shape))
            && let Some(slot) = out.get_mut(at)
        {
            *slot = shape;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(text: &str, rtl: Option<bool>) -> String {
        let chars: Vec<char> = text.chars().collect();
        let levels = levels(text, rtl);
        let base = u8::from(rtl.or_else(|| first_strong(text)) == Some(true));
        order(&chars, &levels, base).into_iter().map(|i| if levels[i] % 2 == 1 { mirrored(chars[i]) } else { chars[i] }).collect()
    }

    #[test]
    fn right_to_left_text_is_drawn_in_its_own_order() {
        // Hebrew letters stand from the right; Latin words and numbers inside them keep their
        // own order; text with nothing right-to-left in it is left alone.
        assert_eq!(drawn("abc", None), "abc");
        assert_eq!(drawn("אבג", None), "גבא");
        assert_eq!(drawn("אב cd גד", None), "דג cd בא");
        assert_eq!(drawn("אב 123 גד", None), "דג 123 בא");
        assert_eq!(drawn("ab אב cd", None), "ab בא cd");
        // The paragraph's direction is the first letter's, unless it is told.
        assert_eq!(drawn("ab אב", Some(true)), "בא ab");
        assert_eq!(drawn("אב ab", Some(false)), "בא ab");
        // A bracket opens towards the text it holds.
        assert_eq!(drawn("א(ב)ג", None), "ג(ב)א");
        assert_eq!(mirrored('('), ')');
        assert_eq!(mirrored('»'), '«');
        assert_eq!(mirrored('x'), 'x');
        // Blanks at the end of a line stand at the paragraph's end: left of right-to-left text.
        assert_eq!(drawn("אב ", None), " בא");
        assert_eq!(drawn("ab אב ", Some(false)), "ab בא ");
        assert_eq!(first_strong("12 אב"), Some(true));
        assert_eq!(first_strong("12 ab א"), Some(false));
        assert_eq!(first_strong("12 ."), None);
        assert!(has_rtl("a\u{0627}") && !has_rtl("abc é 中"));
    }

    #[test]
    fn arabic_letters_take_the_shapes_they_have_beside_their_neighbours() {
        let shaped = |text: &str| -> Vec<u32> { joined(&text.chars().collect::<Vec<_>>(), &mut |_| true).into_iter().map(u32::from).collect() };
        // Beh three times: joined to the letter after, to both, to the letter before.
        assert_eq!(shaped("\u{0628}\u{0628}\u{0628}"), [0xFE91, 0xFE92, 0xFE90]);
        // Alone; and alef joins only the letter before it, so what follows it starts anew.
        assert_eq!(shaped("\u{0628}"), [0xFE8F]);
        assert_eq!(shaped("\u{0628}\u{0627}\u{0628}"), [0xFE91, 0xFE8E, 0xFE8F]);
        // A mark between two letters does not part them; a space does, and so does a Latin letter.
        assert_eq!(shaped("\u{0628}\u{064E}\u{0628}"), [0xFE91, 0x064E, 0xFE90]);
        assert_eq!(shaped("\u{0628} \u{0628}"), [0xFE8F, 0x20, 0xFE8F]);
        assert_eq!(shaped("\u{0628}a\u{0628}"), [0xFE8F, 0x61, 0xFE8F]);
        // Tatweel joins on both sides and stays what it is; hamza joins nothing.
        assert_eq!(shaped("\u{0628}\u{0640}\u{0628}"), [0xFE91, 0x0640, 0xFE90]);
        assert_eq!(shaped("\u{0628}\u{0621}\u{0628}"), [0xFE8F, 0x0621, 0xFE8F]);
        // Lam and alef are one letter, alone and after a letter that joins; the count is kept.
        assert_eq!(shaped("\u{0644}\u{0627}"), [0xFEFB, 0x200B]);
        assert_eq!(shaped("\u{0628}\u{0644}\u{0627}"), [0xFE91, 0xFEFC, 0x200B]);
        assert_eq!(shaped("\u{0644}\u{0623}\u{0628}"), [0xFEF7, 0x200B, 0xFE8F]);
        // Persian letters: peh joined to the letter after, farsi yeh to the letter before.
        assert_eq!(shaped("\u{067E}\u{06CC}"), [0xFB58, 0xFBFD]);
        // A font without the shapes: the letters stay as they are, lam and alef too.
        let plain: Vec<char> = "\u{0628}\u{0644}\u{0627}".chars().collect();
        assert_eq!(joined(&plain, &mut |_| false), plain);
        // Only the letters whose shape can be drawn are changed.
        assert_eq!(
            joined(&"\u{0628}\u{0628}".chars().collect::<Vec<_>>(), &mut |c| u32::from(c) == 0xFE91).into_iter().map(u32::from).collect::<Vec<_>>(),
            [0xFE91, 0x0628]
        );
    }

    #[test]
    fn hostile_text_keeps_its_length_and_never_panics() {
        let odd = [
            "\u{202E}\u{202E}\u{202E}abc\u{202C}\u{202C}\u{202C}\u{202C}",
            "\u{2067}\u{2066}\u{2068}א\u{2069}",
            "\u{0644}",
            "\u{0627}\u{0644}",
            "\u{064E}\u{064E}\u{064E}",
            "\n\n\u{05D0}\n\u{0627}\r\n",
            "",
        ];
        for text in odd {
            let chars: Vec<char> = text.chars().collect();
            for rtl in [None, Some(true), Some(false)] {
                let depth = levels(text, rtl);
                assert_eq!(depth.len(), chars.len());
                for cut in 0..=chars.len() {
                    let mut places = order(&chars[..cut], &depth[..cut], 1);
                    places.sort_unstable();
                    assert_eq!(places, (0..cut).collect::<Vec<_>>());
                }
            }
            assert_eq!(joined(&chars, &mut |_| true).len(), chars.len());
        }
        // Levels that are not the text's, and more of one than of the other.
        assert_eq!(order(&['a', 'b'], &[255, 255, 3], 0), [1, 0]);
        assert_eq!(order(&['a', 'b', 'c'], &[1], 7), [0]);
        assert!(order(&[], &[], 0).is_empty());
        // A deep nest of turns, and a text too long to be worked out.
        let deep: String = "\u{202E}".repeat(500) + "ab";
        assert_eq!(levels(&deep, None).len(), 502);
        let long: String = "\u{05D0}".repeat(MAX_CHARS + 1);
        assert!(levels(&long, None).iter().all(|level| *level == 0));
        let long: Vec<char> = long.chars().collect();
        assert_eq!(joined(&long, &mut |_| true).len(), long.len());
    }

    #[test]
    fn a_tab_stands_between_what_is_on_either_side_the_way_the_paragraph_is_read() {
        let places = |text: &str, rtl: bool| {
            let chars: Vec<char> = text.chars().collect();
            order(&chars, &levels(text, Some(rtl)), u8::from(rtl))
        };
        // Latin on both sides in a paragraph read from the right: the first stands at the right.
        assert_eq!(places("ab\tcd", true), [3, 4, 2, 0, 1]);
        // The blanks before the tab go with it.
        assert_eq!(places("a \tb", true), [3, 2, 1, 0]);
        // Hebrew on both sides in a paragraph read from the left: the first stays at the left.
        assert_eq!(places("\u{5d0}\u{5d1}\t\u{5d2}", false), [1, 0, 2, 3]);
        // A blank that is no tab still stands inside the words around it.
        assert_eq!(places("\u{5d0} \u{5d1}", false), [2, 1, 0]);
        assert_eq!(places("\t\t", true), [1, 0]);
        assert_eq!(order(&['\t'], &[9], 0), [0]);
    }
}
