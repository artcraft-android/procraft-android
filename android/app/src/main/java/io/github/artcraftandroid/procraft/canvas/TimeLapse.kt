package io.github.artcraftandroid.procraft.canvas

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import io.github.artcraftandroid.procraft.core.PackageTimeLapse
import io.github.artcraftandroid.procraft.core.timeLapseBitsPerPixel
import io.github.artcraftandroid.procraft.core.timeLapseFps
import io.github.artcraftandroid.procraft.core.timeLapsePicks
import io.github.artcraftandroid.procraft.core.timeLapseSide
import java.io.File
import java.nio.ByteBuffer
import io.github.artcraftandroid.procraft.core.TimeLapse as CoreTimeLapse
import io.github.artcraftandroid.procraft.core.TimeLapseSettings as CoreTimeLapseSettings

/**
 * A time-lapse's video: 1080p, 2K or 4K ([resolution] 0 to 2), Low, Good, Studio quality or
 * Lossless ([quality] 0 to 3), and HEVC or H.264.
 */
data class TimeLapseSettings(val resolution: Int = 0, val quality: Int = 1, val hevc: Boolean = false) {
    /** As the shared core takes them. */
    fun core() = CoreTimeLapseSettings(resolution.toUInt(), quality.toUInt(), hevc)

    /** The frames' longest side, pixels. */
    val side: Int get() = timeLapseSide(core()).toInt()
    val bitsPerPixel: Int get() = timeLapseBitsPerPixel(core()).toInt()
}

/**
 * An artwork's time-lapse: a picture of the whole canvas after each change, kept as numbered JPEG
 * frames in the artwork's folder, replayed or made into a video at [FPS] frames a second. The
 * shared core keeps the frames, the settings and a package's videos (crates/core timelapse.rs);
 * this makes and reads the videos with the system's codecs.
 */
class TimeLapse(artwork: File) {
    private val core = CoreTimeLapse(artwork.path)

    /** The video's size, quality and codec, kept with the artwork. */
    var settings: TimeLapseSettings
        get() = core.settings().let { TimeLapseSettings(it.resolution.toInt(), it.quality.toInt(), it.hevc) }
        set(v) = core.setSettings(v.core())

    /** Frames recorded so far. */
    val count: Int get() = core.count().toInt()

    /** Whether changes are recorded (on unless turned off for this artwork). */
    var recording: Boolean
        get() = core.recording()
        set(on) = core.setRecording(on)

    /** Keeps a frame (premultiplied RGBA, [width] × [height]) unless it is the same as the last. */
    fun record(rgba: ByteArray, width: Int, height: Int) {
        core.record(rgba, width.toUInt(), height.toUInt())
    }

    /** What the recorded frames take on disk, bytes. */
    fun bytes(): Long = core.bytes().toLong()

    fun frame(i: Int): Bitmap? = core.frameFile(i.toUInt())?.let { runCatching { BitmapFactory.decodeFile(it) }.getOrNull() }

    /**
     * The frames as an H.264 MP4 at [out]: all of them, or (with [seconds]) spread evenly over
     * that long. [going] hears how far it has come (0..1) before each frame, and stops it by saying
     * false. False if there are none or the encoder fails.
     */
    fun export(out: File, seconds: Int?, going: (Float) -> Boolean = { true }): Boolean {
        // all of it: the videos kept for packages, made where they don't reach yet, put end to end
        if (seconds == null) joined(out, going)?.let { return it }
        return encode(out, timeLapsePicks(count.toUInt(), seconds?.toUInt()).map { it.toInt() }, null, going) != null
    }

