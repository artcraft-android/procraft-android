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
//!
//! A crash (or a panic on the window's thread) while a backend starts, or as a canvas first draws
//! on it, starts the app again at once on the next backend, until each has been tried; then, or
//! where it can't start again, it says why it stopped (see [`stopped`]).

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

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
/// The app has stopped (a crash, a panic on the window's thread): the record stays as it was then,
/// whatever the other threads do before the process ends.
static STOPPED: AtomicBool = AtomicBool::new(false);
/// Whether the system tells the app when the session is ending (Windows): there a run that never
/// ended was a crash, or the app killed.
const ENDS_TOLD: bool = cfg!(windows);
/// The log's file, let go before the app starts itself again (the new one's log takes its place).
static LOG_FILE: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);
/// When the window last showed a frame, and when its thread last answered the start's watch, ms
/// after the app started (none yet: 0).
static LAST_FRAME: AtomicU64 = AtomicU64::new(0);
static LAST_ANSWER: AtomicU64 = AtomicU64::new(0);
/// How often the start is logged while it isn't over, and for how long at most.
const START_WATCH: Duration = Duration::from_secs(5);
const START_WATCH_MOST: Duration = Duration::from_secs(10 * 60);
/// When the app was started.
static LAUNCHED: OnceLock<Instant> = OnceLock::new();

