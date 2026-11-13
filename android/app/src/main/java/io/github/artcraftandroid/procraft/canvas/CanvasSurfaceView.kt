package io.github.artcraftandroid.procraft.canvas

import android.annotation.SuppressLint
import android.content.Context
import android.util.Log
import android.view.MotionEvent
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.compose.ui.geometry.Offset
import androidx.input.motionprediction.MotionEventPredictor
import io.github.artcraftandroid.procraft.core.CanvasSession
import io.github.artcraftandroid.procraft.core.MobileException
import io.github.artcraftandroid.procraft.core.PenSample
import io.github.artcraftandroid.procraft.core.CompanionMode
import io.github.artcraftandroid.procraft.core.PointerKind
import io.github.artcraftandroid.procraft.core.TouchCommand
import io.github.artcraftandroid.procraft.core.TouchMove
import io.github.artcraftandroid.procraft.core.TransformHandle
import kotlin.math.atan2
import io.github.artcraftandroid.procraft.core.Adjustment

/**
 * The canvas: a surface the Rust renderer draws into, and the touch input that drives it.
 *
 * One pen or finger paints. A second finger turns a finger stroke into a gesture (the stroke is
 * dropped): two fingers move, zoom and turn the canvas; a quick tap with two fingers undoes, three
 * redo, four hide or show the controls. Fingers are ignored while the pen draws.
 */
@SuppressLint("ViewConstructor")
class CanvasSurfaceView(context: Context, private val vm: CanvasViewModel) : SurfaceView(context), SurfaceHolder.Callback {
    private var session: CanvasSession? = null
    private var attached = false

    // the window as last given to the session, and the display rotation it was in
    private var windowWidth = 0
    private var windowHeight = 0
    private var windowRotation = Surface.ROTATION_0

    /** Predicts where the pen is going, so the stroke can be drawn ahead of the latest input. */
    private val predictor = MotionEventPredictor.newInstance(this)

    /** The pen is hovering over the canvas. */
    private var penHovering = false

    /** The pen's side buttons last seen held. */
    private var penButtons = 0

    /** Where the pen last was over the canvas: a side button that comes as a key says nothing of it. */
    private var penAt: Offset? = null

    private var strokePointer = -1

    private val pickSide get() = (CanvasViewModel.PICK_WINDOW_DP * vm.density).toInt() or 1

    init {
        holder.addCallback(this)
    }

