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
import io.github.artcraftandroid.procraft.core.SourceKind
import kotlin.math.roundToInt
import kotlinx.coroutines.launch

private val EditorPage get() = if (Pc.light) Color(0xFFE5E5E5) else Color(0xFF2A2A2A)
private val EditorView get() = if (Pc.light) Color(0xFFE5E5E5) else Color(0xFF262626)
private val EditorTitle get() = if (Pc.light) Color(0xFF333333) else Color(0xFFEEEEEE)
private val CapsuleFill get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0xFF1C1C1C)
private val CapsuleEdge get() = if (Pc.light) Color(0xFFE0E0E0) else Color(0xFF383838)
private val CapsuleInk get() = if (Pc.light) Color(0xFF585858) else Color(0xFFAAAAAA)
private val DoneBlue = Color(0xFF0190FF)
private val RepeatLabel get() = if (Pc.light) Color(0xFF808080) else Color(0xFF808080)
private val LibraryVeil = Color(0x80000000)
private val LibrarySheet get() = if (Pc.light) Color(0xFFF2F2F7) else Color(0xFF1C1C1E)
private val LibraryTitle get() = if (Pc.light) Color(0xFF3A3A3A) else Color(0xFFC1C1C1)
private val LibraryDim get() = if (Pc.light) Color(0xFF868686) else Color(0xFF868686)
private val LibraryEdge get() = if (Pc.light) Color(0xFFC8C8C8) else Color(0xFF5A5A5A)
private val LibrarySearchFill get() = if (Pc.light) Color(0xFFE6E6EA) else Color(0xFF1B1B1B)
private val LibrarySearchEdge get() = if (Pc.light) Color(0xFFD8D8DC) else Color(0xFF363636)
private val LibraryHint get() = if (Pc.light) Color(0xFF8A8A8E) else Color(0xFF787878)
private val MenuBlue = Color(0xFF0A84FF)

/**
 * The brush studio's editor for the source [CanvasViewModel.sourceEditing] says (the shape, the
 * grain, or what the brush paints metalness or roughness through): the image in a 600 square,
 * changed by Import (a photo, a file, the source library or the clipboard) or turned negative by a
 * two-finger tap; all but the shape's can also make it repeat without seams, and take the grains
 * from the library. Cancel leaves the brush as it was, the tick keeps the change.
 */
