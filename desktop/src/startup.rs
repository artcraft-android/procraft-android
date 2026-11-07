//! What the app keeps about its own runs: its log, written to a file in the app's folder too (a
//! release build on Windows has no console to show it), a crash's message in it (and said in a
//! message, so the app doesn't just vanish), and which graphics backend to start from.
//!
//! A driver that fails says so, and the next backend is tried at once; one that crashes takes the
//! app down with it. So the app keeps a record of its backend: "<index> trying" while a backend's
//! device, surface and canvases' pipelines are made and until the window's first frame shows,
//! "<index> running" from then until the app ends as it should ("<index> ended"). A start that
//! finds a try or a run that never ended begins from the next backend.

use std::io::Write;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use procraft_gpu::Gpu;

use crate::files;

/// The log, and the one from the run before.
const LOG: &str = "procraft.log";
const PREVIOUS_LOG: &str = "procraft-previous.log";
/// The backend being tried, or the one the last run used and whether that run is going still
/// (or ended without closing).
const GPU_START: &str = "gpu-start.txt";

/// The backend the window's device was made from (`usize::MAX`: none yet).
static BACKEND: AtomicUsize = AtomicUsize::new(usize::MAX);
/// Whether the window's first frame has shown, and the canvases' pipelines are made: the
/// backend's try is over.
static SHOWN: AtomicBool = AtomicBool::new(false);
static PREPARED: AtomicBool = AtomicBool::new(false);
/// The canvases' pipelines failed on the backend: its try isn't over (the next start, or the app
/// started again, takes the next one).
static FAILED: AtomicBool = AtomicBool::new(false);
/// The log's file, let go before the app starts itself again (the new one's log takes its place).
static LOG_FILE: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);
/// When the app was started.
static LAUNCHED: OnceLock<Instant> = OnceLock::new();

