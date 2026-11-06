//! The liquify brush: the stamps a stroke lays, in the steps the canvas carries its field along by.
//! Everything here is in canvas pixels and seconds.
//!
//! A push moves what is under it by how far the brush went. A twirl, pinch, expand and reconstruct
//! work for as long as they are down, moving or not, the speed only adding when it is high. What
//! keeps going by itself (the brush's momentum, what distortion throws off) is kept as movers, which
//! lay stamps along their own way as time passes.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use procraft_engine::geom::Vec2;
use procraft_gpu::{LiquifyMode, LiquifyStamp};

use crate::adjust::{LiquifyBrush, LiquifyKind};

/// Stamps fall this part of the radius apart, and never less than the least gap.
const STAMP_GAP: f32 = 0.085;
const LEAST_GAP: f32 = 2.0;
/// However short the time, a timed brush works this long (seconds) for each pixel it moves.
const WORK_PER_PIXEL: f32 = 1.0 / 3000.0;
/// A step of the field covers this part of the radius of the brush's move: little enough that how
/// fast the brush went or how often input came doesn't show. And the most steps a batch of input
/// takes, for when a long move comes at once.
const STEP_REACH: f32 = 0.05;
const MOST_STEPS: usize = 16;
/// A move counts for no more time than this (seconds): a longer gap is the brush having stopped,
/// and being held works on by itself.
const LONGEST_MOVE: f32 = 0.05;
/// How fast the speed the brush lifts at follows the moves it makes (of the difference, a second).
const VELOCITY_FOLLOW: f32 = 40.0;
/// A pen's pressure follows by these parts at each point; anything else presses evenly.
const PEN_FOLLOW: f32 = 0.1;
const OTHER_FOLLOW: f32 = 0.2;
/// What keeps going slows by this much in this many seconds, slower still with more momentum.
const SLOWS_BY: f32 = 0.968;
const SLOWS_IN: f32 = 0.008;
/// Momentum (seconds a whole life lasts): none at all is this long, so the sums stay finite.
const LEAST_MOMENTUM: f32 = 0.001;
/// Speeds left under this part of what they were count as stopped.
const STOPPED: f32 = 1e-5;
/// A thrown off piece stops mattering when its life times its speed falls under this.
const LEAST_REACH: f32 = 50.0;
/// The most movers at once: the oldest go first.
const MOST_MOVERS: usize = 2048;

/// The radius of a brush at `size` (0..1).
pub(crate) fn radius(size: f32) -> f32 {
    4.0 + 1596.0 * size.clamp(0.0, 1.0).powf(2.2)
}

/// How far a Liquify brush at `size` (0..1) reaches, canvas pixels: for showing it.
#[uniffi::export]
pub fn liquify_reach(size: f32) -> f32 {
    radius(size)
}

/// How hard a brush at the `pressure` slider (0..1) presses, at full force.
fn strength(pressure: f32) -> f32 {
    0.01 + 0.99 * pressure.clamp(0.0, 1.0)
}

/// Whether `kind` works for as long as it is down, and so held still as well.
pub(crate) fn timed(kind: LiquifyKind) -> bool {
    !matches!(kind, LiquifyKind::Push | LiquifyKind::Edge)
}

/// A point of a stroke: where, with how much of the pen's force (0..1), and how long since the last.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Point {
    pub at: Vec2,
    pub force: f32,
    pub seconds: f32,
}

/// Something going on by itself, laying stamps along its way.
#[derive(Clone, Copy, Debug)]
struct Mover {
    at: Vec2,
    velocity: Vec2,
    /// What is left of it, 1 at first for the brush's own momentum.
    life: f32,
    /// How hard it presses at full life.
    press: f32,
    /// Its own radius; none: the brush's.
    radius: Option<f32>,
    /// How much of its life goes in a second; a brush's momentum goes faster for less of it.
    fading: f32,
    /// How many times faster than usual it slows.
    slowing: f32,
    /// The brush it stands for, or a bare push (`None`).
    kind: Option<LiquifyKind>,
}

