//! Layers: the list the layers panel shows and every change to it, each one undoable. A slider
//! drag (opacity, background colour) is one undo step from where it began to `done`.

use procraft_engine::document::{BlendMode, Document, LayerId, LayerKind};

use crate::CanvasSession;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LayerInfo {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    /// A blend mode id (see [`blend_modes`]).
    pub blend: u32,
    pub alpha_lock: bool,
    pub clipped: bool,
    pub locked: bool,
    pub current: bool,
    /// Picked to act on together with the current layer.
    pub picked: bool,
    pub reference: bool,
    /// Left out of the gallery's thumbnail (put in by a private insert or paste).
    pub private: bool,
    /// Strokes on it follow the drawing guide.
    pub assisted: bool,
    pub kind: LayerType,
    /// A mask's layer, or the group a layer is in.
    pub parent: Option<u64>,
    /// A group shown closed.
    pub collapsed: bool,
    /// A text layer, its text still editable.
    pub text: bool,
    /// How many groups it is inside.
    pub depth: u32,
    /// Whether Merge Down, Combine Down and Clipping Mask apply to it.
    pub can_merge: bool,
    pub can_combine: bool,
    pub can_clip: bool,
}

/// A layer's pixels' version: new whenever they may have changed.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LayerVersion {
    pub id: u64,
    pub version: u64,
}

