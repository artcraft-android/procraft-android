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
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
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
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
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
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.exportFpsAt
import io.github.artcraftandroid.procraft.core.exportFpsFraction
import io.github.artcraftandroid.procraft.core.exportFpsLabel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.math.roundToInt

private val ScreenFill get() = if (Pc.light) Color(0xFFF2F2F2) else Color(0xFF1F1F1F)
private val TitleInk get() = if (Pc.light) Color(0xFF3D3D3D) else Color(0xFFAAAAAA)
private val RowInk get() = if (Pc.light) Color(0xFF595959) else Color(0xFFC1C1C1)
private val PanelFill get() = if (Pc.light) Color(0xFFEBEBEB) else Color(0xFF262626)
private val PanelEdge get() = if (Pc.light) Color(0xFFDADADA) else Color(0xFF363636)
private val SegmentTrack get() = if (Pc.light) Color(0xFFD5D5D5) else Color(0xFF3C3C3C)
private val CapsuleFill get() = if (Pc.light) Color(0xFFFBFBFB) else Color(0xFF1A1A1A)
private val CapsuleEdge get() = if (Pc.light) Color(0xFFE0E0E0) else Color(0xFF373737)
private val DoneBlue get() = Pc.tinted(Color(0xFF0190FF))

/**
 * Sharing the layers as an animation: the frames playing, the settings (the size, the frame rate,
 * and per format dithering and a transparent background), Cancel, and the round Done that exports
 * and shares. The file is made as the settings change, to show its size.
 */
@Composable
fun AnimatedExport(vm: CanvasViewModel, format: CanvasViewModel.Animated) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var webReady by remember { mutableStateOf(false) }
    var dither by remember { mutableStateOf(true) }
    var perFramePalette by remember { mutableStateOf(false) }
    var transparent by remember { mutableStateOf(false) }
    // where the frame rate slider is, starting at the artwork's own rate; the rate as it shows it
    var fpsAt by remember { mutableFloatStateOf(exportFpsFraction(vm.artworkFps())) }
    val fps = exportFpsAt(fpsAt)
    var exporting by remember { mutableStateOf(false) }
    val canTransparent = format == CanvasViewModel.Animated.GIF || format == CanvasViewModel.Animated.PNG
    // the file as the settings make it, for its size
    var bytes by remember { mutableStateOf<Long?>(null) }
    LaunchedEffect(webReady, dither, perFramePalette, transparent, fps) {
        bytes = null
        bytes = vm.exportAnimation(format, webReady, dither, perFramePalette, transparent, fps)?.length()
    }
    ExportScreen(
        title = format.title,
        name = vm.artworkTitle(),
        size = bytes?.let(::fileSize),
        exporting = exporting,
        onCancel = { vm.endAnimatedExport() },
        onDone = {
            exporting = true
            scope.launch {
                // the export card shows how far it has come, and says if it couldn't be made
                val file = vm.shareAnimation(format, webReady, dither, perFramePalette, transparent, fps)
                exporting = false
                if (file != null) {
                    vm.endAnimatedExport()
                    shareFile(context, file, format.mime)
                }
            }
        },
        preview = { side ->
            // the frames, fitted to the box, played over and over
            var frames by remember { mutableStateOf<List<Bitmap>>(emptyList()) }
            var shown by remember { mutableIntStateOf(0) }
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
            frames.getOrNull(shown)?.let { Image(it.asImageBitmap(), contentDescription = "Preview", contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize()) }
        },
    ) {
        Segments(listOf("Max Resolution", "Web Ready"), if (webReady) 1 else 0) { webReady = it == 1 }
        FpsSlider(fpsAt, exportFpsLabel(fps)) { fpsAt = it }
        if (format == CanvasViewModel.Animated.GIF) {
            SettingRow("Dithering", dither) { dither = it }
            SettingRow("Per frame color palette", perFramePalette) { perFramePalette = it }
        }
        if (canTransparent) SettingRow("Transparent background", transparent) { transparent = it }
    }
}

/** A file's size as the export screen shows it: "1.3 MB", "850 KB". */
private fun fileSize(bytes: Long): String =
    if (bytes >= 1_000_000) "%.1f MB".format(bytes / 1_000_000.0) else "${(bytes / 1000).coerceAtLeast(1)} KB"

/**
 * An export screen. Wide, the settings stay in a 400-wide panel on the left, and the artwork's
 * name and the file's [size] sit over the preview, which fills the rest (41 in at the sides,
 * 110 down). Upright, "Export Settings" opens the panel over a square preview (24 in), a tap
 * beside it closing it again. Cancel and the round Done are at the top right.
 */
