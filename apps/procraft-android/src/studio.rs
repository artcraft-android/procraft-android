//! The brush studio: the brush the current tool uses, edited control by control while the studio
//! is open (strokes and previews use it as it is), kept or put back when it closes.

use std::sync::Arc;

use procraft_engine::brush::{Brush, MaterialSource, ResetPoint, Source, StockImage};
use procraft_engine::curve::Curve;
use procraft_engine::geom::{Affine, Vec2};
use procraft_engine::studio::{MAX_GRAIN_SCALE, SliderCurve};
use procraft_engine::textures;
use procraft_gpu::{SphereLook, SpherePreview, wgpu};

use crate::{CanvasSession, PenSample, PointerKind, State, ToolKind};

/// The curve through `points` (x, y pairs on the unit square) at `count` (2 to 1024) evenly
/// spaced x from 0 to 1: the curve as strokes apply it, for drawing.
#[uniffi::export]
pub fn curve_samples(points: Vec<f32>, count: u32) -> Vec<f32> {
    let pts: Vec<Vec2> = points.as_chunks::<2>().0.iter().map(|&[x, y]| Vec2::new(x, y)).collect();
    let curve = Curve::new(&pts);
    let n = count.clamp(2, 1024);
    (0..n).map(|i| curve.eval(i as f32 / (n - 1) as f32)).collect()
}

/// A shipped shape or grain image: its name and its pixels (`TEXTURE_SIZE` square greyscale,
/// white = paint).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SourceImage {
    pub name: String,
    pub pixels: Vec<u8>,
}

/// The shipped shape images (or, `grain`, grain images), in order: the source library.
#[uniffi::export]
pub fn source_library(grain: bool) -> Vec<SourceImage> {
    let images = if grain { textures::grains() } else { textures::shapes() };
    images.into_iter().map(|i| SourceImage { name: i.name.to_owned(), pixels: i.pixels }).collect()
}

/// `pixels` (a `source_size` square greyscale image) made to repeat without seams: the grain
/// editor's Auto repeat, the image `scale` times across (1 to 4), turned `rotation_degrees`,
/// faded at its edges over `overlap` of the way in (0..1) as sharply as `hardness` (0..1), into
/// its shifted copy, turned round with `mirror`, fine detail apart with `pyramid`.
#[uniffi::export]
pub fn repeatable_source(
    pixels: Vec<u8>,
    scale: f32,
    rotation_degrees: f32,
    overlap: f32,
    hardness: f32,
    mirror: bool,
    pyramid: bool,
) -> Vec<u8> {
    let repeat =
        textures::Repeat { scale, rotation: rotation_degrees.to_radians(), overlap, hardness, mirror, pyramid };
    textures::repeatable(&pixels, repeat)
}

/// Which of a brush's images the studio's editors change: its shape, its grain, or what it paints a
/// model's metalness or roughness through (tiled as the grain is, from the same library).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SourceKind {
    Shape,
    Grain,
    Metallic,
    Roughness,
}

impl SourceKind {
    /// Whether it draws from the grain images (all but the shape).
    fn grain_like(self) -> bool {
        self != SourceKind::Shape
    }
}

/// What a studio slider's keypad offers besides typing its value: its own pressure curve
/// (`pressure`), its own tilt (`tilt`) with the control of the switch that turns it on (empty: the
/// slider is the tilt's own amount, always on) and of the angle it starts from.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SliderExtrasInfo {
    pub pressure: bool,
    pub tilt: bool,
    pub tilt_switch: String,
    pub tilt_angle: String,
}

/// The keypad extras of the slider for studio control `key`.
#[uniffi::export]
pub fn slider_extras(key: String) -> SliderExtrasInfo {
    let e = procraft_engine::studio::slider_extras(&key);
    SliderExtrasInfo {
        pressure: e.curve.is_some(),
        tilt: e.tilt.is_some(),
        tilt_switch: e.tilt.and_then(|t| t.switch).unwrap_or("").to_owned(),
        tilt_angle: e.tilt.map_or("", |t| t.angle).to_owned(),
    }
}

/// The edge of a shape or grain image, pixels.
#[uniffi::export]
pub fn source_size() -> u32 {
    textures::TEXTURE_SIZE as u32
}

