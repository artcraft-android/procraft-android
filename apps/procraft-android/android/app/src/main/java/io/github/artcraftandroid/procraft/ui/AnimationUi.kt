package io.github.artcraftandroid.procraft.ui

import androidx.compose.animation.core.animate
import androidx.compose.animation.core.calculateTargetValue
import androidx.compose.animation.core.exponentialDecay
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.layout
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInParent
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.AnimationPlayback
import kotlin.math.roundToInt
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/** The bar is 657 wide on a wide screen, 446 upright; the timeline 12 in from each side. */
private val WideBarWidth = 657.dp
private val UprightBarWidth = 446.dp
private val BarHeight = 96.dp
private val BarGap = 25.dp
private val FrameHeight = 40.dp
private val BarFill get() = if (Pc.light) Color(0xFFE4E4E4) else Color(0xFF2A2A2A)
private val TrackFill get() = if (Pc.light) Color(0xFFD0D0D0) else Color(0xFF474747)
private val TrackEdge get() = if (Pc.light) Color(0xFFBDBDBD) else Color(0xFF5C5C5C)
private val BarInk get() = if (Pc.light) Color(0xFF393939) else Color(0xFFC5C5C5)
private const val MAX_ONION = 12

/** Play's and Add Frame's inset from the bar's ends. */
private val BarInset = 26.dp

/**
 * Animation Assist's bar at the bottom: Play, Settings and Add Frame over the timeline of frames,
 * the current one in the middle; with its settings or the current frame's options above it.
 */
@Composable
fun BoxScope.AnimationBar(vm: CanvasViewModel) {
    if (vm.animation == null) return
    val screen = androidx.compose.ui.platform.LocalConfiguration.current
    val barWidth = if (screen.screenWidthDp > screen.screenHeightDp) WideBarWidth else UprightBarWidth
    Box(Modifier.align(Alignment.BottomCenter).fillMaxWidth()) {
        // where Settings' middle is along the bar, for the arrow of its popover
        var settingsAt by remember { mutableStateOf(281.5.dp) }
        val density = LocalDensity.current
        Column(Modifier.align(Alignment.BottomCenter)) {
            // Onion skin colors opens a page in the settings' place; closed, they open on settings again
            var colorsPage by remember { mutableStateOf(false) }
            if (!vm.animationSettingsOpen) colorsPage = false
            if (vm.animationSettingsOpen) {
                DownPopover(290.dp, 145.dp, Modifier.offset(x = (settingsAt - 145.dp).coerceIn(0.dp, barWidth - 290.dp))) {
                    if (colorsPage) OnionColorsPage(vm) { colorsPage = false } else AnimationSettingsPage(vm) { colorsPage = true }
                }
            } else if (vm.frameOptionsOpen) {
                DownPopover(260.dp, 130.dp, Modifier.offset(x = barWidth / 2 - 130.dp)) { FrameOptionsPage(vm) }
            }
            // the whole bar takes its touches: none reach the canvas under it (a swipe would paint)
            Box(Modifier.padding(bottom = BarGap).size(barWidth, BarHeight).clip(RoundedCornerShape(30.dp)).background(BarFill).pointerInput(Unit) { detectTapGestures { } }) {
                val pages = vm.animation?.pages == true
                Row(Modifier.fillMaxWidth().padding(start = BarInset, end = BarInset, top = 8.dp).height(30.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        if (pages) "Page Assist" else if (vm.playingFrame != null) "Pause" else "Play", color = BarInk, fontSize = 17.sp, fontWeight = FontWeight.Bold,
                        modifier = if (pages) Modifier else Modifier.clickable { vm.togglePlaying() },
                    )
                    Spacer(Modifier.weight(1f))
                    if (!pages) BarText(
                        "Settings",
                        // measured inside the row's padding
                        Modifier.onGloballyPositioned { c -> settingsAt = BarInset + with(density) { (c.positionInParent().x + c.size.width / 2f).toDp() } },
                    ) {
                        vm.animationSettingsOpen = !vm.animationSettingsOpen
                        vm.frameOptionsOpen = false
                    }
                    Spacer(Modifier.width(23.dp))
                    BarText(if (pages) "New Page" else "Add Frame", Modifier) { vm.addFrame() }
                }
                Timeline(vm, barWidth - 24.dp, Modifier.offset(x = 12.dp, y = 42.dp))
            }
        }
    }
}

