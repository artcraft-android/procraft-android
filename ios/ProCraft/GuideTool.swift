import SwiftUI

private func themed(_ dark: UInt32, _ light: UInt32) -> Color {
    Color(argb: Look.shared.light ? light : dark)
}

private var guideTop: Color { themed(0xFF1F_1F1F, 0xFFE1_E1E3) }
private var guideButton: Color { themed(0xFF1C_1C1C, 0xFFF5_F8F9) }
private var guideButtonEdge: Color { themed(0xFF38_3838, 0xFFDC_DEDF) }
private var guideTitle: Color { themed(0xFFD8_D8D8, 0xFF3A_3A3A) }
private var guideNote: Color { themed(0xFF4A_4A4A, 0xFFB5_B5B5) }
/// The toolbar's words: light, or dark grey with the light interface.
private var guideLabel: Color { themed(0xFFD0_D0D0, 0xFF41_4141) }
private var panelFill: Color { themed(0xF71E_1E1E, 0xF7F1_F1F1) }
private let guideDone = Color(argb: 0xFF01_90FF)
private let nodeCentre = Color(argb: 0xFF0A_84FF)
private let nodeTurn = Color(argb: 0xFF30_D158)
private let optionsFill = Color(argb: 0xFF30_3030)

// the grid size's keypad, frosted as measured; light, as Custom Canvas's
private var sheetFill: Color { themed(0xE91A_1A1A, 0xFFD1_D3D8) }
private var sheetKey: Color { themed(0x514E_4E4E, 0xFFAB_B2BB) }
private var sheetDigit: Color { themed(0x9A75_7575, 0xFFFF_FFFF) }
private var sheetUnit: Color { themed(0xFFA0_A0A1, 0xFF81_8C96) }
private var sheetUnitInk: Color { Look.shared.light ? .white : Color(argb: 0xFF14_1414) }
private var sheetInk: Color { themed(0xFFAA_AAAA, 0xFF3E_3E3E) }
/// The keypad's keys, 79.5 tall, 9 apart; 30 between the units, the digits and the right-hand keys.
private let keyHeight: CGFloat = 79.5
private let keyGap: CGFloat = 9
private let keyGroups: CGFloat = 30
/// The sheet: the keys from 51 below its top to 38 above the screen's foot.
private let sheetHeight = 51 + keyHeight * 4 + keyGap * 3 + 38

/// Where the canvas fits while the guide is edited: a fifth of the screen's height above and below,
/// 40 in from the sides.
private let fitBand: CGFloat = 0.2
private let fitSide: CGFloat = 40
/// How near a touch takes a handle, points.
private let handleReach: CGFloat = 30

/// The colour bar's stops, as the engine's (dark through blue, green, orange, red, pink, white).
private let barStops: [UInt32] = [
    0xFF05_040A, 0xFF01_0C2C, 0xFF00_1857, 0xFF08_3984, 0xFF12_8BB6, 0xFF19_C6CB, 0xFF1B_D0B8, 0xFF2F_D083,
    0xFF67_C064, 0xFFBC_9B4C, 0xFFCE_9049, 0xFFD9_7644, 0xFFDA_574A, 0xFFDC_414D, 0xFFE5_3D71, 0xFFEC_36AB,
    0xFFF5_34DE, 0xFFF5_5CF9, 0xFFFB_92FD, 0xFFFC_CAFF, 0xFFE8_E1E9,
]

private let units: [(unit: GuideUnit, label: String)] = [
    (.millimeters, "Millimeters"), (.centimeters, "Centimeters"), (.inches, "Inches"), (.pixels, "Pixels"),
]

/// A grid size slider's position for a size, and back (canvas pixels, 10 to 500).
private func sizeToSlider(_ px: Float) -> Float {
    (min(max((px - 10) / 490, 0), 1)).squareRoot()
}

private func sliderToSize(_ v: Float) -> Float {
    10 + v * v * 490
}

/// The drawing guide as the core has it, and its editor.
@Observable
final class GuideTool {
    @ObservationIgnored private unowned let model: CanvasModel

    private(set) var info: GuideInfo
    private(set) var editing = false
    var symmetryOptionsOpen = false

