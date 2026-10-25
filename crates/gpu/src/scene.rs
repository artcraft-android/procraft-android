//! 3D painting's view of a model: its triangles drawn textured (each part's albedo over its base
//! colour) and lit, seen by a camera orbiting it, into a target with a depth buffer of its own.

use procraft_engine::geom::IRect;
use procraft_engine::mesh::Mesh;
use wgpu::util::DeviceExt;

use crate::lighting::{CUBE_LEVELS, Lighting};
use crate::{Gpu, GpuError, SurroundGrid, read_texture};
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
/// The most lights a model is lit by.
pub const MOST_LIGHTS: usize = 4;
/// The Lighting Studio's camera as it first opens: tilted this far down (radians) and this far
/// back (in the framing's units, the model's largest side counted as 4).
const STUDIO_PITCH: f32 = 0.716;
const STUDIO_DISTANCE: f32 = 22.0;
/// The studio's floor under the model: its radius and a grid cell's side, and each light's cube
/// (framing units).
const FLOOR_RADIUS: f32 = 9.5;
const FLOOR_CELL: f32 = 1.0;
const CUBE_SIDE: f32 = 0.8;

/// A light shining on the model: its place (in the framing's units from the model's middle, along
/// the world's axes), its colour (linear RGB) and its intensity (0 to 1; its radiance is the
/// intensity^4 x 50 of its colour).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub place: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
}

impl Light {
    /// The lights a model starts with: white, one high on the left in front, one high on the right.
    pub const DEFAULTS: [Light; 2] = [
        Light { place: [-2.0, 5.0, 2.0], color: [1.0; 3], intensity: 0.36 },
        Light { place: [3.0, 4.0, 0.0], color: [1.0; 3], intensity: 0.30 },
    ];
}
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
    /// Each light's place in the camera's space, and its radiance; how many there are.
    lights: [[f32; 4]; MOST_LIGHTS],
    radiances: [[f32; 4]; MOST_LIGHTS],
    sh: [[f32; 4]; 9],
    params: [f32; 4],
    projection: [f32; 4],
    convert: [[f32; 4]; 3],
    counts: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct StudioUniform {
    view_proj: [[f32; 4]; 4],
    floor: [f32; 4],
    cubes: [[f32; 4]; MOST_LIGHTS],
    colors: [[f32; 4]; MOST_LIGHTS],
    target: [f32; 4],
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
    /// Roughness and metalness, and whether it has a normal map and an occlusion map (1 or 0).
    material: [f32; 4],
}

/// The maps a model's file brings for a part and that are shown as they are (not painted).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceMap {
    /// Tangent-space normals (RGB, 0.5 the middle; green up the texture's v).
    Normal,
    /// Ambient occlusion: how much of the environment's light reaches each point (grey).
    Occlusion,
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
    /// What it shows: its albedo, its roughness and metalness painted over the material's, and
    /// its normal and occlusion maps.
    views: [wgpu::TextureView; 5],
    /// The normal and occlusion maps, once given.
    maps: [Option<wgpu::Texture>; 2],
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
    /// A depth buffer for each size the model was last drawn at (the window, and small under
    /// frosted panels).
    depths: Vec<(wgpu::Texture, wgpu::TextureView)>,
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
    /// Without the environment behind it, the canvas's surround grid is, if given; and what draws it.
    grid: Option<SurroundGrid>,
    grid_draw: Option<GridDraw>,
    /// The model's middle and the height of its lowest point (the studio's floor), in the world.
    middle: [f32; 3],
    floor: f32,
    /// What draws the Lighting Studio's floor and lights.
    studio_draw: Option<StudioDraw>,
    /// The lights it is lit by (at most [`MOST_LIGHTS`]).
    pub lights: Vec<Light>,
    /// A channel with nothing painted in it.
    blank: wgpu::TextureView,
    /// Each submesh's part and its run of indices, and whether it is hidden.
    submeshes: Vec<(usize, u32, u32)>,
    hidden: Vec<bool>,
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

