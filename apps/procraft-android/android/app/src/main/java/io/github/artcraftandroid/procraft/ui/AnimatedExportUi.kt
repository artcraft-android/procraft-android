package io.github.artcraftandroid.procraft.ui

import android.graphics.Bitmap
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.math.roundToInt

private val ScreenFill = Color(0xFF1F1F1F)
private val TitleInk = Color(0xFFAAAAAA)
private val RowInk = Color(0xFFC1C1C1)
private val PanelFill = Color(0xFF262626)
private val PanelEdge = Color(0xFF363636)
private val SegmentTrack = Color(0xFF3C3C3C)
private val CapsuleFill = Color(0xFF1A1A1A)
private val CapsuleEdge = Color(0xFF373737)
private val DoneBlue = Color(0xFF0190FF)

/**
 * Sharing the layers as an animation: the frames playing in a square, Export Settings (the size,
 * and per format dithering and a transparent background) in a panel on the left, Cancel, and
 * the round Done that exports and shares.
 */
@Composable
fun AnimatedExport(vm: CanvasViewModel, format: CanvasViewModel.Animated) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var settingsOpen by remember { mutableStateOf(false) }
    var webReady by remember { mutableStateOf(false) }
    var dither by remember { mutableStateOf(true) }
    var transparent by remember { mutableStateOf(false) }
    var exporting by remember { mutableStateOf(false) }
    fun cancel() {
        if (settingsOpen) settingsOpen = false else vm.endAnimatedExport()
    }
    BackHandler { cancel() }
    BoxWithConstraints(Modifier.fillMaxSize().background(ScreenFill).pointerInput(Unit) { detectTapGestures { } }) {
        val box = maxWidth - 48.dp
        val side = with(LocalDensity.current) { box.roundToPx() }
        // the frames, fitted to the square, played over and over
        var frames by remember { mutableStateOf<List<Bitmap>>(emptyList()) }
        var shown by remember { mutableIntStateOf(0) }
        val canTransparent = format == CanvasViewModel.Animated.GIF || format == CanvasViewModel.Animated.PNG
        LaunchedEffect(transparent && canTransparent, side) {
            val loaded = withContext(Dispatchers.IO) { List(vm.animationFrameCount()) { vm.animationFrame(it, side, transparent && canTransparent) }.filterNotNull() }
            frames = loaded
            shown = 0
        }
        LaunchedEffect(frames) {
            while (frames.size > 1) {
                delay(1000L / CanvasViewModel.ANIMATION_FPS)
                shown = (shown + 1) % frames.size
            }
        }
        Box(Modifier.offset(x = 24.dp, y = 96.dp).size(box), contentAlignment = Alignment.Center) {
            frames.getOrNull(shown)?.let { Image(it.asImageBitmap(), contentDescription = "Preview", contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize()) }
        }
        // the header: Export Settings on the left, Cancel and Done on the right
        Row(Modifier.offset(x = 14.dp, y = 20.dp).height(44.dp).clickable { settingsOpen = !settingsOpen }.padding(end = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.width(60.dp), contentAlignment = Alignment.Center) {
                Icon(PcIcons.ChevronLeft, contentDescription = null, tint = TitleInk, modifier = Modifier.size(28.dp))
            }
            Text("Export Settings", color = TitleInk, fontSize = 24.sp, fontWeight = FontWeight.Bold, maxLines = 1)
        }
        Row(Modifier.align(Alignment.TopEnd).padding(top = 16.dp, end = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier.height(43.dp).clip(CircleShape).background(CapsuleFill).border(1.dp, CapsuleEdge, CircleShape).clickable { cancel() }.padding(horizontal = 13.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text("Cancel", color = Color(0xFFA5A5A5), fontSize = 17.sp)
            }
            Spacer(Modifier.width(13.dp))
            Box(
                Modifier.size(44.dp, 45.dp).clip(CircleShape).background(DoneBlue).clickable(enabled = !exporting) {
                    exporting = true
                    scope.launch {
                        val file = vm.exportAnimation(format, webReady, dither, transparent)
                        exporting = false
                        if (file != null) {
                            vm.endAnimatedExport()
                            shareFile(context, file, format.mime)
                        } else {
                            vm.message = "The animation could not be exported."
                        }
                    }
                },
                contentAlignment = Alignment.Center,
            ) {
                if (exporting) CircularProgressIndicator(color = Color.White, strokeWidth = 2.dp, modifier = Modifier.size(24.dp))
                else Icon(PcIcons.DoneTick, contentDescription = "Export", tint = Color.White, modifier = Modifier.requiredSize(40.dp))
            }
        }
        if (settingsOpen) {
            // a tap outside the panel closes it
            Box(Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures { settingsOpen = false } })
            Column(
                Modifier.padding(10.dp).width(400.dp).fillMaxHeight().clip(RoundedCornerShape(20.dp)).background(PanelFill).border(1.dp, PanelEdge, RoundedCornerShape(20.dp))
                    .pointerInput(Unit) { detectTapGestures { } },
            ) {
                Text(format.title, color = TitleInk, fontSize = 24.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.padding(start = 24.dp, top = 15.dp).height(36.dp))
                Spacer(Modifier.height(8.dp))
                Segments(listOf("Max Resolution", "Web Ready"), if (webReady) 1 else 0) { webReady = it == 1 }
                Spacer(Modifier.height(15.dp))
                if (format == CanvasViewModel.Animated.GIF) SettingRow("Dithering", dither) { dither = it }
                if (canTransparent) SettingRow("Transparent background", transparent) { transparent = it }
            }
        }
    }
}

