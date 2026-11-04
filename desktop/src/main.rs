//! ProCraft on the desktop: the engine draws the canvas into the window, and the interface is
//! drawn over it with egui, on the same device.

// no console window behind the app on Windows (a debug build keeps it, for the log)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod adjust_panels;
mod adjustments;
mod alert;
mod animated_export;
mod animation;
mod canvas_info;
mod chrome;
mod colour_drop;
mod colour_models;
mod colours;
mod compact_colours;
mod companion;
mod copy_paste;
mod crop;
mod curve;
mod custom_canvas;
mod export_card;
mod files;
#[cfg(target_os = "macos")]
mod finder;
mod gallery;
mod gesture_controls;
mod gradient_map;
mod guide_editor;
mod icons;
mod input;
mod layer_choice;
mod layers;
mod library;
mod licences;
mod lighting;
mod liquify;
mod look;
mod menu;
mod overlays;
mod panel;
mod pdf;
mod pen;
mod prefs;
mod projection;
mod quick_menu;
mod recolor;
mod reference;
mod screen;
mod selection;
mod settings;
mod shortcuts;
mod sound;
mod startup;
mod source_editor;
mod studio;
mod text;
mod timelapse;
mod tool_header;
mod transform;
mod transform_numeric;
mod ui;
mod video;
mod zoom;

use std::sync::Arc;
use std::time::{Duration, Instant};

use procraft_core::CanvasSession;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::screen::Screen;
use crate::ui::{Action, Interface};

/// A new artwork's size when the screen's isn't known.
const FALLBACK_CANVAS: (u32, u32) = (1920, 1080);

/// A picture being made a new artwork: since when, the gallery folder it goes in, and its reading.
type ReadingPhoto = (Instant, Option<String>, std::thread::JoinHandle<Result<Arc<CanvasSession>, String>>);

/// An artwork in the gallery being opened: since when, its folder, the moving format it is
/// opened to be shared as (the gallery's Share), and its reading.
type ReadingArtwork =
    (Instant, String, Option<animated_export::Animated>, std::thread::JoinHandle<Result<Arc<CanvasSession>, String>>);

/// Artworks being brought in into a gallery folder: one read on a thread of its own (an artwork of
/// many layers takes seconds), since when, and those still to come.
struct Importing {
    since: Instant,
    stage: ImportStage,
    folder: Option<String>,
    rest: std::collections::VecDeque<Vec<std::path::PathBuf>>,
}

/// An artwork coming in: read, then (a package's) its time-lapse taken into its new folder, both
/// off the window's thread (a long time-lapse is hundreds of videos, every frame decoded).
enum ImportStage {
    Reading(std::thread::JoinHandle<Option<(Arc<CanvasSession>, Option<std::path::PathBuf>)>>),
    TakingTimeLapse(std::thread::JoinHandle<()>, Arc<CanvasSession>, String),
}

impl ImportStage {
    fn is_finished(&self) -> bool {
        match self {
            ImportStage::Reading(t) => t.is_finished(),
            ImportStage::TakingTimeLapse(t, ..) => t.is_finished(),
        }
    }
}

impl Importing {
    /// The first of `artworks` (each its files) read on its own thread, into `folder`; none left,
    /// none.
    fn next(
        mut artworks: std::collections::VecDeque<Vec<std::path::PathBuf>>,
        folder: Option<String>,
    ) -> Option<Importing> {
        let paths = artworks.pop_front()?;
        let reading = std::thread::spawn(move || files::open_artwork_files(paths));
        Some(Importing { since: Instant::now(), stage: ImportStage::Reading(reading), folder, rest: artworks })
    }
}

/// What the layers' textures may take on the GPU, bytes: as much as the layers may (2 GiB).
const LAYERS_ON_GPU: u64 = 2 << 30;

/// How long an artwork opens before it says so, as the tablet's.
const SLOW_OPENING: Duration = Duration::from_millis(500);

/// A window event (a frame among them) that takes longer than this is logged: the window waits on
/// it.
const SLOW_EVENT: Duration = Duration::from_millis(100);

/// Logs, once dropped, a window event that took longer than [`SLOW_EVENT`] (a dialog it waited on
/// counts too): what holds the window up shows in the log.
struct Timed(&'static str, Instant);

impl Drop for Timed {
    fn drop(&mut self) {
        let took = self.1.elapsed();
        if took > SLOW_EVENT {
            log::warn!("{} took {} ms", self.0, took.as_millis());
        }
    }
}
/// The veil over the window while it does.
const OPENING_VEIL: u32 = 0x66000000;

/// Over the window while a slow artwork opens: veiled, saying so (and taking the presses).
fn opening_veil(ctx: &egui::Context) {
    veil(ctx, "Opening…");
}

/// Over the window while something slow is under way: veiled, saying `what` (and taking the
/// presses).
fn veil(ctx: &egui::Context, what: &str) {
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::unique("opening")).fixed_pos(screen.min).order(egui::Order::Foreground).show(ctx, |ui| {
        ui.allocate_rect(screen, egui::Sense::click_and_drag());
        ui.painter().rect_filled(screen, 0, look::argb(OPENING_VEIL));
        let ink = look::argb(look::pc::TEXT);
        look::text(ui.painter(), screen.center(), egui::Align2::CENTER_CENTER, what, 17.0, look::Weight::Regular, ink);
    });
}

