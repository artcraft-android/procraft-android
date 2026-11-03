//! Feedback Sounds' click, as the Android app plays the system's key click: a few milliseconds of
//! our own, played by the system (PlaySound on Windows, from memory; elsewhere the system's own
//! player of sounds, from a file: a Mac's afplay, Linux's PipeWire, PulseAudio or ALSA player).

/// The click's sample rate, how long it lasts and how fast it dies away (a second's worth).
const RATE: u32 = 44_100;
const LENGTH_MS: u32 = 14;
const DECAY: f32 = 700.0;

/// The click as a WAV file: a short burst of noise over a falling tone, dying away fast.
fn click_wav() -> Vec<u8> {
    let n = (RATE * LENGTH_MS / 1000) as usize;
    let mut noise = 0x2545_f491_u32;
    let samples = (0..n).map(|i| {
        let t = i as f32 / RATE as f32;
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        let hiss = noise as f32 / u32::MAX as f32 * 2.0 - 1.0;
        let tone = (t * std::f32::consts::TAU * (3_000.0 - 60_000.0 * t)).sin();
        let s = (0.55 * hiss + 0.45 * tone) * (-t * DECAY).exp() * 0.3;
        (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
    });
    let data: Vec<u8> = samples.flat_map(i16::to_le_bytes).collect();
    let data_len = data.len() as u32;
    let mut wav = Vec::with_capacity(44 + data.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    // PCM, mono, 16 bits a sample
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&data);
    wav
}

/// PlaySound: the click is made once and kept, as the system reads it while it plays.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn click() {
    use std::sync::OnceLock;

    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    use windows::core::PCWSTR;

    static WAV: OnceLock<Vec<u8>> = OnceLock::new();
    let wav = WAV.get_or_init(click_wav);
    // with SND_MEMORY the "name" is the WAV file in memory; a click still playing is cut short
    let _ = unsafe { PlaySoundW(PCWSTR(wav.as_ptr().cast()), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT) };
}

/// The players of sounds tried in turn (PipeWire's, PulseAudio's, ALSA's), with what quiets each.
#[cfg(not(any(windows, target_os = "macos")))]
const PLAYERS: [(&str, &[&str]); 3] = [("pw-play", &[]), ("paplay", &[]), ("aplay", &["-q"])];

/// A Mac's own player of sounds, by its whole path (an app opened from the Finder isn't given a
/// shell's search path).
#[cfg(target_os = "macos")]
const PLAYERS: [(&str, &[&str]); 1] = [("/usr/bin/afplay", &[])];

/// The first of [`PLAYERS`] there is, handed the click written once to a file (in the user's own
/// runtime folder where there is one); with none of them the click is silent.
#[cfg(not(windows))]
pub fn click() {
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The player last found to be there, its place in [`PLAYERS`]: past the end, none is.
    static PLAYER: AtomicUsize = AtomicUsize::new(0);
    static FILE: OnceLock<Option<PathBuf>> = OnceLock::new();
    let file = FILE.get_or_init(|| {
        let folder = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
        let path = folder.join("procraft-click.wav");
        std::fs::write(&path, click_wav()).ok().map(|()| path)
    });
    let Some(file) = file else { return };
    let mut at = PLAYER.load(Ordering::Relaxed);
    while let Some((player, quiet)) = PLAYERS.get(at) {
        let started = Command::new(player)
            .args(*quiet)
            .arg(file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut playing) = started {
            // waited for away from the window, so it doesn't linger once done
            std::thread::spawn(move || playing.wait());
            return;
        }
        at += 1;
        PLAYER.store(at, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_click_is_a_whole_wav_file() {
        let wav = super::click_wav();
        assert_eq!(wav.get(..4), Some(&b"RIFF"[..]));
        assert_eq!(wav.get(8..16), Some(&b"WAVEfmt "[..]));
        let samples = (super::RATE * super::LENGTH_MS / 1000) as usize;
        assert_eq!(wav.len(), 44 + samples * 2);
    }
}
