package io.github.artcraftandroid.procraft.canvas

import io.github.artcraftandroid.procraft.ui.GradientPreset
import io.github.artcraftandroid.procraft.ui.gradientColorAt
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MainColoursTest {
    @Test
    fun aPictureOfTwoColoursGivesThoseTwoLightestFirst() {
        val pixels = List(100) { 0x102030 } + List(50) { 0xF0E0D0 }
        assertEquals(listOf(0xF0E0D0, 0x102030), mainColours(pixels, 30))
    }

    @Test
    fun aGradientGivesAsManyColoursAsAsked() {
        val pixels = (0 until 256).map { (it shl 16) or (it shl 8) or it }
        val colours = mainColours(pixels, 30)
        assertEquals(30, colours.size)
        // lightest first
        assertTrue(colours.zipWithNext().all { (a, b) -> (a and 0xFF) >= (b and 0xFF) })
    }

    @Test
    fun nothingGivesNothing() {
        assertEquals(emptyList<Int>(), mainColours(emptyList(), 30))
    }

    @Test
    fun aGradientsColourIsMixedBetweenItsStops() {
        val g = GradientPreset("Test", listOf(1f to 0xFFFFFFFF, 0f to 0xFF000000))
        assertEquals(0xFF000000, gradientColorAt(g, 0f))
        assertEquals(0xFFFFFFFF, gradientColorAt(g, 1f))
        assertEquals(0xFF7F7F7F, gradientColorAt(g, 0.5f))
    }
}
