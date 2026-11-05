//! LZO1X, as the older artwork packages keep their tiles (`.chunk`): a stream of literal runs and
//! copies from what was already written, ended by a copy from 16384 back with nothing to copy.
//! Written from the format's published description.

/// Unpacks `data` into `len` bytes; `None` if it is damaged or comes out another size.
pub fn decode(data: &[u8], len: usize) -> Option<Vec<u8>> {
    let mut s = Stream { data, at: 0, out: Vec::with_capacity(len), len };
    // how many literals the last instruction left after itself (4: a run of four or more), which
    // decides what an instruction under 16 means
    let mut state;
    let first = *data.first()?;
    if first > 17 {
        // the stream may start with a run of literals
        s.at = 1;
        let n = usize::from(first - 17);
        s.literals(n)?;
        state = n.min(4);
    } else {
        state = 0;
    }
    loop {
        let t = usize::from(s.byte()?);
        let trailing = if t < 16 {
            match state {
                0 => {
                    // a run of three or more literals more
                    let n = if t == 0 { s.long(15)? } else { t };
                    s.literals(n + 3)?;
                    state = 4;
                    continue;
                }
                1..=3 => {
                    // two bytes from up to 1 KiB back
                    let back = (usize::from(s.byte()?) << 2) + (t >> 2) + 1;
                    s.copy(back, 2)?;
                    t & 3
                }
                _ => {
                    // three bytes from 2 to 3 KiB back
                    let back = (usize::from(s.byte()?) << 2) + (t >> 2) + 2049;
                    s.copy(back, 3)?;
                    t & 3
                }
            }
        } else if t < 32 {
            // from 16 to 48 KiB back; from exactly 16 KiB it is the end
            let n = if t & 7 == 0 { s.long(7)? } else { t & 7 };
            let v = s.le16()?;
            let back = 16384 + ((t & 8) << 11) + (v >> 2);
            if back == 16384 {
                break;
            }
            s.copy(back, n + 2)?;
            v & 3
        } else if t < 64 {
            // up to 16 KiB back
            let n = if t & 31 == 0 { s.long(31)? } else { t & 31 };
            let v = s.le16()?;
            s.copy((v >> 2) + 1, n + 2)?;
            v & 3
        } else {
            // three to eight bytes from up to 2 KiB back
            let n = if t < 128 { 3 + ((t >> 5) & 1) } else { 5 + ((t >> 5) & 3) };
            let back = (usize::from(s.byte()?) << 3) + ((t >> 2) & 7) + 1;
            s.copy(back, n)?;
            t & 3
        };
        s.literals(trailing)?;
        state = trailing;
    }
    (s.out.len() == len).then_some(s.out)
}

struct Stream<'a> {
    data: &'a [u8],
    at: usize,
    out: Vec<u8>,
    /// The most the output may come to.
    len: usize,
}

impl Stream<'_> {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.data.get(self.at)?;
        self.at += 1;
        Some(b)
    }

    fn le16(&mut self) -> Option<usize> {
        Some(usize::from(self.byte()?) | usize::from(self.byte()?) << 8)
    }

    /// A length past `base`: each zero byte adds 255, then the first other byte itself.
    fn long(&mut self, base: usize) -> Option<usize> {
        let mut n = base;
        loop {
            match self.byte()? {
                0 => n = n.checked_add(255)?,
                b => return n.checked_add(usize::from(b)),
            }
        }
    }

    /// `n` bytes as they are.
    fn literals(&mut self, n: usize) -> Option<()> {
        if self.out.len().checked_add(n)? > self.len {
            return None;
        }
        let run = self.data.get(self.at..self.at.checked_add(n)?)?;
        self.out.extend_from_slice(run);
        self.at += n;
        Some(())
    }

    /// `n` bytes from `back` behind the end, one at a time (they may run into what they write).
    fn copy(&mut self, back: usize, n: usize) -> Option<()> {
        if self.out.len().checked_add(n)? > self.len {
            return None;
        }
        let from = self.out.len().checked_sub(back)?;
        for k in from..from + n {
            let b = *self.out.get(k)?;
            self.out.push(b);
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::decode;

    /// Bytes from a fixed generator, as the test streams were made from.
    fn noise(n: usize, seed: u32) -> Vec<u8> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345) & 0x7fff_ffff;
                (x >> 16) as u8
            })
            .collect()
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn a_short_text_with_repeats_unpacks() {
        let text = b"hello hello hello world".repeat(3);
        let packed = hex("0368656c6c6f202a140002776f726c642a4000ac012a58000d656c6c6f2068656c6c6f20776f726c64110000");
        assert_eq!(decode(&packed, text.len()), Some(text.clone()));
        // packed harder: it starts with its literals' count past 17
        let tighter = hex("1768656c6c6f202a140002776f726c64200d5800110000");
        assert_eq!(decode(&tighter, text.len()), Some(text.clone()));
        // the wrong size, a cut stream or a copy from before the start are refused
        assert_eq!(decode(&packed, text.len() + 1), None);
        assert_eq!(decode(&packed[..packed.len() - 3], text.len()), None);
        assert_eq!(decode(&[0x01, 0x61, 0x62, 0x63, 0x64, 0x80, 0xff, 0x11, 0, 0], 12), None);
    }

    #[test]
    fn a_tile_with_long_runs_and_far_copies_unpacks() {
        // 3000 bytes of noise, 20000 zeros, the noise again (from over 16 KiB back), noise
        let mut tile = noise(3000, 7);
        tile.extend(std::iter::repeat_n(0u8, 20_000));
        tile.extend(noise(3000, 7));
        tile.extend(noise(1000, 9));
        let packed = include_bytes!("lzo_test.bin");
        assert_eq!(decode(packed, tile.len()).as_deref(), Some(&tile[..]));
    }
}
