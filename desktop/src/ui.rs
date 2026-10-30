//! The interface over the canvas: the bars (see [`crate::chrome`]), the popovers hanging from
//! the top bar, and the keyboard's shortcuts.

use std::path::PathBuf;

use egui::{Color32, Key, KeyboardShortcut, Modifiers, Rect, Stroke, Ui, vec2};
use procraft_core::{Blocked, CanvasInfo, CanvasSession, ToolKind, TouchCommand};

use crate::actions::{Act, Actions};
use crate::alert::{Alert, Answer, Asking};
use crate::chrome::{BarButton, Lit, SideBar};
use crate::colour_drop::{ColourDrop, Touch};
use crate::colours::{Colours, srgb, to_hex};
use crate::files::Format;
use crate::input::Input;
use crate::layers::Layers;
use crate::library::Library;
use crate::look::{Look, argb, pc};
use crate::overlays::{Overlays, snapped_to};

const POPOVER_FILL: Color32 = Color32::from_rgb(0x28, 0x2a, 0x2d);

/// What the interface asks the app to do once the frame is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    New,
    Open,
    Save,
    SaveAs,
    Export(Format),
    InsertFile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Popover {
    Actions,
    Brushes,
    Layers,
    Colour,
}

pub struct Interface {
    pub actions: Vec<Action>,
    /// Where the canvas shows, clear of the bar (points).
    pub canvas_rect: Rect,
    /// A popover is open: a press on the canvas only closes it.
    pub popover_open: bool,
    /// The file the artwork was last saved to (or opened from, if it saves back there).
    pub path: Option<PathBuf>,
    /// The artwork's edits when it was last saved: anything else is unsaved.
    pub saved_edits: u64,
    open: Option<Popover>,
    colours: Colours,
    drop: ColourDrop,
    library: Library,
    overlays: Overlays,
    /// The interface is hidden (full screen).
    bars_hidden: bool,
    /// A layer copied or cut, to paste.
    clipboard: Option<procraft_core::ThumbnailImage>,
    actions_panel: Actions,
    layers: Layers,
    look: Look,
    side_bar: SideBar,
    styled: bool,
    /// The current layer turned a change away: why, and the alert saying so (offering to unlock or
    /// show it, or open Layers).
    blocked: Option<(Blocked, u64)>,
    blocked_alert: Alert,
    /// The brushes changed (the library closed, a slider let go, a notch saved): keep them.
    pub keep_brushes: bool,
    brushes_were_open: bool,
}

impl Default for Interface {
    fn default() -> Interface {
        Interface {
            actions: Vec::new(),
            canvas_rect: Rect::EVERYTHING,
            popover_open: false,
            path: None,
            saved_edits: 0,
            open: None,
            colours: Colours::default(),
            drop: ColourDrop::default(),
            library: Library::default(),
            overlays: Overlays::default(),
            bars_hidden: false,
            clipboard: None,
            actions_panel: Actions::default(),
            layers: Layers::default(),
            look: Look::default(),
            side_bar: SideBar::default(),
            styled: false,
            blocked: None,
            blocked_alert: Alert::default(),
            keep_brushes: false,
            brushes_were_open: false,
        }
    }
}

impl Interface {
    /// `session` is shown now (at `scale` window pixels a point): it paints in the colour chosen.
    pub fn opened(&mut self, session: &CanvasSession, scale: f32) {
        self.layers.forget();
        self.drop.end_filling();
        session.use_colour_history(self.colours.history.clone());
        self.colours.send(session);
        self.place_canvas(session, scale);
    }

    /// Fitting the canvas keeps it clear of the bar.
    pub fn place_canvas(&self, session: &CanvasSession, scale: f32) {
        session.set_fit_insets(0.0, pc::BAR_HEIGHT * scale, 0.0, 0.0);
    }

