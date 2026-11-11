//! Brush sounds: a sound as you draw, behaving as Krita's Brush SFX plugin does: each brush the
//! sound its kind takes, louder the faster and harder you go. The app tells this how the stroke
//! moves, what the brush is called and what is picked in its rows, and plays what this hands back
//! (the plugin kit passes it all); everything about the sound itself is here ([`voice`]). It's a
//! plugin, not part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
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

static CONTROLS: kit::Controls<10> = kit::Controls::new([1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);

// a recording let go plays the sound's own again
kit::controls!(CONTROLS, |index, value| {
    if let Some(k) = index.checked_sub(RECORDINGS).filter(|k| *k < 6 && value < 0.5) {
        with(|s| {
            s.recordings[k] = None;
            s.fit(k);
        });
    }
});

kit::sound_prepare!(|rate| {
    with(|s| {
        s.rate = rate;
        s.voice.prepare(rate);
        for k in 0..6 {
            s.fit(k);
        }
    })
});

kit::stroke_sound!(|stroke, frames, rate| {
    with(|s| {
        let pressed = stroke.since_ms.is_some();
        let since = stroke.since_ms.unwrap_or(f32::MAX);
        let motion = Motion {
            speed: stroke.speed,
            pressure: stroke.pressure,
            pressed,
            moving: pressed && since <= STILL_MS,
            sprayed: pressed && since <= STILL_MS + SPRAY_GRACE_MS,
            hover: stroke.hover,
            height: stroke.height,
        };
        let kind = s.kind(stroke.erasing);
        s.voice.render(frames, rate, kind, motion, CONTROLS.get(VOLUME).clamp(0.0, 1.0))
    })
});

kit::brush!(|name, set| with(|s| s.brush = (name.to_owned(), set.to_owned())));

// the brush's own sound, its one row in the brush studio
kit::brush_control!(|index, value| {
    if index == 0 {
        with(|s| s.brush_own = value);
    }
});

// a recording picked for one of the six sounds
kit::files!(|index, file| {
    let Some(k) = index.checked_sub(RECORDINGS).filter(|k| *k < 6) else { return };
    with(|s| {
        s.recordings[k] = file.and_then(wav::read_wav).map(|(samples, rate)| (samples, rate as f32));
        s.fit(k);
    });
});

struct State {
    voice: Voice,
    /// The rate the sounds are made at, once the app has said.
    rate: f32,
    /// The current brush's name and its set's, and its own sound (its row in the brush studio: 0 as
    /// the controls say, 1 to 6 one of the six, 7 none).
    brush: (String, String),
    brush_own: f32,
    /// Each sound's recording as picked: its samples and their rate.
    recordings: [Option<(Vec<f32>, f32)>; 6],
}

impl State {
    fn new() -> State {
        State {
            voice: Voice::default(),
            rate: 0.0,
            brush: (String::new(), String::new()),
            brush_own: 0.0,
            recordings: Default::default(),
        }
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
            return CONTROLS.on(ERASER_ON).then(|| one_of(CONTROLS.get(ERASER_SOUND)).unwrap_or(Kind::Eraser));
        }
        one_of(CONTROLS.get(SOUND)).or_else(|| Some(voice::kind_of(&self.brush.0, &self.brush.1)))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests one at a time, each from a fresh state: they share the plugin's.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn fresh() -> std::sync::MutexGuard<'static, ()> {
        let turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        *STATE.lock().unwrap() = Some(State::new());
        for (i, start) in [1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0].into_iter().enumerate() {
            CONTROLS.told(i as i32, start);
        }
        turn
    }

    fn tell_brush(name: &str, set: &str) {
        with(|s| s.brush = (name.to_owned(), set.to_owned()));
    }

    fn kind(erasing: bool) -> Option<Kind> {
        with(|s| s.kind(erasing))
    }

    /// The samples the sound hook hands out for the stroke so (as the app would read them).
    fn heard(frames: i32, since_ms: f32) -> Vec<f32> {
        procraft_stroke_sound(frames, 48_000, 3.0, 1.0, since_ms, i32::from(since_ms >= 0.0), 0.0, 0.0, 1_000.0);
        kit::SOUND.with(|bytes| bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt()
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
        let _ = heard(4_800, 10.0);
        assert!(rms(&heard(4_800, 10.0)) > 0.005, "pressed: heard");
        let _ = heard(48_000, -1.0);
        assert!(rms(&heard(4_800, -1.0)) < 1e-4, "no stroke: quiet");
        // a recording of a 440 Hz tone for the airbrush: what is heard crosses zero 880 times a second
        let tone: Vec<f32> =
            (0..22_050).map(|i| 0.7 * (std::f32::consts::TAU * 440.0 * i as f32 / 44_100.0).sin()).collect();
        let file = wav::write_wav(&tone, 44_100);
        procraft_control((RECORDINGS + 4) as i32, 1.0);
        kit::FILE.with(|bytes| *bytes = file.clone());
        procraft_file((RECORDINGS + 4) as i32, file.len() as i32);
        assert_eq!(with(|s| s.voice.own_rate(Kind::Airbrush)), Some(48_000.0));
        let _ = heard(4_800, 10.0);
        let tone_heard = heard(48_000, 10.0);
        let crossings = tone_heard.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((800..960).contains(&crossings), "{crossings}");
        // let go: its own again
        procraft_control((RECORDINGS + 4) as i32, 0.0);
        assert_eq!(with(|s| s.voice.own_rate(Kind::Airbrush)), None);
    }
}
