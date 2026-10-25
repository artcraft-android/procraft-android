package io.github.artcraftandroid.procraft.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.TextRenderer
import io.github.artcraftandroid.procraft.core.TextAlignment
import io.github.artcraftandroid.procraft.core.TextInfo
import kotlin.math.roundToInt

private val SheetFill = Color(0xFF535353)
private val SheetBar = Color(0xFF4F4F4F)
private val SheetCapsule = Color(0xFF3E3E3E)
private val CardFill = Color(0xFF373737)
private val ButtonFill = Color(0xFF474747)
private val SegmentTrack = Color(0xFF646464)
private val CapsuleInk = Color(0xFFC5C5C5)
private val KeyboardDisc = Color(0xFF393939)
private val KeyboardBar = Color(0xFF2B2B2B)
private val DoneBlue = Color(0xFF0190FF)
private val BoxLine = Color(0xFF0076FD)

/**
 * Text being edited: its box drawn over the canvas, a hidden field the keyboard types into, and a
 * bar above the keyboard with the style sheet's button and Done. Tapping outside the box keeps the
 * text.
 */
@Composable
fun BoxScope.TextEditor(vm: CanvasViewModel) {
    val info = vm.textInfo ?: return
    val focus = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    val box by rememberUpdatedState(windowBox(vm, info))
    // a tap outside the box ends the edit, keeping it; dragging inside it moves the text
    Box(Modifier.fillMaxSize().pointerInput(Unit) {
        awaitEachGesture {
            val down = awaitFirstDown()
            if (!box.contains(down.position)) {
                vm.endText(true)
                return@awaitEachGesture
            }
            // a touch on the text brings the keyboard back if it was put away
            focus.requestFocus()
            keyboard?.show()
            var last = down.position
            while (true) {
                val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                if (!c.pressed) break
                vm.moveText(last, c.position)
                last = c.position
                c.consume()
            }
        }
    })
    TextBox(vm, info)
    BasicTextField(vm.textField, vm::changeTextField, Modifier.size(1.dp).alpha(0f).focusRequester(focus))
    LaunchedEffect(vm.textStyleOpen) {
        if (vm.textStyleOpen) keyboard?.hide() else {
            focus.requestFocus()
            keyboard?.show()
        }
    }
    // the bar rides on the keyboard: put away, it goes with it
    val keyboardUp = WindowInsets.ime.getBottom(androidx.compose.ui.platform.LocalDensity.current) > 0
    Column(Modifier.align(Alignment.BottomCenter).fillMaxWidth().windowInsetsPadding(WindowInsets.ime)) {
        if (vm.textStyleOpen) TextStyleSheet(vm, info) else if (keyboardUp) TextBar(vm)
    }
}

/** A stroke on a text layer: it takes paint once rasterized. */
@Composable
fun RasterizePrompt(vm: CanvasViewModel) {
    Alert(
        if (vm.askRasterize) Unit else null,
        title = { "Rasterize this text layer?" },
        message = { "Its text can't be edited afterwards; you can paint on it." },
        action = "Rasterize",
        destructive = false,
        onCancel = { vm.askRasterize = false },
        onAction = {
            vm.askRasterize = false
            vm.layers.firstOrNull { it.current }?.let(vm::rasterize)
        },
    )
}

/** The text's box in the window, as the smallest upright rectangle around it. */
private fun windowBox(vm: CanvasViewModel, info: TextInfo): androidx.compose.ui.geometry.Rect {
    val b = TextRenderer.bounds(info)
    val corners = listOf(Offset(b.left, b.top), Offset(b.right, b.top), Offset(b.right, b.bottom), Offset(b.left, b.bottom)).map(vm::canvasToWindow)
    return androidx.compose.ui.geometry.Rect(corners.minOf { it.x }, corners.minOf { it.y }, corners.maxOf { it.x }, corners.maxOf { it.y })
}

