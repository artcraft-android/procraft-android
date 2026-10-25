package io.github.artcraftandroid.procraft.ui

import androidx.compose.animation.togetherWith
import kotlinx.coroutines.launch
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.activity.compose.BackHandler
import android.graphics.BitmapFactory
import android.graphics.BlurMaskFilter
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.requiredWidth
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Paint
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.gallery.Artwork
import io.github.artcraftandroid.procraft.gallery.CanvasPreset
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.spring
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntOffset
import kotlin.math.roundToInt
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlin.math.max
import kotlin.math.min

/**
 * A gallery cell's sizes: its pitch both ways (the screen's width over the columns, three upright
 * and four on its side), the content's width (40 less) and the thumbnail box's height (82 less).
 */
private data class CellSize(val pitch: Dp, val width: Dp, val thumb: Dp)

private fun cellSize(screenWidth: Dp, columns: Int): CellSize {
    val pitch = screenWidth / columns
    return CellSize(pitch, pitch - 40.dp, pitch - 82.dp)
}

private val LocalCell = androidx.compose.runtime.staticCompositionLocalOf { cellSize(747.dp, 3) }
private val NewCanvasWidth = 347.dp

/** How long an artwork can take to open before the gallery says it is opening. */
private const val SLOW_OPENING_MS = 500L

