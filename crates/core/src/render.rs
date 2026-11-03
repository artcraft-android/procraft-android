//! The render thread: owns the window's surface and draws a frame whenever something changed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use procraft_engine::profile::Profile;
use procraft_gpu::{Gpu, wgpu};

use crate::Shared;
use crate::frame::{configure, screen_profile};

/// The window drawn into: Android's native window, or the iPad app's `CAMetalLayer`.
#[cfg(target_os = "android")]
type Window = ndk::native_window::NativeWindow;
#[cfg(target_os = "ios")]
type Window = crate::layer::Layer;

pub(crate) fn run(shared: Arc<Shared>, window: Window) {
    let own = shared.lock().gpu.as_ref().map(|g| g.instance.clone());
    // the app's device (waited for while it is being made), else one made for this window and
    // kept for every artwork after
    let mut kept = crate::device::kept();
    let instance = own.clone().or_else(|| kept.as_ref().map(|g| g.instance.clone())).unwrap_or_else(Gpu::instance);
    let mut surface = match crate::window::create_surface(&instance, &window) {
        Ok(s) => s,
        Err(e) => return fail(&shared, e),
    };
    let device = if own.is_some() {
        None
    } else if let Some(gpu) = kept.as_ref() {
        Some(gpu.clone())
    } else {
        // the first instance that makes a device for the window (see `Gpu::instance_after`)
        let mut instance = instance;
        let mut tries = 0;
        loop {
            match Gpu::for_surface(instance.clone(), &surface) {
                Ok(gpu) => {
                    *kept = Some(gpu.clone());
                    break Some(gpu);
                }
                Err(e) => {
                    tries += 1;
                    let Some(next) = Gpu::instance_after(tries) else { return fail(&shared, e.to_string()) };
                    log::warn!("{e}; trying the next backend");
                    surface = match crate::window::create_surface(&next, &window) {
                        Ok(s) => s,
                        Err(e) => return fail(&shared, e),
                    };
                    instance = next;
                }
            }
        }
    };
    drop(kept);
    if let Some(gpu) = device {
        // it holds the session's layers from now on
        shared.lock().gpu.get_or_insert(gpu);
    }
    shared.make_canvas();
    let queue = {
        let mut st = shared.lock();
        st.reconfigure = true;
        st.needs_frame = true;
        st.gpu.as_ref().map(|g| g.queue.clone())
    };
    let Some(queue) = queue else { return };

    let mut config: Option<wgpu::SurfaceConfiguration> = None;
    let mut last_present: Option<Instant> = None;
    let started = Instant::now();
    loop {
        let (size, reconfigure) = {
            let mut st = shared.lock();
            while !st.stop && !st.needs_frame {
                shared.wake.wait(&mut st);
            }
            if st.stop {
                break;
            }
            (st.window_size, std::mem::replace(&mut st.reconfigure, false))
        };
        if reconfigure || config.is_none() {
            let st = shared.lock();
            let Some(gpu) = st.gpu.as_ref() else { break };
            config = configure(gpu, &surface, size, true);
        }
        let Some(format) = config.as_ref().map(|c| c.format) else {
            std::thread::sleep(Duration::from_millis(16));
            continue;
        };
        let screen = config.as_ref().map_or(Profile::Srgb, screen_profile);
        // wait for the next image before looking at the input, so the frame shows the latest
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                shared.lock().reconfigure = true;
                f
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                shared.lock().reconfigure = true;
                std::thread::sleep(Duration::from_millis(4));
                continue;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                std::thread::sleep(Duration::from_millis(4));
                continue;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut guard = shared.lock();
            let st = &mut *guard;
            let held = Instant::now();
            // the window changed size while the image was awaited (the device turned): the view is
            // already for the new size, so this image is let go and the surface configured again
            if st.window_size != size {
                continue;
            }
            st.draw_frame(&view, format, size, screen, started);
            let now = Instant::now();
            if let Some(prev) = last_present {
                st.frame_interval_ms = now.duration_since(prev).as_secs_f32() * 1000.0;
            }
            last_present = Some(now);
            // everything else waits on the lock meanwhile
            if held.elapsed() > SLOW_FRAME {
                log::warn!("a frame held the canvas {} ms", held.elapsed().as_millis());
            }
            // whoever waited on the lock meanwhile (a touch) has it next, before this thread's
            // next frame
            parking_lot::MutexGuard::unlock_fair(guard);
        }
        queue.present(frame);
    }
    drop(surface);
    drop(window);
}

/// A frame that holds the canvas longer than this is logged: touches wait on it.
const SLOW_FRAME: Duration = Duration::from_millis(200);

/// How often the second display looks for a new frame of the main window.
const PROJECTION_POLL: Duration = Duration::from_millis(8);

/// Project canvas's thread: the whole canvas fitted and centred on black on a second display,
/// drawn again after each frame of the main window.
pub(crate) fn project(shared: Arc<Shared>, window: Window) {
    // the device comes with the main window
    let instance = loop {
        {
            let st = shared.lock();
            if st.projection_stop {
                return;
            }
            if let Some(gpu) = st.gpu.as_ref() {
                break gpu.instance.clone();
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let surface = match crate::window::create_surface(&instance, &window) {
        Ok(s) => s,
        Err(e) => return fail(&shared, e),
    };
    let Some(queue) = shared.lock().gpu.as_ref().map(|g| g.queue.clone()) else { return };
    let mut config: Option<wgpu::SurfaceConfiguration> = None;
    let mut size = (0, 0);
    let mut drawn: Option<u64> = None;
    loop {
        let (wanted, frames) = {
            let st = shared.lock();
            if st.projection_stop {
                break;
            }
            (st.projection_size, st.frames)
        };
        if wanted != size || config.is_none() {
            size = wanted;
            let st = shared.lock();
            let Some(gpu) = st.gpu.as_ref() else { break };
            config = configure(gpu, &surface, size, true);
            drawn = None;
        }
        let Some(format) = config.as_ref().map(|c| c.format) else {
            std::thread::sleep(Duration::from_millis(16));
            continue;
        };
        if drawn == Some(frames) {
            std::thread::sleep(PROJECTION_POLL);
            continue;
        }
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                config = None;
                std::thread::sleep(Duration::from_millis(4));
                continue;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                std::thread::sleep(Duration::from_millis(4));
                continue;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut st = shared.lock();
            if st.projection_size != size {
                continue;
            }
            let screen = config.as_ref().map_or(Profile::Srgb, screen_profile);
            st.draw_fitted(&view, format, size, screen);
        }
        drawn = Some(frames);
        queue.present(frame);
    }
    // the picture goes with the canvas: the display is left black
    if config.is_some()
        && let wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) =
            surface.get_current_texture()
    {
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        if let Some(gpu) = shared.lock().gpu.as_ref() {
            let mut encoder =
                gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("projection clear") });
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("projection clear"),
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
        queue.present(frame);
    }
    drop(surface);
    drop(window);
}

fn fail(shared: &Shared, reason: String) {
    log::error!("render thread: {reason}");
    shared.lock().error = Some(reason);
}