/// The steps a batch of input makes: each ends once the brush has gone far enough.
struct Steps {
    all: Vec<Vec<LiquifyStamp>>,
    gone: f32,
    reach: f32,
}

impl Steps {
    fn new(radius: f32) -> Steps {
        Steps { all: vec![Vec::new()], gone: 0.0, reach: (radius * STEP_REACH).max(1.0) }
    }

    fn add(&mut self, stamp: LiquifyStamp) {
        if let Some(step) = self.all.last_mut() {
            step.push(stamp);
        }
    }

    /// The brush went on by `moved`.
    fn went(&mut self, moved: f32) {
        self.gone += moved;
        if self.gone >= self.reach && self.all.len() < MOST_STEPS {
            self.gone = 0.0;
            self.all.push(Vec::new());
        }
    }

    fn done(mut self) -> Vec<Vec<LiquifyStamp>> {
        self.all.retain(|step| !step.is_empty());
        self.all
    }
}

/// The brush as it goes: where it was, what it has set going.
#[derive(Clone, Debug, Default)]
pub(crate) struct LiquifyRun {
    last: Option<(Vec2, f32)>,
    /// How the force has been following the pen.
    force: Option<f32>,
    /// The speed it would lift at (pixels a second).
    velocity: Vec2,
    /// Whether it moved since time last passed.
    moved: bool,
    /// How long its moves have worked since it was last held, so a shaking pen, moving and held at
    /// once, isn't counted twice.
    worked: f32,
    movers: Vec<Mover>,
    /// Seconds since it began, for what turns as it goes.
    clock: f32,
    random: u32,
}

