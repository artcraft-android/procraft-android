package io.github.artcraftandroid.procraft.ui

import android.graphics.BlurMaskFilter
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Paint
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.PointerInputScope
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupProperties
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import kotlinx.coroutines.launch
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.ColorMode
import io.github.artcraftandroid.procraft.canvas.HarmonyKind
import io.github.artcraftandroid.procraft.gallery.Palette
import kotlin.math.PI
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.max
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlin.math.sin
import kotlin.math.sqrt

private val PanelWidth = 376.dp

/** Screens shorter than this (landscape) get the short colour panel: no history, the rest tighter. */
private val ShortScreen = 900.dp

/**
 * Short, the classic square takes the height left over: the screen less the bar and arrow (56),
 * the header (64), the sliders (108), the palette (142), the tabs (70) and a 25 margin.
 */
private val ShortClassicRest = 465.dp
private val RingOuter = 182.dp
private val RingInner = 124.dp
private val DiscRadius = 105.dp
private val HarmonyRadius = 149.dp

/**
 * The hue controls are not linear in hue. These are the hues (degrees) at evenly spaced points
 * along each, measured; positions between them interpolate. The classic slider has 25, the
 * disc's ring 49 (every 7.5 degrees, anticlockwise from the right).
 */
private val SliderHues = floatArrayOf(
    0f, 0.2f, 8f, 17.7f, 27.3f, 36.4f, 44.2f, 52.7f, 80.4f, 112.8f, 120.5f, 120.5f, 133.3f,
    154.3f, 178.2f, 196.1f, 211.1f, 226.1f, 241.1f, 259.7f, 275.1f, 291.7f, 310f, 340.2f, 360f,
)
private val RingHues = floatArrayOf(
    0f, 0.2f, 0.2f, 0.2f, 4.5f, 13.2f, 18.9f, 23.7f, 27.9f, 32.4f, 36.4f, 40.9f, 44.6f, 49.1f,
    53.1f, 57.2f, 61.4f, 74.2f, 88.6f, 114.8f, 120.5f, 120.5f, 120.5f, 120.5f, 134.8f, 149.9f,
    162.2f, 173.4f, 180f, 186.9f, 195.4f, 203.4f, 211.4f, 219.4f, 227.5f, 235.3f, 242.1f, 251.1f,
    258.5f, 268f, 276.5f, 285.3f, 293.6f, 300.2f, 303.3f, 318.2f, 333.7f, 351.2f, 360f,
)

/** The hue (0..1) at [position] (0..1) along a control whose hues are [stops]. */
private fun hueAt(stops: FloatArray, position: Float): Float {
    val p = position.coerceIn(0f, 1f) * (stops.size - 1)
    val i = p.toInt().coerceAtMost(stops.size - 2)
    return (stops[i] + (stops[i + 1] - stops[i]) * (p - i)) / 360f
}

/** Where [hue] (0..1) sits (0..1) along a control whose hues are [stops]. */
private fun huePosition(stops: FloatArray, hue: Float): Float {
    val h = hue.coerceIn(0f, 1f) * 360f
    for (i in 0 until stops.size - 1) {
        val a = stops[i]
        val b = stops[i + 1]
        if (h <= b && b > a) return (i + ((h - a) / (b - a)).coerceIn(0f, 1f)) / (stops.size - 1)
    }
    return 1f
}

private fun rgbColor(rgb: Int) = CanvasSpace.color(rgb)

/**
 * [colours] all in one colour space (the first that isn't sRGB, if any): a gradient's colours
 * must share one, and canvas colours may be Display P3 beside the panel's own sRGB greys.
 */
private fun oneSpace(colours: List<Color>): List<Color> {
    val space = colours.firstOrNull { it.colorSpace != androidx.compose.ui.graphics.colorspace.ColorSpaces.Srgb }?.colorSpace ?: return colours
    return colours.map { it.convert(space) }
}

private fun hsvColor(h: Float, s: Float, v: Float) = rgbColor(CanvasViewModel.hsvToRgb(h, s, v))

/** A colour the pickers show and change: the one painted with, or one of its own (a background). */
interface ColorPick {
    val hue: Float
    val saturation: Float
    val brightness: Float
    fun rgb(): Int
    fun setHsv(h: Float, s: Float, v: Float)
    fun setRgb(rgb: Int)
}

/** The colour painted with. */
private class BrushColor(private val vm: CanvasViewModel) : ColorPick {
    override val hue get() = vm.hue
    override val saturation get() = vm.saturation
    override val brightness get() = vm.brightness
    override fun rgb() = vm.colorRgb()
    override fun setHsv(h: Float, s: Float, v: Float) = vm.setColor(h, s, v)
    override fun setRgb(rgb: Int) = vm.setRgb(rgb)
}

/** A colour of its own, starting at [rgb]; [onChange] hears every change. Greys keep their hue. */
class HeldColor(rgb: Int, private val onChange: (Int) -> Unit) : ColorPick {
    override var hue by mutableFloatStateOf(0f)
        private set
    override var saturation by mutableFloatStateOf(0f)
        private set
    override var brightness by mutableFloatStateOf(0f)
        private set

    init {
        take(rgb)
    }

    private fun take(rgb: Int) {
        val hsv = FloatArray(3)
        android.graphics.Color.RGBToHSV((rgb shr 16) and 0xFF, (rgb shr 8) and 0xFF, rgb and 0xFF, hsv)
        if (hsv[1] > 0f) hue = hsv[0] / 360f
        saturation = hsv[1]
        brightness = hsv[2]
    }

    override fun rgb() = CanvasViewModel.hsvToRgb(hue, saturation, brightness)

    override fun setHsv(h: Float, s: Float, v: Float) {
        hue = h.coerceIn(0f, 1f)
        saturation = s.coerceIn(0f, 1f)
        brightness = v.coerceIn(0f, 1f)
        onChange(rgb())
    }

    override fun setRgb(rgb: Int) {
        take(rgb)
        onChange(rgb)
    }
}

/**
 * The colour panel: the current and second colours, a picker per mode, the history and the mode
 * tabs. With [background] it sets the canvas's background colour instead, titled so, with a tick
 * that closes it. On a screen less than [ShortScreen] tall it leaves the history out and tightens
 * the pickers to fit.
 */
@Composable
fun ColorPanel(vm: CanvasViewModel, arrowX: Dp, screenHeight: Dp, modifier: Modifier = Modifier, background: Boolean = false) {
    val short = screenHeight < ShortScreen
    // short, the disc reaches up into the header's last 7
    val headerHeight = if (short && vm.colorMode == ColorMode.DISC) 57.dp else 64.dp
    val c: ColorPick = if (background) {
        remember { HeldColor(vm.backgroundRgb()) { vm.setBackgroundRgb(it, done = false) } }
    } else {
        remember(vm) { BrushColor(vm) }
    }
    Popover(PanelWidth, arrowX, modifier) {
        Box {
            if (background) {
                PanelHeader("Background", start = 20.dp, end = 12.dp, height = headerHeight, chevron = false) {
                    Box(
                        Modifier.padding(top = 19.dp).size(44.dp).clip(CircleShape).background(Pc.Accent).clickable { vm.endBackgroundPick() },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(PcIcons.DoneTick, contentDescription = "Done", tint = Color.White, modifier = Modifier.requiredSize(40.dp))
                    }
                }
            } else if (vm.colorMode == ColorMode.PALETTES) {
                PanelHeader("Palettes", start = 20.dp, end = 17.dp, chevron = false) { NewPaletteButton(vm) }
            } else PanelHeader("Colors", start = 20.dp, end = 20.dp, height = headerHeight, chevron = false) {
                Row(Modifier.padding(top = 21.5.dp), horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                    val swatch = Modifier.size(43.dp, 25.dp).clip(RoundedCornerShape(6.dp))
                    Box(swatch.background(rgbColor(vm.colorRgb())).border(1.dp, Pc.SwatchEdge, RoundedCornerShape(6.dp)))
                    Box(swatch.background(rgbColor(vm.secondaryRgb)).clickable { vm.swapColors() })
                }
            }
            Box(Modifier.align(Alignment.TopCenter).padding(top = 4.dp).size(40.dp, 4.dp).clip(CircleShape).background(Pc.Grabber))
            if (vm.colorMode == ColorMode.HARMONY) HarmonyKindButton(vm)
        }
        // the other modes come one by one; until then they show the classic picker
        when (vm.colorMode) {
            ColorMode.DISC -> Disc(c, short = short)
            ColorMode.VALUE -> Value(c, short = short, inks = vm.inks)
            ColorMode.HARMONY -> Harmony(vm, c, short = short)
            ColorMode.PALETTES -> Palettes(vm, c, short)
            else -> Classic(c, square = if (short) (screenHeight - ShortClassicRest).coerceIn(150.dp, 344.dp) else 344.dp)
        }
        if (vm.colorMode != ColorMode.PALETTES && short) {
            ShortBottom(vm, c)
        } else if (vm.colorMode != ColorMode.PALETTES) {
            History(vm, c)
            vm.palettes.getOrNull(vm.defaultPalette)?.let { p ->
                Box(Modifier.fillMaxWidth().height(if (short) 34.dp else 44.dp)) {
                    Text(p.name, color = Pc.PanelTitle, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, modifier = Modifier.padding(start = 21.dp, top = if (short) 8.dp else 13.dp))
                }
                SwatchGrid(vm, c, vm.defaultPalette, p, Modifier.padding(horizontal = 8.dp))
            }
        }
        ModeTabs(vm)
    }
}

