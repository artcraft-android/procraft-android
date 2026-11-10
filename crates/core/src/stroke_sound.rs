//! Brush sounds (a plugin's): the sound of the stroke being drawn, made here as it goes, behaving
//! as Krita's Brush SFX plugin does (davi133/brush_sfx, which it follows; nothing of its code or
//! recordings is in here). There are six sounds, an eraser, a pencil, a pen, a paintbrush, an
//! airbrush and a spray can; each brush has the one its kind takes, the eraser tool its own.
//!
//! Each sound is noise with its own shape over the frequencies, made once (and looped): bands of
//! it raised or lowered, smoothly at their edges. As the stroke goes the sound gets louder with its
//! speed, measured in window heights a second against each sound's top speed, and with its
//! pressure, from each sound's floor up; the pen's and the paintbrush's ring rises with speed,
//! the pencil gains body with pressure. The airbrush hisses while pressed, still or not; the spray
//! can while it moves, and its ball rattles when the pen is shaken back and forth above the canvas
//! (the rattle our own, made of clicks). It is made a block of about 20 ms at a time: a window of two blocks of the
//! loop, made as loud as the stroke says and filtered in the frequencies as it moves, its first
//! half eased in over the last window's second half. The apps only play what this makes.

use std::collections::VecDeque;
use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use crate::{CanvasSession, ToolKind};

/// Quiet after this long with no new input: a pen held still sends none.
const STILL_MS: u128 = 45;
/// The spray can goes on this long after the stroke stops moving.
const SPRAY_GRACE_MS: u128 = 60;
/// A hover older than this is no longer heard (the pen has gone).
const HOVER_STALE_MS: u128 = 100;
/// How fast the can's shake dies away, a second.
const SHAKE_FADE: f32 = 2.0;
/// How far back the speed is measured over, milliseconds.
const SPEED_SPAN_MS: f64 = 20.0;
/// A block: what is made at once, the stroke read once for it (about 21 ms at 48 kHz).
const BLOCK: usize = 1 << 10;
/// The samples of each sound's loop (a power of two, about 1.4 s at 48 kHz).
const LOOP: usize = 1 << 16;
/// How loud the pencil's loop is made (the others are as their bands make them), as a root mean
/// square.
const PENCIL_RMS: f32 = 0.3;

/// The six sounds, in the order a plugin numbers them (from 1; 0 is the brush's own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Eraser,
    Pencil,
    Pen,
    Paintbrush,
    Airbrush,
    SprayCan,
}

const KINDS: [Kind; 6] = [Kind::Eraser, Kind::Pencil, Kind::Pen, Kind::Paintbrush, Kind::Airbrush, Kind::SprayCan];

/// A band of the frequencies raised (`gain` above 0) or lowered (down to -1, none of it): in full
/// from `inner_low` to `inner_high`, eased in from `low` and out to `high` (Hz).
#[derive(Clone, Copy)]
struct Band {
    low: f32,
    inner_low: f32,
    inner_high: f32,
    high: f32,
    gain: f32,
}

const fn band(low: f32, inner_low: f32, inner_high: f32, high: f32, gain: f32) -> Band {
    Band { low, inner_low, inner_high, high, gain }
}

impl Band {
    /// How much of the band reaches frequency `f`, 0 to 1, eased.
    fn reach(self, f: f32) -> f32 {
        let rise = (f - self.low) / (self.inner_low - self.low + 1e-4);
        let fall = (self.high - f) / (self.high - self.inner_high + 1e-4);
        let t = rise.min(fall).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Frequency `f`'s amplitude through it.
    fn through(self, f: f32) -> f32 {
        1.0 + self.gain * self.reach(f)
    }
}

/// Each sound's shape over the frequencies.
const ERASER_BANDS: [Band; 3] = [
    band(-100.0, 0.0, 25_000.0, 38_000.0, -0.7),
    band(13_000.0, 15_000.0, 15_100.0, 17_000.0, 2.0),
    band(17_000.0, 18_000.0, 24_000.0, 30_000.0, -1.0),
];
/// Graphite on paper (our own, as the plugin followed plays a recording): a dry rasp in the upper
/// middle, little low body.
const PENCIL_BANDS: [Band; 4] = [
    band(-100.0, 0.0, 25_000.0, 38_000.0, -0.7),
    band(0.0, 0.0, 150.0, 450.0, -0.9),
    band(900.0, 2_200.0, 4_800.0, 9_000.0, 3.0),
    band(9_000.0, 11_000.0, 24_000.0, 30_000.0, -0.85),
];
const PEN_BANDS: [Band; 6] = [
    band(-100.0, 0.0, 25_000.0, 38_000.0, -0.95),
    band(-300.0, 570.0, 980.0, 2_800.0, 12.0),
    band(0.0, 100.0, 100.0, 150.0, 1.0),
    band(70.0, 360.0, 360.0, 460.0, 1.5),
    band(2_500.0, 3_000.0, 3_010.0, 3_500.0, 0.6),
    band(8_000.0, 8_300.0, 15_000.0, 18_000.0, -0.9),
];
const PAINTBRUSH_BANDS: [Band; 2] =
    [band(-100.0, 0.0, 25_000.0, 38_000.0, -0.25), band(-100.0, 0.0, 1_200.0, 3_800.0, 0.2)];
const AIRBRUSH_BANDS: [Band; 2] =
    [band(-100.0, 0.0, 25_000.0, 38_000.0, -0.52), band(7_000.0, 11_990.0, 12_010.0, 17_000.0, 4.0)];
const SPRAY_BANDS: [Band; 2] =
    [band(-100.0, 0.0, 25_000.0, 38_000.0, -0.58), band(3_250.0, 11_990.0, 12_010.0, 20_750.0, 4.0)];

impl Kind {
    /// The sound's shape over the frequencies.
    fn bands(self) -> &'static [Band] {
        match self {
            Kind::Eraser => &ERASER_BANDS,
            Kind::Pencil => &PENCIL_BANDS,
            Kind::Pen => &PEN_BANDS,
            Kind::Paintbrush => &PAINTBRUSH_BANDS,
            Kind::Airbrush => &AIRBRUSH_BANDS,
            Kind::SprayCan => &SPRAY_BANDS,
        }
    }

