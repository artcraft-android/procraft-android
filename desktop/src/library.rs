//! The brush library, as the Android app's (ui/BrushLibrary.kt): sets on the left, the shown set's
//! brushes with a sample stroke on the right. Tapping a set shows it, the shown one for its menu
//! (rename, duplicate, share, delete); tapping a brush chooses it; sliding one left shows Share,
//! Duplicate and Delete; holding one drags it to another place or set. The plus makes a brush or a
//! set, or brings brushes in from a file.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align2, Color32, Pos2, Rect, ScrollArea, Sense, TextureHandle, TextureOptions, Ui, UiBuilder, Vec2, pos2, vec2,
};
use procraft_core::{BrushInfo, BrushSetInfo, CanvasSession, ReadBrushes, ToolKind, read_any_brushes};

use crate::alert::{Alert, Answer};
use crate::icons;
use crate::look::{Look, Shade, Themed, Weight, argb, pc, text};
use crate::menu::{self, Entry, Menu};
use crate::panel::{self, PANEL_GLYPH, PANEL_TITLE};

const LIBRARY_WIDTH: f32 = 440.0;
const SETS_WIDTH: f32 = 168.0;
const HEADER_HEIGHT: f32 = 52.0;
/// The search field with the space above and below it (out of sight until the lists are scrolled
/// up past their top), its fill and hint, and a found brush's tile.
const SEARCH_BLOCK: f32 = 60.0;
const SEARCH_FILL: u32 = 0x1F767680;
const SEARCH_HINT: u32 = 0x4DEBEBF5;
const RESULT_FILL: u32 = 0x14FFFFFF;
const RESULT_HEIGHT: f32 = 81.0;
/// The clear button's cross, cut through to the panel.
const CLEAR_CROSS: Themed = Themed(0xFF1E1E1E, 0xFFF2F2F2);
const SET_HEIGHT: f32 = 50.0;
const BRUSH_HEIGHT: f32 = 80.0;
const BRUSH_GAP: f32 = 8.0;
/// The library ends this far above the window's bottom edge.
const BOTTOM_MARGIN: f32 = 25.0;
const SET_COLUMN: Themed = Themed(0x00000000, 0xFFE6E6E6);
const SET_CELL: Themed = Themed(0x2E5B5B5B, 0xFFD7D7D7);
const BRUSH_COLUMN: Themed = Themed(0x26000000, 0xFFE0E0E0);
const NAME_PILL: u32 = 0x80000000;
/// The sample stroke's picture: half its own canvas, the cell showing it across its width.
const PREVIEW: (u32, u32) = (530, 162);
/// Sample strokes drawn on a thread of their own at a time, so those still in sight go next.
const PREVIEWS_AT_A_TIME: usize = 4;
/// How often the interface looks for sample strokes drawn while a thread is at them.
const PREVIEW_LOOK: Duration = Duration::from_millis(16);
/// Menus stay below the top bar; the set's menu sits this far left of it.
const MENU_TOP: f32 = 57.0;
const SET_MENU_GAP: f32 = 6.0;
/// The Recent set's menu, of one item, is this much taller than its item and margins (65 in all).
const RECENT_MENU_EXTRA: f32 = 11.0;
/// The set's menu: frosted glass over the canvas, edged.
const SET_MENU_GLASS: menu::Glass =
    menu::Glass { fill: menu::GLASS, edge: Some(menu::EDGE), frost: true, corner: menu::LIBRARY_CORNER, shadow: false };
/// A brush slid left shows three buttons filling its place, this far apart.
const SWIPE_GAP: f32 = 5.5;
const SWIPE_FILL: u32 = 0xFF3F3F3F;
const SWIPE_INK: u32 = 0xFFC5C5C5;
const SWIPE_DELETE: u32 = 0xFFFF3A46;
/// A brush slid right is picked, this fill over its row; it follows the pointer this far.
const PICKED_BRUSH: u32 = 0xFF2C5281;
const PICK_REACH: f32 = 72.0;
/// The set icons to choose from: five across, each cell this big, in a panel this big (edged,
/// nearly solid over the library), 11 left of the set, kept 25 above the window's bottom.
const PICKER_COLUMNS: usize = 5;
const PICKED_SET_ICONS: usize = 42;
const PICKER: (f32, f32) = (250.0, 325.0);
const PICKER_CELL: f32 = 48.0;
const PICKER_INSET: f32 = 11.0;
const PICKER_MARGIN: f32 = 25.0;
const PICKER_GLASS: menu::Glass = menu::Glass {
    fill: Themed(0xFA303335, 0xFA303335),
    edge: Some(menu::EDGE),
    frost: false,
    corner: 26.0,
    shadow: false,
};
/// A press moved this far (points) has gone somewhere; held still this long, it drags the brush.
const SLOP: f32 = 8.0;
const HOLD: Duration = Duration::from_millis(400);
/// A press slides its brush once this far sideways (points), going this many times further
/// sideways than up or down: a hand scrolling swings in an arc, and a slide opens onto Delete, so
/// anything less plainly sideways is left to the list.
const SLIDE_SLOP: f32 = 14.0;
const SLIDE_LEAN: f32 = 2.0;

/// A brush's sample stroke, and whether it runs under the name (which then sits on a dark pill).
struct Preview {
    texture: TextureHandle,
    shaded: bool,
}

/// A sample stroke drawn on its thread: the brush's, its picture (none if it draws nothing), and
/// whether it runs under the name.
type DrawnPreview = ((u32, u32), Option<(egui::ColorImage, bool)>);

/// What a press on a brush turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Way {
    Undecided,
    /// Sideways: the brush slides.
    Swipe,
    /// Held: the brush is dragged to another place or set.
    Drag,
    /// Up or down first: the list has it, and scrolls.
    Scroll,
}

/// What a press on a brush that has moved `d` (points) from where it went down is, once it can be
/// told: plainly sideways it slides the brush; else, gone up or down, the list has it.
fn way_moved(d: Vec2) -> Option<Way> {
    let (across, along) = (d.x.abs(), d.y.abs());
    if across >= SLIDE_SLOP && across >= along * SLIDE_LEAN {
        Some(Way::Swipe)
    } else if along >= SLOP && across < along * SLIDE_LEAN {
        Some(Way::Scroll)
    } else {
        None
    }
}

/// A press on a brush, followed until let go: which (set and place), where and when it went down,
/// what it became, how far the brush was slid, where on it the press took hold, how far the
/// pointer has moved since the last frame, and how far the list was scrolled.
#[derive(Clone, Copy)]
struct Press {
    list: f32,
    key: (u32, u32),
    origin: Pos2,
    since: Instant,
    way: Way,
    start: f32,
    grab: Vec2,
    last: Pos2,
}

/// A press on a set, followed until let go: which, where and when it went down, where on it the
/// press took hold, where the pointer is now, whether it was held still long enough to lift the set,
/// and whether it moved first (then it is the list's, to scroll).
#[derive(Clone, Copy)]
struct SetPress {
    set: usize,
    origin: Pos2,
    since: Instant,
    grab: Vec2,
    last: Pos2,
    lifted: bool,
    spent: bool,
}

/// What Delete asks about.
#[derive(Clone, Copy)]
enum Deleting {
    Set(u32),
    Brush(u32, u32),
    Picked,
}

#[derive(Default)]
pub struct Library {
    /// The set shown, once one is tapped (else the current brush's).
    shown_set: Option<usize>,
    /// The Recent set is shown instead (the brushes last painted with, and those pinned there);
    /// its menu (Clear), and a brush found from it, to scroll to in its own set.
    recent: bool,
    recent_menu: Menu,
    found: Option<(u32, u32)>,
    previews: HashMap<(u32, u32), Option<Preview>>,
    /// The brushes in sight without a sample stroke yet, and those being drawn on a thread of
    /// their own (the GPU takes a while over each: the list isn't held up for it).
    wanted: Vec<((u32, u32), String)>,
    drawing: Option<std::sync::mpsc::Receiver<DrawnPreview>>,
    plus_menu: Menu,
    /// The shown set's menu, and which set it is for.
    set_menu: Menu,
    set_menu_for: Option<usize>,
    /// The set's icons to choose from (Customize icon), and whether it has been scrolled to the
    /// set's own.
    icon_picker: Menu,
    picker_scrolled: bool,
    /// A set renamed in place: which, the name as typed, and whether it has been focused.
    renaming_set: Option<(usize, String, bool)>,
    deleting: Alert,
    delete_what: Option<Deleting>,
    press: Option<Press>,
    set_press: Option<SetPress>,
    /// How far each brush is slid (points, left negative), and the brush slid open.
    slid: HashMap<(u32, u32), f32>,
    swiped: Option<(u32, u32)>,
    /// The brushes of the shown set picked by sliding them right.
    picked: Vec<(u32, u32)>,
    /// How far the brushes list is scrolled.
    list_at: f32,
    /// Where the sets and the shown set's brushes were last drawn, for a dragged brush to land.
    set_rects: Vec<(usize, Rect)>,
    brush_rects: Vec<((u32, u32), Rect)>,
    /// The library changed: the app keeps it; its sample strokes are drawn again next frame (not
    /// dropped in the middle of one that shows them).
    pub changed: bool,
    stale: bool,
    /// The studio asked for: on the brush now chosen, or on one just made (Cancel throws it away).
    pub studio: Option<Option<(u32, u32)>>,
    /// Brush files read on threads of their own (another app's big ones take a while): each
    /// batch's brushes come back (a file that held none left out), and whether to say that they
    /// came in.
    importing: Vec<(std::sync::mpsc::Receiver<Vec<ReadBrushes>>, bool)>,
    /// The search: what is typed, whether its field has the keys, whether it has come down into
    /// sight, and whether the sets and the brushes were at their tops last frame.
    query: String,
    searching: bool,
    search_down: bool,
    lists_top: (bool, bool),
}

