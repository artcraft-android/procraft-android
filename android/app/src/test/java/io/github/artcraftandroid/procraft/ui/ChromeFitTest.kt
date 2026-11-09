package io.github.artcraftandroid.procraft.ui

import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ChromeFitTest {
    private val sideBarClear = Pc.SideBarWidth + Pc.PopoverGap

    private fun widths(from: Int, to: Int) = (from..to).map { it.dp }

    @Test
    fun aTabletsTopBarKeepsItsMeasuredSteps() {
        for ((width, tool) in listOf(720.dp to 8.dp, 744.dp to 8.dp, 1133.dp to 26.dp, 1200.dp to 26.dp)) {
            assertEquals(Pc.BarFit(40.dp, tool, 14.dp, 44.dp), Pc.barFit(width))
        }
        // wherever the measured step fits, it is the one used
        for (width in widths(705, 2000)) {
            val fit = Pc.barFit(width)
            assertEquals(40.dp, fit.margin)
            assertEquals(44.dp, fit.button)
            assertEquals(14.dp, fit.actionGap)
        }
    }

    @Test
    fun theTopBarsEndsNeverMeet() {
        for (width in widths(200, 2000)) assertTrue("$width", Pc.barFit(width).needs <= width + 0.01.dp)
    }

    @Test
    fun aPhonesTopBarShrinksItsButtonsWithTheirGlyphsThenTakesTwoRows() {
        for (width in widths(200, 2000)) {
            val fit = Pc.barFit(width)
            // the glyphs are drawn as wide as their buttons, so none runs into the next
            assertEquals(fit.button.value / Pc.BarButtonWidth.value, fit.scale, 1e-4f)
            assertTrue("$width", fit.scale <= 1f)
            if (fit.rows == 1) assertTrue("$width", fit.button >= 32.dp - 0.01.dp)
        }
        assertEquals(1, Pc.barFit(360.dp).rows)
        assertEquals(2, Pc.barFit(340.dp).rows)
        // two rows of a phone held upright: the buttons as measured again
        assertEquals(Pc.BarButtonWidth, Pc.barFit(320.dp).button)
    }

    @Test
    fun panelsStillHangFromTheirButtons() {
        // the bar at 411: 8 margins, the buttons and Gallery at 87% in one row, nothing between them
        assertEquals(27.14f, Pc.toolCentreFromRight(0, 411.dp).value, 0.01f)
        assertEquals(180.27f, Pc.toolCentreFromRight(4, 411.dp).value, 0.01f)
        assertEquals(77.60f, Pc.actionCentre(0, 411.dp).value, 0.01f)
        // on a tablet, as ever
        assertEquals(62.dp, Pc.toolCentreFromRight(0, 1200.dp))
        assertEquals(342.dp, Pc.toolCentreFromRight(4, 1200.dp))
        assertEquals(134.dp, Pc.actionCentre(0, 1200.dp))
    }

    @Test
    fun aTabletsSideBarStaysWhereItWasMeasured() {
        // portrait top 333.5, landscape top 130
        assertEquals(333.5f, Pc.sideBarFit(1133.dp).top.value, 0.1f)
        assertEquals(130f, Pc.sideBarFit(744.dp).top.value, 0.1f)
        for (height in widths(580, 2000)) {
            val fit = Pc.sideBarFit(height)
            assertEquals(Pc.SliderHeight, fit.track)
            assertEquals(height * Pc.SIDE_BAR_CENTRE - Pc.SideBarHeight / 2, fit.top)
            assertTrue(fit.top >= Pc.BarHeight)
        }
    }

    @Test
    fun theSideBarFitsUnderTheTopBarOfAShortWindow() {
        for (height in widths(328, 579)) {
            val fit = Pc.sideBarFit(height)
            assertEquals(Pc.BarHeight, fit.top)
            val bar = Pc.SideBarHeight - (Pc.SliderHeight - fit.track) * 2
            assertTrue("$height", fit.top + bar <= height)
            assertTrue(fit.track <= Pc.SliderHeight)
        }
        // no jump where the two meet
        assertEquals(170f, Pc.sideBarFit(579.dp).track.value, 0.6f)
    }

    @Test
    fun aPanelInAWideEnoughWindowGoesWhereItAlwaysDid() {
        // as it was before narrow windows were thought of
        fun old(width: Dp, tip: Dp, screen: Dp, minLeft: Dp, minRight: Dp) =
            (tip + Pc.ArrowShift - width / 2).coerceIn(minLeft, (screen - minRight - width).coerceAtLeast(minLeft))
        for (screen in widths(451, 2000)) {
            val tip = screen - Pc.toolCentreFromRight(0, screen)
            assertEquals(old(376.dp, tip, screen, sideBarClear, Pc.PopoverGap), popoverPlace(376.dp, tip, screen, sideBarClear).left)
            assertEquals(old(376.dp, tip, screen, Pc.PopoverGap, sideBarClear), popoverPlace(376.dp, tip, screen, minRight = sideBarClear).left)
        }
    }

    @Test
    fun aPanelStaysInsideANarrowWindow() {
        for (panel in listOf(330.dp, 376.dp, 384.dp, 390.dp, 440.dp)) {
            for (screen in widths(panel.value.toInt(), 700)) {
                val tip = screen - Pc.toolCentreFromRight(0, screen)
                for (place in listOf(popoverPlace(panel, tip, screen, sideBarClear), popoverPlace(panel, tip, screen, minRight = sideBarClear))) {
                    assertTrue("$panel in $screen", place.left >= 0.dp && place.left + panel <= screen)
                }
            }
        }
        // the colour panel in a window 411 wide, which ran 28 off its right edge
        assertEquals(17.5.dp, popoverPlace(376.dp, 411.dp - 26.dp, 411.dp, sideBarClear).left)
    }
}
