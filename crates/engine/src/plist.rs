//! Binary property lists (`bplist00`): reading them into [`Value`]s and writing them back. Enough
//! of the format for keyed archives: no 128-bit integers, sets or legacy types.

/// A property list value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    /// Seconds since 2001-01-01.
    Date(f64),
    Data(Vec<u8>),
    String(String),
    /// A reference to another object of a keyed archive.
    Uid(u64),
    Array(Vec<Value>),
    /// Keys in the order they were read (or are to be written).
    Dict(Vec<(String, Value)>),
}

impl Value {
    /// The value of `key`, if this is a dictionary that has it.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Real(r) if r.is_finite() => Some(*r as i64),
            Value::Bool(b) => Some(i64::from(*b)),
            _ => None,
        }
    }

    pub fn as_real(&self) -> Option<f64> {
        match self {
            Value::Real(r) => Some(*r),
            Value::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Int(i) => Some(*i != 0),
            _ => None,
        }
    }

    pub fn as_data(&self) -> Option<&[u8]> {
        match self {
            Value::Data(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_uid(&self) -> Option<u64> {
        match self {
            Value::Uid(u) => Some(*u),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
}

/// Deepest nesting read: a list deeper than this is refused rather than followed.
const MAX_DEPTH: usize = 64;
/// Most objects a list may claim to hold.
const MAX_OBJECTS: u64 = 4_000_000;

/// Reads a binary property list; `None` if it isn't one or is damaged.
pub fn read(bytes: &[u8]) -> Option<Value> {
    if bytes.get(..8)? != b"bplist00" {
        return None;
    }
    let trailer = bytes.get(bytes.len().checked_sub(32)?..)?;
    let offset_size = usize::from(*trailer.get(6)?);
    let ref_size = usize::from(*trailer.get(7)?);
    let count = be(trailer.get(8..16)?)?;
    let top = be(trailer.get(16..24)?)?;
    let table = be(trailer.get(24..32)?)?;
    if !(1..=8).contains(&offset_size) || !(1..=8).contains(&ref_size) || count > MAX_OBJECTS || top >= count {
        return None;
    }
    let reader = Reader { bytes, offset_size, ref_size, count, table: usize::try_from(table).ok()? };
    reader.object(top, 0)
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset_size: usize,
    ref_size: usize,
    count: u64,
    table: usize,
}

impl Reader<'_> {
    fn offset_of(&self, object: u64) -> Option<usize> {
        if object >= self.count {
            return None;
        }
        let at = self.table.checked_add(usize::try_from(object).ok()?.checked_mul(self.offset_size)?)?;
        usize::try_from(be(self.bytes.get(at..at.checked_add(self.offset_size)?)?)?).ok()
    }

    /// The length that follows a marker whose low nibble is `low`, and where the content starts.
    fn length(&self, at: usize, low: u8) -> Option<(usize, usize)> {
        if low != 0x0F {
            return Some((usize::from(low), at + 1));
        }
        let marker = *self.bytes.get(at + 1)?;
        if marker >> 4 != 0x1 {
            return None;
        }
        let size = 1usize << (marker & 0x0F);
        let len = usize::try_from(be(self.bytes.get(at + 2..at + 2 + size)?)?).ok()?;
        Some((len, at + 2 + size))
    }

    fn reference(&self, at: usize, i: usize) -> Option<u64> {
        let start = at.checked_add(i.checked_mul(self.ref_size)?)?;
        be(self.bytes.get(start..start.checked_add(self.ref_size)?)?)
    }

    fn object(&self, object: u64, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        let at = self.offset_of(object)?;
        let marker = *self.bytes.get(at)?;
        let (kind, low) = (marker >> 4, marker & 0x0F);
        Some(match kind {
            0x0 => match low {
                0x8 => Value::Bool(false),
                0x9 => Value::Bool(true),
                _ => Value::Null,
            },
            0x1 => {
                let size = 1usize.checked_shl(u32::from(low))?;
                let raw = self.bytes.get(at + 1..at + 1 + size)?;
                // 8-byte integers are signed; shorter ones are unsigned
                let v = be(raw)?;
                Value::Int(if size == 8 { v as i64 } else { i64::try_from(v).ok()? })
            }
            0x2 => match low {
                2 => Value::Real(f64::from(f32::from_be_bytes(self.bytes.get(at + 1..at + 5)?.try_into().ok()?))),
                3 => Value::Real(f64::from_be_bytes(self.bytes.get(at + 1..at + 9)?.try_into().ok()?)),
                _ => return None,
            },
            0x3 => Value::Date(f64::from_be_bytes(self.bytes.get(at + 1..at + 9)?.try_into().ok()?)),
            0x4 => {
                let (len, start) = self.length(at, low)?;
                Value::Data(self.bytes.get(start..start.checked_add(len)?)?.to_vec())
            }
            0x5 => {
                let (len, start) = self.length(at, low)?;
                let raw = self.bytes.get(start..start.checked_add(len)?)?;
                Value::String(raw.iter().map(|&b| char::from(b)).collect())
            }
            0x6 => {
                let (len, start) = self.length(at, low)?;
                let raw = self.bytes.get(start..start.checked_add(len.checked_mul(2)?)?)?;
                let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
                Value::String(String::from_utf16_lossy(&units))
            }
            0x8 => Value::Uid(be(self.bytes.get(at + 1..at + 2 + usize::from(low))?)?),
            0xA => {
                let (len, start) = self.length(at, low)?;
                let mut items = Vec::with_capacity(len.min(4096));
                for i in 0..len {
                    items.push(self.object(self.reference(start, i)?, depth + 1)?);
                }
                Value::Array(items)
            }
            0xD => {
                let (len, start) = self.length(at, low)?;
                let values = start.checked_add(len.checked_mul(self.ref_size)?)?;
                let mut entries = Vec::with_capacity(len.min(4096));
                for i in 0..len {
                    let key = match self.object(self.reference(start, i)?, depth + 1)? {
                        Value::String(s) => s,
                        _ => return None,
                    };
                    entries.push((key, self.object(self.reference(values, i)?, depth + 1)?));
                }
                Value::Dict(entries)
            }
            _ => return None,
        })
    }
}

/// A big-endian unsigned integer of up to 8 bytes.
fn be(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    Some(bytes.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b)))
}

/// Writes `value` as a binary property list.
pub fn write(value: &Value) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = Vec::new();
    // first pass: count the objects so references get their width
    let count = count_objects(value);
    let ref_size = width_for(count as u64);
    flatten(value, &mut objects, ref_size);
    let mut out = b"bplist00".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for o in &objects {
        offsets.push(out.len() as u64);
        out.extend_from_slice(o);
    }
    let table = out.len() as u64;
    let offset_size = width_for(table);
    for o in offsets {
        put_be(&mut out, o, offset_size);
    }
    out.extend_from_slice(&[0; 6]);
    out.push(offset_size as u8);
    out.push(ref_size as u8);
    out.extend_from_slice(&(objects.len() as u64).to_be_bytes());
    out.extend_from_slice(&0u64.to_be_bytes());
    out.extend_from_slice(&table.to_be_bytes());
    out
}

