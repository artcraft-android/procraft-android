package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.input.pointer.AwaitPointerEventScope
import androidx.compose.foundation.layout.wrapContentHeight
import kotlinx.coroutines.withTimeoutOrNull
import kotlin.math.roundToInt
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.Panel
import io.github.artcraftandroid.procraft.core.ToolKind

private val SliderLeft = 5.dp
private val SizeTrackTop = 6.dp
private val OpacityTrackTop = 248.dp

/** A tap target without a ripple: the bars show their state through colour alone. */
private fun Modifier.tap(enabled: Boolean = true, onClick: () -> Unit): Modifier = composed {
    clickable(remember { MutableInteractionSource() }, indication = null, enabled = enabled, onClick = onClick)
}

/** The bar across the top of the canvas: artwork actions on the left, painting tools on the right. */
@Composable
fun TopBar(vm: CanvasViewModel, modifier: Modifier = Modifier) {
    BoxWithConstraints(modifier.fillMaxWidth().height(Pc.BarHeight).background(Pc.Bar)) {
        val (toolGap, actionGap) = Pc.barGaps(maxWidth)
        Row(
            Modifier.align(Alignment.CenterStart).padding(start = Pc.BarMargin),
            horizontalArrangement = Arrangement.spacedBy(actionGap),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.size(Pc.GalleryWidth, Pc.BarButtonHeight).tap { vm.closeArtwork() }, contentAlignment = Alignment.CenterStart) {
                Text("Gallery", color = Pc.BarText, fontSize = 18.sp, fontWeight = FontWeight.Medium, maxLines = 1, softWrap = false, modifier = Modifier.offset(y = (-1).dp))
            }
            ActionButton(PcIcons.Actions, "Actions", selected = vm.panel == Panel.ACTIONS) { vm.toggle(Panel.ACTIONS) }
            ActionButton(PcIcons.Adjustments, "Adjustments", selected = vm.panel == Panel.ADJUSTMENTS || vm.adjustment != null) { vm.adjustmentsPressed() }
            ActionButton(PcIcons.Selection, "Selection", selected = vm.selecting, glyph = 33.dp) { vm.toggleSelecting() }
            ActionButton(PcIcons.Transform, "Transform", selected = vm.transforming, glyph = 44.dp) { vm.toggleTransform() }
        }
        // the layers and colour panels take the highlight from the current tool while open
        val panelOpen = vm.panel == Panel.LAYERS || vm.panel == Panel.COLOR || vm.transforming || vm.selecting || vm.adjustment != null
        Row(
            Modifier.align(Alignment.CenterEnd).padding(end = Pc.BarMargin),
            horizontalArrangement = Arrangement.spacedBy(toolGap),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // painting an adjustment on (or cloning), the brush shows it
            ToolButton(
                if (vm.adjustPencil) PcIcons.BrushPaintedOn else PcIcons.Brush,
                "Paint",
                selected = vm.adjustPencil || (!panelOpen && vm.tool == ToolKind.PAINT),
                onHold = { vm.toolHeld(ToolKind.PAINT) },
            ) {
                vm.toolPressed(ToolKind.PAINT)
            }
            ToolButton(PcIcons.Smudge, "Smudge", selected = !panelOpen && vm.tool == ToolKind.SMUDGE, onHold = { vm.toolHeld(ToolKind.SMUDGE) }) {
                vm.toolPressed(ToolKind.SMUDGE)
            }
            ToolButton(PcIcons.Erase, "Erase", selected = !panelOpen && vm.tool == ToolKind.ERASE, onHold = { vm.toolHeld(ToolKind.ERASE) }) {
                vm.toolPressed(ToolKind.ERASE)
            }
            ToolButton(PcIcons.Layers, "Layers", selected = vm.panel == Panel.LAYERS) { vm.toggle(Panel.LAYERS) }
            ColorButton(vm)
        }
    }
}

