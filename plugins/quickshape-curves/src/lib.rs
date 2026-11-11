//! QuickShape curves: a stroke that bends more than once, held, snaps to a curve through its bends
//! rather than a polyline, its points there to reshape it. The app hands over the stroke's points
//! and what QuickShape found first; this finds the bends and lays a chain of cubics through them,
//! each anchor's controls leaning the way the stroke came and went so the joins are smooth. The app
//! draws the curve and lets it be reshaped. It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The interface is written down at the top of `crates/core/src/plugin_run.rs`.

use std::sync::Mutex;

/// A stroke's bend is a corner when the path strays this far (as a part of the stroke's size) from
/// the straight line between its neighbours.
const CORNER_TOLERANCE: f32 = 0.08;
/// QuickShape's kinds as the app numbers them: the polyline, and a curve asked for by name.
const POLYLINE: i32 = 1;
const ASKED: i32 = -1;

struct Job {
    best: i32,
    /// The stroke's points as they come in (x, y), and the curve's nodes as they go out.
    points: Vec<f32>,
    nodes: Vec<f32>,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);

/// A stroke of `count` points snapping, QuickShape having found `best` first: where the points go.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_curve_begin(count: i32, best: i32) -> i32 {
    let mut guard = JOB.lock().unwrap_or_else(|e| e.into_inner());
    let job = guard.insert(Job { best, points: vec![0.0; count.max(0) as usize * 2], nodes: Vec::new() });
    job.points.as_mut_ptr() as i32
}

/// The curve, if there is one to offer: how many nodes it has (0: none). A stroke QuickShape takes
/// for a polyline bends more than once, and gets a curve; one asked for by name gets one whatever.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_curve_run() -> i32 {
    let mut guard = JOB.lock().unwrap_or_else(|e| e.into_inner());
    let Some(job) = guard.as_mut() else { return 0 };
    if job.best != POLYLINE && job.best != ASKED {
        return 0;
    }
    let points: Vec<(f32, f32)> = job.points.chunks_exact(2).map(|p| (p[0], p[1])).collect();
    let nodes = fit(&points);
    job.nodes = nodes.iter().flat_map(|&(x, y)| [x, y]).collect();
    if nodes.len() >= 4 { nodes.len() as i32 } else { 0 }
}

/// Where the curve's nodes are (x, y each).
#[unsafe(no_mangle)]
pub extern "C" fn procraft_curve_nodes() -> i32 {
    let mut guard = JOB.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_mut().map_or(0, |job| job.nodes.as_mut_ptr() as i32)
}

type P = (f32, f32);

fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}

fn add(a: P, b: P) -> P {
    (a.0 + b.0, a.1 + b.1)
}

fn scale(a: P, k: f32) -> P {
    (a.0 * k, a.1 * k)
}

fn length(a: P) -> f32 {
    a.0.hypot(a.1)
}

/// How far `p` is from the segment `a`–`b`.
fn distance_to_segment(p: P, a: P, b: P) -> f32 {
    let ab = sub(b, a);
    let len2 = ab.0 * ab.0 + ab.1 * ab.1;
    if len2 <= 1e-9 {
        return length(sub(p, a));
    }
    let ap = sub(p, a);
    let t = ((ap.0 * ab.0 + ap.1 * ab.1) / len2).clamp(0.0, 1.0);
    length(sub(p, add(a, scale(ab, t))))
}

/// How big the stroke is: its box's diagonal.
fn size(pts: &[P]) -> f32 {
    let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
    for &(x, y) in pts {
        (lo.0, lo.1, hi.0, hi.1) = (lo.0.min(x), lo.1.min(y), hi.0.max(x), hi.1.max(y));
    }
    if pts.is_empty() { 1.0 } else { length(sub(hi, lo)).max(1.0) }
}

