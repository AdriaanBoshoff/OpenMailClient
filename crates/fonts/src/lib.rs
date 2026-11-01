//! Finds the fonts a message names among the fonts installed on this machine.
//!
//! No font is bundled here and none is fetched. A message names fonts as a list, first choice
//! first (CSS Fonts Level 3, section 3.1, `font-family`); [`wanted`] turns that list into the
//! family names to look for, adding open fonts drawn to the same measures as well-known
//! proprietary ones, so that a message asking for a font that is not installed keeps its line
//! lengths. [`Library`] then finds an installed face by family name and hands back its file.
//!
//! The names come from the message and are hostile: they are only ever compared with the names
//! of installed fonts. A font file is checked to be readable before it is handed on.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod boxes;
pub mod cover;
mod letters;
pub mod script;

pub use letters::{Letter, Lettering, Letters, Ring, Scribe, letters, shapes};

use std::collections::HashMap;
use std::sync::Arc;

use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use skrifa::raw::TableProvider;
use skrifa::raw::types::Tag;
use skrifa::{FontRef, MetadataProvider};

/// Names read from one list, and names looked for on its behalf.
const MAX_LISTED: usize = 8;
const MAX_NAMES: usize = 32;
const MAX_NAME_LEN: usize = 64;
/// Largest font file handed on.
const MAX_FACE_BYTES: usize = 16 << 20;

/// What a message shows unknown fonts in, and what `serif` means: as Outlook Classic was seen to.
const SERIF: &[&str] = &["Times New Roman", "Liberation Serif", "Tinos", "DejaVu Serif", "Noto Serif"];
const SANS: &[&str] = &["Arial", "Liberation Sans", "Arimo", "Helvetica", "DejaVu Sans", "Noto Sans"];

/// The fonts to look for, in order, for a list of names as a message gives it. An empty answer
/// means the reader's own font. A list that names only fonts, none of them a kind of font, ends
/// with the serif fonts.
pub fn wanted(list: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let add = |names: &mut Vec<String>, name: &str| {
        if names.len() < MAX_NAMES && !names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            names.push(name.to_string());
        }
    };
    for listed in list.split(',').take(MAX_LISTED) {
        let name: String =
            listed.trim().trim_matches(|c| c == '"' || c == '\'').trim().chars().filter(|c| !c.is_control()).take(MAX_NAME_LEN).collect();
        match name.to_ascii_lowercase().as_str() {
            "" | "cursive" | "fantasy" | "emoji" | "math" | "fangsong" => {}
            "serif" | "ui-serif" => {
                SERIF.iter().for_each(|n| add(&mut names, n));
                return names;
            }
            "sans-serif" | "ui-sans-serif" => {
                SANS.iter().for_each(|n| add(&mut names, n));
                return names;
            }
            // The reader has its own fixed-width font and its own interface font.
            "monospace" | "ui-monospace" | "system-ui" | "ui-rounded" | "inherit" | "initial" => return names,
            lower => {
                add(&mut names, &name);
                stand_ins(lower).iter().for_each(|n| add(&mut names, n));
            }
        }
    }
    SERIF.iter().for_each(|n| add(&mut names, n));
    names
}

/// Open fonts that keep the measures of a proprietary one, or come close to it; `name` in lower
/// case. The pairs are the ones their makers publish: Liberation and the Croscore fonts (Arimo,
/// Tinos, Cousine) for Arial, Times New Roman and Courier New; Carlito for Calibri; Caladea for
/// Cambria; Gelasio for Georgia; Selawik for Segoe UI.
fn stand_ins(name: &str) -> &'static [&'static str] {
    match name {
        "arial" => &["Liberation Sans", "Arimo", "Helvetica"],
        "helvetica" | "helvetica neue" => &["Arial", "Liberation Sans", "Arimo"],
        "times new roman" | "times" => &["Times New Roman", "Liberation Serif", "Tinos"],
        "courier new" | "courier" => &["Courier New", "Liberation Mono", "Cousine"],
        "calibri" => &["Carlito"],
        "cambria" => &["Caladea"],
        "georgia" => &["Gelasio"],
        "segoe ui" => &["Selawik", "Open Sans", "Noto Sans"],
        "aptos" => &["Carlito", "Noto Sans"],
        "verdana" | "tahoma" => &["DejaVu Sans"],
        "consolas" => &["Inconsolata", "DejaVu Sans Mono"],
        _ => &[],
    }
}

