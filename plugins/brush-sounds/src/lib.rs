//! Brush sounds: a sound as you draw, behaving as Krita's Brush SFX plugin does: each brush the
//! sound its kind takes, louder the faster and harder you go. The app tells this how the stroke
//! moves, what the brush is called and what is picked in its rows, and plays what this hands back;
//! everything about the sound itself is here ([`voice`]). It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The interface is written down at the top of `crates/core/src/plugin_run.rs`.

mod voice;
mod wav;

use std::sync::Mutex;

use voice::{KINDS, Kind, Motion, SPRAY_GRACE_MS, STILL_MS, Voice};

/// The controls, by their place in the manifest: the volume (0 to 1), the sound for every brush (0
/// each brush's kind's, 1 to 6 one of the six), whether erasing makes a sound (0 or 1), the sound
/// erasing makes (1 to 6), then for each of the six sounds whether a recording is picked (0 or 1).
const VOLUME: usize = 0;
const SOUND: usize = 1;
const ERASER_ON: usize = 2;
const ERASER_SOUND: usize = 3;
const RECORDINGS: usize = 4;
const CONTROLS: usize = RECORDINGS + 6;

/// What the app says of the stroke, as bits: a stroke is open, the pen hovers, the tool erases.
const PRESSED: i32 = 1;
const HOVERING: i32 = 2;
const ERASING: i32 = 4;

struct State {
    controls: [f32; CONTROLS],
    voice: Voice,
    /// The rate the sounds are made at, once the app has said.
    rate: f32,
    /// The current brush's name and its set's, and its own sound (its row in the brush studio: 0 as
    /// the controls say, 1 to 6 one of the six, 7 none).
    brush: (String, String),
    brush_own: f32,
    /// Each sound's recording as picked: its samples and their rate.
    recordings: [Option<(Vec<f32>, f32)>; 6],
    /// Where text and files come in, and where the samples go out.
    text: Vec<u8>,
    file: Vec<u8>,
    out: Vec<f32>,
}

impl State {
    fn new() -> State {
        let mut controls = [0.0; CONTROLS];
        controls[VOLUME] = 1.0;
        controls[ERASER_ON] = 1.0;
        controls[ERASER_SOUND] = 1.0;
        State {
            controls,
            voice: Voice::default(),
            rate: 0.0,
            brush: (String::new(), String::new()),
            brush_own: 0.0,
            recordings: Default::default(),
            text: Vec::new(),
            file: Vec::new(),
            out: Vec::new(),
        }
    }

    fn control(&self, i: usize) -> f32 {
        self.controls.get(i).copied().unwrap_or(0.0)
    }

    /// The sound the stroke makes now: the brush's own if it has one (none for no sound); else
    /// erasing, its sound if erasing is heard; else the one for every brush; else the brush's
    /// kind's, by its name or its set's.
    fn kind(&self, erasing: bool) -> Option<Kind> {
        match self.brush_own.round() as usize {
            0 => {}
            own => return KINDS.get(own - 1).copied(),
        }
        if erasing {
            return (self.control(ERASER_ON) >= 0.5).then(|| one_of(self.control(ERASER_SOUND)).unwrap_or(Kind::Eraser));
        }
        one_of(self.control(SOUND)).or_else(|| Some(voice::kind_of(&self.brush.0, &self.brush.1)))
    }

    /// Recording `k`'s sound plays it, fitted to the rate the sounds are made at (none: its own).
    fn fit(&mut self, k: usize) {
        let Some(&kind) = KINDS.get(k) else { return };
        let fitted = match self.recordings.get(k).and_then(Option::as_ref) {
            Some((samples, from)) if self.rate > 0.0 => {
                Some((self.rate, voice::recorded_loop(kind, self.rate, samples, *from)))
            }
            _ => None,
        };
        self.voice.set_own(kind, fitted);
    }
}

/// One of the six sounds, by its number (from 1); none for 0.
fn one_of(n: f32) -> Option<Kind> {
    (n.round().max(0.0) as usize).checked_sub(1).and_then(|i| KINDS.get(i)).copied()
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(State::new))
}

