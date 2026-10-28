//! Text layers in PSDs: the type tool's block (`TySh`), its descriptor and the text engine's data
//! inside it, read into editable text. Its first style and paragraph are taken (one look for all
//! of the text); text turned or slanted stays pixels, as there is no editable text like it.

use super::Reader;
use crate::document::{LayerText, TextAlign};

/// The editable text a type tool block describes, if it reads and can be held as text.
pub(super) fn layer_text(data: &[u8]) -> Option<LayerText> {
    let mut r = Reader::new(data);
    if r.u16()? != 1 {
        return None;
    }
    // from the text's own space to the canvas
    let mut t = [0.0f64; 6];
    for v in &mut t {
        *v = r.f64()?;
    }
    if r.u16()? != 50 || r.u32()? != 16 {
        return None;
    }
    let desc = descriptor(&mut r, 0)?;
    let vertical = matches!(desc.get(b"Ornt"), Some(Item::Enum(b"Vrtc")));
    // the box, in the text's space
    let Some(Item::Desc(bounds)) = desc.get(b"bounds") else { return None };
    let side = |key: &[u8]| match bounds.get(key) {
        Some(Item::Number(v)) => Some(*v),
        _ => None,
    };
    let (left, top, right) = (side(b"Left")?, side(b"Top ")?, side(b"Rght")?);
    let Some(Item::Data(engine)) = desc.get(b"EngineData") else { return None };
    let root = Engine::parse(engine)?;
    let editor = root.at(&["EngineDict", "Editor", "Text"])?.text()?;
    let text: String = editor.chars().map(|c| if c == '\r' || c == '\u{3}' { '\n' } else { c }).collect();
    let text = text.strip_suffix('\n').unwrap_or(&text).to_owned();
    if text.is_empty() {
        return None;
    }
    // only moved and evenly scaled: turned, slanted or mirrored text has no editable likeness
    let [xx, xy, yx, yy, tx, ty] = t;
    let plain = yy > 0.0 && (xx - yy).abs() <= 1e-3 * yy && xy.abs() <= 1e-4 * yy && yx.abs() <= 1e-4 * yy;
    if !plain || !yy.is_finite() {
        return None;
    }
    let scale = yy;
    let style = Style::of(&root);
    // a raised or lowered run is drawn at half the size
    let raised = style.number("FontBaseline").map_or(0, |v| v as i64);
    let size = style.number("FontSize").unwrap_or(12.0) * scale * if raised != 0 { 0.5 } else { 1.0 };
    let lift = match raised {
        1 => 0.7,
        2 => -0.5,
        _ => 0.0,
    };
    let shift = style.number("BaselineShift").unwrap_or(0.0) * scale;
    // a stroke without a fill: no colour of its own, so black
    let filled = style.flag("FillFlag").unwrap_or(true) || !style.flag("StrokeFlag").unwrap_or(false);
    let color = match style.get("FillColor").and_then(|c| c.at(&["Values"])) {
        Some(Engine::List(v)) if filled => {
            let c = |i: usize| v.get(i).and_then(Engine::number).unwrap_or(0.0) as f32;
            [c(1), c(2), c(3)]
        }
        _ => [0.0; 3],
    };
    let (font, face) = style.font(&root).map(|name| family(&name)).unwrap_or_default();
    let align = match paragraph(&root, "Justification").map_or(0, |v| v as i64) {
        0 => TextAlign::Left,
        1 => TextAlign::Right,
        2 => TextAlign::Center,
        _ => TextAlign::Justify,
    };
    let size = size as f32;
    Some(
        LayerText {
            text,
            font,
            style: face,
            size,
            // a thousandth of the size per unit of tracking, over 1.2
            tracking: (style.number("Tracking").unwrap_or(0.0) / 1200.0) as f32,
            baseline: if size > 0.0 { (shift as f32 + size * lift) / size } else { 0.0 },
            align,
            underline: style.flag("Underline").unwrap_or(false),
            vertical,
            color,
            origin: [(xx * left + tx) as f32, (yy * top + ty) as f32],
            // vertical text runs down from its corner unwrapped
            width: if vertical { 0.0 } else { ((right - left) * scale) as f32 },
            ..LayerText::default()
        }
        .sanitized(),
    )
}

