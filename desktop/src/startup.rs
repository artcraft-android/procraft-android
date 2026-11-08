//! What the app keeps about its own runs: its log, written to a file in the app's folder too (a
//! release build on Windows has no console to show it), a crash's message in it (and said in a
//! message, so the app doesn't just vanish), and which graphics backend to start from.
//!
//! A driver that fails says so, and the next backend is tried at once; one that crashes takes the
//! app down with it. So the app keeps a record of its backend, "<index> <stage>", with "drew"
//! after it once a canvas has drawn on it. A backend is at risk while it is
//! - "trying": its device, surface and canvases' pipelines being made, until the window's first
//!   frame shows (on every start);
//! - "drawing": a canvas opening on it, until the canvas's first frame shows (until a canvas has
//!   drawn on it: from then on the backend is trusted).
//!
//! A start that finds a backend at risk begins from the next. Otherwise the record says "running",
//! or "ended" once the app ends as it should. On Windows, which tells the app when the session ends
//! (a log-out or a shut-down writes "ended" too), a run that never ended counts against its backend
//! as well, however far it got. Elsewhere the next start keeps the backend: an end the app isn't
//! told of (a log-out on Linux, the app killed) looks just like a crash, and would move a backend
//! that works for good.
//!
//! The backends are tried in the order [`backend_order`] gives for this machine; "the next" is the
//! next in it, round to the first. `WGPU_BACKEND` names one by hand: then no record is kept.

use std::io::Write;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

use procraft_gpu::{Gpu, wgpu};

use crate::files;

/// The log, and the one from the run before.
const LOG: &str = "procraft.log";
const PREVIOUS_LOG: &str = "procraft-previous.log";
/// The backend being tried, or the one the last run used, how far it got and whether a canvas
/// has drawn on it.
const GPU_START: &str = "gpu-start.txt";
/// There while Direct3D 12 is being asked which GPUs there are: a driver that crashes as it is
/// asked mustn't do so at every start.
const GPU_ASKING: &str = "gpu-asking.txt";

/// This run's part in the record.
static RUN: Mutex<Run> = Mutex::new(Run::NEW);
/// Whether the system tells the app when the session is ending (Windows): there a run that never
/// ended was a crash, or the app killed.
const ENDS_TOLD: bool = cfg!(windows);
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

/// Whether this run keeps the backend record: not a test's window, nor a run on a backend
/// `WGPU_BACKEND` names (its index is in a list of its own).
fn recorded() -> bool {
    kept() && !Gpu::backends_named()
}

/// Intel's PCI vendor number, as its GPUs give it.
const INTEL: u32 = 0x8086;

/// The order to try the graphics backends in on this machine, as indices of `Gpu::backends`
/// (made once): as they are listed, unless the GPU the app would draw with is an Intel integrated
/// one, on Windows where Vulkan comes first (see [`order_for`]).
pub fn backend_order() -> &'static [usize] {
    static ORDER: OnceLock<Vec<usize>> = OnceLock::new();
    ORDER.get_or_init(|| {
        let backends = Gpu::backends();
        let listed: Vec<usize> = (0..backends.len()).collect();
        // asked only where it changes anything: not with a backend named by hand, and not where
        // Direct3D 12 already comes first (Windows on ARM) or isn't there
        let asked = cfg!(windows)
            && !Gpu::backends_named()
            && backends.first() == Some(&wgpu::Backends::VULKAN)
            && backends.contains(&wgpu::Backends::DX12);
        if !asked {
            return listed;
        }
        // nothing to choose where the record keeps its backend: and asking takes a Direct3D 12
        // device for each GPU, which a machine that has already drawn needn't risk at each start
        let saved = if recorded() {
            std::fs::read_to_string(files::app_dir().join(GPU_START)).unwrap_or_default()
        } else {
            String::new()
        };
        if keeps_backend(&saved, backends.len(), ENDS_TOLD) {
            return listed;
        }
        let asking = files::app_dir().join(GPU_ASKING);
        if recorded() {
            if asking.exists() {
                log::warn!("the last start stopped as Direct3D 12 listed the GPUs: the backends in their order");
                return listed;
            }
            if let Err(e) = files::write_whole(&asking, b"asking") {
                log::warn!("keeping that the GPUs are being listed: {e}");
            }
        }
        // as Direct3D 12 sees them: Vulkan's driver is what may crash, so it isn't asked
        let started = Instant::now();
        let seen = std::panic::catch_unwind(|| Gpu::adapters(wgpu::Backends::DX12)).unwrap_or_else(|panic| {
            log::warn!("the GPUs couldn't be listed: {}", procraft_gpu::panic_message(panic.as_ref()));
            Vec::new()
        });
        if recorded() {
            let _ = std::fs::remove_file(&asking);
        }
        let gpus: Vec<(u32, wgpu::DeviceType)> = seen.iter().map(|a| (a.vendor, a.device_type)).collect();
        let picked = drawn_with(&gpus).and_then(|i| seen.get(i));
        let order = order_for(&backends, picked.map(|a| (a.vendor, a.device_type)));
        let took = started.elapsed().as_millis();
        match picked {
            Some(gpu) if order != listed => log::info!(
                "the GPU to draw with is {} ({:?}, vendor {:#06x}; listed by Direct3D 12 in {took} ms): {} first",
                gpu.name,
                gpu.device_type,
                gpu.vendor,
                Gpu::backend_name(order.first().copied().unwrap_or(0))
            ),
            Some(gpu) => log::info!(
                "the GPU to draw with is {} ({:?}, vendor {:#06x}; listed by Direct3D 12 in {took} ms)",
                gpu.name,
                gpu.device_type,
                gpu.vendor
            ),
            None => log::info!("Direct3D 12 lists no GPU to draw with ({took} ms): the backends in their order"),
        }
        order
    })
}

