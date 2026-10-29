package io.github.artcraftandroid.procraft.canvas

import android.annotation.SuppressLint
import android.content.Context
import android.util.Log
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import io.github.artcraftandroid.procraft.core.CanvasSession
import io.github.artcraftandroid.procraft.core.MobileException
import io.github.artcraftandroid.procraft.core.PenSample
import io.github.artcraftandroid.procraft.core.PointerKind

/**
 * The brush studio's drawing pad: a small canvas of its own, as big as the view, painted with the
 * brush being edited. Pen and finger both paint; there are no gestures.
 */
/** How often (ms) the pad is asked whether it has drawn yet, and for how long at most. */
private const val PAD_POLL_MS = 32L
private const val PAD_POLL_LIMIT_MS = 4000L

class PadSurfaceView(context: Context, private val vm: CanvasViewModel) : SurfaceView(context), SurfaceHolder.Callback {
    private var attached = false
    private var strokePointer = -1

    init {
        // over the canvas's own surface, under the studio's controls
        setZOrderMediaOverlay(true)
        holder.addCallback(this)
    }

    override fun surfaceCreated(holder: SurfaceHolder) = Unit

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        val pad = vm.pad ?: CanvasSession(width.toUInt(), height.toUInt()).also { vm.startPad(it) }
        if (attached) {
            pad.resizeWindow(width.toUInt(), height.toUInt())
            return
        }
        val id = NativeWindows.acquire(holder.surface)
        if (id == 0L) return
        try {
            pad.attachWindow(id.toULong(), width.toUInt(), height.toUInt())
            pad.setDisplayScale(resources.displayMetrics.density)
            attached = true
            // uncovered once it has drawn a frame of its own
            uncoverWhenDrawn(pad)
        } catch (e: MobileException) {
            NativeWindows.release(id)
            Log.e("ProCraft", "attaching the pad", e)
        }
    }

    /**
     * Once the pad has presented a frame (or after a while regardless), draws its sample scribble,
     * and uncovers it (see [CanvasViewModel.padShown]) a poll later, the scribble drawn.
     */
    private fun uncoverWhenDrawn(pad: CanvasSession, waited: Long = 0L, scribbled: Boolean = false) {
        if (vm.pad !== pad) return
        if (scribbled) {
            vm.padShown = true
            return
        }
        if (waited >= PAD_POLL_LIMIT_MS || runCatching { pad.info().frames > 0uL }.getOrDefault(true)) {
            vm.padReady(pad)
            postDelayed({ uncoverWhenDrawn(pad, waited, scribbled = true) }, PAD_POLL_MS)
            return
        }
        postDelayed({ uncoverWhenDrawn(pad, waited + PAD_POLL_MS) }, PAD_POLL_MS)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        vm.pad?.detachWindow()
        attached = false
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(e: MotionEvent): Boolean {
        val pad = vm.pad ?: return true
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                requestUnbufferedDispatch(e)
                vm.padMenuOpen = false
                strokePointer = e.getPointerId(0)
                // the first stroke of one's own takes the sample scribble's place
                pad.beginOwnStroke()
                pad.strokeBegin(kindOf(e), samples(e, 0))
            }
            MotionEvent.ACTION_MOVE -> {
                val i = e.findPointerIndex(strokePointer)
                if (i >= 0) pad.strokeAdd(samples(e, i))
            }
            MotionEvent.ACTION_UP -> {
                val i = e.findPointerIndex(strokePointer)
                pad.strokeEnd(if (i >= 0) samples(e, i) else emptyList())
            }
            MotionEvent.ACTION_CANCEL -> pad.strokeCancel()
        }
        return true
    }

    private fun kindOf(e: MotionEvent): PointerKind = when (e.getToolType(0)) {
        MotionEvent.TOOL_TYPE_STYLUS -> PointerKind.STYLUS
        MotionEvent.TOOL_TYPE_ERASER -> PointerKind.STYLUS_ERASER
        else -> PointerKind.FINGER
    }

    private fun samples(e: MotionEvent, i: Int): List<PenSample> {
        val out = ArrayList<PenSample>(e.historySize + 1)
        for (h in 0 until e.historySize) {
            out += PenSample(
                e.getHistoricalX(i, h),
                e.getHistoricalY(i, h),
                e.getHistoricalPressure(i, h),
                e.getHistoricalAxisValue(MotionEvent.AXIS_TILT, i, h),
                e.getHistoricalOrientation(i, h),
                e.getHistoricalEventTime(h).toDouble(),
            )
        }
        out += PenSample(e.getX(i), e.getY(i), e.getPressure(i), e.getAxisValue(MotionEvent.AXIS_TILT, i), e.getOrientation(i), e.eventTime.toDouble())
        return out
    }
}
