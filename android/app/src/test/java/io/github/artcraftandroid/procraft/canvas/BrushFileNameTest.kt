package io.github.artcraftandroid.procraft.canvas

import org.junit.Assert.assertEquals
import org.junit.Test

class BrushFileNameTest {
    @Test
    fun aNameInAnyScriptIsKept() {
        assertEquals("水彩 笔.brush", brushFileName("水彩 笔", single = true))
        assertEquals("Кисть-2.brush", brushFileName("Кисть-2", single = true))
        assertEquals("Café Inks.brushset", brushFileName("Café Inks", single = false))
    }

    @Test
    fun whatAFileNameCantHoldIsMadeAnUnderscore() {
        assertEquals("a_b_c_.brush", brushFileName("a/b:c?", single = true))
        assertEquals("Brush.brush", brushFileName("   ", single = true))
        assertEquals("Brush Set.brushset", brushFileName("", single = false))
    }
}
