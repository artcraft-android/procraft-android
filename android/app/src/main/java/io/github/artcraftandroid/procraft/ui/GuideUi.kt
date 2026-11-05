package io.github.artcraftandroid.procraft.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.GuideType
import io.github.artcraftandroid.procraft.core.GuideUnit
import io.github.artcraftandroid.procraft.core.SymmetryType
import kotlin.math.roundToInt
import kotlin.math.sqrt

private val GuideTop get() = if (Pc.light) Color(0xFFE1E1E3) else Color(0xFF1F1F1F)
private val GuideButton get() = if (Pc.light) Color(0xFFF5F8F9) else Color(0xFF1C1C1C)
private val GuideButtonEdge get() = if (Pc.light) Color(0xFFDCDEDF) else Color(0xFF383838)
private val GuideTitle get() = if (Pc.light) Color(0xFF3A3A3A) else Color(0xFFD8D8D8)
private val GuideDone = Color(0xFF0190FF)
private val GuideNote get() = if (Pc.light) Color(0xFFB5B5B5) else Color(0xFF4A4A4A)
/** The toolbar's words: light, or dark grey with the light interface. */
private val GuideLabel get() = if (Pc.light) Color(0xFF414141) else Color(0xFFD0D0D0)
private val GuideNodeCentre = Color(0xFF0A84FF)
private val GuideNodeTurn = Color(0xFF30D158)

/**
 * The grid size's keypad, frosted as measured: the sheet and its keys let a little of what is
 * under them through (dark over the dark editor: the sheet #1B1B1B, the side keys #2B2B2B, the
 * digits #525252; over the white canvas #2E2E30, #38383A, #59595B). Light, as Custom Canvas's.
 */
private val SheetFill get() = if (Pc.light) Color(0xFFD1D3D8) else Color(0xE91A1A1A)
private val SheetKey get() = if (Pc.light) Color(0xFFABB2BB) else Color(0x514E4E4E)
private val SheetDigit get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0x9A757575)
private val SheetUnit get() = if (Pc.light) Color(0xFF818C96) else Color(0xFFA0A0A1)
private val SheetUnitInk get() = if (Pc.light) Color.White else Color(0xFF141414)
private val SheetInk get() = if (Pc.light) Color(0xFF3E3E3E) else Color(0xFFAAAAAA)

/** The keypad's keys, 79.5 tall, 9 apart; 30 between the units, the digits and the right-hand keys. */
private val KeyHeight = 79.5.dp
private val KeyGap = 9.dp
private val KeyGroups = 30.dp
/** The sheet: the keys from 51 below its top to 38 above the screen's foot. */
private val SheetHeight = 51.dp + KeyHeight * 4 + KeyGap * 3 + 38.dp

private val KEYPAD_UNITS = listOf(
    GuideUnit.MILLIMETERS to "Millimeters",
    GuideUnit.CENTIMETERS to "Centimeters",
    GuideUnit.INCHES to "Inches",
    GuideUnit.PIXELS to "Pixels",
)

/** The grid size's keypad while it is up: what is typed (the size as it was, to start with) and in which unit. */
private data class SizeKeypad(val typed: String, val unit: GuideUnit)

/** The colour bar's stops, as the engine's (dark through blue, green, orange, red, pink, white). */
private val BAR = listOf(
    0xFF05040A, 0xFF010C2C, 0xFF001857, 0xFF083984, 0xFF128BB6, 0xFF19C6CB, 0xFF1BD0B8, 0xFF2FD083, 0xFF67C064, 0xFFBC9B4C, 0xFFCE9049,
    0xFFD97644, 0xFFDA574A, 0xFFDC414D, 0xFFE53D71, 0xFFEC36AB, 0xFFF534DE, 0xFFF55CF9, 0xFFFB92FD, 0xFFFCCAFF, 0xFFE8E1E9,
).map { Color(it) }

/** A grid size slider's position for a size, and back (canvas pixels, 10 to 500). */
private fun sizeToSlider(px: Float) = sqrt(((px - 10f) / 490f).coerceIn(0f, 1f))
private fun sliderToSize(v: Float) = 10f + v * v * 490f

