import SwiftUI

private func themed(_ dark: UInt32, _ light: UInt32) -> Color {
    Color(argb: Look.shared.light ? light : dark)
}

/// Translucent over the canvas, as measured.
private var modeBar: Color { themed(0xD63B_3B3B, 0xD6EE_EEEE) }
private var modeText: Color { themed(0xFFC5_C5C5, 0xFF6A_6A6A) }
private var panelFill: Color { themed(0xF71E_1E1E, 0xF7F1_F1F1) }
private var panelButton: Color { themed(0xFF30_3030, 0xFFF8_F8F8) }
private var menuButton: Color { themed(0xFF3B_3B3B, 0xFFFF_FFFF) }
private var menuButtonText: Color { themed(0xFFD8_D8D8, 0xFF41_4141) }
private var menuFill: Color { themed(0xEB2A_2A2A, 0xEBF0_F0F0) }
private var menuGroup: Color { themed(0xFF31_3131, 0xFFFF_FFFF) }
private var menuLine: Color { themed(0xFF37_3737, 0xFFE2_E2E2) }
private var menuText: Color { themed(0xFFC5_C5C5, 0xFF39_3939) }
private var sliderRest: Color { themed(0xFF5A_5A5A, 0xFFDD_DDDD) }
private var liquifyIdle: Color { themed(0xFFAA_AAAA, 0xFF41_4141) }
private var liquifyLabel: Color { themed(0xFFA3_A3A3, 0xFF41_4141) }
private var liquifyDim: Color { themed(0xFF57_5757, 0xFF98_9898) }
/// Where a slider's knob centre runs, from its row's left edge and from its right.
private let sliderStart: CGFloat = 21.5
private let sliderEnd: CGFloat = 15

private func icon(_ a: Adjustment) -> PcIcon {
    switch a {
    case .hsb: PcIcons.adjustHsb
    case .colorBalance: PcIcons.adjustBalance
    case .curves: PcIcons.adjustCurves
    case .gradientMap: PcIcons.adjustGradient
    case .gaussianBlur: PcIcons.adjustBlur
    case .motionBlur: PcIcons.adjustMotion
    case .perspectiveBlur: PcIcons.adjustPerspective
    case .noise: PcIcons.adjustNoise
    case .sharpen: PcIcons.adjustSharpen
    case .bloom: PcIcons.adjustBloom
    case .glitch: PcIcons.adjustGlitch
    case .halftone: PcIcons.adjustHalftone
    case .chromaticAberration: PcIcons.adjustChromatic
    case .liquify: PcIcons.adjustLiquify
    case .clone: PcIcons.adjustClone
    case .plugin: PcIcons.adjustments
    }
}

/// The bar under the top bar while an adjustment is open: its icon and name, its amount when it has
/// one; and under it the amount as a thin blue bar. It takes its touches: a swipe from it adjusts
/// nothing.
struct AdjustmentBar: View {
    let model: CanvasModel

    var body: some View {
        if let a = model.adjustment {
            let amount = model.adjustShown?.amount ?? 0
            let swiped = adjustmentSwiped(adjustment: a)
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 0) {
                    // drawn in a 32 frame, shown as it is
                    PcIconView(icon(a), size: 32, color: modeText)
                        .frame(width: 33, height: 32)
                    Text(verbatim: model.adjustShown?.plugin?.name ?? adjustmentLabel(adjustment: a))
                        .font(.system(size: 17, weight: .bold))
                        .foregroundStyle(modeText)
                        .lineLimit(1)
                    if swiped {
                        Text(verbatim: "\(wholeNumber(amount * 100))%")
                            .font(.system(size: 14))
                            .foregroundStyle(modeText)
                            .lineLimit(1)
                            .padding(.leading, 3)
                    }
                    // Layer or Pencil, for the adjustments that can be painted on
                    if adjustmentPaintable(adjustment: a) {
                        PcIconView(PcIcons.disclose, size: 32, color: modeText)
                            .frame(width: 33, height: 32)
                    }
                }
                .frame(maxWidth: .infinity)
                .frame(height: 32)
                .background(modeBar)
                .contentShape(Rectangle())
                .onTapGesture {
                    if adjustmentPaintable(adjustment: a) {
                        model.adjustModesOpen.toggle()
                    }
                }
                if swiped {
                    GeometryReader { g in
                        Pc.accent.frame(width: g.size.width * CGFloat(min(max(amount, 0), 1)))
                    }
                    .frame(height: 4)
                }
            }
        }
    }
}

