//! The canvas on the GPU: a texture per layer, the stroke being drawn, the composite, and the
//! undo history (pixels and document structure).
//!
//! Calls queue work; [`Canvas::render`] records and submits it in order, recomposites what
//! changed and, given a target, draws the canvas to it.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU64;
use std::sync::mpsc;

use procraft_engine::animation::{self, FrameLook};
use procraft_engine::brush::{BRUSH_BEHIND, Brush, GRAIN_LINEAR_HEIGHT, GrainMode, Rendering, Source, run_out};
use procraft_engine::document::{BlendMode, Document, Layer, LayerId, LayerKind};
use procraft_engine::geom::{Affine, IRect};
use procraft_engine::stroke::{MOST_ROUNDNESS, Stamp};
use procraft_engine::textures::{self, GrayImage, TEXTURE_SIZE};

use crate::{Gpu, GpuError};

const LAYER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Half floats, so a stroke's build-up from many faint stamps doesn't band.
const STROKE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Every draw gets its own slot in the uniform buffer (dynamic offsets are 256-byte aligned).
const SLOT: u64 = 256;
/// The Multiply blend mode's number.
const MULTIPLY_MODE: u32 = 1;

/// The largest uniform block a shader reads.
const UNIFORM_SIZE: u64 = 112;
const _: () = assert!(
    size_of::<LayerUniform>() as u64 <= UNIFORM_SIZE
        && size_of::<StampUniform>() as u64 <= UNIFORM_SIZE
        && size_of::<FilterUniform>() as u64 <= UNIFORM_SIZE,
    "a uniform block outgrew its binding"
);

/// The vertex layout of a warp mesh corner: where it goes, and where it came from (pixels).
const MESH_ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];

/// The vertex layout of a stamp instance (stroke stamps and the predicted tail).
const STAMP_ATTRIBUTES: [wgpu::VertexAttribute; 11] = wgpu::vertex_attr_array![
    0 => Float32x2, 1 => Float32, 2 => Float32, 3 => Float32, 4 => Float32, 5 => Float32x2, 6 => Float32x2, 7 => Float32x4,
    8 => Float32x4, 9 => Float32x4, 10 => Float32
];

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct OverlayUniform {
    m0: [f32; 4],
    m1: [f32; 4],
    color: [f32; 4],
    shape: i32,
    srgb: u32,
    _pad: [u32; 2],
}
/// Undo steps kept, and the most memory their pixels may hold.
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
    /// Wet mix: how many stamps' worth of paint the brush holds; none never runs out.
    pub load: f32,
    /// Wet mix: water in the paint, 0..1: it goes on thinner.
    pub dilution: f32,
    /// The brush's fall off, 0..1: the paint it mixes in runs out along the stroke as its stamps
    /// fade.
    pub falloff: f32,
    /// Wet mix: how hard the paint goes on over what is there (less of it pulled in), its grain
    /// (the mix uneven from pixel to pixel), how blurred what it pulls in is, and how much the
    /// pull varies from stamp to stamp; each 0..1.
    pub attack: f32,
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
    /// No shape filtering: the shape image read pixel by pixel, its edges and specks hard.
    pub shape_sharp: bool,
    /// The secondary colour (sRGB, 0..1), mixed in as far as each stamp says.
    pub secondary: [f32; 3],
    /// Drawn as a brush preview (the studio's drawing pad, the library's strokes), which draws
    /// wet edges only as a firmer rim and burns nothing.
    pub preview: bool,
}

impl StrokeStyle {
    /// Whether the stroke changes its layer as it goes (smudging, painting wet) rather than
    /// building up in the stroke texture.
    pub fn in_place(&self) -> bool {
        self.tool == StrokeTool::Smudge || (self.tool == StrokeTool::Paint && self.wet_pull.is_some())
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
            load: 0.0,
            dilution: 0.0,
            falloff: 0.0,
            attack: 0.0,
            grade: 0.0,
            blur: 0.0,
            wet_jitter: 0.0,
            blend: 0,
            blur_jitter: 0.0,
            grain_blend: MULTIPLY_MODE,
            burnt_blend: MULTIPLY_MODE,
            grain_rotation: 0.0,
            grain_sharp: false,
            shape_sharp: false,
            secondary: [1.0, 1.0, 1.0],
            preview: false,
        }
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
            grain_scale: brush.grain.scale.max(1.0),
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
            load: if wet { brush.setting("wet_charge").clamp(0.0, 1.0) } else { 0.0 },
            dilution: if wet { brush.setting("wet_dilution").clamp(0.0, 1.0) } else { 0.0 },
            falloff: brush.setting("falloff").clamp(0.0, 1.0),
            attack: if wet { brush.setting("wet_attack").clamp(0.0, 1.0) } else { 0.0 },
            // the grade goes both ways; drawn yet only as how uneven the mix is
            grade: if wet { brush.setting("wet_grade").abs().clamp(0.0, 1.0) } else { 0.0 },
            blur: if wet { brush.setting("wet_blur").clamp(0.0, 1.0) } else { 0.0 },
            wet_jitter: if wet { brush.setting("wet_jitter").clamp(0.0, 1.0) } else { 0.0 },
            blend: brush.setting("blend_mode").round().clamp(0.0, BRUSH_BEHIND as f32) as u32,
            blur_jitter: if wet { brush.setting("wet_blur_jitter").clamp(0.0, 1.0) } else { 0.0 },
            grain_blend: brush.setting("grain_blend").round().clamp(0.0, GRAIN_LINEAR_HEIGHT as f32) as u32,
            burnt_blend: brush.setting("burnt_blend").round().clamp(0.0, BRUSH_BEHIND as f32) as u32,
            grain_rotation: brush.setting("grain_rotation").clamp(-1.0, 1.0) * std::f32::consts::PI,
            grain_sharp: brush.setting("grain_filtering") < 0.5,
            shape_sharp: brush.setting("shape_filtering") < 0.5,
            secondary: [1.0, 1.0, 1.0],
            preview: false,
        }
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
    pub sigma: f32,
}

/// Draws a scene for [`Canvas::frost_over`] into the view given (cleared first), at the size
/// given.
pub type SmallScene<'s> = &'s mut dyn FnMut(&mut wgpu::CommandEncoder, &wgpu::TextureView, (u32, u32));

/// The backdrop's blur is this many texels wide (sigma) where it is worked out: the scene is
/// drawn that much smaller.
const BACKDROP_TEXELS: f32 = 6.0;