/** The gallery: every artwork as a thumbnail; + makes a new canvas. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun GalleryScreen(vm: CanvasViewModel) {
    var newCanvas by remember { mutableStateOf(false) }
    // where each stack's cell is (window pixels), for its artworks to fly out of as it opens
    val stackCellAt = remember { mutableMapOf<String, androidx.compose.ui.geometry.Rect>() }
    var renaming by remember { mutableStateOf<Artwork?>(null) }
    var deleting by remember { mutableStateOf<Artwork?>(null) }
    // the artwork (by folder) whose actions a swipe left is showing
    var swiped by remember { mutableStateOf<String?>(null) }
    // Select: the artworks picked, by folder
    var selecting by remember { mutableStateOf(false) }
    val selected = remember { mutableStateListOf<String>() }
    var moreOpen by remember { mutableStateOf(false) }
    // Share's formats, open for these artworks
    var sharing by remember { mutableStateOf<GalleryShare?>(null) }
    // artworks waiting for their PDF's quality
    var pdfFor by remember { mutableStateOf<List<Artwork>?>(null) }
    var deletingSelected by remember { mutableStateOf(false) }
    var previewing by remember { mutableStateOf<List<Artwork>?>(null) }
    val context = androidx.compose.ui.platform.LocalContext.current
    val scope = rememberCoroutineScope()
    fun picked() = vm.artworks.filter { it.dir.name in selected }
    fun endSelecting() {
        selecting = false
        selected.clear()
        moreOpen = false
    }
    // several at once, as a model's material file and images come with it
    val importFile = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (uris.isNotEmpty()) vm.importFiles(uris)
    }
    val photo = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia()) { uri ->
        uri?.let(vm::newFromPicture)
    }
    if (vm.openStack != null && !selecting) BackHandler { vm.showStack(null) }
    BoxWithConstraints(Modifier.fillMaxSize().background(Pc.GalleryBackground)) {
        val (screenWidth, screenHeight) = maxWidth to maxHeight
        vm.windowPx = androidx.compose.ui.unit.IntSize(constraints.maxWidth, constraints.maxHeight)
        // out of focus while an artwork is being renamed
        val blur by androidx.compose.animation.core.animateDpAsState(
            if (renaming != null) RenameBlur else 0.dp,
            if (renaming != null) tween(300) else tween(150),
            label = "blur",
        )
        Column(Modifier.fillMaxSize().then(if (blur > 0.dp) Modifier.blur(blur) else Modifier)) {
            if (selecting) {
                SelectHeader(
                    title = vm.openStack?.name ?: "ProCraft",
                    any = selected.isNotEmpty(),
                    onShare = { sharing = GalleryShare(picked(), null) },
                    onDelete = { deletingSelected = true },
                    onMore = { moreOpen = !moreOpen },
                    onDone = ::endSelecting,
                )
            } else {
                GalleryHeader(
                    stack = vm.openStack?.name,
                    onBack = { vm.showStack(null) },
                    onSelect = { selecting = true },
                    onNew = { newCanvas = !newCanvas },
                    onImport = { importFile.launch(arrayOf("*/*")) },
                    onPhoto = { photo.launch(androidx.activity.result.PickVisualMediaRequest(androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia.ImageOnly)) },
                )
            }
            // into a stack the gallery goes at once and the stack's artworks fly out of its cell to
            // their places; back out, they fly back into it as the gallery grows back into place
            // under them, fading in
            androidx.compose.animation.AnimatedContent(
                targetState = vm.openStack to vm.galleryEntries,
                contentKey = { it.first?.dir?.path },
                transitionSpec = {
                    if (targetState.first != null && initialState.first == null) {
                        androidx.compose.animation.EnterTransition.None togetherWith androidx.compose.animation.fadeOut(snap())
                    } else {
                        // the stack's artworks fly over the gallery coming back
                        androidx.compose.animation.ContentTransform(
                            androidx.compose.animation.fadeIn(Motion.stackPop()) +
                                androidx.compose.animation.scaleIn(Motion.stackPop(), initialScale = Motion.STACK_POP_SCALE),
                            androidx.compose.animation.ExitTransition.KeepUntilTransitionsFinished,
                            targetContentZIndex = -1f,
                            sizeTransform = null,
                        )
                    }
                },
                label = "stack",
            ) { (stack, entries) ->
                // where the stack's cell was, for its artworks to fly out of (and back into)
                val stackFrom = remember(stack) { stack?.let { stackCellAt[it.dir.name] } }
                val leaving = transition.targetState == androidx.compose.animation.EnterExitState.PostExit
                if (entries.isEmpty()) {
                    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                        Text("Tap + to start a new canvas", color = Pc.GalleryText, fontSize = 17.sp)
                    }
                } else {
                    val columns = if (screenWidth > screenHeight) 4 else 3
                    val cell = cellSize(screenWidth, columns)
                    androidx.compose.runtime.CompositionLocalProvider(LocalCell provides cell) {
                    // the whole screen under the header, so artworks flying in from a stack lower down
                    // aren't clipped by a grid of one short row
                    LazyVerticalGrid(
                        modifier = Modifier.fillMaxSize(),
                        state = if (stack == null) vm.galleryGrid else androidx.compose.foundation.lazy.grid.rememberLazyGridState(),
                        columns = GridCells.Fixed(columns),
                        contentPadding = PaddingValues(bottom = 24.dp),
                    ) {
                        items(entries, key = { it.dir.name }) { entry ->
                            if (entry is io.github.artcraftandroid.procraft.gallery.Stack) {
                                StackCell(entry, vm.galleryVersion, dim = selecting, onPlaced = { stackCellAt[entry.dir.name] = it }) {
                                    if (!selecting) vm.showStack(entry)
                                }
                                return@items
                            }
                            val a = entry as Artwork
                            val chosen = a.dir.name in selected
                            val view = LocalView.current
                            // swiped open, it fades back and its actions grow in over it
                            val open = androidx.compose.animation.core.animateFloatAsState(
                                if (swiped == a.dir.name) 1f else 0f, Motion.swipeActions(), label = "swipe",
                            ).value
                            // held, its thumbnail shrinks, then lifts (to be dragged out), and settles when let go
                            val lift = remember { androidx.compose.animation.core.Animatable(1f) }
                            val cellScope = rememberCoroutineScope()
                            Box(Modifier.flyIn(stackFrom, Motion.stackPushMove()).flyOut(stackFrom, leaving, Motion.stackPop())) {
                                // across the whole column, so the thumbnail sits in its middle as a stack's does
                                Column(
                                    Modifier.fillMaxWidth().height(cell.pitch).graphicsLayer { alpha = 1f - SWIPED_FADE * open.coerceIn(0f, 1f) }.pointerInput(a, selecting, chosen) {
                                        // a tap opens it (or picks it), and on its name renames it; swiped left, its
                                        // actions show (right, they go); held and moved, it is dragged out to other apps
                                        awaitEachGesture {
                                            val down = awaitFirstDown()
                                            // a moment held before it shrinks, so a scroll starting on it leaves it be
                                            val press = cellScope.launch {
                                                kotlinx.coroutines.delay(Motion.ARTWORK_PRESS_DELAY_MS)
                                                lift.animateTo(Motion.ARTWORK_PRESSED, Motion.artworkPress())
                                            }
                                            try {
                                                var up: androidx.compose.ui.input.pointer.PointerInputChange? = null
                                                var moved: Offset? = null
                                                val held = withTimeoutOrNull(viewConfiguration.longPressTimeoutMillis) {
                                                    while (up == null && moved == null) {
                                                        val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                                                        val d = c.position - down.position
                                                        if (!c.pressed) up = c else if (d.getDistance() > viewConfiguration.touchSlop) moved = d
                                                    }
                                                    false
                                                } ?: true
                                                val d = moved
                                                if (d != null) {
                                                    if (!selecting && kotlin.math.abs(d.x) > kotlin.math.abs(d.y)) swiped = if (d.x < 0f) a.dir.name else null
                                                    return@awaitEachGesture
                                                }
                                                if (!held) {
                                                    up?.let {
                                                        it.consume()
                                                        when {
                                                            // a tap puts away the actions showing
                                                            swiped != null -> swiped = null
                                                            !selecting && it.position.y > (39.dp + cell.thumb + 8.dp).toPx() -> renaming = a
                                                            !selecting -> vm.openArtwork(a)
                                                            chosen -> selected.remove(a.dir.name)
                                                            else -> selected.add(a.dir.name)
                                                        }
                                                    }
                                                    return@awaitEachGesture
                                                }
                                                if (selecting) return@awaitEachGesture
                                                view.performHapticFeedback(android.view.HapticFeedbackConstants.LONG_PRESS)
                                                cellScope.launch { lift.animateTo(Motion.ARTWORK_LIFTED, Motion.artworkLift()) }
                                                while (true) {
                                                    val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                                                    c.consume()
                                                    if (!c.pressed) break
                                                    if ((c.position - down.position).getDistance() > viewConfiguration.touchSlop) {
                                                        io.github.artcraftandroid.procraft.gallery.startArtworkDrag(view, a, vm.store.root)
                                                        break
                                                    }
                                                }
                                            } finally {
                                                press.cancel()
                                                cellScope.launch { lift.animateTo(1f, Motion.artworkDrop()) }
                                            }
                                        }
                                    },
                                    horizontalAlignment = Alignment.CenterHorizontally,
                                ) {
                                    Box(
                                        Modifier.padding(top = 39.dp).size(cell.width, cell.thumb).graphicsLayer {
                                            scaleX = lift.value
                                            scaleY = lift.value
                                        },
                                        contentAlignment = Alignment.BottomCenter,
                                    ) {
                                        // where it is, for zooming into it, unless the gallery is zooming itself
                                        Thumbnail(a, vm.galleryVersion, if (selecting) chosen else null) { if (vm.galleryZoom == null) vm.thumbAt[a.dir.name] = it }
                                    }
                                    Text(
                                        a.name,
                                        color = if (selecting && chosen) Color.White else Pc.GalleryText,
                                        fontSize = 13.sp,
                                        fontWeight = FontWeight.SemiBold,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                        modifier = Modifier.padding(top = 12.dp),
                                    )
                                    Text("${a.width} × ${a.height}px", color = Pc.GallerySize, fontSize = 10.sp)
                                }
                                if (open > 0f) {
                                    SwipeActions(
                                        open,
                                        // centred on the thumbnail's frame
                                        Modifier.align(Alignment.TopCenter).padding(top = 39.dp + cell.thumb / 2 - SwipePillHeight / 2),
                                        onShare = { at -> sharing = GalleryShare(listOf(a), at) },
                                        onDuplicate = {
                                            swiped = null
                                            vm.duplicateArtwork(a)
                                        },
                                        onDelete = {
                                            swiped = null
                                            deleting = a
                                        },
                                    )
                                }
                            }
                        }
                    }
                    }
                }
            }
        }

        if (newCanvas) Box(Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures { newCanvas = false } })
        // hangs from the + button, 2 dp from the screen's edge, and grows out of its arrow
        val newLeft = maxWidth - 2.dp - NewCanvasWidth
        val newArrow = maxWidth - 49.dp - newLeft
        MenuPresence(newCanvas, androidx.compose.ui.graphics.TransformOrigin(newArrow / NewCanvasWidth, 0f)) { look ->
            NewCanvasPanel(
                Modifier.offset(x = newLeft, y = 56.5.dp).then(look), arrowX = newArrow, presets = vm.canvasPresets,
                // Custom Canvas opens over the list, which is still there when it closes
                onCustom = { vm.openCustomCanvas() },
                onEdit = { vm.openCustomCanvas(it) },
                onDelete = vm::deletePreset,
                onPick = {
                    newCanvas = false
                    vm.newPresetArtwork(it)
                },
            ) { w, h ->
                newCanvas = false
                vm.newArtwork(w, h)
            }
        }
        if (moreOpen) Box(Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures { moreOpen = false } })
        // under More, kept 4 from the screen's edge
        val morePlace = popoverPlace(250.dp, maxWidth - 20.dp - 44.dp - 13.dp - 61.dp / 2, maxWidth, minRight = 4.dp)
        MenuPresence(moreOpen, androidx.compose.ui.graphics.TransformOrigin(morePlace.arrowX / 250.dp, 0f)) { look ->
            val place = morePlace
            MoreOptions(
                Modifier.offset(x = place.left, y = 60.dp).then(look),
                place.arrowX,
                count = selected.size,
                canStack = vm.openStack == null,
                onStack = {
                    moreOpen = false
                    vm.stackArtworks(picked())
                    endSelecting()
                },
                onPreview = {
                    moreOpen = false
                    previewing = picked()
                },
                onDuplicate = {
                    moreOpen = false
                    picked().forEach(vm::duplicateArtwork)
                    endSelecting()
                },
            )
        }
        // Share's formats: beside a swiped artwork's Share, or under Select's
        var shown by remember { mutableStateOf<GalleryShare?>(null) }
        sharing?.let { shown = it }
        if (sharing != null) Box(Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures { sharing = null } })
        shown?.let { share ->
            val density = LocalDensity.current.density
            val from = share.from?.let { r -> androidx.compose.ui.geometry.Rect(r.left / density, r.top / density, r.right / density, r.bottom / density) }
            val place = formatsPlace(from, maxWidth, maxHeight, selectShareCentre(maxWidth))
            MenuPresence(sharing != null, place.origin) { look ->
                ImageFormats(Modifier.offset(x = place.left, y = place.top).then(look), place.side, place.arrowAt) { format ->
                    sharing = null
                    swiped = null
                    val animated = CanvasViewModel.Animated.entries.firstOrNull { it.title == format }
                    if (animated != null) {
                        share.artworks.firstOrNull()?.let { vm.openForAnimatedExport(it, animated) }
                        if (selecting) endSelecting()
                    } else if (format == "PDF") {
                        pdfFor = share.artworks
                    } else {
                        scope.launch {
                            val (files, mime) = vm.shareableFiles(share.artworks, format)
                            if (files.isNotEmpty()) shareFiles(context, files, mime) else vm.message = "The artwork could not be exported."
                        }
                    }
                }
            }
        }
        PdfQualityAlert(pdfFor != null, onCancel = { pdfFor = null }) { quality ->
            val list = pdfFor.orEmpty()
            pdfFor = null
            scope.launch {
                val (files, mime) = vm.shareableFiles(list, "PDF", quality.jpeg)
                if (files.isNotEmpty()) shareFiles(context, files, mime) else vm.message = "The artwork could not be exported."
            }
        }
        previewing?.let { list -> ArtworkPreview(list, vm.galleryVersion) { previewing = null } }
        // a sheet sliding up over the gallery, and back down
        androidx.compose.animation.AnimatedVisibility(
            vm.customCanvasOpen,
            enter = androidx.compose.animation.slideInVertically(Motion.studioIn()) { it },
            exit = androidx.compose.animation.slideOutVertically(Motion.studioOut()) { it },
        ) { CustomCanvasScreen(vm) { newCanvas = false } }
        // only a slow artwork (a big one) shows it is being opened; a quick one just zooms in
        var slowOpening by remember { mutableStateOf(false) }
        LaunchedEffect(vm.opening) {
            slowOpening = false
            if (vm.opening) {
                delay(SLOW_OPENING_MS)
                slowOpening = true
            }
        }
        if (vm.opening && slowOpening) {
            Box(Modifier.fillMaxSize().background(Color(0x66000000)), contentAlignment = Alignment.Center) {
                Text("Opening…", color = Pc.Text, fontSize = 17.sp)
            }
        }
        vm.message?.let { text ->
            LaunchedEffect(text) {
                delay(3000)
                vm.message = null
            }
        }
        FadingValue(renaming, Motion.renameFade()) { a, shown ->
            RenameArtwork(a, vm.galleryVersion, vm.thumbAt[a.dir.name], closing = renaming == null, shown = shown) { name ->
                if (name != a.name) vm.renameArtwork(a, name)
                renaming = null
            }
        }
        FadingValue(vm.message, Motion.message()) { text, shown ->
            Text(
                text,
                color = Pc.Text,
                modifier = Modifier.align(Alignment.BottomCenter).graphicsLayer { alpha = shown }.padding(32.dp).clip(RoundedCornerShape(10.dp))
                    .background(Pc.Panel).padding(horizontal = 18.dp, vertical = 12.dp),
            )
        }
    }

    // deleting the picked artworks, or one swiped to
    Alert(
        if (deletingSelected) selected.size else null,
        title = { n -> if (n == 1) "Delete 1 artwork?" else "Delete $n artworks?" },
        message = { "This can't be undone." },
        action = "Delete",
        icon = PcIcons.Trash,
        onCancel = { deletingSelected = false },
        onAction = {
            picked().forEach(vm::deleteArtwork)
            deletingSelected = false
            endSelecting()
        },
    )
    Alert(
        deleting,
        title = { a -> "Delete “${a.name}”?" },
        message = { "This can't be undone." },
        action = "Delete",
        icon = PcIcons.Trash,
        onCancel = { deleting = null },
        onAction = { a ->
            vm.deleteArtwork(a)
            deleting = null
        },
    )
}