/// One brush's changes from how it ships, as earlier versions kept them: where it is in the
/// library, its name, the controls that differ and, if it differs, its pressure curve.
#[derive(serde::Serialize, serde::Deserialize)]
struct BrushChange {
    set: usize,
    index: usize,
    name: String,
    settings: std::collections::BTreeMap<String, f32>,
    #[serde(default)]
    curve: Vec<[f32; 2]>,
}

impl State {
    /// The brush being edited.
    fn studio_brush(&mut self) -> Option<&mut Brush> {
        let ((set, index), _) = self.studio.as_ref()?;
        let (set, index) = (*set, *index);
        self.library.get_mut(set)?.brushes.get_mut(index)
    }

    /// The brush being edited, about to change: its preview is drawn afresh from now on.
    fn studio_change(&mut self) -> Option<&mut Brush> {
        let brush = self.studio_brush()?;
        brush.thumbnail = None;
        Some(brush)
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Opens the studio on the brush the current tool uses (a studio already open is kept);
    /// returns the brush's name.
    pub fn studio_begin(&self) -> String {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        let at = st.brush_ref(st.tool);
        if st.studio.is_none() {
            // the first time in the studio sets the brush aside, to go back to
            if let Some(brush) = st.library.get_mut(at.0).and_then(|s| s.brushes.get_mut(at.1))
                && brush.reset_point.is_none()
            {
                brush.reset_point = Some(ResetPoint::of(brush, now()));
            }
            st.studio = Some((at, st.brush(st.tool).clone()));
        }
        st.brush(st.tool).name.clone()
    }

    /// Who made the brush being edited, and when its reset point was made.
    pub fn studio_about(&self) -> StudioAbout {
        let mut st = self.shared.lock();
        let Some(brush) = st.studio_brush() else {
            return StudioAbout { author: String::new(), picture: None, signature: None, reset_made: None };
        };
        let author = brush.author.as_ref();
        StudioAbout {
            author: author.map_or_else(String::new, |a| a.name.clone()),
            picture: author.and_then(|a| a.picture.as_deref()).map(<[u8]>::to_vec),
            signature: author.and_then(|a| a.signature.as_deref()).map(<[u8]>::to_vec),
            reset_made: brush.reset_point.as_ref().map(|r| r.made),
        }
    }

    /// Sets the brush being edited aside as it is now, for resetting it to go back to.
    pub fn studio_new_reset_point(&self) {
        let mut st = self.shared.lock();
        if let Some(brush) = st.studio_brush() {
            brush.reset_point = Some(ResetPoint::of(brush, now()));
        }
    }

    /// The values the controls `keys` show (see `procraft_engine::studio`).
    pub fn studio_settings(&self, keys: Vec<String>) -> Vec<f32> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(brush) = st.studio_brush() else { return vec![0.0; keys.len()] };
        keys.iter().map(|k| brush.setting(k)).collect()
    }

    /// Moves the control `key` to `value`.
    pub fn set_studio_setting(&self, key: String, value: f32) {
        let mut st = self.shared.lock();
        if let Some(brush) = st.studio_change() {
            brush.set_setting(&key, value);
            // spacing set by hand is spaced the current way, as a new brush is
            if key == "spacing" {
                brush.set_setting("spacing_version", 1.0);
            }
        }
    }

