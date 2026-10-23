package io.github.artcraftandroid.procraft.canvas

import io.github.artcraftandroid.procraft.ui.CanvasSpace
import android.app.Application
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import io.github.artcraftandroid.procraft.gallery.Artwork
import io.github.artcraftandroid.procraft.gallery.ArtworkStore
import io.github.artcraftandroid.procraft.gallery.CanvasPreset
import io.github.artcraftandroid.procraft.core.BackgroundInfo
import io.github.artcraftandroid.procraft.core.BlendModeInfo
import io.github.artcraftandroid.procraft.ui.GradientPreset
import io.github.artcraftandroid.procraft.ui.gradientColorAt
import io.github.artcraftandroid.procraft.core.BrushInfo
import io.github.artcraftandroid.procraft.core.BrushSetInfo
import io.github.artcraftandroid.procraft.core.CanvasSession
import io.github.artcraftandroid.procraft.core.ChromaticKind
import io.github.artcraftandroid.procraft.core.GlitchKind
import io.github.artcraftandroid.procraft.core.GuideInfo
import io.github.artcraftandroid.procraft.core.GuideType
import io.github.artcraftandroid.procraft.core.SymmetryType
import io.github.artcraftandroid.procraft.core.HalftoneKind
import io.github.artcraftandroid.procraft.core.NoiseKind
import io.github.artcraftandroid.procraft.core.QuickShapeInfo
import io.github.artcraftandroid.procraft.core.QuickShapeKind
import io.github.artcraftandroid.procraft.core.GuideSettings
import io.github.artcraftandroid.procraft.core.Interpolation
import io.github.artcraftandroid.procraft.core.LayerInfo
import io.github.artcraftandroid.procraft.core.TextAlignment
import io.github.artcraftandroid.procraft.core.TextInfo
import io.github.artcraftandroid.procraft.core.LayerType
import io.github.artcraftandroid.procraft.core.MobileException
import io.github.artcraftandroid.procraft.core.SelectionDisplay
import io.github.artcraftandroid.procraft.core.SelectionJoin
import io.github.artcraftandroid.procraft.core.SwipeAdjustment
import io.github.artcraftandroid.procraft.core.LiquifyBrush
import io.github.artcraftandroid.procraft.core.LiquifyKind
import io.github.artcraftandroid.procraft.core.ToolKind
import io.github.artcraftandroid.procraft.core.TransformAction
import io.github.artcraftandroid.procraft.core.TransformBox
import io.github.artcraftandroid.procraft.core.TransformHandle
import io.github.artcraftandroid.procraft.core.TransformKind
import io.github.artcraftandroid.procraft.gallery.PALETTE_SLOTS
import io.github.artcraftandroid.procraft.gallery.Palette
import io.github.artcraftandroid.procraft.gallery.PaletteStore
import io.github.artcraftandroid.procraft.gallery.StandardPresets
import io.github.artcraftandroid.procraft.core.blendModes
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlin.math.abs
import kotlin.math.roundToInt
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.text.input.TextFieldValue
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.nio.ByteBuffer

/** Which panel is open over the canvas. */
private const val TAG = "ProCraft"

enum class Panel { NONE, LAYERS, COLOR, BRUSHES, ACTIONS, ADJUSTMENTS }

/** The eyedropper while it is open: the window point (pixels), what is shown around it, the colour it would pick. */
class PickState(val x: Float, val y: Float, val image: ImageBitmap?, val rgb: Int?)

/** The colour panel's ways of choosing a colour. */
enum class ColorMode { DISC, CLASSIC, HARMONY, VALUE, PALETTES }

/** Companion colours in the harmony mode, as hue turns (degrees) from the chosen colour. */
enum class HarmonyKind(val label: String, val turns: List<Float>) {
    COMPLEMENTARY("Complementary", listOf(180f)),
    SPLIT_COMPLEMENTARY("Split Complementary", listOf(150f, 210f)),
    ANALOGOUS("Analogous", listOf(30f, -30f)),
    TRIADIC("Triadic", listOf(120f, 240f)),
    TETRADIC("Tetradic", listOf(90f, 180f, 270f)),
}

/** The gallery of artworks, or one artwork open on the canvas. */
enum class Screen { GALLERY, CANVAS }

/** A popover over the transform toolbar. */
enum class TransformPopup { NONE, SNAPPING, INTERPOLATION }

/**
 * The adjustments, in the menu's order; [ready] once made, [swiped] for those whose one amount is
 * dragged across the canvas.
 */
/**
 * An adjustment: [swiped] ones have one amount dragged across the canvas, which painting it on
 * ([paintable], Pencil) starts at [pencilAmount] (null: as it was).
 */
enum class Adjustment(val label: String, val ready: Boolean = false, val swiped: Boolean = false, val pencilAmount: Float? = null) {
    HSB("Hue, Saturation, Brightness", ready = true),
    COLOR_BALANCE("Color Balance", ready = true),
    CURVES("Curves", ready = true),
    GRADIENT_MAP("Gradient Map", ready = true, swiped = true),
    GAUSSIAN_BLUR("Gaussian Blur", ready = true, swiped = true, pencilAmount = 0.6f),
    MOTION_BLUR("Motion Blur", ready = true, swiped = true, pencilAmount = 0.6f),
    PERSPECTIVE_BLUR("Perspective Blur", ready = true, swiped = true, pencilAmount = 0.4f),
    NOISE("Noise", ready = true, swiped = true, pencilAmount = 0.69f),
    SHARPEN("Sharpen", ready = true, swiped = true, pencilAmount = 0.4f),
    BLOOM("Bloom", ready = true, swiped = true, pencilAmount = 0.5f),
    GLITCH("Glitch", ready = true, swiped = true, pencilAmount = 0.5f),
    HALFTONE("Halftone", ready = true, swiped = true, pencilAmount = 0.12f),
    CHROMATIC_ABERRATION("Chromatic Aberration", ready = true, swiped = true, pencilAmount = 1f),
    LIQUIFY("Liquify", ready = true),
    CLONE("Clone", ready = true),
    ;

    /** Can be painted on with the brush instead of changing the whole layer. */
    val paintable get() = this != LIQUIFY && this != CLONE

    companion object {
        /** The menu's groups: colour, blurs, effects, painting. */
        val groups = listOf(
            listOf(HSB, COLOR_BALANCE, CURVES, GRADIENT_MAP),
            listOf(GAUSSIAN_BLUR, MOTION_BLUR, PERSPECTIVE_BLUR),
            listOf(NOISE, SHARPEN, BLOOM, GLITCH, HALFTONE, CHROMATIC_ABERRATION),
            listOf(LIQUIFY, CLONE),
        )
    }
}

/** The selection tool's ways of making a selection. */
enum class SelectionKind(val label: String) { AUTOMATIC("Automatic"), FREEHAND("Freehand"), RECTANGLE("Rectangle"), ELLIPSE("Ellipse") }

/** A popover over the selection toolbar. */
enum class SelectionPopup { NONE, FEATHER, SAVED }

/** The gallery, the open artwork and the state of the controls around it; outlives configuration changes. */
class CanvasViewModel(app: Application) : AndroidViewModel(app) {
    var session: CanvasSession? = null
        private set

    /** Frosted panels open over the canvas, for the engine to blur what is behind them. */
    val backdrops = io.github.artcraftandroid.procraft.ui.Backdrops { session?.setBackdrops(it) }

    var screen by mutableStateOf(Screen.GALLERY)
        private set
    val store = ArtworkStore(File(app.filesDir, "artworks"))
    var artworks by mutableStateOf<List<Artwork>>(emptyList())
        private set

    /** What the gallery shows: artworks and stacks (in the stack open, only its artworks). */
    var galleryEntries by mutableStateOf<List<io.github.artcraftandroid.procraft.gallery.GalleryEntry>>(emptyList())
        private set

    /** The stack open in the gallery, if any. */
    var openStack by mutableStateOf<io.github.artcraftandroid.procraft.gallery.Stack?>(null)
        private set

    /** Bumped when artworks are saved, so thumbnails reload. */
    var galleryVersion by mutableStateOf(0)
        private set
    var opening by mutableStateOf(false)
        private set
    var message by mutableStateOf<String?>(null)
    private var artworkDir: File? = null

    var tool by mutableStateOf(ToolKind.PAINT)
        private set
    var size by mutableFloatStateOf(0f)
        private set
    var opacity by mutableFloatStateOf(1f)
        private set

    /** The paint colour as hue (0..1), saturation and brightness. */
    var hue by mutableFloatStateOf(DEFAULT_HUE)
        private set
    var saturation by mutableFloatStateOf(1f)
        private set
    var brightness by mutableFloatStateOf(1f)
        private set

    private var secondaryState by mutableStateOf(0xFFFFFF)

    /**
     * The second colour (0xRRGGBB), beside the current one in the colour panel: swapped with it,
     * and mixed in by brushes whose colour dynamics say so.
     */
    var secondaryRgb: Int
        get() = secondaryState
        private set(v) {
            secondaryState = v
            pushSecondary()
        }

    private fun pushSecondary() {
        val c = secondaryState
        session?.setSecondaryColor(((c shr 16) and 0xFF) / 255f, ((c shr 8) and 0xFF) / 255f, (c and 0xFF) / 255f)
    }

    /** Colours painted with recently (0xRRGGBB), newest first. */
    var history by mutableStateOf<List<Int>>(emptyList())
        private set

    /** Whether the size and opacity side bar shows. */
    var showSideBar by mutableStateOf(true)

    private val interfacePrefs = app.getSharedPreferences("interface", android.content.Context.MODE_PRIVATE)

    /** The colour panel's mode, kept from one session to the next. */
    private var colorModeState by mutableStateOf(ColorMode.entries.getOrElse(interfacePrefs.getInt(COLOR_MODE, ColorMode.DISC.ordinal)) { ColorMode.DISC })
    var colorMode: ColorMode
        get() = colorModeState
        set(mode) {
            colorModeState = mode
            interfacePrefs.edit().putInt(COLOR_MODE, mode.ordinal).apply()
        }

    /** The side bar on the right edge instead of the left. */
    var rightHanded by mutableStateOf(interfacePrefs.getBoolean(RIGHT_HANDED, false))
        private set

    /** How long two fingers are held before undo repeats, seconds. */
    var rapidUndoDelay by mutableStateOf(interfacePrefs.getFloat(RAPID_UNDO_DELAY, 0.65f))
        private set

    /** Brush cursor: the brush's outline where the pen paints or hovers. */
    var brushCursorOn by mutableStateOf(interfacePrefs.getBoolean(BRUSH_CURSOR, true))
        private set

    fun changeBrushCursor(on: Boolean) {
        brushCursorOn = on
        if (!on) cursorAt = null
        interfacePrefs.edit().putBoolean(BRUSH_CURSOR, on).apply()
    }

    /** Where the brush cursor is (window pixels), and its radius there; none while not shown. */
    var cursorAt by mutableStateOf<Offset?>(null)
    var cursorRadius by mutableStateOf(0f)
        private set

    /** The current brush's own outline, used when the cursor's style is per brush: 0 none, 1 contrast, 2 the colour. */
    var cursorOutline by mutableStateOf(1)
        private set

    /** When the brush cursor shows: while the pen hovers, while painting, or both. */
    enum class CursorShows { HOVERING, PAINTING, BOTH }

    /** How it is drawn: light and dark rings, in the colour painted with, or the brush's own grey ring. */
    enum class CursorStyle { CONTRAST, COLOR, BRUSH }

    var cursorShows by mutableStateOf(CursorShows.entries.getOrElse(interfacePrefs.getInt(CURSOR_SHOWS, 0)) { CursorShows.HOVERING })
        private set
    var cursorStyle by mutableStateOf(CursorStyle.entries.getOrElse(interfacePrefs.getInt(CURSOR_STYLE, 2)) { CursorStyle.BRUSH })
        private set

    fun changeCursorShows(v: CursorShows) {
        cursorShows = v
        interfacePrefs.edit().putInt(CURSOR_SHOWS, v.ordinal).apply()
    }

    fun changeCursorStyle(v: CursorStyle) {
        cursorStyle = v
        interfacePrefs.edit().putInt(CURSOR_STYLE, v.ordinal).apply()
    }

    /** The palettes show as cards (named tiles) rather than compact rows of swatches. */
    var paletteCards by mutableStateOf(interfacePrefs.getBoolean(PALETTE_CARDS, false))
        private set

    fun changePaletteCards(on: Boolean) {
        paletteCards = on
        interfacePrefs.edit().putBoolean(PALETTE_CARDS, on).apply()
    }

    /** The short colour panel shows the colour history under the picker, rather than the active palette. */
    var colorHistoryShown by mutableStateOf(interfacePrefs.getBoolean(COLOR_HISTORY_SHOWN, false))
        private set

    fun changeColorHistoryShown(on: Boolean) {
        colorHistoryShown = on
        interfacePrefs.edit().putBoolean(COLOR_HISTORY_SHOWN, on).apply()
    }

    /** Project canvas: the canvas, without the interface, on a second display when there is one. */
    var projectCanvas by mutableStateOf(interfacePrefs.getBoolean(PROJECT_CANVAS, false))
        private set

    fun changeProjectCanvas(on: Boolean) {
        projectCanvas = on
        interfacePrefs.edit().putBoolean(PROJECT_CANVAS, on).apply()
    }

    // the second display's window while it is shown, and its size
    private var projection: Triple<android.view.Surface, Int, Int>? = null

    /** The second display's window came ([surface], [width] × [height]) or went (null). */
    fun setProjectionSurface(surface: android.view.Surface?, width: Int = 0, height: Int = 0) {
        val same = surface != null && projection?.first == surface
        projection = surface?.let { Triple(it, width, height) }
        if (same) session?.resizeProjection(width.toUInt(), height.toUInt()) else attachProjection()
    }

    /** The open artwork shows on the second display, if there is one. */
    private fun attachProjection() {
        val s = session ?: return
        val (surface, w, h) = projection ?: return s.detachProjection()
        val id = NativeWindows.acquire(surface)
        if (id != 0L) runCatching { s.attachProjection(id.toULong(), w.toUInt(), h.toUInt()) }
    }

    /** Brush scaling relative to the screen (else to the canvas). */
    var brushOnScreen by mutableStateOf(interfacePrefs.getBoolean(BRUSH_ON_SCREEN, false))
        private set

    fun changeBrushScaling(onScreen: Boolean) {
        brushOnScreen = onScreen
        session?.setBrushScaling(onScreen)
        interfacePrefs.edit().putBoolean(BRUSH_ON_SCREEN, onScreen).apply()
    }

    /**
     * Shows the brush cursor at window point [p] (or hides it), sized for the current brush and
     * zoom, as long as it shows when [hovering] (or painting).
     */
    fun showCursor(p: Offset?, hovering: Boolean = false) {
        val wanted = if (hovering) cursorShows != CursorShows.PAINTING else cursorShows != CursorShows.HOVERING
        if (!brushCursorOn || p == null || !wanted) {
            cursorAt = null
            return
        }
        if (cursorAt == null) session?.let { s ->
            cursorRadius = s.info().let { it.brushDiameter * it.zoom / 2f }
            cursorOutline = s.currentBrushSetting("cursor_outline").roundToInt()
        }
        cursorAt = p
    }

    /** The light interface: light grey panels and bars with dark text. */
    var lightInterface by mutableStateOf(interfacePrefs.getBoolean(LIGHT_INTERFACE, false).also { io.github.artcraftandroid.procraft.ui.Pc.light = it })
        private set

    fun changeLightInterface(on: Boolean) {
        lightInterface = on
        io.github.artcraftandroid.procraft.ui.Pc.light = on
        interfacePrefs.edit().putBoolean(LIGHT_INTERFACE, on).apply()
    }

    fun changeRapidUndoDelay(seconds: Float) {
        rapidUndoDelay = seconds.coerceIn(RAPID_UNDO_MIN_S, RAPID_UNDO_MAX_S)
        interfacePrefs.edit().putFloat(RAPID_UNDO_DELAY, rapidUndoDelay).apply()
    }

    /** How strongly what isn't selected is banded, 0..1. */
    var selectionVisibility by mutableStateOf(interfacePrefs.getFloat(SELECTION_VISIBILITY, 0.25f))
        private set

    fun changeSelectionVisibility(v: Float) {
        selectionVisibility = v.coerceIn(0f, 1f)
        session?.setSelectionVisibility(selectionVisibility)
        interfacePrefs.edit().putFloat(SELECTION_VISIBILITY, selectionVisibility).apply()
    }

    // pressure and smoothing, for every stroke

    var inputStabilization by mutableStateOf(interfacePrefs.getFloat(INPUT_STABILIZATION, 0f))
        private set
    var inputMotionFiltering by mutableStateOf(interfacePrefs.getFloat(INPUT_MOTION, 0f))
        private set
    var inputMotionExpression by mutableStateOf(interfacePrefs.getFloat(INPUT_EXPRESSION, 0f))
        private set
    var inputPressureSmoothing by mutableStateOf(interfacePrefs.getFloat(INPUT_PRESSURE_SMOOTHING, 0f))
        private set
    var inputCurve by mutableStateOf(
        interfacePrefs.getString(INPUT_CURVE, null)?.split(',')?.mapNotNull { it.toFloatOrNull() }?.chunked(2)?.filter { it.size == 2 }
            ?.map { Offset(it[0], it[1]) }?.takeIf { it.size >= 2 } ?: listOf(Offset(0f, 0f), Offset(1f, 1f)),
    )
        private set

    fun changeInput(
        stabilization: Float = inputStabilization,
        motion: Float = inputMotionFiltering,
        expression: Float = inputMotionExpression,
        smoothing: Float = inputPressureSmoothing,
        curve: List<Offset> = inputCurve,
    ) {
        inputStabilization = stabilization
        inputMotionFiltering = motion
        inputMotionExpression = expression
        inputPressureSmoothing = smoothing
        inputCurve = curve
        interfacePrefs.edit()
            .putFloat(INPUT_STABILIZATION, stabilization).putFloat(INPUT_MOTION, motion)
            .putFloat(INPUT_EXPRESSION, expression).putFloat(INPUT_PRESSURE_SMOOTHING, smoothing)
            .putString(INPUT_CURVE, curve.joinToString(",") { "${it.x},${it.y}" })
            .apply()
        applyInput()
    }

    fun resetInput() = changeInput(0f, 0f, 0f, 0f, listOf(Offset(0f, 0f), Offset(1f, 1f)))

    private fun applyInput() {
        session?.setInputSettings(inputStabilization, inputMotionFiltering, inputMotionExpression, inputPressureSmoothing, inputCurve.flatMap { listOf(it.x, it.y) })
    }

    fun changeRightHanded(on: Boolean) {
        rightHanded = on
        interfacePrefs.edit().putBoolean(RIGHT_HANDED, on).apply()
    }

    /** The actions panel's open tab, kept from one session to the next. */
    private var actionsTabState by mutableStateOf(interfacePrefs.getInt(ACTIONS_TAB, 0))
    var actionsTab: Int
        get() = actionsTabState
        set(tab) {
            actionsTabState = tab
            interfacePrefs.edit().putInt(ACTIONS_TAB, tab).apply()
        }

    /** The eyedropper, while it is open. */
    var picking by mutableStateOf<PickState?>(null)
        private set

    /** The modify button was pressed: the next touch on the canvas picks a colour. */
    var pickNext by mutableStateOf(false)

    /** Transform is open (with a box when the layer holds anything). */
    var transforming by mutableStateOf(false)
        private set
    var transformBox by mutableStateOf<TransformBox?>(null)
        private set
    private val transformPrefs = app.getSharedPreferences("transform", android.content.Context.MODE_PRIVATE)
    /** The brush library as edited, kept between artworks and launches. */
    private val brushPrefs = app.getSharedPreferences("brushes", android.content.Context.MODE_PRIVATE)
    var transformKind by mutableStateOf(enumPref(TRANSFORM_KIND, TransformKind.FREEFORM))
        private set
    var interpolation by mutableStateOf(enumPref(INTERPOLATION, Interpolation.BILINEAR))
        private set
    var transformPopup by mutableStateOf(TransformPopup.NONE)
    /** Magnetics, snapping, and snapping's distance (1..50 points) and velocity (0..10). */
    var guides by mutableStateOf(
        GuideSettings(
            transformPrefs.getBoolean(MAGNETICS, false),
            transformPrefs.getBoolean(SNAPPING, false),
            transformPrefs.getFloat(SNAP_DISTANCE, 3f),
            transformPrefs.getFloat(SNAP_VELOCITY, 10f),
        ),
    )
        private set

    /** A warp shows and takes all 16 of its control points, not only its corners. */
    var advancedMesh by mutableStateOf(transformPrefs.getBoolean(ADVANCED_MESH, false))
        private set

    /** The selection tool is open. */
    var selecting by mutableStateOf(false)
        private set
    var selectionKind by mutableStateOf(enumPref(SELECTION_KIND, SelectionKind.FREEHAND))
        private set
    /** New shapes take away from the selection instead of adding to it. */
    var selectionRemoves by mutableStateOf(false)
        private set
    /** Each new selection is filled with the paint colour. */
    var colorFill by mutableStateOf(false)
        private set
    var hasSelection by mutableStateOf(false)
        private set
    private var selectionPopupState by mutableStateOf(SelectionPopup.NONE)
    /** The popover over the selection toolbar; closing the feather popover keeps its feathering as one undo step. */
    var selectionPopup: SelectionPopup
        get() = selectionPopupState
        set(value) {
            if (selectionPopupState == SelectionPopup.FEATHER && value != SelectionPopup.FEATHER) {
                session?.selectFeatherDone()
                refreshFlags()
            }
            selectionPopupState = value
        }
    /** The feather popover's amount, 0..1. */
    var featherAmount by mutableFloatStateOf(0f)
        private set
    var savedSelections by mutableStateOf(0)
        private set
    /** Pictures of the kept selections, by index. */
    val savedThumbnails = mutableStateMapOf<Int, ImageBitmap>()
    /** The freehand path being drawn, in canvas pixels (closed by tapping its first point). */
    var freehand by mutableStateOf<List<Offset>>(emptyList())
        private set
    /** A rectangle or ellipse being dragged out: its corners in window pixels. */
    var shapeDrag by mutableStateOf<Pair<Offset, Offset>?>(null)
    /** A colour selection's threshold being dragged: where (window pixels), and the threshold (0..1). */
    var thresholdDrag by mutableStateOf<Pair<Offset, Float>?>(null)
    /** The colour selection's threshold, kept from one selection to the next (0..1). */
    var selectionThreshold by mutableFloatStateOf(transformPrefs.getFloat(SELECTION_THRESHOLD, 0.5f))
        private set

    /** The adjustment open on the current layer, and its values. */
    var adjustment by mutableStateOf<Adjustment?>(null)
        private set
    var adjustAmount by mutableFloatStateOf(0f)
        private set
    var hue50 by mutableFloatStateOf(0.5f)
        private set
    var saturation50 by mutableFloatStateOf(0.5f)
        private set
    var brightness50 by mutableFloatStateOf(0.5f)
        private set
    /** The liquify brush: its way and sliders (0..1), and how much of it shows. */
    var liquifyKind by mutableStateOf(LiquifyKind.PUSH)
        private set
    var liquifySize by mutableFloatStateOf(0.5f)
    var liquifyPressure by mutableFloatStateOf(1f)
    var liquifyDistortion by mutableFloatStateOf(0f)
    var liquifyMomentum by mutableFloatStateOf(0.8f)
    var liquifyAmount by mutableFloatStateOf(1f)
        private set
    /** Liquify's mode list or adjust slider is open. */
    var liquifyPopup by mutableStateOf(0)
    var liquified by mutableStateOf(false)
        private set

    /** The adjustment is painted on with the brush (Pencil), and the Layer / Pencil choice is open. */
    var adjustPencil by mutableStateOf(false)
        private set
    var adjustModesOpen by mutableStateOf(false)
    /** Cloning: where the source is (canvas pixels), and how far from the brush once a stroke fixed it. */
    var clonePuck by mutableStateOf<Offset?>(null)
        private set
    private var cloneOffset: Offset? = null
    /** Where the source was before each clone stroke kept, and before each undone, for undo and redo. */
    private val puckBefore = ArrayDeque<Offset>()
    private val puckAfter = ArrayDeque<Offset>()
    /** The canvas window's size (pixels) and density, for what starts in its middle. */
    var windowSize = Offset.Zero
    var density = 2f

