package io.github.artcraftandroid.procraft.ui

import io.github.artcraftandroid.procraft.canvas.ColorMode
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Locale

class ColourModelTextTest {
    @Test
    fun theModelButtonShowsOnThePagesThatWorkInAModel() {
        for (mode in listOf(ColorMode.DISC, ColorMode.CLASSIC, ColorMode.VALUE)) assertTrue(showsModelButton(mode))
        // Harmony's wheel is in Display P3 HSB whatever the model; the palettes have no picker
        assertFalse(showsModelButton(ColorMode.HARMONY))
        assertFalse(showsModelButton(ColorMode.PALETTES))
    }

    @Test
    fun valueShowsItsNumbersAsTheDesktopDoes() {
        assertEquals("180°", degreesShown(0.5f))
        assertEquals("0°", degreesShown(0f))
        assertEquals("26%", percentShown(0.256f))
        assertEquals("100%", percentShown(1f))
        // OKLab's a and b keep their sign and a point in a language that writes a comma
        val was = Locale.getDefault()
        try {
            Locale.setDefault(Locale.GERMANY)
            assertEquals("+0.12", signedShown(0.1234f))
            assertEquals("-0.30", signedShown(-0.3f))
            assertEquals("+0.00", signedShown(0f))
        } finally {
            Locale.setDefault(was)
        }
    }
}
