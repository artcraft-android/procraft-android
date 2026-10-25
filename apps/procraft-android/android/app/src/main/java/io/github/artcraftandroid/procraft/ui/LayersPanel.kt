package io.github.artcraftandroid.procraft.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.snapping.SnapPosition
import androidx.compose.foundation.gestures.snapping.rememberSnapFlingBehavior
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
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
import androidx.compose.ui.zIndex
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.LayerInfo
import io.github.artcraftandroid.procraft.core.LayerType
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.roundToInt

private val LayersWidth = 384.dp
private val RowHeight = 69.dp
/** A layer with a mask, and the mask, are a little shorter. */
private val MaskedRowHeight = 65.dp
private val RowGap = 1.dp
private val ListCorner = 8.dp
/** A group's row height, and how far in each level of group sits. */
private val GroupHeight = 44.dp
private val GroupIndent = 21.dp
private val GroupLine = Color(0xFF4E4E4E)
private val MaskOwnerRow = Color(0xFF2C5281)
/** Swiping a row left shows these buttons, each this wide, this far apart. */
private val SwipeButton = 82.dp
private val SwipeGap = 2.dp
private val SwipeFill = Color(0xFF3F3F3F)
private val SwipeInk = Color(0xFFC1C1C1)
private val SwipeDelete = Color(0xFFFF473D)
/** How far right a row slides to be picked. */
private val PickReach = 72.dp
/** A picked row (the current one stays blue). */
private val PickedRow = Color(0xFF2C5281)
private val LockedLetter = Color(0xFF808080)
/** The options never go higher than this from the window's top (the top bar and a gap). */
private val OptionsTop = 57.dp

/**
 * The layers panel: top layer first. Tap a layer to make it current, tap the current one for its
 * options, tap the blend letter for blend mode and opacity, hold and drag to reorder, swipe left
 * for lock, duplicate and delete, swipe right to pick it to act on with the current one (the
 * title's menu then groups, merges, shares, duplicates or deletes them).
 */
