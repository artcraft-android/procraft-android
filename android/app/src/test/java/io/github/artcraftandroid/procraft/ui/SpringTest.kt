package io.github.artcraftandroid.procraft.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs

class SpringTest {
    private fun samples(s: Spring) = (0..100).map { s.transform(it / 100f) }

    @Test
    fun startsAtNothingAndEndsAtTheEnd() {
        for (s in listOf(Spring(0.3f, 0.75f, 0f), Spring(0.3f, 0.18f, 6f), Spring(0.5f, 8f, 5f), Spring(0.3f, 1f, 0f))) {
            assertEquals(0f, s.transform(0f), 1e-4f)
            assertEquals(1f, s.transform(1f), 0f)
        }
    }

    @Test
    fun aLightlyDampedSpringOvershootsAndSettles() {
        val bouncy = samples(Spring(0.3f, 0.18f, 6f))
        assertTrue("overshoots", bouncy.any { it > 1.2f })
        // within a thousandth of the end by the time it is given
        assertTrue(abs(bouncy[99] - 1f) < 2e-3f)
    }

    @Test
    fun aDampedSpringBarelyOvershootsAndIsNearlyThereEarly() {
        val rows = samples(Spring(0.3f, 0.75f, 0f))
        assertTrue(rows.max() < 1.05f)
        assertTrue(abs(rows[60] - 1f) < 0.05f)
    }

    @Test
    fun anOverdampedSpringNeverGoesPastTheEnd() {
        val stiff = samples(Spring(0.5f, 8f, 5f))
        assertTrue(stiff.all { it <= 1f + 1e-3f })
        // it moves at once with its starting speed
        assertTrue(stiff[5] > 0.05f)
        assertTrue(stiff.zipWithNext().all { (a, b) -> b >= a - 1e-4f })
    }
}
