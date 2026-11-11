//! What a plugin can do to the app (interface 2): app functions it imports from `procraft`, each
//! under a permission its manifest asks for and the import screen shows. A module may import only
//! what its permissions give, with the types written here, or the file is refused. The functions
//! work only while the plugin runs one of its actions (a tap on its card), never inside a hook,
//! which only answers; what an action changes is one step undo takes back.

use std::sync::Arc;

use procraft_engine::document::{LayerId, LayerKind};
use procraft_engine::geom::IRect;
use wasmi::{Caller, Extern, Linker, Memory, Module, StoreLimits, ValType};

use crate::CanvasSession;
use crate::plugin_panel::{PluginPanel, read_panel};

/// What a plugin may ask to do besides answering hooks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PluginPermission {
    /// The artwork's size, its layers and their pixels.
    ArtworkRead,
    /// Adding layers, naming them, how they show, and painting on them.
    ArtworkWrite,
    /// The brush's colour, size and opacity.
    Brush,
    /// A short message.
    Notice,
    /// A panel of its own.
    Panel,
}

impl PluginPermission {
    /// The permission by its name in a manifest.
    pub(crate) fn named(name: &str) -> Option<PluginPermission> {
        match name {
            "artwork.read" => Some(PluginPermission::ArtworkRead),
            "artwork.write" => Some(PluginPermission::ArtworkWrite),
            "brush" => Some(PluginPermission::Brush),
            "notice" => Some(PluginPermission::Notice),
            "panel" => Some(PluginPermission::Panel),
            _ => None,
        }
    }

    /// What it lets a plugin do, for the screen that asks whether to import one.
    pub(crate) fn sentence(self) -> &'static str {
        match self {
            PluginPermission::ArtworkRead => "It can read your artwork: its size, its layers and their pixels.",
            PluginPermission::ArtworkWrite => {
                "It can change your artwork when you tap one of its actions: add layers, name them, set how they show and paint on them, one step undo takes back."
            }
            PluginPermission::Brush => "It can read and set the brush's colour, size and opacity.",
            PluginPermission::Notice => "It can show a short message.",
            PluginPermission::Panel => "It can show a panel of its own when you tap one of its actions.",
        }
    }
}

/// What a running plugin has to hand: the caps on its memory, the artwork while it runs an action
/// (a [`CanvasSession`], held unnamed so the plugin list needn't take on all it holds), and the panel
/// it asked to show or to close meanwhile.
pub(crate) struct Host {
    pub(crate) limits: StoreLimits,
    pub(crate) session: Option<Arc<dyn std::any::Any + Send + Sync>>,
    pub(crate) panel: Option<PluginPanel>,
    pub(crate) close_panel: bool,
}

impl Host {
    pub(crate) fn new(limits: StoreLimits) -> Host {
        Host { limits, session: None, panel: None, close_panel: false }
    }
}

/// The module plugins import the app's functions from.
const FROM: &str = "procraft";
/// The most pixels one read or write moves, and the most bytes of text.
const MOST_PIXELS: u64 = 4096 * 4096;
const MOST_TEXT: usize = 200;
/// The most bytes of a panel's description.
const MOST_PANEL: usize = 16 * 1024;

use ValType::{F32, I32, I64};

