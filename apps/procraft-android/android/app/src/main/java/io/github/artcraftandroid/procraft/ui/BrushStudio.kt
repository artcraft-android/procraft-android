package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import android.content.res.Configuration
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.PadSurfaceView
import io.github.artcraftandroid.procraft.core.SourceKind
import io.github.artcraftandroid.procraft.core.curveSamples
import io.github.artcraftandroid.procraft.core.sliderExtras
import kotlin.math.pow
import kotlin.math.roundToInt
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.ui.platform.LocalDensity

private val StudioTabs get() = if (Pc.light) Color(0xFFEBEBEB) else Color(0xFF272727)
private val StudioBack get() = if (Pc.light) Color(0xFFF3F3F3) else Color(0xFF1F1F1F)
private val StudioLine get() = if (Pc.light) Color(0xFFCFCFCF) else Color(0xFF303030)
private val StudioHeader get() = if (Pc.light) Color(0xFF818181) else Color(0xFF9C9C9C)
private val StudioLabel get() = if (Pc.light) Color(0xFF343434) else Color(0xFFEDEDED)
private val StudioDim get() = if (Pc.light) Color(0xFF9A9A9A) else Color(0xFF8E8E8E)
// a control that can't be used now
private val StudioOff get() = if (Pc.light) Color(0xFFD5D5D5) else Color(0xFF505050)
/** Landscape: the pages' card's edge and title, the pages' names, and the settings column's width. */
private val StudioCardEdge get() = if (Pc.light) Color(0xFFDADADA) else Color(0xFF373737)
private val StudioCardTitle get() = if (Pc.light) Color(0xFF4A4A4A) else Color(0xFFC1C1C1)
private val StudioPageInk get() = if (Pc.light) Color(0xFF3E3E3E) else Color(0xFF797979)
private val SettingsWidth = 323.dp
/** A switch that can't be changed here: the usual one, faded. */
private val StudioSwitchFixed = Color(0xFF515153)
private val StudioSwitchFixedKnob = Color(0xFF9B9B9B)
/** A slider that can't be changed here: its value and knob faded into the panel. */
private val StudioPillOff get() = if (Pc.light) Color(0xFFEDEDED) else Color(0xFF222222)
private val StudioValueOff get() = if (Pc.light) Color(0xFFA0A0A0) else Color(0xFF646464)
private val StudioKnobOff get() = if (Pc.light) Color(0xFFD0D0D0) else Color(0xFF353535)
private val StudioList get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0xFF242424)
private val StudioListLine get() = if (Pc.light) Color(0xFFE5E5E5) else Color(0xFF202020)
private val StudioSegments get() = if (Pc.light) Color(0xFFDCDCDC) else Color(0xFF3C3C3C)
private val StudioMaterial get() = if (Pc.light) Color(0xFFECECEC) else Color(0xFF232323)
private val StudioMaterialInk get() = if (Pc.light) Color(0xFF4A4A4A) else Color(0xFFB1B1B1)
private val StudioPill get() = if (Pc.light) Color(0xFFE5E5E5) else Color(0xFF292929)
private val StudioTrack get() = if (Pc.light) Color(0xFFE5E5E5) else Color(0xFF333333)
private val StudioTabIcon get() = if (Pc.light) Color(0xFF3D3D3D) else Color(0xFFAAAAAA)
private val StudioButton get() = if (Pc.light) Color(0xFFFBFBFB) else Color(0xFF1C1C1C)
private val StudioButtonEdge get() = if (Pc.light) Color(0xFFE0E0E0) else Color(0xFF383838)
// the pad's buttons' text and icon: grey, not the labels' white
private val StudioButtonInk get() = if (Pc.light) Color(0xFF585858) else Color(0xFFAAAAAA)
private val StudioGroup get() = if (Pc.light) Color(0xFFEBEBEB) else Color(0xFF262626)
private val StudioCard get() = if (Pc.light) Color(0xFFE5E5E5) else Color(0xFF313131)
private val PreviewCardFill get() = if (Pc.light) Color(0xFFE8E8E8) else Color(0xFF2A2A2A)
private val PreviewCardEdge get() = if (Pc.light) Color(0xFFD0D0D0) else Color(0xFF4A4A4A)
private val PreviewCardName get() = if (Pc.light) Color(0xFF3A3A3A) else Color(0xFFC5C5C5)
private val StudioDone = Color(0xFF0190FF)

/** Where the settings start below the drawing pad (points from the top). */
private val PadHeight = 510.dp
private val TabsWidth = 81.dp

/**
 * How a slider reads; [SIZE] reads the size the control sets (the brush's most size × place^2.2) as a
 * percent (Max at the control's end), [BOTH] goes both ways from 0% in its middle, -100% to Max (and
 * [FOLLOW] and [GRADE] to a name of their own), [DOUBLE] reads 100% at its middle; [METAL] and
 * [GLOSS] name their ends as a material's metalness and roughness.
 */
enum class Shown { PERCENT, BOTH, COUNT, TIP, MOVEMENT, ZOOM, CHARGE, ZERO, SIZE, FOLLOW, GRADE, DOUBLE, TWICE, METAL, GLOSS }

/** One control in the brush studio. */
sealed class StudioControl {
    /** A slider; with [enabledBy] (a control's key and the choice it must be on), greyed out otherwise. */
    class Slider(val key: String, val label: String, val shown: Shown = Shown.PERCENT, val enabledBy: Pair<String, Int>? = null) : StudioControl()
    /** A switch; with [enabledBy] (a control's key and the choice it must be on), greyed out otherwise. */
    class Toggle(val key: String, val label: String, val enabledBy: Pair<String, Int>? = null) : StudioControl()
    class Choice(val key: String, val options: List<String>) : StudioControl()
    class Kinds(val key: String, val options: List<Pair<ImageVector, String>>) : StudioControl()
    /** The taper's two ends on a bar, with the switch linking their sizes under it. */
    class TaperBar(val start: String, val end: String, val link: String) : StudioControl()
    data object PressureCurve : StudioControl()
    /** The tilt angle on its quarter-circle graph. */
    data object TiltAngle : StudioControl()
    /** The shape's roundness and angle on an ellipse with handles. */
    data object Roundness : StudioControl()
    /** The brush's [kind] of source image, opening its editor. */
    class Source(val label: String, val kind: SourceKind) : StudioControl()
    /** A blend mode, kept by the engine's number for it, picked from a list. */
    class Picker(val key: String, val label: String) : StudioControl()
    /** A card that a tap on its title opens to show [controls] inside it. */
    class Disclosure(val label: String, val controls: List<StudioControl>) : StudioControl()
    data object PreviewCard : StudioControl()
    data object About : StudioControl()
}

class StudioSection(val title: String?, val controls: List<StudioControl>)

/** A page of the studio: its name, its icon (shown [iconSize] across) and its sections. */
class StudioTab(val name: String, val icon: ImageVector, val iconSize: Dp, val sections: List<StudioSection>)

private fun slider(key: String, label: String, shown: Shown = Shown.PERCENT, enabledBy: Pair<String, Int>? = null) =
    StudioControl.Slider(key, label, shown, enabledBy)
private fun toggle(key: String, label: String, enabledBy: Pair<String, Int>? = null) = StudioControl.Toggle(key, label, enabledBy)
private val FILTERING = listOf("No filtering", "Classic filtering", "Improved filtering")
private fun colorJitter(prefix: String, both: Boolean, brightness: String) = listOf(
    slider("${prefix}_hue", "Hue", if (both) Shown.BOTH else Shown.PERCENT),
    slider("${prefix}_saturation", "Saturation", if (both) Shown.BOTH else Shown.PERCENT),
    slider("${prefix}_brightness", brightness, if (both) Shown.BOTH else Shown.PERCENT),
)

