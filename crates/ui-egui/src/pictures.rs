//! Pictures a message body shows from the message itself: its own attachments (`cid:`
//! references) and pictures written into the body (`data:` addresses).
//!
//! Only what the body refers to is decoded, only as pictures, and only in this process:
//! nothing is fetched, written to disk or handed to another program. A picture is hostile input
//! like the rest of the message, so its file size, its dimensions and the memory it may take are
//! capped before anything is decoded (`openmailclient_engine::pictures`); one that cannot be read
//! stays a labelled placeholder. Pictures are decoded on a thread of their own, never on the one
//! that draws the window.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError, channel};
use std::time::Duration;
use web_time::Instant;

use openmailclient_engine::remote::Downloaded;

use egui::{ColorImage, Context, TextureHandle, TextureOptions, Vec2, vec2};
use openmailclient_formats::rich::{Block, Doc, Picture};
use openmailclient_model::Item;

/// Pictures and decoded bytes kept for one message.
const MAX_PICTURES: usize = 64;
const MAX_KEPT_BYTES: usize = 128 << 20;
const MAX_DEPTH: usize = 64;

pub(crate) struct Shown {
    /// The picture, or the first frame of one that moves.
    pub texture: TextureHandle,
    /// A still picture at each half of its size, the largest first (`smaller`).
    pub smaller: Vec<TextureHandle>,
    /// Size in pixels as decoded, before any scaling to fit what is kept.
    pub size: Vec2,
    /// The frames of a picture that moves.
    pub film: Option<Reel>,
    /// For a picture that is seen through in places: how light what is seen of it is, from 0
    /// (black) to 1 (white). `None` for one that covers all of its room.
    pub ink: Option<f32>,
}

/// Pixels looked at to tell how light a picture is.
const MAX_LOOKED_AT: usize = 1 << 16;

/// How light the part of a picture that is seen is, when part of it is seen through: `None` for
/// a picture that covers its room, or of which nothing is seen at all. `rgba` is four bytes a
/// pixel, colour not multiplied by cover.
pub(crate) fn ink(rgba: &[u8]) -> Option<f32> {
    let pixels = rgba.len() / 4;
    let step = pixels.div_ceil(MAX_LOOKED_AT).max(1);
    let (mut looked, mut through, mut cover, mut light) = (0u64, 0u64, 0u64, 0u64);
    for pixel in rgba.chunks_exact(4).step_by(step) {
        let [r, g, b, a] = [pixel.first()?, pixel.get(1)?, pixel.get(2)?, pixel.get(3)?].map(|v| u64::from(*v));
        looked += 1;
        through += u64::from(a < 128);
        cover += a;
        // Rec. 601 weights, in thousandths.
        light += a * (299 * r + 587 * g + 114 * b) / 1000;
    }
    // A few soft pixels at the corners of a photograph do not make it a drawing on glass.
    if looked == 0 || cover == 0 || through * 20 < looked {
        return None;
    }
    Some((light as f32 / cover as f32 / 255.0).clamp(0.0, 1.0))
}

/// The frames of a picture that moves, the first among them.
pub(crate) struct Reel {
    /// Each frame, with the time from the start of one playing at which it ends, in
    /// thousandths of a second.
    frames: Vec<(TextureHandle, u64)>,
    /// How many times it is played; `None` is without end.
    loops: Option<u32>,
    /// When it was first shown, by the window's clock.
    since: f64,
}

impl Reel {
    #[cfg(test)]
    pub(crate) fn frames(&self) -> usize {
        self.frames.len()
    }

