package io.github.artcraftandroid.procraft.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.input.pointer.PointerInputScope
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.runtime.remember
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
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
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import kotlin.math.roundToInt

// the dark bars are glass, like the panels: what the picture has under them shows through, blurred
private val BarFill get() = if (Pc.light) Color(0xFFEDEDED) else Color(0xDE1E1E1E)
private val PictureFill get() = if (Pc.light) Color(0xFFF0F0F0) else Color(0xFF333333)
private val GrabLine get() = if (Pc.light) Color(0xFF909090) else Color(0x80BFBFBF)
private val TitleInk get() = if (Pc.light) Color(0xFF3A3A3A) else Color(0xFFC5C5C5)
private val CloseFill get() = if (Pc.light) Color(0xFFFAFAFA) else Color(0x94181818)
private val CloseEdge get() = if (Pc.light) Color(0xFFE0E0E0) else Color(0x80515151)
private val CloseInk get() = if (Pc.light) Color(0xFF595959) else Color(0xFFA5A5A5)
private val SegmentTrack get() = if (Pc.light) Color(0xFFD7D7D7) else Color(0x17FFFFFF)
private val ImportFill get() = if (Pc.light) Color(0xFFD7D7D7) else Color(0xFF4D4D4D)
private val Muted get() = if (Pc.light) Color(0xFF3B3B3B) else Color(0xFFAAAAAA)
private val TopBarHeight = 64.dp
private val BottomBarHeight = 45.dp
private val ReferenceCorner = 24.dp
private val HandleSide = 60.dp
/** How far down from the window's top a drag moves it, the bars shown or not. */
private val GrabHeight = 16.dp

/** How many pixels a picture's longer side has as the bars' glass: drawn back up, that's its blur. */
private const val GlassSide = 24

/**
 * The reference window: the whole canvas as it is, or a picture to work from, floating over the
 * canvas. Its grab line drags it; the round button closes it (Reference off); handles over its
 * bottom corners size it. Its bars lie over the picture, and a tap on it slides them away and back.
 */
@Composable
fun BoxScope.ReferenceWindow(vm: CanvasViewModel) {
    if (!vm.referenceOn) return
    Box(
        Modifier
            .fillMaxSize()
            // ... existing modifiers
    ) {
        ReferenceWindowContent(vm)
    }
}

/** Touches here go no further: not to the picture under a bar. */
private val Modifier.ownTouches get() = pointerInput(Unit) {
    awaitEachGesture { awaitFirstDown(requireUnconsumed = false).consume() }
}

/** [picture] made a few pixels across, inside a clear pixel so that its edge blurs too: the bars' glass. */
private fun glassOf(picture: ImageBitmap): ImageBitmap {
    val scale = GlassSide.toFloat() / maxOf(picture.width, picture.height)
    val (w, h) = (picture.width * scale).roundToInt().coerceAtLeast(1) to (picture.height * scale).roundToInt().coerceAtLeast(1)
    val out = android.graphics.Bitmap.createBitmap(w + 2, h + 2, android.graphics.Bitmap.Config.ARGB_8888)
    val paint = android.graphics.Paint(android.graphics.Paint.FILTER_BITMAP_FLAG)
    android.graphics.Canvas(out).drawBitmap(picture.asAndroidBitmap(), null, android.graphics.Rect(1, 1, w + 1, h + 1), paint)
    return out.asImageBitmap()
}

