package io.github.artcraftandroid.procraft.ui

import android.content.res.Configuration
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
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
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.ImageShader
import androidx.compose.ui.graphics.ShaderBrush
import androidx.compose.ui.graphics.TileMode
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.PopupProperties
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import kotlin.math.roundToInt
import kotlinx.coroutines.launch

private val EditorPage = Color(0xFF2A2A2A)
private val EditorView = Color(0xFF262626)
private val EditorTitle = Color(0xFFEEEEEE)
private val CapsuleFill = Color(0xFF1C1C1C)
private val CapsuleEdge = Color(0xFF383838)
private val CapsuleInk = Color(0xFFAAAAAA)
private val DoneBlue = Color(0xFF0190FF)
private val RepeatLabel = Color(0xFF808080)
private val LibraryVeil = Color(0x80000000)
private val LibraryBar = Color(0xF21C1C1C)
private val MenuBlue = Color(0xFF0A84FF)

/**
 * The shape or grain editor of the brush studio (the one [CanvasViewModel.sourceEditing] says):
 * the image in a 600 square, changed by Import (a photo, a file, the source library or the
 * clipboard) or turned negative by a two-finger tap; the grain editor can also make it repeat
 * without seams. Cancel leaves the brush as it was, the tick keeps the change.
 */
@Composable
fun SourceEditor(vm: CanvasViewModel) {
    val grain = vm.sourceEditing ?: return
    val scope = rememberCoroutineScope()
    // the image as edited here (empty: the round tip), and the shipped one it is if it is one
    var image by remember(grain) { mutableStateOf(vm.studioSourcePixels(grain)) }
    var shipped by remember(grain) { mutableStateOf<Int?>(null) }
    var importing by remember { mutableStateOf(false) }
    var library by remember { mutableStateOf(false) }
    // Auto repeat and its settings
    var repeating by remember { mutableStateOf(false) }
    var scale by remember { mutableFloatStateOf(0f) }
    var rotate by remember { mutableFloatStateOf(0f) }
    var overlap by remember { mutableFloatStateOf(1f) }
    var hardness by remember { mutableFloatStateOf(0f) }
    var mirror by remember { mutableStateOf(false) }
    var pyramid by remember { mutableStateOf(true) }
    fun result(): ByteArray =
        if (repeating && image.isNotEmpty()) vm.repeatSource(image, 1f + scale * 3f, rotate * 360f, overlap, hardness, mirror, pyramid) else image
    val shown = remember(image, repeating, scale, rotate, overlap, hardness, mirror, pyramid) { vm.sourceBitmap(result(), shape = !grain) }
    fun take(pixels: ByteArray?) {
        if (pixels != null && pixels.isNotEmpty()) {
            image = pixels
            shipped = null
        }
    }
    val file = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri -> uri?.let { scope.launch { take(vm.decodeSource(it)) } } }
    val photo = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri -> uri?.let { scope.launch { take(vm.decodeSource(it)) } } }
    BackHandler { vm.sourceEditing = null }
    // landscape: a page sliding over the studio from the right, its pages' card still showing
    val landscape = LocalConfiguration.current.orientation == Configuration.ORIENTATION_LANDSCAPE
    Box(Modifier.fillMaxSize().pointerInput(Unit) { awaitEachGesture { awaitFirstDown().consume() } }) {
        BoxWithConstraints(
            Modifier.fillMaxSize()
                .then(if (landscape) Modifier.padding(start = 246.dp).clip(RoundedCornerShape(topStart = 20.dp, bottomStart = 20.dp)) else Modifier)
                .background(EditorPage),
        ) {
            // the grain repeating over the whole page
            if (repeating && shown != null) {
                Box(Modifier.fillMaxSize().background(ShaderBrush(ImageShader(shown, TileMode.Repeated, TileMode.Repeated))))
            } else {
                Box(
                    Modifier.align(Alignment.Center).offset(y = 32.dp).size(600.dp, 601.dp).clip(RoundedCornerShape(20.dp)).background(EditorView)
                        .pointerInput(Unit) {
                            // a two-finger tap turns the image negative
                            awaitEachGesture {
                                awaitFirstDown(requireUnconsumed = false)
                                var most = 1
                                while (true) {
                                    val event = awaitPointerEvent()
                                    most = maxOf(most, event.changes.count { it.pressed })
                                    if (event.changes.none { it.pressed }) break
                                }
                                if (most == 2 && image.isNotEmpty()) {
                                    image = ByteArray(image.size) { i -> (255 - (image[i].toInt() and 0xFF)).toByte() }
                                    shipped = null
                                }
                            }
                        },
                ) {
                    shown?.let { Image(it, contentDescription = null, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize()) }
                        // the round tip: a white disc
                        ?: if (!grain) Box(Modifier.align(Alignment.Center).size(480.dp).clip(CircleShape).background(Color.White)) else Box(Modifier.fillMaxSize().background(Color.White))
                }
            }
            Text(
                if (grain) "Grain Editor" else "Shape Editor", color = EditorTitle, fontSize = 22.sp, fontWeight = FontWeight.Bold,
                modifier = Modifier.offset(24.dp, if (landscape) 16.dp else 26.dp),
            )
            Row(
                Modifier.align(Alignment.TopEnd).padding(top = if (landscape) 10.dp else 20.dp, end = if (landscape) 12.dp else 20.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Row(Modifier.height(43.dp).clip(CircleShape).background(CapsuleFill).border(1.dp, CapsuleEdge, CircleShape), verticalAlignment = Alignment.CenterVertically) {
                    Box {
                        CapsuleButton("Import", 71) { importing = true }
                        ImportMenu(importing, onDismiss = { importing = false }) { choice ->
                            importing = false
                            when (choice) {
                                0 -> photo.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
                                1 -> file.launch(arrayOf("image/*"))
                                2 -> library = true
                                else -> scope.launch { take(vm.pastedSource()) }
                            }
                        }
                    }
                    if (grain) CapsuleButton("Auto repeat", 107, on = repeating) { repeating = !repeating }
                    CapsuleButton("Cancel", 73) { vm.sourceEditing = null }
                }
                Spacer(Modifier.width(13.dp))
                Box(
                    Modifier.size(44.dp, 45.dp).clip(CircleShape).background(DoneBlue).tap {
                        vm.keepStudioSource(grain, result(), shipped.takeIf { !repeating })
                    },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(PcIcons.DoneTick, contentDescription = "Done", tint = Color.White, modifier = Modifier.requiredSize(40.dp))
                }
            }
            if (repeating) RepeatPanel(
                Modifier.align(Alignment.BottomCenter).padding(bottom = 30.dp),
                scale, rotate, overlap, hardness, mirror, pyramid,
                onScale = { scale = it }, onRotate = { rotate = it }, onOverlap = { overlap = it }, onHardness = { hardness = it },
                onMirror = { mirror = it }, onPyramid = { pyramid = it },
            )
            if (library) SourceLibrary(vm, grain, onPick = { i ->
                image = vm.librarySource(grain, i)
                shipped = i
                library = false
            }) { library = false }
        }
    }
}

