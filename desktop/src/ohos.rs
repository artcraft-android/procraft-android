//! The app on HarmonyOS, until it has an interface of its own: the canvas component (an ArkUI
//! XComponent, full screen) is its window, and its native side's callbacks drive it as winit's
//! event loop does on the desktop. harmony/bridge hands them here, on the interface's thread: the
//! surface coming, changing size and going, touches (with the pen's tool, pressure and tilt, and
//! the samples kept between frames), the pen hovering, the mouse, keys, focus, and the soft
//! keyboard's text.
//!
//! The app runs on a thread of its own, which takes those as events and draws. It can then wait
//! for what it asks of the system through ArkTS (the document picker, the pasteboard:
//! `ohos_platform.rs`) as it waits for rfd's dialogs on the desktop, while the interface's thread
//! answers. The GPU device, the canvas and the interface outlive the surface: the app going to
//! the background loses nothing.
#![allow(unsafe_code)]

use std::cell::Cell;
use std::ffi::c_void;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::raw_window_handle as rwh;

use crate::egui_events::{EguiEvents, Wants};
use crate::native_input::{Held, mouse_events, touch_event};
pub use crate::native_input::{MouseAction, NativeButton, NativeKey, NativeMouse, NativeTouch, Phase, Tool};
pub use crate::ohos_platform::{answer, set_requester};
use crate::{App, Looper};

/// The display's refresh when the system hasn't said, millihertz.
const REFRESH: u32 = 60_000;

/// How long the interface's thread waits, as the surface goes, for the app to let go of it.
const LET_GO_WAIT: Duration = Duration::from_millis(500);

/// The pen's id while it hovers (no touch has it then).
const HOVER: u64 = u64::MAX - 1;

// libnative_window.so: a reference of the app's own to the component's native window, so the
// window outlives a surface the app hasn't let go of yet
#[link(name = "native_window")]
unsafe extern "C" {
    fn OH_NativeWindow_NativeObjectReference(object: *mut c_void) -> i32;
    fn OH_NativeWindow_NativeObjectUnreference(object: *mut c_void) -> i32;
}

/// The canvas component's native window, with a reference of the app's own.
pub struct NativeWindow(NonNull<c_void>);

impl NativeWindow {
    /// `window`, referenced for as long as this is kept.
    pub fn new(window: NonNull<c_void>) -> NativeWindow {
        // SAFETY: the system's live native window, as its surface callback hands it over
        unsafe { OH_NativeWindow_NativeObjectReference(window.as_ptr()) };
        NativeWindow(window)
    }
}

impl Drop for NativeWindow {
    fn drop(&mut self) {
        // SAFETY: the reference taken in `new`, given back once
        unsafe { OH_NativeWindow_NativeObjectUnreference(self.0.as_ptr()) };
    }
}

// SAFETY: the window is only handed to wgpu (to make the surface on, which it may do from any
// thread) and referenced and let go of, which the system does from any thread
unsafe impl Send for NativeWindow {}
// SAFETY: as for `Send`: nothing is written through the pointer here
unsafe impl Sync for NativeWindow {}

/// The canvas component's surface, as the app's window: the system's native window (handed to
/// wgpu for the surface), its size, the display's density, and whether a frame was asked for.
pub struct Window {
    native: NativeWindow,
    width: AtomicU32,
    height: AtomicU32,
    /// Pixels per vp (ArkUI's unit), which the interface's points are.
    scale: f32,
    redraw: Arc<AtomicBool>,
}

impl Window {
    pub fn request_redraw(&self) {
        self.redraw.store(true, Ordering::Relaxed);
    }

    pub fn inner_size(&self) -> PhysicalSize<u32> {
        PhysicalSize::new(self.width.load(Ordering::Relaxed), self.height.load(Ordering::Relaxed))
    }

    pub fn scale_factor(&self) -> f64 {
        f64::from(self.scale)
    }

    /// The window is the canvas component, always shown: nothing to hide.
    pub fn set_visible(&self, _visible: bool) {}

    /// No title bar to show it in.
    pub fn set_title(&self, _title: &str) {}

    pub fn pre_present_notify(&self) {}

    /// The screen the window fills.
    pub fn current_monitor(&self) -> Option<Monitor> {
        Some(Monitor { size: self.inner_size() })
    }

    pub fn theme(&self) -> Option<winit::window::Theme> {
        None
    }

    /// Always the whole screen: nothing to switch.
    pub fn fullscreen(&self) -> Option<winit::window::Fullscreen> {
        None
    }