/// A control's value, by its place in the manifest. A recording let go plays the sound's own.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_control(index: i32, value: f32) {
    let Ok(i) = usize::try_from(index) else { return };
    if !value.is_finite() {
        return;
    }
    with(|s| {
        if let Some(slot) = s.controls.get_mut(i) {
            *slot = value;
        }
        if let Some(k) = i.checked_sub(RECORDINGS).filter(|k| *k < 6 && value < 0.5) {
            if let Some(slot) = s.recordings.get_mut(k) {
                *slot = None;
            }
            s.fit(k);
        }
    });
}

/// The sounds are to be made at `rate` a second: each one's loop made now, and the recordings
/// fitted to it. Not 0 when done.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_sound_prepare(rate: i32) -> i32 {
    with(|s| {
        s.rate = rate.max(1) as f32;
        s.voice.prepare(s.rate);
        for k in 0..6 {
            s.fit(k);
        }
    });
    1
}

/// The next `frames` samples at `rate` a second, for the stroke moving at `speed` (window heights a
/// second) with `pressure`, its last input `since_ms` ago (below 0: none), as `state` says (bits:
/// [`PRESSED`], [`HOVERING`], [`ERASING`]), the pen hovering at (`hover_x`, `hover_y`) in a window
/// `height` tall: where they are (32-bit floats, -1 to 1).
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn procraft_stroke_sound(
    frames: i32,
    rate: i32,
    speed: f32,
    pressure: f32,
    since_ms: f32,
    state: i32,
    hover_x: f32,
    hover_y: f32,
    height: f32,
) -> i32 {
    with(|s| {
        let pressed = state & PRESSED != 0 && since_ms >= 0.0;
        let motion = Motion {
            speed: if speed.is_finite() { speed.max(0.0) } else { 0.0 },
            pressure: if pressure.is_finite() { pressure.clamp(0.0, 1.0) } else { 1.0 },
            pressed,
            moving: pressed && since_ms <= STILL_MS,
            sprayed: pressed && since_ms <= STILL_MS + SPRAY_GRACE_MS,
            hover: (state & HOVERING != 0).then_some((hover_x, hover_y)),
            height: height.max(1.0),
        };
        let kind = s.kind(state & ERASING != 0);
        let volume = s.control(VOLUME).clamp(0.0, 1.0);
        s.out = s.voice.render(frames.clamp(0, 48_000) as usize, rate.max(1) as f32, kind, motion, volume);
        s.out.as_ptr() as i32
    })
}

/// Where the current brush's name (and its set's after it) goes, `len` bytes.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_text_buffer(len: i32) -> i32 {
    with(|s| {
        s.text = vec![0; len.max(0) as usize];
        s.text.as_mut_ptr() as i32
    })
}

/// The current brush is the one named in the text: its name's `name_len` bytes, then its set's
/// `set_len` (UTF-8).
#[unsafe(no_mangle)]
pub extern "C" fn procraft_brush(name_len: i32, set_len: i32) {
    with(|s| {
        let (name_len, set_len) = (name_len.max(0) as usize, set_len.max(0) as usize);
        let name = s.text.get(..name_len).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
        let set = s.text.get(name_len..name_len + set_len).map(|b| String::from_utf8_lossy(b).into_owned());
        s.brush = (name, set.unwrap_or_default());
    });
}

/// The current brush's row `index` (in the brush studio, by its place in the manifest) is at
/// `value`: the brush's own sound.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_brush_control(index: i32, value: f32) {
    if index == 0 && value.is_finite() {
        with(|s| s.brush_own = value);
    }
}

/// Where the file picked for control `index` goes, `len` bytes.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_file_buffer(_index: i32, len: i32) -> i32 {
    with(|s| {
        s.file = vec![0; len.max(0) as usize];
        s.file.as_mut_ptr() as i32
    })
}

