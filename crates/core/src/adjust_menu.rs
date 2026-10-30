//! The Adjustments menu, the same in every app: each adjustment's name, its group in the menu,
//! whether its amount is swiped across the canvas, and how much it starts at when painted on
//! (Pencil).

use crate::quick_menu::QuickAction;

/// An adjustment from the Adjustments menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Adjustment {
    Hsb,
    ColorBalance,
    Curves,
    GradientMap,
    GaussianBlur,
    MotionBlur,
    PerspectiveBlur,
    Noise,
    Sharpen,
    Bloom,
    Glitch,
    Halftone,
    ChromaticAberration,
    Liquify,
    Clone,
}

/// Each adjustment: its name, whether its amount is swiped, and its amount when first painted on.
const ADJUSTMENTS: [(Adjustment, &str, bool, Option<f32>); 15] = [
    (Adjustment::Hsb, "Hue, Saturation, Brightness", false, None),
    (Adjustment::ColorBalance, "Color Balance", false, None),
    (Adjustment::Curves, "Curves", false, None),
    (Adjustment::GradientMap, "Gradient Map", true, None),
    (Adjustment::GaussianBlur, "Gaussian Blur", true, Some(0.6)),
    (Adjustment::MotionBlur, "Motion Blur", true, Some(0.6)),
    (Adjustment::PerspectiveBlur, "Perspective Blur", true, Some(0.4)),
    (Adjustment::Noise, "Noise", true, Some(0.69)),
    (Adjustment::Sharpen, "Sharpen", true, Some(0.4)),
    (Adjustment::Bloom, "Bloom", true, Some(0.5)),
    (Adjustment::Glitch, "Glitch", true, Some(0.5)),
    (Adjustment::Halftone, "Halftone", true, Some(0.12)),
    (Adjustment::ChromaticAberration, "Chromatic Aberration", true, Some(1.0)),
    (Adjustment::Liquify, "Liquify", false, None),
    (Adjustment::Clone, "Clone", false, None),
];

fn entry(a: Adjustment) -> (Adjustment, &'static str, bool, Option<f32>) {
    ADJUSTMENTS.iter().copied().find(|e| e.0 == a).unwrap_or(ADJUSTMENTS[0])
}

/// What the menu calls it.
#[uniffi::export]
pub fn adjustment_label(adjustment: Adjustment) -> String {
    entry(adjustment).1.to_owned()
}

/// Whether its amount is swiped across the canvas.
#[uniffi::export]
pub fn adjustment_swiped(adjustment: Adjustment) -> bool {
    entry(adjustment).2
}

/// Its amount when first painted on with the brush, if it starts at one of its own.
#[uniffi::export]
pub fn adjustment_pencil_amount(adjustment: Adjustment) -> Option<f32> {
    entry(adjustment).3
}

/// Whether it can be painted on with the brush (Pencil) instead of changing the whole layer.
#[uniffi::export]
pub fn adjustment_paintable(adjustment: Adjustment) -> bool {
    !matches!(adjustment, Adjustment::Liquify | Adjustment::Clone)
}

/// The menu's groups in order: colour, blurs, effects, painting.
#[uniffi::export]
pub fn adjustment_groups() -> Vec<Vec<Adjustment>> {
    use Adjustment as A;
    vec![
        vec![A::Hsb, A::ColorBalance, A::Curves, A::GradientMap],
        vec![A::GaussianBlur, A::MotionBlur, A::PerspectiveBlur],
        vec![A::Noise, A::Sharpen, A::Bloom, A::Glitch, A::Halftone, A::ChromaticAberration],
        vec![A::Liquify, A::Clone],
    ]
}

/// The adjustment a QuickMenu action opens, if it is one of them.
#[uniffi::export]
pub fn quick_action_adjustment(action: QuickAction) -> Option<Adjustment> {
    use Adjustment as A;
    Some(match action {
        QuickAction::Hsb => A::Hsb,
        QuickAction::ColorBalance => A::ColorBalance,
        QuickAction::Curves => A::Curves,
        QuickAction::GradientMap => A::GradientMap,
        QuickAction::GaussianBlur => A::GaussianBlur,
        QuickAction::MotionBlur => A::MotionBlur,
        QuickAction::PerspectiveBlur => A::PerspectiveBlur,
        QuickAction::Noise => A::Noise,
        QuickAction::Sharpen => A::Sharpen,
        QuickAction::Bloom => A::Bloom,
        QuickAction::Glitch => A::Glitch,
        QuickAction::Halftone => A::Halftone,
        QuickAction::ChromaticAberration => A::ChromaticAberration,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_adjustment_is_in_the_menu_once() {
        let all: Vec<Adjustment> = adjustment_groups().into_iter().flatten().collect();
        assert_eq!(all.len(), ADJUSTMENTS.len());
        for e in ADJUSTMENTS {
            assert_eq!(all.iter().filter(|&&a| a == e.0).count(), 1, "{:?}", e.0);
        }
    }

    #[test]
    fn the_quick_menus_adjustments_open_theirs() {
        for a in crate::quick_menu::quick_adjustments_listed() {
            let adjustment = quick_action_adjustment(a);
            assert!(adjustment.is_some(), "{a:?}");
            assert_eq!(adjustment.map(adjustment_label), Some(crate::quick_menu::quick_action_label(a)));
        }
        assert_eq!(quick_action_adjustment(QuickAction::Undo), None);
    }
}
