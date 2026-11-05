import SwiftUI

private func themed(_ dark: UInt32, _ light: UInt32) -> Color {
    Color(argb: Look.shared.light ? light : dark)
}

private var panelFill: Color { themed(0xF71E_1E1E, 0xF7F1_F1F1) }
private var hsbLabel: Color { themed(0xFF80_8080, 0xFF59_5959) }
private var hsbValue: Color { themed(0xFFED_EDED, 0xFF36_3636) }
private var panelButton: Color { themed(0xFF30_3030, 0xFFF8_F8F8) }
private var panelButtonText: Color { themed(0xFFA6_A6A6, 0xFF51_5151) }
private var modesText: Color { themed(0xFFD0_D0D0, 0xFF3B_3B3B) }
private var modesFill: Color { themed(0xFF7B_7B7B, 0xFFFF_FFFF) }
private var kindsFill: Color { themed(0xFF53_5353, 0xFFFF_FFFF) }
private var kindsEdge: Color { themed(0xFF74_7474, 0xFFFA_FAFA) }
private var sliderTrack: Color { themed(0xFF2E_2E2E, 0xFFDD_DDDD) }
private var sliderTick: Color { themed(0xFF4A_4A4A, 0xFFC8_C8C8) }
private var noiseButton: Color { themed(0xFF1E_1E1E, 0xFFFF_FFFF) }
private var moreFill: Color { themed(0xFF1C_1C1C, 0xFFFA_FAFA) }
private var moreEdge: Color { themed(0xFF3A_3A3A, 0xFFE1_E1E1) }
private var moreDots: Color { themed(0xFF9A_9A9A, 0xFF59_5959) }
private var optionsMenuFill: Color { themed(0xF226_2626, 0xF2F0_F0F0) }
private var optionsGroupFill: Color { themed(0xFF1B_1B1B, 0xFFFF_FFFF) }
private var optionsLine: Color { themed(0xFF2E_2E2E, 0xFFE2_E2E2) }
private var curveInk: Color { Look.shared.light ? .black : .white }
private let gridLine = Color(argb: 0xFF8A_8A8A)
private let curveNode = Color(argb: 0xFF01_71EA)
private let curveMenuFill = Color(argb: 0xFF26_2626)
private let knobShadow = Color(argb: 0x5500_0000)
private let thinKnobShadow = Color(argb: 0x3300_0000)
private let balanceKnob = Color(argb: 0xFF7A_7A7A)

private let hueStops: [UInt32] = [
    0xFFFF_0000, 0xFFFF_FF00, 0xFF00_FF00, 0xFF00_FFFF, 0xFF00_00FF, 0xFFFF_00FF, 0xFFFF_0000,
]
private let balancePairs: [(left: String, right: String, colours: [UInt32])] = [
    ("Cyan", "Red", [0xFF00_FFFF, 0xFF80_8080, 0xFFFF_0000]),
    ("Magenta", "Green", [0xFFFF_00FF, 0xFF80_8080, 0xFF00_FF00]),
    ("Yellow", "Blue", [0xFFFF_FF00, 0xFF80_8080, 0xFF00_00FF]),
]
private let balanceTones = ["Shadows", "Midtones", "Highlights"]

extension CanvasModel {
    /// The adjustments with a screen on the iPad so far.
    func opensOnThisDevice(_ a: Adjustment) -> Bool {
        a != .gradientMap
    }

    /// Changes the open adjustment and reads what it shows.
    func adjust(_ change: (CanvasSession) -> Void) {
        change(session)
        readAdjust()
    }

    /// Layer or Pencil: painting the adjustment on starts it at its own amount, and only where the
    /// brush goes shows it; back to the layer starts it again.
    func chooseAdjustMode(pencil: Bool) {
        guard let a = adjustment else { return }
        adjustModesOpen = false
        guard pencil != adjustShown?.pencil, adjustmentPaintable(adjustment: a) else { return }
        if pencil {
            adjust { $0.adjustmentPencil(pencil: true) }
            readSliders()
        } else {
            resetAdjustment()
        }
    }
}

/// The adjustment's own panel, rising from the foot of the screen.
struct AdjustmentPanel: View {
    let model: CanvasModel
    let screen: CGSize

    var body: some View {
        let wide = screen.width > screen.height
        if let shown = model.adjustShown {
            switch model.adjustment {
            case .hsb: HsbPanel(model: model, shown: shown, wide: wide)
            case .colorBalance: BalancePanel(model: model, shown: shown, wide: wide)
            case .curves: CurvesPanel(model: model, shown: shown)
            case .glitch: GlitchPanel(model: model, shown: shown, wide: wide)
            case .noise: NoisePanel(model: model, shown: shown, wide: wide)
            case .bloom: BloomPanel(model: model, shown: shown, wide: wide)
            case .halftone: HalftonePanel(model: model, shown: shown)
            case .chromaticAberration: ChromaticPanel(model: model, shown: shown)
            case .perspectiveBlur: PerspectivePanel(model: model, shown: shown)
            default: EmptyView()
            }
        }
    }
}

