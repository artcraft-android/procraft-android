import SwiftUI

private func themed(_ dark: UInt32, _ light: UInt32) -> Color {
    Color(argb: Look.shared.light ? light : dark)
}

private var panelFill: Color { themed(0xF71E_1E1E, 0xF7F1_F1F1) }
private var band: Color { themed(0xFF53_5353, 0xFFFF_FFFF) }
private var plusDisc: Color { themed(0xFF1B_1B1B, 0xFFFB_FBFB) }
private var plusRim: Color { themed(0xFF37_3737, 0xFFE1_E1E1) }
private var plusGlyph: Color { themed(0xFFAA_AAAA, 0xFF59_5959) }
private var editorCard: Color { themed(0xFF38_3838, 0xFFEE_EEEE) }
private var editorName: Color { themed(0xFFC5_C5C5, 0xFF3A_3A3A) }
private var card: Color { themed(0xFF28_2828, 0xFFEE_EEEE) }
private var cardName: Color { themed(0xFFC1_C1C1, 0xFF5D_5D5D) }
private let doneFill = Color(argb: 0xFF01_90FF)
private let stopSize: CGFloat = 42
private let gradientSpace = "gradientPanel"
/// How long a stop or card stays pressed to be held, seconds.
private let holdWait = 0.4

/// A gradient's colours left to right, over `from`..`to` of the width (0..1).
private func gradientFill(_ g: GradientInfo, from: CGFloat = 0, to: CGFloat = 1) -> LinearGradient {
    let stops = g.stops.sorted { $0.position < $1.position }.map { stop in
        Gradient.Stop(color: Color(argb: 0xFF00_0000 | stop.rgb), location: from + CGFloat(stop.position) * (to - from))
    }
    return LinearGradient(stops: stops, startPoint: .leading, endPoint: .trailing)
}

/// The gradient library: the core keeps it, written at each change; the adjustment maps by the
/// chosen one.
@Observable
final class GradientMapTool {
    @ObservationIgnored private unowned let model: CanvasModel
    @ObservationIgnored private let library: Gradients

    private(set) var gradients: [GradientInfo]
    /// The gradient being edited, by its place in `gradients`.
    private(set) var editing: Int?
    /// A stop's colour being chosen: the stop, and what shows it.
    private(set) var stopColour: (stop: Int, held: HeldColor, at: CGPoint)?

    init(model: CanvasModel) {
        self.model = model
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory())
        let opened = Gradients.open(path: support.appendingPathComponent("gradients.json").path)
        library = opened
        gradients = opened.list()
        model.session.useGradients(gradients: opened)
    }

    /// The gradient mapped by.
    var index: Int {
        Int(model.adjustShown?.gradient ?? 0)
    }

    private func apply(_ i: Int) {
        let shown = min(max(i, 0), max(gradients.count - 1, 0))
        model.adjust { $0.adjustmentGradient(index: UInt32(shown)) }
    }

    func choose(_ i: Int) {
        apply(i)
    }

    /// A new gradient, black to white, just after the chosen one: chosen and opened to edit.
    func addNew() {
        let at = Int(library.addAfter(after: UInt32(index)))
        gradients = library.list()
        editing = at
        apply(at)
    }

    /// Opens gradient `i` (chosen) to edit.
    func edit(_ i: Int) {
        apply(i)
        editing = i
    }

    func endEdit() {
        editing = nil
        stopColour = nil
    }

    func delete(_ i: Int) {
        guard gradients.count > 1, gradients.indices.contains(i) else { return }
        let chosen = index
        library.delete(index: UInt32(i))
        gradients = library.list()
        apply(chosen > i || chosen >= gradients.count ? chosen - 1 : chosen)
    }

    /// A copy of gradient `i` just after it, chosen.
    func duplicate(_ i: Int) {
        guard gradients.indices.contains(i) else { return }
        let at = Int(library.duplicate(index: UInt32(i)))
        gradients = library.list()
        apply(at)
    }

    /// The gradient being edited changed by `f`, and mapped by again.
    private func changeEdited(_ f: (UInt32) -> Void) {
        guard let i = editing, gradients.indices.contains(i) else { return }
        f(UInt32(i))
        gradients = library.list()
        apply(i)
    }

    func rename(_ name: String) {
        changeEdited { library.rename(index: $0, name: name) }
    }

    func addStop(_ position: Float) {
        changeEdited { _ = library.addStop(index: $0, position: position) }
    }

    func moveStop(_ i: Int, _ position: Float) {
        changeEdited { library.moveStop(index: $0, stop: UInt32(i), position: position) }
    }

    /// Takes a stop out; a gradient keeps two.
    func deleteStop(_ i: Int) {
        changeEdited { library.deleteStop(index: $0, stop: UInt32(i)) }
    }

    /// Opens the colours for stop `i`, their arrow on the window's point `at`, each change mapped by
    /// at once.
    func colour(_ i: Int, at: CGPoint) {
        guard let e = editing, gradients.indices.contains(e), gradients[e].stops.indices.contains(i) else { return }
        let held = HeldColor(rgb: gradients[e].stops[i].rgb) { [weak self] rgb in
            self?.changeEdited { self?.library.setStopColour(index: $0, stop: UInt32(i), rgb: rgb) }
        }
        stopColour = (i, held, at)
    }

    func endColour() {
        stopColour = nil
    }
}