    pub fn show(&mut self, ui: &mut Ui, session: &CanvasSession, input: &mut Input) {
        let ctx = ui.ctx().clone();
        if !self.styled {
            style(&ctx);
            self.styled = true;
        }
        self.commands(&ctx, session, input);
        // the brush library closed: what was done in it is kept
        let brushes_open = self.open == Some(Popover::Brushes);
        self.keep_brushes |= self.brushes_were_open && !brushes_open;
        self.brushes_were_open = brushes_open;
        let info = session.info();
        self.shortcuts(&ctx, session, &info);
        let lit = Lit {
            actions: self.open == Some(Popover::Actions),
            layers: self.open == Some(Popover::Layers),
            panel_open: matches!(self.open, Some(Popover::Layers | Popover::Colour)),
            ..Lit::default()
        };
        let colour = srgb(self.colours.rgb());
        if self.bars_hidden {
            // full screen: the canvas alone (the overlays still show)
            self.canvas_rect = Rect::EVERYTHING;
            self.overlays.show(ui, &mut self.look, colour, false);
            self.drop.show(&ctx, session, to_hex(self.colours.rgb()), 0.0);
            self.blocked_alert(&ctx, session);
            session.set_backdrops(Vec::new());
            self.popover_open = false;
            return;
        }
        let bar = crate::chrome::top_bar(ui, &mut self.look, info.tool, colour, self.colours.hsv[2], &lit);
        match bar.pressed {
            Some(BarButton::Gallery) => self.actions.push(Action::Open),
            Some(BarButton::Actions) => self.toggle(Popover::Actions),
            Some(BarButton::Layers) => self.toggle(Popover::Layers),
            Some(b @ (BarButton::Paint | BarButton::Smudge | BarButton::Erase)) => {
                let tool = match b {
                    BarButton::Smudge => ToolKind::Smudge,
                    BarButton::Erase => ToolKind::Erase,
                    _ => ToolKind::Paint,
                };
                // the chosen tool again opens its brushes
                if info.tool == tool && self.open.is_none_or(|o| o == Popover::Brushes) {
                    self.toggle(Popover::Brushes);
                } else {
                    session.set_tool(tool);
                    self.library.tool_changed();
                    self.open = None;
                }
            }
            Some(BarButton::Adjustments | BarButton::Selection | BarButton::Transform | BarButton::Colour) | None => {}
        }
        // the colour button: tapped it opens the colours, held it goes back to the previous
        // colour, and dragged out its colour fills where it is let go (ColorDrop)
        if let Some(rect) = bar.rect(BarButton::Colour) {
            let id = ui.scope_id().with("colour button");
            let rgb = to_hex(self.colours.rgb());
            match self.drop.source(ui, session, id, rect, rgb, false, true, None) {
                Some(Touch::Tap) => self.toggle(Popover::Colour),
                Some(Touch::Hold) => {
                    if self.colours.previous(session) {
                        self.overlays.notice("Previous colour", false);
                    }
                }
                Some(Touch::Dragged) => self.open = None,
                None => {}
            }
        }
        self.canvas_rect = ui.available_rect_before_wrap();
        let mut backdrops = Vec::new();
        let taps = self.side_bar.show(ui, &mut self.look, session, &info, session.pick_next(), &mut backdrops);
        self.keep_brushes |= taps.brush_changed;
        if taps.undo {
            self.undo(session);
        }
        if taps.redo {
            self.redo(session);
        }
        // the modify button tapped opens or closes the eyedropper; held, touches pick meanwhile
        // (unless one did, its tap then does nothing)
        let used = session.set_modify_held(taps.modify_down);
        if taps.modify_tapped && !used {
            session.set_pick_next(!session.pick_next());
        }
        if let Some(kind) = self.overlays.show(ui, &mut self.look, colour, true) {
            self.overlays.shape = session.quickshape_choose(kind).or(self.overlays.shape.take());
        }
        // its bar tapped, the snapped shape's handles show and take drags
        session.set_shape_editing(self.overlays.shape_editing);
        let tool_button = match info.tool {
            ToolKind::Smudge => BarButton::Smudge,
            ToolKind::Erase => BarButton::Erase,
            ToolKind::Paint => BarButton::Paint,
        };
        let anchor = |p: Popover| match p {
            Popover::Actions => bar.rect(BarButton::Actions),
            Popover::Brushes => bar.rect(tool_button),
            Popover::Layers => bar.rect(BarButton::Layers),
            Popover::Colour => bar.rect(BarButton::Colour),
        };
        if let Some(open) = self.open {
            self.popover(&ctx, session, open, anchor(open), info.tool, &mut backdrops);
        }
        self.drop.show(&ctx, session, to_hex(self.colours.rgb()), pc::BAR_HEIGHT);
        self.blocked_alert(&ctx, session);
        session.set_backdrops(backdrops);
        if input.moving_view() && !ctx.is_pointer_over_egui() {
            ctx.set_cursor_icon(egui::CursorIcon::Grab);
        }
        self.popover_open = self.open.is_some();
    }