/// A panel's plate, `width` x `height`, 25 above the bottom.
private struct Plate: ViewModifier {
    let width: CGFloat
    let height: CGFloat

    func body(content: Content) -> some View {
        content
            .frame(width: width, height: height, alignment: .top)
            .background(RoundedRectangle(cornerRadius: 33, style: .circular).fill(panelFill))
            .contentShape(Rectangle())
            .onTapGesture {}
            .offset(y: -25)
    }
}

private extension View {
    func plate(_ width: CGFloat, _ height: CGFloat) -> some View {
        modifier(Plate(width: width, height: height))
    }
}

private func value(_ list: [Float], _ i: Int) -> Float {
    list.indices.contains(i) ? list[i] : 0
}

/// A slider's value as shown: None at nothing, Max at all (`max`), else a percentage.
private func reading(_ v: Float, max: Bool = false) -> String {
    if v <= 0 {
        return "None"
    }
    return max && v >= 1 ? "Max" : "\(wholeNumber(v * 100))%"
}

/// A labelled slider over a coloured track, its knob taking `knob` where it sits.
private struct TrackRow: View {
    let left: String
    let right: String
    let leftInk: Color
    let value: Float
    let track: [Color]
    let knob: Color
    let onChange: (Float) -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                Text(verbatim: left)
                    .foregroundStyle(leftInk)
                Spacer(minLength: 0)
                Text(verbatim: right)
                    .foregroundStyle(hsbValue)
            }
            .font(.system(size: 16))
            .lineLimit(1)
            .padding(.top, 6)
            GeometryReader { g in
                let w = g.size.width
                let x = min(max(w * CGFloat(value), 12), max(w - 12, 12))
                ZStack(alignment: .leading) {
                    Capsule()
                        .fill(LinearGradient(colors: track, startPoint: .leading, endPoint: .trailing))
                        .frame(height: 3)
                    Capsule().fill(knobShadow).frame(width: 24, height: 14).offset(x: x - 12, y: 0.5)
                    Capsule().fill(knob).frame(width: 24, height: 14).offset(x: x - 12)
                }
                .frame(width: w, height: 24)
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
                    onChange(Float(min(max(drag.location.x / max(w, 1), 0), 1)))
                })
            }
            .frame(height: 24)
            Spacer(minLength: 0)
        }
        .frame(height: 55)
    }
}

/// The floating panel of hue, saturation and brightness, each 0..100% with 50% leaving the layer as
/// it was.
private struct HsbPanel: View {
    let model: CanvasModel
    let shown: AdjustShown
    let wide: Bool

    private func set(_ h: Float, _ s: Float, _ b: Float) {
        model.adjust { $0.adjustmentHsb(hue: h, saturation: s, brightness: b) }
    }

    private func row(_ label: String, _ v: Float, _ track: [Color], _ knob: Color, _ change: @escaping (Float) -> Void) -> some View {
        TrackRow(left: label, right: "\(wholeNumber(v * 100))%", leftInk: hsbLabel, value: v, track: track, knob: knob, onChange: change)
    }

    @ViewBuilder private var rows: some View {
        let h = value(shown.hsb, 0)
        let s = value(shown.hsb, 1)
        let b = value(shown.hsb, 2)
        let grey = 0.5 + 0.5 * Double(s)
        let rest = 0.5 - 0.5 * Double(s)
        row("Hue", h, hueStops.map { Color(argb: $0) }, Color(hue: Double(h.truncatingRemainder(dividingBy: 1)), saturation: 1, brightness: 1)) { set($0, s, b) }
        row("Saturation", s, [Color(argb: 0xFF80_8080), Color(argb: 0xFFFF_0000)], Color(red: grey, green: rest, blue: rest)) { set(h, $0, b) }
        row("Brightness", b, [.black, .white], Color(white: Double(b))) { set(h, s, $0) }
    }

    var body: some View {
        if wide {
            // side by side, 301 wide and 13 apart
            HStack(spacing: 13) { rows }
                .padding(.horizontal, 16)
                .padding(.top, 24)
                .plate(961, 95)
        } else {
            VStack(spacing: 8) { rows }
                .padding(.horizontal, 26)
                .padding(.vertical, 16)
                .plate(532, 220)
        }
    }
}

/// The colour balance panel: three sliders between opposite colours, for the tone chosen below them
/// (side by side on a wide screen, the tones then in the menu of the "•••" button after them).
private struct BalancePanel: View {
    let model: CanvasModel
    let shown: AdjustShown
    let wide: Bool

    private func row(_ k: Int) -> some View {
        let pair = balancePairs[k]
        return TrackRow(
            left: pair.left, right: pair.right, leftInk: ToolbarInk.menuText,
            value: value(shown.balance, Int(shown.balanceTone) * 3 + k),
            track: pair.colours.map { Color(argb: $0) }, knob: balanceKnob,
            onChange: { v in model.adjust { $0.adjustmentBalance(k: UInt32(k), value: v) } }
        )
    }

