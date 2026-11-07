//! The Reference window as the artwork keeps it: the session holds what the artwork left and saves
//! it with the artwork, and each app shows it and says what changes. Apart from the document, so
//! that undoing never moves the window.

use std::io::Cursor;
use std::sync::Arc;

use procraft_engine::reference_window::{ReferenceKind, ReferenceWindow};

use crate::{CanvasSession, ReferenceView};

/// The window's imported picture, beside the artwork's layers.
pub(crate) const REFERENCE_PICTURE: &str = "reference.png";

/// What the window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ReferenceShows {
    Canvas,
    Image,
    Face,
}

/// The window as an app shows it, in points. Until it has been `sized` or `placed` the app chooses
/// (the numbers then mean nothing).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct ReferenceWindowState {
    pub shown: bool,
    pub shows: ReferenceShows,
    pub sized: bool,
    pub width: f32,
    pub height: f32,
    pub placed: bool,
    pub x: f32,
    pub y: f32,
    pub canvas_view: ReferenceView,
    pub image_view: ReferenceView,
}

/// A place for the window, in points.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct ReferencePlace {
    pub x: f32,
    pub y: f32,
}

fn view_of([zoom, x, y]: [f32; 3]) -> ReferenceView {
    ReferenceView { zoom, x, y }
}

fn kept_view(v: ReferenceView) -> [f32; 3] {
    if [v.zoom, v.x, v.y].iter().all(|n| n.is_finite()) { [v.zoom, v.x, v.y] } else { [1.0, 0.0, 0.0] }
}

fn state_of(w: &ReferenceWindow) -> ReferenceWindowState {
    let [width, height] = w.size.unwrap_or_default();
    let [x, y] = w.at.unwrap_or_default();
    ReferenceWindowState {
        shown: w.shown,
        shows: match w.kind {
            ReferenceKind::Canvas => ReferenceShows::Canvas,
            ReferenceKind::Image => ReferenceShows::Image,
            ReferenceKind::Face => ReferenceShows::Face,
        },
        sized: w.size.is_some(),
        width,
        height,
        placed: w.at.is_some(),
        x,
        y,
        canvas_view: view_of(w.canvas_view),
        image_view: view_of(w.image_view),
    }
}

/// `w` as `state` says, what only a file said staying as it was.
fn take_state(w: &mut ReferenceWindow, state: ReferenceWindowState) {
    let pair = |on: bool, a: f32, b: f32| (on && a.is_finite() && b.is_finite()).then_some([a, b]);
    w.shown = state.shown;
    w.kind = match state.shows {
        ReferenceShows::Canvas => ReferenceKind::Canvas,
        ReferenceShows::Image => ReferenceKind::Image,
        ReferenceShows::Face => ReferenceKind::Face,
    };
    w.size = pair(state.sized, state.width, state.height);
    w.at = pair(state.placed, state.x, state.y);
    w.canvas_view = kept_view(state.canvas_view);
    w.image_view = kept_view(state.image_view);
}

/// `png` as it shows: a package's picture is kept with the turn it is to be shown at, one of the
/// system's eight orientations (0 is upright). One that can't be read stays as it is.
pub(crate) fn upright(png: Vec<u8>, orientation: i64) -> Vec<u8> {
    if !(1..=7).contains(&orientation) {
        return png;
    }
    let Ok(picture) = image::load_from_memory(&png) else { return png };
    let turned = match orientation {
        1 => picture.rotate180(),
        2 => picture.rotate270(),
        3 => picture.rotate90(),
        4 => picture.fliph(),
        5 => picture.flipv(),
        6 => picture.fliph().rotate270(),
        _ => picture.fliph().rotate90(),
    };
    let mut out = Cursor::new(Vec::new());
    match turned.write_to(&mut out, image::ImageFormat::Png) {
        Ok(()) => out.into_inner(),
        Err(_) => png,
    }
}

/// A window's size, in points: its width, and its picture's height between the bars.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct ReferenceSize {
    pub width: f32,
    pub height: f32,
}

/// The size a window `width` wide, its picture `height` tall between the bars, shows at on a
/// screen `screen_width` x `screen_height` (points; not a number: no screen yet): 282 to 650 wide
/// and 230 to 650 tall with its bars, and no bigger than the screen while that leaves it that.
#[uniffi::export]
pub fn reference_window_size(width: f32, height: f32, screen_width: f32, screen_height: f32) -> ReferenceSize {
    let [width, height] =
        procraft_engine::reference_window::fitted_size([width, height], [screen_width, screen_height]);
    ReferenceSize { width, height }
}