/** Two or more choices side by side in a grey track, the chosen one blue. */
@Composable
private fun Segments(labels: List<String>, chosen: Int, onChoose: (Int) -> Unit) {
    Row(Modifier.padding(start = 9.dp).width(382.dp).height(33.dp).clip(CircleShape).background(SegmentTrack).padding(1.5.dp)) {
        labels.forEachIndexed { i, label ->
            Box(
                Modifier.weight(1f).fillMaxHeight().clip(CircleShape).background(if (i == chosen) Pc.Accent else Color.Transparent).clickable { onChoose(i) },
                contentAlignment = Alignment.Center,
            ) {
                Text(label, color = if (i == chosen) Color.White else RowInk, fontSize = 15.sp, maxLines = 1)
            }
        }
    }
}

/** A setting's name and its switch, 52 tall. */
@Composable
private fun SettingRow(label: String, on: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.height(52.dp).clickable { onChange(!on) }.padding(start = 16.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = RowInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.width(307.dp))
        Switch(on, enabled = true)
    }
}

private val ChoiceFill = Color(0xFF2E2E2E)
private val SliderTrack = Color(0xFF3E3E3E)

/**
 * Sharing a model on a turntable: it turning in a square as it will, Export Settings (the size,
 * once round or swinging, how long a turn takes, how far the camera pulls back, the easing, and
 * whether the environment shows) in a panel on the left, Cancel, and the round Done that exports
 * and shares.
 */
@Composable
fun ModelAnimatedExport(vm: CanvasViewModel, format: CanvasViewModel.Animated) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var settingsOpen by remember { mutableStateOf(false) }
    var exporting by remember { mutableStateOf(false) }
    fun cancel() {
        if (settingsOpen) settingsOpen = false else vm.modelAnimatedExport = null
    }
    BackHandler { cancel() }
    BoxWithConstraints(Modifier.fillMaxSize().background(ScreenFill).pointerInput(Unit) { detectTapGestures { } }) {
        val box = minOf(maxWidth - 48.dp, maxHeight - 120.dp)
        val side = with(LocalDensity.current) { box.roundToPx() }
        // the turntable's frames, made again whenever its settings change, played over and over
        var frames by remember { mutableStateOf<List<Bitmap>>(emptyList()) }
        var shown by remember { mutableIntStateOf(0) }
        LaunchedEffect(vm.turntable, side) {
            frames = vm.turntablePreview(side / 2, PREVIEW_FPS)
            shown = 0
        }
        LaunchedEffect(frames) {
            while (frames.size > 1) {
                delay(1000L / PREVIEW_FPS)
                shown = (shown + 1) % frames.size
            }
        }
        Box(Modifier.align(Alignment.TopCenter).offset(y = 96.dp).size(box), contentAlignment = Alignment.Center) {
            frames.getOrNull(shown)?.let { Image(it.asImageBitmap(), contentDescription = "Preview", contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize()) }
        }
        Row(Modifier.offset(x = 14.dp, y = 20.dp).height(44.dp).clickable { settingsOpen = !settingsOpen }.padding(end = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.width(60.dp), contentAlignment = Alignment.Center) {
                Icon(PcIcons.ChevronLeft, contentDescription = null, tint = TitleInk, modifier = Modifier.size(28.dp))
            }
            Text("Export Settings", color = TitleInk, fontSize = 24.sp, fontWeight = FontWeight.Bold, maxLines = 1)
        }
        Row(Modifier.align(Alignment.TopEnd).padding(top = 16.dp, end = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier.height(43.dp).clip(CircleShape).background(CapsuleFill).border(1.dp, CapsuleEdge, CircleShape).clickable { cancel() }.padding(horizontal = 13.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text("Cancel", color = Color(0xFFA5A5A5), fontSize = 17.sp)
            }
            Spacer(Modifier.width(13.dp))
            Box(
                Modifier.size(44.dp, 45.dp).clip(CircleShape).background(DoneBlue).clickable(enabled = !exporting) {
                    exporting = true
                    scope.launch {
                        val file = vm.exportModelAnimation(format)
                        exporting = false
                        if (file != null) {
                            vm.modelAnimatedExport = null
                            shareFile(context, file, format.mime)
                        } else {
                            vm.message = "The animation could not be exported."
                        }
                    }
                },
                contentAlignment = Alignment.Center,
            ) {
                if (exporting) CircularProgressIndicator(color = Color.White, strokeWidth = 2.dp, modifier = Modifier.size(24.dp))
                else Icon(PcIcons.DoneTick, contentDescription = "Export", tint = Color.White, modifier = Modifier.requiredSize(40.dp))
            }
        }
        if (settingsOpen) {
            Box(Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures { settingsOpen = false } })
            val t = vm.turntable
            Column(
                Modifier.padding(10.dp).width(400.dp).fillMaxHeight().clip(RoundedCornerShape(20.dp)).background(PanelFill).border(1.dp, PanelEdge, RoundedCornerShape(20.dp))
                    .pointerInput(Unit) { detectTapGestures { } },
            ) {
                Text(format.title, color = TitleInk, fontSize = 24.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.padding(start = 24.dp, top = 15.dp).height(36.dp))
                Spacer(Modifier.height(8.dp))
                Segments(listOf("720p", "1080p", "4K", "Square"), vm.turntableSize) { vm.turntableSize = it }
                Spacer(Modifier.height(23.dp))
                Column(Modifier.padding(horizontal = 16.dp).clip(RoundedCornerShape(14.dp)).background(ChoiceFill)) {
                    ChoiceRow("Animated rotate", !t.swing) { vm.turntable = t.copy(swing = false) }
                    ChoiceRow("Animated swing", t.swing) { vm.turntable = t.copy(swing = true) }
                }
                Spacer(Modifier.height(26.dp))
                val seconds = if (t.seconds < 1f) "%.1f".format(t.seconds) else "${t.seconds.roundToInt()}"
                TurntableSlider("Animation duration", seconds, (t.seconds - 0.5f) / 4.5f) { vm.turntable = vm.turntable.copy(seconds = 0.5f + it * 4.5f) }
                TurntableSlider("Zoom distance", "${(t.zoom * 100).roundToInt()}%", t.zoom) { vm.turntable = vm.turntable.copy(zoom = it) }
                TurntableSlider("Ease", "${(t.ease * 100).roundToInt()}%", t.ease) { vm.turntable = vm.turntable.copy(ease = it) }
                Spacer(Modifier.height(10.dp))
                SettingRow("Show environment", t.environment) { vm.turntable = vm.turntable.copy(environment = it) }
            }
        }
    }
}