/** The drawing guide's lines over the canvas while it is on (and its handles while editing). */
@Composable
fun GuideOverlay(vm: CanvasViewModel) {
    val g = vm.guide ?: return
    if (!g.on && !vm.guideEditing) return
    val density = androidx.compose.ui.platform.LocalDensity.current.density
    val color = Color(g.color.getOrElse(0) { 0f }, g.color.getOrElse(1) { 0f }, g.color.getOrElse(2) { 0f }, g.opacity)
    Canvas(Modifier.fillMaxSize()) {
        val width = (0.25f + 1.5f * g.thickness) * density
        val l = g.lines
        for (i in 0 until l.size / 4) {
            drawLine(color, Offset(l[i * 4], l[i * 4 + 1]), Offset(l[i * 4 + 2], l[i * 4 + 3]), width)
        }
        if (vm.guideEditing) {
            fun node(at: Offset, fill: Color) {
                drawCircle(Color.White, 7.dp.toPx(), at)
                drawCircle(fill, 5.dp.toPx(), at)
            }
            if (g.kind == GuideType.PERSPECTIVE) {
                for (i in 0 until g.vanishing.size / 2) node(Offset(g.vanishing[i * 2], g.vanishing[i * 2 + 1]), GuideNodeCentre)
            } else {
                node(Offset(g.handle[0], g.handle[1]), GuideNodeTurn)
                node(Offset(g.centre[0], g.centre[1]), GuideNodeCentre)
            }
        }
    }
}

/**
 * The guide editor over the canvas: its title with Cancel and Done, the colour bar, the canvas
 * fitted between them and the toolbar (kinds, look and assisted drawing), the guide's handles
 * dragged on the canvas, and the grid size typed on a keypad from its chip.
 */
@Composable
fun GuideEditor(vm: CanvasViewModel) {
    if (!vm.guideEditing) return
    val g = vm.guide ?: return
    var keypad by remember { mutableStateOf<SizeKeypad?>(null) }
    // up while the kind has a grid
    val open = keypad?.takeIf { g.kind == GuideType.GRID || g.kind == GuideType.ISOMETRIC }
    BackHandler(open != null) { keypad = null }
    BoxWithConstraints(Modifier.fillMaxSize()) {
        // the canvas: handles dragged, perspective points added; nothing paints
        Box(Modifier.fillMaxSize().pointerInput(Unit) {
            awaitEachGesture {
                val down = awaitFirstDown()
                down.consume()
                val grab = vm.guideGrab(down.position)
                while (true) {
                    val ev = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                    if (!ev.pressed) break
                    vm.guideDrag(grab, ev.position)
                    ev.consume()
                }
            }
        })
        Box(Modifier.fillMaxWidth().height(82.dp).background(GuideTop)) {
            Text("Drawing Guides", color = GuideTitle, fontSize = 20.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.offset(32.dp, 27.dp))
            Row(Modifier.align(Alignment.TopEnd).padding(top = 20.dp, end = 20.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(73.dp, 43.dp).clip(CircleShape).background(GuideButton).border(1.dp, GuideButtonEdge, CircleShape).tap { vm.closeGuideEditor(keep = false) },
                    contentAlignment = Alignment.Center) {
                    Text("Cancel", color = if (Pc.light) Color(0xFF585858) else GuideTitle, fontSize = 17.sp, maxLines = 1)
                }
                Spacer(Modifier.width(13.dp))
                Box(Modifier.size(44.dp).clip(CircleShape).background(GuideDone).tap { vm.closeGuideEditor(keep = true) }, contentAlignment = Alignment.Center) {
                    Icon(PcIcons.DoneTick, contentDescription = "Done", tint = Color.White, modifier = Modifier.requiredSize(40.dp))
                }
            }
        }
        ColorBar(g.hue, Modifier.offset(y = 84.dp)) { vm.setGuideLook(hue = it) }
        if (g.kind == GuideType.PERSPECTIVE && g.vanishing.isEmpty()) {
            Box(Modifier.offset(y = 94.dp).fillMaxWidth().height(24.dp).background(GuideNote), contentAlignment = Alignment.Center) {
                Text("Tap to create vanishing points", color = GuideTitle, fontSize = 12.sp, maxLines = 1)
            }
        }
        GuideToolbar(vm, Modifier.align(Alignment.BottomCenter)) { keypad = SizeKeypad(g.sizeText, g.unit) }
        SymmetryOptions(vm, maxWidth, maxHeight)
        open?.let { k ->
            // each key changes the size at once; a unit chosen clears what was typed
            fun type(typed: String) {
                keypad = k.copy(typed = typed)
                vm.setGuideLength(typed, k.unit)
            }
            GridSizeKeypad(
                k, maxWidth, Modifier.align(Alignment.BottomCenter),
                onUnit = {
                    keypad = SizeKeypad("", it)
                    vm.setGuideLength("", it)
                },
                onKey = { key ->
                    val t = k.typed
                    when {
                        key == '⌫' -> if (t.isNotEmpty()) type(t.dropLast(1))
                        key == '.' -> if ('.' !in t && t.length < 7) type("$t.")
                        t.length < 7 -> type(t + key)
                    }
                },
                onDone = { keypad = null },
            )
        }
    }
}