    /// Does what the touches asked.
    fn commands(&mut self, ctx: &egui::Context, session: &CanvasSession, input: &mut Input) {
        let mut slid = None;
        let mut stroke_ended = false;
        for command in std::mem::take(&mut input.commands) {
            stroke_ended |= command == TouchCommand::StrokeEnded;
            match command {
                TouchCommand::Undo => self.undo(session),
                TouchCommand::Redo => self.redo(session),
                TouchCommand::ClearLayer => self.clear_layer(session),
                TouchCommand::ToggleBars => self.toggle_bars(),
                TouchCommand::Fit => session.settle_to_fit(),
                TouchCommand::QuickShapeLast => {
                    if let Some(info) = session.quickshape_last() {
                        self.snapped(info);
                    }
                }
                TouchCommand::Pick { x, y } => self.overlays.pick(ctx, session, x, y),
                TouchCommand::PickDone => {
                    if let Some(rgb) = self.overlays.picking.take().and_then(|p| p.rgb) {
                        self.colours.set_rgb(session, rgb);
                    }
                }
                TouchCommand::PickCancel => self.overlays.picking = None,
                TouchCommand::ShapeSnapped => {
                    if let Some(info) = session.quickshape_info() {
                        self.snapped(info);
                    }
                }
                // a snapped shape the pen moved, dragged or snapped perfect: as it now is
                TouchCommand::ShapeChanged | TouchCommand::StrokeEnded => {
                    if self.overlays.shape.is_some() {
                        self.overlays.shape = session.quickshape_info().or(self.overlays.shape.take());
                    }
                }
                TouchCommand::ShapeEnded => {
                    self.overlays.shape = None;
                    self.overlays.shape_editing = false;
                }
                TouchCommand::Cursor { x, y } => self.overlays.cursor(ctx, session, Some((x, y))),
                TouchCommand::CursorGone => self.overlays.cursor(ctx, session, None),
                TouchCommand::FillThreshold { threshold } => {
                    self.drop.slid(threshold);
                    slid = Some(threshold);
                }
                TouchCommand::Filled => {
                    self.drop.filled();
                    slid = None;
                }
                // still to come on the desktop: copy and paste, the QuickMenu, layer select,
                // rasterizing a text layer
                TouchCommand::CopyPaste
                | TouchCommand::QuickMenu { .. }
                | TouchCommand::LayerSelect { .. }
                | TouchCommand::ViewMoved
                | TouchCommand::TextLayer => {}
            }
        }
        // a slid threshold redoes the whole fill: once a frame, at the last
        if let Some(threshold) = slid {
            session.pour_threshold(threshold);
        }
        // a stroke on a locked or hidden layer drew nothing: say why (a text layer takes none
        // until it is rasterized, which it asks about itself)
        let on_text = session.layers().iter().any(|l| l.current && l.text);
        if stroke_ended && !on_text {
            self.takes_changes(session);
        }
    }

    /// A stroke snapped to a shape: its bar shows, and the notice says what it is.
    fn snapped(&mut self, info: procraft_core::QuickShapeInfo) {
        self.overlays.notice(snapped_to(info.kind), false);
        self.overlays.shape = Some(info);
        self.overlays.shape_editing = false;
    }

    fn undo(&mut self, session: &CanvasSession) {
        let could = session.info().can_undo;
        let name = session.undo();
        if could {
            self.overlays.notice(name.map_or("Undo".into(), |n| format!("Undo {n}")), false);
        }
    }

    fn redo(&mut self, session: &CanvasSession) {
        let could = session.info().can_redo;
        let name = session.redo();
        if could {
            self.overlays.notice(name.map_or("Redo".into(), |n| format!("Redo {n}")), false);
        }
    }

    fn clear_layer(&mut self, session: &CanvasSession) {
        if self.takes_changes(session) {
            session.clear_layer();
            self.overlays.notice("Layer cleared", false);
        }
    }

