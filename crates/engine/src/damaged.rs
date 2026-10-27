//! Damaged files: each reader, given a good file cut short or with bytes changed, says it can't
//! read it (or reads what it can) and never panics.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::brush::Brush;
use crate::brushfile::{self, Pictures};
use crate::document::{BlendMode, Document};
use crate::formats::{self, PsdKind, PsdLayer};
use crate::native::{self, Export, TimeLapse};
use crate::profile::Profile;

/// Gives `read` the file `good` cut short at many lengths and with bytes changed at many places;
/// fails, naming the cases, if any of them panicked.
pub(crate) fn survives(name: &str, good: &[u8], read: impl Fn(&[u8])) {
    let n = good.len();
    assert!(n > 0, "{name}: no sample");
    let mut cases: Vec<(String, Vec<u8>)> = Vec::new();
    // cut short: every length near the start and the end, and others spread between
    let mut lengths: Vec<usize> =
        (0..n.min(64)).chain(n.saturating_sub(64)..n).chain((0..256).map(|i| i * n / 256)).collect();
    lengths.sort_unstable();
    lengths.dedup();
    for len in lengths {
        cases.push((format!("cut to {len} bytes"), good[..len].to_vec()));
    }
    // a byte changed: each of the first ones (headers), then places spread over the file
    let mut seed = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for i in 0..512usize {
        let at = if i < 64 { i % n } else { (next() % n as u64) as usize };
        let mut bad = good.to_vec();
        bad[at] ^= match i % 4 {
            0 => 0xFF,
            1 => 0x80,
            2 => 0x01,
            _ => (next() as u8) | 1,
        };
        cases.push((format!("byte {at} changed"), bad));
    }
    // several at once
    for i in 0..64 {
        let mut bad = good.to_vec();
        for _ in 0..8 {
            let at = (next() % n as u64) as usize;
            bad[at] = next() as u8;
        }
        cases.push((format!("eight bytes changed ({i})"), bad));
    }
    let failed: Vec<&str> = cases
        .iter()
        .filter(|(_, bad)| catch_unwind(AssertUnwindSafe(|| read(bad))).is_err())
        .map(|c| c.0.as_str())
        .collect();
    assert!(failed.is_empty(), "{name}: panicked when {}", failed.join("; "));
}

/// A small layered artwork with a mask and a time-lapse, as a package.
fn package() -> Vec<u8> {
    let (w, h) = (300, 280);
    let mut doc = Document::new(w, h);
    let base = doc.current();
    doc.set_blend(base, BlendMode::Multiply);
    let top = doc.add_layer();
    doc.set_clipped(top, true);
    let mask = doc.add_mask(base).unwrap();
    let paint = |f: &dyn Fn(u32, u32) -> [u8; 4]| -> Vec<u8> {
        (0..h).flat_map(|y| (0..w).flat_map(move |x| f(x, y))).collect()
    };
    let pixels = HashMap::from([
        (base, paint(&|x, y| [(x % 256) as u8, (y % 256) as u8, 0, 255])),
        (top, paint(&|x, y| if (x / 37 + y / 41) % 3 == 0 { [10, 20, 30, 128] } else { [0; 4] })),
        (mask, paint(&|x, _| if x < 150 { [255; 4] } else { [0, 0, 0, 255] })),
    ]);
    let time_lapse =
        TimeLapse { segments: vec![b"segment one".to_vec()], frame_size: (64, 48), ..TimeLapse::default() };
    native::write(&Export {
        document: &doc,
        pixels: &pixels,
        composite: &vec![255u8; (w * h * 4) as usize],
        thumbnail_png: b"preview",
        name: "Damaged",
        author: "Someone",
        strokes: 3,
        tracked_seconds: 1.5,
        seed: 9,
        time_lapse: Some(&time_lapse),
    })
}

/// A brush with pictures of its own, as a brush file.
fn brush_file() -> Vec<u8> {
    let mut brush = Brush::round_pen();
    brush.set_setting("spacing", 0.3);
    brush.set_setting("grain_scale", 0.4);
    brushfile::write(&brush, "Someone", &pictures())
}

fn pictures() -> Pictures<'static> {
    Pictures {
        shape_png: b"shape picture",
        grain_png: Some(b"grain picture"),
        metallic_png: None,
        roughness_png: None,
        thumbnail_png: b"preview",
        author_png: Some(b"author"),
        signature_png: None,
    }
}

#[test]
fn a_damaged_package_is_refused_not_a_crash() {
    let good = package();
    assert!(native::read(&good).is_ok());
    survives("package", &good, |b| {
        let _ = native::read(b);
    });
    survives("package's time-lapse", &good, |b| {
        let _ = native::read_time_lapse(b);
    });
}