impl Library {
    /// The tool changed: the library shows its brush's set again.
    pub fn tool_changed(&mut self) {
        self.shown_set = None;
    }

    /// A menu or question of the library's is up: a press elsewhere only closes it.
    pub fn modal(&self) -> bool {
        self.plus_menu.is_open()
            || self.set_menu.is_open()
            || self.recent_menu.is_open()
            || self.icon_picker.is_open()
            || self.deleting.is_open()
    }

    /// Brush files' `(bytes, name)` read on a thread of their own, their brushes added to the
    /// library once read (`added`). `tell`: say that they came in (else only that they held none).
    pub fn add_brushes(&mut self, files: Vec<(Vec<u8>, String)>, tell: bool) {
        if files.is_empty() {
            return;
        }
        let (done, importing) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let read = files.into_iter().filter_map(|(bytes, name)| read_any_brushes(&bytes, Some(name))).collect();
            let _ = done.send(read);
        });
        self.importing.push((importing, tell));
    }

    /// Brushes read meanwhile added to the library, each batch's set shown: whether any came in,
    /// and whether to say so. Not while the library holds brushes or sets by their places (a
    /// menu, a question, a set being renamed, a brush pressed, the studio asked for or open:
    /// `studio_open`), which brushes added at the top would move; they wait for that to end.
    pub fn added(&mut self, session: &CanvasSession, studio_open: bool) -> Vec<(bool, bool)> {
        let holding = self.modal() || self.renaming_set.is_some() || self.press.is_some() || self.studio.is_some();
        if studio_open || holding {
            return Vec::new();
        }
        let mut came = Vec::new();
        self.importing.retain(|(r, tell)| match r.try_recv() {
            Ok(read) => {
                came.push((read, *tell));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        came.into_iter().map(|(read, tell)| (self.add_read(session, read), tell)).collect()
    }

    /// Brushes read from files added one after another, each into the set the library shows (or
    /// the current brush's) when it is one of ours from before, that set then shown. Whether any
    /// were.
    fn add_read(&mut self, session: &CanvasSession, read: Vec<ReadBrushes>) -> bool {
        let mut any = false;
        for brushes in read {
            let shown = self.shown_set.filter(|_| !self.recent).unwrap_or(session.current_brush().set as usize) as u32;
            if let Some(set) = session.add_read_brushes(brushes, shown) {
                self.shown_set = Some(set as usize);
                self.recent = false;
                any = true;
            }
        }
        if any {
            self.library_changed();
        }
        any
    }

    /// Brush files are still being read, or their brushes wait to be added.
    pub fn importing(&self) -> bool {
        !self.importing.is_empty()
    }

    /// Waits for brush files still being read and adds their brushes (the artwork is closing:
    /// they are kept with the rest).
    pub fn finish_importing(&mut self, session: &CanvasSession) {
        for (importing, _) in std::mem::take(&mut self.importing) {
            if let Ok(read) = importing.recv() {
                self.add_read(session, read);
            }
        }
    }

    /// The library changed: its sample strokes are drawn again (they go by place), and it is kept.
    pub fn library_changed(&mut self) {
        self.stale = true;
        self.slid.clear();
        self.swiped = None;
        self.picked.clear();
        self.changed = true;
    }

    /// Shows the library hanging from `button`; returns where it is (points).
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &Arc<CanvasSession>,
        tool: ToolKind,
        button: Rect,
        backdrops: &mut Vec<f32>,
    ) -> Rect {
        let screen = ctx.content_rect();
        let (left, arrow_x) = panel::place(ctx, LIBRARY_WIDTH, button.center().x);
        let top = screen.top() + pc::BAR_HEIGHT;
        let rect = Rect::from_min_size(
            pos2(left, top),
            vec2(LIBRARY_WIDTH, (screen.bottom() - BOTTOM_MARGIN - top).max(200.0)),
        );
        if std::mem::take(&mut self.stale) {
            self.previews.clear();
            self.drawing = None;
        }
        self.wanted.clear();
        let library = session.brush_library();
        let current = session.current_brush();
        // with the Recent set shown, the current brush's set takes new brushes
        let shown = self
            .shown_set
            .filter(|_| !self.recent)
            .unwrap_or(current.set as usize)
            .min(library.len().saturating_sub(1));
        let recent = self.recent;
        let recents = if recent { session.recent_brushes() } else { Vec::new() };
        self.picked.retain(|p| !recent && p.0 as usize == shown);
        self.settle_slides(ctx);
        // the set a dragged brush or set is over
        let dragging = self.press.filter(|p| p.way == Way::Drag);
        let lifted = self.set_press.filter(|p| p.lifted);
        let taken = self.press.is_some_and(|p| matches!(p.way, Way::Swipe | Way::Drag));
        // a press on a brush holds the list where it was (stopping it if it was running on) until
        // it has gone up or down
        let held_at = self.press.filter(|p| p.way != Way::Scroll).map(|p| p.list);
        let over_set = dragging
            .map(|p| p.last)
            .or(lifted.map(|p| p.last))
            .and_then(|at| self.set_rects.iter().find(|(_, r)| r.contains(at)).map(|(i, _)| *i))
            .filter(|i| lifted.is_none_or(|p| p.set != *i));
        let mut set_rects = Vec::new();
        let mut brush_rects = Vec::new();
        let mut plus = Rect::NOTHING;
        let mut lists = Rect::NOTHING;
        let mut set_tapped = None;
        let mut recent_row = None;
        let mut recent_tapped = false;
        let mut brush_tapped = None;
        let mut swipe_chose = None;
        egui::Area::new(egui::Id::unique("brush library")).fixed_pos(rect.min).order(egui::Order::Foreground).show(
            ctx,
            |ui| {
                // nothing under the panel takes a touch
                ui.allocate_rect(rect, Sense::click());
                panel::frame(ui, look, "library frame", rect, arrow_x, backdrops);
                let body = panel::body(rect);
                let title = match tool {
                    ToolKind::Erase => "Erase library",
                    ToolKind::Smudge => "Smudge library",
                    ToolKind::Paint => "Brush library",
                };
                panel::header(ui, look, body, title, 20.0, true, false);
                // the plus: new brushes and sets, and importing
                plus = Rect::from_min_size(pos2(body.right() - 12.0 - 41.0, body.top() + 12.0), vec2(41.0, 40.0));
                if self.picked.is_empty() {
                    look.icon(ui, &icons::PLUS_MEDIUM, plus.center(), 41.0, argb(PANEL_GLYPH));
                    if ui.interact(plus, ui.scope_id().with("plus"), Sense::click()).clicked() {
                        self.plus_menu.open();
                    }
                } else {
                    self.picked_actions(ui, session, plus);
                }

                lists = Rect::from_min_max(pos2(body.left(), body.top() + HEADER_HEIGHT), body.max);
                // the search: out of sight at first, it comes down as a list is scrolled up past
                // its top, and goes as they scroll down again (unless it is in use)
                let in_use = self.searching || !self.query.is_empty();
                // (by the wheel, or a finger or the mouse pulling a list)
                let pulled =
                    self.press.is_some_and(|p| p.way == Way::Scroll) || self.set_press.is_some_and(|p| p.spent);
                let wheel = ui.input(|i| i.smooth_scroll_delta.y + if pulled { i.pointer.delta().y } else { 0.0 });
                if !in_use && let Some(p) = ui.input(|i| i.pointer.hover_pos()).filter(|p| lists.contains(*p)) {
                    let at_top = if p.x < lists.left() + SETS_WIDTH { self.lists_top.0 } else { self.lists_top.1 };
                    if wheel > 0.0 && at_top {
                        self.search_down = true;
                    } else if wheel < 0.0 {
                        self.search_down = false;
                    }
                }
                let shown_down = in_use || self.search_down;
                let down =
                    ui.ctx().animate_bool_with_time(egui::Id::unique("library search"), shown_down, 0.2) * SEARCH_BLOCK;
                if down > 0.0 {
                    let block = Rect::from_min_size(
                        pos2(lists.left(), lists.top() + down - SEARCH_BLOCK),
                        vec2(lists.width(), SEARCH_BLOCK),
                    );
                    self.search_field(ui, block, lists);
                }
                lists.min.y += down;
                if in_use {
                    if let Some(key) = self.search_results(ui, &library, (current.set, current.index), lists) {
                        brush_tapped = Some((key, false));
                    }
                } else {
                    let sets = Rect::from_min_size(lists.min, vec2(SETS_WIDTH, lists.height()));
                    let brushes = Rect::from_min_max(pos2(lists.left() + SETS_WIDTH, lists.top()), lists.max);
                    if SET_COLUMN.value() >> 24 != 0 {
                        ui.painter().rect_filled(sets, 0, argb(SET_COLUMN));
                    }
                    let corner = panel::POPOVER_CORNER;
                    panel::rounded(ui, look, "library brushes", brushes, [0.0, 0.0, corner, 0.0], argb(BRUSH_COLUMN));

                    let sets_out = ui.scope_builder(UiBuilder::new().max_rect(sets), |ui| {
                        ScrollArea::vertical()
                            .id_salt("library sets")
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                            // a lifted set is dragged, not the list
                            .scroll_source(if lifted.is_some() {
                                egui::scroll_area::ScrollSource::NONE
                            } else {
                                egui::scroll_area::ScrollSource::default()
                            })
                            .show(ui, |ui| {
                                ui.add_space(12.0);
                                // the brushes last painted with, and those pinned there, above the sets
                                let (response, at) = self.recent_row(ui, look, recent);
                                recent_row = Some(at);
                                recent_tapped = response.clicked();
                                ui.add_space(12.0);
                                for (i, set) in library.iter().enumerate() {
                                    let fill = if over_set == Some(i) { Some(argb(pc::ACCENT)) } else { None };
                                    // a lifted set is faint where it was
                                    let faint = lifted.is_some_and(|p| p.set == i);
                                    let (response, at) = ui
                                        .scope(|ui| {
                                            if faint {
                                                ui.multiply_opacity(0.3);
                                            }
                                            self.set_row(ui, look, session, i, set, i == shown && !recent, fill)
                                        })
                                        .inner;
                                    set_rects.push((i, at));
                                    if self.set_press.is_none()
                                        && self.press.is_none()
                                        && response.is_pointer_button_down_on()
                                        && let Some(p) = response.interact_pointer_pos()
                                    {
                                        self.set_press = Some(SetPress {
                                            set: i,
                                            origin: p,
                                            since: Instant::now(),
                                            grab: p - at.min,
                                            last: p,
                                            lifted: false,
                                            spent: false,
                                        });
                                    }
                                    if response.clicked() {
                                        set_tapped = Some(i);
                                    }
                                    ui.add_space(12.0);
                                }
                            })
                    });
                    let brushes_out = ui.scope_builder(UiBuilder::new().max_rect(brushes), |ui| {
                        let list = ScrollArea::vertical()
                            .id_salt(("library brushes", if recent { usize::MAX } else { shown }))
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                            // the list follows a press itself, and carries on when flicked; a brush
                            // slid or dragged keeps it still
                            .scroll_source(if taken {
                                egui::scroll_area::ScrollSource::NONE
                            } else {
                                egui::scroll_area::ScrollSource::ALL
                            });
                        let list = match held_at {
                            Some(at) => list.vertical_scroll_offset(at),
                            None => list,
                        };
                        list.show(ui, |ui| {
                            ui.add_space(9.75);
                            // the Recent set's brushes (pinned there or not), or the shown set's
                            let listed: Vec<(&BrushInfo, Option<bool>)> = if recent {
                                recents
                                    .iter()
                                    .filter_map(|r| {
                                        let b = library.get(r.set as usize)?.brushes.get(r.index as usize)?;
                                        Some((b, Some(r.pinned)))
                                    })
                                    .collect()
                            } else {
                                library
                                    .get(shown)
                                    .map(|set| set.brushes.iter().map(|b| (b, None)).collect())
                                    .unwrap_or_default()
                            };
                            let chosen = library.get(shown).and_then(|set| set.chosen);
                            for (b, pinned) in listed {
                                // the current brush, or the brush last chosen in this set (in the
                                // Recent set the current brush alone)
                                let selected = (b.set, b.index) == (current.set, current.index)
                                    || (!recent && chosen == Some(b.index));
                                let (response, cell) = self.brush_row(ui, look, b, selected, pinned);
                                // a brush found from the Recent set is scrolled to in its own
                                if self.found == Some((b.set, b.index)) {
                                    self.found = None;
                                    ui.scroll_to_rect(cell, Some(egui::Align::Center));
                                }
                                brush_rects.push(((b.set, b.index), cell));
                                if let Some(r) = response {
                                    if r.0 {
                                        brush_tapped = Some(((b.set, b.index), selected));
                                    }
                                    if let Some(chose) = r.1 {
                                        swipe_chose = Some(((b.set, b.index), b.name.clone(), chose));
                                    }
                                }
                                ui.add_space(BRUSH_GAP);
                            }
                            ui.add_space(12.0 - BRUSH_GAP);
                        })
                    });
                    self.list_at = brushes_out.inner.state.clamped_offset().y;
                    self.lists_top = (
                        sets_out.inner.state.clamped_offset().y <= 0.5,
                        brushes_out.inner.state.clamped_offset().y <= 0.5,
                    );
                }
                // the dragged brush follows the pointer, over the lists
                if let Some(p) = dragging
                    && let Some(b) = library.iter().flat_map(|s| &s.brushes).find(|b| (b.set, b.index) == p.key)
                {
                    let size = self
                        .brush_rects
                        .iter()
                        .find(|(k, _)| *k == p.key)
                        .map_or(vec2(240.0, BRUSH_HEIGHT), |(_, r)| r.size());
                    let at = Rect::from_min_size(p.last - p.grab, size);
                    let painter = ui.painter_at(lists);
                    painter.rect_filled(at, 12, argb(pc::ACCENT).gamma_multiply(0.9));
                    self.preview_in(&painter, (b.set, b.index), at, true);
                    let name = one_line(ui, &b.name, 14.0, Weight::SemiBold, Color32::WHITE, at.width() - 28.0);
                    painter.galley(at.min + vec2(18.0, 11.0), name, Color32::WHITE);
                    ctx.request_repaint();
                }
                // and the lifted set
                if let Some(p) = lifted
                    && let Some(set) = library.get(p.set)
                {
                    let size = self
                        .set_rects
                        .iter()
                        .find(|(s, _)| *s == p.set)
                        .map_or(vec2(SETS_WIDTH - 24.0, SET_HEIGHT), |(_, r)| r.size());
                    let at = Rect::from_min_size(p.last - p.grab, size);
                    let painter = ui.painter_at(lists);
                    painter.rect_filled(at, 12, argb(pc::ACCENT).gamma_multiply(0.9));
                    let icon = icons::SET_ICONS.get(set.icon as usize).unwrap_or(&icons::BRUSH_SET);
                    look.icon(ui, icon, pos2(at.left() + 24.0, at.center().y), 40.0, Color32::WHITE);
                    let left = at.left() + 4.0 + 40.0 + 1.0;
                    let name = one_line(ui, &set.name, 14.0, Weight::SemiBold, Color32::WHITE, at.right() - 8.0 - left);
                    painter.galley(pos2(left, at.center().y - name.size().y / 2.0), name, Color32::WHITE);
                    ctx.request_repaint();
                }
            },
        );
        self.set_rects = set_rects;
        self.brush_rects = brush_rects;
        self.draw_previews(ctx, session);
        self.follow_press(ctx, session);
        // a set let go after it was lifted was dragged, not tapped
        if self.follow_set_press(ctx, session, shown) {
            set_tapped = None;
        }

        if let Some(i) = set_tapped {
            if i == shown && !recent {
                self.set_menu_for = Some(i);
                self.set_menu.open();
            } else {
                self.shown_set = Some(i);
                self.recent = false;
            }
        }
        if recent_tapped {
            if recent {
                self.recent_menu.open();
            } else {
                self.recent = true;
                self.swiped = None;
            }
        }
        if let Some((key, selected)) = brush_tapped {
            // with a brush slid open, a tap only closes it; with brushes picked, only lets them go
            if self.swiped.is_some() {
                self.swiped = None;
            } else if !self.picked.is_empty() {
                self.picked.clear();
            } else {
                session.select_brush(key.0, key.1);
                self.changed = true;
                // a highlighted brush opens in the studio
                if selected {
                    self.studio = Some(None);
                }
            }
        }
        if let Some((key, name, chose)) = swipe_chose {
            self.swiped = None;
            match chose {
                Swiped::Share => {
                    if let Some(bytes) = session.brush_as_file(key.0, key.1) {
                        crate::files::save_file(&bytes, &name, "brush", "Brush");
                    }
                }
                Swiped::Duplicate => {
                    if session.duplicate_brush(key.0, key.1).is_some() {
                        self.library_changed();
                    }
                }
                Swiped::Delete => self.ask_delete(Deleting::Brush(key.0, key.1)),
                // shown in its own set, chosen there
                Swiped::Find => {
                    self.recent = false;
                    self.shown_set = Some(key.0 as usize);
                    self.found = Some(key);
                    session.select_brush(key.0, key.1);
                    self.changed = true;
                }
                Swiped::Pin | Swiped::Unpin => {
                    self.changed |= session.pin_recent_brush(key.0, key.1, matches!(chose, Swiped::Pin));
                }
                Swiped::Clear => self.changed |= session.clear_recent_brush(key.0, key.1),
            }
        }
        self.plus_menu(ctx, look, session, plus, shown, backdrops);
        self.set_menu(ctx, look, session, &library, backdrops);
        if let Some(row) = recent_row {
            self.recent_menu(ctx, look, session, row, backdrops);
        }
        if self.deleting.show(ctx, look) == Some(Answer::Action) {
            match self.delete_what.take() {
                Some(Deleting::Set(i)) => {
                    if session.delete_brush_set(i) {
                        let at = i as usize;
                        self.shown_set = Some(if shown >= at { shown.saturating_sub(1) } else { shown });
                        self.library_changed();
                    }
                }
                Some(Deleting::Brush(set, index)) if session.delete_brush(set, index) => self.library_changed(),
                Some(Deleting::Picked) => {
                    // from the last up, so the places of the rest hold
                    self.picked.sort_unstable();
                    for (set, index) in self.picked.iter().rev() {
                        session.delete_brush(*set, *index);
                    }
                    self.library_changed();
                }
                Some(Deleting::Brush(..)) | None => {}
            }
        }
        rect
    }

    /// In the place of the `plus`: Delete for the picked brushes, and Combine beside it
    /// for one in the current brush's set, it taking the picked brush as its second.
    fn picked_actions(&mut self, ui: &Ui, session: &CanvasSession, plus: Rect) {
        let (right, top) = (plus.right(), plus.top() + 4.0);
        let word = |ui: &Ui, text: &str, at: Rect, salt: &str| {
            let galley = one_line(ui, text, 15.0, Weight::Regular, argb(SWIPE_INK), at.width());
            let middle = at.center() - galley.size() / 2.0;
            ui.painter().galley(middle, galley, argb(SWIPE_INK));
            ui.interact(at, ui.scope_id().with(salt), Sense::click()).clicked()
        };
        let delete = Rect::from_min_size(pos2(right - 63.0, top), vec2(63.0, 32.0));
        if word(ui, "Delete", delete, "delete picked") {
            self.ask_delete(Deleting::Picked);
        }
        let current = session.current_brush();
        let pair = match self.picked[..] {
            [(set, index)] if set == current.set => Some(index),
            _ => None,
        };
        let pair = pair
            .filter(|i| !session.brush_is_dual(current.set, *i) && !session.brush_is_dual(current.set, current.index));
        if let Some(second) = pair {
            let combine = Rect::from_min_size(pos2(delete.left() - 4.0 - 80.0, top), vec2(80.0, 32.0));
            if word(ui, "Combine", combine, "combine picked")
                && session.combine_brushes(current.set, current.index, second)
            {
                self.library_changed();
            }
        }
    }

    fn ask_delete(&mut self, what: Deleting) {
        let title = match what {
            Deleting::Set(_) => "Delete this set and its brushes?",
            Deleting::Brush(..) => "Delete this brush?",
            Deleting::Picked => "Delete these brushes?",
        };
        self.delete_what = Some(what);
        self.deleting.open(title, "It can't be brought back.", "Delete", Some(&icons::TRASH));
    }

    /// The plus's menu, hanging below it, their right edges in line: a new brush, a new set, or
    /// brushes from a file.
    #[allow(clippy::too_many_arguments)]
    fn plus_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &Arc<CanvasSession>,
        plus: Rect,
        shown: usize,
        backdrops: &mut Vec<f32>,
    ) {
        let ink = menu::INK;
        // the Recent set holds no brushes of its own to add one to
        let mut entries = vec![
            Entry::Item(&icons::MENU_NEW_BRUSH, "Create new brush", ink),
            Entry::Item(&icons::MENU_NEW_SET, "Create new set", ink),
            Entry::Rule(11.0),
            Entry::Item(&icons::MENU_FOLDER, "Import from Files", ink),
        ];
        if self.recent {
            entries.remove(0);
        }
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries));
        let rect = Rect::from_min_size(pos2(plus.right() - size.x, plus.bottom()), size);
        let mut tapped = None;
        self.plus_menu.show(ctx, "library plus menu", rect, rect.right_top(), menu::OVER_A_PANEL, backdrops, |ui| {
            tapped = menu::entries(ui, look, rect, &entries);
        });
        let Some(Entry::Item(_, label, _)) = tapped.and_then(|t| entries.get(t)) else { return };
        self.plus_menu.close();
        match *label {
            // a new brush, opened in the studio
            "Create new brush" => {
                if let Some(index) = session.new_brush(shown as u32) {
                    session.select_brush(shown as u32, index);
                    self.library_changed();
                    self.studio = Some(Some((shown as u32, index)));
                }
            }
            "Create new set" => {
                let set = session.new_brush_set() as usize;
                self.shown_set = Some(set);
                self.recent = false;
                let name = session.brush_library().get(set).map(|s| s.name.clone()).unwrap_or_default();
                self.renaming_set = Some((set, name, false));
                self.library_changed();
            }
            _ => {
                let Some(file) = crate::files::pick_file("Import brushes") else { return };
                self.shown_set = Some(shown);
                self.add_brushes(vec![file], false);
            }
        }
    }

    /// The shown set's menu, left of it and centred on it: Rename, Duplicate, Customize icon (its
    /// icons to choose from beside it), Share, Delete.
    fn set_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        library: &[BrushSetInfo],
        backdrops: &mut Vec<f32>,
    ) {
        let Some(i) = self.set_menu_for else { return };
        let Some(set_rect) = self.set_rects.iter().find(|(s, _)| *s == i).map(|(_, r)| *r) else { return };
        let ink = menu::INK;
        let own = library.get(i).map_or(0, |s| s.icon as usize);
        let set_icon = icons::SET_ICONS.get(own).unwrap_or(&icons::BRUSH_SET);
        let mut entries = vec![
            Entry::Item(&icons::MENU_RENAME, "Rename", ink),
            Entry::Item(&icons::MENU_DUPLICATE, "Duplicate", ink),
            Entry::More(set_icon, "Customize icon", ink),
            menu::DIVIDER,
            Entry::Item(&icons::MENU_SHARE, "Share", ink),
        ];
        if library.len() > 1 {
            entries.push(menu::DIVIDER);
            entries.push(Entry::Item(&icons::MENU_TRASH, "Delete", menu::DANGER));
        }
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries));
        let screen = ctx.content_rect();
        let top = (set_rect.center().y - size.y / 2.0)
            .clamp(screen.top() + MENU_TOP, (screen.bottom() - size.y).max(screen.top() + MENU_TOP));
        let rect = Rect::from_min_size(pos2(set_rect.left() - SET_MENU_GAP - size.x, top), size);
        let mut tapped = None;
        self.set_menu.show(ctx, "library set menu", rect, rect.right_center(), SET_MENU_GLASS, backdrops, |ui| {
            tapped = menu::entries(ui, look, rect, &entries);
        });
        if !self.set_menu.is_open() {
            self.icon_picker.close();
        }
        // the icons to choose from: under the Customize icon row, or over it with no room below
        let row_top = rect.top() + menu::entry_top(&entries, 2);
        let (row_top, row_bottom) = (row_top, row_top + menu::ITEM_HEIGHT);
        let below = row_bottom + PICKER.1 <= screen.bottom() - PICKER_MARGIN;
        let picker = Rect::from_min_size(
            pos2(set_rect.left() - PICKER_INSET, if below { row_bottom } else { row_top - PICKER.1 }),
            vec2(PICKER.0, PICKER.1),
        );
        let mut picked = None;
        let scrolled = self.picker_scrolled;
        self.icon_picker.show(ctx, "set icon picker", picker, picker.min, PICKER_GLASS, backdrops, |ui| {
            picked = icon_grid(ui, look, picker, own, scrolled);
        });
        self.picker_scrolled = self.icon_picker.is_open();
        if let Some(icon) = picked {
            self.icon_picker.close();
            self.set_menu.close();
            if session.set_brush_set_icon(i as u32, icon as u32) {
                self.library_changed();
            }
            return;
        }
        let Some(Entry::Item(_, label, _) | Entry::More(_, label, _)) = tapped.and_then(|t| entries.get(t)) else {
            return;
        };
        if *label == "Customize icon" {
            if self.icon_picker.is_open() {
                self.icon_picker.close();
            } else {
                self.icon_picker.open();
            }
            return;
        }
        self.set_menu.close();
        match *label {
            "Rename" => {
                let name = library.get(i).map(|s| s.name.clone()).unwrap_or_default();
                self.renaming_set = Some((i, name, false));
            }
            "Duplicate" => {
                if let Some(copy) = session.duplicate_brush_set(i as u32) {
                    self.shown_set = Some(copy as usize);
                    self.library_changed();
                }
            }
            "Share" => {
                let name = library.get(i).map_or("Brushes", |s| s.name.as_str());
                if let Some(bytes) = session.brush_set_as_file(i as u32) {
                    crate::files::save_file(&bytes, name, "brushset", "Brush set");
                }
            }
            _ => self.ask_delete(Deleting::Set(i as u32)),
        }
    }

    /// The Recent set's menu, left of its row and centred on it as a set's: Clear alone.
    fn recent_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        row: Rect,
        backdrops: &mut Vec<f32>,
    ) {
        let entries = [Entry::Item(&icons::MENU_CLEAR, "Clear", menu::INK)];
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries) + RECENT_MENU_EXTRA);
        let screen = ctx.content_rect();
        let top = (row.center().y - size.y / 2.0)
            .clamp(screen.top() + MENU_TOP, (screen.bottom() - size.y).max(screen.top() + MENU_TOP));
        let rect = Rect::from_min_size(pos2(row.left() - SET_MENU_GAP - size.x, top), size);
        let mut tapped = None;
        self.recent_menu.show(ctx, "library recent menu", rect, rect.right_center(), SET_MENU_GLASS, backdrops, |ui| {
            tapped = menu::entries(ui, look, rect.shrink2(vec2(0.0, RECENT_MENU_EXTRA / 2.0)), &entries);
        });
        if tapped.is_some() {
            self.recent_menu.close();
            session.clear_recent_brushes();
            self.changed = true;
        }
    }

    /// The brushes not being slid go back, or open to their buttons, as they were left.
    fn settle_slides(&mut self, ctx: &egui::Context) {
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        let widths: HashMap<(u32, u32), f32> = self.brush_rects.iter().map(|(k, r)| (*k, r.width())).collect();
        let held = self.press.filter(|p| p.way == Way::Swipe).map(|p| p.key);
        let mut keys: Vec<(u32, u32)> = self.slid.keys().copied().collect();
        if let Some(open) = self.swiped
            && !keys.contains(&open)
        {
            keys.push(open);
        }
        for key in keys {
            if held == Some(key) {
                continue;
            }
            let to = if self.swiped == Some(key) { -widths.get(&key).copied().unwrap_or(0.0) } else { 0.0 };
            let v = self.slid.entry(key).or_insert(0.0);
            *v += (to - *v) * (dt * 14.0).min(1.0);
            if (to - *v).abs() < 0.5 {
                *v = to;
            } else {
                ctx.request_repaint();
            }
            if *v == 0.0 {
                self.slid.remove(&key);
            }
        }
    }

    /// Follows the press on a brush: moved sideways it slides the brush (let go past a third of
    /// the way, it stays open); moved up or down, the list scrolls; held still, the brush is dragged
    /// and, let go, goes to the top of the set it is over, or to the place of the brush it is over.
    fn follow_press(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        let Some(mut p) = self.press else { return };
        let (at, down) = ctx.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
        let at = at.unwrap_or(p.last);
        let d = at - p.origin;
        let width = self.brush_rects.iter().find(|(k, _)| *k == p.key).map_or(0.0, |(_, r)| r.width());
        if down {
            match p.way {
                Way::Undecided => {
                    if let Some(way) = way_moved(d) {
                        p.way = way;
                    } else if d.length() > SLOP {
                        // going somewhere, not yet plain which way: it is no hold
                    } else if p.since.elapsed() >= HOLD && !self.recent {
                        // (the Recent set's brushes stay where they are)
                        p.way = Way::Drag;
                        self.swiped = None;
                    } else {
                        ctx.request_repaint_after(HOLD.saturating_sub(p.since.elapsed()));
                    }
                }
                Way::Swipe => {
                    // the current brush counts as picked already
                    let current = session.current_brush();
                    let closed = self.swiped != Some(p.key) && !self.recent && p.key != (current.set, current.index);
                    let most = if closed { PICK_REACH } else { 0.0 };
                    self.slid.insert(p.key, (p.start + d.x).clamp(-width, most));
                    ctx.request_repaint();
                }
                Way::Scroll => {}
                Way::Drag => ctx.request_repaint(),
            }
            p.last = at;
            self.press = Some(p);
            return;
        }
        self.press = None;
        match p.way {
            Way::Swipe => {
                let slide = self.slid.get(&p.key).copied().unwrap_or(0.0);
                if slide > PICK_REACH / 2.0 {
                    if let Some(at) = self.picked.iter().position(|k| *k == p.key) {
                        self.picked.remove(at);
                    } else {
                        self.picked.push(p.key);
                    }
                }
                let open = self.swiped == Some(p.key);
                let opened = slide < -width / 3.0;
                if opened != open {
                    self.swiped = opened.then_some(p.key);
                }
            }
            Way::Drag => {
                // onto a set: to its top; onto a brush of this set: to its place
                let (set, index) = p.key;
                let moved = if let Some(&(to, _)) = self.set_rects.iter().find(|(_, r)| r.contains(at)) {
                    to as u32 != set && session.move_brush(set, index, to as u32, 0)
                } else if let Some(&(target, _)) = self.brush_rects.iter().find(|(_, r)| r.contains(at)) {
                    target != p.key && session.move_brush(set, index, target.0, target.1)
                } else {
                    false
                };
                if moved {
                    self.library_changed();
                }
            }
            Way::Undecided | Way::Scroll => {}
        }
    }

    /// Follows the press on a set: moved first, it is left to the list; held still, the set lifts
    /// and follows the pointer and, let go over another set, takes its place (the set shown stays
    /// shown). True when a lifted set was let go.
    fn follow_set_press(&mut self, ctx: &egui::Context, session: &CanvasSession, shown: usize) -> bool {
        let Some(mut p) = self.set_press else { return false };
        let (at, down) = ctx.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
        let at = at.unwrap_or(p.last);
        if down {
            if p.lifted {
                ctx.request_repaint();
            } else if !p.spent {
                if (at - p.origin).length() > SLOP {
                    p.spent = true;
                } else if p.since.elapsed() >= HOLD {
                    p.lifted = true;
                    ctx.request_repaint();
                } else {
                    ctx.request_repaint_after(HOLD.saturating_sub(p.since.elapsed()));
                }
            }
            p.last = at;
            self.set_press = Some(p);
            return false;
        }
        self.set_press = None;
        if !p.lifted {
            return false;
        }
        if let Some(&(to, _)) = self.set_rects.iter().find(|(_, r)| r.contains(at))
            && to != p.set
            && session.move_brush_set(p.set as u32, to as u32)
        {
            let from = p.set;
            self.shown_set = Some(match shown {
                s if s == from => to,
                s if s > from && s <= to => s - 1,
                s if s >= to && s < from => s + 1,
                s => s,
            });
            self.library_changed();
        }
        true
    }

    /// The Recent set's row, as a set's: its hourglass and its name. Returns its response and where
    /// it is.
    fn recent_row(&mut self, ui: &mut Ui, look: &mut Look, selected: bool) -> (egui::Response, Rect) {
        let width = ui.available_width();
        let (outer, response) = ui.allocate_exact_size(vec2(width, SET_HEIGHT), Sense::click());
        let rect = outer.shrink2(vec2(12.0, 0.0));
        ui.painter().rect_filled(rect, 12, panel::row_fill(selected, SET_CELL));
        let ink = if selected { Color32::WHITE } else { argb(PANEL_TITLE) };
        look.icon(ui, &icons::RECENT_SET, pos2(rect.left() + 24.0, rect.center().y), 40.0, ink);
        let name = one_line(ui, "Recent", 14.0, Weight::SemiBold, ink, rect.width() - 53.0);
        let left = rect.left() + 4.0 + 40.0 + 1.0;
        ui.painter().galley(pos2(left, rect.center().y - name.size().y / 2.0), name, ink);
        (response, rect)
    }

    /// A set: its icon and its name (two lines at most), or a field renaming it. Returns its
    /// response and where it is.
    #[allow(clippy::too_many_arguments)]
    fn set_row(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        session: &CanvasSession,
        i: usize,
        set: &BrushSetInfo,
        selected: bool,
        fill: Option<Color32>,
    ) -> (egui::Response, Rect) {
        let width = ui.available_width();
        let (outer, response) = ui.allocate_exact_size(vec2(width, SET_HEIGHT), Sense::click());
        let rect = outer.shrink2(vec2(12.0, 0.0));
        ui.painter().rect_filled(rect, 12, fill.unwrap_or_else(|| panel::row_fill(selected, SET_CELL)));
        let ink = if selected { Color32::WHITE } else { argb(PANEL_TITLE) };
        // a 40 frame centred 24 in
        let icon = icons::SET_ICONS.get(set.icon as usize).unwrap_or(&icons::BRUSH_SET);
        look.icon(ui, icon, pos2(rect.left() + 24.0, rect.center().y), 40.0, ink);
        let left = rect.left() + 4.0 + 40.0 + 1.0;
        if let Some((_, typed, focused)) = self.renaming_set.as_mut().filter(|(s, _, _)| *s == i) {
            let field = Rect::from_min_max(
                pos2(left, rect.center().y - 10.0),
                pos2(rect.right() - 8.0, rect.center().y + 10.0),
            );
            let mut output = egui::TextEdit::singleline(typed)
                .font(Weight::SemiBold.font(14.0))
                .text_color(ink)
                .frame(egui::Frame::NONE)
                .margin(vec2(0.0, 0.0))
                .desired_width(field.width())
                .show(&mut ui.new_child(UiBuilder::new().max_rect(field)));
            if !*focused {
                // the whole name chosen, to type over
                output.response.request_focus();
                let all = CCursorRange::two(CCursor::new(0), CCursor::new(typed.chars().count()));
                output.state.cursor.set_char_range(Some(all));
                output.state.store(ui.ctx(), output.response.id);
                *focused = true;
            } else if output.response.lost_focus() {
                let name = typed.clone();
                self.renaming_set = None;
                if session.rename_brush_set(i as u32, name) {
                    self.library_changed();
                }
            }
            return (response, rect);
        }
        let mut job = LayoutJob::default();
        job.append(&set.name, 0.0, format(14.0, Weight::SemiBold, ink));
        job.wrap = TextWrapping {
            max_width: rect.right() - 8.0 - left,
            max_rows: 2,
            break_anywhere: false,
            overflow_character: Some('…'),
        };
        let galley = ui.painter().layout_job(job);
        let at = Align2::LEFT_CENTER.anchor_size(pos2(left, rect.center().y), galley.size());
        ui.painter().galley(at.min, galley, ink);
        (response, rect)
    }

    /// A brush: its sample stroke filling the cell's height (its ends past the cell's sides), under
    /// its name; slid left, its buttons behind it. `pinned`: in the Recent set, whether it is pinned
    /// there (marked so). Returns whether it was tapped and which of its buttons was, and where its
    /// cell is.
    #[allow(clippy::too_many_arguments)]
    fn brush_row(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        b: &BrushInfo,
        selected: bool,
        pinned: Option<bool>,
    ) -> (Option<(bool, Option<Swiped>)>, Rect) {
        let row = ui.available_width();
        let (outer, _) = ui.allocate_exact_size(vec2(row, BRUSH_HEIGHT), Sense::hover());
        let cell = outer.shrink2(vec2(12.0, 0.0));
        if !ui.is_rect_visible(outer) {
            return (None, cell);
        }
        let key = (b.set, b.index);
        let slide = self.slid.get(&key).copied().unwrap_or(0.0);
        let at = cell.translate(vec2(slide, 0.0));
        let mut chose = None;
        if slide < 0.0 {
            chose = swipe_buttons(ui, look, cell, key, pinned);
        }
        // (only a click: a drag goes through to the list, which scrolls by itself)
        let response = ui.interact(at.intersect(cell), ui.scope_id().with(("brush", key)), Sense::click());
        if response.is_pointer_button_down_on()
            && self.press.is_none()
            && let Some(origin) = ui.input(|i| i.pointer.interact_pos())
        {
            let since = Instant::now();
            self.press = Some(Press {
                list: self.list_at,
                key,
                origin,
                since,
                way: Way::Undecided,
                start: slide,
                grab: origin - cell.min,
                last: origin,
            });
        }
        self.want_preview(ui, b, cell);
        let painter = ui.painter_at(cell);
        // the dragged brush is faint where it was
        let faint = self.press.is_some_and(|p| p.key == key && p.way == Way::Drag);
        let alpha = if faint { 0.3 } else { 1.0 };
        let picked = self.picked.contains(&key);
        if selected {
            painter.rect_filled(at, 12, argb(pc::ACCENT).gamma_multiply(alpha));
        } else if picked {
            painter.rect_filled(at, 12, argb(PICKED_BRUSH).gamma_multiply(alpha));
        }
        let mut faded = painter.clone();
        faded.multiply_opacity(alpha);
        let shaded = self.preview_in(&faded, key, at, selected);
        let ink = if selected || picked { Color32::WHITE } else { argb(PANEL_TITLE) }.gamma_multiply(alpha);
        let galley = one_line(ui, &b.name, 14.0, Weight::SemiBold, ink, at.width() - 18.0 - 10.0);
        let name_at = at.min + vec2(18.0, 11.75);
        if shaded {
            let pill = Rect::from_min_size(name_at, galley.size()).expand2(vec2(6.0, 2.0));
            painter.rect_filled(pill, pill.height() / 2.0, argb(NAME_PILL).gamma_multiply(alpha));
        }
        painter.galley(name_at, galley, ink);
        // a pinned brush's mark, at the cell's top right
        if pinned == Some(true) {
            let mut marked = ui.new_child(UiBuilder::new().max_rect(cell));
            marked.set_clip_rect(cell.intersect(ui.clip_rect()));
            look.icon(&marked, &icons::PIN_MARK, pos2(at.right() - 11.0 - 8.0, at.top() + 12.5 + 8.0), 16.0, ink);
        }
        let tapped = response.clicked() && self.press.is_none_or(|p| p.way == Way::Undecided);
        (Some((tapped, chose)), cell)
    }

    /// Asks for a brush's sample stroke when it is in sight and not drawn yet.
    fn want_preview(&mut self, ui: &Ui, b: &BrushInfo, cell: Rect) {
        let key = (b.set, b.index);
        if !self.previews.contains_key(&key) && ui.is_rect_visible(cell) {
            self.wanted.push((key, b.name.clone()));
        }
    }

    /// Takes the sample strokes drawn so far, and has those wanted drawn on a thread of their own.
    fn draw_previews(&mut self, ctx: &egui::Context, session: &Arc<CanvasSession>) {
        if let Some(drawing) = &self.drawing {
            loop {
                match drawing.try_recv() {
                    Ok((key, drawn)) => {
                        let preview = drawn.map(|(image, shaded)| {
                            let texture = ctx.load_texture(format!("brush {key:?}"), image, TextureOptions::LINEAR);
                            Preview { texture, shaded }
                        });
                        self.previews.insert(key, preview);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.drawing = None;
                        break;
                    }
                }
            }
        }
        let previews = &self.previews;
        self.wanted.retain(|(key, _)| !previews.contains_key(key));
        if self.drawing.is_none() && !self.wanted.is_empty() {
            let (done, drawing) = std::sync::mpsc::channel();
            let batch: Vec<_> = self.wanted.drain(..self.wanted.len().min(PREVIEWS_AT_A_TIME)).collect();
            let session = Arc::clone(session);
            let started = std::thread::Builder::new().name("brush previews".into()).spawn(move || {
                for (key, name) in batch {
                    let drawn = session.brush_preview(key.0, key.1, PREVIEW.0, PREVIEW.1).map(|t| {
                        let shaded = under_name(&t.rgba, &name);
                        let size = [t.width as usize, t.height as usize];
                        (egui::ColorImage::from_rgba_premultiplied(size, &t.rgba), shaded)
                    });
                    // (the library changed meanwhile: these are no longer wanted)
                    if done.send((key, drawn)).is_err() {
                        return;
                    }
                }
            });
            match started {
                Ok(_) => self.drawing = Some(drawing),
                Err(e) => log::warn!("the brushes' sample strokes aren't drawn: {e}"),
            }
        }
        if self.drawing.is_some() {
            ctx.request_repaint_after(PREVIEW_LOOK);
        }
    }

    /// The search field in `block` (shown where it overlaps `clip`): a magnifier, what is typed
    /// (or "Search"), and once typed, a button that clears it. Enter, Escape or a press elsewhere
    /// leaves it.
    fn search_field(&mut self, ui: &mut Ui, block: Rect, clip: Rect) {
        let field = Rect::from_min_max(block.min + vec2(12.0, 10.0), pos2(block.right() - 12.0, block.top() + 50.0));
        let hint = argb(SEARCH_HINT);
        let mut inside = ui.new_child(UiBuilder::new().max_rect(field));
        inside.set_clip_rect(clip.intersect(field));
        let painter = inside.painter().clone();
        painter.rect_filled(field, 10, argb(SEARCH_FILL));
        // a magnifier: a ring and its handle
        let c = pos2(field.left() + 41.0 * 0.47, field.top() + 40.0 * 0.46);
        painter.circle_stroke(c, 7.0, egui::Stroke::new(2.0, hint));
        let d = 7.0 * std::f32::consts::FRAC_1_SQRT_2;
        painter.line_segment([c + vec2(d, d), c + vec2(d + 5.0, d + 5.0)], egui::Stroke::new(2.4, hint));
        let clear = (!self.query.is_empty())
            .then(|| Rect::from_center_size(pos2(field.right() - 5.0 - 9.5, field.center().y), vec2(19.0, 18.0)));
        let words = Rect::from_min_max(
            pos2(field.left() + 41.0, field.top()),
            pos2(clear.map_or(field.right() - 8.0, |r| r.left() - 6.0), field.bottom()),
        );
        if self.query.is_empty() {
            text(&painter, words.left_center(), Align2::LEFT_CENTER, "Search", 17.0, Weight::Regular, hint);
        }
        let edit = egui::TextEdit::singleline(&mut self.query)
            .frame(egui::Frame::NONE)
            .font(Weight::Regular.font(17.0))
            .text_color(Color32::WHITE)
            .align(Align2::LEFT_CENTER)
            .min_size(words.size())
            .desired_width(words.width())
            .show(&mut inside.new_child(UiBuilder::new().max_rect(words)));
        // taking or leaving the keys changes what shows: drawn again now
        if self.searching != edit.response.has_focus() {
            self.searching = edit.response.has_focus();
            ui.ctx().request_repaint();
        }
        if let Some(clear) = clear {
            // a filled circle with a cross through it
            painter.circle_filled(clear.center(), 9.0, hint);
            let k = 3.5;
            for s in [1.0, -1.0] {
                let (a, b) = (clear.center() + vec2(-k, -k * s), clear.center() + vec2(k, k * s));
                painter.line_segment([a, b], egui::Stroke::new(1.8, argb(CLEAR_CROSS)));
            }
            if inside.interact(clear, inside.scope_id().with("clear search"), Sense::click()).clicked() {
                self.query.clear();
            }
        }
    }

    /// The brushes of every set whose name holds the search, two to a row in `area`; returns the
    /// one tapped. None found (or nothing typed yet): "No results".
    fn search_results(
        &mut self,
        ui: &mut Ui,
        library: &[BrushSetInfo],
        current: (u32, u32),
        area: Rect,
    ) -> Option<(u32, u32)> {
        let query = self.query.trim().to_lowercase();
        let found: Vec<&BrushInfo> = if query.is_empty() {
            Vec::new()
        } else {
            library.iter().flat_map(|s| &s.brushes).filter(|b| b.name.to_lowercase().contains(&query)).collect()
        };
        if found.is_empty() {
            let at = pos2(area.center().x, area.top() + 24.0);
            text(ui.painter(), at, Align2::CENTER_TOP, "No results", 15.0, Weight::Medium, argb(PANEL_TITLE));
            return None;
        }
        let mut tapped = None;
        ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
            ScrollArea::vertical()
                .id_salt("library results")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.add_space(11.0);
                    let width = (area.width() - 12.0 * 2.0 - 6.0) / 2.0;
                    for pair in found.chunks(2) {
                        let (row, _) = ui.allocate_exact_size(vec2(area.width(), RESULT_HEIGHT), Sense::hover());
                        for (k, b) in pair.iter().enumerate() {
                            let tile = Rect::from_min_size(
                                pos2(row.left() + 12.0 + (width + 6.0) * k as f32, row.top()),
                                vec2(width, RESULT_HEIGHT),
                            );
                            let key = (b.set, b.index);
                            let selected = key == current;
                            self.want_preview(ui, b, tile);
                            let painter = ui.painter_at(tile);
                            painter.rect_filled(tile, 12, if selected { argb(pc::ACCENT) } else { argb(RESULT_FILL) });
                            let shaded = self.preview_in(&painter, key, tile, selected);
                            let ink = if selected { Color32::WHITE } else { argb(PANEL_TITLE) };
                            let galley = one_line(ui, &b.name, 14.0, Weight::SemiBold, ink, tile.width() - 14.0 - 10.0);
                            let name_at = tile.min + vec2(14.0, 11.0);
                            if shaded {
                                let pill = Rect::from_min_size(name_at, galley.size()).expand2(vec2(6.0, 2.0));
                                painter.rect_filled(pill, pill.height() / 2.0, argb(NAME_PILL));
                            }
                            painter.galley(name_at, galley, ink);
                            if ui.interact(tile, ui.scope_id().with(("result", key)), Sense::click()).clicked() {
                                tapped = Some(key);
                            }
                        }
                        ui.add_space(6.0);
                    }
                    ui.add_space(12.0 - 6.0);
                });
        });
        tapped
    }

    /// Draws brush `key`'s sample stroke filling `at`'s height (white on blue when `selected`,
    /// black on the light interface's rows); returns whether it runs under the name.
    fn preview_in(&self, painter: &egui::Painter, key: (u32, u32), at: Rect, selected: bool) -> bool {
        let Some(Some(p)) = self.previews.get(&key) else { return false };
        let (pw, ph) = (PREVIEW.0 as f32, PREVIEW.1 as f32);
        let k = (at.width() / pw).max(at.height() / ph);
        let shown = Rect::from_center_size(at.center(), vec2(pw * k, ph * k));
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let tint = if crate::look::light() && !selected { Color32::BLACK } else { Color32::WHITE };
        painter.image(p.texture.id(), shown, uv, tint);
        p.shaded
    }
}

