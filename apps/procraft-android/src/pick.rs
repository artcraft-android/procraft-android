//! The eyedropper: what is shown around a window point, and the canvas colour under it.

use procraft_engine::geom::{Affine, Vec2};
use procraft_gpu::{Target, read_texture, wgpu};

use crate::CanvasSession;

/// What the eyedropper sees.
#[derive(Clone, Debug, uniffi::Record)]
pub struct Pick {
    /// The canvas colour under the point (0xRRGGBB), or none when the point is off the canvas.
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
            }),
        );
        let pixels = read_texture(gpu, &texture, side, side).ok()?;
        let on_canvas = view
            .screen_to_canvas(Vec2::new(x, y))
            .is_some_and(|p| p.x >= 0.0 && p.y >= 0.0 && p.x < st.doc.width() as f32 && p.y < st.doc.height() as f32);
        let middle = ((side / 2 * side + side / 2) * 4) as usize;
        let rgb = pixels
            .get(middle..middle + 3)
            .filter(|_| on_canvas)
            .map(|c| c.iter().fold(0u32, |acc, v| (acc << 8) | u32::from(*v)));
        Some(Pick { rgb, side, pixels })
    }
}
