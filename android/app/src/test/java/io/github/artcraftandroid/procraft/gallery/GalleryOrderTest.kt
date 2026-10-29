package io.github.artcraftandroid.procraft.gallery

import org.junit.Assert.assertEquals
import org.junit.Test

class GalleryOrderTest {
    @Test
    fun withNoOrderYetTheNewestComeFirst() {
        val present = listOf("a" to 10L, "b" to 30L, "c" to 20L)
        assertEquals(listOf("b", "c", "a"), GalleryOrder.arrange(present, emptyList()))
    }

    @Test
    fun aChangedArtworkKeepsItsPlaceAndANewOneGoesFirst() {
        // "a" was changed last, but it was placed already; "d" is new
        val present = listOf("a" to 99L, "b" to 30L, "c" to 20L, "d" to 50L)
        assertEquals(listOf("d", "c", "a", "b"), GalleryOrder.arrange(present, listOf("c", "a", "b")))
    }

    @Test
    fun goneFoldersAndRepeatsLeaveTheOrder() {
        val present = listOf("a" to 1L, "b" to 2L)
        assertEquals(listOf("b", "a"), GalleryOrder.arrange(present, listOf("b", "x", "a", "b")))
    }

    @Test
    fun aMoveTakesTheEntryOutAndPutsItBackAtItsNewPlace() {
        val order = listOf("a", "b", "c", "d")
        assertEquals(listOf("b", "c", "a", "d"), GalleryOrder.moved(order, "a", 2))
        assertEquals(listOf("d", "a", "b", "c"), GalleryOrder.moved(order, "d", 0))
        // past either end it goes to that end
        assertEquals(listOf("b", "c", "d", "a"), GalleryOrder.moved(order, "a", 99))
        assertEquals(listOf("c", "a", "b", "d"), GalleryOrder.moved(order, "c", -1))
        // a name not there yet is put in
        assertEquals(listOf("a", "e", "b", "c", "d"), GalleryOrder.moved(order, "e", 1))
    }
}
