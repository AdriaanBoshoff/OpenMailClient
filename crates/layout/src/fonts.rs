//! The faces a layout is set in: found by name among the installed fonts, read once, and asked
//! for glyphs and measures. Table layouts are from the OpenType specification (`head`, `hhea`,
//! `hmtx`, `maxp`); every number in a font file is read as hostile.
//!
//! Pairs of glyphs that stand closer or further apart than their widths say (kerning) are
//! read from the `kern` feature of the `GPOS` table (pair adjustment, lookup type 2), or
//! from the `kern` table (format 0) of a face that has no such feature.
//!
//! A face with a `GSUB` or a `GPOS` table shapes its own text ([`Fonts::shape`]): the glyphs a
//! run of characters is drawn with, how far each moves on and where it stands are worked out
//! from those tables by `harfrust`, a port of HarfBuzz. The measures above are what is left
//! for a face that has neither table.

use std::collections::HashMap;
use std::sync::Arc;

use openmailclient_fonts::{Face, Library};
use skrifa::raw::TableProvider;
use skrifa::raw::tables::gpos::{PairPos, PositionSubtables};
use skrifa::raw::tables::kern::SubtableKind;
use skrifa::raw::types::Tag;
use skrifa::{FontRef, GlyphId, MetadataProvider};

/// Faces one layout may load.
const MAX_FACES: usize = 48;
/// Characters remembered per face.
const MAX_REMEMBERED: usize = 20_000;
/// Pairs of glyphs remembered per face.
const MAX_PAIRS: usize = 60_000;
/// Lookups, and subtables of one, that are asked about a pair.
const MAX_LOOKUPS: usize = 32;
const MAX_SUBTABLES: usize = 256;

/// Fixed-width fonts, for text that asks for one.
const MONO: &[&str] = &["Consolas", "Inconsolata", "Courier New", "Liberation Mono", "Cousine", "DejaVu Sans Mono", "Noto Sans Mono"];
/// Looked through, after the fonts a text names, for a character none of those has.
const LAST: &[&str] = &[
    "Segoe UI",
    "Arial",
    "Liberation Sans",
    "DejaVu Sans",
    "Noto Sans",
    "Segoe UI Symbol",
    "Noto Sans Symbols",
    "Segoe UI Emoji",
    "Microsoft YaHei",
    "Noto Sans CJK SC",
];

/// Where a face says which pairs of glyphs stand closer together.
#[derive(Clone, Debug)]
enum Pairs {
    None,
    /// The lookups of the `kern` feature of its `GPOS` table, in the order they apply.
    Lookups(Vec<u16>),
    /// Its `kern` table.
    Table,
}

/// What shaping text from a face's own tables needs, read once.
#[derive(Clone)]
struct Shaping {
    data: Arc<harfrust::ShaperData>,
    /// The face joins Arabic letters itself (it has an `init` feature).
    joins: bool,
}

impl std::fmt::Debug for Shaping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shaping").field("joins", &self.joins).finish()
    }
}

/// One glyph of a shaped run: which, the character of the run it comes from, and in ems how
/// far it moves the line on and how far from its place it stands (up is more).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Shaped {
    pub glyph: u16,
    pub cluster: usize,
    pub advance: f32,
    pub dx: f32,
    pub dy: f32,
}

/// A face that is used: its file, and its measures in font units.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub data: Arc<[u8]>,
    pub index: u32,
    units: f32,
    ascent: f32,
    descent: f32,
    gap: f32,
    advances: Vec<u16>,
    chars: HashMap<char, u16>,
    pairs: Pairs,
    kerns: HashMap<(u16, u16), i16>,
    shaping: Option<Shaping>,
}

impl Loaded {
    /// Height above the baseline, depth below it and room between lines, at `size`.
    pub(crate) fn line(&self, size: f32) -> (f32, f32, f32) {
        let scale = size / self.units;
        (self.ascent * scale, self.descent * scale, self.gap * scale)
    }

    /// How far `glyph` moves the line on at `size`.
    pub fn advance(&self, glyph: u16, size: f32) -> f32 {
        f32::from(self.advances.get(usize::from(glyph)).copied().unwrap_or(0)) * size / self.units
    }
}

