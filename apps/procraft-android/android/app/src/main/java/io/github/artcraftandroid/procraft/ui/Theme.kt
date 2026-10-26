package io.github.artcraftandroid.procraft.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** Interface colours and sizes. Sizes in dp equal the reference's points. */
object Pc {
    /** The light interface (Prefs) is on: panels and bars go light grey with dark text. */
    var light by mutableStateOf(false)

    /** A colour for the interface as it is now: [dark], or [light] when the light interface is on. */
    private fun pick(dark: Long, light: Long) = Color(if (this.light) light else dark)

    val Panel: Color get() = pick(0xF7262626, 0xF7E6E6E6)
    val Row: Color get() = pick(0xFF303030, 0xFFD8D8D8)
    val RowSelected = Color(0xFF0076FD)
    val Accent = Color(0xFF0076FD)
    val Text: Color get() = pick(0xFFFFFFFF, 0xFF222222)
    val TextDim: Color get() = pick(0x99FFFFFF, 0x99000000)
    val Disabled: Color get() = pick(0x4DFFFFFF, 0x4D000000)
    val Divider: Color get() = pick(0x26FFFFFF, 0x1F000000)
    val Surround = Color(0xFF1C1C1C)

    // the bar across the top of the canvas
    /** Translucent: #333333 over white, as the reference's bar. */
    // light, see-through enough to read 226 over the light surround and 235 over white
    val Bar: Color get() = pick(0xE61C1C1C, 0xCFE6E6E6)
    val BarText: Color get() = pick(0xFFAAAAAA, 0xFF3B3B3B)
    val BarGlyph: Color get() = pick(0xFFC7C7C7, 0xFF373737)
    val BarGlyphSelected = Color(0xFF0076FD)
    val ActionDisc: Color get() = pick(0xFF141414, 0xFFEDEDED)
    val ActionGlyph: Color get() = pick(0xFF969696, 0xFF646464)
    val ColorRing: Color get() = pick(0xFF969696, 0xFF696969)
    val BarHeight = 44.dp
    val BarMargin = 40.dp
    val BarButtonWidth = 44.dp
    val BarButtonHeight = 38.dp
    val GalleryWidth = 58.dp
    val ActionDiscSize = 32.dp
    val ColorDot = 31.dp

