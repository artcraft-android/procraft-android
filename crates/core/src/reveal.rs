//! A dropped colour's fill spreading in (ColorDrop): the canvas as it was stays over the fill but
//! for a hole that grows from where the colour landed until it is past everything the fill
//! reached. The fill itself is made at once (its undo step too); this only veils it while the hole
//! grows. Not with Reduce Motion, nor in Continue filling (only a dragged colour spreads).

use std::f32::consts::PI;
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use procraft_engine::geom::{IRect, Vec2};
use procraft_engine::view::View;

/// The hole at first: a circle this many points round the point the colour landed on.
const START: f32 = 10.0;
/// At last: an ellipse round the middle of the fill's box on the screen, its half-axes this many
/// times the box's sides (points, rounded up).
const END: f32 = 1.414;
/// How long it takes: this long, and this much more the more of the view the fill's box takes up
/// (all of it or more: the whole of this).
const SHORTEST: f32 = 0.2;
const LONGER: f32 = 0.3;
/// The hole's six corners, a sixth of a turn apart from the right; their handles reach this much
/// of a corner's distance from the middle.
const HANDLE: f32 = 0.35;
/// At last its corners are shaken: each moved by up to a tenth of the half-axes, its handles
/// turned up to an eighth of a half turn and made from 0.7 to 1.3 times as long.
const SHAKE_MOVE: f32 = 0.1;
const SHAKE_TURN: f32 = PI / 8.0;
const SHAKE_LONGER: (f32, f32) = (0.7, 1.3);
/// Lines each of the hole's six curves is drawn with (the veil's shader takes 64 points).
const CURVE_STEPS: usize = 10;

/// One of the hole's corners: where it is and its handles, back towards the corner before and on
/// towards the next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Corner {
    at: Vec2,
    back: Vec2,
    on: Vec2,
}

/// A small random source for the shake (xorshift), begun from anything but nought.
pub(crate) struct Shake(u64);

impl Shake {
    pub(crate) fn new(seed: u64) -> Shake {
        Shake(seed.max(1))
    }

    /// Begun from the clock.
    pub(crate) fn from_clock() -> Shake {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos());
        Shake::new(nanos as u64 ^ 0x9E37_79B9_7F4A_7C15)
    }

    /// Somewhere from `low` to `high`.
    fn between(&mut self, low: f32, high: f32) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        low + (high - low) * ((x >> 40) as f32 / (1u64 << 24) as f32)
    }
}

/// The hole's corners round `middle` with half-axes `half` (across, down), shaken by `shake` if
/// given: all in points.
pub(crate) fn hole(middle: Vec2, half: Vec2, mut shake: Option<&mut Shake>) -> [Corner; 6] {
    let mut corners = [Corner { at: middle, back: middle, on: middle }; 6];
    for (i, corner) in corners.iter_mut().enumerate() {
        let a = i as f32 * PI / 3.0;
        let (moved, turned, longer) = match shake.as_deref_mut() {
            Some(s) => {
                let moved = Vec2::new(
                    s.between(-SHAKE_MOVE * half.x, SHAKE_MOVE * half.x),
                    s.between(-SHAKE_MOVE * half.y, SHAKE_MOVE * half.y),
                );
                (moved, s.between(-SHAKE_TURN, SHAKE_TURN), s.between(SHAKE_LONGER.0, SHAKE_LONGER.1))
            }
            None => (Vec2::ZERO, 0.0, 1.0),
        };
        let at = middle + Vec2::new(a.cos() * half.x, a.sin() * half.y) + moved;
        // the handles lie along the way round, back a quarter turn from the corner's angle
        let tangent = a - PI / 2.0 + turned;
        let reach = (middle - at).length() * HANDLE * longer;
        let handle = Vec2::new(tangent.cos(), tangent.sin()) * reach;
        *corner = Corner { at, back: at + handle, on: at - handle };
    }
    corners
}

/// The closed outline of `corners`, each curve from one corner to the next drawn as lines.
pub(crate) fn outline(corners: &[Corner; 6]) -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(6 * CURVE_STEPS);
    for i in 0..6 {
        let (Some(from), Some(to)) = (corners.get((i + 5) % 6), corners.get(i)) else { continue };
        for k in 1..=CURVE_STEPS {
            let t = k as f32 / CURVE_STEPS as f32;
            let u = 1.0 - t;
            let p =
                from.at * (u * u * u) + from.on * (3.0 * u * u * t) + to.back * (3.0 * u * t * t) + to.at * (t * t * t);
            out.push([p.x, p.y]);
        }
    }
    out
}

/// How far along it is at `t` (0..1 of its time): slow to start, quickest at the end (a cubic
/// timing curve through (0.75, 0) and (1, 1)).
pub(crate) fn eased(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let (x1, y1, x2, y2) = (0.75, 0.0, 1.0, 1.0);
    let at = |u: f32, p1: f32, p2: f32| {
        let v = 1.0 - u;
        3.0 * v * v * u * p1 + 3.0 * v * u * u * p2 + u * u * u
    };
    // the curve's own parameter where it is `t` across, by halving
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    for _ in 0..30 {
        let mid = (low + high) / 2.0;
        if at(mid, x1, x2) < t {
            low = mid;
        } else {
            high = mid;
        }
    }
    at((low + high) / 2.0, y1, y2)
}

