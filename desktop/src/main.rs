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
mod colours;
mod compact_colours;
mod companion;
mod copy_paste;
mod crop;
mod curve;
mod custom_canvas;
mod files;
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
mod prefs;
mod projection;
mod quick_menu;
mod recolor;
mod reference;
mod screen;
mod selection;
mod settings;
mod sound;
mod source_editor;
mod studio;
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

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            log::error!("no event loop: {e}");
            return;
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    // the colour profiles imported before
    procraft_core::set_profile_folder(files::app_dir().join("profiles").to_string_lossy().into_owned());
    let mut app = App {
        started: Instant::now(),
        last_frame: Instant::now(),
        again: None,
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
        dropped: Vec::new(),
        tap_held: None,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("event loop: {e}");
    }
}

struct App {
    started: Instant,
    /// When the last frame was drawn, and when another is wanted (the canvas or the interface still
    /// moving, or a hold to be timed).
    last_frame: Instant,
    again: Option<Instant>,
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
    /// Files dropped on the window, taken in together at the next frame (a model with its
    /// material file and images comes in one drop).
    dropped: Vec<std::path::PathBuf>,
    /// Tap Assistance: a lone finger's touch, where it landed and when, while it is held there.
    tap_held: Option<(u64, winit::dpi::PhysicalPosition<f64>, Instant)>,
}

