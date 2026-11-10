//! Reading Clip Studio sub tools (`.sut`): a SQLite database whose `Node` table names the brushes,
//! each pointing (`NodeVariantID`) at its settings in `Variant`, and whose `MaterialFile` table
//! holds the tip and texture images that the settings name by key. A material is a tar of a
//! thumbnail (a PNG of the picture, no more than 300 pixels a side), the picture itself as a layer
//! (in a container of its own, its pixels not readable here, but with a blank preview PNG of the
//! picture's full size in it) and the material's settings.

use crate::sqlite::{self, Table, Value};

/// A brush as a sub tool has it: its settings where it has them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CspBrush {
    pub name: String,
    /// The edge, 0 soft to 1 hard.
    pub hardness: Option<f32>,
    /// Space between stamps as a share of the size; none where the app spaces them itself.
    pub spacing: Option<f32>,
    /// How round the tip is, 1 a circle; flattened top to bottom if `flattened_vertically`, else
    /// side to side.
    pub roundness: Option<f32>,
    pub flattened_vertically: bool,
    /// The tip's angle, radians (from the stroke's way, where it turns with the stroke).
    pub angle: f32,
    /// Whether the tip turns with the stroke's way.
    pub follows: bool,
    /// How far each stamp turns at random: 0 not at all, 1 any way.
    pub random_turn: f32,
    /// Whether its tip is laid along the stroke as a ribbon, one picture after the other.
    pub ribbon: bool,
    /// Its size, pixels, its opacity, and how much ink each stamp lays (0 to 1).
    pub size: Option<f32>,
    pub opacity: Option<f32>,
    pub flow: Option<f32>,
    /// What of it follows the pen, where the file says.
    pub pen: CspPen,
    /// Its start and end, where the file says.
    pub taper: Option<CspTaper>,
    /// The pictures (PNGs) its tip may be, largest first (one may be empty), none if it is round.
    pub tip_pngs: Vec<Vec<u8>>,
    /// The paper texture it paints through, if it has one.
    pub texture: Option<CspTexture>,
    /// Its tip sprayed as particles about each place, if it is.
    pub spray: Option<CspSpray>,
}

/// How a sub tool sprays its tip: as particles falling about each place, in an area the brush's
/// size across.
#[derive(Clone, Debug, PartialEq)]
pub struct CspSpray {
    /// A particle's size, pixels, at the brush's own size.
    pub particle: f32,
    /// How many fall at each place, and what of that follows the pen.
    pub density: u32,
    pub density_by: Option<CspDynamics>,
    /// How they gather, -1 to 1: the higher, the nearer the middle.
    pub bias: f32,
    /// Each particle's angle, radians (from the stroke's way, where it turns with the stroke), and
    /// how far each turns at random (0 not at all, 1 any way).
    pub angle: f32,
    pub random_turn: f32,
    pub follows: bool,
}

/// How a sub tool's size, opacity, flow, thickness and texture density follow the pen (none where
/// the file doesn't say).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CspPen {
    pub size: Option<CspDynamics>,
    pub opacity: Option<CspDynamics>,
    pub flow: Option<CspDynamics>,
    pub thickness: Option<CspDynamics>,
    pub texture_density: Option<CspDynamics>,
}

/// What drives one setting, as the file's effect sources keep it: for each that does, the share
/// of the setting left at its least (no pressure; the tilt curve's 0; the fastest; the least
/// chance), and pressure's and tilt's curves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CspDynamics {
    pub pressure: Option<CspCurve>,
    pub tilt: Option<CspCurve>,
    pub speed: Option<f32>,
    pub random: Option<f32>,
}

/// A source's curve: the share left at its least (0 to 1), and the curve's points (x the source,
/// y the share of the way from the least to all of it, both 0 to 1, x rising).
#[derive(Clone, Debug, PartialEq)]
pub struct CspCurve {
    pub least: f32,
    pub points: Vec<(f32, f32)>,
}

/// A sub tool's start and end: how long each is (none where it has none), in pixels or (if
/// `of_stroke`) as a share of the stroke, and how far they thin its size and fade its opacity (0
/// to 1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CspTaper {
    pub start: Option<f32>,
    pub end: Option<f32>,
    pub of_stroke: bool,
    pub size: f32,
    pub opacity: f32,
}

/// A sub tool's texture: the pictures it may be (as a tip's), how strongly it shows (0 to 1), its
/// scale (1 the picture's own size) and whether it is reversed (dark showing, not light).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CspTexture {
    pub pngs: Vec<Vec<u8>>,
    /// How wide the picture itself is, pixels, where its material says (its PNGs may be smaller).
    pub width: Option<u32>,
    pub density: Option<f32>,
    pub scale: Option<f32>,
    pub reverse: bool,
    /// Its differences in density made stronger.
    pub emphasized: bool,
    pub mode: TextureMode,
    /// Its turn, radians.
    pub rotation: f32,
    /// How much lighter (darker, below 0) and of more contrast (less) its picture is made, -1 to 1.
    pub brightness: f32,
    pub contrast: f32,
}

