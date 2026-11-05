import SwiftUI

private let nodeBlue = Color(argb: 0xFF01_71EA)
private let rotateGreen = Color(argb: 0xFF00_E607)
private let rotateLine = Color(argb: 0xFF00_E400)
private let turnOrange = Color(argb: 0xFFFE_B800)
private let turnLine = Color(argb: 0xFFFE_BF00)
private let nodeShadow = Color(argb: 0x1F00_0000)
private let meshLine = Color(argb: 0xFF8B_8B8B)
/// The lines a moved box snapped onto, across the canvas.
private var snapGuide: Color {
    Color(argb: Look.shared.light ? 0xFFFF_9500 : 0xFFFF_9F0A)
}

/// A held node is drawn this much larger.
private let heldScale: CGFloat = 1.5
/// How long a let-go rotate or box-turning node takes to spring back, seconds.
private let handleReturn: TimeInterval = 0.5
private let handleCurve = UnitCurve.bezier(
    startControlPoint: UnitPoint(x: 0.4, y: 0), endControlPoint: UnitPoint(x: 0.2, y: 1)
)

/// The transform toolbar's buttons: with room (landscape), and where the screen is narrow.
private let buttonRoomy: CGFloat = 89
private let buttonNarrow: CGFloat = 83

private let snapDistanceMin: Float = 1
private let snapDistanceMax: Float = 50
private let snapDistanceDefault: Float = 3
private let snapVelocityMax: Float = 10

private let fieldIdle = Color(argb: 0xFF38_3838)
private let topKey = Color(argb: 0xFF2E_2E2E)
private let digitKey = Color(argb: 0xFF41_4141)
private let keyInk = Color(argb: 0xFFDA_DADA)
private let linkOn = Color(argb: 0xFF1F_436B)
private let numericWidth: CGFloat = 248
/// How far the arrow's tip stands from the node's middle (the panel's edge 17 from it).
private let numericNodeGap: CGFloat = 5

extension PcIcons {
    /// The advanced mesh icon's cells: four across, three down, with thin gaps.
    static var advancedMesh: PcIcon {
        var cells = ""
        for row in 0 ..< 3 {
            for col in 0 ..< 4 {
                let x = 1.5 + Double(col) * 5.625
                let y = 4.9 + Double(row) * 5.233
                cells += "M\(x) \(y)h4.425v4.033h-4.425z"
            }
        }
        return PcIcon(name: "advanced-mesh", width: 24, height: 24, parts: [
            PcIconPart(d: cells, stroke: 0.6, fill: true, evenOdd: false),
        ])
    }
}

/// A popover over the transform toolbar.
enum TransformPopup {
    case none, snapping, interpolation
}

/// Numeric entry open beside the node at window point `at`: for the box's angle (the rotation
/// handle) or its size; to the node's right when it is on the box's right side, else to its left.
struct TransformNumericAsk: Equatable {
    let angle: Bool
    let at: CGPoint
    let right: Bool
}

/// The transform's state: the core keeps the box, this what the tool shows of it.
@Observable
final class TransformTool {
    private static let kindKey = "transformKind"
    private static let interpolationKey = "interpolation"
    private static let meshKey = "advancedMesh"
    private static let magneticsKey = "magnetics"
    private static let snappingKey = "snapping"
    private static let distanceKey = "snapDistance"
    private static let velocityKey = "snapVelocity"

    @ObservationIgnored private unowned let model: CanvasModel

    private(set) var box: TransformBox?
    private(set) var kind: TransformKind
    private(set) var interpolation: Interpolation
    var popup = TransformPopup.none
    /// Magnetics, snapping, and snapping's distance (1..50 points) and velocity (0..10).
    private(set) var guides: GuideSettings
    /// A warp shows and takes all 16 of its control points, not only its corners.
    private(set) var advancedMesh: Bool
    /// The node held and where (window points); the one let go, springing back from where and since
    /// when.
    private(set) var held: (handle: TransformHandle, at: CGPoint)?
    private(set) var letGo: (handle: TransformHandle, at: CGPoint, since: Date)?
    var numeric: TransformNumericAsk?

