//! The app's GPU device: one for every artwork, as making one compiles the canvases' pipelines (a
//! second and a half on a tablet).
//!
//! And which graphics backend it is made from (Android: Vulkan, then OpenGL ES). A driver that
//! fails says so and the next backend is tried at once, but one that crashes takes the app down
//! with it, leaving nothing to see. So the app keeps a record (in its own folder) of the backend
//! it is trying while that is at risk: while the device and its pipelines are made, and until a
//! canvas first shows on it. A start that finds a try never finished begins from the next
//! backend; one that drew a canvas is kept for good.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use procraft_gpu::Gpu;

static DEVICE: Mutex<Option<Gpu>> = Mutex::new(None);

/// The backend the app's device was made from (`usize::MAX`: none yet).
static BACKEND: AtomicUsize = AtomicUsize::new(usize::MAX);

/// What is left to the system whatever the app asks for, bytes: GPU memory is charged to no
/// process, so the system can't tell who to stop when it runs out, and stops everything else first.
const LEFT_FREE: u64 = 512 << 20;

/// Less than this, bytes, is never what leaves the system short.
const SMALL_ASK: u64 = 32 << 20;

/// Whether `bytes` more of the GPU's memory would leave the system short. Only where the system
/// says what it has free (Android, whose GPU memory is the same memory as everything else's).
pub(crate) fn short_of_memory(bytes: u64) -> bool {
    bytes >= SMALL_ASK && memory_free().is_some_and(|free| bytes.saturating_add(LEFT_FREE) > free)
}

#[cfg(test)]
std::thread_local! {
    /// What this thread's tests have free, in place of the system's.
    pub(crate) static MEMORY_FREE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn memory_free() -> Option<u64> {
    MEMORY_FREE.with(std::cell::Cell::get)
}

#[cfg(all(target_os = "android", not(test)))]
fn memory_free() -> Option<u64> {
    available_in(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(not(any(target_os = "android", test)))]
fn memory_free() -> Option<u64> {
    None
}

/// The memory `/proc/meminfo`'s text says is available, bytes.
#[cfg(any(target_os = "android", test))]
pub(crate) fn available_in(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find_map(|l| l.strip_prefix("MemAvailable:"))?;
    let kb: u64 = line.trim().strip_suffix("kB")?.trim().parse().ok()?;
    Some(kb * 1024)
}

/// Where the record of the backend being tried is kept (the app's own folder); none, none kept.
static RECORD_FOLDER: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The record's file in [`RECORD_FOLDER`]: "<backend> trying|made|drew".
const RECORD: &str = "gpu-start.txt";

/// Whether a canvas has shown in this run (so nothing is at risk any more).
#[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
static SHOWN: AtomicBool = AtomicBool::new(false);

/// The app's device, held while one is made so that two aren't made at once.
pub(crate) fn kept() -> MutexGuard<'static, Option<Gpu>> {
    DEVICE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `gpu`, a device the app made for itself, is the one for drawing with no window as well.
pub fn keep_device(gpu: &Gpu) {
    *kept() = Some(gpu.clone());
}

/// Makes the app's device and its canvases' pipelines ahead of the first artwork, so that it opens
/// at once; nothing if it is made. Slow: call it off the main thread.
#[uniffi::export]
pub fn warm_gpu() {
    start_log();
    let mut kept = kept();
    if kept.is_some() {
        return;
    }
    let from = first_backend();
    let made = std::panic::catch_unwind(|| {
        Gpu::for_canvases_from(from, trying).map(|(gpu, backend)| {
            // the last backend tried is kept whatever its pipelines did: made now if not yet
            gpu.prepare_canvases();
            (gpu, backend)
        })
    });
    match made {
        Ok(Ok((gpu, backend))) => {
            made_on(backend);
            *kept = Some(gpu);
        }
        Ok(Err(e)) => log::error!("making the GPU device: {e}"),
        Err(panic) => log::error!("making the GPU device panicked: {}", procraft_gpu::panic_message(panic.as_ref())),
    }
}

/// Keeps the record of which graphics backend the app is trying in `dir` (the app's own folder),
/// so that a driver crashing the app as its device is made, or before a canvas first shows, sends
/// the next start to the next backend. Call it before [`warm_gpu`].
#[uniffi::export]
pub fn set_gpu_record_folder(dir: String) {
    start_log();
    // the canvases' pipelines kept there between runs as well, where the GPU can
    procraft_gpu::keep_pipelines_in(PathBuf::from(&dir));
    *RECORD_FOLDER.lock().unwrap_or_else(PoisonError::into_inner) = Some(PathBuf::from(dir));
}

/// The log, to Android's (tag "procraft"), with a panic's message and place in it (the platform
/// drops what the standard error says); once.
pub(crate) fn start_log() {
    #[cfg(target_os = "android")]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            android_logger::init_once(
                android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("procraft"),
            );
            let before = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                log::error!("{info}");
                before(info);
            }));
        });
    }
}

/// The backend to start from: the next one after a try that never finished (see the module's
/// note), else the one used last.
pub(crate) fn first_backend() -> usize {
    folder().map_or(0, |dir| start_in(&dir, Gpu::backend_count()))
}

/// Backend `backend` is about to be tried.
pub(crate) fn trying(backend: usize) {
    note(backend, Stage::Trying);
}

/// The app's device is made, from backend `backend`.
pub(crate) fn made_on(backend: usize) {
    BACKEND.store(backend, Ordering::Relaxed);
    note(backend, Stage::Made);
}

/// A canvas is about to show on the app's device: until it has ([`Showing::shown`]), a crash
/// counts against the device's backend. The try ends when this is dropped (unless panicking,
/// which counts against it too).
#[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
pub(crate) struct Showing(Option<usize>);

