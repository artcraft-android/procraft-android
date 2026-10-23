package io.github.artcraftandroid.procraft.canvas

import android.content.res.AssetManager
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import io.github.artcraftandroid.procraft.core.TextAlignment
import io.github.artcraftandroid.procraft.core.TextInfo
import java.nio.ByteBuffer
import kotlin.math.ceil
import kotlin.math.roundToInt

/**
 * Lays text layers out and draws them, with the platform's typefaces (and the bundled Inter):
 * the layer's pixels come from here whenever its text or look changes.
 */
object TextRenderer {
    /** A typeface the text tool offers: the name shown, and the family it is made from. */
    class Family(val name: String, val family: String)

    private val builtIn = listOf(
        Family("Sans Serif", "sans-serif"),
        Family("Sans Serif Condensed", "sans-serif-condensed"),
        Family("Serif", "serif"),
        Family("Monospace", "monospace"),
        Family("Serif Monospace", "serif-monospace"),
        Family("Casual", "casual"),
        Family("Cursive", "cursive"),
        Family("Small Caps", "sans-serif-smallcaps"),
        Family("Inter", INTER),
    )

    /** Fonts imported into the app, kept in its files. */
    val imported = androidx.compose.runtime.mutableStateListOf<Family>()

    /** Every typeface on offer: the system's, Inter and those imported. */
    val families: List<Family> get() = builtIn + imported

    /** Reads the imported fonts kept in [dir]. */
    fun loadImported(dir: java.io.File) {
        imported.clear()
        dir.listFiles { f -> f.extension.lowercase() in listOf("ttf", "otf") }?.sortedBy { it.name.lowercase() }?.forEach {
            imported.add(Family(it.nameWithoutExtension, FILE + it.path))
        }
    }

    /** The styles a family offers: a name, its weight and whether it slants. */
    class Style(val name: String, val weight: Int, val italic: Boolean)

    val styles = listOf(
        Style("Thin", 100, false), Style("Thin Italic", 100, true),
        Style("Light", 300, false), Style("Light Italic", 300, true),
        Style("Regular", 400, false), Style("Italic", 400, true),
        Style("Medium", 500, false), Style("Medium Italic", 500, true),
        Style("Bold", 700, false), Style("Bold Italic", 700, true),
        Style("Black", 900, false), Style("Black Italic", 900, true),
    )

    /** Where the bundled Inter comes from (set once the app starts). */
    var assets: AssetManager? = null

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

    private fun paint(t: TextInfo): TextPaint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
        typeface = typeface(t.font, t.style)
        textSize = t.size
        val a = (t.opacity.coerceIn(0f, 1f) * 255).roundToInt()
        color = android.graphics.Color.argb(a, (t.r * 255).roundToInt(), (t.g * 255).roundToInt(), (t.b * 255).roundToInt())
        // spacing between letters, in sizes
        letterSpacing = t.tracking + t.kerning
        isUnderlineText = t.underline
        if (t.outline) {
            style = Paint.Style.STROKE
            strokeWidth = (t.size * OUTLINE).coerceAtLeast(1f)
        }
    }

    /** What is drawn: upper case if asked, one letter a line when vertical. */
    private fun shown(t: TextInfo): String {
        val text = if (t.allCaps) t.text.uppercase() else t.text
        return if (t.vertical) text.lines().joinToString("\n\n") { line -> line.toList().joinToString("\n") } else text
    }

    fun layout(t: TextInfo): StaticLayout {
        val p = paint(t)
        val text = shown(t)
        val natural = text.lines().maxOfOrNull { ceil(StaticLayout.getDesiredWidth(it, p)).toInt() } ?: 0
        val width = if (t.width > 0f) t.width.roundToInt() else natural.coerceAtLeast(1)
        val align = when (t.align) {
            TextAlignment.CENTER -> Layout.Alignment.ALIGN_CENTER
            TextAlignment.RIGHT -> Layout.Alignment.ALIGN_OPPOSITE
            else -> Layout.Alignment.ALIGN_NORMAL
        }
        return StaticLayout.Builder.obtain(text, 0, text.length, p, width.coerceAtLeast(1))
            .setAlignment(align)
            .setLineSpacing(t.leading * t.size, 1f)
            .setIncludePad(false)
            .setJustificationMode(if (t.align == TextAlignment.JUSTIFY) Layout.JUSTIFICATION_MODE_INTER_WORD else Layout.JUSTIFICATION_MODE_NONE)
            .build()
    }

    /** The text's box on the canvas, pixels. */
    fun bounds(t: TextInfo): RectF {
        val l = layout(t)
        val top = t.y - t.baseline * t.size
        return RectF(t.x, top, t.x + l.width, top + l.height)
    }

    /** The text drawn over transparency on a picture the canvas's size: premultiplied RGBA8. */
    fun render(t: TextInfo, width: Int, height: Int): ByteArray {
        val bitmap = Bitmap.createBitmap(width.coerceAtLeast(1), height.coerceAtLeast(1), Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        canvas.translate(t.x, t.y - t.baseline * t.size)
        layout(t).draw(canvas)
        val buffer = ByteBuffer.allocate(bitmap.byteCount)
        bitmap.copyPixelsToBuffer(buffer)
        bitmap.recycle()
        return buffer.array()
    }

    const val INTER = "inter"
    /** An imported font's family: this, then the file's path. */
    const val FILE = "file:"
    /** An outline's width, as a fraction of the size. */
    private const val OUTLINE = 0.04f
}
