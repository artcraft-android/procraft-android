//! Reading Clip Studio sub tools (`.sut`): a SQLite database whose `Node` table names the brushes,
//! each pointing (`NodeVariantID`) at its settings in `Variant`, and whose `MaterialFile` table
//! holds the tip images (PNGs, often inside a tar) that the settings name by key.

use crate::sqlite::{self, Table, Value};

/// A brush as a sub tool has it: its settings where it has them.
#[derive(Clone, Debug, PartialEq)]
pub struct CspBrush {
    pub name: String,
    /// The edge, 0 soft to 1 hard.
    pub hardness: Option<f32>,
    /// Space between stamps as a share of the size.
    pub spacing: Option<f32>,
    /// How round the tip is, 1 a circle.
    pub roundness: Option<f32>,
    /// The tip's angle, radians.
    pub angle: f32,
    /// The pictures (PNGs) its tip may be, largest first (one may be empty), none if it is round.
    pub tip_pngs: Vec<Vec<u8>>,
}

/// The most brushes read from one file, PNGs looked at in a material, pictures a tip may be, and
/// bytes of them all the brushes may have together (each has its own copy).
const MOST_BRUSHES: usize = 256;
const MOST_PNGS: usize = 32;
const MOST_TIPS: usize = 4;
const MOST_TIP_BYTES: usize = 256 << 20;
const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The brushes in a sub tool file, in its order; none if it isn't one.
pub fn read(bytes: &[u8]) -> Option<Vec<CspBrush>> {
    let db = sqlite::open(bytes)?;
    let nodes = db.table("Node")?;
    let variants = db.table("Variant");
    let materials = db.table("MaterialFile");
    let named = |row: &Vec<Value>| {
        nodes.get(row, "NodeName").and_then(Value::as_text).map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned)
    };
    // the nodes with settings of their own (a folder of sub tools has none)
    let mut out = Vec::new();
    let mut left = MOST_TIP_BYTES;
    for row in &nodes.rows {
        let Some(name) = named(row) else { continue };
        let id = nodes.get(row, "NodeVariantID").and_then(Value::as_int);
        let variant = variants.as_ref().and_then(|v| {
            id.and_then(|id| v.rows.iter().find(|r| v.get(r, "VariantID").and_then(Value::as_int) == Some(id)))
                .map(|r| (v, r))
        });
        if variant.is_some() {
            out.push(brush(&name, variant, materials.as_ref(), &mut left));
        }
        if out.len() >= MOST_BRUSHES {
            break;
        }
    }
    // none: the first named node, round
    if out.is_empty() {
        out.extend(nodes.rows.iter().find_map(named).map(|name| brush(&name, None, materials.as_ref(), &mut left)));
    }
    (!out.is_empty()).then_some(out)
}

/// A brush from its settings; its tip's pictures taken from what is `left` of the bytes they may
/// have (round once that runs out).
fn brush(name: &str, variant: Option<(&Table, &Vec<Value>)>, materials: Option<&Table>, left: &mut usize) -> CspBrush {
    let number =
        |key: &str| variant.and_then(|(t, r)| t.get(r, key)).and_then(Value::as_real).filter(|v| v.is_finite());
    let pattern =
        variant.and_then(|(t, r)| t.get(r, "BrushPatternImageArray")).and_then(Value::as_blob).and_then(pattern_key);
    let mut tips = pattern.and_then(|key| materials.map(|m| material_pngs(m, &key))).unwrap_or_default();
    tips.retain(|png| left.checked_sub(png.len()).map(|l| *left = l).is_some());
    CspBrush {
        name: name.to_owned(),
        hardness: number("BrushHardness").filter(|v| (0.0..=100.0).contains(v)).map(|v| (v / 100.0) as f32),
        spacing: number("BrushInterval").filter(|v| *v > 0.0).map(|v| (v / 100.0) as f32),
        roundness: number("BrushThickness").filter(|v| (1.0..=100.0).contains(v)).map(|v| (v / 100.0) as f32),
        angle: number("BrushRotation").map_or(0.0, |v| v.to_radians() as f32),
        tip_pngs: tips,
    }
}

/// The key a pattern list names its first image by: a UTF-16 string with `:data:` in it.
fn pattern_key(blob: &[u8]) -> Option<String> {
    let mut current = String::new();
    let mut found = Vec::new();
    for pair in blob.as_chunks::<2>().0 {
        match char::from_u32(u32::from(u16::from_le_bytes(*pair))).filter(|c| c.is_ascii_graphic()) {
            Some(c) => current.push(c),
            None => found.push(std::mem::take(&mut current)),
        }
    }
    found.push(current);
    found.into_iter().find(|s| s.contains(":data:") && s.len() >= 8)
}

