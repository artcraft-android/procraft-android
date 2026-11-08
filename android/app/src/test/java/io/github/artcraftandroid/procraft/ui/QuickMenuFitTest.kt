package io.github.artcraftandroid.procraft.ui

import androidx.compose.ui.unit.dp
import org.junit.Assert.assertEquals
import org.junit.Test

class QuickMenuFitTest {
    @Test
    fun theRingStaysOnAWideScreen() {
        // a press near the corner: the whole ring is brought onto the screen
        assertEquals(276.5.dp to 117.dp, ringCentre(10.dp, 10.dp, 1067.dp, 712.dp))
        assertEquals(500.dp to 300.dp, ringCentre(500.dp, 300.dp, 1067.dp, 712.dp))
    }

    @Test
    fun aWindowTooSmallForTheRingTakesItInItsMiddle() {
        // half of a split screen, and a phone: no range to keep it in, so the middle (this threw)
        assertEquals(265.dp to 300.dp, ringCentre(10.dp, 300.dp, 530.dp, 712.dp))
        assertEquals(205.dp to 100.dp, ringCentre(400.dp, 50.dp, 410.dp, 200.dp))
    }

    @Test
    fun aValueIsKeptWithinItsRangeOrHalfwayBetweenAnEmptyOnesEnds() {
        assertEquals(8.dp, 2.dp.within(8.dp, 100.dp))
        assertEquals(100.dp, 120.dp.within(8.dp, 100.dp))
        assertEquals(54.dp, 8.dp.within(100.dp, 8.dp))
    }
}