/// The set icons to choose from, five across in `rect` (scrolled at first to put `own` a few rows
/// down, unless `scrolled` already): `own` blue. Returns the one tapped.
fn icon_grid(ui: &mut Ui, look: &mut Look, rect: Rect, own: usize, scrolled: bool) -> Option<usize> {
    let inner = rect.shrink(3.0);
    let cell_w = inner.width() / PICKER_COLUMNS as f32;
    let mut picked = None;
    ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
        let mut area = ScrollArea::vertical()
            .id_salt("set icons")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
        if !scrolled {
            // an icon that isn't one of these starts it at the top
            let first = if own < PICKED_SET_ICONS { (own / PICKER_COLUMNS).saturating_sub(3) } else { 0 };
            area = area.vertical_scroll_offset(first as f32 * cell_w);
        }
        area.show(ui, |ui| {
            let rows = PICKED_SET_ICONS.div_ceil(PICKER_COLUMNS);
            let (all, _) = ui.allocate_exact_size(vec2(inner.width(), rows as f32 * cell_w), Sense::hover());
            for k in 0..PICKED_SET_ICONS {
                let (col, row) = (k % PICKER_COLUMNS, k / PICKER_COLUMNS);
                let slot =
                    Rect::from_min_size(all.min + vec2(col as f32 * cell_w, row as f32 * cell_w), vec2(cell_w, cell_w));
                let cell = Rect::from_center_size(slot.center(), vec2(PICKER_CELL, PICKER_CELL));
                if k == own {
                    ui.painter().rect_filled(cell, 21, argb(pc::ACCENT));
                }
                if let Some(icon) = icons::SET_ICONS.get(k) {
                    look.icon(ui, icon, cell.center(), 40.0, Color32::WHITE);
                }
                if ui.interact(cell, ui.scope_id().with(("set icon", k)), Sense::click()).clicked() {
                    picked = Some(k);
                }
            }
        });
    });
    picked
}