/// Whether the record `saved` keeps its backend for the next start, of `count`.
fn keeps_backend(saved: &str, count: usize, ends_told: bool) -> bool {
    Record::read(saved).is_some_and(|r| r.backend < count && !r.stage.counts(ends_told))
}

/// Which of `gpus` (vendor, type) the app would draw with: the first discrete one, else the first
/// integrated one, as wgpu picks one for high performance; none if all are software or unknown.
fn drawn_with(gpus: &[(u32, wgpu::DeviceType)]) -> Option<usize> {
    use wgpu::DeviceType::{DiscreteGpu, IntegratedGpu};
    let first = |kind| gpus.iter().position(|&(_, t)| t == kind);
    first(DiscreteGpu).or_else(|| first(IntegratedGpu))
}

/// The order to try `backends` in (their indices) when the app draws with `gpu` (vendor, type): an
/// Intel integrated GPU starts on Direct3D 12 and tries Vulkan last, the others in between as
/// listed. On those GPUs Vulkan is what crashes (6th to 10th generation chips, so far), and before
/// the 6th generation there is no Vulkan driver at all, while Direct3D 12 and OpenGL work; with
/// Vulkan last, the next after a Vulkan that crashed (a record from an earlier version) is
/// Direct3D 12. Any other GPU, Intel's discrete Arc too, keeps the order as listed.
fn order_for(backends: &[wgpu::Backends], gpu: Option<(u32, wgpu::DeviceType)>) -> Vec<usize> {
    let listed: Vec<usize> = (0..backends.len()).collect();
    let at = |kind| backends.iter().position(|b| *b == kind);
    let (Some(dx12), Some((INTEL, wgpu::DeviceType::IntegratedGpu))) = (at(wgpu::Backends::DX12), gpu) else {
        return listed;
    };
    let vulkan = at(wgpu::Backends::VULKAN);
    let between = listed.iter().copied().filter(|&i| i != dx12 && Some(i) != vulkan);
    std::iter::once(dx12).chain(between).chain(vulkan).collect()
}

/// The backend to start from, of those in `order`: the one the last run used, or the next if that
/// one was at risk when the run stopped (see the module's note).
pub fn gpu_start(order: &[usize]) -> usize {
    let saved = if recorded() {
        std::fs::read_to_string(files::app_dir().join(GPU_START)).unwrap_or_default()
    } else {
        String::new()
    };
    let (run, start) = Run::after(&saved, order, ENDS_TOLD);
    *run_now() = run;
    let Some(record) = Record::read(&saved) else { return start };
    let (index, name) = (record.backend, Gpu::backend_name(record.backend));
    match record.stage {
        Stage::Trying => log::warn!(
            "graphics backend {index} ({name}) never finished its try (the app crashed or its pipelines failed): \
             starting from {start}"
        ),
        Stage::Drawing => log::warn!(
            "the app stopped while a canvas first drew on graphics backend {index} ({name}): starting from {start}"
        ),
        Stage::Running if ENDS_TOLD => log::warn!(
            "the last run, on graphics backend {index} ({name}), ended without closing (it crashed or was \
             killed): starting from {start}"
        ),
        Stage::Running => log::info!(
            "the last run, on graphics backend {index} ({name}), ended without closing (a crash after its try, a \
             log-out, the app killed): starting from {start}"
        ),
        Stage::Ended => {}
    }
    if record.drew && start == index {
        log::info!("a canvas has drawn on graphics backend {index} ({name}) before: it is trusted");
    }
    start
}