@Composable
fun LayersPanel(vm: CanvasViewModel, arrowX: Dp, modifier: Modifier = Modifier) {
    var options by remember { mutableStateOf<ULong?>(null) }
    var blendFor by remember { mutableStateOf<ULong?>(null) }
    // blend modes opening under a layer: the rows under it go at once and the panel grows over where
    // they were; closing, it shrinks back to them and they come back a moment later. [blendAt] is
    // the row they open under (-1 when nothing is moving), [belowShown] the rows under it.
    var blendAt by remember { mutableIntStateOf(-1) }
    var belowShown by remember { mutableFloatStateOf(1f) }
    val blendScope = rememberCoroutineScope()
    var blendSettled by remember { mutableStateOf<kotlinx.coroutines.Job?>(null) }
    fun changeBlend(id: ULong?) {
        if (id == blendFor) return
        val shown = vm.layers.filter { !vm.hidden(it) }
        val at = listOfNotNull(blendFor, id).map { b -> shown.indexOfFirst { it.id == b } }.filter { it >= 0 }.minOrNull() ?: -1
        val opening = id != null
        blendFor = id
        blendAt = at
        belowShown = 0f
        blendSettled?.cancel()
        blendSettled = blendScope.launch {
            kotlinx.coroutines.delay(if (opening) Motion.BLEND_OPEN_MS else Motion.BLEND_CLOSE_MS + Motion.BLEND_ROWS_WAIT_MS)
            androidx.compose.animation.core.animate(0f, 1f, animationSpec = Motion.blendRowsBack()) { v, _ -> belowShown = v }
            blendAt = -1
        }
    }
    var renaming by remember { mutableStateOf<ULong?>(null) }
    var swiped by remember { mutableStateOf<ULong?>(null) }
    var titleMenu by remember { mutableStateOf(false) }
    var deletingChosen by remember { mutableStateOf(false) }
    val context = androidx.compose.ui.platform.LocalContext.current
    val scope = rememberCoroutineScope()
    var dragId by remember { mutableStateOf<ULong?>(null) }
    var dragOffset by remember { mutableFloatStateOf(0f) }
    // where each row is in the window: for dropping a dragged row and placing the options
    val rows = remember { mutableMapOf<ULong, Rect>() }

    // a model's meshes are pictured as the model is seen when the panel opens
    if (vm.modelOpen) LaunchedEffect(Unit) { vm.refreshSubmeshThumbnails() }
    Popover(LayersWidth, arrowX, modifier) {
        // kept while they fade out
        val open = options?.let { id -> vm.layers.firstOrNull { it.id == id }?.let { l -> rows[id]?.let { l to it } } }
        val last = remember { arrayOfNulls<Pair<LayerInfo, Rect>>(1) }
        if (open != null) last[0] = open
        last[0]?.let { (l, row) -> LayerOptions(open != null, vm, l, row.center.y, onRename = { renaming = l.id }, onDone = { options = null }) }
        PanelHeader(
            "Layers",
            chevronOn = vm.anyPicked,
            onTitle = { titleMenu = !titleMenu },
            menu = {
                LayersTitleMenu(titleMenu, vm, onDismiss = { titleMenu = false }, onShare = {
                    scope.launch {
                        val files = vm.chosenLayerFiles()
                        if (files.isNotEmpty()) shareFiles(context, files, "image/png")
                    }
                }, onDelete = { deletingChosen = true })
            },
        ) {
            Box(Modifier.padding(top = 12.dp).size(40.dp, 41.dp).clickable { vm.addLayer() }, contentAlignment = Alignment.Center) {
                Icon(PcIcons.PlusMedium, contentDescription = "New layer", tint = Pc.PanelGlyph, modifier = Modifier.requiredSize(41.dp))
            }
        }
        // a layer added at the top of what shows comes in there, the rows under it sliding down,
        // rather than the list holding its place and leaving the new row out of sight above
        val list = rememberLazyListState()
        val ids = vm.layers.filter { !vm.hidden(it) }.map { it.id }
        val lastIds = remember { arrayOfNulls<List<ULong>>(1) }
        val before = lastIds[0]
        if (before != null && ids != before) {
            val added = ids.indices.firstOrNull { ids[it] !in before }
            if (added != null && added <= list.firstVisibleItemIndex) list.requestScrollToItem(added)
        }
        lastIds[0] = ids
        LazyColumn(
            Modifier.weight(1f, fill = false).padding(horizontal = 12.dp),
            state = list,
            verticalArrangement = Arrangement.spacedBy(RowGap),
            contentPadding = PaddingValues(bottom = 12.dp),
        ) {
            // a closed group's layers are left out
            val shown = vm.layers.filter { !vm.hidden(it) }
            // a layer's row: in the list, or on a model under its submesh, indented
            val layerItem: @Composable LazyItemScope.(Int, LayerInfo, Shape, Dp, String?) -> Unit = { index, l, shape, indent, subtitle ->
                val dragging = dragId == l.id
                DisposableEffect(l.id) { onDispose { rows.remove(l.id) } }
                Column(
                    Modifier.animateItem(Motion.rowFade(), if (dragging || blendAt >= 0) null else Motion.rowMove(), Motion.rowFade())
                        .onGloballyPositioned { rows[l.id] = it.boundsInWindow() }
                        .zIndex(if (dragging) 1f else 0f).graphicsLayer {
                            translationY = if (dragging) dragOffset else 0f
                            alpha = if (blendAt in 0 until index) belowShown else 1f
                        }
                        .pointerInput(l.id) {
                            if (l.kind == LayerType.MASK) return@pointerInput
                            detectDragGesturesAfterLongPress(
                                onDragStart = {
                                    dragId = l.id
                                    dragOffset = 0f
                                    swiped = null
                                },
                                onDrag = { change, amount ->
                                    change.consume()
                                    dragOffset += amount.y
                                },
                                onDragEnd = {
                                    rows[l.id]?.let { drop(vm, l, it.center.y + dragOffset, rows) }
                                    dragId = null
                                    dragOffset = 0f
                                },
                                onDragCancel = {
                                    dragId = null
                                    dragOffset = 0f
                                },
                            )
                        },
                ) {
                    val onTap = {
                        swiped = null
                        // on a model the row means the layer itself: its colour, no channel's tile
                        if (vm.modelOpen && vm.channelTileChosen && l.kind != LayerType.GROUP) {
                            vm.leaveChannelTiles()
                            if (!l.current) vm.selectLayer(l.id)
                        } else if (vm.anyPicked) {
                            // picks give way to the layer tapped, the current one too
                            vm.selectLayer(l.id)
                            options = null
                        } else if (l.current) {
                            options = if (options == l.id) null else l.id
                            changeBlend(null)
                        } else {
                            vm.selectLayer(l.id)
                            options = null
                        }
                    }
                    // renamed in place
                    val name: @Composable (Color, Modifier) -> Unit = { ink, m ->
                        if (renaming == l.id) {
                            InlineName(l.name, ink, 17.sp, m) {
                                vm.rename(l, it)
                                renaming = null
                            }
                        } else {
                            Text(l.name, color = ink, fontSize = 17.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = m)
                        }
                    }
                    SwipeRow(vm, l, open = swiped == l.id, enabled = dragId == null, onOpen = { swiped = if (it) l.id else null }, onPick = { vm.pickLayer(l) }) {
                        if (l.kind == LayerType.GROUP) {
                            GroupRow(vm, l, shape, name, onTap, indent) { vm.toggleCollapsed(l) }
                        } else LayerRow(
                            vm,
                            l,
                            shape,
                            name,
                            owner = vm.layers.any { it.current && it.kind == LayerType.MASK && it.parent == l.id },
                            onTap = onTap,
                            onBlend = {
                                changeBlend(if (blendFor == l.id) null else l.id)
                                options = null
                            },
                            indent = indent,
                            subtitle = subtitle,
                            // on a model: its channels, and the row giving its blue to the channel painted
                            onCube = if (vm.modelOpen && l.kind != LayerType.MASK) ({ vm.toggleChannelTray(l.id) }) else null,
                            cubeLit = vm.modelOpen && l.current && vm.channelTileChosen,
                            selected = l.current && !(vm.modelOpen && vm.channelTileChosen),
                        )
                    }
                    if (vm.modelOpen && vm.channelTrayFor == l.id) ChannelTray(vm, l)
                    // the blend modes open under the row, the panel growing as they fade in, and close back
                    androidx.compose.animation.AnimatedVisibility(
                        blendFor == l.id,
                        // held at the top, uncovered as the panel grows and covered as it shrinks
                        enter = androidx.compose.animation.expandVertically(Motion.blendOpenSize(), expandFrom = Alignment.Top) +
                            androidx.compose.animation.fadeIn(Motion.blendOpen()),
                        exit = androidx.compose.animation.shrinkVertically(Motion.blendCloseSize(), shrinkTowards = Alignment.Top) +
                            androidx.compose.animation.fadeOut(Motion.blendClose()),
                    ) { BlendEditor(vm, l) }
                }
            }
            if (!vm.modelOpen) {
                itemsIndexed(shown, key = { _, l -> l.id.toLong() }) { index, l ->
                    // the list's ends round; the rows between are square
                    val shape = RoundedCornerShape(
                        topStart = if (index == 0) ListCorner else 0.dp,
                        topEnd = if (index == 0) ListCorner else 0.dp,
                        bottomStart = if (index == shown.size - 1) ListCorner else 0.dp,
                        bottomEnd = if (index == shown.size - 1) ListCorner else 0.dp,
                    )
                    layerItem(index, l, shape, 0.dp, null)
                }
            } else {
                // on a model: each texture set headed by its name, its meshes under it; the open
                // mesh's texture set's layers under that, shared by every mesh in the set
                vm.textureSets.forEachIndexed { set, setName ->
                    val meshes = vm.submeshes.withIndex().filter { it.value.textureSet.toInt() == set }
                    if (meshes.isEmpty()) return@forEachIndexed
                    item(key = "set $set") { SetHeader(setName, first = set == 0) }
                    meshes.forEachIndexed { k, (m, info) ->
                        val open = vm.openSubmesh == m
                        val lastMesh = k == meshes.size - 1
                        val layersUnder = open && set == vm.textureSet && shown.isNotEmpty()
                        item(key = "mesh $m") {
                            SubmeshRow(
                                vm,
                                m,
                                info.name,
                                open = layersUnder,
                                shape = RoundedCornerShape(
                                    topStart = if (k == 0) ListCorner else 0.dp,
                                    topEnd = if (k == 0) ListCorner else 0.dp,
                                    bottomStart = if (lastMesh && !layersUnder) ListCorner else 0.dp,
                                    bottomEnd = if (lastMesh && !layersUnder) ListCorner else 0.dp,
                                ),
                            )
                        }
                        if (layersUnder) {
                            itemsIndexed(shown, key = { _, l -> "layer ${l.id}" }) { index, l ->
                                val end = lastMesh && index == shown.size - 1
                                val shape = RoundedCornerShape(bottomStart = if (end) ListCorner else 0.dp, bottomEnd = if (end) ListCorner else 0.dp)
                                layerItem(index, l, shape, GroupIndent, if (meshes.size > 1) "Shared" else null)
                            }
                        }
                    }
                }
            }
            // it moves down and up with the rows above it
            if (!vm.modelOpen) item(key = "background") {
                Box(
                    Modifier.animateItem(null, if (dragId != null || blendAt >= 0) null else Motion.rowMove(), null)
                        .padding(top = 10.dp).graphicsLayer { alpha = if (blendAt >= 0) belowShown else 1f },
                ) { BackgroundRow(vm) }
            }
        }
    }
    // the title menu's Delete asks first
    Alert(
        if (deletingChosen) Unit else null,
        title = { "Delete these layers?" },
        message = { "Undo brings them back." },
        action = "Delete",
        icon = PcIcons.Trash,
        onCancel = { deletingChosen = false },
        onAction = {
            vm.deletePicked()
            deletingChosen = false
        },
    )
}

