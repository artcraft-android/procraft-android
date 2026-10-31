//! Which open modes a button ends: the tools, the panels and the mode buttons each end what they
//! leave behind first (keeping what it made), so the apps go between transform, selection, the
//! adjustments, Recolor and the rest alike. The platforms say what is open and end what this says.

/// The modes that can be open on the canvas, each as the app shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct Modes {
    /// A snapped QuickShape being edited.
    pub quickshape: bool,
    /// ColorDrop's Continue filling.
    pub filling: bool,
    pub recolor: bool,
    pub layer_opacity: bool,
    pub transform: bool,
    pub selection: bool,
    pub adjustment: bool,
}

impl Modes {
    /// Every mode in `self` but those in `kept`.
    fn but(self, kept: Modes) -> Modes {
        Modes {
            quickshape: self.quickshape && !kept.quickshape,
            filling: self.filling && !kept.filling,
            recolor: self.recolor && !kept.recolor,
            layer_opacity: self.layer_opacity && !kept.layer_opacity,
            transform: self.transform && !kept.transform,
            selection: self.selection && !kept.selection,
            adjustment: self.adjustment && !kept.adjustment,
        }
    }

    /// Only the modes in `self` that are also in `only`.
    fn only(self, only: Modes) -> Modes {
        self.but(self.but(only))
    }
}

/// No mode open.
const NONE: Modes = Modes {
    quickshape: false,
    filling: false,
    recolor: false,
    layer_opacity: false,
    transform: false,
    selection: false,
    adjustment: false,
};

/// A button pressed that goes between modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ModeButton {
    /// Paint, Smudge or Erase tapped.
    Tool,
    /// A tool's button held: that tool takes the current brush.
    ToolHeld,
    /// A panel's button (the brushes', layers', Actions'); the Colour panel's (`colour`) leaves
    /// Recolor open, its colour being what Recolor fills with.
    Panel {
        colour: bool,
    },
    Transform,
    Selection,
    /// The Adjustments button.
    Adjustments,
    /// An adjustment chosen from the Adjustments menu.
    Adjustment,
    Recolor,
    LayerOpacity,
    /// The button that hides the interface.
    Bars,
}

/// What a press does with the modes open: the ones it ends first, keeping what they made, and
/// whether it then does what it does (`goes_on`). Not going on, a tool tapped only picks its tool
/// (its brushes don't open), and any other button only ends what it ended, or nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct ModeSwitch {
    pub end: Modes,
    pub goes_on: bool,
}