/// Gradient Map's panel: the library (a card tapped maps by it, tapped again opens it to edit, a
/// hold offers Delete and Duplicate, "+" makes a new one), or the gradient being edited.
struct GradientMapPanel: View {
    let model: CanvasModel
    let wide: Bool

    var body: some View {
        let tool = model.gradientMap
        let editing = tool.editing
        // wider on a wide screen, showing more of the library
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                Text(verbatim: editing != nil ? "Gradient Map" : "Gradient Library")
                    .font(.system(size: 20, weight: .bold))
                    .foregroundStyle(ToolbarInk.menuText)
                    .lineLimit(1)
                Spacer(minLength: 0)
                if editing != nil {
                    PcIconView(PcIcons.doneTick, size: 36, color: .white)
                        .frame(width: 40, height: 40)
                        .background(Circle().fill(doneFill))
                        .contentShape(Circle())
                        .onTapGesture { tool.endEdit() }
                } else {
                    PcIconView(PcIcons.add, size: 34, color: plusGlyph)
                        .frame(width: 40, height: 40)
                        .background(Circle().fill(plusDisc))
                        .overlay(Circle().strokeBorder(plusRim, lineWidth: 1))
                        .contentShape(Circle())
                        .onTapGesture { tool.addNew() }
                }
            }
            .frame(height: 40)
            if let editing, tool.gradients.indices.contains(editing) {
                GradientEditor(tool: tool, gradient: tool.gradients[editing], index: editing)
            } else {
                GradientCards(tool: tool)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .frame(width: wide ? 835 : 544, height: 159, alignment: .top)
        .background(RoundedRectangle(cornerRadius: 33, style: .circular).fill(panelFill))
        .contentShape(Rectangle())
        .onTapGesture {}
        .coordinateSpace(.named(gradientSpace))
        .offset(y: -25)
    }
}

/// The library's cards on a grey band: each dark, its name over a strip of it; the chosen one
/// stands taller.
private struct GradientCards: View {
    let tool: GradientMapTool

    /// The card held, and where its middle is in the panel.
    @State private var held: Int?
    @State private var heldAt: CGFloat = 0

