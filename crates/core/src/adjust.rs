//! Adjustments: the current layer redrawn through a filter while its values are dragged, kept as
//! one undo step or put back; only what is selected changes.

use procraft_engine::geom::Vec2;
use procraft_engine::selection::feather_radius;
use procraft_engine::tone;
use procraft_gpu::{Filter, LiquifyMode, LiquifyStamp};

use procraft_engine::stroke::StrokeBuilder;
use procraft_gpu::{Canvas, StrokeStyle, StrokeTool};

use crate::{CanvasSession, PenSample, PointerKind, State, ToolKind};

/// The adjustments whose one amount (0..1) is dragged across the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SwipeAdjustment {
    GaussianBlur,
    Sharpen,
    Noise,
    PerspectiveBlur,
    ChromaticAberration,
    Halftone,
    Bloom,
}

/// The kinds of noise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NoiseKind {
    Clouds,
    Billows,
    Ridges,
}

/// The kinds of halftone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum HalftoneKind {
    FullColor,
    ScreenPrint,
    Newspaper,
}

/// Chromatic aberration pulled from the middle, or moved one way everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ChromaticKind {
    Perspective,
    Displace,
}

/// Noise from its sliders (0..1): the amount reads three times as strong; the scale makes it
/// coarser, the octaves add finer layers (one to five), the turbulence keeps more of each.
fn noise(amount: f32, kind: NoiseKind, scale: f32, octaves: f32, turbulence: f32, multi: bool, added: bool) -> Filter {
    Filter::Noise {
        amount: amount * 3.0 * 0.6,
        frequency: 0.0000517 + 0.4564 * (scale - 1.0).powi(4),
        octaves: (octaves * 4.0) as u32 + 1,
        turbulence: turbulence * 0.32,
        kind: kind as u32,
        multi,
        added,
    }
}

/// Bloom from its sliders (0..1): the size through the blur's own curve, the burn from a little
/// weaker to much stronger.
fn bloom(amount: f32, transition: f32, size: f32, burn: f32) -> Filter {
    Filter::Bloom {
        sigma: feather_radius(size.powf(1.8)),
        amount,
        threshold: BLOOM_THRESHOLD,
        transition,
        burn: -0.3 + 1.3 * burn,
    }
}

/// Chromatic aberration from its sliders (0..1): perspective pulls from `centre` by the amount,
/// `a` smearing the fringes and `b` keeping them off the middle; displace moves them by `offset`
/// (canvas pixels), `a` smearing them and `b` making them transparent.
fn chromatic(centre: [f32; 2], kind: ChromaticKind, amount: f32, offset: [f32; 2], a: f32, b: f32) -> Filter {
    let displace = kind == ChromaticKind::Displace;
    Filter::Chromatic {
        centre,
        amount: amount * 0.05,
        displace,
        offset,
        blur: a,
        falloff: if displace { 0.0 } else { b },
        transparency: if displace { b } else { 0.0 },
    }
}

/// A halftone's cells (canvas pixels) for the amount.
fn halftone_pitch(amount: f32) -> f32 {
    amount * 96.0 + 2.0
}

/// A slider's value kept to 0..1 (0 if not a number).
fn unit(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
}

/// The ways a glitch breaks the layer up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum GlitchKind {
    /// Blocks copied from a few blocks away.
    Artifact,
    /// Rows bent sideways.
    Wave,
    /// Bands ghosted sideways.
    Signal,
    /// The channels moved apart, block by block.
    Diverge,
}

/// How a liquify brush moves what is under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LiquifyKind {
    Push,
    TwirlRight,
    TwirlLeft,
    Pinch,
    Expand,
    Crystals,
    Edge,
    Reconstruct,
}

impl From<LiquifyKind> for LiquifyMode {
    fn from(k: LiquifyKind) -> LiquifyMode {
        match k {
            LiquifyKind::Push => LiquifyMode::Push,
            LiquifyKind::TwirlRight => LiquifyMode::TwirlRight,
            LiquifyKind::TwirlLeft => LiquifyMode::TwirlLeft,
            LiquifyKind::Pinch => LiquifyMode::Pinch,
            LiquifyKind::Expand => LiquifyMode::Expand,
            LiquifyKind::Crystals => LiquifyMode::Crystals,
            LiquifyKind::Edge => LiquifyMode::Edge,
            LiquifyKind::Reconstruct => LiquifyMode::Reconstruct,
        }
    }
}