/** The studio's tabs, in order, with what each shows. */
val STUDIO_TABS: List<StudioTab> by lazy {
    listOf(
        StudioTab("Stroke path", PcIcons.StudioStroke, 33.dp, listOf(
            StudioSection("Stroke properties", listOf(
                slider("spacing", "Spacing"), slider("spacing_jitter", "Spacing Jitter"), slider("jitter_lateral", "Jitter Lateral"),
                slider("jitter_linear", "Jitter Linear"), slider("falloff", "Fall off"),
            )),
        )),
        StudioTab("Stabilization", PcIcons.StudioStabilize, 45.dp, listOf(
            StudioSection("StreamLine", listOf(slider("streamline", "Amount"), slider("streamline_pressure", "Pressure"))),
            StudioSection("Stabilization", listOf(slider("stabilization", "Amount"))),
            StudioSection("Motion filtering", listOf(slider("motion_filtering", "Amount"), slider("motion_expression", "Expression"))),
        )),
        StudioTab("Taper", PcIcons.StudioTaper, 41.dp, listOf(
            StudioSection("Pressure taper", listOf(
                StudioControl.TaperBar("pen_taper_start", "pen_taper_end", "pen_taper_link"),
                slider("pen_taper_size", "Size"), slider("pen_taper_opacity", "Opacity"), slider("pen_taper_pressure", "Pressure"),
                slider("pen_taper_tip", "Tip", Shown.TIP), toggle("pen_tip_animation", "Tip animation"),
            )),
            StudioSection("Touch taper", listOf(
                StudioControl.TaperBar("touch_taper_start", "touch_taper_end", "touch_taper_link"),
                slider("touch_taper_size", "Size"), slider("touch_taper_opacity", "Opacity"), slider("touch_taper_tip", "Tip", Shown.TIP),
            )),
            StudioSection("Taper properties", listOf(toggle("classic_taper", "Classic taper"))),
        )),
        StudioTab("Shape", PcIcons.StudioShape, 45.dp, listOf(
            StudioSection(null, listOf(StudioControl.Source("Shape Source", SourceKind.SHAPE))),
            StudioSection("Input style", listOf(
                StudioControl.Choice("input_style", listOf("Touch only", "Azimuth", "Azimuth and barrel roll")),
                // only with the barrel roll to follow
                toggle("relative_to_stroke", "Relative to stroke", enabledBy = "input_style" to 2),
            )),
            StudioSection("Touch properties", listOf(slider("shape_rotation", "Rotation", Shown.FOLLOW))),
            StudioSection("Shape properties", listOf(
                slider("scatter", "Scatter", Shown.TWICE), slider("count", "Count", Shown.COUNT), slider("count_jitter", "Count jitter"),
                toggle("randomized", "Randomized"), toggle("flip_x", "Flip X"), toggle("flip_y", "Flip Y"),
                StudioControl.Roundness, slider("pressure_roundness", "Pressure Roundness", Shown.BOTH),
                slider("tilt_roundness", "Tilt roundness", Shown.BOTH), slider("roundness_jitter_y", "Roundness Vertical Jitter"),
                slider("roundness_jitter_x", "Roundness Horizontal Jitter"),
            )),
            StudioSection("Shape filtering", listOf(StudioControl.Choice("shape_filtering", FILTERING))),
        )),
        StudioTab("Grain", PcIcons.StudioGrain, 45.dp, listOf(
            StudioSection(null, listOf(StudioControl.Source("Grain Source", SourceKind.GRAIN))),
            StudioSection("Grain behavior", listOf(
                StudioControl.Kinds("grain_behavior", listOf(PcIcons.GrainMoving to "Moving", PcIcons.GrainTexturized to "Texturized")),
                slider("grain_movement", "Movement", Shown.MOVEMENT), slider("grain_scale", "Scale"), slider("grain_zoom", "Zoom", Shown.ZOOM),
                slider("grain_rotation", "Rotation", Shown.FOLLOW), slider("grain_depth", "Depth"), slider("grain_depth_min", "Depth minimum"),
                slider("grain_depth_jitter", "Depth jitter"), toggle("grain_offset_jitter", "Offset jitter"),
                StudioControl.Picker("grain_blend", "Blend mode"), slider("grain_brightness", "Brightness", Shown.BOTH),
                slider("grain_contrast", "Contrast", Shown.BOTH),
            )),
            StudioSection("Grain filtering", listOf(StudioControl.Choice("grain_filtering", FILTERING))),
            StudioSection("3D grain behaviour", listOf(toggle("grain_follows_camera", "Grain follows camera"))),
        )),
        StudioTab("Rendering", PcIcons.StudioRendering, 37.dp, listOf(
            StudioSection("Rendering mode", listOf(
                StudioControl.Choice("rendering_mode", listOf("Light Glaze", "Uniformed Glaze", "Intense Glaze", "Heavy Glaze", "Uniform Blending", "Intense Blending")),
            )),
            StudioSection("Blending", listOf(
                slider("flow", "Flow"), slider("wet_edges", "Wet edges"), slider("burnt_edges", "Burnt edges"),
                StudioControl.Picker("burnt_blend", "Burnt edges mode"), StudioControl.Picker("blend_mode", "Blend mode"),
                toggle("luminance_blending", "Luminance blending"), toggle("alpha_threshold", "Alpha Threshold"),
                slider("alpha_threshold_amount", "Threshold Amount", enabledBy = "alpha_threshold" to 1),
                // only for a brush paired with another, which none are yet
                toggle("classic_combine", "Classic Normal Combine mode", enabledBy = "dual_brush" to 1),
            )),
        )),
        StudioTab("Wet Mix", PcIcons.StudioWet, 45.dp, listOf(
            StudioSection("Wet mix", listOf(
                slider("wet_dilution", "Dilution"), slider("wet_charge", "Charge", Shown.CHARGE), slider("wet_attack", "Attack"),
                slider("wet_pull", "Pull"), slider("wet_grade", "Grade", Shown.GRADE), slider("wet_blur", "Blur"),
                slider("wet_blur_jitter", "Blur jitter"), slider("wet_jitter", "Wetness jitter"),
            )),
        )),
        StudioTab("Color dynamics", PcIcons.StudioColor, 41.dp, listOf(
            StudioSection("Stamp color jitter", listOf(
                slider("stamp_hue", "Hue"), slider("stamp_saturation", "Saturation"), slider("stamp_lightness", "Lightness"),
                slider("stamp_darkness", "Darkness"), slider("stamp_secondary", "Secondary color"),
            )),
            StudioSection("Stroke color jitter", listOf(
                slider("stroke_hue", "Hue"), slider("stroke_saturation", "Saturation"), slider("stroke_lightness", "Lightness"),
                slider("stroke_darkness", "Darkness"), slider("stroke_secondary", "Secondary color"),
            )),
            StudioSection("Color pressure", colorJitter("pressure", true, "Brightness") + slider("pressure_secondary", "Secondary Color")),
            StudioSection("Color tilt", colorJitter("tilt", true, "Brightness") + slider("tilt_secondary", "Secondary Color")),
            StudioSection("Color barrel roll", colorJitter("roll", false, "Brightness") + slider("roll_secondary", "Secondary Color")),
        )),
        StudioTab("Dynamics", PcIcons.StudioDynamics, 37.dp, listOf(
            StudioSection("Speed", listOf(slider("speed_size", "Size", Shown.BOTH), slider("speed_opacity", "Opacity", Shown.BOTH), slider("speed_spacing", "Spacing"))),
            StudioSection("Jitter", listOf(slider("jitter_size", "Size"), slider("jitter_opacity", "Opacity"))),
        )),
        StudioTab("Stylus", PcIcons.StudioStylus, 43.dp, listOf(
            StudioSection("Pressure", listOf(
                StudioControl.PressureCurve, slider("pressure_size", "Size", Shown.BOTH), slider("pressure_flow", "Opacity"),
                slider("pressure_opacity", "Flow", Shown.BOTH), slider("pressure_bleed", "Bleed"),
            )),
            StudioSection("Tilt", listOf(
                StudioControl.TiltAngle, slider("tilt_opacity", "Opacity"), slider("tilt_gradation", "Gradation"),
                slider("tilt_bleed", "Bleed"), slider("tilt_size", "Size"), toggle("tilt_size_compression", "Size compression"),
            )),
            StudioSection("Barrel roll", listOf(slider("roll_size", "Size", Shown.BOTH), slider("roll_opacity", "Opacity"), slider("roll_bleed", "Bleed"))),
            StudioSection("Cursor outline", listOf(StudioControl.Choice("cursor_outline", listOf("None", "Contrast", "Active color")))),
            StudioSection("Hover", listOf(slider("hover_pressure", "Estimated pressure"))),
            StudioSection("Hover fill", listOf(StudioControl.Choice("hover_fill", listOf("None", "Shape", "All")))),
        )),
        StudioTab("Properties", PcIcons.StudioProperties, 49.dp, listOf(
            StudioSection("Brush properties", listOf(toggle("orient_to_screen", "Orient to screen"), slider("smudge_pull", "Smudge Pull"))),
            StudioSection("Brush behavior", listOf(
                slider("max_size", "Maximum size", Shown.SIZE), slider("min_size", "Minimum size", Shown.SIZE),
                slider("max_opacity", "Maximum opacity"), slider("min_opacity", "Minimum opacity"),
            )),
        )),
        StudioTab("Materials", PcIcons.StudioMaterials, 29.dp, listOf(
            StudioSection("3D Materials", listOf(
                StudioControl.Disclosure("Metallic", listOf(
                    slider("metallic_amount", "Amount", Shown.METAL), StudioControl.Source("Metallic Source", SourceKind.METALLIC),
                    slider("metallic_scale", "Scale"),
                )),
                StudioControl.Disclosure("Roughness", listOf(
                    slider("roughness_amount", "Amount", Shown.GLOSS), StudioControl.Source("Roughness Source", SourceKind.ROUGHNESS),
                    slider("roughness_scale", "Scale"),
                )),
            )),
        )),
        StudioTab("Preview", PcIcons.StudioPreview, 45.dp, listOf(
            StudioSection("Preview render", listOf(StudioControl.PreviewCard)),
            StudioSection("Preview properties", listOf(
                toggle("preview_stamp", "Use stamp preview"), slider("preview_size", "Size"), slider("preview_pressure_min", "Pressure Minimum"),
                slider("preview_pressure_scale", "Pressure Scale", Shown.DOUBLE), toggle("preview_wet_mix", "Wet Mix"),
                slider("preview_tilt_offset", "Tilt angle", Shown.BOTH),
            )),
        )),
        StudioTab("About this brush", PcIcons.StudioAbout, 45.dp, listOf(StudioSection(null, listOf(StudioControl.About)))),
    )
}

/** Every key the studio's controls read. */
val STUDIO_KEYS: List<String> by lazy {
    STUDIO_TABS.flatMap { t -> t.sections.flatMap { s -> s.controls } }.flatMap { c -> if (c is StudioControl.Disclosure) c.controls else listOf(c) }.flatMap { c ->
        when (c) {
            // a slider's own tilt switch and angle, which its keypad shows
            is StudioControl.Slider -> sliderExtras(c.key).let { e -> listOf(c.key, e.tiltSwitch, e.tiltAngle).filter { it.isNotEmpty() } }
            is StudioControl.Toggle -> listOf(c.key)
            is StudioControl.Choice -> listOf(c.key)
            is StudioControl.Kinds -> listOf(c.key)
            is StudioControl.Picker -> listOf(c.key)
            is StudioControl.TaperBar -> listOf(c.start, c.end, c.link)
            StudioControl.TiltAngle -> listOf("tilt_angle")
            StudioControl.Roundness -> listOf("roundness", "shape_angle")
            else -> emptyList()
        }
    }.plus("most_size").distinct()
}

/** The space above a control: after the section's title (when [titled]) or the control before it. */
/** What a space in the studio's column can follow or come before, besides a control. */
private enum class Edge { TOP, TITLE }

/**
 * The space between [above] (a control, a section's title or the column's top) and [below] (a
 * control or a title), as the reference spaces them. Titles here are 23 tall (24 there), slider rows
 * 55, switch and picker rows 47, list rows 44 with a line between.
 */
private fun gap(above: Any, below: Any): Dp {
    // switch and picker rows
    fun line(c: Any) = c is StudioControl.Toggle || c is StudioControl.Picker
    return when {
        above == Edge.TOP -> 8.dp
        above == Edge.TITLE -> when {
            line(below) -> 7.5.dp
            below == StudioControl.PressureCurve -> 29.dp
            below == StudioControl.TiltAngle -> 28.dp
            below == StudioControl.About -> 29.dp
            below is StudioControl.Disclosure -> 41.dp
            else -> 23.dp
        }
        below == Edge.TITLE -> when {
            line(above) -> 50.5.dp
            above is StudioControl.Slider -> 46.5.dp
            above == StudioControl.PreviewCard -> 48.dp
            else -> 47.dp
        }
        line(above) -> if (line(below)) 4.dp else 19.5.dp
        above is StudioControl.Slider || above is StudioControl.Choice -> if (line(below)) 0.dp else 15.5.dp
        above is StudioControl.Kinds -> 16.dp
        above is StudioControl.TaperBar -> 23.5.dp
        above == StudioControl.PressureCurve -> 24.5.dp
        above == StudioControl.TiltAngle -> 21.5.dp
        above == StudioControl.Roundness -> 16.5.dp
        above is StudioControl.Disclosure -> 29.dp
        else -> 15.5.dp
    }
}

/**
 * A blend mode setting: its mode's name, in blue, opens the modes as a list over it (the grain's
 * own list for the grain, the layers' and behind for the brush and its burnt edges).
 */