    private func cardView(_ i: Int) -> some View {
        let g = tool.gradients[i]
        let chosen = tool.index == i
        return VStack(alignment: .leading, spacing: 0) {
            Spacer(minLength: 0)
            Text(verbatim: g.name)
                .font(.system(size: 12))
                .foregroundStyle(chosen ? Color.white : cardName)
                .lineLimit(1)
                .padding(.leading, 11)
                .padding(.bottom, 10)
            Rectangle()
                .fill(gradientFill(g))
                .frame(height: 13)
        }
        .frame(width: 108, height: chosen ? 69 : 65)
        .background(card)
        .clipShape(RoundedRectangle(cornerRadius: 9, style: .circular))
        .padding(.bottom, 5.5)
        .background {
            GeometryReader { place in
                Color.clear.onChange(of: held) { _, now in
                    if now == i {
                        heldAt = place.frame(in: .named(gradientSpace)).midX
                    }
                }
            }
        }
        .contentShape(Rectangle())
        .onTapGesture {
            if chosen {
                tool.edit(i)
            } else {
                tool.choose(i)
            }
        }
        .onLongPressGesture(minimumDuration: holdWait) { held = i }
    }

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(alignment: .bottom, spacing: 6) {
                ForEach(tool.gradients.indices, id: \.self) { i in
                    cardView(i)
                }
            }
            .padding(.horizontal, 6)
            .frame(height: 81, alignment: .bottom)
        }
        .frame(height: 81)
        .background(RoundedRectangle(cornerRadius: 12, style: .circular).fill(band))
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .circular))
        .padding(.top, 11)
        // a hold: Delete and Duplicate above the card
        .overlay(alignment: .topLeading) {
            if let i = held {
                ZStack(alignment: .topLeading) {
                    MenuDismissal { held = nil }
                    GlassMenu(overPanel: true) {
                        MenuRow(icon: PcIcons.menuTrash, label: "Delete", ink: Pc.menuDanger) {
                            held = nil
                            tool.delete(i)
                        }
                        MenuRow(icon: PcIcons.menuDuplicate, label: "Duplicate") {
                            held = nil
                            tool.duplicate(i)
                        }
                    }
                    .offset(x: heldAt - 14 - glassMenuWidth / 2, y: -112)
                }
            }
        }
    }
}

/// The gradient being edited: its name to rename, over its bar, with a disc for each stop. A tap on
/// the bar adds a stop; on a stop, its colours open. A stop drags along the bar; a hold offers
/// Delete.
private struct GradientEditor: View {
    let tool: GradientMapTool
    let gradient: GradientInfo
    let index: Int

    @State private var name: String
    /// The stop a touch took, whether it moved, and when it became a hold.
    @State private var dragged: Int?
    @State private var moved = false
    @State private var touching = false
    @State private var wasHeld = false
    @State private var holdTimer: Timer?
    /// The stop whose Delete shows, and where it is along the bar.
    @State private var holding: Int?

    init(tool: GradientMapTool, gradient: GradientInfo, index: Int) {
        self.tool = tool
        self.gradient = gradient
        self.index = index
        _name = State(initialValue: gradient.name)
    }

    private var r: CGFloat {
        stopSize / 2
    }

    /// A stop's centre along a bar `width` wide.
    private func xOf(_ p: Float, _ width: CGFloat) -> CGFloat {
        r + CGFloat(p) * (width - 2 * r)
    }

    private func position(_ x: CGFloat, _ width: CGFloat) -> Float {
        Float(min(max((x - r) / max(width - 2 * r, 1), 0), 1))
    }

    /// The stop under a point along the bar.
    private func stop(at x: CGFloat, _ width: CGFloat) -> Int? {
        gradient.stops.indices
            .filter { abs(xOf(gradient.stops[$0].position, width) - x) <= r }
            .min { abs(xOf(gradient.stops[$0].position, width) - x) < abs(xOf(gradient.stops[$1].position, width) - x) }
    }

    private func touched(_ drag: DragGesture.Value, _ width: CGFloat) {
        if !touching {
            touching = true
            moved = false
            wasHeld = false
            dragged = stop(at: drag.startLocation.x, width)
            let hit = dragged
            holdTimer = Timer.scheduledTimer(withTimeInterval: holdWait, repeats: false) { _ in
                wasHeld = true
                holding = hit
            }
        }
        guard !wasHeld else { return }
        if !moved, hypot(drag.translation.width, drag.translation.height) < 8 {
            return
        }
        moved = true
        holdTimer?.invalidate()
        if let dragged {
            tool.moveStop(dragged, position(drag.location.x, width))
        }
    }

