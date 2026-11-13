//! QuickShape curves: a stroke that bends more than once, held, snaps to a curve through its bends
//! rather than a polyline, its points there to reshape it. The app hands over the stroke's points
//! and what QuickShape found first (the plugin kit passes them); this finds the bends and lays a chain of cubics through them,
//! each anchor's controls leaning the way the stroke came and went so the joins are smooth. The app
//! draws the curve and lets it be reshaped. It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The interface is written down at the top of `crates/core/src/plugin_run.rs`.

/// A stroke's bend is a corner when the path strays this far (as a part of the stroke's size) from
/// the straight line between its neighbours.
const CORNER_TOLERANCE: f32 = 0.08;

kit::quickshape_curve!(offer);

/// The curve offered for a stroke of `points`, QuickShape having found `found` first: one for a
/// polyline (it bends more than once) or when a curve is asked for by name (none found), else none.
fn offer(points: &[P], found: Option<kit::ShapeKind>) -> Option<Vec<P>> {
    match found {
        None | Some(kit::ShapeKind::Polyline) => Some(fit(points)).filter(|nodes| nodes.len() >= 4),
        Some(_) => None,
    }
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
    fn a_curve_is_offered_for_a_polyline_or_when_asked_and_not_otherwise() {
        let stroke = zigzag();
        assert!(offer(&stroke, Some(kit::ShapeKind::Polyline)).is_some_and(|n| n.len() >= 10));
        assert!(offer(&stroke, None).is_some_and(|n| n.len() >= 10));
        assert_eq!(offer(&stroke, Some(kit::ShapeKind::Line)), None);
        assert_eq!(offer(&stroke, Some(kit::ShapeKind::Ellipse)), None);
    }
}
