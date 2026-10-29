package io.github.artcraftandroid.procraft.ui

import android.graphics.BlurMaskFilter
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.graphics.ClipOp
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Paint
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.layout.positionOnScreen
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** Where frosted panels are told about: on the canvas screen only, which has a canvas behind them. */
val LocalBackdrops = staticCompositionLocalOf<Backdrops?> { null }

/** The rounded bodies of the frosted panels open now (window pixels), sent on whenever they change. */
class Backdrops(private val send: (List<Float>) -> Unit) {
    private val regions = LinkedHashMap<Any, FloatArray>()

    fun put(key: Any, region: FloatArray) {
        if (regions[key]?.contentEquals(region) == true) return
        regions[key] = region
        send(regions.values.flatMap { it.asList() })
    }

    fun remove(key: Any) {
        if (regions.remove(key) == null) return
        send(regions.values.flatMap { it.asList() })
    }
}

/**
 * Makes the node it is put on frosted glass over the canvas while it is shown: what is behind its
 * rounded box ([corner]) is blurred. Works from popups too (it goes by screen position).
 */
@Composable
fun rememberFrost(corner: Dp): Modifier {
    val backdrops = LocalBackdrops.current ?: return Modifier
    val key = remember { Any() }
    val density = LocalDensity.current
    // the blur grows and fades with the panel
    val look = LocalPanelLook.current
    var box by remember { mutableStateOf<FloatArray?>(null) }
    DisposableEffect(backdrops) { onDispose { backdrops.remove(key) } }
    SideEffect {
        box?.let { (x0, y0, x1, y1, corner) ->
            val (px, py, k) = Triple(look.pivot.x, look.pivot.y, look.scale)
            backdrops.put(key, floatArrayOf(px + (x0 - px) * k, py + (y0 - py) * k, px + (x1 - px) * k, py + (y1 - py) * k, corner * k, look.alpha))
        }
    }
    return Modifier.onGloballyPositioned { c ->
        val at = c.positionOnScreen()
        box = floatArrayOf(at.x, at.y, at.x + c.size.width, at.y + c.size.height, with(density) { corner.toPx() })
    }
}


/** The side of a popover its arrow sticks out of, pointing at its button; or none. */
enum class ArrowSide { TOP, BOTTOM, LEFT, RIGHT, NONE }

/**
 * A panel hanging from the top bar, with an arrow pointing up at its button: [arrowX] is the tip's
 * distance from the panel's left edge (from its top, for an arrow on the [side]). [popoverPlace]
 * finds where it goes. Without a [fill] of its own it is frosted over the canvas: translucent,
 * with what is behind it blurred. [width] is the body's: a side arrow adds to it.
 */