/**
 * The grid size's keypad, as measured: a sheet over the editor's foot, the whole width, rounded 30
 * at its top; "Length" and what is typed in a 250 wide title centred 24 below its top; then the
 * units, the digits (three across, 0 two wide) and Backspace over Done, in keys 150 x 79.5 from 51
 * below its top. Where the screen is too narrow for them, the keys are narrower.
 */
@Composable
private fun GridSizeKeypad(k: SizeKeypad, screenWidth: Dp, modifier: Modifier, onUnit: (GuideUnit) -> Unit, onKey: (Char) -> Unit, onDone: () -> Unit) {
    val w = ((screenWidth - 16.dp - KeyGroups * 2 - KeyGap * 2) / 5).coerceIn(40.dp, 150.dp)
    Box(
        modifier.fillMaxWidth().height(SheetHeight).clip(RoundedCornerShape(topStart = 30.dp, topEnd = 30.dp)).background(SheetFill)
            .pointerInput(Unit) { detectTapGestures { } },
    ) {
        Box(Modifier.align(Alignment.TopCenter).padding(top = 8.dp).size(250.dp, 32.dp)) {
            Text("Length", color = SheetInk, fontSize = 12.sp, maxLines = 1, modifier = Modifier.align(Alignment.CenterStart).padding(start = 16.dp))
            Text(k.typed, color = SheetInk, fontSize = 12.sp, maxLines = 1, modifier = Modifier.align(Alignment.CenterStart).padding(start = 160.dp))
        }
        Row(Modifier.align(Alignment.TopCenter).padding(top = 51.dp), horizontalArrangement = Arrangement.spacedBy(KeyGroups)) {
            Column(verticalArrangement = Arrangement.spacedBy(KeyGap)) {
                for ((unit, label) in KEYPAD_UNITS) {
                    val chosen = unit == k.unit
                    PadKey(label, if (chosen) SheetUnit else SheetKey, if (chosen) SheetUnitInk else SheetInk, 16.sp, w, KeyHeight) { onUnit(unit) }
                }
            }
            Column(verticalArrangement = Arrangement.spacedBy(KeyGap)) {
                for (row in listOf("789", "456", "123")) {
                    Row(horizontalArrangement = Arrangement.spacedBy(KeyGap)) {
                        for (d in row) PadKey(d.toString(), SheetDigit, SheetInk, 23.sp, w, KeyHeight) { onKey(d) }
                    }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(KeyGap)) {
                    PadKey("0", SheetDigit, SheetInk, 23.sp, w * 2 + KeyGap, KeyHeight) { onKey('0') }
                    PadKey(".", SheetDigit, SheetInk, 23.sp, w, KeyHeight) { onKey('.') }
                }
            }
            Column(verticalArrangement = Arrangement.spacedBy(KeyGap)) {
                PadKey("Backspace", SheetKey, SheetInk, 16.sp, w, KeyHeight) { onKey('⌫') }
                PadKey("Done", SheetKey, SheetInk, 16.sp, w, KeyHeight * 3 + KeyGap * 2) { onDone() }
            }
        }
    }
}

@Composable
private fun PadKey(label: String, fill: Color, ink: Color, size: TextUnit, width: Dp, height: Dp, onClick: () -> Unit) {
    Box(Modifier.size(width, height).clip(RoundedCornerShape(8.dp)).background(fill).tap(onClick), contentAlignment = Alignment.Center) {
        Text(label, color = ink, fontSize = size, maxLines = 1)
    }
}

/** The colour bar: the guide's colour picked by dragging along it. */
@Composable
private fun ColorBar(hue: Float, modifier: Modifier, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    Canvas(
        modifier.fillMaxWidth().height(10.dp).pointerInput(Unit) {
            awaitEachGesture {
                val down = awaitFirstDown()
                down.consume()
                change((down.position.x / size.width).coerceIn(0f, 1f))
                while (true) {
                    val ev = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                    if (!ev.pressed) break
                    change((ev.position.x / size.width).coerceIn(0f, 1f))
                    ev.consume()
                }
            }
        },
    ) {
        val y = size.height / 2f
        val bar = 5.5.dp.toPx()
        drawRect(Brush.horizontalGradient(BAR), Offset(0f, y - bar / 2f), androidx.compose.ui.geometry.Size(size.width, bar))
        val x = size.width * hue.coerceIn(0f, 1f)
        drawCircle(Color.White, 7.dp.toPx(), Offset(x.coerceIn(7.dp.toPx(), size.width - 7.dp.toPx()), y))
        drawCircle(BAR[(hue * (BAR.size - 1)).roundToInt().coerceIn(0, BAR.size - 1)], 5.5.dp.toPx(), Offset(x.coerceIn(7.dp.toPx(), size.width - 7.dp.toPx()), y))
    }
}

