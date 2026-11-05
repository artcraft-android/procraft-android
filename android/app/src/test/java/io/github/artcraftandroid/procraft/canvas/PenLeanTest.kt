package io.github.artcraftandroid.procraft.canvas

import kotlin.math.PI
import kotlin.math.acos
import kotlin.math.atan
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.sin
import kotlin.math.sqrt
import kotlin.math.tan
import org.junit.Assert.assertEquals
import org.junit.Test

class PenLeanTest {
    private fun radians(degrees: Int) = degrees * PI / 180.0

    /** The tilt axis Android makes of tilts [x] and [y] (radians). */
    private fun tiltAxis(x: Double, y: Double) = acos(cos(x) * cos(y)).toFloat()

    /** The orientation axis Android makes of them, the display turned [quarters] quarter turns. */
    private fun orientationAxis(x: Double, y: Double, quarters: Int) = (atan2(-sin(x), sin(y)) + quarters * PI / 2).toFloat()

    /** How far a pen tilted [x] and [y] leans from upright. */
    private fun trueLean(x: Double, y: Double) = atan(sqrt(tan(x) * tan(x) + tan(y) * tan(y))).toFloat()

    @Test
    fun aLeanComesBackFromAndroidsAxesForEveryTiltAndTurnOfTheDisplay() {
        for (x in -60..60 step 5) for (y in -60..60 step 5) for (quarters in 0..3) {
            val (tx, ty) = radians(x) to radians(y)
            val lean = penLean(tiltAxis(tx, ty), orientationAxis(tx, ty, quarters))
            assertEquals("$x $y turned $quarters", trueLean(tx, ty), lean, 1e-3f)
        }
    }

    @Test
    fun aPenLeaningOnOneAxisReadsAsAndroidSaysAndADiagonalOneLess() {
        val along = radians(50)
        assertEquals(along.toFloat(), penLean(tiltAxis(along, 0.0), orientationAxis(along, 0.0, 0)), 1e-3f)
        // 45 degrees from upright across both axes: Android's axis says 48.2
        val each = atan(sqrt(0.5))
        val axis = tiltAxis(each, each)
        assertEquals(radians(48).toFloat(), axis, 0.01f)
        assertEquals(radians(45).toFloat(), penLean(axis, orientationAxis(each, each, 0)), 1e-3f)
    }

    @Test
    fun aPenThatGivesNoTiltStaysUpright() {
        assertEquals(0f, penLean(0f, 1f), 0f)
        assertEquals(0f, penLean(Float.NaN, 1f), 0f)
    }
}