    /// The speed (window heights a second) at which it is at its loudest.
    fn top_speed(self) -> f32 {
        match self {
            Kind::Eraser => 4.0,
            Kind::Pen | Kind::Airbrush | Kind::SprayCan => 6.0,
            Kind::Pencil | Kind::Paintbrush => 8.0,
        }
    }

    /// How loud it is made against the others: what the brushes taking it are given on average.
    fn volume(self) -> f32 {
        match self {
            Kind::Eraser => 0.68,
            Kind::Pencil => 0.75,
            Kind::Pen => 0.55,
            Kind::Paintbrush => 0.91,
            Kind::Airbrush => 0.85,
            Kind::SprayCan => 0.88,
        }
    }

    /// The bands that move with the stroke, `speed` its share of its top speed: the pen's ring and
    /// the paintbrush's rise with it, the pencil gains body pressed hard.
    fn moving_bands(self, speed: f32, pressure: f32) -> Vec<Band> {
        match self {
            Kind::Pen => {
                let s = -50.0 + 125.0 * speed * speed;
                vec![
                    band(650.0 + s, 700.0 + s, 720.0 + s, 1_320.0 + s, 2.0),
                    band(650.0 + s, 700.0 + s, 720.0 + s, 1_320.0 + s, 5.0),
                ]
            }
            Kind::Pencil => vec![band(200.0, 400.0, 1_000.0, 1_700.0, 2.5 * (2.0 * pressure - 1.0).clamp(0.0, 1.0))],
            Kind::Paintbrush => {
                let s = 500.0 * speed.powf(1.2);
                vec![band(300.0 + s, 800.0 + s, 900.0 + s, 1_200.0 + s, 0.9)]
            }
            Kind::Eraser | Kind::Airbrush | Kind::SprayCan => Vec::new(),
        }
    }

    /// The share of its loudness a light touch still has.
    fn pressure_floor(self) -> f32 {
        match self {
            Kind::Eraser => 0.7,
            Kind::Pencil | Kind::Pen => 0.3,
            Kind::Paintbrush => 0.1,
            Kind::Airbrush | Kind::SprayCan => 0.45,
        }
    }
}

/// What a plugin sets: how loud, whether erasing makes a sound and which, and a sound for every
/// brush (none: each brush's kind's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Settings {
    pub(crate) volume: f32,
    pub(crate) eraser_own: bool,
    pub(crate) eraser: Kind,
    pub(crate) chosen: Option<Kind>,
    /// For each of [`KINDS`], the control whose recording it plays, by its place (none: its own).
    pub(crate) recorded: [Option<usize>; 6],
}

/// For each of [`KINDS`], the recording it plays in place of its own sound, if there is one.
pub(crate) type Recordings = [Option<PathBuf>; 6];

/// A recording as it is now: where it is, and when it last changed (another picked in its place
/// is another recording).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Source {
    path: PathBuf,
    changed: Option<std::time::SystemTime>,
}

impl Source {
    fn of(path: &Path) -> Option<Source> {
        let meta = std::fs::metadata(path).ok().filter(std::fs::Metadata::is_file)?;
        Some(Source { path: path.to_owned(), changed: meta.modified().ok() })
    }
}

/// How loud `samples` are, as a root mean square.
fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

/// `samples` at `from` a second, at `to` a second (each sample between its neighbours, in a line).
fn resampled(samples: &[f32], from: f32, to: f32) -> Vec<f32> {
    if (from - to).abs() < 0.5 || from <= 0.0 || to <= 0.0 {
        return samples.to_vec();
    }
    let step = f64::from(from) / f64::from(to);
    let n = (samples.len() as f64 / step) as usize;
    (0..n)
        .map(|i| {
            let x = i as f64 * step;
            let j = x as usize;
            let a = samples.get(j).copied().unwrap_or(0.0);
            let b = samples.get(j + 1).copied().unwrap_or(a);
            a + (b - a) * (x - j as f64) as f32
        })
        .collect()
}

