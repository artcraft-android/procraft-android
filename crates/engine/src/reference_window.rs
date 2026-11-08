//! An artwork's Reference window: what it shows, how big it is and where. It is kept with the
//! artwork, so each has its own and one that never had a window opens without one.

use serde::{Deserialize, Serialize};

/// The window's two bars together (points): a package's size and middle count them, ours the
/// picture between them.
const BARS: f32 = 64.0 + 45.0;

/// The picture's size until the window is sized (points).
const FIRST_SIZE: [f32; 2] = [350.0, 423.0];

/// The least and the most the window is sized to by its handles (points, its bars included).
const LEAST: [f32; 2] = [282.0, 230.0];
const MOST: [f32; 2] = [650.0, 650.0];

/// The picture's size `size` (points, between the bars) as the window shows it in `room` (the
/// screen, points; not a number: no screen yet): within its least and most, and no bigger than
/// the room while that leaves it its least. One that isn't a number is the first size.
pub fn fitted_size(size: [f32; 2], room: [f32; 2]) -> [f32; 2] {
    let [width, height] = if size.iter().all(|v| v.is_finite()) { size } else { FIRST_SIZE };
    let ([least_w, least_h], [most_w, most_h], [room_w, room_h]) = (LEAST, MOST, room);
    let side = |v: f32, least: f32, most: f32, room: f32| {
        let most = if room.is_finite() { most.min(room) } else { most };
        v.clamp(least, most.max(least))
    };
    [side(width, least_w, most_w, room_w), side(height + BARS, least_h, most_h, room_h) - BARS]
}

/// What the window shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceKind {
    #[default]
    Canvas,
    Image,
    Face,
}

impl ReferenceKind {
    /// Its number in a package; anything else there is the canvas (a model's view among them).
    pub fn of_package(n: i64) -> ReferenceKind {
        match n {
            1 => ReferenceKind::Image,
            2 => ReferenceKind::Face,
            _ => ReferenceKind::Canvas,
        }
    }

    pub fn package(self) -> i64 {
        match self {
            ReferenceKind::Canvas => 0,
            ReferenceKind::Image => 1,
            ReferenceKind::Face => 2,
        }
    }
}

/// The Reference window as an artwork left it. Anything unreadable in it is its default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReferenceWindow {
    pub shown: bool,
    pub kind: ReferenceKind,
    /// The picture's size (points), once it has been sized.
    pub size: Option<[f32; 2]>,
    /// The window's top left corner (points), once it has been placed.
    pub at: Option<[f32; 2]>,
    /// How the canvas and the image each sit in it: zoom, then how far moved.
    pub canvas_view: [f32; 3],
    pub image_view: [f32; 3],
    /// On a model, the canvas kind shows the model itself, turned on its own, not its texture.
    /// Kept as it came on an artwork that isn't a model, which shows the canvas.
    pub three_d: bool,
    /// What a package said that this app doesn't use, kept to be written back as it came.
    pub corner: i64,
    pub docked: bool,
    pub face_background_hidden: bool,
}

impl Default for ReferenceWindow {
    fn default() -> ReferenceWindow {
        ReferenceWindow {
            shown: false,
            kind: ReferenceKind::Canvas,
            size: None,
            at: None,
            canvas_view: [1.0, 0.0, 0.0],
            image_view: [1.0, 0.0, 0.0],
            corner: 0,
            docked: false,
            three_d: false,
            face_background_hidden: false,
        }
    }
}

impl ReferenceWindow {
    /// As one that was never used: nothing to keep.
    pub fn is_unused(&self) -> bool {
        *self == ReferenceWindow::default()
    }

    /// The window's size in a package (points, with its bars): none until it has been sized.
    pub fn package_size(&self) -> [f32; 2] {
        self.size.map_or([0.0; 2], |[w, h]| [w, h + BARS])
    }

    /// The window's middle in a package (points): none until it has been placed.
    pub fn package_middle(&self) -> Option<[f32; 2]> {
        let [w, h] = self.size.unwrap_or(FIRST_SIZE);
        self.at.map(|[x, y]| [x + w / 2.0, y + (h + BARS) / 2.0])
    }