@Composable
private fun CapsuleButton(label: String, width: Int, on: Boolean = false, onClick: () -> Unit) {
    Box(
        Modifier.size(width.dp, 43.dp).clip(CircleShape).background(if (on) Pc.Accent else Color.Transparent).tap(onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = if (on) Color.White else CapsuleInk, fontSize = 17.sp, maxLines = 1)
    }
}

/** Where the image can come from, in a light popover under Import. */
@Composable
private fun ImportMenu(open: Boolean, onDismiss: () -> Unit, onPick: (Int) -> Unit) {
    val density = androidx.compose.ui.platform.LocalDensity.current
    MenuPresence(open, androidx.compose.ui.graphics.TransformOrigin(0.5f, 0f)) { look ->
        PcPopup(
            textScale = MATCHED_POPUP_TEXT,
            alignment = Alignment.TopCenter,
            offset = with(density) { androidx.compose.ui.unit.IntOffset(0, 48.dp.roundToPx()) },
            onDismissRequest = onDismiss,
            properties = PopupProperties(focusable = true),
        ) {
            Column(look.width(320.dp).clip(RoundedCornerShape(24.dp)).background(Pc.LightPanel).padding(start = 8.dp, end = 8.dp, bottom = 8.dp)) {
                Text("Image source", color = Pc.LightText, fontSize = 22.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 12.dp, top = 16.dp, bottom = 12.dp))
                Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(Pc.LightRow)) {
                    MenuRow(PcIcons.SourcePhoto, "Import a photo") { onPick(0) }
                    Box(Modifier.padding(start = 52.dp).fillMaxWidth().height(1.dp).background(Pc.LightSeparator))
                    MenuRow(PcIcons.Folder, "Import a file") { onPick(1) }
                    Box(Modifier.padding(start = 52.dp).fillMaxWidth().height(1.dp).background(Pc.LightSeparator))
                    MenuRow(PcIcons.SourceBooks, "Source Library") { onPick(2) }
                }
                Spacer(Modifier.height(20.dp))
                Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(Pc.LightRow)) {
                    MenuRow(PcIcons.TextPaste, "Paste", iconSize = 40.dp) { onPick(3) }
                }
            }
        }
    }
}