    init(model: CanvasModel) {
        self.model = model
        info = model.session.guide()
    }

    /// The guide's lines follow the canvas.
    func read() {
        info = model.session.guide()
    }

    func setOn(_ on: Bool) {
        info = model.session.setGuideOn(on: on)
    }

    func setAssisted(_ on: Bool) {
        info = model.session.setGuideAssisted(assisted: on)
        model.refresh()
    }

    /// Opens the editor (turning the guide on), the canvas fitted between its bars.
    func open() {
        let session = model.session
        model.endQuickShape()
        model.closePanels()
        if !info.on {
            _ = session.setGuideOn(on: true)
        }
        let size = CGSize(width: model.screenCentre.x * 2, height: model.screenCentre.y * 2)
        let band = size.height * fitBand
        let side = fitSide * model.pixels
        info = session.guideEditBegin(
            left: Float(side), top: Float(band), right: Float(size.width - side), bottom: Float(size.height - band)
        )
        editing = true
        model.refreshView()
    }

    func close(keep: Bool) {
        info = model.session.guideEditEnd(keep: keep)
        editing = false
        symmetryOptionsOpen = false
        model.refreshView()
    }

    func choose(_ kind: GuideType) {
        info = model.session.setGuideKind(kind: kind)
        symmetryOptionsOpen = false
    }

    /// The shown kind's look, each left as it is when not given.
    func setLook(opacity: Float? = nil, thickness: Float? = nil, size: Float? = nil, hue: Float? = nil) {
        info = model.session.setGuideLook(
            opacity: opacity ?? info.opacity, thickness: thickness ?? info.thickness,
            size: size ?? info.size, hue: hue ?? info.hue
        )
    }

    /// The shown grid's size typed on its keypad: `typed` so far, in `unit`. It changes at once.
    func setLength(_ typed: String, unit: GuideUnit) {
        info = model.session.setGuideLength(typed: typed, unit: unit)
    }

    func setSymmetry(_ kind: SymmetryType, rotational: Bool) {
        info = model.session.setGuideSymmetry(kind: kind, rotational: rotational)
    }

    /// What a touch at window points `p` takes: the turn handle (-2), the centre (-1), a vanishing
    /// point (its index), or nothing.
    func grab(_ p: CGPoint) -> Int? {
        let pixels = model.pixels
        let x = p.x * pixels
        let y = p.y * pixels
        symmetryOptionsOpen = false
        if info.kind == .perspective {
            let index = model.session.touchVanishingPoint(x: Float(x), y: Float(y)).map { Int($0) }
            read()
            return index
        }
        func near(_ at: [Float]) -> Bool {
            at.count >= 2 && hypot(CGFloat(at[0]) - x, CGFloat(at[1]) - y) <= handleReach * pixels
        }
        if near(info.handle) {
            return -2
        }
        return near(info.centre) ? -1 : nil
    }

    func drag(_ grab: Int?, _ p: CGPoint) {
        guard let grab else { return }
        let session = model.session
        let x = Float(p.x * model.pixels)
        let y = Float(p.y * model.pixels)
        switch grab {
        case -2: info = session.turnGuide(x: x, y: y)
        case -1: info = session.moveGuideCentre(x: x, y: y)
        default: info = session.moveVanishingPoint(index: UInt32(grab), x: x, y: y)
        }
    }
}

/// The drawing guide's lines over the canvas while it is on (and its handles while editing).
struct GuideOverlay: View {
    let model: CanvasModel

