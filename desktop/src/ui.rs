//! The interface over the canvas: the bars (see [`crate::chrome`]), the popovers hanging from
//! the top bar, and the keyboard's shortcuts.

use std::path::PathBuf;

use egui::{Color32, Key, KeyboardShortcut, Modifiers, Rect, Stroke, Ui, vec2};
use procraft_core::{Blocked, CanvasInfo, CanvasSession, GesturePurpose, QuickAction, ToolKind, TouchCommand};

use crate::actions::{Act, Actions};
use crate::alert::{Alert, Answer, Asking};
use crate::chrome::{BarButton, Lit, SideBar};
use crate::colour_drop::{ColourDrop, Touch};
use crate::colours::{Colours, srgb, to_hex};
use crate::copy_paste::Choice;
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
    /// Back to the gallery, the artwork kept.
    Gallery,
    New,
    Open,
    Save,
    SaveAs,
    Export(Format),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Popover {
    Actions,
    Adjustments,
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
    pub(crate) colours: Colours,
    drop: ColourDrop,
    library: Library,
    overlays: Overlays,
    /// The interface is hidden (full screen).
    bars_hidden: bool,
    /// A layer copied or cut, to paste.
    clipboard: Option<procraft_core::ThumbnailImage>,
    /// The Copy & Paste panel is up.
    copy_paste: bool,
    /// Layer Select's list: where (points) and the layers drawn there, top first.
    layer_choice: Option<(egui::Pos2, Vec<u64>)>,
    actions_panel: Actions,
    layers: Layers,
    pub look: Look,
    side_bar: SideBar,
    /// Transform: its box over the canvas and its toolbar, while open.
    transform: crate::transform::Transform,
    /// Selection: its toolbar and what shows while selecting, while open.
    selection: crate::selection::Selection,
    /// The gesture settings (kept between runs), and their page while it is open.
    gestures: std::sync::Arc<procraft_core::GestureSettings>,
    gesture_controls: crate::gesture_controls::GestureControls,
    /// Canvas information, while it is open.
    canvas_info: Option<crate::canvas_info::CanvasInfo>,
    /// The reference window.
    reference: crate::reference::Reference,
    /// Crop and Resize.
    crop: crate::crop::Crop,
    /// The drawing guide's editor.
    guide_editor: crate::guide_editor::GuideEditor,
    /// The artwork's folder in the gallery.
    pub artwork_dir: Option<String>,
    quick_menu: crate::quick_menu::QuickMenu,
    /// An open adjustment's bar, panel and tap menu; Gradient Map's library (kept between runs).
    adjustments: crate::adjustments::Adjustments,
    gradients: std::sync::Arc<procraft_core::Gradients>,
    styled: bool,
    /// The current layer turned a change away: why, and the alert saying so (offering to unlock or
    /// show it, or open Layers).
    blocked: Option<(Blocked, u64)>,
    blocked_alert: Alert,
    /// The brushes changed (the library closed, a slider let go, a notch saved): keep them.
    pub keep_brushes: bool,
    brushes_were_open: bool,
    /// The Prefs tab's settings (kept between runs), whether the side bar shows, and the licences
    /// while they show.
    prefs: crate::prefs::Prefs,
    side_bar_shown: bool,
    licences: bool,
    /// Animation Assist's bar, while it is on, and sharing the layers as an animation.
    animation: crate::animation::AnimationBar,
    animated_export: Option<crate::animated_export::AnimatedExport>,
    /// The artwork's time-lapse: recorded, replayed, made into a video.
    time_lapse: crate::timelapse::TimeLapseUi,
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
            copy_paste: false,
            layer_choice: None,
            actions_panel: Actions::default(),
            layers: Layers::default(),
            look: Look::default(),
            side_bar: SideBar::default(),
            transform: crate::transform::Transform::default(),
            selection: crate::selection::Selection::default(),
            gestures: procraft_core::GestureSettings::new(crate::files::kept_gestures()),
            gesture_controls: crate::gesture_controls::GestureControls::default(),
            canvas_info: None,
            reference: crate::reference::Reference::default(),
            crop: crate::crop::Crop::default(),
            guide_editor: crate::guide_editor::GuideEditor::default(),
            artwork_dir: None,
            quick_menu: crate::quick_menu::QuickMenu::default(),
            adjustments: crate::adjustments::Adjustments::default(),
            gradients: procraft_core::Gradients::open(
                crate::files::app_dir().join("gradients.json").to_string_lossy().into_owned(),
            ),
            styled: false,
            blocked: None,
            blocked_alert: Alert::default(),
            keep_brushes: false,
            brushes_were_open: false,
            prefs: crate::prefs::Prefs::kept(),
            side_bar_shown: true,
            licences: false,
            animation: crate::animation::AnimationBar::default(),
            animated_export: None,
            time_lapse: crate::timelapse::TimeLapseUi::default(),
        }
    }
}