    private func choose(_ tone: Int) {
        model.adjust { $0.adjustmentBalanceTone(tone: UInt32(tone)) }
    }

    var body: some View {
        let tone = Int(shown.balanceTone)
        if wide {
            HStack(alignment: .top, spacing: 19) {
                ForEach(0 ..< 3, id: \.self) { k in
                    row(k).frame(width: 251)
                }
                MoreButton(title: "Color Balance") {
                    OptionsGroup {
                        ForEach(balanceTones.indices, id: \.self) { i in
                            OptionsChoice(label: balanceTones[i], chosen: tone == i, height: 40, first: i == 0) { choose(i) }
                        }
                    }
                }
            }
            .padding(.leading, 16)
            .padding(.top, 24)
            .frame(maxWidth: .infinity, alignment: .leading)
            .plate(903, 95)
        } else {
            VStack(spacing: 8) {
                ForEach(0 ..< 3, id: \.self) { k in
                    row(k)
                }
                HStack(spacing: 7) {
                    ForEach(balanceTones.indices, id: \.self) { i in
                        let chosen = tone == i
                        Text(verbatim: balanceTones[i])
                            .font(.system(size: 15))
                            .foregroundStyle(chosen ? Color.white : panelButtonText)
                            .lineLimit(1)
                            .frame(maxWidth: .infinity, minHeight: 40, maxHeight: 40)
                            .background(Capsule().fill(chosen ? Pc.accent : panelButton))
                            .contentShape(Rectangle())
                            .onTapGesture { choose(i) }
                    }
                }
                .padding(.top, 4)
            }
            .padding(.horizontal, 26)
            .padding(.vertical, 16)
            .plate(532, 272)
        }
    }
}

/// The curves panel: the chosen channel's curve over a grid (drag its points; touch elsewhere to add
/// one; tap a point for Delete over it), and the channels.
private struct CurvesPanel: View {
    let model: CanvasModel
    let shown: AdjustShown

    /// The point whose Delete shows; the point a touch is moving, whether it made it, and whether it
    /// has moved.
    @State private var tapped: Int?
    @State private var holding: Int?
    @State private var added = false
    @State private var moved = false

    private var channel: Int {
        Int(shown.curveChannel)
    }

    private var points: [CGPoint] {
        guard shown.curves.indices.contains(channel) else { return [] }
        let flat = shown.curves[channel]
        return stride(from: 0, to: flat.count - 1, by: 2).map { CGPoint(x: CGFloat(flat[$0]), y: CGFloat(flat[$0 + 1])) }
    }

    private func toCurve(_ p: CGPoint, _ size: CGSize) -> (x: Float, y: Float) {
        (Float(min(max(p.x / size.width, 0), 1)), Float(min(max(1 - p.y / size.height, 0), 1)))
    }

    private func touched(_ drag: DragGesture.Value, _ size: CGSize) {
        if let index = holding {
            if !moved, hypot(drag.translation.width, drag.translation.height) < 8 {
                return
            }
            moved = true
            let c = toCurve(drag.location, size)
            model.adjust { $0.adjustmentCurveMove(index: UInt32(index), x: c.x, y: c.y) }
            return
        }
        tapped = nil
        moved = false
        let down = drag.startLocation
        let near = points.firstIndex {
            hypot($0.x * size.width - down.x, (1 - $0.y) * size.height - down.y) <= 22
        }
        added = near == nil
        if let near {
            holding = near
        } else {
            let c = toCurve(down, size)
            var index = 0
            model.adjust { index = Int($0.adjustmentCurveAdd(x: c.x, y: c.y)) }
            holding = index
        }
    }

