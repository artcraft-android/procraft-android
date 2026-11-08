package io.github.artcraftandroid.procraft.ui

import android.os.SystemClock
import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel.DropBand
import kotlin.math.PI
import kotlin.math.acos
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sin

/** What a touch on a colour that can be dragged out turned out to be. */
private enum class DropTouch { TAP, HOLD, DRAG, GONE }

/**
 * A colour that can be dragged out onto the canvas to fill there (the colour button, or a swatch
 * with [swatch] giving its colour): tapped it does [onTap], held still [onHold]; dragged, its colour
 * goes with the finger as a dot and fills where it is let go below the top bar.
 */
internal fun Modifier.dropSource(vm: CanvasViewModel, swatch: (() -> Int)? = null, onHold: (() -> Unit)? = null, onTap: () -> Unit): Modifier = composed {
    val tap by rememberUpdatedState(onTap)
    val hold by rememberUpdatedState(onHold)
    val colour by rememberUpdatedState(swatch)
    var origin by remember { mutableStateOf(Offset.Zero) }
    var extent by remember { mutableStateOf(IntSize.Zero) }
    onGloballyPositioned {
        origin = it.positionInRoot()
        extent = it.size
    }.pointerInput(vm) {
        awaitEachGesture {
            val down = awaitFirstDown()
            down.consume()
            // whichever comes first: lifted, moved past the slop, or (when holding does something) held
            val wait = if (hold != null) viewConfiguration.longPressTimeoutMillis else Long.MAX_VALUE
            val touch = withTimeoutOrNull(wait) {
                var seen: DropTouch? = null
                while (seen == null) {
                    val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id }
                    seen = when {
                        c == null -> DropTouch.GONE
                        !c.pressed -> DropTouch.TAP.also { c.consume() }
                        (c.position - down.position).getDistance() > viewConfiguration.touchSlop -> DropTouch.DRAG
                        else -> null
                    }
                }
                seen
            } ?: DropTouch.HOLD
            when (touch) {
                DropTouch.TAP -> tap()
                DropTouch.HOLD -> hold?.invoke()
                DropTouch.GONE -> Unit
                DropTouch.DRAG -> {
                    var at = origin + down.position
                    // the middle of the button or swatch it came out of, which is where the pull is
                    // measured from - the touch itself can land anywhere on it
                    val centre = origin + Offset(extent.width / 2f, extent.height / 2f)
                    vm.dropDragStart(at, centre, colour?.invoke(), down.type == PointerType.Stylus || down.type == PointerType.Eraser, down.uptimeMillis)
                    var lifted = false
                    while (true) {
                        // held still a moment, the colour comes to rest (a slide then sets the threshold)
                        val due = vm.dropRestDue()
                        val event = if (due == null) {
                            awaitPointerEvent()
                        } else {
                            withTimeoutOrNull((due - SystemClock.uptimeMillis()).coerceAtLeast(1L)) { awaitPointerEvent() }
                        }
                        if (event == null) {
                            vm.dropDragRest(SystemClock.uptimeMillis())
                            continue
                        }
                        val c = event.changes.firstOrNull { it.id == down.id } ?: break
                        c.consume()
                        at = origin + c.position
                        if (!c.pressed) {
                            lifted = true
                            break
                        }
                        vm.dropDragMove(at, c.uptimeMillis)
                    }
                    // let go on the top bar, it is put back
                    vm.dropDragEnd(at.takeIf { lifted && it.y > Pc.BarHeight.toPx() })
                }
            }
        }
    }
}

/**
 * The colour pulled out of a button, drawn as the pull it is: the button's own dot and the colour out
 * of it, each a drop pointing at the other while they hold together. Close in the two cross into each
 * other rather than meeting on a point; the button's dot is dragged a little after the pointer and
 * goes back the moment they part; and once it is free the colour carries its own velocity, so whirled
 * about it swings wide of the pointer, squashed along the way it is going and drawn out across it.
 */