/// A face picked for a text, and what the face lacks of the style asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pick {
    pub face: usize,
    pub bold: bool,
    pub slant: bool,
}

/// The font names one piece of text looks through, first choice first.
pub(crate) struct Chain {
    names: Vec<String>,
    bold: bool,
    italic: bool,
    picks: Vec<Option<Option<Pick>>>,
}

pub struct Fonts<'a> {
    library: &'a Library,
    given: Vec<(String, bool, bool, Face)>,
    faces: Vec<Loaded>,
    found: HashMap<(String, bool, bool), Option<usize>>,
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*data.get(at)?, *data.get(at.checked_add(1)?)?]))
}

/// The lookups a face kerns Latin text with: those of the `kern` feature in the default
/// language system of its `latn` script, else of `DFLT`, else of the first script it has
/// (OpenType, "OpenType Layout Common Table Formats": script list, language system, feature
/// list).
fn kern_lookups(font: &FontRef) -> Option<Vec<u16>> {
    let gpos = font.gpos().ok()?;
    let scripts = gpos.script_list().ok()?;
    let records = scripts.script_records();
    let named = |tag: &[u8; 4]| records.iter().find(|r| r.script_tag() == Tag::new(tag));
    let script = named(b"latn").or_else(|| named(b"DFLT")).or_else(|| records.first())?.script(scripts.offset_data()).ok()?;
    let system = match script.default_lang_sys() {
        Some(system) => system.ok()?,
        None => script.lang_sys_records().first()?.lang_sys(script.offset_data()).ok()?,
    };
    let features = gpos.feature_list().ok()?;
    let mut out: Vec<u16> = Vec::new();
    for index in system.feature_indices().iter().take(1024) {
        let Some(record) = features.feature_records().get(usize::from(index.get())) else { continue };
        if record.feature_tag() != Tag::new(b"kern") {
            continue;
        }
        let Ok(feature) = record.feature(features.offset_data()) else { continue };
        for lookup in feature.lookup_list_indices() {
            if out.len() < MAX_LOOKUPS && !out.contains(&lookup.get()) {
                out.push(lookup.get());
            }
        }
    }
    // Lookups apply in the order of the lookup list.
    out.sort_unstable();
    (!out.is_empty()).then_some(out)
}

/// What one pair adjustment subtable says of a pair: how much further the first glyph moves
/// on, in font units. `None` when the subtable does not speak of the pair, and the next one is
/// asked (OpenType `GPOS`, "Lookup type 2 subtable: pair adjustment positioning").
fn pair_value(subtable: &PairPos, left: u16, right: u16) -> Option<i32> {
    match subtable {
        PairPos::Format1(pairs) => {
            let at = pairs.coverage().ok()?.get(GlyphId::from(left))?;
            let set = pairs.pair_sets().get(usize::from(at)).ok()?;
            for record in set.pair_value_records().iter() {
                let record = record.ok()?;
                if record.second_glyph().to_u16() == right {
                    return Some(i32::from(record.value_record1().x_advance().unwrap_or(0)));
                }
            }
            None
        }
        PairPos::Format2(classes) => {
            classes.coverage().ok()?.get(GlyphId::from(left))?;
            let first = classes.class_def1().ok()?.get(GlyphId::from(left));
            let second = classes.class_def2().ok()?.get(GlyphId::from(right));
            let row = classes.class1_records().get(usize::from(first)).ok()?;
            let record = row.class2_records().get(usize::from(second)).ok()?;
            Some(i32::from(record.value_record1().x_advance().unwrap_or(0)))
        }
    }
}

