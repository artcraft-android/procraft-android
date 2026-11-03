import Observation
import SwiftUI

/// Whether the light interface (Prefs) is on: panels and bars go light grey with dark text.
@Observable
final class Look {
    static let shared = Look()
    var light = false
}

extension Color {
    /// A colour from 0xAARRGGBB, in sRGB.
    init(argb: UInt32) {
        self.init(
            .sRGB,
            red: Double((argb >> 16) & 0xFF) / 255,
            green: Double((argb >> 8) & 0xFF) / 255,
            blue: Double(argb & 0xFF) / 255,
            opacity: Double((argb >> 24) & 0xFF) / 255
        )
    }
}

/// Interface colours and sizes, in points as measured.
enum Pc {
    /// A colour for the interface as it is now: `dark`, or `light` when the light interface is on.
    private static func pick(_ dark: UInt32, _ light: UInt32) -> Color {
        Color(argb: Look.shared.light ? light : dark)
    }

    static var panel: Color { pick(0xF726_2626, 0xF7E6_E6E6) }
    static var row: Color { pick(0xFF30_3030, 0xFFD8_D8D8) }
    static let rowSelected = Color(argb: 0xFF00_76FD)
    static let accent = Color(argb: 0xFF00_76FD)
    static var text: Color { pick(0xFFFF_FFFF, 0xFF22_2222) }
    static var textDim: Color { pick(0x99FF_FFFF, 0x9900_0000) }
    static var disabled: Color { pick(0x4DFF_FFFF, 0x4D00_0000) }
    static var divider: Color { pick(0x26FF_FFFF, 0x1F00_0000) }
    static let surround = Color(argb: 0xFF1C_1C1C)

    // the bar across the top of the canvas: translucent, #333333 over white as the bar measures
    static var bar: Color { pick(0xE61C_1C1C, 0xCFE6_E6E6) }
    static var barText: Color { pick(0xFFAA_AAAA, 0xFF3B_3B3B) }
    static var barGlyph: Color { pick(0xFFC7_C7C7, 0xFF37_3737) }
    static let barGlyphSelected = Color(argb: 0xFF00_76FD)
    static var actionDisc: Color { pick(0xFF14_1414, 0xFFED_EDED) }
    static var actionGlyph: Color { pick(0xFF96_9696, 0xFF64_6464) }
    static var colorRing: Color { pick(0xFF96_9696, 0xFF69_6969) }
    static let barHeight: CGFloat = 44
    static let barMargin: CGFloat = 40
    static let barButtonWidth: CGFloat = 44
    static let barButtonHeight: CGFloat = 38
    static let galleryWidth: CGFloat = 58
    static let actionDiscSize: CGFloat = 32
    static let colorDot: CGFloat = 31

