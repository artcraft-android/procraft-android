package io.github.artcraftandroid.procraft.canvas

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files

class VideoWriterTest {
    /** A share of the frames kept in one video needs nothing of the device's media system. */
    @Test
    fun oneVideoIsCopiedAsItIs() {
        val dir = Files.createTempDirectory("join").toFile()
        val part = File(dir, "segment-1.mp4").apply { writeBytes(byteArrayOf(1, 2, 3)) }
        val out = File(dir, "out.mp4").apply { writeBytes(byteArrayOf(9)) }
        assertTrue(VideoWriter.join(listOf(part), out, 30f))
        assertArrayEquals(byteArrayOf(1, 2, 3), out.readBytes())
        dir.deleteRecursively()
    }

    @Test
    fun oneVideoThatIsGoneMakesNothing() {
        val dir = Files.createTempDirectory("join").toFile()
        val out = File(dir, "out.mp4")
        assertFalse(VideoWriter.join(listOf(File(dir, "segment-1.mp4")), out, 30f))
        assertFalse(out.exists())
        dir.deleteRecursively()
    }
}
