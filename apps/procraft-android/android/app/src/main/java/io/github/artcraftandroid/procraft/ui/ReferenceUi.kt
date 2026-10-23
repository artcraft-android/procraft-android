package io.github.artcraftandroid.procraft.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
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
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import kotlin.math.roundToInt

private val BarFill = Color(0xFF393939)
private val PictureFill = Color(0xFF262626)
private val GrabLine = Color(0xFF7C7C7C)
private val TitleInk = Color(0xFFC5C5C5)
private val CloseFill = Color(0xFF262626)
private val CloseEdge = Color(0xFF454545)
private val CloseInk = Color(0xFFA5A5A5)
private val SegmentTrack = Color(0xFF4D4D4D)
private val Muted = Color(0xFFAAAAAA)
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
            Box(Modifier.fillMaxWidth().height(picture.height), contentAlignment = Alignment.Center) {
                when (vm.referenceMode) {
                    CanvasViewModel.ReferenceMode.CANVAS -> vm.referenceCanvas?.let {
                        Image(it, contentDescription = "The canvas", contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize())
                    }
                    CanvasViewModel.ReferenceMode.IMAGE -> {
                        val image = vm.referenceImage
                        if (image != null) {
                            Image(image, contentDescription = "Reference picture", contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize())
                        } else {
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