/**
 * The title's menu, under it: Select All; Group and Merge (with layers picked); Share and
 * Duplicate; Delete.
 */
@Composable
private fun LayersTitleMenu(open: Boolean, vm: CanvasViewModel, onDismiss: () -> Unit, onShare: () -> Unit, onDelete: () -> Unit) {
    val picked = vm.anyPicked
    // mostly over the panel, whose rows the canvas's frosting doesn't blur: nearly solid
    LibraryMenu(
        open,
        androidx.compose.ui.graphics.TransformOrigin(0.5f, 0f),
        MenuPlace { a, size -> IntOffset(a.center.x - size.width / 2, a.bottom) },
        onDismiss,
        overPanel = true,
    ) {
        MenuItem(PcIcons.MenuSelectAll, "Select All") {
            onDismiss()
            vm.pickAllLayers()
        }
        MenuDivider()
        if (picked) {
            MenuItem(PcIcons.MenuGroup, "Group") {
                onDismiss()
                vm.groupPicked()
            }
            MenuItem(PcIcons.MenuMerge, "Merge") {
                onDismiss()
                vm.mergePicked()
            }
            MenuDivider()
        }
        MenuItem(PcIcons.MenuShare, "Share") {
            onDismiss()
            onShare()
        }
        MenuItem(PcIcons.MenuDuplicate, "Duplicate") {
            onDismiss()
            vm.duplicatePicked()
        }
        MenuDivider()
        MenuItem(PcIcons.MenuTrash, "Delete", Pc.MenuDanger) {
            onDismiss()
            onDelete()
        }
    }
}

/** Drops [l] where its row's middle came to rest: just above or below the row it is over. */
private fun drop(vm: CanvasViewModel, l: LayerInfo, y: Float, rows: Map<ULong, Rect>) {
    val unit = vm.unitOf(l)
    val others = vm.layers.filter { it.id !in unit && !vm.hidden(it) }.mapNotNull { o -> rows[o.id]?.let { o to it } }
    val (target, row) = others.firstOrNull { (_, r) -> y >= r.top && y < r.bottom }
        ?: others.minByOrNull { (_, r) -> abs(r.center.y - y) }
        ?: return
    vm.moveNextTo(l, target, above = y < row.center.y)
}

/**
 * A row that slides left to show Lock, Duplicate (not for a mask) and Delete, or a little right to
 * pick it ([onPick]), springing back.
 */