    private func graph(_ size: CGSize) -> some View {
        let shape = model.session.adjustmentCurveShape(channel: UInt32(channel))
        let nodes = points
        return Canvas { context, canvas in
            let w = canvas.width
            let h = canvas.height
            var grid = Path()
            for k in 0 ... 4 {
                let t = CGFloat(k) / 4
                grid.move(to: CGPoint(x: w * t, y: 0))
                grid.addLine(to: CGPoint(x: w * t, y: h))
                grid.move(to: CGPoint(x: 0, y: h * t))
                grid.addLine(to: CGPoint(x: w, y: h * t))
            }
            context.stroke(grid, with: .color(gridLine), lineWidth: 1)
            var line = Path()
            for (i, y) in shape.enumerated() {
                let p = CGPoint(x: CGFloat(i) / 255 * w, y: (1 - CGFloat(y)) * h)
                if i == 0 {
                    line.move(to: p)
                } else {
                    line.addLine(to: p)
                }
            }
            context.stroke(line, with: .color(curveInk), lineWidth: 1.5)
            for p in nodes {
                let at = CGPoint(x: p.x * w, y: (1 - p.y) * h)
                context.fill(Path(ellipseIn: CGRect(x: at.x - 5.5, y: at.y - 5.5, width: 11, height: 11)), with: .color(.white))
                context.fill(Path(ellipseIn: CGRect(x: at.x - 3.5, y: at.y - 3.5, width: 7, height: 7)), with: .color(curveNode))
            }
        }
        .contentShape(Rectangle())
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { touched($0, size) }
                .onEnded { _ in
                    // a tap on a point: Delete over it
                    if !moved, !added {
                        tapped = holding
                    }
                    holding = nil
                }
        )
    }

    var body: some View {
        HStack(spacing: 16) {
            GeometryReader { g in
                ZStack(alignment: .topLeading) {
                    graph(g.size)
                    if let i = tapped, points.indices.contains(i) {
                        // a pill 78 x 44 over the point, its bottom 14 above it
                        Text(verbatim: "Delete")
                            .font(.system(size: 17))
                            .foregroundStyle(Color.white)
                            .lineLimit(1)
                            .frame(width: 78, height: 44)
                            .background(Capsule().fill(curveMenuFill))
                            .contentShape(Capsule())
                            .onTapGesture {
                                model.adjust { $0.adjustmentCurveDelete(index: UInt32(i)) }
                                tapped = nil
                            }
                            .offset(x: points[i].x * g.size.width - 39, y: (1 - points[i].y) * g.size.height - 58)
                    }
                }
            }
            .padding(8)
            VStack(spacing: 1) {
                ForEach(Array(["Gamma", "Red", "Green", "Blue"].enumerated()), id: \.offset) { i, name in
                    let chosen = channel == i
                    Text(verbatim: name)
                        .font(.system(size: 16))
                        .foregroundStyle(chosen ? Color.white : panelButtonText)
                        .lineLimit(1)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .background(chosen ? Pc.accent : panelButton)
                        .contentShape(Rectangle())
                        .onTapGesture {
                            tapped = nil
                            model.adjust { $0.adjustmentCurveChannel(channel: UInt32(i)) }
                        }
                }
            }
            .frame(width: 111)
            .clipShape(RoundedRectangle(cornerRadius: 13, style: .circular))
        }
        .padding(16)
        .plate(584, 183)
    }
}

/// One choice in a `KindsSwitch`.
private struct Kind {
    let icon: PcIcon
    let label: String
    let width: CGFloat
}

/// A toolbar's row of kinds, `width` wide, the `chosen` one blue.
private struct KindsSwitch: View {
    let width: CGFloat
    let kinds: [Kind]
    let chosen: Int
    let onChoose: (Int) -> Void

    var body: some View {
        HStack(spacing: 0) {
            ForEach(kinds.indices, id: \.self) { i in
                let on = i == chosen
                let ink = on ? Color.white : modesText
                if i > 0 {
                    Spacer(minLength: 0)
                }
                // drawn in a 50 frame, shown 30 across, the name 2 after
                HStack(spacing: 2) {
                    PcIconView(kinds[i].icon, size: 30, color: ink)
                    Text(verbatim: kinds[i].label)
                        .font(.system(size: 14))
                        .foregroundStyle(ink)
                        .lineLimit(1)
                        .fixedSize()
                }
                .frame(width: kinds[i].width, height: 30)
                .background(Capsule().fill(on ? Pc.accent : Color.clear))
                .contentShape(Capsule())
                .onTapGesture { onChoose(i) }
            }
        }
        .padding(2)
        .frame(width: width, height: 34)
        .background(Capsule().fill(kindsFill))
        .overlay(Capsule().strokeBorder(kindsEdge, lineWidth: 1))
    }
}

/// A labelled toolbar slider `width` wide: `both` ways runs -1..1 with its fill from the middle,
/// else 0..1.
private struct OptionSlider: View {
    let label: String
    let value: Float
    let shown: String
    let width: CGFloat
    var both = false
    var labelInk: Color?
    let onChange: (Float) -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                Text(verbatim: label)
                    .foregroundStyle(labelInk ?? hsbLabel)
                Spacer(minLength: 0)
                Text(verbatim: shown)
                    .foregroundStyle(hsbValue)
            }
            .font(.system(size: 16))
            .lineLimit(1)
            .padding(.leading, 10)
            .padding(.trailing, 15)
            .padding(.top, 6)
            GeometryReader { g in
                let w = g.size.width
                let t = both ? (value + 1) / 2 : value
                let x = w * CGFloat(min(max(t, 0), 1))
                let from = both ? w / 2 : 0
                let knob = min(max(x, 8.5), max(w - 8.5, 8.5))
                ZStack(alignment: .leading) {
                    Capsule().fill(sliderTrack).frame(height: 2)
                    Capsule().fill(Pc.accent)
                        .frame(width: abs(x - from), height: 2)
                        .offset(x: min(x, from))
                    if both {
                        Rectangle().fill(sliderTick).frame(width: 1, height: 12).offset(x: from - 0.5)
                    }
                    Capsule().fill(thinKnobShadow).frame(width: 17, height: 11).offset(x: knob - 8.5, y: 0.5)
                    Capsule().fill(Color.white).frame(width: 17, height: 11).offset(x: knob - 8.5)
                }
                .frame(width: w, height: 24)
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
                    guard drag.translation != .zero else { return }
                    let at = Float(min(max(drag.location.x / max(w, 1), 0), 1))
                    onChange(both ? at * 2 - 1 : at)
                })
            }
            .frame(height: 24)
            .padding(.leading, 10)
            .padding(.trailing, 11)
            Spacer(minLength: 0)
        }
        .frame(width: width, height: 54)
    }
}