    var body: some View {
        let tool = model.guide
        let g = tool.info
        if g.on || tool.editing {
            let pixels = model.pixels
            let rgb = g.color + [0, 0, 0]
            let colour = Color(red: Double(rgb[0]), green: Double(rgb[1]), blue: Double(rgb[2]), opacity: Double(g.opacity))
            let editing = tool.editing
            Canvas { context, _ in
                func point(_ v: [Float], _ i: Int) -> CGPoint {
                    CGPoint(x: CGFloat(v[i]) / pixels, y: CGFloat(v[i + 1]) / pixels)
                }
                var lines = Path()
                for i in 0 ..< g.lines.count / 4 {
                    lines.move(to: point(g.lines, i * 4))
                    lines.addLine(to: point(g.lines, i * 4 + 2))
                }
                context.stroke(lines, with: .color(colour), lineWidth: CGFloat(0.25 + 1.5 * g.thickness))
                guard editing else { return }
                func node(_ at: CGPoint, _ fill: Color) {
                    context.fill(Path(ellipseIn: CGRect(x: at.x - 7, y: at.y - 7, width: 14, height: 14)), with: .color(.white))
                    context.fill(Path(ellipseIn: CGRect(x: at.x - 5, y: at.y - 5, width: 10, height: 10)), with: .color(fill))
                }
                if g.kind == .perspective {
                    for i in 0 ..< g.vanishing.count / 2 {
                        node(point(g.vanishing, i * 2), nodeCentre)
                    }
                } else if g.handle.count >= 2, g.centre.count >= 2 {
                    node(point(g.handle, 0), nodeTurn)
                    node(point(g.centre, 0), nodeCentre)
                }
            }
            .allowsHitTesting(false)
            // the lines are the view's: they wait for the canvas to be attached
            .task { tool.read() }
        }
    }
}

/// The guide editor over the canvas: its title with Cancel and Done, the colour bar, the canvas
/// fitted between them and the toolbar (kinds, look and assisted drawing), the guide's handles
/// dragged on the canvas, and the grid size typed on a keypad from its chip.
struct GuideEditor: View {
    let model: CanvasModel
    let screen: CGSize

    /// What the grid size's keypad has typed, and in which unit, while it is up.
    @State private var keypad: (typed: String, unit: GuideUnit)?
    /// What the touch on the canvas took, once it has come down.
    @State private var grabbed: Int??

    private func type(_ typed: String, unit: GuideUnit) {
        keypad = (typed, unit)
        model.guide.setLength(typed, unit: unit)
    }

    private func press(_ key: String, _ k: (typed: String, unit: GuideUnit)) {
        let t = k.typed
        switch key {
        case "⌫":
            if !t.isEmpty {
                type(String(t.dropLast()), unit: k.unit)
            }
        case ".":
            if !t.contains("."), t.count < 7 {
                type(t + ".", unit: k.unit)
            }
        default:
            if t.count < 7 {
                type(t + key, unit: k.unit)
            }
        }
    }

    private var header: some View {
        ZStack(alignment: .topLeading) {
            Text(verbatim: "Drawing Guides")
                .font(.system(size: 20, weight: .bold))
                .foregroundStyle(guideTitle)
                .lineLimit(1)
                .offset(x: 32, y: 29)
            HStack(spacing: 13) {
                Spacer(minLength: 0)
                Text(verbatim: "Cancel")
                    .font(.system(size: 17))
                    .foregroundStyle(Look.shared.light ? Color(argb: 0xFF58_5858) : guideTitle)
                    .lineLimit(1)
                    .frame(width: 73, height: 43)
                    .background(Capsule().fill(guideButton))
                    .overlay(Capsule().strokeBorder(guideButtonEdge, lineWidth: 1))
                    .contentShape(Capsule())
                    .onTapGesture { model.guide.close(keep: false) }
                PcIconView(PcIcons.doneTick, size: 40, color: .white)
                    .frame(width: 44, height: 44)
                    .background(Circle().fill(guideDone))
                    .contentShape(Circle())
                    .onTapGesture { model.guide.close(keep: true) }
            }
            .padding(.top, 20)
            .padding(.trailing, 20)
        }
        .frame(height: 82, alignment: .top)
        .frame(maxWidth: .infinity)
        .background(guideTop)
        .contentShape(Rectangle())
        .onTapGesture {}
    }

