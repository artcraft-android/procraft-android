package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.unit.Dp
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import io.github.artcraftandroid.procraft.canvas.ADJUSTMENTS_LISTED
import io.github.artcraftandroid.procraft.canvas.LISTED
import io.github.artcraftandroid.procraft.canvas.adjustment
import io.github.artcraftandroid.procraft.canvas.label
import io.github.artcraftandroid.procraft.core.BrushInfo
import io.github.artcraftandroid.procraft.core.BrushSetInfo
import io.github.artcraftandroid.procraft.core.QuickAction
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.clipRect
import kotlin.math.roundToInt
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel

// glass: the buttons a dark tint over the canvas with a faint rim, the middle one a light tint
private val ButtonFill = Color(0x9C282828)
private val ButtonEdge = Color(0x40FFFFFF)
private val ButtonInk = Color(0xFFF7F7F7)
private val TitleFill = Color(0x94CFCFCF)
private val TitleInk = Color(0xFF0F0F0F)
private val RingButtonWidth = 171.dp
private val RingButtonHeight = 48.dp

/** Where the buttons' centres are from the middle, clockwise from the top. */
private val Slots = listOf(0 to -69, 191 to -39, 191 to 39, 0 to 69, -191 to 39, -191 to -39)
private val RingReachX = 191.dp
private val RingReachY = 69.dp

