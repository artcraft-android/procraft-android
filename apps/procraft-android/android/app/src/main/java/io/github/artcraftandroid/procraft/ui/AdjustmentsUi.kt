package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.Adjustment
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.ChromaticKind
import io.github.artcraftandroid.procraft.core.GlitchKind
import io.github.artcraftandroid.procraft.core.HalftoneKind
import io.github.artcraftandroid.procraft.core.NoiseKind
import io.github.artcraftandroid.procraft.core.QuickShapeKind
import io.github.artcraftandroid.procraft.core.LiquifyKind
import kotlin.math.roundToInt

// light: the band see-through (241 over white, 228 over the surround), the panel near white
private val ModeBar get() = if (Pc.light) Color(0xBAECECEC) else Color(0xFF5A5A5A)
private val ModeText get() = if (Pc.light) Color(0xFF6A6A6A) else Color(0xFFC5C5C5)
private val HsbPanelFill get() = if (Pc.light) Color(0xF7F1F1F1) else Color(0xF71E1E1E)
private val HsbLabel get() = if (Pc.light) Color(0xFF595959) else Color(0xFF808080)
private val HsbValue get() = if (Pc.light) Color(0xFF363636) else Color(0xFFEDEDED)

/** The adjustments menu hanging from its button, in groups: colour, blurs, effects, painting. */
@Composable
fun AdjustmentsPanel(vm: CanvasViewModel, arrowX: Dp, modifier: Modifier = Modifier) {
    Popover(330.dp, arrowX, modifier) {
        // a 52 header, the title's capitals from 28.5 below the panel's top; the groups 8 into the
        // list under it
        Box(Modifier.fillMaxWidth().height(52.dp).padding(start = 24.dp, top = 22.5.dp)) {
            Text("Adjustments", color = Pc.PanelTitle, fontSize = 24.sp, fontWeight = FontWeight.Bold, maxLines = 1)
        }
        Column(
            Modifier.verticalScroll(rememberScrollState()).padding(start = 10.dp, top = 8.dp, end = 10.dp, bottom = 10.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            for (group in Adjustment.groups) {
                Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(13.dp)).background(Pc.ActionsGroup)) {
                    group.forEachIndexed { i, a ->
                        if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
                        Box(
                            Modifier.fillMaxWidth().height(44.dp).then(if (a.ready) Modifier.tap { vm.startAdjustment(a) } else Modifier)
                                .padding(start = 15.dp),
                            contentAlignment = Alignment.CenterStart,
                        ) {
                            Text(a.label, color = if (a.ready) MenuText else Pc.ActionsDisabled, fontSize = 17.sp, maxLines = 1)
                        }
                    }
                }
            }
        }
    }
}

/**
 * The bar under the top bar while an adjustment is open: its icon and name, its amount when it has
 * one, and the triangle that opens its options; and under it the amount as a thin blue bar.
 */
@Composable
fun AdjustmentBar(vm: CanvasViewModel, modifier: Modifier = Modifier) {
    val a = vm.adjustment ?: return
    Column(modifier.fillMaxWidth()) {
        Row(Modifier.fillMaxWidth().height(32.dp).background(ModeBar), horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(33.dp, 32.dp), contentAlignment = Alignment.Center) {
                // drawn in a 32 frame, shown as it is
                Icon(a.icon(), contentDescription = null, tint = ModeText, modifier = Modifier.requiredSize(32.dp))
            }
            Text(a.label, color = ModeText, fontSize = 17.sp, fontWeight = FontWeight.Bold, maxLines = 1)
            if (a.swiped) {
                Spacer(Modifier.width(3.dp))
                Text("${(vm.adjustAmount * 100).roundToInt()}%", color = ModeText, fontSize = 14.sp, maxLines = 1)
            }
            // Layer or Pencil, for the adjustments that can be painted on
            if (a.paintable) {
                Box(Modifier.size(33.dp, 32.dp).tap { vm.adjustModesOpen = !vm.adjustModesOpen }, contentAlignment = Alignment.Center) {
                    Icon(PcIcons.Disclose, contentDescription = null, tint = ModeText, modifier = Modifier.requiredSize(32.dp))
                }
            }
        }
        if (a.swiped) {
            Box(Modifier.fillMaxWidth(vm.adjustAmount.coerceIn(0f, 1f)).height(4.dp).background(Pc.Accent))
        }
    }
}

private fun Adjustment.icon() = when (this) {
    Adjustment.HSB -> PcIcons.AdjustHsb
    Adjustment.COLOR_BALANCE -> PcIcons.AdjustBalance
    Adjustment.CURVES -> PcIcons.AdjustCurves
    Adjustment.GRADIENT_MAP -> PcIcons.AdjustGradient
    Adjustment.GAUSSIAN_BLUR -> PcIcons.AdjustBlur
    Adjustment.MOTION_BLUR -> PcIcons.AdjustMotion
    Adjustment.PERSPECTIVE_BLUR -> PcIcons.AdjustPerspective
    Adjustment.NOISE -> PcIcons.AdjustNoise
    Adjustment.SHARPEN -> PcIcons.AdjustSharpen
    Adjustment.BLOOM -> PcIcons.AdjustBloom
    Adjustment.GLITCH -> PcIcons.AdjustGlitch
    Adjustment.HALFTONE -> PcIcons.AdjustHalftone
    Adjustment.CHROMATIC_ABERRATION -> PcIcons.AdjustChromatic
    Adjustment.LIQUIFY -> PcIcons.AdjustLiquify
    Adjustment.CLONE -> PcIcons.AdjustClone
}

/**
 * Whether the adjustment toolbars lay their sliders side by side: on a screen wider than tall, as
 * the reference does in landscape (upright they stack).
 */
@Composable
private fun widePanels(): Boolean {
    val screen = androidx.compose.ui.platform.LocalConfiguration.current
    return screen.screenWidthDp > screen.screenHeightDp
}

/** The floating panel of hue, saturation and brightness, each 0..100% with 50% leaving the layer as it was. */
@Composable
fun HsbPanel(vm: CanvasViewModel, modifier: Modifier = Modifier) {
    val (h, s, b) = Triple(vm.hue50, vm.saturation50, vm.brightness50)
    val rows: @Composable (Modifier) -> Unit = { m ->
        HsbRow("Hue", h, Brush.horizontalGradient(HUE_STOPS.map { Color(it) }), { Color.hsv((it * 360f) % 360f, 1f, 1f) }, m) { vm.setHsb(it, s, b) }
        HsbRow("Saturation", s, Brush.horizontalGradient(listOf(Color(0xFF808080), Color(0xFFFF0000))), { lerp(Color(0xFF808080), Color.Red, it) }, m) { vm.setHsb(h, it, b) }
        HsbRow("Brightness", b, Brush.horizontalGradient(listOf(Color.Black, Color.White)), { lerp(Color.Black, Color.White, it) }, m) { vm.setHsb(h, s, it) }
    }
    if (widePanels()) {
        // side by side, 301 wide and 13 apart
        Row(
            modifier.offset(y = (-25).dp).size(961.dp, 95.dp).clip(RoundedCornerShape(33.dp)).background(HsbPanelFill).padding(start = 16.dp, end = 16.dp, top = 24.dp, bottom = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(13.dp),
        ) { rows(Modifier.weight(1f)) }
    } else {
        Column(
            modifier.offset(y = (-25).dp).size(532.dp, 220.dp).clip(RoundedCornerShape(33.dp)).background(HsbPanelFill).padding(horizontal = 26.dp, vertical = 16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) { rows(Modifier.fillMaxWidth()) }
    }
}

private val HUE_STOPS = listOf(0xFFFF0000, 0xFFFFFF00, 0xFF00FF00, 0xFF00FFFF, 0xFF0000FF, 0xFFFF00FF, 0xFFFF0000)

/** A labelled slider over a coloured track, its knob taking the track's colour where it sits. */
@Composable
private fun HsbRow(label: String, value: Float, track: Brush, knobColor: (Float) -> Color, modifier: Modifier, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    Column(modifier.height(55.dp)) {
        Row(Modifier.fillMaxWidth().padding(top = 6.dp)) {
            Text(label, color = HsbLabel, fontSize = 16.sp, maxLines = 1, modifier = Modifier.weight(1f))
            Text("${(value * 100).roundToInt()}%", color = HsbValue, fontSize = 16.sp, maxLines = 1)
        }
        Box(
            Modifier.fillMaxWidth().height(24.dp).pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    change((down.position.x / size.width).coerceIn(0f, 1f))
                    while (true) {
                        val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!c.pressed) break
                        change((c.position.x / size.width).coerceIn(0f, 1f))
                        c.consume()
                    }
                }
            }.drawBehind {
                val y = size.height / 2f
                drawLine(track, Offset(0f, y), Offset(size.width, y), 3.dp.toPx(), cap = StrokeCap.Round)
                val knob = Size(24.dp.toPx(), 14.dp.toPx())
                val x = (size.width * value).coerceIn(knob.width / 2, size.width - knob.width / 2)
                drawRoundRect(Color(0x55000000), Offset(x - knob.width / 2, y - knob.height / 2 + 0.5.dp.toPx()), knob, CornerRadius(knob.height / 2))
                drawRoundRect(knobColor(value), Offset(x - knob.width / 2, y - knob.height / 2), knob, CornerRadius(knob.height / 2))
            },
        )
    }
}

