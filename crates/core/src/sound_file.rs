//! Sound files: WAV read (whole numbers of 8 to 32 bits, or floats; any number of channels, mixed
//! to one) and written (one channel of 32-bit floats), for the recordings a plugin's sound plays.

/// The most of a recording kept, seconds: more is cut off.
pub const MOST_SECONDS: f32 = 60.0;
/// The least a recording may be, seconds.
pub const LEAST_SECONDS: f32 = 0.05;

/// The little-endian whole number of `N` bytes at `at` in `bytes`.
fn bytes_at<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at.checked_add(N)?)?.try_into().ok()
}

/// `bytes` as a WAV file: its samples, mixed to one channel (-1 to 1) and at most
/// [`MOST_SECONDS`] of them, and how many a second; none if it isn't one this reads.
pub fn read_wav(bytes: &[u8]) -> Option<(Vec<f32>, u32)> {
    if bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let (mut format, mut data) = (None, None);
    let mut at = 12usize;
    while let (Some(id), Some(size)) = (bytes.get(at..at + 4), bytes_at::<4>(bytes, at + 4)) {
        let size = u32::from_le_bytes(size) as usize;
        let start = at + 8;
        // a chunk said to go past the end (a file cut short, or written as it streamed) ends there
        let end = start.saturating_add(size).min(bytes.len());
        let body = bytes.get(start..end)?;
        match id {
            b"fmt " => format = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        if format.is_some() && data.is_some() {
            break;
        }
        at = end.saturating_add(size & 1);
    }
    let (format, data) = (format?, data?);
    let mut tag = u16::from_le_bytes(bytes_at(format, 0)?);
    let channels = usize::from(u16::from_le_bytes(bytes_at(format, 2)?));
    let rate = u32::from_le_bytes(bytes_at(format, 4)?);
    let bits = u16::from_le_bytes(bytes_at(format, 14)?);
    // the extensible kind says its kind in its first two bytes past the extra size
    if tag == 0xFFFE {
        tag = u16::from_le_bytes(bytes_at(format, 24)?);
    }
    let known = matches!((tag, bits), (1, 8 | 16 | 24 | 32) | (3, 32 | 64));
    if !known || channels == 0 || channels > 32 || !(1_000..=384_000).contains(&rate) {
        return None;
    }
    let width = usize::from(bits / 8);
    let sample = |b: &[u8]| -> f32 {
        match (tag, b.len()) {
            (1, 1) => b.first().map_or(0.0, |&v| (f32::from(v) - 128.0) / 128.0),
            (1, 2) => <[u8; 2]>::try_from(b).map_or(0.0, |a| f32::from(i16::from_le_bytes(a)) / 32_768.0),
            (1, 3) => <[u8; 3]>::try_from(b)
                .map_or(0.0, |[x, y, z]| (i32::from_le_bytes([0, x, y, z]) >> 8) as f32 / 8_388_608.0),
            (1, 4) => <[u8; 4]>::try_from(b).map_or(0.0, |a| i32::from_le_bytes(a) as f32 / 2_147_483_648.0),
            (3, 4) => <[u8; 4]>::try_from(b).map_or(0.0, f32::from_le_bytes),
            (3, 8) => <[u8; 8]>::try_from(b).map_or(0.0, |a| f64::from_le_bytes(a) as f32),
            _ => 0.0,
        }
    };
    let most = (MOST_SECONDS * rate as f32) as usize;
    let samples: Vec<f32> = data
        .chunks_exact(width * channels)
        .take(most)
        .map(|frame| frame.chunks_exact(width).map(sample).sum::<f32>() / channels as f32)
        .map(|s| if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 })
        .collect();
    (samples.len() as f32 >= LEAST_SECONDS * rate as f32).then_some((samples, rate))
}

/// `samples` (one channel, -1 to 1) at `rate` a second as a WAV file of 32-bit floats.
pub fn write_wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let data = (samples.len() * 4) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 4);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAV file's head for `channels` of `bits` (`tag` 1 whole numbers, 3 floats) at `rate`.
    fn head(tag: u16, channels: u16, rate: u32, bits: u16, data: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(channels * bits / 8)).to_le_bytes());
        out.extend_from_slice(&(channels * bits / 8).to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        out
    }

    #[test]
    fn a_written_recording_reads_back() {
        let samples: Vec<f32> = (0..4_800).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
        let (read, rate) = read_wav(&write_wav(&samples, 48_000)).unwrap();
        assert_eq!((read, rate), (samples, 48_000));
    }

    #[test]
    fn whole_numbers_and_channels_are_mixed_to_one() {
        // 16-bit stereo: left full, right silent, reads as half
        let frames = 2_205u32;
        let mut file = head(1, 2, 44_100, 16, frames * 4);
        for _ in 0..frames {
            file.extend_from_slice(&16_384i16.to_le_bytes());
            file.extend_from_slice(&0i16.to_le_bytes());
        }
        let (read, rate) = read_wav(&file).unwrap();
        assert_eq!((read.len(), rate), (2_205, 44_100));
        assert!(read.iter().all(|s| (s - 0.25).abs() < 1e-4), "{:?}", read.first());
        // 24-bit mono, a negative half
        let mut file = head(1, 1, 8_000, 24, 3 * 800);
        for _ in 0..800 {
            file.extend_from_slice(&(-4_194_304i32).to_le_bytes()[..3]);
        }
        let (read, _) = read_wav(&file).unwrap();
        assert!(read.iter().all(|s| (s + 0.5).abs() < 1e-4), "{:?}", read.first());
    }

    #[test]
    fn what_is_no_recording_is_refused() {
        assert_eq!(read_wav(b""), None);
        assert_eq!(read_wav(b"RIFF\0\0\0\0WAVE"), None);
        // a kind it doesn't read (A-law), and too short to be heard
        assert_eq!(read_wav(&head(6, 1, 8_000, 8, 0)), None);
        let mut short = head(3, 1, 48_000, 32, 40);
        short.extend_from_slice(&[0; 40]);
        assert_eq!(read_wav(&short), None);
        // cut short: what is there is read
        let mut cut = head(3, 1, 8_000, 32, 4 * 8_000);
        cut.extend(std::iter::repeat_n(0u8, 4 * 1_000));
        assert_eq!(read_wav(&cut).map(|(s, _)| s.len()), Some(1_000));
    }
}