@Composable
private fun SwipeRow(
    vm: CanvasViewModel,
    l: LayerInfo,
    open: Boolean,
    enabled: Boolean,
    onOpen: (Boolean) -> Unit,
    onPick: () -> Unit,
    content: @Composable () -> Unit,
) {
    val buttons = if (l.kind == LayerType.MASK) 2 else 3
    // the row slides 6 past the buttons
    val reach = with(LocalDensity.current) { (SwipeButton * buttons + SwipeGap * (buttons - 1) + 6.dp).toPx() }
    val pickReach = with(LocalDensity.current) { PickReach.toPx() }
    val offset = remember { Animatable(0f) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(open, reach) { offset.animateTo(if (open) -reach else 0f, Motion.swipeSettle()) }
    // the list only clips top and bottom; a slid row stops at its edge
    Box(Modifier.clipToBounds()) {
        if (offset.value < 0f) {
            Row(Modifier.matchParentSize(), horizontalArrangement = Arrangement.spacedBy(SwipeGap, Alignment.End)) {
                SwipeAction(if (l.locked) "Unlock" else "Lock", SwipeFill, SwipeInk) {
                    vm.toggleLocked(l)
                    onOpen(false)
                }
                if (l.kind != LayerType.MASK) SwipeAction("Duplicate", SwipeFill, SwipeInk) {
                    vm.duplicate(l)
                    onOpen(false)
                }
                // the only layer can't go: it is cleared instead
                val alone = vm.layers.count { it.kind != LayerType.MASK } == 1
                SwipeAction(if (alone) "Clear" else "Delete", SwipeDelete, Color.White) {
                    if (alone) vm.clear(l) else vm.delete(l)
                    onOpen(false)
                }
            }
        }
        Box(
            Modifier.offset { IntOffset(offset.value.roundToInt(), 0) }.pointerInput(l.id, open, enabled, reach) {
                if (!enabled) return@pointerInput
                detectHorizontalDragGestures(
                    onHorizontalDrag = { change, amount ->
                        change.consume()
                        // closed, it goes right as far as picking takes
                        scope.launch { offset.snapTo((offset.value + amount).coerceIn(-reach, if (open) 0f else pickReach)) }
                    },
                    onDragEnd = {
                        if (offset.value > pickReach / 2f) {
                            onPick()
                            scope.launch { offset.animateTo(0f, Motion.swipeSettle()) }
                            return@detectHorizontalDragGestures
                        }
                        val opened = offset.value < -reach / 2f
                        if (opened == open) scope.launch { offset.animateTo(if (open) -reach else 0f, Motion.swipeSettle()) } else onOpen(opened)
                    },
                    onDragCancel = { scope.launch { offset.animateTo(if (open) -reach else 0f, Motion.swipeSettle()) } },
                )
            },
        ) { content() }
    }
}

@Composable
private fun SwipeAction(label: String, fill: Color, ink: Color, onClick: () -> Unit) {
    Box(
        Modifier.width(SwipeButton).fillMaxHeight().clip(RoundedCornerShape(8.dp)).background(fill).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = ink, fontSize = 15.sp, fontWeight = FontWeight.Medium, maxLines = 1)
    }
}

/** A panel's title row, [height] tall: the title [start] in, its menu chevron, and [action] [end] from the right. */
@Composable
internal fun PanelHeader(
    title: String,
    start: Dp = 24.dp,
    end: Dp = 18.dp,
    height: Dp = 64.dp,
    chevron: Boolean = true,
    chevronOn: Boolean = false,
    onTitle: (() -> Unit)? = null,
    menu: @Composable () -> Unit = {},
    action: @Composable () -> Unit,
) {
    Box(Modifier.fillMaxWidth().height(height)) {
        Row(
            Modifier.align(Alignment.TopStart).padding(start = start, top = 19.dp)
                .then(if (onTitle != null) Modifier.clickable(interactionSource = null, indication = null, onClick = onTitle) else Modifier),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(title, color = Pc.PanelTitle, fontSize = 24.sp, fontWeight = FontWeight.Bold, maxLines = 1)
            menu()
            if (chevron) {
                Box(
                    Modifier.padding(start = 10.dp).offset(y = (-1.5).dp).size(17.dp).clip(CircleShape)
                        .background(if (chevronOn) Pc.RowSelected else Pc.ChevronDisc),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(PcIcons.ChevronDown, contentDescription = null, tint = Pc.ChevronGlyph, modifier = Modifier.size(13.dp))
                }
            }
        }
        Box(Modifier.align(Alignment.TopEnd).padding(end = end)) { action() }
    }
}

/** A row's name, where it always is; locked, a small padlock in the gap before it. */
@Composable
private fun LockedName(l: LayerInfo, ink: Color, name: @Composable (Color, Modifier) -> Unit, modifier: Modifier) {
    Box(modifier, contentAlignment = Alignment.CenterStart) {
        // the padlock's glyph ends 12 short of the name
        if (l.locked) Icon(PcIcons.LockRow, contentDescription = "Locked", tint = ink, modifier = Modifier.offset(x = 3.dp).size(17.dp))
        name(ink, Modifier.padding(start = 29.dp, end = 6.dp))
    }
}