/// Where a window `width` x `tall` at (`x`, `y`) goes in a screen `screen_width` x `screen_height`,
/// so that it can be reached: all of it shown (the top left of it, if it is bigger). An artwork
/// may have left it on a screen of another shape.
#[uniffi::export]
pub fn reference_window_fitted(
    x: f32,
    y: f32,
    width: f32,
    tall: f32,
    screen_width: f32,
    screen_height: f32,
) -> ReferencePlace {
    let inside = |at: f32, size: f32, room: f32| {
        let most = (room - size).max(0.0);
        if at.is_finite() { at.clamp(0.0, most) } else { 0.0 }
    };
    ReferencePlace { x: inside(x, width, screen_width), y: inside(y, tall, screen_height) }
}

#[uniffi::export]
impl CanvasSession {
    /// The Reference window as this artwork left it.
    pub fn reference_window(&self) -> ReferenceWindowState {
        state_of(&self.shared.lock().reference_window)
    }

    /// The window has changed: kept with the artwork, saved with it (a change being an edit, the
    /// autosave has it).
    pub fn set_reference_window(&self, state: ReferenceWindowState) {
        let mut st = self.shared.lock();
        let mut next = st.reference_window.clone();
        take_state(&mut next, state);
        if next != st.reference_window {
            st.reference_window = next;
            st.revision += 1;
        }
    }

    /// The window's imported picture, a PNG; none if it has none.
    pub fn reference_picture(&self) -> Option<Vec<u8>> {
        self.shared.lock().reference_picture.as_deref().cloned()
    }

    /// `png` is now the window's imported picture.
    pub fn set_reference_picture(&self, png: Vec<u8>) {
        let mut st = self.shared.lock();
        st.reference_picture = Some(Arc::new(png));
        st.revision += 1;
    }

