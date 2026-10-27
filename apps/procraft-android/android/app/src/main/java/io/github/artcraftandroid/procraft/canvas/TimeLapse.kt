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
 * A time-lapse's video: 1080p, 2K or 4K ([resolution] 0 to 2), Low, Good, Studio quality or
 * Lossless ([quality] 0 to 3), and HEVC or H.264.
 */
data class TimeLapseSettings(val resolution: Int = 0, val quality: Int = 1, val hevc: Boolean = false) {
    /** The frames' longest side, pixels. */
    val side: Int get() = listOf(1920, 2732, 3840).getOrElse(resolution) { 1920 }
    val bitsPerPixel: Int get() = listOf(2, 4, 8, 16).getOrElse(quality) { 4 }
}

/**
 * An artwork's time-lapse: a picture of the whole canvas after each change, kept as numbered JPEG
 * frames in the artwork's folder, replayed or made into a video at [FPS] frames a second.
 */
class TimeLapse(artwork: File) {
    private val dir = File(artwork, "timelapse")
    private val off = File(dir, "off")
    private val settingsFile = File(dir, "settings.json")
    private val segmentsAbout = File(dir, "segments.json")

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

    private fun segmentFile(i: Int) = File(dir, "segment-${i + 1}.mp4")

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
    fun export(out: File, seconds: Int?): Boolean {
        val frames = count
        val picks = if (seconds != null && frames > seconds * FPS) {
            val n = seconds * FPS
            List(n) { (it.toLong() * (frames - 1) / (n - 1).coerceAtLeast(1)).toInt() }
        } else List(frames) { it }
        return encode(out, picks, null) != null
    }

    /**
     * The frames [picks] as a video at [out], each fitted on white into [size] (else into the first
     * one's, within the settings' side); its size, or null if there are none or the encoder fails.
     */
    private fun encode(out: File, picks: List<Int>, size: Pair<Int, Int>?): Pair<Int, Int>? {
        if (picks.isEmpty()) return null
        val video = settings
        val mime = if (video.hevc) android.media.MediaFormat.MIMETYPE_VIDEO_HEVC else android.media.MediaFormat.MIMETYPE_VIDEO_AVC
        val (w, h) = size ?: run {
            val first = frame(picks.first()) ?: return null
            VideoWriter.fit(first.width, first.height, video.side, mime).also { first.recycle() }
        } ?: return null
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
     * The videos made of the frames for packages: the settings they were made with, their size and
     * bit rate, and the frame each ends before ([ends], rising), their files numbered from 1.
     */
    private data class Segments(val video: TimeLapseSettings, val width: Int, val height: Int, val bitrate: Long, val ends: List<Int>)

    private fun readSegments(): Segments? = runCatching {
        val j = org.json.JSONObject(segmentsAbout.readText())
        val ends = j.getJSONArray("ends").let { a -> List(a.length()) { a.getInt(it) } }
        Segments(
            TimeLapseSettings(j.getInt("resolution"), j.getInt("quality"), j.getBoolean("hevc")),
            j.getInt("width"), j.getInt("height"), j.getLong("bitrate"), ends,
        )
    }.getOrNull()

    private fun writeSegments(s: Segments) {
        runCatching {
            segmentsAbout.writeText(
                org.json.JSONObject().put("resolution", s.video.resolution).put("quality", s.video.quality).put("hevc", s.video.hevc)
                    .put("width", s.width).put("height", s.height).put("bitrate", s.bitrate).put("ends", org.json.JSONArray(s.ends)).toString(),
            )
        }
    }

    /** Every package video, and the note of them, gone. */
    private fun dropSegments() {
        segmentsAbout.delete()
        dir.listFiles { f -> f.name.startsWith("segment-") && f.name.endsWith(".mp4") }?.forEach { it.delete() }
    }

    /**
     * Every frame as videos, for a package: the ones made before, and one more of the frames since
     * (so a share makes only what is new); all made again when the settings changed or there
     * would be more than [MOST_SEGMENTS]. Null with no frames, or if the encoder fails.
     */
    private fun segments(): Segments? {
        val frames = count
        val video = settings
        var kept = readSegments()?.takeIf { s ->
            s.video == video && s.width > 0 && s.height > 0 && s.ends.size < MOST_SEGMENTS &&
                (s.ends.firstOrNull() ?: 1) > 0 && s.ends.zipWithNext().all { (a, b) -> a < b } &&
                (s.ends.lastOrNull() ?: 0) <= frames && s.ends.indices.all { segmentFile(it).exists() }
        }
        if (kept == null) dropSegments()
        val done = kept?.ends?.lastOrNull() ?: 0
        if (done == frames) return kept
        // the frames since, the same size as the videos before them (another app's may be a size
        // the encoder here can't make: then all of them, at a size it can)
        val file = segmentFile(kept?.ends?.size ?: 0)
        val size = kept?.let { encode(file, (done until frames).toList(), it.width to it.height) }
        if (size != null && kept != null) {
            kept = kept.copy(ends = kept.ends + frames)
        } else {
            dropSegments()
            val whole = encode(segmentFile(0), List(frames) { it }, null) ?: return null.also { dropSegments() }
            kept = Segments(video, whole.first, whole.second, VideoWriter.bitRate(whole.first, whole.second, video.bitsPerPixel).toLong(), listOf(frames))
        }
        writeSegments(kept)
        return kept
    }

    /** The time-lapse as a package keeps it: its settings, and its frames made into videos. */
    fun forPackage(): PackageTimeLapse {
        val video = settings
        val segments = segments()
        return PackageTimeLapse(
            recording = recording,
            quality = video.quality.toUByte(),
            resolution = video.resolution.toUByte(),
            hevc = video.hevc,
            segments = segments?.ends?.indices?.map { segmentFile(it).path }.orEmpty(),
            width = (segments?.width ?: 0).toUInt(),
            height = (segments?.height ?: 0).toUInt(),
            fps = FPS.toUInt(),
            bitrate = (segments?.bitrate ?: 0L).toULong(),
        )
    }

    /**
     * Takes a package's time-lapse [t]: its settings, and its videos' frames after these. The
     * videos themselves are kept to go back into a package as they are, when every frame of them
     * came in and there were no frames before.
     */
    fun take(t: PackageTimeLapse) {
        val video = TimeLapseSettings(t.resolution.toInt(), t.quality.toInt(), t.hevc)
        settings = video
        recording = t.recording
        dropSegments()
        var whole = count == 0 && t.width > 0u && t.height > 0u
        val ends = mutableListOf<Int>()
        for (path in t.segments) {
            val before = count
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
                whole = whole && n > 0 && count - before == n
            } catch (e: Exception) {
                android.util.Log.e("ProCraft", "a time-lapse's video", e)
                whole = false
            } finally {
                retriever.release()
            }
            if (whole) whole = runCatching { File(path).copyTo(segmentFile(ends.size), overwrite = true) }.isSuccess
            ends += count
        }
        if (whole && ends.isNotEmpty() && ends.size < MOST_SEGMENTS) {
            writeSegments(Segments(video, t.width.toInt(), t.height.toInt(), t.bitrate.toLong(), ends))
        } else {
            dropSegments()
        }
    }

    companion object {
        const val FPS = 30
        /** A frame's longest side, pixels. */
        const val FRAME_SIDE = 1280
        private const val JPEG_QUALITY = 85
        /** Frames decoded together from a package's video. */
        private const val DECODE_RUN = 8
        /** A package's videos are made again as one rather than be more than this. */
        private const val MOST_SEGMENTS = 32
    }
}