    /// Whether the current layer takes a change; if not (locked, hidden, or an empty group), an
    /// alert says why. What was tried is not done afterwards.
    fn takes_changes(&mut self, session: &CanvasSession) -> bool {
        let Some(why) = session.why_blocked() else { return true };
        let Some(l) = session.layers().into_iter().find(|l| l.current) else { return true };
        let (title, message, icon, extra) = match why {
            Blocked::Locked => (
                "Layer is locked",
                format!("\u{201c}{}\u{201d} can't be changed while it is locked.", l.name),
                &crate::icons::PAGE_LOCKED,
                Some("Unlock"),
            ),
            Blocked::Hidden => (
                "Layer is hidden",
                format!("\u{201c}{}\u{201d} can't be changed while it is hidden.", l.name),
                &crate::icons::EYE_STRUCK,
                Some("Show"),
            ),
            Blocked::EmptyGroup => (
                "Group is empty",
                format!("\u{201c}{}\u{201d} holds no layers to change.", l.name),
                &crate::icons::FOLDER,
                None,
            ),
        };
        self.blocked = Some((why, l.id));
        self.blocked_alert.ask(Asking {
            title: title.into(),
            message,
            action: "Open Layers".into(),
            icon: Some(icon),
            destructive: false,
            extra: extra.map(Into::into),
        });
        false
    }

    /// The blocked alert, while it shows: unlock or show the layer, or open Layers.
    fn blocked_alert(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        let Some(answer) = self.blocked_alert.show(ctx, &mut self.look) else { return };
        let Some((why, id)) = self.blocked.take() else { return };
        match answer {
            Answer::Extra if why == Blocked::Locked => _ = session.set_layer_locked(id, false),
            Answer::Extra if why == Blocked::Hidden => _ = session.set_layer_visible(id, true),
            Answer::Action => self.open = Some(Popover::Layers),
            _ => {}
        }
    }

    /// The interface hides (full screen) or comes back; hiding, it closes its panels.
    fn toggle_bars(&mut self) {
        self.bars_hidden = !self.bars_hidden;
        if self.bars_hidden {
            self.open = None;
        }
    }

    fn toggle(&mut self, popover: Popover) {
        self.open = if self.open == Some(popover) { None } else { Some(popover) };
    }

    /// The open panel, hanging from its button; a press anywhere else, or Escape, closes it.
    fn popover(
        &mut self,
        ctx: &egui::Context,
        session: &CanvasSession,
        which: Popover,
        button: Option<Rect>,
        tool: ToolKind,
        backdrops: &mut Vec<f32>,
    ) {
        let Some(button) = button else {
            self.open = None;
            return;
        };
        // while one of the panel's menus is open, a press elsewhere only closes the menu
        let modal = which == Popover::Layers && self.layers.modal();
        let shown = match which {
            Popover::Brushes => self.library.show(ctx, &mut self.look, session, tool, button, backdrops),
            Popover::Layers => {
                let shown = self.layers.show(ctx, &mut self.look, session, button, backdrops);
                if let Some((text, warning)) = self.layers.notice.take() {
                    self.overlays.notice(text, warning);
                }
                self.clipboard = self.layers.copied.take().or(self.clipboard.take());
                if std::mem::take(&mut self.layers.share) {
                    self.actions.push(Action::Export(Format::ChosenLayerPngs));
                }
                shown
            }
            Popover::Colour => self.colours.show(ctx, &mut self.look, session, &mut self.drop, button, backdrops),
            Popover::Actions => {
                let (shown, act) = self.actions_panel.show(ctx, &mut self.look, button, backdrops);
                match act {
                    Some(Act::Flip { horizontal }) => session.flip_canvas(horizontal),
                    Some(Act::InsertFile) => {
                        self.actions.push(Action::InsertFile);
                        self.open = None;
                    }
                    Some(Act::Share(format)) => {
                        self.actions.push(Action::Export(format));
                        self.open = None;
                    }
                    Some(Act::About) => crate::files::about(),
                    None => {}
                }
                shown
            }
        };
        if !modal {
            self.close_outside(ctx, shown, button);
        }
    }