/** The title row: the app's name, the select/import/photo capsule and the new-canvas button. */
@Composable
private fun GalleryHeader(stack: String?, onBack: () -> Unit, onSelect: () -> Unit, onNew: () -> Unit, onImport: () -> Unit, onPhoto: () -> Unit) {
    Box(Modifier.fillMaxWidth().height(55.dp)) {
        // into a stack the title changes at once; back out, the stack's name fades out as the
        // gallery's fades in
        androidx.compose.animation.AnimatedContent(
            stack,
            transitionSpec = {
                if (targetState == null) {
                    androidx.compose.animation.ContentTransform(
                        androidx.compose.animation.fadeIn(Motion.titleIn()),
                        androidx.compose.animation.fadeOut(Motion.titleOut()),
                        sizeTransform = null,
                    )
                } else {
                    androidx.compose.animation.ContentTransform(
                        androidx.compose.animation.EnterTransition.None,
                        androidx.compose.animation.ExitTransition.None,
                        sizeTransform = null,
                    )
                }
            },
            label = "title",
        ) { name ->
            if (name != null) {
                // in a stack: back to the gallery (blue while touched), and the stack's name
                val touch = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
                val ink = if (heldOf(touch)) Pc.Accent else Pc.GalleryTitle
                Row(
                    Modifier.padding(start = 20.dp, top = 12.dp).height(41.dp).clickable(touch, indication = null, onClick = onBack),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.width(40.dp), contentAlignment = Alignment.Center) {
                        Icon(PcIcons.ChevronLeft, contentDescription = "Back", tint = ink, modifier = Modifier.size(28.dp))
                    }
                    Text(name, color = ink, fontSize = 36.sp, fontWeight = FontWeight.Bold, maxLines = 1)
                }
            } else {
                Text(
                    "ProCraft",
                    color = Pc.GalleryTitle,
                    fontSize = 36.sp,
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.padding(start = 32.dp, top = 11.dp),
                )
            }
        }
        Row(Modifier.align(Alignment.TopEnd).padding(top = 10.dp, end = 20.dp), verticalAlignment = Alignment.CenterVertically) {
            Row(
                Modifier.height(45.dp).clip(CircleShape).background(Pc.Capsule).border(1.dp, Pc.CapsuleEdge, CircleShape),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                for ((label, width) in listOf("Select" to 69.dp, "Import" to 71.dp, "Photo" to 66.dp)) {
                    val action: () -> Unit = when (label) {
                        "Import" -> onImport
                        "Photo" -> onPhoto
                        else -> onSelect
                    }
                    Box(Modifier.width(width).height(45.dp).clickable(onClick = action), contentAlignment = Alignment.Center) {
                        Text(label, color = Pc.GalleryText, fontSize = 16.sp)
                    }
                }
            }
            Box(Modifier.padding(start = 12.dp).size(44.dp, 45.dp).clickable(onClick = onNew), contentAlignment = Alignment.Center) {
                Box(Modifier.size(42.dp).clip(CircleShape).background(Pc.Capsule).border(1.dp, Pc.CapsuleEdge, CircleShape))
                Icon(PcIcons.Add, contentDescription = "New canvas", tint = Pc.GalleryText, modifier = Modifier.requiredSize(40.dp))
            }
        }
    }
}

