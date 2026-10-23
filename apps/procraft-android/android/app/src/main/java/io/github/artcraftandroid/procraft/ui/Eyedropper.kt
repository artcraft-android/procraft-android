package io.github.artcraftandroid.procraft.ui

import android.graphics.BlurMaskFilter
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.Paint
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.PickState
import kotlin.math.roundToInt

private val LoupeOuter = 69.dp
private val LoupeInner = 53.5.dp
private val LoupeLift = 41.dp

/** The bar under the top bar while the eyedropper is open. */
@Composable
fun PickingBar(modifier: Modifier = Modifier) {
    Box(modifier.fillMaxWidth().height(24.dp).background(Pc.MessageBar), contentAlignment = Alignment.Center) {
        Text("Picking color from canvas", color = Pc.MessageText, fontSize = 14.sp)
    }
}

/**
 * The eyedropper's loupe, centred [LoupeLift] above the finger: a ring whose top half is the colour
 * under the finger and bottom half the current colour, around what is shown there at twice the size.
 */
@Composable
fun Loupe(pick: PickState, current: Color) {
    val density = LocalDensity.current
    val side = with(density) { (LoupeOuter * 2 + 12.dp).roundToPx() }
    val cx = pick.x.roundToInt()
    val cy = pick.y.roundToInt() - with(density) { LoupeLift.roundToPx() }
    Canvas(Modifier.offset { IntOffset(cx - side / 2, cy - side / 2) }.size(with(density) { side.toDp() })) {
        val c = center
        val outer = LoupeOuter.toPx()
        val inner = LoupeInner.toPx()
        drawIntoCanvas { canvas ->
            val shadow = Paint().apply {
                color = Color(0x40000000)
                asFrameworkPaint().maskFilter = BlurMaskFilter(3.dp.toPx(), BlurMaskFilter.Blur.NORMAL)
            }
            canvas.drawCircle(c + Offset(0f, 1.5.dp.toPx()), outer, shadow)
        }
        val ringMid = (outer + inner) / 2f
        val ring = Stroke(outer - inner)
        val arcBox = Rect(c.x - ringMid, c.y - ringMid, c.x + ringMid, c.y + ringMid)
        val picked = pick.rgb?.let { CanvasSpace.color(it) } ?: current
        drawArc(picked, 180f, 180f, false, arcBox.topLeft, arcBox.size, style = ring)
        drawArc(current, 0f, 180f, false, arcBox.topLeft, arcBox.size, style = ring)
        pick.image?.let { image ->
            clipPath(Path().apply { addOval(Rect(c, inner)) }) {
                val shown = (inner * 2).roundToInt()
                drawImage(
                    image,
                    dstOffset = IntOffset((c.x - shown / 2f).roundToInt(), (c.y - shown / 2f).roundToInt()),
                    dstSize = IntSize(shown, shown),
                    filterQuality = FilterQuality.None,
                )
            }
        }
        val edge = Stroke(0.5.dp.toPx())
        drawCircle(Pc.LoupeEdge, outer, c, style = edge)
        drawCircle(Pc.LoupeEdge, inner, c, style = edge)
        // the crosshair inverts what is under it
        val arm = Size(2.dp.toPx(), 1.dp.toPx())
        val gap = 1.75.dp.toPx()
        for ((dx, dy) in listOf(1f to 0f, -1f to 0f, 0f to 1f, 0f to -1f)) {
            val size = if (dx != 0f) arm else Size(arm.height, arm.width)
            val x = if (dx > 0f) c.x + gap else if (dx < 0f) c.x - gap - arm.width else c.x - arm.height / 2f
            val y = if (dy > 0f) c.y + gap else if (dy < 0f) c.y - gap - arm.width else c.y - arm.height / 2f
            drawRect(Color.White, Offset(x, y), size, blendMode = BlendMode.Difference)
        }
    }
}