    /// The image the brush's `kind` of source uses now: its own, else the shipped one
    /// (`source_size` square greyscale, white = paint); empty for the round tip, no grain, or a
    /// material's amount all over.
    pub fn studio_source(&self, kind: SourceKind) -> Vec<u8> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(brush) = st.studio_brush() else { return Vec::new() };
        let (own, shipped) = match kind {
            SourceKind::Shape => (&brush.shape.source, brush.shape.image),
            SourceKind::Grain => (&brush.grain.source, brush.grain.image),
            SourceKind::Metallic => (&brush.metallic.source, brush.metallic.image),
            SourceKind::Roughness => (&brush.roughness.source, brush.roughness.image),
        };
        if let Some(source) = own {
            return source.pixels.to_vec();
        }
        let images = if kind.grain_like() { textures::grains() } else { textures::shapes() };
        shipped.and_then(|i| images.into_iter().nth(i as usize)).map_or_else(Vec::new, |i| i.pixels)
    }

    /// The brush's `kind` of source becomes an image of the user's own (`source_size` square
    /// greyscale, white = paint). False if the pixels aren't that size.
    pub fn set_studio_source(&self, kind: SourceKind, pixels: Vec<u8>) -> bool {
        let Some(source) = Source::new(pixels) else { return false };
        let mut st = self.shared.lock();
        let Some(brush) = st.studio_change() else { return false };
        // an image of the user's own: no longer the stock one its file named
        let (own, _, stock) = sources(brush, kind);
        *own = Some(source);
        *stock = None;
        true
    }

    /// The brush's `kind` of source becomes shipped image `index` of the source library (the
    /// grains for all but the shape).
    pub fn set_studio_shipped_source(&self, kind: SourceKind, index: u32) -> bool {
        let count = if kind.grain_like() { textures::grains().len() } else { textures::shapes().len() };
        if index as usize >= count {
            return false;
        }
        let mut st = self.shared.lock();
        let Some(brush) = st.studio_change() else { return false };
        let (own, shipped, stock) = sources(brush, kind);
        *own = None;
        *shipped = Some(index);
        *stock = None;
        true
    }

    /// Studio control `key`'s own pressure curve, as x, y pairs (0..1); empty while it has none
    /// (or for a slider that can't have one).
    pub fn studio_slider_curve(&self, key: String) -> Vec<f32> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(brush) = st.studio_brush() else { return Vec::new() };
        let curve = match procraft_engine::studio::slider_extras(&key).curve {
            Some(SliderCurve::Own(kind)) => brush.pressure_curves.get(kind),
            // the size's and opacity's own responses: on unless straight
            Some(SliderCurve::Size) => Some(&brush.size_curve).filter(|c| **c != Curve::linear()),
            Some(SliderCurve::Opacity) => Some(&brush.opacity_curve).filter(|c| **c != Curve::linear()),
            None => None,
        };
        curve.map_or_else(Vec::new, |c| c.points().iter().flat_map(|p| [p.x, p.y]).collect())
    }

    /// Gives studio control `key` its own pressure curve through `points` (x, y pairs, 0..1), or
    /// takes it away (`points` empty).
    pub fn set_studio_slider_curve(&self, key: String, points: Vec<f32>) {
        let curve = (points.len() >= 4).then(|| {
            let pts: Vec<Vec2> = points.as_chunks::<2>().0.iter().map(|&[x, y]| Vec2::new(x, y)).collect();
            Curve::new(&pts)
        });
        let mut st = self.shared.lock();
        let Some(brush) = st.studio_change() else { return };
        match procraft_engine::studio::slider_extras(&key).curve {
            Some(SliderCurve::Own(kind)) => brush.pressure_curves.set(kind, curve),
            Some(SliderCurve::Size) => brush.size_curve = curve.unwrap_or_else(Curve::linear),
            Some(SliderCurve::Opacity) => brush.opacity_curve = curve.unwrap_or_else(Curve::linear),
            None => {}
        }
    }

    /// The pressure curve's points, as x, y pairs (0..1).
    pub fn studio_curve(&self) -> Vec<f32> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.studio_brush().map_or_else(Vec::new, |b| b.pressure_curve.points().iter().flat_map(|p| [p.x, p.y]).collect())
    }

    /// The pressure curve through `points` (x, y pairs, 0..1).
    pub fn set_studio_curve(&self, points: Vec<f32>) {
        let pts: Vec<Vec2> = points.as_chunks::<2>().0.iter().map(|&[x, y]| Vec2::new(x, y)).collect();
        let mut st = self.shared.lock();
        if pts.len() >= 2
            && let Some(brush) = st.studio_change()
        {
            brush.pressure_curve = Curve::new(&pts);
        }
    }

    /// The app's own smoothing and pressure for every stroke, each 0..1, and its pressure curve
    /// (x, y pairs, 0..1; fewer than two points keep it straight).
    pub fn set_input_settings(
        &self,
        stabilization: f32,
        motion_filtering: f32,
        motion_expression: f32,
        pressure_smoothing: f32,
        curve: Vec<f32>,
    ) {
        let unit = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        let pts: Vec<Vec2> = curve.as_chunks::<2>().0.iter().map(|&[x, y]| Vec2::new(unit(x), unit(y))).collect();
        let mut st = self.shared.lock();
        st.input = procraft_engine::stroke::InputSettings {
            stabilization: unit(stabilization),
            motion_filtering: unit(motion_filtering),
            motion_expression: unit(motion_expression),
            pressure_smoothing: unit(pressure_smoothing),
            pressure_curve: if pts.len() >= 2 { Curve::new(&pts) } else { Curve::linear() },
            speed_distance: st.input.speed_distance,
        };
    }

    /// How far input must move for the speed dynamics to count it as fast, as a part of the usual
    /// distance (0.25 to 1).
    pub fn set_speed_distance(&self, part: f32) {
        self.shared.lock().input.speed_distance = if part.is_finite() { part.clamp(0.25, 1.0) } else { 1.0 };
    }

    /// The brush back to how it started: as it ships, or as a new brush is (its name kept).
    pub fn studio_reset(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(((set, index), _)) = st.studio.as_ref() else { return };
        let (set, index) = (*set, *index);
        let origin = st.library.get(set).and_then(|s| s.origins.get(index)).copied().flatten();
        let start = st.origin_brush(origin);
        if let Some(brush) = st.studio_brush() {
            let name = std::mem::take(&mut brush.name);
            let reset = brush.reset_point.take();
            // back to its reset point, else to how it started
            let back = reset.as_ref().map_or(start, |r| (*r.brush).clone());
            *brush = Brush { name, reset_point: reset, ..back };
        }
    }

    /// Paints with the brush `other` has open in its studio (else its current tool's brush), as
    /// big as its preview size and in `color` (sRGB 0..1): for a drawing pad.
    pub fn paint_like(&self, other: std::sync::Arc<CanvasSession>, r: f32, g: f32, b: f32) {
        let brush = {
            let st = other.shared.lock();
            let (set, index) = st.studio.as_ref().map_or_else(|| st.brush_ref(st.tool), |(at, _)| *at);
            st.library.get(set).and_then(|s| s.brushes.get(index)).cloned()
        };
        let Some(brush) = brush else { return };
        let mut st = self.shared.lock();
        st.finish_stroke();
        st.tool = crate::ToolKind::Paint;
        // the size slider's place for the size the pad draws at
        let size = procraft_engine::preview::pad_size_share(&brush).powf(1.0 / procraft_engine::brush::SIZE_CURVE);
        let (set, index) = st.paint_brush;
        if let Some(slot) = st.library.get_mut(set).and_then(|s| s.brushes.get_mut(index)) {
            *slot = brush;
        }
        let sliders = st.sliders_mut();
        sliders.size = size;
        sliders.opacity = 1.0;
        st.color = [r, g, b].map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 });
    }

    /// Starts keeping this canvas's strokes, to draw them again as the brush changes (the brush
    /// studio's drawing pad).
    pub fn keep_strokes(&self) {
        let mut st = self.shared.lock();
        if st.pad_strokes.is_none() {
            st.pad_strokes = Some(Vec::new());
        }
    }

    /// Draws the kept strokes again on a cleared layer, with the brush as it is now.
    pub fn replay_strokes(&self) {
        let strokes = {
            let mut st = self.shared.lock();
            match st.pad_strokes.as_mut() {
                Some(kept) => std::mem::take(kept),
                None => return,
            }
        };
        self.clear_layer();
        // drawn as they were, they are kept again as they go
        for (pointer, input) in strokes {
            if input.is_empty() {
                continue;
            }
            self.stroke_begin(pointer, input);
            self.stroke_end(Vec::new());
        }
    }

    /// Forgets the kept strokes (the pad cleared).
    pub fn forget_strokes(&self) {
        let mut st = self.shared.lock();
        if let Some(kept) = st.pad_strokes.as_mut() {
            kept.clear();
        }
        st.pad_sample = false;
    }

    /// Draws the pad's sample scribble across it (wrapped round the sphere when it is one), kept
    /// like a stroke until one's own replaces it.
    pub fn draw_sample_scribble(&self) {
        {
            let mut st = self.shared.lock();
            let (w, h) = (st.doc.width() as f32, st.doc.height() as f32);
            let scale = st.display_scale;
            let scribble = if st.sphere.is_some() { sphere_scribble(w, h, scale) } else { pad_scribble(w, h, scale) };
            st.pad_strokes = Some(vec![(crate::PointerKind::Stylus, scribble)]);
            st.pad_sample = true;
        }
        self.replay_strokes();
    }

    /// Draws the pad as a sphere showing the brush's materials (`shown`), starting from its own
    /// sample scribble each time; or flat again, with its strokes as they were.
    pub fn show_sphere(&self, shown: bool) {
        {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            if shown == st.sphere.is_some() {
                return;
            }
            st.finish_stroke();
            if shown {
                let flat = st.pad_strokes.as_mut().map(std::mem::take).unwrap_or_default();
                st.sphere = Some(SpherePad { flat, flat_sample: st.pad_sample, preview: None, sources: [None, None] });
            } else if let Some(sphere) = st.sphere.take() {
                if let Some(kept) = st.pad_strokes.as_mut() {
                    *kept = sphere.flat;
                }
                st.pad_sample = sphere.flat_sample;
            }
            // the sphere shows what is painted over its own colour, the flat pad over the pad's
            st.doc.background_visible = !shown;
            if let Some(canvas) = st.canvas.as_mut() {
                canvas.invalidate();
            }
        }
        let flat_empty = !shown && self.shared.lock().pad_strokes.as_ref().is_none_or(Vec::is_empty);
        if shown || flat_empty {
            self.draw_sample_scribble();
        } else {
            self.replay_strokes();
        }
    }

    /// A stroke of one's own starts on the pad: its sample scribble goes, if it is still there.
    pub fn begin_own_stroke(&self) {
        let sample = std::mem::replace(&mut self.shared.lock().pad_sample, false);
        if sample {
            self.forget_strokes();
            self.clear_layer();
        }
    }

    /// Applies brush changes kept by earlier versions (to the library as it ships); anything it
    /// cannot read is skipped.
    pub fn apply_brush_changes(&self, json: String) {
        let Ok(changes) = serde_json::from_str::<Vec<BrushChange>>(&json) else { return };
        let mut st = self.shared.lock();
        for c in changes {
            let Some(brush) = st.library.get_mut(c.set).and_then(|s| s.brushes.get_mut(c.index)) else { continue };
            if !c.name.is_empty() {
                brush.name = c.name;
            }
            brush.set_settings(c.settings.iter().map(|(k, v)| (k.as_str(), *v)));
            if c.curve.len() >= 2 {
                let pts: Vec<Vec2> = c.curve.iter().map(|&[x, y]| Vec2::new(x, y)).collect();
                brush.pressure_curve = Curve::new(&pts);
            }
        }
    }

    /// Closes the studio: `keep` keeps the edits, otherwise the brush goes back to how it opened.
    pub fn studio_end(&self, keep: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(((set, index), before)) = st.studio.take() else { return };
        if !keep && let Some(brush) = st.library.get_mut(set).and_then(|s| s.brushes.get_mut(index)) {
            // a reset point made meanwhile stays
            let reset = brush.reset_point.take();
            *brush = Brush { reset_point: reset, ..before };
        }
    }
}