private val PanelButton get() = if (Pc.light) Color(0xFFF8F8F8) else Color(0xFF303030)
private val PanelButtonText get() = if (Pc.light) Color(0xFF515151) else Color(0xFFA6A6A6)
private val GridLine = Color(0xFF8A8A8A)

private val BALANCE_PAIRS = listOf(
    Triple("Cyan", "Red", listOf(Color(0xFF00FFFF), Color(0xFF808080), Color(0xFFFF0000))),
    Triple("Magenta", "Green", listOf(Color(0xFFFF00FF), Color(0xFF808080), Color(0xFF00FF00))),
    Triple("Yellow", "Blue", listOf(Color(0xFFFFFF00), Color(0xFF808080), Color(0xFF0000FF))),
)

private val BALANCE_TONES = listOf("Shadows", "Midtones", "Highlights")

/**
 * The colour balance panel: three sliders between opposite colours, for the tone chosen below them
 * (side by side on a wide screen, the tones then in the menu of the "•••" button after them).
 */
@Composable
fun ColorBalancePanel(vm: CanvasViewModel, modifier: Modifier = Modifier) {
    val tone = vm.balanceTone
    if (widePanels()) {
        Row(
            modifier.offset(y = (-25).dp).size(903.dp, 95.dp).clip(RoundedCornerShape(33.dp)).background(HsbPanelFill).padding(start = 16.dp, top = 24.dp, bottom = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(19.dp),
        ) {
            BALANCE_PAIRS.forEachIndexed { k, (left, right, colours) ->
                BalanceRow(left, right, vm.balance[tone * 3 + k], Brush.horizontalGradient(colours), Modifier.width(251.dp)) { vm.setBalance(k, it) }
            }
            MoreButton("Color Balance") {
                OptionsGroup {
                    BALANCE_TONES.forEachIndexed { i, name ->
                        OptionsChoice(name, tone == i, 40.dp, first = i == 0) { vm.chooseBalanceTone(i) }
                    }
                }
            }
        }
        return
    }
    Column(
        modifier.offset(y = (-25).dp).size(532.dp, 272.dp).clip(RoundedCornerShape(33.dp)).background(HsbPanelFill).padding(horizontal = 26.dp, vertical = 16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        BALANCE_PAIRS.forEachIndexed { k, (left, right, colours) ->
            val value = vm.balance[tone * 3 + k]
            BalanceRow(left, right, value, Brush.horizontalGradient(colours), Modifier.fillMaxWidth()) { vm.setBalance(k, it) }
        }
        Row(Modifier.fillMaxWidth().padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(7.dp)) {
            BALANCE_TONES.forEachIndexed { i, name ->
                val chosen = tone == i
                Box(
                    Modifier.weight(1f).height(40.dp).clip(RoundedCornerShape(20.dp)).background(if (chosen) Pc.Accent else PanelButton).tap { vm.chooseBalanceTone(i) },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(name, color = if (chosen) Color.White else PanelButtonText, fontSize = 15.sp, maxLines = 1)
                }
            }
        }
    }
}

/** A slider between two named colours, its middle leaving the colour as it was. */
@Composable
private fun BalanceRow(left: String, right: String, value: Float, track: Brush, modifier: Modifier, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    Column(modifier.height(55.dp)) {
        Row(Modifier.fillMaxWidth().padding(top = 6.dp)) {
            Text(left, color = MenuText, fontSize = 16.sp, maxLines = 1, modifier = Modifier.weight(1f))
            Text(right, color = HsbValue, fontSize = 16.sp, maxLines = 1)
        }
        Box(
            Modifier.fillMaxWidth().height(24.dp).pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    change((down.position.x / size.width).coerceIn(0f, 1f))
                    while (true) {
                        val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!c.pressed) break
                        change((c.position.x / size.width).coerceIn(0f, 1f))
                        c.consume()
                    }
                }
            }.drawBehind {
                val y = size.height / 2f
                drawLine(track, Offset(0f, y), Offset(size.width, y), 3.dp.toPx(), cap = StrokeCap.Round)
                val knob = Size(24.dp.toPx(), 14.dp.toPx())
                val x = (size.width * value).coerceIn(knob.width / 2, size.width - knob.width / 2)
                drawRoundRect(Color(0x55000000), Offset(x - knob.width / 2, y - knob.height / 2 + 0.5.dp.toPx()), knob, CornerRadius(knob.height / 2))
                drawRoundRect(Color(0xFF7A7A7A), Offset(x - knob.width / 2, y - knob.height / 2), knob, CornerRadius(knob.height / 2))
            },
        )
    }
}

/**
 * The curves panel: the chosen channel's curve over a grid (drag its points; touch elsewhere to add
 * one; tap a point for Delete over it), and the channels.
 */
@Composable
fun CurvesPanel(vm: CanvasViewModel, modifier: Modifier = Modifier) {
    Row(
        modifier.offset(y = (-25).dp).size(584.dp, 183.dp).clip(RoundedCornerShape(33.dp)).background(HsbPanelFill).padding(16.dp),
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        val points = vm.curves[vm.curveChannel]
        // the point whose Delete shows, and the graph's size for placing it
        var tapped by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf<Int?>(null) }
        var graph by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf(androidx.compose.ui.unit.IntSize.Zero) }
        Box(
            Modifier.weight(1f).fillMaxHeight().padding(8.dp).onSizeChanged { graph = it }.pointerInput(vm.curveChannel) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    tapped = null
                    val w = size.width.toFloat()
                    val h = size.height.toFloat()
                    fun toCurve(p: Offset) = Offset((p.x / w).coerceIn(0f, 1f), (1f - p.y / h).coerceIn(0f, 1f))
                    val reach = 22.dp.toPx()
                    val current = vm.curves[vm.curveChannel]
                    var index = current.indexOfFirst { (Offset(it.x * w, (1f - it.y) * h) - down.position).getDistance() <= reach }
                    val added = index < 0
                    if (added) index = vm.addCurvePoint(toCurve(down.position))
                    var moved = false
                    while (true) {
                        val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!c.pressed) break
                        if (!moved && (c.position - down.position).getDistance() < viewConfiguration.touchSlop) continue
                        moved = true
                        index = vm.moveCurvePoint(index, toCurve(c.position))
                        c.consume()
                    }
                    // a tap on a point: Delete over it
                    if (!moved && !added) tapped = index
                }
            }.drawBehind {
                val w = size.width
                val h = size.height
                for (k in 0..4) {
                    drawLine(GridLine, Offset(w * k / 4f, 0f), Offset(w * k / 4f, h), 1.dp.toPx())
                    drawLine(GridLine, Offset(0f, h * k / 4f), Offset(w, h * k / 4f), 1.dp.toPx())
                }
                val curve = vm.curveShape(vm.curveChannel)
                for (i in 1 until curve.size) {
                    drawLine(CurveInk, Offset(curve[i - 1].x * w, (1f - curve[i - 1].y) * h), Offset(curve[i].x * w, (1f - curve[i].y) * h), 1.5.dp.toPx())
                }
                for (p in points) node(Offset(p.x * w, (1f - p.y) * h), CurveNode, 1f)
            },
        ) {
            tapped?.let { i -> points.getOrNull(i) }?.let { p ->
                // a pill 78 x 44 over the point, its bottom 14 above it
                val at = with(androidx.compose.ui.platform.LocalDensity.current) {
                    androidx.compose.ui.unit.IntOffset(
                        (p.x * graph.width - 39.dp.toPx()).roundToInt(),
                        ((1f - p.y) * graph.height - (14 + 44).dp.toPx()).roundToInt(),
                    )
                }
                PcPopup(offset = at, onDismissRequest = { tapped = null }, properties = androidx.compose.ui.window.PopupProperties(focusable = true, clippingEnabled = false)) {
                    Box(
                        Modifier.size(78.dp, 44.dp).clip(CircleShape).background(CurveMenuFill).tap {
                            tapped?.let(vm::deleteCurvePoint)
                            tapped = null
                        },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text("Delete", color = Color.White, fontSize = 17.sp, maxLines = 1)
                    }
                }
            }
        }
        Column(Modifier.width(111.dp).fillMaxHeight().clip(RoundedCornerShape(13.dp)), verticalArrangement = Arrangement.spacedBy(1.dp)) {
            listOf("Gamma", "Red", "Green", "Blue").forEachIndexed { i, name ->
                val chosen = vm.curveChannel == i
                Box(Modifier.fillMaxWidth().weight(1f).background(if (chosen) Pc.Accent else PanelButton).tap { vm.chooseCurveChannel(i) }, contentAlignment = Alignment.Center) {
                    Text(name, color = if (chosen) Color.White else PanelButtonText, fontSize = 16.sp, maxLines = 1)
                }
            }
        }
    }
}