/// What a slid brush's buttons do.
#[derive(Clone, Copy)]
enum Swiped {
    Share,
    Duplicate,
    Delete,
    Find,
    Pin,
    Unpin,
    Clear,
}

/// A slid brush's buttons, filling its cell: Share, Duplicate and Delete; in the Recent set
/// (`pinned` there or not) Find, Pin or Unpin, and Clear. Returns the one tapped.
fn swipe_buttons(ui: &Ui, look: &mut Look, cell: Rect, key: (u32, u32), pinned: Option<bool>) -> Option<Swiped> {
    let w = (cell.width() - SWIPE_GAP * 2.0) / 3.0;
    let buttons = match pinned {
        None => [
            (&icons::SWIPE_SHARE, "Share", SWIPE_FILL, SWIPE_INK, Swiped::Share),
            (&icons::SWIPE_DUPLICATE, "Duplicate", SWIPE_FILL, SWIPE_INK, Swiped::Duplicate),
            (&icons::SWIPE_TRASH, "Delete", SWIPE_DELETE, 0xFFFFFFFF, Swiped::Delete),
        ],
        Some(pinned) => [
            (&icons::SWIPE_FIND, "Find", SWIPE_FILL, SWIPE_INK, Swiped::Find),
            if pinned {
                (&icons::SWIPE_UNPIN, "Unpin", SWIPE_FILL, SWIPE_INK, Swiped::Unpin)
            } else {
                (&icons::SWIPE_PIN, "Pin", SWIPE_FILL, SWIPE_INK, Swiped::Pin)
            },
            (&icons::SWIPE_CLEAR, "Clear", SWIPE_FILL, SWIPE_INK, Swiped::Clear),
        ],
    };
    let mut chose = None;
    for (i, (icon, label, fill, ink, does)) in buttons.into_iter().enumerate() {
        let rect =
            Rect::from_min_size(pos2(cell.left() + (w + SWIPE_GAP) * i as f32, cell.top()), vec2(w, cell.height()));
        ui.painter().rect_filled(rect, 16, argb(fill));
        // 19 across, the label under it, the two in the middle (5 down)
        let middle = rect.center().y + 2.5;
        look.icon(ui, icon, pos2(rect.center().x, middle - 9.5 - 1.0), 19.0, argb(ink));
        text(
            ui.painter(),
            pos2(rect.center().x, middle + 1.0),
            Align2::CENTER_TOP,
            label,
            14.0,
            Weight::Medium,
            argb(ink),
        );
        if ui.interact(rect, ui.scope_id().with(("swipe", key, i)), Sense::click()).clicked() {
            chose = Some(does);
        }
    }
    chose
}

