//! A baseline JPEG encoder (8-bit, YCbCr 4:4:4, the standard's example tables), for pictures
//! inside the files we write (a PDF's pages).

/// The standard's example quantisation tables (luminance, chrominance), in row order.
const LUMA_Q: [u8; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56, 14, 17, 22, 29, 51,
    87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92, 49, 64, 78, 87, 103, 121, 120, 101,
    72, 92, 95, 98, 112, 100, 103, 99,
];
const CHROMA_Q: [u8; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99, 47, 66, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99,
];

/// Where each coefficient goes in the zigzag order.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21,
    28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54,
    47, 55, 62, 63,
];

// the standard's example Huffman tables: code counts per length 1..16, then the symbols
const DC_LUMA_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_CHROMA_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_SYMBOLS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const AC_LUMA_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7D];
const AC_LUMA_SYMBOLS: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14,
    0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09,
    0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A,
    0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65,
    0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88,
    0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9,
    0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA,
    0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA,
    0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA,
];
const AC_CHROMA_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
const AC_CHROMA_SYMBOLS: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13, 0x22, 0x32,
    0x81, 0x08, 0x14, 0x42, 0x91, 0xA1, 0xB1, 0xC1, 0x09, 0x23, 0x33, 0x52, 0xF0, 0x15, 0x62, 0x72, 0xD1, 0x0A, 0x16,
    0x24, 0x34, 0xE1, 0x25, 0xF1, 0x17, 0x18, 0x19, 0x1A, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x35, 0x36, 0x37, 0x38, 0x39,
    0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64,
    0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x82, 0x83, 0x84, 0x85, 0x86,
    0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7,
    0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8,
    0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9,
    0xEA, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA,
];

/// A Huffman table as (code, length) per symbol.
struct Codes([(u16, u8); 256]);

impl Codes {
    fn new(bits: &[u8; 16], symbols: &[u8]) -> Codes {
        let mut table = [(0u16, 0u8); 256];
        let mut code = 0u16;
        let mut k = 0;
        for (len, &count) in bits.iter().enumerate() {
            for _ in 0..count {
                if let Some(t) = symbols.get(k).and_then(|&s| table.get_mut(usize::from(s))) {
                    *t = (code, len as u8 + 1);
                }
                code = code.wrapping_add(1);
                k += 1;
            }
            code <<= 1;
        }
        Codes(table)
    }

    fn get(&self, symbol: u8) -> (u16, u8) {
        self.0.get(usize::from(symbol)).copied().unwrap_or((0, 0))
    }
}

/// Bits written most significant first, 0xFF bytes stuffed with a zero.
struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn put(&mut self, value: u16, len: u8) {
        if len == 0 {
            return;
        }
        self.acc = (self.acc << len) | (u32::from(value) & ((1 << len) - 1));
        self.n += u32::from(len);
        while self.n >= 8 {
            let byte = (self.acc >> (self.n - 8)) as u8;
            self.out.push(byte);
            if byte == 0xFF {
                self.out.push(0);
            }
            self.n -= 8;
        }
        self.acc &= (1 << self.n) - 1;
    }

    /// Pads the last byte with ones.
    fn flush(&mut self) {
        if self.n > 0 {
            let pad = 8 - self.n as u8;
            self.put((1 << pad) - 1, pad);
        }
    }
}

/// The 8 × 8 forward DCT of `block` (values centred on 0), rows then columns.
fn dct(block: &mut [f32; 64]) {
    let mut cos = [[0f32; 8]; 8];
    for (u, row) in cos.iter_mut().enumerate() {
        let c = if u == 0 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 };
        for (x, v) in row.iter_mut().enumerate() {
            *v = 0.5 * c * (((2 * x + 1) as f32) * (u as f32) * std::f32::consts::PI / 16.0).cos();
        }
    }
    let mut tmp = [0f32; 64];
    for y in 0..8 {
        for (u, row) in cos.iter().enumerate() {
            let mut s = 0.0;
            for (x, c) in row.iter().enumerate() {
                s += block.get(y * 8 + x).copied().unwrap_or(0.0) * c;
            }
            if let Some(t) = tmp.get_mut(y * 8 + u) {
                *t = s;
            }
        }
    }
    for u in 0..8 {
        for (v, row) in cos.iter().enumerate() {
            let mut s = 0.0;
            for (y, c) in row.iter().enumerate() {
                s += tmp.get(y * 8 + u).copied().unwrap_or(0.0) * c;
            }
            if let Some(b) = block.get_mut(v * 8 + u) {
                *b = s;
            }
        }
    }
}

/// A quantisation table scaled for `quality` (1..100), in row order.
fn scaled(table: &[u8; 64], quality: u8) -> [u8; 64] {
    let q = u32::from(quality.clamp(1, 100));
    let scale = if q < 50 { 5000 / q } else { 200 - 2 * q };
    table.map(|v| ((u32::from(v) * scale + 50) / 100).clamp(1, 255) as u8)
}

