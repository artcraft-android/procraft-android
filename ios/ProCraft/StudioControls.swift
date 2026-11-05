import SwiftUI

/// How far in from the track's ends a studio slider's knob stops.
private let knobInset: CGFloat = 5
/// How far in from the row's ends the taper handles stop: where the sliders' knobs do.
private let taperTravel: CGFloat = 15
/// How far a touch goes before it is a drag and not a tap.
private let touchSlop: CGFloat = 8

private func themed(_ dark: UInt32, _ light: UInt32) -> Color {
    Color(argb: Look.shared.light ? light : dark)
}

/// Whether a control waiting on `by` may be used.
private func isEnabled(_ studio: StudioModel, _ by: EnabledBy?) -> Bool {
    guard let by else { return true }
    return wholeNumber(studio.value(by.key)) == by.choice
}

/// One control of a page.
struct StudioControlView: View {
    let model: CanvasModel
    let ui: StudioUi
    let control: StudioControl
    let landscape: Bool

    private var studio: StudioModel {
        model.studio
    }

    var body: some View {
        switch control {
        case let .slider(key, label, shown, enabledBy):
            StudioSlider(
                key: key,
                label: label,
                value: studio.value(key),
                shown: shown,
                most: shown == .size ? studio.value("most_size") : 16,
                enabled: isEnabled(studio, enabledBy),
                extras: keypadExtras(studio, key),
                ui: ui
            ) { studio.set(key, $0) }
        case let .toggle(key, label, enabledBy):
            StudioToggle(studio: studio, key: key, label: label, enabled: isEnabled(studio, enabledBy))
        case let .choice(key, options):
            ChoiceList(studio: studio, key: key, options: options)
        case let .kinds(key, options):
            KindsBar(studio: studio, key: key, options: options)
        case let .taperBar(start, end, link):
            VStack(spacing: 0) {
                // linked, the two ends move together
                let linked = studio.value(link) >= 0.5
                TaperBar(
                    start: studio.value(start),
                    end: studio.value(end),
                    onStart: {
                        studio.set(start, $0)
                        if linked {
                            studio.set(end, $0)
                        }
                    },
                    onEnd: {
                        studio.set(end, $0)
                        if linked {
                            studio.set(start, $0)
                        }
                    }
                )
                Color.clear.frame(height: 4.5)
                StudioToggle(studio: studio, key: link, label: "Link tip sizes", enabled: true)
            }
        case .pressureCurve:
            PressureCurve(
                points: studio.curve,
                edge: themed(0xFF35_3535, 0xFFD8_D8D8),
                fill: themed(0xFF1B_1B1B, 0xFFFF_FFFF),
                corner: 8,
                line: themed(0xFFD5_D5D5, 0xFF3A_3A3A),
                lineWidth: 1,
                ring: 5.5,
                middle: 3.5,
                ui: ui
            ) { studio.changeCurve($0) }
                .aspectRatio(1, contentMode: .fit)
                .padding(.horizontal, 8)
        case .tiltAngle:
            TiltAngle(value: studio.value("tilt_angle")) { studio.set("tilt_angle", $0) }
        case .roundness:
            RoundnessGraph(
                roundness: studio.value("roundness"),
                angle: studio.value("shape_angle"),
                onRoundness: { studio.set("roundness", $0) },
                onAngle: { studio.set("shape_angle", $0) }
            )
        case let .source(label, kind):
            if landscape {
                SourceBox(studio: studio, label: label, kind: kind)
            } else {
                SourceRow(studio: studio, label: label, kind: kind)
            }
        case let .picker(key, label):
            BlendPicker(model: model, ui: ui, key: key, label: label)
        case let .disclosure(label, controls):
            DisclosureCard(model: model, ui: ui, label: label, controls: controls, landscape: landscape)
        case .previewCard:
            PreviewCard(studio: studio, landscape: landscape)
        case .about:
            AboutBrushView(studio: studio)
        }
    }
}

// MARK: switches and lists

/// A switch row: a long name runs on under the switch, which is drawn over it.
private struct StudioToggle: View {
    let studio: StudioModel
    let key: String
    let label: String
    let enabled: Bool

    var body: some View {
        let on = studio.value(key) >= 0.5
        ZStack(alignment: .leading) {
            Text(verbatim: label)
                .font(.system(size: 17))
                .foregroundStyle(enabled ? StudioInk.dim : StudioInk.off)
                .lineLimit(1)
                .fixedSize()
            PcSwitch(on: on, enabled: enabled, fixedTrack: StudioInk.switchFixed, fixedKnob: StudioInk.switchFixedKnob)
                .contentShape(Rectangle())
                .onTapGesture {
                    if enabled {
                        studio.set(key, on ? 0 : 1)
                    }
                }
                .padding(.trailing, 6)
                .frame(maxWidth: .infinity, alignment: .trailing)
        }
        .frame(height: 47)
        .padding(.leading, 8)
    }
}

/// A choice of a few, one to a row, the chosen one blue with a tick.
private struct ChoiceList: View {
    let studio: StudioModel
    let key: String
    let options: [String]

    var body: some View {
        let chosen = wholeNumber(studio.value(key))
        VStack(spacing: 0) {
            ForEach(Array(options.enumerated()), id: \.offset) { i, label in
                if i > 0 {
                    StudioInk.listLine.frame(height: 1)
                }
                HStack(spacing: 0) {
                    Text(verbatim: label)
                        .font(.system(size: 17))
                        .foregroundStyle(i == chosen ? Color.white : StudioInk.dim)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    if i == chosen {
                        PcIconView(PcIcons.menuTick, size: 34, color: .white)
                    }
                }
                .padding(.leading, 16)
                .padding(.trailing, 14)
                .frame(height: 44)
                .background(i == chosen ? Pc.accent : Color.clear)
                .contentShape(Rectangle())
                .onTapGesture { studio.set(key, Float(i)) }
            }
        }
        .background(StudioInk.list)
        .clipShape(RoundedRectangle(cornerRadius: 22, style: .circular))
    }
}

/// A choice between kinds, each an icon and a name, side by side.
private struct KindsBar: View {
    let studio: StudioModel
    let key: String
    let options: [StudioKind]

