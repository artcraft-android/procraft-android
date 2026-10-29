package io.github.artcraftandroid.procraft.gallery

import io.github.artcraftandroid.procraft.ui.GRADIENTS
import io.github.artcraftandroid.procraft.ui.GradientPreset
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * The gradient library (Gradient Map's gradients), kept as JSON in [file]: each gradient's name and
 * stops (position, 0xAARRGGBB). With nothing saved yet it holds ours.
 */
class GradientStore(private val file: File) {

    fun load(): List<GradientPreset> {
        val read = runCatching {
            val list = JSONObject(file.readText()).getJSONArray("gradients")
            (0 until list.length()).map { i ->
                val g = list.getJSONObject(i)
                val stops = g.getJSONArray("stops")
                GradientPreset(
                    g.getString("name"),
                    (0 until stops.length()).map { j ->
                        val s = stops.getJSONArray(j)
                        s.getDouble(0).toFloat().coerceIn(0f, 1f) to (s.getLong(1) or 0xFF000000)
                    },
                )
            }.filter { it.stops.size >= 2 }
        }.getOrNull()
        return if (read.isNullOrEmpty()) GRADIENTS else read
    }

    fun save(gradients: List<GradientPreset>) {
        val root = JSONObject().put(
            "gradients",
            JSONArray(gradients.map { g -> JSONObject().put("name", g.name).put("stops", JSONArray(g.stops.map { (p, c) -> JSONArray().put(p.toDouble()).put(c) })) }),
        )
        val tmp = File(file.parentFile, file.name + ".tmp")
        tmp.writeText(root.toString())
        if (!tmp.renameTo(file)) {
            file.delete()
            tmp.renameTo(file)
        }
    }
}