fn count_objects(v: &Value) -> usize {
    match v {
        Value::Array(items) => 1 + items.iter().map(count_objects).sum::<usize>(),
        Value::Dict(entries) => 1 + entries.iter().map(|(_, v)| 1 + count_objects(v)).sum::<usize>(),
        _ => 1,
    }
}

fn width_for(max: u64) -> usize {
    if max < 1 << 8 {
        1
    } else if max < 1 << 16 {
        2
    } else if max < 1 << 32 {
        4
    } else {
        8
    }
}

fn put_be(out: &mut Vec<u8>, v: u64, size: usize) {
    out.extend_from_slice(v.to_be_bytes().get(8 - size..).unwrap_or(&[]));
}

/// Adds `v` (and what it holds) to `objects`; returns its index. Objects are written in the order
/// they are first reached, a container before its contents.
fn flatten(v: &Value, objects: &mut Vec<Vec<u8>>, ref_size: usize) -> u64 {
    let index = objects.len();
    objects.push(Vec::new());
    let body = match v {
        Value::Null => vec![0x00],
        Value::Bool(b) => vec![if *b { 0x09 } else { 0x08 }],
        Value::Int(i) => {
            let mut o = Vec::new();
            if *i >= 0 && *i < 1 << 8 {
                o.push(0x10);
                o.push(*i as u8);
            } else if *i >= 0 && *i < 1 << 16 {
                o.push(0x11);
                o.extend_from_slice(&(*i as u16).to_be_bytes());
            } else if *i >= 0 && *i < 1 << 32 {
                o.push(0x12);
                o.extend_from_slice(&(*i as u32).to_be_bytes());
            } else {
                o.push(0x13);
                o.extend_from_slice(&i.to_be_bytes());
            }
            o
        }
        Value::Real(r) => {
            let mut o = vec![0x23];
            o.extend_from_slice(&r.to_be_bytes());
            o
        }
        Value::Date(d) => {
            let mut o = vec![0x33];
            o.extend_from_slice(&d.to_be_bytes());
            o
        }
        Value::Data(d) => {
            let mut o = marker(0x4, d.len());
            o.extend_from_slice(d);
            o
        }
        Value::String(s) => {
            if s.is_ascii() {
                let mut o = marker(0x5, s.len());
                o.extend_from_slice(s.as_bytes());
                o
            } else {
                let units: Vec<u16> = s.encode_utf16().collect();
                let mut o = marker(0x6, units.len());
                for u in units {
                    o.extend_from_slice(&u.to_be_bytes());
                }
                o
            }
        }
        Value::Uid(u) => {
            let size = width_for(*u);
            let mut o = vec![0x80 | (size as u8 - 1)];
            put_be(&mut o, *u, size);
            o
        }
        Value::Array(items) => {
            let refs: Vec<u64> = items.iter().map(|i| flatten(i, objects, ref_size)).collect();
            let mut o = marker(0xA, refs.len());
            for r in refs {
                put_be(&mut o, r, ref_size);
            }
            o
        }
        Value::Dict(entries) => {
            let keys: Vec<u64> =
                entries.iter().map(|(k, _)| flatten(&Value::String(k.clone()), objects, ref_size)).collect();
            let values: Vec<u64> = entries.iter().map(|(_, v)| flatten(v, objects, ref_size)).collect();
            let mut o = marker(0xD, keys.len());
            for r in keys.into_iter().chain(values) {
                put_be(&mut o, r, ref_size);
            }
            o
        }
    };
    if let Some(slot) = objects.get_mut(index) {
        *slot = body;
    }
    index as u64
}