    init(model: CanvasModel) {
        self.model = model
        let d = UserDefaults.standard
        kind = Self.kinds[d.string(forKey: Self.kindKey) ?? ""] ?? .freeform
        interpolation = Self.interpolations[d.string(forKey: Self.interpolationKey) ?? ""] ?? .bilinear
        advancedMesh = d.bool(forKey: Self.meshKey)
        guides = GuideSettings(
            magnetics: d.bool(forKey: Self.magneticsKey),
            snapping: d.bool(forKey: Self.snappingKey),
            distance: d.object(forKey: Self.distanceKey) as? Float ?? 3,
            velocity: d.object(forKey: Self.velocityKey) as? Float ?? 10
        )
    }

    private static let kinds: [String: TransformKind] = [
        "freeform": .freeform, "uniform": .uniform, "distort": .distort, "warp": .warp,
    ]
    private static let interpolations: [String: Interpolation] = [
        "nearest": .nearest, "bilinear": .bilinear, "bicubic": .bicubic,
    ]

    /// Opens on the current layer; nothing there to move, it says so and stays out of transform.
    func begin() -> Bool {
        let s = model.session
        s.transformSetInterpolation(interpolation: interpolation)
        s.transformSetGuides(guides: guides)
        guard let first = s.transformBegin() else {
            model.showNotice(s.transformRefusal())
            return false
        }
        // a warp lays its mesh over what began
        box = s.transformSetKind(kind: kind) ?? first
        popup = .none
        return true
    }

    func ended() {
        box = nil
        popup = .none
        numeric = nil
        held = nil
        letGo = nil
    }

    func choose(_ next: TransformKind) {
        kind = next
        box = model.session.transformSetKind(kind: next) ?? box
        UserDefaults.standard.set(Self.kinds.first { $0.value == next }?.key, forKey: Self.kindKey)
    }

    func choose(_ next: Interpolation) {
        interpolation = next
        model.session.transformSetInterpolation(interpolation: next)
        UserDefaults.standard.set(Self.interpolations.first { $0.value == next }?.key, forKey: Self.interpolationKey)
    }

    func toggleAdvancedMesh() {
        advancedMesh.toggle()
        UserDefaults.standard.set(advancedMesh, forKey: Self.meshKey)
        model.showNotice("Advanced mesh " + (advancedMesh ? "on" : "off"))
    }

    func changeGuides(_ g: GuideSettings) {
        if g.snapping != guides.snapping {
            model.showNotice("Snapping " + (g.snapping ? "on" : "off"))
        } else if g.magnetics != guides.magnetics {
            model.showNotice("Magnetics " + (g.magnetics ? "on" : "off"))
        }
        guides = g
        model.session.transformSetGuides(guides: g)
        let d = UserDefaults.standard
        d.set(g.magnetics, forKey: Self.magneticsKey)
        d.set(g.snapping, forKey: Self.snappingKey)
        d.set(g.distance, forKey: Self.distanceKey)
        d.set(g.velocity, forKey: Self.velocityKey)
    }

    func apply(_ action: TransformAction) {
        popup = .none
        box = model.session.transformApply(action: action) ?? box
    }

    /// The box after a drag step on the canvas.
    func moved(_ next: TransformBox?) {
        if let next {
            box = next
        }
    }

    func setSize(width: Float, height: Float) {
        moved(model.session.transformSetSize(width: width, height: height))
    }

    func setAngle(_ degrees: Float) {
        moved(model.session.transformSetAngle(degrees: degrees))
    }

    /// The touches hold `handle` at window pixels (`x`, `y`).
    func hold(_ handle: TransformHandle, x: Float, y: Float) {
        held = (handle, CGPoint(x: CGFloat(x) / model.pixels, y: CGFloat(y) / model.pixels))
    }

