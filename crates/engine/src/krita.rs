//! Reading Krita brushes: a preset (`.kpp`) is a PNG whose `preset` text holds the brush's
//! settings as XML, its tip either drawn from a few numbers (an auto brush) or an image it names;
//! a bundle (`.bundle`) is a zip of presets (`paintoppresets/`) and the images they name
//! (`brushes/`: GIMP brushes, PNGs, ABR files).

use crate::abr::AbrTip;
use crate::xml::{self, Element};
use crate::zip::ZipReader;

/// A brush as a Krita preset has it.
#[derive(Clone, Debug, PartialEq)]
pub struct KritaBrush {
    pub name: String,
    pub tip: KritaTip,
    /// Space between stamps as a share of the size, if the preset sets it (not automatic).
    pub spacing: Option<f32>,
    /// The tip's own angle, radians.
    pub angle: f32,
    /// Whether pen pressure changes the size and the opacity.
    pub pressure_size: bool,
    pub pressure_opacity: bool,
    /// The texture it paints through, if it has one.
    pub texture: Option<KritaTexture>,
}

/// A preset's texture: its picture (a PNG), its scale (1 the picture's own size), how strongly it
/// shows (0 to 1) and whether it is inverted.
#[derive(Clone, Debug, PartialEq)]
pub struct KritaTexture {
    pub png: Vec<u8>,
    pub scale: f32,
    pub strength: f32,
    pub invert: bool,
}

/// What a preset's tip is.
#[derive(Clone, Debug, PartialEq)]
pub enum KritaTip {
    /// A round tip: its edge (0 soft to 1 hard) and how round it is (1 a circle).
    Round { hardness: f32, roundness: f32 },
    /// A grey image (white paints).
    Image(AbrTip),
    /// A PNG to read as one (dark and opaque paints).
    Png(Vec<u8>),
}

/// The most presets read from a bundle, and the most bytes their tip files may come to together.
const MOST_PRESETS: usize = 512;
const MOST_TIP_BYTES: usize = 256 << 20;
/// The most a text chunk inflates to.
const MOST_TEXT: usize = 16 << 20;
/// The side a drawn rectangular tip is made at.
const DRAWN_SIDE: usize = 128;
const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The preset in `kpp`, its tip images looked up by file name with `file`; none if it isn't one.
pub fn read_preset(kpp: &[u8], file: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<KritaBrush> {
    let text = png_text(kpp).into_iter().find(|(k, _)| k == "preset")?.1;
    let root = xml::parse(&text)?;
    if root.name != "Preset" {
        return None;
    }
    let param = |name: &str| {
        root.all("param").into_iter().find(|p| p.attribute("name") == Some(name)).map(|p| p.text.trim().to_owned())
    };
    let definition = param("brush_definition").and_then(|d| xml::parse(&d));
    let brush = definition.as_ref().and_then(|d| d.find("Brush"));
    let number = |e: Option<&Element>, key: &str| {
        e.and_then(|e| e.attribute(key)).and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite())
    };
    let automatic = brush.and_then(|b| b.attribute("useAutoSpacing")).is_some_and(|v| v == "1" || v == "true");
    let tip = brush.and_then(|b| tip(b, file)).unwrap_or(KritaTip::Round { hardness: 1.0, roundness: 1.0 });
    let flag = |name: &str| param(name).is_some_and(|v| v == "true");
    let value = |name: &str| param(name).and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite());
    // the texture's picture: in the preset, as the base64 of its base64; else one it names
    let embedded = || {
        let once = crate::base64::decode(&param("Texture/Pattern/Pattern")?.split_whitespace().collect::<String>())?;
        if once.starts_with(&PNG) {
            return Some(once);
        }
        crate::base64::decode(&String::from_utf8_lossy(&once).split_whitespace().collect::<String>())
    };
    let texture = flag("Texture/Pattern/Enabled")
        .then(|| embedded().filter(|p| p.starts_with(&PNG)).or_else(|| file(&param("Texture/Pattern/Name")?)))
        .flatten()
        .map(|png| KritaTexture {
            png,
            scale: value("Texture/Pattern/Scale").filter(|s| *s > 0.0).unwrap_or(1.0),
            strength: value("Texture/Pattern/Strength")
                .or_else(|| value("Texture/Strength/Value"))
                .unwrap_or(1.0)
                .clamp(0.0, 1.0),
            invert: flag("Texture/Pattern/Invert"),
        });
    Some(KritaBrush {
        name: root.attribute("name").map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Krita Brush").to_owned(),
        tip,
        spacing: if automatic { None } else { number(brush, "spacing").filter(|s| *s > 0.0) },
        angle: number(brush, "angle").unwrap_or(0.0),
        pressure_size: param("PressureSize").is_none_or(|v| v == "true"),
        pressure_opacity: param("PressureOpacity").is_some_and(|v| v == "true"),
        texture,
    })
}

