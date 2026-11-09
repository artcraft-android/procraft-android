package io.github.artcraftandroid.procraft.ui

import android.graphics.BlurMaskFilter
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.ClipOp
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Paint
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * Apple's liquid glass, as ProCraft Pocket has it: over the canvas the canvas renderer bends what is
 * behind the rounded box ([BarGlass.liquid] given to [rememberFrost]), and over that goes this look:
 * a faint [tint], a soft shadow outside it only (inside, what is behind shows), and its rim lit as a
 * bead of glass is, brightest toward the top left, a fainter light on the far side, and a thin line
 * all round keeping its shape. [corner] at half its height or more makes a pill.
 */
fun Modifier.liquidLook(corner: Dp, tint: Color = Pc.LiquidTint): Modifier = drawBehind {
    val r = minOf(corner.toPx(), size.minDimension / 2f)
    val outline = Path().apply { addRoundRect(RoundRect(0f, 0f, size.width, size.height, CornerRadius(r))) }
    clipPath(outline, ClipOp.Difference) {
        drawIntoCanvas { c ->
            val paint = Paint().asFrameworkPaint().apply {
                isAntiAlias = true
                color = Pc.LiquidShadow.toArgb()
                maskFilter = BlurMaskFilter(LiquidShadowBlur.toPx(), BlurMaskFilter.Blur.NORMAL)
            }
            val drop = LiquidShadowDrop.toPx()
            c.nativeCanvas.drawRoundRect(0f, drop, size.width, size.height + drop, r, r, paint)
        }
    }
    drawPath(outline, tint)
    rim(r, LiquidGlowWidth.toPx(), GLOW_SHARE)
    rim(r, LiquidRimWidth.toPx(), 1f)
}

/** A bead of white glass (a liquid slider's thumb), [radius] round [centre], with a soft shadow under it. */
internal fun DrawScope.liquidBead(centre: Offset, radius: Float) {
    drawIntoCanvas { c ->
        val paint = Paint().asFrameworkPaint().apply {
            isAntiAlias = true
            color = Pc.LiquidShadow.toArgb()
            maskFilter = BlurMaskFilter(BeadShadowBlur.toPx(), BlurMaskFilter.Blur.NORMAL)
        }
        c.nativeCanvas.drawCircle(centre.x, centre.y + BeadShadowDrop.toPx(), radius, paint)
    }
    drawCircle(Pc.LiquidBead, radius, centre)
    val rim = LiquidRimWidth.toPx()
    drawCircle(Pc.LiquidBeadRim, radius - rim / 2f, centre, style = Stroke(rim))
}

/** A light around the rim [width] wide, at [share] of its full strength. */
private fun DrawScope.rim(r: Float, width: Float, share: Float) {
    // by the angle round its middle, whatever the shape's proportions: the light from the top left
    // (at 225°, clockwise from the right), the fainter one opposite (45°)
    val light = Pc.LiquidLight
    val floor = light.copy(alpha = Pc.LiquidFloor * share)
    drawRoundRect(
        Brush.sweepGradient(
            0f to floor,
            0.125f to light.copy(alpha = Pc.LiquidCounter * share),
            0.3f to floor,
            0.5f to floor,
            0.625f to light.copy(alpha = Pc.LiquidKey * share),
            0.8f to floor,
            1f to floor,
            center = Offset(size.width / 2f, size.height / 2f),
        ),
        topLeft = Offset(width / 2f, width / 2f),
        size = Size(size.width - width, size.height - width),
        cornerRadius = CornerRadius((r - width / 2f).coerceAtLeast(0f)),
        style = Stroke(width),
    )
}

private val LiquidShadowBlur = 14.dp
private val LiquidShadowDrop = 4.dp
private val BeadShadowBlur = 5.dp
private val BeadShadowDrop = 2.dp
private val LiquidRimWidth = 1.dp
private val LiquidGlowWidth = 4.dp

/** The broad soft light inside the rim, of the thin line's strength. */
private const val GLOW_SHARE = 0.22f