fn format(size: f32, weight: Weight, colour: Color32) -> TextFormat {
    TextFormat { font_id: weight.font(size), color: colour, extra_letter_spacing: -0.045 * size, ..Default::default() }
}

/// Text on one line, cut short with an ellipsis past `max_width`.
fn one_line(ui: &Ui, text: &str, size: f32, weight: Weight, colour: Color32, max_width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, format(size, weight, colour));
    job.wrap = TextWrapping { max_width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.painter().layout_job(job)
}

/// Whether a sample stroke (`PREVIEW` big, premultiplied RGBA) runs under its name: the name's box
/// (19.5 in, 14 to 26 down, about 7.7 a letter wide, in a cell the picture fills 80 high) covered
/// more than 2/255 on average.
fn under_name(rgba: &[u8], name: &str) -> bool {
    let (w, h) = (PREVIEW.0 as usize, PREVIEW.1 as usize);
    let k = PREVIEW.1 as f32 / BRUSH_HEIGHT;
    let x0 = ((19.5 + 6.85) * k) as usize;
    let x1 = (((19.5 + 6.85 + 7.7 * name.chars().count() as f32) * k) as usize).min(w);
    let (y0, y1) = (((14.0 * k) as usize).min(h), ((26.0 * k) as usize).min(h));
    if x1 <= x0 || y1 <= y0 {
        return false;
    }
    let mut cover = 0u64;
    for y in y0..y1 {
        let line = rgba.get((y * w + x0) * 4..(y * w + x1) * 4).unwrap_or(&[]);
        cover += line.as_chunks::<4>().0.iter().map(|&[_, _, _, a]| u64::from(a)).sum::<u64>();
    }
    cover > 2 * ((x1 - x0) * (y1 - y0)) as u64
}