private val CompactWidth = 260.dp
/** How wide the compact colour picker is, for placing it. */
internal val CompactColorsWidth get() = CompactWidth
private val CompactFill = Color(0xFF1E1E1E)
private val CompactGroup = Color(0xFF292929)
private val CompactHexField = Color(0xFF343434)

/**
 * The compact colour picker other screens open (Custom Canvas's background): "Colors", a picker
 * per mode and the mode tabs as icons, 260 wide; no swatches, history or palette.
 */
@Composable
fun CompactColors(vm: CanvasViewModel, c: ColorPick, arrowX: Dp, modifier: Modifier = Modifier, side: ArrowSide = ArrowSide.TOP) {
    var mode by remember { mutableStateOf(ColorMode.DISC) }
    Popover(CompactWidth, arrowX, modifier, fill = CompactFill, side = side) {
        Text("Colors", color = Pc.PanelTitle, fontSize = 24.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 18.dp, top = 9.dp, bottom = 5.dp))
        when (mode) {
            ColorMode.DISC -> Disc(c, compact = true)
            ColorMode.VALUE -> Value(c, compact = true)
            ColorMode.HARMONY -> Harmony(vm, c, compact = true)
            ColorMode.PALETTES -> CompactPalettes(vm, c)
            else -> Classic(c, compact = true)
        }
        Row(Modifier.fillMaxWidth().height(52.dp)) {
            for ((m, icon, label) in Modes) {
                Box(Modifier.weight(1f).fillMaxSize().clickable { mode = m }, contentAlignment = Alignment.Center) {
                    // the tabs' 36-unit glyphs, a little smaller here
                    Icon(icon, contentDescription = label, tint = if (m == mode) Pc.Accent else Pc.PanelTitle, modifier = Modifier.requiredSize(33.dp))
                }
            }
        }
    }
}