    /** Where the adjustment's menu is open (window pixels), after a tap on the canvas. */
    var adjustMenuAt by mutableStateOf<Offset?>(null)
    /** The layer shows as it was before the adjustment. */
    var adjustPreview by mutableStateOf(false)
        private set
    /** Colour balance: the tone shown (shadows, midtones, highlights) and its nine sliders (0..1, 0.5 none). */
    var balanceTone by mutableStateOf(1)
        private set
    var balance by mutableStateOf(List(9) { 0.5f })
        private set
    /** Curves: the channel shown (all, red, green, blue) and each channel's points (0..1). */
    var curveChannel by mutableStateOf(0)
        private set
    var curves by mutableStateOf(List(4) { listOf(Offset(0f, 0f), Offset(1f, 1f)) })
        private set
    /** The gradient mapped by. */
    var gradientIndex by mutableStateOf(0)
        private set
    private val gradientStore = io.github.artcraftandroid.procraft.gallery.GradientStore(File(app.filesDir, "gradients.json"))
    /** The gradient library: ours, and those made or changed here. */
    var gradients by mutableStateOf(gradientStore.load())
        private set
    /** The gradient being edited, by its place in [gradients]. */
    var gradientEditing by mutableStateOf<Int?>(null)
        private set
    /** Glitch: the kind shown and each kind's sliders (the last one its zoom). */
    var glitchKind by mutableStateOf(GlitchKind.ARTIFACT)
        private set
    var glitchValues by mutableStateOf(GLITCH_DEFAULTS)
        private set
    /** Noise: its kind, scale, octaves and turbulence (0..1), each channel its own, added or mixed in. */
    var noiseKind by mutableStateOf(NoiseKind.CLOUDS)
        private set
    var noiseScale by mutableFloatStateOf(0f)
        private set
    var noiseOctaves by mutableFloatStateOf(0f)
        private set
    var noiseTurbulence by mutableFloatStateOf(0.7f)
        private set
    var noiseMulti by mutableStateOf(false)
        private set
    var noiseAdded by mutableStateOf(true)
        private set
    /** Bloom: transition, size and burn (0..1). */
    var bloomTransition by mutableFloatStateOf(1f)
        private set
    var bloomSize by mutableFloatStateOf(0.3f)
        private set
    var bloomBurn by mutableFloatStateOf(0.35f)
        private set
    var halftoneKind by mutableStateOf(HalftoneKind.FULL_COLOR)
        private set
    /** Chromatic aberration: its kind and each kind's two sliders; displaced, how far (window pixels). */
    var chromaKind by mutableStateOf(ChromaticKind.PERSPECTIVE)
        private set
    var chromaValues by mutableStateOf(mapOf(ChromaticKind.PERSPECTIVE to listOf(0f, 0f), ChromaticKind.DISPLACE to listOf(0f, 0f)))
        private set
    private var chromaOffset = Offset.Zero
    private var chromaDragFrom = Offset.Zero

    /** The node held on the transform box and where the touch is (window pixels), while a drag lasts. */
    var transformHeld by mutableStateOf<Pair<TransformHandle, Offset>?>(null)
    /** The rotate or box-turning node just let go where the touch left it, springing back. */
    var transformLetGo by mutableStateOf<Pair<TransformHandle, Offset>?>(null)

    /** The view (canvas pixels to window pixels) as [a, b, c, d, e, f], read while something is drawn over the canvas. */
    var viewMatrix by mutableStateOf(listOf(1f, 0f, 0f, 1f, 0f, 0f))
        private set

    private var pickJob: Job? = null
    private var pendingPick: Pair<Float, Float>? = null

    private val paletteStore = PaletteStore(File(app.filesDir, "palettes.json"))
    var palettes by mutableStateOf<List<Palette>>(emptyList())
        private set

    /** The palette shown under the history in the other colour modes. */
    var defaultPalette by mutableStateOf(0)
        private set

    init {
        val (p, d) = paletteStore.load()
        palettes = p
        defaultPalette = d
        // colour profiles the user imported
        io.github.artcraftandroid.procraft.core.setProfileFolder(File(app.filesDir, "icc").path)
    }

    /** A CMYK canvas's inks for a colour and back, for the colour panel; null on an RGB canvas. */
    var inks by mutableStateOf<io.github.artcraftandroid.procraft.ui.Inks?>(null)
        private set

    /** How the colour panel and swatches show the canvas's colours: its space, its press. */
    private fun refreshCanvasSpace(s: CanvasSession) {
        val name = s.profile()
        val space = io.github.artcraftandroid.procraft.ui.CanvasSpace
        val p3 = io.github.artcraftandroid.procraft.gallery.CanvasPreset.P3
        when {
            s.isCmyk() -> {
                space.set(true) { s.proofColor(it.toUInt()).toInt() }
                inks = io.github.artcraftandroid.procraft.ui.Inks(
                    { rgb -> s.cmykOf(rgb.toUInt()).toFloatArray().takeIf { it.size == 4 } ?: FloatArray(4) },
                    { v -> s.rgbOfCmyk(v.toList()).toInt() },
                )
                return
            }
            io.github.artcraftandroid.procraft.core.profileIsBuiltIn(name) -> space.set(name == p3)
            else -> space.set(true) { io.github.artcraftandroid.procraft.core.convertColor(it.toUInt(), name, p3).toInt() }
        }
        inks = null
    }

    /** Adds the colour profile file at [uri] to those a canvas can be in; its name, or null if it isn't one. */
    suspend fun importProfile(uri: android.net.Uri): String? {
        val resolver = getApplication<Application>().contentResolver
        val name = withContext(Dispatchers.IO) {
            runCatching { resolver.openInputStream(uri)?.use { it.readBytes() }?.let { io.github.artcraftandroid.procraft.core.importProfile(it) } }.getOrNull()
        }
        if (name == null) message = "This file isn't a color profile."
        return name
    }
    var harmonyKind by mutableStateOf(HarmonyKind.COMPLEMENTARY)

    var barsHidden by mutableStateOf(false)
        private set
    var panel by mutableStateOf(Panel.NONE)
        private set

    /** The panel just closed fades away (an adjustment picked from its list) instead of going at once. */
    var panelFading by mutableStateOf(false)

    var layers by mutableStateOf<List<LayerInfo>>(emptyList())
        private set
    var background by mutableStateOf(BackgroundInfo(1f, 1f, 1f, true))
        private set
    var canUndo by mutableStateOf(false)
        private set
    var canRedo by mutableStateOf(false)
        private set
    val thumbnails = mutableStateMapOf<ULong, ImageBitmap>()

    var brushSets by mutableStateOf<List<BrushSetInfo>>(emptyList())
        private set
    var currentBrush by mutableStateOf<BrushInfo?>(null)
        private set
    var shownSet by mutableStateOf(0)
        private set
    val previews = mutableStateMapOf<String, ImageBitmap>()

    /** Previews whose stroke runs under the brush's name: the name then sits on a dark pill. */
    val shadedNames = mutableStateMapOf<String, Boolean>()

    /** The drawing guide, its editor open, and symmetry's options open. */
    var guide by mutableStateOf<GuideInfo?>(null)
        private set
    var guideEditing by mutableStateOf(false)
        private set
    var symmetryOptionsOpen by mutableStateOf(false)

    /** What was last copied or cut, for pasting (in this artwork or another). */
    private var clipboard: io.github.artcraftandroid.procraft.core.ThumbnailImage? = null
    var canPaste by mutableStateOf(false)
        private set

    /** The shape the last stroke snapped to (drawn and held), and whether its handles are shown. */
    var quickShape by mutableStateOf<QuickShapeInfo?>(null)
        private set
    var quickShapeEditing by mutableStateOf(false)

    /** The brush studio: open, the tab shown, the brush's name, its controls' values, curve and sample stroke. */
    var studioOpen by mutableStateOf(false)
        private set

    /**
     * The drawing pad's surface has drawn: until then the pad is covered, since its surface punches
     * through to the canvas's until it has a frame of its own.
     */
    var padShown by mutableStateOf(false)

    /** The brush studio's open page, whichever brush it opens on, kept from one session to the next. */
    private var studioTabState by mutableStateOf(interfacePrefs.getInt(STUDIO_TAB, 0))
    var studioTab: Int
        get() = studioTabState
        set(tab) {
            studioTabState = tab
            interfacePrefs.edit().putInt(STUDIO_TAB, tab).apply()
        }
    var studioName by mutableStateOf("")
        private set
    private val studioValues = mutableStateMapOf<String, Float>()
    var studioCurve by mutableStateOf(listOf(Offset(0f, 0f), Offset(1f, 1f)))
        private set
    var studioPreview by mutableStateOf<ImageBitmap?>(null)
        private set
    private var studioPreviewJob: Job? = null
    /** The studio's drawing pad, the colour it paints in (an index into [PAD_COLORS]), and its settings popover. */
    var pad: CanvasSession? = null
        private set
    var padColor by mutableStateOf(PAD_WHITE)
        private set
    var padMenuOpen by mutableStateOf(false)
    val blendModeList: List<BlendModeInfo> = blendModes()

    init {
        refreshGallery()
    }

    // ---- gallery ----

    /** Where new artworks go: the stack open, or the gallery. */
    private val galleryDir get() = openStack?.dir ?: store.root

    fun refreshGallery() {
        viewModelScope.launch {
            val stack = openStack
            val entries = withContext(Dispatchers.IO) { store.list(stack?.dir ?: store.root) }
            galleryEntries = entries
            artworks = entries.filterIsInstance<Artwork>()
            // an emptied stack closes
            if (stack != null && entries.isEmpty()) {
                openStack = null
                refreshGallery()
            }
        }
    }

    fun showStack(s: io.github.artcraftandroid.procraft.gallery.Stack?) {
        openStack = s
        refreshGallery()
    }

    /** The artworks put together in a new stack. */
    fun stackArtworks(list: List<Artwork>) {
        if (list.size < 2 || openStack != null) return
        viewModelScope.launch {
            withContext(Dispatchers.IO) { store.stack(list) }
            refreshGallery()
        }
    }

