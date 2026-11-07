//! The canvas on the GPU: a texture per layer, the stroke being drawn, the composite, and the
//! undo history (pixels and document structure).
//!
//! Calls queue work; [`Canvas::render`] records and submits it in order, recomposites what
//! changed and, given a target, draws the canvas to it.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, mpsc};

use procraft_engine::animation::{self, FrameLook};
use procraft_engine::brush::{BRUSH_BEHIND, Brush, GRAIN_LINEAR_HEIGHT, GrainMode, Rendering, Source, run_out};
use procraft_engine::document::{BlendMode, Document, Layer, LayerId, LayerKind};
use procraft_engine::geom::{Affine, IRect};
use procraft_engine::stroke::{MOST_ROUNDNESS, Stamp};
use procraft_engine::textures::{self, GrayImage, TEXTURE_SIZE};

use wgpu::util::DeviceExt;

use crate::{Gpu, GpuError};

pub(crate) mod plugin_filter;

const LAYER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The most textures the canvas's size kept for reuse (see `Canvas::spares`).
const SPARES_KEPT: usize = 1;
/// How many pixels of a row or column each pixel of a layer's bounds lines stands for (as
/// `bounds.wgsl` has it).
const BOUNDS_STRETCH: u32 = 512;
/// Half floats, so a stroke's build-up from many faint stamps doesn't band.
const STROKE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Every draw gets its own slot in the uniform buffer (dynamic offsets are 256-byte aligned).
const SLOT: u64 = 256;
/// The Multiply blend mode's number.
const MULTIPLY_MODE: u32 = 1;
/// Set in a layer uniform's `stroke_blend` (over the blend mode's number): luminance blending.
const STROKE_LUMINANCE: u32 = 0x100;

/// The largest uniform block a shader reads.
const UNIFORM_SIZE: u64 = 128;
const _: () = assert!(
    size_of::<LayerUniform>() as u64 <= UNIFORM_SIZE
        && size_of::<StampUniform>() as u64 <= UNIFORM_SIZE
        && size_of::<FilterUniform>() as u64 <= UNIFORM_SIZE,
    "a uniform block outgrew its binding"
);

/// The vertex layout of a warp mesh corner: where it goes, and where it came from (pixels).
const MESH_ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];

/// The vertex layout of a stamp instance (stroke stamps and the predicted tail).
const STAMP_ATTRIBUTES: [wgpu::VertexAttribute; 12] = wgpu::vertex_attr_array![
    0 => Float32x2, 1 => Float32, 2 => Float32, 3 => Float32, 4 => Float32, 5 => Float32x2, 6 => Float32x2, 7 => Float32x4,
    8 => Float32x4, 9 => Float32x4, 10 => Float32, 11 => Float32
];

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct OverlayUniform {
    m0: [f32; 4],
    m1: [f32; 4],
    color: [f32; 4],
    shape: i32,
    srgb: u32,
    shape_lod_cap: f32,
    _pad: u32,
}
/// The most painting sent to the GPU as one piece of work, by [`stamp_work`]. A driver stops work
/// that runs too long and takes the device, and all it holds, with it: an Adreno 750 did at under
/// three seconds, which a long stroke's redrawing at its end took as one piece, and a slower GPU
/// is there with a much shorter stroke. This much is a thousandth of a second on that Adreno, so
/// a fair margin even at a hundredth of its speed. Much less costs more than it saves: a tiled
/// GPU can read and write the whole stroke for each piece.
const WORK_AT_ONCE: u64 = 64 << 20;
/// What a stamp counts for besides the pixels it covers; and a stamp painted in place, a compute
/// pass for each, for each pixel and for itself.
const STAMP_WORK: u64 = 1 << 10;
const IN_PLACE_PIXEL_WORK: u64 = 16;
const IN_PLACE_STAMP_WORK: u64 = 256 << 10;

/// Undo steps kept, and the most memory they may hold (their pixels and the selections they put
/// back).
const MAX_UNDO: usize = 100;
const MAX_UNDO_BYTES: u64 = 768 << 20;

/// How a transformed layer is resampled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resample {
    Nearest,
    Bilinear,
    Bicubic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeTool {
    Paint,
    Erase,
    /// Drags colour along: the layer itself changes as the stroke goes.
    Smudge,
    /// The stroke's pixels take the layer's place where they reach (their alpha marked by 1): what
    /// a change made elsewhere brings back to the layer. Drawn in, never stamped.
    Laid,
}

/// How a stroke is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeStyle {
    pub tool: StrokeTool,
    /// sRGB, 0..1.
    pub color: [f32; 3],
    /// The stroke's opacity: its stamps build up to at most this.
    pub opacity: f32,
    /// The round tip's hardness, 0..1.
    pub hardness: f32,
    /// The layer's alpha lock when the stroke began: paint only recolours existing pixels.
    pub preserve_alpha: bool,
    /// A shape image, or none for the round tip.
    pub shape: Option<u32>,
    /// A grain image, or none.
    pub grain: Option<u32>,
    pub grain_mode: GrainMode,
    /// One grain tile on the canvas, pixels.
    pub grain_scale: f32,
    pub grain_depth: f32,
    pub rendering: Rendering,
    /// Smudge: how much of the colour it carries the brush keeps at each stamp, 0..1.
    pub pull: f32,
    /// How much the stroke's paint pools at its edges (wet edges), and burns the paint it goes
    /// over where it is thin (burnt edges), 0..1.
    pub wet_edges: f32,
    pub burnt_edges: f32,
    /// Where the stroke's coverage turns fully on or off (0..1), or none if negative.
    pub alpha_threshold: f32,
    /// The grain made lighter or darker, and of more or less contrast, −1..1.
    pub grain_brightness: f32,
    pub grain_contrast: f32,
    /// Wet mix: how much of the colour under the brush mixes into its paint, 0..1; none paints
    /// plainly.
    pub wet_pull: Option<f32>,
    /// Wet paint mixes with the layer as it was before the stroke (a glaze), not with the stroke
    /// as it goes (a blending mode).
    pub wet_from_before: bool,
    /// Wet mix: how many stamps' worth of paint the brush holds; none never runs out.
    pub load: f32,
    /// Wet mix: water in the paint, 0..1: it goes on thinner.
    pub dilution: f32,
    /// The brush's fall off, 0..1: the paint it mixes in runs out along the stroke as its stamps
    /// fade.
    pub falloff: f32,
    /// Wet mix: how hard the paint goes on over what is there (less of it pulled in), its grade
    /// (how much of its mixture each stamp lays down, -1..1), how blurred what it lays down is,
    /// and how much the pull varies from stamp to stamp; each 0..1 but the grade.
    pub attack: f32,
    /// How far wet paint's stamps lay, by the opacity slider (see [`StrokeStyle::wetness_at`]);
    /// 1 at full.
    pub wetness: f32,
    pub grade: f32,
    pub blur: f32,
    pub wet_jitter: f32,
    /// How the stroke blends into its layer: a blend mode's number (0 is Normal, in the order of
    /// `BlendMode`).
    pub blend: u32,
    /// Wet mix: how much the blur varies from stamp to stamp, 0..1.
    pub blur_jitter: f32,
    /// How the grain combines with the shape, and the burnt edges with the stroke (blend modes'
    /// numbers; both multiply as they start).
    pub grain_blend: u32,
    pub burnt_blend: u32,
    /// The grain turned, radians.
    pub grain_rotation: f32,
    /// No grain filtering: the grain read pixel by pixel, sharp, as from a finer image.
    pub grain_sharp: bool,
    /// A brush file's grain to read in place of the brush images (see
    /// [`Canvas::use_full_grain`]), and pixels a side of the image the grain is read from.
    pub full_grain: Option<u64>,
    pub grain_side: f32,
    /// The brush file's shape it reads at its own size, if it does (see
    /// [`Canvas::use_full_shape`]).
    pub full_shape: Option<u64>,
    /// No shape filtering: the shape image read pixel by pixel, its edges and specks hard.
    pub shape_sharp: bool,
    /// The coarsest mip level a small stamp reads its shape from, so a thin shape keeps its
    /// strength however small the stamp.
    pub shape_lod_cap: f32,
    /// The secondary colour (sRGB, 0..1), mixed in as far as each stamp says.
    pub secondary: [f32; 3],
    /// Drawn as a brush preview (the studio's drawing pad, the library's strokes), which draws
    /// wet edges only as a firmer rim and burns nothing.
    pub preview: bool,
    /// Luminance blending: the stroke mixed into its layer by its coverage in squared colour
    /// (nearer to light), not as the colour's numbers are.
    pub luminance: bool,
    /// Dry paint laid in place, each pixel taking paint only as far as it has room left (see
    /// `Brush::laid_in_place`).
    pub laid_in_place: bool,
}

impl StrokeStyle {
    /// Whether the stroke changes its layer as it goes (smudging, painting wet) rather than
    /// building up in the stroke texture.
    pub fn in_place(&self) -> bool {
        self.tool == StrokeTool::Smudge || self.paints_in_place()
    }

    /// Whether a dual brush's second can go with the stroke: not with a smudge (paint laid in
    /// place, dry or wet, is put together with it, see `cs_synthesize`).
    pub fn takes_second(&self) -> bool {
        self.tool != StrokeTool::Smudge
    }

    /// The style as a dual brush's second: only coverage, drawn apart into its own texture, so
    /// never laid in place or mixed wet, whatever its own settings say.
    pub fn as_second(self) -> StrokeStyle {
        StrokeStyle { laid_in_place: false, wet_pull: None, wet_from_before: false, ..self }
    }

    /// The style without its wet mix: dry, as the brush would be with none set (a preview that
    /// shows it dry).
    pub fn dry(self) -> StrokeStyle {
        StrokeStyle {
            wet_pull: None,
            wet_from_before: false,
            load: 0.0,
            dilution: 0.0,
            attack: 0.0,
            grade: 0.0,
            blur: 0.0,
            wet_jitter: 0.0,
            blur_jitter: 0.0,
            ..self
        }
    }

    /// Whether the stroke paints its layer as it goes: wet paint, or dry paint laid in place.
    fn paints_in_place(&self) -> bool {
        self.tool == StrokeTool::Paint && (self.wet_pull.is_some() || self.laid_in_place)
    }

    /// A plain hard round stroke.
    pub fn new(tool: StrokeTool, color: [f32; 3], opacity: f32) -> StrokeStyle {
        StrokeStyle {
            tool,
            color,
            opacity,
            hardness: 1.0,
            preserve_alpha: false,
            shape: None,
            grain: None,
            grain_mode: GrainMode::Texturized,
            grain_scale: 256.0,
            grain_depth: 0.0,
            rendering: Rendering::BuildUp,
            pull: DEFAULT_PULL,
            wet_edges: 0.0,
            burnt_edges: 0.0,
            alpha_threshold: -1.0,
            grain_brightness: 0.0,
            grain_contrast: 0.0,
            wet_pull: None,
            wet_from_before: false,
            load: 0.0,
            dilution: 0.0,
            falloff: 0.0,
            attack: 0.0,
            wetness: 1.0,
            grade: 0.0,
            blur: 0.0,
            wet_jitter: 0.0,
            blend: 0,
            blur_jitter: 0.0,
            grain_blend: MULTIPLY_MODE,
            burnt_blend: MULTIPLY_MODE,
            grain_rotation: 0.0,
            grain_sharp: false,
            full_grain: None,
            grain_side: TEXTURE_SIZE as f32,
            full_shape: None,
            shape_sharp: false,
            shape_lod_cap: SHAPE_LOD_CAP,
            secondary: [1.0, 1.0, 1.0],
            preview: false,
            luminance: false,
            laid_in_place: false,
        }
    }

    fn brush_blend(value: f32) -> u32 {
        let id = value.round().clamp(0.0, BlendMode::MAX_ID as f32) as u32;
        if BlendMode::from_id(id).is_some() || id == BRUSH_BEHIND { id } else { 0 }
    }

    /// A stroke drawn with `brush`'s shape, grain and rendering.
    pub fn for_brush(
        brush: &Brush,
        tool: StrokeTool,
        color: [f32; 3],
        opacity: f32,
        preserve_alpha: bool,
    ) -> StrokeStyle {
        let wet = brush.mixes_wet();
        StrokeStyle {
            tool,
            color,
            opacity,
            hardness: brush.hardness,
            preserve_alpha,
            shape: brush.shape.image,
            grain: brush.grain.image.filter(|_| brush.grain.depth > 0.0),
            grain_mode: brush.grain.mode,
            grain_scale: brush.grain_tile().max(1.0),
            grain_depth: brush.grain.depth.clamp(0.0, 1.0),
            rendering: brush.rendering,
            pull: DEFAULT_PULL,
            wet_edges: brush.setting("wet_edges").clamp(0.0, 1.0),
            burnt_edges: brush.setting("burnt_edges").clamp(0.0, 1.0),
            alpha_threshold: if brush.setting("alpha_threshold") >= 0.5 {
                brush.setting("alpha_threshold_amount").clamp(0.0, 1.0)
            } else {
                -1.0
            },
            grain_brightness: brush.setting("grain_brightness").clamp(-1.0, 1.0),
            grain_contrast: brush.setting("grain_contrast").clamp(-1.0, 1.0),
            // wet mix once any of its settings is set
            wet_pull: wet.then(|| brush.setting("wet_pull").clamp(0.0, 1.0)),
            wet_from_before: wet && !brush.mixes_with_stroke(),
            load: if wet { brush.setting("wet_charge").clamp(0.0, 1.0) } else { 0.0 },
            dilution: if wet { brush.setting("wet_dilution").clamp(0.0, 1.0) } else { 0.0 },
            falloff: brush.setting("falloff").clamp(0.0, 1.0),
            attack: if wet { brush.setting("wet_attack").clamp(0.0, 1.0) } else { 0.0 },
            wetness: 1.0,
            // the grade goes both ways (below 0 the stamps lay down less than their mixture)
            grade: if wet { brush.setting("wet_grade").clamp(-1.0, 1.0) } else { 0.0 },
            blur: if wet { brush.setting("wet_blur").clamp(0.0, 1.0) } else { 0.0 },
            wet_jitter: if wet { brush.setting("wet_jitter").clamp(0.0, 1.0) } else { 0.0 },
            blend: Self::brush_blend(brush.setting("blend_mode")),
            blur_jitter: if wet { brush.setting("wet_blur_jitter").clamp(0.0, 1.0) } else { 0.0 },
            grain_blend: brush.setting("grain_blend").round().clamp(0.0, GRAIN_LINEAR_HEIGHT as f32) as u32,
            burnt_blend: Self::brush_blend(brush.setting("burnt_blend")),
            // a brush's rotation turns its grain the other way to the shaders' angle
            // the editors' turn of the image, the whole grain with it (the rotation control is
            // each stamp's own: `Stamp::grain_turn`)
            grain_rotation: brush.setting("grain_turn").clamp(-1.0, 1.0) * std::f32::consts::PI,
            grain_sharp: brush.setting("grain_filtering") < 0.5,
            full_grain: None,
            grain_side: TEXTURE_SIZE as f32,
            full_shape: None,
            shape_sharp: brush.setting("shape_filtering") < 0.5,
            shape_lod_cap: if brush.setting("shape_filtering").round() == 1.0 {
                SHAPE_LOD_CAP_CLASSIC
            } else {
                SHAPE_LOD_CAP
            },
            secondary: [1.0, 1.0, 1.0],
            preview: false,
            luminance: brush.setting("luminance_blending") >= 0.5,
            laid_in_place: tool == StrokeTool::Paint && brush.laid_in_place(),
        }
    }

    /// How far wet paint's stamps lay at the opacity slider's `opacity`.
    pub fn wetness_at(opacity: f32) -> f32 {
        1.0 - (1.0 - opacity.clamp(0.0, 1.0)).powf(2.2)
    }

    /// The grain applied to each stamp (moving), as (layer, scale, depth).
    fn moving_grain(&self) -> Option<(u32, f32, f32)> {
        self.grain.filter(|_| self.grain_mode == GrainMode::Moving).map(|g| (g, self.grain_scale, self.grain_depth))
    }

    /// The grain applied to the whole stroke, fixed to the canvas (texturized).
    fn canvas_grain(&self) -> Option<(u32, f32, f32)> {
        self.grain.filter(|_| self.grain_mode == GrainMode::Texturized).map(|g| (g, self.grain_scale, self.grain_depth))
    }
}

/// Where [`Canvas::render`] draws the canvas.
pub struct Target<'a> {
    pub view: &'a wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    /// Target size in pixels.
    pub size: (u32, u32),
    /// Canvas pixels to target pixels.
    pub to_screen: Affine,
    /// Colour around the canvas, sRGB 0..1.
    pub surround: [f32; 3],
    /// Lines over that colour, if any.
    pub grid: Option<SurroundGrid>,
    /// How the selection shows, while there is one.
    pub selection: Option<SelectionLook>,
    /// Frosted panels over the target, if any.
    pub backdrop: Option<Backdrop<'a>>,
    /// The canvas's colours to the target's colour space: a matrix in linear light (none when
    /// they are the same space).
    pub convert: Option<[[f32; 3]; 3]>,
    /// Or a table taking the canvas's colours to the target's (made by [`Canvas::display_lut`]),
    /// in place of the matrix.
    pub lut: Option<&'a DisplayLut>,
    /// Zoomed in to this many target pixels to a canvas pixel or more, each canvas pixel shows as
    /// a square of its own colour; less, smoothed between them. The screen's pixels per point
    /// makes them crisp from 100%; infinity keeps them smoothed.
    pub crisp_from: f32,
    /// The canvas shown in perceptual greys (each colour as the grey of its Rec. 709 luminance, as
    /// a soft proof to a grey profile shows it), to judge its values: the window's view only, never
    /// what is read or recorded from it.
    pub grey: bool,
}

/// A colour table on the GPU for [`Target::lut`].
pub struct DisplayLut {
    bind: wgpu::BindGroup,
}

/// Panels of frosted glass over the target: what is behind each region is blurred (Gaussian,
/// `sigma` target pixels) and drawn in its place, for a translucent panel to be drawn over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Backdrop<'a> {
    /// Rounded rectangles in target pixels: left, top, right, bottom, corner radius, and how much
    /// the region shows (0..1, as its panel fades).
    pub regions: &'a [[f32; 6]],
    /// Regions likewise, drawn over those, where the glass is thin: what is behind only softens
    /// (about a tenth of `sigma`).
    pub windows: &'a [[f32; 6]],
    pub sigma: f32,
}

/// Draws a scene for [`Canvas::frost_over`] into the view given (cleared first), at the size
/// given.
pub type SmallScene<'s> = &'s mut dyn FnMut(&mut wgpu::CommandEncoder, &wgpu::TextureView, (u32, u32));

/// The backdrop's blur is this many texels wide (sigma) where it is worked out: the scene is
/// drawn that much smaller, and drawn back up as it is that's a window's softening.
const BACKDROP_TEXELS: f32 = 5.0;

/// The scene drawn small for frosted panels, half blurred, and blurred.
struct BackdropGpu {
    size: (u32, u32),
    format: wgpu::TextureFormat,
    views: [wgpu::TextureView; 3],
    binds: [wgpu::BindGroup; 3],
}

impl BackdropGpu {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> BackdropGpu {
        let view = || {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("backdrop"),
                    size: extent(size.0, size.1),
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let views = [view(), view(), view()];
        let binds = views.each_ref().map(|v| sampled_bind(device, layout, v, sampler));
        BackdropGpu { size, format, views, binds }
    }
}

/// The glass [`Canvas::frost_ready`] worked out, `k` times smaller than the target of `size`, drawn
/// into `pass`: blurred into each region, and the small scene as it is into the windows.
#[allow(clippy::too_many_arguments)]
fn draw_glass(
    pass: &mut wgpu::RenderPass<'_>,
    frost: &wgpu::RenderPipeline,
    small: &BackdropGpu,
    uniform_bind: &wgpu::BindGroup,
    uniforms: &mut UniformData,
    backdrop: Backdrop<'_>,
    size: (u32, u32),
    k: f32,
) {
    let (tw, th) = (size.0.max(1) as f32, size.1.max(1) as f32);
    pass.set_pipeline(frost);
    for (bind, regions) in [(&small.binds[2], backdrop.regions), (&small.binds[0], backdrop.windows)] {
        pass.set_bind_group(0, bind, &[]);
        for &[x0, y0, x1, y1, corner, alpha] in regions {
            let usable = [x0, y0, x1, y1, corner, alpha].iter().all(|v| v.is_finite());
            if !usable || x1 <= x0 || y1 <= y0 || alpha <= 0.0 {
                continue;
            }
            let offset = uniforms.push(&BackdropUniform {
                v0: [x0, y0, x1, y1],
                v1: [corner, tw, th, k],
                v2: [alpha.min(1.0), 0.0, 0.0, 0.0],
            });
            pass.set_bind_group(1, uniform_bind, &[offset]);
            pass.draw(0..6, 0..1);
        }
    }
}

/// How the selection shows over the canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelectionLook {
    /// Bands over what is not selected.
    Stripes(Stripes),
    /// What is selected is multiplied by a colour (sRGB).
    Tint([f32; 3]),
    /// What is selected is covered by a colour (sRGB).
    Fill([f32; 3]),
}

/// Bands over the canvas outside the selection, dark and light by turns: one pair every `period`
/// target pixels (measured across), shifted `drift` pixels, at `coverage`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stripes {
    pub period: f32,
    pub drift: f32,
    pub coverage: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SelectionUniform {
    m0: [f32; 4],
    m1: [f32; 4],
    v: [f32; 4],
    mode: [f32; 4],
}

/// The selection on the GPU: its coverage, and that bound for drawing the stripes.
struct SelectionGpu {
    coverage: Vec<u8>,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

/// Grid lines around the canvas, fixed to the target: `width` pixels wide (their edges smoothed),
/// every `spacing` pixels across and down from one whose middle is at `origin`, none above `top`,
/// in `color` (sRGB 0..1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurroundGrid {
    pub spacing: f32,
    pub origin: [f32; 2],
    pub top: f32,
    pub width: f32,
    pub color: [f32; 3],
}

/// A small premultiplied RGBA8 image (rows top to bottom).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct StampInstance {
    center: [f32; 2],
    radius: f32,
    alpha: f32,
    angle: f32,
    roundness: f32,
    grain_offset: [f32; 2],
    /// −1 where the shape is mirrored on that axis, else 1.
    flip: [f32; 2],
    /// The tint (see [`stamp_extras`]): the hue's turn, the saturation's keep and add, the
    /// brightness's keep.
    tint: [f32; 4],
    /// A moving grain's depth and tile size as multiples of the brush's, the share of the
    /// secondary colour, and how far it fades across its width (tilt gradation).
    look: [f32; 4],
    /// How hard the paint is pressed into a moving grain and how much that counts, and room for
    /// more.
    more: [f32; 4],
    /// The tint's brightness add.
    brightness_add: f32,
    /// How far the grain's patch turns (see `Stamp::grain_turn`).
    grain_turn: f32,
}

/// How much work stamp `s` is on a canvas `full`: the pixels it covers there, more for one
/// painted `in_place` (see [`WORK_AT_ONCE`]).
fn stamp_work(s: &StampInstance, full: IRect, in_place: bool) -> u64 {
    let at = procraft_engine::geom::Vec2::new(s.center[0], s.center[1]);
    let reach = IRect::around(at, s.radius * s.roundness.max(1.0) * std::f32::consts::SQRT_2).intersect(full);
    let pixels = u64::from(reach.width()) * u64::from(reach.height());
    if in_place { pixels * IN_PLACE_PIXEL_WORK + IN_PLACE_STAMP_WORK } else { pixels + STAMP_WORK }
}

/// A stamp's (alpha, transfer) as its instance takes them: a capped stroke keeps its transfer
/// apart for the capped pipeline, or (without one) takes it into the alpha.
fn capped_alpha(style: &StrokeStyle, capped: bool, s: &Stamp) -> (f32, f32) {
    let alpha = s.alpha.clamp(0.0, 1.0);
    let transfer = if s.transfer.is_finite() { s.transfer.clamp(0.0, 1.0) } else { 1.0 };
    match (style.rendering, capped) {
        (Rendering::Capped, true) => (alpha, transfer),
        (Rendering::Capped, false) => (alpha * transfer, 1.0),
        _ => (alpha, 1.0),
    }
}

/// How far past its size a round tip reaches, by its hardness: (scale, growth) for its drawn
/// radius, scale·r + growth. Measured for a hard tip (0.95: a stroke of diameter d is 0.971·d +
/// 1.87 px across) and a medium hard one (0.75: 1.043·d + 1.44 px), whose soft rim builds up
/// past the size along a stroke; between them in between, and no further either way.
fn round_tip_reach(hardness: f32) -> (f32, f32) {
    let t = if hardness.is_finite() { ((hardness - 0.75) / 0.2).clamp(0.0, 1.0) } else { 1.0 };
    (1.096 + (0.971 - 1.096) * t, 0.36 + (0.735 - 0.36) * t)
}

/// The radius a stamp of `radius` is drawn at in `style`, thinned `taper` of the way by a taper
/// (the round tip's rim with it: measured, a taper narrows a thin brush and a wide one to the same
/// shares of their widths).
fn drawn_radius(style: &StrokeStyle, radius: f32, taper: f32) -> f32 {
    let r = radius.max(0.0);
    if style.shape.is_some() || r <= 0.0 {
        return r;
    }
    let (scale, growth) = round_tip_reach(style.hardness);
    let taper = if taper.is_finite() { taper.clamp(0.0, 1.0) } else { 1.0 };
    r * scale + growth * taper
}

/// A stamp's mirroring and colour shift for the GPU, kept finite.
/// A stamp's flips (each 1 or -1), and its tint as the shader takes it: the hue's turn, the
/// saturation's keep and add, the brightness's keep, then apart its add.
fn stamp_extras(s: &Stamp) -> ([f32; 2], [f32; 4], f32) {
    let flip = s.flip.map(|f| if f { -1.0 } else { 1.0 });
    let t = s.tint;
    let ok = |v: f32, none: f32| if v.is_finite() { v.clamp(-4.0, 4.0) } else { none };
    let [sk, sa] = t.saturation;
    let [bk, ba] = t.brightness;
    (flip, [ok(t.hue, 0.0), ok(sk, 1.0), ok(sa, 0.0), ok(bk, 1.0)], ok(ba, 0.0))
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct StampUniform {
    canvas_size: [f32; 2],
    hardness: f32,
    shape: i32,
    color: [f32; 4],
    grain: i32,
    grain_scale: f32,
    grain_depth: f32,
    grain_brightness: f32,
    grain_contrast: f32,
    grain_rotation: f32,
    /// How the grain combines with the shape (a blend mode's number).
    grain_blend: u32,
    /// 1 when the grain is read unfiltered (sharp).
    grain_sharp: u32,
    /// The secondary colour (straight sRGB), which stamps mix in as far as they say.
    secondary: [f32; 4],
    /// 1 when the shape is read unfiltered (sharp).
    shape_sharp: u32,
    /// The coarsest mip level the shape is read from.
    shape_lod_cap: f32,
    /// Pixels a side of the image a moving grain is read from.
    grain_side: f32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LayerUniform {
    opacity: f32,
    stroke_mode: u32,
    stroke_opacity: f32,
    blend: u32,
    clipped: u32,
    grain: i32,
    grain_scale: f32,
    grain_depth: f32,
    solid: [f32; 4],
    wet: f32,
    burnt: f32,
    threshold: f32,
    grain_brightness: f32,
    grain_contrast: f32,
    /// 1 when the layer shows through its mask, and when the stroke being drawn is on the mask.
    masked: f32,
    stroke_on_mask: f32,
    /// How the stroke blends into the layer (a blend mode's number).
    stroke_blend: u32,
    /// An onion skin's colour, screened over the layer, and how much (none at 0).
    tint: [f32; 4],
    /// The stroke's texturized grain turned, radians, and how it and the burnt edges combine
    /// with the stroke (blend modes' numbers).
    grain_rotation: f32,
    grain_blend: u32,
    burnt_blend: u32,
    /// 1 when the grain is read unfiltered (sharp).
    grain_sharp: f32,
    /// A dual brush's second stroke: how its coverage combines with the first's (a blend mode's
    /// number + 1; 0: no second), and where it is cut first (0..1; negative: not cut).
    dual: u32,
    dual_threshold: f32,
    /// 1 when the clip base shows through its own mask, which clipping then follows.
    clip_masked: f32,
    /// Pixels a side of the image the grain is read from.
    grain_side: f32,
}

impl Default for LayerUniform {
    fn default() -> LayerUniform {
        LayerUniform {
            opacity: 1.0,
            stroke_mode: STROKE_NONE,
            stroke_opacity: 0.0,
            blend: 0,
            clipped: 0,
            grain: -1,
            grain_scale: 1.0,
            grain_depth: 0.0,
            solid: [0.0; 4],
            wet: 0.0,
            burnt: 0.0,
            threshold: -1.0,
            grain_brightness: 0.0,
            grain_contrast: 0.0,
            masked: 0.0,
            stroke_on_mask: 0.0,
            stroke_blend: 0,
            tint: [0.0; 4],
            grain_rotation: 0.0,
            grain_blend: MULTIPLY_MODE,
            burnt_blend: MULTIPLY_MODE,
            grain_sharp: 0.0,
            dual: 0,
            dual_threshold: -1.0,
            clip_masked: 0.0,
            grain_side: TEXTURE_SIZE as f32,
        }
    }
}

/// How a layer shows besides its own settings: Animation Assist's onion skins.
#[derive(Clone, Copy)]
struct Look {
    tint: [f32; 4],
    opacity: f32,
}

impl Look {
    const PLAIN: Look = Look { tint: [0.0; 4], opacity: 1.0 };
}

impl LayerUniform {
    /// With the stroke's second, if it has one (a dual brush's).
    fn with_dual(self, stroke: Option<&ActiveStroke>) -> LayerUniform {
        match stroke.and_then(|s| s.second) {
            Some(d) => LayerUniform { dual: d.mode + 1, dual_threshold: d.threshold, ..self },
            None => self,
        }
    }

    /// With the stroke's look: its canvas grain, if it has one, and its edges and threshold.
    fn with_grain(self, style: Option<&StrokeStyle>) -> LayerUniform {
        let Some(s) = style else { return self };
        let look = LayerUniform {
            wet: if s.preview { -s.wet_edges } else { s.wet_edges },
            burnt: if s.preview { 0.0 } else { s.burnt_edges },
            threshold: s.alpha_threshold,
            grain_brightness: s.grain_brightness,
            grain_contrast: s.grain_contrast,
            stroke_blend: s.blend | if s.luminance { STROKE_LUMINANCE } else { 0 },
            grain_rotation: s.grain_rotation,
            grain_blend: s.grain_blend,
            burnt_blend: s.burnt_blend,
            grain_sharp: if s.grain_sharp { 1.0 } else { 0.0 },
            grain_side: s.grain_side,
            ..self
        };
        match s.canvas_grain() {
            Some((g, scale, depth)) => LayerUniform { grain: g as i32, grain_scale: scale, grain_depth: depth, ..look },
            None => look,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DisplayUniform {
    m0: [f32; 4],
    m1: [f32; 4],
    /// A colour matrix's rows (linear light), the first's last number 1 to apply it.
    c0: [f32; 4],
    c1: [f32; 4],
    c2: [f32; 4],
}

impl DisplayUniform {
    /// The map `m` and target encoding `srgb`, colours moved by `convert` if there is one, or by
    /// the bound table if `lut`.
    fn new(m0: [f32; 4], m1: [f32; 4], convert: Option<[[f32; 3]; 3]>, lut: bool) -> DisplayUniform {
        let row = |r: [f32; 3], on: f32| [r[0], r[1], r[2], on];
        let table = if lut { 1.0 } else { 0.0 };
        match convert.filter(|_| !lut) {
            Some([r0, r1, r2]) => DisplayUniform { m0, m1, c0: row(r0, 1.0), c1: row(r1, 0.0), c2: row(r2, 0.0) },
            None => DisplayUniform { m0, m1, c0: [0.0; 4], c1: [0.0, 0.0, 0.0, table], c2: [0.0; 4] },
        }
    }
}

/// An encoded colour moved by a linear-light matrix (clipped), as the display shader does.
fn convert_colour(c: [f32; 3], m: Option<[[f32; 3]; 3]>) -> [f32; 3] {
    let Some(m) = m else { return c };
    use procraft_engine::profile::{apply, decode, encode};
    apply(m, c.map(decode)).map(|v| encode(v.clamp(0.0, 1.0)))
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BackdropUniform {
    v0: [f32; 4],
    v1: [f32; 4],
    v2: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TransformUniform {
    r0: [f32; 4],
    r1: [f32; 4],
    r2: [f32; 4],
    p: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SurroundUniform {
    v0: [f32; 4],
    v1: [f32; 4],
    v2: [f32; 4],
}

const STROKE_NONE: u32 = 0;
const STROKE_PAINT: u32 = 1;
const STROKE_ERASE: u32 = 2;
const STROKE_PAINT_LOCKED: u32 = 3;
const STROKE_LAID: u32 = 4;

/// How much of what it carries a smudge brush keeps at each stamp.
const DEFAULT_PULL: f32 = 0.93;
/// A number in 0..1 that looks random, from `n`.
fn unit_hash(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x & 0xffff) as f32 / 65535.0
}

/// What a smudge brush carries: a grid fixed to the canvas's axes, this many cells on a side, a
/// canvas pixel a cell unless the stroke's first stamp is wider than it (see the shader).
const SMUDGE_GRID: u32 = 256;
/// The most cells a stamp reaches from the grid's middle.
const SMUDGE_REACH: i32 = SMUDGE_GRID as i32 / 2 - 1;
const SMUDGE_UNIFORM_SIZE: u64 = std::mem::size_of::<SmudgeUniform>() as u64;

/// A wet stamp's (mixture, pull) once the stroke has gone `length` canvas pixels: the fresh paint
/// it mixes into what it carries is its charge (running out by the cube root of the length) and
/// what its dilution leaves ((1 - dilution)^2.4, as far as the fall off has not run it out), at
/// least its attack (the stamp's `attack` share of it: its pressure), times `jitter`; the pull is
/// how much of what it carries the brush keeps at each stamp (the brush's pull to the power 1/8).
/// The stamp's pressure scales the dilution and the charge by `dilution_by` and `charge_by` (their
/// curves).
fn wet_mixture(
    style: &StrokeStyle,
    length: f32,
    jitter: f32,
    attack: f32,
    dilution_by: f32,
    charge_by: f32,
) -> (f32, f32) {
    let charge = if style.load > 0.0 { 1.0 - run_out(1.0 - style.load, length) } else { 0.0 } * charge_by;
    let diluted =
        (1.0 - run_out(style.falloff, length)) * (1.0 - style.dilution.clamp(0.0, 1.0) * dilution_by).powf(2.4);
    let attack = style.attack.clamp(0.0, 1.0) * attack.clamp(0.0, 1.0);
    let mixture = ((charge + diluted).max(attack) * jitter).clamp(0.0, 1.0);
    (mixture, style.wet_pull.unwrap_or(0.0).clamp(0.0, 1.0).powf(0.125))
}

/// How much of what a wet stamp carries it lays over what was under its pass's first stamp: all
/// of it when it neither picks up nor mixes (a plain smudge), its mixture on the stroke's first
/// pass, else its mixture ramping up through the pass (stamp `k` of `n`) from the grade's share.
fn final_mixture(mixture: f32, pull: f32, grade: f32, (k, n): (usize, usize), first: bool) -> f32 {
    if pull * mixture <= 0.0 {
        1.0
    } else if first || n == 0 {
        mixture
    } else {
        (mixture * (grade + (1.0 - grade) * k as f32 / n as f32)).clamp(0.0, 1.0)
    }
}

/// Where a stamp comes in its stroke: when its input came (ms) and how far along the path it is
/// (canvas pixels).
#[derive(Clone, Copy)]
struct Paced {
    time_ms: f64,
    distance: f32,
}

/// The display frame wet paint input at `time_ms` is laid in.
fn wet_frame(time_ms: f64) -> i64 {
    (time_ms / WET_FRAME_MS).floor() as i64
}

/// A smudge stroke's buffers bound for its compute passes.
fn smudge_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    (params, wet): (&wgpu::Buffer, &wgpu::Buffer),
    spare: &[wgpu::Buffer; 2],
    smudge: &Smudge,
    second: Option<&wgpu::TextureView>,
) -> wgpu::BindGroup {
    let uniform = |binding, buffer, size| wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer, offset: 0, size: NonZeroU64::new(size) }),
    };
    let before = smudge.before.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("smudge"),
        layout,
        entries: &[
            uniform(0, params, SMUDGE_UNIFORM_SIZE),
            wgpu::BindGroupEntry { binding: 1, resource: smudge.work.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: smudge.carry.as_entire_binding() },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: smudge.grained.as_ref().unwrap_or(&spare[0]).as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: smudge.amount.as_ref().unwrap_or(&spare[1]).as_entire_binding(),
            },
            uniform(5, wet, WET_PASS_SIZE),
            // a dual brush's second (else anything: not read)
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(second.unwrap_or(&before)),
            },
            wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&before) },
        ],
    })
}

/// The colour `c` tinted (see [`stamp_extras`]), as the stamp shader does it.
fn tinted(c: [f32; 3], [turn, keep_s, add_s, keep_b]: [f32; 4], add_b: f32) -> [f32; 3] {
    if [turn, keep_s, add_s, keep_b, add_b] == [0.0, 1.0, 0.0, 1.0, 0.0] {
        return c;
    }
    let [r, g, b] = c;
    let high = r.max(g).max(b);
    let range = high - r.min(g).min(b);
    let (hue, saturation) = if range <= 1e-5 {
        (0.0, 0.0)
    } else if high == r {
        ((g - b) / range, range / high)
    } else if high == g {
        ((b - r) / range + 2.0, range / high)
    } else {
        ((r - g) / range + 4.0, range / high)
    };
    let hue = (hue / 6.0 + turn).rem_euclid(1.0);
    let saturation = (saturation * keep_s + add_s).clamp(0.0, 1.0);
    let brightness = (high * keep_b + add_b).clamp(0.0, 1.0);
    [5.0, 3.0, 1.0].map(|n: f32| {
        let k = (hue + n / 6.0).rem_euclid(1.0) * 6.0;
        brightness - brightness * saturation * k.min(4.0 - k).clamp(0.0, 1.0)
    })
}

/// What the stamp `s` paints with: the stroke's colour as it tints it, mixed towards the secondary
/// colour as far as it says.
fn stamp_paint(s: &StampInstance, style: &StrokeStyle) -> [f32; 3] {
    let [r, g, b] = tinted(style.color, s.tint, s.brightness_add);
    let [other_r, other_g, other_b] = style.secondary;
    let towards = |from: f32, to: f32| from + (to - from) * s.look[2];
    [towards(r, other_r), towards(g, other_g), towards(b, other_b)]
}

/// A smudge stamp's uniform: what every pass reads of the stamp `s` and the stroke's `style`
/// (the rest as the pass sets it).
fn smudge_uniform(
    s: &StampInstance,
    style: &StrokeStyle,
    bbox: IRect,
    (width, height, row): (u32, u32, u32),
) -> SmudgeUniform {
    let moving = style.grain_mode == GrainMode::Moving;
    SmudgeUniform {
        center: s.center,
        radius: s.radius.max(0.5),
        angle: s.angle,
        roundness: s.roundness,
        alpha: (s.alpha * style.opacity).clamp(0.0, 1.0),
        pull: 1.0,
        hardness: style.hardness.clamp(0.0, 1.0),
        shape: style.shape.map_or(-1, |s| s as i32),
        kept: -1,
        preserve_alpha: u32::from(style.preserve_alpha),
        stride: row / 4,
        bbox: [bbox.x0, bbox.y0, bbox.x1, bbox.y1],
        size: [width, height],
        grid: SMUDGE_GRID,
        uniform_glaze: u32::from(style.rendering == Rendering::Strongest),
        paint: [style.color[0], style.color[1], style.color[2], 1.0],
        mode: 0,
        mixture: 0.0,
        final_mixture: 1.0,
        grade: style.grade,
        blur: 0.0,
        grain: style.grain.map_or(-1, |g| g as i32),
        // a moving grain sized and as deep as this stamp says
        grain_scale: if moving { -(style.grain_scale * s.look[1]).max(1.0) } else { style.grain_scale },
        grain_depth: if moving { (style.grain_depth * s.look[0]).clamp(0.0, 1.0) } else { style.grain_depth },
        // only the patch turns by the stamp's own turn, where it is read from staying put: the
        // wet shader turns all it reads, so that is turned back first
        grain_offset: {
            let (sin, cos) = s.grain_turn.sin_cos();
            let [x, y] = s.grain_offset;
            [x * cos + y * sin, y * cos - x * sin]
        },
        grain_rotation: style.grain_rotation + s.grain_turn,
        grain_sharp: if style.grain_sharp { 1.0 } else { 0.0 },
        shape_sharp: if style.shape_sharp { 1.0 } else { 0.0 },
        shape_lod_cap: style.shape_lod_cap,
        grain_press: s.more[0],
        grain_bleed: s.more[1],
        grain_once: 0.0,
        anchor: s.center,
        scale: 1.0,
        half: 1,
        count: 0,
        plot: 0,
        index: 0,
        carried: 0,
        modulated: 0,
        grain_blend: style.grain_blend,
        dual: 0,
        dual_threshold: -1.0,
        laid_apart: 0,
        source_before: u32::from(style.wet_from_before),
        grain_brightness: style.grain_brightness,
        grain_contrast: style.grain_contrast,
        wet_edges: if style.preview { 0.0 } else { style.wet_edges },
        luminance: match (style.luminance && style.wet_pull.is_some(), style.wet_from_before) {
            (false, _) => 0,
            (true, false) => 1,
            (true, true) => 2,
        },
        grain_side: style.grain_side,
        burnt: if style.preview { 0.0 } else { style.burnt_edges },
        burnt_blend: style.burnt_blend,
        flip: s.flip,
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SmudgeUniform {
    center: [f32; 2],
    radius: f32,
    angle: f32,
    roundness: f32,
    alpha: f32,
    pull: f32,
    hardness: f32,
    shape: i32,
    /// The cells carried over from the stamp before (those within this many of the middle; -1
    /// at the stroke's first stamp).
    kept: i32,
    preserve_alpha: u32,
    stride: u32,
    bbox: [i32; 4],
    size: [u32; 2],
    grid: u32,
    /// 1 when stamps don't build up past the strongest over a pixel (the uniform glaze).
    uniform_glaze: u32,
    /// Wet mix: the paint (straight sRGB), how much of it the brush mixes into what it carries at
    /// this stamp, and how much of what it carries it lays down.
    paint: [f32; 4],
    /// 0 smudges; 1 paints wet.
    mode: u32,
    mixture: f32,
    final_mixture: f32,
    grade: f32,
    blur: f32,
    /// The grain: a layer (-1 for none), a tile's size in pixels (negative when it moves with the
    /// stamp) and its depth.
    grain: i32,
    grain_scale: f32,
    grain_depth: f32,
    /// Where a moving grain is read from at the stamp's middle (see `Stamp::grain_offset`), and
    /// the grain turned (radians), as the stamp shader takes them.
    grain_offset: [f32; 2],
    grain_rotation: f32,
    /// 1 when the grain is read unfiltered (sharp).
    grain_sharp: f32,
    /// 1 when the shape is read unfiltered (sharp).
    shape_sharp: f32,
    /// How hard the paint is pressed into a moving grain and how much that counts (see
    /// `Stamp::grain_press`).
    grain_press: f32,
    grain_bleed: f32,
    /// 1 when the texturized grain is laid over the wet stroke once (see `grain_once`).
    grain_once: f32,
    /// Where the grid's middle cell is (a whole pixel when a cell is one), canvas pixels a cell,
    /// and the cells this stamp reaches from the middle.
    anchor: [f32; 2],
    scale: f32,
    half: i32,
    /// Wet paint (see `cs_wet_plots`): the stamps in the pass, their plots' texels a side, this
    /// stamp's place in the pass, and the texels a side of the plot the pass starts from.
    count: u32,
    plot: u32,
    index: u32,
    carried: u32,
    /// 1 when wet paint builds up by what is left at each pixel (the modulated mode).
    modulated: u32,
    /// How the moving grain combines with the shape (a blend mode), for paint laid in place.
    grain_blend: u32,
    /// A dual brush's paint laid in place (see `cs_synthesize`): its dual mode + 1 (0: none) and
    /// its second's threshold (negative: none).
    dual: u32,
    dual_threshold: f32,
    /// 1 when the stamp only adds to the paint laid so far, which `cs_synthesize` lays down; 2
    /// when it is wet paint, which keeps what it lays as well.
    laid_apart: u32,
    /// The coarsest mip level the shape is read from.
    shape_lod_cap: f32,
    /// 1 when wet paint mixes with the layer as it was before the stroke (a glaze).
    source_before: u32,
    /// The grain's levels, as the stamp shader takes them.
    grain_brightness: f32,
    grain_contrast: f32,
    /// How far paint laid in place pools, as `cs_synthesize` lays it.
    wet_edges: f32,
    /// Wet paint with luminance blending: 1 when its colours mix squared.
    luminance: u32,
    /// Pixels a side of the image the grain is read from.
    grain_side: f32,
    /// How far paint laid apart burns the paint it goes over, and by what blend mode.
    burnt: f32,
    burnt_blend: u32,
    /// −1 where the stamp's shape is mirrored on that axis, else 1.
    flip: [f32; 2],
}

/// The coarsest mip level of our brush images (256 texels a side) a small stamp reads its shape
/// from: 8 texels a side, so that a thin tip keeps its strength.
const SHAPE_LOD_CAP: f32 = 5.0;

/// As [`SHAPE_LOD_CAP`], for classic filtering (a filtering setting of 1): 16 texels a side.
const SHAPE_LOD_CAP_CLASSIC: f32 = 4.0;

/// How far a stamp's quad reaches past the stamp each way (pixels): its shape fills all of it.
const QUAD_REACH: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Wet paint is laid a display frame's stamps at a time, the frames this long (ms).
const WET_FRAME_MS: f64 = 1000.0 / 60.0;
/// The most stamps one pass of wet paint takes (a frame with more is laid in several).
const WET_PASS_MOST: usize = 64;
/// A wet stamp's plot's most texels a side, and the texels the plots of one pass share between
/// them (a pass of many stamps takes smaller plots).
const WET_PLOT_MOST: u32 = 128;
const WET_PLOTS_TEXELS: u64 = 1024 * 1024;

/// The stamps of one pass of wet paint (see `cs_wet_plots`), each as its middle, radius and turn,
/// then its roundness, pull, mixture and final mixture, then what it paints with.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct WetPass {
    stamps: [[f32; 12]; WET_PASS_MOST],
}
const WET_PASS_SIZE: u64 = std::mem::size_of::<WetPass>() as u64;

/// A smudge stroke in progress: the layer as a buffer the compute passes read and write, what the
/// brush carries, and the layer as it was before the stroke (for undo and cancel).
struct Smudge {
    layer: LayerId,
    work: wgpu::Buffer,
    carry: wgpu::Buffer,
    before: wgpu::Texture,
    started: bool,
    /// Canvas pixels a cell of what the brush carries (see `SMUDGE_GRID`), and the cells the stamp
    /// before reached (-1 before the first).
    scale: f32,
    kept: i32,
    /// Stamps laid so far (for the wetness jitter).
    stamps: u32,
    /// Wet paint with a texturized grain: the stroke without the grain, then the layer as it was,
    /// the grain laid over the change once (see `grain_once`).
    grained: Option<wgpu::Buffer>,
    /// Wet paint that builds up by what is left (the modulated mode): how much each pixel has taken
    /// so far, 0..1; for paint laid apart, then how much of the paint shows at each pixel.
    amount: Option<wgpu::Buffer>,
    /// A dual brush's paint laid in place: laid apart (into `amount`'s second half) and put
    /// together with its second where either goes (`cs_synthesize`), with the stroke's look and
    /// its second's dual mode + 1 and threshold (the mode 0 once the second is dropped).
    apart: Option<(StrokeStyle, u32, f32)>,
    /// Wet paint: stamps waiting for the rest of their display frame (with their wet-mix scaling
    /// and input time), the passes laid so far, and the texels a side of the last pass's plots.
    pending: Vec<(StampInstance, [f32; 8], Paced)>,
    passes: u32,
    plot: u32,
}

/// Whether a stroke's texturized grain is laid over its wet paint once, as it ends up, rather
/// than stamp by stamp (where it would wash out as the stamps build up).
fn grain_once(style: &StrokeStyle) -> bool {
    style.paints_in_place()
        && style.grain.is_some()
        && style.grain_mode == GrainMode::Texturized
        && style.grain_depth > 0.0
}

#[derive(Clone, Copy, Debug)]
struct ActiveStroke {
    layer: LayerId,
    style: StrokeStyle,
    bounds: IRect,
    /// Painting an adjustment on: the stamps go into the adjustment's mask, not a layer.
    pencil: bool,
    /// A dual brush's second stroke, if it has one.
    second: Option<SecondStroke>,
}

/// A dual brush's second stroke, drawn along the open stroke's path into a texture of its own
/// (in the stroke's colour): where the two meet, its coverage combines with the first's by `mode`
/// (a blend mode's number: by that blend of the two amounts; Normal: the second shows where the
/// first doesn't), cut at `threshold` first if that is 0..1.
#[derive(Clone, Copy, Debug)]
pub struct SecondStroke {
    pub style: StrokeStyle,
    pub mode: u32,
    pub threshold: f32,
}

enum Op {
    /// A plugin's filter, drawn as [`Op::FilterSet`] is, in pieces of its own.
    PluginFilter(plugin_filter::FilterValues),
    /// The open stroke's second cleared (a dual brush's stroke begins, with this second).
    ClearSecond(SecondStroke),
    /// The open stroke goes on without its second.
    DropSecond,
    /// Stamps added to the stroke (`clear`: the stroke is redrawn from scratch).
    Stamps {
        /// Into the stroke's second (a dual brush's).
        second: bool,
        instances: Vec<StampInstance>,
        /// Each stamp's wet-mix scaling (see [`Stamp::wet`]) and where it comes in the stroke, for
        /// the wet paint drawn in place.
        wet: Vec<[f32; 8]>,
        times: Vec<Paced>,
        style: StrokeStyle,
        clear: bool,
        layer: LayerId,
        /// Into the adjustment's mask instead of the stroke.
        pencil: bool,
    },
    Commit(ActiveStroke),
    /// The next stroke committed changed pixels only within these rectangles: its undo step
    /// keeps only them.
    Within(Vec<IRect>),
    Cancel,
    /// What the stroke has under this rectangle is put aside, for a tip drawn over it.
    SaveTip(IRect),
    /// What was put aside under this rectangle comes back (the tip drawn over it goes).
    Untip(IRect),
    Undo,
    Redo,
    /// A layer cleared, as one undo step keeping what was painted (all of it, or the box given
    /// when nothing is painted outside it).
    Clear(LayerId, Option<IRect>),
    /// Clears a layer without an undo step (scratch canvases).
    Wipe(LayerId),
    /// A layer's colours inverted within a rectangle (its coverage kept), as one undo step keeping
    /// that rectangle.
    Invert(LayerId, IRect),
    /// A layer filled all over with an opaque colour (RGB), as one undo step keeping the whole of
    /// it.
    Fill(LayerId, [u8; 3]),
    /// The document changed structurally; `before` is how it was.
    Record(Box<Document>),
    /// Layers were added as copies: (from, to) pairs.
    Duplicate {
        pairs: Vec<(LayerId, LayerId)>,
        before: Box<Document>,
    },
    /// `upper` is blended into `lower` and removed from the document; `within` (where `upper` is
    /// painted, if known) is all of `lower` that can change, and all its undo step keeps. Each
    /// of `masks` (a layer, its mask) is applied to its layer first, in the same step; with
    /// `drop_lower` (it was hidden), `lower`'s own pixels go first.
    MergeDown {
        upper: LayerId,
        lower: LayerId,
        before: Box<Document>,
        within: Option<IRect>,
        masks: Vec<(LayerId, LayerId)>,
        drop_lower: bool,
    },
    /// `uppers` (bottom first; groups drawn first) are blended into `lower` in turn, as one step,
    /// and removed from the document; each of `masks` (a layer, its mask) applied to its layer
    /// first, and with `drop_lower` (it was hidden) `lower`'s own pixels let go first.
    MergeMany {
        /// Where they are painted, if known: all of `lower` that can change, all the step keeps.
        within: Option<IRect>,
        uppers: Vec<LayerId>,
        lower: LayerId,
        before: Box<Document>,
        masks: Vec<(LayerId, LayerId)>,
        drop_lower: bool,
    },
    /// Layer `layer` multiplied by its mask `mask` in place; the caller removes the mask once
    /// this is queued (`before` is the document with it).
    ApplyMask {
        layer: LayerId,
        mask: LayerId,
        before: Box<Document>,
    },
    /// Group `group` of `before` is drawn into `into`, a new layer.
    Flatten {
        group: LayerId,
        into: LayerId,
        before: Box<Document>,
    },
    /// The layer is copied aside to be transformed.
    TransformBegin(LayerId),
    /// Another layer is copied aside to be transformed with the first.
    TransformAlso(LayerId),
    /// The layer is redrawn from its copy through `to_source` (layer pixels to copy pixels).
    TransformSet {
        to_source: [f32; 9],
        resample: Resample,
    },
    /// The layer is redrawn from its copy as triangles, three corners each of (where it goes,
    /// where it came from) in pixels.
    TransformMesh {
        vertices: Vec<[f32; 4]>,
        resample: Resample,
    },
    /// The next step made is named so.
    Name(String),
    /// A step that changes nothing here, named so (see [`Canvas::record_outside_change`]).
    OutsideChange(String),
    /// Where the history is now is marked.
    Mark,
    /// The steps made since the mark become one.
    Collapse,
    /// Layers mirrored, left to right (`horizontal`) or top to bottom, together as one undo step.
    Flip {
        layers: Vec<LayerId>,
        horizontal: bool,
    },
    /// Layers' pixels are replaced inside a rectangle each (premultiplied RGBA8, the rectangle's
    /// rows), together as one undo step keeping those rectangles, with the document as it was
    /// (`before`) when that changed too.
    Write {
        layers: Vec<(LayerId, IRect, Vec<u8>)>,
        before: Option<Box<Document>>,
    },
    /// A layer's pixels are replaced with no undo step (during an edit held with `Hold`).
    Put(LayerId, Vec<u8>),
    /// Part of a layer's pixels replaced so: the rectangle at (`x`, `y`), `width` x `height`.
    PutRegion {
        layer: LayerId,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    },
    /// A layer's pixels are set aside: what an edit made of many changes goes back to.
    Hold(LayerId),
    /// The held pixels, and the document as it was (`before`), become one undo step (`keep`), or
    /// the pixels are put back.
    Release {
        keep: bool,
        before: Option<Box<Document>>,
    },
    /// The layer being edited is redrawn from its copy through an adjustment.
    FilterSet(Filter),
    /// The liquify field made, with no offsets.
    LiquifyBegin,
    /// The adjustment's painted-on mask made, empty.
    PencilBegin,
    /// A step of the liquify brush: its stamps added up, the field carried along by them.
    LiquifyStamps(Vec<LiquifyStamp>),
    /// The liquify field scaled by this much.
    LiquifyScale(f32),
    /// The liquify field back to no offsets.
    LiquifyClear,
    /// The transform ends: kept as one undo step (keeping only the part `within`, when its layers
    /// changed only there), or put back.
    TransformEnd {
        commit: bool,
        within: Option<IRect>,
    },
}

/// A layer being transformed: its pixels from before, which it is redrawn from.
struct Transforming {
    layer: LayerId,
    before: wgpu::Texture,
    bind: wgpu::BindGroup,
    /// Only what the selection takes moves.
    masked: bool,
    /// The copy alone, for an adjustment's last pass to mix with.
    before_bind: wgpu::BindGroup,
}

/// An adjustment drawn into the layer being edited (begun as a transform is), from its copy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Filter {
    /// In luma and chroma: the chroma turned by `hue` (radians) and scaled by `saturation`, and
    /// `brightness` added to the luma.
    Hsb { hue: f32, saturation: f32, brightness: f32 },
    /// A Gaussian blur of `sigma` canvas pixels.
    Blur { sigma: f32 },
    /// Each pixel pushed away from its neighbours by `amount`.
    Sharpen { amount: f32 },
    /// Fractal noise of `kind` (0 clouds, 1 billows, 2 ridges): `amount`, `frequency` (per canvas
    /// pixel), `octaves` (1..5) and `turbulence` (how much each finer octave keeps); each channel
    /// its own when `multi`; added to the colour, or else mixed in.
    Noise { amount: f32, frequency: f32, octaves: u32, turbulence: f32, kind: u32, multi: bool, added: bool },
    /// Each pixel the mean along `along` (canvas pixels), centred on it.
    Motion { along: [f32; 2] },
    /// Each pixel streaked towards `centre` (canvas pixels) by `amount` of the way. With `toward`
    /// (a unit vector; zero for all round) only the side it points to: all of it straight along,
    /// less the further a pixel's way from the centre turns from it, none from square to it on.
    Zoom { centre: [f32; 2], amount: f32, toward: [f32; 2] },
    /// Red and blue pulled apart: from `centre` (canvas pixels) by `amount` of the distance, less
    /// within `falloff` (0..1 of the half diagonal) of it; or else (`displace`) by `offset` (canvas
    /// pixels) everywhere. `blur` (0..1) smears the fringes along the pull; `transparency` (0..1)
    /// fades the fringes beyond the picture.
    Chromatic {
        centre: [f32; 2],
        amount: f32,
        displace: bool,
        offset: [f32; 2],
        blur: f32,
        falloff: f32,
        transparency: f32,
    },
    /// A halftone of `kind` (0 full colour, 1 screen print, 2 newspaper) with cells `pitch` canvas
    /// pixels across.
    Halftone { pitch: f32, kind: u32 },
    /// The light of a blur (`sigma`) screened over the layer: `amount`, from `threshold` up over
    /// a ramp `transition` (0..1) long, strengthened by `burn` (−0.3..1).
    Bloom { sigma: f32, amount: f32, threshold: f32, transition: f32, burn: f32 },
    /// Each channel through the curves set with [`Canvas::set_table`].
    Curves,
    /// The gradient set with [`Canvas::set_table`] by brightness, `amount` of the way.
    GradientMap { amount: f32 },
    /// Each channel (red, green, blue) shifted in its shadows, midtones and highlights (−1..1).
    ColorBalance { shadows: [f32; 3], midtones: [f32; 3], highlights: [f32; 3] },
    /// The copy moved by the liquify field (begun with [`Canvas::begin_liquify`]), `amount` of it.
    Liquify { amount: f32 },
    /// The copy from `offset` (canvas pixels) away: cloning, painted on.
    Clone { offset: [f32; 2] },
    /// A glitch of `kind` 0 artifact (blocks copied from a few blocks away), 1 wave (rows bent
    /// sideways), 2 signal (bands ghosted sideways), 3 diverge (the channels moved apart in
    /// blocks), `amount` (0..1) of it. `values` are the kind's own: artifact and signal how many and
    /// the block size (0..1), wave the amplitude and frequency (0..1), diverge the red, green and
    /// blue shifts (−1..1). `zoom` (0..1) scales the pattern; `seed` varies it.
    Glitch { kind: u32, amount: f32, values: [f32; 3], zoom: f32, seed: f32 },
}

/// How a liquify stamp moves what is under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LiquifyMode {
    Push,
    TwirlRight,
    TwirlLeft,
    Pinch,
    Expand,
    /// Drawn in (a negative amount: out) towards its way only, narrower the nearer to 1 `narrow`.
    Lobe {
        narrow: f32,
    },
    Edge,
    Reconstruct,
}

impl LiquifyMode {
    /// The number the shader knows the mode by, and how narrow a lobe is.
    fn code(self) -> (f32, f32) {
        match self {
            LiquifyMode::Push => (0.0, 0.0),
            LiquifyMode::TwirlRight => (1.0, 0.0),
            LiquifyMode::TwirlLeft => (2.0, 0.0),
            LiquifyMode::Pinch => (3.0, 0.0),
            LiquifyMode::Expand => (4.0, 0.0),
            LiquifyMode::Lobe { narrow } => (5.0, narrow),
            LiquifyMode::Edge => (6.0, 0.0),
            LiquifyMode::Reconstruct => (7.0, 0.0),
        }
    }
}

/// One liquify stamp: where and how wide (canvas pixels), the way it goes (one long, for a push, a
/// lobe or an edge) and how much it does: for a push or an edge how far the brush moved, the rest
/// how long it worked (seconds), each times how hard it presses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquifyStamp {
    pub centre: [f32; 2],
    pub way: [f32; 2],
    pub radius: f32,
    pub amount: f32,
    pub mode: LiquifyMode,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LiquifyUniform {
    stamp: [f32; 4],
    way: [f32; 4],
    canvas: [f32; 4],
    area: [f32; 4],
}

/// An adjustment painted on: how much of each pixel the brush has covered (the stroke format, its
/// red channel read).
struct PencilMask {
    view: wgpu::TextureView,
    /// Where it has been painted (canvas pixels): all the adjustment can have changed.
    bounds: IRect,
}

/// The liquify field: offsets (canvas pixels) a pixel shows the copy from, kept at half the
/// canvas's size, and that with the copy for the adjustment's last pass.
struct LiquifyField {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
    /// Field pixels a canvas pixel, across and down.
    scale: [f32; 2],
    /// Where a step works, grown to the widest it has needed.
    scratch: Option<LiquifyScratch>,
}

/// What a step's stamps add up to and the field as they leave it, for a part of the field.
struct LiquifyScratch {
    width: u32,
    height: u32,
    sums: wgpu::TextureView,
    next: wgpu::Texture,
    next_view: wgpu::TextureView,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FilterUniform {
    a: [f32; 4],
    b: [f32; 4],
    size: [f32; 4],
    c: [f32; 4],
}

struct LayerGpu {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// Steps (oldest first) as one: its pixels latest first, so undoing them puts back the oldest
/// last, and the document and selection from before the first that changed them.
fn collapsed(steps: Vec<Step>) -> Step {
    let made = steps.last().map_or(0, |s| s.made);
    let name = steps.iter().rev().find_map(|s| s.name.clone());
    let mut out = Step { made, name, doc: None, pixels: Vec::new(), selection: None, flip: None };
    for step in steps.into_iter().rev() {
        out.pixels.extend(step.pixels);
        if step.doc.is_some() {
            out.doc = step.doc;
        }
        if step.selection.is_some() {
            out.selection = step.selection;
        }
    }
    out
}

/// Pixels of a layer region from before an edit.
struct Snapshot {
    layer: LayerId,
    rect: IRect,
    texture: wgpu::Texture,
}

impl Snapshot {
    fn bytes(&self) -> u64 {
        u64::from(self.rect.width()) * u64::from(self.rect.height()) * 4
    }
}

/// One undoable step: the document as it was (for structural changes) and the pixels it changed.
struct Step {
    /// Which step it was ([`Canvas::steps_made`] once it was made); kept through undo and redo.
    made: u64,
    /// What it did, as undo and redo name it ("Paint stroke"), if it was named.
    name: Option<String>,
    doc: Option<Box<Document>>,
    pixels: Vec<Snapshot>,
    /// The selection to put back (its coverage, or none for nothing selected), when the step
    /// changed it.
    selection: Option<Option<Vec<u8>>>,
    /// Layers flipped (across, or down): undone and redone by flipping them again, nothing kept.
    flip: Option<(Vec<LayerId>, bool)>,
}

impl Step {
    /// What it holds: its pixels on the GPU, and the selection it puts back (a byte a canvas
    /// pixel), so many selection changes can't outgrow the history's budget either.
    fn bytes(&self) -> u64 {
        let selection = self.selection.as_ref().and_then(Option::as_ref).map_or(0, |s| s.len() as u64);
        self.pixels.iter().map(Snapshot::bytes).sum::<u64>() + selection
    }
}

/// The pipelines every canvas on a device draws with, made once for the device (see
/// [`crate::Gpu`]): compiling them takes a tablet a second and a half.
pub(crate) struct Pipelines {
    /// A 3D colour table and its sampler, for the display.
    lut_layout: wgpu::BindGroupLayout,
    /// The display's: the canvas, the uniforms and the colour table.
    display_lut_layout: wgpu::PipelineLayout,
    quad_layout: wgpu::BindGroupLayout,
    brush_layout: wgpu::BindGroupLayout,
    sampled_layout: wgpu::BindGroupLayout,
    uniform_layout: wgpu::BindGroupLayout,
    stamp: wgpu::RenderPipeline,
    stamp_max: wgpu::RenderPipeline,
    /// Stamps adding towards their transfer (dual-source blending), where the GPU has it.
    stamp_capped: Option<wgpu::RenderPipeline>,
    layer: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
    solid: wgpu::RenderPipeline,
    commit_paint: wgpu::RenderPipeline,
    commit_erase: wgpu::RenderPipeline,
    commit_locked: wgpu::RenderPipeline,
    thumb: wgpu::RenderPipeline,
    transform: wgpu::RenderPipeline,
    mesh: wgpu::RenderPipeline,
    /// What a transform leaves where it was (with a selection).
    transform_static: wgpu::RenderPipeline,
    transform_layout: wgpu::BindGroupLayout,
    /// Where a layer is painted, across its rows and down its columns; and where it differs from
    /// another.
    bounds_layout: wgpu::BindGroupLayout,
    bounds_rows: wgpu::RenderPipeline,
    bounds_columns: wgpu::RenderPipeline,
    changed_layout: wgpu::BindGroupLayout,
    changed_rows: wgpu::RenderPipeline,
    changed_columns: wgpu::RenderPipeline,
    /// A layer mirrored left to right, and top to bottom.
    flip_across: wgpu::RenderPipeline,
    flip_down: wgpu::RenderPipeline,
    invert: wgpu::RenderPipeline,
    /// A layer of the canvas before a crop, seen through it.
    crop_layout: wgpu::BindGroupLayout,
    crop: wgpu::RenderPipeline,
    /// Adjustments: the copy alone, and the passes.
    before_layout: wgpu::BindGroupLayout,
    /// Adjustment passes by their fragment entry.
    filters: HashMap<&'static str, wgpu::RenderPipeline>,
    /// What every adjustment's passes are laid out by, a plugin's filter too.
    filter_layout: wgpu::PipelineLayout,
    /// Liquify's stamps added up and the field carried along by them, the format the field
    /// takes here, and what that second pass reads.
    liquify_stamp: wgpu::RenderPipeline,
    liquify_step: wgpu::RenderPipeline,
    liquify_format: wgpu::TextureFormat,
    liquify_layout: wgpu::BindGroupLayout,
    selection_module: wgpu::ShaderModule,
    smudge_layout: wgpu::BindGroupLayout,
    smudge_carry: wgpu::ComputePipeline,
    smudge_blur_down: wgpu::ComputePipeline,
    smudge_blur_across: wgpu::ComputePipeline,
    smudge_deposit: wgpu::ComputePipeline,
    smudge_synthesize: wgpu::ComputePipeline,
    smudge_wet_plots: wgpu::ComputePipeline,
    display_module: wgpu::ShaderModule,
    display_pipeline_layout: wgpu::PipelineLayout,
    surround_module: wgpu::ShaderModule,
    surround_pipeline_layout: wgpu::PipelineLayout,
    stamp_layout: wgpu::PipelineLayout,
    overlay_module: wgpu::ShaderModule,
    /// Frosted panels: the blur passes and the regions drawn.
    backdrop_module: wgpu::ShaderModule,
}

/// A canvas's own pipelines for drawing to a window, made for each target format as it comes.
#[derive(Default)]
struct ScreenPipelines {
    display: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    surround: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    overlay: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    /// The selection's look, per target format and whether it multiplies.
    selection: HashMap<(wgpu::TextureFormat, bool), wgpu::RenderPipeline>,
    blur: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    frost: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
}

pub struct Canvas {
    width: u32,
    height: u32,
    pipelines: std::sync::Arc<Pipelines>,
    screen: ScreenPipelines,
    layers: HashMap<LayerId, LayerGpu>,
    stroke: wgpu::Texture,
    stroke_view: wgpu::TextureView,
    /// Where the open stroke's tip is drawn (see [`Canvas::set_tip`]), and what it covered.
    tip: Option<IRect>,
    tip_backup: Option<wgpu::Texture>,
    /// The finished composite, and the other half of its ping-pong pair: where a layer of a
    /// blend mode other than Normal goes, made the first time one does.
    composite: wgpu::Texture,
    composite_view: wgpu::TextureView,
    composite_second: OnDemand,
    /// Where a group's layers are composited before the group goes onto the stack below: a pair
    /// for each depth of group (made when first needed).
    groups: Vec<([wgpu::Texture; 2], [wgpu::TextureView; 2])>,
    /// Animation Assist playing: the frame shown (alone, no onion skins).
    play_frame: Option<usize>,
    /// Where a stroke of a blend mode, or one on a locked layer, and a merge are drawn before
    /// they go back into the layer; made the first time one is.
    scratch: OnDemand,
    /// A dual brush's second stroke (see [`SecondStroke`]), made when one is first drawn.
    second_stroke: OnDemand,
    /// The second of the stroke being drawn from the ops (told by [`Op::ClearSecond`]), until it
    /// ends.
    queued_second: Option<SecondStroke>,
    /// A blur's working textures by size and part, kept from one step of an adjustment to the
    /// next (made each step, those the canvas's size were most of a narrow blur's cost on a
    /// tablet); let go when the adjustment ends.
    blur_textures: Mutex<HashMap<(u32, u32, u8), wgpu::TextureView>>,
    /// The copy a transform or adjustment kept of its layer, when it ended without it (let go,
    /// or kept only in part): the next one copies into it.
    spare_before: Option<wgpu::Texture>,
    /// Textures the canvas's size let go (an undo step's, redo cleared by a new step or the
    /// oldest dropped; a layer's nothing refers to any more), a few kept for the next new layer or
    /// step that keeps the whole of a layer: making one takes a tablet 15-25 ms.
    spares: Mutex<Vec<wgpu::Texture>>,
    /// Buffers pixels go up to the layers through, kept mapped for the next upload once the GPU
    /// is done with them (see [`Uploads`]).
    uploads: Uploads,
    sampler: wgpu::Sampler,
    grains_view: wgpu::TextureView,
    grain_sampler: wgpu::Sampler,
    shapes_view: wgpu::TextureView,
    shape_sampler: wgpu::Sampler,
    /// The shape and grain arrays, and which images of the user's own their spare layers hold;
    /// and whether they hold the images yet (else one-texel stand-ins, for a canvas that takes
    /// only drawn strokes until something wants them).
    shapes: wgpu::Texture,
    grains: wgpu::Texture,
    brush_images: bool,
    /// The open stroke's grain fixed to the canvas, from a picture its size in place of a brush
    /// image (see [`Canvas::set_stroke_grain`]); kept until the stroke has ended.
    stroke_grain: Option<wgpu::TextureView>,
    /// Brush files' grains at their own size, and the one the stroke's bind reads.
    full_grains: FullGrains,
    full_shapes: FullGrains,
    bound_full_grain: Option<u64>,
    shape_sources: SourceSlots,
    grain_sources: SourceSlots,
    /// Full coverage, stood in for the selection when there is none.
    everything: wgpu::TextureView,
    /// Where an adjustment is painted on (Pencil), instead of everywhere the selection takes.
    pencil: Option<PencilMask>,
    liquify: Option<LiquifyField>,
    /// The curves or gradient adjustments read (256 RGBA8 entries).
    table: wgpu::Texture,
    table_view: wgpu::TextureView,
    selection: Option<SelectionGpu>,
    /// Shapes, grains and the selection for the stamp shader.
    brush_bind: wgpu::BindGroup,
    /// Every texture slot bound to the stroke texture: for draws that only read the stroke.
    stroke_bind: wgpu::BindGroup,
    /// The picture as shown: smoothed between its pixels, or each a square ([`Target::crisp_from`]).
    display_bind: wgpu::BindGroup,
    crisp_bind: wgpu::BindGroup,
    /// The table that leaves colours as they are, bound when a target has none.
    identity_lut: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    uniform_bind: wgpu::BindGroup,
    uniform_slots: u64,
    stamp_buffer: wgpu::Buffer,
    stamp_capacity: u64,
    /// The most stamps' work sent to the GPU as one piece (see [`WORK_AT_ONCE`]).
    work_at_once: u64,
    /// Pieces of work sent early, there being too much for one.
    pieces_sent: u64,
    /// How the last plugin filter's draw went, not yet asked for.
    filter_run: Option<plugin_filter::FilterRun>,
    /// How many times the composite has been redrawn.
    composites: u64,
    active: Option<ActiveStroke>,
    smudge: Option<Smudge>,
    /// The open smudge stroke couldn't be begun (what it takes couldn't be made): it does nothing
    /// until it ends. And whether that is still to be told.
    smudge_refused: bool,
    smudge_untold: bool,
    /// A layer's pixels set aside by [`Canvas::hold_layer`].
    held: Option<Snapshot>,
    /// Counts the undo steps ever made, so that a change since some moment shows.
    steps_made: u64,
    /// The steps made when [`Canvas::mark_steps`] was last done.
    mark: Option<u64>,
    /// The name the next step made takes.
    next_name: Option<String>,
    /// Where the next stroke committed changes pixels, if known more closely than its bounds.
    within: Option<Vec<IRect>>,
    smudge_params: wgpu::Buffer,
    smudge_capacity: u64,
    /// The passes of wet paint a render lays (see [`WetPass`]), and how many it holds.
    wet_params: wgpu::Buffer,
    wet_capacity: u64,
    /// Stand-ins for a smudge's grain buffers when its stroke has none.
    smudge_spare: [wgpu::Buffer; 2],
    /// Stamps of input that hasn't happened yet (predicted), drawn over the canvas for one frame.
    prediction: Vec<StampInstance>,
    ops: Vec<Op>,
    dirty: IRect,
    /// Whether the composite is made around pictures of what isn't being worked on, and them.
    cache_composites: bool,
    caches: Option<Caches>,
    /// Caches being remade a little each render (more layers than fit), and the composite left
    /// as it was until they are ready.
    remake: Option<Remake>,
    /// The textures of caches let go, kept to make the next ones in: four the canvas's size are
    /// slow to make on a tablet (a layer added or deleted, a duplicate, a merge each made them).
    spare_caches: Option<Caches>,
    /// Layers' pixels kept compressed off the GPU: those parked (no texture), and copies of
    /// resident ones unchanged since, so that letting them go again costs nothing.
    parked: HashMap<LayerId, Parked>,
    /// Small pictures of the parked layers, so their thumbnails need no texture.
    pictures: HashMap<LayerId, Pictures>,
    /// Layers whose pixels changed since [`Canvas::take_changed`] took them, so a save reads only
    /// those.
    changed: HashSet<LayerId>,
    /// Each layer's pixels' version, from a count that goes up with every change: thumbnails are
    /// read again only when theirs has.
    versions: HashMap<LayerId, u64>,
    version: u64,
    /// Layers past the budget are still to be parked (a render parks only a few).
    parking_left: bool,
    /// How the last remake of the caches went.
    last_remake: Option<RemakeStats>,
    /// What the layers' textures may take before those needed least lately are parked (0: none
    /// are), and when each was last needed, counted in renders.
    resident_budget: u64,
    needed_at: HashMap<LayerId, u64>,
    renders: u64,
    undo: Vec<Step>,
    redo: Vec<Step>,
    transforming: Option<Transforming>,
    /// Other layers transformed along with it (layers picked together).
    transform_more: Vec<Transforming>,
    /// For frosted panels, made when first needed.
    backdrop: Option<BackdropGpu>,
}

const PREMULTIPLIED_OVER: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

/// The larger of the two: stamps that don't build up (uniform glaze).
/// The target multiplied by what is drawn (its alpha kept).
const MULTIPLY: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Dst,
        dst_factor: wgpu::BlendFactor::Zero,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

/// A capped stamp: its paint (scaled by its transfer) plus what is there kept by what the stamp
/// leaves (the second colour out of the shader), so each stamp adds towards its transfer.
const TOWARDS_TRANSFER: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrc1,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrc1Alpha,
        operation: wgpu::BlendOperation::Add,
    },
};

const MAXIMUM: wgpu::BlendState = wgpu::BlendState {
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

/// `dst · (1 − src.a)`: erasing.
const DESTINATION_OUT: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

/// A stroke texture canvases of one size take turns to draw their strokes in, so that each
/// needn't hold one of its own: a model's texture sets, one painted at a time. Clones are the
/// same texture.
#[derive(Clone)]
pub struct SharedStroke {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl SharedStroke {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> SharedStroke {
        let texture = texture(&gpu.device, "shared stroke", width.max(1), height.max(1), STROKE_FORMAT);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        SharedStroke { texture, view }
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
}

/// An array of brush images (shapes or grains) viewed whole.
fn array_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

impl Canvas {
    pub fn new(gpu: &Gpu, doc: &Document) -> Result<Canvas, GpuError> {
        let mut canvas = Canvas::unfilled(gpu, doc)?;
        canvas.fill_rest(gpu, doc);
        Ok(canvas)
    }

    /// A canvas with no layer textures yet, for layers about to come from elsewhere (an artwork
    /// opened, a crop): each is made as its layer goes up, those past the budget being parked
    /// instead, so that a canvas of many layers never holds a texture for every one at once.
    /// [`Canvas::fill_rest`] ends it.
    pub fn unfilled(gpu: &Gpu, doc: &Document) -> Result<Canvas, GpuError> {
        Canvas::make(gpu, doc, None, true)
    }

    /// Makes the textures of the layers that have none and aren't parked (clear).
    pub fn fill_rest(&mut self, gpu: &Gpu, doc: &Document) {
        self.sync_layers(gpu, doc);
    }

    /// A canvas whose strokes are drawn into it from elsewhere ([`Canvas::begin_drawn_stroke`]),
    /// as a model's texture sets take them from the screen: it draws them in `stroke` (shared
    /// with other canvases its size, one stroke open among them at a time), and makes its shape
    /// and grain images only once something wants them.
    pub fn for_drawn_strokes(gpu: &Gpu, doc: &Document, stroke: &SharedStroke) -> Result<Canvas, GpuError> {
        let mut canvas = Canvas::make(gpu, doc, Some(stroke), false)?;
        canvas.fill_rest(gpu, doc);
        Ok(canvas)
    }

    fn make(gpu: &Gpu, doc: &Document, stroke: Option<&SharedStroke>, images: bool) -> Result<Canvas, GpuError> {
        let (width, height) = (doc.width(), doc.height());
        let max = gpu.max_texture_size();
        if width > max || height > max {
            return Err(GpuError::TooLarge(width, height, max));
        }
        let device = &gpu.device;
        let pipelines = gpu.canvas_pipelines();
        let fits = stroke.filter(|s| (s.texture.width(), s.texture.height()) == (width, height));
        let (stroke_texture, stroke_view) = match fits {
            Some(s) => (s.texture.clone(), s.view.clone()),
            None => {
                let t = texture(device, "stroke", width, height, STROKE_FORMAT);
                let v = t.create_view(&wgpu::TextureViewDescriptor::default());
                (t, v)
            }
        };
        let composite = texture(device, "composite", width, height, LAYER_FORMAT);
        let composite_view = composite.create_view(&wgpu::TextureViewDescriptor::default());
        let (shape_images, grain_images) =
            if images { (textures::shapes(), textures::grains()) } else { (Vec::new(), Vec::new()) };
        let (shapes, grains) = if images {
            (brush_images(gpu, "shapes", &shape_images), brush_images(gpu, "grains", &grain_images))
        } else {
            (stand_in_images(gpu, "shapes"), stand_in_images(gpu, "grains"))
        };
        let shapes_view = array_view(&shapes);
        let grains_view = array_view(&grains);
        let mipmapped = |label, address| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: address,
                address_mode_v: address,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            })
        };
        let shape_sampler = mipmapped("shape", wgpu::AddressMode::ClampToEdge);
        let grain_sampler = mipmapped("grain", wgpu::AddressMode::Repeat);
        let table = texture(device, "adjustment table", 256, 1, LAYER_FORMAT);
        let table_view = table.create_view(&wgpu::TextureViewDescriptor::default());
        let everything =
            coverage_texture(device, &gpu.queue, 1, 1, &[255]).create_view(&wgpu::TextureViewDescriptor::default());
        let brush_bind = brush_bind(
            device,
            &pipelines.brush_layout,
            [&shapes_view, &grains_view, &everything],
            [&shape_sampler, &grain_sampler],
        );
        let stroke_bind = quad_bind(device, &pipelines.quad_layout, [&stroke_view; 7], &grains_view, &grain_sampler);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let display_bind = sampled_bind(device, &pipelines.sampled_layout, &composite_view, &sampler);
        let crisp_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("crisp"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let crisp_bind = sampled_bind(device, &pipelines.sampled_layout, &composite_view, &crisp_sampler);
        // two points on a side: each corner its own colour
        let corners: Vec<u8> = (0..8u8).flat_map(|i| [(i >> 2) & 1, (i >> 1) & 1, i & 1, 1].map(|c| c * 255)).collect();
        let identity_lut = lut_bind(gpu, &pipelines.lut_layout, 2, &corners);
        let uniform_slots = 64;
        let (uniform_buffer, uniform_bind) = uniforms(device, &pipelines.uniform_layout, uniform_slots);
        let stamp_capacity = 4096;
        let stamp_buffer = stamp_buffer(device, stamp_capacity);
        let canvas = Canvas {
            width,
            height,
            pipelines,
            screen: ScreenPipelines::default(),
            layers: HashMap::new(),
            stroke: stroke_texture,
            stroke_view,
            tip: None,
            tip_backup: None,
            composite,
            composite_view,
            composite_second: OnDemand::new("composite 2", width, height, LAYER_FORMAT),
            groups: Vec::new(),
            play_frame: None,
            scratch: OnDemand::new("scratch", width, height, LAYER_FORMAT),
            second_stroke: OnDemand::new("second stroke", width, height, STROKE_FORMAT),
            queued_second: None,
            blur_textures: Mutex::new(HashMap::new()),
            spare_before: None,
            spares: Mutex::new(Vec::new()),
            uploads: Uploads::default(),
            sampler,
            grains_view,
            grain_sampler,
            shapes_view,
            shape_sampler,
            everything,
            pencil: None,
            liquify: None,
            table,
            table_view,
            selection: None,
            brush_bind,
            stroke_bind,
            display_bind,
            crisp_bind,
            identity_lut,
            uniform_buffer,
            uniform_bind,
            uniform_slots,
            stamp_buffer,
            stamp_capacity,
            work_at_once: WORK_AT_ONCE,
            pieces_sent: 0,
            filter_run: None,
            composites: 0,
            active: None,
            smudge: None,
            smudge_refused: false,
            smudge_untold: false,
            held: None,
            steps_made: 0,
            mark: None,
            next_name: None,
            within: None,
            smudge_params: smudge_params(device, 64),
            smudge_spare: [spare_buffer(device), spare_buffer(device)],
            smudge_capacity: 64,
            wet_params: wet_params(device, 4),
            wet_capacity: 4,
            prediction: Vec::new(),
            ops: Vec::new(),
            cache_composites: false,
            caches: None,
            remake: None,
            spare_caches: None,
            parked: HashMap::new(),
            pictures: HashMap::new(),
            changed: HashSet::new(),
            versions: HashMap::new(),
            version: 0,
            parking_left: false,
            last_remake: None,
            resident_budget: 0,
            needed_at: HashMap::new(),
            renders: 0,
            dirty: IRect::from_size(width, height),
            undo: Vec::new(),
            redo: Vec::new(),
            transforming: None,
            transform_more: Vec::new(),
            backdrop: None,
            shapes,
            grains,
            brush_images: images,
            stroke_grain: None,
            full_grains: FullGrains::default(),
            full_shapes: FullGrains::default(),
            bound_full_grain: None,
            shape_sources: SourceSlots::after(shape_images.len()),
            grain_sources: SourceSlots::after(grain_images.len()),
        };
        Ok(canvas)
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Points `style` at `brush`'s own shape and grain images, if it has them, putting them in
    /// the arrays' spare layers first if they aren't there.
    pub fn use_sources(&mut self, gpu: &Gpu, style: &mut StrokeStyle, brush: &Brush) {
        if brush.shape.source.is_some() || brush.grain.source.is_some() {
            self.prepare_brush_images(gpu);
        }
        if let Some(source) = brush.shape.source.as_ref() {
            style.shape = Some(self.shape_sources.layer(gpu, &self.shapes, source));
        }
        if let Some(source) = brush.grain.source.as_ref() {
            style.grain = Some(self.grain_sources.layer(gpu, &self.grains, source));
        }
    }

    /// Makes the shape and grain images now if the canvas goes without them (see
    /// [`Canvas::for_drawn_strokes`]), so the first stroke that wants them doesn't wait.
    pub fn prepare_brush_images(&mut self, gpu: &Gpu) {
        if self.brush_images {
            return;
        }
        let (shape_images, grain_images) = (textures::shapes(), textures::grains());
        self.shapes = brush_images(gpu, "shapes", &shape_images);
        self.grains = brush_images(gpu, "grains", &grain_images);
        self.shape_sources = SourceSlots::after(shape_images.len());
        self.grain_sources = SourceSlots::after(grain_images.len());
        self.brush_images = true;
        self.bind_brush_images(&gpu.device);
    }

    /// Lets go of what is kept only to be quicker next time: the textures of caches let go, a
    /// transform's copy let go, a blur's working textures, the upload buffers (the system short
    /// of memory, or the app put away). Each is made again when next wanted.
    pub fn let_go_of_spares(&mut self) {
        self.spare_caches = None;
        self.spare_before = None;
        self.spares.lock().unwrap_or_else(PoisonError::into_inner).clear();
        self.blur_textures.lock().unwrap_or_else(PoisonError::into_inner).clear();
        // a dual brush's second stroke's, unless one is open (or queued)
        let dual = self.active.is_some_and(|a| a.second.is_some())
            || self.ops.iter().any(|op| match op {
                Op::ClearSecond(_) | Op::DropSecond | Op::Stamps { second: true, .. } => true,
                Op::Commit(stroke) => stroke.second.is_some(),
                _ => false,
            });
        if !dual {
            self.second_stroke.let_go();
        }
        self.uploads.kept.lock().unwrap_or_else(PoisonError::into_inner).clear();
        // made again when a group composites on its own
        self.groups.clear();
    }

    /// Lets go of the shape and grain images until something wants them again (a canvas left
    /// waiting); not while a stroke is open.
    pub fn let_go_of_brush_images(&mut self, gpu: &Gpu) {
        if !self.brush_images || self.active.is_some() || self.smudge.is_some() || !self.ops.is_empty() {
            return;
        }
        self.shapes = stand_in_images(gpu, "shapes");
        self.grains = stand_in_images(gpu, "grains");
        self.shape_sources = SourceSlots::after(0);
        self.grain_sources = SourceSlots::after(0);
        self.brush_images = false;
        self.prediction.clear();
        self.bind_brush_images(&gpu.device);
    }

    /// `style` reads the brush file's grain `id` (`pixels`, `side` square) in place of the brush
    /// images: each stroke its own, kept on the GPU while it is among the last few used.
    pub fn use_full_grain(&mut self, gpu: &Gpu, style: &mut StrokeStyle, id: u64, side: u32, pixels: &[u8]) {
        if self.full_grains.hold(gpu, id, side, pixels) {
            style.grain = Some(0);
            style.full_grain = Some(id);
            style.grain_side = side as f32;
        }
    }

    /// `style` reads the brush file's shape `id` (`pixels`, `side` square) in place of the brush
    /// images: each stroke its own, kept on the GPU while it is among the last few used.
    pub fn use_full_shape(&mut self, gpu: &Gpu, style: &mut StrokeStyle, id: u64, side: u32, pixels: &[u8]) {
        if self.full_shapes.hold(gpu, id, side, pixels) {
            style.shape = Some(0);
            style.full_shape = Some(id);
        }
    }

    /// The open stroke's grain fixed to the canvas is read from `grain` from now on, in place of a
    /// brush image: a picture the canvas's size (square), one grey per pixel, viewed as an array
    /// of one, such as 3D painting reads along a model's three axes. How the grain shows (its
    /// depth, brightness, contrast and blend mode) stays the stroke's. Kept until the stroke has
    /// ended; nothing without an open stroke whose grain is fixed to the canvas.
    pub fn set_stroke_grain(&mut self, device: &wgpu::Device, grain: &wgpu::TextureView) {
        let square = self.width == self.height;
        let Some(active) = self.active.as_mut().filter(|a| square && a.style.canvas_grain().is_some()) else {
            return;
        };
        // one tile the canvas, read texel for texel
        active.style.full_grain = None;
        active.style.grain_side = TEXTURE_SIZE as f32;
        active.style.grain = Some(0);
        active.style.grain_scale = self.width as f32;
        active.style.grain_rotation = 0.0;
        active.style.grain_sharp = false;
        self.stroke_grain = Some(grain.clone());
        self.bind_brush_images(device);
    }

    /// The grains the open stroke reads: its own picture, else the brush images.
    fn grain_view(&self) -> &wgpu::TextureView {
        match self.active.as_ref() {
            Some(a) => self.grain_view_of(&a.style),
            None => self.stroke_grain.as_ref().unwrap_or(&self.grains_view),
        }
    }

    /// The grains a stroke in `style` reads, the open one or one still going on to its layer.
    fn grain_view_of(&self, style: &StrokeStyle) -> &wgpu::TextureView {
        match style.full_grain.and_then(|id| self.full_grains.view(id)) {
            Some(view) => view,
            None => self.stroke_grain.as_ref().unwrap_or(&self.grains_view),
        }
    }

    /// The bind of the shape and grain arrays for a stroke in `style` that reads its own grain or
    /// shape.
    fn full_grain_bind(&self, device: &wgpu::Device, style: &StrokeStyle) -> Option<wgpu::BindGroup> {
        let grain = style.full_grain.and_then(|id| self.full_grains.view(id));
        let shape = style.full_shape.and_then(|id| self.full_shapes.view(id));
        if grain.is_none() && shape.is_none() {
            return None;
        }
        let mask = self.selection.as_ref().map_or(&self.everything, |s| &s.view);
        Some(brush_bind(
            device,
            &self.pipelines.brush_layout,
            [shape.unwrap_or(&self.shapes_view), grain.unwrap_or(&self.grains_view), mask],
            [&self.shape_sampler, &self.grain_sampler],
        ))
    }

    /// Whether it holds its shape and grain images (else stand-ins until something wants them).
    pub fn has_brush_images(&self) -> bool {
        self.brush_images
    }

    /// The binds that hold the shape and grain arrays or the stroke, made again.
    fn bind_brush_images(&mut self, device: &wgpu::Device) {
        self.shapes_view = array_view(&self.shapes);
        self.grains_view = array_view(&self.grains);
        let mask = self.selection.as_ref().map_or(&self.everything, |s| &s.view);
        self.brush_bind = brush_bind(
            device,
            &self.pipelines.brush_layout,
            [&self.shapes_view, &self.grains_view, mask],
            [&self.shape_sampler, &self.grain_sampler],
        );
        self.stroke_bind = quad_bind(
            device,
            &self.pipelines.quad_layout,
            [&self.stroke_view; 7],
            self.grain_view(),
            &self.grain_sampler,
        );
    }

    /// Whether the queued work or the stroke open reads the shape or grain images: stamps, the
    /// predicted tail, a smudge, or a grain fixed to the canvas.
    fn wants_brush_images(&self) -> bool {
        let grained = |s: &StrokeStyle| s.canvas_grain().is_some() && self.stroke_grain.is_none();
        !self.prediction.is_empty()
            || self.smudge.is_some()
            || self.active.is_some_and(|a| grained(&a.style))
            || self.ops.iter().any(|op| match op {
                Op::Stamps { .. } => true,
                Op::Commit(stroke) => grained(&stroke.style),
                _ => false,
            })
    }

    /// From now on draws its strokes in `stroke` (a texture its size that other canvases take
    /// turns with), letting go of its own; whether it does. Not while a stroke is open.
    pub fn draw_strokes_in(&mut self, gpu: &Gpu, stroke: &SharedStroke) -> bool {
        if self.stroke == stroke.texture {
            return true;
        }
        let fits = (stroke.texture.width(), stroke.texture.height()) == (self.width, self.height);
        let busy = self.active.is_some() || self.smudge.is_some() || self.tip.is_some() || !self.ops.is_empty();
        if !fits || busy {
            return false;
        }
        self.stroke = stroke.texture.clone();
        self.stroke_view = stroke.view.clone();
        self.bind_brush_images(&gpu.device);
        true
    }

    fn full(&self) -> IRect {
        IRect::from_size(self.width, self.height)
    }

    pub fn has_stroke(&self) -> bool {
        self.active.is_some()
    }

    /// The open stroke's tool, if a stroke is open.
    pub fn stroke_tool(&self) -> Option<StrokeTool> {
        self.active.map(|a| a.style.tool)
    }

    /// Whether an undo is possible, counting queued work.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
            || self.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Commit(_)
                        | Op::Clear(..)
                        | Op::Invert(..)
                        | Op::Fill(..)
                        | Op::Record(_)
                        | Op::OutsideChange(_)
                        | Op::Duplicate { .. }
                        | Op::MergeDown { .. }
                        | Op::MergeMany { .. }
                        | Op::ApplyMask { .. }
                        | Op::Flatten { .. }
                        | Op::TransformEnd { commit: true, .. }
                        | Op::Write { .. }
                        | Op::Flip { .. }
                        | Op::Release { keep: true, .. }
                )
            })
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Drops what could be redone, as a new step would: the canvas is set aside for something new
    /// (a crop), and what was undone on it can no longer come back.
    pub fn forget_redo(&mut self) {
        for dropped in std::mem::take(&mut self.redo) {
            self.keep_spare_snapshot(dropped);
        }
    }

    /// Names the next step made (what undo and redo call it), queued with the work that makes it.
    pub fn name_next_step(&mut self, name: &str) {
        self.ops.push(Op::Name(name.to_owned()));
    }

    /// The name of the step undo would undo next, and of the step redo would redo, as the
    /// history is after the work run so far.
    pub fn undo_name(&self) -> Option<String> {
        self.undo.last().and_then(|s| s.name.clone())
    }

    pub fn redo_name(&self) -> Option<String> {
        self.redo.last().and_then(|s| s.name.clone())
    }

    /// Which step undo would undo next, and redo would redo, numbered as [`Canvas::steps_made`]
    /// counted them when they were made.
    pub fn undo_step(&self) -> Option<u64> {
        self.undo.last().map(|s| s.made)
    }

    pub fn redo_step(&self) -> Option<u64> {
        self.redo.last().map(|s| s.made)
    }

    /// Marks where the history is once the queued work is done, for [`Canvas::collapse_steps`].
    pub fn mark_steps(&mut self) {
        self.end_stroke();
        self.ops.push(Op::Mark);
    }

    /// Makes the steps still to undo that were made since the mark one step, which undoes them
    /// all; the mark is used up.
    pub fn collapse_steps(&mut self) {
        self.end_stroke();
        self.ops.push(Op::Collapse);
    }

    /// How many undo steps have ever been made here (queued work not counted until drawn): it
    /// changes whenever something new is done.
    pub fn steps_made(&self) -> u64 {
        self.steps_made
    }

    /// How many times the composite has been redrawn: it changes whenever the picture does.
    pub fn composites(&self) -> u64 {
        self.composites
    }

    /// Starts a stroke on `layer` (finishing one still open).
    pub fn begin_stroke(&mut self, layer: LayerId, style: StrokeStyle) {
        self.end_stroke();
        self.prediction.clear();
        self.active = Some(ActiveStroke { layer, style, bounds: IRect::EMPTY, pencil: false, second: None });
    }

    /// Begins a stroke as [`Canvas::begin_stroke`] does, with a dual brush's second (its stamps
    /// given with [`Canvas::add_second_stamps`]).
    pub fn begin_dual_stroke(&mut self, layer: LayerId, style: StrokeStyle, second: SecondStroke) {
        self.begin_stroke(layer, style);
        if let Some(active) = self.active.as_mut() {
            active.second = Some(second);
        }
        self.ops.push(Op::ClearSecond(second));
    }

    /// Draws more of the open stroke's second.
    pub fn add_second_stamps(&mut self, stamps: &[Stamp]) {
        self.push_stamps_to(stamps, false, true);
    }

    /// Redraws the open stroke's second from scratch with `stamps` (its final form).
    pub fn replace_second_stamps(&mut self, stamps: &[Stamp]) {
        self.push_stamps_to(stamps, true, true);
    }

    /// The open stroke goes on without its second (it was made into a shape).
    pub fn drop_second(&mut self) {
        if let Some(active) = self.active.as_mut()
            && active.second.take().is_some()
        {
            self.dirty = self.dirty.union(active.bounds);
            self.ops.push(Op::DropSecond);
        }
    }

    /// Begins a stroke whose pixels another pass draws into [`Canvas::stroke_target`] (3D
    /// painting carries a stroke drawn on the screen onto a model's texture); each draw is told
    /// with [`Canvas::stroke_drawn`], and the stroke ends (into `layer`) as any other.
    pub fn begin_drawn_stroke(&mut self, layer: LayerId, style: StrokeStyle) {
        self.begin_stroke(layer, style);
    }

    /// Where a drawn stroke's pixels go: premultiplied, the canvas's size.
    pub fn stroke_target(&self) -> &wgpu::TextureView {
        &self.stroke_view
    }

    /// The open stroke's pixels as drawn so far (premultiplied, the canvas's size): what 3D
    /// painting carries onto a model.
    pub fn stroke_view(&self) -> &wgpu::TextureView {
        &self.stroke_view
    }

    /// The canvas as composited by the last render (premultiplied, its size, the background in
    /// if shown): what 3D painting shows on a model.
    pub fn composite_view(&self) -> &wgpu::TextureView {
        &self.composite_view
    }

    /// A drawn stroke now reaches `bounds` (canvas pixels): it is shown there, and kept there
    /// when it ends.
    pub fn stroke_drawn(&mut self, bounds: IRect) {
        let full = self.full();
        let Some(active) = self.active.as_mut() else { return };
        let bounds = bounds.intersect(full);
        active.bounds = active.bounds.union(bounds);
        self.dirty = self.dirty.union(bounds);
    }

    /// Draws more of the open stroke.
    pub fn add_stamps(&mut self, stamps: &[Stamp]) {
        self.untip();
        self.push_stamps(stamps, false);
    }

    /// Redraws the open stroke from scratch with `stamps` (its final form, e.g. with the end taper).
    pub fn replace_stamps(&mut self, stamps: &[Stamp]) {
        self.tip = None;
        self.push_stamps(stamps, true);
    }

    /// Whether the open stroke can show a tip (see [`Canvas::set_tip`]): one built up in the
    /// stroke texture, not painted in place or onto an adjustment, nor a dual brush's: its second
    /// isn't there yet to cut it.
    pub fn shows_tip(&self) -> bool {
        self.active.is_some_and(|a| !a.pencil && !a.style.in_place() && a.second.is_none())
    }

    /// The open stroke's tip, drawn over it until the next stamps or tip: its last stamps as
    /// they'd be drawn were it to end now.
    pub fn set_tip(&mut self, stamps: &[Stamp]) {
        self.untip();
        if !self.shows_tip() || stamps.is_empty() {
            return;
        }
        let at = self.ops.len();
        let bounds = self.push_stamps(stamps, false);
        if bounds.is_empty() {
            return;
        }
        self.ops.insert(at, Op::SaveTip(bounds));
        self.tip = Some(bounds);
    }

    /// Takes the tip off the open stroke.
    fn untip(&mut self) {
        if let Some(rect) = self.tip.take() {
            self.dirty = self.dirty.union(rect);
            self.ops.push(Op::Untip(rect));
        }
    }

    /// Queues `stamps` into the open stroke (`clear`: from scratch); where they reach.
    fn push_stamps(&mut self, stamps: &[Stamp], clear: bool) -> IRect {
        self.push_stamps_to(stamps, clear, false)
    }

    /// As [`Canvas::push_stamps`], into the open stroke's second if `second` (none without one).
    fn push_stamps_to(&mut self, stamps: &[Stamp], clear: bool, second: bool) -> IRect {
        let full = self.full();
        let Some(active) = self.active.as_mut() else { return IRect::EMPTY };
        let mut bounds = IRect::EMPTY;
        let style = match (second, active.second) {
            (false, _) => active.style,
            (true, Some(s)) => s.style,
            (true, None) => return IRect::EMPTY,
        };
        let capped = self.pipelines.stamp_capped.is_some();
        let drawn: Vec<&Stamp> =
            stamps.iter().filter(|s| s.center.is_finite() && s.radius.is_finite() && s.alpha.is_finite()).collect();
        let wet: Vec<[f32; 8]> =
            drawn.iter().map(|s| s.wet.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 })).collect();
        let times: Vec<Paced> = drawn
            .iter()
            .map(|s| Paced {
                time_ms: if s.time_ms.is_finite() { s.time_ms } else { 0.0 },
                distance: if s.distance.is_finite() { s.distance } else { 0.0 },
            })
            .collect();
        let instances: Vec<StampInstance> = drawn
            .iter()
            .map(|s| {
                let radius = drawn_radius(&style, s.radius, s.taper);
                let tall = if s.roundness.is_finite() { s.roundness.clamp(1.0, MOST_ROUNDNESS) } else { 1.0 };
                bounds = bounds.union(IRect::around(s.center, radius * tall * std::f32::consts::SQRT_2));
                // without the capped pipeline, the transfer goes into the stamp
                let (alpha, transfer) = capped_alpha(&style, capped, s);
                StampInstance {
                    center: [s.center.x, s.center.y],
                    radius,
                    alpha,
                    angle: if s.angle.is_finite() { s.angle } else { 0.0 },
                    roundness: if s.roundness.is_finite() { s.roundness.clamp(0.05, MOST_ROUNDNESS) } else { 1.0 },
                    grain_offset: if s.grain_offset.is_finite() {
                        [s.grain_offset.x, s.grain_offset.y]
                    } else {
                        [0.0; 2]
                    },
                    flip: stamp_extras(s).0,
                    tint: stamp_extras(s).1,
                    brightness_add: stamp_extras(s).2,
                    grain_turn: if s.grain_turn.is_finite() { s.grain_turn } else { 0.0 },
                    look: [s.grain_depth, s.grain_size, s.secondary, s.gradation]
                        .map(|v| if v.is_finite() { v.clamp(0.0, 4096.0) } else { 0.0 }),
                    // the bleed up to 2 (the pressure's and the lean's), the rest 0..1
                    more: [(s.grain_press, 1.0), (s.grain_bleed, 2.0), (transfer, 1.0), (s.attack, 1.0)]
                        .map(|(v, most)| if v.is_finite() { v.clamp(0.0, most) } else { 0.0 }),
                }
            })
            .collect();
        let bounds = bounds.intersect(full);
        if clear && !second {
            self.dirty = self.dirty.union(active.bounds);
            active.bounds = bounds;
        } else {
            active.bounds = active.bounds.union(bounds);
        }
        self.dirty = self.dirty.union(bounds);
        if !instances.is_empty() || clear {
            let pencil = active.pencil;
            self.ops.push(Op::Stamps {
                instances,
                wet,
                times,
                style,
                clear: clear && !pencil,
                layer: active.layer,
                pencil,
                second,
            });
        }
        bounds
    }

    /// Puts the open stroke into its layer (undoably).
    /// Stamps for where the input is predicted to go next: drawn over the canvas until replaced,
    /// never into the layer. Only painting strokes show them.
    pub fn set_prediction(&mut self, stamps: &[Stamp]) {
        self.prediction.clear();
        // (a dual brush's not: drawn over the canvas, it couldn't be cut by its second; nor paint
        // laid in place: drawn over every layer, its stamps show none of what it is laid on, a
        // block of colour that the stroke then isn't)
        let Some(style) = self
            .active
            .filter(|a| a.second.is_none())
            .map(|a| a.style)
            .filter(|s| s.tool == StrokeTool::Paint && !s.paints_in_place())
        else {
            return;
        };
        let capped = self.pipelines.stamp_capped.is_some();
        self.prediction.extend(
            stamps.iter().filter(|s| s.center.is_finite() && s.radius.is_finite() && s.alpha.is_finite()).map(|s| {
                let (alpha, transfer) = capped_alpha(&style, capped, s);
                StampInstance {
                    center: [s.center.x, s.center.y],
                    radius: drawn_radius(&style, s.radius, s.taper),
                    alpha,
                    angle: if s.angle.is_finite() { s.angle } else { 0.0 },
                    roundness: if s.roundness.is_finite() { s.roundness.clamp(0.05, MOST_ROUNDNESS) } else { 1.0 },
                    grain_offset: [0.0; 2],
                    flip: stamp_extras(s).0,
                    tint: stamp_extras(s).1,
                    brightness_add: stamp_extras(s).2,
                    grain_turn: 0.0,
                    look: [s.grain_depth, s.grain_size, s.secondary, s.gradation]
                        .map(|v| if v.is_finite() { v.clamp(0.0, 4096.0) } else { 0.0 }),
                    // the bleed up to 2 (the pressure's and the lean's), the rest 0..1
                    more: [(s.grain_press, 1.0), (s.grain_bleed, 2.0), (transfer, 1.0), (s.attack, 1.0)]
                        .map(|(v, most)| if v.is_finite() { v.clamp(0.0, most) } else { 0.0 }),
                }
            }),
        );
    }

    pub fn end_stroke(&mut self) {
        self.prediction.clear();
        self.untip();
        if let Some(active) = self.active.take() {
            self.ops.push(Op::Commit(active));
        }
    }

    /// Ends the open stroke as [`Canvas::end_stroke`] does, knowing that it changes pixels only
    /// within `rects` (a stroke drawn in from elsewhere, whose reach is known more closely than
    /// its bounds): its undo step keeps only what is under them, and none if they are empty.
    pub fn end_stroke_within(&mut self, rects: Vec<IRect>) {
        self.prediction.clear();
        self.untip();
        if let Some(active) = self.active.take() {
            self.ops.push(Op::Within(rects));
            self.ops.push(Op::Commit(active));
        }
    }

    /// Sends stamps to the GPU in pieces of at most this much work, in place of [`WORK_AT_ONCE`]:
    /// for tests.
    #[doc(hidden)]
    pub fn set_work_at_once(&mut self, work: u64) {
        self.work_at_once = work;
    }

    /// How many pieces of work were sent early, there being too much for one: for tests.
    #[doc(hidden)]
    pub fn pieces_sent(&self) -> u64 {
        self.pieces_sent
    }

    /// What the undo and redo history's pixels take on the GPU, bytes.
    pub fn history_bytes(&self) -> u64 {
        self.undo.iter().chain(&self.redo).map(Step::bytes).sum()
    }

    /// Drops the open stroke.
    pub fn cancel_stroke(&mut self) {
        self.prediction.clear();
        self.tip = None;
        if let Some(active) = self.active.take() {
            self.dirty = self.dirty.union(active.bounds);
            self.ops.push(Op::Cancel);
        }
    }

    /// Undoes the last step; a structural step puts the document back during the next render.
    pub fn undo(&mut self) {
        self.end_stroke();
        self.ops.push(Op::Undo);
    }

    pub fn redo(&mut self) {
        self.end_stroke();
        self.ops.push(Op::Redo);
    }

    pub fn clear_layer(&mut self, layer: LayerId) {
        self.end_stroke();
        self.ops.push(Op::Clear(layer, None));
    }

    /// Inverts a layer's colours within `rect` (where it is painted), as one undo step; on the
    /// GPU, nothing read back.
    pub fn invert_layer(&mut self, layer: LayerId, rect: IRect) {
        self.end_stroke();
        self.ops.push(Op::Invert(layer, rect));
    }

    /// Fills a layer all over with the opaque colour `rgb`, as one undo step; on the GPU, nothing
    /// sent.
    pub fn fill_layer(&mut self, layer: LayerId, rgb: [u8; 3]) {
        self.end_stroke();
        self.ops.push(Op::Fill(layer, rgb));
    }

    /// Clears a layer painted only within `painted` (as [`Canvas::painted_boxes`] finds), the undo
    /// step keeping only that.
    pub fn clear_layer_within(&mut self, layer: LayerId, painted: IRect) {
        self.end_stroke();
        self.ops.push(Op::Clear(layer, Some(painted)));
    }

    /// Replaces a layer's pixels (premultiplied RGBA8, the canvas's size) as one undo step; pixels
    /// of the wrong size are ignored.
    pub fn write_layer(&mut self, layer: LayerId, pixels: Vec<u8>) {
        self.write_layers(vec![(layer, pixels)]);
    }

    /// Replaces a layer's pixels with no undo step; a picture of the wrong size is left out.
    pub fn put_layer(&mut self, layer: LayerId, pixels: Vec<u8>) {
        if pixels.len() == self.width as usize * self.height as usize * 4 {
            self.ops.push(Op::Put(layer, pixels));
            self.invalidate();
        }
    }

    /// Replaces the rectangle of a layer's pixels at (`x`, `y`), `width` x `height` (`pixels` its
    /// RGBA8, row by row) with no undo step, as [`Canvas::put_layer`] does the whole of it. Left
    /// out if it isn't on the canvas or `pixels` isn't its size. Only that rectangle composites
    /// again, around the pictures of the rest when the layer is the one worked on (text typed, a
    /// fill dragged).
    pub fn put_layer_region(&mut self, layer: LayerId, x: u32, y: u32, width: u32, height: u32, pixels: Vec<u8>) {
        let on = width > 0
            && height > 0
            && x.checked_add(width).is_some_and(|r| r <= self.width)
            && y.checked_add(height).is_some_and(|b| b <= self.height);
        if on && pixels.len() == width as usize * height as usize * 4 {
            self.ops.push(Op::PutRegion { layer, x, y, width, height, pixels });
        }
    }

    /// Sets a layer's pixels aside, for an edit made of many changes (with [`Canvas::put_layer`])
    /// that ends with [`Canvas::release_layer`].
    pub fn hold_layer(&mut self, layer: LayerId) {
        self.end_stroke();
        self.ops.push(Op::Hold(layer));
    }

    /// Ends a held edit: `keep` makes it one undo step back to the held pixels and `before` (the
    /// document as the edit began); otherwise the held pixels are put back.
    pub fn release_layer(&mut self, keep: bool, before: Option<Document>) {
        self.ops.push(Op::Release { keep, before: before.map(Box::new) });
        self.invalidate();
    }

    /// Replaces a layer's pixels inside `rect` with `part` (premultiplied RGBA8, the rectangle's
    /// rows) as one undo step keeping only that part. Left out if `rect` isn't wholly on the
    /// canvas or `part` isn't its size.
    pub fn write_layer_part(&mut self, layer: LayerId, rect: IRect, part: Vec<u8>) {
        let fits = !rect.is_empty() && rect.intersect(self.full()) == rect;
        if !fits || part.len() != rect.width() as usize * rect.height() as usize * 4 {
            return;
        }
        self.end_stroke();
        self.ops.push(Op::Write { layers: vec![(layer, rect, part)], before: None });
    }

    /// Mirrors `layers`, left to right (`horizontal`) or top to bottom, as one undo step; on the
    /// GPU, nothing read back.
    pub fn flip_layers(&mut self, layers: Vec<LayerId>, horizontal: bool) {
        self.end_stroke();
        self.ops.push(Op::Flip { layers, horizontal });
    }

    /// Replaces several layers' pixels at once, as one undo step; pictures of the wrong size are
    /// left out.
    pub fn write_layers(&mut self, layers: Vec<(LayerId, Vec<u8>)>) {
        let (size, full) = (self.width as usize * self.height as usize * 4, self.full());
        let layers: Vec<(LayerId, IRect, Vec<u8>)> =
            layers.into_iter().filter(|(_, p)| p.len() == size).map(|(id, p)| (id, full, p)).collect();
        if !layers.is_empty() {
            self.end_stroke();
            self.ops.push(Op::Write { layers, before: None });
        }
    }

    /// Replaces a rectangle of several layers' pixels (premultiplied RGBA8, the rectangle's rows),
    /// the rest of each as it was, and changes the document together, as one undo step back to
    /// the pixels and to `before` (the document as it was); parts that don't fit their rectangle,
    /// or the canvas, are left out.
    pub fn write_layer_parts_and_document(&mut self, layers: Vec<(LayerId, IRect, Vec<u8>)>, before: Document) {
        let full = self.full();
        let fits = |rect: &IRect, part: &[u8]| {
            !rect.is_empty()
                && rect.intersect(full) == *rect
                && part.len() == rect.width() as usize * rect.height() as usize * 4
        };
        let layers: Vec<(LayerId, IRect, Vec<u8>)> = layers.into_iter().filter(|(_, r, p)| fits(r, p)).collect();
        self.end_stroke();
        self.ops.push(Op::Write { layers, before: Some(Box::new(before)) });
        self.invalidate();
    }

    /// Clears a layer without recording it (for scratch canvases such as brush previews).
    pub fn wipe_layer(&mut self, layer: LayerId) {
        self.end_stroke();
        self.ops.push(Op::Wipe(layer));
    }

    /// Drops the undo history (scratch canvases).
    pub fn forget_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Drops the steps (to undo and to redo) that change the document's layers, keeping those
    /// that only put pixels back: the document now follows another's layers (a model's channels
    /// share theirs), which its own such steps would undo and redo out of step with.
    pub fn forget_document_steps(&mut self) {
        self.undo.retain(|s| s.doc.is_none());
        self.redo.retain(|s| s.doc.is_none());
    }

    /// An undo step that changes nothing here, named `name`: a change kept outside the canvas
    /// (which of a model's channels is painted), which its owner follows as the step is undone
    /// and redone.
    pub fn record_outside_change(&mut self, name: &str) {
        self.end_stroke();
        self.ops.push(Op::OutsideChange(name.to_owned()));
    }

    /// The document was changed (layers added, removed, reordered, their settings...): `before`
    /// is how it was, for undo.
    pub fn record_document(&mut self, before: Document) {
        self.end_stroke();
        self.ops.push(Op::Record(Box::new(before)));
        self.invalidate();
    }

    /// `to` (already in the document) was made as a copy of `from`; `before` is the document
    /// without it.
    pub fn duplicate_layer(&mut self, from: LayerId, to: LayerId, before: Document) {
        self.duplicate_layers(vec![(from, to)], before);
    }

    /// Layers (already in the document) were made as copies, each pair (from, to), as one undo
    /// step; `before` is the document without them.
    pub fn duplicate_layers(&mut self, pairs: Vec<(LayerId, LayerId)>, before: Document) {
        self.end_stroke();
        self.ops.push(Op::Duplicate { pairs, before: Box::new(before) });
        self.invalidate();
    }

    /// Blends `upper` into `lower`; the caller removes `upper` from the document once this is
    /// queued (`before` is the document with it). `within`: where `upper` is painted, if known
    /// (the undo step keeps only that much of `lower`).
    pub fn merge_down(&mut self, upper: LayerId, lower: LayerId, before: Document, within: Option<IRect>) {
        self.merge_down_with_masks(upper, lower, Vec::new(), false, before, within);
    }

    /// [`Canvas::merge_down`], each of `masks` (`upper` or `lower`, and its mask) applied to its
    /// layer first in the same undo step, and with `drop_lower` (it is hidden: only what shows is
    /// kept) `lower`'s own pixels let go first; the caller removes the masks with `upper`.
    pub fn merge_down_with_masks(
        &mut self,
        upper: LayerId,
        lower: LayerId,
        masks: Vec<(LayerId, LayerId)>,
        drop_lower: bool,
        before: Document,
        within: Option<IRect>,
    ) {
        self.end_stroke();
        self.ops.push(Op::MergeDown { upper, lower, before: Box::new(before), within, masks, drop_lower });
        self.invalidate();
    }

    /// Layer `layer` takes on its mask `mask` (shown as much as the mask is light) as one undo
    /// step; the caller removes the mask once this is queued (`before` is the document with it).
    pub fn apply_mask(&mut self, layer: LayerId, mask: LayerId, before: Document) {
        self.end_stroke();
        self.ops.push(Op::ApplyMask { layer, mask, before: Box::new(before) });
        self.invalidate();
    }

    /// Blends `uppers` (bottom first: layers, or groups composited first) into `lower` in turn as
    /// one undo step; the caller removes them from the document once this is queued (`before` is
    /// the document with them).
    /// `within`: where they are painted, if known (the undo step keeps only that much of `lower`).
    pub fn merge_layers(&mut self, uppers: Vec<LayerId>, lower: LayerId, before: Document, within: Option<IRect>) {
        self.merge_layers_with_masks(uppers, lower, Vec::new(), false, before, within);
    }

    /// [`Canvas::merge_layers`], each of `masks` (`lower` or one of `uppers`, and its mask)
    /// applied to its layer first in the same undo step, and with `drop_lower` (it is hidden)
    /// `lower`'s own pixels let go first; the caller removes the masks too.
    pub fn merge_layers_with_masks(
        &mut self,
        uppers: Vec<LayerId>,
        lower: LayerId,
        masks: Vec<(LayerId, LayerId)>,
        drop_lower: bool,
        before: Document,
        within: Option<IRect>,
    ) {
        self.end_stroke();
        self.ops.push(Op::MergeMany { uppers, lower, before: Box::new(before), within, masks, drop_lower });
        self.invalidate();
    }

    /// Draws group `group` into `into`, a new layer already in the document. `before` is the
    /// document with the group; the caller changes the document once this is queued.
    pub fn flatten_group(&mut self, group: LayerId, into: LayerId, before: Document) {
        self.end_stroke();
        self.ops.push(Op::Flatten { group, into, before: Box::new(before) });
        self.invalidate();
    }

    /// Recomposite everything next frame (after a change to visibility, opacity, order...).
    /// Whether the composite is made around pictures of the top level's items that aren't being
    /// worked on, kept until something changes them, so that only the items worked on composite
    /// each time (and the others' layers needn't be at hand): for artworks of more layers than
    /// fit at once. Off, every layer composites every time.
    pub fn set_cache_composites(&mut self, on: bool) {
        self.cache_composites = on;
        self.let_go_caches(true);
        if !on {
            self.spare_caches = None;
        }
    }

    /// Lets the caches go (and any remake of them, with `remake`), their textures kept for the
    /// next.
    fn let_go_caches(&mut self, remake: bool) {
        let remade = if remake { self.remake.take().map(|r| r.caches) } else { None };
        if let Some(c) = self.caches.take().or(remade) {
            self.spare_caches = Some(c);
        }
    }

    /// Whether the canvas is still remaking its caches (a little each render): it wants another
    /// render soon, and the composite shows as it was until then.
    pub fn busy(&self) -> bool {
        self.remake.is_some()
    }

    /// Renders until the caches are remade, so the composite is the picture as it is (to read it
    /// back: a save's, an export's).
    pub fn render_finished(&mut self, gpu: &Gpu, doc: &mut Document) {
        self.render(gpu, doc, None);
        // each render remakes at least one item, so this ends well before the bound
        for _ in 0..100_000 {
            if !self.busy() {
                break;
            }
            self.render(gpu, doc, None);
        }
    }

    /// Whether the canvas has work that more renders do (caches being remade, layers being
    /// parked): it wants another render soon.
    pub fn wants_render(&self) -> bool {
        self.busy() || self.parking_left
    }

    /// How the last remake of the caches went (none made yet).
    pub fn last_remake(&self) -> Option<RemakeStats> {
        self.last_remake
    }

    /// Whether the composite is being made around pictures of what isn't worked on.
    pub fn composites_cached(&self) -> bool {
        self.caches.is_some()
    }

    /// Whether a smudge stroke couldn't be begun since this was last asked (what it takes couldn't
    /// be made on the GPU: past its memory, or more than a binding may hold), once.
    pub fn take_smudge_refused(&mut self) -> bool {
        std::mem::take(&mut self.smudge_untold)
    }

    /// What the layers' textures and the history may take, in bytes: past it, the layers needed
    /// least lately that aren't being worked on are parked (kept compressed off the GPU, and the
    /// composite made around pictures of them) until needed again. 0 keeps every layer on the GPU.
    pub fn set_resident_budget(&mut self, bytes: u64) {
        self.resident_budget = bytes;
        self.set_cache_composites(bytes > 0);
    }

    /// What was set with [`Canvas::set_resident_budget`].
    pub fn resident_budget(&self) -> u64 {
        self.resident_budget
    }

    /// A parked layer's small pictures, if they are big enough for a thumbnail `max_side` across.
    fn parked_pictures(&self, id: LayerId, max_side: u32) -> Option<&Pictures> {
        if self.layers.contains_key(&id) || max_side > PARKED_PICTURE_SIDE {
            return None;
        }
        self.pictures.get(&id)
    }

    /// The layers whose pixels changed since this was last done (by painting, undo, uploads and
    /// every other change), forgotten here: a save reads only these.
    pub fn take_changed(&mut self) -> HashSet<LayerId> {
        std::mem::take(&mut self.changed)
    }

    /// Layer `id`'s pixels are changing: counted for the next save, and given a new version.
    fn touched(&mut self, id: LayerId) {
        self.changed.insert(id);
        self.version += 1;
        self.versions.insert(id, self.version);
    }

    /// Layer `id`'s pixels' version as of the last render: it is new whenever they may have
    /// changed (painting, undo, uploads, every other change), so a thumbnail of the same version
    /// is still right.
    pub fn layer_version(&self, id: LayerId) -> u64 {
        self.versions.get(&id).copied().unwrap_or(0)
    }

    /// Layers to count as changed again (a save of them that failed).
    pub fn mark_changed(&mut self, ids: impl IntoIterator<Item = LayerId>) {
        self.changed.extend(ids);
    }

    /// How many layers are parked (kept compressed, without a texture).
    pub fn parked_layers(&self) -> usize {
        self.parked.keys().filter(|id| !self.layers.contains_key(*id)).count()
    }

    /// Gives layer `id` its texture again from its compressed pixels, if it is parked; whether
    /// it has one now. Its pixels go up before any work recorded in this render.
    fn restore(&mut self, gpu: &Gpu, id: LayerId) -> bool {
        if self.layers.contains_key(&id) {
            return true;
        }
        let Some((painted, rows)) = self.parked.get(&id).and_then(|p| Some((p.rect, p.rows()?))) else {
            return false;
        };
        let texture = texture(&gpu.device, "layer", self.width, self.height, LAYER_FORMAT);
        // only where it is painted (a new texture is clear): nothing at all for an empty layer
        if let Some(b) = painted {
            self.upload_now(gpu, &texture, b, &rows, 0, b.width() as usize * 4);
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.layers.insert(id, LayerGpu { texture, view });
        true
    }

    /// Where an item's layers are painted, as far as is known (the whole canvas for a layer
    /// with a texture of its own; empty if every one is parked and empty).
    fn painted(&self, layers: &[LayerId]) -> IRect {
        layers.iter().fold(IRect::EMPTY, |r, id| {
            match self.pictures.get(id).filter(|_| !self.layers.contains_key(id)) {
                Some(p) => p.bounds.map_or(r, |b| r.union(b)),
                None => self.full(),
            }
        })
    }

    /// Every layer in `ids` given its texture again; those about to be written lose their
    /// compressed copies.
    fn restore_all(&mut self, gpu: &Gpu, ids: impl IntoIterator<Item = LayerId>, written: bool) {
        for (n, id) in ids.into_iter().enumerate() {
            let restored = !self.layers.contains_key(&id) && self.restore(gpu, id);
            if written {
                self.parked.remove(&id);
                self.pictures.remove(&id);
                self.touched(id);
            }
            self.needed_at.insert(id, self.renders);
            // the staged pixels sent on now and then, so many in a row don't pile up
            if restored && n % 8 == 7 {
                gpu.queue.submit(std::iter::empty());
            }
        }
    }

    /// The layers the queued work reads and those it writes (which need their textures and lose
    /// their compressed copies).
    fn ops_layers(&self) -> (Vec<LayerId>, Vec<LayerId>) {
        let (mut read, mut written) = (Vec::new(), Vec::new());
        let contents = |d: &Document, id: LayerId| -> Vec<LayerId> {
            if d.layer(id).is_some_and(|l| l.kind == LayerKind::Group) { d.contents(id) } else { vec![id] }
        };
        for op in &self.ops {
            match op {
                Op::Commit(s) => written.push(s.layer),
                Op::Clear(id, _)
                | Op::Invert(id, _)
                | Op::Fill(id, _)
                | Op::Wipe(id)
                | Op::TransformBegin(id)
                | Op::TransformAlso(id)
                | Op::Put(id, _)
                | Op::PutRegion { layer: id, .. }
                | Op::Hold(id) => written.push(*id),
                Op::Write { layers, .. } => written.extend(layers.iter().map(|(id, ..)| *id)),
                Op::Flip { layers, .. } => written.extend(layers.iter().copied()),
                Op::Duplicate { pairs, .. } => {
                    read.extend(pairs.iter().map(|(from, _)| *from));
                    written.extend(pairs.iter().map(|(_, to)| *to));
                }
                Op::MergeDown { upper, lower, before, masks, .. } => {
                    read.extend(contents(before, *upper));
                    read.extend(masks.iter().map(|(_, m)| *m));
                    written.extend(masks.iter().map(|(l, _)| *l));
                    written.push(*lower);
                }
                Op::MergeMany { uppers, lower, before, masks, .. } => {
                    read.extend(uppers.iter().flat_map(|u| contents(before, *u)));
                    read.extend(masks.iter().map(|(_, m)| *m));
                    written.extend(masks.iter().map(|(l, _)| *l));
                    written.push(*lower);
                }
                Op::ApplyMask { layer, mask, .. } => {
                    read.extend([*layer, *mask]);
                    written.push(*layer);
                }
                Op::Flatten { group, into, before, .. } => {
                    read.extend(before.contents(*group));
                    written.push(*into);
                }
                Op::Release { .. } => written.extend(self.held.as_ref().map(|h| h.layer)),
                _ => {}
            }
        }
        written.extend(self.active.map(|a| a.layer));
        (read, written)
    }

    /// Parks the layers needed least lately until the textures fit the budget, keeping those
    /// worked on (and what they are clipped to); none while the composite isn't made around
    /// pictures of the others.
    fn park_extra(&mut self, gpu: &Gpu, doc: &Document) {
        self.parking_left = false;
        let most = self.most_resident();
        if self.resident_budget == 0 || self.layers.len() <= most {
            return;
        }
        let mut keep = match (&self.caches, self.shown_frames(doc)) {
            (Some(c), _) => {
                let mut keep = c.live_layers.clone();
                keep.extend(c.live.first().into_iter().flat_map(|first| clip_base_and_mask(doc, *first)));
                keep
            }
            // the frames shown, while Animation Assist is on
            (None, Some(shown)) => shown,
            (None, None) => return,
        };
        keep.extend(self.active.map(|a| a.layer));
        keep.extend(self.held.as_ref().map(|h| h.layer));
        let mut gone: Vec<LayerId> = self.layers.keys().filter(|id| !keep.contains(*id)).copied().collect();
        gone.sort_by_key(|id| self.needed_at.get(id).copied().unwrap_or(0));
        // a layer with no compressed copy yet is read back first, the slow part: a few a render
        // (at least one), the rest left for the renders after
        let started = std::time::Instant::now();
        let mut reads = 0;
        for id in gone {
            if self.layers.len() <= most {
                break;
            }
            if !self.parked.contains_key(&id) {
                if reads > 0 && (reads >= MOST_PARK_READS || started.elapsed() >= PARK_SLICE) {
                    self.parking_left = true;
                    break;
                }
                reads += 1;
                let Ok(pixels) = self.read_layer(gpu, id) else { continue };
                let pictures = Pictures::of(&pixels, self.width, self.height);
                self.parked.insert(id, Parked::of(&pixels, self.width, pictures.bounds));
                self.pictures.insert(id, pictures);
            }
            self.layers.remove(&id);
        }
    }

    pub fn invalidate(&mut self) {
        self.let_go_caches(true);
        self.dirty = self.full();
    }

    /// Animation Assist playing: frame `frame` shows alone (none: back to drawing).
    pub fn set_play_frame(&mut self, frame: Option<usize>) {
        if self.play_frame != frame {
            self.play_frame = frame;
            self.invalidate();
        }
    }

    /// Sets the selection: one byte of coverage per canvas pixel, rows top to bottom; none (or a
    /// buffer of the wrong size) selects everything. Painting and erasing keep inside it.
    pub fn set_selection(&mut self, gpu: &Gpu, coverage: Option<&[u8]>) {
        self.put_selection(&gpu.device, &gpu.queue, coverage.map(std::borrow::Cow::Borrowed));
    }

    /// Sets the selection as [`Self::set_selection`] does, as one undo step that puts `before` back
    /// (the coverage it had, or none). Runs what is queued first, so the step falls in order.
    pub fn change_selection(
        &mut self,
        gpu: &Gpu,
        doc: &mut Document,
        coverage: Option<&[u8]>,
        before: Option<Vec<u8>>,
    ) {
        self.render(gpu, doc, None);
        self.push_step(Step {
            made: 0,
            name: None,
            doc: None,
            pixels: Vec::new(),
            selection: Some(before),
            flip: None,
        });
        self.set_selection(gpu, coverage);
    }

    /// Layer `layer`'s pixels as they are on the GPU, if it has any there.
    pub fn layer_view(&self, layer: LayerId) -> Option<&wgpu::TextureView> {
        self.layers.get(&layer).map(|l| &l.view)
    }

    /// Replaces layer `layer`'s pixels with `source` (premultiplied, the layer's format and the
    /// canvas's size), on the GPU, with no undo step. False if the layer has none there yet.
    pub fn copy_to_layer(&mut self, gpu: &Gpu, layer: LayerId, source: &wgpu::Texture) -> bool {
        let Some(target) = self.layers.get(&layer) else { return false };
        let mut encoder =
            gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("to layer") });
        encoder.copy_texture_to_texture(
            source.as_image_copy(),
            target.texture.as_image_copy(),
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([encoder.finish()]);
        self.invalidate();
        true
    }

    /// The selection's texture (coverage in its red), or none when nothing is selected.
    pub fn selection_view(&self) -> Option<&wgpu::TextureView> {
        self.selection.as_ref().map(|s| &s.view)
    }

    /// The selection's coverage, one byte a canvas pixel, or none when nothing is selected.
    pub fn selection(&self) -> Option<&[u8]> {
        self.selection.as_ref().map(|s| s.coverage.as_slice())
    }

    fn put_selection(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        coverage: Option<std::borrow::Cow<'_, [u8]>>,
    ) {
        let size = self.width as usize * self.height as usize;
        let coverage = coverage.filter(|c| c.len() == size);
        // a selection already shown takes the new coverage in its own texture (a slider drags it
        // a frame at a time), its bindings as they are; only the rows that changed are sent
        if let (Some(coverage), Some(shown)) = (&coverage, self.selection.as_mut()) {
            let row = (self.width as usize).max(1);
            let differs = |(a, b): (&[u8], &[u8])| a != b;
            let rows = || shown.coverage.chunks(row).zip(coverage.chunks(row));
            let first = rows().position(differs);
            let last = rows().rposition(differs);
            if let (Some(first), Some(last)) = (first, last) {
                let changed = coverage.get(first * row..(last + 1) * row).unwrap_or_default();
                queue.write_texture(
                    copy_at(&shown.texture, 0, i32::try_from(first).unwrap_or(0)),
                    changed,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(self.width),
                        rows_per_image: Some(u32::try_from(last + 1 - first).unwrap_or(0)),
                    },
                    extent(self.width, u32::try_from(last + 1 - first).unwrap_or(0)),
                );
                if let Some(kept) = shown.coverage.get_mut(first * row..(last + 1) * row) {
                    kept.copy_from_slice(changed);
                }
            }
            return;
        }
        self.selection = coverage.map(|coverage| {
            let texture = coverage_texture(device, queue, self.width, self.height, &coverage);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind = sampled_bind(device, &self.pipelines.sampled_layout, &view, &self.sampler);
            SelectionGpu { coverage: coverage.into_owned(), texture, view, bind }
        });
        let mask = self.selection.as_ref().map_or(&self.everything, |s| &s.view);
        self.brush_bind = brush_bind(
            device,
            &self.pipelines.brush_layout,
            [&self.shapes_view, &self.grains_view, mask],
            [&self.shape_sampler, &self.grain_sampler],
        );
    }

    /// The 256 entries (RGBA8, 1024 bytes) the curves and gradient map adjustments read: for curves
    /// the red, green and blue curves and the curve for all three; for a gradient map its colours
    /// from dark to light. Other lengths are ignored.
    pub fn set_table(&mut self, gpu: &Gpu, entries: &[u8]) {
        if entries.len() == 1024 {
            gpu.queue.write_texture(
                copy_at(&self.table, 0, 0),
                entries,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1024), rows_per_image: Some(1) },
                extent(256, 1),
            );
        }
    }

    pub fn has_selection(&self) -> bool {
        self.selection.is_some()
    }

    /// Copies `layer` aside to transform it (ending a transform still open, kept).
    pub fn begin_transform(&mut self, layer: LayerId) {
        self.end_stroke();
        if self.is_transforming() {
            self.end_transform(true);
        }
        self.ops.push(Op::TransformBegin(layer));
    }

    /// Starts transforming `layers` together (the first as [`Self::begin_transform`] does).
    pub fn begin_transform_layers(&mut self, layers: &[LayerId]) {
        let Some((first, rest)) = layers.split_first() else { return };
        self.begin_transform(*first);
        self.ops.extend(rest.iter().map(|id| Op::TransformAlso(*id)));
    }

    /// Redraws the layer being transformed from its copy: `to_source` maps layer pixel
    /// coordinates to copy pixel coordinates (3 × 3, row-major).
    pub fn set_transform(&mut self, to_source: [f32; 9], resample: Resample) {
        if to_source.iter().all(|v| v.is_finite()) {
            self.replace_placement(Op::TransformSet { to_source, resample });
        }
    }

    /// Redraws the layer being transformed from its copy as a warp: `vertices` are triangles, three
    /// corners each of (layer x, y, copy x, y) in pixels.
    pub fn set_transform_mesh(&mut self, vertices: &[[f32; 4]], resample: Resample) {
        if vertices.iter().flatten().all(|v| v.is_finite()) {
            self.replace_placement(Op::TransformMesh { vertices: vertices.to_vec(), resample });
        }
    }

    /// Starts liquifying the layer being edited (begun with [`Self::begin_transform`]): a field of
    /// no offsets, which [`Self::liquify_stamps`] draws into and [`Filter::Liquify`] shows.
    pub fn begin_liquify(&mut self) {
        self.ops.push(Op::LiquifyBegin);
    }

    /// Layer `layer` is edited along with the one begun with [`Self::begin_transform`]: liquify
    /// pushes it the same way (a layer's mask), and the edit's end keeps or puts back both.
    pub fn transform_also(&mut self, layer: LayerId) {
        self.ops.push(Op::TransformAlso(layer));
    }

    /// The adjustment being edited shows only where it is painted on from now on: strokes begun with
    /// [`Self::begin_pencil_stroke`] paint where (within the selection).
    pub fn begin_pencil(&mut self) {
        self.ops.push(Op::PencilBegin);
    }

    /// Whether an adjustment is being painted on, counting queued work.
    pub fn is_pencil(&self) -> bool {
        let mut on = self.pencil.is_some();
        for op in &self.ops {
            match op {
                Op::PencilBegin => on = true,
                Op::TransformEnd { .. } => on = false,
                _ => {}
            }
        }
        on
    }

    /// A stroke that paints the adjustment on (see [`Self::begin_pencil`]); stamps are added as for
    /// any stroke and [`Self::end_stroke`] ends it.
    pub fn begin_pencil_stroke(&mut self, layer: LayerId, style: StrokeStyle) {
        self.end_stroke();
        self.prediction.clear();
        self.active = Some(ActiveStroke { layer, style, bounds: IRect::EMPTY, pencil: true, second: None });
    }

    /// A step of the liquify brush: `stamps` added up, then the field carried along by them.
    pub fn liquify_stamps(&mut self, stamps: &[LiquifyStamp]) {
        if !stamps.is_empty() {
            self.ops.push(Op::LiquifyStamps(stamps.to_vec()));
        }
    }

    /// The liquify field kept at `amount` of itself.
    pub fn liquify_scale(&mut self, amount: f32) {
        self.ops.push(Op::LiquifyScale(amount));
    }

    /// The liquify field back to no offsets.
    pub fn liquify_clear(&mut self) {
        self.ops.push(Op::LiquifyClear);
    }

    /// Layer `id` copied aside to be transformed (or adjusted) from.
    fn copy_aside(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        id: LayerId,
    ) -> Option<Transforming> {
        let layer = self.layers.get(&id)?;
        let before = self.spare_before.take().unwrap_or_else(|| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("transform source"),
                size: extent(self.width, self.height),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: LAYER_FORMAT,
                usage: wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        });
        encoder.copy_texture_to_texture(
            copy_at(&layer.texture, 0, 0),
            copy_at(&before, 0, 0),
            extent(self.width, self.height),
        );
        let view = before.create_view(&wgpu::TextureViewDescriptor::default());
        let masked = self.selection.is_some();
        let chosen = self.selection.as_ref().map_or(&self.everything, |s| &s.view);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("transform source"),
            layout: &self.pipelines.transform_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(chosen) },
            ],
        });
        let before_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("adjustment copy"),
            layout: &self.pipelines.before_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.table_view) },
            ],
        });
        Some(Transforming { layer: id, before, bind, masked, before_bind })
    }

    /// The edit's copy and `second` bound as an adjustment's last-pass group.
    fn copy_bind_with(&self, device: &wgpu::Device, second: &wgpu::TextureView) -> wgpu::BindGroup {
        let fallback;
        let copy = match &self.transforming {
            Some(t) => {
                fallback = t.before.create_view(&wgpu::TextureViewDescriptor::default());
                &fallback
            }
            None => &self.everything,
        };
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("adjustment copy"),
            layout: &self.pipelines.before_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(copy) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(second) },
            ],
        })
    }

    /// Redraws the layer being edited (begun with [`Self::begin_transform`]) from its copy through
    /// `filter`; [`Self::end_transform`] keeps it or puts the layer back.
    pub fn set_filter(&mut self, filter: Filter) {
        self.replace_placement(Op::FilterSet(filter));
    }

    /// Queues a redraw of the layer being transformed; only the latest needs drawing.
    fn replace_placement(&mut self, op: Op) {
        if let Some(Op::TransformSet { .. } | Op::TransformMesh { .. } | Op::FilterSet(_) | Op::PluginFilter(_)) =
            self.ops.last()
        {
            self.ops.pop();
        }
        self.ops.push(op);
    }

    /// Ends the transform: `commit` keeps it as one undo step, otherwise the layer is put back.
    pub fn end_transform(&mut self, commit: bool) {
        self.ops.push(Op::TransformEnd { commit, within: None });
    }

    /// Ends the transform as [`Canvas::end_transform`] does, the layers changed only inside
    /// `within`: an undo step keeps only that part of them.
    pub fn end_transform_within(&mut self, commit: bool, within: IRect) {
        self.ops.push(Op::TransformEnd { commit, within: Some(within) });
    }

    /// Whether a transform is open, counting queued work.
    pub fn is_transforming(&self) -> bool {
        let mut open = self.transforming.is_some();
        for op in &self.ops {
            match op {
                Op::TransformBegin(_) => open = true,
                Op::TransformEnd { .. } => open = false,
                _ => {}
            }
        }
        open
    }

    /// Creates textures for layers that have none yet: the document's and those queued work
    /// refers to (a layer can be added, painted and merged away before a frame is drawn).
    fn sync_layers(&mut self, gpu: &Gpu, doc: &Document) {
        let mut ids: Vec<LayerId> = doc.pixel_layer_ids().collect();
        for op in &self.ops {
            match op {
                Op::Record(d)
                | Op::Duplicate { before: d, .. }
                | Op::MergeDown { before: d, .. }
                | Op::MergeMany { before: d, .. }
                | Op::ApplyMask { before: d, .. }
                | Op::Flatten { before: d, .. } => ids.extend(d.pixel_layer_ids()),
                Op::Commit(s) => ids.push(s.layer),
                Op::Clear(id, _)
                | Op::Invert(id, _)
                | Op::Fill(id, _)
                | Op::Wipe(id)
                | Op::TransformBegin(id)
                | Op::TransformAlso(id) => {
                    ids.push(*id);
                }
                Op::Write { layers, .. } => ids.extend(layers.iter().map(|(id, ..)| *id)),
                Op::Flip { layers, .. } => ids.extend(layers.iter().copied()),
                Op::Put(id, _) | Op::PutRegion { layer: id, .. } | Op::Hold(id) => ids.push(*id),
                Op::Release { before: Some(d), .. } => ids.extend(d.pixel_layer_ids()),
                _ => {}
            }
            if let Op::Duplicate { pairs, .. } = op {
                ids.extend(pairs.iter().flat_map(|(from, to)| [*from, *to]));
            }
            if let Op::MergeDown { upper, lower, .. } = op {
                ids.extend([*upper, *lower]);
            }
            if let Op::MergeMany { uppers, lower, .. } = op {
                ids.extend(uppers.iter().copied().chain([*lower]));
            }
            if let Op::Flatten { into, .. } = op {
                ids.push(*into);
            }
        }
        if let Some(s) = self.active {
            ids.push(s.layer);
        }
        let (read, written) = self.ops_layers();
        self.restore_all(gpu, read, false);
        self.restore_all(gpu, written, true);
        let mut added = false;
        // spares cleared before this render's work
        let mut clears: Option<wgpu::CommandEncoder> = None;
        for id in ids {
            if self.layers.contains_key(&id) || self.parked.contains_key(&id) {
                continue;
            }
            // a kept texture, cleared, else a new one (new textures are zeroed: transparent)
            let spare = self.take_spare();
            let reused = spare.is_some();
            let texture = spare.unwrap_or_else(|| texture(&gpu.device, "layer", self.width, self.height, LAYER_FORMAT));
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            if reused {
                let encoder = clears.get_or_insert_with(|| gpu.device.create_command_encoder(&Default::default()));
                drop(render_pass(encoder, "spare cleared", &view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)));
            }
            self.layers.insert(id, LayerGpu { texture, view });
            added = true;
        }
        if let Some(encoder) = clears {
            gpu.queue.submit([encoder.finish()]);
        }
        if added {
            self.invalidate();
        }
    }

    /// Drops the textures of layers that neither the document nor the history refers to.
    fn collect_layers(&mut self, doc: &Document) {
        let mut keep: HashSet<LayerId> = doc.layer_ids().collect();
        for step in self.undo.iter().chain(self.redo.iter()) {
            if let Some(d) = &step.doc {
                keep.extend(d.layer_ids());
            }
            keep.extend(step.pixels.iter().map(|s| s.layer));
        }
        for op in &self.ops {
            if let Op::Record(d)
            | Op::Duplicate { before: d, .. }
            | Op::MergeDown { before: d, .. }
            | Op::MergeMany { before: d, .. }
            | Op::ApplyMask { before: d, .. }
            | Op::Flatten { before: d, .. }
            | Op::Release { before: Some(d), .. }
            | Op::Write { before: Some(d), .. } = op
            {
                keep.extend(d.layer_ids());
            }
        }
        let gone: Vec<LayerId> = self.layers.keys().filter(|id| !keep.contains(id)).copied().collect();
        for id in gone {
            if let Some(layer) = self.layers.remove(&id) {
                self.keep_spare(layer.texture);
            }
        }
        self.parked.retain(|id, _| keep.contains(id));
        self.pictures.retain(|id, _| keep.contains(id));
        self.needed_at.retain(|id, _| keep.contains(id));
    }

    /// Records and submits everything queued, recomposites what changed and draws to `target`.
    /// Undoing a structural step puts the earlier document into `doc`.
    pub fn render(&mut self, gpu: &Gpu, doc: &mut Document, target: Option<Target<'_>>) {
        self.sync_layers(gpu, doc);
        // the stroke's bind goes with the open stroke's grain
        let full_grain = self.active.as_ref().and_then(|a| a.style.full_grain);
        if full_grain != self.bound_full_grain {
            self.bound_full_grain = full_grain;
            self.bind_brush_images(&gpu.device);
        }
        if !self.brush_images && self.wants_brush_images() {
            self.prepare_brush_images(gpu);
        }
        let device = &gpu.device;
        // groups flattened or merged are composited on their own, whatever they are
        let flattening = self.ops.iter().filter_map(|op| match op {
            Op::Flatten { before, .. } | Op::MergeMany { before, .. } => Some(before.max_depth()),
            _ => None,
        });
        let depth = flattening.max().unwrap_or(0);
        self.make_groups(device, depth);
        let ops = in_pieces(without_passing_tips(std::mem::take(&mut self.ops)), self.full(), self.work_at_once);

        // this frame's stamps in one upload
        let mut instances: Vec<StampInstance> = Vec::new();
        for op in &ops {
            if let Op::Stamps { instances: i, .. } = op {
                instances.extend_from_slice(i);
            }
        }
        // the predicted tail goes last, drawn with the canvas
        let predicted_first = u32::try_from(instances.len()).unwrap_or(u32::MAX);
        if target.is_some() {
            instances.extend_from_slice(&self.prediction);
        }
        let predicted = predicted_first..u32::try_from(instances.len()).unwrap_or(u32::MAX);
        let needed = instances.len() as u64;
        if needed > self.stamp_capacity {
            self.stamp_capacity = needed.next_power_of_two();
            self.stamp_buffer = stamp_buffer(device, self.stamp_capacity);
        }
        if !instances.is_empty() {
            gpu.queue.write_buffer(&self.stamp_buffer, 0, bytemuck::cast_slice(&instances));
        }

        // the display takes four slots, and frosted panels three and one for each
        let frosted = target.as_ref().and_then(|t| t.backdrop).map_or(0, |b| b.regions.len() + b.windows.len() + 3);
        // remaking the caches composites the layers once more
        let cached = if self.cache_composites { doc.layers().len() + 2 } else { 0 };
        let worked: usize = ops.iter().map(|op| self.slots_for(op, &ops)).sum();
        let slots = (worked + doc.layers().len() + 4 + frosted + cached) as u64;
        if slots > self.uniform_slots {
            self.uniform_slots = slots.next_power_of_two();
            let (buffer, bind) = uniforms(device, &self.pipelines.uniform_layout, self.uniform_slots);
            self.uniform_buffer = buffer;
            self.uniform_bind = bind;
        }
        // the slots stamps painted in place take: a stamp's own and, for wet paint, its pass's (at
        // most one a stamp, the stamps held back from before among them). Paint laid apart is put
        // together after each lot of stamps, each lot of its second's, and twice as a stroke ends.
        // Too few and none of it is written: the stroke, started again, comes out with nothing
        let held = self.smudge.as_ref().map_or(0, |s| s.pending.len() as u64);
        let (smudge_count, wet_count) = ops.iter().fold((2 * held + 1, held + 1), |(slots, passes), op| match op {
            Op::Stamps { second: true, .. } => (slots + 1, passes),
            Op::Stamps { instances, style, .. } if style.in_place() => {
                let stamps = instances.len() as u64;
                (slots + 2 * stamps + 1, passes + stamps)
            }
            Op::Commit(stroke) if stroke.style.in_place() => (slots + 2, passes),
            _ => (slots, passes),
        });
        if smudge_count > self.smudge_capacity {
            self.smudge_capacity = smudge_count.next_power_of_two();
            self.smudge_params = smudge_params(device, self.smudge_capacity);
        }
        if wet_count > self.wet_capacity {
            self.wet_capacity = wet_count.next_power_of_two();
            self.wet_params = wet_params(device, self.wet_capacity);
        }
        let mut uniforms = UniformData::default();
        let mut smudge_uniforms = UniformData::default();
        let mut wet_uniforms = UniformData::default();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("canvas") });

        let mut next_instance = 0u32;
        let mut history_changed = false;
        // the upload buffers this render's work copies from, given back once it is sent
        let mut uploaded: Vec<wgpu::Buffer> = Vec::new();
        // what is worked on, for the caches: other work drops them
        let live = self.caches.as_ref().or(self.remake.as_ref().map(|r| &r.caches)).map(|c| c.live_layers.clone());
        // the layers a stroke was put into in this render's work so far, and whether the rest of
        // the work waits for a render of its own
        let mut committed: Vec<LayerId> = Vec::new();
        let mut deferred = false;
        // stamps' work recorded and not sent yet
        let mut recorded = 0u64;
        let mut ops = ops.into_iter();
        while let Some(op) = ops.next() {
            if live.as_ref().is_some_and(|live| !self.keeps_caches(&op, live)) {
                self.let_go_caches(true);
            }
            // on OpenGL (ES) a stroke painting in place, which starts by copying its layer, waits
            // for a render of its own when a stroke went into that layer in this one: some
            // drivers' copy reads the layer as it was before (wet paint then picks up nothing)
            if gpu.is_gl()
                && let Op::Stamps { style, layer, .. } = &op
                && style.in_place()
                && self.smudge.as_ref().is_none_or(|s| s.layer != *layer)
                && committed.contains(layer)
            {
                let rest: Vec<Op> = std::iter::once(op).chain(ops.by_ref()).collect();
                self.ops.splice(0..0, rest);
                deferred = true;
                break;
            }
            // what is recorded goes first if this would make it too much for one piece
            if let Op::Stamps { instances, style, second, .. } = &op {
                let work = stamps_work(instances, self.full(), style.in_place() && !second);
                if recorded > 0 && recorded.saturating_add(work) > self.work_at_once {
                    self.send_piece(gpu, &mut encoder, (&mut uniforms, &mut smudge_uniforms, &mut wet_uniforms));
                    recorded = 0;
                }
                recorded = recorded.saturating_add(work);
            }
            if let Op::Commit(stroke) = &op {
                committed.push(stroke.layer);
            }
            // a stroke ends: the next has a second only if it says so
            if matches!(op, Op::Commit(_) | Op::Cancel) {
                self.queued_second = None;
                self.smudge_refused = false;
            }
            match op {
                Op::Stamps { instances, wet, times, style, layer, clear, second: false, .. } if style.in_place() => {
                    next_instance = next_instance.saturating_add(u32::try_from(instances.len()).unwrap_or(u32::MAX));
                    let stamps = (instances.as_slice(), wet.as_slice(), times.as_slice());
                    let uniforms = (&mut smudge_uniforms, &mut wet_uniforms);
                    self.smudge_stamps(device, &mut encoder, uniforms, stamps, &style, layer, clear);
                }
                Op::DropSecond => {
                    self.queued_second = None;
                    // paint laid apart is laid alone from here
                    if let Some(apart) = self.smudge.as_mut().and_then(|s| s.apart.as_mut()) {
                        apart.1 = 0;
                    }
                }
                Op::ClearSecond(second) => {
                    self.queued_second = Some(second);
                    let (_, view) = self.second_stroke.get(device);
                    drop(render_pass(
                        &mut encoder,
                        "second clear",
                        view,
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    ));
                }
                Op::Stamps { instances, style, clear, pencil, second, .. } => {
                    let count = u32::try_from(instances.len()).unwrap_or(u32::MAX);
                    let first = next_instance;
                    next_instance = next_instance.saturating_add(count);
                    let (grain, grain_scale, grain_depth) =
                        style.moving_grain().map_or((-1, 1.0, 0.0), |(g, s, d)| (g as i32, s, d));
                    let own_grain = self.full_grain_bind(device, &style);
                    let offset = uniforms.push(&StampUniform {
                        canvas_size: [self.width as f32, self.height as f32],
                        hardness: style.hardness.clamp(0.0, 1.0),
                        shape: style.shape.map_or(-1, |s| s as i32),
                        color: [style.color[0], style.color[1], style.color[2], 1.0],
                        grain,
                        grain_scale,
                        grain_depth,
                        grain_brightness: style.grain_brightness,
                        grain_contrast: style.grain_contrast,
                        grain_rotation: style.grain_rotation,
                        grain_blend: style.grain_blend,
                        grain_sharp: u32::from(style.grain_sharp),
                        shape_sharp: u32::from(style.shape_sharp),
                        shape_lod_cap: style.shape_lod_cap,
                        grain_side: style.grain_side,
                        _pad: 0,
                        secondary: [style.secondary[0], style.secondary[1], style.secondary[2], 1.0],
                    });
                    let load = if clear { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) } else { wgpu::LoadOp::Load };
                    let target = match (&self.pencil, pencil) {
                        _ if second => self.second_stroke.get(device).1,
                        (Some(mask), true) => &mask.view,
                        _ => &self.stroke_view,
                    };
                    let mut pass = render_pass(&mut encoder, "stamps", target, load);
                    if count > 0 {
                        pass.set_pipeline(match style.rendering {
                            Rendering::BuildUp | Rendering::Gradual => &self.pipelines.stamp,
                            Rendering::Strongest => &self.pipelines.stamp_max,
                            Rendering::Capped => self.pipelines.stamp_capped.as_ref().unwrap_or(&self.pipelines.stamp),
                        });
                        pass.set_bind_group(0, &self.uniform_bind, &[offset]);
                        pass.set_bind_group(1, own_grain.as_ref().unwrap_or(&self.brush_bind), &[]);
                        let start = u64::from(first) * size_of::<StampInstance>() as u64;
                        let end = u64::from(next_instance) * size_of::<StampInstance>() as u64;
                        pass.set_vertex_buffer(0, self.stamp_buffer.slice(start..end));
                        pass.draw(0..6, 0..count);
                    }
                    drop(pass);
                    // a dual brush's paint laid in place: put together again where the second went
                    if second && self.smudge.as_ref().is_some_and(|s| s.apart.is_some()) {
                        let rect = instances.iter().fold(IRect::EMPTY, |r, s| {
                            let at = procraft_engine::geom::Vec2::new(s.center[0], s.center[1]);
                            r.union(IRect::around(at, s.radius * s.roundness.max(1.0) * std::f32::consts::SQRT_2))
                        });
                        self.synthesize(device, &mut encoder, &mut smudge_uniforms, rect);
                    }
                }
                Op::Commit(stroke) if stroke.style.in_place() => {
                    self.within = None;
                    // one that couldn't be begun made no step, nor takes the name of one
                    if self.smudge.is_none() {
                        self.next_name = None;
                    }
                    // wet paint's last frame, held back for the rest of it
                    let held = self.smudge.as_mut().map(|s| std::mem::take(&mut s.pending)).unwrap_or_default();
                    let uniforms = (&mut smudge_uniforms, &mut wet_uniforms);
                    self.lay_wet(device, &mut encoder, uniforms, &held, &stroke.style);
                    // a dual brush's paint laid apart, put together over the whole stroke once more
                    // (its second's first stamps may have come before the paint laid apart began)
                    self.synthesize(device, &mut encoder, &mut smudge_uniforms, stroke.bounds);
                    self.finish_smudge(device, &mut encoder, stroke.bounds, false);
                    history_changed = true;
                }
                // painting an adjustment on leaves the layer to the adjustment
                Op::Commit(stroke) if stroke.pencil => {
                    self.within = None;
                    if let Some(mask) = self.pencil.as_mut() {
                        mask.bounds = mask.bounds.union(stroke.bounds);
                    }
                }
                Op::Commit(stroke) => {
                    self.commit(device, &mut encoder, &mut uniforms, stroke);
                    history_changed = true;
                }
                Op::Cancel if self.smudge.is_some() => {
                    self.finish_smudge(device, &mut encoder, IRect::EMPTY, true);
                }
                Op::Cancel => {
                    drop(render_pass(
                        &mut encoder,
                        "cancel",
                        &self.stroke_view,
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    ));
                }
                Op::SaveTip(rect) => {
                    let (width, height) = (rect.width(), rect.height());
                    let roomy = self.tip_backup.as_ref().is_some_and(|b| b.width() >= width && b.height() >= height);
                    if !roomy {
                        // grown in steps, so a tip that grows doesn't make one every frame
                        let room = |n: u32, most: u32| n.max(1).next_multiple_of(256).min(most);
                        let (w, h) = self.tip_backup.as_ref().map_or((0, 0), |b| (b.width(), b.height()));
                        let size = (room(width.max(w), self.width), room(height.max(h), self.height));
                        self.tip_backup = Some(texture(device, "tip backup", size.0, size.1, STROKE_FORMAT));
                    }
                    if let Some(backup) = &self.tip_backup {
                        encoder.copy_texture_to_texture(
                            copy_at(&self.stroke, rect.x0, rect.y0),
                            copy_at(backup, 0, 0),
                            extent(width, height),
                        );
                    }
                }
                Op::Untip(rect) => {
                    if let Some(backup) = &self.tip_backup {
                        encoder.copy_texture_to_texture(
                            copy_at(backup, 0, 0),
                            copy_at(&self.stroke, rect.x0, rect.y0),
                            extent(rect.width(), rect.height()),
                        );
                    }
                }
                Op::Undo => {
                    if let Some(step) = self.undo.pop() {
                        let inverse = self.apply_step(gpu, &mut encoder, doc, step);
                        self.redo.push(inverse);
                        history_changed = true;
                    }
                }
                Op::Redo => {
                    if let Some(step) = self.redo.pop() {
                        let inverse = self.apply_step(gpu, &mut encoder, doc, step);
                        self.undo.push(inverse);
                        history_changed = true;
                    }
                }
                Op::Within(rects) => self.within = Some(rects),
                Op::Mark => self.mark = Some(self.steps_made),
                Op::Name(name) => self.next_name = Some(name),
                Op::Collapse => {
                    let since = self.mark.take().unwrap_or(u64::MAX);
                    let first = self.undo.iter().position(|s| s.made > since).unwrap_or(self.undo.len());
                    let flips = self.undo.get(first..).is_some_and(|steps| steps.iter().any(|s| s.flip.is_some()));
                    if self.undo.len() - first >= 2 && !flips {
                        let steps = self.undo.split_off(first);
                        self.undo.push(collapsed(steps));
                        history_changed = true;
                    }
                }
                Op::Invert(id, rect) => {
                    let rect = rect.intersect(self.full());
                    if !rect.is_empty()
                        && let Some(snapshot) = self.snapshot(device, &mut encoder, id, rect)
                    {
                        self.push_step(Step {
                            made: 0,
                            name: None,
                            doc: None,
                            pixels: vec![snapshot],
                            selection: None,
                            flip: None,
                        });
                        self.encode_invert(device, &mut encoder, id, rect);
                        self.dirty = self.dirty.union(rect);
                        history_changed = true;
                    }
                }
                Op::Fill(id, rgb) => {
                    let full = self.full();
                    if let Some(snapshot) = self.snapshot(device, &mut encoder, id, full) {
                        if let Some(layer) = self.layers.get(&id) {
                            let [r, g, b] = rgb.map(|c| f64::from(c) / 255.0);
                            let colour = wgpu::Color { r, g, b, a: 1.0 };
                            drop(render_pass(&mut encoder, "fill", &layer.view, wgpu::LoadOp::Clear(colour)));
                        }
                        self.push_step(Step {
                            made: 0,
                            name: None,
                            doc: None,
                            pixels: vec![snapshot],
                            selection: None,
                            flip: None,
                        });
                        self.dirty = full;
                        history_changed = true;
                    }
                }
                Op::Clear(id, painted) => {
                    let full = self.full();
                    let kept = painted.map_or(full, |r| r.intersect(full));
                    if let Some(snapshot) = self.snapshot(device, &mut encoder, id, kept) {
                        self.push_step(Step {
                            made: 0,
                            name: None,
                            doc: None,
                            pixels: vec![snapshot],
                            selection: None,
                            flip: None,
                        });
                    }
                    if let Some(layer) = self.layers.get(&id) {
                        drop(render_pass(
                            &mut encoder,
                            "clear",
                            &layer.view,
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        ));
                    }
                    self.dirty = full;
                    history_changed = true;
                }
                Op::Put(id, pixels) => {
                    if let Some(layer) = self.layers.get(&id) {
                        let (full, row) = (self.full(), self.width as usize * 4);
                        uploaded.extend(self.upload(device, &mut encoder, &layer.texture, full, &pixels, 0, row));
                    }
                    self.dirty = self.full();
                }
                Op::PutRegion { layer: id, x, y, width, height, pixels } => {
                    let at = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
                    let rect = IRect::new(at(x), at(y), at(x.saturating_add(width)), at(y.saturating_add(height)));
                    if let Some(layer) = self.layers.get(&id) {
                        let row = width as usize * 4;
                        uploaded.extend(self.upload(device, &mut encoder, &layer.texture, rect, &pixels, 0, row));
                    }
                    self.dirty = self.dirty.union(rect.intersect(self.full()));
                }
                Op::Hold(id) => {
                    let full = self.full();
                    self.held = self.snapshot(device, &mut encoder, id, full);
                }
                Op::Release { keep, before } => {
                    let held = self.held.take();
                    if keep {
                        if held.is_some() || before.is_some() {
                            self.push_step(Step {
                                made: 0,
                                name: None,
                                doc: before,
                                pixels: held.into_iter().collect(),
                                selection: None,
                                flip: None,
                            });
                            history_changed = true;
                        }
                    } else if let Some(s) = held
                        && let Some(layer) = self.layers.get(&s.layer)
                    {
                        encoder.copy_texture_to_texture(
                            copy_at(&s.texture, 0, 0),
                            copy_at(&layer.texture, s.rect.x0, s.rect.y0),
                            extent(s.rect.width(), s.rect.height()),
                        );
                    }
                    self.dirty = self.full();
                }
                Op::Flip { layers, horizontal } => {
                    // undone by flipping again: nothing kept
                    let step = Step {
                        made: 0,
                        name: None,
                        doc: None,
                        pixels: Vec::new(),
                        selection: None,
                        flip: Some((layers.clone(), horizontal)),
                    };
                    self.push_step(step);
                    self.encode_flip(device, &mut encoder, &layers, horizontal);
                    self.dirty = self.full();
                    history_changed = true;
                }
                Op::Write { layers, before } => {
                    let snapshots: Vec<Snapshot> = layers
                        .iter()
                        .filter_map(|(id, rect, _)| self.snapshot(device, &mut encoder, *id, *rect))
                        .collect();
                    let document_changed = before.is_some();
                    if !snapshots.is_empty() || document_changed {
                        self.push_step(Step {
                            made: 0,
                            name: None,
                            doc: before,
                            pixels: snapshots,
                            selection: None,
                            flip: None,
                        });
                    }
                    for (id, rect, pixels) in layers {
                        let Some(layer) = self.layers.get(&id) else { continue };
                        // copied in the encoder's order, so the snapshot above is taken first
                        let row = rect.width() as usize * 4;
                        uploaded.extend(self.upload(device, &mut encoder, &layer.texture, rect, &pixels, 0, row));
                        self.dirty = self.dirty.union(rect);
                    }
                    if document_changed {
                        self.dirty = self.full();
                    }
                    history_changed = true;
                }
                Op::TransformBegin(id) => {
                    self.transform_more.clear();
                    if let Some(t) = self.copy_aside(device, &mut encoder, id) {
                        self.transforming = Some(t);
                    }
                }
                Op::TransformAlso(id) => {
                    if let Some(t) = self.copy_aside(device, &mut encoder, id) {
                        self.transform_more.push(t);
                    }
                }
                Op::TransformSet { to_source: m, resample } => {
                    let mut drew = false;
                    for t in self.transforming.iter().chain(&self.transform_more) {
                        let Some(layer) = self.layers.get(&t.layer) else { continue };
                        let filter = match resample {
                            Resample::Nearest => 0.0,
                            Resample::Bilinear => 1.0,
                            Resample::Bicubic => 2.0,
                        };
                        let [a, b, c, d, e, f, g, h, i] = m;
                        let offset = uniforms.push(&TransformUniform {
                            r0: [a, b, c, 0.0],
                            r1: [d, e, f, 0.0],
                            r2: [g, h, i, 0.0],
                            p: [filter, self.width as f32, self.height as f32, if t.masked { 1.0 } else { 0.0 }],
                        });
                        let mut pass = render_pass(
                            &mut encoder,
                            "transform",
                            &layer.view,
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        );
                        if t.masked {
                            pass.set_pipeline(&self.pipelines.transform_static);
                            pass.set_bind_group(0, &t.bind, &[]);
                            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
                            pass.draw(0..3, 0..1);
                        }
                        pass.set_pipeline(&self.pipelines.transform);
                        pass.set_bind_group(0, &t.bind, &[]);
                        pass.set_bind_group(1, &self.uniform_bind, &[offset]);
                        pass.draw(0..3, 0..1);
                        drop(pass);
                        drew = true;
                    }
                    if drew {
                        self.dirty = self.full();
                    }
                }
                Op::TransformMesh { vertices, resample } => {
                    let filter = match resample {
                        Resample::Nearest => 0.0,
                        Resample::Bilinear => 1.0,
                        Resample::Bicubic => 2.0,
                    };
                    let bytes: &[u8] = bytemuck::cast_slice(&vertices);
                    let count = u32::try_from(vertices.len()).unwrap_or(0);
                    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("warp mesh"),
                        size: (bytes.len() as u64).max(16),
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    // written before this frame's commands run
                    gpu.queue.write_buffer(&buffer, 0, bytes);
                    let mut drew = false;
                    for t in self.transforming.iter().chain(&self.transform_more) {
                        let Some(layer) = self.layers.get(&t.layer) else { continue };
                        let offset = uniforms.push(&TransformUniform {
                            r0: [0.0; 4],
                            r1: [0.0; 4],
                            r2: [0.0; 4],
                            p: [filter, self.width as f32, self.height as f32, if t.masked { 1.0 } else { 0.0 }],
                        });
                        let mut pass = render_pass(
                            &mut encoder,
                            "warp",
                            &layer.view,
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        );
                        if t.masked {
                            pass.set_pipeline(&self.pipelines.transform_static);
                            pass.set_bind_group(0, &t.bind, &[]);
                            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
                            pass.draw(0..3, 0..1);
                        }
                        if count > 0 {
                            pass.set_pipeline(&self.pipelines.mesh);
                            pass.set_bind_group(0, &t.bind, &[]);
                            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
                            pass.set_vertex_buffer(0, buffer.slice(..));
                            pass.draw(0..count, 0..1);
                        }
                        drop(pass);
                        drew = true;
                    }
                    if drew {
                        self.dirty = self.full();
                    }
                }
                Op::FilterSet(filter) => {
                    self.filter(gpu, &mut encoder, &mut uniforms, filter);
                }
                Op::PluginFilter(filter) => {
                    self.plugin_filter(gpu, &mut encoder, &mut uniforms, &filter);
                }
                Op::LiquifyBegin => {
                    let (width, height) = (self.width.div_ceil(2), self.height.div_ceil(2));
                    let format = self.pipelines.liquify_format;
                    let field = texture(device, "liquify field", width, height, format);
                    let view = field.create_view(&wgpu::TextureViewDescriptor::default());
                    drop(render_pass(
                        &mut encoder,
                        "liquify clear",
                        &view,
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    ));
                    self.liquify = Some(LiquifyField {
                        bind: self.copy_bind_with(device, &view),
                        view,
                        texture: field,
                        scale: [width as f32 / self.width as f32, height as f32 / self.height as f32],
                        scratch: None,
                    });
                }
                Op::PencilBegin => {
                    let mask = texture(device, "pencil mask", self.width, self.height, STROKE_FORMAT);
                    let view = mask.create_view(&wgpu::TextureViewDescriptor::default());
                    drop(render_pass(
                        &mut encoder,
                        "pencil clear",
                        &view,
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    ));
                    self.pencil = Some(PencilMask { view, bounds: IRect::EMPTY });
                    // the edit's source now takes the mask where it took the selection
                    let source = self
                        .transforming
                        .as_ref()
                        .map(|t| t.before.create_view(&wgpu::TextureViewDescriptor::default()));
                    if let Some(view) = source {
                        let bind = self.adjustment_bind(device, &view, true);
                        if let Some(t) = self.transforming.as_mut() {
                            t.bind = bind;
                            t.masked = true;
                        }
                    }
                }
                Op::LiquifyStamps(stamps) => self.liquify_step(device, &mut encoder, &mut uniforms, &stamps, 1.0),
                Op::LiquifyScale(amount) => self.liquify_step(device, &mut encoder, &mut uniforms, &[], amount),
                Op::LiquifyClear => {
                    if let Some(field) = &self.liquify {
                        drop(render_pass(
                            &mut encoder,
                            "liquify clear",
                            &field.view,
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        ));
                    }
                }
                Op::TransformEnd { commit, within } => {
                    self.liquify = None;
                    self.blur_textures.lock().unwrap_or_else(PoisonError::into_inner).clear();
                    let painted = self.pencil.take().map(|p| p.bounds.intersect(self.full()));
                    let more = std::mem::take(&mut self.transform_more);
                    // a save made while it was open read them as they showed then
                    let ended: Vec<LayerId> = self.transforming.iter().chain(&more).map(|t| t.layer).collect();
                    for id in ended {
                        self.touched(id);
                    }
                    if let Some(t) = self.transforming.take() {
                        if commit && let Some(rect) = painted {
                            // painted on: only where it was painted changed, and nothing if nowhere
                            if !rect.is_empty() {
                                let texture = region_copy(device, &mut encoder, &t.before, rect);
                                self.push_step(Step {
                                    made: 0,
                                    name: None,
                                    doc: None,
                                    pixels: vec![Snapshot { layer: t.layer, rect, texture }],
                                    selection: None,
                                    flip: None,
                                });
                                history_changed = true;
                            }
                            self.spare_before = Some(t.before);
                        } else if commit {
                            let full = self.full();
                            // only the part changed, when that is known
                            let part = within.map(|r| r.intersect(full)).filter(|r| !r.is_empty() && *r != full);
                            let mut spare = None;
                            let pixels = std::iter::once(t)
                                .chain(more)
                                .map(|t| match part {
                                    Some(rect) => {
                                        let texture = region_copy(device, &mut encoder, &t.before, rect);
                                        spare = Some(t.before);
                                        Snapshot { layer: t.layer, rect, texture }
                                    }
                                    None => Snapshot { layer: t.layer, rect: full, texture: t.before },
                                })
                                .collect();
                            if spare.is_some() {
                                self.spare_before = spare;
                            }
                            self.push_step(Step {
                                made: 0,
                                name: None,
                                doc: None,
                                pixels,
                                selection: None,
                                flip: None,
                            });
                            history_changed = true;
                        } else {
                            for t in std::iter::once(t).chain(more) {
                                if let Some(layer) = self.layers.get(&t.layer) {
                                    encoder.copy_texture_to_texture(
                                        copy_at(&t.before, 0, 0),
                                        copy_at(&layer.texture, 0, 0),
                                        extent(self.width, self.height),
                                    );
                                }
                                self.spare_before = Some(t.before);
                            }
                        }
                        self.dirty = self.full();
                    }
                }
                Op::Wipe(id) => {
                    if let Some(layer) = self.layers.get(&id) {
                        drop(render_pass(
                            &mut encoder,
                            "wipe",
                            &layer.view,
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        ));
                    }
                    self.dirty = self.full();
                }
                Op::Record(before) => {
                    self.push_step(Step {
                        made: 0,
                        name: None,
                        doc: Some(before),
                        pixels: Vec::new(),
                        selection: None,
                        flip: None,
                    });
                    self.dirty = self.full();
                    history_changed = true;
                }
                Op::OutsideChange(name) => {
                    self.push_step(Step {
                        made: 0,
                        name: Some(name),
                        doc: None,
                        pixels: Vec::new(),
                        selection: None,
                        flip: None,
                    });
                    history_changed = true;
                }
                Op::Duplicate { pairs, before } => {
                    for (from, to) in pairs {
                        if let (Some(src), Some(dst)) = (self.layers.get(&from), self.layers.get(&to)) {
                            encoder.copy_texture_to_texture(
                                copy_at(&src.texture, 0, 0),
                                copy_at(&dst.texture, 0, 0),
                                extent(self.width, self.height),
                            );
                        }
                    }
                    self.push_step(Step {
                        made: 0,
                        name: None,
                        doc: Some(before),
                        pixels: Vec::new(),
                        selection: None,
                        flip: None,
                    });
                    self.dirty = self.full();
                    history_changed = true;
                }
                Op::MergeDown { upper, lower, before, within, masks, drop_lower } => {
                    let opacity = before.layer(lower).map_or(1.0, |l| l.opacity);
                    let pixels = self.before_merge(
                        device,
                        &mut encoder,
                        &mut uniforms,
                        (lower, opacity),
                        &masks,
                        drop_lower,
                        within,
                    );
                    self.blend_layer_into(device, &mut encoder, &mut uniforms, &before, upper, lower);
                    self.push_step(Step {
                        made: 0,
                        name: None,
                        doc: Some(before),
                        pixels,
                        selection: None,
                        flip: None,
                    });
                    self.dirty = self.full();
                    history_changed = true;
                }
                Op::MergeMany { uppers, lower, before, within, masks, drop_lower } => {
                    let opacity = before.layer(lower).map_or(1.0, |l| l.opacity);
                    let pixels = self.before_merge(
                        device,
                        &mut encoder,
                        &mut uniforms,
                        (lower, opacity),
                        &masks,
                        drop_lower,
                        within,
                    );
                    for upper in uppers {
                        if before.layer(upper).is_some_and(|l| l.kind == LayerKind::Group) {
                            self.draw_group_onto(device, &mut encoder, &mut uniforms, &before, upper, lower);
                        } else {
                            self.blend_layer_into(device, &mut encoder, &mut uniforms, &before, upper, lower);
                        }
                    }
                    self.push_step(Step {
                        made: 0,
                        name: None,
                        doc: Some(before),
                        pixels,
                        selection: None,
                        flip: None,
                    });
                    self.dirty = self.full();
                    history_changed = true;
                }
                Op::ApplyMask { layer, mask, before } => {
                    let pixels =
                        self.masked_in_place(device, &mut encoder, &mut uniforms, layer, mask).into_iter().collect();
                    self.push_step(Step {
                        made: 0,
                        name: None,
                        doc: Some(before),
                        pixels,
                        selection: None,
                        flip: None,
                    });
                    self.dirty = self.full();
                    history_changed = true;
                }
                Op::Flatten { group, into, before } => {
                    // a new layer: no pixels of its own to keep
                    self.flatten(device, &mut encoder, &mut uniforms, &before, group, into);
                    let pixels = Vec::new();
                    self.push_step(Step {
                        made: 0,
                        name: None,
                        doc: Some(before),
                        pixels,
                        selection: None,
                        flip: None,
                    });
                    self.dirty = self.full();
                    history_changed = true;
                }
            }
        }
        if history_changed {
            self.collect_layers(doc);
        }
        // a stroke's own grain goes with it once it has ended
        if self.active.is_none() && self.stroke_grain.take().is_some() {
            self.bind_brush_images(device);
        }

        if !smudge_uniforms.data.is_empty() {
            gpu.queue.write_buffer(&self.smudge_params, 0, &smudge_uniforms.data);
        }
        if !wet_uniforms.data.is_empty() {
            gpu.queue.write_buffer(&self.wet_params, 0, &wet_uniforms.data);
        }
        self.make_groups(device, group_levels(doc));
        self.ensure_caches(gpu, doc, &mut encoder, &mut uniforms);
        // after the caches: caches just made want the whole picture composited again
        let dirty = self.dirty.intersect(self.full());
        let device = &gpu.device;
        // while caches are remade the composite stays as it was, what changed kept for after
        if self.remake.is_none() {
            if !dirty.is_empty() {
                self.composite(device, doc, &mut encoder, &mut uniforms, dirty);
                self.composites = self.composites.wrapping_add(1);
            }
            self.dirty = IRect::EMPTY;
        }

        if let Some(target) = target {
            // what the glass shows is worked out first: drawn in the display's own pass, a tiled
            // GPU needn't write the whole target out and read it back for a pass of its own
            let shows = |b: &Backdrop<'_>| !(b.regions.is_empty() && b.windows.is_empty()) && b.sigma > 0.0;
            let backdrop = target.backdrop.filter(shows);
            let glass = backdrop.map(|b| (b, self.frost_ready(device, &mut encoder, &mut uniforms, &target, b, None)));
            self.display(device, &mut encoder, &mut uniforms, &target, predicted, glass);
        }
        self.write_uniforms(gpu, &uniforms);
        gpu.queue.submit([encoder.finish()]);
        for buffer in uploaded {
            self.uploads.give_back(buffer);
        }
        self.renders = self.renders.wrapping_add(1);
        self.park_extra(gpu, doc);
        // the work held back, in a submission after this one's
        if deferred {
            self.render(gpu, doc, None);
        }
    }

    /// Bytes per row of a layer copied into a buffer (rows are 256-byte aligned for copies).
    fn row_bytes(&self) -> u32 {
        (self.width * 4).div_ceil(256) * 256
    }

    /// Smudges `instances` into the stroke's layer: per stamp, the brush picks up colour and lays
    /// down what it carries; then the changed pixels go back into the layer texture. Wet paint is
    /// laid a display frame at a time (see [`Canvas::lay_wet`]), the newest frame's stamps held
    /// back until the rest of it comes.
    fn smudge_stamps(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        (params, passes): (&mut UniformData, &mut UniformData),
        (instances, wet, times): (&[StampInstance], &[[f32; 8]], &[Paced]),
        style: &StrokeStyle,
        target: LayerId,
        restart: bool,
    ) {
        let row = self.row_bytes();
        // redrawn from scratch: the stroke starts again on the layer as it was before it
        if restart
            && let (Some(smudge), Some(layer)) =
                (self.smudge.as_mut().filter(|s| s.layer == target), self.layers.get(&target))
        {
            encoder.copy_texture_to_texture(
                copy_at(&smudge.before, 0, 0),
                copy_at(&layer.texture, 0, 0),
                extent(self.width, self.height),
            );
            encoder.copy_texture_to_buffer(
                copy_at(&smudge.before, 0, 0),
                wgpu::TexelCopyBufferInfo {
                    buffer: &smudge.work,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: Some(self.height),
                    },
                },
                extent(self.width, self.height),
            );
            smudge.started = false;
            smudge.scale = 1.0;
            smudge.kept = -1;
            smudge.stamps = 0;
            smudge.pending.clear();
            smudge.passes = 0;
            smudge.plot = 0;
            if let Some(amount) = &smudge.amount {
                encoder.clear_buffer(amount, 0, None);
            }
            if let Some(raw) = &smudge.grained {
                encoder.copy_texture_to_buffer(
                    copy_at(&smudge.before, 0, 0),
                    wgpu::TexelCopyBufferInfo {
                        buffer: raw,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(row),
                            rows_per_image: Some(self.height),
                        },
                    },
                    extent(self.width, self.height),
                );
            }
            self.dirty = self.full();
        }
        if self.smudge.as_ref().is_none_or(|s| s.layer != target) {
            if self.smudge_refused {
                return;
            }
            let Some(layer) = self.layers.get(&target) else { return };
            let wet_paint = style.paints_in_place();
            let layer_bytes = u64::from(row) * u64::from(self.height);
            // paint laid in place with a second, a texturized grain, wet or burnt edges is laid
            // apart, and put together with them
            let second = self.queued_second.map(|d| (d.mode + 1, d.threshold));
            let apart = second
                .or((grain_once(style) || style.wet_edges > 0.0 || style.burnt_edges > 0.0).then_some((0, -1.0)))
                .filter(|_| wet_paint)
                .map(|(mode, threshold)| (*style, mode, threshold));
            // and keeps the stroke as it was laid, each stamp's colour in it (and wet paint to go on
            // mixing with)
            let grain = apart.is_some();
            // and builds up by what is left in the modulated mode (and keeps how much shows, laid
            // apart)
            let amount_bytes = (wet_paint && style.rendering == Rendering::Gradual || apart.is_some())
                .then(|| layer_bytes * if apart.is_some() { 2 } else { 1 });
            // all it takes made first, and the stroke not begun if any of it can't be (past the
            // GPU's memory, or more than a binding may hold): begun, each frame it is in would go
            // undrawn (the screen flashing) and its undo step would wipe what it covered
            let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
            let invalid = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let work = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("smudge layer"),
                size: layer_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let carry = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("smudge carry"),
                // the carry, and the blur's two passes over it (see the shader's cs_blur_down); or
                // wet paint's two carried-over plots and a pass's plots (see cs_wet_plots)
                size: if wet_paint {
                    (2 * u64::from(WET_PLOT_MOST * WET_PLOT_MOST) + WET_PLOTS_TEXELS) * 16
                } else {
                    u64::from(SMUDGE_GRID * SMUDGE_GRID) * 16 * 3
                },
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            let before = texture(device, "smudge before", self.width, self.height, LAYER_FORMAT);
            let grained = grain.then(|| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("smudge grained"),
                    size: layer_bytes,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            });
            let amount = amount_bytes.map(|size| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("smudge amount"),
                    size,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            });
            let failed = pollster::block_on(invalid.pop()).or(pollster::block_on(memory.pop()));
            let bound = (if grain { 2 * layer_bytes } else { layer_bytes }).max(amount_bytes.unwrap_or(0));
            let most = device.limits().max_storage_buffer_binding_size;
            let too_much = || (bound > most).then(|| format!("{bound} bytes bound, {most} at most"));
            let refused = failed.map(|e| e.to_string()).or_else(too_much);
            if let Some(why) = refused {
                log::error!("smudging: {why}");
                self.smudge_refused = true;
                self.smudge_untold = true;
                return;
            }
            encoder.copy_texture_to_texture(
                copy_at(&layer.texture, 0, 0),
                copy_at(&before, 0, 0),
                extent(self.width, self.height),
            );
            let copy_into = |encoder: &mut wgpu::CommandEncoder, buffer: &wgpu::Buffer, offset: u64| {
                encoder.copy_texture_to_buffer(
                    copy_at(&layer.texture, 0, 0),
                    wgpu::TexelCopyBufferInfo {
                        buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset,
                            bytes_per_row: Some(row),
                            rows_per_image: Some(self.height),
                        },
                    },
                    extent(self.width, self.height),
                );
            };
            copy_into(encoder, &work, 0);
            if let Some(buffer) = &grained {
                copy_into(encoder, buffer, 0);
            }
            self.smudge = Some(Smudge {
                layer: target,
                work,
                carry,
                before,
                started: false,
                scale: 1.0,
                kept: -1,
                stamps: 0,
                grained,
                amount,
                apart,
                pending: Vec::new(),
                passes: 0,
                plot: 0,
            });
        }
        if style.paints_in_place() {
            let Some(smudge) = self.smudge.as_mut() else { return };
            smudge.pending.extend(instances.iter().zip(wet).zip(times).map(|((s, w), t)| (*s, *w, *t)));
            let newest = smudge.pending.last().map(|s| wet_frame(s.2.time_ms));
            let ready = smudge.pending.iter().take_while(|s| Some(wet_frame(s.2.time_ms)) != newest).count();
            let ready: Vec<_> = smudge.pending.drain(..ready).collect();
            self.lay_wet(device, encoder, (params, passes), &ready, style);
            return;
        }
        let own_grain = self.full_grain_bind(device, style);
        let Some(smudge) = self.smudge.as_mut() else { return };
        let bind = smudge_bind(
            device,
            &self.pipelines.smudge_layout,
            (&self.smudge_params, &self.wet_params),
            &self.smudge_spare,
            smudge,
            None,
        );
        let full = IRect::from_size(self.width, self.height);
        let mut changed = IRect::EMPTY;
        let mut pass =
            encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("smudge"), timestamp_writes: None });
        pass.set_bind_group(1, own_grain.as_ref().unwrap_or(&self.brush_bind), &[]);
        for (k, s) in instances.iter().enumerate() {
            let center = procraft_engine::geom::Vec2::new(s.center[0], s.center[1]);
            let bbox = IRect::around(center, (s.radius + QUAD_REACH) * s.roundness.max(1.0) * std::f32::consts::SQRT_2)
                .intersect(full);
            if bbox.is_empty() || s.radius <= 0.0 {
                continue;
            }
            changed = changed.union(bbox);
            // the blur and the pull as far as this stamp's pressure takes them
            let [_, blur_by, blur_jitter_by, _, _, _, _, smudge_pull_by] = wet.get(k).copied().unwrap_or([1.0; 8]);
            smudge.stamps = smudge.stamps.saturating_add(1);
            // the blur, jitter only ever lowering it
            let blur = style.blur
                * blur_by
                * (1.0 - style.blur_jitter * blur_jitter_by * unit_hash(smudge.stamps.wrapping_add(7919)));
            // what the brush carries a pixel a cell, unless the stroke's first stamp is wider than
            // the grid; its middle at the stamp's pixel, and the cells the stamp reaches around it
            let reach = s.radius.max(0.5) + 1.0;
            if !smudge.started {
                smudge.scale = (reach / SMUDGE_REACH as f32).max(1.0);
            }
            let anchor = if smudge.scale <= 1.0 { s.center.map(f32::floor) } else { s.center };
            let half = ((reach / smudge.scale).ceil() as i32).clamp(1, SMUDGE_REACH);
            let offset = params.push(&SmudgeUniform {
                pull: style.pull.clamp(0.0, 1.0) * smudge_pull_by,
                kept: smudge.kept,
                blur,
                anchor,
                scale: smudge.scale,
                half,
                ..smudge_uniform(s, style, bbox, (self.width, self.height, row))
            });
            smudge.started = true;
            smudge.kept = half;
            let cells = (2 * half.unsigned_abs() + 1).div_ceil(8);
            pass.set_bind_group(0, &bind, &[offset, 0]);
            pass.set_pipeline(&self.pipelines.smudge_carry);
            pass.dispatch_workgroups(cells, cells, 1);
            if blur > 0.0 {
                pass.set_pipeline(&self.pipelines.smudge_blur_down);
                pass.dispatch_workgroups(cells, cells, 1);
                pass.set_pipeline(&self.pipelines.smudge_blur_across);
                pass.dispatch_workgroups(cells, cells, 1);
            }
            pass.set_pipeline(&self.pipelines.smudge_deposit);
            pass.dispatch_workgroups(bbox.width().div_ceil(8), bbox.height().div_ceil(8), 1);
        }
        drop(pass);
        self.smudged(encoder, changed);
    }

    /// Puts the stroke's pixels changed within `changed` back into its layer's texture.
    fn smudged(&mut self, encoder: &mut wgpu::CommandEncoder, changed: IRect) {
        if changed.is_empty() {
            return;
        }
        let row = self.row_bytes();
        let Some(smudge) = self.smudge.as_ref() else { return };
        if let Some(layer) = self.layers.get(&smudge.layer) {
            let offset =
                u64::from(row) * u64::try_from(changed.y0).unwrap_or(0) + 4 * u64::try_from(changed.x0).unwrap_or(0);
            encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: &smudge.work,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset,
                        bytes_per_row: Some(row),
                        rows_per_image: Some(changed.height()),
                    },
                },
                copy_at(&layer.texture, changed.x0, changed.y0),
                extent(changed.width(), changed.height()),
            );
        }
        self.dirty = self.dirty.union(changed);
    }

    /// A dual brush's paint laid in place put together with its second over `rect` (see
    /// `cs_synthesize`), and shown.
    fn synthesize(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        params: &mut UniformData,
        rect: IRect,
    ) {
        let rect = rect.intersect(IRect::from_size(self.width, self.height));
        let Some(&(style, dual, dual_threshold)) = self.smudge.as_ref().and_then(|s| s.apart.as_ref()) else { return };
        let own_grain = self.full_grain_bind(device, &style);
        if rect.is_empty() {
            return;
        }
        let size = (self.width, self.height, self.row_bytes());
        let second = (dual != 0).then(|| self.second_stroke.get(device).1);
        let Some(smudge) = self.smudge.as_ref() else { return };
        let bind = smudge_bind(
            device,
            &self.pipelines.smudge_layout,
            (&self.smudge_params, &self.wet_params),
            &self.smudge_spare,
            smudge,
            second,
        );
        let offset = params.push(&SmudgeUniform {
            mode: 1,
            dual,
            dual_threshold,
            grain_once: if grain_once(&style) { 1.0 } else { 0.0 },
            laid_apart: 2,
            ..smudge_uniform(&bytemuck::Zeroable::zeroed(), &style, rect, size)
        });
        let mut pass = encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("synthesize"), timestamp_writes: None });
        pass.set_bind_group(0, &bind, &[offset, 0]);
        pass.set_bind_group(1, own_grain.as_ref().unwrap_or(&self.brush_bind), &[]);
        pass.set_pipeline(&self.pipelines.smudge_synthesize);
        pass.dispatch_workgroups(rect.width().div_ceil(8), rect.height().div_ceil(8), 1);
        drop(pass);
        self.smudged(encoder, rect);
    }

    /// Lays wet paint, a pass a display frame (at most [`WET_PASS_MOST`] stamps): `stamps` in
    /// order, with their wet-mix scaling and places in the stroke. Each pass works out what each of its
    /// stamps lays down from the layer as the pass finds it (see the shader's `cs_wet_plots`), then
    /// lays the stamps, each through its shape and grain.
    fn lay_wet(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        (params, passes): (&mut UniformData, &mut UniformData),
        stamps: &[(StampInstance, [f32; 8], Paced)],
        style: &StrokeStyle,
    ) {
        if stamps.is_empty() {
            return;
        }
        let size = (self.width, self.height, self.row_bytes());
        let full = IRect::from_size(self.width, self.height);
        let own_grain = self.full_grain_bind(device, style);
        let dual = self.smudge.as_ref().and_then(|s| s.apart.as_ref()).is_some_and(|a| a.1 != 0);
        let second = dual.then(|| self.second_stroke.get(device).1);
        let Some(smudge) = self.smudge.as_mut() else { return };
        let bind = smudge_bind(
            device,
            &self.pipelines.smudge_layout,
            (&self.smudge_params, &self.wet_params),
            &self.smudge_spare,
            smudge,
            second,
        );
        let grain_once = if grain_once(style) { 1.0 } else { 0.0 };
        let modulated = u32::from(smudge.amount.is_some() && style.rendering == Rendering::Gradual);
        let laid_apart = u32::from(smudge.apart.is_some());
        let mut changed = IRect::EMPTY;
        let mut pass =
            encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("wet"), timestamp_writes: None });
        pass.set_bind_group(1, own_grain.as_ref().unwrap_or(&self.brush_bind), &[]);
        if style.wet_pull.is_none() {
            // dry paint laid in place: each stamp lays the paint itself, no pass of plots
            for (s, _, _) in stamps {
                let center = procraft_engine::geom::Vec2::new(s.center[0], s.center[1]);
                let bbox =
                    IRect::around(center, (s.radius + QUAD_REACH) * s.roundness.max(1.0) * std::f32::consts::SQRT_2)
                        .intersect(full);
                if bbox.is_empty() || s.radius <= 0.0 {
                    continue;
                }
                changed = changed.union(bbox);
                let [r, g, b] = stamp_paint(s, style);
                let offset = params.push(&SmudgeUniform {
                    mode: 1,
                    modulated,
                    grain_once,
                    count: 0,
                    laid_apart: laid_apart * 2,
                    paint: [r, g, b, 1.0],
                    ..smudge_uniform(s, style, bbox, size)
                });
                pass.set_bind_group(0, &bind, &[offset, 0]);
                pass.set_pipeline(&self.pipelines.smudge_deposit);
                pass.dispatch_workgroups(bbox.width().div_ceil(8), bbox.height().div_ceil(8), 1);
            }
            drop(pass);
            if laid_apart == 1 {
                self.synthesize(device, encoder, params, changed);
            } else {
                self.smudged(encoder, changed);
            }
            return;
        }
        let mut from = 0;
        while let Some(head) = stamps.get(from) {
            let frame = wet_frame(head.2.time_ms);
            let count =
                stamps.iter().skip(from).take(WET_PASS_MOST).take_while(|s| wet_frame(s.2.time_ms) == frame).count();
            let group = stamps.get(from..from + count.max(1)).unwrap_or_default();
            from += group.len().max(1);
            let n = group.len();
            let first = smudge.passes == 0;
            // each plot as many texels a side as the pass's largest stamp is pixels across, halved
            // until the pass's plots fit
            let largest = group.iter().map(|s| s.0.radius).fold(0.0f32, f32::max);
            let mut plot = ((2.0 * largest).ceil().max(4.0) as u32).min(WET_PLOT_MOST);
            while plot > 4 && u64::from(plot * plot) * n as u64 > WET_PLOTS_TEXELS {
                plot /= 2;
            }
            let mut list = WetPass { stamps: [[0.0; 12]; WET_PASS_MOST] };
            for (k, (s, w, paced)) in group.iter().enumerate() {
                let [grade_by, _, _, wet_jitter_by, charge_by, dilution_by, wet_pull_by, _] = *w;
                // the mixture varies from stamp to stamp by the wetness jitter
                let jitter = 1.0 - style.wet_jitter * wet_jitter_by * unit_hash(smudge.stamps);
                smudge.stamps = smudge.stamps.saturating_add(1);
                // the paint runs out with the length the stroke has gone
                let (mixture, pull) = wet_mixture(style, paced.distance, jitter, s.more[3], dilution_by, charge_by);
                let pull = pull * wet_pull_by;
                let grade = (style.grade * grade_by).clamp(-1.0, 1.0);
                let final_mixture = final_mixture(mixture, pull, grade, (k, n), first);
                let [r, g, b] = stamp_paint(s, style);
                if let Some(slot) = list.stamps.get_mut(k) {
                    *slot = [
                        s.center[0],
                        s.center[1],
                        s.radius.max(0.5),
                        s.angle,
                        s.roundness,
                        pull,
                        mixture,
                        final_mixture,
                        r,
                        g,
                        b,
                        1.0,
                    ];
                }
            }
            let listed = passes.push(&list);
            let carried = smudge.plot.max(1);
            let turn = smudge.passes % 2;
            let wet = |s: &StampInstance, bbox: IRect, index: u32| SmudgeUniform {
                mode: 1,
                modulated,
                kept: if first { -1 } else { 0 },
                grain_once,
                laid_apart: laid_apart * 2,
                alpha: (s.alpha * style.opacity * style.wetness).clamp(0.0, 1.0),
                count: u32::try_from(n).unwrap_or(0),
                plot,
                index,
                carried,
                ..smudge_uniform(s, style, bbox, size)
            };
            // the pass's plots, carrying over from the plot the last pass kept (the two kept
            // plots take turns)
            let Some(head) = group.first() else { continue };
            let offset = params.push(&wet(&head.0, IRect::EMPTY, turn));
            pass.set_bind_group(0, &bind, &[offset, listed]);
            pass.set_pipeline(&self.pipelines.smudge_wet_plots);
            pass.dispatch_workgroups(plot.div_ceil(8), plot.div_ceil(8), 1);
            // then each stamp laid from its plot
            for (k, (s, _, _)) in group.iter().enumerate() {
                let center = procraft_engine::geom::Vec2::new(s.center[0], s.center[1]);
                let bbox =
                    IRect::around(center, (s.radius + QUAD_REACH) * s.roundness.max(1.0) * std::f32::consts::SQRT_2)
                        .intersect(full);
                if bbox.is_empty() || s.radius <= 0.0 {
                    continue;
                }
                changed = changed.union(bbox);
                let offset = params.push(&wet(s, bbox, u32::try_from(k).unwrap_or(0)));
                pass.set_bind_group(0, &bind, &[offset, listed]);
                pass.set_pipeline(&self.pipelines.smudge_deposit);
                pass.dispatch_workgroups(bbox.width().div_ceil(8), bbox.height().div_ceil(8), 1);
            }
            smudge.passes = smudge.passes.saturating_add(1);
            smudge.plot = plot;
        }
        drop(pass);
        if laid_apart == 1 {
            self.synthesize(device, encoder, params, changed);
        } else {
            self.smudged(encoder, changed);
        }
    }

    /// Ends a smudge stroke: one undo step with the pixels it changed, or (`cancel`) puts the
    /// layer back as it was.
    fn finish_smudge(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        bounds: IRect,
        cancel: bool,
    ) {
        let Some(smudge) = self.smudge.take() else { return };
        let Some(layer) = self.layers.get(&smudge.layer) else { return };
        if cancel {
            encoder.copy_texture_to_texture(
                copy_at(&smudge.before, 0, 0),
                copy_at(&layer.texture, 0, 0),
                extent(self.width, self.height),
            );
            self.dirty = self.full();
            return;
        }
        let rect = bounds.intersect(self.full());
        if rect.is_empty() {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("undo"),
            size: extent(rect.width(), rect.height()),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: LAYER_FORMAT,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        encoder.copy_texture_to_texture(
            copy_at(&smudge.before, rect.x0, rect.y0),
            copy_at(&texture, 0, 0),
            extent(rect.width(), rect.height()),
        );
        self.push_step(Step {
            made: 0,
            name: None,
            doc: None,
            pixels: vec![Snapshot { layer: smudge.layer, rect, texture }],
            selection: None,
            flip: None,
        });
    }

    fn commit(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        stroke: ActiveStroke,
    ) {
        let rect = stroke.bounds.intersect(self.full());
        let within = self.within.take();
        if !rect.is_empty() && self.layers.contains_key(&stroke.layer) {
            // what the stroke can change: its bounds, or only the rectangles it was said to reach
            let parts = within.unwrap_or_else(|| vec![rect]);
            let pixels: Vec<Snapshot> =
                parts.iter().filter_map(|r| self.snapshot(device, encoder, stroke.layer, r.intersect(rect))).collect();
            if !pixels.is_empty() {
                self.push_step(Step { made: 0, name: None, doc: None, pixels, selection: None, flip: None });
            }
            let opacity = stroke.style.opacity.clamp(0.0, 1.0);
            let locked = stroke.style.preserve_alpha;
            let burns = stroke.style.burnt_edges > 0.0 && !stroke.style.preview;
            // a dual brush's: its second's texture read too (the stroke has ended: told here)
            let second = if stroke.second.is_some() { self.second_stroke.get(device).1 } else { &self.stroke_view };
            let stroke_view = &self.stroke_view;
            let quad = |layer: &wgpu::TextureView| {
                let views = [layer, stroke_view, stroke_view, stroke_view, stroke_view, second, stroke_view];
                let grain = self.grain_view_of(&stroke.style);
                quad_bind(device, &self.pipelines.quad_layout, views, grain, &self.grain_sampler)
            };
            let laid = stroke.style.tool == StrokeTool::Laid;
            match (stroke.style.tool, laid || locked || stroke.style.blend != 0 || burns || stroke.style.luminance) {
                (StrokeTool::Paint | StrokeTool::Laid, true) => {
                    // recolour where the layer has alpha, or blend by a mode or burn edges (which
                    // read the layer): into scratch, then back
                    let offset = uniforms.push(
                        &LayerUniform {
                            opacity: 1.0,
                            stroke_mode: match (laid, locked) {
                                (true, _) => STROKE_LAID,
                                (_, true) => STROKE_PAINT_LOCKED,
                                _ => STROKE_PAINT,
                            },
                            stroke_opacity: opacity,
                            ..Default::default()
                        }
                        .with_grain(Some(&stroke.style))
                        .with_dual(Some(&stroke)),
                    );
                    if let Some(layer) = self.layers.get(&stroke.layer) {
                        let bind = quad(&layer.view);
                        let (scratch, scratch_view) = self.scratch.get(device);
                        let mut pass = render_pass(encoder, "commit locked", scratch_view, wgpu::LoadOp::Load);
                        pass.set_pipeline(&self.pipelines.commit_locked);
                        scissor(&mut pass, rect);
                        pass.set_bind_group(0, &bind, &[]);
                        pass.set_bind_group(1, &self.uniform_bind, &[offset]);
                        pass.draw(0..3, 0..1);
                        drop(pass);
                        encoder.copy_texture_to_texture(
                            copy_at(scratch, rect.x0, rect.y0),
                            copy_at(&layer.texture, rect.x0, rect.y0),
                            extent(rect.width(), rect.height()),
                        );
                    }
                }
                (tool, _) => {
                    let offset = uniforms.push(
                        &LayerUniform { opacity: 1.0, stroke_opacity: opacity, ..Default::default() }
                            .with_grain(Some(&stroke.style))
                            .with_dual(Some(&stroke)),
                    );
                    let dual =
                        (stroke.second.is_some() || stroke.style.full_grain.is_some()).then(|| quad(&self.stroke_view));
                    if let Some(layer) = self.layers.get(&stroke.layer) {
                        let mut pass = render_pass(encoder, "commit", &layer.view, wgpu::LoadOp::Load);
                        pass.set_pipeline(match tool {
                            StrokeTool::Paint | StrokeTool::Smudge | StrokeTool::Laid => &self.pipelines.commit_paint,
                            StrokeTool::Erase => &self.pipelines.commit_erase,
                        });
                        scissor(&mut pass, rect);
                        pass.set_bind_group(0, dual.as_ref().unwrap_or(&self.stroke_bind), &[]);
                        pass.set_bind_group(1, &self.uniform_bind, &[offset]);
                        pass.draw(0..3, 0..1);
                    }
                }
            }
        }
        drop(render_pass(encoder, "stroke clear", &self.stroke_view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)));
        self.dirty = self.dirty.union(rect);
    }

    /// Before layers are blended into `lower` (the layer, and its opacity): each of `masks`
    /// applied to its layer, and `lower` made as it showed: through its mask and at its opacity,
    /// or, with `drop_lower` (it was hidden), its pixels let go. Returns what the step keeps: each
    /// layer changed so whole as it was (once each: a second copy would put back a changed one),
    /// else `lower` where the merge can change it (`within`; nothing for an empty box).
    #[allow(clippy::too_many_arguments)]
    fn before_merge(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        (lower, opacity): (LayerId, f32),
        masks: &[(LayerId, LayerId)],
        drop_lower: bool,
        within: Option<IRect>,
    ) -> Vec<Snapshot> {
        let mut pixels: Vec<Snapshot> = masks
            .iter()
            .filter(|&&(layer, _)| layer != lower)
            .filter_map(|&(layer, mask)| self.masked_in_place(device, encoder, uniforms, layer, mask))
            .collect();
        let lower_mask = masks.iter().find(|&&(layer, _)| layer == lower).map(|&(_, mask)| mask);
        let opacity = opacity.clamp(0.0, 1.0);
        if drop_lower || lower_mask.is_some() || opacity < 1.0 {
            pixels.extend(self.snapshot(device, encoder, lower, self.full()));
            if drop_lower {
                if let Some(layer) = self.layers.get(&lower) {
                    let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
                    drop(render_pass(encoder, "hidden let go", &layer.view, clear));
                }
            } else {
                self.redraw_in_place(device, encoder, uniforms, lower, lower_mask, opacity);
            }
        } else {
            pixels.extend(self.snapshot(device, encoder, lower, within.unwrap_or(self.full())));
        }
        pixels
    }

    /// Layer `upper` of `doc` blended into layer `lower` with its opacity, mode and clipping.
    fn blend_layer_into(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        doc: &Document,
        upper: LayerId,
        lower: LayerId,
    ) -> Option<()> {
        let settings = doc.layer(upper)?;
        let clip = doc.clip_base(upper).and_then(|id| self.layers.get(&id));
        let up = self.layers.get(&upper)?;
        let uniform = LayerUniform {
            opacity: if settings.visible { settings.opacity.clamp(0.0, 1.0) } else { 0.0 },
            blend: settings.blend.id(),
            clipped: u32::from(clip.is_some()),
            ..Default::default()
        };
        self.blend_down(device, encoder, uniforms, &up.view, clip.map(|c| &c.view), uniform, lower)
    }

    /// `upper` blended into layer `lower` (through `clip`, as `uniform` says), by way of the
    /// scratch texture.
    #[allow(clippy::too_many_arguments)]
    fn blend_down(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        upper: &wgpu::TextureView,
        clip: Option<&wgpu::TextureView>,
        uniform: LayerUniform,
        lower: LayerId,
    ) -> Option<()> {
        let low = self.layers.get(&lower)?;
        let offset = uniforms.push(&uniform);
        let bind = self.quad(device, [upper, &self.stroke_view, clip.unwrap_or(&self.stroke_view), &low.view]);
        let (scratch, scratch_view) = self.scratch.get(device);
        let mut pass = render_pass(encoder, "merge", scratch_view, wgpu::LoadOp::Load);
        pass.set_pipeline(&self.pipelines.blend);
        pass.set_bind_group(0, &bind, &[]);
        pass.set_bind_group(1, &self.uniform_bind, &[offset]);
        pass.draw(0..3, 0..1);
        drop(pass);
        encoder.copy_texture_to_texture(
            copy_at(scratch, 0, 0),
            copy_at(&low.texture, 0, 0),
            extent(self.width, self.height),
        );
        Some(())
    }

    /// Layer `layer` multiplied by mask `mask` (its brightness), by way of the scratch texture;
    /// returns what the layer was.
    fn masked_in_place(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        layer: LayerId,
        mask: LayerId,
    ) -> Option<Snapshot> {
        let snapshot = self.snapshot(device, encoder, layer, self.full());
        self.redraw_in_place(device, encoder, uniforms, layer, Some(mask), 1.0)?;
        snapshot
    }

    /// Layer `layer` drawn again in place through `mask` (its brightness), if any, and at
    /// `opacity`, by way of the scratch texture.
    fn redraw_in_place(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        layer: LayerId,
        mask: Option<LayerId>,
        opacity: f32,
    ) -> Option<()> {
        let target = self.layers.get(&layer)?;
        let none = &self.stroke_view;
        let mask_view = match mask {
            Some(m) => Some(&self.layers.get(&m)?.view),
            None => None,
        };
        let masked = if mask_view.is_some() { 1.0 } else { 0.0 };
        let offset = uniforms.push(&LayerUniform { opacity, masked, ..Default::default() });
        let bind = self.quad_masked(device, [&target.view, none, none, none], mask_view.unwrap_or(none), none);
        let (scratch, scratch_view) = self.scratch.get(device);
        let mut pass = render_pass(encoder, "redraw", scratch_view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
        pass.set_pipeline(&self.pipelines.layer);
        pass.set_bind_group(0, &bind, &[]);
        pass.set_bind_group(1, &self.uniform_bind, &[offset]);
        pass.draw(0..3, 0..1);
        drop(pass);
        encoder.copy_texture_to_texture(
            copy_at(scratch, 0, 0),
            copy_at(&target.texture, 0, 0),
            extent(self.width, self.height),
        );
        Some(())
    }

    /// Group `group` of `doc` composited and copied into layer `into` (a new layer).
    fn flatten(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        doc: &Document,
        group: LayerId,
        into: LayerId,
    ) -> Option<()> {
        let full = self.full();
        self.composite_group(device, doc, encoder, uniforms, full, group)?;
        let (textures, _) = self.groups.get(doc.depth(group))?;
        let target = self.layers.get(&into)?;
        encoder.copy_texture_to_texture(
            copy_at(&textures[0], 0, 0),
            copy_at(&target.texture, 0, 0),
            extent(self.width, self.height),
        );
        Some(())
    }

    /// Group `group` of `doc` composited and blended into layer `into` with the group's opacity
    /// and mode.
    fn draw_group_onto(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        doc: &Document,
        group: LayerId,
        into: LayerId,
    ) -> Option<()> {
        let settings = doc.layer(group)?;
        let view = self.composite_group(device, doc, encoder, uniforms, self.full(), group)?;
        let uniform = LayerUniform {
            opacity: if settings.visible { settings.opacity.clamp(0.0, 1.0) } else { 0.0 },
            blend: settings.blend.id(),
            ..Default::default()
        };
        self.blend_down(device, encoder, uniforms, view, None, uniform, into)
    }

    /// Makes sure there is a pair of group textures for each of `depth` levels.
    fn make_groups(&mut self, device: &wgpu::Device, depth: usize) {
        while self.groups.len() < depth {
            let textures = [
                texture(device, "group", self.width, self.height, LAYER_FORMAT),
                texture(device, "group 2", self.width, self.height, LAYER_FORMAT),
            ];
            let views = [
                textures[0].create_view(&wgpu::TextureViewDescriptor::default()),
                textures[1].create_view(&wgpu::TextureViewDescriptor::default()),
            ];
            self.groups.push((textures, views));
        }
    }

    /// Copies a region of a layer aside.
    fn snapshot(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        id: LayerId,
        rect: IRect,
    ) -> Option<Snapshot> {
        let layer = self.layers.get(&id)?;
        let rect = rect.intersect(self.full());
        if rect.is_empty() {
            return None;
        }
        // the whole layer: into a texture let go, if one is kept, else one that can be kept when
        // this step is let go in turn
        if rect == self.full() {
            let texture =
                self.take_spare().unwrap_or_else(|| texture(device, "undo", self.width, self.height, LAYER_FORMAT));
            encoder.copy_texture_to_texture(
                copy_at(&layer.texture, 0, 0),
                copy_at(&texture, 0, 0),
                extent(self.width, self.height),
            );
            return Some(Snapshot { layer: id, rect, texture });
        }
        Some(Snapshot { layer: id, rect, texture: region_copy(device, encoder, &layer.texture, rect) })
    }

    /// Applies an undo or redo step and returns its inverse: the pixels and document it replaced.
    fn apply_step(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, doc: &mut Document, step: Step) -> Step {
        let device = &gpu.device;
        let made = step.made;
        let name = step.name.clone();
        let step_flip = step.flip;
        let mut pixels = Vec::with_capacity(step.pixels.len());
        for s in step.pixels {
            self.restore_all(gpu, [s.layer], true);
            let rect = s.rect.intersect(self.full());
            if let Some(layer) = self.layers.get(&s.layer).filter(|_| rect == s.rect && !rect.is_empty()) {
                // swapped through the scratch texture: the layer's pixels there go into the step's
                // own texture as the step back, nothing made
                let (scratch, _) = self.scratch.get(device);
                let size = extent(rect.width(), rect.height());
                encoder.copy_texture_to_texture(
                    copy_at(&layer.texture, rect.x0, rect.y0),
                    copy_at(scratch, 0, 0),
                    size,
                );
                encoder.copy_texture_to_texture(
                    copy_at(&s.texture, 0, 0),
                    copy_at(&layer.texture, rect.x0, rect.y0),
                    size,
                );
                encoder.copy_texture_to_texture(copy_at(scratch, 0, 0), copy_at(&s.texture, 0, 0), size);
                pixels.push(s);
            } else if let Some(layer) = self.layers.get(&s.layer) {
                if let Some(current) = self.snapshot(device, encoder, s.layer, s.rect) {
                    pixels.push(current);
                }
                encoder.copy_texture_to_texture(
                    copy_at(&s.texture, 0, 0),
                    copy_at(&layer.texture, s.rect.x0, s.rect.y0),
                    extent(s.rect.width(), s.rect.height()),
                );
            }
            self.dirty = self.dirty.union(rect);
        }
        let solo = doc.solo;
        let replaced = step.doc.map(|before| Box::new(std::mem::replace(doc, *before)));
        if let Some(replaced) = &replaced {
            // no layer it numbered numbered again: its pixels may still be held for redo
            doc.keep_ids_from(replaced);
            // a layer shown alone is how the artwork is looked at, not a change to undo
            doc.solo = solo.filter(|s| doc.layer(*s).is_some());
            self.dirty = self.full();
        }
        let selection = step.selection.map(|before| {
            let now = self.selection.take().map(|s| s.coverage);
            self.put_selection(device, &gpu.queue, before.map(std::borrow::Cow::Owned));
            now
        });
        // put back last first, so pixels that several snapshots cover end as the first left them
        pixels.reverse();
        if let Some((layers, horizontal)) = &step_flip {
            self.restore_all(gpu, layers.iter().copied(), true);
            self.encode_flip(device, encoder, layers, *horizontal);
            self.dirty = self.full();
        }
        Step { made, name, doc: replaced, pixels, selection, flip: step_flip }
    }

    /// Draws layer `id` inverted within `rect`, from its copy there in the scratch texture.
    fn encode_invert(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, id: LayerId, rect: IRect) {
        let Some(layer) = self.layers.get(&id) else { return };
        let (scratch, scratch_view) = self.scratch.get(device);
        encoder.copy_texture_to_texture(
            copy_at(&layer.texture, rect.x0, rect.y0),
            copy_at(scratch, rect.x0, rect.y0),
            extent(rect.width(), rect.height()),
        );
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("invert"),
            layout: &self.pipelines.bounds_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(scratch_view) }],
        });
        let mut pass = render_pass(encoder, "invert", &layer.view, wgpu::LoadOp::Load);
        scissor(&mut pass, rect);
        pass.set_pipeline(&self.pipelines.invert);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Draws `layers` mirrored (across when `horizontal`, else down) into themselves.
    fn encode_flip(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        layers: &[LayerId],
        horizontal: bool,
    ) {
        let pipeline = if horizontal { &self.pipelines.flip_across } else { &self.pipelines.flip_down };
        // each layer drawn mirrored from a copy of itself in the scratch texture (a pass
        // can't read what it draws into), one after another
        let (scratch, scratch_view) = self.scratch.get(device);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("flip"),
            layout: &self.pipelines.bounds_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(scratch_view) }],
        });
        for id in layers {
            let Some(layer) = self.layers.get(id) else { continue };
            encoder.copy_texture_to_texture(
                copy_at(&layer.texture, 0, 0),
                copy_at(scratch, 0, 0),
                extent(self.width, self.height),
            );
            let mut pass = render_pass(encoder, "flip", &layer.view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Draws the layer being edited from its copy through `filter`, kept to the selection.
    fn filter(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, uniforms: &mut UniformData, filter: Filter) {
        let Some(t) = &self.transforming else { return };
        let Some(layer) = self.layers.get(&t.layer) else { return };
        let (w, h) = (self.width as f32, self.height as f32);
        let full = [w, h, if t.masked { 1.0 } else { 0.0 }, 0.0];
        let values = |a: [f32; 4], b: [f32; 4]| FilterUniform { a, b, size: full, c: [0.0; 4] };
        // most adjustments are one pass from the copy into the layer
        let (pass, uniform) = match filter {
            Filter::Hsb { hue, saturation, brightness } => {
                ("fs_hsb", values([hue, saturation, brightness, 0.0], [0.0; 4]))
            }
            Filter::Sharpen { amount } => ("fs_sharpen", values([amount, 0.0, 0.0, 0.0], [0.0; 4])),
            Filter::Noise { amount, frequency, octaves, turbulence, kind, multi, added } => {
                let flags = [kind.min(2) as f32, f32::from(u8::from(multi)), f32::from(u8::from(added)), 0.0];
                ("fs_noise", values([amount, frequency, octaves.clamp(1, 8) as f32, turbulence], flags))
            }
            Filter::Motion { along: [x, y] } => {
                let count = (x.hypot(y)).clamp(1.0, 128.0);
                ("fs_streak", values([x, y, count, 1.0], [0.0; 4]))
            }
            Filter::Zoom { centre: [x, y], amount, toward: [tx, ty] } => {
                ("fs_zoom", values([amount, 0.0, 0.0, 0.0], [x, y, tx, ty]))
            }
            Filter::Chromatic { centre: [x, y], amount, displace, offset: [ox, oy], blur, falloff, transparency } => {
                // displaced, the centre's place carries the offset and the amount says so
                let (b, a) = if displace { ([ox, oy, -1.0, 0.0], 0.0) } else { ([x, y, 0.0, 0.0], amount) };
                ("fs_chroma", values([a, blur, falloff, transparency], b))
            }
            Filter::Halftone { pitch, kind } => {
                ("fs_halftone", values([pitch.max(2.0), kind.min(2) as f32, 0.0, 0.0], [0.0; 4]))
            }
            Filter::Blur { sigma } => ("fs_up", values([0.0; 4], [sigma, 0.0, 0.0, 0.0])),
            Filter::Bloom { sigma, amount, threshold, transition, burn } => {
                ("fs_bloom", values([amount, threshold, transition, burn], [sigma, 0.0, 0.0, 0.0]))
            }
            Filter::Curves => ("fs_curves", values([0.0; 4], [0.0; 4])),
            Filter::Liquify { amount } => {
                let [across, down] = self.liquify.as_ref().map_or([1.0; 2], |l| l.scale);
                ("fs_liquified", values([amount, across, down, 0.0], [0.0; 4]))
            }
            Filter::Clone { offset: [x, y] } => ("fs_clone", values([x, y, 0.0, 0.0], [0.0; 4])),
            Filter::Glitch { kind, amount, values: [v0, v1, v2], zoom, seed } => {
                ("fs_glitch", values([kind.min(3) as f32, amount, v0, v1], [v2, zoom, seed, 0.0]))
            }
            Filter::GradientMap { amount } => ("fs_gradient", values([amount, 0.0, 0.0, 0.0], [0.0; 4])),
            Filter::ColorBalance { shadows: [s0, s1, s2], midtones: [m0, m1, m2], highlights: [h0, h1, h2] } => (
                "fs_balance",
                FilterUniform { a: [s0, s1, s2, 0.0], b: [m0, m1, m2, 0.0], size: full, c: [h0, h1, h2, 0.0] },
            ),
        };
        let blurred = match filter {
            Filter::Blur { sigma } | Filter::Bloom { sigma, .. } => self.blur_copy(gpu, encoder, uniforms, sigma),
            _ => None,
        };
        let Some(pipeline) = self.pipelines.filters.get(pass) else { return };
        let offset = uniforms.push(&uniform);
        // the last pass reads the copy, or the blurred copy, and takes the selection from the edit
        let source = blurred.as_ref().map(|view| self.adjustment_bind(&gpu.device, view, t.masked));
        let mut out = render_pass(encoder, "adjustment", &layer.view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
        out.set_pipeline(pipeline);
        out.set_bind_group(0, source.as_ref().unwrap_or(&t.bind), &[]);
        out.set_bind_group(1, &self.uniform_bind, &[offset]);
        // liquify reads its field where the others read the table
        let last = match (filter, &self.liquify) {
            (Filter::Liquify { .. }, Some(field)) => &field.bind,
            _ => &t.before_bind,
        };
        out.set_bind_group(2, last, &[]);
        out.draw(0..3, 0..1);
        drop(out);
        // liquify pushes what is edited along with it (a layer's mask) the same way
        if let (Filter::Liquify { .. }, Some(field)) = (filter, &self.liquify) {
            for more in &self.transform_more {
                let Some(target) = self.layers.get(&more.layer) else { continue };
                let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
                let mut along = render_pass(encoder, "liquify along", &target.view, clear);
                along.set_pipeline(pipeline);
                along.set_bind_group(0, &more.bind, &[]);
                along.set_bind_group(1, &self.uniform_bind, &[offset]);
                along.set_bind_group(2, &field.bind, &[]);
                along.draw(0..3, 0..1);
            }
        }
        self.dirty = self.full();
    }

    /// A step of the liquify brush: `stamps` added up over the part of the field they reach, the
    /// field there carried along by that and `keep` of it left. No stamps: the whole field.
    fn liquify_step(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        stamps: &[LiquifyStamp],
        keep: f32,
    ) {
        let (canvas_w, canvas_h) = (self.width as f32, self.height as f32);
        let chosen = self.selection.as_ref().map(|s| &s.view);
        let Some(field) = self.liquify.as_mut() else { return };
        let (width, height) = (field.texture.width(), field.texture.height());
        let [across, down] = field.scale;
        // the stamps' reach, in field pixels
        let (x0, y0, x1, y1) = if stamps.is_empty() {
            (0, 0, width, height)
        } else {
            let (mut left, mut top, mut right, mut bottom) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for st in stamps {
                let ([x, y], r) = (st.centre, st.radius.max(LIQUIFY_LEAST_RADIUS));
                left = left.min(x - r);
                top = top.min(y - r);
                right = right.max(x + r);
                bottom = bottom.max(y + r);
            }
            let low = |v: f32, most: u32| (v.floor() as i64 - 1).clamp(0, i64::from(most)) as u32;
            let high = |v: f32, most: u32| (v.ceil() as i64 + 1).clamp(0, i64::from(most)) as u32;
            (
                low(left * across, width),
                low(top * down, height),
                high(right * across, width),
                high(bottom * down, height),
            )
        };
        let (area_w, area_h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        if area_w == 0 || area_h == 0 {
            return;
        }
        let scratch = match field.scratch.take() {
            Some(s) if s.width >= area_w && s.height >= area_h => s,
            old => {
                // each way up to the next size a little over, to do this seldom
                let wide = old.as_ref().map_or(0, |s| s.width).max(area_w.next_multiple_of(128)).min(width);
                let high = old.as_ref().map_or(0, |s| s.height).max(area_h.next_multiple_of(128)).min(height);
                let sums = texture(device, "liquify sums", wide, high, LIQUIFY_SUMS_FORMAT);
                let next = texture(device, "liquify step", wide, high, self.pipelines.liquify_format);
                LiquifyScratch {
                    width: wide,
                    height: high,
                    sums: sums.create_view(&wgpu::TextureViewDescriptor::default()),
                    next_view: next.create_view(&wgpu::TextureViewDescriptor::default()),
                    next,
                }
            }
        };
        let area = [x0 as f32, y0 as f32, area_w as f32, area_h as f32];
        let canvas = [canvas_w, canvas_h, across, down];
        let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        let mut pass = render_pass(encoder, "liquify stamps", &scratch.sums, clear);
        pass.set_viewport(0.0, 0.0, area[2], area[3], 0.0, 1.0);
        pass.set_pipeline(&self.pipelines.liquify_stamp);
        for st in stamps {
            let (mode, narrow) = st.mode.code();
            let offset = uniforms.push(&LiquifyUniform {
                stamp: [st.centre[0], st.centre[1], st.radius.max(LIQUIFY_LEAST_RADIUS), st.amount],
                way: [st.way[0], st.way[1], mode, narrow],
                canvas,
                area,
            });
            pass.set_bind_group(0, &self.uniform_bind, &[offset]);
            pass.draw(0..6, 0..1);
        }
        drop(pass);
        let reads = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("liquify step"),
            layout: &self.pipelines.liquify_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&scratch.sums) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&field.view) },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(chosen.unwrap_or(&self.everything)),
                },
            ],
        });
        let offset = uniforms.push(&LiquifyUniform {
            stamp: [keep, 0.0, 0.0, 0.0],
            way: [0.0, 0.0, 0.0, if chosen.is_some() { 1.0 } else { 0.0 }],
            canvas,
            area,
        });
        let mut pass = render_pass(encoder, "liquify step", &scratch.next_view, clear);
        pass.set_viewport(0.0, 0.0, area[2], area[3], 0.0, 1.0);
        pass.set_pipeline(&self.pipelines.liquify_step);
        pass.set_bind_group(0, &self.uniform_bind, &[offset]);
        pass.set_bind_group(1, &reads, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        encoder.copy_texture_to_texture(
            scratch.next.as_image_copy(),
            wgpu::TexelCopyTextureInfo {
                texture: &field.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: x0, y: y0, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            extent(area_w, area_h),
        );
        field.scratch = Some(scratch);
    }

    /// `view` bound as an adjustment's source, with where it shows when `masked`: what is painted
    /// on, or else the selection.
    fn adjustment_bind(&self, device: &wgpu::Device, view: &wgpu::TextureView, masked: bool) -> wgpu::BindGroup {
        let painted = self.pencil.as_ref().map(|p| &p.view);
        let chosen =
            painted.or(self.selection.as_ref().map(|s| &s.view)).filter(|_| masked).unwrap_or(&self.everything);
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("adjustment pass"),
            layout: &self.pipelines.transform_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(chosen) },
            ],
        })
    }

    /// The edit's copy blurred by `sigma` canvas pixels, at a smaller size for wide blurs (halved
    /// until the blur is a few pixels wide where it runs); none when the blur is too small to do.
    fn blur_copy(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        sigma: f32,
    ) -> Option<wgpu::TextureView> {
        let device = &gpu.device;
        let t = self.transforming.as_ref()?;
        let (w, h) = (self.width, self.height);
        let mut scale = 1u32;
        while sigma / scale as f32 > 8.0 && scale < 32 && w / (scale * 2) > 0 && h / (scale * 2) > 0 {
            scale *= 2;
        }
        let small = |s: u32| (w.div_ceil(s).max(1), h.div_ceil(s).max(1));
        let mut pass = |encoder: &mut wgpu::CommandEncoder,
                        target: &wgpu::TextureView,
                        fs: &str,
                        source: &wgpu::BindGroup,
                        uniform: FilterUniform| {
            let Some(pipeline) = self.pipelines.filters.get(fs) else { return };
            let offset = uniforms.push(&uniform);
            let mut p = render_pass(encoder, "adjustment", target, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            p.set_pipeline(pipeline);
            p.set_bind_group(0, source, &[]);
            p.set_bind_group(1, &self.uniform_bind, &[offset]);
            p.set_bind_group(2, &t.before_bind, &[]);
            p.draw(0..3, 0..1);
        };
        let mut current: Option<wgpu::TextureView> = None;
        let mut level = 1u32;
        while level < scale {
            level *= 2;
            let (lw, lh) = small(level);
            let next = self.blur_texture(device, "adjustment level", lw, lh, 0);
            let uniform =
                FilterUniform { a: [0.0; 4], b: [0.0; 4], size: [lw as f32, lh as f32, 0.0, 0.0], c: [0.0; 4] };
            let source = current.as_ref().map(|v| self.adjustment_bind(device, v, false));
            pass(encoder, &next, "fs_down", source.as_ref().unwrap_or(&t.bind), uniform);
            current = Some(next);
        }
        let s_sigma = sigma / scale as f32;
        if s_sigma < 0.3 {
            return current;
        }
        let (lw, lh) = small(scale);
        let reach = (s_sigma * 3.0).ceil().min(64.0);
        let size = [lw as f32, lh as f32, 0.0, 0.0];
        // the canvas's size: the first pass's into the scratch texture, read straight back (one
        // texture the canvas's size fewer to make: a tablet takes 15-25 ms for each)
        let across = if (lw, lh) == (w, h) {
            self.scratch.get(device).1.clone()
        } else {
            self.blur_texture(device, "adjustment across", lw, lh, 1)
        };
        let down = self.blur_texture(device, "adjustment down", lw, lh, 2);
        let source = current.as_ref().map(|v| self.adjustment_bind(device, v, false));
        pass(
            encoder,
            &across,
            "fs_blur",
            source.as_ref().unwrap_or(&t.bind),
            FilterUniform { a: [1.0, 0.0, s_sigma, reach], b: [0.0; 4], size, c: [0.0; 4] },
        );
        let source = self.adjustment_bind(device, &across, false);
        pass(
            encoder,
            &down,
            "fs_blur",
            &source,
            FilterUniform { a: [0.0, 1.0, s_sigma, reach], b: [0.0; 4], size, c: [0.0; 4] },
        );
        Some(down)
    }

    /// A blur's working texture `width` x `height` for `part` (0 a halving, 1 across, 2 down),
    /// made the first time and kept until the adjustment ends.
    fn blur_texture(&self, device: &wgpu::Device, label: &str, width: u32, height: u32, part: u8) -> wgpu::TextureView {
        let mut kept = self.blur_textures.lock().unwrap_or_else(PoisonError::into_inner);
        kept.entry((width, height, part))
            .or_insert_with(|| {
                texture(device, label, width, height, LAYER_FORMAT).create_view(&wgpu::TextureViewDescriptor::default())
            })
            .clone()
    }

    /// A new step: it joins the history and anything undone can no longer be redone.
    fn push_step(&mut self, mut step: Step) {
        self.steps_made += 1;
        step.made = self.steps_made;
        if step.name.is_none() {
            step.name = self.next_name.take();
        }
        for dropped in std::mem::take(&mut self.redo) {
            self.keep_spare_snapshot(dropped);
        }
        self.undo.push(step);
        let mut bytes: u64 = self.undo.iter().map(Step::bytes).sum();
        while self.undo.len() > MAX_UNDO || (bytes > MAX_UNDO_BYTES && self.undo.len() > 1) {
            let dropped = self.undo.remove(0);
            bytes = bytes.saturating_sub(dropped.bytes());
            self.keep_spare_snapshot(dropped);
        }
    }

    /// A step let go: its textures the canvas's size kept as spares.
    fn keep_spare_snapshot(&self, step: Step) {
        for s in step.pixels {
            self.keep_spare(s.texture);
        }
    }

    /// `texture` kept for reuse if it is the canvas's size and can be a layer, and fewer than
    /// [`SPARES_KEPT`] are kept.
    fn keep_spare(&self, texture: wgpu::Texture) {
        let layer_like = wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST;
        let mut spares = self.spares.lock().unwrap_or_else(PoisonError::into_inner);
        if (texture.width(), texture.height()) == (self.width, self.height)
            && texture.format() == LAYER_FORMAT
            && texture.usage().contains(layer_like)
            && spares.len() < SPARES_KEPT
        {
            spares.push(texture);
        }
    }

    /// A kept texture the canvas's size, if there is one (its pixels whatever they were).
    fn take_spare(&self) -> Option<wgpu::Texture> {
        self.spares.lock().unwrap_or_else(PoisonError::into_inner).pop()
    }

    /// Composites the visible layers over the background inside `rect`, into `composite[0]`.
    /// Normal layers blend in place; other modes read what is below and write the other texture.
    fn composite(
        &self,
        device: &wgpu::Device,
        doc: &Document,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        rect: IRect,
    ) {
        let [r, g, b] = doc.background;
        let solid = if doc.background_visible { [r, g, b, 1.0] } else { [0.0; 4] };
        {
            let offset = uniforms.push(&LayerUniform { opacity: 1.0, solid, ..Default::default() });
            let mut pass = render_pass(encoder, "background", &self.composite_view, wgpu::LoadOp::Load);
            scissor(&mut pass, rect);
            pass.set_pipeline(&self.pipelines.solid);
            pass.set_bind_group(0, &self.stroke_bind, &[]);
            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
            pass.draw(0..3, 0..1);
        }
        // the stack bottom up; a group's layers go onto its depth's pair of textures first, and
        // the group onto the stack where its entry is
        let main = Stack {
            first: (&self.composite, &self.composite_view),
            second: Second::OnDemand(&self.composite_second, device),
        };
        if let Some(c) = &self.caches {
            // what is below as it was, what is worked on, then what is above as it was
            encoder.copy_texture_to_texture(
                copy_at(&c.below[0], rect.x0, rect.y0),
                copy_at(&self.composite, rect.x0, rect.y0),
                extent(rect.width(), rect.height()),
            );
            let mut current = 0;
            self.composite_items(device, doc, encoder, uniforms, rect, &c.live, true, &main, &mut current);
            if !c.above_items.is_empty() {
                let uniform = LayerUniform {
                    opacity: 1.0,
                    blend: BlendMode::Normal.id(),
                    tint: Look::PLAIN.tint,
                    ..Default::default()
                };
                let view = &c.above_views[0];
                self.draw_onto(
                    device,
                    encoder,
                    uniforms,
                    rect,
                    view,
                    None,
                    &self.stroke_view,
                    uniform,
                    BlendMode::Normal,
                    &main,
                    &mut current,
                );
            }
            main.settle(encoder, rect, current);
            return;
        }
        let current = self.composite_members(device, doc, encoder, uniforms, rect, None, &main);
        main.settle(encoder, rect, current);
    }

    /// The top level's items composited live while caches are used (from the one holding the
    /// current layer up to a run of plain items at the top: normal, the first not clipped to what
    /// is below the run), with every layer in them; none while something else is under way
    /// (Animation Assist's frames, an adjustment, liquify, a transform, a stroke elsewhere).
    fn live_items(&self, doc: &Document) -> Option<(Vec<LayerId>, std::ops::Range<usize>)> {
        if doc.animation.is_some() {
            return None;
        }
        let tops = doc.members(None);
        let mut top = doc.current();
        if doc.layer(top).is_some_and(|l| l.kind == LayerKind::Mask) {
            top = doc.layer(top)?.parent?;
        }
        for _ in 0..doc.layers().len() {
            match doc.group_of(top) {
                Some(g) => top = g,
                None => break,
            }
        }
        let start = tops.iter().position(|t| *t == top)?;
        let layer = |i: usize| tops.get(i).and_then(|t| doc.layer(*t));
        let mut end = tops.len();
        while end > start + 1 && tops.get(end - 1).is_some_and(|t| plain(doc, *t)) {
            end -= 1;
        }
        while end < tops.len() && layer(end).is_some_and(|l| l.clipped) {
            end += 1;
        }
        let live: HashSet<LayerId> = tops.get(start..end)?.iter().flat_map(|t| item_layers(doc, *t)).collect();
        // what is being worked on (a stroke, a smudge, a transform or adjustment, a held edit)
        // must be among them
        let worked = self
            .active
            .map(|a| a.layer)
            .into_iter()
            .chain(self.smudge.as_ref().map(|s| s.layer))
            .chain(self.transforming.iter().chain(&self.transform_more).map(|t| t.layer))
            .chain(self.held.as_ref().map(|h| h.layer));
        if worked.into_iter().any(|id| !live.contains(&id)) {
            return None;
        }
        Some((tops, start..end))
    }

    /// Makes (or keeps) the caches the composite is made around, while they are used: what is
    /// below the items worked on and what is above them, each remade only when what it was made
    /// from changed.
    fn ensure_caches(
        &mut self,
        gpu: &Gpu,
        doc: &Document,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
    ) {
        let device = &gpu.device;
        let live = self.live_items(doc).filter(|_| self.cache_composites);
        // nothing below or above what is worked on: no picture of it would save any compositing
        let alone = live.as_ref().is_some_and(|(tops, live)| live.start == 0 && live.end >= tops.len());
        if alone {
            self.remake = None;
        }
        let Some((tops, live)) = live.filter(|_| !alone) else {
            // composited without them: their textures let go too, not kept for caches that may not
            // come back (a large canvas's four take hundreds of MB)
            self.caches = None;
            self.spare_caches = None;
            // everything shown composites: Animation Assist's frames around the current one, or
            // every layer
            let needed: Vec<LayerId> = match self.shown_frames(doc) {
                Some(shown) => shown.into_iter().collect(),
                None => doc.pixel_layer_ids().collect(),
            };
            self.restore_all(gpu, needed, false);
            return;
        };
        let below_items = tops.get(..live.start).unwrap_or_default().to_vec();
        let above_items = tops.get(live.end..).unwrap_or_default().to_vec();
        let below_key = items_key(doc, &below_items, true);
        let above_key = items_key(doc, &above_items, false);
        // a remake of these same caches goes on; of others, starts again
        let same = |r: &Remake| {
            r.below_items == below_items
                && r.below_key == below_key
                && r.above_items == above_items
                && r.above_key == above_key
        };
        if self.remake.as_ref().is_some_and(same) {
            self.send_now(gpu, encoder, uniforms);
            self.go_on_remaking(gpu, doc);
            return;
        }
        // or made in the textures of those let go or of a remake left off: what they hold is
        // made again
        let stale = self.remake.take().map(|r| r.caches).or_else(|| self.spare_caches.take()).map(|mut c| {
            c.below_items.clear();
            c.below_key = !below_key;
            c.above_items.clear();
            c.above_key = !above_key;
            c
        });
        let mut caches = self.caches.take().or(stale).unwrap_or_else(|| {
            let pair = |label| {
                let textures = [
                    texture(device, label, self.width, self.height, LAYER_FORMAT),
                    texture(device, label, self.width, self.height, LAYER_FORMAT),
                ];
                let views = textures.each_ref().map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
                (textures, views)
            };
            let ((below, below_views), (above, above_views)) = (pair("below"), pair("above"));
            Caches {
                live: Vec::new(),
                live_layers: HashSet::new(),
                below,
                below_views,
                below_items: Vec::new(),
                // nothing matches until made
                below_key: !below_key,
                above,
                above_views,
                above_items: Vec::new(),
                above_key: !above_key,
            }
        });
        let full = self.full();
        let remake_below = caches.below_items != below_items || caches.below_key != below_key;
        let remake_above = caches.above_items != above_items || caches.above_key != above_key;
        // what is composited needs its layers: the items worked on (and what the first is clipped
        // to), and what is cached when it is made again
        let mut needed: Vec<LayerId> =
            tops.get(live.clone()).unwrap_or_default().iter().flat_map(|t| item_layers(doc, *t)).collect();
        needed.extend(tops.get(live.start).into_iter().flat_map(|first| clip_base_and_mask(doc, *first)));
        if remake_below {
            needed.extend(below_items.iter().flat_map(|t| item_layers(doc, *t)));
        }
        if remake_above {
            needed.extend(above_items.iter().flat_map(|t| item_layers(doc, *t)));
        }
        let most = self.most_resident();
        let missing = needed.iter().filter(|id| !self.layers.contains_key(*id)).count();
        if self.layers.len().saturating_add(missing) > most {
            // more than fit: remade a part at a time, a little each render, each part's layers let
            // go before the next comes back; the composite waits for it
            let mut keep: HashSet<LayerId> =
                tops.get(live.clone()).unwrap_or_default().iter().flat_map(|t| item_layers(doc, *t)).collect();
            keep.extend(tops.get(live.start).into_iter().flat_map(|first| clip_base_and_mask(doc, *first)));
            keep.extend(self.active.map(|a| a.layer));
            keep.extend(self.held.as_ref().map(|h| h.layer));
            self.restore_all(gpu, keep.iter().copied().collect::<Vec<_>>(), false);
            // the work so far first, so what is cached is made from it
            self.send_now(gpu, encoder, uniforms);
            let mut start = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("remake") });
            let mut start_uniforms = UniformData::default();
            if remake_below {
                let [r, g, b] = doc.background;
                let solid = if doc.background_visible { [r, g, b, 1.0] } else { [0.0; 4] };
                let offset = start_uniforms.push(&LayerUniform { opacity: 1.0, solid, ..Default::default() });
                let mut pass = render_pass(
                    &mut start,
                    "below",
                    &caches.below_views[0],
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                );
                pass.set_pipeline(&self.pipelines.solid);
                pass.set_bind_group(0, &self.stroke_bind, &[]);
                pass.set_bind_group(1, &self.uniform_bind, &[offset]);
                pass.draw(0..3, 0..1);
            }
            if remake_above {
                drop(render_pass(
                    &mut start,
                    "above",
                    &caches.above_views[0],
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                ));
            }
            self.write_uniforms(gpu, &start_uniforms);
            gpu.queue.submit([start.finish()]);
            caches.live = tops.get(live).unwrap_or_default().to_vec();
            caches.live_layers = caches.live.iter().flat_map(|t| item_layers(doc, *t)).collect();
            self.remake = Some(Remake {
                caches,
                below: remake_below.then_some((0, 0)),
                above: remake_above.then_some((0, 0)),
                below_items,
                below_key,
                above_items,
                above_key,
                keep,
                started: std::time::Instant::now(),
                slices: 0,
                largest_part: 0,
                restored: 0,
                restoring: std::time::Duration::ZERO,
            });
            self.go_on_remaking(gpu, doc);
            return;
        }
        self.restore_all(gpu, needed, false);
        if remake_below {
            let [r, g, b] = doc.background;
            let solid = if doc.background_visible { [r, g, b, 1.0] } else { [0.0; 4] };
            let offset = uniforms.push(&LayerUniform { opacity: 1.0, solid, ..Default::default() });
            {
                let mut pass = render_pass(
                    encoder,
                    "below",
                    &caches.below_views[0],
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                );
                pass.set_pipeline(&self.pipelines.solid);
                pass.set_bind_group(0, &self.stroke_bind, &[]);
                pass.set_bind_group(1, &self.uniform_bind, &[offset]);
                pass.draw(0..3, 0..1);
            }
            let stack = Stack::pair(&caches.below, &caches.below_views);
            let mut current = 0;
            self.composite_items(device, doc, encoder, uniforms, full, &below_items, true, &stack, &mut current);
            stack.settle(encoder, full, current);
            caches.below_items = below_items;
            caches.below_key = below_key;
        }
        if remake_above {
            drop(render_pass(encoder, "above", &caches.above_views[0], wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)));
            let stack = Stack::pair(&caches.above, &caches.above_views);
            let mut current = 0;
            self.composite_items(device, doc, encoder, uniforms, full, &above_items, true, &stack, &mut current);
            stack.settle(encoder, full, current);
            caches.above_items = above_items;
            caches.above_key = above_key;
        }
        caches.live = tops.get(live).unwrap_or_default().to_vec();
        caches.live_layers = caches.live.iter().flat_map(|t| item_layers(doc, *t)).collect();
        self.caches = Some(caches);
    }

    /// With Animation Assist on (and nothing else under way), the layers of the frames shown: the
    /// current (or played) one, its onion skins, and a background or foreground frame.
    fn shown_frames(&self, doc: &Document) -> Option<HashSet<LayerId>> {
        let a = doc.animation.as_ref()?;
        let busy =
            self.transforming.is_some() || self.liquify.is_some() || self.pencil.is_some() || self.held.is_some();
        if busy || self.smudge.is_some() {
            return None;
        }
        let frames = animation::frames(doc);
        let focus = self.play_frame.or_else(|| animation::current_frame(doc))?;
        let shown: HashSet<LayerId> = frames
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                !matches!(animation::look(a, *i, frames.len(), focus, self.play_frame.is_some()), FrameLook::Hidden)
            })
            .flat_map(|(_, f)| item_layers(doc, *f))
            .collect();
        Some(shown)
    }

    /// How many layers' textures fit the budget (any number without one).
    pub fn most_resident(&self) -> usize {
        if self.resident_budget == 0 {
            return usize::MAX;
        }
        // the history takes from the same memory, a layer's worth at a time
        let per = (u64::from(self.width) * u64::from(self.height) * 4).max(1);
        let room = self.resident_budget.saturating_sub(self.history_bytes() / per * per);
        most_resident(room.max(1), self.width, self.height)
    }

    /// Lets go of the textures of layers unchanged since parked (their compressed copies kept),
    /// but those in `keep`, as far as the next part of a remake wants room.
    fn let_go_clean(&mut self, keep: &HashSet<LayerId>) {
        // only as many as the next part wants room for, needed least lately first: layers that
        // fit stay (letting many textures go at once holds a frame up)
        let most = self.most_resident();
        let wanted = most.saturating_sub(LET_GO_ROOM);
        if self.layers.len() <= wanted {
            return;
        }
        let mut clean: Vec<LayerId> =
            self.layers.keys().filter(|id| !keep.contains(*id) && self.parked.contains_key(*id)).copied().collect();
        clean.sort_by_key(|id| self.needed_at.get(id).copied().unwrap_or(0));
        for id in clean {
            if self.layers.len() <= wanted {
                break;
            }
            self.layers.remove(&id);
        }
    }

    /// Puts `uniforms` in the uniform buffer for the work about to be sent; one that would not fit
    /// (the buffer sized for less) is said in the log, as the device refuses it.
    fn write_uniforms(&self, gpu: &Gpu, uniforms: &UniformData) {
        if uniforms.data.is_empty() {
            return;
        }
        if uniforms.data.len() as u64 > self.uniform_slots * SLOT {
            log::error!("uniforms outgrew their buffer ({} bytes, {} slots)", uniforms.data.len(), self.uniform_slots);
        }
        gpu.queue.write_buffer(&self.uniform_buffer, 0, &uniforms.data);
    }

    /// Sends the work recorded so far (with its uniforms), going on with a new encoder.
    fn send_now(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, uniforms: &mut UniformData) {
        let fresh = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("canvas") });
        let done = std::mem::replace(encoder, fresh);
        self.write_uniforms(gpu, uniforms);
        gpu.queue.submit([done.finish()]);
        *uniforms = UniformData::default();
    }

    /// Sends the work recorded so far with the three kinds of uniforms a render's painting uses,
    /// going on with a new encoder (see [`WORK_AT_ONCE`]).
    fn send_piece(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        (uniforms, smudge, wet): (&mut UniformData, &mut UniformData, &mut UniformData),
    ) {
        if !smudge.data.is_empty() {
            gpu.queue.write_buffer(&self.smudge_params, 0, &smudge.data);
        }
        if !wet.data.is_empty() {
            gpu.queue.write_buffer(&self.wet_params, 0, &wet.data);
        }
        self.send_now(gpu, encoder, uniforms);
        *smudge = UniformData::default();
        *wet = UniformData::default();
        self.pieces_sent = self.pieces_sent.wrapping_add(1);
    }

    /// Goes on remaking the caches for a while (parts of whole items, each part's layers brought
    /// back, composited, sent, then let go before the next part's come back); once both are done,
    /// they are the caches and the whole composite is made again.
    fn go_on_remaking(&mut self, gpu: &Gpu, doc: &Document) {
        let Some(mut r) = self.remake.take() else { return };
        let device = &gpu.device;
        let full = self.full();
        let deadline = std::time::Instant::now() + REMAKE_SLICE;
        let (mut restored, mut restoring, mut largest) = (0, std::time::Duration::ZERO, 0);
        loop {
            let (below, items, progress) = match (r.below, r.above) {
                (Some(p), _) => (true, &r.below_items, p),
                (None, Some(p)) => (false, &r.above_items, p),
                (None, None) => break,
            };
            let (mut at, mut current) = progress;
            let (textures, views) =
                if below { (&r.caches.below, &r.caches.below_views) } else { (&r.caches.above, &r.caches.above_views) };
            let stack = Stack::pair(textures, views);
            // as many whole items as fit and the slice's time allows (at least one), each with what
            // it is clipped to; an item painted nowhere is passed over, a normal one composites only
            // where it is painted
            let room = self.most_resident().saturating_sub(self.layers.len()).max(1);
            let mut part = Vec::new();
            let mut layers = Vec::new();
            let mut staged = 0;
            while let Some(&item) = items.get(at) {
                let mut more = item_layers(doc, item);
                more.extend(clip_base_and_mask(doc, item));
                // the time counts bringing layers back, not the compositing after: the count
                // keeps a part whose layers come back quickly from compositing too long
                let late = std::time::Instant::now() >= deadline;
                let full_part = part.len() >= MOST_PART_ITEMS || layers.len() + more.len() > room;
                if !part.is_empty() && (late || full_part) {
                    break;
                }
                at += 1;
                let painted = self.painted(&more).intersect(full);
                if painted.is_empty() {
                    continue;
                }
                // a normal item draws in place; another mode writes the stack's other texture, which
                // has to be whole
                let normal = doc.layer(item).is_some_and(|l| l.blend == BlendMode::Normal);
                let rect = if normal { painted } else { full };
                // brought back now (the slow part, so the time taken counts), the staged pixels sent
                // on now and then so they don't pile up
                staged += more.len();
                let restore_started = std::time::Instant::now();
                self.restore_all(gpu, more.iter().copied(), false);
                restoring += restore_started.elapsed();
                restored += more.len();
                if staged >= 8 {
                    gpu.queue.submit(std::iter::empty());
                    staged = 0;
                }
                layers.extend(more);
                part.push((item, rect));
            }
            largest = largest.max(part.len());
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("cache part") });
            let mut uniforms = UniformData::default();
            for (item, rect) in &part {
                self.composite_items(
                    device,
                    doc,
                    &mut encoder,
                    &mut uniforms,
                    *rect,
                    &[*item],
                    true,
                    &stack,
                    &mut current,
                );
            }
            let done = at >= items.len();
            if done {
                stack.settle(&mut encoder, full, current);
            }
            self.write_uniforms(gpu, &uniforms);
            gpu.queue.submit([encoder.finish()]);
            self.let_go_clean(&r.keep);
            gpu.flush();
            let next = (!done).then_some((at, current));
            if below {
                r.below = next;
            } else {
                r.above = next;
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
        }
        r.slices += 1;
        r.restored += restored;
        r.restoring += restoring;
        r.largest_part = r.largest_part.max(largest);
        if r.below.is_some() || r.above.is_some() {
            self.remake = Some(r);
            return;
        }
        log::info!(
            "caches remade in {} ms over {} renders: {} items (at most {} a part), {} layers brought back in {} ms",
            r.started.elapsed().as_millis(),
            r.slices,
            r.below_items.len() + r.above_items.len(),
            r.largest_part,
            r.restored,
            r.restoring.as_millis()
        );
        self.last_remake = Some(RemakeStats { renders: r.slices, largest_part: r.largest_part });
        // made: they are the caches, and the whole picture is composited again around them
        let Remake { mut caches, below_items, below_key, above_items, above_key, .. } = r;
        caches.below_items = below_items;
        caches.below_key = below_key;
        caches.above_items = above_items;
        caches.above_key = above_key;
        self.caches = Some(caches);
        self.dirty = self.full();
    }

    /// Whether another layer's texture fits the budget.
    pub fn has_room(&self) -> bool {
        self.layers.len() < self.most_resident()
    }

    /// Keeps layer `id`'s pixels (premultiplied RGBA, the canvas's size) compressed, without a
    /// texture, as a layer parked: for an artwork opening with more layers than fit.
    pub fn park_pixels(&mut self, id: LayerId, rgba: &[u8]) -> Result<(), GpuError> {
        let expected = self.width as usize * self.height as usize * 4;
        if rgba.len() != expected {
            return Err(GpuError::Readback(format!("layer {id:?}: {} bytes, expected {expected}", rgba.len())));
        }
        self.park_ready(id, ReadyToPark::of(rgba, self.width, self.height));
        Ok(())
    }

    /// As [`Canvas::upload_layer`], keeping `ready` (made from the same pixels by
    /// [`ReadyToPark::of`]) as the layer's compressed copy: letting its texture go later, while
    /// it is unchanged, costs nothing.
    pub fn upload_layer_kept(
        &mut self,
        gpu: &Gpu,
        id: LayerId,
        rgba: &[u8],
        ready: ReadyToPark,
        painted: Option<IRect>,
    ) -> Result<(), GpuError> {
        self.upload_layer_painted(gpu, id, rgba, painted)?;
        self.parked.insert(id, ready.parked);
        self.pictures.insert(id, ready.pictures);
        Ok(())
    }

    /// Parks layer `id` with pixels already made ready (by [`ReadyToPark::of`], with this
    /// canvas's size).
    pub fn park_ready(&mut self, id: LayerId, ready: ReadyToPark) {
        self.layers.remove(&id);
        self.parked.insert(id, ready.parked);
        self.pictures.insert(id, ready.pictures);
        self.invalidate();
    }

    /// The most uniform slots `op` takes in a render (among `ops`, the render's work): a liquify
    /// drag's stamps one each, a transform one for each layer it moves (those the same render adds
    /// too), a filter one for each halving of a wide blur and its passes; the rest one or none.
    fn slots_for(&self, op: &Op, ops: &[Op]) -> usize {
        match op {
            Op::LiquifyStamps(stamps) => stamps.len() + 1,
            Op::TransformSet { .. } | Op::TransformMesh { .. } => {
                let added = ops.iter().filter(|o| matches!(o, Op::TransformAlso(_))).count();
                1 + self.transform_more.len() + added
            }
            Op::FilterSet(_) | Op::PluginFilter(_) => FILTER_SLOTS,
            _ => 2,
        }
    }

    /// Whether work keeps the caches: it changes only the pixels of layers worked on (painting,
    /// transforms and adjustments, held edits, clearing, and undoing or redoing those).
    fn keeps_caches(&self, op: &Op, live: &HashSet<LayerId>) -> bool {
        let only_live = |step: Option<&Step>| {
            step.is_some_and(|s| {
                s.doc.is_none() && s.flip.is_none() && s.pixels.iter().all(|p| live.contains(&p.layer))
            })
        };
        let transformed_live = || self.transforming.iter().chain(&self.transform_more).all(|t| live.contains(&t.layer));
        match op {
            Op::Stamps { layer, .. } => live.contains(layer),
            Op::Commit(stroke) => live.contains(&stroke.layer),
            Op::Cancel
            | Op::SaveTip(_)
            | Op::Untip(_)
            | Op::Mark
            | Op::Name(_)
            | Op::OutsideChange(_)
            | Op::Collapse
            | Op::Within(_) => true,
            Op::Undo => only_live(self.undo.last()),
            Op::Redo => only_live(self.redo.last()),
            Op::TransformBegin(id)
            | Op::TransformAlso(id)
            | Op::Hold(id)
            | Op::Put(id, _)
            | Op::PutRegion { layer: id, .. }
            | Op::Clear(id, _)
            | Op::Invert(id, _)
            | Op::Fill(id, _)
            | Op::Wipe(id) => live.contains(id),
            Op::TransformSet { .. }
            | Op::TransformMesh { .. }
            | Op::FilterSet(_)
            | Op::PluginFilter(_)
            | Op::LiquifyBegin
            | Op::PencilBegin
            | Op::LiquifyStamps(_)
            | Op::LiquifyScale(_)
            | Op::LiquifyClear
            | Op::TransformEnd { .. } => transformed_live(),
            Op::Release { before: None, .. } => self.held.as_ref().is_none_or(|h| live.contains(&h.layer)),
            Op::Write { layers, before: None } => layers.iter().all(|(id, ..)| live.contains(id)),
            _ => false,
        }
    }

    /// The members of `group` (None: the top level) bottom up onto `stack`; returns which of its
    /// textures holds the result.
    #[allow(clippy::too_many_arguments)]
    fn composite_members(
        &self,
        device: &wgpu::Device,
        doc: &Document,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        rect: IRect,
        group: Option<LayerId>,
        stack: &Stack<'_>,
    ) -> usize {
        let mut current = 0usize;
        self.composite_items(
            device,
            doc,
            encoder,
            uniforms,
            rect,
            &doc.members(group),
            group.is_none(),
            stack,
            &mut current,
        );
        current
    }

    /// `items` (a level's members, bottom up; `top` the top level's) onto `stack`, whose result
    /// is in the stack's first texture (`current` 0) or its second (1).
    #[allow(clippy::too_many_arguments)]
    fn composite_items(
        &self,
        device: &wgpu::Device,
        doc: &Document,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        rect: IRect,
        items: &[LayerId],
        top: bool,
        stack: &Stack<'_>,
        current: &mut usize,
    ) {
        // Animation Assist: the top level's frames show around the current (or played) one
        let animation = if top { doc.animation.as_ref() } else { None };
        let frames = animation.map(|_| animation::frames(doc)).unwrap_or_default();
        let focus = animation.and_then(|_| self.play_frame.or_else(|| animation::current_frame(doc)));
        for &id in items {
            let Some(layer) = doc.layer(id) else { continue };
            // another layer shown alone
            if doc.soloed_out(id) {
                continue;
            }
            let look = match (animation, focus, frames.iter().position(|f| *f == id)) {
                (Some(a), Some(c), Some(i)) => {
                    match animation::look(a, i, frames.len(), c, self.play_frame.is_some()) {
                        FrameLook::Shown => Look::PLAIN,
                        FrameLook::Onion { color, opacity } => {
                            Look { tint: [color[0], color[1], color[2], 1.0], opacity }
                        }
                        FrameLook::Hidden => continue,
                    }
                }
                _ => Look::PLAIN,
            };
            match layer.kind {
                LayerKind::Mask => {}
                LayerKind::Paint => {
                    self.composite_layer(device, doc, encoder, uniforms, rect, layer, look, stack, current)
                }
                LayerKind::Group => {
                    if !layer.visible || layer.opacity <= 0.0 {
                        continue;
                    }
                    // its members straight onto what is below (an onion skin's frame on its own,
                    // to be tinted whole)
                    if layer.passes_through() && look.tint[3] == 0.0 {
                        let members = doc.members(Some(id));
                        self.composite_items(device, doc, encoder, uniforms, rect, &members, false, stack, current);
                        continue;
                    }
                    let Some(view) = self.composite_group(device, doc, encoder, uniforms, rect, id) else { continue };
                    let uniform = LayerUniform {
                        opacity: layer.opacity.clamp(0.0, 1.0) * look.opacity,
                        blend: layer.blend.id(),
                        tint: look.tint,
                        ..Default::default()
                    };
                    self.draw_onto(
                        device,
                        encoder,
                        uniforms,
                        rect,
                        view,
                        None,
                        &self.stroke_view,
                        uniform,
                        layer.blend,
                        stack,
                        current,
                    );
                }
            }
        }
    }

    /// Group `id`'s members composited over transparency on the textures for its depth; returns
    /// the view holding the result.
    fn composite_group(
        &self,
        device: &wgpu::Device,
        doc: &Document,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        rect: IRect,
        id: LayerId,
    ) -> Option<&wgpu::TextureView> {
        let (textures, views) = self.groups.get(doc.depth(id))?;
        let inner = Stack::pair(textures, views);
        drop(render_pass(encoder, "group clear", &views[0], wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)));
        let at = self.composite_members(device, doc, encoder, uniforms, rect, Some(id), &inner);
        inner.settle(encoder, rect, at);
        Some(&views[0])
    }

    /// One paint layer (through its mask, with the stroke being drawn on it or its mask) onto a
    /// stack whose result is in its first texture (`current` 0) or its second (1).
    #[allow(clippy::too_many_arguments)]
    fn composite_layer(
        &self,
        device: &wgpu::Device,
        doc: &Document,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        rect: IRect,
        layer: &Layer,
        look: Look,
        stack: &Stack<'_>,
        current: &mut usize,
    ) {
        let clip = doc.clip_base(layer.id);
        let clip_visible = clip.is_none_or(|id| doc.layer(id).is_some_and(|l| l.visible));
        if !layer.visible || !clip_visible || layer.opacity <= 0.0 {
            return;
        }
        let Some(gpu_layer) = self.layers.get(&layer.id) else { return };
        let clip_view = clip.and_then(|id| self.layers.get(&id)).map_or(&self.stroke_view, |c| &c.view);
        // clipping follows the base as its mask shows it
        let clip_mask_view = clip
            .and_then(|b| doc.mask_of(b))
            .filter(|m| doc.layer(*m).is_some_and(|l| l.visible))
            .and_then(|m| self.layers.get(&m))
            .map(|m| &m.view);
        let mask = doc.mask_of(layer.id).filter(|m| doc.layer(*m).is_some_and(|l| l.visible));
        let mask_view = mask.and_then(|m| self.layers.get(&m)).map(|m| &m.view);
        // the stroke being drawn shows on its layer, or through the mask it is painting
        let stroke = self.active.filter(|s| (s.layer == layer.id || Some(s.layer) == mask) && !s.pencil);
        let uniform = LayerUniform {
            opacity: layer.opacity.clamp(0.0, 1.0) * look.opacity,
            tint: look.tint,
            stroke_mode: match stroke.map(|s| s.style.tool) {
                None => STROKE_NONE,
                // the stroke changes the layer itself
                _ if stroke.is_some_and(|s| s.style.in_place()) => STROKE_NONE,
                Some(StrokeTool::Paint) if stroke.is_some_and(|s| s.style.preserve_alpha) => STROKE_PAINT_LOCKED,
                Some(StrokeTool::Paint) => STROKE_PAINT,
                Some(StrokeTool::Erase) => STROKE_ERASE,
                Some(StrokeTool::Laid) => STROKE_LAID,
                // the smudge changes the layer itself
                Some(StrokeTool::Smudge) => STROKE_NONE,
            },
            stroke_opacity: stroke.map_or(0.0, |s| s.style.opacity.clamp(0.0, 1.0)),
            blend: layer.blend.id(),
            clipped: u32::from(clip.is_some()),
            clip_masked: if clip_mask_view.is_some() { 1.0 } else { 0.0 },
            masked: if mask_view.is_some() { 1.0 } else { 0.0 },
            stroke_on_mask: if stroke.is_some_and(|s| Some(s.layer) == mask) { 1.0 } else { 0.0 },
            ..Default::default()
        }
        .with_grain(stroke.as_ref().map(|s| &s.style))
        .with_dual(stroke.as_ref());
        let mask_view = mask_view.unwrap_or(&self.stroke_view);
        self.draw_onto(
            device,
            encoder,
            uniforms,
            rect,
            &gpu_layer.view,
            Some((clip_view, clip_mask_view.unwrap_or(&self.stroke_view))),
            mask_view,
            uniform,
            layer.blend,
            stack,
            current,
        );
    }

    /// `view` (a layer or a group's result) drawn onto a stack: "over" for normal, else blended
    /// with what is below into the stack's other texture.
    #[allow(clippy::too_many_arguments)]
    fn draw_onto(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        rect: IRect,
        view: &wgpu::TextureView,
        clip: Option<(&wgpu::TextureView, &wgpu::TextureView)>,
        mask: &wgpu::TextureView,
        uniform: LayerUniform,
        blend: BlendMode,
        stack: &Stack<'_>,
        current: &mut usize,
    ) {
        let offset = uniforms.push(&uniform);
        // the base clipped to and its mask
        let (clip, clip_mask) = clip.unwrap_or((&self.stroke_view, &self.stroke_view));
        let first = stack.first.1;
        if blend == BlendMode::Normal {
            let bind = self.quad_masked(device, [view, &self.stroke_view, clip, &self.stroke_view], mask, clip_mask);
            let target = if *current == 0 { first } else { stack.second().1 };
            let mut pass = render_pass(encoder, "layer", target, wgpu::LoadOp::Load);
            scissor(&mut pass, rect);
            pass.set_pipeline(&self.pipelines.layer);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
            pass.draw(0..3, 0..1);
        } else {
            let second = stack.second().1;
            let (backdrop, target) = if *current == 0 { (first, second) } else { (second, first) };
            let bind = self.quad_masked(device, [view, &self.stroke_view, clip, backdrop], mask, clip_mask);
            let mut pass = render_pass(encoder, "blend", target, wgpu::LoadOp::Load);
            scissor(&mut pass, rect);
            pass.set_pipeline(&self.pipelines.blend);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
            pass.draw(0..3, 0..1);
            *current = 1 - *current;
        }
    }

    fn display(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        target: &Target<'_>,
        predicted: std::ops::Range<u32>,
        glass: Option<(Backdrop<'_>, f32)>,
    ) {
        let srgb = target.format.is_srgb();
        let Pipelines {
            display_module,
            display_pipeline_layout,
            display_lut_layout,
            surround_module,
            surround_pipeline_layout,
            overlay_module,
            stamp_layout,
            selection_module,
            ..
        } = &*self.pipelines;
        // the images a predicted tail reads, if they are a brush file's own
        let own_images =
            self.active.filter(|_| !predicted.is_empty()).and_then(|a| self.full_grain_bind(device, &a.style));
        let ScreenPipelines {
            display,
            surround: surround_pipelines,
            overlay,
            selection: selection_pipelines,
            frost,
            ..
        } = &mut self.screen;
        let pipeline = display.entry(target.format).or_insert_with(|| {
            // over the surround, which shows through see-through parts (a hidden background)
            blended_pipeline(
                device,
                display_lut_layout,
                display_module,
                "display",
                ("vs_display", "fs_display"),
                target.format,
                Some(PREMULTIPLIED_OVER),
            )
        });
        let (w, h) = (target.size.0.max(1) as f32, target.size.1.max(1) as f32);
        let to_clip = Affine { a: 2.0 / w, b: 0.0, c: 0.0, d: -2.0 / h, e: -1.0, f: 1.0 };
        let unit_to_canvas = Affine { a: self.width as f32, b: 0.0, c: 0.0, d: self.height as f32, e: 0.0, f: 0.0 };
        let m = unit_to_canvas.then(target.to_screen).then(to_clip);
        let offset = uniforms.push(&DisplayUniform::new(
            [m.a, m.b, m.c, m.d],
            [m.e, m.f, if srgb { 1.0 } else { 0.0 }, if target.grey { 1.0 } else { 0.0 }],
            target.convert,
            target.lut.is_some(),
        ));
        let surround = target.surround.map(|c| if srgb { srgb_to_linear(c) } else { c });
        let clear =
            wgpu::Color { r: f64::from(surround[0]), g: f64::from(surround[1]), b: f64::from(surround[2]), a: 1.0 };
        let grid = target.grid.map(|g| {
            let pipeline = surround_pipelines.entry(target.format).or_insert_with(|| {
                sampled_pipeline(
                    device,
                    surround_pipeline_layout,
                    surround_module,
                    "surround",
                    "vs_surround",
                    "fs_surround",
                    target.format,
                )
            });
            let offset = uniforms.push(&SurroundUniform {
                v0: [g.spacing, g.origin[0], g.origin[1], g.width],
                v1: [g.color[0], g.color[1], g.color[2], if srgb { 1.0 } else { 0.0 }],
                v2: [target.surround[0], target.surround[1], target.surround[2], g.top],
            });
            (&*pipeline, offset)
        });
        let mut pass = render_pass(encoder, "display", target.view, wgpu::LoadOp::Clear(clear));
        if let Some((grid_pipeline, offset)) = grid {
            pass.set_pipeline(grid_pipeline);
            pass.set_bind_group(0, &self.uniform_bind, &[offset]);
            pass.draw(0..3, 0..1);
        }
        pass.set_pipeline(pipeline);
        // a little short of the mark still counts, for a zoom that lands just under it
        let crisp = target.to_screen.scale_factor() >= target.crisp_from * 0.999;
        pass.set_bind_group(0, if crisp { &self.crisp_bind } else { &self.display_bind }, &[]);
        pass.set_bind_group(1, &self.uniform_bind, &[offset]);
        pass.set_bind_group(2, target.lut.map_or(&self.identity_lut, |l| &l.bind), &[]);
        pass.draw(0..6, 0..1);

        // the selection: bands over what is not selected, or what is selected tinted or covered
        if let (Some(look), Some(selection), Some(inverse)) =
            (target.selection, &self.selection, unit_to_canvas.then(target.to_screen).inverse())
        {
            let multiply = matches!(look, SelectionLook::Tint(_));
            let pipeline = selection_pipelines.entry((target.format, multiply)).or_insert_with(|| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("selection"),
                    layout: Some(&*display_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: selection_module,
                        entry_point: Some("vs_selection"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: selection_module,
                        entry_point: Some("fs_selection"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: target.format,
                            blend: Some(if multiply { MULTIPLY } else { PREMULTIPLIED_OVER }),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            });
            let i = inverse;
            let srgb_flag = if srgb { 1.0 } else { 0.0 };
            let (m1_rest, v) = match look {
                SelectionLook::Stripes(s) => ([s.period.max(1.0), s.drift], [0.0, 0.0, 0.0, s.coverage]),
                SelectionLook::Tint(c) => ([1.0, 0.0], [c[0], c[1], c[2], 1.0]),
                SelectionLook::Fill(c) => ([1.0, 0.0], [c[0], c[1], c[2], 1.0]),
            };
            let mode = match look {
                SelectionLook::Stripes(_) => 0.0,
                SelectionLook::Tint(_) => 1.0,
                SelectionLook::Fill(_) => 2.0,
            };
            let offset = uniforms.push(&SelectionUniform {
                m0: [i.a, i.b, i.c, i.d],
                m1: [i.e, i.f, m1_rest[0], m1_rest[1]],
                v,
                mode: [mode, srgb_flag, 0.0, 0.0],
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &selection.bind, &[]);
            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
            pass.draw(0..3, 0..1);
        }

        // the predicted tail of a painting stroke, over the canvas
        let style = self.active.filter(|a| !a.pencil).map(|a| a.style).filter(|s| s.tool == StrokeTool::Paint);
        if let Some(style) = style.filter(|_| !predicted.is_empty()) {
            let overlay_pipeline = overlay.entry(target.format).or_insert_with(|| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("prediction"),
                    layout: Some(stamp_layout),
                    vertex: wgpu::VertexState {
                        module: overlay_module,
                        entry_point: Some("vs_overlay"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: size_of::<StampInstance>() as u64,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &STAMP_ATTRIBUTES,
                        })],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: overlay_module,
                        entry_point: Some("fs_overlay"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: target.format,
                            blend: Some(PREMULTIPLIED_OVER),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            });
            let c = target.to_screen.then(to_clip);
            let offset = uniforms.push(&OverlayUniform {
                m0: [c.a, c.b, c.c, c.d],
                m1: [c.e, c.f, target.to_screen.scale_factor(), style.hardness.clamp(0.0, 1.0)],
                color: {
                    let [r, g, b] = convert_colour(style.color, target.convert);
                    [r, g, b, style.opacity.clamp(0.0, 1.0)]
                },
                shape: style.shape.map_or(-1, |s| s as i32),
                srgb: u32::from(srgb),
                shape_lod_cap: style.shape_lod_cap,
                _pad: 0,
            });
            let size = size_of::<StampInstance>() as u64;
            pass.set_pipeline(overlay_pipeline);
            pass.set_bind_group(0, &self.uniform_bind, &[offset]);
            pass.set_bind_group(1, own_images.as_ref().unwrap_or(&self.brush_bind), &[]);
            pass.set_vertex_buffer(
                0,
                self.stamp_buffer.slice(u64::from(predicted.start) * size..u64::from(predicted.end) * size),
            );
            pass.draw(0..6, 0..predicted.end - predicted.start);
        }

        // the glass, over everything
        if let (Some((backdrop, k)), Some(small), Some(frost)) =
            (glass, self.backdrop.as_ref(), frost.get(&target.format))
        {
            draw_glass(&mut pass, frost, small, &self.uniform_bind, uniforms, backdrop, target.size, k);
        }
    }

    /// Frosted panels over a picture something else drew on `target` (a model): `scene` draws it
    /// again into the view it is given (of the target's format, cleared, the size given), small,
    /// and that is blurred and drawn into each region of the target.
    #[allow(clippy::too_many_arguments)]
    pub fn frost_over(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        surround: [f32; 3],
        backdrop: Backdrop<'_>,
        scene: SmallScene<'_>,
    ) {
        if (backdrop.regions.is_empty() && backdrop.windows.is_empty()) || backdrop.sigma <= 0.0 {
            return;
        }
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frost") });
        let mut uniforms = UniformData::default();
        let target = Target {
            view: target,
            format,
            size,
            to_screen: Affine::IDENTITY,
            surround,
            grid: None,
            selection: None,
            backdrop: None,
            convert: None,
            lut: None,
            crisp_from: f32::INFINITY,
            grey: false,
        };
        let k = self.frost_ready(&gpu.device, &mut encoder, &mut uniforms, &target, backdrop, Some(scene));
        if let (Some(small), Some(frost)) = (self.backdrop.as_ref(), self.screen.frost.get(&target.format)) {
            let mut pass = render_pass(&mut encoder, "frost", target.view, wgpu::LoadOp::Load);
            draw_glass(&mut pass, frost, small, &self.uniform_bind, &mut uniforms, backdrop, target.size, k);
        }
        self.write_uniforms(gpu, &uniforms);
        gpu.queue.submit([encoder.finish()]);
    }

    /// Frosted panels: the canvas and its surround (or what `other` draws) drawn again, small, and
    /// blurred, for [`draw_glass`]. Returns how many times smaller it is.
    fn frost_ready(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        target: &Target<'_>,
        backdrop: Backdrop<'_>,
        other: Option<SmallScene<'_>>,
    ) -> f32 {
        let k = (backdrop.sigma / BACKDROP_TEXELS).floor().clamp(1.0, 16.0);
        let size = ((target.size.0 as f32 / k).ceil() as u32, (target.size.1 as f32 / k).ceil() as u32);
        if self.backdrop.as_ref().is_none_or(|b| b.size != size || b.format != target.format) {
            self.backdrop =
                Some(BackdropGpu::new(device, &self.pipelines.sampled_layout, &self.sampler, size, target.format));
        }
        let Some(small) = self.backdrop.as_ref() else { return k };
        let srgb = target.format.is_srgb();
        let Pipelines { display_module, display_pipeline_layout, display_lut_layout, backdrop_module, .. } =
            &*self.pipelines;
        let ScreenPipelines { display, blur, frost, .. } = &mut self.screen;
        let display = display.entry(target.format).or_insert_with(|| {
            // over the surround, which shows through see-through parts (a hidden background)
            blended_pipeline(
                device,
                display_lut_layout,
                display_module,
                "display",
                ("vs_display", "fs_display"),
                target.format,
                Some(PREMULTIPLIED_OVER),
            )
        });
        let blur = blur.entry(target.format).or_insert_with(|| {
            sampled_pipeline(
                device,
                display_pipeline_layout,
                backdrop_module,
                "blur",
                "vs_full",
                "fs_blur",
                target.format,
            )
        });
        frost.entry(target.format).or_insert_with(|| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("frost"),
                layout: Some(&*display_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: backdrop_module,
                    entry_point: Some("vs_frost"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: backdrop_module,
                    entry_point: Some("fs_frost"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target.format,
                        blend: Some(PREMULTIPLIED_OVER),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        });

        // the scene, small: the surround and the canvas (its grid lines and the selection's look
        // would not show through the blur)
        let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
        let to_clip = Affine { a: 2.0 / w, b: 0.0, c: 0.0, d: -2.0 / h, e: -1.0, f: 1.0 };
        let unit_to_canvas = Affine { a: self.width as f32, b: 0.0, c: 0.0, d: self.height as f32, e: 0.0, f: 0.0 };
        let shrink = Affine { a: 1.0 / k, b: 0.0, c: 0.0, d: 1.0 / k, e: 0.0, f: 0.0 };
        let m = unit_to_canvas.then(target.to_screen).then(shrink).then(to_clip);
        let offset = uniforms.push(&DisplayUniform::new(
            [m.a, m.b, m.c, m.d],
            [m.e, m.f, if srgb { 1.0 } else { 0.0 }, if target.grey { 1.0 } else { 0.0 }],
            target.convert,
            target.lut.is_some(),
        ));
        let surround = target.surround.map(|c| if srgb { srgb_to_linear(c) } else { c });
        let clear =
            wgpu::Color { r: f64::from(surround[0]), g: f64::from(surround[1]), b: f64::from(surround[2]), a: 1.0 };
        if let Some(draw) = other {
            draw(encoder, &small.views[0], size);
        } else {
            let mut pass = render_pass(encoder, "backdrop scene", &small.views[0], wgpu::LoadOp::Clear(clear));
            pass.set_pipeline(display);
            pass.set_bind_group(0, &self.display_bind, &[]);
            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
            pass.set_bind_group(2, target.lut.map_or(&self.identity_lut, |l| &l.bind), &[]);
            pass.draw(0..6, 0..1);
        }
        // blurred across, then down
        let sigma = backdrop.sigma / k;
        let radius = (sigma * 3.0).ceil().min(64.0);
        for (from, to, step) in
            [(&small.binds[0], &small.views[1], [1.0 / w, 0.0]), (&small.binds[1], &small.views[2], [0.0, 1.0 / h])]
        {
            let offset =
                uniforms.push(&BackdropUniform { v0: [step[0], step[1], sigma, radius], v1: [0.0; 4], v2: [0.0; 4] });
            let mut pass = render_pass(encoder, "backdrop blur", to, wgpu::LoadOp::Clear(clear));
            pass.set_pipeline(blur);
            pass.set_bind_group(0, from, &[]);
            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
            pass.draw(0..3, 0..1);
        }
        k
    }

    /// A layer scaled to fit `max_side` pixels (premultiplied). Runs what is queued first, then
    /// waits for the GPU.
    pub fn layer_thumbnail(
        &mut self,
        gpu: &Gpu,
        doc: &mut Document,
        id: LayerId,
        max_side: u32,
    ) -> Result<Thumbnail, GpuError> {
        self.render(gpu, doc, None);
        let mask = doc.layer(id).is_some_and(|l| l.kind == LayerKind::Mask);
        if let Some(p) = self.parked_pictures(id, max_side).filter(|_| !mask) {
            return Ok(shrink(p.drawn.as_ref().unwrap_or(&p.whole), max_side));
        }
        self.restore_all(gpu, [id], false);
        let full = self.full();
        // a mask is white where nothing is painted on it
        let bounds = if doc.layer(id).is_some_and(|l| l.kind == LayerKind::Mask) { ink_bounds } else { alpha_bounds };
        // where its pixels are: roughly, on a copy small enough to read back quickly but fine
        // enough (8 pixels to one) not to miss a dot, then exactly, on the region drawn
        let coarse = self.sample_layer(gpu, id, full, (self.width.max(self.height) / 8).clamp(64, 2048))?;
        let Some([x0, y0, x1, y1]) = bounds(&coarse) else { return self.sample_layer(gpu, id, full, max_side) };
        let (sx, sy) = (self.width as f32 / coarse.width as f32, self.height as f32 / coarse.height as f32);
        let rect = IRect {
            x0: ((x0 as f32 - 1.0) * sx).floor() as i32,
            y0: ((y0 as f32 - 1.0) * sy).floor() as i32,
            x1: ((x1 as f32 + 1.0) * sx).ceil() as i32,
            y1: ((y1 as f32 + 1.0) * sy).ceil() as i32,
        }
        .intersect(full);
        let fine = self.sample_layer(gpu, id, rect, max_side)?;
        Ok(match bounds(&fine) {
            Some(b) => crop(&fine, b),
            None => fine,
        })
    }

    /// `rect` of a layer box-filtered to fit `max_side` pixels (never enlarged), read back.
    fn sample_layer(&self, gpu: &Gpu, id: LayerId, rect: IRect, max_side: u32) -> Result<Thumbnail, GpuError> {
        let layer = self.layers.get(&id).ok_or_else(|| GpuError::Readback(format!("no layer {id:?}")))?;
        self.sample_view(gpu, &layer.view, rect, max_side)
    }

    /// The whole picture as composited, scaled to fit `max_side` pixels (never enlarged).
    /// Each paint layer's coverage (alpha, 0 to 255) at canvas pixel (`x`, `y`), in the
    /// document's order; none for a point off the canvas.
    pub fn alphas_at(&mut self, gpu: &Gpu, doc: &mut Document, x: u32, y: u32) -> Result<Vec<(LayerId, u8)>, GpuError> {
        if x >= self.width || y >= self.height {
            return Ok(Vec::new());
        }
        self.render(gpu, doc, None);
        let paint: Vec<LayerId> = doc.layers().iter().filter(|l| l.kind == LayerKind::Paint).map(|l| l.id).collect();
        let ids: Vec<LayerId> = paint.iter().copied().filter(|id| self.layers.contains_key(id)).collect();
        // a parked layer's from its small picture
        let parked: HashMap<LayerId, u8> = paint
            .iter()
            .filter(|id| !self.layers.contains_key(*id))
            .filter_map(|id| {
                let w = &self.pictures.get(id)?.whole;
                let px = (u64::from(x) * u64::from(w.width) / u64::from(self.width.max(1))) as usize;
                let py = (u64::from(y) * u64::from(w.height) / u64::from(self.height.max(1))) as usize;
                Some((*id, *w.rgba.get((py * w.width as usize + px) * 4 + 3)?))
            })
            .collect();
        if ids.is_empty() {
            return Ok(paint.iter().filter_map(|id| Some((*id, *parked.get(id)?))).collect());
        }
        // one pixel of each layer, each in its own 256-byte row of a buffer
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("layer pixels"),
            size: 256 * ids.len() as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder =
            gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("layer pixels") });
        for (i, id) in ids.iter().enumerate() {
            let Some(layer) = self.layers.get(id) else { continue };
            encoder.copy_texture_to_buffer(
                copy_at(&layer.texture, i32::try_from(x).unwrap_or(0), i32::try_from(y).unwrap_or(0)),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 256 * i as u64,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(1),
                    },
                },
                extent(1, 1),
            );
        }
        gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| GpuError::Readback(e.to_string()))?;
        rx.recv().map_err(|e| GpuError::Readback(e.to_string()))?.map_err(|e| GpuError::Readback(e.to_string()))?;
        let mapped = buffer.slice(..).get_mapped_range().map_err(|e| GpuError::Readback(e.to_string()))?;
        let read: HashMap<LayerId, u8> =
            ids.iter().enumerate().map(|(i, id)| (*id, mapped.get(256 * i + 3).copied().unwrap_or(0))).collect();
        drop(mapped);
        buffer.unmap();
        // in the document's order, parked or not
        Ok(paint.iter().filter_map(|id| Some((*id, *read.get(id).or_else(|| parked.get(id))?))).collect())
    }

    /// Layer `id` over the whole canvas (not cropped to what is drawn) fitted to `max_side`, as
    /// Animation Assist's timeline shows a frame.
    pub fn layer_picture(
        &mut self,
        gpu: &Gpu,
        doc: &mut Document,
        id: LayerId,
        max_side: u32,
    ) -> Result<Thumbnail, GpuError> {
        self.render(gpu, doc, None);
        if let Some(p) = self.parked_pictures(id, max_side) {
            return Ok(shrink(&p.whole, max_side));
        }
        self.restore_all(gpu, [id], false);
        let full = self.full();
        self.sample_layer(gpu, id, full, max_side)
    }

    /// The picture as it shows (while the caches are remade, as it was), scaled to fit
    /// `max_side` pixels: quick enough to ask for while it is drawn on.
    pub fn composite_thumbnail(&mut self, gpu: &Gpu, doc: &mut Document, max_side: u32) -> Result<Thumbnail, GpuError> {
        // while the caches are remade a render leaves the picture as it is: none, so this never
        // does that work (on the main thread, as the artwork closes)
        if !self.busy() {
            self.render(gpu, doc, None);
        }
        self.sample_view(gpu, &self.composite_view, self.full(), max_side)
    }

    /// [`Canvas::composite_thumbnail`] without the layers `left_out` (a time-lapse frame leaves
    /// private layers out), as [`Canvas::read_composite_without`] makes the picture; the canvas's
    /// own composite is made again after, whole (a frame is taken after every change: left to
    /// later renders, a large canvas would show without those layers for a moment each time).
    pub fn composite_thumbnail_without(
        &mut self,
        gpu: &Gpu,
        doc: &mut Document,
        max_side: u32,
        left_out: &[LayerId],
    ) -> Result<Thumbnail, GpuError> {
        if left_out.is_empty() {
            return self.composite_thumbnail(gpu, doc, max_side);
        }
        // the work queued done first, as it may change `doc`
        self.render(gpu, doc, None);
        let mut shown = doc.clone();
        for &id in left_out {
            shown.set_visible(id, false);
        }
        self.invalidate();
        self.render_finished(gpu, &mut shown);
        let thumbnail = self.sample_view(gpu, &self.composite_view, self.full(), max_side);
        self.invalidate();
        self.render_finished(gpu, doc);
        thumbnail
    }

    /// `rect` of a texture box-filtered to fit `max_side` pixels (never enlarged), read back.
    fn sample_view(
        &self,
        gpu: &Gpu,
        source: &wgpu::TextureView,
        rect: IRect,
        max_side: u32,
    ) -> Result<Thumbnail, GpuError> {
        let side = max_side.clamp(1, 2048) as f32;
        let (rw, rh) = (rect.width().max(1) as f32, rect.height().max(1) as f32);
        let scale = (side / rw).min(side / rh).min(1.0);
        let (w, h) = (((rw * scale).round() as u32).max(1), ((rh * scale).round() as u32).max(1));
        let small = texture(&gpu.device, "thumbnail", w, h, LAYER_FORMAT);
        let view = small.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = sampled_bind(&gpu.device, &self.pipelines.sampled_layout, source, &self.sampler);
        let mut data = UniformData::default();
        // the target size and the source pixels each target pixel covers; the source rectangle
        let (cw, ch) = (self.width.max(1) as f32, self.height.max(1) as f32);
        let offset = data.push(&DisplayUniform::new(
            [w as f32, h as f32, rw / w as f32, rh / h as f32],
            [rect.x0 as f32 / cw, rect.y0 as f32 / ch, rw / cw, rh / ch],
            None,
            false,
        ));
        let mut encoder =
            gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("thumbnail") });
        {
            let mut pass = render_pass(&mut encoder, "thumbnail", &view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_pipeline(&self.pipelines.thumb);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
            pass.draw(0..3, 0..1);
        }
        self.write_uniforms(gpu, &data);
        gpu.queue.submit([encoder.finish()]);
        let rgba = read_texture(gpu, &small, w, h)?;
        Ok(Thumbnail { width: w, height: h, rgba })
    }

    /// Layer `id` of canvas `from` (on the same device) as this canvas's, its pixels moved by
    /// whole pixels: this canvas's (x, y) is `from`'s (x + `offset.0`, y + `offset.1`), clear
    /// where that is off `from`. Copied on the GPU; false (nothing done) if `from` has no texture
    /// for it (a parked layer). Not an undo step.
    pub fn copy_layer_from(&mut self, gpu: &Gpu, from: &Canvas, id: LayerId, offset: (i64, i64)) -> bool {
        let Some(source) = from.layers.get(&id) else { return false };
        if !self.layers.contains_key(&id) {
            let texture = texture(&gpu.device, "layer", self.width, self.height, LAYER_FORMAT);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.layers.insert(id, LayerGpu { texture, view });
        }
        self.touched(id);
        self.let_go_caches(false);
        self.parked.remove(&id);
        self.pictures.remove(&id);
        let Some(layer) = self.layers.get(&id) else { return false };
        // the part of this canvas that falls on `from`
        let clamp = |v: i64, hi: u32| v.clamp(0, i64::from(hi));
        let (x0, x1) = (clamp(-offset.0, self.width), clamp(i64::from(from.width) - offset.0, self.width));
        let (y0, y1) = (clamp(-offset.1, self.height), clamp(i64::from(from.height) - offset.1, self.height));
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("crop") });
        // what was there first cleared (a layer made just now is clear already)
        drop(render_pass(&mut encoder, "crop clear", &layer.view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)));
        if x1 > x0 && y1 > y0 {
            let at = |v: i64| i32::try_from(v).unwrap_or(0);
            encoder.copy_texture_to_texture(
                copy_at(&source.texture, at(x0 + offset.0), at(y0 + offset.1)),
                copy_at(&layer.texture, at(x0), at(y0)),
                extent(u32::try_from(x1 - x0).unwrap_or(0), u32::try_from(y1 - y0).unwrap_or(0)),
            );
        }
        gpu.queue.submit([encoder.finish()]);
        self.invalidate();
        true
    }

    /// Layer `id` of canvas `from` (on the same device) seen through a crop as this canvas's:
    /// each pixel's middle mapped onto `from` by `to_old` (a, b, c, d, e, f: x' = a x + c y + e,
    /// y' = b x + d y + f), bilinear, clear past its edges, as the engine's crop resamples on the
    /// CPU. Drawn on the GPU; false (nothing done) if `from` has no texture for it (a parked
    /// layer). Not an undo step.
    pub fn resample_layer_from(&mut self, gpu: &Gpu, from: &Canvas, id: LayerId, to_old: [f32; 6]) -> bool {
        let Some(source) = from.layers.get(&id) else { return false };
        if !self.layers.contains_key(&id) {
            let texture = texture(&gpu.device, "layer", self.width, self.height, LAYER_FORMAT);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.layers.insert(id, LayerGpu { texture, view });
        }
        self.touched(id);
        self.let_go_caches(false);
        self.parked.remove(&id);
        self.pictures.remove(&id);
        let Some(layer) = self.layers.get(&id) else { return false };
        let [a, b, c, d, e, f] = to_old;
        let map = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("crop"),
            contents: bytemuck::cast_slice(&[a, b, c, d, e, f, 0.0, 0.0]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("crop"),
            layout: &self.pipelines.crop_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source.view) },
                wgpu::BindGroupEntry { binding: 1, resource: map.as_entire_binding() },
            ],
        });
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("crop") });
        let mut pass = render_pass(&mut encoder, "crop", &layer.view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
        pass.set_pipeline(&self.pipelines.crop);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        gpu.queue.submit([encoder.finish()]);
        self.invalidate();
        true
    }

    /// Replaces a layer's pixels (premultiplied RGBA8, the canvas's size, rows top to bottom), for
    /// opening a saved artwork. Not an undo step.
    pub fn upload_layer(&mut self, gpu: &Gpu, id: LayerId, rgba: &[u8]) -> Result<(), GpuError> {
        self.upload_layer_part(gpu, id, self.full(), rgba)
    }

    /// [`Canvas::upload_layer`] for the pixels inside `rect` only (`part`, its rows): for a layer
    /// with nothing on it elsewhere (a new one). Not an undo step.
    pub fn upload_layer_part(&mut self, gpu: &Gpu, id: LayerId, rect: IRect, part: &[u8]) -> Result<(), GpuError> {
        let expected = rect.width() as usize * rect.height() as usize * 4;
        if rect.is_empty() || rect.intersect(self.full()) != rect || part.len() != expected {
            return Err(GpuError::Readback(format!("layer {id:?}: {} bytes, expected {expected}", part.len())));
        }
        self.put_layer_pixels(gpu, id, Some(rect), part, 0, rect.width() * 4);
        Ok(())
    }

    /// [`Canvas::upload_layer`] for a layer as just made (clear): of `rgba`, the canvas's size,
    /// only `painted` is sent (where it isn't clear; none: nothing is). Not an undo step.
    pub fn upload_layer_painted(
        &mut self,
        gpu: &Gpu,
        id: LayerId,
        rgba: &[u8],
        painted: Option<IRect>,
    ) -> Result<(), GpuError> {
        let full = self.full();
        let expected = full.width() as usize * full.height() as usize * 4;
        if rgba.len() != expected {
            return Err(GpuError::Readback(format!("layer {id:?}: {} bytes, expected {expected}", rgba.len())));
        }
        let rect = painted.map(|r| r.intersect(full)).filter(|r| !r.is_empty());
        // the box's first pixel, read with the whole picture's rows
        let offset = rect
            .map_or(0, |r| (u64::from(r.y0.max(0) as u32) * u64::from(self.width) + u64::from(r.x0.max(0) as u32)) * 4);
        self.put_layer_pixels(gpu, id, rect, rgba, offset, self.width * 4);
        Ok(())
    }

    /// Writes `data` (from `offset`, rows `row` bytes apart) into layer `id` within `rect` (none:
    /// nothing written, the layer made if it wasn't), the layer made if it wasn't, its compressed
    /// copy and the caches let go.
    fn put_layer_pixels(&mut self, gpu: &Gpu, id: LayerId, rect: Option<IRect>, data: &[u8], offset: u64, row: u32) {
        if !self.layers.contains_key(&id) {
            let texture = texture(&gpu.device, "layer", self.width, self.height, LAYER_FORMAT);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.layers.insert(id, LayerGpu { texture, view });
        }
        self.touched(id);
        self.let_go_caches(false);
        // its compressed copy no longer what it holds
        self.parked.remove(&id);
        self.pictures.remove(&id);
        self.invalidate();
        let (Some(layer), Some(rect)) = (self.layers.get(&id), rect) else { return };
        // sent now, so many layers in a row (an artwork opening, a moving picture pasted) go up
        // through the same buffer
        self.upload_now(gpu, &layer.texture, rect, data, offset as usize, row as usize);
    }

    /// Copies `rect` of `texture` from `data` (from `offset`, rows `row` bytes apart) in
    /// `encoder`, through an upload buffer: returns it, to give back once `encoder` is sent.
    #[allow(clippy::too_many_arguments)]
    fn upload(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        rect: IRect,
        data: &[u8],
        offset: usize,
        row: usize,
    ) -> Option<wgpu::Buffer> {
        let (w, h) = (rect.width(), rect.height());
        let last = offset + (h as usize).saturating_sub(1) * row + w as usize * 4;
        if w == 0 || h == 0 || data.len() < last {
            log::error!("an upload of {w} x {h} given {} bytes", data.len());
            return None;
        }
        // copies from a buffer take rows 256 bytes apart
        let stride = (w * 4).div_ceil(256) * 256;
        let size = u64::from(stride) * u64::from(h);
        let buffer = self.uploads.take(device, size);
        {
            let Ok(mut view) = buffer.slice(..size).get_mapped_range_mut() else {
                log::error!("an upload buffer wasn't mapped");
                return None;
            };
            let (wide, stride) = (w as usize * 4, stride as usize);
            for y in 0..h as usize {
                let from = offset + y * row;
                if let Some(src) = data.get(from..from + wide) {
                    view.slice(y * stride..y * stride + wide).copy_from_slice(src);
                }
            }
        }
        buffer.unmap();
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: Some(h) },
            },
            copy_at(texture, rect.x0, rect.y0),
            extent(w, h),
        );
        Some(buffer)
    }

    /// [`Canvas::upload`] sent at once, ahead of whatever is recorded but not yet sent.
    fn upload_now(&self, gpu: &Gpu, texture: &wgpu::Texture, rect: IRect, data: &[u8], offset: usize, row: usize) {
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("upload") });
        let used = self.upload(&gpu.device, &mut encoder, texture, rect, data, offset, row);
        gpu.queue.submit([encoder.finish()]);
        if let Some(buffer) = used {
            self.uploads.give_back(buffer);
        }
    }

    /// A colour table for [`Target::lut`] from `lut` (its first three outputs, 0..1 encoded).
    pub fn display_lut(&self, gpu: &Gpu, lut: &procraft_engine::cms::Lut) -> DisplayLut {
        let rgba: Vec<u8> = lut
            .values
            .chunks(lut.outputs.max(1))
            .flat_map(|v| {
                let c = |k: usize| (v.get(k).copied().unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8;
                [c(0), c(1), c(2), 255]
            })
            .collect();
        DisplayLut { bind: lut_bind(gpu, &self.pipelines.lut_layout, lut.grid as u32, &rgba) }
    }

    /// The composite's pixels (premultiplied RGBA8, rows top to bottom) as last rendered: after
    /// [`Canvas::render_finished`], the picture as it is. Waits for the GPU.
    pub fn read_composite(&self, gpu: &Gpu) -> Result<Vec<u8>, GpuError> {
        read_texture(gpu, &self.composite, self.width, self.height)
    }

    /// The composite's pixel at canvas pixel (`x`, `y`) as last rendered (premultiplied RGBA8);
    /// none off the canvas. Waits for the GPU.
    pub fn read_composite_pixel(&self, gpu: &Gpu, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let (x, y) = (i32::try_from(x).ok()?, i32::try_from(y).ok()?);
        let px = start_reading_part(gpu, &self.composite, IRect::new(x, y, x + 1, y + 1)).pixels().ok()?;
        Some([*px.first()?, *px.get(1)?, *px.get(2)?, *px.get(3)?])
    }

    /// The composite's pixels as they would be without the layers `left_out` (the gallery's
    /// thumbnail leaves private layers out); the canvas's own composite is made again after.
    pub fn read_composite_without(
        &mut self,
        gpu: &Gpu,
        doc: &mut Document,
        left_out: &[LayerId],
    ) -> Result<Vec<u8>, GpuError> {
        self.render_finished(gpu, doc);
        if left_out.is_empty() {
            return self.read_composite(gpu);
        }
        let mut shown = doc.clone();
        for &id in left_out {
            shown.set_visible(id, false);
        }
        self.invalidate();
        self.render_finished(gpu, &mut shown);
        let read = self.read_composite(gpu);
        self.invalidate();
        self.render(gpu, doc, None);
        read
    }

    /// Starts reading a layer's pixels back (premultiplied RGBA8): see [`Readback`].
    pub fn start_reading_layer(&self, gpu: &Gpu, id: LayerId) -> Result<Readback, GpuError> {
        if self.parked.contains_key(&id) && !self.layers.contains_key(&id) {
            return self.read_layer(gpu, id).map(Readback::Ready);
        }
        let layer = self.layers.get(&id).ok_or_else(|| GpuError::Readback(format!("no layer {id:?}")))?;
        Ok(start_reading(gpu, &layer.texture, self.width, self.height))
    }

    /// Starts reading the composite back as it would be without the layers `left_out`, as
    /// [`Canvas::read_composite_without`] does: see [`Readback`].
    pub fn start_reading_composite_without(&mut self, gpu: &Gpu, doc: &mut Document, left_out: &[LayerId]) -> Readback {
        self.render_finished(gpu, doc);
        if left_out.is_empty() {
            return start_reading(gpu, &self.composite, self.width, self.height);
        }
        let mut shown = doc.clone();
        for &id in left_out {
            shown.set_visible(id, false);
        }
        self.invalidate();
        self.render_finished(gpu, &mut shown);
        // copied before the composite is made again below
        let read = start_reading(gpu, &self.composite, self.width, self.height);
        self.invalidate();
        self.render(gpu, doc, None);
        read
    }

    /// Where each of `layers` is painted: the box around its pixels that aren't clear, none for one
    /// with nothing on it (or not on the canvas). Found on the GPU, which sends back only a few
    /// bytes a row and column; a parked layer's is known already. Waits for the GPU.
    pub fn painted_boxes(&self, gpu: &Gpu, layers: &[LayerId]) -> Vec<Option<IRect>> {
        self.painted_boxes_or_why(gpu, layers).into_iter().map(|b| b.ok().flatten()).collect()
    }

    /// [`Canvas::painted_boxes`], an error where the GPU didn't say (rather than nothing painted).
    pub fn painted_boxes_or_why(&self, gpu: &Gpu, layers: &[LayerId]) -> Vec<Result<Option<IRect>, GpuError>> {
        let sources = layers
            .iter()
            .map(|id| match self.layers.get(id) {
                Some(layer) => Ok(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("bounds"),
                    layout: &self.pipelines.bounds_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&layer.view),
                    }],
                })),
                None => Err(self.parked.get(id).and_then(|p| p.rect)),
            })
            .collect();
        self.boxes(gpu, sources, [&self.pipelines.bounds_rows, &self.pipelines.bounds_columns])
    }

    /// Where the layers being transformed or adjusted differ from their copies from when it
    /// began, all drawn so far: the box around the pixels changed, inner none if none did; none
    /// if it can't be told (nothing open, or the GPU didn't say). Waits for the GPU.
    pub fn changed_box(&self, gpu: &Gpu) -> Option<Option<IRect>> {
        let open: Vec<&Transforming> = self.transforming.iter().chain(&self.transform_more).collect();
        if open.is_empty() {
            return None;
        }
        let mut sources = Vec::new();
        for t in open {
            let layer = self.layers.get(&t.layer)?;
            let before = t.before.create_view(&wgpu::TextureViewDescriptor::default());
            sources.push(Ok(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("changed"),
                layout: &self.pipelines.changed_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&layer.view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&before) },
                ],
            })));
        }
        let boxes = self.boxes(gpu, sources, [&self.pipelines.changed_rows, &self.pipelines.changed_columns]);
        let boxes: Vec<Option<IRect>> = boxes.into_iter().collect::<Result<_, _>>().ok()?;
        Some(boxes.into_iter().flatten().reduce(IRect::union))
    }

    /// The boxes the GPU's passes (`pipelines`: across rows, down columns) find for each of
    /// `sources` (a bind group for them, or a box already known), all drawn and read back
    /// together: none where nothing is marked, an error where the GPU didn't say.
    fn boxes(
        &self,
        gpu: &Gpu,
        sources: Vec<Result<wgpu::BindGroup, Option<IRect>>>,
        [rows_pass, columns_pass]: [&wgpu::RenderPipeline; 2],
    ) -> Vec<Result<Option<IRect>, GpuError>> {
        let (w, h) = (self.width, self.height);
        let (across, down) = (w.div_ceil(BOUNDS_STRETCH).max(1), h.div_ceil(BOUNDS_STRETCH).max(1));
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("bounds") });
        // each source's rows (h x across) and columns (w x down)
        let lines: Vec<_> = sources
            .into_iter()
            .map(|source| {
                source.map(|bind| {
                    [(rows_pass, (h, across)), (columns_pass, (w, down))].map(|(pipeline, size)| {
                        let target = texture(&gpu.device, "bounds", size.0, size.1, LAYER_FORMAT);
                        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
                        let mut pass =
                            render_pass(&mut encoder, "bounds", &view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, &bind, &[]);
                        pass.draw(0..3, 0..1);
                        drop(pass);
                        (target, size)
                    })
                })
            })
            .collect();
        gpu.queue.submit([encoder.finish()]);
        // all copied back together, then each read
        let reads: Vec<_> =
            lines.into_iter().map(|l| l.map(|t| t.map(|(t, (a, b))| start_reading(gpu, &t, a, b)))).collect();
        // a line marked if any of its stretches is
        let span = |pixels: &[u8], n: u32| -> Option<(i32, i32)> {
            let n = n as usize;
            let marked =
                |i: usize| pixels.chunks_exact(4 * n).any(|stretch| stretch.get(4 * i).is_some_and(|&v| v > 0));
            let first = (0..n).find(|&i| marked(i))?;
            let last = (0..n).rev().find(|&i| marked(i))?;
            Some((i32::try_from(first).ok()?, i32::try_from(last + 1).ok()?))
        };
        reads
            .into_iter()
            .map(|read| match read {
                Err(known) => Ok(known),
                Ok([rows, columns]) => {
                    let (rows, columns) = (rows.pixels()?, columns.pixels()?);
                    let spans = span(&rows, h).zip(span(&columns, w));
                    Ok(spans.map(|((y0, y1), (x0, x1))| IRect::new(x0, y0, x1, y1)))
                }
            })
            .collect()
    }

    /// A layer's pixels inside `rect` (on the canvas), rows top to bottom; none if it is off the
    /// canvas. Waits for the GPU.
    pub fn read_layer_part(&self, gpu: &Gpu, id: LayerId, rect: IRect) -> Result<Vec<u8>, GpuError> {
        let rect = rect.intersect(self.full());
        if rect.is_empty() {
            return Ok(Vec::new());
        }
        if rect == self.full() || (self.parked.contains_key(&id) && !self.layers.contains_key(&id)) {
            return self
                .read_layer(gpu, id)
                .map(|p| if rect == self.full() { p } else { cropped(&p, self.width, rect) });
        }
        let layer = self.layers.get(&id).ok_or_else(|| GpuError::Readback(format!("no layer {id:?}")))?;
        start_reading_part(gpu, &layer.texture, rect).pixels()
    }

    /// A layer's pixels (premultiplied RGBA8). Waits for the GPU.
    pub fn read_layer(&self, gpu: &Gpu, id: LayerId) -> Result<Vec<u8>, GpuError> {
        if let Some(parked) = self.parked.get(&id).filter(|_| !self.layers.contains_key(&id)) {
            return parked
                .pixels(self.width, self.height)
                .ok_or_else(|| GpuError::Readback(format!("layer {id:?} parked damaged")));
        }
        let layer = self.layers.get(&id).ok_or_else(|| GpuError::Readback(format!("no layer {id:?}")))?;
        read_texture(gpu, &layer.texture, self.width, self.height)
    }

    /// A bind group of four textures (layer, stroke, clip base, backdrop) plus the grains.
    fn quad(&self, device: &wgpu::Device, views: [&wgpu::TextureView; 4]) -> wgpu::BindGroup {
        let [a, b, c, d] = views;
        self.quad_masked(device, [a, b, c, d], &self.stroke_view, &self.stroke_view)
    }

    /// As [`Self::quad`], with the layer's mask and the mask of the base it is clipped to.
    fn quad_masked(
        &self,
        device: &wgpu::Device,
        views: [&wgpu::TextureView; 4],
        mask: &wgpu::TextureView,
        clip_mask: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let [a, b, c, d] = views;
        quad_bind(
            device,
            &self.pipelines.quad_layout,
            [a, b, c, d, mask, self.second_view(device), clip_mask],
            self.grain_view(),
            &self.grain_sampler,
        )
    }

    /// The open stroke's second's texture while it has one (else any, unread).
    fn second_view(&self, device: &wgpu::Device) -> &wgpu::TextureView {
        if self.active.is_some_and(|a| a.second.is_some()) {
            self.second_stroke.get(device).1
        } else {
            &self.stroke_view
        }
    }

    /// Layer textures held (the document's and those the history still needs).
    pub fn layer_texture_count(&self) -> usize {
        self.layers.len()
    }

    /// Every texture the canvas holds, each once: its layers, stroke, composite and what that is
    /// made with, its brush images, the undo history's pixels and what any edit under way keeps.
    pub fn textures(&self) -> Vec<wgpu::Texture> {
        let mut all: Vec<&wgpu::Texture> = vec![&self.stroke, &self.composite, &self.shapes, &self.grains, &self.table];
        all.extend(self.layers.values().map(|l| &l.texture));
        all.extend(self.tip_backup.as_ref());
        all.extend(self.composite_second.made());
        all.extend(self.scratch.made());
        all.extend(self.second_stroke.made());
        all.extend(self.groups.iter().flat_map(|(pair, _)| pair));
        all.push(self.everything.texture());
        all.extend(self.pencil.as_ref().map(|p| p.view.texture()));
        all.extend(self.liquify.as_ref().map(|l| l.view.texture()));
        all.extend(self.selection.as_ref().map(|s| s.view.texture()));
        all.extend(self.held.as_ref().map(|h| &h.texture));
        all.extend(self.smudge.as_ref().map(|s| &s.before));
        for caches in self.caches.iter().chain(self.remake.as_ref().map(|r| &r.caches)) {
            all.extend(caches.below.iter().chain(&caches.above));
        }
        all.extend(self.undo.iter().chain(&self.redo).flat_map(|s| s.pixels.iter().map(|p| &p.texture)));
        all.extend(self.transforming.iter().chain(&self.transform_more).map(|t| &t.before));
        all.extend(self.backdrop.iter().flat_map(|b| b.views.iter().map(wgpu::TextureView::texture)));
        let mut out: Vec<wgpu::Texture> = Vec::with_capacity(all.len());
        for t in all {
            if !out.contains(t) {
                out.push(t.clone());
            }
        }
        out
    }

    /// What the canvas's textures take, bytes (see [`Canvas::textures`]).
    pub fn texture_bytes(&self) -> u64 {
        textures_bytes(&self.textures())
    }
}

/// What `textures` take on the GPU, bytes: every level and layer of each, as its format packs
/// them (before the driver's own rounding up).
pub fn textures_bytes(textures: &[wgpu::Texture]) -> u64 {
    textures
        .iter()
        .map(|t| {
            let (bw, bh) = t.format().block_dimensions();
            let block = u64::from(t.format().block_copy_size(None).unwrap_or(4));
            let layers = u64::from(t.depth_or_array_layers()) * u64::from(t.sample_count());
            (0..t.mip_level_count())
                .map(|level| {
                    let (w, h) = ((t.width() >> level).max(1), (t.height() >> level).max(1));
                    u64::from(w.div_ceil(bw)) * u64::from(h.div_ceil(bh)) * block * layers
                })
                .sum::<u64>()
        })
        .sum()
}

impl Pipelines {
    pub(crate) fn new(device: &wgpu::Device, liquify_format: wgpu::TextureFormat) -> Pipelines {
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let array_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT.union(wgpu::ShaderStages::COMPUTE),
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT.union(wgpu::ShaderStages::COMPUTE),
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let quad_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("layer, stroke, clip, backdrop, grains"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                tex_entry(2),
                tex_entry(3),
                array_entry(4),
                sampler_entry(5),
                tex_entry(6),
                tex_entry(7),
                tex_entry(8),
            ],
        });
        let brush_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shapes, grains and the selection"),
            entries: &[
                array_entry(0),
                array_entry(1),
                sampler_entry(2),
                sampler_entry(3),
                // the selection, filtered where the stamp samples it
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let sampled_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sampled"),
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
            ],
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(UNIFORM_SIZE),
                },
                count: None,
            }],
        });
        let module = |label, src: &str| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            })
        };
        let stamp_module =
            module("stamp", concat!(include_str!("shaders/blend.wgsl"), include_str!("shaders/stamp.wgsl")));
        let composite_module =
            module("composite", concat!(include_str!("shaders/blend.wgsl"), include_str!("shaders/composite.wgsl")));
        let display_module = module("display", include_str!("shaders/display.wgsl"));
        let surround_module = module("surround", include_str!("shaders/surround.wgsl"));
        let backdrop_module = module("backdrop", include_str!("shaders/backdrop.wgsl"));
        let transform_module = module("transform", include_str!("shaders/transform.wgsl"));
        let selection_module = module("selection", include_str!("shaders/selection.wgsl"));
        let layout = |label, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                immediate_size: 0,
            })
        };
        let stamp_layout = layout("stamp", &[Some(&uniform_layout), Some(&brush_layout)]);
        let composite_layout = layout("composite", &[Some(&quad_layout), Some(&uniform_layout)]);
        let display_pipeline_layout = layout("display", &[Some(&sampled_layout), Some(&uniform_layout)]);
        let lut_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("colour table"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
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
            ],
        });
        let display_lut_layout =
            layout("display with a table", &[Some(&sampled_layout), Some(&uniform_layout), Some(&lut_layout)]);
        // a transform's copy, its sampler and the selection
        let transform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("transform source"),
            entries: &[
                // filtered where an adjustment samples it
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
                sampler_entry(1),
                tex_entry(2),
            ],
        });
        let transform_pipeline_layout = layout("transform", &[Some(&transform_layout), Some(&uniform_layout)]);
        let before_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("adjustment copy"),
            entries: &[tex_entry(0), tex_entry(1)],
        });
        let filter_pipeline_layout =
            layout("adjustment", &[Some(&transform_layout), Some(&uniform_layout), Some(&before_layout)]);
        let bounds_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bounds"),
            entries: &[tex_entry(0)],
        });
        let bounds_module = module("bounds", include_str!("shaders/bounds.wgsl"));
        let changed_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("changed"),
            entries: &[tex_entry(0), tex_entry(1)],
        });
        let bounds_pipeline = |group: &wgpu::BindGroupLayout, fs| {
            let pipeline_layout = layout("bounds", &[Some(group)]);
            blended_pipeline(device, &pipeline_layout, &bounds_module, fs, ("vs_full", fs), LAYER_FORMAT, None)
        };
        let (bounds_rows, bounds_columns) =
            (bounds_pipeline(&bounds_layout, "fs_rows"), bounds_pipeline(&bounds_layout, "fs_columns"));
        let (changed_rows, changed_columns) = (
            bounds_pipeline(&changed_layout, "fs_changed_rows"),
            bounds_pipeline(&changed_layout, "fs_changed_columns"),
        );
        let flip_module = module("flip", include_str!("shaders/flip.wgsl"));
        let flip_layout = layout("flip", &[Some(&bounds_layout)]);
        let flip_pipeline =
            |fs| blended_pipeline(device, &flip_layout, &flip_module, fs, ("vs_full", fs), LAYER_FORMAT, None);
        let (flip_across, flip_down) = (flip_pipeline("fs_across"), flip_pipeline("fs_down"));
        let invert = flip_pipeline("fs_invert");
        let crop_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("crop"),
            entries: &[
                tex_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
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
        let crop_module = module("crop", include_str!("shaders/crop.wgsl"));
        let crop = blended_pipeline(
            device,
            &layout("crop", &[Some(&crop_layout)]),
            &crop_module,
            "crop",
            ("vs_full", "fs_crop"),
            LAYER_FORMAT,
            None,
        );
        let filter_module = module("filter", include_str!("shaders/filter.wgsl"));
        let filters = FILTER_PASSES
            .iter()
            .map(|&fs| {
                let pipeline = blended_pipeline(
                    device,
                    &filter_pipeline_layout,
                    &filter_module,
                    fs,
                    ("vs_filter", fs),
                    LAYER_FORMAT,
                    None,
                );
                (fs, pipeline)
            })
            .collect();
        let liquify_module = module("liquify", include_str!("shaders/liquify.wgsl"));
        let liquify_stamp = blended_pipeline(
            device,
            &layout("liquify stamps", &[Some(&uniform_layout)]),
            &liquify_module,
            "liquify stamps",
            ("vs_stamp", "fs_stamp"),
            LIQUIFY_SUMS_FORMAT,
            Some(ADD),
        );
        let liquify_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("liquify step"),
            entries: &[tex_entry(0), tex_entry(1), tex_entry(2)],
        });
        let liquify_step = blended_pipeline(
            device,
            &layout("liquify step", &[Some(&uniform_layout), Some(&liquify_layout)]),
            &liquify_module,
            "liquify step",
            ("vs_step", "fs_step"),
            liquify_format,
            None,
        );
        let surround_pipeline_layout = layout("surround", &[Some(&uniform_layout)]);
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let compute_texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let smudge_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("smudge"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(SMUDGE_UNIFORM_SIZE),
                    },
                    count: None,
                },
                storage(1),
                storage(2),
                storage(3),
                storage(4),
                // a pass of wet paint's stamps
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(WET_PASS_SIZE),
                    },
                    count: None,
                },
                // a dual brush's second, and the layer as it was (its paint laid in place)
                compute_texture(6),
                compute_texture(7),
            ],
        });
        let smudge_module =
            module("smudge", concat!(include_str!("shaders/blend.wgsl"), include_str!("shaders/smudge.wgsl")));
        let smudge_pipeline_layout = layout("smudge", &[Some(&smudge_layout), Some(&brush_layout)]);
        let compute = |label, entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&smudge_pipeline_layout),
                module: &smudge_module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let smudge_carry = compute("smudge carry", "cs_carry");
        let smudge_blur_down = compute("smudge blur down", "cs_blur_down");
        let smudge_blur_across = compute("smudge blur across", "cs_blur_across");
        let smudge_deposit = compute("smudge deposit", "cs_deposit");
        let smudge_synthesize = compute("smudge synthesize", "cs_synthesize");
        let smudge_wet_plots = compute("smudge wet plots", "cs_wet_plots");

        let overlay_module = module("overlay", include_str!("shaders/overlay.wgsl"));
        let stamp_pipeline = |label, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&stamp_layout),
                vertex: wgpu::VertexState {
                    module: &stamp_module,
                    entry_point: Some("vs_stamp"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<StampInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &STAMP_ATTRIBUTES,
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &stamp_module,
                    entry_point: Some("fs_stamp"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: STROKE_FORMAT,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let stamp = stamp_pipeline("stamp", PREMULTIPLIED_OVER);
        let stamp_max = stamp_pipeline("stamp max", MAXIMUM);
        // stamps adding towards their transfer: the paint (scaled by it) over what is there, kept
        // by what the stamp covers (a second colour out of the shader)
        let stamp_capped = device.features().contains(wgpu::Features::DUAL_SOURCE_BLENDING).then(|| {
            let capped_module = module(
                "stamp capped",
                concat!(
                    "enable dual_source_blending;\n",
                    include_str!("shaders/blend.wgsl"),
                    include_str!("shaders/stamp.wgsl"),
                    include_str!("shaders/stamp_capped.wgsl")
                ),
            );
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("stamp capped"),
                layout: Some(&stamp_layout),
                vertex: wgpu::VertexState {
                    module: &capped_module,
                    entry_point: Some("vs_stamp"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<StampInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &STAMP_ATTRIBUTES,
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &capped_module,
                    entry_point: Some("fs_stamp_capped"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: STROKE_FORMAT,
                        blend: Some(TOWARDS_TRANSFER),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        });
        let full = |label, entry, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&composite_layout),
                vertex: wgpu::VertexState {
                    module: &composite_module,
                    entry_point: Some("vs_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &composite_module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: LAYER_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Pipelines {
            layer: full("layer", "fs_layer", Some(PREMULTIPLIED_OVER)),
            blend: full("blend", "fs_blend", None),
            solid: full("solid", "fs_solid", None),
            commit_paint: full("commit paint", "fs_commit", Some(PREMULTIPLIED_OVER)),
            commit_erase: full("commit erase", "fs_commit", Some(DESTINATION_OUT)),
            commit_locked: full("commit locked", "fs_commit_locked", None),
            transform: blended_pipeline(
                device,
                &transform_pipeline_layout,
                &transform_module,
                "transform",
                ("vs_transform", "fs_transform"),
                LAYER_FORMAT,
                Some(PREMULTIPLIED_OVER),
            ),
            transform_static: sampled_pipeline(
                device,
                &transform_pipeline_layout,
                &transform_module,
                "transform static",
                "vs_transform",
                "fs_static",
                LAYER_FORMAT,
            ),
            transform_layout,
            bounds_layout,
            bounds_rows,
            bounds_columns,
            changed_layout,
            changed_rows,
            changed_columns,
            flip_across,
            flip_down,
            invert,
            crop_layout,
            crop,
            before_layout,
            filters,
            filter_layout: filter_pipeline_layout,
            liquify_stamp,
            liquify_step,
            liquify_format,
            liquify_layout,
            mesh: device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("warp"),
                layout: Some(&transform_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &transform_module,
                    entry_point: Some("vs_mesh"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<[f32; 4]>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &MESH_ATTRIBUTES,
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &transform_module,
                    entry_point: Some("fs_mesh"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: LAYER_FORMAT,
                        blend: Some(PREMULTIPLIED_OVER),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            }),
            thumb: sampled_pipeline(
                device,
                &display_pipeline_layout,
                &display_module,
                "thumbnail",
                "vs_thumb",
                "fs_thumb",
                LAYER_FORMAT,
            ),
            stamp,
            stamp_max,
            stamp_capped,
            smudge_layout,
            smudge_carry,
            smudge_blur_down,
            smudge_blur_across,
            smudge_deposit,
            smudge_synthesize,
            smudge_wet_plots,
            lut_layout,
            display_lut_layout,
            quad_layout,
            brush_layout,
            sampled_layout,
            uniform_layout,
            display_module,
            display_pipeline_layout,

            surround_module,
            surround_pipeline_layout,

            stamp_layout,
            overlay_module,

            selection_module,

            backdrop_module,
        }
    }
}

/// The adjustment passes, by their fragment entry in `filter.wgsl`.
/// A copy of `rect` of the layer-format texture `source`, for an undo step.
fn region_copy(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    source: &wgpu::Texture,
    rect: IRect,
) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("undo"),
        size: extent(rect.width(), rect.height()),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: LAYER_FORMAT,
        usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    encoder.copy_texture_to_texture(
        copy_at(source, rect.x0, rect.y0),
        copy_at(&texture, 0, 0),
        extent(rect.width(), rect.height()),
    );
    texture
}

/// The smallest a liquify stamp can be, canvas pixels.
const LIQUIFY_LEAST_RADIUS: f32 = 0.25;

/// What a liquify step's stamps add up to: an offset, and how much is rebuilt.
const LIQUIFY_SUMS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The liquify field: two floats of offset a pixel, as many bits as can be drawn into here. Each
/// step redraws the field from itself, so half floats would drift.
pub(crate) fn liquify_format(adapter: &wgpu::Adapter) -> wgpu::TextureFormat {
    let wanted = wgpu::TextureFormat::Rg32Float;
    let needed = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT;
    let can = adapter.get_texture_format_features(wanted).allowed_usages.contains(needed);
    if can { wanted } else { wgpu::TextureFormat::Rg16Float }
}

/// Added to what is there (liquify stamps into the field).
const ADD: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

const FILTER_PASSES: [&str; 17] = [
    "fs_glitch",
    "fs_clone",
    "fs_liquified",
    "fs_hsb",
    "fs_down",
    "fs_blur",
    "fs_up",
    "fs_sharpen",
    "fs_noise",
    "fs_streak",
    "fs_zoom",
    "fs_chroma",
    "fs_halftone",
    "fs_bloom",
    "fs_curves",
    "fs_gradient",
    "fs_balance",
];

/// A full-screen pipeline (no vertex buffers) with its blending.
fn blended_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    label: &str,
    (vs, fs): (&str, &str),
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vs),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn sampled_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    label: &str,
    vs: &str,
    fs: &str,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    blended_pipeline(device, layout, module, label, (vs, fs), format, None)
}

/// One frame's uniforms, a slot per draw.
#[derive(Default)]
struct UniformData {
    data: Vec<u8>,
}

impl UniformData {
    /// Adds `value` in the next slot (as many as it fills) and returns its dynamic offset.
    fn push<T: bytemuck::Pod>(&mut self, value: &T) -> u32 {
        let offset = self.data.len();
        self.data.extend_from_slice(bytemuck::bytes_of(value));
        let slots = (size_of::<T>() as u64).div_ceil(SLOT).max(1);
        self.data.resize(offset + (slots * SLOT) as usize, 0);
        u32::try_from(offset).unwrap_or(0)
    }
}

/// `ops` without the tips taken off again among them: what a tip covered comes back as it was, so
/// one that never reached the screen is work for nothing. Behind with a fast stroke, a frame has
/// a tip for every input event since the last, and drawing them all left it further behind.
fn without_passing_tips(ops: Vec<Op>) -> Vec<Op> {
    let mut kept: Vec<Op> = Vec::with_capacity(ops.len());
    for op in ops {
        if let Op::Untip(rect) = op
            && let [.., Op::SaveTip(saved), Op::Stamps { .. }] = kept.as_slice()
            && *saved == rect
        {
            kept.truncate(kept.len() - 2);
            continue;
        }
        kept.push(op);
    }
    kept
}

/// The work of `stamps` on a canvas `full` (see [`stamp_work`]).
fn stamps_work(stamps: &[StampInstance], full: IRect, in_place: bool) -> u64 {
    stamps.iter().fold(0, |work, s| work.saturating_add(stamp_work(s, full, in_place)))
}

/// `ops` with stamps of more work than `most` as several in a row of no more (a stamp is never cut
/// in two): a stroke redrawn whole would otherwise reach the GPU as one piece.
fn in_pieces(ops: Vec<Op>, full: IRect, most: u64) -> Vec<Op> {
    let mut pieces = Vec::with_capacity(ops.len());
    for op in ops {
        let Op::Stamps { second, instances, wet, times, style, clear, layer, pencil } = op else {
            pieces.push(op);
            continue;
        };
        let in_place = style.in_place() && !second;
        // how many stamps go in each
        let mut counts = Vec::new();
        let (mut count, mut work) = (0usize, 0u64);
        for s in &instances {
            let more = stamp_work(s, full, in_place);
            if count > 0 && work.saturating_add(more) > most {
                counts.push(count);
                (count, work) = (0, 0);
            }
            count += 1;
            work = work.saturating_add(more);
        }
        if counts.is_empty() {
            pieces.push(Op::Stamps { second, instances, wet, times, style, clear, layer, pencil });
            continue;
        }
        counts.push(count);
        let (mut instances, mut wet, mut times) = (instances.into_iter(), wet.into_iter(), times.into_iter());
        for (i, count) in counts.into_iter().enumerate() {
            pieces.push(Op::Stamps {
                second,
                instances: instances.by_ref().take(count).collect(),
                wet: wet.by_ref().take(count).collect(),
                times: times.by_ref().take(count).collect(),
                style,
                // from scratch the first time only
                clear: clear && i == 0,
                layer,
                pencil,
            });
        }
    }
    pieces
}

/// The rows of `rect` (on the picture) out of a picture `width` pixels wide (RGBA8).
fn cropped(pixels: &[u8], width: u32, rect: IRect) -> Vec<u8> {
    let (row, x0, x1) = (width as usize * 4, rect.x0.max(0) as usize * 4, rect.x1.max(0) as usize * 4);
    let mut out = Vec::with_capacity(rect.width() as usize * rect.height() as usize * 4);
    for line in pixels.chunks_exact(row.max(1)).skip(rect.y0.max(0) as usize).take(rect.height() as usize) {
        out.extend_from_slice(line.get(x0..x1).unwrap_or_default());
    }
    out
}

fn texture(device: &wgpu::Device, label: &str, width: u32, height: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
    // where the GPU's block compression loses parts of a picture redrawn a part at a time (Mali),
    // a texture that can be a storage one is kept uncompressed
    let uncompressed = crate::textures_uncompressed() && format == wgpu::TextureFormat::Rgba8Unorm;
    #[cfg(test)]
    let width = if tests::fails(label) { device.limits().max_texture_dimension_2d + 1 } else { width };
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: extent(width, height),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | if uncompressed { wgpu::TextureUsages::STORAGE_BINDING } else { wgpu::TextureUsages::empty() },
        view_formats: &[],
    })
}

/// A pair of textures layers are composited onto, the result in one of them.
/// The smallest box [x0, y0, x1, y1) around a thumbnail's pixels with any alpha.
fn alpha_bounds(t: &Thumbnail) -> Option<[u32; 4]> {
    painted_bounds(t, |[_, _, _, alpha]| alpha > 0)
}

/// Where a mask's thumbnail is painted on: what isn't white.
fn ink_bounds(t: &Thumbnail) -> Option<[u32; 4]> {
    painted_bounds(t, |[r, ..]| r < 255)
}

/// The pixels of a thumbnail that `painted` holds for, as [x0, y0, x1, y1); none if none.
fn painted_bounds(t: &Thumbnail, painted: impl Fn([u8; 4]) -> bool) -> Option<[u32; 4]> {
    let mut b: Option<[u32; 4]> = None;
    for (i, px) in t.rgba.as_chunks::<4>().0.iter().enumerate() {
        if painted(*px) {
            let (x, y) = ((i as u32) % t.width.max(1), (i as u32) / t.width.max(1));
            b = Some(match b {
                Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)],
                None => [x, y, x + 1, y + 1],
            });
        }
    }
    b
}

/// The longest side of a parked layer's small pictures, pixels: as big as any thumbnail asked for.
const PARKED_PICTURE_SIDE: u32 = 192;

/// How a remake of the caches went: over how many renders, and the most items one part took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemakeStats {
    pub renders: u32,
    pub largest_part: usize,
}

/// A layer's pixels made ready to park: compressed, with its small pictures. Made on any thread
/// (the slow part of [`Canvas::park_pixels`]), then given to [`Canvas::park_ready`].
pub struct ReadyToPark {
    parked: Parked,
    pictures: Pictures,
}

impl ReadyToPark {
    /// Of a layer's pixels (premultiplied RGBA, `width` x `height`).
    pub fn of(rgba: &[u8], width: u32, height: u32) -> ReadyToPark {
        let pictures = Pictures::of(rgba, width, height);
        ReadyToPark { parked: Parked::of(rgba, width, pictures.bounds), pictures }
    }

    /// How many bytes the compressed pixels take.
    pub fn bytes(&self) -> usize {
        self.parked.lz4.len()
    }
}

/// A parked layer's pixels: the rectangle painted (none if nothing), its rows compressed, so a
/// layer drawn on in one place costs little to keep and to bring back.
struct Parked {
    rect: Option<IRect>,
    lz4: Vec<u8>,
}

impl Parked {
    /// Of a layer's pixels (premultiplied RGBA, `width` wide), painted only within `rect`.
    fn of(rgba: &[u8], width: u32, rect: Option<IRect>) -> Parked {
        let rect = rect.filter(|r| !r.is_empty());
        let mut rows = Vec::new();
        if let Some(r) = rect {
            let row = r.width() as usize * 4;
            rows.reserve(row * r.height() as usize);
            for y in r.y0..r.y1 {
                let start = (y as usize * width as usize + r.x0 as usize) * 4;
                rows.extend_from_slice(rgba.get(start..start + row).unwrap_or_default());
            }
        }
        Parked { rect, lz4: procraft_engine::applz4::encode(&rows) }
    }

    /// The painted rectangle's rows, one after another; none if damaged.
    fn rows(&self) -> Option<Vec<u8>> {
        let size = self.rect.map_or(0, |r| r.width() as usize * r.height() as usize * 4);
        procraft_engine::applz4::decode(&self.lz4, size).filter(|p| p.len() == size)
    }

    /// The whole layer (`width` x `height`, clear outside the rectangle); none if damaged.
    fn pixels(&self, width: u32, height: u32) -> Option<Vec<u8>> {
        let rows = self.rows()?;
        let mut full = vec![0u8; width as usize * height as usize * 4];
        if let Some(r) = self.rect {
            let row = r.width() as usize * 4;
            for (y, src) in (r.y0..r.y1).zip(rows.chunks_exact(row.max(1))) {
                let start = (y as usize * width as usize + r.x0 as usize) * 4;
                full.get_mut(start..start + row)?.copy_from_slice(src);
            }
        }
        Some(full)
    }
}

/// A parked layer's small pictures: the whole layer, and what is drawn on it (none if nothing);
/// and where it is drawn (none if nowhere).
struct Pictures {
    whole: Thumbnail,
    drawn: Option<Thumbnail>,
    bounds: Option<IRect>,
}

impl Pictures {
    /// Of a layer's pixels (premultiplied RGBA, `width` x `height`).
    fn of(rgba: &[u8], width: u32, height: u32) -> Pictures {
        let full = Thumbnail { width, height, rgba: rgba.to_vec() };
        let whole = shrink(&full, PARKED_PICTURE_SIDE);
        let painted = alpha_bounds(&full);
        let drawn = painted.map(|b| shrink(&crop(&full, b), PARKED_PICTURE_SIDE));
        let bounds =
            painted.map(|[x0, y0, x1, y1]| IRect { x0: x0 as i32, y0: y0 as i32, x1: x1 as i32, y1: y1 as i32 });
        Pictures { whole, drawn, bounds }
    }
}

/// A picture box-filtered to fit `side` pixels (never enlarged).
fn shrink(t: &Thumbnail, side: u32) -> Thumbnail {
    let (width, height) = (t.width.max(1), t.height.max(1));
    let scale = (side.max(1) as f32 / width.max(height) as f32).min(1.0);
    let w = ((width as f32 * scale).round() as u32).clamp(1, width);
    let h = ((height as f32 * scale).round() as u32).clamp(1, height);
    if (w, h) == (width, height) {
        return Thumbnail { width, height, rgba: t.rgba.clone() };
    }
    let mut sums = vec![0u32; w as usize * h as usize * 4];
    let mut counts = vec![0u32; w as usize * h as usize];
    for (y, row) in t.rgba.chunks_exact(width as usize * 4).enumerate() {
        let ty = (y as u64 * u64::from(h) / u64::from(height)) as usize;
        for (x, px) in row.as_chunks::<4>().0.iter().enumerate() {
            let tx = (x as u64 * u64::from(w) / u64::from(width)) as usize;
            let i = ty * w as usize + tx;
            if let Some(c) = counts.get_mut(i) {
                *c += 1;
            }
            for (k, v) in px.iter().enumerate() {
                if let Some(sum) = sums.get_mut(i * 4 + k) {
                    *sum += u32::from(*v);
                }
            }
        }
    }
    let rgba = sums
        .as_chunks::<4>()
        .0
        .iter()
        .zip(&counts)
        .flat_map(|(sum, n)| sum.iter().map(move |v| ((v + n / 2) / (*n).max(1)) as u8).collect::<Vec<_>>())
        .collect();
    Thumbnail { width: w, height: h, rgba }
}

/// Part [x0, y0, x1, y1) of a thumbnail.
fn crop(t: &Thumbnail, [x0, y0, x1, y1]: [u32; 4]) -> Thumbnail {
    let row = t.width as usize * 4;
    let mut rgba = Vec::with_capacity((x1 - x0) as usize * (y1 - y0) as usize * 4);
    for y in y0..y1 {
        let start = y as usize * row + x0 as usize * 4;
        rgba.extend_from_slice(t.rgba.get(start..start + (x1 - x0) as usize * 4).unwrap_or_default());
    }
    Thumbnail { width: x1 - x0, height: y1 - y0, rgba }
}

/// A pair of textures layers are composited onto, the result in one of them: the first, and
/// the second that blend modes other than Normal draw into.
struct Stack<'a> {
    first: (&'a wgpu::Texture, &'a wgpu::TextureView),
    second: Second<'a>,
}

enum Second<'a> {
    Made(&'a wgpu::Texture, &'a wgpu::TextureView),
    /// Made the first time a blend needs it.
    OnDemand(&'a OnDemand, &'a wgpu::Device),
}

/// A texture made the first time it is wanted, and kept from then on: one most canvases never
/// want (a ping-pong partner, a scratch copy).
struct OnDemand {
    label: &'static str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    made: OnceLock<(wgpu::Texture, wgpu::TextureView)>,
}

impl OnDemand {
    fn new(label: &'static str, width: u32, height: u32, format: wgpu::TextureFormat) -> OnDemand {
        OnDemand { label, width, height, format, made: OnceLock::new() }
    }

    fn get<'a>(&'a self, device: &wgpu::Device) -> (&'a wgpu::Texture, &'a wgpu::TextureView) {
        let (texture, view) = self.made.get_or_init(|| {
            let t = texture(device, self.label, self.width, self.height, self.format);
            let v = t.create_view(&wgpu::TextureViewDescriptor::default());
            (t, v)
        });
        (texture, view)
    }

    /// The texture, if it has been made.
    fn made(&self) -> Option<&wgpu::Texture> {
        self.made.get().map(|(t, _)| t)
    }

    /// Lets the texture go, to be made again when next wanted.
    fn let_go(&mut self) {
        self.made.take();
    }
}

/// Buffers that pixels go to the GPU through, written while mapped and copied in a command
/// encoder's order, then mapped again for the next upload once the GPU is done with them:
/// `queue.write_texture` makes a buffer for every write, which for a layer the canvas's size took
/// several times the copy itself on a tablet. A few are kept (to [`UPLOADS_KEPT`] bytes).
#[derive(Default)]
struct Uploads {
    kept: Mutex<Vec<(wgpu::Buffer, Arc<AtomicU8>)>>,
}

/// An upload buffer's state: on its way back, mapped again, or not mapped (let go).
const UPLOAD_WAITING: u8 = 0;
const UPLOAD_READY: u8 = 1;
const UPLOAD_FAILED: u8 = 2;

/// The most bytes of upload buffers kept between uploads: two layers the size of a large tablet
/// canvas; bigger ones are made for each upload.
const UPLOADS_KEPT: u64 = 96 << 20;

impl Uploads {
    /// A mapped buffer of `size` bytes or more: one kept and back from the GPU, or one waited
    /// for, or else a new one.
    fn take(&self, device: &wgpu::Device, size: u64) -> wgpu::Buffer {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        kept.retain(|(_, state)| state.load(Ordering::Acquire) != UPLOAD_FAILED);
        let fits = |kept: &[(wgpu::Buffer, Arc<AtomicU8>)]| {
            kept.iter().position(|(b, state)| b.size() >= size && state.load(Ordering::Acquire) == UPLOAD_READY)
        };
        let _ = device.poll(wgpu::PollType::Poll);
        let mut found = fits(&kept);
        if found.is_none() && kept.iter().any(|(b, _)| b.size() >= size) {
            // one on its way back: waited for rather than another as big made
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            found = fits(&kept);
        }
        if let Some(i) = found {
            return kept.swap_remove(i).0;
        }
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("upload"),
            size,
            usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        })
    }

    /// `buffer` back once what used it has been sent: mapped again, and kept while there is room.
    fn give_back(&self, buffer: wgpu::Buffer) {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        let room = kept.iter().map(|(b, _)| b.size()).sum::<u64>().saturating_add(buffer.size()) <= UPLOADS_KEPT;
        if !room {
            return;
        }
        let state = Arc::new(AtomicU8::new(UPLOAD_WAITING));
        let mapped = state.clone();
        buffer.slice(..).map_async(wgpu::MapMode::Write, move |r| {
            mapped.store(if r.is_ok() { UPLOAD_READY } else { UPLOAD_FAILED }, Ordering::Release);
        });
        kept.push((buffer, state));
    }
}

/// Pictures of the top level's items that aren't being worked on, so only those that are
/// composite each time (and the others' layers needn't be at hand): the background with every
/// item below them, and a run of plain items above them over transparency.
struct Caches {
    /// The top level's items composited live, and every layer in them.
    live: Vec<LayerId>,
    live_layers: HashSet<LayerId>,
    /// The items below them, composited over the background (in the first texture), and what
    /// that was made from.
    below: [wgpu::Texture; 2],
    below_views: [wgpu::TextureView; 2],
    below_items: Vec<LayerId>,
    below_key: u64,
    /// The items above them, over transparency (in the first texture), and what from.
    above: [wgpu::Texture; 2],
    above_views: [wgpu::TextureView; 2],
    above_items: Vec<LayerId>,
    above_key: u64,
}

/// How many layers a render reads back to park, at most, and how long it goes on reading them
/// (after the first).
const MOST_PARK_READS: usize = 4;
const PARK_SLICE: std::time::Duration = std::time::Duration::from_millis(6);

/// The room (in layers' textures) a remake keeps free for its next part, letting go of the
/// layers it brought back only so far.
const LET_GO_ROOM: usize = 2 * MOST_PART_ITEMS;

/// The most items a part of a remake composites before it is sent (and the time looked at).
const MOST_PART_ITEMS: usize = 8;

/// The most uniform slots a filter takes: a wide blur's halvings (at most five), its two passes
/// and the filter's own.
const FILTER_SLOTS: usize = 8;

/// How long each render goes on remaking caches before it lets the app go on.
const REMAKE_SLICE: std::time::Duration = std::time::Duration::from_millis(16);

/// Caches being remade a little each render: their textures, how far each has got (the next item
/// and where its stack's result is; none when done or not remade), what they are made from,
/// and the layers worked on, which stay.
struct Remake {
    caches: Caches,
    below: Option<(usize, usize)>,
    above: Option<(usize, usize)>,
    below_items: Vec<LayerId>,
    below_key: u64,
    above_items: Vec<LayerId>,
    above_key: u64,
    keep: HashSet<LayerId>,
    /// For the log: when it started, over how many renders, and the layers brought back for it
    /// and the time that took.
    started: std::time::Instant,
    slices: u32,
    largest_part: usize,
    restored: usize,
    restoring: std::time::Duration,
}

/// How many levels of group textures compositing `doc` takes: down to the deepest group
/// composited on its own (with Animation Assist any group, its frame's onion skin tinted whole).
fn group_levels(doc: &Document) -> usize {
    if doc.animation.is_some() {
        return doc.max_depth();
    }
    let alone = doc.layers().iter().filter(|l| l.kind == LayerKind::Group && !l.passes_through());
    alone.map(|l| doc.depth(l.id) + 1).max().unwrap_or(0)
}

/// Whether item `id` composited over transparency, then over what is below, shows as it does
/// composited straight onto it: normal, and if a group passing through, only of such items.
fn plain(doc: &Document, id: LayerId) -> bool {
    match doc.layer(id) {
        Some(l) if l.kind == LayerKind::Mask => true,
        Some(l) if l.blend != BlendMode::Normal => false,
        Some(l) if l.passes_through() => doc.members(Some(id)).iter().all(|m| plain(doc, *m)),
        _ => true,
    }
}

/// What a top-level item composites from: itself and its mask, or a group and all it holds.
fn item_layers(doc: &Document, item: LayerId) -> Vec<LayerId> {
    let mut layers = vec![item];
    match doc.layer(item).map(|l| l.kind) {
        Some(LayerKind::Group) => layers.extend(doc.contents(item)),
        Some(LayerKind::Paint) => layers.extend(doc.mask_of(item)),
        _ => {}
    }
    layers
}

/// A fingerprint of how `items` composite (with the background, if `background`): every setting
/// of every layer in them, and which layer is shown alone. Their pixels change only through work
/// that drops the caches.
fn items_key(doc: &Document, items: &[LayerId], background: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if background {
        format!("{:?} {}", doc.background, doc.background_visible).hash(&mut h);
    }
    format!("{:?}", doc.solo).hash(&mut h);
    for &item in items {
        for id in item_layers(doc, item) {
            format!("{:?}", doc.layer(id)).hash(&mut h);
        }
    }
    h.finish()
}

impl<'a> Stack<'a> {
    /// Both made already.
    fn pair(textures: &'a [wgpu::Texture; 2], views: &'a [wgpu::TextureView; 2]) -> Stack<'a> {
        let [t0, t1] = textures;
        let [v0, v1] = views;
        Stack { first: (t0, v0), second: Second::Made(t1, v1) }
    }

    /// The second texture, made now if it has to be.
    fn second(&self) -> (&'a wgpu::Texture, &'a wgpu::TextureView) {
        match self.second {
            Second::Made(t, v) => (t, v),
            Second::OnDemand(lazy, device) => lazy.get(device),
        }
    }

    /// Leaves the result in the first texture (where `current` says it is, within `rect`).
    fn settle(&self, encoder: &mut wgpu::CommandEncoder, rect: IRect, current: usize) {
        if current == 1 {
            encoder.copy_texture_to_texture(
                copy_at(self.second().0, rect.x0, rect.y0),
                copy_at(self.first.0, rect.x0, rect.y0),
                extent(rect.width(), rect.height()),
            );
        }
    }
}

fn quad_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    views: [&wgpu::TextureView; 7],
    grains: &wgpu::TextureView,
    grain_sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    let [a, b, c, d, mask, second, clip_mask] = views;
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("layer, stroke, clip, backdrop, grains"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(a) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(b) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(c) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(d) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(grains) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(grain_sampler) },
            wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(mask) },
            wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(second) },
            wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(clip_mask) },
        ],
    })
}

/// The layer `first` is clipped to, and that layer's mask: what compositing it reads.
fn clip_base_and_mask(doc: &Document, first: LayerId) -> impl Iterator<Item = LayerId> {
    let base = doc.clip_base(first);
    base.into_iter().chain(base.and_then(|b| doc.mask_of(b)))
}

/// How many images of the user's own each of the shape and grain arrays holds at once.
const SOURCE_SLOTS: usize = 8;

/// How many brush files' grains stay on the GPU at their own size.
const FULL_GRAINS_HELD: usize = 3;

/// Brush files' grains or shapes at their own size, each an array with its mip levels, the image in its first
/// layer: only the last few used, so that their memory doesn't grow with the brushes.
#[derive(Default)]
struct FullGrains {
    held: Vec<(u64, wgpu::TextureView, u64)>,
    clock: u64,
}

impl FullGrains {
    /// Has the image `id` ready; false if `pixels` aren't `side` square.
    fn hold(&mut self, gpu: &Gpu, id: u64, side: u32, pixels: &[u8]) -> bool {
        self.clock += 1;
        let clock = self.clock;
        if let Some(slot) = self.held.iter_mut().find(|slot| slot.0 == id) {
            slot.2 = clock;
            return true;
        }
        if !side.is_power_of_two() || pixels.len() != (side as usize).pow(2) {
            return false;
        }
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("full grain"),
            size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: gpu.array_layers_least() },
            mip_level_count: side.ilog2() + 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        write_image_levels(gpu, &texture, 0, side, pixels);
        if self.held.len() >= FULL_GRAINS_HELD
            && let Some(oldest) = self.held.iter().enumerate().min_by_key(|(_, slot)| slot.2).map(|(i, _)| i)
        {
            self.held.swap_remove(oldest);
        }
        self.held.push((id, array_view(&texture), clock));
        true
    }

    fn view(&self, id: u64) -> Option<&wgpu::TextureView> {
        self.held.iter().find(|slot| slot.0 == id).map(|slot| &slot.1)
    }
}

/// The spare layers of a brush image array, after the shipped images: which image of the user's
/// own each holds, and when it was last used (the longest unused gives way).
struct SourceSlots {
    first: u32,
    held: Vec<(u64, u64)>,
    clock: u64,
}

impl SourceSlots {
    fn after(shipped: usize) -> SourceSlots {
        SourceSlots { first: shipped as u32, held: Vec::new(), clock: 0 }
    }

    /// The layer of `texture` holding `source`, put there if it isn't.
    fn layer(&mut self, gpu: &Gpu, texture: &wgpu::Texture, source: &Source) -> u32 {
        self.clock += 1;
        let clock = self.clock;
        if let Some(i) = self.held.iter().position(|&(id, _)| id == source.id) {
            if let Some(slot) = self.held.get_mut(i) {
                slot.1 = clock;
            }
            return self.first + i as u32;
        }
        let i = if self.held.len() < SOURCE_SLOTS {
            self.held.push((source.id, clock));
            self.held.len() - 1
        } else {
            let oldest = self.held.iter().enumerate().min_by_key(|(_, slot)| slot.1).map_or(0, |(i, _)| i);
            if let Some(slot) = self.held.get_mut(oldest) {
                *slot = (source.id, clock);
            }
            oldest
        };
        let layer = self.first + i as u32;
        write_brush_image(gpu, texture, layer, &source.pixels);
        layer
    }
}

/// One white texel standing in for an array of brush images, until a canvas wants them.
fn stand_in_images(gpu: &Gpu, label: &str) -> wgpu::Texture {
    let layers = gpu.array_layers_least();
    gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &vec![255; layers as usize],
    )
}

/// Greyscale brush images as one array texture with a full mip chain (each level a 2×2 average
/// of the one above), with [`SOURCE_SLOTS`] spare layers after them.
fn brush_images(gpu: &Gpu, label: &str, images: &[GrayImage]) -> wgpu::Texture {
    let n = TEXTURE_SIZE as u32;
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: n, height: n, depth_or_array_layers: (images.len() + SOURCE_SLOTS) as u32 },
        mip_level_count: n.ilog2() + 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (layer, image) in images.iter().enumerate() {
        write_brush_image(gpu, &texture, layer as u32, &image.pixels);
    }
    texture
}

/// One brush image (`TEXTURE_SIZE` square) into `layer` of `texture`, every mip level.
fn write_brush_image(gpu: &Gpu, texture: &wgpu::Texture, layer: u32, pixels: &[u8]) {
    write_image_levels(gpu, texture, layer, TEXTURE_SIZE as u32, pixels);
}

/// A greyscale image `n` square into `layer` of `texture`, every mip level.
fn write_image_levels(gpu: &Gpu, texture: &wgpu::Texture, layer: u32, n: u32, pixels: &[u8]) {
    let mut level = pixels.to_vec();
    let mut size = n;
    for mip in 0..n.ilog2() + 1 {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: mip,
                origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                aspect: wgpu::TextureAspect::All,
            },
            &level,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size), rows_per_image: Some(size) },
            wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        );
        if size > 1 {
            level = half_size(&level, size as usize);
            size /= 2;
        }
    }
}

/// A square greyscale image at half the size: each pixel the average of four.
fn half_size(pixels: &[u8], size: usize) -> Vec<u8> {
    let half = size / 2;
    let at = |x: usize, y: usize| u32::from(pixels.get(y * size + x).copied().unwrap_or(0));
    let mut out = Vec::with_capacity(half * half);
    for y in 0..half {
        for x in 0..half {
            let sum = at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1);
            out.push(((sum + 2) / 4) as u8);
        }
    }
    out
}

/// The stamp shader's shapes, grains and selection, and its two samplers.
fn brush_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    [shapes, grains, mask]: [&wgpu::TextureView; 3],
    [shape_sampler, grain_sampler]: [&wgpu::Sampler; 2],
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shapes, grains and the selection"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(shapes) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(grains) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(shape_sampler) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(grain_sampler) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(mask) },
        ],
    })
}

/// A one-channel texture of coverage, `width` × `height`, from `data` (one byte a pixel).
fn coverage_texture(device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32, data: &[u8]) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("selection"),
        size: extent(width, height),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width), rows_per_image: Some(height) },
        extent(width, height),
    );
    texture
}

/// A 3D colour table `grid` on a side (RGBA8, blue varying fastest, then green, then red) and
/// its sampler, bound for the display.
fn lut_bind(gpu: &Gpu, layout: &wgpu::BindGroupLayout, grid: u32, rgba: &[u8]) -> wgpu::BindGroup {
    let size = wgpu::Extent3d { width: grid, height: grid, depth_or_array_layers: grid };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("colour table"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * grid), rows_per_image: Some(grid) },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("colour table"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("colour table"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    })
}

fn sampled_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sampled"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

fn uniforms(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, slots: u64) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("uniforms"),
        size: slots * SLOT,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("uniforms"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &buffer,
                offset: 0,
                size: NonZeroU64::new(UNIFORM_SIZE),
            }),
        }],
    });
    (buffer, bind)
}

/// Uniform slots for `count` smudge stamps.
fn spare_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("smudge spare"),
        size: 16,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    })
}

fn wet_params(device: &wgpu::Device, count: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("wet passes"),
        size: count.max(1) * WET_PASS_SIZE.div_ceil(SLOT) * SLOT,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn smudge_params(device: &wgpu::Device, count: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("smudge stamps"),
        size: count.max(1) * SLOT,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn stamp_buffer(device: &wgpu::Device, capacity: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("stamps"),
        size: capacity.max(1) * size_of::<StampInstance>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn render_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

fn scissor(pass: &mut wgpu::RenderPass<'_>, r: IRect) {
    let x = u32::try_from(r.x0).unwrap_or(0);
    let y = u32::try_from(r.y0).unwrap_or(0);
    pass.set_scissor_rect(x, y, r.width(), r.height());
}

fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 }
}

fn copy_at(texture: &wgpu::Texture, x: i32, y: i32) -> wgpu::TexelCopyTextureInfo<'_> {
    wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d { x: u32::try_from(x).unwrap_or(0), y: u32::try_from(y).unwrap_or(0), z: 0 },
        aspect: wgpu::TextureAspect::All,
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Reads a texture of 4-byte pixels back from the GPU, rows top to bottom without padding.
pub fn read_texture(gpu: &Gpu, texture: &wgpu::Texture, width: u32, height: u32) -> Result<Vec<u8>, GpuError> {
    start_reading(gpu, texture, width, height).pixels()
}

/// Pixels on their way back from the GPU: their copy is queued (under whatever lock guards what
/// they are copied from), and waited for later, the lock let go ([`Readback::pixels`]).
pub enum Readback {
    /// Already here.
    Ready(Vec<u8>),
    /// The copy couldn't be made (why): nothing is coming.
    Failed(String),
    Copying {
        device: wgpu::Device,
        /// The buffer and how much of it the pixels take; kept for the next readback after.
        buffer: wgpu::Buffer,
        size: u64,
        kept: Arc<Mutex<Vec<wgpu::Buffer>>>,
        /// A row's bytes, and as the copy pads it.
        row: usize,
        padded: usize,
        mapped: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    },
}

/// The most bytes of readback buffers kept: two layers the size of a large tablet canvas.
const READBACKS_KEPT: u64 = 96 << 20;

impl Readback {
    /// How many bytes the pixels take.
    pub fn bytes(&self) -> usize {
        match self {
            Readback::Ready(p) => p.len(),
            Readback::Failed(_) => 0,
            Readback::Copying { size, .. } => usize::try_from(*size).unwrap_or(usize::MAX),
        }
    }

    /// The pixels (4 bytes each, rows top to bottom without padding), once the GPU has copied
    /// them.
    pub fn pixels(self) -> Result<Vec<u8>, GpuError> {
        let (device, buffer, size, kept, row, padded, mapped) = match self {
            Readback::Ready(p) => return Ok(p),
            Readback::Failed(why) => return Err(GpuError::Readback(why)),
            Readback::Copying { device, buffer, size, kept, row, padded, mapped } => {
                (device, buffer, size, kept, row, padded, mapped)
            }
        };
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| GpuError::Readback(e.to_string()))?;
        mapped.recv().map_err(|e| GpuError::Readback(e.to_string()))?.map_err(|e| GpuError::Readback(e.to_string()))?;
        let view = buffer.slice(..size).get_mapped_range().map_err(|e| GpuError::Readback(e.to_string()))?;
        let mut out = Vec::with_capacity(row * (view.len() / padded.max(1)));
        for chunk in view.chunks(padded.max(1)) {
            out.extend_from_slice(chunk.get(..row).unwrap_or(chunk));
        }
        drop(view);
        buffer.unmap();
        // kept for the next readback while there is room
        let mut kept = kept.lock().unwrap_or_else(PoisonError::into_inner);
        if kept.iter().map(wgpu::Buffer::size).sum::<u64>().saturating_add(buffer.size()) <= READBACKS_KEPT {
            kept.push(buffer);
        }
        Ok(out)
    }
}

/// How many layers `width` x `height` a canvas keeps on the GPU at once with textures' budget
/// `budget` bytes (0: no limit; one at least).
pub fn most_resident(budget: u64, width: u32, height: u32) -> usize {
    let per = u64::from(width) * u64::from(height) * 4;
    if budget == 0 { usize::MAX } else { usize::try_from(budget / per.max(1)).unwrap_or(usize::MAX).max(1) }
}

/// Starts reading a texture of 4-byte pixels back from the GPU: its copy is queued now.
pub fn start_reading(gpu: &Gpu, texture: &wgpu::Texture, width: u32, height: u32) -> Readback {
    start_reading_part(gpu, texture, IRect::from_size(width, height))
}

/// [`start_reading`] for the pixels inside `rect` (on the texture, not empty) only.
fn start_reading_part(gpu: &Gpu, texture: &wgpu::Texture, rect: IRect) -> Readback {
    let (width, height) = (rect.width(), rect.height());
    let row = width as usize * 4;
    let padded = row.div_ceil(256) * 256;
    let size = (padded * height as usize) as u64;
    let buffer = gpu.readback_buffer(size);
    // a texture there was no memory for (or one its device let go) copies nothing, and the buffer
    // would give back what it held before, another layer's pixels perhaps: the read fails instead
    let invalid = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
    encoder.copy_texture_to_buffer(
        copy_at(texture, rect.x0, rect.y0),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded as u32),
                rows_per_image: Some(height),
            },
        },
        extent(width, height),
    );
    gpu.queue.submit([encoder.finish()]);
    if let Some(e) = pollster::block_on(invalid.pop()) {
        return Readback::Failed(e.to_string());
    }
    let (tx, rx) = mpsc::channel();
    buffer.slice(..size).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let kept = gpu.readbacks.clone();
    Readback::Copying { device: gpu.device.clone(), buffer, size, kept, row, padded, mapped: rx }
}

#[cfg(test)]
mod tests {
    use super::*;

    std::thread_local! {
        /// The label of textures this thread can't make, as past the GPU's memory.
        static FAILING: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
    }

    /// Whether a texture labelled `label` is to fail on this thread.
    pub(super) fn fails(label: &str) -> bool {
        FAILING.with(|f| f.get() == Some(label))
    }

    #[test]
    fn a_canvas_made_for_filling_holds_no_layer_textures_until_they_come() {
        use procraft_engine::document::Document;
        let Ok(gpu) = Gpu::headless() else { return };
        let mut doc = Document::new(512, 512);
        for _ in 0..7 {
            doc.add_layer();
        }
        let held = || {
            gpu.flush();
            gpu.device.generate_allocator_report().map_or(0, |r| r.total_allocated_bytes)
        };
        let before = held();
        let mut canvas = Canvas::unfilled(&gpu, &doc).unwrap();
        let unfilled = held() - before;
        canvas.fill_rest(&gpu, &doc);
        // where the device can't say, there is nothing to compare
        let layers = 8 * 512 * 512 * 4;
        assert!(before == 0 || held() - before - unfilled >= layers);
    }

    #[test]
    fn the_history_takes_from_the_room_for_layers() {
        use procraft_engine::document::Document;
        use procraft_engine::stroke::Stamp;
        let Ok(gpu) = Gpu::headless() else { return };
        let mut doc = Document::new(64, 64);
        let per = 64 * 64 * 4;
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        canvas.set_resident_budget(4 * per);
        assert_eq!(canvas.most_resident(), 4);
        let id = doc.current();
        // each stroke over the whole layer keeps a layer's worth to undo
        for _ in 0..2 {
            canvas.begin_stroke(id, StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0));
            canvas.add_stamps(&[Stamp::round(procraft_engine::geom::Vec2::new(32.0, 32.0), 64.0, 1.0)]);
            canvas.end_stroke();
            canvas.render(&gpu, &mut doc, None);
        }
        assert!(canvas.history_bytes() >= 2 * per);
        assert_eq!(canvas.most_resident(), 2);
        // never none
        canvas.set_resident_budget(per);
        assert_eq!(canvas.most_resident(), 1);
    }

    #[test]
    fn a_texture_the_gpu_couldnt_make_reads_as_a_failure_not_as_old_pixels() {
        let Ok(gpu) = Gpu::headless() else { return };
        // a read that leaves its pixels in the buffer kept for the next
        let filled = texture(&gpu.device, "filled", 64, 64, LAYER_FORMAT);
        gpu.queue.write_texture(
            copy_at(&filled, 0, 0),
            &[200; 64 * 64 * 4],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(64 * 4), rows_per_image: Some(64) },
            extent(64, 64),
        );
        assert_eq!(read_texture(&gpu, &filled, 64, 64).unwrap()[0], 200);
        FAILING.with(|f| f.set(Some("empty")));
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let empty = texture(&gpu.device, "empty", 64, 64, LAYER_FORMAT);
        assert!(pollster::block_on(scope.pop()).is_some());
        FAILING.with(|f| f.set(None));
        assert!(read_texture(&gpu, &empty, 64, 64).is_err());
    }

    #[test]
    fn a_smudge_the_gpu_has_no_memory_for_draws_on_and_makes_no_step() {
        use procraft_engine::document::Document;
        use procraft_engine::stroke::Stamp;
        let Ok(gpu) = Gpu::headless() else { return };
        let dot = |x: f32, radius: f32| Stamp::round(procraft_engine::geom::Vec2::new(x, 32.0), radius, 1.0);
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let id = doc.current();
        canvas.begin_stroke(id, StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0));
        canvas.add_stamps(&[dot(20.0, 12.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let before = canvas.read_layer(&gpu, id).unwrap();
        let steps = canvas.steps_made();
        let smudge = || StrokeStyle::new(StrokeTool::Smudge, [0.0; 3], 1.0);
        let stamps: Vec<Stamp> = (0..8).map(|i| dot(20.0 + i as f32 * 3.0, 10.0)).collect();
        // refused: frames go on being drawn, the layer as it was, nothing to undo, told once
        FAILING.with(|f| f.set(Some("smudge before")));
        canvas.begin_stroke(id, smudge());
        canvas.add_stamps(&stamps);
        canvas.render(&gpu, &mut doc, None);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        FAILING.with(|f| f.set(None));
        assert_eq!(gpu.errors(), 0);
        assert!(canvas.take_smudge_refused());
        assert!(!canvas.take_smudge_refused());
        assert_eq!(canvas.steps_made(), steps);
        assert_eq!(canvas.read_layer(&gpu, id).unwrap(), before);
        // the next, with room, smudges as ever
        canvas.begin_stroke(id, smudge());
        canvas.add_stamps(&stamps);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        assert_eq!(gpu.errors(), 0);
        assert!(!canvas.take_smudge_refused());
        assert_eq!(canvas.steps_made(), steps + 1);
        assert_ne!(canvas.read_layer(&gpu, id).unwrap(), before);
    }

    #[test]
    fn a_stamps_pressure_scales_the_wet_mixes_dilution_and_charge() {
        let style = StrokeStyle {
            dilution: 0.5,
            load: 0.5,
            wet_pull: Some(0.5),
            ..StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0)
        };
        // no dilution at no pressure: the paint goes on whole
        assert_eq!(wet_mixture(&style, 0.0, 1.0, 0.0, 0.0, 1.0).0, 1.0);
        // diluted by half: what it leaves, by 2.4, with no charge
        let (mixed, ..) = wet_mixture(&style, 33.0, 1.0, 0.0, 1.0, 0.0);
        assert!((mixed - 0.5f32.powf(2.4)).abs() < 1e-5);
        // the charge, half run out by 33 px, as far as its curve lets it, goes into the fresh
        // paint with the dilution's; the pull is the brush's to the power 1/8
        let (mixed, pull) = wet_mixture(&style, 33.0, 1.0, 0.0, 1.0, 1.0);
        assert!((mixed - (0.5f32.powf(2.4) + 0.5)).abs() < 1e-5);
        assert!((pull - 0.5f32.powf(0.125)).abs() < 1e-6);
        // at least the attack
        let attacked = StrokeStyle { attack: 0.9, ..style };
        assert!((wet_mixture(&attacked, 1000.0, 1.0, 1.0, 1.0, 0.0).0 - 0.9).abs() < 1e-5);
    }

    #[test]
    fn the_fall_off_runs_the_wet_mixes_paint_out_along_the_stroke() {
        let style = StrokeStyle {
            dilution: 0.5,
            falloff: 0.5,
            wet_pull: Some(0.5),
            ..StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0)
        };
        let diluted = 0.5f32.powf(2.4);
        // whole where the stroke starts, half gone by 33 px, all gone by 264
        assert!((wet_mixture(&style, 0.0, 1.0, 0.0, 1.0, 1.0).0 - diluted).abs() < 1e-5);
        assert!((wet_mixture(&style, 33.0, 1.0, 0.0, 1.0, 1.0).0 - diluted * 0.5).abs() < 1e-5);
        assert!(wet_mixture(&style, 264.0, 1.0, 0.0, 1.0, 1.0).0.abs() < 1e-5);
    }

    #[test]
    fn a_pass_lays_its_stamps_up_to_their_mixture_as_the_grade_says() {
        // the stroke's first pass lays the whole mixture, stamp by stamp
        assert_eq!(final_mixture(0.4, 0.9, 0.0, (0, 4), true), 0.4);
        // later ones ramp up through the pass from the grade's share
        assert_eq!(final_mixture(0.4, 0.9, 0.0, (0, 4), false), 0.0);
        assert!((final_mixture(0.4, 0.9, 0.0, (2, 4), false) - 0.2).abs() < 1e-6);
        assert!((final_mixture(0.4, 0.9, 0.5, (2, 4), false) - 0.3).abs() < 1e-6);
        assert_eq!(final_mixture(0.4, 0.9, 1.0, (0, 4), false), 0.4);
        // neither picking up nor mixing: what it carries, whole
        assert_eq!(final_mixture(0.0, 0.9, 0.0, (1, 4), false), 1.0);
    }
}