private val CurveNode = Color(0xFF0171EA)
/** The curve: white, or black with the light interface. */
private val CurveInk get() = if (Pc.light) Color.Black else Color.White
/** The Delete pill over a tapped point. */
private val CurveMenuFill get() = if (Pc.light) Color(0xFFF0F0F0) else Color(0xFF262626)

/**
 * The gradient map panel: the gradient library (a tap chooses one, a tap on the chosen one edits
 * it, a hold offers Delete and Duplicate, "+" makes a new one), or the gradient being edited.
 */
@Composable
fun GradientMapPanel(vm: CanvasViewModel, modifier: Modifier = Modifier) {
    val editing = vm.gradientEditing
    // wider on a wide screen, showing more of the library
    val width = if (widePanels()) 835.dp else 544.dp
    Column(modifier.offset(y = (-25).dp).size(width, 159.dp).clip(RoundedCornerShape(33.dp)).background(HsbPanelFill).padding(horizontal = 14.dp, vertical = 12.dp)) {
        Row(Modifier.fillMaxWidth().height(40.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(if (editing != null) "Gradient Map" else "Gradient Library", color = MenuText, fontSize = 20.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.weight(1f).padding(start = 14.dp))
            if (editing != null) {
                Box(Modifier.size(40.dp).clip(androidx.compose.foundation.shape.CircleShape).background(GradientDone).tap { vm.endGradientEdit() }, contentAlignment = Alignment.Center) {
                    Icon(PcIcons.DoneTick, contentDescription = "Done", tint = Color.White, modifier = Modifier.requiredSize(36.dp))
                }
            } else {
                val disc = androidx.compose.foundation.shape.CircleShape
                Box(Modifier.size(40.dp).clip(disc).background(PlusDisc).border(1.dp, PlusRim, disc).tap { vm.newGradient() }, contentAlignment = Alignment.Center) {
                    Icon(PcIcons.Add, contentDescription = "New gradient", tint = PlusGlyph, modifier = Modifier.requiredSize(34.dp))
                }
            }
        }
        if (editing != null) GradientEditor(vm, editing) else GradientCards(vm)
    }
}

/** The library's cards on a grey band: each dark, its name over a strip of it; the chosen one stands taller. */
@Composable
private fun GradientCards(vm: CanvasViewModel) {
    var held by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf<Int?>(null) }
    Row(
        Modifier.fillMaxWidth().padding(top = 11.dp).height(81.dp).clip(RoundedCornerShape(12.dp)).background(GradientBand)
            .horizontalScroll(rememberScrollState()).padding(horizontal = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        vm.gradients.forEachIndexed { i, g ->
            val chosen = vm.gradientIndex == i
            Column(
                Modifier.padding(bottom = 5.5.dp).size(108.dp, if (chosen) 69.dp else 65.dp).clip(RoundedCornerShape(9.dp)).background(GradientCard)
                    .pointerInput(i, chosen) {
                        detectTapGestures(
                            onTap = { if (chosen) vm.editGradient(i) else vm.chooseGradient(i) },
                            onLongPress = { held = i },
                        )
                    },
            ) {
                // a hold: Delete and Duplicate above the card
                LibraryMenu(held == i, androidx.compose.ui.graphics.TransformOrigin(0.5f, 1f), MenuPlace { a, size -> androidx.compose.ui.unit.IntOffset(a.center.x - size.width / 2, a.top - size.height) }, onDismiss = { held = null }, overPanel = true) {
                    MenuItem(PcIcons.MenuTrash, "Delete", Pc.MenuDanger) {
                        held = null
                        vm.deleteGradient(i)
                    }
                    MenuItem(PcIcons.MenuDuplicate, "Duplicate") {
                        held = null
                        vm.duplicateGradient(i)
                    }
                }
                Spacer(Modifier.weight(1f))
                Text(g.name, color = if (chosen) Color.White else GradientName, fontSize = 12.sp, maxLines = 1, modifier = Modifier.padding(start = 11.dp, bottom = 10.dp))
                Box(Modifier.fillMaxWidth().height(13.dp).background(gradientBrush(g)))
            }
        }
    }
}

/** A gradient's colours left to right, over [from]..[to] of the width (0..1). */
private fun gradientBrush(g: GradientPreset, from: Float = 0f, to: Float = 1f): Brush {
    val stops = g.stops.sortedBy { it.first }.map { (p, c) -> (from + p * (to - from)) to Color(c) }
    return if (stops.size >= 2) Brush.horizontalGradient(*stops.toTypedArray()) else androidx.compose.ui.graphics.SolidColor(stops.firstOrNull()?.second ?: Color.Black)
}

private val StopSize = 42.dp
/** Room round a popover shown in a popup of its own, for its shadow. */
private val PopupShadowRoom = 56.dp

/**
 * The gradient being edited: its name to rename, over its bar, with a disc for each stop. A tap on
 * the bar adds a stop; on a stop, its colours open above it. A stop drags along the bar; a hold
 * offers Delete.
 */
@Composable
private fun GradientEditor(vm: CanvasViewModel, index: Int) {
    val g = vm.gradients.getOrNull(index) ?: return
    val stops by rememberUpdatedState(g.stops)
    val density = androidx.compose.ui.platform.LocalDensity.current
    var colouring by androidx.compose.runtime.remember(index) { androidx.compose.runtime.mutableStateOf<Int?>(null) }
    var holding by androidx.compose.runtime.remember(index) { androidx.compose.runtime.mutableStateOf<Int?>(null) }
    var barWidth by androidx.compose.runtime.remember { androidx.compose.runtime.mutableFloatStateOf(1f) }
    // where the bar is in the window, for what opens over a stop
    var barAt by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf(Offset.Zero) }
    val r = with(density) { StopSize.toPx() / 2f }
    // a stop's centre along the bar, and the stop under a point
    fun xOf(p: Float) = r + p * (barWidth - 2f * r)
    fun positionAt(x: Float) = ((x - r) / (barWidth - 2f * r).coerceAtLeast(1f)).coerceIn(0f, 1f)
    fun stopAt(at: Offset): Int? = stops.indices.filter { kotlin.math.abs(xOf(stops[it].first) - at.x) <= r }.minByOrNull { kotlin.math.abs(xOf(stops[it].first) - at.x) }
    Column(Modifier.padding(top = 11.dp).fillMaxWidth().height(81.dp).clip(RoundedCornerShape(12.dp)).background(EditorCard)) {
        var name by androidx.compose.runtime.remember(index) { androidx.compose.runtime.mutableStateOf(g.name) }
        androidx.compose.foundation.text.BasicTextField(
            name,
            { name = it.take(64); vm.renameGradient(name) },
            singleLine = true,
            textStyle = androidx.compose.ui.text.TextStyle(color = EditorName, fontSize = 14.sp),
            cursorBrush = androidx.compose.ui.graphics.SolidColor(Pc.Accent),
            modifier = Modifier.padding(start = 14.dp, top = 8.dp, end = 14.dp).fillMaxWidth().height(28.dp),
        )
        Spacer(Modifier.weight(1f))
        Box(
            Modifier.fillMaxWidth().height(44.dp)
                .then(
                    Modifier.onGloballyPositioned {
                        barWidth = it.size.width.toFloat()
                        barAt = it.positionInWindow()
                    },
                )
                .drawBehind { drawRect(gradientBrush(g, r / size.width, 1f - r / size.width)) }
                .pointerInput(index) {
                    detectTapGestures(
                        onTap = { at ->
                            val hit = stopAt(at)
                            if (hit != null) colouring = hit else vm.addGradientStop(positionAt(at.x))
                        },
                        onLongPress = { at -> holding = stopAt(at) },
                    )
                }
                .pointerInput(index) {
                    var dragged: Int? = null
                    detectDragGestures(
                        onDragStart = { at -> dragged = stopAt(at) },
                        onDrag = { change, _ ->
                            dragged?.let {
                                change.consume()
                                vm.moveGradientStop(it, positionAt(change.position.x))
                            }
                        },
                        onDragEnd = { dragged = null },
                        onDragCancel = { dragged = null },
                    )
                },
        ) {
            androidx.compose.foundation.Canvas(Modifier.fillMaxSize()) {
                val ring = 2.dp.toPx()
                for ((p, c) in stops) {
                    val centre = Offset(xOf(p), size.height / 2f)
                    val fill = Color(c)
                    // the ring stands out from what it rings: white round a dark colour, black round a light one
                    val light = 0.299f * fill.red + 0.587f * fill.green + 0.114f * fill.blue > 0.45f
                    drawCircle(if (light) Color.Black else Color.White, r, centre)
                    drawCircle(fill, r - ring, centre)
                }
            }
            colouring?.let { i ->
                val rgb = (stops.getOrNull(i)?.second ?: 0xFF000000).toInt() and 0xFFFFFF
                val held = androidx.compose.runtime.remember(i) { HeldColor(rgb) { vm.setGradientStopColor(i, it) } }
                // over the stop, its arrow at the stop's top, kept inside the window
                val window = androidx.compose.ui.platform.LocalWindowInfo.current.containerSize
                val width = with(density) { CompactColorsWidth.toPx() }
                val centre = barAt.x + xOf(stops.getOrNull(i)?.first ?: 0f)
                val left = (centre - width / 2f).coerceIn(0f, (window.width - width).coerceAtLeast(0f))
                val tip = barAt.y + with(density) { (22.dp - StopSize / 2).toPx() }
                // room round it for its shadow to fade out in
                val room = with(density) { PopupShadowRoom.toPx() }
                PcPopup(
                    popupPositionProvider = object : androidx.compose.ui.window.PopupPositionProvider {
                        override fun calculatePosition(
                            anchorBounds: androidx.compose.ui.unit.IntRect,
                            windowSize: androidx.compose.ui.unit.IntSize,
                            layoutDirection: androidx.compose.ui.unit.LayoutDirection,
                            popupContentSize: androidx.compose.ui.unit.IntSize,
                        ) = androidx.compose.ui.unit.IntOffset((left - room).roundToInt(), (tip + room - popupContentSize.height).roundToInt())
                    },
                    onDismissRequest = { colouring = null },
                    properties = androidx.compose.ui.window.PopupProperties(focusable = true, clippingEnabled = false),
                ) {
                    CompactColors(vm, held, arrowX = with(density) { (centre - left).toDp() }, modifier = Modifier.padding(PopupShadowRoom), side = ArrowSide.BOTTOM)
                }
            }
            holding?.let { i ->
                val stopX = xOf(stops.getOrNull(i)?.first ?: 0f)
                LibraryMenu(
                    true,
                    androidx.compose.ui.graphics.TransformOrigin(0.5f, 1f),
                    MenuPlace { a, size -> androidx.compose.ui.unit.IntOffset(a.left + stopX.roundToInt() - size.width / 2, a.top + with(density) { (22.dp - StopSize / 2).roundToPx() } - size.height) },
                    onDismiss = { holding = null },
                    overPanel = true,
                ) {
                    MenuItem(PcIcons.MenuTrash, "Delete", Pc.MenuDanger) {
                        holding = null
                        vm.deleteGradientStop(i)
                    }
                }
            }
        }
    }
}

