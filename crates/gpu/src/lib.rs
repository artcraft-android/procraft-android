//! ProCraft's GPU renderer (wgpu): layer pixels, stroke rendering, compositing, and drawing the
//! canvas to a window surface. Layers are premultiplied RGBA8 textures the size of the canvas.
//!
//! Every blend here uses fixed-function blending or reads from a different texture than it writes:
//! nothing relies on reading the render target in the shader (Android GPUs have no portable way).

mod canvas;
mod lighting;
mod scene;
mod sphere;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

pub use canvas::{
    Backdrop, Canvas, DisplayLut, Filter, LiquifyMode, LiquifyStamp, Readback, ReadyToPark, RemakeStats, Resample,
    SecondStroke, SelectionLook, SharedStroke, SmallScene, Stripes, StrokeStyle, StrokeTool, SurroundGrid, Target,
    Thumbnail, most_resident, read_texture, start_reading, textures_bytes,
};
pub use lighting::Lighting;
pub use scene::{GrainLook, Light, MOST_LIGHTS, MaterialLook, MeshView, Orbit, SurfaceMap, material_source};
pub use sphere::{SphereLook, SpherePreview, preview_environment};
pub use wgpu;

/// The canvas's textures are made so the GPU keeps them uncompressed: on Mali GPUs (Vulkan), whose
/// block compression (AFBC) can lose blocks of a picture redrawn a part at a time (a stroke's
/// live picture showed holes in a grid on a Mali-G68).
static UNCOMPRESSED: AtomicBool = AtomicBool::new(false);

/// Keeps the canvas's textures made from now on uncompressed (`on`), as on a GPU that needs it:
/// for tests on other GPUs.
#[doc(hidden)]
pub fn keep_textures_uncompressed(on: bool) {
    UNCOMPRESSED.store(on, Ordering::Relaxed);
}

/// Whether the canvas's textures are to be kept uncompressed (see [`UNCOMPRESSED`]).
pub(crate) fn textures_uncompressed() -> bool {
    UNCOMPRESSED.load(Ordering::Relaxed)
}

/// What a panic said (its message, when it gave one as text).
pub fn panic_message(panic: &(dyn std::any::Any + Send)) -> &str {
    panic.downcast_ref::<&str>().copied().or_else(|| panic.downcast_ref::<String>().map(String::as_str)).unwrap_or("")
}

/// The graphics APIs in `backends` by name ("Vulkan", "Direct3D 12", "OpenGL", ...).
fn backends_name(backends: wgpu::Backends) -> String {
    let named = [
        (wgpu::Backends::VULKAN, "Vulkan"),
        (wgpu::Backends::DX12, "Direct3D 12"),
        (wgpu::Backends::METAL, "Metal"),
        (wgpu::Backends::GL, "OpenGL"),
    ];
    let names: Vec<&str> = named.iter().filter(|(b, _)| backends.contains(*b)).map(|(_, name)| *name).collect();
    if names.is_empty() { format!("{backends:?}") } else { names.join(" or ") }
}

/// Whether a GPU named `name` on `backend` needs its canvas textures uncompressed.
fn keeps_textures_uncompressed(name: &str, backend: wgpu::Backend) -> bool {
    backend == wgpu::Backend::Vulkan && name.to_ascii_lowercase().contains("mali")
}

#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("no suitable GPU: {0}")]
    NoAdapter(String),
    #[error("GPU device: {0}")]
    Device(String),
    #[error("canvas of {0}×{1} pixels is larger than this GPU allows ({2})")]
    TooLarge(u32, u32, u32),
    #[error("surface: {0}")]
    Surface(String),
    #[error("reading pixels back: {0}")]
    Readback(String),
    #[error("the GPU is out of memory: {0}")]
    OutOfMemory(String),
    #[error("the GPU device was lost")]
    Lost,
    #[error("GPU work failed: {0}")]
    Failed(String),
}