/// The log to the terminal and to the app folder's log file, and a crash's message into it. A
/// start the app made itself on another graphics backend goes on in the same log, so it holds
/// every backend's try (the run before them all stays the previous log).
pub fn init() {
    LAUNCHED.get_or_init(Instant::now);
    let dir = files::app_dir();
    let _ = std::fs::create_dir_all(&dir);
    let tried = gpu_tried();
    let file = if tried > 0 {
        std::fs::OpenOptions::new().create(true).append(true).open(dir.join(LOG))
    } else {
        let _ = std::fs::rename(dir.join(LOG), dir.join(PREVIOUS_LOG));
        std::fs::File::create(dir.join(LOG))
    };
    *LOG_FILE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = file.ok();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        // a system font it can't read is left out; one line for each says nothing to act on
        .filter_module("fontdb", log::LevelFilter::Error)
        // each line with the time since the app started: where a slow start spent it shows
        .format(|out, record| {
            writeln!(out, "[{:>7} ms {:<5} {}] {}", since_launch(), record.level(), record.target(), record.args())
        })
        .target(env_logger::Target::Pipe(Box::new(Both)))
        .init();
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panicked: {info}\n{}", std::backtrace::Backtrace::force_capture());
        before(info);
    }));
    let again = if tried > 0 { format!(", started again with {tried} graphics backends tried") } else { String::new() };
    log::info!("ProCraft {} on {} {}{again}", env!("PROCRAFT_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
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
/// once): its log let go first, so the new one can keep it. `tried`: how many graphics backends
/// this start has tried, so the new one tries only those left (see [`gpu_tried`]); none, a new
/// start. Whether the new one started.
pub fn start_again(tried: Option<usize>) -> bool {
    spawn_again(tried, true)
}

/// [`start_again`], with the files the app was given or without them; whether the new process
/// started. The log is let go only if no other thread holds it (a crash may have stopped one that
/// did).
fn spawn_again(tried: Option<usize>, with_files: bool) -> bool {
    log::info!("starting again");
    if let Ok(mut file) = LOG_FILE.try_lock() {
        file.take();
    }
    let started = std::env::current_exe().and_then(|me| {
        let mut again = std::process::Command::new(me);
        if with_files {
            again.args(std::env::args_os().skip(1));
        }
        match tried {
            Some(n) => again.env(TRIED, n.to_string()),
            None => again.env_remove(TRIED),
        };
        again.spawn()
    });
    started.inspect_err(|e| log::error!("starting again: {e}")).is_ok()
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
    let (mut run, start) = Run::after(&saved, order, ENDS_TOLD);
    run.tried = gpu_tried();
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
    // the drivers are loaded now, and one may have put its own filter in place of the app's
    #[cfg(windows)]
    native::watch_again();
}

/// The graphics device was lost. While a backend starts, or as a canvas first draws on it, that
/// counts against the backend, and the app is to start again on the next: how many have been
/// tried, then (see [`start_on_next`]). Else none.
pub fn device_lost() -> Option<usize> {
    let mut run = run_now();
    match run.stop(Gpu::backend_count(), ENDS_TOLD) {
        Stop::Again(tried) if kept() => {
            run.prepare(false);
            keep(&mut run);
            Some(tried)
        }
        _ => None,
    }
}

/// Starts the app again on the next backend, `tried` tried, without the files it was opened with
/// (they may have been taken in already); whether it started.
pub fn start_on_next(tried: usize) -> bool {
    spawn_again(Some(tried), false)
}

/// The window has shown a frame: the first ends a part of the start (see [`gpu_prepared`]).
pub fn gpu_shown() {
    LAST_FRAME.store(since_launch().max(1), Ordering::Relaxed);
    let mut run = run_now();
    if !run.show() {
        return;
    }
    log::info!("the window's first frame shown, {} ms after the app started", since_launch());
    keep(&mut run);
}

/// How long ago the app was started, ms.
fn since_launch() -> u64 {
    LAUNCHED.get().map_or(0, |t| u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX))
}

/// Logs, every few seconds while the start isn't over, what it still waits for, whether the
/// window's thread answers (`wake` asks it; see [`answered`]) and when it last showed a frame. A
/// log that ends there tells a slow start (still waiting, the window answering) from a stuck one
/// (the window not answering: it stands "not responding") and from one cut short (a crash logs
/// itself; with nothing, the app was ended).
pub fn watch_start(wake: winit::event_loop::EventLoopProxy<()>) {
    let watch = std::thread::Builder::new().name("procraft-start-watch".to_owned()).spawn(move || {
        let watching = Instant::now();
        while watching.elapsed() < START_WATCH_MOST {
            let asked = since_launch();
            let _ = wake.send_event(());
            std::thread::sleep(START_WATCH);
            let Some(awaited) = run_now().awaited() else { return };
            let now = since_launch();
            let (answer, frame) = (LAST_ANSWER.load(Ordering::Relaxed), LAST_FRAME.load(Ordering::Relaxed));
            let thread = if answer >= asked {
                "the window answers".to_owned()
            } else {
                format!("the window hasn't answered for {} ms", now.saturating_sub(asked))
            };
            let shown = if frame == 0 {
                "no frame shown yet".to_owned()
            } else {
                format!("its last frame {} ms ago", now.saturating_sub(frame))
            };
            log::info!("{} s after the app started, still {}; {thread}, {shown}", now / 1000, awaited.words());
        }
    });
    if let Err(e) = watch {
        log::warn!("the start's watch: {e}");
    }
}

/// The window's thread has answered the start's watch (see [`watch_start`]).
pub fn answered() {
    LAST_ANSWER.store(since_launch().max(1), Ordering::Relaxed);
}

/// The canvases' pipelines are made, `fine` without a GPU error. Once the window's first frame
/// has shown too, the backend's try is over; one that failed stays tried, whatever comes after.
pub fn gpu_prepared(fine: bool) {
    let mut run = run_now();
    run.prepare(fine);
    keep(&mut run);
    #[cfg(windows)]
    native::watch_again();
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
    if record == run.written || !recorded() || STOPPED.load(Ordering::SeqCst) {
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
    /// How many backends have been tried, with those tried before the app started itself again:
    /// the one being tried the last of them.
    tried: usize,
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
        tried: 0,
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
        self.tried += 1;
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

    /// What the start still waits for; none once it is over, its pipelines failed or the app
    /// ends (or before a backend is tried).
    fn awaited(&self) -> Option<Awaited> {
        if self.ended || self.failed {
            return None;
        }
        match (self.backend, self.trying, self.shown, self.prepared) {
            (None, Some(trying), ..) => Some(Awaited::Backend(trying)),
            (None, None, ..) | (Some(_), _, true, true) => None,
            (Some(_), _, false, false) => Some(Awaited::FrameAndPipelines),
            (Some(_), _, false, true) => Some(Awaited::Frame),
            (Some(_), _, true, false) => Some(Awaited::Pipelines),
        }
    }

    /// Whether a crash now would count against the backend (see [`Stage::counts`]).
    fn at_risk(&self, ends_told: bool) -> bool {
        self.record().is_some_and(|r| r.stage.counts(ends_told))
    }

    /// What the app does if it stops now (a crash, a panic on the window's thread), with `count`
    /// backends: while its backend starts, or as a canvas first draws on it, it starts again on
    /// the next, until each has been tried; else it says so (see [`Stop`]).
    fn stop(&self, count: usize, ends_told: bool) -> Stop {
        let Some(record) = self.record() else { return Stop::Say };
        if count < 2 {
            return Stop::Say;
        }
        match record.stage {
            Stage::Trying | Stage::Drawing if self.tried < count => Stop::Again(self.tried),
            Stage::Trying | Stage::Drawing => Stop::SayAllTried,
            _ if self.at_risk(ends_told) => Stop::SayAnother,
            _ => Stop::Say,
        }
    }
}

/// What the start still waits for (see [`Run::awaited`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Awaited {
    /// A backend's device, the window's surface and the interface's pipelines, on this backend.
    Backend(usize),
    /// The window's first frame and the canvases' pipelines.
    FrameAndPipelines,
    /// The window's first frame.
    Frame,
    /// The canvases' pipelines.
    Pipelines,
}

impl Awaited {
    fn words(self) -> String {
        match self {
            Awaited::Backend(index) => format!(
                "setting up graphics backend {index} ({}): its device, the window's surface, the interface's pipelines",
                Gpu::backend_name(index)
            ),
            Awaited::FrameAndPipelines => "showing the window's first frame and making the canvases' pipelines".into(),
            Awaited::Frame => "showing the window's first frame".into(),
            Awaited::Pipelines => "making the canvases' pipelines".into(),
        }
    }
}

/// What the app does as it stops (see [`Run::stop`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    /// Starts itself again on the next backend, this many tried.
    Again(usize),
    /// Says why it stopped, and that starting it again tries another backend.
    SayAnother,
    /// Says why it stopped, and that every backend was tried.
    SayAllTried,
    /// Says why it stopped.
    Say,
}

