package io.github.artcraftandroid.procraft.ui

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OverBarsTest {
    private val side = Rect(0f, 300f, 115f, 1468f)

    @Test
    fun theTopBarAndTheSideBarAreOverTheCanvas() {
        // between two of the top bar's buttons, and on the side bar's body
        assertTrue(overBars(Offset(900f, 40f), 99f, side, hidden = false))
        assertTrue(overBars(Offset(60f, 700f), 99f, side, hidden = false))
        // the canvas below the bar and beside the side bar
        assertFalse(overBars(Offset(900f, 120f), 99f, side, hidden = false))
        assertFalse(overBars(Offset(200f, 700f), 99f, side, hidden = false))
    }

    @Test
    fun withTheInterfaceHiddenItIsAllCanvas() {
        assertFalse(overBars(Offset(900f, 40f), 99f, side, hidden = true))
        assertFalse(overBars(Offset(60f, 700f), 99f, null, hidden = true))
    }
}