/// The menu a tap on the canvas opens during an adjustment: apply above, undo to the left, preview
/// in the middle, cancel to the right and reset below, around the tap, kept on the screen.
struct AdjustmentMenu: View {
    let model: CanvasModel
    let screen: CGSize

    private let step: CGFloat = 88

    private func button(_ glyph: PcIcon, _ label: String, _ place: CGPoint, centre: CGPoint, action: @escaping () -> Void) -> some View {
        ZStack(alignment: .top) {
            RoundedRectangle(cornerRadius: 22, style: .circular).fill(menuButton)
            PcIconView(glyph, size: 30, color: menuButtonText)
                .padding(.top, 18)
            Text(verbatim: label)
                .font(.system(size: 13.5))
                .foregroundStyle(menuButtonText)
                .lineLimit(1)
                .padding(.top, 44)
        }
        .frame(width: 80, height: 80)
        .contentShape(Rectangle())
        .onTapGesture {
            model.adjustMenuAt = nil
            action()
        }
        .offset(x: centre.x - 40 + step * place.x, y: centre.y - 40 + step * place.y)
    }

    var body: some View {
        if let at = model.adjustMenuAt {
            // 12 from the sides, against the top, 25 above the bottom (as measured)
            let reach = step + 40
            let centre = CGPoint(
                x: min(max(at.x, reach + 12), max(reach + 12, screen.width - reach - 12)),
                y: min(max(at.y, reach), max(reach, screen.height - reach - 25))
            )
            ZStack(alignment: .topLeading) {
                button(PcIcons.apply, "Apply", CGPoint(x: 0, y: -1), centre: centre) { model.applyAdjustment() }
                button(PcIcons.pieUndo, "Undo", CGPoint(x: -1, y: 0), centre: centre) { model.undoAdjustment() }
                button(PcIcons.preview, "Preview", .zero, centre: centre) { model.togglePreview() }
                button(PcIcons.cancelAdjust, "Cancel", CGPoint(x: 1, y: 0), centre: centre) { model.endAdjustment(commit: false) }
                button(PcIcons.pieReset, "Reset", CGPoint(x: 0, y: 1), centre: centre) { model.resetAdjustment() }
            }
        }
    }
}

/// A row of a toolbar's panel: its name and value over a thin slider, blue up to a small white
/// knob, grey after it.
struct PanelSlider: View {
    let label: String
    let value: Float
    let shown: String
    let onChange: (Float) -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                Text(verbatim: label)
                Spacer(minLength: 0)
                Text(verbatim: shown)
            }
            .font(.system(size: 16))
            .foregroundStyle(menuText)
            .lineLimit(1)
            .padding(.leading, 14)
            .padding(.trailing, 13)
            .padding(.top, 9)
            GeometryReader { g in
                let w = g.size.width
                let span = max(w - sliderStart - sliderEnd, 1)
                let x = sliderStart + span * CGFloat(min(max(value, 0), 1))
                ZStack(alignment: .leading) {
                    Capsule().fill(sliderRest)
                        .frame(width: max(w - sliderEnd - x, 0), height: 2)
                        .offset(x: x)
                    Capsule().fill(Pc.accent)
                        .frame(width: max(x - sliderStart, 0), height: 2)
                        .offset(x: sliderStart)
                    Capsule().fill(Color(argb: 0x3300_0000))
                        .frame(width: 15, height: 11)
                        .offset(x: x - 7.5, y: 0.5)
                    Capsule().fill(Color.white)
                        .frame(width: 15, height: 11)
                        .offset(x: x - 7.5)
                }
                .frame(width: w, height: 14)
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
                    onChange(Float(min(max((drag.location.x - sliderStart) / span, 0), 1)))
                })
            }
            .frame(height: 14)
            .padding(.top, 9)
        }
        .frame(height: 59, alignment: .top)
    }
}

private let liquifyKinds: [(kind: LiquifyKind, name: String, glyph: PcIcon)] = [
    (.push, "Push", PcIcons.liquifyPush),
    (.twirlRight, "Twirl Right", PcIcons.liquifyTwirlRight),
    (.twirlLeft, "Twirl Left", PcIcons.liquifyTwirlLeft),
    (.pinch, "Pinch", PcIcons.liquifyPinch),
    (.expand, "Expand", PcIcons.liquifyExpand),
    (.crystals, "Crystals", PcIcons.liquifyCrystals),
    (.edge, "Edge", PcIcons.liquifyEdge),
    (.reconstruct, "Reconstruct", PcIcons.liquifyReconstruct),
]

