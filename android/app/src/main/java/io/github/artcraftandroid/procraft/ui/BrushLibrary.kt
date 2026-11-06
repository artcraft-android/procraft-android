package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.layout
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionOnScreen
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.RECENT_SET
import io.github.artcraftandroid.procraft.core.BrushInfo
import io.github.artcraftandroid.procraft.core.ToolKind
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

private val LibraryWidth = 440.dp
private val SetsWidth = 168.dp
private val BrushHeight = 80.dp
private val BrushGap = 8.dp
/** The library's menus: dark glass with a light edge, rows 40 high. */
private val LibraryMenuWidth = 253.dp
private val MenuFill = Color(0xFA303335)
private val MenuEdge = Color(0xFF5A5D5F)
private val MenuRule = Color(0xFF3F4347)
/** Menus stay below the top bar. */
private val MenuTop = 57.dp
/** A brush swiped left shows three buttons filling its place, this far apart. */
private val SwipeGap = 5.5.dp
private val SwipeFill = Color(0xFF3F3F3F)
private val SwipeInk = Color(0xFFC5C5C5)
private val SwipeDelete = Color(0xFFFF3A46)
/** A brush swiped right is picked, this fill over its row; it follows the finger this far. */
private val PickedBrush = Color(0xFF2C5281)
private val PickReach = 72.dp

/** A brush's name on its cell: on a dark pill where its sample stroke runs under it. */
@Composable
private fun BrushName(name: String, selected: Boolean, shaded: Boolean, modifier: Modifier) {
    Text(
        name,
        color = if (selected) Color.White else Pc.PanelTitle,
        fontSize = 14.sp,
        fontWeight = FontWeight.SemiBold,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = modifier.offset(x = (-6).dp, y = (-2).dp)
            .then(if (shaded) Modifier.clip(RoundedCornerShape(50)).background(NamePill) else Modifier)
            .padding(horizontal = 6.dp, vertical = 2.dp),
    )
}

private val NamePill = Color(0x80000000)
/** The search field with the space above and below it, which scrolls away with the lists. */
private val SearchBlock = 60.dp
private val SearchFill = Color(0x1F767680)
private val SearchHint = Color(0x4DEBEBF5)
private val ResultFill = Color(0x14FFFFFF)

/**
 * The search field under the library's header: a magnifier, the text and, once typed, a clear
 * button. Putting the keyboard away leaves the field.
 */
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
private fun LibrarySearch(query: String, onQuery: (String) -> Unit, onFocus: (Boolean) -> Unit) {
    val focus = androidx.compose.ui.platform.LocalFocusManager.current
    val keyboard = WindowInsets.isImeVisible
    var hadKeyboard by remember { mutableStateOf(false) }
    LaunchedEffect(keyboard) {
        if (keyboard) hadKeyboard = true else if (hadKeyboard) {
            hadKeyboard = false
            focus.clearFocus()
        }
    }
    Row(
        Modifier.padding(start = 12.dp, end = 12.dp, top = 10.dp, bottom = 10.dp).fillMaxWidth().height(40.dp)
            .clip(RoundedCornerShape(10.dp)).background(SearchFill),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Canvas(Modifier.size(41.dp, 40.dp)) {
            // a magnifier: a ring and its handle
            val c = Offset(size.width * 0.47f, size.height * 0.46f)
            val r = 7.dp.toPx()
            drawCircle(SearchHint, r, c, style = androidx.compose.ui.graphics.drawscope.Stroke(2.dp.toPx()))
            val d = r * 0.7071f
            drawLine(SearchHint, c + Offset(d, d), c + Offset(d + 5.dp.toPx(), d + 5.dp.toPx()), 2.4.dp.toPx(), cap = androidx.compose.ui.graphics.StrokeCap.Round)
        }
        Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
            if (query.isEmpty()) Text("Search", color = SearchHint, fontSize = 17.sp, maxLines = 1)
            androidx.compose.foundation.text.BasicTextField(
                query,
                onValueChange = onQuery,
                singleLine = true,
                textStyle = androidx.compose.ui.text.TextStyle(color = Color.White, fontSize = 17.sp),
                cursorBrush = androidx.compose.ui.graphics.SolidColor(Pc.Accent),
                keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(imeAction = androidx.compose.ui.text.input.ImeAction.Search),
                keyboardActions = androidx.compose.foundation.text.KeyboardActions(onSearch = { focus.clearFocus() }),
                modifier = Modifier.fillMaxWidth().onFocusChanged { onFocus(it.isFocused) },
            )
        }
        if (query.isNotEmpty()) {
            // a filled circle with a cross cut out of it
            Canvas(Modifier.padding(end = 5.dp).size(19.dp, 18.dp).clip(CircleShape).clickable { onQuery("") }) {
                val c = center
                drawCircle(SearchHint, size.minDimension / 2f, c)
                val k = 3.5.dp.toPx()
                for (s in listOf(1f, -1f)) {
                    drawLine(Color.Black, c + Offset(-k, -k * s), c + Offset(k, k * s), 1.8.dp.toPx(), cap = androidx.compose.ui.graphics.StrokeCap.Round, blendMode = androidx.compose.ui.graphics.BlendMode.Clear)
                }
            }
        }
    }
}

