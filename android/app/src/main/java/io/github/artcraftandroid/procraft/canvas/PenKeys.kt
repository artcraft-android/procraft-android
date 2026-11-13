package io.github.artcraftandroid.procraft.canvas

import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent

/** The key Huawei's M-Pencil gives for a double-tap, twice over for each one. */
internal const val PEN_DOUBLE_TAP_KEY = 718

/** Presses of that key closer than this (ms) are one double-tap given twice. */
private const val DOUBLE_TAP_GIVEN_TWICE_MS = 300L

/** What a pen's button that comes as a key did: pressed, let go, or (a double-tap's key) tapped, never held. */
enum class PenKeyAction { PRESSED, LET_GO, DOUBLE_TAPPED }

/** A pen's two side buttons, as a motion event's button state has them. */
internal const val PEN_BUTTONS = MotionEvent.BUTTON_STYLUS_PRIMARY or MotionEvent.BUTTON_STYLUS_SECONDARY

/** Whether a pointer of [toolType] is a pen's (its tip or its eraser end). */
internal fun isPen(toolType: Int) = toolType == MotionEvent.TOOL_TYPE_STYLUS || toolType == MotionEvent.TOOL_TYPE_ERASER

/** The pen's side buttons as its events say they are held, from one event to the next. */
internal class PenButtons {
    private var held = 0

    /**
     * The buttons pressed and let go ([PEN_BUTTONS] bits) since the last event, by this one's
     * [buttonState]. An event without the pen in it ([penIn] false: a hand's touch, whose buttons read
     * as none held) changes nothing, or a hand landing while a button is held would let it go.
     */
    fun next(buttonState: Int, penIn: Boolean): Pair<Int, Int> {
        if (!penIn) return 0 to 0
        val now = buttonState and PEN_BUTTONS
        val change = (now and held.inv()) to (held and now.inv())
        held = now
        return change
    }
}

/** Tells a pen's next double-tap from the second press of the one just heard. */
internal class PenDoubleTaps {
    private var heardAt: Long? = null

    /** Whether a press at [at] (ms) is a double-tap of its own. */
    fun isNew(at: Long): Boolean {
        val last = heardAt
        if (last != null && at - last < DOUBLE_TAP_GIVEN_TWICE_MS) return false
        heardAt = at
        return true
    }
}

/** A device named as a pen, as a word of its own ("Smart Pen", not "Open Keyboard"). */
private val PEN_NAME = Regex("""\b(pen|stylus)\b""", RegexOption.IGNORE_CASE)

/**
 * Which of a pen's two side buttons [keyCode] is (true: the second), from a device that has
 * [sources], is of [keyboardType] and is called [name]; null if it is no pen's. Some pens, as
 * Redmi's Smart Pen, give their buttons as Page Up and Page Down. A key's own source says keyboard
 * whatever else its device is, so this goes by the device: a stylus, one without letters, or one
 * named as a pen (a pen paired over Bluetooth may list a whole keyboard's keys). Those keys stay a
 * typing keyboard's own.
 */
internal fun penKeyButton(keyCode: Int, sources: Int, keyboardType: Int, name: String): Boolean? {
    val second = when (keyCode) {
        KeyEvent.KEYCODE_PAGE_UP -> false
        KeyEvent.KEYCODE_PAGE_DOWN -> true
        else -> return null
    }
    val stylus = sources and InputDevice.SOURCE_STYLUS == InputDevice.SOURCE_STYLUS
    val pen = stylus || keyboardType != InputDevice.KEYBOARD_TYPE_ALPHABETIC || PEN_NAME.containsMatchIn(name)
    return second.takeIf { pen }
}
