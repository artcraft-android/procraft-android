//! The window's surface and the device the canvas and the interface share: each frame the
//! canvas is drawn into the window's image, and the interface over it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use procraft_core::{CanvasSession, configure, screen_profile};
use procraft_gpu::{Gpu, wgpu};
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
}

impl Screen {
    /// The window's surface, and a device that can show it.
    pub fn new(window: Arc<Window>) -> Result<Screen, String> {
        let instance = Gpu::instance();
        let surface = instance.create_surface(Arc::clone(&window)).map_err(|e| e.to_string())?;
        let gpu = Gpu::for_surface(instance, &surface).map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let config = configure(&gpu, &surface, (size.width, size.height));
        let format = config.as_ref().map_or(wgpu::TextureFormat::Bgra8Unorm, |c| c.format);
        let renderer = egui_wgpu::Renderer::new(&gpu.device, format, egui_wgpu::RendererOptions::default());
        let ctx = egui::Context::default();
        let egui = egui_winit::State::new(
            ctx,
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            window.theme(),
            usize::try_from(gpu.max_texture_size()).ok(),
        );
        Ok(Screen { window, surface, config, gpu, egui, renderer, format })
    }

    /// The window is `width` × `height` pixels now.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            // minimised: nothing is shown until it comes back
            self.config = None;
            return;
        }
        self.config = configure(&self.gpu, &self.surface, (width, height));
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
        if canvas_soon { Some(Duration::ZERO) } else { interface_after }
    }
}