private val GUIDE_KINDS = listOf(
    Triple(GuideType.GRID, "2D Grid", 114.dp),
    Triple(GuideType.ISOMETRIC, "Isometric", 123.dp),
    Triple(GuideType.PERSPECTIVE, "Perspective", 138.dp),
    Triple(GuideType.SYMMETRY, "Symmetry", 129.dp),
)

private fun GuideType.icon() = when (this) {
    GuideType.GRID -> PcIcons.GuideGrid
    GuideType.ISOMETRIC -> PcIcons.GuideIsometric
    GuideType.PERSPECTIVE -> PcIcons.ChromaPerspective
    GuideType.SYMMETRY -> PcIcons.GuideSymmetry
}

/**
 * The guide editor's toolbar: the four kinds, then the kind's look and assisted drawing (on a wide
 * screen the whole width less 8 each side, the sliders in one row and the switch at the end).
 */
@Composable
private fun GuideToolbar(vm: CanvasViewModel, modifier: Modifier, onChip: () -> Unit) {
    val g = vm.guide ?: return
    val screen = androidx.compose.ui.platform.LocalConfiguration.current
    if (screen.screenWidthDp > screen.screenHeightDp) {
        val slider = guideSlider(screen.screenWidthDp.dp)
        OptionsPanel(screen.screenWidthDp.dp - 16.dp, 137.dp, modifier, 16.dp) {
            KindsSwitch(546.dp, GUIDE_KINDS.map { (k, label, w) -> Kind(k.icon(), label, w) }, GUIDE_KINDS.indexOfFirst { it.first == g.kind }) {
                vm.chooseGuideKind(GUIDE_KINDS[it].first)
            }
            Spacer(Modifier.height(16.dp))
            Row(Modifier.fillMaxWidth().height(54.dp).padding(start = 16.dp, end = 22.dp), horizontalArrangement = Arrangement.spacedBy(19.dp), verticalAlignment = Alignment.CenterVertically) {
                OptionSlider("Opacity", g.opacity, shown(g.opacity, max = true), slider) { vm.setGuideLook(opacity = it) }
                OptionSlider("Thickness", g.thickness, shown(g.thickness, max = true), slider) { vm.setGuideLook(thickness = it) }
                when (g.kind) {
                    GuideType.GRID, GuideType.ISOMETRIC ->
                        OptionSlider("Grid size", sizeToSlider(g.size), g.sizeLabel, slider, chip = true, onChip = onChip) { vm.setGuideLook(size = sliderToSize(it)) }
                    GuideType.SYMMETRY -> Box(
                        Modifier.size(slider, 40.dp).clip(RoundedCornerShape(20.dp)).background(Color(0xFF303030)).tap { vm.symmetryOptionsOpen = !vm.symmetryOptionsOpen },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text("Options", color = GuideLabel, fontSize = 15.sp, maxLines = 1)
                    }
                    GuideType.PERSPECTIVE -> Unit
                }
                Spacer(Modifier.weight(1f))
                Text("Assisted Drawing", color = GuideLabel, fontSize = 15.sp, maxLines = 1)
                Box(Modifier.tap { vm.setGuideAssisted(!g.assisted) }) { Switch(g.assisted, enabled = true) }
            }
        }
        return
    }
    OptionsPanel(652.dp, 199.dp, modifier, 16.dp) {
        KindsSwitch(546.dp, GUIDE_KINDS.map { (k, label, w) -> Kind(k.icon(), label, w) }, GUIDE_KINDS.indexOfFirst { it.first == g.kind }) {
            vm.chooseGuideKind(GUIDE_KINDS[it].first)
        }
        Spacer(Modifier.height(14.dp))
        Row(Modifier.width(620.dp), horizontalArrangement = Arrangement.spacedBy(20.dp)) {
            OptionSlider("Opacity", g.opacity, shown(g.opacity, max = true), 300.dp) { vm.setGuideLook(opacity = it) }
            OptionSlider("Thickness", g.thickness, shown(g.thickness, max = true), 300.dp) { vm.setGuideLook(thickness = it) }
        }
        Spacer(Modifier.height(8.dp))
        Row(Modifier.width(620.dp).height(54.dp), horizontalArrangement = Arrangement.spacedBy(20.dp), verticalAlignment = Alignment.CenterVertically) {
            when (g.kind) {
                GuideType.GRID, GuideType.ISOMETRIC -> {
                    OptionSlider("Grid size", sizeToSlider(g.size), g.sizeLabel, 300.dp, chip = true, onChip = onChip) { vm.setGuideLook(size = sliderToSize(it)) }
                    AssistedSwitch(vm, g.assisted)
                }
                GuideType.PERSPECTIVE -> AssistedSwitch(vm, g.assisted)
                GuideType.SYMMETRY -> Box(
                    Modifier.size(300.dp, 40.dp).clip(RoundedCornerShape(20.dp)).background(Color(0xFF303030)).tap { vm.symmetryOptionsOpen = !vm.symmetryOptionsOpen },
                    contentAlignment = Alignment.Center,
                ) {
                    Text("Options", color = GuideLabel, fontSize = 15.sp, maxLines = 1)
                }
            }
        }
    }
}