@Composable
fun DropDot(vm: CanvasViewModel) {
    val away = vm.dropSwatchAway
    if (vm.dropDot == null && !away) return
    val across = with(LocalDensity.current) { Pc.ColorDot.toPx() }
    val blob = vm.dropBlob
    var drawn by remember { mutableStateOf(DropDrawn()) }
    LaunchedEffect(Unit) {
        var last = -1.0
        while (true) {
            val now = androidx.compose.runtime.withFrameNanos { it } / 1_000_000_000.0
            val dt = if (last < 0.0) 0f else (now - last).toFloat().coerceIn(0f, 0.05f)
            last = now
            val dot = vm.dropDot
            if (dot != null) blob.step(dot.first, across, now) else blob.settle(dt)
            vm.dropAway(blob.swatchAway)
            drawn = blob.shape(dot?.second ?: vm.colorRgb(), dot != null)
        }
    }
    val shape = drawn
    Canvas(Modifier.fillMaxSize()) {
        val ink = CanvasSpace.color(shape.rgb)
        if (shape.swatchRadius > 0.2f) {
            drawPath(dropPath(shape.swatch, shape.swatchRadius, shape.colour, shape.swatchTail), ink)
        }
        if (shape.colourRadius <= 0.2f) return@Canvas
        if (shape.pointed) {
            drawPath(dropPath(shape.colour, shape.colourRadius, shape.swatch, shape.colourTail), ink)
        } else {
            drawPath(squashedPath(shape.colour, shape.colourRadius, shape.heading, shape.stretch), ink)
        }
    }
}

/** What the pull looks like this frame: its two ends, or the colour on its own. */
internal data class DropDrawn(
    val rgb: Int = 0,
    val swatch: Offset = Offset.Zero,
    val swatchRadius: Float = 0f,
    val swatchTail: Float = 0f,
    val colour: Offset = Offset.Zero,
    val colourRadius: Float = 0f,
    val colourTail: Float = 0f,
    val heading: Float = 0f,
    val stretch: Float = 0f,
    val pointed: Boolean = false,
)

/**
 * The state of one pull out of a button, a frame at a time: where each end is, how much of them is
 * still holding together, how far the button's dot has been dragged after the pointer, and - once
 * they have parted - the colour's own velocity and how far that has drawn it out of shape.
 */
internal class DropBlob {
    var home = Offset.Zero
    var swing = Offset.Zero
    var near = Offset.Zero
    var far = Offset.Zero
    var target = Offset.Zero
    var reach = 1f
    var link = 1f
    var holding = true
    var moving = Offset.Zero
    var heading = 0f
    var stretch = 0f
    var drawn = 0f
    var radius = 1f
    var bar = true
    private var last = -1.0

    /** Whether it is drawing the button's own dot off its place, so the bar leaves it be. */
    val swatchAway: Boolean get() = bar && (link > 0.05f || swing.getDistance() > radius * HOME)

    /** Starts afresh at the middle of the button the colour is pulled out of. */
    fun start(centre: Offset, fromBar: Boolean) {
        home = centre
        bar = fromBar
        swing = Offset.Zero
        near = centre
        far = centre
        target = centre
        reach = 1f
        link = 1f
        holding = true
        moving = Offset.Zero
        heading = 0f
        stretch = 0f
        drawn = 0f
        last = -1.0
    }

    /** One frame, `now` seconds since the app started, the pointer at [at], the dot [across] wide. */
    fun step(at: Offset, across: Float, now: Double) {
        radius = across / 2f
        target = at
        val dt = if (last < 0.0) 0f else (now - last).toFloat().coerceIn(0f, 0.05f)
        last = now
        reach = radius * 2f * REACH
        val off = at - home
        val to = off.getDistance()
        // dragged a little after the pointer while they hold on, and let go back home once they part
        val sway = if (bar) unit(off) * (min(to * SWAY, radius * SWAY_MAX) * link) else Offset.Zero
        swing += (sway - swing) * (dt * 12f).coerceIn(0f, 1f)
        near = home + swing
        if (holding) {
            far = at
            moving = Offset.Zero
        } else {
            moving += ((at - far) * SPRING - moving * DAMPING) * dt
            far += moving * dt
        }
        val gap = (near - far).getDistance()
        val pulled = (gap / reach).coerceIn(0f, 1f)
        // it thins as it is pulled, and comes up a little again once it is free
        val wanted = if (gap < reach) radius * (1f - (1f - THINNEST) * pulled) else radius * SETTLED
        drawn += (wanted - drawn) * (dt * 14f).coerceIn(0f, 1f)
        // the point it had goes quickly once they part: what is left of it is a ball of liquid
        val wantedLink = if (gap < reach) 1f else 0f
        val givenUp = if (wantedLink > 0f) 18f else 42f
        link += (wantedLink - link) * (dt * givenUp).coerceIn(0f, 1f)
        holding = gap < reach
        // squashed along the way it is going by how fast that is and drawn out across it, but only
        // once its point has gone, so the shape it is already drawing does not jump when it changes
        val speed = moving.getDistance()
        val wantedStretch = if (holding || tail(drawn) > 0.05f) 0f else min(speed / (radius * SPEED), STRETCH)
        stretch += (wantedStretch - stretch) * (dt * 20f).coerceIn(0f, 1f)
        if (speed > 1f) heading += wrapped(atan2(moving.y, moving.x) - heading) * (dt * 14f).coerceIn(0f, 1f)
    }