/// A toolbar's "•••" button (41 x 40, 3 lower than the sliders' top): it opens a small menu over it,
/// `title` above the `content`, 16 above the button; a tap outside closes it.
private struct MoreButton<Content: View>: View {
    let title: String
    @ViewBuilder let content: () -> Content

    @State private var open = false

    var body: some View {
        HStack(spacing: 4) {
            ForEach(0 ..< 3, id: \.self) { _ in
                Circle().fill(moreDots).frame(width: 6, height: 6)
            }
        }
        .frame(width: 41, height: 40)
        .background(Capsule().fill(moreFill))
        .overlay(Capsule().strokeBorder(moreEdge, lineWidth: 1))
        .contentShape(Capsule())
        .onTapGesture { open.toggle() }
        .padding(.top, 3)
        .overlay(alignment: .topLeading) {
            if open {
                MenuDismissal { open = false }
            }
        }
        .overlay(alignment: .bottom) {
            if open {
                VStack(alignment: .leading, spacing: 0) {
                    Text(verbatim: title)
                        .font(.system(size: 23, weight: .bold))
                        .foregroundStyle(ToolbarInk.menuText)
                        .lineLimit(1)
                        .padding(.leading, 12)
                        .padding(.vertical, 16)
                    content()
                }
                .padding(.horizontal, 10)
                .padding(.bottom, 10)
                .frame(width: 260)
                .background(RoundedRectangle(cornerRadius: 26, style: .circular).fill(optionsMenuFill))
                .contentShape(Rectangle())
                .onTapGesture {}
                .fixedSize()
                .offset(y: -56)
            }
        }
    }
}

/// Rows in a rounded group of an options menu.
private struct OptionsGroup<Rows: View>: View {
    @ViewBuilder let rows: () -> Rows

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 16, style: .circular)
        VStack(spacing: 0) {
            rows()
        }
        .frame(maxWidth: .infinity)
        .background(shape.fill(optionsGroupFill))
        .clipShape(shape)
    }
}

/// One choice of an options menu: blue with a tick when `chosen`, a line above all but the `first`.
private struct OptionsChoice: View {
    let label: String
    let chosen: Bool
    let height: CGFloat
    let first: Bool
    let onChoose: () -> Void

    var body: some View {
        if !first {
            Rectangle().fill(optionsLine).frame(height: 1)
        }
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 15))
                .foregroundStyle(chosen ? Color.white : ToolbarInk.menuText)
                .lineLimit(1)
            Spacer(minLength: 0)
            if chosen {
                PcIconView(PcIcons.tick, size: 20, color: .white)
            }
        }
        .padding(.leading, 12)
        .padding(.trailing, 14)
        .frame(height: height)
        .background(chosen ? Pc.accent : Color.clear)
        .contentShape(Rectangle())
        .onTapGesture(perform: onChoose)
    }
}

/// The glitch toolbar: its four kinds above the kind's sliders, two to a row.
private struct GlitchPanel: View {
    let model: CanvasModel
    let shown: AdjustShown
    let wide: Bool

    private static let kinds: [(kind: GlitchKind, row: Kind)] = [
        (.artifact, Kind(icon: PcIcons.glitchArtifact, label: "Artifact", width: 111)),
        (.wave, Kind(icon: PcIcons.glitchWave, label: "Wave", width: 99)),
        (.signal, Kind(icon: PcIcons.glitchSignal, label: "Signal", width: 104)),
        (.diverge, Kind(icon: PcIcons.glitchDiverge, label: "Diverge", width: 113)),
    ]

    private var names: [String] {
        switch shown.glitchKind {
        case .artifact, .signal: ["Amount", "Block Size", "Zoom"]
        case .wave: ["Amplitude", "Frequency", "Zoom"]
        case .diverge: ["Red shift", "Green shift", "Blue shift", "Zoom"]
        }
    }

    private func slider(_ k: Int, width: CGFloat) -> some View {
        let v = value(shown.glitch, k)
        // diverge's shifts go both ways from the middle
        return OptionSlider(
            label: names[k], value: v, shown: "\(wholeNumber(v * 100))%", width: width,
            both: shown.glitchKind == .diverge && k < 3,
            onChange: { n in model.adjust { $0.adjustmentGlitchValue(index: UInt32(k), value: n) } }
        )
    }

