//! An adjustment open on the current layer, the same in every app: what it starts at, its values
//! as its bar and panels show them, and how each change redraws it (the filters themselves are in
//! adjust.rs). The app opens and ends it and shows its values; the touches drag its amount.
//! Noise, bloom, halftone, chromatic aberration and glitch keep their settings from one adjustment
//! to the next; the amount, HSB, colour balance and curves start afresh, and perspective blur
//! positional, its puck in the canvas's middle and its arrow up.

use std::sync::Arc;

use procraft_engine::geom::Vec2;
use procraft_engine::view::View;

use crate::CanvasSession;
use crate::adjust::{
    ChromaticKind, GlitchKind, HalftoneKind, LiquifyBrush, LiquifyKind, MOTION_DRAG_PT, NoiseKind, PerspectiveKind,
    SwipeAdjustment,
};
use crate::adjust_menu::{Adjustment, adjustment_paintable, adjustment_pencil_amount};
use crate::gradients::Gradients;

/// Painted on, bloom starts smaller and burns more.
const PENCIL_BLOOM_SIZE: f32 = 0.335;
const PENCIL_BLOOM_BURN: f32 = 0.57;
/// How close (0..1) a curve's point may come to its neighbours.
const CURVE_GAP: f32 = 0.01;
/// The clone source's ring (points from its centre), and how far off it a touch still takes it.
pub const CLONE_PUCK_RADIUS: f32 = 30.0;
pub(crate) const CLONE_PUCK_SLOP: f32 = 8.0;
/// Perspective blur's puck takes a drag that starts this near it (points): positional's moves it;
/// directional's within the inner ring moves it, and out to the reach turns the arrow when the drag
/// starts on the arrow's side (within a right angle of it) and moves the puck otherwise.
pub(crate) const POSITIONAL_GRAB: f32 = 50.0;
pub(crate) const DIRECTIONAL_INNER: f32 = 40.0;
pub(crate) const DIRECTIONAL_REACH: f32 = 110.0;
/// Perspective blur's arrow at first: up the window. The point along it given for drawing lies
/// this far from the puck (canvas pixels).
/// How many ways liquify has.
const LIQUIFY_KINDS: usize = 8;
const ARROW_UP: f32 = -std::f32::consts::FRAC_PI_2;
const ARROW_TIP: f32 = 100.0;

/// Each glitch kind's sliders as they start (the last its zoom).
fn glitch_defaults() -> [Vec<f32>; 4] {
    [vec![1.0, 0.09, 1.0], vec![0.2, 0.3, 1.0], vec![0.8, 0.19, 1.0], vec![0.98, -0.5, 0.85, 0.5]]
}

fn straight() -> Vec<(f32, f32)> {
    vec![(0.0, 0.0), (1.0, 1.0)]
}

/// What an open adjustment shows: its values as its bar and panels show them.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AdjustShown {
    pub adjustment: Option<Adjustment>,
    /// The amount (0..1) swiped, dragged or painted.
    pub amount: f32,
    /// Painted on with the brush (Pencil), and the layer shown as it was before.
    pub pencil: bool,
    pub preview: bool,
    /// Hue, saturation and brightness, each 0..1 (0.5 none).
    pub hsb: Vec<f32>,
    /// Colour balance: the tone shown (0 shadows, 1 midtones, 2 highlights) and its nine sliders,
    /// red, green and blue for each tone (0..1, 0.5 none).
    pub balance_tone: u32,
    pub balance: Vec<f32>,
    /// Curves: the channel shown (0 all, 1 red, 2 green, 3 blue) and each channel's points as
    /// x, y pairs (0..1).
    pub curve_channel: u32,
    pub curves: Vec<Vec<f32>>,
    /// The gradient mapped by (its place in the library).
    pub gradient: u32,
    pub noise_kind: NoiseKind,
    /// Noise's scale, octaves and turbulence (0..1); each channel its own; added or mixed in.
    pub noise: Vec<f32>,
    pub noise_multi: bool,
    pub noise_added: bool,
    /// Bloom's transition, size and burn (0..1).
    pub bloom: Vec<f32>,
    pub halftone_kind: HalftoneKind,
    pub chroma_kind: ChromaticKind,
    /// The chromatic kind shown's two sliders.
    pub chroma: Vec<f32>,
    pub glitch_kind: GlitchKind,
    /// The glitch kind shown's sliders (0..1, or −1..1 for diverge's shifts; the last its zoom).
    pub glitch: Vec<f32>,
    /// Liquify: its way, its size, pressure, distortion and momentum (0..1), how much of what it
    /// did shows (Adjust), and whether it has done anything yet.
    pub liquify_kind: LiquifyKind,
    pub liquify: Vec<f32>,
    pub liquify_amount: f32,
    pub liquified: bool,
    /// Cloning: where the source is in the window (x, y; none when not cloning or off the view).
    pub clone_puck: Option<Vec<f32>>,
    /// Perspective blur: all round or towards its arrow, and (on the layer, not painted on) its
    /// puck in the window and a point along its arrow there (x, y, then x, y; window pixels).
    pub perspective_kind: PerspectiveKind,
    pub perspective_puck: Option<Vec<f32>>,
    /// Anything was changed since it opened (or was last kept).
    pub changed: bool,
    /// A plugin's filter: its name, and its controls for one panel to draw.
    pub plugin: Option<crate::plugin_filter::PluginFilterShown>,
}