/// How a texture combines with the stroke, by the number a sub tool keeps it as (in the order the
/// app lists them, one number left out); one we don't know is [`TextureMode::Other`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextureMode {
    #[default]
    Normal,
    Multiply,
    Subtract,
    Compare,
    Overlay,
    ColorDodge,
    HardMix,
    Height,
    Other,
}

impl TextureMode {
    fn from_number(n: i64) -> TextureMode {
        match n {
            0 => TextureMode::Normal,
            1 => TextureMode::Multiply,
            2 => TextureMode::Subtract,
            3 => TextureMode::Compare,
            5 => TextureMode::Overlay,
            7 => TextureMode::ColorDodge,
            8 => TextureMode::HardMix,
            9 => TextureMode::Height,
            _ => TextureMode::Other,
        }
    }
}

/// The most brushes read from one file, PNGs looked at in a material, pictures a tip may be, and
/// bytes of them all the brushes may have together (each has its own copy).
const MOST_BRUSHES: usize = 256;
const MOST_PNGS: usize = 32;
const MOST_TIPS: usize = 4;
const MOST_TIP_BYTES: usize = 256 << 20;
/// The most files looked at in a material's tar.
const MOST_TAR_FILES: usize = 64;
const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// What turns the tip, as bits of the number a sub tool keeps (the two lowest always set): the
/// stroke's way, and chance (as far as its random scale says).
const TURNS_WITH_STROKE: i64 = 64;
const TURNS_AT_RANDOM: i64 = 128;

/// What drives a setting, as bits of an effect source list: pressure, tilt, speed and chance.
const BY_PRESSURE: u32 = 0x10;
const BY_TILT: u32 = 0x20;
const BY_SPEED: u32 = 0x40;
const BY_CHANCE: u32 = 0x80;
/// The most points a curve of an effect source has.
const MOST_CURVE_POINTS: u32 = 64;

/// Pixels to a millimetre, for lengths a sub tool keeps in millimetres (unit 2): at 350 pixels an
/// inch, the resolution a new illustration starts at in the app.
const PIXELS_PER_MM: f64 = 350.0 / 25.4;

/// What a start or end thins or fades, by its number in a sub tool's list: the opacity, the size,
/// and how much ink each stamp lays.
const TAPERS_OPACITY: u32 = 112;
const TAPERS_SIZE: u32 = 1001;
const TAPERS_FLOW: u32 = 1021;

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
        // a brush's settings give it a size: a fill's, or another tool's, don't
        let variant = variants.as_ref().and_then(|v| {
            id.and_then(|id| v.rows.iter().find(|r| v.get(r, "VariantID").and_then(Value::as_int) == Some(id)))
                .filter(|r| v.get(r, "BrushSize").and_then(Value::as_real).is_some())
                .map(|r| (v, r))
        });
        if variant.is_some() {
            out.push(brush(&name, variant, materials.as_ref(), &mut left));
        }
        if out.len() >= MOST_BRUSHES {
            break;
        }
    }
    // no settings at all to go by: the first named node, round
    if out.is_empty() && variants.is_none() {
        out.extend(nodes.rows.iter().find_map(named).map(|name| brush(&name, None, materials.as_ref(), &mut left)));
    }
    (!out.is_empty()).then_some(out)
}

