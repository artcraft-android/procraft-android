package io.github.artcraftandroid.procraft.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
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
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.ModelLight
import kotlin.math.abs
import kotlin.math.roundToInt

private val StudioBar = Color(0xFF26292A)
private val StudioTitle = Color(0xFFD8D8D8)
private val PillFill = Color(0xFF1D2021)
private val PillEdge = Color(0xFF3B3E3F)
private val PillLit = Color(0xFF474747)
private val PillInk = Color(0xFFC1C1C1)
private val DoneBlue = Color(0xFF0190FF)
private val PopoverFill = Color(0xFF282A2D)
private val GroupFill = Color(0xFF232528)
private val RowLine = Color(0xFF2C2E31)
private val LabelInk = Color(0xFFB7B7B7)
private val DimInk = Color(0xFF8A8D8E)

/** How bright the light can be taken: Min to Max on the exposure slider. */
private const val MOST_EXPOSURE = 5f

/** The environments as the studio lists them: their names, assets and pictures. */
private class Environment(val label: String, val asset: String, val icon: ImageVector)

private val Environments by lazy {
    listOf(
        Environment("Studio", "studio", PcIcons.EnvStudio),
        Environment("Coast", "coast", PcIcons.EnvCoast),
        Environment("Auditorium", "auditorium", PcIcons.EnvAuditorium),
        Environment("City", "city", PcIcons.EnvCity),
        Environment("Nightlife", "nightlife", PcIcons.EnvNightlife),
        Environment("Portside", "portside", PcIcons.EnvPortside),
        Environment("Industrial", "industrial", PcIcons.EnvIndustrial),
        Environment("Sunrise", "sunrise", PcIcons.EnvSunrise),
        Environment("Beach", "beach", PcIcons.EnvBeach),
        Environment("Mountain", "mountain", PcIcons.EnvMountain),
        Environment("Daytime", "daytime", PcIcons.EnvDaytime),
    )
}

/** The hue bar's colours, evenly spaced along it (as the lights take them). */
private val HueStops = listOf(0xFFFF0000, 0xFFFF6E00, 0xFFFFDC00, 0xFF35FF00, 0xFF00FFEC, 0xFF0034FF, 0xFFAA00FF, 0xFFFF0000).map { Color(it) }
private val SaturationStart = Color(0xFF808080)
private val SettingsFill = Color(0xFF29292B)
private val SettingsGroup = Color(0xFF222427)

/** The hue bar's colour at [t] along it (0 to 1). */
private fun hueColor(t: Float): Color {
    val at = t.coerceIn(0f, 1f) * (HueStops.size - 1)
    val i = at.toInt().coerceAtMost(HueStops.size - 2)
    return androidx.compose.ui.graphics.lerp(HueStops[i], HueStops[i + 1], at - i)
}

/**
 * The Lighting Studio over a model: its bar (Cancel, Add Light, Environment and Done), the model
 * on its floor among its lights, seen through the studio's camera, which a finger takes round
 * the model and two fingers nearer; a light's cube is dragged across the view, and tapped, its
 * settings show. The Environment popover: whether the environment shows, the exposure, and which
 * environment lights the model.
 */
