package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.SelectionKind
import io.github.artcraftandroid.procraft.canvas.SelectionPopup
import kotlin.math.roundToInt

/** The selection toolbar's buttons: with room (landscape), and where the screen is narrow. */
private val SelectionButtonRoomy = 83.dp
private val SelectionButtonWidth = 70.dp
private val FeatherWidth = 250.dp
private val StartDot = Color(0xFFC7C7C7)
private val SavedThumb = Color(0xFF000000)

/** The selection tool's floating toolbar: its four ways, and what can be done with the selection. */
@Composable
fun SelectionToolbar(vm: CanvasViewModel, screenWidth: Dp, modifier: Modifier = Modifier) {
    val has = vm.hasSelection
    val drawing = vm.freehand.isNotEmpty()
    FloatingToolbar(
        toolbarLayout(screenWidth, 8, SelectionButtonRoomy, SelectionButtonWidth),
        onBackground = { vm.selectionPopup = SelectionPopup.NONE },
        modifier = modifier,
        segments = {
            for (kind in SelectionKind.entries) {
                val icon = when (kind) {
                    SelectionKind.AUTOMATIC -> PcIcons.Automatic
                    SelectionKind.FREEHAND -> PcIcons.Freehand
                    SelectionKind.RECTANGLE -> PcIcons.RectangleShape
                    SelectionKind.ELLIPSE -> PcIcons.EllipseShape
                }
                // drawn in a 44 frame, shown 41 across
                Segment(icon, kind.label, vm.selectionKind == kind, iconSize = 41.dp) { vm.chooseSelectionKind(kind) }
            }
        },
    ) { w ->
        // a long name smaller only where the buttons are narrow
        val long = if (w < SelectionButtonRoomy) 11.sp else 12.5.sp
        // selecting by colour always adds
        val joins = vm.selectionKind != SelectionKind.AUTOMATIC
        ToolbarButton(PcIcons.SelectAdd, "Add", active = joins && !vm.selectionRemoves, enabled = joins, width = w, iconSize = 44.dp) { vm.chooseSelectionRemoves(false) }
        ToolbarButton(PcIcons.SelectRemove, "Remove", active = joins && vm.selectionRemoves, enabled = joins, width = w, iconSize = 44.dp) { vm.chooseSelectionRemoves(true) }
        ToolbarButton(PcIcons.Invert, "Invert", enabled = has, width = w, iconSize = 44.dp) { vm.invertSelection() }
        ToolbarButton(PcIcons.SelectCopyPaste, "Copy & Paste", enabled = has || drawing, width = w, iconSize = 44.dp, labelSize = long) { vm.copySelectionToLayer() }
        ToolbarButton(PcIcons.Feather, "Feather", active = vm.selectionPopup == SelectionPopup.FEATHER, enabled = has || drawing, width = w, iconSize = 44.dp) {
            vm.openSelectionPopup(SelectionPopup.FEATHER)
        }
        ToolbarButton(PcIcons.SaveLoad, "Save & Load", active = vm.selectionPopup == SelectionPopup.SAVED, enabled = has || drawing, width = w, iconSize = 44.dp, labelSize = long) {
            vm.openSelectionPopup(SelectionPopup.SAVED)
        }
        ToolbarButton(PcIcons.ColorFill, "Color Fill", active = vm.colorFill, width = w, iconSize = 37.dp) { vm.toggleColorFill() }
        ToolbarButton(PcIcons.ClearSelection, "Clear", enabled = has || drawing, width = w, iconSize = 44.dp) { vm.clearSelection() }
    }
}

/** A popover over the selection toolbar: feathering the selection, or the selections kept. */
@Composable
fun SelectionPopover(vm: CanvasViewModel, screenWidth: Dp, screenHeight: Dp) {
    val popup = vm.selectionPopup
    if (popup == SelectionPopup.NONE) return
    val width = if (popup == SelectionPopup.FEATHER) FeatherWidth else MenuWidth
    val layout = toolbarLayout(screenWidth, 8, SelectionButtonRoomy, SelectionButtonWidth)
    val centre = toolbarButtonCentre(screenWidth, layout, if (popup == SelectionPopup.FEATHER) 4 else 5, 8)
    val left = (centre - width / 2).coerceIn(0.dp, screenWidth - width)
    MenuPopover(centre - left, left, toolbarButtonsTop(screenHeight), width) {
        when (popup) {
            SelectionPopup.FEATHER -> {
                MenuTitle("Feather")
                Spacer(Modifier.height(4.dp))
                Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(MenuGroupCorner)).background(MenuGroup).padding(vertical = 8.dp)) {
                    val amount = vm.featherAmount
                    SliderRow("Amount", amount, if (amount <= 0f) "None" else "${(amount * 100).roundToInt()}%") { vm.featherTo(it) }
                }
            }
            SelectionPopup.SAVED -> SavedSelections(vm)
            SelectionPopup.NONE -> Unit
        }
    }
}

