package io.github.artcraftandroid.procraft.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.lerp
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import kotlin.math.roundToInt
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

/**
 * The gallery zooming into an artwork as it opens, and out of it as it closes. Opening, it is drawn
 * over the canvas getting ready ([overlay]): the artwork's cell grows to where its canvas will
 * be, and the gallery fades over the last part, showing the canvas and its bars. A canvas not yet
 * drawn by then (a big artwork) stays covered by the grown artwork until it is, then that fades.
 * Closing, it is the gallery itself, starting with the cell where the canvas was and settling
 * back, as a picture of the canvas fades away over it.
 */
@Composable
fun ZoomingGallery(vm: CanvasViewModel, overlay: Boolean) {
    val z = vm.galleryZoom?.takeIf { it.opening == overlay }
    if (z == null) {
        if (!overlay) GalleryScreen(vm)
        return
    }
    val progress = remember(z) { Animatable(0f) }
    var drawn by remember(z) { mutableStateOf(!z.opening) }
    // whether the canvas was drawn when the fade was due; if not, the cover holds and fades later
    var fadesWithZoom by remember(z) { mutableStateOf(true) }
    val cover = remember(z) { Animatable(1f) }
    LaunchedEffect(z) {
        if (z.opening) {
            launch {
                vm.awaitCanvasDrawn()
                drawn = true
            }
            launch {
                snapshotFlow { progress.value >= 1f - OPEN_FADE }.first { it }
                fadesWithZoom = drawn
            }
        }
        progress.animateTo(1f, if (z.opening) Motion.galleryOpen() else Motion.galleryClose())
        if (!fadesWithZoom) {
            snapshotFlow { drawn }.first { it }
            cover.animateTo(0f, Motion.coverFade())
        }
        vm.galleryZoom = null
    }
    val e = progress.value
    val at = if (z.opening) lerp(z.cell, z.canvas, e) else lerp(z.canvas, z.cell, e)
    Box(Modifier.fillMaxSize()) {
        Box(
            Modifier.fillMaxSize()
                .zoomed(z.cell, at)
                .graphicsLayer {
                    alpha = when {
                        !z.opening -> 1f
                        fadesWithZoom -> ((1f - e) / OPEN_FADE).coerceIn(0f, 1f)
                        else -> cover.value
                    }
                }
                // nothing in it is touched while it moves: taken before the gallery in it sees them
                .then(
                    if (overlay) {
                        Modifier.pointerInput(Unit) {
                            awaitEachGesture { while (true) awaitPointerEvent(PointerEventPass.Initial).changes.forEach { it.consume() } }
                        }
                    } else {
                        Modifier
                    },
                ),
        ) { GalleryScreen(vm) }
        // closing, the canvas fades away over its cell
        val picture = z.picture
        if (!z.opening && picture != null) {
            Canvas(Modifier.fillMaxSize()) {
                drawImage(
                    picture,
                    dstOffset = IntOffset(at.left.roundToInt(), at.top.roundToInt()),
                    dstSize = IntSize(at.width.roundToInt().coerceAtLeast(1), at.height.roundToInt().coerceAtLeast(1)),
                    alpha = (1f - e / CLOSE_FADE).coerceIn(0f, 1f),
                )
            }
        }
    }
}

/** Draws what it is on so that [from] (window pixels) lands on [to]: scaled about the top left, then moved. */
private fun Modifier.zoomed(from: Rect, to: Rect): Modifier = graphicsLayer {
    val k = if (from.height > 0f) to.height / from.height else 1f
    transformOrigin = TransformOrigin(0f, 0f)
    scaleX = k
    scaleY = k
    translationX = to.left - from.left * k
    translationY = to.top - from.top * k
}

/** How much of the opening zoom's end the gallery fades over, and how much of the closing zoom's start the canvas does. */
private const val OPEN_FADE = 0.35f
private const val CLOSE_FADE = 0.3f
