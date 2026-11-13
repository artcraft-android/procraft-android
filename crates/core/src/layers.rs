//! Layers: the list the layers panel shows and every change to it, each one undoable. A slider
//! drag (opacity, background colour) is one undo step from where it began to `done`.

use procraft_engine::document::{BlendMode, Document, LayerId, LayerKind, Unseen};

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
    /// Left out of the gallery's thumbnail and the time-lapse (put in by a private insert or
    /// paste).
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
    /// The kind of layer a plugin adds (`<id>.<key>`), if it is one: its strokes are kept as items
    /// too. Shown locked while its plugin is off.
    pub plugin_kind: Option<String>,
    /// What a plugin's layer is known by on its thumbnail: its kind's badge, or a plug when its kind
    /// draws none or its plugin is off or gone.
    #[uniffi(default = None)]
    pub plugin_badge: Option<crate::PluginBadge>,
    /// How many groups it is inside.
    pub depth: u32,
    /// Left out of the picture while another layer is shown alone (solo visibility): its box
    /// shows it hidden, whatever it is set to.
    pub soloed_out: bool,
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

/// What a row of a layer's options menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LayerOption {
    Rename,
    Ungroup,
    Flatten,
    Select,
    Copy,
    Fill,
    Clear,
    AlphaLock,
    Mask,
    ClippingMask,
    DrawingAssist,
    Invert,
    Reference,
    EditText,
    Rasterize,
    /// A plugin's layer: the strokes the selection takes (every one with nothing selected) given
    /// the paint colour.
    Recolour,
    /// The same given the paint brush, with its size and opacity.
    ApplyBrush,
    MergeDown,
    CombineDown,
    MergeMask,
}

/// A row of a layer's options menu: what it does, its label, whether it can be chosen, and for a
/// switch whether it is on (a tick beside it). Every row closes the menu, a switch's too.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LayerOptionRow {
    pub option: LayerOption,
    pub label: String,
    pub enabled: bool,
    pub switch_on: Option<bool>,
}

/// What locking (`on`) or unlocking layer `id` is called (see [`CanvasSession::lock_said`]).
fn lock_name(d: &Document, id: LayerId, on: bool) -> &'static str {
    let by_group = |l: &procraft_engine::document::Layer| !on && !l.locked && !d.locked_groups(id).is_empty();
    let group = d.layer(id).is_some_and(|l| l.kind == LayerKind::Group || by_group(l));
    match (on, group) {
        (true, false) => "Lock layer",
        (false, false) => "Unlock layer",
        (true, true) => "Lock Layer Group",
        (false, true) => "Unlock Layer Group",
    }
}

/// The rows of layer `l`'s options menu (`masked`: it has a mask; `guided`: the drawing guide is
/// on or has been), in order: what doesn't apply to it left out. Locked, as measured, it offers
/// only what leaves its pixels, name and place as they are (a locked group or mask: nothing).
/// Lock, duplicate and delete are on its row's swipe.
fn layer_options(l: &LayerInfo, masked: bool, guided: bool) -> Vec<LayerOptionRow> {
    use LayerOption as O;
    let unlocked = !l.locked;
    let mut rows = Vec::new();
    let mut row = |option, label: &str, switch_on: Option<bool>| {
        rows.push(LayerOptionRow { option, label: label.to_owned(), enabled: true, switch_on });
    };
    if unlocked {
        row(O::Rename, "Rename", None);
    }
    match l.kind {
        LayerType::Group => {
            if unlocked {
                row(O::Ungroup, "Ungroup", None);
                row(O::Flatten, "Flatten", None);
            }
        }
        LayerType::Mask => {
            if unlocked {
                row(O::Select, "Select", None);
                row(O::Fill, "Fill Layer", None);
                row(O::Clear, "Clear", None);
                row(O::Invert, "Invert", None);
            }
        }
        LayerType::Paint => {
            row(O::Select, "Select", None);
            row(O::Copy, "Copy", None);
            // a text layer's pixels come from its text, a plugin's layer's from its items: nothing to
            // fill, clear, lock or invert
            let drawn_from_data = l.text || l.plugin_kind.is_some();
            if !drawn_from_data && unlocked {
                row(O::Fill, "Fill Layer", None);
                row(O::Clear, "Clear", None);
                row(O::AlphaLock, "Alpha Lock", Some(l.alpha_lock));
            }
            // one mask each: with one, its Merge Mask comes last instead
            if !masked {
                row(O::Mask, "Mask", None);
            }
            if l.can_clip {
                row(O::ClippingMask, "Clipping Mask", Some(l.clipped));
            }
            if l.text {
                if unlocked {
                    row(O::EditText, "Edit Text", None);
                    row(O::Rasterize, "Rasterize", None);
                }
            } else if l.plugin_kind.is_some() {
                if guided || l.assisted {
                    row(O::DrawingAssist, "Drawing Assist", Some(l.assisted));
                }
                // its strokes cleared, recoloured or restyled as strokes
                if unlocked {
                    row(O::Clear, "Clear", None);
                    row(O::Recolour, "Recolour", None);
                    row(O::ApplyBrush, "Apply Brush", None);
                    row(O::Rasterize, "Rasterize", None);
                }
            } else {
                // offered once the canvas's drawing guide has been on (on or off since), or while
                // the layer has it
                if guided || l.assisted {
                    row(O::DrawingAssist, "Drawing Assist", Some(l.assisted));
                }
                if unlocked {
                    row(O::Invert, "Invert", None);
                }
            }
            row(O::Reference, "Reference", Some(l.reference));
        }
    }
    if l.kind != LayerType::Mask && unlocked {
        if l.can_merge {
            row(O::MergeDown, "Merge Down", None);
        }
        if l.can_combine {
            row(O::CombineDown, "Combine Down", None);
        }
    }
    if masked && unlocked {
        row(O::MergeMask, "Merge Mask", None);
    }
    rows
}

