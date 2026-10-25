//! The render thread: owns the window's surface and draws a frame whenever something changed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ndk::native_window::NativeWindow;
use procraft_engine::geom::Vec2;
use procraft_engine::profile::Profile;
use procraft_engine::view::View;
use procraft_gpu::{Backdrop, Gpu, SelectionLook, Stripes, Target, wgpu};

use crate::selection::SelectionDisplay;
use crate::{
    BACKDROP_SIGMA_PT, SELECTION_FILL, SELECTION_TINT, STRIPE_COVERAGE, STRIPE_PERIOD_PT, STRIPE_SPEED_PT, SURROUND,
    Shared,
};

pub(crate) fn run(shared: Arc<Shared>, window: NativeWindow) {
    let own = shared.lock().gpu.as_ref().map(|g| g.instance.clone());
    // the app's device (waited for while it is being made), else one made for this window and
    // kept for every artwork after
    let mut kept = crate::device::kept();
    let instance = own.clone().or_else(|| kept.as_ref().map(|g| g.instance.clone())).unwrap_or_else(Gpu::instance);
    let surface = match crate::window::create_surface(&instance, &window) {
        Ok(s) => s,
        Err(e) => return fail(&shared, e),
    };
    let device = if own.is_some() {
        None
    } else if let Some(gpu) = kept.as_ref() {
        Some(gpu.clone())
    } else {
        match Gpu::for_surface(instance, &surface) {
            Ok(gpu) => {
                *kept = Some(gpu.clone());
                Some(gpu)
            }
            Err(e) => return fail(&shared, e.to_string()),
        }
    };
    drop(kept);
    if let Some(gpu) = device {
        // it holds the session's layers from now on
        shared.lock().gpu.get_or_insert(gpu);
    }
    let queue = {
        let mut st = shared.lock();
        st.ensure_canvas();
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
                st = shared.wake.wait(st).unwrap_or_else(|e| e.into_inner());
            }
            if st.stop {
                break;
            }
            (st.window_size, std::mem::replace(&mut st.reconfigure, false))
        };
        if reconfigure || config.is_none() {
            let st = shared.lock();
            let Some(gpu) = st.gpu.as_ref() else { break };
            config = configure(gpu, &surface, size);
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
            // the window changed size while the image was awaited (the device turned): the view is
            // already for the new size, so this image is let go and the surface configured again
            if st.window_size != size {
                continue;
            }
            st.needs_frame = false;
            // the view settling into place keeps drawing until it is there
            if st.step_view_motion() {
                st.needs_frame = true;
            }
            let to_screen = st.view.map(|v| v.to_screen());
            let grid = st.surround_grid(Vec2::ZERO);
            // a model is drawn in place of the flat canvas
            if st.scene_shown() {
                let convert = colours_to(st.doc.profile, screen);
                st.render_scene(&view, format, size, convert);
                st.frames += 1;
            } else if let (Some(canvas), Some(gpu), Some(to_screen)) = (st.canvas.as_mut(), st.gpu.as_ref(), to_screen)
            {
                let s = st.display_scale;
                // the bands over what is not selected drift, so they keep drawing
                let look = canvas.has_selection().then(|| match st.selection_display {
                    SelectionDisplay::Stripes => {
                        let period = STRIPE_PERIOD_PT * s;
                        let drift = (started.elapsed().as_secs_f32() * STRIPE_SPEED_PT * s) % period;
                        SelectionLook::Stripes(Stripes {
                            period,
                            drift,
                            coverage: STRIPE_COVERAGE * st.selection_visibility,
                        })
                    }
                    SelectionDisplay::Tint => SelectionLook::Tint(Profile::Srgb.convert(SELECTION_TINT, screen)),
                    SelectionDisplay::Fill => SelectionLook::Fill(Profile::Srgb.convert(SELECTION_FILL, screen)),
                });
                let animating = matches!(look, Some(SelectionLook::Stripes(_)));
                let convert = colours_to(st.doc.profile, screen);
                // a canvas with a profile of its own shows through a table, made when that changes
                let key = crate::colour::table_key(&st.doc, screen);
                if st.display_lut.as_ref().map(|(k, _)| *k) != key {
                    st.display_lut = key
                        .zip(crate::colour::display_table(&st.doc, screen))
                        .map(|(k, table)| (k, canvas.display_lut(gpu, &table)));
                }
                let lut = st.display_lut.as_ref().map(|(_, l)| l);
                let backdrop = (!st.backdrops.is_empty())
                    .then(|| Backdrop { regions: &st.backdrops, sigma: BACKDROP_SIGMA_PT * s });
                canvas.render(
                    gpu,
                    &mut st.doc,
                    Some(Target {
                        view: &view,
                        format,
                        size,
                        to_screen,
                        surround: SURROUND,
                        grid: Some(grid),
                        selection: look,
                        backdrop,
                        convert,
                        lut,
                    }),
                );
                if animating {
                    st.needs_frame = true;
                }
                // counted only when the canvas was drawn, so a first frame means the artwork shows
                st.frames += 1;
            }
            let now = Instant::now();
            if let Some(prev) = last_present {
                st.frame_interval_ms = now.duration_since(prev).as_secs_f32() * 1000.0;
            }
            last_present = Some(now);
        }
        queue.present(frame);
    }
    drop(surface);
    drop(window);
}