/// What a layer in the list is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LayerType {
    Paint,
    Group,
    Mask,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BlendModeInfo {
    pub id: u32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BackgroundInfo {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub visible: bool,
}

/// A small premultiplied RGBA image, rows top to bottom.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ThumbnailImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The memory layers may take on this device, bytes: it sets how many layers a canvas holds.
#[uniffi::export]
pub fn set_layer_budget(bytes: u64) {
    procraft_engine::document::set_layer_budget(bytes);
}

/// What the layers' textures may take on this device, bytes: past it, layers not worked on are
/// kept compressed off the GPU until needed. 0 keeps them all on it.
static RESIDENT_BUDGET: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Sets what the layers' textures may take, bytes (see [`RESIDENT_BUDGET`]).
#[uniffi::export]
pub fn set_resident_budget(bytes: u64) {
    RESIDENT_BUDGET.store(bytes, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn resident_budget() -> u64 {
    RESIDENT_BUDGET.load(std::sync::atomic::Ordering::Relaxed)
}

/// How many layers a canvas holds, has, and of which kinds.
#[derive(uniffi::Record)]
pub struct LayerStats {
    /// Layers with pixels (paint layers and masks) the canvas can hold.
    pub maximum: u32,
    /// Layers with pixels it has.
    pub used: u32,
    pub clipping: u32,
    pub masks: u32,
    pub groups: u32,
}

/// The sRGB colour profile (ICC), as palette files carry it with their colours.
#[uniffi::export]
pub fn srgb_profile() -> Vec<u8> {
    procraft_engine::icc::icc(procraft_engine::profile::Profile::Srgb)
}

/// The layer blend modes in the order the panel lists them.
#[uniffi::export]
pub fn blend_modes() -> Vec<BlendModeInfo> {
    BlendMode::ALL.iter().map(|m| BlendModeInfo { id: m.id(), name: m.name().to_owned() }).collect()
}

/// The modes a brush and its burnt edges blend by, in the order the studio lists them.
#[uniffi::export]
pub fn brush_blend_modes() -> Vec<BlendModeInfo> {
    procraft_engine::brush::brush_blends()
        .into_iter()
        .map(|(id, name)| BlendModeInfo { id, name: name.to_owned() })
        .collect()
}

/// The modes a brush's grain blends by, in the order the studio lists them.
#[uniffi::export]
pub fn grain_blend_modes() -> Vec<BlendModeInfo> {
    procraft_engine::brush::grain_blends()
        .into_iter()
        .map(|(id, name)| BlendModeInfo { id, name: name.to_owned() })
        .collect()
}

#[uniffi::export]
impl CanvasSession {
    /// The visible paint layers with something drawn under window point (`x`, `y`), top first.
    pub fn layers_at(&self, x: f32, y: f32) -> Vec<u64> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(p) = st.view.and_then(|v| v.screen_to_canvas(procraft_engine::geom::Vec2::new(x, y))) else {
            return Vec::new();
        };
        if p.x < 0.0 || p.y < 0.0 {
            return Vec::new();
        }
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return Vec::new() };
        let Ok(alphas) = canvas.alphas_at(gpu, &mut st.doc, p.x as u32, p.y as u32) else { return Vec::new() };
        let doc = &st.doc;
        alphas
            .into_iter()
            .rev()
            .filter(|(id, a)| *a >= 26 && doc.layer(*id).is_some_and(|l| l.visible))
            .map(|(id, _)| id.0)
            .collect()
    }

    pub fn layer_stats(&self) -> LayerStats {
        let st = self.shared.lock();
        let d = &st.doc;
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let kind = |k: LayerKind| count(d.layers().iter().filter(|l| l.kind == k).count());
        LayerStats {
            maximum: count(d.layer_cap()),
            used: count(d.pixel_layer_ids().count()),
            clipping: count(d.layers().iter().filter(|l| l.clipped).count()),
            masks: kind(LayerKind::Mask),
            groups: kind(LayerKind::Group),
        }
    }

    /// Top layer first, as the panel lists them.
    pub fn layers(&self) -> Vec<LayerInfo> {
        let st = self.shared.lock();
        let current = st.doc.current();
        st.doc
            .layers()
            .iter()
            .rev()
            .map(|l| LayerInfo {
                id: l.id.0,
                name: l.name.clone(),
                visible: l.visible,
                opacity: l.opacity,
                blend: l.blend.id(),
                alpha_lock: l.alpha_lock,
                clipped: l.clipped,
                locked: l.locked,
                current: l.id == current,
                picked: st.doc.is_picked(l.id),
                reference: l.reference,
                private: l.private,
                assisted: l.assisted,
                kind: match l.kind {
                    LayerKind::Paint => LayerType::Paint,
                    LayerKind::Group => LayerType::Group,
                    LayerKind::Mask => LayerType::Mask,
                },
                parent: match l.kind {
                    LayerKind::Mask => l.parent.map(|p| p.0),
                    _ => st.doc.group_of(l.id).map(|g| g.0),
                },
                collapsed: l.collapsed,
                text: l.text.is_some(),
                depth: u32::try_from(st.doc.depth(l.id)).unwrap_or(u32::MAX),
                can_merge: st.doc.merge_target(l.id).is_some(),
                can_combine: l.kind != LayerKind::Mask && st.doc.below(l.id).is_some(),
                can_clip: st.doc.can_clip(l.id),
            })
            .collect()
    }

    pub fn background(&self) -> BackgroundInfo {
        let st = self.shared.lock();
        let [r, g, b] = st.doc.background;
        BackgroundInfo { r, g, b, visible: st.doc.background_visible }
    }

    /// A new empty layer above the current one; it becomes current.
    /// A new empty layer above the current one, made current; 0 when the canvas has no room for
    /// another (see [`Document::layer_cap`]).
    pub fn add_layer(&self) -> u64 {
        self.change("new layer", |d| d.has_room(1).then(|| d.add_layer().0)).unwrap_or(0)
    }

    /// Makes `id` the layer painting goes to, and the only one chosen (no picks): an undo step,
    /// unless nothing changes.
    pub fn select_layer(&self, id: u64) -> bool {
        let id = LayerId(id);
        {
            let st = self.shared.lock();
            if st.doc.current() == id && st.doc.picked().is_empty() {
                return st.doc.layer(id).is_some();
            }
        }
        let ok = self
            .change("select layer", |d| {
                d.clear_picks();
                d.set_current(id).then_some(())
            })
            .is_some();
        // the onion skins follow the current frame
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if ok
            && st.doc.animation.is_some()
            && let Some(c) = st.canvas.as_mut()
        {
            c.invalidate();
            self.shared.frame(st);
        }
        ok
    }

    /// Picks layer `id` to act on with the current one, or unpicks it: an undo step.
    pub fn pick_layer(&self, id: u64) -> bool {
        self.change("pick layer", |d| d.toggle_pick(LayerId(id)).then_some(())).is_some()
    }

    /// Picks every layer: an undo step.
    pub fn pick_all_layers(&self) -> bool {
        self.change("select all layers", |d| d.pick_all().then_some(())).is_some()
    }

    /// The current and picked layers, bottom first.
    pub fn chosen_layers(&self) -> Vec<u64> {
        self.shared.lock().doc.chosen().iter().map(|l| l.0).collect()
    }

    /// Puts the current and picked layers in a new group where the top of them was; returns the
    /// group, or 0.
    pub fn group_picked(&self) -> u64 {
        self.change("group layers", |d| d.group_chosen().map(|g| g.0)).unwrap_or(0)
    }

    /// Copies of the current and picked layers, together above the top of them; the copies are
    /// what is chosen then. False if there is no room for them.
    pub fn duplicate_picked(&self) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let before = st.doc.clone();
        let mut trial = st.doc.clone();
        let Some(pairs) = trial.duplicate_chosen() else { return false };
        if !st.doc.has_room(pairs.len()) {
            return false;
        }
        st.doc = trial;
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step("duplicate layers");
            canvas.duplicate_layers(pairs, before);
        }
        st.revision += 1;
        self.shared.frame(st);
        true
    }

    /// Removes the current and picked layers that aren't locked (the last layer stays).
    pub fn delete_picked(&self) -> bool {
        self.change("delete layers", |d| d.delete_chosen().then_some(())).is_some()
    }

    /// Merges the picked layers above the bottom one of what is chosen into it.
    pub fn merge_picked(&self) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let Some((lower, uppers)) = st.doc.merge_chosen_plan() else { return false };
        let before = st.doc.clone();
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step("merge layers");
            canvas.merge_layers(uppers.clone(), lower, before);
        }
        st.doc.finish_merge_chosen(lower, &uppers);
        st.revision += 1;
        self.shared.frame(st);
        true
    }

    pub fn delete_layer(&self, id: u64) -> bool {
        self.change("delete layer", |d| {
            let unlocked = d.layer(LayerId(id)).is_some_and(|l| !l.locked);
            (unlocked && d.delete_layer(LayerId(id))).then_some(())
        })
        .is_some()
    }

    /// A copy just above `id` (a layer with its mask, a group with everything in it), made
    /// current; returns its id.
    pub fn duplicate_layer(&self, id: u64) -> Option<u64> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let before = st.doc.clone();
        let unit = st.doc.layer(LayerId(id)).map_or(0, |_| {
            st.doc
                .pixel_layer_ids()
                .filter(|&l| {
                    l == LayerId(id) || st.doc.is_inside(l, LayerId(id)) || st.doc.mask_of(LayerId(id)) == Some(l)
                })
                .count()
        });
        if !st.doc.has_room(unit) {
            return None;
        }
        let (copy, pairs) = st.doc.duplicate(LayerId(id))?;
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step("duplicate layer");
            canvas.duplicate_layers(pairs, before);
        }
        st.revision += 1;
        self.shared.frame(st);
        Some(copy.0)
    }

    /// Gives paint layer `id` a mask (white: showing all of it), made current; returns its id.
    pub fn add_mask(&self, id: u64) -> Option<u64> {
        let mask = self.change("mask", |d| if d.has_room(1) { d.add_mask(LayerId(id)) } else { None })?;
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let white = vec![255u8; st.doc.width() as usize * st.doc.height() as usize * 4];
        if let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) {
            let _ = canvas.upload_layer(gpu, mask, &white);
        }
        self.shared.frame(st);
        Some(mask.0)
    }

    /// Puts layer or group `id` and what is below it in a new group, or into the group below;
    /// returns the group's id.
    pub fn combine_down(&self, id: u64) -> Option<u64> {
        self.change("combine down", |d| d.combine_down(LayerId(id))).map(|g| g.0)
    }

    /// Takes the layers out of group `id` and removes it.
    pub fn ungroup(&self, id: u64) -> bool {
        self.change("ungroup", |d| d.ungroup(LayerId(id)).then_some(())).is_some()
    }

    /// Replaces group `id` with one layer showing the same; returns the layer's id.
    pub fn flatten_group(&self, id: u64) -> Option<u64> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let before = st.doc.clone();
        let layer = st.doc.flatten(LayerId(id))?;
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.flatten_group(LayerId(id), layer, false, before);
        }
        st.revision += 1;
        self.shared.frame(st);
        Some(layer.0)
    }

    /// Opens or closes a group in the list (not an undo step).
    pub fn set_group_collapsed(&self, id: u64, collapsed: bool) -> bool {
        let mut st = self.shared.lock();
        let ok = st.doc.set_collapsed(LayerId(id), collapsed);
        if ok {
            st.revision += 1;
        }
        ok
    }

    /// Moves layer or group `id` (with its mask, or everything in it) to `position`, counted from
    /// the bottom without them. Landing in a group, it joins it.
    pub fn move_layer(&self, id: u64, position: u32) -> bool {
        self.change("move layer", |d| {
            let to = usize::try_from(position).unwrap_or(usize::MAX);
            let before = d.layers().to_vec();
            (d.move_layer(LayerId(id), to) && d.layers() != before.as_slice()).then_some(())
        })
        .is_some()
    }

    /// Moves the current and picked layers together to `position` of the stack without them
    /// (0 = bottom), as [`CanvasSession::move_layer`] moves one.
    pub fn move_chosen(&self, position: u32) -> bool {
        self.change("move layers", |d| {
            let to = usize::try_from(position).unwrap_or(usize::MAX);
            let before = d.layers().to_vec();
            (d.move_chosen(to) && d.layers() != before.as_slice()).then_some(())
        })
        .is_some()
    }

    /// Blends layer or group `id` into the layer below and removes it.
    pub fn merge_down(&self, id: u64) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let upper = LayerId(id);
        let Some(lower) = st.doc.merge_target(upper) else { return false };
        if [upper, lower].iter().any(|l| st.doc.layer(*l).is_none_or(|l| l.locked)) {
            return false;
        }
        let before = st.doc.clone();
        let group = st.doc.layer(upper).is_some_and(|l| l.kind == LayerKind::Group);
        if let Some(canvas) = st.canvas.as_mut() {
            if group {
                canvas.flatten_group(upper, lower, true, before);
            } else {
                canvas.name_next_step("merge down");
                canvas.merge_down(upper, lower, before);
            }
        }
        st.doc.finish_merge_down(upper);
        st.revision += 1;
        self.shared.frame(st);
        true
    }

    pub fn set_layer_visible(&self, id: u64, visible: bool) -> bool {
        self.change(if visible { "show layer" } else { "hide layer" }, |d| {
            changed(d, id, |l| l.visible != visible)
                .then(|| d.set_visible(LayerId(id), visible))
                .filter(|ok| *ok)
                .map(|_| ())
        })
        .is_some()
    }

    /// Shows layer `id` alone, the others left out of the picture (solo visibility), or with
    /// none every layer as it is set again. Not an undo step; false if `id` isn't a layer.
    pub fn set_solo(&self, id: Option<u64>) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let solo = id.map(LayerId);
        if solo.is_some_and(|s| st.doc.layer(s).is_none()) {
            return false;
        }
        st.doc.solo = solo;
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.invalidate();
        }
        st.revision += 1;
        self.shared.frame(st);
        true
    }

    /// The layer shown alone, if one is.
    pub fn solo(&self) -> Option<u64> {
        let st = self.shared.lock();
        st.doc.solo.filter(|s| st.doc.layer(*s).is_some()).map(|s| s.0)
    }

    /// Opacity 0..1; `done` ends the drag (one undo step for the whole drag).
    pub fn set_layer_opacity(&self, id: u64, opacity: f32, done: bool) -> bool {
        self.change_live("layer opacity", done, |d| d.set_opacity(LayerId(id), opacity))
    }

    /// `blend` is a blend mode id (see [`blend_modes`]).
    pub fn set_layer_blend(&self, id: u64, blend: u32) -> bool {
        let Some(mode) = BlendMode::from_id(blend) else { return false };
        self.change("blend mode", |d| {
            changed(d, id, |l| l.blend != mode).then(|| d.set_blend(LayerId(id), mode)).filter(|ok| *ok).map(|_| ())
        })
        .is_some()
    }

    pub fn set_layer_alpha_lock(&self, id: u64, on: bool) -> bool {
        self.change("alpha lock", |d| {
            changed(d, id, |l| l.alpha_lock != on)
                .then(|| d.set_alpha_lock(LayerId(id), on))
                .filter(|ok| *ok)
                .map(|_| ())
        })
        .is_some()
    }

    pub fn set_layer_clipped(&self, id: u64, on: bool) -> bool {
        self.change(if on { "clip layer" } else { "unclip layer" }, |d| {
            changed(d, id, |l| l.clipped != on).then(|| d.set_clipped(LayerId(id), on)).filter(|ok| *ok).map(|_| ())
        })
        .is_some()
    }

    pub fn set_layer_locked(&self, id: u64, on: bool) -> bool {
        self.change(if on { "lock layer" } else { "unlock layer" }, |d| {
            changed(d, id, |l| l.locked != on).then(|| d.set_locked(LayerId(id), on)).filter(|ok| *ok).map(|_| ())
        })
        .is_some()
    }

    pub fn rename_layer(&self, id: u64, name: String) -> bool {
        self.change("rename layer", |d| {
            changed(d, id, |l| l.name != name.trim()).then(|| d.rename(LayerId(id), &name)).filter(|ok| *ok).map(|_| ())
        })
        .is_some()
    }

    /// Background colour, sRGB 0..1; `done` ends the drag.
    pub fn set_background_color(&self, r: f32, g: f32, b: f32, done: bool) -> bool {
        let c = [r, g, b];
        if c.iter().any(|v| !v.is_finite()) {
            return false;
        }
        self.change_live("change background color", done, |d| {
            d.background = c.map(|v| v.clamp(0.0, 1.0));
            true
        })
    }

    pub fn set_background_visible(&self, visible: bool) -> bool {
        self.change(if visible { "show background" } else { "hide background" }, |d| {
            (d.background_visible != visible).then(|| {
                d.background_visible = visible;
            })
        })
        .is_some()
    }

    /// The whole picture scaled to fit `max_side` pixels (premultiplied), for the reference
    /// window; none before the canvas exists.
    pub fn canvas_thumbnail(&self, max_side: u32) -> Option<ThumbnailImage> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return None };
        let t = canvas.composite_thumbnail(gpu, &mut st.doc, max_side).ok()?;
        Some(ThumbnailImage { width: t.width, height: t.height, rgba: t.rgba })
    }

    /// The layer scaled to fit `max_side` pixels, or none before the canvas exists.
    /// Each paint layer's and mask's pixels' version, the queued work done first: a thumbnail
    /// need only be read again when its layer's is new.
    pub fn layer_versions(&self) -> Vec<LayerVersion> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.flush();
        let Some(canvas) = st.canvas.as_ref() else { return Vec::new() };
        st.doc
            .layers()
            .iter()
            .filter(|l| l.kind != LayerKind::Group)
            .map(|l| LayerVersion { id: l.id.0, version: canvas.layer_version(l.id) })
            .collect()
    }

    pub fn layer_thumbnail(&self, id: u64, max_side: u32) -> Option<ThumbnailImage> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return None };
        match canvas.layer_thumbnail(gpu, &mut st.doc, LayerId(id), max_side) {
            Ok(t) => Some(ThumbnailImage { width: t.width, height: t.height, rgba: t.rgba }),
            Err(e) => {
                log::warn!("thumbnail: {e}");
                None
            }
        }
    }
}

