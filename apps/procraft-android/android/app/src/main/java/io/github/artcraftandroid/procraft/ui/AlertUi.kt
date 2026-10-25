package io.github.artcraftandroid.procraft.ui

import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.PopupProperties

// buttons keep their width and the panel grows with them: 428 across for two, 632 for three
private val AlertButtonWidth = 192.dp
private val AlertButtonGap = 12.dp
private val AlertSide = 16.dp
// dark glass, or light with the light interface
private val AlertFill get() = if (Pc.light) Color(0xF2E3E3E3) else Color(0xF21E1E1E)
private val AlertInk get() = if (Pc.light) Color(0xFF222222) else Color(0xFFC5C5C5)
private val AlertCancelFill get() = if (Pc.light) Color(0xFFD9D9D9) else Color(0xFF2D2D2D)
private val AlertCancelInk get() = if (Pc.light) Color.Black else Color.White
private val AlertDestroyFill get() = if (Pc.light) Color(0xFFFD0100) else Color(0xFFFE2D2E)
private const val ALERT_DIM = 0.7f
private const val ALERT_START_SCALE = 1.1f

/**
 * An alert over everything while [value] is not null: the screen dims to 70% black as a dark glass
 * panel fades in, settling from a little larger (0.3 s); in it a title (after an [icon]), a
 * message, and [cancel] beside the action, red when it [destructive]ly destroys; an [extra] choice
 * (blue, as the action) can sit between them, the panel widening for it. It goes over 0.35 s.
 */
@Composable
fun <T : Any> Alert(
    value: T?,
    title: (T) -> String,
    message: (T) -> String,
    action: String,
    onCancel: () -> Unit,
    onAction: (T) -> Unit,
    icon: ImageVector? = null,
    destructive: Boolean = true,
    cancel: String = "Cancel",
    extra: String? = null,
    onExtra: (T) -> Unit = {},
    first: ((T) -> Unit)? = null,
) {
    val last = remember { arrayOfNulls<Any>(1) }
    if (value != null) last[0] = value
    val shown by animateFloatAsState(
        if (value != null) 1f else 0f,
        if (value != null) tween(300, easing = Motion.EaseInOut) else tween(350, easing = CubicBezierEasing(0f, 0f, 0.58f, 1f)),
        label = "alert",
    )
    @Suppress("UNCHECKED_CAST")
    val kept = last[0] as T? ?: return
    if (shown <= 0f) return
    PcPopup(
        textScale = MATCHED_POPUP_TEXT,
        alignment = Alignment.Center,
        onDismissRequest = { if (value != null) onCancel() },
        properties = PopupProperties(focusable = value != null, clippingEnabled = false),
    ) {
        Box(
            Modifier.fillMaxSize().background(Color.Black.copy(alpha = ALERT_DIM * shown))
                // nothing under it is touched while it is up
                .pointerInput(Unit) { awaitEachGesture { awaitFirstDown().consume() } },
            contentAlignment = Alignment.Center,
        ) {
            val buttons = if (extra != null) 3 else 2
            Column(
                Modifier.width(AlertButtonWidth * buttons + AlertButtonGap * (buttons - 1) + AlertSide * 2).graphicsLayer {
                    alpha = shown
                    val k = ALERT_START_SCALE + (1f - ALERT_START_SCALE) * shown
                    scaleX = k
                    scaleY = k
                }.clip(RoundedCornerShape(24.dp)).background(AlertFill).padding(start = AlertSide, end = AlertSide, top = 21.dp, bottom = 15.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Row(Modifier.height(29.dp), verticalAlignment = Alignment.CenterVertically) {
                    icon?.let {
                        Icon(it, contentDescription = null, tint = Pc.Accent, modifier = Modifier.size(24.dp))
                        Spacer(Modifier.width(8.dp))
                    }
                    Text(title(kept), color = AlertInk, fontSize = 22.sp, fontWeight = FontWeight.Bold, maxLines = 1)
                }
                Spacer(Modifier.height(13.dp))
                message(kept).takeIf { it.isNotEmpty() }?.let { Text(it, color = AlertInk, fontSize = 16.sp, lineHeight = 20.sp, textAlign = TextAlign.Center) }
                Spacer(Modifier.height(27.dp))
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(AlertButtonGap)) {
                    // a first choice of three, blue too (a tap outside still cancels), or Cancel
                    if (first != null) {
                        AlertButton(cancel, Pc.Accent, Color.White, Modifier.weight(1f)) { if (value != null) first(kept) }
                    } else {
                        AlertButton(cancel, AlertCancelFill, AlertCancelInk, Modifier.weight(1f)) { if (value != null) onCancel() }
                    }
                    extra?.let { AlertButton(it, Pc.Accent, Color.White, Modifier.weight(1f)) { if (value != null) onExtra(kept) } }
                    AlertButton(action, if (destructive) AlertDestroyFill else Pc.Accent, Color.White, Modifier.weight(1f)) { if (value != null) onAction(kept) }
                }
            }
        }
    }
}

@Composable
private fun AlertButton(label: String, fill: Color, ink: Color, modifier: Modifier, onClick: () -> Unit) {
    Box(modifier.height(48.dp).clip(CircleShape).background(fill).clickable(onClick = onClick), contentAlignment = Alignment.Center) {
        Text(label, color = ink, fontSize = 17.sp, fontWeight = FontWeight.Medium, maxLines = 1)
    }
}

/** How well a PDF keeps its pictures: JPEG at these qualities, or exactly. */
enum class PdfQuality(val label: String, val jpeg: Int) { GOOD("Good", 60), BETTER("Better", 85), BEST("Best", 100) }

/** Asks how well a PDF keeps its pictures while [open]: Good, Better or Best. */
@Composable
fun PdfQualityAlert(open: Boolean, onCancel: () -> Unit, onPick: (PdfQuality) -> Unit) {
    Alert(
        if (open) Unit else null,
        title = { "PDF quality" },
        message = { "" },
        action = PdfQuality.BEST.label,
        onCancel = onCancel,
        onAction = { onPick(PdfQuality.BEST) },
        icon = PcIcons.Page,
        destructive = false,
        cancel = PdfQuality.GOOD.label,
        extra = PdfQuality.BETTER.label,
        onExtra = { onPick(PdfQuality.BETTER) },
        first = { onPick(PdfQuality.GOOD) },
    )
}