fn main() {
    // the log, to the app folder's file too, and a crash's message into it
    startup::init();
    // the layers' textures may take what the layers themselves may (the engine's budget): none is
    // ever kept off the GPU, but the composite is made around the layers not being worked on, so
    // a frame composites only those (as the tablet's)
    procraft_core::set_resident_budget(LAYERS_ON_GPU);
    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            log::error!("no event loop: {e}");
            return;
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    // a Mac hands the files opened from the Finder to the app as it runs, not in its arguments
    #[cfg(target_os = "macos")]
    finder::watch();
    // the colour profiles imported before
    procraft_core::set_profile_folder(files::app_dir().join("profiles").to_string_lossy().into_owned());
    let mut app = App {
        wake: event_loop.create_proxy(),
        started: Instant::now(),
        last_frame: Instant::now(),
        again: None,
        opening: None,
        importing: None,
        reading_artwork: None,
        share_once_made: None,
        reading_photo: None,
        screen: None,
        session: None,
        artwork: None,
        gallery: gallery::Gallery::new(),
        ui: Interface::default(),
        input: input::Input::default(),
        title: String::new(),
        zoom: None,
        projection: None,
        displays_seen: None,
        // files the app was opened with ("Open with", or dropped on its icon) come in as a drop on
        // the gallery does
        dropped: std::env::args_os().skip(1).map(std::path::PathBuf::from).filter(|p| p.is_file()).collect(),
        tap_held: None,
        closed: false,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("event loop: {e}");
    }
}

struct App {
    /// Wakes the loop from other threads (the pen's, on Wayland).
    wake: winit::event_loop::EventLoopProxy<()>,
    started: Instant,
    /// When the last frame was drawn, and when another is wanted (the canvas or the interface still
    /// moving, or a hold to be timed).
    last_frame: Instant,
    again: Option<Instant>,
    /// An artwork's canvas being made on a thread of its own as it opens, and since when.
    opening: Option<(Instant, std::thread::JoinHandle<()>)>,
    /// Artworks being brought in, read on a thread of their own one after another.
    importing: Option<Importing>,
    /// An artwork in the gallery being opened, read on a thread of its own (an artwork of many
    /// layers holds hundreds of MB): since when, its folder, and the reading.
    reading_artwork: Option<ReadingArtwork>,
    /// The gallery's animated Share of the artwork just opened, started once its canvas is made
    /// (the preview's frames and the file are read from it).
    share_once_made: Option<animated_export::Animated>,
    /// A picture made a new artwork (the gallery's Photo), read on a thread of its own: since
    /// when, the gallery folder it goes in, and the reading.
    reading_photo: Option<ReadingPhoto>,
    screen: Option<Screen>,
    /// The artwork open and its folder in the gallery; none while the gallery shows.
    session: Option<Arc<CanvasSession>>,
    artwork: Option<String>,
    gallery: gallery::Gallery,
    ui: Interface,
    input: input::Input,
    /// The window's title as last set.
    title: String,
    /// The gallery zooming into the artwork opening, or out of the one closing.
    zoom: Option<zoom::Zoom>,
    /// Project canvas's window on a second display, and when the displays were last looked at.
    projection: Option<projection::Projection>,
    displays_seen: Option<Instant>,
    /// Files dropped on the window (or the app opened with), taken in together at the next frame
    /// (a model with its material file and images comes in one drop).
    dropped: Vec<std::path::PathBuf>,
    /// Tap Assistance: a lone finger's touch, where it landed and when, while it is held there.
    tap_held: Option<(u64, winit::dpi::PhysicalPosition<f64>, Instant)>,
    /// The window was closed (the artwork open kept): the app's end needs nothing more.
    closed: bool,
}

/// The app's icon, for the window's title bar and the task bar.
fn window_icon() -> Option<winit::window::Icon> {
    let picture = image::load_from_memory(include_bytes!("../../assets/app-icon/procraft-256.png")).ok()?;
    let rgba = picture.into_rgba8();
    let (width, height) = rgba.dimensions();
    winit::window::Icon::from_rgba(rgba.into_raw(), width, height).ok()
}

