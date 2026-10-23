package io.github.artcraftandroid.procraft.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.TimeLapseSettings
import io.github.artcraftandroid.procraft.gallery.CanvasPreset
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

private val ScreenFill = Color(0xFF1F1F1F)
private val PanelFill = Color(0xFF282828)
private val Ink = Color(0xFFAAAAAA)
private val IdleInk = Color(0xFF797979)
private val Editing = Color(0xFF436BF0)
private val KeypadFill = Color(0xFF1A1A1A)
private val DigitFill = Color(0xFF525252)
private val KeyFill = Color(0xFF2B2B2B)
private val UnitChosen = Color(0xFFA1A1A1)
private val SwitchTrack = Color(0xFF353535)
private val SwitchIdle = Color(0xFFC1C1C1)
private val CapsuleFill = Color(0xFF1A1A1A)
private val CapsuleEdge = Color(0xFF373737)
private val DoneBlue = Color(0xFF0190FF)

/** The units sizes are typed in: the key's name, the unit's (as presets keep it) and its mark. */
private enum class SizeUnit(val label: String, val key: String, val mark: String) {
    MILLIMETERS("Millimeters", "mm", "mm"), CENTIMETERS("Centimeters", "cm", "cm"), INCHES("Inches", "in", "″"), PIXELS("Pixels", "px", "px")
}

/** The fields the keypad types into. */
private enum class Field { WIDTH, HEIGHT, DPI }

/** A section of the rail: its name, and its icon shown [size] across. */
private class Section(val label: String, val icon: ImageVector, val size: Dp)

private val Sections = listOf(
    Section("Dimensions", PcIcons.InfoDimensions, 50.dp),
    Section("Color profile", PcIcons.InfoProfile, 35.dp),
    Section("Time-lapse settings", PcIcons.InfoVideo, 60.dp),
    Section("Canvas properties", PcIcons.TabCanvas, 40.dp),
)

/**
 * Custom Canvas, full screen: a panel of sections (dimensions, colour profile, time-lapse
 * settings, canvas properties) on the left, the chosen one on the right with the canvas's name,
 * Cancel and Done; sizes are typed on a keypad of its own, in pixels, inches, centimetres or
 * millimetres. It edits [CanvasViewModel.editingPreset], or makes a new canvas ([onMade] then).
 */