    var body: some View {
        let chosen = wholeNumber(studio.value(key))
        HStack(spacing: 11) {
            ForEach(Array(options.enumerated()), id: \.offset) { i, kind in
                let ink = i == chosen ? Color.white : StudioInk.dim
                HStack(spacing: 2) {
                    // drawn in a 50 frame, shown 30 across
                    PcIconView(kind.icon, size: 30, color: ink)
                    Text(verbatim: kind.name)
                        .font(.system(size: 13))
                        .foregroundStyle(ink)
                        .lineLimit(1)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(RoundedRectangle(cornerRadius: 15, style: .circular).fill(i == chosen ? Pc.accent : Color.clear))
                .contentShape(Rectangle())
                .onTapGesture { studio.set(key, Float(i)) }
            }
        }
        .padding(2)
        .frame(height: 34)
        .background(RoundedRectangle(cornerRadius: 17, style: .circular).fill(StudioInk.segments))
        .padding(.horizontal, 8)
    }
}

// MARK: the slider

/// What a slider's keypad offers besides typing: its own pressure curve (none while off) and its
/// own tilt (its switch, if it has one, and the angle the tilt starts from, 0..1 of a quarter
/// turn). Each setter changes the brush at once.
struct KeypadExtras {
    let pressure: Bool
    let curve: () -> [CGPoint]?
    let setCurve: ([CGPoint]?) -> Void
    let tilt: Bool
    let tiltOn: () -> Bool?
    let setTiltOn: (Bool) -> Void
    let tiltAngle: () -> Float
    let setTiltAngle: (Float) -> Void
}

/// What control `key`'s keypad offers besides typing, as its brush holds it; none if nothing.
private func keypadExtras(_ studio: StudioModel, _ key: String) -> KeypadExtras? {
    let e = sliderExtras(key: key)
    guard e.pressure || e.tilt else { return nil }
    return KeypadExtras(
        pressure: e.pressure,
        curve: { studio.sliderCurve(key) },
        setCurve: { studio.setSliderCurve(key, $0) },
        tilt: e.tilt,
        tiltOn: { e.tiltSwitch.isEmpty ? nil : studio.value(e.tiltSwitch) >= 0.5 },
        setTiltOn: { on in
            if !e.tiltSwitch.isEmpty {
                studio.set(e.tiltSwitch, on ? 1 : 0)
            }
        },
        tiltAngle: { studio.value(e.tiltAngle) },
        setTiltAngle: { studio.set(e.tiltAngle, $0) }
    )
}

/// A labelled slider with its value in a pill: blue up to the knob, or out from the middle both
/// ways. A tap on the value types it on a keypad beside it, the pill blue meanwhile.
private struct StudioSlider: View {
    let key: String
    let label: String
    let value: Float
    let shown: Shown
    let most: Float
    let enabled: Bool
    let extras: KeypadExtras?
    let ui: StudioUi
    let onChange: (Float) -> Void

    /// Where the finger last was, and the value it has made, while it drags: relative, the knob
    /// follows the drag as far as it travels.
    @State private var lastX: CGFloat?
    @State private var dragged: Float = 0

    private var both: Bool {
        shown == .both || shown == .follow || shown == .grade
    }

    private var pill: some View {
        let typing = ui.keypad?.id == key
        return Text(verbatim: shownValue(value, shown, most: most))
            .font(.system(size: 16))
            .foregroundStyle(typing ? Color.white : enabled ? StudioInk.label : StudioInk.valueOff)
            .lineLimit(1)
            .fixedSize()
            .padding(.horizontal, 13)
            // as wide as its words need, 13 either side, and never narrower than 73
            .frame(minWidth: 73, minHeight: 28, maxHeight: 28)
            .background(RoundedRectangle(cornerRadius: 14, style: .circular).fill(typing ? Pc.accent : enabled ? StudioInk.pill : StudioInk.pillOff))
            .overlay {
                GeometryReader { g in
                    Color.clear
                        .contentShape(Rectangle())
                        .onTapGesture {
                            guard enabled else { return }
                            ui.keypad = KeypadAsk(
                                id: key, title: label, pill: g.frame(in: .named(studioSpace)),
                                typed: typedNumber(shown, most: most), onValue: onChange, extras: extras
                            )
                        }
                }
            }
    }

    private func track(width w: CGFloat, height h: CGFloat) -> some View {
        let t = CGFloat(min(max(both ? (value + 1) / 2 : value, 0), 1))
        // the knob's middle travels from 5 in at either end, so it overhangs the track by 3
        let x = knobInset + (w - 2 * knobInset) * t
        let y = h / 2
        return ZStack(alignment: .topLeading) {
            Canvas { context, size in
                var line = Path()
                line.move(to: CGPoint(x: 0, y: y))
                line.addLine(to: CGPoint(x: size.width, y: y))
                // a two-way slider's middle: a tick across the track
                if both {
                    var tick = Path()
                    tick.move(to: CGPoint(x: size.width / 2, y: y - 10))
                    tick.addLine(to: CGPoint(x: size.width / 2, y: y + 10))
                    context.stroke(tick, with: .color(StudioInk.track), lineWidth: 1)
                }
                context.stroke(line, with: .color(StudioInk.track), lineWidth: 3)
                if enabled {
                    var part = Path()
                    part.move(to: CGPoint(x: both ? size.width / 2 : 0, y: y))
                    part.addLine(to: CGPoint(x: x, y: y))
                    context.stroke(part, with: .color(Pc.accent), lineWidth: 3)
                }
            }
            Capsule()
                .fill(enabled ? Color.white : StudioInk.knobOff)
                .frame(width: 16, height: 10)
                .offset(x: x - 8, y: y - 5)
        }
        .frame(width: w, height: h, alignment: .topLeading)
    }