    /**
     * Every frame at [out] as the package's videos joined: the frames in none yet made into one
     * first (as [forPackage] makes them), the videos then copied one after another, nothing made
     * again. False if [going] stopped it (the videos kept as they were); null if they can't be
     * joined, so it is made afresh.
     */
    private fun joined(out: File, going: (Float) -> Boolean): Boolean? {
        val total = count
        if (total <= 0) return null
        var plan = core.segmentPlan()
        while (plan != null) {
            val todo = plan.frames.size
            val before = total - todo
            var stopped = false
            val size = if (plan.width > 0u && plan.height > 0u) plan.width.toInt() to plan.height.toInt() else null
            val made = encode(File(plan.path), plan.frames.map { it.toInt() }, size) { done ->
                going((before + done * todo) / total).also { stopped = !it }
            }
            if (stopped) {
                // not kept: the next one makes it again
                File(plan.path).delete()
                return false
            }
            plan = core.segmentMade(plan, (made?.first ?: 0).toUInt(), (made?.second ?: 0).toUInt(), made != null)
            if (made == null && plan == null) return null
        }
        val parts = core.forPackage().segments.map(::File)
        return if (parts.isNotEmpty() && VideoWriter.join(parts, out, FPS)) true else null
    }

    /**
     * The frames [picks] as a video at [out], each fitted on white into [size] (else into the first
     * one's, within the settings' side); its size, or null if there are none or the encoder fails.
     * [going] as for [export]: the frames after it says false are left out.
     */
    private fun encode(out: File, picks: List<Int>, size: Pair<Int, Int>?, going: (Float) -> Boolean = { true }): Pair<Int, Int>? {
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
                if (!going(n / picks.size.toFloat())) return@write false
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
     * The time-lapse as a package keeps it: its settings, and its frames made into videos (the
     * ones made before kept, so a share makes only what is new).
     */
    fun forPackage(): PackageTimeLapse {
        var plan = core.segmentPlan()
        while (plan != null) {
            val size = if (plan.width > 0u && plan.height > 0u) plan.width.toInt() to plan.height.toInt() else null
            val made = encode(File(plan.path), plan.frames.map { it.toInt() }, size)
            plan = core.segmentMade(plan, (made?.first ?: 0).toUInt(), (made?.second ?: 0).toUInt(), made != null)
        }
        return core.forPackage()
    }

    /**
     * Takes a package's time-lapse [t]: its settings, and its videos' frames after these. The
     * videos themselves are kept to go back into a package as they are, when every frame of them
     * came in and there were no frames before.
     */
    fun take(t: PackageTimeLapse) {
        core.takeBegin(t)
        for (path in t.segments) {
            var frames = 0
            val retriever = android.media.MediaMetadataRetriever()
            try {
                retriever.setDataSource(path)
                frames = retriever.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_VIDEO_FRAME_COUNT)?.toIntOrNull() ?: 0
                // a few at a time: one seek for each run, not each frame
                var i = 0
                while (i < frames) {
                    val run = minOf(DECODE_RUN, frames - i)
                    val pictures = runCatching { retriever.getFramesAtIndex(i, run) }.getOrNull() ?: break
                    pictures.forEach { keepFrame(it); it.recycle() }
                    i += run
                }
            } catch (e: Exception) {
                android.util.Log.e("ProCraft", "a time-lapse's video", e)
                frames = 0
            } finally {
                retriever.release()
            }
            core.takeVideo(path, frames.toUInt())
        }
        core.takeEnd()
    }

    /** A package video's frame [picture] kept as the next frame. */
    private fun keepFrame(picture: Bitmap) {
        val argb = if (picture.config == Bitmap.Config.ARGB_8888) picture else picture.copy(Bitmap.Config.ARGB_8888, false)
        val rgba = ByteBuffer.allocate(argb.width * argb.height * 4).also { argb.copyPixelsToBuffer(it) }.array()
        core.keepFrame(rgba, argb.width.toUInt(), argb.height.toUInt())
        if (argb !== picture) argb.recycle()
    }

    companion object {
        val FPS = timeLapseFps().toInt()
        /** Frames decoded together from a package's video. */
        private const val DECODE_RUN = 8
    }
}