    /// Takes a package's size and middle. Nothing sized is `{0, 0}`, and earlier versions of this
    /// app wrote `{0, 0}` for the middle too, so that isn't a place.
    pub fn set_from_package(&mut self, size: Option<[f64; 2]>, middle: Option<[f64; 2]>) {
        let points = |p: [f64; 2]| p.map(|v| v as f32);
        let usable = |p: &[f32; 2]| p.iter().all(|v| v.is_finite() && v.abs() < 1e6);
        let size = size.map(points).filter(usable).filter(|&[w, h]| w > 0.0 && h > BARS);
        self.size = size.map(|[w, h]| [w, h - BARS]);
        let [w, h] = self.size.unwrap_or(FIRST_SIZE);
        let middle = middle.map(points).filter(usable).filter(|&[x, y]| x != 0.0 || y != 0.0);
        self.at = middle.map(|[x, y]| [x - w / 2.0, y - (h + BARS) / 2.0]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_never_used_has_nothing_to_keep_and_old_text_reads_as_one() {
        assert!(ReferenceWindow::default().is_unused());
        let read: ReferenceWindow = serde_json::from_str("{}").unwrap();
        assert!(read.is_unused());
        assert!(!ReferenceWindow { shown: true, ..ReferenceWindow::default() }.is_unused());
    }

    #[test]
    fn the_window_is_sized_within_its_least_and_most_and_the_screen() {
        let big = [f32::NAN; 2];
        // the first size, and one left at the old least or most
        assert_eq!(fitted_size(FIRST_SIZE, big), [350.0, 423.0]);
        assert_eq!(fitted_size([150.0, 150.0], big), [282.0, 150.0]);
        assert_eq!(fitted_size([300.0, 100.0], big), [300.0, 230.0 - BARS]);
        assert_eq!(fitted_size([700.0, 700.0], big), [650.0, 650.0 - BARS]);
        // a split screen's half: no bigger than it; a screen smaller than the least: the least
        assert_eq!(fitted_size([600.0, 500.0], [530.0, 400.0]), [530.0, 400.0 - BARS]);
        assert_eq!(fitted_size([350.0, 423.0], [200.0, 180.0]), [282.0, 230.0 - BARS]);
        assert_eq!(fitted_size([f32::NAN, 9.0], big), FIRST_SIZE);
        assert_eq!(fitted_size([300.0, 300.0], [f32::INFINITY, -f32::INFINITY]), [300.0, 300.0]);
        assert_eq!(fitted_size([300.0, 300.0], [-5.0, 0.0]), [282.0, 230.0 - BARS]);
    }

    #[test]
    fn a_package_s_kind_numbers_and_an_unknown_one() {
        for kind in [ReferenceKind::Canvas, ReferenceKind::Image, ReferenceKind::Face] {
            assert_eq!(ReferenceKind::of_package(kind.package()), kind);
        }
        assert_eq!(ReferenceKind::of_package(3), ReferenceKind::Canvas);
        assert_eq!(ReferenceKind::of_package(-1), ReferenceKind::Canvas);
    }

    #[test]
    fn a_package_s_size_and_middle_count_the_bars() {
        let window = ReferenceWindow { size: Some([300.0, 200.0]), at: Some([40.0, 60.0]), ..Default::default() };
        assert_eq!(window.package_size(), [300.0, 309.0]);
        assert_eq!(window.package_middle(), Some([190.0, 214.5]));
        let mut back = ReferenceWindow::default();
        back.set_from_package(Some([300.0, 309.0]), Some([190.0, 214.5]));
        assert_eq!((back.size, back.at), (window.size, window.at));
    }

    #[test]
    fn a_package_with_no_size_or_place_leaves_them_unset() {
        let mut window = ReferenceWindow::default();
        window.set_from_package(Some([0.0, 0.0]), Some([0.0, 0.0]));
        assert_eq!((window.size, window.at), (None, None));
        window.set_from_package(None, None);
        assert_eq!((window.size, window.at), (None, None));
        window.set_from_package(Some([f64::NAN, 500.0]), Some([f64::INFINITY, 2.0]));
        assert_eq!((window.size, window.at), (None, None));
        assert_eq!(ReferenceWindow::default().package_size(), [0.0, 0.0]);
        assert_eq!(ReferenceWindow::default().package_middle(), None);
    }
}