    /// The touches let `handle` go there: a rotate or box-turning node springs back.
    func release(_ handle: TransformHandle, x: Float, y: Float) {
        held = nil
        if handle == .rotate || handle == .turnBox {
            letGo = (handle, CGPoint(x: CGFloat(x) / model.pixels, y: CGFloat(y) / model.pixels), Date())
        }
    }

    func settled() {
        letGo = nil
    }
}

/// The transform box on screen, in window points: its corners (top left, top right, bottom right,
/// bottom left), the middles of its edges (top, right, bottom, left), the rotate node above it, the
/// node below it that turns the box alone, and the box's up direction.
private struct BoxOnScreen {
    let corners: [CGPoint]
    let edges: [CGPoint]
    let rotate: CGPoint
    let turn: CGPoint
    let up: CGPoint

    init?(_ box: TransformBox, view: [Float], pixels: CGFloat) {
        guard let s = screenBox(corners: box.corners, view: view, density: Float(pixels)),
              s.rotate.count >= 2, s.turn.count >= 2, s.up.count >= 2 else { return nil }
        func points(_ v: [Float]) -> [CGPoint] {
            stride(from: 0, to: v.count - 1, by: 2).map {
                CGPoint(x: CGFloat(v[$0]) / pixels, y: CGFloat(v[$0 + 1]) / pixels)
            }
        }
        corners = points(s.corners)
        edges = points(s.edges)
        rotate = points(s.rotate)[0]
        turn = points(s.turn)[0]
        up = CGPoint(x: CGFloat(s.up[0]), y: CGFloat(s.up[1]))
    }
}

/// A node: a coloured dot on a white disc with a faint shadow, `scale` times its size (larger while
/// held).
private func node(_ context: GraphicsContext, at p: CGPoint, color: Color, scale: CGFloat) {
    func disc(_ radius: CGFloat) -> Path {
        Path(ellipseIn: CGRect(x: p.x - radius, y: p.y - radius, width: radius * 2, height: radius * 2))
    }
    blurred(context, disc(5.5 * scale + 0.5))
    context.fill(disc(5.5 * scale), with: .color(.white))
    context.fill(disc(3.5 * scale), with: .color(color))
}

private func blurred(_ context: GraphicsContext, _ path: Path) {
    context.drawLayer { layer in
        layer.addFilter(.blur(radius: 1))
        layer.fill(path, with: .color(nodeShadow))
    }
}

/// The transform box over the canvas: dashed edges, its nodes, the rotate node and the box-turning
/// node.
struct TransformOverlay: View {
    let model: CanvasModel

    var body: some View {
        let tool = model.transform
        if let box = tool.box {
            Group {
                if tool.kind == .warp, box.mesh.count == 32 {
                    WarpOverlay(model: model, box: box)
                } else if let shown = BoxOnScreen(box, view: model.viewMatrix, pixels: model.pixels) {
                    BoxOverlay(model: model, box: box, shown: shown)
                }
            }
            .allowsHitTesting(false)
        }
    }
}

private struct BoxOverlay: View {
    let model: CanvasModel
    let box: TransformBox
    let shown: BoxOnScreen

    private func spot(_ x: Float, _ y: Float) -> CGPoint {
        model.canvasToWindow(CGPoint(x: CGFloat(x), y: CGFloat(y)))
    }

    /// Where a node is: following the touch while held, on its way back once let go.
    private func placed(_ handle: TransformHandle, rest: CGPoint, now: Date) -> CGPoint {
        let tool = model.transform
        if let held = tool.held, held.handle == handle {
            return held.at
        }
        if let back = tool.letGo, back.handle == handle {
            let t = handleCurve.value(at: min(now.timeIntervalSince(back.since) / handleReturn, 1))
            return CGPoint(x: back.at.x + (rest.x - back.at.x) * t, y: back.at.y + (rest.y - back.at.y) * t)
        }
        return rest
    }

