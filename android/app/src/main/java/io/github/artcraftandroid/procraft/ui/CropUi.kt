package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.roundToInt
import kotlin.math.sin

// light over the dark surround; dark with the light interface
private val FrameLine get() = if (Pc.light) Color(0xCC303030) else Color(0xCCFFFFFF)
private val GridLine get() = if (Pc.light) Color(0x99606060) else Color(0x66C8C8C8)
private val Mark get() = if (Pc.light) Color(0xFF3A3A3A) else Color(0xFFD0D0D0)
// Settings: dark, or with the light interface #dbdedf behind #e4e4e6 groups, #d3d3d5 fields, dark words
private val PopoverFill get() = if (Pc.light) Color(0xF2DBDEDF) else Color(0xF21F1F1F)
private val FieldFill get() = if (Pc.light) Color(0xFFD3D3D5) else Color(0xFF242424)
private val GroupFill get() = if (Pc.light) Color(0xFFE4E4E6) else Color(0xFF2A2A2A)
private val RowLine get() = if (Pc.light) Color(0xFFD6D6D8) else Color(0xFF3A3A3A)
private val SettingsInk get() = if (Pc.light) Color(0xFF3B3B3B) else Color(0xFFD2D2D2)
private val Muted = Color(0xFF8E8E93)

/** Which part of the crop's frame a drag takes. */
private enum class Grab { MOVE, LEFT, TOP, RIGHT, BOTTOM, TOP_LEFT, TOP_RIGHT, BOTTOM_LEFT, BOTTOM_RIGHT }

/**
 * Crop and Resize: the canvas, fitted inside a margin, with a frame over it in quarters whose
 * corners and sides are dragged to change what is kept (inside it, to move it); the header with
 * Cancel, Settings, Reset and Done.
 */
