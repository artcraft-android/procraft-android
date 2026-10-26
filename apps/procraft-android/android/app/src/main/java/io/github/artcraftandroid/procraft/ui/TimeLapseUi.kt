package io.github.artcraftandroid.procraft.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.TimeLapse
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

// the canvas's surround and grid, light with the light interface
private val ReplayFill get() = if (Pc.light) Color(0xFFCED1D2) else Color(0xFF252525)
private val GridLine get() = if (Pc.light) Color(0xFFCACDCE) else Color(0xFF2E2E2E)

/**
 * The surround's grid drawn as the canvas draws it (1 1/3 pt lines every 16, fixed to the glass
 * from [origin], none under the top bar), for screens shown in the canvas's place.
 */
internal fun androidx.compose.ui.graphics.drawscope.DrawScope.surroundGrid(origin: androidx.compose.ui.geometry.Offset?) {
    val step = 16.dp.toPx()
    val width = (4f / 3f).dp.toPx()
    val top = 44.dp.toPx()
    val o = origin ?: androidx.compose.ui.geometry.Offset(0.5.dp.toPx(), 0.5.dp.toPx())
    var x = o.x - kotlin.math.floor(o.x / step) * step
    while (x < size.width) {
        drawRect(GridLine, androidx.compose.ui.geometry.Offset(x - width / 2f, top), androidx.compose.ui.geometry.Size(width, size.height - top))
        x += step
    }
    var y = o.y - kotlin.math.floor((o.y - top) / step) * step
    while (y < size.height) {
        if (y - width / 2f >= top) drawRect(GridLine, androidx.compose.ui.geometry.Offset(0f, y - width / 2f), androidx.compose.ui.geometry.Size(size.width, width))
        y += step
    }
}

/**
 * The time-lapse replayed over everything at its frame rate; dragging across the screen scrubs
 * through it. A bar on top: Done, the title and the time into the replay.
 */
@Composable
fun TimeLapseReplay(vm: CanvasViewModel) {
    val count = remember { vm.timeLapseFrames() }
    var index by remember { mutableIntStateOf(0) }
    var scrubbing by remember { mutableStateOf(false) }
    var shown by remember { mutableStateOf<ImageBitmap?>(null) }
    BackHandler { vm.timeLapseReplaying = false }
    // play, frame by frame, until the end or a scrub
    LaunchedEffect(scrubbing) {
        while (!scrubbing && index < count - 1) {
            delay(1000L / TimeLapse.FPS)
            index++
        }
    }
    LaunchedEffect(index) {
        val i = index
        val image = withContext(Dispatchers.IO) { vm.timeLapseFrame(i)?.asImageBitmap() }
        if (image != null) shown = image
    }
    Box(
        Modifier.fillMaxSize().background(ReplayFill).drawBehind { surroundGrid(vm.gridOrigin) }.pointerInput(count) {
            var start = 0f
            var from = 0
            detectHorizontalDragGestures(
                onDragStart = { start = it.x; from = index; scrubbing = true },
                onHorizontalDrag = { change, _ ->
                    change.consume()
                    val moved = (change.position.x - start) / size.width
                    index = (from + (moved * count).toInt()).coerceIn(0, (count - 1).coerceAtLeast(0))
                },
                onDragEnd = { scrubbing = false },
                onDragCancel = { scrubbing = false },
            )
        },
    ) {
        shown?.let {
            // edge to edge, under the header
            Image(it, contentDescription = "Time-lapse", contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize())
        }
        if (count == 0) Text("Nothing recorded yet", color = Color(0xFF8E8E93), fontSize = 17.sp, modifier = Modifier.align(Alignment.Center))
        // the time into the replay, in frames past the second
        val frames = index
        val seconds = frames / TimeLapse.FPS
        val time = "%02d:%02d:%02d:%02d".format(seconds / 3600, seconds / 60 % 60, seconds % 60, frames % TimeLapse.FPS)
        ToolHeader("Time-lapse Replay", listOf(ToolButton(time)), onDone = { vm.timeLapseReplaying = false }, colors = if (Pc.light) DarkToolHeader else LightToolHeader)
        // how far it has played
        Box(Modifier.padding(top = ToolHeaderHeight).fillMaxWidth().height(4.dp)) {
            val f = if (count > 1) index / (count - 1).toFloat() else 1f
            Box(Modifier.fillMaxWidth(f.coerceIn(0f, 1f)).height(4.dp).background(Color(0xFF0076FD)))
        }
    }
}
