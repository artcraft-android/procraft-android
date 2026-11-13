//! Keyboard shortcuts: pick one of the app's shortcuts and press the new keys for it. It's a plugin,
//! not part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app tells it its shortcuts as it starts (what each does and the keys that do it by default)
//! and hands it every press on the canvas before its own shortcuts see it. Each press bound to a
//! shortcut anew is taken as that shortcut's own key, and a key a shortcut no longer has does
//! nothing; anything else is left as it is. Its panel lists the shortcuts with their keys now, a
//! group at a time; one tapped asks for the next key press, and has a way back to its keys as they
//! came, or none. What it binds it keeps, so it holds through a restart; turned off, every key is
//! as it came again.

use std::sync::{Mutex, MutexGuard};

use kit::app::Panel;
use kit::keys::{Press, Shortcut, Taken};

kit::key_shortcut!(|press| state().bindings.taken(press), |press| state().bindings.keys_for(press));
kit::key_shortcuts!(|list| state().bindings.shortcuts = list);
kit::kept!(|data| state().bindings.read(data));
kit::actions!(|_| {
    // the list from its first page, what it binds as it is
    let mut state = state();
    *state = State { bindings: std::mem::take(&mut state.bindings), ..State::new() };
    state.show();
});
kit::panel!(|row, value| state().touched(row, value));
kit::panel_key!(|press| state().pressed(press));

/// The most shortcuts a page of the panel lists: with its group, Reset all and a note, a panel's 16
/// rows.
const PAGE: usize = 13;
/// The most words a label, and a choice's option, may have.
const LABEL: usize = 40;
const OPTION: usize = 24;

/// The app's shortcuts, and those bound anew: each by its first key as it came (written as the app
/// writes a press), with its keys now.
#[derive(Default)]
struct Bindings {
    shortcuts: Vec<Shortcut>,
    bound: Vec<(String, Vec<Press>)>,
}

impl Bindings {
    const fn new() -> Bindings {
        Bindings { shortcuts: Vec::new(), bound: Vec::new() }
    }
}

/// A shortcut's name here: its first key as it came. One with no key, or kept as it is, has none.
fn name(shortcut: &Shortcut) -> Option<String> {
    (!shortcut.fixed).then(|| shortcut.presses.first().map(Press::text)).flatten()
}

impl Bindings {
    /// The shortcut a press is a key of as it came, if one can be bound anew.
    fn came_with(&self, press: &Press) -> Option<usize> {
        self.shortcuts.iter().position(|s| !s.fixed && s.presses.contains(press))
    }

    /// The keys bound anew to the shortcut named `name`, if it has been.
    fn bound_to(&self, name: &str) -> Option<&Vec<Press>> {
        self.bound.iter().find(|(n, _)| n == name).map(|(_, presses)| presses)
    }

    /// The keys shortcut `at` has now.
    fn now(&self, at: usize) -> Vec<Press> {
        let Some(shortcut) = self.shortcuts.get(at) else { return Vec::new() };
        match name(shortcut).as_deref().and_then(|n| self.bound_to(n)) {
            Some(presses) => presses.clone(),
            None => shortcut.presses.clone(),
        }
    }

    /// What `press` is taken as: a key bound anew to a shortcut is that shortcut's first key as it
    /// came, and a key a shortcut had and doesn't now is none (with Shift too, where the app takes a
    /// key with Shift as it is without it). Anything else is as it is.
    fn taken(&self, press: &Press) -> Taken {
        for (named, presses) in &self.bound {
            let known = self.shortcuts.iter().any(|s| name(s).as_deref() == Some(named.as_str()));
            if known && presses.contains(press) {
                return Press::read(named).map_or(Taken::AsItIs, Taken::As);
            }
        }
        let gone = |press: &Press| {
            self.came_with(press).is_some_and(|at| {
                let shortcut = self.shortcuts.get(at);
                shortcut.and_then(name).is_some_and(|n| self.bound_to(&n).is_some())
            })
        };
        if self.came_with(press).is_some() {
            return if gone(press) { Taken::Nothing } else { Taken::AsItIs };
        }
        if press.shift && gone(&press.without_shift()) { Taken::Nothing } else { Taken::AsItIs }
    }

    /// The keys that now do what `press` does as it came: none if it is as it came.
    fn keys_for(&self, press: &Press) -> Option<Vec<Press>> {
        let at = self.came_with(press)?;
        let named = self.shortcuts.get(at).and_then(name)?;
        self.bound_to(&named).cloned()
    }