/// The corners that keep the path within `tolerance` (Ramer–Douglas–Peucker), ends included.
fn simplify(pts: &[P], tolerance: f32) -> Vec<P> {
    fn keep(pts: &[P], tolerance: f32, out: &mut Vec<P>) {
        let (Some(&a), Some(&b)) = (pts.first(), pts.last()) else { return };
        let far = pts
            .iter()
            .enumerate()
            .map(|(i, &p)| (i, distance_to_segment(p, a, b)))
            .fold((0, 0.0f32), |best, x| if x.1 > best.1 { x } else { best });
        if far.1 > tolerance && far.0 > 0 && far.0 + 1 < pts.len() {
            keep(&pts[..=far.0], tolerance, out);
            out.pop();
            keep(&pts[far.0..], tolerance, out);
        } else {
            out.push(a);
            out.push(b);
        }
    }
    let mut out = Vec::new();
    keep(pts, tolerance, &mut out);
    out.dedup();
    out
}

/// The curve through the stroke's bends: an anchor at each corner, and between each two the two
/// controls of a cubic, leaning the way the stroke came and went (a sixth of the way to the
/// neighbours either side), so the joins are smooth.
fn fit(pts: &[P]) -> Vec<P> {
    let pts: Vec<P> = pts.iter().copied().filter(|p| p.0.is_finite() && p.1.is_finite()).collect();
    let mut anchors = simplify(&pts, CORNER_TOLERANCE * size(&pts));
    if anchors.len() < 2 {
        let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { return Vec::new() };
        anchors = vec![first, last];
    }
    let n = anchors.len();
    let mut nodes = Vec::with_capacity(n * 3);
    for i in 0..n {
        let here = anchors[i];
        let before = if i == 0 { here } else { anchors[i - 1] };
        let after = if i + 1 == n { here } else { anchors[i + 1] };
        nodes.push(here);
        if i + 1 < n {
            let beyond = anchors.get(i + 2).copied().unwrap_or(after);
            nodes.push(add(here, scale(sub(after, before), 1.0 / 6.0)));
            nodes.push(sub(after, scale(sub(beyond, here), 1.0 / 6.0)));
        }
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stroke across, back and across again.
    fn zigzag() -> Vec<P> {
        let corners = [(0.0, 0.0), (200.0, 10.0), (10.0, 90.0), (200.0, 160.0)];
        corners
            .windows(2)
            .flat_map(|w| (0..25).map(move |i| add(w[0], scale(sub(w[1], w[0]), i as f32 / 25.0))))
            .collect()
    }

    #[test]
    fn a_stroke_bending_twice_gets_a_curve_through_its_bends() {
        let nodes = fit(&zigzag());
        // anchors at 0, 3, 6 ...: each span's two controls between them; one at each corner
        assert_eq!(nodes.len() % 3, 1, "{nodes:?}");
        assert!(nodes.len() >= 10, "{nodes:?}");
        assert_eq!(nodes[0], (0.0, 0.0));
        // each anchor's controls either side on one line (smooth), the first's toward the next
        let (before, at, after) = (nodes[2], nodes[3], nodes[4]);
        let (u, v) = (sub(at, before), sub(after, at));
        assert!((u.0 * v.1 - u.1 * v.0).abs() < 1e-2 * length(u) * length(v), "{u:?} {v:?}");
    }

    #[test]
    fn the_calls_offer_a_curve_for_a_polyline_or_when_asked_and_not_otherwise() {
        let stroke = zigzag();
        let mut offer = |best: i32| {
            procraft_curve_begin(stroke.len() as i32, best);
            JOB.lock().unwrap().as_mut().unwrap().points = stroke.iter().flat_map(|&(x, y)| [x, y]).collect();
            procraft_curve_run()
        };
        assert!(offer(POLYLINE) >= 10);
        assert!(offer(ASKED) >= 10);
        assert_eq!(offer(0), 0, "a line");
        assert_eq!(offer(4), 0, "an ellipse");
    }
}
