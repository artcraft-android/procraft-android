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
    SelectionLook, SharedStroke, SmallScene, Stripes, StrokeStyle, StrokeTool, SurroundGrid, Target, Thumbnail,
    most_resident, read_texture, start_reading, textures_bytes,
};
pub use lighting::Lighting;
pub use scene::{GrainLook, Light, MOST_LIGHTS, MaterialLook, MeshView, Orbit, SurfaceMap, material_source};
pub use sphere::{SphereLook, SpherePreview, preview_environment};
pub use wgpu;

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
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = if cfg!(target_os = "android") { wgpu::Backends::VULKAN } else { wgpu::Backends::PRIMARY };
        wgpu::Instance::new(desc)
    }

    /// Sends the writes queued so far to the device and waits for them, so the copies they were
    /// staged in are let go.
    pub fn flush(&self) {
        self.queue.submit(std::iter::empty());
        if let Err(e) = self.device.poll(wgpu::PollType::wait_indefinitely()) {
            log::error!("flushing uploads: {e}");
        }
    }

    /// A device for offscreen work (tests, exports).
    pub fn headless() -> Result<Gpu, GpuError> {
        Gpu::create(Gpu::instance(), None)
    }

    /// A device that can present to `surface` (made from `instance`).
    pub fn for_surface(instance: wgpu::Instance, surface: &wgpu::Surface<'_>) -> Result<Gpu, GpuError> {
        Gpu::create(instance, Some(surface))
    }

    fn create(instance: wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<Gpu, GpuError> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: surface,
            apply_limit_buckets: false,
        }))
        .map_err(|e| GpuError::NoAdapter(e.to_string()))?;
        let info = adapter.get_info();
        log::info!("GPU: {} ({:?}, driver {})", info.name, info.backend, info.driver_info);
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