/// The liquify brush: its way, and its size, pressure and distortion sliders (0..1).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct LiquifyBrush {
    pub kind: LiquifyKind,
    pub size: f32,
    pub pressure: f32,
    pub distortion: f32,
}

/// Noise's default fineness (per canvas pixel), octaves and turbulence.
/// Noise's turbulence slider as it starts.
const NOISE_TURBULENCE: f32 = 0.7;
/// Bloom's default size (as a blur amount) and where its light starts.
/// Bloom's size and burn sliders as they start.
const BLOOM_SIZE: f32 = 0.3;
const BLOOM_BURN: f32 = 0.35;
const BLOOM_THRESHOLD: f32 = 0.5;
/// How long (milliseconds) the liquify brush carries on at its lifting speed, at full momentum
/// (it covers that distance while slowing to a stop), and the most stamps it adds doing so.
const LIQUIFY_COAST_MS: f32 = 300.0;
const LIQUIFY_COAST_STAMPS: usize = 400;
/// The (set, brush) cloning paints with: the soft airbrush.
const CLONE_BRUSH: (usize, usize) = (3, 0);
/// How far (points) a motion blur's drag goes for its whole amount, and the blur that is (canvas
/// pixels).
const MOTION_DRAG_PT: f32 = 500.0;
const MOTION_LENGTH_PX: f32 = 256.0;

impl State {
    /// A stroke that paints the adjustment on, with the paint brush (white into the mask). Cloning,
    /// the first stroke fixes how far from the source it copies.
    pub(crate) fn pencil_stroke(&mut self, pointer: PointerKind, samples: &[PenSample]) {
        let current = self.doc.current();
        let sliders = self.shown_sliders();
        let brush = if self.cloning {
            self.shipped.get(CLONE_BRUSH.0).and_then(|s| s.brushes.get(CLONE_BRUSH.1)).unwrap_or(&self.fallback).clone()
        } else {
            self.brush(ToolKind::Paint).clone()
        };
        self.strokes = self.strokes.wrapping_add(1);
        let seed = self.strokes.wrapping_mul(0x9e37_79b9);
        let mut builder = StrokeBuilder::with_seed(brush.clone(), brush.diameter(sliders.size), pointer.into(), seed)
            .with_input(self.input.clone())
            .on_view_turned(self.view.map_or(0.0, |v| v.rotation()))
            .seen_at(self.view.map_or(1.0, |v| v.zoom()) / self.display_scale);
        let canvas_samples = self.to_canvas(samples);
        if let (Some(source), None, Some(first)) = (self.clone_source, self.clone_offset, canvas_samples.first()) {
            let offset = source - first.pos;
            self.clone_offset = Some(offset);
            self.last_filter = Some(Filter::Clone { offset: [offset.x, offset.y] });
        }
        let stamps = builder.add(&canvas_samples);
        let mut style =
            StrokeStyle::for_brush(&brush, StrokeTool::Paint, [1.0, 1.0, 1.0], brush.opacity(sliders.opacity), false);
        let Some(canvas) = self.canvas.as_mut() else { return };
        if let Some(gpu) = self.gpu.as_ref() {
            canvas.use_sources(gpu, &mut style, &brush);
        }
        canvas.begin_pencil_stroke(current, style);
        canvas.add_stamps(&stamps);
        self.stroke = Some(builder);
        self.redraw_painted_on();
    }