private val GradientBand get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0xFF535353)
private val GradientDone = Color(0xFF0190FF)
private val PlusDisc get() = if (Pc.light) Color(0xFFFBFBFB) else Color(0xFF1B1B1B)
private val PlusRim get() = if (Pc.light) Color(0xFFE1E1E1) else Color(0xFF373737)
private val PlusGlyph get() = if (Pc.light) Color(0xFF595959) else Color(0xFFAAAAAA)
private val EditorCard get() = if (Pc.light) Color(0xFFEEEEEE) else Color(0xFF383838)
private val EditorName get() = if (Pc.light) Color(0xFF3A3A3A) else Color(0xFFC5C5C5)
private val GradientCard get() = if (Pc.light) Color(0xFFEEEEEE) else Color(0xFF282828)
private val GradientName get() = if (Pc.light) Color(0xFF5D5D5D) else Color(0xFFC1C1C1)

/** A gradient to map by: its name and colour stops (position, 0xAARRGGBB), in any order. */
data class GradientPreset(val name: String, val stops: List<Pair<Float, Long>>)

/** The colour gradient [g] has at [p] (0..1): its stops' colours mixed between them. */
fun gradientColorAt(g: GradientPreset, p: Float): Long {
    val stops = g.stops.sortedBy { it.first }
    val first = stops.firstOrNull() ?: return 0xFF000000
    if (p <= first.first) return first.second
    for ((a, b) in stops.zipWithNext()) {
        if (p <= b.first) {
            val t = if (b.first > a.first) (p - a.first) / (b.first - a.first) else 1f
            fun ch(c: Long, shift: Int) = ((c shr shift) and 0xFF).toFloat()
            fun mix(shift: Int) = (ch(a.second, shift) + (ch(b.second, shift) - ch(a.second, shift)) * t).toInt().coerceIn(0, 255).toLong() shl shift
            return 0xFF000000 or mix(16) or mix(8) or mix(0)
        }
    }
    return stops.last().second
}

/** ProCraft's own gradients. */
val GRADIENTS = listOf(
    GradientPreset("Ember", listOf(0f to 0xFF140A1E, 0.45f to 0xFF9C2A1F, 0.8f to 0xFFF29B38, 1f to 0xFFFFF4D6)),
    GradientPreset("Glacier", listOf(0f to 0xFF0B1A33, 0.5f to 0xFF3F7FB5, 1f to 0xFFE8F6FF)),
    GradientPreset("Moss", listOf(0f to 0xFF101A0C, 0.5f to 0xFF5C7A34, 1f to 0xFFE9F0C8)),
    GradientPreset("Citrus", listOf(0f to 0xFF2B1640, 0.5f to 0xFFE0466A, 1f to 0xFFFFE15C)),
    GradientPreset("Graphite", listOf(0f to 0xFF000000, 1f to 0xFFFFFFFF)),
    GradientPreset("Lagoon", listOf(0f to 0xFF06202A, 0.55f to 0xFF1B9A8E, 1f to 0xFFF4F1D0)),
)

private val MenuButton get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0xFF3B3B3B)
private val MenuButtonText get() = if (Pc.light) Color(0xFF414141) else Color(0xFFD8D8D8)

/**
 * The menu a tap on the canvas opens during an adjustment: apply above, undo to the left, preview
 * in the middle, cancel to the right and reset below, around the tap.
 */
@Composable
fun AdjustmentMenu(vm: CanvasViewModel) {
    val at = vm.adjustMenuAt ?: return
    val density = androidx.compose.ui.platform.LocalDensity.current
    val centre = with(density) { androidx.compose.ui.unit.DpOffset(at.x.toDp(), at.y.toDp()) }
    val step = 88.dp
    val items = listOf(
        Triple(PcIcons.Apply, "Apply", Offset(0f, -1f)) to { vm.applyAdjustment() },
        Triple(PcIcons.PieUndo, "Undo", Offset(-1f, 0f)) to { vm.undoAdjustment() },
        Triple(PcIcons.Preview, "Preview", Offset(0f, 0f)) to { vm.togglePreview() },
        Triple(PcIcons.CancelAdjust, "Cancel", Offset(1f, 0f)) to { vm.cancelAdjustment() },
        Triple(PcIcons.PieReset, "Reset", Offset(0f, 1f)) to { vm.resetAdjustment() },
    )
    for ((item, action) in items) {
        val (icon, label, place) = item
        Box(
            Modifier.offset(centre.x - 40.dp + step * place.x, centre.y - 40.dp + step * place.y).size(80.dp)
                .clip(RoundedCornerShape(22.dp)).background(MenuButton).tap {
                    vm.adjustMenuAt = null
                    action()
                },
        ) {
            // the icon (a 50 frame) 30 across, centred 33 down; the name from 45 down
            Icon(icon, contentDescription = null, tint = MenuButtonText, modifier = Modifier.align(Alignment.TopCenter).padding(top = 18.dp).requiredSize(30.dp))
            Text(label, color = MenuButtonText, fontSize = 13.5.sp, maxLines = 1, modifier = Modifier.align(Alignment.TopCenter).padding(top = 44.dp))
        }
    }
}

private val LIQUIFY_NAMES = listOf(
    LiquifyKind.PUSH to "Push",
    LiquifyKind.TWIRL_RIGHT to "Twirl Right",
    LiquifyKind.TWIRL_LEFT to "Twirl Left",
    LiquifyKind.PINCH to "Pinch",
    LiquifyKind.EXPAND to "Expand",
    LiquifyKind.CRYSTALS to "Crystals",
    LiquifyKind.EDGE to "Edge",
    LiquifyKind.RECONSTRUCT to "Reconstruct",
)

