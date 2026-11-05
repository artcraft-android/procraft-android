import SwiftUI

private func themed(_ dark: UInt32, _ light: UInt32) -> Color {
    Color(argb: Look.shared.light ? light : dark)
}

private let toolbarWidth: CGFloat = 728
private let toolbarHeight: CGFloat = 126
private let toolbarBottom: CGFloat = 25
private let toolbarCorner: CGFloat = 33
private let segmentGap: CGFloat = 11.33
private let buttonHeight: CGFloat = 60
/// The space between a roomy toolbar's buttons.
private let buttonGap: CGFloat = 19
/// Where a capital's middle sits below the top of its line, as a fraction of the font size.
private let capMiddle: CGFloat = 0.6056

/// The toolbars' and their popovers' colours: translucent, #282828 to #252525 over white, #212121 to
/// #1e1e1e over the surround; light, #f2f2f2 with a white track, dark labels, and grey ones for what
/// can't be used yet.
enum ToolbarInk {
    static var top: Color { themed(0xF721_2121, 0xF7F2_F2F2) }
    static var fill: Color { themed(0xF71E_1E1E, 0xF7F2_F2F2) }
    static var segmentTrack: Color { themed(0xFF53_5353, 0xFFFF_FFFF) }
    static var segmentEdge: Color { themed(0xFF7E_7E7E, 0xFFEB_EBEB) }
    static var segmentText: Color { themed(0xFFC1_C1C1, 0xFF3B_3B3B) }
    static var buttonText: Color { themed(0xFFA3_A3A3, 0xFF41_4141) }
    static var buttonDimmed: Color { themed(0xFF57_5757, 0xFF98_9898) }
    static var menuFill: Color { themed(0xEB2A_2A2A, 0xEBF0_F0F0) }
    static var menuGroup: Color { themed(0xFF31_3131, 0xFFFF_FFFF) }
    static var menuLine: Color { themed(0xFF37_3737, 0xFFE2_E2E2) }
    static var menuText: Color { themed(0xFFC5_C5C5, 0xFF39_3939) }
}

/// A popover's width, and its groups' corners.
let toolbarMenuWidth: CGFloat = 260
let toolbarMenuCorner: CGFloat = 13

/// A floating toolbar's width, how many buttons it has and how wide they are.
struct ToolbarLayout {
    let width: CGFloat
    let count: Int
    let button: CGFloat

    /// The space between its buttons.
    var gap: CGFloat {
        (width - 32 - button * CGFloat(count)) / CGFloat(max(count - 1, 1))
    }

    /// A floating toolbar of `count` buttons on a screen `screenWidth` wide: with room, `roomy`-wide
    /// buttons 19 apart inside its padding (as measured in landscape); else across the screen (8
    /// clear of each side, at most 728) with `narrow`-wide buttons spread over it.
    init(screenWidth: CGFloat, count: Int, roomy: CGFloat, narrow: CGFloat) {
        self.count = count
        let wide = roomy * CGFloat(count) + buttonGap * CGFloat(count - 1) + 32
        if wide <= screenWidth - 16 {
            width = wide
            button = roomy
        } else {
            width = min(toolbarWidth, screenWidth - 16)
            button = narrow
        }
    }

    /// Where button `index` is centred, from the left of a screen `screenWidth` wide.
    func buttonCentre(_ index: Int, screenWidth: CGFloat) -> CGFloat {
        (screenWidth - width) / 2 + 16 + (button + gap) * CGFloat(index) + button / 2
    }

    /// The top of the buttons, from the top of a screen `screenHeight` tall: where popovers point.
    static func buttonsTop(screenHeight: CGFloat) -> CGFloat {
        screenHeight - toolbarBottom - toolbarHeight + 16 + 34
    }
}

/// The floating toolbar at the foot of the screen (transform, selection): a row of `segments` in a
/// pill over a row of `buttons` (given their width) spread across it. A tap on its background calls
/// `onBackground`.
struct FloatingToolbar<Segments: View, Buttons: View>: View {
    let layout: ToolbarLayout
    let onBackground: () -> Void
    @ViewBuilder let segments: () -> Segments
    @ViewBuilder let buttons: (CGFloat) -> Buttons

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: toolbarCorner, style: .circular)
        VStack(spacing: 0) {
            HStack(spacing: segmentGap) {
                segments()
            }
            .padding(2)
            .frame(height: 34)
            .background(Capsule().fill(ToolbarInk.segmentTrack))
            .overlay(Capsule().strokeBorder(ToolbarInk.segmentEdge, lineWidth: 0.5))
            HStack(spacing: layout.gap) {
                buttons(layout.button)
            }
            .frame(maxWidth: .infinity)
        }
        .padding(16)
        .frame(width: layout.width, height: toolbarHeight)
        .background(
            shape.fill(
                LinearGradient(
                    stops: [
                        .init(color: ToolbarInk.top, location: 0),
                        .init(color: ToolbarInk.fill, location: 0.12),
                    ],
                    startPoint: .top, endPoint: .bottom
                )
            )
        )
        .clipShape(shape)
        .contentShape(shape)
        .onTapGesture(perform: onBackground)
        .offset(y: -toolbarBottom)
    }
}