    /// Keeps the stroke just painted on as its own undo step, and goes on with the adjustment.
    pub(crate) fn bake_painted_on(&mut self) {
        if !(self.adjusting && self.pencil) {
            return;
        }
        let current = self.doc.current();
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.end_transform(true);
            canvas.begin_transform(current);
            canvas.begin_pencil();
            if let Some(filter) = self.last_filter {
                canvas.set_filter(filter);
            }
        }
    }

    /// Drops what was painted on since the last stroke kept, runs `between` (an undo or redo),
    /// and goes on with the adjustment.
    pub(crate) fn restart_painted_on(&mut self, between: Option<fn(&mut Canvas)>) {
        if !(self.adjusting && self.pencil) {
            return;
        }
        let current = self.doc.current();
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.end_transform(false);
            if let Some(f) = between {
                f(canvas);
            }
            canvas.begin_transform(current);
            canvas.begin_pencil();
            if let Some(filter) = self.last_filter {
                canvas.set_filter(filter);
            }
        }
    }

    /// Draws the adjustment again after more of it was painted on.
    pub(crate) fn redraw_painted_on(&mut self) {
        if self.pencil
            && let (Some(filter), Some(canvas)) = (self.last_filter, self.canvas.as_mut())
        {
            canvas.set_filter(filter);
        }
    }

    /// Ends an open adjustment: `commit` keeps it as one undo step, otherwise the layer goes back.
    pub(crate) fn end_adjustment(&mut self, commit: bool) {
        self.pencil = false;
        self.cloning = false;
        self.clone_source = None;
        self.clone_offset = None;
        if std::mem::take(&mut self.adjusting)
            && let Some(canvas) = self.canvas.as_mut()
        {
            if commit {
                canvas.name_next_step(match self.last_filter {
                    Some(Filter::Hsb { .. }) => "adjust HSB",
                    Some(Filter::Curves) => "adjust Curves",
                    Some(Filter::ColorBalance { .. }) => "change Color Balance",
                    Some(Filter::GradientMap { .. }) => "change Gradient Map",
                    _ => "adjustment",
                });
            }
            canvas.end_transform(commit);
            if self.simplified_undos {
                canvas.collapse_steps();
            }
        }
    }

    fn adjust(&mut self, filter: Filter) {
        if self.adjusting
            && let Some(canvas) = self.canvas.as_mut()
        {
            self.last_filter = Some(filter);
            canvas.set_filter(filter);
        }
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Starts adjusting the current layer; false when it cannot be (locked or hidden). An
    /// adjustment already open is kept first.
    pub fn adjust_begin(&self) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let current = st.doc.current();
        if st.doc.layer(current).is_none_or(|l| l.locked || !l.visible || l.text.is_some()) {
            return false;
        }
        let Some(canvas) = st.canvas.as_mut() else { return false };
        canvas.mark_steps();
        canvas.begin_transform(current);
        st.adjusting = true;
        st.last_filter = None;
        self.shared.frame(st);
        true
    }

    /// Hue, saturation and brightness, each 0..1 with 0.5 leaving the layer as it is: the hue turns
    /// the colour up to half a turn either way, saturation scales it from grey to twice as strong,
    /// brightness lightens or darkens it all the way.
    pub fn adjust_hsb(&self, hue: f32, saturation: f32, brightness: f32) {
        let [h, s, b] =
            [hue, saturation, brightness].map(|v| if v.is_finite() { (v.clamp(0.0, 1.0) - 0.5) * 2.0 } else { 0.0 });
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.adjust(Filter::Hsb { hue: h * std::f32::consts::PI, saturation: 1.0 + s, brightness: b });
        self.shared.frame(st);
    }

    /// One of the dragged adjustments at `amount` (0..1), with its other values at their defaults.
    pub fn adjust_swipe(&self, kind: SwipeAdjustment, amount: f32) {
        let a = if amount.is_finite() { amount.clamp(0.0, 1.0) } else { 0.0 };
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let centre = [st.doc.width() as f32 / 2.0, st.doc.height() as f32 / 2.0];
        let filter = match kind {
            SwipeAdjustment::GaussianBlur => Filter::Blur { sigma: feather_radius(a) },
            SwipeAdjustment::Sharpen => Filter::Sharpen { amount: a },
            // the amount reads three times as strong
            SwipeAdjustment::Noise => noise(a, NoiseKind::Clouds, 0.0, 0.0, NOISE_TURBULENCE, false, true),
            SwipeAdjustment::PerspectiveBlur => Filter::Zoom { centre, amount: a * 0.25 },
            SwipeAdjustment::ChromaticAberration => {
                chromatic(centre, ChromaticKind::Perspective, a, [0.0; 2], 0.0, 0.0)
            }
            SwipeAdjustment::Halftone => Filter::Halftone { pitch: halftone_pitch(a), kind: 0 },
            SwipeAdjustment::Bloom => bloom(a, 1.0, BLOOM_SIZE, BLOOM_BURN),
        };
        st.adjust(filter);
        self.shared.frame(st);
    }

    /// A motion blur along the drag from its start to window point offset (`dx`, `dy`): longer the
    /// further the drag goes. Returns the amount (0..1).
    pub fn adjust_motion(&self, dx: f32, dy: f32) -> f32 {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let points = Vec2::new(dx, dy) * (1.0 / st.display_scale.max(1e-6));
        let amount = (points.length() / MOTION_DRAG_PT).clamp(0.0, 1.0);
        // the drag's direction on the canvas, through the view's turn
        let turn = st.view.map_or(0.0, |v| v.to_screen().rotation());
        let angle = dy.atan2(dx) - turn;
        let length = amount * MOTION_LENGTH_PX;
        if length.is_finite() {
            st.adjust(Filter::Motion { along: [length * angle.cos(), length * angle.sin()] });
            self.shared.frame(st);
        }
        amount
    }

    /// Curves: the points (x, y pairs in 0..1) of the curve for all three channels and of the red,
    /// green and blue curves; each runs through its points as a smooth spline.
    pub fn adjust_curves(&self, all: Vec<f32>, red: Vec<f32>, green: Vec<f32>, blue: Vec<f32>) {
        let points = |v: &[f32]| v.as_chunks::<2>().0.iter().map(|&[x, y]| (x, y)).collect::<Vec<_>>();
        let [a, r, g, b] = [&all, &red, &green, &blue].map(|v| tone::curve_table(&points(v)));
        let entries: Vec<u8> = r.iter().zip(&g).zip(&b).zip(&a).flat_map(|(((&r, &g), &b), &a)| [r, g, b, a]).collect();
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) {
            canvas.set_table(gpu, &entries);
        }
        st.adjust(Filter::Curves);
        self.shared.frame(st);
    }

    /// The curve through `points` (x, y pairs in 0..1) as 256 heights (0..1), for drawing.
    pub fn curve_shape(&self, points: Vec<f32>) -> Vec<f32> {
        let pts: Vec<(f32, f32)> = points.as_chunks::<2>().0.iter().map(|&[x, y]| (x, y)).collect();
        tone::curve_table(&pts).iter().map(|&v| f32::from(v) / 255.0).collect()
    }

    /// Noise of `kind`, `amount` (0..1) of it, with its sliders (0..1); each channel its own when
    /// `multi`; added to the colour, or else mixed in.
    #[allow(clippy::too_many_arguments)]
    pub fn adjust_noise(
        &self,
        kind: NoiseKind,
        amount: f32,
        scale: f32,
        octaves: f32,
        turbulence: f32,
        multi: bool,
        added: bool,
    ) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.adjust(noise(unit(amount), kind, unit(scale), unit(octaves), unit(turbulence), multi, added));
        self.shared.frame(st);
    }

    /// Bloom, `amount` (0..1) of it, with its transition, size and burn sliders (0..1).
    pub fn adjust_bloom(&self, amount: f32, transition: f32, size: f32, burn: f32) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.adjust(bloom(unit(amount), unit(transition), unit(size), unit(burn)));
        self.shared.frame(st);
    }

    /// A halftone of `kind`, its cells growing with `amount` (0..1).
    pub fn adjust_halftone(&self, kind: HalftoneKind, amount: f32) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.adjust(Filter::Halftone { pitch: halftone_pitch(unit(amount)), kind: kind as u32 });
        self.shared.frame(st);
    }

    /// Chromatic aberration of `kind`: perspective by `amount` (0..1) from the middle; displace by
    /// the drag (`dx`, `dy`, window pixels). `a` and `b` are its two sliders (0..1): perspective's
    /// transition and fall off, displace's blur and transparency.
    pub fn adjust_chromatic(&self, kind: ChromaticKind, amount: f32, dx: f32, dy: f32, a: f32, b: f32) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let centre = [st.doc.width() as f32 / 2.0, st.doc.height() as f32 / 2.0];
        // the drag on the canvas, through the view's scale and turn
        let offset = st
            .view
            .and_then(|v| v.to_screen().inverse())
            .map(|inv| inv.apply(Vec2::new(dx, dy)) - inv.apply(Vec2::new(0.0, 0.0)))
            .filter(|o| o.is_finite())
            .map_or([0.0; 2], |o| [o.x, o.y]);
        st.adjust(chromatic(centre, kind, unit(amount), offset, unit(a), unit(b)));
        self.shared.frame(st);
    }

    /// A glitch of `kind`, `amount` (0..1) of it. `values` are the kind's own sliders: artifact and
    /// signal amount and block size, wave amplitude and frequency (each 0..1), diverge the red,
    /// green and blue shifts (−1..1); `zoom` (0..1) scales the pattern.
    pub fn adjust_glitch(&self, kind: GlitchKind, amount: f32, values: Vec<f32>, zoom: f32) {
        let finite = |v: f32, lo: f32| if v.is_finite() { v.clamp(lo, 1.0) } else { 0.0 };
        let lo = if kind == GlitchKind::Diverge { -1.0 } else { 0.0 };
        let v = |k: usize| finite(values.get(k).copied().unwrap_or(0.0), lo);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        // the same pattern for the whole adjustment, another for the next
        let seed = (st.revision % 97) as f32 * 0.731;
        st.adjust(Filter::Glitch {
            kind: kind as u32,
            amount: finite(amount, 0.0),
            values: [v(0), v(1), v(2)],
            zoom: finite(zoom, 0.0),
            seed,
        });
        self.shared.frame(st);
    }

    /// Colour balance: for the shadows, midtones and highlights, how far each of red (against
    /// cyan), green (against magenta) and blue (against yellow) moves, each −1..1.
    pub fn adjust_color_balance(&self, shadows: Vec<f32>, midtones: Vec<f32>, highlights: Vec<f32>) {
        let three = |v: &[f32]| -> [f32; 3] {
            std::array::from_fn(|i| v.get(i).copied().filter(|x| x.is_finite()).unwrap_or(0.0).clamp(-1.0, 1.0))
        };
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.adjust(Filter::ColorBalance {
            shadows: three(&shadows),
            midtones: three(&midtones),
            highlights: three(&highlights),
        });
        self.shared.frame(st);
    }

    /// A gradient map: the gradient through `stops` (position and red, green, blue, all 0..1, four
    /// numbers a stop) by brightness, `amount` (0..1) of the way.
    pub fn adjust_gradient_map(&self, stops: Vec<f32>, amount: f32) {
        let stops: Vec<(f32, [f32; 3])> = stops.as_chunks::<4>().0.iter().map(|&[p, r, g, b]| (p, [r, g, b])).collect();
        let table = tone::gradient_table(&stops);
        let entries: Vec<u8> = table.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect();
        let a = if amount.is_finite() { amount.clamp(0.0, 1.0) } else { 1.0 };
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) {
            canvas.set_table(gpu, &entries);
        }
        st.adjust(Filter::GradientMap { amount: a });
        self.shared.frame(st);
    }

    /// Starts liquifying the current layer; false when it cannot be.
    pub fn liquify_begin(&self) -> bool {
        if !self.adjust_begin() {
            return false;
        }
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.begin_liquify();
        }
        st.adjust(Filter::Liquify { amount: 1.0 });
        true
    }

    /// The liquify brush along window points `points` (x, y, pressure 0..1 each); `start` begins a
    /// new stroke. Stamps fall a tenth of the brush apart.
    pub fn liquify_stroke(&self, points: Vec<f32>, start: bool, brush: LiquifyBrush) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(view) = st.view else { return };
        let size = brush.size.clamp(0.0, 1.0);
        let radius = 4.0 + 1596.0 * size.powf(2.2);
        let strength = 0.01 + 0.99 * brush.pressure.clamp(0.0, 1.0);
        let step = (radius * 0.1).max(1.0);
        if start {
            st.liquify_last = None;
        }
        let mut stamps = Vec::new();
        for &[x, y, pen] in points.as_chunks::<3>().0 {
            let Some(p) = view.screen_to_canvas(Vec2::new(x, y)).filter(|p| p.is_finite()) else { continue };
            let pressure = strength * if pen.is_finite() && pen > 0.0 { pen.min(1.0) } else { 1.0 };
            let Some(last) = st.liquify_last else {
                st.liquify_last = Some(p);
                continue;
            };
            let gap = p.distance(last);
            let count = (gap / step).floor() as usize;
            if count == 0 {
                continue;
            }
            let dir = (p - last) * (1.0 / gap);
            let mut at = last;
            for _ in 0..count {
                let next = at + dir * step;
                st.strokes = st.strokes.wrapping_add(1);
                stamps.push(LiquifyStamp {
                    centre: [next.x, next.y],
                    moved: [dir.x * step, dir.y * step],
                    radius,
                    pressure,
                    distortion: brush.distortion.clamp(0.0, 1.0),
                    seed: (st.strokes % 997) as f32,
                    mode: brush.kind.into(),
                });
                at = next;
            }
            st.liquify_last = Some(at);
        }
        if let (false, Some(canvas)) = (stamps.is_empty(), st.canvas.as_mut()) {
            canvas.liquify_stamps(&stamps);
            if let Some(f) = st.last_filter {
                canvas.set_filter(f);
            }
            self.shared.frame(st);
        }
    }

    /// The liquify brush carries on after it lifts: from its last stamp along the window velocity
    /// (`vx`, `vy`, pixels a millisecond), slowing to a stop and fading as it goes, for as long as
    /// `momentum` (0..1) lets it.
    pub fn liquify_coast(&self, vx: f32, vy: f32, momentum: f32, brush: LiquifyBrush) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let (Some(view), Some(last)) = (st.view, st.liquify_last.take()) else { return };
        let Some(inv) = view.to_screen().inverse() else { return };
        let velocity = inv.apply(Vec2::new(vx, vy)) - inv.apply(Vec2::new(0.0, 0.0));
        let momentum = if momentum.is_finite() { momentum.clamp(0.0, 1.0) } else { 0.0 };
        let reach = velocity.length() * momentum * LIQUIFY_COAST_MS;
        let radius = 4.0 + 1596.0 * brush.size.clamp(0.0, 1.0).powf(2.2);
        let step = (radius * 0.1).max(1.0);
        if !reach.is_finite() || reach < step {
            return;
        }
        let dir = velocity * (1.0 / velocity.length());
        let strength = 0.01 + 0.99 * brush.pressure.clamp(0.0, 1.0);
        let count = ((reach / step) as usize).min(LIQUIFY_COAST_STAMPS);
        let mut stamps = Vec::with_capacity(count);
        let mut at = last;
        for k in 0..count {
            // the speed falls off evenly, so the stamps fade as the brush slows
            let left = 1.0 - k as f32 / count as f32;
            at = at + dir * step;
            st.strokes = st.strokes.wrapping_add(1);
            stamps.push(LiquifyStamp {
                centre: [at.x, at.y],
                moved: [dir.x * step, dir.y * step],
                radius,
                pressure: strength * left,
                distortion: brush.distortion.clamp(0.0, 1.0),
                seed: (st.strokes % 997) as f32,
                mode: brush.kind.into(),
            });
        }
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.liquify_stamps(&stamps);
            if let Some(f) = st.last_filter {
                canvas.set_filter(f);
            }
            self.shared.frame(st);
        }
    }

    /// How much of the liquify shows (0..1): the adjust slider.
    pub fn liquify_amount(&self, amount: f32) {
        let a = if amount.is_finite() { amount.clamp(0.0, 1.0) } else { 1.0 };
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.adjust(Filter::Liquify { amount: a });
        self.shared.frame(st);
    }

    /// Everything liquified goes back to where it was.
    pub fn liquify_reset(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.liquify_clear();
            if let Some(f) = st.last_filter {
                canvas.set_filter(f);
            }
            self.shared.frame(st);
        }
    }

    /// Ends the adjustment: `commit` keeps it as one undo step, otherwise the layer goes back.
    pub fn adjust_end(&self, commit: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.adjusting {
            st.end_adjustment(commit);
            st.flush();
            st.revision += 1;
            self.shared.frame(st);
        }
    }

    /// From now on the adjustment shows only where it is painted on with the paint brush.
    pub fn adjust_pencil(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if !st.adjusting || st.pencil {
            return;
        }
        st.pencil = true;
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.begin_pencil();
            if let Some(filter) = st.last_filter {
                canvas.set_filter(filter);
            }
        }
        self.shared.frame(st);
    }

    /// Starts cloning the current layer: painting copies from the source (see
    /// [`Self::clone_source`]); false when the layer cannot take it.
    /// Whether leaving an adjustment makes the steps made in it one undo step (on at first).
    pub fn set_simplified_undos(&self, on: bool) {
        self.shared.lock().simplified_undos = on;
    }

    pub fn clone_begin(&self) -> bool {
        if !self.adjust_begin() {
            return false;
        }
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.pencil = true;
        st.cloning = true;
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.begin_pencil();
        }
        st.adjust(Filter::Clone { offset: [0.0, 0.0] });
        self.shared.frame(st);
        true
    }

    /// The clone source moved to window point (`x`, `y`): the next stroke copies from there.
    pub fn clone_source(&self, x: f32, y: f32) {
        let mut st = self.shared.lock();
        if let Some(p) = st.view.and_then(|v| v.screen_to_canvas(Vec2::new(x, y))).filter(|p| p.is_finite()) {
            st.clone_source = Some(p);
            st.clone_offset = None;
        }
    }

    /// While `original`, the layer shows as it was before the adjustment; then the adjustment again.
    pub fn adjust_preview(&self, original: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if !st.adjusting {
            return;
        }
        // nothing sharpened is the layer as it was
        let filter = if original { Some(Filter::Sharpen { amount: 0.0 }) } else { st.last_filter };
        if let (Some(f), Some(canvas)) = (filter, st.canvas.as_mut()) {
            canvas.set_filter(f);
            self.shared.frame(st);
        }
    }

    pub fn is_adjusting(&self) -> bool {
        self.shared.lock().adjusting
    }
}
