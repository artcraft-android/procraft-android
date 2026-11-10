//! Pictures on the system's clipboard, copied in other apps, for Paste: what the system holds goes
//! before what was copied here once it has changed since then. Each look is on a thread of its
//! own, the owner of what is copied being free to take seconds to hand it over.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use procraft_core::ThumbnailImage;

/// One look at a time: the clipboard's connection is shared.
static LOOKING: Mutex<()> = Mutex::new(());

/// What was last cut or copied here, for the studio's source editors, which paste on their own:
/// the copy, the mark of what the system held as it was made (none until looked at), and which
/// files count as pictures.
type OwnCopy = (ThumbnailImage, Option<u64>, fn(&std::path::Path) -> bool);
static OWN: Mutex<Option<OwnCopy>> = Mutex::new(None);

/// What was last cut or copied here, unless the system's clipboard took something else since.
pub fn own_copy_unless_replaced() -> Option<ThumbnailImage> {
    let (image, mark, picture) = OWN.lock().ok()?.clone()?;
    let replaced = mark.is_some_and(|mark| Held::read(picture).mark() != mark);
    (!replaced).then_some(image)
}

/// What the system's clipboard holds that can be pasted.
#[derive(Debug, PartialEq)]
enum Held {
    /// A picture, as straight RGBA.
    Picture(ThumbnailImage),
    /// Picture files copied in a file manager.
    Files(Vec<PathBuf>),
    Nothing,
}

impl Held {
    /// What is on the clipboard now (`picture` says which files are pictures).
    fn read(picture: fn(&std::path::Path) -> bool) -> Held {
        let _one = LOOKING.lock();
        let Ok(mut clipboard) = arboard::Clipboard::new() else { return Held::Nothing };
        if let Ok(image) = clipboard.get_image() {
            return Held::Picture(ThumbnailImage {
                width: image.width as u32,
                height: image.height as u32,
                rgba: image.bytes.into_owned(),
            });
        }
        let files = clipboard.get().file_list().unwrap_or_default();
        let files: Vec<PathBuf> = files.into_iter().filter(|f| picture(f)).collect();
        if files.is_empty() { Held::Nothing } else { Held::Files(files) }
    }

    /// What tells this from what the clipboard held before.
    fn mark(&self) -> u64 {
        let mut hash = DefaultHasher::new();
        match self {
            Held::Picture(image) => (image.width, image.height, &image.rgba).hash(&mut hash),
            Held::Files(files) => files.hash(&mut hash),
            Held::Nothing => return 0,
        }
        hash.finish()
    }
}

/// What Paste came to.
#[derive(Debug, PartialEq)]
pub enum Pasted {
    /// What was copied here.
    Own { private: bool },
    /// A picture from another app, as straight RGBA.
    Picture { image: ThumbnailImage, private: bool },
    /// Picture files from a file manager.
    Files { paths: Vec<PathBuf>, private: bool },
}

/// What Paste takes, given what the system holds and the mark of what it held when something was
/// last copied here (none: nothing has been): that, unchanged, is older than the app's own copy.
fn pasted(held: Held, before_ours: Option<u64>, private: bool) -> Pasted {
    if before_ours == Some(held.mark()) {
        return Pasted::Own { private };
    }
    match held {
        Held::Picture(image) => Pasted::Picture { image, private },
        Held::Files(paths) => Pasted::Files { paths, private },
        Held::Nothing => Pasted::Own { private },
    }
}

/// The system's clipboard as Paste sees it.
pub struct SystemClipboard {
    /// Which files count as pictures.
    picture: fn(&std::path::Path) -> bool,
    /// The mark of what it held as something was last copied here, and that look while under way.
    before_ours: Option<u64>,
    marking: Option<Receiver<u64>>,
    /// It held something to paste when last looked at for Paste's button, and that look.
    pub offers: bool,
    offering: Option<Receiver<bool>>,
    /// A paste waiting to see what it holds.
    pasting: Option<(Receiver<Held>, bool)>,
}

impl SystemClipboard {
    pub fn new(picture: fn(&std::path::Path) -> bool) -> SystemClipboard {
        SystemClipboard { picture, before_ours: None, marking: None, offers: false, offering: None, pasting: None }
    }

