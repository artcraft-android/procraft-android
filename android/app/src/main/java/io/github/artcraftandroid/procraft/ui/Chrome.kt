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
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.ui.graphics.graphicsLayer
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
    // under the screen's cutout, its fill reaching up behind it
    BoxWithConstraints(modifier.fillMaxWidth().background(Pc.Bar).padding(top = Pc.safeTop, start = Pc.safeLeft, end = Pc.safeRight)) {
        val fit = Pc.barFit(maxWidth)
        val k = fit.scale
        Box(Modifier.fillMaxWidth().height(Pc.BarRowHeight * fit.rows)) {
            Row(
                Modifier.align(Alignment.TopStart).height(Pc.BarRowHeight).padding(start = fit.margin),
                horizontalArrangement = Arrangement.spacedBy(fit.actionGap),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.size(fit.gallery, Pc.BarButtonHeight).tap { vm.closeArtwork() }, contentAlignment = Alignment.CenterStart) {
                    // sized to its box, not the system's font size: larger, it would run under the buttons
                    val fontSize = with(androidx.compose.ui.platform.LocalDensity.current) { (18.dp * k).toSp() }
                    Text("Gallery", color = Pc.BarText, fontSize = fontSize, fontWeight = FontWeight.Medium, maxLines = 1, softWrap = false, modifier = Modifier.offset(y = (-1).dp))
                }
                ActionButtons(vm, fit.button, k)
            }
            Row(
                Modifier.align(Alignment.BottomEnd).height(Pc.BarRowHeight).padding(end = fit.margin),
                horizontalArrangement = Arrangement.spacedBy(fit.toolGap),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                ToolButtons(vm, fit.button, k)
            }
        }
    }
}

/** Actions, Adjustments, Selection and Transform, [width] wide each and [scale]d. */
@Composable
private fun ActionButtons(vm: CanvasViewModel, width: Dp, scale: Float) {
    ActionButton(PcIcons.Actions, "Actions", width, scale, selected = vm.panel == Panel.ACTIONS) { vm.toggle(Panel.ACTIONS) }
    ActionButton(PcIcons.Adjustments, "Adjustments", width, scale, selected = vm.panel == Panel.ADJUSTMENTS || vm.adjustment != null || vm.opacityLayer != null || vm.recoloring) { vm.adjustmentsPressed() }
    ActionButton(PcIcons.Selection, "Selection", width, scale, selected = vm.selecting, glyph = 33.dp) { vm.toggleSelecting() }
    ActionButton(PcIcons.Transform, "Transform", width, scale, selected = vm.transforming, glyph = 44.dp) { vm.toggleTransform() }
}

/** Paint, Smudge, Erase, Layers and the colour, [width] wide each and [scale]d; the tool lit on a blue [disc] in ProCraft Pocket. */
@Composable
private fun ToolButtons(vm: CanvasViewModel, width: Dp, scale: Float, disc: Boolean = false, colorScale: Float = scale) {
    // the layers and colour panels take the highlight from the current tool while open
    val panelOpen = vm.panel == Panel.LAYERS || vm.panel == Panel.COLOR || vm.transforming || vm.selecting || vm.adjustment != null
    // painting an adjustment on (or cloning), the brush shows it
    ToolButton(
        if (vm.adjustPencil) PcIcons.BrushPaintedOn else PcIcons.Brush,
        "Paint",
        width,
        scale,
        selected = vm.adjustPencil || (!panelOpen && vm.tool == ToolKind.PAINT),
        disc = disc,
        onHold = { vm.toolHeld(ToolKind.PAINT) },
    ) {
        vm.toolPressed(ToolKind.PAINT)
    }
    ToolButton(PcIcons.Smudge, "Smudge", width, scale, selected = !panelOpen && vm.tool == ToolKind.SMUDGE, disc = disc, onHold = { vm.toolHeld(ToolKind.SMUDGE) }) {
        vm.toolPressed(ToolKind.SMUDGE)
    }
    ToolButton(PcIcons.Erase, "Erase", width, scale, selected = !panelOpen && vm.tool == ToolKind.ERASE, disc = disc, onHold = { vm.toolHeld(ToolKind.ERASE) }) {
        vm.toolPressed(ToolKind.ERASE)
    }
    ToolButton(PcIcons.Layers, "Layers", width, scale, selected = vm.panel == Panel.LAYERS, disc = disc) { vm.toggle(Panel.LAYERS) }
    ColorButton(vm, width, colorScale)
}