/// How long the hole takes to grow (seconds) for a fill whose box on the screen takes up `share`
/// of the view.
pub(crate) fn seconds_for(share: f32) -> f32 {
    let share = if share.is_finite() { share.clamp(0.0, 1.0) } else { 0.0 };
    SHORTEST + LONGER * share
}

/// The hole growing over the fill just made.
pub(crate) struct Reveal {
    began: Instant,
    seconds: f32,
    /// Its corners at first and at last, canvas pixels.
    from: [Corner; 6],
    to: [Corner; 6],
}

impl Reveal {
    /// For a fill over `rect` (canvas pixels) of a colour landed on window pixel `point`, seen
    /// through `view` in a window `window` pixels big, `scale` window pixels to a point; shaken by
    /// `shake`. None if any of it is off (no view, nothing filled).
    pub(crate) fn new(
        point: Vec2,
        rect: IRect,
        view: &View,
        window: (u32, u32),
        scale: f32,
        shake: &mut Shake,
    ) -> Option<Reveal> {
        if rect.is_empty() || scale.is_nan() || scale <= 0.0 {
            return None;
        }
        // the fill's box on the screen, in points, out to whole points
        let to_screen = view.to_screen();
        let corners = [(rect.x0, rect.y0), (rect.x1, rect.y0), (rect.x0, rect.y1), (rect.x1, rect.y1)]
            .map(|(x, y)| to_screen.apply(Vec2::new(x as f32, y as f32)) * (1.0 / scale));
        let (mut low, mut high) = (Vec2::new(f32::MAX, f32::MAX), Vec2::new(f32::MIN, f32::MIN));
        for c in corners {
            low = Vec2::new(low.x.min(c.x), low.y.min(c.y));
            high = Vec2::new(high.x.max(c.x), high.y.max(c.y));
        }
        let (low, high) = (Vec2::new(low.x.floor(), low.y.floor()), Vec2::new(high.x.ceil(), high.y.ceil()));
        let size = high - low;
        let viewed = window.0 as f32 * window.1 as f32 / (scale * scale);
        let seconds = seconds_for(size.x * size.y / viewed);
        let from = hole(point * (1.0 / scale), Vec2::new(START, START), None);
        let half = Vec2::new((size.x * END).ceil(), (size.y * END).ceil());
        let to = hole((low + high) * 0.5, half, Some(shake));
        // points to canvas pixels: the view is a similarity, so the curves map exactly
        let canvas = |c: Corner| -> Option<Corner> {
            let f = |p: Vec2| view.screen_to_canvas(p * scale).filter(|q| q.is_finite());
            Some(Corner { at: f(c.at)?, back: f(c.back)?, on: f(c.on)? })
        };
        let map = |cs: [Corner; 6]| -> Option<[Corner; 6]> {
            let mut out = cs;
            for c in out.iter_mut() {
                *c = canvas(*c)?;
            }
            Some(out)
        };
        Some(Reveal { began: Instant::now(), seconds, from: map(from)?, to: map(to)? })
    }

    /// How long it takes.
    #[cfg(test)]
    fn duration(&self) -> Duration {
        Duration::from_secs_f32(self.seconds)
    }

    /// The hole's outline `seconds` after it began.
    #[cfg(test)]
    pub(crate) fn hole_after(&self, seconds: f32) -> Option<Vec<[f32; 2]>> {
        self.hole_at(self.began + Duration::from_secs_f32(seconds))
    }