    /// `press` bound to shortcut `at`, in place of the keys it had: what it was taken from, if it
    /// was another shortcut's; or why it can't be (a key the app keeps as it is).
    fn bind(&mut self, at: usize, press: Press) -> Result<Option<String>, String> {
        if let Some(kept) = self.shortcuts.iter().find(|s| s.fixed && s.presses.contains(&press)) {
            return Err(format!("That key stays as it is: {}.", kept.does));
        }
        let Some(named) = self.shortcuts.get(at).and_then(name) else { return Ok(None) };
        let mut from = None;
        // another shortcut with it gives it up, keeping its other keys
        for other in 0..self.shortcuts.len() {
            let Some(other_name) = self.shortcuts.get(other).and_then(name) else { continue };
            if other == at || !self.now(other).contains(&press) {
                continue;
            }
            let left: Vec<Press> = self.now(other).into_iter().filter(|p| *p != press).collect();
            self.set(&other_name, left);
            from = self.shortcuts.get(other).map(|s| s.does.clone());
        }
        self.set(&named, vec![press]);
        Ok(from)
    }

    /// Shortcut `at` with no keys.
    fn unbind(&mut self, at: usize) {
        if let Some(named) = self.shortcuts.get(at).and_then(name) {
            self.set(&named, Vec::new());
        }
    }

    /// Shortcut `at` with its keys as they came, taken back from any other bound to them.
    fn reset(&mut self, at: usize) {
        let Some(shortcut) = self.shortcuts.get(at).cloned() else { return };
        let Some(named) = name(&shortcut) else { return };
        self.bound.retain(|(n, _)| *n != named);
        for (_, presses) in &mut self.bound {
            presses.retain(|p| !shortcut.presses.contains(p));
        }
        self.settle();
    }

    /// The shortcut named `named` with `presses` for keys.
    fn set(&mut self, named: &str, presses: Vec<Press>) {
        match self.bound.iter_mut().find(|(n, _)| n == named) {
            Some((_, now)) => *now = presses,
            None => self.bound.push((named.to_owned(), presses)),
        }
        self.settle();
    }

    /// A shortcut back to its keys as they came is no longer bound anew.
    fn settle(&mut self) {
        let shortcuts = &self.shortcuts;
        self.bound.retain(|(named, presses)| {
            let came = shortcuts.iter().find(|s| name(s).as_deref() == Some(named.as_str()));
            came.is_none_or(|s| s.presses != *presses)
        });
    }

    /// What it binds, to keep: a line a shortcut, its name, a tab, then its keys a space apart.
    fn written(&self) -> Vec<u8> {
        let lines: Vec<String> = self
            .bound
            .iter()
            .map(|(named, presses)| {
                let keys: Vec<String> = presses.iter().map(Press::text).collect();
                format!("{named}\t{}", keys.join(" "))
            })
            .collect();
        lines.join("\n").into_bytes()
    }

    /// What it kept, as [`Bindings::written`] writes it, each line that reads as one.
    fn read(&mut self, data: &[u8]) {
        let text = String::from_utf8_lossy(data);
        self.bound = text
            .lines()
            .filter_map(|line| {
                let (named, keys) = line.split_once('\t')?;
                Press::read(named)?;
                Some((named.to_owned(), Press::read_all(keys)))
            })
            .collect();
    }
}

/// What a row of the panel shown does when tapped.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Page,
    Shortcut(usize),
    ResetAll,
    AsItCame,
    NoKeys,
    Back,
    Nothing,
}

/// The panel: which page it shows, the shortcut waiting for its new keys, a note to show, and what
/// its rows do.
struct State {
    bindings: Bindings,
    page: usize,
    waiting: Option<usize>,
    note: Option<String>,
    rows: Vec<Row>,
}

static STATE: Mutex<State> = Mutex::new(State::new());

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// `text` cut to `most` characters, an ellipsis ending what was cut.
fn short(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(most - 1).collect();
    cut.push('…');
    cut
}

impl State {
    const fn new() -> State {
        State { bindings: Bindings::new(), page: 0, waiting: None, note: None, rows: Vec::new() }
    }

    /// The pages, by name: each group's shortcuts that can be bound anew, [`PAGE`] at most a page
    /// (a group of more goes on over pages of its own), eight pages at most.
    fn pages(&self) -> Vec<(String, Vec<usize>)> {
        let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
        for (at, shortcut) in self.bindings.shortcuts.iter().enumerate().filter(|(_, s)| name(s).is_some()) {
            match groups.iter_mut().find(|(group, _)| *group == shortcut.group) {
                Some((_, rows)) => rows.push(at),
                None => groups.push((&shortcut.group, vec![at])),
            }
        }
        let mut pages = Vec::new();
        for (group, rows) in groups {
            for (i, part) in rows.chunks(PAGE).enumerate() {
                let title = if i == 0 { group.to_owned() } else { format!("{group} {}", i + 1) };
                pages.push((short(&title, OPTION), part.to_vec()));
            }
        }
        pages.truncate(8);
        pages
    }

