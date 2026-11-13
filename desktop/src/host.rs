//! What the app takes from the system around it: the window, its event loop's waker, egui's input
//! from the window's events, the file dialogs and the clipboard. On Windows, macOS and Linux winit,
//! egui-winit, rfd and arboard; on HarmonyOS the canvas component (`ohos.rs`), the system's
//! document picker and share sheet through ArkTS (`ohos_platform.rs`), and for the clipboard's
//! pictures and files (which reading the system's pasteboard would need a permission for) nothing
//! yet: egui's text goes to and from the pasteboard where the app may read it.

#[cfg(not(target_env = "ohos"))]
pub use egui_winit::{EventResponse, State as EguiState};
#[cfg(not(target_env = "ohos"))]
pub use winit::event_loop::{EventLoopProxy, OwnedDisplayHandle};
#[cfg(not(target_env = "ohos"))]
pub use winit::window::Window;
#[cfg(not(target_env = "ohos"))]
pub use {arboard, rfd};

#[cfg(target_env = "ohos")]
pub use crate::ohos::{EguiState, EventLoopProxy, EventResponse, OwnedDisplayHandle, Window};

/// A line of the log, also handed to the system's own log where the app has one to hand it to
/// (HarmonyOS: hilog, which `hdc hilog` reads; the terminal and the log file have it too).
pub fn also_log(line: &[u8]) {
    #[cfg(target_env = "ohos")]
    crate::ohos::log_line(line);
    #[cfg(not(target_env = "ohos"))]
    let _ = line;
}

/// What the system's dialogs would have said, over the interface: on HarmonyOS, until it has
/// them, the app's messages show as a card (tapped away); elsewhere the dialogs show them.
pub fn notices(ctx: &egui::Context) {
    #[cfg(target_env = "ohos")]
    notice::show(ctx);
    #[cfg(not(target_env = "ohos"))]
    let _ = ctx;
}

/// The app's messages waiting to be shown, oldest first (HarmonyOS).
#[cfg(target_env = "ohos")]
mod notice {
    use std::sync::Mutex;

    static WAITING: Mutex<Vec<String>> = Mutex::new(Vec::new());

    /// `text` is shown at the next frame, after those before it.
    pub fn say(text: String) {
        log::info!("notice: {text}");
        WAITING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(text);
    }

    /// The oldest message waiting, over everything; a tap anywhere puts it away.
    pub fn show(ctx: &egui::Context) {
        let mut waiting = WAITING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(text) = waiting.first().cloned() else { return };
        let screen = ctx.content_rect();
        let mut gone = false;
        egui::Area::new(egui::Id::unique("system notice")).fixed_pos(screen.min).order(egui::Order::Tooltip).show(
            ctx,
            |ui| {
                gone = ui.allocate_rect(screen, egui::Sense::click()).clicked();
                ui.painter().rect_filled(screen, 0, crate::look::argb(0x66000000));
                let ink = crate::look::argb(crate::look::pc::TEXT);
                let galley = ui.painter().layout(
                    text,
                    crate::look::Weight::Regular.font(17.0),
                    ink,
                    (screen.width() - 96.0).clamp(120.0, 480.0),
                );
                let card = egui::Rect::from_center_size(screen.center(), galley.size() + egui::vec2(48.0, 40.0));
                ui.painter().rect_filled(card, 18.0, crate::look::argb(crate::look::pc::BAR));
                ui.painter().galley(card.center() - galley.size() / 2.0, galley, ink);
            },
        );
        if gone {
            waiting.remove(0);
            ctx.request_repaint();
        }
    }
}

/// rfd's dialogs as the app uses them, on HarmonyOS's own (`ohos_platform.rs`): picking opens the
/// document picker (the files copied into the app's cache), saving writes into the cache and then
/// copies where the user picked in the picker's Save, or shares from the share sheet for a "Share
/// as …"; a folder's files are shared together. A message shows as the app's own card.
#[cfg(target_env = "ohos")]
pub mod rfd {
    use std::path::PathBuf;

    /// What the app says where there is no picker to open (ArkTS not reached).
    const NO_PICKER: &str = "The system's file picker couldn't be opened.";