/// A brush from its settings; its tip's pictures taken from what is `left` of the bytes they may
/// have (round once that runs out).
fn brush(name: &str, variant: Option<(&Table, &Vec<Value>)>, materials: Option<&Table>, left: &mut usize) -> CspBrush {
    let number =
        |key: &str| variant.and_then(|(t, r)| t.get(r, key)).and_then(Value::as_real).filter(|v| v.is_finite());
    // the pictures a column names (by the key in its list) from the materials, and their own size
    let mut pictures = |column: &str, guess: bool| {
        let key = variant.and_then(|(t, r)| t.get(r, column)).and_then(Value::as_blob).and_then(pattern_key)?;
        let (mut pngs, size) = materials.map(|m| material_pngs(m, &key, guess)).unwrap_or_default();
        pngs.retain(|png| left.checked_sub(png.len()).map(|l| *left = l).is_some());
        Some((pngs, size))
    };
    // a tip of pictures only if it says (its list may be left from before)
    let patterned = number("BrushUsePatternImage").is_none_or(|v| v != 0.0);
    let tips = pictures("BrushPatternImageArray", true).filter(|_| patterned).map(|(pngs, _)| pngs).unwrap_or_default();
    // a texture only as its key names it
    let share = |key: &str| number(key).filter(|v| (-100.0..=100.0).contains(v)).map_or(0.0, |v| (v / 100.0) as f32);
    let texture = pictures("TextureImage", false).filter(|(p, _)| !p.is_empty()).map(|(pngs, size)| CspTexture {
        pngs,
        width: size.map(|(w, _)| w),
        density: number("TextureDensity").filter(|v| (0.0..=100.0).contains(v)).map(|v| (v / 100.0) as f32),
        scale: number("TextureScale2").filter(|v| *v > 0.0).map(|v| (v / 100.0) as f32),
        reverse: number("TextureReverseDensity").is_some_and(|v| v != 0.0),
        emphasized: number("TextureStressDensity").is_some_and(|v| v != 0.0),
        mode: number("TextureCompositeMode").map_or(TextureMode::Normal, |v| TextureMode::from_number(v as i64)),
        rotation: number("TextureRotate").map_or(0.0, |v| v.to_radians() as f32),
        brightness: share("TextureBrightness"),
        contrast: share("TextureContrast"),
    });
    let percent = |key: &str| number(key).filter(|v| (0.0..=100.0).contains(v)).map(|v| (v / 100.0) as f32);
    // an interval of the app's own (any but the fixed one, 0) leaves the number kept unused
    let fixed = number("BrushAutoIntervalType").is_none_or(|v| v == 0.0);
    let turns = number("BrushRotationEffector").map_or(0, |v| v as i64);
    let blob = |key: &str| variant.and_then(|(t, r)| t.get(r, key)).and_then(Value::as_blob);
    let follows = |key: &str| blob(key).and_then(dynamics);
    let pen = CspPen {
        size: follows("BrushSizeEffector"),
        opacity: follows("BrushOpacityEffector"),
        flow: follows("BrushFlowEffector"),
        thickness: follows("BrushThicknessEffector"),
        texture_density: follows("TextureDensityEffector"),
    };
    // a length in pixels (unit 0) or millimetres (2)
    let length = |key: &str| {
        let unit = number(&format!("{key}Unit")).unwrap_or(0.0);
        let per = if unit == 0.0 {
            1.0
        } else if unit == 2.0 {
            PIXELS_PER_MM
        } else {
            0.0
        };
        number(key).map(|v| v * per).filter(|v| *v > 0.0).map(|v| v as f32)
    };
    // its start and end: by length, or (kind 1) by a share of the stroke
    let of_stroke = number("BrushInOutType").is_some_and(|v| v == 1.0);
    let end = |used: &str, by_length: &str, share: &str| {
        let on = number(used).is_some_and(|v| v != 0.0);
        on.then(|| if of_stroke { percent(share) } else { length(by_length) }).flatten()
    };
    // none where neither is on
    let (start, end) =
        (end("BrushUseIn", "BrushInLength", "BrushInRatio"), end("BrushUseOut", "BrushOutLength", "BrushOutRatio"));
    let taper = (start.is_some() || end.is_some()).then(|| {
        let (size, opacity) = blob("BrushInOutTarget").map_or((1.0, 0.0), taper_targets);
        CspTaper { start, end, of_stroke, size, opacity }
    });
    // its tip sprayed: the particles' own size, how many, how they gather and turn
    let spray_turns = number("BrushRotationEffectorInSpray").map_or(0, |v| v as i64);
    let spray = number("BrushUseSpray").filter(|v| *v != 0.0).and_then(|_| {
        Some(CspSpray {
            particle: length("BrushSpraySize")?,
            density: number("BrushSprayDensity").map_or(1, |v| v.clamp(1.0, 100.0) as u32),
            density_by: follows("BrushSprayDensityEffector"),
            bias: number("BrushSprayBias").map_or(0.0, |v| (v / 100.0).clamp(-1.0, 1.0) as f32),
            angle: number("BrushRotationInSpray").map_or(0.0, |v| v.to_radians() as f32),
            random_turn: if spray_turns & TURNS_AT_RANDOM != 0 {
                percent("BrushRotationRandomInSpray").unwrap_or(1.0)
            } else {
                0.0
            },
            follows: spray_turns & TURNS_WITH_STROKE != 0,
        })
    });
    CspBrush {
        name: name.to_owned(),
        hardness: percent("BrushHardness"),
        spacing: number("BrushInterval").filter(|v| fixed && *v > 0.0).map(|v| (v / 100.0) as f32),
        roundness: number("BrushThickness").filter(|v| (1.0..=100.0).contains(v)).map(|v| (v / 100.0) as f32),
        flattened_vertically: number("BrushVerticalThicknes").is_some_and(|v| v != 0.0),
        angle: number("BrushRotation").map_or(0.0, |v| v.to_radians() as f32),
        follows: turns & TURNS_WITH_STROKE != 0,
        random_turn: if turns & TURNS_AT_RANDOM != 0 {
            percent("BrushRotationRandomScale").unwrap_or(1.0)
        } else {
            0.0
        },
        ribbon: number("BrushRibbon").is_some_and(|v| v != 0.0),
        size: length("BrushSize"),
        opacity: percent("Opacity"),
        flow: percent("BrushFlow"),
        pen,
        taper,
        tip_pngs: tips,
        texture,
        spray,
    }
}

