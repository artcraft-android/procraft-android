package io.github.artcraftandroid.procraft.ui

import org.junit.Assert.assertEquals
import org.junit.Test

class KeyboardReachTest {
    @Test
    fun aKeyboardUpIsMeasuredFromTheWindowsBottom() {
        // a 2000 x 1200 tablet on its side, the keyboard's top at 568: the bar sits on it
        assertEquals(632, keyboardReach(windowBottom = 1200, visibleBottom = 568, windowHeight = 1200))
        // a window starting lower down still measures from its own bottom
        assertEquals(632, keyboardReach(windowBottom = 1250, visibleBottom = 618, windowHeight = 1200))
    }

    @Test
    fun nothingShownBelowButASystemBarIsNoKeyboard() {
        assertEquals(0, keyboardReach(windowBottom = 1200, visibleBottom = 1200, windowHeight = 1200))
        // the navigation bar's 48 dp strip at 2.25 pixels a dp
        assertEquals(0, keyboardReach(windowBottom = 1200, visibleBottom = 1092, windowHeight = 1200))
    }
}