/** The text's box: a thin blue line around where it is drawn, a dot at the middle of each side. */
@Composable
private fun TextBox(vm: CanvasViewModel, info: TextInfo) {
    val box = remember(info) { TextRenderer.bounds(info) }
    val layout = remember(info) { TextRenderer.layout(info) }
    val corners = listOf(Offset(box.left, box.top), Offset(box.right, box.top), Offset(box.right, box.bottom), Offset(box.left, box.bottom)).map(vm::canvasToWindow)
    val selection = vm.textField.selection
    Canvas(Modifier.fillMaxSize()) {
        val path = Path().apply {
            moveTo(corners[0].x, corners[0].y)
            corners.drop(1).forEach { lineTo(it.x, it.y) }
            close()
        }
        drawPath(path, BoxLine, style = Stroke(1.dp.toPx()))
        for (side in listOf(corners[0] + (corners[3] - corners[0]) / 2f, corners[1] + (corners[2] - corners[1]) / 2f)) {
            drawCircle(BoxLine, 3.dp.toPx(), side)
        }
        // the caret, or the selected letters ringed in blue with a handle at each end (the text as
        // laid out; vertical text spreads its letters apart, so it shows none)
        if (info.vertical) return@Canvas
        val length = layout.text.length
        val (start, end) = selection.min.coerceIn(0, length) to selection.max.coerceIn(0, length)
        val m = vm.viewMatrix
        val toWindow = android.graphics.Matrix().apply {
            setValues(floatArrayOf(m[0], m[2], m[4], m[1], m[3], m[5], 0f, 0f, 1f))
            preTranslate(info.x, info.y - info.baseline * info.size)
        }
        fun at(offset: Int, bottom: Boolean): Offset {
            val line = layout.getLineForOffset(offset)
            val p = floatArrayOf(layout.getPrimaryHorizontal(offset), (if (bottom) layout.getLineBottom(line) else layout.getLineTop(line)).toFloat())
            toWindow.mapPoints(p)
            return Offset(p[0], p[1])
        }
        if (start == end) {
            drawLine(BoxLine, at(start, false), at(start, true), 2.dp.toPx())
            return@Canvas
        }
        drawIntoCanvas { c ->
            val canvas = c.nativeCanvas
            val chosen = android.graphics.Path().also { layout.getSelectionPath(start, end, it) }
            val paint = layout.paint
            val (style, width, colour) = Triple(paint.style, paint.strokeWidth, paint.color)
            canvas.save()
            canvas.concat(toWindow)
            canvas.clipPath(chosen)
            paint.style = android.graphics.Paint.Style.STROKE
            paint.strokeWidth = info.size * SELECTED_RING
            paint.strokeJoin = android.graphics.Paint.Join.ROUND
            paint.color = BoxLine.toArgb()
            layout.draw(canvas)
            // the letters again in their own colour over their ring
            paint.style = style
            paint.strokeWidth = width
            paint.color = colour
            layout.draw(canvas)
            canvas.restore()
        }
        val (top, bottom) = at(start, false) to at(end, true)
        drawLine(BoxLine, top, at(start, true), 2.dp.toPx())
        drawLine(BoxLine, at(end, false), bottom, 2.dp.toPx())
        drawCircle(BoxLine, 6.dp.toPx(), top)
        drawCircle(BoxLine, 6.dp.toPx(), bottom)
    }
}

/** How wide the blue ring round selected letters is, as a fraction of the text's size. */
private const val SELECTED_RING = 0.08f

/** Above the keyboard: Edit style on the right. */
@Composable
private fun TextBar(vm: CanvasViewModel) {
    Row(Modifier.fillMaxWidth().height(44.dp).background(KeyboardBar).padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        Spacer(Modifier.weight(1f))
        Text("Edit style", color = Color.White, fontSize = 15.sp, modifier = Modifier.clickable { vm.openTextStyle() }.padding(8.dp))
    }
}

/**
 * The style sheet in the keyboard's place: a bar (the keyboard back, cut, copy and paste, import
 * a font, Cancel, Done); then Typeface (fonts and styles, each name in its own face) or Design
 * (size, spacing, opacity, and the attributes), switched at the bottom.
 */
