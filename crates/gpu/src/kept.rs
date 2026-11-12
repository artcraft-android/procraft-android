//! The pipelines' compiled form, kept between runs where the driver hands it over (Vulkan): a later
//! start makes them without compiling their shaders again, most of the time a first canvas took to
//! open after the app had been closed.
//!
//! The file is the driver's data and a checksum after it. A start that loads it leaves a mark
//! beside it until the pipelines are made: a driver that crashes on the data then finds the mark
//! at the next start, and the data goes.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// Where the app keeps it (its own folder); none, none kept.
static FOLDER: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The most a kept file holds: a larger one is let go, and the cache made afresh. The driver
/// keeps what it was given with what is new, so each update with shaders of its own adds to it.
const MOST: u64 = 16 << 20;

/// Keeps the pipelines' compiled form in `dir` (the app's own folder), for devices made from now
/// on.
pub fn keep_pipelines_in(dir: PathBuf) {
    *FOLDER.lock().unwrap_or_else(PoisonError::into_inner) = Some(dir);
}

/// Where the pipelines' compiled form is kept, if anywhere.
pub(crate) fn folder() -> Option<PathBuf> {
    FOLDER.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// A device's pipeline cache and the file it is kept in.
pub(crate) struct Kept {
    pub(crate) cache: wgpu::PipelineCache,
    file: PathBuf,
    /// The checksum of what the file holds, if anything.
    saved: Mutex<Option<u64>>,
}

impl Kept {
    /// `device`'s pipeline cache kept in `dir`: with what an earlier run left there, else empty.
    /// None where the device can't hand its pipelines over.
    pub(crate) fn open(device: &wgpu::Device, adapter: &wgpu::Adapter, dir: &Path) -> Option<Kept> {
        if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
            return None;
        }
        let file = dir.join(wgpu::util::pipeline_cache_key(&adapter.get_info())?);
        if let Err(e) = std::fs::create_dir_all(dir) {
            log::warn!("the kept pipelines' folder: {e}");
            return None;
        }
        let mark = loading(&file);
        if mark.exists() {
            log::warn!("the kept pipelines stopped the last start before they were made: made afresh");
            let _ = std::fs::remove_file(&file);
            let _ = std::fs::remove_file(&mark);
        }
        let data = read(&file);
        if data.is_some()
            && let Err(e) = std::fs::write(&mark, b"")
        {
            // without the mark a crash on the data would come back each start
            log::warn!("marking the kept pipelines: {e}");
            return None;
        }
        // SAFETY: the data is what `PipelineCache::get_data` gave on an earlier run, whole (its
        // checksum checked), in a file named for the adapter (wgpu also checks the adapter and
        // driver it came from, and starts empty when they differ). A driver that crashes on it
        // anyway leaves the mark, and the next start goes without.
        #[allow(unsafe_code)]
        let cache = unsafe {
            device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                label: Some("kept pipelines"),
                data: data.as_ref().map(|(bytes, _)| bytes.as_slice()),
                fallback: true,
            })
        };
        log::info!("the kept pipelines: {} KB", data.as_ref().map_or(0, |(bytes, _)| bytes.len() / 1024));
        Some(Kept { cache, file, saved: Mutex::new(data.map(|(_, sum)| sum)) })
    }

    /// The pipelines are made with it: it didn't stop the start.
    pub(crate) fn made(&self) {
        let mark = loading(&self.file);
        if mark.exists()
            && let Err(e) = std::fs::remove_file(&mark)
        {
            log::warn!("the kept pipelines' mark: {e}");
        }
    }

    /// Keeps what the cache holds now, unless the file has it already.
    pub(crate) fn save(&self) {
        let Some(data) = self.cache.get_data() else {
            return;
        };
        let sum = checksum(&data);
        let mut saved = self.saved.lock().unwrap_or_else(PoisonError::into_inner);
        if *saved == Some(sum) {
            return;
        }
        match write(&self.file, &data, sum) {
            Ok(()) => {
                *saved = Some(sum);
                log::info!("the pipelines kept: {} KB", data.len() / 1024);
            }
            Err(e) => log::warn!("keeping the pipelines: {e}"),
        }
    }
}