/// How much further than its width `left` moves on before `right`, in font units.
fn pair(loaded: &Loaded, left: u16, right: u16) -> Option<i32> {
    let font = FontRef::from_index(&loaded.data, loaded.index).ok()?;
    let mut sum = 0i32;
    match &loaded.pairs {
        Pairs::None => return None,
        Pairs::Lookups(lookups) => {
            let list = font.gpos().ok()?.lookup_list().ok()?;
            for index in lookups {
                let Ok(lookup) = list.lookups().get(usize::from(*index)) else { continue };
                let Ok(PositionSubtables::Pair(subtables)) = lookup.subtables() else { continue };
                // The first subtable that speaks of the pair is the lookup's answer.
                if let Some(value) = subtables.iter().take(MAX_SUBTABLES).flatten().find_map(|s| pair_value(&s, left, right)) {
                    sum = sum.saturating_add(value);
                }
            }
        }
        Pairs::Table => {
            // OpenType `kern`: pairs of the subtables that kern along the line.
            for subtable in font.kern().ok()?.subtables().take(MAX_LOOKUPS).flatten() {
                if !subtable.is_horizontal() || subtable.is_cross_stream() || subtable.is_variable() {
                    continue;
                }
                if let Ok(SubtableKind::Format0(pairs)) = subtable.kind() {
                    sum = sum.saturating_add(pairs.kerning(GlyphId::from(left), GlyphId::from(right)).unwrap_or(0));
                }
            }
        }
    }
    Some(sum)
}

fn load(face: Face) -> Option<Loaded> {
    openmailclient_fonts::embedding(&face.data, face.index)?;
    let font = FontRef::from_index(&face.data, face.index).ok()?;
    let table = |tag: &[u8; 4]| font.data_for_tag(Tag::new(tag)).map(|d| d.as_bytes());
    let (head, hhea, hmtx, maxp) = (table(b"head")?, table(b"hhea")?, table(b"hmtx")?, table(b"maxp")?);
    let units = be16(head, 18).filter(|u| *u >= 16)?;
    let (mut ascent, mut descent) = (f32::from(be16(hhea, 4)? as i16), -f32::from(be16(hhea, 6)? as i16));
    // A font that gives no heights, or absurd ones, is set as a common one is.
    let sane = |v: f32| (0.0..=f32::from(units) * 4.0).contains(&v);
    if !sane(ascent) || !sane(descent) || ascent + descent <= 0.0 {
        (ascent, descent) = (f32::from(units) * 0.8, f32::from(units) * 0.2);
    }
    let gap = f32::from(be16(hhea, 8)? as i16).clamp(0.0, f32::from(units));
    let glyphs = usize::from(be16(maxp, 4)?);
    let full = usize::from(be16(hhea, 34)?);
    let mut advances = Vec::with_capacity(glyphs);
    let mut last = 0u16;
    for glyph in 0..glyphs {
        if glyph < full
            && let Some(advance) = be16(hmtx, glyph * 4)
        {
            // No glyph moves on by more than a few ems.
            last = advance.min(units.saturating_mul(8));
        }
        advances.push(last);
    }
    let pairs = match kern_lookups(&font) {
        Some(lookups) => Pairs::Lookups(lookups),
        None if font.kern().is_ok() => Pairs::Table,
        None => Pairs::None,
    };
    let init = Tag::new(b"init");
    let joins =
        font.gsub().ok().and_then(|gsub| gsub.feature_list().ok()).is_some_and(|list| list.feature_records().iter().any(|r| r.feature_tag() == init));
    let shaping = (table(b"GSUB").is_some() || table(b"GPOS").is_some())
        .then(|| harfrust::FontRef::from_index(&face.data, face.index).ok())
        .flatten()
        .map(|font| Shaping { data: Arc::new(harfrust::ShaperData::new(&font)), joins });
    Some(Loaded {
        data: face.data.into(),
        index: face.index,
        units: f32::from(units),
        ascent,
        descent,
        gap,
        advances,
        chars: HashMap::new(),
        pairs,
        kerns: HashMap::new(),
        shaping,
    })
}

impl<'a> Fonts<'a> {
    pub fn new(library: &'a Library) -> Self {
        Self { library, given: Vec::new(), faces: Vec::new(), found: HashMap::new() }
    }

    /// Hand over a face under a family name, to be found before the installed ones.
    pub fn give(&mut self, family: &str, bold: bool, italic: bool, face: Face) {
        self.given.push((family.to_lowercase(), bold, italic, face));
    }

    pub fn faces(&self) -> &[Loaded] {
        &self.faces
    }

