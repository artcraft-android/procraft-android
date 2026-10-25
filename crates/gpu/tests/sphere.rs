//! The brush studio's 3D preview: a lit sphere showing the canvas under it.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use procraft_engine::document::Document;
use procraft_gpu::{Canvas, Gpu, SphereLook, SpherePreview, read_texture, wgpu};

const SIDE: u32 = 64;

/// The preview of a `SIDE` square canvas whose background is `background` (or none, nothing
/// painted), the sphere filling the target, with the brush's metalness `metallic`.
fn draw(gpu: &Gpu, background: Option<[f32; 3]>, metallic: f32) -> Vec<u8> {
    let mut doc = Document::new(SIDE, SIDE);
    doc.background_visible = background.is_some();
    doc.background = background.unwrap_or([1.0; 3]);
    let mut canvas = Canvas::new(gpu, &doc).unwrap();
    canvas.render(gpu, &mut doc, None);
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test target"),
        size: wgpu::Extent3d { width: SIDE, height: SIDE, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut preview = SpherePreview::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let look = SphereLook {
        metallic,
        roughness: 0.5,
        metallic_source: None,
        roughness_source: None,
        metallic_tile: 100.0,
        roughness_tile: 100.0,
        behind: [0.0, 0.0, 1.0],
        convert: None,
    };
    let half = SIDE as f32 / 2.0;
    preview.render(gpu, &view, [half, half], half, &canvas, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], &look);
    read_texture(gpu, &target, SIDE, SIDE).unwrap()
}

fn pixel(p: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * SIDE + x) * 4) as usize;
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

#[test]
fn the_sphere_shows_its_own_grey_and_the_pad_round_it() {
    let Some(gpu) = Gpu::headless().ok() else { return };
    let px = draw(&gpu, None, 0.0);
    // round the sphere, the pad's colour
    assert_eq!(pixel(&px, 0, 0), [0, 0, 255, 255]);
    // its middle a lit grey, brighter toward its light high on the right, darker and warmer below
    let middle = pixel(&px, SIDE / 2, SIDE / 2);
    assert!(middle[0] > 50 && middle[0] < 90 && middle[0].abs_diff(middle[1]) < 6, "{middle:?}");
    let (lit, below) = (pixel(&px, 45, 18), pixel(&px, 32, 54));
    assert!(lit[0] > middle[0] + 30, "{lit:?}");
    assert!(below[0] < middle[0] && below[0] > below[2], "{below:?}");
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn paint_shows_in_its_colour_and_metal_reflects_the_light() {
    let Some(gpu) = Gpu::headless().ok() else { return };
    let plain = draw(&gpu, None, 0.0);
    let red = draw(&gpu, Some([1.0, 0.0, 0.0]), 0.0);
    let middle = pixel(&red, SIDE / 2, SIDE / 2);
    assert!(middle[0] > middle[1] + 60 && middle[0] > middle[2] + 60, "{middle:?}");
    // the same red, metallic: its own colour now only in what it reflects
    let metal = draw(&gpu, Some([1.0, 0.0, 0.0]), 1.0);
    assert_ne!(pixel(&metal, SIDE / 2, SIDE / 2), middle);
    assert_ne!(pixel(&plain, SIDE / 2, SIDE / 2), middle);
    assert_eq!(gpu.errors(), 0);
}
