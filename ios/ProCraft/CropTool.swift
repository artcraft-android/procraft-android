import SwiftUI

private func themed(_ dark: UInt32, _ light: UInt32) -> Color {
    Color(argb: Look.shared.light ? light : dark)
}

private var headerLabel: Color { themed(0xFFC5_C5C5, 0xFF3A_3A3A) }
private var buttonLabel: Color { themed(0xFFA5_A5A5, 0xFF56_5656) }
private var chosenLabel: Color { themed(0xFFC0_C0C0, 0xFF3A_3A3A) }
private var capsuleChosen: Color { themed(0xFF46_4646, 0xFFA5_A5A5) }
private var headerBand: Color { themed(0xFF1D_1D1D, 0xFFDF_E2E3) }
private var headerCapsule: Color { themed(0xFF1A_1A1A, 0xFFF5_F8F9) }
private var headerEdge: Color { themed(0xFF37_3737, 0xFFDC_DEDF) }
private let doneBlue = Color(argb: 0xFF01_90FF)

// light over the dark surround; dark with the light interface
private var frameLine: Color { themed(0xCCFF_FFFF, 0xCC30_3030) }
private var gridLine: Color { themed(0x66C8_C8C8, 0x9960_6060) }
private var mark: Color { themed(0xFFD0_D0D0, 0xFF3A_3A3A) }
// Settings: dark, or with the light interface #dbdedf behind #e4e4e6 groups, #d3d3d5 fields, dark words
private var popoverFill: Color { themed(0xF21F_1F1F, 0xF2DB_DEDF) }
private var fieldFill: Color { themed(0xFF24_2424, 0xFFD3_D3D5) }
private var groupFill: Color { themed(0xFF2A_2A2A, 0xFFE4_E4E6) }
private var rowLine: Color { themed(0xFF3A_3A3A, 0xFFD6_D6D8) }
private var settingsInk: Color { themed(0xFFD2_D2D2, 0xFF3B_3B3B) }
private var rotationTrack: Color { themed(0xFF55_5555, 0xFFCF_CFD1) }
private let muted = Color(argb: 0xFF8E_8E93)

/// A button in a tool header's capsule: its text, shown chosen, and what it does.
struct ToolHeaderButton {
    let label: String
    var chosen = false
    let action: () -> Void
}

/// The header full-screen tools share: the title on the left; on the right a capsule of text buttons
/// and the round blue Done.
struct ToolHeader: View {
    let title: String
    let buttons: [ToolHeaderButton]
    let onDone: () -> Void

    var body: some View {
        ZStack(alignment: .topLeading) {
            Text(verbatim: title)
                .font(.system(size: 22, weight: .bold))
                .foregroundStyle(headerLabel)
                .lineLimit(1)
                .padding(.leading, 30)
                .padding(.top, 28)
            HStack(spacing: 13) {
                Spacer(minLength: 0)
                if !buttons.isEmpty {
                    HStack(spacing: 0) {
                        ForEach(buttons.indices, id: \.self) { i in
                            let b = buttons[i]
                            Text(verbatim: b.label)
                                .font(.system(size: 17))
                                .foregroundStyle(b.chosen ? chosenLabel : buttonLabel)
                                .lineLimit(1)
                                .padding(.horizontal, 13)
                                .frame(height: 43)
                                .background(Capsule().fill(b.chosen ? capsuleChosen : Color.clear))
                                .contentShape(Capsule())
                                .onTapGesture(perform: b.action)
                        }
                    }
                    .background(Capsule().fill(headerCapsule))
                    .overlay(Capsule().strokeBorder(headerEdge, lineWidth: 1))
                }
                PcIconView(PcIcons.doneTick, size: 40, color: .white)
                    .frame(width: 44, height: 44)
                    .background(Circle().fill(doneBlue))
                    .contentShape(Circle())
                    .onTapGesture(perform: onDone)
            }
            .padding(.top, 20)
            .padding(.trailing, 20)
        }
        .frame(maxWidth: .infinity)
        .frame(height: 85, alignment: .top)
        .background(headerBand)
        .contentShape(Rectangle())
        .onTapGesture {}
    }
}