/// A family and a style name from a font's PostScript name ("Arial-BoldMT": Arial, Bold).
fn family(postscript: &str) -> (String, String) {
    let (name, rest) = postscript.split_once('-').unwrap_or((postscript, ""));
    let name = name.strip_suffix("MT").filter(|n| !n.is_empty()).unwrap_or(name);
    let rest = rest.to_ascii_lowercase();
    let italic = rest.contains("italic") || rest.contains("oblique") || rest.ends_with("it");
    let weight = [
        ("thin", "Thin"),
        ("light", "Light"),
        ("medium", "Medium"),
        ("semibold", "Bold"),
        ("bold", "Bold"),
        ("heavy", "Black"),
        ("black", "Black"),
    ]
    .iter()
    .find(|(k, _)| rest.contains(k))
    .map(|(_, w)| *w);
    let style = match (weight, italic) {
        (Some(w), true) => format!("{w} Italic"),
        (Some(w), false) => w.to_owned(),
        (None, true) => "Italic".to_owned(),
        (None, false) => "Regular".to_owned(),
    };
    (name.to_owned(), style)
}

/// A setting of the text's first paragraph, or else of the normal paragraph style.
fn paragraph(root: &Engine, key: &str) -> Option<f64> {
    let first = root.at(&["EngineDict", "ParagraphRun", "RunArray"]).and_then(|l| l.item(0));
    let normal = || {
        let resources = root.get("ResourceDict").or_else(|| root.get("DocumentResources"))?;
        let at = resources.get("TheNormalParagraphSheet").and_then(Engine::number).unwrap_or(0.0) as usize;
        resources.get("ParagraphSheetSet")?.item(at)
    };
    first
        .and_then(|p| p.at(&["ParagraphSheet", "Properties", key]))
        .or_else(|| normal()?.at(&["Properties", key]))
        .and_then(Engine::number)
}

/// The look of the text's first run, falling back to the normal style for what it leaves out.
struct Style<'e> {
    run: Option<&'e Engine>,
    normal: Option<&'e Engine>,
}

impl<'e> Style<'e> {
    fn of(root: &'e Engine) -> Style<'e> {
        let run = root
            .at(&["EngineDict", "StyleRun", "RunArray"])
            .and_then(|l| l.item(0))
            .and_then(|r| r.at(&["StyleSheet", "StyleSheetData"]));
        let normal = root.get("ResourceDict").or_else(|| root.get("DocumentResources")).and_then(|res| {
            let at = res.get("TheNormalStyleSheet").and_then(Engine::number).unwrap_or(0.0) as usize;
            res.get("StyleSheetSet")?.item(at)?.get("StyleSheetData")
        });
        Style { run, normal }
    }

    fn get(&self, key: &str) -> Option<&'e Engine> {
        self.run.and_then(|r| r.get(key)).or_else(|| self.normal.and_then(|n| n.get(key)))
    }

    fn number(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Engine::number)
    }

    fn flag(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            Engine::Bool(b) => Some(*b),
            Engine::Number(n) => Some(*n != 0.0),
            _ => None,
        }
    }

    /// The font's PostScript name.
    fn font(&self, root: &Engine) -> Option<String> {
        let at = self.number("Font")? as usize;
        let resources = root.get("ResourceDict").or_else(|| root.get("DocumentResources"))?;
        Some(resources.get("FontSet")?.item(at)?.get("Name")?.text()?.to_owned()).filter(|n| !n.is_empty())
    }
}

/// How deep descriptors and the engine's data may nest.
const DEEPEST: usize = 64;

/// A descriptor: its items by key.
struct Desc<'a> {
    items: Vec<(&'a [u8], Item<'a>)>,
}

impl<'a> Desc<'a> {
    fn get(&self, key: &[u8]) -> Option<&Item<'a>> {
        self.items.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }
}

