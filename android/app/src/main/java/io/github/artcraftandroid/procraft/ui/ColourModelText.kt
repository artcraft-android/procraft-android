package io.github.artcraftandroid.procraft.ui

import io.github.artcraftandroid.procraft.canvas.ColorMode
import kotlin.math.roundToInt

// What the colour panel shows of its colour model, apart from the core (so the JVM tests reach it).

/** Whether the colour panel shows the colour model's button: on the pages that work in one (not Harmony's P3 wheel, nor the palettes). */
internal fun showsModelButton(mode: ColorMode) = mode != ColorMode.HARMONY && mode != ColorMode.PALETTES

/** A hue as Value shows it, in whole degrees. */
internal fun degreesShown(hue: Float) = "${(hue * 360f).roundToInt()}°"

/** A share as Value shows it, in whole percent. */
internal fun percentShown(v: Float) = "${(v * 100f).roundToInt()}%"

/** OKLab's a or b as Value shows it: signed, to two places, with a point whatever the language. */
internal fun signedShown(v: Float): String = String.format(java.util.Locale.ROOT, "%+.2f", v)