/// Crop and Resize as the core has it: the core edits it, this shows it.
@Observable
final class CropTool {
    @ObservationIgnored private unowned let model: CanvasModel

    /// What the core has while it is open.
    private(set) var shown: CropShown?
    var settingsOpen = false

    init(model: CanvasModel) {
        self.model = model
    }

    private func read() {
        shown = model.session.cropShown()
    }

    /// The canvas fitted to the screen and shrunk, in the middle, so the frame's handles are clear.
    func begin() {
        model.closePanels()
        model.session.cropBegin()
        model.refreshView()
        model.followView()
        read()
        settingsOpen = false
    }

    func press(_ at: CGPoint) {
        model.session.cropPress(x: Float(at.x * model.pixels), y: Float(at.y * model.pixels))
    }

    func drag(_ at: CGPoint) {
        model.session.cropDrag(x: Float(at.x * model.pixels), y: Float(at.y * model.pixels))
        read()
    }

    func release() {
        model.session.cropRelease()
    }

    func change(_ apply: (CanvasSession) -> Void) {
        apply(model.session)
        read()
    }

    func end(keep: Bool) {
        guard shown != nil else { return }
        shown = nil
        settingsOpen = false
        // either way the canvas is fitted to the screen again: cancelled, growing back into place
        let changed = model.session.cropEnd(keep: keep)
        // a crop the GPU couldn't make: the canvas is as it was, and says so
        if let failure = model.session.takeFailure() {
            model.showNotice(failure, warning: true)
        }
        if !keep {
            model.refreshView()
            model.followView()
            return
        }
        model.refresh()
        model.refreshView()
        if changed, model.panel == .layers {
            model.refreshThumbnails()
        }
    }
}

/// Crop and Resize: the canvas, fitted inside a margin, with a frame over it in quarters whose
/// corners and sides are dragged to change what is kept (inside it, to move it); the header with
/// Cancel, Settings, Reset and Done.
struct CropOverlay: View {
    let model: CanvasModel

    @State private var pressed = false

    /// The frame's corners in the window, turned with the crop.
    private func corners(_ c: CropShown) -> [CGPoint] {
        let x = CGFloat(c.x)
        let y = CGFloat(c.y)
        let w = CGFloat(c.width)
        let h = CGFloat(c.height)
        let centre = CGPoint(x: x + w / 2, y: y + h / 2)
        let s = sin(CGFloat(c.angle))
        let k = cos(CGFloat(c.angle))
        return [CGPoint(x: x, y: y), CGPoint(x: x + w, y: y), CGPoint(x: x + w, y: y + h), CGPoint(x: x, y: y + h)].map { p in
            let dx = p.x - centre.x
            let dy = p.y - centre.y
            return model.canvasToWindow(CGPoint(x: centre.x + dx * k - dy * s, y: centre.y + dx * s + dy * k))
        }
    }