    /// Which frame is shown `passed` seconds after the first was, and for how long yet; no
    /// time when it has played as often as it asks and stands on its last frame.
    pub(crate) fn at(&self, passed: f64) -> (usize, Option<f64>) {
        let last = self.frames.len().saturating_sub(1);
        let whole = self.frames.last().map_or(0, |(_, ends)| *ends);
        if whole == 0 || !passed.is_finite() {
            return (if passed == f64::INFINITY && self.loops.is_some() { last } else { 0 }, None);
        }
        // Whole thousandths: the times of the frames are, and a sum of them is then exact.
        let passed = (passed.max(0.0) * 1000.0).round().min(1e18) as u64;
        if self.loops.is_some_and(|times| passed / whole >= u64::from(times)) {
            return (last, None);
        }
        let within = passed % whole;
        let frame = self.frames.iter().position(|(_, ends)| *ends > within).unwrap_or(last);
        (frame, self.frames.get(frame).map(|(_, ends)| (ends.saturating_sub(within).max(1)) as f64 / 1000.0))
    }
}

impl Shown {
    /// What is drawn now: the picture, or the frame of the moment when it moves and `moving`
    /// says such pictures are played. The window is asked to draw again when the next is due.
    /// A still one is drawn from the copy of it nearest the `drawn` size, in points.
    pub(crate) fn frame(&self, ctx: &Context, moving: bool, drawn: Vec2) -> egui::TextureId {
        let Some(reel) = self.film.as_ref().filter(|_| moving) else {
            return sharpest(&self.texture, &self.smaller, drawn * ctx.pixels_per_point());
        };
        let (frame, left) = reel.at(ctx.input(|i| i.time) - reel.since);
        if let Some(left) = left {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(left.min(60.0)));
        }
        reel.frames.get(frame).map_or(self.texture.id(), |(texture, _)| texture.id())
    }
}

/// How long drawing the window waits for pictures being decoded before it goes on without them:
/// a small picture is there in the first frame, a large one comes when it is ready.
const PATIENCE: Duration = Duration::from_millis(20);

/// Where a picture goes among those of the message.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Slot {
    Id(String),
    Written(usize),
    Address(String),
}

/// A picture to be decoded: where it goes, what it is called and its file.
struct Job {
    slot: Slot,
    name: String,
    data: Arc<[u8]>,
}

/// What decoding a picture came to, made into textures on the decoding thread.
struct Made {
    texture: TextureHandle,
    smaller: Vec<TextureHandle>,
    size: Vec2,
    /// The frames of one that moves, with how many times it is played.
    film: Option<(Vec<(TextureHandle, u64)>, Option<u32>)>,
    ink: Option<f32>,
    /// The memory its pixels take.
    bytes: usize,
}

type Done = (Slot, Option<Made>);

/// The decoded pictures of the message being read, by content id. Pictures are decoded on a
/// thread of their own, so that a large or slow one never holds the window; until one is ready
/// its place is drawn as an empty frame.
pub(crate) struct Pictures {
    key: u64,
    by_id: HashMap<String, Option<Shown>>,
    /// Pictures written into the body, by their place in the document's order.
    written: HashMap<usize, Option<Shown>>,
    /// Pictures on a server the reader had downloaded, by the address the message gave.
    by_address: HashMap<String, Option<Shown>>,
    /// Pictures being decoded, and what is heard from the threads decoding them.
    pending: HashSet<Slot>,
    batches: Vec<Receiver<Done>>,
    /// The session's downloads as they were when these were looked for.
    generation: u64,
    kept_bytes: usize,
    /// Whether pictures that move are played; else each stands on its first frame.
    pub moving: bool,
    /// How long `load` waits for pictures being decoded; `None` until all are.
    patience: Option<Duration>,
}

impl Default for Pictures {
    fn default() -> Self {
        Pictures {
            key: 0,
            by_id: HashMap::new(),
            written: HashMap::new(),
            by_address: HashMap::new(),
            pending: HashSet::new(),
            batches: Vec::new(),
            generation: 0,
            kept_bytes: 0,
            moving: false,
            patience: Some(PATIENCE),
        }
    }
}