/// Where `brush` keeps its `kind` of source: its own image, the shipped one, and the stock one its
/// file named.
fn sources(brush: &mut Brush, kind: SourceKind) -> (&mut Option<Source>, &mut Option<u32>, &mut Option<StockImage>) {
    let stock = &mut brush.stock;
    match kind {
        SourceKind::Shape => (&mut brush.shape.source, &mut brush.shape.image, &mut stock.shape),
        SourceKind::Grain => (&mut brush.grain.source, &mut brush.grain.image, &mut stock.grain),
        SourceKind::Metallic => material(&mut brush.metallic, &mut stock.metallic),
        SourceKind::Roughness => material(&mut brush.roughness, &mut stock.roughness),
    }
}

fn material<'a>(
    m: &'a mut MaterialSource,
    stock: &'a mut Option<StockImage>,
) -> (&'a mut Option<Source>, &'a mut Option<u32>, &'a mut Option<StockImage>) {
    (&mut m.source, &mut m.image, stock)
}

/// The drawing pad while it is a sphere: the flat pad's strokes set aside (and whether they were
/// its sample scribble), the preview once drawn, and the brush's material sources as last given
/// to it (a key for each and its pixels).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct SpherePad {
    flat: Vec<(PointerKind, Vec<PenSample>)>,
    flat_sample: bool,
    preview: Option<SpherePreview>,
    sources: [Option<(u64, Arc<Vec<u8>>)>; 2],
}