pub(crate) struct Adjusting {
    open: Option<Adjustment>,
    amount: f32,
    pencil: bool,
    preview: bool,
    hsb: [f32; 3],
    balance_tone: usize,
    balance: [f32; 9],
    curve_channel: usize,
    curves: [Vec<(f32, f32)>; 4],
    gradient: usize,
    gradients: Option<Arc<Gradients>>,
    noise_kind: NoiseKind,
    noise: [f32; 3],
    noise_multi: bool,
    noise_added: bool,
    bloom: [f32; 3],
    halftone_kind: HalftoneKind,
    chroma_kind: ChromaticKind,
    chroma: [[f32; 2]; 2],
    /// Displacing: how far the fringes moved (window pixels), and from where this drag began.
    chroma_offset: (f32, f32),
    chroma_from: (f32, f32),
    glitch_kind: GlitchKind,
    glitch: [Vec<f32>; 4],
    liquify_kind: LiquifyKind,
    /// Size, pressure, distortion and momentum; each way keeps its own distortion.
    liquify: [f32; 4],
    liquify_distortions: [f32; LIQUIFY_KINDS],
    liquify_amount: f32,
    liquified: bool,
    /// Changed since it opened (a slider, a choice, a liquify stroke): undo takes that back.
    touched: bool,
    /// Cloning: the source (canvas pixels), how far it is from the brush once a stroke fixed that,
    /// and where it was before each stroke kept and each undone (for undo and redo).
    puck: Option<Vec2>,
    puck_offset: Option<Vec2>,
    puck_before: Vec<Vec2>,
    puck_after: Vec<Vec2>,
    /// Perspective blur: its way, its puck and arrow, and where they were before each drag on the
    /// puck and each undone (for undo and redo).
    perspective_kind: PerspectiveKind,
    perspective: Perspective,
    perspective_before: Vec<Perspective>,
    perspective_after: Vec<Perspective>,
}

/// Perspective blur's puck: where it is (canvas pixels; none for the canvas's middle), and the way
/// its arrow points (a canvas angle, radians).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Perspective {
    origin: Option<Vec2>,
    arrow: f32,
}

impl Default for Perspective {
    fn default() -> Perspective {
        Perspective { origin: None, arrow: ARROW_UP }
    }
}

impl Default for Adjusting {
    fn default() -> Adjusting {
        Adjusting {
            open: None,
            amount: 0.0,
            pencil: false,
            preview: false,
            hsb: [0.5; 3],
            balance_tone: 1,
            balance: [0.5; 9],
            curve_channel: 0,
            curves: [straight(), straight(), straight(), straight()],
            gradient: 0,
            gradients: None,
            noise_kind: NoiseKind::Clouds,
            noise: [0.0, 0.0, 0.7],
            noise_multi: false,
            noise_added: true,
            bloom: [1.0, 0.3, 0.35],
            halftone_kind: HalftoneKind::FullColor,
            chroma_kind: ChromaticKind::Perspective,
            chroma: [[0.0; 2]; 2],
            chroma_offset: (0.0, 0.0),
            chroma_from: (0.0, 0.0),
            glitch_kind: GlitchKind::Artifact,
            glitch: glitch_defaults(),
            liquify_kind: LiquifyKind::Push,
            liquify: [0.5, 1.0, 0.0, 0.0],
            liquify_distortions: [0.0; LIQUIFY_KINDS],
            liquify_amount: 1.0,
            liquified: false,
            touched: false,
            puck: None,
            puck_offset: None,
            puck_before: Vec::new(),
            puck_after: Vec::new(),
            perspective_kind: PerspectiveKind::Positional,
            perspective: Perspective::default(),
            perspective_before: Vec::new(),
            perspective_after: Vec::new(),
        }
    }
}

impl Adjusting {
    /// The adjustment open, and whether it is painted on (Pencil).
    pub(crate) fn open(&self) -> Option<(Adjustment, bool)> {
        self.open.map(|a| (a, self.pencil))
    }

    /// One open as if begun (for the touches' tests, which have no canvas to begin it on).
    #[cfg(test)]
    pub(crate) fn opened(adjustment: Adjustment) -> Adjusting {
        Adjusting { open: Some(adjustment), ..Adjusting::default() }
    }

    /// The liquify brush as it is set.
    pub(crate) fn liquify_brush(&self) -> LiquifyBrush {
        let [size, pressure, distortion, _] = self.liquify;
        LiquifyBrush { kind: self.liquify_kind, size, pressure, distortion }
    }

    pub(crate) fn liquify_momentum(&self) -> f32 {
        self.liquify[3]
    }

    /// Cloning, undo puts the source back where it was before the last stroke, and redo where
    /// that stroke took it.
    pub(crate) fn puck_step(&mut self, undo: bool) {
        if self.open != Some(Adjustment::Clone) {
            return;
        }
        let (from, to) = if undo {
            (&mut self.puck_before, &mut self.puck_after)
        } else {
            (&mut self.puck_after, &mut self.puck_before)
        };
        if let Some(p) = from.pop() {
            to.extend(self.puck.replace(p));
        }
    }

    /// Perspective blur, undo puts its puck and arrow back as they were before the last drag on it,
    /// and redo as that drag left them. Returns whether there was one to step.
    fn perspective_step(&mut self, undo: bool) -> bool {
        if self.open != Some(Adjustment::PerspectiveBlur) {
            return false;
        }
        let (from, to) = if undo {
            (&mut self.perspective_before, &mut self.perspective_after)
        } else {
            (&mut self.perspective_after, &mut self.perspective_before)
        };
        let Some(p) = from.pop() else { return false };
        to.push(std::mem::replace(&mut self.perspective, p));
        true
    }

