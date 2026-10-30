//! WAVE sound files (RIFF, as Microsoft and IBM's Multimedia Programming Interface and Data
//! Specifications 1.0 lay them out, and the WAVE_FORMAT_EXTENSIBLE header of Microsoft's
//! "Multiple channel audio data and WAVE files"), read into samples to be played: a reminder's
//! sound chosen from a file of the owner's.
//!
//! Integer PCM of 8, 16, 24 and 32 bits and 32- and 64-bit floats are read, up to eight
//! channels. A file is hostile like any other: every length is checked against what is there,
//! sizes are capped, and a sound is cut at [`MAX_SECONDS`].

/// The most of a file read.
pub const MAX_BYTES: usize = 32 << 20;
/// A reminder's sound is cut after this long.
pub const MAX_SECONDS: u32 = 30;
const MAX_CHANNELS: u16 = 8;
const RATES: std::ops::RangeInclusive<u32> = 4_000..=384_000;
/// Chunks looked at before the sound's own are given up on.
const MAX_CHUNKS: usize = 256;

const PCM: u16 = 1;
const FLOAT: u16 = 3;
const EXTENSIBLE: u16 = 0xFFFE;

/// A sound read from a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Wav {
    /// Samples a second.
    pub rate: u32,
    pub channels: u16,
    /// Each channel's sample in turn, from -1 to 1.
    pub samples: Vec<f32>,
}

impl Wav {
    /// How long it plays, in seconds.
    pub fn seconds(&self) -> f32 {
        let frames = self.samples.len() / usize::from(self.channels.max(1));
        frames as f32 / self.rate.max(1) as f32
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// The sound in `bytes`, or why it cannot be played.
pub fn read(bytes: &[u8]) -> Result<Wav, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("the sound file is larger than {} MB", MAX_BYTES >> 20));
    }
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err("not a WAVE sound file".into());
    }
    // The chunks: "fmt " says how the sound is kept, "data" holds it.
    let (mut at, mut format, mut data) = (12usize, None, None);
    for _ in 0..MAX_CHUNKS {
        let (Some(id), Some(len)) = (bytes.get(at..at.saturating_add(4)), u32_at(bytes, at.saturating_add(4))) else { break };
        let start = at.saturating_add(8);
        let len = usize::try_from(len).unwrap_or(usize::MAX);
        // A last chunk longer than the file holds what is there, as players take it.
        let end = start.saturating_add(len).min(bytes.len());
        let body = bytes.get(start..end).unwrap_or_default();
        match id {
            b"fmt " => format = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        if format.is_some() && data.is_some() {
            break;
        }
        // Chunks are padded to an even length.
        at = start.saturating_add(len).saturating_add(len & 1);
    }
    let format = format.ok_or("the sound file says nothing of how it is kept")?;
    let data = data.ok_or("the sound file holds no sound")?;
    let mut tag = u16_at(format, 0).ok_or("the sound file's format is cut short")?;
    let channels = u16_at(format, 2).unwrap_or(0);
    let rate = u32_at(format, 4).unwrap_or(0);
    let align = u16_at(format, 12).unwrap_or(0);
    let bits = u16_at(format, 14).unwrap_or(0);
    if tag == EXTENSIBLE {
        // The kind is the first two bytes of the SubFormat GUID, after the valid bits and the
        // channel mask.
        tag = u16_at(format, 24).ok_or("the sound file's format is cut short")?;
    }
    if !(1..=MAX_CHANNELS).contains(&channels) {
        return Err(format!("a sound of {channels} channels is not played"));
    }
    if !RATES.contains(&rate) {
        return Err(format!("a sound of {rate} samples a second is not played"));
    }
    let width = match (tag, bits) {
        (PCM, 8 | 16 | 24 | 32) | (FLOAT, 32 | 64) => usize::from(bits / 8),
        (PCM | FLOAT, _) => return Err(format!("a sound of {bits} bits a sample is not played")),
        _ => return Err("only sounds kept as PCM or floating point are played".into()),
    };
    let frame = width.saturating_mul(usize::from(channels));
    // A block may be padded past its samples; never less than they take.
    let block = usize::from(align).max(frame);
    let most = usize::try_from(rate.saturating_mul(MAX_SECONDS)).unwrap_or(usize::MAX);
    let frames = (data.len() / block).min(most);
    let mut samples = Vec::with_capacity(frames.saturating_mul(usize::from(channels)));
    for f in 0..frames {
        let base = f.saturating_mul(block);
        for c in 0..usize::from(channels) {
            let at = base.saturating_add(c.saturating_mul(width));
            let Some(s) = data.get(at..at.saturating_add(width)) else { break };
            samples.push(sample(tag, s));
        }
    }
    if samples.is_empty() {
        return Err("the sound file holds no sound".into());
    }
    Ok(Wav { rate, channels, samples })
}

