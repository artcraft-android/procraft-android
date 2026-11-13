package io.github.artcraftandroid.procraft.canvas

import android.view.KeyEvent

/** The keys the shared shortcuts name by a word or a sign (crates/core keys.rs), by their key codes; letters and digits go by themselves. */
private val NAMED = listOf(
    KeyEvent.KEYCODE_LEFT_BRACKET to "[",
    KeyEvent.KEYCODE_RIGHT_BRACKET to "]",
    KeyEvent.KEYCODE_SEMICOLON to ";",
    KeyEvent.KEYCODE_COMMA to ",",
    KeyEvent.KEYCODE_PERIOD to ".",
    KeyEvent.KEYCODE_SLASH to "/",
    KeyEvent.KEYCODE_BACKSLASH to "\\",
    KeyEvent.KEYCODE_APOSTROPHE to "'",
    KeyEvent.KEYCODE_GRAVE to "`",
    KeyEvent.KEYCODE_MINUS to "-",
    KeyEvent.KEYCODE_EQUALS to "=",
    KeyEvent.KEYCODE_PLUS to "plus",
    KeyEvent.KEYCODE_SPACE to "space",
    KeyEvent.KEYCODE_DEL to "backspace",
    KeyEvent.KEYCODE_FORWARD_DEL to "delete",
    KeyEvent.KEYCODE_ESCAPE to "escape",
    KeyEvent.KEYCODE_DPAD_LEFT to "left",
    KeyEvent.KEYCODE_DPAD_RIGHT to "right",
    KeyEvent.KEYCODE_DPAD_UP to "up",
    KeyEvent.KEYCODE_DPAD_DOWN to "down",
) + (0 until 12).map { KeyEvent.KEYCODE_F1 + it to "f${it + 1}" }

/** The key [keyCode] as the shared shortcuts name it ("z", "[", "space", "f5"), as a plugin rebinding keys is told it; null for one they don't name. */
internal fun keyName(keyCode: Int): String? = when (keyCode) {
    in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z -> ('a' + (keyCode - KeyEvent.KEYCODE_A)).toString()
    in KeyEvent.KEYCODE_0..KeyEvent.KEYCODE_9 -> ('0' + (keyCode - KeyEvent.KEYCODE_0)).toString()
    else -> NAMED.firstOrNull { it.first == keyCode }?.second
}

/** The key code of a key as the shared shortcuts name it ([keyName] the other way). */
internal fun keyCodeNamed(name: String): Int? = when {
    name.length == 1 && name[0] in 'a'..'z' -> KeyEvent.KEYCODE_A + (name[0] - 'a')
    name.length == 1 && name[0] in '0'..'9' -> KeyEvent.KEYCODE_0 + (name[0] - '0')
    else -> NAMED.firstOrNull { it.second == name }?.first
}
