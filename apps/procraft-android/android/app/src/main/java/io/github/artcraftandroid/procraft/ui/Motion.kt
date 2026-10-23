package io.github.artcraftandroid.procraft.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.AnimationSpec
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.Easing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.positionOnScreen
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.animation.core.VectorConverter
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.sin
import kotlin.math.sqrt

/** How the interface moves: every animation's timing in one place, as measured on the reference. */
object Motion {
    /**
     * A menu opening: it grows out of the point it hangs from, from almost nothing, overshooting a
     * little; closing, it fades as it shrinks to [MENU_END_SCALE]. (The panels hanging from the top
     * bar don't move: they come and go at once.)
     */
    const val MENU_MS = 300
    const val MENU_START_SCALE = 0.001f
    const val MENU_END_SCALE = 0.8f
    val MenuOpen: Easing = CubicBezierEasing(0.26f, 1.22f, 0.39f, 1.01f)

    /** A menu closing, and the curve most timed changes take. */
    val Settle: Easing = CubicBezierEasing(0.2f, 0.8f, 0.5f, 1.0f)

    fun menuOpen(): AnimationSpec<Float> = tween(MENU_MS, easing = MenuOpen)
    fun menuClose(): AnimationSpec<Float> = tween(MENU_MS, easing = Settle)

    /** Buttons lighting up as they are pressed, and going back. */
    const val HIGHLIGHT_ON_MS = 67L
    fun highlightOn(): AnimationSpec<Float> = tween(HIGHLIGHT_ON_MS.toInt(), easing = LinearEasing)
    fun highlightOff(): AnimationSpec<Float> = tween(150, easing = LinearEasing)

    /** The ease in and out of most timed changes. */
    val EaseInOut: Easing = CubicBezierEasing(0.42f, 0f, 0.58f, 1f)

    /**
     * The band under the top bar telling what undo and redo did: down from behind the bar in 0.3 s
     * ease-in-out (fit rms 0.02), back up the same once 0.75 s have passed since it started down.
     */
    fun noticeIn(): AnimationSpec<Float> = tween(300, easing = EaseInOut)
    const val NOTICE_SHOWN_MS = 750L
    const val NOTICE_WARNING_SHOWN_MS = 1500L
    fun noticeOut(): AnimationSpec<Float> = tween(300, easing = EaseInOut)

    /** Messages coming and going. */
    fun message(): AnimationSpec<Float> = tween(300, easing = CubicBezierEasing(0.42f, 0f, 1f, 1f))

    /** The bars hidden and shown (the interface hidden). */
    fun bars(): AnimationSpec<Float> = tween(300, easing = EaseInOut)

    /**
     * A switch flipped: the knob slides across in about a third of a second as its track's colour
     * fades; it swells into a lens as it starts (to 1.35 by 1.4) and settles back once across.
     */
    fun switchKnob(): AnimationSpec<Float> = tween(330, easing = CubicBezierEasing(0.3f, 0f, 0.2f, 1f))
    fun switchSwell(): AnimationSpec<Float> = tween(70, easing = EaseInOut)
    fun switchSettle(): AnimationSpec<Float> = tween(120, easing = EaseInOut)
    const val SWITCH_HOLD_MS = 140L
    const val SWITCH_SWELL_X = 0.35f
    const val SWITCH_SWELL_Y = 0.4f

    /** A swatch pressed: it shrinks fast, then springs back with a bounce when let go. */
    const val SWATCH_PRESSED = 0.78f
    fun swatchPress(): AnimationSpec<Float> = tween(100, easing = Spring(0.1f, 0.8f, 2f))
    fun swatchRelease(): AnimationSpec<Float> = tween(300, easing = Spring(0.3f, 0.18f, 6f))

    /** The QuickMenu's buttons spreading from its middle as they fade in (0.15 s), and drawing back in (0.12 s). */
    fun quickMenuOpen(): AnimationSpec<Float> = tween(150, easing = Ease)
    fun quickMenuClose(): AnimationSpec<Float> = tween(120, easing = CubicBezierEasing(0.42f, 0f, 1f, 1f))