    /**
     * How far the end of [at] is drawn out: as much as it takes to reach the middle of the gap
     * between the two of them, and none while that middle is still inside it. Both ends ask for the
     * same middle, which is what brings their points onto each other.
     */
    fun tail(at: Float): Float {
        val gap = (near - far).getDistance()
        val slack = ((1f - gap / reach.coerceAtLeast(1f)) / CLOSING).coerceIn(0f, 1f)
        return (gap * 0.5f - at + at * OVERLAP * slack).coerceAtLeast(0f) * link
    }

    /** Let go of mid-pull: the rest of the way home, on its own. */
    fun settle(dt: Float) {
        swing *= 1f - (dt * 12f).coerceIn(0f, 1f)
        link = 0f
        holding = true
    }

    /** What to draw this frame, for a pull out of the top bar's button or out of a palette. */
    fun shape(rgb: Int, live: Boolean): DropDrawn {
        if (!bar) {
            // a palette draws its own swatch and it does not bend, so the colour is a plain ball
            if (!live) return DropDrawn(rgb = rgb)
            return DropDrawn(rgb = rgb, colour = target, colourRadius = drawn)
        }
        val colourTail = tail(drawn)
        return DropDrawn(
            rgb = rgb,
            swatch = near,
            swatchRadius = radius,
            swatchTail = tail(radius),
            colour = far,
            // let go of, there is no colour out any more: only the button's dot is still on its way back
            colourRadius = if (live) drawn else 0f,
            colourTail = colourTail,
            heading = heading,
            stretch = stretch,
            pointed = colourTail > 0.05f,
        )
    }
}

/** How far it can be pulled before the two let go, as a share of the button's dot across. */
private const val REACH = 1.8f

/** How far each point runs past the middle of the gap while they are still close, as a share of itself. */
private const val OVERLAP = 0.65f

/** How much of the pull, at its far end, the crossing takes to close up. */
private const val CLOSING = 0.22f

/** How thin it gets while it is still holding on. */
private const val THINNEST = 0.55f

/** What the dragged one is worth once they are apart, against the button's dot. */
private const val SETTLED = 0.72f

/** How far the button's dot is dragged after the pointer, as a share of the gap and of its radius. */
private const val SWAY = 0.21f
private const val SWAY_MAX = 0.9f

/** Under this, of the dot's radius, the button's dot is back on its own place. */
private const val HOME = 0.02f

/** How fast the colour goes, in dot radii a second, to be drawn right out of shape, and how far out. */
private const val SPEED = 116f
private const val STRETCH = 0.35f

/** How hard the colour is pulled after the pointer once it is free, and how hard that is damped. */
private const val SPRING = 256f
private const val DAMPING = 10f

/** [v] made a length of one, or nothing at all if it has no length. */
private fun unit(v: Offset): Offset {
    val length = v.getDistance()
    return if (length > 0.0001f) v / length else Offset.Zero
}

/** [turn] brought into -pi..pi. */
private fun wrapped(turn: Float): Float {
    val full = (2f * PI).toFloat()
    return ((turn + PI.toFloat()) % full + full) % full - PI.toFloat()
}

/**
 * A drop: the ball of [radius] at [at], drawn out to a point [tail] beyond it toward [toward]. The
 * point is where the two lines off the ball, run straight in to it, meet, so the outline comes to a
 * corner there rather than ending in a nose.
 */
private fun dropPath(at: Offset, radius: Float, toward: Offset, tail: Float): Path {
    val path = Path()
    val off = toward - at
    val span = off.getDistance()
    if (span < 0.01f || tail <= 0.05f) {
        path.addOval(Rect(center = at, radius = radius))
        return path
    }
    val dir = off / span
    val out = radius + tail
    // how far round the ball the point is seen to sit, which is where its two lines touch
    val edge = acos((radius / out).coerceIn(-1f, 1f))
    val base = atan2(dir.y, dir.x)
    val full = (2f * PI).toFloat()
    val steps = 64
    for (i in 0..steps) {
        val angle = base + edge + (full - 2f * edge) * (i.toFloat() / steps)
        val x = at.x + cos(angle) * radius
        val y = at.y + sin(angle) * radius
        if (i == 0) path.moveTo(x, y) else path.lineTo(x, y)
    }
    path.lineTo(at.x + dir.x * out, at.y + dir.y * out)
    path.close()
    return path
}