impl Pictures {
    /// Pictures that `load` waits for until all are decoded, as a picture of the window taken
    /// without a screen wants them.
    #[cfg(test)]
    pub(crate) fn patient() -> Self {
        Pictures { patience: None, ..Pictures::default() }
    }

    /// Pictures that `load` never waits for.
    #[cfg(test)]
    pub(crate) fn hasty() -> Self {
        Pictures { patience: Some(Duration::ZERO), ..Pictures::default() }
    }

    pub(crate) fn get(&self, content_id: &str) -> Option<&Shown> {
        self.by_id.get(content_id)?.as_ref()
    }

    /// A picture on a server, once the reader had it downloaded.
    pub(crate) fn remote(&self, address: &str) -> Option<&Shown> {
        self.by_address.get(address)?.as_ref()
    }

    /// A picture written into the body, by its place in the document's list.
    pub(crate) fn written(&self, index: usize) -> Option<&Shown> {
        self.written.get(&index)?.as_ref()
    }

    /// Whether `load` waits until every picture is decoded, or only a moment.
    pub(crate) fn wait_for_all(&mut self, all: bool) {
        self.patience = if all { None } else { Some(PATIENCE) };
    }

    fn count(&self) -> usize {
        self.by_id.len() + self.written.len() + self.by_address.len() + self.pending.len()
    }

    fn known(&self, slot: &Slot) -> bool {
        self.pending.contains(slot)
            || match slot {
                Slot::Id(id) => self.by_id.contains_key(id),
                Slot::Written(n) => self.written.contains_key(n),
                Slot::Address(a) => self.by_address.contains_key(a),
            }
    }

    fn put(&mut self, slot: Slot, shown: Option<Shown>) {
        match slot {
            Slot::Id(id) => {
                self.by_id.insert(id, shown);
            }
            Slot::Written(n) => {
                self.written.insert(n, shown);
            }
            Slot::Address(a) => {
                self.by_address.insert(a, shown);
            }
        }
    }

    /// Decode the pictures `doc` refers to, once per message. `key` names the message's body.
    pub(crate) fn load(&mut self, ctx: &Context, key: u64, doc: &Doc, item: &Item, downloaded: &Downloaded) {
        if self.key != key {
            // What is still being decoded for the last message is let go: its threads stop
            // when they find no one listening.
            *self = Pictures { key, moving: self.moving, patience: self.patience, ..Pictures::default() };
        }
        self.take(ctx);
        let mut jobs = Vec::new();
        let room = |this: &Self, jobs: &Vec<Job>| this.count() + jobs.len() < MAX_PICTURES && this.kept_bytes < MAX_KEPT_BYTES;
        if self.generation != downloaded.generation {
            self.generation = downloaded.generation;
            self.by_address.retain(|_, shown| shown.is_some());
            for address in openmailclient_engine::remote::addresses(doc) {
                let slot = Slot::Address(address);
                if self.known(&slot) || jobs.iter().any(|j: &Job| j.slot == slot) {
                    continue;
                }
                let Slot::Address(address) = &slot else { continue };
                let Some(data) = downloaded.get(address) else { continue };
                if room(self, &jobs) {
                    jobs.push(Job { name: format!("remote-{}", self.count() + jobs.len()), data: Arc::from(&data[..]), slot });
                } else {
                    self.put(slot, None);
                }
            }
        }
        let mut wanted = Vec::new();
        content_ids(&doc.blocks, &mut wanted, 0);
        for id in wanted {
            let slot = Slot::Id(id.to_string());
            if self.known(&slot) || jobs.iter().any(|j| j.slot == slot) {
                continue;
            }
            if self.by_id.len() + jobs.len() >= MAX_PICTURES {
                break;
            }
            let attachment = openmailclient_engine::pictures::attached(item, id);
            match attachment.filter(|_| room(self, &jobs)) {
                Some(a) => jobs.push(Job { name: format!("picture-{}", self.count() + jobs.len()), data: Arc::from(a.data.as_slice()), slot }),
                None => self.put(slot, None),
            }
        }
        for (n, data) in doc.written.iter().enumerate() {
            let slot = Slot::Written(n);
            if self.known(&slot) {
                continue;
            }
            if room(self, &jobs) {
                jobs.push(Job { name: format!("written-{}", self.count() + jobs.len()), data: data.clone(), slot });
            } else {
                self.put(slot, None);
            }
        }
        if !jobs.is_empty() {
            self.begin(ctx, jobs);
        }
        self.wait(ctx);
    }