/// Backend `index` is about to be tried for the window: kept as trying until the window's first
/// frame shows and the canvases' pipelines are made.
pub fn gpu_trying(index: usize) {
    log::info!("trying graphics backend {index} ({})", Gpu::backend_name(index));
    let mut run = run_now();
    run.try_backend(index);
    keep(&mut run);
}

/// The window's device is made from backend `index`.
pub fn gpu_made(index: usize) {
    run_now().made(index);
}

/// The window's first frame has shown (see [`gpu_prepared`]).
pub fn gpu_shown() {
    let mut run = run_now();
    if !run.show() {
        return;
    }
    let since = LAUNCHED.get().map_or(0, |t| t.elapsed().as_millis());
    log::info!("the window's first frame shown, {since} ms after the app started");
    keep(&mut run);
}

/// The canvases' pipelines are made, `fine` without a GPU error. Once the window's first frame
/// has shown too, the backend's try is over; one that failed stays tried, whatever comes after.
pub fn gpu_prepared(fine: bool) {
    let mut run = run_now();
    run.prepare(fine);
    keep(&mut run);
}

/// A canvas is opening in the window: until its first frame shows (see [`canvas_shown`]), a crash
/// counts against the backend, unless a canvas has drawn on it before.
pub fn canvas_opening() {
    let mut run = run_now();
    if run.open_canvas() {
        log::info!("a canvas opens on a graphics backend none has drawn on yet: kept as at risk until it shows");
    }
    keep(&mut run);
}

/// Whether a canvas's first frame is awaited (see [`canvas_opening`]).
pub fn canvas_awaited() -> bool {
    run_now().opening
}

/// A frame has shown, with the canvas opening drawn in it (`Some(true)`), not drawn yet
/// (`Some(false)`), or with no canvas open any more (none: it closed before it showed).
pub fn canvas_shown(drawn: Option<bool>) {
    let mut run = run_now();
    if run.canvas_frame(drawn) {
        log::info!("a canvas has drawn on this graphics backend: it is trusted from now on");
    }
    keep(&mut run);
}

/// This run is ending as it should: its backend is kept for the next, unless the canvases'
/// pipelines failed on it.
pub fn gpu_ended() {
    let mut run = run_now();
    run.end();
    keep(&mut run);
}

/// This run's part in the record, held.
fn run_now() -> std::sync::MutexGuard<'static, Run> {
    RUN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Writes the record as `run` has it now, if that changed (see [`recorded`]): whole or not at
/// all, so a crash as it is written leaves the last.
fn keep(run: &mut Run) {
    let record = run.record();
    if record == run.written || !recorded() {
        return;
    }
    run.written = record;
    if let Some(record) = record
        && let Err(e) = files::write_whole(&files::app_dir().join(GPU_START), record.text().as_bytes())
    {
        log::warn!("keeping the graphics backend: {e}");
    }
}

/// How far the app got with a backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Being tried: its device, surface and canvases' pipelines made, until the window's first
    /// frame shows.
    Trying,
    /// A canvas's first frame being drawn on it, none having drawn on it before.
    Drawing,
    /// The app is drawing with it.
    Running,
    /// The app drew with it and ended as it should.
    Ended,
}

impl Stage {
    fn word(self) -> &'static str {
        match self {
            Stage::Trying => "trying",
            Stage::Drawing => "drawing",
            Stage::Running => "running",
            Stage::Ended => "ended",
        }
    }

    /// Whether a run that stopped at this stage counts against the backend: a crash then is
    /// taken for the driver's. Where the system tells the app of the session's end (`ends_told`),
    /// any run that never ended does.
    fn counts(self, ends_told: bool) -> bool {
        match self {
            Stage::Trying | Stage::Drawing => true,
            Stage::Running => ends_told,
            Stage::Ended => false,
        }
    }
}

/// What the record says: a backend, how far it got, and whether a canvas has drawn on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Record {
    backend: usize,
    stage: Stage,
    drew: bool,
}

impl Record {
    fn text(self) -> String {
        format!("{} {}{}", self.backend, self.stage.word(), if self.drew { " drew" } else { "" })
    }