@Composable
private fun TextStyleSheet(vm: CanvasViewModel, info: TextInfo) {
    var page by remember { mutableIntStateOf(0) }
    val sheetWide = androidx.compose.ui.platform.LocalConfiguration.current.screenWidthDp.dp >= WIDE_SHEET
    val importFont = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri -> uri?.let(vm::importFont) }
    Column(Modifier.fillMaxWidth().height(561.dp).background(SheetFill)) {
        Row(Modifier.fillMaxWidth().height(62.dp).background(SheetBar).padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            SheetIcon(PcIcons.Keyboard, "Keyboard", Modifier.size(44.dp).clip(CircleShape).background(KeyboardDisc), 40.dp) { vm.textStyleOpen = false }
            Spacer(Modifier.width(13.dp))
            Row(Modifier.height(40.dp).clip(RoundedCornerShape(12.dp)).background(SheetCapsule)) {
                SheetIcon(PcIcons.TextCut, "Cut", Modifier.size(41.dp, 40.dp), 40.dp) { vm.copyText(cut = true) }
                Spacer(Modifier.width(7.dp))
                SheetIcon(PcIcons.TextCopy, "Copy", Modifier.size(41.dp, 40.dp), 40.dp) { vm.copyText(cut = false) }
                Spacer(Modifier.width(7.dp))
                SheetIcon(PcIcons.TextPaste, "Paste", Modifier.size(41.dp, 40.dp), 40.dp) { vm.pasteText() }
            }
            Spacer(Modifier.weight(1f))
            Row(Modifier.height(43.dp).clip(CircleShape).background(SheetCapsule), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "Import", color = CapsuleInk, fontSize = 15.sp,
                    modifier = Modifier.clickable { importFont.launch(arrayOf("font/ttf", "font/otf", "font/sfnt", "application/x-font-ttf", "application/x-font-otf", "application/octet-stream")) }.padding(horizontal = 14.dp),
                )
                Text("Cancel", color = CapsuleInk, fontSize = 15.sp, modifier = Modifier.clickable { vm.cancelTextStyle() }.padding(horizontal = 14.dp))
            }
            Spacer(Modifier.width(13.dp))
            Box(Modifier.size(44.dp, 45.dp).clip(CircleShape).background(DoneBlue).clickable { vm.endText(true) }, contentAlignment = Alignment.Center) {
                Icon(PcIcons.DoneTick, contentDescription = "Done", tint = Color.White, modifier = Modifier.requiredSize(40.dp))
            }
        }
        // wide (landscape), the four cards side by side; narrow, Typeface or Design, switched below
        BoxWithConstraints(Modifier.fillMaxWidth().weight(1f)) {
            val wide = maxWidth >= WIDE_SHEET
            Row(Modifier.fillMaxSize().padding(horizontal = if (wide) 9.dp else 3.dp)) {
                if (wide) {
                    FontCard(vm, info, Modifier.weight(264f))
                    Spacer(Modifier.width(2.dp))
                    StyleCard(vm, info, Modifier.weight(184f))
                    Spacer(Modifier.width(8.dp))
                    DesignCard(vm, info, Modifier.weight(326f))
                    Spacer(Modifier.width(8.dp))
                    AttributesCard(vm, info, Modifier.weight(325f), wide = true)
                } else if (page == 0) {
                    FontCard(vm, info, Modifier.weight(417f))
                    Spacer(Modifier.width(2.dp))
                    StyleCard(vm, info, Modifier.weight(296f))
                } else {
                    DesignCard(vm, info, Modifier.weight(598f))
                    Spacer(Modifier.width(10.dp))
                    AttributesCard(vm, info, Modifier.weight(130f), wide = false)
                }
            }
        }
        // Typeface | Design, when the cards don't all fit
        if (!sheetWide) Box(Modifier.fillMaxWidth().height(81.dp).background(SheetFill)) {
            Row(
                Modifier.align(Alignment.TopCenter).padding(top = 11.dp).height(34.dp).clip(CircleShape).background(SegmentTrack).padding(2.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                for ((i, label) in listOf("Typeface", "Design").withIndex()) {
                    Box(
                        Modifier.width(156.dp).height(30.dp).clip(CircleShape).background(if (i == page) Pc.Accent else Color.Transparent).clickable { page = i },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(label, color = if (i == page) Color.White else CapsuleInk, fontSize = 13.sp)
                    }
                }
            }
        }
    }
}

/** Below this width the sheet shows its cards in two pages. */
private val WIDE_SHEET = 900.dp

@Composable
private fun FontCard(vm: CanvasViewModel, info: TextInfo, modifier: Modifier) {
    Card("Font", modifier) {
        LazyColumn(Modifier.padding(horizontal = 7.dp), verticalArrangement = Arrangement.spacedBy(9.dp)) {
            items(TextRenderer.families) { f ->
                Cell(f.name, f.family == info.font, TextRenderer.typeface(f.family, "Regular")) { vm.changeText { it.copy(font = f.family) } }
            }
        }
    }
}

