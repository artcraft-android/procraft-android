//! Frames as an MP4 video through the system's own encoder, as the Android app's VideoWriter does
//! with MediaCodec: H.264 or HEVC, at a frame rate, the bit rate four bits a pixel a second (at
//! least a megabit). On Windows that is Media Foundation; elsewhere there is none yet.

use std::path::Path;

/// `bits_per_pixel` bits a pixel a second, at least a megabit.
#[cfg_attr(not(windows), allow(dead_code))]
fn bit_rate(width: u32, height: u32, bits_per_pixel: u32) -> u32 {
    width.saturating_mul(height).saturating_mul(bits_per_pixel).max(1_000_000)
}

/// The size a video of a `width` × `height` picture is made at: fitted to `max_side`, even, and
/// as large as `fits` (the encoder taking it) allows, a tenth smaller at a time.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn fit(width: u32, height: u32, max_side: u32, mut fits: impl FnMut(u32, u32) -> bool) -> Option<(u32, u32)> {
    let mut scale = (max_side as f32 / width.max(height).max(1) as f32).min(1.0);
    for _ in 0..40 {
        let w = (((width as f32 * scale) as u32) / 2 * 2).max(2);
        let h = (((height as f32 * scale) as u32) / 2 * 2).max(2);
        if fits(w, h) {
            return Some((w, h));
        }
        scale *= 0.9;
    }
    None
}

/// Media Foundation's calls, unsafe as the system's COM interfaces are.
#[cfg(windows)]
#[allow(unsafe_code)]
mod system {
    use std::path::Path;

    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    use windows::core::{GUID, HSTRING};

    /// Media Foundation, started for as long as it is held.
    struct Started;

    impl Started {
        fn new() -> windows::core::Result<Started> {
            // this thread may already be in COM in another way: Media Foundation works either way
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            unsafe { MFStartup((MF_SDK_VERSION << 16) | MF_API_VERSION, MFSTARTUP_FULL)? };
            Ok(Started)
        }
    }

    impl Drop for Started {
        fn drop(&mut self) {
            let _ = unsafe { MFShutdown() };
        }
    }

    fn video_type(subtype: &GUID, width: u32, height: u32, fps: u32) -> windows::core::Result<IMFMediaType> {
        let t = unsafe { MFCreateMediaType()? };
        unsafe {
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            t.SetGUID(&MF_MT_SUBTYPE, subtype)?;
            t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            t.SetUINT64(&MF_MT_FRAME_SIZE, (u64::from(width) << 32) | u64::from(height))?;
            t.SetUINT64(&MF_MT_FRAME_RATE, (u64::from(fps) << 32) | 1)?;
            t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1 << 32) | 1)?;
        }
        Ok(t)
    }

    /// A writer whose encoder takes `width` × `height` frames, the stream it writes, and Media
    /// Foundation kept started while it lives.
    pub struct Writer {
        writer: IMFSinkWriter,
        stream: u32,
        width: u32,
        height: u32,
        frame_time: i64,
        written: i64,
        _started: Started,
    }

    impl Writer {
        pub fn new(
            out: &Path,
            width: u32,
            height: u32,
            fps: u32,
            hevc: bool,
            bits: u32,
        ) -> windows::core::Result<Writer> {
            let started = Started::new()?;
            let mut attributes = None;
            unsafe { MFCreateAttributes(&mut attributes, 1)? };
            if let Some(a) = &attributes {
                unsafe { a.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)? };
            }
            let url = HSTRING::from(out.as_os_str());
            let writer = unsafe { MFCreateSinkWriterFromURL(&url, None, attributes.as_ref())? };
            let subtype = if hevc { MFVideoFormat_HEVC } else { MFVideoFormat_H264 };
            let output = video_type(&subtype, width, height, fps)?;
            unsafe { output.SetUINT32(&MF_MT_AVG_BITRATE, super::bit_rate(width, height, bits))? };
            let stream = unsafe { writer.AddStream(&output)? };
            // the frames as 32-bit BGRX rows, top first; the writer converts them for the encoder
            let input = video_type(&MFVideoFormat_RGB32, width, height, fps)?;
            unsafe {
                input.SetUINT32(&MF_MT_DEFAULT_STRIDE, width * 4)?;
                writer.SetInputMediaType(stream, &input, None)?;
                writer.BeginWriting()?;
            }
            // a frame lasts 1/fps seconds, in 100 ns units
            let frame_time = 10_000_000 / i64::from(fps.max(1));
            Ok(Writer { writer, stream, width, height, frame_time, written: 0, _started: started })
        }

        /// The next frame, `bgrx` rows top first (`width` × `height` × 4 bytes).
        pub fn frame(&mut self, bgrx: &[u8]) -> windows::core::Result<()> {
            let len = self.width * self.height * 4;
            let buffer = unsafe { MFCreateMemoryBuffer(len)? };
            unsafe {
                let mut data = std::ptr::null_mut();
                buffer.Lock(&mut data, None, None)?;
                let n = bgrx.len().min(len as usize);
                std::ptr::copy_nonoverlapping(bgrx.as_ptr(), data, n);
                buffer.Unlock()?;
                buffer.SetCurrentLength(len)?;
                let sample = MFCreateSample()?;
                sample.AddBuffer(&buffer)?;
                sample.SetSampleTime(self.written * self.frame_time)?;
                sample.SetSampleDuration(self.frame_time)?;
                self.writer.WriteSample(self.stream, &sample)?;
            }
            self.written += 1;
            Ok(())
        }

        pub fn finish(self) -> windows::core::Result<()> {
            unsafe { self.writer.Finalize() }
        }
    }

    /// The frames of the video at `path`, each as straight RGBA rows to `each` with when it
    /// shows (seconds); how many.
    pub fn read(path: &Path, each: &mut dyn FnMut(Vec<u8>, u32, u32, f64)) -> windows::core::Result<u32> {
        let _started = Started::new()?;
        let mut attributes = None;
        unsafe { MFCreateAttributes(&mut attributes, 1)? };
        if let Some(a) = &attributes {
            unsafe { a.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)? };
        }
        let url = HSTRING::from(path.as_os_str());
        let reader = unsafe { MFCreateSourceReaderFromURL(&url, attributes.as_ref())? };
        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        // out as 32-bit BGRX, the reader converting
        let wanted = unsafe { MFCreateMediaType()? };
        unsafe {
            wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            reader.SetCurrentMediaType(stream, None, &wanted)?;
        }
        let current = unsafe { reader.GetCurrentMediaType(stream)? };
        let size = unsafe { current.GetUINT64(&MF_MT_FRAME_SIZE)? };
        let (width, height) = ((size >> 32) as u32, (size & 0xFFFF_FFFF) as u32);
        let stride = unsafe { current.GetUINT32(&MF_MT_DEFAULT_STRIDE) }.map_or(width as i32 * 4, |s| s as i32);
        // the picture within the decoded frame (which may be padded): where it is and its size
        let mut area = [0u8; 16];
        let (mut x0, mut y0, mut w, mut h) = (0u32, 0u32, width, height);
        if unsafe { current.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, &mut area, None) }.is_ok() {
            let int = |at: usize| area.get(at..at + 4).and_then(|b| b.try_into().ok()).map_or(0, i32::from_le_bytes);
            let short = |at: usize| area.get(at..at + 2).and_then(|b| b.try_into().ok()).map_or(0, i16::from_le_bytes);
            let (cx, cy) = (int(8), int(12));
            if cx > 0 && cy > 0 {
                (x0, y0) = (short(2).max(0) as u32, short(6).max(0) as u32);
                (w, h) = ((cx as u32).min(width - x0.min(width)), (cy as u32).min(height - y0.min(height)));
            }
        }
        let mut frames = 0;
        loop {
            let (mut flags, mut sample) = (0u32, None);
            unsafe { reader.ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))? };
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
            let Some(sample) = sample else { continue };
            let buffer = unsafe { sample.ConvertToContiguousBuffer()? };
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            unsafe {
                let (mut data, mut len) = (std::ptr::null_mut(), 0u32);
                buffer.Lock(&mut data, None, Some(&mut len))?;
                let bytes = std::slice::from_raw_parts(data, len as usize);
                for y in 0..h {
                    // a negative stride: the rows bottom first
                    let row = if stride < 0 { height - 1 - (y + y0) } else { y + y0 };
                    let start = row as usize * stride.unsigned_abs() as usize + x0 as usize * 4;
                    let (Some(src), Some(dst)) = (
                        bytes.get(start..start + w as usize * 4),
                        rgba.get_mut((y * w * 4) as usize..((y + 1) * w * 4) as usize),
                    ) else {
                        continue;
                    };
                    for (d, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(src.as_chunks::<4>().0) {
                        *d = [s[2], s[1], s[0], 255];
                    }
                }
                buffer.Unlock()?;
            }
            // its time in 100 ns units
            let at = unsafe { sample.GetSampleTime() }.unwrap_or(0);
            each(rgba, w, h, at as f64 / 10_000_000.0);
            frames += 1;
        }
        Ok(frames)
    }
}

