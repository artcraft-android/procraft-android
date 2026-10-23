package io.github.artcraftandroid.procraft.canvas

import android.view.Surface

/** Hands a [Surface]'s native window to the Rust library (`window.rs`), which draws into it. */
object NativeWindows {
    init {
        // JNA loads the library for the UniFFI bindings; the JVM needs it loaded too, for these
        System.loadLibrary("procraft_android")
    }

    /** Registers [surface]'s window and returns its id for `CanvasSession.attachWindow`, or 0. */
    @JvmStatic external fun acquire(surface: Surface): Long

    /** Lets go of a window that was never attached. */
    @JvmStatic external fun release(id: Long)
}