    /// Chromatic aberration displacing: a drag moves the fringes.
    pub(crate) fn displacing(&self) -> bool {
        self.open == Some(Adjustment::ChromaticAberration) && self.chroma_kind == ChromaticKind::Displace
    }

    /// A plugin's filter is open, as if just begun.
    pub(crate) fn open_plugin(&mut self) {
        self.open = Some(Adjustment::Plugin);
        self.pencil = false;
        self.preview = false;
        self.touched = false;
    }

    /// The plugin's filter open has stopped.
    pub(crate) fn close_plugin(&mut self) {
        self.open = None;
        self.preview = false;
    }

    /// Something open was changed.
    pub(crate) fn touch(&mut self) {
        self.touched = true;
    }

    /// Open, and changed since it opened: undo takes that back.
    pub(crate) fn touched(&self) -> bool {
        self.open.is_some() && self.touched
    }

    fn changed(&self) -> bool {
        match self.open {
            Some(Adjustment::Hsb) => self.hsb.iter().any(|&v| v != 0.5),
            Some(Adjustment::ColorBalance) => self.balance.iter().any(|&v| v != 0.5),
            Some(Adjustment::Curves) => self.curves.iter().any(|c| *c != straight()),
            Some(Adjustment::GradientMap | Adjustment::Plugin) => true,
            Some(Adjustment::PerspectiveBlur) => self.amount > 0.0 || !self.perspective_before.is_empty(),
            _ => self.amount > 0.0,
        }
    }

    fn shown(&self, view: Option<View>, middle: Vec2) -> AdjustShown {
        AdjustShown {
            adjustment: self.open,
            amount: self.amount,
            pencil: self.pencil,
            preview: self.preview,
            hsb: self.hsb.to_vec(),
            balance_tone: self.balance_tone as u32,
            balance: self.balance.to_vec(),
            curve_channel: self.curve_channel as u32,
            curves: self.curves.iter().map(|c| c.iter().flat_map(|&(x, y)| [x, y]).collect()).collect(),
            gradient: self.gradient as u32,
            noise_kind: self.noise_kind,
            noise: self.noise.to_vec(),
            noise_multi: self.noise_multi,
            noise_added: self.noise_added,
            bloom: self.bloom.to_vec(),
            halftone_kind: self.halftone_kind,
            chroma_kind: self.chroma_kind,
            chroma: self.chroma.get(self.chroma_kind as usize).map_or_else(Vec::new, |c| c.to_vec()),
            glitch_kind: self.glitch_kind,
            glitch: self.glitch.get(self.glitch_kind as usize).cloned().unwrap_or_default(),
            liquify_kind: self.liquify_kind,
            liquify: self.liquify.to_vec(),
            liquify_amount: self.liquify_amount,
            liquified: self.liquified,
            clone_puck: self.puck.zip(view).map(|(p, v)| {
                let at = v.to_screen().apply(p);
                vec![at.x, at.y]
            }),
            perspective_kind: self.perspective_kind,
            perspective_puck: self.perspective_puck(view, middle),
            changed: self.changed(),
            plugin: None,
        }
    }

    /// Perspective blur on the layer (not painted on): its puck and a point along its arrow in the
    /// window, its puck in the canvas's `middle` until moved.
    fn perspective_puck(&self, view: Option<View>, middle: Vec2) -> Option<Vec<f32>> {
        let v = view.filter(|_| self.open == Some(Adjustment::PerspectiveBlur) && !self.pencil)?;
        let origin = self.perspective.origin.unwrap_or(middle);
        let arrow = Vec2::new(self.perspective.arrow.cos(), self.perspective.arrow.sin());
        let (at, tip) = (v.to_screen().apply(origin), v.to_screen().apply(origin + arrow * ARROW_TIP));
        Some(vec![at.x, at.y, tip.x, tip.y])
    }
}

/// How an adjustment's values redraw it: the filter call to make, worked out under the lock and
/// made after it is let go.
enum Redraw {
    None,
    Swipe(SwipeAdjustment, f32),
    Hsb([f32; 3]),
    Balance([f32; 9]),
    Curves([Vec<f32>; 4]),
    Gradient(Vec<f32>, f32),
    Noise(NoiseKind, f32, [f32; 3], bool, bool),
    Bloom(f32, [f32; 3]),
    Halftone(HalftoneKind, f32),
    Chroma(ChromaticKind, f32, (f32, f32), [f32; 2]),
    Glitch(GlitchKind, f32, Vec<f32>),
    Perspective(PerspectiveKind, f32, Option<[f32; 2]>, f32),
}