    // the panels hanging from the top bar
    static var popoverFill: Color { pick(0xFF38_3838, 0xFFE6_E6E6) }
    /// A panel's own material over the canvas: what is behind it shows through, blurred.
    static var popoverFrost: Color { pick(0xDE1E_1E1E, 0xFFE6_E6E6) }
    /// The gallery's popovers: their glass as it shows over the gallery's dark.
    static var galleryPopover: Color { pick(0xFF21_2121, 0xFFE6_E6E6) }
    static var popoverShadow: Color { pick(0x3800_0000, 0x2400_0000) }
    static let popoverShadowRadius: CGFloat = 51
    static let popoverCorner: CGFloat = 35
    static let popoverGap: CGFloat = 12
    static let arrowHeight: CGFloat = 12
    static let arrowShift: CGFloat = 2
    static var panelTitle: Color { pick(0xFFC7_C7C7, 0xFF39_3939) }
    static var panelGlyph: Color { pick(0xFFC8_C8C8, 0xFF39_3939) }
    static var chevronDisc: Color { pick(0xFF4C_4C4C, 0xFFCF_CFCF) }
    static var chevronGlyph: Color { pick(0xFF2E_2E2E, 0xFFE6_E6E6) }
    // a little white over the panel, so they follow its frosting
    static var layerRow: Color { pick(0x0DFF_FFFF, 0xFFD8_D8D8) }
    static var backgroundRow: Color { pick(0x0DFF_FFFF, 0xFFD8_D8D8) }
    static var checkFill: Color { pick(0xFFC7_C7C7, 0xFF57_5757) }
    static var thumbBackground: Color { pick(0xFF4F_4F4F, 0xFFC8_C8C8) }
    /// A text layer's glyph on its thumbnail.
    static var textLayerGlyph: Color { pick(0xFFB8_B8B8, 0xFF00_0000) }
    // the brush library is glass like the other panels: the sets on the panel's own frost, the
    // brushes a shade darker, the set cells a little lighter
    static var setColumn: Color { pick(0x0000_0000, 0xFFE6_E6E6) }
    static var setCell: Color { pick(0x2E5B_5B5B, 0xFFD7_D7D7) }
    static var brushColumn: Color { pick(0x2600_0000, 0xFFE0_E0E0) }
    static var grabber: Color { pick(0x7AC6_C6C6, 0xFFB0_B0B0) }
    static var swatchEdge: Color { pick(0xFF5A_5A5A, 0xFFC0_C0C0) }
    static let saturationGrey = Color(argb: 0xFF7F_7F7F)
    static var clearText: Color { pick(0xFFB5_B5B5, 0xFF4A_4A4A) }
    static var emptySwatch: Color { pick(0xFF2A_2A2A, 0xFFD9_D9D9) }
    static var valueGroup: Color { pick(0xFF43_4343, 0xFFD5_D5D5) }
    static var hexRow: Color { pick(0xFF41_4141, 0xFFD8_D8D8) }
    static var hexField: Color { pick(0xFF4B_4B4B, 0xFFCF_CFCF) }
    static var menuFill: Color { pick(0xFF5A_5C5B, 0xFFF0_F0F0) }
    /// A menu's words: white, black on the light interface's menus.
    static var menuInk: Color { pick(0xFFFF_FFFF, 0xFF00_0000) }
    /// The small frosted menus: over the canvas what is behind shows through, blurred.
    static var glassMenu: Color { pick(0xCF31_3135, 0xFAF0_F0F0) }
    /// A small menu over a panel: the panel can't show through blurred, so it is nearly solid, grey.
    static var glassMenuOverPanel: Color { pick(0xFA22_2222, 0xFAF0_F0F0) }
    static var glassMenuEdge: Color { pick(0x24FF_FFFF, 0x1400_0000) }
    static var radioRing: Color { pick(0xFF60_6060, 0xFFB0_B0B0) }
    static var menuDanger: Color { pick(0xFFE8_4F51, 0xFFD9_3A3D) }
    static var menuLine: Color { pick(0x33FF_FFFF, 0x2600_0000) }
    static var menuDisabled: Color { pick(0x66FF_FFFF, 0x6600_0000) }
    static var sliderRest: Color { pick(0xFF5A_5A5A, 0xFFBE_BEBE) }
    static var emptySlot: Color { pick(0xFF55_5555, 0xFFC8_C8C8) }
    // a dark wash on the panel's glass
    static var actionsGroup: Color { pick(0x420C_0C0C, 0xFFE8_E8E8) }
    // a faint lightening of whatever the glass shows
    static var actionsSeparator: Color { pick(0x06FF_FFFF, 0xFFE2_E2E2) }
    /// An Actions row's text: dimmer than the panel's title.
    static var actionsRow: Color { pick(0xFFA8_A8A8, 0xFF39_3939) }
    static var actionsDisabled: Color { pick(0xFF5E_5E5E, 0xFFAB_ABAB) }
    static var tabIdle: Color { pick(0xFFA3_A3A3, 0xFF40_4040) }
    static var switchOff: Color { pick(0xFF74_7377, 0xFFA9_A8AC) }
    static var messageBar: Color { pick(0xFF60_6060, 0xFFD0_D0D0) }
    static var messageText: Color { pick(0xFFB5_B5B5, 0xFF3B_3B3B) }
    static let loupeEdge = Color(argb: 0xFF80_8080)
    static var slideLabel: Color { pick(0xFFC1_C1C1, 0xFF3D_3D3D) }
    static var slideValue: Color { pick(0xFFED_EDED, 0xFF3D_3D3D) }
    static var slideTrack: Color { pick(0xFF41_4141, 0xFFBE_BEBE) }
    static var switchFixed: Color { pick(0xFF40_4040, 0xFFD2_D2D2) }
    static var switchFixedKnob: Color { pick(0xFF52_5252, 0xFFE9_E9E9) }