    private func along(_ a: CGPoint, _ b: CGPoint, _ t: CGFloat) -> CGPoint {
        CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t)
    }

    private func unit(_ from: CGPoint, _ to: CGPoint) -> CGPoint {
        let d = max(hypot(to.x - from.x, to.y - from.y), 1)
        return CGPoint(x: (to.x - from.x) / d, y: (to.y - from.y) / d)
    }

    private func draw(_ context: GraphicsContext, _ cs: [CGPoint]) {
        // quarters
        var grid = Path()
        for i in 1 ... 3 {
            let f = CGFloat(i) / 4
            grid.move(to: along(cs[0], cs[1], f))
            grid.addLine(to: along(cs[3], cs[2], f))
            grid.move(to: along(cs[0], cs[3], f))
            grid.addLine(to: along(cs[1], cs[2], f))
        }
        context.stroke(grid, with: .color(gridLine), lineWidth: 1)
        var frame = Path()
        frame.addLines(cs)
        frame.closeSubpath()
        context.stroke(frame, with: .color(frameLine), lineWidth: 1)
        // small marks at the corners and the sides' middles
        let arm: CGFloat = 10
        var marks = Path()
        for i in 0 ... 3 {
            let p = cs[i]
            let next = cs[(i + 1) % 4]
            let toNext = unit(p, next)
            let toPrev = unit(p, cs[(i + 3) % 4])
            marks.move(to: p)
            marks.addLine(to: CGPoint(x: p.x + toNext.x * arm, y: p.y + toNext.y * arm))
            marks.move(to: p)
            marks.addLine(to: CGPoint(x: p.x + toPrev.x * arm, y: p.y + toPrev.y * arm))
            let m = along(p, next, 0.5)
            marks.move(to: CGPoint(x: m.x - toNext.x * arm, y: m.y - toNext.y * arm))
            marks.addLine(to: CGPoint(x: m.x + toNext.x * arm, y: m.y + toNext.y * arm))
        }
        context.stroke(marks, with: .color(mark), lineWidth: 3)
    }

    var body: some View {
        let tool = model.crop
        if let shown = tool.shown {
            let cs = corners(shown)
            ZStack(alignment: .topTrailing) {
                Canvas { context, _ in
                    draw(context, cs)
                }
                .contentShape(Rectangle())
                // the core takes the part pressed and follows the drag (snapping, a locked aspect)
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { drag in
                            if !pressed {
                                pressed = true
                                tool.press(drag.startLocation)
                            }
                            tool.drag(drag.location)
                        }
                        .onEnded { _ in
                            pressed = false
                            tool.release()
                        }
                )
                ToolHeader(
                    title: "Crop and Resize",
                    buttons: [
                        ToolHeaderButton(label: "Cancel") { tool.end(keep: false) },
                        ToolHeaderButton(label: "Settings", chosen: tool.settingsOpen) { tool.settingsOpen.toggle() },
                        ToolHeaderButton(label: "Reset") { tool.begin() },
                    ],
                    onDone: { tool.end(keep: true) }
                )
                // 9 from the screen's right edge, its arrow up at Settings (183 from that edge)
                if tool.settingsOpen {
                    CropSettings(tool: tool, shown: shown)
                        .offset(x: -9, y: 66)
                }
            }
        }
    }
}

/// The settings popover: the size (aspect locked or not), DPI, resampling, snapping, the turn.
private struct CropSettings: View {
    let tool: CropTool
    let shown: CropShown

