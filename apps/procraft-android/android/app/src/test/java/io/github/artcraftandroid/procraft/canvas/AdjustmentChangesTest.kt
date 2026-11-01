package io.github.artcraftandroid.procraft.canvas

import androidx.compose.ui.geometry.Offset
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** What undo inside an adjustment takes back: the values it has changed, not the tool being open. */
class AdjustmentChangesTest {
    private val neutralCurves = List(4) { listOf(Offset(0f, 0f), Offset(1f, 1f)) }

    private fun changed(
        adjustment: Adjustment,
        hue: Float = 0.5f,
        saturation: Float = 0.5f,
        brightness: Float = 0.5f,
        balance: List<Float> = List(9) { 0.5f },
        curves: List<List<Offset>> = neutralCurves,
        amount: Float = 0f,
        liquified: Boolean = false,
    ): Boolean =
        adjustmentChanged(adjustment, hue, saturation, brightness, balance, curves, amount, liquified)

    @Test
    fun hsbIsChangedOnlyWhenOneOfItsThreeLeavesTheMiddle() {
        assertFalse(changed(Adjustment.HSB))
        assertTrue(changed(Adjustment.HSB, hue = 0.6f))
        assertTrue(changed(Adjustment.HSB, saturation = 0.4f))
        assertTrue(changed(Adjustment.HSB, brightness = 0.7f))
    }

    @Test
    fun colorBalanceIsChangedWhenAnyOfItsNineLeavesTheMiddle() {
        assertFalse(changed(Adjustment.COLOR_BALANCE))
        assertTrue(changed(Adjustment.COLOR_BALANCE, balance = List(9) { if (it == 4) 0.6f else 0.5f }))
    }

    @Test
    fun curvesAreChangedWhenAPointMovesOrOneIsAdded() {
        assertFalse(changed(Adjustment.CURVES))
        assertTrue(changed(Adjustment.CURVES, curves = List(4) { listOf(Offset(0f, 0f), Offset(1f, 0.5f)) }))
        assertTrue(
            changed(
                Adjustment.CURVES,
                curves = neutralCurves.mapIndexed { i, points -> if (i == 0) points + Offset(0.5f, 0.8f) else points },
            ),
        )
    }

    @Test
    fun liquifyIsChangedByAStrokeAndNotByItsAmount() {
        assertFalse(changed(Adjustment.LIQUIFY))
        assertTrue(changed(Adjustment.LIQUIFY, liquified = true))
        // the amount alone changes nothing until the field has a stroke in it
        assertFalse(changed(Adjustment.LIQUIFY, amount = 1f))
    }

    @Test
    fun gradientMapIsChangedByItsAmount() {
        assertFalse(changed(Adjustment.GRADIENT_MAP))
        assertTrue(changed(Adjustment.GRADIENT_MAP, amount = 0.1f))
    }

    @Test
    fun anAmountToolIsChangedByItsAmount() {
        assertFalse(changed(Adjustment.GAUSSIAN_BLUR))
        assertTrue(changed(Adjustment.GAUSSIAN_BLUR, amount = 0.2f))
    }

    @Test
    fun everySwipedAdjustmentStartsPencilAtAnAmount() {
        // A swiped tool is driven by its amount, so painting it on needs one to start from. Gradient
        // Map had none, so Pencil began at nothing and the brush showed no effect.
        for (a in Adjustment.entries.filter { it.swiped }) {
            assertNotNull("${a.label} has no pencil amount", a.pencilAmount)
        }
    }
}
