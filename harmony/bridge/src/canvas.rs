//! The canvas as ArkTS sees it: a [`Session`] (`CanvasSession` there, as in the other apps; ArkUI
//! has a `Canvas` component of its own) owns a core session, draws into the canvas component's
//! surface and takes its touches. Names are camelCase on the ArkTS side (`attachSurface`,
//! `touchDown`...); `harmony/app/entry/types/libprocraft_harmony/index.d.ts` declares them.
// Node-API's registration code, which the `napi` attribute writes, is unsafe; nothing here is.
#![allow(unsafe_code)]

use std::sync::Arc;

use napi_derive_ohos::napi;
use napi_ohos::{Error, Result};
use procraft_core::{CanvasSession, PenSample, PointerKind, TouchCommand};

use crate::pen;

/// One touch sample as ArkTS sends it: pixels on the canvas component's surface (ArkUI's vp times
/// the display's density), pressure 0..1, the pen's tilts along the panel in degrees (0 for a
/// finger), and the event's time in nanoseconds since boot.
#[napi(object)]
pub struct TouchSample {
    pub x: f64,
    pub y: f64,
    pub pressure: f64,
    pub tilt_x: f64,
    pub tilt_y: f64,
    pub time_ns: f64,
}

impl From<&TouchSample> for PenSample {
    fn from(s: &TouchSample) -> PenSample {
        let (tilt, orientation) = pen::lean(s.tilt_x, s.tilt_y);
        PenSample {
            x: s.x as f32,
            y: s.y as f32,
            pressure: pen::pressure(s.pressure),
            tilt,
            orientation,
            time_ms: pen::time_ms(s.time_ns),
            roll: 0.0,
        }
    }
}

fn to_core(from: &[TouchSample]) -> Vec<PenSample> {
    from.iter().map(PenSample::from).collect()
}

/// What the touches ask of the interface, by name ("Undo", "QuickMenu"...): the shell acts on the
/// ones it knows. A typed form comes with the generated bindings.
fn commands(from: Vec<TouchCommand>) -> Vec<String> {
    from.iter()
        .map(|c| format!("{c:?}").split(|ch: char| !ch.is_alphanumeric()).next().unwrap_or_default().to_owned())
        .collect()
}

/// An artwork and its canvas.
#[napi(js_name = "CanvasSession")]
pub struct Session {
    session: Arc<CanvasSession>,
}

#[napi]
impl Session {
    /// A new artwork of `width` × `height` pixels.
    #[napi(constructor)]
    pub fn new(width: u32, height: u32) -> Session {
        Session { session: CanvasSession::new(width, height) }
    }

    /// Starts drawing into the canvas component's surface, `width` × `height` pixels: `surfaceId`
    /// as the XComponent gives it (`onSurfaceCreated`, or its controller's
    /// `getXComponentSurfaceId()`), a number in a string.
    #[napi]
    pub fn attach_surface(&self, surface_id: String, width: u32, height: u32) -> Result<()> {
        let id: u64 =
            surface_id.trim().parse().map_err(|_| Error::from_reason(format!("not a surface id: {surface_id}")))?;
        let window = procraft_core::acquire_surface(id);
        if window == 0 {
            return Err(Error::from_reason(format!("no native window for surface {id}")));
        }
        self.session.attach_window(window, width, height).map_err(|e| Error::from_reason(e.to_string()))
    }

    /// The surface's new size, pixels (`onSurfaceChanged`).
    #[napi]
    pub fn resize(&self, width: u32, height: u32) {
        self.session.resize_window(width, height);
    }

    /// Stops drawing into the surface (`onSurfaceDestroyed`); the artwork stays.
    #[napi]
    pub fn detach(&self) {
        self.session.detach_window();
    }

    /// A finger or pen (`pen`) touching down, its first samples.
    #[napi]
    pub fn touch_down(&self, id: u32, pen: bool, samples: Vec<TouchSample>) -> Vec<String> {
        let kind = if pen { PointerKind::Stylus } else { PointerKind::Finger };
        commands(self.session.touch_down(u64::from(id), kind, to_core(&samples)))
    }

    /// A touch moving: every sample since the last call (ArkUI's historical points, then the
    /// event's own).
    #[napi]
    pub fn touch_move(&self, id: u32, samples: Vec<TouchSample>) -> Vec<String> {
        commands(self.session.touch_move(u64::from(id), to_core(&samples)))
    }

    /// A touch lifting, its last samples.
    #[napi]
    pub fn touch_up(&self, id: u32, samples: Vec<TouchSample>) -> Vec<String> {
        commands(self.session.touch_up(u64::from(id), to_core(&samples)))
    }

    /// Every touch cancelled (the system took them).
    #[napi]
    pub fn touch_cancel(&self) -> Vec<String> {
        commands(self.session.touch_cancel())
    }

    /// Undoes the last change; its name for the notice, if there was one.
    #[napi]
    pub fn undo(&self) -> Option<String> {
        self.session.undo()
    }

    /// Redoes the last change undone; its name, if there was one.
    #[napi]
    pub fn redo(&self) -> Option<String> {
        self.session.redo()
    }

    /// The brush's colour, sRGB 0..1.
    #[napi]
    pub fn set_color(&self, r: f64, g: f64, b: f64) {
        self.session.set_color(r as f32, g as f32, b as f32);
    }

    /// The brush size slider, 0..1.
    #[napi]
    pub fn set_brush_size(&self, slider: f64) {
        self.session.set_brush_size(slider as f32);
    }
}