/**
 * ProCraft Pocket's bar: a card of glass in the top left, the painting tools over the artwork's
 * actions; on a phone held sideways ([Pc.pocketSplit]) two pills across the top, the actions on the
 * left and the tools on the right. Swiped up, the card folds to a pill of the current tool and a
 * button that opens it again. It slides up off the screen and fades as the interface hides ([shown]).
 */
@Composable
fun PocketBar(vm: CanvasViewModel, modifier: Modifier = Modifier, shown: Float = 1f) {
    val top = Pc.safeTop + Pc.PocketInset
    val lift = with(androidx.compose.ui.platform.LocalDensity.current) { (top + Pc.PocketCardHeight).toPx() } * (1f - shown)
    val glass = BarGlass(shown, Offset(0f, -lift))
    val slide = Modifier.graphicsLayer {
        translationY = -lift
        alpha = shown
    }
    val fold = Modifier.pointerInput(Unit) {
        var pulled = 0f
        detectVerticalDragGestures(onDragStart = { pulled = 0f }) { change, dy ->
            change.consume()
            pulled += dy
            if (pulled < -PocketFoldPull.toPx()) Pc.pocketFolded = true
        }
    }
    val tool = Pc.PocketToolDisc / 44.dp
    Box(modifier.fillMaxWidth().padding(top = top, start = Pc.safeLeft + Pc.PocketInset, end = Pc.safeRight + Pc.PocketInset)) {
        when {
            Pc.pocketFolded -> PocketGlass(Modifier.align(Alignment.TopStart), glass, slide) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    val brush = when (vm.tool) {
                        ToolKind.SMUDGE -> PcIcons.Smudge
                        ToolKind.ERASE -> PcIcons.Erase
                        else -> if (vm.adjustPencil) PcIcons.BrushPaintedOn else PcIcons.Brush
                    }
                    ToolButton(brush, "Brushes", Pc.PocketButton, tool, selected = true, disc = true) { vm.toolPressed(vm.tool) }
                    Box(Modifier.size(Pc.PocketButton, Pc.PocketRowHeight).tap { Pc.pocketFolded = false }, contentAlignment = Alignment.Center) {
                        Icon(PcIcons.More, contentDescription = "Show the tools", tint = Pc.BarGlyph, modifier = Modifier.size(26.dp))
                    }
                }
            }
            Pc.pocketSplit -> {
                PocketGlass(Modifier.align(Alignment.TopStart), glass, slide) {
                    Row(verticalAlignment = Alignment.CenterVertically) { PocketActions(vm) }
                }
                PocketGlass(Modifier.align(Alignment.TopEnd), glass, slide) {
                    Row(verticalAlignment = Alignment.CenterVertically) { ToolButtons(vm, Pc.PocketButton, tool, disc = true, colorScale = 1f) }
                }
            }
            else -> PocketGlass(Modifier.align(Alignment.TopStart).then(fold), glass, slide) {
                androidx.compose.foundation.layout.Column(verticalArrangement = Arrangement.spacedBy(Pc.PocketRowGap)) {
                    Row(Modifier.height(Pc.PocketRowHeight), verticalAlignment = Alignment.CenterVertically) {
                        ToolButtons(vm, Pc.PocketButton, tool, disc = true, colorScale = 1f)
                    }
                    Row(Modifier.height(Pc.PocketRowHeight), verticalAlignment = Alignment.CenterVertically) { PocketActions(vm) }
                }
            }
        }
    }
}

/** Back to the gallery, then the actions, in ProCraft Pocket's bar. */
@Composable
private fun PocketActions(vm: CanvasViewModel) {
    Box(Modifier.size(Pc.PocketButton, Pc.PocketRowHeight).tap { vm.closeArtwork() }, contentAlignment = Alignment.Center) {
        Icon(PcIcons.ChevronLeft, contentDescription = "Gallery", tint = Pc.BarGlyph, modifier = Modifier.size(28.dp))
    }
    ActionButtons(vm, Pc.PocketButton, 1f)
}