    var body: some View {
        let tool = model.guide
        let g = tool.info
        // up while the kind has a grid
        let hasGrid = g.kind == .grid || g.kind == .isometric
        ZStack(alignment: .topLeading) {
            // the canvas: handles dragged, perspective points added; nothing paints
            Color.clear
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { drag in
                            if grabbed == nil {
                                grabbed = .some(tool.grab(drag.startLocation))
                            }
                            if drag.translation != .zero, let grab = grabbed {
                                tool.drag(grab, drag.location)
                            }
                        }
                        .onEnded { _ in grabbed = nil }
                )
            header
            ColorBar(hue: g.hue) { tool.setLook(hue: $0) }
                .offset(y: 84)
            if g.kind == .perspective, g.vanishing.isEmpty {
                Text(verbatim: "Tap to create vanishing points")
                    .font(.system(size: 12))
                    .foregroundStyle(guideTitle)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, minHeight: 24, maxHeight: 24)
                    .background(guideNote)
                    .offset(y: 94)
                    .allowsHitTesting(false)
            }
            GuideToolbar(tool: tool, screen: screen) { keypad = (g.sizeText, g.unit) }
                .frame(width: screen.width, height: screen.height, alignment: .bottom)
            if tool.symmetryOptionsOpen, g.kind == .symmetry {
                SymmetryOptions(tool: tool, screen: screen)
            }
            if let k = keypad, hasGrid {
                GridSizeKeypad(
                    typed: k.typed, unit: k.unit, screenWidth: screen.width,
                    onUnit: { type("", unit: $0) },
                    onKey: { press($0, k) },
                    onDone: { keypad = nil }
                )
                .frame(width: screen.width, height: screen.height, alignment: .bottom)
            }
        }
    }
}

/// The colour bar: the guide's colour picked by dragging along it.
private struct ColorBar: View {
    let hue: Float
    let onChange: (Float) -> Void

    var body: some View {
        GeometryReader { g in
            let w = g.size.width
            let x = min(max(w * CGFloat(hue), 7), max(w - 7, 7))
            let shown = barStops[min(max(wholeNumber(hue * Float(barStops.count - 1)), 0), barStops.count - 1)]
            ZStack(alignment: .leading) {
                LinearGradient(colors: barStops.map { Color(argb: $0) }, startPoint: .leading, endPoint: .trailing)
                    .frame(height: 5.5)
                Circle().fill(Color.white).frame(width: 14, height: 14).offset(x: x - 7)
                Circle().fill(Color(argb: shown)).frame(width: 11, height: 11).offset(x: x - 5.5)
            }
            .frame(width: w, height: 10)
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
                onChange(Float(min(max(drag.location.x / max(w, 1), 0), 1)))
            })
        }
        .frame(height: 10)
    }
}

/// The guide editor's toolbar: the four kinds, then the kind's look and assisted drawing (on a wide
/// screen the whole width less 8 each side, the sliders in one row and the switch at the end).
private struct GuideToolbar: View {
    let tool: GuideTool
    let screen: CGSize
    let onChip: () -> Void

    private static let kinds: [(kind: GuideType, row: Kind)] = [
        (.grid, Kind(icon: PcIcons.guideGrid, label: "2D Grid", width: 114)),
        (.isometric, Kind(icon: PcIcons.guideIsometric, label: "Isometric", width: 123)),
        (.perspective, Kind(icon: PcIcons.chromaPerspective, label: "Perspective", width: 138)),
        (.symmetry, Kind(icon: PcIcons.guideSymmetry, label: "Symmetry", width: 129)),
    ]

    private var kindsSwitch: some View {
        KindsSwitch(
            width: 546, kinds: Self.kinds.map { $0.row },
            chosen: Self.kinds.firstIndex { $0.kind == tool.info.kind } ?? 0,
            onChoose: { tool.choose(Self.kinds[$0].kind) }
        )
    }

    private func look(_ width: CGFloat) -> some View {
        let g = tool.info
        return Group {
            OptionSlider(label: "Opacity", value: g.opacity, shown: reading(g.opacity, max: true), width: width) { tool.setLook(opacity: $0) }
            OptionSlider(label: "Thickness", value: g.thickness, shown: reading(g.thickness, max: true), width: width) { tool.setLook(thickness: $0) }
        }
    }

    private func gridSize(_ width: CGFloat) -> some View {
        let g = tool.info
        return OptionSlider(
            label: "Grid size", value: sizeToSlider(g.size), shown: g.sizeLabel, width: width, chip: true, onChip: onChip,
            onChange: { tool.setLook(size: sliderToSize($0)) }
        )
    }