/// A descriptor item's value, as far as text needs one.
enum Item<'a> {
    Desc(Desc<'a>),
    Number(f64),
    Enum(&'a [u8]),
    Data(&'a [u8]),
    Other,
}

fn descriptor<'a>(r: &mut Reader<'a>, depth: usize) -> Option<Desc<'a>> {
    if depth > DEEPEST {
        return None;
    }
    unicode(r)?;
    id(r)?;
    let count = r.u32()?;
    let mut items = Vec::new();
    for _ in 0..count {
        let key = id(r)?;
        let kind = r.take(4)?;
        items.push((key, value(r, kind, depth)?));
    }
    Some(Desc { items })
}

fn value<'a>(r: &mut Reader<'a>, kind: &[u8], depth: usize) -> Option<Item<'a>> {
    if depth > DEEPEST {
        return None;
    }
    Some(match kind {
        b"Objc" | b"GlbO" => Item::Desc(descriptor(r, depth + 1)?),
        b"VlLs" => {
            for _ in 0..r.u32()? {
                let kind = r.take(4)?;
                value(r, kind, depth + 1)?;
            }
            Item::Other
        }
        b"doub" => Item::Number(r.f64()?),
        b"UntF" => {
            r.take(4)?;
            Item::Number(r.f64()?)
        }
        b"UnFl" => {
            r.take(4)?;
            let n = r.u32()? as usize;
            r.take(n.checked_mul(8)?)?;
            Item::Other
        }
        b"TEXT" => {
            unicode(r)?;
            Item::Other
        }
        b"enum" => {
            id(r)?;
            Item::Enum(id(r)?)
        }
        b"long" => Item::Number(f64::from(r.i32()?)),
        b"comp" => {
            r.take(8)?;
            Item::Other
        }
        b"bool" => Item::Number(f64::from(r.u8()?)),
        b"type" | b"GlbC" => {
            unicode(r)?;
            id(r)?;
            Item::Other
        }
        b"alis" | b"tdta" | b"Pth " => {
            let n = r.u32()? as usize;
            Item::Data(r.take(n)?)
        }
        b"obj " => {
            for _ in 0..r.u32()? {
                match r.take(4)? {
                    b"prop" => {
                        unicode(r)?;
                        id(r)?;
                        id(r)?;
                    }
                    b"Enmr" => {
                        unicode(r)?;
                        id(r)?;
                        id(r)?;
                        id(r)?;
                    }
                    b"Clss" => {
                        unicode(r)?;
                        id(r)?;
                    }
                    b"rele" => {
                        unicode(r)?;
                        id(r)?;
                        r.take(4)?;
                    }
                    b"Idnt" | b"indx" => {
                        r.take(4)?;
                    }
                    b"name" => {
                        unicode(r)?;
                        id(r)?;
                        unicode(r)?;
                    }
                    _ => return None,
                }
            }
            Item::Other
        }
        _ => return None,
    })
}

/// A Unicode string: its length in UTF-16 units, then them.
fn unicode(r: &mut Reader<'_>) -> Option<String> {
    let n = r.u32()? as usize;
    let units: Vec<u16> =
        r.take(n.checked_mul(2)?)?.as_chunks::<2>().0.iter().map(|&u| u16::from_be_bytes(u)).collect();
    Some(String::from_utf16_lossy(&units).trim_end_matches('\0').to_owned())
}

/// A key or class: four bytes, or as many as its length says.
fn id<'a>(r: &mut Reader<'a>) -> Option<&'a [u8]> {
    let n = r.u32()? as usize;
    r.take(if n == 0 { 4 } else { n })
}

/// The text engine's data: dictionaries, lists, numbers, flags and strings, written out as text.
enum Engine {
    Dict(Vec<(String, Engine)>),
    List(Vec<Engine>),
    Number(f64),
    Bool(bool),
    Text(String),
    Other,
}

impl Engine {
    fn parse(bytes: &[u8]) -> Option<Engine> {
        let mut p = Parser { bytes, at: 0 };
        p.value(0)
    }