/** A piece of ProCraft Pocket's glass: rounded, frosted over the canvas, sliding with the bar ([slide], [glass]). */
@Composable
private fun PocketGlass(modifier: Modifier, glass: BarGlass, slide: Modifier, content: @Composable () -> Unit) {
    val shape = RoundedCornerShape(Pc.PocketCorner)
    Box(
        modifier.then(rememberFrost(Pc.PocketCorner, glass)).then(slide).clip(shape).background(Pc.PocketGlass)
            .border(1.dp, Pc.GlassMenuEdge, shape).padding(Pc.PocketPadding),
    ) { content() }
}

/** How far ProCraft Pocket's card is swiped up to fold away. */
private val PocketFoldPull = 24.dp

/** An action button: its glyph on a dark disc, which turns blue while its panel is open; all of it [scale]d with the bar. */
@Composable
private fun ActionButton(icon: ImageVector, label: String, width: Dp, scale: Float, selected: Boolean = false, glyph: Dp = 36.dp, onClick: () -> Unit = {}) {
    val lit = animateHighlight(selected)
    Box(Modifier.size(width, Pc.BarButtonHeight).tap(onClick = onClick), contentAlignment = Alignment.Center) {
        Box(Modifier.size(Pc.ActionDiscSize * scale).clip(CircleShape).background(lerp(Pc.ActionDisc, Pc.BarGlyphSelected, lit)))
        // the glyphs are drawn in a 44 frame, shown [glyph] across (most 36)
        Icon(icon, contentDescription = label, tint = lerp(Pc.ActionGlyph, Color.White, lit), modifier = Modifier.requiredSize(glyph * scale))
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
private fun ToolButton(icon: ImageVector, label: String, width: Dp, scale: Float, selected: Boolean, disc: Boolean = false, onHold: (() -> Unit)? = null, onClick: () -> Unit) {
    val touch = if (onHold != null) Modifier.tapOrHold(onClick, onHold) else Modifier.tap(onClick = onClick)
    val lit = animateHighlight(selected)
    Box(Modifier.size(width, Pc.BarButtonHeight).then(touch), contentAlignment = Alignment.Center) {
        // on a disc, the glyph goes white over the blue
        if (disc) Box(Modifier.requiredSize(44.dp * scale).clip(CircleShape).background(Pc.BarGlyphSelected.copy(alpha = lit)))
        val tint = if (disc) lerp(Pc.BarGlyph, Color.White, lit) else lerp(Pc.BarGlyph, Pc.BarGlyphSelected, lit)
        // a 44 frame shown as wide as the button, over the 38 tall button
        Icon(icon, contentDescription = label, tint = tint, modifier = Modifier.requiredSize(44.dp * scale))
    }
}

@Composable
private fun ColorButton(vm: CanvasViewModel, width: Dp, scale: Float) {
    val rgb = vm.colorRgb()
    // ringed only where the colour would fade into the bar: dark ones on the dark bar, light ones
    // on the light bar
    val ringed = if (Pc.light) vm.brightness > LIGHT_RING_ABOVE else vm.brightness < DARK_RING_BELOW
    // held, the colour painted with before
    // dragged out onto the canvas, it fills there (ColorDrop)
    Box(Modifier.size(width, Pc.BarButtonHeight).dropSource(vm, onHold = { vm.previousColor() }) { vm.toggle(Panel.COLOR) }, contentAlignment = Alignment.Center) {
        val dot = Modifier.size(Pc.ColorDot * scale).clip(CircleShape).background(CanvasSpace.color(rgb))
        Box(if (ringed) dot.border(1.dp, Pc.ColorRing, CircleShape) else dot)
    }
}

/** The colour button's brightness (HSB) under which it is ringed on the dark bar. */
private const val DARK_RING_BELOW = 0.35f

/** The colour button's brightness over which it is ringed on the light bar. */
private const val LIGHT_RING_ABOVE = 0.85f

/**
 * The side bar against the left edge (the right one, [right]-handed): brush size, the modify
 * button, opacity, undo and redo, its tracks [trackHeight] long. On the right everything is
 * mirrored but the glyphs. [floating] (ProCraft Pocket), it stands off the edge, rounded all round.
 */
@Composable
fun SideBar(vm: CanvasViewModel, modifier: Modifier = Modifier, right: Boolean = false, trackHeight: Dp = Pc.SliderHeight, shown: Float = 1f, floating: Boolean = false) {
    // a control's left edge, mirrored on the right
    fun x(left: Dp, width: Dp) = if (right) Pc.SideBarWidth - left - width else left
    // shorter tracks bring what is under each up with them
    val shorter = Pc.SliderHeight - trackHeight
    val opacityTop = OpacityTrackTop - shorter
    // its glass slides out with it as the interface hides, its screen side's corners off the screen
    val aside = with(androidx.compose.ui.platform.LocalDensity.current) { Pc.SideBarWidth.toPx() } * (1f - shown) * if (right) 1f else -1f
    val moved = Offset(aside, 0f)
    val past = if (floating) 0.dp else Pc.SideBarCorner
    val body = BarGlass(shown, moved, pastLeft = if (right) 0.dp else past, pastRight = if (right) past else 0.dp)
    val track = BarGlass(shown, moved, window = true)
    Box(
        Modifier.then(rememberFrost(Pc.SideBarCorner, body)).then(modifier).size(Pc.SideBarWidth, Pc.SideBarHeight - shorter * 2)
            .drawBehind {
                // translucent, with windows where the slider tracks are: they show what is behind
                val r = Pc.SideBarCorner.toPx()
                val track = Size(Pc.SliderWidth.toPx(), trackHeight.toPx())
                val trackR = CornerRadius(Pc.SliderCorner.toPx())
                val body = Path().apply {
                    fillType = PathFillType.EvenOdd
                    val (inner, outer) = (if (floating) CornerRadius(r) else CornerRadius.Zero) to CornerRadius(r)
                    addRoundRect(
                        if (right) RoundRect(0f, 0f, size.width, size.height, outer, inner, inner, outer)
                        else RoundRect(0f, 0f, size.width, size.height, inner, outer, outer, inner),
                    )
                    for (top in listOf(SizeTrackTop, opacityTop)) {
                        val o = Offset(x(SliderLeft, Pc.SliderWidth).toPx(), top.toPx())
                        addRoundRect(RoundRect(o.x, o.y, o.x + track.width, o.y + track.height, trackR))
                    }
                }
                drawPath(body, Pc.SideBar)
            },
    ) {
        BrushSlider(vm.size, vm::changeSize, Modifier.offset(x(SliderLeft, Pc.SliderWidth), SizeTrackTop).then(rememberFrost(Pc.SliderCorner, track)), panel(vm, CanvasViewModel.SliderPanel.SIZE), vm.sizeNotches, trackHeight)
        // the modify button: the next touch on the canvas picks a colour
        Box(
            Modifier.offset(x(5.dp, 40.dp), 191.dp - shorter).size(40.dp, 41.dp).pointerInput(Unit) {
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
        BrushSlider(vm.opacity, vm::changeOpacity, Modifier.offset(x(SliderLeft, Pc.SliderWidth), opacityTop).then(rememberFrost(Pc.SliderCorner, track)), panel(vm, CanvasViewModel.SliderPanel.OPACITY), vm.opacityNotches, trackHeight)
        // beside the slider it belongs to, a little above it, outside the bar
        vm.sliderPanel?.let { which ->
            val top = if (which == CanvasViewModel.SliderPanel.SIZE) SizeTrackTop else opacityTop
            SliderPanel(vm, which, Modifier.offset(x(SliderLeft + Pc.SliderWidth + SliderPanelGap, SliderPanelSide), top - SliderPanelRise))
        }
        SideButton(PcIcons.Undo, "Undo", vm.canUndo, Modifier.offset(x(5.dp, 40.dp), 430.5.dp - shorter * 2)) { vm.undo() }
        SideButton(PcIcons.Redo, "Redo", vm.canRedo, Modifier.offset(x(5.dp, 40.dp), 472.5.dp - shorter * 2)) { vm.redo() }
    }
}

@Composable
private fun SideButton(icon: ImageVector, label: String, enabled: Boolean, modifier: Modifier, onClick: () -> Unit) {
    // turning on or off (a stroke kept, the last step undone) fades over 0.15 s either way, linear,
    // as measured: the lit glyph over the unlit one
    val lit = androidx.compose.animation.core.animateFloatAsState(if (enabled) 1f else 0f, Motion.highlightOff(), label = "enabled").value
    Box(modifier.size(40.dp, 41.dp).tap(enabled, onClick), contentAlignment = Alignment.Center) {
        // a 50 frame shown 36 across
        if (lit < 1f) Icon(icon, contentDescription = label.takeIf { lit <= 0f }, tint = Pc.SideGlyphDisabled, modifier = Modifier.requiredSize(36.dp))
        if (lit > 0f) Icon(icon, contentDescription = label, tint = Pc.SideGlyph.copy(alpha = Pc.SideGlyph.alpha * lit), modifier = Modifier.requiredSize(36.dp))
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
 * slider ("+"; "-" takes away the notch it is on), and in the middle the brush's stamp as it would
 * paint now (soft where it is soft), its size on the screen (and at its opacity).
 */
@Composable
private fun SliderPanel(vm: CanvasViewModel, which: CanvasViewModel.SliderPanel, modifier: Modifier) {
    val size = which == CanvasViewModel.SliderPanel.SIZE
    val value = if (size) vm.size else vm.opacity
    val diameter = androidx.compose.runtime.remember(vm.size) { vm.brushScreenDiameter() }
    // the brush's stamp as it paints (soft where it is soft), as many pixels across as it shows
    val density = androidx.compose.ui.platform.LocalDensity.current
    val radius = with(density) { (diameter / 2f).coerceIn(1.5f, SliderPanelSide.toPx() / 2f - 8.dp.toPx()) }
    val side = (radius * 2f).roundToInt().coerceAtLeast(2)
    val stamp = androidx.compose.runtime.remember(vm.size, side) { vm.brushStamp(side) }
    // its own size, past the bar's edge (the bar's width would otherwise squeeze it)
    Box(
        modifier.wrapContentSize(Alignment.TopStart, unbounded = true).size(SliderPanelSide)
            .then(if (Pc.light) Modifier else rememberFrost(22.dp))
            .clip(RoundedCornerShape(22.dp)).background(SliderPanelFill)
            // a hairline rim, as measured (about 25 over the glass)
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
        // read again as the notches change, so a value saved or taken away shows at once
        val notches = if (size) vm.sizeNotches else vm.opacityNotches
        val onNotch = androidx.compose.runtime.remember(notches, value) { vm.notchAt(size, value) != null }
        val canSave = androidx.compose.runtime.remember(notches, value) { vm.canSaveNotch(size) }
        if (onNotch || canSave) {
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
            val ink = SliderPanelDot.copy(alpha = if (size) 1f else value.coerceIn(0.02f, 1f))
            if (stamp == null) {
                drawCircle(ink, radius, center)
            } else {
                drawImage(
                    stamp,
                    topLeft = androidx.compose.ui.geometry.Offset(center.x - side / 2f, center.y - side / 2f),
                    colorFilter = androidx.compose.ui.graphics.ColorFilter.tint(ink),
                )
            }
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
fun BrushSlider(value: Float, onChange: (Float) -> Unit, modifier: Modifier = Modifier, panel: SliderPanelHooks? = null, notches: List<Float> = emptyList(), height: Dp = Pc.SliderHeight) {
    val current by rememberUpdatedState(value)
    val change by rememberUpdatedState(onChange)
    val hooks by rememberUpdatedState(panel)
    val marks by rememberUpdatedState(notches)
    fun near(v: Float, reach: Float = CanvasViewModel.NOTCH_NEAR) =
        marks.filter { kotlin.math.abs(it - v) < reach }.minByOrNull { kotlin.math.abs(it - v) }
    Box(
        modifier.size(Pc.SliderWidth, height)
            .pointerInput(height) {
                val travel = (height - Pc.ThumbHeight - Pc.ThumbInset * 2).toPx()
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    var last = down.position.y
                    var moved = false
                    var lifted = false
                    // the system took the touch back (the pen coming near takes the screen from a
                    // finger): it wasn't lifted, so the panel stays up for the pen to tap its +
                    var taken = false
                    // where the finger has taken it, before a notch pulls it in
                    var raw = current
                    // follows the finger; false once it is lifted. Another pointer's event (the
                    // pen hovering in) isn't this touch's.
                    suspend fun AwaitPointerEventScope.follow(): Boolean {
                        val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: return true
                        if (!c.pressed) {
                            // a cancelled touch ends already consumed; a lift doesn't
                            taken = c.isConsumed
                            return false
                        }
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
                    if (lifted && !moved && !taken) {
                        val inset = Pc.ThumbInset.toPx()
                        val at = 1f - (down.position.y - inset - Pc.ThumbHeight.toPx() / 2f) / travel
                        near(at, Pc.ThumbHeight.toPx() / 2f / travel)?.let { change(it) }
                    }
                    while (!lifted) lifted = !follow()
                    if (taken) hooks?.show(true) else if (moved) hooks?.hide()
                }
            }
            .drawBehind {
                val inset = Pc.ThumbInset.toPx()
                val thumb = Size(Pc.ThumbWidth.toPx(), Pc.ThumbHeight.toPx())
                val at = Offset((size.width - thumb.width) / 2f, inset + (size.height - thumb.height - inset * 2) * (1f - value))
                val knob = RoundRect(at.x, at.y, at.x + thumb.width, at.y + thumb.height, CornerRadius(thumb.height / 2f))
                // the track has a window where the thumb is, so the thumb shows over the bar alone;
                // a lighter rim runs round it
                val corner = Pc.SliderCorner.toPx()
                val rim = Pc.SliderRimWidth.toPx()
                val inside = RoundRect(rim, rim, size.width - rim, size.height - rim, CornerRadius(corner - rim))
                val track = Path().apply {
                    fillType = PathFillType.EvenOdd
                    addRoundRect(inside)
                    addRoundRect(knob)
                }
                drawPath(track, Pc.SliderTrack)
                val ring = Path().apply {
                    fillType = PathFillType.EvenOdd
                    addRoundRect(RoundRect(0f, 0f, size.width, size.height, CornerRadius(corner)))
                    addRoundRect(inside)
                }
                drawPath(ring, Pc.SliderRim)
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

/**
 * The brush's outline where the pen paints or hovers, in the brush's own shape (the round tip's a
 * circle): a thin dark line with a light edge, or in the colour painted with.
 */
@Composable
fun BrushCursor(vm: CanvasViewModel) {
    val at = vm.cursorAt ?: return
    val r = vm.cursorRadius
    if (r < 2f) return
    val style = vm.cursorStyle
    val paint = CanvasSpace.color(vm.colorRgb())
    val shape = vm.cursorShape
    androidx.compose.foundation.Canvas(Modifier.fillMaxSize()) {
        // its loops across the brush's diameter, about where it is
        val paths = shape.map { ring ->
            Path().apply {
                for (i in 0 until ring.size / 2) {
                    val x = at.x + ring[i * 2] * r * 2f
                    val y = at.y + ring[i * 2 + 1] * r * 2f
                    if (i == 0) moveTo(x, y) else lineTo(x, y)
                }
                close()
            }
        }
        fun outline(color: Color, width: Float) {
            if (paths.isEmpty()) drawCircle(color, r, at, style = Stroke(width))
            for (p in paths) drawPath(p, color, style = Stroke(width))
        }
        val contrast = { outline(Color.White, 3.dp.toPx()); outline(Color.Black, 1.dp.toPx()) }
        val colour = { outline(Color(0x99FFFFFF), 3.dp.toPx()); outline(paint, 1.5.dp.toPx()) }
        when (style) {
            CanvasViewModel.CursorStyle.CONTRAST -> contrast()
            CanvasViewModel.CursorStyle.COLOR -> colour()
            // as the brush says: none, contrasting lines, or the colour
            CanvasViewModel.CursorStyle.BRUSH -> when (vm.cursorOutline) {
                0 -> Unit
                2 -> colour()
                else -> contrast()
            }
        }
    }
}
