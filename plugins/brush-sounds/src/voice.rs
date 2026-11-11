//! The sounds themselves: six of them, an eraser, a pencil, a pen, a paintbrush, an airbrush and a
//! spray can, behaving as Krita's Brush SFX plugin does (davi133/brush_sfx, which it follows;
//! nothing of its code or recordings is in here).
//!
//! Each sound is noise with its own shape over the frequencies, made once (and looped): bands of
//! it raised or lowered, smoothly at their edges. As the stroke goes the sound gets louder with its
//! speed, measured in window heights a second against each sound's top speed, and with its
//! pressure, from each sound's floor up; the pen's and the paintbrush's ring rises with speed,
//! the pencil gains body with pressure. The airbrush hisses while pressed, still or not; the spray
//! can while it moves, and its ball rattles when the pen is shaken back and forth above the canvas
//! (the rattle our own, made of clicks). It is made a block of about 20 ms at a time: the loop,
//! eased from the last block's loudness to this one's, through a filter that moves with the stroke.
//! Each sound's shape over the frequencies is made by a few second-order filters (peaks and
//! shelves), cheap enough to run sample by sample in a plugin on a tablet.

use std::collections::VecDeque;
use std::f32::consts::TAU;

/// Quiet after this long with no new input: a pen held still sends none.
pub const STILL_MS: f32 = 45.0;
/// The spray can goes on this long after the stroke stops moving.
pub const SPRAY_GRACE_MS: f32 = 60.0;
/// How fast the can's shake dies away, a second.
const SHAKE_FADE: f32 = 2.0;
/// A block: what is made at once, the stroke read once for it (about 21 ms at 48 kHz).
const BLOCK: usize = 1 << 10;
/// The samples of each sound's loop (a power of two, about 1.4 s at 48 kHz).
const LOOP: usize = 1 << 16;

/// The six sounds, in the order a plugin numbers them (from 1; 0 is the brush's own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Eraser,
    Pencil,
    Pen,
    Paintbrush,
    Airbrush,
    SprayCan,
}

pub const KINDS: [Kind; 6] = [Kind::Eraser, Kind::Pencil, Kind::Pen, Kind::Paintbrush, Kind::Airbrush, Kind::SprayCan];

/// A second-order filter (a biquad, as the cookbook gives them), normalised.
#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

/// A biquad's memory: its last two inputs and outputs.
#[derive(Clone, Copy, Debug, Default)]
struct Memory {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    /// The angle's sine and cosine at `f` Hz for `rate`, kept short of the top.
    fn angle(f: f32, rate: f32) -> (f32, f32) {
        (TAU * (f / rate).clamp(1e-4, 0.49)).sin_cos()
    }