    private func scale(_ handle: TransformHandle) -> CGFloat {
        model.transform.held?.handle == handle ? heldScale : 1
    }

    private func draw(_ context: GraphicsContext, now: Date) {
        let rotate = placed(.rotate, rest: shown.rotate, now: now)
        let turn = placed(.turnBox, rest: shown.turn, now: now)
        // the lines the box snapped onto, across the canvas, while it is held (under the box)
        let snaps = box.snapLines
        for i in 0 ..< snaps.count / 4 {
            var line = Path()
            line.move(to: spot(snaps[4 * i], snaps[4 * i + 1]))
            line.addLine(to: spot(snaps[4 * i + 2], snaps[4 * i + 3]))
            context.stroke(line, with: .color(snapGuide), lineWidth: 1)
        }
        var edge = Path()
        edge.addLines(shown.corners)
        edge.closeSubpath()
        context.stroke(edge, with: .color(overlayDashGap), style: StrokeStyle(lineWidth: 1, dash: [5, 5], dashPhase: 5))
        context.stroke(edge, with: .color(overlayDash), style: StrokeStyle(lineWidth: 1, dash: [5, 5], dashPhase: 0))
        var green = Path()
        green.move(to: shown.edges[0])
        green.addLine(to: rotate)
        context.stroke(green, with: .color(rotateLine), lineWidth: 1)
        var orange = Path()
        orange.move(to: shown.edges[2])
        orange.addLine(to: turn)
        context.stroke(orange, with: .color(turnLine), lineWidth: 1)
        for (i, p) in shown.corners.enumerated() {
            node(context, at: p, color: nodeBlue, scale: scale(.corner(index: UInt8(i))))
        }
        for (i, p) in shown.edges.enumerated() {
            node(context, at: p, color: nodeBlue, scale: scale(.edge(index: UInt8(i))))
        }
        node(context, at: rotate, color: rotateGreen, scale: scale(.rotate))
        // the box-turning node is a square, turned with the box
        let k = scale(.turnBox)
        var square = context
        square.translateBy(x: turn.x, y: turn.y)
        square.rotate(by: .radians(atan2(shown.up.x, -shown.up.y)))
        blurred(square, Path(ellipseIn: CGRect(x: -6 * k, y: -6 * k, width: 12 * k, height: 12 * k)))
        let outer = 11 * k
        let inner = 8 * k
        square.fill(
            Path(roundedRect: CGRect(x: -outer / 2, y: -outer / 2, width: outer, height: outer), cornerRadius: 2.5 * k),
            with: .color(.white)
        )
        square.fill(
            Path(roundedRect: CGRect(x: -inner / 2, y: -inner / 2, width: inner, height: inner), cornerRadius: k),
            with: .color(turnOrange)
        )
    }

    var body: some View {
        let tool = model.transform
        TimelineView(.animation(paused: tool.letGo == nil)) { timeline in
            Canvas { context, _ in
                draw(context, now: timeline.date)
            }
        }
        .task(id: tool.letGo?.since) {
            guard tool.letGo != nil else { return }
            try? await Task.sleep(for: .seconds(handleReturn))
            guard !Task.isCancelled else { return }
            tool.settled()
        }
    }
}

/// A warp over the canvas: its lines at the thirds, its shown control points, and with the advanced
/// mesh the lines joining them.
private struct WarpOverlay: View {
    let model: CanvasModel
    let box: TransformBox

    private func spot(_ x: Float, _ y: Float) -> CGPoint {
        model.canvasToWindow(CGPoint(x: CGFloat(x), y: CGFloat(y)))
    }

    /// Point `i` of the mesh is held (a corner taking the point under it).
    private func holding(_ i: Int) -> Bool {
        switch model.transform.held?.handle {
        case let .mesh(index): Int(index) == i
        case let .corner(index): [0, 3, 15, 12][Int(index) % 4] == i
        default: false
        }
    }