#[cfg(test)]
mod tests {
    use egui::{Event, PointerButton, RawInput, TouchDeviceId, TouchId, TouchPhase};

    use super::*;

    /// The library open with a finger on it, as the window's touches reach the interface.
    struct Finger {
        ctx: egui::Context,
        look: Look,
        library: Library,
        session: Arc<CanvasSession>,
        time: f64,
    }

    impl Finger {
        fn new() -> Finger {
            let session = CanvasSession::new(64, 64);
            let (ctx, look, library) = (egui::Context::default(), Look::default(), Library::default());
            let mut finger = Finger { ctx, look, library, session, time: 0.0 };
            // two frames to lay the list out and find it scrolls
            finger.frame(Vec::new());
            finger.frame(Vec::new());
            finger
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 1.0 / 60.0;
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 700.0))),
                time: Some(self.time),
                events,
                ..RawInput::default()
            };
            let Finger { ctx, look, library, session, .. } = self;
            let mut output = ctx.clone().run_ui(raw, |ui| {
                let button = Rect::from_min_size(pos2(600.0, 10.0), vec2(40.0, 40.0));
                library.show(ui.ctx(), look, session, ToolKind::Paint, button, &mut Vec::new());
            });
            output.textures_delta.clear();
        }

        fn touch(phase: TouchPhase, pos: Pos2) -> Event {
            let (device_id, id) = (TouchDeviceId(1), TouchId(1));
            Event::Touch { device_id, id, phase, pos, force: None }
        }

        fn down(&mut self, at: Pos2) {
            self.frame(vec![
                Self::touch(TouchPhase::Start, at),
                Event::PointerMoved(at),
                Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }

        fn to(&mut self, at: Pos2) {
            self.frame(vec![Self::touch(TouchPhase::Move, at), Event::PointerMoved(at)]);
        }

        fn up(&mut self, at: Pos2) {
            self.frame(vec![
                Self::touch(TouchPhase::End, at),
                Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
                Event::PointerGone,
            ]);
        }

        /// How far up the list has gone: where its first brush is.
        fn top(&self) -> f32 {
            self.library.brush_rects.first().map_or(0.0, |(_, r)| r.top())
        }

        /// A point on a brush in the middle of the list.
        fn on_a_brush(&self) -> Pos2 {
            let middle = self.library.brush_rects.len() / 3;
            self.library.brush_rects.get(middle).map_or(Pos2::ZERO, |(_, r)| r.center())
        }
    }

    #[test]
    fn the_brushes_keep_up_with_a_finger_and_carry_on_when_flicked() {
        let mut f = Finger::new();
        assert!(f.library.brush_rects.len() > 6, "too few brushes to scroll");
        let (start, from) = (f.top(), f.on_a_brush());
        f.down(from);
        // up the list, frame by frame: past a press's slop it moves as far as the finger does
        let mut at = from;
        for _ in 0..4 {
            at.y -= 5.0;
            f.to(at);
        }
        let (mid, finger) = (f.top(), at.y);
        for _ in 0..16 {
            at.y -= 5.0;
            f.to(at);
            let (moved, went) = (mid - f.top(), finger - at.y);
            assert!((moved - went).abs() < 0.01, "the list went {moved} as the finger went {went}");
        }
        assert!(start > f.top());
        // let go moving, it runs on
        f.up(at);
        let lifted = f.top();
        for _ in 0..10 {
            f.frame(Vec::new());
        }
        assert!(f.top() < lifted - 10.0, "stopped dead: {} to {}", lifted, f.top());
    }

    #[test]
    fn a_press_on_a_brush_holds_the_list_until_it_goes_up_or_down() {
        let mut f = Finger::new();
        let from = f.on_a_brush();
        f.down(from);
        let mut at = from;
        for _ in 0..12 {
            at.y -= 8.0;
            f.to(at);
        }
        f.up(at);
        f.frame(Vec::new());
        // still running on, a press stops it
        let press = f.on_a_brush();
        f.down(press);
        let held = f.top();
        for _ in 0..6 {
            f.frame(Vec::new());
        }
        assert!((f.top() - held).abs() < 0.01, "ran on from {held} to {}", f.top());
        // and however the pointer jumps while it is undecided or slid sideways, the list stays
        f.to(press + vec2(-30.0, 3.0));
        f.to(press + vec2(-60.0, 12.0));
        f.to(press + vec2(-90.0, -9.0));
        assert!((f.top() - held).abs() < 0.01, "moved from {held} to {}", f.top());
        f.up(press + vec2(-90.0, -9.0));
    }

    #[test]
    fn a_press_slides_a_brush_only_going_plainly_sideways() {
        let swipe = |x, y| way_moved(vec2(x, y)) == Some(Way::Swipe);
        let scroll = |x, y| way_moved(vec2(x, y)) == Some(Way::Scroll);
        // straight across, and a little off it, either way
        assert!(swipe(-14.0, 0.0) && swipe(14.0, 0.0) && swipe(-20.0, 9.0) && swipe(20.0, -9.0));
        // straight up or down, and a hand's arc across as far as along, or further
        assert!(scroll(0.0, 8.0) && scroll(0.0, -8.0) && scroll(-12.0, -10.0) && scroll(-19.0, 10.0));
        // too little to tell: it waits
        assert_eq!(way_moved(vec2(-13.0, 7.0)), None);
        assert_eq!(way_moved(vec2(-5.0, 5.0)), None);
    }

    #[test]
    fn a_hand_swinging_across_as_it_scrolls_moves_the_list_and_slides_no_brush() {
        let mut f = Finger::new();
        let (start, from) = (f.top(), f.on_a_brush());
        f.down(from);
        let mut at = from;
        // up and to the left, as a wrist turns: as far across as along at first
        for k in 0..16 {
            at += if k < 3 { vec2(-6.0, -5.0) } else { vec2(-3.0, -8.0) };
            f.to(at);
            assert!(f.library.slid.values().all(|s| *s == 0.0), "a brush slid on a scroll, step {k}");
        }
        f.up(at);
        assert!(f.top() < start - 50.0, "the list stayed: {} to {}", start, f.top());
        assert!(f.library.swiped.is_none(), "a brush opened on a scroll");
    }

    #[test]
    fn a_brush_slid_sideways_or_tapped_leaves_the_list_where_it_is() {
        let mut f = Finger::new();
        let (start, from) = (f.top(), f.on_a_brush());
        f.down(from);
        let mut at = from;
        for k in 0..10 {
            at += vec2(-8.0, if k % 2 == 0 { 1.0 } else { -1.0 });
            f.to(at);
        }
        f.up(at);
        for _ in 0..5 {
            f.frame(Vec::new());
        }
        assert!((f.top() - start).abs() < 3.0, "the list moved {}", f.top() - start);
        assert!(f.library.slid.values().any(|s| *s < 0.0) || f.library.swiped.is_some(), "the brush didn't slide");
    }
}
