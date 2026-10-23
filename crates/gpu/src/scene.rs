//! 3D painting's view of a model: its triangles drawn textured (each part's albedo over its base
//! colour) and lit, seen by a camera orbiting it, into a target with a depth buffer of its own.

use procraft_engine::geom::IRect;
use procraft_engine::mesh::Mesh;
use wgpu::util::DeviceExt;

use crate::lighting::{CUBE_LEVELS, Lighting};
use crate::{Gpu, GpuError, read_texture};
use procraft_engine::environment::Equirect;

const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const ALBEDO: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// A projected stroke's pixels, as a canvas keeps a stroke.
const STROKE: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Each screen pixel's distance to the nearest surface.
const DISTANCE: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
/// Where a part's triangles lie in its texture.
const ISLANDS: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
/// How far past its islands a projected stroke is grown (texels; as the shader's `DILATE`).
const DILATE: i32 = 2;

/// A model is framed as if its largest side were this long, seen from this far off, with this
/// field of view (radians) across the screen's shorter side.
const FRAMED_SIDE: f32 = 4.0;
const FRAMED_DISTANCE: f32 = 8.0;
const FIELD_OF_VIEW: f32 = 0.8727;
/// The two lights, fixed to the camera: each one's place (in the framing's units, from the
/// model's middle as the camera sees it) and its intensity (radiance intensity^4 x 50, white).
const LIGHTS: [([f32; 3], f32); 2] = [([-2.0, 5.0, 2.0], 0.36), ([3.0, 4.0, 0.0], 0.30)];
/// A part's roughness and metalness when its material gives none.
const ROUGHNESS: f32 = 1.0;
const METALLIC: f32 = 0.0;
/// The light of a model before it is given an environment: the same soft grey all round.
const NEUTRAL_LIGHT: f32 = 0.6;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct VertexGpu {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewUniform {
    view_proj: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
    light0: [f32; 4],
    radiance0: [f32; 4],
    light1: [f32; 4],
    radiance1: [f32; 4],
    sh: [[f32; 4]; 9],
    params: [f32; 4],
    projection: [f32; 4],
    convert: [[f32; 4]; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ProjectUniform {
    view_proj: [[f32; 4]; 4],
    screen: [f32; 4],
    eye: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PartUniform {
    base: [f32; 4],
    /// Roughness and metalness.
    material: [f32; 4],
}

/// Where the camera is: turned by `rotation` about `target`, `distance` from it, seeing `fov`
/// (radians) across the screen's shorter side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    pub target: [f32; 3],
    /// A unit quaternion (i, j, k, real); unturned, the camera sits on +z looking along -z, up +y.
    pub rotation: [f32; 4],
    pub distance: f32,
    pub fov: f32,
}

impl Orbit {
    /// Facing the model's front (from +z) square on, its middle in the middle, as far back as a
    /// model scaled to a largest side of 4 is seen from 8 off.
    pub fn framing(mesh: &Mesh) -> Orbit {
        let (lo, hi) = mesh.bounds().unwrap_or(([-1.0; 3], [1.0; 3]));
        let target = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        Orbit { target, rotation: UNTURNED, distance: FRAMED_DISTANCE * Orbit::unit(mesh), fov: FIELD_OF_VIEW }
    }

    /// The camera turned `yaw` about the upright axis, then tilted `pitch` up (radians).
    pub fn rotation_of(yaw: f32, pitch: f32) -> [f32; 4] {
        quaternion_mul(axis_angle([0.0, 1.0, 0.0], yaw), axis_angle([1.0, 0.0, 0.0], -pitch))
    }

    /// Turns the model under a drag of (`dx`, `dy`) (right and down; any length) by `angle`
    /// radians, like a trackball: about the axis across the drag, the camera the other way.
    pub fn turn(&mut self, dx: f32, dy: f32, angle: f32) {
        let length = (dx * dx + dy * dy).sqrt();
        if !(length > 0.0 && length.is_finite() && angle.is_finite()) {
            return;
        }
        let axis = [dy / length, dx / length, 0.0];
        self.rotation = normalized_quaternion(quaternion_mul(self.rotation, axis_angle(axis, -angle)));
    }

    /// The camera carried `angle` radians round the world's upright axis through the target (the
    /// model seen spinning the other way), as a turntable does.
    pub fn spun(&self, angle: f32) -> Orbit {
        let turn = axis_angle([0.0, 1.0, 0.0], angle);
        Orbit { rotation: normalized_quaternion(quaternion_mul(turn, self.rotation)), ..*self }
    }

    /// The camera's right and up, in the world.
    pub fn right(&self) -> [f32; 3] {
        rotate_by(self.rotation, [1.0, 0.0, 0.0])
    }

    pub fn up(&self) -> [f32; 3] {
        rotate_by(self.rotation, [0.0, 1.0, 0.0])
    }

    /// The model's length that counts as one in framing: its largest side over 4.
    pub fn unit(mesh: &Mesh) -> f32 {
        let (lo, hi) = mesh.bounds().unwrap_or(([-1.0; 3], [1.0; 3]));
        let largest = hi.iter().zip(lo).map(|(h, l)| h - l).fold(0.0f32, f32::max);
        (largest / FRAMED_SIDE).max(1e-6)
    }

    /// The field of view top to bottom on a view `aspect` wide to its height.
    pub fn fov_y(&self, aspect: f32) -> f32 {
        if aspect >= 1.0 { self.fov } else { 2.0 * ((self.fov / 2.0).tan() / aspect.max(1e-3)).atan() }
    }

    /// The camera's place.
    pub fn eye(&self) -> [f32; 3] {
        let back = rotate_by(self.rotation, [0.0, 0.0, self.distance]);
        let t = self.target;
        [t[0] + back[0], t[1] + back[1], t[2] + back[2]]
    }

    /// Clip space from world space, for a view `aspect` wide to its height.
    pub fn view_proj(&self, aspect: f32) -> [[f32; 4]; 4] {
        let near = (self.distance * 0.01).max(1e-4);
        let far = self.distance * 10.0 + 1.0;
        let aspect = aspect.max(1e-3);
        mul(perspective(self.fov_y(aspect), aspect, near, far), look_at(self.eye(), self.target, self.up()))
    }
}

struct PartGpu {
    first: u32,
    count: u32,
    texture: wgpu::Texture,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    /// The albedo with its smaller levels, once given, so a far or slanting surface shows it
    /// filtered rather than speckled.
    mips: Option<(wgpu::Texture, Vec<wgpu::TextureView>, wgpu::TextureView)>,
}

/// A model on the GPU, ready to draw.
pub struct MeshView {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    parts: Vec<PartGpu>,
    pipeline: wgpu::RenderPipeline,
    view_buffer: wgpu::Buffer,
    view_bind: wgpu::BindGroup,
    depth: Option<(wgpu::Texture, wgpu::TextureView)>,
    texture_size: u32,
    part_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The model as the CPU keeps it, to tell which texels a projected stroke can reach.
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    triangles: Vec<[u32; 3]>,
    projector: Option<Projector>,
    /// What builds the albedos' smaller levels.
    mipper: Option<Mipper>,
    /// The environment it is lit by, and the layout and samplers to bind it with.
    lighting: Lighting,
    view_layout: wgpu::BindGroupLayout,
    cube_sampler: wgpu::Sampler,
    table_sampler: wgpu::Sampler,
    /// The environment drawn behind the model, and how bright the light is taken.
    backdrop: Option<wgpu::RenderPipeline>,
    exposure: f32,
    /// The framing's unit (the model's largest side over 4): the lights stand that far apart.
    unit: f32,
    /// What it draws into.
    format: wgpu::TextureFormat,
    /// The textures' colours to the target's (rows, in linear light), if they differ.
    convert: Option<[[f32; 3]; 3]>,
    pub orbit: Orbit,
    /// Strokes reach every surface under them, not only the nearest.
    pub paint_through: bool,
}

/// What carrying strokes onto the model takes: its pipelines, and the nearest surface's distance
/// for each screen pixel (with a depth buffer to find it), the screen's size.
struct Projector {
    distance_pipeline: wgpu::RenderPipeline,
    project_pipeline: wgpu::RenderPipeline,
    islands_pipeline: wgpu::RenderPipeline,
    dilate_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    screen: Option<(wgpu::Texture, wgpu::TextureView, wgpu::TextureView)>,
    /// The stroke as projected, before it is grown past the islands (the texture's size).
    scratch: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// Each part's islands, once drawn.
    islands: Vec<Option<(wgpu::Texture, wgpu::TextureView)>>,
}

impl MeshView {
    /// `mesh` on the GPU, each part with an albedo texture `texture_size` square (empty: its base
    /// colour shows, white to begin with), to draw into targets of `format`.
    pub fn new(gpu: &Gpu, mesh: &Mesh, texture_size: u32, format: wgpu::TextureFormat) -> MeshView {
        let device = &gpu.device;
        let vertices: Vec<VertexGpu> =
            mesh.vertices.iter().map(|v| VertexGpu { position: v.position, normal: v.normal, uv: v.uv }).collect();
        let indices: Vec<u32> = mesh.triangles.iter().flatten().copied().collect();
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let texture_entry = |binding, view_dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh view"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1, wgpu::TextureViewDimension::Cube),
                sampler_entry(2),
                texture_entry(3, wgpu::TextureViewDimension::D2),
                sampler_entry(4),
            ],
        });
        let part_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh part"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let view_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mesh view"),
            size: std::mem::size_of::<ViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cube_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("environment radiance"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let table_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("brdf table"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let lighting = Lighting::new(gpu, &Equirect::uniform([NEUTRAL_LIGHT; 3]));
        let view_bind = view_bind(device, &view_layout, &view_buffer, &lighting, &cube_sampler, &table_sampler);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mesh albedo"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        let side = texture_size.clamp(1, gpu.max_texture_size());
        let parts = mesh
            .parts
            .iter()
            .map(|p| {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("mesh albedo"),
                    size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: ALBEDO,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                // the material file's roughness and metalness, else plain
                let m = mesh.materials.iter().find(|m| m.name == p.material);
                let roughness = m.and_then(|m| m.roughness).unwrap_or(ROUGHNESS);
                let metallic = m.and_then(|m| m.metallic).unwrap_or(METALLIC);
                let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh part"),
                    contents: bytemuck::bytes_of(&PartUniform {
                        base: [1.0, 1.0, 1.0, 1.0],
                        material: [roughness.clamp(0.0, 1.0), metallic.clamp(0.0, 1.0), 0.0, 0.0],
                    }),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("mesh part"),
                    layout: &part_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                        wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
                    ],
                });
                PartGpu { first: (p.first * 3) as u32, count: (p.count * 3) as u32, texture, uniform, bind, mips: None }
            })
            .collect();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/mesh.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh"),
            bind_group_layouts: &[Some(&view_layout), Some(&part_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<VertexGpu>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
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
        // the environment behind the model: drawn first, over everything, under the model
        let backdrop = MeshView::backdrop_pipeline(gpu, &view_layout, format);
        MeshView {
            vertices: vertex_buffer,
            indices: index_buffer,
            parts,
            pipeline,
            view_buffer,
            view_bind,
            depth: None,
            texture_size: side,
            part_layout,
            sampler,
            positions: mesh.vertices.iter().map(|v| v.position).collect(),
            uvs: mesh.vertices.iter().map(|v| v.uv).collect(),
            triangles: mesh.triangles.clone(),
            projector: None,
            mipper: None,
            lighting,
            view_layout,
            cube_sampler,
            table_sampler,
            backdrop: Some(backdrop),
            exposure: 1.0,
            unit: Orbit::unit(mesh),
            format,
            convert: None,
            orbit: Orbit::framing(mesh),
            paint_through: false,
        }
    }

    /// Takes the textures' colours into the target's (rows of a matrix in linear light) from now
    /// on, or none when they are the same.
    pub fn set_convert(&mut self, convert: Option<[[f32; 3]; 3]>) {
        self.convert = convert;
    }

    /// The model as [`MeshView::render`] draws it, `width` × `height`, read back as RGBA8 in the
    /// textures' own colours (for a thumbnail). Waits for the GPU.
    pub fn picture(&mut self, gpu: &Gpu, width: u32, height: u32, clear: [f64; 4]) -> Result<Vec<u8>, GpuError> {
        let (width, height) = (width.max(1), height.max(1));
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("model picture"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("model picture") });
        let convert = self.convert.take();
        self.render(gpu, &mut encoder, &view, width, height, clear);
        gpu.queue.submit([encoder.finish()]);
        self.convert = convert;
        let mut rgba = read_texture(gpu, &target, width, height)?;
        if matches!(self.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb) {
            for px in rgba.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
        }
        Ok(rgba)
    }

    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    /// The side of each part's texture.
    pub fn texture_size(&self) -> u32 {
        self.texture_size
    }

    /// Part `part`'s albedo: premultiplied RGBA8, the texture's size square, rows top down.
    /// False if there is no such part or the pixels aren't that size.
    pub fn set_albedo(&mut self, gpu: &Gpu, part: usize, rgba: &[u8]) -> bool {
        let side = self.texture_size;
        let Some(p) = self.parts.get(part) else { return false };
        if rgba.len() != (side as usize) * (side as usize) * 4 {
            return false;
        }
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &p.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(side * 4), rows_per_image: Some(side) },
            wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
        );
        true
    }

    /// Part `part` shows `albedo` from now on (premultiplied, any size): a texture set's canvas
    /// as composited, say, so the model shows its layers and the stroke being drawn.
    pub fn set_albedo_view(&mut self, gpu: &Gpu, part: usize, albedo: &wgpu::TextureView) {
        let Some(p) = self.parts.get_mut(part) else { return };
        p.bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh part"),
            layout: &self.part_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(albedo) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: p.uniform.as_entire_binding() },
            ],
        });
    }

    /// Shows `albedo` (premultiplied RGBA8, the texture size square) on part `part` through
    /// smaller copies of itself made now, so a far or slanting surface shows it filtered. Called
    /// again whenever the albedo changes.
    pub fn set_albedo_filtered(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        part: usize,
        albedo: &wgpu::TextureView,
    ) {
        let side = self.texture_size;
        let mipper = self.mipper.get_or_insert_with(|| Mipper::new(gpu));
        let Some(p) = self.parts.get_mut(part) else { return };
        if p.mips.is_none() {
            let levels = u32::BITS - side.max(1).leading_zeros();
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("albedo levels"),
                size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
                mip_level_count: levels,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let level_views = (0..levels)
                .map(|level| {
                    texture.create_view(&wgpu::TextureViewDescriptor {
                        base_mip_level: level,
                        mip_level_count: Some(1),
                        ..Default::default()
                    })
                })
                .collect();
            let all = texture.create_view(&wgpu::TextureViewDescriptor::default());
            p.mips = Some((texture, level_views, all));
        }
        let Some((_, levels, all)) = p.mips.as_ref() else { return };
        // the albedo into the first level, then each level a quarter of the one before
        let mut source = albedo;
        for target in levels {
            let bind = mipper.bind(gpu, source);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("albedo level"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(if std::ptr::eq(source, albedo) { &mipper.copy } else { &mipper.halve });
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
            drop(pass);
            source = target;
        }
        p.bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh part"),
            layout: &self.part_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(all) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: p.uniform.as_entire_binding() },
            ],
        });
    }

    /// The colour part `part` shows where nothing is painted.
    pub fn set_base(&mut self, gpu: &Gpu, part: usize, color: [f32; 3]) {
        if let Some(p) = self.parts.get(part) {
            gpu.queue.write_buffer(&p.uniform, 0, bytemuck::bytes_of(&[color[0], color[1], color[2], 1.0]));
        }
    }

    /// Part `part`'s roughness and metalness (0..1).
    pub fn set_material(&mut self, gpu: &Gpu, part: usize, roughness: f32, metallic: f32) {
        if let Some(p) = self.parts.get(part) {
            let m = [roughness.clamp(0.0, 1.0), metallic.clamp(0.0, 1.0), 0.0, 0.0];
            gpu.queue.write_buffer(&p.uniform, 16, bytemuck::bytes_of(&m));
        }
    }

    /// Lights the model by `lighting` from now on.
    pub fn set_lighting(&mut self, gpu: &Gpu, lighting: Lighting) {
        self.view_bind = view_bind(
            &gpu.device,
            &self.view_layout,
            &self.view_buffer,
            &lighting,
            &self.cube_sampler,
            &self.table_sampler,
        );
        self.lighting = lighting;
    }

    /// How bright the light is taken (1 as it is).
    pub fn set_exposure(&mut self, exposure: f32) {
        if exposure.is_finite() {
            self.exposure = exposure.max(0.0);
        }
    }

    /// Whether the environment shows behind the model (else the clear colour does).
    pub fn show_environment(&mut self, gpu: &Gpu, shown: bool) {
        if shown == self.backdrop.is_some() {
            return;
        }
        // made again when wanted: the pipeline is cheap beside the frame
        self.backdrop =
            if shown { Some(MeshView::backdrop_pipeline(gpu, &self.view_layout, self.format)) } else { None };
    }

    fn backdrop_pipeline(
        gpu: &Gpu,
        view_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/mesh.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh backdrop"),
            bind_group_layouts: &[Some(view_layout)],
            immediate_size: 0,
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh backdrop"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_backdrop"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_backdrop"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// Draws the model into `target` (`width` × `height`), cleared to `clear` first.
    pub fn render(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        clear: [f64; 4],
    ) {
        let (width, height) = (width.max(1), height.max(1));
        let stale = self.depth.as_ref().is_none_or(|(t, _)| t.width() != width || t.height() != height);
        if stale {
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("mesh depth"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.depth = Some((texture, view));
        }
        let Some((_, depth_view)) = &self.depth else { return };
        let aspect = width as f32 / height as f32;
        let view = look_at(self.orbit.eye(), self.orbit.target, self.orbit.up());
        // the lights stand about the model's middle as the camera sees it: they turn with it
        let middle = view[3];
        let light = |k: usize| {
            let (place, intensity) = LIGHTS.get(k).copied().unwrap_or(([0.0; 3], 0.0));
            let [x, y, z] = place;
            let at = [x * self.unit + middle[0], y * self.unit + middle[1], z * self.unit + middle[2]];
            let radiance = intensity.powi(4) * 50.0;
            ([at[0], at[1], at[2], 1.0], [radiance, radiance, radiance, 0.0])
        };
        let ((light0, radiance0), (light1, radiance1)) = (light(0), light(1));
        let f = 1.0 / (self.orbit.fov_y(aspect) / 2.0).tan();
        let uniform = ViewUniform {
            view_proj: self.orbit.view_proj(aspect),
            view,
            light0,
            radiance0,
            light1,
            radiance1,
            sh: self.lighting.sh,
            params: [self.exposure, (CUBE_LEVELS - 1) as f32, width as f32, height as f32],
            projection: [f / aspect, f, 0.0, 0.0],
            convert: self.convert.map_or([[0.0; 4]; 3], |m| m.map(|r| [r[0], r[1], r[2], 1.0])),
        };
        gpu.queue.write_buffer(&self.view_buffer, 0, bytemuck::bytes_of(&uniform));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mesh"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: clear[0], g: clear[1], b: clear[2], a: clear[3] }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.view_bind, &[]);
        if let Some(backdrop) = &self.backdrop {
            pass.set_pipeline(backdrop);
            pass.draw(0..3, 0..1);
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        for p in &self.parts {
            pass.set_bind_group(1, &p.bind, &[]);
            pass.draw_indexed(p.first..p.first + p.count, 0, 0..1);
        }
    }
}

impl MeshView {
    /// Carries `stroke` (premultiplied, drawn on a `width` × `height` screen, inside `bounds`) onto
    /// part `part` as the camera sees it, into `target` (the part's texture, its size square, the
    /// stroke format): each texel takes the stroke where its surface lands on the screen, if no
    /// other surface is nearer there. Returns the texels it can have reached.
    #[allow(clippy::too_many_arguments)]
    pub fn project_stroke(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        part: usize,
        stroke: &wgpu::TextureView,
        bounds: IRect,
        width: u32,
        height: u32,
        target: &wgpu::TextureView,
    ) -> IRect {
        let (width, height) = (width.max(1), height.max(1));
        let Some(p) = self.parts.get(part) else { return IRect::EMPTY };
        let (first, count) = (p.first, p.count);
        let view_proj = self.orbit.view_proj(width as f32 / height as f32);
        let projector = self.projector.get_or_insert_with(|| Projector::new(gpu));
        let stale = projector.screen.as_ref().is_none_or(|(t, _, _)| t.width() != width || t.height() != height);
        if stale {
            let make = |label, format, usage| {
                gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let distance = make(
                "screen distance",
                DISTANCE,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            );
            let depth = make("screen depth", DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT);
            let distance_view = distance.create_view(&wgpu::TextureViewDescriptor::default());
            let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
            projector.screen = Some((distance, distance_view, depth_view));
        }
        let side = self.texture_size;
        let make_square = |label, format| {
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            (texture, view)
        };
        if projector.scratch.is_none() {
            projector.scratch = Some(make_square("projected stroke", STROKE));
        }
        if projector.islands.len() < self.parts.len() {
            projector.islands.resize_with(self.parts.len(), || None);
        }
        // the part's islands, drawn the first time it is painted
        if projector.islands.get(part).is_some_and(Option::is_none) {
            let (texture, view) = make_square("islands", ISLANDS);
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("islands"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&projector.islands_pipeline);
                pass.set_bind_group(0, &projector.bind(gpu, stroke, None), &[]);
                pass.set_vertex_buffer(0, self.vertices.slice(..));
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(first..first + count, 0, 0..1);
            }
            if let Some(slot) = projector.islands.get_mut(part) {
                *slot = Some((texture, view));
            }
        }
        let (Some((_, distance_view, depth_view)), Some((_, scratch)), Some(Some((_, islands)))) =
            (&projector.screen, &projector.scratch, projector.islands.get(part))
        else {
            return IRect::EMPTY;
        };
        gpu.queue.write_buffer(
            &projector.uniform,
            0,
            bytemuck::bytes_of(&ProjectUniform {
                view_proj,
                screen: [width as f32, height as f32, if self.paint_through { 1.0 } else { 0.0 }, 0.0],
                eye: {
                    let e = self.orbit.eye();
                    [e[0], e[1], e[2], 1.0]
                },
            }),
        );
        // the nearest surface's distance for each screen pixel: the whole model's
        {
            let bind = projector.bind(gpu, stroke, None);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("screen distance"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: distance_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 1e30, g: 0.0, b: 0.0, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&projector.distance_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..(self.triangles.len() * 3) as u32, 0, 0..1);
        }
        // the part in its texture's space, taking the stroke where it is the nearest surface
        {
            let bind = projector.bind(gpu, stroke, Some(distance_view));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("project stroke"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: scratch,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&projector.project_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(first..first + count, 0, 0..1);
        }
        // grown past the islands' edges into the target
        {
            let bind = projector.bind(gpu, scratch, Some(islands));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("dilate stroke"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&projector.dilate_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        let reach = self.reach(part, view_proj, bounds, width, height);
        if reach.is_empty() {
            return reach;
        }
        let grown = IRect::new(reach.x0 - DILATE, reach.y0 - DILATE, reach.x1 + DILATE, reach.y1 + DILATE);
        grown.intersect(IRect::from_size(self.texture_size, self.texture_size))
    }

    /// The texels of part `part` whose triangles land on the screen inside `bounds`.
    fn reach(&self, part: usize, view_proj: [[f32; 4]; 4], bounds: IRect, width: u32, height: u32) -> IRect {
        let Some(p) = self.parts.get(part) else { return IRect::EMPTY };
        let side = self.texture_size as f32;
        let tris = self.triangles.get(p.first as usize / 3..(p.first + p.count) as usize / 3).unwrap_or(&[]);
        let mut out = IRect::EMPTY;
        for t in tris {
            let mut screen = IRect::EMPTY;
            let mut behind = false;
            for &i in t {
                let Some(q) = self.positions.get(i as usize) else { continue };
                let c = transform(view_proj, *q);
                if c[3] <= 1e-6 {
                    behind = true;
                    continue;
                }
                let (x, y) = ((c[0] / c[3] * 0.5 + 0.5) * width as f32, (0.5 - c[1] / c[3] * 0.5) * height as f32);
                screen = screen.union(pixels_round(x, y));
            }
            // a triangle reaching behind the camera may land anywhere: counted in
            if !behind && screen.intersect(bounds).is_empty() {
                continue;
            }
            for &i in t {
                if let Some(&[u, v]) = self.uvs.get(i as usize) {
                    out = out.union(pixels_round(u * side, (1.0 - v) * side));
                }
            }
        }
        out.intersect(IRect::from_size(self.texture_size, self.texture_size))
    }
}

/// What makes an albedo's smaller levels: a copy into the first, then each level the average of
/// the four texels under each of its own.
struct Mipper {
    copy: wgpu::RenderPipeline,
    halve: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

impl Mipper {
    fn new(gpu: &Gpu) -> Mipper {
        let device = &gpu.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("albedo levels"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("albedo levels"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/mips.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("albedo levels"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |fs| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("albedo levels"),
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
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Mipper { copy: pipeline("fs_copy"), halve: pipeline("fs_halve"), layout }
    }

    fn bind(&self, gpu: &Gpu, source: &wgpu::TextureView) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("albedo level"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(source) }],
        })
    }
}

/// The view's bindings: its uniform, and the lighting's radiance cube and table.
fn view_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    lighting: &Lighting,
    cube_sampler: &wgpu::Sampler,
    table_sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mesh view"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&lighting.cube_view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(cube_sampler) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&lighting.table_view) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(table_sampler) },
        ],
    })
}

/// Keeps the most of what is there and what is drawn, in each channel.
const MAX: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Max,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Max,
    },
};