    /// The centre of tool button `index` (0 is the colour button, counting leftwards), from the
    /// bar's right edge.
    static func toolCentreFromRight(_ index: Int, barWidth: CGFloat) -> CGFloat {
        barMargin + barButtonWidth / 2 + (barButtonWidth + barGaps(barWidth).tool) * CGFloat(index)
    }

    /// The centre of action button `index` (0 is actions, counting rightwards), from the bar's left
    /// edge.
    static func actionCentre(_ index: Int, barWidth: CGFloat) -> CGFloat {
        let gap = barGaps(barWidth).action
        return barMargin + galleryWidth + gap + barButtonWidth / 2 + (barButtonWidth + gap) * CGFloat(index)
    }

    // the gallery
    static var galleryBackground: Color { pick(0xFF21_2121, 0xFFF0_F0F0) }
    static var galleryTitle: Color { pick(0xFFF0_F0F0, 0xFF3C_3C3C) }
    static var galleryText: Color { pick(0xFFAA_AAAA, 0xFF59_5959) }
    static var gallerySize: Color { pick(0xFF66_6666, 0xFF6A_6A6A) }
    static var capsule: Color { pick(0xFF1C_1C1C, 0xF0F7_F7F7) }
    static var capsuleEdge: Color { pick(0xFF39_3939, 0x0F00_0000) }
    static let lightPanel = Color(argb: 0xFFD5_D5D5)
    static let lightText = Color(argb: 0xFF39_3939)
    static let lightRowStrong = Color(argb: 0xFFE1_E1E1)
    static let lightRow = Color(argb: 0xFFDE_DEDE)
    static let lightSeparator = Color(argb: 0xFFD2_D2D2)
    static let lightChip = Color(argb: 0xFFD6_D6D6)

    // the side bar: the bar and the tracks translucent, each straight over what is behind; the
    // thumbs opaque
    static var sideBar: Color { pick(0xE717_1717, 0xCFE5_E5E5) }
    static var sliderTrack: Color { pick(0xC93E_3E3E, 0xFFE9_E9E9) }
    // a lighter rim round each track, more of what is behind showing through it
    static var sliderRim: Color { pick(0xB96E_6E6E, 0xFFE9_E9E9) }
    static let sliderRimWidth: CGFloat = 0.8
    static var sliderThumb: Color { pick(0xFFA6_A6A6, 0xFFF8_F8F8) }
    static var sideGlyph: Color { pick(0xFF96_9696, 0xFF64_6464) }
    static var sideGlyphDisabled: Color { pick(0x1AFF_FFFF, 0x3300_0000) }
    static let sideBarWidth: CGFloat = 51
    static let sideBarHeight: CGFloat = 519
    static let sideBarCorner: CGFloat = 20
    /// Where the side bar's centre sits, as a fraction of the screen height.
    static let sideBarCentre: CGFloat = 0.5234
    static let sliderWidth: CGFloat = 40
    static let sliderHeight: CGFloat = 170
    static let sliderCorner: CGFloat = 13
    static let thumbWidth: CGFloat = 32
    static let thumbHeight: CGFloat = 18
    static let thumbInset: CGFloat = 4
    static let modifySize: CGFloat = 22.5
    static let modifyStroke: CGFloat = 2.4
    static let modifyCorner: CGFloat = 6

    /// The gaps between the tool buttons and between the action buttons for a bar `width` wide:
    /// the room left after both margins picks one of five steps.
    static func barGaps(_ width: CGFloat) -> (tool: CGFloat, action: CGFloat) {
        let room = width - barMargin * 2
        switch room {
        case ..<435: return (2, 4)
        case ..<530: return (8, 14)
        case ..<625: return (26, 14)
        case ..<730: return (8, 14)
        default: return (26, 14)
        }
    }
}

/// The open canvas's colour space: its colours (0xRRGGBB values) are Display P3 numbers or sRGB
/// ones.
@Observable
final class CanvasSpace {
    static let shared = CanvasSpace()
    private(set) var p3 = false

    func set(p3: Bool) {
        self.p3 = p3
    }

    /// A canvas colour, in the canvas's space.
    func color(_ rgb: UInt32) -> Color {
        let r = Double((rgb >> 16) & 0xFF) / 255
        let g = Double((rgb >> 8) & 0xFF) / 255
        let b = Double(rgb & 0xFF) / 255
        return Color(p3 ? .displayP3 : .sRGB, red: r, green: g, blue: b, opacity: 1)
    }
}