    /// The top of stop `i` on the window, the bar `width` wide and starting at `origin`.
    private func top(_ i: Int, _ width: CGFloat, _ origin: CGPoint) -> CGPoint {
        CGPoint(x: origin.x + xOf(gradient.stops[i].position, width), y: origin.y + 22 - stopSize / 2)
    }

    private func letGo(_ drag: DragGesture.Value, _ width: CGFloat, _ origin: CGPoint) {
        holdTimer?.invalidate()
        touching = false
        defer { dragged = nil }
        guard !moved, !wasHeld else { return }
        if let dragged {
            tool.colour(dragged, at: top(dragged, width, origin))
        } else {
            tool.addStop(position(drag.startLocation.x, width))
        }
    }

    private func bar(_ width: CGFloat, origin: CGPoint) -> some View {
        let stops = gradient.stops
        return UnevenRoundedRectangle(bottomLeadingRadius: 12, bottomTrailingRadius: 12, style: .circular)
            .fill(gradientFill(gradient, from: r / width, to: 1 - r / width))
            .overlay {
                Canvas { context, size in
                    for stop in stops {
                        let rgb = stop.rgb
                        let red = Float((rgb >> 16) & 0xFF) / 255
                        let green = Float((rgb >> 8) & 0xFF) / 255
                        let blue = Float(rgb & 0xFF) / 255
                        // the ring stands out from what it rings: white round a dark colour, black
                        // round a light one
                        let light = 0.299 * red + 0.587 * green + 0.114 * blue > 0.45
                        let x = xOf(stop.position, size.width)
                        let y = size.height / 2
                        context.fill(
                            Path(ellipseIn: CGRect(x: x - r, y: y - r, width: r * 2, height: r * 2)),
                            with: .color(light ? .black : .white)
                        )
                        let inner = r - 2
                        context.fill(
                            Path(ellipseIn: CGRect(x: x - inner, y: y - inner, width: inner * 2, height: inner * 2)),
                            with: .color(Color(argb: 0xFF00_0000 | rgb))
                        )
                    }
                }
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { touched($0, width) }
                    .onEnded { letGo($0, width, origin) }
            )
    }

    var body: some View {
        VStack(spacing: 0) {
            TextField("", text: $name)
                .font(.system(size: 14))
                .foregroundStyle(editorName)
                .tint(Pc.accent)
                .textFieldStyle(.plain)
                .padding(.horizontal, 14)
                .padding(.top, 8)
                .frame(height: 36)
                .onChange(of: name) { _, value in
                    tool.rename(String(value.prefix(64)))
                }
            Spacer(minLength: 0)
            GeometryReader { place in
                bar(place.size.width, origin: place.frame(in: .global).origin)
                    #if DEBUG
                    .onAppear {
                        guard ScreenCapture.stopOpens, gradient.stops.indices.contains(1) else { return }
                        // once the panel has come up
                        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
                            tool.colour(1, at: top(1, place.size.width, place.frame(in: .global).origin))
                        }
                    }
                    #endif
                    .overlay(alignment: .topLeading) {
                        if let i = holding, gradient.stops.indices.contains(i) {
                            ZStack(alignment: .topLeading) {
                                MenuDismissal { holding = nil }
                                GlassMenu(overPanel: true) {
                                    MenuRow(icon: PcIcons.menuTrash, label: "Delete", ink: Pc.menuDanger) {
                                        holding = nil
                                        tool.deleteStop(i)
                                    }
                                }
                                .offset(x: xOf(gradient.stops[i].position, place.size.width) - glassMenuWidth / 2, y: -60)
                            }
                        }
                    }
            }
            .frame(height: 44)
        }
        .frame(height: 81)
        .background(RoundedRectangle(cornerRadius: 12, style: .circular).fill(editorCard))
        .padding(.top, 11)
    }
}