/**
 * Select's title row: Share, Delete and More (dimmed until an artwork is picked), and the blue
 * round button that ends selecting.
 */
@Composable
private fun SelectHeader(title: String, any: Boolean, onShare: () -> Unit, onDelete: () -> Unit, onMore: () -> Unit, onDone: () -> Unit) {
    Box(Modifier.fillMaxWidth().height(55.dp)) {
        Text(title, color = Pc.GalleryTitle, fontSize = 36.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 32.dp, top = 11.dp))
        Row(Modifier.align(Alignment.TopEnd).padding(top = 10.dp, end = 20.dp), verticalAlignment = Alignment.CenterVertically) {
            Row(Modifier.height(45.dp).clip(CircleShape).background(Pc.Capsule).border(1.dp, Pc.CapsuleEdge, CircleShape), verticalAlignment = Alignment.CenterVertically) {
                // More opens with nothing picked too (its rows greyed then)
                for ((label, width, action) in listOf(Triple("Share", 65.dp, onShare), Triple("Delete", 70.dp, onDelete), Triple("More", 61.dp, onMore))) {
                    val enabled = any || label == "More"
                    Box(Modifier.width(width).height(45.dp).clickable(enabled = enabled, onClick = action), contentAlignment = Alignment.Center) {
                        Text(label, color = if (enabled) Pc.GalleryTitle else Pc.GallerySize, fontSize = 16.sp)
                    }
                }
            }
            Box(Modifier.padding(start = 13.dp).size(44.dp, 45.dp).clip(CircleShape).background(Color(0xFF0190FF)).clickable(onClick = onDone), contentAlignment = Alignment.Center) {
                Icon(PcIcons.DoneTick, contentDescription = "Done", tint = Color.White, modifier = Modifier.requiredSize(40.dp))
            }
        }
    }
}

private val RenameBlur = 20.dp