/** An action button: its glyph on a dark disc, which turns blue while its panel is open. */
@Composable
private fun ActionButton(icon: ImageVector, label: String, selected: Boolean = false, glyph: Dp = 36.dp, onClick: () -> Unit = {}) {
    val lit = animateHighlight(selected)
    Box(Modifier.size(Pc.BarButtonWidth, Pc.BarButtonHeight).tap(onClick = onClick), contentAlignment = Alignment.Center) {
        Box(Modifier.size(Pc.ActionDiscSize).clip(CircleShape).background(lerp(Pc.ActionDisc, Pc.BarGlyphSelected, lit)))
        // the glyphs are drawn in a 44 frame, shown [glyph] across (most 36)
        Icon(icon, contentDescription = label, tint = lerp(Pc.ActionGlyph, Color.White, lit), modifier = Modifier.requiredSize(glyph))
    }
}

/** A tap target that also does [onHold] when held instead. */
private fun Modifier.tapOrHold(onClick: () -> Unit, onHold: () -> Unit): Modifier = composed {
    val tap by rememberUpdatedState(onClick)
    val hold by rememberUpdatedState(onHold)
    pointerInput(Unit) { detectTapGestures(onLongPress = { hold() }, onTap = { tap() }) }
}

/** A tool button: tapped, the tool (or its brushes); held ([onHold]), it takes the current brush. */
@Composable
private fun ToolButton(icon: ImageVector, label: String, selected: Boolean, onHold: (() -> Unit)? = null, onClick: () -> Unit) {
    val touch = if (onHold != null) Modifier.tapOrHold(onClick, onHold) else Modifier.tap(onClick = onClick)
    Box(Modifier.size(Pc.BarButtonWidth, Pc.BarButtonHeight).then(touch), contentAlignment = Alignment.Center) {
        // a 44 frame shown as it is, over the 38 tall button
        Icon(icon, contentDescription = label, tint = lerp(Pc.BarGlyph, Pc.BarGlyphSelected, animateHighlight(selected)), modifier = Modifier.requiredSize(44.dp))
    }
}

@Composable
private fun ColorButton(vm: CanvasViewModel) {
    val rgb = vm.colorRgb()
    // ringed only where the colour would fade into the bar: dark ones on the dark bar, light ones
    // on the light bar
    val ringed = if (Pc.light) vm.brightness > LIGHT_RING_ABOVE else vm.brightness < DARK_RING_BELOW
    // held, the colour painted with before
    Box(Modifier.size(Pc.BarButtonWidth, Pc.BarButtonHeight).tapOrHold({ vm.toggle(Panel.COLOR) }, { vm.previousColor() }), contentAlignment = Alignment.Center) {
        val dot = Modifier.size(Pc.ColorDot).clip(CircleShape).background(CanvasSpace.color(rgb))
        Box(if (ringed) dot.border(1.dp, Pc.ColorRing, CircleShape) else dot)
    }
}

/** The colour button's brightness (HSB) under which it is ringed on the dark bar. */
private const val DARK_RING_BELOW = 0.35f

/** The colour button's brightness over which it is ringed on the light bar. */
private const val LIGHT_RING_ABOVE = 0.85f

/**
 * The side bar against the left edge (the right one, [right]-handed): brush size, the modify
 * button, opacity, undo and redo. On the right everything is mirrored but the glyphs.
 */
