//! Project canvas, as the Android app's (canvas/Projection.kt): with it on and a second display
//! attached, that display shows the whole canvas fitted and centred on black, drawn again after
//! each frame of the main window, and black while the gallery shows.

use std::sync::Arc;

use procraft_core::{CanvasSession, configure, screen_profile};
use procraft_gpu::{Gpu, wgpu};
use winit::event_loop::ActiveEventLoop;
use winit::monitor::MonitorHandle;
use winit::window::{Fullscreen, Window};

pub struct Projection {
    pub window: Arc<Window>,
    /// The display it fills.
    pub monitor: MonitorHandle,
    surface: wgpu::Surface<'static>,
    config: Option<wgpu::SurfaceConfiguration>,
    /// What it last showed: the canvas's frame, or none for black; none yet, drawn again.
    drawn: Option<Option<u64>>,
}

impl Projection {
    /// A borderless window filling `monitor`, its surface on `gpu`'s device.
    pub fn open(event_loop: &ActiveEventLoop, monitor: MonitorHandle, gpu: &Gpu) -> Option<Projection> {
        let attributes = Window::default_attributes()
            .with_title("ProCraft")
            .with_decorations(false)
            .with_fullscreen(Some(Fullscreen::Borderless(Some(monitor.clone()))))
            // the keyboard stays with the main window
            .with_active(false);
        let window = Arc::new(event_loop.create_window(attributes).ok()?);
        let surface = gpu.instance.create_surface(Arc::clone(&window)).ok()?;
        let size = window.inner_size();
        // only the canvas is shown there: in wide colour where the display takes it
        let config = configure(gpu, &surface, (size.width, size.height), true);
        Some(Projection { window, monitor, surface, config, drawn: None })
    }

    /// The window is `width` × `height` pixels now.
    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        self.config = if width > 0 && height > 0 { configure(gpu, &self.surface, (width, height), true) } else { None };
        self.drawn = None;
    }

    /// Shows the canvas of `session` (black with none) if that changed since it was last shown.
    pub fn draw(&mut self, gpu: &Gpu, session: Option<&CanvasSession>) {
        let Some(config) = self.config.as_ref() else { return };
        let showing = session.map(CanvasSession::frames_drawn);
        if self.drawn == Some(showing) {
            return;
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                let size = (config.width, config.height);
                self.resize(gpu, size.0, size.1);
                return;
            }
            _ => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let size = (config.width, config.height);
        match session {
            Some(session) => session.draw_projection(&view, config.format, size, screen_profile(config)),
            None => {
                let mut encoder =
                    gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("projection") });
                drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("projection"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                }));
                gpu.queue.submit([encoder.finish()]);
            }
        }
        self.window.pre_present_notify();
        gpu.queue.present(frame);
        self.drawn = Some(showing);
    }
}

/// The display a projection goes on: one the main `window` isn't on, if there is one.
pub fn second_display(event_loop: &ActiveEventLoop, window: &Window) -> Option<MonitorHandle> {
    let main = window.current_monitor();
    event_loop.available_monitors().find(|m| Some(m) != main.as_ref())
}