/// The surround grid's pipeline and its uniform.
struct GridDraw {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GridUniform {
    v0: [f32; 4],
    v1: [f32; 4],
    v2: [f32; 4],
}

impl GridDraw {
    fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> GridDraw {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh surround"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/surround.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh surround"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mesh surround"),
            size: std::mem::size_of::<GridUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh surround"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh surround"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh surround"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_surround"),
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
                entry_point: Some("fs_surround"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        GridDraw { pipeline, buffer, bind }
    }
}

/// The Lighting Studio's floor and lights' pipelines and their uniform.
struct StudioDraw {
    floor: wgpu::RenderPipeline,
    cubes: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

impl StudioDraw {
    fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> StudioDraw {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("studio"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/studio.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("studio"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("studio"),
            size: std::mem::size_of::<StudioUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("studio"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("studio"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label: &str, vs: &str, fs: &str, write: bool, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(write),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
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
        let floor = pipeline(
            "studio floor",
            "vs_floor",
            "fs_floor",
            false,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let cubes = pipeline("studio lights", "vs_cube", "fs_cube", true, None);
        StudioDraw { floor, cubes, buffer, bind }
    }
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
                map_entry(3),
                map_entry(4),
                map_entry(5),
                map_entry(6),
            ],
        });
        // nothing painted in a channel: the material's own value shows
        let blank = device
            .create_texture_with_data(
                &gpu.queue,
                &wgpu::TextureDescriptor {
                    label: Some("mesh blank map"),
                    size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: ALBEDO,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &[0, 0, 0, 0],
            )
            .create_view(&wgpu::TextureViewDescriptor::default());
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
            .enumerate()
            .map(|(i, p)| {
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
                // the material's own colour, roughness and metalness under what is painted
                let ([r, g, b], roughness, metallic) = mesh.surface(i);
                let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh part"),
                    contents: bytemuck::bytes_of(&PartUniform {
                        base: [r, g, b, 1.0],
                        material: [roughness, metallic, 0.0, 0.0],
                    }),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });
                let views = [view, blank.clone(), blank.clone(), blank.clone(), blank.clone()];
                let bind = part_bind(device, &part_layout, &sampler, &views, &uniform);
                PartGpu {
                    first: (p.first * 3) as u32,
                    count: (p.count * 3) as u32,
                    texture,
                    uniform,
                    bind,
                    views,
                    maps: [None, None],
                    mips: None,
                }
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
            depths: Vec::new(),
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
            grid: None,
            grid_draw: None,
            middle: mesh
                .bounds()
                .map_or([0.0; 3], |(lo, hi)| [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0]),
            floor: mesh.bounds().map_or(-1.0, |(lo, _)| lo[1]),
            studio_draw: None,
            lights: Light::DEFAULTS.to_vec(),
            blank,
            submeshes: mesh.submeshes.iter().map(|m| (m.part, (m.first * 3) as u32, (m.count * 3) as u32)).collect(),
            hidden: vec![false; mesh.submeshes.len()],
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

    /// Submesh `index` on its own, seen from where the camera looks at the model now, filling a
    /// `side` square with a little room round it, on nothing (RGBA8): its row's picture.
    pub fn submesh_picture(&mut self, gpu: &Gpu, index: usize, side: u32) -> Result<Vec<u8>, GpuError> {
        let Some(&(_, first, count)) = self.submeshes.get(index) else {
            return Ok(vec![0; (side * side * 4) as usize]);
        };
        // its corners' middle and how far they reach from it
        let tris = self.triangles.get(first as usize / 3..(first + count) as usize / 3).unwrap_or(&[]);
        let corners: Vec<[f32; 3]> =
            tris.iter().flatten().filter_map(|&i| self.positions.get(i as usize).copied()).collect();
        let (lo, hi) = corners.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
            ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])])
        });
        let middle = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        let reach = corners
            .iter()
            .map(|p| ((p[0] - middle[0]).powi(2) + (p[1] - middle[1]).powi(2) + (p[2] - middle[2]).powi(2)).sqrt())
            .fold(0.0f32, f32::max)
            .max(1e-6);
        let kept = (self.orbit, self.hidden.clone(), self.backdrop.take(), self.grid.take());
        self.orbit = Orbit { target: middle, distance: reach * 1.15 / (self.orbit.fov / 2.0).sin(), ..self.orbit };
        for (k, h) in self.hidden.iter_mut().enumerate() {
            *h = k != index;
        }
        let picture = self.picture(gpu, side, side, [0.0; 4]);
        (self.orbit, self.hidden, self.backdrop, self.grid) = kept;
        picture
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
        p.views[0] = albedo.clone();
        p.bind = part_bind(&gpu.device, &self.part_layout, &self.sampler, &p.views, &p.uniform);
    }

    /// Part `part`'s roughness (`channel` 1) or metalness (2) painted over its material's from
    /// now on: `map` premultiplied, its grey taking the place of the material's value as far as
    /// it covers (any size); none, the material's own everywhere.
    pub fn set_channel_view(&mut self, gpu: &Gpu, part: usize, channel: usize, map: Option<&wgpu::TextureView>) {
        let Some(p) = self.parts.get_mut(part) else { return };
        let Some(slot) = p.views.get_mut(channel).filter(|_| channel == 1 || channel == 2) else { return };
        *slot = map.unwrap_or(&self.blank).clone();
        p.bind = part_bind(&gpu.device, &self.part_layout, &self.sampler, &p.views, &p.uniform);
    }

    /// Shades part `part` by its `kind` of map from now on: `rgba` (`side` square, RGBA8); none,
    /// without one.
    pub fn set_surface_map(&mut self, gpu: &Gpu, part: usize, kind: SurfaceMap, map: Option<(u32, &[u8])>) {
        let Some(p) = self.parts.get_mut(part) else { return };
        let k = match kind {
            SurfaceMap::Normal => 0,
            SurfaceMap::Occlusion => 1,
        };
        let texture =
            map.filter(|(side, rgba)| *side > 0 && rgba.len() == (*side as usize).pow(2) * 4).map(|(side, rgba)| {
                gpu.device.create_texture_with_data(
                    &gpu.queue,
                    &wgpu::TextureDescriptor {
                        label: Some("mesh surface map"),
                        size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    rgba,
                )
            });
        let view = texture
            .as_ref()
            .map_or_else(|| self.blank.clone(), |t| t.create_view(&wgpu::TextureViewDescriptor::default()));
        let shown = if texture.is_some() { 1.0f32 } else { 0.0 };
        if let (Some(v), Some(slot)) = (p.views.get_mut(3 + k), p.maps.get_mut(k)) {
            *v = view;
            *slot = texture;
        }
        // its flag: the material's third or fourth number
        gpu.queue.write_buffer(&p.uniform, 24 + 4 * k as u64, bytemuck::bytes_of(&shown));
        p.bind = part_bind(&gpu.device, &self.part_layout, &self.sampler, &p.views, &p.uniform);
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
        p.views[0] = all.clone();
        p.bind = part_bind(&gpu.device, &self.part_layout, &self.sampler, &p.views, &p.uniform);
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
            let m = [roughness.clamp(0.0, 1.0), metallic.clamp(0.0, 1.0)];
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

    /// The grid drawn over the clear colour while the environment doesn't show behind the model.
    pub fn set_grid(&mut self, grid: Option<SurroundGrid>) {
        self.grid = grid;
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
        self.draw(gpu, encoder, target, (width, height), clear, None);
    }

    /// Draws the Lighting Studio into `target` (`width` × `height`), cleared to `clear` first: the
    /// model seen through `orbit` on its round floor, each light a cube.
    #[allow(clippy::too_many_arguments)]
    pub fn render_studio(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        clear: [f64; 4],
        orbit: Orbit,
    ) {
        self.draw(gpu, encoder, target, (width, height), clear, Some(orbit));
    }

    /// Hides submesh `index` (or shows it again): not drawn, and not painted.
    pub fn set_hidden(&mut self, index: usize, hidden: bool) {
        if let Some(h) = self.hidden.get_mut(index) {
            *h = hidden;
        }
    }

    /// The runs of indices of part `part` that are shown: all of it, unless a submesh in it is
    /// hidden.
    fn shown_ranges(&self, part: usize) -> Vec<std::ops::Range<u32>> {
        let Some(p) = self.parts.get(part) else { return Vec::new() };
        let mine = || self.submeshes.iter().zip(&self.hidden).filter(move |((at, _, _), _)| *at == part);
        if !mine().any(|(_, &h)| h) {
            return std::iter::once(p.first..p.first + p.count).collect();
        }
        mine().filter(|(_, h)| !**h).map(|((_, first, count), _)| *first..*first + *count).collect()
    }

    /// The studio's camera as it first opens: the model from in front, high up and far back.
    pub fn studio_orbit(&self) -> Orbit {
        Orbit {
            target: self.middle,
            rotation: Orbit::rotation_of(0.0, STUDIO_PITCH),
            distance: STUDIO_DISTANCE * self.unit,
            fov: FIELD_OF_VIEW,
        }
    }

    /// The model upright, turned `yaw` about its upright axis, seen from `pitch` above level and
    /// `distance` back (framing units), its middle in the middle.
    pub fn upright_orbit(&self, yaw: f32, pitch: f32, distance: f32) -> Orbit {
        Orbit {
            target: self.middle,
            rotation: Orbit::rotation_of(yaw, pitch),
            distance: distance * self.unit,
            fov: FIELD_OF_VIEW,
        }
    }

    /// The picture of the model as `orbit` sees it at `width` × `height` (RGBA8), on `clear`,
    /// with or without the environment behind it (never the surround's grid).
    pub fn picture_from(
        &mut self,
        gpu: &Gpu,
        orbit: Orbit,
        width: u32,
        height: u32,
        clear: [f64; 4],
        environment: bool,
    ) -> Result<Vec<u8>, GpuError> {
        let kept = (self.orbit, self.grid.take(), self.backdrop.is_some());
        self.orbit = orbit;
        if !environment {
            self.backdrop = None;
        }
        let picture = self.picture(gpu, width, height, clear);
        self.orbit = kept.0;
        self.grid = kept.1;
        if kept.2 && self.backdrop.is_none() {
            self.backdrop = Some(MeshView::backdrop_pipeline(gpu, &self.view_layout, self.format));
        }
        picture
    }

    /// Where `light` stands in the world.
    fn light_world(&self, light: &Light) -> [f32; 3] {
        let m = self.middle;
        let p = light.place;
        [m[0] + p[0] * self.unit, m[1] + p[1] * self.unit, m[2] + p[2] * self.unit]
    }

    /// Light `index` on a `width` × `height` screen seen through `orbit`: its middle and about how
    /// far its cube reaches from it (pixels); none if it is behind the camera.
    pub fn light_on_screen(&self, orbit: &Orbit, index: usize, width: u32, height: u32) -> Option<[f32; 3]> {
        let light = self.lights.get(index)?;
        self.place_on_screen(orbit, light.place, width, height)
    }

    /// A light's `place` (framing units from the model's middle) on a `width` × `height` screen
    /// seen through `orbit`, as [`MeshView::light_on_screen`] gives it.
    pub fn place_on_screen(&self, orbit: &Orbit, place: [f32; 3], width: u32, height: u32) -> Option<[f32; 3]> {
        let at = self.light_world(&Light { place, color: [1.0; 3], intensity: 0.0 });
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        let depth = orbit_depth(orbit, at);
        if depth <= orbit.distance * 0.01 {
            return None;
        }
        let m = orbit.view_proj(w / h);
        let clip = transform(m, at);
        if clip[3] <= 0.0 {
            return None;
        }
        let (x, y) = ((clip[0] / clip[3] + 1.0) / 2.0 * w, (1.0 - clip[1] / clip[3]) / 2.0 * h);
        let per_pixel = 2.0 * depth * (orbit.fov_y(w / h) / 2.0).tan() / h;
        Some([x, y, CUBE_SIDE * self.unit * 0.8 / per_pixel])
    }

    /// Moves light `index` by (`dx`, `dy`) pixels (right and down) on a `width` × `height` screen
    /// seen through `orbit`: across the plane facing the camera at the light's depth.
    pub fn move_light(&mut self, orbit: &Orbit, index: usize, dx: f32, dy: f32, width: u32, height: u32) {
        let Some(light) = self.lights.get(index).copied() else { return };
        let place = self.moved_place(orbit, light.place, dx, dy, width, height);
        if let Some(l) = self.lights.get_mut(index) {
            l.place = place;
        }
    }

    /// A light's `place` dragged by (`dx`, `dy`) pixels, as [`MeshView::move_light`] moves it.
    pub fn moved_place(&self, orbit: &Orbit, place: [f32; 3], dx: f32, dy: f32, width: u32, height: u32) -> [f32; 3] {
        let at = self.light_world(&Light { place, color: [1.0; 3], intensity: 0.0 });
        let depth = orbit_depth(orbit, at).max(orbit.distance * 0.05);
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        let per_unit = 2.0 * depth * (orbit.fov_y(w / h) / 2.0).tan() / h / self.unit;
        let (right, up) = (orbit.right(), orbit.up());
        let mut out = place;
        for (k, p) in out.iter_mut().enumerate() {
            if let (Some(r), Some(u)) = (right.get(k), up.get(k)) {
                *p += (dx * r - dy * u) * per_unit;
            }
        }
        out
    }

    fn draw(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        (width, height): (u32, u32),
        clear: [f64; 4],
        studio: Option<Orbit>,
    ) {
        let orbit = studio.unwrap_or(self.orbit);
        let (width, height) = (width.max(1), height.max(1));
        let light_worlds: Vec<[f32; 3]> = self.lights.iter().take(MOST_LIGHTS).map(|l| self.light_world(l)).collect();
        let shown_by_part: Vec<Vec<std::ops::Range<u32>>> =
            (0..self.parts.len()).map(|i| self.shown_ranges(i)).collect();
        let found = self.depths.iter().position(|(t, _)| t.width() == width && t.height() == height);
        let at = if let Some(i) = found {
            i
        } else {
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
            if self.depths.len() >= 2 {
                self.depths.remove(0);
            }
            self.depths.push((texture, view));
            self.depths.len() - 1
        };
        let Some((_, depth_view)) = self.depths.get(at) else { return };
        let aspect = width as f32 / height as f32;
        let view = look_at(orbit.eye(), orbit.target, orbit.up());
        // the lights stand about the model in the world, seen from the camera
        let mut lights = [[0.0; 4]; MOST_LIGHTS];
        let mut radiances = [[0.0; 4]; MOST_LIGHTS];
        let shown = self.lights.len().min(MOST_LIGHTS);
        for (k, (light, w)) in self.lights.iter().zip(&light_worlds).enumerate() {
            let radiance = light.intensity.clamp(0.0, 1.0).powi(4) * 50.0;
            if let (Some(l), Some(r)) = (lights.get_mut(k), radiances.get_mut(k)) {
                *l = transform(view, *w);
                *r = [light.color[0] * radiance, light.color[1] * radiance, light.color[2] * radiance, 0.0];
            }
        }
        let f = 1.0 / (orbit.fov_y(aspect) / 2.0).tan();
        let uniform = ViewUniform {
            view_proj: orbit.view_proj(aspect),
            view,
            lights,
            radiances,
            sh: self.lighting.sh,
            params: [self.exposure, (CUBE_LEVELS - 1) as f32, width as f32, height as f32],
            projection: [f / aspect, f, 0.0, 0.0],
            convert: self.convert.map_or([[0.0; 4]; 3], |m| m.map(|r| [r[0], r[1], r[2], 1.0])),
            counts: [shown as f32, 0.0, 0.0, 0.0],
        };
        gpu.queue.write_buffer(&self.view_buffer, 0, bytemuck::bytes_of(&uniform));
        // the surround's grid where no environment shows
        let grid = match (self.backdrop.is_none(), self.grid) {
            (true, Some(g)) => {
                let format = self.format;
                let draw = self.grid_draw.get_or_insert_with(|| GridDraw::new(gpu, format));
                let srgb = if format.is_srgb() { 1.0 } else { 0.0 };
                let u = GridUniform {
                    v0: [g.spacing, g.origin[0], g.origin[1], g.width],
                    v1: [g.color[0], g.color[1], g.color[2], srgb],
                    v2: [clear[0] as f32, clear[1] as f32, clear[2] as f32, g.top],
                };
                gpu.queue.write_buffer(&draw.buffer, 0, bytemuck::bytes_of(&u));
                Some(&*draw)
            }
            _ => None,
        };
        // the studio's floor and lights, with the camera it is seen through
        let studio_draw = match studio {
            Some(o) => {
                let format = self.format;
                let mut cubes = [[0.0; 4]; MOST_LIGHTS];
                let mut colors = [[0.0; 4]; MOST_LIGHTS];
                for (k, (light, w)) in self.lights.iter().zip(&light_worlds).enumerate() {
                    if let (Some(c), Some(col)) = (cubes.get_mut(k), colors.get_mut(k)) {
                        *c = [w[0], w[1], w[2], CUBE_SIDE * self.unit];
                        let s = light.color.map(linear_to_srgb);
                        *col = [s[0], s[1], s[2], 1.0];
                    }
                }
                let u = StudioUniform {
                    view_proj: o.view_proj(aspect),
                    floor: [self.floor, FLOOR_RADIUS * self.unit, FLOOR_CELL * self.unit, 0.0],
                    cubes,
                    colors,
                    target: [if format.is_srgb() { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
                };
                let draw = self.studio_draw.get_or_insert_with(|| StudioDraw::new(gpu, format));
                gpu.queue.write_buffer(&draw.buffer, 0, bytemuck::bytes_of(&u));
                Some(&*draw)
            }
            None => None,
        };
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
        if let Some(draw) = grid {
            pass.set_pipeline(&draw.pipeline);
            pass.set_bind_group(0, &draw.bind, &[]);
            pass.draw(0..3, 0..1);
        }
        pass.set_bind_group(0, &self.view_bind, &[]);
        if let Some(backdrop) = &self.backdrop {
            pass.set_pipeline(backdrop);
            pass.draw(0..3, 0..1);
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        for (p, shown) in self.parts.iter().zip(&shown_by_part) {
            pass.set_bind_group(1, &p.bind, &[]);
            for range in shown {
                pass.draw_indexed(range.clone(), 0, 0..1);
            }
        }
        if let Some(draw) = studio_draw {
            pass.set_bind_group(0, &draw.bind, &[]);
            pass.set_pipeline(&draw.floor);
            pass.draw(0..6, 0..1);
            pass.set_pipeline(&draw.cubes);
            pass.draw(0..36, 0..shown as u32);
        }
    }
}

/// A part's bindings: its albedo, the sampler, its uniform, and its roughness and metalness maps.
fn part_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    views: &[wgpu::TextureView; 5],
    uniform: &wgpu::Buffer,
) -> wgpu::BindGroup {
    let [albedo, roughness, metallic, normal, occlusion] = views;
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mesh part"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(albedo) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(roughness) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(metallic) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(normal) },
            wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(occlusion) },
        ],
    })
}

/// A part's channel map at `binding`, sampled as its albedo is.
fn map_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// How far in front of the camera `at` is, along its view.
fn orbit_depth(orbit: &Orbit, at: [f32; 3]) -> f32 {
    let eye = orbit.eye();
    let t = orbit.target;
    let forward = [t[0] - eye[0], t[1] - eye[1], t[2] - eye[2]];
    let length = (forward[0] * forward[0] + forward[1] * forward[1] + forward[2] * forward[2]).sqrt().max(1e-9);
    ((at[0] - eye[0]) * forward[0] + (at[1] - eye[1]) * forward[1] + (at[2] - eye[2]) * forward[2]) / length
}

/// A linear-light value as sRGB.
fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
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
        // what's shown of each part, worked out before the projector is borrowed
        let shown_by_part: Vec<Vec<std::ops::Range<u32>>> =
            (0..self.parts.len()).map(|i| self.shown_ranges(i)).collect();
        let shown = |i: usize| shown_by_part.get(i).cloned().unwrap_or_default();
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
            for range in (0..self.parts.len()).flat_map(&shown) {
                pass.draw_indexed(range, 0, 0..1);
            }
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
            for range in shown(part) {
                pass.draw_indexed(range, 0, 0..1);
            }
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