    /// The record `saved` (with "drew" or not, as earlier versions wrote it); none if it can't be
    /// read.
    fn read(saved: &str) -> Option<Record> {
        let mut words = saved.split_whitespace();
        let backend = words.next()?.parse().ok()?;
        let stage = match words.next()? {
            "trying" => Stage::Trying,
            "drawing" => Stage::Drawing,
            "running" => Stage::Running,
            "ended" => Stage::Ended,
            _ => return None,
        };
        Some(Record { backend, stage, drew: words.next() == Some("drew") })
    }
}

/// The backend to start from, of those in `order`, after the record `saved`: the next in `order`
/// (round to the first) if the run stopped where that counts against it (see [`Stage::counts`]),
/// else the same one; the first in `order` without a record, or for a backend not in it.
fn next_start(saved: &str, order: &[usize], ends_told: bool) -> usize {
    let first = order.first().copied().unwrap_or(0);
    let Some(record) = Record::read(saved) else { return first };
    let Some(at) = order.iter().position(|&b| b == record.backend) else { return first };
    if record.stage.counts(ends_told) { order.get(at + 1).copied().unwrap_or(first) } else { record.backend }
}

/// How far this run has got with its backend, and so what the record is to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Run {
    /// The backend being tried for the window, and the one its device was made from.
    trying: Option<usize>,
    backend: Option<usize>,
    /// The backend a canvas has drawn on: the record's at the start, or this run's.
    drew_on: Option<usize>,
    /// The window's first frame has shown; the canvases' pipelines are made; they failed.
    shown: bool,
    prepared: bool,
    failed: bool,
    /// A canvas is opening on a backend none has drawn on, its first frame not shown yet.
    opening: bool,
    /// The app is ending as it should.
    ended: bool,
    /// What the record was last written to say.
    written: Option<Record>,
}

impl Run {
    const NEW: Run = Run {
        trying: None,
        backend: None,
        drew_on: None,
        shown: false,
        prepared: false,
        failed: false,
        opening: false,
        ended: false,
        written: None,
    };

    /// A run after the record `saved`, and the backend it starts from, of those in `order` (see
    /// [`next_start`]).
    fn after(saved: &str, order: &[usize], ends_told: bool) -> (Run, usize) {
        let drew_on = Record::read(saved).filter(|r| r.drew).map(|r| r.backend);
        (Run { drew_on, ..Run::NEW }, next_start(saved, order, ends_told))
    }

    /// Backend `index` is about to be tried for the window.
    fn try_backend(&mut self, index: usize) {
        self.trying = Some(index);
    }

    /// The window's device is made from backend `index`.
    fn made(&mut self, index: usize) {
        self.backend = Some(index);
    }

    /// The window's first frame has shown; true the first time.
    fn show(&mut self) -> bool {
        !std::mem::replace(&mut self.shown, true)
    }

    /// The canvases' pipelines are made, `fine` without a GPU error (else the backend stays tried).
    fn prepare(&mut self, fine: bool) {
        if fine {
            self.prepared = true;
        } else {
            self.failed = true;
        }
    }

    /// The app is ending as it should.
    fn end(&mut self) {
        self.ended = true;
    }

    /// A canvas opens: at risk if no canvas has drawn on the backend. True if it is now.
    fn open_canvas(&mut self) -> bool {
        let at_risk = self.backend.is_some() && self.drew_on != self.backend && !self.opening;
        self.opening |= at_risk;
        at_risk
    }

    /// A frame shown with the canvas opening drawn (see [`canvas_shown`]): its backend is trusted.
    /// True if it is now.
    fn canvas_frame(&mut self, drawn: Option<bool>) -> bool {
        if !self.opening || drawn == Some(false) {
            return false;
        }
        // drawn, or closed before it showed: no longer at risk
        self.opening = false;
        let drew = drawn == Some(true);
        if drew {
            self.drew_on = self.backend;
        }
        drew
    }

    /// What the record is to say; none before a backend is tried.
    fn record(&self) -> Option<Record> {
        let (backend, stage) = match (self.backend, self.trying) {
            (Some(backend), _) => {
                let stage = if self.ended && !self.failed {
                    Stage::Ended
                } else if !(self.shown && self.prepared) {
                    Stage::Trying
                } else if self.opening {
                    Stage::Drawing
                } else {
                    Stage::Running
                };
                (backend, stage)
            }
            // none of those tried made a device yet
            (None, Some(trying)) => (trying, Stage::Trying),
            (None, None) => return None,
        };
        Some(Record { backend, stage, drew: self.drew_on == Some(backend) })
    }