@Composable
private fun BarText(label: String, modifier: Modifier, onClick: () -> Unit) {
    Box(modifier.height(30.dp).clickable(onClick = onClick), contentAlignment = Alignment.Center) {
        Text(label, color = BarInk, fontSize = 17.sp, maxLines = 1)
    }
}

/**
 * The frames in a rounded track [width] wide, the current (or played) one in the middle with a
 * blue line under it; each frame's card is 40 tall and as wide as the canvas's shape makes it.
 * Dragged, the track scrolls through the frames, each showing as it passes the middle; let go,
 * it coasts and settles on a frame, which becomes the current one.
 */
@Composable
private fun Timeline(vm: CanvasViewModel, width: Dp, modifier: Modifier) {
    val frames = vm.animationFrames
    val focus = vm.playingFrame ?: vm.currentFrame ?: 0
    val (cw, ch) = vm.canvasSize()
    val aspect = if (cw > 0 && ch > 0) (cw.toFloat() / ch).coerceIn(0.25f, 2f) else 0.66f
    val frameWidth = FrameHeight * aspect + 1.dp
    val pitch = frameWidth + 1.dp
    val pitchPx = with(LocalDensity.current) { pitch.toPx() }
    val last = (frames.size - 1).coerceAtLeast(0)
    // the frame in the middle, fractional while the track moves
    var at by remember { mutableFloatStateOf(focus.toFloat()) }
    var dragging by remember { mutableStateOf(false) }
    // the frame shown last while dragging
    var shown by remember { mutableIntStateOf(focus) }
    var settling by remember { mutableStateOf<Job?>(null) }
    val scope = rememberCoroutineScope()
    fun scrubTo(p: Float) {
        at = p
        val i = p.roundToInt()
        if (i != shown) {
            shown = i
            vm.scrubFrame(i)
        }
    }
    LaunchedEffect(focus) {
        if (dragging) return@LaunchedEffect
        // played frames step; a frame chosen or added slides into the middle (a drag takes over)
        if (vm.playingFrame != null) at = focus.toFloat() else animate(at, focus.toFloat(), animationSpec = Motion.frameSlide()) { v, _ -> if (!dragging) at = v }
    }
    DisposableEffect(Unit) { onDispose { if (dragging) vm.endScrub(null) } }
    val drag = rememberDraggableState { delta -> scrubTo((at - delta / pitchPx).coerceIn(0f, last.toFloat())) }
    Box(
        modifier.size(width, 44.dp).clip(RoundedCornerShape(18.dp)).background(TrackFill).border(1.dp, TrackEdge, RoundedCornerShape(18.dp))
            .draggable(
                drag,
                Orientation.Horizontal,
                enabled = frames.size > 1,
                onDragStarted = {
                    settling?.cancel()
                    if (!dragging) {
                        dragging = true
                        shown = focus
                        vm.beginScrub()
                    }
                },
                onDragStopped = { velocity ->
                    val target = exponentialDecay<Float>().calculateTargetValue(at, -velocity / pitchPx).roundToInt().coerceIn(0, last)
                    settling = scope.launch {
                        animate(at, target.toFloat(), -velocity / pitchPx, Motion.frameSlide()) { v, _ -> scrubTo(v.coerceIn(0f, last.toFloat())) }
                        dragging = false
                        vm.endScrub(target)
                    }
                },
            ),
    ) {
        frames.forEachIndexed { i, id ->
            val x = width / 2 - frameWidth / 2 + pitch * (i - at)
            if (x > -frameWidth && x < width) {
                Box(
                    Modifier.offset(x = x, y = 2.dp).size(frameWidth, FrameHeight).clip(RoundedCornerShape(6.dp)).background(Color.White)
                        .border(1.dp, Color(0xFF9A9A9A), RoundedCornerShape(6.dp)).clickable { vm.tapFrame(i) },
                ) {
                    vm.frameThumbnails[id]?.let { Image(it, contentDescription = "Frame ${i + 1}", contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize().padding(2.dp)) }
                }
            }
        }
    }
    Box(Modifier.offset(x = 12.dp + width / 2 - 12.dp, y = 42.dp + 44.dp + 3.dp).size(24.dp, 3.dp).clip(RoundedCornerShape(1.5.dp)).background(Pc.Accent))
}

