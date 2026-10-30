//! The sound new mail and reminders are told of with: two soft bell notes made here in code,
//! our own, so no sound file is shipped; or, for a reminder, a WAVE file the owner chose, read
//! on the playing thread and the chime played instead if it cannot be. Played on a thread of
//! its own, one at a time; a system with no sound device, or none to play it with, stays quiet.

use openmailclient_formats::wav::{self, Wav};
use openmailclient_ui_egui::Sound;
use std::sync::atomic::{AtomicBool, Ordering};

const RATE: u32 = 44_100;
/// Seconds the chime lasts.
const LENGTH: f32 = 0.75;
/// Each note: when it starts (seconds), its pitch (Hz) and how loud it is.
const NOTES: [(f32, f32, f32); 2] = [(0.0, 880.0, 0.22), (0.12, 1_318.5, 0.18)];

static PLAYING: AtomicBool = AtomicBool::new(false);

/// The chime's samples, mono, from -1 to 1.
fn samples() -> Vec<f32> {
    let count = (RATE as f32 * LENGTH) as usize;
    (0..count)
        .map(|n| {
            let t = n as f32 / RATE as f32;
            let mut v = 0.0;
            for (start, pitch, loud) in NOTES {
                let at = t - start;
                if at < 0.0 {
                    continue;
                }
                // A quick rise so it does not click, then a bell's fall; a quieter overtone
                // above the note gives it a bell's colour.
                let rise = (at / 0.006).min(1.0);
                let fall = (-at / 0.2).exp();
                let phase = std::f32::consts::TAU * pitch * at;
                v += loud * rise * fall * (phase.sin() + 0.3 * (2.76 * phase).sin() * (-at / 0.08).exp());
            }
            // The last moments fade to nothing.
            let tail = ((LENGTH - t) / 0.05).clamp(0.0, 1.0);
            (v * tail).clamp(-1.0, 1.0)
        })
        .collect()
}

fn chime() -> Wav {
    Wav { rate: RATE, channels: 1, samples: samples() }
}

/// The sound a file of the owner's holds, read as `reminders.options` made sure of it.
fn of_file(path: &str) -> Result<Wav, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > wav::MAX_BYTES as u64 {
        return Err("it is too large".into());
    }
    wav::read(&std::fs::read(path).map_err(|e| e.to_string())?)
}

/// Play the chime unless a sound is playing already.
pub fn play() {
    play_sound(Sound::Chime);
}

/// Play `sound` unless one is playing already.
pub fn play_sound(sound: Sound) {
    if PLAYING.swap(true, Ordering::AcqRel) {
        return;
    }
    let begun = std::thread::Builder::new().name("chime".into()).spawn(move || {
        let wav = match &sound {
            Sound::Chime => chime(),
            Sound::File(path) => of_file(path).unwrap_or_else(|e| {
                log::info!("the reminder sound {path} could not be played, the chime is: {e}");
                chime()
            }),
        };
        if let Err(e) = out(wav) {
            log::info!("the sound was not played: {e}");
        }
        PLAYING.store(false, Ordering::Release);
    });
    if begun.is_err() {
        PLAYING.store(false, Ordering::Release);
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn out(wav: Wav) -> Result<(), String> {
    let (_stream, handle) = rodio::OutputStream::try_default().map_err(|e| e.to_string())?;
    let sink = rodio::Sink::try_new(&handle).map_err(|e| e.to_string())?;
    sink.append(rodio::buffer::SamplesBuffer::new(wav.channels, wav.rate, wav.samples));
    sink.sleep_until_end();
    Ok(())
}

/// Elsewhere the sound is written as a WAV file of our own, as 16-bit PCM every player takes,
/// and handed to the sound server's player, or ALSA's.
#[cfg(not(any(windows, target_os = "macos")))]
fn out(sound: Wav) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!("openmailclient-chime-{}.wav", std::process::id()));
    std::fs::write(&path, wav::write(&sound)).map_err(|e| e.to_string())?;
    let played = ["paplay", "pw-play", "aplay"].iter().any(|player| {
        let mut command = std::process::Command::new(player);
        if *player == "aplay" {
            command.arg("-q");
        }
        command
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    });
    let _ = std::fs::remove_file(&path);
    if played { Ok(()) } else { Err("no sound player was found".into()) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_chime_is_short_quiet_and_ends_in_silence() {
        let s = super::samples();
        assert_eq!(s.len(), (super::RATE as f32 * super::LENGTH) as usize);
        let loudest = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(loudest > 0.1 && loudest < 0.6, "{loudest}");
        assert!(s.last().is_some_and(|v| v.abs() < 1e-3));
        let w = super::wav::write(&super::chime());
        assert_eq!(w.len(), 44 + s.len() * 2);
        assert_eq!(super::wav::read(&w).unwrap().samples.len(), s.len());
        // A file that cannot be played is told of, for the chime to be played instead.
        assert!(super::of_file("no-such-sound.wav").is_err());
    }
}