    /// Whether a crash now would count against the backend (see [`Stage::counts`]).
    fn at_risk(&self, ends_told: bool) -> bool {
        self.record().is_some_and(|r| r.stage.counts(ends_told))
    }
}

/// Says the app stopped on `what` (a panic's message, a crash) as it goes, where its log is and,
/// while its backend was at risk, that starting it again tries another backend. Only logged for a
/// test's window.
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
    // not waited for: a crash may have stopped the thread holding it
    let at_risk = match RUN.try_lock() {
        Ok(run) => run.at_risk(ENDS_TOLD),
        Err(std::sync::TryLockError::Poisoned(run)) => run.into_inner().at_risk(ENDS_TOLD),
        Err(std::sync::TryLockError::WouldBlock) => false,
    };
    let again = if at_risk && kept() && Gpu::backend_count() > 1 {
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
    use super::{Record, Run, Stage, next_start};

    /// The record a run leaves if it stops now with nothing more written (a crash, a log-out, the
    /// app killed): the record is written each time what it says changes.
    fn left(run: &Run) -> String {
        run.record().map(Record::text).unwrap_or_default()
    }

    /// A run after `saved` with two backends, its window's device made from the one it starts
    /// from, its first frame shown and its canvases' pipelines made, on a system that tells of the
    /// session's end (`ends_told`: Windows) or doesn't (Linux, a Mac).
    fn started_on(saved: &str, ends_told: bool) -> Run {
        let (mut run, start) = Run::after(saved, &[0, 1], ends_told);
        run.try_backend(start);
        run.made(start);
        run.show();
        run.prepare(true);
        run
    }

    /// [`started_on`] Linux or a Mac.
    fn started_after(saved: &str) -> Run {
        started_on(saved, false)
    }

    #[test]
    fn a_backend_at_risk_gives_way_to_the_next_and_any_other_is_kept() {
        assert_eq!(next_start("", &[0, 1, 2], false), 0);
        assert_eq!(next_start("0 ended", &[0, 1, 2], false), 0);
        assert_eq!(next_start("2 ended drew", &[0, 1, 2], false), 2);
        // stopped while trying the first (before its first frame) or as a canvas first drew on
        // it: the second; on the last: round to the first
        assert_eq!(next_start("0 trying", &[0, 1, 2], false), 1);
        assert_eq!(next_start("0 trying drew", &[0, 1, 2], false), 1);
        assert_eq!(next_start("1 drawing", &[0, 1, 2], false), 2);
        assert_eq!(next_start("2 trying", &[0, 1, 2], false), 0);
        // stopped while running where the system doesn't say when it ends: a log-out looks just the same
        assert_eq!(next_start("0 running", &[0, 1, 2], false), 0);
        assert_eq!(next_start("1 running drew", &[0, 1, 2], false), 1);
        // fewer backends now, a backend there isn't, or a record it can't read: the first
        assert_eq!(next_start("1 ended", &[0], false), 0);
        assert_eq!(next_start("1 trying", &[0], false), 0);
        assert_eq!(next_start("garbled", &[0, 1, 2], false), 0);
        assert_eq!(next_start("0 sideways", &[0, 1, 2], false), 0);
        assert_eq!(next_start(&format!("{} trying", usize::MAX), &[0, 1, 2], false), 0);
        assert_eq!(next_start("7 ended drew", &[1, 2, 0], false), 1);
    }

    use super::{INTEL, drawn_with, keeps_backend, order_for};
    use procraft_gpu::wgpu::Backends;
    use procraft_gpu::wgpu::DeviceType::{Cpu, DiscreteGpu, IntegratedGpu, Other};

    /// Windows on x64's backends, as `Gpu::backends` lists them.
    const WINDOWS: [Backends; 3] = [Backends::VULKAN, Backends::DX12, Backends::GL];
    const NVIDIA: u32 = 0x10DE;
    const AMD: u32 = 0x1002;
    const MICROSOFT: u32 = 0x1414;

    /// The order for a machine whose GPUs (vendor, type) are `gpus`, as Direct3D 12 lists them.
    fn order_with(backends: &[Backends], gpus: &[(u32, procraft_gpu::wgpu::DeviceType)]) -> Vec<usize> {
        order_for(backends, drawn_with(gpus).and_then(|i| gpus.get(i).copied()))
    }

    #[test]
    fn an_intel_integrated_gpu_starts_on_direct3d_12_and_tries_vulkan_last() {
        // an HD 530 beside Windows's own software renderer: Direct3D 12, OpenGL, Vulkan
        assert_eq!(order_with(&WINDOWS, &[(INTEL, IntegratedGpu), (MICROSOFT, Cpu)]), [1, 2, 0]);
        assert_eq!(order_with(&WINDOWS, &[(MICROSOFT, Cpu), (INTEL, IntegratedGpu)]), [1, 2, 0]);
        // a list with no Vulkan, or Direct3D 12 first already: Direct3D 12 first, the rest as listed
        assert_eq!(order_with(&[Backends::GL, Backends::DX12], &[(INTEL, IntegratedGpu)]), [1, 0]);
        assert_eq!(order_with(&[Backends::DX12, Backends::VULKAN, Backends::GL], &[(INTEL, IntegratedGpu)]), [0, 2, 1]);
        // no Direct3D 12 to start on: as listed
        assert_eq!(order_with(&[Backends::VULKAN, Backends::GL], &[(INTEL, IntegratedGpu)]), [0, 1]);
    }

    #[test]
    fn a_discrete_gpu_or_another_maker_s_keeps_the_backends_as_listed() {
        // NVIDIA or AMD, on their own or beside an Intel integrated GPU (a laptop draws with the
        // discrete one)
        assert_eq!(order_with(&WINDOWS, &[(NVIDIA, DiscreteGpu), (MICROSOFT, Cpu)]), [0, 1, 2]);
        assert_eq!(order_with(&WINDOWS, &[(INTEL, IntegratedGpu), (NVIDIA, DiscreteGpu)]), [0, 1, 2]);
        assert_eq!(order_with(&WINDOWS, &[(AMD, DiscreteGpu), (INTEL, IntegratedGpu)]), [0, 1, 2]);
        // AMD's integrated GPUs
        assert_eq!(order_with(&WINDOWS, &[(AMD, IntegratedGpu)]), [0, 1, 2]);
        // Intel's discrete Arc, on its own or beside an Intel integrated GPU
        assert_eq!(order_with(&WINDOWS, &[(INTEL, DiscreteGpu)]), [0, 1, 2]);
        assert_eq!(order_with(&WINDOWS, &[(INTEL, IntegratedGpu), (INTEL, DiscreteGpu)]), [0, 1, 2]);
        // nothing to draw with but software or an unknown GPU, or nothing listed
        assert_eq!(order_with(&WINDOWS, &[(MICROSOFT, Cpu)]), [0, 1, 2]);
        assert_eq!(order_with(&WINDOWS, &[(INTEL, Other)]), [0, 1, 2]);
        assert_eq!(order_with(&WINDOWS, &[]), [0, 1, 2]);
    }

    #[test]
    fn the_gpus_are_asked_about_only_when_the_record_keeps_no_backend() {
        // a backend that ended well is kept, and nothing needs choosing
        assert!(keeps_backend("0 ended", 3, true));
        assert!(keeps_backend("1 ended drew", 3, true));
        // a first start, one at risk, a backend there isn't: chosen
        assert!(!keeps_backend("", 3, true));
        assert!(!keeps_backend("0 trying", 3, true));
        assert!(!keeps_backend("1 drawing drew", 3, false));
        assert!(!keeps_backend("7 ended", 3, true));
        // a run that never ended counts only where the system says when it ends
        assert!(!keeps_backend("0 running drew", 3, true));
        assert!(keeps_backend("0 running drew", 3, false));
    }

    #[test]
    fn on_an_intel_integrated_gpu_the_record_follows_its_order() {
        let intel = [1, 2, 0];
        // a first start: Direct3D 12
        assert_eq!(next_start("", &intel, true), 1);
        // a backend that has drawn is kept, Vulkan too (a machine whose Vulkan works, from an
        // earlier version)
        assert_eq!(next_start("1 ended drew", &intel, true), 1);
        assert_eq!(next_start("0 ended drew", &intel, true), 0);
        // stopped at risk: Direct3D 12, then OpenGL, then Vulkan, then round
        assert_eq!(next_start("1 trying", &intel, true), 2);
        assert_eq!(next_start("2 drawing", &intel, true), 0);
        // Vulkan crashed (an earlier version started there): Direct3D 12 next
        assert_eq!(next_start("0 trying", &intel, true), 1);
        assert_eq!(next_start("0 running drew", &intel, true), 1);
        // a run's own start follows it too
        let (_, start) = Run::after("", &intel, true);
        assert_eq!(start, 1);
    }

    #[test]
    fn the_record_reads_back_as_written_and_as_earlier_versions_wrote_it() {
        for stage in [Stage::Trying, Stage::Drawing, Stage::Running, Stage::Ended] {
            for drew in [false, true] {
                let record = Record { backend: 2, stage, drew };
                assert_eq!(Record::read(&record.text()), Some(record));
            }
        }
        assert_eq!(Record::read("1 running"), Some(Record { backend: 1, stage: Stage::Running, drew: false }));
        assert_eq!(Record::read("0 ended\n"), Some(Record { backend: 0, stage: Stage::Ended, drew: false }));
        assert_eq!(Record::read("0 ended sideways"), Some(Record { backend: 0, stage: Stage::Ended, drew: false }));
        assert_eq!(Record::read("drew 0"), None);
    }

    #[test]
    fn a_crash_before_a_canvas_has_drawn_moves_to_the_next_backend() {
        // as the device is made: the next start takes the second
        let (mut run, start) = Run::after("", &[0, 1], false);
        assert_eq!(start, 0);
        run.try_backend(0);
        assert_eq!(left(&run), "0 trying");
        assert_eq!(next_start(&left(&run), &[0, 1], false), 1);
        // after the window's first frame but before the pipelines are made
        run.made(0);
        run.show();
        assert_eq!(next_start(&left(&run), &[0, 1], false), 1);
        // the gallery shows: nothing at risk
        run.prepare(true);
        assert_eq!(left(&run), "0 running");
        // a canvas opens: at risk while its layers go up and until its first frame shows
        assert!(run.open_canvas());
        assert!(!run.open_canvas());
        assert_eq!(left(&run), "0 drawing");
        assert!(!run.canvas_frame(Some(false)));
        assert_eq!(left(&run), "0 drawing");
        assert_eq!(next_start(&left(&run), &[0, 1], false), 1);
        // the next start, on the second, is at risk as a canvas opens there as well
        let mut next = started_after(&left(&run));
        assert_eq!(next.backend, Some(1));
        assert!(next.open_canvas());
        assert_eq!(left(&next), "1 drawing");
    }

    #[test]
    fn a_backend_a_canvas_has_drawn_on_is_kept_whatever_ends_the_run_after() {
        let mut run = started_after("");
        run.open_canvas();
        assert!(run.canvas_frame(Some(true)));
        assert_eq!(left(&run), "0 running drew");
        // a crash, a log-out or the app killed from now on: the same backend
        assert_eq!(next_start(&left(&run), &[0, 1], false), 0);
        // the next run starts on it, tried again until its first frame, and kept as trusted
        let mut next = started_after(&left(&run));
        assert_eq!(left(&next), "0 running drew");
        assert!(!next.open_canvas());
        assert_eq!(left(&next), "0 running drew");
        assert!(!next.canvas_frame(Some(true)));
        next.end();
        assert_eq!(left(&next), "0 ended drew");
        assert_eq!(next_start(&left(&next), &[0, 1], false), 0);
    }

    #[test]
    fn a_trusted_backend_that_crashes_as_it_starts_gives_way_and_the_next_is_tried_afresh() {
        // a driver updated since: the trusted backend crashes as its device or pipelines are made
        let (mut run, start) = Run::after("0 ended drew", &[0, 1], false);
        assert_eq!(start, 0);
        run.try_backend(0);
        assert_eq!(left(&run), "0 trying drew");
        assert_eq!(next_start(&left(&run), &[0, 1], false), 1);
        // the second hasn't drawn a canvas: at risk as one opens
        let mut next = started_after(&left(&run));
        assert_eq!(left(&next), "1 running");
        assert!(next.open_canvas());
        assert_eq!(next_start(&left(&next), &[0, 1], false), 0);
    }

    #[test]
    fn quitting_or_logging_out_never_moves_a_backend() {
        // quit in the gallery, before any canvas has drawn
        let mut run = started_after("");
        run.end();
        assert_eq!(left(&run), "0 ended");
        assert_eq!(next_start(&left(&run), &[0, 1], false), 0);
        // quit while a canvas opens
        let mut run = started_after("");
        run.open_canvas();
        run.end();
        assert_eq!(left(&run), "0 ended");
        assert_eq!(next_start(&left(&run), &[0, 1], false), 0);
        // quit before the window's first frame
        let (mut run, _) = Run::after("", &[0, 1], false);
        run.try_backend(0);
        run.made(0);
        run.end();
        assert_eq!(next_start(&left(&run), &[0, 1], false), 0);
        // a log-out (nothing written) in the gallery, or with a canvas that closed before it showed
        let mut run = started_after("1 running");
        assert_eq!(next_start(&left(&run), &[0, 1], false), 1);
        run.open_canvas();
        assert!(!run.canvas_frame(None));
        assert_eq!(left(&run), "1 running");
        assert_eq!(next_start(&left(&run), &[0, 1], false), 1);
    }

    #[test]
    fn a_backend_whose_pipelines_failed_stays_tried_whatever_comes_after() {
        let (mut run, _) = Run::after("", &[0, 1], false);
        run.try_backend(0);
        run.made(0);
        run.prepare(false);
        run.show();
        run.open_canvas();
        run.canvas_frame(Some(true));
        assert_eq!(left(&run), "0 trying drew");
        run.end();
        assert_eq!(next_start(&left(&run), &[0, 1], false), 1);
    }

    #[test]
    fn a_canvas_opened_before_the_start_is_over_is_at_risk_until_it_shows() {
        // the app opened with a file: its canvas opens before the pipelines are made
        let (mut run, _) = Run::after("", &[0, 1], false);
        run.try_backend(0);
        run.made(0);
        assert!(run.open_canvas());
        assert_eq!(left(&run), "0 trying");
        run.show();
        run.prepare(true);
        assert_eq!(left(&run), "0 drawing");
        run.canvas_frame(Some(true));
        assert_eq!(left(&run), "0 running drew");
    }

    #[test]
    fn backends_that_fail_with_an_error_leave_the_last_tried() {
        // none made a device: the next start goes on from after the last
        let (mut run, _) = Run::after("", &[0, 1], false);
        run.try_backend(0);
        run.try_backend(1);
        run.end();
        assert_eq!(left(&run), "1 trying");
        assert_eq!(next_start(&left(&run), &[0, 1], false), 0);
        // the trusted one failing with an error, the next made: that one isn't trusted
        let (mut run, start) = Run::after("0 running drew", &[0, 1], false);
        run.try_backend(start);
        run.try_backend(1);
        run.made(1);
        run.show();
        run.prepare(true);
        assert_eq!(left(&run), "1 running");
        assert!(run.open_canvas());
    }

    #[test]
    fn on_windows_a_crash_after_a_canvas_has_drawn_still_moves_to_the_next_backend() {
        let mut run = started_on("", true);
        run.open_canvas();
        assert!(run.canvas_frame(Some(true)));
        assert_eq!(left(&run), "0 running drew");
        // it never ended: a crash or a kill, not a log-out (that writes "ended")
        assert!(run.at_risk(true));
        assert_eq!(next_start(&left(&run), &[0, 1], true), 1);
        // in the gallery before any canvas too
        assert_eq!(next_start("0 running", &[0, 1], true), 1);
        // the next start, on the second, isn't trusted: a canvas opening there is at risk
        let mut next = started_on(&left(&run), true);
        assert_eq!(next.backend, Some(1));
        assert!(next.open_canvas());
        assert_eq!(left(&next), "1 drawing");
        // a run that ended (quit, a log-out, a shut-down) keeps its backend
        run.end();
        assert_eq!(left(&run), "0 ended drew");
        assert!(!run.at_risk(true));
        assert_eq!(next_start(&left(&run), &[0, 1], true), 0);
    }

    #[test]
    fn on_linux_or_a_mac_a_crash_after_a_canvas_has_drawn_keeps_the_backend() {
        let mut run = started_on("", false);
        run.open_canvas();
        assert!(run.canvas_frame(Some(true)));
        assert_eq!(left(&run), "0 running drew");
        // a crash then looks just like a log-out: no mark against it
        assert!(!run.at_risk(false));
        assert_eq!(next_start(&left(&run), &[0, 1], false), 0);
        assert_eq!(next_start("0 running", &[0, 1], false), 0);
        // and the next start trusts it
        let mut next = started_on(&left(&run), false);
        assert_eq!(next.backend, Some(0));
        assert!(!next.open_canvas());
        // a crash before a canvas drew still moves on, on both
        for ends_told in [false, true] {
            let mut run = started_on("", ends_told);
            run.open_canvas();
            assert!(run.at_risk(ends_told));
            assert_eq!(next_start(&left(&run), &[0, 1], ends_told), 1);
        }
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
