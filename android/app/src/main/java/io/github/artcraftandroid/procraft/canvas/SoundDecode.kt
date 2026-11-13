package io.github.artcraftandroid.procraft.canvas

import android.content.Context
import android.media.AudioFormat
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import android.net.Uri
import android.util.Log
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** The most of a recording read, seconds (the core keeps no more). */
private const val MOST_SECONDS = 60

/**
 * The sound at [uri] (whatever Android can play: MP3, AAC, OGG, FLAC, WAV...) as a WAV file of one
 * channel of floats, at most [MOST_SECONDS] of it, for a plugin's recording; null if it can't be read.
 */
fun decodedSound(context: Context, uri: Uri): ByteArray? {
    val extractor = MediaExtractor()
    var codec: MediaCodec? = null
    return try {
        extractor.setDataSource(context, uri, null)
        val track = (0 until extractor.trackCount).firstOrNull { extractor.getTrackFormat(it).getString(MediaFormat.KEY_MIME)?.startsWith("audio/") == true } ?: return null
        extractor.selectTrack(track)
        val format = extractor.getTrackFormat(track)
        var rate = format.getInteger(MediaFormat.KEY_SAMPLE_RATE)
        var channels = format.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
        var encoding = AudioFormat.ENCODING_PCM_16BIT
        val decoder = MediaCodec.createDecoderByType(format.getString(MediaFormat.KEY_MIME) ?: return null)
        codec = decoder
        decoder.configure(format, null, null, 0)
        decoder.start()
        var samples = FloatArray(1 shl 16)
        var count = 0
        val info = MediaCodec.BufferInfo()
        var fed = false
        var done = false
        val started = System.nanoTime()
        while (!done && System.nanoTime() - started < 30_000_000_000L) {
            if (!fed) {
                val i = decoder.dequeueInputBuffer(10_000)
                if (i >= 0) {
                    val input = decoder.getInputBuffer(i) ?: return null
                    val read = extractor.readSampleData(input, 0)
                    if (read < 0) {
                        decoder.queueInputBuffer(i, 0, 0, 0, MediaCodec.BUFFER_FLAG_END_OF_STREAM)
                        fed = true
                    } else {
                        decoder.queueInputBuffer(i, 0, read, extractor.sampleTime, 0)
                        extractor.advance()
                    }
                }
            }
            val o = decoder.dequeueOutputBuffer(info, 10_000)
            if (o == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                val now = decoder.outputFormat
                rate = now.getInteger(MediaFormat.KEY_SAMPLE_RATE)
                channels = now.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
                if (now.containsKey(MediaFormat.KEY_PCM_ENCODING)) encoding = now.getInteger(MediaFormat.KEY_PCM_ENCODING)
            } else if (o >= 0) {
                val out: ByteBuffer = decoder.getOutputBuffer(o) ?: return null
                out.position(info.offset)
                out.limit(info.offset + info.size)
                out.order(ByteOrder.nativeOrder())
                val frames = when (encoding) {
                    AudioFormat.ENCODING_PCM_FLOAT -> out.asFloatBuffer().let { b -> FloatArray(b.remaining() / channels) { (0 until channels).sumOf { b.get().toDouble() }.toFloat() / channels } }
                    AudioFormat.ENCODING_PCM_16BIT -> out.asShortBuffer().let { b -> FloatArray(b.remaining() / channels) { (0 until channels).sumOf { b.get().toDouble() }.toFloat() / channels / 32768f } }
                    else -> return null
                }
                if (count + frames.size > samples.size) samples = samples.copyOf(maxOf(samples.size * 2, count + frames.size))
                frames.copyInto(samples, count)
                count += frames.size
                decoder.releaseOutputBuffer(o, false)
                done = info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0 || count >= MOST_SECONDS * rate
            }
        }
        if (count == 0 || channels <= 0) return null
        wavOf(samples, minOf(count, MOST_SECONDS * rate), rate)
    } catch (e: Exception) {
        Log.w("ProCraft", "a sound couldn't be read", e)
        null
    } finally {
        runCatching { codec?.stop() }
        runCatching { codec?.release() }
        extractor.release()
    }
}

/** The first [count] of [samples] at [rate] a second as a WAV file of 32-bit floats, one channel. */
private fun wavOf(samples: FloatArray, count: Int, rate: Int): ByteArray {
    val data = count * 4
    val out = ByteBuffer.allocate(44 + data).order(ByteOrder.LITTLE_ENDIAN)
    out.put("RIFF".toByteArray()).putInt(36 + data).put("WAVEfmt ".toByteArray()).putInt(16)
    out.putShort(3).putShort(1).putInt(rate).putInt(rate * 4).putShort(4).putShort(32)
    out.put("data".toByteArray()).putInt(data)
    for (i in 0 until count) out.putFloat(samples[i])
    return out.array()
}
