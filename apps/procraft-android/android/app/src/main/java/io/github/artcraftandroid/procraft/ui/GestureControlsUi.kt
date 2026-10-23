package io.github.artcraftandroid.procraft.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
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
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.GestureOption
import io.github.artcraftandroid.procraft.canvas.GestureOptions
import io.github.artcraftandroid.procraft.canvas.GesturePurpose
import io.github.artcraftandroid.procraft.canvas.GestureSettings

private val ScreenFill = Color(0xFF1F1F1F)
private val RailFill = Color(0xFF272727)
private val CardFill = Color(0xFF252525)
private val Ink = Color(0xFFC1C1C1)
private val Detail = Color(0xFF9A9A9A)
private val RailIdle = Color(0xFFAAAAAA)
private val Track = Color(0xFF414141)
// a delay whose way of touching is off: a dark line and a dim knob, no blue
private val TrackOff = Color(0xFF1D1D1D)
private val KnobOff = Color(0xFF333333)
private val DoneBlue = Color(0xFF0190FF)

private fun icon(p: GesturePurpose): ImageVector = when (p) {
    GesturePurpose.SMUDGE -> PcIcons.Smudge
    GesturePurpose.ERASE -> PcIcons.Erase
    GesturePurpose.ASSISTED -> PcIcons.GestureAssisted
    GesturePurpose.EYEDROPPER -> PcIcons.GestureEyedropper
    GesturePurpose.QUICKSHAPE -> PcIcons.GestureQuickShape
    GesturePurpose.QUICKMENU -> PcIcons.GestureQuickMenu
    GesturePurpose.FULL_SCREEN -> PcIcons.GestureFullScreen
    GesturePurpose.CLEAR_LAYER -> PcIcons.GestureClearLayer
    GesturePurpose.COPY_PASTE -> PcIcons.GestureCopyPaste
    GesturePurpose.LAYER_SELECT -> PcIcons.GestureLayerSelect
    GesturePurpose.HOVER -> PcIcons.GestureHover
    GesturePurpose.GENERAL -> PcIcons.GestureGeneral
}

/**
 * Gesture controls, full screen: a rail of what gestures can do and, for the one chosen, a card of
 * switches for the ways of touching that do it (a delay under the held ones), and Done.
 */
@Composable
fun GestureControls(vm: CanvasViewModel) {
    var page by remember { mutableStateOf(GesturePurpose.EYEDROPPER) }
    var resetting by remember { mutableStateOf(false) }
    val settings = vm.gestures
    BackHandler { vm.gestureControlsOpen = false }
    Box(Modifier.fillMaxSize().background(ScreenFill).pointerInput(Unit) { detectTapGestures { } }) {
        Column(Modifier.width(81.dp).fillMaxHeight().background(RailFill).verticalScroll(rememberScrollState()).padding(start = 8.dp, top = 46.dp, bottom = 20.dp)) {
            for (p in GesturePurpose.entries) {
                val chosen = p == page
                Box(
                    Modifier.size(65.dp, 53.dp).clip(CircleShape).background(if (chosen) Pc.Accent else Color.Transparent).clickable { page = p },
                    contentAlignment = Alignment.Center,
                ) {
                    // each drawn in a padded frame, fitted 37 across
                    Icon(icon(p), contentDescription = p.title, tint = if (chosen) Color.White else RailIdle, modifier = Modifier.requiredSize(37.dp))
                }
                Spacer(Modifier.height(3.dp))
            }
        }
        Text(page.heading, color = Ink, fontSize = 20.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.offset(x = 101.dp, y = 28.dp))
        Box(
            Modifier.align(Alignment.TopEnd).padding(top = 20.dp, end = 20.dp).size(44.dp, 45.dp).clip(CircleShape).background(DoneBlue).clickable { vm.gestureControlsOpen = false },
            contentAlignment = Alignment.Center,
        ) {
            Icon(PcIcons.DoneTick, contentDescription = "Done", tint = Color.White, modifier = Modifier.requiredSize(40.dp))
        }
        Column(Modifier.padding(start = 105.dp, end = 24.dp, top = 100.dp).verticalScroll(rememberScrollState())) {
            Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(25.dp)).background(CardFill)) {
                val options = GestureOptions[page].orEmpty()
                options.forEachIndexed { i, o ->
                    if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(ScreenFill))
                    OptionRow(o, settings.isOn(page, o.key)) { settings.set(page, o.key, it) }
                    if (o.delayed) DelayRow(settings.delay(page), enabled = settings.isOn(page, o.key)) { settings.setDelay(page, it) }
                }
            }
            // under General's settings, everything back as it ships
            if (page == GesturePurpose.GENERAL) {
                Spacer(Modifier.height(14.dp))
                Box(
                    Modifier.fillMaxWidth().height(50.dp).clip(RoundedCornerShape(25.dp)).background(CardFill).clickable { resetting = true },
                    contentAlignment = Alignment.Center,
                ) {
                    Text("Reset to defaults", color = Ink, fontSize = 15.sp, maxLines = 1)
                }
            }
            Spacer(Modifier.height(40.dp))
        }
    }
    Alert(
        if (resetting) Unit else null,
        title = { "Reset gesture controls?" },
        message = { "Every gesture goes back to how the app starts out." },
        action = "Reset",
        onCancel = { resetting = false },
        onAction = {
            settings.resetAll()
            resetting = false
        },
    )
}