/// The liquify panel: the brush's size, pressure, distortion and momentum; its way, adjust and
/// reset. On a wide screen the ways are a row of buttons; upright, one button lists them.
struct LiquifyPanel: View {
    let model: CanvasModel
    let screen: CGSize

    private var wide: Bool {
        screen.width > screen.height
    }

    private var canAdjust: Bool {
        model.adjustShown?.liquified == true
    }

    private func percent(_ v: Float, none: String? = nil) -> String {
        if v >= 1 {
            return "Max"
        }
        if v <= 0, let none {
            return none
        }
        return "\(wholeNumber(v * 100))%"
    }

    private func slider(_ index: Int, _ label: String, none: String? = nil) -> some View {
        let value = model.liquifyValue(index)
        return PanelSlider(label: label, value: value, shown: percent(value, none: none)) {
            model.setLiquifyValue(index, $0)
        }
    }

    /// One of the wide panel's buttons: its icon (`size` across, centred 9 down) over its name.
    private func wideButton(_ glyph: PcIcon, _ label: String, ink: Color, words: Color, size: CGFloat, enabled: Bool = true, action: @escaping () -> Void) -> some View {
        ZStack(alignment: .top) {
            PcIconView(glyph, size: size, color: ink)
                .frame(width: 24, height: 24)
                .offset(y: -3)
            Text(verbatim: label)
                .font(.system(size: 13))
                .foregroundStyle(words)
                .lineLimit(1)
                .padding(.top, 22)
        }
        .frame(width: 75, height: 60, alignment: .top)
        .contentShape(Rectangle())
        .onTapGesture {
            if enabled {
                action()
            }
        }
    }

    /// Liquify on a wide screen (1093 x 162): its ways, Adjust and Reset as a row of icon buttons (75
    /// wide, 94 apart; the chosen way's icon blue), and the four sliders side by side under them.
    private var widePanel: some View {
        let chosen = model.adjustShown?.liquifyKind
        let ink = canAdjust ? liquifyIdle : liquifyDim
        let words = canAdjust ? liquifyLabel : liquifyDim
        return VStack(spacing: 8) {
            HStack(spacing: 19) {
                ForEach(Array(liquifyKinds.enumerated()), id: \.offset) { _, way in
                    wideButton(way.glyph, way.name, ink: chosen == way.kind ? Pc.accent : liquifyIdle, words: liquifyLabel, size: 24) {
                        model.chooseLiquifyKind(way.kind)
                    }
                }
                wideButton(PcIcons.adjustKnob, "Adjust", ink: ink, words: words, size: 39.5, enabled: canAdjust) {
                    model.liquifyPopup = model.liquifyPopup == 2 ? 0 : 2
                }
                wideButton(PcIcons.pieReset, "Reset", ink: ink, words: words, size: 23, enabled: canAdjust) {
                    model.resetLiquify()
                }
            }
            .frame(height: 60)
            HStack(spacing: 11) {
                slider(0, "Size")
                slider(1, "Pressure")
                slider(2, "Distortion", none: "None")
                slider(3, "Momentum", none: "None")
            }
        }
        .padding(.horizontal, 16)
        .padding(.top, 28)
        .frame(width: 1093, height: 162, alignment: .top)
        .background(RoundedRectangle(cornerRadius: 33, style: .circular).fill(panelFill))
    }

    private func uprightButton<Label: View>(enabled: Bool = true, action: @escaping () -> Void, @ViewBuilder label: () -> Label) -> some View {
        label()
            .frame(maxWidth: .infinity)
            .frame(height: 40)
            .background(Capsule().fill(panelButton.opacity(enabled ? 1 : 0.5)))
            .contentShape(Capsule())
            .onTapGesture {
                if enabled {
                    action()
                }
            }
    }

