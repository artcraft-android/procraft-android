package io.github.artcraftandroid.procraft.canvas

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMuxer
import java.io.File
import java.nio.ByteBuffer
import kotlin.math.roundToInt

/** Video files made a frame at a time with the device's encoder. */
object VideoWriter {
    private const val BITS_PER_PIXEL = 4
    private const val TIMEOUT_US = 10_000L

    /**
     * The largest size no bigger than [width] × [height] (same shape, even sides, longest side at
     * most [maxSide]) the encoder for [mime] takes; null if there is no such encoder.
     */
    fun fit(width: Int, height: Int, maxSide: Int, mime: String = MediaFormat.MIMETYPE_VIDEO_AVC): Pair<Int, Int>? {
        val caps = MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos
            .filter { it.isEncoder && it.supportedTypes.any { t -> t.equals(mime, ignoreCase = true) } }
            .mapNotNull { runCatching { it.getCapabilitiesForType(mime).videoCapabilities }.getOrNull() }
        if (caps.isEmpty()) return null
        var scale = minOf(1f, maxSide / maxOf(width, height).toFloat())
        repeat(40) {
            val w = ((width * scale).toInt() / 2 * 2).coerceAtLeast(2)
            val h = ((height * scale).toInt() / 2 * 2).coerceAtLeast(2)
            if (caps.any { it.isSizeSupported(w, h) }) return w to h
            scale *= 0.9f
        }
        return null
    }

    /**
     * [count] frames, [width] × [height] (even), [fps] a second (a fraction of a frame too, to a
     * thousandth, each frame's time exact), as an MP4 at [out] encoded with [mime]. [frame] fills a
     * frame's ARGB pixels (rows top to bottom) and says whether it has one. Each encoder that takes
     * the size is tried in the system's order (a tablet's own may refuse a size its software one
     * takes); false if none makes it.
     */
    fun write(
        out: File, width: Int, height: Int, fps: Float, count: Int, mime: String = MediaFormat.MIMETYPE_VIDEO_AVC,
        bitsPerPixel: Int = BITS_PER_PIXEL, frame: (Int, IntArray) -> Boolean,
    ): Boolean {
        for (name in encoders(mime, width, height)) {
            if (writeWith(name, out, width, height, fps, count, mime, bitsPerPixel, frame)) return true
            out.delete()
        }
        return false
    }

    /** The names of the encoders for [mime] that take [width] × [height], in the system's order. */
    private fun encoders(mime: String, width: Int, height: Int): List<String> =
        MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos
            .filter { it.isEncoder && it.supportedTypes.any { t -> t.equals(mime, ignoreCase = true) } }
            .filter { runCatching { it.getCapabilitiesForType(mime).videoCapabilities?.isSizeSupported(width, height) == true }.getOrDefault(false) }
            .map { it.name }

    /** When frame [n] starts at [fps] frames a second, in microseconds: exact for a rate in thousandths. */
    fun frameTime(n: Int, fps: Float): Long {
        val rate = io.github.artcraftandroid.procraft.core.frameRate(fps)
        return n * 1_000_000L * rate.seconds.toLong() / rate.frames.toLong().coerceAtLeast(1)
    }

