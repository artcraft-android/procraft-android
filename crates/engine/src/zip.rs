//! Zip archives: reading entries (stored or deflated) and writing deflated ones. No Zip64,
//! encryption or multi-part archives.

/// An archive read into memory: its entries by name.
pub struct ZipReader<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
    /// Where each name's (first) entry is.
    by_name: std::collections::HashMap<String, usize>,
}

#[derive(Clone)]
struct Entry {
    name: String,
    method: u16,
    compressed: usize,
    size: usize,
    crc: u32,
    local: usize,
}

/// The largest entry read: anything claiming more is refused.
const MAX_ENTRY: usize = 1 << 30;

impl<'a> ZipReader<'a> {
    /// The archive in `bytes`, if it is one.
    pub fn new(bytes: &'a [u8]) -> Option<ZipReader<'a>> {
        let from = bytes.len().saturating_sub(TAIL);
        let end = end_record(bytes.get(from..)?)? + from;
        let count = usize::from(u16le(bytes, end + 10)?);
        let at = usize::try_from(u32le(bytes, end + 16)?).ok()?;
        let entries = central_entries(bytes, at, count)?;
        let mut by_name = std::collections::HashMap::with_capacity(entries.len());
        for (i, e) in entries.iter().enumerate() {
            by_name.entry(e.name.clone()).or_insert(i);
        }
        Some(ZipReader { bytes, entries, by_name })
    }

    fn entry(&self, name: &str) -> Option<&Entry> {
        self.by_name.get(name).and_then(|&i| self.entries.get(i))
    }

    /// Every entry's name, folders included.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    /// The size entry `name` says it has once read (`read` gives no more).
    pub fn size(&self, name: &str) -> Option<usize> {
        self.entry(name).map(|e| e.size)
    }

    /// The contents of entry `name`; `None` if it is missing, damaged or of a kind not read.
    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        let e = self.entry(name)?;
        if e.size > MAX_ENTRY || e.compressed > MAX_ENTRY || u32le(self.bytes, e.local)? != 0x0403_4B50 {
            return None;
        }
        let name_len = usize::from(u16le(self.bytes, e.local + 26)?);
        let extra = usize::from(u16le(self.bytes, e.local + 28)?);
        let start = e.local + 30 + name_len + extra;
        let raw = self.bytes.get(start..start.checked_add(e.compressed)?)?;
        let data = match e.method {
            0 => raw.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, e.size).ok()?,
            _ => return None,
        };
        (data.len() == e.size && crc32(&data) == e.crc).then_some(data)
    }
}

/// The end record is in the last 64 KiB of an archive (its comment can be that long).
const TAIL: usize = 22 + 0xFFFF;

/// Where the end record is in `tail`, the archive's last [`TAIL`] bytes (or all of it).
fn end_record(tail: &[u8]) -> Option<usize> {
    (0..tail.len().checked_sub(21)?).rev().find(|&i| tail.get(i..i + 4) == Some(&[0x50, 0x4B, 0x05, 0x06]))
}

/// The `count` entries the central directory at `at` in `bytes` lists.
fn central_entries(bytes: &[u8], mut at: usize, count: usize) -> Option<Vec<Entry>> {
    let mut entries = Vec::with_capacity(count.min(65_536));
    for _ in 0..count {
        if u32le(bytes, at)? != 0x0201_4B50 {
            return None;
        }
        let method = u16le(bytes, at + 10)?;
        let crc = u32le(bytes, at + 16)?;
        let compressed = usize::try_from(u32le(bytes, at + 20)?).ok()?;
        let size = usize::try_from(u32le(bytes, at + 24)?).ok()?;
        let name_len = usize::from(u16le(bytes, at + 28)?);
        let extra = usize::from(u16le(bytes, at + 30)?);
        let comment = usize::from(u16le(bytes, at + 32)?);
        let local = usize::try_from(u32le(bytes, at + 42)?).ok()?;
        let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + name_len)?).into_owned();
        entries.push(Entry { name, method, compressed, size, crc, local });
        at += 46 + name_len + extra + comment;
    }
    Some(entries)
}

