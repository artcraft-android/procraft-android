//! LZ4 in a block framing: blocks of up to 64 KiB, each `bv41`
//! (compressed: raw size, compressed size, LZ4 data that may refer back into earlier blocks) or
//! `bv4-` (stored: raw size, bytes), ended by `bv4$`.

/// Raw bytes per block written.
const BLOCK: usize = 64 * 1024;

/// Decodes a framed stream; `None` if it is damaged or would come out longer than `limit`.
pub fn decode(data: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut at = 0;
    loop {
        match data.get(at..at + 4)? {
            b"bv4$" => return Some(out),
            b"bv41" => {
                let raw = le(data, at + 4)?;
                let comp = le(data, at + 8)?;
                if out.len().checked_add(raw)? > limit {
                    return None;
                }
                let block = data.get(at + 12..at.checked_add(12)?.checked_add(comp)?)?;
                let goal = out.len() + raw;
                block_into(block, &mut out, goal)?;
                at += 12 + comp;
            }
            b"bv4-" => {
                let raw = le(data, at + 4)?;
                if out.len().checked_add(raw)? > limit {
                    return None;
                }
                out.extend_from_slice(data.get(at + 8..at.checked_add(8)?.checked_add(raw)?)?);
                at += 8 + raw;
            }
            _ => return None,
        }
    }
}

fn le(b: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?)).ok()
}

/// Decodes one LZ4 block onto `out` until it holds `goal` bytes; matches may reach back into what
/// `out` held before.
fn block_into(src: &[u8], out: &mut Vec<u8>, goal: usize) -> Option<()> {
    decode_block(src, out, goal)?;
    (out.len() == goal).then_some(())
}

/// One bare LZ4 block (no framing), whatever its length up to `limit`; `None` if it is damaged or
/// longer.
pub fn raw_block(src: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    decode_block(src, &mut out, limit)?;
    Some(out)
}

/// Decodes an LZ4 block onto `out`, to the block's end or until `out` holds `limit` bytes.
fn decode_block(src: &[u8], out: &mut Vec<u8>, limit: usize) -> Option<()> {
    let goal = limit;
    let mut i = 0;
    while i < src.len() {
        let token = *src.get(i)?;
        i += 1;
        let mut literals = usize::from(token >> 4);
        if literals == 15 {
            loop {
                let b = *src.get(i)?;
                i += 1;
                literals += usize::from(b);
                if b != 255 {
                    break;
                }
            }
        }
        out.extend_from_slice(src.get(i..i.checked_add(literals)?)?);
        i += literals;
        if i >= src.len() || out.len() >= goal {
            break;
        }
        let offset = usize::from(u16::from_le_bytes(src.get(i..i + 2)?.try_into().ok()?));
        i += 2;
        let mut length = usize::from(token & 15);
        if length == 15 {
            loop {
                let b = *src.get(i)?;
                i += 1;
                length += usize::from(b);
                if b != 255 {
                    break;
                }
            }
        }
        length += 4;
        let start = out.len().checked_sub(offset)?;
        if offset == 0 || out.len().checked_add(length)? > goal {
            return None;
        }
        // byte by byte: the match may overlap what it writes
        for k in 0..length {
            let b = *out.get(start + k)?;
            out.push(b);
        }
    }
    Some(())
}

/// Encodes `data` in the framing, each block compressed (or stored, when that is smaller).
pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 4 + 16);
    for block in data.chunks(BLOCK) {
        let packed = compress_block(block);
        if packed.len() < block.len() {
            out.extend_from_slice(b"bv41");
            out.extend_from_slice(&(block.len() as u32).to_le_bytes());
            out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            out.extend_from_slice(&packed);
        } else {
            out.extend_from_slice(b"bv4-");
            out.extend_from_slice(&(block.len() as u32).to_le_bytes());
            out.extend_from_slice(block);
        }
    }
    out.extend_from_slice(b"bv4$");
    out
}