    /// The window's imported picture goes.
    pub fn clear_reference_picture(&self) {
        let mut st = self.shared.lock();
        if st.reference_picture.take().is_some() {
            st.revision += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ReferenceWindowState {
        ReferenceWindowState {
            shown: true,
            shows: ReferenceShows::Image,
            sized: true,
            width: 300.0,
            height: 200.0,
            placed: true,
            x: 40.0,
            y: 60.0,
            canvas_view: ReferenceView { zoom: 2.0, x: 5.0, y: 6.0 },
            image_view: ReferenceView { zoom: 3.0, x: 7.0, y: 8.0 },
        }
    }

    /// A folder of its own for test `name`, empty.
    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("procraft-reference-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn path(dir: &std::path::Path) -> String {
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn the_window_and_its_picture_come_back_with_the_artwork() {
        let dir = folder("store");
        let s = CanvasSession::new(64, 48);
        let before = s.edits();
        s.set_reference_window(state());
        s.set_reference_picture(b"a picture".to_vec());
        // a change the autosave sees
        assert_ne!(s.edits(), before);
        s.save(path(&dir)).unwrap();
        assert!(dir.join(REFERENCE_PICTURE).exists());
        let opened = CanvasSession::open(path(&dir)).unwrap();
        assert_eq!(opened.reference_window(), state());
        assert_eq!(opened.reference_picture().as_deref(), Some(&b"a picture"[..]));
        // the same again: nothing changes, nothing is counted
        let edits = opened.edits();
        opened.set_reference_window(state());
        assert_eq!(opened.edits(), edits);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_artwork_that_never_had_a_window_opens_with_none_as_older_ones_do() {
        let dir = folder("never");
        let s = CanvasSession::new(64, 48);
        s.save(path(&dir)).unwrap();
        let json = std::fs::read_to_string(dir.join("artwork.json")).unwrap();
        let kept: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(kept.get("reference").is_none(), "{json}");
        let opened = CanvasSession::open(path(&dir)).unwrap();
        assert!(!opened.reference_window().shown && !opened.reference_window().sized);
        assert_eq!(opened.reference_picture(), None);
        // one whose window can't be read opens as one that never had it
        let broken = json.replacen('{', "{\"reference\":{\"shown\":\"yes\"},", 1);
        std::fs::write(dir.join("artwork.json"), broken).unwrap();
        let opened = CanvasSession::open(path(&dir)).unwrap();
        assert!(!opened.reference_window().shown);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_artworks_keep_windows_of_their_own() {
        let (one, two) = (folder("one"), folder("two"));
        let a = CanvasSession::new(64, 48);
        a.set_reference_window(state());
        a.save(path(&one)).unwrap();
        let b = CanvasSession::new(64, 48);
        let other = ReferenceWindowState { shows: ReferenceShows::Canvas, x: 500.0, ..state() };
        b.set_reference_window(other);
        b.save(path(&two)).unwrap();
        assert_eq!(CanvasSession::open(path(&one)).unwrap().reference_window(), state());
        assert_eq!(CanvasSession::open(path(&two)).unwrap().reference_window(), other);
        let _ = std::fs::remove_dir_all(&one);
        let _ = std::fs::remove_dir_all(&two);
    }

    #[test]
    fn a_picture_taken_away_leaves_the_folder_with_the_next_save() {
        let dir = folder("cleared");
        let s = CanvasSession::new(64, 48);
        s.set_reference_picture(b"a picture".to_vec());
        s.save(path(&dir)).unwrap();
        s.clear_reference_picture();
        s.save(path(&dir)).unwrap();
        assert!(!dir.join(REFERENCE_PICTURE).exists());
        assert_eq!(CanvasSession::open(path(&dir)).unwrap().reference_picture(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_package_s_window_and_picture_come_in_upright() {
        use procraft_engine::document::Document;
        let mut png = Cursor::new(Vec::new());
        let two = image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
        image::DynamicImage::ImageRgba8(two).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let window = ReferenceWindow {
            shown: true,
            kind: ReferenceKind::Image,
            size: Some([300.0, 200.0]),
            at: Some([40.0, 60.0]),
            ..ReferenceWindow::default()
        };
        let doc = Document::new(64, 48);
        let bytes = procraft_engine::native::write(&procraft_engine::native::Export {
            document: &doc,
            pixels: &std::collections::HashMap::new(),
            composite: &[255; 64 * 48 * 4],
            thumbnail_png: &[],
            name: "Package",
            author: "",
            strokes: 0,
            tracked_seconds: 0.0,
            seed: 3,
            time_lapse: None,
            reference: Some(&window),
            reference_picture: Some(png.get_ref()),
        });
        let package = std::env::temp_dir().join(format!("procraft-reference-package-{}.procraft", std::process::id()));
        std::fs::write(&package, &bytes).unwrap();
        let s = CanvasSession::open_package(package.to_string_lossy().into_owned()).unwrap();
        assert_eq!(
            s.reference_window(),
            ReferenceWindowState {
                shows: ReferenceShows::Image,
                canvas_view: ReferenceView::default(),
                image_view: ReferenceView::default(),
                ..state()
            }
        );
        let shown = image::load_from_memory(&s.reference_picture().unwrap()).unwrap();
        assert_eq!((shown.width(), shown.height()), (2, 1));
        let _ = std::fs::remove_file(&package);
    }

    #[test]
    fn a_state_comes_back_as_it_was_set_and_what_a_file_said_stays() {
        let mut window = ReferenceWindow { corner: 3, docked: true, ..ReferenceWindow::default() };
        take_state(&mut window, state());
        assert_eq!(state_of(&window), state());
        assert_eq!((window.corner, window.docked), (3, true));
    }

    #[test]
    fn numbers_that_mean_nothing_are_not_kept() {
        let mut window = ReferenceWindow::default();
        let unset = ReferenceWindowState { sized: false, placed: false, ..state() };
        take_state(&mut window, unset);
        assert_eq!((window.size, window.at), (None, None));
        let broken = ReferenceWindowState {
            width: f32::NAN,
            x: f32::INFINITY,
            image_view: ReferenceView { zoom: f32::NAN, x: 0.0, y: 0.0 },
            ..state()
        };
        take_state(&mut window, broken);
        assert_eq!((window.size, window.at), (None, None));
        assert_eq!(window.image_view, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_window_left_off_a_smaller_screen_comes_back_on_it() {
        let place = |x, y| reference_window_fitted(x, y, 350.0, 532.0, 1000.0, 600.0);
        assert_eq!(place(100.0, 20.0), ReferencePlace { x: 100.0, y: 20.0 });
        assert_eq!(place(900.0, 500.0), ReferencePlace { x: 650.0, y: 68.0 });
        assert_eq!(place(-40.0, -9.0), ReferencePlace { x: 0.0, y: 0.0 });
        assert_eq!(place(f32::NAN, f32::INFINITY), ReferencePlace { x: 0.0, y: 0.0 });
        let big = reference_window_fitted(50.0, 50.0, 800.0, 900.0, 600.0, 400.0);
        assert_eq!(big, ReferencePlace { x: 0.0, y: 0.0 });
    }

    #[test]
    fn a_picture_is_turned_upright_and_a_bad_one_left_alone() {
        // two pixels side by side, red then blue, kept turned a quarter clockwise to show upright
        let mut png = Cursor::new(Vec::new());
        let two = image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
        image::DynamicImage::ImageRgba8(two).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let png = png.into_inner();
        assert_eq!(upright(png.clone(), 0), png);
        assert_eq!(upright(png.clone(), 9), png);
        let turned = image::load_from_memory(&upright(png, 3)).unwrap().to_rgba8();
        assert_eq!(turned.dimensions(), (1, 2));
        assert_eq!(turned.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(upright(b"not a picture".to_vec(), 3), b"not a picture");
    }
}