/// The recording at `path` as `kind`'s loop at `rate`: at that rate, centred, as loud as `kind`'s
/// own loop, its end eased into its start so it goes round without a click. Empty if it can't be
/// read (the sound's own loop plays).
fn recorded_loop(kind: Kind, rate: f32, path: &Path) -> Vec<f32> {
    let Some((samples, from)) = std::fs::read(path).ok().and_then(|bytes| crate::sound_file::read_wav(&bytes)) else {
        log::warn!("a recording for a brush sound couldn't be read: {}", path.display());
        return Vec::new();
    };
    let mut out = resampled(&samples, from as f32, rate);
    let mean = out.iter().sum::<f32>() / out.len().max(1) as f32;
    out.iter_mut().for_each(|s| *s -= mean);
    // the last 30 ms eased over the first, and cut off: the end runs on into the start
    let fade = ((rate * 0.03) as usize).min(out.len() / 4);
    if fade > 0 {
        let tail = out.split_off(out.len() - fade);
        for (k, (s, t)) in out.iter_mut().zip(tail).enumerate() {
            let r = k as f32 / fade as f32;
            *s = *s * r + t * (1.0 - r);
        }
    }
    let seed = 0x9E37_79B9 ^ (kind as u32).wrapping_mul(0x85EB_CA6B);
    let target = rms(&make_loop(kind, rate, seed));
    let own = rms(&out);
    if own > 1e-6 {
        out.iter_mut().for_each(|s| *s *= target / own);
    }
    out
}

/// A brush's own sound, its setting: 0 as the plugin sets, 1 to 6 one of [`KINDS`], 7 none.
const BRUSH_SOUND: &str = "brush_sound";

impl Settings {
    /// The sound a plugin's setting picks (0 to 1, in sixths): none for 0.
    pub(crate) fn chosen_of(share: f32) -> Option<Kind> {
        let n = (share.clamp(0.0, 1.0) * 6.0).round() as usize;
        n.checked_sub(1).and_then(|i| KINDS.get(i)).copied()
    }
}

/// The sound a brush makes, by its name, else its set's (as the plugin it follows gives each kind
/// of brush its sound), a pencil's if neither says.
pub(crate) fn kind_of(brush: &str, set: &str) -> Kind {
    let by = |name: &str| {
        let name = name.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
        if has(&["eraser"]) {
            Some(Kind::Eraser)
        } else if has(&["spray", "splatter", "graffiti"]) {
            Some(Kind::SprayCan)
        } else if has(&["airbrush", "air brush"]) {
            Some(Kind::Airbrush)
        } else if has(&["pencil", "graphite", "sketch", "charcoal", "chalk", "crayon", "marker", "pastel"]) {
            Some(Kind::Pencil)
        } else if has(&["pen", "ink", "liner", "callig", "nib", "quill"]) {
            Some(Kind::Pen)
        } else if has(&["paint", "brush", "oil", "acrylic", "gouache", "water", "wet", "bristle", "sumi", "glaze"]) {
            Some(Kind::Paintbrush)
        } else if has(&["drawing"]) {
            Some(Kind::Pencil)
        } else {
            None
        }
    };
    by(brush).or_else(|| by(set)).unwrap_or(Kind::Pencil)
}

/// A complex number, for the transform the loops are shaped in.
#[derive(Clone, Copy, Default)]
struct Complex {
    re: f32,
    im: f32,
}

/// The discrete Fourier transform of `data` (its length a power of two) in place; `inverse` for
/// the way back (unscaled).
fn transform(data: &mut [Complex], inverse: bool) {
    let n = data.len();
    // bit reversal
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            data.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut size = 2;
    while size <= n {
        let step = sign * TAU / size as f32;
        for chunk in data.chunks_exact_mut(size) {
            let (front, back) = chunk.split_at_mut(size / 2);
            for (k, (x, y)) in front.iter_mut().zip(back.iter_mut()).enumerate() {
                let (s, c) = (step * k as f32).sin_cos();
                let t = Complex { re: y.re * c - y.im * s, im: y.re * s + y.im * c };
                let was = *x;
                *x = Complex { re: was.re + t.re, im: was.im + t.im };
                *y = Complex { re: was.re - t.re, im: was.im - t.im };
            }
        }
        size <<= 1;
    }
}

/// `kind`'s loop at `rate`: noise shaped by its bands (the pencil's made as loud as [`PENCIL_RMS`]).
fn make_loop(kind: Kind, rate: f32, seed: u32) -> Vec<f32> {
    let mut noise = seed | 1;
    let mut data: Vec<Complex> = (0..LOOP)
        .map(|_| {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            Complex { re: noise as f32 / u32::MAX as f32 - 0.5, im: 0.0 }
        })
        .collect();
    transform(&mut data, false);
    for (k, c) in data.iter_mut().enumerate() {
        let f = k.min(LOOP - k) as f32 * rate / LOOP as f32;
        let gain: f32 = kind.bands().iter().map(|b| b.through(f)).product();
        c.re *= gain;
        c.im *= gain;
    }
    transform(&mut data, true);
    let samples: Vec<f32> = data.iter().map(|c| c.re / LOOP as f32).collect();
    if kind != Kind::Pencil {
        return samples;
    }
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / LOOP as f32).sqrt().max(1e-9);
    samples.into_iter().map(|s| s * PENCIL_RMS / rms).collect()
}