/// What pressing `button` does with the modes `open`:
/// - a tool ends every mode; with one of them open, it only picks its tool;
/// - a tool held is refused while a transform, a selection or an adjustment is open;
/// - a panel ends what works on the canvas (transform, selection, an adjustment, Recolor but for
///   the Colour panel's) and leaves Continue filling and Layer Opacity open;
/// - a mode's own button ends it when it is open, and otherwise ends every other mode and opens
///   it (Layer Opacity starts again on the current layer);
/// - the Adjustments button ends Layer Opacity, Recolor or the open adjustment, whichever is open,
///   and otherwise opens its menu as a panel does;
/// - hiding the interface is refused while transforming.
///
/// A QuickShape being edited ends with every press but the interface's.
#[uniffi::export]
pub fn mode_switch(button: ModeButton, open: Modes) -> ModeSwitch {
    let shape = Modes { quickshape: true, ..NONE };
    let ends = |kept: Modes, goes_on: bool| ModeSwitch { end: open.but(kept), goes_on };
    let own = |mode: Modes| {
        if open.only(mode) == NONE {
            ends(mode, true)
        } else {
            // its own button ends it, with a shape being edited
            ModeSwitch { end: open.only(Modes { quickshape: true, ..mode }), goes_on: false }
        }
    };
    match button {
        ModeButton::Tool => {
            let busy = open.but(shape) != NONE;
            ends(NONE, !busy)
        }
        ModeButton::ToolHeld => {
            if open.transform || open.selection || open.adjustment {
                ModeSwitch { end: NONE, goes_on: false }
            } else {
                ModeSwitch { end: open.only(shape), goes_on: true }
            }
        }
        ModeButton::Panel { colour } => {
            ends(Modes { filling: true, layer_opacity: true, recolor: colour, ..NONE }, true)
        }
        ModeButton::Transform => own(Modes { transform: true, ..NONE }),
        ModeButton::Selection => own(Modes { selection: true, ..NONE }),
        ModeButton::Recolor => own(Modes { recolor: true, ..NONE }),
        ModeButton::LayerOpacity => ends(Modes { layer_opacity: true, ..NONE }, true),
        // a new adjustment ends the one open too
        ModeButton::Adjustment => ends(NONE, true),
        ModeButton::Adjustments => {
            let one = Modes { layer_opacity: true, recolor: true, adjustment: true, ..NONE };
            if open.only(one) == NONE {
                ends(NONE, true)
            } else {
                // the first of them that is open ends, alone
                let first = if open.layer_opacity {
                    Modes { layer_opacity: true, ..NONE }
                } else if open.recolor {
                    Modes { recolor: true, ..NONE }
                } else {
                    Modes { adjustment: true, ..NONE }
                };
                ModeSwitch { end: open.only(Modes { quickshape: true, filling: true, ..first }), goes_on: false }
            }
        }
        ModeButton::Bars => ModeSwitch { end: NONE, goes_on: !open.transform },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Modes = Modes {
        quickshape: true,
        filling: true,
        recolor: true,
        layer_opacity: true,
        transform: true,
        selection: true,
        adjustment: true,
    };

    fn open(f: impl FnOnce(&mut Modes)) -> Modes {
        let mut m = NONE;
        f(&mut m);
        m
    }

    #[test]
    fn a_tool_ends_every_mode_and_then_only_picks_itself() {
        // nothing open: a tool as usual
        assert_eq!(mode_switch(ModeButton::Tool, NONE), ModeSwitch { end: NONE, goes_on: true });
        // a shape being edited ends, and the tool is as usual
        let s = mode_switch(ModeButton::Tool, open(|m| m.quickshape = true));
        assert!(s.end.quickshape && s.goes_on);
        // a transform and a selection end; the tap only picks the tool
        let s = mode_switch(ModeButton::Tool, open(|m| (m.transform, m.selection) = (true, true)));
        assert_eq!(s, ModeSwitch { end: open(|m| (m.transform, m.selection) = (true, true)), goes_on: false });
        let s = mode_switch(ModeButton::Tool, open(|m| m.filling = true));
        assert!(s.end.filling && !s.goes_on);
    }

    #[test]
    fn a_held_tool_waits_for_a_transform_a_selection_or_an_adjustment() {
        for busy in [open(|m| m.transform = true), open(|m| m.selection = true), open(|m| m.adjustment = true)] {
            assert_eq!(mode_switch(ModeButton::ToolHeld, busy), ModeSwitch { end: NONE, goes_on: false });
        }
        let s = mode_switch(ModeButton::ToolHeld, open(|m| (m.quickshape, m.recolor) = (true, true)));
        assert_eq!(s, ModeSwitch { end: open(|m| m.quickshape = true), goes_on: true });
    }

    #[test]
    fn a_panel_ends_what_works_on_the_canvas_and_the_colour_panel_keeps_recolor() {
        let s = mode_switch(ModeButton::Panel { colour: false }, ALL);
        assert_eq!(s.end, open(|m| *m = Modes { filling: false, layer_opacity: false, ..ALL }));
        assert!(s.goes_on);
        let s = mode_switch(ModeButton::Panel { colour: true }, ALL);
        assert!(!s.end.recolor && s.end.transform);
    }

    #[test]
    fn a_mode_s_own_button_ends_it_else_ends_the_others_and_opens_it() {
        let s = mode_switch(ModeButton::Transform, open(|m| (m.transform, m.quickshape) = (true, true)));
        assert_eq!(s, ModeSwitch { end: open(|m| (m.transform, m.quickshape) = (true, true)), goes_on: false });
        let s =
            mode_switch(ModeButton::Transform, open(|m| (m.selection, m.recolor, m.adjustment) = (true, true, true)));
        assert_eq!(
            s,
            ModeSwitch { end: open(|m| (m.selection, m.recolor, m.adjustment) = (true, true, true)), goes_on: true }
        );
        assert!(!mode_switch(ModeButton::Selection, open(|m| m.selection = true)).goes_on);
        assert!(!mode_switch(ModeButton::Recolor, open(|m| m.recolor = true)).goes_on);
        // Layer Opacity starts again, a new adjustment ends the open one
        let s = mode_switch(ModeButton::LayerOpacity, open(|m| (m.layer_opacity, m.transform) = (true, true)));
        assert_eq!(s, ModeSwitch { end: open(|m| m.transform = true), goes_on: true });
        assert_eq!(mode_switch(ModeButton::Adjustment, ALL), ModeSwitch { end: ALL, goes_on: true });
    }

    #[test]
    fn the_adjustments_button_ends_layer_opacity_recolor_or_the_adjustment_else_opens_its_menu() {
        let s = mode_switch(ModeButton::Adjustments, open(|m| (m.recolor, m.filling) = (true, true)));
        assert_eq!(s, ModeSwitch { end: open(|m| (m.recolor, m.filling) = (true, true)), goes_on: false });
        let s = mode_switch(ModeButton::Adjustments, open(|m| m.adjustment = true));
        assert_eq!(s, ModeSwitch { end: open(|m| m.adjustment = true), goes_on: false });
        let s = mode_switch(ModeButton::Adjustments, open(|m| m.layer_opacity = true));
        assert_eq!(s, ModeSwitch { end: open(|m| m.layer_opacity = true), goes_on: false });
        let s = mode_switch(ModeButton::Adjustments, open(|m| (m.transform, m.quickshape) = (true, true)));
        assert_eq!(s, ModeSwitch { end: open(|m| (m.transform, m.quickshape) = (true, true)), goes_on: true });
    }

    #[test]
    fn the_interface_stays_while_transforming() {
        assert!(!mode_switch(ModeButton::Bars, open(|m| m.transform = true)).goes_on);
        assert_eq!(
            mode_switch(ModeButton::Bars, ALL.but(open(|m| m.transform = true))),
            ModeSwitch { end: NONE, goes_on: true }
        );
    }
}