/// What drives a setting, from its effect source list: a header (its length, 40 to 48 bytes, at
/// 0; what drives it at 8; the least each source leaves, in percent, at 12, 16, 20 and 24; and
/// how long pressure's and tilt's curves are at 32 and 36), then those curves.
fn dynamics(blob: &[u8]) -> Option<CspDynamics> {
    let word = |at: usize| blob.get(at..at.checked_add(4)?).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes);
    let head = word(0)? as usize;
    if !(40..=48).contains(&head) || !head.is_multiple_of(4) || head > blob.len() {
        return None;
    }
    let on = word(8)?;
    let least = |k: usize| word(12 + 4 * k).map_or(0.0, |v| (v as i32).clamp(0, 100) as f32 / 100.0);
    // the curves, one after the other: none where the header gives none, and none from one that
    // doesn't read on
    let (pressure_bytes, tilt_bytes) = (word(32)? as usize, word(36)? as usize);
    let pressure_curve = (pressure_bytes > 0).then(|| curve(blob, head, pressure_bytes)).flatten();
    let tilt_curve = head
        .checked_add(pressure_bytes)
        .filter(|_| tilt_bytes > 0 && (pressure_bytes == 0 || pressure_curve.is_some()))
        .and_then(|at| curve(blob, at, tilt_bytes));
    let straight = || vec![(0.0, 0.0), (1.0, 1.0)];
    let source = |bit: u32, k: usize, points: Option<Vec<(f32, f32)>>| {
        (on & bit != 0).then(|| CspCurve { least: least(k), points: points.unwrap_or_else(straight) })
    };
    Some(CspDynamics {
        pressure: source(BY_PRESSURE, 0, pressure_curve),
        tilt: source(BY_TILT, 1, tilt_curve),
        speed: (on & BY_SPEED != 0).then(|| least(2)),
        random: (on & BY_CHANCE != 0).then(|| least(3)),
    })
}

/// The curve `bytes` long at `at`: its kind (12), how many points and how far apart (16 bytes:
/// two big-endian doubles each), then the points; held to 0..1, x rising.
fn curve(blob: &[u8], at: usize, bytes: usize) -> Option<Vec<(f32, f32)>> {
    let word = |at: usize| blob.get(at..at.checked_add(4)?).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes);
    let (kind, count, stride) = (word(at)?, word(at.checked_add(4)?)?, word(at.checked_add(8)?)?);
    if kind != 12 || stride != 16 || !(2..=MOST_CURVE_POINTS).contains(&count) || bytes != 12 + 16 * count as usize {
        return None;
    }
    let double = |at: usize| blob.get(at..at.checked_add(8)?).and_then(|b| b.try_into().ok()).map(f64::from_be_bytes);
    let mut points = Vec::new();
    for k in 0..count as usize {
        let p = at.checked_add(12 + 16 * k)?;
        let (x, y) = (double(p)?, double(p.checked_add(8)?)?);
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        points.push((x.clamp(0.0, 1.0) as f32, y.clamp(0.0, 1.0) as f32));
    }
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    points.dedup_by(|later, earlier| (later.0 - earlier.0).abs() < 1e-6);
    (points.len() >= 2).then_some(points)
}

/// How far a start and end thin the size and fade the opacity (0 to 1), from the list of what
/// they act on (a header of its length, a count and each entry's length, then entries of a
/// setting's number, whether it is on, and how far, in percent): the size fully where none is on.
fn taper_targets(blob: &[u8]) -> (f32, f32) {
    let word = |at: usize| blob.get(at..at.checked_add(4)?).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes);
    let (Some(head), Some(count), Some(12)) = (word(0), word(4), word(8)) else { return (1.0, 0.0) };
    let entries: Vec<(u32, f32)> = (0..count.min(64) as usize)
        .map_while(|k| {
            let at = (head as usize).checked_add(12 * k)?;
            Some((word(at)?, word(at.checked_add(4)?)?, word(at.checked_add(8)?)?))
        })
        .filter(|&(_, on, _)| on != 0)
        .map(|(id, _, amount)| (id, amount.min(100) as f32 / 100.0))
        .collect();
    if entries.is_empty() {
        return (1.0, 0.0);
    }
    let amount = |ids: &[u32]| entries.iter().filter(|(id, _)| ids.contains(id)).map(|e| e.1).fold(0.0f32, f32::max);
    (amount(&[TAPERS_SIZE]), amount(&[TAPERS_OPACITY, TAPERS_FLOW]))
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

/// The pictures (PNGs) of the material `key` names (a row with it in a text column), else (a tip's,
/// `guess`) of one that holds a brush pattern, else of any, with the size of the picture itself
/// where the material says: a material's thumbnail (its layer's PNG is a blank preview), else the
/// [`MOST_TIPS`] largest PNGs in it, largest first.
fn material_pngs(table: &Table, key: &str, guess: bool) -> (Vec<Vec<u8>>, Option<(u32, u32)>) {
    let named: Vec<&Vec<Value>> =
        table.rows.iter().filter(|r| r.iter().any(|v| v.as_text().is_some_and(|t| t.contains(key)))).collect();
    let rows = if !named.is_empty() || !guess {
        named
    } else {
        let patterns: Vec<&Vec<Value>> = table
            .rows
            .iter()
            .filter(|r| r.iter().any(|v| v.as_blob().is_some_and(|b| find(b, 0, b"BrushPattern").is_some())))
            .collect();
        if patterns.is_empty() { table.rows.iter().collect() } else { patterns }
    };
    let blobs: Vec<&[u8]> = rows.into_iter().flat_map(|r| r.iter().filter_map(Value::as_blob)).collect();
    // a material's tar: its thumbnails, and its layer's size
    let (mut thumbnails, mut size) = (Vec::new(), None);
    for files in blobs.iter().filter_map(|b| tar_files(b)) {
        for (name, file) in files {
            if name.ends_with(b".png") && file.starts_with(&PNG) {
                thumbnails.push(file.to_vec());
            } else if name.ends_with(b".layer") {
                size = size.or_else(|| png_size(file));
            }
        }
    }
    if !thumbnails.is_empty() {
        thumbnails.truncate(MOST_TIPS);
        return (thumbnails, size);
    }
    let mut found: Vec<(u64, &[u8])> = blobs.into_iter().flat_map(pngs).take(MOST_PNGS).collect();
    // largest first, in the material's order where they are as large
    found.sort_by_key(|(area, _)| std::cmp::Reverse(*area));
    (found.into_iter().take(MOST_TIPS).map(|(_, png)| png.to_vec()).collect(), size)
}