/** Where a renamed artwork rises to: fitted in a 281 x 280 frame 20 from the top, the field 20 under it. */
private val RenameFrameWidth = 281.dp
private val RenameFrameHeight = 280.dp
private val RenameTop = 20.dp
private val RenameArtMargin = 12.dp
private val RenameFieldGap = 20.dp
private val RenameFieldWidth = 262.dp
private val RenameFieldHeight = 38.dp
private val RenameFieldFill = Color(0x14FFFFFF)

/**
 * Renaming an artwork: it rises from its cell ([from], window pixels) to the top middle, enlarged,
 * with its name in a field under it and the keyboard up. Done, or tapped away, it calls [onDone]
 * with the name and fades away ([closing], [shown]).
 */
@Composable
private fun RenameArtwork(a: Artwork, version: Int, from: androidx.compose.ui.geometry.Rect?, closing: Boolean, shown: Float, onDone: (String) -> Unit) {
    val image by produceState<ImageBitmap?>(null, a.dir.path, a.modified, version) {
        value = withContext(Dispatchers.IO) { BitmapFactory.decodeFile(a.thumbnail.path)?.asImageBitmap() }
    }
    var text by remember(a) { mutableStateOf(androidx.compose.ui.text.input.TextFieldValue(a.name, androidx.compose.ui.text.TextRange(0, a.name.length))) }
    val rise = remember(a) { androidx.compose.animation.core.Animatable(if (from == null) 1f else 0f) }
    LaunchedEffect(a) { rise.animateTo(1f, Motion.renameRise()) }
    val focus = remember { androidx.compose.ui.focus.FocusRequester() }
    LaunchedEffect(a) { focus.requestFocus() }
    var done by remember(a) { mutableStateOf(false) }
    // losing the focus it had (not the lack of it before it is first given) ends the renaming
    var focused by remember(a) { mutableStateOf(false) }
    fun finish() {
        if (done) return
        done = true
        onDone(text.text.trim().ifEmpty { a.name })
    }
    BackHandler(enabled = !closing) { finish() }
    val ratio = if (a.width > 0 && a.height > 0) a.width.toFloat() / a.height else 0.75f
    BoxWithConstraints(
        Modifier.fillMaxSize().graphicsLayer { alpha = if (closing) shown else 1f }
            .pointerInput(a) { detectTapGestures { finish() } },
    ) {
        val density = LocalDensity.current
        var origin by remember { mutableStateOf(Offset.Zero) }
        // the art fitted in its frame, centred across the screen
        val (fw, fh) = (RenameFrameWidth - RenameArtMargin * 2) to (RenameFrameHeight - RenameArtMargin * 2)
        val (aw, ah) = if (ratio > fw / fh) fw to fw / ratio else fh * ratio to fh
        val to = with(density) {
            val left = (maxWidth - aw).toPx() / 2f
            val top = (RenameTop + (RenameFrameHeight - ah) / 2).toPx()
            androidx.compose.ui.geometry.Rect(left, top, left + aw.toPx(), top + ah.toPx())
        }
        val start = from?.translate(-origin) ?: to
        Canvas(Modifier.fillMaxSize().onGloballyPositioned { origin = it.positionInWindow() }) {
            val at = androidx.compose.ui.geometry.lerp(start, to, rise.value)
            val corner = 8.dp.toPx()
            val outline = androidx.compose.ui.graphics.Path().apply {
                addRoundRect(androidx.compose.ui.geometry.RoundRect(at, androidx.compose.ui.geometry.CornerRadius(corner)))
            }
            drawPath(outline, Color.White)
            image?.let { img ->
                clipPath(outline) {
                    drawImage(
                        img,
                        dstOffset = IntOffset(at.left.toInt(), at.top.toInt()),
                        dstSize = IntSize(at.width.toInt().coerceAtLeast(1), at.height.toInt().coerceAtLeast(1)),
                    )
                }
            }
        }
        androidx.compose.foundation.text.BasicTextField(
            value = text,
            onValueChange = { text = it },
            singleLine = true,
            textStyle = androidx.compose.material3.LocalTextStyle.current.copy(color = Pc.Text, fontSize = 17.sp, textAlign = androidx.compose.ui.text.style.TextAlign.Center),
            cursorBrush = androidx.compose.ui.graphics.SolidColor(Pc.Accent),
            keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(imeAction = androidx.compose.ui.text.input.ImeAction.Done),
            keyboardActions = androidx.compose.foundation.text.KeyboardActions(onDone = { finish() }),
            modifier = Modifier.align(Alignment.TopCenter).padding(top = RenameTop + RenameFrameHeight + RenameFieldGap)
                .size(RenameFieldWidth, RenameFieldHeight).graphicsLayer { alpha = rise.value }
                .clip(RoundedCornerShape(10.dp)).background(RenameFieldFill).focusRequester(focus)
                .onFocusChanged {
                    if (it.isFocused) focused = true else if (focused) finish()
                },
            decorationBox = { field -> Box(Modifier.fillMaxSize().padding(horizontal = 12.dp), contentAlignment = Alignment.Center) { field() } },
        )
    }
}

/** How far a swiped artwork fades back under its actions. */
private const val SWIPED_FADE = 0.64f
private val SwipePillWidth = 94.dp
private val SwipePillHeight = 56.dp
private val SwipePillFill get() = if (Pc.light) Color(0xFFF5F5F5) else Color(0xDB1C1C1C)
private val SwipePillInk get() = if (Pc.light) Color(0xFF575757) else Color(0xFFA9A9A9)
private val SwipeDeleteFill = Color(0xFFFE604D)

/**
 * An artwork's actions, Share, Duplicate and Delete, in a row over it: they grow in from the right
 * as it is swiped open ([open], 0..1, springing), and shrink back as it closes.
 */