/**
 * The QuickMenu: its name in the middle of where it was opened, its six actions round it; an action
 * tapped runs and closes it, a tap anywhere else closes it, and a button held lists the actions it
 * can be set to.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun BoxScope.QuickMenu(vm: CanvasViewModel) {
    // the buttons spread out from the point as it opens, and draw back in as it closes (the last
    // point kept while they go)
    val target = vm.quickMenuAt
    val last = remember { arrayOfNulls<Offset>(1) }
    if (target != null) last[0] = target
    val open = remember { androidx.compose.animation.core.Animatable(0f) }
    androidx.compose.runtime.LaunchedEffect(target) {
        if (target != null) {
            open.snapTo(0f)
            open.animateTo(1f, Motion.quickMenuOpen())
        } else {
            open.animateTo(0f, Motion.quickMenuClose())
        }
    }
    val at = last[0] ?: return
    if (target == null && open.value <= 0f) return
    val shown = open.value
    val density = LocalDensity.current
    // the button whose action is being chosen
    var choosing by remember { mutableStateOf<Int?>(null) }
    // the menus' popover (the middle tapped): open, the menus added while it shows, the row blue
    var menusOpen by remember { mutableStateOf(false) }
    val fresh = remember { mutableStateListOf<Int>() }
    var marked by remember { mutableStateOf<Int?>(null) }
    val closeMenus = {
        menusOpen = false
        fresh.clear()
    }
    // a tap elsewhere closes the picker or the menus if open, else the menu
    BoxWithConstraints(
        Modifier.fillMaxSize().pointerInput(Unit) {
            detectTapGestures {
                when {
                    choosing != null -> choosing = null
                    menusOpen -> closeMenus()
                    else -> vm.quickMenuAt = null
                }
            }
        },
    ) {
        // kept on the screen
        val c = with(density) {
            val x = at.x.toDp().coerceIn(RingReachX + RingButtonWidth / 2, maxWidth - RingReachX - RingButtonWidth / 2)
            val y = at.y.toDp().coerceIn(RingReachY + RingButtonHeight, maxHeight - RingReachY - RingButtonHeight)
            x to y
        }
        Box(
            Modifier.offset(x = c.first - RingButtonWidth / 2, y = c.second - RingButtonHeight / 2).size(RingButtonWidth, RingButtonHeight)
                .graphicsLayer { alpha = shown.coerceIn(0f, 1f) }.clip(CircleShape).background(TitleFill)
                // tapped, it lists the menus
                .pointerInput(Unit) {
                    detectTapGestures {
                        if (choosing != null) return@detectTapGestures
                        if (menusOpen) {
                            closeMenus()
                        } else {
                            menusOpen = true
                            marked = vm.quickMenuActive
                        }
                    }
                },
            contentAlignment = Alignment.Center,
        ) {
            Text(vm.quickMenus.getOrNull(vm.quickMenuShown)?.name.orEmpty(), color = TitleInk, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
        }
        val slots = vm.quickMenuSlots
        slots.forEachIndexed { i, _ ->
            val (dx, dy) = Slots.getOrNull(i) ?: return@forEachIndexed
            val x = c.first + dx.dp * shown - RingButtonWidth / 2
            val y = c.second + dy.dp * shown - RingButtonHeight / 2
            Box(
                Modifier.offset(x = x, y = y).size(RingButtonWidth, RingButtonHeight).graphicsLayer { alpha = shown.coerceIn(0f, 1f) }.clip(CircleShape)
                    .background(ButtonFill).border(1.dp, ButtonEdge, CircleShape)
                    .combinedClickable(
                        onClick = {
                            if (menusOpen) {
                                closeMenus()
                            } else {
                                vm.quickMenuAt = null
                                vm.runQuickSlot(i, at)
                            }
                        },
                        onLongClick = { if (!menusOpen) choosing = i },
                    ),
                contentAlignment = Alignment.Center,
            ) {
                Text(vm.quickSlotLabel(i), color = ButtonInk, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, textAlign = TextAlign.Center)
            }
        }
        // the actions a held button can be set to, hung from it
        choosing?.let { i ->
            val (dx, dy) = Slots.getOrNull(i) ?: return@let
            val centre = c.first + dx.dp
            val top = c.second + dy.dp - RingButtonHeight / 2
            ActionPicker(
                slots.getOrNull(i) ?: QuickAction.NONE, vm.quickSlotBrush(i), vm.brushSets, centre, top, top + RingButtonHeight, maxWidth, maxHeight,
                onBrush = {
                    vm.setQuickSlotBrush(i, it)
                    choosing = null
                },
            ) {
                vm.setQuickSlot(i, it)
                choosing = null
            }
        }
        if (menusOpen) {
            val left = c.first - RingButtonWidth / 2
            val top = c.second - RingButtonHeight / 2
            MenusPopover(
                vm, left, top, left + RingButtonWidth, top + RingButtonHeight, maxWidth, maxHeight, fresh, marked,
                onAdd = {
                    // it shows, but the active one stays until a row is chosen
                    fresh.add(0, vm.addQuickMenu())
                    marked = null
                },
                onChoose = {
                    vm.chooseQuickMenu(it)
                    marked = it
                },
                onDelete = { i ->
                    vm.deleteQuickMenu(i)
                    fresh.remove(i)
                    for (k in fresh.indices) if (fresh[k] > i) fresh[k] = fresh[k] - 1
                    marked = null
                },
            )
        }
    }
}

// the menus' popover (as measured): the picker's panel, 249 tall, 17 off the middle with its
// arrow's tip 5.5 off it; rows from 54 under its top, 8 in; a plus 21 across and 3.6 thick, 30 from
// its right and 28 from its top; a row swiped 70 shows a red Delete 63 x 42, 4.5 in from its end
private val MenusHeight = 249.dp
private val MenusOff = 17.dp
private val MenusTip = 5.5.dp
private val MenusRowsAt = 54.dp
private val MenusInset = 8.dp
private val PlusLength = 21.dp
private val PlusWeight = 3.6.dp
private val PlusInk = Color(0xFFC0C0C0)
private val SwipeOpen = 70.dp
private val DeleteWidth = 63.dp
private val DeleteHeight = 42.dp
private val DeleteIn = 4.5.dp
private val DeleteFill = Color(0xFFFE4346)

/**
 * The QuickMenu's menus, hung from the middle (below it where it fits, else above, else beside): a
 * plus adds one and shows it, a row tapped makes it the active one, a row swiped left shows Delete
 * (swiped most of the way, it goes at once). Menus added while it shows are listed first.
 */
