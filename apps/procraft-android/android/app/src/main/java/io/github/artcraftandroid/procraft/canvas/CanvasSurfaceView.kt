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
import io.github.artcraftandroid.procraft.core.ChromaticKind
import io.github.artcraftandroid.procraft.core.PointerKind
import io.github.artcraftandroid.procraft.core.ToolKind
import io.github.artcraftandroid.procraft.core.TransformHandle
import io.github.artcraftandroid.procraft.core.TransformKind
import kotlin.math.PI
import kotlin.math.atan2
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.pow

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

    // IDLE: a finger that doesn't paint, waiting for a hold or more fingers; HOVER: fingers
    // setting the brush while the pen hovers
    private enum class Mode { NONE, PEN, FINGER, IDLE, GESTURE, PICK, TRANSFORM, SELECT, ADJUST, PUCK, SHAPE, HOVER, TURN }

    /** The pen is hovering over the canvas. */
    private var penHovering = false

    // Hover: where the finger last was, and the two fingers' last spread
    private var hoverX = 0f
    private var hoverSpread = 0f

    /** The tool this stroke uses instead of the chosen one (Gesture controls' Erase or Smudge), if any. */
    private var toolOverride: ToolKind? = null

    /** The chosen tool back after a stroke that used another. */
    // □ + Touch / □ + Pen: the touch that came while □ was held (its pointer id), and what it does
    private var modifyPointer = -1
    private var modifyPurpose: GesturePurpose? = null

    /**
     * A touch on the canvas while □ is held does what □ + Touch (or □ + Pen) is set to; the touch
     * is followed by its pointer id alone, whether it comes as the first or as a further pointer
     * (the finger on □ may be in the same event). False if the event is not one of these.
     */
    private fun modifyTouch(e: MotionEvent, s: CanvasSession): Boolean {
        if (modifyPointer == -1) {
            val starts = e.actionMasked == MotionEvent.ACTION_DOWN || (e.actionMasked == MotionEvent.ACTION_POINTER_DOWN && mode == Mode.NONE)
            if (!starts || !vm.modifyHeld) return false
            val i = e.actionIndex
            val kind = kindOf(e, i)
            val pen = kind == PointerKind.STYLUS || kind == PointerKind.STYLUS_ERASER
            val purpose = vm.gestures.purposeOf(if (pen) Trigger.MODIFY_PEN else Trigger.MODIFY_TOUCH) ?: return false
            vm.useModify()
            vm.endQuickShape()
            modifyPointer = e.getPointerId(i)
            modifyPurpose = purpose
            val at = Offset(e.getX(i), e.getY(i))
            when (purpose) {
                GesturePurpose.EYEDROPPER -> vm.pickAt(at.x, at.y, pickSide)
                GesturePurpose.QUICKMENU -> vm.toggleQuickMenu(at)
                GesturePurpose.COPY_PASTE -> vm.copyPasteOpen = true
                GesturePurpose.LAYER_SELECT -> vm.layerSelectAt(at)
                GesturePurpose.SMUDGE, GesturePurpose.ERASE, GesturePurpose.ASSISTED -> {
                    requestUnbufferedDispatch(e)
                    toolOverride = when (purpose) {
                        GesturePurpose.ERASE -> ToolKind.ERASE
                        GesturePurpose.SMUDGE -> ToolKind.SMUDGE
                        else -> null
                    }
                    toolOverride?.let { s.setTool(it) }
                    if (purpose == GesturePurpose.ASSISTED) s.setStrokeAssist(true)
                    if (vm.currentIsText()) vm.askRasterize = true
                    s.strokeBegin(kind, samples(e, i))
                }
                else -> Unit
            }
            return true
        }
        val i = e.findPointerIndex(modifyPointer)
        val stroke = modifyPurpose == GesturePurpose.SMUDGE || modifyPurpose == GesturePurpose.ERASE || modifyPurpose == GesturePurpose.ASSISTED
        when (e.actionMasked) {
            MotionEvent.ACTION_MOVE -> if (i >= 0) {
                if (modifyPurpose == GesturePurpose.EYEDROPPER) vm.pickAt(e.getX(i), e.getY(i), pickSide)
                if (stroke) s.strokeAdd(samples(e, i))
            }
            MotionEvent.ACTION_POINTER_UP, MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                val ended = e.actionMasked == MotionEvent.ACTION_CANCEL || e.getPointerId(e.actionIndex) == modifyPointer
                if (ended) {
                    if (modifyPurpose == GesturePurpose.EYEDROPPER) vm.finishPick()
                    if (stroke) {
                        if (e.actionMasked == MotionEvent.ACTION_CANCEL) s.strokeCancel() else s.strokeEnd(emptyList())
                        s.setStrokeAssist(false)
                        restoreTool(s)
                        vm.strokeEnded()
                    }
                    modifyPointer = -1
                    modifyPurpose = null
                }
                // the last finger up: the canvas waits for a new touch
                if (e.actionMasked != MotionEvent.ACTION_POINTER_UP) mode = Mode.NONE
            }
        }
        return true
    }

    private fun restoreTool(s: CanvasSession) {
        if (toolOverride != null) s.setTool(vm.tool)
        toolOverride = null
    }

    private val holdMs get() = (vm.gestures.delay(vm.gestures.purposeOf(Trigger.HOLD) ?: GesturePurpose.EYEDROPPER) * 1000).toLong()
    private val quickShapeMs get() = (vm.gestures.delay(GesturePurpose.QUICKSHAPE) * 1000).toLong()
    private val drawAndHold get() = vm.gestures.isOn(GesturePurpose.QUICKSHAPE, "DRAW_HOLD")
    private val undoTaps get() = !vm.gestures.isOn(GesturePurpose.GENERAL, "NO_UNDO")

    private var mode = Mode.NONE
    private var strokePointer = -1
    /** The liquify brush's last place (window pixels), when, and its velocity (pixels a millisecond). */
    private var liquifyAt = Offset.Zero
    private var liquifyTime = 0L
    private var liquifyVelocity = Offset.Zero

    /** Follows the liquify brush's velocity, smoothed over its moves. */
    private fun followLiquify(e: MotionEvent) {
        val dt = (e.eventTime - liquifyTime).toFloat()
        if (dt <= 0f) return
        val now = Offset(e.x, e.y)
        liquifyVelocity = liquifyVelocity * 0.5f + (now - liquifyAt) / dt * 0.5f
        liquifyAt = now
        liquifyTime = e.eventTime
    }

    /** Where the stroke last moved from (window pixels), for QuickShape's hold; the handle being dragged. */
    private var holdAt = Offset.Zero
    private var shapeHandle = -1
    private val quickShapeHold = Runnable {
        if ((mode == Mode.PEN || mode == Mode.FINGER) && drawAndHold) {
            removeCallbacks(holdToPick)
            vm.quickShapeHold()
        }
    }

    // a freehand selection drawn and held still snaps to a shape, which the lift then selects
    private var lassoHoldAt = Offset.Zero
    private var lassoSnapped = false
    private val lassoHold = Runnable {
        if (mode == Mode.SELECT && vm.selectionKind == SelectionKind.FREEHAND && selectMoved && drawAndHold) {
            lassoSnapped = vm.snapFreehand()
        }
    }

    /** A displacing drag has begun. */
    private var chromaDragging = false
    /** Where on the clone source it was taken (window pixels from its centre). */
    private var puckGrab = Offset.Zero

    // the two fingers a gesture follows (pointer ids) and where they were
    private var gestureA = -1
    private var gestureB = -1
    private var lastX = 0f
    private var lastY = 0f
    private var lastDistance = 0f
    private var lastAngle = 0f

    // whether the touches so far still make a tap
    private var tapStart = 0L
    private var tapPointers = 0
    private var tapMoved = 0f
    private val tapSlop = 24f * resources.displayMetrics.density

    // the gesture as a whole: when it began, how far it has zoomed, fingers down now
    private var gestureStart = 0L
    private var gestureScale = 1f
    private var fingers = 0

    // the three-finger scrub that clears the layer: direction changes of the fingers' centre
    private var scrubX = 0f
    private var scrubDirection = 0
    private var scrubTravel = 0f
    private var scrubTurns = 0
    private val scrubStep = 24f * resources.displayMetrics.density

    // while transforming: the handle the touch took, and whether its drag has begun (past the slop)
    private var transformHandle: TransformHandle? = null
    private var transformGrabbed = false
    private val transformSlop = TRANSFORM_SLOP_DP * resources.displayMetrics.density

    // the selection tool's touch: where it went down, and whether it has moved past the slop
    private var selectStart = Offset.Zero
    private var selectMoved = false
    private var lastSelectX = 0f
    private var lastSelectTime = 0L
    private var threshold = 0f

    // where the finger went down, for the hold that opens the eyedropper
    private var downX = 0f
    private var downY = 0f
    private val pickSide get() = (PICK_WINDOW_DP * resources.displayMetrics.density).toInt() or 1

    /** A finger held still on the canvas does what Touch and hold is set to: picks a colour, or clears the layer. */
    private val holdToPick = Runnable {
        val s = session ?: return@Runnable
        if (mode != Mode.FINGER && mode != Mode.IDLE) return@Runnable
        when (vm.gestures.purposeOf(Trigger.HOLD)) {
            GesturePurpose.EYEDROPPER -> {
                if (mode == Mode.FINGER) s.strokeCancel()
                restoreTool(s)
                mode = Mode.PICK
                vm.pickAt(downX, downY, pickSide)
            }
            GesturePurpose.CLEAR_LAYER -> {
                if (mode == Mode.FINGER) s.strokeCancel()
                restoreTool(s)
                mode = Mode.IDLE
                vm.clearCurrentLayer()
            }
            GesturePurpose.QUICKMENU -> {
                if (mode == Mode.FINGER) s.strokeCancel()
                restoreTool(s)
                mode = Mode.IDLE
                vm.toggleQuickMenu(Offset(downX, downY))
            }
            GesturePurpose.LAYER_SELECT -> {
                if (mode == Mode.FINGER) s.strokeCancel()
                restoreTool(s)
                mode = Mode.IDLE
                vm.layerSelectAt(Offset(downX, downY))
            }
            GesturePurpose.COPY_PASTE -> {
                if (mode == Mode.FINGER) s.strokeCancel()
                restoreTool(s)
                mode = Mode.IDLE
                vm.copyPasteOpen = true
            }
            else -> Unit
        }
    }

    /** Holding two fingers still undoes, then keeps undoing; three fingers redo. */
    private val rapidUndo = object : Runnable {
        override fun run() {
            if (mode != Mode.GESTURE || tapMoved > tapSlop || fingers != tapPointers || !undoTaps) return
            when (fingers) {
                2 -> vm.undo()
                3 -> vm.redo()
                else -> return
            }
            postDelayed(this, RAPID_REPEAT_MS)
        }
    }

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
        vm.screenCentre = Offset(width / 2f, height / 2f)
        windowWidth = width
        windowHeight = height
        vm.windowSize = Offset(width.toFloat(), height.toFloat())
        vm.density = resources.displayMetrics.density
        windowRotation = rotation
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

    /** Where the last companion drag sample was, while a one-finger zoom or move goes on. */
    private var companionAt: Offset? = null

    /**
     * With the companion's Zoom or Move on, a single finger dragged on the canvas zooms it (up
     * zooms in, about where it started) or moves it, instead of painting.
     */
    private fun companionTouch(e: MotionEvent, s: CanvasSession): Boolean {
        val mode = vm.companionMode
        if (mode == CanvasViewModel.CompanionMode.NONE && companionAt == null) return false
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                if (mode == CanvasViewModel.CompanionMode.NONE) return false
                companionAt = Offset(e.x, e.y)
                downX = e.x
                downY = e.y
            }
            MotionEvent.ACTION_MOVE -> {
                val last = companionAt ?: return false
                val (dx, dy) = (e.x - last.x) to (e.y - last.y)
                if (mode == CanvasViewModel.CompanionMode.ZOOM) {
                    val scale = kotlin.math.exp(-dy / (COMPANION_ZOOM_DP * resources.displayMetrics.density))
                    s.viewGesture(downX, downY, 0f, 0f, scale, 0f)
                } else {
                    s.viewGesture(e.x, e.y, dx, dy, 1f, 0f)
                }
                companionAt = Offset(e.x, e.y)
                vm.refreshView()
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                companionAt ?: return false
                companionAt = null
                vm.refreshView()
            }
            else -> return companionAt != null
        }
        return true
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(e: MotionEvent): Boolean {
        val s = session ?: return true
        if (penButton(e)) return true
        if (modifyTouch(e, s)) return true
        if (companionTouch(e, s)) return true
        predictor.record(e)
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                // every sample as it arrives, not batched to the frame
                requestUnbufferedDispatch(e)
                fingers = 1
                downX = e.x
                downY = e.y
                if (vm.pickNext) {
                    // the modify button was pressed: this touch picks a colour
                    mode = Mode.PICK
                    vm.pickAt(e.x, e.y, pickSide)
                    return true
                }
                tapStart = e.eventTime
                tapPointers = 1
                tapMoved = 0f
                val a = vm.adjustment
                if (a != null) {
                    vm.adjustMenuAt = null
                    vm.adjustModesOpen = false
                    val kind = kindOf(e, 0)
                    val pen = kind == PointerKind.STYLUS || kind == PointerKind.STYLUS_ERASER
                    if (a == Adjustment.CLONE && vm.onClonePuck(Offset(e.x, e.y))) {
                        // the clone source is dragged
                        mode = Mode.PUCK
                        val puck = vm.clonePuck?.let(vm::canvasToWindow) ?: Offset(e.x, e.y)
                        puckGrab = Offset(e.x, e.y) - puck
                        return true
                    }
                    if (a == Adjustment.CLONE || (vm.adjustPencil && pen)) {
                        // cloning, and the pen painting an adjustment on, paint (no eyedropper)
                        strokePointer = e.getPointerId(0)
                        mode = if (pen) Mode.PEN else Mode.FINGER
                        if (a == Adjustment.CLONE) vm.cloneStrokeAt(Offset(e.x, e.y), start = true)
                        s.strokeBegin(kind, samples(e, 0))
                        return true
                    }
                    // otherwise one pen or finger drags the amount across the canvas (a tap opens the menu); nothing paints
                    mode = Mode.ADJUST
                    chromaDragging = false
                    if (vm.adjustment == Adjustment.LIQUIFY) {
                        vm.liquifyPopup = 0
                        vm.liquifyStroke(liquifyPoints(e), start = true)
                        liquifyVelocity = Offset.Zero
                        liquifyAt = Offset(e.x, e.y)
                        liquifyTime = e.eventTime
                    }
                    lastSelectX = e.x
                    lastSelectTime = e.eventTime
                    return true
                }
                if (vm.selecting) {
                    // one pen or finger makes the selection; nothing paints
                    mode = Mode.SELECT
                    vm.selectionPopup = SelectionPopup.NONE
                    selectStart = Offset(e.x, e.y)
                    selectMoved = false
                    lassoSnapped = false
                    lassoHoldAt = selectStart
                    lastSelectX = e.x
                    lastSelectTime = e.eventTime
                    threshold = vm.selectionThreshold
                    return true
                }
                if (vm.transforming) {
                    // one pen or finger works the box; nothing paints
                    mode = Mode.TRANSFORM
                    vm.transformPopup = TransformPopup.NONE
                    val density = resources.displayMetrics.density
                    val box = vm.transformBox
                    transformHandle = when {
                        box == null -> null
                        vm.transformKind == TransformKind.WARP && box.mesh.isNotEmpty() ->
                            meshHandleAt(box, vm.viewMatrix, Offset(e.x, e.y), density, vm.advancedMesh)
                        else -> boxOnScreen(box, vm.viewMatrix, density)?.let { handleAt(it, Offset(e.x, e.y), density) }
                    }
                    transformGrabbed = false
                    return true
                }
                // on a model, a finger turns it; the pen paints
                if (vm.modelOpen && !vm.modelFlat && kindOf(e, 0) == PointerKind.FINGER) {
                    mode = Mode.TURN
                    lastX = e.x
                    lastY = e.y
                    return true
                }
                // a snapped shape's handle is dragged; anything else keeps the shape and draws on
                val handle = vm.quickShapeHandleAt(Offset(e.x, e.y))
                if (handle != null) {
                    mode = Mode.SHAPE
                    shapeHandle = handle
                    return true
                }
                vm.endQuickShape()
                val kind = kindOf(e, 0)
                val pen = kind == PointerKind.STYLUS || kind == PointerKind.STYLUS_ERASER
                // Hover: with the pen held over the canvas, a finger drags the opacity and a pinch the size
                if (!pen && penHovering && vm.gestures.isOn(GesturePurpose.HOVER, "HOVER_TOUCH")) {
                    mode = Mode.HOVER
                    hoverX = e.x
                    return true
                }
                val gestures = vm.gestures
                // what this kind of touch is set to do in Gesture controls
                val touchDoes = gestures.purposeOf(if (pen) Trigger.PEN else Trigger.TOUCH)
                if (!pen && touchDoes == GesturePurpose.EYEDROPPER) {
                    mode = Mode.PICK
                    vm.pickAt(e.x, e.y, pickSide)
                    return true
                }
                // a finger set to open a menu or show the interface does that instead of painting
                when (touchDoes.takeIf { !pen }) {
                    GesturePurpose.QUICKMENU -> {
                        mode = Mode.IDLE
                        vm.toggleQuickMenu(Offset(e.x, e.y))
                        return true
                    }
                    GesturePurpose.COPY_PASTE -> {
                        mode = Mode.IDLE
                        vm.copyPasteOpen = true
                        return true
                    }
                    GesturePurpose.FULL_SCREEN -> {
                        mode = Mode.IDLE
                        vm.toggleBars()
                        return true
                    }
                    GesturePurpose.LAYER_SELECT -> {
                        mode = Mode.IDLE
                        vm.layerSelectAt(Offset(e.x, e.y))
                        return true
                    }
                    GesturePurpose.QUICKSHAPE -> {
                        mode = Mode.IDLE
                        vm.quickShapeLast()
                        return true
                    }
                    else -> Unit
                }
                if (!pen && touchDoes == null && !gestures.isOn(GesturePurpose.GENERAL, "FINGER_PAINT")) {
                    mode = Mode.IDLE
                    if (gestures.purposeOf(Trigger.HOLD) != null) postDelayed(holdToPick, holdMs)
                    return true
                }
                strokePointer = e.getPointerId(0)
                mode = if (pen) Mode.PEN else Mode.FINGER
                toolOverride = when (touchDoes) {
                    GesturePurpose.ERASE -> ToolKind.ERASE
                    GesturePurpose.SMUDGE -> ToolKind.SMUDGE
                    else -> null
                }
                toolOverride?.let { s.setTool(it) }
                // a text layer takes no paint until it is rasterized
                if (vm.currentIsText()) vm.askRasterize = true
                s.strokeBegin(kind, samples(e, 0))
                vm.showCursor(Offset(e.x, e.y))
                if (mode == Mode.FINGER && gestures.purposeOf(Trigger.HOLD) != null) postDelayed(holdToPick, holdMs)
                holdAt = Offset(e.x, e.y)
                if (drawAndHold) postDelayed(quickShapeHold, quickShapeMs)
            }
            MotionEvent.ACTION_POINTER_DOWN -> {
                fingers = e.pointerCount
                if (mode == Mode.HOVER) {
                    if (e.pointerCount >= 2) hoverSpread = hypot(e.getX(1) - e.getX(0), e.getY(1) - e.getY(0))
                    return true
                }
                removeCallbacks(holdToPick)
                removeCallbacks(quickShapeHold)
                if (mode == Mode.PEN || mode == Mode.PICK) return true
                vm.showCursor(null)
                if (mode == Mode.FINGER) s.strokeCancel()
                restoreTool(s)
                if (mode == Mode.TRANSFORM && transformGrabbed) vm.transformMoved(s.transformCancelDrag())
                vm.transformHeld = null
                if (mode == Mode.SELECT) {
                    vm.shapeDrag = null
                    vm.thresholdDrag?.let { (p, t) -> vm.selectByColour(p, t, live = false) }
                }
                if (mode == Mode.FINGER || mode == Mode.IDLE || mode == Mode.TRANSFORM || mode == Mode.SELECT || mode == Mode.TURN) {
                    gestureStart = e.eventTime
                    gestureScale = 1f
                }
                mode = Mode.GESTURE
                tapPointers = max(tapPointers, e.pointerCount)
                followFingers(e, exclude = -1)
                if (fingers == 3) {
                    startScrub(e)
                    if (!vm.gestures.isOn(GesturePurpose.CLEAR_LAYER, Trigger.SCRUB)) scrubTurns = -1
                }
                removeCallbacks(rapidUndo)
                postDelayed(rapidUndo, (vm.rapidUndoDelay * 1000).toLong())
            }
            MotionEvent.ACTION_MOVE -> when (mode) {
                Mode.PEN, Mode.FINGER -> {
                    val i = e.findPointerIndex(strokePointer)
                    if (i >= 0) {
                        if (mode == Mode.FINGER && hypot(e.getX(i) - downX, e.getY(i) - downY) > tapSlop) removeCallbacks(holdToPick)
                        if (vm.adjustment == Adjustment.CLONE) vm.cloneStrokeAt(Offset(e.getX(i), e.getY(i)), start = false)
                        // moving on restarts the wait for a hold
                        if (hypot(e.getX(i) - holdAt.x, e.getY(i) - holdAt.y) > CanvasViewModel.QUICKSHAPE_STILL_DP * resources.displayMetrics.density) {
                            holdAt = Offset(e.getX(i), e.getY(i))
                            removeCallbacks(quickShapeHold)
                            if (vm.quickShape == null && drawAndHold) postDelayed(quickShapeHold, quickShapeMs)
                        }
                        s.strokeAdd(samples(e, i))
                        vm.showCursor(Offset(e.getX(i), e.getY(i)))
                        predict(s)
                    }
                }
                Mode.PICK -> vm.pickAt(e.x, e.y, pickSide)
                Mode.IDLE -> if (hypot(e.x - downX, e.y - downY) > tapSlop) removeCallbacks(holdToPick)
                Mode.HOVER -> hoverAdjust(e)
                Mode.TURN -> {
                    s.turnModel(e.x - lastX, e.y - lastY)
                    tapMoved += hypot(e.x - lastX, e.y - lastY)
                    lastX = e.x
                    lastY = e.y
                }
                Mode.PUCK -> vm.moveClonePuck(Offset(e.x, e.y) - puckGrab)
                Mode.SHAPE -> vm.moveQuickShapeHandle(shapeHandle, Offset(e.x, e.y))
                Mode.GESTURE -> if (fingers >= 3) scrub(e) else moveGesture(e, s)
                Mode.SELECT -> select(e, done = false)
                Mode.ADJUST -> if (vm.adjustment == Adjustment.LIQUIFY) {
                    vm.liquifyStroke(liquifyPoints(e), start = false)
                    followLiquify(e)
                } else if (vm.adjustment == Adjustment.MOTION_BLUR) {
                    // a motion blur follows the drag: along it, longer the further it goes
                    vm.dragMotionBlur(e.x - downX, e.y - downY)
                } else if (vm.adjustment == Adjustment.CHROMATIC_ABERRATION && vm.chromaKind == ChromaticKind.DISPLACE) {
                    // displacing, the drag moves the fringes
                    vm.dragChromatic(Offset(e.x - downX, e.y - downY), start = !chromaDragging)
                    chromaDragging = true
                } else if (vm.adjustment?.swiped == true) {
                    // across the canvas the amount follows the drag, faster the faster it goes
                    val dx = (e.x - lastSelectX) / resources.displayMetrics.density
                    vm.changeAdjustAmount(dragStep(dx, e.eventTime - lastSelectTime))
                    lastSelectX = e.x
                    lastSelectTime = e.eventTime
                }
                Mode.TRANSFORM -> transformHandle?.let { handle ->
                    if (!transformGrabbed && hypot(e.x - downX, e.y - downY) > transformSlop) {
                        // the drag counts from where it was recognised, as the slop is used up
                        s.transformGrab(handle, e.x, e.y)
                        transformGrabbed = true
                    }
                    if (transformGrabbed) {
                        vm.transformMoved(s.transformDrag(e.x, e.y))
                        vm.transformHeld = handle to Offset(e.x, e.y)
                    }
                }
                Mode.NONE -> Unit
            }
            MotionEvent.ACTION_POINTER_UP -> {
                fingers = e.pointerCount - 1
                if (mode == Mode.GESTURE) followFingers(e, exclude = e.actionIndex)
            }
            MotionEvent.ACTION_UP -> {
                vm.showCursor(null)
                when (mode) {
                    Mode.PEN, Mode.FINGER -> {
                        val i = e.findPointerIndex(strokePointer)
                        s.strokeEnd(if (i >= 0) samples(e, i) else emptyList())
                        restoreTool(s)
                        vm.strokeEnded()
                    }
                    Mode.GESTURE -> if (e.eventTime - tapStart <= TAP_MS && tapMoved <= tapSlop) {
                        when (tapPointers) {
                            2 -> if (undoTaps) vm.undo()
                            3 -> if (undoTaps) vm.redo()
                            4 -> when (vm.gestures.purposeOf(Trigger.FOUR_TAP)) {
                                GesturePurpose.FULL_SCREEN -> vm.toggleBars()
                                GesturePurpose.CLEAR_LAYER -> vm.clearCurrentLayer()
                                // in the middle of the screen, not under the fingers
                                GesturePurpose.QUICKMENU -> vm.toggleQuickMenu(vm.screenCentre)
                                GesturePurpose.COPY_PASTE -> vm.copyPasteOpen = true
                                GesturePurpose.QUICKSHAPE -> vm.quickShapeLast()
                                else -> Unit
                            }
                        }
                    } else if (tapPointers == 2 && e.eventTime - gestureStart <= QUICK_PINCH_MS && gestureScale < QUICK_PINCH_SCALE) {
                        // a quick pinch in fits the canvas to the screen, settling into place
                        vm.settleToFit()
                    }
                    Mode.PICK -> vm.finishPick()
                    Mode.PUCK, Mode.SHAPE, Mode.IDLE, Mode.HOVER, Mode.TURN -> Unit
                    Mode.SELECT -> select(e, done = true)
                    Mode.ADJUST -> if (vm.adjustment == Adjustment.LIQUIFY) {
                        // still moving when it lifts, liquify carries on
                        if (e.eventTime - liquifyTime <= LIQUIFY_STILL_MS) vm.liquifyLifted(liquifyVelocity.x, liquifyVelocity.y)
                    } else if (e.eventTime - tapStart <= TAP_MS && hypot(e.x - downX, e.y - downY) <= tapSlop) {
                        vm.adjustMenuAt = Offset(e.x, e.y)
                    }
                    Mode.TRANSFORM -> if (transformGrabbed) {
                        vm.transformMoved(s.transformRelease())
                        val handle = transformHandle
                        if (handle == TransformHandle.Rotate || handle == TransformHandle.TurnBox) vm.transformLetGo = handle to Offset(e.x, e.y)
                        vm.transformHeld = null
                    } else if (transformHandle == TransformHandle.Move && e.eventTime - tapStart <= TAP_MS) {
                        // a tap off the nodes nudges the content a pixel towards it
                        vm.transformMoved(s.transformNudge(e.x, e.y))
                    } else if (e.eventTime - tapStart <= TAP_MS) {
                        // a tap on a node types its numbers: the rotation handle its angle, the others the size
                        when (transformHandle) {
                            TransformHandle.Rotate -> vm.transformNumeric = CanvasViewModel.TransformNumeric(true, Offset(downX, downY))
                            is TransformHandle.Corner, is TransformHandle.Edge -> vm.transformNumeric = CanvasViewModel.TransformNumeric(false, Offset(downX, downY))
                            else -> Unit
                        }
                    }
                    Mode.NONE -> Unit
                }
                mode = Mode.NONE
                transformGrabbed = false
                fingers = 0
                removeCallbacks(rapidUndo)
                removeCallbacks(holdToPick)
                removeCallbacks(quickShapeHold)
            }
            MotionEvent.ACTION_CANCEL -> {
                vm.showCursor(null)
                if (mode == Mode.PEN || mode == Mode.FINGER) s.strokeCancel()
                restoreTool(s)
                if (mode == Mode.PICK) vm.cancelPick()
                if (mode == Mode.TRANSFORM && transformGrabbed) vm.transformMoved(s.transformCancelDrag())
                vm.transformHeld = null
                transformGrabbed = false
                mode = Mode.NONE
                fingers = 0
                removeCallbacks(rapidUndo)
                removeCallbacks(holdToPick)
            }
        }
        return true
    }

    /** The pen's side buttons, hovering or touching: what Pen double-tap or squeeze is set to do. */
    private fun penButton(e: MotionEvent): Boolean {
        if (e.actionMasked != MotionEvent.ACTION_BUTTON_PRESS) return false
        when (e.actionButton) {
            MotionEvent.BUTTON_STYLUS_PRIMARY -> vm.penButton(secondary = false, Offset(e.x, e.y))
            MotionEvent.BUTTON_STYLUS_SECONDARY -> vm.penButton(secondary = true, Offset(e.x, e.y))
            else -> return false
        }
        return true
    }

    override fun onGenericMotionEvent(e: MotionEvent): Boolean = penButton(e) || super.onGenericMotionEvent(e)

    /** The pen hovering over the canvas shows the brush cursor where it would paint. */
    override fun onHoverEvent(e: MotionEvent): Boolean {
        if (penButton(e)) return true
        if (e.getToolType(0) != MotionEvent.TOOL_TYPE_STYLUS || mode != Mode.NONE) return super.onHoverEvent(e)
        when (e.actionMasked) {
            MotionEvent.ACTION_HOVER_ENTER, MotionEvent.ACTION_HOVER_MOVE -> {
                penHovering = true
                if (painting()) vm.showCursor(Offset(e.x, e.y), hovering = true)
            }
            MotionEvent.ACTION_HOVER_EXIT -> {
                penHovering = false
                vm.showCursor(null)
            }
        }
        return true
    }

    /** Hover: one finger across the screen moves the opacity; two pinching change the size. */
    private fun hoverAdjust(e: MotionEvent) {
        if (e.pointerCount >= 2) {
            val spread = hypot(e.getX(1) - e.getX(0), e.getY(1) - e.getY(0))
            if (hoverSpread > 1f && spread > 1f) vm.changeSize((vm.size * spread / hoverSpread).coerceIn(0f, 1f))
            hoverSpread = spread
        } else {
            val dx = (e.x - hoverX) / width.coerceAtLeast(1)
            vm.changeOpacity((vm.opacity + dx).coerceIn(0f, 1f))
            hoverX = e.x
        }
    }

    /** Whether a pen on the canvas now would paint (no tool of another kind open). */
    private fun painting() = vm.adjustment == null && !vm.selecting && !vm.transforming

    /**
     * A step of the selection tool's touch: a rectangle or ellipse dragged out; a freehand path drawn
     * (or tapped point by point, closed by a tap on its first point); a colour picked by a tap, its
     * threshold dragged left and right.
     */
    private fun select(e: MotionEvent, done: Boolean) {
        val start = selectStart
        val p = Offset(e.x, e.y)
        val density = resources.displayMetrics.density
        if ((p - start).getDistance() > transformSlop) selectMoved = true
        when (vm.selectionKind) {
            SelectionKind.RECTANGLE, SelectionKind.ELLIPSE -> {
                if (done) vm.selectShape(start, p) else if (selectMoved) vm.shapeDrag = start to p
            }
            SelectionKind.FREEHAND -> {
                // snapped: the shape is kept as it is, and the lift selects it
                if (lassoSnapped) {
                    if (done) {
                        lassoSnapped = false
                        vm.closeFreehand()
                    }
                    return
                }
                if (done) {
                    removeCallbacks(lassoHold)
                } else if (selectMoved && (p - lassoHoldAt).getDistance() > CanvasViewModel.QUICKSHAPE_STILL_DP * density) {
                    // still for the QuickShape delay, it snaps
                    lassoHoldAt = p
                    removeCallbacks(lassoHold)
                    postDelayed(lassoHold, quickShapeMs)
                }
                val last = vm.freehand.lastOrNull()?.let { vm.canvasToWindow(it) }
                if (done && !selectMoved) {
                    vm.freehandPoint(p, FREEHAND_CLOSE_DP * density)
                } else if (selectMoved && (last == null || (p - last).getDistance() >= FREEHAND_STEP_DP * density)) {
                    if (last == null) vm.freehandPoint(start, 0f)
                    vm.freehandPoint(p, 0f)
                }
            }
            SelectionKind.AUTOMATIC -> {
                // across the screen the threshold follows the drag, faster the faster it goes
                val dx = (p.x - lastSelectX) / density
                threshold = (threshold + dragStep(dx, e.eventTime - lastSelectTime)).coerceIn(0f, 1f)
                lastSelectX = p.x
                lastSelectTime = e.eventTime
                if (done) vm.selectByColour(start, threshold, live = false)
                else if (selectMoved) vm.selectByColour(start, threshold, live = true)
            }
        }
    }

    /**
     * How much an amount dragged across the screen changes for a step of [dx] points over [ms]: more
     * for faster drags (the speed taken per 60 Hz frame), 200 points at a steady pace moving it most
     * of the way.
     */
    private fun dragStep(dx: Float, ms: Long): Float {
        val frames = (ms.coerceAtLeast(1L) / 1000f * 60f).coerceAtLeast(0.06f)
        val speed = kotlin.math.abs(dx) / frames
        return (speed / 30f).toDouble().pow(0.86).toFloat() * dx / 200f
    }

    /** The first pointer's samples in this event as (x, y, pressure), for liquify. */
    private fun liquifyPoints(e: MotionEvent): List<Float> {
        val out = ArrayList<Float>((e.historySize + 1) * 3)
        for (h in 0 until e.historySize) {
            out += e.getHistoricalX(0, h)
            out += e.getHistoricalY(0, h)
            out += if (e.getToolType(0) == MotionEvent.TOOL_TYPE_STYLUS) e.getHistoricalPressure(0, h) else 1f
        }
        out += e.getX(0)
        out += e.getY(0)
        out += if (e.getToolType(0) == MotionEvent.TOOL_TYPE_STYLUS) e.getPressure(0) else 1f
        return out
    }

    /** Draws the predicted next samples of the stroke ahead of it (replaced at the next input). */
    private fun predict(s: CanvasSession) {
        val p = predictor.predict() ?: return
        try {
            val i = p.findPointerIndex(strokePointer)
            if (i >= 0) s.strokePredict(samples(p, i))
        } finally {
            p.recycle()
        }
    }

    /** Starts following the first two fingers other than the one at [exclude]. */
    private fun startScrub(e: MotionEvent) {
        swipeStartY = centreY(e)
        swipeStartX = centreX(e)
        swiped = false
        scrubX = centreX(e)
        scrubDirection = 0
        scrubTravel = 0f
        scrubTurns = 0
    }

    /** Three fingers rubbed back and forth (three turns) clear the current layer, once per gesture. */
    private fun scrub(e: MotionEvent) {
        threeFingerSwipe(e)
        val x = centreX(e)
        val dx = x - scrubX
        scrubX = x
        tapMoved += kotlin.math.abs(dx)
        if (dx == 0f || scrubTurns < 0) return
        val direction = if (dx > 0f) 1 else -1
        if (direction != scrubDirection) {
            if (scrubDirection != 0 && scrubTravel >= scrubStep) scrubTurns++
            scrubDirection = direction
            scrubTravel = 0f
        }
        scrubTravel += kotlin.math.abs(dx)
        if (scrubTurns >= SCRUB_TURNS) {
            scrubTurns = -1
            vm.clearCurrentLayer()
        }
    }

    // three fingers swiped down: where they started, and whether it has done its thing
    private var swipeStartX = 0f
    private var swipeStartY = 0f
    private var swiped = false

    /** Three fingers swiped down (mostly straight) do what Three-finger swipe is set to, once. */
    private fun threeFingerSwipe(e: MotionEvent) {
        if (swiped || e.pointerCount != 3) return
        val dy = centreY(e) - swipeStartY
        val dx = centreX(e) - swipeStartX
        if (dy < SWIPE_DOWN_DP * resources.displayMetrics.density || kotlin.math.abs(dx) > dy / 2f) return
        swiped = true
        scrubTurns = -1
        when (vm.gestures.purposeOf(Trigger.THREE_SWIPE)) {
            GesturePurpose.COPY_PASTE -> vm.copyPasteOpen = true
            GesturePurpose.FULL_SCREEN -> vm.toggleBars()
            GesturePurpose.CLEAR_LAYER -> vm.clearCurrentLayer()
            GesturePurpose.QUICKMENU -> vm.toggleQuickMenu(Offset(centreX(e), centreY(e)))
            GesturePurpose.QUICKSHAPE -> vm.quickShapeLast()
            else -> Unit
        }
    }

    private fun centreY(e: MotionEvent): Float = (0 until e.pointerCount).sumOf { e.getY(it).toDouble() }.toFloat() / e.pointerCount

    private fun centreX(e: MotionEvent): Float = (0 until e.pointerCount).sumOf { e.getX(it).toDouble() }.toFloat() / e.pointerCount

    private fun followFingers(e: MotionEvent, exclude: Int) {
        val picked = (0 until e.pointerCount).filter { it != exclude }.take(2)
        if (picked.size < 2) {
            gestureA = -1
            gestureB = -1
            return
        }
        gestureA = e.getPointerId(picked[0])
        gestureB = e.getPointerId(picked[1])
        val (x, y, d, a) = pair(e, picked[0], picked[1])
        lastX = x
        lastY = y
        lastDistance = d
        lastAngle = a
    }

    private fun moveGesture(e: MotionEvent, s: CanvasSession) {
        val ia = e.findPointerIndex(gestureA)
        val ib = e.findPointerIndex(gestureB)
        if (ia < 0 || ib < 0) return
        val (x, y, d, a) = pair(e, ia, ib)
        val scale = if (lastDistance > 1f && d > 1f) d / lastDistance else 1f
        var turn = a - lastAngle
        if (turn > PI) turn -= (2 * PI).toFloat()
        if (turn < -PI) turn += (2 * PI).toFloat()
        if (!vm.gestures.isOn(GesturePurpose.GENERAL, "ROTATE")) turn = 0f
        tapMoved += hypot(x - lastX, y - lastY) + kotlin.math.abs(d - lastDistance)
        gestureScale *= scale
        s.viewGesture(lastX, lastY, x - lastX, y - lastY, scale, turn)
        vm.refreshView()
        lastX = x
        lastY = y
        lastDistance = d
        lastAngle = a
    }

    /** Centre, distance and angle of two fingers. */
    private fun pair(e: MotionEvent, a: Int, b: Int): FloatArray {
        val (ax, ay, bx, by) = floatArrayOf(e.getX(a), e.getY(a), e.getX(b), e.getY(b))
        return floatArrayOf((ax + bx) / 2f, (ay + by) / 2f, hypot(bx - ax, by - ay), atan2(by - ay, bx - ax))
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
                e.getHistoricalAxisValue(MotionEvent.AXIS_TILT, i, h),
                e.getHistoricalOrientation(i, h),
                e.getHistoricalEventTime(h).toDouble(),
            )
        }
        out += PenSample(e.getX(i), e.getY(i), e.getPressure(i), e.getAxisValue(MotionEvent.AXIS_TILT, i), e.getOrientation(i), e.eventTime.toDouble())
        return out
    }

    private companion object {
        /** Lifted after holding still this long (milliseconds), liquify does not carry on. */
        private const val LIQUIFY_STILL_MS = 60L
        const val TAG = "ProCraft"
        const val TAP_MS = 300L
        const val RAPID_UNDO_DELAY_MS = 650L
        const val RAPID_REPEAT_MS = 120L
        const val QUICK_PINCH_MS = 400L
        /** How far (dp) a companion zoom drag goes for the canvas to grow e times. */
        const val COMPANION_ZOOM_DP = 180f
        const val QUICK_PINCH_SCALE = 0.75f
        const val SCRUB_TURNS = 3
        /** How far three fingers go down to count as a swipe, dp. */
        const val SWIPE_DOWN_DP = 70f

        /** A tap this near a freehand path's first point closes it; points are added this far apart (dp). */
        const val FREEHAND_CLOSE_DP = 22f
        const val FREEHAND_STEP_DP = 2f

        /** How far a touch on the transform box moves before its drag begins (dp); the drag counts from there. */
        const val TRANSFORM_SLOP_DP = 15f

        /** The window the eyedropper shows (magnified twice), dp across. */
        const val PICK_WINDOW_DP = 54f
    }
}