/// The number of bits `v` needs, and those bits as JPEG writes a coefficient.
fn magnitude(v: i32) -> (u8, u16) {
    let a = v.unsigned_abs();
    let size = (32 - a.leading_zeros()) as u8;
    // a negative value is written as its ones' complement in `size` bits
    let raw = if v < 0 { (v - 1) as u32 } else { v as u32 };
    (size, (raw & ((1u32 << size) - 1)) as u16)
}

/// How a JPEG's components are kept.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Luminance and two colour differences, from RGB.
    Ycc,
    /// Four inks, each inverted, as print programs keep CMYK JPEGs (with their marker saying so).
    Cmyk,
}

/// `rgb` (`width` × `height`, 3 bytes a pixel, top row first) as a baseline JPEG at `quality`
/// (1..100).
pub fn encode(rgb: &[u8], width: u32, height: u32, quality: u8) -> Vec<u8> {
    encode_with(Kind::Ycc, width, height, quality, |at| {
        let (r, g, b) = match rgb.get(at * 3..at * 3 + 3) {
            Some(&[r, g, b]) => (f32::from(r), f32::from(g), f32::from(b)),
            _ => (0.0, 0.0, 0.0),
        };
        [
            0.299 * r + 0.587 * g + 0.114 * b - 128.0,
            -0.168_736 * r - 0.331_264 * g + 0.5 * b,
            0.5 * r - 0.418_688 * g - 0.081_312 * b,
            0.0,
        ]
    })
}

/// `cmyk` (`width` × `height`, 4 bytes a pixel: cyan, magenta, yellow and black ink, 0 for none)
/// as a baseline JPEG at `quality` (1..100), each ink inverted under an Adobe marker as print
/// programs keep them (a PDF reading it says `/Decode [1 0 1 0 1 0 1 0]`).
pub fn encode_cmyk(cmyk: &[u8], width: u32, height: u32, quality: u8) -> Vec<u8> {
    encode_with(Kind::Cmyk, width, height, quality, |at| {
        let ink = |k: usize| f32::from(cmyk.get(at * 4 + k).copied().unwrap_or(0));
        [127.0 - ink(0), 127.0 - ink(1), 127.0 - ink(2), 127.0 - ink(3)]
    })
}