    override fun surfaceCreated(holder: SurfaceHolder) = Unit

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        val s = vm.session ?: return
        val rotation = display?.rotation ?: Surface.ROTATION_0
        if (attached && session === s) {
            if (rotation != windowRotation) followTurn(s, windowRotation, rotation)
            s.resizeWindow(width.toUInt(), height.toUInt())
            vm.refreshView()
            remember(width, height, rotation)
            return
        }
        val id = NativeWindows.acquire(holder.surface)
        if (id == 0L) {
            Log.e(TAG, "no native window for the surface")
            return
        }
        try {
            s.attachWindow(id.toULong(), width.toUInt(), height.toUInt())
            s.setDisplayScale(resources.displayMetrics.density)
            // the display turned while there was no window (the app away, the upright lock screen):
            // the view follows now, as it would have then, so the canvas stays put on the glass
            if (session === s && windowWidth > 0 && rotation != windowRotation) followTurn(s, windowRotation, rotation)
            session = s
            attached = true
            vm.refreshView()
            remember(width, height, rotation)
        } catch (e: MobileException) {
            NativeWindows.release(id)
            Log.e(TAG, "attaching the window", e)
        }
    }

    private fun remember(width: Int, height: Int, rotation: Int) {
        vm.session?.let { placeGrid(it, width, height, rotation) }
        vm.screenCentre = Offset(width / 2f, height / 2f)
        windowWidth = width
        windowHeight = height
        vm.windowSize = Offset(width.toFloat(), height.toFloat())
        vm.screenDensity = resources.displayMetrics.density
        windowRotation = rotation
    }

    /**
     * The surround's grid is fixed to the glass as the canvas is: its 1 pt lines lie every 16 pt
     * from the panel's own top-left corner in its natural orientation, so in the window a line's
     * middle is half a point in from whichever corner that is.
     */
    private fun placeGrid(s: CanvasSession, width: Int, height: Int, rotation: Int) {
        val half = 0.5f * resources.displayMetrics.density
        val (nw, nh) = if (rotation % 2 == 0) width.toFloat() to height.toFloat() else height.toFloat() to width.toFloat()
        val (x, y) = when (rotation) {
            Surface.ROTATION_90 -> half to nw - half
            Surface.ROTATION_180 -> nw - half to nh - half
            Surface.ROTATION_270 -> nh - half to half
            else -> half to half
        }
        s.setGridOrigin(x, y)
        vm.gridOrigin = Offset(x, y)
    }

    /**
     * The display turned from rotation [from] to [to]: the view turns the other way, so the canvas
     * stays where it was on the glass while the bars turn upright.
     */
    private fun followTurn(s: CanvasSession, from: Int, to: Int) {
        // the panel's own size, in its natural orientation
        val (nw, nh) = if (from % 2 == 0) windowWidth.toFloat() to windowHeight.toFloat() else windowHeight.toFloat() to windowWidth.toFloat()
        fun toPanel(r: Int, x: Float, y: Float) = when (r) {
            Surface.ROTATION_90 -> nw - y to x
            Surface.ROTATION_180 -> nw - x to nh - y
            Surface.ROTATION_270 -> y to nh - x
            else -> x to y
        }
        fun fromPanel(r: Int, x: Float, y: Float) = when (r) {
            Surface.ROTATION_90 -> y to nw - x
            Surface.ROTATION_180 -> nw - x to nh - y
            Surface.ROTATION_270 -> nh - y to x
            else -> x to y
        }
        fun map(x: Float, y: Float) = toPanel(from, x, y).let { (px, py) -> fromPanel(to, px, py) }
        val (ox, oy) = map(0f, 0f)
        val (ux, uy) = map(1f, 0f)
        s.viewGesture(0f, 0f, ox, oy, 1f, atan2(uy - oy, ux - ox))
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        // returns once the render thread has stopped drawing into the surface
        session?.detachWindow()
        attached = false
    }

    // every touch is the shared touches' (crates/core touch.rs): painting, picking, the snapped shape,
    // the gestures and every tool's touch, the same in every app

    /** Contacts too big for a fingertip (a palm, on a screen that says how big a touch is): the shared touches never see them, and one that grows that big is taken back. */
    private val palms = mutableSetOf<Int>()

    /** Whether pointer [i] of [e] is a finger's contact [PALM_MM] or more across. */
    private fun palmSized(e: MotionEvent, i: Int): Boolean {
        if (e.getToolType(i) != MotionEvent.TOOL_TYPE_FINGER) return false
        val dpi = resources.displayMetrics.xdpi.takeIf { it > 0f } ?: return false
        return e.getTouchMajor(i) / dpi * 25.4f >= PALM_MM
    }

    /** Hands [e] to the shared touches and does what they ask. */
    private fun route(e: MotionEvent, s: CanvasSession) {
        val out = when (e.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                val i = e.actionIndex
                if (e.actionMasked == MotionEvent.ACTION_DOWN) {
                    strokePointer = e.getPointerId(i)
                    palms.clear()
                }
                if (palmSized(e, i)) {
                    palms += e.getPointerId(i)
                    emptyList()
                } else {
                    s.touchDown(e.getPointerId(i).toULong(), kindOf(e, i), samples(e, i))
                }
            }
            MotionEvent.ACTION_MOVE -> {
                val grown = (0 until e.pointerCount).filter { i -> e.getPointerId(i) !in palms && palmSized(e, i) }
                val back = grown.flatMap { i -> palms += e.getPointerId(i); s.touchCancelOne(e.getPointerId(i).toULong()) }
                back + s.touchMoves((0 until e.pointerCount).filter { i -> e.getPointerId(i) !in palms }.map { i -> TouchMove(e.getPointerId(i).toULong(), samples(e, i)) })
            }
            MotionEvent.ACTION_POINTER_UP, MotionEvent.ACTION_UP -> {
                val i = e.actionIndex
                when {
                    palms.remove(e.getPointerId(i)) -> emptyList()
                    // the system took the pointer back (a palm): what it did is undone, not ended
                    e.flags and MotionEvent.FLAG_CANCELED != 0 -> s.touchCancelOne(e.getPointerId(i).toULong())
                    else -> s.touchUp(e.getPointerId(i).toULong(), samples(e, i))
                }
            }
            MotionEvent.ACTION_CANCEL -> {
                palms.clear()
                s.touchCancel()
            }
            else -> emptyList()
        }
        if (e.actionMasked == MotionEvent.ACTION_MOVE) predict(s)
        obey(out)
        scheduleTouchTick(s)
    }

    /** Does what the shared touches asked. */
    private fun obey(out: List<TouchCommand>) {
        for (c in out) when (c) {
            TouchCommand.Undo -> vm.undo()
            TouchCommand.Redo -> vm.redo()
            TouchCommand.ClearLayer -> vm.clearCurrentLayer()
            TouchCommand.ToggleBars -> vm.toggleBars()
            TouchCommand.Fit -> vm.settleToFit()
            TouchCommand.CopyPaste -> vm.copyPasteOpen = true
            is TouchCommand.QuickMenu -> vm.toggleQuickMenu(Offset(c.x, c.y))
            is TouchCommand.LayerSelect -> vm.layerSelectAt(Offset(c.x, c.y))
            TouchCommand.QuickShapeLast -> vm.quickShapeLast()
            is TouchCommand.Pick -> vm.pickAt(c.x, c.y, pickSide)
            TouchCommand.PickDone -> vm.finishPick()
            TouchCommand.PickCancel -> vm.cancelPick()
            TouchCommand.ShapeSnapped -> vm.quickShapeSnapped()
            TouchCommand.ShapeChanged -> vm.quickShapeChanged()
            TouchCommand.ShapeEnded -> vm.quickShapeEnded()
            TouchCommand.StrokeEnded -> vm.strokeEnded()
            TouchCommand.ViewMoved -> vm.refreshView()
            is TouchCommand.Cursor -> {
                vm.showCursor(Offset(c.x, c.y))
                // cloning, the source follows the brush
                if (vm.adjustment == Adjustment.CLONE) vm.followClonePuck()
            }
            TouchCommand.CursorGone -> {
                vm.showCursor(null)
                vm.transformHeld = null
            }
            TouchCommand.TextLayer -> vm.askRasterize = true
            TouchCommand.BrushChanged -> vm.brushTouched()
            // Recolor's reticle dragged or tapped elsewhere (its fills on the view model's fill thread)
            is TouchCommand.RecolorMoved -> vm.recolorMoveTo(Offset(c.x, c.y))
            is TouchCommand.RecolorTapped -> vm.recolorTap(Offset(c.x, c.y))
            // Continue filling's threshold slid (redone on the view model's fill thread), and the fill let go
            is TouchCommand.FillThreshold -> vm.pourThreshold(c.threshold)
            TouchCommand.Filled -> vm.filled()
            // a transform's touch: its popovers close; the box as it now is; the node held, and let go
            // (a rotate or box-turning node springs back); a node tapped types its numbers beside it
            TouchCommand.TransformTouched -> vm.transformPopup = TransformPopup.NONE
            is TouchCommand.TransformMoved -> vm.transformMoved(c.transform)
            is TouchCommand.TransformHeld -> vm.transformHeld = c.handle to Offset(c.x, c.y)
            is TouchCommand.TransformLetGo -> {
                vm.transformHeld = null
                if (c.handle == TransformHandle.Rotate || c.handle == TransformHandle.TurnBox) vm.transformLetGo = c.handle to Offset(c.x, c.y)
            }
            is TouchCommand.TransformNumeric -> vm.transformNumeric = CanvasViewModel.TransformNumeric(c.angle, Offset(c.x, c.y), c.right)
            // the selection tool's touch: a shape dragged out, a colour selection's threshold slid, the
            // freehand path drawn, a selection made
            is TouchCommand.SelectionDrag -> vm.shapeDrag = Offset(c.x0, c.y0) to Offset(c.x1, c.y1)
            is TouchCommand.SelectionThreshold -> vm.colourSlid(Offset(c.x, c.y), c.threshold, c.done)
            TouchCommand.FreehandChanged -> vm.freehandChanged()
            TouchCommand.SelectionChanged -> vm.selectionTouched()
            // an adjustment's amount, liquify, the clone source and the like changed; a tap opens its menu
            TouchCommand.AdjustChanged -> vm.adjustTouched()
            is TouchCommand.AdjustMenu -> if (!closedAdjustMenu) vm.adjustMenuAt = Offset(c.x, c.y)
            // what a pen's own settings ask its double-tap or squeeze to show
            TouchCommand.ShowColours -> vm.toggle(Panel.COLOR)
            TouchCommand.ShowBrushes -> vm.toggle(Panel.BRUSHES)
            // a touch on the canvas with a panel open: dragged, it fades the panel; tapped, it only closes it
            TouchCommand.PanelFades -> {
                vm.swipedBrush = null
                vm.fadePanels()
            }
            TouchCommand.PanelTapped -> vm.tapOutsidePanel()
        }
    }

    // the shared touches' waits (a hold, a repeat), run when they run out
    private val touchTick = Runnable {
        val s = session ?: return@Runnable
        obey(s.touchTick(android.os.SystemClock.uptimeMillis().toDouble()))
        scheduleTouchTick(s)
    }

    private fun scheduleTouchTick(s: CanvasSession) {
        removeCallbacks(touchTick)
        val due = s.touchDeadline() ?: return
        postDelayed(touchTick, (due - android.os.SystemClock.uptimeMillis()).toLong().coerceAtLeast(0L))
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(e: MotionEvent): Boolean {
        val s = session ?: return true
        if (penButton(e)) return true
        predictor.record(e)
        if (e.actionMasked == MotionEvent.ACTION_DOWN) {
            // Continue filling on a layer that takes no fill: the dialog says why, and the touch does nothing
            if (vm.dropBand == CanvasViewModel.DropBand.FILLING && !vm.currentTakesChanges(paint = true)) {
                skipping = true
                return true
            }
            skipping = false
            // a touch on the canvas closes an adjustment's menu, its Layer / Pencil choice and liquify's popups
            closedAdjustMenu = vm.adjustMenuAt != null
            if (vm.adjustment != null) {
                vm.adjustMenuAt = null
                vm.adjustModesOpen = false
                vm.liquifyPopup = 0
            }
            // every sample as it arrives, not batched to the frame
            requestUnbufferedDispatch(e)
            s.setTouchSettings(vm.touchSettings())
            // with a panel open, the touch waits to be a tap or a drag
            s.setTouchPanelOpen(vm.panel != Panel.NONE)
            // Continue filling: each touch fills where it lands; slid, it changes the threshold
            s.setTouchFilling(vm.touchFill())
            // the selection tool's touch selects as the tool is set
            s.setTouchSelecting(vm.selectTouch())
            if (vm.selecting) vm.selectionPopup = SelectionPopup.NONE
            // a transform's touch takes the node it lands on (the warp's inner points with the advanced mesh)
            s.setTouchTransforming(if (vm.transforming) io.github.artcraftandroid.procraft.core.TransformTouch(vm.advancedMesh) else null)
            // the companion's Zoom or Move: a lone press zooms or moves the canvas
            s.setTouchCompanion(
                when (vm.companionMode) {
                    CanvasViewModel.CompanionMode.ZOOM -> CompanionMode.ZOOM
                    CanvasViewModel.CompanionMode.MOVE -> CompanionMode.MOVE
                    CanvasViewModel.CompanionMode.NONE -> null
                },
            )
        }
        if (skipping) return true
        // Liquify's brush shows as a ring of its reach where the touch presses, until it lifts
        if (vm.adjustment == Adjustment.LIQUIFY) {
            val lifted = e.actionMasked == MotionEvent.ACTION_UP || e.actionMasked == MotionEvent.ACTION_CANCEL
            vm.showLiquifyRing(if (lifted || e.pointerCount > 1) null else Offset(e.x, e.y))
        }
        route(e, s)
        // Layer opacity: the shared touches drag it; the band shows it as they go
        if (vm.opacityLayer != null) vm.layerOpacityTouched(e.actionMasked == MotionEvent.ACTION_UP || e.actionMasked == MotionEvent.ACTION_CANCEL)
        return true
    }

    /** The touch went down where Continue filling takes no fill: the rest of it does nothing. */
    private var skipping = false

    /** The touch went down with an adjustment's menu open: a tap only closes it. */
    private var closedAdjustMenu = false

    /**
     * The pen's side buttons, hovering or touching: what the shared touches say Gesture controls set
     * them to do (the first, set to nothing, switches to the eraser and back).
     */
    private fun penButton(e: MotionEvent): Boolean {
        val tool = e.getToolType(0)
        if (tool == MotionEvent.TOOL_TYPE_STYLUS || tool == MotionEvent.TOOL_TYPE_ERASER) penAt = Offset(e.x, e.y)
        // by what the event says is held: some pens' drivers send no press of their own, only their
        // hover and touch events with the button in them
        val held = e.buttonState and PEN_BUTTONS
        val pressed = held and penButtons.inv()
        penButtons = held
        val own = e.actionMasked == MotionEvent.ACTION_BUTTON_PRESS || e.actionMasked == MotionEvent.ACTION_BUTTON_RELEASE
        if (pressed == 0) return own && (e.actionButton and PEN_BUTTONS) != 0
        // under the brush studio the tool would change unseen
        if (vm.studioOpen) return own
        val s = session ?: return own
        penPressed(s, pressed and MotionEvent.BUTTON_STYLUS_PRIMARY == 0, e.x, e.y)
        return own
    }

    /** A side button went down with the pen at ([x], [y]). */
    private fun penPressed(s: CanvasSession, second: Boolean, x: Float, y: Float) {
        // as they are now: nothing has told the session if no touch has come since they were set
        s.setTouchSettings(vm.touchSettings())
        obey(s.touchPenButton(second, x, y))
        vm.penButtonDone()
    }

    /** A side button that comes as a key: at the pen, or the middle of the canvas before it has shown. */
    private val penKeyHeard: (Boolean) -> Unit = { second ->
        val at = penAt ?: Offset(width / 2f, height / 2f)
        session?.let { penPressed(it, second, at.x, at.y) }
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        vm.penKeyHeard = penKeyHeard
    }

    override fun onDetachedFromWindow() {
        if (vm.penKeyHeard === penKeyHeard) vm.penKeyHeard = null
        super.onDetachedFromWindow()
    }

    override fun onGenericMotionEvent(e: MotionEvent): Boolean = penButton(e) || super.onGenericMotionEvent(e)

    /** The pen hovering over the canvas shows the brush cursor where it would paint. */
    override fun onHoverEvent(e: MotionEvent): Boolean {
        if (penButton(e)) return true
        if (e.getToolType(0) != MotionEvent.TOOL_TYPE_STYLUS) return super.onHoverEvent(e)
        when (e.actionMasked) {
            MotionEvent.ACTION_HOVER_ENTER, MotionEvent.ACTION_HOVER_MOVE -> {
                // (told each time: a session opened meanwhile knows it too); a hand already down is set aside
                session?.setTouchPenHovering(true)?.let(::obey)
                // a spray can's sound (a plugin's) rattles shaken above the canvas
                session?.soundHover(e.x, e.y)
                penHovering = true
                if (painting()) vm.showCursor(Offset(e.x, e.y), hovering = true)
                // and Liquify's ring where the pen hovers
                if (vm.adjustment == Adjustment.LIQUIFY) vm.showLiquifyRing(Offset(e.x, e.y))
            }
            MotionEvent.ACTION_HOVER_EXIT -> {
                session?.setTouchPenHovering(false)?.let(::obey)
                penHovering = false
                vm.showCursor(null)
                vm.showLiquifyRing(null)
            }
        }
        return true
    }

    /** Whether a pen on the canvas now would paint (no tool of another kind open). */
    private fun painting() = vm.adjustment == null && !vm.selecting && !vm.transforming

    /** Input stopped coming, so the finger or pen has stopped: nothing runs ahead of it any more. */
    private val predictionOver = Runnable { session?.strokePredict(emptyList()) }

    /** Draws the predicted next samples of the stroke ahead of it (replaced at the next input). */
    private fun predict(s: CanvasSession) {
        removeCallbacks(predictionOver)
        val p = predictor.predict() ?: return
        try {
            val i = p.findPointerIndex(strokePointer)
            if (i >= 0) s.strokePredict(samples(p, i))
        } finally {
            p.recycle()
        }
        postDelayed(predictionOver, PREDICTION_LASTS_MS)
    }

    private fun kindOf(e: MotionEvent, index: Int): PointerKind = when (e.getToolType(index)) {
        MotionEvent.TOOL_TYPE_STYLUS -> PointerKind.STYLUS
        MotionEvent.TOOL_TYPE_ERASER -> PointerKind.STYLUS_ERASER
        MotionEvent.TOOL_TYPE_MOUSE -> PointerKind.MOUSE
        else -> PointerKind.FINGER
    }

    /** The pointer's samples in this event: the batched ones since the last event, then the latest. */
    private fun samples(e: MotionEvent, i: Int): List<PenSample> {
        val out = ArrayList<PenSample>(e.historySize + 1)
        for (h in 0 until e.historySize) {
            out += PenSample(
                e.getHistoricalX(i, h),
                e.getHistoricalY(i, h),
                e.getHistoricalPressure(i, h),
                penLean(e.getHistoricalAxisValue(MotionEvent.AXIS_TILT, i, h), e.getHistoricalOrientation(i, h)),
                e.getHistoricalOrientation(i, h),
                e.getHistoricalEventTime(h).toDouble(),
            )
        }
        out += PenSample(e.getX(i), e.getY(i), e.getPressure(i), penLean(e.getAxisValue(MotionEvent.AXIS_TILT, i), e.getOrientation(i)), e.getOrientation(i), e.eventTime.toDouble())
        return out
    }

    private companion object {
        const val TAG = "ProCraft"

        /** How long a prediction stands with no input after it, milliseconds: a few input intervals. */
        const val PREDICTION_LASTS_MS = 40L

        const val PEN_BUTTONS = MotionEvent.BUTTON_STYLUS_PRIMARY or MotionEvent.BUTTON_STYLUS_SECONDARY

        /** A finger's contact this far across (mm) or more is a palm: a fingertip is 8 to 15, a flat thumb under 20. */
        const val PALM_MM = 25f
    }
}