@Composable
fun CustomCanvasScreen(vm: CanvasViewModel, onMade: () -> Unit) {
    val start = remember { vm.editingPreset?.let { vm.canvasPresets.getOrNull(it) } ?: CanvasPreset("Untitled Canvas", "px", 1080f, 1080f, 300f) }
    var section by remember { mutableIntStateOf(0) }
    var name by remember { mutableStateOf(start.name) }
    // the size in the unit chosen
    var width by remember { mutableStateOf(start.sizeWidth) }
    var height by remember { mutableStateOf(start.sizeHeight) }
    var dpi by remember { mutableStateOf(start.dpi) }
    var unit by remember { mutableStateOf(SizeUnit.entries.firstOrNull { it.key == start.unit } ?: SizeUnit.PIXELS) }
    var field by remember { mutableStateOf<Field?>(Field.WIDTH) }
    // what has been typed into the field, until it is left
    var typed by remember { mutableStateOf("") }
    var background by remember { mutableIntStateOf(start.background) }
    var hidden by remember { mutableStateOf(start.backgroundHidden) }
    var resolution by remember { mutableIntStateOf(start.timeLapse.resolution) }
    var quality by remember { mutableIntStateOf(start.timeLapse.quality) }
    var hevc by remember { mutableStateOf(start.timeLapse.hevc) }
    var profile by remember { mutableStateOf(start.profile) }
    // which profiles the list shows
    var cmyk by remember { mutableStateOf(io.github.artcraftandroid.procraft.core.isCmykProfile(start.profile)) }
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    val importer = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let {
            scope.launch {
                vm.importProfile(it)?.let { name ->
                    profile = name
                    cmyk = io.github.artcraftandroid.procraft.core.isCmykProfile(name)
                }
            }
        }
    }
    // the background's colour picker is open
    var picking by remember { mutableStateOf(false) }

    fun preset() = CanvasPreset(name.trim().ifEmpty { "Untitled Canvas" }, unit.key, width, height, dpi, background, hidden, TimeLapseSettings(resolution, quality, hevc), profile)
    // what typing has made of the field so far goes into it
    fun commit() {
        val v = typed.toFloatOrNull()
        if (v != null && v > 0f) when (field) {
            Field.WIDTH -> width = v
            Field.HEIGHT -> height = v
            Field.DPI -> dpi = v.coerceIn(1f, 9999f)
            null -> Unit
        }
        typed = ""
    }
    // the size kept, in another unit (straight from one length to another; through the pixels
    // otherwise)
    fun convert(to: SizeUnit) {
        val p = preset()
        val from = CanvasPreset.perInch(unit.key)
        fun change(v: Float, px: Int) = CanvasPreset.perInch(to.key)?.let { if (from != null) v / from * it else px / dpi * it } ?: px.toFloat()
        width = change(width, p.width)
        height = change(height, p.height)
        unit = to
    }
    fun create() {
        commit()
        if (vm.finishCustomCanvas(preset())) onMade()
    }
    BackHandler { vm.closeCustomCanvas() }
    BoxWithConstraints(Modifier.fillMaxSize().background(ScreenFill).pointerInput(Unit) { detectTapGestures { } }) {
        // the sections
        Column(Modifier.padding(start = 10.dp, top = 10.dp, bottom = 10.dp).width(319.dp).fillMaxHeight().clip(RoundedCornerShape(20.dp)).background(PanelFill)) {
            Text("Custom Canvas", color = Ink, fontSize = 22.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 24.dp, top = 16.dp, bottom = 17.dp))
            Sections.forEachIndexed { i, s ->
                val chosen = i == section
                Row(
                    Modifier.padding(start = 8.dp, bottom = 10.dp).size(304.dp, 52.dp).clip(CircleShape).background(if (chosen) Pc.Accent else Color.Transparent).clickable {
                        commit()
                        section = i
                    }.padding(start = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    // icons centred 43 in, whatever their size; the names 69 in
                    Box(Modifier.width(38.dp), contentAlignment = Alignment.Center) {
                        Icon(s.icon, contentDescription = null, tint = if (chosen) Color.White else IdleInk, modifier = Modifier.requiredSize(s.size))
                    }
                    Text(s.label, color = if (chosen) Color.White else IdleInk, fontSize = 15.sp, modifier = Modifier.padding(start = 7.dp))
                }
            }
        }
        // the name, Cancel and Done
        BasicTextField(
            name, { name = it.take(64) }, singleLine = true,
            textStyle = TextStyle(color = Ink, fontSize = 22.sp, fontWeight = FontWeight.Bold), cursorBrush = SolidColor(Color.White),
            modifier = Modifier.offset(x = 394.dp, y = 28.dp).width(maxWidth - 394.dp - 160.dp),
        )
        Row(Modifier.align(Alignment.TopEnd).padding(top = 20.dp, end = 20.dp), verticalAlignment = Alignment.CenterVertically) {
            // Cancel, and Import beside it on the colour profile page
            Row(Modifier.height(43.dp).clip(CircleShape).background(CapsuleFill).border(1.dp, CapsuleEdge, CircleShape), verticalAlignment = Alignment.CenterVertically) {
                CapsuleText("Cancel") { vm.closeCustomCanvas() }
                if (section == 1) CapsuleText("Import") { importer.launch(arrayOf("*/*")) }
            }
            Spacer(Modifier.width(13.dp))
            Box(Modifier.size(44.dp, 45.dp).clip(CircleShape).background(DoneBlue).clickable { create() }, contentAlignment = Alignment.Center) {
                Icon(PcIcons.DoneTick, contentDescription = "Create", tint = Color.White, modifier = Modifier.requiredSize(40.dp))
            }
        }
        Column(Modifier.padding(start = 372.dp, end = 32.dp, top = 120.dp)) {
            when (section) {
                0 -> {
                    Group {
                        SizeRow("Width", if (field == Field.WIDTH && typed.isNotEmpty()) typed else CanvasPreset.number(width), unit.mark, field == Field.WIDTH) { commit(); field = Field.WIDTH }
                        Line()
                        SizeRow("Height", if (field == Field.HEIGHT && typed.isNotEmpty()) typed else CanvasPreset.number(height), unit.mark, field == Field.HEIGHT) { commit(); field = Field.HEIGHT }
                    }
                    Spacer(Modifier.height(24.dp))
                    Group {
                        SizeRow("DPI", if (field == Field.DPI && typed.isNotEmpty()) typed else "${dpi.roundToInt()}", "", field == Field.DPI) { commit(); field = Field.DPI }
                        Line()
                        val p = preset()
                        SizeRow("Maximum Layers", "${vm.layersFor(p.width, p.height)}", "", false) {}
                    }
                }
                1 -> {
                    KindSwitch(cmyk) { cmyk = it }
                    Spacer(Modifier.height(24.dp))
                    Group {
                        val names = if (cmyk) io.github.artcraftandroid.procraft.core.cmykProfiles() else io.github.artcraftandroid.procraft.core.colorProfiles()
                        for ((i, name) in names.withIndex()) {
                            if (i > 0) Line()
                            ChoiceRow(name, name == profile) { profile = name }
                        }
                    }
                }
                2 -> {
                    Group {
                        for ((i, pair) in listOf("1080p" to "1920 × 1080px", "2K" to "2732 × 2048px", "4K" to "3840 × 2160px").withIndex()) {
                            if (i > 0) Line()
                            ChoiceRow(pair.first, i == resolution, pair.second) { resolution = i }
                        }
                    }
                    Spacer(Modifier.height(24.dp))
                    Group {
                        for ((i, label) in listOf("Low quality", "Good quality", "Studio quality", "Lossless").withIndex()) {
                            if (i > 0) Line()
                            ChoiceRow(label, i == quality, tick = true) { quality = i }
                        }
                    }
                    Spacer(Modifier.height(24.dp))
                    Group { SwitchLine("HEVC", hevc) { hevc = it } }
                }
                else -> {
                    Group {
                        Row(
                            Modifier.fillMaxWidth().height(47.dp).clickable { picking = true }.padding(horizontal = 16.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text("Background color", color = Ink, fontSize = 15.sp, modifier = Modifier.weight(1f))
                            Box(Modifier.size(24.dp).clip(CircleShape).background(Color(0xFF000000 or background.toLong())).border(1.dp, Color(0xFF555555), CircleShape))
                        }
                    }
                    Spacer(Modifier.height(24.dp))
                    Group { SwitchLine("Background hidden", hidden) { hidden = it } }
                }
            }
        }
        // the keypad, while a size is typed
        if (section == 0 && field != null) {
            Keypad(
                Modifier.align(Alignment.BottomCenter),
                unit,
                onUnit = {
                    commit()
                    convert(it)
                },
                onKey = { k ->
                    when (k) {
                        "⌫" -> typed = typed.dropLast(1)
                        "." -> if ('.' !in typed && field != Field.DPI && unit != SizeUnit.PIXELS) typed += "."
                        else -> if (typed.length < 7) typed += k
                    }
                },
                onNext = {
                    commit()
                    field = when (field) { Field.WIDTH -> Field.HEIGHT; Field.HEIGHT -> Field.DPI; else -> Field.WIDTH }
                },
                onDone = {
                    commit()
                    field = null
                },
            )
        }
        // the background's colour, in a compact picker hanging from its swatch (60 in from the
        // right, under the row) at the screen's edge
        if (picking && section == 3) {
            Box(Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures { picking = false } })
            val pick = remember { HeldColor(background) { background = it } }
            CompactColors(vm, pick, arrowX = 200.dp, modifier = Modifier.offset(x = maxWidth - 260.dp, y = 168.dp))
        }
    }
}

