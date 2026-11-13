//! An SVG picture of an artwork's vector layers, each a group of its items in order: a stroke as its
//! outline, as wide as its brush where the pen pressed; a fill as its region; a mask hiding what is
//! under it in its layer, as a mask of its own. A smudge, which moves paint about, has no shape to
//! give.

use std::fmt::Write;

use kit::layer::{Export, Item, Layer, Tool};

use crate::Vectors;

/// How much of the brush's width a stroke has where the pen barely pressed, the rest coming with
/// the pressure.
const LIGHTEST: f32 = 0.35;
/// The steps of a lone dot's circle.
const DOT_STEPS: usize = 16;

/// The picture `export` asks for: the canvas's size, each of its layers a group.
pub(crate) fn picture(export: &Export) -> Vec<u8> {
    let (w, h) = (export.width.max(1), export.height.max(1));
    let mut defs = String::new();
    let mut body = String::new();
    let mut masks = 0;
    for (n, layer) in export.layers.iter().enumerate() {
        let items = if layer.data.is_empty() {
            Vec::new()
        } else {
            Vectors::load(export.kind, &layer.data)
                .map(|v| v.items.iter().map(|k| k.item()).collect())
                .unwrap_or_default()
        };
        // what is drawn so far, a mask wrapping it as it comes
        let mut drawn = String::new();
        for item in &items {
            match item.tool() {
                Tool::Smudge => {}
                Tool::Mask => {
                    masks += 1;
                    let _ = write!(
                        defs,
                        r#"<mask id="m{masks}" maskUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{h}"><rect width="{w}" height="{h}" fill="white"/><path d="{}" fill="black"/></mask>"#,
                        stroke_path(item)
                    );
                    drawn = format!(r#"<g mask="url(#m{masks})">{drawn}</g>"#);
                }
                Tool::Paint => drawn.push_str(&shape_of(item)),
            }
        }
        let mut group = format!(r#"<g id="layer-{}" data-name="{}""#, n + 1, escaped(&layer.name));
        if layer.opacity < 1.0 {
            let _ = write!(group, r#" opacity="{}""#, num(layer.opacity));
        }
        if !layer.shown {
            group.push_str(r#" display="none""#);
        }
        let _ = write!(body, "{group}>{drawn}</g>\n");
    }
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(out, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#);
    if !defs.is_empty() {
        let _ = writeln!(out, "<defs>{defs}</defs>");
    }
    out.push_str(&body);
    out.push_str("</svg>\n");
    out.into_bytes()
}

/// `item` as an SVG element: a fill's region, or a stroke's outline, in its colour at its opacity.
fn shape_of(item: &Item) -> String {
    let [r, g, b] = item.colour().map(|c| (c * 255.0).round() as u8);
    let mut style = format!("#{r:02x}{g:02x}{b:02x}\"");
    let opacity = item.opacity();
    if opacity < 1.0 {
        let _ = write!(style, r#" fill-opacity="{}""#, num(opacity));
    }
    if item.is_fill() {
        let t = item.transform();
        let mut d = String::new();
        for outline in item.outlines() {
            let points: Vec<(f32, f32)> = outline.into_iter().map(|p| kit::layer::apply(&t, p)).collect();
            d.push_str(&polygon(&points));
        }
        format!(r#"<path d="{d}" fill-rule="evenodd" fill="{style}/>"#)
    } else {
        format!(r#"<path d="{}" fill="{style}/>"#, stroke_path(item))
    }
}

/// The outline of `item`'s stroke and of each copy its symmetry makes, as path data.
fn stroke_path(item: &Item) -> String {
    let half = item.width() / 2.0;
    let radii: Vec<f32> =
        item.samples().iter().map(|s| half * (LIGHTEST + (1.0 - LIGHTEST) * s.pressure.clamp(0.0, 1.0))).collect();
    item.paths().iter().map(|path| outline(path, &radii)).collect()
}

/// A closed polygon through `points`, as path data.
fn polygon(points: &[(f32, f32)]) -> String {
    let mut d = String::new();
    for (i, &(x, y)) in points.iter().enumerate() {
        let _ = write!(d, "{}{} {} ", if i == 0 { "M" } else { "L" }, num(x), num(y));
    }
    if !points.is_empty() {
        d.push_str("Z ");
    }
    d
}

/// The outline of a stroke along `points`, `radii` wide either side at each, round at its ends.
fn outline(points: &[(f32, f32)], radii: &[f32]) -> String {
    // each place once (the widest there)
    let mut at: Vec<((f32, f32), f32)> = Vec::with_capacity(points.len());
    for (&p, &r) in points.iter().zip(radii) {
        match at.last_mut() {
            Some((q, rq)) if (q.0 - p.0).hypot(q.1 - p.1) < 0.01 => *rq = rq.max(r),
            _ => at.push((p, r)),
        }
    }
    match at.as_slice() {
        [] => String::new(),
        [((x, y), r)] => {
            let dot: Vec<(f32, f32)> = (0..DOT_STEPS)
                .map(|k| {
                    let a = std::f32::consts::TAU * k as f32 / DOT_STEPS as f32;
                    (x + r * a.cos(), y + r * a.sin())
                })
                .collect();
            polygon(&dot)
        }
        _ => {
            let n = at.len();
            let side = |i: usize| {
                let (a, b) = (at[i.saturating_sub(1)].0, at[(i + 1).min(n - 1)].0);
                let (tx, ty) = (b.0 - a.0, b.1 - a.1);
                let length = tx.hypot(ty).max(1e-6);
                let ((x, y), r) = at[i];
                let (nx, ny) = (-ty / length * r, tx / length * r);
                ((x + nx, y + ny), (x - nx, y - ny))
            };
            let sides: Vec<_> = (0..n).map(side).collect();
            let mut d = String::new();
            for (i, (left, _)) in sides.iter().enumerate() {
                let _ = write!(d, "{}{} {} ", if i == 0 { "M" } else { "L" }, num(left.0), num(left.1));
            }
            // round about the end, back down the other side, round about the start
            let (end, start) = (at[n - 1].1, at[0].1);
            let _ = write!(d, "A{} {} 0 0 0 {} {} ", num(end), num(end), num(sides[n - 1].1.0), num(sides[n - 1].1.1));
            for (_, right) in sides.iter().rev().skip(1) {
                let _ = write!(d, "L{} {} ", num(right.0), num(right.1));
            }
            let _ = write!(d, "A{} {} 0 0 0 {} {} Z ", num(start), num(start), num(sides[0].0.0), num(sides[0].0.1));
            d
        }
    }
}

/// `v` written short: two places at most, no trailing zeros.
fn num(v: f32) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let text = format!("{v:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0".to_owned() } else { text.to_owned() }
}

/// `text` as it goes in an attribute.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use kit::layer::ExportedLayer;

    #[test]
    fn numbers_are_short_and_names_safe() {
        assert_eq!(
            (num(1.0), num(2.5), num(-0.001), num(3.14159)),
            ("1".into(), "2.5".into(), "0".into(), "3.14".into())
        );
        assert_eq!(escaped(r#"A & "B" <c>"#), "A &amp; &quot;B&quot; &lt;c&gt;");
    }

    #[test]
    fn a_stroke_s_outline_goes_round_it() {
        let d = outline(&[(0.0, 10.0), (10.0, 10.0), (20.0, 10.0)], &[2.0, 2.0, 2.0]);
        assert!(d.starts_with("M0 12 L10 12 L20 12 A2 2 0 0 0 20 8 L10 8 L0 8 A2 2 0 0 0 0 12 Z"), "{d}");
        assert!(outline(&[(5.0, 5.0)], &[3.0]).starts_with("M8 5 "));
        assert!(outline(&[], &[]).is_empty());
    }

    #[test]
    fn an_empty_artwork_is_a_picture_of_its_size_with_its_layers() {
        let export = Export {
            export: 0,
            kind: 0,
            width: 64,
            height: 32,
            layers: vec![ExportedLayer { name: "Ink & paper".into(), shown: false, opacity: 0.5, data: Vec::new() }],
        };
        let text = String::from_utf8(picture(&export)).unwrap();
        assert!(text.contains(r#"width="64" height="32" viewBox="0 0 64 32""#));
        assert!(
            text.contains(r#"<g id="layer-1" data-name="Ink &amp; paper" opacity="0.5" display="none"></g>"#),
            "{text}"
        );
        assert!(text.trim_end().ends_with("</svg>"));
    }
}