    var body: some View {
        let advanced = model.transform.advancedMesh
        Canvas { context, _ in
            let mesh = (0 ..< 16).map { spot(box.mesh[2 * $0], box.mesh[2 * $0 + 1]) }
            if advanced {
                // the control points joined along each row, and down the two edges, dotted, under
                // the lines (which hide them where they agree)
                var dots = Path()
                for k in 0 ..< 4 {
                    for j in 0 ..< 3 {
                        dots.move(to: mesh[k * 4 + j])
                        dots.addLine(to: mesh[k * 4 + j + 1])
                        if k == 0 || k == 3 {
                            dots.move(to: mesh[j * 4 + k])
                            dots.addLine(to: mesh[(j + 1) * 4 + k])
                        }
                    }
                }
                context.stroke(dots, with: .color(overlayDash), style: StrokeStyle(lineWidth: 1, dash: [1, 2]))
            }
            let grid = box.grid
            let perLine = grid.count / 2 / 8
            if perLine >= 2 {
                for line in 0 ..< 8 {
                    var path = Path()
                    for k in 0 ..< perLine {
                        let i = (line * perLine + k) * 2
                        let p = spot(grid[i], grid[i + 1])
                        if k == 0 {
                            path.move(to: p)
                        } else {
                            path.addLine(to: p)
                        }
                    }
                    context.stroke(path, with: .color(meshLine), lineWidth: 1)
                }
            }
            for i in shownMeshPoints(advanced: advanced).map({ Int($0) }) {
                node(context, at: mesh[i], color: nodeBlue, scale: holding(i) ? heldScale : 1)
            }
        }
    }
}

private func interpolationName(_ i: Interpolation, short: Bool) -> String {
    switch i {
    case .nearest: short ? "Nearest" : "Nearest Neighbor"
    case .bilinear: "Bilinear"
    case .bicubic: "Bicubic"
    }
}

/// The floating toolbar at the foot of the screen while transforming.
struct TransformToolbar: View {
    let model: CanvasModel
    let screenWidth: CGFloat

    var body: some View {
        let tool = model.transform
        let popup = tool.popup
        FloatingToolbar(
            layout: ToolbarLayout(screenWidth: screenWidth, count: 7, roomy: buttonRoomy, narrow: buttonNarrow),
            onBackground: { tool.popup = .none }
        ) {
            ToolbarSegment(icon: PcIcons.freeform, label: "Freeform", selected: tool.kind == .freeform, iconSize: 43) { tool.choose(.freeform) }
            ToolbarSegment(icon: PcIcons.uniform, label: "Uniform", selected: tool.kind == .uniform, iconSize: 43) { tool.choose(.uniform) }
            ToolbarSegment(icon: PcIcons.distort, label: "Distort", selected: tool.kind == .distort, iconSize: 43) { tool.choose(.distort) }
            ToolbarSegment(icon: PcIcons.warp, label: "Warp", selected: tool.kind == .warp, iconSize: 43) { tool.choose(.warp) }
        } buttons: { w in
            // a long name smaller only where the buttons are narrow
            let long: CGFloat = w < buttonRoomy ? 11 : 12.5
            if tool.kind == .warp {
                ToolbarButton(icon: PcIcons.advancedMesh, label: "Advanced Mesh", active: tool.advancedMesh, width: w, iconSize: 28.8, labelSize: long) {
                    tool.toggleAdvancedMesh()
                }
            } else {
                ToolbarButton(icon: PcIcons.snapping, label: "Snapping", active: popup == .snapping, width: w, iconSize: 44) {
                    tool.popup = popup == .snapping ? .none : .snapping
                }
            }
            ToolbarButton(icon: PcIcons.flipHorizontal, label: "Flip Horizontal", width: w, iconSize: 50, labelSize: long) { tool.apply(.flipHorizontal) }
            ToolbarButton(icon: PcIcons.flipVertical, label: "Flip Vertical", width: w, iconSize: 50, labelSize: long) { tool.apply(.flipVertical) }
            ToolbarButton(icon: PcIcons.rotate45, label: "Rotate 45°", width: w, iconSize: 44) { tool.apply(.rotate45) }
            ToolbarButton(icon: PcIcons.fitToCanvas, label: "Fit to Canvas", width: w, iconSize: 35, labelSize: long) { tool.apply(.fitToCanvas) }
            ToolbarButton(icon: PcIcons.interpolation, label: interpolationName(tool.interpolation, short: true), active: popup == .interpolation, width: w, iconSize: 44) {
                tool.popup = popup == .interpolation ? .none : .interpolation
            }
            ToolbarButton(icon: PcIcons.transformReset, label: "Reset", width: w, iconSize: 44) { tool.apply(.reset) }
        }
    }
}

