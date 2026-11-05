//! What the app keeps about its own runs: its log, written to a file in the app's folder too (a
//! release build on Windows has no console to show it), a crash's message in it, and which
//! graphics backend to start from: the next one after a run that never ended, as a driver that
//! takes the app down ends it.

use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::files;

/// The log, and the one from the run before.
const LOG: &str = "procraft.log";
const PREVIOUS_LOG: &str = "procraft-previous.log";
/// The backend the last run used, and whether that run is going still (or ended without
/// closing).
const GPU_START: &str = "gpu-start.txt";

/// The backend this run uses (`usize::MAX`: none yet, or not kept: a test's window).
static RUNNING: AtomicUsize = AtomicUsize::new(usize::MAX);

/// The log to the terminal and to the app folder's log file, and a crash's message into it.
pub fn init() {
    let dir = files::app_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::rename(dir.join(LOG), dir.join(PREVIOUS_LOG));
    let file = std::fs::File::create(dir.join(LOG)).ok();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        // a system font it can't read is left out; one line for each says nothing to act on
        .filter_module("fontdb", log::LevelFilter::Error)
        .target(env_logger::Target::Pipe(Box::new(Both(file))))
        .init();
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("crashed: {info}\n{}", std::backtrace::Backtrace::force_capture());
        before(info);
    }));
    log::info!("ProCraft {} on {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
}

/// Writes to the terminal and, if it could be made, the log file (each line as it comes, so a
/// crash leaves what led up to it).
struct Both(Option<std::fs::File>);

impl Write for Both {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if let Some(file) = self.0.as_mut() {
            let _ = file.write_all(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        if let Some(file) = self.0.as_mut() {
            let _ = file.flush();
        }
        Ok(())
    }
}

/// Whether this run keeps which backend it uses: not a test's window, which may be ended any
/// time.
fn kept() -> bool {
    std::env::var_os("PROCRAFT_BACKGROUND").is_none()
}

/// The backend to start from, of `count`: the one the last run used, or the next if that run
/// never ended.
pub fn gpu_start(count: usize) -> usize {
    if !kept() {
        return 0;
    }
    let saved = std::fs::read_to_string(files::app_dir().join(GPU_START)).unwrap_or_default();
    let start = next_start(&saved, count);
    if saved.trim().ends_with("running") {
        log::warn!("the last run ended without closing (on graphics backend {}): starting from {start}", saved.trim());
    }
    start
}

/// This run uses backend `index`: kept as going until it ends.
pub fn gpu_running(index: usize) {
    if kept() {
        RUNNING.store(index, Ordering::Relaxed);
        let _ = std::fs::write(files::app_dir().join(GPU_START), format!("{index} running"));
    }
}

/// This run is ending as it should: its backend is kept for the next.
pub fn gpu_ended() {
    let index = RUNNING.load(Ordering::Relaxed);
    if index != usize::MAX {
        let _ = std::fs::write(files::app_dir().join(GPU_START), format!("{index} ended"));
    }
}

/// The backend to start from, of `count`, after the last run's record `saved` ("<index>
/// running" or "<index> ended"): the next one if that run never ended, else the same one;
/// the first without a record.
fn next_start(saved: &str, count: usize) -> usize {
    let mut words = saved.split_whitespace();
    let (Some(Ok(index)), Some(state)) = (words.next().map(str::parse::<usize>), words.next()) else { return 0 };
    let count = count.max(1);
    match state {
        "running" => (index + 1) % count,
        _ if index < count => index,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::next_start;

    #[test]
    fn a_run_that_never_ended_starts_the_next_from_the_next_backend() {
        assert_eq!(next_start("", 2), 0);
        assert_eq!(next_start("0 ended", 2), 0);
        assert_eq!(next_start("1 ended", 2), 1);
        // the run on the first never ended: the second; on the last: round to the first
        assert_eq!(next_start("0 running", 2), 1);
        assert_eq!(next_start("1 running", 2), 0);
        // fewer backends now (one named by WGPU_BACKEND), or a record it can't read
        assert_eq!(next_start("1 ended", 1), 0);
        assert_eq!(next_start("1 running", 1), 0);
        assert_eq!(next_start("garbled", 2), 0);
    }
}