impl LiquifyRun {
    /// The next random number, 0..1.
    fn unit(&mut self) -> f32 {
        // xorshift, begun from anything but zero
        let mut x = if self.random == 0 { 0x9e37_79b9 } else { self.random };
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    /// The next random number, -1..1.
    fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    /// A new stroke begins: what is still going goes on.
    pub(crate) fn begin(&mut self) {
        self.last = None;
        self.force = None;
        self.velocity = Vec2::new(0.0, 0.0);
        self.moved = false;
    }

    /// Whether anything is still going by itself.
    pub(crate) fn going(&self) -> bool {
        !self.movers.is_empty()
    }

    /// The force at the next point, `raw` the pen's (0..1): it follows, a pen's slower.
    fn followed(&mut self, raw: f32, pen: bool) -> f32 {
        let follow = if pen { PEN_FOLLOW } else { OTHER_FOLLOW };
        let force = self.force.map_or(raw, |f| f + (raw - f) * follow);
        self.force = Some(force);
        force
    }

    /// The brush goes along `points`, `pen` saying whose: the steps that makes.
    pub(crate) fn along(&mut self, points: &[Point], pen: bool, brush: LiquifyBrush) -> Vec<Vec<LiquifyStamp>> {
        let mut steps = Steps::new(radius(brush.size));
        let start = self.last.map(|l| l.0).or(points.first().map(|p| p.at));
        let mut seconds = 0.0;
        for point in points {
            let force = self.followed(point.force, pen);
            let to = (point.at, force);
            let Some(from) = self.last.replace(to) else { continue };
            let took = point.seconds.min(LONGEST_MOVE);
            seconds += took;
            self.worked += took;
            self.clock += took;
            self.lay(from, to, took, brush, &mut steps);
        }
        if let (Some(start), Some((end, _))) = (start, self.last)
            && seconds > 0.0
        {
            let now = (end - start) * (1.0 / seconds);
            self.velocity = self.velocity + (now - self.velocity) * (seconds * VELOCITY_FOLLOW).min(1.0);
            self.moved = true;
        }
        steps.done()
    }

    /// The brush held still at `at` works on for `seconds`.
    pub(crate) fn held(&mut self, at: Vec2, seconds: f32, brush: LiquifyBrush) -> Vec<Vec<LiquifyStamp>> {
        let mut steps = Steps::new(radius(brush.size));
        let force = self.force.unwrap_or(1.0);
        let seconds = (seconds - std::mem::take(&mut self.worked)).max(0.0);
        if seconds > 0.0 {
            self.clock += seconds;
            self.lay((at, force), (at, force), seconds, brush, &mut steps);
        }
        steps.done()
    }

    /// The brush lifts: with `momentum` (0..1) it carries on as it was going.
    pub(crate) fn lift(&mut self, momentum: f32, brush: LiquifyBrush) {
        let Some((at, force)) = self.last.take() else { return };
        if momentum > 0.0 && self.velocity.length() > 0.0 {
            let held = 1.0 / (momentum + LEAST_MOMENTUM);
            self.movers.push(Mover {
                at,
                velocity: self.velocity,
                life: 1.0,
                press: strength(brush.pressure) * force,
                radius: None,
                fading: held,
                slowing: held,
                kind: Some(brush.kind),
            });
        }
    }

    /// `seconds` pass: what is going goes on.
    pub(crate) fn tick(&mut self, seconds: f32, brush: LiquifyBrush) -> Vec<Vec<LiquifyStamp>> {
        if !std::mem::take(&mut self.moved) {
            let velocity = self.velocity;
            self.velocity = velocity + (velocity * -1.0) * (seconds * VELOCITY_FOLLOW).min(1.0);
        }
        self.clock += seconds;
        let mut steps = Steps::new(radius(brush.size));
        let mut movers = std::mem::take(&mut self.movers);
        for m in &mut movers {
            let from = m.at;
            m.at = m.at + m.velocity * seconds;
            m.life = (m.life - m.fading * seconds).max(0.0);
            let kept = SLOWS_BY.powf(seconds / SLOWS_IN * m.slowing);
            m.velocity = m.velocity * if kept < STOPPED { 0.0 } else { kept };
            let wide = m.radius.unwrap_or_else(|| radius(brush.size));
            let press = m.press * m.life;
            match m.kind {
                Some(kind) => {
                    let brush = LiquifyBrush { kind, ..brush };
                    self.lay_pressed((from, press), (m.at, press), seconds, brush, &mut steps);
                }
                None => bare_push(from, m.at, wide, press, &mut steps),
            }
        }
        // a thrown off piece is gone once it can't reach anywhere
        let going = |m: &Mover| m.life > 0.0 && (m.kind.is_some() || m.life * m.velocity.length() >= LEAST_REACH);
        movers.retain(going);
        // pieces thrown off meanwhile came after these
        movers.append(&mut self.movers);
        self.movers = movers;
        self.keep_to_the_most();
        steps.done()
    }

    fn keep_to_the_most(&mut self) {
        let over = self.movers.len().saturating_sub(MOST_MOVERS);
        self.movers.drain(..over);
    }

    /// The stamps from `from` to `to` (each a place and a force), `seconds` apart.
    fn lay(&mut self, from: (Vec2, f32), to: (Vec2, f32), seconds: f32, brush: LiquifyBrush, steps: &mut Steps) {
        let press = strength(brush.pressure);
        self.lay_pressed((from.0, from.1 * press), (to.0, to.1 * press), seconds, brush, steps);
    }

    /// The same, each with how hard it presses as a whole.
    fn lay_pressed(
        &mut self,
        from: (Vec2, f32),
        to: (Vec2, f32),
        seconds: f32,
        brush: LiquifyBrush,
        steps: &mut Steps,
    ) {
        let wide = radius(brush.size);
        let moved = to.0 - from.0;
        let length = moved.length();
        let way = if length > 0.0 { moved * (1.0 / length) } else { Vec2::new(1.0, 0.0) };
        if timed(brush.kind) || length > 0.0 {
            let count = stamps_for(length, wide);
            let share = 1.0 / count as f32;
            let each =
                if timed(brush.kind) { (seconds * share).max(length * share * WORK_PER_PIXEL) } else { length * share };
            for k in 1..=count {
                let t = k as f32 * share;
                let at = from.0 + moved * t;
                let press = from.1 + (to.1 - from.1) * t;
                self.place(at, way, wide, press * each, brush, steps);
                steps.went(length * share);
            }
        }
        match brush.kind {
            LiquifyKind::Push => self.wake(from.0, to.0, to.1, brush),
            LiquifyKind::Crystals => self.crystals(from.0, to.0, to.1, brush),
            _ => {}
        }
    }

    /// A stamp of `brush` of `amount`, with what its distortion adds around it.
    fn place(&mut self, at: Vec2, way: Vec2, wide: f32, amount: f32, brush: LiquifyBrush, steps: &mut Steps) {
        let distortion = brush.distortion.clamp(0.0, 1.0);
        match brush.kind {
            LiquifyKind::Push => steps.add(stamp(at, way, wide, amount, LiquifyMode::Push)),
            LiquifyKind::Edge => steps.add(stamp(at, way, wide, amount, LiquifyMode::Edge)),
            LiquifyKind::Reconstruct => steps.add(stamp(at, way, wide, amount, LiquifyMode::Reconstruct)),
            LiquifyKind::TwirlRight | LiquifyKind::TwirlLeft => {
                self.eddies(at, wide, amount, brush.kind == LiquifyKind::TwirlRight, distortion, steps);
            }
            LiquifyKind::Pinch | LiquifyKind::Expand => {
                self.lobes(at, wide, amount, brush.kind == LiquifyKind::Pinch, distortion, steps);
            }
            // all it does, the pieces it throws off do
            LiquifyKind::Crystals => {}
        }
    }

    /// A twirl: with distortion it shrinks and small ones turning the other way circle it, and
    /// past half way smaller ones circle those.
    fn eddies(&self, at: Vec2, wide: f32, amount: f32, right: bool, distortion: f32, steps: &mut Steps) {
        let (mode, other) = if right {
            (LiquifyMode::TwirlRight, LiquifyMode::TwirlLeft)
        } else {
            (LiquifyMode::TwirlLeft, LiquifyMode::TwirlRight)
        };
        let some = (distortion * 2.0).min(1.0);
        let more = (distortion * 2.0 - 1.0).clamp(0.0, 1.0);
        let inner = wide * 0.46;
        let main = inner * some + wide * (1.0 - some);
        steps.add(stamp(at, Vec2::new(1.0, 0.0), main, amount, mode));
        if some <= 0.0 {
            return;
        }
        let small = inner * 0.66;
        let turn = self.clock * 0.25;
        let round = if right { -1.0 } else { 1.0 };
        for k in 0..5 {
            let a = round * (turn + TAU * k as f32 / 5.0);
            let centre = at + Vec2::new(a.cos(), a.sin()) * (inner * 1.3);
            let size = small * more + inner * 0.75 * (1.0 - more);
            steps.add(stamp(centre, Vec2::new(1.0, 0.0), size, amount * some * 2.0, other));
            if more <= 0.0 {
                continue;
            }
            for j in 0..3 {
                let b = -round * (turn + TAU * j as f32 / 3.0);
                let round_it = centre + Vec2::new(b.cos(), b.sin()) * (small * 0.75);
                steps.add(stamp(round_it, Vec2::new(1.0, 0.0), small * 0.66, amount * more * 3.0, mode));
            }
        }
    }

    /// A pinch or expand: with distortion part of it goes into lobes turning slowly about it, so
    /// the shape comes out starred.
    fn lobes(&self, at: Vec2, wide: f32, amount: f32, pinch: bool, distortion: f32, steps: &mut Steps) {
        let mode = if pinch { LiquifyMode::Pinch } else { LiquifyMode::Expand };
        steps.add(stamp(at, Vec2::new(1.0, 0.0), wide, amount * (1.0 - distortion), mode));
        if distortion <= 0.0 {
            return;
        }
        let way = if pinch { 1.0 } else { -1.0 };
        for (ring, (count, weight, narrow)) in [(3, 0.4, 0.75), (6, 0.85, 0.95)].into_iter().enumerate() {
            let spin = if ring == 0 { self.clock } else { -self.clock };
            for k in 0..count {
                let part = k as f32 / count as f32;
                let a = spin * 0.5 + part * TAU;
                let beat = (ring as f32 * 3.19 + 7.41 * part + self.clock * PI * 2.0).sin();
                let size = (0.2 * (beat * 0.5 + 0.5) + 0.75) * weight;
                // a little off the centre, wandering
                let drift = Vec2::new((a * 1.7 + self.clock).sin(), (a * 2.3 - self.clock).cos()) * (wide * 0.1);
                let lobe = LiquifyMode::Lobe { narrow };
                let press = way * amount * distortion * size * 2.0;
                steps.add(stamp(at + drift, Vec2::new(a.cos(), a.sin()), wide, press, lobe));
            }
        }
    }

    /// A piece thrown off: a small push going its own way.
    fn throw(&mut self, at: Vec2, velocity: Vec2, life: f32, press: f32, radius: f32) {
        self.movers.push(Mover {
            at,
            velocity,
            life,
            press,
            radius: Some(radius),
            fading: 1.0,
            slowing: 1.0,
            kind: None,
        });
    }

    /// A pushing brush's distortion: pieces thrown off to both sides of its way, stirring what it
    /// leaves behind.
    fn wake(&mut self, from: Vec2, to: Vec2, press: f32, brush: LiquifyBrush) {
        let distortion = brush.distortion.clamp(0.0, 1.0);
        let moved = to - from;
        let length = moved.length();
        if distortion <= 0.0 || length <= 0.0 {
            return;
        }
        const SPREAD: f32 = 0.45;
        let wide = radius(brush.size);
        let gap = wide / 16.0;
        let way = moved * (1.0 / length);
        let mut along = 0.0;
        while along < length {
            let left = (length - along).clamp(1.0, gap);
            if left > gap * 0.25 {
                let speed = wide * (self.signed() + 3.0);
                let at = from + moved * (along / length) + turned(way, FRAC_PI_2) * (wide * 0.25 * self.signed());
                for side in [-FRAC_PI_2, FRAC_PI_2] {
                    let velocity = turned(way, side + self.signed() * SPREAD * 1.5) * speed;
                    let life = distortion.sqrt() * (1.0 - SPREAD * self.unit());
                    let press = press * 0.35 * (1.0 - SPREAD * self.signed());
                    let radius = left * 4.0 * (1.0 + SPREAD * self.signed());
                    self.throw(at, velocity, life, press, radius);
                }
            }
            along += gap;
        }
        self.keep_to_the_most();
    }

    /// Crystals: small sharp pieces thrown off, to both sides of the way the brush goes, or every
    /// way where it stays. More distortion makes them wider and fewer.
    fn crystals(&mut self, from: Vec2, to: Vec2, press: f32, brush: LiquifyBrush) {
        let plain = 1.0 - brush.distortion.clamp(0.0, 1.0);
        let wide = radius(brush.size);
        let moved = to - from;
        let length = moved.length();
        let piece = (0.4 - 0.3676 * plain) * wide / 16.0;
        let count = plain * press * 3.0 + 1.0;
        let by_way = (count * (length / piece.max(4.0)) * 0.5) as usize;
        let at_least = count as usize;
        let pieces = at_least.max(by_way);
        let hard = press * (0.64 + 2.86 * plain.powi(5));
        let way = if length > 0.0 { moved * (1.0 / length) } else { Vec2::new(1.0, 0.0) };
        for i in 0..pieces {
            let speed = wide * (self.signed() + 3.0);
            let at = from + moved * ((i + 1) as f32 / pieces as f32);
            for side in [FRAC_PI_2, -FRAC_PI_2] {
                let a = if by_way < at_least { self.signed() * PI } else { side + 0.675 * self.signed() };
                let velocity = turned(way, a) * speed;
                let life = (1.0 - self.signed() * 0.25) * 1.5;
                let press = hard * (1.0 - self.signed() * 0.25);
                let radius = piece * (self.signed() * 0.25 + 1.0);
                self.throw(at, velocity, life, press, radius);
            }
        }
        self.keep_to_the_most();
    }
}

/// `v` turned `a` radians.
fn turned(v: Vec2, a: f32) -> Vec2 {
    let (sin, cos) = a.sin_cos();
    Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
}

fn stamp(at: Vec2, way: Vec2, radius: f32, amount: f32, mode: LiquifyMode) -> LiquifyStamp {
    LiquifyStamp { centre: [at.x, at.y], way: [way.x, way.y], radius, amount, mode }
}

/// How many stamps a move of `length` takes with a brush of `radius`: one at least.
fn stamps_for(length: f32, radius: f32) -> usize {
    ((length / (radius * STAMP_GAP).max(LEAST_GAP)) as usize).max(1)
}

/// A plain push from `from` to `to`.
fn bare_push(from: Vec2, to: Vec2, radius: f32, press: f32, steps: &mut Steps) {
    let moved = to - from;
    let length = moved.length();
    if length <= 0.0 {
        return;
    }
    let way = moved * (1.0 / length);
    let count = stamps_for(length, radius);
    let share = 1.0 / count as f32;
    for k in 1..=count {
        let at = from + moved * (k as f32 * share);
        steps.add(stamp(at, way, radius, press * length * share, LiquifyMode::Push));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brush(kind: LiquifyKind, distortion: f32) -> LiquifyBrush {
        LiquifyBrush { kind, size: 0.5, pressure: 1.0, distortion }
    }

    fn at(x: f32) -> Point {
        Point { at: Vec2::new(x, 100.0), force: 1.0, seconds: 0.01 }
    }

    fn total(steps: &[Vec<LiquifyStamp>]) -> f32 {
        steps.iter().flatten().map(|s| s.amount).sum()
    }

    #[test]
    fn a_push_moves_as_far_as_the_brush_however_finely_it_is_read() {
        for (points, gap) in [(201, 1.0), (21, 10.0), (5, 50.0)] {
            let mut run = LiquifyRun::default();
            let path: Vec<Point> = (0..points).map(|i| at(i as f32 * gap)).collect();
            let steps = run.along(&path, false, brush(LiquifyKind::Push, 0.0));
            assert!((total(&steps) - 200.0).abs() < 0.01, "{points} points: {}", total(&steps));
        }
    }

    #[test]
    fn a_step_goes_only_so_far_so_a_long_move_is_carried_along_as_a_short_one_is() {
        let mut run = LiquifyRun::default();
        let wide = radius(0.5);
        let path = [at(0.0), at(wide * 0.5)];
        let steps = run.along(&path, false, brush(LiquifyKind::Push, 0.0));
        assert!(steps.len() >= 5, "{} steps", steps.len());
    }

    #[test]
    fn a_timed_brush_works_for_its_time_whether_it_moves_or_not() {
        let kind = LiquifyKind::TwirlRight;
        let mut run = LiquifyRun::default();
        let path: Vec<Point> = (0..=100).map(|i| at(i as f32)).collect();
        let moving = total(&run.along(&path, false, brush(kind, 0.0)));
        assert!((moving - 1.0).abs() < 0.01, "a second moving: {moving}");
        let mut run = LiquifyRun::default();
        run.along(&[at(0.0)], false, brush(kind, 0.0));
        let held: f32 = (0..100).map(|_| total(&run.held(Vec2::new(0.0, 100.0), 0.01, brush(kind, 0.0)))).sum();
        assert!((held - 1.0).abs() < 0.01, "a second held: {held}");
    }

    #[test]
    fn a_timed_brush_counts_its_time_once_however_it_is_moved_and_held() {
        let kind = LiquifyKind::TwirlLeft;
        // stopped for two seconds, it moves on: the stop was worked by being held, not by this
        let mut run = LiquifyRun::default();
        run.along(&[at(0.0)], false, brush(kind, 0.0));
        let after_a_stop = Point { seconds: 2.0, ..at(1.0) };
        assert!(total(&run.along(&[after_a_stop], false, brush(kind, 0.0))) <= 0.05 + 1e-4);
        // a pen shaking while held: its moves and its being held add up to the time, not twice it
        let mut run = LiquifyRun::default();
        run.along(&[at(0.0)], false, brush(kind, 0.0));
        let mut done = 0.0;
        for i in 0..100 {
            done += total(&run.along(&[at((i % 2) as f32)], false, brush(kind, 0.0)));
            done += total(&run.held(Vec2::new(0.0, 100.0), 0.01, brush(kind, 0.0)));
        }
        assert!((done - 1.0).abs() < 0.02, "a second: {done}");
    }

    #[test]
    fn only_a_very_fast_timed_brush_gets_more_done_by_its_speed() {
        let kind = LiquifyKind::Pinch;
        let path = |gap: f32| (0..=10).map(|i| at(i as f32 * gap)).collect::<Vec<_>>();
        // 1000 a second: its time counts
        let slow = total(&LiquifyRun::default().along(&path(10.0), false, brush(kind, 0.0)));
        assert!((slow - 0.1).abs() < 0.001, "{slow}");
        // 6000 a second: twice its time
        let fast = total(&LiquifyRun::default().along(&path(60.0), false, brush(kind, 0.0)));
        assert!((fast - 0.2).abs() < 0.001, "{fast}");
    }

    #[test]
    fn a_pen_presses_smoothly_and_a_slider_scales_it() {
        let mut run = LiquifyRun::default();
        let soft = LiquifyBrush { pressure: 0.0, ..brush(LiquifyKind::Push, 0.0) };
        let path = [at(0.0), at(100.0)];
        assert!((total(&run.along(&path, false, soft)) - 1.0).abs() < 0.001, "the least slider is a hundredth");
        let mut run = LiquifyRun::default();
        let first = Point { force: 1.0, ..at(0.0) };
        let second = Point { force: 0.0, ..at(100.0) };
        // from all of it, a pen follows a tenth of the way
        let done = total(&run.along(&[first, second], true, brush(LiquifyKind::Push, 0.0)));
        assert!(done > 90.0 && done < 100.0, "{done}");
    }

    #[test]
    fn momentum_carries_on_slowing_and_stops_without_it() {
        let kind = LiquifyKind::Push;
        let going = |momentum: f32| {
            let mut run = LiquifyRun::default();
            let path: Vec<Point> = (0..=10).map(|i| at(i as f32 * 10.0)).collect();
            run.along(&path, false, brush(kind, 0.0));
            run.lift(momentum, brush(kind, 0.0));
            let mut done = 0.0;
            let mut ticks = 0;
            while run.going() && ticks < 1000 {
                done += total(&run.tick(1.0 / 60.0, brush(kind, 0.0)));
                ticks += 1;
            }
            (done, ticks)
        };
        assert_eq!(going(0.0), (0.0, 0));
        let (some, ticks) = going(0.5);
        let (more, longer) = going(1.0);
        assert!(some > 0.0 && more > some, "{some} then {more}");
        assert!(longer > ticks, "{ticks} then {longer} ticks");
        // a second at most at full momentum
        assert!(longer <= 62, "{longer} ticks");
    }

    #[test]
    fn distortion_throws_pieces_off_a_push_only_when_it_is_on() {
        let path: Vec<Point> = (0..=10).map(|i| at(i as f32 * 10.0)).collect();
        let mut plain = LiquifyRun::default();
        plain.along(&path, false, brush(LiquifyKind::Push, 0.0));
        assert!(!plain.going());
        let mut stirred = LiquifyRun::default();
        stirred.along(&path, false, brush(LiquifyKind::Push, 0.6));
        assert!(stirred.going());
        // they go to both sides of the way, and in time they stop
        let sides: Vec<f32> = stirred.movers.iter().map(|m| m.velocity.y).collect();
        assert!(sides.iter().any(|v| *v > 0.0) && sides.iter().any(|v| *v < 0.0));
        for _ in 0..200 {
            stirred.tick(1.0 / 60.0, brush(LiquifyKind::Push, 0.6));
        }
        assert!(!stirred.going(), "{} still going", stirred.movers.len());
    }

    #[test]
    fn distortion_splits_a_twirl_into_eddies_and_keeps_a_pinch_s_total() {
        let plain = LiquifyRun::default();
        let mut steps = Steps::new(100.0);
        plain.eddies(Vec2::new(0.0, 0.0), 100.0, 1.0, true, 0.0, &mut steps);
        assert_eq!(steps.done().iter().flatten().count(), 1);
        let mut steps = Steps::new(100.0);
        plain.eddies(Vec2::new(0.0, 0.0), 100.0, 1.0, true, 0.4, &mut steps);
        let stamps = steps.done().concat();
        assert_eq!(stamps.len(), 6);
        assert!(stamps[1..].iter().all(|s| s.mode == LiquifyMode::TwirlLeft));
        let mut steps = Steps::new(100.0);
        plain.eddies(Vec2::new(0.0, 0.0), 100.0, 1.0, true, 1.0, &mut steps);
        assert_eq!(steps.done().concat().len(), 21);
        let mut steps = Steps::new(100.0);
        plain.lobes(Vec2::new(0.0, 0.0), 100.0, 1.0, true, 1.0, &mut steps);
        let stamps = steps.done().concat();
        assert_eq!(stamps.len(), 10);
        assert_eq!(stamps[0].amount, 0.0);
    }

    #[test]
    fn crystals_throw_pieces_even_held_still_and_nothing_else_of_theirs_moves() {
        let mut run = LiquifyRun::default();
        run.along(&[at(0.0)], false, brush(LiquifyKind::Crystals, 0.0));
        let steps = run.held(Vec2::new(0.0, 100.0), 0.02, brush(LiquifyKind::Crystals, 0.0));
        assert!(steps.is_empty());
        assert!(run.going());
        let steps = run.tick(1.0 / 60.0, brush(LiquifyKind::Crystals, 0.0));
        assert!(steps.iter().flatten().all(|s| s.mode == LiquifyMode::Push && s.radius < 10.0), "{steps:?}");
    }

    #[test]
    fn each_way_keeps_its_own_distortion_and_momentum_starts_at_none() {
        let s = crate::CanvasSession::new(64, 64);
        assert_eq!(s.adjustment_shown().liquify, vec![0.5, 1.0, 0.0, 0.0]);
        s.adjustment_liquify_kind(LiquifyKind::TwirlRight);
        s.adjustment_liquify_value(2, 0.59);
        s.adjustment_liquify_value(0, 0.3);
        s.adjustment_liquify_kind(LiquifyKind::Pinch);
        assert_eq!(s.adjustment_shown().liquify, vec![0.3, 1.0, 0.0, 0.0], "the size is shared, the distortion isn't");
        s.adjustment_liquify_kind(LiquifyKind::TwirlRight);
        assert_eq!(s.adjustment_shown().liquify[2], 0.59);
    }

    #[test]
    fn only_a_push_and_an_edge_need_to_move() {
        for kind in [LiquifyKind::Push, LiquifyKind::Edge] {
            let mut run = LiquifyRun::default();
            run.along(&[at(0.0)], false, brush(kind, 0.0));
            assert!(run.held(Vec2::new(0.0, 100.0), 0.1, brush(kind, 0.0)).is_empty());
        }
    }
}