/// Each function a plugin may import: its name, the permission it needs, its parameters and results.
const COMMANDS: &[(&str, PluginPermission, &[ValType], &[ValType])] = &[
    ("canvas_size", PluginPermission::ArtworkRead, &[], &[I64]),
    ("layer_count", PluginPermission::ArtworkRead, &[], &[I32]),
    ("layer_at", PluginPermission::ArtworkRead, &[I32], &[I64]),
    ("current_layer", PluginPermission::ArtworkRead, &[], &[I64]),
    ("read_pixels", PluginPermission::ArtworkRead, &[I64, I32, I32, I32, I32, I32], &[I32]),
    ("add_layer", PluginPermission::ArtworkWrite, &[], &[I64]),
    ("write_pixels", PluginPermission::ArtworkWrite, &[I64, I32, I32, I32, I32, I32], &[I32]),
    ("set_layer_name", PluginPermission::ArtworkWrite, &[I64, I32, I32], &[I32]),
    ("set_layer_opacity", PluginPermission::ArtworkWrite, &[I64, F32], &[I32]),
    ("set_layer_visible", PluginPermission::ArtworkWrite, &[I64, I32], &[I32]),
    ("brush_colour", PluginPermission::Brush, &[], &[I32]),
    ("set_brush_colour", PluginPermission::Brush, &[I32], &[]),
    ("brush_size", PluginPermission::Brush, &[], &[F32]),
    ("set_brush_size", PluginPermission::Brush, &[F32], &[]),
    ("brush_opacity", PluginPermission::Brush, &[], &[F32]),
    ("set_brush_opacity", PluginPermission::Brush, &[F32], &[]),
    ("notice", PluginPermission::Notice, &[I32, I32], &[]),
    ("show_panel", PluginPermission::Panel, &[I32, I32], &[I32]),
    ("close_panel", PluginPermission::Panel, &[], &[]),
];

/// Whether everything `module` imports is one of the app's functions `granted` gives, with its
/// types.
pub(crate) fn imports_fit(module: &Module, granted: &[PluginPermission]) -> bool {
    module.imports().all(|import| {
        let found = COMMANDS.iter().find(|(name, ..)| import.module() == FROM && import.name() == *name);
        found.is_some_and(|(_, needs, params, results)| {
            granted.contains(needs)
                && import.ty().func().is_some_and(|func| func.params() == *params && func.results() == *results)
        })
    })
}

/// The artwork the running action works on, if one is running.
fn session(caller: &Caller<'_, Host>) -> Option<Arc<CanvasSession>> {
    caller.data().session.clone()?.downcast::<CanvasSession>().ok()
}

fn memory(caller: &Caller<'_, Host>) -> Option<Memory> {
    caller.get_export("memory").and_then(Extern::into_memory)
}

/// `len` bytes of the module's memory from `at`.
fn read_bytes(caller: &Caller<'_, Host>, at: i32, len: usize) -> Option<Vec<u8>> {
    let memory = memory(caller)?;
    let mut bytes = vec![0_u8; len];
    memory.read(caller, usize::try_from(at).ok()?, &mut bytes).ok()?;
    Some(bytes)
}

/// `len` bytes of text from `at`, if they are some (and not too many).
fn read_text(caller: &Caller<'_, Host>, at: i32, len: i32) -> Option<String> {
    let len = usize::try_from(len).ok().filter(|&n| n <= MOST_TEXT * 4)?;
    let text = String::from_utf8(read_bytes(caller, at, len)?).ok()?;
    let text: String = text.chars().filter(|c| !c.is_control()).take(MOST_TEXT).collect();
    (!text.trim().is_empty()).then_some(text)
}

/// The rectangle (`x`, `y`, `w` × `h`) if it is wholly on a canvas `size` big and not too many
/// pixels; with its size in bytes.
fn rect_on(size: (u32, u32), x: i32, y: i32, w: i32, h: i32) -> Option<(IRect, usize)> {
    let (w, h) = (u32::try_from(w).ok().filter(|&w| w > 0)?, u32::try_from(h).ok().filter(|&h| h > 0)?);
    if u64::from(w) * u64::from(h) > MOST_PIXELS {
        return None;
    }
    let (x1, y1) = (x.checked_add(i32::try_from(w).ok()?)?, y.checked_add(i32::try_from(h).ok()?)?);
    let rect = IRect::new(x, y, x1, y1);
    let whole = IRect::from_size(size.0, size.1);
    (rect.intersect(whole) == rect).then_some((rect, w as usize * h as usize * 4))
}

/// A layer that can be painted on: a pixel layer, not locked.
fn paintable(session: &CanvasSession, id: u64) -> bool {
    session.layers().iter().any(|l| l.id == id && !l.locked && l.kind == crate::layers::LayerType::Paint)
}

/// A plugin's change of the artwork in progress (see [`CanvasSession::plugin_step`]): what it
/// changed becomes one step when this goes, however the plugin finished.
pub(crate) struct PluginStep<'a>(&'a CanvasSession);

