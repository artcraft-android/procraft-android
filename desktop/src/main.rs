//! ProCraft on the desktop: the engine draws the canvas into the window, and the interface is
//! drawn over it with egui, on the same device.

// no console window behind the app on Windows (a debug build keeps it, for the log)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod alert;
mod chrome;
mod colour_drop;
mod colours;
mod files;
mod icons;
mod input;
mod layers;
mod library;
mod look;
mod menu;
mod overlays;
mod panel;
mod screen;
mod ui;

use std::sync::Arc;
use std::time::Instant;

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
    let mut app = App {
        started: Instant::now(),
        screen: None,
        session: None,
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
    screen: Option<Screen>,
    /// The artwork open, made once the window is there.
    session: Option<Arc<CanvasSession>>,
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
        // a new artwork is the screen's size
        let canvas = screen
            .window
            .current_monitor()
            .map(|m| m.size())
            .filter(|s| s.width > 0 && s.height > 0)
            .map_or(FALLBACK_CANVAS, |s| (s.width, s.height));
        let session = CanvasSession::new(canvas.0, canvas.1);
        self.screen = Some(screen);
        self.show(session);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(screen), Some(session)) = (self.screen.as_mut(), self.session.clone()) else { return };
        let response = screen.egui.on_window_event(&screen.window, &event);
        if response.repaint {
            screen.window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => {
                if files::may_close(&session, &mut self.ui) {
                    session.keep_brushes(files::brushes_dir());
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                screen.resize(size.width, size.height);
                session.resize_window(size.width, size.height);
                screen.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                session.set_display_scale(scale_factor as f32);
                self.ui.place_canvas(&session, scale_factor as f32);
            }
            WindowEvent::RedrawRequested => self.frame(),
            event => {
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
        let (Some(screen), Some(session)) = (self.screen.as_ref(), self.session.as_ref()) else { return };
        // the touches' waits (a hold, a repeat) run out
        if self.input.tick(session) || session.wants_frame() || !self.input.commands.is_empty() {
            screen.window.request_redraw();
        }
        event_loop
            .set_control_flow(self.input.next_deadline(session).map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
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
        let (Some(screen), Some(session)) = (self.screen.as_mut(), self.session.clone()) else { return };
        let raw = screen.egui.take_egui_input(&screen.window);
        let output = screen.egui.egui_ctx().clone().run_ui(raw, |ui| self.ui.show(ui, &session, &mut self.input));
        let wants = screen.draw(&session, self.started, output);
        if wants {
            screen.window.request_redraw();
        }
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

    fn act(&mut self, action: Action, session: &Arc<CanvasSession>) {
        let opened = match action {
            Action::New => files::new_artwork(session, &mut self.ui),
            Action::Open => files::open(session, &mut self.ui),
            Action::Save => {
                files::save(session, &mut self.ui, false);
                None
            }
            Action::SaveAs => {
                files::save(session, &mut self.ui, true);
                None
            }
            Action::Export(format) => {
                files::export(session, format);
                None
            }
            Action::InsertFile => {
                files::insert_file(session);
                None
            }
        };
        if let Some(session) = opened {
            self.show(session);
        }
    }
}