@Composable
private fun SwipeActions(open: Float, modifier: Modifier, onShare: (androidx.compose.ui.geometry.Rect) -> Unit, onDuplicate: () -> Unit, onDelete: () -> Unit) {
    var shareAt by remember { mutableStateOf(androidx.compose.ui.geometry.Rect.Zero) }
    Row(
        modifier.requiredWidth(SwipePillWidth * 3 + 6.dp).height(SwipePillHeight).graphicsLayer {
            val k = 0.2f + 0.8f * open
            scaleX = k
            scaleY = k
            alpha = open.coerceIn(0f, 1f)
            transformOrigin = androidx.compose.ui.graphics.TransformOrigin(1f, 0.5f)
        },
        horizontalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        for ((label, delete, action) in listOf(Triple("Share", false, { onShare(shareAt) }), Triple("Duplicate", false, onDuplicate), Triple("Delete", true, onDelete))) {
            Box(
                Modifier.size(SwipePillWidth, SwipePillHeight)
                    .then(if (label == "Share") Modifier.onGloballyPositioned { shareAt = it.boundsInWindow() } else Modifier)
                    .clip(RoundedCornerShape(12.dp))
                    .background(if (delete) SwipeDeleteFill else SwipePillFill).clickable(onClick = action),
                contentAlignment = Alignment.Center,
            ) {
                Text(label, color = if (delete) Color.White else SwipePillInk, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
            }
        }
    }
}

/** Artworks to share, and the swiped Share button's place (window pixels); none from Select. */
private class GalleryShare(val artworks: List<Artwork>, val from: androidx.compose.ui.geometry.Rect?)

private val FormatsWidth = 260.dp
private val FormatsTitleHeight = 55.dp
private val FormatsRow = 40.dp
private val FormatsHeight = FormatsTitleHeight + FormatsRow * 10 + 9.dp + 10.dp

/** Where Share's formats go: their top left, the arrow's side and place, and where they grow from. */
private class FormatsPlace(val left: Dp, val top: Dp, val side: ArrowSide, val arrowAt: Dp, val origin: androidx.compose.ui.graphics.TransformOrigin)

/** Select's Share button's centre: left of Delete and More, in the capsule 77 from the right. */
private fun selectShareCentre(screenWidth: Dp): Dp = screenWidth - 20.dp - 44.dp - 13.dp - 61.dp - 70.dp - 65.dp / 2

/**
 * Beside a swiped artwork's Share ([from], dp), pointing at it, on its right where they fit and
 * else on its left, kept 2 inside the screen; with no button, under Select's Share, centred on it
 * and 15 below it, kept 4 inside.
 */
private fun formatsPlace(from: androidx.compose.ui.geometry.Rect?, screenWidth: Dp, screenHeight: Dp, selectShare: Dp): FormatsPlace {
    if (from == null) {
        val left = (selectShare - FormatsWidth / 2).coerceIn(4.dp, (screenWidth - 4.dp - FormatsWidth).coerceAtLeast(4.dp))
        return FormatsPlace(left, 69.dp, ArrowSide.NONE, 0.dp, androidx.compose.ui.graphics.TransformOrigin((selectShare - left) / FormatsWidth, 0f))
    }
    val gap = 2.dp
    val centreY = from.center.y.dp
    val top = (centreY - FormatsHeight / 2).coerceIn(2.dp, (screenHeight - 2.dp - FormatsHeight).coerceAtLeast(2.dp))
    val arrowAt = (centreY - top).coerceIn(Pc.PopoverCorner + Pc.ArrowHeight, FormatsHeight - Pc.PopoverCorner - Pc.ArrowHeight)
    val total = FormatsWidth + Pc.ArrowHeight
    val y = arrowAt / FormatsHeight
    return if (from.right.dp + gap + total <= screenWidth - 2.dp) {
        FormatsPlace(from.right.dp + gap, top, ArrowSide.LEFT, arrowAt, androidx.compose.ui.graphics.TransformOrigin(0f, y))
    } else {
        FormatsPlace((from.left.dp - gap - total).coerceAtLeast(2.dp), top, ArrowSide.RIGHT, arrowAt, androidx.compose.ui.graphics.TransformOrigin(1f, y))
    }
}

/** Share's formats, the animated ones last: a tap on one shares in it. */
@Composable
private fun ImageFormats(modifier: Modifier, side: ArrowSide, arrowAt: Dp, onPick: (String) -> Unit) {
    Popover(FormatsWidth, arrowAt, modifier, fill = Pc.GalleryPopover, side = side) {
        Box(Modifier.height(FormatsTitleHeight).padding(start = 22.dp), contentAlignment = Alignment.CenterStart) {
            Text("Image Format", color = Pc.PanelTitle, fontSize = 24.sp, fontWeight = FontWeight.Bold)
        }
        Column(Modifier.padding(start = 10.dp, end = 10.dp, bottom = 10.dp).clip(RoundedCornerShape(18.dp)).background(Pc.ActionsGroup)) {
            for ((i, format) in CanvasViewModel.SHARE_FORMATS.withIndex()) {
                if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
                Box(Modifier.fillMaxWidth().height(FormatsRow).clickable { onPick(format) }.padding(start = 16.dp), contentAlignment = Alignment.CenterStart) {
                    Text(format, color = Pc.PanelTitle, fontSize = 17.sp)
                }
            }
        }
    }
}

/** More options for the picked artworks: Preview, Stack (two or more), Duplicate. */
@Composable
private fun MoreOptions(modifier: Modifier, arrowX: Dp, count: Int, canStack: Boolean, onStack: () -> Unit, onPreview: () -> Unit, onDuplicate: () -> Unit) {
    Popover(250.dp, arrowX, modifier, fill = Pc.GalleryPopover) {
        Text("More Options", color = Pc.PanelTitle, fontSize = 24.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 24.dp, top = 14.dp, bottom = 12.dp))
        Column(Modifier.padding(start = 10.dp, end = 10.dp, bottom = 12.dp).clip(RoundedCornerShape(18.dp)).background(Pc.ActionsGroup)) {
            for ((i, row) in listOf(Triple("Preview", true, onPreview), Triple("Stack", canStack && count >= 2, onStack), Triple("Duplicate", true, onDuplicate)).withIndex()) {
                val (label, ready, action) = row
                val enabled = ready && count > 0
                if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
                Box(Modifier.fillMaxWidth().height(44.dp).clickable(enabled = enabled, onClick = action).padding(start = 16.dp), contentAlignment = Alignment.CenterStart) {
                    Text(label, color = if (enabled) Pc.PanelTitle else Pc.ActionsDisabled, fontSize = 17.sp)
                }
            }
        }
    }
}

