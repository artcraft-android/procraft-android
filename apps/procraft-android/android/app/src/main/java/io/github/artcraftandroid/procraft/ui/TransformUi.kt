package io.github.artcraftandroid.procraft.ui

import android.graphics.BlurMaskFilter
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.composed
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.geometry.lerp
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Paint
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.drawscope.rotateRad
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.TransformPopup
import io.github.artcraftandroid.procraft.canvas.boxOnScreen
import io.github.artcraftandroid.procraft.canvas.shownMeshPoints
import io.github.artcraftandroid.procraft.canvas.toScreen
import io.github.artcraftandroid.procraft.core.Interpolation
import io.github.artcraftandroid.procraft.core.TransformAction
import io.github.artcraftandroid.procraft.core.TransformBox
import io.github.artcraftandroid.procraft.core.TransformHandle
import io.github.artcraftandroid.procraft.core.TransformKind
import kotlin.math.atan2
import kotlin.math.roundToInt

private val NodeBlue = Color(0xFF0171EA)
private val RotateGreen = Color(0xFF00E607)
private val RotateLine = Color(0xFF00E400)
private val TurnOrange = Color(0xFFFEB800)
private val TurnLine = Color(0xFFFEBF00)
internal val Dash = Color(0xE6000000)
internal val DashGap = Color(0xE6FFFFFF)
private val NodeShadow = Color(0x1F000000)
private val MeshLine = Color(0xFF8B8B8B)

/** Where a capital's middle sits below the top of its line, as a fraction of the font size. */
private const val CAP_MIDDLE = 0.6056f

/** A held node is drawn this much larger. */
private const val HELD_SCALE = 1.5f

/** How long a let-go rotate or box-turning node takes to spring back (ms). */
private const val HANDLE_RETURN_MS = 500

private val ToolbarWidth = 728.dp
private val ToolbarHeight = 126.dp
private val ToolbarBottom = 25.dp
private val ToolbarCorner = 33.dp
// translucent: #282828 to #252525 over white, #212121 to #1e1e1e over the surround; light, #f2f2f2
// with a white track, dark labels, and grey ones for what can't be used yet
private val ToolbarTop get() = if (Pc.light) Color(0xF7F2F2F2) else Color(0xF7212121)
private val ToolbarFill get() = if (Pc.light) Color(0xF7F2F2F2) else Color(0xF71E1E1E)
private val SegmentTrack get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0xFF535353)
private val SegmentEdge get() = if (Pc.light) Color(0xFFEBEBEB) else Color(0xFF7E7E7E)
private val SegmentText get() = if (Pc.light) Color(0xFF3B3B3B) else Color(0xFFC1C1C1)
private val ButtonText get() = if (Pc.light) Color(0xFF414141) else Color(0xFFA3A3A3)
private val ButtonDimmed get() = if (Pc.light) Color(0xFF989898) else Color(0xFF575757)
private val SegmentGap = 11.33.dp
/** The transform toolbar's buttons: with room (landscape), and where the screen is narrow. */
private val TransformButtonRoomy = 89.dp
internal val ButtonWidth = 83.dp
private val ButtonHeight = 60.dp
/** The space between a roomy toolbar's buttons. */
private val ButtonGap = 19.dp

internal val MenuWidth = 260.dp
private val MenuArrow = 15.dp
private val MenuCorner = 26.dp
private val MenuFill get() = if (Pc.light) Color(0xEBF0F0F0) else Color(0xEB2A2A2A)
internal val MenuGroup get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0xFF313131)
internal val MenuLine get() = if (Pc.light) Color(0xFFE2E2E2) else Color(0xFF373737)
internal val MenuText: Color get() = if (Pc.light) Color(0xFF393939) else Color(0xFFC5C5C5)
internal val MenuGroupCorner = 13.dp

