//! The light a 3D model is lit by, from an environment panorama: its irradiance (as spherical
//! harmonics, for diffuse light and the backdrop), a radiance cube prefiltered for rough
//! reflections (each level rougher), and the split-sum table for the reflections' strength.

use procraft_engine::environment::{Equirect, brdf_table, to_half};
use wgpu::util::DeviceExt;

use crate::Gpu;

/// The radiance cube's side and levels (the last 2 texels across).
const CUBE_SIDE: u32 = 256;
pub(crate) const CUBE_LEVELS: u32 = 8;
const CUBE: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The BRDF table's side and samples per cell.
const TABLE_SIDE: usize = 64;
const TABLE_SAMPLES: u32 = 256;
/// Panoramas are prefiltered from at most this wide.
const PANORAMA_WIDTH: u32 = 1024;

/// An environment ready to light with.
pub struct Lighting {
    pub(crate) cube_view: wgpu::TextureView,
    _cube: wgpu::Texture,
    pub(crate) table_view: wgpu::TextureView,
    _table: wgpu::Texture,
    /// The irradiance's nine coefficients (RGB, a fourth unused).
    pub(crate) sh: [[f32; 4]; 9],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PrefilterUniform {
    face: u32,
    side: f32,
    roughness: f32,
    texels: f32,
}

impl Lighting {
    /// The light of `panorama` (linear RGB).
    pub fn new(gpu: &Gpu, panorama: &Equirect) -> Lighting {
        let device = &gpu.device;
        let panorama = panorama.at_most(PANORAMA_WIDTH);
        let sh = panorama.irradiance_sh().map(|c| [c[0], c[1], c[2], 0.0]);

        // the panorama with its own smaller levels, for the prefilter to read coarser where a
        // sample stands for more of the sphere
        let mut levels = vec![panorama.clone()];
        while let Some(last) = levels.last()
            && (last.width > 1 || last.height > 1)
        {
            levels.push(last.halved());
        }
        let source = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("environment panorama"),
            size: wgpu::Extent3d { width: panorama.width, height: panorama.height, depth_or_array_layers: 1 },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, e) in levels.iter().enumerate() {
            let halves: Vec<u16> =
                e.pixels.iter().flat_map(|p| [to_half(p[0]), to_half(p[1]), to_half(p[2]), to_half(1.0)]).collect();
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &source,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&halves),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(e.width * 8),
                    rows_per_image: Some(e.height),
                },
                wgpu::Extent3d { width: e.width, height: e.height, depth_or_array_layers: 1 },
            );
        }
        let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("environment panorama"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        // the radiance cube, level by level and face by face
        let cube = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("environment radiance"),
            size: wgpu::Extent3d { width: CUBE_SIDE, height: CUBE_SIDE, depth_or_array_layers: 6 },
            mip_level_count: CUBE_LEVELS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CUBE,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("prefilter"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("prefilter"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/prefilter.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("prefilter"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("prefilter"),
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
                entry_point: Some("fs_prefilter"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: CUBE,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let texels = (panorama.width * panorama.height) as f32;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("prefilter") });
        for level in 0..CUBE_LEVELS {
            let side = (CUBE_SIDE >> level).max(1);
            let roughness = level as f32 / (CUBE_LEVELS - 1) as f32;
            for face in 0..6u32 {
                let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("prefilter"),
                    contents: bytemuck::bytes_of(&PrefilterUniform { face, side: side as f32, roughness, texels }),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("prefilter"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&source_view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
                    ],
                });
                let target = cube.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("environment radiance face"),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    base_array_layer: face,
                    array_layer_count: Some(1),
                    ..Default::default()
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("prefilter"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target,
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
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        gpu.queue.submit([encoder.finish()]);
        let cube_view = cube.create_view(&wgpu::TextureViewDescriptor {
            label: Some("environment radiance"),
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });

        // the split-sum table: scale and bias by n.v across and roughness down
        let table_data: Vec<u16> =
            brdf_table(TABLE_SIDE, TABLE_SAMPLES).iter().flat_map(|v| [to_half(v[0]), to_half(v[1])]).collect();
        let side = TABLE_SIDE as u32;
        let table = device.create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("brdf table"),
                size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rg16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(&table_data),
        );
        let table_view = table.create_view(&wgpu::TextureViewDescriptor::default());
        Lighting { cube_view, _cube: cube, table_view, _table: table, sh }
    }
}