@Composable
private fun StyleCard(vm: CanvasViewModel, info: TextInfo, modifier: Modifier) {
    Card("Style", modifier) {
        LazyColumn(Modifier.padding(horizontal = 7.dp), verticalArrangement = Arrangement.spacedBy(9.dp)) {
            items(TextRenderer.styles) { st ->
                Cell(st.name, st.name == info.style, TextRenderer.typeface(info.font, st.name)) { vm.changeText { it.copy(style = st.name) } }
            }
        }
    }
}

@Composable
private fun DesignCard(vm: CanvasViewModel, info: TextInfo, modifier: Modifier) {
    val dpi = vm.canvasDpi()
    val pt = { px: Float -> px * 72f / dpi }
    Card("Design", modifier) {
        Column(Modifier.padding(horizontal = 5.dp)) {
            DesignRow("Size", sizeToSlider(info.size), "%.1fpt".format(pt(info.size)), fromMiddle = false) { v -> vm.changeText { it.copy(size = sliderToSize(v)) } }
            DesignRow("Kerning", info.kerning / 2f + 0.5f, "%.1f%%".format(info.kerning * 100f)) { v -> vm.changeText { it.copy(kerning = v * 2f - 1f) } }
            DesignRow("Tracking", info.tracking / 2f + 0.5f, "%.1f%%".format(info.tracking * 100f)) { v -> vm.changeText { it.copy(tracking = v * 2f - 1f) } }
            DesignRow("Leading", info.leading / 2f + 0.5f, "${pt(info.leading * info.size).roundToInt()}pt") { v -> vm.changeText { it.copy(leading = v * 2f - 1f) } }
            DesignRow("Baseline", info.baseline / 2f + 0.5f, "${pt(info.baseline * info.size).roundToInt()}pt") { v -> vm.changeText { it.copy(baseline = v * 2f - 1f) } }
            DesignRow("Opacity", info.opacity, "%.1f%%".format(info.opacity * 100f), fromMiddle = false) { v -> vm.changeText { it.copy(opacity = v) } }
        }
    }
}

/**
 * Alignment, underline, outline, vertical and all caps: wide, the four alignments in a row and the
 * other three in a row under them; narrow, the alignments two by two and the others one a row.
 */
@Composable
private fun AttributesCard(vm: CanvasViewModel, info: TextInfo, modifier: Modifier, wide: Boolean) {
    val aligns = listOf(
        TextAlignment.LEFT to PcIcons.AlignLeft, TextAlignment.CENTER to PcIcons.AlignCenter,
        TextAlignment.RIGHT to PcIcons.AlignRight, TextAlignment.JUSTIFY to PcIcons.AlignJustify,
    )
    Card("Attributes", modifier) {
        Column(Modifier.padding(horizontal = if (wide) 9.dp else 4.dp), verticalArrangement = Arrangement.spacedBy(if (wide) 19.dp else 2.dp)) {
            for (row in aligns.chunked(if (wide) 4 else 2)) {
                Row(horizontalArrangement = Arrangement.spacedBy(3.dp)) {
                    for ((a, icon) in row) Attribute(icon, a.name, info.align == a, Modifier.weight(1f), 32.dp) { vm.changeText { it.copy(align = a) } }
                }
            }
            if (!wide) Spacer(Modifier.height(14.dp))
            val looks: @Composable (Modifier, Modifier, Modifier) -> Unit = { u, o, v ->
                Attribute(PcIcons.TextUnderline, "Underline", info.underline, u, 38.dp) { vm.changeText { it.copy(underline = !it.underline) } }
                Attribute(PcIcons.TextOutline, "Outline", info.outline, o, 38.dp) { vm.changeText { it.copy(outline = !it.outline) } }
                Attribute(PcIcons.TextVertical, "Vertical", info.vertical, v, 41.dp) { vm.changeText { it.copy(vertical = !it.vertical) } }
            }
            if (wide) {
                Row(horizontalArrangement = Arrangement.spacedBy(3.dp)) { looks(Modifier.weight(94f), Modifier.weight(94f), Modifier.weight(111f)) }
            } else {
                looks(Modifier.fillMaxWidth(), Modifier.fillMaxWidth(), Modifier.fillMaxWidth())
                Spacer(Modifier.height(16.dp))
            }
            Row(Modifier.clickable { vm.changeText { it.copy(allCaps = !it.allCaps) } }, verticalAlignment = Alignment.CenterVertically) {
                Text("TT", color = Color.White, fontSize = 20.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 8.dp).width(48.dp))
                if (wide) Spacer(Modifier.weight(1f))
                Switch(info.allCaps, enabled = true)
            }
        }
    }
}