/** The turntable preview's frame rate. */
private const val PREVIEW_FPS = 15

/** One of two choices in a group, 40 tall: the chosen one blue with a tick. */
@Composable
private fun ChoiceRow(label: String, chosen: Boolean, onChoose: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().height(40.dp).background(if (chosen) Pc.Accent else Color.Transparent).clickable(onClick = onChoose).padding(start = 16.dp, end = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = if (chosen) Color.White else RowInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
        if (chosen) {
            Box(Modifier.size(20.dp).clip(CircleShape).background(Color.White), contentAlignment = Alignment.Center) {
                Icon(PcIcons.Tick, contentDescription = null, tint = Pc.Accent, modifier = Modifier.size(14.dp))
            }
        }
    }
}

/** A setting from 0 to 1, 66 tall: its name and value over a track filled blue up to its white knob. */
@Composable
private fun TurntableSlider(label: String, value: String, fraction: Float, onChange: (Float) -> Unit) {
    val change by androidx.compose.runtime.rememberUpdatedState(onChange)
    val current by androidx.compose.runtime.rememberUpdatedState(fraction)
    Box(Modifier.padding(start = 8.dp, end = 8.dp).fillMaxWidth().height(66.dp)) {
        Text(label, color = RowInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.padding(start = 16.dp, top = 6.dp))
        Text(value, color = RowInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.align(Alignment.TopEnd).padding(end = 16.dp, top = 6.dp))
        androidx.compose.foundation.Canvas(
            Modifier.fillMaxWidth().height(28.dp).align(Alignment.BottomStart).offset(y = (-8).dp).pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    val travel = size.width - 2 * 16.dp.toPx()
                    var last = down.position.x
                    while (true) {
                        val ev = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!ev.pressed) break
                        change((current + (ev.position.x - last) / travel).coerceIn(0f, 1f))
                        last = ev.position.x
                        ev.consume()
                    }
                }
            },
        ) {
            val y = size.height / 2f
            val inset = 16.dp.toPx()
            val thick = 3.dp.toPx()
            val x = inset + (size.width - 2 * inset) * current.coerceIn(0f, 1f)
            drawRoundRect(SliderTrack, androidx.compose.ui.geometry.Offset(inset, y - thick / 2f), androidx.compose.ui.geometry.Size(size.width - 2 * inset, thick), androidx.compose.ui.geometry.CornerRadius(thick / 2f))
            drawRoundRect(Pc.Accent, androidx.compose.ui.geometry.Offset(inset, y - thick / 2f), androidx.compose.ui.geometry.Size(x - inset, thick), androidx.compose.ui.geometry.CornerRadius(thick / 2f))
            val (w, h) = 26.dp.toPx() to 16.dp.toPx()
            drawRoundRect(Color.White, androidx.compose.ui.geometry.Offset(x - w / 2f, y - h / 2f), androidx.compose.ui.geometry.Size(w, h), androidx.compose.ui.geometry.CornerRadius(h / 2f))
        }
    }
}