    private fun writeWith(
        name: String, out: File, width: Int, height: Int, fps: Float, count: Int, mime: String, bitsPerPixel: Int,
        frame: (Int, IntArray) -> Boolean,
    ): Boolean {
        val format = MediaFormat.createVideoFormat(mime, width, height).apply {
            setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatYUV420Flexible)
            setInteger(MediaFormat.KEY_BIT_RATE, bitRate(width, height, bitsPerPixel))
            // the encoder's guide to its bits a frame, whole (encoders may refuse a rate under one);
            // the frames' times say how long each shows
            setInteger(MediaFormat.KEY_FRAME_RATE, fps.roundToInt().coerceAtLeast(1))
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1)
        }
        val codec = runCatching { MediaCodec.createByCodecName(name) }.getOrNull() ?: return false
        val muxer = MediaMuxer(out.path, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
        var track = -1
        val info = MediaCodec.BufferInfo()
        val pixels = IntArray(width * height)
        try {
            codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
            codec.start()
            fun drain(end: Boolean) {
                while (true) {
                    val index = codec.dequeueOutputBuffer(info, if (end) TIMEOUT_US else 0)
                    when {
                        index == MediaCodec.INFO_TRY_AGAIN_LATER -> if (!end) return
                        index == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                            track = muxer.addTrack(codec.outputFormat)
                            muxer.start()
                        }
                        index >= 0 -> {
                            val buffer = codec.getOutputBuffer(index)
                            if (buffer != null && info.size > 0 && track >= 0 && info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG == 0) {
                                muxer.writeSampleData(track, buffer, info)
                            }
                            codec.releaseOutputBuffer(index, false)
                            if (info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) return
                        }
                    }
                }
            }
            fun nextInput(): Int {
                var input: Int
                do {
                    input = codec.dequeueInputBuffer(TIMEOUT_US)
                    if (input < 0) drain(false)
                } while (input < 0)
                return input
            }
            var n = 0
            for (i in 0 until count) {
                if (!frame(i, pixels)) continue
                val input = nextInput()
                // without a picture to fill, the buffer could never be given back
                val image = codec.getInputImage(input) ?: return false
                writeYuv(image, pixels, width, height)
                codec.queueInputBuffer(input, 0, width * height * 3 / 2, frameTime(n, fps), 0)
                n++
                drain(false)
            }
            codec.queueInputBuffer(nextInput(), 0, 0, frameTime(n, fps), MediaCodec.BUFFER_FLAG_END_OF_STREAM)
            drain(true)
            return track >= 0
        } catch (e: Exception) {
            android.util.Log.e("ProCraft", "video", e)
            return false
        } finally {
            runCatching { codec.stop() }
            codec.release()
            runCatching { if (track >= 0) muxer.stop() }
            muxer.release()
        }
    }

    /**
     * Videos [parts] (made alike: one video track each, the same kind, size and stream settings) one
     * after another at [out], their frames copied, nothing encoded again; each part's frames follow
     * the last one's by one frame at [fps]. False if a part can't be read or was made otherwise.
     */
    fun join(parts: List<File>, out: File, fps: Float): Boolean {
        val muxer = runCatching { MediaMuxer(out.path, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4) }.getOrNull() ?: return false
        var track = -1
        var first: MediaFormat? = null
        var offset = 0L
        var buffer = ByteBuffer.allocate(0)
        val info = MediaCodec.BufferInfo()
        try {
            for (part in parts) {
                val extractor = MediaExtractor()
                try {
                    extractor.setDataSource(part.path)
                    val index = (0 until extractor.trackCount).firstOrNull {
                        extractor.getTrackFormat(it).getString(MediaFormat.KEY_MIME)?.startsWith("video/") == true
                    } ?: return false
                    val format = extractor.getTrackFormat(index)
                    val lead = first
                    if (lead == null) {
                        first = format
                        track = muxer.addTrack(format)
                        muxer.start()
                    } else if (!madeAlike(lead, format)) {
                        return false
                    }
                    val most = maxOf(
                        if (format.containsKey(MediaFormat.KEY_MAX_INPUT_SIZE)) format.getInteger(MediaFormat.KEY_MAX_INPUT_SIZE) else 0,
                        format.getInteger(MediaFormat.KEY_WIDTH) * format.getInteger(MediaFormat.KEY_HEIGHT) * 3 / 2,
                    )
                    if (buffer.capacity() < most) buffer = ByteBuffer.allocate(most)
                    extractor.selectTrack(index)
                    var last = -1L
                    while (true) {
                        val size = extractor.readSampleData(buffer, 0)
                        if (size < 0) break
                        val key = extractor.sampleFlags and MediaExtractor.SAMPLE_FLAG_SYNC != 0
                        info.set(0, size, offset + extractor.sampleTime, if (key) MediaCodec.BUFFER_FLAG_KEY_FRAME else 0)
                        muxer.writeSampleData(track, buffer, info)
                        last = maxOf(last, extractor.sampleTime)
                        extractor.advance()
                    }
                    if (last < 0) return false
                    offset += last + frameTime(1, fps)
                } finally {
                    extractor.release()
                }
            }
            return track >= 0
        } catch (e: Exception) {
            android.util.Log.e("ProCraft", "joining videos", e)
            return false
        } finally {
            runCatching { if (track >= 0) muxer.stop() }
            muxer.release()
        }
    }

    /** Whether two videos were made the same way: the same kind and size, and the same stream settings. */
    private fun madeAlike(a: MediaFormat, b: MediaFormat): Boolean {
        val same = { key: String -> a.containsKey(key) == b.containsKey(key) }
        return a.getString(MediaFormat.KEY_MIME) == b.getString(MediaFormat.KEY_MIME) &&
            a.getInteger(MediaFormat.KEY_WIDTH) == b.getInteger(MediaFormat.KEY_WIDTH) &&
            a.getInteger(MediaFormat.KEY_HEIGHT) == b.getInteger(MediaFormat.KEY_HEIGHT) &&
            listOf("csd-0", "csd-1", "csd-2").all { key -> same(key) && (!a.containsKey(key) || a.getByteBuffer(key) == b.getByteBuffer(key)) }
    }

    /** The bit rate a [width] × [height] video is encoded at, bits a second. */
    fun bitRate(width: Int, height: Int, bitsPerPixel: Int = BITS_PER_PIXEL): Int = (width * height * bitsPerPixel).coerceAtLeast(1_000_000)

    /**
     * Whether [second]'s chroma samples are [first]'s memory one byte on (the two take turns in one
     * plane, each two apart): a byte written through one is read back through the other.
     */
    private fun takeTurns(first: java.nio.ByteBuffer, second: java.nio.ByteBuffer): Boolean {
        if (first.capacity() < 2 || second.capacity() < 1) return false
        return listOf(0x5A, 0xA5).all { mark ->
            first.put(1, mark.toByte())
            second.get(0) == mark.toByte()
        }
    }

    /** ARGB pixels into the encoder's YUV 4:2:0 planes (BT.601, video range). */
    private fun writeYuv(image: android.media.Image, argb: IntArray, w: Int, h: Int) {
        val planes = image.planes
        val y = planes[0]
        val u = planes[1]
        val v = planes[2]
        val yb = y.buffer
        val ub = u.buffer
        val vb = v.buffer
        // a row at a time where a plane's samples are packed, and where the chroma's take turns in one
        // plane (U then V, or V then U, as encoders mostly take it); else one by one
        val luma = ByteArray(w)
        val half = (w + 1) / 2
        val cus = ByteArray(half)
        val cvs = ByteArray(half)
        val packedChroma = u.pixelStride == 1 && v.pixelStride == 1
        val interleaved = u.pixelStride == 2 && v.pixelStride == 2 && u.rowStride == v.rowStride
        val vAfterU = interleaved && takeTurns(ub, vb)
        val uAfterV = interleaved && !vAfterU && takeTurns(vb, ub)
        // the plane that starts first takes the row; the other's last sample goes after it
        val (lead, trail) = if (uAfterV) vb to ub else ub to vb
        val (leading, trailing) = if (uAfterV) cvs to cus else cus to cvs
        val turns = ByteArray(half * 2)
        for (row in 0 until h) {
            val even = row % 2 == 0
            for (col in 0 until w) {
                val c = argb[row * w + col]
                val r = (c shr 16) and 255
                val g = (c shr 8) and 255
                val b = c and 255
                luma[col] = (((66 * r + 129 * g + 25 * b + 128) shr 8) + 16).toByte()
                if (even && col % 2 == 0) {
                    cus[col / 2] = (((-38 * r - 74 * g + 112 * b + 128) shr 8) + 128).toByte()
                    cvs[col / 2] = (((112 * r - 94 * g - 18 * b + 128) shr 8) + 128).toByte()
                }
            }
            if (y.pixelStride == 1) {
                yb.position(row * y.rowStride)
                yb.put(luma, 0, w)
            } else {
                for (col in 0 until w) yb.put(row * y.rowStride + col * y.pixelStride, luma[col])
            }
            if (!even) continue
            val at = row / 2
            if (packedChroma) {
                ub.position(at * u.rowStride)
                ub.put(cus, 0, half)
                vb.position(at * v.rowStride)
                vb.put(cvs, 0, half)
            } else if (vAfterU || uAfterV) {
                for (i in 0 until half) {
                    turns[2 * i] = leading[i]
                    turns[2 * i + 1] = trailing[i]
                }
                lead.position(at * u.rowStride)
                lead.put(turns, 0, half * 2 - 1)
                trail.put(at * u.rowStride + (half - 1) * 2, trailing[half - 1])
            } else {
                for (i in 0 until half) {
                    ub.put(at * u.rowStride + i * u.pixelStride, cus[i])
                    vb.put(at * v.rowStride + i * v.pixelStride, cvs[i])
                }
            }
        }
    }
}