impl Projector {
    fn new(gpu: &Gpu) -> Projector {
        let device = &gpu.device;
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("project"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1),
                texture_entry(2),
            ],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("project"),
            size: std::mem::size_of::<ProjectUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("project"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/project.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("project"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label, vs, fs, format, depth: bool, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<VertexGpu>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: depth.then(|| wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // drawn over the whole texture, with no model
        let dilate_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dilate stroke"),
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
                entry_point: Some("fs_dilate"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: STROKE,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Projector {
            distance_pipeline: pipeline("screen distance", "vs_distance", "fs_distance", DISTANCE, true, None),
            project_pipeline: pipeline("project stroke", "vs_project", "fs_project", STROKE, false, Some(MAX)),
            islands_pipeline: pipeline("islands", "vs_islands", "fs_islands", ISLANDS, false, None),
            dilate_pipeline,
            layout,
            uniform,
            screen: None,
            scratch: None,
            islands: Vec::new(),
        }
    }

    /// Its bindings: the uniform, the stroke and the screen's distances (the stroke standing in
    /// while the distances are being drawn).
    fn bind(&self, gpu: &Gpu, stroke: &wgpu::TextureView, distance: Option<&wgpu::TextureView>) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("project"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(stroke) },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(distance.unwrap_or(stroke)),
                },
            ],
        })
    }
}