impl Drop for PluginStep<'_> {
    fn drop(&mut self) {
        self.0.end_plugin_step();
    }
}

impl CanvasSession {
    /// Marks the history before a plugin's action or panel touch: until what this gives goes, no
    /// stroke begins and undo and redo wait (they would join the plugin's step).
    pub(crate) fn plugin_step(&self) -> PluginStep<'_> {
        let mut st = self.shared.lock();
        st.finish_stroke();
        // (one inside another, an event told during an action, joins its step)
        if st.plugin_stepping == 0
            && let Some(canvas) = st.canvas.as_mut()
        {
            canvas.mark_steps();
        }
        st.plugin_stepping += 1;
        drop(st);
        PluginStep(self)
    }

    /// What a plugin's action changed becomes one step undo takes back.
    fn end_plugin_step(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.plugin_stepping = st.plugin_stepping.saturating_sub(1);
        if st.plugin_stepping == 0
            && let Some(canvas) = st.canvas.as_mut()
        {
            canvas.collapse_steps();
        }
        self.shared.frame(st);
    }

    /// `part` (premultiplied RGBA8, its rows) written over layer `id` inside `rect`.
    fn plugin_write(&self, id: u64, rect: IRect, part: Vec<u8>) -> bool {
        if !paintable(self, id) {
            return false;
        }
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.doc.layer(LayerId(id)).is_none_or(|l| l.kind != LayerKind::Paint) {
            return false;
        }
        st.rewrite_layer(LayerId(id), rect, part);
        self.shared.frame(st);
        true
    }
}