    /** The gallery zooming into an artwork as it opens (0.4 s), and out of it as it closes (0.5 s). */
    fun galleryOpen(): AnimationSpec<Float> = tween(400, easing = EaseInOut)
    fun galleryClose(): AnimationSpec<Float> = tween(500, easing = EaseInOut)

    /** The gallery going into and out of Select. */
    fun selectMode(): AnimationSpec<Float> = tween(150, easing = EaseInOut)

    /** An artwork in the gallery held: it shrinks under the finger, then lifts, and settles when let go. */
    const val ARTWORK_PRESSED = 0.87f
    const val ARTWORK_PRESS_DELAY_MS = 100L
    const val ARTWORK_LIFTED = 1.07f
    fun artworkPress(): AnimationSpec<Float> = tween(150, easing = EaseInOut)
    fun artworkLift(): AnimationSpec<Float> = tween(250, easing = EaseInOut)
    fun artworkDrop(): AnimationSpec<Float> = tween(250, easing = EaseInOut)

    /** Renaming an artwork: it rises to the top over the gallery blurring (0.3 s); done, it fades away (0.15 s). */
    fun renameRise(): AnimationSpec<Float> = tween(300, easing = EaseInOut)
    fun renameFade(): AnimationSpec<Float> = tween(150, easing = EaseInOut)

    /** An artwork's actions shown by a swipe growing in, and going. */
    fun swipeActions(): AnimationSpec<Float> = tween(300, easing = Spring(0.3f, 0.75f, 0f))

    /** A swiped row settling open or shut. */
    fun swipeSettle(): AnimationSpec<Float> = tween(300, easing = Spring(0.3f, 0.8f, 0.2f))

    /** A colour panel tab's icon held down (it shrinks as a highlight comes on, and back as one goes off). */
    const val TAB_PRESSED = 0.75f

    /** The Actions panel changing tabs: the old tab's rows fade out, then the new tab's fade in. */
    fun actionsTabOut(): AnimationSpec<Float> = tween(100, easing = EaseInOut)
    fun actionsTabIn(): AnimationSpec<Float> = tween(200, easing = EaseInOut)

    /** A panel's pages pushed in and back: 0.45 s on the ease curve. */
    const val PANEL_PAGE_MS = 450
    val Ease: Easing = CubicBezierEasing(0.25f, 0.1f, 0.25f, 1f)

    /**
     * A panel's page pushed in ([forward]): it slides in from the right as the page it covers
     * slides off to the left. Going back, the page goes at once as the one under it slides back in
     * from the left. The panel's size follows.
     */
    fun panelPage(forward: Boolean): androidx.compose.animation.ContentTransform {
        val move = tween<androidx.compose.ui.unit.IntOffset>(PANEL_PAGE_MS, easing = Ease)
        return androidx.compose.animation.ContentTransform(
            androidx.compose.animation.slideInHorizontally(move) { if (forward) it else -it },
            if (forward) androidx.compose.animation.slideOutHorizontally(move) { -it } else androidx.compose.animation.fadeOut(androidx.compose.animation.core.snap()),
            sizeTransform = androidx.compose.animation.SizeTransform(clip = true) { _, _ -> tween(PANEL_PAGE_MS, easing = Ease) },
        )
    }

    /**
     * The brush studio and the full-screen pages (canvas information, gesture controls) sliding up
     * (a spring settling in 0.5 s, not bouncing) and down (0.3 s).
     */
    fun studioIn(): androidx.compose.animation.core.FiniteAnimationSpec<androidx.compose.ui.unit.IntOffset> =
        tween(500, easing = Spring(0.5f, 1f, 0f))
    fun studioOut(): androidx.compose.animation.core.FiniteAnimationSpec<androidx.compose.ui.unit.IntOffset> =
        tween(300, easing = EaseInOut)

    /** A bottom toolbar (transform, selection) rising from below the screen on a spring settling in 0.4 s, and dropping away in 0.075 s. */
    fun toolbarIn(): androidx.compose.animation.EnterTransition =
        androidx.compose.animation.slideInVertically(tween(400, easing = Spring(0.4f, 0.9f, 0f))) { it * 3 / 2 }
    fun toolbarOut(): androidx.compose.animation.ExitTransition =
        androidx.compose.animation.slideOutVertically(tween(75, easing = CubicBezierEasing(0.42f, 0f, 1f, 1f))) { it * 3 / 2 }

