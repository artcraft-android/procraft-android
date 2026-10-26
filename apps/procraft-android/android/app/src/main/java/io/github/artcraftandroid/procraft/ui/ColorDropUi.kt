package io.github.artcraftandroid.procraft.ui

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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel.DropBand
import kotlin.math.roundToInt

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
    onGloballyPositioned { origin = it.positionInRoot() }.pointerInput(vm) {
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
                    vm.dropDragStart(at, colour?.invoke())
                    var lifted = false
                    while (true) {
                        val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        c.consume()
                        at = origin + c.position
                        if (!c.pressed) {
                            lifted = true
                            break
                        }
                        vm.dropDragMove(at)
                    }
                    // let go on the top bar, it is put back
                    vm.dropDragEnd(at.takeIf { lifted && it.y > Pc.BarHeight.toPx() })
                }
            }
        }
    }
}

/** The colour being dragged out: a flat dot under the finger. */
@Composable
fun DropDot(vm: CanvasViewModel) {
    val (at, rgb) = vm.dropDot ?: return
    Box(
        Modifier.offset { IntOffset((at.x - DotSize.toPx() / 2).roundToInt(), (at.y - DotSize.toPx() / 2).roundToInt()) }
            .size(DotSize).clip(CircleShape).background(CanvasSpace.color(rgb)),
    )
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

/** A tap target without a ripple. */
private fun Modifier.press(onClick: () -> Unit): Modifier = composed {
    clickable(remember { MutableInteractionSource() }, indication = null, onClick = onClick)
}

/** The dragged colour's dot, across. */
private val DotSize = 55.5.dp

/** The band, translucent over the canvas, dark and light. */
private val BandDark = Color(0xD63B3B3B)
private val BandLight = Color(0xD6EEEEEE)

/** The threshold's row under it. */
private val RowDark = Color(0xCC4A4A4A)
private val RowLight = Color(0xCCDCDCDC)