impl Adjusting {
    /// What the amount (or the values of the adjustment open) redraws.
    fn redraw(&self) -> Redraw {
        let a = self.amount;
        match self.open {
            Some(Adjustment::GaussianBlur) => Redraw::Swipe(SwipeAdjustment::GaussianBlur, a),
            Some(Adjustment::Sharpen) => Redraw::Swipe(SwipeAdjustment::Sharpen, a),
            Some(Adjustment::PerspectiveBlur) => Redraw::Perspective(
                self.perspective_kind,
                a,
                self.perspective.origin.map(|o| [o.x, o.y]),
                self.perspective.arrow,
            ),
            Some(Adjustment::Hsb) => Redraw::Hsb(self.hsb),
            Some(Adjustment::ColorBalance) => Redraw::Balance(self.balance),
            Some(Adjustment::Curves) => {
                Redraw::Curves(self.curves.clone().map(|c| c.iter().flat_map(|&(x, y)| [x, y]).collect()))
            }
            Some(Adjustment::GradientMap) => {
                let list = self.gradients.as_ref().map(|g| g.list()).unwrap_or_default();
                match list.get(self.gradient) {
                    Some(g) => Redraw::Gradient(Gradients::map_stops(g), a),
                    None => Redraw::None,
                }
            }
            Some(Adjustment::Noise) => {
                Redraw::Noise(self.noise_kind, a, self.noise, self.noise_multi, self.noise_added)
            }
            Some(Adjustment::Bloom) => Redraw::Bloom(a, self.bloom),
            Some(Adjustment::Halftone) => Redraw::Halftone(self.halftone_kind, a),
            Some(Adjustment::ChromaticAberration) => {
                let offset = if self.chroma_kind == ChromaticKind::Displace { self.chroma_offset } else { (0.0, 0.0) };
                let values = self.chroma.get(self.chroma_kind as usize).copied().unwrap_or_default();
                Redraw::Chroma(self.chroma_kind, a, offset, values)
            }
            Some(Adjustment::Glitch) => Redraw::Glitch(
                self.glitch_kind,
                a,
                self.glitch.get(self.glitch_kind as usize).cloned().unwrap_or_default(),
            ),
            _ => Redraw::None,
        }
    }
}

impl CanvasSession {
    /// Undo (`undo`) or redo with an adjustment open on the layer (not painted on): undo takes
    /// back what it changed since it opened and it stays open (named for the notice); with nothing
    /// changed, `step` is made and it opens again on what that leaves. Redo leaves changes made
    /// alone (they would be lost). None when no such adjustment is open.
    pub(crate) fn step_adjusting(
        &self,
        step: impl FnOnce(&Self) -> Option<String>,
        undo: bool,
    ) -> Option<Option<String>> {
        let (open, touched) = self.adjusting(|a, _| (a.open, a.touched));
        let open = open?;
        if self.shared.lock().pencil {
            return None;
        }
        // perspective blur's puck steps back (or on) a drag at a time
        if self.perspective_step(undo) {
            return Some(Some(crate::adjust_menu::adjustment_label(open)));
        }
        if touched {
            if !undo {
                return Some(None);
            }
            self.adjustment_again(open, false);
            return Some(Some(crate::adjust_menu::adjustment_label(open)));
        }
        let kept = self.adjusting(|a, _| (a.perspective_kind, a.perspective));
        self.adjustment_end(false);
        let name = step(self);
        self.adjustment_start(open);
        self.adjusting(|a, _| (a.perspective_kind, a.perspective) = kept);
        Some(name)
    }

    /// Perspective blur's puck and arrow stepped back (`undo`) or on a drag, and the blur redrawn.
    /// Returns whether there was one to step.
    fn perspective_step(&self, undo: bool) -> bool {
        let redraw = self.adjusting(|a, _| a.perspective_step(undo).then(|| a.redraw()));
        redraw.map(|r| self.redraw_adjustment(r)).is_some()
    }

    /// Opens the adjustment open again from nothing, or (`commit`) keeping what it did as one undo
    /// step. Perspective blur keeps its way, and committed, its puck and arrow.
    fn adjustment_again(&self, open: Adjustment, commit: bool) {
        let (kind, puck) = self.adjusting(|a, _| (a.perspective_kind, a.perspective));
        self.adjustment_end(commit);
        self.adjustment_start(open);
        self.adjusting(|a, _| {
            a.perspective_kind = kind;
            if commit {
                a.perspective = puck;
            }
        });
    }

    fn adjusting<R>(&self, f: impl FnOnce(&mut Adjusting, (u32, u32)) -> R) -> R {
        let mut st = self.shared.lock();
        let window = st.window_size;
        f(&mut st.adjust, window)
    }

    fn redraw_adjustment(&self, redraw: Redraw) {
        match redraw {
            Redraw::None => {}
            Redraw::Swipe(kind, amount) => self.adjust_swipe(kind, amount),
            Redraw::Hsb([h, s, b]) => self.adjust_hsb(h, s, b),
            Redraw::Balance(v) => {
                let v: Vec<f32> = v.iter().map(|x| (x - 0.5) * 2.0).collect();
                let tone = |k: usize| v.get(k * 3..k * 3 + 3).map(<[f32]>::to_vec).unwrap_or_default();
                self.adjust_color_balance(tone(0), tone(1), tone(2));
            }
            Redraw::Curves([all, red, green, blue]) => self.adjust_curves(all, red, green, blue),
            Redraw::Gradient(stops, amount) => self.adjust_gradient_map(stops, amount),
            Redraw::Noise(kind, amount, [scale, octaves, turbulence], multi, added) => {
                self.adjust_noise(kind, amount, scale, octaves, turbulence, multi, added);
            }
            Redraw::Bloom(amount, [transition, size, burn]) => self.adjust_bloom(amount, transition, size, burn),
            Redraw::Halftone(kind, amount) => self.adjust_halftone(kind, amount),
            Redraw::Chroma(kind, amount, (dx, dy), [a, b]) => self.adjust_chromatic(kind, amount, dx, dy, a, b),
            Redraw::Glitch(kind, amount, mut values) => {
                let zoom = values.pop().unwrap_or(1.0);
                self.adjust_glitch(kind, amount, values, zoom);
            }
            Redraw::Perspective(kind, amount, origin, arrow) => self.adjust_perspective(kind, amount, origin, arrow),
        }
    }

