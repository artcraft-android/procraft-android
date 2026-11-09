//! Frames as an MP4 video through the system's own encoder, as the Android app's VideoWriter does
//! with MediaCodec: H.264 or HEVC, at a frame rate (a fraction of a frame a second too, each
//! frame's time exact), the bit rate four bits a pixel a second (at least a megabit). On Windows that is Media Foundation; on a Mac AVFoundation; elsewhere the
//! system's ffmpeg, the frames piped to it raw (and read back from it the same way).

use std::path::Path;

use procraft_core::FrameRate;

/// What is said where videos are made and read with ffmpeg and it isn't installed.
pub const NO_FFMPEG: &str = "Making and opening videos needs ffmpeg: install it from your system's packages.";

/// `bits_per_pixel` bits a pixel a second, at least a megabit.
fn bit_rate(width: u32, height: u32, bits_per_pixel: u32) -> u32 {
    width.saturating_mul(height).saturating_mul(bits_per_pixel).max(1_000_000)
}

/// The size a video of a `width` × `height` picture is made at: fitted to `max_side`, even, and
/// as large as `fits` (the encoder taking it) allows, a tenth smaller at a time.
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

    use super::FrameRate;

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

    fn video_type(subtype: &GUID, width: u32, height: u32, rate: FrameRate) -> windows::core::Result<IMFMediaType> {
        let t = unsafe { MFCreateMediaType()? };
        unsafe {
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            t.SetGUID(&MF_MT_SUBTYPE, subtype)?;
            t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            t.SetUINT64(&MF_MT_FRAME_SIZE, (u64::from(width) << 32) | u64::from(height))?;
            t.SetUINT64(&MF_MT_FRAME_RATE, (u64::from(rate.frames.max(1)) << 32) | u64::from(rate.seconds.max(1)))?;
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
        rate: FrameRate,
        written: u64,
        _started: Started,
    }

    impl Writer {
        pub fn new(
            out: &Path,
            width: u32,
            height: u32,
            rate: FrameRate,
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
            let output = video_type(&subtype, width, height, rate)?;
            unsafe { output.SetUINT32(&MF_MT_AVG_BITRATE, super::bit_rate(width, height, bits))? };
            let stream = unsafe { writer.AddStream(&output)? };
            // the frames as 32-bit BGRX rows, top first; the writer converts them for the encoder
            let input = video_type(&MFVideoFormat_RGB32, width, height, rate)?;
            unsafe {
                input.SetUINT32(&MF_MT_DEFAULT_STRIDE, width * 4)?;
                writer.SetInputMediaType(stream, &input, None)?;
                writer.BeginWriting()?;
            }
            Ok(Writer { writer, stream, width, height, rate, written: 0, _started: started })
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
                // when it starts and how long it lasts, in 100 ns units
                let at = |i: u64| i64::try_from(self.rate.time_of(i, 10_000_000)).unwrap_or(i64::MAX);
                sample.SetSampleTime(at(self.written))?;
                sample.SetSampleDuration(at(self.written + 1) - at(self.written))?;
                self.writer.WriteSample(self.stream, &sample)?;
            }
            self.written += 1;
            Ok(())
        }

        pub fn finish(self) -> windows::core::Result<()> {
            unsafe { self.writer.Finalize() }
        }
    }

    /// How a decoded frame's pixels lie: its height and stride (bytes; negative, the rows bottom
    /// first), and where the picture is within it and its size (a frame may be padded).
    #[derive(Clone, Copy)]
    struct Layout {
        height: u32,
        stride: i32,
        x0: u32,
        y0: u32,
        w: u32,
        h: u32,
    }

    impl Layout {
        /// The frames of type `t`; a picture size it doesn't say is `before`'s (fitted), else the
        /// whole frame.
        fn of(t: &IMFMediaType, before: Option<&Layout>) -> windows::core::Result<Layout> {
            let size = unsafe { t.GetUINT64(&MF_MT_FRAME_SIZE)? };
            let (width, height) = ((size >> 32) as u32, (size & 0xFFFF_FFFF) as u32);
            let stride = unsafe { t.GetUINT32(&MF_MT_DEFAULT_STRIDE) }.map_or(width as i32 * 4, |s| s as i32);
            let (mut x0, mut y0, mut w, mut h) = (0, 0, width, height);
            if let Some(b) = before {
                (x0, y0) = (b.x0.min(width), b.y0.min(height));
                (w, h) = (b.w.min(width - x0), b.h.min(height - y0));
            }
            let mut area = [0u8; 16];
            if unsafe { t.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, &mut area, None) }.is_ok() {
                let int =
                    |at: usize| area.get(at..at + 4).and_then(|b| b.try_into().ok()).map_or(0, i32::from_le_bytes);
                let short =
                    |at: usize| area.get(at..at + 2).and_then(|b| b.try_into().ok()).map_or(0, i16::from_le_bytes);
                let (cx, cy) = (int(8), int(12));
                if cx > 0 && cy > 0 {
                    (x0, y0) = (short(2).max(0) as u32, short(6).max(0) as u32);
                    (w, h) = ((cx as u32).min(width - x0.min(width)), (cy as u32).min(height - y0.min(height)));
                }
            }
            Ok(Layout { height, stride, x0, y0, w, h })
        }
    }

    /// The frames of the video at `path`, each as straight RGBA rows to `each` with when it
    /// shows (seconds), until it says to stop (false); how many.
    pub fn read(path: &Path, each: &mut dyn FnMut(Vec<u8>, u32, u32, f64) -> bool) -> windows::core::Result<u32> {
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
        let mut layout = Layout::of(&unsafe { reader.GetCurrentMediaType(stream)? }, None)?;
        let mut frames = 0;
        loop {
            let (mut flags, mut sample) = (0u32, None);
            unsafe { reader.ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))? };
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
            // the decoder settled on a frame of its own, padded perhaps (a width not a multiple of
            // 16 comes out wider, with a longer stride): read from here on as it says
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                layout = Layout::of(&unsafe { reader.GetCurrentMediaType(stream)? }, Some(&layout))?;
            }
            let Layout { height, stride, x0, y0, w, h, .. } = layout;
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
            frames += 1;
            if !each(rgba, w, h, at as f64 / 10_000_000.0) {
                break;
            }
        }
        Ok(frames)
    }
}