/// A marker of `kind` for `len` items, with the long-length form past 14.
fn marker(kind: u8, len: usize) -> Vec<u8> {
    if len < 15 {
        return vec![(kind << 4) | len as u8];
    }
    let size = width_for(len as u64);
    let power = size.trailing_zeros() as u8;
    let mut o = vec![(kind << 4) | 0x0F, 0x10 | power];
    put_be(&mut o, len as u64, size);
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip() {
        let v = Value::Dict(vec![
            ("$version".into(), Value::Int(100_000)),
            ("big".into(), Value::Int(-5_000_000_000)),
            ("flag".into(), Value::Bool(true)),
            ("real".into(), Value::Real(132.5)),
            ("data".into(), Value::Data((0..40).collect())),
            ("text".into(), Value::String("Layer 1".into())),
            ("wide".into(), Value::String("Ébauche ✏".into())),
            ("long".into(), Value::String("x".repeat(300))),
            ("ref".into(), Value::Uid(1)),
            ("list".into(), Value::Array(vec![Value::Uid(2), Value::Null, Value::Date(3.0)])),
        ]);
        let bytes = write(&v);
        assert_eq!(read(&bytes), Some(v));
    }

    #[test]
    fn damaged_lists_are_refused() {
        assert_eq!(read(b"not a plist"), None);
        let bytes = write(&Value::Array(vec![Value::Int(1); 20]));
        for cut in [10, bytes.len() / 2, bytes.len() - 1] {
            assert_eq!(read(bytes.get(..cut).unwrap()), None, "cut at {cut}");
        }
        // a header and a trailer pointing nowhere
        let mut fake = b"bplist00".to_vec();
        fake.extend_from_slice(&[0; 6]);
        fake.extend_from_slice(&[1, 1]);
        fake.extend_from_slice(&5u64.to_be_bytes());
        fake.extend_from_slice(&0u64.to_be_bytes());
        fake.extend_from_slice(&9999u64.to_be_bytes());
        assert_eq!(read(&fake), None);
    }
}