/** A way of touching: its name and what it does, and its switch. */
@Composable
private fun OptionRow(o: GestureOption, on: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().height(72.dp).clickable { onChange(!on) }.padding(start = 28.dp, end = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(o.label, color = Ink, fontSize = 18.sp, fontWeight = FontWeight.Medium, maxLines = 1)
            Text(o.detail, color = Detail, fontSize = 13.sp, maxLines = 1)
        }
        Switch(on, enabled = true)
    }
}

/** How long a hold waits, 0.1 to 1.5 seconds, dragged or tapped along the bar; greyed while its way of touching is off. */
@Composable
private fun DelayRow(seconds: Float, enabled: Boolean, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    val span = GestureSettings.MAX_DELAY_S - GestureSettings.MIN_DELAY_S
    val fraction = ((seconds - GestureSettings.MIN_DELAY_S) / span).coerceIn(0f, 1f)
    Row(Modifier.fillMaxWidth().height(44.dp).padding(start = 28.dp, end = 24.dp), verticalAlignment = Alignment.CenterVertically) {
        Text("Delay", color = Ink, fontSize = 15.sp, modifier = Modifier.width(74.dp))
        Box(
            Modifier.weight(1f).fillMaxHeight()
                .then(
                    if (!enabled) {
                        Modifier
                    } else {
                        Modifier.pointerInput(Unit) {
                            fun at(x: Float) = GestureSettings.MIN_DELAY_S + (x / size.width).coerceIn(0f, 1f) * span
                            detectTapGestures { change(at(it.x)) }
                        }.pointerInput(Unit) {
                            fun at(x: Float) = GestureSettings.MIN_DELAY_S + (x / size.width).coerceIn(0f, 1f) * span
                            detectHorizontalDragGestures(onDragStart = { change(at(it.x)) }) { c, _ ->
                                c.consume()
                                change(at(c.position.x))
                            }
                        }
                    },
                )
                .drawBehind {
                    val y = size.height / 2f
                    val x = fraction * size.width
                    if (enabled) {
                        val h = 3.dp.toPx()
                        drawRect(Track, Offset(0f, y - h / 2f), Size(size.width, h))
                        drawRect(Pc.Accent, Offset(0f, y - h / 2f), Size(x, h))
                        val knob = Size(16.dp.toPx(), 10.dp.toPx())
                        drawRoundRect(Color.White, Offset(x - knob.width / 2f, y - knob.height / 2f), knob, CornerRadius(knob.height / 2f))
                    } else {
                        val h = 1.5.dp.toPx()
                        drawRect(TrackOff, Offset(0f, y - h / 2f), Size(size.width, h))
                        val knob = Size(16.dp.toPx(), 10.dp.toPx())
                        drawRoundRect(KnobOff, Offset(x - knob.width / 2f, y - knob.height / 2f), knob, CornerRadius(knob.height / 2f))
                    }
                },
        )
        Text("%.2fs".format(seconds), color = Ink, fontSize = 15.sp, modifier = Modifier.padding(start = 16.dp))
    }
}