/// The device everything renders with. A clone is the same device, sharing what is made for it.
#[derive(Clone)]
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    errors: Arc<AtomicU64>,
    /// Whether the device has been lost (its driver reset or stopped).
    lost: Arc<AtomicBool>,
    /// The canvases' pipelines, once the first canvas has made them.
    canvas_pipelines: Arc<OnceLock<Arc<canvas::Pipelines>>>,
    /// Buffers pixels came back from the GPU through, kept for the next readback (see
    /// [`Gpu::readback_buffer`]).
    readbacks: Arc<std::sync::Mutex<Vec<wgpu::Buffer>>>,
}

impl Gpu {
    /// A wgpu instance for this platform: Vulkan on Android, the native API elsewhere.
    pub fn instance() -> wgpu::Instance {
        Gpu::instance_after(0)
            .unwrap_or_else(|| wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle()))
    }

    /// The backends a device is made from, best first: those `WGPU_BACKEND` names if it is set
    /// (to try another when a driver fails); on Android Vulkan, then OpenGL ES (for a device
    /// whose Vulkan is missing or fails); on Windows Vulkan and Direct3D 12, Direct3D 12 first on
    /// ARM (the Snapdragon X's Vulkan driver fails as a canvas opens), then OpenGL (a GPU or
    /// driver too old for either); on Linux Vulkan, then OpenGL (the same); elsewhere the native
    /// API.
    fn backends() -> Vec<wgpu::Backends> {
        if let Some(chosen) = wgpu::Backends::from_env().filter(|b| !b.is_empty()) {
            return vec![chosen];
        }
        if cfg!(target_os = "android") {
            vec![wgpu::Backends::VULKAN, wgpu::Backends::GL]
        } else if cfg!(all(windows, target_arch = "aarch64")) {
            vec![wgpu::Backends::DX12, wgpu::Backends::VULKAN, wgpu::Backends::GL]
        } else if cfg!(windows) {
            vec![wgpu::Backends::VULKAN, wgpu::Backends::DX12, wgpu::Backends::GL]
        } else if cfg!(target_os = "linux") {
            vec![wgpu::Backends::VULKAN, wgpu::Backends::GL]
        } else {
            vec![wgpu::Backends::PRIMARY]
        }
    }

    /// How many backends there are to make a device from (see [`Gpu::instance_after`]).
    pub fn backend_count() -> usize {
        Gpu::backends().len()
    }

    /// Backend `index`'s name, for the log and messages (see [`Gpu::instance_after`]).
    pub fn backend_name(index: usize) -> String {
        Gpu::backends().get(index).map_or_else(|| format!("backend {index}"), |b| backends_name(*b))
    }

    /// The instance to try after `tries` failed to make a device (see [`Gpu::backends`]); None
    /// past the last.
    pub fn instance_after(tries: usize) -> Option<wgpu::Instance> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = *Gpu::backends().get(tries)?;
        Some(wgpu::Instance::new(desc))
    }

    /// [`Gpu::instance_after`] for windows on `display`: OpenGL under Wayland and X11 can show
    /// nothing on a window without it, its surface failing as the window's is set up.
    pub fn instance_for_display(
        tries: usize,
        display: Box<dyn wgpu::wgt::WgpuHasDisplayHandle>,
    ) -> Option<wgpu::Instance> {
        let mut desc = wgpu::InstanceDescriptor::new_with_display_handle(display);
        desc.backends = *Gpu::backends().get(tries)?;
        Some(wgpu::Instance::new(desc))
    }

    /// Sends the writes queued so far to the device and waits for them, so the copies they were
    /// staged in are let go.
    pub fn flush(&self) {
        self.queue.submit(std::iter::empty());
        if let Err(e) = self.device.poll(wgpu::PollType::wait_indefinitely()) {
            log::error!("flushing uploads: {e}");
        }
    }

    /// A device for offscreen work (tests, exports), from the first instance that makes one.
    pub fn headless() -> Result<Gpu, GpuError> {
        Gpu::first_that(0, |_| {}, |_| true).map(|(gpu, _)| gpu)
    }

    /// The app's device, the canvases' pipelines made: from the first instance that makes one
    /// and builds them without an error or losing it (a driver that fails on them gives way to
    /// the next backend; the last is kept as it is).
    pub fn for_canvases() -> Result<Gpu, GpuError> {
        Gpu::for_canvases_from(0, |_| {}).map(|(gpu, _)| gpu)
    }

    /// [`Gpu::for_canvases`] from backend `from` on (round to those before it), `trying` told
    /// each backend's index before it is tried (a driver that takes the app down leaves no error
    /// to see); the device, and its backend's index.
    pub fn for_canvases_from(from: usize, trying: impl FnMut(usize)) -> Result<(Gpu, usize), GpuError> {
        Gpu::first_that(from, trying, |gpu| {
            gpu.prepare_canvases();
            let fine = gpu.errors() == 0 && !gpu.is_lost();
            if !fine {
                log::warn!("the canvases' pipelines: {} GPU errors, lost {}", gpu.errors(), gpu.is_lost());
            }
            fine
        })
    }

    /// A device from the first instance, from backend `from` on, that makes one `fine` takes, or
    /// the last's; a backend whose driver panics making it or in `fine` fails as if it had said
    /// so.
    fn first_that(
        from: usize,
        mut trying: impl FnMut(usize),
        fine: impl Fn(&Gpu) -> bool,
    ) -> Result<(Gpu, usize), GpuError> {
        let count = Gpu::backends().len().max(1);
        let mut tries = 0;
        loop {
            let backend = (from + tries) % count;
            let Some(instance) = Gpu::instance_after(backend) else {
                return Err(GpuError::NoAdapter("no backend".to_owned()));
            };
            let last = tries + 1 >= count;
            trying(backend);
            let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Gpu::create(instance, None).map(|gpu| {
                    let taken = last || fine(&gpu);
                    (gpu, taken)
                })
            }))
            .unwrap_or_else(|panic| Err(GpuError::Device(format!("panicked: {}", panic_message(panic.as_ref())))));
            match made {
                Ok((gpu, true)) => return Ok((gpu, backend)),
                Ok(_) => log::warn!("trying the next backend"),
                Err(e) if !last => log::warn!("{e}; trying the next backend"),
                Err(e) => return Err(e),
            }
            tries += 1;
        }
    }

    /// A device that can present to `surface` (made from `instance`).
    pub fn for_surface(instance: wgpu::Instance, surface: &wgpu::Surface<'_>) -> Result<Gpu, GpuError> {
        Gpu::create(instance, Some(surface))
    }

    fn create(instance: wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<Gpu, GpuError> {
        // every GPU the backend sees, for the log (a laptop's two, a driver's own name and version)
        for adapter in pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all())) {
            let info = adapter.get_info();
            log::info!(
                "GPU seen: {} ({:?}, {:?}, driver {} {})",
                info.name,
                info.backend,
                info.device_type,
                info.driver,
                info.driver_info
            );
        }
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: surface,
            apply_limit_buckets: false,
        }))
        .map_err(|e| GpuError::NoAdapter(e.to_string()))?;
        let info = adapter.get_info();
        log::info!("GPU: {} ({:?}, driver {})", info.name, info.backend, info.driver_info);
        if keeps_textures_uncompressed(&info.name, info.backend) {
            UNCOMPRESSED.store(true, Ordering::Relaxed);
            log::info!("the canvas's textures kept uncompressed on this GPU");
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("procraft"),
            // where the GPU has it, for strokes that build up only to their transfer
            required_features: adapter.features() & wgpu::Features::DUAL_SOURCE_BLENDING,
            // the adapter's own limits, so large canvases fit where the GPU allows them
            required_limits: adapter.limits(),
            // memory taken in small blocks (a few MB up to 64): a block of each kind, once made,
            // is kept for good, and the larger ones held what was never used (a first read back
            // of a few bytes kept 64 MB the CPU can see)
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .map_err(|e| GpuError::Device(e.to_string()))?;
        // new canvases and crops kept to what its textures can be
        procraft_engine::document::set_gpu_texture_most(device.limits().max_texture_dimension_2d);
        log::info!(
            "GPU capped strokes (dual-source blending): {}",
            device.features().contains(wgpu::Features::DUAL_SOURCE_BLENDING)
        );
        // a validation error must never take the app down: log it instead
        let errors = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&errors);
        device.on_uncaptured_error(Arc::new(move |e| {
            counter.fetch_add(1, Ordering::Relaxed);
            log::error!("GPU error: {e}");
        }));
        // a device lost keeps nothing it held: what its textures had is gone
        let lost = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&lost);
        device.set_device_lost_callback(move |reason, message| {
            flag.store(true, Ordering::Relaxed);
            log::error!("GPU lost ({reason:?}): {message}");
        });
        Ok(Gpu {
            instance,
            adapter,
            device,
            queue,
            errors,
            lost,
            canvas_pipelines: Arc::default(),
            readbacks: Arc::default(),
        })
    }

    /// A buffer of `size` bytes or more to read pixels back through: one kept from an earlier
    /// readback, or a new one. Making one the size of a layer each time took twice the read
    /// itself on a tablet.
    pub(crate) fn readback_buffer(&self, size: u64) -> wgpu::Buffer {
        let mut kept = self.readbacks.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let best =
            kept.iter().enumerate().filter(|(_, b)| b.size() >= size).min_by_key(|(_, b)| b.size()).map(|(i, _)| i);
        if let Some(i) = best {
            return kept.swap_remove(i);
        }
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Lets go of the readback buffers kept for next time (see [`Gpu::readback_buffer`]).
    pub fn let_go_of_kept_buffers(&self) {
        self.readbacks.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
    }

    /// Whether the device has been lost: nothing it held can be read back.
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }

    /// Runs `work`, waits for what it sent the GPU, and says whether any of it went wrong: a
    /// texture or buffer there was no memory for (made all the same, but holding nothing), work
    /// that used one (left undone: a copy into it copies nothing), the device lost meanwhile, or
    /// the work panicking (as wgpu does on a lost device). What `work` made must then not take
    /// the place of what was there. Only what this thread does is looked at.
    pub fn checked<T>(&self, work: impl FnOnce() -> T) -> Result<T, GpuError> {
        if self.is_lost() {
            return Err(GpuError::Lost);
        }
        let memory = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let invalid = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
        // done on the GPU, so a device lost doing it shows
        self.flush();
        let invalid = pollster::block_on(invalid.pop());
        let memory = pollster::block_on(memory.pop());
        if self.is_lost() {
            return Err(GpuError::Lost);
        }
        if let Some(e) = memory {
            return Err(GpuError::OutOfMemory(e.to_string()));
        }
        if let Some(e) = invalid {
            return Err(GpuError::Failed(e.to_string()));
        }
        done.map_err(|panic| GpuError::Failed(format!("panicked: {}", panic_message(panic.as_ref()))))
    }

    /// Makes the pipelines canvases draw with, and the brush images they hold, now, so the first
    /// canvas on this device is made at once (slow: off the main thread).
    pub fn prepare_canvases(&self) {
        let _ = self.canvas_pipelines();
        procraft_engine::textures::prepare();
    }

    /// The pipelines canvases draw with, made by the first canvas on this device.
    pub(crate) fn canvas_pipelines(&self) -> Arc<canvas::Pipelines> {
        Arc::clone(self.canvas_pipelines.get_or_init(|| Arc::new(canvas::Pipelines::new(&self.device))))
    }

    /// GPU validation errors so far (each one is logged); tests expect none.
    pub fn errors(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }

    /// Whether strokes that build up only to their transfer (the uniformed glaze) can be drawn
    /// exactly (with dual-source blending); else their transfer is taken into each stamp.
    pub fn caps_transfer(&self) -> bool {
        self.device.features().contains(wgpu::Features::DUAL_SOURCE_BLENDING)
    }

    pub fn max_texture_size(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    /// The fewest layers a texture viewed as an array has: two on OpenGL (ES), which makes a
    /// texture of one a plain 2D one that an array view reads nothing from; else one.
    pub fn array_layers_least(&self) -> u32 {
        if self.is_gl() { 2 } else { 1 }
    }

    /// Whether the device is OpenGL (ES)'s.
    pub fn is_gl(&self) -> bool {
        self.adapter.get_info().backend == wgpu::Backend::Gl
    }
}