/** The liquify panel: the brush's size, pressure, distortion and momentum; its way, adjust and reset. */
@Composable
fun LiquifyPanel(vm: CanvasViewModel, screenWidth: Dp, screenHeight: Dp, modifier: Modifier = Modifier) {
    val wide = widePanels()
    if (wide) LiquifyWide(vm, modifier) else LiquifyUpright(vm, modifier)
    // the way's list and the adjust slider open over the panel's buttons
    val panelLeft = (screenWidth - 654.dp) / 2
    val buttonsTop = if (wide) screenHeight - 25.dp - 162.dp + 28.dp else screenHeight - 25.dp - 16.dp - 40.dp
    when (vm.liquifyPopup) {
        1 -> if (!wide) {
            val centre = panelLeft + 16.dp + (654.dp - 32.dp - 16.dp) / 6
            val left = (centre - 130.dp).coerceIn(0.dp, screenWidth - 260.dp)
            MenuPopover(centre - left, left, buttonsTop) {
                MenuTitle("Liquify Mode")
                Spacer(Modifier.height(10.dp))
                Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(13.dp)).background(MenuGroup)) {
                    LIQUIFY_NAMES.forEachIndexed { i, (kind, label) ->
                        if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(MenuLine))
                        val chosen = vm.liquifyKind == kind
                        Row(
                            Modifier.fillMaxWidth().height(44.dp).background(if (chosen) Pc.Accent else Color.Transparent).tap { vm.chooseLiquifyKind(kind) }.padding(start = 16.dp, end = 14.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            MenuLabel(label, if (chosen) Color.White else MenuText, Modifier.weight(1f))
                            if (chosen) Icon(PcIcons.MenuTick, contentDescription = null, tint = Color.White, modifier = Modifier.size(29.dp))
                        }
                    }
                }
            }
        }
        2 -> {
            // over the Adjust button: the middle one upright, the ninth of the row when wide
            val centre = if (wide) (screenWidth - 1093.dp) / 2 + 86.dp + 94.dp * 8 + 37.5.dp else screenWidth / 2
            val left = (centre - 130.dp).coerceIn(0.dp, screenWidth - 260.dp)
            MenuPopover(centre - left, left, buttonsTop) {
                MenuTitle("Adjust")
                Spacer(Modifier.height(4.dp))
                Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(13.dp)).background(MenuGroup).padding(vertical = 8.dp)) {
                    SliderRow("Amount", vm.liquifyAmount, "${(vm.liquifyAmount * 100).roundToInt()}%") { vm.changeLiquifyAmount(it) }
                }
            }
        }
    }
}

private val LiquifyIcons = mapOf(
    LiquifyKind.PUSH to { PcIcons.LiquifyPush },
    LiquifyKind.TWIRL_RIGHT to { PcIcons.LiquifyTwirlRight },
    LiquifyKind.TWIRL_LEFT to { PcIcons.LiquifyTwirlLeft },
    LiquifyKind.PINCH to { PcIcons.LiquifyPinch },
    LiquifyKind.EXPAND to { PcIcons.LiquifyExpand },
    LiquifyKind.CRYSTALS to { PcIcons.LiquifyCrystals },
    LiquifyKind.EDGE to { PcIcons.LiquifyEdge },
    LiquifyKind.RECONSTRUCT to { PcIcons.LiquifyReconstruct },
)
private val LiquifyIdle get() = if (Pc.light) Color(0xFF414141) else Color(0xFFAAAAAA)
private val LiquifyLabel get() = if (Pc.light) Color(0xFF414141) else Color(0xFFA3A3A3)
private val LiquifyDim get() = if (Pc.light) Color(0xFF989898) else Color(0xFF575757)

/**
 * Liquify on a wide screen (1093 x 162): its ways, Adjust and Reset as a row of icon buttons (75
 * wide, 94 apart; the chosen way's icon blue), and the four sliders side by side under them.
 */
@Composable
private fun LiquifyWide(vm: CanvasViewModel, modifier: Modifier) {
    val percent = { v: Float, none: String? -> if (v >= 1f) "Max" else if (v <= 0f && none != null) none else "${(v * 100).roundToInt()}%" }
    Column(
        modifier.offset(y = (-25).dp).size(1093.dp, 162.dp).clip(RoundedCornerShape(33.dp)).background(HsbPanelFill).padding(start = 16.dp, end = 16.dp, top = 28.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        val canAdjust = vm.liquified
        Row(Modifier.height(60.dp), horizontalArrangement = Arrangement.spacedBy(19.dp)) {
            for ((kind, label) in LIQUIFY_NAMES) {
                val chosen = vm.liquifyKind == kind
                LiquifyButton(LiquifyIcons[kind]?.invoke() ?: PcIcons.AdjustLiquify, label, if (chosen) Pc.Accent else LiquifyIdle, LiquifyLabel, 24.dp) { vm.chooseLiquifyKind(kind) }
            }
            val ink = if (canAdjust) LiquifyIdle else LiquifyDim
            val words = if (canAdjust) LiquifyLabel else LiquifyDim
            LiquifyButton(PcIcons.AdjustKnob, "Adjust", ink, words, 39.5.dp, canAdjust) { vm.liquifyPopup = if (vm.liquifyPopup == 2) 0 else 2 }
            LiquifyButton(PcIcons.PieReset, "Reset", ink, words, 23.dp, canAdjust) { vm.resetLiquify() }
        }
        Spacer(Modifier.height(8.dp))
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(11.dp)) {
            Box(Modifier.weight(1f)) { SliderRow("Size", vm.liquifySize, percent(vm.liquifySize, null)) { vm.liquifySize = it } }
            Box(Modifier.weight(1f)) { SliderRow("Pressure", vm.liquifyPressure, percent(vm.liquifyPressure, null)) { vm.liquifyPressure = it } }
            Box(Modifier.weight(1f)) { SliderRow("Distortion", vm.liquifyDistortion, percent(vm.liquifyDistortion, "None")) { vm.liquifyDistortion = it } }
            Box(Modifier.weight(1f)) { SliderRow("Momentum", vm.liquifyMomentum, percent(vm.liquifyMomentum, "None")) { vm.liquifyMomentum = it } }
        }
    }
}

/** One of wide Liquify's buttons: its icon ([iconSize] across, centred 9 down) over its name. */
@Composable
private fun LiquifyButton(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, ink: Color, words: Color, iconSize: Dp, enabled: Boolean = true, onClick: () -> Unit) {
    Box(Modifier.size(75.dp, 60.dp).then(if (enabled) Modifier.tap(onClick = onClick) else Modifier)) {
        Box(Modifier.align(Alignment.TopCenter).offset(y = 9.dp - 12.dp).size(24.dp), contentAlignment = Alignment.Center) {
            Icon(icon, contentDescription = label, tint = ink, modifier = Modifier.requiredSize(iconSize))
        }
        Text(label, color = words, fontSize = 13.sp, maxLines = 1, modifier = Modifier.align(Alignment.TopCenter).padding(top = 22.dp))
    }
}

/** Liquify upright: two rows of two sliders, then the way, Adjust and Reset. */
@Composable
private fun LiquifyUpright(vm: CanvasViewModel, modifier: Modifier) {
    Column(
        modifier.offset(y = (-25).dp).size(654.dp, 200.dp).clip(RoundedCornerShape(33.dp)).background(HsbPanelFill).padding(16.dp),
        verticalArrangement = Arrangement.SpaceBetween,
    ) {
        val percent = { v: Float, none: String? -> if (v >= 1f) "Max" else if (v <= 0f && none != null) none else "${(v * 100).roundToInt()}%" }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(20.dp)) {
            Box(Modifier.weight(1f)) { SliderRow("Size", vm.liquifySize, percent(vm.liquifySize, null)) { vm.liquifySize = it } }
            Box(Modifier.weight(1f)) { SliderRow("Pressure", vm.liquifyPressure, percent(vm.liquifyPressure, null)) { vm.liquifyPressure = it } }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(20.dp)) {
            Box(Modifier.weight(1f)) { SliderRow("Distortion", vm.liquifyDistortion, percent(vm.liquifyDistortion, "None")) { vm.liquifyDistortion = it } }
            Box(Modifier.weight(1f)) { SliderRow("Momentum", vm.liquifyMomentum, percent(vm.liquifyMomentum, "None")) { vm.liquifyMomentum = it } }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            val name = LIQUIFY_NAMES.firstOrNull { it.first == vm.liquifyKind }?.second ?: ""
            Box(Modifier.weight(1f).height(40.dp).clip(RoundedCornerShape(20.dp)).background(PanelButton).tap { vm.liquifyPopup = if (vm.liquifyPopup == 1) 0 else 1 }, contentAlignment = Alignment.Center) {
                Text(name, color = MenuText, fontSize = 15.sp, maxLines = 1)
            }
            val canAdjust = vm.liquified
            Row(
                Modifier.weight(1f).height(40.dp).clip(RoundedCornerShape(20.dp)).background(PanelButton.copy(alpha = if (canAdjust) 1f else 0.5f))
                    .then(if (canAdjust) Modifier.tap { vm.liquifyPopup = if (vm.liquifyPopup == 2) 0 else 2 } else Modifier),
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                val tint = if (canAdjust) MenuText else Color(0xFF575757)
                // a 50 frame shown 39.5 across, the name 2 after
                Icon(PcIcons.AdjustKnob, contentDescription = null, tint = tint, modifier = Modifier.requiredSize(39.5.dp))
                Spacer(Modifier.width(2.dp))
                Text("Adjust", color = tint, fontSize = 15.sp, maxLines = 1)
            }
            Row(
                Modifier.weight(1f).height(40.dp).clip(RoundedCornerShape(20.dp)).background(PanelButton.copy(alpha = if (canAdjust) 1f else 0.5f))
                    .then(if (canAdjust) Modifier.tap { vm.resetLiquify() } else Modifier),
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                val tint = if (canAdjust) MenuText else Color(0xFF575757)
                // the pie's reset, 23 across here, the name 2.5 after
                Icon(PcIcons.PieReset, contentDescription = null, tint = tint, modifier = Modifier.requiredSize(23.dp))
                Spacer(Modifier.width(2.5.dp))
                Text("Reset", color = tint, fontSize = 15.sp, maxLines = 1)
            }
        }
    }
}

