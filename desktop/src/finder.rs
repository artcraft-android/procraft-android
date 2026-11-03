//! Files opened from the Finder on a Mac (a double-click, Open With, a drop on the app's Dock
//! icon): the system hands them to the app's delegate as it runs, not in its arguments, and winit
//! has no event for them. Its delegate is given the method that takes them, and they are kept for
//! the app to take in as files dropped on it. Unsafe as the system's calls are.
#![allow(unsafe_code)]

use std::path::PathBuf;
use std::sync::Mutex;

use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSArray, NSURL};

/// The files opened since the app last took them.
static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// The files opened from the Finder since the last call.
pub fn take() -> Vec<PathBuf> {
    OPENED.lock().map(|mut opened| std::mem::take(&mut *opened)).unwrap_or_default()
}

/// Takes the files opened from the Finder from now on. Called once the event loop is made (winit
/// makes the app's delegate with it) and before it runs: the files the app is started with come
/// as it starts.
pub fn watch() {
    let Some(main) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(main);
    let Some(delegate) = app.delegate() else {
        return log::warn!("files opened from the Finder aren't taken: the app has no delegate");
    };
    let class: &AnyClass = AsRef::<AnyObject>::as_ref(&*delegate).class();
    let method: unsafe extern "C-unwind" fn(&AnyObject, Sel, &NSApplication, &NSArray<NSURL>) = open_urls;
    // SAFETY: `open_urls` takes what `application:openURLs:` is sent with (its encoding: no result;
    // the delegate, the selector, the app and the array of URLs), added to the delegate's own
    // class, which doesn't have it
    let added = unsafe {
        objc2::ffi::class_addMethod(
            (class as *const AnyClass).cast_mut(),
            sel!(application:openURLs:),
            std::mem::transmute::<unsafe extern "C-unwind" fn(&AnyObject, Sel, &NSApplication, &NSArray<NSURL>), Imp>(
                method,
            ),
            c"v@:@@".as_ptr(),
        )
    };
    if !added.as_bool() {
        log::warn!("files opened from the Finder aren't taken: the app's delegate takes them itself");
    }
}

/// `application:openURLs:`: the files' paths kept (a URL that isn't a file's is left out).
unsafe extern "C-unwind" fn open_urls(_delegate: &AnyObject, _cmd: Sel, _app: &NSApplication, urls: &NSArray<NSURL>) {
    let paths: Vec<PathBuf> = urls.iter().filter_map(|url| url.to_file_path()).collect();
    log::info!("{} file(s) opened from the Finder", paths.len());
    if let Ok(mut opened) = OPENED.lock() {
        opened.extend(paths);
    }
}