/** A group's row: a line where a thumbnail would be, its name, its open or closed arrow and its box. */
@Composable
private fun GroupRow(
    vm: CanvasViewModel,
    l: LayerInfo,
    shape: Shape,
    name: @Composable (Color, Modifier) -> Unit,
    onTap: () -> Unit,
    indent: Dp,
    onToggle: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().height(GroupHeight).clip(shape)
            .background(if (l.current) Pc.RowSelected else if (l.picked) PickedRow else Pc.LayerRow)
            .clickable(onClick = onTap)
            .padding(start = indent + GroupIndent * l.depth.toInt()),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val ink = if (l.current || l.picked) Color.White else Pc.PanelTitle
        Box(Modifier.padding(start = 4.dp).size(93.dp, 4.dp).clip(RoundedCornerShape(2.dp)).background(GroupLine))
        LockedName(l, ink, name, Modifier.weight(1f))
        Box(Modifier.offset(x = 8.dp).size(44.dp, GroupHeight).clickable(onClick = onToggle), contentAlignment = Alignment.Center) {
            Icon(if (l.collapsed) PcIcons.ChevronRight else PcIcons.ChevronDown, contentDescription = if (l.collapsed) "Open" else "Close", tint = ink, modifier = Modifier.size(16.dp))
        }
        Box(Modifier.size(44.dp, GroupHeight).clickable { vm.toggleVisible(l) }, contentAlignment = Alignment.Center) {
            CheckBox(l.visible, if (l.current || l.picked) Color.White else Pc.CheckFill)
        }
        Spacer(Modifier.width(8.dp))
    }
}