@Composable
fun SideBar(vm: CanvasViewModel, modifier: Modifier = Modifier, right: Boolean = false) {
    // a control's left edge, mirrored on the right
    fun x(left: Dp, width: Dp) = if (right) Pc.SideBarWidth - left - width else left
    Box(
        modifier.size(Pc.SideBarWidth, Pc.SideBarHeight)
            .drawBehind {
                // translucent, with windows where the slider tracks are: they show what is behind
                val r = Pc.SideBarCorner.toPx()
                val track = Size(Pc.SliderWidth.toPx(), Pc.SliderHeight.toPx())
                val trackR = CornerRadius(Pc.SliderCorner.toPx())
                val body = Path().apply {
                    fillType = PathFillType.EvenOdd
                    val (inner, outer) = CornerRadius.Zero to CornerRadius(r)
                    addRoundRect(
                        if (right) RoundRect(0f, 0f, size.width, size.height, outer, inner, inner, outer)
                        else RoundRect(0f, 0f, size.width, size.height, inner, outer, outer, inner),
                    )
                    for (top in listOf(SizeTrackTop, OpacityTrackTop)) {
                        val o = Offset(x(SliderLeft, Pc.SliderWidth).toPx(), top.toPx())
                        addRoundRect(RoundRect(o.x, o.y, o.x + track.width, o.y + track.height, trackR))
                    }
                }
                drawPath(body, Pc.SideBar)
            },
    ) {
        BrushSlider(vm.size, vm::changeSize, Modifier.offset(x(SliderLeft, Pc.SliderWidth), SizeTrackTop), panel(vm, CanvasViewModel.SliderPanel.SIZE), vm.sizeNotches)
        // the modify button: the next touch on the canvas picks a colour
        Box(
            Modifier.offset(x(5.dp, 40.dp), 191.dp).size(40.dp, 41.dp).pointerInput(Unit) {
                // held, it changes what a canvas touch does; tapped, it does what Tap □ is set to
                awaitEachGesture {
                    awaitFirstDown().consume()
                    vm.modifyPressed()
                    val up = waitForUpOrCancellation()
                    up?.consume()
                    vm.modifyReleased(cancelled = up == null)
                }
            },
            contentAlignment = Alignment.Center,
        ) {
            Box(
                Modifier.size(Pc.ModifySize).drawBehind {
                    val w = Pc.ModifyStroke.toPx()
                    drawRoundRect(
                        if (vm.pickNext) Pc.Accent else Pc.SideGlyph,
                        topLeft = Offset(w / 2f, w / 2f),
                        size = Size(size.width - w, size.height - w),
                        cornerRadius = CornerRadius(Pc.ModifyCorner.toPx() - w / 2f),
                        style = Stroke(w),
                    )
                },
            )
        }
        BrushSlider(vm.opacity, vm::changeOpacity, Modifier.offset(x(SliderLeft, Pc.SliderWidth), OpacityTrackTop), panel(vm, CanvasViewModel.SliderPanel.OPACITY), vm.opacityNotches)
        // beside the slider it belongs to, a little above it, outside the bar
        vm.sliderPanel?.let { which ->
            val top = if (which == CanvasViewModel.SliderPanel.SIZE) SizeTrackTop else OpacityTrackTop
            SliderPanel(vm, which, Modifier.offset(x(SliderLeft + Pc.SliderWidth + SliderPanelGap, SliderPanelSide), top - SliderPanelRise))
        }
        SideButton(PcIcons.Undo, "Undo", vm.canUndo, Modifier.offset(x(5.dp, 40.dp), 430.5.dp)) { vm.undo() }
        SideButton(PcIcons.Redo, "Redo", vm.canRedo, Modifier.offset(x(5.dp, 40.dp), 472.5.dp)) { vm.redo() }
    }
}

@Composable
private fun SideButton(icon: ImageVector, label: String, enabled: Boolean, modifier: Modifier, onClick: () -> Unit) {
    Box(modifier.size(40.dp, 41.dp).tap(enabled, onClick), contentAlignment = Alignment.Center) {
        // a 50 frame shown 36 across
        Icon(icon, contentDescription = label, tint = if (enabled) Pc.SideGlyph else Pc.SideGlyphDisabled, modifier = Modifier.requiredSize(36.dp))
    }
}

/** What a slider's panel does: shown while it is dragged ([held] false), kept up when it is held still. */
class SliderPanelHooks(val show: (held: Boolean) -> Unit, val hide: () -> Unit)

private fun panel(vm: CanvasViewModel, which: CanvasViewModel.SliderPanel) = SliderPanelHooks(
    show = { held ->
        vm.sliderPanel = which
        vm.sliderPanelKept = held
    },
    hide = { if (!vm.sliderPanelKept) vm.sliderPanel = null },
)

private val SliderPanelSide = 190.dp
private val SliderPanelGap = 8.dp
private val SliderPanelRise = 10.dp
// dark glass over the canvas (what is behind it blurred), or light
private val SliderPanelFill get() = if (Pc.light) Color(0xFFE7E7E7) else Color(0xC93E3E3E)
private val SliderPanelInk get() = if (Pc.light) Color(0xFF393939) else Color(0xFFF0F0F0)
// the dot is drawn in the interface's ink, not the brush's colour
private val SliderPanelDot get() = if (Pc.light) Color.Black else Color.White