    fn get(&self, key: &str) -> Option<&Engine> {
        match self {
            Engine::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn at(&self, path: &[&str]) -> Option<&Engine> {
        path.iter().try_fold(self, |e, k| e.get(k))
    }

    fn item(&self, i: usize) -> Option<&Engine> {
        match self {
            Engine::List(items) => items.get(i),
            _ => None,
        }
    }

    fn number(&self) -> Option<f64> {
        match self {
            Engine::Number(n) => Some(*n),
            _ => None,
        }
    }

    fn text(&self) -> Option<&str> {
        match self {
            Engine::Text(s) => Some(s),
            _ => None,
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace() || b == 0) {
            self.at += 1;
        }
    }

    fn starts(&self, s: &[u8]) -> bool {
        self.bytes.get(self.at..).is_some_and(|rest| rest.starts_with(s))
    }

    /// A run of letters, digits and the like (a name, number or word).
    fn word(&mut self) -> &[u8] {
        let from = self.at;
        while self.peek().is_some_and(|b| !b.is_ascii_whitespace() && !b"()<>[]/".contains(&b) && b != 0) {
            self.at += 1;
        }
        self.bytes.get(from..self.at).unwrap_or_default()
    }

    fn value(&mut self, depth: usize) -> Option<Engine> {
        if depth > DEEPEST {
            return None;
        }
        self.skip_space();
        if self.starts(b"<<") {
            self.at += 2;
            let mut items = Vec::new();
            loop {
                self.skip_space();
                if self.starts(b">>") {
                    self.at += 2;
                    return Some(Engine::Dict(items));
                }
                if self.peek()? != b'/' {
                    return None;
                }
                self.at += 1;
                let key = String::from_utf8_lossy(self.word()).into_owned();
                items.push((key, self.value(depth + 1)?));
            }
        }
        match self.peek()? {
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_space();
                    if self.peek()? == b']' {
                        self.at += 1;
                        return Some(Engine::List(items));
                    }
                    items.push(self.value(depth + 1)?);
                }
            }
            b'(' => {
                self.at += 1;
                let mut raw = Vec::new();
                loop {
                    let b = self.peek()?;
                    self.at += 1;
                    match b {
                        b'\\' => {
                            raw.push(self.peek()?);
                            self.at += 1;
                        }
                        b')' => break,
                        _ => raw.push(b),
                    }
                }
                Some(Engine::Text(decoded(&raw)))
            }
            b'/' => {
                self.at += 1;
                self.word();
                Some(Engine::Other)
            }
            _ => {
                let word = self.word();
                if word.is_empty() {
                    return None;
                }
                Some(match word {
                    b"true" => Engine::Bool(true),
                    b"false" => Engine::Bool(false),
                    _ => std::str::from_utf8(word)
                        .ok()
                        .and_then(|w| w.parse().ok())
                        .map_or(Engine::Other, Engine::Number),
                })
            }
        }
    }
}

