//! The eyedropper: what is shown around a window point, and the canvas colour under it.

use procraft_engine::geom::{Affine, Vec2};
use procraft_gpu::{Target, read_texture, wgpu};

use crate::CanvasSession;

/// What the eyedropper sees.
#[derive(Clone, Debug, uniffi::Record)]
pub struct Pick {
    /// The colour under the point (0xRRGGBB): the canvas's, or the current layer's when a plugin
    /// says to read that (none where it is clear); none when the point is off the canvas.
    pub rgb: Option<u32>,
    /// The window around the point as it is shown, `side` × `side` RGBA8, rows top to bottom.
    pub side: u32,
    pub pixels: Vec<u8>,
}

#[uniffi::export]
impl CanvasSession {
    /// The `side` × `side` window pixels centred on window point (`x`, `y`) as they are shown
    /// (canvas through the view, surround and grid), and the colour under the point. None before
    /// the canvas is shown.
    pub fn pick(&self, x: f32, y: f32, side: u32) -> Option<Pick> {
        let side = side.clamp(1, 1024);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.scene_shown() {
            return st.scene_pick(x, y, side);
        }
        let view = st.view?;
        // the window shifted so that the point lands in the middle of the small target
        let corner = Vec2::new(x - side as f32 / 2.0, y - side as f32 / 2.0);
        let grid = st.surround_grid(Vec2::ZERO - corner);
        let surround = st.surround();
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return None };
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("eyedropper"),
            size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = texture.create_view(&Default::default());
        let to_screen = view.to_screen().then(Affine::translate(Vec2::ZERO - corner));
        canvas.render(
            gpu,
            &mut st.doc,
            Some(Target {
                view: &target_view,
                format,
                size: (side, side),
                to_screen,
                surround,
                grid: Some(grid),
                selection: None,
                backdrop: None,
                convert: None,
                lut: None,
                crisp_from: st.display_scale,
                grey: false,
            }),
        );
        let pixels = read_texture(gpu, &texture, side, side).ok()?;
        // the colour is the canvas's own pixel there as composited, not as shown: where it is
        // clear with the background hidden, that is what the composite holds, not the surround
        let (w, h) = (st.doc.width() as f32, st.doc.height() as f32);
        let at = view.screen_to_canvas(Vec2::new(x, y)).filter(|p| p.x >= 0.0 && p.y >= 0.0 && p.x < w && p.y < h);
        let rgb = if st.hooks.as_ref().is_some_and(|hooks| hooks.pick_layer()) {
            // the current layer's own pixel (kept premultiplied): its colour, where it isn't clear
            at.and_then(|p| {
                let (px, py) = (p.x as i32, p.y as i32);
                let rect = procraft_engine::geom::IRect::new(px, py, px + 1, py + 1);
                let pixel = canvas.read_layer_part(gpu, st.doc.current(), rect).ok()?;
                let [r, g, b, a] = [pixel.first()?, pixel.get(1)?, pixel.get(2)?, pixel.get(3)?].map(|c| u32::from(*c));
                let un = |c: u32| ((c * 255 + a / 2) / a.max(1)).min(255);
                (a > 0).then(|| un(r) << 16 | un(g) << 8 | un(b))
            })
        } else {
            at.and_then(|p| canvas.read_composite_pixel(gpu, p.x as u32, p.y as u32))
                .map(|[r, g, b, _]| u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
        };
        Some(Pick { rgb, side, pixels })
    }
}
