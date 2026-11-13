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
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.geometry.lerp
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.graphics.drawscope.scale
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
 * be, turning as its view turns it, and the gallery fades over the last part, showing the canvas
 * and its bars. A canvas not yet
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
    // how far from the cell (0) towards the canvas (1)
    val t = if (z.opening) e else 1f - e
    val place = Place.between(z, t)
    Box(Modifier.fillMaxSize()) {
        Box(
            Modifier.fillMaxSize()
                .zoomed(z.cell.center, place)
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
                val size = Size(z.cell.width * place.scale, z.cell.height * place.scale)
                rotate(Math.toDegrees(place.angle.toDouble()).toFloat(), pivot = place.centre) {
                    scale(place.across, 1f, pivot = place.centre) {
                        drawImage(
                            picture,
                            dstOffset = IntOffset((place.centre.x - size.width / 2f).roundToInt(), (place.centre.y - size.height / 2f).roundToInt()),
                            dstSize = IntSize(size.width.roundToInt().coerceAtLeast(1), size.height.roundToInt().coerceAtLeast(1)),
                            alpha = (1f - e / CLOSE_FADE).coerceIn(0f, 1f),
                        )
                    }
                }
            }
        }
    }
}

/**
 * Where the artwork's cell is drawn part way (`t`, 0 at the cell, 1 at the canvas) to its canvas:
 * its middle, how much bigger than the cell, how far turned (radians) and across which way (1, or
 * -1 mirrored).
 */
private class Place(val centre: Offset, val scale: Float, val angle: Float, val across: Float) {
    companion object {
        fun between(z: CanvasViewModel.GalleryZoom, t: Float): Place {
            val full = if (z.cell.height > 0f) z.canvas.size.height / z.cell.height else 1f
            // the shorter way round
            var angle = z.canvas.angle
            while (angle > Math.PI) angle -= (2 * Math.PI).toFloat()
            while (angle < -Math.PI) angle += (2 * Math.PI).toFloat()
            return Place(
                centre = lerp(z.cell.center, z.canvas.centre, t),
                scale = 1f + (full - 1f) * t,
                angle = angle * t,
                // a mirrored canvas: the cell turns over on the way
                across = if (z.canvas.mirrored) 1f - 2f * t else 1f,
            )
        }
    }
}

/** Draws what it is on so that the point [from] (window pixels) lands on [to]'s middle, scaled, turned and mirrored about it as [to] says. */
private fun Modifier.zoomed(from: Offset, to: Place): Modifier = graphicsLayer {
    transformOrigin = TransformOrigin(
        if (size.width > 0f) from.x / size.width else 0f,
        if (size.height > 0f) from.y / size.height else 0f,
    )
    scaleX = to.scale * to.across
    scaleY = to.scale
    rotationZ = Math.toDegrees(to.angle.toDouble()).toFloat()
    translationX = to.centre.x - from.x
    translationY = to.centre.y - from.y
}

/** How much of the opening zoom's end the gallery fades over, and how much of the closing zoom's start the canvas does. */
private const val OPEN_FADE = 0.35f
private const val CLOSE_FADE = 0.3f