    pub(crate) fn face(&self, face: usize) -> Option<&Loaded> {
        self.faces.get(face)
    }

    /// The face of exactly this family and style, loaded once.
    fn exact(&mut self, family: &str, bold: bool, italic: bool) -> Option<usize> {
        let key = (family.to_lowercase(), bold, italic);
        if let Some(known) = self.found.get(&key) {
            return *known;
        }
        let face = match self.given.iter().find(|(name, b, i, _)| *name == key.0 && *b == bold && *i == italic) {
            Some((.., face)) => Some(face.clone()),
            None => self.library.face(family, bold, italic),
        };
        let loaded = if self.faces.len() < MAX_FACES { face.and_then(load) } else { None };
        let at = loaded.map(|loaded| {
            self.faces.push(loaded);
            self.faces.len() - 1
        });
        self.found.insert(key, at);
        at
    }

    /// A face of the family for the style: its own when it has one, else a plainer one that is
    /// then thickened or leant.
    fn pick(&mut self, family: &str, bold: bool, italic: bool) -> Option<Pick> {
        if let Some(face) = self.exact(family, bold, italic) {
            return Some(Pick { face, bold: false, slant: false });
        }
        if italic && let Some(face) = self.exact(family, bold, false) {
            return Some(Pick { face, bold: false, slant: true });
        }
        if bold && let Some(face) = self.exact(family, false, italic) {
            return Some(Pick { face, bold: true, slant: false });
        }
        if bold && italic {
            return self.exact(family, false, false).map(|face| Pick { face, bold: true, slant: true });
        }
        None
    }