    var body: some View {
        let count = names.count
        VStack(spacing: 12) {
            KindsSwitch(
                width: 466, kinds: Self.kinds.map { $0.row },
                chosen: Self.kinds.firstIndex { $0.kind == shown.glitchKind } ?? 0,
                onChoose: { i in model.adjust { $0.adjustmentGlitchKind(kind: Self.kinds[i].kind) } }
            )
            if wide {
                // all in one row, 19 apart
                let each = (941 - 19 * CGFloat(count - 1)) / CGFloat(count)
                HStack(spacing: 19) {
                    ForEach(0 ..< count, id: \.self) { k in
                        slider(k, width: each)
                    }
                }
                .frame(width: 941, height: 54)
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(stride(from: 0, to: count, by: 2)), id: \.self) { first in
                        HStack(spacing: 8) {
                            slider(first, width: 300)
                            if first + 1 < count {
                                slider(first + 1, width: 300)
                            } else {
                                Spacer(minLength: 0)
                            }
                        }
                        .frame(width: 608, height: 54)
                    }
                }
            }
        }
        .padding(.top, 8)
        .plate(wide ? 973 : 640, wide ? 120 : 167)
    }
}

/// The noise toolbar: its three kinds, scale, octaves and turbulence, single or multi channel, added
/// or not.
private struct NoisePanel: View {
    let model: CanvasModel
    let shown: AdjustShown
    let wide: Bool

    private static let kinds: [NoiseKind] = [.clouds, .billows, .ridges]

    private var kindsSwitch: some View {
        KindsSwitch(
            width: 472,
            kinds: [
                Kind(icon: PcIcons.noiseClouds, label: "Clouds", width: 149),
                Kind(icon: PcIcons.noiseBillows, label: "Billows", width: 149),
                Kind(icon: PcIcons.noiseRidges, label: "Ridges", width: 149),
            ],
            chosen: Self.kinds.firstIndex(of: shown.noiseKind) ?? 0,
            onChoose: { i in model.adjust { $0.adjustmentNoiseKind(kind: Self.kinds[i]) } }
        )
    }

    private func slider(_ label: String, _ k: Int, width: CGFloat) -> some View {
        let v = value(shown.noise, k)
        return OptionSlider(
            label: label, value: v, shown: reading(v), width: width, labelInk: modesText,
            onChange: { n in model.adjust { $0.adjustmentNoiseValue(index: UInt32(k), value: n) } }
        )
    }

    private func chooseMulti(_ multi: Bool) {
        model.adjust { $0.adjustmentNoiseWays(multi: multi, added: shown.noiseAdded) }
    }

    private func toggleAdded() {
        model.adjust { $0.adjustmentNoiseWays(multi: shown.noiseMulti, added: !shown.noiseAdded) }
    }

    var body: some View {
        if wide {
            // the sliders side by side, single or multi channel and additive in the "•••" menu
            VStack(spacing: 8) {
                kindsSwitch
                HStack(alignment: .top, spacing: 19) {
                    slider("Scale", 0, width: 251)
                    slider("Octaves", 1, width: 251)
                    slider("Turbulence", 2, width: 251)
                    MoreButton(title: "Channel") {
                        OptionsGroup {
                            HStack(spacing: 0) {
                                Text(verbatim: "Additive")
                                    .font(.system(size: 15))
                                    .foregroundStyle(ToolbarInk.menuText)
                                    .lineLimit(1)
                                Spacer(minLength: 0)
                                PcSwitch(on: shown.noiseAdded)
                                    .contentShape(Rectangle())
                                    .onTapGesture(perform: toggleAdded)
                            }
                            .padding(.leading, 12)
                            .padding(.trailing, 8)
                            .frame(height: 44)
                        }
                        .padding(.bottom, 12)
                        OptionsGroup {
                            OptionsChoice(label: "Single", chosen: !shown.noiseMulti, height: 44, first: true) { chooseMulti(false) }
                            OptionsChoice(label: "Multi", chosen: shown.noiseMulti, height: 44, first: false) { chooseMulti(true) }
                        }
                    }
                }
                .padding(.leading, 16)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(.top, 8)
            .plate(903, 113)
        } else {
            VStack(spacing: 0) {
                kindsSwitch
                    .padding(.bottom, 8)
                VStack(spacing: 8.5) {
                    slider("Scale", 0, width: 500)
                    slider("Octaves", 1, width: 500)
                    slider("Turbulence", 2, width: 500)
                }
                .padding(.bottom, 12)
                HStack(spacing: 0) {
                    ForEach([false, true], id: \.self) { multi in
                        let on = shown.noiseMulti == multi
                        Text(verbatim: multi ? "Multi" : "Single")
                            .font(.system(size: 15))
                            .foregroundStyle(on ? Color.white : modesText)
                            .lineLimit(1)
                            .frame(width: 160, height: 40)
                            .background(Capsule().fill(on ? Pc.accent : noiseButton))
                            .contentShape(Rectangle())
                            .onTapGesture { chooseMulti(multi) }
                            .padding(.trailing, 7)
                    }
                    Text(verbatim: "Additive")
                        .font(.system(size: 16))
                        .foregroundStyle(modesText)
                        .lineLimit(1)
                        .frame(maxWidth: .infinity)
                    PcSwitch(on: shown.noiseAdded)
                        .contentShape(Rectangle())
                        .onTapGesture(perform: toggleAdded)
                }
                .frame(width: 492, height: 40)
            }
            .padding(.top, 10)
            .plate(532, 292)
        }
    }
}

/// The bloom toolbar: transition, size and burn.
private struct BloomPanel: View {
    let model: CanvasModel
    let shown: AdjustShown
    let wide: Bool

