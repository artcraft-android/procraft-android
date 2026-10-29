package io.github.artcraftandroid.procraft.gallery

import java.nio.ByteBuffer
import kotlin.math.cbrt
import kotlin.math.pow
import kotlin.math.roundToInt

/**
 * Palettes from other painting and design programs' swatch files: colour swatch (`.aco`) and
 * swatch exchange (`.ase`) files. Their colours (RGB, HSB, CMYK, Lab or grey) come in as sRGB,
 * the first [PALETTE_SLOTS] of them.
 */
object ColourSwatches {
    /** Whether [head] (a file's first bytes) starts a swatch exchange file. */
    fun isExchange(head: ByteArray) = head.size >= 4 && String(head, 0, 4, Charsets.US_ASCII) == "ASEF"

    /**
     * The palette in [bytes], a swatch exchange file or (when [fileName] ends in `.aco`) a colour
     * swatch file, named by its first group or else the file; null if it is neither.
     */
    fun read(bytes: ByteArray, fileName: String?): Palette? {
        val base = fileName?.substringAfterLast('/')?.substringBeforeLast('.')?.trim().orEmpty()
        return runCatching {
            when {
                isExchange(bytes) -> exchange(bytes, base)
                fileName?.endsWith(".aco", ignoreCase = true) == true -> colourSwatch(bytes, base)
                else -> null
            }
        }.getOrNull()
    }

    /** A colour swatch file: a list of colours, then (in version 2) the same again with names. */
    private fun colourSwatch(bytes: ByteArray, name: String): Palette? {
        val b = ByteBuffer.wrap(bytes)
        val version = b.short.toInt()
        if (version !in 1..2) return null
        val count = b.short.toInt() and 0xFFFF
        val colours = (0 until count).mapNotNull {
            val space = b.short.toInt() and 0xFFFF
            val v = IntArray(4) { b.short.toInt() and 0xFFFF }
            // version 2 names each colour: its length in UTF-16 units, then the units
            if (version == 2) b.position(b.position() + 2 * b.int)
            acoColour(space, v)
        }
        if (colours.isEmpty()) return null
        return Palette(name.ifEmpty { "Imported Palette" }, List(PALETTE_SLOTS) { colours.getOrNull(it) })
    }

    /** A colour swatch file's colour in colour space [space] from its four numbers. */
    private fun acoColour(space: Int, v: IntArray): Int? = when (space) {
        0 -> rgb(v[0] / 65535.0, v[1] / 65535.0, v[2] / 65535.0)
        1 -> hsb(v[0] / 65535.0, v[1] / 65535.0, v[2] / 65535.0)
        // 0 is full ink
        2 -> cmyk(1 - v[0] / 65535.0, 1 - v[1] / 65535.0, 1 - v[2] / 65535.0, 1 - v[3] / 65535.0)
        7 -> lab(v[0] / 100.0, v[1].toShort() / 100.0, v[2].toShort() / 100.0)
        // 0 is white
        8 -> (1 - v[0] / 10000.0).let { rgb(it, it, it) }
        9 -> cmyk(v[0] / 10000.0, v[1] / 10000.0, v[2] / 10000.0, v[3] / 10000.0)
        else -> null
    }

    /** A swatch exchange file: blocks of groups and colours, each colour named and in a model. */
    private fun exchange(bytes: ByteArray, fallback: String): Palette? {
        val b = ByteBuffer.wrap(bytes)
        b.position(8)
        val blocks = b.int
        val colours = mutableListOf<Int>()
        var group: String? = null
        repeat(blocks) {
            val type = b.short.toInt() and 0xFFFF
            val length = b.int
            val end = b.position() + length
            when (type) {
                // a group starts: the palette takes the first one's name
                0xC001 -> if (length >= 2) utf16(b).let { if (group == null && it.isNotBlank()) group = it }
                0x0001 -> {
                    utf16(b)
                    val model = String(ByteArray(4).also { b.get(it) }, Charsets.US_ASCII)
                    val c = when (model) {
                        "RGB " -> rgb(b.float.toDouble(), b.float.toDouble(), b.float.toDouble())
                        "CMYK" -> cmyk(b.float.toDouble(), b.float.toDouble(), b.float.toDouble(), b.float.toDouble())
                        "LAB " -> lab(b.float * 100.0, b.float.toDouble(), b.float.toDouble())
                        "Gray" -> b.float.toDouble().let { rgb(it, it, it) }
                        else -> null
                    }
                    c?.let(colours::add)
                }
            }
            b.position(end)
        }
        if (colours.isEmpty()) return null
        val name = group?.trim()?.ifEmpty { null } ?: fallback.ifEmpty { "Imported Palette" }
        return Palette(name, List(PALETTE_SLOTS) { colours.getOrNull(it) })
    }