/// Tap Assistance holds a finger's touch where it landed this long.
const TAP_HOLD: std::time::Duration = std::time::Duration::from_millis(300);

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.screen.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("ProCraft")
            .with_inner_size(LogicalSize::new(1440.0, 900.0))
            .with_min_inner_size(LogicalSize::new(640.0, 480.0))
            // opened for a test (PROCRAFT_BACKGROUND set), it leaves the keyboard where it was
            .with_active(std::env::var_os("PROCRAFT_BACKGROUND").is_none());
        let window = match event_loop.create_window(attributes) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("no window: {e}");
                event_loop.exit();
                return;
            }
        };
        let screen = match Screen::new(window) {
            Ok(s) => s,
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
        if self.projection.as_ref().is_some_and(|p| p.window.id() == id) {
            self.projection_event(event);
            return;
        }
        let Some(event) = self.tap_assist(event) else { return };
        let Some(screen) = self.screen.as_mut() else { return };
        let session = self.session.clone();
        let response = screen.egui.on_window_event(&screen.window, &event);
        // egui counts every redraw as wanting another; the frames it does want come from its
        // repaint delay, paced in `about_to_wait`
        if response.repaint && event != WindowEvent::RedrawRequested {
            screen.window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => {
                // the artwork open is kept in the gallery
                if self.leave_artwork() {
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
                // with a popover open, a press on the canvas only closes it
                let (blocked, canvas) = (self.ui.popover_open, self.ui.canvas_rect);
                let over = |p: (f32, f32)| blocked || screen.over_interface(p, canvas);
                if self.input.on_event(&session, &event, response.consumed, over) {
                    screen.window.request_redraw();
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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
        session.use_gpu(screen.gpu.clone());
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
            if studio.pad_texture.take().is_some() {
                screen.drop_pad();
            }
            return false;
        }
        let ppp = screen.egui.egui_ctx().pixels_per_point();
        let Some(rect) = studio.pad_rect else { return true };
        let size = ((rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32);
        if size.0 < 2 || size.1 < 2 {
            return true;
        }
        if studio.pad.is_none() {
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
        let Some(screen) = self.screen.as_mut() else { return };
        let Some(session) = self.session.clone() else {
            self.gallery_frame();
            return;
        };
        self.last_frame = Instant::now();
        let raw = screen.egui.take_egui_input(&screen.window);
        let output = screen.egui.egui_ctx().clone().run_ui(raw, |ui| {
            self.ui.show(ui, &session, &mut self.input);
            // opening: the gallery over it, growing from the artwork's picture
            if let Some(zoom) = self.zoom.as_ref().filter(|z| z.opening) {
                zoom.over_canvas(ui.ctx(), &mut self.gallery, &mut self.ui.look);
            }
        });
        let pad_soon = self.studio_pad(&session);
        let Some(screen) = self.screen.as_mut() else { return };
        self.again = screen.draw(Some(&session), self.started, output).map(|after| Instant::now() + after);
        if pad_soon {
            self.again = Some(Instant::now());
        }
        let unsaved = if session.edits() == self.ui.saved_edits { "" } else { " •" };
        let title = format!("{}{unsaved} — ProCraft", session.name());
        if title != self.title {
            screen.window.set_title(&title);
            self.title = title;
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
        let output = screen.egui.egui_ctx().clone().run_ui(raw, |ui| {
            interface.style_once(ui.ctx());
            // closing: the gallery settling back into place, the canvas fading over it
            // otherwise in its place (a closing zoom cut short leaves nothing behind)
            match closing {
                Some(zoom) => zoom.over_gallery(ui.ctx()),
                None => zoom::Zoom::end(ui.ctx()),
            }
            asked = gallery.show(ui.ctx(), &mut interface.look, &interface.colours);
        });
        self.again = screen.draw(None, self.started, output).map(|after| Instant::now() + after);
        if self.title != "ProCraft" {
            screen.window.set_title("ProCraft");
            self.title = "ProCraft".into();
        }
        self.zoom_on();
        // Project canvas is black while the gallery shows
        if let (Some(p), Some(screen)) = (self.projection.as_mut(), &self.screen) {
            p.draw(&screen.gpu, None);
        }
        match asked {
            Some(gallery::Asked::Open(dir)) => match CanvasSession::open(dir.clone()) {
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
                Err(e) => files::tell("Can't open the artwork", &e.to_string()),
            },
            Some(gallery::Asked::New(preset, folder)) => {
                let session = files::new_from_preset(&preset);
                let dir = self.gallery.store.new_folder(folder);
                self.open_artwork(session, dir);
                self.ui.started_new();
            }
            Some(gallery::Asked::ScreenSize(folder)) => {
                let (w, h) = self.gallery.screen_px;
                let session = CanvasSession::new(w, h);
                session.set_new_profile(procraft_core::P3.into());
                let dir = self.gallery.store.new_folder(folder);
                self.open_artwork(session, dir);
                self.ui.started_new();
            }
            Some(gallery::Asked::Share(list, format)) => files::share_artworks(&list, format, 100),
            Some(gallery::Asked::SharePdf(list, quality)) => files::share_artworks(&list, files::Format::Pdf, quality),
            Some(gallery::Asked::DragOut(artwork)) => self.drag_out(&artwork),
            Some(gallery::Asked::ShareAnimated(artwork, format)) => match CanvasSession::open(artwork.dir.clone()) {
                Ok(session) => {
                    self.open_artwork(session, artwork.dir);
                    self.ui.share_animated_from_gallery(format);
                }
                Err(e) => files::tell("Can't open the artwork", &e.to_string()),
            },
            Some(gallery::Asked::Photo(folder)) => {
                if let Some(session) = files::pick_picture() {
                    let dir = self.gallery.store.new_folder(folder);
                    self.open_artwork(session, dir);
                    self.ui.started_new();
                }
            }
            Some(gallery::Asked::Import(folder)) => {
                if let Some((session, package)) = files::pick_artwork() {
                    let dir = self.gallery.store.new_folder(folder);
                    // a model is a new artwork painting on it
                    let model = session.model_info().is_some();
                    if let Some(package) = package {
                        timelapse::take_package(&package, &dir);
                    }
                    self.open_artwork(session, dir);
                    if model {
                        self.ui.started_new();
                    }
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
        self.artwork = Some(dir);
        self.ui.path = None;
        self.ui.saved_edits = session.edits();
        self.show(session);
    }

    /// Tap Assistance: for its first moments a lone finger's touch stays where it landed (its
    /// moves held back, its lift put back there), so a tap that drifts still lands where it began.
    /// More fingers, a pen, or a touch held longer go on as they are. None: the event is held back.
    fn tap_assist(&mut self, event: WindowEvent) -> Option<WindowEvent> {
        let WindowEvent::Touch(touch) = &event else { return Some(event) };
        if !self.ui.tap_assistance() || touch.force.is_some() {
            return Some(event);
        }
        let held = self.tap_held.filter(|(id, _, _)| *id == touch.id);
        match touch.phase {
            // a second finger lets the first go on as it is
            winit::event::TouchPhase::Started if self.tap_held.is_some() => self.tap_held = None,
            winit::event::TouchPhase::Started => self.tap_held = Some((touch.id, touch.location, Instant::now())),
            winit::event::TouchPhase::Moved => match held {
                Some((_, _, since)) if since.elapsed() <= TAP_HOLD => return None,
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
        // brushes wait for a canvas to open
        let (brushes, paths): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| files::is_brush_file(p));
        if !brushes.is_empty() {
            files::keep_pending_brushes(&brushes);
        }
        if paths.is_empty() {
            return;
        }
        let Some((session, package)) = files::open_artwork_files(paths) else { return };
        let dir = self.gallery.store.new_folder(self.gallery.folder());
        let model = session.model_info().is_some();
        if let Some(package) = package {
            timelapse::take_package(&package, &dir);
        }
        self.open_artwork(session, dir);
        if model {
            self.ui.started_new();
        }
    }

    /// An artwork dragged out of the window: written as Drag and Drop export's file format says,
    /// then dragged on as that file, for wherever it is let go (the system's drag carries it).
    #[cfg(windows)]
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
        // the button was let go inside the system's drag, which egui never heard
        let pos = screen.egui.egui_ctx().input(|i| i.pointer.latest_pos()).unwrap_or(egui::Pos2::ZERO);
        screen.egui.egui_input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        screen.window.request_redraw();
    }

    #[cfg(not(windows))]
    fn drag_out(&mut self, _artwork: &procraft_core::GalleryArtwork) {}

    /// The artwork open is put away and kept; false if it couldn't be.
    fn leave_artwork(&mut self) -> bool {
        if let Some(session) = self.session.clone() {
            self.ui.closing(&session);
        }
        self.keep_artwork()
    }

    /// Saves the artwork open into its folder in the gallery (and the brushes as it left them);
    /// false if it couldn't be.
    fn keep_artwork(&mut self) -> bool {
        let (Some(session), Some(dir)) = (self.session.clone(), self.artwork.clone()) else { return true };
        session.keep_brushes(files::brushes_dir());
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
            // a new artwork the size of this one, or one picked, beside it in the gallery
            Action::New | Action::Open => {
                if !self.leave_artwork() {
                    return;
                }
                let next = if matches!(action, Action::New) {
                    let info = session.info();
                    Some((CanvasSession::new(info.width, info.height), None))
                } else {
                    files::pick_artwork()
                };
                if let Some((next, package)) = next {
                    let folder = self.artwork.as_deref().and_then(|d| std::path::Path::new(d).parent());
                    let folder =
                        folder.map(|f| f.to_string_lossy().into_owned()).filter(|f| *f != self.gallery.store.root());
                    let dir = self.gallery.store.new_folder(folder);
                    let new = package.is_none() && (matches!(action, Action::New) || next.model_info().is_some());
                    if let Some(package) = package {
                        timelapse::take_package(&package, &dir);
                    }
                    self.open_artwork(next, dir);
                    if new {
                        self.ui.started_new();
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
                files::export(session, format, self.ui.artwork_dir.as_deref(), pdf_quality);
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