    /// A peak (or a dip, below 0) of `db` at `f` Hz, `q` narrow.
    fn peak(f: f32, q: f32, db: f32, rate: f32) -> Biquad {
        let a = 10f32.powf(db / 40.0);
        let (s, c) = Biquad::angle(f, rate);
        let alpha = s / (2.0 * q);
        let a0 = 1.0 + alpha / a;
        Biquad {
            b0: (1.0 + alpha * a) / a0,
            b1: -2.0 * c / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: -2.0 * c / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }

    /// Everything below (`low`) or above `f` Hz raised or lowered by `db`.
    fn shelf(f: f32, db: f32, low: bool, rate: f32) -> Biquad {
        let a = 10f32.powf(db / 40.0);
        let (s, c) = Biquad::angle(f, rate);
        let k = 2.0 * a.sqrt() * s / 2.0 * std::f32::consts::SQRT_2;
        let (p, m) = (a + 1.0, a - 1.0);
        if low {
            let a0 = p + m * c + k;
            Biquad {
                b0: a * (p - m * c + k) / a0,
                b1: 2.0 * a * (m - p * c) / a0,
                b2: a * (p - m * c - k) / a0,
                a1: -2.0 * (m + p * c) / a0,
                a2: (p + m * c - k) / a0,
            }
        } else {
            let a0 = p - m * c + k;
            Biquad {
                b0: a * (p + m * c + k) / a0,
                b1: -2.0 * a * (m + p * c) / a0,
                b2: a * (p + m * c - k) / a0,
                a1: 2.0 * (m - p * c) / a0,
                a2: (p - m * c - k) / a0,
            }
        }
    }

    /// Everything above `f` Hz taken out, gently.
    fn low_pass(f: f32, rate: f32) -> Biquad {
        let (s, c) = Biquad::angle(f, rate);
        let alpha = s / std::f32::consts::SQRT_2;
        let a0 = 1.0 + alpha;
        Biquad { b0: (1.0 - c) / 2.0 / a0, b1: (1.0 - c) / a0, b2: (1.0 - c) / 2.0 / a0, a1: -2.0 * c / a0, a2: (1.0 - alpha) / a0 }
    }

    /// `x` through it, as `m` remembers.
    fn run(self, m: &mut Memory, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * m.x1 + self.b2 * m.x2 - self.a1 * m.y1 - self.a2 * m.y2;
        (m.x2, m.x1, m.y2, m.y1) = (m.x1, x, m.y1, y);
        y
    }
}

impl Kind {
    /// The sound's shape over the frequencies at `rate`: the eraser's hiss peaking near 15 kHz, the
    /// pencil's dry rasp in the upper middle with little low body, the pen's ring around 750 Hz,
    /// the paintbrush's low rush, the airbrush's and the spray can's hiss round 12 kHz.
    fn shape(self, rate: f32) -> Vec<Biquad> {
        match self {
            Kind::Eraser => vec![Biquad::peak(15_050.0, 3.0, 9.5, rate), Biquad::low_pass(18_000.0, rate)],
            Kind::Pencil => vec![
                Biquad::shelf(300.0, -20.0, true, rate),
                Biquad::peak(3_300.0, 0.8, 12.0, rate),
                Biquad::shelf(10_000.0, -16.0, false, rate),
            ],
            Kind::Pen => vec![
                Biquad::peak(750.0, 0.8, 22.0, rate),
                Biquad::peak(100.0, 2.0, 6.0, rate),
                Biquad::peak(360.0, 1.5, 8.0, rate),
                Biquad::peak(3_000.0, 6.0, 4.0, rate),
                Biquad::shelf(9_000.0, -20.0, false, rate),
            ],
            Kind::Paintbrush => vec![Biquad::shelf(2_000.0, 1.6, true, rate)],
            Kind::Airbrush => vec![Biquad::peak(12_000.0, 2.0, 14.0, rate)],
            Kind::SprayCan => vec![Biquad::peak(12_000.0, 0.8, 14.0, rate)],
        }
    }