/// Whether layer `id` exists and `differs` holds for it.
fn changed(d: &Document, id: u64, differs: impl FnOnce(&procraft_engine::document::Layer) -> bool) -> bool {
    d.layer(LayerId(id)).is_some_and(differs)
}

impl CanvasSession {
    /// An undoable change to the document, called `name` by undo and redo; `f` returns `Some`
    /// when it changed something (and must leave the document alone otherwise).
    pub(crate) fn change<T>(&self, name: &str, f: impl FnOnce(&mut Document) -> Option<T>) -> Option<T> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let before = st.doc.clone();
        let out = f(&mut st.doc)?;
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step(name);
            canvas.record_document(before);
        }
        st.revision += 1;
        self.shared.frame(st);
        Some(out)
    }

    /// A change made continuously, called `name` by undo and redo: shown at once, recorded as one
    /// step when `done`.
    fn change_live(&self, name: &str, done: bool, f: impl FnOnce(&mut Document) -> bool) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        let before = st.doc.clone();
        if !f(&mut st.doc) {
            return false;
        }
        st.pending_doc.get_or_insert(before);
        st.pending_name = Some(name.to_owned());
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.invalidate();
        }
        if done {
            st.settle();
        }
        st.revision += 1;
        self.shared.frame(st);
        true
    }
}