@Composable
private fun LayerRow(
    vm: CanvasViewModel,
    l: LayerInfo,
    shape: Shape,
    name: @Composable (Color, Modifier) -> Unit,
    owner: Boolean,
    onTap: () -> Unit,
    onBlend: () -> Unit,
    indent: Dp = 0.dp,
    subtitle: String? = null,
    onCube: (() -> Unit)? = null,
    cubeLit: Boolean = false,
    selected: Boolean = l.current,
) {
    val mask = l.kind == LayerType.MASK
    val masked = vm.hasMask(l)
    Row(
        Modifier.fillMaxWidth().height(if (mask || masked) MaskedRowHeight else RowHeight).clip(shape)
            // the layer a selected mask belongs to shows a darker blue
            .background(if (selected) Pc.RowSelected else if (l.picked) PickedRow else if (owner) MaskOwnerRow else Pc.LayerRow)
            .clickable(onClick = onTap)
            .padding(start = indent + (if (l.clipped) 18.dp else 0.dp) + GroupIndent * l.depth.toInt()),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val ink = if (selected || l.picked) Color.White else Pc.PanelTitle
        if (l.clipped) Icon(PcIcons.Clip, contentDescription = "Clipped", tint = ink, modifier = Modifier.size(16.dp))
        // a mask's thumbnail and its layer's join into one, rounded only at the outer corners
        val corner = 4.dp
        val thumbShape = when {
            mask -> RoundedCornerShape(topStart = corner, topEnd = corner)
            masked -> RoundedCornerShape(bottomStart = corner, bottomEnd = corner)
            else -> RoundedCornerShape(corner)
        }
        val shift = when {
            mask -> 2.dp
            masked -> (-2).dp
            else -> 0.dp
        }
        Box(Modifier.padding(start = 4.dp).offset(y = shift)) { Thumb(vm, l, thumbShape) }
        // what the layer is, under its name (which stays in the middle): the reference layer, a
        // private one, a model's layer shared by its texture set's meshes
        val tag = when {
            l.reference -> subtitle?.let { "$it Reference" } ?: "Reference"
            subtitle != null -> subtitle
            l.private -> "Private"
            else -> null
        }
        Box(Modifier.weight(1f).fillMaxHeight(), contentAlignment = Alignment.CenterStart) {
            LockedName(l, ink, name, Modifier)
            if (tag != null) {
                Text(tag, color = ink, fontSize = 13.sp, maxLines = 1, modifier = Modifier.offset(y = 17.5.dp).padding(start = 29.dp))
            }
        }
        // a model's layer: the button that shows its channels, blue while one of them is painted
        if (onCube != null) {
            Box(Modifier.offset(x = 16.dp).size(44.dp, 47.dp).clickable(onClick = onCube), contentAlignment = Alignment.Center) {
                Icon(PcIcons.Tab3D, contentDescription = "Channels", tint = if (cubeLit) Pc.Accent else ink, modifier = Modifier.requiredSize(48.dp))
            }
        }
        // the blend and visibility buttons overlap by 8, as on the reference; a mask has no blend
        if (mask) {
            Spacer(Modifier.width(36.dp))
        } else {
            Box(Modifier.offset(x = 8.dp).size(44.dp, 47.dp).clickable(onClick = onBlend), contentAlignment = Alignment.Center) {
                // a locked layer's mode is greyed
                val letter = if (l.locked && !l.current && !l.picked) LockedLetter else ink
                Text(blendCode(vm, l.blend), color = letter, fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
            }
        }
        Box(Modifier.size(44.dp, 47.dp).clickable { vm.toggleVisible(l) }, contentAlignment = Alignment.Center) {
            CheckBox(l.visible, if (selected || l.picked) Color.White else Pc.CheckFill)
        }
        Spacer(Modifier.width(8.dp))
    }
}

private val TrayFill = Color(0xFF303032)
private val TileFill = Color(0xFF3B3B3D)

/**
 * A model's layer's channels under its row: Color, Roughness and Metallic, each its picture of
 * the layer; the one painted on (of the current layer) blue. A tap paints on it.
 */
@Composable
private fun ChannelTray(vm: CanvasViewModel, l: LayerInfo) {
    Row(Modifier.fillMaxWidth().height(159.dp).background(TrayFill).padding(4.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        listOf("Color", "Roughness", "Metallic").forEachIndexed { c, label ->
            val chosen = l.current && vm.channelTileChosen && vm.modelChannel == c
            Column(
                Modifier.weight(1f).fillMaxHeight().clip(RoundedCornerShape(6.dp)).background(if (chosen) Pc.Accent else TileFill)
                    .clickable {
                        if (!l.current) vm.selectLayer(l.id)
                        vm.chooseChannelTile(c)
                    }
                    .padding(6.dp),
            ) {
                Box(Modifier.fillMaxWidth().height(102.dp).clip(RoundedCornerShape(3.dp)).background(Pc.ThumbBackground)) {
                    vm.channelThumbnails[c to l.id]?.let { Image(it, contentDescription = null, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize()) }
                }
                Text(label, color = if (chosen) Color.White else Pc.PanelTitle, fontSize = 15.sp, maxLines = 1, modifier = Modifier.padding(start = 2.dp, top = 9.dp))
            }
        }
    }
}

/** A layer's visibility box: filled with a check when shown, an outline when hidden. */
@Composable
private fun CheckBox(checked: Boolean, fill: Color) {
    val shape = RoundedCornerShape(5.dp)
    val tick = androidx.compose.ui.graphics.vector.rememberVectorPainter(PcIcons.Tick)
    Box(
        Modifier.size(18.dp)
            .graphicsLayer { compositingStrategy = androidx.compose.ui.graphics.CompositingStrategy.Offscreen }
            .clip(shape)
            .then(if (checked) Modifier.background(fill) else Modifier.border(1.5.dp, fill, shape))
            .drawWithContent {
                drawContent()
                if (!checked) return@drawWithContent
                // the tick is cut out of the box: the row shows through it
                val side = 14.dp.toPx()
                val inset = (size.width - side) / 2f
                val cut = androidx.compose.ui.graphics.Paint().apply { blendMode = androidx.compose.ui.graphics.BlendMode.DstOut }
                drawContext.canvas.saveLayer(Rect(Offset.Zero, size), cut)
                translate(inset, inset) { with(tick) { draw(Size(side, side)) } }
                drawContext.canvas.restore()
            },
    )
}

@Composable
private fun Thumb(vm: CanvasViewModel, l: LayerInfo, shape: Shape) {
    Box(
        Modifier.size(width = 93.dp, height = 61.dp).clip(shape)
            // a mask's thumbnail sits on white
            .then(
                when {
                    l.alphaLock -> Modifier.drawBehind { checker() }
                    l.kind == LayerType.MASK -> Modifier.background(Color.White)
                    else -> Modifier.background(Pc.ThumbBackground)
                },
            ),
        contentAlignment = Alignment.Center,
    ) {
        // the thumbnail is what is drawn, fitted; small drawings show their pixels
        vm.thumbnails[l.id]?.let {
            Image(it, contentDescription = null, contentScale = ContentScale.Fit, filterQuality = FilterQuality.None, modifier = Modifier.fillMaxSize())
        }
    }
}

/** The transparency checkerboard behind thumbnails. */
private fun androidx.compose.ui.graphics.drawscope.DrawScope.checker() {
    val cell = 6.dp.toPx()
    drawRect(Color(0xFFFFFFFF))
    var y = 0
    while (y * cell < size.height) {
        var x = 0
        while (x * cell < size.width) {
            if ((x + y) % 2 == 1) drawRect(Color(0xFFD6D6D6), Offset(x * cell, y * cell), Size(cell, cell))
            x++
        }
        y++
    }
}

/**
 * The letters on a layer's row for its mode: a two-word name's initials (the second small), a
 * one-word name's first letter, or its first two where one letter is already taken.
 */
private fun blendCode(vm: CanvasViewModel, blend: UInt): String {
    val name = vm.blendModeList.firstOrNull { it.id == blend }?.name ?: return "N"
    if (name in TWO_LETTER_BLENDS) return name.take(2)
    val words = name.split(' ')
    return if (words.size == 1) name.take(1) else words[0].take(1) + words[1].take(1).lowercase()
}

private val TWO_LETTER_BLENDS = setOf("Darken", "Shade", "Lighten", "Subtract", "Divide", "Saturation")

@Composable
private fun BlendEditor(vm: CanvasViewModel, l: LayerInfo) {
    var opacity by remember(l.id) { mutableFloatStateOf(l.opacity) }
    Column(Modifier.fillMaxWidth().padding(top = 22.dp, bottom = 15.dp)) {
        Row(Modifier.padding(horizontal = 13.dp).fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text("Opacity", color = Pc.PanelTitle, fontSize = 17.sp, modifier = Modifier.weight(1f))
            Text(if (opacity >= 0.995f) "Max" else "${(opacity * 100).roundToInt()}%", color = Pc.PanelTitle, fontSize = 17.sp)
        }
        OpacitySlider(
            opacity,
            onChange = {
                opacity = it
                vm.setLayerOpacity(l, it, done = false)
            },
            onDone = { vm.setLayerOpacity(l, opacity, done = true) },
        )
        // a picker: the rows snap to the list's middle and the row there is the layer's mode (set
        // once a scroll settles); the ends padded so every row can reach the middle, the layer's
        // mode there when it opens
        val modes = vm.blendModeList
        val layer by rememberUpdatedState(l)
        val list = rememberLazyListState(modes.indexOfFirst { it.id == l.blend }.coerceAtLeast(0))
        val scope = rememberCoroutineScope()
        LaunchedEffect(list.isScrollInProgress) {
            if (list.isScrollInProgress) return@LaunchedEffect
            val info = list.layoutInfo
            val middle = (info.viewportStartOffset + info.viewportEndOffset) / 2
            val at = info.visibleItemsInfo.minByOrNull { kotlin.math.abs(it.offset + it.size / 2 - middle) } ?: return@LaunchedEffect
            val m = modes.getOrNull(at.index) ?: return@LaunchedEffect
            if (m.id != layer.blend) vm.setBlend(layer, m.id)
        }
        LazyColumn(
            Modifier.padding(top = 11.dp).fillMaxWidth().height(BlendListHeight).clip(RoundedCornerShape(12.dp)).background(Pc.LayerRow),
            state = list,
            contentPadding = PaddingValues(vertical = (BlendListHeight - BlendRow) / 2),
            flingBehavior = rememberSnapFlingBehavior(list, SnapPosition.Center),
        ) {
            itemsIndexed(modes) { i, m ->
                val chosen = m.id == l.blend
                val ink = if (chosen) Color.White else Pc.PanelTitle
                Row(
                    Modifier.fillMaxWidth().height(BlendRow).background(if (chosen) Pc.RowSelected else Color.Transparent)
                        .clickable { scope.launch { list.animateScrollToItem(i) } }.padding(start = 12.dp, end = 18.5.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(m.name, color = ink, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
                    // its 26 frame centred 31.5 in from the right; grey unless chosen
                    PcIcons.BlendGlyphs[m.name]?.let {
                        Icon(it, contentDescription = null, tint = if (chosen) Color.White else BlendGlyphInk, modifier = Modifier.size(26.dp))
                    }
                }
            }
        }
    }
}

/** The layer's blend list: nine rows of [BlendRow] showing; its glyphs a darker grey than its names. */
private val BlendRow = 38.8.dp
private val BlendListHeight = 349.dp
private val BlendGlyphInk = Color(0xFF808080)

/** A thin blue slider with a white knob, set by dragging anywhere along it. */
@Composable
internal fun OpacitySlider(value: Float, onChange: (Float) -> Unit, onDone: () -> Unit) {
    val change by rememberUpdatedState(onChange)
    val done by rememberUpdatedState(onDone)
    Box(
        Modifier.padding(horizontal = 13.dp, vertical = 2.dp).fillMaxWidth().height(16.dp)
            .pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    change((down.position.x / size.width).coerceIn(0f, 1f))
                    while (true) {
                        val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!c.pressed) break
                        change((c.position.x / size.width).coerceIn(0f, 1f))
                        c.consume()
                    }
                    done()
                }
            }
            .drawBehind {
                val y = size.height / 2f
                val h = 3.dp.toPx()
                drawRoundRect(Pc.SliderRest, Offset(0f, y - h / 2f), Size(size.width, h), CornerRadius(h / 2f))
                drawRoundRect(Pc.Accent, Offset(0f, y - h / 2f), Size(size.width * value, h), CornerRadius(h / 2f))
                val knob = Size(26.dp.toPx(), 15.dp.toPx())
                val x = (size.width * value).coerceIn(knob.width / 2f, size.width - knob.width / 2f)
                drawRoundRect(Color.White, Offset(x - knob.width / 2f, y - knob.height / 2f), knob, CornerRadius(knob.height / 2f))
            },
    )
}
/**
 * The current layer's options, left of the panel and centred on its row. What doesn't apply to
 * it is left out; lock, duplicate and delete are on the row's swipe.
 */
@Composable
private fun LayerOptions(open: Boolean, vm: CanvasViewModel, l: LayerInfo, rowY: Float, onRename: () -> Unit, onDone: () -> Unit) {
    val density = LocalDensity.current
    val place = remember(rowY, density) {
        OptionsPlace(with(density) { (-183).dp.roundToPx() }, rowY, with(density) { OptionsTop.roundToPx() })
    }
    MenuPresence(open, androidx.compose.ui.graphics.TransformOrigin(1f, 0.5f)) { look ->
        PcPopup(textScale = MATCHED_POPUP_TEXT, popupPositionProvider = place, onDismissRequest = onDone, properties = PopupProperties(focusable = true)) {
            // frosted over the canvas, as the library's menus are
            Column(look.width(194.dp).clip(RoundedCornerShape(24.dp)).then(rememberFrost(24.dp)).background(Pc.GlassMenu).padding(vertical = 7.dp)) {
                val then = { f: () -> Unit -> { f(); onDone() } }
                Option("Rename", onClick = then(onRename))
                when (l.kind) {
                    LayerType.GROUP -> {
                        Option("Ungroup", onClick = then { vm.ungroup(l) })
                        Option("Flatten", onClick = then { vm.flattenGroup(l) })
                    }
                    LayerType.MASK -> {
                        Option("Select", onClick = then { vm.selectLayerContents(l) })
                        Option("Fill Layer", enabled = !l.locked, onClick = then { vm.fillLayer(l) })
                        Option("Clear", enabled = !l.locked, onClick = then { vm.clear(l) })
                        Option("Invert", enabled = !l.locked, onClick = then { vm.invertLayer(l) })
                    }
                    else -> {
                        Option("Select", onClick = then { vm.selectLayerContents(l) })
                        Option("Copy", onClick = then { vm.copyLayerContents(l) })
                        // a text layer's pixels come from its text
                        if (!l.text) {
                            Option("Fill Layer", enabled = !l.locked, onClick = then { vm.fillLayer(l) })
                            Option("Clear", enabled = !l.locked, onClick = then { vm.clear(l) })
                        }
                        Option("Alpha Lock", checked = l.alphaLock) { vm.toggleAlphaLock(l) }
                        Option("Mask", enabled = !vm.hasMask(l), onClick = then { vm.addMask(l) })
                        if (l.canClip) Option("Clipping Mask", checked = l.clipped) { vm.toggleClipped(l) }
                        // only while the drawing guide is on
                        if (vm.guide?.on == true) {
                            Option("Drawing Assist", checked = vm.guide?.assisted == true) { vm.setGuideAssisted(vm.guide?.assisted != true) }
                        }
                        if (!l.text) Option("Invert", enabled = !l.locked, onClick = then { vm.invertLayer(l) })
                        Option("Reference", checked = l.reference) { vm.toggleReference(l) }
                    }
                }
                if (l.text) {
                    Option("Edit Text", enabled = !l.locked, onClick = then { vm.editText(l) })
                    Option("Rasterize", enabled = !l.locked, onClick = then { vm.rasterize(l) })
                }
                if (l.kind != LayerType.MASK) {
                    if (l.canMerge) Option("Merge Down", enabled = !l.locked, onClick = then { vm.mergeDown(l) })
                    if (l.canCombine) Option("Combine Down", onClick = then { vm.combineDown(l) })
                }
            }
        }
    }
}

/** Puts the options left of the panel, centred on the row tapped and kept below the top bar. */
private class OptionsPlace(private val left: Int, private val rowY: Float, private val top: Int) : PopupPositionProvider {
    override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
        val bottom = (windowSize.height - popupContentSize.height).coerceAtLeast(top)
        val y = (rowY - popupContentSize.height / 2f).roundToInt().coerceIn(top, bottom)
        return IntOffset(anchorBounds.left + left, y)
    }
}

