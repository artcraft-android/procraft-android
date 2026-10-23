package io.github.artcraftandroid.procraft.gallery

import android.graphics.Color
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.InputStream
import java.util.zip.ZipEntry
import java.util.zip.ZipInputStream
import java.util.zip.ZipOutputStream

/**
 * Palettes as swatches files: a zip holding `Swatches.json`, the palette's name and its swatches
 * as hue, saturation and brightness (0..1), alpha and a colour space number; empty slots null.
 */
object Swatches {
    const val EXTENSION = "swatches"
    private const val ENTRY = "Swatches.json"

    /**
     * Writes palette [p] to [out]: all thirty slots, empty ones null, each colour in sRGB with the
     * profile it is in kept once beside them (named by its SHA-256 in base64).
     */
    fun write(p: Palette, out: File) {
        val icc = io.github.artcraftandroid.procraft.core.srgbProfile()
        val hash = java.util.Base64.getEncoder().encodeToString(java.security.MessageDigest.getInstance("SHA-256").digest(icc))
        val swatches = JSONArray()
        for (rgb in p.colors) {
            if (rgb == null) {
                swatches.put(JSONObject.NULL)
                continue
            }
            val hsv = FloatArray(3)
            Color.RGBToHSV((rgb shr 16) and 0xFF, (rgb shr 8) and 0xFF, rgb and 0xFF, hsv)
            val (h, sat, b) = Triple(hsv[0] / 360.0, hsv[1].toDouble(), hsv[2].toDouble())
            swatches.put(
                JSONObject().put("alpha", 1).put("origin", 1).put("colorSpace", 0).put("colorModel", 0).put("brightness", b)
                    .put("components", JSONArray().put(h).put(sat).put(b)).put("version", "5.0").put("colorProfile", hash)
                    .put("saturation", sat).put("hue", h),
            )
        }
        val profile = JSONObject().put("colorSpace", 0).put("hash", hash)
            .put("iccData", java.util.Base64.getEncoder().encodeToString(icc)).put("iccName", "sRGB IEC61966-2.1")
        val json = JSONObject().put("name", p.name).put("swatches", swatches).put("colorProfiles", JSONArray().put(profile)).toString()
        ZipOutputStream(out.outputStream()).use { zip ->
            zip.putNextEntry(ZipEntry(ENTRY))
            zip.write(json.toByteArray())
            zip.closeEntry()
        }
    }

    /** The palette in a swatches file, or null if [input] isn't one. */
    fun read(input: InputStream): Palette? = runCatching {
        ZipInputStream(input).use { zip ->
            generateSequence { zip.nextEntry }.firstOrNull { it.name.substringAfterLast('/') == ENTRY }
                ?.let { parse(zip.readBytes().decodeToString()) }
        }
    }.getOrNull()

    /** A palette from its JSON; tolerant of trailing commas. */
    private fun parse(text: String): Palette? {
        val clean = text.replace(Regex(",\\s*([}\\]])"), "$1")
        val root = clean.trim().let { if (it.startsWith("[")) JSONArray(it).optJSONObject(0) else JSONObject(it) } ?: return null
        val list = root.optJSONArray("swatches") ?: return null
        val colors = List(PALETTE_SLOTS) { i ->
            val s = if (i < list.length()) list.optJSONObject(i) else null
            s?.let {
                val hsv = floatArrayOf(it.optDouble("hue", 0.0).toFloat() * 360f, it.optDouble("saturation", 0.0).toFloat(), it.optDouble("brightness", 0.0).toFloat())
                Color.HSVToColor(hsv) and 0xFFFFFF
            }
        }
        return Palette(root.optString("name").trim().ifEmpty { "Untitled Palette" }, colors)
    }
}
