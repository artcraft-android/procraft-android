package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.ExperimentalFoundationApi
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
    BoxWithConstraints(Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures { vm.quickMenuAt = null } }) {
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
                    .background(if (choosing == i) Pc.Accent else ButtonFill).border(1.dp, ButtonEdge, CircleShape)
                    .combinedClickable(
                        onClick = {
                            vm.quickMenuAt = null
                            vm.runQuickAction(action, at)
                        },
                        onLongClick = { choosing = i },
                    ),
                contentAlignment = Alignment.Center,
            ) {
                Text(action.button, color = if (choosing == i) Color.White else ButtonInk, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, textAlign = TextAlign.Center)
            }
        }
        // the actions a held button can be set to, beside the ring
        choosing?.let { i ->
            val right = c.first + RingReachX + RingButtonWidth / 2
            val left = (if (right + 236.dp < maxWidth) right + 16.dp else c.first - RingReachX - RingButtonWidth / 2 - 236.dp)
                .coerceIn(8.dp, (maxWidth - 228.dp).coerceAtLeast(8.dp))
            val top = (c.second - 220.dp).coerceIn(8.dp, (maxHeight - 448.dp).coerceAtLeast(8.dp))
            Column(
                Modifier.offset(x = left, y = top).width(220.dp).height(440.dp).clip(RoundedCornerShape(16.dp)).background(Pc.PopoverFill)
                    .verticalScroll(rememberScrollState()).padding(vertical = 6.dp),
            ) {
                for (a in QuickAction.entries) {
                    val chosen = a == slots.getOrNull(i)
                    Box(
                        Modifier.fillMaxWidth().height(40.dp).clickable {
                            vm.setQuickSlot(i, a)
                            choosing = null
                        }.padding(horizontal = 16.dp),
                        contentAlignment = Alignment.CenterStart,
                    ) {
                        Text(a.label, color = Pc.PanelTitle, fontSize = 16.sp, maxLines = 1)
                        if (chosen) Icon(PcIcons.Tick, contentDescription = null, tint = Pc.Accent, modifier = Modifier.align(Alignment.CenterEnd).size(18.dp))
                    }
                }
            }
        }
    }
}

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
