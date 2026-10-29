package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import kotlin.math.roundToInt

private val CompanionWidth = 432.dp
private val CompanionHeight = 89.dp
private val CompanionFill = Color(0xE6404040)
private val CompanionInk = Color(0xFFBEBEBE)
private val CompanionGrab = Color(0xFF8A8A8A)
private val CompanionButtonWidth = 80.dp
private val CompanionButtonGap = 4.dp

/** Where the bar sits at first: its middle across the screen and this far down it. */
private const val COMPANION_START_Y = 0.54f

/**
 * The single touch gestures companion: a floating bar standing in for gestures that need more
 * than one finger. Undo, redo, Zoom and Move (while one is on, a finger dragged on the canvas zooms
 * or moves it), and Fit Canvas. Dragged by its top edge.
 */
@Composable
fun BoxScope.TouchCompanion(vm: CanvasViewModel) {
    if (!vm.singleTouchCompanion) return
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val density = LocalDensity.current
        val (w, h) = with(density) { CompanionWidth.toPx() to CompanionHeight.toPx() }
        val (sw, sh) = with(density) { maxWidth.toPx() to maxHeight.toPx() }
        var at by remember { mutableStateOf(Offset((sw - w) / 2f, sh * COMPANION_START_Y - h / 2f)) }
        Column(
            Modifier.offset { IntOffset(at.x.roundToInt(), at.y.roundToInt()) }.size(CompanionWidth, CompanionHeight)
                .shadow(16.dp, RoundedCornerShape(30.dp)).clip(RoundedCornerShape(30.dp)).background(CompanionFill),
        ) {
            // the grab line, and the strip it sits in, drags the bar about, kept on the screen
            Box(
                Modifier.fillMaxWidth().height(24.dp).pointerInput(sw, sh) {
                    detectDragGestures { change, amount ->
                        change.consume()
                        at = Offset((at.x + amount.x).coerceIn(0f, sw - w), (at.y + amount.y).coerceIn(0f, sh - h))
                    }
                },
                contentAlignment = Alignment.Center,
            ) {
                Box(Modifier.size(40.dp, 5.dp).clip(CircleShape).background(CompanionGrab))
            }
            Row(Modifier.padding(start = 8.dp)) {
                CompanionButton(PcIcons.Undo, "Undo", false, 30.dp) { vm.undo() }
                Spacer(Modifier.width(CompanionButtonGap))
                CompanionButton(PcIcons.Redo, "Redo", false, 30.dp) { vm.redo() }
                Spacer(Modifier.width(CompanionButtonGap))
                CompanionButton(PcIcons.Zoom, "Zoom", vm.companionMode == CanvasViewModel.CompanionMode.ZOOM, 22.dp) {
                    vm.toggleCompanionMode(CanvasViewModel.CompanionMode.ZOOM)
                }
                Spacer(Modifier.width(CompanionButtonGap))
                CompanionButton(PcIcons.MoveCanvas, "Move", vm.companionMode == CanvasViewModel.CompanionMode.MOVE, 22.dp) {
                    vm.toggleCompanionMode(CanvasViewModel.CompanionMode.MOVE)
                }
                Spacer(Modifier.width(CompanionButtonGap))
                CompanionButton(PcIcons.FitCanvas, "Fit Canvas", false, 22.dp) { vm.settleToFit() }
            }
        }
    }
}

/** A button of the bar: its icon (blue while its mode is on) over its name. */
@Composable
private fun CompanionButton(icon: ImageVector, label: String, on: Boolean, iconSize: androidx.compose.ui.unit.Dp, onClick: () -> Unit) {
    Column(
        Modifier.size(CompanionButtonWidth, 50.dp).clip(RoundedCornerShape(10.dp)).clickable(onClick = onClick),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(Modifier.height(30.dp), contentAlignment = Alignment.Center) {
            Icon(icon, contentDescription = null, tint = if (on) Pc.Accent else CompanionInk, modifier = Modifier.size(iconSize))
        }
        Text(label, color = CompanionInk, fontSize = 13.sp, maxLines = 1)
    }
}