/// `window` filtered by `bands` in the frequencies, at `rate` (its length a power of two).
fn filtered(window: &mut [f32], bands: &[Band], rate: f32) {
    let n = window.len();
    let mut data: Vec<Complex> = window.iter().map(|&re| Complex { re, im: 0.0 }).collect();
    transform(&mut data, false);
    for (k, c) in data.iter_mut().enumerate() {
        let f = k.min(n - k) as f32 * rate / n as f32;
        let gain: f32 = bands.iter().map(|b| b.through(f)).product();
        c.re *= gain;
        c.im *= gain;
    }
    transform(&mut data, true);
    for (s, c) in window.iter_mut().zip(&data) {
        *s = c.re / n as f32;
    }
}

/// The master volume as heard: rising slowly at first (`share` 0 to 1).
fn heard_volume(share: f32) -> f32 {
    10f32.powf(0.3 * share.clamp(0.0, 1.0)) - 1.0
}

/// The stroke as the sound goes by it now.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Motion {
    /// Window heights a second, and the pressure (a finger as pressed all the way).
    pub(crate) speed: f32,
    pub(crate) pressure: f32,
    /// Whether a stroke is open, whether it is moving (or just was).
    pub(crate) pressed: bool,
    pub(crate) moving: bool,
    pub(crate) sprayed: bool,
    /// Where the pen hovers (window pixels), if it does, and the window's height (pixels).
    pub(crate) hover: Option<(f32, f32)>,
    pub(crate) height: f32,
}

/// The can's rattle at `rate`, a second of it to loop: its ball knocking about, a short ringing
/// click every 15 to 45 ms, each as hard as it happens to be.
fn make_rattle(rate: f32) -> Vec<f32> {
    let n = rate as usize;
    let mut out = vec![0.0f32; n];
    let mut noise = 0x6C8E_9CF5_u32;
    let next = |noise: &mut u32| {
        *noise ^= *noise << 13;
        *noise ^= *noise >> 17;
        *noise ^= *noise << 5;
        *noise as f32 / u32::MAX as f32
    };
    let mut at = 0usize;
    while at < n {
        let hard = 0.4 + 0.6 * next(&mut noise);
        let turn = TAU * next(&mut noise);
        let ring = (rate * 0.03) as usize;
        for (k, slot) in out.iter_mut().skip(at).take(ring).enumerate() {
            let t = k as f32 / rate;
            let fade = (-t / 0.006).exp();
            let tone = (TAU * 2_900.0 * t).sin() + 0.6 * (TAU * 5_300.0 * t + turn).sin();
            let tap = if t < 0.001 { next(&mut noise) * 2.0 - 1.0 } else { 0.0 };
            *slot += hard * fade * (0.6 * tone + tap) * 0.35;
        }
        at += (rate * (0.015 + 0.03 * next(&mut noise))) as usize;
    }
    out
}

/// The sound as it is being made: each sound's loop (for the rate it was made at), where it is
/// in it, the last window's second half, what is made and not yet handed out, and the pencil's
/// wander.
#[derive(Default)]
pub(crate) struct Voice {
    loops: Vec<(Kind, f32, Vec<f32>)>,
    /// The recordings read for sounds (empty: one that couldn't be read), and the one to play now.
    recorded: Vec<(Kind, f32, Source, Vec<f32>)>,
    playing: Option<Source>,
    at: usize,
    tail: Vec<f32>,
    ready: VecDeque<f32>,
    /// A slow wander for the pencil's grain of paper.
    tooth: f32,
    noise: u32,
    /// The can's rattle (for the rate it was made at) and where it is in it; how shaken the can
    /// is, 0 to 1; and the hover and its speed at the last block.
    rattle: Option<(f32, Vec<f32>)>,
    rattle_at: usize,
    shake: f32,
    last_hover: Option<(f32, f32)>,
    last_velocity: (f32, f32),
}

impl Voice {
    /// Whether `kind`'s loop at `rate` is still to be made.
    fn lacks(&self, kind: Kind, rate: f32) -> bool {
        !self.loops.iter().any(|(k, r, _)| *k == kind && *r == rate)
    }

    /// Whether the recording `source` for `kind` at `rate` is still to be read.
    fn lacks_recorded(&self, kind: Kind, rate: f32, source: &Source) -> bool {
        !self.recorded.iter().any(|(k, r, s, _)| *k == kind && *r == rate && s == source)
    }

    /// `kind`'s loop at `rate`, made the first time it is asked for.
    fn looped(&mut self, kind: Kind, rate: f32) -> usize {
        if let Some(i) = self.loops.iter().position(|(k, r, _)| *k == kind && *r == rate) {
            return i;
        }
        let seed = 0x9E37_79B9 ^ (kind as u32).wrapping_mul(0x85EB_CA6B);
        self.loops.push((kind, rate, make_loop(kind, rate, seed)));
        self.loops.len() - 1
    }

