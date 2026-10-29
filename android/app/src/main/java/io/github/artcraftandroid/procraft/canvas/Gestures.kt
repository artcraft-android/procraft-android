package io.github.artcraftandroid.procraft.canvas

import android.content.SharedPreferences
import androidx.compose.runtime.mutableStateMapOf

/** What a gesture can do: the pages of Gesture controls. */
enum class GesturePurpose(val title: String, val heading: String) {
    SMUDGE("Smudge", "Customize smudge"),
    ERASE("Erase", "Customize erase"),
    ASSISTED("Assisted Drawing", "Customize assisted drawing"),
    EYEDROPPER("Eyedropper", "Customize eyedropper"),
    QUICKSHAPE("QuickShape", "Customize QuickShape"),
    QUICKMENU("QuickMenu", "Customize QuickMenu"),
    FULL_SCREEN("Full Screen", "Customize full screen"),
    CLEAR_LAYER("Clear Layer", "Customize clear layer"),
    COPY_PASTE("Copy & Paste", "Customize copy & paste"),
    LAYER_SELECT("Layer Select", "Customize layer select"),
    HOVER("Hover", "Customize hover"),
    GENERAL("General", "General"),
}

/** A way of touching: each one does one thing at most (General's settings aside). */
enum class Trigger(val label: String) {
    TAP_MODIFY("Tap □"),
    TOUCH("Touch"),
    PEN("Pen"),
    MODIFY_TOUCH("□ + Touch"),
    MODIFY_PEN("□ + Pen"),
    THREE_SWIPE("Three-finger swipe"),
    FOUR_TAP("Four-finger tap"),
    // the pen's side buttons
    PEN_DOUBLE_TAP("Pen button"),
    PEN_SQUEEZE("Pen second button"),
    HOLD("Touch and hold"),
    SCRUB("Scrub"),
}

/**
 * One switch on a page: a trigger (or, for General and Hover, a setting of its own [key]), its
 * name, what it does, whether it starts on, and whether it has a delay slider under it.
 */
class GestureOption(
    val key: String,
    val label: String,
    val detail: String,
    val default: Boolean = false,
    val trigger: Trigger? = null,
    val delayed: Boolean = false,
)

private fun opt(t: Trigger, detail: String, default: Boolean = false) =
    GestureOption(t.name, t.label, detail, default, t, delayed = t == Trigger.HOLD)

