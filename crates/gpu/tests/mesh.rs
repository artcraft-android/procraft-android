//! 3D painting's model view: drawn with depth, textured and lit.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use procraft_engine::mesh::read_obj;
use procraft_gpu::{Gpu, MeshView, Orbit, read_texture, wgpu};
use wgpu::util::DeviceExt;

fn gpu() -> Option<Gpu> {
    Gpu::headless().ok()
}

const SIDE: u32 = 64;

/// Draws `view` into a `SIDE` square and reads it back (RGBA8).
fn draw(gpu: &Gpu, view: &mut MeshView) -> Vec<u8> {
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
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    // the clear colour round the model, not the environment
    view.show_environment(gpu, false);
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("test") });
    view.render(gpu, &mut encoder, &target_view, SIDE, SIDE, [0.0, 0.0, 0.0, 1.0]);
    gpu.queue.submit([encoder.finish()]);
    read_texture(gpu, &target, SIDE, SIDE).unwrap()
}

fn pixel(p: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * SIDE + x) * 4) as usize;
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

/// Two squares facing +z, the near one (red) given first, the far one (blue) after it.
const QUADS: &str = "v -1 -1 0.5\nv 1 -1 0.5\nv 1 1 0.5\nv -1 1 0.5\nv -2 -2 -0.5\nv 2 -2 -0.5\nv 2 2 -0.5\nv -2 2 -0.5\n\
vt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nvn 0 0 1\n\
usemtl near\nf 1/1/1 2/2/1 3/3/1 4/4/1\nusemtl far\nf 5/1/1 6/2/1 7/3/1 8/4/1\n";