    var body: some View {
        VStack(spacing: 0) {
            // the name and value 2 below the top half's middle
            HStack(spacing: 0) {
                // made smaller, to 60%, when it would not fit beside the value
                Text(verbatim: label)
                    .font(.system(size: 17))
                    .foregroundStyle(StudioInk.label)
                    .lineLimit(1)
                    .minimumScaleFactor(0.6)
                    .padding(.trailing, 12)
                Spacer(minLength: 0)
                pill
            }
            .padding(.leading, 10)
            .padding(.trailing, 3.5)
            .frame(height: 28)
            .offset(y: 2)
            GeometryReader { g in
                let w = g.size.width
                track(width: w, height: g.size.height)
                    .contentShape(Rectangle())
                    .gesture(
                        DragGesture(minimumDistance: 0)
                            .onChanged { drag in
                                let last = lastX ?? drag.startLocation.x
                                let from = lastX == nil ? value : dragged
                                let span: Float = both ? 2 : 1
                                let travel = Float(max(w - 2 * knobInset, 1))
                                let next = min(max(from + Float(drag.location.x - last) / travel * span, both ? -1 : 0), 1)
                                lastX = drag.location.x
                                dragged = next
                                if drag.translation != .zero {
                                    onChange(next)
                                }
                            }
                            .onEnded { _ in lastX = nil }
                    )
                    .allowsHitTesting(enabled)
            }
            .frame(height: 20)
            .padding(.leading, 10)
            .padding(.trailing, 10.5)
        }
        .frame(height: 55, alignment: .top)
    }
}

// MARK: the keypad

/// A slider's keypad, asked for from its row.
struct KeypadAsk {
    /// The control's key.
    let id: String
    let title: String
    /// Where the value is on the screen.
    let pill: CGRect
    let typed: TypedNumber
    let onValue: (Float) -> Void
    let extras: KeypadExtras?
}

private let keypadFill = Color(argb: 0xFFD5_D5D5)
private let keypadKeyFill = Color(argb: 0xFFE6_E6E6)
private let keypadInk = Color(argb: 0xFF1D_1D1D)
private let keypadWidth: CGFloat = 275
private let keypadTabs = Color(argb: 0xFFC1_C1C1)
private let keypadTabChosen = Color(argb: 0xFF0A_8CFD)
private let keypadTabInk = Color(argb: 0xFF39_3939)
private let keypadGraph = Color(argb: 0xFFE2_E2E2)
private let keypadGraphEdge = Color(argb: 0xFF99_9999)
private let keypadGraphGrid = Color(argb: 0xFFD0_D0D0)
private let keypadSwitchOff = Color(argb: 0xFFC2_C1C5)

/// A slider keypad's pages: typing the value, the slider's own pressure curve, its own tilt.
private enum KeypadPage {
    case numeric, pressure, tilt
}

/// A slider's keypad, a light popover to the right of its value, its arrow at the value: the
/// slider's name, then 7 8 9 / 4 5 6 / 1 2 3 / 0 and backspace (81 x 66 keys), "-" 0 and backspace
/// for a value that may be negative. Each key sets the value at once: the digits typed so far are
/// the number (beyond the most, the most); "-" makes it negative; backspace takes the last one off
/// (none left: nothing). A tap anywhere else closes it.
struct SliderKeypad: View {
    let ask: KeypadAsk
    let ui: StudioUi
    let height: CGFloat
    let onDismiss: () -> Void

    @State private var page = KeypadPage.numeric
    @State private var text = ""

    /// Tabs over the keys when the slider has a pressure curve or a tilt of its own.
    private var pages: [KeypadPage] {
        var all = [KeypadPage.numeric]
        if ask.extras?.pressure == true {
            all.append(.pressure)
        }
        if ask.extras?.tilt == true {
            all.append(.tilt)
        }
        return all
    }

    /// With pages to choose between, the tabs under the title push the keys down: the popover is
    /// 410 tall with two tabs, 386 with three (as measured).
    private func popoverHeight(_ tabs: Int) -> CGFloat {
        tabs == 1 ? 360 : tabs == 2 ? 410 : 386
    }

    private func enter(_ next: String) {
        // a sign alone waits for its digits
        if next == "-" {
            text = next
            return
        }
        let n = min(max(Int(next) ?? 0, -ask.typed.most), ask.typed.most)
        text = next.isEmpty ? "" : String(n)
        ask.onValue(ask.typed.toValue(n))
    }

    private func key(_ label: String, width: CGFloat, height: CGFloat, action: @escaping () -> Void) -> some View {
        ZStack {
            RoundedRectangle(cornerRadius: 12, style: .circular).fill(keypadKeyFill)
            if label == "⌫" {
                PcIconView(PcIcons.backspace, size: 26, color: keypadInk)
            } else {
                Text(verbatim: label)
                    .font(.system(size: 29))
                    .foregroundStyle(keypadInk)
            }
        }
        .frame(width: width, height: height)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
    }

    private func keys(_ keyHeight: CGFloat) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            ForEach([["7", "8", "9"], ["4", "5", "6"], ["1", "2", "3"]], id: \.self) { row in
                HStack(spacing: 4) {
                    ForEach(row, id: \.self) { k in
                        key(k, width: 81, height: keyHeight) { enter(text + k) }
                    }
                }
            }
            HStack(spacing: 4) {
                if ask.typed.signed {
                    key("-", width: 81, height: keyHeight) {
                        enter(text.hasPrefix("-") ? String(text.dropFirst()) : "-" + text)
                    }
                    key("0", width: 81, height: keyHeight) { enter(text + "0") }
                } else {
                    key("0", width: 166, height: keyHeight) { enter(text + "0") }
                }
                key("⌫", width: 81, height: keyHeight) { enter(String(text.dropLast())) }
            }
        }
        .padding(.leading, 12)
    }

    private var tabRow: some View {
        HStack(spacing: 0) {
            ForEach(pages, id: \.self) { p in
                let icon = p == .numeric ? PcIcons.keypad : p == .pressure ? PcIcons.penPressure : PcIcons.penTilt
                ZStack {
                    RoundedRectangle(cornerRadius: 7, style: .circular).fill(p == page ? keypadTabChosen : Color.clear)
                    PcIconView(icon, size: 30, color: p == page ? .white : keypadTabInk)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .contentShape(Rectangle())
                .onTapGesture { page = p }
            }
        }
        .padding(2)
        .frame(width: 251, height: 30)
        .background(RoundedRectangle(cornerRadius: 9, style: .circular).fill(keypadTabs))
        .padding(.leading, 12)
    }

    var body: some View {
        let tabs = pages.count
        let tall = popoverHeight(tabs)
        let middle = ask.pill.midY
        let top = min(max(middle - tall / 2, 2), max(height - tall - 2, 2))
        ZStack(alignment: .topLeading) {
            Color.clear
                .contentShape(Rectangle())
                .onTapGesture(perform: onDismiss)
            Popover(width: keypadWidth, arrowX: middle - top, fill: keypadFill, corner: 22, side: .left) {
                VStack(alignment: .leading, spacing: 0) {
                    Text(verbatim: ask.title)
                        .font(.system(size: 22, weight: .bold))
                        .foregroundStyle(keypadInk)
                        .lineLimit(1)
                        .frame(height: tabs > 1 ? 43 : 51, alignment: .top)
                        .padding(.leading, 25)
                        .padding(.top, 19)
                    if tabs > 1 {
                        tabRow
                        Color.clear.frame(height: 12)
                    }
                    switch page {
                    case .numeric:
                        keys(tabs > 1 ? 64 : 66)
                    case .pressure:
                        if let extras = ask.extras {
                            KeypadPressure(extras: extras, ui: ui)
                        }
                    case .tilt:
                        if let extras = ask.extras {
                            KeypadTilt(extras: extras)
                        }
                    }
                }
                .frame(width: keypadWidth, height: tall, alignment: .topLeading)
            }
            // the arrow's tip a little into the pill, the body 3 past its end
            .offset(x: ask.pill.maxX - (Pc.arrowHeight - 3), y: top)
        }
    }
}

/// A page's name and its switch, in a row across the keypad.
private struct KeypadSwitchRow: View {
    let label: String
    let on: Bool
    let onChange: (Bool) -> Void