    #[derive(Default)]
    pub struct FileDialog {
        title: String,
        extensions: Vec<String>,
        name: Option<String>,
    }

    impl FileDialog {
        pub fn new() -> FileDialog {
            FileDialog::default()
        }

        pub fn set_title(self, title: impl Into<String>) -> FileDialog {
            FileDialog { title: title.into(), ..self }
        }

        pub fn add_filter(mut self, _name: impl Into<String>, extensions: &[impl ToString]) -> FileDialog {
            self.extensions.extend(extensions.iter().map(ToString::to_string));
            self
        }

        pub fn set_file_name(self, name: impl Into<String>) -> FileDialog {
            FileDialog { name: Some(name.into()), ..self }
        }

        pub fn pick_file(self) -> Option<PathBuf> {
            self.open(false)?.into_iter().next()
        }

        pub fn pick_files(self) -> Option<Vec<PathBuf>> {
            self.open(true)
        }

        /// A folder for files the app writes, shared together once written.
        pub fn pick_folder(self) -> Option<PathBuf> {
            crate::ohos_platform::folder()
        }

        pub fn save_file(self) -> Option<PathBuf> {
            let name = self.name.unwrap_or_else(|| "ProCraft".to_owned());
            let path = crate::ohos_platform::save_file(&self.title, &name);
            if path.is_none() && !crate::ohos_platform::reachable() {
                super::notice::say(NO_PICKER.to_owned());
            }
            path
        }

        fn open(&self, multiple: bool) -> Option<Vec<PathBuf>> {
            let picked = crate::ohos_platform::open_files(&self.title, &self.extensions, multiple);
            if picked.is_none() && !crate::ohos_platform::reachable() {
                super::notice::say(NO_PICKER.to_owned());
            }
            picked
        }
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum MessageLevel {
        #[default]
        Info,
        Error,
    }

    #[derive(Default)]
    pub struct MessageDialog {
        title: String,
        description: String,
    }

    impl MessageDialog {
        pub fn new() -> MessageDialog {
            MessageDialog::default()
        }

        pub fn set_level(self, _level: MessageLevel) -> MessageDialog {
            self
        }

        pub fn set_title(self, title: impl Into<String>) -> MessageDialog {
            MessageDialog { title: title.into(), ..self }
        }

        pub fn set_description(self, description: impl Into<String>) -> MessageDialog {
            MessageDialog { description: description.into(), ..self }
        }

        /// Shown as the app's own card (the app's name as a title says nothing more).
        pub fn show(self) {
            let text = if self.title.is_empty() || self.title == "ProCraft" {
                self.description
            } else {
                format!(
                    "{}

{}",
                    self.title, self.description
                )
            };
            super::notice::say(text);
        }
    }
}

/// arboard's clipboard as the app uses it, for pictures and files from other apps: none on
/// HarmonyOS yet (reading the pasteboard takes a permission most apps don't get, or the system's
/// paste button), so a paste of a picture from the system finds nothing.
#[cfg(target_env = "ohos")]
pub mod arboard {
    use std::borrow::Cow;
    use std::path::PathBuf;

    #[derive(Debug)]
    pub enum Error {
        ClipboardNotSupported,
    }

    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("the system's clipboard isn't available yet")
        }
    }

    pub struct ImageData<'a> {
        pub width: usize,
        pub height: usize,
        pub bytes: Cow<'a, [u8]>,
    }

    pub struct Clipboard;

    pub struct Get;

    impl Clipboard {
        pub fn new() -> Result<Clipboard, Error> {
            Err(Error::ClipboardNotSupported)
        }

        pub fn get_text(&mut self) -> Result<String, Error> {
            Err(Error::ClipboardNotSupported)
        }

        pub fn get_image(&mut self) -> Result<ImageData<'static>, Error> {
            Err(Error::ClipboardNotSupported)
        }

        pub fn get(&mut self) -> Get {
            Get
        }
    }

    impl Get {
        pub fn file_list(self) -> Result<Vec<PathBuf>, Error> {
            Err(Error::ClipboardNotSupported)
        }
    }
}