/** Every page's switches, in order. */
val GestureOptions: Map<GesturePurpose, List<GestureOption>> = mapOf(
    GesturePurpose.SMUDGE to listOf(
        opt(Trigger.TOUCH, "Finger strokes smudge"),
        opt(Trigger.PEN, "Pen strokes smudge"),
        opt(Trigger.MODIFY_TOUCH, "Finger strokes smudge while □ is held down"),
        opt(Trigger.MODIFY_PEN, "Pen strokes smudge while □ is held down"),
    ),
    GesturePurpose.ERASE to listOf(
        opt(Trigger.TOUCH, "Finger strokes erase"),
        opt(Trigger.PEN, "Pen strokes erase"),
        opt(Trigger.MODIFY_TOUCH, "Finger strokes erase while □ is held down"),
        opt(Trigger.MODIFY_PEN, "Pen strokes erase while □ is held down"),
    ),
    GesturePurpose.ASSISTED to listOf(
        opt(Trigger.TAP_MODIFY, "Turns drawing assist on or off for this layer"),
        opt(Trigger.TOUCH, "Finger strokes follow the drawing guide"),
        opt(Trigger.PEN, "Pen strokes follow the drawing guide"),
        opt(Trigger.MODIFY_TOUCH, "Finger strokes follow the guide while □ is held down"),
        opt(Trigger.MODIFY_PEN, "Pen strokes follow the guide while □ is held down"),
        opt(Trigger.PEN_DOUBLE_TAP, "Turns drawing assist on or off for this layer"),
    ),
    GesturePurpose.EYEDROPPER to listOf(
        opt(Trigger.TAP_MODIFY, "Opens or closes the eyedropper", default = true),
        opt(Trigger.TOUCH, "A finger on the canvas picks a colour"),
        opt(Trigger.MODIFY_TOUCH, "A finger picks a colour while □ is held down", default = true),
        opt(Trigger.MODIFY_PEN, "The pen picks a colour while □ is held down", default = true),
        opt(Trigger.PEN_DOUBLE_TAP, "Opens the eyedropper"),
        opt(Trigger.PEN_SQUEEZE, "Opens the eyedropper"),
        opt(Trigger.HOLD, "A finger kept still on the canvas picks a colour", default = true),
    ),
    GesturePurpose.QUICKSHAPE to listOf(
        opt(Trigger.TAP_MODIFY, "Turns the last stroke into a shape"),
        opt(Trigger.TOUCH, "A finger touch turns the last stroke into a shape"),
        opt(Trigger.THREE_SWIPE, "Three fingers swiped down turn the last stroke into a shape"),
        opt(Trigger.FOUR_TAP, "Four fingers tapped turn the last stroke into a shape"),
        opt(Trigger.PEN_DOUBLE_TAP, "Turns the last stroke into a shape"),
        opt(Trigger.PEN_SQUEEZE, "Turns the last stroke into a shape"),
        GestureOption("DRAW_HOLD", "Draw and hold", "Pausing at the end of a stroke turns it into a shape", default = true, delayed = true),
    ),
    GesturePurpose.QUICKMENU to listOf(
        opt(Trigger.TAP_MODIFY, "Opens or closes the quick menu"),
        opt(Trigger.TOUCH, "A finger touch opens the quick menu"),
        opt(Trigger.MODIFY_TOUCH, "A finger opens the quick menu while □ is held down"),
        opt(Trigger.MODIFY_PEN, "The pen opens the quick menu while □ is held down"),
        opt(Trigger.THREE_SWIPE, "Three fingers swiped down open the quick menu"),
        opt(Trigger.FOUR_TAP, "Four fingers tapped open or close the quick menu"),
        opt(Trigger.PEN_DOUBLE_TAP, "Opens the quick menu"),
        opt(Trigger.PEN_SQUEEZE, "Opens the quick menu"),
        opt(Trigger.HOLD, "A finger kept still on the canvas opens the quick menu"),
    ),
    GesturePurpose.FULL_SCREEN to listOf(
        opt(Trigger.TOUCH, "A finger touch shows or hides the interface"),
        opt(Trigger.THREE_SWIPE, "Three fingers swiped down show or hide the interface"),
        opt(Trigger.FOUR_TAP, "Four fingers tapped show or hide the interface", default = true),
        opt(Trigger.PEN_DOUBLE_TAP, "Shows or hides the interface"),
        GestureOption("AUTO", "Automatic Full Screen", "The interface steps aside while you paint", delayed = true),
    ),
    GesturePurpose.CLEAR_LAYER to listOf(
        opt(Trigger.SCRUB, "Rubbing three fingers back and forth clears the layer", default = true),
        opt(Trigger.THREE_SWIPE, "Three fingers swiped down clear the layer"),
        opt(Trigger.FOUR_TAP, "Four fingers tapped clear the layer"),
        opt(Trigger.PEN_DOUBLE_TAP, "Clears the layer"),
        opt(Trigger.HOLD, "A finger kept still on the canvas clears the layer"),
    ),
    GesturePurpose.COPY_PASTE to listOf(
        opt(Trigger.TAP_MODIFY, "Opens the copy and paste menu"),
        opt(Trigger.TOUCH, "A finger touch opens the copy and paste menu"),
        opt(Trigger.MODIFY_TOUCH, "A finger opens the copy and paste menu while □ is held down"),
        opt(Trigger.MODIFY_PEN, "The pen opens the copy and paste menu while □ is held down"),
        opt(Trigger.THREE_SWIPE, "Three fingers swiped down open the copy and paste menu", default = true),
        opt(Trigger.FOUR_TAP, "Four fingers tapped open the copy and paste menu"),
        opt(Trigger.PEN_DOUBLE_TAP, "Opens the copy and paste menu"),
        opt(Trigger.HOLD, "A finger kept still on the canvas opens the copy and paste menu"),
    ),
    GesturePurpose.LAYER_SELECT to listOf(
        opt(Trigger.TOUCH, "A finger touch picks the layer drawn there"),
        opt(Trigger.MODIFY_TOUCH, "A finger picks the layer drawn there while □ is held down"),
        opt(Trigger.MODIFY_PEN, "The pen picks the layer drawn there while □ is held down"),
        opt(Trigger.PEN_SQUEEZE, "Picks the layer drawn under the pen"),
        opt(Trigger.HOLD, "A finger kept still picks the layer drawn there"),
    ),
    GesturePurpose.HOVER to listOf(
        GestureOption("HOVER_TOUCH", "Touch", "While the pen hovers, drag a finger for opacity and pinch for size"),
    ),
    GesturePurpose.GENERAL to listOf(
        GestureOption("FINGER_PAINT", "Painting with a finger", "One finger paints as the pen does", default = true),
        GestureOption("NO_UNDO", "Disable undo and redo gestures", "Two- and three-finger taps leave the history alone"),
        GestureOption("ROTATE", "Rotate with pinch zoom", "Two fingers turn the canvas as they zoom it", default = true),
    ),
)