    private func switchRow(_ label: String, on: Bool, change: @escaping () -> Void) -> some View {
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 17))
                .foregroundStyle(settingsInk)
            Spacer(minLength: 0)
            PcSwitch(on: on)
        }
        .padding(.leading, 12)
        .padding(.trailing, 6)
        .frame(height: 44)
        .contentShape(Rectangle())
        .onTapGesture(perform: change)
    }

    var body: some View {
        let group = RoundedRectangle(cornerRadius: 14, style: .circular)
        Popover(width: 348, arrowX: 174.5, fill: popoverFill, corner: 24) {
            VStack(alignment: .leading, spacing: 10) {
                Text(verbatim: "Settings")
                    .font(.system(size: 22, weight: .bold))
                    .foregroundStyle(settingsInk)
                    .padding(.leading, 14)
                    .frame(height: 40)
                HStack(spacing: 0) {
                    NumberField(value: "\(shown.sizeWidth)", unit: "px") { text in
                        if let v = UInt32(text) {
                            tool.change { $0.cropSetSize(width: max(v, 1), height: nil) }
                        }
                    }
                    .frame(width: 137)
                    PcIconView(PcIcons.link, size: 24, color: shown.aspectLocked ? Pc.accent : settingsInk)
                        .frame(width: 34, height: 40)
                        .contentShape(Rectangle())
                        .onTapGesture { tool.change { $0.cropSetAspectLocked(on: !shown.aspectLocked) } }
                        .padding(.horizontal, 8)
                    NumberField(value: "\(shown.sizeHeight)", unit: "px") { text in
                        if let v = UInt32(text) {
                            tool.change { $0.cropSetSize(width: nil, height: max(v, 1)) }
                        }
                    }
                    .frame(width: 137)
                }
                .padding(.horizontal, 2)
                .frame(height: 44)
                VStack(spacing: 0) {
                    HStack(spacing: 0) {
                        Text(verbatim: "DPI")
                            .font(.system(size: 17))
                            .foregroundStyle(settingsInk)
                        Spacer(minLength: 0)
                        NumberField(value: String(format: "%.1f", shown.dpi), unit: nil) { text in
                            if let v = Float(text) {
                                tool.change { $0.cropSetDpi(dpi: v) }
                            }
                        }
                        .frame(width: 80, height: 36)
                    }
                    .padding(.leading, 12)
                    .padding(.trailing, 4)
                    .frame(height: 44)
                    Rectangle().fill(rowLine).frame(height: 1)
                    switchRow("Resample canvas", on: shown.resample) { tool.change { $0.cropSetResample(on: !shown.resample) } }
                    Rectangle().fill(rowLine).frame(height: 1)
                    switchRow("Snapping", on: shown.snapping) { tool.change { $0.cropSetSnapping(on: !shown.snapping) } }
                }
                .background(group.fill(groupFill))
                .clipShape(group)
                RotationSlider(radians: shown.angle) { v in tool.change { $0.cropSetAngle(radians: v) } }
                    .frame(height: 64)
                    .background(group.fill(groupFill))
            }
            .padding(10)
        }
        .contentShape(Rectangle())
        .onTapGesture {}
    }
}

/// A number typed into a capsule, its unit after it; done as it is typed.
private struct NumberField: View {
    let value: String
    let unit: String?
    let onChange: (String) -> Void

    @State private var text = ""

    var body: some View {
        HStack(spacing: 0) {
            TextField("", text: $text)
                .keyboardType(.decimalPad)
                .multilineTextAlignment(.center)
                .font(.system(size: 17))
                .foregroundStyle(settingsInk)
                .tint(.white)
                .onChange(of: text) { _, typed in
                    let kept = String(typed.filter { $0.isNumber || $0 == "." }.prefix(7))
                    if kept != typed {
                        text = kept
                    } else if kept != value {
                        onChange(kept)
                    }
                }
            if let unit {
                Text(verbatim: " \(unit)")
                    .font(.system(size: 17))
                    .foregroundStyle(settingsInk)
            }
        }
        .padding(.horizontal, 12)
        .frame(height: 40)
        .background(Capsule().fill(fieldFill))
        .onChange(of: value, initial: true) { _, next in
            text = next
        }
    }
}

/// The turn, -180 to 180 degrees, dragged anywhere along the bar; a tick marks no turn.
private struct RotationSlider: View {
    let radians: Float
    let onChange: (Float) -> Void

    var body: some View {
        let degrees = radians * 180 / .pi
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                Text(verbatim: "Rotation")
                Spacer(minLength: 0)
                Text(verbatim: String(format: "%.1f°", degrees))
            }
            .font(.system(size: 17))
            .foregroundStyle(settingsInk)
            GeometryReader { g in
                let w = g.size.width
                let x = min(max(CGFloat((degrees + 180) / 360) * w, 13), max(w - 13, 13))
                ZStack(alignment: .leading) {
                    Capsule().fill(rotationTrack).frame(height: 3)
                    Rectangle().fill(muted).frame(width: 1, height: 12).offset(x: w / 2)
                    Capsule().fill(Color.white).frame(width: 26, height: 15).offset(x: x - 13)
                }
                .frame(width: w, height: 22)
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
                    let turn = Float(min(max(drag.location.x / max(w, 1), 0), 1)) * 360 - 180
                    onChange((abs(turn) < 2 ? 0 : turn) * .pi / 180)
                })
            }
            .frame(height: 22)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
    }
}
