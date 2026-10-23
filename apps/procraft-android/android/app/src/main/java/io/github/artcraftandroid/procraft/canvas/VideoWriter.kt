package io.github.artcraftandroid.procraft.canvas

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaFormat
import android.media.MediaMuxer
import java.io.File

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
     * [count] frames, [width] × [height] (even), [fps] a second, as an MP4 at [out] encoded with
     * [mime]. [frame] fills a frame's ARGB pixels (rows top to bottom) and says whether it has one.
     * False if the encoder fails.
     */
    fun write(
        out: File, width: Int, height: Int, fps: Int, count: Int, mime: String = MediaFormat.MIMETYPE_VIDEO_AVC,
        bitsPerPixel: Int = BITS_PER_PIXEL, frame: (Int, IntArray) -> Boolean,
    ): Boolean {
        val format = MediaFormat.createVideoFormat(mime, width, height).apply {
            setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatYUV420Flexible)
            setInteger(MediaFormat.KEY_BIT_RATE, (width * height * bitsPerPixel).coerceAtLeast(1_000_000))
            setInteger(MediaFormat.KEY_FRAME_RATE, fps)
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1)
        }
        val codec = runCatching { MediaCodec.createEncoderByType(mime) }.getOrNull() ?: return false
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
                val image = codec.getInputImage(input) ?: continue
                writeYuv(image, pixels, width, height)
                codec.queueInputBuffer(input, 0, width * height * 3 / 2, n * 1_000_000L / fps, 0)
                n++
                drain(false)
            }
            codec.queueInputBuffer(nextInput(), 0, 0, n * 1_000_000L / fps, MediaCodec.BUFFER_FLAG_END_OF_STREAM)
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

    /** ARGB pixels into the encoder's YUV 4:2:0 planes (BT.601, video range). */
    private fun writeYuv(image: android.media.Image, argb: IntArray, w: Int, h: Int) {
        val planes = image.planes
        val y = planes[0]
        val u = planes[1]
        val v = planes[2]
        for (row in 0 until h) {
            for (col in 0 until w) {
                val c = argb[row * w + col]
                val r = (c shr 16) and 255
                val g = (c shr 8) and 255
                val b = c and 255
                y.buffer.put(row * y.rowStride + col * y.pixelStride, (((66 * r + 129 * g + 25 * b + 128) shr 8) + 16).toByte())
                if (row % 2 == 0 && col % 2 == 0) {
                    val cu = ((-38 * r - 74 * g + 112 * b + 128) shr 8) + 128
                    val cv = ((112 * r - 94 * g - 18 * b + 128) shr 8) + 128
                    u.buffer.put((row / 2) * u.rowStride + (col / 2) * u.pixelStride, cu.toByte())
                    v.buffer.put((row / 2) * v.rowStride + (col / 2) * v.pixelStride, cv.toByte())
                }
            }
        }
    }
}