    /// Changes the open adjustment's values with `f` and redraws it.
    fn change_adjustment(&self, f: impl FnOnce(&mut Adjusting, (u32, u32))) {
        let redraw = self.adjusting(|a, window| {
            if a.open.is_none() {
                return Redraw::None;
            }
            a.touched = true;
            f(a, window);
            a.redraw()
        });
        self.redraw_adjustment(redraw);
    }
}

#[uniffi::export]
impl CanvasSession {
    /// The gradient library Gradient Map maps by.
    pub fn use_gradients(&self, gradients: Arc<Gradients>) {
        self.adjusting(|a, _| a.gradients = Some(gradients));
    }

    /// Opens `adjustment` on the current layer (one already open kept first); false when the layer
    /// can't be adjusted. The amount starts at none (a gradient map's at all of it), HSB, colour
    /// balance and curves as they were; the other adjustments keep their settings.
    pub fn adjustment_start(&self, adjustment: Adjustment) -> bool {
        // Liquify leaves an alpha locked layer alone
        if self.why_not_adjusted(adjustment) == Some(crate::layers::Blocked::AlphaLocked) {
            return false;
        }
        self.adjustment_end(true);
        let begun = match adjustment {
            Adjustment::Liquify => self.liquify_begin(),
            Adjustment::Clone => self.clone_begin(),
            Adjustment::Plugin => return self.plugin_filter_again(),
            _ => self.adjust_begin(),
        };
        if !begun {
            return false;
        }
        // these work on the texture, not through the camera; the others keep to the meshes picked
        if matches!(adjustment, Adjustment::Liquify | Adjustment::Clone) {
            self.shared.lock().scene_flat_for_tool();
        } else {
            self.shared.lock().scene_selection_for_adjustment();
        }
        let up = self.shared.lock().canvas_angle(ARROW_UP);
        let redraw = self.adjusting(|a, _| {
            a.open = Some(adjustment);
            a.amount = if adjustment == Adjustment::GradientMap { 1.0 } else { 0.0 };
            a.pencil = adjustment == Adjustment::Clone;
            a.preview = false;
            a.hsb = [0.5; 3];
            a.balance = [0.5; 9];
            a.curves = [straight(), straight(), straight(), straight()];
            a.chroma_offset = (0.0, 0.0);
            a.liquified = false;
            a.touched = false;
            a.liquify_amount = 1.0;
            a.puck = None;
            a.puck_offset = None;
            a.puck_before.clear();
            a.puck_after.clear();
            a.perspective_kind = PerspectiveKind::Positional;
            a.perspective = Perspective { origin: None, arrow: up };
            a.perspective_before.clear();
            a.perspective_after.clear();
            // a gradient map and a glitch show at once
            if matches!(adjustment, Adjustment::GradientMap | Adjustment::Glitch) { a.redraw() } else { Redraw::None }
        });
        self.redraw_adjustment(redraw);
        if adjustment == Adjustment::Clone {
            // the source starts in the middle of the window
            let (w, h) = self.shared.lock().window_size;
            self.adjustment_clone_move(w as f32 / 2.0, h as f32 / 2.0);
        }
        true
    }

    /// Ends the open adjustment: `commit` keeps it as one undo step, otherwise the layer goes back.
    /// Returns whether one was open.
    pub fn adjustment_end(&self, commit: bool) -> bool {
        let open = self.adjusting(|a, _| {
            a.preview = false;
            a.pencil = false;
            a.puck = None;
            a.open.take().is_some()
        });
        if open {
            self.adjust_end(commit);
            let mut st = self.shared.lock();
            st.scene_selection_back();
            st.scene_back_from_tool();
        }
        open
    }

    /// What the open adjustment shows.
    pub fn adjustment_shown(&self) -> AdjustShown {
        let st = self.shared.lock();
        let shown = st.adjust.shown(st.view, Vec2::new(st.doc.width() as f32 / 2.0, st.doc.height() as f32 / 2.0));
        AdjustShown { plugin: st.filters.shown(), ..shown }
    }

    /// The amount moved by `step` (a swipe's), within 0..1.
    pub fn adjustment_amount_by(&self, step: f32) {
        self.change_adjustment(|a, _| a.amount = (a.amount + step).clamp(0.0, 1.0));
    }

    /// A motion blur's drag reached (`dx`, `dy`) window pixels from where it began: along it,
    /// longer the further it goes.
    pub fn adjustment_motion(&self, dx: f32, dy: f32) {
        if self.adjusting(|a, _| a.open == Some(Adjustment::MotionBlur)) {
            let amount = self.adjust_motion(dx, dy);
            self.adjusting(|a, _| a.amount = amount);
        }
    }

    pub fn adjustment_hsb(&self, hue: f32, saturation: f32, brightness: f32) {
        self.change_adjustment(|a, _| a.hsb = [hue, saturation, brightness].map(|v| v.clamp(0.0, 1.0)));
    }

    /// Colour balance's tone shown: 0 shadows, 1 midtones, 2 highlights.
    pub fn adjustment_balance_tone(&self, tone: u32) {
        self.adjusting(|a, _| a.balance_tone = (tone as usize).min(2));
    }

    /// Slider `k` (0 red, 1 green, 2 blue) of the tone shown moved to `value` (0..1).
    pub fn adjustment_balance(&self, k: u32, value: f32) {
        self.change_adjustment(|a, _| {
            let i = a.balance_tone * 3 + (k as usize).min(2);
            if let Some(v) = a.balance.get_mut(i) {
                *v = value.clamp(0.0, 1.0);
            }
        });
    }

