package io.github.artcraftandroid.procraft.canvas

import android.content.res.AssetManager
import android.graphics.RectF
import android.graphics.Typeface
import io.github.artcraftandroid.procraft.core.TextInfo
import io.github.artcraftandroid.procraft.core.TextPicture
import java.io.File
import java.util.concurrent.CountDownLatch

/**
 * Text is laid out and drawn by the shared core (as on every platform, so text draws alike
 * everywhere); here are the fonts it is given (the bundled Inter, the system's, the imported ones
 * kept in the app's files), and the typefaces the text tool lists, each shown in its own face.
 */
object TextRenderer {
    /** A typeface the text tool offers: the name shown, and the family it is made from. */
    class Family(val name: String, val family: String)

    /** Every typeface on offer, as the core lists them (the imported ones last, by name). */
    val families = androidx.compose.runtime.mutableStateListOf<Family>()

    /** The styles a family offers: a name, its weight and whether it slants (for its picture). */
    class Style(val name: String, val weight: Int, val italic: Boolean)

    val styles: List<Style> = io.github.artcraftandroid.procraft.core.textStyles().map { name ->
        val weight = when (name.substringBefore(' ')) {
            "Thin" -> 100
            "Light" -> 300
            "Medium" -> 500
            "Bold" -> 700
            "Black" -> 900
            else -> 400
        }
        Style(name, weight, name.endsWith("Italic"))
    }

    /** Where the bundled Inter comes from (set once the app starts), for the lists' pictures. */
    var assets: AssetManager? = null

    private val ready = CountDownLatch(1)
    private val prepared = java.util.concurrent.atomic.AtomicBoolean(false)

    /**
     * Gives the core its fonts off the main thread: Inter's faces, the system's (the generic
     * families pointed at them), and the imported ones kept in [fontDir]; then lists them. Once
     * a process (an activity made again, as on turning, gives them no second time).
     */
    fun prepare(assets: AssetManager, fontDir: File) {
        this.assets = assets
        if (!prepared.compareAndSet(false, true)) return
        Thread {
            for (face in INTER_FACES) {
                runCatching { assets.open(face).use { io.github.artcraftandroid.procraft.core.addTextFont(it.readBytes()) } }
            }
            io.github.artcraftandroid.procraft.core.addSystemTextFonts()
            io.github.artcraftandroid.procraft.core.setTextFontFolder(fontDir.path)
            ready.countDown()
            refreshFamilies()
        }.start()
    }

    /** Waits until the core has its fonts (text laid out before would show nothing). */
    private fun fonts() = ready.await()

    /** The typefaces listed again (after a font is imported), on the main thread. */
    fun refreshFamilies() {
        val listed = io.github.artcraftandroid.procraft.core.textTypefaces().map { Family(it.name, it.family) }
        android.os.Handler(android.os.Looper.getMainLooper()).post {
            families.clear()
            families.addAll(listed)
        }
    }

    /** Keeps the font file the user chose ([bytes], called [name]) and offers it: its family, or null. */
    fun importFont(bytes: ByteArray, name: String): String? {
        fonts()
        val typeface = io.github.artcraftandroid.procraft.core.importTextFont(bytes, name) ?: return null
        refreshFamilies()
        return typeface.family
    }

    /** [font] in [style] as Android draws it, for the text tool's lists (the canvas's is the core's). */
    fun typeface(font: String, style: String): Typeface {
        val s = styles.firstOrNull { it.name == style } ?: styles.first { it.name == "Regular" }
        val base = if (font == INTER) {
            val file = when {
                s.weight >= 700 -> "fonts/Inter-Bold.ttf"
                s.weight >= 600 -> "fonts/Inter-SemiBold.ttf"
                s.weight >= 500 -> "fonts/Inter-Medium.ttf"
                else -> "fonts/Inter-Regular.ttf"
            }
            assets?.let { runCatching { Typeface.createFromAsset(it, file) }.getOrNull() } ?: Typeface.SANS_SERIF
        } else if (font.startsWith(FILE)) {
            runCatching { Typeface.createFromFile(font.removePrefix(FILE)) }.getOrNull() ?: Typeface.SANS_SERIF
        } else {
            Typeface.create(font.ifEmpty { "sans-serif" }, Typeface.NORMAL)
        }
        return Typeface.create(base, s.weight, s.italic)
    }

    /** The text's box on the canvas, pixels. */
    fun bounds(t: TextInfo): RectF {
        fonts()
        val b = io.github.artcraftandroid.procraft.core.textBox(t)
        return RectF(b[0], b[1], b[0] + b[2], b[1] + b[3])
    }

    /** The text [s] is editing drawn into its layer (only where it was and now reaches); here, slow. */
    fun drawInto(s: io.github.artcraftandroid.procraft.core.CanvasSession): Boolean {
        fonts()
        return s.drawTextIntoLayer()
    }

    /**
     * Where the caret before [index] (a place in the text as Android counts it, in UTF-16 units)
     * stands on the canvas: x, its top and bottom; null for vertical text.
     */
    fun caret(t: TextInfo, index: Int): FloatArray? {
        fonts()
        val at = io.github.artcraftandroid.procraft.core.textIndexFromUtf16(t.text, index.coerceAtLeast(0).toUInt())
        return io.github.artcraftandroid.procraft.core.textCaret(t, at).toFloatArray().takeIf { it.size == 3 }
    }

    /** The letters from [start] to [end] (UTF-16 places) ringed in [ring] (ARGB), as a picture of the canvas. */
    fun chosen(t: TextInfo, start: Int, end: Int, ring: Int): TextPicture? {
        fonts()
        val from = io.github.artcraftandroid.procraft.core.textIndexFromUtf16(t.text, start.coerceAtLeast(0).toUInt())
        val to = io.github.artcraftandroid.procraft.core.textIndexFromUtf16(t.text, end.coerceAtLeast(0).toUInt())
        return io.github.artcraftandroid.procraft.core.drawChosenText(t, from, to, ring.toUInt())
    }

    const val INTER = "inter"
    /** An imported font's family: this, then the file's path. */
    const val FILE = "file:"
    private val INTER_FACES = listOf("fonts/Inter-Regular.ttf", "fonts/Inter-Medium.ttf", "fonts/Inter-SemiBold.ttf", "fonts/Inter-Bold.ttf")
}
