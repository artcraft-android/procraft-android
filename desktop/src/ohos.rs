//! The app on HarmonyOS, until it has an interface of its own: the canvas component (an ArkUI
//! XComponent, full screen) is its window, and its native side's callbacks drive it as winit's
//! event loop does on the desktop. harmony/bridge hands them here, on the interface's thread: the
//! surface coming, changing size and going, touches (with the pen's tool, pressure and tilt), the
//! mouse, keys, focus, and the display's frames, at each of which the app draws if it asked to.
//!
//! What the system has and the app doesn't use yet: the soft keyboard (text fields take a hardware
//! keyboard only), the document picker and the pasteboard (`host.rs` says so), a second display.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::raw_window_handle as rwh;

use crate::egui_events::{EguiEvents, Wants};
use crate::native_input::{Held, mouse_events, touch_event};
pub use crate::native_input::{MouseAction, NativeButton, NativeKey, NativeMouse, NativeTouch, Phase, Tool};
use crate::{App, Looper};

/// The display's refresh when the system hasn't said, millihertz.
const REFRESH: u32 = 60_000;

/// The canvas component's surface, as the app's window: the system's native window (handed to
/// wgpu for the surface), its size, the display's density, and whether a frame was asked for.
pub struct Window {
    native: NonNull<c_void>,
    width: AtomicU32,
    height: AtomicU32,
    /// Pixels per vp (ArkUI's unit), which the interface's points are.
    scale: f32,
    redraw: Arc<AtomicBool>,
}

// SAFETY: the native window is only handed to wgpu to make the surface on (which it may do from
// any thread, and HarmonyOS's EGL and Vulkan take it from any); everything else is atomics
#[allow(unsafe_code)]
unsafe impl Send for Window {}
// SAFETY: as for `Send`: nothing is written through the pointer here
#[allow(unsafe_code)]
unsafe impl Sync for Window {}

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