    var body: some View {
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 17))
                .foregroundStyle(keypadInk)
                .lineLimit(1)
            Spacer(minLength: 0)
            PcSwitch(on: on, offTrack: keypadSwitchOff)
        }
        .frame(height: 28)
        .contentShape(Rectangle())
        .onTapGesture { onChange(!on) }
        .padding(.leading, 18)
        .padding(.trailing, 8)
    }
}

/// The pressure page: the slider's own curve, on or off, drawn on a light grid.
private struct KeypadPressure: View {
    let extras: KeypadExtras
    let ui: StudioUi
    @State private var curve: [CGPoint]?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Color.clear.frame(height: 6)
            KeypadSwitchRow(label: "Pressure", on: curve != nil) { on in
                let next: [CGPoint]? = on ? [.zero, CGPoint(x: 1, y: 1)] : nil
                curve = next
                extras.setCurve(next)
            }
            Color.clear.frame(height: 12)
            PressureCurve(
                points: curve ?? [],
                grid: keypadGraphGrid,
                edge: keypadGraphEdge,
                fill: keypadGraph,
                line: Pc.accent,
                ui: ui
            ) { points in
                if curve != nil {
                    curve = points
                    extras.setCurve(points)
                }
            }
            .frame(width: 239, height: 219)
            .padding(.leading, 18)
        }
        .onAppear { curve = extras.curve() }
    }
}

/// The tilt page: the slider's own tilt switch (if it has one) over the angle its tilt starts from.
private struct KeypadTilt: View {
    let extras: KeypadExtras
    @State private var on: Bool?
    @State private var angle: Float = 0

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Color.clear.frame(height: 6)
            if let on {
                KeypadSwitchRow(label: "Tilt", on: on) {
                    self.on = $0
                    extras.setTiltOn($0)
                }
            }
            Color.clear.frame(height: 8)
            TiltAngle(value: angle, look: .light, active: on ?? true) {
                angle = $0
                extras.setTiltAngle($0)
            }
            .padding(5)
            .frame(width: 255, height: 209)
            .background(keypadFill)
            .padding(.leading, 10)
        }
        .onAppear {
            on = extras.tiltOn()
            angle = extras.tiltAngle()
        }
    }
}

// MARK: the graphs

/// The taper bar: a box inset as far as the sliders' knobs travel, the bar across it thinning to
/// points at its ends as the start and end handles (on lines across the box) move in, each up to
/// half way.
private struct TaperBar: View {
    let start: Float
    let end: Float
    let onStart: (Float) -> Void
    let onEnd: (Float) -> Void

    /// Which handle the touch took: the nearer.
    @State private var grabbedStart: Bool?

    var body: some View {
        GeometryReader { g in
            let w = g.size.width
            let h = g.size.height
            let half = (w - 2 * taperTravel) / 2
            let startX = taperTravel + CGFloat(start) * half
            let endX = w - taperTravel - CGFloat(end) * half
            Canvas { context, size in
                let left = taperTravel - 1
                let right = size.width - left
                let y = size.height / 2
                let box = CGRect(x: left, y: 0, width: right - left, height: size.height)
                context.fill(Path(roundedRect: box, cornerRadius: 5), with: .color(themed(0xFF21_2121, 0xFFEB_EBEB)))
                var bar = Path()
                bar.move(to: CGPoint(x: left, y: y))
                bar.addLine(to: CGPoint(x: startX, y: y - 7))
                bar.addLine(to: CGPoint(x: endX, y: y - 7))
                bar.addLine(to: CGPoint(x: right, y: y))
                bar.addLine(to: CGPoint(x: endX, y: y + 7))
                bar.addLine(to: CGPoint(x: startX, y: y + 7))
                bar.closeSubpath()
                context.fill(bar, with: .color(themed(0xFF36_3636, 0xFFDC_DCDC)))
                for x in [startX, endX] {
                    var line = Path()
                    line.move(to: CGPoint(x: x, y: 0))
                    line.addLine(to: CGPoint(x: x, y: size.height))
                    context.stroke(line, with: .color(.white), lineWidth: 2)
                    context.fill(Path(ellipseIn: CGRect(x: x - 6.5, y: y - 6.5, width: 13, height: 13)), with: .color(.white))
                    context.fill(Path(ellipseIn: CGRect(x: x - 4, y: y - 4, width: 8, height: 8)), with: .color(Pc.accent))
                }
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        let takesStart = grabbedStart ?? (abs(drag.startLocation.x - startX) <= abs(drag.startLocation.x - endX))
                        grabbedStart = takesStart
                        guard drag.translation != .zero else { return }
                        let x = drag.location.x
                        if takesStart {
                            onStart(Float(min(max((x - taperTravel) / half, 0), 1)))
                        } else {
                            onEnd(Float(min(max((w - taperTravel - x) / half, 0), 1)))
                        }
                    }
                    .onEnded { _ in grabbedStart = nil }
            )
            .frame(width: w, height: h)
        }
        .frame(height: 72)
    }
}

/// How the tilt angle's graph is coloured: its box, the quarter disc, the arc, the line, the
/// angle's label, and its handle's middle while the tilt is off.
struct TiltLook {
    let box: Color
    let quarter: Color
    let arc: Color
    let line: Color
    let label: Color
    let off: Color
    var fill = true

    static let dark = TiltLook(
        box: Color(argb: 0xFF1B_1B1B), quarter: Color(argb: 0xFF26_2626), arc: Color(argb: 0xFF4A_4A4A),
        line: Color(argb: 0xFF6E_6E6E), label: Color(argb: 0xFF8E_8E8E), off: Color(argb: 0xFF6E_6E6E)
    )
    /// In the keypads: the disc darker than the box round it, as measured.
    static let light = TiltLook(
        box: Color(argb: 0xFFE6_E6E6), quarter: Color(argb: 0xFFDA_DADA), arc: Color(argb: 0xFFAB_ABAB),
        line: Color(argb: 0xFF79_7979), label: Color(argb: 0xFFAA_AAAA), off: Color(argb: 0xFFAA_AAAA), fill: false
    )
}

/// The tilt angle: a quarter disc from the bottom-left corner with a line at the angle (0 flat, 1
/// upright) and its handle on the arc; drag anywhere to turn it.
struct TiltAngle: View {
    let value: Float
    var look = TiltLook.dark
    var active = true
    let onChange: (Float) -> Void