/// The PNGs in the material `key` names (a row with it in a text column), else in one that holds a
/// brush pattern, else in any; the [`MOST_TIPS`] largest, largest first.
fn material_pngs(table: &Table, key: &str) -> Vec<Vec<u8>> {
    let named: Vec<&Vec<Value>> =
        table.rows.iter().filter(|r| r.iter().any(|v| v.as_text().is_some_and(|t| t.contains(key)))).collect();
    let rows = if !named.is_empty() {
        named
    } else {
        let patterns: Vec<&Vec<Value>> = table
            .rows
            .iter()
            .filter(|r| r.iter().any(|v| v.as_blob().is_some_and(|b| find(b, 0, b"BrushPattern").is_some())))
            .collect();
        if patterns.is_empty() { table.rows.iter().collect() } else { patterns }
    };
    let mut found: Vec<(u64, &[u8])> =
        rows.into_iter().flat_map(|r| r.iter().filter_map(Value::as_blob).flat_map(pngs)).take(MOST_PNGS).collect();
    // largest first, in the material's order where they are as large
    found.sort_by_key(|(area, _)| std::cmp::Reverse(*area));
    found.into_iter().take(MOST_TIPS).map(|(_, png)| png.to_vec()).collect()
}

/// The PNGs inside `bytes` (each from its signature through its end chunk), with their areas.
fn pngs(bytes: &[u8]) -> Vec<(u64, &[u8])> {
    let mut out = Vec::new();
    let mut from = 0;
    while out.len() < MOST_PNGS {
        let Some(start) = find(bytes, from, &PNG) else { break };
        let Some(end) = find(bytes, start + 8, b"IEND").map(|e| e + 8) else { break };
        let size = |at: usize| bytes.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes);
        // the header chunk's width and height
        if let (Some(w), Some(h), Some(png)) = (size(start + 16), size(start + 20), bytes.get(start..end)) {
            out.push((u64::from(w) * u64::from(h), png));
        }
        from = end;
    }
    out
}

fn find(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::tests::database;

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// A PNG-shaped run: signature, a header chunk of `w` × `h`, an end chunk.
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut p = PNG.to_vec();
        p.extend_from_slice(&13u32.to_be_bytes());
        p.extend_from_slice(b"IHDR");
        p.extend_from_slice(&w.to_be_bytes());
        p.extend_from_slice(&h.to_be_bytes());
        p.extend_from_slice(&[8, 0, 0, 0, 0, 0, 0, 0, 0]);
        p.extend_from_slice(&[0, 0, 0, 0]);
        p.extend_from_slice(b"IEND");
        p.extend_from_slice(&[0; 4]);
        p
    }

    fn sample() -> Vec<u8> {
        let mut pattern = vec![0u8; 6];
        pattern.extend(utf16("0:data:tip-1"));
        pattern.extend([0, 0]);
        let mut tar = b"tip.png\0padding".to_vec();
        tar.extend(png(4, 4));
        tar.extend(b"more");
        tar.extend(png(64, 32));
        database(&[
            (
                "Node",
                "CREATE TABLE Node(_PW_ID INTEGER PRIMARY KEY, NodeName TEXT, NodeVariantID INTEGER)",
                vec![
                    vec![Value::Null, Value::Text("Tools".into()), Value::Int(0)],
                    vec![Value::Null, Value::Text("Dry Pencil".into()), Value::Int(7)],
                ],
            ),
            (
                "Variant",
                "CREATE TABLE Variant(_PW_ID INTEGER PRIMARY KEY, VariantID INTEGER, BrushHardness REAL, BrushInterval REAL, BrushThickness REAL, BrushRotation REAL, BrushPatternImageArray BLOB)",
                vec![vec![
                    Value::Null,
                    Value::Int(7),
                    Value::Real(80.0),
                    Value::Real(25.0),
                    Value::Real(50.0),
                    Value::Real(90.0),
                    Value::Blob(pattern),
                ]],
            ),
            (
                "MaterialFile",
                "CREATE TABLE MaterialFile(_PW_ID INTEGER PRIMARY KEY, CatalogPath TEXT, FileData BLOB)",
                vec![
                    vec![Value::Null, Value::Text("other".into()), Value::Blob(png(500, 500))],
                    vec![Value::Null, Value::Text("x/0:data:tip-1/y".into()), Value::Blob(tar)],
                ],
            ),
        ])
    }

    #[test]
    fn a_sub_tool_reads_its_settings_and_named_tip() {
        let brushes = read(&sample()).unwrap();
        // the folder (no settings) isn't a brush
        assert_eq!(brushes.len(), 1);
        let b = &brushes[0];
        assert_eq!(b.name, "Dry Pencil");
        assert_eq!((b.hardness, b.spacing, b.roundness), (Some(0.8), Some(0.25), Some(0.5)));
        assert!((b.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        // the material its pattern names, the larger of its two pictures first
        let sizes: Vec<&[u8]> = b.tip_pngs.iter().map(|t| &t[16..24]).collect();
        assert_eq!(sizes, [&[0, 0, 0, 64, 0, 0, 0, 32][..], &[0, 0, 0, 4, 0, 0, 0, 4][..]]);
    }

    #[test]
    fn other_files_and_damaged_ones_are_refused_not_a_crash() {
        assert!(read(b"nothing").is_none());
        crate::damaged::survives("Clip Studio sub tool", &sample(), |b| {
            let _ = read(b);
        });
    }
}