/** One of the header capsule's words, a button. */
@Composable
private fun CapsuleText(label: String, onClick: () -> Unit) {
    Box(Modifier.fillMaxHeight().clickable(onClick = onClick).padding(horizontal = 13.dp), contentAlignment = Alignment.Center) {
        Text(label, color = Color(0xFFA5A5A5), fontSize = 17.sp)
    }
}

/** RGB or CMYK: which profiles the colour profile page lists, as wide as the list, the chosen half blue. */
@Composable
private fun KindSwitch(cmyk: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().height(34.dp).clip(CircleShape).background(SwitchTrack).padding(2.dp)) {
        for ((i, label) in listOf("RGB", "CMYK").withIndex()) {
            val on = cmyk == (i == 1)
            val ink = if (on) Color.White else SwitchIdle
            Row(
                Modifier.weight(1f).fillMaxHeight().clip(CircleShape).background(if (on) Pc.Accent else Color.Transparent).clickable { onChange(i == 1) },
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(if (i == 0) PcIcons.ProfileRgb else PcIcons.ProfileCmyk, contentDescription = null, tint = ink, modifier = Modifier.size(20.dp))
                Text(label, color = ink, fontSize = 15.sp, modifier = Modifier.padding(start = 8.dp))
            }
        }
    }
}