/** The delays at first, seconds: holds 0.40 s, QuickShape 0.65 s, automatic full screen 0.66 s. */
private fun defaultDelay(p: GesturePurpose) = when (p) {
    GesturePurpose.QUICKSHAPE -> 0.65f
    GesturePurpose.FULL_SCREEN -> 0.66f
    else -> 0.40f
}

/** The gesture settings, kept in [prefs]: a trigger switched on for one page goes off on the others. */
class GestureSettings(private val prefs: SharedPreferences) {
    private val on = mutableStateMapOf<String, Boolean>()
    private val delays = mutableStateMapOf<GesturePurpose, Float>()

    init {
        for ((p, options) in GestureOptions) {
            for (o in options) on[name(p, o.key)] = prefs.getBoolean(name(p, o.key), o.default)
            delays[p] = prefs.getFloat("${p.name}.delay", defaultDelay(p))
        }
    }

    private fun name(p: GesturePurpose, key: String) = "${p.name}.$key"

    fun isOn(p: GesturePurpose, key: String): Boolean = on[name(p, key)] == true

    fun isOn(p: GesturePurpose, t: Trigger): Boolean = isOn(p, t.name)

    /** Turns a switch on or off; on, its trigger goes off everywhere else. */
    fun set(p: GesturePurpose, key: String, value: Boolean) {
        val edit = prefs.edit()
        val trigger = GestureOptions[p]?.firstOrNull { it.key == key }?.trigger
        if (value && trigger != null) {
            for ((other, options) in GestureOptions) {
                if (other == p) continue
                for (o in options) if (o.trigger == trigger && isOn(other, o.key)) {
                    on[name(other, o.key)] = false
                    edit.putBoolean(name(other, o.key), false)
                }
            }
        }
        on[name(p, key)] = value
        edit.putBoolean(name(p, key), value).apply()
    }

    /** What [t] does, if anything. */
    fun purposeOf(t: Trigger): GesturePurpose? = GestureOptions.entries.firstOrNull { (p, options) -> options.any { it.trigger == t && isOn(p, it.key) } }?.key

    /** A page's delay, seconds. */
    fun delay(p: GesturePurpose): Float = delays[p] ?: defaultDelay(p)

    fun setDelay(p: GesturePurpose, seconds: Float) {
        val s = seconds.coerceIn(MIN_DELAY_S, MAX_DELAY_S)
        delays[p] = s
        prefs.edit().putFloat("${p.name}.delay", s).apply()
    }

    /** Every switch and delay as the app ships them. */
    fun resetAll() {
        val edit = prefs.edit()
        for ((p, options) in GestureOptions) {
            for (o in options) {
                on[name(p, o.key)] = o.default
                edit.remove(name(p, o.key))
            }
            delays[p] = defaultDelay(p)
            edit.remove("${p.name}.delay")
        }
        edit.apply()
    }

    companion object {
        const val MIN_DELAY_S = 0.1f
        const val MAX_DELAY_S = 1.5f
    }
}