#[cfg(test)]
mod tests {
    /// A GPU or driver too old for the rest still draws: without OpenGL behind them the app can't
    /// open at all.
    #[test]
    #[cfg(any(target_os = "linux", windows, target_os = "android"))]
    fn opengl_is_the_last_backend_tried() {
        if std::env::var_os("WGPU_BACKEND").is_none() {
            assert_eq!(super::Gpu::backends().last(), Some(&wgpu::Backends::GL));
        }
    }

    #[test]
    fn backends_are_named_for_the_log() {
        use wgpu::Backends;
        assert_eq!(super::backends_name(Backends::DX12), "Direct3D 12");
        assert_eq!(super::backends_name(Backends::VULKAN | Backends::GL), "Vulkan or OpenGL");
        assert_eq!(super::Gpu::backend_name(99), "backend 99");
    }

    #[test]
    fn mali_gpus_on_vulkan_keep_their_canvas_textures_uncompressed() {
        use wgpu::Backend;
        assert!(super::keeps_textures_uncompressed("Mali-G68 MC4", Backend::Vulkan));
        assert!(!super::keeps_textures_uncompressed("Mali-G68 MC4", Backend::Gl));
        assert!(!super::keeps_textures_uncompressed("Adreno (TM) 750", Backend::Vulkan));
    }