/// The app stopped on `what` (a crash, a panic's message on the window's thread), and is to end:
/// started again on the next backend, if it can be (see [`Run::stop`]), else said, with where its
/// log is (only logged for a test's window). The backend record stays as it is now.
pub fn stopped(what: &str) {
    log::error!("stopped: {what}");
    STOPPED.store(true, Ordering::SeqCst);
    if !kept() {
        return;
    }
    let stop = run_if_free().map_or(Stop::Say, |run| run.stop(Gpu::backend_count(), ENDS_TOLD));
    let stop = match stop {
        // without the files it was opened with: they may have been taken in already
        Stop::Again(tried) if apart("procraft-again", move || start_on_next(tried)) == Some(true) => {
            return;
        }
        Stop::Again(_) => Stop::SayAnother,
        stop => stop,
    };
    let text = stopped_text(what, stop);
    apart("procraft-stopped", move || files::tell("ProCraft stopped", &text));
}

/// This run's part in the record as it is now, if another thread lets go of it soon: a crash may
/// have stopped the thread holding it.
fn run_if_free() -> Option<Run> {
    for _ in 0..20 {
        match RUN.try_lock() {
            Ok(run) => return Some(*run),
            Err(std::sync::TryLockError::Poisoned(run)) => return Some(*run.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    None
}

/// Runs `work` on a thread of its own and waits for it: on the window's thread, a crash in the
/// middle of the window's own messages keeps a message from ever showing. A thread that doesn't
/// start (the crash holding the system's loader) isn't waited for. None if it didn't start or
/// panicked.
fn apart<T: Send + 'static>(name: &str, work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (started, start) = std::sync::mpsc::channel();
    let thread = std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            let _ = started.send(());
            work()
        })
        .ok()?;
    start.recv_timeout(Duration::from_secs(3)).ok()?;
    thread.join().ok()
}