/// One installed face: its file and which face of the file it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Face {
    pub data: Vec<u8>,
    pub index: u32,
}

/// The fonts installed on this machine, by family name.
#[derive(Default)]
pub struct Library {
    fonts: Arc<Database>,
    /// Family names as the fonts spell them, by their lower-case form: a message may write a
    /// name in any case.
    spelled: HashMap<String, String>,
}

impl Library {
    /// No fonts at all.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The fonts the system has installed. Reads the name of every font, which takes a moment.
    pub fn system() -> Self {
        let mut fonts = Database::new();
        fonts.load_system_fonts();
        let mut spelled = HashMap::new();
        for (name, _) in fonts.faces().flat_map(|face| face.families.iter()) {
            spelled.entry(name.to_lowercase()).or_insert_with(|| name.clone());
        }
        Self { fonts: Arc::new(fonts), spelled }
    }

    /// The same list for another reader of fonts in this program: it is shared, and no font
    /// is listed twice.
    pub fn shared(&self) -> Arc<Database> {
        Arc::clone(&self.fonts)
    }

    /// How many faces are known.
    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }

    /// Whether a family of this name is installed, in any letter case.
    pub fn has(&self, family: &str) -> bool {
        !family.is_empty() && family.len() <= MAX_NAME_LEN * 4 && self.spelled.contains_key(&family.to_lowercase())
    }

    /// One face of `family`: upright or italic, regular or bold. `None` when the family is not
    /// installed, when it has no such face (a bold or an italic one is never made up from
    /// another), or when the file is too large or cannot be read as a font.
    pub fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Face> {
        if family.is_empty() || family.len() > MAX_NAME_LEN * 4 {
            return None;
        }
        let family = self.spelled.get(&family.to_lowercase())?;
        let query = Query {
            families: &[Family::Name(family)],
            weight: if bold { Weight::BOLD } else { Weight::NORMAL },
            stretch: Stretch::Normal,
            style: if italic { Style::Italic } else { Style::Normal },
        };
        let id = self.fonts.query(&query)?;
        // The nearest face is found whatever it is; a light one is not a bold one, and an
        // upright one is not an italic one.
        let found = self.fonts.face(id)?;
        if (found.weight.0 >= 600) != bold || (found.style != Style::Normal) != italic {
            return None;
        }
        self.fonts
            .with_face_data(id, |data, index| (data.len() <= MAX_FACE_BYTES && readable(data, index)).then(|| Face { data: data.to_vec(), index }))
            .flatten()
    }
}

/// The file holds a font at `index` that can be opened and that maps characters to glyphs.
pub fn readable(data: &[u8], index: u32) -> bool {
    skrifa::FontRef::from_index(data, index).is_ok_and(|font| font.charmap().has_map())
}

/// How a font may be put into a document that is handed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Embedding {
    /// Only the glyphs the document uses.
    Subset,
    /// The whole font or nothing: its maker does not allow parts of it.
    Whole,
}