    private func graph(_ size: CGSize) -> some View {
        let r = size.height
        let a = Double(min(max(value, 0), 1)) * .pi / 2
        let dir = CGPoint(x: cos(a), y: -sin(a))
        let handle = CGPoint(x: dir.x * r, y: size.height + dir.y * r)
        return ZStack(alignment: .topLeading) {
            Canvas { context, size in
                let corner = CGPoint(x: 0, y: size.height)
                let disc = Path(ellipseIn: CGRect(x: corner.x - r, y: corner.y - r, width: 2 * r, height: 2 * r))
                context.fill(disc, with: .color(look.quarter))
                context.stroke(disc, with: .color(look.arc), lineWidth: 1)
                // the line runs on to the box's edge; the handle sits on the arc
                let across = dir.x > 1e-4 ? size.width / dir.x : .greatestFiniteMagnitude
                let up = dir.y < -1e-4 ? size.height / -dir.y : .greatestFiniteMagnitude
                let reach = min(across, up)
                var line = Path()
                line.move(to: corner)
                line.addLine(to: CGPoint(x: corner.x + dir.x * reach, y: corner.y + dir.y * reach))
                context.stroke(line, with: .color(look.line), lineWidth: 1)
            }
            // not cut by the box: the handle sits on its edge, whole
            Circle().fill(Color.white)
                .frame(width: 11, height: 11)
                .offset(x: handle.x - 5.5, y: handle.y - 5.5)
            Circle().fill(active ? Pc.accent : look.off)
                .frame(width: 7, height: 7)
                .offset(x: handle.x - 3.5, y: handle.y - 3.5)
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
        .contentShape(Rectangle())
        .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
            // the angle of the touch from the bottom-left corner, a quarter turn at most
            let turned = atan2(Double(size.height - drag.location.y), Double(drag.location.x)) / (.pi / 2)
            onChange(Float(min(max(turned, 0), 1)))
        })
    }

    private var reading: some View {
        Text(verbatim: "\(wholeNumber(min(max(value, 0), 1) * 90))°")
            .font(.system(size: 17))
            .foregroundStyle(look.label)
            .padding(.top, 7)
            .padding(.trailing, 9)
            .frame(maxWidth: .infinity, alignment: .trailing)
            .allowsHitTesting(false)
    }

    var body: some View {
        if look.fill {
            GeometryReader { g in
                graph(g.size)
            }
            .aspectRatio(271.5 / 221, contentMode: .fit)
            .background(RoundedRectangle(cornerRadius: 6, style: .circular).fill(look.box))
            .overlay(alignment: .top) { reading }
            .padding(.horizontal, 5)
        } else {
            GeometryReader { g in
                graph(g.size)
            }
            .background(look.box)
            .overlay(alignment: .top) { reading }
        }
    }
}

/// The shape's roundness and angle: an ellipse on crossed lines, its blue handles (the ends of its
/// short axis) dragged in and out for the roundness, its green handle (the end of its long axis)
/// dragged round for the angle, shown in degrees.
private struct RoundnessGraph: View {
    let roundness: Float
    let angle: Float
    let onRoundness: (Float) -> Void
    let onAngle: (Float) -> Void

    /// What the touch took hold of: turning, or squashing along the short axis.
    private struct Grab {
        let turning: Bool
        let short: CGPoint
    }

    @State private var grab: Grab?

    /// A handle: a white ring round `colour`.
    private func handle(_ context: GraphicsContext, _ at: CGPoint, _ colour: Color) {
        for (radius, ink) in [(CGFloat(5.6), Color.white), (CGFloat(3.7), colour)] {
            let dot = CGRect(x: at.x - radius, y: at.y - radius, width: radius * 2, height: radius * 2)
            context.fill(Path(ellipseIn: dot), with: .color(ink))
        }
    }

    var body: some View {
        let round = CGFloat(min(max(roundness, 0.05), 1))
        let part = min(max(angle, 0), 1)
        let turn = CGFloat(part)
        GeometryReader { g in
            let size = g.size
            let c = CGPoint(x: size.width / 2, y: size.height / 2)
            let r = size.height * 78 / 231.5
            let rad = turn * 2 * .pi
            let long = CGPoint(x: cos(rad), y: -sin(rad))
            let short = CGPoint(x: -long.y, y: long.x)
            Canvas { context, size in
                // the crossed lines show dimly through the ellipse's fill
                var cross = Path()
                cross.move(to: CGPoint(x: 0, y: c.y))
                cross.addLine(to: CGPoint(x: size.width, y: c.y))
                cross.move(to: CGPoint(x: c.x, y: 0))
                cross.addLine(to: CGPoint(x: c.x, y: size.height))
                context.stroke(cross, with: .color(themed(0xFF4C_4C4C, 0xFFC0_C0C0)), lineWidth: 1)
                let oval = Path(ellipseIn: CGRect(x: c.x - r, y: c.y - r * round, width: 2 * r, height: 2 * r * round))
                    .applying(
                        CGAffineTransform(translationX: c.x, y: c.y)
                            .rotated(by: -rad)
                            .translatedBy(x: -c.x, y: -c.y)
                    )
                context.fill(oval, with: .color(themed(0xD623_2323, 0xD6F0_F0F0)))
                context.stroke(oval, with: .color(themed(0xFF56_5656, 0xFFC8_C8C8)), lineWidth: 1)
                handle(context, CGPoint(x: c.x + short.x * r * round, y: c.y + short.y * r * round), Pc.accent)
                handle(context, CGPoint(x: c.x - short.x * r * round, y: c.y - short.y * r * round), Pc.accent)
                handle(context, CGPoint(x: c.x + long.x * r, y: c.y + long.y * r), Color(argb: 0xFF42_E347))
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        // the green handle turns it; anything else squashes it
                        let green = CGPoint(x: c.x + long.x * r, y: c.y + long.y * r)
                        let held = grab ?? Grab(
                            turning: hypot(drag.startLocation.x - green.x, drag.startLocation.y - green.y) < 28,
                            short: short
                        )
                        grab = held
                        guard drag.translation != .zero else { return }
                        let dx = drag.location.x - c.x
                        let dy = drag.location.y - c.y
                        if held.turning {
                            let t = atan2(-dy, dx) / (2 * .pi)
                            onAngle(Float(t < 0 ? t + 1 : t))
                        } else {
                            let along = abs(dx * held.short.x + dy * held.short.y)
                            onRoundness(Float(min(max(along / r, 0.05), 1)))
                        }
                    }
                    .onEnded { _ in grab = nil }
            )
        }
        .aspectRatio(283 / 231.5, contentMode: .fit)
        .background(themed(0xFF1B_1B1B, 0xFFFF_FFFF))
        .overlay(alignment: .top) {
            Text(verbatim: "\(wholeNumber(part * 360) % 360)°")
                .font(.system(size: 17))
                .foregroundStyle(themed(0xFF8E_8E8E, 0xFF80_8080))
                .padding(.top, 12)
                .padding(.trailing, 14)
                .frame(maxWidth: .infinity, alignment: .trailing)
                .allowsHitTesting(false)
        }
        .clipShape(RoundedRectangle(cornerRadius: 6, style: .circular))
    }
}