/// A string's bytes as text: UTF-16 after its byte order mark, or else one byte a letter.
fn decoded(raw: &[u8]) -> String {
    match raw.strip_prefix(&[0xFE, 0xFF]) {
        Some(rest) => {
            let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|&u| u16::from_be_bytes(u)).collect();
            String::from_utf16_lossy(&units)
        }
        None => raw.iter().map(|&b| char::from(b)).collect(),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// A type tool block for `text` at `size` in `font`, moved by (`tx`, `ty`) and scaled by
    /// `scale`, in a box from (`left`, `top`) `width` wide, coloured `rgb`, with `justification`.
    pub(crate) fn type_tool(
        text: &str,
        font: &str,
        size: f64,
        rgb: [f64; 3],
        justification: u8,
        [xx, xy, yx, yy, tx, ty]: [f64; 6],
        [left, top, right, bottom]: [f64; 4],
        vertical: bool,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(1u16.to_be_bytes());
        for v in [xx, xy, yx, yy, tx, ty] {
            out.extend(v.to_be_bytes());
        }
        out.extend(50u16.to_be_bytes());
        out.extend(16u32.to_be_bytes());
        // the descriptor: no name, class TxLr
        let mut d = Vec::new();
        unicode_into(&mut d, "");
        key_into(&mut d, b"TxLr");
        d.extend(5u32.to_be_bytes());
        key_into(&mut d, b"Txt ");
        d.extend(b"TEXT");
        unicode_into(&mut d, text);
        key_into(&mut d, b"Ornt");
        d.extend(b"enum");
        key_into(&mut d, b"Ornt");
        key_into(&mut d, if vertical { b"Vrtc" } else { b"Hrzn" });
        // a list, read past
        key_into(&mut d, b"warpRotate".as_slice());
        d.extend(b"VlLs");
        d.extend(2u32.to_be_bytes());
        d.extend(b"long");
        d.extend(7i32.to_be_bytes());
        d.extend(b"doub");
        d.extend(1.5f64.to_be_bytes());
        key_into(&mut d, b"bounds".as_slice());
        d.extend(b"Objc");
        unicode_into(&mut d, "");
        key_into(&mut d, b"bounds".as_slice());
        d.extend(4u32.to_be_bytes());
        for (k, v) in [(b"Left", left), (b"Top ", top), (b"Rght", right), (b"Btom", bottom)] {
            key_into(&mut d, k);
            d.extend(b"UntF#Pnt");
            d.extend(v.to_be_bytes());
        }
        // the strings stand in as @T@ (the text), @A@ and @B@ (the fonts)
        let [r, g, b] = rgb;
        let engine = format!(
            r#"

<<
	/EngineDict
	<<
		/Editor << /Text (@T@) >>
		/ParagraphRun
		<<
			/RunArray [ << /ParagraphSheet << /DefaultStyleSheet 0 /Properties << /Justification {justification} >> >> >> ]
		>>
		/StyleRun
		<<
			/RunArray [
			<<
				/StyleSheet
				<<
					/StyleSheetData
					<<
						/Font 1
						/FontSize {size}
						/Tracking 120
						/AutoKerning true
						/FillColor << /Type 1 /Values [ 1.0 {r} {g} {b} ] >>
					>>
				>>
			>>
			]
			/RunLengthArray [ 6 ]
		>>
		/AntiAlias 4
	>>
	/ResourceDict
	<<
		/FontSet [ << /Name (@A@) /Script 0 >> << /Name (@B@) /Script 0 >> ]
		/StyleSheetSet [ << /Name (Normal) /StyleSheetData << /Font 0 /FontSize 12.0 /Underline true >> >> ]
		/SuperscriptSize .583
	>>
>>"#
        );
        // the strings in UTF-16 after a byte order mark, parentheses escaped
        let utf16 = |s: &str| -> Vec<u8> {
            let mut b = vec![0xFE, 0xFF];
            for u in s.encode_utf16() {
                for byte in u.to_be_bytes() {
                    if byte == b'(' || byte == b')' || byte == b'\\' {
                        b.push(b'\\');
                    }
                    b.push(byte);
                }
            }
            b
        };
        let mut data = Vec::new();
        let mut rest = engine.as_str();
        while let Some(at) = rest.find('@') {
            data.extend(rest[..at].bytes());
            let (marker, after) = rest[at..].split_at(3);
            data.extend(match marker {
                "@T@" => utf16(&format!("{}\r", text.replace('\n', "\r"))),
                "@A@" => utf16("AdobeInvisFont"),
                _ => utf16(font),
            });
            rest = after;
        }
        data.extend(rest.bytes());
        key_into(&mut d, b"EngineData".as_slice());
        d.extend(b"tdta");
        d.extend((data.len() as u32).to_be_bytes());
        d.extend(data);
        out.extend(d);
        // the warp, and the box again
        out.extend(1u16.to_be_bytes());
        out.extend(16u32.to_be_bytes());
        unicode_into(&mut out, "");
        key_into(&mut out, b"warp");
        out.extend(0u32.to_be_bytes());
        for _ in 0..4 {
            out.extend(0f64.to_be_bytes());
        }
        out
    }

    fn unicode_into(out: &mut Vec<u8>, s: &str) {
        let units: Vec<u16> = s.encode_utf16().chain([0]).collect();
        out.extend((units.len() as u32).to_be_bytes());
        for u in units {
            out.extend(u.to_be_bytes());
        }
    }

    fn key_into(out: &mut Vec<u8>, key: &[u8]) {
        if key.len() == 4 {
            out.extend(0u32.to_be_bytes());
        } else {
            out.extend((key.len() as u32).to_be_bytes());
        }
        out.extend(key);
    }

    const MOVED: [f64; 6] = [2.0, 0.0, 0.0, 2.0, 10.0, 40.0];

    #[test]
    fn text_comes_in_with_its_font_size_colour_box_and_alignment() {
        let block = type_tool(
            "Hi (there)\nTwo",
            "Arial-BoldMT",
            24.0,
            [1.0, 0.5, 0.0],
            2,
            MOVED,
            [0.0, -20.0, 100.0, 5.0],
            false,
        );
        let t = layer_text(&block).unwrap();
        assert_eq!(t.text, "Hi (there)\nTwo");
        assert_eq!((t.font.as_str(), t.style.as_str()), ("Arial", "Bold"));
        assert!((t.size - 48.0).abs() < 1e-4);
        assert_eq!(t.color, [1.0, 0.5, 0.0]);
        assert_eq!(t.align, TextAlign::Center);
        assert_eq!(t.origin, [10.0, 0.0]);
        assert!((t.width - 200.0).abs() < 1e-4);
        assert!((t.tracking - 0.1).abs() < 1e-6);
        // what the run leaves out comes from the normal style
        assert!(t.underline);
        assert!(!t.vertical && t.baseline == 0.0);
    }

    #[test]
    fn turned_text_and_broken_blocks_are_not_text() {
        let turned = [0.0, 1.0, -1.0, 0.0, 10.0, 40.0];
        assert!(
            layer_text(&type_tool("Hi", "F", 24.0, [0.0; 3], 0, turned, [0.0, -20.0, 100.0, 5.0], false)).is_none()
        );
        let empty = type_tool("", "F", 24.0, [0.0; 3], 0, MOVED, [0.0, -20.0, 100.0, 5.0], false);
        assert!(layer_text(&empty).is_none());
        let good = type_tool("Hi", "F", 24.0, [0.0; 3], 1, MOVED, [0.0, -20.0, 100.0, 5.0], true);
        let t = layer_text(&good).unwrap();
        assert!(t.vertical && t.width == 0.0 && t.align == TextAlign::Right);
        assert_eq!((t.font.as_str(), t.style.as_str()), ("F", "Regular"));
        for n in 0..good.len() {
            let _ = layer_text(&good[..n]);
        }
        let mut bad = good.clone();
        for i in (0..bad.len()).step_by(7) {
            bad[i] ^= 0xA5;
            let _ = layer_text(&bad);
        }
    }

    #[test]
    fn engine_data_reads_nested_values_and_survives_deep_nesting() {
        let e = Engine::parse(b"<< /A [ 1 -2.5 .5 true (x\\)y) /Name ] /B << /C false >> >>").unwrap();
        let a = e.get("A").unwrap();
        assert_eq!(a.item(1).and_then(Engine::number), Some(-2.5));
        assert_eq!(a.item(2).and_then(Engine::number), Some(0.5));
        assert_eq!(a.item(4).and_then(Engine::text), Some("x)y"));
        assert!(matches!(e.at(&["B", "C"]), Some(Engine::Bool(false))));
        let deep = "[".repeat(10_000);
        assert!(Engine::parse(deep.as_bytes()).is_none());
        assert!(Engine::parse(b"<< /A (unended").is_none());
        assert_eq!(family("MyriadPro-BoldIt"), ("MyriadPro".to_owned(), "Bold Italic".to_owned()));
        assert_eq!(family("ArialMT"), ("Arial".to_owned(), "Regular".to_owned()));
        assert_eq!(family("Helvetica-LightOblique"), ("Helvetica".to_owned(), "Light Italic".to_owned()));
    }
}