@Composable
fun LightingStudio(vm: CanvasViewModel) {
    if (!vm.lightingStudioOpen) return
    var environmentOpen by remember { mutableStateOf(false) }
    // the light whose settings are showing
    var lightOpen by remember { mutableStateOf<Int?>(null) }
    BackHandler {
        when {
            lightOpen != null -> lightOpen = null
            environmentOpen -> environmentOpen = false
            else -> vm.closeLightingStudio(keep = false)
        }
    }
    BoxWithConstraints(Modifier.fillMaxSize()) {
        // the view: a touch with a popover open only closes it
        Box(
            Modifier.fillMaxSize().pointerInput(Unit) {
                val slop = viewConfiguration.touchSlop
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    if (environmentOpen || lightOpen != null) {
                        environmentOpen = false
                        lightOpen = null
                        do {
                            val ev = awaitPointerEvent()
                            ev.changes.forEach { it.consume() }
                        } while (ev.changes.any { it.pressed })
                        return@awaitEachGesture
                    }
                    val grabbed = vm.lightAt(down.position.x, down.position.y)
                    var last = down.position
                    var travelled = 0f
                    var pinch: Float? = null
                    while (true) {
                        val ev = awaitPointerEvent()
                        val pressed = ev.changes.filter { it.pressed }
                        if (pressed.isEmpty()) break
                        if (pressed.size >= 2) {
                            // two fingers: nearer or farther, never a tap
                            val apart = (pressed[0].position - pressed[1].position).getDistance()
                            pinch?.let { if (it > 0f) vm.studioZoom(apart / it) }
                            pinch = apart
                            travelled = Float.MAX_VALUE
                        } else {
                            val c = ev.changes.firstOrNull { it.id == down.id && it.pressed } ?: pressed[0]
                            val (dx, dy) = c.position.x - last.x to c.position.y - last.y
                            last = c.position
                            pinch = null
                            // still for a tap until it goes past the slop
                            val before = travelled
                            travelled += abs(dx) + abs(dy)
                            if (travelled > slop && before != Float.MAX_VALUE) {
                                if (grabbed != null) vm.moveLight(grabbed, dx, dy) else vm.studioTurn(dx, dy)
                            }
                        }
                        ev.changes.forEach { it.consume() }
                    }
                    if (grabbed != null && travelled <= slop) lightOpen = grabbed
                }
            },
        )
        Box(Modifier.fillMaxWidth().height(85.dp).background(StudioBar).pointerInput(Unit) { detectTapGestures { environmentOpen = false } }) {
            Text("Lighting Studio", color = StudioTitle, fontSize = 20.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.offset(32.dp, 27.dp))
            Row(Modifier.align(Alignment.TopEnd).padding(top = 21.dp, end = 20.dp), verticalAlignment = Alignment.CenterVertically) {
                Row(Modifier.height(43.dp).clip(CircleShape).background(PillFill).border(1.dp, PillEdge, CircleShape)) {
                    PillButton("Cancel", 73.dp, lit = false) { vm.closeLightingStudio(keep = false) }
                    PillButton("Add Light", 92.dp, lit = false) {
                        environmentOpen = false
                        lightOpen = null
                        vm.addLight()
                    }
                    PillButton("Environment", 113.dp, lit = environmentOpen) { environmentOpen = !environmentOpen }
                }
                Spacer(Modifier.width(13.dp))
                Box(Modifier.size(44.dp, 45.dp).clip(CircleShape).background(DoneBlue).clickable { vm.closeLightingStudio(keep = true) }, contentAlignment = Alignment.Center) {
                    Icon(PcIcons.DoneTick, contentDescription = "Done", tint = Color.White, modifier = Modifier.requiredSize(40.dp))
                }
            }
        }
        if (environmentOpen) {
            EnvironmentPopover(vm, Modifier.align(Alignment.TopEnd).padding(top = 85.dp, end = 12.dp).heightIn(max = maxHeight - 97.dp))
        }
        lightOpen?.let { index ->
            val light = vm.modelLights.getOrNull(index)
            val at = vm.lightOnScreen(index)
            if (light == null || at == null || at.size < 3) {
                lightOpen = null
            } else {
                val density = androidx.compose.ui.platform.LocalDensity.current
                val (x, y, reach) = with(density) { Triple(at[0].toDp(), at[1].toDp(), at[2].toDp()) }
                LightSettings(vm, index, light, x, y, reach, maxWidth, onDone = { lightOpen = null })
            }
        }
    }
}

/** The popover's size: its body and the arrow under it. */
private val SettingsWidth = 280.dp
private val SettingsHeight = 347.dp

/**
 * Light Settings, pointing at the light from above it (below it when there's no room): its hue,
 * how much of that colour it has, how strong it is; Delete and Duplicate.
 */