    /// Curves' channel shown: 0 all, 1 red, 2 green, 3 blue.
    pub fn adjustment_curve_channel(&self, channel: u32) {
        self.adjusting(|a, _| a.curve_channel = (channel as usize).min(3));
    }

    /// Adds a point to the channel shown; returns its index.
    pub fn adjustment_curve_add(&self, x: f32, y: f32) -> u32 {
        let mut at = 0;
        self.change_adjustment(|a, _| {
            let Some(points) = a.curves.get_mut(a.curve_channel) else { return };
            let p = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
            at = points.partition_point(|q| q.0 <= p.0);
            points.insert(at, p);
        });
        at as u32
    }

    /// Moves point `index` of the channel shown to (`x`, `y`), kept between its neighbours.
    pub fn adjustment_curve_move(&self, index: u32, x: f32, y: f32) {
        self.change_adjustment(|a, _| {
            let Some(points) = a.curves.get_mut(a.curve_channel) else { return };
            let i = index as usize;
            if i >= points.len() {
                return;
            }
            let lo = if i == 0 { 0.0 } else { points.get(i - 1).map_or(0.0, |p| p.0 + CURVE_GAP) };
            let hi = points.get(i + 1).map_or(1.0, |p| p.0 - CURVE_GAP);
            if let Some(p) = points.get_mut(i) {
                *p = (x.clamp(lo, lo.max(hi)), y.clamp(0.0, 1.0));
            }
        });
    }

    /// Takes point `index` off the channel shown (a curve keeps two).
    pub fn adjustment_curve_delete(&self, index: u32) {
        self.change_adjustment(|a, _| {
            if let Some(points) = a.curves.get_mut(a.curve_channel)
                && points.len() > 2
                && (index as usize) < points.len()
            {
                points.remove(index as usize);
            }
        });
    }

    /// The gradient (by its place in the library) mapped by; also redraws after the library
    /// changed.
    pub fn adjustment_gradient(&self, index: u32) {
        self.change_adjustment(|a, _| {
            let count = a.gradients.as_ref().map_or(1, |g| g.list().len()).max(1);
            a.gradient = (index as usize).min(count - 1);
        });
    }

    pub fn adjustment_noise_kind(&self, kind: NoiseKind) {
        self.change_adjustment(|a, _| a.noise_kind = kind);
    }

    /// Noise's slider `index` (0 scale, 1 octaves, 2 turbulence) moved to `value` (0..1).
    pub fn adjustment_noise_value(&self, index: u32, value: f32) {
        self.change_adjustment(|a, _| {
            if let Some(v) = a.noise.get_mut(index as usize) {
                *v = value.clamp(0.0, 1.0);
            }
        });
    }

    /// Noise each channel its own (`multi`) or one for all; added to the layer or mixed in.
    pub fn adjustment_noise_ways(&self, multi: bool, added: bool) {
        self.change_adjustment(|a, _| {
            a.noise_multi = multi;
            a.noise_added = added;
        });
    }

    /// Bloom's slider `index` (0 transition, 1 size, 2 burn) moved to `value` (0..1).
    pub fn adjustment_bloom_value(&self, index: u32, value: f32) {
        self.change_adjustment(|a, _| {
            if let Some(v) = a.bloom.get_mut(index as usize) {
                *v = value.clamp(0.0, 1.0);
            }
        });
    }

    pub fn adjustment_halftone_kind(&self, kind: HalftoneKind) {
        self.change_adjustment(|a, _| a.halftone_kind = kind);
    }

    pub fn adjustment_chroma_kind(&self, kind: ChromaticKind) {
        self.change_adjustment(|a, _| a.chroma_kind = kind);
    }

    /// Chromatic aberration's slider `index` of the kind shown moved to `value` (0..1).
    pub fn adjustment_chroma_value(&self, index: u32, value: f32) {
        self.change_adjustment(|a, _| {
            if let Some(v) = a.chroma.get_mut(a.chroma_kind as usize).and_then(|c| c.get_mut(index as usize)) {
                *v = value.clamp(0.0, 1.0);
            }
        });
    }

    /// Displacing: a drag (`dx`, `dy`) window pixels from where it began (`start` at its beginning)
    /// moves the fringes on; how far shows as the amount, a quarter of the window's short side all
    /// of it.
    pub fn adjustment_chroma_drag(&self, dx: f32, dy: f32, start: bool) {
        self.change_adjustment(|a, (w, h)| {
            if start {
                a.chroma_from = a.chroma_offset;
            }
            a.chroma_offset = (a.chroma_from.0 + dx, a.chroma_from.1 + dy);
            let whole = w.min(h) as f32 / 4.0;
            let far = a.chroma_offset.0.hypot(a.chroma_offset.1);
            a.amount = if whole > 0.0 { (far / whole).clamp(0.0, 1.0) } else { 0.0 };
        });
    }

    pub fn adjustment_glitch_kind(&self, kind: GlitchKind) {
        self.change_adjustment(|a, _| a.glitch_kind = kind);
    }

    /// Slider `index` of the glitch kind shown moved to `value` (0..1, or −1..1 for diverge's
    /// shifts).
    pub fn adjustment_glitch_value(&self, index: u32, value: f32) {
        self.change_adjustment(|a, _| {
            if let Some(v) = a.glitch.get_mut(a.glitch_kind as usize).and_then(|g| g.get_mut(index as usize)) {
                *v = value.clamp(-1.0, 1.0);
            }
        });
    }

    /// The layer shown as it was before the adjustment, or as adjusted.
    pub fn adjustment_preview(&self, original: bool) {
        if self.adjusting(|a, _| a.open.map(|_| a.preview = original)).is_some() {
            self.adjust_preview(original);
        }
    }