#[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
impl Showing {
    pub(crate) fn begin() -> Showing {
        let backend = BACKEND.load(Ordering::Relaxed);
        if SHOWN.load(Ordering::Relaxed) || backend == usize::MAX {
            return Showing(None);
        }
        note(backend, Stage::Trying);
        Showing(Some(backend))
    }

    /// The canvas has shown (a frame presented): its backend draws.
    pub(crate) fn shown(&mut self) {
        if let Some(backend) = self.0.take()
            && !SHOWN.swap(true, Ordering::Relaxed)
        {
            note(backend, Stage::Drew);
        }
    }
}

impl Drop for Showing {
    fn drop(&mut self) {
        // stopped before it showed (the app away), or failed with an error: back as it was
        if let Some(backend) = self.0
            && !std::thread::panicking()
        {
            note(backend, Stage::Made);
        }
    }
}

/// How far a backend got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Being tried: a device made from it, its pipelines, or a canvas's first frame.
    Trying,
    /// The app's device is made from it.
    Made,
    /// A canvas has shown on it: kept for good.
    Drew,
}

impl Stage {
    fn word(self) -> &'static str {
        match self {
            Stage::Trying => "trying",
            Stage::Made => "made",
            Stage::Drew => "drew",
        }
    }
}

fn folder() -> Option<PathBuf> {
    RECORD_FOLDER.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn note(backend: usize, stage: Stage) {
    if let Some(dir) = folder() {
        note_in(&dir, backend, stage);
    }
}

/// Records in `dir` that `backend` got to `stage`, unless it has drawn already.
fn note_in(dir: &Path, backend: usize, stage: Stage) {
    let path = dir.join(RECORD);
    let saved = std::fs::read_to_string(&path).unwrap_or_default();
    if read(&saved) == Some((backend, Stage::Drew)) {
        return;
    }
    let text = format!("{backend} {}", stage.word());
    let written = crate::project::write_atomically(&path, |tmp| {
        std::fs::write(tmp, &text).map_err(|e| crate::project::file_error("the GPU record", e))
    });
    if let Err(e) = written {
        log::warn!("{e}");
    }
}

/// The backend to start from, of `count`, by the record in `dir`.
fn start_in(dir: &Path, count: usize) -> usize {
    let saved = std::fs::read_to_string(dir.join(RECORD)).unwrap_or_default();
    let start = next_start(&saved, count);
    if let Some((backend, Stage::Trying)) = read(&saved) {
        log::warn!("graphics backend {backend} never finished its try (the app crashed): starting from {start}");
    }
    start
}

/// A record's backend and stage; none if it can't be read.
fn read(saved: &str) -> Option<(usize, Stage)> {
    let mut words = saved.split_whitespace();
    let backend = words.next()?.parse().ok()?;
    let stage = match words.next()? {
        "trying" => Stage::Trying,
        "made" => Stage::Made,
        "drew" => Stage::Drew,
        _ => return None,
    };
    Some((backend, stage))
}

/// The backend to start from, of `count`, after the record `saved`: the next one if its try
/// never finished, else the same; the first without a record.
fn next_start(saved: &str, count: usize) -> usize {
    let count = count.max(1);
    match read(saved) {
        Some((backend, Stage::Trying)) => (backend + 1) % count,
        Some((backend, _)) if backend < count => backend,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warming_makes_the_apps_device_once() {
        warm_gpu();
        let Some(first) = kept().clone() else { return eprintln!("skipped: no GPU") };
        warm_gpu();
        let again = kept().clone().unwrap();
        assert!(first.device == again.device);
    }

    #[test]
    fn a_backend_whose_try_never_finished_gives_way_to_the_next() {
        assert_eq!(next_start("", 2), 0);
        assert_eq!(next_start("0 made", 2), 0);
        assert_eq!(next_start("1 drew", 2), 1);
        // a crash while trying the first: the second; while trying the last: round to the first
        assert_eq!(next_start("0 trying", 2), 1);
        assert_eq!(next_start("1 trying", 2), 0);
        // fewer backends now (one named by WGPU_BACKEND), or a record it can't read
        assert_eq!(next_start("1 made", 1), 0);
        assert_eq!(next_start("1 trying", 1), 0);
        assert_eq!(next_start("garbled", 2), 0);
        assert_eq!(next_start("0 sideways", 2), 0);
    }

    #[test]
    fn the_record_follows_the_tries_and_keeps_a_backend_that_drew() {
        let dir = std::env::temp_dir().join(format!("procraft-gpu-record-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join(RECORD));
        assert_eq!(start_in(&dir, 2), 0);
        // the first made its device; then crashed before a canvas showed
        note_in(&dir, 0, Stage::Trying);
        note_in(&dir, 0, Stage::Made);
        assert_eq!(start_in(&dir, 2), 0);
        note_in(&dir, 0, Stage::Trying);
        assert_eq!(start_in(&dir, 2), 1);
        // the second draws, and stays so
        note_in(&dir, 1, Stage::Trying);
        note_in(&dir, 1, Stage::Made);
        note_in(&dir, 1, Stage::Drew);
        note_in(&dir, 1, Stage::Trying);
        assert_eq!(std::fs::read_to_string(dir.join(RECORD)).unwrap(), "1 drew");
        assert_eq!(start_in(&dir, 2), 1);
        // a crash on the other, tried since (the one that drew failing with an error): back to it
        note_in(&dir, 0, Stage::Trying);
        assert_eq!(start_in(&dir, 2), 1);
        // nothing left beside it
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).collect();
        assert_eq!(files.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