/// What tells a material source's image from another's: its own image's, else the shipped one's.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn source_key(m: &MaterialSource) -> Option<u64> {
    m.source.as_ref().map(|s| s.id).or_else(|| m.image.map(|i| (1 << 63) | u64::from(i)))
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
impl State {
    /// Draws the pad as a sphere into `target` (`size`): [`SPHERE_RADIUS_PT`] round, in its
    /// middle, showing the canvas painted with the pad's brush, its metalness and roughness.
    pub(crate) fn render_sphere(
        &mut self,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        convert: Option<[[f32; 3]; 3]>,
    ) {
        let (metallic, roughness, tiles, materials) = {
            let b = self.brush(ToolKind::Paint);
            let tile = |key: &str| (b.setting(key) * MAX_GRAIN_SCALE).max(4.0);
            (
                b.setting("metallic_amount"),
                b.setting("roughness_amount"),
                [tile("metallic_scale"), tile("roughness_scale")],
                [b.metallic.clone(), b.roughness.clone()],
            )
        };
        let to_canvas = self.view.and_then(|v| v.to_screen().inverse()).unwrap_or(Affine::IDENTITY);
        let behind = self.doc.background;
        let (Some(gpu), Some(canvas), Some(sphere)) = (self.gpu.as_ref(), self.canvas.as_mut(), self.sphere.as_mut())
        else {
            return;
        };
        canvas.render(gpu, &mut self.doc, None);
        for (slot, m) in sphere.sources.iter_mut().zip(&materials) {
            let key = source_key(m);
            if slot.as_ref().map(|(k, _)| *k) != key {
                *slot = key.zip(crate::brush_files::material_pixels(m)).map(|(k, p)| (k, Arc::new(p)));
            }
        }
        let [metallic_source, roughness_source] =
            sphere.sources.each_ref().map(|s| s.as_ref().map(|(k, p)| (*k, p.as_slice())));
        let look = SphereLook {
            metallic,
            roughness,
            metallic_source,
            roughness_source,
            metallic_tile: tiles[0],
            roughness_tile: tiles[1],
            behind,
            convert,
        };
        let (w, h) = (size.0 as f32, size.1 as f32);
        let Affine { a, b, c, d, e, f } = to_canvas;
        let radius = SPHERE_RADIUS_PT * self.display_scale;
        let preview = sphere.preview.get_or_insert_with(|| SpherePreview::new(gpu, format));
        preview.render(gpu, target, [w / 2.0, h / 2.0], radius, canvas, [a, b, c, d, e, f], &look);
    }
}