/// The JPEG of a `width` × `height` picture whose components (level-shifted: −128..127) at each
/// pixel `pixel` gives, top row first.
fn encode_with(kind: Kind, width: u32, height: u32, quality: u8, pixel: impl Fn(usize) -> [f32; 4]) -> Vec<u8> {
    let (w, h) = (width.clamp(1, 65535) as usize, height.clamp(1, 65535) as usize);
    let n = if kind == Kind::Cmyk { 4 } else { 3 };
    // the tables (quantisation and Huffman) each component uses: inks all use the first
    let table_of = |c: usize| usize::from(kind == Kind::Ycc && c > 0);
    let tables = [scaled(&LUMA_Q, quality), scaled(&CHROMA_Q, quality)];
    let mut out = vec![0xFF, 0xD8];
    match kind {
        // JFIF header
        Kind::Ycc => out.extend_from_slice(&[0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0]),
        // the Adobe marker: version 100, no flags, no colour transform
        Kind::Cmyk => out.extend_from_slice(&[0xFF, 0xEE, 0, 14, b'A', b'd', b'o', b'b', b'e', 0, 100, 0, 0, 0, 0, 0]),
    }
    for (id, t) in tables.iter().enumerate() {
        out.extend_from_slice(&[0xFF, 0xDB, 0, 67, id as u8]);
        out.extend(ZIGZAG.iter().map(|&i| t.get(i).copied().unwrap_or(1)));
    }
    out.extend_from_slice(&[0xFF, 0xC0]);
    out.extend_from_slice(&(8 + 3 * n as u16).to_be_bytes());
    out.push(8);
    out.extend_from_slice(&(h as u16).to_be_bytes());
    out.extend_from_slice(&(w as u16).to_be_bytes());
    out.push(n as u8);
    for c in 0..n {
        out.extend_from_slice(&[c as u8 + 1, 0x11, table_of(c) as u8]);
    }
    for (class_id, bits, symbols) in [
        (0x00u8, &DC_LUMA_BITS, &DC_SYMBOLS[..]),
        (0x10, &AC_LUMA_BITS, &AC_LUMA_SYMBOLS[..]),
        (0x01, &DC_CHROMA_BITS, &DC_SYMBOLS[..]),
        (0x11, &AC_CHROMA_BITS, &AC_CHROMA_SYMBOLS[..]),
    ] {
        let len = (3 + 16 + symbols.len()) as u16;
        out.extend_from_slice(&[0xFF, 0xC4]);
        out.extend_from_slice(&len.to_be_bytes());
        out.push(class_id);
        out.extend_from_slice(bits);
        out.extend_from_slice(symbols);
    }
    out.extend_from_slice(&[0xFF, 0xDA]);
    out.extend_from_slice(&(6 + 2 * n as u16).to_be_bytes());
    out.push(n as u8);
    for c in 0..n {
        let t = table_of(c) as u8;
        out.extend_from_slice(&[c as u8 + 1, (t << 4) | t]);
    }
    out.extend_from_slice(&[0, 63, 0]);

    let dc = [Codes::new(&DC_LUMA_BITS, &DC_SYMBOLS), Codes::new(&DC_CHROMA_BITS, &DC_SYMBOLS)];
    let ac = [Codes::new(&AC_LUMA_BITS, &AC_LUMA_SYMBOLS), Codes::new(&AC_CHROMA_BITS, &AC_CHROMA_SYMBOLS)];
    let mut bits = Bits { out, acc: 0, n: 0 };
    let mut previous = [0i32; 4];
    for by in (0..h).step_by(8) {
        for bx in (0..w).step_by(8) {
            // the block's components, edges repeated past the picture
            let mut blocks = [[0f32; 64]; 4];
            for k in 0..64 {
                let (sx, sy) = ((bx + k % 8).min(w - 1), (by + k / 8).min(h - 1));
                for (block, value) in blocks.iter_mut().zip(pixel(sy * w + sx)) {
                    if let Some(slot) = block.get_mut(k) {
                        *slot = value;
                    }
                }
            }
            for ((c, block), last) in blocks.iter_mut().enumerate().take(n).zip(previous.iter_mut()) {
                let t = table_of(c);
                let (Some(dc), Some(ac)) = (dc.get(t), ac.get(t)) else { continue };
                dct(block);
                let q = tables.get(t).copied().unwrap_or([1; 64]);
                let coeff: Vec<i32> = ZIGZAG
                    .iter()
                    .map(|&i| {
                        (block.get(i).copied().unwrap_or(0.0) / f32::from(q.get(i).copied().unwrap_or(1))).round()
                            as i32
                    })
                    .collect();
                let first = coeff.first().copied().unwrap_or(0);
                let (size, value) = magnitude(first - *last);
                *last = first;
                let (code, len) = dc.get(size);
                bits.put(code, len);
                bits.put(value, size);
                let mut zeros = 0u8;
                for &v in coeff.get(1..).unwrap_or(&[]) {
                    if v == 0 {
                        zeros += 1;
                        continue;
                    }
                    while zeros >= 16 {
                        let (code, len) = ac.get(0xF0);
                        bits.put(code, len);
                        zeros -= 16;
                    }
                    let (size, value) = magnitude(v);
                    let (code, len) = ac.get((zeros << 4) | size);
                    bits.put(code, len);
                    bits.put(value, size);
                    zeros = 0;
                }
                if zeros > 0 {
                    let (code, len) = ac.get(0x00);
                    bits.put(code, len);
                }
            }
        }
    }
    bits.flush();
    let mut out = bits.out;
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_encodes_to_a_well_formed_file() {
        let (w, h) = (37u32, 21u32);
        let rgb: Vec<u8> = (0..w * h).flat_map(|i| [(i % 256) as u8, (i * 3 % 256) as u8, 128]).collect();
        let jpg = encode(&rgb, w, h, 85);
        assert_eq!(&jpg[..2], &[0xFF, 0xD8]);
        assert_eq!(&jpg[jpg.len() - 2..], &[0xFF, 0xD9]);
        // the frame header gives the size
        let sof = jpg.windows(2).position(|m| m == [0xFF, 0xC0]).unwrap();
        assert_eq!(u16::from_be_bytes([jpg[sof + 5], jpg[sof + 6]]), 21);
        assert_eq!(u16::from_be_bytes([jpg[sof + 7], jpg[sof + 8]]), 37);
        // lower quality, smaller file
        assert!(encode(&rgb, w, h, 20).len() < jpg.len());
        // if asked to, a sample is written for a decoder to check
        if let Ok(out) = std::env::var("JPEG_SAMPLE_OUT") {
            std::fs::write(out, &jpg).unwrap();
        }
    }

    #[test]
    fn inks_encode_too() {
        let (w, h) = (19u32, 11u32);
        let cmyk: Vec<u8> = (0..w * h).flat_map(|i| [(i % 256) as u8, 0, 200, 30]).collect();
        let jpg = encode_cmyk(&cmyk, w, h, 90);
        // the Adobe marker, and four components in the frame
        assert!(jpg.windows(7).any(|m| m == [0xFF, 0xEE, 0, 14, b'A', b'd', b'o']));
        let sof = jpg.windows(2).position(|m| m == [0xFF, 0xC0]).unwrap();
        assert_eq!(jpg[sof + 9], 4);
        if let Ok(out) = std::env::var("JPEG_CMYK_SAMPLE_OUT") {
            std::fs::write(out, &jpg).unwrap();
        }
    }

    #[test]
    fn coefficients_take_the_bits_they_need() {
        assert_eq!(magnitude(0), (0, 0));
        assert_eq!(magnitude(1), (1, 1));
        assert_eq!(magnitude(-1), (1, 0));
        assert_eq!(magnitude(-3), (2, 0));
        assert_eq!(magnitude(5), (3, 5));
    }
}