@Composable
private fun BlendPicker(vm: CanvasViewModel, c: StudioControl.Picker) {
    var open by remember { mutableStateOf(false) }
    var at by remember { mutableFloatStateOf(0f) }
    val current = vm.studio(c.key).roundToInt()
    val modes = if (c.key == "grain_blend") vm.grainBlendModeList else vm.brushBlendModeList
    Row(
        Modifier.offset(y = 2.dp).fillMaxWidth().height(47.dp).padding(start = 8.dp, end = 8.dp).onGloballyPositioned { at = it.boundsInWindow().center.y },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(c.label, color = StudioDim, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
        // a file may give the grain a layer mode the list leaves out
        val name = (modes + vm.blendModeList).firstOrNull { it.id.toInt() == current }?.name ?: "Normal"
        Text(name, color = Pc.Accent, fontSize = 17.sp, maxLines = 1, modifier = Modifier.tap { open = true })
    }
    if (open) {
        BlendModeList(modes, current, at) { picked ->
            open = false
            picked?.let { vm.setStudio(c.key, it.toFloat()) }
        }
    }
}

private val ModeRow = 38.dp
private val ModeListWidth = 250.dp
/** At most this many rows show at once; the rest scroll. */
private const val MODE_ROWS_SHOWN = 11

/**
 * The blend modes in the layer panel's order, as a light list 10 from the window's right with the
 * current one ([current], an engine number) over the window height [centreY]; a tap picks one,
 * a tap outside it closes it ([onDone] with null).
 */
@Composable
private fun BlendModeList(modes: List<io.github.artcraftandroid.procraft.core.BlendModeInfo>, current: Int, centreY: Float, onDone: (Int?) -> Unit) {
    val density = LocalDensity.current
    val chosen = modes.indexOfFirst { it.id.toInt() == current }.coerceAtLeast(0)
    val shown = minOf(MODE_ROWS_SHOWN, modes.size)
    val first = (chosen - shown / 2).coerceIn(0, (modes.size - shown).coerceAtLeast(0))
    val pitch = with(density) { (ModeRow + 1.dp).toPx() }
    val height = ModeRow * shown + 1.dp * (shown - 1)
    val list = rememberLazyListState(first)
    val place = remember(centreY, chosen, first) {
        object : androidx.compose.ui.window.PopupPositionProvider {
            override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
                val margin = with(density) { 10.dp.roundToPx() }
                val top = (centreY - (chosen - first + 0.5f) * pitch).roundToInt()
                return IntOffset(
                    windowSize.width - margin - popupContentSize.width,
                    top.coerceIn(margin, (windowSize.height - margin - popupContentSize.height).coerceAtLeast(margin)),
                )
            }
        }
    }
    PcPopup(textScale = MATCHED_POPUP_TEXT, popupPositionProvider = place, onDismissRequest = { onDone(null) }, properties = androidx.compose.ui.window.PopupProperties(focusable = true)) {
        androidx.compose.foundation.lazy.LazyColumn(
            Modifier.size(ModeListWidth, height).shadow(24.dp, RoundedCornerShape(13.dp)).clip(RoundedCornerShape(13.dp)).background(ModeListFill),
            state = list,
        ) {
            items(modes.size) { i ->
                val m = modes[i]
                val picked = i == chosen
                if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(ModeListLine))
                Row(
                    Modifier.fillMaxWidth().height(ModeRow).background(if (picked) Pc.RowSelected else Color.Transparent).tap { onDone(m.id.toInt()) }
                        .padding(start = 12.dp, end = 18.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(m.name, color = if (picked) Color.White else ModeListInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
                    // its 26 frame centred 31 in from the right
                    PcIcons.BlendGlyphs[m.name]?.let { Icon(it, contentDescription = null, tint = if (picked) Color.White else ModeListGlyph, modifier = Modifier.size(26.dp)) }
                }
            }
        }
    }
}

private val ModeListFill = Color(0xFFE1E1E1)
private val ModeListLine = Color(0xFFCDCDCD)
private val ModeListInk = Color(0xFF393939)
private val ModeListGlyph = Color(0xFF7F7F7F)

/** A slider's value as it reads. */
private fun shownValue(v: Float, shown: Shown, most: Float = 16f): String {
    val percent = "${(v * 100).roundToInt()}%"
    // between ends that read by name, a value never rounds onto an end (0.996 reads 99%)
    val between = "${(v * 100).roundToInt().coerceIn(1, 99)}%"
    val belowTop = "${(v * 100).roundToInt().coerceAtMost(99)}%"
    return when (shown) {
        Shown.PERCENT -> when {
            v <= 0f -> "None"
            v >= 1f -> "Max"
            else -> between
        }
        Shown.ZERO -> percent
        Shown.BOTH -> if (v >= 1f) "Max" else belowTop
        // smoothing all the way
        Shown.GRADE -> if (v >= 1f) "Smooth" else belowTop
        // the control's middle reads as 100%
        Shown.DOUBLE -> "${(v * 200).roundToInt()}%"
        // as a percent, with the control's middle at 100%
        Shown.TWICE -> when {
            v <= 0f -> "None"
            v >= 1f -> "Max"
            else -> "${(v * 200).roundToInt().coerceIn(1, 199)}%"
        }
        // turning with the stroke: all the way reads as following it
        Shown.FOLLOW -> if (v >= 1f) "Follow stroke" else belowTop
        Shown.COUNT -> "${1 + (v * 15).roundToInt()}"
        Shown.TIP -> if (v <= 0f) "Sharp" else if (v >= 1f) "Blunt" else between
        Shown.MOVEMENT -> if (v <= 0f) "Stamp" else if (v >= 1f) "Rolling" else between
        Shown.ZOOM -> if (v <= 0f) "Follow size" else if (v >= 1f) "Cropped" else between
        Shown.CHARGE -> if (v <= 0f) "Disabled" else if (v >= 1f) "Max" else between
        Shown.METAL -> if (v <= 0f) "Non-Metallic" else if (v >= 1f) "Metallic" else between
        Shown.GLOSS -> if (v <= 0f) "Glossy" else if (v >= 1f) "Matte" else between
        Shown.SIZE -> {
            val size = most * v.coerceIn(0f, 1f).toDouble().pow(2.2).toFloat()
            // Max only at the control's end: a size of more than a unit reads as its percent
            if (v >= 1f) "Max" else if (size <= 0f) "None" else "${(size * 100).roundToInt()}%"
        }
    }
}

private val PadBehind get() = if (Pc.light) Color(0xFFF3F3F3) else Color(0xFF1F1F1F)

/**
 * The brush studio: tabs down the left, the drawing pad above, the tab's settings below. It slides up
 * from the bottom of the screen, and back down when it closes.
 */
@Composable
fun BrushStudio(vm: CanvasViewModel) {
    androidx.compose.animation.AnimatedVisibility(
        vm.studioOpen,
        enter = androidx.compose.animation.slideInVertically(Motion.studioIn()) { it },
        exit = androidx.compose.animation.slideOutVertically(Motion.studioOut()) { it },
    ) { Studio(vm) }
}

@Composable
private fun Studio(vm: CanvasViewModel) {
    val landscape = LocalConfiguration.current.orientation == Configuration.ORIENTATION_LANDSCAPE
    Box(Modifier.fillMaxSize().background(StudioBack).pointerInput(Unit) { awaitEachGesture { awaitFirstDown().consume() } }) {
        if (landscape) {
            // the pages listed by name on a card, the settings beside them, the pad on the right
            Row(Modifier.fillMaxSize()) {
                StudioPagesCard(vm)
                StudioSettings(vm, Modifier.width(SettingsWidth).fillMaxHeight())
                Box(Modifier.width(1.dp).fillMaxHeight().background(StudioLine))
                StudioPad(vm, Modifier.fillMaxSize())
            }
        } else {
            Row(Modifier.fillMaxSize()) {
                StudioRail(vm)
                Column(Modifier.fillMaxSize()) {
                    StudioPad(vm, Modifier.fillMaxWidth().height(PadHeight))
                    Box(Modifier.fillMaxWidth().height(1.dp).background(StudioLine))
                    StudioSettings(vm, Modifier.fillMaxSize())
                }
            }
        }
    }
    SourceEditor(vm)
}

/** Portrait: the pages as icons down the left. */
@Composable
private fun StudioRail(vm: CanvasViewModel) {
    Column(Modifier.width(TabsWidth).fillMaxHeight().background(StudioTabs).padding(top = 46.dp)) {
        STUDIO_TABS.forEachIndexed { i, tab ->
            val on = vm.studioTab == i
            Box(
                Modifier.padding(start = 8.dp, bottom = 3.dp).size(65.dp, 45.dp).clip(RoundedCornerShape(22.5.dp))
                    .background(if (on) Pc.Accent else Color.Transparent).tap { vm.studioTab = i },
                contentAlignment = Alignment.Center,
            ) {
                // drawn in a 50 frame, fitted to the tab's own size
                Icon(tab.icon, contentDescription = tab.name, tint = if (on) Color.White else StudioTabIcon, modifier = Modifier.requiredSize(tab.iconSize))
            }
        }
    }
}

/** Landscape: the pages by icon and name on a card headed "Brush Studio", the list scrolling under the title. */
@Composable
private fun StudioPagesCard(vm: CanvasViewModel) {
    val shape = RoundedCornerShape(20.dp)
    Column(
        Modifier.padding(start = 10.dp, top = 10.dp, bottom = 10.dp).width(320.dp).fillMaxHeight().clip(shape).background(StudioTabs)
            .border(1.dp, StudioCardEdge, shape),
    ) {
        Text("Brush Studio", color = StudioCardTitle, fontSize = 24.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.padding(start = 24.dp, top = 19.dp))
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(start = 8.dp, end = 8.dp, top = 17.dp, bottom = 8.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            STUDIO_TABS.forEachIndexed { i, tab ->
                val on = vm.studioTab == i
                Row(
                    Modifier.fillMaxWidth().height(44.dp).clip(RoundedCornerShape(22.dp)).background(if (on) Pc.Accent else Color.Transparent).tap { vm.studioTab = i },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.width(52.dp), contentAlignment = Alignment.Center) {
                        Icon(tab.icon, contentDescription = null, tint = if (on) Color.White else StudioTabIcon, modifier = Modifier.requiredSize(tab.iconSize))
                    }
                    Text(tab.name, color = if (on) Color.White else StudioPageInk, fontSize = 17.sp, maxLines = 1)
                }
            }
        }
    }
}