/** A panel standing on the bar, its arrow pointing down at [arrowX] from its left edge. */
@Composable
private fun DownPopover(width: Dp, arrowX: Dp, modifier: Modifier, content: @Composable ColumnScope.() -> Unit) {
    val fill = Pc.PopoverFill
    Box(
        modifier.width(width).padding(bottom = 2.dp).drawBehind {
            val arrowH = Pc.ArrowHeight.toPx()
            val tipX = arrowX.toPx()
            val outline = Path().apply {
                addRoundRect(RoundRect(0f, 0f, size.width, size.height - arrowH, CornerRadius(24.dp.toPx())))
                moveTo(tipX - arrowH, size.height - arrowH - 0.5f)
                lineTo(tipX, size.height)
                lineTo(tipX + arrowH, size.height - arrowH - 0.5f)
                close()
            }
            drawPath(outline, fill)
        }.pointerInput(Unit) { detectTapGestures { } },
    ) {
        Column(Modifier.padding(start = 8.dp, end = 8.dp, top = 8.dp, bottom = Pc.ArrowHeight + 8.dp), content = content)
    }
}

/** Animation Assist's settings: playback, frames a second, onion skins, the primary frame. */
@Composable
private fun ColumnScope.AnimationSettingsPage(vm: CanvasViewModel, onColors: () -> Unit) {
    val a = vm.animation ?: return
    Text("Settings", color = Pc.PanelTitle, fontSize = 23.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 14.dp, top = 11.dp, bottom = 21.dp))
    Group {
        for ((i, pair) in listOf(AnimationPlayback.LOOP to "Loop", AnimationPlayback.PING_PONG to "Ping-Pong", AnimationPlayback.ONE_SHOT to "One Shot").withIndex()) {
            val (mode, label) = pair
            val chosen = a.playback == mode
            if (i > 0) RowLine()
            Row(
                Modifier.fillMaxWidth().height(40.dp).background(if (chosen) Pc.Accent else Color.Transparent).clickable { vm.changeAnimation { it.copy(playback = mode) } }.padding(horizontal = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(label, color = if (chosen) Color.White else Pc.PanelTitle, fontSize = 17.sp, modifier = Modifier.weight(1f))
                if (chosen) Icon(PcIcons.MenuTick, contentDescription = null, tint = Color.White, modifier = Modifier.size(22.dp))
            }
        }
    }
    Spacer(Modifier.height(10.dp))
    SliderGroup {
        ValueSlider("Frames Per Second", "${a.fps}", (a.fps.toFloat() - 1f) / 59f) { v -> vm.changeAnimation { it.copy(fps = (1 + (v * 59f).roundToInt()).toUInt()) } }
        val onion = a.onionFrames.toInt()
        ValueSlider("Onion skin frames", when (onion) { 0 -> "None"; MAX_ONION -> "Max"; else -> "$onion" }, onion / MAX_ONION.toFloat()) { v ->
            vm.changeAnimation { it.copy(onionFrames = (v * MAX_ONION).roundToInt().toUInt()) }
        }
        ValueSlider("Onion skin opacity", "${(a.onionOpacity * 100).roundToInt()} %", a.onionOpacity) { v -> vm.changeAnimation { it.copy(onionOpacity = v) } }
    }
    Spacer(Modifier.height(9.dp))
    Group {
        ToggleRow("Blend primary frame", a.blendPrimary, 44.5.dp) { vm.changeAnimation { it.copy(blendPrimary = !it.blendPrimary) } }
        RowLine()
        Box(Modifier.fillMaxWidth().height(44.5.dp).clickable(onClick = onColors).padding(start = 16.dp), contentAlignment = Alignment.CenterStart) {
            Text("Onion skin colors", color = Pc.PanelTitle, fontSize = 17.sp)
        }
    }
}