@Composable
private fun ReferenceWindowContent(vm: CanvasViewModel) {
    val density = LocalDensity.current
    val config = LocalConfiguration.current
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri -> uri?.let(vm::importReference) }
    val picture = vm.referenceSize
    val tall = TopBarHeight + picture.height + BottomBarHeight
    // the first time: in the middle near the bottom of the screen
    SideEffect {
        with(density) { vm.placeReference(config.screenWidthDp.dp.toPx(), config.screenHeightDp.dp.toPx(), tall.toPx()) }
    }
    val at = vm.referenceOffset.takeIf { it != Offset.Unspecified } ?: Offset.Zero
    // room around the window for the handles, which reach past its corners (touches outside a
    // layout's bounds don't reach it)
    val margin = HandleSide / 2 - 12.dp
    val below = HandleSide / 2 - 15.dp
    val barsShown = androidx.compose.animation.core.animateFloatAsState(if (vm.referenceBarsHidden) 0f else 1f, Motion.bars(), label = "reference bars").value
    Box(Modifier.offset { IntOffset(at.x.roundToInt() - margin.roundToPx(), at.y.roundToInt()) }) {
        Box(Modifier.padding(start = margin, end = margin, bottom = below).size(picture.width, tall).shadow(18.dp, RoundedCornerShape(ReferenceCorner)).clip(RoundedCornerShape(ReferenceCorner)).background(PictureFill)) {
            // the picture: the whole window's, under the bars. A tap hides them or brings them
            // back; a finger moves it, two zoom and move it, a finger held still picks a colour
            // from it; touches here never reach the canvas under the window
            val shown = when (vm.referenceMode) {
                CanvasViewModel.ReferenceMode.CANVAS -> vm.referenceCanvas
                CanvasViewModel.ReferenceMode.IMAGE -> vm.referenceImage
                CanvasViewModel.ReferenceMode.FACE -> null
            }
            val view = if (vm.referenceMode == CanvasViewModel.ReferenceMode.IMAGE) vm.referenceImageView else vm.referenceCanvasView
            val glass = remember(shown) { shown?.let(::glassOf) }
            // where the picture's area is, read when a finger picks (kept as the layout itself, so a
            // window dragged since is never read where it was)
            val area = remember { arrayOfNulls<androidx.compose.ui.layout.LayoutCoordinates>(1) }
            val toScreen = { p: Offset -> area[0]?.takeIf { it.isAttached }?.localToRoot(p) ?: p }
            Box(
                Modifier.fillMaxSize().clipToBounds()
                    .onGloballyPositioned { area[0] = it }
                    .pointerInput(shown) { pictureGestures(vm, shown, toScreen) },
                contentAlignment = Alignment.Center,
            ) {
                if (shown != null) {
                    Canvas(Modifier.fillMaxSize()) {
                        val r = io.github.artcraftandroid.procraft.core.referencePictureRect(view, pictureSizes(size.width, size.height, shown))
                        // the canvas's pixels crisp from 100%, as on the canvas
                        val crisp = vm.referenceMode == CanvasViewModel.ReferenceMode.CANVAS && vm.referenceCanvasWhole && r[2] >= shown.width * this.density * 0.999f
                        drawImage(
                            shown,
                            dstOffset = IntOffset(r[0].roundToInt(), r[1].roundToInt()),
                            dstSize = IntSize(r[2].roundToInt(), r[3].roundToInt()),
                            filterQuality = if (crisp) androidx.compose.ui.graphics.FilterQuality.None else androidx.compose.ui.graphics.FilterQuality.Low,
                        )
                    }
                }
                when (vm.referenceMode) {
                    CanvasViewModel.ReferenceMode.CANVAS -> Unit
                    CanvasViewModel.ReferenceMode.IMAGE -> {
                        if (vm.referenceImage == null) {
                            Box(
                                Modifier.width(180.dp).height(60.dp).clip(RoundedCornerShape(30.dp)).background(ImportFill)
                                    .clickable { pick.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) },
                                contentAlignment = Alignment.Center,
                            ) {
                                Text("Import Image", color = Muted, fontSize = 16.sp)
                            }
                        }
                    }
                    CanvasViewModel.ReferenceMode.FACE -> Text("Not available on this device", color = Muted, fontSize = 14.sp)
                }
            }
            // what a bar at [top] (window pixels) has behind it: the window, and the picture blurred
            fun Modifier.overPicture(top: Float) = background(PictureFill).drawBehind {
                if (shown == null || glass == null) return@drawBehind
                val window = with(density) { Offset(picture.width.toPx(), tall.toPx()) }
                val r = io.github.artcraftandroid.procraft.core.referencePictureRect(view, pictureSizes(window.x, window.y, shown))
                val (px, py) = r[2] / (glass.width - 2) to r[3] / (glass.height - 2)
                drawImage(
                    glass,
                    dstOffset = IntOffset((r[0] - px).roundToInt(), (r[1] - py - top).roundToInt()),
                    dstSize = IntSize((r[2] + px * 2).roundToInt(), (r[3] + py * 2).roundToInt()),
                    filterQuality = androidx.compose.ui.graphics.FilterQuality.Low,
                )
            }.background(BarFill)
            val (topSlide, bottomSlide) = with(density) { -TopBarHeight.toPx() * (1f - barsShown) to (BottomBarHeight.toPx() + 1f) * (1f - barsShown) }
            Box(Modifier.offset { IntOffset(0, topSlide.roundToInt()) }.fillMaxWidth().height(TopBarHeight).clipToBounds().overPicture(topSlide).ownTouches) {
                Text("Reference", color = TitleInk, fontSize = 22.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.align(Alignment.TopStart).padding(start = 20.dp, top = 10.dp))
                Row(Modifier.align(Alignment.CenterEnd).padding(end = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                    if (vm.referenceMode == CanvasViewModel.ReferenceMode.IMAGE && vm.referenceImage != null) {
                        Text("Clear", color = Pc.Accent, fontSize = 15.sp, modifier = Modifier.clickable { vm.clearReference() }.padding(6.dp))
                        Text("Import", color = Pc.Accent, fontSize = 15.sp, modifier = Modifier.clickable { pick.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) }.padding(6.dp))
                    }
                    Box(
                        Modifier.size(43.dp).clip(CircleShape).background(CloseFill).border(1.dp, CloseEdge, CircleShape).clickable { vm.changeReferenceOn(false) },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(PcIcons.Cross, contentDescription = "Close", tint = CloseInk, modifier = Modifier.size(20.dp))
                    }
                }
            }
            val bottomTop = with(density) { (tall - BottomBarHeight).toPx() } + bottomSlide
            Box(
                Modifier.align(Alignment.BottomStart).offset { IntOffset(0, bottomSlide.roundToInt()) }.fillMaxWidth().height(BottomBarHeight).clipToBounds().overPicture(bottomTop).ownTouches,
                contentAlignment = Alignment.Center,
            ) {
                Row(
                    Modifier.width(270.dp).height(35.dp).clip(CircleShape).background(SegmentTrack).padding(2.dp),
                    horizontalArrangement = Arrangement.spacedBy(11.dp),
                ) {
                    for ((mode, label) in listOf(CanvasViewModel.ReferenceMode.CANVAS to "Canvas", CanvasViewModel.ReferenceMode.IMAGE to "Image", CanvasViewModel.ReferenceMode.FACE to "Face")) {
                        val on = vm.referenceMode == mode
                        Box(
                            Modifier.weight(1f).fillMaxHeight().clip(CircleShape).background(if (on) Pc.Accent else Color.Transparent)
                                .clickable { vm.changeReferenceMode(mode) },
                            contentAlignment = Alignment.Center,
                        ) {
                            Text(label, color = if (on) Color.White else TitleInk, fontSize = 15.sp, maxLines = 1)
                        }
                    }
                }
            }
            // the grab line stays when the bars go, and dragging by it moves the window
            Box(
                Modifier.fillMaxWidth().height(GrabHeight)
                    .pointerInput(Unit) { detectDragGestures { change, amount -> change.consume(); vm.moveReference(amount) } },
            ) {
                Box(Modifier.align(Alignment.TopCenter).padding(top = 4.dp).size(40.dp, 4.dp).clip(CircleShape).background(GrabLine))
            }
        }
        // the size handles, unseen, centred 12 in from the window's bottom corners and 15 up
        for (left in listOf(true, false)) {
            val x = if (left) 0.dp else picture.width + margin * 2 - HandleSide
            Box(
                Modifier.offset(x = x, y = tall - 15.dp - HandleSide / 2).size(HandleSide)
                    .pointerInput(left) {
                        detectDragGestures { change, amount ->
                            change.consume()
                            vm.resizeReference(with(density) { amount.x.toDp() }, with(density) { amount.y.toDp() }, fromLeft = left)
                        }
                    },
            )
        }
    }
}