/// The memory layers may take on this device, bytes: it sets how many layers a canvas holds.
#[uniffi::export]
pub fn set_layer_budget(bytes: u64) {
    procraft_engine::document::set_layer_budget(bytes);
}

/// How many layers a canvas `width` × `height` holds on this device (Custom Canvas's Maximum
/// Layers).
#[uniffi::export]
pub fn layers_for(width: u32, height: u32) -> u32 {
    u32::try_from(procraft_engine::document::layer_cap_for(width, height)).unwrap_or(u32::MAX)
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
    /// Layers with pixels (paint layers and masks) the canvas can hold; each of a model's texture
    /// sets holds this many. The counts below are of all of them.
    pub maximum: u32,
    /// Layers with pixels it has.
    pub used: u32,
    /// Layers drawn with the drawing guide's help (Drawing Assist on).
    pub assisted: u32,
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

/// The modes a dual brush's two strokes combine by, in the order the studio lists them.
#[uniffi::export]
pub fn combine_blend_modes() -> Vec<BlendModeInfo> {
    procraft_engine::brush::combine_modes()
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
    /// Lets go of the GPU memory kept only to make later work quicker (spare textures, upload and
    /// readback buffers): for when the system is short of memory or the app is put away. Nothing
    /// shown changes; what is let go is made again when next wanted.
    pub fn trim_memory(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.let_go_of_spares();
        }
        if let Some(gpu) = st.gpu.as_ref() {
            gpu.let_go_of_kept_buffers();
        }
    }

    /// The visible paint layers with something drawn under window point (`x`, `y`), top first.
    pub fn layers_at(&self, x: f32, y: f32) -> Vec<u64> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some((px, py)) = st.window_texel(x, y) else { return Vec::new() };
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return Vec::new() };
        let Ok(alphas) = canvas.alphas_at(gpu, &mut st.doc, px, py) else { return Vec::new() };
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
        let docs = st.colour_docs();
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let all = |of: &dyn Fn(&Document) -> usize| count(docs.iter().map(|d| of(d)).sum());
        let kind = |k: LayerKind| all(&|d| d.layers().iter().filter(|l| l.kind == k).count());
        LayerStats {
            maximum: count(st.doc.layer_cap()),
            used: all(&|d| d.pixel_layer_ids().count()),
            assisted: all(&|d| d.layers().iter().filter(|l| l.assisted).count()),
            clipping: all(&|d| d.layers().iter().filter(|l| l.clipped).count()),
            masks: kind(LayerKind::Mask),
            groups: kind(LayerKind::Group),
        }
    }

    /// The rows of layer `id`'s options menu; none if there is no such layer.
    pub fn layer_options(&self, id: u64) -> Vec<LayerOptionRow> {
        let layers = self.layers();
        let Some(l) = layers.iter().find(|l| l.id == id) else { return Vec::new() };
        let masked = layers.iter().any(|m| m.kind == LayerType::Mask && m.parent == Some(id));
        let guided = {
            let g = &self.shared.lock().doc.guide;
            g.on || g.set_up
        };
        layer_options(l, masked, guided)
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
                // a plugin's layer its plugin can't take strokes on (off, say) shows locked
                locked: st.doc.is_locked(l.id) || st.plugin_layer_off(l.id),
                current: l.id == current,
                picked: st.doc.is_picked(l.id),
                reference: l.reference,
                private: l.private,
                assisted: st.doc.is_assisted(l.id),
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
                plugin_kind: l.plugin.as_ref().map(|p| p.kind.clone()),
                plugin_badge: l.plugin.as_ref().map(|p| {
                    let badge = st.hooks.as_ref().and_then(|hooks| hooks.layer_kind_badge(&p.kind));
                    badge.unwrap_or_else(crate::PluginBadge::plug)
                }),
                depth: u32::try_from(st.doc.depth(l.id)).unwrap_or(u32::MAX),
                soloed_out: st.doc.soloed_out(l.id),
                // as measured, not onto a locked layer: both left out of its menu then
                can_merge: st.doc.merge_target(l.id).is_some_and(|b| !st.doc.is_locked(b)),
                can_combine: l.kind != LayerKind::Mask && st.doc.below(l.id).is_some_and(|b| !st.doc.is_locked(b)),
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
        let id = self.add_layer_quietly();
        // a layer added by hand: plugins told
        if id != 0 {
            self.tell_plugins(crate::plugin_run::Hook::LayerAdded);
        }
        id
    }

    /// Add Frame (New Page with Page Assist): a new empty layer above the current one, as
    /// [`Self::add_layer`], named "Frame N" ("Page N") by the frames there are with it.
    pub fn add_frame(&self) -> u64 {
        self.change("new layer", |d| {
            if !d.has_room(1) {
                return None;
            }
            let pages = d.animation.as_ref().is_some_and(|a| a.pages);
            let id = d.add_layer();
            let n = procraft_engine::animation::frames(d).len();
            d.rename(id, &format!("{} {n}", if pages { "Page" } else { "Frame" }));
            Some(id.0)
        })
        .unwrap_or(0)
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

    /// Whether the current and picked layers (and what their groups hold) include a locked one:
    /// as measured, Delete then removes nothing and says why.
    pub fn chosen_locked(&self) -> bool {
        self.shared.lock().doc.chosen_locked()
    }

    /// Removes the current and picked layers (the last layer stays); nothing while one is locked.
    pub fn delete_picked(&self) -> bool {
        self.change("delete layers", |d| d.delete_chosen().then_some(())).is_some()
    }

    /// Merges the picked layers above the bottom one of what is chosen into it.
    pub fn merge_picked(&self) -> bool {
        self.merge_chosen(None)
    }

    /// Two rows pinched together in the panel (layers `a` and `b`, either way up): they and every
    /// layer between merge into the lowest, as Merge does with layers picked. False (what was
    /// chosen kept) if they can't, a locked one among them, or nothing to merge.
    pub fn merge_between(&self, a: u64, b: u64) -> bool {
        self.merge_chosen(Some((LayerId(a), LayerId(b))))
    }

    pub fn delete_layer(&self, id: u64) -> bool {
        self.change("delete layer", |d| {
            let unlocked = d.layer(LayerId(id)).is_some() && !d.is_locked(LayerId(id));
            (unlocked && d.delete_layer(LayerId(id))).then_some(())
        })
        .is_some()
    }

    /// A copy just above `id` (a layer with its mask, a group with everything in it), made
    /// current; returns its id.
    pub fn duplicate_layer(&self, id: u64) -> Option<u64> {
        self.duplicated(id, "duplicate layer", |_, _| {})
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
            canvas.flatten_group(LayerId(id), layer, before);
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

    /// Moves layer `id` just above or below `target`'s row, as a row dragged in the panel drops:
    /// with what goes with it (its mask, or a group with what is in it); the current layer with
    /// others picked (or a picked one) takes them all along, in their order. False if nothing moved.
    pub fn move_next_to(&self, id: u64, target: u64, above: bool) -> bool {
        let layers = self.layers();
        let Some((unit, together)) = moving(&layers, id) else { return false };
        if unit.contains(&target) {
            return false;
        }
        // the others, bottom first: it goes in among them
        let Some(at) = layers.iter().filter(|o| !unit.contains(&o.id)).rev().position(|o| o.id == target) else {
            return false;
        };
        let to = u32::try_from(if above { at + 1 } else { at }).unwrap_or(u32::MAX);
        if together { self.move_chosen(to) } else { self.move_layer(id, to) }
    }

    /// Why the current layer takes no change (a stroke, a transform, an adjustment, clearing,
    /// cutting): it is locked, hidden (itself, by a group it is in, or left out while another
    /// layer is shown alone), or a group with nothing in it. None if it takes them.
    pub fn why_blocked(&self) -> Option<Blocked> {
        if let Some(why) = self.shared.lock().scene_blocked() {
            return Some(why);
        }
        let layers = self.layers();
        let l = layers.iter().find(|l| l.current)?;
        let unseen = self.shared.lock().doc.unseen(LayerId(l.id));
        if l.locked {
            Some(Blocked::Locked)
        } else if unseen.is_some() {
            // the alert is a group's or a layer's by what is current, whatever hides it
            Some(if l.kind == LayerType::Group { Blocked::GroupHidden } else { Blocked::Hidden })
        } else if l.kind == LayerType::Group && !layers.iter().any(|o| o.parent == Some(l.id)) {
            Some(Blocked::EmptyGroup)
        } else {
            None
        }
    }

    /// Why `adjustment` can't start on the current layer, if it can't: as [`Self::why_blocked`],
    /// and Liquify turns away a layer whose alpha is locked.
    pub fn why_not_adjusted(&self, adjustment: crate::adjust_menu::Adjustment) -> Option<Blocked> {
        self.why_blocked().or_else(|| {
            let liquify = adjustment == crate::adjust_menu::Adjustment::Liquify;
            (liquify && self.layers().iter().any(|l| l.current && l.alpha_lock)).then_some(Blocked::AlphaLocked)
        })
    }

    /// Why a change to the current layer would go unseen, if it would (what a hidden alert
    /// says, [`Blocked::Hidden`] and [`Blocked::GroupHidden`]).
    pub fn hidden_by(&self) -> Option<HiddenBy> {
        let st = self.shared.lock();
        Some(match st.doc.unseen(st.doc.current())? {
            Unseen::Itself => HiddenBy::Itself,
            Unseen::MaskLayer => HiddenBy::MaskLayer,
            Unseen::Group => HiddenBy::Group,
            Unseen::Inside => HiddenBy::Inside,
            Unseen::Solo => HiddenBy::Solo,
        })
    }

    /// Why a stroke or a fill on the current layer would leave nothing: why it takes no change
    /// ([`CanvasSession::why_blocked`]), or a group is current, an empty one too (strokes and fills
    /// go into layers). None if it takes paint.
    pub fn why_unpainted(&self) -> Option<Blocked> {
        let why = self.why_blocked();
        let group = {
            let st = self.shared.lock();
            st.doc.layer(st.doc.current()).is_some_and(|l| l.kind == LayerKind::Group)
        };
        match why {
            Some(Blocked::EmptyGroup) => Some(Blocked::Group),
            None if group => Some(Blocked::Group),
            other => other,
        }
    }

    /// A hidden alert's Show: layer `id`'s own box ticked (one step), nothing else, so a layer kept
    /// hidden by a group, by its layer (a mask) or by another shown alone stays so. Returns what
    /// the notice band says, or None if its box was ticked already.
    pub fn show_layer(&self, id: u64) -> Option<String> {
        self.set_layer_visible(id, true).then(|| "Show layer".to_owned())
    }

    /// The rows a drag of layer `id` in the panel takes along (see
    /// [`CanvasSession::move_next_to`]), it among them.
    pub fn moving_with(&self, id: u64) -> Vec<u64> {
        let layers = self.layers();
        let Some((unit, _)) = moving(&layers, id) else { return Vec::new() };
        layers.iter().map(|l| l.id).filter(|id| unit.contains(id)).collect()
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

    /// Layer `id`'s mask applied to it and removed, as when the two rows are pinched together.
    /// False if it has none, or either is locked.
    pub fn merge_mask(&self, id: u64) -> bool {
        let mask = self.layers().into_iter().find(|m| m.kind == LayerType::Mask && m.parent == Some(id));
        mask.is_some_and(|m| self.merge_between(m.id, id))
    }

    /// Blends layer or group `id` into the layer below and removes it.
    pub fn merge_down(&self, id: u64) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let id = LayerId(id);
        // a group's takes its top layer into the one under that, in the group or not
        let pair = if st.doc.layer(id).is_some_and(|l| l.kind == LayerKind::Group) {
            st.doc.group_merge_pair(id)
        } else {
            st.doc.merge_target(id).map(|lower| (id, lower))
        };
        let Some((upper, lower)) = pair else { return false };
        if [id, upper, lower].iter().any(|l| st.doc.layer(*l).is_none() || st.doc.is_locked(*l)) {
            return false;
        }
        let before = st.doc.clone();
        // a layer changes the one below only where it is painted: only that much is kept for undo
        // (none at all for an empty one), not a copy of the whole of it
        let within = st.painted_box(upper).unwrap_or(procraft_engine::geom::IRect::new(0, 0, 0, 0));
        if let Some(canvas) = st.canvas.as_mut() {
            // each mask applied to its layer first, as one step; a hidden layer below gives up
            // what it holds (the merge keeps what showed)
            let masks = [upper, lower].into_iter().filter_map(|l| st.doc.mask_of(l).map(|m| (l, m))).collect();
            let drop_lower = st.doc.layer(lower).is_some_and(|l| !l.visible);
            canvas.name_next_step("merge down");
            canvas.merge_down_with_masks(upper, lower, masks, drop_lower, before, Some(within));
        }
        st.doc.finish_merge_down(id);
        st.revision += 1;
        self.shared.frame(st);
        true
    }

    /// Shows or hides layer `id`: one step, named as measured ("Undo Toggle visibility").
    pub fn set_layer_visible(&self, id: u64, visible: bool) -> bool {
        self.change("Toggle visibility", |d| {
            changed(d, id, |l| l.visible != visible)
                .then(|| d.set_visible(LayerId(id), visible))
                .filter(|ok| *ok)
                .map(|_| ())
        })
        .is_some()
    }

    /// Shows layer `id` alone, the others left out of the picture (solo visibility), or with
    /// none every layer as it is set again: an undo step, as the reference's is. False if `id`
    /// isn't a layer.
    pub fn set_solo(&self, id: Option<u64>) -> bool {
        let solo = id.map(LayerId);
        if solo.is_some_and(|s| self.shared.lock().doc.layer(s).is_none()) {
            return false;
        }
        self.change("Solo visibility", |d| (d.solo != solo).then(|| d.solo = solo));
        true
    }

    /// The layer shown alone, if one is.
    pub fn solo(&self) -> Option<u64> {
        let st = self.shared.lock();
        st.doc.solo.filter(|s| st.doc.layer(*s).is_some()).map(|s| s.0)
    }

    /// Layer `id`'s visibility box tapped: while a layer is shown alone, every layer shows as it
    /// is set again (the others' boxes showed them hidden); otherwise the layer shows or hides.
    /// Returns what the notice band says, or None if nothing changed.
    pub fn tap_visibility(&self, id: u64) -> Option<String> {
        if self.solo().is_some() {
            return self.set_solo(None).then(|| "Solo visibility off".to_owned());
        }
        let visible = self.shared.lock().doc.layer(LayerId(id))?.visible;
        self.set_layer_visible(id, !visible).then(|| if visible { "Hide layer" } else { "Show layer" }.to_owned())
    }

    /// Layer `id`'s visibility box held: it shows alone, or, alone already, every layer shows as
    /// it is set again. Returns what the notice band says, or None if nothing changed.
    pub fn hold_visibility(&self, id: u64) -> Option<String> {
        let on = self.solo() != Some(id);
        self.set_solo(on.then_some(id))
            .then(|| if on { "Solo visibility on" } else { "Solo visibility off" }.to_owned())
    }

    /// Opacity 0..1; `done` ends the drag (one undo step for the whole drag).
    pub fn set_layer_opacity(&self, id: u64, opacity: f32, done: bool) -> bool {
        // a plugin moving picked layers together: each of them, if this is one
        let hooks = self.shared.lock().hooks.clone();
        let together = hooks.is_some_and(|hooks| hooks.opacity_together());
        self.change_live("layer opacity", done, |d| {
            let chosen = d.chosen();
            if !(together && chosen.len() > 1 && chosen.contains(&LayerId(id))) {
                return d.set_opacity(LayerId(id), opacity);
            }
            let layers: Vec<LayerId> =
                chosen.into_iter().filter(|&c| d.layer(c).is_some_and(|l| l.kind != LayerKind::Mask)).collect();
            layers.into_iter().fold(false, |any, c| d.set_opacity(c, opacity) | any)
        })
    }

    /// `blend` is a blend mode id (see [`blend_modes`]).
    pub fn set_layer_blend(&self, id: u64, blend: u32) -> bool {
        let Some(mode) = BlendMode::from_id(blend) else { return false };
        self.change("blend mode", |d| {
            changed(d, id, |l| l.blend != mode).then(|| d.set_blend(LayerId(id), mode)).filter(|ok| *ok).map(|_| ())
        })
        .is_some()
    }

    /// Turns layer `id`'s alpha lock on or off: one step, named as measured ("Undo Alpha Lock on").
    pub fn set_layer_alpha_lock(&self, id: u64, on: bool) -> bool {
        self.change(if on { "Alpha Lock on" } else { "Alpha Lock off" }, |d| {
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

    /// What locking (`on`) or unlocking layer `id` is called in the notice band, as measured:
    /// "Lock layer", or "Lock Layer Group" for a group; a layer locked by its group alone unlocks
    /// the group, so "Unlock Layer Group".
    pub fn lock_said(&self, id: u64, on: bool) -> String {
        lock_name(&self.shared.lock().doc, LayerId(id), on).to_owned()
    }

    /// Locks layer `id`, or unlocks it: as measured, a layer locked by its group shows locked and
    /// its Unlock unlocks the group (its own lock, if any, too). The step is named as
    /// [`CanvasSession::lock_said`] says, its first letter lowered (undo says "Undo lock layer",
    /// "Undo lock Layer Group").
    pub fn set_layer_locked(&self, id: u64, on: bool) -> bool {
        let said = lock_name(&self.shared.lock().doc, LayerId(id), on);
        let name = said[..1].to_lowercase() + &said[1..];
        self.change(&name, |d| {
            let id = LayerId(id);
            let differs = if on { d.layer(id).is_some_and(|l| !l.locked) } else { d.is_locked(id) };
            (differs && d.set_locked(id, on)).then_some(())
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

    /// The picture a time-lapse frame keeps, scaled to fit `max_side` pixels (premultiplied): as
    /// [`Self::canvas_thumbnail`], but without the private layers, so a private insert or paste
    /// never shows in the replay or its video. None before the canvas exists.
    pub fn time_lapse_frame(&self, max_side: u32) -> Option<ThumbnailImage> {
        // the copy started under the lock, waited for once it is let go: touches and frames go
        // on while the GPU copies it
        let pending = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return None };
            let private = st.doc.private_layers();
            canvas.start_composite_thumbnail_without(gpu, &mut st.doc, max_side, &private)
        };
        let t = pending.pixels().ok()?;
        Some(ThumbnailImage { width: t.width, height: t.height, rgba: t.rgba })
    }

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

    /// The layer scaled to fit `max_side` pixels, or none before the canvas exists.
    pub fn layer_thumbnail(&self, id: u64, max_side: u32) -> Option<ThumbnailImage> {
        // each read started under the lock and waited for once it is let go, so touches and
        // frames go on meanwhile (thumbnails are read after every change while Layers is open)
        let id = LayerId(id);
        let no_canvas = || procraft_gpu::GpuError::Readback("no canvas".into());
        let read = || -> Result<procraft_gpu::Thumbnail, procraft_gpu::GpuError> {
            let first = {
                let mut guard = self.shared.lock();
                let st = &mut *guard;
                let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return Err(no_canvas()) };
                canvas.start_layer_thumbnail(gpu, &mut st.doc, id, max_side)?
            };
            let (coarse, mask) = match first {
                procraft_gpu::LayerThumbnailStep::Ready(t) => return Ok(t),
                procraft_gpu::LayerThumbnailStep::Coarse { pending, mask } => (pending.pixels()?, mask),
            };
            let fine = {
                let mut guard = self.shared.lock();
                let st = &mut *guard;
                let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return Err(no_canvas()) };
                canvas.start_layer_thumbnail_fine(gpu, id, &coarse, mask, max_side)?
            };
            fine.pixels()
        };
        match read() {
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
    /// A copy of layer `id` above it, and made current: an undo step called `name`, `then` seeing
    /// to the document with the copy in it. None if there is no room.
    pub(crate) fn duplicated(&self, id: u64, name: &str, then: impl FnOnce(&mut Document, LayerId)) -> Option<u64> {
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
        then(&mut st.doc, copy);
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step(name);
            canvas.duplicate_layers(pairs, before);
        }
        st.revision += 1;
        self.shared.frame(st);
        Some(copy.0)
    }

    /// Merges what is chosen, or the layers between `between` (chosen first).
    fn merge_chosen(&self, between: Option<(LayerId, LayerId)>) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        // a mask pinched into its own layer is merged into it
        if let Some((a, b)) = between {
            let mask_of = |m: LayerId, l: LayerId| {
                st.doc.layer(m).is_some_and(|m| m.kind == LayerKind::Mask && m.parent == Some(l))
            };
            let mask = if mask_of(a, b) { Some((a, b)) } else { mask_of(b, a).then_some((b, a)) };
            if let Some((mask, layer)) = mask {
                if [mask, layer].iter().any(|l| st.doc.layer(*l).is_none() || st.doc.is_locked(*l)) {
                    return false;
                }
                let before = st.doc.clone();
                if let Some(canvas) = st.canvas.as_mut() {
                    canvas.name_next_step("merge mask");
                    canvas.apply_mask(layer, mask, before);
                }
                st.doc.delete_layer(mask);
                st.doc.set_current(layer);
                st.doc.rasterize(layer);
                st.revision += 1;
                self.shared.frame(st);
                return true;
            }
        }
        let chosen_before = st.doc.clone();
        if let Some((a, b)) = between
            && !st.doc.choose_between(a, b)
        {
            return false;
        }
        let Some((lower, uppers)) = st.doc.merge_chosen_plan() else {
            st.doc = chosen_before;
            return false;
        };
        // undone, the layers come back chosen as they were before the pinch
        let before = chosen_before;
        // the layer below changes only where they (a group's layers within it) are painted: only
        // that much is kept for undo, where the GPU can tell
        st.flush();
        let painted: Vec<LayerId> = uppers
            .iter()
            .flat_map(|&u| {
                if st.doc.layer(u).is_some_and(|l| l.kind == LayerKind::Group) { st.doc.contents(u) } else { vec![u] }
            })
            .collect();
        let within = match (st.canvas.as_ref(), st.gpu.as_ref()) {
            (Some(canvas), Some(gpu)) => canvas
                .painted_boxes_or_why(gpu, &painted)
                .into_iter()
                .try_fold(procraft_engine::geom::IRect::new(0, 0, 0, 0), |all, b| {
                    b.ok().map(|b| b.map_or(all, |b| if all.is_empty() { b } else { all.union(b) }))
                }),
            _ => None,
        };
        // each layer's mask applied to it first, as one step
        let masks = std::iter::once(lower)
            .chain(uppers.iter().copied())
            .filter_map(|l| st.doc.mask_of(l).map(|m| (l, m)))
            .collect();
        let drop_lower = st.doc.layer(lower).is_some_and(|l| !l.visible);
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step("merge layers");
            canvas.merge_layers_with_masks(uppers.clone(), lower, masks, drop_lower, before, within);
        }
        st.doc.finish_merge_chosen(lower, &uppers);
        st.revision += 1;
        self.shared.frame(st);
        true
    }

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
    pub(crate) fn change_live(&self, name: &str, done: bool, f: impl FnOnce(&mut Document) -> bool) -> bool {
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

/// Why the current layer takes no change (see [`CanvasSession::why_blocked`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Blocked {
    Locked,
    /// A layer that doesn't show: hidden itself, in a hidden group (a mask, on a hidden layer),
    /// or left out while another layer is shown alone.
    Hidden,
    EmptyGroup,
    /// On a model, every mesh a stroke is meant for is hidden.
    MeshHidden,
    /// On a model, every mesh a stroke is meant for is hidden or locked.
    MeshLocked,
    /// A group that doesn't show (as [`Blocked::Hidden`] is a layer).
    GroupHidden,
    /// A group is current where a stroke or a fill takes a layer
    /// ([`CanvasSession::why_unpainted`]).
    Group,
    /// Liquify on a layer whose alpha is locked ([`CanvasSession::why_not_adjusted`]).
    AlphaLocked,
}

/// Why a change to the current layer would go unseen (see [`CanvasSession::hidden_by`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum HiddenBy {
    /// Its own box is off.
    Itself,
    /// A mask whose layer is hidden.
    MaskLayer,
    /// A group it is in is hidden.
    Group,
    /// A group with something hidden in it.
    Inside,
    /// Left out while another layer is shown alone, and so is every layer picked with it.
    Solo,
}

/// What a drag of layer `id` takes along, and whether that is the chosen layers together: it with
/// its mask (or a group with what is in it), or, the current layer with others picked (or a picked
/// one), them all, a picked mask going with its layer.
fn moving(layers: &[LayerInfo], id: u64) -> Option<(std::collections::HashSet<u64>, bool)> {
    let l = layers.iter().find(|l| l.id == id)?;
    let together = layers.iter().any(|l| l.picked) && (l.current || l.picked);
    let unit = if together {
        layers.iter().filter(|c| c.current || c.picked).flat_map(|c| unit_of(layers, owner_of(layers, c))).collect()
    } else {
        unit_of(layers, l)
    };
    Some((unit, together))
}

/// A mask's layer; anything else itself.
fn owner_of<'a>(layers: &'a [LayerInfo], l: &'a LayerInfo) -> &'a LayerInfo {
    if l.kind == LayerType::Mask { layers.iter().find(|o| Some(o.id) == l.parent).unwrap_or(l) } else { l }
}

/// Whether `l` is somewhere inside group `group`.
fn is_inside(layers: &[LayerInfo], l: &LayerInfo, group: u64) -> bool {
    let mut at = owner_of(layers, l).parent;
    for _ in 0..layers.len() {
        let Some(id) = at else { return false };
        if id == group {
            return true;
        }
        at = layers.iter().find(|o| o.id == id).and_then(|o| o.parent);
    }
    false
}

/// The rows that go with `l`: it with its mask, or a group with everything in it.
fn unit_of(layers: &[LayerInfo], l: &LayerInfo) -> std::collections::HashSet<u64> {
    layers
        .iter()
        .filter(|o| {
            o.id == l.id
                || (o.kind == LayerType::Mask && o.parent == Some(l.id))
                || (l.kind == LayerType::Group && is_inside(layers, o, l.id))
        })
        .map(|o| o.id)
        .collect()
}

impl CanvasSession {
    /// A new empty layer above the current one, as [`CanvasSession::add_layer`] adds, telling no
    /// plugin: for the core's own work that makes one (a picture of a model, a selection copied to a
    /// layer, a plugin's own).
    pub(crate) fn add_layer_quietly(&self) -> u64 {
        self.change("new layer", |d| d.has_room(1).then(|| d.add_layer().0)).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use crate::CanvasSession;

    #[test]
    fn a_row_dropped_by_another_goes_above_or_below_it_with_its_mask() {
        let s = CanvasSession::new(64, 64);
        let bottom = s.layers().first().map(|l| l.id).unwrap_or_default();
        let middle = s.add_layer();
        let top = s.add_layer();
        let order = |s: &CanvasSession| {
            s.layers().iter().filter(|l| l.kind != super::LayerType::Mask).map(|l| l.id).collect::<Vec<_>>()
        };
        assert_eq!(order(&s), vec![top, middle, bottom]);
        assert!(s.move_next_to(bottom, top, true));
        assert_eq!(order(&s), vec![bottom, top, middle]);
        assert!(s.move_next_to(bottom, middle, false));
        assert_eq!(order(&s), vec![top, middle, bottom]);
        assert!(!s.move_next_to(top, top, true), "onto itself it stays");
        assert_eq!(s.why_blocked(), None);
        s.set_layer_locked(top, true);
        assert_eq!(s.why_blocked(), Some(super::Blocked::Locked));
        s.set_layer_locked(top, false);
        s.set_layer_visible(top, false);
        assert_eq!(s.why_blocked(), Some(super::Blocked::Hidden));
        s.set_layer_visible(top, true);
        // a layer's mask goes with it
        let mask = s.add_mask(middle).unwrap_or_default();
        assert!(s.move_next_to(middle, top, true));
        let ids: Vec<u64> = s.layers().iter().map(|l| l.id).collect();
        let at = |id| ids.iter().position(|&i| i == id).unwrap_or(usize::MAX);
        assert!(at(mask) < at(middle) && at(middle) < at(top), "{ids:?}");
        assert!(!s.move_next_to(middle, mask, true), "onto its own mask it stays");
        let mut with = s.moving_with(middle);
        with.sort_unstable();
        let mut expected = vec![middle, mask];
        expected.sort_unstable();
        assert_eq!(with, expected);
    }
}
