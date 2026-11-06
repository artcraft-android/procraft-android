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
import io.github.artcraftandroid.procraft.core.BrushInfo
import io.github.artcraftandroid.procraft.core.BrushSetInfo
import io.github.artcraftandroid.procraft.core.RecentBrush
import io.github.artcraftandroid.procraft.core.CanvasSession
import io.github.artcraftandroid.procraft.core.ModelFile
import io.github.artcraftandroid.procraft.core.SourceKind
import io.github.artcraftandroid.procraft.core.ChromaticKind
import io.github.artcraftandroid.procraft.core.PerspectiveKind
import io.github.artcraftandroid.procraft.core.GlitchKind
import io.github.artcraftandroid.procraft.core.GuideInfo
import io.github.artcraftandroid.procraft.core.KeyShortcut
import io.github.artcraftandroid.procraft.core.ModeButton
import io.github.artcraftandroid.procraft.core.Modes
import io.github.artcraftandroid.procraft.core.modeSwitch
import io.github.artcraftandroid.procraft.core.GuideType
import io.github.artcraftandroid.procraft.core.GuideUnit
import io.github.artcraftandroid.procraft.core.SymmetryType
import io.github.artcraftandroid.procraft.core.HalftoneKind
import io.github.artcraftandroid.procraft.core.NoiseKind
import io.github.artcraftandroid.procraft.core.Adjustment
import io.github.artcraftandroid.procraft.core.GesturePurpose
import io.github.artcraftandroid.procraft.core.QuickAction
import io.github.artcraftandroid.procraft.core.QuickShapeInfo
import io.github.artcraftandroid.procraft.core.QuickShapeKind
import io.github.artcraftandroid.procraft.core.Trigger
import io.github.artcraftandroid.procraft.core.GuideSettings
import io.github.artcraftandroid.procraft.core.Interpolation
import io.github.artcraftandroid.procraft.core.LayerInfo
import io.github.artcraftandroid.procraft.core.TextAlignment
import io.github.artcraftandroid.procraft.core.TextInfo
import io.github.artcraftandroid.procraft.core.TextSide
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
import io.github.artcraftandroid.procraft.core.blendModes
import io.github.artcraftandroid.procraft.core.keyShortcut as sharedKeyShortcut
import io.github.artcraftandroid.procraft.core.brushBlendModes
import io.github.artcraftandroid.procraft.core.grainBlendModes
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlin.math.abs
import kotlin.math.roundToInt
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.text.input.TextFieldValue
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.io.File
import java.nio.ByteBuffer

/** Which panel is open over the canvas. */
private const val TAG = "ProCraft"

/** [CanvasViewModel.shownSet] while the library shows the Recent set. */
const val RECENT_SET = -1

/** Where the set at place [set] is once the set at [from] has moved to place [to]. */
internal fun placeAfterSetMove(set: Int, from: Int, to: Int): Int = when (set) {
    from -> to
    in (from + 1)..to -> set - 1
    in to until from -> set + 1
    else -> set
}

enum class Panel { NONE, LAYERS, COLOR, BRUSHES, ACTIONS, ADJUSTMENTS }

/** The eyedropper while it is open: the window point (pixels), what is shown around it, the colour it would pick. */
class PickState(val x: Float, val y: Float, val image: ImageBitmap?, val rgb: Int?, val fromReference: Boolean = false)

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

/** The selection tool's ways of making a selection. */
enum class SelectionKind(val label: String) { AUTOMATIC("Automatic"), FREEHAND("Freehand"), RECTANGLE("Rectangle"), ELLIPSE("Ellipse") }

/** A popover over the selection toolbar. */
enum class SelectionPopup { NONE, FEATHER, SAVED }

/** The gallery, the open artwork and the state of the controls around it; outlives configuration changes. */
class CanvasViewModel(app: Application) : AndroidViewModel(app) {
    var session: CanvasSession? = null
        private set

    /** Frosted panels open over the canvas, for the engine to blur what is behind them. */
    val backdrops = io.github.artcraftandroid.procraft.ui.Backdrops { regions, windows -> session?.setGlass(regions, windows) }

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

    /** A save failed and was told of: the next failure isn't, until one succeeds again. */
    private var saveFailing = false

    /**
     * What a save came to, told when it failed (the first time since one last succeeded: autosave
     * would say it again each time); false (nothing saved, nothing wrong) says nothing.
     */
    private suspend fun told(result: Result<Boolean>) {
        val e = result.exceptionOrNull()
        withContext(Dispatchers.Main) {
            when {
                e == null -> if (result.getOrDefault(false)) saveFailing = false
                !saveFailing -> {
                    saveFailing = true
                    message = io.github.artcraftandroid.procraft.core.saveFailureSaid(e.message.orEmpty())
                }
            }
        }
    }

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

    /** Colours painted with recently (0xRRGGBB), newest first, as the shared core keeps them. */
    var history by mutableStateOf<List<Int>>(emptyList())
        private set

    /** The colour history, kept by the shared core across artworks: each canvas's paint strokes add to it. */
    private val colourHistory = io.github.artcraftandroid.procraft.core.ColourHistory()

    /** Whether the size and opacity side bar shows. */
    var showSideBar by mutableStateOf(true)

    private val interfacePrefs = app.getSharedPreferences("interface", android.content.Context.MODE_PRIVATE)

    /** Whether a mask is current, the colour then the one masks paint with. */
    private var onMask = false

    /**
     * The colour not in use (hue, saturation, brightness): masks' (black at first) while a layer
     * of another kind is current, the brush colour while a mask is.
     */
    private var otherColour = floatArrayOf(0f, 0f, 0f)

    init {
        // the colours as they were last left
        hue = interfacePrefs.getFloat(KEPT_HUE, DEFAULT_HUE).coerceIn(0f, 1f)
        saturation = interfacePrefs.getFloat(KEPT_SATURATION, 1f).coerceIn(0f, 1f)
        brightness = interfacePrefs.getFloat(KEPT_BRIGHTNESS, 1f).coerceIn(0f, 1f)
        otherColour = listOf(KEPT_MASK_HUE, KEPT_MASK_SATURATION, KEPT_MASK_BRIGHTNESS).map { interfacePrefs.getFloat(it, 0f).coerceIn(0f, 1f) }.toFloatArray()
    }

    /** Keeps the brush colour and the masks' for the next session. */
    private fun keepColour() {
        val now = floatArrayOf(hue, saturation, brightness)
        val (brush, mask) = if (onMask) otherColour to now else now to otherColour
        interfacePrefs.edit()
            .putFloat(KEPT_HUE, brush[0]).putFloat(KEPT_SATURATION, brush[1]).putFloat(KEPT_BRIGHTNESS, brush[2])
            .putFloat(KEPT_MASK_HUE, mask[0]).putFloat(KEPT_MASK_SATURATION, mask[1]).putFloat(KEPT_MASK_BRIGHTNESS, mask[2])
            .apply()
    }

    /**
     * A mask became current, or stopped being: the colour trades places with the other kind's, as
     * masks paint with a colour of their own.
     */
    private fun followMaskColour() {
        val mask = layers.firstOrNull { it.current }?.kind == LayerType.MASK
        if (mask == onMask) return
        onMask = mask
        val now = floatArrayOf(hue, saturation, brightness)
        setColor(otherColour[0], otherColour[1], otherColour[2])
        otherColour = now
    }

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