    /// Shows the panel as it stands: a shortcut's own, while one waits for its keys, else the list.
    fn show(&mut self) {
        let shown = match self.waiting {
            Some(at) => self.waiting_panel(at),
            None => self.list_panel(),
        };
        shown.show();
    }

    fn list_panel(&mut self) -> Panel {
        let pages = self.pages();
        self.page = self.page.min(pages.len().saturating_sub(1));
        let mut panel = Panel::new("Keyboard shortcuts");
        self.rows.clear();
        if pages.is_empty() {
            self.rows.push(Row::Nothing);
            return panel.text("There are no shortcuts to change.");
        }
        let names: Vec<&str> = pages.iter().map(|(name, _)| name.as_str()).collect();
        panel = panel.choice("Shortcuts", &names, self.page);
        self.rows.push(Row::Page);
        for &at in pages.get(self.page).map(|(_, rows)| rows.as_slice()).unwrap_or_default() {
            let does = self.bindings.shortcuts.get(at).map(|s| short(&s.does, LABEL)).unwrap_or_default();
            panel = panel.keys(&does, &self.bindings.now(at));
            self.rows.push(Row::Shortcut(at));
        }
        if !self.bindings.bound.is_empty() {
            panel = panel.button("Reset all");
            self.rows.push(Row::ResetAll);
        }
        if let Some(note) = &self.note {
            panel = panel.text(note);
            self.rows.push(Row::Nothing);
        }
        panel
    }

    fn waiting_panel(&mut self, at: usize) -> Panel {
        let Some(shortcut) = self.bindings.shortcuts.get(at) else { return self.list_panel() };
        let mut panel = Panel::new(&short(&shortcut.does, LABEL))
            .text("Press the new keys. Escape goes back.")
            .keys("Now", &self.bindings.now(at))
            .keys("As it came", &shortcut.presses)
            .button("No keys")
            .button("Back")
            .key_press();
        self.rows = vec![Row::Nothing, Row::Nothing, Row::AsItCame, Row::NoKeys, Row::Back];
        if let Some(note) = &self.note {
            panel = panel.text(note);
            self.rows.push(Row::Nothing);
        }
        panel
    }

    /// What it binds kept, for the next start and the app's other copies of it.
    fn changed(&mut self) {
        kit::keys::keep(self.bindings.written());
    }

    fn touched(&mut self, row: usize, value: f32) {
        let doing = self.rows.get(row).copied().unwrap_or(Row::Nothing);
        match (doing, self.waiting) {
            (Row::Page, _) => {
                self.page = value.max(0.0) as usize;
                self.note = None;
            }
            (Row::Shortcut(at), _) => {
                self.waiting = Some(at);
                self.note = None;
            }
            (Row::ResetAll, _) => {
                self.bindings.bound.clear();
                self.changed();
                self.note = Some("Every shortcut is as it came.".to_owned());
            }
            (Row::AsItCame, Some(at)) => {
                self.bindings.reset(at);
                self.changed();
                self.waiting = None;
                self.note = None;
            }
            (Row::NoKeys, Some(at)) => {
                self.bindings.unbind(at);
                self.changed();
                self.waiting = None;
                self.note = None;
            }
            (Row::Back, _) => {
                self.waiting = None;
                self.note = None;
            }
            _ => {}
        }
        self.show();
    }