@Composable
private fun LightSettings(
    vm: CanvasViewModel,
    index: Int,
    light: ModelLight,
    x: androidx.compose.ui.unit.Dp,
    y: androidx.compose.ui.unit.Dp,
    reach: androidx.compose.ui.unit.Dp,
    screenWidth: androidx.compose.ui.unit.Dp,
    onDone: () -> Unit,
) {
    val above = y - reach - Pc.ArrowHeight - SettingsHeight > 85.dp + 8.dp
    val left = (x - SettingsWidth / 2).coerceIn(12.dp, (screenWidth - SettingsWidth - 12.dp).coerceAtLeast(12.dp))
    val top = if (above) y - reach - Pc.ArrowHeight - SettingsHeight else y + reach
    Popover(
        SettingsWidth,
        (x - left).coerceIn(28.dp, SettingsWidth - 28.dp),
        Modifier.offset(left, top).pointerInput(Unit) { detectTapGestures { } },
        fill = SettingsFill,
        side = if (above) ArrowSide.BOTTOM else ArrowSide.TOP,
    ) {
        Text("Light Settings", color = Pc.PanelTitle, fontSize = 24.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.padding(start = 16.dp, top = 5.dp).height(40.dp))
        Column(Modifier.padding(horizontal = 10.dp)) {
            Spacer(Modifier.height(10.dp))
            Column(Modifier.clip(RoundedCornerShape(14.dp)).background(SettingsGroup).padding(vertical = 5.dp)) {
                val hue = hueColor(light.hue)
                val degrees = run {
                    val hsv = FloatArray(3)
                    android.graphics.Color.RGBToHSV((hue.red * 255).roundToInt(), (hue.green * 255).roundToInt(), (hue.blue * 255).roundToInt(), hsv)
                    if (light.hue >= 1f) 360 else hsv[0].roundToInt()
                }
                LightSlider(
                    "Hue",
                    if (light.saturation <= 0f) "None" else "$degrees°",
                    light.hue,
                    Brush.horizontalGradient(HueStops),
                    hue,
                ) { vm.changeLight(index, light.copy(hue = it)) }
                LightSlider(
                    "Saturation",
                    level(light.saturation),
                    light.saturation,
                    Brush.horizontalGradient(listOf(SaturationStart, hue)),
                    androidx.compose.ui.graphics.lerp(SaturationStart, hue, light.saturation),
                ) { vm.changeLight(index, light.copy(saturation = it)) }
                LightSlider(
                    "Intensity",
                    level(light.intensity),
                    light.intensity,
                    Brush.horizontalGradient(listOf(Color.Black, Color.White)),
                    Color(light.intensity, light.intensity, light.intensity),
                ) { vm.changeLight(index, light.copy(intensity = it)) }
            }
            Spacer(Modifier.height(10.dp))
            Column(Modifier.clip(RoundedCornerShape(14.dp)).background(SettingsGroup)) {
                Box(Modifier.fillMaxWidth().height(44.dp).clickable { vm.deleteLight(index); onDone() }.padding(start = 16.dp), contentAlignment = Alignment.CenterStart) {
                    Text("Delete", color = LabelInk, fontSize = 17.sp, maxLines = 1)
                }
                Box(Modifier.fillMaxWidth().height(1.dp).background(RowLine))
                Box(Modifier.fillMaxWidth().height(44.dp).clickable { vm.duplicateLight(index); onDone() }.padding(start = 16.dp), contentAlignment = Alignment.CenterStart) {
                    Text("Duplicate", color = LabelInk, fontSize = 17.sp, maxLines = 1)
                }
            }
            Spacer(Modifier.height(10.dp))
        }
    }
}

/** A share shown as the light's sliders show it: None, a percentage, or Max. */
private fun level(v: Float) = when {
    v <= 0f -> "None"
    v >= 1f -> "Max"
    else -> "${(v * 100f).roundToInt()}%"
}

/**
 * One of a light's settings, 56 tall: its name and value over a thin track drawn with [track],
 * whose capsule knob takes [knob], the track's colour where it is. Dragged by as far as the
 * finger goes.
 */
@Composable
private fun LightSlider(label: String, value: String, fraction: Float, track: Brush, knob: Color, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    val current by rememberUpdatedState(fraction)
    Box(Modifier.fillMaxWidth().height(56.dp)) {
        Text(label, color = DimInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.padding(start = 16.dp, top = 6.dp))
        Text(value, color = Color.White, fontSize = 17.sp, maxLines = 1, modifier = Modifier.align(Alignment.TopEnd).padding(end = 16.dp, top = 6.dp))
        Canvas(
            Modifier.fillMaxWidth().height(24.dp).align(Alignment.BottomStart).pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    val travel = size.width - 2 * 18.5.dp.toPx()
                    var last = down.position.x
                    while (true) {
                        val ev = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!ev.pressed) break
                        val moved = (ev.position.x - last) / travel
                        last = ev.position.x
                        change((current + moved).coerceIn(0f, 1f))
                        ev.consume()
                    }
                }
            },
        ) {
            val y = size.height / 2f
            val inset = 14.dp.toPx()
            val thick = 3.dp.toPx()
            drawRoundRect(track, Offset(inset, y - thick / 2f), Size(size.width - 2 * inset, thick), CornerRadius(thick / 2f))
            val ends = 18.5.dp.toPx()
            val x = ends + (size.width - 2 * ends) * fraction.coerceIn(0f, 1f)
            val (w, h) = 16.dp.toPx() to 10.dp.toPx()
            drawRoundRect(knob, Offset(x - w / 2f, y - h / 2f), Size(w, h), CornerRadius(h / 2f))
        }
    }
}

/** One of the bar's capsule buttons, lit while what it opened is showing. */
@Composable
private fun PillButton(label: String, width: androidx.compose.ui.unit.Dp, lit: Boolean, enabled: Boolean = true, onClick: () -> Unit) {
    Box(
        Modifier.width(width).height(43.dp).padding(2.dp).clip(CircleShape).background(if (lit) PillLit else Color.Transparent)
            .clickable(enabled = enabled, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = if (enabled) PillInk else PillInk.copy(alpha = 0.4f), fontSize = 17.sp, maxLines = 1)
    }
}