    /// How loud `kind` is for `m`, 0 to 1.
    fn loudness(kind: Kind, m: Motion) -> f32 {
        let floor = kind.pressure_floor();
        let pressed = |p: f32| floor + (1.0 - floor) * p;
        let speed = (m.speed / kind.top_speed()).clamp(0.0, 1.0);
        let eased = |s: f32| (9.0 * s + 1.0).log10();
        match kind {
            _ if !m.pressed => 0.0,
            Kind::Eraser if m.moving => speed * pressed(m.pressure),
            Kind::Pencil | Kind::Pen if m.moving => eased(speed) * pressed(m.pressure),
            Kind::Paintbrush if m.moving => {
                let p = m.pressure * m.pressure * (3.0 - 2.0 * m.pressure);
                eased(speed.powf(1.75)) * pressed(p)
            }
            Kind::Airbrush => pressed(m.pressure),
            Kind::SprayCan if m.sprayed => pressed(m.pressure),
            _ => 0.0,
        }
    }

    /// The next block of `kind` (none: silence) at `rate`, for the stroke as `m` has it now.
    fn block(&mut self, rate: f32, kind: Option<Kind>, m: Motion, volume: f32) {
        if self.tail.len() != BLOCK {
            self.tail = vec![0.0; BLOCK];
        }
        let mut window = vec![0.0; 2 * BLOCK];
        if let Some(kind) = kind {
            // the recording picked for it, else its own loop
            let own = self.playing.as_ref().and_then(|playing| {
                self.recorded.iter().position(|(k, r, s, samples)| *k == kind && *r == rate && s == playing && !samples.is_empty())
            });
            let looped = if own.is_none() { Some(self.looped(kind, rate)) } else { None };
            let samples = match (own, looped) {
                (Some(i), _) => self.recorded.get(i).map(|(_, _, _, samples)| samples),
                (None, Some(i)) => self.loops.get(i).map(|(_, _, samples)| samples),
                (None, None) => None,
            };
            let len = samples.map_or(LOOP, |samples| samples.len().max(1));
            if let Some(samples) = samples {
                let from = samples.iter().cycle().skip(self.at % len);
                for (w, &s) in window.iter_mut().zip(from) {
                    *w = s;
                }
            }
            self.at = (self.at + BLOCK) % len;
            let gain = Self::loudness(kind, m) * kind.volume() * heard_volume(volume);
            window.iter_mut().for_each(|w| *w *= gain);
            if kind == Kind::Pencil {
                self.tooth_on(&mut window, rate);
            }
            let speed = (m.speed / kind.top_speed()).clamp(0.0, 1.0);
            let bands = kind.moving_bands(speed, m.pressure);
            if !bands.is_empty() && gain > 0.0 {
                filtered(&mut window, &bands, rate);
            }
        }
        self.rattle_on(&mut window, rate, kind, m, volume);
        // the first half eased in over the last window's second half
        let (first, second) = window.split_at_mut(BLOCK);
        for (k, (w, &was)) in first.iter_mut().zip(&self.tail).enumerate() {
            let t = k as f32 / (BLOCK - 1) as f32;
            let r = t * t * (3.0 - 2.0 * t);
            *w = was * (1.0 - r) + *w * r;
        }
        self.ready.extend(first.iter().map(|s| s.clamp(-1.0, 1.0)));
        self.tail.copy_from_slice(second);
    }

    /// The can's rattle into `window`: shaken back and forth while the pen hovers (not pressed),
    /// the can's ball knocks, as loud as the square of how shaken it is, which dies away.
    fn rattle_on(&mut self, window: &mut [f32], rate: f32, kind: Option<Kind>, m: Motion, volume: f32) {
        if kind != Some(Kind::SprayCan) {
            self.shake = 0.0;
            self.last_hover = None;
            self.last_velocity = (0.0, 0.0);
            return;
        }
        let dt = BLOCK as f32 / rate;
        let velocity = match (m.hover, self.last_hover) {
            (Some(now), Some(was)) => {
                let scale = 1.0 / dt / m.height.max(1.0) / Kind::SprayCan.top_speed();
                let (x, y) = ((now.0 - was.0) * scale, (now.1 - was.1) * scale);
                let length = x.hypot(y);
                if length > 1.0 { (x / length, y / length) } else { (x, y) }
            }
            _ => (0.0, 0.0),
        };
        // turning back against the way it went shakes it
        let (was, accel) = (self.last_velocity, (velocity.0 - self.last_velocity.0, velocity.1 - self.last_velocity.1));
        let against = -(was.0 * accel.0 + was.1 * accel.1);
        let most = if m.pressed { 0.0 } else { 1.0 };
        self.shake = (self.shake + against.clamp(0.0, most)).clamp(0.0, 1.0);
        let loud = self.shake * self.shake * Kind::SprayCan.volume() * heard_volume(volume);
        if loud > 0.0 && m.hover.is_some() {
            if self.rattle.as_ref().is_none_or(|(r, _)| *r != rate) {
                self.rattle = Some((rate, make_rattle(rate)));
                self.rattle_at = 0;
            }
            if let Some((_, rattle)) = &self.rattle {
                let from = rattle.iter().cycle().skip(self.rattle_at % rattle.len().max(1));
                for (w, &r) in window.iter_mut().zip(from) {
                    *w += r * loud;
                }
                self.rattle_at = (self.rattle_at + BLOCK) % rattle.len().max(1);
            }
        }
        self.shake = (self.shake - SHAKE_FADE * dt).max(0.0);
        self.last_velocity = velocity;
        self.last_hover = m.hover;
    }

