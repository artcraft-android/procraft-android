package io.github.artcraftandroid.procraft.canvas

import kotlin.math.atan
import kotlin.math.cos
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * How far a pen really leans from upright, radians, from the [tilt] and [orientation] axes of its
 * samples. Android works its tilt axis out as acos(cos x cos y) of the pen's tilts along the panel,
 * and that reads a pen leaning across both of them past where it is: 3 degrees at 45, 5 at 55, the
 * more the nearer the diagonal, where a hand holds a pen. The brushes' tilt goes by the true lean.
 * Android's orientation is atan2(-sin x, sin y), so the two give x and y back, whichever way the
 * display is turned (a quarter turn only swaps them). A pen that gives no tilt stays upright.
 */
internal fun penLean(tilt: Float, orientation: Float): Float {
    if (!(tilt > 0f)) return 0f
    val sinTilt = sin(tilt)
    val sinTwice = sin(2f * orientation)
    // sin^2 x + sin^2 y, from cos^2 tilt = (1 - sin^2 x)(1 - sin^2 y) and how the direction parts them
    val spread = sinTilt * sinTilt
    val both = 2f * spread / (1f + sqrt((1f - sinTwice * sinTwice * spread).coerceAtLeast(0f)))
    val squareX = both * sin(orientation) * sin(orientation)
    val squareY = both * cos(orientation) * cos(orientation)
    if (squareX >= 1f || squareY >= 1f) return tilt
    return atan(sqrt(squareX / (1f - squareX) + squareY / (1f - squareY)))
}