/** A slider's width in the wide guide toolbar: 257, less what a narrower screen would take from its switch. */
private fun guideSlider(screenWidth: androidx.compose.ui.unit.Dp) = minOf(257.dp, (screenWidth - 328.dp) / 3)

@Composable
private fun AssistedSwitch(vm: CanvasViewModel, on: Boolean) {
    Row(Modifier.width(300.dp).padding(start = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Text("Assisted Drawing", color = GuideLabel, fontSize = 15.sp, maxLines = 1, modifier = Modifier.weight(1f))
        Box(Modifier.tap { vm.setGuideAssisted(!on) }) { Switch(on, enabled = true) }
    }
}

/** Symmetry's options: vertical, horizontal, quadrant or radial; turned instead of mirrored; assisted. */
@Composable
private fun SymmetryOptions(vm: CanvasViewModel, screenWidth: androidx.compose.ui.unit.Dp, screenHeight: androidx.compose.ui.unit.Dp) {
    val g = vm.guide ?: return
    if (!vm.symmetryOptionsOpen || g.kind != GuideType.SYMMETRY) return
    // over the Options button, its arrow at the button's middle: the third of the row when wide
    val wide = screenWidth > screenHeight
    val slider = guideSlider(screenWidth)
    val buttonMiddle = if (wide) 8.dp + 16.dp + (slider + 19.dp) * 2 + slider / 2 else (screenWidth - 652.dp) / 2 + 16.dp + 150.dp
    val left = (buttonMiddle - 150.dp).coerceIn(0.dp, screenWidth - 300.dp)
    MenuPopover(buttonMiddle - left, left, if (wide) screenHeight - 89.dp else screenHeight - 98.dp) {
        MenuTitle("Guide Options")
        Spacer(Modifier.height(10.dp))
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(13.dp)).background(MenuGroup)) {
            listOf(SymmetryType.VERTICAL to "Vertical", SymmetryType.HORIZONTAL to "Horizontal", SymmetryType.QUADRANT to "Quadrant", SymmetryType.RADIAL to "Radial")
                .forEachIndexed { i, (kind, label) ->
                    if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(MenuLine))
                    val chosen = g.symmetry == kind
                    Row(
                        Modifier.fillMaxWidth().height(44.dp).background(if (chosen) Pc.Accent else Color.Transparent).tap { vm.setGuideSymmetry(kind, g.rotational) }
                            .padding(start = 16.dp, end = 14.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        MenuLabel(label, if (chosen) Color.White else MenuText, Modifier.weight(1f))
                        if (chosen) Icon(PcIcons.MenuTick, contentDescription = null, tint = Color.White, modifier = Modifier.size(29.dp))
                    }
                }
        }
        Spacer(Modifier.height(14.dp))
        Row(Modifier.fillMaxWidth().height(40.dp), verticalAlignment = Alignment.CenterVertically) {
            MenuLabel("Rotational Symmetry", MenuText, Modifier.weight(1f))
            Box(Modifier.tap { vm.setGuideSymmetry(g.symmetry, !g.rotational) }) { Switch(g.rotational, enabled = true) }
        }
        Row(Modifier.fillMaxWidth().height(40.dp), verticalAlignment = Alignment.CenterVertically) {
            MenuLabel("Assisted Drawing", MenuText, Modifier.weight(1f))
            Box(Modifier.tap { vm.setGuideAssisted(!g.assisted) }) { Switch(g.assisted, enabled = true) }
        }
    }
}