    private func optionsButton(_ width: CGFloat) -> some View {
        Text(verbatim: "Options")
            .font(.system(size: 15))
            .foregroundStyle(guideLabel)
            .lineLimit(1)
            .frame(width: width, height: 40)
            .background(Capsule().fill(optionsFill))
            .contentShape(Capsule())
            .onTapGesture { tool.symmetryOptionsOpen.toggle() }
    }

    private var assisted: some View {
        HStack(spacing: 0) {
            Text(verbatim: "Assisted Drawing")
                .font(.system(size: 15))
                .foregroundStyle(guideLabel)
                .lineLimit(1)
            Spacer(minLength: 0)
            PcSwitch(on: tool.info.assisted)
                .contentShape(Rectangle())
                .onTapGesture { tool.setAssisted(!tool.info.assisted) }
        }
    }

    var body: some View {
        let kind = tool.info.kind
        let wide = screen.width > screen.height
        let plate = RoundedRectangle(cornerRadius: 33, style: .circular)
        Group {
            if wide {
                VStack(spacing: 16) {
                    kindsSwitch
                    HStack(spacing: 19) {
                        look(257)
                        switch kind {
                        case .grid, .isometric: gridSize(257)
                        case .symmetry: optionsButton(257)
                        case .perspective: EmptyView()
                        }
                        Spacer(minLength: 0)
                        assisted
                            .frame(width: 210)
                    }
                    .padding(.leading, 16)
                    .padding(.trailing, 22)
                    .frame(height: 54)
                }
                .padding(.top, 16)
                .frame(width: screen.width - 16, height: 137, alignment: .top)
            } else {
                VStack(spacing: 0) {
                    kindsSwitch
                        .padding(.bottom, 14)
                    HStack(spacing: 20) { look(300) }
                        .frame(width: 620)
                        .padding(.bottom, 8)
                    HStack(spacing: 20) {
                        switch kind {
                        case .grid, .isometric:
                            gridSize(300)
                            assisted
                                .padding(.leading, 8)
                                .frame(width: 300)
                        case .perspective:
                            assisted
                                .padding(.leading, 8)
                                .frame(width: 300)
                            Spacer(minLength: 0)
                        case .symmetry:
                            optionsButton(300)
                            Spacer(minLength: 0)
                        }
                    }
                    .frame(width: 620, height: 54)
                }
                .padding(.top, 16)
                .frame(width: 652, height: 199, alignment: .top)
            }
        }
        .background(plate.fill(panelFill))
        .contentShape(Rectangle())
        .onTapGesture {}
        .offset(y: -25)
    }
}

/// The grid size's keypad, as measured: a sheet over the editor's foot, the whole width, rounded 30
/// at its top; "Length" and what is typed in a 250 wide title centred 24 below its top; then the
/// units, the digits (three across, 0 two wide) and Backspace over Done, in keys 150 x 79.5 from 51
/// below its top. Where the screen is too narrow for them, the keys are narrower.
private struct GridSizeKeypad: View {
    let typed: String
    let unit: GuideUnit
    let screenWidth: CGFloat
    let onUnit: (GuideUnit) -> Void
    let onKey: (String) -> Void
    let onDone: () -> Void

