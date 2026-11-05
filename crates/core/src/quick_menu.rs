//! The QuickMenu's actions: what each of its six buttons can be set to, by the name its picker
//! lists, the picker's order, the buttons a menu starts with, and how the buttons are kept. What
//! an action does stays with each app, as it opens their panels and tools.

/// What a QuickMenu button can be set to. The adjustments are listed under Adjustments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum QuickAction {
    None,
    Guide2d,
    ActualSize,
    AddText,
    AlphaLock,
    AnimationAssist,
    BrushScaling,
    ClearLayer,
    CopyPasteMenu,
    Clone,
    Copy,
    CopyCanvas,
    Cut,
    DeleteLayer,
    DrawingAssist,
    DuplicateLayer,
    Erase,
    Eyedropper,
    FillLayer,
    FitCanvas,
    FlipHorizontal,
    FlipVertical,
    Interface,
    GuideIsometric,
    LayerSelect,
    LayerOpacity,
    LayerVisibility,
    Liquify,
    MergeDown,
    NewLayer,
    NextFrame,
    OnionSkin,
    Adjustments,
    Colors,
    Layers,
    PageAssist,
    Paint,
    Paste,
    GuidePerspective,
    PreviousBrush,
    SwapColors,
    PreviousFrame,
    ProjectCanvas,
    QuickshapeOn,
    Quickshape,
    Recolor,
    Redo,
    Reference,
    Brushes,
    SelectLayer,
    Selection,
    CompanionMove,
    CompanionZoom,
    Smudge,
    Solo,
    GuideSymmetry,
    Transform,
    Undo,
    Bloom,
    ChromaticAberration,
    ColorBalance,
    Curves,
    GaussianBlur,
    Glitch,
    GradientMap,
    Halftone,
    Hsb,
    MotionBlur,
    Noise,
    PerspectiveBlur,
    Sharpen,
}