    fun renameStack(s: io.github.artcraftandroid.procraft.gallery.Stack, name: String) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) { store.renameStack(s, name) }
            if (openStack?.dir == s.dir) openStack = s.copy(name = name.trim().ifEmpty { "Stack" })
            refreshGallery()
        }
    }

    private val presetPrefs = app.getSharedPreferences("canvas_presets", android.content.Context.MODE_PRIVATE)

    /** The New canvas list under the screen's size: the standard presets, then those made since. */
    var canvasPresets by mutableStateOf(readPresets())
        private set

    /** Custom Canvas is open, on a new preset or on [editingPreset]. */
    var customCanvasOpen by mutableStateOf(false)
        private set

    /** The preset Custom Canvas edits (its place in [canvasPresets]); null for a new one. */
    var editingPreset by mutableStateOf<Int?>(null)
        private set

    private fun readPresets(): List<CanvasPreset> {
        val saved = presetPrefs.getString(PRESETS, null) ?: return StandardPresets
        return runCatching {
            val list = org.json.JSONArray(saved)
            List(list.length()) { CanvasPreset.fromJson(list.getJSONObject(it)) }
        }.getOrDefault(StandardPresets)
    }

    private fun savePresets(list: List<CanvasPreset>) {
        canvasPresets = list
        val json = org.json.JSONArray()
        list.forEach { json.put(it.toJson()) }
        presetPrefs.edit().putString(PRESETS, json.toString()).apply()
    }

    /** Opens Custom Canvas on the preset at [index], or on a new one. */
    fun openCustomCanvas(index: Int? = null) {
        editingPreset = index?.takeIf { it in canvasPresets.indices }
        customCanvasOpen = true
    }

    fun closeCustomCanvas() {
        customCanvasOpen = false
        editingPreset = null
    }

    /**
     * Custom Canvas's Done: an edited preset is saved in its place; a new one goes at the end of
     * the list and is made into an artwork. True if an artwork was made.
     */
    fun finishCustomCanvas(p: CanvasPreset): Boolean {
        val index = editingPreset
        closeCustomCanvas()
        if (index != null && index in canvasPresets.indices) {
            savePresets(canvasPresets.toMutableList().also { it[index] = p })
            return false
        }
        savePresets(canvasPresets + p)
        newPresetArtwork(p)
        return true
    }

    fun deletePreset(index: Int) {
        if (index in canvasPresets.indices) savePresets(canvasPresets.filterIndexed { i, _ -> i != index })
    }

    /** A new artwork made as preset [p] says, opened on the canvas. */
    fun newPresetArtwork(p: CanvasPreset) {
        val rgb = listOf(((p.background shr 16) and 255) / 255f, ((p.background shr 8) and 255) / 255f, (p.background and 255) / 255f)
        val s = CanvasSession.newCustom(p.width.toUInt(), p.height.toUInt(), p.dpi, rgb, !p.backgroundHidden, "Untitled Artwork")
        s.setNewProfile(p.profile)
        start(s, store.newFolder(galleryDir))
        timeLapse?.settings = p.timeLapse
        startedNew()
    }

    /** The layers a canvas [width] × [height] holds on this device. */
    /** Says the canvas holds no more layers, and how many it holds. */
    private fun noRoom(): String {
        val (w, h) = canvasSize()
        return "This canvas holds at most ${layersFor(w, h)} layers."
    }

    fun layersFor(width: Int, height: Int): Int {
        val memory = android.app.ActivityManager.MemoryInfo()
        getApplication<Application>().getSystemService(android.app.ActivityManager::class.java)?.getMemoryInfo(memory)
        val budget = if (memory.totalMem > 0) memory.totalMem / 4 else 2L shl 30
        return (budget / (width.toLong().coerceAtLeast(1) * height.toLong().coerceAtLeast(1) * 4)).toInt().coerceAtLeast(1)
    }

    /** A new artwork of `width` × `height` pixels (the screen's size), in Display P3, opened on the canvas. */
    fun newArtwork(width: Int, height: Int) {
        val s = CanvasSession(width.coerceAtLeast(1).toUInt(), height.coerceAtLeast(1).toUInt())
        s.setNewProfile(io.github.artcraftandroid.procraft.gallery.CanvasPreset.P3)
        start(s, store.newFolder(galleryDir))
        startedNew()
    }

    // the app's own settings (the settings page beside the canvas): read when the app comes back

    /** An artwork opens as it was left. */
    var orientationMemory = interfacePrefs.getBoolean(ORIENTATION_MEMORY, true)
        private set

    /** Fitting the canvas keeps it clear of the top bar and the sidebar. */
    var fitInside = interfacePrefs.getBoolean(FIT_INSIDE, false)
        private set

    /** New artworks don't record their time-lapse. */
    var timeLapseDisabled = interfacePrefs.getBoolean(NO_TIME_LAPSE, false)
        private set

    /** Accessibility: the colour named in a message as it changes, and clicks for actions. */
    var colorNames = interfacePrefs.getBoolean(COLOR_NAMES, false)
        private set
    var feedbackSounds = interfacePrefs.getBoolean(FEEDBACK_SOUNDS, false)
        private set

    /** Accessibility: a floating bar standing in for gestures that need more than one finger. */
    var singleTouchCompanion by mutableStateOf(interfacePrefs.getBoolean(SINGLE_TOUCH_COMPANION, false))
        private set

    /** Accessibility: a tap lands where the finger first touched, even if it drifts before lifting. */
    var tapAssistance = interfacePrefs.getBoolean(TAP_ASSISTANCE, false)
        private set

    /** The companion's one-finger mode: a finger dragged on the canvas zooms it, or moves it. */
    enum class CompanionMode { NONE, ZOOM, MOVE }

    var companionMode by mutableStateOf(CompanionMode.NONE)
        private set

    fun toggleCompanionMode(m: CompanionMode) {
        feedback()
        companionMode = if (companionMode == m) CompanionMode.NONE else m
    }

    fun readAppSettings() {
        colorNames = interfacePrefs.getBoolean(COLOR_NAMES, false)
        feedbackSounds = interfacePrefs.getBoolean(FEEDBACK_SOUNDS, false)
        singleTouchCompanion = interfacePrefs.getBoolean(SINGLE_TOUCH_COMPANION, false)
        tapAssistance = interfacePrefs.getBoolean(TAP_ASSISTANCE, false)
        if (!singleTouchCompanion) companionMode = CompanionMode.NONE
        orientationMemory = interfacePrefs.getBoolean(ORIENTATION_MEMORY, true)
        fitInside = interfacePrefs.getBoolean(FIT_INSIDE, false)
        timeLapseDisabled = interfacePrefs.getBoolean(NO_TIME_LAPSE, false)
        session?.let(::applyAppSettings)
        applyFitInsets()
    }

    /** The settings the canvas itself keeps: simplified undos and the speed distance. */
    private fun applyAppSettings(s: CanvasSession) {
        s.setSimplifiedUndos(interfacePrefs.getBoolean(SIMPLIFIED_UNDOS, true))
        s.setSpeedDistance(interfacePrefs.getFloat(SPEED_DISTANCE, 1f))
    }

    /** What fitting the canvas keeps clear of: the top bar and the sidebar, with Fit Canvas Inside Interface. */
    fun applyFitInsets() {
        val s = session ?: return
        if (!fitInside) {
            s.setFitInsets(0f, 0f, 0f, 0f)
            return
        }
        val side = if (showSideBar) (io.github.artcraftandroid.procraft.ui.Pc.SideBarWidth.value + io.github.artcraftandroid.procraft.ui.Pc.PopoverGap.value) * density else 0f
        s.setFitInsets(if (rightHanded) 0f else side, io.github.artcraftandroid.procraft.ui.Pc.BarHeight.value * density, if (rightHanded) side else 0f, 0f)
    }

    /** A new artwork just started: without time-lapse if it is turned off. */
    private fun startedNew() {
        if (timeLapseDisabled) changeTimeLapseRecording(false)
    }

    /** A new artwork from a photo or picture file, its size, the picture as its one layer. */
    fun newFromPicture(uri: android.net.Uri) {
        viewModelScope.launch {
            // in Display P3 if the picture is, else in sRGB
            val p3 = withContext(Dispatchers.IO) { pictureIsP3(uri) }
            val image = withContext(Dispatchers.IO) { decodePicture(uri, p3) }
            if (image == null) {
                message = "This picture can't be opened."
                return@launch
            }
            val s = CanvasSession.fromImage(image, "Untitled Artwork")
            if (p3) s.setNewProfile(io.github.artcraftandroid.procraft.gallery.CanvasPreset.P3)
            start(s, store.newFolder(galleryDir))
            startedNew()
        }
    }

    /** A file from elsewhere into the gallery: an artwork file, or a picture as a new artwork. */
    fun importFile(uri: android.net.Uri) {
        val resolver = getApplication<Application>().contentResolver
        viewModelScope.launch {
            val head = withContext(Dispatchers.IO) { runCatching { resolver.openInputStream(uri)?.use { s -> ByteArray(4).also { s.read(it) } } }.getOrNull() }
            val name = withContext(Dispatchers.IO) { fileName(uri) }
            // another program's palette
            val foreign = head != null && io.github.artcraftandroid.procraft.gallery.ColourSwatches.isExchange(head) || name?.endsWith(".aco", ignoreCase = true) == true
            val zip = head != null && head[0] == 'P'.code.toByte() && head[1] == 'K'.code.toByte()
            // a zip of brushes
            val brushes = if (zip) withContext(Dispatchers.IO) { runCatching { resolver.openInputStream(uri)?.use { it.readBytes() } }.getOrNull() }?.takeIf(::isBrushFile) else null
            // a zip: brushes, a palette, an artwork file of ours, or a layered package
            if (brushes != null) {
                if (!addBrushes(brushes)) message = "This file has no brushes in it."
            } else if (head != null && (foreign || zip) && importSwatches(uri)) {
                message = "Palette added."
            } else if (foreign) {
                message = "This palette can't be read."
            } else if (head != null && head[0] == 'P'.code.toByte() && head[1] == 'K'.code.toByte()) {
                val ok = withContext(Dispatchers.IO) {
                    val dir = store.newFolder(galleryDir)
                    val ours = runCatching { resolver.openInputStream(uri)?.use { io.github.artcraftandroid.procraft.gallery.ArtworkFile.read(it, dir) } }.getOrNull() == true
                    (ours || importPackage(uri, dir)).also { if (!it) dir.deleteRecursively() }
                }
                if (ok) refreshGallery() else message = "This file can't be opened."
            } else {
                newFromPicture(uri)
            }
        }
    }

    /**
     * A layered package from [uri] made into the artwork folder [dir]: its layers, settings and
     * preview (as the gallery's thumbnail until it is opened); false if it isn't one.
     */
    private fun importPackage(uri: android.net.Uri, dir: File): Boolean {
        val resolver = getApplication<Application>().contentResolver
        val temp = File(getApplication<Application>().cacheDir, "import-${System.nanoTime()}.zip")
        return try {
            resolver.openInputStream(uri)?.use { input -> temp.outputStream().use { input.copyTo(it) } } ?: return false
            val s = runCatching { CanvasSession.openPackage(temp.path) }.getOrNull() ?: return false
            s.save(dir.path)
            java.util.zip.ZipFile(temp).use { zip ->
                zip.getEntry("QuickLook/Thumbnail.png")?.let { e ->
                    zip.getInputStream(e).use { input -> File(dir, "thumbnail.png").outputStream().use { input.copyTo(it) } }
                }
            }
            true
        } catch (e: Exception) {
            android.util.Log.e("ProCraft", "importing a package", e)
            false
        } finally {
            temp.delete()
        }
    }

    /** The open artwork as a layered package in the cache, to share. */
    suspend fun exportPackageFile(): File? {
        val s = session ?: return null
        return withContext(Dispatchers.IO) {
            runCatching {
                val base = s.name().filter { it.isLetterOrDigit() || it in " _-" }.trim().replace(' ', '_').ifEmpty { "Untitled_Artwork" }
                val out = File(File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }, "$base.$PACKAGE_EXTENSION")
                s.exportPackage(out.path, System.nanoTime().toULong())
                out
            }.onFailure { android.util.Log.e("ProCraft", "exporting a package", it) }.getOrNull()
        }
    }

    /** The open artwork saved and zipped into the cache as one file, to share. */
    suspend fun exportArtworkFile(): File? {
        val s = session ?: return null
        val dir = artworkDir ?: return null
        return withContext(Dispatchers.IO) {
            runCatching {
                s.save(dir.path)
                val out = File(File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() },
                    s.name().filter { it.isLetterOrDigit() || it in " _-" }.trim().ifEmpty { "Untitled Artwork" } + "." + io.github.artcraftandroid.procraft.gallery.ArtworkFile.EXTENSION)
                io.github.artcraftandroid.procraft.gallery.ArtworkFile.write(dir, out)
                out
            }.getOrNull()
        }
    }

    /**
     * The gallery zooming into an artwork as it opens ([opening]), or out of it as it closes: the
     * artwork's cell and where its canvas is (window pixels), and closing, a picture of the canvas.
     */
    class GalleryZoom(
        val cell: androidx.compose.ui.geometry.Rect,
        val canvas: androidx.compose.ui.geometry.Rect,
        val opening: Boolean,
        val picture: androidx.compose.ui.graphics.ImageBitmap? = null,
    )

    var galleryZoom by mutableStateOf<GalleryZoom?>(null)

    /** Where each artwork's thumbnail was last shown in the gallery (window pixels), by its folder's name. */
    val thumbAt = mutableMapOf<String, androidx.compose.ui.geometry.Rect>()

    /** The gallery's grid, kept so it is where it was when the gallery comes back. */
    val galleryGrid = androidx.compose.foundation.lazy.grid.LazyGridState()

    /** The window's size in pixels, as the gallery last saw it. */
    var windowPx = androidx.compose.ui.unit.IntSize(1, 1)

    /** Where [s]'s canvas is on screen through the view [m] (as `viewMatrix` gives it), window pixels. */
    private fun canvasRect(s: CanvasSession, m: List<Float>): androidx.compose.ui.geometry.Rect {
        val info = s.info()
        val (w, h) = info.width.toFloat() to info.height.toFloat()
        val corners = listOf(Offset(0f, 0f), Offset(w, 0f), Offset(0f, h), Offset(w, h)).map { p ->
            Offset(m[0] * p.x + m[2] * p.y + m[4], m[1] * p.x + m[3] * p.y + m[5])
        }
        return androidx.compose.ui.geometry.Rect(corners.minOf { it.x }, corners.minOf { it.y }, corners.maxOf { it.x }, corners.maxOf { it.y })
    }

    /**
     * Waits (off the main thread, as a big artwork's first frame holds the session a while) until
     * the open artwork's canvas has drawn a frame, or a few seconds have passed.
     */
    suspend fun awaitCanvasDrawn() = withContext(Dispatchers.IO) {
        val s = session ?: return@withContext
        var waited = 0L
        while (waited < CANVAS_DRAWN_LIMIT_MS && runCatching { s.info().frames }.getOrDefault(1uL) == 0uL) {
            kotlinx.coroutines.delay(CANVAS_POLL_MS)
            waited += CANVAS_POLL_MS
        }
    }

    /** Opens artwork [a], the gallery zooming into its cell; then [then]. */
    fun openArtwork(a: Artwork, then: (() -> Unit)? = null) {
        if (opening) return
        opening = true
        val cell = thumbAt[a.dir.name]
        viewModelScope.launch {
            val s = withContext(Dispatchers.IO) { runCatching { CanvasSession.open(a.dir.path) } }
            opening = false
            s.onSuccess {
                if (!orientationMemory) it.forgetSavedView()
                val m = it.openingViewMatrix(windowPx.width.toUInt(), windowPx.height.toUInt())
                galleryZoom = cell?.let { c -> GalleryZoom(c, canvasRect(it, m), opening = true) }
                start(it, a.dir)
                then?.invoke()
            }.onFailure { message = "This artwork can't be opened." }
        }
    }

    /**
     * Artworks from the gallery written as [format] (one of [SHARE_FORMATS] but the animated ones)
     * into the cache, to share; with the type they are.
     */
    suspend fun shareableFiles(list: List<Artwork>, format: String, pdfQuality: Int = 100): Pair<List<File>, String> = withContext(Dispatchers.IO) {
        val (extension, mime) = when (format) {
            "PSD" -> "psd" to "image/vnd.adobe.photoshop"
            "PDF" -> "pdf" to "application/pdf"
            "JPEG" -> "jpg" to "image/jpeg"
            "PNG" -> "png" to "image/png"
            "TIFF" -> "tif" to "image/tiff"
            else -> PACKAGE_EXTENSION to "application/octet-stream"
        }
        val dir = File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }
        val used = mutableSetOf<String>()
        list.mapNotNull { a ->
            val base = a.name.filter { it.isLetterOrDigit() || it in " _-" }.trim().ifEmpty { "Untitled Artwork" }
            var name = base
            var n = 2
            while (!used.add(name)) name = "$base ${n++}"
            File(dir, "$name.$extension").takeIf { io.github.artcraftandroid.procraft.gallery.exportArtwork(a.dir, format, it, pdfQuality) }
        } to mime
    }

    /** Opens artwork [a] to set up its animated export as [kind], going back to the gallery after. */
    fun openForAnimatedExport(a: Artwork, kind: Animated) {
        openArtwork(a) {
            exportingFromGallery = true
            animatedExport = kind
        }
    }

    fun deleteArtwork(a: Artwork) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) { store.delete(a) }
            refreshGallery()
        }
    }

    fun duplicateArtwork(a: Artwork) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) { store.duplicate(a) }
            refreshGallery()
        }
    }

    fun renameArtwork(a: Artwork, name: String) {
        if (name.isBlank()) return
        viewModelScope.launch {
            withContext(Dispatchers.IO) { store.rename(a, name) }
            refreshGallery()
        }
    }

    /** Saves the open artwork and goes back to the gallery. */
    fun closeArtwork() {
        endTransform(true)
        endAdjustment(true)
        val s = session ?: return
        noteTime()
        keepChoices(s)
        s.detachProjection()
        val dir = artworkDir
        autosave?.cancel()
        // the gallery zooms out of the artwork's cell, the canvas fading away over it
        val cell = dir?.let { thumbAt[it.name] }
        galleryZoom = cell?.let { c ->
            val picture = s.canvasThumbnail(CLOSING_PICTURE_SIDE.toUInt())?.let { t ->
                val bitmap = io.github.artcraftandroid.procraft.ui.CanvasSpace.tag(Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), Bitmap.Config.ARGB_8888))
                bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(t.rgba))
                bitmap.asImageBitmap()
            }
            GalleryZoom(c, canvasRect(s, s.viewMatrix()), opening = false, picture = picture)
        }
        session = null
        artworkDir = null
        panel = Panel.NONE
        screen = Screen.GALLERY
        viewModelScope.launch {
            withContext(Dispatchers.IO) { dir?.let { runCatching { s.save(it.path) } } }
            galleryVersion++
            refreshGallery()
        }
    }

    /** When time on the open artwork last started counting (elapsed realtime), 0 while away. */
    private var activeSince = 0L

    /** Adds the time since it last started counting to the artwork's, and stops counting. */
    private fun noteTime() {
        val s = session ?: return
        val now = android.os.SystemClock.elapsedRealtime()
        if (activeSince > 0) s.addTrackedTime((now - activeSince).coerceAtLeast(0).toULong())
        activeSince = 0
    }

    /** The app is back in front: time on the open artwork counts again. */
    fun resumeTracking() {
        if (session != null) activeSince = android.os.SystemClock.elapsedRealtime()
    }

    /** What each touch gesture does. */
    val gestures = GestureSettings(app.getSharedPreferences("gestures", android.content.Context.MODE_PRIVATE))

    /**
     * The pen's side button pressed ([secondary]: its second one), at window point [at]: what Pen
     * double-tap (or Pen squeeze) is set to do in Gesture controls.
     */
    fun penButton(secondary: Boolean, at: Offset) {
        val trigger = if (secondary) Trigger.PEN_SQUEEZE else Trigger.PEN_DOUBLE_TAP
        when (gestures.purposeOf(trigger)) {
            GesturePurpose.EYEDROPPER -> pickNext = true
            GesturePurpose.ASSISTED -> guide?.let { setGuideAssisted(!it.assisted) }
            GesturePurpose.QUICKMENU -> toggleQuickMenu(at)
            GesturePurpose.FULL_SCREEN -> toggleBars()
            GesturePurpose.CLEAR_LAYER -> clearCurrentLayer()
            GesturePurpose.COPY_PASTE -> copyPasteOpen = true
            GesturePurpose.LAYER_SELECT -> layerSelectAt(at)
            GesturePurpose.QUICKSHAPE -> quickShapeLast()
            else -> Unit
        }
    }

    /** The modify button tapped: what Tap □ is set to do (open or close the eyedropper at first). */
    fun tapModify() {
        when (gestures.purposeOf(Trigger.TAP_MODIFY)) {
            GesturePurpose.EYEDROPPER -> pickNext = !pickNext
            GesturePurpose.QUICKMENU -> toggleQuickMenu(Offset(screenCentre.x, screenCentre.y))
            GesturePurpose.COPY_PASTE -> copyPasteOpen = true
            GesturePurpose.QUICKSHAPE -> quickShapeLast()
            else -> Unit
        }
    }

    /** □ is held down: a canvas touch now does what □ + Touch (or □ + Pen) is set to. */
    var modifyHeld = false
        private set

    // when □ went down, and whether a touch has used it since
    private var modifyDownAt = 0L
    private var modifyUsed = false

    fun modifyPressed() {
        modifyHeld = true
        modifyUsed = false
        modifyDownAt = android.os.SystemClock.uptimeMillis()
    }

    /** □ let go: a short press nothing used is a tap ([cancelled] when the finger slid off). */
    fun modifyReleased(cancelled: Boolean) {
        modifyHeld = false
        if (!cancelled && !modifyUsed && android.os.SystemClock.uptimeMillis() - modifyDownAt < MODIFY_TAP_MS) tapModify()
    }

    fun useModify() {
        modifyUsed = true
    }

    /** The middle of the screen (window pixels), for menus a button opens. */
    var screenCentre = Offset(800f, 1280f)

    /** Gesture controls is open. */
    var gestureControlsOpen by mutableStateOf(false)

    /** Canvas information is open. */
    var canvasInfoOpen by mutableStateOf(false)

    /** The artwork's facts, its time counted up to now. */
    fun artworkInfo(): io.github.artcraftandroid.procraft.core.ArtworkInfo? {
        val s = session ?: return null
        if (activeSince > 0) {
            noteTime()
            resumeTracking()
        }
        return s.artworkInfo()
    }

    /** The artwork's folder (for its size and when it was saved). */
    fun artworkFolder(): File? = artworkDir

    fun timeLapseInfo(): Pair<Int, Long> = timeLapse?.let { it.count to it.bytes() } ?: (0 to 0L)

    /** Renames the artwork and sets its author, from Canvas information. */
    fun changeArtworkInfo(name: String, author: String) {
        val s = session ?: return
        if (name.isNotBlank() && name.trim() != s.name()) s.setName(name.trim())
        s.setAuthor(author)
    }

    /** Puts the canvas in colour profile [name], its colours and the current colour turned into it. */
    fun changeProfile(name: String) {
        val s = session ?: return
        val from = s.profile()
        val rgb = colorRgb()
        if (!s.changeProfile(name)) return
        refresh()
        setRgb(io.github.artcraftandroid.procraft.core.convertColor(rgb.toUInt(), from, name).toInt())
    }

    /** Saves the open artwork where it is (the app going into the background). */
    fun saveNow() {
        val s = session ?: return
        val dir = artworkDir ?: return
        noteTime()
        keepChoices(s)
        viewModelScope.launch(Dispatchers.IO) { runCatching { s.save(dir.path) } }
    }

    private fun start(s: CanvasSession, dir: File) {
        // the canvas's colours are P3 or sRGB numbers: the colour panel and swatches follow
        refreshCanvasSpace(s)
        // layers may take a quarter of the device's memory: every layer stays on the GPU
        val memory = android.app.ActivityManager.MemoryInfo()
        getApplication<Application>().getSystemService(android.app.ActivityManager::class.java)?.getMemoryInfo(memory)
        if (memory.totalMem > 0) io.github.artcraftandroid.procraft.core.setLayerBudget((memory.totalMem / 4).toULong())
        s.setSelectionVisibility(selectionVisibility)
        s.setBrushScaling(brushOnScreen)
        // as it was left, or fitted
        if (!orientationMemory) s.forgetSavedView()
        s.setInputSettings(inputStabilization, inputMotionFiltering, inputMotionExpression, inputPressureSmoothing, inputCurve.flatMap { listOf(it.x, it.y) })
        applyAppSettings(s)
        timeLapse = TimeLapse(dir).also { timeLapseRecording = it.recording }
        // the library as kept, else edits kept by earlier versions
        val kept = brushPrefs.getString(BRUSH_LIBRARY, null)
        val loaded = kept != null && s.loadBrushLibrary(kept)
        if (!loaded) brushPrefs.getString(BRUSH_CHANGES, null)?.let { s.applyBrushChanges(it) }
        // brushes and sets shipped since the library was kept join it, once
        val seen = brushPrefs.getString(BRUSHES_SEEN, null)?.split(',')?.mapNotNull { it.toUIntOrNull() } ?: FIRST_SHIPPED
        val shipped = if (loaded) s.addShippedBrushes(seen) else s.shippedBrushCounts()
        if (shipped != seen || !brushPrefs.contains(BRUSHES_SEEN)) {
            brushPrefs.edit().apply {
                // kept with them, or the next start would load the library without them
                if (loaded) putString(BRUSH_LIBRARY, s.savedBrushLibrary())
                putString(BRUSHES_SEEN, shipped.joinToString(","))
            }.apply()
        }
        // the brushes in use as they were left (they go by place in the kept library)
        if (loaded) brushPrefs.getString(BRUSH_CHOICES, null)?.let { s.loadBrushChoices(it) }
        session = s
        // brush files that came while no canvas was open
        addPendingBrushes(s)
        pushSecondary()
        artworkDir = dir
        applyFitInsets()
        attachProjection()
        activeSince = android.os.SystemClock.elapsedRealtime()
        thumbnails.clear()
        previews.clear()
        shadedNames.clear()
        panel = Panel.NONE
        barsHidden = false
        transforming = false
        transformBox = null
        adjustment = null
        selecting = false
        hasSelection = false
        freehand = emptyList()
        tool = ToolKind.PAINT
        applyColor()
        readSliders()
        refresh()
        brushSets = s.brushLibrary()
        readSetChoices()
        currentBrush = s.currentBrush()
        screen = Screen.CANVAS
        // a new artwork exists in the gallery from the start
        viewModelScope.launch(Dispatchers.IO) { runCatching { s.save(dir.path) } }
        startAutosave(s, dir)
    }

    private var autosave: kotlinx.coroutines.Job? = null

    /**
     * Keeps [s] saved into [dir] while it is open: a few seconds after it changes, once nothing is
     * under way that saving would cut short (only the layers that changed are written).
     */
    private fun startAutosave(s: CanvasSession, dir: File) {
        autosave?.cancel()
        autosave = viewModelScope.launch(Dispatchers.IO) {
            var saved = s.edits()
            while (session === s) {
                kotlinx.coroutines.delay(AUTOSAVE_MS)
                val now = s.edits()
                if (now != saved && session === s && runCatching { s.saveIfIdle(dir.path) }.getOrDefault(false)) saved = now
            }
        }
    }

    fun select(tool: ToolKind) {
        feedback()
        session?.setTool(tool)
        this.tool = tool
        readSliders()
        currentBrush = session?.currentBrush()
    }

    /** A tool button: the first tap picks the tool, the next opens its brush library. */
    // the drawing guide

    fun setGuideOn(on: Boolean) {
        guide = session?.setGuideOn(on)
    }

    /** Opens the guide editor (turning the guide on), the canvas fitted between its bars. */
    fun openGuideEditor() {
        val s = session ?: return
        endQuickShape()
        closePanels()
        if (guide?.on != true) s.setGuideOn(true)
        val d = density
        guide = s.guideEditBegin(GUIDE_FIT_SIDE_DP * d, GUIDE_FIT_TOP_DP * d, windowSize.x - GUIDE_FIT_SIDE_DP * d, windowSize.y - GUIDE_FIT_BOTTOM_DP * d)
        guideEditing = true
        refreshView()
    }

    fun closeGuideEditor(keep: Boolean) {
        guide = session?.guideEditEnd(keep)
        guideEditing = false
        symmetryOptionsOpen = false
        refreshView()
    }

    fun chooseGuideKind(kind: GuideType) {
        guide = session?.setGuideKind(kind)
        symmetryOptionsOpen = false
    }

    /** The shown kind's look, each left as it is when not given. */
    fun setGuideLook(opacity: Float? = null, thickness: Float? = null, size: Float? = null, hue: Float? = null) {
        val g = guide ?: return
        guide = session?.setGuideLook(opacity ?: g.opacity, thickness ?: g.thickness, size ?: g.size, hue ?: g.hue)
    }

    fun setGuideSymmetry(kind: SymmetryType, rotational: Boolean) {
        guide = session?.setGuideSymmetry(kind, rotational)
    }

    fun setGuideAssisted(on: Boolean) {
        guide = session?.setGuideAssisted(on)
    }

    /** What a touch at [p] in the guide editor takes: the turn handle (-2), the centre (-1), or a vanishing point (its index); a tap elsewhere in perspective adds one. */
    fun guideGrab(p: Offset): Int? {
        val g = guide ?: return null
        val reach = QUICKSHAPE_REACH_DP * density
        symmetryOptionsOpen = false
        if (g.kind == GuideType.PERSPECTIVE) {
            val index = session?.touchVanishingPoint(p.x, p.y)?.toInt()
            guide = session?.guide()
            return index
        }
        return when {
            (Offset(g.handle[0], g.handle[1]) - p).getDistance() <= reach -> -2
            (Offset(g.centre[0], g.centre[1]) - p).getDistance() <= reach -> -1
            else -> null
        }
    }

    fun guideDrag(grab: Int?, p: Offset) {
        val s = session ?: return
        guide = when (grab) {
            null -> return
            -2 -> s.turnGuide(p.x, p.y)
            -1 -> s.moveGuideCentre(p.x, p.y)
            else -> s.moveVanishingPoint(grab.toUInt(), p.x, p.y)
        }
    }

    // copy and paste, flipping, pictures

    fun copyLayer() {
        val copied = session?.copyLayer()
        clipboard = copied ?: clipboard
        canPaste = clipboard != null
        closePanels()
        if (copied != null) showNotice("Layer copied")
    }

    fun copyCanvas() {
        val copied = session?.copyCanvas()
        clipboard = copied ?: clipboard
        canPaste = clipboard != null
        closePanels()
        if (copied != null) showNotice("Canvas copied")
    }

    fun cutLayer() {
        if (!currentTakesChanges()) {
            closePanels()
            return
        }
        val cut = session?.cutLayer()
        clipboard = cut ?: clipboard
        canPaste = clipboard != null
        closePanels()
        refresh()
        if (cut != null) showNotice("Layer cut")
    }

    fun paste() {
        val image = clipboard ?: return
        if (session?.paste(image) == null) message = noRoom()
        closePanels()
        refresh()
        if (panel == Panel.LAYERS) refreshThumbnails()
    }

    /** Layer Select: the layers drawn under a point (top first), listed there to choose from. */
    var layerChoice by mutableStateOf<Pair<Offset, List<ULong>>?>(null)

    /** Layer Select at window point [at]: the top layer drawn there becomes current, the others listed. */
    fun layerSelectAt(at: Offset) {
        val ids = session?.layersAt(at.x, at.y).orEmpty()
        if (ids.isEmpty()) {
            message = "Nothing is drawn there."
            return
        }
        selectLayer(ids.first())
        layerChoice = if (ids.size > 1) at to ids else null
    }

    /** The QuickMenu, open around this window point. */
    var quickMenuAt by mutableStateOf<Offset?>(null)

    /** The QuickMenu's actions, in order round from the top. */
    /** The QuickMenu's buttons, clockwise from the top: each set to an action (a hold changes it). */
    var quickMenuSlots by mutableStateOf(
        interfacePrefs.getString(QUICK_MENU, null)?.split(',')?.map { n -> QuickAction.entries.firstOrNull { it.name == n } ?: QuickAction.NONE }
            ?.takeIf { it.size == QuickAction.DEFAULTS.size } ?: QuickAction.DEFAULTS,
    )
        private set

    fun setQuickSlot(i: Int, a: QuickAction) {
        if (i !in quickMenuSlots.indices) return
        quickMenuSlots = quickMenuSlots.toMutableList().also { it[i] = a }
        interfacePrefs.edit().putString(QUICK_MENU, quickMenuSlots.joinToString(",") { it.name }).apply()
    }

    /**
     * A key of a hardware keyboard on the canvas ([ctrl] the command key): true if it is a
     * shortcut, done. Typing in a text field never comes here.
     */
    fun keyShortcut(keyCode: Int, ctrl: Boolean, shift: Boolean, alt: Boolean): Boolean {
        if (screen != Screen.CANVAS || session == null) return false
        when {
            ctrl && keyCode == android.view.KeyEvent.KEYCODE_Z -> if (shift) redo() else undo()
            ctrl && keyCode == android.view.KeyEvent.KEYCODE_C -> copyLayer()
            ctrl && keyCode == android.view.KeyEvent.KEYCODE_X -> cutLayer()
            ctrl && keyCode == android.view.KeyEvent.KEYCODE_V -> paste()
            ctrl -> return false
            keyCode == android.view.KeyEvent.KEYCODE_B -> select(ToolKind.PAINT)
            keyCode == android.view.KeyEvent.KEYCODE_E -> select(ToolKind.ERASE)
            keyCode == android.view.KeyEvent.KEYCODE_M -> pickNext = !pickNext
            keyCode == android.view.KeyEvent.KEYCODE_S -> toggleSelecting()
            keyCode == android.view.KeyEvent.KEYCODE_V -> toggleTransform()
            keyCode == android.view.KeyEvent.KEYCODE_U -> startAdjustment(Adjustment.HSB)
            keyCode == android.view.KeyEvent.KEYCODE_C -> toggle(Panel.COLOR)
            keyCode == android.view.KeyEvent.KEYCODE_X -> swapColors()
            keyCode == android.view.KeyEvent.KEYCODE_D -> clearSelection()
            keyCode == android.view.KeyEvent.KEYCODE_J -> duplicateCurrent()
            keyCode == android.view.KeyEvent.KEYCODE_K -> toggle(Panel.ACTIONS)
            keyCode == android.view.KeyEvent.KEYCODE_L -> toggle(Panel.LAYERS)
            keyCode == android.view.KeyEvent.KEYCODE_0 -> toggleBars()
            keyCode == android.view.KeyEvent.KEYCODE_DEL || keyCode == android.view.KeyEvent.KEYCODE_FORWARD_DEL -> clearCurrentLayer()
            // the brush a step smaller or bigger: 1%, with shift 5%, with alt 10%
            keyCode == android.view.KeyEvent.KEYCODE_LEFT_BRACKET || keyCode == android.view.KeyEvent.KEYCODE_RIGHT_BRACKET -> {
                val step = if (alt) 0.10f else if (shift) 0.05f else 0.01f
                val sign = if (keyCode == android.view.KeyEvent.KEYCODE_LEFT_BRACKET) -1f else 1f
                changeSize((size + sign * step).coerceIn(0f, 1f))
            }
            // arrows nudge what is being transformed a pixel at a time
            transforming && keyCode in listOf(
                android.view.KeyEvent.KEYCODE_DPAD_LEFT,
                android.view.KeyEvent.KEYCODE_DPAD_RIGHT,
                android.view.KeyEvent.KEYCODE_DPAD_UP,
                android.view.KeyEvent.KEYCODE_DPAD_DOWN,
            ) -> {
                val box = transformBox?.corners ?: return true
                if (box.size < 8) return true
                val cx = (0 until 4).map { box[it * 2] }.average().toFloat()
                val cy = (0 until 4).map { box[it * 2 + 1] }.average().toFloat()
                val far = 100_000f
                val (dx, dy) = when (keyCode) {
                    android.view.KeyEvent.KEYCODE_DPAD_LEFT -> -far to 0f
                    android.view.KeyEvent.KEYCODE_DPAD_RIGHT -> far to 0f
                    android.view.KeyEvent.KEYCODE_DPAD_UP -> 0f to -far
                    else -> 0f to far
                }
                session?.transformNudge(cx + dx, cy + dy)?.let { transformMoved(it) }
            }
            else -> return false
        }
        return true
    }

    /** Does [a], a QuickMenu button opened at window point [at]. */
    fun runQuickAction(a: QuickAction, at: Offset) {
        val current = layers.firstOrNull { it.current }
        when (a) {
            QuickAction.NONE -> Unit
            QuickAction.NEW_LAYER -> addLayer()
            QuickAction.DUPLICATE_LAYER -> current?.let(::duplicate)
            QuickAction.DELETE_LAYER -> current?.let(::delete)
            QuickAction.MERGE_DOWN -> current?.let(::mergeDown)
            QuickAction.CLEAR_LAYER -> clearCurrentLayer()
            QuickAction.FILL_LAYER -> current?.let(::fillLayer)
            QuickAction.SELECT_LAYER -> current?.let(::selectLayerContents)
            QuickAction.ALPHA_LOCK -> current?.let(::toggleAlphaLock)
            QuickAction.LAYER_VISIBILITY -> current?.let(::toggleVisible)
            QuickAction.COPY -> copyLayer()
            QuickAction.CUT -> cutLayer()
            QuickAction.PASTE -> paste()
            QuickAction.COPY_PASTE_MENU -> copyPasteOpen = true
            QuickAction.FLIP_HORIZONTAL -> flipCanvas(horizontal = true)
            QuickAction.FLIP_VERTICAL -> flipCanvas(horizontal = false)
            QuickAction.UNDO -> undo()
            QuickAction.REDO -> redo()
            QuickAction.PAINT -> select(ToolKind.PAINT)
            QuickAction.SMUDGE -> select(ToolKind.SMUDGE)
            QuickAction.ERASE -> select(ToolKind.ERASE)
            QuickAction.EYEDROPPER -> pickNext = true
            QuickAction.SWAP_COLORS -> swapColors()
            QuickAction.SELECTION -> toggleSelecting()
            QuickAction.INVERT_SELECTION -> invertSelection()
            QuickAction.TRANSFORM -> toggleTransform()
            QuickAction.ADJUSTMENTS -> adjustmentsPressed()
            QuickAction.COLORS -> toggle(Panel.COLOR)
            QuickAction.LAYERS -> toggle(Panel.LAYERS)
            QuickAction.BRUSHES -> toggle(Panel.BRUSHES)
            QuickAction.ACTIONS -> toggle(Panel.ACTIONS)
            QuickAction.INTERFACE -> toggleBars()
            QuickAction.FIT_CANVAS -> settleToFit()
            QuickAction.ADD_TEXT -> addText()
            QuickAction.QUICKSHAPE -> quickShapeLast()
            QuickAction.LAYER_SELECT -> layerSelectAt(at)
            QuickAction.DRAWING_GUIDE -> setGuideOn(guide?.on != true)
        }
    }

    /** Does what a gesture set to the QuickMenu does: opens it at [at], or closes it if open. */
    fun toggleQuickMenu(at: Offset) {
        quickMenuAt = if (quickMenuAt == null) at else null
    }

    /** The Copy & Paste panel (three fingers swiped down) is open. */
    var copyPasteOpen by mutableStateOf(false)

    /** Cut & Paste: what is selected (or the layer) cut out and put back as a new layer. */
    fun cutAndPaste() {
        cutLayer()
        paste()
    }

    /** Duplicate: the selection copied to a new layer, or without one the current layer. */
    fun duplicateCurrent() {
        if (hasSelection) {
            copyLayer()
            paste()
        } else {
            layers.firstOrNull { it.current }?.let(::duplicate)
        }
    }

    fun flipCanvas(horizontal: Boolean) {
        endQuickShape()
        session?.flipCanvas(horizontal)
        refresh()
    }

    /** A photo or picture file comes in as a new layer, fitted to the canvas, ready to transform. */
    fun insertPicture(uri: android.net.Uri) {
        val s = session ?: return
        closePanels()
        viewModelScope.launch {
            val image = withContext(Dispatchers.IO) { decodePicture(uri, io.github.artcraftandroid.procraft.ui.CanvasSpace.p3) }
            if (image == null) {
                message = "This picture can't be opened."
                return@launch
            }
            if (s.insertImage(image) != null) {
                refresh()
                toggleTransform()
            } else {
                message = noRoom()
            }
        }
    }

    /** Whether the picture at [uri] is in Display P3. */
    private fun pictureIsP3(uri: android.net.Uri): Boolean = runCatching {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        getApplication<Application>().contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
        bounds.outColorSpace == android.graphics.ColorSpace.get(android.graphics.ColorSpace.Named.DISPLAY_P3)
    }.getOrDefault(false)

    /**
     * A picture's straight RGBA pixels, at most [MAX_PICTURE] pixels on a side, its colours turned
     * into Display P3 ([p3]) or sRGB.
     */
    private fun decodePicture(uri: android.net.Uri, p3: Boolean): io.github.artcraftandroid.procraft.core.ThumbnailImage? = runCatching {
        val resolver = getApplication<Application>().contentResolver
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
        var sample = 1
        while (maxOf(bounds.outWidth, bounds.outHeight) / sample > MAX_PICTURE) sample *= 2
        val options = BitmapFactory.Options().apply {
            inSampleSize = sample
            inPremultiplied = false
            inPreferredConfig = Bitmap.Config.ARGB_8888
            inPreferredColorSpace = android.graphics.ColorSpace.get(if (p3) android.graphics.ColorSpace.Named.DISPLAY_P3 else android.graphics.ColorSpace.Named.SRGB)
        }
        val bitmap = resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, options) } ?: return null
        val buffer = ByteBuffer.allocate(bitmap.width * bitmap.height * 4)
        bitmap.copyPixelsToBuffer(buffer)
        io.github.artcraftandroid.procraft.core.ThumbnailImage(bitmap.width.toUInt(), bitmap.height.toUInt(), buffer.array())
    }.getOrNull()

    // QuickShape

    /** The stroke being drawn was held still: it snaps to a shape if it looks like one. */
    fun quickShapeHold() {
        val info = session?.quickshapeHold() ?: return
        quickShape = info
        quickShapeEditing = false
        showNotice(snappedTo(info.kind))
    }

    /** What a stroke snapped to, for the band under the top bar. */
    private fun snappedTo(kind: QuickShapeKind): String = "Snapped to " + when (kind) {
        QuickShapeKind.LINE -> "a line"
        QuickShapeKind.POLYLINE -> "a polyline"
        QuickShapeKind.ARC -> "an arc"
        QuickShapeKind.ELLIPSE -> "an ellipse"
        QuickShapeKind.CIRCLE -> "a circle"
        QuickShapeKind.TRIANGLE -> "a triangle"
        QuickShapeKind.EQUILATERAL_TRIANGLE -> "an equilateral triangle"
        QuickShapeKind.QUADRILATERAL -> "a quadrilateral"
        QuickShapeKind.RECTANGLE -> "a rectangle"
        QuickShapeKind.SQUARE -> "a square"
    }

    /** The last stroke snaps to the shape it looks like, if nothing has been done since (a QuickShape trigger). */
    fun quickShapeLast() {
        val info = session?.quickshapeLast() ?: return
        quickShape = info
        quickShapeEditing = false
        showNotice(snappedTo(info.kind))
        refreshFlags()
    }

    fun chooseQuickShape(kind: QuickShapeKind) {
        quickShape = session?.quickshapeChoose(kind) ?: quickShape
    }

    fun moveQuickShapeHandle(index: Int, at: Offset) {
        quickShape = session?.quickshapeMoveHandle(index.toUInt(), at.x, at.y) ?: quickShape
    }

    /** The handle of the snapped shape under window point [p], while its handles show. */
    fun quickShapeHandleAt(p: Offset): Int? {
        val h = quickShape?.handles ?: return null
        if (!quickShapeEditing) return null
        return (0 until h.size / 2).minByOrNull { (Offset(h[it * 2], h[it * 2 + 1]) - p).getDistance() }
            ?.takeIf { (Offset(h[it * 2], h[it * 2 + 1]) - p).getDistance() <= QUICKSHAPE_REACH_DP * density }
    }

    /** Keeps the snapped shape as drawn. */
    fun endQuickShape() {
        if (quickShape == null) return
        session?.quickshapeDone()
        quickShape = null
        quickShapeEditing = false
        refreshFlags()
    }

    fun toolPressed(tool: ToolKind) {
        endQuickShape()
        if (transforming || selecting || adjustment != null) {
            // leaving transform, selection or an adjustment for a tool keeps what they made, and only picks the tool
            endTransform(true)
            closeSelecting()
            endAdjustment(true)
            select(tool)
            return
        }
        if (this.tool == tool) {
            currentBrush?.let { shownSet = it.set.toInt() }
            toggle(Panel.BRUSHES)
        } else {
            select(tool)
            if (panel == Panel.BRUSHES) panel = Panel.NONE
        }
    }

    /**
     * A tool button held: that tool takes the brush the current one has (erasing with the painting
     * brush, say), and becomes the tool.
     */
    fun toolHeld(target: ToolKind) {
        val s = session ?: return
        val brush = currentBrush ?: return
        if (transforming || selecting || adjustment != null || target == tool) return
        endQuickShape()
        select(target)
        if (s.selectBrush(brush.set, brush.index)) {
            currentBrush = s.currentBrush()
            readSliders()
            brushSets = s.brushLibrary()
            readSetChoices()
            keepChoices(s)
        }
        if (panel == Panel.BRUSHES) panel = Panel.NONE
        showNotice(
            when (target) {
                ToolKind.PAINT -> "Painting"
                ToolKind.SMUDGE -> "Smudging"
                ToolKind.ERASE -> "Erasing"
            } + " with the current brush",
        )
    }

    fun showSet(index: Int) {
        shownSet = index
        // the set's last chosen brush becomes the current one, as choosing it again would
        val choice = setChoice[index] ?: return
        brushSets.getOrNull(index)?.brushes?.getOrNull(choice)?.let { chooseBrush(it) }
    }

    // ---- time-lapse ----

    private var timeLapse: TimeLapse? = null
    var timeLapseRecording by mutableStateOf(true)
        private set

    /** The replay shows over everything. */
    var timeLapseReplaying by mutableStateOf(false)

    /** Frames recorded, for the replay. */
    fun timeLapseFrames() = timeLapse?.count ?: 0

    fun timeLapseFrame(i: Int): Bitmap? = timeLapse?.frame(i)

    fun changeTimeLapseRecording(on: Boolean) {
        timeLapse?.recording = on
        timeLapseRecording = on
    }

    private var frameJob: Job? = null
    private var frameStale = false

    /** Records the canvas as a time-lapse frame, one at a time while changes come. */
    private fun recordFrame() {
        val t = timeLapse ?: return
        if (!t.recording) return
        if (frameJob?.isActive == true) {
            frameStale = true
            return
        }
        val s = session ?: return
        frameJob = viewModelScope.launch {
            do {
                frameStale = false
                withContext(Dispatchers.Default) {
                    // frames as large as the video they make
                    val shot = s.canvasThumbnail((timeLapse?.settings?.side ?: TimeLapse.FRAME_SIDE).toUInt()) ?: return@withContext
                    t.record(shot.rgba, shot.width.toInt(), shot.height.toInt())
                }
            } while (frameStale)
        }
    }

    /** The time-lapse as a video in the cache, to share: all of it, or [seconds] long. */
    suspend fun exportTimeLapse(seconds: Int?): File? {
        val t = timeLapse ?: return null
        val s = session ?: return null
        return withContext(Dispatchers.IO) {
            val dir = File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }
            val base = s.name().filter { it.isLetterOrDigit() || it in " _-" }.trim().ifEmpty { "Untitled Artwork" }
            val out = File(dir, "$base.mp4")
            out.delete()
            if (t.export(out, seconds)) out else null
        }
    }

    // ---- crop and resize ----

    /** The rectangle being kept (canvas pixels: left, top, width, height), or null outside crop. */
    var cropRect by mutableStateOf<androidx.compose.ui.geometry.Rect?>(null)
        private set
    var cropAngle by mutableStateOf(0f)
        private set
    var cropResample by mutableStateOf(false)
        private set
    /** With resampling, the size what is kept becomes. */
    var cropSize by mutableStateOf(0 to 0)
        private set
    var cropAspectLocked by mutableStateOf(false)
    var cropSettingsOpen by mutableStateOf(false)
    var cropSnapping by mutableStateOf(false)
    /** The canvas's dots per inch, as the crop's settings have it. */
    var cropDpi by mutableStateOf(132f)
        private set

    fun changeCropDpi(dpi: Float) {
        if (dpi.isFinite() && dpi >= 1f) cropDpi = dpi.coerceAtMost(9999f)
    }

    /** The canvas's size, pixels. */
    fun canvasSize(): Pair<Int, Int> = session?.info()?.let { it.width.toInt() to it.height.toInt() } ?: (0 to 0)

    fun beginCrop() {
        closePanels()
        val (w, h) = canvasSize()
        if (w == 0) return
        // the canvas fitted to the screen and shrunk, in the middle, so the frame's handles are clear
        session?.fitCanvasScaled(CROP_SCALE)
        cropDpi = session?.info()?.dpi ?: 132f
        refreshView()
        followView()
        cropRect = androidx.compose.ui.geometry.Rect(0f, 0f, w.toFloat(), h.toFloat())
        cropAngle = 0f
        cropResample = false
        cropSize = w to h
        cropSettingsOpen = false
    }

    fun resetCrop() = beginCrop()

    fun changeCropRect(r: androidx.compose.ui.geometry.Rect) {
        val w = r.width.coerceAtLeast(1f)
        val h = r.height.coerceAtLeast(1f)
        cropRect = androidx.compose.ui.geometry.Rect(r.left, r.top, r.left + w, r.top + h)
        if (!cropResample) cropSize = w.roundToInt() to h.roundToInt()
    }

    fun changeCropAngle(radians: Float) {
        cropAngle = radians
    }

    fun changeCropResample(on: Boolean) {
        cropResample = on
        cropRect?.let { cropSize = it.width.roundToInt() to it.height.roundToInt() }
    }

    /** A width or height typed in: resampling, it is the new size; otherwise the kept rectangle's. */
    fun changeCropSize(width: Int?, height: Int?) {
        val r = cropRect ?: return
        val (w0, h0) = cropSize
        var w = (width ?: w0).coerceIn(1, MAX_CANVAS_EDGE)
        var h = (height ?: h0).coerceIn(1, MAX_CANVAS_EDGE)
        if (cropAspectLocked && w0 > 0 && h0 > 0) {
            if (width != null) h = (w * h0.toFloat() / w0).roundToInt().coerceIn(1, MAX_CANVAS_EDGE)
            else if (height != null) w = (h * w0.toFloat() / h0).roundToInt().coerceIn(1, MAX_CANVAS_EDGE)
        }
        cropSize = w to h
        if (!cropResample) cropRect = androidx.compose.ui.geometry.Rect(r.left, r.top, r.left + w, r.top + h)
    }

    /** How many layers a canvas of the size kept can hold. */
    fun cropLayerCount(): Int {
        val (w, h) = cropSize
        val bytes = w.toLong() * h.toLong() * 4
        return if (bytes <= 0) 0 else (LAYER_BUDGET / bytes).toInt().coerceAtMost(999)
    }

    fun endCrop(keep: Boolean) {
        val r = cropRect ?: return
        val s = session
        cropRect = null
        cropSettingsOpen = false
        // either way the canvas is fitted to the screen again: cancelled, growing back into place
        if (!keep || s == null) {
            s?.easeToFit()
            refreshView()
            followView()
            return
        }
        s.fitView()
        val (w, h) = cropSize
        if (abs(cropDpi - (s.info().dpi)) > 0.01f) s.setDpi(cropDpi)
        // nothing to crop: the whole canvas, unturned, at its size
        val (cw, ch) = canvasSize()
        val same = r.left == 0f && r.top == 0f && r.width.roundToInt() == cw && r.height.roundToInt() == ch && cropAngle == 0f && w == cw && h == ch
        if (same) {
            refresh()
            refreshView()
            return
        }
        if (s.cropCanvas(r.left, r.top, r.width, r.height, cropAngle, cropResample, w.toUInt(), h.toUInt())) {
            thumbnails.clear()
            refresh()
            refreshView()
            if (panel == Panel.LAYERS) refreshThumbnails()
        }
    }

    // ---- the reference window ----

    enum class ReferenceMode { CANVAS, IMAGE, FACE }

    var referenceOn by mutableStateOf(false)
        private set
    var referenceMode by mutableStateOf(ReferenceMode.CANVAS)
        private set

    /** The whole canvas as it is, kept up to date while the window shows it. */
    var referenceCanvas by mutableStateOf<ImageBitmap?>(null)
        private set
    var referenceImage by mutableStateOf<ImageBitmap?>(null)
        private set

    /** Where the window is (window pixels), and the size of its picture. */
    var referenceOffset by mutableStateOf(Offset.Unspecified)
        private set
    var referenceSize by mutableStateOf(DpSize(REFERENCE_WIDTH_DP.dp, REFERENCE_HEIGHT_DP.dp))
        private set

    private var referenceJob: Job? = null
    private var referenceStale = false

    fun changeReferenceOn(on: Boolean) {
        referenceOn = on
        if (on) refreshReference()
    }

    fun changeReferenceMode(mode: ReferenceMode) {
        referenceMode = mode
        refreshReference()
    }

    /** The window's first place on a screen [width] × [height] (pixels), [tall] (pixels) tall in all: in the middle, near the bottom. */
    fun placeReference(width: Float, height: Float, tall: Float) {
        if (referenceOffset != Offset.Unspecified) return
        val w = referenceSize.width.value * density
        referenceOffset = Offset((width - w) / 2f, (height - REFERENCE_BOTTOM_DP * density - tall).coerceAtLeast(0f))
    }

    fun moveReference(by: Offset) {
        referenceOffset += by
    }

    /** A drag on a size handle; from the left one the window's left edge moves. */
    fun resizeReference(dx: Dp, dy: Dp, fromLeft: Boolean) {
        val old = referenceSize
        val w = (old.width + if (fromLeft) -dx else dx).coerceIn(REFERENCE_MIN_DP.dp, REFERENCE_MAX_DP.dp)
        val h = (old.height + dy).coerceIn(REFERENCE_MIN_DP.dp, REFERENCE_MAX_DP.dp)
        if (fromLeft) referenceOffset = referenceOffset.copy(x = referenceOffset.x + (old.width - w).value * density)
        referenceSize = DpSize(w, h)
    }

    fun importReference(uri: android.net.Uri) {
        viewModelScope.launch {
            val image = withContext(Dispatchers.IO) {
                runCatching {
                    val resolver = getApplication<Application>().contentResolver
                    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                    resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
                    var sample = 1
                    while (maxOf(bounds.outWidth, bounds.outHeight) / sample > MAX_PICTURE) sample *= 2
                    resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, BitmapFactory.Options().apply { inSampleSize = sample }) }
                }.getOrNull()
            }
            if (image != null) referenceImage = image.asImageBitmap() else message = "This picture can't be opened."
        }
    }

    fun clearReference() {
        referenceImage = null
    }

    /** Reads the canvas again for the window, at most every so often while changes come. */
    fun refreshReference() {
        if (!referenceOn || referenceMode != ReferenceMode.CANVAS) return
        if (referenceJob?.isActive == true) {
            referenceStale = true
            return
        }
        val s = session ?: return
        referenceJob = viewModelScope.launch {
            do {
                referenceStale = false
                val image = withContext(Dispatchers.Default) {
                    val t = s.canvasThumbnail(REFERENCE_PIXELS.toUInt()) ?: return@withContext null
                    val bitmap = io.github.artcraftandroid.procraft.ui.CanvasSpace.tag(Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), Bitmap.Config.ARGB_8888))
                    bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(t.rgba))
                    bitmap.asImageBitmap()
                }
                if (image != null) referenceCanvas = image
                kotlinx.coroutines.delay(REFERENCE_INTERVAL_MS)
            } while (referenceStale)
        }
    }

    // ---- text ----

    /** The text being edited, as it stands; null when no text is being edited. */
    var textInfo by mutableStateOf<TextInfo?>(null)
        private set

    /** What the hidden text field holds: the text with its caret and selection. */
    var textField by mutableStateOf(TextFieldValue(""))
        private set

    /** The style sheet (fonts, styles, design, attributes) is open over the keyboard. */
    var textStyleOpen by mutableStateOf(false)

    /** The text as it was when the style sheet opened, for its Cancel. */
    private var textBeforeStyle: TextInfo? = null

    /** The canvas's dots per inch. */
    fun canvasDpi(): Float = session?.info()?.dpi?.coerceAtLeast(1f) ?: 132f

    fun openTextStyle() {
        textBeforeStyle = textInfo
        textStyleOpen = true
    }

    /** The style sheet's Cancel: its changes go back (the text stays), and the keyboard returns. */
    fun cancelTextStyle() {
        textBeforeStyle?.let { before -> changeText { before.copy(text = it.text) } }
        textStyleOpen = false
    }

    private val systemClipboard get() = getApplication<Application>().getSystemService(android.content.ClipboardManager::class.java)

    /** Copies the selected text (all of it if none is), and with [cut] takes it out. */
    fun copyText(cut: Boolean) {
        val v = textField
        val sel = if (v.selection.collapsed) TextRange(0, v.text.length) else v.selection
        val part = v.text.substring(sel.min, sel.max)
        if (part.isEmpty()) return
        systemClipboard?.setPrimaryClip(android.content.ClipData.newPlainText("Text", part))
        if (cut) changeTextField(TextFieldValue(v.text.removeRange(sel.min, sel.max), TextRange(sel.min)))
    }

    /** Puts the copied text in place of the selection. */
    fun pasteText() {
        val add = systemClipboard?.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.coerceToText(getApplication())?.toString() ?: return
        val v = textField
        val text = v.text.replaceRange(v.selection.min, v.selection.max, add)
        changeTextField(TextFieldValue(text, TextRange(v.selection.min + add.length)))
    }

    private val fontDir get() = File(getApplication<Application>().filesDir, "fonts")

    /** Copies a font file into the app and offers it; the text being edited takes it. */
    fun importFont(uri: android.net.Uri) {
        viewModelScope.launch {
            val family = withContext(Dispatchers.IO) {
                runCatching {
                    val resolver = getApplication<Application>().contentResolver
                    val name = resolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                        if (c.moveToFirst()) c.getString(0) else null
                    } ?: "Font.ttf"
                    val safe = name.filter { it.isLetterOrDigit() || it in " ._-" }.ifEmpty { "Font.ttf" }
                    val file = File(fontDir.apply { mkdirs() }, if (safe.substringAfterLast('.', "").lowercase() in listOf("ttf", "otf")) safe else "$safe.ttf")
                    resolver.openInputStream(uri)?.use { input -> file.outputStream().use { input.copyTo(it) } }
                    // a file that isn't a font is dropped
                    if (android.graphics.Typeface.createFromFile(file) == android.graphics.Typeface.DEFAULT && file.length() == 0L) file.delete()
                    TextRenderer.FILE + file.path
                }.getOrNull()
            }
            TextRenderer.loadImported(fontDir)
            if (family != null && TextRenderer.families.any { it.family == family }) {
                changeText { it.copy(font = family) }
                showNotice("Font imported")
            }
        }
    }

    /** A stroke landed on a text layer: offer to rasterize it. */
    var askRasterize by mutableStateOf(false)

    private var textJob: Job? = null
    private var textGeneration = 0

    /** New text in the middle of the canvas, all of it selected for typing over. */
    fun addText() {
        val s = session ?: return
        closePanels()
        val c = s.info()
        val rgb = colorRgb()
        // a medium sans about an eighth of the canvas's width, in a box as wide as the canvas,
        // centred where the canvas shows above the keyboard (about a quarter of the way down)
        val w = c.width.toFloat()
        var info = TextInfo(
            text = "Text", font = "sans-serif", style = "Medium",
            size = w * TEXT_SIZE_OF_WIDTH,
            kerning = 0f, tracking = 0f, leading = 0f, baseline = 0f, opacity = 1f,
            align = TextAlignment.CENTER, underline = false, outline = false, vertical = false, allCaps = false,
            r = ((rgb shr 16) and 255) / 255f, g = ((rgb shr 8) and 255) / 255f, b = (rgb and 255) / 255f,
            x = w * (1f - TEXT_BOX_OF_WIDTH) / 2f, y = 0f, width = w * TEXT_BOX_OF_WIDTH,
        )
        val box = TextRenderer.bounds(info)
        val h = c.height.toFloat()
        val middle = windowToCanvas(Offset(windowSize.x / 2f, windowSize.y * TEXT_SCREEN_Y))?.y ?: (h / 4f)
        info = info.copy(y = (middle - box.height() / 2f).coerceIn(0f, (h - box.height()).coerceAtLeast(0f)))
        if (s.textBegin(null, info) == null) {
            message = noRoom()
            return
        }
        startText(info)
    }

    /** Edits a text layer's text again. */
    fun editText(l: LayerInfo) {
        val s = session ?: return
        val info = s.layerText(l.id) ?: return
        closePanels()
        s.textBegin(l.id, info) ?: return
        startText(info)
    }

    private fun startText(info: TextInfo) {
        textInfo = info
        textField = TextFieldValue(info.text, TextRange(0, info.text.length))
        textStyleOpen = false
        renderText()
        refresh()
    }

    fun changeTextField(v: TextFieldValue) {
        textField = v
        if (v.text != textInfo?.text) changeText { it.copy(text = v.text) }
    }

    fun changeText(f: (TextInfo) -> TextInfo) {
        val info = textInfo?.let(f) ?: return
        textInfo = info
        session?.setText(info)
        renderText()
    }

    /** Draws the text into its layer off the main thread; only the latest drawing lands. */
    private fun renderText() {
        val s = session ?: return
        val info = textInfo ?: return
        val c = s.info()
        val generation = ++textGeneration
        textJob?.cancel()
        textJob = viewModelScope.launch {
            val pixels = withContext(Dispatchers.Default) { TextRenderer.render(info, c.width.toInt(), c.height.toInt()) }
            if (generation == textGeneration) s.putTextPixels(pixels)
        }
    }

    /** Ends editing text: [keep] makes it one undo step, otherwise it goes back. */
    fun endText(keep: Boolean) {
        val s = session ?: return
        if (textInfo == null) return
        val job = textJob
        textInfo = null
        textStyleOpen = false
        viewModelScope.launch {
            // the last drawing lands first
            job?.join()
            s.textEnd(keep)
            refresh()
            if (panel == Panel.LAYERS) refreshThumbnails()
        }
    }

    /** Moves the text being edited by a drag from window point [from] to [to]. */
    fun moveText(from: Offset, to: Offset) {
        val a = windowToCanvas(from) ?: return
        val b = windowToCanvas(to) ?: return
        changeText { it.copy(x = it.x + b.x - a.x, y = it.y + b.y - a.y) }
    }

    /** Whether the current layer holds editable text. */
    fun currentIsText() = layers.firstOrNull { it.current }?.text == true

    /** Why the current layer turned a change away. */
    enum class Blocked { LOCKED, HIDDEN, EMPTY_GROUP }

    /** The current layer turned a change away, and why: a dialog offers to unlock or show it, or open Layers. */
    var blocked by mutableStateOf<Pair<Blocked, LayerInfo>?>(null)
        private set

    /**
     * Whether the current layer takes a change (a stroke, a transform, an adjustment, clearing,
     * cutting); if not (locked, hidden, or an empty group), a dialog says why. What was tried is
     * not done afterwards.
     */
    fun currentTakesChanges(): Boolean {
        val l = layers.firstOrNull { it.current } ?: return true
        val why = when {
            l.locked -> Blocked.LOCKED
            !l.visible -> Blocked.HIDDEN
            l.kind == LayerType.GROUP && layers.none { it.parent == l.id } -> Blocked.EMPTY_GROUP
            else -> return true
        }
        blocked = why to l
        return false
    }

    /** The dialog's answer: unlock or show the layer ([fix]), open Layers ([openLayers]), or neither. */
    fun answerBlocked(fix: Boolean, openLayers: Boolean) {
        val (why, l) = blocked ?: return
        blocked = null
        if (fix) {
            when (why) {
                Blocked.LOCKED -> toggleLocked(l)
                Blocked.HIDDEN -> toggleVisible(l)
                Blocked.EMPTY_GROUP -> Unit
            }
        }
        if (openLayers) panel = Panel.LAYERS
    }

    fun rasterize(l: LayerInfo) {
        layerChange { it.rasterizeLayer(l.id) }
        showNotice("Text rasterized", warning = true)
    }

    // editing the library

    /** The set whose name is being edited in place. */
    var renamingSet by mutableStateOf<Int?>(null)

    /** The studio is open on a brush just made: cancelling throws it away. */
    private var studioNew = false

    /** After a change to the library: read it again, keep it, and draw previews again (they go by place). */
    private fun libraryChanged() {
        val s = session ?: return
        brushSets = s.brushLibrary()
        currentBrush = s.currentBrush()
        readSetChoices()
        shownSet = shownSet.coerceIn(0, (brushSets.size - 1).coerceAtLeast(0))
        readSliders()
        previews.clear()
        shadedNames.clear()
        brushPrefs.edit().putString(BRUSH_LIBRARY, s.savedBrushLibrary()).putString(BRUSH_CHOICES, s.savedBrushChoices()).apply()
    }

    fun duplicateBrush(b: BrushInfo) {
        session?.duplicateBrush(b.set, b.index) ?: return
        libraryChanged()
    }

    fun deleteBrush(b: BrushInfo) {
        if (session?.deleteBrush(b.set, b.index) == true) libraryChanged()
    }

    /** Moves [b] to place [index] of set [set] (a brush dragged onto a set, or along its own). */
    fun moveBrush(b: BrushInfo, set: Int, index: Int) {
        if (set == b.set.toInt() && index == b.index.toInt()) return
        if (session?.moveBrush(b.set, b.index, set.toUInt(), index.toUInt()) == true) libraryChanged()
    }

    /** A new brush at the top of the shown set, opened in the studio. */
    fun newBrush() {
        val s = session ?: return
        s.newBrush(shownSet.toUInt()) ?: return
        libraryChanged()
        studioNew = true
        openStudio()
    }

    /** A new set at the top of the library, shown, with its name ready to edit. */
    fun newBrushSet() {
        val s = session ?: return
        shownSet = s.newBrushSet().toInt()
        libraryChanged()
        renamingSet = shownSet
    }

    fun renameBrushSet(i: Int, name: String) {
        renamingSet = null
        if (session?.renameBrushSet(i.toUInt(), name) == true) libraryChanged()
    }

    fun changeBrushSetIcon(i: Int, icon: Int) {
        if (session?.setBrushSetIcon(i.toUInt(), icon.toUInt()) == true) libraryChanged()
    }

    fun duplicateBrushSet(i: Int) {
        shownSet = session?.duplicateBrushSet(i.toUInt())?.toInt() ?: return
        libraryChanged()
    }

    fun deleteBrushSet(i: Int) {
        if (session?.deleteBrushSet(i.toUInt()) != true) return
        if (shownSet >= i) shownSet = (shownSet - 1).coerceAtLeast(0)
        libraryChanged()
    }

    /** A brush, or (no brush) the shown set, written to a brush or set file to share. */
    suspend fun brushFileToShare(b: BrushInfo?): File? = withContext(Dispatchers.IO) {
        val s = session ?: return@withContext null
        val (name, bytes) = if (b != null) {
            b.name to (s.brushAsFile(b.set, b.index) ?: return@withContext null)
        } else {
            val set = brushSets.getOrNull(shownSet) ?: return@withContext null
            set.name to (s.brushSetAsFile(shownSet.toUInt()) ?: return@withContext null)
        }
        runCatching {
            val dir = File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }
            val safe = name.replace(Regex("[^A-Za-z0-9 _-]"), "_").ifBlank { "Brush" }
            File(dir, safe + if (b != null) ".$BRUSH_EXTENSION" else ".$BRUSH_SET_EXTENSION").apply { writeBytes(bytes) }
        }.getOrNull()
    }

    /** A brush or set file from elsewhere: it joins the library and its set is shown. */
    fun importBrushes(uri: android.net.Uri) {
        viewModelScope.launch {
            val bytes = withContext(Dispatchers.IO) {
                runCatching { getApplication<Application>().contentResolver.openInputStream(uri)?.use { it.readBytes() } }.getOrNull()
            } ?: return@launch
            if (!addBrushes(bytes)) message = "This file has no brushes in it."
        }
    }

    /**
     * Brushes from a brush or set file [bytes] into the library (shown), or, with no canvas open,
     * kept until one opens; false if the file has none.
     */
    private suspend fun addBrushes(bytes: ByteArray): Boolean {
        val s = session
        if (s == null) {
            if (!isBrushFile(bytes)) return false
            withContext(Dispatchers.IO) {
                pendingBrushDir.mkdirs()
                File(pendingBrushDir, "${System.nanoTime()}.$BRUSH_EXTENSION").writeBytes(bytes)
            }
            message = "Brushes will be added to the library when a canvas opens."
            return true
        }
        val set = s.importBrushes(bytes)
            // files of ours, from before
            ?: runCatching { s.importBrushFile(shownSet.toUInt(), bytes.decodeToString()) }.getOrNull()
            ?: return false
        shownSet = set.toInt()
        libraryChanged()
        return true
    }

    /** Brush files kept while no canvas was open, added now. */
    private fun addPendingBrushes(s: CanvasSession) {
        val files = pendingBrushDir.listFiles()?.sortedBy { it.name } ?: return
        var any = false
        for (f in files) {
            runCatching { s.importBrushes(f.readBytes()) }.getOrNull()?.let {
                shownSet = it.toInt()
                any = true
            }
            f.delete()
        }
        if (any) {
            libraryChanged()
            message = "Brushes added to the library."
        }
    }

    private val pendingBrushDir get() = File(getApplication<Application>().filesDir, "pending-brushes")

    /** Whether [bytes] is a zip holding a brush or a set of them. */
    private fun isBrushFile(bytes: ByteArray): Boolean = runCatching {
        java.util.zip.ZipInputStream(bytes.inputStream()).use { zip ->
            generateSequence { zip.nextEntry }.any { it.name == "Brush.archive" || it.name == "brushset.plist" || it.name.endsWith("/Brush.archive") }
        }
    }.getOrDefault(false)

    fun chooseBrush(b: BrushInfo) {
        val s = session ?: return
        val before = currentBrush
        if (s.selectBrush(b.set, b.index)) {
            // the set left behind keeps showing the brush it had
            before?.let { setChoice[it.set.toInt()] = it.index.toInt() }
            currentBrush = s.currentBrush()
            setChoice[b.set.toInt()] = b.index.toInt()
            readSliders()
            keepChoices(s)
        }
    }

    /** Each set's last chosen brush, which it shows highlighted (by place in the set). */
    val setChoice = mutableStateMapOf<Int, Int>()

    /** Each set's last choice, as the library read last has it. */
    private fun readSetChoices() {
        setChoice.clear()
        brushSets.forEachIndexed { i, set -> set.chosen?.let { setChoice[i] = it.toInt() } }
    }

    /** Keeps the tools' brushes, the sets' choices and each brush's sliders for the next session. */
    private fun keepChoices(s: CanvasSession) {
        brushPrefs.edit().putString(BRUSH_CHOICES, s.savedBrushChoices()).apply()
    }

    /** Whether [b] shows highlighted: the current brush, or its set's last choice. */
    fun highlighted(b: BrushInfo): Boolean {
        val c = currentBrush
        if (c != null && c.set == b.set) return c.index == b.index
        return setChoice[b.set.toInt()] == b.index.toInt()
    }

    // the brush studio

    fun studio(key: String): Float = studioValues[key] ?: 0f

    /** Opens the studio on the current tool's brush. */
    fun openStudio() {
        val s = session ?: return
        // the brush library stays open under it, so Cancel and Done go back to it
        if (panel != Panel.BRUSHES) closePanels()
        studioName = s.studioBegin()
        readStudio()
        studioOpen = true
    }

    private fun readStudio() {
        val s = session ?: return
        val keys = io.github.artcraftandroid.procraft.ui.STUDIO_KEYS
        s.studioSettings(keys).forEachIndexed { i, v -> studioValues[keys[i]] = v }
        studioCurve = s.studioCurve().chunked(2).filter { it.size == 2 }.map { Offset(it[0], it[1]) }
        readStudioSources()
        readStudioAbout()
        refreshStudioPreview()
    }

    /** Who made the studio's brush, and when its reset point was made: its About page. */
    class BrushAbout(val author: String, val picture: ImageBitmap?, val signature: ImageBitmap?, val resetMade: Long?)

    var studioAbout by mutableStateOf<BrushAbout?>(null)
        private set

    private fun readStudioAbout() {
        val a = session?.studioAbout() ?: return
        val picture = { png: ByteArray? -> png?.let { android.graphics.BitmapFactory.decodeByteArray(it, 0, it.size)?.asImageBitmap() } }
        studioAbout = BrushAbout(a.author, picture(a.picture), picture(a.signature), a.resetMade)
    }

    /** Sets the studio's brush aside as it is now, for resetting it to go back to. */
    fun newResetPoint() {
        session?.studioNewResetPoint()
        readStudioAbout()
    }

    fun setStudio(key: String, value: Float) {
        studioValues[key] = value
        session?.setStudioSetting(key, value)
        // the spacing sets how big the brush may be, so where the size controls sit
        if (key == "spacing") session?.let { s ->
            val keys = listOf("max_size", "min_size", "most_size")
            s.studioSettings(keys).forEachIndexed { i, v -> studioValues[keys[i]] = v }
        }
        refreshStudioPreview()
        syncPad()
    }

    // the studio brush's shape and grain images

    /** The images the studio brush's shape and grain use (white on black), for their rows and editors. */
    var studioShapeSource by mutableStateOf<ImageBitmap?>(null)
        private set
    var studioGrainSource by mutableStateOf<ImageBitmap?>(null)
        private set

    /** The shape (false) or grain (true) editor is open. */
    var sourceEditing by mutableStateOf<Boolean?>(null)

    /** The shipped shape and grain images, by name, for the source library. */
    val shapeLibrary by lazy { io.github.artcraftandroid.procraft.core.sourceLibrary(false).map { it.name to grayBitmap(it.pixels) } }
    val grainLibrary by lazy { io.github.artcraftandroid.procraft.core.sourceLibrary(true).map { it.name to grayBitmap(it.pixels) } }

    private fun readStudioSources() {
        val s = session ?: return
        val shape = sourceBitmap(s.studioSource(false), shape = true)
        val grain = sourceBitmap(s.studioSource(true), shape = false)
        studioShapeSource = shape?.let { small(it, SOURCE_ROW_PX) }
        studioGrainSource = grain?.let { small(it, SOURCE_ROW_PX) }
        studioShapeSourceBig = shape?.let { small(it, SOURCE_BOX_PX) }
        studioGrainSourceBig = grain?.let { small(it, SOURCE_BOX_PX) }
    }

    /** The same images as big as the landscape studio's square shows them. */
    var studioShapeSourceBig by mutableStateOf<ImageBitmap?>(null)
        private set
    var studioGrainSourceBig by mutableStateOf<ImageBitmap?>(null)
        private set

    /** [image] shrunk by halves to about [side] pixels across, each step averaging, so fine detail evens out. */
    private fun small(image: ImageBitmap, side: Int): ImageBitmap {
        var b = image.asAndroidBitmap()
        while (b.width >= side * 2) b = Bitmap.createScaledBitmap(b, b.width / 2, b.height / 2, true)
        return b.asImageBitmap()
    }

    /** A square greyscale image (white = paint) as a bitmap, white on black; none if it is empty. */
    private fun grayBitmap(pixels: ByteArray): ImageBitmap? {
        val n = kotlin.math.sqrt(pixels.size.toDouble()).toInt()
        if (n <= 0 || n * n != pixels.size) return null
        val argb = IntArray(n * n) { i -> (pixels[i].toInt() and 0xFF).let { v -> (0xFF shl 24) or (v shl 16) or (v shl 8) or v } }
        return Bitmap.createBitmap(argb, n, n, Bitmap.Config.ARGB_8888).asImageBitmap()
    }

    /** The image the studio brush's shape (or [grain]) uses now: square greyscale, white = paint; empty for the round tip. */
    fun studioSourcePixels(grain: Boolean): ByteArray = session?.studioSource(grain) ?: ByteArray(0)

    /** [pixels] made to repeat without seams (the grain editor's Auto repeat). */
    fun repeatSource(pixels: ByteArray, scale: Float, rotation: Float, overlap: Float, hardness: Float, mirror: Boolean, pyramid: Boolean): ByteArray =
        io.github.artcraftandroid.procraft.core.repeatableSource(pixels, scale, rotation, overlap, hardness, mirror, pyramid)

    /** A square greyscale image as the editors show it: a [shape] white over what is behind, a grain as its greys. */
    fun sourceBitmap(pixels: ByteArray, shape: Boolean): ImageBitmap? {
        val n = kotlin.math.sqrt(pixels.size.toDouble()).toInt()
        if (n <= 0 || n * n != pixels.size) return null
        val argb = IntArray(n * n) { i ->
            val v = pixels[i].toInt() and 0xFF
            if (shape) (v shl 24) or 0xFFFFFF else (0xFF shl 24) or (v shl 16) or (v shl 8) or v
        }
        return Bitmap.createBitmap(argb, n, n, Bitmap.Config.ARGB_8888).asImageBitmap()
    }

    /** Shipped image [index] of the source library. */
    fun librarySource(grain: Boolean, index: Int): ByteArray =
        io.github.artcraftandroid.procraft.core.sourceLibrary(grain).getOrNull(index)?.pixels ?: ByteArray(0)

    /** [bitmap] fitted into the square on black, by its lightness. */
    private fun sourceFrom(bitmap: Bitmap): ByteArray {
        val n = io.github.artcraftandroid.procraft.core.sourceSize().toInt()
        val square = Bitmap.createBitmap(n, n, Bitmap.Config.ARGB_8888)
        val canvas = android.graphics.Canvas(square)
        canvas.drawColor(android.graphics.Color.BLACK)
        val scale = minOf(n / bitmap.width.toFloat(), n / bitmap.height.toFloat())
        val (w, h) = bitmap.width * scale to bitmap.height * scale
        canvas.drawBitmap(bitmap, null, android.graphics.RectF((n - w) / 2f, (n - h) / 2f, (n + w) / 2f, (n + h) / 2f), android.graphics.Paint(android.graphics.Paint.FILTER_BITMAP_FLAG))
        val argb = IntArray(n * n)
        square.getPixels(argb, 0, n, 0, 0, n, n)
        return ByteArray(n * n) { i ->
            val c = argb[i]
            ((((c shr 16) and 255) * 54 + ((c shr 8) and 255) * 183 + (c and 255) * 19) shr 8).toByte()
        }
    }

    /** A picture from elsewhere as a source image; none if it can't be read. */
    suspend fun decodeSource(uri: android.net.Uri): ByteArray? {
        val bitmap = withContext(Dispatchers.IO) {
            runCatching {
                val resolver = getApplication<Application>().contentResolver
                val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
                var sample = 1
                while (maxOf(bounds.outWidth, bounds.outHeight) / sample > 2048) sample *= 2
                resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, BitmapFactory.Options().apply { inSampleSize = sample }) }
            }.getOrNull()
        }
        if (bitmap == null) {
            message = "This picture can't be opened."
            return null
        }
        return sourceFrom(bitmap)
    }

    /** A picture on the clipboard as a source image; none if there isn't one. */
    suspend fun pastedSource(): ByteArray? {
        val uri = systemClipboard?.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.uri
        if (uri == null) {
            message = "There's no picture to paste."
            return null
        }
        return decodeSource(uri)
    }

    /** The editor's tick: the studio brush's shape (or [grain]) becomes [pixels], or shipped image [shipped]. */
    fun keepStudioSource(grain: Boolean, pixels: ByteArray, shipped: Int?) {
        val s = session ?: return
        val changed = when {
            shipped != null -> s.setStudioShippedSource(grain, shipped.toUInt())
            pixels.isNotEmpty() -> s.setStudioSource(grain, pixels)
            else -> false
        }
        sourceEditing = null
        if (changed) sourceChanged()
    }

    private fun sourceChanged() {
        readStudioSources()
        refreshStudioPreview()
        syncPad()
    }

    /** The drawing pad's canvas is made: dark, painting with the brush being edited, keeping its strokes. */
    fun startPad(p: CanvasSession) {
        pad = p
        p.setBackgroundColor(PAD_BACK, PAD_BACK, PAD_BACK, true)
        p.keepStrokes()
        syncPad()
    }

    /** The pad has drawn its first frame: it shows the brush's sample scribble. */
    fun padReady(p: CanvasSession) {
        if (pad === p) p.drawSampleScribble()
    }

    /** The pad paints with the brush as it is now, and what is on it is drawn again that way. */
    private fun syncPad() {
        val p = pad ?: return
        val s = session ?: return
        val c = PAD_COLORS.getOrElse(padColor) { 0xFFFFFF }
        p.paintLike(s, ((c shr 16) and 0xFF) / 255f, ((c shr 8) and 0xFF) / 255f, (c and 0xFF) / 255f)
        p.replayStrokes()
    }

    fun choosePadColor(i: Int) {
        padColor = i
        syncPad()
    }

    fun clearPad() {
        pad?.forgetStrokes()
        pad?.clearLayer()
        padMenuOpen = false
    }

    fun changeStudioCurve(points: List<Offset>) {
        studioCurve = points
        session?.setStudioCurve(points.flatMap { listOf(it.x, it.y) })
        refreshStudioPreview()
        syncPad()
    }

    /** Every control back to how the brush ships. */
    fun resetStudio() {
        session?.studioReset()
        readStudio()
        syncPad()
        padMenuOpen = false
    }

    /** Closes the studio: [keep] keeps the edits, otherwise the brush goes back to how it opened. */
    fun closeStudio(keep: Boolean) {
        session?.studioEnd(keep)
        // a new brush cancelled is thrown away
        if (!keep && studioNew) currentBrush?.let { session?.deleteBrush(it.set, it.index) }
        studioNew = false
        studioOpen = false
        padShown = false
        padMenuOpen = false
        // the pad's canvas goes with the studio
        val p = pad
        pad = null
        p?.detachWindow()
        p?.destroy()
        studioPreview = null
        libraryChanged()
        readSliders()
    }

    /** Draws the brush's sample stroke again, a moment after the last change. */
    private fun refreshStudioPreview() {
        val s = session ?: return
        val b = currentBrush ?: return
        studioPreviewJob?.cancel()
        studioPreviewJob = viewModelScope.launch {
            kotlinx.coroutines.delay(STUDIO_PREVIEW_DELAY_MS)
            val image = withContext(Dispatchers.Default) {
                val t = s.brushPreview(b.set, b.index, 1060u, 324u) ?: return@withContext null
                val bitmap = io.github.artcraftandroid.procraft.ui.CanvasSpace.tag(Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), Bitmap.Config.ARGB_8888))
                bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(t.rgba))
                bitmap.asImageBitmap()
            }
            if (image != null && studioOpen) studioPreview = image
        }
    }

    /** Renders the brush's sample stroke once, off the main thread. */
    fun loadPreview(b: BrushInfo) {
        val key = "${b.set}-${b.index}"
        if (previews.containsKey(key)) return
        val s = session ?: return
        viewModelScope.launch {
            val made = withContext(Dispatchers.Default) {
                // half the sample stroke's own canvas: the cell shows it across its width
                val t = s.brushPreview(b.set, b.index, 530u, 162u) ?: return@withContext null
                // whether the stroke runs under the name: the name's box (19.5 in, 14 to 26 down, about
                // 7.7 a letter wide, in a cell the picture fills 80 high and 261.7 wide) covered more
                // than 2/255 on average
                val k = 162f / 80f
                val x0 = ((19.5f + 6.85f) * k).toInt()
                val x1 = ((19.5f + 6.85f + 7.7f * b.name.length) * k).toInt().coerceAtMost(530)
                val y0 = (14f * k).toInt()
                val y1 = (26f * k).toInt()
                var cover = 0L
                for (y in y0 until y1) for (x in x0 until x1) cover += t.rgba[(y * 530 + x) * 4 + 3].toInt() and 0xFF
                val under = x1 > x0 && cover > 2L * (x1 - x0) * (y1 - y0)
                val bitmap = io.github.artcraftandroid.procraft.ui.CanvasSpace.tag(Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), Bitmap.Config.ARGB_8888))
                bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(t.rgba))
                bitmap.asImageBitmap() to under
            }
            if (made != null) {
                shadedNames[key] = made.second
                previews[key] = made.first
            }
        }
    }

    fun changeSize(value: Float) {
        size = value
        session?.setBrushSize(value)
    }

    /** The current brush's saved sizes and opacities: the side sliders' notches (0..1). */
    var sizeNotches by mutableStateOf<List<Float>>(emptyList())
        private set
    var opacityNotches by mutableStateOf<List<Float>>(emptyList())
        private set

    /** The notch [value] is on, of the size or opacity slider's ([size]), if any. */
    fun notchAt(size: Boolean, value: Float): Float? =
        (if (size) sizeNotches else opacityNotches)
            .filter { kotlin.math.abs(it - value) < NOTCH_NEAR }
            .minByOrNull { kotlin.math.abs(it - value) }

    /** Whether the slider's panel offers to save its value: not when it is on a notch or the slider is full. */
    fun canSaveNotch(size: Boolean): Boolean =
        notchAt(size, if (size) this.size else opacity) == null && (if (size) sizeNotches else opacityNotches).size < NOTCH_MOST

    /** The slider panel's button: saves the value as a notch, or takes away the notch it is on. */
    fun toggleNotch(size: Boolean) {
        val s = session ?: return
        val value = if (size) this.size else opacity
        if (notchAt(size, value) != null) s.removeSliderNotch(size, value) else s.addSliderNotch(size, value)
        readNotches()
        brushPrefs.edit().putString(BRUSH_LIBRARY, s.savedBrushLibrary()).apply()
    }

    private fun readNotches() {
        val s = session ?: return
        sizeNotches = s.sliderNotches(true)
        opacityNotches = s.sliderNotches(false)
    }

    /** Which slider's panel shows beside it: the size's or the opacity's. */
    enum class SliderPanel { SIZE, OPACITY }

    /** The panel beside a slider: shown while it is dragged, and kept up after it is held still. */
    var sliderPanel by mutableStateOf<SliderPanel?>(null)

    /** The panel stays up (the slider was held still) until the next touch elsewhere. */
    var sliderPanelKept by mutableStateOf(false)

    /** The brush's diameter on the screen now, window pixels. */
    fun brushScreenDiameter(): Float = session?.info()?.let { it.brushDiameter * it.zoom } ?: 0f

    fun changeOpacity(value: Float) {
        opacity = value
        session?.setBrushOpacity(value)
    }

    fun setColor(h: Float, s: Float, v: Float) {
        hue = h.coerceIn(0f, 1f)
        saturation = s.coerceIn(0f, 1f)
        brightness = v.coerceIn(0f, 1f)
        applyColor()
    }

    /** The colour as 0xRRGGBB. */
    fun colorRgb(): Int = hsvToRgb(hue, saturation, brightness)

    fun setRgb(rgb: Int) {
        val hsv = FloatArray(3)
        android.graphics.Color.RGBToHSV((rgb shr 16) and 0xFF, (rgb shr 8) and 0xFF, rgb and 0xFF, hsv)
        // a grey keeps the hue it had, so the hue slider does not jump
        setColor(if (hsv[1] > 0f) hsv[0] / 360f else hue, hsv[1], hsv[2])
    }

    /**
     * Moves the eyedropper to window point ([x], [y]) and reads what is shown there. The reading
     * happens off the main thread; points that arrive meanwhile replace each other.
     */
    fun pickAt(x: Float, y: Float, side: Int) {
        pendingPick = x to y
        if (picking == null) picking = PickState(x, y, null, null)
        if (pickJob?.isActive == true) return
        pickJob = viewModelScope.launch {
            while (true) {
                val (px, py) = pendingPick ?: break
                pendingPick = null
                val s = session ?: break
                val seen = withContext(Dispatchers.Default) {
                    s.pick(px, py, side.toUInt())?.let { p ->
                        val n = p.side.toInt()
                        val bytes = p.pixels
                        val argb = IntArray(n * n) { i ->
                            (0xFF shl 24) or ((bytes[i * 4].toInt() and 0xFF) shl 16) or ((bytes[i * 4 + 1].toInt() and 0xFF) shl 8) or (bytes[i * 4 + 2].toInt() and 0xFF)
                        }
                        Bitmap.createBitmap(argb, n, n, Bitmap.Config.ARGB_8888).asImageBitmap() to p.rgb?.toInt()
                    }
                }
                val now = picking ?: break
                picking = PickState(px, py, seen?.first ?: now.image, seen?.second ?: now.rgb)
            }
        }
    }

    /** The eyedropper lets go: its colour becomes the current one. */
    fun finishPick() {
        pickNext = false
        // a quick tap ends before the colour under it has been read: it is taken once it has
        val job = pickJob
        if (job?.isActive == true) {
            viewModelScope.launch {
                job.join()
                endPick()
            }
        } else {
            endPick()
        }
    }

    private fun endPick() {
        pendingPick = null
        picking?.rgb?.let { setRgb(it) }
        picking = null
    }

    fun cancelPick() {
        pendingPick = null
        picking = null
    }

    /** The colour button held: back to the colour painted with before this one, if there was one. */
    fun previousColor() {
        val now = colorRgb()
        val before = history.firstOrNull { it != now } ?: return
        setRgb(before)
        showNotice("Previous colour")
    }

    /** The current and second colours trade places. */
    fun swapColors() {
        val current = colorRgb()
        setRgb(secondaryRgb)
        secondaryRgb = current
    }

    fun clearHistory() {
        history = emptyList()
    }

    /**
     * The flattened picture as a PNG (or a JPEG on white) in the cache's exports folder, for
     * sharing; null when it could not be made.
     */
    /** What Share can write besides JPEG and PNG. */
    enum class Export(val extension: String, val mime: String) {
        PSD("psd", "image/vnd.adobe.photoshop"),
        PDF("pdf", "application/pdf"),
        TIFF("tif", "image/tiff"),
        LAYERS_PDF("pdf", "application/pdf"),
        LAYER_PNGS("png", "image/png"),
    }

    /** The artwork written as [kind] into the cache, to share: one file, or a PNG per layer. */
    suspend fun export(kind: Export, pdfQuality: Int = 100): List<File> {
        val s = session ?: return emptyList()
        return withContext(Dispatchers.IO) {
            val dir = File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }
            val base = s.name().filter { it.isLetterOrDigit() || it in " _-" }.trim().ifEmpty { "Untitled Artwork" }
            val file = File(dir, "$base.${kind.extension}")
            try {
                when (kind) {
                    Export.PSD -> s.exportPsd(file.path)
                    Export.PDF -> s.exportPdf(file.path, false, pdfQuality.toUByte())
                    Export.TIFF -> s.exportTiff(file.path)
                    Export.LAYERS_PDF -> s.exportPdf(file.path, true, pdfQuality.toUByte())
                    Export.LAYER_PNGS -> {
                        val folder = File(dir, base).apply { deleteRecursively(); mkdirs() }
                        return@withContext s.exportLayerPngs(folder.path).map { File(it) }
                    }
                }
                listOf(file)
            } catch (e: MobileException) {
                Log.e(TAG, "exporting", e)
                emptyList()
            }
        }
    }

    /** The layers as an animation, a frame each: the formats, by name, extension and type. */
    enum class Animated(val title: String, val extension: String, val mime: String) {
        GIF("Animated GIF", "gif", "image/gif"),
        PNG("Animated PNG", "png", "image/png"),
        MP4("Animated MP4", "mp4", "video/mp4"),
        HEVC("Animated HEVC", "mp4", "video/mp4"),
    }

    /** The animated export being set up, if any. */
    var animatedExport by mutableStateOf<Animated?>(null)

    /** The artwork was opened from the gallery's Share only for [animatedExport]: closing it goes back. */
    private var exportingFromGallery = false

    /** Ends the animated export; an artwork opened for it from the gallery is closed again. */
    fun endAnimatedExport() {
        animatedExport = null
        if (exportingFromGallery) {
            exportingFromGallery = false
            closeArtwork()
        }
    }

    fun animationFrameCount(): Int = session?.animationFrameCount()?.toInt() ?: 0

    /** Frame [index] fitted to [side] pixels, without the background if [transparent]. */
    fun animationFrame(index: Int, side: Int, transparent: Boolean): Bitmap? {
        val t = session?.animationFrame(index.toUInt(), transparent, side.coerceAtLeast(1).toUInt()) ?: return null
        val w = t.width.toInt()
        val h = t.height.toInt()
        val argb = IntArray(w * h) { i ->
            val o = i * 4
            ((t.rgba[o + 3].toInt() and 255) shl 24) or ((t.rgba[o].toInt() and 255) shl 16) or ((t.rgba[o + 1].toInt() and 255) shl 8) or (t.rgba[o + 2].toInt() and 255)
        }
        return Bitmap.createBitmap(argb, w, h, Bitmap.Config.ARGB_8888)
    }

    /**
     * The animation written as [format] into the cache, to share: Web Ready fits 480 pixels, else
     * the canvas size (a video as large as the encoder takes, up to 4096).
     */
    suspend fun exportAnimation(format: Animated, webReady: Boolean, dither: Boolean, transparent: Boolean): File? {
        val s = session ?: return null
        return withContext(Dispatchers.IO) {
            val dir = File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }
            val base = s.name().filter { it.isLetterOrDigit() || it in " _-" }.trim().ifEmpty { "Untitled Artwork" }
            val file = File(dir, "$base.${format.extension}")
            val side = if (webReady) WEB_READY_SIDE else 0
            // Animation Assist's own frame rate, when it is on
            val fps = s.animationSettings()?.fps?.toInt() ?: ANIMATION_FPS
            try {
                when (format) {
                    Animated.GIF -> s.exportGif(file.path, side.toUInt(), fps.toFloat(), dither, transparent)
                    Animated.PNG -> s.exportApng(file.path, side.toUInt(), fps.toFloat(), transparent)
                    Animated.MP4, Animated.HEVC -> {
                        val mime = if (format == Animated.HEVC) android.media.MediaFormat.MIMETYPE_VIDEO_HEVC else android.media.MediaFormat.MIMETYPE_VIDEO_AVC
                        val (cw, ch) = canvasSize()
                        val (w, h) = VideoWriter.fit(cw, ch, if (webReady) WEB_READY_SIDE else MAX_VIDEO_SIDE, mime) ?: return@withContext null
                        val frame = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
                        val paint = android.graphics.Paint(android.graphics.Paint.FILTER_BITMAP_FLAG)
                        val ok = try {
                            val order = s.animationSequence()
                            VideoWriter.write(file, w, h, fps, order.size, mime) { i, pixels ->
                                val picture = animationFrame(order[i].toInt(), maxOf(w, h), false) ?: return@write false
                                android.graphics.Canvas(frame).drawBitmap(picture, null, android.graphics.Rect(0, 0, w, h), paint)
                                picture.recycle()
                                frame.getPixels(pixels, 0, w, 0, 0, w, h)
                                true
                            }
                        } finally {
                            frame.recycle()
                        }
                        if (!ok) return@withContext null
                    }
                }
                file
            } catch (e: MobileException) {
                Log.e(TAG, "exporting an animation", e)
                null
            }
        }
    }

    suspend fun exportImage(jpeg: Boolean): File? {
        val s = session ?: return null
        return withContext(Dispatchers.IO) {
            val dir = File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }
            val base = s.name().filter { it.isLetterOrDigit() || it in " _-" }.trim().ifEmpty { "Untitled Artwork" }
            val png = File(dir, "$base.png")
            try {
                s.exportPng(png.path)
            } catch (e: MobileException) {
                Log.e(TAG, "exporting", e)
                return@withContext null
            }
            if (!jpeg) return@withContext png
            val jpg = File(dir, "$base.jpg")
            val ok = io.github.artcraftandroid.procraft.gallery.pngToJpeg(png, jpg)
            png.delete()
            jpg.takeIf { ok }
        }
    }

    // ---- palettes ----

    fun addPalette() = changePalettes(palettes + Palette("Untitled Palette", List(PALETTE_SLOTS) { null }))

    /** Our example palettes back, those missing added at the end. */
    fun restoreExamplePalettes() {
        val missing = io.github.artcraftandroid.procraft.gallery.PaletteStore.starters().filter { s -> palettes.none { it.name == s.name } }
        if (missing.isNotEmpty()) changePalettes(palettes + missing)
    }

    /** A palette of the main colours in the picture at [uri], light to dark, added at the end. */
    fun paletteFromImage(uri: android.net.Uri) {
        val resolver = getApplication<Application>().contentResolver
        viewModelScope.launch {
            val colours = withContext(Dispatchers.IO) {
                runCatching {
                    val bounds = android.graphics.BitmapFactory.Options().apply { inJustDecodeBounds = true }
                    resolver.openInputStream(uri)?.use { android.graphics.BitmapFactory.decodeStream(it, null, bounds) }
                    // read small: a few hundred pixels across is plenty to find its colours
                    var sample = 1
                    while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= 256) sample *= 2
                    val bitmap = resolver.openInputStream(uri)?.use {
                        android.graphics.BitmapFactory.decodeStream(it, null, android.graphics.BitmapFactory.Options().apply { inSampleSize = sample })
                    } ?: return@runCatching null
                    val pixels = IntArray(bitmap.width * bitmap.height).also { bitmap.getPixels(it, 0, bitmap.width, 0, 0, bitmap.width, bitmap.height) }
                    mainColours(pixels.filter { (it ushr 24) > 128 }.map { it and 0xFFFFFF }, PALETTE_SLOTS)
                }.getOrNull()
            }
            if (colours.isNullOrEmpty()) {
                message = "This picture can't be read."
                return@launch
            }
            changePalettes(palettes + Palette("Palette from image", List(PALETTE_SLOTS) { colours.getOrNull(it) }))
        }
    }

    /**
     * Reads a palette file at [uri] (ours, or another program's colour swatch or swatch exchange
     * file) into a new palette at the end; false if it isn't one.
     */
    suspend fun importSwatches(uri: android.net.Uri): Boolean {
        val resolver = getApplication<Application>().contentResolver
        val p = withContext(Dispatchers.IO) {
            runCatching {
                val head = resolver.openInputStream(uri)?.use { s -> ByteArray(4).also { s.read(it) } } ?: return@runCatching null
                val name = fileName(uri)
                when {
                    head[0] == 'P'.code.toByte() && head[1] == 'K'.code.toByte() ->
                        resolver.openInputStream(uri)?.use { io.github.artcraftandroid.procraft.gallery.Swatches.read(it) }
                    io.github.artcraftandroid.procraft.gallery.ColourSwatches.isExchange(head) || name?.endsWith(".aco", ignoreCase = true) == true ->
                        resolver.openInputStream(uri)?.use { io.github.artcraftandroid.procraft.gallery.ColourSwatches.read(it.readBytes(), name) }
                    else -> null
                }
            }.getOrNull()
        } ?: return false
        changePalettes(palettes + p)
        return true
    }

    /** The name of the file at [uri], if its provider says. */
    private fun fileName(uri: android.net.Uri): String? = runCatching {
        getApplication<Application>().contentResolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) else null
        }
    }.getOrNull()

    /** Palette [i] as a swatches file in the cache, to share. */
    suspend fun paletteFile(i: Int): File? {
        val p = palettes.getOrNull(i) ?: return null
        return withContext(Dispatchers.IO) {
            runCatching {
                val base = p.name.filter { it.isLetterOrDigit() || it in " _-" }.trim().ifEmpty { "Palette" }
                File(File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }, "$base.${io.github.artcraftandroid.procraft.gallery.Swatches.EXTENSION}")
                    .also { io.github.artcraftandroid.procraft.gallery.Swatches.write(p, it) }
            }.onFailure { Log.e(TAG, "sharing a palette", it) }.getOrNull()
        }
    }

    fun renamePalette(i: Int, name: String) = editPalette(i) { it.copy(name = name.trim().ifEmpty { it.name }) }

    fun duplicatePalette(i: Int) {
        val p = palettes.getOrNull(i) ?: return
        changePalettes(palettes.take(i + 1) + p.copy(name = p.name + " copy") + palettes.drop(i + 1))
    }

    fun deletePalette(i: Int) {
        if (palettes.size <= 1 || i !in palettes.indices) return
        val d = when {
            defaultPalette == i -> 0
            defaultPalette > i -> defaultPalette - 1
            else -> defaultPalette
        }
        changePalettes(palettes.filterIndexed { j, _ -> j != i }, d)
    }

    fun chooseDefaultPalette(i: Int) {
        if (i in palettes.indices) changePalettes(palettes, i)
    }

    /** Puts [rgb] in a palette's [slot], or empties it when null. */
    fun setSwatch(i: Int, slot: Int, rgb: Int?) = editPalette(i) { p ->
        if (slot !in 0 until PALETTE_SLOTS) p else p.copy(colors = p.colors.toMutableList().also { it[slot] = rgb })
    }

    private fun editPalette(i: Int, f: (Palette) -> Palette) {
        val p = palettes.getOrNull(i) ?: return
        changePalettes(palettes.toMutableList().also { it[i] = f(p) })
    }

    private fun changePalettes(next: List<Palette>, default: Int = defaultPalette) {
        palettes = next
        defaultPalette = default.coerceIn(0, (next.size - 1).coerceAtLeast(0))
        val snapshot = palettes to defaultPalette
        viewModelScope.launch(Dispatchers.IO) { paletteStore.save(snapshot.first, snapshot.second) }
    }

    private fun applyColor() {
        val c = colorRgb()
        session?.setColor(((c shr 16) and 0xFF) / 255f, ((c shr 8) and 0xFF) / 255f, (c and 0xFF) / 255f)
        if (colorNames) nameColorSoon()
    }

    private var naming: kotlinx.coroutines.Job? = null
    private var namedAt = 0L

    /**
     * Names the colour in a message as it changes (Color Description Notifications): at once, or,
     * while it keeps changing, the latest colour at most every [COLOR_NAME_GAP_MS].
     */
    private fun nameColorSoon() {
        naming?.cancel()
        val wait = (namedAt + COLOR_NAME_GAP_MS - android.os.SystemClock.uptimeMillis()).coerceAtLeast(0L)
        naming = viewModelScope.launch {
            kotlinx.coroutines.delay(wait)
            namedAt = android.os.SystemClock.uptimeMillis()
            message = io.github.artcraftandroid.procraft.ui.colourName(colorRgb())
        }
    }

    /** A click for an action done, with Feedback Sounds on. */
    fun feedback() {
        if (!feedbackSounds) return
        getApplication<Application>().getSystemService(android.media.AudioManager::class.java)
            ?.playSoundEffect(android.media.AudioManager.FX_KEY_CLICK)
    }

    fun toggle(p: Panel) {
        feedback()
        endQuickShape()
        if (transforming) endTransform(true)
        endAdjustment(true)
        closeSelecting()
        panel = if (panel == p) Panel.NONE else p
        if (panel == Panel.LAYERS) refreshThumbnails()
    }

    fun closePanels() {
        panel = Panel.NONE
    }

    /** Closes the open panel by fading it away, as picking something that takes the screen does. */
    fun fadePanels() {
        if (panel != Panel.NONE) panelFading = true
        panel = Panel.NONE
    }

    fun undo() {
        feedback()
        // undoing while editing text undoes the whole edit
        if (textInfo != null) {
            textInfo = null
            textStyleOpen = false
        }
        if (transforming) {
            transformBox = session?.transformApply(TransformAction.UNDO) ?: transformBox
            return
        }
        // cloning, the source goes back to where it was before the stroke
        if (adjustment == Adjustment.CLONE) {
            puckBefore.removeLastOrNull()?.let { before -> clonePuck?.let { puckAfter.addLast(it) }; clonePuck = before }
        }
        val could = canUndo
        val name = session?.undo()
        if (could) showNotice("Undo" + name?.let { " $it" }.orEmpty())
        refresh()
    }

    /**
     * A passing message under the top bar (what undo and redo did, and the like); [id] tells
     * repeats apart; a [warning] stays longer.
     */
    data class Notice(val text: String, val id: Long, val warning: Boolean = false)

    var notice by mutableStateOf<Notice?>(null)
        private set

    private var notices = 0L

    /** Shows [text] in the band under the top bar, in place of any message there; a [warning] stays longer. */
    fun showNotice(text: String, warning: Boolean = false) {
        notice = Notice(text, ++notices, warning)
    }

    /** The message under the top bar has gone. */
    fun noticeGone(n: Notice) {
        if (notice == n) notice = null
    }

    fun redo() {
        feedback()
        if (transforming) return
        if (adjustment == Adjustment.CLONE) {
            puckAfter.removeLastOrNull()?.let { after -> clonePuck?.let { puckBefore.addLast(it) }; clonePuck = after }
        }
        val could = canRedo
        val name = session?.redo()
        if (could) showNotice("Redo" + name?.let { " $it" }.orEmpty())
        refresh()
    }

    // ---- adjustments ----

    /** Opens adjustment [a] on the current layer (keeping one already open). */
    fun startAdjustment(a: Adjustment) {
        val s = session ?: return
        if (!currentTakesChanges()) return
        endAdjustment(true)
        endTransform(true)
        closeSelecting()
        // the list fades away rather than going at once
        if (panel == Panel.ADJUSTMENTS) panelFading = true
        panel = Panel.NONE
        val begun = when (a) {
            Adjustment.LIQUIFY -> s.liquifyBegin()
            Adjustment.CLONE -> s.cloneBegin()
            else -> s.adjustBegin()
        }
        if (!begun) return
        adjustment = a
        chromaOffset = Offset.Zero
        adjustPencil = a == Adjustment.CLONE
        adjustModesOpen = false
        cloneOffset = null
        puckBefore.clear()
        puckAfter.clear()
        clonePuck = null
        if (a == Adjustment.CLONE) {
            // the source starts in the middle of the window
            moveClonePuck(windowSize / 2f)
            readSliders()
        }
        liquified = false
        liquifyAmount = 1f
        liquifyPopup = 0
        adjustAmount = if (a == Adjustment.GRADIENT_MAP) 1f else 0f
        hue50 = 0.5f
        saturation50 = 0.5f
        brightness50 = 0.5f
        balance = List(9) { 0.5f }
        curves = List(4) { listOf(Offset(0f, 0f), Offset(1f, 1f)) }
        if (a == Adjustment.GRADIENT_MAP) applyGradient()
        if (a == Adjustment.GLITCH) applyGlitch()
    }

    /** Ends the adjustment: [commit] keeps it as one undo step, otherwise the layer goes back. */
    fun endAdjustment(commit: Boolean) {
        if (adjustment == null) return
        // a gradient being edited is kept as it is
        if (gradientEditing != null) endGradientEdit()
        adjustMenuAt = null
        adjustPreview = false
        session?.adjustEnd(commit)
        if (commit) showNotice("Adjustment applied")
        adjustment = null
        val painted = adjustPencil
        adjustPencil = false
        adjustModesOpen = false
        clonePuck = null
        if (painted) readSliders()
        refresh()
        if (panel == Panel.LAYERS) refreshThumbnails()
    }

    /**
     * Layer or Pencil: painting the adjustment on starts it at its own amount, and only where the
     * brush goes shows it; back to the layer starts it again.
     */
    fun chooseAdjustMode(pencil: Boolean) {
        val a = adjustment ?: return
        adjustModesOpen = false
        if (pencil == adjustPencil || !a.paintable) return
        if (!pencil) {
            resetAdjustment()
            return
        }
        session?.adjustPencil()
        adjustPencil = true
        readSliders()
        val amount = a.pencilAmount ?: return
        if (a == Adjustment.BLOOM) {
            // painted on, bloom starts smaller and burns more
            bloomSize = PENCIL_BLOOM_SIZE
            bloomBurn = PENCIL_BLOOM_BURN
        }
        if (a == Adjustment.MOTION_BLUR) {
            // along the canvas, as long as a drag of that much
            dragMotionBlur(amount * MOTION_DRAG_PT * density, 0f)
        } else {
            changeAdjustAmount(amount - adjustAmount)
        }
    }

    /** The clone source moved to window point [p]: the next stroke fixes how far from it the brush is. */
    fun moveClonePuck(p: Offset) {
        val at = windowToCanvas(p) ?: return
        clonePuck = at
        cloneOffset = null
        session?.cloneSource(p.x, p.y)
    }

    /** A clone stroke reached window point [p] ([start]: it begins there); the source follows the brush. */
    fun cloneStrokeAt(p: Offset, start: Boolean) {
        val puck = clonePuck ?: return
        val at = windowToCanvas(p) ?: return
        if (start) {
            puckBefore.addLast(puck)
            puckAfter.clear()
        }
        val offset = cloneOffset ?: (puck - at).also { cloneOffset = it }
        clonePuck = at + offset
    }

    /** Whether window point [p] is on the clone source. */
    fun onClonePuck(p: Offset): Boolean {
        val puck = clonePuck ?: return false
        return (canvasToWindow(puck) - p).getDistance() <= (CLONE_PUCK_RADIUS_DP + CLONE_PUCK_SLOP_DP) * density
    }

    /** The amount dragged across the canvas changed by [step] (0..1 of the whole). */
    fun changeAdjustAmount(step: Float) {
        val a = adjustment ?: return
        adjustAmount = (adjustAmount + step).coerceIn(0f, 1f)
        val kind = when (a) {
            Adjustment.GAUSSIAN_BLUR -> SwipeAdjustment.GAUSSIAN_BLUR
            Adjustment.SHARPEN -> SwipeAdjustment.SHARPEN
            Adjustment.PERSPECTIVE_BLUR -> SwipeAdjustment.PERSPECTIVE_BLUR
            Adjustment.GRADIENT_MAP -> {
                applyGradient()
                return
            }
            Adjustment.GLITCH -> {
                applyGlitch()
                return
            }
            Adjustment.NOISE -> {
                applyNoise()
                return
            }
            Adjustment.BLOOM -> {
                applyBloom()
                return
            }
            Adjustment.HALFTONE -> {
                applyHalftone()
                return
            }
            Adjustment.CHROMATIC_ABERRATION -> {
                applyChroma()
                return
            }
            else -> return
        }
        session?.adjustSwipe(kind, adjustAmount)
    }

    /** A motion blur's drag reached ([dx], [dy]) window pixels from where it started. */
    fun dragMotionBlur(dx: Float, dy: Float) {
        adjustAmount = session?.adjustMotion(dx, dy) ?: adjustAmount
    }

    fun chooseBalanceTone(tone: Int) {
        balanceTone = tone.coerceIn(0, 2)
    }

    /** Slider [k] (red, green, blue) of the tone shown moved to [value] (0..1). */
    fun setBalance(k: Int, value: Float) {
        balance = balance.toMutableList().also { it[balanceTone * 3 + k] = value }
        val v = balance.map { (it - 0.5f) * 2f }
        session?.adjustColorBalance(v.subList(0, 3), v.subList(3, 6), v.subList(6, 9))
    }

    fun chooseCurveChannel(channel: Int) {
        curveChannel = channel.coerceIn(0, 3)
    }

    /** Adds a point to the channel shown; returns its index. */
    fun addCurvePoint(p: Offset): Int {
        val pts = (curves[curveChannel] + p).sortedBy { it.x }
        setCurve(pts)
        return pts.indexOf(p)
    }

    /** Moves point [index] of the channel shown to [p], kept between its neighbours; returns its index. */
    fun moveCurvePoint(index: Int, p: Offset): Int {
        val pts = curves[curveChannel].toMutableList()
        if (index !in pts.indices) return index
        val lo = pts.getOrNull(index - 1)?.x?.plus(0.01f) ?: 0f
        val hi = pts.getOrNull(index + 1)?.x?.minus(0.01f) ?: 1f
        pts[index] = Offset(p.x.coerceIn(lo, maxOf(lo, hi)), p.y)
        setCurve(pts)
        return index
    }

    private fun setCurve(points: List<Offset>) {
        curves = curves.toMutableList().also { it[curveChannel] = points }
        val flat = curves.map { c -> c.flatMap { listOf(it.x, it.y) } }
        session?.adjustCurves(flat[0], flat[1], flat[2], flat[3])
    }

    /** The curve of [channel] as points to draw (x, height). */
    fun curveShape(channel: Int): List<Offset> {
        val heights = session?.curveShape(curves[channel].flatMap { listOf(it.x, it.y) }) ?: return emptyList()
        return heights.mapIndexed { i, h -> Offset(i / 255f, h) }
    }

    fun chooseGradient(i: Int) {
        gradientIndex = i.coerceIn(0, gradients.size - 1)
        applyGradient()
    }

    /** A new gradient, black to white, just after the chosen one: chosen and opened to edit. */
    fun newGradient() {
        val at = (gradientIndex + 1).coerceIn(0, gradients.size)
        gradients = gradients.toMutableList().apply { add(at, GradientPreset("Gradient", listOf(0f to 0xFF000000, 1f to 0xFFFFFFFF))) }
        gradientIndex = at
        gradientEditing = at
        gradientStore.save(gradients)
        applyGradient()
    }

    /** Opens gradient [i] (chosen) to edit. */
    fun editGradient(i: Int) {
        chooseGradient(i)
        gradientEditing = gradientIndex
    }

    /** Done editing: the library is saved. */
    fun endGradientEdit() {
        gradientEditing = null
        gradientStore.save(gradients)
    }

    fun deleteGradient(i: Int) {
        if (gradients.size <= 1 || i !in gradients.indices) return
        gradients = gradients.toMutableList().apply { removeAt(i) }
        if (gradientIndex > i || gradientIndex >= gradients.size) gradientIndex = (gradientIndex - 1).coerceAtLeast(0)
        gradientStore.save(gradients)
        applyGradient()
    }

    /** A copy of gradient [i] just after it, chosen. */
    fun duplicateGradient(i: Int) {
        val g = gradients.getOrNull(i) ?: return
        gradients = gradients.toMutableList().apply { add(i + 1, g) }
        gradientIndex = i + 1
        gradientStore.save(gradients)
        applyGradient()
    }

    private fun changeEdited(f: (GradientPreset) -> GradientPreset) {
        val i = gradientEditing ?: return
        val g = gradients.getOrNull(i) ?: return
        gradients = gradients.toMutableList().apply { set(i, f(g)) }
        applyGradient()
    }

    fun renameGradient(name: String) = changeEdited { if (name.isBlank()) it else it.copy(name = name.trim()) }

    /** A stop at [position] (0..1), coloured as the gradient was there; returns its index. */
    fun addGradientStop(position: Float): Int {
        val g = gradientEditing?.let { gradients.getOrNull(it) } ?: return -1
        val p = position.coerceIn(0f, 1f)
        changeEdited { it.copy(stops = it.stops + (p to gradientColorAt(it, p))) }
        return g.stops.size
    }

    fun moveGradientStop(i: Int, position: Float) = changeEdited { g ->
        g.copy(stops = g.stops.mapIndexed { k, s -> if (k == i) position.coerceIn(0f, 1f) to s.second else s })
    }

    fun setGradientStopColor(i: Int, rgb: Int) = changeEdited { g ->
        g.copy(stops = g.stops.mapIndexed { k, s -> if (k == i) s.first to (0xFF000000 or (rgb.toLong() and 0xFFFFFF)) else s })
    }

    /** Takes a stop out; a gradient keeps two. */
    fun deleteGradientStop(i: Int) = changeEdited { g -> if (g.stops.size <= 2) g else g.copy(stops = g.stops.filterIndexed { k, _ -> k != i }) }

    // noise, bloom, halftone and chromatic aberration's toolbars

    fun chooseNoiseKind(kind: NoiseKind) {
        noiseKind = kind
        applyNoise()
    }

    /** Noise's slider [index] (scale, octaves, turbulence) moved to [value]. */
    fun setNoiseValue(index: Int, value: Float) {
        when (index) {
            0 -> noiseScale = value
            1 -> noiseOctaves = value
            else -> noiseTurbulence = value
        }
        applyNoise()
    }

    fun chooseNoiseMulti(multi: Boolean) {
        noiseMulti = multi
        applyNoise()
    }

    fun toggleNoiseAdded() {
        noiseAdded = !noiseAdded
        applyNoise()
    }

    private fun applyNoise() {
        session?.adjustNoise(noiseKind, adjustAmount, noiseScale, noiseOctaves, noiseTurbulence, noiseMulti, noiseAdded)
    }

    /** Bloom's slider [index] (transition, size, burn) moved to [value]. */
    fun setBloomValue(index: Int, value: Float) {
        when (index) {
            0 -> bloomTransition = value
            1 -> bloomSize = value
            else -> bloomBurn = value
        }
        applyBloom()
    }

    private fun applyBloom() {
        session?.adjustBloom(adjustAmount, bloomTransition, bloomSize, bloomBurn)
    }

    fun chooseHalftoneKind(kind: HalftoneKind) {
        halftoneKind = kind
        applyHalftone()
    }

    private fun applyHalftone() {
        session?.adjustHalftone(halftoneKind, adjustAmount)
    }

    fun chooseChromaKind(kind: ChromaticKind) {
        chromaKind = kind
        applyChroma()
    }

    /** Chromatic aberration's slider [index] of the kind shown moved to [value]. */
    fun setChromaValue(index: Int, value: Float) {
        val values = chromaValues[chromaKind] ?: return
        if (index !in values.indices) return
        chromaValues = chromaValues + (chromaKind to values.toMutableList().also { it[index] = value })
        applyChroma()
    }

    /** Displacing: a drag [d] window pixels from where it began ([start] at its beginning) moves the fringes on. */
    fun dragChromatic(d: Offset, start: Boolean) {
        if (start) chromaDragFrom = chromaOffset
        chromaOffset = chromaDragFrom + d
        // how far, shown as the amount: a quarter of the window's short side is all of it
        val whole = minOf(windowSize.x, windowSize.y) / 4f
        adjustAmount = if (whole > 0f) (chromaOffset.getDistance() / whole).coerceIn(0f, 1f) else 0f
        applyChroma()
    }

    private fun applyChroma() {
        val v = chromaValues[chromaKind] ?: return
        val offset = if (chromaKind == ChromaticKind.DISPLACE) chromaOffset else Offset.Zero
        session?.adjustChromatic(chromaKind, adjustAmount, offset.x, offset.y, v[0], v[1])
    }

    fun chooseGlitchKind(kind: GlitchKind) {
        glitchKind = kind
        applyGlitch()
    }

    /** Slider [index] of the glitch kind shown moved to [value] (0..1, or −1..1 for diverge's shifts). */
    fun setGlitchValue(index: Int, value: Float) {
        val values = glitchValues[glitchKind] ?: return
        if (index !in values.indices) return
        glitchValues = glitchValues + (glitchKind to values.toMutableList().also { it[index] = value })
        applyGlitch()
    }

    private fun applyGlitch() {
        val v = glitchValues[glitchKind] ?: return
        session?.adjustGlitch(glitchKind, adjustAmount, v.dropLast(1), v.last())
    }

    private fun applyGradient() {
        val g = gradients.getOrNull(gradientIndex) ?: return
        val stops = g.stops.sortedBy { it.first }.flatMap { (p, c) ->
            listOf(p, ((c shr 16) and 0xFF) / 255f, ((c shr 8) and 0xFF) / 255f, (c and 0xFF) / 255f)
        }
        session?.adjustGradientMap(stops, adjustAmount)
    }

    // the menu a tap on the canvas opens during an adjustment

    /** Keeps what the adjustment did as one undo step and starts it again from there. */
    fun applyAdjustment() {
        val a = adjustment ?: return
        endAdjustment(true)
        startAdjustment(a)
    }

    /** Takes back the adjustment's changes, or with none, the last one kept. */
    fun undoAdjustment() {
        val a = adjustment ?: return
        if (adjustChanged()) {
            resetAdjustment()
            return
        }
        endAdjustment(false)
        session?.undo()
        startAdjustment(a)
    }

    fun togglePreview() {
        adjustPreview = !adjustPreview
        session?.adjustPreview(adjustPreview)
        adjustMenuAt = null
    }

    fun cancelAdjustment() = endAdjustment(false)

    /** Every value back to where the adjustment started. */
    fun resetAdjustment() {
        val a = adjustment ?: return
        endAdjustment(false)
        startAdjustment(a)
    }

    private fun adjustChanged(): Boolean = when (adjustment) {
        Adjustment.HSB -> listOf(hue50, saturation50, brightness50).any { it != 0.5f }
        Adjustment.COLOR_BALANCE -> balance.any { it != 0.5f }
        Adjustment.CURVES -> curves.any { it.size != 2 || it[0] != Offset(0f, 0f) || it[1] != Offset(1f, 1f) }
        Adjustment.GRADIENT_MAP -> true
        else -> adjustAmount > 0f
    }

    /** A liquify stroke's window points (x, y, pressure each); [start] begins it. */
    /** The liquify brush lifted moving at ([vx], [vy]) window pixels a millisecond: it carries on with its momentum. */
    fun liquifyLifted(vx: Float, vy: Float) {
        session?.liquifyCoast(vx, vy, liquifyMomentum, LiquifyBrush(liquifyKind, liquifySize, liquifyPressure, liquifyDistortion))
    }

    fun liquifyStroke(points: List<Float>, start: Boolean) {
        val s = session ?: return
        s.liquifyStroke(points, start, LiquifyBrush(liquifyKind, liquifySize, liquifyPressure, liquifyDistortion))
        liquified = true
    }

    fun chooseLiquifyKind(kind: LiquifyKind) {
        liquifyKind = kind
        liquifyPopup = 0
    }

    fun changeLiquifyAmount(amount: Float) {
        liquifyAmount = amount
        session?.liquifyAmount(amount)
    }

    fun resetLiquify() {
        session?.liquifyReset()
        liquified = false
        liquifyAmount = 1f
        liquifyPopup = 0
    }

    fun setHsb(h: Float, s: Float, b: Float) {
        hue50 = h
        saturation50 = s
        brightness50 = b
        session?.adjustHsb(h, s, b)
    }

    /** The adjustments button: closes an open adjustment (keeping it), or opens the menu. */
    fun adjustmentsPressed() {
        endQuickShape()
        if (adjustment != null) endAdjustment(true) else toggle(Panel.ADJUSTMENTS)
    }

    // ---- selection ----

    /** The selection button: opens or closes the selection tool (a selection made stays). */
    fun toggleSelecting() {
        endQuickShape()
        if (selecting) {
            closeSelecting()
            return
        }
        endTransform(true)
        endAdjustment(true)
        panel = Panel.NONE
        pickNext = false
        // each time the tool opens, shapes add to the selection
        selectionRemoves = false
        selecting = true
        refreshView()
        showSelectionLook()
    }

    fun closeSelecting() {
        if (!selecting) return
        closeFreehand()
        selecting = false
        selectionPopup = SelectionPopup.NONE
        showSelectionLook()
    }

    fun chooseSelectionKind(kind: SelectionKind) {
        closeFreehand()
        selectionKind = kind
        transformPrefs.edit().putString(SELECTION_KIND, kind.name).apply()
        showSelectionLook()
    }

    fun chooseSelectionRemoves(removes: Boolean) {
        selectionRemoves = removes
    }

    private val join get() = if (selectionRemoves) SelectionJoin.REMOVE else SelectionJoin.ADD

    /** A rectangle or ellipse dragged out between window points [a] and [b]. */
    fun selectShape(a: Offset, b: Offset) {
        val s = session ?: return
        shapeDrag = null
        if ((b - a).getDistance() < 1f) return
        s.selectShape(selectionKind == SelectionKind.ELLIPSE, a.x, a.y, b.x, b.y, join)
        selectionChanged()
    }

    /** Adds window point [p] to the freehand path; a point near its first one closes it. */
    fun freehandPoint(p: Offset, closeReach: Float) {
        val canvas = windowToCanvas(p) ?: return
        val first = freehand.firstOrNull()
        if (first != null && freehand.size > 2 && closeReach > 0f && (canvasToWindow(first) - p).getDistance() <= closeReach) {
            closeFreehand()
            return
        }
        freehand = freehand + canvas
    }

    /** The freehand path, held still, snaps to the shape it looks like; true if it did. */
    fun snapFreehand(): Boolean {
        val outline = io.github.artcraftandroid.procraft.core.quickshapeOutline(freehand.flatMap { listOf(it.x, it.y) }) ?: return false
        if (outline.size < 6) return false
        freehand = outline.chunked(2).map { (x, y) -> Offset(x, y) }
        return true
    }

    /** Closes the freehand path into a selection (a path of fewer than three points is dropped). */
    fun closeFreehand() {
        val s = session ?: return
        val path = freehand
        freehand = emptyList()
        if (path.size < 3) return
        s.selectPolygon(path.flatMap { canvasToWindow(it).let { w -> listOf(w.x, w.y) } }, join)
        selectionChanged()
    }

    /** Selects by colour at window point [p] with [threshold]; [live] while the threshold is dragged. */
    fun selectByColour(p: Offset, threshold: Float, live: Boolean) {
        val s = session ?: return
        thresholdDrag = if (live) p to threshold else null
        s.setSelectionDisplay(if (live) SelectionDisplay.FILL else SelectionDisplay.TINT)
        s.selectByColour(p.x, p.y, threshold, join, live)
        if (!live) {
            selectionThreshold = threshold
            transformPrefs.edit().putFloat(SELECTION_THRESHOLD, threshold).apply()
        }
        selectionChanged(fill = !live)
    }

    fun invertSelection() {
        session?.selectInvert()
        selectionChanged(fill = false)
    }

    fun copySelectionToLayer() {
        val s = session ?: return
        s.copySelectionToLayer() ?: return
        s.selectNone()
        selectionChanged(fill = false)
        refresh()
        if (panel == Panel.LAYERS) refreshThumbnails()
    }

    /** The feather popover's amount (0..1): the selection's edges softened from how they were before. */
    fun featherTo(amount: Float) {
        featherAmount = amount
        session?.selectFeather(amount)
    }

    fun openSelectionPopup(p: SelectionPopup) {
        selectionPopup = if (selectionPopup == p) SelectionPopup.NONE else p
        if (selectionPopup == SelectionPopup.FEATHER) featherAmount = 0f
        session?.let { savedSelections = it.savedSelectionCount().toInt() }
        if (selectionPopup == SelectionPopup.SAVED) loadSavedThumbnails()
    }

    fun saveSelection() {
        savedSelections = session?.saveSelection()?.toInt() ?: savedSelections
        loadSavedThumbnails()
    }

    private fun loadSavedThumbnails() {
        val s = session ?: return
        savedThumbnails.clear()
        for (i in 0 until savedSelections) {
            val t = s.savedSelectionThumbnail(i.toUInt(), 128u, 92u) ?: continue
            val bitmap = io.github.artcraftandroid.procraft.ui.CanvasSpace.tag(Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), Bitmap.Config.ARGB_8888))
            bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(t.rgba))
            savedThumbnails[i] = bitmap.asImageBitmap()
        }
    }

    fun loadSelection(i: Int) {
        session?.loadSelection(i.toUInt())
        selectionChanged(fill = false)
    }

    fun deleteSavedSelection(i: Int) {
        val s = session ?: return
        s.deleteSavedSelection(i.toUInt())
        savedSelections = s.savedSelectionCount().toInt()
        loadSavedThumbnails()
    }

    fun toggleColorFill() {
        colorFill = !colorFill
        if (colorFill && hasSelection) {
            session?.fillSelection()
            refresh()
        }
    }

    fun clearSelection() {
        closeFreehand()
        session?.selectNone()
        selectionChanged(fill = false)
    }

    private fun selectionChanged(fill: Boolean = true) {
        val s = session ?: return
        hasSelection = s.hasSelection()
        if (fill && colorFill && hasSelection) {
            s.fillSelection()
            refresh()
        }
        showSelectionLook()
    }

    /** Bands over what is not selected, except while selecting by colour, which tints what is. */
    private fun showSelectionLook() {
        val tint = selecting && selectionKind == SelectionKind.AUTOMATIC
        session?.setSelectionDisplay(if (tint) SelectionDisplay.TINT else SelectionDisplay.STRIPES)
    }

    /** Window pixels to canvas pixels through the view, or none when it cannot be inverted. */
    fun windowToCanvas(p: Offset): Offset? {
        val (a, b, c, d) = viewMatrix
        val e = viewMatrix[4]
        val f = viewMatrix[5]
        val det = a * d - b * c
        if (kotlin.math.abs(det) < 1e-9f) return null
        val x = p.x - e
        val y = p.y - f
        return Offset((d * x - c * y) / det, (a * y - b * x) / det)
    }

    fun canvasToWindow(p: Offset): Offset {
        val m = viewMatrix
        return Offset(m[0] * p.x + m[2] * p.y + m[4], m[1] * p.x + m[3] * p.y + m[5])
    }

    // ---- transform ----

    /** The transform button: opens transform on the current layer, or keeps it and closes. */
    fun toggleTransform() {
        endQuickShape()
        if (transforming) {
            endTransform(true)
            return
        }
        val s = session ?: return
        if (!currentTakesChanges()) return
        closeSelecting()
        endAdjustment(true)
        panel = Panel.NONE
        pickNext = false
        s.transformSetInterpolation(interpolation)
        s.transformSetGuides(guides)
        transformBox = s.transformBegin()
        // nothing there to move: it says so and stays out of transform
        if (transformBox == null) {
            showNotice(
                when {
                    currentIsText() -> "Rasterize the text to transform it"
                    hasSelection -> "Nothing is selected on this layer"
                    else -> "This layer is empty"
                },
            )
            return
        }
        // a warp lays its mesh over what began
        transformBox = s.transformSetKind(transformKind) ?: transformBox
        transformPopup = TransformPopup.NONE
        refreshView()
        transforming = true
    }

    /** Ends transform: [commit] keeps the change as one undo step, otherwise the layer goes back. */
    fun endTransform(commit: Boolean) {
        if (!transforming) return
        session?.transformEnd(commit)
        transforming = false
        transformBox = null
        transformPopup = TransformPopup.NONE
        transformNumeric = null
        refresh()
        if (panel == Panel.LAYERS) refreshThumbnails()
    }

    fun chooseTransformKind(kind: TransformKind) {
        transformKind = kind
        session?.transformSetKind(kind)?.let { transformBox = it }
        transformPrefs.edit().putString(TRANSFORM_KIND, kind.name).apply()
    }

    fun chooseInterpolation(i: Interpolation) {
        interpolation = i
        session?.transformSetInterpolation(i)
        transformPrefs.edit().putString(INTERPOLATION, i.name).apply()
    }

    fun toggleAdvancedMesh() {
        advancedMesh = !advancedMesh
        transformPrefs.edit().putBoolean(ADVANCED_MESH, advancedMesh).apply()
        showNotice("Advanced mesh " + if (advancedMesh) "on" else "off")
    }

    fun changeGuides(g: GuideSettings) {
        if (g.snapping != guides.snapping) showNotice("Snapping " + if (g.snapping) "on" else "off")
        else if (g.magnetics != guides.magnetics) showNotice("Magnetics " + if (g.magnetics) "on" else "off")
        guides = g
        session?.transformSetGuides(g)
        transformPrefs.edit()
            .putBoolean(MAGNETICS, g.magnetics)
            .putBoolean(SNAPPING, g.snapping)
            .putFloat(SNAP_DISTANCE, g.distance)
            .putFloat(SNAP_VELOCITY, g.velocity)
            .apply()
    }

    private inline fun <reified E : Enum<E>> enumPref(key: String, default: E): E =
        transformPrefs.getString(key, null)?.let { name -> enumValues<E>().firstOrNull { it.name == name } } ?: default

    fun transformAction(action: TransformAction) {
        transformPopup = TransformPopup.NONE
        transformBox = session?.transformApply(action) ?: transformBox
    }

    /** The box after a drag step on the canvas. */
    fun transformMoved(box: TransformBox?) {
        if (box != null) transformBox = box
    }

    /** Numeric entry open beside window point [at]: for the box's angle (the rotation handle) or its size. */
    data class TransformNumeric(val angle: Boolean, val at: Offset)

    var transformNumeric by mutableStateOf<TransformNumeric?>(null)

    fun transformSetSize(width: Float, height: Float) = transformMoved(session?.transformSetSize(width, height))

    fun transformSetAngle(degrees: Float) = transformMoved(session?.transformSetAngle(degrees))

    /** Re-reads the view, after a gesture moved it, while something is drawn over the canvas. */
    /** Fits the canvas to the screen, settling into place; what is drawn over it follows each frame. */
    fun settleToFit() {
        session?.settleToFit()
        followView()
    }

    /** What is drawn over the canvas follows the view each frame while it moves into place. */
    private fun followView() {
        val s = session ?: return
        settling?.cancel()
        settling = viewModelScope.launch {
            val until = System.nanoTime() + (s.viewSettleSeconds() * 1e9).toLong() + SETTLE_MARGIN_NS
            while (System.nanoTime() < until && s.viewMoving()) {
                refreshView()
                kotlinx.coroutines.delay(FRAME_MS)
            }
            refreshView()
        }
    }

    private var settling: kotlinx.coroutines.Job? = null

    fun refreshView() {
        session?.let {
            viewMatrix = it.viewMatrix()
            // the guide's lines follow the canvas
            guide = it.guide()
        }
        // the shape's handles follow the canvas
        if (quickShape != null) quickShape = session?.quickshapeInfo()
    }

    fun toggleBars() {
        if (transforming) return
        barsHidden = !barsHidden
        if (barsHidden) panel = Panel.NONE
    }

    /** A stroke finished: its layer's thumbnail and the undo state changed. */
    fun strokeEnded() {
        // a stroke on a locked or hidden layer drew nothing: say why (text layers ask to rasterize instead)
        if (adjustment == null && !currentIsText()) currentTakesChanges()
        refreshFlags()
        if (animation != null) refreshFrameThumbnails()
        refreshReference()
        recordFrame()
        // painting an adjustment on uses no colour
        if (tool == ToolKind.PAINT && adjustment == null) {
            val c = colorRgb()
            history = (listOf(c) + history.filter { it != c }).take(HISTORY_SIZE)
        }
        if (panel == Panel.LAYERS) layers.firstOrNull { it.current }?.let { loadThumbnail(it.id) }
    }

    // ---- Animation Assist ----

    /** Animation Assist's settings while it is on. */
    var animation by mutableStateOf<io.github.artcraftandroid.procraft.core.AnimationSettings?>(null)
        private set

    /** The frames' layers, bottom first, and which one is current. */
    var animationFrames by mutableStateOf<List<ULong>>(emptyList())
        private set
    var currentFrame by mutableStateOf<Int?>(null)
        private set

    /** Each frame's picture for the timeline. */
    val frameThumbnails = mutableStateMapOf<ULong, ImageBitmap>()

    /** Playing: the frame shown. */
    var playingFrame by mutableStateOf<Int?>(null)
        private set
    private var playJob: Job? = null

    var animationSettingsOpen by mutableStateOf(false)
    var frameOptionsOpen by mutableStateOf(false)

    fun changeAnimationAssist(on: Boolean) {
        stopPlaying()
        session?.setAnimationAssist(on)
        refresh()
    }

    fun changePageAssist(on: Boolean) {
        stopPlaying()
        session?.setPageAssist(on)
        refresh()
    }

    /** Re-reads Animation Assist from the session (after any change to the layers). */
    private fun refreshAnimation() {
        val s = session ?: return
        val settings = s.animationSettings()
        animation = settings
        if (settings == null) {
            animationFrames = emptyList()
            currentFrame = null
            animationSettingsOpen = false
            frameOptionsOpen = false
            return
        }
        animationFrames = s.animationFrames()
        currentFrame = s.currentFrame()?.toInt()
        refreshFrameThumbnails()
    }

    private var frameThumbJob: Job? = null

    private fun refreshFrameThumbnails() {
        val s = session ?: return
        val ids = animationFrames
        frameThumbJob?.cancel()
        frameThumbJob = viewModelScope.launch {
            for (id in ids) {
                val image = withContext(Dispatchers.Default) {
                    val t = s.frameThumbnail(id, FRAME_THUMB_SIDE.toUInt()) ?: return@withContext null
                    val bitmap = io.github.artcraftandroid.procraft.ui.CanvasSpace.tag(Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), Bitmap.Config.ARGB_8888))
                    bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(t.rgba))
                    bitmap.asImageBitmap()
                }
                if (image != null) frameThumbnails[id] = image else frameThumbnails.remove(id)
            }
        }
    }

    fun changeAnimation(f: (io.github.artcraftandroid.procraft.core.AnimationSettings) -> io.github.artcraftandroid.procraft.core.AnimationSettings) {
        val s = session ?: return
        val a = animation ?: return
        s.setAnimationSettings(f(a))
        animation = s.animationSettings()
    }

    /** Frame [i] becomes the one drawn on (a tap on the current one opens its options). */
    fun tapFrame(i: Int) {
        if (playingFrame != null) stopPlaying()
        if (i == currentFrame) {
            frameOptionsOpen = !frameOptionsOpen
            animationSettingsOpen = false
            return
        }
        frameOptionsOpen = false
        animationFrames.getOrNull(i)?.let { selectLayer(it) }
    }

    fun addFrame() {
        stopPlaying()
        frameOptionsOpen = false
        addLayer()
    }

    private fun currentFrameLayer(): LayerInfo? = currentFrame?.let { animationFrames.getOrNull(it) }?.let { id -> layers.firstOrNull { it.id == id } }

    fun duplicateFrame() {
        frameOptionsOpen = false
        currentFrameLayer()?.let(::duplicate)
    }

    fun deleteFrame() {
        frameOptionsOpen = false
        currentFrameLayer()?.let(::delete)
    }

    fun frameHold(): Int = currentFrame?.let { animationFrames.getOrNull(it) }?.let { session?.frameHold(it)?.toInt() } ?: 0

    var holdVersion by mutableStateOf(0)
        private set

    fun changeFrameHold(frames: Int) {
        val id = currentFrame?.let { animationFrames.getOrNull(it) } ?: return
        session?.setFrameHold(id, frames.coerceIn(0, MAX_HOLD).toUInt())
        holdVersion++
    }

    /** The frames in playing order, each as often as it is held. */
    private fun playOrder(): List<Int> {
        val s = session ?: return emptyList()
        return animationFrames.flatMapIndexed { i, id -> List(1 + s.frameHold(id).toInt()) { i } }
    }

    fun togglePlaying() = if (playingFrame != null) stopPlaying() else play()

    private fun play() {
        val s = session ?: return
        val a = animation ?: return
        val order = playOrder()
        if (order.isEmpty()) return
        animationSettingsOpen = false
        frameOptionsOpen = false
        playJob?.cancel()
        playJob = viewModelScope.launch {
            var at = 0
            var step = 1
            while (true) {
                val frame = order[at]
                playingFrame = frame
                s.setPlayFrame(frame.toUInt())
                kotlinx.coroutines.delay(1000L / (animation?.fps ?: a.fps).toInt().coerceAtLeast(1))
                val next = at + step
                when {
                    next in order.indices -> at = next
                    a.playback == io.github.artcraftandroid.procraft.core.AnimationPlayback.LOOP -> at = 0
                    a.playback == io.github.artcraftandroid.procraft.core.AnimationPlayback.PING_PONG && order.size > 1 -> {
                        step = -step
                        at += step
                    }
                    else -> break
                }
            }
            // a single pass ends on its last frame, still shown
        }
    }

    fun stopPlaying() {
        playJob?.cancel()
        playJob = null
        if (playingFrame != null) {
            playingFrame = null
            session?.setPlayFrame(null)
        }
    }

    // ---- layers ----

    fun addLayer() = layerChange { if (it.addLayer() == 0uL) message = noRoom() }

    fun selectLayer(id: ULong) = layerChange { it.selectLayer(id) }

    fun toggleVisible(l: LayerInfo) = layerChange { it.setLayerVisible(l.id, !l.visible) }

    fun setLayerOpacity(l: LayerInfo, value: Float, done: Boolean) = layerChange(thumbs = false) { it.setLayerOpacity(l.id, value, done) }

    fun setBlend(l: LayerInfo, blend: UInt) = layerChange(thumbs = false) { it.setLayerBlend(l.id, blend) }

    fun toggleAlphaLock(l: LayerInfo) = layerChange(thumbs = false) { it.setLayerAlphaLock(l.id, !l.alphaLock) }

    /** A mask's layer; anything else itself. */
    fun ownerOf(l: LayerInfo): LayerInfo = if (l.kind == LayerType.MASK) layers.firstOrNull { it.id == l.parent } ?: l else l

    /** Whether [l] is somewhere inside group [group]. */
    fun isInside(l: LayerInfo, group: ULong): Boolean {
        var at = ownerOf(l).parent
        repeat(layers.size) {
            val id = at ?: return false
            if (id == group) return true
            at = layers.firstOrNull { it.id == id }?.parent
        }
        return false
    }

    /** Whether a closed group hides [l]'s row. */
    fun hidden(l: LayerInfo) = layers.any { it.kind == LayerType.GROUP && it.collapsed && isInside(l, it.id) }

    fun hasMask(l: LayerInfo) = layers.any { it.kind == LayerType.MASK && it.parent == l.id }

    /** The rows that go with [l]: it with its mask, or a group with everything in it. */
    fun unitOf(l: LayerInfo): Set<ULong> = layers
        .filter { it.id == l.id || (it.kind == LayerType.MASK && it.parent == l.id) || (l.kind == LayerType.GROUP && isInside(it, l.id)) }
        .map { it.id }
        .toSet()

    /**
     * Moves [l] (with what goes with it) just above or below [target]'s row; a chosen layer with
     * others picked takes them along, in their order.
     */
    fun moveNextTo(l: LayerInfo, target: LayerInfo, above: Boolean) = layerChange(thumbs = false) {
        val together = anyPicked && (l.current || l.picked)
        val unit = if (together) {
            // a picked mask moves with its layer
            layers.filter { it.current || it.picked }
                .mapNotNull { c -> if (c.kind == LayerType.MASK) layers.firstOrNull { it.id == c.parent } else c }
                .flatMap { unitOf(it) }.toSet()
        } else {
            unitOf(l)
        }
        if (target.id in unit) return@layerChange null
        val rest = layers.filter { it.id !in unit }.asReversed()
        val at = rest.indexOfFirst { it.id == target.id }
        if (at < 0) return@layerChange null
        val to = (if (above) at + 1 else at).toUInt()
        if (together) it.moveChosen(to) else it.moveLayer(l.id, to)
    }

    fun addMask(l: LayerInfo) = layerChange { if (it.addMask(l.id) == null && !hasMask(l)) message = noRoom() }

    fun combineDown(l: LayerInfo) = layerChange { it.combineDown(l.id) }

    fun ungroup(l: LayerInfo) = layerChange { it.ungroup(l.id) }

    fun flattenGroup(l: LayerInfo) = layerChange { it.flattenGroup(l.id) }

    fun toggleCollapsed(l: LayerInfo) = layerChange(thumbs = false) { it.setGroupCollapsed(l.id, !l.collapsed) }

    fun selectLayerContents(l: LayerInfo) {
        session?.selectLayerContents(l.id)
        refreshFlags()
    }

    fun copyLayerContents(l: LayerInfo) {
        clipboard = session?.copyLayerContents(l.id) ?: clipboard
        canPaste = clipboard != null
    }

    fun fillLayer(l: LayerInfo) = layerChange { it.fillLayer(l.id) }

    fun invertLayer(l: LayerInfo) = layerChange { it.invertLayer(l.id) }

    fun toggleReference(l: LayerInfo) = layerChange(thumbs = false) { it.setLayerReference(l.id, !l.reference) }

    fun toggleClipped(l: LayerInfo) = layerChange(thumbs = false) { it.setLayerClipped(l.id, !l.clipped) }

    fun toggleLocked(l: LayerInfo) = layerChange(thumbs = false) { it.setLayerLocked(l.id, !l.locked) }

    fun duplicate(l: LayerInfo) = layerChange { if (it.duplicateLayer(l.id) == null) message = noRoom() }

    fun delete(l: LayerInfo) = layerChange { it.deleteLayer(l.id) }

    fun mergeDown(l: LayerInfo) = layerChange { it.mergeDown(l.id) }

    /** Whether layers are picked to act on with the current one. */
    val anyPicked: Boolean get() = layers.any { it.picked }

    /** Picks layer [l] to act on with the current one, or unpicks it. */
    fun pickLayer(l: LayerInfo) = layerChange(thumbs = false) { it.pickLayer(l.id) }

    fun pickAllLayers() = layerChange(thumbs = false) { it.pickAllLayers() }

    fun groupPicked() = layerChange { it.groupPicked() }

    fun mergePicked() = layerChange { it.mergePicked() }

    fun duplicatePicked() = layerChange { if (!it.duplicatePicked()) message = noRoom() }

    fun deletePicked() = layerChange { it.deletePicked() }

    /** The current and picked layers each as a PNG in the cache, to share. */
    suspend fun chosenLayerFiles(): List<File> {
        val s = session ?: return emptyList()
        return withContext(Dispatchers.IO) {
            runCatching {
                val folder = File(File(getApplication<Application>().cacheDir, "exports"), "layers").apply {
                    deleteRecursively()
                    mkdirs()
                }
                s.exportChosenPngs(folder.path).map { File(it) }
            }.onFailure { Log.e(TAG, "exporting layers", it) }.getOrDefault(emptyList())
        }
    }

    fun clearCurrentLayer() {
        refresh()
        layers.firstOrNull { it.current }?.let {
            if (currentTakesChanges()) {
                clear(it)
                showNotice("Layer cleared")
            }
        }
    }

    fun clear(l: LayerInfo) = layerChange {
        it.selectLayer(l.id)
        it.clearLayer()
    }

    fun rename(l: LayerInfo, name: String) = layerChange(thumbs = false) { it.renameLayer(l.id, name) }

    /** Moves a layer to `index` in the panel's top-first order. */
    fun moveTo(l: LayerInfo, index: Int) = layerChange(thumbs = false) {
        val fromBottom = (layers.size - 1 - index).coerceAtLeast(0)
        it.moveLayer(l.id, fromBottom.toUInt())
    }

    fun toggleBackground() = layerChange(thumbs = false) { it.setBackgroundVisible(!background.visible) }

    /** The Layers panel shows the colour panel, picking the background's colour. */
    var backgroundPicking by mutableStateOf(false)
        private set

    fun pickBackground() {
        backgroundPicking = true
    }

    /** The background's colour as 0xRRGGBB. */
    fun backgroundRgb(): Int {
        fun c(v: Float) = (v.coerceIn(0f, 1f) * 255f).roundToInt()
        return (c(background.r) shl 16) or (c(background.g) shl 8) or c(background.b)
    }

    /** Sets the background's colour, live while a picker moves; [done] makes the change one undo step. */
    fun setBackgroundRgb(rgb: Int, done: Boolean) {
        val s = session ?: return
        s.setBackgroundColor(((rgb shr 16) and 0xFF) / 255f, ((rgb shr 8) and 0xFF) / 255f, (rgb and 0xFF) / 255f, done)
        if (done) refresh() else background = s.background()
    }

    /** The background picker closes (its tick, or the panel going away): the colour is kept as one undo step. */
    fun endBackgroundPick() {
        if (!backgroundPicking) return
        backgroundPicking = false
        setBackgroundRgb(backgroundRgb(), done = true)
    }

    private fun layerChange(thumbs: Boolean = true, f: (CanvasSession) -> Any?) {
        val s = session ?: return
        f(s)
        refresh()
        if (thumbs && panel == Panel.LAYERS) refreshThumbnails()
    }

    /** Re-reads the layers and undo state from the session. */
    fun refresh() {
        val s = session ?: return
        refreshReference()
        recordFrame()
        // undo and redo keep a snapped shape as drawn first
        if (quickShape != null && s.quickshapeInfo() == null) {
            quickShape = null
            quickShapeEditing = false
        }
        layers = s.layers()
        background = s.background()
        // undoing may put the canvas back in another profile
        refreshCanvasSpace(s)
        refreshFlags()
        refreshAnimation()
    }

    private fun refreshFlags() {
        val info = session?.info() ?: return
        canUndo = info.canUndo
        canRedo = info.canRedo
        hasSelection = session?.hasSelection() ?: false
    }

    private fun refreshThumbnails() {
        for (l in layers) loadThumbnail(l.id)
    }

    private fun loadThumbnail(id: ULong) {
        val s = session ?: return
        viewModelScope.launch {
            val image = withContext(Dispatchers.Default) {
                val t = s.layerThumbnail(id, THUMB_SIDE.toUInt()) ?: return@withContext null
                val bitmap = io.github.artcraftandroid.procraft.ui.CanvasSpace.tag(Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), Bitmap.Config.ARGB_8888))
                bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(t.rgba))
                bitmap.asImageBitmap()
            }
            if (image != null) thumbnails[id] = image
        }
    }

    private fun readSliders() {
        val info = session?.info() ?: return
        size = info.size
        opacity = info.opacity
        readNotches()
    }

    override fun onCleared() {
        saveNow()
        session = null
    }

    companion object {
        /** While the view settles, what is drawn over the canvas is moved this often (ms), and a little past its end (ns). */
        const val FRAME_MS = 16L
        const val SETTLE_MARGIN_NS = 50_000_000L

        /** The clone source's ring (points from its centre), and how far off it a touch still takes it. */
        const val CLONE_PUCK_RADIUS_DP = 30f
        const val CLONE_PUCK_SLOP_DP = 8f
        /** Each glitch kind's sliders as they start: its own two (diverge three), then the zoom. */
        val GLITCH_DEFAULTS = mapOf(
            GlitchKind.ARTIFACT to listOf(1f, 0.09f, 1f),
            GlitchKind.WAVE to listOf(0.2f, 0.3f, 1f),
            GlitchKind.SIGNAL to listOf(0.8f, 0.19f, 1f),
            GlitchKind.DIVERGE to listOf(0.98f, -0.5f, 0.85f, 0.5f),
        )
        const val BRUSH_CHANGES = "changes"
        const val BRUSH_LIBRARY = "library"
        /** How big the studio's Shape and Grain Source rows show their image (pixels across). */
        const val SOURCE_ROW_PX = 88
        /** And the landscape studio's square of it. */
        const val SOURCE_BOX_PX = 566
        /** The brushes in use, each set's last choice and each brush's sliders. */
        const val BRUSH_CHOICES = "choices"

        /** How many brushes of each shipped set the kept library has had the chance to hold. */
        const val BRUSHES_SEEN = "brushes_seen"

        /** What shipped before brushes and sets were added to kept libraries. */
        val FIRST_SHIPPED = listOf(3u, 3u, 3u, 2u, 2u, 2u)
        /** Said when a layer can't be added: the canvas holds no more at its size. */
        /** The memory layers may take, which sets how many a canvas can hold. */
        const val LAYER_BUDGET = 2L * 1024 * 1024 * 1024
        const val MAX_CANVAS_EDGE = 16384
        /** How much smaller than fitted to the screen the canvas is while cropping. */
        const val CROP_SCALE = 0.597f
        /** The reference window: its first place and picture size, its limits, and how sharp and how often its canvas is read. */
        /** The reference window's picture at first, and its gap to the screen's bottom. */
        const val REFERENCE_WIDTH_DP = 350f
        const val REFERENCE_HEIGHT_DP = 423f
        const val REFERENCE_BOTTOM_DP = 50f
        const val REFERENCE_MIN_DP = 150f
        const val REFERENCE_MAX_DP = 700f
        const val REFERENCE_PIXELS = 700
        const val REFERENCE_INTERVAL_MS = 300L
        /** New text's size: the canvas's shorter side over this. */
        /** New text's size and its box's width, as fractions of the canvas width. */
        const val TEXT_SIZE_OF_WIDTH = 0.13f
        const val TEXT_SCREEN_Y = 0.24f
        const val TEXT_BOX_OF_WIDTH = 1f
        const val RIGHT_HANDED = "right_handed"
        const val COLOR_MODE = "color_mode"
        const val ACTIONS_TAB = "actions_tab"
        const val STUDIO_TAB = "studio_tab"
        const val RAPID_UNDO_DELAY = "rapid_undo_delay"
        const val LIGHT_INTERFACE = "light_interface"
        const val BRUSH_CURSOR = "brush_cursor"
        const val CURSOR_SHOWS = "cursor_shows"
        const val CURSOR_STYLE = "cursor_style"
        const val BRUSH_ON_SCREEN = "brush_on_screen"
        const val PROJECT_CANVAS = "project_canvas"
        const val PALETTE_CARDS = "palette_cards"
        const val COLOR_HISTORY_SHOWN = "color_history_shown"
        const val ORIENTATION_MEMORY = "orientation_memory"
        const val FIT_INSIDE = "fit_inside"
        const val NO_TIME_LAPSE = "no_time_lapse"
        const val SIMPLIFIED_UNDOS = "simplified_undos"

        /** The picture of the canvas the gallery zooms out of, at most this many pixels on a side. */
        const val CLOSING_PICTURE_SIDE = 768
        const val COLOR_NAMES = "color_names"
        const val FEEDBACK_SOUNDS = "feedback_sounds"
        const val SINGLE_TOUCH_COMPANION = "single_touch_companion"
        const val TAP_ASSISTANCE = "tap_assistance"

        /** The layered package's file extension, and its name in the Share lists. */
        const val PACKAGE_EXTENSION = "procreate"

        /** Brush and brush set files, as other painting apps share them. */
        const val BRUSH_EXTENSION = "brush"
        const val BRUSH_SET_EXTENSION = "brushset"
        val PACKAGE_LABEL = PACKAGE_EXTENSION.replaceFirstChar(Char::titlecase)

        /** What the gallery's Share offers, in order: still pictures, then animations. */
        val SHARE_FORMATS = listOf(PACKAGE_LABEL, "PSD", "PDF", "JPEG", "PNG", "TIFF") + Animated.entries.map { it.title }

        /** How often, and for how long at most, opening waits for the canvas's first frame, milliseconds. */
        const val CANVAS_POLL_MS = 32L
        const val CANVAS_DRAWN_LIMIT_MS = 15_000L

        /** How near a slider value is on a notch (and is pulled onto it), and the most a slider takes. */
        const val NOTCH_NEAR = 0.04f
        const val NOTCH_MOST = 4

        /** The least time between two colour names while the colour keeps changing, milliseconds. */
        const val COLOR_NAME_GAP_MS = 150L

        /** How often an open artwork is saved when it has changed, milliseconds. */
        const val AUTOSAVE_MS = 3000L
        const val SPEED_DISTANCE = "speed_distance"

        /** What an artwork dragged out of the gallery becomes (an index into [DRAG_FORMATS]), and a picture's format when one is wanted. */
        const val DRAG_FORMAT = "drag_format"
        const val DRAG_IMAGE_FORMAT = "drag_image_format"
        val DRAG_FORMATS = listOf("ProCraft", "PSD", "JPEG", "PNG", "TIFF")
        val DRAG_IMAGE_FORMATS = listOf("JPEG", "PNG", "TIFF")
        const val QUICK_MENU = "quick_menu"
        /** A press of □ this short, used by nothing, is a tap. */
        const val MODIFY_TAP_MS = 500L
        const val PRESETS = "presets"
        const val INPUT_STABILIZATION = "input_stabilization"
        const val INPUT_MOTION = "input_motion_filtering"
        const val INPUT_EXPRESSION = "input_motion_expression"
        const val INPUT_PRESSURE_SMOOTHING = "input_pressure_smoothing"
        const val INPUT_CURVE = "input_pressure_curve"
        /** Frames a second an animation plays at (Animation Assist's own when it is on). */
        const val ANIMATION_FPS = 15
        /** The timeline's frame pictures' longest side, pixels; a frame's longest hold. */
        const val FRAME_THUMB_SIDE = 96
        const val MAX_HOLD = 60
        /** A Web Ready animation's longest side, pixels. */
        const val WEB_READY_SIDE = 480
        /** An animated video's longest side at most, pixels. */
        const val MAX_VIDEO_SIDE = 4096
        const val RAPID_UNDO_MIN_S = 0.1f
        const val RAPID_UNDO_MAX_S = 1.5f
        const val SELECTION_VISIBILITY = "selection_visibility"
        /** The guide editor fits the canvas this far in from the window's sides, top and bottom (points). */
        const val GUIDE_FIT_SIDE_DP = 40f
        const val GUIDE_FIT_TOP_DP = 226f
        const val GUIDE_FIT_BOTTOM_DP = 228f
        /** The largest side a picture brought in is decoded at (pixels). */
        const val MAX_PICTURE = 4096
        /** How long a stroke is held still to snap (milliseconds), how still (points), and how near a handle a touch takes it. */
        const val QUICKSHAPE_MS = 650L
        const val QUICKSHAPE_STILL_DP = 6f
        const val QUICKSHAPE_REACH_DP = 30f
        /** The drawing pad's colours (0xRRGGBB), the white one, and its background's grey. */
        val PAD_COLORS = listOf(0xF83045, 0xFCBD03, 0x85D921, 0x2BB5E6, 0x4956A5, 0xDA3D92, 0xFFFFFF, 0x000000)
        const val PAD_WHITE = 6
        const val PAD_BACK = 31f / 255f
        /** How long after a studio control stops moving its sample stroke is drawn again (milliseconds). */
        const val STUDIO_PREVIEW_DELAY_MS = 60L
        /** Bloom's size and burn sliders when painted on. */
        const val PENCIL_BLOOM_SIZE = 0.335f
        const val PENCIL_BLOOM_BURN = 0.57f
        /** How far a motion blur's drag goes for its whole amount (points). */
        const val MOTION_DRAG_PT = 500f
        /** The first colour: a saturated blue. */
        const val DEFAULT_HUE = 0.59f
        /** Layer thumbnails are read at about the size they show (93 dp at 2 px a dp). */
        const val THUMB_SIDE = 192
        const val HISTORY_SIZE = 10
        private const val TRANSFORM_KIND = "kind"
        private const val INTERPOLATION = "interpolation"
        private const val MAGNETICS = "magnetics"
        private const val SNAPPING = "snapping"
        private const val SNAP_DISTANCE = "snapDistance"
        private const val SNAP_VELOCITY = "snapVelocity"
        private const val ADVANCED_MESH = "advancedMesh"
        private const val SELECTION_KIND = "selectionKind"
        private const val SELECTION_THRESHOLD = "selectionThreshold"

        fun hsvToRgb(h: Float, s: Float, v: Float): Int {
            val hsv = floatArrayOf((h * 360f) % 360f, s, v)
            return android.graphics.Color.HSVToColor(hsv) and 0xFFFFFF
        }
    }
}