/// AVFoundation's writer and reader on a Mac (its encoders and decoders VideoToolbox's), in the
/// shape of the Media Foundation module's: unsafe as the system's calls are.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod system {
    use std::path::Path;
    use std::ptr::NonNull;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_av_foundation::{
        AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetTrack, AVAssetWriter, AVAssetWriterInput,
        AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus, AVFileTypeMPEG4, AVMediaTypeVideo, AVURLAsset,
        AVVideoAverageBitRateKey, AVVideoCodecKey, AVVideoCodecTypeH264, AVVideoCodecTypeHEVC,
        AVVideoColorPrimaries_ITU_R_709_2, AVVideoColorPrimariesKey, AVVideoColorPropertiesKey,
        AVVideoCompressionPropertiesKey, AVVideoHeightKey, AVVideoTransferFunction_ITU_R_709_2,
        AVVideoTransferFunctionKey, AVVideoWidthKey, AVVideoYCbCrMatrix_ITU_R_709_2, AVVideoYCbCrMatrixKey,
    };
    use objc2_core_foundation::{CFRetained, CFString};
    use objc2_core_media::CMTime;
    use objc2_core_video::{
        CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
        CVPixelBufferGetDataSize, CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
        CVPixelBufferLockFlags, CVPixelBufferPool, CVPixelBufferUnlockBaseAddress, kCVPixelBufferHeightKey,
        kCVPixelBufferPixelFormatTypeKey, kCVPixelBufferWidthKey, kCVPixelFormatType_32BGRA, kCVReturnSuccess,
    };
    use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString, NSURL};

    /// How long the encoder may take to want the next frame, or to finish the file; and the
    /// video's tracks to be found.
    const WAIT: Duration = Duration::from_secs(30);

    /// What went wrong, as the system says it, else `what`.
    fn said(error: Option<Retained<NSError>>, what: &str) -> String {
        error.map_or_else(|| what.to_owned(), |e| e.localizedDescription().to_string())
    }

    /// One of the system's names (a setting's key, a codec's), which an old system may lack.
    fn name(known: Option<&'static NSString>) -> Result<&'static NSString, String> {
        known.ok_or_else(|| "this system lacks a video setting".to_owned())
    }

    /// A Core Foundation string as the Foundation one it is the same as.
    fn bridged(s: &'static CFString) -> &'static NSString {
        // SAFETY: the two are one type to the system (toll-free bridged)
        unsafe { &*(s as *const CFString).cast::<NSString>() }
    }

    fn file_url(path: &Path) -> Result<Retained<NSURL>, String> {
        NSURL::from_file_path(path).ok_or_else(|| format!("{} isn't a file's path", path.display()))
    }

    /// Pixel buffers of 32-bit BGRA, `width` × `height` if given.
    fn bgra(size: Option<(u32, u32)>) -> Retained<NSDictionary<NSString, AnyObject>> {
        // SAFETY: the system's own constant keys, there for as long as the app runs
        let (format, width, height) =
            unsafe { (kCVPixelBufferPixelFormatTypeKey, kCVPixelBufferWidthKey, kCVPixelBufferHeightKey) };
        let mut keys = vec![bridged(format)];
        let mut values = vec![NSNumber::new_u32(kCVPixelFormatType_32BGRA)];
        if let Some((w, h)) = size {
            keys.extend([bridged(width), bridged(height)]);
            values.extend([NSNumber::new_u32(w), NSNumber::new_u32(h)]);
        }
        let values: Vec<&AnyObject> = values.iter().map(|v| v.as_ref()).collect();
        NSDictionary::from_slices(&keys, &values)
    }

    /// A writer whose encoder takes `width` × `height` frames, its input and the adaptor its
    /// frames go in through.
    pub struct Writer {
        writer: Retained<AVAssetWriter>,
        input: Retained<AVAssetWriterInput>,
        adaptor: Retained<AVAssetWriterInputPixelBufferAdaptor>,
        width: u32,
        height: u32,
        /// The frame rate as a ratio: frame `i` shows at `i * seconds / frames`.
        frames: i32,
        seconds: i64,
        written: i64,
    }

    impl Writer {
        pub fn new(
            out: &Path,
            width: u32,
            height: u32,
            rate: super::FrameRate,
            hevc: bool,
            bits: u32,
        ) -> Result<Writer, String> {
            // the writer won't write over a file
            let _ = std::fs::remove_file(out);
            let url = file_url(out)?;
            let frames = i32::try_from(rate.frames.max(1)).map_err(|e| e.to_string())?;
            let seconds = i64::from(rate.seconds.max(1));
            // SAFETY: the system's constant names; the settings are what a compressed video's are
            // (checked before the input is made, which would throw on ones it can't apply)
            unsafe {
                let (file, video) = (name(AVFileTypeMPEG4)?, name(AVMediaTypeVideo)?);
                let writer =
                    AVAssetWriter::assetWriterWithURL_fileType_error(&url, file).map_err(|e| said(Some(e), ""))?;
                // its index at the front, as players streaming it want
                writer.setShouldOptimizeForNetworkUse(true);
                let rate = NSNumber::new_u32(super::bit_rate(width, height, bits));
                let compression = NSDictionary::<NSString, AnyObject>::from_slices(
                    &[name(AVVideoAverageBitRateKey)?],
                    &[rate.as_ref()],
                );
                // HD video's colours (BT.709), as players take an untagged one
                let colours = NSDictionary::from_slices(
                    &[name(AVVideoColorPrimariesKey)?, name(AVVideoTransferFunctionKey)?, name(AVVideoYCbCrMatrixKey)?],
                    &[
                        name(AVVideoColorPrimaries_ITU_R_709_2)?,
                        name(AVVideoTransferFunction_ITU_R_709_2)?,
                        name(AVVideoYCbCrMatrix_ITU_R_709_2)?,
                    ],
                );
                let codec = name(if hevc { AVVideoCodecTypeHEVC } else { AVVideoCodecTypeH264 })?;
                let (w, h) = (NSNumber::new_u32(width), NSNumber::new_u32(height));
                let settings = NSDictionary::<NSString, AnyObject>::from_slices(
                    &[
                        name(AVVideoCodecKey)?,
                        name(AVVideoWidthKey)?,
                        name(AVVideoHeightKey)?,
                        name(AVVideoCompressionPropertiesKey)?,
                        name(AVVideoColorPropertiesKey)?,
                    ],
                    &[codec.as_ref(), w.as_ref(), h.as_ref(), compression.as_ref(), colours.as_ref()],
                );
                if !writer.canApplyOutputSettings_forMediaType(Some(&settings), video) {
                    return Err("the system's encoder doesn't take this video".into());
                }
                let input = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(video, Some(&settings));
                input.setExpectsMediaDataInRealTime(false);
                let adaptor = AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(
                    &input,
                    Some(&bgra(Some((width, height)))),
                );
                if !writer.canAddInput(&input) {
                    return Err(said(writer.error(), "the writer takes no such video"));
                }
                writer.addInput(&input);
                if !writer.startWriting() {
                    return Err(said(writer.error(), "the writer didn't start"));
                }
                writer.startSessionAtSourceTime(CMTime::new(0, frames));
                Ok(Writer { writer, input, adaptor, width, height, frames, seconds, written: 0 })
            }
        }

        /// A pixel buffer for the next frame: one of the adaptor's, else a new one.
        fn buffer(&self) -> Result<CFRetained<CVPixelBuffer>, String> {
            let mut made: *mut CVPixelBuffer = std::ptr::null_mut();
            // SAFETY: `made` is where the buffer made goes, owned by the caller from then on
            let done = unsafe {
                match self.adaptor.pixelBufferPool() {
                    Some(pool) => CVPixelBufferPool::create_pixel_buffer(None, &pool, NonNull::from(&mut made)),
                    None => CVPixelBufferCreate(
                        None,
                        self.width as usize,
                        self.height as usize,
                        kCVPixelFormatType_32BGRA,
                        None,
                        NonNull::from(&mut made),
                    ),
                }
            };
            let made = NonNull::new(made).filter(|_| done == kCVReturnSuccess).ok_or("no pixel buffer for a frame")?;
            // SAFETY: made for this call, which owns it
            Ok(unsafe { CFRetained::from_raw(made) })
        }

        /// The next frame, `bgrx` rows top first (`width` × `height` × 4 bytes).
        pub fn frame(&mut self, bgrx: &[u8]) -> Result<(), String> {
            let started = Instant::now();
            // SAFETY: the writer and its input are this one's own
            while !unsafe { self.input.isReadyForMoreMediaData() } {
                if unsafe { self.writer.status() } != AVAssetWriterStatus::Writing {
                    return Err(said(unsafe { self.writer.error() }, "the writer stopped"));
                }
                if started.elapsed() > WAIT {
                    return Err("the encoder stopped taking frames".into());
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            let buffer = self.buffer()?;
            // SAFETY: the buffer is locked while its rows are written, each within its row's bytes
            unsafe {
                if CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags::empty()) != kCVReturnSuccess {
                    return Err("a frame's pixel buffer can't be written".into());
                }
                let base = CVPixelBufferGetBaseAddress(&buffer).cast::<u8>();
                let stride = CVPixelBufferGetBytesPerRow(&buffer);
                let row = (self.width as usize * 4).min(CVPixelBufferGetWidth(&buffer) * 4).min(stride);
                let rows = (self.height as usize).min(CVPixelBufferGetHeight(&buffer));
                if !base.is_null() {
                    for (y, src) in bgrx.chunks(self.width as usize * 4).take(rows).enumerate() {
                        std::ptr::copy_nonoverlapping(src.as_ptr(), base.add(y * stride), row.min(src.len()));
                    }
                }
                CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags::empty());
                let at = CMTime::new(self.written * self.seconds, self.frames);
                if !self.adaptor.appendPixelBuffer_withPresentationTime(&buffer, at) {
                    return Err(said(self.writer.error(), "a frame wasn't written"));
                }
            }
            self.written += 1;
            Ok(())
        }

        pub fn finish(self) -> Result<(), String> {
            let (done, finished) = mpsc::channel();
            let handler = RcBlock::new(move || {
                let _ = done.send(());
            });
            // SAFETY: every frame's append has returned; the handler only says it is done
            unsafe {
                self.input.markAsFinished();
                self.writer.finishWritingWithCompletionHandler(&handler);
            }
            finished.recv_timeout(WAIT).map_err(|_| "the video wasn't finished in time")?;
            if unsafe { self.writer.status() } != AVAssetWriterStatus::Completed {
                return Err(said(unsafe { self.writer.error() }, "the video wasn't finished"));
            }
            Ok(())
        }
    }

    /// What a completion hands back from the thread it runs on: the system's tracks and errors
    /// may be read on any.
    struct Handed<T>(T);

    // SAFETY: see above
    unsafe impl<T> Send for Handed<T> {}

    /// The first video track of `asset`, as the system loads its tracks.
    fn video_track(asset: &AVURLAsset) -> Result<Retained<AVAssetTrack>, String> {
        let (sent, got) = mpsc::channel();
        let handler = RcBlock::new(move |tracks: *mut NSArray<AVAssetTrack>, error: *mut NSError| {
            // SAFETY: the system hands the tracks or an error, each null or alive
            let (tracks, error) = unsafe { (Retained::retain(tracks), Retained::retain(error)) };
            let _ = sent.send(Handed(tracks.ok_or_else(|| said(error, "the video's tracks can't be read"))));
        });
        // SAFETY: the system's constant name; the handler only hands on what it is given
        unsafe { asset.loadTracksWithMediaType_completionHandler(name(AVMediaTypeVideo)?, &handler) };
        let Handed(tracks) = got.recv_timeout(WAIT).map_err(|_| "the video's tracks weren't found in time")?;
        tracks?.firstObject().ok_or_else(|| "the file has no video".to_owned())
    }

    /// A decoded frame's pixels as straight RGBA rows, its width and height.
    fn rgba_of(image: &CVPixelBuffer) -> Option<(Vec<u8>, u32, u32)> {
        // SAFETY: the buffer is locked while it is read, within its own bytes
        unsafe {
            if CVPixelBufferLockBaseAddress(image, CVPixelBufferLockFlags::ReadOnly) != kCVReturnSuccess {
                return None;
            }
            let (w, h, stride) =
                (CVPixelBufferGetWidth(image), CVPixelBufferGetHeight(image), CVPixelBufferGetBytesPerRow(image));
            let base = CVPixelBufferGetBaseAddress(image).cast::<u8>().cast_const();
            let rgba = (!base.is_null()).then(|| {
                let bytes = std::slice::from_raw_parts(base, CVPixelBufferGetDataSize(image).min(stride * h));
                let mut rgba = Vec::with_capacity(w * h * 4);
                for row in bytes.chunks(stride.max(1)).take(h) {
                    let pixels = row.get(..w * 4).unwrap_or(row);
                    rgba.extend(pixels.as_chunks::<4>().0.iter().flat_map(|&[b, g, r, _]| [r, g, b, 255]));
                }
                rgba
            });
            CVPixelBufferUnlockBaseAddress(image, CVPixelBufferLockFlags::ReadOnly);
            let rgba = rgba.filter(|p| p.len() == w * h * 4)?;
            Some((rgba, u32::try_from(w).ok()?, u32::try_from(h).ok()?))
        }
    }

    /// The frames of the video at `path`, each as straight RGBA rows to `each` with when it
    /// shows (seconds), until it says to stop (false); how many.
    pub fn read(path: &Path, each: &mut dyn FnMut(Vec<u8>, u32, u32, f64) -> bool) -> Result<u32, String> {
        let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&*file_url(path)?, None) };
        let track = video_track(&asset)?;
        // SAFETY: the asset and its track as the system made them; the output's settings are a
        // pixel buffer's
        unsafe {
            let reader = AVAssetReader::assetReaderWithAsset_error(&asset).map_err(|e| said(Some(e), ""))?;
            let output =
                AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(&track, Some(&bgra(None)));
            output.setAlwaysCopiesSampleData(false);
            if !reader.canAddOutput(&output) {
                return Err("the video's frames can't be read".into());
            }
            reader.addOutput(&output);
            if !reader.startReading() {
                return Err(said(reader.error(), "the video can't be read"));
            }
            let mut frames = 0;
            while let Some(sample) = output.copyNextSampleBuffer() {
                let Some((rgba, w, h)) = sample.image_buffer().and_then(|image| rgba_of(&image)) else { continue };
                frames += 1;
                if !each(rgba, w, h, sample.presentation_time_stamp().seconds()) {
                    reader.cancelReading();
                    return Ok(frames);
                }
            }
            if reader.status() == AVAssetReaderStatus::Failed {
                return Err(said(reader.error(), "the video can't be read"));
            }
            Ok(frames)
        }
    }
}

