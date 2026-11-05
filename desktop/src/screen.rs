//! The window's surface and the device the canvas and the interface share: each frame the
//! canvas is drawn into the window's image, and the interface over it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use procraft_core::{CanvasSession, configure, screen_profile};
use procraft_gpu::{Gpu, wgpu};
use winit::event_loop::OwnedDisplayHandle;
use winit::window::Window;

pub struct Screen {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: Option<wgpu::SurfaceConfiguration>,
    pub gpu: Gpu,
    pub egui: egui_winit::State,
    /// Made for the surface's format.
    renderer: egui_wgpu::Renderer,
    format: wgpu::TextureFormat,
    /// The brush studio's drawing pad: its own picture, shown by the interface.
    pad: Option<Pad>,
    /// The canvases' pipelines being made on a thread of their own, the window showing meanwhile
    /// (seconds; many more with Direct3D 12's shader compiler): whether they were made without a
    /// GPU error.
    canvases: Option<std::thread::JoinHandle<bool>>,
    /// How many backends have been tried, its own the last of them (with those the app tried
    /// before it started again), and whether that is all there are.
    pub tried: usize,
    pub last: bool,
}

/// A canvas drawn into a picture of its own (the surface's format), its size, and the
/// interface's name for it.
struct Pad {
    view: wgpu::TextureView,
    size: (u32, u32),
    id: egui::TextureId,
}

/// Makes the canvases' pipelines on `gpu` (and the brush images they hold) on a thread of their
/// own, waking `wake` once they are; a canvas made meanwhile waits for them. The thread says
/// whether they were made without a GPU error (a driver that panics on them fails too).
fn make_canvases(gpu: &Gpu, wake: winit::event_loop::EventLoopProxy<()>) -> Option<std::thread::JoinHandle<bool>> {
    let gpu = gpu.clone();
    let made = std::thread::Builder::new().name("procraft-pipelines".to_owned()).spawn(move || {
        let (started, errors) = (Instant::now(), gpu.errors());
        let fine = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| gpu.prepare_canvases())) {
            Ok(()) => gpu.errors() == errors && !gpu.is_lost(),
            Err(panic) => {
                log::error!("the canvases' pipelines panicked: {}", procraft_gpu::panic_message(panic.as_ref()));
                false
            }
        };
        log::info!(
            "the canvases' pipelines made in {} ms: {} GPU errors, lost {}",
            started.elapsed().as_millis(),
            gpu.errors().saturating_sub(errors),
            gpu.is_lost()
        );
        let _ = wake.send_event(());
        fine
    });
    // without a thread, the first canvas makes them
    made.inspect_err(|e| log::warn!("the canvases' pipelines' thread: {e}")).ok()
}

/// Whether the window's surface may be wide colour: no, as egui draws the interface in sRGB onto
/// it as it is (a Mac's screen, which takes Display P3, would show it over-saturated); the canvas
/// is shown in sRGB there, as on screens without wide colour.
const INTERFACE_WIDE: bool = false;