/** The drawing pad with its buttons: Drawing Pad (its menu), Cancel and Done. */
@Composable
private fun StudioPad(vm: CanvasViewModel, modifier: Modifier) {
    // the pad's own colour behind its surface, so the canvas doesn't show through before the
    // pad's first frame (as the studio slides up)
    // a sphere showing the brush's materials on their page, or on every page with the 3D preview on
    val sphere = STUDIO_TABS.getOrNull(vm.studioTab)?.name == "Materials" || vm.pad3d
    androidx.compose.runtime.LaunchedEffect(sphere) { vm.showPadSphere(sphere) }
    Box(modifier.background(PadBehind)) {
        androidx.compose.ui.viewinterop.AndroidView(factory = { PadSurfaceView(it, vm) }, modifier = Modifier.fillMaxSize())
        // drawn over the surface's hole until the pad has a frame of its own
        if (!vm.padShown) Box(Modifier.fillMaxSize().background(PadBehind))
        Row(Modifier.offset(21.dp, 21.dp).height(43.dp).onGloballyPositioned { vm.padButtonCentre = it.boundsInWindow().center.x }
            .clip(CircleShape).background(StudioButton).border(1.dp, StudioButtonEdge, CircleShape)
            .tap { vm.padMenuOpen = !vm.padMenuOpen }.padding(start = 9.dp, end = 13.dp),
            verticalAlignment = Alignment.CenterVertically) {
            Icon(PcIcons.StudioPad, contentDescription = null, tint = StudioButtonInk, modifier = Modifier.size(26.dp))
            Spacer(Modifier.width(6.dp))
            Text("Drawing Pad", color = StudioButtonInk, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
        }
        Row(Modifier.align(Alignment.TopEnd).padding(top = 20.dp, end = 20.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(73.dp, 43.dp).clip(CircleShape).background(StudioButton).border(1.dp, StudioButtonEdge, CircleShape).tap { vm.closeStudio(keep = false) },
                contentAlignment = Alignment.Center) {
                Text("Cancel", color = StudioButtonInk, fontSize = 17.sp, maxLines = 1)
            }
            Spacer(Modifier.width(13.dp))
            Box(Modifier.size(44.dp).clip(CircleShape).background(StudioDone).tap { vm.closeStudio(keep = true) }, contentAlignment = Alignment.Center) {
                Icon(PcIcons.DoneTick, contentDescription = "Done", tint = Color.White, modifier = Modifier.requiredSize(40.dp))
            }
        }
    }
}

/** The open page's settings, in sections, spaced as the reference spaces them. */
@Composable
private fun StudioSettings(vm: CanvasViewModel, modifier: Modifier) {
    val tab = STUDIO_TABS.getOrNull(vm.studioTab) ?: return
    // each section opens at its top
    Column(modifier.verticalScroll(remember(vm.studioTab) { androidx.compose.foundation.ScrollState(0) }).padding(start = 20.dp, end = 20.dp, bottom = 77.dp)) {
        // what the space above each thing follows: a control, a title, or the top
        var above: Any = Edge.TOP
        tab.sections.forEachIndexed { k, section ->
            // the About page is headed by the brush's name
            val title = section.title ?: vm.studioName.takeIf { section.controls.firstOrNull() == StudioControl.About }
            title?.let {
                Spacer(Modifier.height(if (k == 0) 25.dp else gap(above, Edge.TITLE)))
                Text(it, color = StudioHeader, fontSize = 19.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.padding(start = 8.dp))
                above = Edge.TITLE
            }
            for (control in section.controls) {
                Spacer(Modifier.height(gap(above, control)))
                StudioControlView(vm, control)
                above = control
            }
        }
    }
}

@Composable
private fun StudioControlView(vm: CanvasViewModel, c: StudioControl) {
    when (c) {
        is StudioControl.Slider -> {
            val enabled = c.enabledBy?.let { (key, choice) -> vm.studio(key).roundToInt() == choice } ?: true
            val extras = remember(c.key) { keypadExtras(vm, c.key) }
            StudioSlider(c.label, vm.studio(c.key), c.shown, most = if (c.shown == Shown.SIZE) vm.studio("most_size") else 16f, enabled = enabled, extras = extras) { vm.setStudio(c.key, it) }
        }
        // a long name runs on under the switch, which is drawn over it
        is StudioControl.Toggle -> Box(Modifier.fillMaxWidth().height(47.dp).padding(start = 8.dp)) {
            val enabled = c.enabledBy?.let { (key, choice) -> vm.studio(key).roundToInt() == choice } ?: true
            Text(c.label, color = if (enabled) StudioDim else StudioOff, fontSize = 17.sp, maxLines = 1, softWrap = false, modifier = Modifier.align(Alignment.CenterStart))
            val on = vm.studio(c.key) >= 0.5f
            Box(Modifier.align(Alignment.CenterEnd).padding(end = 6.dp).then(if (enabled) Modifier.tap { vm.setStudio(c.key, if (on) 0f else 1f) } else Modifier)) {
                Switch(on, enabled = enabled, fixedTrack = StudioSwitchFixed, fixedKnob = StudioSwitchFixedKnob)
            }
        }
        is StudioControl.Choice -> Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(22.dp)).background(StudioList)) {
            val chosen = vm.studio(c.key).roundToInt()
            c.options.forEachIndexed { i, label ->
                if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(StudioListLine))
                Row(
                    Modifier.fillMaxWidth().height(44.dp).background(if (i == chosen) Pc.Accent else Color.Transparent).tap { vm.setStudio(c.key, i.toFloat()) }
                        .padding(start = 16.dp, end = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(label, color = if (i == chosen) Color.White else StudioDim, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
                    if (i == chosen) Icon(PcIcons.MenuTick, contentDescription = null, tint = Color.White, modifier = Modifier.requiredSize(34.dp))
                }
            }
        }
        is StudioControl.Kinds -> {
            val chosen = vm.studio(c.key).roundToInt()
            Row(
                Modifier.padding(start = 8.dp, end = 8.dp).fillMaxWidth().height(34.dp).clip(RoundedCornerShape(17.dp)).background(StudioSegments).padding(2.dp),
                horizontalArrangement = Arrangement.spacedBy(11.dp),
            ) {
                c.options.forEachIndexed { i, (icon, label) ->
                    Row(
                        Modifier.weight(1f).fillMaxHeight().clip(RoundedCornerShape(15.dp)).background(if (i == chosen) Pc.Accent else Color.Transparent).tap { vm.setStudio(c.key, i.toFloat()) },
                        horizontalArrangement = Arrangement.Center,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        // drawn in a 50 frame, shown 30 across, the name 2 after
                        Icon(icon, contentDescription = null, tint = if (i == chosen) Color.White else StudioDim, modifier = Modifier.requiredSize(30.dp))
                        Spacer(Modifier.width(2.dp))
                        Text(label, color = if (i == chosen) Color.White else StudioDim, fontSize = 13.sp, maxLines = 1)
                    }
                }
            }
        }
        is StudioControl.TaperBar -> Column {
            // linked, the two ends move together
            val linked = vm.studio(c.link) >= 0.5f
            TaperBar(
                vm.studio(c.start), vm.studio(c.end),
                { vm.setStudio(c.start, it); if (linked) vm.setStudio(c.end, it) },
            ) { vm.setStudio(c.end, it); if (linked) vm.setStudio(c.start, it) }
            Spacer(Modifier.height(4.5.dp))
            StudioControlView(vm, StudioControl.Toggle(c.link, "Link tip sizes"))
        }
        StudioControl.PressureCurve -> PressureCurve(
            vm.studioCurve, edge = StudioCurveEdge, fill = StudioCurveFill, corner = 8.dp, line = StudioCurveLine, lineWidth = 1.dp, handle = 5.5.dp to 3.5.dp,
            onChange = vm::changeStudioCurve,
        )
        StudioControl.TiltAngle -> TiltAngle(vm.studio("tilt_angle")) { vm.setStudio("tilt_angle", it) }
        StudioControl.Roundness -> RoundnessGraph(vm.studio("roundness"), vm.studio("shape_angle"), onRoundness = { vm.setStudio("roundness", it) }, onAngle = { vm.setStudio("shape_angle", it) })
        is StudioControl.Source -> if (LocalConfiguration.current.orientation == Configuration.ORIENTATION_LANDSCAPE) SourceBox(vm, c) else Row(
            Modifier.fillMaxWidth().height(60.dp).clip(RoundedCornerShape(12.dp)).background(StudioCard)
                .tap { vm.sourceEditing = c.kind }.padding(start = 16.dp, end = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(c.label, color = StudioHeader, fontSize = 19.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.weight(1f))
            // the image it uses: the shape white on the row (the round tip a white disc), the grain as its greys, a
            // material's amount all over as white
            val image = vm.studioSources[c.kind]
            val plain = if (c.kind.material()) Modifier.background(Color.White) else Modifier.clip(CircleShape).background(Color.White)
            Box(Modifier.size(44.dp).clip(RoundedCornerShape(10.dp)).then(if (image == null) plain else Modifier)) {
                image?.let { Image(it, contentDescription = null, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize()) }
            }
        }
        is StudioControl.Picker -> BlendPicker(vm, c)
        is StudioControl.Disclosure -> DisclosureCard(vm, c)
        // the brush's stroke filling the card's height, its name over it on a patch of the card's own colour
        StudioControl.PreviewCard -> Box(
            Modifier.fillMaxWidth()
                .aspectRatio(if (LocalConfiguration.current.orientation == Configuration.ORIENTATION_PORTRAIT) 623f / 200f else 278.5f / 91.5f)
                .clip(RoundedCornerShape(12.dp))
                .background(PreviewCardFill).border(1.dp, PreviewCardEdge, RoundedCornerShape(12.dp)),
        ) {
            vm.studioPreview?.let {
                Image(it, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
            }
            Box(
                Modifier.padding(start = 15.5.dp, top = 14.5.dp).height(37.dp).clip(RoundedCornerShape(10.dp)).background(PreviewCardFill)
                    .padding(horizontal = 17.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                Text(vm.studioName, color = PreviewCardName, fontSize = 20.sp, fontWeight = FontWeight.Bold, maxLines = 1, softWrap = false)
            }
        }
        StudioControl.About -> AboutBrush(vm)
    }
}

/**
 * A brush's About page: a card with its maker's picture, name and signature (ProCraft's own for
 * ours), then making a new reset point and resetting to it, with when it was made.
 */
@Composable
private fun AboutBrush(vm: CanvasViewModel) {
    val about = vm.studioAbout
    Column(Modifier.fillMaxWidth()) {
        Column(Modifier.fillMaxWidth().height(295.dp).clip(RoundedCornerShape(20.dp)).background(AboutCard), horizontalAlignment = Alignment.CenterHorizontally) {
            Spacer(Modifier.height(30.dp))
            Box(Modifier.size(96.dp).clip(CircleShape).background(AboutFace), contentAlignment = Alignment.Center) {
                val picture = about?.picture
                if (picture != null) {
                    Image(picture, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
                } else {
                    Icon(PcIcons.Brush, contentDescription = null, tint = StudioDim, modifier = Modifier.size(56.dp))
                }
            }
            Spacer(Modifier.height(21.dp))
            Text("Made by ${about?.author?.takeIf { it.isNotBlank() } ?: "ProCraft"}", color = AboutName, fontSize = 20.sp, maxLines = 1)
            Spacer(Modifier.height(25.dp))
            // the signature in grey, whatever its own colour
            about?.signature?.let {
                Image(it, contentDescription = null, contentScale = ContentScale.Fit, colorFilter = ColorFilter.tint(AboutSignature), modifier = Modifier.size(300.dp, 64.dp))
            }
        }
        Spacer(Modifier.height(25.dp))
        Box(
            Modifier.fillMaxWidth().height(44.dp).clip(RoundedCornerShape(22.dp)).background(AboutCard).tap { vm.newResetPoint() }.padding(start = 15.dp),
            contentAlignment = Alignment.CenterStart,
        ) {
            Text("Create new reset point", color = AboutRowInk, fontSize = 17.sp, maxLines = 1)
        }
        Spacer(Modifier.height(11.dp))
        val made = about?.resetMade
        Column(
            Modifier.fillMaxWidth().height(if (made != null) 69.dp else 44.dp).clip(RoundedCornerShape(22.dp)).background(AboutCard).tap { vm.resetStudio() }
                .padding(start = 15.dp),
            verticalArrangement = Arrangement.Center,
        ) {
            Text("Reset brush", color = AboutRowInk, fontSize = 17.sp, maxLines = 1)
            if (made != null) {
                Spacer(Modifier.height(4.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(17.dp).clip(CircleShape).background(Pc.Accent), contentAlignment = Alignment.Center) {
                        Icon(PcIcons.MenuTick, contentDescription = null, tint = Color.White, modifier = Modifier.requiredSize(20.dp))
                    }
                    Spacer(Modifier.width(6.dp))
                    // the short date and time with a comma between, as the reference writes them
                    val day = java.util.Date(made * 1000)
                    val at = java.text.DateFormat.getDateInstance(java.text.DateFormat.SHORT).format(day) + ", " +
                        java.text.DateFormat.getTimeInstance(java.text.DateFormat.SHORT).format(day)
                    Text("Last reset point: $at", color = AboutRowInk, fontSize = 13.sp, maxLines = 1)
                }
            }
        }
    }
}

/** Whether this is a material's source (white all over when it has no image). */
private fun SourceKind.material() = this == SourceKind.METALLIC || this == SourceKind.ROUGHNESS

/**
 * A card titled [StudioControl.Disclosure.label] that a tap on its title opens, its chevron turned
 * down, to show its controls inside it, 8 in from its sides and spaced as a page's are; the page
 * opens with it closed.
 */
@Composable
private fun DisclosureCard(vm: CanvasViewModel, c: StudioControl.Disclosure) {
    var open by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(20.dp)).background(StudioMaterial)) {
        Row(
            Modifier.fillMaxWidth().height(60.dp).tap { open = !open }.padding(start = 16.dp, end = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(c.label, color = StudioMaterialInk, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, modifier = Modifier.weight(1f))
            Box(Modifier.size(30.dp).clip(CircleShape).background(StudioBack), contentAlignment = Alignment.Center) {
                Icon(PcIcons.ChevronRight, contentDescription = null, tint = Color.White, modifier = Modifier.size(16.dp).rotate(if (open) 90f else 0f))
            }
        }
        if (open) Column(Modifier.padding(start = 8.dp, end = 8.dp, top = 12.dp, bottom = 8.dp)) {
            c.controls.forEachIndexed { i, control ->
                c.controls.getOrNull(i - 1)?.let { Spacer(Modifier.height(gap(it, control))) }
                StudioControlView(vm, control)
            }
        }
    }
}

/**
 * Landscape: a source as a title with an Edit button, over a square of its image as wide as the
 * rows (the shape white on dark, the grain as its greys, a material's amount all over white).
 */
@Composable
private fun SourceBox(vm: CanvasViewModel, c: StudioControl.Source) {
    val open = { vm.sourceEditing = c.kind }
    Column(Modifier.fillMaxWidth()) {
        Box(Modifier.fillMaxWidth().padding(top = 8.5.dp).height(45.dp)) {
            // where a section's title sits
            Text(c.label, color = StudioHeader, fontSize = 19.sp, fontWeight = FontWeight.Bold, maxLines = 1,
                modifier = Modifier.align(Alignment.TopStart).padding(start = 8.dp, top = 9.5.dp))
            Box(Modifier.align(Alignment.CenterEnd).offset(x = 4.dp).size(54.dp, 45.dp).clip(CircleShape).background(StudioButton).tap { open() },
                contentAlignment = Alignment.Center) {
                Text("Edit", color = StudioButtonInk, fontSize = 17.sp, maxLines = 1)
            }
        }
        Spacer(Modifier.height(12.5.dp))
        val image = vm.studioSourcesBig[c.kind]
        val white = image == null && c.kind.material()
        Box(Modifier.fillMaxWidth().aspectRatio(1f).clip(RoundedCornerShape(22.dp)).background(if (white) Color.White else StudioButton).tap { open() }) {
            image?.let { Image(it, contentDescription = null, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize()) }
            // the round tip: a white disc
            if (image == null && !white) Box(Modifier.fillMaxSize(0.7f).align(Alignment.Center).clip(CircleShape).background(Color.White))
        }
    }
}

private val AboutCard get() = if (Pc.light) Color(0xFFEBEBEB) else Color(0xFF252525)
private val AboutFace get() = if (Pc.light) Color(0xFFD8D8D8) else Color(0xFF3A3A3A)
private val AboutName get() = if (Pc.light) Color(0xFF2A2A2A) else Color(0xFFE0E0E0)
private val AboutRowInk get() = if (Pc.light) Color(0xFF4A4A4A) else Color(0xFFBBBBBB)
private val AboutSignature get() = if (Pc.light) Color(0xFFA8A8A8) else Color(0xFF595959)

private val PadMenuFill = Color(0xFFC1C1C1)
private val PadMenuGroup = Color(0xFFD3D3D3)
private val PadMenuText = Color(0xFF393939)
private val PadMenuLine = Color(0xFFBDBDBD)

/**
 * The drawing pad's settings: preview size, the pad's colour, and clearing or resetting. It opens
 * under the Drawing Pad button, centred on it but kept 14 in from the screen's sides ([width]).
 */
@Composable
fun PadMenu(vm: CanvasViewModel, width: Dp) {
    if (!vm.studioOpen || !vm.padMenuOpen) return
    val centre = with(androidx.compose.ui.platform.LocalDensity.current) { vm.padButtonCentre.toDp() }
    val left = (centre - 160.dp).coerceIn(14.dp, (width - 334.dp).coerceAtLeast(14.dp))
    Column(
        Modifier.offset(left, 79.dp).width(320.dp).clip(RoundedCornerShape(26.dp)).background(PadMenuFill).padding(start = 10.dp, end = 10.dp, top = 17.dp, bottom = 10.dp),
    ) {
        Text("Settings", color = PadMenuText, fontSize = 22.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.padding(start = 12.dp))
        Spacer(Modifier.height(18.dp))
        Row(Modifier.fillMaxWidth().padding(horizontal = 10.dp)) {
            Text("Preview size", color = PadMenuText, fontSize = 16.sp, maxLines = 1, modifier = Modifier.weight(1f))
            Text("${(vm.studio("preview_size") * 100).roundToInt()}%", color = PadMenuText, fontSize = 16.sp, maxLines = 1)
        }
        PadSizeSlider(vm.studio("preview_size")) { vm.setStudio("preview_size", it) }
        Spacer(Modifier.height(18.dp))
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
            CanvasViewModel.PAD_COLORS.forEachIndexed { i, c ->
                val chosen = vm.padColor == i
                Box(Modifier.size(37.dp).clip(CircleShape).background(if (chosen) Color(0xFFEBEBEB) else Color.Transparent).tap { vm.choosePadColor(i) }, contentAlignment = Alignment.Center) {
                    Box(Modifier.size(19.dp).clip(CircleShape).background(CanvasSpace.color(c)))
                }
            }
        }
        Spacer(Modifier.height(18.dp))
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(PadMenuGroup)) {
            Box(Modifier.fillMaxWidth().height(45.dp).padding(start = 16.dp), contentAlignment = Alignment.CenterStart) {
                Text("Stylus", color = PadMenuText, fontSize = 17.sp, maxLines = 1)
            }
            Box(Modifier.fillMaxWidth().height(1.dp).background(PadMenuLine))
            Row(
                Modifier.fillMaxWidth().height(45.dp).tap { vm.pad3d = !vm.pad3d }.padding(start = 16.dp, end = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("3D preview", color = PadMenuText, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
                Switch(vm.pad3d, enabled = true)
            }
        }
        Spacer(Modifier.height(10.dp))
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(PadMenuGroup)) {
            Box(Modifier.fillMaxWidth().height(41.dp).tap { vm.clearPad() }.padding(start = 16.dp), contentAlignment = Alignment.CenterStart) {
                Text("Clear drawing pad", color = PadMenuText, fontSize = 17.sp, maxLines = 1)
            }
            Box(Modifier.fillMaxWidth().height(1.dp).background(PadMenuLine))
            Box(Modifier.fillMaxWidth().height(41.dp).tap { vm.resetStudio() }.padding(start = 16.dp), contentAlignment = Alignment.CenterStart) {
                Text("Reset all brush settings", color = PadMenuText, fontSize = 17.sp, maxLines = 1)
            }
        }
    }
}

/** The pad menu's thin slider: blue up to a white knob, jumping to the touch. */
@Composable
private fun PadSizeSlider(value: Float, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    Box(
        Modifier.fillMaxWidth().height(28.dp).padding(horizontal = 10.dp).pointerInput(Unit) {
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
        }.drawBehind {
            val y = size.height / 2f
            val x = size.width * value.coerceIn(0f, 1f)
            drawLine(Color(0xFFA9A9A9), Offset(0f, y), Offset(size.width, y), 3.dp.toPx(), cap = StrokeCap.Round)
            drawLine(Pc.Accent, Offset(0f, y), Offset(x, y), 3.dp.toPx(), cap = StrokeCap.Round)
            val knob = Size(28.dp.toPx(), 17.dp.toPx())
            val kx = x.coerceIn(knob.width / 2, size.width - knob.width / 2)
            drawRoundRect(Color(0x33000000), Offset(kx - knob.width / 2, y - knob.height / 2 + 1.dp.toPx()), knob, CornerRadius(knob.height / 2))
            drawRoundRect(Color.White, Offset(kx - knob.width / 2, y - knob.height / 2), knob, CornerRadius(knob.height / 2))
        },
    )
}

/**
 * What a slider's keypad offers besides typing: its own pressure curve ([curve], none while off) and
 * its own tilt (its switch, if it has one, and the angle the tilt starts from, 0..1 of a quarter
 * turn). Each setter changes the brush at once.
 */
class KeypadExtras(
    val pressure: Boolean,
    val curve: () -> List<Offset>?,
    val setCurve: (List<Offset>?) -> Unit,
    val tilt: Boolean,
    val tiltOn: () -> Boolean?,
    val setTiltOn: (Boolean) -> Unit,
    val tiltAngle: () -> Float,
    val setTiltAngle: (Float) -> Unit,
)

/** What studio control [key]'s keypad offers besides typing, as its brush holds it; none if nothing. */
private fun keypadExtras(vm: CanvasViewModel, key: String): KeypadExtras? {
    val e = sliderExtras(key)
    if (!e.pressure && !e.tilt) return null
    return KeypadExtras(
        pressure = e.pressure,
        curve = { vm.studioSliderCurve(key) },
        setCurve = { vm.setStudioSliderCurve(key, it) },
        tilt = e.tilt,
        tiltOn = { e.tiltSwitch.takeIf { it.isNotEmpty() }?.let { vm.studio(it) >= 0.5f } },
        setTiltOn = { on -> if (e.tiltSwitch.isNotEmpty()) vm.setStudio(e.tiltSwitch, if (on) 1f else 0f) },
        tiltAngle = { vm.studio(e.tiltAngle) },
        setTiltAngle = { vm.setStudio(e.tiltAngle, it) },
    )
}

/** A labelled slider with its value in a pill: blue up to the knob, or out from the middle both ways. */
@Composable
internal fun StudioSlider(
    label: String,
    value: Float,
    shown: Shown,
    most: Float = 16f,
    enabled: Boolean = true,
    extras: KeypadExtras? = null,
    onChange: (Float) -> Unit,
) {
    val change by rememberUpdatedState(onChange)
    val current by rememberUpdatedState(value)
    val both = shown == Shown.BOTH || shown == Shown.FOLLOW || shown == Shown.GRADE
    // a tap on the value types it on a keypad beside it (the pill blue meanwhile)
    val typed = typedNumber(shown, most)
    var typing by remember { mutableStateOf(false) }
    var pill by remember { mutableStateOf(androidx.compose.ui.geometry.Rect.Zero) }
    Column(Modifier.fillMaxWidth().height(55.dp)) {
        // the name and value 2 below the top half's middle
        Row(Modifier.offset(y = 2.dp).fillMaxWidth().height(28.dp).padding(start = 10.dp, end = 3.5.dp), verticalAlignment = Alignment.CenterVertically) {
            FittedLabel(label, Modifier.weight(1f).padding(end = 12.dp))
            // as wide as its words need, 13 either side, and never narrower than 73
            Box(
                Modifier.height(28.dp).widthIn(min = 73.dp).onGloballyPositioned { pill = it.boundsInWindow() }.clip(RoundedCornerShape(14.dp))
                    .background(if (typing) Pc.Accent else if (enabled) StudioPill else StudioPillOff)
                    .then(if (enabled) Modifier.tap { typing = true } else Modifier)
                    .padding(horizontal = 13.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(shownValue(value, shown, most), color = if (typing) Color.White else if (enabled) StudioLabel else StudioValueOff, fontSize = 16.sp, maxLines = 1, softWrap = false)
                if (typing) SliderKeypad(label, pill, typed, { change(it) }, extras) { typing = false }
            }
        }
        Box(
            Modifier.fillMaxWidth().height(20.dp).padding(start = 10.dp, end = 10.5.dp).pointerInput(both, enabled) {
                if (!enabled) return@pointerInput
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    var last = down.position.x
                    var v = current
                    while (true) {
                        val ev = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!ev.pressed) break
                        // relative: the knob follows the drag, as far as it travels
                        val span = if (both) 2f else 1f
                        val travel = (size.width - 2 * KNOB_INSET.toPx()).coerceAtLeast(1f)
                        v = (v + (ev.position.x - last) / travel * span).coerceIn(if (both) -1f else 0f, 1f)
                        last = ev.position.x
                        change(v)
                        ev.consume()
                    }
                }
            }.drawBehind {
                val y = size.height / 2f
                val t = (if (both) (value + 1f) / 2f else value).coerceIn(0f, 1f)
                // the knob's middle travels from 5 in at either end, so it overhangs the track by 3
                val inset = KNOB_INSET.toPx()
                val x = inset + (size.width - 2 * inset) * t
                val line = 3.dp.toPx()
                // a two-way slider's middle: a tick across the track
                if (both) drawLine(StudioTrack, Offset(size.width / 2f, y - 10.dp.toPx()), Offset(size.width / 2f, y + 10.dp.toPx()), 1.dp.toPx())
                drawLine(StudioTrack, Offset(0f, y), Offset(size.width, y), line)
                if (enabled) drawLine(Pc.Accent, Offset(if (both) size.width / 2f else 0f, y), Offset(x, y), line)
                val knob = Size(16.dp.toPx(), 10.dp.toPx())
                drawRoundRect(if (enabled) Color.White else StudioKnobOff, Offset(x - knob.width / 2, y - knob.height / 2), knob, CornerRadius(knob.height / 2))
            },
        )
    }
}

/**
 * How a slider's value is typed: the largest number, whether it may be negative (a "-" key), and what
 * a number makes of the value.
 */
private class Typed(val most: Int, val signed: Boolean, val toValue: (Int) -> Float)

/**
 * A slider's [Typed]: a percent of the slider (or of its doubled scale), both ways for the controls
 * going both ways, the count itself, or for a size the percent the slider reads (of [most]).
 */
private fun typedNumber(shown: Shown, most: Float): Typed = when (shown) {
    Shown.PERCENT, Shown.ZERO, Shown.TIP, Shown.MOVEMENT, Shown.ZOOM, Shown.CHARGE, Shown.METAL, Shown.GLOSS -> Typed(100, false) { n -> n / 100f }
    Shown.DOUBLE, Shown.TWICE -> Typed(200, false) { n -> n / 200f }
    Shown.BOTH, Shown.FOLLOW, Shown.GRADE -> Typed(100, true) { n -> n / 100f }
    Shown.COUNT -> Typed(16, false) { n -> (n.coerceAtLeast(1) - 1) / 15f }
    Shown.SIZE -> Typed((most * 100f).roundToInt(), false) { n -> ((n / 100f) / most.coerceAtLeast(1e-3f)).coerceIn(0f, 1f).toDouble().pow(1.0 / 2.2).toFloat() }
}

private val KeypadFill = Color(0xFFD5D5D5)
private val KeypadKey = Color(0xFFE6E6E6)
private val KeypadInk = Color(0xFF1D1D1D)
private val KeypadWidth = 275.dp
private val KeypadHeight = 360.dp
/**
 * With pages to choose between, the tabs under the title push the keys down, 64 tall: the popover
 * is 410 tall with two tabs, 386 with three (as the reference's measure).
 */
private fun keypadHeight(tabs: Int) = when (tabs) {
    1 -> KeypadHeight
    2 -> 410.dp
    else -> 386.dp
}
private fun keypadKeyHeight(tabs: Int) = if (tabs > 1) 64.dp else 66.dp
private val KeypadTabs = Color(0xFFC1C1C1)
private val KeypadTabChosen = Color(0xFF0A8CFD)
private val KeypadTabInk = Color(0xFF393939)
private val KeypadGraph = Color(0xFFE2E2E2)
private val KeypadGraphEdge = Color(0xFF999999)
private val KeypadGraphGrid = Color(0xFFD0D0D0)
private val KeypadSwitchOff = Color(0xFFC2C1C5)

/** A slider keypad's pages: typing the value, the slider's own pressure curve, its own tilt. */
private enum class KeypadPage { NUMERIC, PRESSURE, TILT }

/**
 * A slider's keypad, a light popover to the right of its value ([pill], window pixels), its arrow
 * at the value: the slider's name, then 7 8 9 / 4 5 6 / 1 2 3 / 0 and backspace (81 x 66 keys),
 * "-" 0 and backspace for a value that may be negative. Each key sets the value at once: the digits
 * typed so far are the number (beyond the most, the most); "-" makes it negative; backspace takes
 * the last one off (none left: nothing).
 */
@Composable
private fun SliderKeypad(
    title: String,
    pill: androidx.compose.ui.geometry.Rect,
    typed: Typed,
    onValue: (Float) -> Unit,
    extras: KeypadExtras?,
    onDismiss: () -> Unit,
) {
    // tabs over the keys when the slider has a pressure curve or a tilt of its own
    val pages = buildList {
        add(KeypadPage.NUMERIC)
        if (extras?.pressure == true) add(KeypadPage.PRESSURE)
        if (extras?.tilt == true) add(KeypadPage.TILT)
    }
    var page by remember { mutableStateOf(KeypadPage.NUMERIC) }
    val height = keypadHeight(pages.size)
    var text by remember { mutableStateOf("") }
    fun type(next: String) {
        // a sign alone waits for its digits
        if (next == "-") {
            text = next
            return
        }
        val n = next.toIntOrNull()?.coerceIn(-typed.most, typed.most) ?: 0
        text = if (next.isEmpty()) "" else n.toString()
        onValue(typed.toValue(n))
    }
    val density = LocalDensity.current
    val screen = with(density) { androidx.compose.ui.platform.LocalView.current.rootView.height.toDp() }
    val (top, arrowAt) = with(density) {
        val middle = (pill.top + pill.height / 2f).toDp()
        val top = (middle - height / 2).coerceIn(2.dp, (screen - height - 2.dp).coerceAtLeast(2.dp))
        top to (middle - top)
    }
    // the arrow's tip a little into the pill, the body 3 past its end
    val offset = with(density) { androidx.compose.ui.unit.IntOffset((pill.width - (Pc.ArrowHeight - 3.dp).toPx()).roundToInt(), (top.toPx() - pill.top).roundToInt()) }
    PcPopup(
        offset = offset,
        onDismissRequest = onDismiss,
        properties = androidx.compose.ui.window.PopupProperties(focusable = true, clippingEnabled = false),
    ) {
        Popover(KeypadWidth, arrowAt, Modifier.height(height), fill = KeypadFill, corner = 22.dp, side = ArrowSide.LEFT) {
            Text(title, color = KeypadInk, fontSize = 22.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.padding(start = 25.dp, top = 19.dp).height(if (pages.size > 1) 43.dp else 51.dp))
            if (pages.size > 1) {
                KeypadTabRow(pages, page) { page = it }
                Spacer(Modifier.height(12.dp))
            }
            when (page) {
                KeypadPage.NUMERIC -> Column(Modifier.padding(start = 12.dp), verticalArrangement = Arrangement.spacedBy(5.dp)) {
                    val keyHeight = keypadKeyHeight(pages.size)
                    for (row in listOf(listOf("7", "8", "9"), listOf("4", "5", "6"), listOf("1", "2", "3"))) {
                        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) { for (k in row) KeypadKey(k, 81.dp, keyHeight) { type(text + k) } }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        if (typed.signed) {
                            KeypadKey("-", 81.dp, keyHeight) { type(if (text.startsWith("-")) text.drop(1) else "-$text") }
                            KeypadKey("0", 81.dp, keyHeight) { type(text + "0") }
                        } else {
                            KeypadKey("0", 166.dp, keyHeight) { type(text + "0") }
                        }
                        KeypadKey("⌫", 81.dp, keyHeight) { type(text.dropLast(1)) }
                    }
                }
                KeypadPage.PRESSURE -> if (extras != null) KeypadPressure(extras)
                KeypadPage.TILT -> if (extras != null) KeypadTilt(extras)
            }
        }
    }
}