/// The files in a tar (by name, as bytes), as far as they can be read; none if `bytes` isn't one.
fn tar_files(bytes: &[u8]) -> Option<Vec<(&[u8], &[u8])>> {
    if bytes.get(257..262)? != b"ustar" {
        return None;
    }
    let mut out = Vec::new();
    let mut at = 0usize;
    while out.len() < MOST_TAR_FILES {
        let Some(header) = at.checked_add(512).and_then(|end| bytes.get(at..end)) else { break };
        let name = header.get(..100).unwrap_or_default();
        let name = name.split(|&b| b == 0).next().unwrap_or_default();
        // the size, in octal (spaces before it, a space or nothing after)
        let size = header
            .get(124..136)
            .unwrap_or_default()
            .iter()
            .skip_while(|&&b| b == b' ')
            .take_while(|b| (b'0'..=b'7').contains(*b))
            .try_fold(0usize, |n, &d| n.checked_mul(8)?.checked_add(usize::from(d - b'0')));
        let (Some(size), false) = (size, name.is_empty()) else { break };
        let start = at + 512;
        let Some(file) = start.checked_add(size).and_then(|end| bytes.get(start..end)) else { break };
        out.push((name, file));
        // each file padded to a whole 512 bytes
        let Some(next) = size.div_ceil(512).checked_mul(512).and_then(|s| start.checked_add(s)) else { break };
        at = next;
    }
    Some(out)
}