    /// The hole's outline at `now` (canvas pixels); none once it has grown.
    pub(crate) fn hole_at(&self, now: Instant) -> Option<Vec<[f32; 2]>> {
        let t = now.saturating_duration_since(self.began).as_secs_f32() / self.seconds.max(1e-3);
        if t >= 1.0 {
            return None;
        }
        let e = eased(t);
        let mix = |a: Vec2, b: Vec2| a + (b - a) * e;
        let mut corners = self.from;
        for (c, (a, b)) in corners.iter_mut().zip(self.from.iter().zip(&self.to)) {
            *c = Corner { at: mix(a.at, b.at), back: mix(a.back, b.back), on: mix(a.on, b.on) };
        }
        Some(outline(&corners))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_takes_a_fifth_of_a_second_and_up_to_three_tenths_more_for_the_whole_view() {
        assert!((seconds_for(0.0) - 0.2).abs() < 1e-6);
        assert!((seconds_for(0.5) - 0.35).abs() < 1e-6);
        assert!((seconds_for(1.0) - 0.5).abs() < 1e-6);
        assert!((seconds_for(4.0) - 0.5).abs() < 1e-6, "a box past the view counts as all of it");
        assert!((seconds_for(f32::NAN) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn it_starts_slow_and_ends_quickest() {
        assert!(eased(0.0).abs() < 1e-6);
        assert!((eased(1.0) - 1.0).abs() < 1e-5);
        // the curve through (0.75, 0) and (1, 1): barely moved half way, quickest at the end
        let half = eased(0.5);
        assert!(half > 0.05 && half < 0.2, "{half}");
        let mut last = 0.0;
        for i in 1..=100 {
            let e = eased(i as f32 / 100.0);
            assert!(e >= last, "never back");
            last = e;
        }
        assert!(eased(1.0) - eased(0.9) > eased(0.1) - eased(0.0));
    }

    #[test]
    fn unshaken_the_hole_is_a_near_circle_through_six_corners() {
        let corners = hole(Vec2::new(100.0, 50.0), Vec2::new(10.0, 10.0), None);
        // the first corner to the right of the middle, then a sixth of a turn round each
        assert!((corners[0].at - Vec2::new(110.0, 50.0)).length() < 1e-4);
        assert!((corners[1].at - Vec2::new(105.0, 50.0 + 8.660_254)).length() < 1e-3);
        for p in outline(&corners) {
            let r = (Vec2::new(p[0], p[1]) - Vec2::new(100.0, 50.0)).length();
            assert!((r - 10.0).abs() < 0.15, "{r}");
        }
        assert_eq!(outline(&corners).len(), 60);
    }

    #[test]
    fn shaken_the_hole_stays_within_its_shake() {
        let mut shake = Shake::new(7);
        let middle = Vec2::new(0.0, 0.0);
        let half = Vec2::new(200.0, 100.0);
        for _ in 0..50 {
            let corners = hole(middle, half, Some(&mut shake));
            for (i, c) in corners.iter().enumerate() {
                let a = i as f32 * PI / 3.0;
                let unshaken = Vec2::new(a.cos() * half.x, a.sin() * half.y);
                let d = c.at - unshaken;
                assert!(d.x.abs() <= 20.0 + 1e-3 && d.y.abs() <= 10.0 + 1e-3, "{d:?}");
                let reach = (c.back - c.at).length();
                let from_middle = c.at.length() * HANDLE;
                assert!(reach >= from_middle * 0.7 - 1e-3 && reach <= from_middle * 1.3 + 1e-3);
            }
        }
    }

    /// The window pixel `p` of `view` on the canvas.
    fn on_canvas(view: &View, p: Vec2) -> Vec2 {
        view.screen_to_canvas(p).unwrap()
    }

    #[test]
    fn it_grows_from_ten_points_round_the_drop_to_past_the_fill_s_box() {
        // a 1000 pixel canvas fitted in an 800 pixel window, two pixels a point
        let view = View::fit((1000, 1000), (800, 800));
        let rect = IRect::new(100, 200, 400, 300);
        let drop = view.to_screen().apply(Vec2::new(250.0, 250.0));
        let r = Reveal::new(drop, rect, &view, (800, 800), 2.0, &mut Shake::new(3)).unwrap();
        let first = r.hole_at(r.began).unwrap();
        // ten points is twenty window pixels: on the canvas, through the view's zoom
        let zoom = view.to_screen().scale_factor();
        let centre = on_canvas(&view, drop);
        for p in &first {
            let d = (Vec2::new(p[0], p[1]) - centre).length() * zoom;
            assert!((d - 20.0).abs() < 0.5, "{d}");
        }
        // near the end every corner of the fill's box is inside the hole
        let last = r.hole_at(r.began + r.duration().mul_f32(0.999)).unwrap();
        let inside = |q: Vec2| {
            let mut crossings = 0;
            for (i, a) in last.iter().enumerate() {
                let b = last[(i + 1) % last.len()];
                if (a[1] > q.y) != (b[1] > q.y) && q.x < a[0] + (q.y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                    crossings += 1;
                }
            }
            crossings % 2 == 1
        };
        for (x, y) in [(100, 200), (400, 200), (100, 300), (400, 300)] {
            assert!(inside(Vec2::new(x as f32, y as f32)), "({x}, {y})");
        }
        // and then it has grown
        assert!(r.hole_at(r.began + r.duration()).is_none());
    }

    #[test]
    fn its_time_goes_by_the_share_of_the_view_the_fill_s_box_takes() {
        let view = View::fit((1000, 1000), (800, 800));
        let drop = Vec2::new(400.0, 400.0);
        let small = Reveal::new(drop, IRect::new(490, 490, 510, 510), &view, (800, 800), 2.0, &mut Shake::new(1));
        let whole = Reveal::new(drop, IRect::new(0, 0, 1000, 1000), &view, (800, 800), 2.0, &mut Shake::new(1));
        let (small, whole) = (small.unwrap().seconds, whole.unwrap().seconds);
        assert!(small < 0.201, "{small}");
        // the fitted canvas is the whole window here
        assert!((whole - 0.5).abs() < 0.01, "{whole}");
    }
}