    /** The side bar sliding off its edge as a mode takes the screen (0.13 s, speeding up), and back (0.2 s, slowing). */
    fun sideBarIn(right: Boolean): androidx.compose.animation.EnterTransition =
        androidx.compose.animation.slideInHorizontally(tween(200, easing = CubicBezierEasing(0f, 0f, 0.58f, 1f))) { if (right) it else -it }
    fun sideBarOut(right: Boolean): androidx.compose.animation.ExitTransition =
        androidx.compose.animation.slideOutHorizontally(tween(130, easing = CubicBezierEasing(0.42f, 0f, 1f, 1f))) { if (right) it else -it }

    /** A tool's header (crop, guides) fading in over the top bar as its mode begins. */
    fun toolHeaderIn(): AnimationSpec<Float> = tween(220, easing = EaseInOut)

    /** The reference window fading in where it is, and out when closed. */
    fun referenceFade(): androidx.compose.animation.core.FiniteAnimationSpec<Float> = tween(280, easing = EaseInOut)

    /** A panel fading away as something picked from it takes over (an adjustment): 0.25 s. */
    fun panelFade(): AnimationSpec<Float> = tween(250, easing = EaseInOut)

    /** The time-lapse replay fading in over the canvas as the panel fades away, and out on Done: 0.25 s. */
    fun replayFade(): androidx.compose.animation.core.FiniteAnimationSpec<Float> = tween(250, easing = EaseInOut)

    /**
     * A layer's blend modes: opening, the panel grows down over them and they fade in, 0.25 s;
     * closing, they fade as it shrinks back, 0.1 s, and 0.1 s later the rows under the layer come
     * back over 0.12 s (after opening, straight away).
     */
    const val BLEND_OPEN_MS = 250L
    const val BLEND_CLOSE_MS = 100L
    const val BLEND_ROWS_WAIT_MS = 100L
    fun blendOpen(): androidx.compose.animation.core.FiniteAnimationSpec<Float> = tween(BLEND_OPEN_MS.toInt(), easing = EaseInOut)
    fun blendOpenSize(): androidx.compose.animation.core.FiniteAnimationSpec<androidx.compose.ui.unit.IntSize> = tween(BLEND_OPEN_MS.toInt(), easing = EaseInOut)
    fun blendClose(): androidx.compose.animation.core.FiniteAnimationSpec<Float> = tween(BLEND_CLOSE_MS.toInt(), easing = EaseInOut)
    fun blendCloseSize(): androidx.compose.animation.core.FiniteAnimationSpec<androidx.compose.ui.unit.IntSize> = tween(BLEND_CLOSE_MS.toInt(), easing = EaseInOut)
    fun blendRowsBack(): AnimationSpec<Float> = tween(120, easing = EaseInOut)


    /**
     * Into a stack of the gallery its artworks fly out of the stack's cell to their places; back out,
     * the gallery grows back into place, fading in.
     */
    fun stackPushMove(): AnimationSpec<Offset> = tween(450, easing = Spring(0.45f, 0.8f, 0f))
    fun stackPop(): androidx.compose.animation.core.FiniteAnimationSpec<Float> = tween(300, easing = Spring(0.3f, 0.8f, 0f))
    const val STACK_POP_SCALE = 0.8f

    /** The grown artwork lifting off a canvas that took a while to draw: 0.15 s. */
    fun coverFade(): AnimationSpec<Float> = tween(150, easing = EaseInOut)

    /** Leaving a stack, the gallery's title fading in (0.11 s) as the stack's fades out (0.07 s). */
    fun titleIn(): androidx.compose.animation.core.FiniteAnimationSpec<Float> = tween(110, easing = EaseInOut)
    fun titleOut(): androidx.compose.animation.core.FiniteAnimationSpec<Float> = tween(70, easing = EaseInOut)