    // the panels hanging from the top bar
    val PopoverFill: Color get() = pick(0xFF383838, 0xFFE6E6E6)
    /** A panel's own material over the canvas: what is behind it shows through, blurred. */
    val PopoverFrost: Color get() = pick(0xDE1E1E1E, 0xFFE6E6E6)
    /**
     * The gallery's popovers: their glass as it shows over the gallery's dark (nothing blurs what
     * is behind them there, and unblurred it shows through too sharply).
     */
    val GalleryPopover: Color get() = pick(0xFF212121, 0xFFE6E6E6)
    val PopoverShadow: Color get() = pick(0x38000000, 0x24000000)
    val PopoverShadowRadius = 51.dp
    val PopoverCorner = 35.dp
    val PopoverGap = 12.dp
    val ArrowHeight = 12.dp
    val ArrowShift = 2.dp
    val PanelTitle: Color get() = pick(0xFFC7C7C7, 0xFF393939)
    val PanelGlyph: Color get() = pick(0xFFC8C8C8, 0xFF393939)
    val ChevronDisc: Color get() = pick(0xFF4C4C4C, 0xFFCFCFCF)
    val ChevronGlyph: Color get() = pick(0xFF2E2E2E, 0xFFE6E6E6)
    // a little white over the panel, so they follow its frosting
    val LayerRow: Color get() = pick(0x0DFFFFFF, 0xFFD8D8D8)
    val BackgroundRow: Color get() = pick(0x0DFFFFFF, 0xFFD8D8D8)
    val CheckFill: Color get() = pick(0xFFC7C7C7, 0xFF575757)
    val ThumbBackground: Color get() = pick(0xFF4F4F4F, 0xFFC8C8C8)
    // the brush library is glass like the other panels: the sets on the panel's own frost, the
    // brushes a shade darker, the set cells a little lighter (measured over white and over the
    // dark surround: 58 / 31, 52 / 26, 64 / 42)
    val SetColumn: Color get() = pick(0x00000000, 0xFFE6E6E6)
    val SetCell: Color get() = pick(0x2E5B5B5B, 0xFFD7D7D7)
    val BrushColumn: Color get() = pick(0x26000000, 0xFFE0E0E0)
    val Grabber: Color get() = pick(0x7AC6C6C6, 0xFFB0B0B0)
    val SwatchEdge: Color get() = pick(0xFF5A5A5A, 0xFFC0C0C0)
    val SaturationGrey = Color(0xFF7F7F7F)
    val ClearText: Color get() = pick(0xFFB5B5B5, 0xFF4A4A4A)
    val EmptySwatch: Color get() = pick(0xFF2A2A2A, 0xFFD9D9D9)
    val ValueGroup: Color get() = pick(0xFF434343, 0xFFD5D5D5)
    val HexRow: Color get() = pick(0xFF414141, 0xFFD8D8D8)
    val HexField: Color get() = pick(0xFF4B4B4B, 0xFFCFCFCF)
    val MenuFill: Color get() = pick(0xFF5A5C5B, 0xFFF0F0F0)
    /** A menu's words: white, black on the light interface's menus. */
    val MenuInk: Color get() = pick(0xFFFFFFFF, 0xFF000000)
    /** The small frosted menus: over the canvas what is behind shows through, blurred. */
    val GlassMenu: Color get() = pick(0xCF313135, 0xFAF0F0F0)
    /** A small menu over a panel: the panel can't show through blurred, so it is nearly solid, grey. */
    val GlassMenuOverPanel: Color get() = pick(0xFA222222, 0xFAF0F0F0)
    val GlassMenuEdge: Color get() = pick(0x24FFFFFF, 0x14000000)
    val RadioRing: Color get() = pick(0xFF606060, 0xFFB0B0B0)
    val MenuDanger: Color get() = pick(0xFFE84F51, 0xFFD93A3D)
    val MenuLine: Color get() = pick(0x33FFFFFF, 0x26000000)
    val MenuDisabled: Color get() = pick(0x66FFFFFF, 0x66000000)
    val SliderRest: Color get() = pick(0xFF5A5A5A, 0xFFBEBEBE)
    val EmptySlot: Color get() = pick(0xFF555555, 0xFFC8C8C8)
    // a dark wash on the panel's glass (26 over the dark surround, 44 over white)
    val ActionsGroup: Color get() = pick(0x420C0C0C, 0xFFE8E8E8)
    // a faint lightening of whatever the glass shows (+5 over the dark surround, +8 over white)
    val ActionsSeparator: Color get() = pick(0x06FFFFFF, 0xFFE2E2E2)
    /** An Actions row's text: dimmer than the panel's title (#a8a8a8 against #c7c7c7). */
    val ActionsRow: Color get() = pick(0xFFA8A8A8, 0xFF393939)
    val ActionsDisabled: Color get() = pick(0xFF5E5E5E, 0xFFABABAB)
    val TabIdle: Color get() = pick(0xFFA3A3A3, 0xFF404040)
    val SwitchOff: Color get() = pick(0xFF747377, 0xFFA9A8AC)
    val MessageBar: Color get() = pick(0xFF606060, 0xFFD0D0D0)
    val MessageText: Color get() = pick(0xFFB5B5B5, 0xFF3B3B3B)
    val LoupeEdge = Color(0xFF808080)
    val SlideLabel: Color get() = pick(0xFFC1C1C1, 0xFF3D3D3D)
    val SlideValue: Color get() = pick(0xFFEDEDED, 0xFF3D3D3D)
    val SlideTrack: Color get() = pick(0xFF414141, 0xFFBEBEBE)
    val SwitchFixed: Color get() = pick(0xFF404040, 0xFFD2D2D2)
    val SwitchFixedKnob: Color get() = pick(0xFF525252, 0xFFE9E9E9)

    /** The centre of tool button [index] (0 is the colour button, counting leftwards), from the bar's right edge. */
    fun toolCentreFromRight(index: Int, barWidth: Dp): Dp = BarMargin + BarButtonWidth / 2 + (BarButtonWidth + barGaps(barWidth).first) * index

    /** The centre of action button [index] (0 is actions, counting rightwards), from the bar's left edge. */
    fun actionCentre(index: Int, barWidth: Dp): Dp {
        val gap = barGaps(barWidth).second
        return BarMargin + GalleryWidth + gap + BarButtonWidth / 2 + (BarButtonWidth + gap) * index
    }