/// The file picked for control `index` is in, `len` bytes of it (0: none is picked): a recording,
/// played in place of its sound's own.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_file(index: i32, len: i32) {
    let Some(k) = usize::try_from(index).ok().and_then(|i| i.checked_sub(RECORDINGS)).filter(|k| *k < 6) else {
        return;
    };
    with(|s| {
        let read = s.file.get(..len.max(0) as usize).and_then(wav::read_wav);
        if let Some(slot) = s.recordings.get_mut(k) {
            *slot = read.map(|(samples, rate)| (samples, rate as f32));
        }
        s.file = Vec::new();
        s.fit(k);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests one at a time, each from a fresh state: they share the plugin's.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn fresh() -> std::sync::MutexGuard<'static, ()> {
        let turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        *STATE.lock().unwrap() = Some(State::new());
        turn
    }

    /// The text written as the app writes it, through the job's own buffer.
    fn tell_brush(name: &str, set: &str) {
        procraft_text_buffer((name.len() + set.len()) as i32);
        with(|s| {
            s.text[..name.len()].copy_from_slice(name.as_bytes());
            s.text[name.len()..].copy_from_slice(set.as_bytes());
        });
        procraft_brush(name.len() as i32, set.len() as i32);
    }

    fn kind(erasing: bool) -> Option<Kind> {
        with(|s| s.kind(erasing))
    }

    #[test]
    fn the_sound_goes_by_the_brush_the_controls_and_the_brush_s_own_row() {
        let _turn = fresh();
        tell_brush("Studio Pen", "Inking");
        assert_eq!(kind(false), Some(Kind::Pen));
        tell_brush("Mystery", "Airbrushing");
        assert_eq!(kind(false), Some(Kind::Airbrush));
        // one for every brush
        procraft_control(SOUND as i32, 4.0);
        assert_eq!(kind(false), Some(Kind::Paintbrush));
        // erasing: the eraser's, another, or none
        assert_eq!(kind(true), Some(Kind::Eraser));
        procraft_control(ERASER_SOUND as i32, 2.0);
        assert_eq!(kind(true), Some(Kind::Pencil));
        procraft_control(ERASER_ON as i32, 0.0);
        assert_eq!(kind(true), None);
        // the brush's own over all of it, or none
        procraft_brush_control(0, 6.0);
        assert_eq!((kind(false), kind(true)), (Some(Kind::SprayCan), Some(Kind::SprayCan)));
        procraft_brush_control(0, 7.0);
        assert_eq!(kind(false), None);
    }

    #[test]
    fn a_stroke_is_heard_and_a_recording_plays_in_place_of_its_sound() {
        let _turn = fresh();
        tell_brush("Soft Airbrush", "Airbrushing");
        assert_eq!(procraft_sound_prepare(48_000), 1);
        let loud = |since_ms: f32| {
            procraft_stroke_sound(4_800, 48_000, 3.0, 1.0, since_ms, PRESSED, 0.0, 0.0, 1_000.0);
            let out = with(|s| s.out.clone());
            (out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32).sqrt()
        };
        let _ = loud(10.0);
        assert!(loud(10.0) > 0.005, "pressed: heard");
        procraft_stroke_sound(48_000, 48_000, 0.0, 1.0, -1.0, 0, 0.0, 0.0, 1_000.0);
        assert!(loud(-1.0) < 1e-4, "no stroke: quiet");
        // a recording of a 440 Hz tone for the airbrush: what is heard crosses zero 880 times a second
        let tone: Vec<f32> = (0..22_050).map(|i| 0.7 * (std::f32::consts::TAU * 440.0 * i as f32 / 44_100.0).sin()).collect();
        let file = wav::write_wav(&tone, 44_100);
        procraft_control((RECORDINGS + 4) as i32, 1.0);
        procraft_file_buffer((RECORDINGS + 4) as i32, file.len() as i32);
        with(|s| s.file.copy_from_slice(&file));
        procraft_file((RECORDINGS + 4) as i32, file.len() as i32);
        assert_eq!(with(|s| s.voice.own_rate(Kind::Airbrush)), Some(48_000.0));
        let _ = loud(10.0);
        procraft_stroke_sound(48_000, 48_000, 3.0, 1.0, 10.0, PRESSED, 0.0, 0.0, 1_000.0);
        let heard = with(|s| s.out.clone());
        let crossings = heard.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((800..960).contains(&crossings), "{crossings}");
        // let go: its own again
        procraft_control((RECORDINGS + 4) as i32, 0.0);
        assert_eq!(with(|s| s.voice.own_rate(Kind::Airbrush)), None);
    }
}