/** The keypad's tabs under its title: an icon for each page, the chosen one on blue. */
@Composable
private fun KeypadTabRow(pages: List<KeypadPage>, chosen: KeypadPage, onChoose: (KeypadPage) -> Unit) {
    Row(Modifier.padding(start = 12.dp).size(251.dp, 30.dp).clip(RoundedCornerShape(9.dp)).background(KeypadTabs).padding(2.dp)) {
        for (p in pages) {
            Box(
                Modifier.weight(1f).fillMaxHeight().clip(RoundedCornerShape(7.dp)).background(if (p == chosen) KeypadTabChosen else Color.Transparent).tap { onChoose(p) },
                contentAlignment = Alignment.Center,
            ) {
                val icon = when (p) {
                    KeypadPage.NUMERIC -> PcIcons.Keypad
                    KeypadPage.PRESSURE -> PcIcons.PenPressure
                    KeypadPage.TILT -> PcIcons.PenTilt
                }
                Icon(icon, contentDescription = p.name.lowercase().replaceFirstChar { it.uppercase() }, tint = if (p == chosen) Color.White else KeypadTabInk, modifier = Modifier.requiredSize(30.dp))
            }
        }
    }
}

/** A page's name and its switch, in a row across the keypad. */
@Composable
private fun KeypadSwitchRow(label: String, on: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.padding(start = 18.dp, end = 8.dp).fillMaxWidth().height(28.dp).tap { onChange(!on) }, verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = KeypadInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
        Switch(on, enabled = true, offTrack = KeypadSwitchOff)
    }
}