/**
 * The onion skins' colours: back to the settings, "Colors" and Reset over two swatches (the frames
 * before the current one, then those after; a tap picks which the picker below edits) and the
 * compact picker with its mode tabs. Black leaves the skins as they are.
 */
@Composable
private fun ColumnScope.OnionColorsPage(vm: CanvasViewModel, onBack: () -> Unit) {
    val a = vm.animation ?: return
    var after by remember { mutableStateOf(false) }
    var mode by remember { mutableStateOf(io.github.artcraftandroid.procraft.canvas.ColorMode.DISC) }
    val rgbOf = { v: List<Float> -> v.take(3).fold(0) { acc, x -> (acc shl 8) or (x.coerceIn(0f, 1f) * 255f).roundToInt() } }
    val unit = { rgb: Int -> listOf((rgb shr 16) and 0xFF, (rgb shr 8) and 0xFF, rgb and 0xFF).map { it / 255f } }
    val c = remember(after) {
        HeldColor(rgbOf(if (after) a.after else a.before), greyHue = 0f) { rgb -> vm.changeAnimation { if (after) it.copy(after = unit(rgb)) else it.copy(before = unit(rgb)) } }
    }
    // the back button at the panel's very edge
    Row(Modifier.offset(x = (-8).dp).fillMaxWidth().height(34.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(44.dp, 34.dp).clickable(onClick = onBack), contentAlignment = Alignment.Center) {
            Icon(PcIcons.ChevronLeft, contentDescription = "Back", tint = Pc.SlideValue, modifier = Modifier.requiredSize(34.dp))
        }
        Text("Colors", color = Pc.PanelTitle, fontSize = 23.sp, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
        Text(
            "Reset", color = Pc.SlideValue, fontSize = 17.sp,
            modifier = Modifier.clickable {
                vm.changeAnimation { it.copy(before = listOf(0f, 0f, 0f), after = listOf(0f, 0f, 0f)) }
                c.setRgb(0)
            }.padding(start = 12.dp, end = 0.dp),
        )
    }
    Row(Modifier.padding(start = 13.dp, top = 7.dp), horizontalArrangement = Arrangement.spacedBy(5.dp)) {
        for ((i, v) in listOf(a.before, a.after).withIndex()) {
            Box(
                Modifier.size(44.dp, 26.dp).clip(RoundedCornerShape(7.dp)).background(SwatchRim).padding(1.dp)
                    .clip(RoundedCornerShape(6.dp)).background(Color(0xFF000000.toInt() or rgbOf(v))).clickable { after = i == 1 },
            )
        }
    }
    // the 260-wide compact picker drawn 282 wide, in a body of fixed height over the tabs
    Box(Modifier.offset(x = (-4).dp).padding(top = 11.dp).size(282.dp, 365.dp)) {
        Box(Modifier.scaledFrom(260.dp, 282.dp)) { CompactBody(vm, c, mode) }
    }
    Box(Modifier.offset(x = (-4).dp).width(282.dp)) { CompactTabs(mode) { mode = it } }
}

private val SwatchRim = Color(0xFF969696)

/** Lays out its content [from] wide and shows it scaled up to [to] wide, touches and all. */
private fun Modifier.scaledFrom(from: Dp, to: Dp): Modifier = layout { measurable, _ ->
    val k = to / from
    val p = measurable.measure(androidx.compose.ui.unit.Constraints.fixedWidth(from.roundToPx()))
    layout((p.width * k).roundToInt(), (p.height * k).roundToInt()) {
        p.placeWithLayer(0, 0) {
            scaleX = k
            scaleY = k
            transformOrigin = androidx.compose.ui.graphics.TransformOrigin(0f, 0f)
        }
    }
}

/**
 * The current frame's options: how long it is held, Duplicate, Delete, and Foreground for the last
 * frame and Background for the first (both for a lone frame). Pages: Duplicate, Delete, and
 * Background for the first.
 */
@Composable
private fun ColumnScope.FrameOptionsPage(vm: CanvasViewModel) {
    val a = vm.animation ?: return
    val index = vm.currentFrame ?: return
    val last = vm.animationFrames.size - 1
    // re-read when the hold changes
    val hold = vm.holdVersion.let { vm.frameHold() }
    Text(if (a.pages) "Page options" else "Frame options", color = Pc.PanelTitle, fontSize = 23.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 14.dp, top = 11.dp, bottom = 15.dp))
    if (!a.pages) {
        SliderGroup(top = 4.dp) {
            ValueSlider("Hold duration", if (hold == 0) "None" else "$hold", hold / CanvasViewModel.MAX_HOLD.toFloat()) { v ->
                vm.changeFrameHold((v * CanvasViewModel.MAX_HOLD).roundToInt())
            }
        }
        Spacer(Modifier.height(10.dp))
    }
    Group {
        Row(Modifier.fillMaxWidth().height(44.dp).clickable { vm.duplicateFrame() }.padding(start = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("Duplicate", color = Pc.PanelTitle, fontSize = 17.sp)
        }
        RowLine()
        Row(Modifier.fillMaxWidth().height(44.dp).clickable { vm.deleteFrame() }.padding(start = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("Delete", color = Pc.PanelTitle, fontSize = 17.sp)
        }
    }
    val foreground = !a.pages && index == last
    val background = index == 0
    if (foreground || background) {
        Spacer(Modifier.height(10.dp))
        Group {
            if (foreground) ToggleRow("Foreground", a.foreground, 44.dp) { vm.changeAnimation { it.copy(foreground = !it.foreground) } }
            if (foreground && background) RowLine()
            if (background) ToggleRow("Background", a.background, 44.dp) { vm.changeAnimation { it.copy(background = !it.background) } }
        }
    }
}

@Composable
private fun ToggleRow(label: String, on: Boolean, height: Dp, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().height(height).clickable(onClick = onClick).padding(start = 16.dp, end = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = Pc.PanelTitle, fontSize = 17.sp, modifier = Modifier.weight(1f))
        Switch(on, enabled = true)
    }
}

/** The faint 2 pt line between a group's rows. */
@Composable
private fun RowLine() {
    Box(Modifier.fillMaxWidth().height(2.dp).background(RowLineFill))
}

@Composable
private fun Group(content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(Pc.ActionsGroup), content = content)
}