/// The names in the archive at `path`, read from its end (the central directory) alone: a big
/// archive is told by what it holds without reading all of it.
pub fn names_in_file(path: &std::path::Path) -> Option<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let tail_len = len.min(TAIL as u64);
    file.seek(SeekFrom::Start(len - tail_len)).ok()?;
    let mut tail = vec![0; usize::try_from(tail_len).ok()?];
    file.read_exact(&mut tail).ok()?;
    let end = end_record(&tail)?;
    let count = usize::from(u16le(&tail, end + 10)?);
    let size = usize::try_from(u32le(&tail, end + 12)?).ok()?;
    let at = u64::from(u32le(&tail, end + 16)?);
    if size > MAX_ENTRY {
        return None;
    }
    let mut directory = vec![0; size];
    file.seek(SeekFrom::Start(at)).ok()?;
    file.read_exact(&mut directory).ok()?;
    Some(central_entries(&directory, 0, count)?.into_iter().map(|e| e.name).collect())
}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// Writes an archive entry by entry.
#[derive(Default)]
pub struct ZipWriter {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

/// An entry made ready to add with [`ZipWriter::add_ready`] (on any thread, for adding in order):
/// its data's checksum and size, and how it is written.
pub struct Ready {
    crc: u32,
    size: usize,
    method: u16,
    body: Vec<u8>,
}

impl Ready {
    /// `data` deflated if that makes it smaller, and `deflate` is asked (it isn't, for data
    /// already compressed that won't shrink); else stored.
    pub fn of(data: &[u8], deflate: bool) -> Ready {
        let (crc, size) = (crc32(data), data.len());
        if deflate {
            let deflated = miniz_oxide::deflate::compress_to_vec(data, 6);
            if deflated.len() < size {
                return Ready { crc, size, method: 8, body: deflated };
            }
        }
        Ready { crc, size, method: 0, body: data.to_vec() }
    }
}

impl ZipWriter {
    pub fn new() -> ZipWriter {
        ZipWriter::default()
    }

    /// Adds `data` as entry `name`, deflated (or stored, if deflating doesn't make it smaller).
    pub fn add(&mut self, name: &str, data: &[u8]) {
        self.add_ready(name, Ready::of(data, true));
    }

    /// Adds an entry made ready (perhaps on another thread) as entry `name`.
    pub fn add_ready(&mut self, name: &str, ready: Ready) {
        self.entry(name, ready.crc, ready.size, ready.method, &ready.body, 0);
    }

    /// Adds `data` as entry `name` stored as it is, starting at a multiple of `align` bytes into
    /// the archive (as USDZ packages ask: the local header padded with an extra field).
    pub fn add_aligned(&mut self, name: &str, data: &[u8], align: usize) {
        let align = align.max(1);
        let at = self.out.len() + 30 + name.len();
        let mut pad = (align - at % align) % align;
        // an extra field needs four bytes for its id and size
        if pad > 0 && pad < 4 {
            pad += align;
        }
        self.entry(name, crc32(data), data.len(), 0, data, pad);
    }

    /// An entry whose data (`size` bytes, checksum `crc`) is written as `body` by `method`.
    fn entry(&mut self, name: &str, crc: u32, size: usize, method: u16, body: &[u8], pad: usize) {
        let local = self.out.len() as u32;
        let name = name.as_bytes();
        let header = |sig: u32, central: bool| {
            let mut h = Vec::with_capacity(46 + name.len());
            h.extend_from_slice(&sig.to_le_bytes());
            if central {
                h.extend_from_slice(&20u16.to_le_bytes()); // made by
            }
            h.extend_from_slice(&20u16.to_le_bytes()); // needed
            h.extend_from_slice(&0x0800u16.to_le_bytes()); // names are UTF-8
            h.extend_from_slice(&method.to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes()); // time
            h.extend_from_slice(&0x0021u16.to_le_bytes()); // date: 1980-01-01
            h.extend_from_slice(&crc.to_le_bytes());
            h.extend_from_slice(&(body.len() as u32).to_le_bytes());
            h.extend_from_slice(&(size as u32).to_le_bytes());
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            let extra = if central { 0 } else { pad as u16 };
            h.extend_from_slice(&extra.to_le_bytes());
            if central {
                h.extend_from_slice(&0u16.to_le_bytes()); // comment
                h.extend_from_slice(&0u16.to_le_bytes()); // disk
                h.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
                h.extend_from_slice(&0u32.to_le_bytes()); // external attributes
                h.extend_from_slice(&local.to_le_bytes());
            }
            h.extend_from_slice(name);
            if !central && pad >= 4 {
                // padding: an extra field of our own id, its size what is left
                h.extend_from_slice(&0x1986u16.to_le_bytes());
                h.extend_from_slice(&((pad - 4) as u16).to_le_bytes());
                h.resize(h.len() + pad - 4, 0);
            }
            h
        };
        self.out.extend_from_slice(&header(0x0403_4B50, false));
        self.out.extend_from_slice(body);
        self.central.extend_from_slice(&header(0x0201_4B50, true));
        self.count = self.count.saturating_add(1);
    }