/// The studio's About page: who made the brush ("" for ProCraft's own) with their picture and
/// signature (PNG) if it has them, and when its reset point was made (seconds since 1970).
#[derive(uniffi::Record)]
pub struct StudioAbout {
    pub author: String,
    pub picture: Option<Vec<u8>>,
    pub signature: Option<Vec<u8>>,
    pub reset_made: Option<i64>,
}

/// Now, in seconds since 1970.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The drawing pad's sample scribble, in points from the pad's middle: a light lead-in up to the
/// top, then a steep zigzag down the pad whose swings shrink as it goes, ending in a small hook to
/// the right. The path runs smoothly through these points. It is the same size whatever the pad's
/// (as the reference's, which runs off a portrait pad's top and fits a landscape one).
const SCRIBBLE: [(f32, f32); 41] = [
    (-147.0, -7.5),
    (-104.0, -66.5),
    (-44.0, -140.5),
    (15.5, -206.0),
    (88.5, -264.0),
    (112.0, -269.5),
    (116.0, -259.0),
    (97.0, -220.0),
    (55.5, -162.5),
    (-16.0, -61.5),
    (-57.0, 5.0),
    (-91.0, 83.5),
    (-81.0, 82.0),
    (-67.0, 66.5),
    (-10.0, -7.0),
    (42.5, -67.0),
    (113.0, -124.0),
    (133.5, -129.5),
    (136.0, -120.5),
    (97.0, -56.0),
    (78.5, -28.5),
    (38.0, 33.5),
    (-7.5, 110.0),
    (-36.0, 180.5),
    (-27.0, 180.0),
    (-6.5, 158.0),
    (30.5, 111.0),
    (68.0, 65.5),
    (117.5, 27.0),
    (125.0, 31.5),
    (86.0, 109.5),
    (70.0, 135.5),
    (27.0, 216.5),
    (77.5, 164.5),
    (113.5, 128.5),
    (130.0, 121.5),
    (133.5, 132.0),
    (117.0, 179.0),
    (100.0, 239.0),
    (109.0, 255.0),
    (130.5, 264.5),
];