/// The frames of the video at `path`, each as straight RGBA rows (`width`, `height`) to `each`
/// with when it shows (seconds), until `each` says to stop (false); how many it was given.
pub fn read_frames(path: &Path, mut each: impl FnMut(Vec<u8>, u32, u32, f64) -> bool) -> Result<u32, String> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        system::read(path, &mut each).map_err(|e| e.to_string())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        ffmpeg::read(path, &mut each)
    }
}

#[allow(clippy::too_many_arguments)]
/// `count` frames as an MP4 at `out`, H.264 (or HEVC) at `fps` frames a second (to a thousandth),
/// the picture `width` × `height` fitted to `max_side`; `frame(i, w, h)` gives frame `i` as
/// straight RGBA rows of `w` × `h`. Returns the size it was made at.
pub fn write_mp4(
    out: &Path,
    width: u32,
    height: u32,
    max_side: u32,
    fps: f32,
    hevc: bool,
    bits_per_pixel: u32,
    count: usize,
    frame: impl Fn(usize, u32, u32) -> Option<Vec<u8>>,
) -> Result<(u32, u32), String> {
    let rate = procraft_core::frame_rate(fps);
    #[cfg(any(windows, target_os = "macos"))]
    {
        let _ = std::fs::remove_file(out);
        // the largest size the encoder takes
        let mut writer = None;
        fit(width, height, max_side, |w, h| match system::Writer::new(out, w, h, rate, hevc, bits_per_pixel) {
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
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let (w, h) = fit(width, height, max_side, |_, _| true).ok_or("no size fits")?;
        ffmpeg::write(out, w, h, rate, hevc, bit_rate(w, h, bits_per_pixel), count, frame)?;
        Ok((w, h))
    }
}

/// The system's ffmpeg, where the system has no encoder of its own to call: frames piped to it as
/// raw RGBA, and read back from it the same way.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
mod ffmpeg {
    use std::io::{ErrorKind, Read, Write};
    use std::path::Path;
    use std::process::{Command, Stdio};

    /// ffmpeg (or ffprobe) to run, its output to read; [`super::NO_FFMPEG`] if it isn't there.
    fn run(program: &str) -> Command {
        let mut c = Command::new(program);
        c.arg("-v").arg("error");
        c
    }

    fn missing(e: &std::io::Error) -> String {
        if e.kind() == ErrorKind::NotFound { super::NO_FFMPEG.to_owned() } else { e.to_string() }
    }

    /// An encoder: what goes before the input (a device to open), the encoder, and the filter
    /// taking the frames onto the GPU.
    type Encoder = (&'static [&'static str], &'static str, &'static str);

    /// The encoders tried in turn, the GPU's first (NVIDIA's, then VAAPI's for AMD and Intel, then
    /// Vulkan's), the codec's software one last.
    fn encoders(hevc: bool) -> Vec<Encoder> {
        vec![
            (&[], if hevc { "hevc_nvenc" } else { "h264_nvenc" }, "format=yuv420p"),
            (
                &["-vaapi_device", "/dev/dri/renderD128"],
                if hevc { "hevc_vaapi" } else { "h264_vaapi" },
                "format=nv12,hwupload",
            ),
            (
                &["-init_hw_device", "vulkan=gpu", "-filter_hw_device", "gpu"],
                if hevc { "hevc_vulkan" } else { "h264_vulkan" },
                "format=nv12,hwupload",
            ),
            // the codec's first encoder there is (libx264, libx265 or another)
            (&[], if hevc { "hevc" } else { "h264" }, "format=yuv420p"),
        ]
    }

    /// The first of [`encoders`] that makes a video `width` × `height` here (each tried once on a
    /// moment of test picture, what was found kept for the size).
    fn encoder(hevc: bool, width: u32, height: u32) -> Encoder {
        /// The encoder found for a codec (HEVC or not) and size.
        type Found = ((bool, u32, u32), usize);
        static FOUND: std::sync::Mutex<Vec<Found>> = std::sync::Mutex::new(Vec::new());
        let all = encoders(hevc);
        let software = all.len() - 1;
        let key = (hevc, width, height);
        let known = FOUND.lock().ok().and_then(|f| f.iter().find(|(k, _)| *k == key).map(|(_, i)| *i));
        let i = known.unwrap_or_else(|| {
            let works = |(device, codec, filter): &Encoder| {
                run("ffmpeg")
                    .args(*device)
                    .args(["-f", "lavfi", "-i", &format!("color=size={width}x{height}:rate=30"), "-frames:v", "2"])
                    .args(["-vf", filter, "-c:v", codec, "-f", "null", "-"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success())
            };
            let i = all.iter().position(works).unwrap_or(software);
            if let Ok(mut f) = FOUND.lock() {
                f.push((key, i));
            }
            i
        });
        all.get(i).or(all.last()).copied().unwrap_or((&[], "h264", "format=yuv420p"))
    }

    /// `count` frames of `width` × `height` (even) as an MP4 at `out`: H.264, or HEVC, at `rate` and
    /// `bit_rate` bits a second; `frame(i, w, h)` gives frame `i` as straight RGBA rows.
    #[allow(clippy::too_many_arguments)]
    pub fn write(
        out: &Path,
        width: u32,
        height: u32,
        rate: super::FrameRate,
        hevc: bool,
        bit_rate: u32,
        count: usize,
        frame: impl Fn(usize, u32, u32) -> Option<Vec<u8>>,
    ) -> Result<(), String> {
        let _ = std::fs::remove_file(out);
        // on the GPU where it can be
        let (device, codec, filter) = encoder(hevc, width, height);
        log::info!("making a {width} x {height} video with {codec}");
        let mut child = run("ffmpeg")
            .args(device)
            .args(["-y", "-f", "rawvideo", "-pix_fmt", "rgba"])
            .args(["-s", &format!("{width}x{height}"), "-r", &format!("{}/{}", rate.frames, rate.seconds), "-i", "-"])
            .args(["-vf", filter, "-c:v", codec])
            .args(["-b:v", &bit_rate.to_string(), "-movflags", "+faststart"])
            .args(if hevc { &["-tag:v", "hvc1"][..] } else { &[][..] })
            .arg(out)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| missing(&e))?;
        let fed = (|| {
            let mut input = child.stdin.take().ok_or("ffmpeg took no input")?;
            for i in 0..count {
                let rgba = frame(i, width, height).ok_or("a frame couldn't be drawn")?;
                input.write_all(&rgba).map_err(|e| e.to_string())?;
            }
            Ok::<(), String>(())
        })();
        // its input closed (dropped above), it finishes the file
        let done = child.wait_with_output().map_err(|e| e.to_string())?;
        fed?;
        if !done.status.success() {
            let _ = std::fs::remove_file(out);
            return Err(format!("ffmpeg: {}", String::from_utf8_lossy(&done.stderr).trim()));
        }
        Ok(())
    }

    /// The video at `path` frame by frame: `each(rgba, width, height, seconds)`, straight RGBA,
    /// until it says to stop (false); how many frames it was given.
    pub fn read(path: &Path, each: &mut dyn FnMut(Vec<u8>, u32, u32, f64) -> bool) -> Result<u32, String> {
        let probe = run("ffprobe")
            .args(["-select_streams", "v:0", "-show_entries", "stream=width,height,avg_frame_rate", "-of", "csv=p=0"])
            .arg(path)
            .output()
            .map_err(|e| missing(&e))?;
        let text = String::from_utf8_lossy(&probe.stdout);
        let mut fields = text.trim().split(',');
        let mut number = || fields.next().map(str::trim).unwrap_or("");
        let (width, height): (u32, u32) =
            (number().parse().map_err(|_| "no video size")?, number().parse().map_err(|_| "no video size")?);
        let fps = {
            let rate = number();
            let (n, d) = rate.split_once('/').unwrap_or((rate, "1"));
            let (n, d) = (n.parse::<f64>().unwrap_or(0.0), d.parse::<f64>().unwrap_or(1.0));
            if n > 0.0 && d > 0.0 { n / d } else { 30.0 }
        };
        let mut child = run("ffmpeg")
            .arg("-i")
            .arg(path)
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| missing(&e))?;
        let mut output = child.stdout.take().ok_or("ffmpeg gave no output")?;
        let size = width as usize * height as usize * 4;
        let mut count = 0u32;
        loop {
            let mut rgba = vec![0u8; size];
            match output.read_exact(&mut rgba) {
                Ok(()) => {
                    count += 1;
                    if !each(rgba, width, height, f64::from(count - 1) / fps) {
                        let _ = child.kill();
                        break;
                    }
                }
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => break,
                Err(e) => {
                    let _ = child.kill();
                    return Err(e.to_string());
                }
            }
        }
        let _ = child.wait();
        Ok(count)
    }

    #[cfg(test)]
    mod tests {
        /// The encoder picked for a time-lapse video here (the GPU's where there is one).
        #[test]
        #[ignore = "says which encoder this machine has"]
        fn the_gpus_encoder_is_picked_where_there_is_one() {
            for hevc in [false, true] {
                eprintln!("{}", super::encoder(hevc, 672, 1102).1);
            }
        }

        /// A tenth of a frame a second, as ffmpeg is told it (a ratio): each frame shows ten seconds.
        #[test]
        fn a_slow_video_made_by_ffmpeg_shows_each_frame_ten_seconds() {
            let dir = std::env::temp_dir().join(format!("procraft-ffmpeg-slow-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let out = dir.join("slow.mp4");
            let rate = procraft_core::frame_rate(0.1);
            let made = super::write(&out, 64, 48, rate, false, 1_000_000, 3, |i, w, h| {
                Some([(i * 80) as u8, 90, 200, 255].repeat(w as usize * h as usize))
            });
            match made {
                Err(e) if e == super::super::NO_FFMPEG => return eprintln!("skipped: no ffmpeg"),
                other => other.unwrap(),
            }
            let mut times = Vec::new();
            let count = super::read(&out, &mut |_, _, _, at| {
                times.push(at);
                true
            });
            let _ = std::fs::remove_dir_all(&dir);
            assert_eq!(count.unwrap(), 3);
            assert_eq!(times, [0.0, 10.0, 20.0]);
        }

        #[test]
        fn frames_made_into_a_video_by_ffmpeg_come_back() {
            let dir = std::env::temp_dir().join(format!("procraft-ffmpeg-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let out = dir.join("made.mp4");
            // six frames, each a flat colour
            let colour = |i: usize| [(i * 40) as u8, 200 - (i * 30) as u8, 90, 255];
            let rate = procraft_core::frame_rate(6.0);
            let made = super::write(&out, 64, 48, rate, false, 1_000_000, 6, |i, w, h| {
                Some(colour(i).repeat(w as usize * h as usize))
            });
            match made {
                Err(e) if e == super::super::NO_FFMPEG => return eprintln!("skipped: no ffmpeg"),
                other => other.unwrap(),
            }
            let mut frames = Vec::new();
            let count = super::read(&out, &mut |rgba, w, h, at| {
                frames.push((rgba, w, h, at));
                true
            })
            .unwrap();
            let _ = std::fs::remove_dir_all(&dir);
            assert_eq!(count, 6);
            for (i, (rgba, w, h, at)) in frames.iter().enumerate() {
                assert_eq!((*w, *h), (64, 48));
                assert!((at - i as f64 / 6.0).abs() < 1e-6);
                // the middle pixel near its colour (the video's own rounding and colour subsampling)
                let mid = ((24 * 64 + 32) * 4) as usize;
                let want = colour(i);
                assert!(
                    (0..3).all(|c| (i32::from(rgba[mid + c]) - i32::from(want[c])).abs() <= 6),
                    "{i}: {:?}",
                    &rgba[mid..mid + 4]
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// A video whose width the encoder pads (850: even, not a multiple of 16) comes back as it was
    /// made, each row in its place: an edge down the middle stays straight, not slanted.
    #[test]
    fn a_video_of_a_width_the_encoder_pads_comes_back_unslanted() {
        let dir = std::env::temp_dir().join(format!("procraft-padded-video-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let out = dir.join("padded.mp4");
        let (w, h) = (850u32, 480u32);
        let made = super::write_mp4(&out, w, h, w, 30.0, false, 4, 3, |_, w, h| {
            Some(
                (0..w * h).flat_map(|i| if i % w < w / 2 { [20, 20, 20, 255] } else { [235, 235, 235, 255] }).collect(),
            )
        });
        match made {
            Err(e) if e == super::NO_FFMPEG => return eprintln!("skipped: no ffmpeg"),
            other => assert_eq!(other.unwrap(), (w, h)),
        }
        let mut edges = Vec::new();
        let mut sizes = Vec::new();
        let count = super::read_frames(&out, |rgba, fw, fh, _| {
            sizes.push((fw, fh));
            // each row's edge: its first light pixel
            for row in rgba.chunks(fw as usize * 4).take(fh as usize) {
                edges.push(row.chunks(4).position(|p| p[1] > 128).unwrap_or(usize::MAX));
            }
            true
        });
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(count.unwrap(), 3);
        assert!(sizes.iter().all(|&s| s == (w, h)), "{sizes:?}");
        let (least, most) = (edges.iter().min().copied().unwrap(), edges.iter().max().copied().unwrap());
        assert!(most - least <= 4 && least.abs_diff(w as usize / 2) <= 4, "edges from {least} to {most}");
    }

    /// Frames at a tenth of a frame a second through the system's writer: each shows for ten
    /// seconds, its time exact.
    #[test]
    fn a_slow_video_shows_each_frame_ten_seconds() {
        let dir = std::env::temp_dir().join(format!("procraft-slow-video-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let out = dir.join("slow.mp4");
        let made = super::write_mp4(&out, 64, 48, 64, 0.1, false, 4, 3, |i, w, h| {
            Some([(i * 80) as u8, 90, 200, 255].repeat(w as usize * h as usize))
        });
        match made {
            Err(e) if e == super::NO_FFMPEG => return eprintln!("skipped: no ffmpeg"),
            other => assert_eq!(other.unwrap(), (64, 48)),
        }
        let mut times = Vec::new();
        let count = super::read_frames(&out, |_, _, _, at| {
            times.push(at);
            true
        });
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(count.unwrap(), 3);
        for (i, at) in times.iter().enumerate() {
            assert!((at - 10.0 * i as f64).abs() < 1e-3, "frame {i} at {at}");
        }
    }
}