private val ModesFill get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0xFF7B7B7B)

/** The Layer / Pencil choice under the mode bar: the adjustment changes the layer, or is painted on. */
@Composable
fun AdjustModes(vm: CanvasViewModel, modifier: Modifier = Modifier) {
    if (!vm.adjustModesOpen) return
    Row(
        modifier.size(225.dp, 36.dp).clip(RoundedCornerShape(18.dp)).background(ModesFill).padding(2.dp),
        horizontalArrangement = Arrangement.spacedBy(1.dp),
    ) {
        for ((pencil, item) in listOf(false to (PcIcons.ModeLayer to "Layer"), true to (PcIcons.ModePencil to "Pencil"))) {
            val (icon, label) = item
            val chosen = vm.adjustPencil == pencil
            Row(
                Modifier.size(110.dp, 32.dp).clip(RoundedCornerShape(16.dp)).background(if (chosen) Pc.Accent else Color.Transparent)
                    .tap { vm.chooseAdjustMode(pencil) }.padding(start = 24.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                // drawn in 27 x 32 (the pencil 24 x 32) frames, shown as they are
                Icon(icon, contentDescription = null, tint = Color.White, modifier = Modifier.requiredSize(if (pencil) 24.dp else 27.dp, 32.dp))
                Spacer(Modifier.width(1.5.dp))
                Text(label, color = Color.White, fontSize = 14.sp, maxLines = 1)
            }
        }
    }
}

/** The clone source: a dark ring with a light edge, so it shows on any colour. */
@Composable
fun ClonePuck(vm: CanvasViewModel) {
    val puck = vm.clonePuck ?: return
    val at = vm.canvasToWindow(puck)
    androidx.compose.foundation.Canvas(Modifier.fillMaxSize()) {
        val r = CanvasViewModel.CLONE_PUCK_RADIUS_DP.dp.toPx()
        drawCircle(Color.White, r, at, style = androidx.compose.ui.graphics.drawscope.Stroke(4.dp.toPx()))
        drawCircle(Color.Black, r, at, style = androidx.compose.ui.graphics.drawscope.Stroke(2.dp.toPx()))
    }
}

private val KindsFill get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0xFF535353)
private val KindsEdge get() = if (Pc.light) Color(0xFFFAFAFA) else Color(0xFF747474)
private val GlitchTrack get() = if (Pc.light) Color(0xFFDDDDDD) else Color(0xFF2E2E2E)
private val GlitchTick get() = if (Pc.light) Color(0xFFC8C8C8) else Color(0xFF4A4A4A)

private val GLITCH_KINDS = listOf(
    Triple(GlitchKind.ARTIFACT, "Artifact", 111.dp),
    Triple(GlitchKind.WAVE, "Wave", 99.dp),
    Triple(GlitchKind.SIGNAL, "Signal", 104.dp),
    Triple(GlitchKind.DIVERGE, "Diverge", 113.dp),
)

/** The glitch toolbar: its four kinds above the kind's sliders, two to a row. */
@Composable
fun GlitchPanel(vm: CanvasViewModel, modifier: Modifier = Modifier) {
    val wide = widePanels()
    Column(
        modifier.offset(y = (-25).dp).size(if (wide) 973.dp else 640.dp, if (wide) 120.dp else 167.dp).clip(RoundedCornerShape(33.dp)).background(HsbPanelFill).padding(top = 8.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        KindsSwitch(466.dp, GLITCH_KINDS.map { (kind, label, width) -> Kind(kind.icon(), label, width) }, GLITCH_KINDS.indexOfFirst { it.first == vm.glitchKind }) {
            vm.chooseGlitchKind(GLITCH_KINDS[it].first)
        }
        val names = when (vm.glitchKind) {
            GlitchKind.ARTIFACT, GlitchKind.SIGNAL -> listOf("Amount", "Block Size", "Zoom")
            GlitchKind.WAVE -> listOf("Amplitude", "Frequency", "Zoom")
            GlitchKind.DIVERGE -> listOf("Red shift", "Green shift", "Blue shift", "Zoom")
        }
        val values = vm.glitchValues[vm.glitchKind].orEmpty()
        Spacer(Modifier.height(12.dp))
        // on a wide screen all in one row, 19 apart
        if (wide) {
            val each = (941.dp - 19.dp * (names.size - 1)) / names.size
            Row(Modifier.width(941.dp).height(54.dp), horizontalArrangement = Arrangement.spacedBy(19.dp)) {
                for (k in names.indices) {
                    val both = vm.glitchKind == GlitchKind.DIVERGE && k < 3
                    val v = values.getOrElse(k) { 0f }
                    OptionSlider(names[k], v, "${(v * 100).roundToInt()}%", each, both) { vm.setGlitchValue(k, it) }
                }
            }
            return@Column
        }
        for (row in names.indices.chunked(2)) {
            Row(Modifier.width(608.dp).height(54.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (k in row) {
                    // diverge's shifts go both ways from the middle
                    val both = vm.glitchKind == GlitchKind.DIVERGE && k < 3
                    val v = values.getOrElse(k) { 0f }
                    OptionSlider(names[k], v, "${(v * 100).roundToInt()}%", 300.dp, both) { vm.setGlitchValue(k, it) }
                }
            }
        }
    }
}

/** One choice in a [KindsSwitch]: its icon, label and width. */
internal class Kind(val icon: androidx.compose.ui.graphics.vector.ImageVector, val label: String, val width: Dp)

/** A toolbar's row of kinds, [width] wide, the [chosen] one blue. */
@Composable
internal fun KindsSwitch(width: Dp, kinds: List<Kind>, chosen: Int, onChoose: (Int) -> Unit) {
    Row(
        Modifier.size(width, 34.dp).clip(RoundedCornerShape(17.dp)).background(KindsFill).border(1.dp, KindsEdge, RoundedCornerShape(17.dp)).padding(2.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        kinds.forEachIndexed { i, kind ->
            val on = i == chosen
            Row(
                Modifier.size(kind.width, 30.dp).clip(RoundedCornerShape(15.dp)).background(if (on) Pc.Accent else Color.Transparent).tap { onChoose(i) },
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                // drawn in a 50 frame, shown 30 across, the name 2 after
                Icon(kind.icon, contentDescription = null, tint = if (on) Color.White else ModesText, modifier = Modifier.requiredSize(30.dp))
                Spacer(Modifier.width(2.dp))
                Text(kind.label, color = if (on) Color.White else ModesText, fontSize = 14.sp, maxLines = 1)
            }
        }
    }
}

/** A toolbar's floating panel, [width] × [height], 25 above the bottom. */
@Composable
internal fun OptionsPanel(width: Dp, height: Dp, modifier: Modifier, top: Dp, content: @Composable androidx.compose.foundation.layout.ColumnScope.() -> Unit) {
    Column(
        modifier.offset(y = (-25).dp).size(width, height).clip(RoundedCornerShape(33.dp)).background(HsbPanelFill).padding(top = top),
        horizontalAlignment = Alignment.CenterHorizontally,
        content = content,
    )
}

/** A slider's value as shown: None at nothing, Max at all ([max]), else a percentage. */
internal fun shown(v: Float, max: Boolean = false) = when {
    v <= 0f -> "None"
    max && v >= 1f -> "Max"
    else -> "${(v * 100).roundToInt()}%"
}

/**
 * A labelled toolbar slider [width] wide: [both] ways runs −1..1 with its fill from the middle, else
 * 0..1. A [chip] value sits on a rounded chip (one that can be typed).
 */
@Composable
internal fun OptionSlider(label: String, value: Float, shown: String, width: Dp, both: Boolean = false, labelColor: Color = HsbLabel, chip: Boolean = false, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    Column(Modifier.width(width).height(54.dp)) {
        Row(Modifier.fillMaxWidth().padding(start = 10.dp, end = 15.dp, top = 6.dp)) {
            Text(label, color = labelColor, fontSize = 16.sp, maxLines = 1, modifier = Modifier.weight(1f))
            if (chip) {
                Box(Modifier.offset(x = 8.dp, y = (-4).dp).clip(RoundedCornerShape(14.dp)).background(ValueChip).padding(horizontal = 12.dp, vertical = 3.dp)) {
                    Text(shown, color = HsbValue, fontSize = 16.sp, maxLines = 1)
                }
            } else {
                Text(shown, color = HsbValue, fontSize = 16.sp, maxLines = 1)
            }
        }
        Box(
            Modifier.fillMaxWidth().height(24.dp).padding(start = 10.dp, end = 11.dp).pointerInput(both) {
                fun at(x: Float): Float {
                    val t = (x / size.width).coerceIn(0f, 1f)
                    return if (both) t * 2f - 1f else t
                }
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    while (true) {
                        val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!c.pressed) break
                        change(at(c.position.x))
                        c.consume()
                    }
                }
            }.drawBehind {
                val y = size.height / 2f
                val t = if (both) (value + 1f) / 2f else value
                val x = size.width * t.coerceIn(0f, 1f)
                val from = if (both) size.width / 2f else 0f
                val line = 2.dp.toPx()
                drawLine(GlitchTrack, Offset(0f, y), Offset(size.width, y), line, cap = StrokeCap.Round)
                drawLine(Pc.Accent, Offset(from, y), Offset(x, y), line, cap = StrokeCap.Round)
                if (both) drawLine(GlitchTick, Offset(from, y - 6.dp.toPx()), Offset(from, y + 6.dp.toPx()), 1.dp.toPx())
                val knob = Size(17.dp.toPx(), 11.dp.toPx())
                val kx = x.coerceIn(knob.width / 2, size.width - knob.width / 2)
                drawRoundRect(Color(0x33000000), Offset(kx - knob.width / 2, y - knob.height / 2 + 0.5.dp.toPx()), knob, CornerRadius(knob.height / 2))
                drawRoundRect(Color.White, Offset(kx - knob.width / 2, y - knob.height / 2), knob, CornerRadius(knob.height / 2))
            },
        )
    }
}

private val ModesText get() = if (Pc.light) Color(0xFF3B3B3B) else Color(0xFFD0D0D0)
private val ValueChip get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0xFF2A2A2A)