/** A ball drawn out of true: shorter along [heading], wider across it. */
private fun squashedPath(at: Offset, radius: Float, heading: Float, stretch: Float): Path {
    val path = Path()
    val along = radius * (1f - stretch * 0.7f)
    val across = radius * (1f + stretch)
    val turnSin = sin(heading)
    val turnCos = cos(heading)
    val full = (2f * PI).toFloat()
    val steps = 48
    for (i in 0 until steps) {
        val angle = full * i / steps
        val x = cos(angle) * along
        val y = sin(angle) * across
        val px = at.x + x * turnCos - y * turnSin
        val py = at.y + x * turnSin + y * turnCos
        if (i == 0) path.moveTo(px, py) else path.lineTo(px, py)
    }
    path.close()
    return path
}

/**
 * ColorDrop's band under the top bar, sliding down from behind it: while a colour is dragged and
 * while filling goes on, with the threshold as a blue line across the screen under it; after a
 * drop, for a moment, offering to continue filling. Sliding the threshold adds a row reading it.
 */
@Composable
fun DropBandUi(vm: CanvasViewModel) {
    val band = vm.dropBand
    val shown = remember { Animatable(0f) }
    // what it showed, kept while it slides away
    val kept = remember { arrayOf(DropBand.NONE) }
    if (band != DropBand.NONE) kept[0] = band
    LaunchedEffect(band == DropBand.NONE) {
        if (band == DropBand.NONE) shown.animateTo(0f, Motion.noticeOut()) else shown.animateTo(1f, Motion.noticeIn())
    }
    if (band == DropBand.NONE && shown.value <= 0f) return
    val what = kept[0]
    val ink = if (Pc.light) Color(0xFF222222) else Color(0xFFC5C5C5)
    val blue = if (Pc.light) Color(0xFF0A8CFD) else Color(0xFF0076FD)
    BoxWithConstraints(Modifier.offset(y = Pc.BarHeight).fillMaxWidth().clipToBounds()) {
        val width = maxWidth
        Column(Modifier.graphicsLayer { translationY = -(1f - shown.value) * size.height }) {
            Box(Modifier.fillMaxWidth().height(32.dp).background(if (Pc.light) BandLight else BandDark), contentAlignment = Alignment.Center) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("ColorDrop", color = ink, fontSize = 16.5.sp, fontWeight = FontWeight.Bold, maxLines = 1)
                    Spacer(Modifier.width(13.dp))
                    if (what == DropBand.FILLING) {
                        // the tick ends filling
                        Box(Modifier.size(20.dp).clip(CircleShape).background(blue).press { vm.endFilling() }) {
                            Canvas(Modifier.size(20.dp)) {
                                val u = size.width / 20f
                                val tick = Path().apply {
                                    moveTo(10f * u - 4.2f * u, 10f * u + 0.1f * u)
                                    lineTo(10f * u - 1.4f * u, 10f * u + 2.6f * u)
                                    lineTo(10f * u + 4f * u, 10f * u - 2.8f * u)
                                }
                                drawPath(tick, Color.White, style = Stroke(2f * u, cap = StrokeCap.Round, join = StrokeJoin.Round))
                            }
                        }
                    } else {
                        Box(
                            Modifier.size(101.dp, 20.dp).clip(RoundedCornerShape(10.dp)).background(blue).press { vm.continueFilling() },
                            contentAlignment = Alignment.Center,
                        ) {
                            Text("Continue filling", color = Color.White, fontSize = 11.7.sp, maxLines = 1)
                        }
                    }
                }
            }
            if (vm.dropSliding) {
                Box(Modifier.fillMaxWidth().height(24.dp).background(if (Pc.light) RowLight else RowDark), contentAlignment = Alignment.Center) {
                    Text("Threshold ${(vm.dropThreshold * 100).roundToInt()}%", color = ink, fontSize = 14.4.sp, maxLines = 1)
                }
            }
            if (what == DropBand.DRAGGING || what == DropBand.FILLING) {
                Box(Modifier.width(width * vm.dropThreshold).height(4.dp).background(blue))
            }
        }
    }
}

/**
 * A fill let go is still coming in where it was dropped: keeps the frames coming and moves it on
 * until it is all there.
 */
@Composable
fun DropSpread(vm: CanvasViewModel) {
    if (!vm.pourSpreading()) return
    LaunchedEffect(Unit) {
        while (vm.pourSpreading()) {
            androidx.compose.runtime.withFrameNanos { }
            vm.pourTick()
        }
    }
}

/** A tap target without a ripple. */
private fun Modifier.press(onClick: () -> Unit): Modifier = composed {
    clickable(remember { MutableInteractionSource() }, indication = null, onClick = onClick)
}

/** The band, translucent over the canvas, dark and light. */
private val BandDark = Color(0xD63B3B3B)
private val BandLight = Color(0xD6EEEEEE)

/** The threshold's row under it. */
private val RowDark = Color(0xCC4A4A4A)
private val RowLight = Color(0xCCDCDCDC)
