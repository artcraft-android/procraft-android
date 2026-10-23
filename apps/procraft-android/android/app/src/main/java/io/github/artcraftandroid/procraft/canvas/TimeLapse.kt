package io.github.artcraftandroid.procraft.canvas

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import java.io.File
import java.nio.ByteBuffer

/**
 * An artwork's time-lapse: a picture of the whole canvas after each change, kept as numbered JPEG
 * frames in the artwork's folder, replayed or made into a video at [FPS] frames a second.
 */
/**
 * A time-lapse's video: 1080p, 2K or 4K ([resolution] 0 to 2), Low, Good, Studio quality or
 * Lossless ([quality] 0 to 3), and HEVC or H.264.
 */
data class TimeLapseSettings(val resolution: Int = 0, val quality: Int = 1, val hevc: Boolean = false) {
    /** The frames' longest side, pixels. */
    val side: Int get() = listOf(1920, 2732, 3840).getOrElse(resolution) { 1920 }
    val bitsPerPixel: Int get() = listOf(2, 4, 8, 16).getOrElse(quality) { 4 }
}

class TimeLapse(artwork: File) {
    private val dir = File(artwork, "timelapse")
    private val off = File(dir, "off")
    private val settingsFile = File(dir, "settings.json")

    /** The video's size, quality and codec, kept with the artwork. */
    var settings: TimeLapseSettings = runCatching {
        val j = org.json.JSONObject(settingsFile.readText())
        TimeLapseSettings(j.optInt("resolution", 0), j.optInt("quality", 1), j.optBoolean("hevc", false))
    }.getOrDefault(TimeLapseSettings())
        set(v) {
            field = v
            dir.mkdirs()
            runCatching {
                settingsFile.writeText(org.json.JSONObject().put("resolution", v.resolution).put("quality", v.quality).put("hevc", v.hevc).toString())
            }
        }

    /** Frames recorded so far. */
    var count: Int = dir.listFiles { f -> f.name.startsWith("frame-") }?.size ?: 0
        private set

    /** Whether changes are recorded (on unless turned off for this artwork). */
    var recording: Boolean
        get() = !off.exists()
        set(on) {
            dir.mkdirs()
            if (on) off.delete() else off.createNewFile()
        }

    private var last: Int? = null

    private fun file(i: Int) = File(dir, "frame-%06d.jpg".format(i))

    /** Keeps a frame (premultiplied RGBA, [width] × [height]) unless it is the same as the last. */
    fun record(rgba: ByteArray, width: Int, height: Int) {
        if (!recording || width <= 0 || height <= 0) return
        val hash = rgba.contentHashCode()
        if (hash == last) return
        last = hash
        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(rgba))
        // on white, as the canvas shows a hidden background
        val flat = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        Canvas(flat).apply {
            drawColor(Color.WHITE)
            drawBitmap(bitmap, 0f, 0f, null)
        }
        dir.mkdirs()
        runCatching { file(count).outputStream().use { flat.compress(Bitmap.CompressFormat.JPEG, JPEG_QUALITY, it) } }.onSuccess { count++ }
        bitmap.recycle()
        flat.recycle()
    }

    /** What the recorded frames take on disk, bytes. */
    fun bytes(): Long = dir.listFiles { f -> f.name.startsWith("frame-") }?.sumOf { it.length() } ?: 0L

    fun frame(i: Int): Bitmap? = runCatching { BitmapFactory.decodeFile(file(i).path) }.getOrNull()

    /**
     * The frames as an H.264 MP4 at [out]: all of them, or (with [seconds]) spread evenly over
     * that long. False if there are none or the encoder fails.
     */
    fun export(out: File, seconds: Int?): Boolean {
        if (count == 0) return false
        val picks = if (seconds != null && count > seconds * FPS) {
            val n = seconds * FPS
            List(n) { (it.toLong() * (count - 1) / (n - 1).coerceAtLeast(1)).toInt() }
        } else List(count) { it }
        val first = frame(picks.first()) ?: return false
        val video = settings
        val mime = if (video.hevc) android.media.MediaFormat.MIMETYPE_VIDEO_HEVC else android.media.MediaFormat.MIMETYPE_VIDEO_AVC
        val (w, h) = VideoWriter.fit(first.width, first.height, video.side, mime) ?: return false
        first.recycle()
        val frameBitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
        val paint = Paint(Paint.FILTER_BITMAP_FLAG)
        try {
            return VideoWriter.write(out, w, h, FPS, picks.size, mime, video.bitsPerPixel) { n, pixels ->
                val source = frame(picks[n]) ?: return@write false
                // fitted on white
                Canvas(frameBitmap).apply {
                    drawColor(Color.WHITE)
                    val s = minOf(w / source.width.toFloat(), h / source.height.toFloat())
                    val dw = (source.width * s).toInt()
                    val dh = (source.height * s).toInt()
                    drawBitmap(source, null, Rect((w - dw) / 2, (h - dh) / 2, (w + dw) / 2, (h + dh) / 2), paint)
                }
                source.recycle()
                frameBitmap.getPixels(pixels, 0, w, 0, 0, w, h)
                true
            }
        } finally {
            frameBitmap.recycle()
        }
    }

    companion object {
        const val FPS = 30
        /** A frame's longest side, pixels. */
        const val FRAME_SIDE = 1280
        private const val JPEG_QUALITY = 85
    }
}