/// Whether the face may be put into a document, and how: it must hold TrueType outlines, and
/// its maker must allow embedding (OpenType specification, `OS/2`, `fsType`). `None` when it
/// may not, or when the file cannot be read.
pub fn embedding(data: &[u8], index: u32) -> Option<Embedding> {
    let font = FontRef::from_index(data, index).ok()?;
    for tag in [b"glyf", b"loca", b"head", b"hhea", b"hmtx", b"maxp"] {
        font.data_for_tag(Tag::new(tag))?;
    }
    let Some(os2) = font.data_for_tag(Tag::new(b"OS/2")) else {
        return Some(Embedding::Subset);
    };
    let bytes = os2.as_bytes();
    let permissions = u16::from_be_bytes([*bytes.get(8)?, *bytes.get(9)?]);
    // Bit 1 alone: no embedding. Bit 9: bitmaps only, and no bitmaps are written here.
    let restricted = permissions & 0x0002 != 0 && permissions & 0x000c == 0;
    if restricted || permissions & 0x0200 != 0 {
        return None;
    }
    Some(if permissions & 0x0100 != 0 { Embedding::Whole } else { Embedding::Subset })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_font_says_whether_it_may_be_embedded() {
        let free = boxes::font("ab ");
        assert!(readable(&free, 0));
        assert_eq!(embedding(&free, 0), Some(Embedding::Subset));
        assert_eq!(embedding(&boxes::font_with("ab", 0x0002), 0), None, "restricted");
        assert_eq!(embedding(&boxes::font_with("ab", 0x0200), 0), None, "bitmaps only");
        assert_eq!(embedding(&boxes::font_with("ab", 0x0004), 0), Some(Embedding::Subset), "print and preview");
        assert_eq!(embedding(&boxes::font_with("ab", 0x0006), 0), Some(Embedding::Subset), "the widest permission counts");
        assert_eq!(embedding(&boxes::font_with("ab", 0x0108), 0), Some(Embedding::Whole));
        assert_eq!(embedding(&free, 3), None);
        assert_eq!(embedding(b"not a font", 0), None);
        for cut in [0, 11, 12, 100, free.len() / 2, free.len() - 1] {
            let _ = embedding(free.get(..cut).unwrap_or_default(), 0);
        }
    }

    #[test]
    fn lists_become_names_to_look_for() {
        assert_eq!(wanted("Georgia"), ["Georgia", "Gelasio", "Times New Roman", "Liberation Serif", "Tinos", "DejaVu Serif", "Noto Serif"]);
        assert_eq!(wanted(" 'Nosuch Font' , \"Calibri\",sans-serif, Georgia")[..3], ["Nosuch Font", "Calibri", "Carlito"]);
        assert!(wanted("Calibri, sans-serif").iter().any(|n| n == "Arial"));
        assert!(!wanted("Calibri, sans-serif, Georgia").iter().any(|n| n == "Georgia"), "the list ends at a kind of font");
        assert_eq!(
            wanted("ARIAL, arial, Arial"),
            ["ARIAL", "Liberation Sans", "Arimo", "Helvetica", "Times New Roman", "Liberation Serif", "Tinos", "DejaVu Serif", "Noto Serif"]
        );
        assert_eq!(wanted("monospace"), [] as [&str; 0]);
        assert_eq!(wanted("system-ui, Arial"), [] as [&str; 0]);
        assert_eq!(wanted("cursive, , fantasy"), SERIF);
        assert_eq!(wanted(""), SERIF);
    }

    #[test]
    fn hostile_lists_are_bounded() {
        let long = "x".repeat(100_000);
        assert!(wanted(&long).iter().all(|n| n.chars().count() <= MAX_NAME_LEN));
        let many = (0..10_000).map(|i| format!("font{i}")).collect::<Vec<_>>().join(",");
        assert!(wanted(&many).len() <= MAX_NAMES);
        for list in [
            "\u{0}\u{1}\u{202e},\"\"\",'''",
            ",,,,,,,,,,,,,,,,",
            "\"unclosed, 'mixed\", é\u{301}\u{fe0f}",
            "arial\narial\r\n",
            "..\\..\\Windows\\Fonts\\arial.ttf, C:/x.ttf",
        ] {
            let names = wanted(list);
            assert!(names.len() <= MAX_NAMES && names.iter().all(|n| !n.chars().any(char::is_control)), "{names:?}");
        }
    }

    #[test]
    fn faces_come_only_from_the_library() {
        let none = Library::empty();
        assert!(none.is_empty());
        assert_eq!(none.face("Arial", false, false), None);
        assert!(!none.has("Arial"));

        // Whatever this machine has installed: a name is only ever looked up, never opened as a path.
        let system = Library::system();
        for name in ["", "No Such Font At All 7f3a", "..\\..\\Windows\\win.ini", "C:\\Windows\\Fonts\\arial.ttf", &"y".repeat(10_000)] {
            assert!(!system.has(name));
            for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                assert_eq!(system.face(name, bold, italic), None, "{name:?}");
            }
        }
        for name in SERIF.iter().chain(SANS) {
            let mut faces = Vec::new();
            for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                if let Some(face) = system.face(name, bold, italic) {
                    assert!(system.has(name) && system.has(&name.to_uppercase()));
                    assert!(face.data.len() <= MAX_FACE_BYTES && readable(&face.data, face.index));
                    // One face is never handed out as another.
                    assert!(!faces.contains(&face), "{name} {bold} {italic}");
                    faces.push(face);
                }
            }
        }
    }

    #[test]
    fn text_becomes_the_shapes_of_its_letters() {
        let face = Face { data: boxes::font("ab "), index: 0 };
        let line = face.letters("Any Name", true, true, "a b").unwrap();
        assert_eq!((line.ascent, line.descent), (0.8, 0.2));
        assert_eq!(line.each.iter().map(|l| (l.rings.len(), l.advance)).collect::<Vec<_>>(), [(1, 0.5), (0, 0.25), (1, 0.5)]);
        // A box from 0.05 to 0.45 of the em across, 0.7 of it high, standing on the line.
        let reach =
            |ring: &Ring| ring.iter().fold((1.0f32, 1.0f32, -1.0f32, -1.0f32), |r, p| (r.0.min(p.0), r.1.min(p.1), r.2.max(p.0), r.3.max(p.1)));
        assert_eq!(reach(&line.each[0].rings[0]), (0.05, -0.7, 0.45, 0.0));
        // Leaning: the top is further along than the foot.
        let leaning = letters(&face.data, 0, true, "a").unwrap();
        assert!(reach(&leaning.each[0].rings[0]).2 > 0.55);
        // A character that only moves the line on draws nothing; another kind of space is a space.
        assert_eq!(face.letters("", false, false, "a\tb\u{a0}").unwrap().each.iter().map(|l| l.rings.len()).collect::<Vec<_>>(), [1, 0, 1, 0]);

        // No letters: a character the font lacks; letters that join or run right to left; a mark
        // that stands on another letter; more text than a line holds; a file that is no font.
        for text in ["az", "a\u{5d0}", "a\u{627}", "a\u{301}", "a\u{915}", "a\u{200f}b", &"a".repeat(5000)] {
            assert_eq!(face.letters("", false, false, text), None, "{}", text.len());
        }
        assert_eq!(letters(b"not a font", 0, false, "a"), None);
        assert_eq!(letters(&face.data, 9, false, "a"), None);
        for cut in [0, 11, 12, 100, face.data.len() / 2, face.data.len() - 1] {
            let _ = letters(face.data.get(..cut).unwrap_or_default(), 0, false, "ab ");
        }
    }

    #[test]
    fn a_scribe_draws_only_in_fonts_of_its_library() {
        let none = Scribe::with(Library::empty());
        assert_eq!(none.letters("Arial", false, false, "a"), None);
        assert_eq!(none.letters("", true, true, "a"), None);
        // Whatever this machine has: a name is only looked up, and is asked for again from memory.
        let system = Scribe::system();
        for name in ["..\\..\\Windows\\win.ini", "C:\\Windows\\Fonts\\arial.ttf", &"y".repeat(10_000), "Arial", "Arial"] {
            for (bold, italic) in [(false, false), (true, true)] {
                if let Some(line) = system.letters(name, bold, italic, "Ab 1") {
                    assert_eq!(line.each.len(), 4);
                    assert!(line.ascent > 0.0 && line.each.iter().all(|l| l.advance.is_finite()));
                }
            }
        }
    }

    #[test]
    fn damaged_font_files_are_not_readable() {
        assert!(!readable(&[], 0));
        assert!(!readable(b"not a font at all", 0));
        assert!(!readable(&[0, 1, 0, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff], 0));
        assert!(!readable(b"ttcf\0\x01\0\0\xff\xff\xff\xff", 7));
        assert!(!readable(&vec![0xff; 4096], u32::MAX));
    }
}