    pub fn set_fullscreen(&self, _fullscreen: Option<winit::window::Fullscreen>) {}

    fn resized(&self, width: u32, height: u32) {
        self.width.store(width, Ordering::Relaxed);
        self.height.store(height, Ordering::Relaxed);
    }
}

impl rwh::HasWindowHandle for Window {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let raw = rwh::RawWindowHandle::OhosNdk(rwh::OhosNdkWindowHandle::new(self.native.0));
        // SAFETY: the canvas component's native window, which the app's own reference keeps alive
        // for as long as this window (and the surface made on it) is kept
        Ok(unsafe { rwh::WindowHandle::borrow_raw(raw) })
    }
}

impl rwh::HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(rwh::DisplayHandle::ohos())
    }
}

/// The screen the window fills.
pub struct Monitor {
    size: PhysicalSize<u32>,
}

impl Monitor {
    pub fn size(&self) -> PhysicalSize<u32> {
        self.size
    }

    pub fn refresh_rate_millihertz(&self) -> Option<u32> {
        Some(REFRESH)
    }
}

/// The system's display, for wgpu's instance: HarmonyOS has one, with nothing to hold.
#[derive(Clone, Debug, Default)]
pub struct OwnedDisplayHandle;

impl rwh::HasDisplayHandle for OwnedDisplayHandle {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(rwh::DisplayHandle::ohos())
    }
}

/// Wakes the app from another thread: it looks again at once.
pub struct EventLoopProxy<T> {
    events: mpsc::Sender<Event>,
    kind: PhantomData<fn(T)>,
}

impl<T> Clone for EventLoopProxy<T> {
    fn clone(&self) -> EventLoopProxy<T> {
        EventLoopProxy { events: self.events.clone(), kind: PhantomData }
    }
}

impl<T> EventLoopProxy<T> {
    pub fn send_event(&self, _event: T) -> Result<(), T> {
        let _ = self.events.send(Event::Wake);
        Ok(())
    }
}

/// Whether egui took an event (`consumed`), and whether it wants a frame for it.
#[derive(Clone, Copy, Debug, Default)]
pub struct EventResponse {
    pub consumed: bool,
    pub repaint: bool,
}

/// egui's input from the window's events, as egui-winit makes it on the desktop
/// (`egui_events.rs`); what egui asks of the system after each frame (the pasteboard, the soft
/// keyboard).
pub struct EguiState {
    ctx: egui::Context,
    events: EguiEvents,
    started: Instant,
    /// Whether the soft keyboard was last asked up.
    keyboard: bool,
}

impl EguiState {
    pub fn new(
        ctx: egui::Context,
        _viewport: egui::ViewportId,
        _window: &Window,
        native_pixels_per_point: Option<f32>,
        _theme: Option<winit::window::Theme>,
        max_texture_side: Option<usize>,
    ) -> EguiState {
        let mut events = EguiEvents::new();
        events.input.focused = true;
        events.input.max_texture_side = max_texture_side;
        events.input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point =
            native_pixels_per_point;
        EguiState { ctx, events, started: Instant::now(), keyboard: false }
    }

    pub fn egui_ctx(&self) -> &egui::Context {
        &self.ctx
    }

    pub fn egui_input_mut(&mut self) -> &mut egui::RawInput {
        &mut self.events.input
    }

    pub fn set_clipboard_shortcuts(&mut self, enabled: bool) {
        self.events.clipboard_shortcuts = enabled;
    }

    fn wants(&self) -> Wants {
        Wants {
            pointer: self.ctx.egui_wants_pointer_input(),
            using_pointer: self.ctx.egui_is_using_pointer(),
            keyboard: self.ctx.egui_wants_keyboard_input(),
        }
    }

    fn pixels_per_point(&self, window: &Window) -> f32 {
        window.scale * self.ctx.zoom_factor()
    }

    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> EventResponse {
        let wants = self.wants();
        let consumed = self.events.window_event(event, self.pixels_per_point(window), wants);
        EventResponse { consumed, repaint: true }
    }

    /// A key, which egui takes as egui-winit gives it (a paste with the system pasteboard's text,
    /// where the app may read it); true if egui took it.
    fn key(&mut self, key: egui::Key, pressed: bool, repeat: bool) -> bool {
        let paste = pressed && key == egui::Key::V && self.events.modifiers.command && self.events.clipboard_shortcuts;
        if paste && let Some(text) = crate::ohos_platform::paste_text() {
            self.events.clipboard = text;
        }
        let wants = self.wants();
        self.events.key(key, pressed, repeat, wants)
    }