@Composable
fun SourceEditor(vm: CanvasViewModel) {
    val kind = vm.sourceEditing ?: return
    val grain = kind != SourceKind.SHAPE
    val scope = rememberCoroutineScope()
    // the image as edited here (empty: the round tip, or white all over), and the shipped one it is if it is one
    var image by remember(kind) { mutableStateOf(vm.studioSourcePixels(kind)) }
    var shipped by remember(kind) { mutableStateOf<Int?>(null) }
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
        Box(
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
                when (kind) {
                    SourceKind.SHAPE -> "Shape Editor"
                    SourceKind.GRAIN -> "Grain Editor"
                    SourceKind.METALLIC -> "Metallic Editor"
                    SourceKind.ROUGHNESS -> "Roughness Editor"
                },
                color = EditorTitle, fontSize = 22.sp, fontWeight = FontWeight.Bold,
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
                        vm.keepStudioSource(kind, result(), shipped.takeIf { !repeating })
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
                shipped = vm.shippedOf(grain, i)
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
        // under Import, its arrow pointing up at it
        PcPopup(
            alignment = Alignment.TopCenter,
            offset = with(density) { androidx.compose.ui.unit.IntOffset(0, 47.dp.roundToPx()) },
            onDismissRequest = onDismiss,
            properties = PopupProperties(focusable = true),
        ) {
            Popover(320.dp, 160.dp, look, fill = Pc.LightPanel, corner = 24.dp, side = ArrowSide.TOP) {
                Column(Modifier.padding(start = 8.dp, end = 8.dp, bottom = 8.dp)) {
                    Text("Image source options", color = Pc.LightText, fontSize = 22.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 12.dp, top = 16.dp, bottom = 12.dp))
                    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(Pc.LightRow)) {
                        MenuRow(PcIcons.SourcePhotoFill, "Import a photo", iconSize = 30.dp) { onPick(0) }
                        Box(Modifier.padding(start = 52.dp).fillMaxWidth().height(1.dp).background(Pc.LightSeparator))
                        MenuRow(PcIcons.FolderFill, "Import a file", iconSize = 30.dp) { onPick(1) }
                        Box(Modifier.padding(start = 52.dp).fillMaxWidth().height(1.dp).background(Pc.LightSeparator))
                        MenuRow(PcIcons.SourceBooksFill, "Source Library", iconSize = 30.dp) { onPick(2) }
                    }
                    Spacer(Modifier.height(20.dp))
                    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(Pc.LightRow)) {
                        MenuRow(PcIcons.TextPaste, "Paste", iconSize = 40.dp) { onPick(3) }
                    }
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
 * The source library: a sheet (580 x 575, 74 down) over the editor with its title and Cancel, Shape
 * Source and Grain Source with a search by name, then the shipped images three across (147 square,
 * their names under them). A pick of the other kind than the one being edited isn't taken.
 */
@Composable
private fun SourceLibrary(vm: CanvasViewModel, grain: Boolean, onPick: (Int) -> Unit, onCancel: () -> Unit) {
    var showGrains by remember { mutableStateOf(grain) }
    var query by remember { mutableStateOf("") }
    val all: List<Pair<String, ImageBitmap?>> = if (showGrains) vm.grainLibrary else vm.shapeLibrary
    val shown = all.withIndex().filter { query.isBlank() || it.value.first.contains(query.trim(), ignoreCase = true) }
    // over the whole window, the studio's pages dimmed too, the sheet in the window's middle
    val atOrigin = remember {
        object : androidx.compose.ui.window.PopupPositionProvider {
            override fun calculatePosition(
                anchorBounds: androidx.compose.ui.unit.IntRect,
                windowSize: androidx.compose.ui.unit.IntSize,
                layoutDirection: androidx.compose.ui.unit.LayoutDirection,
                popupContentSize: androidx.compose.ui.unit.IntSize,
            ) = androidx.compose.ui.unit.IntOffset.Zero
        }
    }
    PcPopup(atOrigin, onDismissRequest = onCancel, properties = PopupProperties(focusable = true, clippingEnabled = false)) {
    Box(Modifier.fillMaxSize().background(LibraryVeil).pointerInput(Unit) { awaitEachGesture { awaitFirstDown().consume() } }) {
        Column(
            Modifier.align(Alignment.TopCenter).offset(y = 74.dp).size(580.dp, 575.dp).clip(RoundedCornerShape(26.dp)).background(LibrarySheet)
                .pointerInput(Unit) { awaitEachGesture { awaitFirstDown(requireUnconsumed = false) } },
        ) {
            Row(Modifier.fillMaxWidth().padding(start = 19.dp, end = 19.dp, top = 69.dp).height(39.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("Source Library", color = LibraryTitle, fontSize = 32.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.weight(1f))
                Text("Cancel", color = LibraryDim, fontSize = 17.sp, maxLines = 1, modifier = Modifier.tap(onCancel))
            }
            Row(Modifier.padding(start = 43.dp, top = 25.dp).height(45.dp), verticalAlignment = Alignment.CenterVertically) {
                for ((label, isGrain) in listOf("Shape Source" to false, "Grain Source" to true)) {
                    Box(Modifier.padding(end = 11.dp).size(115.dp, 41.dp).tap { showGrains = isGrain }, contentAlignment = Alignment.Center) {
                        Text(label, color = if (showGrains == isGrain) Pc.MenuInk else LibraryDim, fontSize = 17.sp, maxLines = 1)
                    }
                }
                Spacer(Modifier.width(8.dp))
                LibrarySearch(query) { query = it }
            }
            LazyVerticalGrid(
                columns = GridCells.Fixed(3),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 11.dp, bottom = 24.dp),
                horizontalArrangement = Arrangement.spacedBy(53.dp),
                verticalArrangement = Arrangement.spacedBy(18.dp),
                modifier = Modifier.fillMaxSize(),
            ) {
                items(shown.size) { k ->
                    val (i, item) = shown[k]
                    val (name, image) = item
                    Column(
                        Modifier.width(147.dp).tap {
                            // each kind takes its own: a grain picked in the shape editor is not taken
                            if (showGrains == grain) onPick(i)
                        },
                    ) {
                        Box(Modifier.size(147.dp, 146.dp).clip(RoundedCornerShape(8.dp)).background(Color.Black).border(1.dp, LibraryEdge, RoundedCornerShape(8.dp))) {
                            image?.let { Image(it, contentDescription = name, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize()) }
                        }
                        Text(name, color = LibraryTitle, fontSize = 13.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.padding(start = 3.dp, top = 5.dp).height(21.dp))
                    }
                }
            }
        }
    }
    }
}

/** The library's search: a round field with a magnifier and its hint, matching names as typed. */
@Composable
private fun LibrarySearch(query: String, onQuery: (String) -> Unit) {
    Row(
        Modifier.size(225.dp, 45.dp).clip(RoundedCornerShape(22.5.dp)).background(LibrarySearchFill).border(1.dp, LibrarySearchEdge, RoundedCornerShape(22.5.dp)),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        androidx.compose.foundation.Canvas(Modifier.padding(start = 11.dp).size(23.dp, 22.dp)) {
            // a magnifier: a ring and its handle
            val c = androidx.compose.ui.geometry.Offset(size.width * 0.44f, size.height * 0.44f)
            val r = 7.dp.toPx()
            drawCircle(LibraryHint, r, c, style = androidx.compose.ui.graphics.drawscope.Stroke(2.dp.toPx()))
            val d = r * 0.7071f
            drawLine(LibraryHint, c + androidx.compose.ui.geometry.Offset(d, d), c + androidx.compose.ui.geometry.Offset(d + 5.dp.toPx(), d + 5.dp.toPx()), 2.4.dp.toPx(), cap = androidx.compose.ui.graphics.StrokeCap.Round)
        }
        Box(Modifier.padding(start = 9.dp, end = 12.dp).weight(1f), contentAlignment = Alignment.CenterStart) {
            if (query.isEmpty()) Text("Search Library", color = LibraryHint, fontSize = 17.sp, maxLines = 1)
            androidx.compose.foundation.text.BasicTextField(
                query,
                onValueChange = onQuery,
                singleLine = true,
                textStyle = androidx.compose.ui.text.TextStyle(color = Pc.MenuInk, fontSize = 17.sp),
                cursorBrush = androidx.compose.ui.graphics.SolidColor(Pc.Accent),
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
