//! A hardware keyboard's presses, and what a plugin keeps: the `key_shortcut` hook, which takes each
//! press on the canvas as another before the app's shortcuts see it ([`key_shortcut!`], told the
//! app's shortcuts with [`key_shortcuts!`]); a panel that asks for the next key press
//! ([`panel_key!`], with [`crate::app::Panel::key_press`]); and what a plugin keeps from one start
//! to the next ([`kept!`] and [`keep`]).
//!
//! ```ignore
//! kit::key_shortcut!(
//!     |press| if press.text() == "q" { Taken::As(Press::read("command+z").unwrap()) } else { Taken::AsItIs },
//!     |press| (press.text() == "command+z").then(|| vec![Press::read("q").unwrap()])
//! );
//! ```

use std::sync::Mutex;

use crate::with_std::Place;

/// A key pressed with modifiers, as the app names it: the key ("z", "[", "space", "delete",
/// "left", "f5", "plus") and whether the command key (Ctrl, ⌘), Shift and Alt (⌥) are held.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Press {
    pub key: String,
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Press {
    /// `key` pressed alone.
    pub fn new(key: &str) -> Press {
        Press { key: key.to_owned(), command: false, shift: false, alt: false }
    }

    /// The press as the app writes it: its modifiers in this order, each with a `+` after it, then its
    /// key ("command+shift+z").
    pub fn text(&self) -> String {
        let mods = [(self.command, "command+"), (self.shift, "shift+"), (self.alt, "alt+")];
        let mut text: String = mods.iter().filter(|(on, _)| *on).map(|(_, m)| *m).collect();
        text.push_str(&self.key);
        text
    }

    /// The press `text` writes (its modifiers in any order), if it is one the app takes: a key of a
    /// few printable characters, lowercase, without `+` or a space.
    pub fn read(text: &str) -> Option<Press> {
        let (mods, key) = text.rsplit_once('+').unwrap_or(("", text));
        let mut press = Press::new(key);
        for m in mods.split('+').filter(|_| !mods.is_empty()) {
            let held = match m {
                "command" => &mut press.command,
                "shift" => &mut press.shift,
                "alt" => &mut press.alt,
                _ => return None,
            };
            if std::mem::replace(held, true) {
                return None;
            }
        }
        let fits = |c: char| c.is_ascii_graphic() && !c.is_ascii_uppercase() && c != '+';
        ((1..=16).contains(&press.key.len()) && press.key.chars().all(fits)).then_some(press)
    }

    /// Presses written with a space between, each that reads as one.
    pub fn read_all(text: &str) -> Vec<Press> {
        text.split(' ').filter_map(Press::read).collect()
    }

    /// The same press without Shift.
    pub fn without_shift(&self) -> Press {
        Press { shift: false, ..self.clone() }
    }
}

/// One of the app's shortcuts, as it tells them: its group ("Edit"), what it does, the presses that
/// do it by default, and whether the app keeps them as they are (`fixed`: held, they move the view,
/// say), which no plugin rebinds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub group: String,
    pub does: String,
    pub presses: Vec<Press>,
    pub fixed: bool,
}

/// The app's shortcuts as `text` tells them (a line each, its fields a tab apart), each line that
/// reads as one.
pub fn read_shortcuts(text: &str) -> Vec<Shortcut> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let (group, does, presses, fixed) = (fields.next()?, fields.next()?, fields.next()?, fields.next()?);
            Some(Shortcut {
                group: group.to_owned(),
                does: does.to_owned(),
                presses: Press::read_all(presses),
                fixed: fixed == "1",
            })
        })
        .collect()
}

/// What a press is taken as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Taken {
    /// As it is: what it always did.
    AsItIs,
    /// As this other press: what that does by default.
    As(Press),
    /// As no press: it does nothing.
    Nothing,
}

/// Where the app writes a press, or its shortcuts, for the key hook; and where answers go out.
pub static KEY: Place = Place::new();
pub static ANSWER: Place = Place::new();
/// What a plugin is told it keeps, and what it hands the app to keep.
pub static KEPT: Place = Place::new();
pub static KEEPING: Place = Place::new();
static TO_KEEP: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// `bytes` kept for the app to read, packed as it reads an answer: where (low half), how long (high).
fn packed(place: &Place, bytes: Vec<u8>) -> i64 {
    let len = bytes.len() as u64;
    let at = place.put(bytes) as u32 as u64;
    ((len << 32) | at) as i64
}

/// The text the app wrote into [`KEY`], `len` bytes of it.
pub fn told_text(len: i32) -> String {
    let bytes = KEY.take();
    bytes.get(..len.max(0) as usize).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default()
}

/// The press the app wrote into [`KEY`].
pub fn told_press(len: i32) -> Option<Press> {
    Press::read(&told_text(len))
}

/// `taken` as the app reads it.
pub fn taken_answer(taken: Taken) -> i64 {
    match taken {
        Taken::AsItIs => 0,
        Taken::Nothing => -1,
        Taken::As(press) => packed(&ANSWER, press.text().into_bytes()),
    }
}

/// `presses` (none: as it is) as the app reads them.
pub fn keys_answer(presses: Option<Vec<Press>>) -> i64 {
    match presses {
        None => 0,
        Some(presses) if presses.is_empty() => -1,
        Some(presses) => {
            let text: Vec<String> = presses.iter().map(Press::text).collect();
            packed(&ANSWER, text.join(" ").into_bytes())
        }
    }
}