    /// Text the soft keyboard typed.
    fn text(&mut self, text: String) {
        self.events.input.events.push(egui::Event::Text(text));
    }

    pub fn take_egui_input(&mut self, window: &Window) -> egui::RawInput {
        let size = window.inner_size();
        let time = self.started.elapsed().as_secs_f64();
        self.events.take((size.width, size.height), self.pixels_per_point(window), window.scale, time)
    }

    pub fn handle_platform_output(&mut self, _window: &Window, output: egui::PlatformOutput) {
        for command in &output.commands {
            if let egui::OutputCommand::CopyText(text) = command {
                crate::ohos_platform::copy_text(text);
            }
        }
        self.events.commands(output.commands);
        // a text field with the keys brings the soft keyboard up, and takes it down as it goes
        let typing = output.ime.is_some();
        if typing != self.keyboard {
            self.keyboard = typing;
            crate::ohos_platform::keyboard(typing);
        }
    }
}

/// Where the log's lines go besides the log file: the system's log, as harmony/bridge sets it.
static LOG: OnceLock<fn(&str)> = OnceLock::new();

/// The log's lines go to `sink` too (hilog), from now on.
pub fn log_to(sink: fn(&str)) {
    let _ = LOG.set(sink);
}

pub(crate) fn log_line(line: &[u8]) {
    if let Some(sink) = LOG.get() {
        sink(String::from_utf8_lossy(line).trim_end());
    }
}

/// What reaches the app's thread from the interface's (and from the app's own other threads).
pub enum Event {
    /// The surface came: its native window, `width` × `height` pixels.
    SurfaceCreated(NativeWindow, u32, u32),
    SurfaceChanged(u32, u32),
    /// The surface is going: the app lets go of it, then says so.
    SurfaceDestroyed(mpsc::Sender<()>),
    Touch(NativeTouch),
    /// The pen hovering at (x, y) vp, with its tilts (degrees), if it gives them.
    PenHover(f32, f32, Option<(f32, f32)>),
    Mouse(NativeMouse),
    Hover(bool),
    Key(NativeKey, bool),
    Focus(bool),
    /// Text the soft keyboard typed.
    Text(String),
    /// A key the soft keyboard pressed (`count` times): Backspace, Delete, Enter.
    SoftKey(egui::Key, u32),
    /// Something wants the app to look again (a thread of its own done, say).
    Wake,
}

/// Where the interface's thread sends the app its events, once the app has started.
static EVENTS: Mutex<Option<mpsc::Sender<Event>>> = Mutex::new(None);

/// Sends `event` to the app, if it is running.
fn send(event: Event) {
    if let Some(events) = EVENTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
        let _ = events.send(event);
    }
}

/// The app's loop on HarmonyOS: when the next frame is due, and whether the app is ending.
#[derive(Default)]
struct Loop {
    until: Cell<Option<Instant>>,
    exit: Cell<bool>,
}

impl Looper for Loop {
    fn exit(&self) {
        self.exit.set(true);
    }

    fn wait(&self, until: Option<Instant>) {
        self.until.set(until);
    }
}

/// The app on its thread, and what it keeps of the system's.
struct Host {
    app: App,
    looper: Loop,
    /// A frame asked for (the window's `request_redraw`); cleared as it is drawn.
    redraw: Arc<AtomicBool>,
    held: Held,
    /// The keys held down (another press of one is the system repeating it).
    keys_down: Vec<egui::Key>,
    /// Pixels per vp.
    density: f32,
}

/// Starts the app on a thread of its own: its folders are the app's sandbox (`files_dir`,
/// `cache_dir`: ArkTS's `context.filesDir` and `cacheDir`), the display `density` pixels per vp.
/// Once, before anything else.
pub fn start(files_dir: PathBuf, cache_dir: PathBuf, density: f32) {
    let mut events = EVENTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if events.is_some() {
        return;
    }
    let (sender, receiver) = mpsc::channel();
    let density = if density.is_finite() && density > 0.0 { density } else { 1.0 };
    let wake = sender.clone();
    let thread = std::thread::Builder::new().name("procraft-app".to_owned()).spawn(move || {
        crate::files::set_app_dir(files_dir);
        crate::ohos_platform::set_cache_dir(cache_dir);
        crate::prepare();
        let host = Host {
            app: App::new(EventLoopProxy { events: wake, kind: PhantomData }),
            looper: Loop::default(),
            redraw: Arc::new(AtomicBool::new(true)),
            held: Held::default(),
            keys_down: Vec::new(),
            density,
        };
        run(host, &receiver);
    });
    match thread {
        Ok(_) => *events = Some(sender),
        Err(e) => log::error!("the app's thread: {e}"),
    }
}