/// One LZ4 block, matches found greedily through a hash of 4-byte sequences.
fn compress_block(src: &[u8]) -> Vec<u8> {
    const HASH_BITS: u32 = 14;
    // the format's rules: the last 5 bytes are literals, and no match starts in the last 12
    const LAST_LITERALS: usize = 5;
    const MATCH_LIMIT: usize = 12;
    let mut out = Vec::with_capacity(src.len() / 2 + 16);
    let mut table = vec![0usize; 1 << HASH_BITS];
    let read4 = |at: usize| src.get(at..at + 4).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_le_bytes);
    let hash = |v: u32| (v.wrapping_mul(2_654_435_761) >> (32 - HASH_BITS)) as usize;
    let mut anchor = 0;
    let mut i = 0;
    let end = src.len().saturating_sub(MATCH_LIMIT);
    while i < end {
        let Some(v) = read4(i) else { break };
        let h = hash(v);
        let candidate = table.get(h).copied().unwrap_or(0);
        if let Some(slot) = table.get_mut(h) {
            *slot = i;
        }
        if candidate < i && i - candidate <= 0xFFFF && read4(candidate) == Some(v) {
            // extend the match, stopping short of the block's literal tail
            let max = src.len() - LAST_LITERALS;
            let mut len = 4;
            while i + len < max && src.get(candidate + len) == src.get(i + len) {
                len += 1;
            }
            emit(&mut out, src.get(anchor..i).unwrap_or(&[]), Some(((i - candidate) as u16, len)));
            i += len;
            anchor = i;
        } else {
            i += 1;
        }
    }
    emit(&mut out, src.get(anchor..).unwrap_or(&[]), None);
    out
}

/// A sequence: `literals`, then a match (offset, length) if any.
fn emit(out: &mut Vec<u8>, literals: &[u8], m: Option<(u16, usize)>) {
    let lit = literals.len();
    let ml = m.map_or(0, |(_, l)| l - 4);
    let token = ((lit.min(15) as u8) << 4) | ml.min(15) as u8;
    out.push(token);
    if lit >= 15 {
        let mut rest = lit - 15;
        while rest >= 255 {
            out.push(255);
            rest -= 255;
        }
        out.push(rest as u8);
    }
    out.extend_from_slice(literals);
    if let Some((offset, _)) = m {
        out.extend_from_slice(&offset.to_le_bytes());
        if ml >= 15 {
            let mut rest = ml - 15;
            while rest >= 255 {
                out.push(255);
                rest -= 255;
            }
            out.push(rest as u8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_round_trips() {
        let empty: Vec<u8> = vec![0; 256 * 256 * 4];
        let mut stroke = empty.clone();
        for (i, b) in stroke.iter_mut().enumerate() {
            if (i / 4) % 997 < 30 {
                *b = (i % 251) as u8;
            }
        }
        let noise: Vec<u8> = (0..70_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
        for data in [&empty, &stroke, &noise, &vec![7u8; 3], &Vec::new()] {
            let packed = encode(data);
            assert_eq!(decode(&packed, data.len()).as_deref(), Some(data.as_slice()));
        }
        // an empty tile packs small
        assert!(encode(&empty).len() < 2_000, "{}", encode(&empty).len());
    }

    #[test]
    fn blocks_may_refer_back_into_earlier_ones() {
        // two blocks: 8 literals, then a block that is one match into the first
        let mut s = b"bv41".to_vec();
        s.extend_from_slice(&8u32.to_le_bytes());
        s.extend_from_slice(&9u32.to_le_bytes());
        s.push(0x80);
        s.extend_from_slice(b"ABCDEFGH");
        s.extend_from_slice(b"bv41");
        s.extend_from_slice(&8u32.to_le_bytes());
        s.extend_from_slice(&4u32.to_le_bytes());
        s.push(0x04);
        s.extend_from_slice(&8u16.to_le_bytes());
        s.push(0x00);
        s.extend_from_slice(b"bv4$");
        // the second block's sequence: no literals, a match 8 back, length 8 (4 + 4); then the
        // block ends with an empty literal run
        assert_eq!(decode(&s, 100).as_deref(), Some(&b"ABCDEFGHABCDEFGH"[..]));
    }

    #[test]
    fn damaged_streams_are_refused() {
        assert_eq!(decode(b"nope", 10), None);
        assert_eq!(decode(b"bv41\x10\x00\x00\x00\xff\x00\x00\x00", 100), None);
        let packed = encode(&[1u8; 1000]);
        assert_eq!(decode(&packed, 10), None, "over the limit");
        assert_eq!(decode(packed.get(..packed.len() - 4).unwrap(), 1000), None, "no end");
    }
}