    /**
     * Rows of a list arriving and going (a 0.3 s spring, damping 0.75), and moving to make room:
     * 0.35 s on the settling curve (measured as a layer was added: the spring is far too quick).
     */
    private val Rows = Spring(0.3f, 0.75f, 0f)
    fun rowMove(): androidx.compose.animation.core.FiniteAnimationSpec<androidx.compose.ui.unit.IntOffset> = tween(350, easing = Settle)
    fun rowFade(): androidx.compose.animation.core.FiniteAnimationSpec<Float> = tween(300, easing = Rows)
}

/**
 * A spring as a curve over a fixed time: the motion of a damped spring released from 0 towards 1
 * at [velocity] (in distances per second), with [damping] (1: critical; less bounces), stiff
 * enough to come to rest within [seconds].
 */
class Spring(private val seconds: Float, private val damping: Float, private val velocity: Float) : Easing {
    // the natural frequency, found once: the slowest at which the motion is within a thousandth of
    // its end by the time given
    private val omega: Double = run {
        var lo = 0.01
        var hi = 2000.0
        repeat(60) {
            val mid = (lo + hi) / 2
            if (envelope(mid, seconds.toDouble()) > 1e-3) lo = mid else hi = mid
        }
        hi
    }

    /** How far from its end the motion can still be at time [t], at frequency [w]. */
    private fun envelope(w: Double, t: Double): Double {
        val z = damping.toDouble().coerceAtLeast(0.01)
        val v = velocity.toDouble()
        return when {
            z < 1.0 -> {
                val wd = w * sqrt(1 - z * z)
                val c2 = (v - z * w) / wd
                sqrt(1 + c2 * c2) * exp(-z * w * t)
            }
            z == 1.0 -> (1 + abs(v - w) * t) * exp(-w * t)
            else -> {
                val s = sqrt(z * z - 1)
                val r1 = -w * (z - s)
                val r2 = -w * (z + s)
                val c1 = (v + r2) / (r1 - r2)
                abs(c1) * exp(r1 * t) + abs(-1 - c1) * exp(r2 * t)
            }
        }
    }

    override fun transform(fraction: Float): Float {
        if (fraction >= 1f) return 1f
        val t = fraction.toDouble() * seconds
        val z = damping.toDouble().coerceAtLeast(0.01)
        val v = velocity.toDouble()
        val w = omega
        val off = when {
            z < 1.0 -> {
                val wd = w * sqrt(1 - z * z)
                exp(-z * w * t) * (-cos(wd * t) + (v - z * w) / wd * sin(wd * t))
            }
            z == 1.0 -> (-1 + (v - w) * t) * exp(-w * t)
            else -> {
                val s = sqrt(z * z - 1)
                val r1 = -w * (z - s)
                val r2 = -w * (z + s)
                val c1 = (v + r2) / (r1 - r2)
                c1 * exp(r1 * t) + (-1 - c1) * exp(r2 * t)
            }
        }
        return (1 + off).toFloat()
    }

    companion object {
        /** A spring's curve run over its own time ([seconds]) as an animation. */
        fun spec(seconds: Float, damping: Float, velocity: Float = 0f, delaySeconds: Float = 0f): AnimationSpec<Float> =
            tween((seconds * 1000).toInt(), (delaySeconds * 1000).toInt(), Spring(seconds, damping, velocity))
    }
}

/**
 * Whether what [touch] is on is held down; after a quick tap it stays held long enough for a
 * highlight to come on.
 */
@Composable
fun heldOf(touch: androidx.compose.foundation.interaction.InteractionSource): Boolean {
    var held by remember { mutableStateOf(false) }
    LaunchedEffect(touch) {
        var since = 0L
        touch.interactions.collect { i ->
            when (i) {
                is androidx.compose.foundation.interaction.PressInteraction.Press -> {
                    held = true
                    since = System.nanoTime()
                }
                is androidx.compose.foundation.interaction.PressInteraction.Release,
                is androidx.compose.foundation.interaction.PressInteraction.Cancel -> {
                    val left = Motion.HIGHLIGHT_ON_MS - (System.nanoTime() - since) / 1_000_000
                    if (left > 0) kotlinx.coroutines.delay(left)
                    held = false
                }
            }
        }
    }
    return held
}