@Composable
private fun SheetIcon(icon: ImageVector, label: String, modifier: Modifier, iconSize: Dp, onClick: () -> Unit) {
    Box(modifier.clip(RoundedCornerShape(10.dp)).clickable(onClick = onClick), contentAlignment = Alignment.Center) {
        // drawn in a padded frame, shown [iconSize] across
        Icon(icon, contentDescription = label, tint = CapsuleInk, modifier = Modifier.requiredSize(iconSize))
    }
}

@Composable
private fun Card(title: String, modifier: Modifier, content: @Composable ColumnScope.() -> Unit) {
    Column(modifier.fillMaxHeight().clip(RoundedCornerShape(bottomStart = 12.dp, bottomEnd = 12.dp)).background(CardFill)) {
        Text(title, color = Color.White, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 17.dp, top = 14.dp, bottom = 9.dp))
        content()
    }
}

/** A font or style: its name drawn in that face, the chosen one a blue cell. */
@Composable
private fun Cell(name: String, selected: Boolean, face: android.graphics.Typeface, onClick: () -> Unit) {
    val family = remember(face) { FontFamily(face) }
    Box(
        Modifier.fillMaxWidth().height(33.dp).background(if (selected) Pc.Accent else Color.Transparent).clickable(onClick = onClick).padding(start = 12.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        Text(name, color = Color.White, fontSize = 15.sp, fontFamily = family, maxLines = 1)
    }
}

/** An attribute's button: its icon, blue when on. */
@Composable
private fun Attribute(icon: ImageVector, label: String, on: Boolean, modifier: Modifier, iconSize: Dp, onClick: () -> Unit) {
    Box(
        modifier.height(37.dp).clip(RoundedCornerShape(8.dp)).background(if (on) Pc.Accent else ButtonFill).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, contentDescription = label, tint = Color.White, modifier = Modifier.requiredSize(iconSize))
    }
}

/** A design setting: its name, its value in a pill, and a thin bar under them (filled from the left, or a tick in the middle). */
@Composable
private fun DesignRow(label: String, value: Float, shown: String, fromMiddle: Boolean = true, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    Column(Modifier.fillMaxWidth().height(58.5.dp).padding(horizontal = 16.dp)) {
        Row(Modifier.fillMaxWidth().padding(top = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(label, color = Color.White, fontSize = 15.sp, modifier = Modifier.weight(1f))
            Box(Modifier.height(24.dp).clip(CircleShape).background(ButtonFill).padding(horizontal = 10.dp), contentAlignment = Alignment.Center) {
                Text(shown, color = Color.White, fontSize = 13.sp)
            }
        }
        Box(
            Modifier.fillMaxWidth().height(20.dp).pointerInput(Unit) {
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
                }
            }.drawBehind {
                val y = size.height / 2f
                val h = 2.dp.toPx()
                val v = value.coerceIn(0f, 1f)
                drawRoundRect(Color(0xFF5A5A5A), Offset(0f, y - h / 2f), Size(size.width, h), CornerRadius(h / 2f))
                if (fromMiddle) {
                    drawLine(Color(0xFF8A8A8A), Offset(size.width / 2f, y - 5.dp.toPx()), Offset(size.width / 2f, y + 5.dp.toPx()), 1.dp.toPx())
                } else {
                    drawRoundRect(Pc.Accent, Offset(0f, y - h / 2f), Size(size.width * v, h), CornerRadius(h / 2f))
                }
                val knob = Size(16.dp.toPx(), 10.dp.toPx())
                val x = (size.width * v).coerceIn(knob.width / 2f, size.width - knob.width / 2f)
                drawRoundRect(Color.White, Offset(x - knob.width / 2f, y - knob.height / 2f), knob, CornerRadius(knob.height / 2f))
            },
        )
    }
}

/** The size slider runs 1 to 2000 pixels, finer at the small end. */
private fun sliderToSize(v: Float) = 1f + 1999f * v * v

private fun sizeToSlider(size: Float) = kotlin.math.sqrt(((size - 1f) / 1999f).coerceIn(0f, 1f))