/// The width and height the first PNG in `bytes` says it has.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let start = find(bytes, 0, &PNG)?;
    let number = |at: usize| bytes.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes);
    Some((number(start + 16)?, number(start + 20)?)).filter(|&(w, h)| w > 0 && h > 0)
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
        let mut texture = vec![0u8; 6];
        texture.extend(utf16("0:data:paper-2"));
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
                "CREATE TABLE Variant(_PW_ID INTEGER PRIMARY KEY, VariantID INTEGER, BrushSize REAL, BrushHardness REAL, BrushInterval REAL, BrushThickness REAL, BrushRotation REAL, BrushPatternImageArray BLOB, TextureImage BLOB, TextureDensity REAL, TextureScale2 REAL, TextureReverseDensity INTEGER)",
                vec![vec![
                    Value::Null,
                    Value::Int(7),
                    Value::Real(12.0),
                    Value::Real(80.0),
                    Value::Real(25.0),
                    Value::Real(50.0),
                    Value::Real(90.0),
                    Value::Blob(pattern),
                    Value::Blob(texture),
                    Value::Real(60.0),
                    Value::Real(150.0),
                    Value::Int(1),
                ]],
            ),
            (
                "MaterialFile",
                "CREATE TABLE MaterialFile(_PW_ID INTEGER PRIMARY KEY, CatalogPath TEXT, FileData BLOB)",
                vec![
                    vec![Value::Null, Value::Text("other".into()), Value::Blob(png(500, 500))],
                    vec![Value::Null, Value::Text("x/0:data:tip-1/y".into()), Value::Blob(tar)],
                    vec![Value::Null, Value::Text("x/0:data:paper-2/y".into()), Value::Blob(png(8, 8))],
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
        // its texture: the material its key names, how strongly, how big, reversed
        let t = b.texture.as_ref().unwrap();
        assert_eq!((t.pngs.len(), &t.pngs[0][16..24]), (1, &[0, 0, 0, 8, 0, 0, 0, 8][..]));
        assert_eq!((t.density, t.scale, t.reverse), (Some(0.6), Some(1.5), true));
    }

    /// A tar of `files`, as a material keeps its own.
    fn tar(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, data) in files {
            let mut header = [0u8; 512];
            header[..name.len()].copy_from_slice(name.as_bytes());
            header[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
            header[257..263].copy_from_slice(b"ustar\0");
            out.extend_from_slice(&header);
            out.extend_from_slice(data);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.extend_from_slice(&[0; 1024]);
        out
    }

    /// A sub tool whose texture is a material as the app keeps one: a tar of its thumbnail and
    /// its layer, which holds a blank preview of the picture's full size.
    fn material_sample() -> Vec<u8> {
        let mut layer = b"\x89C2F\r\n\x1a\n".to_vec();
        layer.extend(png(801, 600));
        let material = tar(&[
            ("catalog.zip", b"zip"),
            ("data/material_0.layer", &layer),
            ("thumbnail/thumbnail.png", &png(300, 225)),
            ("icedata/layerData.xml", b"<xml/>"),
        ]);
        let mut key = vec![0u8; 6];
        key.extend(utf16("0:data:paper"));
        database(&[
            (
                "Node",
                "CREATE TABLE Node(NodeName TEXT, NodeVariantID INTEGER)",
                vec![vec![Value::Text("Pencil".into()), Value::Int(1)]],
            ),
            (
                "Variant",
                "CREATE TABLE Variant(VariantID INTEGER, BrushSize REAL, TextureImage BLOB, TextureCompositeMode INTEGER, TextureStressDensity INTEGER, TextureRotate REAL, TextureBrightness INTEGER, TextureContrast INTEGER)",
                vec![vec![
                    Value::Int(1),
                    Value::Real(5.0),
                    Value::Blob(key),
                    Value::Int(2),
                    Value::Int(1),
                    Value::Real(90.0),
                    Value::Int(-30),
                    Value::Int(250),
                ]],
            ),
            (
                "MaterialFile",
                "CREATE TABLE MaterialFile(OriginalPath TEXT, FileData BLOB)",
                vec![vec![Value::Text("x:0:data:paper".into()), Value::Blob(material)]],
            ),
        ])
    }

    #[test]
    fn a_materials_thumbnail_is_its_picture_and_its_layer_gives_its_size() {
        let t = read(&material_sample()).unwrap()[0].texture.clone().unwrap();
        // the thumbnail alone, not the layer's blank preview, and the width of the picture itself
        assert_eq!((t.pngs.len(), &t.pngs[0][16..24]), (1, &[0, 0, 1, 44, 0, 0, 0, 225][..]));
        assert_eq!(t.width, Some(801));
        // its mode, its differences made stronger, its turn, and its brightness (a contrast past
        // what the app sets left out)
        assert_eq!((t.mode, t.emphasized, t.brightness, t.contrast), (TextureMode::Subtract, true, -0.3, 0.0));
        assert!((t.rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        crate::damaged::survives("Clip Studio material", &material_sample(), |b| {
            let _ = read(b);
        });
    }

    #[test]
    fn a_sub_tool_reads_how_its_tip_turns_its_size_and_how_much_it_lays() {
        let mut pattern = vec![0u8; 6];
        pattern.extend(utf16("0:data:tip-1"));
        let db = |rows: Vec<Vec<Value>>| {
            database(&[
                (
                    "Node",
                    "CREATE TABLE Node(NodeName TEXT, NodeVariantID INTEGER)",
                    rows.iter()
                        .enumerate()
                        .map(|(i, _)| vec![Value::Text(format!("B{i}")), Value::Int(i as i64)])
                        .collect(),
                ),
                (
                    "Variant",
                    "CREATE TABLE Variant(VariantID INTEGER, BrushSize REAL, BrushSizeUnit INTEGER, Opacity INTEGER, BrushFlow INTEGER, BrushInterval REAL, BrushAutoIntervalType INTEGER, BrushRotationEffector INTEGER, BrushRotationRandomScale INTEGER, BrushRibbon INTEGER, BrushUsePatternImage INTEGER, BrushPatternImageArray BLOB, BrushVerticalThicknes INTEGER)",
                    rows.into_iter().enumerate().map(|(i, r)| [vec![Value::Int(i as i64)], r].concat()).collect(),
                ),
                (
                    "MaterialFile",
                    "CREATE TABLE MaterialFile(CatalogPath TEXT, FileData BLOB)",
                    vec![vec![Value::Text("x/0:data:tip-1/y".into()), Value::Blob(png(8, 8))]],
                ),
            ])
        };
        let (int, real) = (Value::Int, Value::Real);
        let brushes = read(&db(vec![
            // in pixels, half opaque, laying 80 %, spaced as it says; turning with the stroke and
            // at random by a quarter; a ribbon
            vec![
                real(30.0),
                int(0),
                int(50),
                int(80),
                real(25.0),
                int(0),
                int(195),
                int(25),
                int(1),
                int(1),
                Value::Blob(pattern.clone()),
                int(1),
            ],
            // spaced by the app (its interval's number left unused), turning at random only, its
            // size in millimetres; its pattern list left from before it was made round
            vec![
                real(30.0),
                int(2),
                int(100),
                int(100),
                real(25.0),
                int(3),
                int(131),
                int(100),
                int(0),
                int(0),
                Value::Blob(pattern),
                int(0),
            ],
        ]))
        .unwrap();
        let b = &brushes[0];
        assert_eq!((b.size, b.opacity, b.flow, b.spacing), (Some(30.0), Some(0.5), Some(0.8), Some(0.25)));
        assert_eq!((b.follows, b.random_turn, b.ribbon, b.tip_pngs.len()), (true, 0.25, true, 1));
        // flattened top to bottom, the other side to side
        assert!(b.flattened_vertically && !brushes[1].flattened_vertically);
        let b = &brushes[1];
        assert_eq!((b.spacing, b.follows, b.random_turn, b.ribbon), (None, false, 1.0, false));
        // 30 mm at 350 pixels an inch
        assert!((b.size.unwrap() - 413.386).abs() < 1e-3);
        assert!(b.tip_pngs.is_empty());
    }

    /// An effect source list as a sub tool keeps one: a 44-byte header driving by `on`, the least
    /// each source leaves (percent), then pressure's and tilt's curves (none if empty).
    fn effector(on: u32, least: [u32; 4], pressure: &[(f64, f64)], tilt: &[(f64, f64)]) -> Vec<u8> {
        let curve = |points: &[(f64, f64)]| {
            let mut c = Vec::new();
            if !points.is_empty() {
                [12, points.len() as u32, 16].iter().for_each(|v| c.extend(v.to_be_bytes()));
                points.iter().for_each(|(x, y)| c.extend([x.to_be_bytes(), y.to_be_bytes()].concat()));
            }
            c
        };
        let (p, t) = (curve(pressure), curve(tilt));
        let mut out = Vec::new();
        for v in [44, 0x1f0, on, least[0], least[1], least[2], least[3], 0, p.len() as u32, t.len() as u32, 100] {
            out.extend(v.to_be_bytes());
        }
        out.extend(p);
        out.extend(t);
        out
    }

    /// What a start and end act on: entries of a setting's number, whether on, and how far.
    fn taper_list(entries: &[(u32, u32, u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        for v in [12, entries.len() as u32, 12] {
            out.extend(v.to_be_bytes());
        }
        for (id, on, amount) in entries {
            [id, on, amount].iter().for_each(|v| out.extend(v.to_be_bytes()));
        }
        out
    }

    #[test]
    fn a_sprayed_sub_tool_reads_its_particles() {
        let columns = "VariantID INTEGER, BrushSize REAL, BrushUseSpray INTEGER, BrushSpraySize REAL, BrushSpraySizeUnit INTEGER, BrushSprayDensity INTEGER, BrushSprayBias INTEGER, BrushRotationInSpray REAL, BrushRotationEffectorInSpray INTEGER, BrushRotationRandomInSpray INTEGER";
        let (int, real) = (Value::Int, Value::Real);
        let variant = format!("CREATE TABLE Variant({columns})");
        let text = |t: &str| Value::Text(t.into());
        let db = database(&[
            (
                "Node",
                "CREATE TABLE Node(NodeName TEXT, NodeVariantID INTEGER)",
                vec![vec![text("A"), int(1)], vec![text("B"), int(2)], vec![text("C"), int(3)]],
            ),
            (
                "Variant",
                variant.as_str(),
                vec![
                    // particles of 7.5 pixels, 16 to a place, gathered a little less than even,
                    // turned at random 90 %
                    vec![
                        int(1),
                        real(30.0),
                        int(1),
                        real(7.5),
                        int(0),
                        int(16),
                        int(-50),
                        real(90.0),
                        int(131),
                        int(90),
                    ],
                    // half a millimetre, turned with the stroke, not at random (its scale kept)
                    vec![int(2), real(30.0), int(1), real(0.5), int(2), int(4), int(100), real(0.0), int(67), int(100)],
                    // not sprayed
                    vec![int(3), real(30.0), int(0), real(7.5), int(0), int(16), int(0), real(0.0), int(3), int(100)],
                ],
            ),
        ]);
        let brushes = read(&db).unwrap();
        let first = brushes[0].spray.as_ref().unwrap();
        assert_eq!((first.particle, first.density, first.bias, first.follows), (7.5, 16, -0.5, false));
        assert!((first.angle - 90f32.to_radians()).abs() < 1e-6 && (first.random_turn - 0.9).abs() < 1e-6);
        let second = brushes[1].spray.as_ref().unwrap();
        assert!((second.particle - 0.5 * PIXELS_PER_MM as f32).abs() < 1e-4, "{}", second.particle);
        assert_eq!((second.density, second.bias, second.random_turn, second.follows), (4, 1.0, 0.0, true));
        assert_eq!(brushes[2].spray, None);
    }

    #[test]
    fn a_sub_tool_reads_what_follows_the_pen_and_its_start_and_end() {
        let curve = [(0.0, 0.0), (0.75, 0.25), (1.0, 1.0)];
        let columns = "VariantID INTEGER, BrushSize REAL, BrushSizeUnit INTEGER, BrushSizeEffector BLOB, BrushOpacityEffector BLOB, BrushFlowEffector BLOB, BrushThicknessEffector BLOB, TextureDensityEffector BLOB, BrushUseIn INTEGER, BrushInLength REAL, BrushInLengthUnit INTEGER, BrushInRatio REAL, BrushUseOut INTEGER, BrushOutLength REAL, BrushOutLengthUnit INTEGER, BrushOutRatio REAL, BrushInOutType INTEGER, BrushInOutTarget BLOB";
        let (int, real, blob) = (Value::Int, Value::Real, Value::Blob);
        let variant = format!("CREATE TABLE Variant({columns})");
        let db = database(&[
            (
                "Node",
                "CREATE TABLE Node(NodeName TEXT, NodeVariantID INTEGER)",
                vec![vec![Value::Text("A".into()), int(1)], vec![Value::Text("B".into()), int(2)]],
            ),
            (
                "Variant",
                variant.as_str(),
                vec![
                    vec![
                        int(1),
                        real(30.0),
                        int(0),
                        // pressure to 30 % along its curve, and chance down to 60 %
                        blob(effector(0x90, [30, 100, 0, 60], &curve, &[])),
                        // a curve kept, but nothing driving it
                        blob(effector(0, [50, 100, 0, 0], &curve, &[])),
                        // speed down to 40 %
                        blob(effector(0x40, [0, 100, 40, 0], &[], &[])),
                        // pressure, its curve not kept: straight
                        blob(effector(0x10, [0, 100, 0, 0], &[], &[])),
                        // tilt, its curve after none of pressure's
                        blob(effector(0x20, [0, 25, 0, 0], &[], &[(0.0, 1.0), (1.0, 0.0)])),
                        int(1),
                        real(20.0),
                        int(0),
                        real(30.0),
                        int(1),
                        real(2.0),
                        int(2),
                        real(30.0),
                        int(0),
                        // the size thinned halfway, the opacity faded 80 %, another left off
                        blob(taper_list(&[(TAPERS_SIZE, 1, 50), (TAPERS_OPACITY, 1, 80), (1041, 0, 100)])),
                    ],
                    vec![
                        int(2),
                        real(1.0),
                        int(2),
                        // not a list (its header too long)
                        blob(vec![0, 0, 0, 99, 0, 0, 0, 0]),
                        Value::Null,
                        Value::Null,
                        Value::Null,
                        Value::Null,
                        int(1),
                        real(20.0),
                        int(0),
                        real(25.0),
                        int(0),
                        real(20.0),
                        int(0),
                        real(30.0),
                        int(1),
                        blob(taper_list(&[(TAPERS_SIZE, 0, 100)])),
                    ],
                ],
            ),
        ]);
        let brushes = read(&db).unwrap();
        let pen = &brushes[0].pen;
        let size = pen.size.as_ref().unwrap();
        let points = vec![(0.0, 0.0), (0.75, 0.25), (1.0, 1.0)];
        assert_eq!(size.pressure, Some(CspCurve { least: 0.3, points }));
        assert_eq!((size.tilt.as_ref(), size.speed, size.random), (None, None, Some(0.6)));
        assert_eq!(pen.opacity, Some(CspDynamics::default()));
        assert_eq!(pen.flow.as_ref().map(|f| f.speed), Some(Some(0.4)));
        let straight = vec![(0.0, 0.0), (1.0, 1.0)];
        assert_eq!(
            pen.thickness.as_ref().and_then(|t| t.pressure.clone()),
            Some(CspCurve { least: 0.0, points: straight })
        );
        let tilt = pen.texture_density.as_ref().and_then(|d| d.tilt.clone());
        assert_eq!(tilt, Some(CspCurve { least: 0.25, points: vec![(0.0, 1.0), (1.0, 0.0)] }));
        // 20 px in, 2 mm out (at 350 pixels an inch)
        let t = brushes[0].taper.as_ref().unwrap();
        assert_eq!((t.start, t.of_stroke, t.size, t.opacity), (Some(20.0), false, 0.5, 0.8));
        assert!((t.end.unwrap() - 27.559).abs() < 1e-3);
        // a quarter of the stroke in, no end; nothing on in its list: the size, fully; its size
        // in millimetres; an effect list that isn't one, none
        let b = &brushes[1];
        let t = b.taper.as_ref().unwrap();
        assert_eq!((t.start, t.end, t.of_stroke, t.size, t.opacity), (Some(0.25), None, true, 1.0, 0.0));
        assert!((b.size.unwrap() - 13.780).abs() < 1e-3);
        assert_eq!(b.pen, CspPen::default());
        crate::damaged::survives("Clip Studio effect sources", &db, |b| {
            let _ = read(b);
        });
    }

    #[test]
    fn a_texture_its_key_doesnt_find_is_none() {
        let db = database(&[
            (
                "Node",
                "CREATE TABLE Node(NodeName TEXT, NodeVariantID INTEGER)",
                vec![vec![Value::Text("Pen".into()), Value::Int(1)]],
            ),
            (
                "Variant",
                "CREATE TABLE Variant(VariantID INTEGER, BrushSize REAL, TextureImage BLOB)",
                vec![vec![Value::Int(1), Value::Real(5.0), Value::Blob(utf16("0:data:gone"))]],
            ),
            ("MaterialFile", "CREATE TABLE MaterialFile(FileData BLOB)", vec![vec![Value::Blob(png(8, 8))]]),
        ]);
        let b = &read(&db).unwrap()[0];
        // not guessed from what else there is, as a tip is
        assert_eq!(b.texture, None);
    }

    #[test]
    fn other_files_and_damaged_ones_are_refused_not_a_crash() {
        assert!(read(b"nothing").is_none());
        // a fill tool: settings, but none of a brush's
        let fill = database(&[
            (
                "Node",
                "CREATE TABLE Node(NodeName TEXT, NodeVariantID INTEGER)",
                vec![vec![Value::Text("Bucket".into()), Value::Int(1)]],
            ),
            (
                "Variant",
                "CREATE TABLE Variant(VariantID INTEGER, FillCloseGap INTEGER)",
                vec![vec![Value::Int(1), Value::Int(1)]],
            ),
        ]);
        assert!(read(&fill).is_none());
        crate::damaged::survives("Clip Studio sub tool", &sample(), |b| {
            let _ = read(b);
        });
    }
}