    /// Liquify upright: two rows of two sliders, then the way, Adjust and Reset.
    private var uprightPanel: some View {
        let name = liquifyKinds.first { $0.kind == model.adjustShown?.liquifyKind }?.name ?? ""
        let ink = canAdjust ? menuText : Color(argb: 0xFF57_5757)
        return VStack(spacing: 0) {
            HStack(spacing: 20) {
                slider(0, "Size")
                slider(1, "Pressure")
            }
            Spacer(minLength: 0)
            HStack(spacing: 20) {
                slider(2, "Distortion", none: "None")
                slider(3, "Momentum", none: "None")
            }
            Spacer(minLength: 0)
            HStack(spacing: 8) {
                uprightButton(action: { model.liquifyPopup = model.liquifyPopup == 1 ? 0 : 1 }) {
                    Text(verbatim: name)
                        .font(.system(size: 15))
                        .foregroundStyle(menuText)
                        .lineLimit(1)
                }
                uprightButton(enabled: canAdjust, action: { model.liquifyPopup = model.liquifyPopup == 2 ? 0 : 2 }) {
                    HStack(spacing: 2) {
                        // a 50 frame shown 39.5 across
                        PcIconView(PcIcons.adjustKnob, size: 39.5, color: ink)
                        Text(verbatim: "Adjust")
                            .font(.system(size: 15))
                            .foregroundStyle(ink)
                            .lineLimit(1)
                    }
                }
                uprightButton(enabled: canAdjust, action: { model.resetLiquify() }) {
                    HStack(spacing: 2.5) {
                        PcIconView(PcIcons.pieReset, size: 23, color: ink)
                        Text(verbatim: "Reset")
                            .font(.system(size: 15))
                            .foregroundStyle(ink)
                            .lineLimit(1)
                    }
                }
            }
        }
        .padding(16)
        .frame(width: 654, height: 200)
        .background(RoundedRectangle(cornerRadius: 33, style: .circular).fill(panelFill))
    }

    private func popover<Content: View>(_ title: String, centre: CGFloat, buttonsTop: CGFloat, @ViewBuilder content: @escaping () -> Content) -> some View {
        let left = min(max(centre - 130, 0), max(screen.width - 260, 0))
        return Popover(width: 260, arrowX: centre - left, fill: menuFill, corner: 26, side: .bottom) {
            VStack(alignment: .leading, spacing: 0) {
                Text(verbatim: title)
                    .font(.system(size: 24, weight: .bold))
                    .foregroundStyle(menuText)
                    .lineLimit(1)
                    .padding(.leading, 12)
                    .frame(height: 40)
                content()
            }
            .padding(10)
        }
        .frame(width: screen.width, height: buttonsTop, alignment: .bottomLeading)
        .offset(x: left)
    }

    /// The way's list and the adjust slider open over the panel's buttons.
    @ViewBuilder
    private var popups: some View {
        let panelLeft: CGFloat = (screen.width - 654) / 2
        let buttonsTop: CGFloat = wide ? screen.height - 25 - 162 + 28 : screen.height - 25 - 16 - 40
        if model.liquifyPopup == 1, !wide {
            popover("Liquify Mode", centre: panelLeft + 16 + (654 - 32 - 16) / 6, buttonsTop: buttonsTop) {
                VStack(spacing: 0) {
                    ForEach(Array(liquifyKinds.enumerated()), id: \.offset) { i, way in
                        let chosen = model.adjustShown?.liquifyKind == way.kind
                        if i > 0 {
                            menuLine.frame(height: 1)
                        }
                        HStack(spacing: 0) {
                            Text(verbatim: way.name)
                                .font(.system(size: 16))
                                .foregroundStyle(chosen ? Color.white : menuText)
                                .lineLimit(1)
                            Spacer(minLength: 0)
                            if chosen {
                                PcIconView(PcIcons.menuTick, size: 29, color: .white)
                            }
                        }
                        .padding(.leading, 16)
                        .padding(.trailing, 14)
                        .frame(height: 44)
                        .background(chosen ? Pc.accent : Color.clear)
                        .contentShape(Rectangle())
                        .onTapGesture { model.chooseLiquifyKind(way.kind) }
                    }
                }
                .background(menuGroup)
                .clipShape(RoundedRectangle(cornerRadius: 13, style: .circular))
                .padding(.top, 10)
            }
        } else if model.liquifyPopup == 2 {
            // over the Adjust button: the middle one upright, the ninth of the row when wide
            let centre: CGFloat = wide ? (screen.width - 1093) / 2 + 86 + 94 * 8 + 37.5 : screen.width / 2
            let amount = model.adjustShown?.liquifyAmount ?? 1
            popover("Adjust", centre: centre, buttonsTop: buttonsTop) {
                PanelSlider(label: "Amount", value: amount, shown: "\(wholeNumber(amount * 100))%") {
                    model.changeLiquifyAmount($0)
                }
                .padding(.vertical, 8)
                .background(RoundedRectangle(cornerRadius: 13, style: .circular).fill(menuGroup))
                .padding(.top, 4)
            }
        }
    }

    var body: some View {
        ZStack(alignment: .topLeading) {
            Group {
                if wide {
                    widePanel
                } else {
                    uprightPanel
                }
            }
            .frame(width: screen.width, height: screen.height - 25, alignment: .bottom)
            popups
        }
    }
}