    /// The pencil's grain of paper on `window`: the rasp catching and slipping (our own touch, as
    /// the plugin it follows plays a recording of a pencil there).
    fn tooth_on(&mut self, window: &mut [f32], rate: f32) {
        if self.noise == 0 {
            self.noise = 0x2545_F491;
        }
        let wander = 1.0 - (-TAU * 40.0 / rate).exp();
        for w in window.iter_mut() {
            self.noise ^= self.noise << 13;
            self.noise ^= self.noise >> 17;
            self.noise ^= self.noise << 5;
            let jitter = self.noise as f32 / u32::MAX as f32;
            self.tooth += wander * (jitter - self.tooth);
            *w *= 0.55 + 0.9 * self.tooth;
        }
    }

    /// `frames` samples of `kind` (none: silence) at `rate` for the stroke as `m` has it, at
    /// master `volume` (0 to 1), made a block at a time as they are needed.
    pub(crate) fn render(&mut self, frames: usize, rate: f32, kind: Option<Kind>, m: Motion, volume: f32) -> Vec<f32> {
        while self.ready.len() < frames {
            self.block(rate, kind, m, volume);
        }
        self.ready.drain(..frames).collect()
    }
}

impl crate::State {
    /// The stroke being drawn as its sound goes by it: its speed in window heights a second, its
    /// pressure, and whether it is pressed, moving, or (for a spray can) moving or just was.
    fn sound_motion(&self) -> Motion {
        let heard = self.sound_heard.map(|at| at.elapsed().as_millis());
        let pressed = self.stroke.is_some();
        let hover =
            self.sound_hover.filter(|(_, at)| at.elapsed().as_millis() <= HOVER_STALE_MS).map(|(p, _)| (p.x, p.y));
        let height = self.window_size.1.max(1) as f32;
        let Some(last) = self.drawn_samples.last().filter(|_| pressed) else {
            return Motion { hover, height, ..Motion::default() };
        };
        let pressure = if last.pressure > 0.0 { last.pressure.clamp(0.0, 1.0) } else { 1.0 };
        let moving = heard.is_some_and(|ms| ms <= STILL_MS);
        let sprayed = heard.is_some_and(|ms| ms <= STILL_MS + SPRAY_GRACE_MS);
        let earlier = self
            .drawn_samples
            .iter()
            .rev()
            .take(32)
            .find(|s| last.time_ms - s.time_ms >= SPEED_SPAN_MS)
            .or(self.drawn_samples.first());
        let ms = earlier.map_or(0.0, |e| (last.time_ms - e.time_ms) as f32);
        let speed = match earlier {
            Some(e) if ms > 0.0 && moving => {
                let zoom = self.view.map_or(1.0, |v| v.zoom());
                let height = self.window_size.1.max(1) as f32;
                last.pos.distance(e.pos) * zoom / ms * 1000.0 / height
            }
            _ => 0.0,
        };
        Motion { speed, pressure, pressed, moving, sprayed, hover, height }
    }

    /// The sound the stroke being drawn makes, as `s` sets it: the brush's own if it has one (none
    /// for no sound); else erasing, its sound if erasing is heard; else the one for every brush,
    /// else the brush's kind's (as the plugin it follows has it).
    fn sound_kind(&self, s: Settings) -> Option<Kind> {
        // the brush's own, as the plugin it follows lets a brush have its own
        match self.brush(self.tool).setting(BRUSH_SOUND).round() as usize {
            0 => {}
            own => return KINDS.get(own - 1).copied(),
        }
        if self.tool == ToolKind::Erase {
            return s.eraser_own.then_some(s.eraser);
        }
        if let Some(kind) = s.chosen {
            return Some(kind);
        }
        let (set, index) = self.brush_ref(self.tool);
        let set = self.library.get(set);
        let brush = set.and_then(|s| s.brushes.get(index)).map_or("", |b| b.name.as_str());
        Some(kind_of(brush, set.map_or("", |s| s.name.as_str())))
    }
}

#[uniffi::export]
impl CanvasSession {
    /// The pen (or the mouse) hovers over the canvas at (`x`, `y`), window pixels: shaken back and
    /// forth with a spray can's sound, its ball rattles.
    pub fn sound_hover(&self, x: f32, y: f32) {
        if x.is_finite() && y.is_finite() {
            self.shared.lock().sound_hover = Some((procraft_engine::geom::Vec2::new(x, y), std::time::Instant::now()));
        }
    }