    /// How loud its loop is, as a root mean square.
    fn level(self) -> f32 {
        match self {
            Kind::Eraser => 0.099,
            Kind::Pencil => 0.30,
            Kind::Pen => 0.056,
            Kind::Paintbrush => 0.221,
            Kind::Airbrush => 0.314,
            Kind::SprayCan => 0.349,
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

    /// The filter that moves with the stroke, `speed` its share of its top speed: the pen's ring and
    /// the paintbrush's rise with it, the pencil gains body pressed hard.
    fn moving_peak(self, speed: f32, pressure: f32, rate: f32) -> Option<Biquad> {
        match self {
            Kind::Pen => Some(Biquad::peak(710.0 - 50.0 + 125.0 * speed * speed, 2.5, 25.0, rate)),
            Kind::Pencil => {
                let gain = 2.5 * (2.0 * pressure - 1.0).clamp(0.0, 1.0);
                (gain > 0.0).then(|| Biquad::peak(700.0, 0.9, 20.0 * (1.0 + gain).log10(), rate))
            }
            Kind::Paintbrush => Some(Biquad::peak(850.0 + 500.0 * speed.powf(1.2), 1.5, 5.6, rate)),
            Kind::Eraser | Kind::Airbrush | Kind::SprayCan => None,
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

/// A recording (`samples` at `from` a second) as `kind`'s loop at `rate`: at that rate, centred, as
/// loud as `kind`'s own loop, its end eased into its start so it goes round without a click.
pub fn recorded_loop(kind: Kind, rate: f32, samples: &[f32], from: f32) -> Vec<f32> {
    let mut out = resampled(samples, from, rate);
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
    let target = kind.level();
    let own = rms(&out);
    if own > 1e-6 {
        out.iter_mut().for_each(|s| *s *= target / own);
    }
    out
}

/// The sound a brush makes, by its name, else its set's (as the plugin it follows gives each kind
/// of brush its sound), a pencil's if neither says.
pub fn kind_of(brush: &str, set: &str) -> Kind {
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

/// A complex number, for measuring the loops' shapes.
#[cfg(test)]
#[derive(Clone, Copy, Default)]
struct Complex {
    re: f32,
    im: f32,
}

/// The discrete Fourier transform of `data` (its length a power of two) in place; `inverse` for
/// the way back (unscaled).
/// The turns a transform of `n` (a power of two) takes, the sine and cosine of each step round
/// the circle: worked out once for each size, as working them out for each step would cost most of
/// the transform.
#[cfg(test)]
fn turns(n: usize) -> std::sync::Arc<Vec<(f32, f32)>> {
    static MADE: std::sync::Mutex<Vec<(usize, std::sync::Arc<Vec<(f32, f32)>>)>> = std::sync::Mutex::new(Vec::new());
    let mut made = MADE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, turns)) = made.iter().find(|(size, _)| *size == n) {
        return std::sync::Arc::clone(turns);
    }
    let turns = std::sync::Arc::new((0..n / 2).map(|k| (TAU * k as f32 / n as f32).sin_cos()).collect::<Vec<_>>());
    made.push((n, std::sync::Arc::clone(&turns)));
    turns
}

#[cfg(test)]
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
    let turns = turns(n);
    let mut size = 2;
    while size <= n {
        let stride = n / size;
        for chunk in data.chunks_exact_mut(size) {
            let (front, back) = chunk.split_at_mut(size / 2);
            for (k, (x, y)) in front.iter_mut().zip(back.iter_mut()).enumerate() {
                let (s, c) = turns.get(k * stride).copied().unwrap_or((0.0, 1.0));
                let s = sign * s;
                let t = Complex { re: y.re * c - y.im * s, im: y.re * s + y.im * c };
                let was = *x;
                *x = Complex { re: was.re + t.re, im: was.im + t.im };
                *y = Complex { re: was.re - t.re, im: was.im - t.im };
            }
        }
        size <<= 1;
    }
}

/// `kind`'s loop at `rate`: noise through its shape, as loud as its level, its end eased into its
/// start so it goes round without a click.
fn make_loop(kind: Kind, rate: f32, seed: u32) -> Vec<f32> {
    let shape = kind.shape(rate);
    let mut memories = vec![Memory::default(); shape.len()];
    let mut noise = seed.max(1);
    // made longer, to let the filters settle first and to ease the end in over the start
    let (settle, fade) = (4_096, ((rate * 0.03) as usize).min(LOOP / 4));
    let mut out: Vec<f32> = (0..settle + LOOP + fade)
        .map(|_| {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            let x = noise as f32 / u32::MAX as f32 - 0.5;
            shape.iter().zip(memories.iter_mut()).fold(x, |v, (b, m)| b.run(m, v))
        })
        .collect();
    out.drain(..settle);
    let tail = out.split_off(LOOP);
    for (k, (s, t)) in out.iter_mut().zip(tail).enumerate() {
        let r = k as f32 / fade.max(1) as f32;
        *s = *s * r + t * (1.0 - r);
    }
    let own = rms(&out);
    if own > 1e-9 {
        out.iter_mut().for_each(|s| *s *= kind.level() / own);
    }
    out
}

/// The master volume as heard: rising slowly at first (`share` 0 to 1).
fn heard_volume(share: f32) -> f32 {
    10f32.powf(0.3 * share.clamp(0.0, 1.0)) - 1.0
}

/// The stroke as the sound goes by it now.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    /// Window heights a second, and the pressure (a finger as pressed all the way).
    pub speed: f32,
    pub pressure: f32,
    /// Whether a stroke is open, whether it is moving (or just was).
    pub pressed: bool,
    pub moving: bool,
    pub sprayed: bool,
    /// Where the pen hovers (window pixels), if it does, and the window's height (pixels).
    pub hover: Option<(f32, f32)>,
    pub height: f32,
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
pub struct Voice {
    loops: Vec<(Kind, f32, Vec<f32>)>,
    /// Each sound's recording, as fitted to a rate, played in place of its loop.
    own: [Option<(f32, Vec<f32>)>; 6],
    at: usize,
    ready: VecDeque<f32>,
    /// The sound the last block was, its loudness, and the moving filter's memory.
    last_kind: Option<Kind>,
    last_gain: f32,
    moving: Memory,
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
    /// Every sound's loop at `rate`, made now (a moment's work) rather than the first time each
    /// is heard.
    pub fn prepare(&mut self, rate: f32) {
        for kind in KINDS {
            self.looped(kind, rate);
        }
        if self.rattle.as_ref().is_none_or(|(r, _)| *r != rate) {
            self.rattle = Some((rate, make_rattle(rate)));
        }
    }

    /// `kind` plays `samples` (fitted to `rate`) in place of its own loop; none: its own again.
    pub fn set_own(&mut self, kind: Kind, own: Option<(f32, Vec<f32>)>) {
        if let Some(slot) = self.own.get_mut(kind as usize) {
            *slot = own.filter(|(_, samples)| !samples.is_empty());
        }
    }

    /// Whether `kind` has a recording fitted to a rate other than `rate`.
    pub fn own_rate(&self, kind: Kind) -> Option<f32> {
        self.own.get(kind as usize).and_then(|own| own.as_ref().map(|(rate, _)| *rate))
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
        let mut out = vec![0.0; BLOCK];
        // another sound comes in from quiet
        if kind != self.last_kind {
            (self.last_kind, self.last_gain, self.moving) = (kind, 0.0, Memory::default());
        }
        if let Some(kind) = kind {
            // the recording picked for it, else its own loop
            let own = self.own.get(kind as usize).and_then(Option::as_ref).is_some_and(|(r, _)| *r == rate);
            let looped = if own { None } else { Some(self.looped(kind, rate)) };
            let samples = match looped {
                None => self.own.get(kind as usize).and_then(Option::as_ref).map(|(_, samples)| samples),
                Some(i) => self.loops.get(i).map(|(_, _, samples)| samples),
            };
            let len = samples.map_or(LOOP, |samples| samples.len().max(1));
            if let Some(samples) = samples {
                let from = samples.iter().cycle().skip(self.at % len);
                for (w, &s) in out.iter_mut().zip(from) {
                    *w = s;
                }
            }
            self.at = (self.at + BLOCK) % len;
            // as loud as the stroke says, eased from the last block's over this one
            let gain = Self::loudness(kind, m) * kind.volume() * heard_volume(volume);
            for (k, w) in out.iter_mut().enumerate() {
                let t = (k + 1) as f32 / BLOCK as f32;
                *w *= self.last_gain + (gain - self.last_gain) * t;
            }
            self.last_gain = gain;
            if kind == Kind::Pencil {
                self.tooth_on(&mut out, rate);
            }
            let speed = (m.speed / kind.top_speed()).clamp(0.0, 1.0);
            if let Some(peak) = kind.moving_peak(speed, m.pressure, rate) {
                for w in out.iter_mut() {
                    *w = peak.run(&mut self.moving, *w);
                }
            }
        }
        self.rattle_on(&mut out, rate, kind, m, volume);
        self.ready.extend(out.iter().map(|s| s.clamp(-1.0, 1.0)));
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
    pub fn render(&mut self, frames: usize, rate: f32, kind: Option<Kind>, m: Motion, volume: f32) -> Vec<f32> {
        while self.ready.len() < frames {
            self.block(rate, kind, m, volume);
        }
        self.ready.drain(..frames).collect()
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
    fn brushes_sound_as_their_kind() {
        assert_eq!(kind_of("6B Pencil", "Sketching"), Kind::Pencil);
        assert_eq!(kind_of("Studio Pen", "Inking"), Kind::Pen);
        assert_eq!(kind_of("Round Brush", "Painting"), Kind::Paintbrush);
        assert_eq!(kind_of("Soft Airbrush", "Airbrushing"), Kind::Airbrush);
        assert_eq!(kind_of("Flicks", "Spray Paint"), Kind::SprayCan);
        assert_eq!(kind_of("Mystery", "Mine"), Kind::Pencil);
    }
}
