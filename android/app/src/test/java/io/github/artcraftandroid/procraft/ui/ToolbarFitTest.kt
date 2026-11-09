package io.github.artcraftandroid.procraft.ui

import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ToolbarFitTest {
    /** The transform toolbar's and the selection toolbar's layouts in a window [width] wide. */
    private fun transform(width: Dp) = toolbarLayout(width, 7, 89.dp, 50.dp, 481.dp)
    private fun selection(width: Dp) = toolbarLayout(width, 8, 83.dp, 44.dp, 524.dp)

    private fun widths(from: Int, to: Int) = (from..to).map { it.dp }

    @Test
    fun withRoomTheToolbarsAreAsMeasuredInLandscape() {
        for (width in listOf(1133.dp, 1280.dp, 2000.dp)) {
            assertEquals(ToolbarLayout(769.dp, 89.dp), transform(width))
            assertEquals(ToolbarLayout(829.dp, 83.dp), selection(width))
        }
    }

    @Test
    fun anUprightWindowHasTheToolbarsMeasuredUpright() {
        // 728 wide on a 744 window: buttons 83 wide, and the selection's 70 wide, 89.4 apart
        val t = transform(744.dp)
        assertEquals(728.dp, t.width)
        assertEquals(83f, t.button.value, 0.5f)
        val s = selection(744.dp)
        assertEquals(728.dp, s.width)
        assertEquals(70f, s.button.value, 0.5f)
        assertEquals(89.4f, (s.button + 19.dp).value, 0.1f)
        for (layout in listOf(t, s)) {
            assertTrue(layout.names)
            assertEquals(1f, layout.iconScale)
            assertFalse(layout.tightPill)
        }
    }

    @Test
    fun theToolbarsFitEveryWindowAndTheirButtonsFillThem() {
        for (width in widths(300, 2000)) {
            for ((layout, count) in listOf(transform(width) to 7, selection(width) to 8)) {
                assertTrue("$width", layout.width <= width - 16.dp)
                val row = layout.button * count + 19.dp * (count - 1) + 32.dp
                assertEquals("$width", layout.width.value, row.value, 0.01f)
            }
            // never wider than with room
            assertTrue(transform(width).width <= 769.dp && selection(width).width <= 829.dp)
        }
        // the widths that clipped (the buttons' row needed 629 and 608)
        assertEquals(612.dp, transform(628.dp).width)
        assertEquals(591.dp, selection(607.dp).width)
    }

    @Test
    fun theButtonsNamesGoInAWindowUnder716() {
        for (width in widths(300, 715)) {
            assertFalse("$width", transform(width).names)
            assertFalse("$width", selection(width).names)
        }
        for (width in widths(716, 2000)) {
            assertTrue("$width", transform(width).names)
            assertTrue("$width", selection(width).names)
        }
    }

    @Test
    fun iconsAreDrawnSmallerOnlyWhereTheyWouldRunIntoEachOther() {
        for (width in widths(300, 2000)) {
            for ((layout, icon) in listOf(transform(width) to 50.dp, selection(width) to 44.dp)) {
                val pitch = layout.button + 19.dp
                assertTrue("$width", icon * layout.iconScale <= pitch + 0.01.dp)
                if (pitch >= icon) assertEquals("$width", 1f, layout.iconScale)
            }
        }
        // half of a portrait tablet: all as big as measured
        assertEquals(1f, transform(400.dp).iconScale)
        assertEquals(1f, selection(400.dp).iconScale)
    }

    @Test
    fun theWaysPillIsTightenedOnlyWhereItDoesNotFit() {
        // inside the padding: 481 for the transform's from a 529 window, 524 for the selection's from 572
        assertFalse(transform(529.dp).tightPill)
        assertTrue(transform(528.dp).tightPill)
        assertFalse(selection(572.dp).tightPill)
        assertTrue(selection(571.dp).tightPill)
    }

    @Test
    fun popoversStillPointAtTheirButtons() {
        for (width in listOf(400.dp, 628.dp, 744.dp, 760.dp, 1133.dp)) {
            val layout = selection(width)
            val left = (width - layout.width) / 2
            assertEquals(left.value + 16f + layout.button.value / 2, toolbarButtonCentre(width, layout, 0, 8).value, 0.01f)
            assertEquals(left.value + layout.width.value - 16f - layout.button.value / 2, toolbarButtonCentre(width, layout, 7, 8).value, 0.01f)
        }
    }
}