/// The scene drawn small for frosted panels, and the other half of its blur.
struct BackdropGpu {
    size: (u32, u32),
    format: wgpu::TextureFormat,
    views: [wgpu::TextureView; 2],
    binds: [wgpu::BindGroup; 2],
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
        let views = [view(), view()];
        let binds =
            [sampled_bind(device, layout, &views[0], sampler), sampled_bind(device, layout, &views[1], sampler)];
        BackdropGpu { size, format, views, binds }
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
    _pad: [u32; 3],
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
    /// With the stroke's look: its canvas grain, if it has one, and its edges and threshold.
    fn with_grain(self, style: Option<&StrokeStyle>) -> LayerUniform {
        let Some(s) = style else { return self };
        let look = LayerUniform {
            wet: if s.preview { -s.wet_edges } else { s.wet_edges },
            burnt: if s.preview { 0.0 } else { s.burnt_edges },
            threshold: s.alpha_threshold,
            grain_brightness: s.grain_brightness,
            grain_contrast: s.grain_contrast,
            stroke_blend: s.blend,
            grain_rotation: s.grain_rotation,
            grain_blend: s.grain_blend,
            burnt_blend: s.burnt_blend,
            grain_sharp: if s.grain_sharp { 1.0 } else { 0.0 },
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

/// Resolution of what a smudge brush carries, cells on a side.
const SMUDGE_GRID: u32 = 32;
const SMUDGE_UNIFORM_SIZE: u64 = std::mem::size_of::<SmudgeUniform>() as u64;

/// A wet stamp's (mixture, final mixture, pull) once the stroke has gone `length` canvas pixels:
/// the paint the brush mixes into what it carries is what its dilution leaves (by 2.4, and as
/// far as the fall off has not run it out) and its charge (running out by the cube root of the
/// length), at least its attack (the stamp's
/// `attack` share of it: its pressure), times `jitter`; it
/// lays down what it carries by the mixture, a share of it by the grade; the pull is the wet
/// pull's eighth root (a pull of 0.9 keeps 0.99 a stamp). The stamp's pressure scales the
/// dilution and the charge by `dilution_by` and `charge_by` (their curves).
fn wet_mixture(
    style: &StrokeStyle,
    length: f32,
    jitter: f32,
    attack: f32,
    dilution_by: f32,
    charge_by: f32,
) -> (f32, f32, f32) {
    let charge = if style.load > 0.0 { 1.0 - run_out(1.0 - style.load, length) } else { 0.0 } * charge_by;
    let diluted =
        (1.0 - run_out(style.falloff, length)) * (1.0 - style.dilution.clamp(0.0, 1.0) * dilution_by).powf(2.4);
    let attack = style.attack.clamp(0.0, 1.0) * attack.clamp(0.0, 1.0);
    let mixture = ((diluted + charge).max(attack) * jitter).clamp(0.0, 1.0);
    let pull = style.wet_pull.unwrap_or(0.0).clamp(0.0, 1.0).powf(0.125);
    // undiluted the brush only paints; otherwise over a stroke it lays down half to all of what
    // its grade says
    let grade = style.grade.clamp(0.0, 1.0);
    let laid =
        if style.dilution <= 0.0 || pull * mixture <= 0.0 { 1.0 } else { mixture * (grade + (1.0 - grade) * 0.5) };
    (mixture, laid, pull)
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
    first: u32,
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
    _pad: [f32; 4],
}

/// A smudge stroke in progress: the layer as a buffer the compute passes read and write, what the
/// brush carries, and the layer as it was before the stroke (for undo and cancel).
struct Smudge {
    layer: LayerId,
    work: wgpu::Buffer,
    carry: wgpu::Buffer,
    before: wgpu::Texture,
    started: bool,
    /// Stamps laid so far (for the wetness jitter).
    stamps: u32,
    /// How far the stroke has gone (canvas pixels, from stamp to stamp) and where its last stamp
    /// was: wet paint runs out with it.
    length: f32,
    last: Option<[f32; 2]>,
    /// Wet paint with a texturized grain: the stroke without the grain, and the layer as it was,
    /// the grain laid over the change once (see `grain_once`).
    raw: Option<wgpu::Buffer>,
    orig: Option<wgpu::Buffer>,
}

/// Whether a stroke's texturized grain is laid over its wet paint once, as it ends up, rather
/// than stamp by stamp (where it would wash out as the stamps build up).
fn grain_once(style: &StrokeStyle) -> bool {
    style.wet_pull.is_some()
        && style.tool == StrokeTool::Paint
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
}

enum Op {
    /// Stamps added to the stroke (`clear`: the stroke is redrawn from scratch).
    Stamps {
        instances: Vec<StampInstance>,
        /// Each stamp's wet-mix scaling (see [`Stamp::wet`]), for the wet paint drawn in place.
        wet: Vec<[f32; 8]>,
        style: StrokeStyle,
        clear: bool,
        layer: LayerId,
        /// Into the adjustment's mask instead of the stroke.
        pencil: bool,
    },
    Commit(ActiveStroke),
    Cancel,
    /// What the stroke has under this rectangle is put aside, for a tip drawn over it.
    SaveTip(IRect),
    /// What was put aside under this rectangle comes back (the tip drawn over it goes).
    Untip(IRect),
    Undo,
    Redo,
    Clear(LayerId),
    /// Clears a layer without an undo step (scratch canvases).
    Wipe(LayerId),
    /// The document changed structurally; `before` is how it was.
    Record(Box<Document>),
    /// Layers were added as copies: (from, to) pairs.
    Duplicate {
        pairs: Vec<(LayerId, LayerId)>,
        before: Box<Document>,
    },
    /// `upper` is blended into `lower` and removed from the document.
    MergeDown {
        upper: LayerId,
        lower: LayerId,
        before: Box<Document>,
    },
    /// `uppers` (bottom first; groups drawn first) are blended into `lower` in turn, as one step,
    /// and removed from the document.
    MergeMany {
        uppers: Vec<LayerId>,
        lower: LayerId,
        before: Box<Document>,
    },
    /// Group `group` of `before` is drawn into `into`: a new layer, or (`onto`) the layer it
    /// merges into.
    Flatten {
        group: LayerId,
        into: LayerId,
        onto: bool,
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
    /// Where the history is now is marked.
    Mark,
    /// The steps made since the mark become one.
    Collapse,
    /// Layers' pixels are replaced (premultiplied RGBA8, the canvas's size), together as one undo
    /// step, with the document as it was (`before`) when that changed too.
    Write {
        layers: Vec<(LayerId, Vec<u8>)>,
        before: Option<Box<Document>>,
    },
    /// A layer's pixels are replaced with no undo step (during an edit held with `Hold`).
    Put(LayerId, Vec<u8>),
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
    /// Liquify stamps drawn into the field.
    LiquifyStamps(Vec<LiquifyStamp>),
    /// The liquify field back to no offsets.
    LiquifyClear,
    /// The transform ends: kept as one undo step, or put back.
    TransformEnd {
        commit: bool,
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
    /// Each pixel streaked towards `centre` (canvas pixels) by `amount` of the way.
    Zoom { centre: [f32; 2], amount: f32 },
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiquifyMode {
    Push,
    TwirlRight,
    TwirlLeft,
    Pinch,
    Expand,
    Crystals,
    Edge,
    Reconstruct,
}

/// One liquify stamp: where (canvas pixels), how far the brush moved since the last one, its
/// radius and pressure (0..1), how much it scatters (distortion, 0..1), and a seed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquifyStamp {
    pub centre: [f32; 2],
    pub moved: [f32; 2],
    pub radius: f32,
    pub pressure: f32,
    pub distortion: f32,
    pub seed: f32,
    pub mode: LiquifyMode,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LiquifyUniform {
    stamp: [f32; 4],
    move_mode: [f32; 4],
    canvas: [f32; 4],
    pad: [f32; 4],
}

/// An adjustment painted on: how much of each pixel the brush has covered (the stroke format, its
/// red channel read).
struct PencilMask {
    view: wgpu::TextureView,
    /// Where it has been painted (canvas pixels): all the adjustment can have changed.
    bounds: IRect,
}

/// The liquify field: offsets (canvas pixels) a pixel shows the copy from, and that with the copy
/// for the adjustment's last pass.
struct LiquifyField {
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
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
    let mut out = Step { made, name, doc: None, pixels: Vec::new(), selection: None };
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
}

impl Step {
    fn bytes(&self) -> u64 {
        self.pixels.iter().map(Snapshot::bytes).sum()
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
    /// Adjustments: the copy alone, and the passes.
    before_layout: wgpu::BindGroupLayout,
    /// Adjustment passes by their fragment entry.
    filters: HashMap<&'static str, wgpu::RenderPipeline>,
    /// Liquify stamps added into the field, and reconstruct's fading of it.
    liquify_add: wgpu::RenderPipeline,
    liquify_fade: wgpu::RenderPipeline,
    selection_module: wgpu::ShaderModule,
    smudge_layout: wgpu::BindGroupLayout,
    smudge_carry: wgpu::ComputePipeline,
    smudge_deposit: wgpu::ComputePipeline,
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
    /// Ping-pong pair; the finished composite is always left in the first.
    composite: [wgpu::Texture; 2],
    composite_views: [wgpu::TextureView; 2],
    /// Where a group's layers are composited before the group goes onto the stack below: a pair
    /// for each depth of group (made when first needed).
    groups: Vec<([wgpu::Texture; 2], [wgpu::TextureView; 2])>,
    /// Animation Assist playing: the frame shown (alone, no onion skins).
    play_frame: Option<usize>,
    scratch: wgpu::Texture,
    scratch_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    grains_view: wgpu::TextureView,
    grain_sampler: wgpu::Sampler,
    shapes_view: wgpu::TextureView,
    shape_sampler: wgpu::Sampler,
    /// The shape and grain arrays, and which images of the user's own their spare layers hold.
    shapes: wgpu::Texture,
    grains: wgpu::Texture,
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
    display_bind: wgpu::BindGroup,
    /// The table that leaves colours as they are, bound when a target has none.
    identity_lut: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    uniform_bind: wgpu::BindGroup,
    uniform_slots: u64,
    stamp_buffer: wgpu::Buffer,
    stamp_capacity: u64,
    /// How many times the composite has been redrawn.
    composites: u64,
    active: Option<ActiveStroke>,
    smudge: Option<Smudge>,
    /// A layer's pixels set aside by [`Canvas::hold_layer`].
    held: Option<Snapshot>,
    /// Counts the undo steps ever made, so that a change since some moment shows.
    steps_made: u64,
    /// The steps made when [`Canvas::mark_steps`] was last done.
    mark: Option<u64>,
    /// The name the next step made takes.
    next_name: Option<String>,
    smudge_params: wgpu::Buffer,
    smudge_capacity: u64,
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
    /// Layers' pixels kept compressed off the GPU: those parked (no texture), and copies of
    /// resident ones unchanged since, so that letting them go again costs nothing.
    parked: HashMap<LayerId, Parked>,
    /// Small pictures of the parked layers, so their thumbnails need no texture.
    pictures: HashMap<LayerId, Pictures>,
    /// Layers whose pixels changed since [`Canvas::take_changed`] took them, so a save reads only
    /// those.
    changed: HashSet<LayerId>,
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

impl Canvas {
    pub fn new(gpu: &Gpu, doc: &Document) -> Result<Canvas, GpuError> {
        let (width, height) = (doc.width(), doc.height());
        let max = gpu.max_texture_size();
        if width > max || height > max {
            return Err(GpuError::TooLarge(width, height, max));
        }
        let device = &gpu.device;
        let pipelines = gpu.canvas_pipelines();
        let stroke_texture = texture(device, "stroke", width, height, STROKE_FORMAT);
        let stroke_view = stroke_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let composite = [
            texture(device, "composite", width, height, LAYER_FORMAT),
            texture(device, "composite 2", width, height, LAYER_FORMAT),
        ];
        let composite_views = [
            composite[0].create_view(&wgpu::TextureViewDescriptor::default()),
            composite[1].create_view(&wgpu::TextureViewDescriptor::default()),
        ];
        let scratch = texture(device, "scratch", width, height, LAYER_FORMAT);
        let scratch_view = scratch.create_view(&wgpu::TextureViewDescriptor::default());
        let array_view =
            wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() };
        let (shape_images, grain_images) = (textures::shapes(), textures::grains());
        let shapes = brush_images(gpu, "shapes", &shape_images);
        let grains = brush_images(gpu, "grains", &grain_images);
        let shapes_view = shapes.create_view(&array_view);
        let grains_view = grains.create_view(&array_view);
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
        let stroke_bind = quad_bind(device, &pipelines.quad_layout, [&stroke_view; 5], &grains_view, &grain_sampler);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let display_bind = sampled_bind(device, &pipelines.sampled_layout, &composite_views[0], &sampler);
        // two points on a side: each corner its own colour
        let corners: Vec<u8> = (0..8u8).flat_map(|i| [(i >> 2) & 1, (i >> 1) & 1, i & 1, 1].map(|c| c * 255)).collect();
        let identity_lut = lut_bind(gpu, &pipelines.lut_layout, 2, &corners);
        let uniform_slots = 64;
        let (uniform_buffer, uniform_bind) = uniforms(device, &pipelines.uniform_layout, uniform_slots);
        let stamp_capacity = 4096;
        let stamp_buffer = stamp_buffer(device, stamp_capacity);
        let mut canvas = Canvas {
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
            composite_views,
            groups: Vec::new(),
            play_frame: None,
            scratch,
            scratch_view,
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
            identity_lut,
            uniform_buffer,
            uniform_bind,
            uniform_slots,
            stamp_buffer,
            stamp_capacity,
            composites: 0,
            active: None,
            smudge: None,
            held: None,
            steps_made: 0,
            mark: None,
            next_name: None,
            smudge_params: smudge_params(device, 64),
            smudge_spare: [spare_buffer(device), spare_buffer(device)],
            smudge_capacity: 64,
            prediction: Vec::new(),
            ops: Vec::new(),
            cache_composites: false,
            caches: None,
            remake: None,
            parked: HashMap::new(),
            pictures: HashMap::new(),
            changed: HashSet::new(),
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
            shape_sources: SourceSlots::after(shape_images.len()),
            grain_sources: SourceSlots::after(grain_images.len()),
        };
        canvas.sync_layers(gpu, doc);
        Ok(canvas)
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Points `style` at `brush`'s own shape and grain images, if it has them, putting them in
    /// the arrays' spare layers first if they aren't there.
    pub fn use_sources(&mut self, gpu: &Gpu, style: &mut StrokeStyle, brush: &Brush) {
        if let Some(source) = brush.shape.source.as_ref() {
            style.shape = Some(self.shape_sources.layer(gpu, &self.shapes, source));
        }
        if let Some(source) = brush.grain.source.as_ref() {
            style.grain = Some(self.grain_sources.layer(gpu, &self.grains, source));
        }
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
                        | Op::Clear(_)
                        | Op::Record(_)
                        | Op::Duplicate { .. }
                        | Op::MergeDown { .. }
                        | Op::MergeMany { .. }
                        | Op::Flatten { .. }
                        | Op::TransformEnd { commit: true }
                        | Op::Write { .. }
                        | Op::Release { keep: true, .. }
                )
            })
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
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
        self.active = Some(ActiveStroke { layer, style, bounds: IRect::EMPTY, pencil: false });
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
        &self.composite_views[0]
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
    /// stroke texture, not painted in place or onto an adjustment.
    pub fn shows_tip(&self) -> bool {
        self.active.is_some_and(|a| !a.pencil && !a.style.in_place())
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
        let full = self.full();
        let Some(active) = self.active.as_mut() else { return IRect::EMPTY };
        let mut bounds = IRect::EMPTY;
        let style = active.style;
        let capped = self.pipelines.stamp_capped.is_some();
        let drawn: Vec<&Stamp> =
            stamps.iter().filter(|s| s.center.is_finite() && s.radius.is_finite() && s.alpha.is_finite()).collect();
        let wet: Vec<[f32; 8]> =
            drawn.iter().map(|s| s.wet.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 })).collect();
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
                    look: [s.grain_depth, s.grain_size, s.secondary, s.gradation]
                        .map(|v| if v.is_finite() { v.clamp(0.0, 64.0) } else { 0.0 }),
                    more: [s.grain_press, s.grain_bleed, transfer, s.attack]
                        .map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }),
                }
            })
            .collect();
        let bounds = bounds.intersect(full);
        if clear {
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
                style: active.style,
                clear: clear && !pencil,
                layer: active.layer,
                pencil,
            });
        }
        bounds
    }

    /// Puts the open stroke into its layer (undoably).
    /// Stamps for where the input is predicted to go next: drawn over the canvas until replaced,
    /// never into the layer. Only painting strokes show them.
    pub fn set_prediction(&mut self, stamps: &[Stamp]) {
        self.prediction.clear();
        let Some(style) = self.active.map(|a| a.style).filter(|s| s.tool == StrokeTool::Paint) else { return };
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
                    look: [s.grain_depth, s.grain_size, s.secondary, s.gradation]
                        .map(|v| if v.is_finite() { v.clamp(0.0, 64.0) } else { 0.0 }),
                    more: [s.grain_press, s.grain_bleed, transfer, s.attack]
                        .map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }),
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
        self.ops.push(Op::Clear(layer));
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

    /// Replaces several layers' pixels at once, as one undo step; pictures of the wrong size are
    /// left out.
    pub fn write_layers(&mut self, layers: Vec<(LayerId, Vec<u8>)>) {
        let size = self.width as usize * self.height as usize * 4;
        let layers: Vec<(LayerId, Vec<u8>)> = layers.into_iter().filter(|(_, p)| p.len() == size).collect();
        if !layers.is_empty() {
            self.end_stroke();
            self.ops.push(Op::Write { layers, before: None });
        }
    }

    /// Replaces several layers' pixels and changes the document together, as one undo step back
    /// to the pixels and to `before` (the document as it was); pictures of the wrong size are left
    /// out.
    pub fn write_layers_and_document(&mut self, layers: Vec<(LayerId, Vec<u8>)>, before: Document) {
        let size = self.width as usize * self.height as usize * 4;
        let layers: Vec<(LayerId, Vec<u8>)> = layers.into_iter().filter(|(_, p)| p.len() == size).collect();
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
    /// queued (`before` is the document with it).
    pub fn merge_down(&mut self, upper: LayerId, lower: LayerId, before: Document) {
        self.end_stroke();
        self.ops.push(Op::MergeDown { upper, lower, before: Box::new(before) });
        self.invalidate();
    }

    /// Blends `uppers` (bottom first: layers, or groups composited first) into `lower` in turn as
    /// one undo step; the caller removes them from the document once this is queued (`before` is
    /// the document with them).
    pub fn merge_layers(&mut self, uppers: Vec<LayerId>, lower: LayerId, before: Document) {
        self.end_stroke();
        self.ops.push(Op::MergeMany { uppers, lower, before: Box::new(before) });
        self.invalidate();
    }

    /// Draws group `group` into `into`: a new layer (`onto` false, already in the document) or the
    /// layer it merges into, blended with the group's opacity and mode. `before` is the document
    /// with the group; the caller changes the document once this is queued.
    pub fn flatten_group(&mut self, group: LayerId, into: LayerId, onto: bool, before: Document) {
        self.end_stroke();
        self.ops.push(Op::Flatten { group, into, onto, before: Box::new(before) });
        self.invalidate();
    }

    /// Recomposite everything next frame (after a change to visibility, opacity, order...).
    /// Whether the composite is made around pictures of the top level's items that aren't being
    /// worked on, kept until something changes them, so that only the items worked on composite
    /// each time (and the others' layers needn't be at hand): for artworks of more layers than
    /// fit at once. Off, every layer composites every time.
    pub fn set_cache_composites(&mut self, on: bool) {
        self.cache_composites = on;
        self.caches = None;
        self.remake = None;
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

    /// Whether the composite is being made around pictures of what isn't worked on.
    pub fn composites_cached(&self) -> bool {
        self.caches.is_some()
    }

    /// What the layers' textures may take, in bytes: past it, the layers needed least lately
    /// that aren't being worked on are parked (kept compressed off the GPU, and the composite made
    /// around pictures of them) until needed again. 0 keeps every layer on the GPU.
    pub fn set_resident_budget(&mut self, bytes: u64) {
        self.resident_budget = bytes;
        self.set_cache_composites(bytes > 0);
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
            gpu.queue.write_texture(
                copy_at(&texture, b.x0, b.y0),
                &rows,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(b.width() * 4),
                    rows_per_image: Some(b.height()),
                },
                extent(b.width(), b.height()),
            );
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
                self.changed.insert(id);
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
                Op::Clear(id)
                | Op::Wipe(id)
                | Op::TransformBegin(id)
                | Op::TransformAlso(id)
                | Op::Put(id, _)
                | Op::Hold(id) => written.push(*id),
                Op::Write { layers, .. } => written.extend(layers.iter().map(|(id, _)| *id)),
                Op::Duplicate { pairs, .. } => {
                    read.extend(pairs.iter().map(|(from, _)| *from));
                    written.extend(pairs.iter().map(|(_, to)| *to));
                }
                Op::MergeDown { upper, lower, before } => {
                    read.extend(contents(before, *upper));
                    written.push(*lower);
                }
                Op::MergeMany { uppers, lower, before } => {
                    read.extend(uppers.iter().flat_map(|u| contents(before, *u)));
                    written.push(*lower);
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
        let per = u64::from(self.width) * u64::from(self.height) * 4;
        let most = usize::try_from(self.resident_budget / per.max(1)).unwrap_or(usize::MAX);
        if self.resident_budget == 0 || self.layers.len() <= most {
            return;
        }
        let mut keep = match (&self.caches, self.shown_frames(doc)) {
            (Some(c), _) => {
                let mut keep = c.live_layers.clone();
                keep.extend(c.live.first().and_then(|first| doc.clip_base(*first)));
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
        for id in gone {
            if self.layers.len() <= most {
                break;
            }
            if !self.parked.contains_key(&id) {
                let Ok(pixels) = self.read_layer(gpu, id) else { continue };
                let pictures = Pictures::of(&pixels, self.width, self.height);
                self.parked.insert(id, Parked::of(&pixels, self.width, pictures.bounds));
                self.pictures.insert(id, pictures);
            }
            self.layers.remove(&id);
        }
    }

    pub fn invalidate(&mut self) {
        self.caches = None;
        self.remake = None;
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
        self.put_selection(&gpu.device, &gpu.queue, coverage.map(<[u8]>::to_vec));
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
        self.push_step(Step { made: 0, name: None, doc: None, pixels: Vec::new(), selection: Some(before) });
        self.set_selection(gpu, coverage);
    }

    /// The selection's coverage, one byte a canvas pixel, or none when nothing is selected.
    pub fn selection(&self) -> Option<&[u8]> {
        self.selection.as_ref().map(|s| s.coverage.as_slice())
    }

    fn put_selection(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, coverage: Option<Vec<u8>>) {
        let size = self.width as usize * self.height as usize;
        self.selection = coverage.filter(|c| c.len() == size).map(|coverage| {
            let view = coverage_texture(device, queue, self.width, self.height, &coverage)
                .create_view(&wgpu::TextureViewDescriptor::default());
            let bind = sampled_bind(device, &self.pipelines.sampled_layout, &view, &self.sampler);
            SelectionGpu { coverage, view, bind }
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
        self.active = Some(ActiveStroke { layer, style, bounds: IRect::EMPTY, pencil: true });
    }

    /// Liquify stamps drawn into the field, in order.
    pub fn liquify_stamps(&mut self, stamps: &[LiquifyStamp]) {
        if !stamps.is_empty() {
            self.ops.push(Op::LiquifyStamps(stamps.to_vec()));
        }
    }

    /// The liquify field back to no offsets.
    pub fn liquify_clear(&mut self) {
        self.ops.push(Op::LiquifyClear);
    }

    /// Layer `id` copied aside to be transformed (or adjusted) from.
    fn copy_aside(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        id: LayerId,
    ) -> Option<Transforming> {
        let layer = self.layers.get(&id)?;
        let before = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("transform source"),
            size: extent(self.width, self.height),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: LAYER_FORMAT,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
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
        if let Some(Op::TransformSet { .. } | Op::TransformMesh { .. } | Op::FilterSet(_)) = self.ops.last() {
            self.ops.pop();
        }
        self.ops.push(op);
    }

    /// Ends the transform: `commit` keeps it as one undo step, otherwise the layer is put back.
    pub fn end_transform(&mut self, commit: bool) {
        self.ops.push(Op::TransformEnd { commit });
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
                | Op::Flatten { before: d, .. } => ids.extend(d.pixel_layer_ids()),
                Op::Commit(s) => ids.push(s.layer),
                Op::Clear(id) | Op::Wipe(id) | Op::TransformBegin(id) | Op::TransformAlso(id) => ids.push(*id),
                Op::Write { layers, .. } => ids.extend(layers.iter().map(|(id, _)| *id)),
                Op::Put(id, _) | Op::Hold(id) => ids.push(*id),
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
        for id in ids {
            if self.layers.contains_key(&id) || self.parked.contains_key(&id) {
                continue;
            }
            let texture = texture(&gpu.device, "layer", self.width, self.height, LAYER_FORMAT);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            // new textures are zeroed: transparent
            self.layers.insert(id, LayerGpu { texture, view });
            added = true;
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
            | Op::Flatten { before: d, .. }
            | Op::Release { before: Some(d), .. }
            | Op::Write { before: Some(d), .. } = op
            {
                keep.extend(d.layer_ids());
            }
        }
        self.layers.retain(|id, _| keep.contains(id));
        self.parked.retain(|id, _| keep.contains(id));
        self.pictures.retain(|id, _| keep.contains(id));
        self.needed_at.retain(|id, _| keep.contains(id));
    }

    /// Records and submits everything queued, recomposites what changed and draws to `target`.
    /// Undoing a structural step puts the earlier document into `doc`.
    pub fn render(&mut self, gpu: &Gpu, doc: &mut Document, target: Option<Target<'_>>) {
        self.sync_layers(gpu, doc);
        let device = &gpu.device;
        let flattening = self.ops.iter().filter_map(|op| match op {
            Op::Flatten { before, .. } => Some(before.max_depth()),
            _ => None,
        });
        let depth = flattening.max().unwrap_or(0);
        self.make_groups(device, depth);
        let ops = std::mem::take(&mut self.ops);

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
        let frosted = target.as_ref().and_then(|t| t.backdrop).map_or(0, |b| b.regions.len() + 3);
        // remaking the caches composites the layers once more
        let cached = if self.cache_composites { doc.layers().len() + 2 } else { 0 };
        let slots = (ops.len() + doc.layers().len() + 4 + frosted + cached) as u64;
        if slots > self.uniform_slots {
            self.uniform_slots = slots.next_power_of_two();
            let (buffer, bind) = uniforms(device, &self.pipelines.uniform_layout, self.uniform_slots);
            self.uniform_buffer = buffer;
            self.uniform_bind = bind;
        }
        let smudge_count: u64 = ops
            .iter()
            .map(|op| match op {
                Op::Stamps { instances, style, .. } if style.in_place() => instances.len() as u64,
                _ => 0,
            })
            .sum();
        if smudge_count > self.smudge_capacity {
            self.smudge_capacity = smudge_count.next_power_of_two();
            self.smudge_params = smudge_params(device, self.smudge_capacity);
        }
        let mut uniforms = UniformData::default();
        let mut smudge_uniforms = UniformData::default();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("canvas") });

        let mut next_instance = 0u32;
        let mut history_changed = false;
        // what is worked on, for the caches: other work drops them
        let live = self.caches.as_ref().or(self.remake.as_ref().map(|r| &r.caches)).map(|c| c.live_layers.clone());
        for op in ops {
            if live.as_ref().is_some_and(|live| !self.keeps_caches(&op, live)) {
                self.caches = None;
                self.remake = None;
            }
            match op {
                Op::Stamps { instances, wet, style, layer, clear, .. } if style.in_place() => {
                    next_instance = next_instance.saturating_add(u32::try_from(instances.len()).unwrap_or(u32::MAX));
                    let stamps = (instances.as_slice(), wet.as_slice());
                    self.smudge_stamps(device, &mut encoder, &mut smudge_uniforms, stamps, &style, layer, clear);
                }
                Op::Stamps { instances, style, clear, pencil, .. } => {
                    let count = u32::try_from(instances.len()).unwrap_or(u32::MAX);
                    let first = next_instance;
                    next_instance = next_instance.saturating_add(count);
                    let (grain, grain_scale, grain_depth) =
                        style.moving_grain().map_or((-1, 1.0, 0.0), |(g, s, d)| (g as i32, s, d));
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
                        _pad: [0; 3],
                        secondary: [style.secondary[0], style.secondary[1], style.secondary[2], 1.0],
                    });
                    let load = if clear { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) } else { wgpu::LoadOp::Load };
                    let target = match (&self.pencil, pencil) {
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
                        pass.set_bind_group(1, &self.brush_bind, &[]);
                        let start = u64::from(first) * size_of::<StampInstance>() as u64;
                        let end = u64::from(next_instance) * size_of::<StampInstance>() as u64;
                        pass.set_vertex_buffer(0, self.stamp_buffer.slice(start..end));
                        pass.draw(0..6, 0..count);
                    }
                }
                Op::Commit(stroke) if stroke.style.in_place() => {
                    self.finish_smudge(device, &mut encoder, stroke.bounds, false);
                    history_changed = true;
                }
                // painting an adjustment on leaves the layer to the adjustment
                Op::Commit(stroke) if stroke.pencil => {
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
                Op::Mark => self.mark = Some(self.steps_made),
                Op::Name(name) => self.next_name = Some(name),
                Op::Collapse => {
                    let since = self.mark.take().unwrap_or(u64::MAX);
                    let first = self.undo.iter().position(|s| s.made > since).unwrap_or(self.undo.len());
                    if self.undo.len() - first >= 2 {
                        let steps = self.undo.split_off(first);
                        self.undo.push(collapsed(steps));
                        history_changed = true;
                    }
                }
                Op::Clear(id) => {
                    let full = self.full();
                    if let Some(snapshot) = self.snapshot(device, &mut encoder, id, full) {
                        self.push_step(Step {
                            made: 0,
                            name: None,
                            doc: None,
                            pixels: vec![snapshot],
                            selection: None,
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
                        let staging = texture(device, "layer put", self.width, self.height, LAYER_FORMAT);
                        gpu.queue.write_texture(
                            copy_at(&staging, 0, 0),
                            &pixels,
                            wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(self.width * 4),
                                rows_per_image: Some(self.height),
                            },
                            extent(self.width, self.height),
                        );
                        encoder.copy_texture_to_texture(
                            copy_at(&staging, 0, 0),
                            copy_at(&layer.texture, 0, 0),
                            extent(self.width, self.height),
                        );
                    }
                    self.dirty = self.full();
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
                Op::Write { layers, before } => {
                    let full = self.full();
                    let snapshots: Vec<Snapshot> =
                        layers.iter().filter_map(|(id, _)| self.snapshot(device, &mut encoder, *id, full)).collect();
                    if !snapshots.is_empty() || before.is_some() {
                        self.push_step(Step { made: 0, name: None, doc: before, pixels: snapshots, selection: None });
                    }
                    for (id, pixels) in layers {
                        let Some(layer) = self.layers.get(&id) else { continue };
                        // through a staging texture, so the snapshot above is taken first
                        let staging = texture(device, "layer write", self.width, self.height, LAYER_FORMAT);
                        gpu.queue.write_texture(
                            copy_at(&staging, 0, 0),
                            &pixels,
                            wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(self.width * 4),
                                rows_per_image: Some(self.height),
                            },
                            extent(self.width, self.height),
                        );
                        encoder.copy_texture_to_texture(
                            copy_at(&staging, 0, 0),
                            copy_at(&layer.texture, 0, 0),
                            extent(self.width, self.height),
                        );
                    }
                    self.dirty = full;
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
                Op::LiquifyBegin => {
                    let field = texture(device, "liquify field", self.width, self.height, LIQUIFY_FORMAT);
                    let view = field.create_view(&wgpu::TextureViewDescriptor::default());
                    drop(render_pass(
                        &mut encoder,
                        "liquify clear",
                        &view,
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    ));
                    self.liquify = Some(LiquifyField { bind: self.copy_bind_with(device, &view), view });
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
                Op::LiquifyStamps(stamps) => {
                    if let Some(field) = &self.liquify {
                        let (w, h) = (self.width as f32, self.height as f32);
                        let mut pass = render_pass(&mut encoder, "liquify", &field.view, wgpu::LoadOp::Load);
                        for st in stamps {
                            let mode = st.mode as u32 as f32;
                            let offset = uniforms.push(&LiquifyUniform {
                                stamp: [st.centre[0], st.centre[1], st.radius.max(1.0), st.pressure],
                                move_mode: [st.moved[0], st.moved[1], mode, st.seed],
                                canvas: [w, h, st.distortion, 0.0],
                                pad: [0.0; 4],
                            });
                            let pipeline = if st.mode == LiquifyMode::Reconstruct {
                                &self.pipelines.liquify_fade
                            } else {
                                &self.pipelines.liquify_add
                            };
                            pass.set_pipeline(pipeline);
                            pass.set_bind_group(0, &self.uniform_bind, &[offset]);
                            pass.draw(0..6, 0..1);
                        }
                    }
                }
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
                Op::TransformEnd { commit } => {
                    self.liquify = None;
                    let painted = self.pencil.take().map(|p| p.bounds.intersect(self.full()));
                    let more = std::mem::take(&mut self.transform_more);
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
                                });
                                history_changed = true;
                            }
                        } else if commit {
                            let full = self.full();
                            let pixels = std::iter::once(t)
                                .chain(more)
                                .map(|t| Snapshot { layer: t.layer, rect: full, texture: t.before })
                                .collect();
                            self.push_step(Step { made: 0, name: None, doc: None, pixels, selection: None });
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
                    });
                    self.dirty = self.full();
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
                    });
                    self.dirty = self.full();
                    history_changed = true;
                }
                Op::MergeDown { upper, lower, before } => {
                    let pixels =
                        self.merge(device, &mut encoder, &mut uniforms, &before, upper, lower).into_iter().collect();
                    self.push_step(Step { made: 0, name: None, doc: Some(before), pixels, selection: None });
                    self.dirty = self.full();
                    history_changed = true;
                }
                Op::MergeMany { uppers, lower, before } => {
                    let full = self.full();
                    let snapshot = self.snapshot(device, &mut encoder, lower, full);
                    for upper in uppers {
                        if before.layer(upper).is_some_and(|l| l.kind == LayerKind::Group) {
                            self.draw_group_onto(device, &mut encoder, &mut uniforms, &before, upper, lower);
                        } else {
                            self.blend_layer_into(device, &mut encoder, &mut uniforms, &before, upper, lower);
                        }
                    }
                    let pixels = snapshot.into_iter().collect();
                    self.push_step(Step { made: 0, name: None, doc: Some(before), pixels, selection: None });
                    self.dirty = self.full();
                    history_changed = true;
                }
                Op::Flatten { group, into, onto, before } => {
                    let pixels = self
                        .flatten(device, &mut encoder, &mut uniforms, &before, group, into, onto)
                        .into_iter()
                        .collect();
                    self.push_step(Step { made: 0, name: None, doc: Some(before), pixels, selection: None });
                    self.dirty = self.full();
                    history_changed = true;
                }
            }
        }
        if history_changed {
            self.collect_layers(doc);
        }

        if !smudge_uniforms.data.is_empty() {
            gpu.queue.write_buffer(&self.smudge_params, 0, &smudge_uniforms.data);
        }
        self.make_groups(device, doc.max_depth());
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
            self.display(device, &mut encoder, &mut uniforms, &target, predicted);
            if let Some(backdrop) = target.backdrop.filter(|b| !b.regions.is_empty() && b.sigma > 0.0) {
                self.frost(device, &mut encoder, &mut uniforms, &target, backdrop, None);
            }
        }
        if !uniforms.data.is_empty() {
            gpu.queue.write_buffer(&self.uniform_buffer, 0, &uniforms.data);
        }
        gpu.queue.submit([encoder.finish()]);
        self.renders = self.renders.wrapping_add(1);
        self.park_extra(gpu, doc);
    }

    /// Bytes per row of a layer copied into a buffer (rows are 256-byte aligned for copies).
    fn row_bytes(&self) -> u32 {
        (self.width * 4).div_ceil(256) * 256
    }

    /// Smudges `instances` into the stroke's layer: per stamp, the brush picks up colour and lays
    /// down what it carries; then the changed pixels go back into the layer texture.
    fn smudge_stamps(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        params: &mut UniformData,
        (instances, wet): (&[StampInstance], &[[f32; 8]]),
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
            smudge.stamps = 0;
            smudge.length = 0.0;
            smudge.last = None;
            if let Some(raw) = &smudge.raw {
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
            let Some(layer) = self.layers.get(&target) else { return };
            let work = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("smudge layer"),
                size: u64::from(row) * u64::from(self.height),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let carry = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("smudge carry"),
                size: u64::from(SMUDGE_GRID * SMUDGE_GRID) * 16,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            let before = texture(device, "smudge before", self.width, self.height, LAYER_FORMAT);
            encoder.copy_texture_to_texture(
                copy_at(&layer.texture, 0, 0),
                copy_at(&before, 0, 0),
                extent(self.width, self.height),
            );
            encoder.copy_texture_to_buffer(
                copy_at(&layer.texture, 0, 0),
                wgpu::TexelCopyBufferInfo {
                    buffer: &work,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: Some(self.height),
                    },
                },
                extent(self.width, self.height),
            );
            // wet paint with a texturized grain keeps the stroke without it, and the layer as it was
            let (raw, orig) = if grain_once(style) {
                let mut copy = |label| {
                    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(label),
                        size: u64::from(row) * u64::from(self.height),
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    encoder.copy_texture_to_buffer(
                        copy_at(&layer.texture, 0, 0),
                        wgpu::TexelCopyBufferInfo {
                            buffer: &buffer,
                            layout: wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(row),
                                rows_per_image: Some(self.height),
                            },
                        },
                        extent(self.width, self.height),
                    );
                    buffer
                };
                (Some(copy("smudge raw")), Some(copy("smudge orig")))
            } else {
                (None, None)
            };
            self.smudge = Some(Smudge {
                layer: target,
                work,
                carry,
                before,
                started: false,
                stamps: 0,
                length: 0.0,
                last: None,
                raw,
                orig,
            });
        }
        let Some(smudge) = self.smudge.as_mut() else { return };
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("smudge"),
            layout: &self.pipelines.smudge_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.smudge_params,
                        offset: 0,
                        size: NonZeroU64::new(SMUDGE_UNIFORM_SIZE),
                    }),
                },
                wgpu::BindGroupEntry { binding: 1, resource: smudge.work.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: smudge.carry.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: smudge.raw.as_ref().unwrap_or(&self.smudge_spare[0]).as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: smudge.orig.as_ref().unwrap_or(&self.smudge_spare[1]).as_entire_binding(),
                },
            ],
        });
        let full = IRect::from_size(self.width, self.height);
        let mut changed = IRect::EMPTY;
        let mut pass =
            encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("smudge"), timestamp_writes: None });
        pass.set_bind_group(1, &self.brush_bind, &[]);
        for (k, s) in instances.iter().enumerate() {
            let center = procraft_engine::geom::Vec2::new(s.center[0], s.center[1]);
            let bbox =
                IRect::around(center, s.radius * s.roundness.max(1.0) * std::f32::consts::SQRT_2).intersect(full);
            if bbox.is_empty() || s.radius <= 0.0 {
                continue;
            }
            changed = changed.union(bbox);
            let wet_paint = style.wet_pull.is_some() && style.tool == StrokeTool::Paint;
            // the paint runs out with the length the stroke has gone
            if let Some([x, y]) = smudge.last {
                smudge.length += (s.center[0] - x).hypot(s.center[1] - y);
            }
            smudge.last = Some(s.center);
            // the grade, blur and jitters as far as this stamp's pressure takes them
            let [grade_by, blur_by, blur_jitter_by, wet_jitter_by, charge_by, dilution_by, wet_pull_by, smudge_pull_by] =
                wet.get(k).copied().unwrap_or([1.0; 8]);
            let stamp_style = StrokeStyle { grade: style.grade * grade_by, ..*style };
            // the mixture varies from stamp to stamp by the wetness jitter
            let jitter = 1.0 - style.wet_jitter * wet_jitter_by * unit_hash(smudge.stamps);
            smudge.stamps = smudge.stamps.saturating_add(1);
            // the pull as far as the stamp's pressure takes it (the wet pull's or the smudge's)
            let (mixture, final_mixture, wet_pull) = if wet_paint {
                let (mixture, laid, pull) =
                    wet_mixture(&stamp_style, smudge.length, jitter, s.more[3], dilution_by, charge_by);
                (mixture, laid, pull * wet_pull_by)
            } else {
                (0.0, 1.0, style.pull.clamp(0.0, 1.0) * smudge_pull_by)
            };
            let moving = style.grain_mode == GrainMode::Moving;
            let offset = params.push(&SmudgeUniform {
                center: s.center,
                radius: s.radius.max(0.5),
                angle: s.angle,
                roundness: s.roundness,
                alpha: (s.alpha * style.opacity).clamp(0.0, 1.0),
                pull: wet_pull,
                hardness: style.hardness.clamp(0.0, 1.0),
                shape: style.shape.map_or(-1, |s| s as i32),
                first: u32::from(!smudge.started),
                preserve_alpha: u32::from(style.preserve_alpha),
                stride: row / 4,
                bbox: [bbox.x0, bbox.y0, bbox.x1, bbox.y1],
                size: [self.width, self.height],
                grid: SMUDGE_GRID,
                uniform_glaze: u32::from(style.rendering == Rendering::Strongest),
                paint: [style.color[0], style.color[1], style.color[2], 1.0],
                mode: u32::from(wet_paint),
                mixture,
                final_mixture,
                grade: stamp_style.grade,
                blur: style.blur
                    * blur_by
                    * (1.0 - style.blur_jitter * blur_jitter_by * unit_hash(smudge.stamps.wrapping_add(7919))),
                grain: style.grain.map_or(-1, |g| g as i32),
                // a moving grain sized and as deep as this stamp says
                grain_scale: if moving { -style.grain_scale * s.look[1].max(0.05) } else { style.grain_scale },
                grain_depth: if moving { (style.grain_depth * s.look[0]).clamp(0.0, 1.0) } else { style.grain_depth },
                grain_offset: s.grain_offset,
                grain_rotation: style.grain_rotation,
                grain_sharp: if style.grain_sharp { 1.0 } else { 0.0 },
                shape_sharp: if style.shape_sharp { 1.0 } else { 0.0 },
                grain_press: s.more[0],
                grain_bleed: s.more[1],
                grain_once: if smudge.raw.is_some() { 1.0 } else { 0.0 },
                _pad: [0.0; 4],
            });
            smudge.started = true;
            pass.set_bind_group(0, &bind, &[offset]);
            pass.set_pipeline(&self.pipelines.smudge_carry);
            pass.dispatch_workgroups(SMUDGE_GRID.div_ceil(8), SMUDGE_GRID.div_ceil(8), 1);
            pass.set_pipeline(&self.pipelines.smudge_deposit);
            pass.dispatch_workgroups(bbox.width().div_ceil(8), bbox.height().div_ceil(8), 1);
        }
        drop(pass);
        if changed.is_empty() {
            return;
        }
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
        if !rect.is_empty() && self.layers.contains_key(&stroke.layer) {
            if let Some(snapshot) = self.snapshot(device, encoder, stroke.layer, rect) {
                self.push_step(Step { made: 0, name: None, doc: None, pixels: vec![snapshot], selection: None });
            }
            let opacity = stroke.style.opacity.clamp(0.0, 1.0);
            let locked = stroke.style.preserve_alpha;
            let burns = stroke.style.burnt_edges > 0.0 && !stroke.style.preview;
            match (stroke.style.tool, locked || stroke.style.blend != 0 || burns) {
                (StrokeTool::Paint, true) => {
                    // recolour where the layer has alpha, or blend by a mode or burn edges (which
                    // read the layer): into scratch, then back
                    let offset = uniforms.push(
                        &LayerUniform {
                            opacity: 1.0,
                            stroke_mode: if locked { STROKE_PAINT_LOCKED } else { STROKE_PAINT },
                            stroke_opacity: opacity,
                            ..Default::default()
                        }
                        .with_grain(Some(&stroke.style)),
                    );
                    if let Some(layer) = self.layers.get(&stroke.layer) {
                        let bind =
                            self.quad(device, [&layer.view, &self.stroke_view, &self.stroke_view, &self.stroke_view]);
                        let mut pass = render_pass(encoder, "commit locked", &self.scratch_view, wgpu::LoadOp::Load);
                        pass.set_pipeline(&self.pipelines.commit_locked);
                        scissor(&mut pass, rect);
                        pass.set_bind_group(0, &bind, &[]);
                        pass.set_bind_group(1, &self.uniform_bind, &[offset]);
                        pass.draw(0..3, 0..1);
                        drop(pass);
                        encoder.copy_texture_to_texture(
                            copy_at(&self.scratch, rect.x0, rect.y0),
                            copy_at(&layer.texture, rect.x0, rect.y0),
                            extent(rect.width(), rect.height()),
                        );
                    }
                }
                (tool, _) => {
                    let offset = uniforms.push(
                        &LayerUniform { opacity: 1.0, stroke_opacity: opacity, ..Default::default() }
                            .with_grain(Some(&stroke.style)),
                    );
                    if let Some(layer) = self.layers.get(&stroke.layer) {
                        let mut pass = render_pass(encoder, "commit", &layer.view, wgpu::LoadOp::Load);
                        pass.set_pipeline(match tool {
                            StrokeTool::Paint | StrokeTool::Smudge => &self.pipelines.commit_paint,
                            StrokeTool::Erase => &self.pipelines.commit_erase,
                        });
                        scissor(&mut pass, rect);
                        pass.set_bind_group(0, &self.stroke_bind, &[]);
                        pass.set_bind_group(1, &self.uniform_bind, &[offset]);
                        pass.draw(0..3, 0..1);
                    }
                }
            }
        }
        drop(render_pass(encoder, "stroke clear", &self.stroke_view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)));
        self.dirty = self.dirty.union(rect);
    }

    /// Blends `upper` into `lower` with its mode, opacity and clipping (as `doc` has them).
    /// Returns `lower`'s previous pixels.
    fn merge(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        doc: &Document,
        upper: LayerId,
        lower: LayerId,
    ) -> Option<Snapshot> {
        let snapshot = self.snapshot(device, encoder, lower, self.full())?;
        self.blend_layer_into(device, encoder, uniforms, doc, upper, lower)?;
        Some(snapshot)
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
        let mut pass = render_pass(encoder, "merge", &self.scratch_view, wgpu::LoadOp::Load);
        pass.set_pipeline(&self.pipelines.blend);
        pass.set_bind_group(0, &bind, &[]);
        pass.set_bind_group(1, &self.uniform_bind, &[offset]);
        pass.draw(0..3, 0..1);
        drop(pass);
        encoder.copy_texture_to_texture(
            copy_at(&self.scratch, 0, 0),
            copy_at(&low.texture, 0, 0),
            extent(self.width, self.height),
        );
        Some(())
    }

    /// Group `group` of `doc` composited into layer `into`: copied into it (a new layer), or
    /// (`onto`) blended into it with the group's opacity and mode, returning what it was.
    #[allow(clippy::too_many_arguments)]
    fn flatten(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        doc: &Document,
        group: LayerId,
        into: LayerId,
        onto: bool,
    ) -> Option<Snapshot> {
        let full = self.full();
        if onto {
            let snapshot = self.snapshot(device, encoder, into, full)?;
            self.draw_group_onto(device, encoder, uniforms, doc, group, into)?;
            return Some(snapshot);
        }
        self.composite_group(device, doc, encoder, uniforms, full, group)?;
        let (textures, _) = self.groups.get(doc.depth(group))?;
        let target = self.layers.get(&into)?;
        encoder.copy_texture_to_texture(
            copy_at(&textures[0], 0, 0),
            copy_at(&target.texture, 0, 0),
            extent(self.width, self.height),
        );
        None
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
        Some(Snapshot { layer: id, rect, texture: region_copy(device, encoder, &layer.texture, rect) })
    }

    /// Applies an undo or redo step and returns its inverse: the pixels and document it replaced.
    fn apply_step(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, doc: &mut Document, step: Step) -> Step {
        let device = &gpu.device;
        let made = step.made;
        let name = step.name.clone();
        let mut pixels = Vec::with_capacity(step.pixels.len());
        for s in step.pixels {
            self.restore_all(gpu, [s.layer], true);
            if let Some(current) = self.snapshot(device, encoder, s.layer, s.rect) {
                pixels.push(current);
            }
            if let Some(layer) = self.layers.get(&s.layer) {
                encoder.copy_texture_to_texture(
                    copy_at(&s.texture, 0, 0),
                    copy_at(&layer.texture, s.rect.x0, s.rect.y0),
                    extent(s.rect.width(), s.rect.height()),
                );
            }
            self.dirty = self.dirty.union(s.rect);
        }
        let solo = doc.solo;
        let replaced = step.doc.map(|before| Box::new(std::mem::replace(doc, *before)));
        if replaced.is_some() {
            // a layer shown alone is how the artwork is looked at, not a change to undo
            doc.solo = solo.filter(|s| doc.layer(*s).is_some());
            self.dirty = self.full();
        }
        let selection = step.selection.map(|before| {
            let now = self.selection.take().map(|s| s.coverage);
            self.put_selection(device, &gpu.queue, before);
            now
        });
        // put back last first, so pixels that several snapshots cover end as the first left them
        pixels.reverse();
        Step { made, name, doc: replaced, pixels, selection }
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
            Filter::Zoom { centre: [x, y], amount } => ("fs_zoom", values([amount, 0.0, 0.0, 0.0], [x, y, 0.0, 0.0])),
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
            Filter::Liquify { amount } => ("fs_liquified", values([amount, 0.0, 0.0, 0.0], [0.0; 4])),
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
        self.dirty = self.full();
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
            let next = texture(device, "adjustment level", lw, lh, LAYER_FORMAT)
                .create_view(&wgpu::TextureViewDescriptor::default());
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
        let across = texture(device, "adjustment across", lw, lh, LAYER_FORMAT)
            .create_view(&wgpu::TextureViewDescriptor::default());
        let down = texture(device, "adjustment down", lw, lh, LAYER_FORMAT)
            .create_view(&wgpu::TextureViewDescriptor::default());
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

    /// A new step: it joins the history and anything undone can no longer be redone.
    fn push_step(&mut self, mut step: Step) {
        self.steps_made += 1;
        step.made = self.steps_made;
        if step.name.is_none() {
            step.name = self.next_name.take();
        }
        self.redo.clear();
        self.undo.push(step);
        let mut bytes: u64 = self.undo.iter().map(Step::bytes).sum();
        while self.undo.len() > MAX_UNDO || (bytes > MAX_UNDO_BYTES && self.undo.len() > 1) {
            let dropped = self.undo.remove(0);
            bytes = bytes.saturating_sub(dropped.bytes());
        }
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
            let mut pass = render_pass(encoder, "background", &self.composite_views[0], wgpu::LoadOp::Load);
            scissor(&mut pass, rect);
            pass.set_pipeline(&self.pipelines.solid);
            pass.set_bind_group(0, &self.stroke_bind, &[]);
            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
            pass.draw(0..3, 0..1);
        }
        // the stack bottom up; a group's layers go onto its depth's pair of textures first, and
        // the group onto the stack where its entry is
        let main = Stack { textures: &self.composite, views: &self.composite_views };
        if let Some(c) = &self.caches {
            // what is below as it was, what is worked on, then what is above as it was
            encoder.copy_texture_to_texture(
                copy_at(&c.below[0], rect.x0, rect.y0),
                copy_at(&self.composite[0], rect.x0, rect.y0),
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
        let busy =
            self.transforming.is_some() || self.liquify.is_some() || self.pencil.is_some() || self.held.is_some();
        if doc.animation.is_some() || busy || self.smudge.is_some() {
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
        while end > start + 1 && layer(end - 1).is_some_and(|l| l.blend == BlendMode::Normal) {
            end -= 1;
        }
        while end < tops.len() && layer(end).is_some_and(|l| l.clipped) {
            end += 1;
        }
        let live: HashSet<LayerId> = tops.get(start..end)?.iter().flat_map(|t| item_layers(doc, *t)).collect();
        if self.active.is_some_and(|a| !live.contains(&a.layer)) {
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
        let Some((tops, live)) = self.live_items(doc).filter(|_| self.cache_composites) else {
            self.caches = None;
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
        self.remake = None;
        let mut caches = self.caches.take().unwrap_or_else(|| {
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
        needed.extend(tops.get(live.start).and_then(|first| doc.clip_base(*first)));
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
            keep.extend(tops.get(live.start).and_then(|first| doc.clip_base(*first)));
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
            if !start_uniforms.data.is_empty() {
                gpu.queue.write_buffer(&self.uniform_buffer, 0, &start_uniforms.data);
            }
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
            let stack = Stack { textures: &caches.below, views: &caches.below_views };
            let mut current = 0;
            self.composite_items(device, doc, encoder, uniforms, full, &below_items, true, &stack, &mut current);
            stack.settle(encoder, full, current);
            caches.below_items = below_items;
            caches.below_key = below_key;
        }
        if remake_above {
            drop(render_pass(encoder, "above", &caches.above_views[0], wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)));
            let stack = Stack { textures: &caches.above, views: &caches.above_views };
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
        let per = u64::from(self.width) * u64::from(self.height) * 4;
        if self.resident_budget == 0 {
            usize::MAX
        } else {
            usize::try_from(self.resident_budget / per.max(1)).unwrap_or(usize::MAX).max(1)
        }
    }

    /// Lets go of the textures of layers unchanged since parked (their compressed copies kept),
    /// but those in `keep`.
    fn let_go_clean(&mut self, keep: &HashSet<LayerId>) {
        let parked = &self.parked;
        self.layers.retain(|id, _| keep.contains(id) || !parked.contains_key(id));
    }

    /// Sends the work recorded so far (with its uniforms), going on with a new encoder.
    fn send_now(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, uniforms: &mut UniformData) {
        let fresh = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("canvas") });
        let done = std::mem::replace(encoder, fresh);
        if !uniforms.data.is_empty() {
            gpu.queue.write_buffer(&self.uniform_buffer, 0, &uniforms.data);
        }
        gpu.queue.submit([done.finish()]);
        *uniforms = UniformData::default();
    }

    /// Goes on remaking the caches for a while (parts of whole items, each part's layers brought
    /// back, composited, sent, then let go before the next part's come back); once both are done,
    /// they are the caches and the whole composite is made again.
    fn go_on_remaking(&mut self, gpu: &Gpu, doc: &Document) {
        let Some(mut r) = self.remake.take() else { return };
        let device = &gpu.device;
        let full = self.full();
        let deadline = std::time::Instant::now() + REMAKE_SLICE;
        let (mut restored, mut restoring) = (0, std::time::Duration::ZERO);
        loop {
            let (below, items, progress) = match (r.below, r.above) {
                (Some(p), _) => (true, &r.below_items, p),
                (None, Some(p)) => (false, &r.above_items, p),
                (None, None) => break,
            };
            let (mut at, mut current) = progress;
            let (textures, views) =
                if below { (&r.caches.below, &r.caches.below_views) } else { (&r.caches.above, &r.caches.above_views) };
            let stack = Stack { textures, views };
            // as many whole items as fit and the slice's time allows (at least one), each with what
            // it is clipped to; an item painted nowhere is passed over, a normal one composites only
            // where it is painted
            let room = self.most_resident().saturating_sub(self.layers.len()).max(1);
            let mut part = Vec::new();
            let mut layers = Vec::new();
            let mut staged = 0;
            while let Some(&item) = items.get(at) {
                let mut more = item_layers(doc, item);
                more.extend(doc.clip_base(item));
                let late = std::time::Instant::now() >= deadline;
                if !part.is_empty() && (late || layers.len() + more.len() > room) {
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
            if !uniforms.data.is_empty() {
                gpu.queue.write_buffer(&self.uniform_buffer, 0, &uniforms.data);
            }
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
        if r.below.is_some() || r.above.is_some() {
            self.remake = Some(r);
            return;
        }
        log::info!(
            "caches remade in {} ms over {} renders: {} items, {} layers brought back in {} ms",
            r.started.elapsed().as_millis(),
            r.slices,
            r.below_items.len() + r.above_items.len(),
            r.restored,
            r.restoring.as_millis()
        );
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

    /// Parks layer `id` with pixels already made ready (by [`ReadyToPark::of`], with this
    /// canvas's size).
    pub fn park_ready(&mut self, id: LayerId, ready: ReadyToPark) {
        self.layers.remove(&id);
        self.parked.insert(id, ready.parked);
        self.pictures.insert(id, ready.pictures);
        self.caches = None;
        self.invalidate();
    }

    /// Whether work keeps the caches: painting, or undoing or redoing it, on a layer worked on.
    fn keeps_caches(&self, op: &Op, live: &HashSet<LayerId>) -> bool {
        let only_live = |step: Option<&Step>| {
            step.is_some_and(|s| s.doc.is_none() && s.pixels.iter().all(|p| live.contains(&p.layer)))
        };
        match op {
            Op::Stamps { layer, .. } => live.contains(layer),
            Op::Commit(stroke) => live.contains(&stroke.layer),
            Op::Cancel | Op::SaveTip(_) | Op::Untip(_) | Op::Mark | Op::Name(_) | Op::Collapse => true,
            Op::Undo => only_live(self.undo.last()),
            Op::Redo => only_live(self.redo.last()),
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
    /// is in `stack.views[current]`.
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
        let inner = Stack { textures, views };
        drop(render_pass(encoder, "group clear", &views[0], wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)));
        let at = self.composite_members(device, doc, encoder, uniforms, rect, Some(id), &inner);
        inner.settle(encoder, rect, at);
        Some(&views[0])
    }

    /// One paint layer (through its mask, with the stroke being drawn on it or its mask) onto a
    /// stack whose result is in `stack.views[current]`.
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
                // the smudge changes the layer itself
                Some(StrokeTool::Smudge) => STROKE_NONE,
            },
            stroke_opacity: stroke.map_or(0.0, |s| s.style.opacity.clamp(0.0, 1.0)),
            blend: layer.blend.id(),
            clipped: u32::from(clip.is_some()),
            masked: if mask_view.is_some() { 1.0 } else { 0.0 },
            stroke_on_mask: if stroke.is_some_and(|s| Some(s.layer) == mask) { 1.0 } else { 0.0 },
            ..Default::default()
        }
        .with_grain(stroke.as_ref().map(|s| &s.style));
        let mask_view = mask_view.unwrap_or(&self.stroke_view);
        self.draw_onto(
            device,
            encoder,
            uniforms,
            rect,
            &gpu_layer.view,
            Some(clip_view),
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
        clip: Option<&wgpu::TextureView>,
        mask: &wgpu::TextureView,
        uniform: LayerUniform,
        blend: BlendMode,
        stack: &Stack<'_>,
        current: &mut usize,
    ) {
        let offset = uniforms.push(&uniform);
        let clip = clip.unwrap_or(&self.stroke_view);
        let [first, second] = stack.views;
        if blend == BlendMode::Normal {
            let bind = self.quad_masked(device, [view, &self.stroke_view, clip, &self.stroke_view], mask);
            let target = if *current == 0 { first } else { second };
            let mut pass = render_pass(encoder, "layer", target, wgpu::LoadOp::Load);
            scissor(&mut pass, rect);
            pass.set_pipeline(&self.pipelines.layer);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
            pass.draw(0..3, 0..1);
        } else {
            let (backdrop, target) = if *current == 0 { (first, second) } else { (second, first) };
            let bind = self.quad_masked(device, [view, &self.stroke_view, clip, backdrop], mask);
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
        let ScreenPipelines { display, surround: surround_pipelines, overlay, selection: selection_pipelines, .. } =
            &mut self.screen;
        let pipeline = display.entry(target.format).or_insert_with(|| {
            sampled_pipeline(
                device,
                display_lut_layout,
                display_module,
                "display",
                "vs_display",
                "fs_display",
                target.format,
            )
        });
        let (w, h) = (target.size.0.max(1) as f32, target.size.1.max(1) as f32);
        let to_clip = Affine { a: 2.0 / w, b: 0.0, c: 0.0, d: -2.0 / h, e: -1.0, f: 1.0 };
        let unit_to_canvas = Affine { a: self.width as f32, b: 0.0, c: 0.0, d: self.height as f32, e: 0.0, f: 0.0 };
        let m = unit_to_canvas.then(target.to_screen).then(to_clip);
        let offset = uniforms.push(&DisplayUniform::new(
            [m.a, m.b, m.c, m.d],
            [m.e, m.f, if srgb { 1.0 } else { 0.0 }, 0.0],
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
        pass.set_bind_group(0, &self.display_bind, &[]);
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
        let Some(style) = self.active.filter(|a| !a.pencil).map(|a| a.style).filter(|s| s.tool == StrokeTool::Paint)
        else {
            return;
        };
        if predicted.is_empty() {
            return;
        }
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
            _pad: [0; 2],
        });
        let size = size_of::<StampInstance>() as u64;
        pass.set_pipeline(overlay_pipeline);
        pass.set_bind_group(0, &self.uniform_bind, &[offset]);
        pass.set_bind_group(1, &self.brush_bind, &[]);
        pass.set_vertex_buffer(
            0,
            self.stamp_buffer.slice(u64::from(predicted.start) * size..u64::from(predicted.end) * size),
        );
        pass.draw(0..6, 0..predicted.end - predicted.start);
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
        if backdrop.regions.is_empty() || backdrop.sigma <= 0.0 {
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
        };
        self.frost(&gpu.device, &mut encoder, &mut uniforms, &target, backdrop, Some(scene));
        if !uniforms.data.is_empty() {
            gpu.queue.write_buffer(&self.uniform_buffer, 0, &uniforms.data);
        }
        gpu.queue.submit([encoder.finish()]);
    }

    /// Frosted panels: the canvas and its surround (or what `other` draws) drawn again, small,
    /// blurred, and drawn into each region of the target.
    fn frost(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        target: &Target<'_>,
        backdrop: Backdrop<'_>,
        other: Option<SmallScene<'_>>,
    ) {
        let k = (backdrop.sigma / BACKDROP_TEXELS).floor().clamp(1.0, 16.0);
        let size = ((target.size.0 as f32 / k).ceil() as u32, (target.size.1 as f32 / k).ceil() as u32);
        if self.backdrop.as_ref().is_none_or(|b| b.size != size || b.format != target.format) {
            self.backdrop =
                Some(BackdropGpu::new(device, &self.pipelines.sampled_layout, &self.sampler, size, target.format));
        }
        let Some(small) = self.backdrop.as_ref() else { return };
        let srgb = target.format.is_srgb();
        let Pipelines { display_module, display_pipeline_layout, display_lut_layout, backdrop_module, .. } =
            &*self.pipelines;
        let ScreenPipelines { display, blur, frost, .. } = &mut self.screen;
        let display = display.entry(target.format).or_insert_with(|| {
            sampled_pipeline(
                device,
                display_lut_layout,
                display_module,
                "display",
                "vs_display",
                "fs_display",
                target.format,
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
        let frost = frost.entry(target.format).or_insert_with(|| {
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
            [m.e, m.f, if srgb { 1.0 } else { 0.0 }, 0.0],
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
            [(&small.binds[0], &small.views[1], [1.0 / w, 0.0]), (&small.binds[1], &small.views[0], [0.0, 1.0 / h])]
        {
            let offset =
                uniforms.push(&BackdropUniform { v0: [step[0], step[1], sigma, radius], v1: [0.0; 4], v2: [0.0; 4] });
            let mut pass = render_pass(encoder, "backdrop blur", to, wgpu::LoadOp::Clear(clear));
            pass.set_pipeline(blur);
            pass.set_bind_group(0, from, &[]);
            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
            pass.draw(0..3, 0..1);
        }
        // into each region
        let (tw, th) = (target.size.0.max(1) as f32, target.size.1.max(1) as f32);
        let mut pass = render_pass(encoder, "frost", target.view, wgpu::LoadOp::Load);
        pass.set_pipeline(frost);
        pass.set_bind_group(0, &small.binds[0], &[]);
        for &[x0, y0, x1, y1, corner, alpha] in backdrop.regions {
            if ![x0, y0, x1, y1, corner, alpha].iter().all(|v| v.is_finite()) || x1 <= x0 || y1 <= y0 || alpha <= 0.0 {
                continue;
            }
            let offset = uniforms.push(&BackdropUniform {
                v0: [x0, y0, x1, y1],
                v1: [corner, tw, th, k],
                v2: [alpha.min(1.0), 0.0, 0.0, 0.0],
            });
            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
            pass.draw(0..6, 0..1);
        }
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
        self.sample_view(gpu, &self.composite_views[0], self.full(), max_side)
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
        gpu.queue.write_buffer(&self.uniform_buffer, 0, &data.data);
        gpu.queue.submit([encoder.finish()]);
        let rgba = read_texture(gpu, &small, w, h)?;
        Ok(Thumbnail { width: w, height: h, rgba })
    }

    /// Replaces a layer's pixels (premultiplied RGBA8, the canvas's size, rows top to bottom), for
    /// opening a saved artwork. Not an undo step.
    pub fn upload_layer(&mut self, gpu: &Gpu, id: LayerId, rgba: &[u8]) -> Result<(), GpuError> {
        let expected = self.width as usize * self.height as usize * 4;
        if rgba.len() != expected {
            return Err(GpuError::Readback(format!("layer {id:?}: {} bytes, expected {expected}", rgba.len())));
        }
        if !self.layers.contains_key(&id) {
            let texture = texture(&gpu.device, "layer", self.width, self.height, LAYER_FORMAT);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.layers.insert(id, LayerGpu { texture, view });
        }
        let Some(layer) = self.layers.get(&id) else { return Ok(()) };
        self.caches = None;
        self.changed.insert(id);
        // its compressed copy no longer what it holds
        self.parked.remove(&id);
        self.pictures.remove(&id);
        gpu.queue.write_texture(
            copy_at(&layer.texture, 0, 0),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.width * 4),
                rows_per_image: Some(self.height),
            },
            extent(self.width, self.height),
        );
        // sent now, so many layers in a row (an artwork opening, a moving picture pasted) don't
        // each keep a staged copy until the next frame
        gpu.flush();
        self.invalidate();
        Ok(())
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
        read_texture(gpu, &self.composite[0], self.width, self.height)
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
        self.quad_masked(device, [a, b, c, d], &self.stroke_view)
    }

    /// As [`Self::quad`], with the layer's mask.
    fn quad_masked(
        &self,
        device: &wgpu::Device,
        views: [&wgpu::TextureView; 4],
        mask: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let [a, b, c, d] = views;
        quad_bind(device, &self.pipelines.quad_layout, [a, b, c, d, mask], &self.grains_view, &self.grain_sampler)
    }

    /// Layer textures held (the document's and those the history still needs).
    pub fn layer_texture_count(&self) -> usize {
        self.layers.len()
    }
}

impl Pipelines {
    pub(crate) fn new(device: &wgpu::Device) -> Pipelines {
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
        let liquify_layout = layout("liquify", &[Some(&uniform_layout)]);
        let liquify_pipeline = |label: &str, blend: wgpu::BlendState| {
            blended_pipeline(
                device,
                &liquify_layout,
                &liquify_module,
                label,
                ("vs_liquify", "fs_liquify"),
                LIQUIFY_FORMAT,
                Some(blend),
            )
        };
        let liquify_add = liquify_pipeline("liquify add", ADD);
        let liquify_fade = liquify_pipeline("liquify fade", FADE);
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
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
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
        let smudge_deposit = compute("smudge deposit", "cs_deposit");

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
            before_layout,
            filters,
            liquify_add,
            liquify_fade,
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
            smudge_deposit,
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

/// The liquify field: two half floats of offset a pixel.
const LIQUIFY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

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

/// What is there kept by one minus what is drawn's alpha (reconstruct fading the field).
const FADE: wgpu::BlendState = wgpu::BlendState {
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
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// One frame's uniforms, a slot per draw.
#[derive(Default)]
struct UniformData {
    data: Vec<u8>,
}

impl UniformData {
    /// Adds `value` in the next slot and returns its dynamic offset.
    fn push<T: bytemuck::Pod>(&mut self, value: &T) -> u32 {
        let offset = self.data.len();
        self.data.extend_from_slice(bytemuck::bytes_of(value));
        self.data.resize(offset + SLOT as usize, 0);
        u32::try_from(offset).unwrap_or(0)
    }
}

fn texture(device: &wgpu::Device, label: &str, width: u32, height: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
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
            | wgpu::TextureUsages::COPY_DST,
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

struct Stack<'a> {
    textures: &'a [wgpu::Texture; 2],
    views: &'a [wgpu::TextureView; 2],
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
    restored: usize,
    restoring: std::time::Duration,
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

impl Stack<'_> {
    /// Leaves the result in the first texture (where `current` says it is, within `rect`).
    fn settle(&self, encoder: &mut wgpu::CommandEncoder, rect: IRect, current: usize) {
        if current == 1 {
            let [first, second] = self.textures;
            encoder.copy_texture_to_texture(
                copy_at(second, rect.x0, rect.y0),
                copy_at(first, rect.x0, rect.y0),
                extent(rect.width(), rect.height()),
            );
        }
    }
}

fn quad_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    views: [&wgpu::TextureView; 5],
    grains: &wgpu::TextureView,
    grain_sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    let [a, b, c, d, mask] = views;
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
        ],
    })
}

/// How many images of the user's own each of the shape and grain arrays holds at once.
const SOURCE_SLOTS: usize = 8;

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
    let n = TEXTURE_SIZE as u32;
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
    let row = width as usize * 4;
    let padded = row.div_ceil(256) * 256;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded * height as usize) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
    encoder.copy_texture_to_buffer(
        copy_at(texture, 0, 0),
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
    let (tx, rx) = mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| GpuError::Readback(e.to_string()))?;
    rx.recv().map_err(|e| GpuError::Readback(e.to_string()))?.map_err(|e| GpuError::Readback(e.to_string()))?;
    let mapped = buffer.slice(..).get_mapped_range().map_err(|e| GpuError::Readback(e.to_string()))?;
    let mut out = Vec::with_capacity(row * height as usize);
    for chunk in mapped.chunks(padded) {
        out.extend_from_slice(chunk.get(..row).unwrap_or(chunk));
    }
    drop(mapped);
    buffer.unmap();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // and the charge, half run out by 33 px, as far as its curve lets it
        let (charged, ..) = wet_mixture(&style, 33.0, 1.0, 0.0, 1.0, 1.0);
        assert!((charged - (0.5f32.powf(2.4) + 0.5)).abs() < 1e-5);
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
}