@Composable
private fun SavedSelections(vm: CanvasViewModel) {
    Row(Modifier.fillMaxWidth().height(40.dp).padding(start = 14.dp), verticalAlignment = Alignment.CenterVertically) {
        Text("Selections", color = MenuText, fontSize = 24.sp, fontWeight = FontWeight.Bold, letterSpacing = (-0.015).em, maxLines = 1, modifier = Modifier.weight(1f))
        Box(Modifier.size(40.dp, 36.dp).tap { vm.saveSelection() }, contentAlignment = Alignment.Center) {
            Icon(PcIcons.Add, contentDescription = "Save new selection", tint = MenuText, modifier = Modifier.requiredSize(40.dp))
        }
    }
    Spacer(Modifier.height(8.dp))
    LazyColumn(Modifier.fillMaxWidth().height(172.dp), verticalArrangement = Arrangement.spacedBy(1.dp)) {
        items(vm.savedSelections) { i ->
            Row(
                Modifier.fillMaxWidth().height(51.dp).clip(RoundedCornerShape(MenuGroupCorner)).background(MenuGroup).tap { vm.loadSelection(i) },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.size(64.dp, 46.dp).clip(RoundedCornerShape(topStart = MenuGroupCorner, bottomStart = MenuGroupCorner)).background(SavedThumb), contentAlignment = Alignment.Center) {
                    vm.savedThumbnails[i]?.let { Image(it, contentDescription = null, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize()) }
                }
                Spacer(Modifier.width(14.dp))
                MenuLabel("Selection ${i + 1}", modifier = Modifier.weight(1f))
            }
        }
    }
}

/**
 * What the selection tool shows while a shape is made: the rectangle or ellipse being dragged and
 * the freehand path (dashed, its first point a dot to close it on), and the bar and crosshair while
 * a colour selection's threshold is dragged.
 */
@Composable
fun SelectionOverlay(vm: CanvasViewModel, barTop: Dp) {
    val drag = vm.shapeDrag
    val path = vm.freehand
    val threshold = vm.thresholdDrag
    if (drag == null && path.isEmpty() && threshold == null) return
    Canvas(Modifier.fillMaxSize()) {
        drag?.let { (a, b) ->
            val outline = Path().apply {
                val r = Rect(minOf(a.x, b.x), minOf(a.y, b.y), maxOf(a.x, b.x), maxOf(a.y, b.y))
                if (vm.selectionKind == SelectionKind.ELLIPSE) addOval(r) else addRect(r)
            }
            dashed(outline)
        }
        if (path.isNotEmpty()) {
            val points = path.map { vm.canvasToWindow(it) }
            val line = Path().apply {
                moveTo(points[0].x, points[0].y)
                for (p in points.drop(1)) lineTo(p.x, p.y)
            }
            dashed(line)
            drawCircle(StartDot, 5.5.dp.toPx(), points[0])
        }
        threshold?.let { (p, _) ->
            // a thin cross where the colour is picked
            val arm = 8.dp.toPx()
            val w = 1.dp.toPx()
            drawLine(Dash, Offset(p.x - arm, p.y), Offset(p.x + arm, p.y), w)
            drawLine(Dash, Offset(p.x, p.y - arm), Offset(p.x, p.y + arm), w)
        }
    }
    threshold?.let { (_, t) ->
        Box(Modifier.offset(y = barTop).fillMaxWidth().height(24.dp).background(Pc.MessageBar), contentAlignment = Alignment.Center) {
            Text("Selection Threshold ${formatPercent(t)}", color = Pc.MessageText, fontSize = 14.sp)
        }
    }
}

/** A path drawn as the selection's outline: thin dark dashes over light ones. */
private fun DrawScope.dashed(path: Path) {
    val dash = 5.dp.toPx()
    val width = 0.75.dp.toPx()
    drawPath(path, DashGap, style = Stroke(width, pathEffect = PathEffect.dashPathEffect(floatArrayOf(dash, dash), dash)))
    drawPath(path, Dash, style = Stroke(width, pathEffect = PathEffect.dashPathEffect(floatArrayOf(dash, dash), 0f)))
}

/** A threshold (0..1) as a percentage with one decimal. */
private fun formatPercent(t: Float): String = "${(t * 1000).roundToInt() / 10f}%"