    /// The names to look through for text that asks for `list` (as a message writes it; empty
    /// for none), fixed-width or not.
    pub(crate) fn chain(&self, list: &str, mono: bool, bold: bool, italic: bool) -> Chain {
        let mut names: Vec<String> = Vec::new();
        let listed = if list.trim().is_empty() { Vec::new() } else { openmailclient_fonts::wanted(list) };
        let own = listed.into_iter();
        let mono_names = MONO.iter().filter(|_| mono).map(|n| n.to_string());
        for name in own.chain(mono_names).chain(LAST.iter().map(|n| n.to_string())) {
            if !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                names.push(name);
            }
        }
        let picks = vec![None; names.len()];
        Chain { names, bold, italic, picks }
    }

    /// How much further than its width `left` moves on when `right` follows it in the same
    /// face, at `size`: less than nothing for a pair that stands closer together. No pair
    /// moves by more than half an em.
    pub(crate) fn kern(&mut self, face: usize, left: u16, right: u16, size: f32) -> f32 {
        let Some(loaded) = self.faces.get_mut(face) else { return 0.0 };
        if matches!(loaded.pairs, Pairs::None) {
            return 0.0;
        }
        let units = match loaded.kerns.get(&(left, right)) {
            Some(known) => *known,
            None => {
                let most = (loaded.units / 2.0) as i32;
                let found = i16::try_from(pair(loaded, left, right).unwrap_or(0).clamp(-most, most)).unwrap_or(0);
                if loaded.kerns.len() < MAX_PAIRS {
                    loaded.kerns.insert((left, right), found);
                }
                found
            }
        };
        f32::from(units) * size / loaded.units
    }

    /// A run of characters, each with its number in its paragraph, as the face's own tables
    /// shape it: the glyphs from the left as they are drawn, so for text written from the right
    /// the last character's glyph comes first. `None` for a face that does not shape its own
    /// text, for Arabic in a face that does not join it, and for an answer that names a glyph
    /// the face has not got. `before` and `after` are what stands beside the run in another
    /// face or look: a letter joins the one beside it across such a change.
    pub(crate) fn shape(&self, face: usize, text: &[(usize, char)], rtl: bool, before: &str, after: &str) -> Option<Vec<Shaped>> {
        let loaded = self.faces.get(face)?;
        let shaping = loaded.shaping.as_ref()?;
        if !shaping.joins && text.iter().any(|(_, c)| matches!(c, '\u{0600}'..='\u{06FF}')) {
            return None;
        }
        let font = harfrust::FontRef::from_index(&loaded.data, loaded.index).ok()?;
        let shaper = shaping.data.shaper(&font).build();
        let mut buffer = harfrust::UnicodeBuffer::new();
        for (at, ch) in text {
            buffer.add(*ch, u32::try_from(*at).ok()?);
        }
        buffer.set_pre_context(before);
        buffer.set_post_context(after);
        buffer.set_direction(if rtl { harfrust::Direction::RightToLeft } else { harfrust::Direction::LeftToRight });
        buffer.guess_segment_properties();
        let shaped = shaper.shape(buffer, harfrust::ShapeOptions::new());
        let em = |units: i32| units as f32 / loaded.units;
        let mut out = Vec::with_capacity(shaped.glyph_infos().len());
        for (info, pos) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
            let glyph = u16::try_from(info.glyph_id).ok().filter(|glyph| usize::from(*glyph) < loaded.advances.len())?;
            out.push(Shaped {
                glyph,
                cluster: usize::try_from(info.cluster).ok()?,
                advance: em(pos.x_advance),
                dx: em(pos.x_offset),
                dy: em(pos.y_offset),
            });
        }
        Some(out)
    }

    pub(crate) fn glyph(&mut self, face: usize, ch: char) -> u16 {
        let Some(loaded) = self.faces.get_mut(face) else { return 0 };
        if let Some(glyph) = loaded.chars.get(&ch) {
            return *glyph;
        }
        let glyph = FontRef::from_index(&loaded.data, loaded.index)
            .ok()
            .and_then(|font| font.charmap().map(ch))
            .and_then(|glyph| u16::try_from(glyph.to_u32()).ok())
            .filter(|glyph| usize::from(*glyph) < loaded.advances.len())
            .unwrap_or(0);
        if loaded.chars.len() < MAX_REMEMBERED {
            loaded.chars.insert(ch, glyph);
        }
        glyph
    }

    /// The glyph for `ch` in the first font of the chain that has one; in the first font there
    /// is, its "not defined" glyph, when none has. `None` when no font of the chain is there.
    pub(crate) fn resolve(&mut self, chain: &mut Chain, ch: char) -> Option<(Pick, u16)> {
        let mut first: Option<Pick> = None;
        for k in 0..chain.names.len() {
            let known = chain.picks.get(k).copied().flatten();
            let pick = match known {
                Some(pick) => pick,
                None => {
                    let pick = chain.names.get(k).and_then(|name| self.pick(name, chain.bold, chain.italic));
                    if let Some(slot) = chain.picks.get_mut(k) {
                        *slot = Some(pick);
                    }
                    pick
                }
            };
            let Some(pick) = pick else { continue };
            let glyph = self.glyph(pick.face, ch);
            if glyph != 0 {
                return Some((pick, glyph));
            }
            first = first.or(Some(pick));
        }
        // An italic or a bold face often has fewer characters than the plain face of its
        // family (no Arabic, no Hebrew): then the plain one is leant or thickened, which is
        // better than a box.
        if chain.bold || chain.italic {
            for k in 0..chain.names.len() {
                let Some(name) = chain.names.get(k).cloned() else { continue };
                for (bold, italic) in [(chain.bold, false), (false, chain.italic), (false, false)] {
                    let Some(face) = self.exact(&name, bold, italic) else { continue };
                    let glyph = self.glyph(face, ch);
                    if glyph != 0 {
                        return Some((Pick { face, bold: chain.bold && !bold, slant: chain.italic && !italic }, glyph));
                    }
                }
            }
        }
        // A letter of a script none of these has is looked for in the fonts that script is
        // written in, those of it that are installed.
        let script = openmailclient_fonts::cover::script_of(ch).and_then(|n| openmailclient_fonts::cover::SCRIPTS.get(n));
        for name in script.map(|script| script.fonts).unwrap_or_default() {
            let Some(pick) = self.pick(name, chain.bold, chain.italic) else { continue };
            let glyph = self.glyph(pick.face, ch);
            if glyph != 0 {
                return Some((pick, glyph));
            }
        }
        first.map(|pick| (pick, 0))
    }
}
