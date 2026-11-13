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
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.foundation.draganddrop.dragAndDropTarget
import androidx.compose.ui.draganddrop.toAndroidDragEvent
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.FilterQuality
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
import androidx.lifecycle.viewModelScope
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.gallery.Artwork
import io.github.artcraftandroid.procraft.gallery.GalleryEntry
import io.github.artcraftandroid.procraft.gallery.CanvasPreset
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.spring
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntOffset
import kotlin.math.roundToInt
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

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
    // a stack renamed as an artwork is: its name tapped
    var renamingStack by remember { mutableStateOf<io.github.artcraftandroid.procraft.gallery.Stack?>(null) }
    // the app's name tapped: what it is and its version, over the gallery
    var about by remember { mutableStateOf(false) }
    // Gallery Recovery's card: how far its pie has come while it shows, and whether it is done
    var recovery by remember { mutableStateOf<Float?>(null) }
    var recovered by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf<Artwork?>(null) }
    // the artwork (by folder) whose actions a swipe left is showing
    var swiped by remember { mutableStateOf<String?>(null) }
    // a drag in the gallery: the artwork dragged, and the cell it would be stacked onto (by folder);
    // where it can go, and the way back out of a stack lit while it rests there
    var dragging by remember { mutableStateOf<String?>(null) }
    var dropOnto by remember { mutableStateOf<String?>(null) }
    val dropPlaces = remember { DropPlaces() }
    var backLit by remember { mutableStateOf(false) }
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
    // which of them Preview starts on
    var previewFrom by remember { mutableIntStateOf(0) }
    // an artwork two fingers are spreading open into Preview
    var opening by remember { mutableStateOf<PinchOpening?>(null) }
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
            if (renaming != null || renamingStack != null || about) RenameBlur else 0.dp,
            if (renaming != null || renamingStack != null || about) tween(300) else tween(150),
            label = "blur",
        )
        val dropCell = cellSize(screenWidth, if (screenWidth > screenHeight) 4 else 3)
        Column(
            Modifier.fillMaxSize().then(if (blur > 0.dp) Modifier.blur(blur) else Modifier).then(
                galleryDrop(
                    dropPlaces, vm.galleryEntries, dropCell, stacking = vm.openStack == null,
                    onBack = { vm.showStack(null) },
                    onOpen = vm::showStack,
                    onBackLit = { backLit = it },
                    onOver = { dropOnto = it },
                    onEnded = {
                        dragging = null
                        dropOnto = null
                    },
                    onMove = vm::moveInGallery,
                    onStack = vm::dropOnto,
                    onFiles = vm::importFiles,
                ),
            ),
        ) {
            if (selecting) {
                val screen = androidx.compose.ui.platform.LocalConfiguration.current
                SelectHeader(
                    title = vm.openStack?.name ?: "ProCraft",
                    count = selected.size,
                    wide = screen.screenWidthDp > screen.screenHeightDp,
                    canStack = vm.openStack == null,
                    onStack = {
                        vm.stackArtworks(picked())
                        endSelecting()
                    },
                    onPreview = {
                        previewFrom = 0
                        previewing = picked()
                    },
                    onShare = { sharing = GalleryShare(picked(), null) },
                    onDuplicate = {
                        picked().forEach(vm::duplicateArtwork)
                        endSelecting()
                    },
                    onDelete = { deletingSelected = true },
                    onMore = { moreOpen = !moreOpen },
                    onDone = ::endSelecting,
                )
            } else {
                GalleryHeader(
                    stack = vm.openStack?.name,
                    backLit = backLit,
                    onBackPlaced = { dropPlaces.back = it },
                    onBack = { vm.showStack(null) },
                    onAbout = { about = true },
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
                    val grid = if (stack == null) vm.galleryGrid else androidx.compose.foundation.lazy.grid.rememberLazyGridState()
                    LazyVerticalGrid(
                        // the grid showing (not one going) is where drags land
                        modifier = Modifier.fillMaxSize().onGloballyPositioned {
                            if (stack?.dir == vm.openStack?.dir) {
                                dropPlaces.grid = grid
                                dropPlaces.gridAt = it.positionInRoot()
                                dropPlaces.gridHeight = it.size.height.toFloat()
                            }
                        },
                        state = grid,
                        columns = GridCells.Fixed(columns),
                        contentPadding = PaddingValues(bottom = 24.dp),
                    ) {
                        items(entries, key = { it.dir.name }) { entry ->
                            if (entry is io.github.artcraftandroid.procraft.gallery.Stack) {
                                val faint = androidx.compose.animation.core.animateFloatAsState(
                                    if (dragging == entry.dir.name) DRAGGED_ALPHA else 1f, Motion.artworkLift(), label = "dragged",
                                )
                                val view = LocalView.current
                                val density = androidx.compose.ui.platform.LocalDensity.current
                                StackCell(
                                    entry, vm.galleryVersion, dim = selecting, wash = dropOnto == entry.dir.name,
                                    modifier = Modifier.animateItem(fadeInSpec = null, fadeOutSpec = null).graphicsLayer { alpha = faint.value },
                                    onPlaced = { stackCellAt[entry.dir.name] = it },
                                    onDrag = { touch ->
                                        // its top artwork's picture, lifted, held where the finger is on it
                                        val placed = stackCellAt[entry.dir.name]
                                        val top = entry.artworks.firstOrNull()
                                        if (!selecting && placed != null && top != null) {
                                            val box = with(density) {
                                                androidx.compose.ui.geometry.Rect(
                                                    placed.center.x - cell.width.toPx() / 2f, placed.top + 39.dp.toPx(),
                                                    placed.center.x + cell.width.toPx() / 2f, placed.top + 39.dp.toPx() + cell.thumb.toPx(),
                                                )
                                            }
                                            val picture = fitted(box, top)
                                            val held = (touch - picture.topLeft) * Motion.ARTWORK_LIFTED
                                            val w = (picture.width * Motion.ARTWORK_LIFTED).toInt()
                                            val h = (picture.height * Motion.ARTWORK_LIFTED).toInt()
                                            if (io.github.artcraftandroid.procraft.gallery.startStackDrag(view, entry, w, h, android.graphics.Point(held.x.toInt(), held.y.toInt()))) {
                                                dragging = entry.dir.name
                                            }
                                        }
                                    },
                                    onRename = { if (!selecting) renamingStack = entry },
                                ) {
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
                            // held, its thumbnail shrinks, then lifts (to be dragged), and settles when let go
                            val lift = remember { androidx.compose.animation.core.Animatable(1f) }
                            val cellScope = rememberCoroutineScope()
                            // dragged, it stays behind faint
                            val faint = androidx.compose.animation.core.animateFloatAsState(
                                if (dragging == a.dir.name) DRAGGED_ALPHA else 1f, Motion.artworkLift(), label = "dragged",
                            )
                            // where the cell is in the window, for where the drag picture is held
                            val cellAt = remember { arrayOf(Offset.Zero) }
                            Box(Modifier.animateItem(fadeInSpec = null, fadeOutSpec = null).flyIn(stackFrom, Motion.stackPushMove()).flyOut(stackFrom, leaving, Motion.stackPop())) {
                                // across the whole column, so the thumbnail sits in its middle as a stack's does
                                Column(
                                    Modifier.fillMaxWidth().height(cell.pitch).onGloballyPositioned { cellAt[0] = it.positionInWindow() }.graphicsLayer {
                                        alpha = (1f - SWIPED_FADE * open.coerceIn(0f, 1f)) * faint.value *
                                            (if (opening?.artwork?.dir?.name == a.dir.name) 0f else 1f)
                                    }.pointerInput(a, selecting, chosen) {
                                        // a tap opens it (or picks it), and on its name renames it; swiped left, its
                                        // actions show (right, they go); held and moved, it is dragged: within the
                                        // gallery to move it or stack it, or out to other apps; two fingers spread
                                        // on it open it in Preview
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
                                                var pinched = false
                                                val held = withTimeoutOrNull(viewConfiguration.longPressTimeoutMillis) {
                                                    while (up == null && moved == null) {
                                                        val e = awaitPointerEvent()
                                                        if (e.changes.count { it.pressed } >= 2) {
                                                            pinched = true
                                                            break
                                                        }
                                                        val c = e.changes.firstOrNull { it.id == down.id } ?: break
                                                        val d = c.position - down.position
                                                        if (!c.pressed) up = c else if (d.getDistance() > viewConfiguration.touchSlop) moved = d
                                                    }
                                                    false
                                                } ?: true
                                                if (pinched) {
                                                    // the picture comes out of its place and follows the fingers, the
                                                    // gallery darkening behind it; let go, it opens or goes back
                                                    press.cancel()
                                                    val shown = vm.thumbAt[a.dir.name]
                                                    if (selecting || shown == null) return@awaitEachGesture
                                                    val o = PinchOpening(a, shown).also { opening = it }
                                                    var from: Float? = null
                                                    var middle: Offset? = null
                                                    while (true) {
                                                        val e = awaitPointerEvent()
                                                        val fingers = e.changes.filter { it.pressed }
                                                        if (fingers.size < 2) break
                                                        e.changes.forEach { it.consume() }
                                                        val apart = (fingers[0].position - fingers[1].position).getDistance()
                                                        val between = (fingers[0].position + fingers[1].position) / 2f
                                                        val start = from ?: apart.also { from = it }
                                                        val first = middle ?: between.also { middle = it }
                                                        o.scale = (apart / start.coerceAtLeast(1f)).coerceAtLeast(PINCH_LEAST)
                                                        o.moved = between - first
                                                    }
                                                    o.letGo = true
                                                    return@awaitEachGesture
                                                }
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
                                                        // the picture as big as the lifted thumbnail, held where the finger is on it
                                                        val shown = vm.thumbAt[a.dir.name]
                                                        val scale = Motion.ARTWORK_LIFTED / lift.value
                                                        val w = ((shown?.width ?: cell.width.toPx()) * scale).toInt()
                                                        val h = ((shown?.height ?: cell.thumb.toPx()) * scale).toInt()
                                                        val touch = cellAt[0] + c.position - (shown?.topLeft ?: cellAt[0])
                                                        val held = android.graphics.Point((touch.x * scale).toInt(), (touch.y * scale).toInt())
                                                        if (io.github.artcraftandroid.procraft.gallery.startArtworkDrag(view, a, vm.store.root, w, h, held)) dragging = a.dir.name
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
                                        Thumbnail(a, vm.galleryVersion, if (selecting) chosen else null, wash = dropOnto == a.dir.name) {
                                            if (vm.galleryZoom == null) vm.thumbAt[a.dir.name] = it
                                        }
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
                kinds = vm.plugins.kinds(),
                onKind = { kind, w, h ->
                    newCanvas = false
                    vm.newArtwork(w, h, kind.key)
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
                    previewFrom = 0
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
                        // with the export card; a failure is told, a cancel shares nothing
                        vm.viewModelScope.launch {
                            vm.shareFromGallery(share.artworks, format)?.let { (files, mime) -> runCatching { shareFiles(context, files, mime) } }
                        }
                    }
                }
            }
        }
        PdfQualityAlert(pdfFor != null, onCancel = { pdfFor = null }) { quality ->
            val list = pdfFor.orEmpty()
            pdfFor = null
            vm.viewModelScope.launch {
                vm.shareFromGallery(list, "PDF", quality.jpeg)?.let { (files, mime) -> runCatching { shareFiles(context, files, mime) } }
            }
        }
        opening?.let { o ->
            PinchOpeningOverlay(o, vm.galleryVersion) { opened ->
                if (opened) {
                    val all = vm.artworks
                    previewFrom = all.indexOfFirst { it.dir.name == o.artwork.dir.name }.coerceAtLeast(0)
                    previewing = all
                }
                opening = null
            }
        }
        previewing?.let { list -> ArtworkPreview(list, vm.galleryVersion, previewFrom) { previewing = null } }
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
        // a stack: its top artwork shown, under the stack's name
        FadingValue(renamingStack, Motion.renameFade()) { st, shown ->
            val top = st.artworks.firstOrNull()
            if (top == null) {
                renamingStack = null
                return@FadingValue
            }
            RenameArtwork(top.copy(name = st.name), vm.galleryVersion, vm.thumbAt[top.dir.name], closing = renamingStack == null, shown = shown) { name ->
                if (name != st.name) vm.renameStack(st, name)
                renamingStack = null
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
        androidx.compose.animation.AnimatedVisibility(
            about,
            enter = androidx.compose.animation.fadeIn(tween(300)),
            exit = androidx.compose.animation.fadeOut(tween(150)),
        ) {
            AboutApp(
                onRecover = {
                    if (recovery == null) {
                        scope.launch {
                            // the card shows long enough to be read, however quick the search
                            recovery = 0f
                            val pie = launch { androidx.compose.animation.core.animate(0f, 0.95f, animationSpec = tween(RECOVERY_SEARCH_MS)) { v, _ -> recovery = v } }
                            vm.recoverLostArtworks()
                            pie.join()
                            recovered = true
                            kotlinx.coroutines.delay(RECOVERY_DONE_MS)
                            recovery = null
                            recovered = false
                            about = false
                        }
                    }
                },
            ) { about = false }
        }
        RecoveryProgress(recovery, recovered)
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
private fun GalleryHeader(
    stack: String?,
    backLit: Boolean,
    onBackPlaced: (androidx.compose.ui.geometry.Rect) -> Unit,
    onBack: () -> Unit,
    onAbout: () -> Unit,
    onSelect: () -> Unit,
    onNew: () -> Unit,
    onImport: () -> Unit,
    onPhoto: () -> Unit,
) {
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
                // in a stack: back to the gallery (blue while touched, or while an artwork dragged
                // rests on it to go back), and the stack's name
                val touch = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
                val ink = if (heldOf(touch) || backLit) Pc.Accent else Pc.GalleryTitle
                Row(
                    Modifier.padding(start = 20.dp, top = 12.dp).height(41.dp).onGloballyPositioned { onBackPlaced(it.boundsInRoot()) }
                        .clickable(touch, indication = null, onClick = onBack),
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
                    modifier = Modifier.padding(start = 32.dp, top = 11.dp)
                        .clickable(remember { androidx.compose.foundation.interaction.MutableInteractionSource() }, indication = null, onClick = onAbout),
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

/** One of Select's buttons: its label, width, whether it can act now, and what it does. */
private class SelectAction(val label: String, val width: Dp, val enabled: Boolean, val onClick: () -> Unit)

/**
 * Select's title row: upright, Share, Delete and More; on a wide screen Stack, Preview, Share,
 * Duplicate and Delete side by side ([wide]). Each is dimmed until it can act on the [count]
 * picked artworks (Stack on two or more; More always opens, its rows greyed). Then the blue round
 * button that ends selecting.
 */
@Composable
private fun SelectHeader(
    title: String,
    count: Int,
    wide: Boolean,
    canStack: Boolean,
    onStack: () -> Unit,
    onPreview: () -> Unit,
    onShare: () -> Unit,
    onDuplicate: () -> Unit,
    onDelete: () -> Unit,
    onMore: () -> Unit,
    onDone: () -> Unit,
) {
    val any = count > 0
    val actions = if (wide) {
        listOf(
            SelectAction("Stack", 65.dp, canStack && count >= 2, onStack),
            SelectAction("Preview", 80.dp, any, onPreview),
            SelectAction("Share", 65.dp, any, onShare),
            SelectAction("Duplicate", 91.dp, any, onDuplicate),
            SelectAction("Delete", 70.dp, any, onDelete),
        )
    } else {
        listOf(SelectAction("Share", 65.dp, any, onShare), SelectAction("Delete", 70.dp, any, onDelete), SelectAction("More", 61.dp, true, onMore))
    }
    Box(Modifier.fillMaxWidth().height(55.dp)) {
        Text(title, color = Pc.GalleryTitle, fontSize = 36.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 32.dp, top = 11.dp))
        Row(Modifier.align(Alignment.TopEnd).padding(top = 10.dp, end = 20.dp), verticalAlignment = Alignment.CenterVertically) {
            Row(Modifier.height(45.dp).clip(CircleShape).background(Pc.Capsule).border(1.dp, Pc.CapsuleEdge, CircleShape), verticalAlignment = Alignment.CenterVertically) {
                for (a in actions) {
                    Box(Modifier.width(a.width).height(45.dp).clickable(enabled = a.enabled, onClick = a.onClick), contentAlignment = Alignment.Center) {
                        Text(a.label, color = if (a.enabled) Pc.GalleryTitle else Pc.GallerySize, fontSize = 16.sp)
                    }
                }
            }
            Box(Modifier.padding(start = 13.dp).size(44.dp, 45.dp).clip(CircleShape).background(Pc.tinted(Color(0xFF0190FF))).clickable(onClick = onDone), contentAlignment = Alignment.Center) {
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
    // the last one decoded shows while it loads again (an artwork opened or closed, saved meanwhile),
    // so a canvas of another colour never flashes white
    val image by produceState(ThumbnailsShown.get(a.thumbnail.path), a.dir.path, a.modified, version) {
        val decoded = withContext(Dispatchers.IO) { BitmapFactory.decodeFile(a.thumbnail.path)?.asImageBitmap() }
        if (decoded != null) ThumbnailsShown.put(a.thumbnail.path, decoded)
        value = decoded ?: value
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
                        filterQuality = thumbnailFilter(img, at.width, density.density),
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

/** How faint an artwork's cell stays while it is dragged. */
private const val DRAGGED_ALPHA = 0.3f

/** The blue over an artwork (or stack) a dragged artwork would be stacked with. */
private val DropWash = Color(0x800064DC)

/** How long a dragged artwork rests on a stack (or a stack's way back) before it opens (the gallery). */
private const val SPRING_MS = 600L

/** Where the gallery's own drags land: the grid showing (its state, and its place and height in the root), and a stack's way back. */
private class DropPlaces {
    var grid: androidx.compose.foundation.lazy.grid.LazyGridState? = null
    var gridAt = Offset.Zero
    var gridHeight = 0f
    var back: androidx.compose.ui.geometry.Rect? = null
}

/** How near the grid's top or bottom a drag scrolls it, and how far each frame. */
private val DragScrollEdge = 60.dp
private val DragScrollStep = 12.dp

/** Artwork [a]'s picture as it sits in its thumbnail's [box]: fitted, standing on the box's bottom. */
private fun fitted(box: androidx.compose.ui.geometry.Rect, a: Artwork): androidx.compose.ui.geometry.Rect {
    val ratio = if (a.width > 0 && a.height > 0) a.width.toFloat() / a.height else 0.75f
    val w = if (ratio <= box.width / box.height) box.height * ratio else box.width
    val h = w / ratio
    return androidx.compose.ui.geometry.Rect(box.center.x - w / 2f, box.bottom - h, box.center.x + w / 2f, box.bottom)
}

/** Where a drag over the gallery's grid is: the cell it would be stacked onto, or else the slot it would go in. */
private data class DropAt(val onto: String?, val slot: Int)

/**
 * Where point [p] (the grid's pixels) is over [entries] laid out in [grid]: over another's picture
 * (the thumbnail as it sits in its box, or a stack's box) it is that one, when [stacking]; anywhere
 * else the slot between cells it is nearest, before a cell's middle or after it.
 */
private fun dropAt(grid: androidx.compose.foundation.lazy.grid.LazyGridState, entries: List<GalleryEntry>, dragged: String, p: Offset, top: Float, box: androidx.compose.ui.geometry.Size, stacking: Boolean): DropAt? {
    val items = grid.layoutInfo.visibleItemsInfo
    for (info in items) {
        val x = info.offset.x.toFloat()
        val y = info.offset.y.toFloat()
        if (p.x < x || p.x >= x + info.size.width || p.y < y || p.y >= y + info.size.height) continue
        val e = entries.getOrNull(info.index) ?: return null
        val left = x + (info.size.width - box.width) / 2f
        val inBox = androidx.compose.ui.geometry.Rect(left, y + top, left + box.width, y + top + box.height)
        val picture = if (e is Artwork) fitted(inBox, e) else inBox
        if (stacking && e.dir.name != dragged && picture.contains(p)) return DropAt(e.dir.name, info.index)
        return DropAt(null, if (p.x < x + info.size.width / 2f) info.index else info.index + 1)
    }
    // past the last cell: last
    val last = items.lastOrNull() ?: return null
    return if (last.index == entries.size - 1 && (p.y >= last.offset.y + last.size.height || (p.y >= last.offset.y && p.x >= last.offset.x + last.size.width))) DropAt(null, entries.size) else null
}

/**
 * The gallery taking its own drags: let go over another artwork's picture (or a stack's), the
 * dragged artwork is stacked with it ([onOver] names it meanwhile), anywhere else on the grid it
 * goes where it is let go. Near the grid's top or bottom the grid scrolls. An artwork resting on a
 * stack opens it ([onOpen]) and, in a stack, on the way back ([onBackLit] meanwhile) opens the
 * gallery ([onBack]), the drag going on, so it can be put in a stack where it is let go, or taken
 * out of one.
 */
@Composable
private fun galleryDrop(
    places: DropPlaces,
    entries: List<GalleryEntry>,
    cell: CellSize,
    stacking: Boolean,
    onBack: () -> Unit,
    onOpen: (io.github.artcraftandroid.procraft.gallery.Stack) -> Unit,
    onBackLit: (Boolean) -> Unit,
    onOver: (String?) -> Unit,
    onEnded: () -> Unit,
    onMove: (GalleryEntry, Int) -> Unit,
    onStack: (Artwork, GalleryEntry) -> Unit,
    onFiles: (List<android.net.Uri>) -> Unit,
): Modifier {
    val now = androidx.compose.runtime.rememberUpdatedState(entries)
    val inGallery = androidx.compose.runtime.rememberUpdatedState(stacking)
    val back = androidx.compose.runtime.rememberUpdatedState(onBack)
    val open = androidx.compose.runtime.rememberUpdatedState(onOpen)
    val backLit = androidx.compose.runtime.rememberUpdatedState(onBackLit)
    val over = androidx.compose.runtime.rememberUpdatedState(onOver)
    val ended = androidx.compose.runtime.rememberUpdatedState(onEnded)
    val move = androidx.compose.runtime.rememberUpdatedState(onMove)
    val stack = androidx.compose.runtime.rememberUpdatedState(onStack)
    // files dragged in from another app are imported, as Import's
    val files = androidx.compose.runtime.rememberUpdatedState(onFiles)
    val activity = androidx.compose.ui.platform.LocalContext.current.activity()
    val density = androidx.compose.ui.platform.LocalDensity.current
    val sizes = androidx.compose.runtime.rememberUpdatedState(
        with(density) { Triple(39.dp.toPx(), androidx.compose.ui.geometry.Size(cell.width.toPx(), cell.thumb.toPx()), DragScrollEdge.toPx()) },
    )
    val scope = rememberCoroutineScope()
    // -1 scrolling up, 1 down
    var edge by remember { mutableIntStateOf(0) }
    LaunchedEffect(edge) {
        val step = with(density) { DragScrollStep.toPx() }
        while (edge != 0) {
            places.grid?.scrollBy(edge * step)
            androidx.compose.runtime.withFrameNanos { }
        }
    }
    val target = remember {
        object : androidx.compose.ui.draganddrop.DragAndDropTarget {
            var at: DropAt? = null
            var spring: kotlinx.coroutines.Job? = null

            // what it rests on: "" the way back, else a stack (by folder)
            var springOn: String? = null

            fun stopSpring() {
                spring?.cancel()
                spring = null
                springOn = null
                backLit.value(false)
            }

            /** After a moment resting on [on] (the same each time it moves), [then]. */
            fun springOn(on: String, then: () -> Unit) {
                if (springOn == on) return
                stopSpring()
                springOn = on
                backLit.value(on.isEmpty())
                spring = scope.launch {
                    kotlinx.coroutines.delay(SPRING_MS)
                    stopSpring()
                    then()
                }
            }

            fun dragged(e: androidx.compose.ui.draganddrop.DragAndDropEvent) = (e.toAndroidDragEvent().localState as? io.github.artcraftandroid.procraft.gallery.GalleryDrag)?.entry

            fun track(e: androidx.compose.ui.draganddrop.DragAndDropEvent) {
                val d = e.toAndroidDragEvent()
                val entry = dragged(e) ?: return
                val r = Offset(d.x, d.y)
                if (!inGallery.value && places.back?.contains(r) == true) {
                    at = null
                    edge = 0
                    over.value(null)
                    springOn("") { back.value() }
                    return
                }
                val p = r - places.gridAt
                val (top, box, near) = sizes.value
                edge = when {
                    p.y < near -> -1
                    p.y > places.gridHeight - near -> 1
                    else -> 0
                }
                at = places.grid?.let { dropAt(it, now.value, entry.dir.name, p, top, box, inGallery.value && entry is Artwork) }
                over.value(at?.onto)
                val stack = at?.onto?.let { name -> now.value.firstOrNull { it.dir.name == name } } as? io.github.artcraftandroid.procraft.gallery.Stack
                if (stack != null) springOn(stack.dir.name) { open.value(stack) } else stopSpring()
            }

            override fun onEntered(event: androidx.compose.ui.draganddrop.DragAndDropEvent) = track(event)

            override fun onMoved(event: androidx.compose.ui.draganddrop.DragAndDropEvent) = track(event)

            override fun onExited(event: androidx.compose.ui.draganddrop.DragAndDropEvent) {
                at = null
                edge = 0
                stopSpring()
                over.value(null)
            }

            override fun onDrop(event: androidx.compose.ui.draganddrop.DragAndDropEvent): Boolean {
                val e = dragged(event) ?: run {
                    val uris = event.droppedFiles(activity)
                    if (uris.isNotEmpty()) files.value(uris)
                    return uris.isNotEmpty()
                }
                val drop = at ?: return true
                val list = now.value
                val onto = drop.onto?.let { name -> list.firstOrNull { it.dir.name == name } }
                if (onto != null && e is Artwork) {
                    stack.value(e, onto)
                } else {
                    // from a stack it comes in at the slot; from here, the slot counted without it
                    val from = list.indexOfFirst { it.dir.name == e.dir.name }
                    val to = if (from >= 0 && drop.slot > from) drop.slot - 1 else drop.slot
                    if (from < 0 || to != from) move.value(e, to)
                }
                return true
            }

            override fun onEnded(event: androidx.compose.ui.draganddrop.DragAndDropEvent) {
                at = null
                edge = 0
                stopSpring()
                ended.value()
            }
        }
    }
    return Modifier.dragAndDropTarget(
        shouldStartDragAndDrop = { it.toAndroidDragEvent().localState is io.github.artcraftandroid.procraft.gallery.GalleryDrag || it.fromElsewhere() },
        target = target,
    )
}
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
private val FormatsHeight = FormatsTitleHeight + FormatsRow * CanvasViewModel.SHARE_FORMATS.size + (CanvasViewModel.SHARE_FORMATS.size - 1).dp + 10.dp

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
    // on the side of the pill with more room (right of a pill left of the middle), the other if
    // that side is too narrow
    val roomRight = screenWidth - 2.dp - from.right.dp - gap
    val roomLeft = from.left.dp - gap - 2.dp
    val right = if (roomRight >= roomLeft) roomRight >= total || roomLeft < total else roomLeft < total && roomRight >= total
    return if (right) {
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
    wash: Boolean,
    modifier: Modifier,
    onPlaced: (androidx.compose.ui.geometry.Rect) -> Unit,
    onDrag: (Offset) -> Unit,
    onRename: () -> Unit,
    onOpen: () -> Unit,
) {
    val top = s.artworks.firstOrNull() ?: return
    val cell = LocalCell.current
    val view = LocalView.current
    // where the cell is in the window, for where the drag picture is held
    val cellAt = remember { arrayOf(Offset.Zero) }
    Column(
        modifier.height(cell.pitch).onGloballyPositioned {
            onPlaced(it.boundsInWindow())
            cellAt[0] = it.positionInWindow()
        }.pointerInput(s) {
            // a tap opens it (on its name, renames it); held and moved, it is dragged ([onDrag], with
            // the finger's place in the window)
            awaitEachGesture {
                val down = awaitFirstDown()
                var up: androidx.compose.ui.input.pointer.PointerInputChange? = null
                var moved = false
                val held = withTimeoutOrNull(viewConfiguration.longPressTimeoutMillis) {
                    while (up == null && !moved) {
                        val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!c.pressed) up = c else if ((c.position - down.position).getDistance() > viewConfiguration.touchSlop) moved = true
                    }
                    false
                } ?: true
                if (!held) {
                    up?.let {
                        it.consume()
                        if (it.position.y > (39.dp + cell.thumb + 8.dp).toPx()) onRename() else onOpen()
                    }
                    return@awaitEachGesture
                }
                view.performHapticFeedback(android.view.HapticFeedbackConstants.LONG_PRESS)
                while (true) {
                    val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                    c.consume()
                    if (!c.pressed) break
                    if ((c.position - down.position).getDistance() > viewConfiguration.touchSlop) {
                        onDrag(cellAt[0] + c.position)
                        break
                    }
                }
            }
        },
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(Modifier.padding(top = 39.dp).size(cell.width, cell.thumb), contentAlignment = Alignment.BottomCenter) {
            // the top artwork drawn again on two cards under it, a little turned (none for one)
            if (s.artworks.size > 1) {
                for (turn in listOf(5f, -4f)) {
                    Box(Modifier.graphicsLayer { rotationZ = turn }, contentAlignment = Alignment.BottomCenter) { Thumbnail(top, version) }
                }
            }
            Thumbnail(top, version, wash = wash)
        }
        Text(s.name, color = if (dim) Pc.GallerySize else Pc.GalleryText, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, modifier = Modifier.padding(top = 12.dp))
        Text("${s.artworks.size} artworks", color = Pc.GallerySize, fontSize = 10.sp)
    }
}

/**
 * An artwork two fingers are spreading open: its thumbnail where it showed (window pixels), how far
 * the fingers have spread it and moved, and whether they have let go.
 */
private class PinchOpening(val artwork: Artwork, val from: androidx.compose.ui.geometry.Rect) {
    var scale by mutableFloatStateOf(1f)
    var moved by mutableStateOf(Offset.Zero)
    var letGo by mutableStateOf(false)
}

/** Pinched in, an artwork being spread open shrinks no further than this. */
private const val PINCH_LEAST = 0.6f

/**
 * An artwork spread open by two fingers ([o]): its picture follows them out of its thumbnail's
 * place, the gallery darkening behind it as it nears the whole screen; let go, it settles fitted
 * over black and [done] is told true (Preview takes over), or, spread too little, back into its
 * place and [done] is told false.
 */
@Composable
private fun PinchOpeningOverlay(o: PinchOpening, version: Int, done: (Boolean) -> Unit) {
    val image by produceState<ImageBitmap?>(null, o.artwork.dir.path, version) {
        value = withContext(Dispatchers.IO) { BitmapFactory.decodeFile(o.artwork.thumbnail.path)?.asImageBitmap() }
    }
    // after the fingers let go: from where they left it (0) to where it settles (1)
    val settle = remember { androidx.compose.animation.core.Animatable(0f) }
    var held by remember { mutableStateOf<androidx.compose.ui.geometry.Rect?>(null) }
    var opens by remember { mutableStateOf(false) }
    val density = LocalDensity.current
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val screen = with(density) { androidx.compose.ui.geometry.Size(maxWidth.toPx(), maxHeight.toPx()) }
        val from = o.from
        // fitted to the screen, as Preview shows it
        val fit = minOf(screen.width / from.width, screen.height / from.height)
        val full = androidx.compose.ui.geometry.Rect(
            Offset((screen.width - from.width * fit) / 2f, (screen.height - from.height * fit) / 2f),
            androidx.compose.ui.geometry.Size(from.width * fit, from.height * fit),
        )
        // following the fingers: grown about the thumbnail's middle and moved with them
        val following = androidx.compose.ui.geometry.Rect(
            from.center + o.moved - Offset(from.width * o.scale / 2f, from.height * o.scale / 2f),
            androidx.compose.ui.geometry.Size(from.width * o.scale, from.height * o.scale),
        )
        LaunchedEffect(o.letGo) {
            if (!o.letGo) return@LaunchedEffect
            held = following
            opens = o.scale >= Motion.ARTWORK_PINCH_OPEN
            settle.animateTo(1f, Motion.pinchSettle())
            done(opens)
        }
        val rect = held?.let { start -> androidx.compose.ui.geometry.lerp(start, if (opens) full else from, settle.value) } ?: following
        // how near the whole screen it is: the gallery darkens as far
        val near = ((rect.width - from.width) / (full.width - from.width).coerceAtLeast(1f)).coerceIn(0f, 1f)
        Box(Modifier.fillMaxSize().graphicsLayer { alpha = near }.background(Color.Black))
        Box(
            Modifier.offset { IntOffset(rect.left.roundToInt(), rect.top.roundToInt()) }
                .size(with(density) { rect.width.toDp() }, with(density) { rect.height.toDp() })
                .clip(RoundedCornerShape(8.dp * (1f - near))),
        ) {
            image?.let { ThumbnailImage(it, o.artwork.name, ContentScale.FillBounds) }
        }
    }
}

/**
 * Artworks one at a time over the whole screen, from the one at [start]: the picked ones, or the
 * gallery's from a thumbnail spread open; swiped through, closed by its cross (pinching in doesn't
 * close it, as measured).
 */
@Composable
private fun ArtworkPreview(list: List<Artwork>, version: Int, start: Int, onClose: () -> Unit) {
    var index by remember(list, start) { mutableIntStateOf(start) }
    // a tap shows (or hides) the close button and the arrows to the other picked artworks
    var controls by remember { mutableStateOf(false) }
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
        }.pointerInput(Unit) { detectTapGestures { controls = !controls } },
        contentAlignment = Alignment.Center,
    ) {
        val image by produceState<ImageBitmap?>(null, a.dir.path, version) {
            value = withContext(Dispatchers.IO) { BitmapFactory.decodeFile(a.thumbnail.path)?.asImageBitmap() }
        }
        // an animated artwork plays as its settings say, its thumbnail showing until its frames are read
        val side = androidx.compose.ui.platform.LocalConfiguration.current.let { with(androidx.compose.ui.platform.LocalDensity.current) { maxOf(it.screenWidthDp, it.screenHeightDp).dp.roundToPx() } }
        val played by produceState<PlayedFrames?>(null, a.dir.path, version) {
            value = null
            value = withContext(Dispatchers.Default) { readFrames(a.dir.path, side) }
        }
        var frame by remember(played) { mutableStateOf<ImageBitmap?>(null) }
        androidx.compose.runtime.LaunchedEffect(played) {
            val p = played ?: return@LaunchedEffect
            var at = 0u
            var step = 1
            while (true) {
                frame = p.pictures.getOrNull(p.order.getOrElse(at.toInt()) { 0 })
                kotlinx.coroutines.delay(1000L / p.fps.coerceAtLeast(1))
                // round again, turned back, or (a single pass) staying on its last frame
                val next = io.github.artcraftandroid.procraft.core.animationNextStep(p.playback, p.order.size.toUInt(), at, step) ?: break
                at = next.at
                step = next.step
            }
        }
        (frame ?: image)?.let { ThumbnailImage(it, a.name, ContentScale.Fit) }
        androidx.compose.animation.AnimatedVisibility(controls, Modifier.fillMaxSize(), enter = androidx.compose.animation.fadeIn(), exit = androidx.compose.animation.fadeOut()) {
            Box(Modifier.fillMaxSize()) {
                // a light disc with a black cross, 41 in from the top right
                Box(
                    Modifier.align(Alignment.TopEnd).offset(x = (-41).dp + 22.dp, y = 22.dp - 22.dp).size(44.dp).clickable(onClick = onClose),
                    contentAlignment = Alignment.Center,
                ) {
                    Box(Modifier.size(23.5.dp).clip(CircleShape).background(PreviewClose), contentAlignment = Alignment.Center) {
                        Icon(PcIcons.Cross, contentDescription = "Close", tint = Color.Black, modifier = Modifier.size(15.dp))
                    }
                }
                PreviewArrow(left = true, Modifier.align(Alignment.CenterStart)) { if (index > 0) index-- }
                PreviewArrow(left = false, Modifier.align(Alignment.CenterEnd)) { if (index < list.size - 1) index++ }
            }
        }
    }
}

/** An artwork's frames as read for Preview, and how they play. */
private class PlayedFrames(val pictures: List<ImageBitmap>, val order: List<Int>, val fps: Int, val playback: io.github.artcraftandroid.procraft.core.AnimationPlayback)

/** The most memory (bytes) an animation's frames take in Preview: past it they are smaller. */
private const val PLAYED_MOST = 256e6

/** The frames of the artwork in [dir] if it is animated, at no more than [side] pixels. Slow. */
private fun readFrames(dir: String, side: Int): PlayedFrames? {
    if (!io.github.artcraftandroid.procraft.core.artworkPlays(dir)) return null
    val s = runCatching { io.github.artcraftandroid.procraft.core.CanvasSession.open(dir) }.getOrNull() ?: return null
    try {
        if (s.modelInfo() != null) return readTurntable(s, side)
        val settings = s.animationSettings()?.takeIf { !it.pages } ?: return null
        if (!s.drawWithoutWindow()) return null
        val count = s.animationFrameCount()
        val fit = kotlin.math.sqrt(PLAYED_MOST / 4.0 / count.coerceAtLeast(1u).toDouble()).toInt()
        val pictures = (0u until count).mapNotNull { i ->
            s.animationFrame(i, false, minOf(side, fit).toUInt())?.let { t ->
                val bitmap = CanvasSpace.tag(android.graphics.Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), android.graphics.Bitmap.Config.ARGB_8888))
                bitmap.copyPixelsFromBuffer(java.nio.ByteBuffer.wrap(t.rgba))
                bitmap.asImageBitmap()
            }
        }
        return PlayedFrames(pictures, s.animationSequence().map { it.toInt() }, settings.fps.toInt(), settings.playback)
    } finally {
        s.destroy()
    }
}

/** A model going round, opened in [s], at no more than [side] pixels. */
private fun readTurntable(s: io.github.artcraftandroid.procraft.core.CanvasSession, side: Int): PlayedFrames? {
    if (!s.drawWithoutWindow()) return null
    val count = s.modelPreviewFrames()
    val fit = kotlin.math.sqrt(PLAYED_MOST / 4.0 / count.coerceAtLeast(1u).toDouble()).toInt()
    val pictures = (0u until count).mapNotNull { i ->
        s.modelPreviewPicture(i, minOf(side, fit).toUInt())?.let { t ->
            val bitmap = CanvasSpace.tag(android.graphics.Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), android.graphics.Bitmap.Config.ARGB_8888))
            bitmap.copyPixelsFromBuffer(java.nio.ByteBuffer.wrap(t.rgba))
            bitmap.asImageBitmap()
        }
    }
    return PlayedFrames(pictures, pictures.indices.toList(), s.modelPreviewFps().toInt(), io.github.artcraftandroid.procraft.core.AnimationPlayback.LOOP)
}

private val PreviewClose = Color(0xFFD7D7D7)
private val PreviewArrowInk = Color(0xFFB4B4B4)

/** One of Preview's arrows: a 13.5 x 23.5 chevron 40 in from its side, in a 44 wide touch area. */
@Composable
private fun PreviewArrow(left: Boolean, modifier: Modifier, onClick: () -> Unit) {
    Box(
        modifier.padding(start = if (left) 18.dp else 0.dp, end = if (left) 0.dp else 18.dp).size(44.dp, 60.dp).clickable(onClick = onClick)
            .drawBehind {
                // drawn a little larger than the 13.5 x 23.5 it shows (the stroke's edges soften)
                val w = 14.5.dp.toPx()
                val h = 25.5.dp.toPx()
                val stroke = 4.dp.toPx()
                val cx = size.width / 2f
                val tip = if (left) cx - w / 2f + stroke / 2f else cx + w / 2f - stroke / 2f
                val back = if (left) cx + w / 2f - stroke / 2f else cx - w / 2f + stroke / 2f
                val top = size.height / 2f - h / 2f + stroke / 2f
                val bottom = size.height / 2f + h / 2f - stroke / 2f
                val path = androidx.compose.ui.graphics.Path().apply {
                    moveTo(back, top)
                    lineTo(tip, size.height / 2f)
                    lineTo(back, bottom)
                }
                drawPath(path, PreviewArrowInk, style = androidx.compose.ui.graphics.drawscope.Stroke(stroke, join = androidx.compose.ui.graphics.StrokeJoin.Miter))
            },
    )
}

/**
 * How a thumbnail drawn [shown] pixels wide is filtered: at 100% or more (each of its pixels a
 * point or larger) a square of its own colour, as the canvas shows them; smaller, smoothed.
 */
private fun thumbnailFilter(image: ImageBitmap, shown: Float, density: Float) =
    if (shown >= image.width * density * 0.999f) FilterQuality.None else FilterQuality.Low

/** An artwork's thumbnail fitted in its box (or filling it, by [scale]), filtered by [thumbnailFilter]. */
@Composable
private fun ThumbnailImage(image: ImageBitmap, name: String, scale: ContentScale) {
    val density = LocalDensity.current.density
    var shown by remember { mutableFloatStateOf(0f) }
    // made again when the filter changes (Image(bitmap) keeps its first)
    val filter = thumbnailFilter(image, shown, density)
    val painter = remember(image, filter) { androidx.compose.ui.graphics.painter.BitmapPainter(image, filterQuality = filter) }
    Image(
        painter,
        contentDescription = name,
        contentScale = scale,
        modifier = Modifier.fillMaxSize().onSizeChanged {
            // fitted, as wide as the box lets its height
            shown = minOf(it.width.toFloat(), it.height * image.width / image.height.coerceAtLeast(1).toFloat())
        },
    )
}

// as measured: a thumbnail's shadow dropped below it, and the rim round it
private val ThumbShadowDrop = 5.dp
private val ThumbShadow get() = if (Pc.light) Color(0x1D000000) else Color(0x45000000)
private val ThumbRim get() = if (Pc.light) Color(0xFFBEBEBE) else Color(0xFF424242)

/** The thumbnails last decoded, by file: shown while one loads again. */
private val ThumbnailsShown = android.util.LruCache<String, ImageBitmap>(64)

/**
 * An artwork's thumbnail, as large as fits in its cell's box, with a soft shadow; while selecting,
 * a circle at its top right ([chosen]: filled blue with a tick).
 */
@Composable
private fun Thumbnail(a: Artwork, version: Int, chosen: Boolean? = null, wash: Boolean = false, onPlaced: (androidx.compose.ui.geometry.Rect) -> Unit = {}) {
    // the last one decoded shows while it loads again (an artwork opened or closed, saved meanwhile),
    // so a canvas of another colour never flashes white
    val image by produceState(ThumbnailsShown.get(a.thumbnail.path), a.dir.path, a.modified, version) {
        val decoded = withContext(Dispatchers.IO) { BitmapFactory.decodeFile(a.thumbnail.path)?.asImageBitmap() }
        if (decoded != null) ThumbnailsShown.put(a.thumbnail.path, decoded)
        value = decoded ?: value
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
                        color = ThumbShadow
                        // a Gaussian 2 across (the blur's radius as the framework takes it)
                        asFrameworkPaint().maskFilter = BlurMaskFilter((2.dp.toPx() - 0.5f) / 0.57735f, BlurMaskFilter.Blur.NORMAL)
                    }
                    val drop = ThumbShadowDrop.toPx()
                    canvas.drawRoundRect(0f, drop, size.width, size.height + drop, 8.dp.toPx(), 8.dp.toPx(), shadow)
                }
            }
            .clip(RoundedCornerShape(8.dp))
            // a picture with its background hidden shows the gallery through it (white while it loads)
            .background(if (image == null) Color.White else Pc.GalleryBackground)
            .border(1.dp, ThumbRim, RoundedCornerShape(8.dp)),
    ) {
        image?.let { ThumbnailImage(it, a.name, ContentScale.FillBounds) }
        // blue while an artwork dragged over it would be stacked with it
        if (wash) Box(Modifier.matchParentSize().background(DropWash))
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
 * size; a preset row swiped left shows Edit and Delete. Under them, a row for each kind of artwork a
 * plugin on makes, the screen's size.
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
    kinds: List<io.github.artcraftandroid.procraft.core.PluginKind> = emptyList(),
    onKind: (io.github.artcraftandroid.procraft.core.PluginKind, Int, Int) -> Unit = { _, _, _ -> },
    onScreenSize: (Int, Int) -> Unit,
) {
    // the screen as it is turned now (2560 x 1600 in landscape, 1600 x 2560 upright)
    val root = LocalView.current.rootView
    val (sw, sh) = root.width.coerceAtLeast(1) to root.height.coerceAtLeast(1)
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
            Modifier.padding(start = 9.dp, end = 9.dp, top = 9.5.dp, bottom = if (kinds.isEmpty()) 30.dp else 0.dp).heightIn(max = 376.dp).clip(RoundedCornerShape(24.dp)).background(Pc.LightRow)
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
        // the plugins' kinds of artwork, each the screen's size
        kinds.forEachIndexed { k, kind ->
            val bottom = if (k == kinds.lastIndex) 30.dp else 0.dp
            PresetRow(
                kind.label, "$sw × ${sh}px", CanvasPreset.P3,
                Modifier.padding(start = 9.dp, end = 9.dp, top = 9.5.dp, bottom = bottom).height(38.dp).clip(CircleShape).background(Pc.LightRowStrong),
            ) { onKind(kind, sw, sh) }
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

/**
 * What the app is, over the gallery (blurred and dimmed under it): its icon 300 across, its name,
 * its version; a round button at the top right closes it, as back does.
 */
@Composable
private fun AboutApp(onRecover: () -> Unit, onClose: () -> Unit) {
    BackHandler(onBack = onClose)
    val context = androidx.compose.ui.platform.LocalContext.current
    val version = remember {
        runCatching {
            val info = context.packageManager.getPackageInfo(context.packageName, 0)
            "Version ${info.versionName} Build ${androidx.core.content.pm.PackageInfoCompat.getLongVersionCode(info)}"
        }.getOrDefault("")
    }
    Box(Modifier.fillMaxSize().background(AboutVeil).pointerInput(Unit) { awaitEachGesture { awaitFirstDown().consume() } }) {
        Column(Modifier.fillMaxWidth().padding(top = 95.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            // the launcher icon: its field, and its picture drawn at the adaptive icon's full size
            Box(Modifier.size(300.dp).clip(RoundedCornerShape(67.dp)).background(AboutIconField), contentAlignment = Alignment.Center) {
                androidx.compose.foundation.Image(
                    androidx.compose.ui.res.painterResource(io.github.artcraftandroid.procraft.R.drawable.procraft_mark),
                    contentDescription = null,
                    modifier = Modifier.requiredSize(450.dp),
                )
            }
            Text("ProCraft", color = Color.White, fontSize = 64.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.padding(top = 12.dp))
            Text(version, color = AboutInk, fontSize = 13.sp, maxLines = 1, modifier = Modifier.padding(top = 14.dp))
        }
        Box(
            Modifier.align(Alignment.TopCenter).padding(top = AboutButtonTop).size(220.dp, 38.dp).clip(RoundedCornerShape(8.dp))
                .background(AboutButtonFill).clickable(onClick = onRecover),
            contentAlignment = Alignment.Center,
        ) {
            Text("Start Gallery Recovery", color = AboutButtonInk, fontSize = 15.sp, maxLines = 1)
        }
        Box(
            Modifier.align(Alignment.TopEnd).padding(top = 31.dp, end = 31.5.dp).size(22.5.dp).clip(CircleShape).background(AboutClose)
                .clickable(onClick = onClose),
            contentAlignment = Alignment.Center,
        ) {
            androidx.compose.foundation.Canvas(Modifier.size(8.dp)) {
                val w = 1.8.dp.toPx()
                drawLine(AboutCloseInk, androidx.compose.ui.geometry.Offset.Zero, androidx.compose.ui.geometry.Offset(size.width, size.height), w, cap = androidx.compose.ui.graphics.StrokeCap.Round)
                drawLine(AboutCloseInk, androidx.compose.ui.geometry.Offset(size.width, 0f), androidx.compose.ui.geometry.Offset(0f, size.height), w, cap = androidx.compose.ui.graphics.StrokeCap.Round)
            }
        }
    }
}

// as measured: the gallery under a near-black veil, the light round close button
private val AboutVeil = Color(0xD90B0B0B)
private val AboutIconField = Color(0xFF0B0B0C)
private val AboutInk = Color(0xFFBDBDBD)
private val AboutClose = Color(0xFFE3E3E3)
private val AboutCloseInk = Color(0xFF1C1C1C)
// the button 520 under the icon's top, 220 x 38
private val AboutButtonTop = 615.dp
private val AboutButtonFill = Color(0xFF1A1A1A)
private val AboutButtonInk = Color(0xFFC4C4C4)
private const val RECOVERY_SEARCH_MS = 700
private const val RECOVERY_DONE_MS = 900L