    /// The next `frames` samples (mono, -1 to 1, at `rate` a second) of the sound of the stroke
    /// being drawn, as a plugin sets it: silence with none on or no stroke. An app plays them one
    /// after another as fast as they are used.
    pub fn stroke_sound(&self, frames: u32, rate: u32) -> Vec<f32> {
        let frames = frames.min(48_000) as usize;
        let rate = rate.clamp(8_000, 192_000) as f32;
        let hooks = self.shared.lock().hooks.clone();
        let Some((settings, recordings)) = hooks.and_then(|hooks| hooks.stroke_sound()) else {
            return vec![0.0; frames];
        };
        // a sound's loop made the first time, away from the artwork's lock (it takes a moment); a
        // recording picked for it read the same way
        let (playing, lacking, lacking_recorded) = {
            let st = self.shared.lock();
            let kind = st.sound_kind(settings);
            let recording = kind.and_then(|kind| KINDS.iter().position(|k| *k == kind)).and_then(|i| recordings.get(i));
            let playing = recording.and_then(|path| path.as_deref()).and_then(Source::of);
            let lacking = kind.filter(|&kind| st.voice.lacks(kind, rate));
            let lacking_recorded = kind.zip(playing.clone()).filter(|(kind, source)| st.voice.lacks_recorded(*kind, rate, source));
            (playing, lacking, lacking_recorded)
        };
        if let Some((kind, source)) = lacking_recorded {
            let read = recorded_loop(kind, rate, &source.path);
            let mut st = self.shared.lock();
            if st.voice.lacks_recorded(kind, rate, &source) {
                // one picked before it, let go
                st.voice.recorded.retain(|(k, r, _, _)| !(*k == kind && *r == rate));
                st.voice.recorded.push((kind, rate, source, read));
            }
        }
        if let Some(kind) = lacking {
            let seed = 0x9E37_79B9 ^ (kind as u32).wrapping_mul(0x85EB_CA6B);
            let made = make_loop(kind, rate, seed);
            let mut st = self.shared.lock();
            if st.voice.lacks(kind, rate) {
                st.voice.loops.push((kind, rate, made));
            }
        }
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.voice.playing = playing;
        let (kind, motion) = (st.sound_kind(settings), st.sound_motion());
        st.voice.render(frames, rate, kind, motion, settings.volume)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt()
    }

    #[test]
    fn each_sound_has_its_shape_and_follows_the_stroke() {
        let rate = 48_000.0;
        let moving =
            Motion { speed: 3.0, pressure: 1.0, pressed: true, moving: true, sprayed: true, ..Motion::default() };
        let still = Motion { moving: false, sprayed: false, speed: 0.0, ..moving };
        for kind in KINDS {
            let mut voice = Voice::default();
            let _ = voice.render(4_800, rate, Some(kind), moving, 1.0);
            let loud = rms(&voice.render(4_800, rate, Some(kind), moving, 1.0));
            assert!(loud > 0.005, "{kind:?} {loud}");
            assert!(voice.render(48_000, rate, Some(kind), moving, 1.0).iter().all(|x| x.abs() <= 1.0));
            // held still: quiet, but for the airbrush, which hisses while pressed
            let _ = voice.render(9_600, rate, Some(kind), still, 1.0);
            let held = rms(&voice.render(4_800, rate, Some(kind), still, 1.0));
            if kind == Kind::Airbrush {
                assert!(held > loud / 2.0, "{held} {loud}");
            } else {
                assert!(held < loud / 50.0, "{kind:?} held {held} after {loud}");
            }
        }
        // the loops' shapes: the airbrush's and the eraser's hiss high, the paintbrush's sits low
        let energy = |kind: Kind, low: f32, high: f32| {
            let mut data: Vec<Complex> =
                make_loop(kind, rate, 7).into_iter().map(|re| Complex { re, im: 0.0 }).collect();
            transform(&mut data, false);
            (1..LOOP / 2)
                .filter(|&k| (low..high).contains(&(k as f32 * rate / LOOP as f32)))
                .map(|k| data[k].re * data[k].re + data[k].im * data[k].im)
                .sum::<f32>()
        };
        assert!(energy(Kind::Airbrush, 10_000.0, 14_000.0) > 4.0 * energy(Kind::Airbrush, 1_000.0, 5_000.0));
        assert!(energy(Kind::Eraser, 14_000.0, 16_000.0) > 2.0 * energy(Kind::Eraser, 2_000.0, 4_000.0));
        assert!(energy(Kind::Pen, 600.0, 1_000.0) > 4.0 * energy(Kind::Pen, 5_000.0, 9_000.0));
    }

    #[test]
    fn a_spray_can_shaken_above_the_canvas_rattles() {
        let rate = 48_000.0;
        let at = |x: f32| Motion { hover: Some((x, 500.0)), height: 1_000.0, ..Motion::default() };
        let mut voice = Voice::default();
        // still above the canvas: nothing
        let _ = voice.render(4_800, rate, Some(Kind::SprayCan), at(500.0), 1.0);
        assert_eq!(rms(&voice.render(2_048, rate, Some(Kind::SprayCan), at(500.0), 1.0)), 0.0);
        // shaken: back and forth a block at a time
        let mut heard = 0.0f32;
        for k in 0..40 {
            let x = if k % 2 == 0 { 300.0 } else { 700.0 };
            heard = heard.max(rms(&voice.render(1_024, rate, Some(Kind::SprayCan), at(x), 1.0)));
        }
        assert!(heard > 0.01, "{heard}");
        // left alone it dies away
        let _ = voice.render(48_000, rate, Some(Kind::SprayCan), at(500.0), 1.0);
        assert!(rms(&voice.render(2_048, rate, Some(Kind::SprayCan), at(500.0), 1.0)) < 1e-4);
        // another sound shaken: no rattle
        let mut pen = Voice::default();
        for k in 0..40 {
            let x = if k % 2 == 0 { 300.0 } else { 700.0 };
            assert_eq!(rms(&pen.render(1_024, rate, Some(Kind::Pen), at(x), 1.0)), 0.0);
        }
    }