    /// Reads it on a thread, what `then` makes of that coming to the receiver.
    fn look<T: Send + 'static>(&self, then: impl FnOnce(Held) -> T + Send + 'static) -> Receiver<T> {
        let (done, result) = channel();
        let picture = self.picture;
        std::thread::spawn(move || {
            // (the app may be gone by then)
            let _ = done.send(then(Held::read(picture)));
        });
        result
    }

    /// Something (`copy`) was copied here: what the system holds now is older than that.
    pub fn copied_here(&mut self, copy: Option<&ThumbnailImage>) {
        if let Ok(mut own) = OWN.lock() {
            *own = copy.map(|c| (c.clone(), None, self.picture));
        }
        self.marking = Some(self.look(|held| {
            let mark = held.mark();
            if let Ok(mut own) = OWN.lock()
                && let Some(own) = own.as_mut()
            {
                own.1.get_or_insert(mark);
            }
            mark
        }));
    }

    /// Paste's button is coming up: whether it is lit goes by this too.
    pub fn offer(&mut self) {
        self.offering = Some(self.look(|held| held != Held::Nothing));
    }

    /// Paste was asked for (on a private layer, if `private`).
    pub fn paste(&mut self, private: bool) {
        self.pasting = Some((self.look(|held| held), private));
    }

    /// What Paste came to, once it has. Called every frame.
    pub fn take_pasted(&mut self) -> Option<Pasted> {
        if let Some(offering) = &self.offering {
            match offering.try_recv() {
                Ok(offers) => {
                    self.offers = offers;
                    self.offering = None;
                }
                Err(TryRecvError::Disconnected) => self.offering = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some(marking) = &self.marking {
            match marking.try_recv() {
                Ok(mark) => {
                    self.before_ours = Some(mark);
                    self.marking = None;
                }
                Err(TryRecvError::Disconnected) => self.marking = None,
                // what was copied here is yet to be told from what is there
                Err(TryRecvError::Empty) => return None,
            }
        }
        let (pasting, private) = self.pasting.as_ref()?;
        let private = *private;
        match pasting.try_recv() {
            Ok(held) => {
                self.pasting = None;
                Some(pasted(held, self.before_ours, private))
            }
            Err(TryRecvError::Disconnected) => {
                self.pasting = None;
                Some(Pasted::Own { private })
            }
            Err(TryRecvError::Empty) => None,
        }
    }

    /// A look is under way, so frames are to keep coming.
    pub fn busy(&self) -> bool {
        self.offering.is_some() || self.marking.is_some() || self.pasting.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(fill: u8) -> ThumbnailImage {
        ThumbnailImage { width: 2, height: 1, rgba: vec![fill; 8] }
    }

    #[test]
    fn paste_takes_what_another_app_copied_unless_something_was_copied_here_since() {
        // nothing copied here: the system's picture
        let got = pasted(Held::Picture(picture(1)), None, false);
        assert_eq!(got, Pasted::Picture { image: picture(1), private: false });
        // copied here with that on the system's: ours is the newer
        let before = Some(Held::Picture(picture(1)).mark());
        assert_eq!(pasted(Held::Picture(picture(1)), before, true), Pasted::Own { private: true });
        // then copied in another app: that is
        let got = pasted(Held::Picture(picture(2)), before, true);
        assert_eq!(got, Pasted::Picture { image: picture(2), private: true });
        // nothing there, or nothing that can be pasted
        assert_eq!(pasted(Held::Nothing, None, false), Pasted::Own { private: false });
        assert_eq!(pasted(Held::Nothing, before, false), Pasted::Own { private: false });
    }

    #[test]
    fn picture_files_copied_elsewhere_are_pasted_as_files() {
        let files = vec![PathBuf::from("/a/b.png")];
        let got = pasted(Held::Files(files.clone()), Some(Held::Nothing.mark()), false);
        assert_eq!(got, Pasted::Files { paths: files.clone(), private: false });
        let before = Some(Held::Files(files.clone()).mark());
        assert_eq!(pasted(Held::Files(files), before, false), Pasted::Own { private: false });
    }

    #[test]
    fn a_picture_of_another_size_with_the_same_bytes_is_another_picture() {
        let wide = Held::Picture(ThumbnailImage { width: 2, height: 1, rgba: vec![7; 8] });
        let tall = Held::Picture(ThumbnailImage { width: 1, height: 2, rgba: vec![7; 8] });
        assert_ne!(wide.mark(), tall.mark());
        assert_eq!(Held::Nothing.mark(), 0);
    }
}
