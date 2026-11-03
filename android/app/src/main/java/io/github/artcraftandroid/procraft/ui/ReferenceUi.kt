package io.github.artcraftandroid.procraft.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.input.pointer.PointerInputScope
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.draw.clipToBounds
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

private val BarFill get() = if (Pc.light) Color(0xFFEDEDED) else Color(0xFF393939)
private val PictureFill get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0xFF262626)
private val GrabLine get() = if (Pc.light) Color(0xFF909090) else Color(0xFF7C7C7C)
private val TitleInk get() = if (Pc.light) Color(0xFF3A3A3A) else Color(0xFFC5C5C5)
private val CloseFill get() = if (Pc.light) Color(0xFFFAFAFA) else Color(0xFF262626)
private val CloseEdge get() = if (Pc.light) Color(0xFFE0E0E0) else Color(0xFF454545)
private val CloseInk get() = if (Pc.light) Color(0xFF595959) else Color(0xFFA5A5A5)
private val SegmentTrack get() = if (Pc.light) Color(0xFFD7D7D7) else Color(0xFF4D4D4D)
private val Muted get() = if (Pc.light) Color(0xFF3B3B3B) else Color(0xFFAAAAAA)
private val TopBarHeight = 64.dp
private val BottomBarHeight = 45.dp
private val ReferenceCorner = 24.dp
private val HandleSide = 60.dp

/**
 * The reference window: the whole canvas as it is, or a picture to work from, floating over the
 * canvas. Its top bar drags it; the round button closes it (Reference off); handles over its
 * bottom corners size it.
 */
@Composable
fun BoxScope.ReferenceWindow(vm: CanvasViewModel) {
    // it fades in where it is, and out when closed; over the whole screen, since the fade draws
    // into a buffer its own size and the window sits away from the corner
    androidx.compose.animation.AnimatedVisibility(
        vm.referenceOn,
        Modifier.matchParentSize(),
        enter = androidx.compose.animation.fadeIn(Motion.referenceFade()),
        exit = androidx.compose.animation.fadeOut(Motion.referenceFade()),
    ) { ReferenceWindowContent(vm) }
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
    Box(Modifier.offset { IntOffset(at.x.roundToInt() - margin.roundToPx(), at.y.roundToInt()) }) {
        Column(Modifier.padding(start = margin, end = margin, bottom = below).width(picture.width).shadow(18.dp, RoundedCornerShape(ReferenceCorner)).clip(RoundedCornerShape(ReferenceCorner)).background(PictureFill)) {
            // the title bar: dragged to move the window
            Box(
                Modifier.fillMaxWidth().height(TopBarHeight).background(BarFill)
                    .pointerInput(Unit) { detectDragGestures { change, amount -> change.consume(); vm.moveReference(amount) } },
            ) {
                Box(Modifier.align(Alignment.TopCenter).padding(top = 4.dp).size(40.dp, 4.dp).clip(CircleShape).background(GrabLine))
                Text("Reference", color = TitleInk, fontSize = 22.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.align(Alignment.CenterStart).padding(start = 20.dp))
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
            // the picture: two fingers zoom and move it, a finger held still picks a colour from it;
            // touches here never reach the canvas under the window
            val shown = when (vm.referenceMode) {
                CanvasViewModel.ReferenceMode.CANVAS -> vm.referenceCanvas
                CanvasViewModel.ReferenceMode.IMAGE -> vm.referenceImage
                CanvasViewModel.ReferenceMode.FACE -> null
            }
            val view = if (vm.referenceMode == CanvasViewModel.ReferenceMode.IMAGE) vm.referenceImageView else vm.referenceCanvasView
            // where the picture's area is, read when a finger picks (kept as the layout itself, so a
            // window dragged since is never read where it was)
            val area = remember { arrayOfNulls<androidx.compose.ui.layout.LayoutCoordinates>(1) }
            val toScreen = { p: Offset -> area[0]?.takeIf { it.isAttached }?.localToRoot(p) ?: p }
            Box(
                Modifier.fillMaxWidth().height(picture.height).clipToBounds()
                    .onGloballyPositioned { area[0] = it }
                    .pointerInput(shown) {
                        if (shown != null) pictureGestures(vm, shown, toScreen) else awaitEachGesture { awaitFirstDown(requireUnconsumed = false) }
                    },
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
                                Modifier.width(180.dp).height(60.dp).clip(RoundedCornerShape(30.dp)).background(SegmentTrack)
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
            Box(Modifier.fillMaxWidth().height(BottomBarHeight).background(BarFill), contentAlignment = Alignment.Center) {
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
 * Touches on the window's [picture]: a finger held still for the eyedropper's delay picks a colour
 * from it, following the finger until it lets go; two fingers zoom and move it. [toScreen] turns a
 * point in the picture's area into one in the window, for the loupe.
 */
private suspend fun PointerInputScope.pictureGestures(vm: CanvasViewModel, picture: ImageBitmap, toScreen: (Offset) -> Offset) {
    awaitEachGesture {
        val first = awaitFirstDown(requireUnconsumed = false)
        first.consume()
        val sizes = pictureSizes(size.width.toFloat(), size.height.toFloat(), picture)
        val hold = (vm.gestures.delay(io.github.artcraftandroid.procraft.core.GesturePurpose.EYEDROPPER) * 1000f).toLong().coerceAtLeast(1L)
        val slop = viewConfiguration.touchSlop
        var fingers = 1
        // held still (no other finger, not moved) until the delay is up: null
        val ended = withTimeoutOrNull(hold) {
            while (true) {
                val e = awaitPointerEvent()
                e.changes.forEach { it.consume() }
                val down = e.changes.filter { it.pressed }
                fingers = down.size
                if (down.size != 1 || (down[0].position - first.position).getDistance() > slop) break
            }
            true
        }
        if (ended == null) {
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
        // until every finger lets go, two or more zoom and move the picture
        while (fingers > 0) {
            val e = awaitPointerEvent()
            e.changes.forEach { it.consume() }
            val down = e.changes.filter { it.pressed }
            fingers = down.size
            val both = down.filter { it.previousPressed }
            if (both.size < 2) continue
            val before = both.fold(Offset.Zero) { a, c -> a + c.previousPosition } / both.size.toFloat()
            val after = both.fold(Offset.Zero) { a, c -> a + c.position } / both.size.toFloat()
            val apartBefore = both.sumOf { (it.previousPosition - before).getDistance().toDouble() }.toFloat()
            val apartAfter = both.sumOf { (it.position - after).getDistance().toDouble() }.toFloat()
            vm.pinchReference(sizes, before, after, if (apartBefore > 0f) apartAfter / apartBefore else 1f)
        }
    }
}