/// The presets in a bundle, in its order, with its name; none if it isn't one.
pub fn read_bundle(bytes: &[u8]) -> Option<(String, Vec<KritaBrush>)> {
    let zip = ZipReader::new(bytes)?;
    let names: Vec<String> = zip.names().map(str::to_owned).collect();
    let is_bundle = zip.read("mimetype").is_some_and(|m| m.starts_with(b"application/x-krita-resourcebundle"))
        || names.iter().any(|n| n == "META-INF/manifest.xml");
    if !is_bundle {
        return None;
    }
    let title = zip
        .read("meta.xml")
        .and_then(|m| xml::parse(&String::from_utf8_lossy(&m)))
        .and_then(|m| ["title", "name"].iter().find_map(|k| m.find(k).map(|e| e.text.trim().to_owned())))
        .filter(|t| !t.is_empty())
        .unwrap_or_default();
    // a tip or texture named by a preset: in the brushes or patterns folder, by its file name; each
    // preset has its own copy, so all of them together may be only so big (round tips and no
    // textures after that)
    let left = std::cell::Cell::new(MOST_TIP_BYTES);
    let take = |n: usize| left.get().checked_sub(n).map(|l| left.set(l)).is_some();
    let file = |wanted: &str| {
        let wanted = wanted.rsplit(['/', '\\']).next().unwrap_or(wanted).to_ascii_lowercase();
        let name = names.iter().find(|n| {
            (n.starts_with("brushes/") || n.starts_with("patterns/"))
                && n.rsplit('/').next().is_some_and(|f| f.to_ascii_lowercase() == wanted)
        })?;
        take(zip.size(name)?).then(|| zip.read(name)).flatten()
    };
    let presets: Vec<KritaBrush> = names
        .iter()
        .filter(|n| n.starts_with("paintoppresets/") && n.to_ascii_lowercase().ends_with(".kpp"))
        .take(MOST_PRESETS)
        .filter_map(|n| read_preset(&zip.read(n)?, &file))
        .map(|mut p| {
            // its texture counts as well (one the bundle had, twice over: a rough budget)
            if p.texture.as_ref().is_some_and(|t| !take(t.png.len())) {
                p.texture = None;
            }
            p
        })
        .collect();
    (!presets.is_empty()).then_some((title, presets))
}

/// The tip a `Brush` element describes.
fn tip(brush: &Element, file: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<KritaTip> {
    let number = |e: &Element, key: &str, or: f32| {
        e.attribute(key).and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).unwrap_or(or)
    };
    match brush.attribute("type")? {
        "auto_brush" => {
            let mask = brush.find("MaskGenerator")?;
            let ratio = number(mask, "ratio", 1.0).clamp(0.01, 1.0);
            // the fades: the share of the radius that is solid (1 hard)
            let (hfade, vfade) =
                (number(mask, "hfade", 1.0).clamp(0.0, 1.0), number(mask, "vfade", 1.0).clamp(0.0, 1.0));
            if mask.attribute("type") == Some("rect") {
                return Some(KritaTip::Image(rectangle(ratio, hfade, vfade)));
            }
            Some(KritaTip::Round { hardness: (hfade + vfade) / 2.0, roundness: ratio })
        }
        _ => {
            let name = brush.attribute("filename")?;
            let bytes = file(name)?;
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".png") {
                Some(KritaTip::Png(bytes))
            } else if lower.ends_with(".abr") {
                let tips = crate::abr::read(&bytes)?;
                let wanted = brush.attribute("name");
                let at = tips.iter().position(|t| Some(t.name.as_str()) == wanted).unwrap_or(0);
                tips.into_iter().nth(at).map(KritaTip::Image)
            } else {
                crate::gbr::read(&bytes)?.into_iter().next().map(KritaTip::Image)
            }
        }
    }
}

/// A rectangular tip drawn: `ratio` as tall as it is wide, solid to `hfade` and `vfade` of the way
/// out, fading to the edge.
fn rectangle(ratio: f32, hfade: f32, vfade: f32) -> AbrTip {
    let n = DRAWN_SIDE;
    let height = ((n as f32 * ratio).round() as usize).max(1);
    let edge = |t: f32, solid: f32| {
        if t <= solid {
            1.0
        } else if solid >= 1.0 {
            0.0
        } else {
            ((1.0 - t) / (1.0 - solid)).clamp(0.0, 1.0)
        }
    };
    let mut pixels = Vec::with_capacity(n * height);
    for y in 0..height {
        let ty = ((y as f32 + 0.5) / height as f32 * 2.0 - 1.0).abs();
        for x in 0..n {
            let tx = ((x as f32 + 0.5) / n as f32 * 2.0 - 1.0).abs();
            pixels.push((edge(tx, hfade) * edge(ty, vfade) * 255.0).round() as u8);
        }
    }
    AbrTip { name: String::new(), spacing: None, width: n as u32, height: height as u32, pixels }
}