/// The mark a start leaves while it loads `file`.
fn loading(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".loading");
    PathBuf::from(name)
}

/// What `file` keeps and its checksum, if it is whole.
fn read(file: &Path) -> Option<(Vec<u8>, u64)> {
    let size = std::fs::metadata(file).ok()?.len();
    if size > MOST {
        log::info!("the kept pipelines' file is {size} bytes: made afresh");
        return None;
    }
    let mut bytes = std::fs::read(file).ok()?;
    let at = bytes.len().checked_sub(8)?;
    let tail: [u8; 8] = bytes.get(at..)?.try_into().ok()?;
    bytes.truncate(at);
    let sum = checksum(&bytes);
    if sum != u64::from_le_bytes(tail) {
        log::warn!("the kept pipelines' file is damaged: not used");
        return None;
    }
    Some((bytes, sum))
}

/// Writes `data` and its checksum `sum` to `file` whole: into a file beside it, then put in its
/// place.
fn write(file: &Path, data: &[u8], sum: u64) -> std::io::Result<()> {
    let mut name = file.as_os_str().to_owned();
    name.push(".new");
    let beside = PathBuf::from(name);
    let mut bytes = Vec::with_capacity(data.len() + 8);
    bytes.extend_from_slice(data);
    bytes.extend_from_slice(&sum.to_le_bytes());
    std::fs::write(&beside, &bytes)?;
    std::fs::rename(&beside, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&beside);
    })
}

/// FNV-1a over `bytes`: enough to find a file damaged on the disk.
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("procraft-kept-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_kept_file_reads_back_whole_and_a_damaged_one_not_at_all() {
        let dir = folder("file");
        let file = dir.join("cache");
        let data = b"the driver's own bytes".to_vec();
        write(&file, &data, checksum(&data)).unwrap();
        assert_eq!(read(&file), Some((data.clone(), checksum(&data))));
        // nothing left beside it
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let mut damaged = std::fs::read(&file).unwrap();
        damaged[3] ^= 1;
        std::fs::write(&file, &damaged).unwrap();
        assert_eq!(read(&file), None);
        std::fs::write(&file, b"short").unwrap();
        assert_eq!(read(&file), None);
        assert_eq!(read(&dir.join("none")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pipelines_made_again_with_what_a_run_kept_and_a_crash_mark_drops_it() {
        let Ok(gpu) = crate::Gpu::headless() else {
            return;
        };
        let Some(name) = wgpu::util::pipeline_cache_key(&gpu.adapter.get_info()) else {
            return;
        };
        if !gpu.device.features().contains(wgpu::Features::PIPELINE_CACHE) {
            return;
        }
        let dir = folder("gpu");
        let file = dir.join(name);
        let first = Kept::open(&gpu.device, &gpu.adapter, &dir).unwrap();
        let _ = crate::canvas::Pipelines::new(
            &gpu.device,
            crate::canvas::liquify_format(&gpu.adapter),
            Some(first.cache.clone()),
        );
        first.made();
        first.save();
        let (kept, _) = read(&file).expect("the pipelines kept");
        assert!(!kept.is_empty());
        // a second start loads it, and leaves the mark until its pipelines are made
        let second = Kept::open(&gpu.device, &gpu.adapter, &dir).unwrap();
        assert!(loading(&file).exists());
        let _ = crate::canvas::Pipelines::new(
            &gpu.device,
            crate::canvas::liquify_format(&gpu.adapter),
            Some(second.cache.clone()),
        );
        second.made();
        assert!(!loading(&file).exists());
        second.save();
        assert!(read(&file).is_some());
        // a start that stopped while loading it: the next goes without
        std::fs::write(loading(&file), b"").unwrap();
        let third = Kept::open(&gpu.device, &gpu.adapter, &dir).unwrap();
        assert!(!file.exists() && !loading(&file).exists());
        assert_eq!(*third.saved.lock().unwrap(), None);
        assert_eq!(gpu.errors(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