/** The pressure page: the slider's own curve, on or off, drawn on a light grid. */
@Composable
private fun KeypadPressure(extras: KeypadExtras) {
    var curve by remember { mutableStateOf(extras.curve()) }
    Column {
        Spacer(Modifier.height(6.dp))
        KeypadSwitchRow("Pressure", curve != null) { on ->
            val next = if (on) listOf(Offset(0f, 0f), Offset(1f, 1f)) else null
            curve = next
            extras.setCurve(next)
        }
        Spacer(Modifier.height(12.dp))
        PressureCurve(
            curve ?: emptyList(),
            modifier = Modifier.padding(start = 18.dp).size(239.dp, 219.dp),
            grid = KeypadGraphGrid,
            edge = KeypadGraphEdge,
            fill = KeypadGraph,
            line = Pc.Accent,
            lineWidth = 1.5.dp,
        ) { points ->
            if (curve != null) {
                curve = points
                extras.setCurve(points)
            }
        }
    }
}

/** The tilt page: the slider's own tilt switch (if it has one) over the angle its tilt starts from. */
@Composable
private fun KeypadTilt(extras: KeypadExtras) {
    var on by remember { mutableStateOf(extras.tiltOn()) }
    var angle by remember { mutableStateOf(extras.tiltAngle()) }
    Column {
        Spacer(Modifier.height(6.dp))
        on?.let { now ->
            KeypadSwitchRow("Tilt", now) {
                on = it
                extras.setTiltOn(it)
            }
        }
        Spacer(Modifier.height(8.dp))
        Box(Modifier.padding(start = 10.dp).size(255.dp, 209.dp).background(KeypadFill).padding(5.dp)) {
            TiltAngle(angle, LightTilt, active = on ?: true) {
                angle = it
                extras.setTiltAngle(it)
            }
        }
    }
}

