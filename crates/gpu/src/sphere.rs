//! The brush studio's 3D preview: a sphere on the drawing pad showing the pad's canvas where the
//! screen sees it (so a stroke lands under the pen), painted over the sphere's own plain material
//! with the brush's metalness and roughness, lit by a light of its own as a model is lit.

use std::f32::consts::PI;

use procraft_engine::environment::Equirect;
use procraft_engine::textures::TEXTURE_SIZE;

use crate::lighting::{CUBE_LEVELS, Lighting};
use crate::{Canvas, Gpu};

/// The sphere's own colour (as stored) and roughness, under what is painted.
const BASE: [f32; 3] = [0.4327, 0.4234, 0.4144];
const BASE_ROUGHNESS: f32 = 0.4836;

/// The light the sphere is lit by: a sky bluest above, palest at the horizon and dark and warm
/// below, each blended to the horizon's colour by the height to a power, and a small bright key
/// light high to the right (directions in the camera's space, y up, z toward the eye). Matched to
/// how the reference's sphere is lit.
const SKY_TOP: [f32; 3] = [0.0, 0.0967, 0.2962];
const SKY_HORIZON: [f32; 3] = [0.2846, 0.3455, 0.4076];
const SKY_BOTTOM: [f32; 3] = [0.0749, 0.0261, 0.0167];
const SKY_TOP_POWER: f32 = 1.5915;
const SKY_BOTTOM_POWER: f32 = 0.1;
/// The key light's direction (its turn about the vertical from the eye, its height, radians),
/// colour and spread (the light falls off as exp((cos - 1) / spread²)).
const KEY_TURN: f32 = 1.1702;
const KEY_HEIGHT: f32 = 0.7204;
const KEY: [f32; 3] = [47.329, 48.8426, 50.0];
const KEY_SPREAD: f32 = 0.0788;
/// The panorama's size the light is made at (the size it was matched at).
const SKY_WIDTH: u32 = 64;