/** The brushes of every set whose name holds the search, two to a row; tapping one chooses it. */
@Composable
private fun SearchResults(vm: CanvasViewModel, query: String) {
    val found = if (query.isBlank()) emptyList() else vm.brushSets.flatMap { it.brushes }.filter { it.name.contains(query.trim(), ignoreCase = true) }
    if (found.isEmpty()) {
        Box(Modifier.fillMaxSize().padding(top = 24.dp), contentAlignment = Alignment.TopCenter) {
            Text("No results", color = Pc.PanelTitle, fontSize = 15.sp, fontWeight = FontWeight.Medium)
        }
        return
    }
    LazyVerticalGrid(
        columns = GridCells.Fixed(2),
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 12.dp, top = 11.dp, end = 12.dp, bottom = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        items(found, key = { "${it.set}-${it.index}" }) { b ->
            val key = "${b.set}-${b.index}"
            val preview = vm.previews[key]
            LaunchedEffect(key, preview == null, vm.previewRound) { if (preview == null) vm.loadPreview(b) }
            val selected = vm.highlighted(b)
            Box(
                Modifier.fillMaxWidth().height(81.dp).clip(RoundedCornerShape(12.dp))
                    .background(if (selected) Pc.RowSelected else ResultFill)
                    .clickable { vm.chooseBrush(b) },
            ) {
                preview?.let { Image(it, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize()) }
                BrushName(b.name, selected, vm.shadedNames[key] == true, Modifier.padding(start = 14.dp, top = 11.dp, end = 10.dp))
            }
        }
    }
}

/**
 * The brush library: sets on the left, the set's brushes with a sample stroke on the right. Tap
 * the chosen set for its menu, swipe a brush left to share, duplicate or delete it, hold and drag
 * it onto a set or to another place in its own, and add brushes and sets with the plus.
 */