    #[test]
    fn faster_and_harder_is_louder() {
        let m = |speed: f32, pressure: f32| Motion {
            speed,
            pressure,
            pressed: true,
            moving: true,
            sprayed: true,
            ..Motion::default()
        };
        assert!(Voice::loudness(Kind::Pen, m(1.0, 1.0)) < Voice::loudness(Kind::Pen, m(4.0, 1.0)));
        assert!(Voice::loudness(Kind::Pen, m(4.0, 0.2)) < Voice::loudness(Kind::Pen, m(4.0, 1.0)));
        assert!((Voice::loudness(Kind::Pen, m(60.0, 1.0)) - 1.0).abs() < 1e-5, "at its top speed in full");
        assert_eq!(Voice::loudness(Kind::Pen, Motion { pressed: false, ..m(4.0, 1.0) }), 0.0);
    }

    #[test]
    fn a_recording_plays_in_place_of_its_sound() {
        let rate = 48_000.0;
        let dir = std::env::temp_dir().join(format!("procraft-recording-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // half a second of 440 Hz at 44.1 kHz, loud and off centre
        let tone: Vec<f32> = (0..22_050).map(|i| 0.2 + 0.7 * (TAU * 440.0 * i as f32 / 44_100.0).sin()).collect();
        let path = dir.join("tone.wav");
        std::fs::write(&path, crate::sound_file::write_wav(&tone, 44_100)).unwrap();
        let read = recorded_loop(Kind::Airbrush, rate, &path);
        assert!((read.len() as f32 - (0.5 - 0.03) * rate).abs() < 10.0, "{}", read.len());
        let own = make_loop(Kind::Airbrush, rate, 0x9E37_79B9 ^ (Kind::Airbrush as u32).wrapping_mul(0x85EB_CA6B));
        assert!((rms(&read) / rms(&own) - 1.0).abs() < 0.01, "as loud as the sound's own");
        assert!(read.iter().sum::<f32>().abs() / (read.len() as f32) < 0.01, "centred");
        // played while the airbrush is pressed: the tone, crossing zero 880 times a second
        let source = Source::of(&path).unwrap();
        let mut voice = Voice { playing: Some(source.clone()), ..Voice::default() };
        voice.recorded.push((Kind::Airbrush, rate, source, read));
        let pressed = Motion { pressure: 1.0, pressed: true, ..Motion::default() };
        let _ = voice.render(4_800, rate, Some(Kind::Airbrush), pressed, 1.0);
        let heard = voice.render(48_000, rate, Some(Kind::Airbrush), pressed, 1.0);
        let crossings = heard.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((800..960).contains(&crossings), "{crossings}");
        // one that can't be read: the sound's own
        std::fs::write(&path, b"no longer a sound").unwrap();
        assert!(recorded_loop(Kind::Airbrush, rate, &path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_brush_given_its_own_sound_makes_it() {
        let s = crate::CanvasSession::new(64, 64);
        let mut st = s.shared.lock();
        let all = Settings { volume: 1.0, eraser_own: true, eraser: Kind::Eraser, chosen: None, recorded: [None; 6] };
        let pen = Settings { chosen: Some(Kind::Pen), ..all };
        assert_eq!(st.sound_kind(pen), Some(Kind::Pen));
        // its own over the one for every brush, or none
        let own = |st: &mut crate::State, tool, value| st.brush_mut(tool).expect("a brush").set_setting(BRUSH_SOUND, value);
        own(&mut st, ToolKind::Paint, 5.0);
        assert_eq!(st.sound_kind(pen), Some(Kind::Airbrush));
        own(&mut st, ToolKind::Paint, 7.0);
        assert_eq!(st.sound_kind(pen), None);
        own(&mut st, ToolKind::Paint, 12.0);
        assert_eq!(st.brush(ToolKind::Paint).setting(BRUSH_SOUND), 7.0);
        // erasing: the sound it is given, or none; the eraser's brush's own over either
        st.tool = ToolKind::Erase;
        assert_eq!(st.sound_kind(all), Some(Kind::Eraser));
        assert_eq!(st.sound_kind(Settings { eraser: Kind::Pencil, ..all }), Some(Kind::Pencil));
        assert_eq!(st.sound_kind(Settings { eraser_own: false, ..all }), None);
        own(&mut st, ToolKind::Erase, 6.0);
        assert_eq!(st.sound_kind(Settings { eraser_own: false, ..all }), Some(Kind::SprayCan));
    }

    #[test]
    fn brushes_sound_as_their_kind() {
        assert_eq!(kind_of("6B Pencil", "Sketching"), Kind::Pencil);
        assert_eq!(kind_of("Studio Pen", "Inking"), Kind::Pen);
        assert_eq!(kind_of("Round Brush", "Painting"), Kind::Paintbrush);
        assert_eq!(kind_of("Soft Airbrush", "Airbrushing"), Kind::Airbrush);
        assert_eq!(kind_of("Flicks", "Spray Paint"), Kind::SprayCan);
        assert_eq!(kind_of("Mystery", "Mine"), Kind::Pencil);
        assert_eq!(Settings::chosen_of(0.0), None);
        assert_eq!(Settings::chosen_of(1.0 / 6.0), Some(Kind::Eraser));
        assert_eq!(Settings::chosen_of(1.0), Some(Kind::SprayCan));
    }
}