#[test]
fn a_damaged_brush_or_brush_set_is_refused_not_a_crash() {
    let good = brush_file();
    assert!(brushfile::read(&good).is_some());
    survives("brush", &good, |b| {
        let _ = brushfile::read(b);
    });
    let (one, two) = (Brush::round_pen(), Brush { name: "Second".to_owned(), ..Brush::round_pen() });
    let set = brushfile::write_set("Set", &[(&one, "", pictures()), (&two, "", pictures())], |i| format!("brush-{i}"));
    assert_eq!(brushfile::read_set(&set).map(|s| s.1.len()), Some(2));
    survives("brush set", &set, |b| {
        let _ = brushfile::read_set(b);
    });
}

#[test]
fn a_damaged_psd_is_refused_not_a_crash() {
    let (w, h) = (40u32, 30u32);
    let rgba: Vec<u8> = (0..w * h).flat_map(|i| [(i % 251) as u8, 40, 90, 255]).collect();
    let grey: Vec<u8> = (0..w * h).map(|i| (i % 256) as u8).collect();
    let layers = [
        PsdLayer {
            name: "Inked",
            kind: PsdKind::Pixels { rgba: &rgba, mask: Some(&grey) },
            opacity: 0.8,
            visible: true,
            blend: BlendMode::Screen,
            clipped: false,
        },
        PsdLayer {
            name: "Base",
            kind: PsdKind::Pixels { rgba: &rgba, mask: None },
            opacity: 1.0,
            visible: true,
            blend: BlendMode::Normal,
            clipped: false,
        },
    ];
    let good = formats::psd(w, h, 132.0, &crate::icc::icc(Profile::DisplayP3), &layers, &rgba, None);
    assert!(crate::psd::read(&good).is_ok());
    survives("PSD", &good, |b| {
        let _ = crate::psd::read(b);
    });
}

#[test]
fn a_damaged_colour_profile_is_refused_not_a_crash() {
    for profile in [Profile::Srgb, Profile::DisplayP3] {
        let good = crate::icc::icc(profile);
        assert!(crate::cms::IccProfile::parse(&good).is_some());
        survives("colour profile", &good, |b| {
            let _ = crate::cms::IccProfile::parse(b);
        });
    }
}

#[test]
fn damaged_archives_and_tiles_are_refused_not_a_crash() {
    use crate::plist::Value;
    let value = Value::Dict(vec![
        ("name".to_owned(), Value::String("A name".to_owned())),
        (
            "list".to_owned(),
            Value::Array(vec![Value::Int(-3), Value::Real(0.5), Value::Bool(true), Value::Data(vec![1, 2, 3])]),
        ),
    ]);
    let good = crate::plist::write(&value);
    assert_eq!(crate::plist::read(&good), Some(value));
    survives("property list", &good, |b| {
        let _ = crate::plist::read(b);
        let _ = crate::keyed::Archive::read(b);
    });
    let tile: Vec<u8> = (0..256 * 64).map(|i| (i % 7 * 30) as u8).collect();
    let good = crate::applz4::encode(&tile);
    assert_eq!(crate::applz4::decode(&good, tile.len()).as_deref(), Some(&tile[..]));
    survives("tile", &good, |b| {
        let _ = crate::applz4::decode(b, tile.len());
    });
    let good = package();
    survives("zip", &good, |b| {
        if let Some(z) = crate::zip::ZipReader::new(b) {
            let names: Vec<String> = z.names().map(str::to_owned).collect();
            for n in names {
                let _ = z.read(&n);
            }
        }
    });
}

#[test]
fn damaged_models_and_environments_are_refused_not_a_crash() {
    let usdz = include_bytes!("../tests/models/cube.usdz");
    assert!(crate::usd::read_usdz(usdz).is_ok());
    survives("USDZ", usdz, |b| {
        let _ = crate::usd::read_usdz(b);
    });
    let usdc = include_bytes!("../tests/models/grid.usdc");
    survives("USD crate file", usdc, |b| {
        let _ = crate::usd::read_usd(b);
    });
    let usda = include_bytes!("../tests/models/cube.usda");
    survives("USD text", usda, |b| {
        let _ = crate::usd::read_usda(&String::from_utf8_lossy(b));
    });
    let obj = b"mtllib cube.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nvn 0 0 1\nusemtl Paint\nf 1/1/1 2/2/1 3/3/1 4/4/1\n";
    let mtl = b"newmtl Paint\nKd 0.5 0.5 0.5\nmap_Kd paint.png\n";
    assert!(crate::mesh::read_obj(&String::from_utf8_lossy(obj), Some(&String::from_utf8_lossy(mtl))).is_ok());
    survives("OBJ", obj, |b| {
        let _ = crate::mesh::read_obj(&String::from_utf8_lossy(b), Some(&String::from_utf8_lossy(mtl)));
    });
    survives("MTL", mtl, |b| {
        let _ = crate::mesh::read_mtl(&String::from_utf8_lossy(b));
    });
    let sky = crate::environment::Equirect {
        width: 16,
        height: 8,
        pixels: (0..128).map(|i| [i as f32 / 128.0, 0.5, 2.0]).collect(),
    };
    let good = crate::environment::write_hdr(&sky);
    assert!(crate::environment::read_hdr(&good).is_some());
    survives("HDR", &good, |b| {
        let _ = crate::environment::read_hdr(b);
    });
}