@Composable
fun BoxScope.CropOverlay(vm: CanvasViewModel) {
    val rect = vm.cropRect ?: return
    val density = LocalDensity.current
    val reach = with(density) { 28.dp.toPx() }
    val snapPx = with(density) { 10.dp.toPx() }
    val current by rememberUpdatedState(rect)
    // the frame's corners in the window, turned with the crop
    fun corners(r: Rect): List<Offset> {
        val c = r.center
        val (s, k) = sin(vm.cropAngle) to cos(vm.cropAngle)
        return listOf(Offset(r.left, r.top), Offset(r.right, r.top), Offset(r.right, r.bottom), Offset(r.left, r.bottom)).map { p ->
            val d = p - c
            vm.canvasToWindow(Offset(c.x + d.x * k - d.y * s, c.y + d.x * s + d.y * k))
        }
    }
    Canvas(
        Modifier.fillMaxSize().pointerInput(Unit) {
            awaitEachGesture {
                val down = awaitFirstDown()
                val cs = corners(current)
                val mid = { a: Offset, b: Offset -> (a + b) / 2f }
                val near = { p: Offset -> (p - down.position).getDistance() < reach }
                val grab = when {
                    near(cs[0]) -> Grab.TOP_LEFT
                    near(cs[1]) -> Grab.TOP_RIGHT
                    near(cs[2]) -> Grab.BOTTOM_RIGHT
                    near(cs[3]) -> Grab.BOTTOM_LEFT
                    near(mid(cs[0], cs[3])) -> Grab.LEFT
                    near(mid(cs[0], cs[1])) -> Grab.TOP
                    near(mid(cs[1], cs[2])) -> Grab.RIGHT
                    near(mid(cs[2], cs[3])) -> Grab.BOTTOM
                    else -> Grab.MOVE
                }
                val start = current
                val from = vm.windowToCanvas(down.position) ?: return@awaitEachGesture
                // canvas pixels a few window points cover, for snapping
                val snap = vm.windowToCanvas(down.position + Offset(snapPx, 0f))?.let { (it - from).getDistance() } ?: 0f
                down.consume()
                while (true) {
                    val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                    if (!c.pressed) break
                    c.consume()
                    val at = vm.windowToCanvas(c.position) ?: continue
                    val d = at - from
                    var l = start.left
                    var t = start.top
                    var r = start.right
                    var b = start.bottom
                    when (grab) {
                        Grab.MOVE -> { l += d.x; r += d.x; t += d.y; b += d.y }
                        Grab.LEFT -> l += d.x
                        Grab.RIGHT -> r += d.x
                        Grab.TOP -> t += d.y
                        Grab.BOTTOM -> b += d.y
                        Grab.TOP_LEFT -> { l += d.x; t += d.y }
                        Grab.TOP_RIGHT -> { r += d.x; t += d.y }
                        Grab.BOTTOM_LEFT -> { l += d.x; b += d.y }
                        Grab.BOTTOM_RIGHT -> { r += d.x; b += d.y }
                    }
                    // snapping: the frame's sides to the canvas's sides and middle
                    if (vm.cropSnapping) {
                        val (cw, ch) = vm.canvasSize()
                        fun snapTo(v: Float, targets: List<Float>) = targets.firstOrNull { abs(it - v) < snap } ?: v
                        val xs = listOf(0f, cw / 2f, cw.toFloat())
                        val ys = listOf(0f, ch / 2f, ch.toFloat())
                        if (grab == Grab.MOVE) {
                            val dl = snapTo(l, xs) - l
                            val dr = snapTo(r, xs) - r
                            val dx = if (dl != 0f) dl else dr
                            l += dx; r += dx
                            val dt = snapTo(t, ys) - t
                            val db = snapTo(b, ys) - b
                            val dy = if (dt != 0f) dt else db
                            t += dy; b += dy
                        } else {
                            l = snapTo(l, xs); r = snapTo(r, xs); t = snapTo(t, ys); b = snapTo(b, ys)
                        }
                    }
                    // a locked aspect follows the width
                    if (vm.cropAspectLocked && grab != Grab.MOVE && start.width > 0f) {
                        val h = (r - l) * start.height / start.width
                        if (grab == Grab.TOP || grab == Grab.TOP_LEFT || grab == Grab.TOP_RIGHT) t = b - h else b = t + h
                    }
                    if (r - l >= 1f && b - t >= 1f) vm.changeCropRect(Rect(l, t, r, b))
                }
            }
        },
    ) {
        val cs = corners(rect)
        // quarters
        for (i in 1..3) {
            val f = i / 4f
            drawLine(GridLine, cs[0] + (cs[1] - cs[0]) * f, cs[3] + (cs[2] - cs[3]) * f, 1.dp.toPx())
            drawLine(GridLine, cs[0] + (cs[3] - cs[0]) * f, cs[1] + (cs[2] - cs[1]) * f, 1.dp.toPx())
        }
        val frame = Path().apply {
            moveTo(cs[0].x, cs[0].y)
            cs.drop(1).forEach { lineTo(it.x, it.y) }
            close()
        }
        drawPath(frame, FrameLine, style = Stroke(1.dp.toPx()))
        // small marks at the corners and the sides' middles
        val arm = 10.dp.toPx()
        val w = 3.dp.toPx()
        for (i in 0..3) {
            val p = cs[i]
            val next = cs[(i + 1) % 4]
            val toNext = (next - p).let { it / it.getDistance().coerceAtLeast(1f) }
            val toPrev = (cs[(i + 3) % 4] - p).let { it / it.getDistance().coerceAtLeast(1f) }
            drawLine(Mark, p, p + toNext * arm, w, StrokeCap.Butt)
            drawLine(Mark, p, p + toPrev * arm, w, StrokeCap.Butt)
            val m = (p + next) / 2f
            drawLine(Mark, m - toNext * arm, m + toNext * arm, w, StrokeCap.Butt)
        }
    }
    ToolHeader(
        "Crop and Resize",
        listOf(
            ToolButton("Cancel") { vm.endCrop(false) },
            ToolButton("Settings", chosen = vm.cropSettingsOpen) { vm.cropSettingsOpen = !vm.cropSettingsOpen },
            ToolButton("Reset") { vm.resetCrop() },
        ),
        onDone = { vm.endCrop(true) },
        modifier = Modifier.align(Alignment.TopCenter),
    )
    // 9 from the screen's right edge, its arrow up at Settings (183 from that edge)
    if (vm.cropSettingsOpen) CropSettings(vm, Modifier.align(Alignment.TopEnd).offset(x = (-9).dp, y = 66.dp))
}