/** A stack: its newest artwork on two more cards, the name, and how many it holds; a tap opens it. */
@Composable
private fun StackCell(
    s: io.github.artcraftandroid.procraft.gallery.Stack,
    version: Int,
    dim: Boolean,
    onPlaced: (androidx.compose.ui.geometry.Rect) -> Unit,
    onOpen: () -> Unit,
) {
    val top = s.artworks.firstOrNull() ?: return
    val cell = LocalCell.current
    Column(
        Modifier.height(cell.pitch).onGloballyPositioned { onPlaced(it.boundsInWindow()) }.clickable(onClick = onOpen),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(Modifier.padding(top = 39.dp).size(cell.width, cell.thumb), contentAlignment = Alignment.BottomCenter) {
            // the cards under the top one, a little turned
            for (turn in listOf(5f, -4f)) {
                Box(
                    Modifier.fillMaxHeight().aspectRatio(0.75f).graphicsLayer { rotationZ = turn }
                        .clip(RoundedCornerShape(8.dp)).background(Color(0xFFF2F2F2)),
                )
            }
            Thumbnail(top, version)
        }
        Text(s.name, color = if (dim) Pc.GallerySize else Pc.GalleryText, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, modifier = Modifier.padding(top = 12.dp))
        Text("${s.artworks.size} artworks", color = Pc.GallerySize, fontSize = 10.sp)
    }
}

/** The picked artworks one at a time over the whole screen; swiped through, tapped to close. */
@Composable
private fun ArtworkPreview(list: List<Artwork>, version: Int, onClose: () -> Unit) {
    var index by remember { mutableIntStateOf(0) }
    val a = list.getOrNull(index) ?: return onClose()
    BackHandler(onBack = onClose)
    Box(
        Modifier.fillMaxSize().background(Color.Black).pointerInput(list) {
            var moved = 0f
            detectHorizontalDragGestures(
                onDragStart = { moved = 0f },
                onDragEnd = {
                    if (moved < -60f && index < list.size - 1) index++ else if (moved > 60f && index > 0) index--
                },
            ) { c, dx ->
                c.consume()
                moved += dx
            }
        }.pointerInput(Unit) { detectTapGestures { onClose() } },
        contentAlignment = Alignment.Center,
    ) {
        val image by produceState<ImageBitmap?>(null, a.dir.path, version) {
            value = withContext(Dispatchers.IO) { BitmapFactory.decodeFile(a.thumbnail.path)?.asImageBitmap() }
        }
        image?.let { Image(it, contentDescription = a.name, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize()) }
    }
}

/**
 * An artwork's thumbnail, as large as fits in its cell's box, with a soft shadow; while selecting,
 * a circle at its top right ([chosen]: filled blue with a tick).
 */
@Composable
private fun Thumbnail(a: Artwork, version: Int, chosen: Boolean? = null, onPlaced: (androidx.compose.ui.geometry.Rect) -> Unit = {}) {
    val image by produceState<ImageBitmap?>(null, a.dir.path, a.modified, version) {
        value = withContext(Dispatchers.IO) { BitmapFactory.decodeFile(a.thumbnail.path)?.asImageBitmap() }
    }
    val ratio = if (a.width > 0 && a.height > 0) a.width.toFloat() / a.height else 0.75f
    val cell = LocalCell.current
    val tall = ratio <= cell.width / cell.thumb
    Box(
        Modifier.then(if (tall) Modifier.fillMaxHeight() else Modifier.fillMaxWidth())
            .aspectRatio(ratio, matchHeightConstraintsFirst = tall)
            .onGloballyPositioned { onPlaced(it.boundsInWindow()) }
            .drawBehind {
                drawIntoCanvas { canvas ->
                    val shadow = Paint().apply {
                        color = Color(0x80000000)
                        asFrameworkPaint().maskFilter = BlurMaskFilter(10.dp.toPx(), BlurMaskFilter.Blur.NORMAL)
                    }
                    canvas.drawRoundRect(0f, 0f, size.width, size.height, 8.dp.toPx(), 8.dp.toPx(), shadow)
                }
            }
            .clip(RoundedCornerShape(8.dp))
            .background(Color.White),
    ) {
        image?.let { Image(it, contentDescription = a.name, contentScale = ContentScale.FillBounds, modifier = Modifier.fillMaxSize()) }
        // the marks fade in as Select starts, and out as it ends
        FadingValue(chosen, Motion.selectMode()) { chosen, shown ->
            Box(
                Modifier.align(Alignment.TopEnd).padding(top = 12.dp, end = 12.dp).size(14.dp).graphicsLayer { alpha = shown }.clip(CircleShape)
                    // unpicked, a thin light ring
                    .background(if (chosen) Color(0xFF0F7EFD) else Color.Transparent)
                    .then(if (chosen) Modifier else Modifier.border(1.dp, Color(0xFFCCCCCC), CircleShape)),
                contentAlignment = Alignment.Center,
            ) {
                if (chosen) Icon(PcIcons.MenuTick, contentDescription = "Picked", tint = Color.White, modifier = Modifier.size(10.dp))
            }
        }
    }
}

/**
 * The new-canvas list: the screen's size first, then the presets, each with its colour profile and
 * size; a preset row swiped left shows Edit and Delete.
 */
