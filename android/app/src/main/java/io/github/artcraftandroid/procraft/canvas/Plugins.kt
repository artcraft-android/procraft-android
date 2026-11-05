package io.github.artcraftandroid.procraft.canvas

import android.content.SharedPreferences
import io.github.artcraftandroid.procraft.core.Plugins

/** The plugins, each value as kept in [prefs], the interface settings: a key no plugin has is passed over. */
fun keptPlugins(prefs: SharedPreferences): Plugins =
    Plugins().also { plugins -> prefs.all.forEach { (key, value) -> plugins.readKept(key, value.toString()) } }