/** The transform box over the canvas: dashed edges, its nodes, the rotate node and the box-turning node. */
@Composable
fun TransformOverlay(vm: CanvasViewModel) {
    val box = vm.transformBox ?: return
    if (vm.transformKind == TransformKind.WARP && box.mesh.size == 32) {
        WarpOverlay(vm, box)
        return
    }
    val density = LocalDensity.current.density
    val shown = boxOnScreen(box, vm.viewMatrix, density) ?: return
    // a rotate or box-turning node let go away from its place springs back
    val letGo = vm.transformLetGo
    val back = remember(letGo) { Animatable(0f) }
    LaunchedEffect(letGo) {
        if (letGo != null) {
            back.animateTo(1f, tween(HANDLE_RETURN_MS))
            vm.transformLetGo = null
        }
    }
    val held = vm.transformHeld
    // the rotate and box-turning nodes follow the touch while held
    fun placed(handle: TransformHandle, rest: Offset): Offset = when {
        held?.first == handle -> held.second
        letGo?.first == handle -> lerp(letGo.second, rest, back.value)
        else -> rest
    }
    val rotate = placed(TransformHandle.Rotate, shown.rotate)
    val turn = placed(TransformHandle.TurnBox, shown.turn)
    fun scale(handle: TransformHandle) = if (held?.first == handle) HELD_SCALE else 1f
    Canvas(Modifier.fillMaxSize()) {
        val c = shown.corners
        val edge = Path().apply {
            moveTo(c[0].x, c[0].y)
            for (i in 1..3) lineTo(c[i].x, c[i].y)
            close()
        }
        val dash = 5.dp.toPx()
        val width = 1.dp.toPx()
        drawPath(edge, DashGap, style = Stroke(width, pathEffect = PathEffect.dashPathEffect(floatArrayOf(dash, dash), dash)))
        drawPath(edge, Dash, style = Stroke(width, pathEffect = PathEffect.dashPathEffect(floatArrayOf(dash, dash), 0f)))
        drawLine(RotateLine, shown.edges[0], rotate, width)
        drawLine(TurnLine, shown.edges[2], turn, width)
        c.forEachIndexed { i, p -> node(p, NodeBlue, scale(TransformHandle.Corner(i.toUByte()))) }
        shown.edges.forEachIndexed { i, p -> node(p, NodeBlue, scale(TransformHandle.Edge(i.toUByte()))) }
        node(rotate, RotateGreen, scale(TransformHandle.Rotate))
        // the box-turning node is a square, turned with the box
        val k = scale(TransformHandle.TurnBox)
        rotateRad(atan2(shown.up.x, -shown.up.y), turn) {
            shadow(turn, 5.5.dp.toPx() * k)
            val outer = 11.dp.toPx() * k
            drawRoundRect(Color.White, turn - Offset(outer / 2, outer / 2), Size(outer, outer), CornerRadius(2.5.dp.toPx() * k))
            val inner = 8.dp.toPx() * k
            drawRoundRect(TurnOrange, turn - Offset(inner / 2, inner / 2), Size(inner, inner), CornerRadius(1.dp.toPx() * k))
        }
    }
}

/** A warp over the canvas: its lines at the thirds, its shown control points, and with the advanced mesh the lines joining them. */
@Composable
private fun WarpOverlay(vm: CanvasViewModel, box: TransformBox) {
    val m = vm.viewMatrix
    val held = vm.transformHeld?.first
    val advanced = vm.advancedMesh
    Canvas(Modifier.fillMaxSize()) {
        val mesh = List(16) { toScreen(box.mesh[2 * it], box.mesh[2 * it + 1], m) }
        if (advanced) {
            // the control points joined along each row, and down the two edges, dotted, under the
            // lines (which hide them where they agree)
            val dots = PathEffect.dashPathEffect(floatArrayOf(1.dp.toPx(), 2.dp.toPx()), 0f)
            for (k in 0 until 4) {
                for (j in 0 until 3) {
                    drawLine(Dash, mesh[k * 4 + j], mesh[k * 4 + j + 1], 1.dp.toPx(), pathEffect = dots)
                    if (k == 0 || k == 3) drawLine(Dash, mesh[j * 4 + k], mesh[(j + 1) * 4 + k], 1.dp.toPx(), pathEffect = dots)
                }
            }
        }
        val grid = box.grid
        val perLine = grid.size / 2 / 8
        if (perLine >= 2) {
            for (line in 0 until 8) {
                val path = Path()
                for (k in 0 until perLine) {
                    val i = (line * perLine + k) * 2
                    val p = toScreen(grid[i], grid[i + 1], m)
                    if (k == 0) path.moveTo(p.x, p.y) else path.lineTo(p.x, p.y)
                }
                drawPath(path, MeshLine, style = Stroke(1.dp.toPx()))
            }
        }
        for (i in shownMeshPoints(advanced)) {
            val holding = held == TransformHandle.Mesh(i.toUByte()) || (held is TransformHandle.Corner && listOf(0, 3, 15, 12)[held.index.toInt() % 4] == i)
            node(mesh[i], NodeBlue, if (holding) HELD_SCALE else 1f)
        }
    }
}