/// A pressure curve on a grid: a touch off the curve adds a node there, dragged it moves between
/// its neighbours, and a tap on a node offers what can be done with it.
struct PressureCurve: View {
    let points: [CGPoint]
    var grid = Color(argb: 0xFF3A_3A3A)
    var edge: Color?
    var fill = Color.clear
    var corner: CGFloat = 0
    var line = Color.white
    var lineWidth: CGFloat = 1.5
    /// The handles' radii.
    var ring: CGFloat = 7
    var middle: CGFloat = 5
    let ui: StudioUi
    let onChange: ([CGPoint]) -> Void

    /// A touch on the curve: the nodes as it has made them, which it holds, whether it added that
    /// one, and whether it has moved.
    private struct Hold {
        var list: [CGPoint]
        let index: Int
        let added: Bool
        var moved = false
    }

    @State private var hold: Hold?

    private func toCurve(_ p: CGPoint, _ size: CGSize) -> CGPoint {
        CGPoint(x: min(max(p.x / size.width, 0), 1), y: min(max(1 - p.y / size.height, 0), 1))
    }

    private func touched(_ drag: DragGesture.Value, _ size: CGSize) {
        ui.nodeMenu = nil
        // no curve (switched off): nothing to touch
        guard !points.isEmpty else { return }
        if hold == nil {
            let at = toCurve(drag.startLocation, size)
            var list = points
            var index = list.firstIndex { hypot($0.x - at.x, $0.y - at.y) < 0.05 }
            let added = index == nil
            if added {
                list.append(at)
                list.sort { $0.x < $1.x }
                index = list.firstIndex(of: at)
                onChange(list)
            }
            hold = Hold(list: list, index: index ?? 0, added: added)
        }
        guard var held = hold else { return }
        if !held.moved, hypot(drag.translation.width, drag.translation.height) < touchSlop {
            return
        }
        held.moved = true
        let p = toCurve(drag.location, size)
        let i = held.index
        let lo = i > 0 ? held.list[i - 1].x : 0
        let hi = i + 1 < held.list.count ? held.list[i + 1].x : 1
        held.list[i] = CGPoint(x: min(max(p.x, lo), hi), y: p.y)
        hold = held
        onChange(held.list)
    }

    /// A tap on a node shows what can be done with it.
    private func letGo(_ box: CGRect) {
        defer { hold = nil }
        guard let held = hold, !held.moved, !held.added else { return }
        let sorted = points.sorted { $0.x < $1.x }
        ui.nodeMenu = NodeMenuAsk(points: sorted, index: held.index, box: box, onChange: onChange)
    }

    var body: some View {
        let flat = points.flatMap { [Float($0.x), Float($0.y)] }
        let samples = points.isEmpty ? [] : curveSamples(points: flat, count: 129)
        GeometryReader { g in
            let size = g.size
            ZStack(alignment: .topLeading) {
                Canvas { context, size in
                    let w = size.width
                    let h = size.height
                    let box = Path(roundedRect: CGRect(origin: .zero, size: size), cornerRadius: corner)
                    context.fill(box, with: .color(fill))
                    var lines = Path()
                    for k in 1 ... 3 {
                        let part = CGFloat(k) / 4
                        lines.move(to: CGPoint(x: w * part, y: 0))
                        lines.addLine(to: CGPoint(x: w * part, y: h))
                        lines.move(to: CGPoint(x: 0, y: h * part))
                        lines.addLine(to: CGPoint(x: w, y: h * part))
                    }
                    context.stroke(lines, with: .color(grid), lineWidth: 1)
                    context.stroke(box, with: .color(edge ?? grid), lineWidth: 1)
                    guard !samples.isEmpty else { return }
                    var path = Path()
                    let last = CGFloat(max(samples.count - 1, 1))
                    for (i, v) in samples.enumerated() {
                        let at = CGPoint(x: w * CGFloat(i) / last, y: (1 - CGFloat(v)) * h)
                        if i == 0 {
                            path.move(to: at)
                        } else {
                            path.addLine(to: at)
                        }
                    }
                    context.stroke(path, with: .color(line), style: StrokeStyle(lineWidth: lineWidth, lineCap: .round))
                }
                // not cut by the box: the ends' handles sit on its corners, whole
                ForEach(Array(points.enumerated()), id: \.offset) { _, p in
                    let x = p.x * size.width
                    let y = (1 - p.y) * size.height
                    Circle().fill(Color.white)
                        .frame(width: ring * 2, height: ring * 2)
                        .offset(x: x - ring, y: y - ring)
                    Circle().fill(Pc.accent)
                        .frame(width: middle * 2, height: middle * 2)
                        .offset(x: x - middle, y: y - middle)
                }
            }
            .frame(width: size.width, height: size.height, alignment: .topLeading)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { touched($0, size) }
                    .onEnded { _ in letGo(g.frame(in: .named(studioSpace))) }
            )
        }
    }
}

/// The menu over a curve's node, asked for from its graph.
struct NodeMenuAsk {
    let points: [CGPoint]
    let index: Int
    /// The graph, on the screen.
    let box: CGRect
    let onChange: ([CGPoint]) -> Void
}

/// The menu over a node of a curve: Delete (a node between the ends) and Reset (the curve straight
/// again), centred over the node, its bottom 11 above it.
struct NodeMenu: View {
    let ask: NodeMenuAsk
    let onClose: () -> Void

    private let width: CGFloat = 70
    private let rowHeight: CGFloat = 36

    var body: some View {
        let points = ask.points
        let index = min(max(ask.index, 0), max(points.count - 1, 0))
        let end = index == 0 || index == points.count - 1
        let rows = end ? ["Reset"] : ["Delete", "Reset"]
        let at = points.isEmpty ? CGPoint.zero : points[index]
        let x = ask.box.minX + at.x * ask.box.width - width / 2
        let y = ask.box.minY + (1 - at.y) * ask.box.height - 11 - rowHeight * CGFloat(rows.count)
        ZStack(alignment: .topLeading) {
            Color.clear
                .contentShape(Rectangle())
                .onTapGesture(perform: onClose)
            VStack(spacing: 0) {
                ForEach(rows, id: \.self) { row in
                    Text(verbatim: row)
                        .font(.system(size: 17))
                        .foregroundStyle(Color.white)
                        .lineLimit(1)
                        .frame(width: width, height: rowHeight)
                        .contentShape(Rectangle())
                        .onTapGesture {
                            onClose()
                            if row == "Reset" {
                                ask.onChange([.zero, CGPoint(x: 1, y: 1)])
                            } else {
                                var left = points
                                left.remove(at: index)
                                ask.onChange(left)
                            }
                        }
                }
            }
            .background(RoundedRectangle(cornerRadius: 12, style: .circular).fill(Color(argb: 0xFF10_1010)))
            .shadow(color: .black, radius: 7)
            .offset(x: x, y: y)
        }
    }
}