/// The popover open over the transform toolbar, its arrow on its button.
struct TransformPopover: View {
    let model: CanvasModel
    let screen: CGSize

    var body: some View {
        let tool = model.transform
        if tool.popup != .none {
            let layout = ToolbarLayout(screenWidth: screen.width, count: 7, roomy: buttonRoomy, narrow: buttonNarrow)
            let centre = layout.buttonCentre(tool.popup == .snapping ? 0 : 5, screenWidth: screen.width)
            let left = min(max(centre - toolbarMenuWidth / 2, 0), screen.width - toolbarMenuWidth)
            ToolbarPopover(
                arrowX: centre - left, left: left, bottom: ToolbarLayout.buttonsTop(screenHeight: screen.height),
                screenWidth: screen.width
            ) {
                if tool.popup == .interpolation {
                    InterpolationMenu(tool: tool)
                } else {
                    SnappingMenu(tool: tool)
                }
            }
        }
    }
}

private struct InterpolationMenu: View {
    let tool: TransformTool

    var body: some View {
        ToolbarMenuTitle(text: "Interpolation")
            .padding(.bottom, 10)
        ToolbarMenuGroup {
            let all: [Interpolation] = [.nearest, .bilinear, .bicubic]
            ForEach(all.indices, id: \.self) { i in
                let kind = all[i]
                let selected = tool.interpolation == kind
                if i > 0 {
                    ToolbarMenuLine()
                }
                HStack(spacing: 0) {
                    ToolbarMenuLabel(text: interpolationName(kind, short: false), color: selected ? Color.white : nil)
                    Spacer(minLength: 0)
                    if selected {
                        PcIconView(PcIcons.menuTick, size: 29, color: .white)
                    }
                }
                .padding(.leading, 11)
                .padding(.trailing, 14)
                .frame(height: 40)
                .background(selected ? Pc.accent : Color.clear)
                .contentShape(Rectangle())
                .onTapGesture {
                    tool.choose(kind)
                    tool.popup = .none
                }
            }
        }
    }
}

private struct SnappingMenu: View {
    let tool: TransformTool

    private func changed(magnetics: Bool? = nil, snapping: Bool? = nil, distance: Float? = nil, velocity: Float? = nil) {
        let g = tool.guides
        tool.changeGuides(GuideSettings(
            magnetics: magnetics ?? g.magnetics, snapping: snapping ?? g.snapping,
            distance: distance ?? g.distance, velocity: velocity ?? g.velocity
        ))
    }

    var body: some View {
        let g = tool.guides
        ToolbarMenuTitle(text: "Settings")
            .padding(.bottom, 4)
        ToolbarMenuGroup {
            ToolbarSwitchRow(label: "Magnetics", on: g.magnetics) { changed(magnetics: !g.magnetics) }
            ToolbarMenuLine()
            ToolbarSwitchRow(label: "Snapping", on: g.snapping) { changed(snapping: !g.snapping) }
        }
        .padding(.bottom, 10)
        ToolbarMenuGroup {
            // distance runs 1..50 points; at its default and below it reads as none
            PanelSlider(
                label: "Distance",
                value: (g.distance - snapDistanceMin) / (snapDistanceMax - snapDistanceMin),
                shown: g.distance <= snapDistanceDefault ? "None" : "\(wholeNumber(g.distance))",
                onChange: {
                    changed(distance: Float(wholeNumber(snapDistanceMin + $0 * (snapDistanceMax - snapDistanceMin))))
                }
            )
            ToolbarMenuLine()
            PanelSlider(
                label: "Velocity",
                value: g.velocity / snapVelocityMax,
                shown: g.velocity >= snapVelocityMax ? "Max" : "\(wholeNumber(g.velocity))",
                onChange: { changed(velocity: Float(wholeNumber($0 * snapVelocityMax))) }
            )
        }
    }
}