/** How lit a button is (0..1): it lights up fast and goes back more slowly. */
@Composable
fun animateHighlight(on: Boolean): Float =
    androidx.compose.animation.core.animateFloatAsState(
        if (on) 1f else 0f,
        if (on) Motion.highlightOn() else Motion.highlightOff(),
        label = "highlight",
    ).value

/**
 * Shows [value] while there is one, fading in and out by [spec]: the last value stays while it
 * fades out. [content] gets the value and how opaque it is.
 */
@Composable
fun <T : Any> FadingValue(value: T?, spec: AnimationSpec<Float>, content: @Composable (T, Float) -> Unit) {
    val last = remember { arrayOfNulls<Any>(1) }
    if (value != null) last[0] = value
    val shown = androidx.compose.animation.core.animateFloatAsState(if (value != null) 1f else 0f, spec, label = "fade").value
    @Suppress("UNCHECKED_CAST")
    val kept = last[0] as T? ?: return
    if (shown > 0f) content(kept, shown)
}

/** How a panel looks as it opens or closes, for its frosted backdrop to follow: [pivot] in screen pixels. */
class PanelLook(val alpha: Float, val scale: Float, val pivot: Offset)

val LocalPanelLook = compositionLocalOf { PanelLook(1f, 1f, Offset.Zero) }

/**
 * Shows [content] for [target] unless it is [hidden]. The panels hanging from the top bar come and
 * go at once (as measured on the reference, within a frame); [content] still gets how opaque and
 * how big the panel is and where this host's top left is on screen (pixels), for its frosted
 * backdrop.
 */
@Composable
fun <T> PanelPresence(
    target: T,
    hidden: T,
    fading: Boolean = false,
    onFaded: () -> Unit = {},
    content: @Composable (shown: T, alpha: Float, scale: Float, origin: Offset) -> Unit,
) {
    var origin by remember { mutableStateOf(Offset.Zero) }
    // the last panel shown, which fades away when closed [fading] (then [onFaded])
    val last = remember { arrayOfNulls<Any>(1) }
    val fade = remember { Animatable(1f) }
    if (target != hidden) last[0] = target
    LaunchedEffect(target, fading) {
        if (target == hidden && fading) {
            fade.snapTo(1f)
            fade.animateTo(0f, Motion.panelFade())
            onFaded()
        } else {
            fade.snapTo(1f)
        }
    }
    @Suppress("UNCHECKED_CAST")
    val shown = if (target != hidden) target else if (fading) last[0] as T? ?: return else return
    Box(
        Modifier.fillMaxSize().onGloballyPositioned { origin = it.positionOnScreen() }
            .then(if (target == hidden) Modifier.pointerInput(Unit) { awaitEachGesture { awaitFirstDown().consume() } } else Modifier),
    ) {
        content(shown, if (target == hidden) fade.value else 1f, 1f, origin)
    }
}

/** Gives the panels inside [content] their look, for their frosted backdrops to follow. */
@Composable
fun ProvidePanelLook(look: PanelLook, content: @Composable () -> Unit) {
    CompositionLocalProvider(LocalPanelLook provides look, content = content)
}

/**
 * A menu (a popup) that grows out of [origin] (a point of the menu, in fractions of its size) as it
 * opens, and fades as it shrinks back towards it as it closes, staying up while it goes. [content] puts the
 * popup up and gives its menu the [Modifier] it gets, which carries how it looks now.
 */