// MARK: blend modes

/// The list of blend modes, asked for from its row.
struct BlendListAsk {
    let modes: [BlendModeInfo]
    /// The mode set now, by the engine's number for it.
    let current: Int
    /// Where the row's middle is on the screen.
    let centreY: CGFloat
    let onPick: (Int) -> Void
}

/// A blend mode setting: its mode's name, in blue, opens the modes as a list over it (the grain's
/// own list for the grain, the layers' and behind for the brush and its burnt edges).
private struct BlendPicker: View {
    let model: CanvasModel
    let ui: StudioUi
    let key: String
    let label: String

    var body: some View {
        let studio = model.studio
        let current = wholeNumber(studio.value(key))
        let modes = key == "grain_blend" ? studio.grainModes : studio.brushModes
        // a file may give the grain a layer mode the list leaves out
        let name = (modes + model.blendModeList).first { Int($0.id) == current }?.name ?? "Normal"
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 17))
                .foregroundStyle(StudioInk.dim)
                .lineLimit(1)
            Spacer(minLength: 0)
            Text(verbatim: name)
                .font(.system(size: 17))
                .foregroundStyle(Pc.accent)
                .lineLimit(1)
                .overlay {
                    GeometryReader { g in
                        Color.clear
                            .contentShape(Rectangle())
                            .onTapGesture {
                                ui.blendList = BlendListAsk(
                                    modes: modes, current: current, centreY: g.frame(in: .named(studioSpace)).midY
                                ) { studio.set(key, Float($0)) }
                            }
                    }
                }
        }
        .padding(.horizontal, 8)
        .frame(height: 47)
        .offset(y: 2)
    }
}

/// The blend modes in the layer panel's order, as a light list 10 from the screen's right with the
/// current one over the row it opened from; a tap picks one, a tap outside it closes it.
struct BlendModeList: View {
    let ask: BlendListAsk
    let height: CGFloat
    let onDone: (Int?) -> Void

    private let rowHeight: CGFloat = 38
    private let listWidth: CGFloat = 250
    /// At most this many rows show at once; the rest scroll.
    private let rowsShown = 11
    private let ink = Color(argb: 0xFF39_3939)
    private let glyphInk = Color(argb: 0xFF7F_7F7F)

    var body: some View {
        let modes = ask.modes
        let chosen = max(modes.firstIndex { Int($0.id) == ask.current } ?? 0, 0)
        let shown = min(rowsShown, modes.count)
        let first = min(max(chosen - shown / 2, 0), max(modes.count - shown, 0))
        let tall = rowHeight * CGFloat(shown) + CGFloat(max(shown - 1, 0))
        let top = ask.centreY - (CGFloat(chosen - first) + 0.5) * (rowHeight + 1)
        let y = min(max(top, 10), max(height - 10 - tall, 10))
        let shape = RoundedRectangle(cornerRadius: 13, style: .circular)
        ZStack(alignment: .topTrailing) {
            Color.clear
                .contentShape(Rectangle())
                .onTapGesture { onDone(nil) }
            ScrollViewReader { reader in
                ScrollView {
                    VStack(spacing: 0) {
                        ForEach(Array(modes.enumerated()), id: \.offset) { i, m in
                            let picked = i == chosen
                            if i > 0 {
                                Color(argb: 0xFFCD_CDCD).frame(height: 1)
                            }
                            HStack(spacing: 0) {
                                Text(verbatim: m.name)
                                    .font(.system(size: 17))
                                    .foregroundStyle(picked ? Color.white : ink)
                                    .lineLimit(1)
                                Spacer(minLength: 0)
                                // its 26 frame centred 31 in from the right
                                if let glyph = PcIcons.blendGlyphs.first(where: { $0.0 == m.name })?.1 {
                                    PcIconView(glyph, size: 26, color: picked ? .white : glyphInk)
                                }
                            }
                            .padding(.leading, 12)
                            .padding(.trailing, 18)
                            .frame(height: rowHeight)
                            .background(picked ? Pc.rowSelected : Color.clear)
                            .contentShape(Rectangle())
                            .onTapGesture { onDone(Int(m.id)) }
                            .id(i)
                        }
                    }
                }
                .scrollIndicators(.hidden)
                .onAppear { reader.scrollTo(first, anchor: .top) }
            }
            .frame(width: listWidth, height: tall)
            .background(shape.fill(Color(argb: 0xFFE1_E1E1)))
            .clipShape(shape)
            .shadow(color: .black.opacity(0.35), radius: 12)
            .padding(.trailing, 10)
            .offset(y: y)
        }
    }
}

// MARK: sources, cards and the About page

/// Whether this is a material's source (white all over when it has no image).
private func isMaterial(_ kind: SourceKind) -> Bool {
    kind == .metallic || kind == .roughness
}

/// Portrait: a source as a row with its image: the shape white (the round tip a white disc), the
/// grain as its greys, a material's amount all over as white.
private struct SourceRow: View {
    let studio: StudioModel
    let label: String
    let kind: SourceKind

    var body: some View {
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 21, weight: .bold))
                .foregroundStyle(StudioInk.header)
                .lineLimit(1)
            Spacer(minLength: 0)
            ZStack {
                if let image = studio.sources[kind] {
                    Image(uiImage: image)
                        .resizable()
                        .scaledToFit()
                } else if isMaterial(kind) {
                    Color.white
                } else {
                    Circle().fill(Color.white)
                }
            }
            .frame(width: 44, height: 44)
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .circular))
        }
        .padding(.leading, 16)
        .padding(.trailing, 8)
        .frame(height: 60)
        .background(RoundedRectangle(cornerRadius: 12, style: .circular).fill(StudioInk.card))
        .contentShape(Rectangle())
        .onTapGesture { studio.sourceEditing = kind }
    }
}

/// Landscape: a source as a title with an Edit button, over a square of its image as wide as the
/// rows (the shape white on dark, the grain as its greys, a material's amount all over white).
private struct SourceBox: View {
    let studio: StudioModel
    let label: String
    let kind: SourceKind