/// The frames of the video at `path`, each as straight RGBA rows (`width`, `height`) to `each`
/// with when it shows (seconds); how many there were.
pub fn read_frames(path: &Path, mut each: impl FnMut(Vec<u8>, u32, u32, f64)) -> Result<u32, String> {
    #[cfg(windows)]
    {
        system::read(path, &mut each).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = (path, &mut each);
        Err("videos can't be read on this system yet".into())
    }
}

#[allow(clippy::too_many_arguments)]
/// `count` frames as an MP4 at `out`, H.264 (or HEVC) at `fps`, the picture `width` × `height`
/// fitted to `max_side`; `frame(i, w, h)` gives frame `i` as straight RGBA rows of `w` × `h`.
/// Returns the size it was made at.
pub fn write_mp4(
    out: &Path,
    width: u32,
    height: u32,
    max_side: u32,
    fps: u32,
    hevc: bool,
    bits_per_pixel: u32,
    count: usize,
    frame: impl Fn(usize, u32, u32) -> Option<Vec<u8>>,
) -> Result<(u32, u32), String> {
    #[cfg(windows)]
    {
        let _ = std::fs::remove_file(out);
        // the largest size the encoder takes
        let mut writer = None;
        fit(width, height, max_side, |w, h| match system::Writer::new(out, w, h, fps, hevc, bits_per_pixel) {
            Ok(made) => {
                writer = Some((made, w, h));
                true
            }
            Err(e) => {
                log::info!("the encoder doesn't take {w} x {h}: {e}");
                false
            }
        });
        let Some((mut writer, w, h)) = writer else {
            return Err("no encoder takes this video".into());
        };
        for i in 0..count {
            let rgba = frame(i, w, h).ok_or("a frame couldn't be drawn")?;
            let bgrx: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, _]| [b, g, r, 255]).collect();
            writer.frame(&bgrx).map_err(|e| e.to_string())?;
        }
        writer.finish().map_err(|e| e.to_string())?;
        Ok((w, h))
    }
    #[cfg(not(windows))]
    {
        let _ = (out, width, height, max_side, fps, hevc, bits_per_pixel, count, frame);
        Err("videos can't be made on this system yet".into())
    }
}