@Composable
private fun Option(label: String, checked: Boolean? = null, enabled: Boolean = true, destructive: Boolean = false, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().height(38.dp).clickable(enabled = enabled, onClick = onClick).padding(start = 19.dp, end = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val color = when {
            !enabled -> Pc.MenuDisabled
            destructive -> Color(0xFFFF453A)
            else -> Color.White
        }
        Text(label, color = color, fontSize = 17.sp, modifier = Modifier.weight(1f))
        if (checked == true) Icon(PcIcons.Tick, contentDescription = null, tint = color, modifier = Modifier.size(18.dp))
    }
}
/** A model's texture set's name, heading its meshes: 41 tall, 16 under the set before it. */
@Composable
private fun SetHeader(name: String, first: Boolean) {
    Column {
        if (!first) Spacer(Modifier.height(16.dp))
        Box(Modifier.fillMaxWidth().height(41.dp), contentAlignment = Alignment.TopStart) {
            Text(name, color = Pc.PanelTitle, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, modifier = Modifier.padding(start = 16.dp, top = 12.dp))
        }
    }
}

/** A model's mesh opened in the layers panel: its row darkened blue. */
private val SubmeshOpen = Color(0xFF25344A)
private val SubmeshTile = Color(0xFF434246)

/**
 * One of a model's meshes: its picture, its name, the arrow that opens it (its texture set's
 * layers under it) and its box (shown or hidden). A tap anywhere but the box opens or closes it.
 */