/**
 * A slider's panel: its name and value ("Size 8%"), a button saving the value as a notch on the
 * slider ("+"; "-" takes away the notch it is on), and in the middle a dot of the brush as it
 * would paint now, its size on the screen (and at its opacity).
 */
@Composable
private fun SliderPanel(vm: CanvasViewModel, which: CanvasViewModel.SliderPanel, modifier: Modifier) {
    val size = which == CanvasViewModel.SliderPanel.SIZE
    val value = if (size) vm.size else vm.opacity
    val diameter = androidx.compose.runtime.remember(vm.size) { vm.brushScreenDiameter() }
    // its own size, past the bar's edge (the bar's width would otherwise squeeze it)
    Box(
        modifier.wrapContentSize(Alignment.TopStart, unbounded = true).size(SliderPanelSide)
            .then(if (Pc.light) Modifier else rememberFrost(22.dp))
            .clip(RoundedCornerShape(22.dp)).background(SliderPanelFill)
            // a hairline rim, as the reference's (about 25 over the glass)
            .border(1.dp, Pc.GlassMenuEdge, RoundedCornerShape(22.dp)),
    ) {
        Text(
            androidx.compose.ui.text.buildAnnotatedString {
                pushStyle(androidx.compose.ui.text.SpanStyle(fontWeight = FontWeight.Bold))
                append(if (size) "Size" else "Opacity")
                pop()
                append(" ${(value * 100f).roundToInt()}%")
            },
            color = SliderPanelInk,
            fontSize = 19.sp,
            maxLines = 1,
            modifier = Modifier.offset(16.dp, 6.dp).height(32.dp).wrapContentHeight(Alignment.CenterVertically),
        )
        val onNotch = vm.notchAt(size, value) != null
        if (onNotch || vm.canSaveNotch(size)) {
            Box(Modifier.offset(150.dp, 6.dp).size(32.dp).tap { vm.toggleNotch(size) }, contentAlignment = Alignment.Center) {
                Icon(
                    if (onNotch) PcIcons.NotchRemove else PcIcons.NotchAdd,
                    contentDescription = if (onNotch) "Remove" else "Save",
                    tint = SliderPanelInk,
                    modifier = Modifier.size(24.dp),
                )
            }
        }
        androidx.compose.foundation.Canvas(Modifier.fillMaxSize()) {
            val r = (diameter / 2f).coerceIn(1.5f, this.size.minDimension / 2f - 8.dp.toPx())
            drawCircle(SliderPanelDot.copy(alpha = if (size) 1f else value.coerceIn(0.02f, 1f)), r, center)
        }
    }
}

/**
 * An upright slider moved by dragging anywhere on it: the thumb follows the finger (relative,
 * not jump-to-touch), 1 at the top. Moved, its [panel] shows while it moves; held still, it stays up.
 * Its [notches] (saved values) show as marks on the track; a value dragged near one is pulled onto
 * it, and a tap on one goes there. On a notch, the thumb shows a blue bar.
 */