/// `data` kept from now on: the app asks for it once the action, the touch on the panel or the key
/// press it asked for ends, keeps it with the plugin (64 KiB at most), and tells it to every copy of
/// the plugin's code as it starts. It needs [`kept!`].
pub fn keep(data: Vec<u8>) {
    *TO_KEEP.lock().unwrap_or_else(|e| e.into_inner()) = Some(data);
}

/// What [`keep`] was last handed, as the app reads it (below 0: nothing new).
pub fn keep_answer() -> i64 {
    match TO_KEEP.lock().unwrap_or_else(|e| e.into_inner()).take() {
        Some(data) => packed(&KEEPING, data),
        None => -1,
    }
}

/// Exports where the app writes a press (or its shortcuts) for a plugin: for one whose panel asks
/// for a key press without being on the key hook ([`key_shortcut!`] has it).
#[macro_export]
macro_rules! key_buffer {
    () => {
        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_key_buffer(len: i32) -> i32 {
            $crate::keys::KEY.make(len)
        }
    };
}

/// Exports the key hook: `$taken` (a `fn(&kit::keys::Press) -> kit::keys::Taken`) says what a press
/// on the canvas is taken as, and `$keys` (a `fn(&Press) -> Option<Vec<Press>>`) which presses now
/// do what a press does by default, for the shortcuts' list (none: still that press).
#[macro_export]
macro_rules! key_shortcut {
    ($taken:expr, $keys:expr) => {
        $crate::key_buffer!();

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_key_shortcut(len: i32) -> i64 {
            let taken: fn(&$crate::keys::Press) -> $crate::keys::Taken = $taken;
            $crate::keys::told_press(len).map_or(0, |press| $crate::keys::taken_answer(taken(&press)))
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_key_shortcut_keys(len: i32) -> i64 {
            let keys: fn(&$crate::keys::Press) -> Option<Vec<$crate::keys::Press>> = $keys;
            $crate::keys::told_press(len).map_or(0, |press| $crate::keys::keys_answer(keys(&press)))
        }
    };
}

/// Exports what the app tells its shortcuts through, as the plugin starts: `$told` (a
/// `fn(Vec<kit::keys::Shortcut>)`). For a plugin on the key hook.
#[macro_export]
macro_rules! key_shortcuts {
    ($told:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_key_shortcuts(len: i32) {
            let told: fn(Vec<$crate::keys::Shortcut>) = $told;
            told($crate::keys::read_shortcuts(&$crate::keys::told_text(len)));
        }
    };
}

/// Exports `procraft_panel_key`: while the plugin's panel asks for a key press, the next one calls
/// `$pressed` (a `fn(kit::keys::Press)`), which acts as a touch on the panel does. The panel asked
/// for that press alone: show it again asking for another.
#[macro_export]
macro_rules! panel_key {
    ($pressed:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_panel_key(len: i32) -> i32 {
            let pressed: fn($crate::keys::Press) = $pressed;
            if let Some(press) = $crate::keys::told_press(len) {
                pressed(press);
            }
            0
        }
    };
}

/// Exports what a plugin keeps goes through: `$told` (a `fn(&[u8])`) is told it as each copy of its
/// code starts, and whenever another copy keeps something new; [`keep`] hands the app something new.
#[macro_export]
macro_rules! kept {
    ($told:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_kept_buffer(len: i32) -> i32 {
            $crate::keys::KEPT.make(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_kept(len: i32) {
            let told: fn(&[u8]) = $told;
            let data = $crate::keys::KEPT.take();
            told(data.get(..len.max(0) as usize).unwrap_or_default());
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_keep() -> i64 {
            $crate::keys::keep_answer()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_is_written_and_read_as_the_app_does() {
        let redo = Press { key: "z".into(), command: true, shift: true, alt: false };
        assert_eq!(redo.text(), "command+shift+z");
        assert_eq!(Press::read("shift+command+z"), Some(redo));
        assert_eq!(Press::read("space"), Some(Press::new("space")));
        assert_eq!(Press::read("command+plus").map(|p| p.key), Some("plus".into()));
        for wrong in ["", "Z", "ctrl+z", "command+command+z", "command+", "a b", "command++"] {
            assert_eq!(Press::read(wrong), None, "{wrong}");
        }
        let read = Press::read_all("command+z  shift+] nonsense+x");
        assert_eq!(read, [Press::read("command+z").unwrap(), Press { shift: true, ..Press::new("]") }]);
    }

    #[test]
    fn the_app_s_shortcuts_are_read_a_line_each() {
        let told = "Edit\tUndo\tcommand+z\t0\nView\tZoom\tcommand+space\t1\nbroken line\n";
        let list = read_shortcuts(told);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].does, "Undo");
        assert_eq!(list[0].presses, [Press::read("command+z").unwrap()]);
        assert!(!list[0].fixed && list[1].fixed);
    }

    #[test]
    fn answers_are_packed_as_the_app_reads_them() {
        assert_eq!(taken_answer(Taken::AsItIs), 0);
        assert_eq!(taken_answer(Taken::Nothing), -1);
        assert_eq!(keys_answer(None), 0);
        assert_eq!(keys_answer(Some(Vec::new())), -1);
        let packed = taken_answer(Taken::As(Press::read("command+y").unwrap()));
        assert_eq!(packed as u64 >> 32, "command+y".len() as u64);
        // nothing to keep until something is handed over, then that once
        assert_eq!(keep_answer(), -1);
        keep(b"kept".to_vec());
        assert_eq!(keep_answer() as u64 >> 32, 4);
        assert_eq!(keep_answer(), -1);
    }
}