@Composable
fun MenuPresence(open: Boolean, origin: androidx.compose.ui.graphics.TransformOrigin, content: @Composable (Modifier) -> Unit) {
    var showing by remember { mutableStateOf(open) }
    // a menu first put up open still opens from its origin
    val alpha = remember { Animatable(0f) }
    val scale = remember { Animatable(Motion.MENU_START_SCALE) }
    LaunchedEffect(open) {
        if (open) {
            if (!showing || alpha.value <= 0f) {
                showing = true
                alpha.snapTo(0f)
                scale.snapTo(Motion.MENU_START_SCALE)
            }
            coroutineScope {
                launch { alpha.animateTo(1f, Motion.menuOpen()) }
                launch { scale.animateTo(1f, Motion.menuOpen()) }
            }
        } else {
            coroutineScope {
                launch { scale.animateTo(Motion.MENU_END_SCALE, Motion.menuClose()) }
                launch { alpha.animateTo(0f, Motion.menuClose()) }
            }
            showing = false
        }
    }
    if (!showing) return
    var pivot by remember { mutableStateOf(Offset.Zero) }
    val a = alpha.value.coerceIn(0f, 1f)
    val k = scale.value
    val look = Modifier
        .onGloballyPositioned { c ->
            pivot = c.positionOnScreen() + Offset(origin.pivotFractionX * c.size.width, origin.pivotFractionY * c.size.height)
        }
        .graphicsLayer {
            this.alpha = a
            scaleX = k
            scaleY = k
            transformOrigin = origin
        }
    ProvidePanelLook(PanelLook(a, k, pivot)) { content(look) }
}

/**
 * How much presses on [touch]'s area shrink something ([value], 1 when let go): held, it goes to
 * [pressed] by [press]; let go, back by [release].
 */
class PressScale(
    private val pressed: Float,
    private val press: AnimationSpec<Float>,
    private val release: AnimationSpec<Float>,
    private val scope: kotlinx.coroutines.CoroutineScope,
) {
    private val scale = Animatable(1f)
    val value: Float get() = scale.value

    val touch: Modifier = Modifier.pointerInput(Unit) {
        awaitEachGesture {
            awaitFirstDown(requireUnconsumed = false)
            scope.launch { scale.animateTo(pressed, press) }
            waitForUpOrCancellation()
            scope.launch { scale.animateTo(1f, release) }
        }
    }
}

@Composable
fun rememberPressScale(pressed: Float, press: AnimationSpec<Float>, release: AnimationSpec<Float>): PressScale {
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    return remember { PressScale(pressed, press, release, scope) }
}

/** Scales what it is on by [p]. */
fun Modifier.scaledBy(p: PressScale): Modifier = graphicsLayer {
    scaleX = p.value
    scaleY = p.value
}

/**
 * Flies what it is on in by [spec] from where [from] (window pixels) is, as its own bounds' top
 * left there; hidden until it is placed. Nothing moves without [from].
 */
fun Modifier.flyIn(from: androidx.compose.ui.geometry.Rect?, spec: AnimationSpec<Offset>): Modifier = composed {
    if (from == null) return@composed this
    val shift = remember { Animatable(Offset.Zero, Offset.VectorConverter) }
    var placed by remember { mutableStateOf(false) }
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    this
        .onGloballyPositioned { c ->
            if (placed) return@onGloballyPositioned
            placed = true
            val at = c.positionInWindow()
            scope.launch {
                shift.snapTo(Offset(from.left - at.x, from.top - at.y))
                shift.animateTo(Offset.Zero, spec)
            }
        }
        .graphicsLayer {
            alpha = if (placed) 1f else 0f
            translationX = shift.value.x
            translationY = shift.value.y
        }
}

/**
 * Once [leaving], flies what it is on by [spec] to where [to] (window pixels) is, its top left
 * there, from where it was last placed. Nothing moves without [to].
 */
fun Modifier.flyOut(to: androidx.compose.ui.geometry.Rect?, leaving: Boolean, spec: AnimationSpec<Float>): Modifier = composed {
    if (to == null) return@composed this
    var from by remember { mutableStateOf<Offset?>(null) }
    val gone = remember { Animatable(0f) }
    LaunchedEffect(leaving) { if (leaving) gone.animateTo(1f, spec) }
    this
        .onGloballyPositioned { if (!leaving) from = it.positionInWindow() }
        .graphicsLayer {
            val f = from ?: return@graphicsLayer
            translationX = (to.left - f.x) * gone.value
            translationY = (to.top - f.y) * gone.value
        }
}

/** Shrinks what it is on while it is pressed and springs it back when let go, as swatches do. */
fun Modifier.pressScale(): Modifier = composed {
    val p = rememberPressScale(Motion.SWATCH_PRESSED, Motion.swatchPress(), Motion.swatchRelease())
    this.then(p.touch).scaledBy(p)
}