    private func key(_ label: String, fill: Color, ink: Color, size: CGFloat, width: CGFloat, height: CGFloat = keyHeight, tap: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: size))
            .foregroundStyle(ink)
            .lineLimit(1)
            .frame(width: width, height: height)
            .background(RoundedRectangle(cornerRadius: 8, style: .circular).fill(fill))
            .contentShape(Rectangle())
            .onTapGesture(perform: tap)
    }

    var body: some View {
        let w = min(max((screenWidth - 16 - keyGroups * 2 - keyGap * 2) / 5, 40), 150)
        VStack(spacing: 0) {
            ZStack(alignment: .leading) {
                Text(verbatim: "Length")
                    .padding(.leading, 16)
                Text(verbatim: typed)
                    .padding(.leading, 160)
            }
            .font(.system(size: 12))
            .foregroundStyle(sheetInk)
            .lineLimit(1)
            .frame(width: 250, height: 32, alignment: .leading)
            .padding(.top, 8)
            .padding(.bottom, 11)
            HStack(alignment: .top, spacing: keyGroups) {
                VStack(spacing: keyGap) {
                    ForEach(units.indices, id: \.self) { i in
                        let chosen = units[i].unit == unit
                        key(units[i].label, fill: chosen ? sheetUnit : sheetKey, ink: chosen ? sheetUnitInk : sheetInk, size: 16, width: w) {
                            onUnit(units[i].unit)
                        }
                    }
                }
                VStack(spacing: keyGap) {
                    ForEach([["7", "8", "9"], ["4", "5", "6"], ["1", "2", "3"]], id: \.self) { row in
                        HStack(spacing: keyGap) {
                            ForEach(row, id: \.self) { d in
                                key(d, fill: sheetDigit, ink: sheetInk, size: 23, width: w) { onKey(d) }
                            }
                        }
                    }
                    HStack(spacing: keyGap) {
                        key("0", fill: sheetDigit, ink: sheetInk, size: 23, width: w * 2 + keyGap) { onKey("0") }
                        key(".", fill: sheetDigit, ink: sheetInk, size: 23, width: w) { onKey(".") }
                    }
                }
                VStack(spacing: keyGap) {
                    key("Backspace", fill: sheetKey, ink: sheetInk, size: 16, width: w) { onKey("⌫") }
                    key("Done", fill: sheetKey, ink: sheetInk, size: 16, width: w, height: keyHeight * 3 + keyGap * 2, tap: onDone)
                }
            }
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity)
        .frame(height: sheetHeight)
        .background(UnevenRoundedRectangle(topLeadingRadius: 30, topTrailingRadius: 30, style: .circular).fill(sheetFill))
        .contentShape(Rectangle())
        .onTapGesture {}
    }
}

/// Symmetry's options: vertical, horizontal, quadrant or radial; turned instead of mirrored;
/// assisted.
private struct SymmetryOptions: View {
    let tool: GuideTool
    let screen: CGSize

    private static let kinds: [(kind: SymmetryType, label: String)] = [
        (.vertical, "Vertical"), (.horizontal, "Horizontal"), (.quadrant, "Quadrant"), (.radial, "Radial"),
    ]

    private func switchRow(_ label: String, on: Bool, change: @escaping () -> Void) -> some View {
        HStack(spacing: 0) {
            ToolbarMenuLabel(text: label)
            Spacer(minLength: 0)
            PcSwitch(on: on)
                .contentShape(Rectangle())
                .onTapGesture(perform: change)
        }
        .frame(height: 40)
    }

    var body: some View {
        let g = tool.info
        // over the Options button, its arrow at the button's middle: the third of the row when wide
        let wide = screen.width > screen.height
        let thirdOfRow: CGFloat = 8 + 16 + 276 * 2 + 128.5
        let firstOfRow: CGFloat = (screen.width - 652) / 2 + 166
        let middle = wide ? thirdOfRow : firstOfRow
        let left = min(max(middle - 150, 0), screen.width - 300)
        ToolbarPopover(
            arrowX: middle - left, left: left, bottom: wide ? screen.height - 89 : screen.height - 98,
            screenWidth: screen.width
        ) {
            ToolbarMenuTitle(text: "Guide Options")
                .padding(.bottom, 10)
            ToolbarMenuGroup {
                ForEach(Self.kinds.indices, id: \.self) { i in
                    let kind = Self.kinds[i].kind
                    let chosen = g.symmetry == kind
                    if i > 0 {
                        ToolbarMenuLine()
                    }
                    HStack(spacing: 0) {
                        ToolbarMenuLabel(text: Self.kinds[i].label, color: chosen ? Color.white : nil)
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
                    .onTapGesture { tool.setSymmetry(kind, rotational: g.rotational) }
                }
            }
            .padding(.bottom, 14)
            switchRow("Rotational Symmetry", on: g.rotational) { tool.setSymmetry(g.symmetry, rotational: !g.rotational) }
            switchRow("Assisted Drawing", on: g.assisted) { tool.setAssisted(!g.assisted) }
        }
    }
}