/// How hard the sample scribble is pressed along its length (the share of the way, the pressure):
/// the pen's own pressure doubled (a preview's pressure is the pen's times 2.04, past any pressure
/// curve), so lightly through the lead-in, about 0.8 on the strokes back up, nearly full on those
/// down to the left, lighter through the sharpest turn, letting go over the end.
const SCRIBBLE_PRESSURE: [(f32, f32); 41] = [
    (0.0, 0.093),
    (0.025, 0.115),
    (0.05, 0.122),
    (0.075, 0.15),
    (0.1, 0.202),
    (0.125, 0.28),
    (0.15, 0.373),
    (0.175, 0.365),
    (0.2, 0.504),
    (0.225, 0.637),
    (0.25, 0.735),
    (0.275, 0.827),
    (0.3, 0.889),
    (0.325, 0.904),
    (0.35, 0.875),
    (0.375, 0.776),
    (0.4, 0.785),
    (0.425, 0.797),
    (0.45, 0.798),
    (0.475, 0.792),
    (0.5, 0.691),
    (0.525, 0.688),
    (0.55, 0.822),
    (0.575, 0.913),
    (0.6, 0.969),
    (0.625, 0.99),
    (0.65, 0.943),
    (0.675, 0.789),
    (0.7, 0.789),
    (0.725, 0.841),
    (0.75, 0.883),
    (0.775, 0.798),
    (0.8, 0.802),
    (0.825, 0.808),
    (0.85, 0.836),
    (0.875, 0.656),
    (0.9, 0.601),
    (0.925, 0.568),
    (0.95, 0.587),
    (0.975, 0.414),
    (1.0, 0.0),
];

/// How far through its time the sample scribble is along its length (the share of the way, the
/// share of the time): quick through the middle, slower into its first turns and out of its last,
/// over [`SCRIBBLE_SECONDS`].
const SCRIBBLE_PACE: [(f32, f32); 21] = [
    (0.0, 0.0),
    (0.05, 0.0845),
    (0.1, 0.1242),
    (0.15, 0.1636),
    (0.2, 0.2329),
    (0.25, 0.2603),
    (0.3, 0.2834),
    (0.35, 0.3212),
    (0.4, 0.3899),
    (0.45, 0.4197),
    (0.5, 0.4606),
    (0.55, 0.5162),
    (0.6, 0.5415),
    (0.65, 0.572),
    (0.7, 0.6403),
    (0.75, 0.6728),
    (0.8, 0.7349),
    (0.85, 0.7673),
    (0.9, 0.8363),
    (0.95, 0.907),
    (1.0, 1.0),
];

/// How long the sample scribble takes to draw.
const SCRIBBLE_SECONDS: f64 = 1.013;

/// `table`'s value `t` of the way along the sample scribble, in a straight line between its points.
fn along(table: &[(f32, f32)], t: f32) -> f32 {
    let mut before = (0.0, 0.0);
    for &(at, p) in table {
        if t <= at {
            let (b, q) = before;
            return if at > b { q + (p - q) * (t - b) / (at - b) } else { p };
        }
        before = (at, p);
    }
    before.1
}

/// The sample scribble's pressure `t` of the way along it.
fn scribble_pressure(t: f32) -> f32 {
    along(&SCRIBBLE_PRESSURE, t)
}

/// The drawing pad's sample scribble on a pad `w` x `h` (canvas pixels, the pad's window, `scale`
/// pixels a point): a smooth path through [`SCRIBBLE`] drawn at [`SCRIBBLE_PACE`], pressed as
/// [`SCRIBBLE_PRESSURE`] says.
fn pad_scribble(w: f32, h: f32, scale: f32) -> Vec<crate::PenSample> {
    const STEPS: usize = 1200;
    let turns: Vec<Vec2> = SCRIBBLE.iter().map(|&(x, y)| Vec2::new(w / 2.0 + x * scale, h / 2.0 + y * scale)).collect();
    let path = evenly(&through(&turns, 24), STEPS);
    path.iter()
        .enumerate()
        .map(|(i, p)| {
            let t = i as f32 / STEPS as f32;
            crate::PenSample {
                x: p.x,
                y: p.y,
                pressure: scribble_pressure(t),
                tilt: 0.0,
                orientation: 0.0,
                time_ms: f64::from(along(&SCRIBBLE_PACE, t)) * SCRIBBLE_SECONDS * 1000.0,
            }
        })
        .collect()
}

