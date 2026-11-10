//! Brush sounds (a plugin's): the sound of the stroke being drawn, which the core makes, played as
//! it is made. On Windows it goes through the system's wave output, a few short buffers queued
//! one after another; on Linux it streams into the system's own player (PipeWire's, PulseAudio's
//! or ALSA's), raw; on a Mac it goes through the system's Audio Queue, a few short buffers filled
//! again on the queue's own thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use procraft_core::CanvasSession;

/// The sound's rate, how much each buffer holds (10 ms) and how many are queued (30 ms ahead).
const RATE: u32 = 48_000;
const FRAMES: u32 = 480;
#[cfg(any(windows, target_os = "macos"))]
const QUEUED: usize = 3;

/// The artwork whose strokes are heard, as the playing thread reads it.
type Heard = Arc<Mutex<Option<Arc<CanvasSession>>>>;

/// The thread that plays the strokes' sound while a plugin makes it.
#[derive(Default)]
pub struct StrokeSound {
    heard: Heard,
    running: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl StrokeSound {
    /// Plays `session`'s strokes while `on` (a plugin makes their sound); stops otherwise.
    pub fn follow(&mut self, session: Option<&Arc<CanvasSession>>, on: bool) {
        let wanted = on && session.is_some();
        if let Ok(mut heard) = self.heard.lock() {
            *heard = session.filter(|_| wanted).cloned();
        }
        if wanted && self.thread.is_none() {
            self.running.store(true, Ordering::Relaxed);
            let (heard, running) = (Arc::clone(&self.heard), Arc::clone(&self.running));
            let thread = std::thread::Builder::new().name("procraft-stroke-sound".to_owned());
            self.thread = thread.spawn(move || play(&heard, &running)).ok();
        } else if !wanted && let Some(thread) = self.thread.take() {
            self.running.store(false, Ordering::Relaxed);
            let _ = thread.join();
        }
    }
}

impl Drop for StrokeSound {
    fn drop(&mut self) {
        self.follow(None, false);
    }
}

/// The next buffer's worth of the heard artwork's sound (silence with none), as 16-bit samples.
fn next(heard: &Heard, out: &mut [i16]) {
    let session = heard.lock().ok().and_then(|heard| heard.clone());
    let sound = session.map_or_else(Vec::new, |session| session.stroke_sound(FRAMES, RATE));
    for (i, slot) in out.iter_mut().enumerate() {
        let s = sound.get(i).copied().unwrap_or(0.0);
        *slot = (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
    }
}

/// Windows' wave output: [`QUEUED`] buffers, each filled again as it finishes playing.
#[cfg(windows)]
#[allow(unsafe_code)]
fn play(heard: &Heard, running: &AtomicBool) {
    use windows::Win32::Media::Audio::{
        CALLBACK_NULL, HWAVEOUT, WAVE_FORMAT_PCM, WAVE_MAPPER, WAVEFORMATEX, WAVEHDR, WHDR_DONE, waveOutClose,
        waveOutOpen, waveOutPrepareHeader, waveOutReset, waveOutUnprepareHeader, waveOutWrite,
    };

    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: 1,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * 2,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    let mut out = HWAVEOUT(std::ptr::null_mut());
    // SAFETY: `format` and `out` outlive the call; no callback is asked for
    if unsafe { waveOutOpen(Some(&raw mut out), WAVE_MAPPER, &raw const format, None, None, CALLBACK_NULL) } != 0 {
        log::warn!("brush sounds: no wave output to play them on");
        return;
    }
    let size = std::mem::size_of::<WAVEHDR>() as u32;
    // each buffer and its header stay where they are until the output has let them go
    let mut buffers: Vec<Vec<i16>> = (0..QUEUED).map(|_| vec![0; FRAMES as usize]).collect();
    let headers: Vec<*mut WAVEHDR> = buffers
        .iter_mut()
        .map(|buffer| {
            let header = WAVEHDR {
                lpData: windows::core::PSTR(buffer.as_mut_ptr().cast()),
                dwBufferLength: FRAMES * 2,
                ..WAVEHDR::default()
            };
            Box::into_raw(Box::new(header))
        })
        .collect();
    for (buffer, &header) in buffers.iter_mut().zip(&headers) {
        next(heard, buffer);
        // SAFETY: `header` and the buffer it points into live until the end of this function
        unsafe {
            waveOutPrepareHeader(out, header, size);
            waveOutWrite(out, header, size);
        }
    }
    while running.load(Ordering::Relaxed) {
        for (buffer, &header) in buffers.iter_mut().zip(&headers) {
            // SAFETY: the header is ours; the output only sets its flags, read whole here
            let flags = unsafe { std::ptr::read_unaligned(&raw const (*header).dwFlags) };
            if flags & WHDR_DONE != 0 {
                next(heard, buffer);
                // SAFETY: as above; the buffer was played and is the output's again
                unsafe { waveOutWrite(out, header, size) };
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    // SAFETY: reset hands every buffer back, so each header can be let go and freed
    unsafe {
        waveOutReset(out);
        for &header in &headers {
            waveOutUnprepareHeader(out, header, size);
            drop(Box::from_raw(header));
        }
        waveOutClose(out);
    }
}

/// The players tried in turn, each told to take raw 16-bit mono at [`RATE`] on its input and to
/// keep little of it waiting, so the sound keeps up with the stroke.
#[cfg(all(unix, not(target_os = "macos")))]
const PLAYERS: [(&str, &[&str]); 3] = [
    ("pw-cat", &["--playback", "--format=s16", "--rate=48000", "--channels=1", "--latency=30ms", "-"]),
    ("pacat", &["--playback", "--format=s16le", "--rate=48000", "--channels=1", "--latency-msec=30"]),
    ("aplay", &["-q", "-t", "raw", "-f", "S16_LE", "-r", "48000", "-c", "1", "--buffer-time=30000"]),
];

/// Linux: the first of [`PLAYERS`] there is, fed a buffer at a time; the writes wait while it plays
/// what it holds, which keeps the pace. With none of them, silence.
#[cfg(all(unix, not(target_os = "macos")))]
fn play(heard: &Heard, running: &AtomicBool) {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let started = PLAYERS.iter().find_map(|(player, args)| {
        Command::new(player).args(*args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()
    });
    let Some(mut player) = started else {
        log::warn!("brush sounds: none of pw-cat, pacat or aplay to play them with");
        return;
    };
    let Some(mut input) = player.stdin.take() else { return };
    let mut buffer = vec![0i16; FRAMES as usize];
    while running.load(Ordering::Relaxed) {
        next(heard, &mut buffer);
        let bytes: Vec<u8> = buffer.iter().flat_map(|s| s.to_le_bytes()).collect();
        if input.write_all(&bytes).is_err() {
            log::warn!("brush sounds: the player stopped");
            break;
        }
    }
    drop(input);
    let _ = player.kill();
    let _ = player.wait();
}

/// A Mac's Audio Queue (AudioToolbox, called directly): [`QUEUED`] buffers of 16-bit mono at
/// [`RATE`], each filled again on the queue's own thread as it finishes playing.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod mac {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::{FRAMES, Heard, QUEUED, RATE, next};

    /// What the queue plays (an AudioStreamBasicDescription).
    #[repr(C)]
    struct StreamFormat {
        sample_rate: f64,
        format_id: u32,
        format_flags: u32,
        bytes_per_packet: u32,
        frames_per_packet: u32,
        bytes_per_frame: u32,
        channels_per_frame: u32,
        bits_per_channel: u32,
        reserved: u32,
    }

    /// One of the queue's buffers (an AudioQueueBuffer).
    #[repr(C)]
    struct QueueBuffer {
        capacity: u32,
        data: *mut c_void,
        byte_size: u32,
        user_data: *mut c_void,
        packet_capacity: u32,
        packets: *mut c_void,
        packet_count: u32,
    }

    type Queue = *mut c_void;
    type Refill = extern "C" fn(*mut c_void, Queue, *mut QueueBuffer);

    /// Linear PCM ('lpcm'), its samples signed whole numbers packed together.
    const LINEAR_PCM: u32 = 0x6C70_636D;
    const SIGNED_PACKED: u32 = (1 << 2) | (1 << 3);

    #[link(name = "AudioToolbox", kind = "framework")]
    unsafe extern "C" {
        fn AudioQueueNewOutput(
            format: *const StreamFormat,
            refill: Refill,
            user: *mut c_void,
            run_loop: *const c_void,
            run_loop_mode: *const c_void,
            flags: u32,
            queue: *mut Queue,
        ) -> i32;
        fn AudioQueueAllocateBuffer(queue: Queue, bytes: u32, buffer: *mut *mut QueueBuffer) -> i32;
        fn AudioQueueEnqueueBuffer(
            queue: Queue,
            buffer: *mut QueueBuffer,
            packets: u32,
            descriptions: *const c_void,
        ) -> i32;
        fn AudioQueueStart(queue: Queue, at: *const c_void) -> i32;
        fn AudioQueueDispose(queue: Queue, immediate: u8) -> i32;
    }

    /// The queue has played `buffer` (or it is new): filled from the heard artwork and queued again.
    extern "C" fn refill(user: *mut c_void, queue: Queue, buffer: *mut QueueBuffer) {
        if user.is_null() || buffer.is_null() {
            return;
        }
        // SAFETY: `user` is the `Heard` that `play` keeps until the queue is disposed of; `buffer`
        // is one of the queue's, allocated to hold FRAMES samples of 16 bits
        unsafe {
            let heard = &*user.cast_const().cast::<Heard>();
            let samples = std::slice::from_raw_parts_mut((*buffer).data.cast::<i16>(), FRAMES as usize);
            next(heard, samples);
            (*buffer).byte_size = FRAMES * 2;
            AudioQueueEnqueueBuffer(queue, buffer, 0, std::ptr::null());
        }
    }

    pub(super) fn play(heard: &Heard, running: &AtomicBool) {
        let format = StreamFormat {
            sample_rate: f64::from(RATE),
            format_id: LINEAR_PCM,
            format_flags: SIGNED_PACKED,
            bytes_per_packet: 2,
            frames_per_packet: 1,
            bytes_per_frame: 2,
            channels_per_frame: 1,
            bits_per_channel: 16,
            reserved: 0,
        };
        let user = std::ptr::from_ref(heard).cast_mut().cast::<c_void>();
        let mut queue: Queue = std::ptr::null_mut();
        // SAFETY: `format` and `queue` outlive the call; `heard` outlives the queue (disposed of
        // below, before this returns); the queue calls `refill` on a thread of its own
        let made = unsafe {
            AudioQueueNewOutput(&raw const format, refill, user, std::ptr::null(), std::ptr::null(), 0, &raw mut queue)
        };
        if made != 0 || queue.is_null() {
            log::warn!("brush sounds: no audio queue to play them on ({made})");
            return;
        }
        for _ in 0..QUEUED {
            let mut buffer: *mut QueueBuffer = std::ptr::null_mut();
            // SAFETY: the queue is ours; a buffer it gives is filled and queued by `refill`
            if unsafe { AudioQueueAllocateBuffer(queue, FRAMES * 2, &raw mut buffer) } == 0 {
                refill(user, queue, buffer);
            }
        }
        // SAFETY: as above
        if unsafe { AudioQueueStart(queue, std::ptr::null()) } != 0 {
            log::warn!("brush sounds: the audio queue wouldn't start");
        }
        while running.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // SAFETY: disposed of at once, it stops calling `refill` and frees its buffers before
        // `heard` can go
        unsafe { AudioQueueDispose(queue, 1) };
    }
}

#[cfg(target_os = "macos")]
use mac::play;
