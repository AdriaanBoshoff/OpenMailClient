//! Scripts the usual faces have no letters for, and the installed fonts that have them.
//!
//! The faces an interface or a message is set in mostly have the letters of the Latin,
//! Greek, Cyrillic, Hebrew and Arabic scripts and little else. Text in another script is
//! drawn in the first of the fonts listed for it that is installed: those that come with
//! Windows first, then the Noto families most Linux systems carry, then those of macOS. None
//! of them is bundled; where none is installed the letters are drawn as boxes. The ranges are
//! the Unicode blocks of each script (The Unicode Standard, chapter 2 and the block list).

/// A script, the characters that are its, and the fonts that have its letters, first
/// choice first.
pub struct Script {
    pub name: &'static str,
    ranges: &'static [(u32, u32)],
    pub fonts: &'static [&'static str],
}

impl Script {
    fn has(&self, c: char) -> bool {
        let c = u32::from(c);
        self.ranges.iter().any(|(from, to)| (*from..=*to).contains(&c))
    }
}

const fn script(name: &'static str, ranges: &'static [(u32, u32)], fonts: &'static [&'static str]) -> Script {
    Script { name, ranges, fonts }
}

pub const SCRIPTS: &[Script] = &[
    script(
        "Devanagari",
        &[(0x0900, 0x097F), (0xA8E0, 0xA8FF)],
        &["Nirmala UI", "Mangal", "Noto Sans Devanagari", "Lohit Devanagari", "Kohinoor Devanagari", "Devanagari Sangam MN"],
    ),
    script("Bengali", &[(0x0980, 0x09FF)], &["Nirmala UI", "Vrinda", "Noto Sans Bengali", "Lohit Bengali", "Kohinoor Bangla", "Bangla Sangam MN"]),
    script("Gurmukhi", &[(0x0A00, 0x0A7F)], &["Nirmala UI", "Raavi", "Noto Sans Gurmukhi", "Lohit Gurmukhi", "Gurmukhi Sangam MN"]),
    script(
        "Gujarati",
        &[(0x0A80, 0x0AFF)],
        &["Nirmala UI", "Shruti", "Noto Sans Gujarati", "Lohit Gujarati", "Kohinoor Gujarati", "Gujarati Sangam MN"],
    ),
    script("Oriya", &[(0x0B00, 0x0B7F)], &["Nirmala UI", "Kalinga", "Noto Sans Oriya", "Lohit Odia", "Oriya Sangam MN"]),
    script("Tamil", &[(0x0B80, 0x0BFF)], &["Nirmala UI", "Latha", "Noto Sans Tamil", "Lohit Tamil", "Tamil Sangam MN"]),
    script("Telugu", &[(0x0C00, 0x0C7F)], &["Nirmala UI", "Gautami", "Noto Sans Telugu", "Lohit Telugu", "Kohinoor Telugu", "Telugu Sangam MN"]),
    script("Kannada", &[(0x0C80, 0x0CFF)], &["Nirmala UI", "Tunga", "Noto Sans Kannada", "Lohit Kannada", "Kannada Sangam MN"]),
    script("Malayalam", &[(0x0D00, 0x0D7F)], &["Nirmala UI", "Kartika", "Noto Sans Malayalam", "Lohit Malayalam", "Malayalam Sangam MN"]),
    script("Sinhala", &[(0x0D80, 0x0DFF)], &["Nirmala UI", "Iskoola Pota", "Noto Sans Sinhala", "Sinhala Sangam MN"]),
    script("Thai", &[(0x0E00, 0x0E7F)], &["Leelawadee UI", "Leelawadee", "Tahoma", "Noto Sans Thai", "Loma", "Thonburi"]),
    script("Lao", &[(0x0E80, 0x0EFF)], &["Leelawadee UI", "Lao UI", "Noto Sans Lao", "Lao Sangam MN"]),
    script("Tibetan", &[(0x0F00, 0x0FFF)], &["Microsoft Himalaya", "Noto Serif Tibetan", "Kailasa"]),
    script("Myanmar", &[(0x1000, 0x109F), (0xA9E0, 0xA9FF), (0xAA60, 0xAA7F)], &["Myanmar Text", "Noto Sans Myanmar", "Myanmar MN"]),
    script("Georgian", &[(0x10A0, 0x10FF), (0x2D00, 0x2D2F), (0x1C90, 0x1CBF)], &["Segoe UI", "Sylfaen", "Noto Sans Georgian", "DejaVu Sans"]),
    script(
        "Hangul",
        &[(0x1100, 0x11FF), (0x3130, 0x318F), (0xA960, 0xA97F), (0xAC00, 0xD7FF)],
        &["Malgun Gothic", "Noto Sans CJK KR", "Noto Sans KR", "NanumGothic", "Apple SD Gothic Neo"],
    ),
    script("Ethiopic", &[(0x1200, 0x139F), (0x2D80, 0x2DDF), (0xAB00, 0xAB2F)], &["Ebrima", "Nyala", "Noto Sans Ethiopic", "Kefa"]),
    script("Cherokee", &[(0x13A0, 0x13FF), (0xAB70, 0xABBF)], &["Gadugi", "Plantagenet Cherokee", "Noto Sans Cherokee"]),
    script("Canadian Syllabics", &[(0x1400, 0x167F), (0x18B0, 0x18FF)], &["Gadugi", "Euphemia", "Noto Sans Canadian Aboriginal"]),
    script("Khmer", &[(0x1780, 0x17FF), (0x19E0, 0x19FF)], &["Leelawadee UI", "Khmer UI", "Noto Sans Khmer", "Khmer Sangam MN"]),
    script("Mongolian", &[(0x1800, 0x18AF)], &["Mongolian Baiti", "Noto Sans Mongolian"]),
    script(
        "Kana",
        &[(0x3040, 0x30FF), (0x31F0, 0x31FF), (0xFF65, 0xFF9F)],
        &["Yu Gothic UI", "Yu Gothic", "Meiryo", "MS Gothic", "Noto Sans CJK JP", "Noto Sans JP", "Hiragino Sans"],
    ),
    script(
        "Han",
        &[
            (0x2E80, 0x2FDF),
            (0x3000, 0x303F),
            (0x3100, 0x312F),
            (0x3190, 0x31EF),
            (0x3200, 0x33FF),
            (0x3400, 0x4DBF),
            (0x4E00, 0x9FFF),
            (0xF900, 0xFAFF),
            (0xFE30, 0xFE4F),
            (0xFF00, 0xFF64),
            (0xFFA0, 0xFFEF),
        ],
        &[
            "Microsoft YaHei UI",
            "Microsoft YaHei",
            "Microsoft JhengHei",
            "SimSun",
            "Noto Sans CJK SC",
            "Noto Sans SC",
            "WenQuanYi Micro Hei",
            "PingFang SC",
        ],
    ),
    script("Yi", &[(0xA000, 0xA4CF)], &["Microsoft Yi Baiti", "Noto Sans Yi"]),
    script("Javanese", &[(0xA980, 0xA9DF)], &["Javanese Text", "Noto Sans Javanese"]),
    script("Tifinagh", &[(0x2D30, 0x2D7F)], &["Ebrima", "Noto Sans Tifinagh"]),
    script("N'Ko", &[(0x07C0, 0x07FF)], &["Ebrima", "Noto Sans NKo"]),
    script("Thaana", &[(0x0780, 0x07BF)], &["MV Boli", "Noto Sans Thaana"]),
    script("Syriac", &[(0x0700, 0x074F)], &["Estrangelo Edessa", "Segoe UI Historic", "Noto Sans Syriac"]),
    script("Armenian", &[(0x0530, 0x058F)], &["Segoe UI", "Sylfaen", "Noto Sans Armenian", "DejaVu Sans"]),
    script(
        "Symbols",
        &[(0x2190, 0x21FF), (0x2300, 0x23FF), (0x2460, 0x24FF), (0x25A0, 0x27BF), (0x2900, 0x2BFF)],
        &["Segoe UI Symbol", "Noto Sans Symbols 2", "Noto Sans Symbols", "DejaVu Sans", "Apple Symbols"],
    ),
];