    private func slider(_ label: String, _ k: Int, width: CGFloat) -> some View {
        let v = value(shown.bloom, k)
        return OptionSlider(
            label: label, value: v, shown: reading(v, max: true), width: width,
            onChange: { n in model.adjust { $0.adjustmentBloomValue(index: UInt32(k), value: n) } }
        )
    }

    var body: some View {
        if wide {
            // side by side, 301 wide and 39 apart
            HStack(spacing: 39) {
                slider("Transition", 0, width: 301)
                slider("Size", 1, width: 301)
                slider("Burn", 2, width: 301)
            }
            .frame(width: 981)
            .padding(.top, 24)
            .plate(1013, 95)
        } else {
            VStack(spacing: 8.5) {
                slider("Transition", 0, width: 500)
                slider("Size", 1, width: 500)
                slider("Burn", 2, width: 500)
            }
            .frame(width: 500)
            .padding(.top, 24)
            .plate(532, 220)
        }
    }
}

/// The halftone toolbar: full colour, screen print or newspaper.
private struct HalftonePanel: View {
    let model: CanvasModel
    let shown: AdjustShown

    private static let kinds: [HalftoneKind] = [.fullColor, .screenPrint, .newspaper]

    var body: some View {
        KindsSwitch(
            width: 475,
            kinds: [
                Kind(icon: PcIcons.halftoneFull, label: "Full Color", width: 140),
                Kind(icon: PcIcons.halftoneScreen, label: "Screen Print", width: 157),
                Kind(icon: PcIcons.halftoneNews, label: "Newspaper", width: 151),
            ],
            chosen: Self.kinds.firstIndex(of: shown.halftoneKind) ?? 0,
            onChoose: { i in model.adjust { $0.adjustmentHalftoneKind(kind: Self.kinds[i]) } }
        )
        .padding(.top, 16)
        .plate(507, 66)
    }
}

/// The perspective blur toolbar: positional or directional.
private struct PerspectivePanel: View {
    let model: CanvasModel
    let shown: AdjustShown

    private static let kinds: [PerspectiveKind] = [.positional, .directional]

    var body: some View {
        KindsSwitch(
            width: 460,
            kinds: [
                Kind(icon: PcIcons.perspectivePositional, label: "Positional", width: 205),
                Kind(icon: PcIcons.perspectiveDirectional, label: "Directional", width: 211.5),
            ],
            chosen: Self.kinds.firstIndex(of: shown.perspectiveKind) ?? 0,
            onChoose: { i in model.adjust { $0.adjustmentPerspectiveKind(kind: Self.kinds[i]) } }
        )
        .padding(.top, 16)
        .plate(492, 66)
    }
}

/// The chromatic aberration toolbar: perspective or displace, with the kind's two sliders.
private struct ChromaticPanel: View {
    let model: CanvasModel
    let shown: AdjustShown

    private static let kinds: [ChromaticKind] = [.perspective, .displace]

    var body: some View {
        let names = shown.chromaKind == .perspective ? ["Transition", "Fall off"] : ["Blur", "Transparency"]
        VStack(spacing: 12) {
            KindsSwitch(
                width: 428,
                kinds: [
                    Kind(icon: PcIcons.chromaPerspective, label: "Perspective", width: 204),
                    Kind(icon: PcIcons.chromaDisplace, label: "Displace", width: 204),
                ],
                chosen: Self.kinds.firstIndex(of: shown.chromaKind) ?? 0,
                onChoose: { i in model.adjust { $0.adjustmentChromaKind(kind: Self.kinds[i]) } }
            )
            HStack(spacing: 20) {
                ForEach(names.indices, id: \.self) { k in
                    let v = value(shown.chroma, k)
                    OptionSlider(
                        label: names[k], value: v, shown: reading(v, max: true), width: 300,
                        onChange: { n in model.adjust { $0.adjustmentChromaValue(index: UInt32(k), value: n) } }
                    )
                }
            }
            .frame(width: 620)
        }
        .padding(.top, 8)
        .plate(668, 120)
    }
}

/// The Layer / Pencil choice under the mode bar: the adjustment changes the layer, or is painted on.
struct AdjustModes: View {
    let model: CanvasModel

    var body: some View {
        if model.adjustModesOpen {
            HStack(spacing: 1) {
                mode("Layer", PcIcons.modeLayer, pencil: false, width: 27)
                mode("Pencil", PcIcons.modePencil, pencil: true, width: 24)
            }
            .padding(2)
            .frame(width: 225, height: 36)
            .background(Capsule().fill(modesFill))
            .frame(maxWidth: .infinity)
            .offset(y: Pc.barHeight + 52)
        }
    }