private fun GlitchKind.icon() = when (this) {
    GlitchKind.ARTIFACT -> PcIcons.GlitchArtifact
    GlitchKind.WAVE -> PcIcons.GlitchWave
    GlitchKind.SIGNAL -> PcIcons.GlitchSignal
    GlitchKind.DIVERGE -> PcIcons.GlitchDiverge
}

/** The noise toolbar: its three kinds, scale, octaves and turbulence, single or multi channel, added or not. */
@Composable
fun NoisePanel(vm: CanvasViewModel, modifier: Modifier = Modifier) {
    val kinds = listOf(NoiseKind.CLOUDS, NoiseKind.BILLOWS, NoiseKind.RIDGES)
    val switch: @Composable () -> Unit = {
        KindsSwitch(472.dp, listOf(Kind(PcIcons.NoiseClouds, "Clouds", 149.dp), Kind(PcIcons.NoiseBillows, "Billows", 149.dp), Kind(PcIcons.NoiseRidges, "Ridges", 148.dp)), kinds.indexOf(vm.noiseKind)) {
            vm.chooseNoiseKind(kinds[it])
        }
    }
    if (widePanels()) {
        // the sliders side by side, single or multi channel and additive in the "•••" menu
        OptionsPanel(903.dp, 113.dp, modifier, 8.dp) {
            switch()
            Spacer(Modifier.height(8.dp))
            Row(Modifier.fillMaxWidth().padding(start = 16.dp), horizontalArrangement = Arrangement.spacedBy(19.dp)) {
                OptionSlider("Scale", vm.noiseScale, shown(vm.noiseScale), 251.dp, labelColor = ModesText) { vm.setNoiseValue(0, it) }
                OptionSlider("Octaves", vm.noiseOctaves, shown(vm.noiseOctaves), 251.dp, labelColor = ModesText) { vm.setNoiseValue(1, it) }
                OptionSlider("Turbulence", vm.noiseTurbulence, shown(vm.noiseTurbulence), 251.dp, labelColor = ModesText) { vm.setNoiseValue(2, it) }
                MoreButton("Channel") {
                    OptionsGroup {
                        Row(Modifier.fillMaxWidth().height(44.dp).padding(start = 12.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                            Text("Additive", color = MenuText, fontSize = 15.sp, maxLines = 1, modifier = Modifier.weight(1f))
                            Box(Modifier.tap { vm.toggleNoiseAdded() }) { Switch(vm.noiseAdded, enabled = true) }
                        }
                    }
                    Spacer(Modifier.height(12.dp))
                    OptionsGroup {
                        for ((i, pair) in listOf(false to "Single", true to "Multi").withIndex()) {
                            OptionsChoice(pair.second, vm.noiseMulti == pair.first, 44.dp, first = i == 0) { vm.chooseNoiseMulti(pair.first) }
                        }
                    }
                }
            }
        }
        return
    }
    OptionsPanel(532.dp, 292.dp, modifier, 10.dp) {
        switch()
        Spacer(Modifier.height(8.dp))
        Column(Modifier.width(500.dp), verticalArrangement = Arrangement.spacedBy(8.5.dp)) {
            OptionSlider("Scale", vm.noiseScale, shown(vm.noiseScale), 500.dp, labelColor = ModesText) { vm.setNoiseValue(0, it) }
            OptionSlider("Octaves", vm.noiseOctaves, shown(vm.noiseOctaves), 500.dp, labelColor = ModesText) { vm.setNoiseValue(1, it) }
            OptionSlider("Turbulence", vm.noiseTurbulence, shown(vm.noiseTurbulence), 500.dp, labelColor = ModesText) { vm.setNoiseValue(2, it) }
        }
        Spacer(Modifier.height(12.dp))
        Row(Modifier.width(492.dp).height(40.dp), verticalAlignment = Alignment.CenterVertically) {
            for ((multi, label) in listOf(false to "Single", true to "Multi")) {
                val on = vm.noiseMulti == multi
                Box(
                    Modifier.size(160.dp, 40.dp).clip(RoundedCornerShape(20.dp)).background(if (on) Pc.Accent else NoiseButton).tap { vm.chooseNoiseMulti(multi) },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(label, color = if (on) Color.White else ModesText, fontSize = 15.sp, maxLines = 1)
                }
                Spacer(Modifier.width(7.dp))
            }
            Box(Modifier.weight(1f), contentAlignment = Alignment.Center) {
                Text("Additive", color = ModesText, fontSize = 16.sp, maxLines = 1)
            }
            Box(Modifier.tap { vm.toggleNoiseAdded() }) { Switch(vm.noiseAdded, enabled = true) }
        }
    }
}

private val NoiseButton get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0xFF1E1E1E)

/** The bloom toolbar: transition, size and burn. */
@Composable
fun BloomPanel(vm: CanvasViewModel, modifier: Modifier = Modifier) {
    if (widePanels()) {
        // side by side, 301 wide and 39 apart
        OptionsPanel(1013.dp, 95.dp, modifier, 24.dp) {
            Row(Modifier.width(981.dp), horizontalArrangement = Arrangement.spacedBy(39.dp)) {
                OptionSlider("Transition", vm.bloomTransition, shown(vm.bloomTransition, max = true), 301.dp) { vm.setBloomValue(0, it) }
                OptionSlider("Size", vm.bloomSize, shown(vm.bloomSize, max = true), 301.dp) { vm.setBloomValue(1, it) }
                OptionSlider("Burn", vm.bloomBurn, shown(vm.bloomBurn, max = true), 301.dp) { vm.setBloomValue(2, it) }
            }
        }
        return
    }
    OptionsPanel(532.dp, 220.dp, modifier, 24.dp) {
        Column(Modifier.width(500.dp), verticalArrangement = Arrangement.spacedBy(8.5.dp)) {
            OptionSlider("Transition", vm.bloomTransition, shown(vm.bloomTransition, max = true), 500.dp) { vm.setBloomValue(0, it) }
            OptionSlider("Size", vm.bloomSize, shown(vm.bloomSize, max = true), 500.dp) { vm.setBloomValue(1, it) }
            OptionSlider("Burn", vm.bloomBurn, shown(vm.bloomBurn, max = true), 500.dp) { vm.setBloomValue(2, it) }
        }
    }
}

private val MoreFill get() = if (Pc.light) Color(0xFFFAFAFA) else Color(0xFF1C1C1C)
private val MoreEdge get() = if (Pc.light) Color(0xFFE1E1E1) else Color(0xFF3A3A3A)
private val MoreDots get() = if (Pc.light) Color(0xFF595959) else Color(0xFF9A9A9A)
private val OptionsMenuFill get() = if (Pc.light) Color(0xF2F0F0F0) else Color(0xF2262626)
private val OptionsGroupFill get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0xFF1B1B1B)
private val OptionsLine get() = if (Pc.light) Color(0xFFE2E2E2) else Color(0xFF2E2E2E)

/**
 * A toolbar's "•••" button (41 × 40, 3 lower than the sliders' top): it opens a small menu over
 * it, [title] above the [content], 16 above the button; a tap outside closes it.
 */