    /// Decode `jobs` on a thread of their own; where there are no threads (the web build), or
    /// one cannot be had, here.
    fn begin(&mut self, ctx: &Context, jobs: Vec<Job>) {
        let room = MAX_KEPT_BYTES.saturating_sub(self.kept_bytes);
        self.pending.extend(jobs.iter().map(|j| j.slot.clone()));
        let (tx, rx) = channel();
        self.batches.push(rx);
        let theirs = ctx.clone();
        let work = move || {
            for job in jobs {
                let made = make(&theirs, &job.name, &job.data, room);
                if tx.send((job.slot, made)).is_err() {
                    break;
                }
                theirs.request_repaint();
            }
        };
        if cfg!(target_arch = "wasm32") {
            work();
        } else {
            // Should no thread be had, the work goes with it, and what was waiting is drawn
            // as not read (`take`).
            let _ = std::thread::Builder::new().name("pictures".into()).spawn(work);
        }
    }

    /// Take in the pictures decoded since last asked. A thread that ended without telling of
    /// all it had leaves those as not read.
    fn take(&mut self, ctx: &Context) {
        let mut done = Vec::new();
        self.batches.retain(|rx| {
            loop {
                match rx.try_recv() {
                    Ok(d) => done.push(d),
                    Err(TryRecvError::Empty) => break true,
                    Err(TryRecvError::Disconnected) => break false,
                }
            }
        });
        for d in done {
            self.arrive(ctx, d);
        }
        if self.batches.is_empty() && !self.pending.is_empty() {
            for slot in std::mem::take(&mut self.pending) {
                self.put(slot, None);
            }
        }
    }

    fn arrive(&mut self, ctx: &Context, (slot, made): Done) {
        if !self.pending.remove(&slot) {
            return;
        }
        let shown = made.filter(|m| self.kept_bytes.saturating_add(m.bytes) <= MAX_KEPT_BYTES).map(|m| {
            self.kept_bytes = self.kept_bytes.saturating_add(m.bytes);
            let since = ctx.input(|i| i.time);
            Shown {
                texture: m.texture,
                smaller: m.smaller,
                size: m.size,
                film: m.film.map(|(frames, loops)| Reel { frames, loops, since }),
                ink: m.ink,
            }
        });
        self.put(slot, shown);
    }

    /// Wait for what is being decoded, as long as `patience` says.
    fn wait(&mut self, ctx: &Context) {
        if self.pending.is_empty() || cfg!(target_arch = "wasm32") {
            return;
        }
        let until = self.patience.map(|p| Instant::now() + p);
        while !self.pending.is_empty() {
            let Some(rx) = self.batches.first() else { break };
            let got = match until {
                Some(until) => {
                    let left = until.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    rx.recv_timeout(left).map_err(|e| e == RecvTimeoutError::Disconnected)
                }
                None => rx.recv().map_err(|_| true),
            };
            match got {
                Ok(d) => self.arrive(ctx, d),
                // That thread is done; on to the next, or what it left is not read.
                Err(true) => {
                    self.batches.remove(0);
                    self.take(ctx);
                }
                Err(false) => break,
            }
        }
    }
}

