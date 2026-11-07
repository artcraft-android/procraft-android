import SwiftUI

private func themed(_ dark: UInt32, _ light: UInt32) -> Color {
    Color(argb: Look.shared.light ? light : dark)
}

private var screenFill: Color { themed(0xFF1F_1F1F, 0xFFF3_F3F3) }
private var railFill: Color { themed(0xFF27_2727, 0xFFEB_EBEB) }
private var cardFill: Color { themed(0xFF25_2525, 0xFFFF_FFFF) }
private var ink: Color { themed(0xFFC1_C1C1, 0xFF41_4141) }
private var detail: Color { themed(0xFF9A_9A9A, 0xFF6A_6A6A) }
private var railIdle: Color { themed(0xFFAA_AAAA, 0xFF3F_3F3F) }
private var track: Color { themed(0xFF41_4141, 0xFFE5_E5E5) }
/// A delay whose way of touching is off: a dark line and a dim knob, no blue.
private var trackOff: Color { themed(0xFF1D_1D1D, 0xFFE9_E9EB) }
private var knobOff: Color { themed(0xFF33_3333, 0xFFD0_D0D0) }
private var cardEdge: Color { themed(0xFF37_3737, 0xFFDA_DADA) }
private var pageInk: Color { themed(0xFF79_7979, 0xFF3E_3E3E) }
private let doneBlue = Color(argb: 0xFF01_90FF)
/// Where a wide screen's page starts and ends, beside the card of what gestures can do.
private let wideContentStart: CGFloat = 358
private let wideContentEnd: CGFloat = 28
/// How long a hold can wait, seconds: the core keeps it to this.
private let leastDelay: Float = 0.1
private let mostDelay: Float = 1.5

private func icon(_ p: GesturePurpose) -> PcIcon {
    switch p {
    case .smudge: PcIcons.smudge
    case .erase: PcIcons.erase
    case .assisted: PcIcons.gestureAssisted
    case .eyedropper: PcIcons.gestureEyedropper
    case .quickshape: PcIcons.gestureQuickShape
    case .quickmenu: PcIcons.gestureQuickMenu
    case .fullScreen: PcIcons.gestureFullScreen
    case .clearLayer: PcIcons.gestureClearLayer
    case .copyPaste: PcIcons.gestureCopyPaste
    case .layerSelect: PcIcons.gestureLayerSelect
    case .hover: PcIcons.gestureHover
    case .general: PcIcons.gestureGeneral
    }
}

/// Gesture controls, full screen: a rail of what gestures can do and, for the one chosen, a card of
/// switches for the ways of touching that do it (a delay under the held ones), and Done. The pages,
/// the switches and the rule that a way of touching does one thing are the core's.
struct GestureControls: View {
    let model: CanvasModel

    private var pages: [GesturePurpose] {
        gesturePurposes()
    }

