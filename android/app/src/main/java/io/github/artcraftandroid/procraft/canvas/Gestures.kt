package io.github.artcraftandroid.procraft.canvas

import android.content.SharedPreferences
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.setValue
import io.github.artcraftandroid.procraft.core.GestureOption
import io.github.artcraftandroid.procraft.core.GesturePurpose
import io.github.artcraftandroid.procraft.core.KeptGestures
import io.github.artcraftandroid.procraft.core.TouchSettings
import io.github.artcraftandroid.procraft.core.Trigger
import io.github.artcraftandroid.procraft.core.gestureOptions
import io.github.artcraftandroid.procraft.core.gesturePurposeHeading
import io.github.artcraftandroid.procraft.core.gesturePurposeTitle
import io.github.artcraftandroid.procraft.core.gesturePurposes
import io.github.artcraftandroid.procraft.core.gestureTriggerLabel
import io.github.artcraftandroid.procraft.core.GestureSettings as CoreGestures

// Gesture controls: the pages, their switches, what starts on, the delays and the rule that a way
// of touching does one thing are the core's (gestures.rs); this keeps them in the app's settings.

/** A page's title (its row in the list) and heading. */
val GesturePurpose.title: String get() = gesturePurposeTitle(this)
val GesturePurpose.heading: String get() = gesturePurposeHeading(this)

/** A way of touching's name on its switch. */
val Trigger.label: String get() = gestureTriggerLabel(this)

/** Every page's switches, in order. */
val GestureOptions: Map<GesturePurpose, List<GestureOption>> by lazy { gesturePurposes().associateWith { gestureOptions(it) } }

/** The gesture settings, kept in [prefs] by each switch's name and each page's delay. */
class GestureSettings(private val prefs: SharedPreferences) {
    private val core = CoreGestures(
        KeptGestures(
            switches = GestureOptions.flatMap { (p, options) -> options.map { "${p.name}.${it.key}" } }
                .filter { prefs.contains(it) }.associateWith { prefs.getBoolean(it, false) },
            delays = GesturePurpose.entries.map { "${it.name}.delay" }.filter { prefs.contains(it) }.associateWith { prefs.getFloat(it, 0f) },
        ),
    )

    // read wherever the settings are, so what shows them follows a change
    private var changes by mutableIntStateOf(0)

    fun isOn(p: GesturePurpose, key: String): Boolean = changes.let { core.isOn(p, key) }

    fun isOn(p: GesturePurpose, t: Trigger): Boolean = isOn(p, t.name)

    /** Turns a switch on or off; on, its trigger goes off everywhere else. */
    fun set(p: GesturePurpose, key: String, value: Boolean) {
        val edit = prefs.edit()
        for ((name, on) in core.set(p, key, value)) edit.putBoolean(name, on)
        edit.apply()
        changes++
    }

    /** What [t] does, if anything. */
    fun purposeOf(t: Trigger): GesturePurpose? = changes.let { core.purposeOf(t) }

    /** A page's delay, seconds. */
    fun delay(p: GesturePurpose): Float = changes.let { core.delay(p) }

    fun setDelay(p: GesturePurpose, seconds: Float) {
        prefs.edit().putFloat("${p.name}.delay", core.setDelay(p, seconds)).apply()
        changes++
    }

    /** Every switch and delay as the app ships them. */
    fun resetAll() {
        core.resetAll()
        val all = core.kept()
        val edit = prefs.edit()
        for (name in all.switches.keys + all.delays.keys) edit.remove(name)
        edit.apply()
        changes++
    }

    /** The settings as the canvas's touches (in the shared core) take them. */
    fun touchSettings(rapidUndoMs: Double): TouchSettings = changes.let { core.touchSettings(rapidUndoMs) }

    companion object {
        // as the core keeps them (gestures.rs)
        const val MIN_DELAY_S = 0.1f
        const val MAX_DELAY_S = 1.5f
    }
}
