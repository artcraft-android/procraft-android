//! Drawing a frame into a window's image: by the render thread on Android and iPadOS, and by the
//! desktop app's own loop; and setting up the window's surface for it.

use std::time::Instant;

use procraft_engine::geom::Vec2;
use procraft_engine::profile::Profile;
use procraft_gpu::{Backdrop, Gpu, SelectionLook, Stripes, Target, wgpu};

use crate::selection::SelectionDisplay;
use crate::{
    BACKDROP_SIGMA_PT, SELECTION_FILL, SELECTION_TINT, STRIPE_COVERAGE, STRIPE_PERIOD_PT, STRIPE_SPEED_PT, State,
};

impl State {
    /// Draws the whole canvas fitted and centred on black into `view` (`format`, `size` pixels,
    /// its colours shown in `screen`): Project canvas, on a second display.
    pub(crate) fn draw_fitted(
        &mut self,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        screen: Profile,
    ) {
        let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) else { return };
        let to_screen = procraft_engine::view::View::fit((self.doc.width(), self.doc.height()), size).to_screen();
        let convert = colours_to(self.doc.profile, screen);
        let key = crate::colour::table_key(&self.doc, screen);
        if self.display_lut.as_ref().map(|(k, _)| *k) != key {
            self.display_lut = key
                .zip(crate::colour::display_table(&self.doc, screen))
                .map(|(k, table)| (k, canvas.display_lut(gpu, &table)));
        }
        let lut = self.display_lut.as_ref().map(|(_, l)| l);
        canvas.render(
            gpu,
            &mut self.doc,
            Some(Target {
                view,
                format,
                size,
                to_screen,
                surround: [0.0; 3],
                grid: None,
                selection: None,
                backdrop: None,
                convert,
                lut,
                // the second display's own density isn't known: crisp once a pixel covers more
                crisp_from: 1.0,
                grey: false,
            }),
        );
    }

    /// How the selection looks, `started` being when drawing began.
    fn selection_shown(&self, started: Instant, screen: Profile) -> SelectionLook {
        let s = self.display_scale;
        match self.selection_display {
            SelectionDisplay::Stripes => {
                let period = STRIPE_PERIOD_PT * s;
                let drift = (started.elapsed().as_secs_f32() * STRIPE_SPEED_PT * s) % period;
                SelectionLook::Stripes(Stripes { period, drift, coverage: STRIPE_COVERAGE * self.selection_visibility })
            }
            SelectionDisplay::Tint => SelectionLook::Tint(Profile::Srgb.convert(SELECTION_TINT, screen)),
            SelectionDisplay::Fill => SelectionLook::Fill(Profile::Srgb.convert(SELECTION_FILL, screen)),
        }
    }

    /// Draws a frame into `view` (`format`, `size` window pixels, its colours shown in `screen`):
    /// the model open to paint on, the drawing pad as a sphere, or the canvas with its surround,
    /// grid, selection and frosted panels. `started` is when drawing began, for the bands' drift.
    /// Leaves `needs_frame` set while another frame is wanted.
    pub(crate) fn draw_frame(
        &mut self,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        screen: Profile,
        started: Instant,
    ) {
        self.needs_frame = false;
        // the view settling into place keeps drawing until it is there
        if self.step_view_motion() {
            self.needs_frame = true;
        }
        let to_screen = self.view.map(|v| v.to_screen());
        let grid = self.surround_grid(Vec2::ZERO);
        let surround = self.surround();
        let selection_shown = self.selection_shown(started, screen);
        // a model is drawn in place of the flat canvas, and the drawing pad may be a sphere
        if self.scene_shown() {
            let convert = colours_to(self.doc.profile, screen);
            let selected = self.selection.is_some();
            let look = selected.then_some(selection_shown);
            // the bands drift, so they keep drawing
            if matches!(look, Some(SelectionLook::Stripes(_))) {
                self.needs_frame = true;
            }
            self.scene_selection_look(look);
            self.render_scene(view, format, size, convert);
            self.frames += 1;
        } else if self.sphere.is_some() {
            let convert = colours_to(self.doc.profile, screen);
            self.render_sphere(view, format, size, convert);
            self.frames += 1;
        } else if let (Some(canvas), Some(gpu), Some(to_screen)) = (self.canvas.as_mut(), self.gpu.as_ref(), to_screen)
        {
            let s = self.display_scale;
            // the bands over what is not selected drift, so they keep drawing
            let shown = selection_shown;
            let look = canvas.has_selection().then_some(shown);
            let animating = matches!(look, Some(SelectionLook::Stripes(_)));
            let convert = colours_to(self.doc.profile, screen);
            // a canvas with a profile of its own shows through a table, made when that changes
            let key = crate::colour::table_key(&self.doc, screen);
            if self.display_lut.as_ref().map(|(k, _)| *k) != key {
                self.display_lut = key
                    .zip(crate::colour::display_table(&self.doc, screen))
                    .map(|(k, table)| (k, canvas.display_lut(gpu, &table)));
            }
            let lut = self.display_lut.as_ref().map(|(_, l)| l);
            let backdrop = (!self.backdrops.is_empty() || !self.backdrop_windows.is_empty()).then(|| Backdrop {
                regions: &self.backdrops,
                windows: &self.backdrop_windows,
                sigma: BACKDROP_SIGMA_PT * s,
            });
            canvas.render(
                gpu,
                &mut self.doc,
                Some(Target {
                    view,
                    format,
                    size,
                    to_screen,
                    surround,
                    grid: Some(grid),
                    selection: look,
                    backdrop,
                    convert,
                    lut,
                    crisp_from: self.display_scale,
                    grey: self.grey_view,
                }),
            );
            // caches still being remade: another frame soon, and the artwork isn't shown yet
            let busy = canvas.busy();
            if animating || canvas.wants_render() {
                self.needs_frame = true;
            }
            // counted only when the canvas was drawn, so a first frame means the artwork shows
            if !busy {
                self.frames += 1;
            }
        }
        self.heed_filter_run();
    }
}