@Composable
private fun MenusPopover(
    vm: CanvasViewModel,
    left: Dp,
    top: Dp,
    right: Dp,
    bottom: Dp,
    maxWidth: Dp,
    maxHeight: Dp,
    fresh: List<Int>,
    marked: Int?,
    onAdd: () -> Unit,
    onChoose: (Int) -> Unit,
    onDelete: (Int) -> Unit,
) {
    val cx = (left + right) / 2
    val cy = (top + bottom) / 2
    val places = listOf(
        cx - PickerWidth / 2 to bottom + MenusOff,
        cx - PickerWidth / 2 to top - MenusOff - MenusHeight,
        right + MenusOff to cy - MenusHeight / 2,
        left - MenusOff - PickerWidth to cy - MenusHeight / 2,
    )
    val side = places.indexOfFirst { (x, y) ->
        x >= PickerMargin && y >= PickerMargin && x + PickerWidth <= maxWidth - PickerMargin && y + MenusHeight <= maxHeight - PickerMargin
    }.coerceAtLeast(0)
    val (px, py) = places[side]
    // the arrow, its point just off the middle
    Canvas(Modifier.fillMaxSize()) {
        val a = PickerArrow.toPx()
        val tip = MenusTip.toPx()
        val path = Path()
        when (side) {
            0 -> path.apply { moveTo(cx.toPx(), bottom.toPx() + tip); lineTo(cx.toPx() + a, py.toPx() + 1f); lineTo(cx.toPx() - a, py.toPx() + 1f) }
            1 -> path.apply { moveTo(cx.toPx(), top.toPx() - tip); lineTo(cx.toPx() - a, (py + MenusHeight).toPx() - 1f); lineTo(cx.toPx() + a, (py + MenusHeight).toPx() - 1f) }
            2 -> path.apply { moveTo(right.toPx() + tip, cy.toPx()); lineTo(px.toPx() + 1f, cy.toPx() - a); lineTo(px.toPx() + 1f, cy.toPx() + a) }
            else -> path.apply { moveTo(left.toPx() - tip, cy.toPx()); lineTo((px + PickerWidth).toPx() - 1f, cy.toPx() + a); lineTo((px + PickerWidth).toPx() - 1f, cy.toPx() - a) }
        }
        path.close()
        drawPath(path, PickerFill)
    }
    Box(
        Modifier.offset(x = px, y = py).size(PickerWidth, MenusHeight).shadow(24.dp, RoundedCornerShape(PickerCorner))
            .clip(RoundedCornerShape(PickerCorner)).background(PickerFill).pointerInput(Unit) { detectTapGestures { } },
    ) {
        Text(
            "QuickMenu", color = PickerTitleInk, fontSize = 19.3.sp, fontWeight = FontWeight.Bold, maxLines = 1,
            modifier = Modifier.padding(start = 21.dp, top = 16.dp),
        )
        // the plus, its bars' ends round
        Canvas(
            Modifier.offset(x = PickerWidth - 30.dp - 22.dp, y = 28.dp - 22.dp).size(44.dp)
                .pointerInput(Unit) { detectTapGestures { onAdd() } },
        ) {
            val half = (PlusLength - PlusWeight).toPx() / 2
            val w = PlusWeight.toPx()
            drawLine(PlusInk, center - Offset(half, 0f), center + Offset(half, 0f), w, StrokeCap.Round)
            drawLine(PlusInk, center - Offset(0f, half), center + Offset(0f, half), w, StrokeCap.Round)
        }
        val menus = vm.quickMenus
        // the menus added this time first, newest first, then the rest as kept
        val order = fresh.filter { it in menus.indices } + menus.indices.filter { it !in fresh }
        // the row swiped and how far (px)
        var swipe by remember { mutableStateOf<Pair<Int, Float>?>(null) }
        val density = LocalDensity.current
        val openPx = with(density) { SwipeOpen.toPx() }
        androidx.compose.foundation.lazy.LazyColumn(
            Modifier.padding(top = MenusRowsAt, bottom = 9.5.dp).fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(PickerRowGap),
            contentPadding = PaddingValues(start = MenusInset, end = MenusInset, bottom = 6.dp),
        ) {
            items(order.size, key = { order[it] }) { k ->
                val i = order[k]
                val name = menus.getOrNull(i)?.name.orEmpty()
                val on = marked == i
                val offset = swipe?.takeIf { it.first == i }?.second ?: 0f
                BoxWithConstraints(
                    Modifier.fillMaxWidth().height(PickerRowHeight)
                        .pointerInput(i, menus.size) {
                            if (menus.size < 2) return@pointerInput
                            val widthPx = size.width.toFloat()
                            detectHorizontalDragGestures(
                                onDragEnd = {
                                    val o = swipe?.takeIf { it.first == i }?.second ?: 0f
                                    // past most of the row it goes; past half the Delete it stays open
                                    swipe = when {
                                        o < -widthPx * 0.6f -> {
                                            onDelete(i)
                                            null
                                        }
                                        o < -openPx / 2 -> i to -openPx
                                        else -> null
                                    }
                                },
                                onDragCancel = { swipe = null },
                            ) { change, dx ->
                                change.consume()
                                val o = swipe?.takeIf { it.first == i }?.second ?: 0f
                                swipe = i to (o + dx).coerceIn(-widthPx, 0f)
                            }
                        }
                        .pointerInput(i) {
                            // a tap closes a row left open, else chooses the menu
                            detectTapGestures { if (swipe != null) swipe = null else onChoose(i) }
                        },
                ) {
                    val rowPx = with(density) { maxWidth.toPx() }
                    // the Delete, shown as far as the row has moved off it
                    Box(
                        Modifier.align(Alignment.CenterEnd).padding(end = DeleteIn).size(DeleteWidth, DeleteHeight)
                            .drawWithContent {
                                val bare = rowPx + offset + 2.5.dp.toPx() - (rowPx - DeleteIn.toPx() - size.width)
                                clipRect(left = bare) { this@drawWithContent.drawContent() }
                            }
                            .clip(CircleShape).background(DeleteFill)
                            .pointerInput(i) {
                                detectTapGestures {
                                    if ((swipe?.takeIf { it.first == i }?.second ?: 0f) <= -openPx) {
                                        swipe = null
                                        onDelete(i)
                                    }
                                }
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text("Delete", color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
                    }
                    Box(
                        Modifier.offset { androidx.compose.ui.unit.IntOffset(offset.roundToInt(), 0) }.fillMaxSize()
                            .clip(CircleShape).background(if (on) Pc.Accent else PickerRowFill).padding(horizontal = 16.dp),
                        contentAlignment = Alignment.CenterStart,
                    ) {
                        Text(
                            name, color = if (on) Color.White else PickerRowInk, fontSize = RowText, letterSpacing = RowTracking,
                            fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal, maxLines = 1, modifier = Modifier.offset(y = RowTextDrop),
                        )
                    }
                }
            }
        }
    }
}

/** A page of a held button's picker. */
private enum class PickerPage { ACTIONS, ADJUSTMENTS, BRUSH_SETS, BRUSHES }

/** A row of the picker: what it says, whether it opens a list of its own, and what a tap does. */
private class PickerRow(val label: String, val more: Boolean, val tap: () -> Unit)

/**
 * A held QuickMenu button's picker, hung from the button by an arrow (above it when there is no
 * room below): every action by name, the button's own blue, scrolled to it. Adjustments opens
 * their own list; Select brush the brush sets, then a set's brushes. A row tapped sets the button.
 */
@Composable
private fun ActionPicker(
    current: QuickAction,
    brush: BrushInfo?,
    sets: List<BrushSetInfo>,
    centreX: Dp,
    buttonTop: Dp,
    buttonBottom: Dp,
    maxWidth: Dp,
    maxHeight: Dp,
    onBrush: (BrushInfo) -> Unit,
    onPick: (QuickAction) -> Unit,
) {
    var page by remember { mutableStateOf(if (current.adjustment != null) PickerPage.ADJUSTMENTS else PickerPage.ACTIONS) }
    var set by remember { mutableStateOf(0) }
    val below = buttonBottom + PickerArrow + PickerHeight <= maxHeight - PickerMargin
    val left = (centreX - PickerWidth / 2).coerceIn(PickerMargin, (maxWidth - PickerWidth - PickerMargin).coerceAtLeast(PickerMargin))
    val top = if (below) buttonBottom + PickerArrow else buttonTop - PickerArrow - PickerHeight
    // the arrow, its point on the button
    Canvas(Modifier.fillMaxSize()) {
        val x = centreX.toPx()
        val a = PickerArrow.toPx()
        val tip = if (below) buttonBottom.toPx() else buttonTop.toPx()
        val base = if (below) tip + a + 1f else tip - a - 1f
        drawPath(Path().apply { moveTo(x, tip); lineTo(x + a, base); lineTo(x - a, base); close() }, PickerFill)
    }
    // the page's title, where its way back goes, its rows and the one blue (in a list of its own,
    // the button's, else the first)
    val (title, back) = when (page) {
        PickerPage.ACTIONS -> "Set Action" to null
        PickerPage.ADJUSTMENTS -> "Adjustments" to PickerPage.ACTIONS
        PickerPage.BRUSH_SETS -> "Brush Set" to PickerPage.ACTIONS
        PickerPage.BRUSHES -> "Brushes" to PickerPage.BRUSH_SETS
    }
    val rows: List<PickerRow> = when (page) {
        PickerPage.ACTIONS -> QuickAction.LISTED.map { a ->
            when (a) {
                null -> PickerRow("Adjustments", true) { page = PickerPage.ADJUSTMENTS }
                QuickAction.BRUSHES -> PickerRow(a.label, true) {
                    set = brush?.set?.toInt() ?: 0
                    page = PickerPage.BRUSH_SETS
                }
                else -> PickerRow(a.label, false) { onPick(a) }
            }
        }
        PickerPage.ADJUSTMENTS -> QuickAction.ADJUSTMENTS_LISTED.map { a -> PickerRow(a.label, false) { onPick(a) } }
        PickerPage.BRUSH_SETS -> sets.mapIndexed { s, info ->
            PickerRow(info.name, true) {
                set = s
                page = PickerPage.BRUSHES
            }
        }
        PickerPage.BRUSHES -> sets.getOrNull(set)?.brushes.orEmpty().map { b -> PickerRow(b.name, false) { onBrush(b) } }
    }
    val chosen = when (page) {
        PickerPage.ACTIONS -> QuickAction.LISTED.indexOf(current)
        PickerPage.ADJUSTMENTS -> QuickAction.ADJUSTMENTS_LISTED.indexOf(current).coerceAtLeast(0)
        PickerPage.BRUSH_SETS -> brush?.set?.toInt() ?: 0
        PickerPage.BRUSHES -> brush?.takeIf { it.set.toInt() == set }?.index?.toInt() ?: 0
    }
    Column(
        Modifier.offset(x = left, y = top).size(PickerWidth, PickerHeight).shadow(24.dp, RoundedCornerShape(PickerCorner))
            .clip(RoundedCornerShape(PickerCorner)).background(PickerFill).pointerInput(Unit) { detectTapGestures { } },
    ) {
        // the title; in a list of its own a way back
        Row(
            Modifier.fillMaxWidth().height(PickerHeader).let { m -> back?.let { m.clickable { page = it } } ?: m }
                .padding(start = if (back != null) 13.dp else 21.dp, top = 15.dp),
            verticalAlignment = Alignment.Top,
        ) {
            if (back != null) {
                Icon(PcIcons.ChevronLeft, contentDescription = "Back", tint = PickerTitleInk, modifier = Modifier.size(24.dp))
                Spacer(Modifier.width(4.dp))
            }
            Text(title, color = PickerTitleInk, fontSize = 19.3.sp, fontWeight = FontWeight.Bold, maxLines = 1)
        }
        val list = androidx.compose.foundation.lazy.rememberLazyListState()
        val past = with(LocalDensity.current) { (PickerRowPitch * 2 - PickerChosenAt).roundToPx() }
        androidx.compose.runtime.LaunchedEffect(page, set) {
            // the chosen row a little below the list's top
            if (chosen >= 2) list.scrollToItem(chosen - 2, past) else list.scrollToItem(0)
        }
        androidx.compose.foundation.lazy.LazyColumn(
            Modifier.fillMaxWidth().weight(1f),
            state = list,
            verticalArrangement = Arrangement.spacedBy(PickerRowGap),
            contentPadding = PaddingValues(start = PickerInset, end = PickerInset, bottom = 6.dp),
        ) {
            items(rows.size) { k ->
                val row = rows[k]
                val on = k == chosen
                Box(
                    Modifier.fillMaxWidth().height(PickerRowHeight).clip(CircleShape).background(if (on) Pc.Accent else PickerRowFill)
                        .clickable { row.tap() }.padding(horizontal = 16.dp),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    Text(
                        row.label, color = if (on) Color.White else PickerRowInk, fontSize = RowText, letterSpacing = RowTracking,
                        fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal, maxLines = 1, modifier = Modifier.offset(y = RowTextDrop),
                    )
                    if (row.more) {
                        val ink = if (on) Color.White else PickerRowInk
                        Icon(PcIcons.ChevronRight, contentDescription = null, tint = ink, modifier = Modifier.align(Alignment.CenterEnd).size(18.dp))
                    }
                }
            }
        }
    }
}

// the picker (as measured): a dark panel 220 wide, corners 30, an arrow 14 tall; capsule rows 47
// tall on a 50 pitch, 8.5 in from its sides, under a 55.5 tall title
private val PickerWidth = 220.dp
private val PickerCorner = 30.dp
private val PickerArrow = 14.dp
private val PickerHeader = 55.5.dp
private val PickerRowHeight = 47.dp
private val PickerRowGap = 3.dp
private val PickerRowPitch = PickerRowHeight + PickerRowGap
private val PickerInset = 8.5.dp
/** The rows' labels (as measured by ink): their size, letter spacing, and how far below the row's middle. */
private val RowText = 15.1.sp
private val RowTracking = (-0.024).em
private val RowTextDrop = 0.75.dp
private val PickerHeight = PickerHeader + PickerRowPitch * 6 - PickerRowGap + 6.dp
private val PickerMargin = 8.dp
/** How far below the list's top the chosen row shows. */
private val PickerChosenAt = 89.dp
private val PickerFill = Color(0xFF373737)
private val PickerRowFill = Color(0xFF2E2E2E)
private val PickerRowInk = Color(0xFFC5C5C5)
private val PickerTitleInk = Color(0xFFC4C4C4)

/** Layer Select's list: the layers drawn under the point, top first, the current one blue. */
@Composable
fun BoxScope.LayerChoice(vm: CanvasViewModel) {
    val (at, ids) = vm.layerChoice ?: return
    val density = LocalDensity.current
    BoxWithConstraints(Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures { vm.layerChoice = null } }) {
        val x = with(density) { at.x.toDp() }.coerceIn(8.dp, maxWidth - 208.dp)
        val y = with(density) { at.y.toDp() + 24.dp }.coerceIn(8.dp, maxHeight - (ids.size * 44 + 16).dp)
        androidx.compose.foundation.layout.Column(
            Modifier.offset(x = x, y = y).size(200.dp, (ids.size * 44 + 16).dp).clip(androidx.compose.foundation.shape.RoundedCornerShape(16.dp))
                .background(Pc.PopoverFill).padding(vertical = 8.dp),
        ) {
            for (id in ids) {
                val l = vm.layers.firstOrNull { it.id == id } ?: continue
                Box(
                    Modifier.fillMaxWidth().height(44.dp).background(if (l.current) Pc.Accent else Color.Transparent).clickable {
                        vm.selectLayer(id)
                        vm.layerChoice = null
                    }.padding(start = 16.dp),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    Text(l.name, color = if (l.current) Color.White else Pc.PanelTitle, fontSize = 16.sp, maxLines = 1)
                }
            }
        }
    }
}