@Composable
private fun MenuRow(icon: ImageVector, label: String, iconSize: Dp = 24.dp, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().height(44.dp).tap(onClick).padding(start = 18.dp), verticalAlignment = Alignment.CenterVertically) {
        // in a 24 slot; icons drawn in a padded frame are shown bigger
        Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) {
            Icon(icon, contentDescription = null, tint = MenuBlue, modifier = Modifier.requiredSize(iconSize))
        }
        Text(label, color = Pc.LightText, fontSize = 17.sp, maxLines = 1, modifier = Modifier.padding(start = 22.dp))
    }
}

/** Auto repeat's settings: two columns of sliders and switches in a panel along the bottom. */
@Composable
private fun RepeatPanel(
    modifier: Modifier,
    scale: Float, rotate: Float, overlap: Float, hardness: Float, mirror: Boolean, pyramid: Boolean,
    onScale: (Float) -> Unit, onRotate: (Float) -> Unit, onOverlap: (Float) -> Unit, onHardness: (Float) -> Unit,
    onMirror: (Boolean) -> Unit, onPyramid: (Boolean) -> Unit,
) {
    Column(
        modifier.size(650.dp, 228.dp).clip(RoundedCornerShape(16.dp)).background(EditorPage).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            RepeatSlider("Grain scale", "%.1f".format(1f + scale * 3f).trimEnd('0').trimEnd('.'), scale, onScale)
            RepeatSlider("Rotate", "${(rotate * 360f).roundToInt()}°", rotate, onRotate)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            RepeatSlider("Border overlap", if (overlap >= 1f) "Maximal" else if (overlap <= 0f) "None" else "${(overlap * 100).roundToInt()}%", overlap, onOverlap)
            RepeatSlider("Mask hardness", if (hardness <= 0f) "Soft" else if (hardness >= 1f) "Hard" else "${(hardness * 100).roundToInt()}%", hardness, onHardness)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            RepeatSwitch("Mirror overlap", mirror, onMirror)
            RepeatSwitch("Pyramid blending", pyramid, onPyramid)
        }
    }
}

@Composable
private fun RepeatSlider(label: String, value: String, at: Float, onChange: (Float) -> Unit) {
    Column(Modifier.width(301.dp).height(55.dp)) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp)) {
            Text(label, color = RepeatLabel, fontSize = 15.sp, maxLines = 1, modifier = Modifier.weight(1f))
            Text(value, color = RepeatLabel, fontSize = 15.sp, maxLines = 1)
        }
        SimpleSlider(at, onChange, Modifier.padding(horizontal = 16.dp))
    }
}