/// What [`stopped`] says, as it does after `stop`.
fn stopped_text(what: &str, stop: Stop) -> String {
    let what = what.trim().trim_end_matches('.');
    let what = if what.is_empty() { "something went wrong" } else { what };
    let after = match stop {
        Stop::SayAnother | Stop::Again(_) => "\n\nStart it again to try another graphics backend.",
        Stop::SayAllTried => "\n\nEach graphics backend has been tried. Updating the graphics driver may help.",
        Stop::Say => "",
    };
    format!("ProCraft stopped: {what}.{after}\n\nIts log is {}", files::app_dir().join(LOG).display())
}

/// What the app says when no backend could draw the window, `why` each failed.
pub fn no_gpu_text(why: &str) -> String {
    format!(
        "No graphics backend could draw the window.\n\n{why}\n\nUpdating the graphics driver may help. ProCraft's log is {}",
        files::app_dir().join(LOG).display()
    )
}

/// A crash in the system's or a driver's own code (Windows): logged with the file it happened in,
/// then the app started again on the next backend or the crash said (see [`stopped`]), rather
/// than the app vanishing.
#[cfg(windows)]
#[allow(unsafe_code)]
mod native {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, EXCEPTION_POINTERS, SetUnhandledExceptionFilter,
    };
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

    /// A stack overflow: little stack is left for the code that sees it.
    const STACK_OVERFLOW: u32 = 0xC000_00FD;
    /// What ends the process if no code handles it: an access violation, a page that can't be
    /// read, an illegal or privileged instruction, a stack overflow.
    const FATAL: [u32; 5] = [0xC000_0005, 0xC000_0006, 0xC000_001D, 0xC000_0096, STACK_OVERFLOW];
    /// How many of them are logged as they happen, and how many have been seen.
    const MOST_SEEN: u32 = 8;
    static SEEN: AtomicU32 = AtomicU32::new(0);
    /// `EXCEPTION_CONTINUE_SEARCH`, as a vectored handler returns it.
    const GO_ON: i32 = 0;

    pub fn watch() {
        // SAFETY: both are handlers as the system calls them, for the whole process
        unsafe {
            SetUnhandledExceptionFilter(Some(crashed));
            AddVectoredExceptionHandler(0, Some(seen));
        }
    }

    /// Puts the filter back if something has put its own in its place (only the last set is
    /// called: a driver's runtime replacing it would leave a crash with no word at all).
    pub fn watch_again() {
        // SAFETY: as in `watch`
        let before = unsafe { SetUnhandledExceptionFilter(Some(crashed)) };
        if before.map(|filter| filter as *const ()) != Some(crashed as *const ()) {
            log::warn!("something had replaced the app's crash filter: put back");
        }
    }

    /// Logs an exception that would end the process as it happens, before any code handles it: a
    /// driver may handle it itself, but what ends the process with no filter called (the driver's
    /// own abort, say) then still leaves where it went wrong.
    unsafe extern "system" fn seen(info: *mut EXCEPTION_POINTERS) -> i32 {
        // SAFETY: the system gives the exception's pointers, valid while the handler runs
        let record = unsafe { info.as_ref().and_then(|i| i.ExceptionRecord.as_ref()) };
        let Some(record) = record else { return GO_ON };
        let code = record.ExceptionCode.0.cast_unsigned();
        if FATAL.contains(&code) && code != STACK_OVERFLOW && SEEN.fetch_add(1, Ordering::Relaxed) < MOST_SEEN {
            let file = module_of(record.ExceptionAddress).unwrap_or_else(|| "the app's code".to_owned());
            log::warn!("an exception {code:#010X} in {file} (it ends the app if nothing handles it)");
        }
        GO_ON
    }

    /// A crash no code handled: its exception's code and the file of the code it happened in,
    /// logged; the app started again or the crash said (see [`super::stopped`]); then the app
    /// ends.
    unsafe extern "system" fn crashed(info: *const EXCEPTION_POINTERS) -> i32 {
        if CRASHED.swap(true, Ordering::SeqCst) {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        // SAFETY: the system gives the crash's pointers, valid while the filter runs
        let record = unsafe { info.as_ref().and_then(|i| i.ExceptionRecord.as_ref()) };
        let (code, address) =
            record.map_or((0, 0), |r| (r.ExceptionCode.0.cast_unsigned(), r.ExceptionAddress as usize));
        // on a stack of its own: after a stack overflow this one has next to none left, and a
        // second crash here would end the app with no word
        if super::apart("procraft-crash", move || said(code, address)).is_none() {
            said(code, address);
        }
        EXCEPTION_EXECUTE_HANDLER
    }

    /// The crash logged, and the app started again or the crash said.
    fn said(code: u32, address: usize) {
        let file = module_of(address as *mut core::ffi::c_void).unwrap_or_else(|| "the app's code".to_owned());
        log::error!("crashed in {file} (exception {code:#010X} at {address:#x})");
        super::stopped(&format!("it crashed in {file} (exception {code:#010X})"));
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
        let text = super::stopped_text("wgpu error: Validation Error.\n", Stop::Say);
        assert!(text.starts_with("ProCraft stopped: wgpu error: Validation Error."), "{text}");
        assert!(text.contains("procraft.log"), "{text}");
        assert!(!text.contains("graphics"), "{text}");
        let text = super::stopped_text("", Stop::Say);
        assert!(text.starts_with("ProCraft stopped: something went wrong."), "{text}");
        let text = super::stopped_text("it crashed in igvk64.dll (exception 0xC0000005)", Stop::SayAnother);
        assert!(text.contains("Start it again to try another graphics backend."), "{text}");
        let text = super::stopped_text("it crashed in a driver (exception 0xC0000005)", Stop::SayAllTried);
        assert!(text.contains("Each graphics backend has been tried."), "{text}");
        assert!(!text.contains("Start it again"), "{text}");
    }

    use super::Stop;

    /// A run on the first of `count` backends, `tried` of them tried before it (the app started
    /// itself again), its device being made.
    fn trying(count: usize, tried: usize) -> Run {
        let order: Vec<usize> = (0..count).collect();
        let (mut run, start) = Run::after("", &order, true);
        run.tried = tried;
        run.try_backend(start);
        run
    }

    #[test]
    fn a_crash_as_a_backend_starts_starts_the_app_again_on_the_next_until_each_is_tried() {
        // the first of three crashes as its device, surface or pipelines are made, or before the
        // window's first frame: started again, one tried
        let mut run = trying(3, 0);
        assert_eq!(run.stop(3, true), Stop::Again(1));
        run.made(0);
        assert_eq!(run.stop(3, true), Stop::Again(1));
        run.show();
        assert_eq!(run.stop(3, true), Stop::Again(1));
        // the next start (one tried): the second crashes too, started again, two tried
        assert_eq!(trying(3, 1).stop(3, true), Stop::Again(2));
        // the last crashes: each has been tried, so it says so instead of going round
        assert_eq!(trying(3, 2).stop(3, true), Stop::SayAllTried);
        // on Linux or a Mac alike
        assert_eq!(trying(2, 0).stop(2, false), Stop::Again(1));
        assert_eq!(trying(2, 1).stop(2, false), Stop::SayAllTried);
        // a backend failing with an error in the same start counts as tried
        let mut run = trying(3, 0);
        run.try_backend(1);
        assert_eq!(run.stop(3, true), Stop::Again(2));
    }

    #[test]
    fn a_crash_as_a_canvas_first_draws_starts_the_app_again_too() {
        let mut run = started_on("", true);
        run.tried = 1;
        assert!(run.open_canvas());
        assert_eq!(run.stop(2, true), Stop::Again(1));
        assert_eq!(run.stop(2, false), Stop::Again(1));
        // on the last backend left: said
        run.tried = 2;
        assert_eq!(run.stop(2, true), Stop::SayAllTried);
    }

    #[test]
    fn a_crash_once_started_is_said_as_before() {
        // in the gallery, the backend's try over, before any canvas: on Windows the next start
        // takes the next backend, so it says so; elsewhere it keeps it
        let mut run = started_on("", true);
        run.tried = 1;
        assert_eq!(run.stop(2, true), Stop::SayAnother);
        assert_eq!(run.stop(2, false), Stop::Say);
        // after a canvas has drawn
        run.open_canvas();
        run.canvas_frame(Some(true));
        assert_eq!(run.stop(2, true), Stop::SayAnother);
        assert_eq!(run.stop(2, false), Stop::Say);
        // on a backend a canvas has drawn on before, a canvas opening isn't a start's
        let mut run = started_on("0 ended drew", true);
        run.tried = 1;
        assert!(!run.open_canvas());
        assert_eq!(run.stop(2, false), Stop::Say);
        // ending as it should
        run.end();
        assert_eq!(run.stop(2, true), Stop::Say);
    }

    #[test]
    fn a_process_that_ends_with_no_word_moves_the_next_start_on_as_a_crash_does() {
        // nothing runs as the process goes (a driver's own abort, a stack overflow): what counts
        // is what the record already says
        for ends_told in [false, true] {
            let mut run = trying(2, 0);
            assert_eq!(next_start(&left(&run), &[0, 1], ends_told), 1);
            run.made(0);
            run.show();
            assert_eq!(next_start(&left(&run), &[0, 1], ends_told), 1);
        }
        // once started, in the gallery or a canvas: on Windows
        assert_eq!(next_start(&left(&started_on("", true)), &[0, 1], true), 1);
    }

    #[test]
    fn a_backend_losing_its_device_as_it_starts_stays_tried_however_the_app_ends() {
        let mut run = trying(2, 0);
        run.made(0);
        run.show();
        assert_eq!(run.stop(2, true), Stop::Again(1));
        // as `device_lost` marks it; the app then ends as it should, to start on the next
        run.prepare(false);
        run.end();
        assert_eq!(next_start(&left(&run), &[0, 1], true), 1);
    }

    #[test]
    fn with_one_backend_or_none_tried_yet_a_crash_is_only_said() {
        // one backend (named by WGPU_BACKEND): nothing to go on to
        assert_eq!(trying(1, 0).stop(1, true), Stop::Say);
        // before any backend is tried
        let (run, _) = Run::after("", &[0, 1], true);
        assert_eq!(run.stop(2, true), Stop::Say);
    }

    #[test]
    fn the_start_s_watch_says_what_it_waits_for_until_the_start_is_over() {
        use super::Awaited;
        let (mut run, _) = Run::after("", &[0, 1], true);
        assert_eq!(run.awaited(), None);
        // a backend being set up, then the next after it failed with an error
        run.try_backend(0);
        assert_eq!(run.awaited(), Some(Awaited::Backend(0)));
        run.try_backend(1);
        assert_eq!(run.awaited(), Some(Awaited::Backend(1)));
        // its device made: the first frame and the pipelines, then the one left
        run.made(1);
        assert_eq!(run.awaited(), Some(Awaited::FrameAndPipelines));
        run.show();
        assert_eq!(run.awaited(), Some(Awaited::Pipelines));
        run.prepare(true);
        assert_eq!(run.awaited(), None);
        // the pipelines made before the first frame
        let mut run = trying(2, 0);
        run.made(0);
        run.prepare(true);
        assert_eq!(run.awaited(), Some(Awaited::Frame));
        // pipelines that failed (the app starts again, or the next start takes the next), or the
        // app ending: nothing more to wait for
        let mut run = trying(2, 0);
        run.made(0);
        run.prepare(false);
        assert_eq!(run.awaited(), None);
        let mut run = trying(2, 0);
        run.end();
        assert_eq!(run.awaited(), None);
    }
}