/** The palettes' swatches, one grid after another, a tap taking a colour. */
@Composable
private fun CompactPalettes(vm: CanvasViewModel, c: ColorPick) {
    LazyColumn(
        Modifier.fillMaxWidth().height(240.dp),
        contentPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        itemsIndexed(vm.palettes) { _, p ->
            Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp))) {
                for (row in 0 until 3) {
                    Row {
                        for (col in 0 until 10) {
                            val rgb = p.colors[row * 10 + col]
                            Box(
                                Modifier.weight(1f).height(24.dp).background(if (rgb != null) rgbColor(rgb) else Pc.EmptySlot)
                                    .then(if (rgb != null) Modifier.clickable { c.setRgb(rgb) } else Modifier),
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Classic(c: ColorPick, compact: Boolean = false, square: Dp = 344.dp) {
    SaturationBrightness(c, compact, square)
    if (compact) {
        // the hue only, 240 wide under the 248 square
        Box(Modifier.padding(start = 10.dp, end = 10.dp).height(40.dp)) {
            SliderRow(huePosition(SliderHues, c.hue), SliderHues.map { hsvColor(it / 360f, 1f, 1f) }) { c.setHsv(hueAt(SliderHues, it), c.saturation, c.brightness) }
        }
        return
    }
    Column(Modifier.padding(horizontal = 20.dp)) {
        val pure = hsvColor(c.hue, 1f, 1f)
        SliderRow(huePosition(SliderHues, c.hue), SliderHues.map { hsvColor(it / 360f, 1f, 1f) }) { c.setHsv(hueAt(SliderHues, it), c.saturation, c.brightness) }
        SliderRow(c.saturation, listOf(Pc.SaturationGrey, pure)) { c.setHsv(c.hue, it, c.brightness) }
        SliderRow(c.brightness, listOf(Color.Black, hsvColor(c.hue, c.saturation, 1f))) { c.setHsv(c.hue, c.saturation, it) }
    }
}

/** Square (saturation across, brightness up, both -1..1 with y down) to disc, by the elliptical grid map. */
private fun squareToDisc(x: Float, y: Float): Offset = Offset(x * sqrt(1f - y * y / 2f), y * sqrt(1f - x * x / 2f))

/** The inverse of [squareToDisc]. */
private fun discToSquare(u: Float, v: Float): Offset {
    val r2 = 2f * sqrt(2f)
    fun half(a: Float, b: Float, s: Float) = 0.5f * sqrt(max(0f, 2f + a * a - b * b + r2 * s)) - 0.5f * sqrt(max(0f, 2f + a * a - b * b - r2 * s))
    return Offset(half(u, v, u).coerceIn(-1f, 1f), half(v, u, v).coerceIn(-1f, 1f))
}

/** The saturation/brightness disc for [hue], [side] pixels square, with a soft edge. */
private fun discImage(hue: Float, side: Int): ImageBitmap {
    val pixels = IntArray(side * side)
    val hsv = floatArrayOf(hue * 360f, 0f, 0f)
    val half = side / 2f
    for (j in 0 until side) {
        for (i in 0 until side) {
            val u = (i + 0.5f) / half - 1f
            val v = (j + 0.5f) / half - 1f
            val edge = ((1f - sqrt(u * u + v * v)) * half + 0.5f).coerceIn(0f, 1f)
            if (edge <= 0f) continue
            val sq = discToSquare(u, v)
            hsv[1] = (sq.x + 1f) / 2f
            hsv[2] = (1f - sq.y) / 2f
            val rgb = android.graphics.Color.HSVToColor(hsv) and 0xFFFFFF
            pixels[j * side + i] = ((edge * 255f).toInt() shl 24) or rgb
        }
    }
    return CanvasSpace.tag(android.graphics.Bitmap.createBitmap(pixels, side, side, android.graphics.Bitmap.Config.ARGB_8888)).asImageBitmap()
}

/**
 * The disc picker, 364 square: a hue ring (radii 124 to 182, the hue scale running anticlockwise
 * from the right) around the saturation/brightness disc (radius 105).
 */
@Composable
private fun Disc(c: ColorPick, compact: Boolean = false, short: Boolean = false) {
    val hue by rememberUpdatedState(c.hue)
    val sat by rememberUpdatedState(c.saturation)
    val bri by rememberUpdatedState(c.brightness)
    val density = LocalDensity.current
    // compact, the whole picker is 240 across instead of 364
    val k = if (compact) 240f / 364f else 1f
    val discRadius = DiscRadius * k
    val ringInner = RingInner * k
    val ringOuter = RingOuter * k
    val disc = remember(c.hue, k) { discImage(c.hue, with(density) { (discRadius * 2).roundToPx() }) }
    val ring = remember {
        val stops = Array(RingHues.size) { i -> (1f - i / (RingHues.size - 1f)) to hsvColor(RingHues[i] / 360f, 1f, 1f) }
        Brush.sweepGradient(*stops.reversedArray())
    }
    Box(
        Modifier.then(
            when {
                compact -> Modifier.padding(start = 10.dp, top = 4.dp, bottom = 6.dp)
                short -> Modifier.padding(start = 6.dp, bottom = 3.dp)
                else -> Modifier.padding(start = 6.dp, top = 8.dp, bottom = 17.dp)
            },
        ).size(364.dp * k)
            .pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    val centre = Offset(size.width / 2f, size.height / 2f)
                    val onRing = (down.position - centre).getDistance() > ((discRadius + ringInner) / 2).toPx()
                    fun send(p: Offset) {
                        val d = p - centre
                        if (onRing) {
                            val turn = (atan2(-d.y, d.x) / (2f * PI.toFloat()) + 1f) % 1f
                            c.setHsv(hueAt(RingHues, turn), sat, bri)
                        } else {
                            val r = discRadius.toPx()
                            var uv = Offset(d.x / r, d.y / r)
                            val len = uv.getDistance()
                            if (len > 1f) uv = Offset(uv.x / len, uv.y / len)
                            val sq = discToSquare(uv.x, uv.y)
                            c.setHsv(hue, (sq.x + 1f) / 2f, (1f - sq.y) / 2f)
                        }
                    }
                    send(down.position)
                    while (true) {
                        val event = awaitPointerEvent()
                        val ch = event.changes.firstOrNull { it.id == down.id } ?: break
                        if (!ch.pressed) break
                        send(ch.position)
                        ch.consume()
                    }
                }
            }
            .drawBehind {
                val o = center
                val inner = ringInner.toPx()
                val outer = ringOuter.toPx()
                drawCircle(ring, (inner + outer) / 2f, o, style = Stroke(outer - inner))
                val r = discRadius.toPx()
                drawImage(disc, topLeft = Offset(o.x - r, o.y - r))
                // the ring knob sits on the middle of the ring, where the hue is
                val turn = huePosition(RingHues, c.hue) * 2f * PI.toFloat()
                val mid = (inner + outer) / 2f
                knob(o + Offset(cos(turn) * mid, -sin(turn) * mid), 26.2.dp.toPx() * k, hsvColor(c.hue, 1f, 1f))
                val p = squareToDisc(c.saturation * 2f - 1f, 1f - c.brightness * 2f)
                knob(o + Offset(p.x * r, p.y * r), 10.75.dp.toPx(), rgbColor(c.rgb()))
            },
    )
}

/** A colour knob: [fill] inside a thin white ring, over a soft shadow. */
private fun DrawScope.knob(c: Offset, r: Float, fill: Color) {
    drawIntoCanvas { canvas ->
        val shadow = Paint().apply {
            color = Color(0x59000000)
            asFrameworkPaint().maskFilter = BlurMaskFilter(4.dp.toPx(), BlurMaskFilter.Blur.NORMAL)
        }
        canvas.drawCircle(c + Offset(0f, 2.dp.toPx()), r, shadow)
    }
    drawCircle(fill, r, c)
    drawCircle(Color.White, r - 0.6.dp.toPx(), c, style = Stroke(1.2.dp.toPx()))
}

@Composable
private fun SaturationBrightness(c: ColorPick, compact: Boolean, height: Dp = 344.dp) {
    val hue by rememberUpdatedState(c.hue)
    val pure = hsvColor(c.hue, 1f, 1f)
    val current = rgbColor(c.rgb())
    Box(
        Modifier.padding(horizontal = if (compact) 6.dp else 16.dp).size(if (compact) 248.dp else 344.dp, if (compact) 248.dp else height).drawWithContent {
            drawContent()
            // the knob may hang over the square's edge
            knob(Offset(c.saturation * size.width, (1f - c.brightness) * size.height), 10.75.dp.toPx(), current)
        },
    ) {
        Box(
            Modifier.fillMaxSize().clip(RoundedCornerShape(10.dp))
                .pointerInput(Unit) { track { x, y -> c.setHsv(hue, x, 1f - y) } }
                .drawBehind {
                    drawRect(Brush.horizontalGradient(oneSpace(listOf(Color.White, pure))))
                    drawRect(Brush.verticalGradient(listOf(Color.Transparent, Color.Black)))
                },
        )
    }
}

/**
 * A 36-tall slider row: a 4-dp track through [colours] and a pill showing the colour under it.
 * Touching anywhere on the row moves the pill there.
 */
@Composable
private fun SliderRow(value: Float, colours: List<Color>, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    Box(
        Modifier.fillMaxWidth().height(36.dp)
            .pointerInput(Unit) { track { x, _ -> change(x) } }
            .drawBehind {
                val trackH = 4.dp.toPx()
                val y = size.height / 2f
                drawRoundRect(Brush.horizontalGradient(oneSpace(colours)), Offset(0f, y - trackH / 2f), Size(size.width, trackH), CornerRadius(trackH / 2f))
                val pill = Size(16.dp.toPx(), 10.dp.toPx())
                val x = (value * size.width).coerceIn(pill.width / 2f, size.width - pill.width / 2f)
                drawRoundRect(colourAlong(colours, value), Offset(x - pill.width / 2f, y - pill.height / 2f), pill, CornerRadius(pill.height / 2f))
            },
    )
}

// The harmony disc is a hue/saturation wheel in Display P3: on an sRGB canvas its colours are
// turned into sRGB (clipped), on a P3 one they are taken as they are (both spaces use the sRGB
// transfer curve).
private fun toLinear(c: Float) = if (c <= 0.04045f) c / 12.92f else ((c + 0.055f) / 1.055f).pow(2.4f)

private fun toEncoded(c: Float) = if (c <= 0.0031308f) c * 12.92f else 1.055f * c.pow(1f / 2.4f) - 0.055f

/** P3 hue (0..1), saturation and value to a canvas colour (0xRRGGBB), clipped to sRGB on an sRGB canvas. */
private fun p3HsvToRgb(h: Float, s: Float, v: Float): Int {
    val p = CanvasViewModel.hsvToRgb(h, s, v)
    if (CanvasSpace.p3) return p
    val r = toLinear(((p shr 16) and 0xFF) / 255f)
    val g = toLinear(((p shr 8) and 0xFF) / 255f)
    val b = toLinear((p and 0xFF) / 255f)
    fun out(x: Float) = (toEncoded(x.coerceIn(0f, 1f)) * 255f).roundToInt()
    return (out(1.2249f * r - 0.2247f * g) shl 16) or (out(-0.0420f * r + 1.0419f * g) shl 8) or out(-0.0197f * r - 0.0786f * g + 1.0979f * b)
}

/** A canvas colour (0xRRGGBB) as P3 hue (0..1), saturation and value. */
private fun rgbToP3Hsv(rgb: Int): FloatArray {
    if (CanvasSpace.p3) {
        val hsv = FloatArray(3)
        android.graphics.Color.RGBToHSV((rgb shr 16) and 0xFF, (rgb shr 8) and 0xFF, rgb and 0xFF, hsv)
        return floatArrayOf(hsv[0] / 360f, hsv[1], hsv[2])
    }
    val r = toLinear(((rgb shr 16) and 0xFF) / 255f)
    val g = toLinear(((rgb shr 8) and 0xFF) / 255f)
    val b = toLinear((rgb and 0xFF) / 255f)
    fun enc(x: Float) = (toEncoded(x.coerceIn(0f, 1f)) * 255f).roundToInt()
    val hsv = FloatArray(3)
    android.graphics.Color.RGBToHSV(enc(0.8225f * r + 0.1774f * g), enc(0.0332f * r + 0.9669f * g), enc(0.0171f * r + 0.0724f * g + 0.9108f * b), hsv)
    return floatArrayOf(hsv[0] / 360f, hsv[1], hsv[2])
}

/** The harmony wheel at value [v], [side] pixels square: hue anticlockwise from the right, saturation outwards. */
private fun harmonyImage(v: Float, side: Int): ImageBitmap {
    val pixels = IntArray(side * side)
    val half = side / 2f
    for (j in 0 until side) {
        for (i in 0 until side) {
            val u = (i + 0.5f) / half - 1f
            val w = (j + 0.5f) / half - 1f
            val d = sqrt(u * u + w * w)
            val edge = ((1f - d) * half + 0.5f).coerceIn(0f, 1f)
            if (edge <= 0f) continue
            val h = (atan2(-w, u) / (2f * PI.toFloat()) + 1f) % 1f
            pixels[j * side + i] = ((edge * 255f).toInt() shl 24) or p3HsvToRgb(h, d.coerceAtMost(1f), v)
        }
    }
    return CanvasSpace.tag(android.graphics.Bitmap.createBitmap(pixels, side, side, android.graphics.Bitmap.Config.ARGB_8888)).asImageBitmap()
}

/** The harmony kind under the title; tapping it lists the kinds. */
@Composable
private fun HarmonyKindButton(vm: CanvasViewModel) {
    var open by remember { mutableStateOf(false) }
    Box(Modifier.padding(start = 20.dp, top = 50.dp).height(28.dp).clickable { open = true }, contentAlignment = Alignment.CenterStart) {
        Text(vm.harmonyKind.label, color = Pc.PanelTitle, fontSize = 17.sp)
        MenuPresence(open, androidx.compose.ui.graphics.TransformOrigin(1f, 0f)) { look ->
            // the menu opens to the left of the panel, its top level with the wheel
            Popup(offset = with(LocalDensity.current) { IntOffset((-204).dp.roundToPx(), 36.dp.roundToPx()) }, properties = PopupProperties(focusable = true), onDismissRequest = { open = false }) {
                Column(look.width(195.dp).clip(RoundedCornerShape(24.dp)).background(Pc.MenuFill).padding(vertical = 9.dp)) {
                    for (kind in HarmonyKind.entries) {
                        Row(
                            Modifier.fillMaxWidth().height(38.dp).clickable {
                                vm.harmonyKind = kind
                                open = false
                            }.padding(start = 20.dp, end = 18.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(kind.label, color = Color.White, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
                            if (kind == vm.harmonyKind) Icon(PcIcons.Tick, contentDescription = null, tint = Color.White, modifier = Modifier.size(18.dp))
                        }
                    }
                }
            }
        }
    }
}

/**
 * The harmony picker: a 298 dp wheel (hue around, saturation out, in Display P3) with a knob for
 * the colour and one for each companion of the harmony kind, and a brightness slider below.
 */
@Composable
private fun Harmony(vm: CanvasViewModel, c: ColorPick, compact: Boolean = false, short: Boolean = false) {
    val rgb = c.rgb()
    val hsv = remember(rgb) { rgbToP3Hsv(rgb) }
    val state by rememberUpdatedState(hsv)
    val density = LocalDensity.current
    // compact, the wheel is 200 across instead of 298; short, 296
    val radius = if (compact) 100.dp else if (short) HarmonyRadius - 1.dp else HarmonyRadius
    val wheel = remember(hsv[2], radius) { harmonyImage(hsv[2], with(density) { radius.roundToPx() }) }
    // the header is 78 tall here, with the kind under the title; the wheel starts 88 down (short, 82)
    Column(Modifier.padding(top = if (short) 4.dp else 10.dp)) {
        Box(
            Modifier.padding(start = if (compact) 30.dp else PanelWidth / 2 - radius).size(radius * 2)
                .pointerInput(Unit) {
                    track { x, y ->
                        val u = x * 2f - 1f
                        val w = y * 2f - 1f
                        val h = (atan2(-w, u) / (2f * PI.toFloat()) + 1f) % 1f
                        c.setRgb(p3HsvToRgb(h, sqrt(u * u + w * w).coerceAtMost(1f), state[2]))
                    }
                }
                .drawBehind {
                    drawImage(wheel, dstSize = IntSize(size.width.roundToInt(), size.height.roundToInt()))
                    val r = size.width / 2f
                    fun at(turn: Float) = center + Offset(cos(turn) * hsv[1] * r, -sin(turn) * hsv[1] * r)
                    for (t in vm.harmonyKind.turns) {
                        val h = ((hsv[0] + t / 360f) % 1f + 1f) % 1f
                        knob(at(h * 2f * PI.toFloat()), 10.75.dp.toPx(), rgbColor(p3HsvToRgb(h, hsv[1], hsv[2])))
                    }
                    knob(at(hsv[0] * 2f * PI.toFloat()), 14.5.dp.toPx(), rgbColor(rgb))
                },
        )
        Box(Modifier.padding(start = if (compact) 10.dp else 20.dp, end = if (compact) 10.dp else 20.dp, top = if (compact) 8.dp else if (short) 10.dp else 31.dp)) {
            SliderRow(hsv[2], listOf(Color.Black, rgbColor(p3HsvToRgb(hsv[0], hsv[1], 1f)))) { c.setRgb(p3HsvToRgb(state[0], state[1], it)) }
        }
    }
}

/** All palettes as compact cards: the default one ticked, each with its menu and its swatches. */
@Composable
private fun Palettes(vm: CanvasViewModel, c: ColorPick, short: Boolean) {
    var renaming by remember { mutableStateOf<Int?>(null) }
    val cards = vm.paletteCards
    Row(Modifier.padding(horizontal = 8.5.dp).fillMaxWidth().height(36.dp).clip(RoundedCornerShape(9.dp)).background(Pc.SetCell).padding(2.dp)) {
        for ((on, icon, label) in listOf(Triple(!cards, PcIcons.CardCompact, "Compact"), Triple(cards, PcIcons.CardStack, "Cards"))) {
            Row(
                Modifier.weight(1f).fillMaxSize().clip(RoundedCornerShape(7.dp)).background(if (on) Pc.Accent else Color.Transparent)
                    .clickable { vm.changePaletteCards(label == "Cards") },
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                // drawn in a 21 x 32 frame, shown as it is
                Icon(icon, contentDescription = null, tint = if (on) Color.White else Pc.PanelTitle, modifier = Modifier.requiredSize(21.dp, 32.dp))
                Text(label, color = if (on) Color.White else Pc.PanelTitle, fontSize = 15.sp, fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal, modifier = Modifier.padding(start = 6.dp))
            }
        }
    }
    LazyColumn(
        Modifier.padding(top = 8.dp).fillMaxWidth().height(if (short) 458.dp else 577.dp),
        // cards: three 120 tiles across
        contentPadding = if (cards) PaddingValues(start = 8.dp, end = 8.dp, bottom = 8.dp) else PaddingValues(start = 8.5.dp, end = 8.5.dp, bottom = 8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        itemsIndexed(vm.palettes) { i, p ->
            Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(Pc.SetCell)) {
                Row(Modifier.fillMaxWidth().height(44.dp), verticalAlignment = Alignment.CenterVertically) {
                    if (i == vm.defaultPalette) {
                        Box(Modifier.padding(start = 11.5.dp).size(20.dp).clip(CircleShape).background(Pc.Accent), contentAlignment = Alignment.Center) {
                            Icon(PcIcons.Tick, contentDescription = "Default", tint = Color.White, modifier = Modifier.size(14.dp))
                        }
                    }
                    // a tap on the name renames it in place
                    Box(Modifier.weight(1f).padding(start = if (i == vm.defaultPalette) 4.dp else 12.dp)) {
                        if (renaming == i) {
                            InlineName(p.name, Pc.PanelTitle, 14.sp, Modifier.fillMaxWidth()) {
                                vm.renamePalette(i, it)
                                renaming = null
                            }
                        } else {
                            Text(p.name, color = Pc.PanelTitle, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, modifier = Modifier.clickable { renaming = i })
                        }
                    }
                    PaletteMenu(vm, i)
                }
                if (cards) PaletteTiles(vm, c, i, p) else SwatchGrid(vm, c, i, p, Modifier)
            }
        }
    }
}

/** The ... button of a palette card and its menu: Set as active, Duplicate, Share; Delete. */
@Composable
private fun PaletteMenu(vm: CanvasViewModel, i: Int) {
    var open by remember { mutableStateOf(false) }
    val context = androidx.compose.ui.platform.LocalContext.current
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    Box(Modifier.padding(end = 8.5.dp).size(30.dp, 44.dp).clickable { open = true }, contentAlignment = Alignment.Center) {
        Icon(PcIcons.More, contentDescription = "More", tint = Pc.PanelTitle, modifier = Modifier.size(20.dp))
        // under the button, its right edge near the button's
        LibraryMenu(open, androidx.compose.ui.graphics.TransformOrigin(1f, 0f), MenuPlace { a, size -> IntOffset(a.right - size.width, a.bottom) }, onDismiss = { open = false }, overPanel = true) {
            MenuItem(PcIcons.MenuCheckCircle, "Set as active") {
                open = false
                vm.chooseDefaultPalette(i)
            }
            MenuItem(PcIcons.MenuDuplicate, "Duplicate") {
                open = false
                vm.duplicatePalette(i)
            }
            MenuItem(PcIcons.MenuShare, "Share") {
                open = false
                scope.launch { vm.paletteFile(i)?.let { shareFile(context, it, "application/octet-stream") } }
            }
            if (vm.palettes.size > 1) {
                MenuDivider()
                MenuItem(PcIcons.MenuTrash, "Delete", Pc.MenuDanger) {
                    open = false
                    vm.deletePalette(i)
                }
            }
        }
    }
}

/**
 * The palettes' "+" and its menu: a new palette, empty or from a photo taken, a file or a picture,
 * and our example palettes back.
 */
@Composable
private fun NewPaletteButton(vm: CanvasViewModel) {
    var open by remember { mutableStateOf(false) }
    val context = androidx.compose.ui.platform.LocalContext.current
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    val photo = remember {
        val dir = java.io.File(context.cacheDir, "exports").apply { mkdirs() }
        androidx.core.content.FileProvider.getUriForFile(context, context.packageName + ".files", java.io.File(dir, "palette-camera.jpg"))
    }
    val camera = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.TakePicture()) { taken ->
        if (taken) vm.paletteFromImage(photo)
    }
    // a file: a palette's, or a picture to take colours from
    val file = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { scope.launch { if (!vm.importSwatches(it)) vm.paletteFromImage(it) } }
    }
    val photos = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia()) { uri ->
        uri?.let(vm::paletteFromImage)
    }
    Box(Modifier.padding(top = 12.dp).size(40.dp, 41.dp).clickable { open = true }, contentAlignment = Alignment.Center) {
        Icon(PcIcons.PlusMedium, contentDescription = "New palette", tint = Pc.PanelGlyph, modifier = Modifier.requiredSize(41.dp))
        LibraryMenu(open, androidx.compose.ui.graphics.TransformOrigin(1f, 0f), MenuPlace { a, size -> IntOffset(a.left - size.width, a.bottom) }, onDismiss = { open = false }, overPanel = true) {
            MenuItem(PcIcons.MenuGrid, "Create new palette") {
                open = false
                vm.addPalette()
            }
            MenuItem(PcIcons.MenuCamera, "New from camera") {
                open = false
                runCatching { camera.launch(photo) }.onFailure { vm.message = "No camera app is available." }
            }
            MenuItem(PcIcons.MenuFolder, "New from file") {
                open = false
                file.launch(arrayOf("*/*"))
            }
            MenuItem(PcIcons.MenuPhoto, "New from photos") {
                open = false
                photos.launch(androidx.activity.result.PickVisualMediaRequest(androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia.ImageOnly))
            }
            MenuDivider()
            MenuItem(PcIcons.MenuRestore, "Restore example palettes") {
                open = false
                vm.restoreExamplePalettes()
            }
        }
    }
}

/**
 * A palette's swatches, ten by three, 36 dp each. Tap a colour to paint with it, tap an empty
 * slot to keep the current colour there, hold a colour to take it out.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun SwatchGrid(vm: CanvasViewModel, c: ColorPick, index: Int, p: Palette, modifier: Modifier) {
    Column(modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp))) {
        for (row in 0 until 3) {
            Row {
                for (col in 0 until 10) {
                    val slot = row * 10 + col
                    val rgb = p.colors[slot]
                    Box(
                        Modifier.weight(1f).height(36.dp).pressScale().background(if (rgb != null) rgbColor(rgb) else Pc.EmptySlot)
                            .combinedClickable(
                                onClick = { if (rgb != null) c.setRgb(rgb) else vm.setSwatch(index, slot, c.rgb()) },
                                onLongClick = { if (rgb != null) vm.setSwatch(index, slot, null) },
                            ),
                    )
                }
            }
        }
    }
}

/**
 * A palette's colours as named tiles, three across, 120 square: a tap takes the colour, a hold
 * takes it out of the palette.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun PaletteTiles(vm: CanvasViewModel, c: ColorPick, index: Int, p: Palette) {
    val filled = p.colors.withIndex().mapNotNull { (slot, rgb) -> rgb?.let { slot to it } }
    Column {
        for (row in filled.chunked(3)) {
            Row {
                for ((slot, rgb) in row) {
                    val hsv = FloatArray(3).also { android.graphics.Color.colorToHSV(rgb, it) }
                    Box(
                        Modifier.size(120.dp).pressScale().background(rgbColor(rgb)).combinedClickable(
                            onClick = { c.setRgb(rgb) },
                            onLongClick = { vm.setSwatch(index, slot, null) },
                        ),
                        contentAlignment = Alignment.BottomStart,
                    ) {
                        // dark lettering on bright colours
                        Text(
                            colourName(rgb),
                            color = if (hsv[2] >= 0.85f) Color.Black else Color.White,
                            fontSize = 15.sp,
                            lineHeight = 17.sp,
                            modifier = Modifier.padding(start = 8.dp, end = 8.dp, bottom = 10.dp),
                        )
                    }
                }
            }
        }
    }
}

/**
 * A plain name for a colour (0xRRGGBB): a grey by its lightness, or a hue with "Dark", "Light" or
 * "Grayish" before it.
 */
internal fun colourName(rgb: Int): String {
    val r = ((rgb shr 16) and 0xFF) / 255f
    val g = ((rgb shr 8) and 0xFF) / 255f
    val b = (rgb and 0xFF) / 255f
    val max = maxOf(r, g, b)
    val min = minOf(r, g, b)
    val lightness = (max + min) / 2f
    val saturation = if (max > 0f) (max - min) / max else 0f
    if (saturation < 0.05f) {
        return when {
            lightness > 0.98f -> "White"
            lightness > 0.75f -> "Light Gray"
            lightness > 0.35f -> "Gray"
            lightness > 0.08f -> "Dark Gray"
            else -> "Black"
        }
    }
    val hsv = FloatArray(3).also { android.graphics.Color.colorToHSV(rgb, it) }
    val hue = HueNames.firstOrNull { hsv[0] < it.first }?.second ?: "Red"
    return when {
        saturation < 0.2f -> "Grayish $hue"
        lightness < 0.5f -> "Dark $hue"
        lightness > 0.8f -> "Light $hue"
        else -> hue
    }
}

/** Hue names, each up to the hue (degrees) it ends at. */
private val HueNames = listOf(
    8f to "Red", 20f to "Red Orange", 40f to "Orange", 50f to "Yellow Orange", 65f to "Yellow",
    80f to "Yellow Green", 150f to "Green", 170f to "Green Cyan", 195f to "Cyan", 220f to "Sky Blue",
    250f to "Blue", 275f to "Violet", 290f to "Purple", 320f to "Magenta", 325f to "Magenta Pink",
    342f to "Pink", 352f to "Pink Red", 361f to "Red",
)

/** A CMYK canvas's inks (cyan, magenta, yellow, black; 0..1) for a canvas colour, and back. */
class Inks(val of: (Int) -> FloatArray, val rgb: (FloatArray) -> Int)

/**
 * Number sliders: hue, saturation and brightness, then red, green and blue (on a CMYK canvas:
 * cyan, magenta, yellow and black, the groups closer together), then the hex code.
 */
@Composable
private fun Value(c: ColorPick, compact: Boolean = false, short: Boolean = false, inks: Inks? = null) {
    val rgb = c.rgb()
    val r = (rgb shr 16) and 0xFF
    val g = (rgb shr 8) and 0xFF
    val b = rgb and 0xFF
    if (compact) {
        CompactValue(c)
        return
    }
    if (inks != null) {
        InkValue(c, inks)
        return
    }
    Column(Modifier.padding(horizontal = 8.dp)) {
        ValueGroup(short = short) {
            ValueRow("H", huePosition(SliderHues, c.hue), SliderHues.map { hsvColor(it / 360f, 1f, 1f) }, "${(c.hue * 360f).roundToInt()}°") {
                c.setHsv(hueAt(SliderHues, it), c.saturation, c.brightness)
            }
            ValueRow("S", c.saturation, listOf(Pc.SaturationGrey, hsvColor(c.hue, 1f, 1f)), "${(c.saturation * 100f).roundToInt()}%") {
                c.setHsv(c.hue, it, c.brightness)
            }
            ValueRow("B", c.brightness, listOf(Color.Black, hsvColor(c.hue, c.saturation, 1f)), "${(c.brightness * 100f).roundToInt()}%") {
                c.setHsv(c.hue, c.saturation, it)
            }
        }
        ValueGroup(Modifier.padding(top = 6.dp), short) {
            ValueRow("R", r / 255f, listOf(Color(0, g, b), Color(255, g, b)), "$r") { c.setRgb(((it * 255f).roundToInt() shl 16) or (g shl 8) or b) }
            ValueRow("G", g / 255f, listOf(Color(r, 0, b), Color(r, 255, b)), "$g") { c.setRgb((r shl 16) or ((it * 255f).roundToInt() shl 8) or b) }
            ValueRow("B", b / 255f, listOf(Color(r, g, 0), Color(r, g, 255)), "$b") { c.setRgb((r shl 16) or (g shl 8) or (it * 255f).roundToInt()) }
        }
        HexRow(rgb) { c.setRgb(it) }
        if (short) Spacer(Modifier.height(2.dp))
    }
}

/** Value on a CMYK canvas: the hue, saturation and brightness, the four inks, then the hex code. */
@Composable
private fun InkValue(c: ColorPick, inks: Inks) {
    val rgb = c.rgb()
    // the inks dragged to, kept while the colour they make is the colour (not worked out again
    // from it, which could move the other inks)
    var held by remember { mutableStateOf<Pair<Int, FloatArray>?>(null) }
    val worked = remember(rgb) { inks.of(rgb) }
    val now = held?.takeIf { it.first == rgb }?.second ?: worked
    fun inksWith(k: Int, v: Float) = now.copyOf().also { it[k] = v.coerceIn(0f, 1f) }
    Column(Modifier.padding(horizontal = 8.dp)) {
        ValueGroup(height = 142.dp, gap = 4.dp, top = 1.dp) {
            ValueRow("H", huePosition(SliderHues, c.hue), SliderHues.map { hsvColor(it / 360f, 1f, 1f) }, "${(c.hue * 360f).roundToInt()}°") {
                c.setHsv(hueAt(SliderHues, it), c.saturation, c.brightness)
            }
            ValueRow("S", c.saturation, listOf(Pc.SaturationGrey, hsvColor(c.hue, 1f, 1f)), "${(c.saturation * 100f).roundToInt()}%") {
                c.setHsv(c.hue, it, c.brightness)
            }
            ValueRow("B", c.brightness, listOf(Color.Black, hsvColor(c.hue, c.saturation, 1f)), "${(c.brightness * 100f).roundToInt()}%") {
                c.setHsv(c.hue, c.saturation, it)
            }
        }
        ValueGroup(Modifier.padding(top = 8.dp), height = 187.dp, gap = 3.dp, top = 1.dp) {
            for ((k, letter) in listOf("C", "M", "Y", "K").withIndex()) {
                val v = now.getOrElse(k) { 0f }
                ValueRow(letter, v, listOf(rgbColor(inks.rgb(inksWith(k, 0f))), rgbColor(inks.rgb(inksWith(k, 1f)))), "${(v * 100f).roundToInt()}%") {
                    val next = inksWith(k, it)
                    val made = inks.rgb(next)
                    held = made to next
                    c.setRgb(made)
                }
            }
        }
        HexRow(rgb, top = 8.dp, height = 42.dp) { c.setRgb(it) }
    }
}

/** Value, compact: HSB or RGB (a switch picks), then the hex code; 240 wide. */
@Composable
private fun CompactValue(c: ColorPick) {
    var rgbShown by remember { mutableStateOf(false) }
    val rgb = c.rgb()
    val r = (rgb shr 16) and 0xFF
    val g = (rgb shr 8) and 0xFF
    val b = rgb and 0xFF
    Column(Modifier.padding(horizontal = 10.dp)) {
        Row(Modifier.fillMaxWidth().height(36.dp).clip(RoundedCornerShape(9.dp)).background(CompactGroup)) {
            for ((i, label) in listOf("HSB", "RGB").withIndex()) {
                val on = rgbShown == (i == 1)
                Box(
                    Modifier.weight(1f).fillMaxSize().clip(RoundedCornerShape(9.dp)).background(if (on) Pc.Accent else Color.Transparent).clickable { rgbShown = i == 1 },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(label, color = if (on) Color.White else Pc.PanelTitle, fontSize = 15.sp, fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal)
                }
            }
        }
        Column(
            Modifier.padding(top = 6.dp).fillMaxWidth().height(188.dp).clip(RoundedCornerShape(10.dp)).background(CompactGroup).padding(start = 12.dp, end = 12.dp, top = 10.dp),
            verticalArrangement = Arrangement.spacedBy(18.dp),
        ) {
            if (rgbShown) {
                CompactValueRow("R", r / 255f, listOf(Color(0, g, b), Color(255, g, b)), "$r") { c.setRgb(((it * 255f).roundToInt() shl 16) or (g shl 8) or b) }
                CompactValueRow("G", g / 255f, listOf(Color(r, 0, b), Color(r, 255, b)), "$g") { c.setRgb((r shl 16) or ((it * 255f).roundToInt() shl 8) or b) }
                CompactValueRow("B", b / 255f, listOf(Color(r, g, 0), Color(r, g, 255)), "$b") { c.setRgb((r shl 16) or (g shl 8) or (it * 255f).roundToInt()) }
            } else {
                CompactValueRow("H", huePosition(SliderHues, c.hue), SliderHues.map { hsvColor(it / 360f, 1f, 1f) }, "${(c.hue * 360f).roundToInt()}°") {
                    c.setHsv(hueAt(SliderHues, it), c.saturation, c.brightness)
                }
                CompactValueRow("S", c.saturation, listOf(Pc.SaturationGrey, hsvColor(c.hue, 1f, 1f)), "${(c.saturation * 100f).roundToInt()}%") {
                    c.setHsv(c.hue, it, c.brightness)
                }
                CompactValueRow("B", c.brightness, listOf(Color.Black, hsvColor(c.hue, c.saturation, 1f)), "${(c.brightness * 100f).roundToInt()}%") {
                    c.setHsv(c.hue, c.saturation, it)
                }
            }
        }
        HexRow(rgb, CompactGroup, CompactHexField) { c.setRgb(it) }
        Spacer(Modifier.height(10.dp))
    }
}

/** One compact number slider, 216 wide: its letter, the track and the value. */
@Composable
private fun CompactValueRow(letter: String, value: Float, colours: List<Color>, shown: String, onChange: (Float) -> Unit) {
    Row(Modifier.fillMaxWidth().height(44.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(letter, color = Pc.PanelTitle, fontSize = 15.sp, modifier = Modifier.width(20.dp))
        Box(Modifier.weight(1f)) { SliderRow(value, colours, onChange) }
        Text(shown, color = Pc.PanelTitle, fontSize = 15.sp, maxLines = 1, textAlign = TextAlign.End, modifier = Modifier.width(48.dp))
    }
}

@Composable
private fun ValueGroup(modifier: Modifier = Modifier, short: Boolean = false, rows: @Composable () -> Unit) {
    // short, the rows are 51 apart instead of 53
    if (short) ValueGroup(modifier, 151.dp, 7.dp, 3.dp, rows) else ValueGroup(modifier, 167.dp, 9.dp, 9.dp, rows)
}

/** A group of number sliders [height] tall, [gap] between rows, [top] above the first. */
@Composable
private fun ValueGroup(modifier: Modifier = Modifier, height: Dp, gap: Dp, top: Dp, rows: @Composable () -> Unit) {
    Column(
        modifier.fillMaxWidth().height(height).clip(RoundedCornerShape(10.dp)).background(Pc.ValueGroup).padding(top = top),
        verticalArrangement = Arrangement.spacedBy(gap),
    ) { rows() }
}

/** One number slider: its letter, a track from 32 to 296 dp, and the value centred at 326.5. */
@Composable
private fun ValueRow(letter: String, value: Float, colours: List<Color>, shown: String, onChange: (Float) -> Unit) {
    Box(Modifier.fillMaxWidth().height(44.dp)) {
        Text(letter, color = Pc.PanelTitle, fontSize = 15.sp, modifier = Modifier.align(Alignment.CenterStart).padding(start = 13.dp))
        Box(Modifier.padding(start = 32.dp).width(264.dp).align(Alignment.CenterStart)) {
            SliderRow(value, colours, onChange)
        }
        Box(Modifier.align(Alignment.CenterStart).padding(start = 304.dp).width(45.dp), contentAlignment = Alignment.Center) {
            Text(shown, color = Pc.PanelTitle, fontSize = 15.sp, maxLines = 1)
        }
    }
}

/**
 * The colour as a hex code: a tap opens the hex keypad beside the panel, and the code typed on it
 * (six digits) sets the colour.
 */
@Composable
private fun HexRow(rgb: Int, fill: Color = Pc.HexRow, field: Color = Pc.HexField, top: Dp = 6.dp, height: Dp = 44.dp, onSet: (Int) -> Unit) {
    // the digits typed since the keypad opened; null while it is shut
    var typed by remember { mutableStateOf<String?>(null) }
    Box(Modifier.padding(top = top).fillMaxWidth().height(height).clip(RoundedCornerShape(10.dp)).background(fill)) {
        Text("Hexadecimal", color = Pc.PanelTitle, fontSize = 16.sp, modifier = Modifier.align(Alignment.CenterStart).padding(start = 13.dp))
        val editing = typed != null
        Box(
            Modifier.align(Alignment.CenterEnd).padding(end = 8.dp).size(84.5.dp, 28.dp).clip(RoundedCornerShape(7.dp)).background(field)
                .then(if (editing) Modifier.border(1.dp, Pc.Accent, RoundedCornerShape(7.dp)) else Modifier)
                .clickable { typed = "" },
            contentAlignment = Alignment.Center,
        ) {
            Text("#" + (typed?.takeIf { it.isNotEmpty() } ?: "%06X".format(rgb)), color = Pc.PanelTitle, fontSize = 14.sp, maxLines = 1)
        }
        if (editing) {
            // beside the panel, its middle level with this row
            val gap = with(LocalDensity.current) { HexPadGap.roundToPx() }
            Popup(popupPositionProvider = remember(gap) { BesidePanel(gap) }, onDismissRequest = { typed = null }, properties = PopupProperties(focusable = true)) {
                HexPad(
                    onKey = { c ->
                        // a seventh digit starts a new code
                        val next = ((typed ?: "").let { if (it.length >= 6) "" else it } + c)
                        typed = next
                        if (next.length == 6) next.toIntOrNull(16)?.let(onSet)
                    },
                    onBack = { typed = typed?.dropLast(1) },
                )
            }
        }
    }
}

/** From the hex row to the keypad's right edge: the panel's padding, then a small gap. */
private val HexPadGap = 12.5.dp

/** Puts a popup to the left of the anchor, its middle level with the anchor's, inside the window. */
private class BesidePanel(private val gap: Int) : androidx.compose.ui.window.PopupPositionProvider {
    override fun calculatePosition(
        anchorBounds: androidx.compose.ui.unit.IntRect,
        windowSize: IntSize,
        layoutDirection: androidx.compose.ui.unit.LayoutDirection,
        popupContentSize: IntSize,
    ): IntOffset {
        val x = (anchorBounds.left - gap - popupContentSize.width).coerceAtLeast(0)
        val y = (anchorBounds.center.y - popupContentSize.height / 2).coerceIn(0, (windowSize.height - popupContentSize.height).coerceAtLeast(0))
        return IntOffset(x, y)
    }
}

/** The hex keypad: D E F, A B C, 7 8 9, 4 5 6, 1 2 3, then a wide 0 and backspace; 259 × 338. */
@Composable
private fun HexPad(onKey: (Char) -> Unit, onBack: () -> Unit) {
    val rows = listOf("DEF", "ABC", "789", "456", "123")
    Column(
        Modifier.size(259.dp, 338.dp).clip(RoundedCornerShape(22.dp)).background(HexPadFill).border(0.5.dp, HexPadEdge, RoundedCornerShape(22.dp))
            .padding(horizontal = 10.5.dp, vertical = 9.5.dp),
        verticalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        for (row in rows) {
            Row(horizontalArrangement = Arrangement.spacedBy(4.5.dp)) {
                for (c in row) HexKey(Modifier.width(76.5.dp), { onKey(c) }) { Text("$c", color = HexKeyInk, fontSize = 21.sp) }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(4.5.dp)) {
            HexKey(Modifier.width(157.5.dp), { onKey('0') }) { Text("0", color = HexKeyInk, fontSize = 21.sp) }
            HexKey(Modifier.width(76.5.dp), onBack) {
                Icon(PcIcons.HexBackspace, contentDescription = "Delete", tint = HexBackInk, modifier = Modifier.size(26.dp))
            }
        }
    }
}

/** One key of the hex keypad, 49 tall, lighter while held. */
@Composable
private fun HexKey(modifier: Modifier, onClick: () -> Unit, label: @Composable () -> Unit) {
    val press = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
    val held by press.collectIsPressedAsState()
    Box(
        modifier.height(49.dp).clip(RoundedCornerShape(18.dp)).background(if (held) HexKeyHeld else HexKeyFill)
            .clickable(interactionSource = press, indication = null, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) { label() }
}

private val HexPadFill = Color(0xFF616161)
private val HexPadEdge = Color(0xFF262626)
private val HexKeyFill = Color(0xFF6C6C6C)
private val HexKeyHeld = Color(0xFF8A8A8A)
private val HexKeyInk = Color(0xFFEEEEEE)
private val HexBackInk = Color(0xFFBABABA)

/** The colour at [t] (0..1) along evenly spaced [colours]. */
private fun colourAlong(colours: List<Color>, t: Float): Color {
    val p = t.coerceIn(0f, 1f) * (colours.size - 1)
    val i = p.toInt().coerceAtMost(colours.size - 2)
    return lerp(colours[i], colours[i + 1], p - i)
}

@Composable
private fun History(vm: CanvasViewModel, c: ColorPick) {
    Box(Modifier.fillMaxWidth().height(44.dp)) {
        Text("History", color = Pc.PanelTitle, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 21.dp, top = 13.dp))
        Box(
            Modifier.align(Alignment.TopEnd).padding(top = 11.dp, end = 16.dp).size(86.dp, 22.5.dp).clip(RoundedCornerShape(7.dp))
                .background(Pc.SetCell).clickable { vm.clearHistory() },
            contentAlignment = Alignment.Center,
        ) {
            Text("Clear", color = Pc.ClearText, fontSize = 13.sp)
        }
    }
    HistoryStrip(vm, c)
}

/** The last colours painted with, ten 36 swatches; a tap takes one. */
@Composable
private fun HistoryStrip(vm: CanvasViewModel, c: ColorPick) {
    Row(Modifier.padding(horizontal = 8.dp).fillMaxWidth().height(36.dp).clip(RoundedCornerShape(8.dp))) {
        for (i in 0 until CanvasViewModel.HISTORY_SIZE) {
            val rgb = vm.history.getOrNull(i)
            Box(
                Modifier.weight(1f).height(36.dp)
                    .background(if (rgb != null) rgbColor(rgb) else Pc.EmptySwatch)
                    .then(if (rgb != null) Modifier.clickable { c.setRgb(rgb) } else Modifier),
            )
        }
    }
}

/**
 * The short panel's last block: the colour history (a 44 header and the strip) or the active
 * palette (a 34 header and its grid), whichever its "•••" menu picked.
 */
@Composable
private fun ShortBottom(vm: CanvasViewModel, c: ColorPick) {
    val palette = vm.palettes.getOrNull(vm.defaultPalette)
    val history = vm.colorHistoryShown || palette == null
    Box(Modifier.fillMaxWidth().height(if (history) 44.dp else 34.dp)) {
        Text(
            if (history) "History" else palette?.name.orEmpty(),
            color = Pc.PanelTitle,
            fontSize = 14.sp,
            fontWeight = FontWeight.SemiBold,
            maxLines = 1,
            modifier = Modifier.padding(start = 21.dp, top = if (history) 13.dp else 8.dp),
        )
        ShortBottomMenu(vm, history, Modifier.align(Alignment.CenterEnd))
    }
    if (history) {
        HistoryStrip(vm, c)
    } else if (palette != null) {
        SwatchGrid(vm, c, vm.defaultPalette, palette, Modifier.padding(horizontal = 8.dp))
    }
}

/**
 * Three dots 31 from the panel's right edge, opening a small menu under them: Color history or
 * Active palette (a radio each), and with the history shown, Clear history.
 */
@Composable
private fun ShortBottomMenu(vm: CanvasViewModel, history: Boolean, modifier: Modifier) {
    var open by remember { mutableStateOf(false) }
    Box(
        modifier.padding(end = 9.dp).size(44.dp, 34.dp).clickable { open = true }.drawBehind {
            val r = 1.8.dp.toPx()
            for (i in -1..1) drawCircle(Pc.PanelTitle, r, center + Offset(i * 6.5.dp.toPx(), 0f))
        },
    ) {
        MenuPresence(open, androidx.compose.ui.graphics.TransformOrigin(1f, 0f)) { look ->
            // its right edge 16 left of the dots, its top 15 below them
            Popup(offset = with(LocalDensity.current) { IntOffset((-246).dp.roundToPx(), 32.dp.roundToPx()) }, properties = PopupProperties(focusable = true), onDismissRequest = { open = false }) {
                Column(
                    look.width(252.dp).clip(RoundedCornerShape(17.dp)).background(Pc.GlassMenuOverPanel)
                        .border(1.dp, Pc.GlassMenuEdge, RoundedCornerShape(17.dp)).padding(top = 6.dp, bottom = if (history) 9.5.dp else 7.dp),
                ) {
                    for ((label, on) in listOf("Color history" to true, "Active palette" to false)) {
                        Row(
                            Modifier.fillMaxWidth().height(40.dp).clickable {
                                vm.changeColorHistoryShown(on)
                                open = false
                            }.padding(start = 20.dp, end = 19.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(label, color = Color.White, fontSize = 18.sp, maxLines = 1, modifier = Modifier.weight(1f))
                            Radio(on == history)
                        }
                    }
                    if (history) {
                        Box(Modifier.padding(start = 16.dp, end = 15.dp, top = 11.dp, bottom = 10.dp).fillMaxWidth().height(1.dp).background(Pc.GlassMenuEdge))
                        Row(
                            Modifier.fillMaxWidth().height(40.dp).clickable {
                                vm.clearHistory()
                                open = false
                            }.padding(start = 9.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            // a 41 frame centred 29.5 in
                            Icon(PcIcons.Sweep, contentDescription = null, tint = Pc.MenuDanger, modifier = Modifier.requiredSize(41.dp))
                            Text("Clear history", color = Pc.MenuDanger, fontSize = 18.sp, modifier = Modifier.padding(start = 2.dp))
                        }
                    }
                }
            }
        }
    }
}

/** A round choice mark, 18 across: a grey ring, or filled blue with a white tick when chosen. */
@Composable
private fun Radio(chosen: Boolean) {
    Box(
        Modifier.size(18.dp).clip(CircleShape).then(if (chosen) Modifier.background(Pc.Accent) else Modifier.border(1.5.dp, Pc.RadioRing, CircleShape)),
        contentAlignment = Alignment.Center,
    ) {
        if (chosen) Icon(PcIcons.Tick, contentDescription = null, tint = Color.White, modifier = Modifier.size(12.dp))
    }
}

private val Modes = listOf(
    Triple(ColorMode.DISC, PcIcons.ModeDisc, "Disc"),
    Triple(ColorMode.CLASSIC, PcIcons.ModeClassic, "Classic"),
    Triple(ColorMode.HARMONY, PcIcons.ModeHarmony, "Harmony"),
    Triple(ColorMode.VALUE, PcIcons.ModeValue, "Value"),
    Triple(ColorMode.PALETTES, PcIcons.ModePalettes, "Palettes"),
)

@Composable
private fun ModeTabs(vm: CanvasViewModel) {
    Row(Modifier.fillMaxWidth().height(70.dp)) {
        for ((mode, icon, label) in Modes) {
            ModeTab(icon, label, selected = vm.colorMode == mode, Modifier.weight(1f)) { vm.colorMode = mode }
        }
    }
}

@Composable
private fun ModeTab(icon: ImageVector, label: String, selected: Boolean, modifier: Modifier, onClick: () -> Unit) {
    // the chosen tab lights up as it is let go; held, its icon shrinks
    val press = rememberPressScale(Motion.TAB_PRESSED, Motion.highlightOn(), Motion.highlightOff())
    Box(modifier.fillMaxSize().then(press.touch).clickable(onClick = onClick)) {
        Icon(
            icon,
            contentDescription = null,
            tint = if (selected) Pc.Accent else Pc.PanelTitle,
            // a 36 frame centred 27 down
            modifier = Modifier.align(Alignment.TopCenter).padding(top = 9.dp).requiredSize(36.dp).scaledBy(press),
        )
        Text(
            label,
            color = if (selected) Color.White else Pc.PanelTitle,
            fontSize = 13.sp,
            fontWeight = FontWeight.Medium,
            maxLines = 1,
            modifier = Modifier.align(Alignment.TopCenter).padding(top = 41.dp),
        )
    }
}

/** Calls [onMove] with the touch position as 0..1 fractions of the area while it is held. */
private suspend fun PointerInputScope.track(onMove: (Float, Float) -> Unit) {
    awaitEachGesture {
        val down = awaitFirstDown()
        down.consume()
        fun send(p: Offset) = onMove((p.x / size.width).coerceIn(0f, 1f), (p.y / size.height).coerceIn(0f, 1f))
        send(down.position)
        while (true) {
            val event = awaitPointerEvent()
            val c = event.changes.firstOrNull { it.id == down.id } ?: break
            if (!c.pressed) break
            send(c.position)
            c.consume()
        }
    }
}