/// Tap Assistance holds a finger's touch where it landed this long.
const TAP_HOLD: std::time::Duration = std::time::Duration::from_millis(300);
/// How far it may drift meanwhile and still be held (points): farther, it is a scroll or a drag.
const TAP_DRIFT: f64 = 24.0;

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.screen.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("ProCraft")
            .with_window_icon(window_icon())
            .with_inner_size(LogicalSize::new(1440.0, 900.0))
            .with_min_inner_size(LogicalSize::new(640.0, 480.0))
            // opened for a test (PROCRAFT_BACKGROUND set), it leaves the keyboard where it was
            .with_active(std::env::var_os("PROCRAFT_BACKGROUND").is_none());
        // on Linux the desktop's menus match the window to the app's entry (procraft.desktop)
        #[cfg(target_os = "linux")]
        let attributes = {
            use winit::platform::wayland::WindowAttributesExtWayland;
            use winit::platform::x11::WindowAttributesExtX11;
            let attributes = WindowAttributesExtWayland::with_name(attributes, "procraft", "procraft");
            WindowAttributesExtX11::with_name(attributes, "procraft", "ProCraft")
        };
        let window = match event_loop.create_window(attributes) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("no window: {e}");
                event_loop.exit();
                return;
            }
        };
        // a pen's lean, which winit's touches leave out (on Wayland the whole pen)
        pen::watch(&window, self.wake.clone());
        // from the backend the last run used, or the next if that run never ended
        let screen = match Screen::new(window, startup::gpu_start(procraft_gpu::Gpu::backend_count())) {
            Ok((s, backend)) => {
                startup::gpu_running(backend);
                s
            }
            Err(e) => {
                log::error!("no GPU for the window: {e}");
                files::tell("ProCraft can't draw", &format!("No GPU device for the window: {e}"));
                event_loop.exit();
                return;
            }
        };
        // Screen Size's artwork is the screen's size
        self.gallery.screen_px = screen
            .window
            .current_monitor()
            .map(|m| m.size())
            .filter(|s| s.width > 0 && s.height > 0)
            .map_or(FALLBACK_CANVAS, |s| (s.width, s.height));
        // the app opens on the gallery
        screen.window.request_redraw();
        self.screen = Some(screen);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let _timed =
            Timed(if event == WindowEvent::RedrawRequested { "a frame" } else { "a window event" }, Instant::now());
        if self.projection.as_ref().is_some_and(|p| p.window.id() == id) {
            self.projection_event(event);
            return;
        }
        let Some(event) = self.tap_assist(event) else { return };
        let Some(screen) = self.screen.as_mut() else { return };
        let session = self.session.clone();
        // on the canvas Ctrl+X, C and V are the app's shortcuts (a layer cut, copied or pasted,
        // from a clipboard of its own, the system's perhaps empty); a text field keeps them for
        // the system's clipboard
        let canvas_keys = session.is_some() && !screen.egui.egui_ctx().egui_wants_keyboard_input();
        screen.egui.set_clipboard_shortcuts(!canvas_keys);
        let response = screen.egui.on_window_event(&screen.window, &event);
        // egui counts every redraw as wanting another; the frames it does want come from its
        // repaint delay, paced in `about_to_wait`
        if response.repaint && event != WindowEvent::RedrawRequested {
            screen.window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => {
                // the artwork open is kept in the gallery, and files still being written finish
                if self.leave_artwork() {
                    files::finish_writing();
                    files::tell_what_was_told();
                    self.closed = true;
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                screen.resize(size.width, size.height);
                if let Some(session) = &session {
                    session.resize_window(size.width, size.height);
                }
                screen.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(session) = &session {
                    session.set_display_scale(scale_factor as f32);
                    self.ui.place_canvas(session, scale_factor as f32);
                }
            }
            WindowEvent::RedrawRequested => self.frame(),
            WindowEvent::DroppedFile(path) => {
                self.dropped.push(path);
                screen.window.request_redraw();
            }
            event => {
                let Some(session) = session else { return };
                // with a popover open, a press on the canvas only closes it (a brush library alone
                // closes and paints); while the artwork is being exported none reaches it
                // (nothing the export would end is started)
                let blocked =
                    (self.ui.popover_open && !self.ui.library_lets_paint) || files::exporting_since().is_some();
                let canvas = self.ui.canvas_rect;
                let over = |p: (f32, f32)| blocked || screen.over_interface(p, canvas);
                if self.input.on_event(&session, &event, response.consumed, over) {
                    screen.window.request_redraw();
                }
            }
        }
    }

    /// A pen read by the app itself moved: its touches and hovering, as winit's would come.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
        let Some(window) = self.screen.as_ref().map(|s| (s.window.id(), s.window.scale_factor())) else { return };
        for event in pen::take_events(window.1) {
            self.window_event(event_loop, window.0, event);
        }
    }

    /// The app ending without its window closed first (a Mac's Quit, from the app menu or
    /// Cmd+Q, ends it with no close asked): the artwork open kept as closing the window keeps it.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // the backend did its run: the next starts on it again
        startup::gpu_ended();
        // before winit closes the connection the pen is read on
        pen::stop();
        if std::mem::replace(&mut self.closed, true) {
            return;
        }
        if !self.leave_artwork() {
            log::error!("the artwork open couldn't be kept as the app ended");
        }
        files::finish_writing();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // files opened from the Finder meanwhile come in as files dropped on the window do
        #[cfg(target_os = "macos")]
        {
            let opened = finder::take();
            if !opened.is_empty() {
                self.dropped.extend(opened);
                if let Some(screen) = &self.screen {
                    screen.window.request_redraw();
                }
            }
        }
        if self.screen.as_ref().is_some_and(|s| s.gpu.is_lost()) {
            self.gpu_lost(event_loop);
            return;
        }
        // what the files written meanwhile had to say; looked at again soon while any is writing
        files::tell_what_was_told();
        if files::still_writing() {
            let soon = Instant::now() + Duration::from_millis(250);
            self.again = Some(self.again.map_or(soon, |due| due.min(soon)));
        }
        self.sync_projection(event_loop);
        let Some(screen) = self.screen.as_ref() else { return };
        let Some(session) = self.session.as_ref() else {
            // the gallery: only the interface's own wants
            match self.again {
                Some(due) if Instant::now() >= due => screen.window.request_redraw(),
                Some(due) => {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(due));
                    return;
                }
                None => {}
            }
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };
        let mut deadline = self.input.next_deadline(session);
        // the touches' waits (a hold, a repeat) run out
        if self.input.tick(session) || !self.input.commands.is_empty() {
            screen.window.request_redraw();
        } else if let Some(due) = self.again.or_else(|| session.wants_frame().then(Instant::now)) {
            // what moves by itself (the selection's bands, a fade) draws once a refresh, not as
            // fast as frames can be made
            let due = due.max(self.last_frame + frame_gap(&screen.window));
            if Instant::now() >= due {
                screen.window.request_redraw();
            } else {
                deadline = Some(deadline.map_or(due, |d| d.min(due)));
            }
        }
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