/// A PNG's text chunks (`tEXt`, `zTXt`, `iTXt`), keyword and text; none if it isn't a PNG.
pub fn png_text(png: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if png.get(..8) != Some(&PNG[..]) {
        return out;
    }
    let mut at = 8usize;
    while let Some(len) = png.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes) {
        let Some(kind) = png.get(at + 4..at + 8) else { break };
        let len = len as usize;
        let Some(data) = png.get(at + 8..(at + 8).saturating_add(len)) else { break };
        at = at.saturating_add(12).saturating_add(len);
        let (keyword, rest) = match data.iter().position(|&b| b == 0) {
            Some(z) => (
                String::from_utf8_lossy(data.get(..z).unwrap_or_default()).into_owned(),
                data.get(z + 1..).unwrap_or_default(),
            ),
            None => continue,
        };
        // tEXt and zTXt are Latin-1, iTXt UTF-8
        let latin1 = |b: &[u8]| b.iter().map(|&b| char::from(b)).collect::<String>();
        let text = match kind {
            b"tEXt" => Some(latin1(rest)),
            b"zTXt" => rest.get(1..).and_then(inflate).map(|t| latin1(&t)),
            b"iTXt" => {
                // compressed or not, the method, then the language and translated keyword
                let (Some(&compressed), Some(body)) = (rest.first(), rest.get(2..)) else { continue };
                let mut parts = body.splitn(3, |&b| b == 0);
                let (_, _, text) = (parts.next(), parts.next(), parts.next().unwrap_or_default());
                let utf8 = if compressed == 1 { inflate(text) } else { Some(text.to_vec()) };
                utf8.map(|t| String::from_utf8_lossy(&t).into_owned())
            }
            b"IEND" => break,
            _ => None,
        };
        if let Some(t) = text {
            out.push((keyword, t));
        }
    }
    out
}

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, MOST_TEXT).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zip::ZipWriter;

    fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&[0; 4]);
        out
    }

    /// A preset's PNG: no picture to speak of, its XML in a compressed text chunk.
    fn kpp(xml: &str) -> Vec<u8> {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]));
        png.extend(chunk(b"tEXt", b"version\x005.0"));
        let mut z = b"preset\0\0".to_vec();
        z.extend(miniz_oxide::deflate::compress_to_vec_zlib(xml.as_bytes(), 6));
        png.extend(chunk(b"zTXt", &z));
        png.extend(chunk(b"IEND", &[]));
        png
    }

    fn preset(name: &str, brush: &str, pressure_size: bool) -> String {
        format!(
            r#"<Preset paintopid="paintbrush" name="{name}"><param type="string" name="PressureSize"><![CDATA[{pressure_size}]]></param><param type="string" name="PressureOpacity"><![CDATA[true]]></param><param type="string" name="brush_definition"><![CDATA[{brush}]]></param></Preset>"#
        )
    }

    #[test]
    fn an_auto_brush_preset_reads_as_a_round_tip() {
        let brush = r#"<Brush type="auto_brush" spacing="0.15" angle="0.5" useAutoSpacing="0"><MaskGenerator type="circle" diameter="40" ratio="0.5" hfade="0.2" vfade="0.4"/></Brush>"#;
        let b = read_preset(&kpp(&preset("Soft Ellipse", brush, false)), &|_| None).unwrap();
        assert_eq!(b.name, "Soft Ellipse");
        assert_eq!(b.spacing, Some(0.15));
        assert_eq!(b.angle, 0.5);
        assert!(!b.pressure_size && b.pressure_opacity);
        match b.tip {
            KritaTip::Round { hardness, roundness } => assert!((hardness - 0.3).abs() < 1e-6 && roundness == 0.5),
            t => panic!("{t:?}"),
        }
        // a rectangle is drawn, solid in the middle and gone at the edge
        let rect = r#"<Brush type="auto_brush" useAutoSpacing="1"><MaskGenerator type="rect" ratio="1" hfade="0.5" vfade="0.5"/></Brush>"#;
        let b = read_preset(&kpp(&preset("Square", rect, true)), &|_| None).unwrap();
        assert_eq!(b.spacing, None);
        let KritaTip::Image(t) = b.tip else { panic!() };
        assert_eq!(t.pixels[64 * 128 + 64], 255);
        assert!(t.pixels[64 * 128] < 10);
    }

    #[test]
    fn a_bundle_reads_its_presets_and_the_tips_they_name() {
        // a GIMP brush 2 × 1 for the tip
        let mut gbr = Vec::new();
        for v in [28u32 + 1, 2, 2, 1, 1] {
            gbr.extend_from_slice(&v.to_be_bytes());
        }
        gbr.extend_from_slice(b"GIMP");
        gbr.extend_from_slice(&10u32.to_be_bytes());
        gbr.push(0);
        gbr.extend_from_slice(&[255, 128]);
        let mut z = ZipWriter::new();
        z.add("mimetype", b"application/x-krita-resourcebundle");
        z.add("META-INF/manifest.xml", b"<manifest/>");
        z.add("meta.xml", b"<meta><dc:title>Charcoals</dc:title></meta>");
        z.add("brushes/Charcoal.gbr", &gbr);
        let named = r#"<Brush type="gbr_brush" filename="charcoal.gbr" spacing="0.3"/>"#;
        z.add("paintoppresets/charcoal.kpp", &kpp(&preset("Charcoal", named, true)));
        z.add("paintoppresets/broken.kpp", b"not a preset");
        let (title, presets) = read_bundle(&z.finish()).unwrap();
        assert_eq!(title, "Charcoals");
        assert_eq!(presets.len(), 1);
        let KritaTip::Image(t) = &presets[0].tip else { panic!("{:?}", presets[0].tip) };
        assert_eq!((t.width, t.height, t.pixels.as_slice()), (2, 1, &[255, 128][..]));
        assert_eq!(presets[0].spacing, Some(0.3));
        // a tip that isn't there: round
        let missing = r#"<Brush type="gbr_brush" filename="gone.gbr"/>"#;
        let b = read_preset(&kpp(&preset("Gone", missing, true)), &|_| None).unwrap();
        assert!(matches!(b.tip, KritaTip::Round { .. }));
    }

    #[test]
    fn a_texture_comes_from_the_preset_itself_or_the_bundle() {
        let round = r#"<Brush type="auto_brush"><MaskGenerator type="circle"/></Brush>"#;
        let with = |params: &str| {
            let xml = preset("Pencil", round, true);
            xml.replace("</Preset>", &format!("{params}</Preset>"))
        };
        let param =
            |name: &str, value: &str| format!(r#"<param type="string" name="{name}"><![CDATA[{value}]]></param>"#);
        let picture = [&PNG[..], b"rest of a picture"].concat();
        // kept in the preset as the base64 of its base64
        let twice = crate::base64::encode(crate::base64::encode(&picture).as_bytes());
        let xml = with(
            &[
                param("Texture/Pattern/Enabled", "true"),
                param("Texture/Pattern/Pattern", &twice),
                param("Texture/Pattern/Scale", "0.35"),
                param("Texture/Pattern/Strength", "0.8"),
                param("Texture/Pattern/Invert", "true"),
            ]
            .concat(),
        );
        let t = read_preset(&kpp(&xml), &|_| None).unwrap().texture.unwrap();
        assert_eq!((t.png, t.scale, t.strength, t.invert), (picture.clone(), 0.35, 0.8, true));
        // turned off: none
        let off = with(&[param("Texture/Pattern/Enabled", "false"), param("Texture/Pattern/Pattern", &twice)].concat());
        assert_eq!(read_preset(&kpp(&off), &|_| None).unwrap().texture, None);
        // not in the preset: the one it names, from the bundle's patterns
        let named =
            with(&[param("Texture/Pattern/Enabled", "true"), param("Texture/Pattern/Name", "paper.png")].concat());
        let mut z = ZipWriter::new();
        z.add("mimetype", b"application/x-krita-resourcebundle");
        z.add("patterns/Paper.png", &picture);
        z.add("paintoppresets/pencil.kpp", &kpp(&named));
        let (_, presets) = read_bundle(&z.finish()).unwrap();
        let t = presets[0].texture.as_ref().unwrap();
        assert_eq!((&t.png, t.scale, t.strength, t.invert), (&picture, 1.0, 1.0, false));
    }

    #[test]
    fn other_files_and_damaged_ones_are_not_brushes() {
        assert!(read_preset(b"nothing", &|_| None).is_none());
        assert!(read_bundle(b"nothing").is_none());
        let good = kpp(&preset("Ink", r#"<Brush type="auto_brush"><MaskGenerator type="circle"/></Brush>"#, true));
        crate::damaged::survives("Krita preset", &good, |b| {
            let _ = read_preset(b, &|_| None);
        });
    }
}