/** A node: a coloured dot on a white disc with a faint shadow, [scale] times its size (larger while held). */
internal fun DrawScope.node(at: Offset, color: Color, scale: Float) {
    shadow(at, 5.5.dp.toPx() * scale)
    drawCircle(Color.White, 5.5.dp.toPx() * scale, at)
    drawCircle(color, 3.5.dp.toPx() * scale, at)
}

private fun DrawScope.shadow(at: Offset, radius: Float) {
    drawIntoCanvas { canvas ->
        val paint = Paint().apply {
            color = NodeShadow
            asFrameworkPaint().maskFilter = BlurMaskFilter(1.dp.toPx(), BlurMaskFilter.Blur.NORMAL)
        }
        canvas.drawCircle(at, radius + 0.5.dp.toPx(), paint)
    }
}

/** A tap target without a ripple. */
internal fun Modifier.tap(onClick: () -> Unit): Modifier = composed {
    clickable(remember { MutableInteractionSource() }, indication = null, onClick = onClick)
}

/** The floating toolbar at the foot of the screen while transforming. */
@Composable
fun TransformToolbar(vm: CanvasViewModel, screenWidth: Dp, modifier: Modifier = Modifier) {
    FloatingToolbar(
        toolbarLayout(screenWidth, 7, TransformButtonRoomy, ButtonWidth),
        onBackground = { vm.transformPopup = TransformPopup.NONE },
        modifier = modifier,
        segments = {
            Segment(PcIcons.Freeform, "Freeform", vm.transformKind == TransformKind.FREEFORM, iconSize = 43.dp) { vm.chooseTransformKind(TransformKind.FREEFORM) }
            Segment(PcIcons.Uniform, "Uniform", vm.transformKind == TransformKind.UNIFORM, iconSize = 43.dp) { vm.chooseTransformKind(TransformKind.UNIFORM) }
            Segment(PcIcons.Distort, "Distort", vm.transformKind == TransformKind.DISTORT, iconSize = 43.dp) { vm.chooseTransformKind(TransformKind.DISTORT) }
            Segment(PcIcons.Warp, "Warp", vm.transformKind == TransformKind.WARP, iconSize = 43.dp) { vm.chooseTransformKind(TransformKind.WARP) }
        },
    ) { w ->
            val popup = vm.transformPopup
            // a long name smaller only where the buttons are narrow
            val long = if (w < TransformButtonRoomy) 11.sp else 12.5.sp
            if (vm.transformKind == TransformKind.WARP) {
                ToolbarButton(PcIcons.AdvancedMesh, "Advanced Mesh", active = vm.advancedMesh, width = w, iconSize = 28.8.dp, labelSize = long) { vm.toggleAdvancedMesh() }
            } else {
                ToolbarButton(PcIcons.Snapping, "Snapping", active = popup == TransformPopup.SNAPPING, width = w, iconSize = 44.dp) {
                    vm.transformPopup = if (popup == TransformPopup.SNAPPING) TransformPopup.NONE else TransformPopup.SNAPPING
                }
            }
            ToolbarButton(PcIcons.FlipHorizontal, "Flip Horizontal", width = w, iconSize = 50.dp) { vm.transformAction(TransformAction.FLIP_HORIZONTAL) }
            ToolbarButton(PcIcons.FlipVertical, "Flip Vertical", width = w, iconSize = 50.dp) { vm.transformAction(TransformAction.FLIP_VERTICAL) }
            ToolbarButton(PcIcons.Rotate45, "Rotate 45°", width = w, iconSize = 44.dp) { vm.transformAction(TransformAction.ROTATE45) }
            ToolbarButton(PcIcons.FitToCanvas, "Fit to Canvas", width = w, iconSize = 35.dp) { vm.transformAction(TransformAction.FIT_TO_CANVAS) }
            ToolbarButton(PcIcons.Interpolation, interpolationName(vm.interpolation, short = true), active = popup == TransformPopup.INTERPOLATION, width = w, iconSize = 44.dp) {
                vm.transformPopup = if (popup == TransformPopup.INTERPOLATION) TransformPopup.NONE else TransformPopup.INTERPOLATION
            }
            ToolbarButton(PcIcons.TransformReset, "Reset", width = w, iconSize = 44.dp) { vm.transformAction(TransformAction.RESET) }
    }
}