impl App {
    /// Shows `session` in the window, in place of the artwork open.
    fn show(&mut self, session: Arc<CanvasSession>) {
        let Some(screen) = self.screen.as_mut() else { return };
        let size = screen.window.inner_size();
        let scale = screen.window.scale_factor() as f32;
        session.set_display_scale(scale);
        // its layers go up on a thread of their own (an artwork of many takes seconds), the window
        // drawing meanwhile
        let (gpu, opened) = (screen.gpu.clone(), session.clone());
        self.opening = Some((Instant::now(), std::thread::spawn(move || opened.use_gpu(gpu))));
        // the brushes as the last artwork left them
        if let Some(last) = &self.session {
            last.keep_brushes(files::brushes_dir());
        }
        session.open_brushes(files::brushes_dir());
        self.ui.opened(&session, scale);
        session.show_in(size.width, size.height);
        self.input = input::Input::default();
        self.session = Some(session);
        screen.window.request_redraw();
    }

    /// The brush studio's drawing pad: its canvas made when the studio opens (the size the pad
    /// shows at, on this device), drawn into its picture each frame, and let go when it closes.
    /// True while the pad wants another frame.
    fn studio_pad(&mut self, session: &Arc<CanvasSession>) -> bool {
        let Some(screen) = self.screen.as_mut() else { return false };
        let studio = &mut self.ui.studio;
        if !studio.open {
            // closing the studio lets its texture go first: the picture goes whether or not
            studio.pad_texture = None;
            screen.drop_pad();
            return false;
        }
        let ppp = screen.egui.egui_ctx().pixels_per_point();
        let Some(rect) = studio.pad_rect else { return true };
        let size = ((rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32);
        if size.0 < 2 || size.1 < 2 {
            return true;
        }
        if studio.pad.is_none() {
            // a new pad is shown afresh: the picture an earlier one was shown in (the studio closed
            // without it being let go) would be drawn into without the new pad ever being told its
            // size and view, and the pad would take no strokes
            screen.drop_pad();
            let pad = CanvasSession::new(size.0, size.1);
            pad.use_gpu(screen.gpu.clone());
            pad.set_display_scale(ppp);
            studio.start_pad(pad, session);
        }
        let Some(pad) = studio.pad.clone() else { return false };
        studio.pad_texture = Some(screen.draw_pad(&pad, size, self.started));
        pad.wants_frame()
    }

    /// Runs the interface, draws the canvas and the interface over it, and does what was asked.
    fn frame(&mut self) {
        if self.screen.is_none() {
            return;
        }
        self.take_dropped();
        self.imported();
        self.artwork_read();
        self.photo_read();
        let Some(screen) = self.screen.as_mut() else { return };
        let Some(session) = self.session.clone() else {
            self.gallery_frame();
            return;
        };
        self.last_frame = Instant::now();
        // the canvas made: drawn from now on; a slow one says it is opening
        if self.opening.as_ref().is_some_and(|(_, made)| made.is_finished())
            && let Some((_, made)) = self.opening.take()
        {
            let _ = made.join();
        }
        // the gallery's animated Share, now there is a canvas to read the frames from
        if self.opening.is_none()
            && let Some(format) = self.share_once_made.take()
        {
            self.ui.share_animated_from_gallery(format);
        }
        let slow = self.opening.as_ref().is_some_and(|(since, _)| since.elapsed() >= SLOW_OPENING)
            || self.importing.as_ref().is_some_and(|i| i.since.elapsed() >= SLOW_OPENING);
        // the gallery stays as it is until there is a canvas to zoom into
        if self.opening.is_some()
            && let Some(zoom) = self.zoom.as_mut().filter(|z| z.opening)
        {
            zoom.hold();
        }
        // an export shows its own card; the veil is for a slow save
        let exporting = !self.ui.export_card.busy()
            && files::exporting_since().is_some_and(|since| since.elapsed() >= SLOW_OPENING);
        let raw = screen.egui.take_egui_input(&screen.window);
        let output = screen.egui.egui_ctx().clone().run_ui(raw, |ui| {
            self.ui.show(ui, &session, &mut self.input);
            // opening: the gallery over it, growing from the artwork's picture
            if let Some(zoom) = self.zoom.as_ref().filter(|z| z.opening) {
                zoom.over_canvas(ui.ctx(), &mut self.gallery, &mut self.ui.look);
            }
            if slow {
                opening_veil(ui.ctx());
            } else if exporting {
                veil(ui.ctx(), "Exporting…");
            }
        });
        let pad_soon = self.studio_pad(&session);
        let Some(screen) = self.screen.as_mut() else { return };
        self.again = screen.draw(Some(&session), self.started, output).map(|after| Instant::now() + after);
        if pad_soon {
            self.again = Some(Instant::now());
        }
        // looked at again soon while the canvas is being made, an artwork read or saved as
        if self.ui.saved_as() || self.opening.is_some() || self.importing.is_some() {
            let soon = Instant::now() + Duration::from_millis(50);
            self.again = Some(self.again.map_or(soon, |due| due.min(soon)));
        }
        let edits = session.edits();
        let saved = edits == self.ui.saved_edits || session.edits_saved() == Some(edits);
        let unsaved = if saved { "" } else { " •" };
        let title = format!("{}{unsaved} — ProCraft", session.name());
        if title != self.title {
            screen.window.set_title(&title);
            self.title = title;
        }
        // looked at again until a save has it (autosaves come in on their own)
        if !saved {
            let soon = Instant::now() + Duration::from_secs(1);
            self.again = Some(self.again.map_or(soon, |due| due.min(soon)));
        }
        self.zoom_on();
        // Project canvas shows the canvas as just drawn
        if let (Some(p), Some(screen)) = (self.projection.as_mut(), &self.screen) {
            p.draw(&screen.gpu, Some(&session));
        }
        if std::mem::take(&mut self.ui.keep_brushes) {
            session.keep_brushes(files::brushes_dir());
        }
        for action in std::mem::take(&mut self.ui.actions) {
            self.act(action, &session);
        }
    }

    /// The gallery over the whole window, and what a tap in it asked for.
    fn gallery_frame(&mut self) {
        let Some(screen) = self.screen.as_mut() else { return };
        self.last_frame = Instant::now();
        let raw = screen.egui.take_egui_input(&screen.window);
        let mut asked = None;
        let (gallery, interface) = (&mut self.gallery, &mut self.ui);
        let closing = self.zoom.as_ref().filter(|z| !z.opening);
        let reading = match (&self.importing, &self.reading_artwork, &self.reading_photo) {
            (None, None, None) => None,
            (importing, opening, photo) => Some(
                importing.as_ref().is_some_and(|i| i.since.elapsed() >= SLOW_OPENING)
                    || opening.as_ref().is_some_and(|(since, ..)| since.elapsed() >= SLOW_OPENING)
                    || photo.as_ref().is_some_and(|(since, ..)| since.elapsed() >= SLOW_OPENING),
            ),
        };
        let output = screen.egui.egui_ctx().clone().run_ui(raw, |ui| {
            interface.style_once(ui.ctx());
            // closing: the gallery settling back into place, the canvas fading over it
            // otherwise in its place (a closing zoom cut short leaves nothing behind)
            match closing {
                Some(zoom) => zoom.over_gallery(ui.ctx()),
                None => zoom::Zoom::end(ui.ctx()),
            }
            asked = gallery.show(ui.ctx(), &mut interface.look, &interface.colours);
            // a share from the gallery shows its card there
            interface.export_card.show(ui.ctx(), &mut interface.look);
            if reading == Some(true) {
                opening_veil(ui.ctx());
            }
        });
        self.again = screen.draw(None, self.started, output).map(|after| Instant::now() + after);
        // looked at again soon while an artwork is read
        if reading.is_some() {
            let soon = Instant::now() + Duration::from_millis(50);
            self.again = Some(self.again.map_or(soon, |due| due.min(soon)));
        }
        if self.title != "ProCraft" {
            screen.window.set_title("ProCraft");
            self.title = "ProCraft".into();
        }
        self.zoom_on();
        // Project canvas is black while the gallery shows
        if let (Some(p), Some(screen)) = (self.projection.as_mut(), &self.screen) {
            p.draw(&screen.gpu, None);
        }
        // while an artwork, a photo or an import is being read, the first tap stands: another
        // artwork, a new one or a photo isn't opened over it
        let busy = self.reading_artwork.is_some() || self.reading_photo.is_some() || self.importing.is_some();
        match asked {
            // read on a thread of its own; one at a time
            Some(gallery::Asked::Open(dir)) => {
                if !busy {
                    self.read_artwork(dir, None);
                }
            }
            Some(gallery::Asked::New(preset, folder)) => {
                if !busy {
                    let session = files::new_from_preset(&preset);
                    let dir = self.gallery.store.new_folder(folder);
                    self.open_artwork(session, dir);
                    self.ui.started_new();
                }
            }
            Some(gallery::Asked::ScreenSize(folder)) => {
                if !busy {
                    let (w, h) = self.gallery.screen_px;
                    let session = CanvasSession::new(w, h);
                    session.set_new_profile(procraft_core::P3.into());
                    let dir = self.gallery.store.new_folder(folder);
                    self.open_artwork(session, dir);
                    self.ui.started_new();
                }
            }
            Some(gallery::Asked::Share(list, format)) => {
                files::share_artworks(&list, format, 100, &mut self.ui.export_card);
            }
            Some(gallery::Asked::SharePdf(list, quality)) => {
                files::share_artworks(&list, files::Format::Pdf, quality, &mut self.ui.export_card);
            }
            Some(gallery::Asked::DragOut(artwork)) => self.drag_out(&artwork),
            // opened (read as Open reads it), then shared
            Some(gallery::Asked::ShareAnimated(artwork, format)) => {
                if !busy {
                    self.read_artwork(artwork.dir, Some(format));
                }
            }
            // read on a thread of its own (a large photo takes a while); one at a time
            Some(gallery::Asked::Photo(folder)) => {
                if !busy && let Some(path) = files::pick_picture() {
                    let reading = std::thread::spawn(move || files::picture_artwork(&path));
                    self.reading_photo = Some((Instant::now(), folder, reading));
                }
            }
            Some(gallery::Asked::Import(folder)) => {
                if let Some(paths) = files::pick_import() {
                    self.import(paths, folder);
                }
            }
            None => {}
        }
    }

    /// Shows `session`, kept in the gallery's folder `dir`.
    /// Project canvas: its window opened on a second display while it is on and there is one
    /// (the displays looked at once a second), closed otherwise.
    fn sync_projection(&mut self, event_loop: &ActiveEventLoop) {
        let Some(screen) = &self.screen else { return };
        if !self.ui.project_canvas() {
            self.projection = None;
            return;
        }
        if self.displays_seen.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
            return;
        }
        self.displays_seen = Some(Instant::now());
        let second = projection::second_display(event_loop, &screen.window);
        // gone, or the main window moved onto its display: opened again where it now goes
        if self.projection.as_ref().is_some_and(|p| Some(&p.monitor) != second.as_ref()) {
            self.projection = None;
        }
        if self.projection.is_none()
            && let Some(monitor) = second
        {
            self.projection = projection::Projection::open(event_loop, monitor, &screen.gpu);
            if let Some(p) = &self.projection {
                p.window.request_redraw();
            }
        }
    }