/**
 * Up to [count] colours standing for [pixels] (0xRRGGBB): the colours split again and again at the
 * middle of their widest channel, each part's mean kept; lightest first.
 */
internal fun mainColours(pixels: List<Int>, count: Int): List<Int> {
    if (pixels.isEmpty()) return emptyList()
    fun ch(c: Int, k: Int) = (c shr (16 - 8 * k)) and 0xFF
    fun spread(box: List<Int>) = (0 until 3).maxOf { k -> box.maxOf { ch(it, k) } - box.minOf { ch(it, k) } }
    val boxes = mutableListOf(pixels)
    while (boxes.size < count) {
        val widest = boxes.filter { it.size > 1 }.maxByOrNull { spread(it) } ?: break
        if (spread(widest) == 0) break
        val k = (0 until 3).maxBy { k -> widest.maxOf { ch(it, k) } - widest.minOf { ch(it, k) } }
        val sorted = widest.sortedBy { ch(it, k) }
        boxes.remove(widest)
        boxes.add(sorted.subList(0, sorted.size / 2))
        boxes.add(sorted.subList(sorted.size / 2, sorted.size))
    }
    val means = boxes.map { box -> (0 until 3).map { k -> box.sumOf { ch(it, k) } / box.size }.let { (r, g, b) -> (r shl 16) or (g shl 8) or b } }
    return means.distinct().sortedByDescending { 0.299 * ch(it, 0) + 0.587 * ch(it, 1) + 0.114 * ch(it, 2) }
}