/// The pixels round a point, one to spare each way.
fn pixels_round(x: f32, y: f32) -> IRect {
    IRect::new(x.floor() as i32 - 1, y.floor() as i32 - 1, x.ceil() as i32 + 1, y.ceil() as i32 + 1)
}

/// `m` (columns) applied to point `p`: clip coordinates.
fn transform(m: [[f32; 4]; 4], p: [f32; 3]) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for (col, w) in m.iter().zip([p[0], p[1], p[2], 1.0]) {
        for (o, v) in out.iter_mut().zip(col) {
            *o += v * w;
        }
    }
    out
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = dot(v, v).sqrt();
    if l > 1e-12 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 0.0, 1.0] }
}

/// A view matrix (columns), right-handed: the camera at `eye` looking at `target`.
fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [[f32; 4]; 4] {
    let f = normalize(sub(target, eye));
    let s = normalize(cross(f, up));
    let u = cross(s, f);
    [
        [s[0], u[0], -f[0], 0.0],
        [s[1], u[1], -f[1], 0.0],
        [s[2], u[2], -f[2], 0.0],
        [-dot(s, eye), -dot(u, eye), dot(f, eye), 1.0],
    ]
}

/// The unturned rotation.
const UNTURNED: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// A turn of `angle` radians about unit `axis`, as a quaternion (i, j, k, real).
fn axis_angle(axis: [f32; 3], angle: f32) -> [f32; 4] {
    let (s, c) = (angle / 2.0).sin_cos();
    [axis[0] * s, axis[1] * s, axis[2] * s, c]
}