    /** A name: its length in UTF-16 units (with the closing zero), then the units. */
    private fun utf16(b: ByteBuffer): String {
        val units = b.short.toInt() and 0xFFFF
        val chars = CharArray(units) { b.char }
        return String(chars).trimEnd('\u0000')
    }

    private fun rgb(r: Double, g: Double, b: Double): Int {
        fun byte(v: Double) = (v.coerceIn(0.0, 1.0) * 255).roundToInt()
        return (byte(r) shl 16) or (byte(g) shl 8) or byte(b)
    }

    private fun hsb(h: Double, s: Double, v: Double): Int {
        val k = { n: Double -> (n + h * 6) % 6 }
        val f = { n: Double -> v - v * s * minOf(k(n), 4 - k(n), 1.0).coerceAtLeast(0.0) }
        return rgb(f(5.0), f(3.0), f(1.0))
    }

    /** Ink amounts (0..1) to RGB, simply: no press profile. */
    private fun cmyk(c: Double, m: Double, y: Double, k: Double) = rgb((1 - c) * (1 - k), (1 - m) * (1 - k), (1 - y) * (1 - k))

    /** CIE Lab (D50, as print programs keep it) to sRGB. */
    private fun lab(l: Double, a: Double, bb: Double): Int {
        val fy = (l + 16) / 116
        val fx = fy + a / 500
        val fz = fy - bb / 200
        fun inv(t: Double) = if (t > 6.0 / 29) t * t * t else 3 * (6.0 / 29).pow(2) * (t - 4.0 / 29)
        val (x, y, z) = Triple(0.96422 * inv(fx), inv(fy), 0.82521 * inv(fz))
        // D50 XYZ to linear sRGB (Bradford-adapted to D65)
        val r = 3.1338561 * x - 1.6168667 * y - 0.4906146 * z
        val g = -0.9787684 * x + 1.9161415 * y + 0.0334540 * z
        val b = 0.0719453 * x - 0.2289914 * y + 1.4052427 * z
        fun encode(c: Double) = if (c <= 0.0031308) 12.92 * c else 1.055 * c.coerceAtLeast(0.0).pow(1 / 2.4) - 0.055
        return rgb(encode(r), encode(g), encode(b))
    }

    /** For tests: sRGB [rgb] as D50 Lab, the inverse of [lab]. */
    internal fun labOf(rgb: Int): DoubleArray {
        fun decode(c: Double) = if (c <= 0.04045) c / 12.92 else ((c + 0.055) / 1.055).pow(2.4)
        val (r, g, b) = listOf(16, 8, 0).map { decode(((rgb shr it) and 0xFF) / 255.0) }
        val x = 0.4360747 * r + 0.3850649 * g + 0.1430804 * b
        val y = 0.2225045 * r + 0.7168786 * g + 0.0606169 * b
        val z = 0.0139322 * r + 0.0971045 * g + 0.7141733 * b
        fun f(t: Double) = if (t > (6.0 / 29).pow(3)) cbrt(t) else t / (3 * (6.0 / 29).pow(2)) + 4.0 / 29
        val (fx, fy, fz) = Triple(f(x / 0.96422), f(y), f(z / 0.82521))
        return doubleArrayOf(116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz))
    }

    internal fun labToRgb(l: Double, a: Double, b: Double) = lab(l, a, b)
}