/// Each action: the name it is kept under (so a button keeps its action when the label changes),
/// its label, and whether it is one of the adjustments.
const ACTIONS: [(QuickAction, &str, &str, bool); 71] = [
    (QuickAction::None, "NONE", "No Action", false),
    (QuickAction::Guide2d, "GUIDE_2D", "2D Guide visibility", false),
    (QuickAction::ActualSize, "ACTUAL_SIZE", "Actual size", false),
    (QuickAction::AddText, "ADD_TEXT", "Add Text", false),
    (QuickAction::AlphaLock, "ALPHA_LOCK", "Alpha Lock", false),
    (QuickAction::AnimationAssist, "ANIMATION_ASSIST", "Animation Assist", false),
    (QuickAction::BrushScaling, "BRUSH_SCALING", "Brush scaling", false),
    (QuickAction::ClearLayer, "CLEAR_LAYER", "Clear layer", false),
    (QuickAction::CopyPasteMenu, "COPY_PASTE_MENU", "Clipboard", false),
    (QuickAction::Clone, "CLONE", "Clone", false),
    (QuickAction::Copy, "COPY", "Copy", false),
    (QuickAction::CopyCanvas, "COPY_CANVAS", "Copy canvas", false),
    (QuickAction::Cut, "CUT", "Cut", false),
    (QuickAction::DeleteLayer, "DELETE_LAYER", "Delete layer", false),
    (QuickAction::DrawingAssist, "DRAWING_ASSIST", "Drawing Assist", false),
    (QuickAction::DuplicateLayer, "DUPLICATE_LAYER", "Duplicate layer", false),
    (QuickAction::Erase, "ERASE", "Erase", false),
    (QuickAction::Eyedropper, "EYEDROPPER", "Eyedropper", false),
    (QuickAction::FillLayer, "FILL_LAYER", "Fill layer", false),
    (QuickAction::FitCanvas, "FIT_CANVAS", "Fit canvas", false),
    (QuickAction::FlipHorizontal, "FLIP_HORIZONTAL", "Flip horizontally", false),
    (QuickAction::FlipVertical, "FLIP_VERTICAL", "Flip vertically", false),
    (QuickAction::Interface, "INTERFACE", "Full screen", false),
    (QuickAction::GuideIsometric, "GUIDE_ISOMETRIC", "Isometric Guide visibility", false),
    (QuickAction::LayerSelect, "LAYER_SELECT", "Layer Select", false),
    (QuickAction::LayerOpacity, "LAYER_OPACITY", "Layer opacity", false),
    (QuickAction::LayerVisibility, "LAYER_VISIBILITY", "Layer visibility", false),
    (QuickAction::Liquify, "LIQUIFY", "Liquify", false),
    (QuickAction::MergeDown, "MERGE_DOWN", "Merge down", false),
    (QuickAction::NewLayer, "NEW_LAYER", "New layer", false),
    (QuickAction::NextFrame, "NEXT_FRAME", "Next frame", false),
    (QuickAction::OnionSkin, "ONION_SKIN", "Onion skin", false),
    (QuickAction::Adjustments, "ADJUSTMENTS", "Open Adjustments", false),
    (QuickAction::Colors, "COLORS", "Open Color panel", false),
    (QuickAction::Layers, "LAYERS", "Open Layers panel", false),
    (QuickAction::PageAssist, "PAGE_ASSIST", "Page Assist", false),
    (QuickAction::Paint, "PAINT", "Paint", false),
    (QuickAction::Paste, "PASTE", "Paste", false),
    (QuickAction::GuidePerspective, "GUIDE_PERSPECTIVE", "Perspective Guide visibility", false),
    (QuickAction::PreviousBrush, "PREVIOUS_BRUSH", "Previous brush", false),
    (QuickAction::SwapColors, "SWAP_COLORS", "Previous color", false),
    (QuickAction::PreviousFrame, "PREVIOUS_FRAME", "Previous frame", false),
    (QuickAction::ProjectCanvas, "PROJECT_CANVAS", "Project canvas", false),
    (QuickAction::QuickshapeOn, "QUICKSHAPE_ON", "QuickShape", false),
    (QuickAction::Quickshape, "QUICKSHAPE", "QuickShape stroke", false),
    (QuickAction::Recolor, "RECOLOR", "Recolor", false),
    (QuickAction::Redo, "REDO", "Redo", false),
    (QuickAction::Reference, "REFERENCE", "Reference", false),
    (QuickAction::Brushes, "BRUSHES", "Select brush", false),
    (QuickAction::SelectLayer, "SELECT_LAYER", "Select layer contents", false),
    (QuickAction::Selection, "SELECTION", "Selection tool", false),
    (QuickAction::CompanionMove, "COMPANION_MOVE", "Single touch move", false),
    (QuickAction::CompanionZoom, "COMPANION_ZOOM", "Single touch zoom", false),
    (QuickAction::Smudge, "SMUDGE", "Smudge", false),
    (QuickAction::Solo, "SOLO", "Solo visibility", false),
    (QuickAction::GuideSymmetry, "GUIDE_SYMMETRY", "Symmetry Guide visibility", false),
    (QuickAction::Transform, "TRANSFORM", "Transform tool", false),
    (QuickAction::Undo, "UNDO", "Undo", false),
    (QuickAction::Bloom, "BLOOM", "Bloom", true),
    (QuickAction::ChromaticAberration, "CHROMATIC_ABERRATION", "Chromatic Aberration", true),
    (QuickAction::ColorBalance, "COLOR_BALANCE", "Color Balance", true),
    (QuickAction::Curves, "CURVES", "Curves", true),
    (QuickAction::GaussianBlur, "GAUSSIAN_BLUR", "Gaussian Blur", true),
    (QuickAction::Glitch, "GLITCH", "Glitch", true),
    (QuickAction::GradientMap, "GRADIENT_MAP", "Gradient Map", true),
    (QuickAction::Halftone, "HALFTONE", "Halftone", true),
    (QuickAction::Hsb, "HSB", "Hue, Saturation, Brightness", true),
    (QuickAction::MotionBlur, "MOTION_BLUR", "Motion Blur", true),
    (QuickAction::Noise, "NOISE", "Noise", true),
    (QuickAction::PerspectiveBlur, "PERSPECTIVE_BLUR", "Perspective Blur", true),
    (QuickAction::Sharpen, "SHARPEN", "Sharpen", true),
];

/// The buttons a QuickMenu starts with, clockwise from the top.
const DEFAULTS: [QuickAction; 6] = [
    QuickAction::NewLayer,
    QuickAction::FlipHorizontal,
    QuickAction::Copy,
    QuickAction::MergeDown,
    QuickAction::ClearLayer,
    QuickAction::FlipVertical,
];

fn entry(action: QuickAction) -> (QuickAction, &'static str, &'static str, bool) {
    ACTIONS.iter().copied().find(|e| e.0 == action).unwrap_or(ACTIONS[0])
}

/// What an action's row and button say.
#[uniffi::export]
pub fn quick_action_label(action: QuickAction) -> String {
    entry(action).2.to_owned()
}

/// Whether the action is one of the adjustments (listed under Adjustments).
#[uniffi::export]
pub fn quick_action_adjusts(action: QuickAction) -> bool {
    entry(action).3
}

