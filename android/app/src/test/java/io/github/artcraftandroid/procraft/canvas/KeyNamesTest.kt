package io.github.artcraftandroid.procraft.canvas

import android.view.KeyEvent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class KeyNamesTest {
    @Test
    fun keysGoByTheNamesTheSharedShortcutsKnowAndBack() {
        val named = mapOf(
            KeyEvent.KEYCODE_Z to "z",
            KeyEvent.KEYCODE_0 to "0",
            KeyEvent.KEYCODE_LEFT_BRACKET to "[",
            KeyEvent.KEYCODE_SPACE to "space",
            KeyEvent.KEYCODE_DEL to "backspace",
            KeyEvent.KEYCODE_FORWARD_DEL to "delete",
            KeyEvent.KEYCODE_ESCAPE to "escape",
            KeyEvent.KEYCODE_PLUS to "plus",
            KeyEvent.KEYCODE_F1 to "f1",
            KeyEvent.KEYCODE_F12 to "f12",
            KeyEvent.KEYCODE_DPAD_UP to "up",
        )
        for ((code, name) in named) {
            assertEquals(name, keyName(code))
            assertEquals(code, keyCodeNamed(name))
        }
        assertNull(keyName(KeyEvent.KEYCODE_TAB))
        assertNull(keyCodeNamed("tab"))
    }
}