@Composable
private fun NewCanvasPanel(
    modifier: Modifier,
    arrowX: Dp,
    presets: List<CanvasPreset>,
    onCustom: () -> Unit,
    onEdit: (Int) -> Unit,
    onDelete: (Int) -> Unit,
    onPick: (CanvasPreset) -> Unit,
    onScreenSize: (Int, Int) -> Unit,
) {
    val root = LocalView.current.rootView
    val (sw, sh) = min(root.width, root.height).coerceAtLeast(1) to max(root.width, root.height).coerceAtLeast(1)
    // the row swiped open, if one is
    var opened by remember { mutableStateOf<Int?>(null) }
    Popover(NewCanvasWidth, arrowX, modifier, fill = Pc.LightPanel, corner = 32.dp) {
        Box(Modifier.fillMaxWidth()) {
            Text(
                "New canvas",
                color = Pc.LightText,
                fontSize = 26.sp,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(start = 23.dp, top = 21.dp, bottom = 20.dp),
            )
            // the button that makes a custom canvas
            Box(
                Modifier.align(Alignment.TopEnd).padding(top = 18.dp, end = 13.dp).size(44.dp, 34.dp).clickable(onClick = onCustom),
                contentAlignment = Alignment.Center,
            ) {
            Box(
                Modifier.size(27.dp, 16.dp).drawBehind {
                    drawRoundRect(Pc.LightText, cornerRadius = CornerRadius(4.dp.toPx()))
                    val w = 1.6.dp.toPx()
                    val c = Offset(8.dp.toPx(), size.height / 2f)
                    val arm = 3.6.dp.toPx()
                    drawLine(Pc.LightPanel, c - Offset(arm, 0f), c + Offset(arm, 0f), w, StrokeCap.Round)
                    drawLine(Pc.LightPanel, c - Offset(0f, arm), c + Offset(0f, arm), w, StrokeCap.Round)
                },
            )
            }
        }
        PresetRow("Screen Size", "$sw × ${sh}px", CanvasPreset.P3, Modifier.padding(horizontal = 9.dp).height(38.dp).clip(CircleShape).background(Pc.LightRowStrong)) { onScreenSize(sw, sh) }
        // the presets scroll past 376 tall
        Column(
            Modifier.padding(start = 9.dp, end = 9.dp, top = 9.5.dp, bottom = 30.dp).heightIn(max = 376.dp).clip(RoundedCornerShape(24.dp)).background(Pc.LightRow)
                .verticalScroll(rememberScrollState()),
        ) {
            presets.forEachIndexed { i, p ->
                if (i > 0) Box(Modifier.padding(start = 16.dp).fillMaxWidth().height(1.dp).background(Pc.LightSeparator))
                SwipeRow(
                    p, Modifier.height(if (i > 0) 44.dp else 45.dp), open = opened == i, onOpen = { opened = if (it) i else null },
                    onEdit = {
                        opened = null
                        onEdit(i)
                    },
                    onDelete = {
                        opened = null
                        onDelete(i)
                    },
                ) {
                    if (opened != null) opened = null else onPick(p)
                }
            }
        }
    }
}

// A swiped preset row: how far it slides, and the two buttons it shows
private val SwipeReveal = 159.dp
private val SwipeBehind = Color(0xFFD6D6D6)
private val EditFill = Color(0xFFC6C6C6)
private val EditInk = Color(0xFF575757)
private val DeleteFill = Color(0xFFFE604D)

/** A preset row that slides left under a finger to show Edit and Delete; a tap picks it (or closes it, open). */
@Composable
private fun SwipeRow(p: CanvasPreset, modifier: Modifier, open: Boolean, onOpen: (Boolean) -> Unit, onEdit: () -> Unit, onDelete: () -> Unit, onClick: () -> Unit) {
    val reveal = with(LocalDensity.current) { SwipeReveal.toPx() }
    var dragging by remember { mutableStateOf(false) }
    var dragX by remember { mutableFloatStateOf(0f) }
    val x by animateFloatAsState(if (dragging) dragX else if (open) -reveal else 0f, if (dragging) snap() else Motion.swipeSettle(), label = "swipe")
    Box(modifier.fillMaxWidth()) {
        Row(
            Modifier.matchParentSize().background(SwipeBehind).padding(end = 13.dp),
            horizontalArrangement = Arrangement.End,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SwipeButton("Edit", EditFill, EditInk, onEdit)
            Spacer(Modifier.width(3.dp))
            SwipeButton("Delete", DeleteFill, Color.White, onDelete)
        }
        PresetRow(
            p.name, p.label, p.profile,
            Modifier.fillMaxSize().offset { IntOffset(x.roundToInt(), 0) }.background(Pc.LightRow).pointerInput(Unit) {
                detectHorizontalDragGestures(
                    onDragStart = {
                        dragX = x
                        dragging = true
                    },
                    onDragEnd = {
                        dragging = false
                        onOpen(dragX < -reveal / 2f)
                    },
                    onDragCancel = { dragging = false },
                ) { change, dx ->
                    change.consume()
                    dragX = (dragX + dx).coerceIn(-reveal, 0f)
                }
            },
            onClick,
        )
    }
}

@Composable
private fun SwipeButton(label: String, fill: Color, ink: Color, onClick: () -> Unit) {
    Box(Modifier.size(69.dp, 33.dp).clip(RoundedCornerShape(7.dp)).background(fill).clickable(onClick = onClick), contentAlignment = Alignment.Center) {
        Text(label, color = ink, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
    }
}

@Composable
private fun PresetRow(name: String, label: String, profile: String, modifier: Modifier, onClick: () -> Unit) {
    Row(modifier.fillMaxWidth().clickable(onClick = onClick).padding(start = 17.dp, end = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(name, color = Pc.LightText, fontSize = 17.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
        Chip(CanvasPreset.chip(profile))
        Chip(label, Modifier.padding(start = 4.dp))
    }
}

@Composable
private fun Chip(text: String, modifier: Modifier = Modifier) {
    Box(modifier.height(18.dp).clip(CircleShape).background(Pc.LightChip).padding(horizontal = 8.dp), contentAlignment = Alignment.Center) {
        Text(text, color = Pc.LightText, fontSize = 13.sp, maxLines = 1)
    }
}