@Composable
fun Popover(
    width: Dp,
    arrowX: Dp,
    modifier: Modifier = Modifier,
    fill: Color? = null,
    corner: Dp = Pc.PopoverCorner,
    side: ArrowSide = ArrowSide.TOP,
    content: @Composable ColumnScope.() -> Unit,
) {
    val backdrops = LocalBackdrops.current.takeIf { fill == null && Pc.PopoverFrost.alpha < 1f }
    val paint = fill ?: if (backdrops != null) Pc.PopoverFrost else Pc.PopoverFill
    val key = remember { Any() }
    val density = LocalDensity.current
    // the blur grows and fades with the panel: its body as drawn (already grown), and how much
    // it shows
    val look = LocalPanelLook.current
    var body by remember { mutableStateOf<FloatArray?>(null) }
    if (backdrops != null) {
        DisposableEffect(backdrops) { onDispose { backdrops.remove(key) } }
        // read here, so its being placed (or moved) comes back through here
        val placed = body
        SideEffect { placed?.let { backdrops.put(key, it + look.alpha) } }
    }
    val frost = if (backdrops == null) {
        Modifier
    } else {
        Modifier.onGloballyPositioned { c ->
            val at = c.positionInWindow()
            val k = look.scale
            val (arrowH, radius) = with(density) { Pc.ArrowHeight.toPx() to corner.toPx() }
            val (l, t, r) = Triple(if (side == ArrowSide.LEFT) arrowH else 0f, if (side == ArrowSide.TOP) arrowH else 0f, if (side == ArrowSide.RIGHT) arrowH else 0f)
            val b = if (side == ArrowSide.BOTTOM) arrowH else 0f
            body = floatArrayOf(at.x + l * k, at.y + t * k, at.x + (c.size.width - r) * k, at.y + (c.size.height - b) * k, radius * k)
        }
    }
    val sideways = side == ArrowSide.LEFT || side == ArrowSide.RIGHT
    Box(
        modifier.width(if (sideways) width + Pc.ArrowHeight else width).then(frost).drawBehind {
            val arrowH = Pc.ArrowHeight.toPx()
            val at = arrowX.toPx()
            val half = arrowH
            val tip = 2.dp.toPx()
            val radius = CornerRadius(corner.toPx())
            val outline = Path().apply {
                when (side) {
                    ArrowSide.TOP -> {
                        addRoundRect(RoundRect(0f, arrowH, size.width, size.height, radius))
                        moveTo(at - half - tip, arrowH + 0.5f)
                        lineTo(at - tip, tip)
                        quadraticTo(at, 0f, at + tip, tip)
                        lineTo(at + half + tip, arrowH + 0.5f)
                        close()
                    }
                    ArrowSide.BOTTOM -> {
                        val h = size.height
                        addRoundRect(RoundRect(0f, 0f, size.width, h - arrowH, radius))
                        moveTo(at - half - tip, h - arrowH - 0.5f)
                        lineTo(at - tip, h - tip)
                        quadraticTo(at, h, at + tip, h - tip)
                        lineTo(at + half + tip, h - arrowH - 0.5f)
                        close()
                    }
                    ArrowSide.LEFT -> {
                        addRoundRect(RoundRect(arrowH, 0f, size.width, size.height, radius))
                        moveTo(arrowH + 0.5f, at - half - tip)
                        lineTo(tip, at - tip)
                        quadraticTo(0f, at, tip, at + tip)
                        lineTo(arrowH + 0.5f, at + half + tip)
                        close()
                    }
                    ArrowSide.RIGHT -> {
                        val w = size.width
                        addRoundRect(RoundRect(0f, 0f, w - arrowH, size.height, radius))
                        moveTo(w - arrowH - 0.5f, at - half - tip)
                        lineTo(w - tip, at - tip)
                        quadraticTo(w, at, w - tip, at + tip)
                        lineTo(w - arrowH - 0.5f, at + half + tip)
                        close()
                    }
                    ArrowSide.NONE -> addRoundRect(RoundRect(0f, 0f, size.width, size.height, radius))
                }
            }
            drawIntoCanvas { canvas ->
                val shadow = Paint().apply {
                    color = Pc.PopoverShadow
                    asFrameworkPaint().maskFilter = BlurMaskFilter(Pc.PopoverShadowRadius.toPx(), BlurMaskFilter.Blur.NORMAL)
                }
                // only around the panel: none of it shows through
                canvas.save()
                canvas.clipPath(outline, ClipOp.Difference)
                canvas.drawPath(outline, shadow)
                canvas.restore()
            }
            drawPath(outline, paint)
        },
    ) {
        val inset = when (side) {
            ArrowSide.TOP -> Modifier.padding(top = Pc.ArrowHeight)
            ArrowSide.BOTTOM -> Modifier.padding(bottom = Pc.ArrowHeight)
            ArrowSide.LEFT -> Modifier.padding(start = Pc.ArrowHeight)
            ArrowSide.RIGHT -> Modifier.padding(end = Pc.ArrowHeight)
            ArrowSide.NONE -> Modifier
        }
        Column(inset.clip(RoundedCornerShape(corner)), content = content)
    }
}

/** Where a popover [width] wide goes on a screen [screenWidth] wide: its left edge and arrow tip. */
data class PopoverPlace(val left: Dp, val arrowX: Dp)

/**
 * A popover is centred on its button (the tip sits [Pc.ArrowShift] right of the button's centre)
 * and kept [Pc.PopoverGap] inside the screen, and at least [minLeft] from its left edge (clear of
 * the side bar).
 */
fun popoverPlace(width: Dp, buttonCentreX: Dp, screenWidth: Dp, minLeft: Dp = Pc.PopoverGap, minRight: Dp = Pc.PopoverGap): PopoverPlace {
    val tip = buttonCentreX + Pc.ArrowShift
    val left = (tip - width / 2).coerceIn(minLeft, (screenWidth - minRight - width).coerceAtLeast(minLeft))
    return PopoverPlace(left, tip - left)
}