@Composable
private fun Group(content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(PanelFill), content = content)
}

@Composable
private fun Line() {
    Box(Modifier.fillMaxWidth().height(1.dp).background(ScreenFill))
}

@Composable
private fun SizeRow(label: String, value: String, unit: String, editing: Boolean, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().height(47.dp).clickable(onClick = onClick).padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = Ink, fontSize = 15.sp, modifier = Modifier.weight(1f))
        Text(value, color = if (editing) Editing else Ink, fontSize = 15.sp)
        if (unit.isNotEmpty()) Text(" $unit", color = Ink, fontSize = 15.sp)
    }
}

@Composable
private fun ChoiceRow(label: String, chosen: Boolean, detail: String? = null, tick: Boolean = false, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().height(47.dp).background(if (chosen) Pc.Accent else Color.Transparent).clickable(onClick = onClick).padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = if (chosen) Color.White else Ink, fontSize = 15.sp, modifier = Modifier.weight(1f))
        if (detail != null) Text(detail, color = if (chosen) Color.White else Ink, fontSize = 15.sp)
        if (tick && chosen) Icon(PcIcons.MenuTick, contentDescription = null, tint = Color.White, modifier = Modifier.size(20.dp))
    }
}

@Composable
private fun SwitchLine(label: String, on: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().height(47.dp).clickable { onChange(!on) }.padding(start = 16.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = Ink, fontSize = 15.sp, modifier = Modifier.weight(1f))
        Switch(on, enabled = true)
    }
}

/** The keypad: units on the left, the digits, and Backspace, Next and Done on the right. */
@Composable
private fun Keypad(modifier: Modifier, unit: SizeUnit, onUnit: (SizeUnit) -> Unit, onKey: (String) -> Unit, onNext: () -> Unit, onDone: () -> Unit) {
    Box(modifier.fillMaxWidth().height(342.dp).background(KeypadFill), contentAlignment = Alignment.Center) {
        Row(horizontalArrangement = Arrangement.spacedBy(30.dp)) {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                for (u in listOf(SizeUnit.MILLIMETERS, SizeUnit.CENTIMETERS, SizeUnit.INCHES, SizeUnit.PIXELS)) {
                    Key(u.label, if (u == unit) UnitChosen else KeyFill, if (u == unit) Color(0xFF1F1F1F) else Ink, 15.sp) { onUnit(u) }
                }
            }
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                for (row in listOf(listOf("7", "8", "9"), listOf("4", "5", "6"), listOf("1", "2", "3"))) {
                    Row(horizontalArrangement = Arrangement.spacedBy(9.dp)) { for (k in row) Key(k, DigitFill, Ink, 26.sp) { onKey(k) } }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(9.dp)) {
                    Key("0", DigitFill, Ink, 26.sp, width = 249) { onKey("0") }
                    Key(".", DigitFill, Ink, 26.sp) { onKey(".") }
                }
            }
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Key("Backspace", KeyFill, Ink, 15.sp) { onKey("⌫") }
                Key("Next", KeyFill, Ink, 15.sp) { onNext() }
                Key("Done", KeyFill, Ink, 15.sp, height = 150) { onDone() }
            }
        }
    }
}

@Composable
private fun Key(label: String, fill: Color, ink: Color, size: androidx.compose.ui.unit.TextUnit, width: Int = 120, height: Int = 71, onClick: () -> Unit) {
    Box(Modifier.size(width.dp, height.dp).clip(RoundedCornerShape(8.dp)).background(fill).clickable(onClick = onClick), contentAlignment = Alignment.Center) {
        Text(label, color = ink, fontSize = size)
    }
}