/// Numeric entry for an open transform, from a tap on a node: "Dimensions" (the box's width and
/// height, linked or not) or "Rotation" (its angle), typed on a keypad. Each key changes the box at
/// once; a tap outside closes it.
struct TransformNumeric: View {
    let model: CanvasModel
    let screen: CGSize

    var body: some View {
        let tool = model.transform
        if let open = tool.numeric, let box = tool.box {
            NumericPanel(tool: tool, open: open, box: box, screen: screen)
                .id(open.at.x + open.at.y * 10000 + (open.angle ? 0.5 : 0))
        }
    }
}

private struct NumericPanel: View {
    let tool: TransformTool
    let open: TransformNumericAsk
    let box: TransformBox
    let screen: CGSize

    /// Which field is typed into (0 width or the angle, 1 height), what is typed so far, the link,
    /// and the panel's height once laid out (it shows from then on).
    @State private var field = 0
    @State private var typed = ""
    @State private var linked: Bool
    @State private var tall: CGFloat?

    init(tool: TransformTool, open: TransformNumericAsk, box: TransformBox, screen: CGSize) {
        self.tool = tool
        self.open = open
        self.box = box
        self.screen = screen
        _linked = State(initialValue: tool.kind == .uniform)
    }

    private func value(_ i: Int) -> Float {
        open.angle ? box.angle : i == 0 ? box.width : box.height
    }

    private func shown(_ i: Int) -> String {
        if field == i, !typed.isEmpty {
            return typed + (open.angle ? " º" : " px")
        }
        let v = value(i)
        if open.angle {
            let turned = v.truncatingRemainder(dividingBy: 360)
            return "\(wholeNumber((turned + 360).truncatingRemainder(dividingBy: 360))) º"
        }
        return "\(wholeNumber(v)) px"
    }

    private func apply() {
        guard let v = Float(typed) else { return }
        if open.angle {
            tool.setAngle(v)
            return
        }
        let w = box.width
        let h = box.height
        if field == 0, linked, w > 0 {
            tool.setSize(width: v, height: h * abs(v) / w)
        } else if field == 0 {
            tool.setSize(width: v, height: h)
        } else if linked, h > 0 {
            tool.setSize(width: w * abs(v) / h, height: v)
        } else {
            tool.setSize(width: w, height: v)
        }
    }

    private func press(_ k: String) {
        switch k {
        case "AC":
            typed = ""
        case "±":
            if typed.hasPrefix("-") {
                typed.removeFirst()
            } else {
                typed = "-" + (typed.isEmpty ? "\(wholeNumber(value(field)))" : typed)
            }
        case "⌫":
            if !typed.isEmpty {
                typed.removeLast()
            }
        case ".":
            if !typed.contains(".") {
                typed = (typed.isEmpty ? "0" : typed) + "."
            }
        default:
            if typed.count < 7 {
                typed += k
            }
        }
        apply()
    }

    private func numberField(_ text: String, chosen: Bool, tap: @escaping () -> Void) -> some View {
        Text(verbatim: text)
            .font(.system(size: 15))
            .foregroundStyle(Color.white)
            .lineLimit(1)
            .frame(width: 80, height: 41)
            .background(RoundedRectangle(cornerRadius: 10, style: .circular).fill(chosen ? Pc.accent : fieldIdle))
            .contentShape(Rectangle())
            .onTapGesture(perform: tap)
    }

