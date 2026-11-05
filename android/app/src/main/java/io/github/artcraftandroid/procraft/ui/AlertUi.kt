package io.github.artcraftandroid.procraft.ui

import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.paddingFromBaseline
import androidx.compose.foundation.layout.requiredSize
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

// buttons at least this wide, the panel growing with them (426 across for two, 630 for three) or
// with a longer message
private val AlertButtonWidth = 192.dp
private val AlertButtonGap = 12.dp
// as measured, the panel is this much wider each side than its message's widest line (or its
// title's), its buttons sharing it; never narrower than they need, nor wider than three
private val AlertTextSide = 27.5.dp
private val AlertMostWidth get() = AlertButtonWidth * 3 + AlertButtonGap * 2 + AlertSide * 2
private val AlertSide = 15.dp
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
 * (blue, as the action) can sit between them, the panel widening for it. A tap beside the panel
 * cancels. It goes over 0.35 s.
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
    alone: Boolean = false,
) {
    val last = remember { arrayOfNulls<Any>(1) }
    if (value != null) last[0] = value
    val shown by animateFloatAsState(
        if (value != null) 1f else 0f,
        if (value != null) tween(300, easing = Motion.EaseInOut) else tween(350, easing = CubicBezierEasing(0f, 0f, 0.58f, 1f)),
        label = "alert",
    )
    val cancelling = androidx.compose.runtime.rememberUpdatedState(if (value != null) onCancel else null)
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
                // nothing under it is touched while it is up, and a tap beside the panel cancels
                .pointerInput(Unit) {
                    awaitEachGesture {
                        awaitFirstDown().consume()
                        if (waitForUpOrCancellation() != null) cancelling.value?.invoke()
                    }
                },
            contentAlignment = Alignment.Center,
        ) {
            val buttons = if (alone) 1 else if (extra != null) 3 else 2
            val need = AlertButtonWidth * buttons + AlertButtonGap * (buttons - 1) + AlertSide * 2
            val measurer = androidx.compose.ui.text.rememberTextMeasurer()
            val density = androidx.compose.ui.platform.LocalDensity.current
            val said = message(kept)
            val words = with(density) {
                val m = if (said.isEmpty()) 0 else measurer.measure(said, androidx.compose.ui.text.TextStyle(fontSize = 16.sp, lineHeight = 20.sp)).size.width
                val t = measurer.measure(title(kept), androidx.compose.ui.text.TextStyle(fontSize = 26.sp, fontWeight = FontWeight.Bold)).size.width
                maxOf(m.toDp(), t.toDp() + if (icon != null) 32.dp else 0.dp)
            }
            val width = (words + AlertTextSide * 2).coerceIn(need, maxOf(AlertMostWidth, need))
            Column(
                Modifier.width(width).graphicsLayer {
                    alpha = shown
                    val k = ALERT_START_SCALE + (1f - ALERT_START_SCALE) * shown
                    scaleX = k
                    scaleY = k
                }.clip(RoundedCornerShape(24.dp)).background(AlertFill).border(1.dp, Pc.GlassMenuEdge, RoundedCornerShape(24.dp))
                    // a tap on the panel itself is not one beside it
                    .pointerInput(Unit) { awaitEachGesture { awaitFirstDown().consume() } }
                    .padding(start = AlertSide, end = AlertSide, top = 21.dp, bottom = 16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Row(Modifier.height(31.dp), verticalAlignment = Alignment.CenterVertically) {
                    icon?.let {
                        Icon(it, contentDescription = null, tint = Pc.Accent, modifier = Modifier.size(24.dp))
                        Spacer(Modifier.width(8.dp))
                    }
                    Text(title(kept), color = AlertInk, fontSize = 26.sp, fontWeight = FontWeight.Bold, maxLines = 1)
                }
                Spacer(Modifier.height(13.dp))
                said.takeIf { it.isNotEmpty() }?.let { Text(it, color = AlertInk, fontSize = 16.sp, lineHeight = 20.sp, textAlign = TextAlign.Center) }
                Spacer(Modifier.height(27.dp))
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(AlertButtonGap)) {
                    // a first choice of three, blue too (a tap outside still cancels), or Cancel; a
                    // notice has its action alone
                    if (alone) {
                        // nothing before it
                    } else if (first != null) {
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

// the export card, as measured: 398 across, its one button 17 in from the sides, a pie 76 across
// (its ring 4 wide) 40 under the title and 22 over the button, 15 under that
private val ExportCardWidth = 398.dp
private val ExportPie = 76.dp
private val ExportRing = 4.dp
// the check drawn in the full pie: through these points about its middle, 6 wide, round
private val ExportCheck = listOf(-14f to 1f, -4f to 11f, 16f to -10f)
private val ExportCheckWidth = 6.dp
private const val EXPORT_CHECK_DRAW_MS = 300

// the import card, as measured: the export card with the file's name (18) under the title, the
// title's row 4.5 lower and its glyph (16.5 across) 3.5 lower in it, 12 before the title, the
// name's baseline 35 under that row and the pie 58 under it
private val ImportTitleDrop = 4.5.dp
private val ImportGlyphDrop = 3.5.dp
private val ImportGlyphWidth = 16.5.dp
private val ImportGlyphGap = 12.dp
private val ImportNameBlock = 58.dp
private val ImportNameBaseline = 35.dp

/** A progress card's state: how far it has come (0..1), whether it is [done], and the file it is about. */
private data class CardShown(val progress: Float, val done: Boolean, val detail: String? = null)

/**
 * The card over everything while an export runs ([shown] not null): "Exporting" after the share
 * glyph, a pie filling clockwise from the top with how far it has come, and Cancel. Once
 * [CanvasViewModel.ExportShown.done] the pie is full and a check is drawn in it. It comes and goes
 * as an alert does, the screen dimmed; nothing under it is touched meanwhile.
 */
@Composable
fun ExportProgress(shown: io.github.artcraftandroid.procraft.canvas.CanvasViewModel.ExportShown?, onCancel: () -> Unit) =
    ProgressCard(shown?.let { CardShown(it.progress, it.done) }, "Exporting", PcIcons.TabShare, onCancel)

/**
 * The card over everything while a file is brought in ([shown] not null): as the export card, with
 * "Importing" after the import glyph and the file's name under it.
 */
@Composable
fun ImportProgress(shown: io.github.artcraftandroid.procraft.canvas.CanvasViewModel.ImportShown?, onCancel: () -> Unit) =
    ProgressCard(shown?.let { CardShown(it.progress, it.done, it.name) }, "Importing", PcIcons.TabImport, onCancel)

/**
 * The card over everything while the gallery is searched for lost artworks ([progress] not null):
 * as the import card, with nothing to cancel.
 */
@Composable
fun RecoveryProgress(progress: Float?, done: Boolean) = ProgressCard(
    progress?.let { CardShown(it, done, "Looking for lost or corrupted artworks") }, "Gallery Recovery", PcIcons.TabImport, null,
)

@Composable
private fun ProgressCard(shown: CardShown?, title: String, glyph: ImageVector, onCancel: (() -> Unit)?) {
    val last = remember { arrayOfNulls<CardShown>(1) }
    if (shown != null) last[0] = shown
    val appear by animateFloatAsState(
        if (shown != null) 1f else 0f,
        if (shown != null) tween(300, easing = Motion.EaseInOut) else tween(350, easing = CubicBezierEasing(0f, 0f, 0.58f, 1f)),
        label = "export card",
    )
    val kept = last[0] ?: return
    val check by animateFloatAsState(if (kept.done) 1f else 0f, tween(EXPORT_CHECK_DRAW_MS), label = "export check")
    if (appear <= 0f) return
    PcPopup(
        textScale = MATCHED_POPUP_TEXT,
        alignment = Alignment.Center,
        onDismissRequest = {},
        properties = PopupProperties(focusable = shown != null, clippingEnabled = false),
    ) {
        Box(
            Modifier.fillMaxSize().background(Color.Black.copy(alpha = ALERT_DIM * appear))
                .pointerInput(Unit) { awaitEachGesture { awaitFirstDown().consume() } },
            contentAlignment = Alignment.Center,
        ) {
            Column(
                Modifier.width(ExportCardWidth).graphicsLayer {
                    alpha = appear
                    val k = ALERT_START_SCALE + (1f - ALERT_START_SCALE) * appear
                    scaleX = k
                    scaleY = k
                }.clip(RoundedCornerShape(24.dp)).background(AlertFill).border(1.dp, Pc.GlassMenuEdge, RoundedCornerShape(24.dp))
                    .padding(start = 17.dp, end = 17.dp, top = 21.dp + if (kept.detail != null) ImportTitleDrop else 0.dp, bottom = 15.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Row(Modifier.height(31.dp), verticalAlignment = Alignment.CenterVertically) {
                    // the glyph 22 tall (it fills 18.6 of its 50 frame), 13 before the title
                    val importing = kept.detail != null
                    Box(
                        Modifier.size(if (importing) ImportGlyphWidth else 22.dp, 31.dp).offset(y = if (importing) ImportGlyphDrop else 0.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(glyph, contentDescription = null, tint = Pc.Accent, modifier = Modifier.requiredSize(59.dp))
                    }
                    Spacer(Modifier.width(if (importing) ImportGlyphGap else 13.dp))
                    Text(title, color = AlertInk, fontSize = 26.sp, fontWeight = FontWeight.Bold, maxLines = 1)
                }
                val detail = kept.detail
                if (detail == null) {
                    Spacer(Modifier.height(40.dp))
                } else {
                    Box(Modifier.fillMaxWidth().height(ImportNameBlock), contentAlignment = Alignment.TopCenter) {
                        Text(
                            detail,
                            color = AlertInk,
                            fontSize = 18.sp,
                            maxLines = 1,
                            overflow = androidx.compose.ui.text.style.TextOverflow.MiddleEllipsis,
                            modifier = Modifier.paddingFromBaseline(top = ImportNameBaseline),
                        )
                    }
                }
                val accent = Pc.Accent
                androidx.compose.foundation.Canvas(Modifier.size(ExportPie)) {
                    val r = size.minDimension / 2f
                    val ring = ExportRing.toPx()
                    if (kept.done) {
                        drawCircle(accent, r)
                        // the check drawn along its length as it shows
                        val path = androidx.compose.ui.graphics.Path().apply {
                            ExportCheck.forEachIndexed { i, (x, y) ->
                                val at = center + androidx.compose.ui.geometry.Offset(x.dp.toPx(), y.dp.toPx())
                                if (i == 0) moveTo(at.x, at.y) else lineTo(at.x, at.y)
                            }
                        }
                        val measure = androidx.compose.ui.graphics.PathMeasure().apply { setPath(path, false) }
                        val part = androidx.compose.ui.graphics.Path()
                        if (check > 0f && measure.getSegment(0f, measure.length * check, part, true)) {
                            drawPath(
                                part,
                                Color.White,
                                style = androidx.compose.ui.graphics.drawscope.Stroke(
                                    ExportCheckWidth.toPx(),
                                    cap = androidx.compose.ui.graphics.StrokeCap.Round,
                                    join = androidx.compose.ui.graphics.StrokeJoin.Round,
                                ),
                            )
                        }
                    } else {
                        drawCircle(accent, r - ring / 2f, style = androidx.compose.ui.graphics.drawscope.Stroke(ring))
                        drawArc(accent, -90f, 360f * kept.progress.coerceIn(0f, 1f), useCenter = true)
                    }
                }
                Spacer(Modifier.height(if (onCancel != null) 22.dp else 7.dp))
                if (onCancel != null) {
                    AlertButton("Cancel", AlertCancelFill, AlertCancelInk, Modifier.fillMaxWidth()) { if (shown != null && !kept.done) onCancel() }
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