#[test]
fn the_nearer_surface_hides_the_farther() {
    let Some(gpu) = gpu() else { return };
    let mesh = read_obj(QUADS, None).unwrap();
    let mut view = MeshView::new(&gpu, &mesh, 4, wgpu::TextureFormat::Rgba8Unorm);
    view.set_base(&gpu, 0, [1.0, 0.0, 0.0]);
    view.set_base(&gpu, 1, [0.0, 0.0, 1.0]);
    // straight on from +z
    view.orbit = Orbit { target: [0.0, 0.0, 0.0], rotation: Orbit::rotation_of(0.0, 0.0), distance: 6.0, fov: 0.9 };
    let px = draw(&gpu, &mut view);
    let middle = pixel(&px, SIDE / 2, SIDE / 2);
    assert!(middle[0] > 100 && middle[2] < middle[0] / 3, "the near square in front: {middle:?}");
    // past the near square's edge the far one shows
    let rim = pixel(&px, SIDE / 2 + 13, SIDE / 2);
    assert!(rim[2] > 100 && rim[0] < rim[2] / 3, "the far square around it: {rim:?}");
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn a_painted_texture_shows_over_the_base_colour() {
    let Some(gpu) = gpu() else { return };
    let mesh = read_obj(QUADS, None).unwrap();
    let mut view = MeshView::new(&gpu, &mesh, 4, wgpu::TextureFormat::Rgba8Unorm);
    view.orbit = Orbit { target: [0.0, 0.0, 0.0], rotation: Orbit::rotation_of(0.0, 0.0), distance: 6.0, fov: 0.9 };
    // the near square painted green all over (premultiplied, opaque)
    let green: Vec<u8> = (0..16).flat_map(|_| [0, 255, 0, 255]).collect();
    assert!(view.set_albedo(&gpu, 0, &green));
    assert!(!view.set_albedo(&gpu, 0, &green[..8]));
    let px = draw(&gpu, &mut view);
    let middle = pixel(&px, SIDE / 2, SIDE / 2);
    assert!(middle[1] > 100 && middle[0] < 60 && middle[2] < 60, "{middle:?}");
    // unpainted, the far square shows its base colour (white), lit
    let rim = pixel(&px, SIDE / 2 + 13, SIDE / 2);
    assert!(rim[0] > 100 && rim[0] == rim[1] && rim[1] == rim[2], "{rim:?}");
    // round the model, the clear colour
    assert_eq!(pixel(&px, 0, 0), [0, 0, 0, 255]);
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn a_framed_model_fills_the_view_with_room_round_it() {
    let Some(gpu) = gpu() else { return };
    let mesh = read_obj(QUADS, None).unwrap();
    let mut view = MeshView::new(&gpu, &mesh, 4, wgpu::TextureFormat::Rgba8Unorm);
    view.orbit = Orbit { rotation: Orbit::rotation_of(0.0, 0.0), ..view.orbit };
    let px = draw(&gpu, &mut view);
    let lit = (0..SIDE * SIDE).filter(|&k| pixel(&px, k % SIDE, k / SIDE)[0] > 20).count();
    // the far square seen whole, about half the view across, not to its edges
    assert!(lit > (SIDE * SIDE / 5) as usize && lit < (SIDE * SIDE / 3) as usize, "{lit}");
    assert_eq!(pixel(&px, 0, SIDE / 2), [0, 0, 0, 255]);
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn a_stroke_on_the_screen_paints_the_surface_it_lands_on() {
    use procraft_engine::document::Document;
    use procraft_engine::geom::{IRect, Vec2};
    use procraft_engine::stroke::Stamp;
    use procraft_gpu::{Canvas, StrokeStyle, StrokeTool};
    let Some(gpu) = gpu() else { return };
    let mesh = read_obj(QUADS, None).unwrap();
    let mut view = MeshView::new(&gpu, &mesh, 16, wgpu::TextureFormat::Rgba8Unorm);
    view.orbit = Orbit { target: [0.0, 0.0, 0.0], rotation: Orbit::rotation_of(0.0, 0.0), distance: 6.0, fov: 0.9 };
    // a dot in the middle of the screen
    let mut screen_doc = Document::new(SIDE, SIDE);
    let mut screen = Canvas::new(&gpu, &screen_doc).unwrap();
    let style = StrokeStyle::new(StrokeTool::Paint, [1.0, 0.0, 0.0], 1.0);
    screen.begin_stroke(screen_doc.current(), style);
    let middle = SIDE as f32 / 2.0;
    screen.add_stamps(&[Stamp::round(Vec2::new(middle, middle), 6.0, 1.0)]);
    screen.render(&gpu, &mut screen_doc, None);
    let bounds = IRect::around(Vec2::new(middle, middle), 7.0);
    // onto each square's texture (16 square)
    let mut painted = Vec::new();
    let mut canvases = Vec::new();
    for part in 0..2 {
        let mut doc = Document::new(16, 16);
        doc.background_visible = false;
        let mut texture = Canvas::new(&gpu, &doc).unwrap();
        texture.begin_drawn_stroke(doc.current(), style);
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("project") });
        let reach = view.project_stroke(
            &gpu,
            &mut encoder,
            part,
            screen.stroke_view(),
            bounds,
            SIDE,
            SIDE,
            texture.stroke_target(),
        );
        gpu.queue.submit([encoder.finish()]);
        texture.stroke_drawn(reach);
        texture.end_stroke();
        texture.render(&gpu, &mut doc, None);
        painted.push((reach, texture.read_layer(&gpu, doc.current()).unwrap()));
        canvases.push((texture, doc));
    }
    let alpha = |px: &[u8], x: u32, y: u32| px[((y * 16 + x) * 4 + 3) as usize];
    // the near square: painted in its middle, not at its corners
    let (reach, near) = &painted[0];
    assert!(!reach.is_empty());
    assert!(alpha(near, 8, 8) > 200, "{}", alpha(near, 8, 8));
    assert_eq!(alpha(near, 1, 1), 0);
    // the far square is hidden there: nothing reaches it
    let (_, far) = &painted[1];
    assert!(far.chunks(4).all(|p| p[3] == 0), "the hidden square was painted");
    // the model shows the near square's canvas: red where painted, its base colour round it
    let (near_canvas, _) = &canvases[0];
    view.set_base(&gpu, 0, [0.0, 1.0, 0.0]);
    view.set_albedo_view(&gpu, 0, near_canvas.composite_view());
    let px = draw(&gpu, &mut view);
    let middle_px = pixel(&px, SIDE / 2, SIDE / 2);
    assert!(middle_px[0] > 100 && middle_px[1] < 60, "{middle_px:?}");
    let off = pixel(&px, SIDE / 2 + 9, SIDE / 2);
    assert!(off[1] > 100 && off[0] < 60, "{off:?}");
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn colours_go_into_the_targets_space_but_pictures_keep_their_own() {
    let Some(gpu) = gpu() else { return };
    let mesh = read_obj(QUADS, None).unwrap();
    let mut view = MeshView::new(&gpu, &mesh, 4, wgpu::TextureFormat::Rgba8Unorm);
    view.set_base(&gpu, 0, [1.0, 0.0, 0.0]);
    view.orbit = Orbit { target: [0.0, 0.0, 0.0], rotation: Orbit::rotation_of(0.0, 0.0), distance: 6.0, fov: 0.9 };
    // a "space" whose red is the target's green
    view.set_convert(Some([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]));
    let shown = pixel(&draw(&gpu, &mut view), SIDE / 2, SIDE / 2);
    assert!(shown[1] > 100 && shown[0] < 10, "{shown:?}");
    let picture = view.picture(&gpu, SIDE, SIDE, [0.0, 0.0, 0.0, 1.0]).unwrap();
    let kept = pixel(&picture, SIDE / 2, SIDE / 2);
    assert!(kept[0] > 100 && kept[1] < kept[0] / 3, "{kept:?}");
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn a_projected_stroke_grows_a_little_past_its_island() {
    use procraft_engine::document::Document;
    use procraft_engine::geom::{IRect, Vec2};
    use procraft_engine::stroke::Stamp;
    use procraft_gpu::{Canvas, StrokeStyle, StrokeTool};
    let Some(gpu) = gpu() else { return };
    // one square facing the camera, its texture's middle half its island (texels 4 to 11)
    let island = "v -1 -1 0\nv 1 -1 0\nv 1 1 0\nv -1 1 0\nvt 0.25 0.25\nvt 0.75 0.25\nvt 0.75 0.75\nvt 0.25 0.75\nf 1/1 2/2 3/3 4/4\n";
    let mesh = read_obj(island, None).unwrap();
    let mut view = MeshView::new(&gpu, &mesh, 16, wgpu::TextureFormat::Rgba8Unorm);
    view.orbit = Orbit { target: [0.0, 0.0, 0.0], rotation: Orbit::rotation_of(0.0, 0.0), distance: 6.0, fov: 0.9 };
    // a stroke over the whole square
    let mut screen_doc = Document::new(SIDE, SIDE);
    let mut screen = Canvas::new(&gpu, &screen_doc).unwrap();
    let style = StrokeStyle::new(StrokeTool::Paint, [1.0, 0.0, 0.0], 1.0);
    screen.begin_stroke(screen_doc.current(), style);
    let middle = SIDE as f32 / 2.0;
    screen.add_stamps(&[Stamp::round(Vec2::new(middle, middle), 30.0, 1.0)]);
    screen.render(&gpu, &mut screen_doc, None);
    let mut doc = Document::new(16, 16);
    doc.background_visible = false;
    let mut texture = Canvas::new(&gpu, &doc).unwrap();
    texture.begin_drawn_stroke(doc.current(), style);
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("project") });
    let bounds = IRect::from_size(SIDE, SIDE);
    let reach =
        view.project_stroke(&gpu, &mut encoder, 0, screen.stroke_view(), bounds, SIDE, SIDE, texture.stroke_target());
    gpu.queue.submit([encoder.finish()]);
    texture.stroke_drawn(reach);
    texture.end_stroke();
    texture.render(&gpu, &mut doc, None);
    let px = texture.read_layer(&gpu, doc.current()).unwrap();
    let alpha = |x: u32| px[((8 * 16 + x) * 4 + 3) as usize];
    // inside, and up to two texels outside, painted; further out, not
    for x in [2, 3, 4, 8, 11, 12, 13] {
        assert!(alpha(x) > 200, "texel {x}: {}", alpha(x));
    }
    for x in [0, 1, 14, 15] {
        assert_eq!(alpha(x), 0, "texel {x}");
    }
    assert!(reach.x0 <= 2 && reach.x1 >= 14, "{reach:?}");
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn a_far_surface_shows_its_texture_filtered() {
    let Some(gpu) = gpu() else { return };
    let mesh = read_obj(QUADS, None).unwrap();
    let mut view = MeshView::new(&gpu, &mesh, 64, wgpu::TextureFormat::Rgba8Unorm);
    // a black and white checker of single texels
    let checker: Vec<u8> =
        (0..64 * 64).flat_map(|i| if (i % 64 + i / 64) % 2 == 0 { [255; 4] } else { [0, 0, 0, 255] }).collect();
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("checker"),
        size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &checker,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(64 * 4), rows_per_image: Some(64) },
        wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
    );
    let albedo = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("levels") });
    view.set_albedo_filtered(&gpu, &mut encoder, 0, &albedo);
    gpu.queue.submit([encoder.finish()]);
    // far off: the near square a few pixels across, each pixel over many texels
    view.orbit = Orbit { target: [0.0, 0.0, 0.0], rotation: Orbit::rotation_of(0.0, 0.0), distance: 40.0, fov: 0.9 };
    let px = draw(&gpu, &mut view);
    let middle = (SIDE / 2 - 1..SIDE / 2 + 2).flat_map(|y| (SIDE / 2 - 1..SIDE / 2 + 2).map(move |x| (x, y)));
    for (x, y) in middle {
        let p = pixel(&px, x, y);
        // grey, as lit: no texel black or white showing through
        assert!(p[0] > 40 && p[0] < 200 && p[0] == p[1], "({x}, {y}): {p:?}");
    }
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn painting_through_reaches_hidden_surfaces() {
    use procraft_engine::document::Document;
    use procraft_engine::geom::{IRect, Vec2};
    use procraft_engine::stroke::Stamp;
    use procraft_gpu::{Canvas, StrokeStyle, StrokeTool};
    let Some(gpu) = gpu() else { return };
    let mesh = read_obj(QUADS, None).unwrap();
    let mut view = MeshView::new(&gpu, &mesh, 16, wgpu::TextureFormat::Rgba8Unorm);
    view.orbit = Orbit { target: [0.0, 0.0, 0.0], rotation: Orbit::rotation_of(0.0, 0.0), distance: 6.0, fov: 0.9 };
    view.paint_through = true;
    let mut screen_doc = Document::new(SIDE, SIDE);
    let mut screen = Canvas::new(&gpu, &screen_doc).unwrap();
    let style = StrokeStyle::new(StrokeTool::Paint, [1.0, 0.0, 0.0], 1.0);
    screen.begin_stroke(screen_doc.current(), style);
    let middle = SIDE as f32 / 2.0;
    screen.add_stamps(&[Stamp::round(Vec2::new(middle, middle), 6.0, 1.0)]);
    screen.render(&gpu, &mut screen_doc, None);
    // onto the far square, behind the near one
    let mut doc = Document::new(16, 16);
    doc.background_visible = false;
    let mut texture = Canvas::new(&gpu, &doc).unwrap();
    texture.begin_drawn_stroke(doc.current(), style);
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("through") });
    let bounds = IRect::around(Vec2::new(middle, middle), 7.0);
    let reach =
        view.project_stroke(&gpu, &mut encoder, 1, screen.stroke_view(), bounds, SIDE, SIDE, texture.stroke_target());
    gpu.queue.submit([encoder.finish()]);
    texture.stroke_drawn(reach);
    texture.end_stroke();
    texture.render(&gpu, &mut doc, None);
    let px = texture.read_layer(&gpu, doc.current()).unwrap();
    assert!(px[((8 * 16 + 8) * 4 + 3) as usize] > 200, "the hidden square took no paint");
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn surfaces_turned_away_take_less_paint() {
    use procraft_engine::document::Document;
    use procraft_engine::geom::{IRect, Vec2};
    use procraft_engine::stroke::Stamp;
    use procraft_gpu::{Canvas, StrokeStyle, StrokeTool};
    let Some(gpu) = gpu() else { return };
    let square = "v -1 -1 0\nv 1 -1 0\nv 1 1 0\nv -1 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nf 1/1 2/2 3/3 4/4\n";
    let mesh = read_obj(square, None).unwrap();
    // the middle texel's paint, seen from `yaw` round
    let paint_from = |yaw: f32| {
        let mut view = MeshView::new(&gpu, &mesh, 16, wgpu::TextureFormat::Rgba8Unorm);
        view.orbit = Orbit { target: [0.0, 0.0, 0.0], rotation: Orbit::rotation_of(yaw, 0.0), distance: 6.0, fov: 0.9 };
        let mut screen_doc = Document::new(SIDE, SIDE);
        let mut screen = Canvas::new(&gpu, &screen_doc).unwrap();
        let style = StrokeStyle::new(StrokeTool::Paint, [1.0, 0.0, 0.0], 1.0);
        screen.begin_stroke(screen_doc.current(), style);
        screen.add_stamps(&[Stamp::round(Vec2::new(SIDE as f32 / 2.0, SIDE as f32 / 2.0), 30.0, 1.0)]);
        screen.render(&gpu, &mut screen_doc, None);
        let mut doc = Document::new(16, 16);
        doc.background_visible = false;
        let mut texture = Canvas::new(&gpu, &doc).unwrap();
        texture.begin_drawn_stroke(doc.current(), style);
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("graze") });
        let reach = view.project_stroke(
            &gpu,
            &mut encoder,
            0,
            screen.stroke_view(),
            IRect::from_size(SIDE, SIDE),
            SIDE,
            SIDE,
            texture.stroke_target(),
        );
        gpu.queue.submit([encoder.finish()]);
        texture.stroke_drawn(reach);
        texture.end_stroke();
        texture.render(&gpu, &mut doc, None);
        texture.read_layer(&gpu, doc.current()).unwrap()[((8 * 16 + 8) * 4 + 3) as usize]
    };
    // square on: all of it; nearly edge on (cos 0.04): next to nothing
    assert!(paint_from(0.3) > 250);
    assert!(paint_from(1.53) < 30, "{}", paint_from(1.53));
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn a_mirror_reflects_what_is_behind_the_camera() {
    use procraft_engine::environment::Equirect;
    use procraft_gpu::Lighting;
    let Some(gpu) = gpu() else { return };
    let square = "v -1 -1 0\nv 1 -1 0\nv 1 1 0\nv -1 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nf 1/1 2/2 3/3 4/4\n";
    let mesh = read_obj(square, None).unwrap();
    // a panorama red behind the camera (its left and right edges, along +z) and green ahead
    let (w, h) = (64u32, 32u32);
    let pixels = (0..w * h)
        .map(|i| {
            let u = (i % w) as f32 / w as f32;
            if !(0.25..0.75).contains(&u) { [4.0, 0.0, 0.0] } else { [0.0, 4.0, 0.0] }
        })
        .collect();
    let panorama = Equirect { width: w, height: h, pixels };
    let mut view = MeshView::new(&gpu, &mesh, 4, wgpu::TextureFormat::Rgba8Unorm);
    view.set_lighting(&gpu, Lighting::new(&gpu, &panorama));
    // a smooth metal, white: a mirror
    view.set_material(&gpu, 0, 0.0, 1.0);
    view.orbit = Orbit { target: [0.0, 0.0, 0.0], rotation: Orbit::rotation_of(0.0, 0.0), distance: 6.0, fov: 0.9 };
    let px = draw(&gpu, &mut view);
    let middle = pixel(&px, SIDE / 2, SIDE / 2);
    assert!(middle[0] > 150 && middle[1] < 60, "{middle:?}");
    // seen from behind (turned half round), it shows what was ahead
    view.orbit.rotation = Orbit::rotation_of(std::f32::consts::PI, 0.0);
    let px = draw(&gpu, &mut view);
    let back = pixel(&px, SIDE / 2, SIDE / 2);
    assert!(back[0] > 150 && back[1] < 60, "the environment turns with the camera: {back:?}");
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn without_its_environment_the_model_stands_on_the_surround_grid() {
    let Some(gpu) = gpu() else { return };
    let mesh = read_obj(QUADS, None).unwrap();
    let mut view = MeshView::new(&gpu, &mesh, 4, wgpu::TextureFormat::Rgba8Unorm);
    view.orbit = Orbit { target: [0.0, 0.0, 0.0], rotation: Orbit::rotation_of(0.0, 0.0), distance: 6.0, fov: 0.9 };
    // lines 2 pixels wide every 8, from 4 down
    view.set_grid(Some(procraft_gpu::SurroundGrid {
        spacing: 8.0,
        origin: [0.0, 4.0],
        top: 4.0,
        width: 2.0,
        color: [0.5, 0.5, 0.5],
    }));
    let px = draw(&gpu, &mut view);
    // at the corner, a line's pixel and the clear colour beside it; nothing over the grid's top
    assert!(pixel(&px, 0, 12)[0] > 100, "{:?}", pixel(&px, 0, 12));
    assert_eq!(pixel(&px, 3, 14)[0], 0);
    assert_eq!(pixel(&px, 0, 2)[0], 0);
    // the model is drawn over it
    let middle = pixel(&px, SIDE / 2, SIDE / 2);
    assert!(middle[0] > 150 && middle[1] > 150, "{middle:?}");
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn the_studio_shows_each_light_as_a_cube_and_moves_it_across_the_screen() {
    let Some(gpu) = gpu() else { return };
    let mesh = read_obj(QUADS, None).unwrap();
    let mut view = MeshView::new(&gpu, &mesh, 4, wgpu::TextureFormat::Rgba8Unorm);
    view.lights = vec![procraft_gpu::Light { place: [-2.0, 2.0, 1.0], color: [1.0, 0.0, 0.0], intensity: 0.5 }];
    let orbit = view.studio_orbit();
    let [x, y, reach] = view.light_on_screen(&orbit, 0, SIDE, SIDE).unwrap();
    assert!(x > 0.0 && x < SIDE as f32 / 2.0 && y > 0.0 && y < SIDE as f32 / 2.0, "up and left: {x} {y}");
    assert!(reach > 0.5, "{reach}");
    let draw_studio = |view: &mut MeshView| {
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("studio target"),
            size: wgpu::Extent3d { width: SIDE, height: SIDE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        view.show_environment(&gpu, false);
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("test") });
        view.render_studio(&gpu, &mut encoder, &target_view, SIDE, SIDE, [0.0, 0.0, 0.0, 1.0], orbit);
        gpu.queue.submit([encoder.finish()]);
        read_texture(&gpu, &target, SIDE, SIDE).unwrap()
    };
    let px = draw_studio(&mut view);
    let cube = pixel(&px, x as u32, y as u32);
    assert!(cube[0] > 150 && cube[1] < cube[0] / 2, "a red cube: {cube:?}");
    // the floor's lines show somewhere below the model
    let floor = (SIDE * 3 / 4..SIDE)
        .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(&px, x, y)[1] > 60)
        .count();
    assert!(floor > 10, "{floor} floor pixels");
    // dragged 10 pixels right and 6 down, it follows
    view.move_light(&orbit, 0, 10.0, 6.0, SIDE, SIDE);
    let [x2, y2, _] = view.light_on_screen(&orbit, 0, SIDE, SIDE).unwrap();
    assert!((x2 - x - 10.0).abs() < 0.5 && (y2 - y - 6.0).abs() < 0.5, "{x} {y} -> {x2} {y2}");
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn a_hidden_submesh_is_not_drawn_and_each_has_its_own_picture() {
    let Some(gpu) = gpu() else { return };
    let mesh = read_obj(&format!("o Front\n{QUADS}").replace("usemtl far", "o Back\nusemtl far"), None).unwrap();
    assert_eq!(mesh.submeshes.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["Front", "Back"]);
    let mut view = MeshView::new(&gpu, &mesh, 4, wgpu::TextureFormat::Rgba8Unorm);
    view.set_base(&gpu, 0, [1.0, 0.0, 0.0]);
    view.set_base(&gpu, 1, [0.0, 0.0, 1.0]);
    view.orbit = Orbit { target: [0.0, 0.0, 0.0], rotation: Orbit::rotation_of(0.0, 0.0), distance: 6.0, fov: 0.9 };
    // the near square hidden, the far one shows through where it was
    view.set_hidden(0, true);
    let px = draw(&gpu, &mut view);
    let middle = pixel(&px, SIDE / 2, SIDE / 2);
    assert!(middle[2] > 100 && middle[0] < middle[2] / 3, "{middle:?}");
    view.set_hidden(0, false);
    // the near square alone fills its picture, on nothing at the corner
    let picture = view.submesh_picture(&gpu, 0, SIDE).unwrap();
    let centre = pixel(&picture, SIDE / 2, SIDE / 2);
    assert!(centre[0] > 100 && centre[3] == 255, "{centre:?}");
    assert_eq!(pixel(&picture, 0, 0)[3], 0);
    // and the view is as it was
    let again = draw(&gpu, &mut view);
    assert!(pixel(&again, SIDE / 2, SIDE / 2)[0] > 100);
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn a_metalness_painted_over_the_material_changes_how_it_shades() {
    let Some(gpu) = gpu() else { return };
    let mesh = read_obj(QUADS, None).unwrap();
    let mut view = MeshView::new(&gpu, &mesh, 4, wgpu::TextureFormat::Rgba8Unorm);
    view.set_base(&gpu, 0, [0.8, 0.2, 0.2]);
    view.orbit = Orbit { target: [0.0, 0.0, 0.0], rotation: Orbit::rotation_of(0.0, 0.0), distance: 6.0, fov: 0.9 };
    let plain = draw(&gpu, &mut view);
    // fully metal: the red base reflects instead of scattering
    let white = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("metal"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &[255, 255, 255, 255],
    );
    let map = white.create_view(&wgpu::TextureViewDescriptor::default());
    view.set_channel_view(&gpu, 0, 2, Some(&map));
    let metal = draw(&gpu, &mut view);
    let (a, b) = (pixel(&plain, SIDE / 2, SIDE / 2), pixel(&metal, SIDE / 2, SIDE / 2));
    assert_ne!(a, b);
    // and back to the material's own
    view.set_channel_view(&gpu, 0, 2, None);
    assert_eq!(pixel(&draw(&gpu, &mut view), SIDE / 2, SIDE / 2), a);
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn a_models_own_normal_and_occlusion_maps_shade_it() {
    use procraft_gpu::SurfaceMap;
    let Some(gpu) = gpu() else { return };
    let square = "v -1 -1 0\nv 1 -1 0\nv 1 1 0\nv -1 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nf 1/1 2/2 3/3 4/4\n";
    let mesh = read_obj(square, None).unwrap();
    let mut view = MeshView::new(&gpu, &mesh, 4, wgpu::TextureFormat::Rgba8Unorm);
    view.orbit = Orbit { target: [0.0, 0.0, 0.0], rotation: Orbit::rotation_of(0.0, 0.0), distance: 6.0, fov: 0.9 };
    let middle = |view: &mut MeshView| pixel(&draw(&gpu, view), SIDE / 2, SIDE / 2);
    let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(&b).all(|(x, y)| x.abs_diff(*y) <= 3);
    let plain = middle(&mut view);
    let filled = |rgba: [u8; 4]| rgba.repeat(16);
    // a flat normal map changes nothing; one leaning along u does
    view.set_surface_map(&gpu, 0, SurfaceMap::Normal, Some((4, &filled([128, 128, 255, 255]))));
    let flat = middle(&mut view);
    assert!(near(flat, plain), "{flat:?} {plain:?}");
    view.set_surface_map(&gpu, 0, SurfaceMap::Normal, Some((4, &filled([240, 128, 160, 255]))));
    let leaning = middle(&mut view);
    assert!(!near(leaning, plain), "{leaning:?} {plain:?}");
    view.set_surface_map(&gpu, 0, SurfaceMap::Normal, None);
    assert!(near(middle(&mut view), plain));
    // full occlusion keeps the environment's light off; none leaves it as it was
    view.set_surface_map(&gpu, 0, SurfaceMap::Occlusion, Some((4, &filled([0, 0, 0, 255]))));
    let shut = middle(&mut view);
    assert!(shut[0] + 10 < plain[0], "{shut:?} {plain:?}");
    view.set_surface_map(&gpu, 0, SurfaceMap::Occlusion, Some((4, &filled([255, 255, 255, 255]))));
    assert!(near(middle(&mut view), plain));
    // a map of the wrong size is left out
    view.set_surface_map(&gpu, 0, SurfaceMap::Occlusion, Some((4, &[0; 8])));
    assert!(near(middle(&mut view), plain));
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn a_normal_map_leans_the_surface_along_its_texture_coordinates() {
    use procraft_gpu::{Light, SurfaceMap};
    let Some(gpu) = gpu() else { return };
    // a square facing the camera, u running to its right (+x) and v up (+y)
    let square = "v -1 -1 0
v 1 -1 0
v 1 1 0
v -1 1 0
vt 0 0
vt 1 0
vt 1 1
vt 0 1
f 1/1 2/2 3/3 4/4
";
    let mesh = read_obj(square, None).unwrap();
    let mut view = MeshView::new(&gpu, &mesh, 4, wgpu::TextureFormat::Rgba8Unorm);
    view.set_material(&gpu, 0, 1.0, 0.0);
    view.orbit = Orbit { target: [0.0, 0.0, 0.0], rotation: Orbit::rotation_of(0.0, 0.0), distance: 6.0, fov: 0.9 };
    let mut lit = |place: [f32; 3], rgb: [u8; 3]| {
        view.lights = vec![Light { place, color: [1.0; 3], intensity: 0.8 }];
        let map = [rgb[0], rgb[1], rgb[2], 255].repeat(16);
        view.set_surface_map(&gpu, 0, SurfaceMap::Normal, Some((4, &map)));
        i32::from(pixel(&draw(&gpu, &mut view), SIDE / 2, SIDE / 2)[0])
    };
    // lit from the right (+x): leaning along +u turns it toward the light, along -u away
    let (toward, away) = (lit([6.0, 0.0, 1.0], [218, 128, 218]), lit([6.0, 0.0, 1.0], [38, 128, 218]));
    assert!(toward > away + 20, "lit from +x: {toward} leaning +u, {away} leaning -u");
    // lit from above (+y): the same along v
    let (toward, away) = (lit([0.0, 6.0, 1.0], [128, 218, 218]), lit([0.0, 6.0, 1.0], [128, 38, 218]));
    assert!(toward > away + 20, "lit from +y: {toward} leaning +v, {away} leaning -v");
    assert_eq!(gpu.errors(), 0);
}