/// One of the four ways: its icon and name, filled blue when chosen.
struct ToolbarSegment: View {
    let icon: PcIcon
    let label: String
    let selected: Bool
    var iconSize: CGFloat = 26.5
    let action: () -> Void

    var body: some View {
        let tint = selected ? Color.white : ToolbarInk.segmentText
        HStack(spacing: 2) {
            PcIconView(icon, size: iconSize, color: tint)
                .frame(width: 30, height: 30)
            Text(verbatim: label)
                .font(.system(size: 13))
                .tracking(-0.025 * 13)
                .foregroundStyle(tint)
                .lineLimit(1)
                .fixedSize()
        }
        .padding(.leading, 14)
        .padding(.trailing, 20)
        .frame(height: 30)
        .background(Capsule().fill(selected ? Pc.accent : Color.clear))
        .contentShape(Capsule())
        .onTapGesture(perform: action)
    }
}

/// A toolbar button: an icon over its name; blue while its popover is open.
struct ToolbarButton: View {
    let icon: PcIcon
    let label: String
    var active = false
    var enabled = true
    var width: CGFloat = 83
    var iconSize: CGFloat = 24
    var labelSize: CGFloat = 12.5
    let action: () -> Void

    var body: some View {
        let tint = !enabled ? ToolbarInk.buttonDimmed : active ? Pc.accent : ToolbarInk.buttonText
        // icons centred 30 below the top, labels' capitals 50 below it; a long name may run past
        ZStack(alignment: .top) {
            PcIconView(icon, size: iconSize, color: tint)
                .offset(y: 30 - iconSize / 2)
            Text(verbatim: label)
                .font(.system(size: labelSize))
                .foregroundStyle(enabled ? ToolbarInk.buttonText : ToolbarInk.buttonDimmed)
                .lineLimit(1)
                .fixedSize()
                .offset(y: 50 - labelSize * capMiddle)
        }
        .frame(width: width, height: buttonHeight, alignment: .top)
        .contentShape(Rectangle())
        .onTapGesture {
            if enabled {
                action()
            }
        }
    }
}

/// A popover `left` from the screen's left whose arrow points down at its button, the tip at
/// `bottom` from the top.
struct ToolbarPopover<Content: View>: View {
    let arrowX: CGFloat
    let left: CGFloat
    let bottom: CGFloat
    var width: CGFloat = toolbarMenuWidth
    let screenWidth: CGFloat
    @ViewBuilder let content: () -> Content

    var body: some View {
        Popover(width: width, arrowX: arrowX, fill: ToolbarInk.menuFill, corner: 26, side: .bottom) {
            VStack(alignment: .leading, spacing: 0) {
                content()
            }
            .padding(10)
        }
        .contentShape(Rectangle())
        .onTapGesture {}
        .frame(width: screenWidth, height: bottom, alignment: .bottomLeading)
        .offset(x: left)
    }
}

struct ToolbarMenuTitle: View {
    let text: String

    var body: some View {
        Text(verbatim: text)
            .font(.system(size: 24, weight: .bold))
            .tracking(-0.015 * 24)
            .foregroundStyle(ToolbarInk.menuText)
            .lineLimit(1)
            .padding(.leading, 12)
            .frame(maxWidth: .infinity, minHeight: 40, maxHeight: 40, alignment: .leading)
    }
}

/// A row's text in the toolbar's popovers.
struct ToolbarMenuLabel: View {
    let text: String
    var color: Color?

    var body: some View {
        Text(verbatim: text)
            .font(.system(size: 16))
            .foregroundStyle(color ?? ToolbarInk.menuText)
            .lineLimit(1)
    }
}

/// A white group of rows in a toolbar's popover.
struct ToolbarMenuGroup<Content: View>: View {
    @ViewBuilder let content: () -> Content

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: toolbarMenuCorner, style: .circular)
        VStack(spacing: 0) {
            content()
        }
        .frame(maxWidth: .infinity)
        .background(shape.fill(ToolbarInk.menuGroup))
        .clipShape(shape)
    }
}

/// The line between two rows of a group.
struct ToolbarMenuLine: View {
    var body: some View {
        Rectangle()
            .fill(ToolbarInk.menuLine)
            .frame(height: 1)
    }
}

/// A row with a switch.
struct ToolbarSwitchRow: View {
    let label: String
    let on: Bool
    let toggle: () -> Void

    var body: some View {
        HStack(spacing: 0) {
            ToolbarMenuLabel(text: label)
            Spacer(minLength: 0)
            PcSwitch(on: on)
        }
        .padding(.leading, 12)
        .padding(.trailing, 6)
        .frame(height: 49)
        .contentShape(Rectangle())
        .onTapGesture(perform: toggle)
    }
}