    /// An event of Project canvas's window.
    fn projection_event(&mut self, event: WindowEvent) {
        let (Some(screen), Some(projection)) = (&self.screen, self.projection.as_mut()) else { return };
        match event {
            WindowEvent::Resized(size) => {
                projection.resize(&screen.gpu, size.width, size.height);
                projection.window.request_redraw();
            }
            WindowEvent::RedrawRequested => projection.draw(&screen.gpu, self.session.as_deref()),
            // closed there: Project canvas goes off
            WindowEvent::CloseRequested => {
                self.projection = None;
                self.ui.stop_projecting();
            }
            _ => {}
        }
    }

    /// While the gallery zooms, another frame soon; once done, everything back in its place.
    fn zoom_on(&mut self) {
        let Some(zoom) = &self.zoom else { return };
        if zoom.done() {
            self.zoom = None;
            if let Some(screen) = &self.screen {
                zoom::Zoom::end(screen.egui.egui_ctx());
                screen.window.request_redraw();
            }
        } else {
            self.again = Some(Instant::now());
        }
    }

    fn open_artwork(&mut self, session: Arc<CanvasSession>, dir: String) {
        self.ui.artwork_dir = Some(dir.clone());
        // kept saved as it changes, as the tablet's
        Arc::clone(&session).start_autosave(dir.clone());
        self.artwork = Some(dir);
        self.ui.path = None;
        self.ui.saved_edits = session.edits();
        self.show(session);
    }