@Composable
fun BrushLibrary(vm: CanvasViewModel, arrowX: Dp, modifier: Modifier = Modifier) {
    val sets = vm.brushSets
    // the Recent set, or one of the library's
    val recent = vm.shownSet == RECENT_SET
    val shown = sets.getOrNull(vm.shownSet)
    if (shown == null && !recent) return
    // the brushes shown, and whether each is pinned in the Recent set
    val listed = if (recent) {
        vm.recentBrushes.mapNotNull { r -> sets.getOrNull(r.set.toInt())?.brushes?.getOrNull(r.index.toInt())?.let { it to r.pinned } }
    } else {
        shown?.brushes.orEmpty().map { it to false }
    }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val density = LocalDensity.current
    val focus = androidx.compose.ui.platform.LocalFocusManager.current
    var adding by remember { mutableStateOf(false) }
    var setMenu by remember { mutableStateOf<Int?>(null) }
    var recentMenu by remember { mutableStateOf(false) }
    // the set menu's icon grid is open; where its Customize icon row and the set are on screen
    var iconPicker by remember { mutableStateOf(false) }
    var customizeRow by remember { mutableStateOf<Pair<Float, Float>?>(null) }
    var setScreenTop by remember { mutableStateOf(0f) }
    // the search: what is typed, and whether the field is in use
    var query by remember { mutableStateOf("") }
    var searching by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf<Pair<String, () -> Unit>?>(null) }
    androidx.compose.runtime.DisposableEffect(Unit) { onDispose { vm.pickedBrushes.clear() } }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri -> uri?.let { vm.importBrushes(it) } }
    // a brush held and dragged: which, where the finger is and where on the brush it took hold
    // (window pixels); and where the sets, the brushes and the lists are on the screen
    var dragged by remember { mutableStateOf<BrushInfo?>(null) }
    // or a set held and dragged (its place)
    var draggedSet by remember { mutableStateOf<Int?>(null) }
    var dragAt by remember { mutableStateOf(Offset.Zero) }
    var grab by remember { mutableStateOf(Offset.Zero) }
    val setBounds = remember { mutableStateMapOf<Int, Rect>() }
    val brushBounds = remember { mutableStateMapOf<String, Rect>() }
    var listBounds by remember { mutableStateOf(Rect.Zero) }
    // the set the dragged brush or set is over, if any
    val overSet = if (dragged == null && draggedSet == null) null else setBounds.entries.firstOrNull { it.value.contains(dragAt) }?.key?.takeIf { it != draggedSet }
    fun drop() {
        val b = dragged ?: return
        dragged = null
        // onto a set: to its top; onto a brush of this set: to its place
        val set = setBounds.entries.firstOrNull { it.value.contains(dragAt) }?.key
        if (set != null) {
            if (set != b.set.toInt()) vm.moveBrush(b, set, 0)
            return
        }
        val target = shown?.brushes?.firstOrNull { brushBounds["${it.set}-${it.index}"]?.contains(dragAt) == true } ?: return
        vm.moveBrush(b, target.set.toInt(), target.index.toInt())
    }
    // a set dropped onto another takes its place
    fun dropSet() {
        val from = draggedSet ?: return
        draggedSet = null
        setBounds.entries.firstOrNull { it.value.contains(dragAt) }?.key?.let { vm.moveBrushSet(from, it) }
    }
    fun share(b: BrushInfo?) {
        scope.launch { vm.brushFileToShare(b)?.let { shareFile(context, it, "application/octet-stream") } }
    }
    // how far the search field (and the lists under it) has scrolled up out of sight: all of it
    // when the library opens
    val searchBlock = with(density) { SearchBlock.toPx() }
    var searchAway by remember { mutableFloatStateOf(searchBlock) }
    val searchScroll = remember(searchBlock) {
        object : NestedScrollConnection {
            override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
                // scrolling up, the field goes first
                if (available.y >= 0f || searchAway >= searchBlock) return Offset.Zero
                val taken = maxOf(available.y, searchAway - searchBlock)
                searchAway -= taken
                return Offset(0f, taken)
            }

            override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset {
                // pulled down past a list's top (not flung), the field comes back
                if (available.y <= 0f || searchAway <= 0f || source != NestedScrollSource.UserInput) return Offset.Zero
                val taken = minOf(available.y, searchAway)
                searchAway -= taken
                return Offset(0f, taken)
            }
        }
    }

    Popover(LibraryWidth, arrowX, modifier) {
        PanelHeader(
            when (vm.tool) {
                ToolKind.ERASE -> "Erase library"
                ToolKind.SMUDGE -> "Smudge library"
                else -> "Brush library"
            },
            start = 20.dp,
            end = 12.dp,
            height = 52.dp,
        ) {
            // brushes picked: Delete in the plus's place, and Combine for one with the current brush
            if (vm.pickedBrushes.isNotEmpty()) Row(Modifier.padding(top = 16.dp)) {
                if (vm.canCombinePicked) {
                    Box(Modifier.size(80.dp, 32.dp).clickable { vm.combinePicked() }, contentAlignment = Alignment.Center) {
                        Text("Combine", color = SwipeInk, fontSize = 15.sp, maxLines = 1)
                    }
                    Spacer(Modifier.width(4.dp))
                }
                Box(
                    Modifier.size(63.dp, 32.dp).clickable { deleting = "Delete these brushes?" to { vm.deletePickedBrushes() } },
                    contentAlignment = Alignment.Center,
                ) {
                    Text("Delete", color = SwipeInk, fontSize = 15.sp, maxLines = 1)
                }
            } else Box(
                Modifier.padding(top = 12.dp)
                    // ProCraft Pocket's plus sits on a disc
                    .then(if (Pc.pocket) Modifier.size(40.dp).background(Pc.HeaderDisc, CircleShape) else Modifier.size(41.dp, 40.dp))
                    .clickable { adding = true },
                contentAlignment = Alignment.Center,
            ) {
                Icon(PcIcons.PlusMedium, contentDescription = "Add", tint = Pc.PanelGlyph, modifier = Modifier.requiredSize(if (Pc.pocket) 34.dp else 41.dp))
                // hangs below the plus, their right edges in line
                LibraryMenu(adding, androidx.compose.ui.graphics.TransformOrigin(1f, 0f), MenuPlace { a, size -> IntOffset(a.right - size.width, a.bottom) }, onDismiss = { adding = false }, overPanel = true) {
                    // the Recent set holds no brushes of its own to add one to
                    if (!recent) {
                        MenuItem(PcIcons.MenuNewBrush, "Create new brush") {
                            adding = false
                            vm.newBrush()
                        }
                    }
                    MenuItem(PcIcons.MenuNewSet, "Create new set") {
                        adding = false
                        vm.newBrushSet()
                    }
                    MenuDivider(11.dp)
                    MenuItem(PcIcons.MenuFolder, "Import from Files") {
                        adding = false
                        pick.launch(arrayOf("*/*"))
                    }
                }
            }
        }
        // the search field scrolls away with the lists: out of sight at first, it comes down as a
        // list is pulled down past its top, and goes again as the lists scroll up
        Box(Modifier.weight(1f).clipToBounds().nestedScroll(searchScroll)) {
        Column(
            Modifier.layout { measurable, constraints ->
                val away = if (searching || query.isNotEmpty()) 0 else searchAway.roundToInt()
                val tall = constraints.maxHeight + away
                val placeable = measurable.measure(constraints.copy(minHeight = tall, maxHeight = tall))
                layout(constraints.maxWidth, constraints.maxHeight) { placeable.place(0, -away) }
            },
        ) {
        LibrarySearch(query, onQuery = { query = it }, onFocus = { searching = it })
        Box(Modifier.weight(1f).onGloballyPositioned { listBounds = it.boundsInWindow() }) {
        if (searching || query.isNotEmpty()) {
            SearchResults(vm, query)
        } else {
        Row(Modifier.fillMaxSize()) {
            LazyColumn(
                Modifier.width(SetsWidth).fillMaxHeight().background(Pc.SetColumn),
                contentPadding = PaddingValues(12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                // the brushes last painted with, and those pinned there, above the sets
                item {
                    Box {
                        Row(
                            Modifier.fillMaxWidth().height(50.dp).clip(RoundedCornerShape(12.dp))
                                .background(if (recent) Pc.RowSelected else Pc.SetCell)
                                .clickable { if (recent) recentMenu = true else vm.showSet(RECENT_SET) },
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            val ink = if (recent) Color.White else Pc.PanelTitle
                            Icon(PcIcons.RecentSet, contentDescription = null, tint = ink, modifier = Modifier.padding(start = 4.dp).requiredSize(40.dp))
                            Text("Recent", color = ink, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, modifier = Modifier.padding(start = 1.dp, end = 8.dp))
                        }
                        // as a set's menu, of one item and 65 tall, as measured
                        val left = with(density) { 6.dp.roundToPx() }
                        val below = with(density) { MenuTop.roundToPx() }
                        LibraryMenu(recentMenu, androidx.compose.ui.graphics.TransformOrigin(1f, 0.5f), MenuPlace(below) { a, size -> IntOffset(a.left - size.width - left, a.center.y - size.height / 2) }, onDismiss = { recentMenu = false }) {
                            Box(Modifier.padding(vertical = 5.5.dp)) {
                                MenuItem(PcIcons.MenuClear, "Clear") {
                                    recentMenu = false
                                    vm.clearRecentBrushes()
                                }
                            }
                        }
                    }
                }
                itemsIndexed(sets) { i, set ->
                    val selected = i == vm.shownSet
                    // gone from the list, it is nowhere: where it last was may be under another row
                    androidx.compose.runtime.DisposableEffect(i) { onDispose { setBounds.remove(i) } }
                    Box {
                        Row(
                            Modifier.fillMaxWidth().height(50.dp).onGloballyPositioned {
                                setBounds[i] = it.boundsInWindow()
                                if (selected) setScreenTop = it.positionOnScreen().y
                            }
                                // held, it lifts and goes where it is dragged; faint where it was
                                .alpha(if (draggedSet == i) 0.3f else 1f)
                                .pointerInput(i) {
                                    detectDragGesturesAfterLongPress(
                                        onDragStart = { at ->
                                            setMenu = null
                                            focus.clearFocus()
                                            grab = at
                                            dragAt = (setBounds[i]?.topLeft ?: Offset.Zero) + at
                                            draggedSet = i
                                        },
                                        onDrag = { change, by ->
                                            change.consume()
                                            dragAt += by
                                        },
                                        onDragEnd = { dropSet() },
                                        onDragCancel = { draggedSet = null },
                                    )
                                }
                                .clip(RoundedCornerShape(12.dp))
                                .background(if (overSet == i) Pc.Accent else if (selected) Pc.RowSelected else Pc.SetCell)
                                .clickable { if (selected) setMenu = i else vm.showSet(i) },
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            val ink = if (selected) Color.White else Pc.PanelTitle
                            // a 40 frame centred 24 in
                            Icon(PcIcons.SetIcons.getOrElse(set.icon.toInt()) { PcIcons.BrushSet }, contentDescription = null, tint = ink, modifier = Modifier.padding(start = 4.dp).requiredSize(40.dp))
                            val nameModifier = Modifier.padding(start = 1.dp, end = 8.dp)
                            if (vm.renamingSet == i) {
                                InlineName(set.name, ink, 14.sp, nameModifier) { vm.renameBrushSet(i, it) }
                            } else {
                                Text(
                                    set.name,
                                    color = ink,
                                    fontSize = 14.sp,
                                    fontWeight = FontWeight.SemiBold,
                                    lineHeight = 16.sp,
                                    maxLines = 2,
                                    overflow = TextOverflow.Ellipsis,
                                    modifier = nameModifier,
                                )
                            }
                        }
                        // centred on the set, a little left of it
                        val left = with(density) { 6.dp.roundToPx() }
                        val below = with(density) { MenuTop.roundToPx() }
                        // the icons to choose from, right of the menu over the library: under the Customize icon
                        // row, or over it when there is no room below; scrolled to the set's own icon
                        MenuPresence(setMenu == i && iconPicker, androidx.compose.ui.graphics.TransformOrigin(0f, 0f)) { look ->
                            PcPopup(
                                popupPositionProvider = remember(customizeRow, setScreenTop, density) {
                                    MenuPlace(below) { a, size ->
                                        val x = a.left - with(density) { PickerInset.roundToPx() }
                                        val row = customizeRow ?: return@MenuPlace IntOffset(x, a.top)
                                        val top = a.top + (row.first - setScreenTop).roundToInt()
                                        val bottom = a.top + (row.second - setScreenTop).roundToInt()
                                        val room = (vm.windowSize.y - with(density) { PickerMargin.toPx() }).roundToInt()
                                        IntOffset(x, if (bottom + size.height <= room) bottom else top - size.height)
                                    }
                                },
                                onDismissRequest = { iconPicker = false },
                            ) {
                                val shape = RoundedCornerShape(26.dp)
                                // an icon that isn't one of the picker's starts it at the top
                                val own = set.icon.toInt().takeIf { it in 0 until PcIcons.PICKED_SET_ICONS }
                                val first = if (own == null) 0 else (own / PICKER_COLUMNS - 3).coerceAtLeast(0) * PICKER_COLUMNS
                                LazyVerticalGrid(
                                    columns = GridCells.Fixed(PICKER_COLUMNS),
                                    state = androidx.compose.foundation.lazy.grid.rememberLazyGridState(initialFirstVisibleItemIndex = first),
                                    modifier = look.size(250.dp, 325.dp).clip(shape).background(MenuFill).border(1.dp, MenuEdge, shape).padding(3.dp),
                                ) {
                                    items(PcIcons.PICKED_SET_ICONS) { k ->
                                        val chosen = k == set.icon.toInt()
                                        Box(
                                            Modifier.size(48.dp).clip(RoundedCornerShape(21.dp)).background(if (chosen) Pc.Accent else Color.Transparent).clickable {
                                                vm.changeBrushSetIcon(i, k)
                                                iconPicker = false
                                                setMenu = null
                                            },
                                            contentAlignment = Alignment.Center,
                                        ) {
                                            Icon(PcIcons.SetIcons[k], contentDescription = PcIcons.SetIconNames[k], tint = Color.White, modifier = Modifier.requiredSize(40.dp))
                                        }
                                    }
                                }
                            }
                        }
                        // left of the set, 6 clear of it, centred on it
                        LibraryMenu(setMenu == i, androidx.compose.ui.graphics.TransformOrigin(1f, 0.5f), MenuPlace(below) { a, size -> IntOffset(a.left - size.width - left, a.center.y - size.height / 2) }, onDismiss = { setMenu = null }) {
                            MenuItem(PcIcons.MenuRename, "Rename") {
                                setMenu = null
                                vm.renamingSet = i
                            }
                            MenuItem(PcIcons.MenuDuplicate, "Duplicate") {
                                setMenu = null
                                vm.duplicateBrushSet(i)
                            }
                            Box(Modifier.onGloballyPositioned { c -> c.positionOnScreen().y.let { customizeRow = it to it + c.size.height } }) {
                                MenuItem(PcIcons.SetIcons.getOrElse(set.icon.toInt()) { PcIcons.BrushSet }, "Customize icon", trailing = PcIcons.MenuChevron) {
                                    iconPicker = !iconPicker
                                }
                            }
                            MenuDivider()
                            MenuItem(PcIcons.MenuShare, "Share") {
                                setMenu = null
                                share(null)
                            }
                            if (sets.size > 1) {
                                MenuDivider()
                                MenuItem(PcIcons.MenuTrash, "Delete", Pc.MenuDanger) {
                                    setMenu = null
                                    deleting = "Delete this set and its brushes?" to { vm.deleteBrushSet(i) }
                                }
                            }
                        }
                    }
                }
            }
            // another set's brushes replace these at once (rows fade only as brushes come and go)
            androidx.compose.runtime.key(vm.shownSet) {
                // a brush found from the Recent set is scrolled to in its own
                val listState = rememberLazyListState()
                LaunchedEffect(vm.foundBrush) {
                    val found = vm.foundBrush ?: return@LaunchedEffect
                    vm.foundBrush = null
                    listed.indexOfFirst { (b, _) -> "${b.set}-${b.index}" == found }.takeIf { it >= 0 }?.let { listState.scrollToItem(it) }
                }
                LazyColumn(
                    Modifier.weight(1f).fillMaxHeight().background(Pc.BrushColumn),
                    state = listState,
                    contentPadding = PaddingValues(start = 12.dp, top = 9.75.dp, end = 12.dp, bottom = 12.dp),
                    verticalArrangement = Arrangement.spacedBy(BrushGap),
                ) {
                    items(listed, key = { (b, _) -> "${b.set}-${b.index}" }) { (b, pinned) ->
                        val key = "${b.set}-${b.index}"
                        // the current brush, or the brush last chosen in this set (in the Recent
                        // set the current brush alone)
                        val selected = if (recent) vm.currentBrush?.let { it.set == b.set && it.index == b.index } == true else vm.highlighted(b)
                        val picked = key in vm.pickedBrushes
                        // previews go by place, and are dropped when the library changes
                        val preview = vm.previews[key]
                        LaunchedEffect(key, preview == null, vm.previewRound) { if (preview == null) vm.loadPreview(b) }
                        // gone from the list, it is nowhere: where it last was may be under another row
                        androidx.compose.runtime.DisposableEffect(key) { onDispose { brushBounds.remove(key) } }
                        Box(Modifier.animateItem(Motion.rowFade(), Motion.rowMove(), Motion.rowFade())) {
                            SwipeBrush(
                                open = vm.swipedBrush == key,
                                onOpen = { vm.swipedBrush = if (it) key else null },
                                // the current brush counts as picked already
                                onPick = if (recent || selected) null else ({ vm.togglePickedBrush(b) }),
                                actions = if (recent) {
                                    listOf(
                                        SwipeAction(PcIcons.SwipeFind, "Find") { vm.findBrush(b) },
                                        if (pinned) {
                                            SwipeAction(PcIcons.SwipeUnpin, "Unpin") { vm.pinRecentBrush(b, false) }
                                        } else {
                                            SwipeAction(PcIcons.SwipePin, "Pin") { vm.pinRecentBrush(b, true) }
                                        },
                                        SwipeAction(PcIcons.SwipeClear, "Clear") { vm.clearRecentBrush(b) },
                                    )
                                } else {
                                    listOf(
                                        SwipeAction(PcIcons.SwipeShare, "Share") { share(b) },
                                        SwipeAction(PcIcons.SwipeDuplicate, "Duplicate") { vm.duplicateBrush(b) },
                                        SwipeAction(PcIcons.SwipeTrash, "Delete", SwipeDelete, Color.White) { deleting = "Delete this brush?" to { vm.deleteBrush(b) } },
                                    )
                                },
                            ) {
                                Box(
                                    Modifier.fillMaxWidth().height(BrushHeight).onGloballyPositioned { brushBounds[key] = it.boundsInWindow() }
                                        .alpha(if (dragged?.let { d -> d.set == b.set && d.index == b.index } == true) 0.3f else 1f)
                                        .pointerInput(key, recent) {
                                            // the Recent set's brushes stay where they are
                                            if (recent) return@pointerInput
                                            detectDragGesturesAfterLongPress(
                                                onDragStart = { at ->
                                                    vm.swipedBrush = null
                                                    val top = brushBounds[key]?.topLeft ?: Offset.Zero
                                                    grab = at
                                                    dragAt = top + at
                                                    dragged = b
                                                },
                                                onDrag = { change, by ->
                                                    change.consume()
                                                    dragAt += by
                                                },
                                                onDragEnd = { drop() },
                                                onDragCancel = { dragged = null },
                                            )
                                        }
                                        .clip(RoundedCornerShape(12.dp))
                                        .background(if (selected) Pc.RowSelected else if (picked) PickedBrush else Color.Transparent)
                                        .clickable {
                                            // with a brush swiped open, a tap only closes it
                                            if (vm.swipedBrush != null) {
                                                vm.swipedBrush = null
                                                return@clickable
                                            }
                                            // and with brushes picked, only lets them go
                                            if (vm.pickedBrushes.isNotEmpty()) {
                                                vm.pickedBrushes.clear()
                                                return@clickable
                                            }
                                            // a highlighted brush opens in the studio
                                            vm.chooseBrush(b)
                                            if (selected) vm.openStudio()
                                        },
                                ) {
                                    // the sample stroke filling the cell's height (its ends past the cell's sides),
                                    // under the name
                                    preview?.let {
                                        // the samples are drawn white: dark on the light interface's cells
                                        Image(
                                            it, contentDescription = null, contentScale = ContentScale.Crop,
                                            colorFilter = if (Pc.light && !selected) androidx.compose.ui.graphics.ColorFilter.tint(Color.Black, androidx.compose.ui.graphics.BlendMode.SrcIn) else null,
                                            modifier = Modifier.fillMaxSize(),
                                        )
                                    }
                                    BrushName(b.name, selected || picked, vm.shadedNames[key] == true, Modifier.padding(start = 18.dp, top = 11.75.dp))
                                    // a pinned brush's mark, at the cell's top right
                                    if (pinned) {
                                        Icon(
                                            PcIcons.PinMark, contentDescription = "Pinned", tint = if (selected) Color.White else Pc.PanelTitle,
                                            modifier = Modifier.align(Alignment.TopEnd).padding(top = 12.5.dp, end = 11.dp).requiredSize(16.dp),
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        }
            // the dragged brush follows the finger
            dragged?.let { d ->
                val size = brushBounds["${d.set}-${d.index}"]?.size
                val at = dragAt - grab - listBounds.topLeft
                Box(
                    Modifier.offset { IntOffset(at.x.roundToInt(), at.y.roundToInt()) }
                        .size(with(density) { (size?.width ?: 0f).toDp() }, BrushHeight)
                        .clip(RoundedCornerShape(12.dp)).background(Pc.RowSelected.copy(alpha = 0.9f)),
                ) {
                    vm.previews["${d.set}-${d.index}"]?.let {
                        Image(it, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
                    }
                    Text(d.name, color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, modifier = Modifier.padding(start = 18.dp, top = 11.dp))
                }
            }
            // and the dragged set
            draggedSet?.let { i ->
                val set = sets.getOrNull(i) ?: return@let
                val at = dragAt - grab - listBounds.topLeft
                Row(
                    Modifier.offset { IntOffset(at.x.roundToInt(), at.y.roundToInt()) }
                        .size(with(density) { (setBounds[i]?.width ?: 0f).toDp() }, 50.dp)
                        .clip(RoundedCornerShape(12.dp)).background(Pc.RowSelected.copy(alpha = 0.9f)),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(PcIcons.SetIcons.getOrElse(set.icon.toInt()) { PcIcons.BrushSet }, contentDescription = null, tint = Color.White, modifier = Modifier.padding(start = 4.dp).requiredSize(40.dp))
                    Text(
                        set.name,
                        color = Color.White,
                        fontSize = 14.sp,
                        fontWeight = FontWeight.SemiBold,
                        lineHeight = 16.sp,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.padding(start = 1.dp, end = 8.dp),
                    )
                }
            }
        }
        }
        }
    }

    // while the question is up, brushes read from a file wait to join the library
    LaunchedEffect(deleting != null) { vm.brushQuestionOpen = deleting != null }
    Alert(
        deleting,
        title = { (title, _) -> title },
        message = { "It can't be brought back." },
        action = "Delete",
        icon = PcIcons.Trash,
        onCancel = { deleting = null },
        onAction = { (_, action) ->
            deleting = null
            action()
        },
    )
}

/** A button a swiped brush shows: its icon and label, its colours, and what it does. */
private class SwipeAction(val icon: ImageVector, val label: String, val fill: Color = SwipeFill, val ink: Color = SwipeInk, val onClick: () -> Unit)

/**
 * A brush that slides away to the left, showing its [actions] in its place: Share, Duplicate and
 * Delete; in the Recent set Find, Pin (or Unpin) and Clear. Slid right past half of [PickReach] it
 * springs back and is picked or let go ([onPick]).
 */
@Composable
private fun SwipeBrush(
    open: Boolean,
    onOpen: (Boolean) -> Unit,
    actions: List<SwipeAction>,
    onPick: (() -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    var width by remember { mutableIntStateOf(0) }
    val reach = if (onPick == null) 0f else with(LocalDensity.current) { PickReach.toPx() }
    val offset = remember { Animatable(0f) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(open, width) { offset.animateTo(if (open) -width.toFloat() else 0f, Motion.swipeSettle()) }
    Box(Modifier.fillMaxWidth().onSizeChanged { width = it.width }.clipToBounds()) {
        if (offset.value < 0f) {
            Row(Modifier.matchParentSize(), horizontalArrangement = Arrangement.spacedBy(SwipeGap)) {
                for (a in actions) {
                    SwipeButton(a.icon, a.label, a.fill, a.ink, Modifier.weight(1f)) {
                        onOpen(false)
                        a.onClick()
                    }
                }
            }
        }
        Box(
            Modifier.offset { IntOffset(offset.value.roundToInt(), 0) }.pointerInput(open, width, reach) {
                detectHorizontalDragGestures(
                    onHorizontalDrag = { change, amount ->
                        change.consume()
                        scope.launch { offset.snapTo((offset.value + amount).coerceIn(-width.toFloat(), if (open) 0f else reach)) }
                    },
                    onDragEnd = {
                        if (offset.value > reach / 2f && reach > 0f) onPick?.invoke()
                        val opened = offset.value < -width / 3f
                        if (opened == open) scope.launch { offset.animateTo(if (open) -width.toFloat() else 0f, Motion.swipeSettle()) } else onOpen(opened)
                    },
                    onDragCancel = { scope.launch { offset.animateTo(if (open) -width.toFloat() else 0f, Motion.swipeSettle()) } },
                )
            },
        ) { content() }
    }
}

@Composable
private fun SwipeButton(icon: ImageVector, label: String, fill: Color, ink: Color, modifier: Modifier, onClick: () -> Unit) {
    Column(
        modifier.fillMaxHeight().clip(RoundedCornerShape(16.dp)).background(fill).clickable(onClick = onClick).padding(top = 5.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        // 19 across, as measured, the label where it was
        Icon(icon, contentDescription = null, tint = ink, modifier = Modifier.size(19.dp))
        Spacer(Modifier.height(2.dp))
        Text(label, color = ink, fontSize = 14.sp, fontWeight = FontWeight.Medium, maxLines = 1)
    }
}

/** A menu of the library (and the layers panel's title), placed by [place]. */
@Composable
internal fun LibraryMenu(
    open: Boolean,
    origin: androidx.compose.ui.graphics.TransformOrigin,
    place: PopupPositionProvider,
    onDismiss: () -> Unit,
    overPanel: Boolean = false,
    content: @Composable ColumnScope.() -> Unit,
) {
    MenuPresence(open, origin) { look ->
        PcPopup(textScale = MATCHED_POPUP_TEXT, popupPositionProvider = place, onDismissRequest = onDismiss, properties = PopupProperties(focusable = true)) {
            val shape = RoundedCornerShape(16.dp)
            // frosted over the canvas; over the library itself, nearly solid
            val glass = if (overPanel) Modifier.background(Pc.GlassMenuOverPanel) else rememberFrost(16.dp).background(Pc.GlassMenu)
            Column(look.width(LibraryMenuWidth).clip(shape).then(glass).border(1.dp, MenuEdge, shape).padding(vertical = 7.dp), content = content)
        }
    }
}

/** A menu row: its icon (a 40 frame) centred 29.5 in, the label from 52, a chevron 27.5 from the right. */
@Composable
internal fun MenuItem(icon: ImageVector, label: String, ink: Color = Pc.MenuInk, trailing: ImageVector? = null, iconSize: Dp = 40.dp, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().height(40.dp).clickable(onClick = onClick), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.padding(start = 9.5.dp).size(40.dp), contentAlignment = Alignment.Center) {
            Icon(icon, contentDescription = null, tint = ink, modifier = Modifier.requiredSize(iconSize))
        }
        // the menus' 17 pt system text is as wide as 18 here
        Text(label, color = ink, fontSize = 18.sp, maxLines = 1, modifier = Modifier.padding(start = 2.5.dp).weight(1f))
        if (trailing != null) Icon(trailing, contentDescription = null, tint = ink, modifier = Modifier.padding(end = 7.5.dp).requiredSize(40.dp))
    }
}

@Composable
internal fun MenuDivider(gap: Dp = 10.dp) {
    Box(Modifier.padding(horizontal = 15.5.dp, vertical = gap).fillMaxWidth().height(1.dp).background(MenuRule))
}

/** Places a menu from its anchor's bounds and its own size, kept in the window below [top]. */
/** The set icon picker: its columns, how far left of the set it starts, and the room it keeps below. */
private const val PICKER_COLUMNS = 5
private val PickerInset = 11.dp
private val PickerMargin = 25.dp

internal class MenuPlace(private val top: Int = 0, private val place: (IntRect, IntSize) -> IntOffset) : PopupPositionProvider {
    override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
        val p = place(anchorBounds, popupContentSize)
        return IntOffset(
            p.x.coerceIn(0, (windowSize.width - popupContentSize.width).coerceAtLeast(0)),
            p.y.coerceIn(top, (windowSize.height - popupContentSize.height).coerceAtLeast(top)),
        )
    }
}