/** The settings popover: the size (aspect locked or not), DPI, resampling, snapping, the turn. */
@Composable
private fun CropSettings(vm: CanvasViewModel, modifier: Modifier) {
    Popover(348.dp, 174.5.dp, modifier, fill = PopoverFill, corner = 24.dp) {
        Column(Modifier.padding(10.dp)) {
            Box(Modifier.height(40.dp), contentAlignment = Alignment.CenterStart) {
                Text("Settings", color = SettingsInk, fontSize = 22.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 14.dp))
            }
            val (w, h) = vm.cropSize
            Row(Modifier.padding(top = 10.dp).fillMaxWidth().height(44.dp).padding(horizontal = 2.dp), verticalAlignment = Alignment.CenterVertically) {
                NumberField(w.toString(), "px", Modifier.width(137.dp)) { it.toIntOrNull()?.let { v -> vm.changeCropSize(v, null) } }
                Box(Modifier.padding(horizontal = 8.dp).size(34.dp, 40.dp).clickable { vm.cropAspectLocked = !vm.cropAspectLocked }, contentAlignment = Alignment.Center) {
                    Icon(PcIcons.Link, contentDescription = "Aspect lock", tint = if (vm.cropAspectLocked) Pc.Accent else SettingsInk, modifier = Modifier.size(20.dp))
                }
                NumberField(h.toString(), "px", Modifier.width(137.dp)) { it.toIntOrNull()?.let { v -> vm.changeCropSize(null, v) } }
            }
            Column(Modifier.padding(top = 10.dp).fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(GroupFill)) {
                Row(Modifier.fillMaxWidth().height(44.dp).padding(start = 12.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("DPI", color = SettingsInk, fontSize = 17.sp, modifier = Modifier.weight(1f))
                    NumberField("%.1f".format(vm.cropDpi), null, Modifier.width(80.dp).height(36.dp)) { it.toFloatOrNull()?.let(vm::changeCropDpi) }
                }
                Box(Modifier.fillMaxWidth().height(1.dp).background(RowLine))
                SwitchRow("Resample canvas", vm.cropResample, vm::changeCropResample)
                Box(Modifier.fillMaxWidth().height(1.dp).background(RowLine))
                SwitchRow("Snapping", vm.cropSnapping) { vm.cropSnapping = it }
            }
            Box(Modifier.padding(top = 10.dp).fillMaxWidth().height(64.dp).clip(RoundedCornerShape(14.dp)).background(GroupFill)) {
                RotationSlider(vm.cropAngle) { vm.changeCropAngle(it) }
            }
        }
    }
}

@Composable
private fun SwitchRow(label: String, on: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().height(44.dp).clickable { onChange(!on) }.padding(start = 12.dp, end = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = SettingsInk, fontSize = 17.sp, modifier = Modifier.weight(1f))
        Switch(on, enabled = true)
    }
}

/** A number typed into a capsule, its unit after it; done as it is typed. */
@Composable
private fun NumberField(value: String, unit: String?, modifier: Modifier, onChange: (String) -> Unit) {
    var text by remember(value) { mutableStateOf(value) }
    val style = TextStyle(color = SettingsInk, fontSize = 17.sp, textAlign = TextAlign.Center)
    // as wide as its text (and the cursor), so the number and unit sit together in the middle
    val measurer = rememberTextMeasurer()
    val textWidth = with(LocalDensity.current) { measurer.measure(text, style).size.width.toDp() } + 2.dp
    Row(modifier.height(40.dp).clip(CircleShape).background(FieldFill).padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.Center) {
        BasicTextField(
            text,
            { t ->
                text = t.filter { it.isDigit() || it == '.' }.take(7)
                onChange(text)
            },
            singleLine = true,
            textStyle = style,
            cursorBrush = SolidColor(Color.White),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
            modifier = Modifier.width(textWidth),
        )
        if (unit != null) Text(" $unit", color = SettingsInk, fontSize = 17.sp)
    }
}

/** The turn, -180 to 180 degrees, dragged anywhere along the bar; a tick marks no turn. */
@Composable
private fun RotationSlider(radians: Float, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    val degrees = Math.toDegrees(radians.toDouble()).toFloat()
    Column(Modifier.padding(horizontal = 10.dp, vertical = 8.dp)) {
        Row {
            Text("Rotation", color = SettingsInk, fontSize = 17.sp, modifier = Modifier.weight(1f))
            Text("%.1f°".format(degrees), color = SettingsInk, fontSize = 17.sp)
        }
        Box(
            Modifier.fillMaxWidth().height(22.dp).pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    fun at(x: Float) = Math.toRadians(((x / size.width).coerceIn(0f, 1f) * 360f - 180f).let { if (abs(it) < 2f) 0f else it }.toDouble()).toFloat()
                    change(at(down.position.x))
                    while (true) {
                        val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!c.pressed) break
                        change(at(c.position.x))
                        c.consume()
                    }
                }
            }.drawBehind {
                val y = size.height / 2f
                val h = 3.dp.toPx()
                drawRoundRect(if (Pc.light) Color(0xFFCFCFD1) else Color(0xFF555555), Offset(0f, y - h / 2f), Size(size.width, h), CornerRadius(h / 2f))
                drawLine(Muted, Offset(size.width / 2f, y - 6.dp.toPx()), Offset(size.width / 2f, y + 6.dp.toPx()), 1.dp.toPx())
                val knob = Size(26.dp.toPx(), 15.dp.toPx())
                val x = (((degrees + 180f) / 360f).coerceIn(0f, 1f) * size.width).coerceIn(knob.width / 2f, size.width - knob.width / 2f)
                drawRoundRect(Color.White, Offset(x - knob.width / 2f, y - knob.height / 2f), knob, CornerRadius(knob.height / 2f))
            },
        )
    }
}