@Composable
private fun RepeatSwitch(label: String, on: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.width(301.dp).height(40.dp).tap { onChange(!on) }.padding(start = 16.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = RepeatLabel, fontSize = 15.sp, maxLines = 1, modifier = Modifier.weight(1f))
        Switch(on, enabled = true)
    }
}

/** A thin track with a white knob: the position follows the finger. */
@Composable
private fun SimpleSlider(value: Float, onChange: (Float) -> Unit, modifier: Modifier) {
    BoxWithConstraints(
        modifier.fillMaxWidth().height(20.dp).pointerInput(Unit) {
            awaitEachGesture {
                val down = awaitFirstDown()
                fun send(x: Float) = onChange((x / size.width).coerceIn(0f, 1f))
                send(down.position.x)
                while (true) {
                    val event = awaitPointerEvent()
                    val c = event.changes.firstOrNull { it.id == down.id } ?: break
                    if (!c.pressed) break
                    send(c.position.x)
                    c.consume()
                }
            }
        },
        contentAlignment = Alignment.CenterStart,
    ) {
        val w = maxWidth
        Box(Modifier.fillMaxWidth().height(3.dp).clip(CircleShape).background(Color(0xFF3E3E3E)))
        Box(Modifier.width(w * value).height(3.dp).clip(CircleShape).background(Pc.Accent))
        Box(Modifier.offset(x = (w - 16.dp) * value).size(16.dp, 10.dp).clip(CircleShape).background(Color.White))
    }
}

/**
 * The source library: the shipped shape or grain images in 180 cells (the image 146 square, its
 * name under it) under a bar with the title, Cancel, and Shape Source and Grain Source.
 */
@Composable
private fun SourceLibrary(vm: CanvasViewModel, grain: Boolean, onPick: (Int) -> Unit, onCancel: () -> Unit) {
    var showGrains by remember { mutableStateOf(grain) }
    val images: List<Pair<String, ImageBitmap?>> = if (showGrains) vm.grainLibrary else vm.shapeLibrary
    BackHandler(onBack = onCancel)
    Box(Modifier.fillMaxSize().background(LibraryVeil)) {
        LazyVerticalGrid(
            columns = GridCells.Adaptive(180.dp),
            contentPadding = PaddingValues(top = 183.dp, start = 12.dp, end = 12.dp, bottom = 24.dp),
            modifier = Modifier.fillMaxSize(),
        ) {
            itemsIndexed(images) { i, (name, image) ->
                Column(
                    Modifier.size(180.dp).tap {
                        // each kind takes its own: a grain picked in the shape editor is not taken
                        if (showGrains == grain) onPick(i)
                    },
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Box(Modifier.padding(top = 6.dp).size(146.dp).clip(RoundedCornerShape(8.dp)).background(Color.Black)) {
                        image?.let { Image(it, contentDescription = name, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize()) }
                    }
                    Text(name, color = Color.White, fontSize = 17.sp, maxLines = 1, modifier = Modifier.padding(top = 5.dp))
                }
            }
        }
        Column(Modifier.fillMaxWidth().height(183.dp).background(LibraryBar)) {
            Row(Modifier.fillMaxWidth().height(128.dp).padding(start = 20.dp, end = 20.dp, bottom = 20.dp), verticalAlignment = Alignment.Bottom) {
                Text("Source Library", color = Color.White, fontSize = 27.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.weight(1f))
                Text("Cancel", color = Color.White, fontSize = 18.sp, maxLines = 1, modifier = Modifier.tap(onCancel))
            }
            Row(Modifier.fillMaxWidth().height(55.dp), horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
                for ((label, isGrain) in listOf("Shape Source" to false, "Grain Source" to true)) {
                    Box(
                        Modifier.padding(horizontal = 6.dp).size(114.dp, 40.dp).clip(RoundedCornerShape(8.dp))
                            .background(if (showGrains == isGrain) Pc.Accent else Color.Transparent).tap { showGrains = isGrain },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(label, color = Color.White, fontSize = 15.sp, maxLines = 1)
                    }
                }
            }
        }
    }
}