/** A floating toolbar's width and its buttons' width. */
internal data class ToolbarLayout(val width: Dp, val button: Dp)

/**
 * A floating toolbar of [count] buttons on a screen [screenWidth] wide: with room, [roomy]-wide
 * buttons 19 apart inside its padding (as measured in landscape); else across the screen (8 clear
 * of each side, at most 728) with [narrow]-wide buttons spread over it.
 */
internal fun toolbarLayout(screenWidth: Dp, count: Int, roomy: Dp, narrow: Dp): ToolbarLayout {
    val wide = roomy * count + ButtonGap * (count - 1) + 32.dp
    return if (wide <= screenWidth - 16.dp) ToolbarLayout(wide, roomy) else ToolbarLayout(minOf(ToolbarWidth, screenWidth - 16.dp), narrow)
}

/**
 * The floating toolbar at the foot of the screen (transform, selection): a row of [segments] in a
 * pill over a row of [buttons] (given their width) spread across it. A tap on its background calls
 * [onBackground].
 */
@Composable
internal fun FloatingToolbar(
    layout: ToolbarLayout,
    onBackground: () -> Unit,
    modifier: Modifier = Modifier,
    segments: @Composable RowScope.() -> Unit,
    buttons: @Composable RowScope.(Dp) -> Unit,
) {
    Column(
        modifier.offset(y = -ToolbarBottom).size(layout.width, ToolbarHeight)
            .clip(RoundedCornerShape(ToolbarCorner))
            .background(Brush.verticalGradient(0f to ToolbarTop, 0.12f to ToolbarFill))
            .tap(onBackground)
            .padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Row(
            Modifier.height(34.dp).clip(CircleShape).background(SegmentTrack).border(0.5.dp, SegmentEdge, CircleShape).padding(2.dp),
            horizontalArrangement = Arrangement.spacedBy(SegmentGap),
            content = segments,
        )
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) { buttons(layout.button) }
    }
}

/** Where button [index] of [count] is centred on a toolbar laid out as [layout], from the screen's left. */
internal fun toolbarButtonCentre(screenWidth: Dp, layout: ToolbarLayout, index: Int, count: Int): Dp {
    val left = (screenWidth - layout.width) / 2 + 16.dp
    val gap = (layout.width - 32.dp - layout.button * count) / (count - 1).coerceAtLeast(1)
    return left + (layout.button + gap) * index + layout.button / 2
}

/** The top of the toolbar's buttons, from the top of a screen [screenHeight] tall: where popovers point. */
internal fun toolbarButtonsTop(screenHeight: Dp): Dp = screenHeight - ToolbarBottom - ToolbarHeight + 16.dp + 34.dp