@Composable
private fun ExportScreen(
    title: String,
    name: String,
    size: String?,
    exporting: Boolean,
    onCancel: () -> Unit,
    onDone: () -> Unit,
    preview: @Composable (side: Int) -> Unit,
    panel: @Composable ColumnScope.() -> Unit,
) {
    var settingsOpen by remember { mutableStateOf(false) }
    BoxWithConstraints(Modifier.fillMaxSize().background(ScreenFill).pointerInput(Unit) { detectTapGestures { } }) {
        val wide = maxWidth > maxHeight
        BackHandler { if (settingsOpen && !wide) settingsOpen = false else onCancel() }
        val left = if (wide) PanelWidth + 10.dp else 0.dp
        val inset = if (wide) 41.dp else 24.dp
        val boxWidth = maxWidth - left - inset * 2
        val boxHeight = if (wide) minOf(boxWidth, maxHeight - 104.dp) else boxWidth
        val side = with(LocalDensity.current) { minOf(boxWidth, boxHeight).roundToPx() }
        Box(Modifier.offset(x = left + inset, y = 110.dp).size(boxWidth, boxHeight), contentAlignment = Alignment.Center) { preview(side) }
        if (wide) {
            // the name and size centred between the panel and the buttons
            Column(
                Modifier.offset(x = left + 24.dp, y = 23.dp).width(maxWidth - left - 24.dp - 224.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(name, color = TitleInk, fontSize = 20.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.height(30.dp))
                Text(size ?: "", color = SizeInk, fontSize = 13.sp, maxLines = 1, modifier = Modifier.height(20.dp))
            }
        } else {
            Row(Modifier.offset(x = 14.dp, y = 20.dp).clickable { settingsOpen = !settingsOpen }.padding(end = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.width(60.dp), contentAlignment = Alignment.Center) {
                    Icon(PcIcons.ChevronLeft, contentDescription = null, tint = TitleInk, modifier = Modifier.size(28.dp))
                }
                // on two lines, as there
                Text("Export\nSettings", color = TitleInk, fontSize = 24.sp, fontWeight = FontWeight.Bold, lineHeight = 29.sp)
            }
        }
        Row(Modifier.align(Alignment.TopEnd).padding(top = 16.dp, end = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier.height(43.dp).clip(CircleShape).background(CapsuleFill).border(1.dp, CapsuleEdge, CircleShape).clickable(onClick = onCancel).padding(horizontal = 13.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text("Cancel", color = Color(0xFFA5A5A5), fontSize = 17.sp)
            }
            Spacer(Modifier.width(13.dp))
            Box(
                Modifier.size(44.dp, 45.dp).clip(CircleShape).background(DoneBlue).clickable(enabled = !exporting, onClick = onDone),
                contentAlignment = Alignment.Center,
            ) {
                // while it exports the card over the screen shows how far it has come
                Icon(PcIcons.DoneTick, contentDescription = "Export", tint = Color.White, modifier = Modifier.requiredSize(40.dp))
            }
        }
        if (wide || settingsOpen) {
            // upright, a tap outside the panel closes it
            if (!wide) Box(Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures { settingsOpen = false } })
            Column(
                Modifier.padding(10.dp).width(PanelWidth).fillMaxHeight().clip(RoundedCornerShape(20.dp)).background(PanelFill).border(1.dp, PanelEdge, RoundedCornerShape(20.dp))
                    .pointerInput(Unit) { detectTapGestures { } },
            ) {
                Text(title, color = TitleInk, fontSize = 24.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.padding(start = 24.dp, top = 19.dp).height(36.dp))
                Spacer(Modifier.height(11.5.dp))
                panel()
            }
        }
    }
}

private val PanelWidth = 400.dp
private val SizeInk get() = if (Pc.light) Color(0xFF3F3F3F) else Color(0xFF8E8E8E)

/**
 * Sharing a model on a turntable: it turning as it will, the settings (the size, once round or
 * swinging, how long a turn takes, how far the camera pulls back, the easing, and whether the
 * environment shows), Cancel, and the round Done that exports and shares.
 */
@Composable
fun ModelAnimatedExport(vm: CanvasViewModel, format: CanvasViewModel.Animated) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var exporting by remember { mutableStateOf(false) }
    ExportScreen(
        title = format.title,
        name = vm.artworkTitle(),
        size = null,
        exporting = exporting,
        onCancel = { vm.modelAnimatedExport = null },
        onDone = {
            exporting = true
            scope.launch {
                val file = vm.shareModelAnimation(format)
                exporting = false
                if (file != null) {
                    vm.modelAnimatedExport = null
                    shareFile(context, file, format.mime)
                }
            }
        },
        preview = { side ->
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
            frames.getOrNull(shown)?.let { Image(it.asImageBitmap(), contentDescription = "Preview", contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize()) }
        },
    ) {
        val t = vm.turntable
        Segments(listOf("720p", "1080p", "4K", "Square"), vm.turntableSize) { vm.turntableSize = it }
        Spacer(Modifier.height(14.5.dp))
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

/** The turntable preview's frame rate. */
private const val PREVIEW_FPS = 15

/** Two or more choices side by side in a grey track 384 wide, 12 apart, the chosen one blue. */
@Composable
private fun Segments(labels: List<String>, chosen: Int, onChoose: (Int) -> Unit) {
    Row(
        Modifier.padding(start = 7.5.dp).width(384.dp).height(34.dp).clip(CircleShape).background(SegmentTrack).padding(2.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
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

private val ChoiceFill get() = if (Pc.light) Color(0xFFE3E3E3) else Color(0xFF2E2E2E)
private val SliderTrack get() = if (Pc.light) Color(0xFFDDDDDD) else Color(0xFF3E3E3E)


/**
 * "Frames per second" under the size's bar, down to the switches: its name and the rate [value]
 * (caps at y 132.5, the value's ink 24 in from the panel's right), over a 3-tall track (its middle
 * at y 160.5) blue up to a small white knob at [fraction], as measured. Dragged anywhere along it,
 * the knob follows.
 */
@Composable
private fun FpsSlider(fraction: Float, value: String, onChange: (Float) -> Unit) {
    val change by androidx.compose.runtime.rememberUpdatedState(onChange)
    val current by androidx.compose.runtime.rememberUpdatedState(fraction)
    Box(Modifier.fillMaxWidth().height(FpsRowHeight)) {
        Text("Frames per second", color = RowInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.padding(start = FpsLabelX, top = FpsLabelY))
        Text(value, color = RowInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.align(Alignment.TopEnd).padding(end = FpsValueRight, top = FpsLabelY))
        androidx.compose.foundation.Canvas(
            Modifier.fillMaxWidth().height(28.dp).offset(y = FpsTrackY - 14.dp).pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    val travel = size.width - 22.5.dp.toPx() - 23.dp.toPx()
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
            val thick = 3.dp.toPx()
            val (left, right) = 18.dp.toPx() to size.width - 18.dp.toPx()
            // the knob's middle from 22.5 in to 23 in
            val (x0, x1) = 22.5.dp.toPx() to size.width - 23.dp.toPx()
            val x = x0 + (x1 - x0) * current.coerceIn(0f, 1f)
            val corner = androidx.compose.ui.geometry.CornerRadius(thick / 2f)
            drawRoundRect(FpsTrack, androidx.compose.ui.geometry.Offset(left, y - thick / 2f), androidx.compose.ui.geometry.Size(right - left, thick), corner)
            drawRoundRect(Pc.Accent, androidx.compose.ui.geometry.Offset(left, y - thick / 2f), androidx.compose.ui.geometry.Size(x - left, thick), corner)
            // 16 x 10, corners 4, over a faint shadow
            val (w, h) = 16.dp.toPx() to 10.dp.toPx()
            val knob = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply {
                color = android.graphics.Color.WHITE
                setShadowLayer(1.5.dp.toPx(), 0f, 1.dp.toPx(), 0x30000000)
            }
            drawContext.canvas.nativeCanvas.drawRoundRect(x - w / 2f, y - h / 2f, x + w / 2f, y + h / 2f, 4.dp.toPx(), 4.dp.toPx(), knob)
        }
    }
}

/**
 * Where "Frames per second" starts and its value ends (in from the panel's sides), its top under
 * the bar, the track's middle under the bar, and how far under the bar the switches start.
 */
private val FpsLabelX = 18.dp
private val FpsValueRight = 22.dp
private val FpsLabelY = 18.dp
private val FpsTrackY = 50.dp
private val FpsRowHeight = 72.5.dp
private val FpsTrack get() = if (Pc.light) Color(0xFFDDDDDD) else Color(0xFF333333)

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
