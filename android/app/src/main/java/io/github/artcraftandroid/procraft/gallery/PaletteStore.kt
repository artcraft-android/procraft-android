package io.github.artcraftandroid.procraft.gallery

import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/** Thirty swatches a palette holds: ten across, three down. */
const val PALETTE_SLOTS = 30

/** A named set of swatches (0xRRGGBB, or null for an empty slot). */
data class Palette(val name: String, val colors: List<Int?>) {
    init {
        require(colors.size == PALETTE_SLOTS)
    }
}

/** The palettes and which one is the default, kept in a JSON file. */
class PaletteStore(private val file: File) {

    fun load(): Pair<List<Palette>, Int> {
        val read = runCatching {
            val root = JSONObject(file.readText())
            val list = root.getJSONArray("palettes")
            val palettes = (0 until list.length()).map { i ->
                val p = list.getJSONObject(i)
                val colors = p.getJSONArray("colors")
                Palette(p.getString("name"), List(PALETTE_SLOTS) { j -> if (j < colors.length() && !colors.isNull(j)) colors.getInt(j) and 0xFFFFFF else null })
            }
            palettes to root.optInt("default", 0)
        }.getOrNull()
        return if (read == null || read.first.isEmpty()) starters() to 0 else read.first to read.second.coerceIn(0, read.first.size - 1)
    }

    fun save(palettes: List<Palette>, default: Int) {
        val root = JSONObject()
        root.put("default", default)
        root.put(
            "palettes",
            JSONArray(palettes.map { p -> JSONObject().put("name", p.name).put("colors", JSONArray(p.colors.map { it ?: JSONObject.NULL })) }),
        )
        val tmp = File(file.parentFile, file.name + ".tmp")
        tmp.writeText(root.toString())
        if (!tmp.renameTo(file)) {
            file.delete()
            tmp.renameTo(file)
        }
    }

    companion object {
        /** The palettes a new install starts with. */
        fun starters(): List<Palette> = listOf(
            Palette(
                "Basics",
                listOf(
                    0xFFFFFF, 0xE3E3E3, 0xC6C6C6, 0xA9A9A9, 0x8C8C8C, 0x707070, 0x545454, 0x393939, 0x1F1F1F, 0x000000,
                    0xD7263D, 0xF46036, 0xF5B700, 0x7CB518, 0x1B998B, 0x2E86AB, 0x2E4A9E, 0x6A3FA0, 0xC2377D, 0x7A4B2A,
                    0xF2A7B1, 0xF9B99A, 0xFBE38E, 0xC5E39A, 0x9AD9CF, 0xA4CCE3, 0xA3B1DD, 0xC5ADE0, 0xE7A9C8, 0xC9A88C,
                ),
            ),
            Palette(
                "Earth",
                listOf(
                    0xF6E7CF, 0xE9CFA5, 0xD8B07A, 0xC28F55, 0xA8703B, 0x8A552B, 0x6C3F20, 0x4F2D17, 0x34200F, 0x1E1309,
                    0x9FB28A, 0x7E9A6A, 0x60804F, 0x46663A, 0x324E2A, 0xB9C4C9, 0x8FA3AD, 0x66808E, 0x46606F, 0x2D4250,
                    0xE8C1A0, 0xD9A37F, 0xC4845F, 0xA86846, 0x8A5135, 0x6B3E2A, null, null, null, null,
                ),
            ),
        )
    }
}