/** One of the four ways: its icon and name, filled blue when chosen. */
@Composable
internal fun Segment(icon: ImageVector, label: String, selected: Boolean, iconSize: Dp = 26.5.dp, onClick: () -> Unit) {
    val tint = if (selected) Color.White else SegmentText
    Row(
        Modifier.height(30.dp).clip(CircleShape).background(if (selected) Pc.Accent else Color.Transparent).tap(onClick)
            .padding(start = 14.dp, end = 20.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(30.dp), contentAlignment = Alignment.Center) {
            Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.requiredSize(iconSize))
        }
        Spacer(Modifier.width(2.dp))
        Text(label, color = tint, fontSize = 13.sp, letterSpacing = (-0.025).em, maxLines = 1, softWrap = false)
    }
}

/** A toolbar button: an icon over its name; blue while its popover is open. */
@Composable
internal fun ToolbarButton(
    icon: ImageVector,
    label: String,
    active: Boolean = false,
    enabled: Boolean = true,
    width: Dp = ButtonWidth,
    iconSize: Dp = 24.dp,
    labelSize: TextUnit = 12.5.sp,
    onClick: () -> Unit,
) {
    val tint = when {
        !enabled -> ButtonDimmed
        active -> Pc.Accent
        else -> ButtonText
    }
    Box(Modifier.size(width, ButtonHeight).then(if (enabled) Modifier.tap(onClick) else Modifier)) {
        // icons centred 30 below the top, labels' capitals 50 below it
        Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.align(Alignment.TopCenter).offset(y = 30.dp - iconSize / 2).requiredSize(iconSize))
        Text(
            label,
            color = if (enabled) ButtonText else ButtonDimmed,
            fontSize = labelSize,
            letterSpacing = 0.em,
            maxLines = 1,
            softWrap = false,
            // a long name may run past the button, as the reference's do
            modifier = Modifier.align(Alignment.TopCenter).offset(y = 50.dp - labelSize.value.dp * CAP_MIDDLE).wrapContentWidth(unbounded = true),
        )
    }
}

private fun interpolationName(i: Interpolation, short: Boolean): String = when (i) {
    Interpolation.NEAREST -> if (short) "Nearest" else "Nearest Neighbor"
    Interpolation.BILINEAR -> "Bilinear"
    Interpolation.BICUBIC -> "Bicubic"
}

/**
 * The popover open over the transform toolbar, its arrow on its button. [screenWidth] and
 * [screenHeight] place it; the toolbar's buttons start 94 dp above its foot.
 */
@Composable
fun TransformPopover(vm: CanvasViewModel, screenWidth: Dp, screenHeight: Dp) {
    val popup = vm.transformPopup
    if (popup == TransformPopup.NONE) return
    val layout = toolbarLayout(screenWidth, 7, TransformButtonRoomy, ButtonWidth)
    val buttonCentre = toolbarButtonCentre(screenWidth, layout, if (popup == TransformPopup.SNAPPING) 0 else 5, 7)
    val left = (buttonCentre - MenuWidth / 2).coerceIn(0.dp, screenWidth - MenuWidth)
    MenuPopover(buttonCentre - left, left, bottom = toolbarButtonsTop(screenHeight)) {
        when (popup) {
            TransformPopup.INTERPOLATION -> InterpolationMenu(vm)
            TransformPopup.SNAPPING -> SnappingMenu(vm)
            TransformPopup.NONE -> Unit
        }
    }
}