    /** The brush's own shape for the cursor: loops of x, y in brush diameters (none: the round tip's circle). */
    var cursorShape by mutableStateOf<List<List<Float>>>(emptyList())
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
        // sized as it shows, and while hovering again now and then: the size may have changed
        // meanwhile (painting, it keeps the size the stroke began at)
        val now = android.os.SystemClock.uptimeMillis()
        if (cursorAt == null || (hovering && now - cursorReadAt >= CURSOR_REREAD_MS)) session?.let { s ->
            cursorRadius = s.info().let { it.brushDiameter * it.zoom / 2f }
            cursorOutline = s.currentBrushSetting("cursor_outline").roundToInt()
            cursorShape = s.brushOutline()
            cursorReadAt = now
        }
        cursorAt = p
    }

    /** When the cursor's size was last read (uptime ms). */
    private var cursorReadAt = 0L

    /** The light interface: light grey panels and bars with dark text. */
    var lightInterface by mutableStateOf(interfacePrefs.getBoolean(LIGHT_INTERFACE, false).also { io.github.artcraftandroid.procraft.ui.Pc.light = it })
        private set

    fun changeLightInterface(on: Boolean) {
        lightInterface = on
        io.github.artcraftandroid.procraft.ui.Pc.light = on
        session?.setLightInterface(on)
        interfacePrefs.edit().putBoolean(LIGHT_INTERFACE, on).apply()
    }

    /** The patches and plugins, their values kept with the interface settings, and what their two pages draw. */
    val plugins = keptPlugins(app, interfacePrefs)
    var patchList by mutableStateOf(plugins.patches())
        private set
    var pluginList by mutableStateOf(plugins.plugins())
        private set

    /** How big the interface shows now, as a patch says (1: as measured). */
    var interfaceShown = 1f
        private set

    /** Applies or unapplies a patch: it goes on or off its hooks at once, and its values wait for it. */
    fun setPluginOn(plugin: io.github.artcraftandroid.procraft.core.PluginInfo, on: Boolean) {
        if (!plugins.setOn(plugin.id, on)) return
        interfacePrefs.edit().putBoolean(plugin.onKey, on).apply()
        pluginsChanged()
    }

    /** Words for someone who changed a setting that now comes as a patch, said once. */
    var movedNotice by mutableStateOf(plugins.takeMovedNotice())
        private set

    fun movedNoticeRead() {
        movedNotice = null
    }

    /** A plugin file waiting on the one screen that asks about it, and what it holds. */
    var pluginOffer by mutableStateOf<io.github.artcraftandroid.procraft.core.PluginOffer?>(null)
        private set
    private var offeredPlugin: ByteArray? = null

    /** Whether [uri] is a plugin file, by its name. */
    fun isPluginFile(uri: android.net.Uri) = fileName(uri)?.endsWith(".procraftplugin", ignoreCase = true) == true

    /** Asks about the plugin file at [uri]: nothing in it runs, and nothing is kept until it is accepted. What can't be imported says why. */
    fun offerPlugin(uri: android.net.Uri) {
        viewModelScope.launch {
            val most = io.github.artcraftandroid.procraft.core.pluginFileMostBytes().toLong() + 1
            val bytes = withContext(Dispatchers.IO) {
                runCatching {
                    getApplication<Application>().contentResolver.openInputStream(uri)?.use { input ->
                        // no further than a file that can be taken, whatever it is
                        val out = java.io.ByteArrayOutputStream()
                        val chunk = ByteArray(64 * 1024)
                        while (out.size() < most) {
                            val read = input.read(chunk)
                            if (read < 0) break
                            out.write(chunk, 0, read)
                        }
                        out.toByteArray()
                    }
                }.getOrNull()
            }
            if (bytes == null) {
                message = "This file can't be read."
                return@launch
            }
            val offer = plugins.inspect(bytes)
            if (offer.refused != null) {
                message = offer.refused
                return@launch
            }
            offeredPlugin = bytes
            pluginOffer = offer
        }
    }

    fun declinePlugin() {
        offeredPlugin = null
        pluginOffer = null
    }

    /** Imports the file asked about; it shows at once. */
    fun acceptPlugin() {
        val bytes = offeredPlugin
        declinePlugin()
        message = bytes?.let(plugins::install)
        pluginsChanged()
    }

    /** Removes an imported plugin, its folder and its values; it goes at once. */
    fun removePlugin(plugin: io.github.artcraftandroid.procraft.core.PluginInfo) {
        val forget = plugins.remove(plugin.id)
        if (forget == null) {
            message = "This plugin couldn't be removed."
            return
        }
        val edit = interfacePrefs.edit()
        forget.forEach(edit::remove)
        edit.apply()
        pluginsChanged()
    }

    /** A plugin's switch flipped; a key no control has changes nothing. */
    fun setPluginSwitch(key: String, on: Boolean) {
        if (!plugins.setSwitch(key, on)) return
        interfacePrefs.edit().putBoolean(key, on).apply()
        pluginsChanged()
    }

    /** A plugin's slider let go at [value]; a key no control has changes nothing. A count stays kept as a whole number. */
    fun setPluginSlider(slider: io.github.artcraftandroid.procraft.core.PluginSlider, value: Float) {
        val landed = plugins.setSlider(slider.key, value) ?: return
        val edit = interfacePrefs.edit()
        if (io.github.artcraftandroid.procraft.core.pluginSliderWhole(slider)) edit.putInt(slider.key, landed.roundToInt()) else edit.putFloat(slider.key, landed)
        edit.apply()
        pluginsChanged()
    }

    /** The two lists as they stand: a patch may have stopped on a stroke since they were read. */
    fun readPlugins() {
        patchList = plugins.patches()
        pluginList = plugins.plugins()
    }

    /** What the patches and extras do now; a plugin nothing answers to does nothing. */
    private fun pluginsChanged() {
        readPlugins()
        session?.let(::applyPlugins)
    }

    /** [s] goes by the plugins that reach a canvas. */
    private fun applyPlugins(s: CanvasSession) {
        s.usePatches(plugins)
        s.setFillGap(plugins.fillGap())
    }

    /** The interface shows at [size] now: what hit-tests its handles, and what clears its bars, go with it. */
    fun interfaceShownAt(size: Float) {
        if (size == interfaceShown) return
        interfaceShown = size
        session?.setInterfaceSize(size)
        applyFitInsets()
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
    var inputTipAttached by mutableStateOf(interfacePrefs.getBoolean(INPUT_TIP_ATTACHED, true))
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
        tipAttached: Boolean = inputTipAttached,
    ) {
        inputStabilization = stabilization
        inputMotionFiltering = motion
        inputMotionExpression = expression
        inputPressureSmoothing = smoothing
        inputCurve = curve
        inputTipAttached = tipAttached
        interfacePrefs.edit()
            .putFloat(INPUT_STABILIZATION, stabilization).putFloat(INPUT_MOTION, motion)
            .putFloat(INPUT_EXPRESSION, expression).putFloat(INPUT_PRESSURE_SMOOTHING, smoothing)
            .putString(INPUT_CURVE, curve.joinToString(",") { "${it.x},${it.y}" })
            .putBoolean(INPUT_TIP_ATTACHED, tipAttached)
            .apply()
        applyInput()
    }

    fun resetInput() = changeInput(0f, 0f, 0f, 0f, listOf(Offset(0f, 0f), Offset(1f, 1f)), true)

    private fun applyInput() {
        session?.setInputSettings(inputStabilization, inputMotionFiltering, inputMotionExpression, inputPressureSmoothing, inputTipAttached, inputCurve.flatMap { listOf(it.x, it.y) })
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

    /** The modify button was pressed: the next touch on the canvas picks a colour (the touches know too). */
    private var pickNextShown by mutableStateOf(false)
    var pickNext: Boolean
        get() = pickNextShown
        set(value) {
            pickNextShown = value
            session?.setPickNext(value)
        }

    /** Transform is open (with a box when the layer holds anything). */
    var transforming by mutableStateOf(false)
        private set
    var transformBox by mutableStateOf<TransformBox?>(null)
        private set
    private val transformPrefs = app.getSharedPreferences("transform", android.content.Context.MODE_PRIVATE)
    /** The brushes in use and what has shipped, kept between artworks and launches. */
    private val brushPrefs = app.getSharedPreferences("brushes", android.content.Context.MODE_PRIVATE)
    /** The brush library as edited, in a file: brought-in brushes' images make it too big for preferences. */
    private val brushesDir = File(app.filesDir, "brushes")
    private val brushLibraryFile = File(brushesDir, "library.json")
    /** Writes of the library, one at a time. */
    private val brushLibraryWrites = Mutex()
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
                // after the feathering still on its way
                session?.let { s -> pourNow { s.selectFeatherDone() } }
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

    /** The plugin's filter open, for its panel: its name and its controls. */
    var pluginFilter by mutableStateOf<io.github.artcraftandroid.procraft.core.PluginFilterShown?>(null)
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
        private set
    var liquifyPressure by mutableFloatStateOf(1f)
        private set
    var liquifyDistortion by mutableFloatStateOf(0f)
        private set
    var liquifyMomentum by mutableFloatStateOf(0.8f)
        private set
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
    /** Cloning: where the source is (canvas pixels). */
    var clonePuck by mutableStateOf<Offset?>(null)
        private set
    /** The canvas window's size (pixels) and the screen's density, for what starts in its middle. */
    var windowSize = Offset.Zero
    var screenDensity = 2f

    /** Pixels a dp of the interface. */
    val density get() = screenDensity * interfaceShown

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
    /** The gradient library, kept by the shared core (crates/core gradients.rs), each change written at once. */
    private val gradientLibrary = io.github.artcraftandroid.procraft.core.Gradients.open(File(app.filesDir, "gradients.json").path)
    /** The gradient library: ours, and those made or changed here. */
    var gradients by mutableStateOf(readGradients())
        private set

    private fun readGradients(): List<GradientPreset> = gradientLibrary.list().map { g ->
        GradientPreset(g.name, g.stops.map { it.position to (0xFF000000 or it.rgb.toLong()) })
    }
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
    /** Perspective blur: positional or directional, and (on the layer) its puck and a point along its arrow (canvas pixels). */
    var perspectiveKind by mutableStateOf(PerspectiveKind.POSITIONAL)
        private set
    var perspectivePuck by mutableStateOf<Pair<Offset, Offset>?>(null)
        private set

    /** The node held on the transform box and where the touch is (window pixels), while a drag lasts. */
    var transformHeld by mutableStateOf<Pair<TransformHandle, Offset>?>(null)
    /** The rotate or box-turning node just let go where the touch left it, springing back. */
    var transformLetGo by mutableStateOf<Pair<TransformHandle, Offset>?>(null)

    /** The view (canvas pixels to window pixels) as [a, b, c, d, e, f], read while something is drawn over the canvas. */
    var viewMatrix by mutableStateOf(listOf(1f, 0f, 0f, 1f, 0f, 0f))
        private set

    private var pickJob: Job? = null
    private var pendingPick: Pair<Float, Float>? = null

    // the palettes, kept by the shared core
    private val paletteStore = io.github.artcraftandroid.procraft.core.Palettes.open(File(app.filesDir, "palettes.json").path)
    var palettes by mutableStateOf<List<Palette>>(emptyList())
        private set

    /** The palette shown under the history in the other colour modes. */
    var defaultPalette by mutableStateOf(0)
        private set

    init {
        refreshPalettes()
        // colour profiles the user imported
        io.github.artcraftandroid.procraft.core.setProfileFolder(File(app.filesDir, "icc").path)
        // layers may take half the device's memory, their textures a quarter: past that, those not
        // worked on are kept compressed off the GPU until needed (set before Custom Canvas asks)
        val memory = android.app.ActivityManager.MemoryInfo()
        app.getSystemService(android.app.ActivityManager::class.java)?.getMemoryInfo(memory)
        if (memory.totalMem > 0) {
            io.github.artcraftandroid.procraft.core.setLayerBudget((memory.totalMem / 2).toULong())
            io.github.artcraftandroid.procraft.core.setResidentBudget((memory.totalMem / 4).toULong())
        }
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

    /** The colour panel floating, dragged out by its grabber: its window's top-left (window pixels). */
    var floatingColorsAt by mutableStateOf<androidx.compose.ui.geometry.Offset?>(null)

    /** The colour panel's grabber is being dragged out: the panel stays, unseen, until let go. */
    var colorsUndocking by mutableStateOf(false)

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
    /** The pixels' version each thumbnail was read at: it is read again only once its layer's is new. */
    private val thumbnailVersions = HashMap<ULong, ULong>()

    var brushSets by mutableStateOf<List<BrushSetInfo>>(emptyList())
        private set
    var currentBrush by mutableStateOf<BrushInfo?>(null)
        private set
    var shownSet by mutableStateOf(0)
        private set
    /** The Recent set: the pinned brushes, then those last painted with, the latest first. */
    var recentBrushes by mutableStateOf<List<RecentBrush>>(emptyList())
        private set
    /** A brush found from the Recent set ("set-index"), which its own set scrolls to. */
    var foundBrush by mutableStateOf<String?>(null)
    val previews = mutableStateMapOf<String, ImageBitmap>()

    /** Counts the library's changes: a sample stroke asked for before the last one isn't kept. */
    var previewRound by androidx.compose.runtime.mutableIntStateOf(0)
        private set

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
    private var quickShapeEditingShown by mutableStateOf(false)
    var quickShapeEditing: Boolean
        get() = quickShapeEditingShown
        set(value) {
            quickShapeEditingShown = value
            session?.setShapeEditing(value)
        }

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
    /** Where the pad's Drawing Pad button's middle is across the window (pixels): its menu opens under it. */
    var padButtonCentre by mutableStateOf(0f)
    val blendModeList: List<BlendModeInfo> = blendModes()
    /** The modes a brush and its burnt edges blend by, as the studio lists them (the layers' and behind). */
    val brushBlendModeList: List<BlendModeInfo> = brushBlendModes()
    /** The modes a brush's grain blends by, as the studio lists them. */
    val grainBlendModeList: List<BlendModeInfo> = grainBlendModes()
    val combineBlendModeList: List<BlendModeInfo> = io.github.artcraftandroid.procraft.core.combineBlendModes()

    init {
        refreshGallery()
        // the GPU made while the gallery shows, so the first artwork opens at once
        viewModelScope.launch(Dispatchers.Default) { io.github.artcraftandroid.procraft.core.warmGpu() }
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
        // the home screen's widget shows the artwork worked on last
        viewModelScope.launch(Dispatchers.IO) { io.github.artcraftandroid.procraft.gallery.RecentArtworkWidget.refresh(getApplication<Application>()) }
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

    private fun readPresets(): List<CanvasPreset> = CanvasPreset.fromJson(presetPrefs.getString(PRESETS, null))

    private fun savePresets(list: List<CanvasPreset>) {
        canvasPresets = list
        presetPrefs.edit().putString(PRESETS, CanvasPreset.toJson(list)).apply()
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
        if (opening || session != null) return
        opening = true
        val rgb = listOf(((p.background shr 16) and 255) / 255f, ((p.background shr 8) and 255) / 255f, (p.background and 255) / 255f)
        viewModelScope.launch {
            val s = withContext(Dispatchers.IO) {
                CanvasSession.newCustom(p.width.toUInt(), p.height.toUInt(), p.dpi, rgb, !p.backgroundHidden, "Untitled Artwork").also {
                    it.setNewProfile(p.profile)
                    loadBrushes(it)
                }
            }
            opening = false
            start(s, store.newFolder(galleryDir), brushesLoaded = true)
            timeLapse?.settings = p.timeLapse
            startedNew()
        }
    }

    /** Says the canvas holds no more layers, and how many it holds. */
    private fun noRoom(): String {
        val (w, h) = canvasSize()
        return "This canvas holds at most ${layersFor(w, h)} layers."
    }

    /** The layers a canvas [width] × [height] holds on this device (the core's cap). */
    fun layersFor(width: Int, height: Int): Int =
        io.github.artcraftandroid.procraft.core.layersFor(width.coerceAtLeast(1).toUInt(), height.coerceAtLeast(1).toUInt())
            .coerceAtMost(Int.MAX_VALUE.toUInt()).toInt()

    /** A new artwork of `width` × `height` pixels (the screen's size), in Display P3, opened on the canvas. */
    fun newArtwork(width: Int, height: Int) {
        if (opening || session != null) return
        opening = true
        viewModelScope.launch {
            val s = withContext(Dispatchers.IO) {
                CanvasSession(width.coerceAtLeast(1).toUInt(), height.coerceAtLeast(1).toUInt()).also {
                    it.setNewProfile(io.github.artcraftandroid.procraft.gallery.CanvasPreset.P3)
                    loadBrushes(it)
                }
            }
            opening = false
            start(s, store.newFolder(galleryDir), brushesLoaded = true)
            startedNew()
        }
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
        viewModelScope.launch { pictureArtwork(uri) }
    }

    /** [newFromPicture], the artwork called [title] (an imported file's name). */
    private suspend fun pictureArtwork(uri: android.net.Uri, title: String = "Untitled Artwork") {
        // in Display P3 if the picture is, else in sRGB
        val p3 = withContext(Dispatchers.IO) { pictureIsP3(uri) }
        val image = withContext(Dispatchers.IO) { decodePicture(uri, p3) }
        if (image == null) {
            message = "This picture can't be opened."
            return
        }
        val s = withContext(Dispatchers.IO) {
            CanvasSession.fromImage(image, title).also {
                if (p3) it.setNewProfile(io.github.artcraftandroid.procraft.gallery.CanvasPreset.P3)
                loadBrushes(it)
            }
        }
        start(s, store.newFolder(galleryDir), brushesLoaded = true)
        startedNew()
    }

    /** The open artwork paints on a model: one finger turns it. */
    var modelOpen by mutableStateOf(false)
        private set

    /** The model's texture sets (by material), and the one painted on: its layers show. */
    var textureSets by mutableStateOf<List<String>>(emptyList())
        private set
    var textureSet by mutableStateOf(0)
        private set

    /** The texture set painted on shows flat, as a canvas, in place of the model (Show 2D texture). */
    var modelFlat by mutableStateOf(false)
        private set

    fun changeModelFlat(on: Boolean) {
        val s = session ?: return
        s.setModelFlat(on)
        modelFlat = s.modelFlat()
    }

    /** The Lighting Studio is open over the model (Actions > 3D > Edit lighting & environment). */
    var lightingStudioOpen by mutableStateOf(false)
        private set

    /** The environment the model is lit by (an asset in environments/), how bright, and whether it shows behind. */
    var modelEnvironment by mutableStateOf(DEFAULT_ENVIRONMENT)
        private set
    var modelExposure by mutableStateOf(1f)
        private set
    var modelEnvironmentShown by mutableStateOf(true)
        private set

    /** The lights the model is lit by, as the studio shows them. */
    var modelLights by mutableStateOf<List<io.github.artcraftandroid.procraft.core.ModelLight>>(emptyList())
        private set

    /** The lighting as the studio found it, put back by Cancel. */
    private class StudioKept(val environment: String, val exposure: Float, val shown: Boolean, val lights: List<io.github.artcraftandroid.procraft.core.ModelLight>)

    private var studioKept: StudioKept? = null

    fun openLightingStudio() {
        val s = session ?: return
        if (!modelOpen || modelFlat) return
        closePanels()
        modelLights = s.modelLights()
        studioKept = StudioKept(modelEnvironment, modelExposure, modelEnvironmentShown, modelLights)
        s.setLightingStudio(true)
        lightingStudioOpen = true
    }

    /** Leaves the studio, keeping what was changed or ([keep] false) putting the lighting back. */
    fun closeLightingStudio(keep: Boolean) {
        val s = session
        if (!keep) studioKept?.let { kept ->
            if (kept.environment != modelEnvironment) chooseEnvironment(kept.environment)
            changeExposure(kept.exposure)
            changeEnvironmentShown(kept.shown)
            s?.setModelLights(kept.lights)
        }
        s?.setLightingStudio(false)
        modelLights = s?.modelLights() ?: emptyList()
        studioKept = null
        lightingStudioOpen = false
    }

    /** Adds a light above the model; at the most there can be, says so instead. */
    fun addLight() {
        val s = session ?: return
        if (!s.addModelLight()) message = "A model can have four lights at most."
        modelLights = s.modelLights()
    }

    fun duplicateLight(index: Int) {
        val s = session ?: return
        if (!s.duplicateModelLight(index.toUInt())) message = "A model can have four lights at most."
        modelLights = s.modelLights()
    }

    fun deleteLight(index: Int) {
        val s = session ?: return
        s.deleteModelLight(index.toUInt())
        modelLights = s.modelLights()
    }

    /** Light [index] changed to [light] (its colour or intensity). */
    fun changeLight(index: Int, light: io.github.artcraftandroid.procraft.core.ModelLight) {
        val s = session ?: return
        s.setModelLight(index.toUInt(), light)
        modelLights = s.modelLights()
    }

    /** The light whose cube is under window point ([x], [y]) in the studio, if one is. */
    fun lightAt(x: Float, y: Float): Int? = session?.modelLightAt(x, y)?.toInt()

    /** Light [index] in the studio: its middle on the window and how far its cube reaches (pixels). */
    fun lightOnScreen(index: Int): List<Float>? = session?.modelLightOnScreen(index.toUInt())

    fun moveLight(index: Int, dx: Float, dy: Float) {
        val s = session ?: return
        s.moveModelLight(index.toUInt(), dx, dy)
        modelLights = s.modelLights()
    }

    /** A drag across the studio: its camera goes round the model and up or down. */
    fun studioTurn(dx: Float, dy: Float) {
        session?.studioTurn(dx, dy)
    }

    fun studioZoom(scale: Float) {
        session?.studioZoom(scale)
    }

    /** Lights the model by the environment [name] (an asset in environments/), read off the main thread. */
    fun chooseEnvironment(name: String) {
        val s = session ?: return
        modelEnvironment = name
        viewModelScope.launch {
            val assets = getApplication<Application>().assets
            val hdr = withContext(Dispatchers.IO) { runCatching { assets.open("environments/$name.hdr").use { it.readBytes() } }.getOrNull() }
            if (hdr != null && session === s && modelEnvironment == name) s.setModelEnvironment(name, hdr)
        }
    }

    fun changeExposure(exposure: Float) {
        val s = session ?: return
        s.setModelExposure(exposure)
        modelExposure = s.modelExposure()
    }

    fun changeEnvironmentShown(shown: Boolean) {
        val s = session ?: return
        s.setModelEnvironmentShown(shown)
        modelEnvironmentShown = s.modelEnvironmentShown()
    }

    /** One finger across the studio's view: the model turns. */
    fun turnModel(dx: Float, dy: Float) {
        session?.turnModel(dx, dy)
    }

    /** Strokes on the model reach every surface under them, not only the nearest. */
    var paintThrough by mutableStateOf(false)
        private set

    fun changePaintThrough(on: Boolean) {
        val s = session ?: return
        s.setPaintThrough(on)
        paintThrough = s.paintThrough()
    }

    /** The model's meshes, each in its texture set; which one is open in the layers panel; which are hidden; their pictures. */
    var submeshes by mutableStateOf<List<io.github.artcraftandroid.procraft.core.SubmeshInfo>>(emptyList())
        private set
    var openSubmesh by mutableStateOf<Int?>(null)
        private set
    var submeshesHidden by mutableStateOf<List<Boolean>>(emptyList())
        private set
    val submeshThumbnails = androidx.compose.runtime.mutableStateMapOf<Int, ImageBitmap>()

    /** Opens submesh [index] in the layers panel (its texture set's layers under it), or closes it again. */
    fun openSubmesh(index: Int) {
        if (openSubmesh == index) {
            openSubmesh = null
            return
        }
        val m = submeshes.getOrNull(index) ?: return
        openSubmesh = index
        chooseTextureSet(m.textureSet.toInt())
    }

    /** Hides submesh [index] (or shows it again), saying which. */
    fun toggleSubmeshHidden(index: Int) {
        val s = session ?: return
        val hide = submeshesHidden.getOrNull(index) != true
        s.setSubmeshHidden(index.toUInt(), hide)
        submeshesHidden = s.submeshesHidden()
        showNotice(if (hide) "Hide Submesh" else "Show Submesh")
    }

    /** Draws each submesh's picture again, as the model is seen now. */
    fun refreshSubmeshThumbnails() {
        val s = session ?: return
        for (i in submeshes.indices) {
            viewModelScope.launch {
                val image = withContext(Dispatchers.Default) {
                    val t = s.submeshThumbnail(i.toUInt(), THUMB_SIDE.toUInt()) ?: return@withContext null
                    val bitmap = io.github.artcraftandroid.procraft.ui.CanvasSpace.tag(Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), Bitmap.Config.ARGB_8888))
                    bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(t.rgba))
                    bitmap.asImageBitmap()
                }
                if (image != null && session === s) submeshThumbnails[i] = image
            }
        }
    }

    /** The channel painted on the model's texture set: 0 its colour, 1 roughness, 2 metalness. */
    var modelChannel by mutableStateOf(0)
        private set

    /** Each channel's own colour (hue, saturation, brightness), kept while another is painted. */
    private val channelColors = arrayOfNulls<FloatArray>(3)

    /** A channel's tile was chosen (blue, the layer's row not): painting goes to that channel of the current layer. */
    var channelTileChosen by mutableStateOf(false)
        private set

    /** Chooses a layer's channel tile: [channel] painted from now on (an undo step). */
    fun chooseChannelTile(channel: Int) {
        val s = session ?: return
        if (channel !in 0..2) return
        s.chooseChannelTile(channel.toUInt())
        followModelChannel()
    }

    /** A layer's row tapped on a model: the layer itself (its colour) is painted, no tile chosen (an undo step). */
    fun leaveChannelTiles() {
        val s = session ?: return
        s.leaveChannelTiles()
        followModelChannel()
    }

    /** The layer whose channels show under it in the layers panel, if one's do; and their pictures. */
    var channelTrayFor by mutableStateOf<ULong?>(null)
    val channelThumbnails = androidx.compose.runtime.mutableStateMapOf<Pair<Int, ULong>, ImageBitmap>()

    /**
     * Paints on [channel] of the texture set from now on (its layers stay the layers), with that
     * channel's own colour: roughness and metalness start black.
     */
    fun chooseChannel(channel: Int) {
        val s = session ?: return
        if (channel == modelChannel || channel !in 0..2) return
        s.setModelChannel(channel.toUInt())
        followModelChannel()
    }

    /** The channel and tile as the core has them now (a tile chosen, or its choice undone or redone), the channel's own colour shown. */
    private fun followModelChannel() {
        val s = session ?: return
        channelTileChosen = s.channelTileChosen()
        val now = s.modelChannel().toInt()
        if (now == modelChannel) return
        channelColors[modelChannel] = floatArrayOf(hue, saturation, brightness)
        modelChannel = now
        val c = channelColors[now] ?: if (now == 0) null else floatArrayOf(0f, 0f, 0f)
        c?.let { setColor(it[0], it[1], it[2]) }
        refresh()
        refreshThumbnails()
    }

    /** Shows layer [id]'s channels under it (or hides them), drawn afresh. */
    fun toggleChannelTray(id: ULong) {
        channelTrayFor = if (channelTrayFor == id) null else id
        if (channelTrayFor == id) refreshChannelThumbnails(id)
    }

    private fun refreshChannelThumbnails(id: ULong) {
        val s = session ?: return
        for (c in 0..2) {
            viewModelScope.launch {
                val image = withContext(Dispatchers.Default) {
                    val t = s.channelLayerThumbnail(c.toUInt(), id, THUMB_SIDE.toUInt()) ?: return@withContext null
                    val bitmap = io.github.artcraftandroid.procraft.ui.CanvasSpace.tag(Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), Bitmap.Config.ARGB_8888))
                    bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(t.rgba))
                    bitmap.asImageBitmap()
                }
                if (image != null && session === s) channelThumbnails[c to id] = image
            }
        }
    }

    /** Paints on texture set [index] from now on: the layers become its layers. */
    fun chooseTextureSet(index: Int) {
        val s = session ?: return
        if (index == textureSet || index !in textureSets.indices) return
        // a texture set is opened on its colour, no tile chosen
        chooseChannel(0)
        channelTrayFor = null
        s.chooseTextureSet(index.toUInt())
        channelTileChosen = s.channelTileChosen()
        textureSet = index
        refresh()
        refreshThumbnails()
    }

    /**
     * A new artwork painting on the model in the file [uri] (named [name]: OBJ, USDZ or USDC, or
     * a zip holding one with its files), with the files picked along with it ([picked], by name:
     * an OBJ's material file and images). [bytes] are the file's, if already read; the artwork
     * is named [title], else after the file.
     */
    private suspend fun newFromModel(
        uri: android.net.Uri,
        name: String,
        picked: List<Pair<android.net.Uri, String>> = emptyList(),
        bytes: ByteArray? = null,
        title: String? = null,
    ) {
        val resolver = getApplication<Application>().contentResolver
        val s = withContext(Dispatchers.IO) {
            runCatching {
                val file = bytes ?: resolver.openInputStream(uri)?.use { it.readBytes() } ?: return@runCatching null
                val files = picked.mapNotNull { (u, n) ->
                    runCatching { resolver.openInputStream(u)?.use { ModelFile(n, it.readBytes()) } }.getOrNull()
                }
                val artwork = (title ?: name.substringBeforeLast('.')).ifBlank { "Untitled Artwork" }
                CanvasSession.openModel(file, name, files, artwork, 0u)
                    .also(::loadModelTextures).also(::loadBrushes)
            }.onFailure { android.util.Log.e("ProCraft", "opening a model", it) }.getOrNull()
        }
        if (s == null) {
            message = "This model can't be opened."
            return
        }
        start(s, store.newFolder(galleryDir), brushesLoaded = true)
        startedNew()
    }

    /**
     * The images a model's file brings (a USDZ's), decoded and scaled to its texture sets' side
     * into their Base Layers by the core: colour as it is, roughness and metalness from the
     * component they name, as grey. An image that can't be read is left out.
     */
    private fun loadModelTextures(s: CanvasSession) {
        s.loadModelTextures()
    }

    /**
     * A file from elsewhere into the gallery, with the import card: an artwork file (opened once
     * in, as the gallery's Import does), or a picture as a new artwork.
     */
    fun importFile(uri: android.net.Uri) {
        viewModelScope.launch { importWithCard(uri, open = true) }
    }

    /**
     * [uri] brought in with the import card showing its name; the one artwork it made opened if
     * [open]. Cancel throws away what it made.
     */
    private suspend fun importWithCard(uri: android.net.Uri, open: Boolean) {
        if (importBusy || exportShown != null) return
        val name = withContext(Dispatchers.IO) { fileName(uri) } ?: "Untitled"
        val made = withImportCard(name) { importOne(uri) } ?: return
        if (open && session == null && made.size == 1) openImported(made.first())
    }

    /** An import under way as its card shows it: the file's [name], how far it has come (0..1), and once [done]. */
    data class ImportShown(val name: String, val progress: Float, val done: Boolean = false)

    /** The import card, while a file is brought in (null without one). */
    var importShown by mutableStateOf<ImportShown?>(null)
        private set

    @Volatile
    private var importCancelled = false

    /** An import is under way (its card may have gone with Cancel while what it made is thrown away). */
    private var importBusy = false

    /** The gallery folders the import under way has made. */
    private val importMade = java.util.Collections.synchronizedList(mutableListOf<File>())

    /** A new gallery folder for an artwork being imported, kept as the import's. */
    private fun importFolder(): File = store.newFolder(galleryDir).also { importMade += it }

    /** How far the import has come as its work tells it (the copy in), 0..1. */
    @Volatile
    private var importStep = 0f

    /** The import card's Cancel: the card goes at once, and what the import makes is thrown away. */
    fun cancelImport() {
        importCancelled = true
        importShown = null
    }

    /**
     * Runs [work], bringing in the file [name], with the import card: its pie follows the copy in,
     * creeps on while the file is read, and is full with a check once done (none if the work told
     * why it couldn't: its message says so). The artwork folders it made in the gallery, or null
     * if Cancel was tapped (they are thrown away; a picture or model opened as a new canvas
     * meanwhile stays).
     */
    private suspend fun withImportCard(name: String, work: suspend () -> Unit): List<File>? {
        importCancelled = false
        importBusy = true
        importMade.clear()
        importStep = 0f
        importShown = ImportShown(name, 0f)
        val told = message
        val following = viewModelScope.launch {
            var creep = 0f
            while (true) {
                creep += (IMPORT_CREEP_TO - creep) * IMPORT_CREEP
                if (!importCancelled) importShown = ImportShown(name, maxOf(importStep, creep))
                kotlinx.coroutines.delay(EXPORT_POLL_MS)
            }
        }
        try {
            work()
            following.cancel()
            val made = withContext(Dispatchers.IO) { importMade.toList().filter { it.isDirectory } }
            if (importCancelled) {
                withContext(Dispatchers.IO) { made.forEach { it.deleteRecursively() } }
                refreshGallery()
                return null
            }
            // full, then the check drawn in it before the card goes
            if (message == told) {
                importShown = ImportShown(name, 1f, done = true)
                kotlinx.coroutines.delay(EXPORT_DONE_MS)
            }
            return made
        } finally {
            following.cancel()
            importShown = null
            importBusy = false
        }
    }

    /** The artwork just imported into [dir] opened, the gallery zooming into its cell. */
    private suspend fun openImported(dir: File) {
        val entries = withContext(Dispatchers.IO) { store.list(galleryDir) }
        galleryEntries = entries
        artworks = entries.filterIsInstance<Artwork>()
        artworks.firstOrNull { it.dir.name == dir.name }?.let { openArtwork(it) }
    }

    /**
     * Files from elsewhere into the gallery, one after another as [importFile] takes each; a
     * model among them takes the rest as the files picked with it (an OBJ's material file and
     * the images it maps).
     */
    fun importFiles(uris: List<android.net.Uri>) {
        viewModelScope.launch {
            val names = withContext(Dispatchers.IO) { uris.map(::fileName) }
            val models = uris.indices.filter { i -> names[i]?.let(::isModelName) == true }
            // each one opened is kept (closed, so saved) as the next comes in
            if (models.isEmpty()) {
                for (uri in uris) {
                    if (session != null) closeArtwork()
                    importWithCard(uri, open = uris.size == 1)
                }
                return@launch
            }
            val picked = (uris.indices - models.toSet()).mapNotNull { i -> names[i]?.let { uris[i] to it } }
            for (i in models) {
                if (session != null) closeArtwork()
                newFromModel(uris[i], names[i] ?: continue, picked)
            }
        }
    }

    private fun isModelName(name: String) = MODEL_EXTENSIONS.any { name.endsWith(".$it", ignoreCase = true) }

    private suspend fun importOne(uri: android.net.Uri) {
        val resolver = getApplication<Application>().contentResolver
        val head = withContext(Dispatchers.IO) { runCatching { resolver.openInputStream(uri)?.use { s -> ByteArray(4).also { s.read(it) } } }.getOrNull() }
        val name = withContext(Dispatchers.IO) { fileName(uri) }
        // a file that can't be read at all (one the app isn't allowed to) is no picture either
        if (head == null) {
            message = "This file can't be opened."
            return
        }
        // another program's palette
        val foreign = head != null && String(head, Charsets.US_ASCII) == "ASEF" || name?.endsWith(".aco", ignoreCase = true) == true
        val zip = head != null && head[0] == 'P'.code.toByte() && head[1] == 'K'.code.toByte()
        // a zip is copied once and told by the names it holds, read from its end: a big artwork
        // isn't read into memory to find out what it is
        val zipFile = if (zip) withContext(Dispatchers.IO) { cacheCopy(uri) } else null
        if (zip && zipFile == null) {
            if (!importCancelled) message = "This file can't be opened."
            return
        }
        try {
            importOne(uri, head, name, foreign, zipFile)
        } finally {
            zipFile?.delete()
        }
    }

    /** [importOne] past the file's first bytes: [zipFile], a copy of a zip, read as what it holds. */
    private suspend fun importOne(uri: android.net.Uri, head: ByteArray?, name: String?, foreign: Boolean, zipFile: File?) {
        val resolver = getApplication<Application>().contentResolver
        val kind = zipFile?.let { f -> withContext(Dispatchers.IO) { io.github.artcraftandroid.procraft.core.zipFileKind(f.path) } }
        // the zip whole, read only for what needs all of it (brushes and a model's files)
        suspend fun zipBytes() = zipFile?.let { f -> withContext(Dispatchers.IO) { runCatching { f.readBytes() }.getOrNull() } }
        if (kind == io.github.artcraftandroid.procraft.core.ZipKind.Brushes) {
            val bytes = zipBytes()
            if (bytes == null || !addBrushes(bytes)) message = "This file has no brushes in it."
        } else if ((foreign || kind == io.github.artcraftandroid.procraft.core.ZipKind.Swatches) && importSwatches(uri)) {
            message = "Palette added."
        } else if (foreign) {
            message = "This palette can't be read."
        } else if (name != null && isModelName(name)) {
            newFromModel(uri, name)
        } else if (kind is io.github.artcraftandroid.procraft.core.ZipKind.Model) {
            // a model zipped with its material file and images (as a folder of them)
            val title = kind.name.substringAfterLast('/').substringBeforeLast('.')
            newFromModel(uri, if (name?.endsWith(".zip", ignoreCase = true) == true) name else "${name ?: "model"}.zip", bytes = zipBytes(), title = title)
        } else if (name != null && isOtherBrushFile(name)) {
            val bytes = zipBytes() ?: withContext(Dispatchers.IO) { runCatching { resolver.openInputStream(uri)?.use { it.readBytes() } }.getOrNull() }
            if (bytes == null || !addOtherBrushes(bytes, name)) message = "This file has no brushes in it."
        } else if (head != null && String(head, Charsets.ISO_8859_1) == "8BPS") {
            importPsd(uri, name)
        } else if (head != null && String(head, Charsets.ISO_8859_1) == "%PDF") {
            importPdf(uri, name)
        } else if (zipFile != null) {
            // an artwork file of ours, or a layered package
            val ok = withContext(Dispatchers.IO) {
                val dir = importFolder()
                val ours = runCatching { zipFile.inputStream().use { io.github.artcraftandroid.procraft.gallery.ArtworkFile.read(it, dir) } }.getOrNull() == true
                (ours || importPackage(zipFile, dir)).also { if (!it) dir.deleteRecursively() }
            }
            if (ok) refreshGallery() else message = "This file can't be opened."
        } else {
            // named after its file, as a picture opened on the computer is
            pictureArtwork(uri, name?.substringBeforeLast('.')?.trim()?.ifBlank { null } ?: "Untitled Artwork")
        }
    }

    /**
     * A PDF from [uri] made into a new artwork named after its file: each page a layer ("Page 1"
     * at the bottom), drawn on white at [PDF_DPI] (the long side kept within [PDF_MOST_SIDE]) and
     * fitted to the first page's size, with Page Assist on; a message says why if it can't be.
     */
    private suspend fun importPdf(uri: android.net.Uri, name: String?) {
        val app = getApplication<Application>()
        val outcome = withContext(Dispatchers.IO) {
            val pages = File(app.cacheDir, "pdf-${System.nanoTime()}").apply { mkdirs() }
            val dir = importFolder()
            try {
                app.contentResolver.openFileDescriptor(uri, "r")?.use { fd ->
                    android.graphics.pdf.PdfRenderer(fd).use { pdf ->
                        if (pdf.pageCount == 0) return@use io.github.artcraftandroid.procraft.core.PagesOutcome.Unreadable
                        // the canvas: the first page's size at the resolution, within the most side
                        val (w, h) = pdf.openPage(0).use { p ->
                            val scale = minOf(PDF_DPI / 72f, PDF_MOST_SIDE / maxOf(p.width, p.height).toFloat())
                            (p.width * scale).roundToInt().coerceAtLeast(1) to (p.height * scale).roundToInt().coerceAtLeast(1)
                        }
                        // more pages than a canvas this size holds as layers: none drawn, as none will be taken
                        val most = io.github.artcraftandroid.procraft.core.layersFor(w.toUInt(), h.toUInt())
                        if (pdf.pageCount.toUInt() > most) return@use io.github.artcraftandroid.procraft.core.PagesOutcome.TooManyPages(most)
                        val bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
                        for (i in 0 until pdf.pageCount) {
                            pdf.openPage(i).use { p ->
                                bitmap.eraseColor(android.graphics.Color.WHITE)
                                // each page fitted into the canvas, in its middle
                                val s = minOf(w / p.width.toFloat(), h / p.height.toFloat())
                                val m = android.graphics.Matrix().apply {
                                    postScale(s, s)
                                    postTranslate((w - p.width * s) / 2f, (h - p.height * s) / 2f)
                                }
                                p.render(bitmap, null, m, android.graphics.pdf.PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)
                            }
                            File(pages, "page-${i + 1}.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
                        }
                        bitmap.recycle()
                        io.github.artcraftandroid.procraft.core.importPages(
                            pages.path, pdf.pageCount.toUInt(), w.toUInt(), h.toUInt(), PDF_DPI, dir.path,
                            name?.substringBeforeLast('.')?.trim().orEmpty(),
                        )
                    }
                } ?: io.github.artcraftandroid.procraft.core.PagesOutcome.Unreadable
            } catch (e: Exception) {
                android.util.Log.e("ProCraft", "importing a PDF", e)
                io.github.artcraftandroid.procraft.core.PagesOutcome.Unreadable
            } finally {
                pages.deleteRecursively()
            }.also { if (it != io.github.artcraftandroid.procraft.core.PagesOutcome.Imported) dir.deleteRecursively() }
        }
        when (outcome) {
            io.github.artcraftandroid.procraft.core.PagesOutcome.Imported -> refreshGallery()
            is io.github.artcraftandroid.procraft.core.PagesOutcome.TooManyPages ->
                message = "This PDF has more pages than a canvas its size can hold as layers (${outcome.most})."
            io.github.artcraftandroid.procraft.core.PagesOutcome.Unreadable -> message = "This PDF can't be opened."
        }
    }

    /** A layered PSD from [uri] made into a new artwork, named after its file; a message says why if it can't be. */
    private suspend fun importPsd(uri: android.net.Uri, name: String?) {
        val app = getApplication<Application>()
        val outcome = withContext(Dispatchers.IO) {
            val temp = File(app.cacheDir, "import-${System.nanoTime()}.psd")
            val dir = importFolder()
            try {
                app.contentResolver.openInputStream(uri)?.use { input -> temp.outputStream().use { input.copyTo(it) } }
                io.github.artcraftandroid.procraft.core.importPsd(temp.path, dir.path, name?.substringBeforeLast('.')?.trim().orEmpty())
            } catch (e: Exception) {
                android.util.Log.e("ProCraft", "importing a PSD", e)
                io.github.artcraftandroid.procraft.core.PsdOutcome.Unreadable
            } finally {
                temp.delete()
            }.also { if (it != io.github.artcraftandroid.procraft.core.PsdOutcome.Imported) dir.deleteRecursively() }
        }
        when (outcome) {
            io.github.artcraftandroid.procraft.core.PsdOutcome.Imported -> refreshGallery()
            is io.github.artcraftandroid.procraft.core.PsdOutcome.TooManyLayers ->
                message = "This file has more layers than a canvas its size can hold (${outcome.most})."
            io.github.artcraftandroid.procraft.core.PsdOutcome.Unsupported -> message = "This kind of PSD can't be opened."
            io.github.artcraftandroid.procraft.core.PsdOutcome.Unreadable -> message = "This file can't be opened."
        }
    }

    /**
     * The file at [uri] copied whole into the cache (streamed through, never held in memory), or
     * none; the import card's pie follows it, and its Cancel stops it.
     */
    private fun cacheCopy(uri: android.net.Uri): File? {
        val temp = File(getApplication<Application>().cacheDir, "import-${System.nanoTime()}.zip")
        val resolver = getApplication<Application>().contentResolver
        val size = runCatching { resolver.openFileDescriptor(uri, "r")?.use { it.statSize } }.getOrNull() ?: -1L
        return runCatching {
            resolver.openInputStream(uri)?.use { input ->
                temp.outputStream().use { out ->
                    val buffer = ByteArray(1 shl 16)
                    var copied = 0L
                    while (true) {
                        if (importCancelled) error("cancelled")
                        val n = input.read(buffer)
                        if (n < 0) break
                        out.write(buffer, 0, n)
                        copied += n
                        if (size > 0) importStep = IMPORT_COPY_SHARE * copied / size
                    }
                }
            } ?: error("nothing to read")
            temp
        }.getOrElse {
            if (!importCancelled) android.util.Log.e("ProCraft", "copying a file in", it)
            temp.delete()
            null
        }
    }

    /**
     * The layered package in [source] (a copy in the cache) made into the artwork folder [dir]: its
     * layers, settings and preview (as the gallery's thumbnail until it is opened); false if it isn't one.
     */
    private fun importPackage(source: File, dir: File): Boolean {
        return try {
            val s = runCatching { CanvasSession.openPackage(source.path) }.getOrNull() ?: return false
            s.save(dir.path)
            // its time-lapse: the settings, and its videos as they are
            val segments = File(getApplication<Application>().cacheDir, "segments-${System.nanoTime()}")
            try {
                runCatching { io.github.artcraftandroid.procraft.core.packageTimeLapse(source.path, segments.path) }
                    .getOrNull()?.let { TimeLapse(dir).take(it) }
            } finally {
                segments.deleteRecursively()
            }
            java.util.zip.ZipFile(source).use { zip ->
                zip.getEntry("QuickLook/Thumbnail.png")?.let { e ->
                    zip.getInputStream(e).use { input -> File(dir, "thumbnail.png").outputStream().use { input.copyTo(it) } }
                }
            }
            true
        } catch (e: Exception) {
            android.util.Log.e("ProCraft", "importing a package", e)
            false
        }
    }

    /** The open artwork as a layered package in the cache, to share, named with [extension] (ours or the other app's). */
    suspend fun exportPackageFile(extension: String = OWN_EXTENSION): File? = withExportCard("The artwork couldn't be exported.") { s ->
        withContext(Dispatchers.IO) {
            runCatching {
                val base = io.github.artcraftandroid.procraft.core.exportFileName(s.name(), true)
                val out = File(File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }, "$base.$extension")
                s.exportPackage(out.path, System.nanoTime().toULong(), timeLapse?.forPackage())
                out
            }.onFailure { android.util.Log.e("ProCraft", "exporting a package", it) }.getOrNull()
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
     * the open artwork's canvas has drawn a frame, or a few seconds have passed; if the canvas can't
     * draw (the device's GPU couldn't make it), says so at once.
     */
    suspend fun awaitCanvasDrawn() {
        val s = session ?: return
        // asked apart from the wait: a call into the session blocks while the first frame holds it,
        // so the limit is the clock's, not the sum of the pauses between calls
        val asking = viewModelScope.async(Dispatchers.IO) {
            var failed: Boolean? = null
            while (failed == null) {
                val info = runCatching { s.info() }.getOrNull()
                failed = when {
                    info == null -> false
                    info.error != null -> true
                    info.frames != 0uL -> false
                    else -> null
                }
                if (failed == null) kotlinx.coroutines.delay(CANVAS_POLL_MS)
            }
            failed
        }
        val failed = kotlinx.coroutines.withTimeoutOrNull(CANVAS_DRAWN_LIMIT_MS) { asking.await() }
        if (failed == null) asking.cancel()
        if (failed == true && session === s) message = "This device's graphics can't draw the canvas."
    }

    /**
     * The home screen's Recent artwork widget was tapped: the artwork worked on last opens, once the window
     * has its size (the app may only now be starting); nothing if one is already open.
     */
    fun openMostRecent() {
        if (opening || session != null) return
        viewModelScope.launch {
            var waited = 0L
            while (windowPx.width <= 1 && waited < WINDOW_WAIT_MS) {
                kotlinx.coroutines.delay(CANVAS_POLL_MS)
                waited += CANVAS_POLL_MS
            }
            val a = withContext(Dispatchers.IO) { store.mostRecent() } ?: return@launch
            openArtwork(a)
        }
    }

    /** Opens artwork [a], the gallery zooming into its cell; then [then]. */
    fun openArtwork(a: Artwork, then: (() -> Unit)? = null) {
        // one artwork open at a time: a second session on it would leave the screen on the first
        if (opening || session != null) return
        opening = true
        val cell = thumbAt[a.dir.name]
        viewModelScope.launch {
            val s = withContext(Dispatchers.IO) { runCatching { CanvasSession.open(a.dir.path).also(::loadBrushes) } }
            opening = false
            s.onSuccess {
                if (!orientationMemory) it.forgetSavedView()
                val m = it.openingViewMatrix(windowPx.width.toUInt(), windowPx.height.toUInt())
                galleryZoom = cell?.let { c -> GalleryZoom(c, canvasRect(it, m), opening = true) }
                start(it, a.dir, brushesLoaded = true)
                then?.invoke()
            }.onFailure { message = "This artwork can't be opened." }
        }
    }

    /** How many of the gallery's artworks being shared are written, as a share of them. */
    @Volatile
    private var galleryShared = 0f

    /**
     * Artworks from the gallery written as [format] (as [shareableFiles]) with the export card, its
     * pie filling an artwork at a time; its Cancel stops after the artwork being written. Null if
     * cancelled or none could be written (that is told).
     */
    suspend fun shareFromGallery(list: List<Artwork>, format: String, pdfQuality: Int = 100): Pair<List<File>, String>? {
        galleryShared = 0f
        return withCard("The artwork couldn't be exported.", { galleryShared }) {
            shareableFiles(list, format, pdfQuality) { written -> galleryShared = written / list.size.coerceAtLeast(1).toFloat() }
                .takeIf { it.first.isNotEmpty() }
        }
    }

    /**
     * Artworks from the gallery written as [format] (one of [SHARE_FORMATS] but the animated ones)
     * into the cache, to share; with the type they are. [written] hears how many are done after
     * each; a cancelled export stops before the next.
     */
    suspend fun shareableFiles(list: List<Artwork>, format: String, pdfQuality: Int = 100, written: (Int) -> Unit = {}): Pair<List<File>, String> = withContext(Dispatchers.IO) {
        val (extension, mime) = when (format) {
            "PSD" -> "psd" to "image/vnd.adobe.photoshop"
            "PDF" -> "pdf" to "application/pdf"
            "JPEG" -> "jpg" to "image/jpeg"
            "PNG" -> "png" to "image/png"
            "TIFF" -> "tif" to "image/tiff"
            PACKAGE_LABEL -> PACKAGE_EXTENSION to "application/octet-stream"
            else -> OWN_EXTENSION to "application/octet-stream"
        }
        val dir = File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }
        val used = mutableSetOf<String>()
        val files = mutableListOf<File>()
        for ((i, a) in list.withIndex()) {
            if (exportCancelled) break
            val base = io.github.artcraftandroid.procraft.core.exportFileName(a.name, false)
            var name = base
            var n = 2
            while (!used.add(name)) name = "$base ${n++}"
            File(dir, "$name.$extension").takeIf { io.github.artcraftandroid.procraft.gallery.exportArtwork(a.dir, format, it, pdfQuality) }?.let(files::add)
            written(i + 1)
        }
        files to mime
    }

    /**
     * Opens artwork [a] to set up its animated export as [kind], going back to the gallery after;
     * the export shows once the canvas is made, as its frames and file are read from it.
     */
    fun openForAnimatedExport(a: Artwork, kind: Animated) {
        openArtwork(a) {
            exportingFromGallery = true
            val s = session ?: return@openArtwork
            viewModelScope.launch {
                while (session === s && !s.canvasMade()) kotlinx.coroutines.delay(16)
                if (session === s) animatedExport = kind
            }
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

    /** [e] dragged to [to] in the gallery or stack showing (its place in the list without it), out of its stack if it was in one. */
    fun moveInGallery(e: io.github.artcraftandroid.procraft.gallery.GalleryEntry, to: Int) {
        // at once, so the cells move as the finger lifts
        val shown = galleryEntries.filter { it.dir != e.dir }.toMutableList()
        shown.add(to.coerceIn(0, shown.size), e)
        galleryEntries = shown
        artworks = shown.filterIsInstance<Artwork>()
        viewModelScope.launch {
            val into = galleryDir
            withContext(Dispatchers.IO) { store.move(e, to, into) }
            refreshGallery()
        }
    }

    /** Artwork [a] dropped onto [onto] in the gallery: with an artwork it makes a stack, [a] first; a stack takes it in first. */
    fun dropOnto(a: Artwork, onto: io.github.artcraftandroid.procraft.gallery.GalleryEntry) {
        if (openStack != null || a.dir == onto.dir) return
        viewModelScope.launch {
            withContext(Dispatchers.IO) { store.putOnto(a, onto) }
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
        endFilling()
        endLayerOpacity()
        endTransform(true)
        endAdjustment(true)
        val s = session ?: return
        noteTime()
        keepChoices(s)
        s.detachProjection()
        val dir = artworkDir
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
        keepColour()
        val lapse = timeLapse
        session = null
        artworkDir = null
        panel = Panel.NONE
        screen = Screen.GALLERY
        closingSaves.incrementAndGet()
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    // after the autosave's save under way, if one is
                    s.stopAutosave()
                    dir?.let { told(runCatching { s.save(it.path) }.map { true }) }
                    // its memory free at once, not when the object is collected
                    s.releaseCanvas()
                    // the frames it recorded made into one more video, their pictures then gone, as
                    // the artwork file keeps a time-lapse
                    lapse?.let { runCatching { it.forPackage() }; it.close() }
                }
            } finally {
                closingSaves.decrementAndGet()
            }
            galleryVersion++
            refreshGallery()
        }
    }

    /** Artworks left that are still being saved. */
    private val closingSaves = java.util.concurrent.atomic.AtomicInteger(0)

    /** Gallery Recovery, once what is being saved is kept: artworks the gallery can't show stacked as Recovered Artworks. */
    suspend fun recoverLostArtworks() {
        while (closingSaves.get() > 0) kotlinx.coroutines.delay(100)
        withContext(Dispatchers.IO) { store.recoverLost() }
        galleryVersion++
        refreshGallery()
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
     * The pen's side button did what the shared touches set it to: the tool, the eyedropper and
     * drawing assist read again as they now are.
     */
    fun penButtonDone() {
        val s = session ?: return
        pickNextShown = s.pickNext()
        val now = s.info().tool
        if (now != tool) {
            feedback()
            endRecolor()
            tool = now
            readSliders()
            currentBrush = s.currentBrush()
        }
        guide = s.guide()
    }

    /** The canvas view's ear for a pen's side button that comes as a key (true: the second). */
    var penKeyHeard: ((second: Boolean) -> Unit)? = null

    /** A pen's side button came as a key (see [penKeyButton]): true if a canvas took it. */
    fun penKey(second: Boolean): Boolean {
        if (screen != Screen.CANVAS || session == null) return false
        val heard = penKeyHeard ?: return false
        heard(second)
        return true
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
        session?.setModifyHeld(true)
    }

    /** □ let go: a short press nothing used is a tap ([cancelled] when the finger slid off). */
    fun modifyReleased(cancelled: Boolean) {
        modifyHeld = false
        // a touch the canvas's touches took meanwhile used it too
        if (session?.setModifyHeld(false) == true) modifyUsed = true
        if (!cancelled && !modifyUsed && android.os.SystemClock.uptimeMillis() - modifyDownAt < MODIFY_TAP_MS) tapModify()
    }

    /** The gesture settings as the canvas's touches (in the shared core) take them. */
    fun touchSettings(): io.github.artcraftandroid.procraft.core.TouchSettings = gestures.touchSettings(rapidUndoDelay * 1000.0)

    /** The middle of the screen (window pixels), for menus a button opens. */
    var screenCentre = Offset(800f, 1280f)

    /** Gesture controls is open. */
    var gestureControlsOpen by mutableStateOf(false)

    /** Where the middle of one of the surround grid's lines falls (window pixels), for screens drawn over the canvas. */
    var gridOrigin by mutableStateOf<androidx.compose.ui.geometry.Offset?>(null)

    /** The page Gesture controls was last on: it opens there again. */
    var gesturePage by mutableStateOf(GesturePurpose.EYEDROPPER)

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

    /** The time-lapse's video settings, kept with the artwork. */
    fun timeLapseSettings(): TimeLapseSettings = timeLapse?.settings ?: TimeLapseSettings()

    /** Renames the artwork and sets its author, from Canvas information. */
    fun changeArtworkInfo(name: String, author: String) {
        val s = session ?: return
        if (name.isNotBlank() && name.trim() != s.name()) s.setName(name.trim())
        s.setAuthor(author)
    }

    /**
     * Keeps the author's photo (the picture at [photo], when one was picked) and the [signature]
     * (strokes of x, y across its box) in the artwork's folder, off the main thread; each takes
     * the old one's place only once written whole, and a notice says which couldn't be kept.
     */
    fun keepAbout(photo: android.net.Uri?, signature: List<List<Float>>) {
        val dir = artworkDir ?: return
        viewModelScope.launch {
            val failed = withContext(Dispatchers.IO) {
                val resolver = getApplication<Application>().contentResolver
                val photoKept = photo == null || runCatching {
                    val bytes = resolver.openInputStream(photo)?.use { it.readBytes() } ?: error("no photo")
                    io.github.artcraftandroid.procraft.core.writePhoto(dir.path, bytes)
                }.isSuccess
                val signed = runCatching { io.github.artcraftandroid.procraft.core.writeSignature(dir.path, signature) }.isSuccess
                when {
                    !photoKept -> "The photo couldn't be kept."
                    !signed -> "The signature couldn't be kept."
                    else -> null
                }
            }
            failed?.let { showNotice(it, warning = true) }
        }
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
        keepColour()
        val s = session ?: return
        val dir = artworkDir ?: return
        noteTime()
        keepChoices(s)
        viewModelScope.launch(Dispatchers.IO) {
            told(runCatching { s.save(dir.path) }.map { true })
            // its new picture on the home screen's widget
            io.github.artcraftandroid.procraft.gallery.RecentArtworkWidget.refresh(getApplication<Application>())
        }
    }

    /**
     * Gives [s] the app's brushes as they were kept: the library (else the edits earlier versions
     * kept), brushes shipped since joining it once, and the brushes in use. Reading the library
     * takes half a second, so callers off the main thread do this before [start].
     */
    private fun loadBrushes(s: CanvasSession) {
        // earlier versions kept the library, what had shipped and the brushes in use in preferences:
        // moved into the brushes' folder once
        if (!brushLibraryFile.exists()) {
            val kept = brushPrefs.getString(BRUSH_LIBRARY, null)
            if (kept != null && s.loadBrushLibrary(kept) && s.saveBrushLibraryFile(brushLibraryFile.path)) {
                brushPrefs.edit().remove(BRUSH_LIBRARY).apply()
            }
        }
        if (brushPrefs.contains(BRUSHES_SEEN) || brushPrefs.contains(BRUSH_CHOICES)) {
            val seen = brushPrefs.getString(BRUSHES_SEEN, null)
            val choices = brushPrefs.getString(BRUSH_CHOICES, null)
            if (io.github.artcraftandroid.procraft.core.adoptKeptBrushes(brushesDir.path, seen, choices)) {
                brushPrefs.edit().remove(BRUSHES_SEEN).remove(BRUSH_CHOICES).apply()
            }
        }
        // the library as kept, with what shipped since joining it, and the brushes in use as left;
        // else edits kept by versions before the library was
        if (!s.openBrushes(brushesDir.path)) brushPrefs.getString(BRUSH_CHANGES, null)?.let { s.applyBrushChanges(it) }
    }

    private fun start(s: CanvasSession, dir: File, brushesLoaded: Boolean = false) {
        // Gradient Map maps by the library
        s.useGradients(gradientLibrary)
        // the canvas's colours are P3 or sRGB numbers: the colour panel and swatches follow
        refreshCanvasSpace(s)
        s.setSelectionVisibility(selectionVisibility)
        s.setBrushScaling(brushOnScreen)
        s.setLightInterface(lightInterface)
        s.setInterfaceSize(interfaceShown)
        applyPlugins(s)
        // as it was left, or fitted
        if (!orientationMemory) s.forgetSavedView()
        s.setInputSettings(inputStabilization, inputMotionFiltering, inputMotionExpression, inputPressureSmoothing, inputTipAttached, inputCurve.flatMap { listOf(it.x, it.y) })
        applyAppSettings(s)
        timeLapse = TimeLapse(dir).also {
            timeLapseRecording = it.recording
            timeLapseRecorded = it.count > 0
        }
        if (!brushesLoaded) loadBrushes(s)
        s.useColourHistory(colourHistory)
        session = s
        val model = s.modelInfo()
        modelOpen = model != null
        textureSets = model?.textureSets ?: emptyList()
        textureSet = s.textureSet().toInt()
        modelFlat = s.modelFlat()
        paintThrough = s.paintThrough()
        submeshes = model?.submeshes ?: emptyList()
        submeshesHidden = s.submeshesHidden()
        openSubmesh = null
        submeshThumbnails.clear()
        modelChannel = s.modelChannel().toInt()
        channelTileChosen = false
        channelColors.fill(null)
        channelTrayFor = null
        channelThumbnails.clear()
        modelExposure = s.modelExposure()
        modelEnvironmentShown = s.modelEnvironmentShown()
        modelLights = s.modelLights()
        lightingStudioOpen = false
        // a model is lit by the environment it was last lit by, else the default
        if (model != null) chooseEnvironment(s.modelEnvironment()?.takeIf { it in ENVIRONMENTS } ?: DEFAULT_ENVIRONMENT)
        // brush files that came while no canvas was open
        addPendingBrushes(s)
        pushSecondary()
        artworkDir = dir
        applyFitInsets()
        attachProjection()
        activeSince = android.os.SystemClock.elapsedRealtime()
        thumbnails.clear()
        thumbnailVersions.clear()
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
        // a new artwork exists in the gallery from the start (an opened one is there already)
        if (!File(dir, "artwork.json").exists()) viewModelScope.launch(Dispatchers.IO) { runCatching { s.save(dir.path) } }
        // kept saved by the core, a few seconds after it changes, once nothing is under way that
        // saving would cut short (only the layers that changed are written); a failure told
        s.startAutosave(dir.path)
        viewModelScope.launch(Dispatchers.IO) {
            while (session === s) {
                kotlinx.coroutines.delay(SAVE_FAILURE_POLL_MS)
                s.takeSaveFailure()?.let { told(Result.failure(Exception(it))) }
                // a smudge the GPU had no memory for, or its device lost
                s.takeFailure()?.let { withContext(Dispatchers.Main) { showNotice(it, warning = true) } }
            }
        }
        // a canvas the device's graphics can't draw says so (one opened from its cell, by the zoom)
        if (galleryZoom == null) viewModelScope.launch { awaitCanvasDrawn() }
    }

    fun select(tool: ToolKind) {
        feedback()
        endRecolor()
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
        // fitted between a fifth of the screen's height above and below (either way up), 40 in from the sides
        val band = windowSize.y * GUIDE_FIT_BAND
        guide = s.guideEditBegin(GUIDE_FIT_SIDE_DP * d, band, windowSize.x - GUIDE_FIT_SIDE_DP * d, windowSize.y - band)
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

    /** The shown grid's size typed on its keypad: [typed] so far, in [unit]. It changes at once. */
    fun setGuideLength(typed: String, unit: GuideUnit) {
        guide = session?.setGuideLength(typed, unit)
    }

    fun setGuideSymmetry(kind: SymmetryType, rotational: Boolean) {
        guide = session?.setGuideSymmetry(kind, rotational)
    }

    /** Drawing assist on the current layer. */
    fun setGuideAssisted(on: Boolean) {
        guide = session?.setGuideAssisted(on)
        refresh()
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
        val s = session ?: return
        closePanels()
        // off the main thread: the picture waits for the canvas to finish any remaking first
        viewModelScope.launch {
            val copied = withContext(Dispatchers.IO) { runCatching { s.copyCanvas() }.getOrNull() }
            clipboard = copied ?: clipboard
            canPaste = clipboard != null
            if (copied != null) showNotice("Canvas copied")
        }
    }

    fun cutLayer(): Boolean {
        if (!currentTakesChanges()) {
            closePanels()
            return false
        }
        val cut = session?.cutLayer()
        clipboard = cut ?: clipboard
        canPaste = clipboard != null
        closePanels()
        refresh()
        if (cut != null) showNotice("Layer cut")
        return cut != null
    }

    /** What was cut or copied, on a new layer; a [private] one is left out of the gallery's thumbnail. */
    fun paste(private: Boolean = false) {
        val image = clipboard ?: return
        val s = session
        // through a patch it takes a moment on a big canvas: the screen doesn't wait for it
        if (!private && s?.pastesIntoMask() == true) {
            closePanels()
            viewModelScope.launch {
                if (withContext(Dispatchers.Default) { s.paste(image) } == null) message = noRoom()
                refresh()
            }
            return
        }
        if ((if (private) s?.pastePrivate(image) else s?.paste(image)) == null) message = noRoom()
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

    /** The QuickMenu's menus, oldest first: each a name and six buttons, clockwise from the top, each set to an action (a hold changes it). */
    var quickMenus by mutableStateOf(
        io.github.artcraftandroid.procraft.core.quickMenus(interfacePrefs.getString(QUICK_MENUS, null), interfacePrefs.getString(QUICK_MENU, null)),
    )
        private set

    /** The menu kept as the active one. */
    var quickMenuActive by mutableStateOf(interfacePrefs.getInt(QUICK_MENU_ACTIVE, 0).coerceIn(0, quickMenus.size - 1))
        private set

    /** The menu the ring shows this time: a menu just added shows without becoming the active one. */
    var quickMenuShown by mutableStateOf(quickMenuActive)
        private set

    /** The buttons the ring shows. */
    val quickMenuSlots: List<QuickAction> get() = quickMenus.getOrNull(quickMenuShown)?.slots.orEmpty()

    fun setQuickSlot(i: Int, a: QuickAction) = setQuickSlot(i, a, 0uL)

    /** Sets button [i] of the ring to give brush [b] to the current tool. */
    fun setQuickSlotBrush(i: Int, b: BrushInfo) {
        val s = session ?: return
        val key = s.quickBrushKey(b.set, b.index)
        if (key == 0uL) return
        setQuickSlot(i, QuickAction.BRUSHES, key)
        // the library keeps the brush's key, following it about
        keepChoices(s)
    }

    private fun setQuickSlot(i: Int, a: QuickAction, brush: ULong) {
        val menu = quickMenus.getOrNull(quickMenuShown) ?: return
        if (i !in menu.slots.indices) return
        val brushes = MutableList(menu.slots.size) { menu.brushes.getOrElse(it) { 0uL } }.also { it[i] = brush }
        val slots = menu.slots.toMutableList().also { it[i] = a }
        quickMenus = quickMenus.toMutableList().also { it[quickMenuShown] = menu.copy(slots = slots, brushes = brushes) }
        keepQuickMenus()
    }

    /** The brush button [i] of the ring gives the current tool, if it is set to one still in the library. */
    fun quickSlotBrush(i: Int): BrushInfo? {
        val menu = quickMenus.getOrNull(quickMenuShown) ?: return null
        val key = menu.brushes.getOrElse(i) { 0uL }
        if (menu.slots.getOrNull(i) != QuickAction.BRUSHES || key == 0uL) return null
        return session?.quickBrush(key)
    }

    /** What button [i] of the ring says: the brush's name when it is set to one. */
    fun quickSlotLabel(i: Int): String = quickSlotBrush(i)?.name ?: quickMenuSlots.getOrNull(i)?.button.orEmpty()

    /** Runs button [i] of the ring: its brush given to the current tool, else its action. */
    fun runQuickSlot(i: Int, at: Offset) {
        quickSlotBrush(i)?.let {
            chooseBrush(it)
            return
        }
        quickMenuSlots.getOrNull(i)?.let { runQuickAction(it, at) }
    }

    /** Adds a menu ("QuickMenu N", every button No Action) and shows it; the active one stays until a menu is chosen. Returns its index. */
    fun addQuickMenu(): Int {
        quickMenus = io.github.artcraftandroid.procraft.core.quickMenuAdded(quickMenus)
        quickMenuShown = quickMenus.size - 1
        keepQuickMenus()
        return quickMenuShown
    }

    /** Makes menu [i] the active one and shows it. */
    fun chooseQuickMenu(i: Int) {
        if (i !in quickMenus.indices) return
        quickMenuActive = i
        quickMenuShown = i
        keepQuickMenus()
    }

    /** Deletes menu [i] (never the last one left); the ring shows the one before it if it showed it. */
    fun deleteQuickMenu(i: Int) {
        if (quickMenus.size < 2 || i !in quickMenus.indices) return
        quickMenus = quickMenus.toMutableList().also { it.removeAt(i) }
        quickMenuActive = io.github.artcraftandroid.procraft.core.quickMenuActiveAfter(quickMenuActive.toUInt(), i.toUInt()).toInt()
        quickMenuShown = io.github.artcraftandroid.procraft.core.quickMenuActiveAfter(quickMenuShown.toUInt(), i.toUInt()).toInt()
        keepQuickMenus()
    }

    private fun keepQuickMenus() {
        interfacePrefs.edit()
            .putString(QUICK_MENUS, io.github.artcraftandroid.procraft.core.quickMenusSaved(quickMenus))
            .putInt(QUICK_MENU_ACTIVE, quickMenuActive)
            .apply()
    }

    /**
     * A key of a hardware keyboard on the canvas ([ctrl] the command key): true if it is a
     * shortcut, done. Typing in a text field never comes here.
     */
    fun keyShortcut(keyCode: Int, ctrl: Boolean, shift: Boolean, alt: Boolean): Boolean {
        if (screen != Screen.CANVAS || session == null) return false
        // the key as the shared shortcuts name it (crates/core keys.rs), which say what it does
        val name = when (keyCode) {
            in android.view.KeyEvent.KEYCODE_A..android.view.KeyEvent.KEYCODE_Z -> ('a' + (keyCode - android.view.KeyEvent.KEYCODE_A)).toString()
            in android.view.KeyEvent.KEYCODE_0..android.view.KeyEvent.KEYCODE_9 -> ('0' + (keyCode - android.view.KeyEvent.KEYCODE_0)).toString()
            android.view.KeyEvent.KEYCODE_LEFT_BRACKET -> "["
            android.view.KeyEvent.KEYCODE_RIGHT_BRACKET -> "]"
            android.view.KeyEvent.KEYCODE_SEMICOLON -> ";"
            android.view.KeyEvent.KEYCODE_SPACE -> "space"
            android.view.KeyEvent.KEYCODE_DEL -> "backspace"
            android.view.KeyEvent.KEYCODE_FORWARD_DEL -> "delete"
            android.view.KeyEvent.KEYCODE_DPAD_LEFT -> "left"
            android.view.KeyEvent.KEYCODE_DPAD_RIGHT -> "right"
            android.view.KeyEvent.KEYCODE_DPAD_UP -> "up"
            android.view.KeyEvent.KEYCODE_DPAD_DOWN -> "down"
            else -> return false
        }
        when (val shortcut = sharedKeyShortcut(name, ctrl, shift, alt, transforming) ?: return false) {
            KeyShortcut.Undo -> undo()
            KeyShortcut.Redo -> redo()
            KeyShortcut.CopyLayer -> copyLayer()
            KeyShortcut.CutLayer -> cutLayer()
            KeyShortcut.Paste -> paste()
            KeyShortcut.Paint -> select(ToolKind.PAINT)
            KeyShortcut.Erase -> select(ToolKind.ERASE)
            KeyShortcut.Eyedropper -> pickNext = !pickNext
            KeyShortcut.Selection -> toggleSelecting()
            KeyShortcut.Transform -> toggleTransform()
            KeyShortcut.HueSaturationBrightness -> startAdjustment(Adjustment.HSB)
            KeyShortcut.Colours -> toggle(Panel.COLOR)
            KeyShortcut.Actions -> toggle(Panel.ACTIONS)
            KeyShortcut.Layers -> toggle(Panel.LAYERS)
            KeyShortcut.SwapColours -> swapColors()
            KeyShortcut.Deselect -> clearSelection()
            KeyShortcut.DuplicateLayer -> duplicateCurrent()
            KeyShortcut.Interface -> toggleBars()
            KeyShortcut.ClearLayer -> clearCurrentLayer()
            KeyShortcut.CopyCanvas -> runQuickAction(QuickAction.COPY_CANVAS, screenCentre)
            KeyShortcut.ColourBalance -> runQuickAction(QuickAction.COLOR_BALANCE, screenCentre)
            KeyShortcut.PerspectiveGuide -> runQuickAction(QuickAction.GUIDE_PERSPECTIVE, screenCentre)
            KeyShortcut.QuickMenu -> toggleQuickMenu(screenCentre)
            KeyShortcut.GreyView -> session?.let { it.setGreyView(!it.greyView()) }
            is KeyShortcut.BrushSize -> changeSize((size + shortcut.by).coerceIn(0f, 1f))
            is KeyShortcut.Nudge -> session?.transformArrow(shortcut.dx, shortcut.dy)?.let { transformMoved(it) }
        }
        return true
    }

    /** Alt on a hardware keyboard went down or up: held it is □ held; it taps nothing, being in the system's own shortcuts. */
    fun modifyKey(down: Boolean): Boolean {
        if (screen != Screen.CANVAS || session == null) return false
        if (down) modifyPressed() else modifyReleased(true)
        return true
    }

    /** Does [a], a QuickMenu button opened at window point [at]. */
    fun runQuickAction(a: QuickAction, at: Offset) {
        val current = layers.firstOrNull { it.current }
        a.adjustment?.let {
            startAdjustment(it)
            return
        }
        when (a) {
            QuickAction.NEW_LAYER -> addLayer()
            QuickAction.DUPLICATE_LAYER -> current?.let(::duplicate)
            QuickAction.DELETE_LAYER -> current?.let(::delete)
            QuickAction.MERGE_DOWN -> current?.let(::mergeDown)
            QuickAction.CLEAR_LAYER -> clearCurrentLayer()
            QuickAction.FILL_LAYER -> current?.let(::fillLayer)
            QuickAction.SELECT_LAYER -> current?.let(::selectLayerContents)
            QuickAction.ALPHA_LOCK -> current?.let(::toggleAlphaLock)
            QuickAction.LAYER_VISIBILITY -> current?.let(::toggleVisible)
            QuickAction.LAYER_OPACITY -> startLayerOpacity()
            QuickAction.SOLO -> current?.let(::toggleSolo)
            QuickAction.COPY -> copyLayer()
            QuickAction.COPY_CANVAS -> copyCanvas()
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
            QuickAction.SWAP_COLORS -> previousColor()
            QuickAction.PREVIOUS_BRUSH -> previousBrush()
            QuickAction.SELECTION -> toggleSelecting()
            QuickAction.TRANSFORM -> toggleTransform()
            QuickAction.ADJUSTMENTS -> adjustmentsPressed()
            QuickAction.LIQUIFY -> startAdjustment(Adjustment.LIQUIFY)
            QuickAction.CLONE -> startAdjustment(Adjustment.CLONE)
            QuickAction.COLORS -> toggle(Panel.COLOR)
            QuickAction.LAYERS -> toggle(Panel.LAYERS)
            QuickAction.BRUSHES -> toggle(Panel.BRUSHES)
            QuickAction.INTERFACE -> toggleBars()
            QuickAction.FIT_CANVAS -> settleToFit()
            QuickAction.ACTUAL_SIZE -> {
                session?.settleToActualSize()
                followView()
            }
            QuickAction.ADD_TEXT -> addText()
            QuickAction.QUICKSHAPE -> quickShapeLast()
            QuickAction.QUICKSHAPE_ON -> gestures.set(GesturePurpose.QUICKSHAPE, "DRAW_HOLD", !gestures.isOn(GesturePurpose.QUICKSHAPE, "DRAW_HOLD"))
            QuickAction.LAYER_SELECT -> layerSelectAt(at)
            QuickAction.GUIDE2D -> toggleGuideKind(GuideType.GRID)
            QuickAction.GUIDE_ISOMETRIC -> toggleGuideKind(GuideType.ISOMETRIC)
            QuickAction.GUIDE_PERSPECTIVE -> toggleGuideKind(GuideType.PERSPECTIVE)
            QuickAction.GUIDE_SYMMETRY -> toggleGuideKind(GuideType.SYMMETRY)
            QuickAction.DRAWING_ASSIST -> guide?.let { setGuideAssisted(!it.assisted) }
            QuickAction.ANIMATION_ASSIST -> changeAnimationAssist(!(animation != null && animation?.pages == false))
            QuickAction.PAGE_ASSIST -> changePageAssist(animation?.pages != true)
            QuickAction.NEXT_FRAME -> stepFrame(1)
            QuickAction.PREVIOUS_FRAME -> stepFrame(-1)
            QuickAction.ONION_SKIN -> toggleOnionSkin()
            QuickAction.BRUSH_SCALING -> changeBrushScaling(!brushOnScreen)
            QuickAction.PROJECT_CANVAS -> changeProjectCanvas(!projectCanvas)
            QuickAction.REFERENCE -> changeReferenceOn(!referenceOn)
            QuickAction.COMPANION_MOVE -> toggleCompanionMode(CompanionMode.MOVE)
            QuickAction.COMPANION_ZOOM -> toggleCompanionMode(CompanionMode.ZOOM)
            QuickAction.RECOLOR -> toggleRecolor()
            // nothing, and the adjustments (done above)
            else -> Unit
        }
    }

    /** The drawing guide on as [kind], or off if it already is that. */
    private fun toggleGuideKind(kind: GuideType) {
        val g = guide
        if (g?.on == true && g.kind == kind) {
            setGuideOn(false)
            return
        }
        if (g?.on != true) setGuideOn(true)
        chooseGuideKind(kind)
    }

    /** Animation Assist's next or previous frame drawn on ([by] frames along), staying in its frames. */
    private fun stepFrame(by: Int) {
        val at = currentFrame ?: return
        val to = (at + by).coerceIn(0, animationFrames.size - 1)
        if (to != at) tapFrame(to)
    }

    /** How many onion skin frames there were before they were last turned off. */
    private var onionKept = 1u

    /** Onion skins off, or back to as many as they were. */
    private fun toggleOnionSkin() {
        changeAnimation {
            if (it.onionFrames > 0u) {
                onionKept = it.onionFrames
                it.copy(onionFrames = 0u)
            } else {
                it.copy(onionFrames = onionKept)
            }
        }
    }

    /** Layer [l] shown alone, or, already alone, every layer again; a message says which. */
    fun toggleSolo(l: LayerInfo) {
        val s = session ?: return
        val on = s.solo() != l.id
        if (!s.setSolo(if (on) l.id else null)) return
        showNotice(if (on) "Solo visibility on" else "Solo visibility off")
    }

    // Layer opacity: a drag across the canvas changes the current layer's opacity, read in the band
    // under the top bar

    /** The layer whose opacity a drag changes, while the mode is on. */
    var opacityLayer by mutableStateOf<LayerInfo?>(null)
        private set

    /** Its opacity as it is being dragged. */
    var opacityValue by mutableFloatStateOf(1f)
        private set

    fun startLayerOpacity() {
        val l = layers.firstOrNull { it.current } ?: return
        if (l.kind != LayerType.PAINT && l.kind != LayerType.GROUP) return
        switchModes(ModeButton.LayerOpacity)
        closePanels()
        opacityLayer = l
        opacityValue = l.opacity
        // the shared touches drag it, each drag one undo step
        session?.setTouchLayerOpacity(l.id)
    }

    /** The shared touches dragged the opacity: shown as it is; let go ([ended]), the layers read again. */
    fun layerOpacityTouched(ended: Boolean) {
        val s = session ?: return
        s.touchLayerOpacity()?.let { opacityValue = it }
        if (ended) refresh()
    }

    fun endLayerOpacity() {
        if (opacityLayer != null) session?.setTouchLayerOpacity(null)
        opacityLayer = null
    }

    /** Does what a gesture set to the QuickMenu does: opens it at [at], or closes it if open. */
    fun toggleQuickMenu(at: Offset) {
        if (quickMenuAt == null) quickMenuShown = quickMenuActive
        quickMenuAt = if (quickMenuAt == null) at else null
    }

    /** The Copy & Paste panel (three fingers swiped down) is open. */
    var copyPasteOpen by mutableStateOf(false)

    /** Cut & Paste: what is selected (or the layer) cut out and put back as a new layer. */
    fun cutAndPaste() {
        // nothing cut (a locked layer, an empty one): what is on the clipboard stays there
        if (cutLayer()) paste()
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

    /**
     * A photo or picture file comes in as a new layer, fitted to the canvas, ready to transform; a
     * [private] one is left out of the gallery's thumbnail.
     */
    fun insertPicture(uri: android.net.Uri, private: Boolean = false) {
        if (session == null) return
        closePanels()
        val type = getApplication<Application>().contentResolver.getType(uri)
        if (type?.startsWith("video/") == true) {
            insertFrames(uri, private)
            return
        }
        viewModelScope.launch { insertPictureNow(uri, private, transform = true) }
    }

    /**
     * Files dragged onto the canvas from another app: a picture or a video comes in as
     * [insertPicture] puts it, a picture alone ready to transform (several each on a new layer of
     * their own, one after another, a video's as its frames); other files' brushes join the
     * library.
     */
    fun dropOnCanvas(uris: List<android.net.Uri>) {
        if (session == null || uris.isEmpty()) return
        val resolver = getApplication<Application>().contentResolver
        val types = uris.associateWith { runCatching { resolver.getType(it) }.getOrNull().orEmpty() }
        val video = { uri: android.net.Uri -> types[uri]?.startsWith("video/") == true }
        val (inserted, others) = uris.partition { types[it]?.startsWith("image/") == true || video(it) }
        others.forEach(::importBrushes)
        if (inserted.isEmpty()) return
        if (transforming) endTransform(true)
        closePanels()
        viewModelScope.launch {
            for (uri in inserted) {
                val came = if (video(uri)) insertFramesNow(uri, private = false) else insertPictureNow(uri, private = false, transform = inserted.size == 1)
                if (!came) break
            }
        }
    }

    /**
     * The picture at [uri] on a new layer, fitted to the canvas (a moving GIF's frames on the
     * timeline instead), then with [transform] ready to transform; false, and a message, if it
     * couldn't be.
     */
    private suspend fun insertPictureNow(uri: android.net.Uri, private: Boolean, transform: Boolean): Boolean {
        val s = session ?: return false
        val resolver = getApplication<Application>().contentResolver
        val type = withContext(Dispatchers.IO) { runCatching { resolver.getType(uri) }.getOrNull() }
        // a moving GIF: its frames, as a video's (a still one is a picture)
        if (type == "image/gif" || withContext(Dispatchers.IO) { fileName(uri) }?.endsWith(".gif", ignoreCase = true) == true) {
            val bytes = withContext(Dispatchers.IO) { runCatching { resolver.openInputStream(uri)?.use { it.readBytes() } }.getOrNull() }
            val done = bytes?.let { withContext(Dispatchers.Default) { runCatching { s.insertGifFrames(it, private) }.getOrNull() } }
            if (done != null) {
                if (done.added == 0u) {
                    message = if (done.full) noRoom() else "This picture can't be opened."
                    return false
                }
                refresh()
                showNotice("Frames saved to timeline")
                if (done.full) message = noRoom()
                return !done.full
            }
        }
        val image = withContext(Dispatchers.IO) { decodePicture(uri, io.github.artcraftandroid.procraft.ui.CanvasSpace.p3) }
        if (image == null) {
            message = "This picture can't be opened."
            return false
        }
        if (s.insertImage(image, private) == null) {
            message = noRoom()
            return false
        }
        refresh()
        if (transform) toggleTransform()
        return true
    }

    /**
     * A video's frames come in as layers above the current one, each fitted to the canvas, at the
     * animation's frame rate (or its first one's), for as many as there is room; Animation Assist
     * comes on with them in its timeline.
     */
    private fun insertFrames(uri: android.net.Uri, private: Boolean) {
        viewModelScope.launch { insertFramesNow(uri, private) }
    }

    /** [insertFrames] done here; whether any frame came in. */
    private suspend fun insertFramesNow(uri: android.net.Uri, private: Boolean): Boolean {
        val s = session ?: return false
        val fps = (animation?.fps?.toInt() ?: DEFAULT_FPS).coerceIn(1, 60)
        var added = 0
        var full = false
        withContext(Dispatchers.IO) {
            val retriever = android.media.MediaMetadataRetriever()
            try {
                retriever.setDataSource(getApplication<Application>(), uri)
                val ms = retriever.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull() ?: 0L
                val count = (ms * fps / 1000L).toInt().coerceIn(1, MAX_VIDEO_FRAMES)
                for (i in 0 until count) {
                    val us = i * 1_000_000L / fps
                    val frame = retriever.getFrameAtTime(us, android.media.MediaMetadataRetriever.OPTION_CLOSEST) ?: continue
                    val image = frameImage(frame)
                    frame.recycle()
                    if (s.insertImage(image, private) == null) {
                        full = true
                        break
                    }
                    added++
                }
            } catch (e: Exception) {
                android.util.Log.e("ProCraft", "a video's frames", e)
            } finally {
                retriever.release()
            }
        }
        if (added == 0) {
            message = if (full) noRoom() else "This video can't be opened."
            return false
        }
        if (animation == null || animation?.pages == true) changeAnimationAssist(true) else refresh()
        showNotice("Frames saved to timeline")
        if (full) message = noRoom()
        return true
    }

    /** A video frame's straight RGBA pixels, at most [MAX_PICTURE] on a side. */
    private fun frameImage(frame: Bitmap): io.github.artcraftandroid.procraft.core.ThumbnailImage {
        val scale = minOf(1f, MAX_PICTURE / maxOf(frame.width, frame.height).toFloat())
        val sized = if (scale < 1f) Bitmap.createScaledBitmap(frame, (frame.width * scale).roundToInt().coerceAtLeast(1), (frame.height * scale).roundToInt().coerceAtLeast(1), true) else frame
        val argb = if (sized.config == Bitmap.Config.ARGB_8888) sized else sized.copy(Bitmap.Config.ARGB_8888, false)
        val buffer = ByteBuffer.allocate(argb.width * argb.height * 4)
        // frames are opaque, so premultiplied and straight are the same
        argb.copyPixelsToBuffer(buffer)
        val image = io.github.artcraftandroid.procraft.core.ThumbnailImage(argb.width.toUInt(), argb.height.toUInt(), buffer.array())
        if (argb !== frame) argb.recycle()
        if (sized !== frame && sized !== argb) sized.recycle()
        return image
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

    /** The stroke held still snapped to a shape (the canvas's touches did it). */
    fun quickShapeSnapped() {
        val info = session?.quickshapeInfo() ?: return
        quickShape = info
        quickShapeEditing = false
        showNotice(snappedTo(info.kind))
    }

    /** The snapped shape changed (dragged, or a finger down while it is held). */
    fun quickShapeChanged() {
        quickShape = session?.quickshapeInfo() ?: quickShape
    }

    /** Undo or redo stepped through the snapped shape: as it now is, if it is still there. */
    private fun followQuickShape() {
        quickShape = session?.quickshapeInfo()
        if (quickShape == null) quickShapeEditing = false
    }

    /** The snapped shape was let go of as it is (the canvas's touches did it). */
    fun quickShapeEnded() {
        quickShape = null
        quickShapeEditing = false
        refreshFlags()
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

    /** Keeps the snapped shape as drawn. */
    fun endQuickShape() {
        if (quickShape == null) return
        session?.quickshapeDone()
        quickShape = null
        quickShapeEditing = false
        refreshFlags()
    }

    fun toolPressed(tool: ToolKind) {
        // leaving a mode for a tool keeps what it made, and only picks the tool
        if (!switchModes(ModeButton.Tool)) {
            select(tool)
            return
        }
        if (this.tool == tool) {
            // the library opens on the current brush's set, or on the Recent set if it was shown
            if (shownSet != RECENT_SET) currentBrush?.let { shownSet = it.set.toInt() }
            readRecentBrushes()
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
        if (target == tool || !switchModes(ModeButton.ToolHeld)) return
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
        pickedBrushes.clear()
        if (index == RECENT_SET) {
            readRecentBrushes()
            return
        }
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

    /** Something has been recorded (Replay and Export need it). */
    var timeLapseRecorded by mutableStateOf(false)
        private set

    /** Recording was turned off with something recorded: whether to delete it is asked. */
    var purgeAsking by mutableStateOf(false)
        private set

    /** Frames recorded, for the replay. */
    fun timeLapseFrames() = timeLapse?.count ?: 0

    fun timeLapseFrame(i: Int): Bitmap? = timeLapse?.frame(i)

    fun changeTimeLapseRecording(on: Boolean) {
        timeLapse?.recording = on
        timeLapseRecording = on
        // the switch goes off at once; then what was recorded may be deleted
        if (!on && timeLapseFrames() > 0) purgeAsking = true
    }

    /** The answer to [purgeAsking]: what was recorded deleted, or kept. */
    fun answerPurge(purge: Boolean) {
        purgeAsking = false
        if (purge) timeLapse?.purge()
        timeLapseRecorded = timeLapseFrames() > 0
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
                    // frames as large as the video they make, private layers left out
                    val shot = s.timeLapseFrame(t.settings.side.toUInt()) ?: return@withContext
                    t.record(shot.rgba, shot.width.toInt(), shot.height.toInt())
                }
            } while (frameStale)
            timeLapseRecorded = t.count > 0
        }
    }

    /** A time-lapse video is being made: another isn't started over it (they'd share its file). */
    var timeLapseExporting by mutableStateOf(false)
        private set

    /** An export under way as its card shows it: how far it has come (0..1), and once [done]. */
    data class ExportShown(val progress: Float, val done: Boolean = false)

    /** The export card, while an export runs (null without one). */
    var exportShown by mutableStateOf<ExportShown?>(null)
        private set

    @Volatile
    private var exportCancelled = false

    /** The export card's Cancel: the export stops, and nothing is shared. */
    fun cancelExport() {
        exportCancelled = true
    }

    /**
     * The time-lapse as a video in the cache, to share: all of it, or [seconds] long; its card
     * shows how far it has come and a check once made. Null if it was cancelled or couldn't be
     * made (a notice says so).
     */
    suspend fun exportTimeLapse(seconds: Int?): File? {
        val t = timeLapse ?: return null
        val s = session ?: return null
        if (timeLapseExporting || exportShown != null) return null
        timeLapseExporting = true
        exportCancelled = false
        exportShown = ExportShown(0f)
        try {
            val made = withContext(Dispatchers.IO) {
                val dir = File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }
                val base = io.github.artcraftandroid.procraft.core.exportFileName(s.name(), false)
                val out = File(dir, "$base.mp4")
                out.delete()
                var shownAt = -1
                val ok = runCatching {
                    t.export(out, seconds) { done ->
                        // a step a hundredth, so the card isn't redrawn for every frame
                        val step = (done * 100).toInt()
                        if (step != shownAt) {
                            shownAt = step
                            exportShown = ExportShown(done)
                        }
                        !exportCancelled
                    }
                }.onFailure { Log.e(TAG, "exporting the time-lapse", it) }.getOrDefault(false)
                out.takeIf { ok }
            }
            if (exportCancelled) return null
            if (made == null) {
                // which of the two: they are different faults, in a report too
                val readable = withContext(Dispatchers.IO) { t.frame(0)?.also { it.recycle() } != null }
                showNotice(if (readable) "The time-lapse video couldn't be made." else "The time-lapse can't be read on this device.", warning = true)
                return null
            }
            // full, then the check drawn in it before the card goes
            exportShown = ExportShown(1f, done = true)
            kotlinx.coroutines.delay(EXPORT_DONE_MS)
            return made
        } finally {
            exportShown = null
            timeLapseExporting = false
        }
    }

    // ---- crop and resize: the shared core edits it (crates/core crop_edit.rs); these show it ----

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
        private set
    var cropSettingsOpen by mutableStateOf(false)
    var cropSnapping by mutableStateOf(false)
        private set
    /** The canvas's dots per inch, as the crop's settings have it. */
    var cropDpi by mutableStateOf(132f)
        private set

    /** The crop as the core has it now. */
    private fun readCrop() {
        val c = session?.cropShown()
        cropRect = c?.let { androidx.compose.ui.geometry.Rect(it.x, it.y, it.x + it.width, it.y + it.height) }
        if (c == null) return
        cropAngle = c.angle
        cropResample = c.resample
        cropSize = c.sizeWidth.toInt() to c.sizeHeight.toInt()
        cropAspectLocked = c.aspectLocked
        cropSnapping = c.snapping
        cropDpi = c.dpi
    }

    fun changeCropDpi(dpi: Float) {
        session?.cropSetDpi(dpi)
        readCrop()
    }

    /** The canvas's size, pixels. */
    fun canvasSize(): Pair<Int, Int> = session?.info()?.let { it.width.toInt() to it.height.toInt() } ?: (0 to 0)

    fun beginCrop() {
        closePanels()
        val (w, _) = canvasSize()
        if (w == 0) return
        // the canvas fitted to the screen and shrunk, in the middle, so the frame's handles are clear
        session?.cropBegin()
        refreshView()
        followView()
        readCrop()
        cropSettingsOpen = false
    }

    fun resetCrop() = beginCrop()

    /** A press on the frame at window point [at]: near a corner or a side's middle it takes that, else the whole frame. */
    fun cropPress(at: Offset) {
        session?.cropPress(at.x, at.y)
    }

    /** The drag on the frame reached window point [at]. */
    fun cropDrag(at: Offset) {
        session?.cropDrag(at.x, at.y)
        readCrop()
    }

    fun cropRelease() {
        session?.cropRelease()
    }

    fun changeCropAngle(radians: Float) {
        session?.cropSetAngle(radians)
        readCrop()
    }

    fun changeCropResample(on: Boolean) {
        session?.cropSetResample(on)
        readCrop()
    }

    fun changeCropAspectLocked(on: Boolean) {
        session?.cropSetAspectLocked(on)
        readCrop()
    }

    fun changeCropSnapping(on: Boolean) {
        session?.cropSetSnapping(on)
        readCrop()
    }

    /** A width or height typed in: resampling, it is the new size; otherwise the kept rectangle's. */
    fun changeCropSize(width: Int?, height: Int?) {
        session?.cropSetSize(width?.coerceAtLeast(1)?.toUInt(), height?.coerceAtLeast(1)?.toUInt())
        readCrop()
    }

    fun endCrop(keep: Boolean) {
        if (cropRect == null) return
        cropRect = null
        cropSettingsOpen = false
        val s = session ?: return
        // either way the canvas is fitted to the screen again: cancelled, growing back into place
        val changed = s.cropEnd(keep)
        // a crop the GPU couldn't make: the canvas is as it was, and says so
        s.takeFailure()?.let { showNotice(it, warning = true) }
        if (!keep) {
            refreshView()
            followView()
            return
        }
        if (changed) {
            thumbnails.clear()
            thumbnailVersions.clear()
        }
        refresh()
        refreshView()
        if (changed && panel == Panel.LAYERS) refreshThumbnails()
    }

    // ---- the reference window ----

    enum class ReferenceMode { CANVAS, IMAGE, FACE }

    var referenceOn by mutableStateOf(false)
        private set
    var referenceMode by mutableStateOf(ReferenceMode.CANVAS)
        private set

    /** The whole canvas as it is, kept up to date while the window shows it. */
    var referenceCanvas by mutableStateOf<ImageBitmap?>(null)

    /** [referenceCanvas] is the canvas at its own size (not made smaller): its pixels are the canvas's. */
    var referenceCanvasWhole by mutableStateOf(false)
        private set
    var referenceImage by mutableStateOf<ImageBitmap?>(null)
        private set

    /** How the window shows the canvas and a picture imported, each zoomed and moved by two fingers on its own. */
    var referenceCanvasView by mutableStateOf(io.github.artcraftandroid.procraft.core.ReferenceView(1f, 0f, 0f))
        private set
    var referenceImageView by mutableStateOf(io.github.artcraftandroid.procraft.core.ReferenceView(1f, 0f, 0f))
        private set

    /** Where the window is (window pixels), and the size of its picture. */
    var referenceOffset by mutableStateOf(Offset.Unspecified)
        private set

    /** The window's bars are slid out of the way. */
    var referenceBarsHidden by mutableStateOf(false)
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
            if (image != null) {
                referenceImage = image.asImageBitmap()
                referenceImageView = io.github.artcraftandroid.procraft.core.ReferenceView(1f, 0f, 0f)
            } else {
                message = "This picture can't be opened."
            }
        }
    }

    /** A tap on the window's picture: its bars slide away, or back. */
    fun toggleReferenceBars() {
        referenceBarsHidden = !referenceBarsHidden
    }

    /** Two taps on the window's picture: it fits the window again, the bars out of the way. */
    fun fitReference() {
        val fitted = io.github.artcraftandroid.procraft.core.ReferenceView(1f, 0f, 0f)
        if (referenceMode == ReferenceMode.IMAGE) referenceImageView = fitted else referenceCanvasView = fitted
        referenceBarsHidden = true
    }

    fun clearReference() {
        referenceImage = null
        referenceImageView = io.github.artcraftandroid.procraft.core.ReferenceView(1f, 0f, 0f)
    }

    /**
     * Two fingers on the window's picture ([sizes]) moved from [from] to [to] (their middle, pixels
     * in the picture's area), [spread] times as far apart: the picture zooms and moves with them.
     */
    fun pinchReference(sizes: io.github.artcraftandroid.procraft.core.ReferenceSizes, from: Offset, to: Offset, spread: Float) {
        val image = referenceMode == ReferenceMode.IMAGE
        val view = if (image) referenceImageView else referenceCanvasView
        val next = io.github.artcraftandroid.procraft.core.referenceViewPinched(view, sizes, from.x, from.y, to.x, to.y, spread)
        if (image) referenceImageView = next else referenceCanvasView = next
    }

    /**
     * The eyedropper over the window's [picture] at its point ([px], [py]) (a window pixel spanning
     * [per] of its pixels), the finger at window point [at]: the colour there and what is around it.
     */
    fun pickReference(picture: ImageBitmap, px: Float, py: Float, per: Float, at: Offset) {
        val bitmap = picture.asAndroidBitmap()
        val x = px.toInt().coerceIn(0, bitmap.width - 1)
        val y = py.toInt().coerceIn(0, bitmap.height - 1)
        val rgb = bitmap.getPixel(x, y) and 0xFFFFFF
        // as much of the picture as the loupe's window shows, around the point
        val n = (PICK_WINDOW_DP * density * per).roundToInt().coerceIn(1, minOf(bitmap.width, bitmap.height))
        val around = Bitmap.createBitmap(bitmap, (x - n / 2).coerceIn(0, bitmap.width - n), (y - n / 2).coerceIn(0, bitmap.height - n), n, n)
        picking = PickState(at.x, at.y, around.asImageBitmap(), rgb, fromReference = true)
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
                val made = withContext(Dispatchers.Default) {
                    val t = s.canvasThumbnail(REFERENCE_PIXELS.toUInt()) ?: return@withContext null
                    val bitmap = io.github.artcraftandroid.procraft.ui.CanvasSpace.tag(Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), Bitmap.Config.ARGB_8888))
                    bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(t.rgba))
                    bitmap.asImageBitmap() to (t.width == s.info().width)
                }
                if (made != null) {
                    referenceCanvas = made.first
                    referenceCanvasWhole = made.second
                }
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

    /** The style sheet's Cancel: its changes go back (the text, where it is and its width stay), and the keyboard returns. */
    fun cancelTextStyle() {
        textBeforeStyle?.let { before -> changeText { before.copy(text = it.text, x = it.x, y = it.y, width = it.width) } }
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

    /** Copies a font file into the app and offers it (the core keeps it); the text being edited takes it. */
    fun importFonts(uris: List<android.net.Uri>) {
        viewModelScope.launch {
            val families = withContext(Dispatchers.IO) { uris.mapNotNull { importFont(it) } }
            val family = families.firstOrNull() ?: return@launch
            changeText { it.copy(font = family) }
            showNotice(if (families.size > 1) "Fonts imported" else "Font imported")
        }
    }

    /** The font file at [uri] kept and offered: its family, or null. */
    private fun importFont(uri: android.net.Uri): String? =
        runCatching {
            val resolver = getApplication<Application>().contentResolver
            val name = resolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst()) c.getString(0) else null
            } ?: "Font.ttf"
            val bytes = resolver.openInputStream(uri)?.use { it.readBytes() } ?: return@runCatching null
            // kept in the fonts folder by the core; a file that isn't a font is dropped
            TextRenderer.importFont(bytes, name)
        }.getOrNull()

    /** A stroke landed on a text layer: offer to rasterize it. */
    var askRasterize by mutableStateOf(false)

    private var textJob: Job? = null
    private var textGeneration = 0

    /** New text in the middle of the canvas, all of it selected for typing over. */
    fun addText() {
        val s = session ?: return
        closePanels()
        val rgb = colorRgb()
        // a medium sans, sized and placed as new text looks on screen (above the keyboard, about
        // a quarter of the way down)
        var info = TextInfo(
            text = "Text", font = "sans-serif", style = "Medium",
            size = 0f,
            kerning = 0f, tracking = 0f, leading = 0f, baseline = 0f, opacity = 1f,
            align = TextAlignment.CENTER, underline = false, outline = false, vertical = false, allCaps = false,
            r = ((rgb shr 16) and 255) / 255f, g = ((rgb shr 8) and 255) / 255f, b = (rgb and 255) / 255f,
            x = 0f, y = 0f, width = 0f,
        )
        info = s.newTextPlaced(info, windowSize.x, windowSize.y, screenDensity)
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

    /** Text layer [id] back in its box, selected but with the keyboard away (undo and redo of it). */
    private fun resumeText(id: ULong): Boolean {
        val s = session ?: return false
        val info = s.layerText(id) ?: return false
        s.textBegin(id, info) ?: return false
        startText(info, keyboard = false)
        return true
    }

    /** Whether the keyboard is wanted for the text being edited (a touch on the text asks for it). */
    var textKeyboard by mutableStateOf(true)

    private fun startText(info: TextInfo, keyboard: Boolean = true) {
        textKeyboard = keyboard
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

    /**
     * Draws the text into its layer off the main thread (only where it was and now reaches), one
     * drawing at a time: those asked for while one draws are passed over for the latest.
     */
    private fun renderText() {
        val s = session ?: return
        if (textInfo == null) return
        val generation = ++textGeneration
        val before = textJob
        textJob = viewModelScope.launch {
            before?.join()
            if (generation == textGeneration) withContext(Dispatchers.Default) { TextRenderer.drawInto(s) }
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

    /** The side of the text box whose dot is within [reach] pixels of window point [p], if any. */
    fun textSideAt(p: Offset, reach: Float): TextSide? = session?.textSideAt(p.x, p.y, reach)

    /** A drag of the text box's [side] begins at window point [from]: the side follows it, the other staying put. */
    fun beginTextSide(side: TextSide, from: Offset): Boolean = session?.textSideBegin(side, from.x, from.y) == true

    /** The side being dragged follows the drag to window point [to]: the text wraps to the box's new width. */
    fun dragTextSide(to: Offset) {
        val info = session?.textSideDrag(to.x, to.y) ?: return
        textInfo = info
        renderText()
    }

    /** The side's drag ends: one undo step while the text is edited. */
    fun endTextSide() {
        session?.textSideEnd()
        refresh()
    }

    /**
     * Inside editing text, a change to its box's width undone ([undo]) or redone, the text still
     * edited; false if there is none.
     */
    private fun stepTextBox(undo: Boolean): Boolean {
        val s = session ?: return false
        val name = (if (undo) s.textBoxUndo() else s.textBoxRedo()) ?: return false
        s.editingText()?.let(s::layerText)?.let { textInfo = it }
        renderText()
        showNotice((if (undo) "Undo " else "Redo ") + name)
        refresh()
        return true
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
        // the shared core decides
        val why = when (session?.whyBlocked()) {
            io.github.artcraftandroid.procraft.core.Blocked.LOCKED -> Blocked.LOCKED
            io.github.artcraftandroid.procraft.core.Blocked.HIDDEN -> Blocked.HIDDEN
            io.github.artcraftandroid.procraft.core.Blocked.EMPTY_GROUP -> Blocked.EMPTY_GROUP
            null -> return true
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
        // picks go by place, which may hold another brush now, and so do sample strokes being drawn
        pickedBrushes.clear()
        previewRound++
        brushSets = s.brushLibrary()
        currentBrush = s.currentBrush()
        readSetChoices()
        if (shownSet != RECENT_SET) shownSet = shownSet.coerceIn(0, (brushSets.size - 1).coerceAtLeast(0))
        recentBrushes = s.recentBrushes()
        readSliders()
        previews.clear()
        shadedNames.clear()
        keepBrushLibrary(s)
    }

    fun duplicateBrush(b: BrushInfo) {
        session?.duplicateBrush(b.set, b.index) ?: return
        libraryChanged()
    }

    fun deleteBrush(b: BrushInfo) {
        if (session?.deleteBrush(b.set, b.index) == true) libraryChanged()
    }

    /** The brushes of the shown set picked by a swipe right, by place ("set-index"). */
    val pickedBrushes = androidx.compose.runtime.mutableStateListOf<String>()

    fun togglePickedBrush(b: BrushInfo) {
        val key = "${b.set}-${b.index}"
        if (!pickedBrushes.remove(key)) pickedBrushes.add(key)
    }

    /** One brush is picked, in the current brush's set, and neither is a pair already. */
    val canCombinePicked: Boolean
        get() {
            val s = session ?: return false
            val c = currentBrush ?: return false
            val (set, index) = pickedBrushes.singleOrNull()?.let(::pickedPlace) ?: return false
            return set == c.set && !s.brushIsDual(set, index) && !s.brushIsDual(c.set, c.index)
        }

    private fun pickedPlace(key: String): Pair<UInt, UInt>? {
        val (set, index) = key.split('-').map { it.toUIntOrNull() ?: return null }
        return set to index
    }

    /** The picked brush becomes the current brush's second; a tool on either has the pair. */
    fun combinePicked() {
        val s = session ?: return
        val c = currentBrush ?: return
        val (_, index) = pickedBrushes.singleOrNull()?.let(::pickedPlace) ?: return
        pickedBrushes.clear()
        if (s.combineBrushes(c.set, c.index, index)) {
            currentBrush = s.currentBrush()
            libraryChanged()
        }
    }

    fun deletePickedBrushes() {
        val s = session ?: return
        // from the last up, so the places of the rest hold
        val places = pickedBrushes.mapNotNull(::pickedPlace).sortedByDescending { it.second }
        pickedBrushes.clear()
        if (places.count { (set, index) -> s.deleteBrush(set, index) } > 0) libraryChanged()
    }

    /** Moves [b] to place [index] of set [set] (a brush dragged onto a set, or along its own). */
    fun moveBrush(b: BrushInfo, set: Int, index: Int) {
        if (set == b.set.toInt() && index == b.index.toInt()) return
        if (session?.moveBrush(b.set, b.index, set.toUInt(), index.toUInt()) == true) libraryChanged()
    }

    /** A new brush at the top of the shown set, opened in the studio. */
    fun newBrush() {
        val s = session ?: return
        if (shownSet == RECENT_SET) shownSet = currentBrush?.set?.toInt() ?: 0
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

    /**
     * Moves set [from] to place [to] (a set held and dragged onto another); the shown set stays
     * shown, and what is held by place goes with its set.
     */
    fun moveBrushSet(from: Int, to: Int) {
        if (from == to || session?.moveBrushSet(from.toUInt(), to.toUInt()) != true) return
        // past the end is the last place, as the core takes it
        val at = to.coerceAtMost(brushSets.lastIndex)
        val moved = { set: Int -> placeAfterSetMove(set, from, at) }
        if (shownSet != RECENT_SET) shownSet = moved(shownSet)
        lastBrush = lastBrush?.let { (set, index) -> moved(set.toInt()).toUInt() to index }
        swipedBrush = null
        // no brush changed: their sample strokes go with their sets, not drawn again
        val place = { key: String -> key.substringBefore('-').toIntOrNull()?.let { "${moved(it)}-${key.substringAfter('-')}" } ?: key }
        val strokes = previews.mapKeys { place(it.key) }
        val shaded = shadedNames.mapKeys { place(it.key) }
        libraryChanged()
        previews.putAll(strokes)
        shadedNames.putAll(shaded)
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

    /**
     * A brush or set file from elsewhere, with the import card: it joins the library and its set
     * is shown (not if the card's Cancel was tapped meanwhile).
     */
    fun importBrushes(uri: android.net.Uri) {
        viewModelScope.launch {
            if (importBusy || exportShown != null) return@launch
            val name = withContext(Dispatchers.IO) { fileName(uri) }
            withImportCard(name ?: "Untitled") { importBrushesFrom(uri, name) }
        }
    }

    private suspend fun importBrushesFrom(uri: android.net.Uri, name: String?) {
        val read = withContext(Dispatchers.IO) { runCatching { readAll(uri) } }
        val bytes = read.getOrNull()
        if (bytes == null) {
            message = if (read.exceptionOrNull() is OutOfMemoryError) "This file is too large to import." else "This file could not be read."
            return
        }
        // the name says which kind, when it has one we know; else our own kinds first, then
        // the other apps' (each told apart by its contents)
        val added = session?.let {
            // the shared core tells the kinds apart and makes them brushes off the main
            // thread; they join the library here, once nothing holds a place in it
            val read = withContext(Dispatchers.Default) {
                io.github.artcraftandroid.procraft.core.readBrushes(bytes, name)
            }
            if (importCancelled) return
            read?.let {
                brushesRead += it
                addBrushesRead()
            } != null
        } ?: if (importCancelled) {
            return
        } else if (name != null && isOtherBrushFile(name)) {
            addOtherBrushes(bytes, name)
        } else {
            addBrushes(bytes) || addOtherBrushes(bytes, name ?: "Imported")
        }
        if (!added) message = "This file has no brushes in it."
    }

    /** Brushes read from files, waiting to join the library. */
    private val brushesRead = mutableListOf<io.github.artcraftandroid.procraft.core.BrushesRead>()

    /** The library's Delete question is up (it holds a brush or set by its place). */
    var brushQuestionOpen = false
        set(open) {
            field = open
            if (!open) addBrushesRead()
        }

    /**
     * Brushes read meanwhile join the library (a set of their own at the top, or a brush at the
     * top of a set), that set shown; not while the studio or the Delete question holds a brush
     * by its place, which they would move: then once that ends.
     */
    fun addBrushesRead() {
        val s = session ?: return
        if (studioOpen || brushQuestionOpen || brushesRead.isEmpty()) return
        if (shownSet == RECENT_SET) shownSet = currentBrush?.set?.toInt() ?: 0
        for (read in brushesRead.toList()) {
            s.addBrushesRead(read, shownSet.toUInt())?.let { shownSet = it.toInt() }
        }
        brushesRead.clear()
        libraryChanged()
    }

    /** All of [uri]'s bytes, read into one array of the file's own size when it is known. */
    private fun readAll(uri: android.net.Uri): ByteArray {
        val resolver = getApplication<Application>().contentResolver
        val size = runCatching { resolver.openFileDescriptor(uri, "r")?.use { it.statSize } }.getOrNull() ?: -1L
        val stream = resolver.openInputStream(uri) ?: throw java.io.IOException("no stream")
        return stream.use { input ->
            if (size in 0..Int.MAX_VALUE.toLong()) {
                val out = ByteArray(size.toInt())
                var at = 0
                while (at < out.size) {
                    val n = input.read(out, at, out.size - at)
                    if (n < 0) break
                    at += n
                }
                if (at < out.size) {
                    out.copyOf(at)
                } else {
                    // a file that grew since its size was read
                    val more = input.readBytes()
                    if (more.isEmpty()) out else out + more
                }
            } else {
                input.readBytes()
            }
        }
    }

    /**
     * Another app's brushes [bytes] as a set of brushes named after the file [name] (shown),
     * or, with no canvas open, kept until one opens; false if the file has none.
     */
    private suspend fun addOtherBrushes(bytes: ByteArray, name: String): Boolean {
        val setName = name.substringBeforeLast('.').trim()
        val s = session
        if (s == null) {
            withContext(Dispatchers.IO) {
                pendingBrushDir.mkdirs()
                File(pendingBrushDir, "${System.nanoTime()}-${setName.replace(Regex("[^A-Za-z0-9 _-]"), "_")}.other").writeBytes(bytes)
            }
            message = "Brushes will be added to the library when a canvas opens."
            return true
        }
        val set = withContext(Dispatchers.Default) { s.importOtherBrushes(bytes, setName) } ?: return false
        shownSet = set.toInt()
        libraryChanged()
        return true
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
            ?: runCatching { s.importBrushFile(shownSet.coerceAtLeast(0).toUInt(), bytes.decodeToString()) }.getOrNull()
            ?: return false
        shownSet = set.toInt()
        libraryChanged()
        return true
    }

    /** Whether a file named [name] holds another app's brushes (see [OTHER_BRUSH_EXTENSIONS]). */
    private fun isOtherBrushFile(name: String) = OTHER_BRUSH_EXTENSIONS.any { name.endsWith(".$it", ignoreCase = true) }

    /** Brush files kept while no canvas was open, added now. */
    private fun addPendingBrushes(s: CanvasSession) {
        val files = pendingBrushDir.listFiles()?.sortedBy { it.name } ?: return
        var any = false
        for (f in files) {
            // another app's (kept as .abr by earlier versions)
            val other = f.name.endsWith(".other") || f.name.endsWith(".abr")
            runCatching { if (other) s.importOtherBrushes(f.readBytes(), f.nameWithoutExtension.substringAfter('-')) else s.importBrushes(f.readBytes()) }.getOrNull()?.let {
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
    private fun isBrushFile(bytes: ByteArray): Boolean = io.github.artcraftandroid.procraft.core.isBrushZip(bytes)

    fun chooseBrush(b: BrushInfo) {
        val s = session ?: return
        val before = currentBrush
        if (s.selectBrush(b.set, b.index)) {
            if (before != null && (before.set != b.set || before.index != b.index)) lastBrush = before.set to before.index
            // the set left behind keeps showing the brush it had
            before?.let { setChoice[it.set.toInt()] = it.index.toInt() }
            currentBrush = s.currentBrush()
            setChoice[b.set.toInt()] = b.index.toInt()
            readSliders()
            keepChoices(s)
        }
    }

    /** Reads the Recent set again: painting adds to it. */
    fun readRecentBrushes() {
        recentBrushes = session?.recentBrushes().orEmpty()
    }

    /** Pins a brush of the Recent set there, or unpins it. */
    fun pinRecentBrush(b: BrushInfo, pinned: Boolean) {
        val s = session ?: return
        if (!s.pinRecentBrush(b.set, b.index, pinned)) return
        recentBrushes = s.recentBrushes()
        keepChoices(s)
    }

    /** Takes a brush out of the Recent set. */
    fun clearRecentBrush(b: BrushInfo) {
        val s = session ?: return
        if (!s.clearRecentBrush(b.set, b.index)) return
        recentBrushes = s.recentBrushes()
        keepChoices(s)
    }

    /** Empties the Recent set, its pinned brushes too. */
    fun clearRecentBrushes() {
        val s = session ?: return
        s.clearRecentBrushes()
        recentBrushes = emptyList()
        keepChoices(s)
    }

    /** A brush of the Recent set shown in its own set, chosen there. */
    fun findBrush(b: BrushInfo) {
        shownSet = b.set.toInt()
        chooseBrush(b)
        foundBrush = "${b.set}-${b.index}"
    }

    /** The brush chosen before the current one (set, place in it), for Previous brush. */
    private var lastBrush: Pair<UInt, UInt>? = null

    /** Back to the brush chosen before this one (and Previous brush again comes back here). */
    fun previousBrush() {
        val s = session ?: return
        val (set, index) = lastBrush ?: return
        val now = currentBrush
        if (s.selectBrush(set, index)) {
            lastBrush = now?.let { it.set to it.index }
            currentBrush = s.currentBrush()
            setChoice[set.toInt()] = index.toInt()
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
        viewModelScope.launch(Dispatchers.IO) {
            val kept = brushLibraryWrites.withLock { runCatching { s.keepBrushChoices(brushesDir.path) }.getOrDefault(false) }
            libraryKept(kept)
        }
    }

    /** The library couldn't be kept and that was told: not again until it is kept once more. */
    private var libraryFailing = false

    /** Tells, once until the library is kept again, that it couldn't be (the storage full, say). */
    private suspend fun libraryKept(kept: Boolean) = withContext(Dispatchers.Main) {
        if (kept) {
            libraryFailing = false
        } else if (!libraryFailing) {
            libraryFailing = true
            message = "The brush library couldn't be kept."
        }
    }

    /** Whether [b] shows highlighted: the current brush, or its set's last choice. */
    fun highlighted(b: BrushInfo): Boolean {
        val c = currentBrush
        if (c != null && c.set == b.set) return c.index == b.index
        return setChoice[b.set.toInt()] == b.index.toInt()
    }

    // the brush studio

    fun studio(key: String): Float = studioValues[key] ?: 0f

    /** The studio's brush is a dual brush, and its second part shows. */
    var studioDual by mutableStateOf(false)
        private set
    var studioSecond by mutableStateOf(false)
        private set
    /** Its two parts' sample strokes, and how they combine. */
    var studioParts by mutableStateOf<List<ImageBitmap?>>(listOf(null, null))
        private set
    var studioCombineMode by androidx.compose.runtime.mutableIntStateOf(0)
        private set
    /** The page the part not shown was left on. */
    private var otherPartTab = 0
    private var studioPartsJob: Job? = null

    /** Shows a dual brush's first or [second], each on the page it was left on. */
    fun showStudioPart(second: Boolean) {
        val s = session ?: return
        if (second == studioSecond) return
        studioSecond = second
        s.studioShowSecond(second)
        otherPartTab = studioTab.also { studioTab = otherPartTab }
        readStudio()
    }

    fun changeStudioCombineMode(mode: Int) {
        val s = session ?: return
        s.setStudioCombineMode(mode.toUInt())
        studioCombineMode = mode
        refreshStudioPreview()
        syncPad()
    }

    /** Draws the parts' sample strokes again, a moment after the last change. */
    private fun refreshStudioParts() {
        val s = session ?: return
        if (!studioDual) return
        studioPartsJob?.cancel()
        studioPartsJob = viewModelScope.launch {
            kotlinx.coroutines.delay(STUDIO_PREVIEW_DELAY_MS)
            val parts = withContext(Dispatchers.Default) {
                listOf(false, true).map { second ->
                    s.studioPartPreview(second, 680u, 222u)?.let { t ->
                        val bitmap = io.github.artcraftandroid.procraft.ui.CanvasSpace.tag(Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), Bitmap.Config.ARGB_8888))
                        bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(t.rgba))
                        bitmap.asImageBitmap()
                    }
                }
            }
            if (studioOpen) studioParts = parts
        }
    }

    /** Opens the studio on the current tool's brush. */
    fun openStudio() {
        val s = session ?: return
        // the brush library stays open under it, so Cancel and Done go back to it
        if (panel != Panel.BRUSHES) closePanels()
        studioName = s.studioBegin()
        studioDual = s.studioIsDual()
        studioSecond = false
        otherPartTab = 0
        studioCombineMode = s.studioCombineMode().toInt()
        // each visit's pad paints in white, black with the light interface
        padColor = if (lightInterface) PAD_BLACK else PAD_WHITE
        readStudio()
        studioOpen = true
    }

    /** Names the studio's brush as it is typed: Done keeps the name, Cancel puts the old one back. */
    fun renameStudio(name: String) {
        session?.setStudioName(name)?.let { studioName = it }
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

    /** Studio control [key]'s own pressure curve (none while it has none). */
    fun studioSliderCurve(key: String): List<Offset>? =
        session?.studioSliderCurve(key)?.takeIf { it.size >= 4 }?.chunked(2) { Offset(it[0], it[1]) }

    /** Gives studio control [key] its own pressure curve through [points], or takes it away (none). */
    fun setStudioSliderCurve(key: String, points: List<Offset>?) {
        session?.setStudioSliderCurve(key, points?.flatMap { listOf(it.x, it.y) } ?: emptyList())
        refreshStudioPreview()
        syncPad()
    }

    // the studio brush's shape, grain and material images

    /** The images the studio brush's sources use (white on black), by kind, for their rows; none for the round tip, no grain or a material's amount all over. */
    var studioSources by mutableStateOf<Map<SourceKind, ImageBitmap>>(emptyMap())
        private set

    /** The editor open for the studio brush's source of this kind, if one is. */
    var sourceEditing by mutableStateOf<SourceKind?>(null)

    /** The shipped shape and grain images, by name, for the source library. */
    val shapeLibrary by lazy { io.github.artcraftandroid.procraft.core.sourceLibrary(false).map { it.name to grayBitmap(it.pixels) } }
    val grainLibrary by lazy { io.github.artcraftandroid.procraft.core.sourceLibrary(true).map { it.name to grayBitmap(it.pixels) } }

    private fun readStudioSources() {
        val s = session ?: return
        val images = SourceKind.entries.mapNotNull { k -> sourceBitmap(s.studioSource(k), shape = k == SourceKind.SHAPE)?.let { k to it } }
        studioSources = images.associate { (k, b) -> k to small(b, SOURCE_ROW_PX) }
        studioSourcesBig = images.associate { (k, b) -> k to small(b, SOURCE_BOX_PX) }
    }

    /** The same images as big as the landscape studio's square shows them. */
    var studioSourcesBig by mutableStateOf<Map<SourceKind, ImageBitmap>>(emptyMap())
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

    /** The image the studio brush's [kind] of source uses now: square greyscale, white = paint; empty for the round tip, no grain or the amount all over. */
    fun studioSourcePixels(kind: SourceKind): ByteArray = session?.studioSource(kind) ?: ByteArray(0)

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

    /** Library image [index]'s place among the shipped images: none for the grains' Blank, which isn't one. */
    fun shippedOf(grain: Boolean, index: Int): Int? = if (grain) (index - 1).takeIf { it >= 0 } else index

    /** Shipped image [index] of the source library. */
    fun librarySource(grain: Boolean, index: Int): ByteArray =
        io.github.artcraftandroid.procraft.core.sourceLibrary(grain).getOrNull(index)?.pixels ?: ByteArray(0)

    /** [bitmap] fitted into the square on black, by its lightness (the shared core's). */
    private fun sourceFrom(bitmap: Bitmap): ByteArray {
        val (w, h) = bitmap.width to bitmap.height
        // straight RGBA, as the core takes it
        val argb = IntArray(w * h).also { bitmap.getPixels(it, 0, w, 0, 0, w, h) }
        val rgba = ByteArray(w * h * 4)
        for ((i, c) in argb.withIndex()) {
            rgba[i * 4] = (c shr 16).toByte()
            rgba[i * 4 + 1] = (c shr 8).toByte()
            rgba[i * 4 + 2] = c.toByte()
            rgba[i * 4 + 3] = (c ushr 24).toByte()
        }
        return io.github.artcraftandroid.procraft.core.sourceFromPicture(rgba, w.toUInt(), h.toUInt())
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

    /** The editor's tick: the studio brush's [kind] of source becomes [pixels], or shipped image [shipped]. */
    fun keepStudioSource(kind: SourceKind, pixels: ByteArray, shipped: Int?) {
        val s = session ?: return
        val changed = when {
            shipped != null -> s.setStudioShippedSource(kind, shipped.toUInt())
            pixels.isNotEmpty() -> s.setStudioSource(kind, pixels)
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
        val back = if (lightInterface) PAD_BACK_LIGHT else PAD_BACK
        p.setBackgroundColor(back, back, back, true)
        p.keepStrokes()
        syncPad()
    }

    /**
     * The pad has drawn its first frame: it shows the brush's sample scribble, flat or on the sphere
     * (drawn again here: a sphere asked for before now had nothing to draw on).
     */
    fun padReady(p: CanvasSession) {
        if (pad !== p) return
        p.showSphere(padSphere)
        p.drawSampleScribble()
    }

    /** The pad shown as a sphere with the brush's materials (the Materials page, or the 3D preview on). */
    var padSphere = false
        private set

    fun showPadSphere(shown: Boolean) {
        padSphere = shown
        pad?.showSphere(shown)
    }

    /** The pad menu's 3D preview: the pad a sphere on every page, kept from one session to the next. */
    private var pad3dState by mutableStateOf(interfacePrefs.getBoolean(PAD_3D, false))
    var pad3d: Boolean
        get() = pad3dState
        set(on) {
            pad3dState = on
            interfacePrefs.edit().putBoolean(PAD_3D, on).apply()
        }

    /** The pad paints with the brush as it is now, and what is on it is drawn again that way. */
    private fun syncPad() {
        val p = pad ?: return
        val s = session ?: return
        val c = PAD_COLORS.getOrElse(padColor) { 0xFFFFFF }
        // drawn again on the core's own thread: a slider dragged doesn't wait for it
        p.syncPad(s, ((c shr 16) and 0xFF) / 255f, ((c shr 8) and 0xFF) / 255f, (c and 0xFF) / 255f)
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
        // brushes read while the studio was open join the library now
        addBrushesRead()
    }

    /** Draws the brush's sample stroke again, a moment after the last change. */
    private fun refreshStudioPreview() {
        val s = session ?: return
        val b = currentBrush ?: return
        refreshStudioParts()
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
        val round = previewRound
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
            if (made != null && round == previewRound) {
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
    fun notchAt(size: Boolean, value: Float): Float? = session?.sliderNotchAt(size, value)

    /** Whether the slider's panel offers to save its value: not when it is on a notch or the slider is full. */
    fun canSaveNotch(size: Boolean): Boolean = session?.canSaveSliderNotch(size) == true

    /** The slider panel's button: saves the value as a notch, or takes away the notch it is on. */
    fun toggleNotch(size: Boolean) {
        val s = session ?: return
        s.toggleSliderNotch(size)
        readNotches()
        keepBrushLibrary(s)
    }

    /** The session whose library a write yet to start will keep. */
    private val libraryKeepWaiting = java.util.concurrent.atomic.AtomicReference<CanvasSession?>(null)

    /**
     * Keeps the library as it is now, with the brushes in use, in its folder, off the main thread.
     * Each write takes the library as it is when it runs, so one waiting already covers this
     * change: a run of them (sets dragged about) costs two writes of a large library, not one each,
     * and every write holds the core for as long as it reads the library.
     */
    private fun keepBrushLibrary(s: CanvasSession) {
        if (libraryKeepWaiting.getAndSet(s) === s) return
        viewModelScope.launch(Dispatchers.IO) {
            val kept = brushLibraryWrites.withLock {
                libraryKeepWaiting.compareAndSet(s, null)
                runCatching { s.keepBrushes(brushesDir.path) }.getOrDefault(false)
            }
            libraryKept(kept)
        }
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

    /** The brush's own shape as loops across its diameter (none: the round tip's circle). */
    fun brushOutline(): List<List<Float>> = session?.brushOutline() ?: emptyList()

    /** The brush's stamp as it paints, [side] pixels square, as a coverage mask (null without a canvas). */
    fun brushStamp(side: Int): androidx.compose.ui.graphics.ImageBitmap? {
        val s = session ?: return null
        val n = side.coerceIn(1, 1024)
        val alpha = s.brushStamp(n.toUInt())
        if (alpha.size < n * n) return null
        // white with the coverage as its alpha (the panel tints it)
        val colors = IntArray(n * n) { i -> (alpha[i].toInt() and 0xFF shl 24) or 0xFFFFFF }
        val bitmap = android.graphics.Bitmap.createBitmap(colors, n, n, android.graphics.Bitmap.Config.ARGB_8888)
        return bitmap.asImageBitmap()
    }

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
        val before = colourHistory.previous(colorRgb().toUInt())?.toInt() ?: return
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
        colourHistory.clear()
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
        /** A model: OBJ, its material file and its textures, zipped. */
        MODEL_OBJ("zip", "application/zip"),
        /** A model as a USDZ package. */
        MODEL_USDZ("usdz", "model/vnd.usdz+zip"),
        /** A model's texture sets, a PNG each. */
        MODEL_TEXTURES("png", "image/png"),
        /** The model as shown, as a PNG, a JPEG or a TIFF. */
        MODEL_RENDER("png", "image/png"),
        MODEL_JPEG("jpg", "image/jpeg"),
        MODEL_TIFF("tif", "image/tiff"),
    }

    /**
     * The artwork written as [kind] into the cache, to share: one file, or a PNG per layer. The
     * export card shows how far it has come (as the core tells it) and a check once made; its
     * Cancel leaves the files unshared (none returned). A failure is told.
     */
    suspend fun export(kind: Export, pdfQuality: Int = 100): List<File> =
        withExportCard("The artwork couldn't be exported.") { s -> exportFiles(s, kind, pdfQuality).takeIf { it.isNotEmpty() } }.orEmpty()

    /**
     * Runs [work], an export by the core, with the export card showing how far it has come (as the
     * core tells it) and a check once made: what it made, or null if it was cancelled (its result
     * left unshared) or made nothing ([failed] is then told). One export at a time.
     */
    private suspend fun <T : Any> withExportCard(failed: String, work: suspend (CanvasSession) -> T?): T? {
        val s = session ?: return null
        if (exportShown != null || timeLapseExporting) return null
        s.resetExportProgress()
        return withCard(failed, s::exportProgress) { work(s) }
    }

    /** As [withExportCard], how far it has come told by [progress] (0..1). */
    private suspend fun <T : Any> withCard(failed: String, progress: () -> Float, work: suspend () -> T?): T? {
        if (exportShown != null || timeLapseExporting) return null
        exportCancelled = false
        exportShown = ExportShown(0f)
        val following = viewModelScope.launch {
            while (true) {
                exportShown = ExportShown(progress())
                kotlinx.coroutines.delay(EXPORT_POLL_MS)
            }
        }
        try {
            val made = work()
            following.cancel()
            if (exportCancelled) return null
            if (made == null) {
                showNotice(failed, warning = true)
                return null
            }
            exportShown = ExportShown(1f, done = true)
            kotlinx.coroutines.delay(EXPORT_DONE_MS)
            return made
        } finally {
            following.cancel()
            exportShown = null
        }
    }

    private suspend fun exportFiles(s: CanvasSession, kind: Export, pdfQuality: Int): List<File> =
        withContext(Dispatchers.IO) {
            val dir = File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }
            val base = io.github.artcraftandroid.procraft.core.exportFileName(s.name(), false)
            val file = File(dir, "$base.${kind.extension}")
            try {
                when (kind) {
                    Export.PSD -> s.exportPsd(file.path)
                    Export.PDF -> s.exportPdf(file.path, false, pdfQuality.toUByte())
                    Export.TIFF -> s.exportTiff(file.path)
                    Export.MODEL_OBJ -> s.exportModelObj(file.path)
                    Export.MODEL_USDZ -> s.exportModelUsdz(file.path)
                    Export.MODEL_RENDER -> s.exportModelRender(file.path, false)
                    Export.MODEL_JPEG -> s.exportModelJpeg(file.path)
                    Export.MODEL_TIFF -> s.exportModelTiff(file.path)
                    Export.MODEL_TEXTURES -> {
                        val folder = File(dir, base).apply { deleteRecursively(); mkdirs() }
                        return@withContext s.exportModelTextures(folder.path).map { File(it) }
                    }
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

    /** The model's turntable being set up to share (Share Image > Animated ...), if it is. */
    var modelAnimatedExport by mutableStateOf<Animated?>(null)

    /** How the model's turntable goes, and its size: 720p, 1080p, 4K or Square (kept while the app runs). */
    var turntable by mutableStateOf(io.github.artcraftandroid.procraft.core.Turntable(swing = false, seconds = 2f, zoom = 0.5f, ease = 1f, environment = true))
    var turntableSize by mutableStateOf(0)

    /** The turntable's picture size for [turntableSize]: video's 16:9 sizes across, or a square. */
    fun turntablePixels(): Pair<Int, Int> = TURNTABLE_SIZES.getOrElse(turntableSize) { TURNTABLE_SIZES[0] }

    /** The turntable's frames, each [side] square, [fps] a second, for the preview. */
    suspend fun turntablePreview(side: Int, fps: Int): List<Bitmap> {
        val s = session ?: return emptyList()
        val t = turntable
        return withContext(Dispatchers.Default) {
            val count = (t.seconds * fps).roundToInt().coerceAtLeast(1)
            (0 until count).mapNotNull { i ->
                val p = s.modelTurntablePicture(i.toUInt(), count.toUInt(), t, side.toUInt(), side.toUInt(), false) ?: return@mapNotNull null
                Bitmap.createBitmap(p.width.toInt(), p.height.toInt(), Bitmap.Config.ARGB_8888).apply { copyPixelsFromBuffer(ByteBuffer.wrap(p.rgba)) }
            }
        }
    }

    /** [exportModelAnimation] with the export card, how far it has come shown; Cancel stops a video. */
    suspend fun shareModelAnimation(format: Animated): File? {
        val s = session ?: return null
        s.resetExportProgress()
        videoDone = 0f
        return withCard("The animation could not be exported.", { if (format.video) videoDone else s.exportProgress() }) {
            exportModelAnimation(format) { exportCancelled }
        }
    }

    /** How far a video being made for the export card has come (its frames written, 0..1). */
    @Volatile private var videoDone = 0f

    /** The model's turntable written as [format] into the cache, to share; null if it can't be made (or [cancelled] said to stop). */
    suspend fun exportModelAnimation(format: Animated, cancelled: () -> Boolean = { false }): File? {
        val s = session ?: return null
        val t = turntable
        val (w, h) = turntablePixels()
        return withContext(Dispatchers.IO) {
            val dir = File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }
            val base = io.github.artcraftandroid.procraft.core.exportFileName(s.name(), false)
            val file = File(dir, "$base.${format.extension}")
            try {
                when (format) {
                    Animated.GIF -> s.exportModelGif(file.path, t, w.toUInt(), h.toUInt(), TURNTABLE_FPS.toFloat())
                    Animated.PNG -> s.exportModelApng(file.path, t, w.toUInt(), h.toUInt(), TURNTABLE_FPS.toFloat())
                    Animated.MP4, Animated.HEVC -> {
                        val mime = if (format == Animated.HEVC) android.media.MediaFormat.MIMETYPE_VIDEO_HEVC else android.media.MediaFormat.MIMETYPE_VIDEO_AVC
                        val (vw, vh) = VideoWriter.fit(w, h, maxOf(w, h), mime) ?: return@withContext null
                        val count = (t.seconds * TURNTABLE_FPS).roundToInt().coerceAtLeast(1)
                        val ok = VideoWriter.write(file, vw, vh, TURNTABLE_FPS.toFloat(), count, mime) { i, pixels ->
                            if (cancelled()) return@write false
                            videoDone = i.toFloat() / count
                            val p = s.modelTurntablePicture(i.toUInt(), count.toUInt(), t, vw.toUInt(), vh.toUInt(), true) ?: return@write false
                            // already ARGB, little-endian
                            ByteBuffer.wrap(p.rgba).order(java.nio.ByteOrder.LITTLE_ENDIAN).asIntBuffer().get(pixels, 0, vw * vh)
                            true
                        }
                        if (!ok) return@withContext null
                    }
                }
                file
            } catch (e: MobileException) {
                Log.e(TAG, "exporting the turntable", e)
                null
            }
        }
    }

    /** The layers as an animation, a frame each: the formats, by name, extension and type. */
    enum class Animated(val title: String, val extension: String, val mime: String) {
        GIF("Animated GIF", "gif", "image/gif"),
        PNG("Animated PNG", "png", "image/png"),
        MP4("Animated MP4", "mp4", "video/mp4"),
        HEVC("Animated HEVC", "mp4", "video/mp4"),
        ;

        /** Made by the system's encoder a frame at a time (its progress counted here), not by the core. */
        val video get() = this == MP4 || this == HEVC
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

    /** The artwork's name, or the name an unnamed one shows. */
    fun artworkTitle(): String = session?.name()?.trim()?.takeIf { it.isNotEmpty() } ?: "Untitled Artwork"

    /** One animation export at a time: they write the same file. */
    private val animationExports = Mutex()

    /** The artwork's own frame rate, which an animated export starts at: Animation Assist's, else 15. */
    fun artworkFps(): Float = (session?.animationSettings()?.fps?.toInt() ?: ANIMATION_FPS).toFloat()

    /**
     * The animation written as [format] into the cache, to share, [fps] frames a second: Web Ready
     * fits 480 pixels, else the canvas size (a video as large as the encoder takes, up to 4096).
     */
    suspend fun exportAnimation(
        format: Animated,
        webReady: Boolean,
        dither: Boolean,
        perFramePalette: Boolean,
        transparent: Boolean,
        fps: Float,
        cancelled: () -> Boolean = { false },
    ): File? {
        val s = session ?: return null
        return animationExports.withLock { withContext(Dispatchers.IO) {
            val dir = File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }
            val base = io.github.artcraftandroid.procraft.core.exportFileName(s.name(), false)
            val file = File(dir, "$base.${format.extension}")
            val side = if (webReady) WEB_READY_SIDE else 0
            try {
                when (format) {
                    Animated.GIF -> s.exportGif(file.path, side.toUInt(), fps, dither, perFramePalette, transparent)
                    Animated.PNG -> s.exportApng(file.path, side.toUInt(), fps, transparent)
                    Animated.MP4, Animated.HEVC -> {
                        val mime = if (format == Animated.HEVC) android.media.MediaFormat.MIMETYPE_VIDEO_HEVC else android.media.MediaFormat.MIMETYPE_VIDEO_AVC
                        val (cw, ch) = canvasSize()
                        val (w, h) = VideoWriter.fit(cw, ch, if (webReady) WEB_READY_SIDE else MAX_VIDEO_SIDE, mime) ?: return@withContext null
                        val frame = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
                        val paint = android.graphics.Paint(android.graphics.Paint.FILTER_BITMAP_FLAG)
                        val ok = try {
                            val order = s.animationSequence()
                            VideoWriter.write(file, w, h, fps, order.size, mime) { i, pixels ->
                                if (cancelled()) return@write false
                                videoDone = i.toFloat() / order.size
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
        } }
    }

    /** [exportAnimation] with the export card (as every export has), how far it has come shown; Cancel stops a video. */
    suspend fun shareAnimation(format: Animated, webReady: Boolean, dither: Boolean, perFramePalette: Boolean, transparent: Boolean, fps: Float): File? {
        val s = session ?: return null
        s.resetExportProgress()
        videoDone = 0f
        return withCard("The animation could not be exported.", { if (format.video) videoDone else s.exportProgress() }) {
            exportAnimation(format, webReady, dither, perFramePalette, transparent, fps) { exportCancelled }
        }
    }

    /** The flattened picture as a JPEG or PNG in the cache, to share, with the export card. */
    suspend fun exportImage(jpeg: Boolean): File? = withExportCard("The picture couldn't be exported.") { s ->
        withContext(Dispatchers.IO) {
            val dir = File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }
            val base = io.github.artcraftandroid.procraft.core.exportFileName(s.name(), false)
            val file = File(dir, if (jpeg) "$base.jpg" else "$base.png")
            try {
                if (jpeg) s.exportJpeg(file.path) else s.exportPng(file.path)
            } catch (e: MobileException) {
                Log.e(TAG, "exporting", e)
                return@withContext null
            }
            file
        }
    }

    // ---- palettes ----

    /** The palettes as the core keeps them now. */
    private fun refreshPalettes() {
        palettes = paletteStore.list().map { p -> Palette(p.name, p.colors.map { it?.toInt() }) }
        // a change that couldn't be written out (the storage full, say) told, once until one is
        if (paletteStore.takeFailure() != null) message = "The palettes couldn't be kept."
        defaultPalette = paletteStore.defaultPalette().toInt()
    }

    fun addPalette() {
        paletteStore.add()
        refreshPalettes()
    }

    /** Our example palettes back, those missing added at the end. */
    fun restoreExamplePalettes() {
        paletteStore.restoreExamples()
        refreshPalettes()
    }

    /** A palette of the main colours in the picture at [uri], light to dark, added at the end. */
    fun paletteFromImage(uri: android.net.Uri) {
        val resolver = getApplication<Application>().contentResolver
        viewModelScope.launch {
            val added = withContext(Dispatchers.IO) {
                runCatching {
                    val bounds = android.graphics.BitmapFactory.Options().apply { inJustDecodeBounds = true }
                    resolver.openInputStream(uri)?.use { android.graphics.BitmapFactory.decodeStream(it, null, bounds) }
                    // read small: a few hundred pixels across is plenty to find its colours
                    var sample = 1
                    while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= 256) sample *= 2
                    val bitmap = resolver.openInputStream(uri)?.use {
                        android.graphics.BitmapFactory.decodeStream(it, null, android.graphics.BitmapFactory.Options().apply { inSampleSize = sample })
                    } ?: return@runCatching false
                    val pixels = IntArray(bitmap.width * bitmap.height).also { bitmap.getPixels(it, 0, bitmap.width, 0, 0, bitmap.width, bitmap.height) }
                    paletteStore.addFromPixels(pixels.map { it.toUInt() })
                }.getOrNull() == true
            }
            if (!added) {
                message = "This picture can't be read."
                return@launch
            }
            refreshPalettes()
        }
    }

    /**
     * Reads a palette file at [uri] (ours, or another program's colour swatch or swatch exchange
     * file) into a new palette at the end; false if it isn't one.
     */
    suspend fun importSwatches(uri: android.net.Uri): Boolean {
        val resolver = getApplication<Application>().contentResolver
        val added = withContext(Dispatchers.IO) {
            runCatching {
                val bytes = resolver.openInputStream(uri)?.use { it.readBytes() } ?: return@runCatching false
                paletteStore.importFile(bytes, fileName(uri))
            }.getOrNull() == true
        }
        if (added) refreshPalettes()
        return added
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
                val bytes = paletteStore.swatchesFile(i.toUInt()) ?: return@runCatching null
                val base = p.name.filter { it.isLetterOrDigit() || it in " _-" }.trim().ifEmpty { "Palette" }
                File(File(getApplication<Application>().cacheDir, "exports").apply { mkdirs() }, "$base.swatches").also { it.writeBytes(bytes) }
            }.onFailure { Log.e(TAG, "sharing a palette", it) }.getOrNull()
        }
    }

    fun renamePalette(i: Int, name: String) {
        paletteStore.rename(i.toUInt(), name)
        refreshPalettes()
    }

    fun duplicatePalette(i: Int) {
        paletteStore.duplicate(i.toUInt())
        refreshPalettes()
    }

    fun deletePalette(i: Int) {
        paletteStore.delete(i.toUInt())
        refreshPalettes()
    }

    fun chooseDefaultPalette(i: Int) {
        paletteStore.chooseDefault(i.toUInt())
        refreshPalettes()
    }

    /** Puts [rgb] in a palette's [slot], or empties it when null. */
    fun setSwatch(i: Int, slot: Int, rgb: Int?) {
        paletteStore.setSwatch(i.toUInt(), slot.toUInt(), rgb?.toUInt())
        refreshPalettes()
    }

    private fun applyColor() {
        val c = colorRgb()
        val (r, g, b) = Triple(((c shr 16) and 0xFF) / 255f, ((c shr 8) and 0xFF) / 255f, (c and 0xFF) / 255f)
        session?.setColor(r, g, b)
        if (colorNames) nameColorSoon()
        // recoloring, the area under the reticle takes the new colour
        val s = session
        if (recoloring && s != null) pourLater("colour") { s.recolorColour(r, g, b) }
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

    /** The library's brush swiped open to its buttons ("set-index"), if one is. */
    var swipedBrush by mutableStateOf<String?>(null)

    fun toggle(p: Panel) {
        // with a brush swiped open, its button only closes the swipe
        if (p == Panel.BRUSHES && panel == Panel.BRUSHES && swipedBrush != null) {
            swipedBrush = null
            return
        }
        feedback()
        switchModes(ModeButton.Panel(p == Panel.COLOR))
        // the colour button with the panel floating: it goes back in its place
        if (p == Panel.COLOR && floatingColorsAt != null) {
            floatingColorsAt = null
            panel = Panel.NONE
        }
        panel = if (panel == p) Panel.NONE else p
        if (panel == Panel.LAYERS) refreshThumbnails()
    }

    /** The modes open now, as the core's [modeSwitch] takes them. */
    private fun openModes() = Modes(
        quickshape = quickShape != null,
        filling = dropBand == DropBand.FILLING,
        recolor = recoloring,
        layerOpacity = opacityLayer != null,
        transform = transforming,
        selection = selecting,
        adjustment = adjustment != null,
    )

    /** [button] pressed: the modes it leaves behind end first (keeping what they made), as the core says; true if it goes on to do what it does. */
    private fun switchModes(button: ModeButton): Boolean {
        val change = modeSwitch(button, openModes())
        val end = change.end
        if (end.quickshape) endQuickShape()
        if (end.filling) endDropBand()
        if (end.recolor) endRecolor()
        if (end.layerOpacity) endLayerOpacity()
        if (end.transform) endTransform(true)
        if (end.selection) closeSelecting()
        if (end.adjustment) endAdjustment(true)
        return change.goesOn
    }

    fun closePanels() {
        panel = Panel.NONE
        swipedBrush = null
    }

    /** A tap outside the open panel closes it; with a brush swiped open, only the swipe. */
    fun tapOutsidePanel() {
        if (panel == Panel.BRUSHES && swipedBrush != null) swipedBrush = null else closePanels()
    }

    /** Closes the open panel by fading it away, as picking something that takes the screen does. */
    fun fadePanels() {
        if (panel != Panel.NONE) panelFading = true
        panel = Panel.NONE
    }

    fun undo() {
        feedback()
        // a finished text's step goes back to editing that text first, as a step of its own
        if (textInfo == null && !transforming) session?.textUndoLayer()?.let { if (resumeText(it)) return }
        // editing text, a change to its box's width goes back a step at a time, still editing
        if (textInfo != null && stepTextBox(undo = true)) return
        // undoing while editing text otherwise undoes the whole edit
        if (textInfo != null) {
            textInfo = null
            textStyleOpen = false
        }
        if (transforming) {
            transformBox = session?.transformApply(TransformAction.UNDO) ?: transformBox
            return
        }
        // recoloring, the core takes the fill under the reticle back first (after the fills queued)
        val could = canUndo
        val name = session?.let { s -> if (recoloring) pourNow { s.undo() } else s.undo() }
        if (could || name != null) showNotice("Undo" + name?.let { " $it" }.orEmpty())
        followQuickShape()
        // a model's tile chosen before comes back, with its channel's colour
        if (modelOpen) followModelChannel()
        // cloning, the core put the source back where it was before the stroke
        if (adjustment != null) readAdjust()
        refresh()
        // the thumbnails of the layers it put back (only those) read again
        if (panel == Panel.LAYERS) refreshThumbnails()
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
        if (textInfo != null && stepTextBox(undo = false)) return
        // text being edited is kept first
        val editing = textInfo != null
        textInfo = null
        textStyleOpen = false
        val could = canRedo
        // a text edit redone is edited again
        val text = if (!editing) session?.textRedoLayer() else null
        // recoloring, the core steps the open fill aside and makes it again over what comes back
        val name = session?.let { s -> if (recoloring) pourNow { s.redo() } else s.redo() }
        if (could) showNotice("Redo" + name?.let { " $it" }.orEmpty())
        followQuickShape()
        if (modelOpen) followModelChannel()
        refresh()
        if (panel == Panel.LAYERS) refreshThumbnails()
        if (adjustment != null) readAdjust()
        text?.let { resumeText(it) }
    }

    // ---- adjustments: the shared core keeps the one open and its values (crates/core adjusting.rs); these show them ----

    /** The adjustment's values as the core has them now. */
    private fun readAdjust() {
        val a = session?.adjustmentShown() ?: return
        adjustment = a.adjustment
        pluginFilter = a.plugin
        adjustAmount = a.amount
        adjustPencil = a.pencil
        adjustPreview = a.preview
        hue50 = a.hsb.getOrElse(0) { 0.5f }
        saturation50 = a.hsb.getOrElse(1) { 0.5f }
        brightness50 = a.hsb.getOrElse(2) { 0.5f }
        balanceTone = a.balanceTone.toInt()
        balance = a.balance
        curveChannel = a.curveChannel.toInt()
        curves = a.curves.map { c -> List(c.size / 2) { Offset(c[it * 2], c[it * 2 + 1]) } }
        gradientIndex = a.gradient.toInt()
        noiseKind = a.noiseKind
        noiseScale = a.noise.getOrElse(0) { 0f }
        noiseOctaves = a.noise.getOrElse(1) { 0f }
        noiseTurbulence = a.noise.getOrElse(2) { 0.7f }
        noiseMulti = a.noiseMulti
        noiseAdded = a.noiseAdded
        bloomTransition = a.bloom.getOrElse(0) { 1f }
        bloomSize = a.bloom.getOrElse(1) { 0.3f }
        bloomBurn = a.bloom.getOrElse(2) { 0.35f }
        halftoneKind = a.halftoneKind
        chromaKind = a.chromaKind
        chromaValues = chromaValues + (a.chromaKind to a.chroma)
        glitchKind = a.glitchKind
        glitchValues = glitchValues + (a.glitchKind to a.glitch)
        liquifyKind = a.liquifyKind
        liquifySize = a.liquify.getOrElse(0) { 0.5f }
        liquifyPressure = a.liquify.getOrElse(1) { 1f }
        liquifyDistortion = a.liquify.getOrElse(2) { 0f }
        liquifyMomentum = a.liquify.getOrElse(3) { 0.8f }
        liquifyAmount = a.liquifyAmount
        liquified = a.liquified
        clonePuck = a.clonePuck?.let { windowToCanvas(Offset(it[0], it[1])) }
        perspectiveKind = a.perspectiveKind
        perspectivePuck = a.perspectivePuck?.takeIf { it.size == 4 }?.let { p ->
            val origin = windowToCanvas(Offset(p[0], p[1]))
            val ahead = windowToCanvas(Offset(p[2], p[3]))
            if (origin != null && ahead != null) origin to ahead else null
        }
    }

    /** Opens adjustment [a] on the current layer (keeping one already open). */
    fun startAdjustment(a: Adjustment) {
        val s = session ?: return
        if (!currentTakesChanges()) return
        switchModes(ModeButton.Adjustment)
        // the list fades away rather than going at once
        if (panel == Panel.ADJUSTMENTS) panelFading = true
        panel = Panel.NONE
        if (!s.adjustmentStart(a)) return
        adjustModesOpen = false
        liquifyPopup = 0
        readAdjust()
        // cloning paints with the brush: its sliders show
        if (a == Adjustment.CLONE) readSliders()
    }

    /**
     * Opens the plugin's filter [filter] on the current layer. The core saves the artwork ahead of the
     * session's first one and compiles it, which isn't for the main thread.
     */
    fun startPluginFilter(filter: io.github.artcraftandroid.procraft.core.PluginInfo) {
        val s = session ?: return
        if (!currentTakesChanges()) return
        switchModes(ModeButton.Adjustment)
        if (panel == Panel.ADJUSTMENTS) panelFading = true
        panel = Panel.NONE
        val dir = artworkDir?.path
        viewModelScope.launch {
            val why = withContext(Dispatchers.IO) { s.pluginFilterStart(plugins, filter.id, dir) }
            if (why != null) showNotice(why, warning = true)
            adjustModesOpen = false
            liquifyPopup = 0
            readAdjust()
            refresh()
            // stopped already: no panel comes up to look for that
            if (adjustment == null) s.takeFailure()?.let { showNotice(it, warning = true) }
        }
    }

    fun setPluginFilterSlider(key: String, value: Float) {
        session?.pluginFilterSlider(key, value, false)
        readAdjust()
    }

    /** The finger left the slider [key]: a filter too slow to follow it redraws now. */
    fun letGoPluginFilter(key: String) {
        val slider = pluginFilter?.controls?.firstNotNullOfOrNull { (it as? io.github.artcraftandroid.procraft.core.PluginControl.Slider)?.control?.takeIf { s -> s.key == key } }
        if (slider != null) session?.pluginFilterSlider(key, slider.value, true)
    }

    fun setPluginFilterSwitch(key: String, on: Boolean) {
        session?.pluginFilterSwitch(key, on)
        readAdjust()
    }

    /** A filter that stopped was closed by the core, which says why. */
    fun checkPluginFilter() {
        val s = session ?: return
        if (adjustment != Adjustment.PLUGIN || s.adjustmentShown().adjustment != null) return
        s.takeFailure()?.let { showNotice(it, warning = true) }
        adjustMenuAt = null
        readAdjust()
        refresh()
    }

    /** Ends the adjustment: [commit] keeps it as one undo step, otherwise the layer goes back. */
    fun endAdjustment(commit: Boolean) {
        if (adjustment == null) return
        // a gradient being edited is kept as it is
        if (gradientEditing != null) endGradientEdit()
        adjustMenuAt = null
        val painted = adjustPencil
        session?.adjustmentEnd(commit)
        if (commit) showNotice("Adjustment applied")
        adjustModesOpen = false
        readAdjust()
        adjustment = null
        clonePuck = null
        perspectivePuck = null
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
        session?.adjustmentPencil(true)
        readAdjust()
        readSliders()
    }

    /** The canvas's (shared) touches changed the adjustment: its values shown as they are now. */
    fun adjustTouched() {
        adjustModesOpen = false
        readAdjust()
    }

    /** Cloning, the source as the core has it while the brush paints. */
    fun followClonePuck() {
        clonePuck = session?.adjustmentShown()?.clonePuck?.let { windowToCanvas(Offset(it[0], it[1])) }
    }

    fun chooseBalanceTone(tone: Int) {
        session?.adjustmentBalanceTone(tone.coerceIn(0, 2).toUInt())
        readAdjust()
    }

    /** Slider [k] (red, green, blue) of the tone shown moved to [value] (0..1). */
    fun setBalance(k: Int, value: Float) {
        session?.adjustmentBalance(k.coerceIn(0, 2).toUInt(), value)
        readAdjust()
    }

    fun chooseCurveChannel(channel: Int) {
        session?.adjustmentCurveChannel(channel.coerceIn(0, 3).toUInt())
        readAdjust()
    }

    /** Adds a point to the channel shown; returns its index. */
    fun addCurvePoint(p: Offset): Int {
        val i = session?.adjustmentCurveAdd(p.x, p.y)?.toInt() ?: -1
        readAdjust()
        return i
    }

    /** Moves point [index] of the channel shown to [p], kept between its neighbours; returns its index. */
    fun moveCurvePoint(index: Int, p: Offset): Int {
        if (index >= 0) session?.adjustmentCurveMove(index.toUInt(), p.x, p.y)
        readAdjust()
        return index
    }

    /** Takes point [index] off the channel shown (the curve keeps two at least). */
    fun deleteCurvePoint(index: Int) {
        if (index >= 0) session?.adjustmentCurveDelete(index.toUInt())
        readAdjust()
    }

    /** The curve of [channel] as points to draw (x, height). */
    fun curveShape(channel: Int): List<Offset> {
        val heights = session?.adjustmentCurveShape(channel.coerceIn(0, 3).toUInt()) ?: return emptyList()
        return heights.mapIndexed { i, h -> Offset(i / 255f, h) }
    }

    fun chooseGradient(i: Int) {
        gradientIndex = i.coerceIn(0, gradients.size - 1)
        applyGradient()
    }

    /** A new gradient, black to white, just after the chosen one: chosen and opened to edit. */
    fun newGradient() {
        val at = gradientLibrary.addAfter(gradientIndex.toUInt()).toInt()
        gradients = readGradients()
        gradientIndex = at
        gradientEditing = at
        applyGradient()
    }

    /** Opens gradient [i] (chosen) to edit. */
    fun editGradient(i: Int) {
        chooseGradient(i)
        gradientEditing = gradientIndex
    }

    /** Done editing (each change was kept as it was made). */
    fun endGradientEdit() {
        gradientEditing = null
    }

    fun deleteGradient(i: Int) {
        if (gradients.size <= 1 || i !in gradients.indices) return
        gradientLibrary.delete(i.toUInt())
        gradients = readGradients()
        if (gradientIndex > i || gradientIndex >= gradients.size) gradientIndex = (gradientIndex - 1).coerceAtLeast(0)
        applyGradient()
    }

    /** A copy of gradient [i] just after it, chosen. */
    fun duplicateGradient(i: Int) {
        if (i !in gradients.indices) return
        gradientIndex = gradientLibrary.duplicate(i.toUInt()).toInt()
        gradients = readGradients()
        applyGradient()
    }

    /** The gradient being edited changed by [f] (in the shared library), and mapped by again. */
    private fun changeEdited(f: (UInt) -> Unit) {
        val i = gradientEditing ?: return
        if (i !in gradients.indices) return
        f(i.toUInt())
        gradients = readGradients()
        applyGradient()
    }

    fun renameGradient(name: String) = changeEdited { gradientLibrary.rename(it, name) }

    /** A stop at [position] (0..1), coloured as the gradient was there; returns its index. */
    fun addGradientStop(position: Float): Int {
        var stop = -1
        changeEdited { stop = gradientLibrary.addStop(it, position).toInt() }
        return stop
    }

    fun moveGradientStop(i: Int, position: Float) = changeEdited { gradientLibrary.moveStop(it, i.toUInt(), position) }

    fun setGradientStopColor(i: Int, rgb: Int) = changeEdited { gradientLibrary.setStopColour(it, i.toUInt(), (rgb and 0xFFFFFF).toUInt()) }

    /** Takes a stop out; a gradient keeps two. */
    fun deleteGradientStop(i: Int) = changeEdited { gradientLibrary.deleteStop(it, i.toUInt()) }

    // noise, bloom, halftone and chromatic aberration's toolbars

    fun chooseNoiseKind(kind: NoiseKind) {
        session?.adjustmentNoiseKind(kind)
        readAdjust()
    }

    /** Noise's slider [index] (scale, octaves, turbulence) moved to [value]. */
    fun setNoiseValue(index: Int, value: Float) {
        session?.adjustmentNoiseValue(index.coerceIn(0, 2).toUInt(), value)
        readAdjust()
    }

    fun chooseNoiseMulti(multi: Boolean) {
        session?.adjustmentNoiseWays(multi, noiseAdded)
        readAdjust()
    }

    fun toggleNoiseAdded() {
        session?.adjustmentNoiseWays(noiseMulti, !noiseAdded)
        readAdjust()
    }

    /** Bloom's slider [index] (transition, size, burn) moved to [value]. */
    fun setBloomValue(index: Int, value: Float) {
        session?.adjustmentBloomValue(index.coerceIn(0, 2).toUInt(), value)
        readAdjust()
    }

    fun chooseHalftoneKind(kind: HalftoneKind) {
        session?.adjustmentHalftoneKind(kind)
        readAdjust()
    }

    fun chooseChromaKind(kind: ChromaticKind) {
        session?.adjustmentChromaKind(kind)
        readAdjust()
    }

    /** Perspective blur all round its puck, or only towards its arrow. */
    fun choosePerspectiveKind(kind: PerspectiveKind) {
        session?.adjustmentPerspectiveKind(kind)
        readAdjust()
    }

    /** Chromatic aberration's slider [index] of the kind shown moved to [value]. */
    fun setChromaValue(index: Int, value: Float) {
        if (index < 0) return
        session?.adjustmentChromaValue(index.toUInt(), value)
        readAdjust()
    }

    fun chooseGlitchKind(kind: GlitchKind) {
        session?.adjustmentGlitchKind(kind)
        readAdjust()
    }

    /** Slider [index] of the glitch kind shown moved to [value] (0..1, or −1..1 for diverge's shifts). */
    fun setGlitchValue(index: Int, value: Float) {
        if (index < 0) return
        session?.adjustmentGlitchValue(index.toUInt(), value)
        readAdjust()
    }

    /** Gradient Map maps by the gradient chosen (also after the library changed). */
    private fun applyGradient() {
        session?.adjustmentGradient(gradientIndex.toUInt())
    }

    // the menu a tap on the canvas opens during an adjustment

    /** Keeps what the adjustment did as one undo step and starts it again from there. */
    fun applyAdjustment() {
        if (adjustment == null) return
        adjustMenuAt = null
        session?.adjustmentApply()
        showNotice("Adjustment applied")
        readAdjust()
        refresh()
        if (panel == Panel.LAYERS) refreshThumbnails()
    }

    /** Takes back the adjustment's changes, or with none, the last one kept. */
    fun undoAdjustment() {
        if (adjustment == null) return
        session?.adjustmentUndo()
        readAdjust()
        refresh()
        if (panel == Panel.LAYERS) refreshThumbnails()
    }

    fun togglePreview() {
        session?.adjustmentPreview(!adjustPreview)
        readAdjust()
        adjustMenuAt = null
    }

    fun cancelAdjustment() = endAdjustment(false)

    /** Every value back to where the adjustment started. */
    fun resetAdjustment() {
        if (adjustment == null) return
        val painted = adjustPencil
        session?.adjustmentReset()
        readAdjust()
        if (painted && !adjustPencil) readSliders()
        refresh()
    }

    fun chooseLiquifyKind(kind: LiquifyKind) {
        session?.adjustmentLiquifyKind(kind)
        liquifyPopup = 0
        readAdjust()
    }

    /** Liquify's slider [index] (size, pressure, distortion, momentum) moved to [value]. */
    fun setLiquifyValue(index: Int, value: Float) {
        session?.adjustmentLiquifyValue(index.coerceIn(0, 3).toUInt(), value)
        readAdjust()
    }

    fun changeLiquifyAmount(amount: Float) {
        session?.adjustmentLiquifyAmount(amount)
        readAdjust()
    }

    fun resetLiquify() {
        session?.adjustmentLiquifyReset()
        liquifyPopup = 0
        readAdjust()
    }

    fun setHsb(h: Float, s: Float, b: Float) {
        session?.adjustmentHsb(h, s, b)
        readAdjust()
    }

    /** The Adjustments button: ends Layer Opacity, Recolor or the open adjustment, else opens its menu. */
    fun adjustmentsPressed() {
        if (switchModes(ModeButton.Adjustments)) toggle(Panel.ADJUSTMENTS)
    }

    // ---- selection ----

    /** The selection button: opens or closes the selection tool (a selection made stays). */
    fun toggleSelecting() {
        // open, it closes; else the other modes end
        if (!switchModes(ModeButton.Selection)) return
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

    /** How a selection joins the one there is: selecting by colour always adds (Remove greyed). */
    private val join
        get() = if (selectionRemoves && selectionKind != SelectionKind.AUTOMATIC) SelectionJoin.REMOVE else SelectionJoin.ADD

    /** How the canvas's (shared) touches select while the selection tool is open; none otherwise. */
    fun selectTouch(): io.github.artcraftandroid.procraft.core.SelectTouch? {
        if (!selecting) return null
        val kind = when (selectionKind) {
            SelectionKind.AUTOMATIC -> io.github.artcraftandroid.procraft.core.SelectKind.AUTOMATIC
            SelectionKind.FREEHAND -> io.github.artcraftandroid.procraft.core.SelectKind.FREEHAND
            SelectionKind.RECTANGLE -> io.github.artcraftandroid.procraft.core.SelectKind.RECTANGLE
            SelectionKind.ELLIPSE -> io.github.artcraftandroid.procraft.core.SelectKind.ELLIPSE
        }
        return io.github.artcraftandroid.procraft.core.SelectTouch(kind, join, selectionThreshold)
    }

    /** The freehand path as the touches have drawn it (canvas pixels). */
    fun freehandChanged() {
        freehand = session?.freehandPath().orEmpty().chunked(2).mapNotNull { (x, y) -> windowToCanvas(Offset(x, y)) }
    }

    /** The touches made a selection (or a shape dragged out ended): Color Fill fills it. */
    fun selectionTouched() {
        shapeDrag = null
        selectionChanged()
    }

    /** A colour selection's threshold slid from window point [p]: shown as it goes; [done], kept for the next one. */
    fun colourSlid(p: Offset, threshold: Float, done: Boolean) {
        if (!done) {
            selectByColour(p, threshold, live = true)
            return
        }
        // the touches made the selection as it was let go
        thresholdDrag = null
        selectionThreshold = threshold
        transformPrefs.edit().putFloat(SELECTION_THRESHOLD, threshold).apply()
    }

    /** Closes the freehand path into a selection (a path of fewer than three points is dropped). */
    fun closeFreehand() {
        val s = session ?: return
        val drawn = freehand.isNotEmpty()
        freehand = emptyList()
        s.closeFreehand()
        if (drawn) selectionChanged()
    }

    /** Selects by colour at window point [p] with [threshold], shown filled while the threshold is dragged. */
    private fun selectByColour(p: Offset, threshold: Float, live: Boolean) {
        val s = session ?: return
        thresholdDrag = if (live) p to threshold else null
        s.setSelectionDisplay(if (live) SelectionDisplay.FILL else SelectionDisplay.TINT)
        s.selectByColour(p.x, p.y, threshold, join, live)
        selectionChanged(fill = !live)
    }

    // ColorDrop: the colour dragged from the colour button (or a swatch) and dropped on the canvas
    // fills the area around where it lands

    /** What the ColorDrop band under the top bar shows. */
    enum class DropBand { NONE, DRAGGING, DROPPED, FILLING }

    var dropBand by mutableStateOf(DropBand.NONE)
        private set

    /** The dragged colour's dot (window pixels) and its colour (0xRRGGBB), while it is dragged. */
    var dropDot by mutableStateOf<Pair<Offset, Int>?>(null)
        private set

    /** The threshold the colour fills with (0..1), kept from one drop to the next. */
    var dropThreshold by mutableFloatStateOf(transformPrefs.getFloat(DROP_THRESHOLD, 0.5f))
        private set

    /** Whether the threshold is being slid: its row shows under the band. */
    var dropSliding by mutableStateOf(false)
        private set

    /** Where a dragged colour let go fills nothing (the colour panel, while open), window pixels. */
    var dropAvoid: androidx.compose.ui.geometry.Rect? = null

    /** The swatch's colour when a swatch was dropped (filling goes on with it); else the current colour. */
    private var dropSwatch: Int? = null
    private var dropBandTimer: kotlinx.coroutines.Job? = null

    /** The colour being dragged: where it is and whether it has come to rest (the core's rules). */
    private var colourDrag: io.github.artcraftandroid.procraft.core.ColourDrag? = null

    /** The dragged colour came to rest and its slid threshold has begun filling (or tried to, and couldn't). */
    private var dropPoured = false
    private var dropPourRefused = false

    /**
     * A colour starts being dragged from the colour button, or [swatch] from the palette, at
     * uptime [ms], by a pen or not ([pen]).
     */
    fun dropDragStart(at: Offset, swatch: Int? = null, pen: Boolean = false, ms: Long = android.os.SystemClock.uptimeMillis()) {
        if (swatch == null) closePanels()
        dropBandTimer?.cancel()
        dropSwatch = swatch
        dropDot = at to (swatch ?: colorRgb())
        dropBand = DropBand.DRAGGING
        dropPoured = false
        dropPourRefused = false
        colourDrag = io.github.artcraftandroid.procraft.core.ColourDrag(at.x / density, at.y / density, ms.toDouble(), pen, dropThreshold)
    }

    /** The dragged colour's pointer moved to [at] at uptime [ms]: the dot follows, or, resting, the threshold slides. */
    fun dropDragMove(at: Offset, ms: Long = android.os.SystemClock.uptimeMillis()) {
        val drag = colourDrag ?: return
        val step = drag.moved(at.x / density, at.y / density, ms.toDouble())
        val dot = Offset(step.dotX * density, step.dotY * density)
        dropDot = dropDot?.let { dot to it.second }
        if (!step.slid) return
        if (dropPoured) {
            pourThreshold(step.threshold)
        } else if (!dropPourRefused) {
            dropThreshold = step.threshold
            dropSliding = true
            dropPoured = pourBegin(dot)
            dropPourRefused = !dropPoured
            if (dropPoured) dropDot = null
        }
    }

    /**
     * When (uptime ms) the dragged colour comes to rest if its pointer stays; null once it has, or
     * while it waits for a move after a rest off the artwork.
     */
    fun dropRestDue(): Long? = colourDrag?.restDue()?.toLong()

    /**
     * The time is uptime [ms]: the dragged colour comes to rest if it is due, on the artwork off
     * the bars, and fills there at once, the fill showing instead of the dot (a slide then sets
     * the threshold).
     */
    fun dropDragRest(ms: Long) {
        val drag = colourDrag ?: return
        val step = drag.now()
        val dot = Offset(step.dotX * density, step.dotY * density)
        val rests = drag.rest(
            ms.toDouble(),
            dot.y > io.github.artcraftandroid.procraft.ui.Pc.BarHeight.value * density &&
                dropAvoid?.contains(dot) != true &&
                session?.pourLands(dot.x, dot.y) == true,
        )
        if (!rests || dropPoured || dropPourRefused) return
        dropPoured = pourBegin(dot)
        dropPourRefused = !dropPoured
        if (dropPoured) dropDot = null
    }

    /** The dragged colour let go at [at] (null: somewhere it doesn't fill, like the top bar). */
    fun dropDragEnd(at: Offset?) {
        val step = colourDrag?.now()
        colourDrag = null
        // come to rest, it fills where it rests (already, if its threshold was slid)
        val rested = step?.takeIf { it.resting }?.let { Offset(it.dotX * density, it.dotY * density) }
        if (dropPoured) {
            dropPoured = false
            dropDot = null
            pourEnd(keep = true)
            dropBand = DropBand.DROPPED
            dropBandTimer = viewModelScope.launch {
                kotlinx.coroutines.delay(DROP_BAND_MS)
                if (dropBand == DropBand.DROPPED) dropBand = DropBand.NONE
            }
            return
        }
        val at = if (at != null) rested ?: at else null
        dropDot = null
        if (at != null && dropAvoid?.contains(at) != true && pourBegin(at)) {
            pourEnd(keep = true)
            // the band offers to go on filling for a moment
            dropBand = DropBand.DROPPED
            dropBandTimer = viewModelScope.launch {
                kotlinx.coroutines.delay(DROP_BAND_MS)
                if (dropBand == DropBand.DROPPED) dropBand = DropBand.NONE
            }
        } else {
            dropBand = DropBand.NONE
        }
    }

    /** Continue filling: each touch on the canvas fills, until [endFilling]. */
    fun continueFilling() {
        dropBandTimer?.cancel()
        dropBand = DropBand.FILLING
    }

    /** ColorDrop's filling ends, and Recolor with it. */
    fun endFilling() {
        endDropBand()
        endRecolor()
    }

    /** ColorDrop's band goes, and Continue filling with it. */
    private fun endDropBand() {
        dropBandTimer?.cancel()
        if (dropBand != DropBand.NONE) dropBand = DropBand.NONE
        dropSliding = false
    }

    /** Fills from window point [p] with the dropped colour; false if nothing was filled. */
    fun pourBegin(p: Offset): Boolean {
        val s = session ?: return false
        if (!currentTakesChanges()) return false
        // text takes no colour until it is rasterized
        if (currentIsText()) {
            askRasterize = true
            return false
        }
        val c = dropSwatch ?: colorRgb()
        val t = dropThreshold
        return pourNow { s.pourBegin(p.x, p.y, ((c shr 16) and 0xFF) / 255f, ((c shr 8) and 0xFF) / 255f, (c and 0xFF) / 255f, t) }
    }

    /** Continue filling: the colour and threshold the canvas's (shared) touches fill with; none when not filling. */
    fun touchFill(): io.github.artcraftandroid.procraft.core.Fill? {
        if (dropBand != DropBand.FILLING) return null
        val c = dropSwatch ?: colorRgb()
        return io.github.artcraftandroid.procraft.core.Fill(((c shr 16) and 0xFF) / 255f, ((c shr 8) and 0xFF) / 255f, (c and 0xFF) / 255f, dropThreshold)
    }

    /** The touches' fill was let go of (kept, or taken back by a second finger): a slid threshold is kept for the next. */
    fun filled() {
        if (dropSliding) transformPrefs.edit().putFloat(DROP_THRESHOLD, dropThreshold).apply()
        dropSliding = false
        refresh()
    }

    /** The fill under way redone with [threshold], which is kept for the next. */
    fun pourThreshold(threshold: Float) {
        val s = session ?: return
        val t = threshold.coerceIn(0f, 1f)
        dropThreshold = t
        dropSliding = true
        pourLater("threshold") { s.pourThreshold(t) }
    }

    /** The fill under way ends: [keep] makes it an undo step, else it is taken back. */
    fun pourEnd(keep: Boolean) {
        val s = session ?: return
        pourNow { s.pourEnd(keep, null) }
        if (dropSliding) transformPrefs.edit().putFloat(DROP_THRESHOLD, dropThreshold).apply()
        dropSliding = false
        refresh()
    }

    /** A fill's work (and feathering's), in order, off the main thread (a slid threshold redoes the whole fill). */
    private val pourThread = java.util.concurrent.Executors.newSingleThreadExecutor()

    /** The latest of each kind of change waiting for the fill's thread (a threshold, a point, a colour). */
    private val pourLatest = java.util.concurrent.ConcurrentHashMap<String, () -> Unit>()

    /** Runs [block] on the fill's thread, unless another of its [kind] is given before it gets there. */
    private fun pourLater(kind: String, block: () -> Unit) {
        if (pourLatest.put(kind, block) == null) pourThread.execute { pourLatest.remove(kind)?.invoke() }
    }

    /** Runs [block] on the fill's thread once what is queued there is done, and waits for it. */
    private fun <T> pourNow(block: () -> T): T = pourThread.submit(java.util.concurrent.Callable(block)).get()

    // Recolor: the area under a reticle takes the current colour. Dragged about, the fill follows it;
    // a tap keeps the fill and starts again where it lands

    /** Recolor is on. */
    var recoloring by mutableStateOf(false)
        private set

    /** Where the reticle is, canvas pixels. */
    var recolorPoint by mutableStateOf<Offset?>(null)
        private set

    /** Recolor's threshold (its Flood slider, 0..1), kept from one time to the next. */
    var recolorThreshold by mutableFloatStateOf(transformPrefs.getFloat(RECOLOR_THRESHOLD, 0f))
        private set

    /** The reticle where the core has it (canvas pixels). */
    private fun readRecolorPoint() {
        recolorPoint = session?.recolorPoint()?.let { windowToCanvas(Offset(it[0], it[1])) }
    }

    /** Recolor on, the reticle in the middle of the screen, or off (keeping what it filled). */
    fun toggleRecolor() {
        if (recoloring) {
            switchModes(ModeButton.Recolor)
            return
        }
        val s = session ?: return
        if (!currentTakesChanges()) return
        // text takes no colour until it is rasterized
        if (currentIsText()) {
            askRasterize = true
            return
        }
        switchModes(ModeButton.Recolor)
        closePanels()
        recoloring = true
        val c = colorRgb()
        val (r, g, b) = Triple(((c shr 16) and 0xFF) / 255f, ((c shr 8) and 0xFF) / 255f, (c and 0xFF) / 255f)
        val t = recolorThreshold
        // the shared core keeps it (crates/core recolor.rs); its fills run on the fill thread
        pourNow { s.recolorStart(screenCentre.x, screenCentre.y, r, g, b, t) }
        readRecolorPoint()
        refresh()
    }

    /** The touches dragged the reticle to window point [at]: shown there at once, the fill following on the fill thread. */
    fun recolorMoveTo(at: Offset) {
        val s = session ?: return
        recolorPoint = windowToCanvas(at) ?: return
        pourLater("point") { s.recolorMoveTo(at.x, at.y) }
    }

    /** A tap while recoloring: the fill so far is kept, and the reticle starts again at [p]. */
    fun recolorTap(p: Offset) {
        val s = session ?: return
        pourNow { s.recolorTap(p.x, p.y) }
        readRecolorPoint()
        refresh()
    }

    /** The Flood slider moved to [t]: the fill redone with it. */
    fun recolorThresholdTo(t: Float) {
        val s = session ?: return
        val v = t.coerceIn(0f, 1f)
        recolorThreshold = v
        if (recoloring) pourLater("threshold") { s.recolorThreshold(v) }
    }

    /** Recolor ends, keeping what it filled. */
    fun endRecolor() {
        if (!recoloring) return
        val s = session
        if (s != null) pourNow { s.recolorEnd() }
        recoloring = false
        recolorPoint = null
        transformPrefs.edit().putFloat(RECOLOR_THRESHOLD, recolorThreshold).apply()
        refresh()
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
        // a big selection takes a while: off the main thread, the newest amount when it gets there
        val s = session ?: return
        pourLater("feather") { s.selectFeather(amount) }
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
        if (transforming) {
            switchModes(ModeButton.Transform)
            return
        }
        val s = session ?: return
        if (!currentTakesChanges()) return
        switchModes(ModeButton.Transform)
        panel = Panel.NONE
        pickNext = false
        s.transformSetInterpolation(interpolation)
        s.transformSetGuides(guides)
        transformBox = s.transformBegin()
        // nothing there to move: it says so and stays out of transform
        if (transformBox == null) {
            showNotice(s.transformRefusal())
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

    /**
     * Numeric entry open beside the node at window point [at]: for the box's angle (the rotation
     * handle) or its size; to the node's right when it is on the box's right side ([right]), else to its left.
     */
    data class TransformNumeric(val angle: Boolean, val at: Offset, val right: Boolean = false)

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

    /** The interface hidden or shown again (not while transforming). */
    fun toggleBars() {
        if (!switchModes(ModeButton.Bars)) return
        barsHidden = !barsHidden
        if (barsHidden) panel = Panel.NONE
    }

    /** A stroke finished: its layer's thumbnail and the undo state changed. */
    fun strokeEnded() {
        // a snapped shape the pen moved: its handles where it now is
        if (quickShape != null) quickShape = session?.quickshapeInfo() ?: quickShape
        // a stroke on a locked or hidden layer drew nothing: say why (text layers ask to rasterize instead)
        if (adjustment == null && !currentIsText()) currentTakesChanges()
        refreshFlags()
        if (animation != null) refreshFrameThumbnails()
        refreshReference()
        recordFrame()
        // the stroke's colour went into the history (painting with the brush, not an adjustment on)
        history = colourHistory.colours().map { it.toInt() }
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

    /** The timeline's places: each frame's index, again for each frame it is held. */
    var frameSlots by mutableStateOf<List<Int>>(emptyList())
        private set

    /** The frame and how far into its hold the timeline was last left. */
    private var heldAt by mutableStateOf(0 to 0)

    /** How far into its hold the current frame is shown (0: the frame itself). */
    val heldPlace: Int
        get() {
            val f = currentFrame ?: return 0
            return if (heldAt.first == f) heldAt.second.coerceAtMost(frameSlots.count { it == f } - 1).coerceAtLeast(0) else 0
        }

    /** The timeline's place in the middle. */
    val currentSlot: Int?
        get() = currentFrame?.let { f -> frameSlots.indexOf(f).takeIf { it >= 0 }?.plus(heldPlace) }

    /** Playing: the timeline's place shown. */
    var playingSlot by mutableStateOf<Int?>(null)
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
            frameSlots = emptyList()
            currentFrame = null
            animationSettingsOpen = false
            frameOptionsOpen = false
            return
        }
        animationFrames = s.animationFrames()
        frameSlots = s.animationSequence().map { it.toInt() }
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

    /** Place [slot] comes to the middle, its frame the one drawn on (a tap on the current one opens its options). */
    fun tapFrame(slot: Int) {
        if (playingFrame != null) stopPlaying()
        if (slot == currentSlot) {
            frameOptionsOpen = !frameOptionsOpen
            animationSettingsOpen = false
            return
        }
        frameOptionsOpen = false
        goToSlot(slot)
    }

    private fun goToSlot(slot: Int) {
        val frame = frameSlots.getOrNull(slot) ?: return
        heldAt = frame to slot - frameSlots.indexOf(frame)
        if (frame != currentFrame) animationFrames.getOrNull(frame)?.let { selectLayer(it) }
    }

    /** The timeline dragged: playing stops and the frame's options close. */
    fun beginScrub() {
        stopPlaying()
        frameOptionsOpen = false
    }

    /** The timeline dragged past frame [i]: it shows alone, as playing shows frames. */
    fun scrubFrame(i: Int) {
        if (i in animationFrames.indices) session?.setPlayFrame(i.toUInt())
    }

    /**
     * The timeline let go with place [slot] in its middle: its frame becomes the current one, as
     * a tap makes it (null: nothing chosen, the current frame shows again).
     */
    fun endScrub(slot: Int?) {
        if (slot != null) goToSlot(slot)
        session?.setPlayFrame(null)
    }

    fun addFrame() {
        stopPlaying()
        frameOptionsOpen = false
        addLayer()
    }

    private fun currentFrameLayer(): LayerInfo? = currentFrame?.let { animationFrames.getOrNull(it) }?.let { id -> layers.firstOrNull { it.id == id } }

    /** The frame copied; on a held place it splits there, the copy taking over. */
    fun duplicateFrame() {
        frameOptionsOpen = false
        val held = heldPlace
        val layer = currentFrameLayer() ?: return
        if (held > 0) layerChange { if (it.splitHeldFrame(layer.id, held.toUInt()) == null) message = noRoom() } else duplicate(layer)
    }

    /** The frame deleted; on a held place it is held one frame less. */
    fun deleteFrame() {
        frameOptionsOpen = false
        val layer = currentFrameLayer() ?: return
        if (heldPlace > 0) layerChange { it.dropHeldFrame(layer.id) } else delete(layer)
    }

    fun frameHold(): Int = currentFrame?.let { animationFrames.getOrNull(it) }?.let { session?.frameHold(it)?.toInt() } ?: 0

    var holdVersion by mutableStateOf(0)
        private set

    fun changeFrameHold(frames: Int) {
        val id = currentFrame?.let { animationFrames.getOrNull(it) } ?: return
        session?.changeFrameHold(id, frames.coerceIn(0, MAX_HOLD).toUInt(), false)
        frameSlots = session?.animationSequence()?.map { it.toInt() }.orEmpty()
        holdVersion++
    }

    /** The hold's slider let go: the whole drag is one undo step. */
    fun endFrameHold() {
        val id = currentFrame?.let { animationFrames.getOrNull(it) } ?: return
        session?.changeFrameHold(id, frameHold().toUInt(), true)
        refresh()
    }

    fun togglePlaying() = if (playingFrame != null) stopPlaying() else play()

    private fun play() {
        val s = session ?: return
        val a = animation ?: return
        // the frames in playing order, each as often as it is held (the shared core's)
        val order = s.animationSequence().map { it.toInt() }
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
                playingSlot = at
                s.setPlayFrame(frame.toUInt())
                kotlinx.coroutines.delay(1000L / (animation?.fps ?: a.fps).toInt().coerceAtLeast(1))
                // round again, turned back, or (a single pass) done
                val next = io.github.artcraftandroid.procraft.core.animationNextStep(a.playback, order.size.toUInt(), at.toUInt(), step) ?: break
                at = next.at.toInt()
                step = next.step
            }
            // a single pass ends on its last frame, still shown
        }
    }

    fun stopPlaying() {
        playJob?.cancel()
        playJob = null
        if (playingFrame != null) {
            playingFrame = null
            playingSlot = null
            session?.setPlayFrame(null)
        }
    }

    // ---- layers ----

    fun addLayer() = layerChange { if (it.addLayer() == 0uL) message = noRoom() }

    // no layer's pixels change, so the thumbnails stay (an artwork's hundreds would load again)
    fun selectLayer(id: ULong) = layerChange(thumbs = false) { it.selectLayer(id) }

    /** Shows or hides [l], saying so in the notice band as measured ("Hide layer"). */
    fun toggleVisible(l: LayerInfo) {
        var done = false
        layerChange { done = it.setLayerVisible(l.id, !l.visible) }
        if (done) showNotice(if (l.visible) "Hide layer" else "Show layer")
    }

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

    /** Layer [l]'s options menu, as the core lists it. */
    fun layerOptions(l: LayerInfo): List<io.github.artcraftandroid.procraft.core.LayerOptionRow> = session?.layerOptions(l.id).orEmpty()

    fun hasMask(l: LayerInfo) = layers.any { it.kind == LayerType.MASK && it.parent == l.id }

    /** The rows a drag of [l] takes along, it among them (the shared core decides). */
    fun movingWith(l: LayerInfo): Set<ULong> = session?.movingWith(l.id)?.toSet() ?: setOf(l.id)

    /**
     * Moves [l] (with what goes with it) just above or below [target]'s row; a chosen layer with
     * others picked takes them along, in their order (the shared core decides).
     */
    fun moveNextTo(l: LayerInfo, target: LayerInfo, above: Boolean) = layerChange(thumbs = false) { it.moveNextTo(l.id, target.id, above) }

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

    /** Locks or unlocks [l], saying so in the notice band as the core words it ("Lock layer", "Unlock Layer Group"). */
    fun toggleLocked(l: LayerInfo) {
        val on = !l.locked
        val said = session?.lockSaid(l.id, on) ?: return
        var done = false
        layerChange(thumbs = false) { done = it.setLayerLocked(l.id, on) }
        if (done) showNotice(said)
    }

    fun duplicate(l: LayerInfo) = layerChange { if (it.duplicateLayer(l.id) == null) message = noRoom() }

    /** Delete on a locked layer's row, told about rather than done (as measured). */
    var lockedDeleteTold by mutableStateOf(false)

    fun delete(l: LayerInfo) {
        if (l.locked) {
            lockedDeleteTold = true
            return
        }
        layerChange { it.deleteLayer(l.id) }
    }

    fun mergeDown(l: LayerInfo) = layerChange { it.mergeDown(l.id) }

    fun mergeMask(l: LayerInfo) = layerChange { it.mergeMask(l.id) }

    /** Whether layers are picked to act on with the current one. */
    val anyPicked: Boolean get() = layers.any { it.picked }

    /** Picks layer [l] to act on with the current one, or unpicks it. */
    fun pickLayer(l: LayerInfo) = layerChange(thumbs = false) { it.pickLayer(l.id) }

    fun pickAllLayers() = layerChange(thumbs = false) { it.pickAllLayers() }

    fun groupPicked() = layerChange { it.groupPicked() }

    fun mergePicked() = layerChange { it.mergePicked() }

    /** Two rows pinched together: layers [a] and [b] and every one between merge into the lowest. */
    fun mergeBetween(a: ULong, b: ULong) = layerChange { it.mergeBetween(a, b) }

    fun duplicatePicked() = layerChange { if (!it.duplicatePicked()) message = noRoom() }

    fun deletePicked() = layerChange { it.deletePicked() }

    /** Whether the chosen layers include a locked one (Delete then removes nothing). */
    fun chosenLocked(): Boolean = session?.chosenLocked() == true

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
        followMaskColour()
        // drawing assist is the current layer's
        if (guide != null) guide = s.guide()
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

    /** Reads again the thumbnails of the layers whose pixels changed since theirs was read (or have none). */
    private fun refreshThumbnails() {
        val s = session ?: return
        viewModelScope.launch {
            val versions = withContext(Dispatchers.Default) { runCatching { s.layerVersions() }.getOrNull() } ?: return@launch
            if (session !== s) return@launch
            for (v in versions) {
                if (v.id !in thumbnails || thumbnailVersions[v.id] != v.version) loadThumbnail(v.id, v.version)
            }
        }
    }

    /** Reads layer [id]'s thumbnail; [version], its pixels' version when asked (none: not known). */
    private fun loadThumbnail(id: ULong, version: ULong? = null) {
        val s = session ?: return
        viewModelScope.launch {
            val image = withContext(Dispatchers.Default) {
                val t = s.layerThumbnail(id, THUMB_SIDE.toUInt()) ?: return@withContext null
                val bitmap = io.github.artcraftandroid.procraft.ui.CanvasSpace.tag(Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), Bitmap.Config.ARGB_8888))
                bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(t.rgba))
                bitmap.asImageBitmap()
            }
            if (image != null && session === s) {
                thumbnails[id] = image
                if (version != null) thumbnailVersions[id] = version else thumbnailVersions.remove(id)
            }
        }
    }

    /** The canvas's (shared) touches changed the brush's size or opacity (Hover's): the sliders show it. */
    fun brushTouched() = readSliders()

    private fun readSliders() {
        val info = session?.info() ?: return
        size = info.size
        opacity = info.opacity
        readNotches()
    }

    override fun onCleared() {
        saveNow()
        session?.stopAutosave()
        session = null
        pourThread.shutdown()
    }

    companion object {
        /** A finished export's check is drawn and shown this long before its card goes (ms, as measured). */
        const val EXPORT_DONE_MS = 450L

        /** How often the export card asks the core how far an export has come (ms). */
        private const val EXPORT_POLL_MS = 50L

        /**
         * The import card's pie: the copy in fills this share of it; while the file is read it
         * creeps on towards [IMPORT_CREEP_TO], this share of the way left each poll.
         */
        private const val IMPORT_COPY_SHARE = 0.6f
        private const val IMPORT_CREEP_TO = 0.95f
        private const val IMPORT_CREEP = 0.03f

        /** A model's turntable: its frame rate, and its sizes (720p, 1080p, 4K, Square). */
        const val TURNTABLE_FPS = 30
        val TURNTABLE_SIZES = listOf(1280 to 720, 1920 to 1080, 3840 to 2160, 1080 to 1080)

        /** Model files the gallery's Import opens to paint on. */
        val MODEL_EXTENSIONS = listOf("obj", "usdz", "usdc", "usda", "usd")

        /** The environment a model is lit by unless another is chosen (an asset in environments/). */
        const val DEFAULT_ENVIRONMENT = "mountain"

        /** The environments to light a model by, as the Lighting Studio lists them (assets in environments/). */
        val ENVIRONMENTS = listOf("studio", "coast", "auditorium", "city", "nightlife", "portside", "industrial", "sunrise", "beach", "mountain", "daytime")

        /** While the view settles, what is drawn over the canvas is moved this often (ms), and a little past its end (ns). */
        const val FRAME_MS = 16L
        const val SETTLE_MARGIN_NS = 50_000_000L

        /** The clone source's ring (points from its centre). */
        const val CLONE_PUCK_RADIUS_DP = 30f
        /** Each glitch kind's sliders as they start: its own two (diverge three), then the zoom. */
        val GLITCH_DEFAULTS = mapOf(
            GlitchKind.ARTIFACT to listOf(1f, 0.09f, 1f),
            GlitchKind.WAVE to listOf(0.2f, 0.3f, 1f),
            GlitchKind.SIGNAL to listOf(0.8f, 0.19f, 1f),
            GlitchKind.DIVERGE to listOf(0.98f, -0.5f, 0.85f, 0.5f),
        )
        const val BRUSH_CHANGES = "changes"
        /** Where earlier versions kept the library (now in a file). */
        const val BRUSH_LIBRARY = "library"
        /** How big the studio's Shape and Grain Source rows show their image (pixels across). */
        const val SOURCE_ROW_PX = 88
        /** And the landscape studio's square of it. */
        const val SOURCE_BOX_PX = 566
        /** Where earlier versions kept the brushes in use, each set's last choice and each brush's sliders. */
        const val BRUSH_CHOICES = "choices"

        /** And how many brushes of each shipped set the kept library had had the chance to hold. */
        const val BRUSHES_SEEN = "brushes_seen"
        const val MAX_CANVAS_EDGE = 16384
        /** How much smaller than fitted to the screen the canvas is while cropping. */
        /** The reference window: its first place and picture size, its limits, and how sharp and how often its canvas is read. */
        /** The reference window's picture at first, and its gap to the screen's bottom. */
        const val REFERENCE_WIDTH_DP = 350f
        const val REFERENCE_HEIGHT_DP = 423f
        const val REFERENCE_BOTTOM_DP = 50f
        const val REFERENCE_MIN_DP = 150f
        const val REFERENCE_MAX_DP = 700f
        const val REFERENCE_PIXELS = 700
        const val REFERENCE_INTERVAL_MS = 300L

        /** The window the eyedropper shows (magnified twice), dp across. */
        const val PICK_WINDOW_DP = 54f
        const val RIGHT_HANDED = "right_handed"
        const val COLOR_MODE = "color_mode"

        /** How often a hovering brush cursor reads its size again, ms. */
        const val CURSOR_REREAD_MS = 100L
        const val ACTIONS_TAB = "actions_tab"
        const val STUDIO_TAB = "studio_tab"
        const val PAD_3D = "pad_3d_preview"
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

        /**
         * The layered package's file extensions, and their names in the Share lists: ours, and the
         * other app's (the same package, so either opens in both). Files are told apart by what is
         * in them, so `.pcraft` ones from before and other apps' `.procraft` zips still open.
         */
        const val OWN_EXTENSION = "procraft"
        const val OWN_LABEL = "ProCraft"
        const val PACKAGE_EXTENSION = "procreate"

        /** Brush and brush set files, as other painting apps share them. */
        const val BRUSH_EXTENSION = "brush"
        const val BRUSH_SET_EXTENSION = "brushset"
        /** Other apps' brush files: Photoshop's, GIMP's brushes, pipes and generated brushes, Krita's presets and bundles, Clip Studio's sub tools. */
        val OTHER_BRUSH_EXTENSIONS = listOf("abr", "gbr", "gih", "vbr", "kpp", "bundle", "sut")
        val PACKAGE_LABEL = PACKAGE_EXTENSION.replaceFirstChar(Char::titlecase)

        /** What the gallery's Share offers, in order: still pictures, then animations. */
        val SHARE_FORMATS = listOf(OWN_LABEL, PACKAGE_LABEL, "PSD", "PDF", "JPEG", "PNG", "TIFF") + Animated.entries.map { it.title }

        /** How often, and for how long at most, opening waits for the canvas's first frame, milliseconds. */
        const val CANVAS_POLL_MS = 32L
        const val CANVAS_DRAWN_LIMIT_MS = 15_000L

        /** How often the open artwork's autosave is asked whether it failed. */
        const val SAVE_FAILURE_POLL_MS = 3_000L

        /** How near a slider value is on a notch (and is pulled onto it), and the most a slider takes. */
        const val NOTCH_NEAR = 0.04f

        /** The least time between two colour names while the colour keeps changing, milliseconds. */
        const val COLOR_NAME_GAP_MS = 150L

        /** How often an open artwork is saved when it has changed, milliseconds. */
        const val SPEED_DISTANCE = "speed_distance"

        /** What an artwork dragged out of the gallery becomes (an index into [DRAG_FORMATS]), and a picture's format when one is wanted. */
        const val DRAG_FORMAT = "drag_format"
        const val DRAG_IMAGE_FORMAT = "drag_image_format"
        val DRAG_FORMATS = listOf(OWN_LABEL, "PSD", "JPEG", "PNG", "TIFF")
        val DRAG_IMAGE_FORMATS = listOf("JPEG", "PNG", "TIFF")
        /** The QuickMenu's buttons as kept before there were several menus (read once, into the first). */
        const val QUICK_MENU = "quick_menu"
        const val QUICK_MENUS = "quick_menus"
        const val QUICK_MENU_ACTIVE = "quick_menu_active"
        /** A press of □ this short, used by nothing, is a tap. */
        const val MODIFY_TAP_MS = 500L
        const val PRESETS = "presets"
        const val INPUT_STABILIZATION = "input_stabilization"
        const val INPUT_MOTION = "input_motion_filtering"
        const val INPUT_EXPRESSION = "input_motion_expression"
        const val INPUT_PRESSURE_SMOOTHING = "input_pressure_smoothing"
        const val INPUT_CURVE = "input_pressure_curve"
        const val INPUT_TIP_ATTACHED = "input_tip_attached"
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
        const val GUIDE_FIT_BAND = 0.2f
        /** The largest side a picture brought in is decoded at (pixels). */
        const val MAX_PICTURE = 4096
        /** How long a stroke is held still to snap (milliseconds), how still (points), and how near a handle a touch takes it. */
        const val QUICKSHAPE_MS = 650L
        const val QUICKSHAPE_STILL_DP = 6f
        const val QUICKSHAPE_REACH_DP = 30f

        /** The drawing pad's colours (0xRRGGBB), the white one, and its background's grey. */
        val PAD_COLORS = listOf(0xF83045, 0xFCBD03, 0x85D921, 0x2BB5E6, 0x4956A5, 0xDA3D92, 0xFFFFFF, 0x000000)
        const val PAD_WHITE = 6
        const val PAD_BLACK = 7
        const val PAD_BACK = 31f / 255f
        /** With the light interface the pad is light and paints in black. */
        const val PAD_BACK_LIGHT = 243f / 255f
        /** How long after a studio control stops moving its sample stroke is drawn again (milliseconds). */
        const val STUDIO_PREVIEW_DELAY_MS = 60L
        /** The first colour: a saturated blue. */
        const val DEFAULT_HUE = 0.59f
        const val KEPT_HUE = "colour.hue"
        const val KEPT_SATURATION = "colour.saturation"
        const val KEPT_BRIGHTNESS = "colour.brightness"
        const val KEPT_MASK_HUE = "colour.mask.hue"
        const val KEPT_MASK_SATURATION = "colour.mask.saturation"
        const val KEPT_MASK_BRIGHTNESS = "colour.mask.brightness"
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
        private const val DROP_THRESHOLD = "dropThreshold"

        /** How long a widget's tap waits for the window to be measured before opening anyway. */
        private const val WINDOW_WAIT_MS = 3000L
        private const val RECOLOR_THRESHOLD = "recolorThreshold"

        /** What undo and redo call a Recolor fill. */

        /** A video's frames come in at Animation Assist's starting frame rate unless it is on. */
        private const val DEFAULT_FPS = 15
        /** The most frames a video brings in. */
        private const val MAX_VIDEO_FRAMES = 999

        /** A PDF's pages come in at this resolution, their long side at most [PDF_MOST_SIDE] pixels. */
        private const val PDF_DPI = 300f
        private const val PDF_MOST_SIDE = 4096

        /** How long the band offers Continue filling after a drop. */
        private const val DROP_BAND_MS = 2_500L

        fun hsvToRgb(h: Float, s: Float, v: Float): Int {
            val hsv = floatArrayOf((h * 360f) % 360f, s, v)
            return android.graphics.Color.HSVToColor(hsv) and 0xFFFFFF
        }
    }
}