    // the gallery; light, near white with dark grey inks and a frosted white capsule
    val GalleryBackground: Color get() = pick(0xFF212121, 0xFFF0F0F0)
    val GalleryTitle: Color get() = pick(0xFFF0F0F0, 0xFF3C3C3C)
    val GalleryText: Color get() = pick(0xFFAAAAAA, 0xFF595959)
    val GallerySize: Color get() = pick(0xFF666666, 0xFF6A6A6A)
    val Capsule: Color get() = pick(0xFF1C1C1C, 0xF0F7F7F7)
    val CapsuleEdge: Color get() = pick(0xFF393939, 0x0F000000)
    val LightPanel = Color(0xFFD5D5D5)
    val LightText = Color(0xFF393939)
    val LightRowStrong = Color(0xFFE1E1E1)
    val LightRow = Color(0xFFDEDEDE)
    val LightSeparator = Color(0xFFD2D2D2)
    val LightChip = Color(0xFFD6D6D6)

    // the side bar
    // the bar and the tracks translucent, each straight over what is behind (#2d2d2d and #676767
    // over white); the thumbs opaque (#a6a6a6 over white and over the dark surround alike)
    val SideBar: Color get() = pick(0xE7171717, 0xCFE5E5E5)
    val SliderTrack: Color get() = pick(0xC93E3E3E, 0xFFE9E9E9)
    // a lighter rim round each track, more of what is behind showing through it (#5a5a5a over the
    // dark surround, #969696 over white)
    val SliderRim: Color get() = pick(0xB96E6E6E, 0xFFE9E9E9)
    val SliderRimWidth = 0.8.dp
    val SliderThumb: Color get() = pick(0xFFA6A6A6, 0xFFF8F8F8)
    val SideGlyph: Color get() = pick(0xFF969696, 0xFF646464)
    val SideGlyphDisabled: Color get() = pick(0x1AFFFFFF, 0x33000000)
    val SideBarWidth = 51.dp
    val SideBarHeight = 519.dp
    val SideBarCorner = 20.dp
    /** Where the side bar's centre sits, as a fraction of the screen height. */
    const val SIDE_BAR_CENTRE = 0.5234f
    val SliderWidth = 40.dp
    val SliderHeight = 170.dp
    val SliderCorner = 13.dp
    val ThumbWidth = 32.dp
    val ThumbHeight = 18.dp
    val ThumbInset = 4.dp
    val ModifySize = 22.5.dp
    val ModifyStroke = 2.4.dp
    val ModifyCorner = 6.dp

    /**
     * The gaps between the tool buttons and between the action buttons for a bar [width] wide:
     * the room left after both margins picks one of five steps.
     */
    fun barGaps(width: Dp): Pair<Dp, Dp> {
        val room = width - BarMargin * 2
        return when {
            room < 435.dp -> 2.dp to 4.dp
            room < 530.dp -> 8.dp to 14.dp
            room < 625.dp -> 26.dp to 14.dp
            room < 730.dp -> 8.dp to 14.dp
            else -> 26.dp to 14.dp
        }
    }
}

/**
 * The open canvas's colour space: its colours (0xRRGGBB values) are Display P3 numbers or sRGB
 * ones. The window is in wide colour, so P3 colours show as they are.
 */
object CanvasSpace {
    var p3 by mutableStateOf(false)
        private set

    /**
     * How the canvas shows a colour of its own, when not as its numbers say: as a CMYK canvas's
     * press prints it, or an imported profile's colour in Display P3.
     */
    private var shown by mutableStateOf<((Int) -> Int)?>(null)
    private val seen = HashMap<Int, Int>()
    private val displayP3 by lazy { android.graphics.ColorSpace.get(android.graphics.ColorSpace.Named.DISPLAY_P3) }

    /** The canvas's colours are Display P3 numbers (else sRGB), shown through [shown] if given. */
    fun set(p3: Boolean, shown: ((Int) -> Int)? = null) {
        seen.clear()
        this.p3 = p3
        this.shown = shown
    }

    /** A canvas colour, in the canvas's space. */
    fun color(rgb: Int): Color {
        val f = shown
        val c = if (f == null) rgb else seen.getOrPut(rgb) { f(rgb) }
        return if (p3) {
            Color(((c shr 16) and 0xFF) / 255f, ((c shr 8) and 0xFF) / 255f, (c and 0xFF) / 255f, 1f, androidx.compose.ui.graphics.colorspace.ColorSpaces.DisplayP3)
        } else {
            Color(0xFF000000 or c.toLong())
        }
    }

    /** [bitmap], of the canvas's colours, marked with the canvas's space. */
    fun tag(bitmap: android.graphics.Bitmap): android.graphics.Bitmap {
        if (p3) bitmap.setColorSpace(displayP3)
        return bitmap
    }
}