/// A picture file as textures: every frame of one that moves, while there is `room` to keep
/// them, else the picture standing still.
fn make(ctx: &Context, name: &str, data: &[u8], room: usize) -> Option<Made> {
    let film = openmailclient_engine::pictures::film(data)
        .filter(|film| film.frames.len().saturating_mul(film.frames.first().map_or(0, |f| f.rgba.len())) <= room);
    if let Some(film) = film {
        let kept = [usize::try_from(film.width).ok()?, usize::try_from(film.height).ok()?];
        let (mut ends, mut bytes) = (0u64, 0usize);
        let mut frames = Vec::with_capacity(film.frames.len());
        for (n, frame) in film.frames.iter().enumerate() {
            if frame.rgba.len() != kept[0].checked_mul(kept[1])?.checked_mul(4)? {
                return None;
            }
            bytes = bytes.saturating_add(frame.rgba.len());
            ends = ends.saturating_add(u64::from(frame.millis));
            let image = ColorImage::from_rgba_unmultiplied(kept, &frame.rgba);
            frames.push((ctx.load_texture(format!("{name}-{n}"), image, TextureOptions::LINEAR), ends));
        }
        let texture = frames.first()?.0.clone();
        let size = vec2(f32::from(film.natural.0), f32::from(film.natural.1));
        let ink = film.frames.first().and_then(|frame| ink(&frame.rgba));
        return Some(Made { texture, smaller: Vec::new(), size, film: Some((frames, film.loops)), ink, bytes });
    }
    let pixels = openmailclient_engine::pictures::decode(data)?;
    let ink = ink(&pixels.rgba);
    let (image, size) = image_of(&pixels)?;
    let (texture, smaller, bytes) = textures(ctx, name, image.size, &pixels.rgba)?;
    Some(Made { texture, smaller, size, film: None, ink, bytes })
}

/// Copies smaller than this on their longer side are not made.
const LEAST_COPY: usize = 16;

/// A still picture of `size` and unmultiplied `rgba` as a texture, with a copy of it at each half
/// of its size, the largest first, and the memory they all take. The renderer keeps no smaller
/// copies of its own and samples only the four texels nearest each pixel, so a picture drawn at
/// a third of its size would miss most of its pixels and look grainy; drawn from the copy
/// nearest its size it is shrunk by less than half, and every pixel counts.
pub(crate) fn textures(ctx: &Context, name: &str, size: [usize; 2], rgba: &[u8]) -> Option<(TextureHandle, Vec<TextureHandle>, usize)> {
    if size[0].checked_mul(size[1])?.checked_mul(4)? != rgba.len() {
        return None;
    }
    let texture = ctx.load_texture(name, ColorImage::from_rgba_unmultiplied(size, rgba), TextureOptions::LINEAR);
    let (mut smaller, mut bytes) = (Vec::new(), rgba.len());
    let mut at = (size, None::<Vec<u8>>);
    while at.0[0].max(at.0[1]) / 2 >= LEAST_COPY {
        let from = at.1.as_deref().unwrap_or(rgba);
        let Some((half, pixels)) = halved(at.0, from) else { break };
        bytes = bytes.saturating_add(pixels.len());
        smaller.push(ctx.load_texture(
            format!("{name}-{}", smaller.len() + 1),
            ColorImage::from_rgba_unmultiplied(half, &pixels),
            TextureOptions::LINEAR,
        ));
        at = (half, Some(pixels));
    }
    Some((texture, smaller, bytes))
}

/// The texture to draw a picture `px` physical pixels in size from: the smallest copy at least
/// that size, so that it is never shrunk by half or more as it is drawn.
pub(crate) fn sharpest(texture: &TextureHandle, smaller: &[TextureHandle], px: Vec2) -> egui::TextureId {
    smaller
        .iter()
        .rev()
        .find(|t| {
            let [w, h] = t.size();
            w as f32 >= px.x - 0.5 && h as f32 >= px.y - 0.5
        })
        .unwrap_or(texture)
        .id()
}