/** A popover [left] from the screen's left whose arrow points down at its button, the tip at [bottom] from the top. */
@Composable
internal fun MenuPopover(arrowX: Dp, left: Dp, bottom: Dp, width: Dp = MenuWidth, content: @Composable ColumnScope.() -> Unit) {
    Box(Modifier.fillMaxSize()) {
        Column(
            Modifier
                .layoutAbove(left, bottom)
                .width(width)
                .drawBehind {
                    val arrowH = MenuArrow.toPx()
                    val tipX = arrowX.toPx()
                    val bodyBottom = size.height - arrowH
                    val tip = 2.dp.toPx()
                    val outline = Path().apply {
                        addRoundRect(RoundRect(0f, 0f, size.width, bodyBottom, CornerRadius(MenuCorner.toPx())))
                        moveTo(tipX - arrowH - tip, bodyBottom - 0.5f)
                        lineTo(tipX - tip, size.height - tip)
                        quadraticTo(tipX, size.height, tipX + tip, size.height - tip)
                        lineTo(tipX + arrowH + tip, bodyBottom - 0.5f)
                        close()
                    }
                    drawIntoCanvas { canvas ->
                        val shadow = Paint().apply {
                            color = Pc.PopoverShadow
                            asFrameworkPaint().maskFilter = BlurMaskFilter(Pc.PopoverShadowRadius.toPx(), BlurMaskFilter.Blur.NORMAL)
                        }
                        canvas.drawPath(outline, shadow)
                    }
                    drawPath(outline, MenuFill)
                }
                .tap {}
                .padding(start = 10.dp, end = 10.dp, top = 10.dp, bottom = 10.dp + MenuArrow),
            content = content,
        )
    }
}

/** Places a node [left] from its parent's left with its bottom edge [bottom] from the parent's top. */
private fun Modifier.layoutAbove(left: Dp, bottom: Dp): Modifier = layout { measurable, constraints ->
    val placeable = measurable.measure(constraints.copy(minWidth = 0, minHeight = 0))
    layout(placeable.width, placeable.height) {
        placeable.place(left.roundToPx(), bottom.roundToPx() - placeable.height)
    }
}

@Composable
internal fun MenuTitle(text: String) {
    Box(Modifier.fillMaxWidth().height(40.dp).padding(start = 12.dp), contentAlignment = Alignment.CenterStart) {
        Text(text, color = MenuText, fontSize = 24.sp, fontWeight = FontWeight.Bold, letterSpacing = (-0.015).em, maxLines = 1)
    }
}

/** A row's text in the toolbar's popovers. */
@Composable
internal fun MenuLabel(text: String, color: Color = MenuText, modifier: Modifier = Modifier) {
    Text(text, color = color, fontSize = 16.sp, letterSpacing = 0.em, maxLines = 1, modifier = modifier)
}

@Composable
private fun InterpolationMenu(vm: CanvasViewModel) {
    MenuTitle("Interpolation")
    Spacer(Modifier.height(10.dp))
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(MenuGroupCorner)).background(MenuGroup)) {
        val all = listOf(Interpolation.NEAREST, Interpolation.BILINEAR, Interpolation.BICUBIC)
        all.forEachIndexed { i, kind ->
            if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(MenuLine))
            val selected = vm.interpolation == kind
            Row(
                Modifier.fillMaxWidth().height(40.dp).background(if (selected) Pc.Accent else Color.Transparent)
                    .tap {
                        vm.chooseInterpolation(kind)
                        vm.transformPopup = TransformPopup.NONE
                    }
                    .padding(start = 11.dp, end = 14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                MenuLabel(interpolationName(kind, short = false), if (selected) Color.White else MenuText, Modifier.weight(1f))
                if (selected) Icon(PcIcons.MenuTick, contentDescription = null, tint = Color.White, modifier = Modifier.size(29.dp))
            }
        }
    }
}