/// The app's loop: events as they come, the app's housekeeping, and a frame when one is due.
fn run(mut host: Host, events: &mpsc::Receiver<Event>) {
    loop {
        let first = match host.looper.until.get() {
            Some(until) => events.recv_timeout(until.saturating_duration_since(Instant::now())).ok(),
            None => match events.recv() {
                Ok(event) => Some(event),
                // nothing can reach the app any more
                Err(_) => return,
            },
        };
        let mut more = first.into_iter().chain(std::iter::from_fn(|| events.try_recv().ok())).collect::<Vec<_>>();
        let turn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for event in more.drain(..) {
                host.take(event);
            }
            host.turn();
        }));
        if let Err(panic) = turn {
            log::error!("the app panicked: {}", procraft_gpu::panic_message(panic.as_ref()));
            host.looper.exit.set(true);
        }
        if host.looper.exit.get() {
            log::info!("the app ended");
            return;
        }
    }
}

impl Host {
    fn window_event(&mut self, event: WindowEvent) {
        self.app.on_window_event(&self.looper, event);
    }

    fn take(&mut self, event: Event) {
        match event {
            Event::SurfaceCreated(native, width, height) => self.surface_created(native, width, height),
            Event::SurfaceChanged(width, height) => {
                if let Some(screen) = &self.app.screen {
                    screen.window.resized(width, height);
                }
                self.window_event(WindowEvent::Resized(PhysicalSize::new(width, height)));
            }
            Event::SurfaceDestroyed(done) => {
                if let Some(screen) = self.app.screen.as_mut() {
                    screen.drop_surface();
                }
                let _ = done.send(());
                // the app in the background may be ended without a word: the artwork is kept now
                self.app.keep_artwork();
            }
            Event::Touch(touch) => {
                let event = touch_event(&touch);
                self.window_event(event);
            }
            Event::PenHover(x, y, tilt) => {
                crate::pen::pointer_kind(HOVER, true);
                let (tilt_x, tilt_y) = tilt.unwrap_or((0.0, 0.0));
                crate::pen::keep_pose(HOVER, crate::pen::lean_of(tilt_x, tilt_y), None);
                let device_id = winit::event::DeviceId::dummy();
                let position =
                    winit::dpi::PhysicalPosition::new(f64::from(x * self.density), f64::from(y * self.density));
                self.window_event(WindowEvent::CursorMoved { device_id, position });
            }
            Event::Mouse(mouse) => {
                for event in mouse_events(&mouse) {
                    self.window_event(event);
                }
            }
            Event::Hover(inside) => {
                let device_id = winit::event::DeviceId::dummy();
                let event = if inside {
                    WindowEvent::CursorEntered { device_id }
                } else {
                    WindowEvent::CursorLeft { device_id }
                };
                self.window_event(event);
            }
            Event::Key(key, pressed) => self.key(key, pressed),
            Event::Focus(focused) => self.window_event(WindowEvent::Focused(focused)),
            Event::Text(text) => {
                if let Some(screen) = self.app.screen.as_mut() {
                    screen.egui.text(text);
                    screen.window.request_redraw();
                }
            }
            Event::SoftKey(key, count) => {
                for _ in 0..count.clamp(1, 64) {
                    self.key(NativeKey::Key(key), true);
                    self.key(NativeKey::Key(key), false);
                }
            }
            Event::Wake => {
                // (the start's watch asks too)
                crate::startup::answered();
            }
        }
    }

    /// The surface is there: the app's screen made on it the first time, else moved onto it (the
    /// device, the canvas and the interface kept from before).
    fn surface_created(&mut self, native: NativeWindow, width: u32, height: u32) {
        let window = Arc::new(Window {
            native,
            width: AtomicU32::new(width),
            height: AtomicU32::new(height),
            scale: self.density,
            redraw: Arc::clone(&self.redraw),
        });
        let Some(screen) = self.app.screen.as_mut() else {
            self.app.start(window, &OwnedDisplayHandle, &self.looper);
            return;
        };
        if let Err(e) = screen.attach(Arc::clone(&window)) {
            log::error!("the canvas couldn't draw into the new surface: {e}");
            return;
        }
        self.window_event(WindowEvent::Resized(PhysicalSize::new(width, height)));
        window.request_redraw();
    }

