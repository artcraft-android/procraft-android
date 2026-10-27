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

/** Artworks kept together: a folder holding their folders, in their own order. */
data class Stack(override val dir: File, override val name: String, val artworks: List<Artwork>) : GalleryEntry {
    override val modified: Long get() = artworks.maxOfOrNull { it.modified } ?: dir.lastModified()
}

/**
 * Where things sit in the gallery (or a stack): an order of folder names kept beside them. New
 * folders go first, newest first, and keep their place from then on: changing an artwork doesn't
 * move it; only a drag does.
 */
internal object GalleryOrder {
    /** The order of the folders [present] (name to time changed), given the order [saved] before. */
    fun arrange(present: List<Pair<String, Long>>, saved: List<String>): List<String> {
        val names = present.mapTo(HashSet()) { it.first }
        val placed = saved.filter { it in names }.distinct()
        val known = placed.toHashSet()
        return present.filter { it.first !in known }.sortedByDescending { it.second }.map { it.first } + placed
    }

    /** [order] with [name] taken out and put back at [to] (its place in the order without it). */
    fun moved(order: List<String>, name: String, to: Int): List<String> {
        val rest = order.filter { it != name }
        return rest.take(to.coerceIn(0, rest.size)) + name + rest.drop(to.coerceIn(0, rest.size))
    }
}

/** The artworks in the app's private storage, one folder each, some in stacks (a folder of them). */
class ArtworkStore(val root: File) {
    init {
        root.mkdirs()
    }

    /** What is in [at] (the gallery, or a stack's folder) in its order; empty stacks go away. */
    fun list(at: File = root): List<GalleryEntry> {
        val dirs = at.listFiles { f -> f.isDirectory }.orEmpty()
        val entries = dirs.mapNotNull { dir -> read(dir) ?: if (at == root) readStack(dir) else null }.associateBy { it.dir.name }
        return order(at, dirs, entries).mapNotNull(entries::get)
    }

    /** Every folder in [at] in order, those that can't be read this moment (being written) too. */
    private fun order(at: File): List<String> {
        val dirs = at.listFiles { f -> f.isDirectory }.orEmpty()
        return order(at, dirs, dirs.mapNotNull(::read).associateBy { it.dir.name })
    }

    /** The artwork changed last, in the gallery or a stack (each is saved when it is left). */
    fun mostRecent(): Artwork? = list().flatMap {
        when (it) {
            is Artwork -> listOf(it)
            is Stack -> it.artworks
        }
    }.maxByOrNull { it.modified }

    fun newFolder(at: File = root): File = File(at, UUID.randomUUID().toString()).also { it.mkdirs() }

    fun delete(a: Artwork) {
        a.dir.deleteRecursively()
    }

    /** A copy under the same name, right after the original. */
    fun duplicate(a: Artwork): File {
        val at = a.dir.parentFile ?: root
        val before = order(at)
        val copy = newFolder(at)
        a.dir.copyRecursively(copy, overwrite = true)
        // not worked on yet: as recent as the original
        File(copy, "artwork.json").setLastModified(a.modified)
        writeOrder(at, GalleryOrder.moved(before + copy.name, copy.name, before.indexOf(a.dir.name) + 1))
        return copy
    }

    /** [e] moved to [to] in the gallery or stack it is in (its place in the order without it). */
    fun move(e: GalleryEntry, to: Int) {
        val at = e.dir.parentFile ?: return
        writeOrder(at, GalleryOrder.moved(order(at), e.dir.name, to))
    }

    /** Artwork [a] (from the gallery) dropped onto [onto]: an artwork makes a stack with it there, [a] first; a stack takes it in first. */
    fun putOnto(a: Artwork, onto: GalleryEntry) {
        if (onto is Artwork) {
            stack(listOf(a, onto), place = onto)
            return
        }
        val inside = order(onto.dir)
        if (!a.dir.renameTo(File(onto.dir, a.dir.name))) return
        writeOrder(onto.dir, listOf(a.dir.name) + inside)
    }

    fun rename(a: Artwork, name: String) {
        val f = File(a.dir, "artwork.json")
        runCatching {
            val json = JSONObject(f.readText())
            json.put("name", name.trim())
            f.writeText(json.toString())
        }
    }

    /**
     * A new stack in the gallery holding [artworks] (moved into it), where [place] was: by default
     * the first of them, and they keep the gallery's order; with [place], in the order given.
     */
    fun stack(artworks: List<Artwork>, place: Artwork? = null): File {
        val before = order(root)
        val names = artworks.map { it.dir.name }
        val inside = if (place != null) names else before.filter { it in names }
        val at = (place?.dir?.name ?: inside.firstOrNull())?.let(before::indexOf) ?: 0
        val dir = newFolder(root)
        File(dir, STACK_FILE).writeText(JSONObject().put("name", "Stack").toString())
        for (a in artworks) a.dir.renameTo(File(dir, a.dir.name))
        writeOrder(dir, inside)
        val rest = before.filter { it !in names }
        writeOrder(root, GalleryOrder.moved(rest + dir.name, dir.name, before.take(at.coerceAtLeast(0)).count { it !in names }))
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

    /** The order of the folders [dirs] in [at] ([entries] those read), kept up to date on disk. */
    private fun order(at: File, dirs: Array<out File>, entries: Map<String, GalleryEntry>): List<String> {
        val f = File(at, ORDER_FILE)
        val saved = runCatching { f.readLines().filter { it.isNotBlank() } }.getOrDefault(emptyList())
        // a folder being written keeps its place; if new, its folder's time places it
        val order = GalleryOrder.arrange(dirs.map { it.name to (entries[it.name]?.modified ?: it.lastModified()) }, saved)
        if (order != saved) writeOrder(at, order)
        return order
    }

    private fun writeOrder(at: File, order: List<String>) {
        runCatching {
            val f = File(at, ORDER_FILE)
            val next = File(at, "$ORDER_FILE.new")
            next.writeText(order.joinToString("\n", postfix = "\n"))
            if (!next.renameTo(f)) {
                f.delete()
                next.renameTo(f)
            }
        }
    }

    private companion object {
        const val STACK_FILE = "stack.json"

        /** The folders' order, one name a line, first first. */
        const val ORDER_FILE = "order.txt"
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