    /// Every value back to where the adjustment started.
    pub fn adjustment_reset(&self) {
        if let Some(open) = self.adjusting(|a, _| a.open) {
            self.adjustment_end(false);
            self.adjustment_start(open);
        }
    }

    /// Keeps what the adjustment did as one undo step and starts it again from there.
    pub fn adjustment_apply(&self) {
        match self.adjusting(|a, _| a.open) {
            // it draws as it opens: opened again, it would go over what it did
            Some(Adjustment::Plugin) => {
                self.adjustment_end(true);
            }
            Some(open) => self.adjustment_again(open, true),
            None => {}
        }
    }

    /// Takes back the adjustment's changes (perspective blur's puck a drag at a time), or with
    /// none, the last one kept.
    pub fn adjustment_undo(&self) {
        let Some((open, changed)) = self.adjusting(|a, _| a.open.map(|o| (o, a.changed()))) else { return };
        if self.perspective_step(true) {
            return;
        }
        if changed {
            self.adjustment_again(open, false);
            return;
        }
        let kept = self.adjusting(|a, _| (a.perspective_kind, a.perspective));
        self.adjustment_end(false);
        self.undo();
        self.adjustment_start(open);
        self.adjusting(|a, _| (a.perspective_kind, a.perspective) = kept);
    }

    /// Layer or Pencil: painting the adjustment on starts it at its own amount, and only where the
    /// brush goes shows it; back to the layer starts it again.
    pub fn adjustment_pencil(&self, pencil: bool) {
        let Some((open, was)) = self.adjusting(|a, _| a.open.map(|o| (o, a.pencil))) else { return };
        if pencil == was || !adjustment_paintable(open) {
            return;
        }
        if !pencil {
            self.adjustment_reset();
            return;
        }
        self.adjust_pencil();
        self.adjusting(|a, _| a.pencil = true);
        let Some(amount) = adjustment_pencil_amount(open) else { return };
        if open == Adjustment::Bloom {
            self.adjusting(|a, _| {
                a.bloom[1] = PENCIL_BLOOM_SIZE;
                a.bloom[2] = PENCIL_BLOOM_BURN;
            });
        }
        if open == Adjustment::MotionBlur {
            // along the canvas, as long as a drag of that much
            let scale = self.shared.lock().display_scale;
            self.adjustment_motion(amount * MOTION_DRAG_PT * scale, 0.0);
        } else {
            self.change_adjustment(|a, _| a.amount = amount);
        }
    }

    /// Perspective blur all round its puck, or only towards its arrow.
    pub fn adjustment_perspective_kind(&self, kind: PerspectiveKind) {
        let redraw = self.adjusting(|a, _| {
            if a.open != Some(Adjustment::PerspectiveBlur) {
                return Redraw::None;
            }
            a.perspective_kind = kind;
            a.redraw()
        });
        self.redraw_adjustment(redraw);
    }

    /// Liquify's way.
    pub fn adjustment_liquify_kind(&self, kind: LiquifyKind) {
        self.adjusting(|a, _| {
            a.liquify_kind = kind;
            a.liquify[2] = a.liquify_distortions.get(kind as usize).copied().unwrap_or(0.0);
        });
    }

    /// Liquify's slider `index` (0 size, 1 pressure, 2 distortion, 3 momentum) moved to `value`.
    pub fn adjustment_liquify_value(&self, index: u32, value: f32) {
        self.adjusting(|a, _| {
            if let Some(v) = a.liquify.get_mut(index as usize) {
                *v = value.clamp(0.0, 1.0);
            }
            if index == 2
                && let Some(v) = a.liquify_distortions.get_mut(a.liquify_kind as usize)
            {
                *v = value.clamp(0.0, 1.0);
            }
        });
    }

    /// How much of what liquify did shows (0..1).
    pub fn adjustment_liquify_amount(&self, amount: f32) {
        let amount = amount.clamp(0.0, 1.0);
        self.adjusting(|a, _| a.liquify_amount = amount);
        self.liquify_amount(amount);
    }

    /// Everything liquify did undone.
    pub fn adjustment_liquify_reset(&self) {
        self.liquify_reset();
        self.adjusting(|a, _| {
            a.liquified = false;
            a.liquify_amount = 1.0;
        });
    }

    /// A liquify stroke's window points (x, y, pressure, milliseconds since the last, each), a
    /// pen's if `pen`; `start` begins it.
    pub fn adjustment_liquify_stroke(&self, points: Vec<f32>, start: bool, pen: bool) {
        let brush = self.adjusting(|a, _| {
            a.liquified = true;
            a.touched = true;
            a.liquify_brush()
        });
        self.liquify_stroke(points, start, pen, brush);
    }

    /// The liquify brush held still at window point (`x`, `y`) works on, from `from` to `to`
    /// seconds of being held. False if the kind of brush does nothing held.
    pub fn adjustment_liquify_held(&self, x: f32, y: f32, from: f32, to: f32) -> bool {
        let brush = self.adjusting(|a, _| a.liquify_brush());
        self.liquify_held(x, y, from, to, brush)
    }

    /// The liquify brush lifts: true if it, or what it threw off, goes on.
    pub fn adjustment_liquify_lift(&self) -> bool {
        let (brush, momentum) = self.adjusting(|a, _| (a.liquify_brush(), a.liquify_momentum()));
        self.liquify_lift(momentum, brush)
    }