/** A group of sliders: [top] above the first, 11.5 between. */
@Composable
private fun SliderGroup(top: Dp = 8.dp, content: @Composable ColumnScope.() -> Unit) {
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(Pc.ActionsGroup).padding(top = top),
        verticalArrangement = Arrangement.spacedBy(11.5.dp),
        content = content,
    )
}

private val RowLineFill = Color(0x09FFFFFF)

/** A setting's name and value over a bar dragged or tapped along. */
@Composable
private fun ValueSlider(label: String, value: String, fraction: Float, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    Column(Modifier.fillMaxWidth().height(55.dp).padding(horizontal = 16.dp)) {
        Row(Modifier.fillMaxWidth().padding(top = 6.5.dp)) {
            Text(label, color = Pc.SlideLabel, fontSize = 17.sp, modifier = Modifier.weight(1f), maxLines = 1)
            Text(value, color = Pc.SlideValue, fontSize = 17.sp, maxLines = 1)
        }
        Box(
            Modifier.fillMaxWidth().height(22.dp)
                .pointerInput(Unit) { detectTapGestures { change((it.x / size.width).coerceIn(0f, 1f)) } }
                .pointerInput(Unit) {
                    detectHorizontalDragGestures(onDragStart = { change((it.x / size.width).coerceIn(0f, 1f)) }) { c, _ ->
                        c.consume()
                        change((c.position.x / size.width).coerceIn(0f, 1f))
                    }
                }
                .drawBehind {
                    val y = size.height / 2f
                    val h = 3.dp.toPx()
                    val x = fraction.coerceIn(0f, 1f) * size.width
                    drawRect(Pc.SlideTrack, Offset(0f, y - h / 2f), Size(size.width, h))
                    drawRect(Pc.Accent, Offset(0f, y - h / 2f), Size(x, h))
                    val knob = Size(16.dp.toPx(), 10.dp.toPx())
                    drawRoundRect(Color.White, Offset((x - knob.width / 2f).coerceIn(0f, size.width - knob.width), y - knob.height / 2f), knob, CornerRadius(knob.height / 2f))
                },
        )
    }
}
