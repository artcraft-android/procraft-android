package io.github.artcraftandroid.procraft.canvas

import org.junit.Assert.assertEquals
import org.junit.Test

class SetMoveTest {
    /** The list [before] with the set at [from] moved to [to], as the core moves it. */
    private fun moved(before: List<Int>, from: Int, to: Int) = before.toMutableList().apply { add(to, removeAt(from)) }

    @Test
    fun everySetIsFoundWhereTheMoveLeftIt() {
        val sets = (0 until 70).toList()
        for (from in sets) for (to in sets) {
            val after = moved(sets, from, to)
            for (set in sets) assertEquals("set $set, $from to $to", set, after[placeAfterSetMove(set, from, to)])
        }
    }

    @Test
    fun aSetMovedOntoItsOwnPlaceMovesNothing() {
        for (set in 0 until 5) assertEquals(set, placeAfterSetMove(set, 2, 2))
    }
}