    fn pressed(&mut self, press: Press) {
        let Some(at) = self.waiting else { return };
        if press == Press::new("escape") {
            self.waiting = None;
            self.note = None;
            self.show();
            return;
        }
        match self.bindings.bind(at, press) {
            Ok(from) => {
                self.changed();
                self.waiting = None;
                self.note = from.map(|does| short(&format!("Taken from {does}."), 400));
            }
            Err(why) => self.note = Some(why),
        }
        self.show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(text: &str) -> Press {
        Press::read(text).unwrap()
    }

    /// The app's shortcuts, a few of them, as it tells them.
    fn bindings() -> Bindings {
        let told = "Edit\tUndo\tcommand+z\t0\n\
                    Edit\tDuplicate the layer\tj command+j\t0\n\
                    Tools\tHue, Saturation, Brightness\tu command+u\t0\n\
                    Tools\tPaint\tb\t0\n\
                    View\tTurn the canvas\tr shift+r\t1\n";
        Bindings { shortcuts: kit::keys::read_shortcuts(told), bound: Vec::new() }
    }

    #[test]
    fn as_it_came_every_press_is_as_it_is() {
        let b = bindings();
        for p in ["command+z", "j", "u", "q", "command+shift+z"] {
            assert_eq!(b.taken(&press(p)), Taken::AsItIs, "{p}");
            assert_eq!(b.keys_for(&press(p)), None, "{p}");
        }
    }

    #[test]
    fn a_key_bound_anew_does_its_shortcut_and_its_old_key_nothing() {
        let mut b = bindings();
        assert_eq!(b.bind(0, press("command+y")), Ok(None));
        assert_eq!(b.taken(&press("command+y")), Taken::As(press("command+z")));
        assert_eq!(b.taken(&press("command+z")), Taken::Nothing);
        assert_eq!(b.keys_for(&press("command+z")), Some(vec![press("command+y")]));
        // a shortcut of two keys bound anew loses both
        assert_eq!(b.bind(1, press("shift+d")), Ok(None));
        assert_eq!(b.taken(&press("j")), Taken::Nothing);
        assert_eq!(b.taken(&press("command+j")), Taken::Nothing);
        assert_eq!(b.taken(&press("shift+d")), Taken::As(press("j")));
        // the app takes B with Shift as B: gone with it
        b.unbind(3);
        assert_eq!(b.taken(&press("b")), Taken::Nothing);
        assert_eq!(b.taken(&press("shift+b")), Taken::Nothing);
        assert_eq!(b.keys_for(&press("b")), Some(Vec::new()));
    }

    #[test]
    fn a_key_taken_from_another_shortcut_leaves_it_its_others() {
        let mut b = bindings();
        assert_eq!(b.bind(0, press("u")), Ok(Some("Hue, Saturation, Brightness".to_owned())));
        assert_eq!(b.taken(&press("u")), Taken::As(press("command+z")));
        // Hue, Saturation, Brightness keeps Ctrl+U, which goes as its first key did
        assert_eq!(b.now(2), [press("command+u")]);
        assert_eq!(b.taken(&press("command+u")), Taken::As(press("u")));
        assert_eq!(b.keys_for(&press("u")), Some(vec![press("command+u")]));
        // back as it came, it takes its key back
        b.reset(2);
        assert_eq!(b.now(2), [press("u"), press("command+u")]);
        assert_eq!(b.now(0), Vec::<Press>::new());
        assert_eq!(b.taken(&press("u")), Taken::AsItIs);
        assert_eq!(b.taken(&press("command+z")), Taken::Nothing);
        b.reset(0);
        assert!(b.bound.is_empty());
    }

    #[test]
    fn a_key_the_app_keeps_can_t_be_bound() {
        let mut b = bindings();
        assert_eq!(b.bind(0, press("r")), Err("That key stays as it is: Turn the canvas.".to_owned()));
        assert!(b.bound.is_empty());
    }

    #[test]
    fn what_it_binds_is_kept_and_read_back() {
        let mut b = bindings();
        b.bind(0, press("command+y")).unwrap();
        b.unbind(3);
        let kept = b.written();
        let mut again = bindings();
        again.read(&kept);
        assert_eq!(again.bound, b.bound);
        assert_eq!(again.taken(&press("command+y")), Taken::As(press("command+z")));
        // what doesn't read is passed over
        again.read(b"nonsense\ncommand+x\tcommand+q\n");
        assert_eq!(again.bound, [("command+x".to_owned(), vec![press("command+q")])]);
        // a shortcut the app no longer has binds nothing
        assert_eq!(again.taken(&press("command+q")), Taken::AsItIs);
    }

    #[test]
    fn the_list_goes_a_group_a_page_and_a_tap_waits_for_keys() {
        let mut state = State { bindings: bindings(), ..State::new() };
        let pages = state.pages();
        let names: Vec<&str> = pages.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Edit", "Tools"], "a group of kept keys has none to change");
        let json = state.list_panel().json();
        assert!(json.contains(r#""options":["Edit","Tools"],"chosen":0"#), "{json}");
        assert!(json.contains(r#"{"type":"keys","label":"Undo","keys":["command+z"]}"#), "{json}");
        assert_eq!(state.rows, [Row::Page, Row::Shortcut(0), Row::Shortcut(1)]);
        state.touched(1, 1.0);
        assert_eq!(state.waiting, Some(0));
        assert!(state.waiting_panel(0).json().ends_with(r#""key_press":true}"#));
        state.pressed(press("command+y"));
        assert_eq!(state.waiting, None);
        assert_eq!(state.bindings.now(0), [press("command+y")]);
        // with something bound, Reset all
        let json = state.list_panel().json();
        assert!(json.contains("Reset all"), "{json}");
        state.touched(state.rows.iter().position(|r| *r == Row::ResetAll).unwrap(), 1.0);
        assert!(state.bindings.bound.is_empty());
        // Escape goes back without binding
        state.touched(1, 1.0);
        state.pressed(Press::new("escape"));
        assert_eq!(state.waiting, None);
        assert!(state.bindings.bound.is_empty());
    }
}