    private func key(_ label: String, fill: Color, width: CGFloat) -> some View {
        Group {
            if label == "⌫" {
                PcIconView(PcIcons.backspace, size: 22, color: keyInk)
            } else {
                Text(verbatim: label)
                    .font(.system(size: label.count > 1 ? 15 : 22))
                    .foregroundStyle(keyInk)
            }
        }
        .frame(width: width, height: 48)
        .background(RoundedRectangle(cornerRadius: 9, style: .circular).fill(fill))
        .contentShape(Rectangle())
        .onTapGesture { press(label) }
    }

    private var fields: some View {
        Group {
            if open.angle {
                HStack(spacing: 0) {
                    Text(verbatim: "Angle")
                        .font(.system(size: 15))
                        .foregroundStyle(keyInk)
                        .padding(.leading, 12)
                    Spacer(minLength: 0)
                    numberField(shown(0), chosen: true) { field = 0 }
                }
                .frame(height: 41)
                .background(RoundedRectangle(cornerRadius: 10, style: .circular).fill(fieldIdle))
            } else {
                HStack(spacing: 6) {
                    numberField(shown(0), chosen: field == 0) {
                        field = 0
                        typed = ""
                    }
                    PcIconView(PcIcons.link, size: 22, color: linked ? Pc.accent : keyInk)
                        .frame(width: 52, height: 35)
                        .background(RoundedRectangle(cornerRadius: 9, style: .circular).fill(linked ? linkOn : fieldIdle))
                        .contentShape(Rectangle())
                        .onTapGesture { linked.toggle() }
                    numberField(shown(1), chosen: field == 1) {
                        field = 1
                        typed = ""
                    }
                }
            }
        }
    }

    private var keys: some View {
        VStack(spacing: 8) {
            ForEach([["AC", "±", "⌫"], ["7", "8", "9"], ["4", "5", "6"], ["1", "2", "3"]], id: \.self) { row in
                HStack(spacing: 7) {
                    ForEach(row, id: \.self) { k in
                        key(k, fill: row[0] == "AC" ? topKey : digitKey, width: 71)
                    }
                }
            }
            HStack(spacing: 7) {
                key("0", fill: digitKey, width: 150)
                key(".", fill: digitKey, width: 71)
            }
        }
        .padding(.top, 14)
    }

    var body: some View {
        // beside the node, on its side of the box, level with it and its arrow pointing at it, kept
        // on the screen
        let whole = numericWidth + Pc.arrowHeight
        let at = open.at
        let left = min(
            max(open.right ? at.x + numericNodeGap : at.x - numericNodeGap - whole, 8),
            max(screen.width - whole - 8, 8)
        )
        let height = tall ?? 0
        let top = min(max(at.y - height / 2, 8), max(screen.height - height - 8, 8))
        ZStack(alignment: .topLeading) {
            Color.clear
                .contentShape(Rectangle())
                .onTapGesture { tool.numeric = nil }
            Popover(
                width: numericWidth,
                arrowX: min(max(at.y - top, 28), max(height - 28, 28)),
                corner: 20,
                side: open.right ? .left : .right
            ) {
                VStack(alignment: .leading, spacing: 0) {
                    Text(verbatim: open.angle ? "Rotation" : "Dimensions")
                        .font(.system(size: 20, weight: .bold))
                        .foregroundStyle(keyInk)
                        .padding(.leading, 10)
                        .padding(.vertical, 12)
                    fields
                    keys
                }
                .padding(.horizontal, 10)
                .padding(.bottom, 10)
            }
            .background {
                GeometryReader { place in
                    Color.clear.onAppear { tall = place.size.height }
                }
            }
            .contentShape(Rectangle())
            .onTapGesture {}
            .opacity(tall == nil ? 0 : 1)
            .offset(x: left, y: top)
        }
    }
}
