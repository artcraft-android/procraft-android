package io.github.artcraftandroid.procraft.gallery

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.io.DataOutputStream

class ColourSwatchesTest {
    private fun bytes(write: DataOutputStream.() -> Unit): ByteArray =
        ByteArrayOutputStream().also { DataOutputStream(it).write() }.toByteArray()

    private fun DataOutputStream.name(s: String) {
        writeShort(s.length + 1)
        writeChars(s)
        writeShort(0)
    }

    @Test
    fun colourSwatchFilesReadEveryColourSpace() {
        val aco = bytes {
            writeShort(1)
            writeShort(5)
            // red in RGB; green as hue 120°; full cyan ink (0 is full); mid grey (0 is white); white in Lab
            writeShort(0); writeShort(65535); writeShort(0); writeShort(0); writeShort(0)
            writeShort(1); writeShort(65535 / 3); writeShort(65535); writeShort(65535); writeShort(0)
            writeShort(2); writeShort(0); writeShort(65535); writeShort(65535); writeShort(65535)
            writeShort(8); writeShort(5000); writeShort(0); writeShort(0); writeShort(0)
            writeShort(7); writeShort(10000); writeShort(0); writeShort(0); writeShort(0)
        }
        val p = ColourSwatches.read(aco, "Shop Colours.aco")!!
        assertEquals("Shop Colours", p.name)
        assertEquals(PALETTE_SLOTS, p.colors.size)
        assertEquals(listOf(0xFF0000, 0x00FF00, 0x00FFFF, 0x808080, 0xFFFFFF), p.colors.take(5))
        assertNull(p.colors[5])
    }

    @Test
    fun swatchExchangeFilesTakeTheirGroupsName() {
        val ase = bytes {
            writeBytes("ASEF")
            writeShort(1); writeShort(0)
            writeInt(4)
            val group = bytes { name("Sunset") }
            writeShort(0xC001); writeInt(group.size); write(group)
            val orange = bytes { name("Orange"); writeBytes("RGB "); writeFloat(1f); writeFloat(0.5f); writeFloat(0f); writeShort(2) }
            writeShort(0x0001); writeInt(orange.size); write(orange)
            val ink = bytes { name("Ink"); writeBytes("CMYK"); writeFloat(0f); writeFloat(0f); writeFloat(0f); writeFloat(1f); writeShort(2) }
            writeShort(0x0001); writeInt(ink.size); write(ink)
            writeShort(0xC002); writeInt(0)
        }
        assertTrue(ColourSwatches.isExchange(ase))
        val p = ColourSwatches.read(ase, "whatever.ase")!!
        assertEquals("Sunset", p.name)
        assertEquals(listOf(0xFF8000, 0x000000, null), p.colors.take(3))
    }

    @Test
    fun labGoesBothWays() {
        for (rgb in listOf(0x3366CC, 0xFFCC00, 0x123456)) {
            val (l, a, b) = ColourSwatches.labOf(rgb).toList()
            assertEquals(rgb, ColourSwatches.labToRgb(l, a, b))
        }
    }

    @Test
    fun otherFilesAreNotPalettes() {
        assertNull(ColourSwatches.read(byteArrayOf(1, 2, 3), "notes.txt"))
        assertNull(ColourSwatches.read(byteArrayOf(0, 1), "broken.aco"))
    }
}
