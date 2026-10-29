package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.TransformKind
import kotlin.math.abs
import kotlin.math.roundToInt

private val FieldIdle = Color(0xFF383838)
private val TopKey = Color(0xFF2E2E2E)
private val DigitKey = Color(0xFF414141)
private val KeyInk = Color(0xFFDADADA)
private val LinkOn = Color(0xFF1F436B)
private const val NUMERIC_WIDTH = 248
/** How far the arrow's tip stands from the node's middle (the panel's edge 17 from it). */
private val NUMERIC_NODE_GAP = 5.dp

/**
 * Numeric entry for an open transform, from a tap on a node: "Dimensions" (the box's width and
 * height, linked or not) or "Rotation" (its angle), typed on a keypad. Each key changes the box at
 * once; a tap outside closes it.
 */
@Composable
fun TransformNumeric(vm: CanvasViewModel) {
    val open = vm.transformNumeric ?: return
    val box = vm.transformBox ?: return
    val density = LocalDensity.current
    // which field is typed into (0 width or the angle, 1 height), what is typed so far, and the link
    var field by remember(open) { mutableStateOf(0) }
    var typed by remember(open) { mutableStateOf("") }
    var linked by remember(open) { mutableStateOf(vm.transformKind == TransformKind.UNIFORM) }
    fun value(i: Int): Float = if (open.angle) box.angle else if (i == 0) box.width else box.height
    fun shown(i: Int): String {
        if (field == i && typed.isNotEmpty()) return typed + if (open.angle) " º" else " px"
        val v = value(i)
        return if (open.angle) "${(((v % 360f) + 360f) % 360f).roundToInt()} º" else "${v.roundToInt()} px"
    }
    fun apply() {
        val v = typed.toFloatOrNull() ?: return
        if (open.angle) {
            vm.transformSetAngle(v)
            return
        }
        val (w, h) = box.width to box.height
        when {
            field == 0 && linked && w > 0f -> vm.transformSetSize(v, h * abs(v) / w)
            field == 0 -> vm.transformSetSize(v, h)
            linked && h > 0f -> vm.transformSetSize(w * abs(v) / h, v)
            else -> vm.transformSetSize(w, v)
        }
    }
    fun key(k: String) {
        typed = when (k) {
            "AC" -> ""
            "±" -> if (typed.startsWith("-")) typed.drop(1) else "-" + typed.ifEmpty { value(field).roundToInt().toString() }
            "⌫" -> typed.dropLast(1)
            "." -> if ('.' in typed) typed else typed.ifEmpty { "0" } + "."
            else -> if (typed.length < 7) typed + k else typed
        }
        apply()
    }
    // the panel's height once laid out (it shows from then on)
    var tall by remember(open) { mutableStateOf<Dp?>(null) }
    BoxWithConstraints(Modifier.fillMaxSize().pointerInput(open) { detectTapGestures { vm.transformNumeric = null } }) {
        // beside the node, on its side of the box, level with it and its arrow pointing at it, kept on the screen
        val width = NUMERIC_WIDTH.dp
        val whole = width + Pc.ArrowHeight
        val at = with(density) { open.at.x.toDp() to open.at.y.toDp() }
        val left = (if (open.right) at.first + NUMERIC_NODE_GAP else at.first - NUMERIC_NODE_GAP - whole)
            .coerceIn(8.dp, (maxWidth - whole - 8.dp).coerceAtLeast(8.dp))
        val height = tall ?: 0.dp
        val top = (at.second - height / 2).coerceIn(8.dp, (maxHeight - height - 8.dp).coerceAtLeast(8.dp))
        Popover(
            width,
            (at.second - top).coerceIn(28.dp, (height - 28.dp).coerceAtLeast(28.dp)),
            Modifier.offset(left, top).alpha(if (tall == null) 0f else 1f)
                .onSizeChanged { tall = with(density) { it.height.toDp() } }
                .pointerInput(Unit) { detectTapGestures { } },
            corner = 20.dp,
            side = if (open.right) ArrowSide.LEFT else ArrowSide.RIGHT,
        ) {
            Column(Modifier.padding(start = 10.dp, end = 10.dp, bottom = 10.dp)) {
                Text(if (open.angle) "Rotation" else "Dimensions", color = KeyInk, fontSize = 20.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 10.dp, top = 12.dp, bottom = 12.dp))
                if (open.angle) {
                    Row(Modifier.fillMaxWidth().height(41.dp).clip(RoundedCornerShape(10.dp)).background(FieldIdle), verticalAlignment = Alignment.CenterVertically) {
                        Text("Angle", color = KeyInk, fontSize = 15.sp, modifier = Modifier.weight(1f).padding(start = 12.dp))
                        NumberField(shown(0), chosen = true) { field = 0 }
                    }
                } else {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        NumberField(shown(0), chosen = field == 0) {
                            field = 0
                            typed = ""
                        }
                        Box(
                            Modifier.size(52.dp, 35.dp).clip(RoundedCornerShape(9.dp)).background(if (linked) LinkOn else FieldIdle).tap { linked = !linked },
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(PcIcons.Link, contentDescription = "Link", tint = if (linked) Pc.Accent else KeyInk, modifier = Modifier.size(22.dp))
                        }
                        NumberField(shown(1), chosen = field == 1) {
                            field = 1
                            typed = ""
                        }
                    }
                }
                Column(Modifier.padding(top = 14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (row in listOf(listOf("AC", "±", "⌫"), listOf("7", "8", "9"), listOf("4", "5", "6"), listOf("1", "2", "3"))) {
                        Row(horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                            for (k in row) NumericKey(k, if (row.first() == "AC") TopKey else DigitKey, 71) { key(k) }
                        }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                        NumericKey("0", DigitKey, 150) { key("0") }
                        NumericKey(".", DigitKey, 71) { key(".") }
                    }
                }
            }
        }
    }
}

@Composable
private fun NumberField(text: String, chosen: Boolean, onClick: () -> Unit) {
    Box(
        Modifier.size(80.dp, 41.dp).clip(RoundedCornerShape(10.dp)).background(if (chosen) Pc.Accent else FieldIdle).tap(onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, color = Color.White, fontSize = 15.sp, maxLines = 1)
    }
}

@Composable
private fun NumericKey(label: String, fill: Color, width: Int, onClick: () -> Unit) {
    Box(Modifier.size(width.dp, 48.dp).clip(RoundedCornerShape(9.dp)).background(fill).tap(onClick), contentAlignment = Alignment.Center) {
        if (label == "⌫") {
            Icon(PcIcons.Backspace, contentDescription = "Back space", tint = KeyInk, modifier = Modifier.size(22.dp))
        } else {
            Text(label, color = KeyInk, fontSize = if (label.length > 1) 15.sp else 22.sp)
        }
    }
}