@Composable
private fun SubmeshRow(vm: CanvasViewModel, index: Int, name: String, open: Boolean, shape: Shape) {
    val hidden = vm.submeshesHidden.getOrNull(index) == true
    Row(
        Modifier.fillMaxWidth().height(RowHeight).clip(shape).background(if (open) SubmeshOpen else Pc.LayerRow)
            .clickable { vm.openSubmesh(index) },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.padding(start = 4.dp).size(93.dp, 61.dp).clip(RoundedCornerShape(4.dp)).background(SubmeshTile), contentAlignment = Alignment.Center) {
            vm.submeshThumbnails[index]?.let { Image(it, contentDescription = null, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize().padding(4.dp)) }
        }
        Text(name, color = Pc.PanelTitle, fontSize = 17.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f).padding(start = 29.dp, end = 6.dp))
        Box(Modifier.offset(x = 8.dp).size(44.dp, 51.dp).clickable { vm.openSubmesh(index) }, contentAlignment = Alignment.Center) {
            Icon(if (open) PcIcons.ChevronDown else PcIcons.ChevronRight, contentDescription = if (open) "Close" else "Open", tint = Pc.PanelTitle, modifier = Modifier.size(16.dp))
        }
        Box(Modifier.size(44.dp, 47.dp).clickable { vm.toggleSubmeshHidden(index) }, contentAlignment = Alignment.Center) {
            CheckBox(!hidden, Pc.CheckFill)
        }
        Spacer(Modifier.width(8.dp))
    }
}

@Composable
private fun BackgroundRow(vm: CanvasViewModel) {
    val bg = vm.background
    Row(
        Modifier.fillMaxWidth().height(50.dp).clip(RoundedCornerShape(17.dp)).background(Pc.BackgroundRow),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(Modifier.weight(1f).fillMaxHeight().clickable { vm.pickBackground() }, verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.padding(start = 4.dp).size(92.dp, 42.dp).clip(RoundedCornerShape(14.dp)).background(Color(bg.r, bg.g, bg.b)))
            Text("Background color", color = Pc.PanelTitle, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f).padding(start = 14.dp))
        }
        Box(Modifier.size(44.dp, 41.dp).clickable { vm.toggleBackground() }, contentAlignment = Alignment.Center) {
            CheckBox(bg.visible, Pc.CheckFill)
        }
        Spacer(Modifier.width(8.dp))
    }
}
