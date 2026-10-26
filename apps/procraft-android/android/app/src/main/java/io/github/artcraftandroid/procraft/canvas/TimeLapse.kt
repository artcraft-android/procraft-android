package io.github.artcraftandroid.procraft.canvas

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import io.github.artcraftandroid.procraft.core.PackageTimeLapse
import java.io.File
import java.nio.ByteBuffer
import kotlin.math.roundToInt

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
    private val segmentFile = File(dir, "segment.mp4")
    private val segmentAbout = File(dir, "segment.json")

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
        keep(flat)
        bitmap.recycle()
        flat.recycle()
    }

    /** Keeps [picture] as the next frame, at most [FRAME_SIDE] on its long side. */
    private fun keep(picture: Bitmap) {
        val s = minOf(1f, FRAME_SIDE / maxOf(picture.width, picture.height).toFloat())
        val frame = if (s < 1f) {
            Bitmap.createScaledBitmap(picture, (picture.width * s).roundToInt().coerceAtLeast(1), (picture.height * s).roundToInt().coerceAtLeast(1), true)
        } else picture
        dir.mkdirs()
        runCatching { file(count).outputStream().use { frame.compress(Bitmap.CompressFormat.JPEG, JPEG_QUALITY, it) } }.onSuccess { count++ }
        if (frame !== picture) frame.recycle()
    }

    /** What the recorded frames take on disk, bytes. */
    fun bytes(): Long = dir.listFiles { f -> f.name.startsWith("frame-") }?.sumOf { it.length() } ?: 0L

    fun frame(i: Int): Bitmap? = runCatching { BitmapFactory.decodeFile(file(i).path) }.getOrNull()

    /**
     * The frames as an H.264 MP4 at [out]: all of them, or (with [seconds]) spread evenly over
     * that long. False if there are none or the encoder fails.
     */
    fun export(out: File, seconds: Int?): Boolean = encode(out, seconds, count) != null

    /** The first [frames] frames as a video at [out], as [export] makes it; its size, or null. */
    private fun encode(out: File, seconds: Int?, frames: Int): Pair<Int, Int>? {
        if (frames == 0) return null
        val picks = if (seconds != null && frames > seconds * FPS) {
            val n = seconds * FPS
            List(n) { (it.toLong() * (frames - 1) / (n - 1).coerceAtLeast(1)).toInt() }
        } else List(frames) { it }
        val first = frame(picks.first()) ?: return null
        val video = settings
        val mime = if (video.hevc) android.media.MediaFormat.MIMETYPE_VIDEO_HEVC else android.media.MediaFormat.MIMETYPE_VIDEO_AVC
        val (w, h) = VideoWriter.fit(first.width, first.height, video.side, mime) ?: return null
        first.recycle()
        val frameBitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
        val paint = Paint(Paint.FILTER_BITMAP_FLAG)
        try {
            val ok = VideoWriter.write(out, w, h, FPS, picks.size, mime, video.bitsPerPixel) { n, pixels ->
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
            return if (ok) w to h else null
        } finally {
            frameBitmap.recycle()
        }
    }

    /**
     * Every frame as one video, for a package, with its size and bit rate: made again only when
     * frames were added or the settings changed. Null with no frames, or if the encoder fails.
     */
    private fun segment(): Triple<File, Pair<Int, Int>, Long>? {
        val frames = count
        if (frames == 0) return null
        val video = settings
        runCatching {
            val j = org.json.JSONObject(segmentAbout.readText())
            if (segmentFile.exists() && j.getInt("frames") == frames && j.getInt("resolution") == video.resolution &&
                j.getInt("quality") == video.quality && j.getBoolean("hevc") == video.hevc
            ) return Triple(segmentFile, j.getInt("width") to j.getInt("height"), j.getLong("bitrate"))
        }
        segmentAbout.delete()
        val size = encode(segmentFile, null, frames) ?: return null.also { segmentFile.delete() }
        val bitrate = VideoWriter.bitRate(size.first, size.second, video.bitsPerPixel).toLong()
        runCatching {
            segmentAbout.writeText(
                org.json.JSONObject().put("frames", frames).put("resolution", video.resolution).put("quality", video.quality)
                    .put("hevc", video.hevc).put("width", size.first).put("height", size.second).put("bitrate", bitrate).toString(),
            )
        }
        return Triple(segmentFile, size, bitrate)
    }

    /** The time-lapse as a package keeps it: its settings, and its frames made into a video. */
    fun forPackage(): PackageTimeLapse {
        val video = settings
        val segment = segment()
        return PackageTimeLapse(
            recording = recording,
            quality = video.quality.toUByte(),
            resolution = video.resolution.toUByte(),
            hevc = video.hevc,
            segments = listOfNotNull(segment?.first?.path),
            width = (segment?.second?.first ?: 0).toUInt(),
            height = (segment?.second?.second ?: 0).toUInt(),
            fps = FPS.toUInt(),
            bitrate = (segment?.third ?: 0L).toULong(),
        )
    }

    /** Takes a package's time-lapse [t]: its settings, and its video's frames after these. */
    fun take(t: PackageTimeLapse) {
        settings = TimeLapseSettings(t.resolution.toInt(), t.quality.toInt(), t.hevc)
        recording = t.recording
        for (path in t.segments) {
            val retriever = android.media.MediaMetadataRetriever()
            try {
                retriever.setDataSource(path)
                val n = retriever.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_VIDEO_FRAME_COUNT)?.toIntOrNull() ?: 0
                // a few at a time: one seek for each run, not each frame
                var i = 0
                while (i < n) {
                    val run = minOf(DECODE_RUN, n - i)
                    val pictures = runCatching { retriever.getFramesAtIndex(i, run) }.getOrNull() ?: break
                    pictures.forEach { keep(it); it.recycle() }
                    i += run
                }
            } catch (e: Exception) {
                android.util.Log.e("ProCraft", "a time-lapse's video", e)
            } finally {
                retriever.release()
            }
        }
    }

    companion object {
        const val FPS = 30
        /** A frame's longest side, pixels. */
        const val FRAME_SIDE = 1280
        private const val JPEG_QUALITY = 85
        /** Frames decoded together from a package's video. */
        private const val DECODE_RUN = 8
    }
}