/// How often the second display looks for a new frame of the main window.
const PROJECTION_POLL: Duration = Duration::from_millis(8);

/// Project canvas's thread: the whole canvas fitted and centred on black on a second display,
/// drawn again after each frame of the main window.
pub(crate) fn project(shared: Arc<Shared>, window: NativeWindow) {
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
            config = configure(gpu, &surface, size);
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
            let mut guard = shared.lock();
            let st = &mut *guard;
            if st.projection_size != size {
                continue;
            }
            if let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) {
                let to_screen = View::fit((st.doc.width(), st.doc.height()), size).to_screen();
                let screen = config.as_ref().map_or(Profile::Srgb, screen_profile);
                let convert = colours_to(st.doc.profile, screen);
                let key = crate::colour::table_key(&st.doc, screen);
                if st.display_lut.as_ref().map(|(k, _)| *k) != key {
                    st.display_lut = key
                        .zip(crate::colour::display_table(&st.doc, screen))
                        .map(|(k, table)| (k, canvas.display_lut(gpu, &table)));
                }
                let lut = st.display_lut.as_ref().map(|(_, l)| l);
                canvas.render(
                    gpu,
                    &mut st.doc,
                    Some(Target {
                        view: &view,
                        format,
                        size,
                        to_screen,
                        surround: [0.0; 3],
                        grid: None,
                        selection: None,
                        backdrop: None,
                        convert,
                        lut,
                    }),
                );
            }
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

/// Configures the surface for `size`: a format that doesn't re-encode sRGB, the lowest-latency
/// present mode on offer, and one frame in flight.
/// The profile a configured surface shows colours in.
fn screen_profile(config: &wgpu::SurfaceConfiguration) -> Profile {
    if config.color_space == wgpu::SurfaceColorSpace::DisplayP3 { Profile::DisplayP3 } else { Profile::Srgb }
}

/// The matrix moving a canvas's colours in `from` to `to`, if they differ.
fn colours_to(from: Profile, to: Profile) -> Option<[[f32; 3]; 3]> {
    (from.is_p3() != to.is_p3()).then(|| from.matrix_to(to))
}

fn configure(gpu: &Gpu, surface: &wgpu::Surface<'_>, size: (u32, u32)) -> Option<wgpu::SurfaceConfiguration> {
    let caps = surface.get_capabilities(&gpu.adapter);
    let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).or_else(|| caps.formats.first().copied())?;

    let mut config = surface.get_default_config(&gpu.adapter, size.0.max(1), size.1.max(1))?;
    config.format = format;
    config.view_formats = Vec::new();
    config.present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
        wgpu::PresentMode::Mailbox
    } else {
        wgpu::PresentMode::Fifo
    };
    if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
    }
    // wide colour where the screen takes it: the canvas is shown in Display P3
    let p3 = caps
        .format_capabilities
        .iter()
        .any(|f| f.format == format && f.color_spaces.contains(wgpu::SurfaceColorSpaces::DISPLAY_P3));
    if p3 {
        config.color_space = wgpu::SurfaceColorSpace::DisplayP3;
    }
    config.desired_maximum_frame_latency = 1;
    surface.configure(&gpu.device, &config);
    log::info!(
        "surface {}×{} {:?} {:?} {:?}",
        config.width,
        config.height,
        config.format,
        config.present_mode,
        config.color_space
    );
    Some(config)
}
