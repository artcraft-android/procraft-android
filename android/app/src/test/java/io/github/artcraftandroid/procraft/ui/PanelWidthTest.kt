package io.github.artcraftandroid.procraft.ui

import androidx.compose.ui.unit.dp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PanelWidthTest {
    @Test
    fun theAdjustmentToolbarsGoSideBySideByTheWindowsWidthNotItsShape() {
        // a 744 window (upright, as measured) stacks every one, a 1133 one (landscape) puts every one side by side
        for (from in listOf(800.dp, 820.dp, 904.dp)) {
            assertFalse(sideBySide(744.dp, from))
            assertTrue(sideBySide(1133.dp, from))
        }
        // a window wider than tall but narrow (a split screen of a landscape tablet) stacks them, one
        // taller than wide but wide enough puts them side by side
        assertFalse(sideBySide(700.dp, 800.dp))
        assertTrue(sideBySide(850.dp, 800.dp))
        // each from its own width: hue, saturation and brightness from 816, colour balance from 836
        assertTrue(sideBySide(816.dp, 800.dp))
        assertFalse(sideBySide(815.dp, 800.dp))
        assertFalse(sideBySide(835.dp, 820.dp))
        assertTrue(sideBySide(836.dp, 820.dp))
    }

    @Test
    fun aSideBySideToolbarKeepsInsideTheWindowAndItsSlidersShareWhatItGivesUp() {
        assertEquals(961.dp, toolbarWidth(961.dp, 1133.dp))
        assertEquals(884.dp, toolbarWidth(961.dp, 900.dp))
        for (window in (816..1400).map { it.dp }) {
            val width = toolbarWidth(903.dp, window)
            assertTrue(width <= window - 16.dp)
            // colour balance: three sliders 19 apart and the menu button, 16 in from the left
            val slider = squeezed(251.dp, 3, 903.dp, width)
            assertEquals(width.value, (903.dp - 251.dp * 3 + slider * 3).value, 0.01f)
        }
        assertEquals(251.dp, squeezed(251.dp, 3, 903.dp, 903.dp))
    }

    @Test
    fun liquifysWholeRowFollowsTheWindow() {
        assertEquals(1093.dp, liquifyFullWidth(1133.dp))
        assertEquals(960.dp, liquifyFullWidth(1000.dp))
        assertEquals(1268.dp, liquifyFullWidth(1600.dp))
    }

    @Test
    fun theAnimationBarHoldsWholeFramesOfTheRoomTheWindowLeaves() {
        // a 744 window upright, a screen-shaped canvas (cards 27 wide, 28 apart): 446, as measured
        assertEquals(28.dp, framePitch(0.657f))
        assertEquals(446.dp, animationBarWidth(744.dp, 744.dp, framePitch(0.657f)))
        // in landscape, a square canvas (cards 41): 656 (657 measured)
        assertEquals(42.dp, framePitch(1f))
        assertEquals(656.dp, animationBarWidth(1133.dp, 1133.dp, framePitch(1f)))
        // a smaller screen leaves less room beside it
        assertEquals(42.dp * 19 + 26.dp, animationBarWidth(1024.dp, 1024.dp, framePitch(1f)))
        for (window in (300..2000).map { it.dp }) {
            for (aspect in listOf(0.25f, 0.657f, 1f, 1.5f, 2f)) {
                val pitch = framePitch(aspect)
                val bar = animationBarWidth(window, maxOf(window, 1133.dp), pitch)
                assertTrue("$window $aspect", bar <= window - 16.dp || bar == pitch + 26.dp)
                val frames = (bar - 26.dp) / pitch
                assertEquals(1f, frames % 2f, 0.001f)
            }
        }
    }
}