@Composable
private fun KeypadKey(label: String, width: Dp, height: Dp = 66.dp, onClick: () -> Unit) {
    Box(Modifier.size(width, height).clip(RoundedCornerShape(12.dp)).background(KeypadKey).tap(onClick = onClick), contentAlignment = Alignment.Center) {
        if (label == "⌫") {
            Icon(PcIcons.Backspace, contentDescription = "Back space", tint = KeypadInk, modifier = Modifier.size(26.dp))
        } else {
            Text(label, color = KeypadInk, fontSize = 29.sp)
        }
    }
}

/** A slider's name, 17 sp, made smaller (to 60%) when it would not fit beside the value. */
@Composable
private fun FittedLabel(label: String, modifier: Modifier) {
    val measurer = androidx.compose.ui.text.rememberTextMeasurer()
    androidx.compose.foundation.layout.BoxWithConstraints(modifier) {
        val room = constraints.maxWidth
        val size = remember(label, room) {
            fun wide(sp: Float) = measurer.measure(label, androidx.compose.ui.text.TextStyle(fontSize = sp.sp), maxLines = 1, softWrap = false).size.width
            var sp = 17f
            while (sp > 17f * 0.6f && wide(sp) > room) sp -= 0.5f
            sp
        }
        Text(label, color = StudioLabel, fontSize = size.sp, maxLines = 1, softWrap = false)
    }
}

/** How far in from the track's ends a studio slider's knob stops. */
private val KNOB_INSET = 5.dp

/**
 * The taper bar: a box inset as far as the sliders' knobs travel, the bar across it thinning to
 * points at its ends as the start and end handles (on lines across the box) move in, each up to
 * half way.
 */
@Composable
private fun TaperBar(start: Float, end: Float, onStart: (Float) -> Unit, onEnd: (Float) -> Unit) {
    val s by rememberUpdatedState(start)
    val e by rememberUpdatedState(end)
    val setStart by rememberUpdatedState(onStart)
    val setEnd by rememberUpdatedState(onEnd)
    Canvas(
        Modifier.fillMaxWidth().height(72.dp).pointerInput(Unit) {
            awaitEachGesture {
                val down = awaitFirstDown()
                down.consume()
                val from = TAPER_TRAVEL.toPx()
                val half = (size.width - 2 * from) / 2f
                // the nearer handle
                val startX = from + s * half
                val endX = size.width - from - e * half
                val grabStart = kotlin.math.abs(down.position.x - startX) <= kotlin.math.abs(down.position.x - endX)
                while (true) {
                    val ev = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                    if (!ev.pressed) break
                    if (grabStart) setStart(((ev.position.x - from) / half).coerceIn(0f, 1f)) else setEnd(((size.width - from - ev.position.x) / half).coerceIn(0f, 1f))
                    ev.consume()
                }
            }
        },
    ) {
        val left = TAPER_TRAVEL.toPx() - 1.dp.toPx()
        val right = size.width - left
        drawRoundRect(TaperBox, Offset(left, 0f), Size(right - left, size.height), CornerRadius(5.dp.toPx()))
        val y = size.height / 2f
        val half = (size.width - 2 * TAPER_TRAVEL.toPx()) / 2f
        val startX = TAPER_TRAVEL.toPx() + start * half
        val endX = size.width - TAPER_TRAVEL.toPx() - end * half
        val thick = 7.dp.toPx()
        val bar = Path().apply {
            moveTo(left, y)
            lineTo(startX, y - thick)
            lineTo(endX, y - thick)
            lineTo(right, y)
            lineTo(endX, y + thick)
            lineTo(startX, y + thick)
            close()
        }
        drawPath(bar, TaperBarFill)
        for (x in listOf(startX, endX)) {
            drawLine(Color.White, Offset(x, 0f), Offset(x, size.height), 2.dp.toPx())
            drawCircle(Color.White, 6.5.dp.toPx(), Offset(x, y))
            drawCircle(Pc.Accent, 4.dp.toPx(), Offset(x, y))
        }
    }
}

/** How far in from the row's ends the taper handles stop: where the sliders' knobs do. */
private val TAPER_TRAVEL = 15.dp
private val TaperBox get() = if (Pc.light) Color(0xFFEBEBEB) else Color(0xFF212121)
/** The studio's pressure curve: a darker box with a light edge, a grey curve, small handles. */
private val StudioCurveFill get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0xFF1B1B1B)
private val StudioCurveEdge get() = if (Pc.light) Color(0xFFD8D8D8) else Color(0xFF353535)
private val StudioCurveLine get() = if (Pc.light) Color(0xFF3A3A3A) else Color(0xFFD5D5D5)
private val TaperBarFill get() = if (Pc.light) Color(0xFFDCDCDC) else Color(0xFF363636)

/**
 * How the tilt angle's graph is coloured: its box, the quarter disc, the arc, the line, the angle's
 * label, and its handle's middle while the tilt is off.
 */
internal class TiltLook(val box: Color, val quarter: Color, val arc: Color, val line: Color, val label: Color, val off: Color, val fill: Boolean = true)

private val DarkTilt = TiltLook(Color(0xFF1B1B1B), Color(0xFF262626), Color(0xFF4A4A4A), Color(0xFF6E6E6E), Color(0xFF8E8E8E), Color(0xFF6E6E6E))
/** In the keypads: the disc darker than the box round it, as the reference's. */
private val LightTilt = TiltLook(Color(0xFFE6E6E6), Color(0xFFDADADA), Color(0xFFABABAB), Color(0xFF797979), Color(0xFFAAAAAA), Color(0xFFAAAAAA), fill = false)

/**
 * The tilt angle: a quarter disc from the bottom-left corner with a line at the angle (0 flat, 1
 * upright) and its handle on the arc; drag anywhere to turn it.
 */
@Composable
internal fun TiltAngle(value: Float, look: TiltLook = DarkTilt, active: Boolean = true, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    val degrees = (value.coerceIn(0f, 1f) * 90f).roundToInt()
    Box(
        // not clipped: the handle sits on the box's edge, whole
        if (look.fill) Modifier.padding(horizontal = 5.dp).fillMaxWidth().aspectRatio(271.5f / 221f).background(look.box, RoundedCornerShape(6.dp))
        else Modifier.fillMaxSize().background(look.box),
    ) {
        Canvas(
            Modifier.fillMaxSize().pointerInput(Unit) {
                // the angle of the touch from the bottom-left corner, a quarter turn at most
                fun angleAt(p: Offset) = (kotlin.math.atan2((size.height - p.y).toDouble(), p.x.toDouble()) / (Math.PI / 2)).toFloat().coerceIn(0f, 1f)
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    change(angleAt(down.position))
                    while (true) {
                        val ev = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!ev.pressed) break
                        change(angleAt(ev.position))
                        ev.consume()
                    }
                }
            },
        ) {
            val corner = Offset(0f, size.height)
            val r = size.height
            clipRect {
                drawCircle(look.quarter, r, corner)
                drawCircle(look.arc, r, corner, style = Stroke(1.dp.toPx()))
            }
            val a = value.coerceIn(0f, 1f) * (Math.PI / 2)
            val dir = Offset(kotlin.math.cos(a).toFloat(), -kotlin.math.sin(a).toFloat())
            // the line runs on to the box's edge; the handle sits on the arc
            val reach = minOf(if (dir.x > 1e-4f) size.width / dir.x else Float.MAX_VALUE, if (dir.y < -1e-4f) size.height / -dir.y else Float.MAX_VALUE)
            drawLine(look.line, corner, corner + dir * reach, 1.dp.toPx())
            drawCircle(Color.White, 5.5.dp.toPx(), corner + dir * r)
            drawCircle(if (active) Pc.Accent else look.off, 3.5.dp.toPx(), corner + dir * r)
        }
        Text("$degrees°", color = look.label, fontSize = 17.sp, modifier = Modifier.align(Alignment.TopEnd).padding(top = 7.dp, end = 9.dp))
    }
}