@Composable
fun BrushSlider(value: Float, onChange: (Float) -> Unit, modifier: Modifier = Modifier, panel: SliderPanelHooks? = null, notches: List<Float> = emptyList()) {
    val current by rememberUpdatedState(value)
    val change by rememberUpdatedState(onChange)
    val hooks by rememberUpdatedState(panel)
    val marks by rememberUpdatedState(notches)
    fun near(v: Float, reach: Float = CanvasViewModel.NOTCH_NEAR) =
        marks.filter { kotlin.math.abs(it - v) < reach }.minByOrNull { kotlin.math.abs(it - v) }
    Box(
        modifier.size(Pc.SliderWidth, Pc.SliderHeight)
            .pointerInput(Unit) {
                val travel = (Pc.SliderHeight - Pc.ThumbHeight - Pc.ThumbInset * 2).toPx()
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    var last = down.position.y
                    var moved = false
                    var lifted = false
                    // where the finger has taken it, before a notch pulls it in
                    var raw = current
                    // follows the finger; false once it is lifted
                    suspend fun AwaitPointerEventScope.follow(): Boolean {
                        val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id }
                        if (c == null || !c.pressed) return false
                        val dy = c.position.y - last
                        if (dy != 0f) {
                            if (!moved) hooks?.show(false)
                            moved = true
                            last = c.position.y
                            raw = (raw - dy / travel).coerceIn(0f, 1f)
                            change(near(raw) ?: raw)
                        }
                        c.consume()
                        return true
                    }
                    withTimeoutOrNull(viewConfiguration.longPressTimeoutMillis) {
                        while (!lifted && !moved) lifted = !follow()
                    }
                    if (!lifted && !moved) hooks?.show(true)
                    // a tap on a notch's mark goes to it
                    if (lifted && !moved) {
                        val inset = Pc.ThumbInset.toPx()
                        val at = 1f - (down.position.y - inset - Pc.ThumbHeight.toPx() / 2f) / travel
                        near(at, Pc.ThumbHeight.toPx() / 2f / travel)?.let { change(it) }
                    }
                    while (!lifted) lifted = !follow()
                    if (moved) hooks?.hide()
                }
            }
            .drawBehind {
                val inset = Pc.ThumbInset.toPx()
                val thumb = Size(Pc.ThumbWidth.toPx(), Pc.ThumbHeight.toPx())
                val at = Offset((size.width - thumb.width) / 2f, inset + (size.height - thumb.height - inset * 2) * (1f - value))
                val knob = RoundRect(at.x, at.y, at.x + thumb.width, at.y + thumb.height, CornerRadius(thumb.height / 2f))
                // the track has a window where the thumb is, so the thumb shows over the bar alone
                val track = Path().apply {
                    fillType = PathFillType.EvenOdd
                    addRoundRect(RoundRect(0f, 0f, size.width, size.height, CornerRadius(Pc.SliderCorner.toPx())))
                    addRoundRect(knob)
                }
                drawPath(track, Pc.SliderTrack)
                // the notches: short bars across the track; on one, the thumb shows a blue bar
                val bar = Size(NotchWidth.toPx(), NotchHeight.toPx())
                val travel = size.height - thumb.height - inset * 2
                for (n in notches) {
                    val y = inset + thumb.height / 2f + travel * (1f - n)
                    drawRoundRect(NotchMark, Offset((size.width - bar.width) / 2f, y - bar.height / 2f), bar, CornerRadius(bar.height / 2f))
                }
                drawPath(Path().apply { addRoundRect(knob) }, Pc.SliderThumb)
                if (notches.any { kotlin.math.abs(it - value) < 1e-4f }) {
                    drawRoundRect(Pc.Accent, Offset((size.width - bar.width) / 2f, at.y + (thumb.height - bar.height) / 2f), bar, CornerRadius(bar.height / 2f))
                }
            },
    )
}

private val NotchWidth = 20.dp
private val NotchHeight = 4.dp
private val NotchMark = Color(0x1AFFFFFF)

/** The brush's outline where the pen paints or hovers: a thin grey ring with a light edge. */
@Composable
fun BrushCursor(vm: CanvasViewModel) {
    val at = vm.cursorAt ?: return
    val r = vm.cursorRadius
    if (r < 2f) return
    val style = vm.cursorStyle
    val paint = CanvasSpace.color(vm.colorRgb())
    androidx.compose.foundation.Canvas(Modifier.fillMaxSize()) {
        when (style) {
            CanvasViewModel.CursorStyle.CONTRAST -> {
                drawCircle(Color.White, r, at, style = Stroke(3.dp.toPx()))
                drawCircle(Color.Black, r, at, style = Stroke(1.dp.toPx()))
            }
            CanvasViewModel.CursorStyle.COLOR -> {
                drawCircle(Color(0x99FFFFFF), r, at, style = Stroke(3.dp.toPx()))
                drawCircle(paint, r, at, style = Stroke(1.5.dp.toPx()))
            }
            // as the brush says: none, contrasting rings, or the colour
            CanvasViewModel.CursorStyle.BRUSH -> when (vm.cursorOutline) {
                0 -> Unit
                2 -> {
                    drawCircle(Color(0x99FFFFFF), r, at, style = Stroke(3.dp.toPx()))
                    drawCircle(paint, r, at, style = Stroke(1.5.dp.toPx()))
                }
                else -> {
                    drawCircle(Color.White, r, at, style = Stroke(3.dp.toPx()))
                    drawCircle(Color.Black, r, at, style = Stroke(1.dp.toPx()))
                }
            }
        }
    }
}