/// One sample, from -1 to 1.
fn sample(tag: u16, s: &[u8]) -> f32 {
    let v = match (tag, s) {
        // 8-bit samples are unsigned, the rest signed.
        (PCM, [a]) => (f32::from(*a) - 128.0) / 128.0,
        (PCM, [a, b]) => f32::from(i16::from_le_bytes([*a, *b])) / 32_768.0,
        (PCM, [a, b, c]) => (i32::from_le_bytes([0, *a, *b, *c]) >> 8) as f32 / 8_388_608.0,
        (PCM, [a, b, c, d]) => i32::from_le_bytes([*a, *b, *c, *d]) as f32 / 2_147_483_648.0,
        (FLOAT, [a, b, c, d]) => f32::from_le_bytes([*a, *b, *c, *d]),
        (FLOAT, [a, b, c, d, e, f, g, h]) => f64::from_le_bytes([*a, *b, *c, *d, *e, *f, *g, *h]) as f32,
        _ => 0.0,
    };
    // A float that is not a number, or louder than full, is not passed on to the speaker.
    if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 }
}

/// 16-bit PCM in a RIFF WAVE file: what is handed to a system's own player.
pub fn write(w: &Wav) -> Vec<u8> {
    let channels = w.channels.clamp(1, MAX_CHANNELS);
    let data = u32::try_from(w.samples.len().saturating_mul(2)).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(w.samples.len().saturating_mul(2).saturating_add(44));
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&data.saturating_add(36).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&PCM.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&w.rate.to_le_bytes());
    out.extend_from_slice(&w.rate.saturating_mul(u32::from(channels) * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in &w.samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut f = b"RIFF\0\0\0\0WAVE".to_vec();
        f.extend_from_slice(b"LIST\x03\0\0\0abc\0"); // an odd chunk of our own first, padded
        f.extend_from_slice(b"fmt \x10\0\0\0");
        f.extend_from_slice(&tag.to_le_bytes());
        f.extend_from_slice(&channels.to_le_bytes());
        f.extend_from_slice(&rate.to_le_bytes());
        let align = channels * bits / 8;
        f.extend_from_slice(&(rate * u32::from(align)).to_le_bytes());
        f.extend_from_slice(&align.to_le_bytes());
        f.extend_from_slice(&bits.to_le_bytes());
        f.extend_from_slice(b"data");
        f.extend_from_slice(&(data.len() as u32).to_le_bytes());
        f.extend_from_slice(data);
        f
    }

    #[test]
    fn every_kind_of_sample_is_read() {
        let w = read(&file(PCM, 1, 8_000, 8, &[0, 128, 255])).unwrap();
        assert_eq!(w.samples, vec![-1.0, 0.0, 127.0 / 128.0]);
        let w = read(&file(PCM, 2, 44_100, 16, &[0x00, 0x80, 0xFF, 0x7F])).unwrap();
        assert_eq!((w.channels, w.samples.len()), (2, 2));
        assert_eq!(w.samples[0], -1.0);
        let w = read(&file(PCM, 1, 48_000, 24, &[0, 0, 0x80, 0, 0, 0x40])).unwrap();
        assert_eq!(w.samples, vec![-1.0, 0.5]);
        let w = read(&file(PCM, 1, 48_000, 32, &i32::MIN.to_le_bytes())).unwrap();
        assert_eq!(w.samples, vec![-1.0]);
        let mut floats = 0.25f32.to_le_bytes().to_vec();
        floats.extend_from_slice(&f32::NAN.to_le_bytes());
        floats.extend_from_slice(&9.0f32.to_le_bytes());
        let w = read(&file(FLOAT, 1, 48_000, 32, &floats)).unwrap();
        assert_eq!(w.samples, vec![0.25, 0.0, 1.0], "not a number is quiet, too loud is full");
        let w = read(&file(FLOAT, 1, 48_000, 64, &(-0.5f64).to_le_bytes())).unwrap();
        assert_eq!(w.samples, vec![-0.5]);
        // What we write reads back.
        let back = read(&write(&Wav { rate: 22_050, channels: 2, samples: vec![0.5, -0.5, 0.0, 1.0] })).unwrap();
        assert_eq!((back.rate, back.channels, back.samples.len()), (22_050, 2, 4));
        assert!((back.samples[0] - 0.5).abs() < 1e-3);
        assert!((w.seconds() - 1.0 / 48_000.0).abs() < 1e-6);
    }

    #[test]
    fn the_extensible_header_names_its_kind() {
        let mut f = b"RIFF\0\0\0\0WAVEfmt \x28\0\0\0".to_vec();
        f.extend_from_slice(&EXTENSIBLE.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&16_000u32.to_le_bytes());
        f.extend_from_slice(&32_000u32.to_le_bytes());
        f.extend_from_slice(&2u16.to_le_bytes());
        f.extend_from_slice(&16u16.to_le_bytes());
        f.extend_from_slice(&22u16.to_le_bytes());
        f.extend_from_slice(&16u16.to_le_bytes());
        f.extend_from_slice(&4u32.to_le_bytes());
        f.extend_from_slice(&PCM.to_le_bytes());
        f.extend_from_slice(&[0; 14]);
        f.extend_from_slice(b"data\x02\0\0\0\x00\x40");
        assert_eq!(read(&f).unwrap().samples, vec![0.5]);
    }

    #[test]
    fn hostile_sound_files_are_refused_or_bounded() {
        for bad in [
            Vec::new(),
            b"RIFF".to_vec(),
            b"RIFF\0\0\0\0AVI LIST".to_vec(),
            b"RIFF\0\0\0\0WAVE".to_vec(),
            file(PCM, 0, 8_000, 16, &[0, 0]),
            file(PCM, 9, 8_000, 16, &[0; 18]),
            file(PCM, 1, 1, 16, &[0, 0]),
            file(PCM, 1, 8_000, 12, &[0, 0]),
            file(2, 1, 8_000, 4, &[0, 0]), // ADPCM
            file(PCM, 1, 8_000, 16, &[]),
            file(PCM, 2, 8_000, 16, &[0, 0]), // less than one frame
        ] {
            assert!(read(&bad).is_err(), "{:?}", bad.get(..bad.len().min(40)));
        }
        // A chunk that says it is longer than the file: what is there is read.
        let mut f = file(PCM, 1, 8_000, 16, &[0, 0x40, 0, 0x40]);
        let n = f.len();
        f[n - 8..n - 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(read(&f).unwrap().samples.len(), 2);
        // An endless run of chunks of no length ends.
        let mut f = b"RIFF\0\0\0\0WAVE".to_vec();
        for _ in 0..1_000 {
            f.extend_from_slice(b"junk\0\0\0\0");
        }
        assert!(read(&f).is_err());
        // A sound longer than it may be is cut.
        let long = vec![0u8; 8_000 * (MAX_SECONDS as usize + 5)];
        let w = read(&file(PCM, 1, 8_000, 8, &long)).unwrap();
        assert_eq!(w.samples.len(), 8_000 * MAX_SECONDS as usize);
        assert!(read(&vec![0u8; MAX_BYTES + 1]).is_err());
        // Truncated anywhere, it never panics.
        let good = file(PCM, 2, 44_100, 24, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        for cut in 0..good.len() {
            let _ = read(&good[..cut]);
        }
    }
}
