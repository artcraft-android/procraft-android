package io.github.artcraftandroid.procraft.canvas

import io.github.artcraftandroid.procraft.ui.GradientPreset
import io.github.artcraftandroid.procraft.ui.gradientColorAt
import org.junit.Assert.assertEquals
import org.junit.Test

class MainColoursTest {
    @Test
    fun aGradientsColourIsMixedBetweenItsStops() {
        val g = GradientPreset("Test", listOf(1f to 0xFFFFFFFF, 0f to 0xFF000000))
        assertEquals(0xFF000000, gradientColorAt(g, 0f))
        assertEquals(0xFFFFFFFF, gradientColorAt(g, 1f))
        assertEquals(0xFF7F7F7F, gradientColorAt(g, 0.5f))
    }
}