    /// Tap Assistance: for its first moments a lone finger's touch stays where it landed (its
    /// moves held back, its lift put back there), so a tap that drifts still lands where it began.
    /// More fingers, a pen, a touch held longer, or one gone farther than a drift (a list scrolled)
    /// go on as they are. None: the event is held back.
    fn tap_assist(&mut self, event: WindowEvent) -> Option<WindowEvent> {
        let WindowEvent::Touch(touch) = &event else { return Some(event) };
        if !self.ui.tap_assistance() || touch.force.is_some() {
            return Some(event);
        }
        let held = self.tap_held.filter(|(id, _, _)| *id == touch.id);
        let drift = TAP_DRIFT * self.screen.as_ref().map_or(1.0, |s| s.window.scale_factor());
        match touch.phase {
            // a second finger lets the first go on as it is
            winit::event::TouchPhase::Started if self.tap_held.is_some() => self.tap_held = None,
            winit::event::TouchPhase::Started => self.tap_held = Some((touch.id, touch.location, Instant::now())),
            winit::event::TouchPhase::Moved => match held {
                Some((_, at, since))
                    if since.elapsed() <= TAP_HOLD
                        && (touch.location.x - at.x).hypot(touch.location.y - at.y) <= drift =>
                {
                    return None;
                }
                Some(_) => self.tap_held = None,
                None => {}
            },
            winit::event::TouchPhase::Ended => {
                if let Some((_, at, _)) = held {
                    self.tap_held = None;
                    return Some(WindowEvent::Touch(winit::event::Touch { location: at, ..*touch }));
                }
            }
            winit::event::TouchPhase::Cancelled => self.tap_held = None,
        }
        Some(event)
    }

    /// Files dropped on the window: on the canvas its pictures are inserted; on the gallery they
    /// come in as Import brings them (an artwork, a picture, or a model with its files).
    fn take_dropped(&mut self) {
        let paths = std::mem::take(&mut self.dropped);
        if paths.is_empty() {
            return;
        }
        if let Some(session) = self.session.clone() {
            self.ui.dropped(&session, &paths);
            return;
        }
        self.import(paths, self.gallery.folder());
    }

