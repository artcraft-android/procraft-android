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
 *
 * As the artwork file keeps a time-lapse, its frames end up in videos only: a package's come in as they
 * are ([take]), the frames recorded become one more when the artwork is left or shared
 * ([forPackage]), their pictures then gone; [frame] reads those from the videos.
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

    /** Deletes what has been recorded; the settings, and whether it records, stay. */
    fun purge() {
        core.purge()
        forgetClips()
    }

    /** What the recorded frames take on disk, bytes. */
    fun bytes(): Long = core.bytes().toLong()

    /** Frame [i]: its picture, or read from the video it is in. */
    fun frame(i: Int): Bitmap? =
        core.frameFile(i.toUInt())?.let { f -> runCatching { BitmapFactory.decodeFile(f) }.getOrNull() } ?: clipFrame(i)

    private val clipLock = Any()
    /** The videos the frames are in, the one being read, and the frames last read from it (a few at a time, the replay and the export reading them in order): the first one's number and them. */
    private var clips: List<io.github.artcraftandroid.procraft.core.TimeLapseClip>? = null
    private var reading: Pair<String, android.media.MediaMetadataRetriever>? = null
    private var decoded: Pair<Int, List<Bitmap>>? = null

    private fun clipFrame(i: Int): Bitmap? = synchronized(clipLock) {
        decoded?.let { (first, frames) -> frames.getOrNull(i - first)?.let { return it.copy(Bitmap.Config.ARGB_8888, false) } }
        val all = clips ?: core.clips().also { clips = it }
        val clip = all.firstOrNull { i >= it.first.toInt() && i - it.first.toInt() < it.frames.toInt() } ?: return null
        val retriever = reading?.takeIf { it.first == clip.path }?.second ?: try {
            reading?.second?.release()
            reading = null
            android.media.MediaMetadataRetriever().also { r ->
                r.setDataSource(clip.path)
                reading = clip.path to r
            }
        } catch (e: Exception) {
            android.util.Log.e("ProCraft", "a time-lapse's video", e)
            return null
        }
        val at = i - clip.first.toInt()
        val frames = runCatching { retriever.getFramesAtIndex(at, minOf(CLIP_RUN, clip.frames.toInt() - at)) }
            .onFailure { android.util.Log.w("ProCraft", "a time-lapse video's frames by number", it) }.getOrNull().orEmpty()
        if (frames.isEmpty()) return frameAtTime(retriever, at, clip.frames.toInt())
        decoded?.second?.forEach { it.recycle() }
        decoded = i to frames
        frames.first().copy(Bitmap.Config.ARGB_8888, false)
    }

    /** Frame [at] of a video of [frames] by its time: not every device's retriever goes by frame number. */
    private fun frameAtTime(retriever: android.media.MediaMetadataRetriever, at: Int, frames: Int): Bitmap? = runCatching {
        val length = retriever.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull()?.times(1000)
            ?: (frames * 1_000_000L / FPS)
        retriever.getFrameAtTime(at * length / frames, android.media.MediaMetadataRetriever.OPTION_CLOSEST)
    }.onFailure { android.util.Log.w("ProCraft", "a time-lapse video's frame by time", it) }.getOrNull()

    /** The videos read again next time (they changed), and the one being read let go. */
    private fun forgetClips() = synchronized(clipLock) {
        clips = null
        reading?.second?.release()
        reading = null
        decoded?.second?.forEach { it.recycle() }
        decoded = null
    }

    /** What it holds while reading its videos let go (the artwork is left). */
    fun close() = forgetClips()

    /**
     * The frames as an H.264 MP4 at [out]: all of them, or (with [seconds]) spread evenly over
     * that long. [going] hears how far it has come (0..1) before each frame, and stops it by saying
     * false. False if there are none or the encoder fails.
     */
    fun export(out: File, seconds: Int?, going: (Float) -> Boolean = { true }): Boolean {
        val picks = timeLapsePicks(count.toUInt(), seconds?.toUInt()).map { it.toInt() }
        // all of it, as asked or too short to cut: the videos kept for packages, made where they
        // don't reach yet, put end to end, nothing read back from them
        if (picks.size == count) joined(out, going)?.let { return it }
        return encode(out, picks, null, going = going) != null
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
            val made = encode(File(plan.path), plan.frames.map { it.toInt() }, size, plan.hevc) { done ->
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
        dropClipFrames()
        val parts = core.forPackage().segments.map(::File)
        return if (parts.isNotEmpty() && VideoWriter.join(parts, out, FPS.toFloat())) true else null
    }

    /**
     * The frames [picks] as a video at [out], HEVC or H.264 as [hevc] says, each fitted on white
     * into [size] (else into the first one's, within the settings' side); its size, or null if
     * there are none or the encoder fails. [going] as for [export]: the frames after it says false
     * are left out.
     */
    private fun encode(
        out: File,
        picks: List<Int>,
        size: Pair<Int, Int>?,
        hevc: Boolean = settings.hevc,
        going: (Float) -> Boolean = { true },
    ): Pair<Int, Int>? {
        if (picks.isEmpty()) return null
        val video = settings
        val mime = if (hevc) android.media.MediaFormat.MIMETYPE_VIDEO_HEVC else android.media.MediaFormat.MIMETYPE_VIDEO_AVC
        val (w, h) = size ?: run {
            val first = frame(picks.first()) ?: return null
            VideoWriter.fit(first.width, first.height, video.side, mime).also { first.recycle() }
        } ?: return null
        val frameBitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
        val paint = Paint(Paint.FILTER_BITMAP_FLAG)
        try {
            val ok = VideoWriter.write(out, w, h, FPS.toFloat(), picks.size, mime, video.bitsPerPixel) { n, pixels ->
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
     * ones made before kept, so a share makes only what is new; the pictures they hold then gone,
     * as the artwork file keeps a time-lapse: only as video). Leaving the artwork does this too.
     */
    fun forPackage(): PackageTimeLapse {
        var plan = core.segmentPlan()
        while (plan != null) {
            val size = if (plan.width > 0u && plan.height > 0u) plan.width.toInt() to plan.height.toInt() else null
            val made = encode(File(plan.path), plan.frames.map { it.toInt() }, size, plan.hevc)
            plan = core.segmentMade(plan, (made?.first ?: 0).toUInt(), (made?.second ?: 0).toUInt(), made != null)
        }
        dropClipFrames()
        return core.forPackage()
    }

    /**
     * The pictures its videos hold too gone (the core checks each video's frames first), once
     * this device has read those videos back: where it can't, the pictures are all there is to
     * replay.
     */
    private fun dropClipFrames() {
        forgetClips()
        val readable = core.clipsWithPictures().all { clip ->
            listOf(clip.first, clip.first + clip.frames - 1u).all { f -> clipFrame(f.toInt())?.also { it.recycle() } != null }
        }
        forgetClips()
        if (!readable) {
            android.util.Log.w("ProCraft", "the time-lapse's videos can't be read here: their pictures stay")
            return
        }
        val gone = core.dropClipFrames()
        if (gone > 0u) android.util.Log.i("ProCraft", "the time-lapse's videos hold $gone frames: their pictures went")
    }

    /**
     * Takes a package's time-lapse [t]: as the package keeps it, its videos as they are (nothing
     * decoded); else (a video whose frames can't be counted) its settings, and its videos' frames
     * after these, the videos themselves kept to go back into a package as they are when every
     * frame of them came in and there were no frames before.
     */
    fun take(t: PackageTimeLapse) {
        if (core.takeClips(t)) return
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
        /** Frames decoded together from a package's video, and from one the frames are in. */
        private const val DECODE_RUN = 8
        private const val CLIP_RUN = 15
    }
}