@Composable
private fun SnappingMenu(vm: CanvasViewModel) {
    val g = vm.guides
    MenuTitle("Settings")
    Spacer(Modifier.height(4.dp))
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(MenuGroupCorner)).background(MenuGroup)) {
        SwitchRow("Magnetics", g.magnetics) { vm.changeGuides(g.copy(magnetics = !g.magnetics)) }
        Box(Modifier.fillMaxWidth().height(1.dp).background(MenuLine))
        SwitchRow("Snapping", g.snapping) { vm.changeGuides(g.copy(snapping = !g.snapping)) }
    }
    Spacer(Modifier.height(10.dp))
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(MenuGroupCorner)).background(MenuGroup)) {
        // distance runs 1..50 points; at its default and below it reads as none
        val distance = (g.distance - SNAP_DISTANCE_MIN) / (SNAP_DISTANCE_MAX - SNAP_DISTANCE_MIN)
        SliderRow("Distance", distance, if (g.distance <= SNAP_DISTANCE_DEFAULT) "None" else "${g.distance.roundToInt()}") {
            vm.changeGuides(g.copy(distance = (SNAP_DISTANCE_MIN + it * (SNAP_DISTANCE_MAX - SNAP_DISTANCE_MIN)).roundToInt().toFloat()))
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(MenuLine))
        SliderRow("Velocity", g.velocity / SNAP_VELOCITY_MAX, if (g.velocity >= SNAP_VELOCITY_MAX) "Max" else "${g.velocity.roundToInt()}") {
            vm.changeGuides(g.copy(velocity = (it * SNAP_VELOCITY_MAX).roundToInt().toFloat()))
        }
    }
}

private const val SNAP_DISTANCE_MIN = 1f
private const val SNAP_DISTANCE_MAX = 50f
private const val SNAP_DISTANCE_DEFAULT = 3f
private const val SNAP_VELOCITY_MAX = 10f

@Composable
private fun SwitchRow(label: String, checked: Boolean, onToggle: () -> Unit) {
    Row(Modifier.fillMaxWidth().height(49.dp).tap(onToggle).padding(start = 12.dp, end = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        MenuLabel(label, modifier = Modifier.weight(1f))
        Switch(checked, enabled = true)
    }
}

@Composable
internal fun SliderRow(label: String, value: Float, shown: String, onChange: (Float) -> Unit) {
    Column(Modifier.fillMaxWidth().height(59.dp)) {
        Row(Modifier.fillMaxWidth().padding(start = 14.dp, end = 13.dp, top = 9.dp)) {
            MenuLabel(label, modifier = Modifier.weight(1f))
            MenuLabel(shown)
        }
        MenuSlider(value, onChange, Modifier.padding(top = 9.dp))
    }
}

/** A thin slider: blue up to a small white knob, grey after it. */
@Composable
private fun MenuSlider(value: Float, onChange: (Float) -> Unit, modifier: Modifier = Modifier) {
    val change by rememberUpdatedState(onChange)
    Box(
        modifier.fillMaxWidth().height(14.dp)
            .pointerInput(Unit) {
                val start = SliderStart.toPx()
                val end = SliderEnd.toPx()
                fun at(x: Float) = ((x - start) / (size.width - start - end)).coerceIn(0f, 1f)
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    change(at(down.position.x))
                    while (true) {
                        val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!c.pressed) break
                        change(at(c.position.x))
                        c.consume()
                    }
                }
            }
            .drawBehind {
                val start = SliderStart.toPx()
                val end = size.width - SliderEnd.toPx()
                val y = size.height / 2f
                val x = start + (end - start) * value.coerceIn(0f, 1f)
                val track = 2.dp.toPx()
                drawLine(SliderRest, Offset(x, y), Offset(end, y), track, cap = StrokeCap.Round)
                drawLine(Pc.Accent, Offset(start, y), Offset(x, y), track, cap = StrokeCap.Round)
                val knob = Size(15.dp.toPx(), 11.dp.toPx())
                drawRoundRect(Color(0x33000000), Offset(x - knob.width / 2, y - knob.height / 2 + 0.5.dp.toPx()), knob, CornerRadius(knob.height / 2))
                drawRoundRect(Color.White, Offset(x - knob.width / 2, y - knob.height / 2), knob, CornerRadius(knob.height / 2))
            },
    )
}

/** Where the slider's knob centre runs, from the row's left edge and from its right. */
private val SliderStart = 21.5.dp
private val SliderEnd = 15.dp
private val SliderRest get() = if (Pc.light) Color(0xFFDDDDDD) else Color(0xFF5A5A5A)
