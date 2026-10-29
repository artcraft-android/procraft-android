//! The app's GPU device: one for every artwork, as making one compiles the canvases' pipelines (a
//! second and a half on a tablet).

use std::sync::{Mutex, MutexGuard, PoisonError};

use procraft_gpu::Gpu;

static DEVICE: Mutex<Option<Gpu>> = Mutex::new(None);

/// The app's device, held while one is made so that two aren't made at once.
pub(crate) fn kept() -> MutexGuard<'static, Option<Gpu>> {
    DEVICE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Makes the app's device and its canvases' pipelines ahead of the first artwork, so that it opens
/// at once; nothing if it is made. Slow: call it off the main thread.
#[uniffi::export]
pub fn warm_gpu() {
    let mut kept = kept();
    if kept.is_some() {
        return;
    }
    match Gpu::headless() {
        Ok(gpu) => {
            gpu.prepare_canvases();
            *kept = Some(gpu);
        }
        Err(e) => log::error!("making the GPU device: {e}"),
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
}