impl Screen {
    /// The window's surface, and a device that can show it, and which backend made them: the
    /// first from backend `from` on (round to those before it) that makes both and can show on
    /// the window without an error (see `Gpu::instance_after`); one whose driver panics gives way
    /// to the next too. `trying` is told each backend before it is tried (a driver that crashes
    /// takes the app down with it). Else what each backend said. The canvases' pipelines are then
    /// made on a thread of their own (see [`Screen::canvases_made`]), `wake` woken once they are.
    /// `tried` backends were tried before the app started again: only those left are.
    pub fn new(
        window: Arc<Window>,
        display: &OwnedDisplayHandle,
        from: usize,
        tried: usize,
        mut trying: impl FnMut(usize),
        wake: winit::event_loop::EventLoopProxy<()>,
    ) -> Result<(Screen, usize), String> {
        let count = Gpu::backend_count().max(1);
        let left = count.saturating_sub(tried).max(1);
        let mut failed = Vec::new();
        for tries in 0..left {
            let backend = (from + tries) % count;
            let last = tries + 1 >= left;
            trying(backend);
            let started = Instant::now();
            let made =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Screen::on(&window, display, backend, last)))
                    .unwrap_or_else(|panic| Err(format!("panicked: {}", procraft_gpu::panic_message(panic.as_ref()))));
            let name = Gpu::backend_name(backend);
            match made {
                Ok(mut screen) => {
                    log::info!("graphics backend {backend} ({name}) ready in {} ms", started.elapsed().as_millis());
                    screen.canvases = make_canvases(&screen.gpu, wake);
                    screen.tried = count - left + tries + 1;
                    return Ok((screen, backend));
                }
                Err(e) => {
                    log::warn!("graphics backend {backend} ({name}): {e}");
                    failed.push(format!("{name}: {e}"));
                }
            }
        }
        Err(failed.join("\n"))
    }

    /// [`Screen::new`] on backend `backend`; one that isn't the `last` to try fails on any GPU
    /// error, a lost device or a surface it can't show.
    fn on(window: &Arc<Window>, display: &OwnedDisplayHandle, backend: usize, last: bool) -> Result<Screen, String> {
        let window = Arc::clone(window);
        let instance =
            Gpu::instance_for_display(backend, Box::new(display.clone())).ok_or("no such graphics backend")?;
        let surface = instance.create_surface(Arc::clone(&window)).map_err(|e| format!("the window's surface: {e}"))?;
        let gpu = Gpu::for_surface(instance, &surface).map_err(|e| e.to_string())?;
        procraft_core::keep_device(&gpu);
        let size = window.inner_size();
        let config = configure(&gpu, &surface, (size.width, size.height), INTERFACE_WIDE);
        if !last && config.is_none() {
            return Err("the window's surface couldn't be set up".to_owned());
        }
        let format = config.as_ref().map_or(wgpu::TextureFormat::Bgra8Unorm, |c| c.format);
        let renderer = egui_wgpu::Renderer::new(&gpu.device, format, egui_wgpu::RendererOptions::default());
        if !last && (gpu.errors() > 0 || gpu.is_lost()) {
            return Err(format!("the interface's pipelines: {} GPU errors, lost {}", gpu.errors(), gpu.is_lost()));
        }
        let ctx = egui::Context::default();
        // Ctrl (⌘) with + - 0 zoom the canvas (the interface's shortcuts), not the whole interface
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let egui = egui_winit::State::new(
            ctx,
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            window.theme(),
            usize::try_from(gpu.max_texture_size()).ok(),
        );
        Ok(Screen { window, surface, config, gpu, egui, renderer, format, pad: None, canvases: None, tried: 0, last })
    }

    /// Whether the canvases' pipelines were made without a GPU error, once they are made (told
    /// once).
    pub fn canvases_made(&mut self) -> Option<bool> {
        if !self.canvases.as_ref().is_some_and(std::thread::JoinHandle::is_finished) {
            return None;
        }
        Some(self.canvases.take()?.join().unwrap_or(false))
    }

    /// The window is `width` × `height` pixels now.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            // minimised: nothing is shown until it comes back
            self.config = None;
            return;
        }
        self.config = configure(&self.gpu, &self.surface, (width, height), INTERFACE_WIDE);
        if let Some(format) = self.config.as_ref().map(|c| c.format).filter(|f| *f != self.format) {
            self.format = format;
            self.renderer = egui_wgpu::Renderer::new(&self.gpu.device, format, egui_wgpu::RendererOptions::default());
        }
    }

    /// Whether `p` (window pixels) is on the interface rather than the canvas, which shows in
    /// `canvas` (points).
    pub fn over_interface(&self, p: (f32, f32), canvas: egui::Rect) -> bool {
        let ctx = self.egui.egui_ctx();
        let at = egui::pos2(p.0, p.1) / ctx.pixels_per_point();
        !canvas.contains(at) || ctx.layer_id_at(at).is_some_and(|l| l.order != egui::Order::Background)
    }

    /// Draws `pad` (the brush studio's drawing pad) into its own picture, `size` pixels (made again
    /// at another size); returns the interface's name for the picture, to show it with.
    pub fn draw_pad(&mut self, pad: &CanvasSession, size: (u32, u32), started: Instant) -> egui::TextureId {
        let size = (size.0.max(1), size.1.max(1));
        let config = self.config.as_ref();
        let screen = config.map_or(procraft_engine::profile::Profile::Srgb, screen_profile);
        if self.pad.as_ref().is_none_or(|p| p.size != size) {
            let device = &self.gpu.device;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("drawing pad"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            // the interface shows the picture's values as they are, as the canvas wrote them
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let shown = &view;
            let id = match &self.pad {
                Some(p) => {
                    self.renderer.update_egui_texture_from_wgpu_texture(device, shown, wgpu::FilterMode::Linear, p.id);
                    p.id
                }
                None => self.renderer.register_native_texture(device, shown, wgpu::FilterMode::Linear),
            };
            pad.show_in(size.0, size.1);
            self.pad = Some(Pad { view, size, id });
        }
        let Some(p) = &self.pad else { return egui::TextureId::default() };
        pad.draw(&p.view, self.format, size, screen, started);
        p.id
    }

    /// The pad's picture goes (the studio closed).
    pub fn drop_pad(&mut self) {
        if let Some(p) = self.pad.take() {
            self.renderer.free_texture(&p.id);
        }
    }

    /// Draws the canvas (if an artwork is open) and the interface over it, and shows them. Returns
    /// how soon another frame is wanted, if one is (the interface can ask for one later, as a hold
    /// waits).
    pub fn draw(
        &mut self,
        session: Option<&CanvasSession>,
        started: Instant,
        output: egui::FullOutput,
    ) -> Option<Duration> {
        let egui::FullOutput { platform_output, mut textures_delta, shapes, pixels_per_point, viewport_output } =
            output;
        // a pen read by the app itself (on Wayland) has its cursor set apart from the mouse's
        crate::pen::set_cursor(platform_output.cursor_icon);
        self.egui.handle_platform_output(&self.window, platform_output);
        let interface_after = viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay)
            .filter(|d| *d < Duration::from_secs(60 * 60));
        // textures change even when nothing is shown, so none is missed (the device keeps one
        // let go of until what drew with it is done)
        let (device, queue) = (&self.gpu.device, &self.gpu.queue);
        for (id, deltas) in std::mem::take(&mut textures_delta.set) {
            for delta in &deltas {
                self.renderer.update_texture(device, queue, id, delta);
            }
        }
        for id in std::mem::take(&mut textures_delta.free) {
            self.renderer.free_texture(&id);
        }
        let config = self.config.as_ref()?;
        let size = (config.width, config.height);
        let (format, screen) = (config.format, screen_profile(config));
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.resize(size.0, size.1);
                return Some(Duration::ZERO);
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return None,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let canvas_soon = session.is_some_and(|s| s.draw(&view, format, size, screen, started));

        let (device, queue) = (&self.gpu.device, &self.gpu.queue);
        let jobs = self.egui.egui_ctx().tessellate(shapes, pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [size.0, size.1], pixels_per_point };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("interface") });
        let uploads = self.renderer.update_buffers(device, queue, &mut encoder, &jobs, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("interface"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    // over the canvas just drawn (the gallery covers the whole window)
                    ops: wgpu::Operations {
                        load: if session.is_some() {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
        }
        queue.submit(uploads.into_iter().chain([encoder.finish()]));
        self.window.pre_present_notify();
        queue.present(frame);
        // the backend shows: a crash from now on isn't its start's
        crate::startup::gpu_shown();
        if canvas_soon { Some(Duration::ZERO) } else { interface_after }
    }
}