    /// A key of a hardware keyboard (or the soft keyboard's) going down (`pressed`) or up: a
    /// modifier changes the modifiers; another goes to egui as a key (and the text it types), and
    /// Space and R to the canvas as well, as on the desktop. A key pressed again while it is held
    /// is the system repeating it.
    fn key(&mut self, key: NativeKey, pressed: bool) {
        if let Some(event) = self.held.key(key, pressed) {
            self.window_event(event);
        }
        let NativeKey::Key(key) = key else { return };
        let repeat = pressed && self.keys_down.contains(&key);
        self.keys_down.retain(|k| *k != key);
        if pressed {
            self.keys_down.push(key);
        }
        let app = &mut self.app;
        let Some(screen) = app.screen.as_mut() else { return };
        let consumed = screen.egui.key(key, pressed, repeat);
        screen.window.request_redraw();
        let canvas = match key {
            egui::Key::Space => crate::input::CanvasKey::Space,
            egui::Key::R => crate::input::CanvasKey::R,
            _ => return,
        };
        if let Some(session) = app.session.clone()
            && app.input.on_key(&session, canvas, pressed, repeat, false, consumed)
            && let Some(screen) = &app.screen
        {
            screen.window.request_redraw();
        }
    }

    /// After the events: the app's housekeeping, files written for the system handed on, and a
    /// frame if one is due.
    fn turn(&mut self) {
        if !self.app.housekeeping(&self.looper) {
            return;
        }
        crate::ohos_platform::hand_on();
        self.app.pace(&self.looper);
        let due = self.looper.until.get().is_some_and(|until| Instant::now() >= until);
        let has_surface = self.app.screen.as_ref().is_some_and(|s| !s.minimised());
        if (self.redraw.swap(false, Ordering::Relaxed) || due) && has_surface {
            self.looper.until.set(None);
            self.window_event(WindowEvent::RedrawRequested);
        }
    }
}

/// The canvas component's surface is there: `native` its native window, `width` × `height`
/// pixels.
pub fn surface_created(native: NonNull<c_void>, width: u32, height: u32) {
    send(Event::SurfaceCreated(NativeWindow::new(native), width, height));
}

/// The surface is `width` × `height` pixels now (the device turned, the window split).
pub fn surface_changed(width: u32, height: u32) {
    send(Event::SurfaceChanged(width, height));
}

/// The surface is going (the app went to the background): the app lets go of it (the canvas
/// stays), waited for a moment so the system doesn't take it from under a frame being drawn.
pub fn surface_destroyed() {
    let (done, let_go) = mpsc::channel();
    send(Event::SurfaceDestroyed(done));
    if let_go.recv_timeout(LET_GO_WAIT).is_err() {
        log::warn!("the surface went while the app was busy: it lets go of it when it's done");
    }
}

/// A touch on the canvas component.
pub fn touch(touch: NativeTouch) {
    send(Event::Touch(touch));
}

/// The pen hovering over the canvas component at (`x`, `y`) vp (ArkUI's `onHoverMove`), with its
/// tilts if it gives them.
pub fn pen_hover(x: f32, y: f32, tilt: Option<(f32, f32)>) {
    send(Event::PenHover(x, y, tilt));
}

/// The mouse on the canvas component.
pub fn mouse(mouse: NativeMouse) {
    send(Event::Mouse(mouse));
}

/// The mouse or the pen came over the canvas component, or left it.
pub fn hover(inside: bool) {
    send(Event::Hover(inside));
}

/// The window has the keys, or lost them.
pub fn focus(focused: bool) {
    send(Event::Focus(focused));
}

/// A key of a hardware keyboard going down (`pressed`) or up.
pub fn key(key: NativeKey, pressed: bool) {
    send(Event::Key(key, pressed));
}

/// Text the soft keyboard typed.
pub fn soft_text(text: String) {
    send(Event::Text(text));
}

/// A key the soft keyboard pressed `count` times: Backspace, Delete or Enter by name.
pub fn soft_key(name: &str, count: u32) {
    let key = match name {
        "Backspace" => egui::Key::Backspace,
        "Delete" => egui::Key::Delete,
        "Enter" => egui::Key::Enter,
        _ => return,
    };
    send(Event::SoftKey(key, count));
}
