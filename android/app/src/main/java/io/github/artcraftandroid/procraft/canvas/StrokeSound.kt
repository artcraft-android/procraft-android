package io.github.artcraftandroid.procraft.canvas

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import io.github.artcraftandroid.procraft.core.CanvasSession

/**
 * Brush sounds (a plugin's): plays the sound the core makes of the stroke being drawn, a little at a
 * time on a thread of its own, while [follow] says so.
 */
class StrokeSound {
    /** The artwork whose strokes are heard; none: silence. */
    @Volatile
    private var heard: CanvasSession? = null

    @Volatile
    private var running = false
    private var thread: Thread? = null

    /** Plays [session]'s strokes while [on] (a plugin makes their sound); stops otherwise. */
    fun follow(session: CanvasSession?, on: Boolean) {
        val wanted = on && session != null
        heard = if (wanted) session else null
        if (wanted && thread == null) {
            running = true
            thread = Thread(::play, "procraft-stroke-sound").also { it.start() }
        } else if (!wanted && thread != null) {
            running = false
            thread?.join(500)
            thread = null
        }
    }

    private fun play() {
        val format = AudioFormat.Builder()
            .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
            .setSampleRate(RATE)
            .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
            .build()
        val least = AudioTrack.getMinBufferSize(RATE, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_FLOAT)
        val track = runCatching {
            AudioTrack.Builder()
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_GAME)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                        .build(),
                )
                .setAudioFormat(format)
                .setBufferSizeInBytes(maxOf(least, FRAMES * 4 * 3))
                .setTransferMode(AudioTrack.MODE_STREAM)
                .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                .build()
        }.getOrNull() ?: return
        val silence = FloatArray(FRAMES)
        try {
            track.play()
            while (running) {
                val sound = heard?.let { s -> runCatching { s.strokeSound(FRAMES.toUInt(), RATE.toUInt()).toFloatArray() }.getOrNull() }
                val pcm = sound?.takeIf { it.isNotEmpty() } ?: silence
                track.write(pcm, 0, pcm.size, AudioTrack.WRITE_BLOCKING)
            }
        } finally {
            runCatching { track.stop() }
            track.release()
        }
    }

    private companion object {
        /** The sound's rate, and how much is written at once (10 ms). */
        const val RATE = 48_000
        const val FRAMES = 480
    }
}