    /// A press anywhere but on the popover at `shown` and its `button`, or Escape, closes it.
    fn close_outside(&mut self, ctx: &egui::Context, shown: Rect, button: Rect) {
        let pressed_outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.interact_pos().is_some_and(|p| !shown.contains(p) && !button.contains(p))
        });
        if pressed_outside || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.open = None;
        }
    }

    /// A hardware keyboard's shortcuts, as the Android app's: Ctrl (⌘) with Z undoes (with Shift
    /// redoes), with C, X and V copies, cuts and pastes the layer; B paints, E erases, M opens the
    /// eyedropper, C the colours, X swaps the colours, J duplicates the layer, K opens Actions, L
    /// Layers, 0 hides the interface, Delete clears the layer, and [ and ] change the brush's size
    /// by 1% (Shift 5%, Alt 10%). The desktop adds its files: Ctrl (⌘) with N, O, S (and Shift).
    fn shortcuts(&mut self, ctx: &egui::Context, session: &CanvasSession, info: &CanvasInfo) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let pressed = |modifiers: Modifiers, key: Key| {
            ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key)))
        };
        let command_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        // with Shift first: the shortcut without it would take it too
        if pressed(command_shift, Key::Z) {
            self.redo(session);
        } else if pressed(Modifiers::COMMAND, Key::Z) {
            self.undo(session);
        }
        if pressed(Modifiers::COMMAND, Key::C) {
            self.clipboard = session.copy_layer().or(self.clipboard.take());
        }
        if pressed(Modifiers::COMMAND, Key::X) {
            self.clipboard = session.cut_layer().or(self.clipboard.take());
        }
        if pressed(Modifiers::COMMAND, Key::V)
            && let Some(image) = self.clipboard.clone()
        {
            session.paste(image);
        }
        if pressed(command_shift, Key::S) {
            self.actions.push(Action::SaveAs);
        } else if pressed(Modifiers::COMMAND, Key::S) {
            self.actions.push(Action::Save);
        }
        if pressed(Modifiers::COMMAND, Key::N) {
            self.actions.push(Action::New);
        }
        if pressed(Modifiers::COMMAND, Key::O) {
            self.actions.push(Action::Open);
        }
        let plain = |key: Key| pressed(Modifiers::NONE, key);
        if plain(Key::B) {
            self.choose_tool(session, ToolKind::Paint);
        }
        if plain(Key::E) {
            self.choose_tool(session, ToolKind::Erase);
        }
        if plain(Key::M) {
            session.set_pick_next(!session.pick_next());
        }
        if plain(Key::C) {
            self.toggle(Popover::Colour);
        }
        if plain(Key::X) {
            self.colours.swap(session);
        }
        if plain(Key::J)
            && let Some(current) = session.layers().into_iter().find(|l| l.current)
        {
            session.duplicate_layer(current.id);
        }
        if plain(Key::K) {
            self.toggle(Popover::Actions);
        }
        if plain(Key::L) {
            self.toggle(Popover::Layers);
        }
        if plain(Key::Num0) {
            self.toggle_bars();
        }
        if plain(Key::Backspace) || plain(Key::Delete) {
            self.clear_layer(session);
        }
        // the brush a step smaller or bigger: 1%, with Shift 5%, with Alt 10%
        let step = ctx.input(|i| {
            if i.modifiers.alt {
                0.10
            } else if i.modifiers.shift {
                0.05
            } else {
                0.01
            }
        });
        let bracket = |open: bool| {
            ctx.input_mut(|i| {
                let keys = if open {
                    [Key::OpenBracket, Key::OpenCurlyBracket]
                } else {
                    [Key::CloseBracket, Key::CloseCurlyBracket]
                };
                keys.iter().any(|k| i.consume_key(i.modifiers, *k))
            })
        };
        if bracket(true) {
            session.set_brush_size((info.size - step).max(0.0));
        }
        if bracket(false) {
            session.set_brush_size((info.size + step).min(1.0));
        }
    }

    /// A tool chosen (as its button does when it isn't the current one).
    fn choose_tool(&mut self, session: &CanvasSession, tool: ToolKind) {
        session.set_tool(tool);
        self.library.tool_changed();
    }
}

fn style(ctx: &egui::Context) {
    crate::look::install_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.visuals.selection.bg_fill = argb(pc::ACCENT);
        s.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        s.visuals.hyperlink_color = argb(pc::ACCENT);
        s.visuals.window_fill = POPOVER_FILL;
        s.visuals.panel_fill = crate::look::argb(pc::BAR);
        s.visuals.slider_trailing_fill = true;
        s.spacing.button_padding = vec2(10.0, 5.0);
        s.spacing.item_spacing = vec2(8.0, 6.0);
    });
}