/// The picker's rows: the actions by name (as written, capitals first), `None` for Adjustments.
#[uniffi::export]
pub fn quick_actions_listed() -> Vec<Option<QuickAction>> {
    let mut rows: Vec<(&str, Option<QuickAction>)> =
        ACTIONS.iter().filter(|e| !e.3).map(|e| (e.2, Some(e.0))).collect();
    rows.push(("Adjustments", None));
    rows.sort_by(|a, b| a.0.cmp(b.0));
    rows.into_iter().map(|r| r.1).collect()
}

/// The rows under Adjustments, by name.
#[uniffi::export]
pub fn quick_adjustments_listed() -> Vec<QuickAction> {
    let mut rows: Vec<(&str, QuickAction)> = ACTIONS.iter().filter(|e| e.3).map(|e| (e.2, e.0)).collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    rows.into_iter().map(|r| r.1).collect()
}

/// The QuickMenu's buttons as kept in `saved` (see [`quick_menu_saved`]), or as a menu starts when
/// nothing is kept or it is not six. A name not known any more is No Action.
#[uniffi::export]
pub fn quick_menu_slots(saved: Option<String>) -> Vec<QuickAction> {
    let slots: Option<Vec<QuickAction>> = saved
        .map(|s| s.split(',').map(|n| ACTIONS.iter().find(|e| e.1 == n).map_or(QuickAction::None, |e| e.0)).collect());
    slots.filter(|s| s.len() == DEFAULTS.len()).unwrap_or_else(|| DEFAULTS.to_vec())
}

/// The QuickMenu's buttons as kept: their names, joined with commas.
#[uniffi::export]
pub fn quick_menu_saved(slots: Vec<QuickAction>) -> String {
    slots.iter().map(|&a| entry(a).1).collect::<Vec<_>>().join(",")
}

/// One of the QuickMenu's menus: its name (shown in the middle) and its six buttons, each with
/// the key of the brush it is set to (see `CanvasSession::quick_brush`) when it is Select brush,
/// else 0 (a Select brush button with none opens the brush library).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct QuickMenuSet {
    pub name: String,
    pub slots: Vec<QuickAction>,
    pub brushes: Vec<u64>,
}

/// How a button set to a brush is kept: this, then the brush's key.
const BRUSH_KEPT: &str = "BRUSH:";

/// A menu's buttons as kept (see [`quick_menus_saved`]): their actions and brushes' keys, or none
/// when they are not six.
fn menu_slots(saved: &str) -> Option<(Vec<QuickAction>, Vec<u64>)> {
    let slots: Vec<(QuickAction, u64)> = saved
        .split(',')
        .map(|n| match n.strip_prefix(BRUSH_KEPT).and_then(|k| k.parse::<u64>().ok()) {
            Some(key) => (QuickAction::Brushes, key),
            None => (ACTIONS.iter().find(|e| e.1 == n).map_or(QuickAction::None, |e| e.0), 0),
        })
        .collect();
    (slots.len() == DEFAULTS.len()).then(|| slots.into_iter().unzip())
}

fn menu_name(n: usize) -> String {
    format!("QuickMenu {n}")
}

/// The QuickMenu's menus as kept in `saved` (see [`quick_menus_saved`]), oldest first. With none
/// kept there is one, "QuickMenu 1", with the buttons kept before there were several (`single`,
/// see [`quick_menu_slots`]).
#[uniffi::export]
pub fn quick_menus(saved: Option<String>, single: Option<String>) -> Vec<QuickMenuSet> {
    let menus: Vec<QuickMenuSet> = saved
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (name, slots) = line.split_once('\t')?;
            let (slots, brushes) = menu_slots(slots).unwrap_or_else(|| (DEFAULTS.to_vec(), vec![0; DEFAULTS.len()]));
            (!name.trim().is_empty()).then(|| QuickMenuSet { name: name.to_owned(), slots, brushes })
        })
        .collect();
    if menus.is_empty() {
        vec![QuickMenuSet { name: menu_name(1), slots: quick_menu_slots(single), brushes: vec![0; DEFAULTS.len()] }]
    } else {
        menus
    }
}