    /// Files brought into the gallery (picked with Import, or dropped on it) into `folder`:
    /// palettes join the colour panel's, brushes wait for a canvas to open, and the rest come in as
    /// an artwork (a package, a PSD, a PDF, a picture, or a model with its files).
    fn import(&mut self, paths: Vec<std::path::PathBuf>, folder: Option<String>) {
        let (palettes, paths): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| files::is_palette_file(p));
        if !palettes.is_empty() {
            let store = &self.ui.colours.palettes;
            let added = palettes.iter().filter(|p| {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
                std::fs::read(p).is_ok_and(|bytes| store.import_file(bytes, name))
            });
            files::say(if added.count() > 0 { "Palette added." } else { "This palette can't be read." });
        }
        let (brushes, paths): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| files::is_brush_file(p));
        if !brushes.is_empty() {
            files::keep_pending_brushes(&brushes);
        }
        // a model takes the files picked with it; else each is an artwork of its own, the one
        // before kept (closed) as the next comes in
        let artworks: std::collections::VecDeque<Vec<std::path::PathBuf>> =
            if paths.iter().any(|p| files::is_model_file(p)) {
                [paths].into()
            } else {
                paths.into_iter().map(|p| vec![p]).collect()
            };
        // after any already coming in
        match &mut self.importing {
            Some(importing) => importing.rest.extend(artworks),
            None => self.importing = Importing::next(artworks, folder),
        }
    }

    /// Starts reading the artwork in gallery folder `dir` on a thread of its own, to open it (to
    /// share it as `shared_as`, if given).
    fn read_artwork(&mut self, dir: String, shared_as: Option<animated_export::Animated>) {
        let folder = dir.clone();
        let reading = std::thread::spawn(move || CanvasSession::open(folder).map_err(|e| e.to_string()));
        self.reading_artwork = Some((Instant::now(), dir, shared_as, reading));
    }

    /// The artwork opened from the gallery, once read: shown, the gallery zooming into its
    /// picture (unless another was opened meanwhile); or, opened to be shared, shared.
    fn artwork_read(&mut self) {
        let Some((_, dir, shared_as, reading)) = self.reading_artwork.take_if(|(.., r)| r.is_finished()) else {
            return;
        };
        let read = reading.join().unwrap_or_else(|_| Err("it stopped".to_owned()));
        if self.session.is_some() {
            return;
        }
        match read {
            Ok(session) if shared_as.is_some() => {
                self.open_artwork(session, dir);
                self.share_once_made = shared_as;
            }
            Ok(session) => {
                let cell = self.gallery.picture_of(&dir);
                self.open_artwork(Arc::clone(&session), dir);
                // the gallery zooms into its picture
                if let (Some(cell), Some(screen)) = (cell, &self.screen) {
                    let size = screen.window.inner_size();
                    let scale = screen.window.scale_factor() as f32;
                    self.zoom = Some(zoom::Zoom::opening(&session, cell, (size.width, size.height), scale));
                }
            }
            Err(e) => files::tell("Can't open the artwork", &e),
        }
    }

    /// The gallery's Photo, once read: a new artwork of the picture, opened (unless another was
    /// opened meanwhile).
    fn photo_read(&mut self) {
        let Some((_, folder, reading)) = self.reading_photo.take_if(|(.., r)| r.is_finished()) else { return };
        let read = reading.join().unwrap_or_else(|_| Err("it stopped".to_owned()));
        if self.session.is_some() {
            return;
        }
        match read {
            Ok(session) => {
                let dir = self.gallery.store.new_folder(folder);
                self.open_artwork(session, dir);
                self.ui.started_new();
            }
            Err(e) => files::tell("Can't open the picture", &e),
        }
    }

    /// An artwork read meanwhile, opened (the one open before kept), and the next one read.
    fn imported(&mut self) {
        let Some(importing) = self.importing.take_if(|i| i.stage.is_finished()) else { return };
        let Importing { since, stage, folder, rest } = importing;
        let (session, dir) = match stage {
            ImportStage::Reading(reading) => {
                let Ok(Some((session, package))) = reading.join() else {
                    self.importing = Importing::next(rest, folder);
                    return;
                };
                if self.session.is_some() && !self.leave_artwork() {
                    return;
                }
                let dir = self.gallery.store.new_folder(folder.clone());
                // a package's time-lapse taken in on its own thread first, the window veiled
                if let Some(package) = package {
                    let into = dir.clone();
                    let taking = std::thread::spawn(move || timelapse::take_package(&package, &into));
                    let stage = ImportStage::TakingTimeLapse(taking, session, dir);
                    self.importing = Some(Importing { since, stage, folder, rest });
                    return;
                }
                (session, dir)
            }
            ImportStage::TakingTimeLapse(taking, session, dir) => {
                let _ = taking.join();
                (session, dir)
            }
        };
        // a model is a new artwork painting on it
        let model = session.model_info().is_some();
        self.open_artwork(session, dir);
        if model {
            self.ui.started_new();
        }
        self.importing = Importing::next(rest, folder);
    }

    /// An artwork dragged out of the window: written as Drag and Drop export's file format says,
    /// then dragged on as that file, for wherever it is let go (the system's drag carries it).
    #[cfg(any(windows, target_os = "macos"))]
    fn drag_out(&mut self, artwork: &procraft_core::GalleryArtwork) {
        let format = self.ui.drag_format();
        let Some(screen) = self.screen.as_mut() else { return };
        let path = match files::drag_file(artwork, format) {
            Ok(path) => path,
            Err(e) => {
                files::tell("Can't drag it out", &e);
                return;
            }
        };
        let image = drag::Image::Raw(files::drag_picture(&artwork.dir, 256).unwrap_or_default());
        let item = drag::DragItem::Files(vec![path]);
        if let Err(e) = drag::start_drag(&*screen.window, item, image, |_, _| {}, drag::Options::default()) {
            log::warn!("dragging an artwork out: {e}");
        }
        // the button is let go inside the system's drag (on Windows once it is over; a Mac's
        // goes on by itself), which egui never hears
        let pos = screen.egui.egui_ctx().input(|i| i.pointer.latest_pos()).unwrap_or(egui::Pos2::ZERO);
        screen.egui.egui_input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        screen.window.request_redraw();
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    fn drag_out(&mut self, _artwork: &procraft_core::GalleryArtwork) {}

    /// The graphics device was lost (its driver reset or stopped), and all it held: nothing is
    /// saved from it (the gallery keeps the artwork as it was last saved, seconds before); the
    /// windows go, and the app says so and starts again.
    fn gpu_lost(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(session) = self.session.take() {
            session.stop_autosave();
        }
        // nothing they show can be drawn (and with the screen gone this happens once)
        self.projection = None;
        self.screen = None;
        files::tell("The graphics device stopped", "ProCraft starts again, with the artwork as it was last saved.");
        match std::env::current_exe() {
            Ok(me) => {
                if let Err(e) = std::process::Command::new(me).spawn() {
                    log::error!("starting again: {e}");
                }
            }
            Err(e) => log::error!("starting again: {e}"),
        }
        event_loop.exit();
    }

    /// The stack the open artwork is in (its folder in the gallery's), none at the top.
    fn artwork_folder(&self) -> Option<String> {
        let folder = self.artwork.as_deref().and_then(|d| std::path::Path::new(d).parent());
        folder.map(|f| f.to_string_lossy().into_owned()).filter(|f| *f != self.gallery.store.root())
    }

    /// The artwork open is put away and kept; false if it couldn't be.
    fn leave_artwork(&mut self) -> bool {
        if let Some(session) = self.session.clone() {
            self.ui.closing(&session);
            // no save of its own after this one
            session.stop_autosave();
        }
        let kept = self.keep_artwork();
        // still open: kept saved again
        if !kept && let (Some(session), Some(dir)) = (&self.session, &self.artwork) {
            Arc::clone(session).start_autosave(dir.clone());
        }
        kept
    }

    /// Saves the artwork open into its folder in the gallery (and the brushes as it left them);
    /// false if it couldn't be.
    fn keep_artwork(&mut self) -> bool {
        let (Some(session), Some(dir)) = (self.session.clone(), self.artwork.clone()) else { return true };
        // its canvas made first (it is being made on a thread of its own), so the save has its picture
        if let Some((_, made)) = self.opening.take() {
            let _ = made.join();
        }
        session.keep_brushes(files::brushes_dir());
        let (brush, mask) = self.ui.colours.brush_and_mask();
        files::keep_colours(brush, mask);
        let edits = session.edits();
        match session.save(dir) {
            Ok(()) => {
                self.ui.saved_edits = edits;
                true
            }
            Err(e) => {
                files::tell("Can't keep the artwork", &e.to_string());
                false
            }
        }
    }

    fn act(&mut self, action: Action, session: &Arc<CanvasSession>) {
        let opened = match action {
            Action::Gallery => {
                if self.leave_artwork() {
                    // the gallery zooms out of the artwork's picture
                    if let (Some(dir), Some(screen)) = (&self.artwork, &self.screen)
                        && let Some(cell) = self.gallery.picture_of(dir)
                    {
                        self.zoom = Some(zoom::Zoom::closing(screen.egui.egui_ctx(), session, cell));
                    }
                    self.session = None;
                    self.artwork = None;
                    self.gallery.refresh();
                    if let Some(screen) = &self.screen {
                        screen.window.request_redraw();
                    }
                }
                None
            }
            // a new artwork the size of this one beside it in the gallery
            Action::New => {
                if !self.leave_artwork() {
                    return;
                }
                let info = session.info();
                let dir = self.gallery.store.new_folder(self.artwork_folder());
                self.open_artwork(CanvasSession::new(info.width, info.height), dir);
                self.ui.started_new();
                None
            }
            // one picked, beside it: read on a thread of its own as the gallery's Import does (a
            // big file takes seconds), this one open meanwhile and kept as that one opens
            Action::Open => {
                if let Some(paths) = files::pick_artwork_files(&[]) {
                    let folder = self.artwork_folder();
                    match &mut self.importing {
                        Some(importing) => importing.rest.push_back(paths),
                        None => self.importing = Importing::next([paths].into(), folder),
                    }
                }
                None
            }
            Action::Save => {
                self.keep_artwork();
                None
            }
            Action::SaveAs => {
                files::save(session, &mut self.ui, true);
                None
            }
            Action::Export(format, pdf_quality) => {
                files::export(session, format, self.ui.artwork_dir.as_deref(), pdf_quality, &mut self.ui.export_card);
                None
            }
        };
        if let Some(session) = opened {
            self.show(session);
        }
    }
}

/// The time between the screen's refreshes (a 60 Hz screen's when it can't be told).
fn frame_gap(window: &Window) -> Duration {
    let millihertz = window.current_monitor().and_then(|m| m.refresh_rate_millihertz()).filter(|&m| m > 0);
    Duration::from_secs_f64(1000.0 / f64::from(millihertz.unwrap_or(60_000)))
}