impl Interface {
    /// `session` is shown now (at `scale` window pixels a point): it paints in the colour chosen.
    pub fn opened(&mut self, session: &CanvasSession, scale: f32) {
        self.layers.forget();
        self.transform.on = false;
        self.selection.on = false;
        self.drop.end_filling();
        session.use_colour_history(self.colours.history.clone());
        session.set_touch_settings(self.gestures.touch_settings(self.prefs.rapid_undo_ms()));
        session.use_gradients(self.gradients.clone());
        self.prefs.apply(session);
        self.time_lapse.open(self.artwork_dir.as_deref(), session.edits());
        self.adjustments.menu_at = None;
        self.colours.send(session);
        self.place_canvas(session, scale);
    }

    /// Fitting the canvas keeps it clear of the bar.
    pub fn place_canvas(&self, session: &CanvasSession, scale: f32) {
        session.set_fit_insets(0.0, pc::BAR_HEIGHT * scale, 0.0, 0.0);
    }

    /// The app's fonts and look, set the first time anything shows.
    pub fn style_once(&mut self, ctx: &egui::Context) {
        if !self.styled {
            style(ctx);
            self.styled = true;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, session: &std::sync::Arc<CanvasSession>, input: &mut Input) {
        let ctx = ui.ctx().clone();
        self.style_once(&ctx);
        self.commands(&ctx, session, input);
        // the brush library closed: what was done in it is kept
        let brushes_open = self.open == Some(Popover::Brushes);
        self.keep_brushes |= self.brushes_were_open && !brushes_open;
        self.brushes_were_open = brushes_open;
        let info = session.info();
        self.shortcuts(&ctx, session, &info);
        self.overlays.cursor_style = self.prefs.cursor_style;
        input.hover_shown = false;
        let lit = Lit {
            actions: self.open == Some(Popover::Actions),
            transform: self.transform.on,
            selection: self.selection.on,
            adjustments: self.open == Some(Popover::Adjustments) || session.adjustment_shown().adjustment.is_some(),
            layers: self.open == Some(Popover::Layers),
            panel_open: matches!(self.open, Some(Popover::Layers | Popover::Colour)),
        };
        let colour = srgb(self.colours.rgb());
        self.time_lapse.record(&ctx, session, input.idle());
        // the time-lapse replayed over everything
        if self.time_lapse.replaying() {
            self.canvas_rect = Rect::NOTHING;
            self.animation.reset(session);
            if let Some(said) = self.time_lapse.show(&ctx, &mut self.look, &session.name()) {
                crate::files::tell("Time-lapse", said);
            }
            session.set_backdrops(Vec::new());
            self.popover_open = false;
            return;
        }
        // sharing as an animation: its screen over everything
        if let Some(export) = self.animated_export.as_mut() {
            self.canvas_rect = Rect::NOTHING;
            self.animation.reset(session);
            match export.show(&ctx, &mut self.look, session) {
                Some(crate::animated_export::Ended::Closed) => self.animated_export = None,
                Some(crate::animated_export::Ended::Failed) => {
                    self.animated_export = None;
                    crate::files::tell("Share", "The animation could not be exported.");
                }
                None => {}
            }
            session.set_backdrops(Vec::new());
            self.popover_open = false;
            return;
        }
        // Crop and Resize: the canvas, its frame and header alone
        if self.crop.open {
            self.canvas_rect = Rect::EVERYTHING;
            self.crop.show(&ctx, &mut self.look, session);
            session.set_backdrops(Vec::new());
            self.popover_open = false;
            return;
        }
        // the drawing guide's editor: the canvas, the guide and the editor alone
        if self.guide_editor.open {
            self.canvas_rect = Rect::EVERYTHING;
            guide_lines(&ctx, session);
            self.guide_editor.show(&ctx, &mut self.look, session);
            session.set_backdrops(Vec::new());
            self.popover_open = false;
            return;
        }
        if self.bars_hidden {
            // full screen: the canvas alone (the overlays still show)
            self.canvas_rect = Rect::EVERYTHING;
            self.hover_cursor(&ctx, session, &info, input);
            self.overlays.show(ui, &mut self.look, colour, false);
            self.drop.show(&ctx, session, to_hex(self.colours.rgb()), 0.0);
            self.copy_paste_panel(&ctx, session);
            self.layer_choice(&ctx, session);
            self.quick_menu(&ctx, session);
            self.blocked_alert(&ctx, session);
            session.set_backdrops(Vec::new());
            self.popover_open = false;
            return;
        }
        let bar = crate::chrome::top_bar(ui, &mut self.look, info.tool, colour, self.colours.hsv[2], &lit);
        match bar.pressed {
            Some(BarButton::Gallery) => self.actions.push(Action::Gallery),
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
            Some(BarButton::Transform) => self.toggle_transform(session),
            Some(BarButton::Selection) => self.toggle_selection(session),
            Some(BarButton::Adjustments) => self.adjustments_pressed(session),
            Some(BarButton::Colour) | None => {}
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
        // a mode taking the screen puts the side bar away
        let adjusting = session.adjustment_shown();
        let side_bar = self.side_bar_shown
            && !self.transform.on
            && !self.selection.on
            && (adjusting.adjustment.is_none() || adjusting.pencil);
        crate::panel::side_bar_at(&ctx, self.side_bar_shown, self.prefs.right_handed);
        let taps = if side_bar {
            let modify_lit = session.pick_next();
            let right = self.prefs.right_handed;
            self.side_bar.show(ui, &mut self.look, session, &info, modify_lit, right, &mut backdrops)
        } else {
            crate::chrome::SideTaps::default()
        };
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
        self.hover_cursor(&ctx, session, &info, input);
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
            Popover::Adjustments => bar.rect(BarButton::Adjustments),
            Popover::Brushes => bar.rect(tool_button),
            Popover::Layers => bar.rect(BarButton::Layers),
            Popover::Colour => bar.rect(BarButton::Colour),
        };
        if let Some(open) = self.open {
            self.popover(&ctx, session, open, anchor(open), info.tool, &mut backdrops);
        }
        if self.open != Some(Popover::Actions) {
            self.actions_panel.closed();
        }
        self.transform.show(&ctx, &mut self.look, session, &mut backdrops);
        self.selection.show(&ctx, &mut self.look, session, pc::BAR_HEIGHT);
        if let Some(asked) = self.adjustments.show(
            &ctx,
            &mut self.look,
            session,
            pc::BAR_HEIGHT,
            &self.gradients,
            &self.colours,
            &mut backdrops,
        ) {
            self.adjustment_asked(session, asked);
        }
        guide_lines(&ctx, session);
        self.reference.show(&ctx, &mut self.look, session);
        // a full-screen page covers the timeline, as it covers the canvas
        if self.canvas_info.is_none() && !self.gesture_controls.open {
            let said = self.animation.show(&ctx, &mut self.look, session, &self.colours.palettes, self.colours.harmony);
            if let Some(crate::animation::Said::NoRoom) = said {
                crate::files::no_room(session);
            }
        } else {
            self.animation.reset(session);
        }
        self.drop.show(&ctx, session, to_hex(self.colours.rgb()), pc::BAR_HEIGHT);
        self.copy_paste_panel(&ctx, session);
        self.layer_choice(&ctx, session);
        self.quick_menu(&ctx, session);
        self.blocked_alert(&ctx, session);
        // a gesture setting changed: the touches follow it, and it is kept
        if self.gesture_controls.show(&ctx, &mut self.look, &self.gestures) {
            session.set_touch_settings(self.gestures.touch_settings(self.prefs.rapid_undo_ms()));
            crate::files::keep_gestures(&self.gestures.kept());
        }
        if let Some(info) = self.canvas_info.as_mut() {
            use crate::canvas_info::Ended;
            match info.show(&ctx, &mut self.look, session) {
                // another profile: the canvas and the current colour turned into it
                Some(Ended::Done(Some(profile))) => {
                    let (from, rgb) = (session.profile(), to_hex(self.colours.rgb()));
                    if session.change_profile(profile.clone()) {
                        self.colours.set_rgb(session, crate::canvas_info::colour_in(rgb, &from, &profile));
                    }
                    self.canvas_info = None;
                }
                Some(_) => self.canvas_info = None,
                None => {}
            }
        }
        if let Some(said) = self.time_lapse.show(&ctx, &mut self.look, &session.name()) {
            crate::files::tell("Time-lapse", said);
        }
        if self.licences && crate::licences::show(&ctx) {
            self.licences = false;
        }
        session.set_backdrops(backdrops);
        if input.moving_view() && !ctx.is_pointer_over_egui() {
            ctx.set_cursor_icon(egui::CursorIcon::Grab);
        }
        self.popover_open = self.open.is_some();
    }

    /// The brush's outline where the mouse hovers over the canvas, as the pen's hover shows it,
    /// while nothing is pressed (and while it would paint there).
    fn hover_cursor(&mut self, ctx: &egui::Context, session: &CanvasSession, info: &CanvasInfo, input: &mut Input) {
        if !input.idle() {
            return;
        }
        let painting = session.adjustment_shown().adjustment.is_none() && !self.selection.on && !self.transform.on;
        input.hover_shown = painting && self.prefs.cursor_shown(true);
        let ppp = ctx.pixels_per_point();
        let at = input.hovering().filter(|_| input.hover_shown).map(|(x, y)| egui::pos2(x, y) / ppp).filter(|at| {
            self.canvas_rect.contains(*at) && ctx.layer_id_at(*at).is_none_or(|l| l.order == egui::Order::Background)
        });
        self.overlays.hover(session, info, ppp, at);
    }

    /// Does what the touches asked.
    fn commands(&mut self, ctx: &egui::Context, session: &CanvasSession, input: &mut Input) {
        let mut slid = None;
        let mut sliding = None;
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
                TouchCommand::Cursor { x, y } if self.prefs.cursor_shown(false) => {
                    self.overlays.cursor(ctx, session, Some((x, y)));
                }
                TouchCommand::Cursor { .. } => {}
                TouchCommand::CursorGone => self.overlays.cursor(ctx, session, None),
                TouchCommand::FillThreshold { threshold } => {
                    self.drop.slid(threshold);
                    slid = Some(threshold);
                }
                TouchCommand::Filled => {
                    self.drop.filled();
                    slid = None;
                }
                TouchCommand::TransformTouched => self.transform.touched(),
                TouchCommand::TransformMoved { transform } => self.transform.moved(transform),
                TouchCommand::TransformHeld { handle, x, y } => self.transform.held(handle, egui::pos2(x, y)),
                TouchCommand::TransformLetGo { handle, x, y } => self.transform.let_go(handle, egui::pos2(x, y)),
                TouchCommand::SelectionDrag { x0, y0, x1, y1 } => {
                    self.selection.dragged(session, egui::pos2(x0, y0), egui::pos2(x1, y1));
                }
                TouchCommand::SelectionThreshold { x, y, threshold, done } => {
                    self.selection.slid(session, egui::pos2(x, y), threshold, done);
                    sliding = (!done).then_some((x, y, threshold));
                }
                TouchCommand::SelectionChanged => self.selection.changed(session),
                // the freehand path is read as it is drawn, each frame
                TouchCommand::FreehandChanged => {}
                // an adjustment's amount is read as it is drawn, each frame; a touch on the canvas
                // closes Layer / Pencil
                TouchCommand::AdjustChanged => self.adjustments.modes_open = false,
                TouchCommand::AdjustMenu { x, y } => {
                    self.adjustments.modes_open = false;
                    self.adjustments.menu_at = Some(egui::pos2(x, y) / ctx.pixels_per_point());
                }
                TouchCommand::TransformNumeric { angle, x, y, right } => {
                    self.transform.numbers(angle, egui::pos2(x, y) / ctx.pixels_per_point(), right);
                }
                // still to come on the desktop: copy and paste, the QuickMenu, layer select,
                // rasterizing a text layer
                TouchCommand::CopyPaste => self.copy_paste = true,
                TouchCommand::LayerSelect { x, y } => self.layer_select(ctx, session, x, y),
                TouchCommand::QuickMenu { x, y } => self.quick_menu.toggle(egui::pos2(x, y) / ctx.pixels_per_point()),
                TouchCommand::ViewMoved | TouchCommand::TextLayer => {}
            }
        }
        // a slid threshold redoes the whole fill: once a frame, at the last
        if let Some(threshold) = slid {
            session.pour_threshold(threshold);
        }
        // and so does a colour selection's
        if let Some((x, y, threshold)) = sliding {
            session.show_colour_selection(x, y, threshold);
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

    /// Layer Select at window point (`x`, `y`): the top layer drawn there becomes current, and
    /// the others drawn there are listed to choose from.
    fn layer_select(&mut self, ctx: &egui::Context, session: &CanvasSession, x: f32, y: f32) {
        let ids = session.layers_at(x, y);
        let Some(&top) = ids.first() else {
            crate::files::say("Nothing is drawn there.");
            return;
        };
        session.select_layer(top);
        self.layer_choice = (ids.len() > 1).then(|| (egui::pos2(x, y) / ctx.pixels_per_point(), ids));
    }

    /// The QuickMenu, while it shows: what a button does is done.
    fn quick_menu(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        if let Some((action, at)) = self.quick_menu.show(ctx, &mut self.look) {
            self.run_quick(ctx, session, action, at);
        }
    }

    /// A QuickMenu button's action, the menu opened at `at` (points). What the desktop doesn't
    /// have yet (adjustments, guides, animation, reference, text) does nothing.
    fn run_quick(&mut self, ctx: &egui::Context, session: &CanvasSession, action: QuickAction, at: egui::Pos2) {
        let current = session.layers().into_iter().find(|l| l.current);
        let id = current.as_ref().map(|l| l.id);
        match action {
            QuickAction::NewLayer => {
                if session.add_layer() == 0 {
                    crate::files::no_room(session);
                }
            }
            QuickAction::DuplicateLayer => self.duplicate_current(session),
            QuickAction::DeleteLayer => _ = id.map(|id| session.delete_layer(id)),
            QuickAction::MergeDown => _ = id.map(|id| session.merge_down(id)),
            QuickAction::ClearLayer => self.clear_layer(session),
            QuickAction::FillLayer => _ = id.map(|id| session.fill_layer(id)),
            QuickAction::SelectLayer => _ = id.map(|id| session.select_layer_contents(id)),
            QuickAction::AlphaLock => _ = current.map(|l| session.set_layer_alpha_lock(l.id, !l.alpha_lock)),
            QuickAction::LayerVisibility => _ = current.map(|l| session.set_layer_visible(l.id, !l.visible)),
            QuickAction::Solo => {
                if let Some(id) = id {
                    let on = session.solo() != Some(id);
                    if session.set_solo(on.then_some(id)) {
                        self.overlays.notice(if on { "Solo visibility on" } else { "Solo visibility off" }, false);
                    }
                }
            }
            QuickAction::Copy => self.copy_layer(session),
            QuickAction::CopyCanvas => self.copy_canvas(session),
            QuickAction::Cut => self.cut_layer(session),
            QuickAction::Paste => self.paste(session),
            QuickAction::CopyPasteMenu => self.copy_paste = true,
            QuickAction::FlipHorizontal => session.flip_canvas(true),
            QuickAction::FlipVertical => session.flip_canvas(false),
            QuickAction::Undo => self.undo(session),
            QuickAction::Redo => self.redo(session),
            QuickAction::Paint | QuickAction::Smudge | QuickAction::Erase => {
                session.set_tool(match action {
                    QuickAction::Smudge => ToolKind::Smudge,
                    QuickAction::Erase => ToolKind::Erase,
                    _ => ToolKind::Paint,
                });
                self.library.tool_changed();
            }
            QuickAction::Eyedropper => session.set_pick_next(true),
            QuickAction::SwapColors => _ = self.colours.previous(session),
            QuickAction::Selection => self.toggle_selection(session),
            QuickAction::Transform => self.toggle_transform(session),
            QuickAction::Colors => self.toggle(Popover::Colour),
            QuickAction::Layers => self.toggle(Popover::Layers),
            QuickAction::Brushes => self.toggle(Popover::Brushes),
            QuickAction::Interface => self.toggle_bars(),
            QuickAction::FitCanvas => session.settle_to_fit(),
            QuickAction::ActualSize => session.settle_to_actual_size(),
            QuickAction::Quickshape => {
                if let Some(info) = session.quickshape_last() {
                    self.snapped(info);
                }
            }
            QuickAction::QuickshapeOn => {
                let on = self.gestures.is_on(GesturePurpose::Quickshape, "DRAW_HOLD".into());
                self.gestures.set(GesturePurpose::Quickshape, "DRAW_HOLD".into(), !on);
                session.set_touch_settings(self.gestures.touch_settings(self.prefs.rapid_undo_ms()));
                crate::files::keep_gestures(&self.gestures.kept());
            }
            QuickAction::LayerSelect => {
                let ppp = ctx.pixels_per_point();
                self.layer_select(ctx, session, at.x * ppp, at.y * ppp);
            }
            _ => {}
        }
    }

    /// Layer Select's list, while it is up.
    fn layer_choice(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        let Some((at, ids)) = &self.layer_choice else { return };
        let Some(chose) = crate::layer_choice::show(ctx, *at, ids, &session.layers()) else { return };
        if let crate::layer_choice::Chose::Layer(id) = chose {
            session.select_layer(id);
        }
        self.layer_choice = None;
    }

    /// Transform on (the current layer, or what is selected of it, in a box) or off (kept).
    /// A picture the user picks put on a new layer of its own, then transformed (a moving GIF's
    /// frames each on a layer of its own, for the timeline), as Insert a file and Insert a photo
    /// do.
    fn insert_picture(&mut self, session: &CanvasSession, photo: bool) {
        let title = if photo { "Insert a photo" } else { "Insert a file" };
        let Some(path) = rfd::FileDialog::new()
            .set_title(title)
            .add_filter("Pictures", &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff"])
            .pick_file()
        else {
            return;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            crate::files::tell("Can't insert it", "This picture can't be opened.");
            return;
        };
        if let Some(done) = session.insert_gif_frames(bytes.clone(), false) {
            if done.added == 0 && !done.full {
                crate::files::tell("Can't insert it", "This picture can't be opened.");
                return;
            }
            if done.added > 0 {
                self.overlays.notice("Frames saved to timeline", false);
            }
            if done.full {
                crate::files::no_room(session);
            }
            return;
        }
        match image::load_from_memory(&bytes) {
            Ok(picture) => {
                let rgba = picture.to_rgba8();
                let image =
                    procraft_core::ThumbnailImage { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() };
                if session.insert_image(image, false).is_some() {
                    self.toggle_transform(session);
                } else {
                    crate::files::no_room(session);
                }
            }
            Err(_) => crate::files::tell("Can't insert it", "This picture can't be opened."),
        }
    }

    fn toggle_transform(&mut self, session: &CanvasSession) {
        self.drop.end_filling();
        if self.transform.on {
            self.transform.end(session, true);
            return;
        }
        if !self.takes_changes(session) {
            return;
        }
        self.selection.close(session);
        self.end_adjustment(session, true);
        self.open = None;
        session.set_pick_next(false);
        // nothing there to move: it says so and stays out of transform
        if let Err(said) = self.transform.begin(session) {
            self.overlays.notice(said, false);
        }
    }

    /// Opens adjustment `a` on the current layer (anything else open ends first).
    fn start_adjustment(&mut self, session: &CanvasSession, a: procraft_core::Adjustment) {
        self.drop.end_filling();
        if !self.takes_changes(session) {
            return;
        }
        self.transform.end(session, true);
        self.selection.close(session);
        self.open = None;
        session.set_pick_next(false);
        session.adjustment_start(a);
    }

    /// Ends the open adjustment: `commit` keeps it (and says so), otherwise the layer goes back.
    fn end_adjustment(&mut self, session: &CanvasSession, commit: bool) {
        self.adjustments.menu_at = None;
        if session.adjustment_end(commit) && commit {
            self.overlays.notice("Adjustment applied", false);
        }
    }

    /// The adjustments button: closes an open adjustment (keeping it), or opens the menu.
    fn adjustments_pressed(&mut self, session: &CanvasSession) {
        self.drop.end_filling();
        if session.adjustment_shown().adjustment.is_some() {
            self.end_adjustment(session, true);
        } else {
            self.toggle(Popover::Adjustments);
        }
    }

    /// What the tap menu asked of the open adjustment.
    fn adjustment_asked(&mut self, session: &CanvasSession, asked: crate::adjustments::Asked) {
        use crate::adjustments::Asked;
        match asked {
            Asked::Apply => {
                session.adjustment_apply();
                self.overlays.notice("Adjustment applied", false);
            }
            Asked::Undo => session.adjustment_undo(),
            Asked::Preview => session.adjustment_preview(!session.adjustment_shown().preview),
            Asked::Cancel => self.end_adjustment(session, false),
            Asked::Reset => session.adjustment_reset(),
        }
    }

    /// Selection on (touches select, as its toolbar says) or off (a freehand path drawn is closed).
    fn toggle_selection(&mut self, session: &CanvasSession) {
        self.drop.end_filling();
        if self.selection.on {
            self.selection.close(session);
            return;
        }
        if self.transform.on {
            self.transform.end(session, true);
        }
        self.end_adjustment(session, true);
        self.open = None;
        session.set_pick_next(false);
        self.selection.open(session);
    }

    /// The Copy & Paste panel, while it is up: what was chosen there is done, and it goes.
    fn copy_paste_panel(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        if !self.copy_paste {
            return;
        }
        let Some(choice) = crate::copy_paste::show(ctx, &mut self.look, self.clipboard.is_some()) else { return };
        self.copy_paste = false;
        match choice {
            Choice::Close => {}
            Choice::Cut => self.cut_layer(session),
            Choice::Copy => self.copy_layer(session),
            Choice::CopyAll => self.copy_canvas(session),
            Choice::Duplicate => self.duplicate_current(session),
            Choice::CutAndPaste => {
                self.cut_layer(session);
                self.paste(session);
            }
            Choice::Paste => self.paste(session),
        }
    }

    /// The current layer (or what is selected of it) cut, to paste.
    fn cut_layer(&mut self, session: &CanvasSession) {
        self.open = None;
        if !self.takes_changes(session) {
            return;
        }
        if let Some(cut) = session.cut_layer() {
            self.clipboard = Some(cut);
            self.overlays.notice("Layer cut", false);
        }
    }

    /// The current layer (or what is selected of it) copied, to paste.
    fn copy_layer(&mut self, session: &CanvasSession) {
        self.open = None;
        if let Some(copied) = session.copy_layer() {
            self.clipboard = Some(copied);
            self.overlays.notice("Layer copied", false);
        }
    }

    /// The whole picture (or what is selected of it) copied, to paste.
    fn copy_canvas(&mut self, session: &CanvasSession) {
        self.open = None;
        if let Some(copied) = session.copy_canvas() {
            self.clipboard = Some(copied);
            self.overlays.notice("Canvas copied", false);
        }
    }

    /// What was copied or cut, pasted as a new layer.
    fn paste(&mut self, session: &CanvasSession) {
        self.open = None;
        let Some(image) = self.clipboard.clone() else { return };
        if session.paste(image).is_none() {
            crate::files::no_room(session);
        }
    }

    /// What is selected copied and pasted as a new layer; nothing selected, the layer duplicated.
    fn duplicate_current(&mut self, session: &CanvasSession) {
        if session.has_selection() {
            self.copy_layer(session);
            self.paste(session);
        } else if let Some(current) = session.layers().into_iter().find(|l| l.current)
            && session.duplicate_layer(current.id).is_none()
        {
            crate::files::no_room(session);
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
            first: None,
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
        let modal = match which {
            Popover::Layers => self.layers.modal(),
            Popover::Brushes => self.library.modal(),
            Popover::Colour => self.colours.modal(),
            Popover::Actions => self.licences || self.actions_panel.modal(),
            Popover::Adjustments => false,
        };
        let shown = match which {
            Popover::Brushes => {
                let shown = self.library.show(ctx, &mut self.look, session, tool, button, backdrops);
                // what was done in the library is kept
                self.keep_brushes |= std::mem::take(&mut self.library.changed);
                shown
            }
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
            Popover::Adjustments => {
                let (shown, tapped) = crate::adjustments::menu(ctx, &mut self.look, button, backdrops);
                if let Some(a) = tapped {
                    self.start_adjustment(session, a);
                }
                shown
            }
            Popover::Actions => {
                let assist = session.animation_settings();
                let showing = crate::actions::Showing {
                    can_paste: self.clipboard.is_some(),
                    animation: assist.as_ref().is_some_and(|a| !a.pages),
                    pages: assist.as_ref().is_some_and(|a| a.pages),
                    time_lapse: self.time_lapse.recording(),
                    reference: self.reference.on,
                    guide: session.guide().on,
                    side_bar: self.side_bar_shown,
                };
                let (shown, act) =
                    self.actions_panel.show(ctx, &mut self.look, button, &showing, &mut self.prefs, backdrops);
                match act {
                    Some(Act::Prefs) => {
                        self.prefs.apply(session);
                        session.set_touch_settings(self.gestures.touch_settings(self.prefs.rapid_undo_ms()));
                        self.prefs.keep();
                    }
                    Some(Act::SideBar) => self.side_bar_shown = !self.side_bar_shown,
                    Some(Act::TimeLapseReplay) => {
                        self.open = None;
                        self.time_lapse.replay();
                    }
                    Some(Act::TimeLapseRecording) => self.time_lapse.set_recording(!self.time_lapse.recording()),
                    Some(Act::ExportTimeLapse) => {
                        self.open = None;
                        self.time_lapse.ask_length();
                    }
                    Some(Act::AnimatedExport(format)) => {
                        self.open = None;
                        self.animated_export = Some(crate::animated_export::AnimatedExport::new(format));
                    }
                    Some(act @ (Act::AnimationAssist | Act::PageAssist)) => {
                        self.animation.reset(session);
                        let pages = act == Act::PageAssist;
                        let on = session.animation_settings().is_some_and(|a| a.pages == pages);
                        if pages {
                            session.set_page_assist(!on);
                        } else {
                            session.set_animation_assist(!on);
                        }
                    }
                    Some(Act::Licences) => self.licences = true,
                    Some(Act::Flip { horizontal }) => session.flip_canvas(horizontal),
                    Some(act @ (Act::InsertFile | Act::InsertPhoto)) => {
                        self.open = None;
                        self.insert_picture(session, act == Act::InsertPhoto);
                    }
                    Some(Act::Share(format)) => {
                        self.actions.push(Action::Export(format));
                        self.open = None;
                    }
                    Some(Act::About) => crate::files::about(),
                    Some(Act::GestureControls) => {
                        self.open = None;
                        self.gesture_controls.open = true;
                    }
                    Some(Act::Reference) => self.reference.on = !self.reference.on,
                    Some(Act::DrawingGuide) => _ = session.set_guide_on(!session.guide().on),
                    Some(Act::EditGuide) => {
                        self.open = None;
                        self.guide_editor.begin(ctx, session);
                    }
                    Some(Act::Crop) => {
                        self.open = None;
                        self.crop.begin(session);
                    }
                    Some(Act::CanvasInfo) => {
                        self.open = None;
                        self.canvas_info = Some(crate::canvas_info::CanvasInfo::new(session, self.artwork_dir.clone()));
                    }
                    Some(Act::Cut) => self.cut_layer(session),
                    Some(Act::Copy) => self.copy_layer(session),
                    Some(Act::CopyCanvas) => self.copy_canvas(session),
                    Some(Act::Paste) => self.paste(session),
                    // the panel does these itself
                    Some(Act::Page(_) | Act::ScalingMenu | Act::RightHanded | Act::BrushCursor | Act::ResetInput)
                    | None => {}
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
        // a text field, or the hex keypad, takes the keys
        if ctx.egui_wants_keyboard_input() || (self.open == Some(Popover::Colour) && self.colours.takes_keys()) {
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
            self.copy_layer(session);
        }
        if pressed(Modifiers::COMMAND, Key::X) {
            self.cut_layer(session);
        }
        if pressed(Modifiers::COMMAND, Key::V) {
            self.paste(session);
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
        if plain(Key::J) {
            self.duplicate_current(session);
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

/// The drawing guide's lines over the canvas while it is on.
fn guide_lines(ctx: &egui::Context, session: &CanvasSession) {
    let g = session.guide();
    if !g.on {
        return;
    }
    let ppp = ctx.pixels_per_point();
    let [r, gr, b] = [0, 1, 2].map(|k| (g.color.get(k).copied().unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8);
    let colour = Color32::from_rgba_unmultiplied(r, gr, b, (g.opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
    let stroke = Stroke::new(0.25 + 1.5 * g.thickness, colour);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::unique("drawing guide")));
    for &[x0, y0, x1, y1] in g.lines.as_chunks::<4>().0 {
        painter.line_segment([egui::pos2(x0, y0) / ppp, egui::pos2(x1, y1) / ppp], stroke);
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