/// The profile a configured surface shows colours in.
pub fn screen_profile(config: &wgpu::SurfaceConfiguration) -> Profile {
    if config.color_space == wgpu::SurfaceColorSpace::DisplayP3 { Profile::DisplayP3 } else { Profile::Srgb }
}

/// The matrix moving a canvas's colours in `from` to `to`, if they differ.
pub(crate) fn colours_to(from: Profile, to: Profile) -> Option<[[f32; 3]; 3]> {
    (from.is_p3() != to.is_p3()).then(|| from.matrix_to(to))
}

/// Configures the surface for `size`: a format that doesn't re-encode sRGB, the lowest-latency
/// present mode on offer, one frame in flight, and (if `wide`) Display P3 where the screen takes
/// it. A surface an sRGB interface is drawn on as it is (egui's) asks for no wide colour, or the
/// interface would show over-saturated. None if it can't be (yet).
pub fn configure(
    gpu: &Gpu,
    surface: &wgpu::Surface<'_>,
    size: (u32, u32),
    wide: bool,
) -> Option<wgpu::SurfaceConfiguration> {
    let caps = surface.get_capabilities(&gpu.adapter);
    // an 8-bit one first: a float surface (Rgba16Float, offered first on some Linux drivers) is
    // read by the compositor as linear, so the sRGB-encoded colours drawn on it show washed out
    let eight_bit = [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8Unorm];
    let format = eight_bit
        .into_iter()
        .find(|f| caps.formats.contains(f))
        .or_else(|| caps.formats.iter().copied().find(|f| !f.is_srgb()))
        .or_else(|| caps.formats.first().copied())?;

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
    let p3 = wide
        && caps
            .format_capabilities
            .iter()
            .any(|f| f.format == format && f.color_spaces.contains(wgpu::SurfaceColorSpaces::DISPLAY_P3));
    if p3 {
        config.color_space = wgpu::SurfaceColorSpace::DisplayP3;
    }
    config.desired_maximum_frame_latency = 1;
    // a configure the GPU refuses (it couldn't come idle in time, as while an artwork opens) leaves
    // the surface unconfigured: none, so the next frame tries again (else it stayed blank)
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    surface.configure(&gpu.device, &config);
    if let Some(e) = pollster::block_on(scope.pop()) {
        log::warn!("the surface couldn't be configured yet: {e}");
        return None;
    }
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
