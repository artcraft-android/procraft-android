//! Bytes as base64 text (the standard alphabet, padded), for keeping binary data in text files.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` as base64.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [0, 1, 2].map(|i| chunk.get(i).copied().unwrap_or(0));
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for k in 0..4 {
            let c = if k <= chunk.len() { ALPHABET.get(((n >> (18 - 6 * k)) & 63) as usize).copied() } else { None };
            out.push(char::from(c.unwrap_or(b'=')));
        }
    }
    out
}

/// The bytes in base64 `text` (spaces at either end ignored); none if it isn't base64.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    for quad in text.trim().as_bytes().chunks(4) {
        let mut n = 0u32;
        let mut real = 0;
        for (k, &c) in quad.iter().enumerate() {
            if c == b'=' {
                break;
            }
            n |= (ALPHABET.iter().position(|&x| x == c)? as u32) << (18 - 6 * k);
            real += 1;
        }
        out.extend([(n >> 16) as u8, (n >> 8) as u8, n as u8].into_iter().take(real.max(1) - 1));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_go_both_ways() {
        for bytes in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0, 255, 128, 7]] {
            assert_eq!(decode(&encode(bytes)).unwrap(), bytes);
        }
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert!(decode("no!").is_none());
    }
}