@Composable
private fun MoreButton(title: String, content: @Composable androidx.compose.foundation.layout.ColumnScope.(close: () -> Unit) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box(
        Modifier.padding(top = 3.dp).size(41.dp, 40.dp).clip(androidx.compose.foundation.shape.CircleShape).background(MoreFill)
            .border(1.dp, MoreEdge, androidx.compose.foundation.shape.CircleShape).tap { open = !open },
        contentAlignment = Alignment.Center,
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            repeat(3) { Box(Modifier.size(6.dp).clip(androidx.compose.foundation.shape.CircleShape).background(MoreDots)) }
        }
        if (open) {
            val lift = with(androidx.compose.ui.platform.LocalDensity.current) { (40.dp + 16.dp).roundToPx() }
            PcPopup(
                alignment = Alignment.BottomCenter,
                offset = androidx.compose.ui.unit.IntOffset(0, -lift),
                onDismissRequest = { open = false },
                properties = androidx.compose.ui.window.PopupProperties(focusable = true),
            ) {
                Column(Modifier.width(260.dp).clip(RoundedCornerShape(26.dp)).background(OptionsMenuFill).padding(start = 10.dp, end = 10.dp, bottom = 10.dp)) {
                    Text(title, color = MenuText, fontSize = 23.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.padding(start = 12.dp, top = 16.dp, bottom = 16.dp))
                    content { open = false }
                }
            }
        }
    }
}

/** Rows in a rounded group of an options menu. */
@Composable
private fun OptionsGroup(rows: @Composable androidx.compose.foundation.layout.ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(OptionsGroupFill), content = rows)
}

/** One choice of an options menu: blue with a tick when [chosen], a line above all but the [first]. */
@Composable
private fun OptionsChoice(label: String, chosen: Boolean, height: Dp, first: Boolean, onChoose: () -> Unit) {
    if (!first) Box(Modifier.fillMaxWidth().height(1.dp).background(OptionsLine))
    Row(
        Modifier.fillMaxWidth().height(height).background(if (chosen) Pc.Accent else Color.Transparent).tap(onClick = onChoose).padding(start = 12.dp, end = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = if (chosen) Color.White else MenuText, fontSize = 15.sp, maxLines = 1, modifier = Modifier.weight(1f))
        if (chosen) Icon(PcIcons.Tick, contentDescription = null, tint = Color.White, modifier = Modifier.size(20.dp))
    }
}

/** The halftone toolbar: full colour, screen print or newspaper. */
@Composable
fun HalftonePanel(vm: CanvasViewModel, modifier: Modifier = Modifier) {
    OptionsPanel(507.dp, 66.dp, modifier, 16.dp) {
        val kinds = listOf(HalftoneKind.FULL_COLOR, HalftoneKind.SCREEN_PRINT, HalftoneKind.NEWSPAPER)
        KindsSwitch(475.dp, listOf(Kind(PcIcons.HalftoneFull, "Full Color", 140.dp), Kind(PcIcons.HalftoneScreen, "Screen Print", 157.dp), Kind(PcIcons.HalftoneNews, "Newspaper", 151.dp)), kinds.indexOf(vm.halftoneKind)) {
            vm.chooseHalftoneKind(kinds[it])
        }
    }
}

/** The chromatic aberration toolbar: perspective or displace, with the kind's two sliders. */
@Composable
fun ChromaticPanel(vm: CanvasViewModel, modifier: Modifier = Modifier) {
    OptionsPanel(668.dp, 120.dp, modifier, 8.dp) {
        val kinds = listOf(ChromaticKind.PERSPECTIVE, ChromaticKind.DISPLACE)
        KindsSwitch(428.dp, listOf(Kind(PcIcons.ChromaPerspective, "Perspective", 204.dp), Kind(PcIcons.ChromaDisplace, "Displace", 204.dp)), kinds.indexOf(vm.chromaKind)) {
            vm.chooseChromaKind(kinds[it])
        }
        Spacer(Modifier.height(12.dp))
        val names = if (vm.chromaKind == ChromaticKind.PERSPECTIVE) listOf("Transition", "Fall off") else listOf("Blur", "Transparency")
        val values = vm.chromaValues[vm.chromaKind].orEmpty()
        Row(Modifier.width(620.dp), horizontalArrangement = Arrangement.spacedBy(20.dp)) {
            names.forEachIndexed { k, name ->
                val v = values.getOrElse(k) { 0f }
                OptionSlider(name, v, shown(v, max = true), 300.dp) { vm.setChromaValue(k, it) }
            }
        }
    }
}

private fun QuickShapeKind.label() = when (this) {
    QuickShapeKind.LINE -> "Line"
    QuickShapeKind.POLYLINE -> "Polyline"
    QuickShapeKind.ARC -> "Arc"
    QuickShapeKind.ELLIPSE -> "Ellipse"
    QuickShapeKind.CIRCLE -> "Circle"
    QuickShapeKind.TRIANGLE -> "Triangle"
    QuickShapeKind.EQUILATERAL_TRIANGLE -> "Equilateral"
    QuickShapeKind.QUADRILATERAL -> "Quadrilateral"
    QuickShapeKind.RECTANGLE -> "Rectangle"
    QuickShapeKind.SQUARE -> "Square"
}

private fun QuickShapeKind.icon() = when (this) {
    QuickShapeKind.LINE -> PcIcons.ShapeLine
    QuickShapeKind.POLYLINE -> PcIcons.ShapePolyline
    QuickShapeKind.ARC -> PcIcons.ShapeArc
    QuickShapeKind.ELLIPSE -> PcIcons.ShapeEllipse
    QuickShapeKind.CIRCLE -> PcIcons.ShapeCircle
    QuickShapeKind.TRIANGLE -> PcIcons.ShapeTriangle
    QuickShapeKind.EQUILATERAL_TRIANGLE -> PcIcons.ShapeEquilateral
    QuickShapeKind.QUADRILATERAL -> PcIcons.ShapeQuad
    QuickShapeKind.RECTANGLE -> PcIcons.ShapeRect
    QuickShapeKind.SQUARE -> PcIcons.ShapeSquare
}

/**
 * The bar a snapped stroke shows: the shape's name (and, while its handles show, "Editing" and
 * the kind in grey); a tap shows the handles, and the kinds it can be shown as below.
 */
@Composable
fun QuickShapeBar(vm: CanvasViewModel, modifier: Modifier = Modifier) {
    val info = vm.quickShape ?: return
    Column(modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
        Row(
            Modifier.fillMaxWidth().height(32.dp).background(ModeBar).tap { vm.quickShapeEditing = true },
            horizontalArrangement = Arrangement.Center,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.size(29.dp, 28.dp), contentAlignment = Alignment.Center) {
                // drawn in a 28 frame, shown as it is
                Icon(info.kind.icon(), contentDescription = null, tint = ModeText, modifier = Modifier.requiredSize(28.dp))
            }
            Text(if (vm.quickShapeEditing) "Editing" else info.kind.label(), color = ModeText, fontSize = 17.sp, fontWeight = FontWeight.Bold, maxLines = 1)
            if (vm.quickShapeEditing && info.choices.size > 1) {
                Spacer(Modifier.width(3.dp))
                Text(info.kind.label(), color = Color(0xFF9A9A9A), fontSize = 15.sp, maxLines = 1)
            }
            if (info.choices.size > 1) {
                Box(Modifier.size(33.dp, 32.dp), contentAlignment = Alignment.Center) {
                    Icon(PcIcons.Disclose, contentDescription = null, tint = ModeText, modifier = Modifier.requiredSize(32.dp))
                }
            }
        }
        if (vm.quickShapeEditing && info.choices.size > 1) {
            val each = if (info.choices.size > 2) 154.dp else 116.dp
            Spacer(Modifier.height(20.dp))
            Row(
                Modifier.height(32.dp).clip(RoundedCornerShape(16.dp)).background(ModesFill).padding(2.dp),
                horizontalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                for (kind in info.choices) {
                    val chosen = kind == info.kind
                    Row(
                        Modifier.size(each, 28.dp).clip(RoundedCornerShape(14.dp)).background(if (chosen) Pc.Accent else Color.Transparent).tap { vm.chooseQuickShape(kind) },
                        horizontalArrangement = Arrangement.Center,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(kind.icon(), contentDescription = null, tint = Color.White, modifier = Modifier.requiredSize(28.dp))
                        Spacer(Modifier.width(6.dp))
                        Text(kind.label(), color = Color.White, fontSize = 14.sp, maxLines = 1)
                    }
                }
            }
        }
    }
}

/** The snapped shape's handles, while they show: blue dots ringed in white. */
@Composable
fun QuickShapeHandles(vm: CanvasViewModel) {
    val info = vm.quickShape ?: return
    if (!vm.quickShapeEditing) return
    androidx.compose.foundation.Canvas(Modifier.fillMaxSize()) {
        val h = info.handles
        for (i in 0 until h.size / 2) {
            val at = Offset(h[i * 2], h[i * 2 + 1])
            drawCircle(Color.White, 6.5.dp.toPx(), at)
            drawCircle(Pc.Accent, 5.dp.toPx(), at)
        }
    }
}