    var body: some View {
        let image = studio.sourcesBig[kind]
        let white = image == nil && isMaterial(kind)
        let shape = RoundedRectangle(cornerRadius: 22, style: .circular)
        VStack(spacing: 0) {
            ZStack(alignment: .topLeading) {
                // where a section's title sits
                Text(verbatim: label)
                    .font(.system(size: 21, weight: .bold))
                    .foregroundStyle(StudioInk.header)
                    .lineLimit(1)
                    .padding(.leading, 8)
                    .padding(.top, 9.5)
                Text(verbatim: "Edit")
                    .font(.system(size: 17))
                    .foregroundStyle(StudioInk.buttonInk)
                    .lineLimit(1)
                    .frame(width: 54, height: 45)
                    .background(Capsule().fill(StudioInk.button))
                    .contentShape(Capsule())
                    .onTapGesture { studio.sourceEditing = kind }
                    .offset(x: 4)
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
            .frame(height: 45)
            .padding(.top, 8.5)
            Color.clear.frame(height: 12.5)
            GeometryReader { g in
                ZStack {
                    white ? Color.white : StudioInk.button
                    if let image {
                        Image(uiImage: image)
                            .resizable()
                            .scaledToFit()
                    } else if !white {
                        // the round tip: a white disc
                        Circle().fill(Color.white)
                            .frame(width: g.size.width * 0.7, height: g.size.width * 0.7)
                    }
                }
            }
            .aspectRatio(1, contentMode: .fit)
            .clipShape(shape)
            .contentShape(shape)
            .onTapGesture { studio.sourceEditing = kind }
        }
    }
}

/// A card titled `label` that a tap on its title opens, its chevron turned down, to show its
/// controls inside it, 8 in from its sides and spaced as a page's are; the page opens with it
/// closed.
private struct DisclosureCard: View {
    let model: CanvasModel
    let ui: StudioUi
    let label: String
    let controls: [StudioControl]
    let landscape: Bool

    @State private var open = false

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                Text(verbatim: label)
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(StudioInk.materialInk)
                    .lineLimit(1)
                Spacer(minLength: 0)
                PcIconView(PcIcons.chevronRight, size: 16, color: .white)
                    .rotationEffect(.degrees(open ? 90 : 0))
                    .frame(width: 30, height: 30)
                    .background(Circle().fill(StudioInk.back))
            }
            .padding(.horizontal, 16)
            .frame(height: 60)
            .contentShape(Rectangle())
            .onTapGesture { open.toggle() }
            if open {
                VStack(spacing: 0) {
                    ForEach(Array(controls.enumerated()), id: \.offset) { i, control in
                        if i > 0 {
                            Color.clear.frame(height: studioGap(.control(controls[i - 1]), .control(control)))
                        }
                        StudioControlView(model: model, ui: ui, control: control, landscape: landscape)
                    }
                }
                .padding(.horizontal, 8)
                .padding(.top, 12)
                .padding(.bottom, 8)
            }
        }
        .background(RoundedRectangle(cornerRadius: 20, style: .circular).fill(StudioInk.material))
    }
}

/// The brush's stroke filling the card's height, its name over it on a patch of the card's own
/// colour.
private struct PreviewCard: View {
    let studio: StudioModel
    let landscape: Bool

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 12, style: .circular)
        ZStack(alignment: .topLeading) {
            StudioInk.previewFill
            if let preview = studio.preview {
                SampleStroke(image: preview)
            }
            Text(verbatim: studio.name)
                .font(.system(size: 20, weight: .bold))
                .foregroundStyle(StudioInk.previewName)
                .lineLimit(1)
                .fixedSize()
                .padding(.horizontal, 17)
                .frame(height: 37)
                .background(RoundedRectangle(cornerRadius: 10, style: .circular).fill(StudioInk.previewFill))
                .padding(.leading, 15.5)
                .padding(.top, 14.5)
        }
        .aspectRatio(landscape ? 278.5 / 91.5 : 623.0 / 200, contentMode: .fit)
        .clipShape(shape)
        .overlay(shape.strokeBorder(StudioInk.previewEdge, lineWidth: 1))
    }
}

/// A brush's About page: a card with its maker's picture, name and signature (ProCraft's own for
/// ours), then making a new reset point and resetting to it, with when it was made.
private struct AboutBrushView: View {
    let studio: StudioModel

    /// The short date and time with a comma between.
    private func when(_ seconds: Int64) -> String {
        let format = DateFormatter()
        format.dateStyle = .short
        format.timeStyle = .short
        return format.string(from: Date(timeIntervalSince1970: TimeInterval(seconds)))
    }

    var body: some View {
        let about = studio.about
        let author = about?.author.trimmingCharacters(in: .whitespaces) ?? ""
        VStack(spacing: 0) {
            VStack(spacing: 0) {
                Color.clear.frame(height: 30)
                ZStack {
                    Circle().fill(StudioInk.aboutFace)
                    if let picture = about?.picture {
                        Image(uiImage: picture)
                            .resizable()
                            .scaledToFill()
                    } else {
                        PcIconView(PcIcons.brush, size: 56, color: StudioInk.dim)
                    }
                }
                .frame(width: 96, height: 96)
                .clipShape(Circle())
                Color.clear.frame(height: 21)
                Text(verbatim: "Made by \(author.isEmpty ? "ProCraft" : author)")
                    .font(.system(size: 20))
                    .foregroundStyle(StudioInk.aboutName)
                    .lineLimit(1)
                Color.clear.frame(height: 25)
                // the signature in grey, whatever its own colour
                if let signature = about?.signature {
                    Image(uiImage: signature)
                        .renderingMode(.template)
                        .resizable()
                        .scaledToFit()
                        .foregroundStyle(StudioInk.aboutSignature)
                        .frame(width: 300, height: 64)
                }
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity)
            .frame(height: 295)
            .background(RoundedRectangle(cornerRadius: 20, style: .circular).fill(StudioInk.aboutCard))
            Color.clear.frame(height: 25)
            Text(verbatim: "Create new reset point")
                .font(.system(size: 17))
                .foregroundStyle(StudioInk.aboutRowInk)
                .lineLimit(1)
                .padding(.leading, 15)
                .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 22, style: .circular).fill(StudioInk.aboutCard))
                .contentShape(Rectangle())
                .onTapGesture { studio.newResetPoint() }
            Color.clear.frame(height: 11)
            VStack(alignment: .leading, spacing: 4) {
                Text(verbatim: "Reset brush")
                    .font(.system(size: 17))
                    .foregroundStyle(StudioInk.aboutRowInk)
                    .lineLimit(1)
                if let made = about?.resetMade {
                    HStack(spacing: 6) {
                        PcIconView(PcIcons.menuTick, size: 20, color: .white)
                            .frame(width: 17, height: 17)
                            .background(Circle().fill(Pc.accent))
                        Text(verbatim: "Last reset point: \(when(made))")
                            .font(.system(size: 13))
                            .foregroundStyle(StudioInk.aboutRowInk)
                            .lineLimit(1)
                    }
                }
            }
            .padding(.leading, 15)
            .frame(maxWidth: .infinity, minHeight: about?.resetMade != nil ? 69 : 44, maxHeight: about?.resetMade != nil ? 69 : 44, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 22, style: .circular).fill(StudioInk.aboutCard))
            .contentShape(Rectangle())
            .onTapGesture { studio.reset() }
        }
    }
}