    use super::*;

    #[test]
    fn work_that_fails_on_the_gpu_is_told() {
        let gpu = match Gpu::headless() {
            Ok(g) => g,
            Err(e) => return eprintln!("skipped: {e}"),
        };
        assert_eq!(gpu.checked(|| 7).ok(), Some(7));
        // a texture it won't make, and a panic on the way
        let most = gpu.device.limits().max_texture_dimension_2d;
        let made = gpu.checked(|| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("too large"),
                size: wgpu::Extent3d { width: most + 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        });
        assert!(matches!(made, Err(GpuError::Failed(_))));
        assert!(matches!(gpu.checked::<()>(|| panic!("a wrapper panicking")), Err(GpuError::Failed(_))));
        // nothing goes past it to the errors counted
        assert_eq!(gpu.errors(), 0);
        // and a device lost
        gpu.device.destroy();
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        assert!(matches!(gpu.checked(|| 7), Err(GpuError::Lost)));
    }

    #[test]
    fn canvases_on_one_device_share_their_pipelines() {
        let gpu = match Gpu::headless() {
            Ok(g) => g,
            Err(e) => return eprintln!("skipped: {e}"),
        };
        let first = gpu.canvas_pipelines();
        // the same once made, and for a clone of the device too
        assert!(Arc::ptr_eq(&first, &gpu.canvas_pipelines()));
        assert!(Arc::ptr_eq(&first, &gpu.clone().canvas_pipelines()));
        // another device makes its own
        if let Ok(other) = Gpu::headless() {
            assert!(!Arc::ptr_eq(&first, &other.canvas_pipelines()));
        }
    }

    #[test]
    fn a_backend_whose_driver_panics_gives_way_to_the_next() {
        let count = Gpu::backend_count();
        let mut tried = Vec::new();
        // every backend but the last panics as its device is checked; the last is kept as it is
        let made = Gpu::first_that(1, |b| tried.push(b), |_| panic!("a driver's wrapper panicking"));
        // tried from the one asked for, round to those before it
        let order: Vec<usize> = (0..count).map(|t| (1 + t) % count).collect();
        assert!(order.starts_with(&tried), "{tried:?} of {order:?}");
        match made {
            Ok((_, backend)) => assert_eq!(Some(&backend), tried.last()),
            Err(e) => eprintln!("skipped: {e}"),
        }
    }
}