/// The sphere's radius on the pad, points, whatever the pad's size (as the reference's, which runs
/// a little past a landscape pad's sides).
const SPHERE_RADIUS_PT: f32 = 240.25;

/// How the sample scribble wraps round the sphere: each of its points (see [`SCRIBBLE`]) this many
/// radians of longitude and latitude, the scribble's middle turned this far right and up from the
/// sphere's front (as the reference's lies, drawn on the sphere's surface).
const SPHERE_LONGITUDE: f32 = 0.003_538_5;
const SPHERE_LATITUDE: f32 = 0.001_788_6;
const SPHERE_TURN: f32 = 0.089_121;
const SPHERE_TILT: f32 = -0.014_435;

/// The sample scribble wrapped round the sphere on a pad `w` x `h` (canvas pixels, `scale` a
/// point): the sphere in its middle, seen straight on.
fn sphere_scribble(w: f32, h: f32, scale: f32) -> Vec<crate::PenSample> {
    let radius = SPHERE_RADIUS_PT * scale;
    pad_scribble(0.0, 0.0, 1.0)
        .into_iter()
        .map(|s| {
            let lon = s.x * SPHERE_LONGITUDE + SPHERE_TURN;
            let lat = s.y * SPHERE_LATITUDE + SPHERE_TILT;
            crate::PenSample { x: w / 2.0 + radius * lat.cos() * lon.sin(), y: h / 2.0 + radius * lat.sin(), ..s }
        })
        .collect()
}

/// A smooth path through `points` (centripetal Catmull-Rom, so no loops or overshoots at sharp
/// turns), `per` points to each span.
fn through(points: &[Vec2], per: usize) -> Vec<Vec2> {
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else { return Vec::new() };
    // the ends carried on straight, to lead in and out
    let lead = |end: Vec2, next: Option<&Vec2>| next.map_or(end, |&n| end + (end - n));
    let mut all = vec![lead(first, points.get(1))];
    all.extend_from_slice(points);
    all.push(lead(last, points.len().checked_sub(2).and_then(|i| points.get(i))));
    let mut out = vec![first];
    for w in all.windows(4) {
        let &[p0, p1, p2, p3] = w else { continue };
        // knots spaced by the root of each span's length
        let knot = |a: Vec2, b: Vec2| a.distance(b).sqrt().max(1e-3);
        let (t1, t2, t3) = (knot(p0, p1), knot(p0, p1) + knot(p1, p2), knot(p0, p1) + knot(p1, p2) + knot(p2, p3));
        for k in 1..=per {
            let t = t1 + (t2 - t1) * k as f32 / per as f32;
            let a1 = p0.lerp(p1, t / t1);
            let a2 = p1.lerp(p2, (t - t1) / (t2 - t1));
            let a3 = p2.lerp(p3, (t - t2) / (t3 - t2));
            let b1 = a1.lerp(a2, t / t2);
            let b2 = a2.lerp(a3, (t - t1) / (t3 - t1));
            out.push(b1.lerp(b2, (t - t1) / (t2 - t1)));
        }
    }
    out
}

/// `steps` + 1 points evenly spaced along `path`.
fn evenly(path: &[Vec2], steps: usize) -> Vec<Vec2> {
    let mut lengths = vec![0.0f32];
    for w in path.windows(2) {
        let &[a, b] = w else { continue };
        let so_far = lengths.last().copied().unwrap_or(0.0);
        lengths.push(so_far + a.distance(b));
    }
    let total = lengths.last().copied().unwrap_or(0.0);
    let mut at = 0;
    (0..=steps)
        .filter_map(|i| {
            let want = total * i as f32 / steps.max(1) as f32;
            while at + 2 < lengths.len() && lengths.get(at + 1).is_some_and(|&l| l < want) {
                at += 1;
            }
            let (&a, &b) = (path.get(at)?, path.get(at + 1).or(path.get(at))?);
            let (la, lb) = (lengths.get(at).copied()?, lengths.get(at + 1).copied().unwrap_or(total));
            Some(a.lerp(b, if lb > la { (want - la) / (lb - la) } else { 0.0 }))
        })
        .collect()
}