    /// The finished archive.
    pub fn finish(mut self) -> Vec<u8> {
        let start = self.out.len() as u32;
        let size = self.central.len() as u32;
        self.out.append(&mut self.central);
        self.out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
        self.out.extend_from_slice(&[0; 4]);
        self.out.extend_from_slice(&self.count.to_le_bytes());
        self.out.extend_from_slice(&self.count.to_le_bytes());
        self.out.extend_from_slice(&size.to_le_bytes());
        self.out.extend_from_slice(&start.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out
    }
}

/// The CRC-32 zip uses.
pub fn crc32(data: &[u8]) -> u32 {
    const fn table() -> [u32; 256] {
        let mut t = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                k += 1;
            }
            #[allow(clippy::indexing_slicing)] // const evaluation: i is below 256
            {
                t[i] = c;
            }
            i += 1;
        }
        t
    }
    const TABLE: [u32; 256] = table();
    !data.iter().fold(!0u32, |c, &b| TABLE.get(usize::from((c as u8) ^ b)).copied().unwrap_or(0) ^ (c >> 8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_round_trip() {
        let mut w = ZipWriter::new();
        let big: Vec<u8> = (0..100_000u32).map(|i| (i % 7) as u8).collect();
        w.add("Document.archive", b"bplist00 not really");
        w.add("A1/0~0.lz4", &big);
        w.add("QuickLook/Thumbnail.png", &[]);
        let bytes = w.finish();
        let r = ZipReader::new(&bytes).unwrap();
        assert_eq!(r.names().collect::<Vec<_>>(), ["Document.archive", "A1/0~0.lz4", "QuickLook/Thumbnail.png"]);
        assert_eq!(r.read("A1/0~0.lz4").as_deref(), Some(big.as_slice()));
        assert_eq!(r.read("QuickLook/Thumbnail.png").as_deref(), Some(&[][..]));
        assert_eq!(r.read("missing"), None);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn a_files_names_are_read_from_its_end() {
        let dir = std::env::temp_dir().join(format!("procraft-zip-names-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut w = ZipWriter::new();
        // a big entry the names are read past
        let big: Vec<u8> = (0..300_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
        w.add("Document.archive", b"bplist00");
        w.add("layer/0~0.chunk", &big);
        w.add("QuickLook/Thumbnail.png", &[]);
        let zip = dir.join("a.zip");
        std::fs::write(&zip, w.finish()).unwrap();
        let other = dir.join("b.txt");
        std::fs::write(&other, b"PK but no archive").unwrap();
        let names = names_in_file(&zip);
        let none = (names_in_file(&other), names_in_file(&dir.join("missing")));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(names.unwrap(), ["Document.archive", "layer/0~0.chunk", "QuickLook/Thumbnail.png"]);
        assert_eq!(none, (None, None));
    }

    #[test]
    fn damaged_archives_read_as_nothing() {
        assert!(ZipReader::new(b"PK nothing here").is_none());
        let mut w = ZipWriter::new();
        w.add("a", &[1, 2, 3, 4, 5, 6, 7, 8].repeat(100));
        let mut bytes = w.finish();
        // a flipped byte in the data fails the check
        if let Some(b) = bytes.get_mut(40) {
            *b ^= 0xFF;
        }
        let r = ZipReader::new(&bytes).unwrap();
        assert_eq!(r.read("a"), None);
    }
}