/// The app's functions `granted` gives, put in `linker` for a plugin's module to import.
pub(crate) fn link(linker: &mut Linker<Host>, granted: &[PluginPermission]) -> Result<(), wasmi::Error> {
    let read = granted.contains(&PluginPermission::ArtworkRead);
    let write = granted.contains(&PluginPermission::ArtworkWrite);
    if read {
        linker.func_wrap(FROM, "canvas_size", |caller: Caller<'_, Host>| -> i64 {
            session(&caller).map_or(0, |s| {
                let info = s.info();
                (u64::from(info.width) | (u64::from(info.height) << 32)) as i64
            })
        })?;
        linker.func_wrap(FROM, "layer_count", |caller: Caller<'_, Host>| -> i32 {
            session(&caller).map_or(0, |s| i32::try_from(s.layers().len()).unwrap_or(i32::MAX))
        })?;
        linker.func_wrap(FROM, "layer_at", |caller: Caller<'_, Host>, index: i32| -> i64 {
            let found = session(&caller).and_then(|s| s.layers().get(usize::try_from(index).ok()?).map(|l| l.id));
            found.map_or(0, |id| id as i64)
        })?;
        linker.func_wrap(FROM, "current_layer", |caller: Caller<'_, Host>| -> i64 {
            let found = session(&caller).and_then(|s| s.layers().iter().find(|l| l.current).map(|l| l.id));
            found.map_or(0, |id| id as i64)
        })?;
        linker.func_wrap(
            FROM,
            "read_pixels",
            |mut caller: Caller<'_, Host>, layer: i64, x: i32, y: i32, w: i32, h: i32, to: i32| -> i32 {
                let Some(s) = session(&caller) else { return 0 };
                let info = s.info();
                let Some((rect, _)) = rect_on((info.width, info.height), x, y, w, h) else { return 0 };
                let Some(pixels) = s.shared.lock().layer_part(LayerId(layer as u64), rect) else { return 0 };
                let (Some(memory), Ok(at)) = (memory(&caller), usize::try_from(to)) else { return 0 };
                i32::from(memory.write(&mut caller, at, &pixels).is_ok())
            },
        )?;
    }
    if write {
        linker.func_wrap(FROM, "add_layer", |caller: Caller<'_, Host>| -> i64 {
            session(&caller).map_or(0, |s| s.add_layer_quietly() as i64)
        })?;
        linker.func_wrap(
            FROM,
            "write_pixels",
            |caller: Caller<'_, Host>, layer: i64, x: i32, y: i32, w: i32, h: i32, from: i32| -> i32 {
                let Some(s) = session(&caller) else { return 0 };
                let info = s.info();
                let Some((rect, bytes)) = rect_on((info.width, info.height), x, y, w, h) else { return 0 };
                let Some(part) = read_bytes(&caller, from, bytes) else { return 0 };
                i32::from(s.plugin_write(layer as u64, rect, part))
            },
        )?;
        linker.func_wrap(FROM, "set_layer_name", |caller: Caller<'_, Host>, layer: i64, at: i32, len: i32| -> i32 {
            let (Some(s), Some(name)) = (session(&caller), read_text(&caller, at, len)) else { return 0 };
            i32::from(s.rename_layer(layer as u64, name))
        })?;
        linker.func_wrap(FROM, "set_layer_opacity", |caller: Caller<'_, Host>, layer: i64, opacity: f32| -> i32 {
            let Some(s) = session(&caller).filter(|_| opacity.is_finite()) else { return 0 };
            i32::from(s.set_layer_opacity(layer as u64, opacity.clamp(0.0, 1.0), true))
        })?;
        linker.func_wrap(FROM, "set_layer_visible", |caller: Caller<'_, Host>, layer: i64, on: i32| -> i32 {
            session(&caller).map_or(0, |s| i32::from(s.set_layer_visible(layer as u64, on != 0)))
        })?;
    }
    if granted.contains(&PluginPermission::Brush) {
        linker.func_wrap(FROM, "brush_colour", |caller: Caller<'_, Host>| -> i32 {
            session(&caller).map_or(0, |s| {
                let c = s.color();
                let byte = |i: usize| (c.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as i32;
                (byte(0) << 16) | (byte(1) << 8) | byte(2)
            })
        })?;
        linker.func_wrap(FROM, "set_brush_colour", |caller: Caller<'_, Host>, rgb: i32| {
            if let Some(s) = session(&caller) {
                let part = |shift: i32| ((rgb >> shift) & 0xFF) as f32 / 255.0;
                s.set_color(part(16), part(8), part(0));
            }
        })?;
        linker.func_wrap(FROM, "brush_size", |caller: Caller<'_, Host>| -> f32 {
            session(&caller).map_or(0.0, |s| s.info().size)
        })?;
        linker.func_wrap(FROM, "set_brush_size", |caller: Caller<'_, Host>, size: f32| {
            if let Some(s) = session(&caller) {
                s.set_brush_size(size);
            }
        })?;
        linker.func_wrap(FROM, "brush_opacity", |caller: Caller<'_, Host>| -> f32 {
            session(&caller).map_or(0.0, |s| s.info().opacity)
        })?;
        linker.func_wrap(FROM, "set_brush_opacity", |caller: Caller<'_, Host>, opacity: f32| {
            if let Some(s) = session(&caller) {
                s.set_brush_opacity(opacity);
            }
        })?;
    }
    if granted.contains(&PluginPermission::Panel) {
        // shown once the action or the touch ends (one that doesn't hold up shows nothing: 0)
        linker.func_wrap(FROM, "show_panel", |mut caller: Caller<'_, Host>, at: i32, len: i32| -> i32 {
            let Some(len) = usize::try_from(len).ok().filter(|&n| n <= MOST_PANEL) else { return 0 };
            let read = read_bytes(&caller, at, len).and_then(|b| String::from_utf8(b).ok());
            let Some(panel) = read.as_deref().and_then(read_panel) else { return 0 };
            let host = caller.data_mut();
            host.panel = Some(panel);
            host.close_panel = false;
            1
        })?;
        linker.func_wrap(FROM, "close_panel", |mut caller: Caller<'_, Host>| {
            let host = caller.data_mut();
            host.panel = None;
            host.close_panel = true;
        })?;
    }
    if granted.contains(&PluginPermission::Notice) {
        linker.func_wrap(FROM, "notice", |caller: Caller<'_, Host>, at: i32, len: i32| {
            if let (Some(s), Some(text)) = (session(&caller), read_text(&caller, at, len)) {
                let hooks = s.shared.lock().hooks.clone();
                if let Some(hooks) = hooks {
                    hooks.say(&text);
                }
            }
        })?;
    }
    Ok(())
}
