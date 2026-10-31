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
mod copy_paste;
mod crop;
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
mod liquify;
mod look;
mod menu;
mod overlays;
mod panel;
mod prefs;
mod quick_menu;
mod reference;
mod screen;
mod selection;
mod timelapse;
mod tool_header;
mod transform;
mod transform_numeric;
mod ui;
mod video;

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
}

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

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
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
                if self.keep_artwork() {
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

    /// Runs the interface, draws the canvas and the interface over it, and does what was asked.
    fn frame(&mut self) {
        let Some(screen) = self.screen.as_mut() else { return };
        let Some(session) = self.session.clone() else {
            self.gallery_frame();
            return;
        };
        self.last_frame = Instant::now();
        let raw = screen.egui.take_egui_input(&screen.window);
        let output = screen.egui.egui_ctx().clone().run_ui(raw, |ui| self.ui.show(ui, &session, &mut self.input));
        self.again = screen.draw(Some(&session), self.started, output).map(|after| Instant::now() + after);
        let unsaved = if session.edits() == self.ui.saved_edits { "" } else { " •" };
        let title = format!("{}{unsaved} — ProCraft", session.name());
        if title != self.title {
            screen.window.set_title(&title);
            self.title = title;
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
        let output = screen.egui.egui_ctx().clone().run_ui(raw, |ui| {
            interface.style_once(ui.ctx());
            asked = gallery.show(ui.ctx(), &mut interface.look, &interface.colours);
        });
        self.again = screen.draw(None, self.started, output).map(|after| Instant::now() + after);
        if self.title != "ProCraft" {
            screen.window.set_title("ProCraft");
            self.title = "ProCraft".into();
        }
        match asked {
            Some(gallery::Asked::Open(dir)) => match CanvasSession::open(dir.clone()) {
                Ok(session) => self.open_artwork(session, dir),
                Err(e) => files::tell("Can't open the artwork", &e.to_string()),
            },
            Some(gallery::Asked::New(preset, folder)) => {
                let session = files::new_from_preset(&preset);
                let dir = self.gallery.store.new_folder(folder);
                self.open_artwork(session, dir);
            }
            Some(gallery::Asked::ScreenSize(folder)) => {
                let (w, h) = self.gallery.screen_px;
                let session = CanvasSession::new(w, h);
                session.set_new_profile(procraft_core::P3.into());
                let dir = self.gallery.store.new_folder(folder);
                self.open_artwork(session, dir);
            }
            Some(gallery::Asked::Share(list, format)) => files::share_artworks(&list, format),
            Some(gallery::Asked::Photo(folder)) => {
                if let Some(session) = files::pick_picture() {
                    let dir = self.gallery.store.new_folder(folder);
                    self.open_artwork(session, dir);
                }
            }
            Some(gallery::Asked::Import(folder)) => {
                if let Some((session, package)) = files::pick_artwork() {
                    let dir = self.gallery.store.new_folder(folder);
                    if let Some(package) = package {
                        timelapse::take_package(&package, &dir);
                    }
                    self.open_artwork(session, dir);
                }
            }
            None => {}
        }
    }

    /// Shows `session`, kept in the gallery's folder `dir`.
    fn open_artwork(&mut self, session: Arc<CanvasSession>, dir: String) {
        self.ui.artwork_dir = Some(dir.clone());
        self.artwork = Some(dir);
        self.ui.path = None;
        self.ui.saved_edits = session.edits();
        self.show(session);
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
                if self.keep_artwork() {
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
                if !self.keep_artwork() {
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
                    if let Some(package) = package {
                        timelapse::take_package(&package, &dir);
                    }
                    self.open_artwork(next, dir);
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
            Action::Export(format) => {
                files::export(session, format, self.ui.artwork_dir.as_deref());
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