/// The turn `a` followed by the turn `b` taken in `a`'s own frame (the product a b).
fn quaternion_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

fn normalized_quaternion(q: [f32; 4]) -> [f32; 4] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n > 1e-12 && n.is_finite() { q.map(|v| v / n) } else { UNTURNED }
}

/// `v` turned by unit quaternion `q`.
fn rotate_by(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let u = [q[0], q[1], q[2]];
    let w = q[3];
    let t = cross(u, v).map(|c| 2.0 * c);
    let ut = cross(u, t);
    [v[0] + w * t[0] + ut[0], v[1] + w * t[1] + ut[1], v[2] + w * t[2] + ut[2]]
}

/// A perspective projection (columns) to wgpu's clip space (depth 0 to 1).
fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (fov_y / 2.0).tan();
    let r = far / (near - far);
    [[f / aspect, 0.0, 0.0, 0.0], [0.0, f, 0.0, 0.0], [0.0, 0.0, r, -1.0], [0.0, 0.0, r * near, 0.0]]
}

/// The product a × b of column-major matrices (b applied first).
fn mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    // each column of the product: a's columns weighted by b's column
    b.map(|col| {
        let mut out = [0.0f32; 4];
        for (ak, bk) in a.iter().zip(col) {
            for (o, v) in out.iter_mut().zip(ak) {
                *o += v * bk;
            }
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trackball_turns_the_camera_freely() {
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4);
        let at = |rotation| Orbit { target: [0.0; 3], rotation, distance: 2.0, fov: 0.9 };
        // the angles' rotation puts the eye where yaw and pitch did
        let (yaw, pitch) = (0.6f32, 0.4f32);
        let want = [2.0 * pitch.cos() * yaw.sin(), 2.0 * pitch.sin(), 2.0 * pitch.cos() * yaw.cos()];
        assert!(close(at(Orbit::rotation_of(yaw, pitch)).eye(), want));
        // dragging right takes the camera left (the model follows the finger); down, up
        let mut o = at(UNTURNED);
        o.turn(10.0, 0.0, 0.3);
        assert!(o.eye()[0] < -0.5 && close(o.up(), [0.0, 1.0, 0.0]));
        let mut o = at(UNTURNED);
        o.turn(0.0, 10.0, 0.3);
        assert!(o.eye()[1] > 0.5);
        // over the top and down the far side: no stop at the pole
        let mut o = at(UNTURNED);
        for _ in 0..10 {
            o.turn(0.0, 10.0, 0.3);
        }
        assert!(o.eye()[2] < -1.9 && o.up()[1] < -0.9, "{:?} {:?}", o.eye(), o.up());
        // nothing for no drag
        let mut o = at(UNTURNED);
        o.turn(0.0, 0.0, 1.0);
        assert_eq!(o.rotation, UNTURNED);
    }

    #[test]
    fn the_target_projects_to_the_middle() {
        let orbit = Orbit { target: [1.0, 2.0, 3.0], rotation: Orbit::rotation_of(0.7, 0.3), distance: 5.0, fov: 0.8 };
        let m = orbit.view_proj(1.5);
        let t = orbit.target;
        let clip: Vec<f32> = (0..4).map(|r| m[0][r] * t[0] + m[1][r] * t[1] + m[2][r] * t[2] + m[3][r]).collect();
        let (x, y, z, w) = (clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3], clip[3]);
        assert!(x.abs() < 1e-4 && y.abs() < 1e-4, "{x} {y}");
        assert!(w > 0.0 && (0.0..1.0).contains(&z), "{z} {w}");
    }
}
