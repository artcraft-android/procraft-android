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
import io.github.artcraftandroid.procraft.canvas.QuickAction
import androidx.compose.foundation.gestures.detectTapGestures
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
    // a tap elsewhere closes the picker if it is open, else the menu
    BoxWithConstraints(Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures { if (choosing != null) choosing = null else vm.quickMenuAt = null } }) {
        // kept on the screen
        val c = with(density) {
            val x = at.x.toDp().coerceIn(RingReachX + RingButtonWidth / 2, maxWidth - RingReachX - RingButtonWidth / 2)
            val y = at.y.toDp().coerceIn(RingReachY + RingButtonHeight, maxHeight - RingReachY - RingButtonHeight)
            x to y
        }
        Box(
            Modifier.offset(x = c.first - RingButtonWidth / 2, y = c.second - RingButtonHeight / 2).size(RingButtonWidth, RingButtonHeight)
                .graphicsLayer { alpha = shown.coerceIn(0f, 1f) }.clip(CircleShape).background(TitleFill),
            contentAlignment = Alignment.Center,
        ) {
            Text("QuickMenu 1", color = TitleInk, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
        }
        val slots = vm.quickMenuSlots
        slots.forEachIndexed { i, action ->
            val (dx, dy) = Slots.getOrNull(i) ?: return@forEachIndexed
            val x = c.first + dx.dp * shown - RingButtonWidth / 2
            val y = c.second + dy.dp * shown - RingButtonHeight / 2
            Box(
                Modifier.offset(x = x, y = y).size(RingButtonWidth, RingButtonHeight).graphicsLayer { alpha = shown.coerceIn(0f, 1f) }.clip(CircleShape)
                    .background(ButtonFill).border(1.dp, ButtonEdge, CircleShape)
                    .combinedClickable(
                        onClick = {
                            vm.quickMenuAt = null
                            vm.runQuickAction(action, at)
                        },
                        onLongClick = { choosing = i },
                    ),
                contentAlignment = Alignment.Center,
            ) {
                Text(action.button, color = ButtonInk, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, textAlign = TextAlign.Center)
            }
        }
        // the actions a held button can be set to, hung from it
        choosing?.let { i ->
            val (dx, dy) = Slots.getOrNull(i) ?: return@let
            val centre = c.first + dx.dp
            val top = c.second + dy.dp - RingButtonHeight / 2
            ActionPicker(slots.getOrNull(i) ?: QuickAction.NONE, centre, top, top + RingButtonHeight, maxWidth, maxHeight) {
                vm.setQuickSlot(i, it)
                choosing = null
            }
        }
    }
}

/**
 * A held QuickMenu button's picker, hung from the button by an arrow (above it when there is no
 * room below): every action by name, the button's own blue, scrolled to it; Adjustments opens
 * their own list. A row tapped sets the button.
 */
@Composable
private fun ActionPicker(current: QuickAction, centreX: Dp, buttonTop: Dp, buttonBottom: Dp, maxWidth: Dp, maxHeight: Dp, onPick: (QuickAction) -> Unit) {
    var adjustments by remember { mutableStateOf(current.adjustment != null) }
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
    Column(
        Modifier.offset(x = left, y = top).size(PickerWidth, PickerHeight).shadow(24.dp, RoundedCornerShape(PickerCorner))
            .clip(RoundedCornerShape(PickerCorner)).background(PickerFill).pointerInput(Unit) { detectTapGestures { } },
    ) {
        // the title; in Adjustments a way back
        Row(
            Modifier.fillMaxWidth().height(PickerHeader).let { if (adjustments) it.clickable { adjustments = false } else it }
                .padding(start = if (adjustments) 13.dp else 21.dp, top = 15.dp),
            verticalAlignment = Alignment.Top,
        ) {
            if (adjustments) {
                Icon(PcIcons.ChevronLeft, contentDescription = "Back", tint = PickerTitleInk, modifier = Modifier.size(24.dp))
                Spacer(Modifier.width(4.dp))
            }
            Text(if (adjustments) "Adjustments" else "Set Action", color = PickerTitleInk, fontSize = 19.3.sp, fontWeight = FontWeight.Bold, maxLines = 1)
        }
        val rows: List<QuickAction?> = if (adjustments) QuickAction.ADJUSTMENTS_LISTED else QuickAction.LISTED
        // the button's own action blue (in Adjustments, the first when it is none of them)
        val chosen = if (adjustments) rows.indexOf(current).takeIf { it >= 0 } ?: 0 else rows.indexOf(current)
        val list = androidx.compose.foundation.lazy.rememberLazyListState()
        val past = with(LocalDensity.current) { (PickerRowPitch * 2 - PickerChosenAt).roundToPx() }
        androidx.compose.runtime.LaunchedEffect(adjustments) {
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
                val a = rows[k]
                val on = k == chosen
                Box(
                    Modifier.fillMaxWidth().height(PickerRowHeight).clip(CircleShape).background(if (on) Pc.Accent else PickerRowFill)
                        .clickable { if (a == null) adjustments = true else onPick(a) }.padding(horizontal = 16.dp),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    Text(a?.label ?: "Adjustments", color = if (on) Color.White else PickerRowInk, fontSize = 14.5.sp, fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal, maxLines = 1)
                    if (a == null) {
                        Icon(PcIcons.ChevronRight, contentDescription = null, tint = PickerRowInk, modifier = Modifier.align(Alignment.CenterEnd).size(18.dp))
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
