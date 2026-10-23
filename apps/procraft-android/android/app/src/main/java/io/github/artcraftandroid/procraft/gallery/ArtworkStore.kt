package io.github.artcraftandroid.procraft.gallery

import io.github.artcraftandroid.procraft.canvas.TimeLapseSettings
import org.json.JSONObject
import java.io.File
import java.util.Locale
import java.util.UUID
import kotlin.math.roundToInt

/** Something the gallery shows: an artwork, or a stack of them. */
sealed interface GalleryEntry {
    val dir: File
    val name: String
    val modified: Long
}

/** A saved artwork: its folder (see the native library's `project.rs` for what is inside). */
data class Artwork(override val dir: File, override val name: String, val width: Int, val height: Int, override val modified: Long) : GalleryEntry {
    val thumbnail: File get() = File(dir, "thumbnail.png")
}

/** Artworks kept together: a folder holding their folders, newest first. */
data class Stack(override val dir: File, override val name: String, val artworks: List<Artwork>) : GalleryEntry {
    override val modified: Long get() = artworks.maxOfOrNull { it.modified } ?: dir.lastModified()
}

/** The artworks in the app's private storage, one folder each, some in stacks (a folder of them). */
class ArtworkStore(val root: File) {
    init {
        root.mkdirs()
    }

    /** What is in [at] (the gallery, or a stack's folder), newest first; empty stacks go away. */
    fun list(at: File = root): List<GalleryEntry> = at.listFiles { f -> f.isDirectory }.orEmpty().mapNotNull { dir ->
        read(dir) ?: if (at == root) readStack(dir) else null
    }.sortedByDescending { it.modified }

    fun newFolder(at: File = root): File = File(at, UUID.randomUUID().toString()).also { it.mkdirs() }

    fun delete(a: Artwork) {
        a.dir.deleteRecursively()
    }

    /** A copy beside the original, named "<name> copy" (the copy's title is set by the caller when opened). */
    fun duplicate(a: Artwork): File {
        val copy = newFolder(a.dir.parentFile ?: root)
        a.dir.copyRecursively(copy, overwrite = true)
        File(copy, "artwork.json").let { f ->
            runCatching {
                val json = JSONObject(f.readText())
                json.put("name", "${a.name} copy")
                f.writeText(json.toString())
            }
        }
        copy.setLastModified(System.currentTimeMillis())
        return copy
    }

    fun rename(a: Artwork, name: String) {
        val f = File(a.dir, "artwork.json")
        runCatching {
            val json = JSONObject(f.readText())
            json.put("name", name.trim())
            f.writeText(json.toString())
        }
    }

    /** A new stack in the gallery holding [artworks] (moved into it). */
    fun stack(artworks: List<Artwork>): File {
        val dir = newFolder(root)
        File(dir, STACK_FILE).writeText(JSONObject().put("name", "Stack").toString())
        for (a in artworks) a.dir.renameTo(File(dir, a.dir.name))
        return dir
    }

    fun renameStack(s: Stack, name: String) {
        runCatching { File(s.dir, STACK_FILE).writeText(JSONObject().put("name", name.trim().ifEmpty { "Stack" }).toString()) }
    }

    private fun read(dir: File): Artwork? {
        val f = File(dir, "artwork.json")
        if (!f.isFile) return null
        return runCatching {
            val json = JSONObject(f.readText())
            val doc = json.getJSONObject("document")
            Artwork(dir, json.optString("name", "Untitled Artwork"), doc.optInt("width"), doc.optInt("height"), f.lastModified())
        }.getOrNull()
    }

    private fun readStack(dir: File): Stack? {
        val f = File(dir, STACK_FILE)
        if (!f.isFile) return null
        val artworks = list(dir).filterIsInstance<Artwork>()
        if (artworks.isEmpty()) {
            dir.deleteRecursively()
            return null
        }
        val name = runCatching { JSONObject(f.readText()).optString("name", "Stack") }.getOrDefault("Stack")
        return Stack(dir, name, artworks)
    }

    private companion object {
        const val STACK_FILE = "stack.json"
    }
}

/**
 * A canvas to start from, in the New canvas list: its name, its size in [unit] ("px", "mm", "cm"
 * or "in"), DPI, background (and whether it is hidden) and time-lapse video.
 */
data class CanvasPreset(
    val name: String,
    val unit: String,
    val sizeWidth: Float,
    val sizeHeight: Float,
    val dpi: Float,
    val background: Int = 0xFFFFFF,
    val backgroundHidden: Boolean = false,
    val timeLapse: TimeLapseSettings = TimeLapseSettings(),
    /** The colour profile, by name. */
    val profile: String = SRGB,
) {
    val width: Int get() = pixels(sizeWidth)
    val height: Int get() = pixels(sizeHeight)

    /** The size as the list shows it, in its unit. */
    val label: String
        get() = if (unit == "in") "${number(sizeWidth)}″ × ${number(sizeHeight)}″" else "${number(sizeWidth)} × ${number(sizeHeight)}$unit"

    private fun pixels(v: Float): Int = (perInch(unit)?.let { v / it * dpi } ?: v).roundToInt().coerceIn(1, MAX_SIDE)

    fun toJson(): JSONObject = JSONObject().put("name", name).put("unit", unit).put("width", sizeWidth.toDouble()).put("height", sizeHeight.toDouble())
        .put("dpi", dpi.toDouble()).put("background", background).put("hidden", backgroundHidden)
        .put("resolution", timeLapse.resolution).put("quality", timeLapse.quality).put("hevc", timeLapse.hevc).put("profile", profile)

    companion object {
        /** The longest side a canvas can have, pixels. */
        const val MAX_SIDE = 16384

        const val SRGB = "sRGB IEC61966-2.1"
        const val P3 = "Display P3"

        /** A profile as a list's chip shows it. */
        fun chip(profile: String): String = when {
            profile == P3 -> "P3"
            io.github.artcraftandroid.procraft.core.isCmykProfile(profile) -> "CMYK"
            else -> "sRGB"
        }

        /** How many of [unit] make an inch; null for pixels. */
        fun perInch(unit: String): Float? = when (unit) {
            "mm" -> 25.4f
            "cm" -> 2.54f
            "in" -> 1f
            else -> null
        }

        /** [v] with at most two decimals and no trailing zeros. */
        fun number(v: Float): String = "%.2f".format(Locale.ROOT, v).trimEnd('0').trimEnd('.')

        fun fromJson(j: JSONObject) = CanvasPreset(
            j.optString("name", "Untitled Canvas"), j.optString("unit", "px"), j.optDouble("width", 1080.0).toFloat(), j.optDouble("height", 1080.0).toFloat(),
            j.optDouble("dpi", 300.0).toFloat(), j.optInt("background", 0xFFFFFF), j.optBoolean("hidden", false),
            TimeLapseSettings(j.optInt("resolution", 0), j.optInt("quality", 1), j.optBoolean("hevc", false)),
            j.optString("profile", SRGB),
        )
    }
}

/** The presets a new gallery starts with, under the screen's size. */
val StandardPresets = listOf(
    CanvasPreset("Square", "px", 2048f, 2048f, 132f),
    CanvasPreset("4K", "px", 4096f, 1714f, 132f),
    CanvasPreset("A4", "mm", 210f, 297f, 300f),
    CanvasPreset("4 × 6 Photo", "in", 6f, 4f, 300f),
    CanvasPreset("Paper", "in", 11f, 8.5f, 300f),
    // printed: CMYK
    CanvasPreset("Comic", "in", 6f, 9.5f, 400f, profile = io.github.artcraftandroid.procraft.core.cmykProfiles().first()),
)