/** The window's picture area ([width] × [height] pixels) and [picture]'s size, for the core's view of it. */
private fun pictureSizes(width: Float, height: Float, picture: ImageBitmap) =
    io.github.artcraftandroid.procraft.core.ReferenceSizes(width, height, picture.width.toFloat(), picture.height.toFloat())

/**
 * Touches on the window's [picture] (none: an empty window): a tap hides the bars or brings them
 * back, two fit the picture; a finger held still for the eyedropper's delay picks a colour from it,
 * following the finger until it lets go; a finger moves it, two zoom and move it. [toScreen] turns
 * a point in the picture's area into one in the window, for the loupe.
 */
private suspend fun PointerInputScope.pictureGestures(vm: CanvasViewModel, picture: ImageBitmap?, toScreen: (Offset) -> Offset) {
    awaitEachGesture {
        val first = awaitFirstDown(requireUnconsumed = false)
        first.consume()
        val sizes = picture?.let { pictureSizes(size.width.toFloat(), size.height.toFloat(), it) }
        val hold = if (picture == null) Long.MAX_VALUE else (vm.gestures.delay(io.github.artcraftandroid.procraft.core.GesturePurpose.EYEDROPPER) * 1000f).toLong().coerceAtLeast(1L)
        val slop = viewConfiguration.touchSlop
        var fingers = 1
        var moved = false
        // held still (no other finger, not moved) until the delay is up: null
        val ended = withTimeoutOrNull(hold) {
            while (true) {
                val e = awaitPointerEvent()
                e.changes.forEach { it.consume() }
                val down = e.changes.filter { it.pressed }
                fingers = down.size
                moved = down.size > 1 || down.any { (it.position - first.position).getDistance() > slop }
                if (down.size != 1 || moved) break
            }
            true
        }
        if (ended == null && picture != null && sizes != null) {
            fun pickAt(p: Offset) {
                val view = if (vm.referenceMode == CanvasViewModel.ReferenceMode.IMAGE) vm.referenceImageView else vm.referenceCanvasView
                val at = io.github.artcraftandroid.procraft.core.referencePicturePoint(view, sizes, p.x, p.y) ?: return
                vm.pickReference(picture, at[0], at[1], at[2], toScreen(p))
            }
            pickAt(first.position)
            while (true) {
                val e = awaitPointerEvent()
                e.changes.forEach { it.consume() }
                val finger = e.changes.firstOrNull { it.id == first.id }
                if (finger == null || !finger.pressed) break
                pickAt(finger.position)
            }
            if (vm.picking != null) vm.finishPick()
            return@awaitEachGesture
        }
        // lifted where it landed: a tap, or the first of two
        if (!moved) {
            val second = withTimeoutOrNull(viewConfiguration.doubleTapTimeoutMillis) { awaitFirstDown(requireUnconsumed = false) }
            second?.consume()
            if (second == null) vm.toggleReferenceBars() else vm.fitReference()
            return@awaitEachGesture
        }
        // until every finger lets go, they move the picture and two or more zoom it
        while (fingers > 0) {
            val e = awaitPointerEvent()
            e.changes.forEach { it.consume() }
            val down = e.changes.filter { it.pressed }
            fingers = down.size
            val both = down.filter { it.previousPressed }
            if (both.isEmpty() || sizes == null) continue
            val before = both.fold(Offset.Zero) { a, c -> a + c.previousPosition } / both.size.toFloat()
            val after = both.fold(Offset.Zero) { a, c -> a + c.position } / both.size.toFloat()
            val apartBefore = both.sumOf { (it.previousPosition - before).getDistance().toDouble() }.toFloat()
            val apartAfter = both.sumOf { (it.position - after).getDistance().toDouble() }.toFloat()
            vm.pinchReference(sizes, before, after, if (apartBefore > 0f) apartAfter / apartBefore else 1f)
        }
    }
}