/// Which of [`SCRIPTS`] `c` is written in, for a character the usual faces may lack. Letters
/// below U+0530 (Latin, Greek, Cyrillic) are none of them.
pub fn script_of(c: char) -> Option<usize> {
    if u32::from(c) < 0x0530 {
        return None;
    }
    SCRIPTS.iter().position(|script| script.has(c))
}

/// Every font listed for any script, each once, in the order they are listed.
pub fn every_font() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for name in SCRIPTS.iter().flat_map(|script| script.fonts) {
        if !out.iter().any(|known| known.eq_ignore_ascii_case(name)) {
            out.push(name);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_script_is_known_by_its_letters() {
        let name = |c: char| script_of(c).and_then(|n| SCRIPTS.get(n)).map(|s| s.name);
        assert_eq!(name('a'), None);
        assert_eq!(name('\u{0416}'), None);
        assert_eq!(name('\u{0928}'), Some("Devanagari"));
        assert_eq!(name('\u{0E2A}'), Some("Thai"));
        assert_eq!(name('\u{AC00}'), Some("Hangul"));
        assert_eq!(name('\u{3042}'), Some("Kana"));
        assert_eq!(name('\u{4E2D}'), Some("Han"));
        assert_eq!(name('\u{1200}'), Some("Ethiopic"));
        // Every range is the right way round and every script has fonts to look for.
        for script in SCRIPTS {
            assert!(!script.fonts.is_empty(), "{}", script.name);
            assert!(script.ranges.iter().all(|(from, to)| from <= to), "{}", script.name);
        }
        let every = every_font();
        assert!(every.contains(&"Nirmala UI") && every.contains(&"Noto Sans CJK SC"));
        assert_eq!(every.iter().filter(|n| **n == "Nirmala UI").count(), 1);
    }
}