/// What the preview shows besides the canvas.
pub struct SphereLook<'a> {
    /// The brush's metalness and roughness (0..1).
    pub metallic: f32,
    pub roughness: f32,
    /// Each through its source, if it has one: a key that changes with it and the image
    /// (`TEXTURE_SIZE` square grey, white = the amount), tiled every so many canvas pixels.
    pub metallic_source: Option<(u64, &'a [u8])>,
    pub roughness_source: Option<(u64, &'a [u8])>,
    pub metallic_tile: f32,
    pub roughness_tile: f32,
    /// Around the sphere (as stored, 0..1).
    pub behind: [f32; 3],
    /// The textures' colours to the target's (rows of a matrix in linear light), if they differ.
    pub convert: Option<[[f32; 3]; 3]>,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SphereUniform {
    sh: [[f32; 4]; 9],
    circle: [f32; 4],
    to_canvas_x: [f32; 4],
    to_canvas_y: [f32; 4],
    canvas: [f32; 4],
    base: [f32; 4],
    brush: [f32; 4],
    behind: [f32; 4],
    c0: [f32; 4],
    c1: [f32; 4],
    c2: [f32; 4],
}

/// A source as a texture, with the key it was made from.
struct SourceTexture {
    key: u64,
    view: wgpu::TextureView,
}

/// The preview's pipeline, its light and its sources' textures.
pub struct SpherePreview {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    lighting: Lighting,
    cube_sampler: wgpu::Sampler,
    table_sampler: wgpu::Sampler,
    painted_sampler: wgpu::Sampler,
    source_sampler: wgpu::Sampler,
    /// The metalness and roughness sources, once given.
    sources: [Option<SourceTexture>; 2],
    /// White: a source that is the amount all over.
    white: wgpu::TextureView,
}

/// The sphere's light as a panorama (see [`SKY_TOP`]).
pub fn preview_environment() -> Equirect {
    let (w, h) = (SKY_WIDTH, SKY_WIDTH / 2);
    let key = [KEY_HEIGHT.cos() * KEY_TURN.sin(), KEY_HEIGHT.sin(), KEY_HEIGHT.cos() * KEY_TURN.cos()];
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        let theta = (y as f32 + 0.5) / h as f32 * PI;
        for x in 0..w {
            let phi = ((x as f32 + 0.5) / w as f32 - 0.5) * 2.0 * PI;
            let d = [theta.sin() * phi.sin(), theta.cos(), -theta.sin() * phi.cos()];
            let up = d[1];
            let (far, t) =
                if up >= 0.0 { (SKY_TOP, up.powf(SKY_TOP_POWER)) } else { (SKY_BOTTOM, (-up).powf(SKY_BOTTOM_POWER)) };
            let along = d[0] * key[0] + d[1] * key[1] + d[2] * key[2];
            let blob = ((along - 1.0) / (KEY_SPREAD * KEY_SPREAD)).exp();
            let mut c = SKY_HORIZON;
            for ((out, far), key) in c.iter_mut().zip(far).zip(KEY) {
                *out = (*out + (far - *out) * t).max(0.0) + key * blob;
            }
            pixels.push(c);
        }
    }
    Equirect { width: w, height: h, pixels }
}

impl SpherePreview {
    /// The preview, drawing into targets of `format`.
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> SpherePreview {
        let device = &gpu.device;
        let texture = |binding, view_dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension,
                multisampled: false,
            },
            count: None,
        };
        let sampler = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sphere preview"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(1, wgpu::TextureViewDimension::Cube),
                sampler(2),
                texture(3, wgpu::TextureViewDimension::D2),
                sampler(4),
                texture(5, wgpu::TextureViewDimension::D2),
                sampler(6),
                texture(7, wgpu::TextureViewDimension::D2),
                texture(8, wgpu::TextureViewDimension::D2),
                sampler(9),
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sphere preview"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/sphere.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sphere preview"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sphere preview"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_full"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sphere preview"),
            size: std::mem::size_of::<SphereUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let linear = |label, mipmap, address_mode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: mipmap,
                address_mode_u: address_mode,
                address_mode_v: address_mode,
                ..Default::default()
            })
        };
        let clamp = wgpu::AddressMode::ClampToEdge;
        SpherePreview {
            pipeline,
            layout,
            uniform,
            lighting: Lighting::new(gpu, &preview_environment()),
            cube_sampler: linear("sphere radiance", wgpu::MipmapFilterMode::Linear, clamp),
            table_sampler: linear("sphere brdf table", wgpu::MipmapFilterMode::Nearest, clamp),
            painted_sampler: linear("sphere canvas", wgpu::MipmapFilterMode::Nearest, clamp),
            source_sampler: linear("sphere sources", wgpu::MipmapFilterMode::Nearest, wgpu::AddressMode::Repeat),
            sources: [None, None],
            white: grey_texture(gpu, 1, &[255]),
        }
    }

    /// Draws the sphere into `target` (`size`): its middle at `middle`, `radius` across (target
    /// pixels), showing `canvas` (its composite as last rendered) through `to_canvas` (target
    /// pixels to canvas pixels: a, b, c, d, e, f of x' = a x + c y + e, y' = b x + d y + f).
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        middle: [f32; 2],
        radius: f32,
        canvas: &Canvas,
        to_canvas: [f32; 6],
        look: &SphereLook<'_>,
    ) {
        for (slot, source) in self.sources.iter_mut().zip([look.metallic_source, look.roughness_source]) {
            match source {
                Some((key, pixels))
                    if slot.as_ref().is_none_or(|s| s.key != key) && pixels.len() == TEXTURE_SIZE * TEXTURE_SIZE =>
                {
                    *slot = Some(SourceTexture { key, view: grey_texture(gpu, TEXTURE_SIZE as u32, pixels) });
                }
                None => *slot = None,
                _ => {}
            }
        }
        let (w, h) = canvas.size();
        let [a, b, c, d, e, f] = to_canvas;
        let [r0, r1, r2] = look.convert.unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        let applied = if look.convert.is_some() { 1.0 } else { 0.0 };
        let has = |s: &Option<SourceTexture>| if s.is_some() { 1.0 } else { 0.0 };
        let [metallic_view, roughness_view] =
            self.sources.each_ref().map(|s| s.as_ref().map_or(&self.white, |s| &s.view));
        let uniform = SphereUniform {
            sh: self.lighting.sh,
            circle: [middle[0], middle[1], radius.max(1.0), (CUBE_LEVELS - 1) as f32],
            to_canvas_x: [a, c, e, 0.0],
            to_canvas_y: [b, d, f, 0.0],
            canvas: [w as f32, h as f32, look.metallic_tile.max(1.0), look.roughness_tile.max(1.0)],
            base: [BASE[0], BASE[1], BASE[2], BASE_ROUGHNESS],
            brush: [
                look.metallic.clamp(0.0, 1.0),
                look.roughness.clamp(0.0, 1.0),
                has(&self.sources[0]),
                has(&self.sources[1]),
            ],
            behind: [look.behind[0], look.behind[1], look.behind[2], 1.0],
            c0: [r0[0], r0[1], r0[2], applied],
            c1: [r1[0], r1[1], r1[2], 0.0],
            c2: [r2[0], r2[1], r2[2], 0.0],
        };
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sphere preview"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.lighting.cube_view),
                },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.cube_sampler) },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&self.lighting.table_view),
                },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.table_sampler) },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(canvas.composite_view()),
                },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&self.painted_sampler) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(metallic_view) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(roughness_view) },
                wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::Sampler(&self.source_sampler) },
            ],
        });
        let mut encoder =
            gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sphere preview") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sphere preview"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
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
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([encoder.finish()]);
    }
}

/// A `side` square grey texture of `pixels` (rows from the top).
fn grey_texture(gpu: &Gpu, side: u32, pixels: &[u8]) -> wgpu::TextureView {
    use wgpu::util::DeviceExt;
    gpu.device
        .create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("sphere source"),
                size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            pixels,
        )
        .create_view(&wgpu::TextureViewDescriptor::default())
}