    private var rail: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 3) {
                ForEach(pages, id: \.self) { p in
                    let chosen = p == model.gesturePage
                    ZStack {
                        Capsule().fill(chosen ? Pc.accent : Color.clear)
                        // each drawn in a padded frame, fitted 37 across
                        PcIconView(icon(p), size: 37, color: chosen ? .white : railIdle)
                    }
                    .frame(width: 65, height: 53)
                    .contentShape(Rectangle())
                    .onTapGesture { model.gesturePage = p }
                }
            }
            .padding(.leading, 8)
            .padding(.top, 46)
            .padding(.bottom, 20)
        }
        .scrollIndicators(.hidden)
        .frame(width: 81, alignment: .leading)
        .frame(maxHeight: .infinity)
        .background(railFill)
    }

    /// What gestures can do, by icon and name on a card headed "Gesture controls".
    private var card: some View {
        let shape = RoundedRectangle(cornerRadius: 20, style: .circular)
        return VStack(alignment: .leading, spacing: 0) {
            Text(verbatim: "Gesture controls")
                .font(.system(size: 24, weight: .bold))
                .foregroundStyle(ink)
                .lineLimit(1)
                .padding(.leading, 24)
                .padding(.top, 19)
            ScrollView {
                VStack(spacing: 3) {
                    ForEach(pages, id: \.self) { p in
                        let chosen = p == model.gesturePage
                        HStack(spacing: 0) {
                            PcIconView(icon(p), size: 37, color: chosen ? .white : railIdle)
                                .frame(width: 54)
                            Text(verbatim: gesturePurposeTitle(purpose: p))
                                .font(.system(size: 17))
                                .foregroundStyle(chosen ? Color.white : pageInk)
                                .lineLimit(1)
                            Spacer(minLength: 0)
                        }
                        .frame(height: 53)
                        .background(Capsule().fill(chosen ? Pc.accent : Color.clear))
                        .contentShape(Rectangle())
                        .onTapGesture { model.gesturePage = p }
                    }
                }
                .padding(.horizontal, 8)
                .padding(.top, 17)
                .padding(.bottom, 8)
            }
            .scrollIndicators(.hidden)
        }
        .frame(width: 320)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(shape.fill(railFill))
        .overlay(shape.strokeBorder(cardEdge, lineWidth: 1))
        .clipShape(shape)
        .padding(.leading, 10)
        .padding(.vertical, 10)
    }

    private var page: some View {
        let settings = Gestures.shared
        let purpose = model.gesturePage
        // painting models isn't here yet: its switch would do nothing
        let options = gestureOptions(purpose: purpose).filter { $0.key != "MODEL_FINGER" }
        return ScrollView {
            VStack(spacing: 0) {
                VStack(spacing: 0) {
                    ForEach(Array(options.enumerated()), id: \.offset) { i, o in
                        if i > 0 {
                            screenFill.frame(height: 1)
                        }
                        // 72 apart with the line; a held one closer up, its delay under it
                        OptionRow(option: o, on: settings.isOn(purpose, o.key), height: o.delayed ? 60 : 71) {
                            settings.set(purpose, o.key, $0)
                        }
                        if o.delayed {
                            DelayRow(seconds: settings.delay(purpose), enabled: settings.isOn(purpose, o.key)) {
                                settings.setDelay(purpose, seconds: $0)
                            }
                        }
                    }
                }
                .background(cardFill)
                .clipShape(RoundedRectangle(cornerRadius: 25, style: .circular))
                // under General's settings, everything back as it ships
                if purpose == .general {
                    Text(verbatim: "Reset to defaults")
                        .font(.system(size: 15))
                        .foregroundStyle(ink)
                        .lineLimit(1)
                        .frame(maxWidth: .infinity, minHeight: 50, maxHeight: 50)
                        .background(RoundedRectangle(cornerRadius: 25, style: .circular).fill(cardFill))
                        .contentShape(Rectangle())
                        .onTapGesture {
                            model.question = Question(
                                title: "Reset gesture controls?",
                                message: "Every gesture goes back to how the app starts out.",
                                action: "Reset",
                                icon: nil
                            ) { settings.resetAll() }
                        }
                        .padding(.top, 14)
                }
                Color.clear.frame(height: 40)
            }
        }
        .scrollIndicators(.hidden)
        // each page opens at its top
        .id(purpose)
    }

    var body: some View {
        GeometryReader { screen in
            // on a wide screen what gestures can do is named on a card, the page and its title to
            // its right
            let wide = screen.size.width > screen.size.height
            let heading = gesturePurposeHeading(purpose: model.gesturePage)
            ZStack(alignment: .topLeading) {
                screenFill
                if wide {
                    card
                } else {
                    rail
                }
                Text(verbatim: heading)
                    .font(.system(size: 20, weight: .bold))
                    .foregroundStyle(ink)
                    .lineLimit(1)
                    .frame(maxWidth: wide ? .infinity : nil, alignment: .center)
                    .padding(.leading, wide ? wideContentStart : 101)
                    .padding(.trailing, wide ? wideContentEnd : 0)
                    .offset(y: 28)
                PcIconView(PcIcons.doneTick, size: 40, color: .white)
                    .frame(width: 44, height: 45)
                    .background(Capsule().fill(doneBlue))
                    .contentShape(Capsule())
                    .onTapGesture { withAnimation(Motion.studioOut) { model.gestureControlsOpen = false } }
                    .padding(.top, 20)
                    .padding(.trailing, 20)
                    .frame(maxWidth: .infinity, alignment: .trailing)
                page
                    .padding(.leading, wide ? wideContentStart : 105)
                    .padding(.trailing, wide ? wideContentEnd : 24)
                    .padding(.top, 100)
            }
        }
    }
}

/// A way of touching: its name and what it does, and its switch.
private struct OptionRow: View {
    let option: GestureOption
    let on: Bool
    let height: CGFloat
    let onChange: (Bool) -> Void

    var body: some View {
        HStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 0) {
                Text(verbatim: option.label)
                    .font(.system(size: 18, weight: .medium))
                    .foregroundStyle(ink)
                    .lineLimit(1)
                Text(verbatim: option.detail)
                    .font(.system(size: 13))
                    .foregroundStyle(detail)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
            PcSwitch(on: on)
        }
        .padding(.leading, 28)
        .padding(.trailing, 16)
        .frame(height: height)
        .contentShape(Rectangle())
        .onTapGesture { onChange(!on) }
    }
}

/// How long a hold waits, 0.1 to 1.5 seconds, dragged or tapped along the bar; greyed while its way
/// of touching is off.
private struct DelayRow: View {
    let seconds: Float
    let enabled: Bool
    let onChange: (Float) -> Void

    var body: some View {
        let span = mostDelay - leastDelay
        let fraction = CGFloat(min(max((seconds - leastDelay) / span, 0), 1))
        HStack(spacing: 0) {
            Text(verbatim: "Delay")
                .font(.system(size: 15))
                .foregroundStyle(ink)
                .frame(width: 74, alignment: .leading)
            GeometryReader { g in
                let w = g.size.width
                let x = fraction * w
                let thin: CGFloat = enabled ? 3 : 1.5
                ZStack(alignment: .leading) {
                    (enabled ? track : trackOff).frame(height: thin)
                    if enabled {
                        Pc.accent.frame(width: x, height: thin)
                    }
                    Capsule().fill(enabled ? Color.white : knobOff)
                        .frame(width: 16, height: 10)
                        .offset(x: x - 8)
                }
                .frame(width: w, height: g.size.height)
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
                    onChange(leastDelay + Float(min(max(drag.location.x / max(w, 1), 0), 1)) * span)
                })
                .allowsHitTesting(enabled)
            }
            Text(verbatim: String(format: "%.2fs", seconds))
                .font(.system(size: 15))
                .foregroundStyle(ink)
                .padding(.leading, 16)
        }
        .padding(.leading, 28)
        .padding(.trailing, 24)
        .frame(height: 50)
    }
}
