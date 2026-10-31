package io.github.artcraftandroid.procraft.gallery

import io.github.artcraftandroid.procraft.canvas.TimeLapseSettings
import io.github.artcraftandroid.procraft.core.GalleryArtwork
import java.io.File
import io.github.artcraftandroid.procraft.core.ArtworkStore as CoreArtworkStore
import io.github.artcraftandroid.procraft.core.GalleryEntry as CoreGalleryEntry

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
 * The artworks in the app's private storage, one folder each, some in stacks (a folder of them),
 * kept by the shared core (crates/core gallery.rs): their order (new folders first, newest first,
 * keeping their place from then on; only a drag moves them), stacks and names.
 */
class ArtworkStore(val root: File) {
    private val core = CoreArtworkStore.open(root.path)

    /** [at] as the core names a folder: none for the gallery. */
    private fun folder(at: File): String? = at.takeIf { it != root }?.path

    /** What is in [at] (the gallery, or a stack's folder) in its order; empty stacks go away. */
    fun list(at: File = root): List<GalleryEntry> = core.list(folder(at)).map(::entry)

    /** The artwork changed last, in the gallery or a stack (each is saved when it is left). */
    fun mostRecent(): Artwork? = core.mostRecent()?.let(::artwork)

    fun newFolder(at: File = root): File = File(core.newFolder(folder(at)))

    fun delete(a: Artwork) = core.delete(a.dir.path)

    /** A copy under the same name, right after the original. */
    fun duplicate(a: Artwork): File = File(core.duplicate(a.dir.path))

    /**
     * [e] moved to [to] (its place in the order without it) in folder [into]: the gallery or a
     * stack, by default the one it is in; from a stack to the gallery, it is taken out of it.
     */
    fun move(e: GalleryEntry, to: Int, into: File = e.dir.parentFile ?: root) = core.moveEntry(e.dir.path, to.toLong(), into.path)

    /** Artwork [a] (from the gallery) dropped onto [onto]: an artwork makes a stack with it there, [a] first; a stack takes it in first. */
    fun putOnto(a: Artwork, onto: GalleryEntry) = core.putOnto(a.dir.path, onto.dir.path)

    fun rename(a: Artwork, name: String) = core.rename(a.dir.path, name)

    /** A new stack in the gallery holding [artworks] (moved into it), where the first of them was, in the gallery's order. */
    fun stack(artworks: List<Artwork>): File = File(core.stack(artworks.map { it.dir.path }))

    fun renameStack(s: Stack, name: String) = core.renameStack(s.dir.path, name)

    private fun artwork(a: GalleryArtwork) = Artwork(File(a.dir), a.name, a.width.toInt(), a.height.toInt(), a.modifiedMs)

    private fun entry(e: CoreGalleryEntry): GalleryEntry = when (e) {
        is CoreGalleryEntry.Artwork -> artwork(e.artwork)
        is CoreGalleryEntry.Stack -> Stack(File(e.dir), e.name, e.artworks.map(::artwork))
    }
}

/**
 * A canvas to start from, in the New canvas list: its name, its size in [unit] ("px", "mm", "cm"
 * or "in"), DPI, background (and whether it is hidden) and time-lapse video. What its size comes to
 * and how the list shows it are the shared core's (crates/core presets.rs).
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
    val width: Int get() = io.github.artcraftandroid.procraft.core.presetWidth(core()).toInt()
    val height: Int get() = io.github.artcraftandroid.procraft.core.presetHeight(core()).toInt()

    /** The size as the list shows it, in its unit. */
    val label: String get() = io.github.artcraftandroid.procraft.core.presetLabel(core())

    /** As the shared core takes it. */
    fun core() = io.github.artcraftandroid.procraft.core.CanvasPreset(
        name, unit, sizeWidth, sizeHeight, dpi, background.toUInt(), backgroundHidden,
        timeLapse.resolution.toUInt(), timeLapse.quality.toUInt(), timeLapse.hevc, profile,
    )

    companion object {
        const val SRGB = "sRGB IEC61966-2.1"
        const val P3 = "Display P3"

        fun from(p: io.github.artcraftandroid.procraft.core.CanvasPreset) = CanvasPreset(
            p.name, p.unit, p.sizeWidth, p.sizeHeight, p.dpi, p.background.toInt(), p.backgroundHidden,
            TimeLapseSettings(p.timeLapseResolution.toInt(), p.timeLapseQuality.toInt(), p.timeLapseHevc), p.profile,
        )

        /** A profile as a list's chip shows it. */
        fun chip(profile: String): String = io.github.artcraftandroid.procraft.core.profileChip(profile)

        /** How many of [unit] make an inch; null for pixels. */
        fun perInch(unit: String): Float? = io.github.artcraftandroid.procraft.core.perInch(unit)

        /** [v] with at most two decimals and no trailing zeros. */
        fun number(v: Float): String = io.github.artcraftandroid.procraft.core.presetNumber(v)

        /** The presets kept as [saved] (a JSON array), or the standard ones if none were. */
        fun fromJson(saved: String?): List<CanvasPreset> = io.github.artcraftandroid.procraft.core.presetsFromJson(saved).map(::from)

        /** [presets] as kept: a JSON array. */
        fun toJson(presets: List<CanvasPreset>): String = io.github.artcraftandroid.procraft.core.presetsJson(presets.map { it.core() })
    }
}

/** The presets a new gallery starts with, under the screen's size. */
val StandardPresets: List<CanvasPreset> by lazy { io.github.artcraftandroid.procraft.core.standardPresets().map(CanvasPreset::from) }