#[allow(unsafe_code)]
impl rwh::HasWindowHandle for Window {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let raw = rwh::RawWindowHandle::OhosNdk(rwh::OhosNdkWindowHandle::new(self.native));
        // SAFETY: the canvas component's native window, alive until its surface goes; the host drops
        // the app's screen (and with it the surface on this window) before it lets the system go on
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

/// Wakes the app from another thread: at the display's next frame it looks again.
pub struct EventLoopProxy<T> {
    wake: Arc<AtomicBool>,
    kind: PhantomData<fn(T)>,
}

impl<T> Clone for EventLoopProxy<T> {
    fn clone(&self) -> EventLoopProxy<T> {
        EventLoopProxy { wake: Arc::clone(&self.wake), kind: PhantomData }
    }
}

impl<T> EventLoopProxy<T> {
    pub fn send_event(&self, _event: T) -> Result<(), T> {
        self.wake.store(true, Ordering::Relaxed);
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
/// (`egui_events.rs`), and what egui asks of the system after each frame.
pub struct EguiState {
    ctx: egui::Context,
    events: EguiEvents,
    started: Instant,
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
        EguiState { ctx, events, started: Instant::now() }
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

    /// A key, which egui takes as egui-winit gives it; true if egui took it.
    fn key(&mut self, key: egui::Key, pressed: bool, repeat: bool) -> bool {
        let wants = self.wants();
        self.events.key(key, pressed, repeat, wants)
    }

    pub fn take_egui_input(&mut self, window: &Window) -> egui::RawInput {
        let size = window.inner_size();
        let time = self.started.elapsed().as_secs_f64();
        self.events.take((size.width, size.height), self.pixels_per_point(window), window.scale, time)
    }

    pub fn handle_platform_output(&mut self, _window: &Window, output: egui::PlatformOutput) {
        self.events.commands(output.commands);
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

/// The app, its window while the surface is there, and its loop.
struct Host {
    app: App,
    window: Option<Arc<Window>>,
    looper: Loop,
    /// A frame asked for (by the app or another thread); cleared as it is drawn.
    wake: Arc<AtomicBool>,
    held: Held,
    /// The keys held down (another press of one is the system repeating it).
    keys_down: Vec<egui::Key>,
    /// Pixels per vp.
    density: f32,
}

thread_local! {
    /// The app, on the interface's thread (where every callback comes).
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

/// Runs `f` on the app, if it is running; a panic in it is logged and ends the app (the system's
/// callback mustn't unwind), the window left as it is.
fn with_host(what: &str, f: impl FnOnce(&mut Host)) {
    HOST.with(|host| {
        let Ok(mut host) = host.try_borrow_mut() else {
            log::warn!("{what}: the app is busy (a callback within a callback)");
            return;
        };
        let Some(running) = host.as_mut() else { return };
        if running.looper.exit.get() {
            return;
        }
        let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(running)));
        if let Err(panic) = done {
            log::error!("{what} panicked: {}", procraft_gpu::panic_message(panic.as_ref()));
            running.looper.exit.set(true);
        }
    });
}

/// Starts the app: its folder is the app's sandbox (`files_dir`, ArkTS's `context.filesDir`), the
/// display `density` pixels per vp. Once, before anything else.
pub fn start(files_dir: PathBuf, density: f32) {
    HOST.with(|host| {
        let Ok(mut host) = host.try_borrow_mut() else { return };
        if host.is_some() {
            return;
        }
        crate::files::set_app_dir(files_dir);
        crate::prepare();
        let wake = Arc::new(AtomicBool::new(true));
        let app = App::new(EventLoopProxy { wake: Arc::clone(&wake), kind: PhantomData });
        let density = if density.is_finite() && density > 0.0 { density } else { 1.0 };
        *host = Some(Host {
            app,
            window: None,
            looper: Loop::default(),
            wake,
            held: Held::default(),
            keys_down: Vec::new(),
            density,
        });
    });
}

/// The canvas component's surface is there: `native` its native window, `width` × `height`
/// pixels. The app makes its screen on it (the first time, and again after the surface went).
pub fn surface_created(native: NonNull<c_void>, width: u32, height: u32) {
    with_host("the surface's coming", |host| {
        let window = Arc::new(Window {
            native,
            width: AtomicU32::new(width),
            height: AtomicU32::new(height),
            scale: host.density,
            redraw: Arc::clone(&host.wake),
        });
        host.window = Some(Arc::clone(&window));
        host.app.start(window, &OwnedDisplayHandle, &host.looper);
    });
}

/// The surface is `width` × `height` pixels now (the device turned, the window split).
pub fn surface_changed(width: u32, height: u32) {
    with_host("the surface's new size", |host| {
        let Some(window) = &host.window else { return };
        window.resized(width, height);
        host.app.on_window_event(&host.looper, WindowEvent::Resized(PhysicalSize::new(width, height)));
    });
}

/// The surface is going (the app went to the background): the artwork open is kept and closed,
/// and the app starts again on the gallery when a surface comes back. (Keeping the device and the
/// canvas across surfaces, as the Android app does, comes later.)
pub fn surface_destroyed() {
    with_host("the surface's going", |host| {
        host.app.on_exiting();
        host.window = None;
        host.looper = Loop::default();
        host.app = App::new(EventLoopProxy { wake: Arc::clone(&host.wake), kind: PhantomData });
    });
}

/// A touch on the canvas component.
pub fn touch(touch: NativeTouch) {
    with_host("a touch", |host| {
        let event = touch_event(&touch);
        host.app.on_window_event(&host.looper, event);
    });
}

/// The mouse on the canvas component.
pub fn mouse(mouse: NativeMouse) {
    with_host("the mouse", |host| {
        for event in mouse_events(&mouse) {
            host.app.on_window_event(&host.looper, event);
        }
    });
}

/// The mouse (or a pen hovering) came over the canvas component, or left it.
pub fn hover(inside: bool) {
    with_host("hovering", |host| {
        let device_id = winit::event::DeviceId::dummy();
        let event =
            if inside { WindowEvent::CursorEntered { device_id } } else { WindowEvent::CursorLeft { device_id } };
        host.app.on_window_event(&host.looper, event);
    });
}

/// The window has the keys, or lost them.
pub fn focus(focused: bool) {
    with_host("focus", |host| host.app.on_window_event(&host.looper, WindowEvent::Focused(focused)));
}

/// A key of a hardware keyboard going down (`pressed`) or up: a modifier changes the modifiers;
/// another goes to egui as a key (and the text it types), and Space and R to the canvas as well,
/// as on the desktop. A key pressed again while it is held is the system repeating it.
pub fn key(key: NativeKey, pressed: bool) {
    with_host("a key", |host| {
        if let Some(event) = host.held.key(key, pressed) {
            host.app.on_window_event(&host.looper, event);
        }
        let NativeKey::Key(key) = key else { return };
        let repeat = pressed && host.keys_down.contains(&key);
        host.keys_down.retain(|k| *k != key);
        if pressed {
            host.keys_down.push(key);
        }
        let app = &mut host.app;
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
    });
}

/// The display's frame: the app looks at what came in meanwhile and when its next frame is due,
/// and draws one if it is due now.
pub fn frame() {
    with_host("a frame", |host| {
        if !host.app.housekeeping(&host.looper) {
            return;
        }
        host.app.pace(&host.looper);
        let due = host.looper.until.get().is_some_and(|until| Instant::now() >= until);
        if host.wake.swap(false, Ordering::Relaxed) || due {
            host.looper.until.set(None);
            host.app.on_window_event(&host.looper, WindowEvent::RedrawRequested);
        }
    });
}

/// Whether the app wants frames: while it is running. (Ended, it waits for the system to close
/// it; the canvas component shows its last frame.)
pub fn running() -> bool {
    HOST.with(|host| host.try_borrow().is_ok_and(|h| h.as_ref().is_some_and(|h| !h.looper.exit.get())))
}
