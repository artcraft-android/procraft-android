//! What the app takes from the system around it: the window, its event loop's waker, egui's input
//! from the window's events, the file dialogs and the clipboard. On Windows, macOS and Linux winit,
//! egui-winit, rfd and arboard; on HarmonyOS the canvas component (`ohos.rs`) and, for what it
//! doesn't have yet, stand-ins of the app's own: the file dialogs say they aren't there yet, and the
//! clipboard is the app's own (egui's text fields copy and paste among themselves).

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

/// rfd's dialogs as the app uses them, which HarmonyOS doesn't have yet: picking and saving files
/// say so (the document picker comes through an ArkTS bridge later); a message shows as the app's
/// own card.
#[cfg(target_env = "ohos")]
pub mod rfd {
    use std::path::PathBuf;

    /// What the app says where a file dialog would open.
    const NOT_YET: &str = "Opening and saving files isn't available on HarmonyOS yet.";

    #[derive(Default)]
    pub struct FileDialog;

    impl FileDialog {
        pub fn new() -> FileDialog {
            FileDialog
        }

        pub fn set_title(self, _title: impl Into<String>) -> FileDialog {
            self
        }

        pub fn add_filter(self, _name: impl Into<String>, _extensions: &[impl ToString]) -> FileDialog {
            self
        }

        pub fn set_file_name(self, _name: impl Into<String>) -> FileDialog {
            self
        }

        pub fn pick_file(self) -> Option<PathBuf> {
            super::notice::say(NOT_YET.to_owned());
            None
        }

        pub fn pick_files(self) -> Option<Vec<PathBuf>> {
            super::notice::say(NOT_YET.to_owned());
            None
        }

        pub fn pick_folder(self) -> Option<PathBuf> {
            super::notice::say(NOT_YET.to_owned());
            None
        }

        pub fn save_file(self) -> Option<PathBuf> {
            super::notice::say(NOT_YET.to_owned());
            None
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
                format!("{}\n\n{}", self.title, self.description)
            };
            super::notice::say(text);
        }
    }
}

/// arboard's clipboard as the app uses it, which HarmonyOS doesn't have yet (its pasteboard comes
/// through an ArkTS bridge later): there is none, so a paste from the system finds nothing.
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