/// The log to the terminal and to the app folder's log file, and a crash's message into it.
pub fn init() {
    LAUNCHED.get_or_init(Instant::now);
    let dir = files::app_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::rename(dir.join(LOG), dir.join(PREVIOUS_LOG));
    *LOG_FILE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = std::fs::File::create(dir.join(LOG)).ok();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        // a system font it can't read is left out; one line for each says nothing to act on
        .filter_module("fontdb", log::LevelFilter::Error)
        .target(env_logger::Target::Pipe(Box::new(Both)))
        .init();
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panicked: {info}\n{}", std::backtrace::Backtrace::force_capture());
        before(info);
    }));
    log::info!("ProCraft {} on {} {}", env!("PROCRAFT_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
    #[cfg(windows)]
    native::watch();
}

/// Writes to the terminal and, while it is kept, the log file (each line as it comes, so a crash
/// leaves what led up to it).
struct Both;

impl Write for Both {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if let Some(file) = LOG_FILE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_mut() {
            let _ = file.write_all(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        if let Some(file) = LOG_FILE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_mut() {
            let _ = file.flush();
        }
        Ok(())
    }
}

/// Starts the app again, with the files it was given, as a new process (this one is to end at
/// once): its log let go first, so the new one can keep it as the one before. `tried`: how many
/// graphics backends this start has tried, so the new one tries only those left (see
/// [`gpu_tried`]); none, a new start.
pub fn start_again(tried: Option<usize>) {
    log::info!("starting again");
    LOG_FILE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
    match std::env::current_exe() {
        Ok(me) => {
            let mut again = std::process::Command::new(me);
            again.args(std::env::args_os().skip(1));
            match tried {
                Some(n) => again.env(TRIED, n.to_string()),
                None => again.env_remove(TRIED),
            };
            if let Err(e) = again.spawn() {
                log::error!("starting again: {e}");
            }
        }
        Err(e) => log::error!("starting again: {e}"),
    }
}

/// How many graphics backends were tried before the app started itself again (on a backend's
/// pipelines failing), so a start doesn't go round them for ever.
const TRIED: &str = "PROCRAFT_GPU_TRIED";

/// How many graphics backends this start tried before it started the app again (none for a new
/// start).
pub fn gpu_tried() -> usize {
    std::env::var(TRIED).ok().and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// Whether this run keeps which backend it uses, and says why it stopped: not a test's window,
/// which may be ended any time (and whose message would wait for a click no one makes).
pub fn kept() -> bool {
    std::env::var_os("PROCRAFT_BACKGROUND").is_none()
}

/// The backend to start from, of `count`: the one the last run used, or the next if that run
/// never ended or its try never finished.
pub fn gpu_start(count: usize) -> usize {
    if !kept() {
        return 0;
    }
    let saved = std::fs::read_to_string(files::app_dir().join(GPU_START)).unwrap_or_default();
    let start = next_start(&saved, count, cfg!(windows));
    match read(&saved) {
        Some((index, Stage::Trying)) => log::warn!(
            "graphics backend {index} ({}) never finished its try (the app crashed or its pipelines failed): \
             starting from {start}",
            Gpu::backend_name(index)
        ),
        Some((index, Stage::Running)) => log::warn!(
            "the last run ended without closing (on graphics backend {index}, {}): starting from {start}",
            Gpu::backend_name(index)
        ),
        _ => {}
    }
    start
}

/// Backend `index` is about to be tried for the window: kept as trying until the window's first
/// frame shows and the canvases' pipelines are made.
pub fn gpu_trying(index: usize) {
    log::info!("trying graphics backend {index} ({})", Gpu::backend_name(index));
    note(index, Stage::Trying);
}

/// The window's device is made from backend `index`.
pub fn gpu_made(index: usize) {
    BACKEND.store(index, Ordering::Relaxed);
}

/// The window's first frame has shown (see [`gpu_prepared`]).
pub fn gpu_shown() {
    if SHOWN.swap(true, Ordering::Relaxed) {
        return;
    }
    let since = LAUNCHED.get().map_or(0, |t| t.elapsed().as_millis());
    log::info!("the window's first frame shown, {since} ms after the app started");
    try_over();
}

/// The canvases' pipelines are made, `fine` without a GPU error. Once the window's first frame
/// has shown too, the backend is kept as going until the app ends; one that failed isn't.
pub fn gpu_prepared(fine: bool) {
    if !fine {
        FAILED.store(true, Ordering::Relaxed);
    } else if !PREPARED.swap(true, Ordering::Relaxed) {
        try_over();
    }
}

/// The backend's try is over, if its first frame has shown and its pipelines are made.
fn try_over() {
    let index = BACKEND.load(Ordering::Relaxed);
    if started() && index != usize::MAX {
        note(index, Stage::Running);
    }
}

/// Whether the backend's try is over: the window's first frame shown, the canvases' pipelines
/// made.
fn started() -> bool {
    SHOWN.load(Ordering::Relaxed) && PREPARED.load(Ordering::Relaxed)
}

/// This run is ending as it should: its backend is kept for the next, unless the canvases'
/// pipelines failed on it.
pub fn gpu_ended() {
    let index = BACKEND.load(Ordering::Relaxed);
    if index != usize::MAX && !FAILED.load(Ordering::Relaxed) {
        note(index, Stage::Ended);
    }
}

/// How far a backend got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Being tried: its device, surface and canvases' pipelines made, until the window's first
    /// frame shows.
    Trying,
    /// The app is drawing with it.
    Running,
    /// The app drew with it and ended as it should.
    Ended,
}

impl Stage {
    fn word(self) -> &'static str {
        match self {
            Stage::Trying => "trying",
            Stage::Running => "running",
            Stage::Ended => "ended",
        }
    }
}

/// Records that backend `index` got to `stage` (not for a test's window).
fn note(index: usize, stage: Stage) {
    if kept() {
        let _ = std::fs::write(files::app_dir().join(GPU_START), format!("{index} {}", stage.word()));
    }
}

/// A record's backend and stage; none if it can't be read.
fn read(saved: &str) -> Option<(usize, Stage)> {
    let mut words = saved.split_whitespace();
    let index = words.next()?.parse().ok()?;
    let stage = match words.next()? {
        "trying" => Stage::Trying,
        "running" => Stage::Running,
        "ended" => Stage::Ended,
        _ => return None,
    };
    Some((index, stage))
}

/// The backend to start from, of `count`, after the record `saved`: the next one if its try
/// never finished, else the same one; the first without a record. A run that never ended counts
/// against its backend only where the system says when it is ending (`ends_told`): elsewhere a
/// log-out or a kill looks just the same, and would move a backend that works for good.
fn next_start(saved: &str, count: usize, ends_told: bool) -> usize {
    let count = count.max(1);
    match read(saved) {
        Some((index, Stage::Trying)) => (index % count + 1) % count,
        Some((index, Stage::Running)) if ends_told => (index % count + 1) % count,
        Some((index, Stage::Running | Stage::Ended)) if index < count => index,
        _ => 0,
    }
}

/// Says the app stopped on `what` (a panic's message, a crash) as it goes, where its log is and,
/// while its backend's try wasn't over, that starting it again tries another backend. Only logged
/// for a test's window.
pub fn say_stopped(what: &str) {
    log::error!("stopped: {what}");
    if !kept() {
        return;
    }
    // said on a thread of its own: on the window's thread, a crash in the middle of the window's
    // own messages keeps the message from ever showing (a thread that can't start, the crash
    // holding the system's loader, isn't waited for)
    let text = stopped_text(what);
    let (started, start) = std::sync::mpsc::channel();
    let said = std::thread::Builder::new().name("procraft-stopped".to_owned()).spawn(move || {
        let _ = started.send(());
        files::tell("ProCraft stopped", &text);
    });
    if let Ok(said) = said
        && start.recv_timeout(std::time::Duration::from_secs(3)).is_ok()
    {
        let _ = said.join();
    }
}

/// What [`say_stopped`] says.
fn stopped_text(what: &str) -> String {
    let what = what.trim().trim_end_matches('.');
    let what = if what.is_empty() { "something went wrong" } else { what };
    let again = if !started() && kept() && Gpu::backend_count() > 1 {
        "\n\nStart it again to try another graphics backend."
    } else {
        ""
    };
    format!("ProCraft stopped: {what}.{again}\n\nIts log is {}", files::app_dir().join(LOG).display())
}

/// What the app says when no backend could draw the window, `why` each failed.
pub fn no_gpu_text(why: &str) -> String {
    format!(
        "No graphics backend could draw the window.\n\n{why}\n\nUpdating the graphics driver may help. ProCraft's log is {}",
        files::app_dir().join(LOG).display()
    )
}

/// A crash in the system's or a driver's own code (Windows): logged with the file it happened in,
/// and said, rather than the app vanishing.
#[cfg(windows)]
#[allow(unsafe_code)]
mod native {
    use std::sync::atomic::{AtomicBool, Ordering};

    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::Diagnostics::Debug::{EXCEPTION_POINTERS, SetUnhandledExceptionFilter};
    use windows::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW,
        GetModuleHandleExW,
    };
    use windows::core::PCWSTR;

    /// What a filter returns: the process ends (as it would have), or the crash goes on to the
    /// system.
    const EXCEPTION_EXECUTE_HANDLER: i32 = 1;
    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

    /// A crash is being said (one crashing as it is said goes to the system).
    static CRASHED: AtomicBool = AtomicBool::new(false);

    pub fn watch() {
        // SAFETY: `crashed` is a filter as the system calls one, for the whole process
        unsafe { SetUnhandledExceptionFilter(Some(crashed)) };
    }

    /// A crash no code handled: its exception's code and the file of the code it happened in,
    /// logged and said; then the app ends.
    unsafe extern "system" fn crashed(info: *const EXCEPTION_POINTERS) -> i32 {
        if CRASHED.swap(true, Ordering::SeqCst) {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        // SAFETY: the system gives the crash's pointers, valid while the filter runs
        let record = unsafe { info.as_ref().and_then(|i| i.ExceptionRecord.as_ref()) };
        let (code, address) =
            record.map_or((0, std::ptr::null_mut()), |r| (r.ExceptionCode.0.cast_unsigned(), r.ExceptionAddress));
        let file = module_of(address).unwrap_or_else(|| "the app's code".to_owned());
        log::error!("crashed in {file} (exception {code:#010X} at {address:?})");
        super::say_stopped(&format!("it crashed in {file} (exception {code:#010X})"));
        EXCEPTION_EXECUTE_HANDLER
    }

    /// The name of the file (a DLL, the app) whose code is at `address`.
    fn module_of(address: *mut core::ffi::c_void) -> Option<String> {
        let mut module = HMODULE::default();
        let flags = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
        // SAFETY: with FROM_ADDRESS the name is read as an address in a module, not as text
        unsafe { GetModuleHandleExW(flags, PCWSTR(address.cast()), &mut module) }.ok()?;
        let mut name = [0u16; 512];
        // SAFETY: `name` is a buffer to fill, its length given
        let len = unsafe { GetModuleFileNameW(Some(module), &mut name) } as usize;
        let path = String::from_utf16_lossy(name.get(..len)?);
        path.rsplit(['\\', '/']).next().map(str::to_owned).filter(|n| !n.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::{next_start, read};

    #[test]
    fn a_start_or_run_that_never_ended_starts_the_next_from_the_next_backend() {
        assert_eq!(next_start("", 3, true), 0);
        assert_eq!(next_start("0 ended", 3, true), 0);
        assert_eq!(next_start("2 ended", 3, true), 2);
        // crashed while trying the first, before its first frame: the second; on the last: round
        assert_eq!(next_start("0 trying", 3, true), 1);
        assert_eq!(next_start("2 trying", 3, true), 0);
        // crashed while running on the first: the second
        assert_eq!(next_start("0 running", 3, true), 1);
        assert_eq!(next_start("1 running", 3, true), 2);
        // where a log-out looks like a crash, a run that went on is no mark against its backend;
        // one that never got started still is
        assert_eq!(next_start("0 running", 2, false), 0);
        assert_eq!(next_start("1 running", 2, false), 1);
        assert_eq!(next_start("0 trying", 2, false), 1);
        // fewer backends now (one named by WGPU_BACKEND), or a record it can't read
        assert_eq!(next_start("1 ended", 1, true), 0);
        assert_eq!(next_start("1 trying", 1, true), 0);
        assert_eq!(next_start("garbled", 3, true), 0);
        assert_eq!(next_start("0 sideways", 3, true), 0);
        assert_eq!(next_start(&format!("{} trying", usize::MAX), 3, true), 1);
        assert_eq!(read("1 running"), Some((1, super::Stage::Running)));
    }

    #[test]
    fn a_stop_says_what_happened_and_where_the_log_is() {
        let text = super::stopped_text("wgpu error: Validation Error.\n");
        assert!(text.starts_with("ProCraft stopped: wgpu error: Validation Error."), "{text}");
        assert!(text.contains("procraft.log"), "{text}");
        let text = super::stopped_text("");
        assert!(text.starts_with("ProCraft stopped: something went wrong."), "{text}");
    }
}