/**
 * The shape's roundness and angle: an ellipse on crossed lines, its blue handles (the ends of its
 * short axis) dragged in and out for the roundness, its green handle (the end of its long axis)
 * dragged round for the angle, shown in degrees.
 */
@Composable
internal fun RoundnessGraph(roundness: Float, angle: Float, onRoundness: (Float) -> Unit, onAngle: (Float) -> Unit) {
    val setRound by rememberUpdatedState(onRoundness)
    val setAngle by rememberUpdatedState(onAngle)
    val round by rememberUpdatedState(roundness.coerceIn(0.05f, 1f))
    val turn by rememberUpdatedState(angle)
    val degrees = ((angle.coerceIn(0f, 1f) * 360f).roundToInt()) % 360
    Box(
        Modifier.fillMaxWidth().aspectRatio(283f / 231.5f)
            .clip(RoundedCornerShape(6.dp)).background(TiltBox),
    ) {
        Canvas(
            Modifier.fillMaxSize().pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    val c = Offset(size.width / 2f, size.height / 2f)
                    val r = size.height * 78f / 231.5f
                    val a = turn * 2f * Math.PI.toFloat()
                    val long = Offset(kotlin.math.cos(a), -kotlin.math.sin(a))
                    val short = Offset(-long.y, long.x)
                    // the green handle turns it; anything else squashes it
                    val turning = (down.position - (c + long * r)).getDistance() < 28.dp.toPx()
                    while (true) {
                        val ev = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!ev.pressed) break
                        val d = ev.position - c
                        if (turning) {
                            val t = (kotlin.math.atan2(-d.y, d.x) / (2 * Math.PI)).toFloat()
                            setAngle(if (t < 0f) t + 1f else t)
                        } else {
                            val along = kotlin.math.abs(d.x * short.x + d.y * short.y)
                            setRound((along / r).coerceIn(0.05f, 1f))
                        }
                        ev.consume()
                    }
                }
            },
        ) {
            val c = Offset(size.width / 2f, size.height / 2f)
            val r = size.height * 78f / 231.5f
            // the crossed lines show dimly through the ellipse's fill
            drawLine(RoundnessCross, Offset(0f, c.y), Offset(size.width, c.y), 1.dp.toPx())
            drawLine(RoundnessCross, Offset(c.x, 0f), Offset(c.x, size.height), 1.dp.toPx())
            val a = turn.coerceIn(0f, 1f) * 360f
            withTransform({ rotate(-a, c) }) {
                val oval = androidx.compose.ui.geometry.Size(2 * r, 2 * r * round)
                val at = Offset(c.x - r, c.y - r * round)
                drawOval(RoundnessFill, at, oval)
                drawOval(RoundnessEdge, at, oval, style = Stroke(1.dp.toPx()))
            }
            val rad = Math.toRadians(a.toDouble())
            val long = Offset(kotlin.math.cos(rad).toFloat(), -kotlin.math.sin(rad).toFloat())
            val short = Offset(-long.y, long.x)
            for (end in listOf(c + short * (r * round), c - short * (r * round))) {
                drawCircle(Color.White, 5.6.dp.toPx(), end)
                drawCircle(Pc.Accent, 3.7.dp.toPx(), end)
            }
            drawCircle(Color.White, 5.6.dp.toPx(), c + long * r)
            drawCircle(RoundnessGreen, 3.7.dp.toPx(), c + long * r)
        }
        Text("$degrees°", color = TiltLabel, fontSize = 17.sp, modifier = Modifier.align(Alignment.TopEnd).padding(top = 12.dp, end = 14.dp))
    }
}

private val RoundnessGreen = Color(0xFF42E347)
private val RoundnessCross get() = if (Pc.light) Color(0xFFC0C0C0) else Color(0xFF4C4C4C)
private val RoundnessFill get() = if (Pc.light) Color(0xD6F0F0F0) else Color(0xD6232323)
private val RoundnessEdge get() = if (Pc.light) Color(0xFFC8C8C8) else Color(0xFF565656)
private val TiltBox get() = if (Pc.light) Color(0xFFFFFFFF) else Color(0xFF1B1B1B)
private val TiltLabel get() = if (Pc.light) Color(0xFF808080) else Color(0xFF8E8E8E)

@Composable
internal fun PressureCurve(
    points: List<Offset>,
    modifier: Modifier = Modifier.padding(horizontal = 8.dp).fillMaxWidth().aspectRatio(1f),
    grid: Color = Color(0xFF3A3A3A),
    edge: Color = grid,
    /** The box's own fill and corners, the curve's colour and width, and its handles' radii (ring, middle). */
    fill: Color = Color.Transparent,
    corner: Dp = 0.dp,
    line: Color = Color.White,
    lineWidth: Dp = 1.5.dp,
    handle: Pair<Dp, Dp> = 7.dp to 5.dp,
    onChange: (List<Offset>) -> Unit,
) {
    val pts by rememberUpdatedState(points)
    val change by rememberUpdatedState(onChange)
    val samples = remember(points) { curveSamples(points.flatMap { listOf(it.x, it.y) }, 129u) }
    // the node whose menu shows (a tap on a node), and the box's size for placing it
    var menu by remember { mutableStateOf<Int?>(null) }
    var box by remember { mutableStateOf(androidx.compose.ui.unit.IntSize.Zero) }
    Box(modifier.onSizeChanged { box = it }) {
    Canvas(
        Modifier.fillMaxSize().pointerInput(Unit) {
            awaitEachGesture {
                val down = awaitFirstDown()
                down.consume()
                menu = null
                // no curve (switched off): nothing to touch
                if (pts.isEmpty()) return@awaitEachGesture
                fun toCurve(p: Offset) = Offset((p.x / size.width).coerceIn(0f, 1f), (1f - p.y / size.height).coerceIn(0f, 1f))
                val at = toCurve(down.position)
                var list = pts.toMutableList()
                var index = list.indexOfFirst { (it - at).getDistance() < 0.05f }
                val added = index < 0
                if (added) {
                    list.add(at)
                    list.sortBy { it.x }
                    index = list.indexOf(at)
                    change(list.toList())
                }
                var moved = false
                while (true) {
                    val ev = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                    if (!ev.pressed) break
                    if (!moved && (ev.position - down.position).getDistance() < viewConfiguration.touchSlop) continue
                    moved = true
                    val p = toCurve(ev.position)
                    val lo = list.getOrNull(index - 1)?.x ?: 0f
                    val hi = list.getOrNull(index + 1)?.x ?: 1f
                    list = list.toMutableList().also { it[index] = Offset(p.x.coerceIn(lo, hi), p.y) }
                    change(list.toList())
                    ev.consume()
                }
                // a tap on a node shows what can be done with it
                if (!moved && !added) menu = index
            }
        },
    ) {
        val w = size.width
        val h = size.height
        val round = CornerRadius(corner.toPx())
        drawRoundRect(fill, cornerRadius = round)
        for (k in 1..3) {
            drawLine(grid, Offset(w * k / 4f, 0f), Offset(w * k / 4f, h), 1.dp.toPx())
            drawLine(grid, Offset(0f, h * k / 4f), Offset(w, h * k / 4f), 1.dp.toPx())
        }
        drawRoundRect(edge, cornerRadius = round, style = Stroke(1.dp.toPx()))
        if (points.isEmpty()) return@Canvas
        val sorted = points.sortedBy { it.x }
        val path = Path()
        val last = (samples.size - 1).coerceAtLeast(1)
        samples.forEachIndexed { i, v ->
            val x = w * i / last
            val y = (1f - v) * h
            if (i == 0) path.moveTo(x, y) else path.lineTo(x, y)
        }
        drawPath(path, line, style = Stroke(lineWidth.toPx(), cap = StrokeCap.Round))
        for (p in sorted) {
            drawCircle(Color.White, handle.first.toPx(), Offset(p.x * w, (1f - p.y) * h))
            drawCircle(Pc.Accent, handle.second.toPx(), Offset(p.x * w, (1f - p.y) * h))
        }
    }
    menu?.let { i -> NodeMenu(points.sortedBy { it.x }, i, box, { menu = null }, change) }
    }
}

/**
 * The menu over node [index] of a curve through [points] (the box [box] pixels): Delete (a node
 * between the ends) and Reset (the curve straight again).
 */
@Composable
private fun NodeMenu(points: List<Offset>, index: Int, box: androidx.compose.ui.unit.IntSize, close: () -> Unit, change: (List<Offset>) -> Unit) {
    val at = points.getOrNull(index) ?: return
    val end = index == 0 || index == points.lastIndex
    val rows = if (end) listOf("Reset") else listOf("Delete", "Reset")
    // centred over the node, its bottom 11 above it; in a window of its own, so the column's edge
    // doesn't cut it (with room round it for its shadow)
    val offset = with(LocalDensity.current) {
        androidx.compose.ui.unit.IntOffset(
            (at.x * box.width - (NodeMenuWidth / 2 + NodeMenuHalo).toPx()).roundToInt(),
            ((1f - at.y) * box.height - (11.dp + NodeMenuRow * rows.size + NodeMenuHalo).toPx()).roundToInt(),
        )
    }
    PcPopup(offset = offset, onDismissRequest = close, properties = androidx.compose.ui.window.PopupProperties(focusable = true, clippingEnabled = false)) {
    Column(
        Modifier.padding(NodeMenuHalo).width(NodeMenuWidth).shadow(14.dp, RoundedCornerShape(12.dp), ambientColor = Color.Black, spotColor = Color.Black)
            .clip(RoundedCornerShape(12.dp)).background(NodeMenuFill),
    ) {
        for (row in rows) {
            Box(
                Modifier.fillMaxWidth().height(NodeMenuRow).tap {
                    close()
                    change(if (row == "Reset") listOf(Offset(0f, 0f), Offset(1f, 1f)) else points.filterIndexed { k, _ -> k != index })
                },
                contentAlignment = Alignment.Center,
            ) {
                Text(row, color = Color.White, fontSize = 17.sp, maxLines = 1)
            }
        }
    }
    }
}

private val NodeMenuFill = Color(0xFF101010)
private val NodeMenuHalo = 16.dp
private val NodeMenuWidth = 70.dp
private val NodeMenuRow = 36.dp