/// Light as it is, from the 8-bit sRGB value of a channel (IEC 61966-2-1).
fn linear(v: u8) -> f32 {
    static TABLE: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0.0; 256];
        for (n, slot) in t.iter_mut().enumerate() {
            let c = n as f32 / 255.0;
            *slot = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        }
        t
    });
    table.get(usize::from(v)).copied().unwrap_or(0.0)
}

/// The 8-bit sRGB value of light `l` (0 to 1).
fn gamma(l: f32) -> u8 {
    const STEPS: usize = 4096;
    static TABLE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        (0..=STEPS)
            .map(|n| {
                let l = n as f32 / STEPS as f32;
                let c = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
                (c * 255.0).round().clamp(0.0, 255.0) as u8
            })
            .collect()
    });
    let at = (l.clamp(0.0, 1.0) * STEPS as f32).round() as usize;
    table.get(at).copied().unwrap_or(255)
}

/// A picture of `size` and unmultiplied `rgba` at half its size, rounded up: each pixel the mean
/// of the two by two it stands for, in light as it is and weighed by how much each covers, so
/// that a dark edge of the see-through part does not bleed in.
pub(crate) fn halved(size: [usize; 2], rgba: &[u8]) -> Option<([usize; 2], Vec<u8>)> {
    let [w, h] = size;
    if w.checked_mul(h)?.checked_mul(4)? != rgba.len() || (w < 2 && h < 2) {
        return None;
    }
    let half = [w.div_ceil(2), h.div_ceil(2)];
    let mut out = Vec::with_capacity(half[0].checked_mul(half[1])?.checked_mul(4)?);
    for y in 0..half[1] {
        for x in 0..half[0] {
            let (mut sum, mut cover, mut n) = ([0.0f32; 3], 0.0f32, 0.0f32);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (sx, sy) = (2 * x + dx, 2 * y + dy);
                if sx >= w || sy >= h {
                    continue;
                }
                let at = (sy * w + sx) * 4;
                let Some(&[r, g, b, a]) = rgba.get(at..at + 4) else { continue };
                let a = f32::from(a) / 255.0;
                for (s, c) in sum.iter_mut().zip([r, g, b]) {
                    *s += linear(c) * a;
                }
                cover += a;
                n += 1.0;
            }
            for s in sum {
                out.push(if cover > 0.0 { gamma(s / cover) } else { 0 });
            }
            out.push(if n > 0.0 { (cover / n * 255.0).round().clamp(0.0, 255.0) as u8 } else { 0 });
        }
    }
    Some((half, out))
}

fn content_ids<'a>(blocks: &'a [Block], out: &mut Vec<&'a str>, depth: usize) {
    for block in blocks {
        match block {
            Block::Para(p) => out.extend(p.runs.iter().filter_map(|r| match &r.picture {
                Some(Picture::Attached(id)) => Some(id.as_str()),
                _ => None,
            })),
            Block::Table(t) if depth < MAX_DEPTH => {
                for cell in t.rows.iter().flatten() {
                    content_ids(&cell.blocks, out, depth + 1);
                }
            }
            _ => {}
        }
    }
}

/// A picture file → pixels and its size as written. `None` for anything that is not a picture
/// this build reads (PNG, JPEG, GIF, BMP, WebP, TIFF, SVG), is over the limits, or is damaged.
#[cfg(test)]
pub(crate) fn decode(data: &[u8]) -> Option<(ColorImage, Vec2)> {
    image_of(&openmailclient_engine::pictures::decode(data)?)
}

fn image_of(pixels: &openmailclient_engine::pictures::Pixels) -> Option<(ColorImage, Vec2)> {
    let kept = [usize::try_from(pixels.width).ok()?, usize::try_from(pixels.height).ok()?];
    let size = vec2(f32::from(pixels.natural.0), f32::from(pixels.natural.1));
    Some((ColorImage::from_rgba_unmultiplied(kept, &pixels.rgba), size))
}
