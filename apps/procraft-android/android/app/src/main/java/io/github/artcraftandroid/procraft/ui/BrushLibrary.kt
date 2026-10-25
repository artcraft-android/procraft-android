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
private val MenuRed = Color(0xFFFF453A)
/** Menus stay below the top bar. */
private val MenuTop = 57.dp
/** A brush swiped left shows three buttons filling its place, this far apart. */
private val SwipeGap = 5.5.dp
private val SwipeFill = Color(0xFF3F3F3F)
private val SwipeInk = Color(0xFFC5C5C5)
private val SwipeDelete = Color(0xFFFF3A46)

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
            LaunchedEffect(key, preview == null) { if (preview == null) vm.loadPreview(b) }
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
    val shown = sets.getOrNull(vm.shownSet) ?: return
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val density = LocalDensity.current
    var adding by remember { mutableStateOf(false) }
    var setMenu by remember { mutableStateOf<Int?>(null) }
    // the set menu's icon grid is open
    var iconPicker by remember { mutableStateOf(false) }
    // the search: what is typed, and whether the field is in use
    var query by remember { mutableStateOf("") }
    var searching by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf<Pair<String, () -> Unit>?>(null) }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri -> uri?.let { vm.importBrushes(it) } }
    // a brush held and dragged: which, where the finger is and where on the brush it took hold
    // (window pixels); and where the sets, the brushes and the lists are on the screen
    var dragged by remember { mutableStateOf<BrushInfo?>(null) }
    var dragAt by remember { mutableStateOf(Offset.Zero) }
    var grab by remember { mutableStateOf(Offset.Zero) }
    val setBounds = remember { mutableStateMapOf<Int, Rect>() }
    val brushBounds = remember { mutableStateMapOf<String, Rect>() }
    var listBounds by remember { mutableStateOf(Rect.Zero) }
    // the set the dragged brush is over, if any
    val overSet = dragged?.let { setBounds.entries.firstOrNull { it.value.contains(dragAt) }?.key }
    fun drop() {
        val b = dragged ?: return
        dragged = null
        // onto a set: to its top; onto a brush of this set: to its place
        val set = setBounds.entries.firstOrNull { it.value.contains(dragAt) }?.key
        if (set != null) {
            if (set != b.set.toInt()) vm.moveBrush(b, set, 0)
            return
        }
        val target = shown.brushes.firstOrNull { brushBounds["${it.set}-${it.index}"]?.contains(dragAt) == true } ?: return
        vm.moveBrush(b, target.set.toInt(), target.index.toInt())
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
            Box(Modifier.padding(top = 12.dp).size(41.dp, 40.dp).clickable { adding = true }, contentAlignment = Alignment.Center) {
                Icon(PcIcons.PlusMedium, contentDescription = "Add", tint = Pc.PanelGlyph, modifier = Modifier.requiredSize(41.dp))
                // hangs below the plus, their right edges in line
                LibraryMenu(adding, androidx.compose.ui.graphics.TransformOrigin(1f, 0f), MenuPlace { a, size -> IntOffset(a.right - size.width, a.bottom) }, onDismiss = { adding = false }, overPanel = true) {
                    MenuItem(PcIcons.MenuNewBrush, "Create new brush") {
                        adding = false
                        vm.newBrush()
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
                itemsIndexed(sets) { i, set ->
                    val selected = i == vm.shownSet
                    Box {
                        Row(
                            Modifier.fillMaxWidth().height(50.dp).onGloballyPositioned { setBounds[i] = it.boundsInWindow() }.clip(RoundedCornerShape(12.dp))
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
                        // the icons to choose from, beside the menu on its left
                        MenuPresence(setMenu == i && iconPicker, androidx.compose.ui.graphics.TransformOrigin(1f, 0.5f)) { look ->
                            PcPopup(
                                textScale = MATCHED_POPUP_TEXT,
                                popupPositionProvider = MenuPlace(below) { a, size -> IntOffset(a.left - left - size.width + with(density) { 6.dp.roundToPx() }, a.center.y - size.height / 2 + with(density) { 40.dp.roundToPx() }) },
                                onDismissRequest = { iconPicker = false },
                            ) {
                                val shape = RoundedCornerShape(16.dp)
                                LazyVerticalGrid(
                                    columns = GridCells.Fixed(5),
                                    modifier = look.size(252.dp, 327.dp).clip(shape).background(MenuFill).border(1.dp, MenuEdge, shape).padding(4.dp),
                                ) {
                                    items(PcIcons.PICKED_SET_ICONS) { k ->
                                        val chosen = k == set.icon.toInt()
                                        Box(
                                            Modifier.size(48.dp).clip(CircleShape).background(if (chosen) Pc.Accent else Color.Transparent).clickable {
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
                            MenuItem(PcIcons.SetIcons.getOrElse(set.icon.toInt()) { PcIcons.BrushSet }, "Customize icon", trailing = PcIcons.MenuChevron) {
                                iconPicker = !iconPicker
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
                LazyColumn(
                    Modifier.weight(1f).fillMaxHeight().background(Pc.BrushColumn),
                    contentPadding = PaddingValues(start = 12.dp, top = 9.75.dp, end = 12.dp, bottom = 12.dp),
                    verticalArrangement = Arrangement.spacedBy(BrushGap),
                ) {
                    items(shown.brushes, key = { "${it.set}-${it.index}" }) { b ->
                        val key = "${b.set}-${b.index}"
                        // the current brush, or the brush last chosen in this set
                        val selected = vm.highlighted(b)
                        // previews go by place, and are dropped when the library changes
                        val preview = vm.previews[key]
                        LaunchedEffect(key, preview == null) { if (preview == null) vm.loadPreview(b) }
                        Box(Modifier.animateItem(Motion.rowFade(), Motion.rowMove(), Motion.rowFade())) {
                            SwipeBrush(
                                open = vm.swipedBrush == key,
                                onOpen = { vm.swipedBrush = if (it) key else null },
                                onShare = { share(b) },
                                onDuplicate = { vm.duplicateBrush(b) },
                                onDelete = { deleting = "Delete this brush?" to { vm.deleteBrush(b) } },
                            ) {
                                Box(
                                    Modifier.fillMaxWidth().height(BrushHeight).onGloballyPositioned { brushBounds[key] = it.boundsInWindow() }
                                        .alpha(if (dragged?.let { d -> d.set == b.set && d.index == b.index } == true) 0.3f else 1f)
                                        .pointerInput(key) {
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
                                        .background(if (selected) Pc.RowSelected else Color.Transparent)
                                        .clickable {
                                            // with a brush swiped open, a tap only closes it
                                            if (vm.swipedBrush != null) {
                                                vm.swipedBrush = null
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
                                    BrushName(b.name, selected, vm.shadedNames[key] == true, Modifier.padding(start = 18.dp, top = 11.75.dp))
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
        }
        }
        }
    }

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

/** A brush that slides away to the left, showing Share, Duplicate and Delete in its place. */
@Composable
private fun SwipeBrush(
    open: Boolean,
    onOpen: (Boolean) -> Unit,
    onShare: () -> Unit,
    onDuplicate: () -> Unit,
    onDelete: () -> Unit,
    content: @Composable () -> Unit,
) {
    var width by remember { mutableIntStateOf(0) }
    val offset = remember { Animatable(0f) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(open, width) { offset.animateTo(if (open) -width.toFloat() else 0f, Motion.swipeSettle()) }
    Box(Modifier.fillMaxWidth().onSizeChanged { width = it.width }.clipToBounds()) {
        if (offset.value < 0f) {
            Row(Modifier.matchParentSize(), horizontalArrangement = Arrangement.spacedBy(SwipeGap)) {
                SwipeButton(PcIcons.SwipeShare, "Share", SwipeFill, SwipeInk, Modifier.weight(1f)) {
                    onOpen(false)
                    onShare()
                }
                SwipeButton(PcIcons.SwipeDuplicate, "Duplicate", SwipeFill, SwipeInk, Modifier.weight(1f)) {
                    onOpen(false)
                    onDuplicate()
                }
                SwipeButton(PcIcons.SwipeTrash, "Delete", SwipeDelete, Color.White, Modifier.weight(1f)) {
                    onOpen(false)
                    onDelete()
                }
            }
        }
        Box(
            Modifier.offset { IntOffset(offset.value.roundToInt(), 0) }.pointerInput(open, width) {
                detectHorizontalDragGestures(
                    onHorizontalDrag = { change, amount ->
                        change.consume()
                        scope.launch { offset.snapTo((offset.value + amount).coerceIn(-width.toFloat(), 0f)) }
                    },
                    onDragEnd = {
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
        // 19 across, as the reference's are, the label where it was
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
internal fun MenuItem(icon: ImageVector, label: String, ink: Color = Color.White, trailing: ImageVector? = null, iconSize: Dp = 40.dp, onClick: () -> Unit) {
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
internal class MenuPlace(private val top: Int = 0, private val place: (IntRect, IntSize) -> IntOffset) : PopupPositionProvider {
    override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
        val p = place(anchorBounds, popupContentSize)
        return IntOffset(
            p.x.coerceIn(0, (windowSize.width - popupContentSize.width).coerceAtLeast(0)),
            p.y.coerceIn(top, (windowSize.height - popupContentSize.height).coerceAtLeast(top)),
        )
    }
}