    /// `seconds` go by for what liquify set going: true if it goes on. Nothing does once it closes.
    pub fn adjustment_liquify_tick(&self, seconds: f32) -> bool {
        let (open, brush) = self.adjusting(|a, _| (a.open, a.liquify_brush()));
        open == Some(Adjustment::Liquify) && self.liquify_tick(seconds, brush)
    }

    /// Cloning, the source moved to window point (`x`, `y`): the next stroke fixes how far from it
    /// the brush is.
    pub fn adjustment_clone_move(&self, x: f32, y: f32) {
        {
            let mut st = self.shared.lock();
            let Some(at) = st.window_to_canvas(x, y) else { return };
            st.adjust.puck = Some(at);
            st.adjust.puck_offset = None;
        }
        self.clone_source(x, y);
    }

    /// The curve of `channel` as heights to draw, 256 across.
    pub fn adjustment_curve_shape(&self, channel: u32) -> Vec<f32> {
        let points = self.adjusting(|a, _| a.curves.get(channel as usize).cloned().unwrap_or_else(straight));
        self.curve_shape(points.iter().flat_map(|&(x, y)| [x, y]).collect())
    }
}

impl CanvasSession {
    /// Cloning, a stroke reached window point `at` (`start`: it begins there): the source follows
    /// the brush, as far from it as the first stroke after it moved put it.
    pub(crate) fn clone_stroke_at(&self, at: (f32, f32), start: bool) {
        let mut st = self.shared.lock();
        let Some(puck) = st.adjust.puck else { return };
        let Some(at) = st.window_to_canvas(at.0, at.1) else { return };
        let a = &mut st.adjust;
        if start {
            a.puck_before.push(puck);
            a.puck_after.clear();
        }
        let offset = *a.puck_offset.get_or_insert(puck - at);
        a.puck = Some(at + offset);
    }

    /// Cloning, where the source is in the window.
    pub(crate) fn clone_puck_at(&self) -> Option<(f32, f32)> {
        let st = self.shared.lock();
        let at = st.view?.to_screen().apply(st.adjust.puck?);
        Some((at.x, at.y))
    }

    /// Perspective blur on the layer (not painted on): its way, where its puck is in the window,
    /// and the way its arrow points there (radians).
    pub(crate) fn perspective_puck_at(&self) -> Option<(PerspectiveKind, (f32, f32), f32)> {
        let shown = self.adjustment_shown();
        let &[x, y, tx, ty] = shown.perspective_puck.as_deref()? else { return None };
        Some((shown.perspective_kind, (x, y), (ty - y).atan2(tx - x)))
    }

    /// A drag on perspective blur's puck begins: undo puts it back as it is now.
    pub(crate) fn perspective_grabbed(&self) {
        self.adjusting(|a, _| {
            if a.open == Some(Adjustment::PerspectiveBlur) {
                a.perspective_before.push(a.perspective);
                a.perspective_after.clear();
            }
        });
    }

    /// Perspective blur's puck dragged to window point (`x`, `y`).
    pub(crate) fn perspective_move(&self, x: f32, y: f32) {
        let Some(at) = self.shared.lock().window_to_canvas(x, y) else { return };
        self.change_adjustment(|a, _| a.perspective.origin = Some(at));
    }

    /// Perspective blur's arrow turned to point from its puck at window point (`x`, `y`).
    pub(crate) fn perspective_turn(&self, x: f32, y: f32) {
        let away = {
            let st = self.shared.lock();
            let middle = Vec2::new(st.doc.width() as f32 / 2.0, st.doc.height() as f32 / 2.0);
            st.window_to_canvas(x, y).map(|at| at - st.adjust.perspective.origin.unwrap_or(middle))
        };
        if let Some(away) = away.filter(|d| d.length() > 0.0) {
            self.change_adjustment(|a, _| a.perspective.arrow = away.y.atan2(away.x));
        }
    }
}

impl crate::State {
    /// The canvas angle (radians) of the way that points at `angle` in the window.
    fn canvas_angle(&self, angle: f32) -> f32 {
        let way = Vec2::new(angle.cos(), angle.sin());
        self.view
            .and_then(|v| v.to_screen().inverse())
            .map(|inv| inv.apply(way) - inv.apply(Vec2::new(0.0, 0.0)))
            .filter(|d| d.is_finite() && d.length() > 0.0)
            .map_or(angle, |d| d.y.atan2(d.x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_start_as_the_app_ships_them() {
        let a = Adjusting::default();
        let shown = a.shown(None, Vec2::new(32.0, 32.0));
        assert_eq!(shown.adjustment, None);
        assert_eq!(shown.noise, vec![0.0, 0.0, 0.7]);
        assert_eq!(shown.bloom, vec![1.0, 0.3, 0.35]);
        assert_eq!(shown.glitch, vec![1.0, 0.09, 1.0]);
        assert_eq!(shown.curves[0], vec![0.0, 0.0, 1.0, 1.0]);
        assert!(!shown.changed);
    }

    #[test]
    fn what_counts_as_changed() {
        let mut a = Adjusting { open: Some(Adjustment::Hsb), ..Adjusting::default() };
        assert!(!a.changed());
        a.hsb[1] = 0.6;
        assert!(a.changed());
        a.open = Some(Adjustment::GradientMap);
        assert!(a.changed());
        a.open = Some(Adjustment::GaussianBlur);
        assert!(!a.changed());
        a.amount = 0.1;
        assert!(a.changed());
        assert!(matches!(a.redraw(), Redraw::Swipe(SwipeAdjustment::GaussianBlur, x) if x == 0.1));
    }
}
