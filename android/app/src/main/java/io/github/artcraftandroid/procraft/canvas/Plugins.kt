package io.github.artcraftandroid.procraft.canvas

import android.content.Context
import android.content.SharedPreferences
import io.github.artcraftandroid.procraft.core.Plugins

/** The patches and the plugins kept in the app's folder, each value as kept in [prefs], the interface settings: a key nothing has is passed over. */
fun keptPlugins(context: Context, prefs: SharedPreferences): Plugins =
    Plugins.withStore(java.io.File(context.filesDir, "plugins").path).also { plugins ->
        prefs.all.forEach { (key, value) -> plugins.readKept(key, value.toString()) }
    }
