//! Animation Assist: the canvas's top-level layers and groups as the frames of an animation, how
//! it plays, and how the frames around the current one show while drawing (onion skins).

use crate::document::{Document, LayerId};

/// How playing goes on after the last frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Playback {
    /// From the first frame again.
    #[default]
    Loop,
    /// Back to the first and forth again.
    PingPong,
    /// Once, stopping on the last.
    OneShot,
}

/// The most onion skin frames shown on each side ("Max").
pub const MAX_ONION_FRAMES: u32 = 12;

/// Animation Assist's settings for a canvas.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Animation {
    /// Frames a second, 1 to 60.
    pub fps: u32,
    /// Frames shown faintly on each side of the current one, 0 to [`MAX_ONION_FRAMES`].
    pub onion_frames: u32,
    /// The nearest onion skin frames' opacity, 0 to 1 (further ones fade).
    pub onion_opacity: f32,
    pub playback: Playback,
    /// The current frame shows blended with the onion skins instead of over them.
    pub blend_primary: bool,
    /// The colours onion skin frames take: before the current frame and after it (sRGB 0..1).
    pub before: [f32; 3],
    pub after: [f32; 3],
    /// The first frame always shows, behind every other.
    pub background: bool,
    /// The last frame always shows, in front of every other.
    pub foreground: bool,
    /// Frames held for longer: (the frame's layer, how many more frames it shows).
    pub holds: Vec<(LayerId, u32)>,
    /// Page Assist: the frames are pages, one shown at a time (no onion skins, no playing).
    pub pages: bool,
}

impl Default for Animation {
    fn default() -> Animation {
        Animation {
            fps: 15,
            onion_frames: MAX_ONION_FRAMES,
            onion_opacity: 0.6,
            playback: Playback::Loop,
            blend_primary: false,
            before: [0.9, 0.12, 0.12],
            after: [0.12, 0.7, 0.2],
            background: false,
            foreground: false,
            holds: Vec::new(),
            pages: false,
        }
    }
}

impl Animation {
    /// The settings with every value in range.
    pub fn repaired(mut self) -> Animation {
        self.fps = self.fps.clamp(1, 60);
        self.onion_frames = self.onion_frames.min(MAX_ONION_FRAMES);
        self.onion_opacity = if self.onion_opacity.is_finite() { self.onion_opacity.clamp(0.0, 1.0) } else { 0.6 };
        let unit = |c: [f32; 3]| c.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 });
        self.before = unit(self.before);
        self.after = unit(self.after);
        self.holds.retain(|(_, n)| *n > 0);
        self
    }

    /// How many more frames the frame of `id` is held.
    pub fn hold(&self, id: LayerId) -> u32 {
        self.holds.iter().find(|(l, _)| *l == id).map_or(0, |(_, n)| *n)
    }

    pub fn set_hold(&mut self, id: LayerId, frames: u32) {
        self.holds.retain(|(l, _)| *l != id);
        if frames > 0 {
            self.holds.push((id, frames.min(60)));
        }
    }
}

/// How a frame shows while drawing: normally, faintly in a colour, or not at all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FrameLook {
    Shown,
    /// An onion skin: tinted towards `color` and at `opacity`.
    Onion {
        color: [f32; 3],
        opacity: f32,
    },
    Hidden,
}

/// The frames: the visible top-level layers and groups, bottom first.
pub fn frames(doc: &Document) -> Vec<LayerId> {
    doc.members(None).into_iter().filter(|id| doc.layer(*id).is_some_and(|l| l.visible)).collect()
}

/// The frame holding the current layer.
pub fn current_frame(doc: &Document) -> Option<usize> {
    let mut id = doc.current();
    // up to the top level
    for _ in 0..64 {
        match doc.group_of(id) {
            Some(g) => id = g,
            None => break,
        }
    }
    frames(doc).iter().position(|f| *f == id)
}

/// How frame `index` of `count` shows with frame `current` the one drawn on (or, `playing`, the
/// one played: no onion skins then).
pub fn look(a: &Animation, index: usize, count: usize, current: usize, playing: bool) -> FrameLook {
    if a.pages {
        return if index == current { FrameLook::Shown } else { FrameLook::Hidden };
    }
    let last = count.saturating_sub(1);
    if index == current || (a.background && index == 0) || (a.foreground && index == last && count > 1) {
        return FrameLook::Shown;
    }
    if playing {
        return FrameLook::Hidden;
    }
    let distance = index.abs_diff(current);
    let n = a.onion_frames as usize;
    if distance == 0 || distance > n {
        return FrameLook::Hidden;
    }
    // the nearest at the set opacity, fading to nothing past the furthest
    let fade = 1.0 - (distance - 1) as f32 / n as f32;
    let color = if index < current { a.before } else { a.after };
    FrameLook::Onion { color, opacity: a.onion_opacity * fade }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onion_skins_fade_and_take_their_side_colour() {
        let a = Animation { onion_frames: 2, ..Animation::default() };
        assert_eq!(look(&a, 3, 6, 3, false), FrameLook::Shown);
        let FrameLook::Onion { color, opacity } = look(&a, 2, 6, 3, false) else { panic!() };
        assert_eq!(color, a.before);
        assert!((opacity - 0.6).abs() < 1e-6);
        let FrameLook::Onion { color, opacity } = look(&a, 5, 6, 3, false) else { panic!() };
        assert_eq!(color, a.after);
        assert!((opacity - 0.3).abs() < 1e-6);
        assert_eq!(look(&a, 0, 6, 3, false), FrameLook::Hidden);
        // playing, only the played frame (and a background) shows
        assert_eq!(look(&a, 2, 6, 3, true), FrameLook::Hidden);
        let b = Animation { background: true, foreground: true, ..a };
        assert_eq!(look(&b, 0, 6, 3, true), FrameLook::Shown);
        assert_eq!(look(&b, 5, 6, 3, true), FrameLook::Shown);
        // pages: only the current one
        let p = Animation { pages: true, ..b };
        assert_eq!(look(&p, 3, 6, 3, false), FrameLook::Shown);
        assert_eq!(look(&p, 0, 6, 3, false), FrameLook::Hidden);
        assert_eq!(look(&p, 2, 6, 3, false), FrameLook::Hidden);
    }

    #[test]
    fn frames_are_the_top_level_units() {
        let mut d = Document::new(10, 10);
        let a = d.current();
        let b = LayerId(d.add_layer().0);
        assert_eq!(frames(&d), vec![a, b]);
        assert_eq!(current_frame(&d), Some(1));
        d.set_current(a);
        assert_eq!(current_frame(&d), Some(0));
        let mut s = Animation::default();
        s.set_hold(b, 3);
        assert_eq!(s.hold(b), 3);
        s.set_hold(b, 0);
        assert_eq!(s.hold(b), 0);
    }
}