/** The Environment popover: Show Environment, Exposure, and the environments, the one lighting the model blue. */
@Composable
private fun EnvironmentPopover(vm: CanvasViewModel, modifier: Modifier) {
    Popover(350.dp, 0.dp, modifier.pointerInput(Unit) { detectTapGestures { } }, fill = PopoverFill, side = ArrowSide.NONE) {
        Text("Environment", color = Pc.PanelTitle, fontSize = 24.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.padding(start = 26.dp, top = 8.dp).height(40.dp))
        Column(Modifier.padding(horizontal = 10.dp)) {
            Spacer(Modifier.height(13.dp))
            Row(
                Modifier.fillMaxWidth().height(44.dp).clip(RoundedCornerShape(14.dp)).background(GroupFill)
                    .clickable { vm.changeEnvironmentShown(!vm.modelEnvironmentShown) }.padding(start = 16.dp, end = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Show Environment", color = LabelInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
                Switch(vm.modelEnvironmentShown, enabled = true)
            }
            Spacer(Modifier.height(10.dp))
            ExposureSlider(vm.modelExposure) { vm.changeExposure(it) }
            Spacer(Modifier.height(12.dp))
            Column(Modifier.clip(RoundedCornerShape(14.dp)).background(GroupFill).verticalScroll(rememberScrollState())) {
                Environments.forEachIndexed { i, env ->
                    if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(RowLine))
                    val chosen = env.asset == vm.modelEnvironment
                    Row(
                        Modifier.fillMaxWidth().height(44.dp).background(if (chosen) Pc.Accent else Color.Transparent)
                            .clickable { if (!chosen) vm.chooseEnvironment(env.asset) }.padding(start = 11.dp, end = 16.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(env.label, color = if (chosen) Color.White else LabelInk, fontSize = 16.sp, maxLines = 1, modifier = Modifier.weight(1f))
                        Icon(env.icon, contentDescription = null, tint = Color.White, modifier = Modifier.size(20.dp))
                    }
                }
            }
            Spacer(Modifier.height(12.dp))
        }
    }
}

/**
 * Exposure, from Min (none) to Max (5 times): its value on the right and a black-to-white track
 * whose knob is filled with the grey the exposure makes (white from 100% up). Dragging moves it
 * by as far as the finger goes.
 */
@Composable
private fun ExposureSlider(value: Float, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    val current by rememberUpdatedState(value)
    val shown = when {
        value <= 0f -> "Min"
        value >= MOST_EXPOSURE -> "Max"
        else -> "${(value * 100f).roundToInt()}%"
    }
    Box(Modifier.fillMaxWidth().height(84.dp).clip(RoundedCornerShape(14.dp)).background(GroupFill)) {
        Text("Exposure", color = DimInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.padding(start = 16.dp, top = 19.dp))
        Text(shown, color = Color.White, fontSize = 17.sp, maxLines = 1, modifier = Modifier.align(Alignment.TopEnd).padding(end = 16.dp, top = 19.dp))
        Canvas(
            Modifier.fillMaxWidth().height(30.dp).align(Alignment.BottomStart).offset(y = (-13).dp).pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    val travel = size.width - 2 * 18.5.dp.toPx()
                    var last = down.position.x
                    while (true) {
                        val ev = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!ev.pressed) break
                        val moved = (ev.position.x - last) / travel * MOST_EXPOSURE
                        last = ev.position.x
                        change((current + moved).coerceIn(0f, MOST_EXPOSURE))
                        ev.consume()
                    }
                }
            },
        ) {
            val y = size.height / 2f
            val inset = 16.dp.toPx()
            val track = 2.dp.toPx()
            drawRoundRect(
                Brush.horizontalGradient(listOf(Color.Black, Color.White), startX = inset, endX = size.width - inset),
                Offset(inset, y - track / 2f),
                Size(size.width - 2 * inset, track),
                CornerRadius(track / 2f),
            )
            val ends = 18.5.dp.toPx()
            val x = ends + (size.width - 2 * ends) * (value / MOST_EXPOSURE).coerceIn(0f, 1f)
            val (w, h) = 16.dp.toPx() to 9.dp.toPx()
            val grey = value.coerceIn(0f, 1f)
            drawRoundRect(Color(grey, grey, grey), Offset(x - w / 2f, y - h / 2f), Size(w, h), CornerRadius(h / 2f))
        }
    }
}