/// The menus as kept: a line each, the name and the buttons (see [`quick_menu_saved`]) split by a
/// tab.
#[uniffi::export]
pub fn quick_menus_saved(menus: Vec<QuickMenuSet>) -> String {
    let slot = |(i, &a): (usize, &QuickAction), brushes: &[u64]| match brushes.get(i).copied().unwrap_or(0) {
        key if a == QuickAction::Brushes && key != 0 => format!("{BRUSH_KEPT}{key}"),
        _ => entry(a).1.to_owned(),
    };
    menus
        .iter()
        .map(|m| {
            let slots: Vec<String> = m.slots.iter().enumerate().map(|s| slot(s, &m.brushes)).collect();
            format!("{}\t{}", m.name.replace(['\t', '\n', '\r'], " "), slots.join(","))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The menus with a new one after them: "QuickMenu N", N one more than there are (or the next
/// not taken), its six buttons No Action.
#[uniffi::export]
pub fn quick_menu_added(menus: Vec<QuickMenuSet>) -> Vec<QuickMenuSet> {
    let mut n = menus.len() + 1;
    while menus.iter().any(|m| m.name == menu_name(n)) {
        n += 1;
    }
    let mut menus = menus;
    menus.push(QuickMenuSet {
        name: menu_name(n),
        slots: vec![QuickAction::None; DEFAULTS.len()],
        brushes: vec![0; DEFAULTS.len()],
    });
    menus
}

/// Which menu is active once menu `removed` has gone, `active` having been: the same one, or the
/// one before the active one when that went (the first when the first went).
#[uniffi::export]
pub fn quick_menu_active_after(active: u32, removed: u32) -> u32 {
    if removed < active || (removed == active && active > 0) { active - 1 } else { active }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_is_named_once() {
        for (i, e) in ACTIONS.iter().enumerate() {
            assert!(ACTIONS.iter().skip(i + 1).all(|f| f.0 != e.0 && f.1 != e.1), "{e:?}");
        }
    }

    #[test]
    fn the_picker_lists_by_name_with_adjustments_among_them() {
        let rows = quick_actions_listed();
        let labels: Vec<String> = rows.iter().map(|r| r.map_or("Adjustments".into(), quick_action_label)).collect();
        let mut sorted = labels.clone();
        sorted.sort();
        assert_eq!(labels, sorted);
        assert_eq!(labels.first().map(String::as_str), Some("2D Guide visibility"));
        assert!(rows.contains(&None));
        assert!(!rows.contains(&Some(QuickAction::Bloom)));
        assert_eq!(quick_adjustments_listed().first(), Some(&QuickAction::Bloom));
        assert_eq!(quick_adjustments_listed().len(), 13);
    }

    #[test]
    fn the_buttons_are_kept_by_name() {
        assert_eq!(quick_menu_slots(None), DEFAULTS.to_vec());
        let kept = "UNDO,GUIDE_2D,NONE,GAUSSIAN_BLUR,GONE,REDO";
        let slots = quick_menu_slots(Some(kept.into()));
        assert_eq!(slots[1], QuickAction::Guide2d);
        assert_eq!(slots[4], QuickAction::None);
        assert_eq!(quick_menu_saved(slots), "UNDO,GUIDE_2D,NONE,GAUSSIAN_BLUR,NONE,REDO");
        // not six: as a menu starts
        assert_eq!(quick_menu_slots(Some("UNDO,REDO".into())), DEFAULTS.to_vec());
    }

    #[test]
    fn the_menus_are_kept_by_name_and_the_first_takes_the_buttons_kept_before() {
        let first = quick_menus(None, Some("UNDO,GUIDE_2D,NONE,GAUSSIAN_BLUR,NONE,REDO".into()));
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].name, "QuickMenu 1");
        assert_eq!(first[0].slots[0], QuickAction::Undo);
        assert_eq!(quick_menus(None, None)[0].slots, DEFAULTS.to_vec());
        let two = quick_menu_added(first);
        assert_eq!(two[1].name, "QuickMenu 2");
        assert_eq!(two[1].slots, vec![QuickAction::None; 6]);
        let kept = quick_menus_saved(two.clone());
        assert_eq!(quick_menus(Some(kept), None), two);
        // a name taken is skipped
        let gone = vec![two[1].clone()];
        assert_eq!(quick_menu_added(gone)[1].name, "QuickMenu 3");
    }

    #[test]
    fn a_button_set_to_a_brush_keeps_its_key() {
        let mut menus = quick_menus(None, None);
        menus[0].slots[2] = QuickAction::Brushes;
        menus[0].brushes[2] = 7;
        menus[0].slots[3] = QuickAction::Brushes;
        let kept = quick_menus_saved(menus.clone());
        assert!(kept.contains(",BRUSH:7,BRUSHES,"), "{kept}");
        assert_eq!(quick_menus(Some(kept), None), menus);
    }

    #[test]
    fn deleting_a_menu_keeps_the_active_one_or_takes_the_one_before() {
        assert_eq!(quick_menu_active_after(1, 1), 0);
        assert_eq!(quick_menu_active_after(2, 0), 1);
        assert_eq!(quick_menu_active_after(0, 1), 0);
        assert_eq!(quick_menu_active_after(0, 0), 0);
    }
}