    private func mode(_ label: String, _ icon: PcIcon, pencil: Bool, width: CGFloat) -> some View {
        let chosen = model.adjustShown?.pencil == pencil
        // drawn in 27 x 32 (the pencil 24 x 32) frames, shown as they are
        return HStack(spacing: 1.5) {
            PcIconView(icon, color: .white)
                .frame(width: width, height: 32)
            Text(verbatim: label)
                .font(.system(size: 14))
                .foregroundStyle(Color.white)
                .lineLimit(1)
            Spacer(minLength: 0)
        }
        .padding(.leading, 24)
        .frame(width: 110, height: 32)
        .background(Capsule().fill(chosen ? Pc.accent : Color.clear))
        .contentShape(Capsule())
        .onTapGesture { model.chooseAdjustMode(pencil: pencil) }
    }
}

/// The clone source: a dark ring with a light edge, so it shows on any colour.
struct ClonePuck: View {
    let model: CanvasModel

    var body: some View {
        if let puck = model.adjustShown?.clonePuck, puck.count >= 2 {
            let at = CGPoint(x: CGFloat(puck[0]) / model.pixels, y: CGFloat(puck[1]) / model.pixels)
            Canvas { context, _ in
                let ring = Path(ellipseIn: CGRect(x: at.x - 30, y: at.y - 30, width: 60, height: 60))
                context.stroke(ring, with: .color(.white), lineWidth: 4)
                context.stroke(ring, with: .color(.black), lineWidth: 2)
            }
            .allowsHitTesting(false)
        }
    }
}

/// Perspective blur's puck: its radius, fill (a dark veil) and shadow (as dark as this at its edge,
/// gone this far out). Directional's is bigger, with a white rim and, round its arrow, a dark band
/// between two radii a right angle wide holding a light triangle that points the arrow's way. Both
/// have a white dot in the middle.
struct PerspectivePuck: View {
    let model: CanvasModel

    private func draw(_ context: GraphicsContext, at: CGPoint, ahead: CGPoint, directional: Bool) {
        let r: CGFloat = directional ? 78.25 : 38.75
        let reach: CGFloat = directional ? 45 : 19
        let dark = directional ? 0.208 : 0.314
        let fill = Color(argb: directional ? 0xB61F_1F1F : 0xAC22_2222)
        let arrow = atan2(ahead.y - at.y, ahead.x - at.x)
        func disc(_ radius: CGFloat) -> Path {
            Path(ellipseIn: CGRect(x: at.x - radius, y: at.y - radius, width: radius * 2, height: radius * 2))
        }
        // the shadow fades out from the edge, starting where the puck's own edge is drawn half across
        // a pixel, so neither a light seam nor a dark ring shows there
        let outside = r + reach
        let under = r - 0.5
        var stops: [Gradient.Stop] = [
            .init(color: .clear, location: 0),
            .init(color: .clear, location: under / outside),
            .init(color: .black.opacity(dark), location: (under + 0.01) / outside),
        ]
        for k in 0 ... 8 {
            let t = CGFloat(k) / 8
            stops.append(.init(color: .black.opacity(dark * Double((1 - t) * (1 - t))), location: (r + t * (outside - r)) / outside))
        }
        context.fill(
            disc(outside),
            with: .radialGradient(Gradient(stops: stops), center: at, startRadius: 0, endRadius: outside)
        )
        context.fill(disc(r), with: .color(fill))
        if directional {
            context.stroke(disc(r - 0.5), with: .color(.white), lineWidth: 1)
            var band = Path()
            let degrees = arrow * 180 / .pi
            band.addArc(center: at, radius: 59, startAngle: .degrees(degrees - 45), endAngle: .degrees(degrees + 45), clockwise: false)
            context.stroke(band, with: .color(Color(argb: 0x8000_0000)), lineWidth: 35)
            let dir = CGPoint(x: cos(arrow), y: sin(arrow))
            let side = CGPoint(x: -dir.y * 4.5, y: dir.x * 4.5)
            let foot = CGPoint(x: at.x + dir.x * 56, y: at.y + dir.y * 56)
            var tip = Path()
            tip.move(to: CGPoint(x: at.x + dir.x * 63, y: at.y + dir.y * 63))
            tip.addLine(to: CGPoint(x: foot.x + side.x, y: foot.y + side.y))
            tip.addLine(to: CGPoint(x: foot.x - side.x, y: foot.y - side.y))
            tip.closeSubpath()
            context.fill(tip, with: .color(Color(argb: 0xFFAC_ACAC)))
        }
        context.fill(disc(2), with: .color(.white))
    }

    var body: some View {
        if let puck = model.adjustShown?.perspectivePuck, puck.count == 4 {
            let pixels = model.pixels
            let at = CGPoint(x: CGFloat(puck[0]) / pixels, y: CGFloat(puck[1]) / pixels)
            let ahead = CGPoint(x: CGFloat(puck[2]) / pixels, y: CGFloat(puck[3]) / pixels)
            let directional = model.adjustShown?.perspectiveKind == .directional
            Canvas { context, _ in
                draw(context, at: at, ahead: ahead, directional: directional)
            }
            .allowsHitTesting(false)
        }
    }
}
