import SwiftUI
import UIKit

private let layersWidth: CGFloat = 384
private let rowHeight: CGFloat = 69
/// A layer with a mask, and its mask: their rows sit closer.
private let maskedRowHeight: CGFloat = 65
private let rowGap: CGFloat = 1
private let listCorner: CGFloat = 8
private let groupHeight: CGFloat = 44
private let groupIndent: CGFloat = 21
private let groupLine = Color(argb: 0xFF4E_4E4E)
private let maskOwnerRow = Color(argb: 0xFF2C_5281)
private let pickedRow = Color(argb: 0xFF2C_5281)
private let lockedLetter = Color(argb: 0xFF80_8080)
/// Blend modes shown by their first two letters (their first is another mode's).
private let twoLetterBlends: Set<String> = ["Darken", "Shade", "Lighten", "Subtract", "Divide", "Saturation"]
/// Swiping a row left shows these buttons, each this wide, this far apart.
private let swipeButton: CGFloat = 82
private let swipeGap: CGFloat = 2
private let swipeFill = Color(argb: 0xFF3F_3F3F)
private let swipeInk = Color(argb: 0xFFC1_C1C1)
private let swipeDelete = Color(argb: 0xFFFF_473D)
/// How far right a row slides to be picked.
private let pickReach: CGFloat = 72
/// The options never go higher than this from the screen's top (the top bar and a gap).
private let optionsTop: CGFloat = 57
/// The layer's blend list: nine rows showing, its glyphs a darker grey than its names.
private let blendRowHeight: CGFloat = 38.8
private let blendListHeight: CGFloat = 349
private let blendGlyphInk = Color(argb: 0xFF80_8080)
/// How long the blend modes take to open and close, and the rows under them to come back.
private let blendOpen = 0.25
private let blendClose = 0.1
private let blendRowsWait = 0.1
private let blendRowsBack = 0.12
/// How much of its finger's way a pinched row moves, and how close two rows are pinched to merge
/// (of how far apart they were), as measured.
private let squeezeFollow: CGFloat = 0.3
private let pinchMerge: CGFloat = 0.65

/// Two rows being pinched: each moved a share of its finger's way, the upper on top.
private struct Squeeze {
    let upper: UInt64
    let lower: UInt64
    let upperBy: CGFloat
    let lowerBy: CGFloat

    func offset(_ id: UInt64) -> CGFloat? {
        id == upper ? upperBy * squeezeFollow : id == lower ? lowerBy * squeezeFollow : nil
    }
}

/// What the panel's rows share while it is open.
@Observable
private final class LayersUi {
    /// The layer whose options are open, and whose blend modes.
    var options: UInt64?
    var blendFor: UInt64?
    /// The row the blend modes open under (-1 when nothing is moving), and how far the rows under
    /// it have come back.
    var blendAt = -1
    var belowShown = 1.0
    var renaming: UInt64?
    var swiped: UInt64?
    var titleMenu = false
    /// A row held and dragged, and how far.
    var dragId: UInt64?
    var dragOffset: CGFloat = 0
    var squeeze: Squeeze?
    /// Where each row is on the screen.
    @ObservationIgnored var rows: [UInt64: CGRect] = [:]
    @ObservationIgnored private var blendJob: DispatchWorkItem?

    /// The layer whose row is at `y` on the screen.
    func row(at y: CGFloat) -> UInt64? {
        rows.first { y >= $0.value.minY && y <= $0.value.maxY }?.key
    }

    /// Blend modes opening under a layer: the rows under it go at once and the panel grows over
    /// where they were; closing, it shrinks back to them and they come back a moment later.
    func changeBlend(_ id: UInt64?, shown: [LayerInfo]) {
        guard id != blendFor else { return }
        let places = [blendFor, id].compactMap { $0 }.compactMap { b in shown.firstIndex { $0.id == b } }
        let opening = id != nil
        blendAt = places.min() ?? -1
        belowShown = 0
        withAnimation(Motion.easeInOut(opening ? blendOpen : blendClose)) {
            blendFor = id
        }
        blendJob?.cancel()
        let job = DispatchWorkItem { [weak self] in
            withAnimation(Motion.easeInOut(blendRowsBack)) {
                self?.belowShown = 1
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + blendRowsBack) {
                self?.blendAt = -1
            }
        }
        blendJob = job
        DispatchQueue.main.asyncAfter(deadline: .now() + (opening ? blendOpen : blendClose + blendRowsWait), execute: job)
    }

    /// Lets `l` go where its row's middle came to rest: just above or below the row it is over.
    func drop(_ model: CanvasModel, _ l: LayerInfo) {
        let y = (rows[l.id]?.midY ?? 0) + dragOffset
        dragId = nil
        dragOffset = 0
        let unit = model.movingWith(l)
        let others = model.layers.filter { !unit.contains($0.id) && !model.hidden($0) }.compactMap { o in
            rows[o.id].map { (layer: o, row: $0) }
        }
        let over = others.first { y >= $0.row.minY && y < $0.row.maxY }
            ?? others.min { abs($0.row.midY - y) < abs($1.row.midY - y) }
        if let over {
            model.moveNextTo(l, over.layer, above: y < over.row.midY)
        }
    }
}

private struct LayerFrames: PreferenceKey {
    static let defaultValue: [UInt64: CGRect] = [:]
    static func reduce(value: inout [UInt64: CGRect], nextValue: () -> [UInt64: CGRect]) {
        value.merge(nextValue()) { $1 }
    }
}

/// The layers panel: top layer first (a closed group's left out), each its thumbnail, name, blend
/// mode's letters and visibility box, and the background colour's row under them. Tap a layer to
/// make it current, tap the current one for its options, tap the blend letter for blend mode and
/// opacity, hold and drag to reorder, swipe left for lock, duplicate and delete, swipe right to
/// pick it to act on with the current one (the title's menu then groups, merges, shares,
/// duplicates or deletes them). Two fingers swiped right on a row set its alpha lock, held there
/// select what is on it, and pinching two rows merges them and every layer between.
struct LayersPanel: View {
    let model: CanvasModel
    let arrowX: CGFloat
    /// The most the panel may be tall: its list scrolls past that.
    let maxHeight: CGFloat
    /// How tall the rows are together.
    @State private var listHeight: CGFloat = 0
    @State private var ui = LayersUi()

    var body: some View {
        let shown = model.layers.filter { !model.hidden($0) }
        let ids = shown.map(\.id)
        Popover(width: layersWidth, arrowX: arrowX) {
            PanelHeader(
                title: "Layers",
                chevronOn: model.anyPicked,
                onTitle: { withAnimation(Motion.menuOpen) { ui.titleMenu.toggle() } }
            ) {
                PcIconView(PcIcons.plusMedium, size: 41, color: Pc.panelGlyph)
                    .frame(width: 40, height: 41)
                    .contentShape(Rectangle())
                    .onTapGesture { model.addLayer() }
                    .padding(.top, 12)
            }
            ScrollViewReader { reader in
                ScrollView {
                    VStack(spacing: rowGap) {
                        ForEach(Array(shown.enumerated()), id: \.element.id) { index, l in
                            LayerItem(model: model, ui: ui, layer: l, index: index, shown: shown)
                                .id(l.id)
                        }
                        BackgroundRow(model: model)
                            .padding(.top, 10)
                            .opacity(ui.blendAt >= 0 ? ui.belowShown : 1)
                    }
                    .animation(Motion.rowMove, value: ids)
                    .padding(.horizontal, 12)
                    .padding(.bottom, 12)
                    .background {
                        GeometryReader { rows in
                            Color.clear
                                .onAppear { listHeight = rows.size.height }
                                .onChange(of: rows.size.height) { _, height in listHeight = height }
                        }
                    }
                }
                .scrollIndicators(.hidden)
                .scrollDisabled(ui.dragId != nil)
                // a layer added at the top of what shows comes in there
                .onChange(of: ids) { before, now in
                    if let added = now.first(where: { !before.contains($0) }) {
                        withAnimation(Motion.rowMove) { reader.scrollTo(added, anchor: .top) }
                    }
                }
            }
            // as tall as its rows, up to what the screen leaves under the header
            .frame(height: max(0, min(listHeight, maxHeight - 64 - Pc.arrowHeight)))
        }
        .background(RowFingers(model: model, ui: ui).frame(width: 0, height: 0))
        .onPreferenceChange(LayerFrames.self) { ui.rows = $0 }
        .overlay(alignment: .topLeading) { LayersMenus(model: model, ui: ui, maxHeight: maxHeight) }
        #if DEBUG
        .onAppear {
            guard let current = model.layers.first(where: { $0.current }) else { return }
            switch ScreenCapture.layersShows {
            case "swipe":
                DispatchQueue.main.asyncAfter(deadline: .now() + 1) { ui.swiped = current.id }
            case "options":
                DispatchQueue.main.asyncAfter(deadline: .now() + 1) { ui.options = current.id }
            case "blend":
                ui.changeBlend(current.id, shown: shown)
            case "menu":
                ui.titleMenu = true
            case "rename":
                ui.renaming = current.id
            default:
                break
            }
        }
        #endif
    }
}

/// A row of the list: the layer or group, what it swipes to show, and the blend modes opening
/// under it.
private struct LayerItem: View {
    let model: CanvasModel
    let ui: LayersUi
    let layer: LayerInfo
    let index: Int
    let shown: [LayerInfo]

    private func tapped() {
        let l = layer
        ui.swiped = nil
        if model.anyPicked {
            // picks give way to the layer tapped, the current one too
            model.selectLayer(l.id)
            ui.options = nil
        } else if l.current {
            withAnimation(Motion.menuOpen) {
                ui.options = ui.options == l.id ? nil : l.id
            }
            ui.changeBlend(nil, shown: shown)
        } else {
            model.selectLayer(l.id)
            ui.options = nil
        }
    }

    var body: some View {
        let l = layer
        let dragging = ui.dragId == l.id
        // the list's ends are rounded
        let first = index == 0
        let last = index == shown.count - 1
        let shape = UnevenRoundedRectangle(
            topLeadingRadius: first ? listCorner : 0, bottomLeadingRadius: last ? listCorner : 0,
            bottomTrailingRadius: last ? listCorner : 0, topTrailingRadius: first ? listCorner : 0,
            style: .circular
        )
        let renaming = ui.renaming == l.id
        let onRenamed: (String) -> Void = {
            model.rename(l, $0)
            ui.renaming = nil
        }
        VStack(spacing: 0) {
            LayerSwipe(
                model: model, layer: l, open: ui.swiped == l.id, enabled: ui.dragId == nil,
                onOpen: { ui.swiped = $0 ? l.id : nil },
                onPick: { model.pickLayer(l) }
            ) {
                if l.kind == .group {
                    GroupRow(model: model, layer: l, shape: shape, renaming: renaming, onRenamed: onRenamed, onTap: tapped)
                } else {
                    LayerRow(
                        model: model, layer: l, shape: shape,
                        // the current mask's layer, or the current layer's mask
                        owner: model.layers.contains {
                            $0.current && (($0.kind == .mask && $0.parent == l.id) || (l.kind == .mask && l.parent == $0.id))
                        },
                        renaming: renaming, onRenamed: onRenamed, onTap: tapped,
                        onBlend: {
                            ui.changeBlend(ui.blendFor == l.id ? nil : l.id, shown: shown)
                            ui.options = nil
                        }
                    )
                }
            }
            if ui.blendFor == l.id {
                BlendEditor(model: model, layer: l)
                    .transition(.opacity)
            }
        }
        .offset(y: dragging ? ui.dragOffset : ui.squeeze?.offset(l.id) ?? 0)
        .opacity(ui.blendAt >= 0 && ui.blendAt < index ? ui.belowShown : 1)
        .background(GeometryReader { g in
            Color.clear.preference(key: LayerFrames.self, value: [l.id: g.frame(in: .global)])
        })
        .zIndex(dragging || ui.squeeze?.upper == l.id ? 1 : 0)
        .modifier(RowDrag(
            enabled: l.kind != .mask,
            began: {
                ui.dragId = l.id
                ui.dragOffset = 0
                ui.swiped = nil
            },
            moved: { ui.dragOffset = $0 },
            ended: { ui.drop(model, l) }
        ))
    }
}

/// A row held still, then dragged up or down: `ended` hears that it is over, let go or taken away
/// by the system alike (a gesture's state goes back either way).
private struct RowDrag: ViewModifier {
    let enabled: Bool
    let began: () -> Void
    let moved: (CGFloat) -> Void
    let ended: () -> Void

    @GestureState private var held = false
    @State private var started = false

    func body(content: Content) -> some View {
        content
            .simultaneousGesture(
                LongPressGesture(minimumDuration: 0.5)
                    .sequenced(before: DragGesture(minimumDistance: 0, coordinateSpace: .global))
                    .updating($held) { value, state, _ in
                        if case .second(true, _) = value {
                            state = true
                        }
                    }
                    .onChanged { value in
                        if case let .second(true, drag?) = value {
                            if !started {
                                started = true
                                began()
                            }
                            moved(drag.translation.height)
                        }
                    },
                including: enabled ? .all : .subviews
            )
            .onChange(of: held) { _, now in
                if !now, started {
                    started = false
                    ended()
                }
            }
    }
}

/// A row that slides left to show Lock, Duplicate (not for a mask) and Delete, or a little right to
/// pick it, springing back.
private struct LayerSwipe<Content: View>: View {
    let model: CanvasModel
    let layer: LayerInfo
    let open: Bool
    let enabled: Bool
    let onOpen: (Bool) -> Void
    let onPick: () -> Void
    @ViewBuilder let content: () -> Content

    @State private var offset: CGFloat = 0
    /// The touch is going along the row, not down the list.
    @State private var sliding = false

    /// How far it slides open: 6 past the buttons.
    private var reach: CGFloat {
        let buttons: CGFloat = layer.kind == .mask ? 2 : 3
        return swipeButton * buttons + swipeGap * (buttons - 1) + 6
    }

    private func button(_ label: String, fill: Color, ink: Color, action: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 15, weight: .medium))
            .foregroundStyle(ink)
            .lineLimit(1)
            .frame(width: swipeButton)
            .frame(maxHeight: .infinity)
            .background(RoundedRectangle(cornerRadius: 8, style: .circular).fill(fill))
            .contentShape(Rectangle())
            .onTapGesture {
                action()
                onOpen(false)
            }
    }

    var body: some View {
        let l = layer
        // the only layer can't go: it is cleared instead
        let alone = model.layers.filter { $0.kind != .mask }.count == 1
        ZStack(alignment: .trailing) {
            if offset < 0 {
                HStack(spacing: swipeGap) {
                    button(l.locked ? "Unlock" : "Lock", fill: swipeFill, ink: swipeInk) { model.toggleLocked(l) }
                    if l.kind != .mask {
                        button("Duplicate", fill: swipeFill, ink: swipeInk) { model.duplicate(l) }
                    }
                    button(alone ? "Clear" : "Delete", fill: swipeDelete, ink: .white) {
                        if alone {
                            model.clear(l)
                        } else {
                            model.delete(l)
                        }
                    }
                }
            }
            content()
                .offset(x: offset)
                .simultaneousGesture(
                    DragGesture(minimumDistance: 14)
                        .onChanged { drag in
                            guard enabled else { return }
                            if !sliding, abs(drag.translation.width) > abs(drag.translation.height) {
                                sliding = true
                            }
                            if sliding {
                                // closed, it goes right as far as picking takes
                                offset = min(max((open ? -reach : 0) + drag.translation.width, -reach), open ? 0 : pickReach)
                            }
                        }
                        .onEnded { _ in
                            guard sliding else { return }
                            sliding = false
                            if offset > pickReach / 2 {
                                onPick()
                                withAnimation(Motion.swipeSettle) { offset = 0 }
                                return
                            }
                            let opened = offset < -reach / 2
                            if opened == open {
                                withAnimation(Motion.swipeSettle) { offset = open ? -reach : 0 }
                            } else {
                                onOpen(opened)
                            }
                        }
                )
        }
        // the list only clips top and bottom; a slid row stops at its edge
        .clipped()
        .onChange(of: open, initial: true) { _, now in
            withAnimation(Motion.swipeSettle) { offset = now ? -reach : 0 }
        }
    }
}

/// Two fingers on the rows. These are UIKit's: a SwiftUI gesture over the rows would take their
/// one-finger touches, which these leave alone (they watch from the window, for as long as the
/// panel is up).
private struct RowFingers: UIViewRepresentable {
    let model: CanvasModel
    let ui: LayersUi

    final class Anchor: UIView, UIGestureRecognizerDelegate {
        var model: CanvasModel?
        fileprivate var ui: LayersUi?
        private var watching: [UIGestureRecognizer] = []
        /// The two rows pinched, where their fingers went down, and how far apart.
        private var ends: (upper: UInt64, lower: UInt64)?
        private var starts: (upper: CGFloat, lower: CGFloat) = (0, 0)
        private var apart: CGFloat = 0
        private var pinched = false

        override func willMove(toWindow window: UIWindow?) {
            super.willMove(toWindow: window)
            watching.forEach { $0.view?.removeGestureRecognizer($0) }
            watching = []
            guard let window else { return }
            let swipe = UISwipeGestureRecognizer(target: self, action: #selector(swiped(_:)))
            swipe.direction = .right
            swipe.numberOfTouchesRequired = 2
            let hold = UILongPressGestureRecognizer(target: self, action: #selector(held(_:)))
            hold.numberOfTouchesRequired = 2
            hold.minimumPressDuration = 0.5
            let pinch = UIPinchGestureRecognizer(target: self, action: #selector(pinching(_:)))
            for recognizer in [swipe, hold, pinch] {
                recognizer.cancelsTouchesInView = false
                recognizer.delaysTouchesBegan = false
                recognizer.delaysTouchesEnded = false
                recognizer.delegate = self
                window.addGestureRecognizer(recognizer)
                watching.append(recognizer)
            }
        }

        func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
            true
        }

        /// Only on the rows: elsewhere two fingers are the canvas's.
        override func gestureRecognizerShouldBegin(_ g: UIGestureRecognizer) -> Bool {
            ui?.row(at: g.location(in: nil).y) != nil && insidePanel(g.location(in: nil).x)
        }

        private func insidePanel(_ x: CGFloat) -> Bool {
            guard let rows = ui?.rows.values, let any = rows.first else { return false }
            return x >= any.minX && x <= any.maxX
        }

        /// The paint layer under a two-finger touch.
        private func paintLayer(_ g: UIGestureRecognizer) -> LayerInfo? {
            guard let id = ui?.row(at: g.location(in: nil).y) else { return nil }
            return model?.layers.first { $0.id == id && $0.kind == .paint }
        }

        @objc private func swiped(_ g: UISwipeGestureRecognizer) {
            if let l = paintLayer(g) {
                model?.toggleAlphaLock(l)
            }
        }

        @objc private func held(_ g: UILongPressGestureRecognizer) {
            if g.state == .began, let l = paintLayer(g) {
                model?.selectLayerContents(l)
            }
        }

        @objc private func pinching(_ g: UIPinchGestureRecognizer) {
            guard let ui else { return }
            switch g.state {
            case .began:
                guard g.numberOfTouches >= 2 else { return }
                let a = g.location(ofTouch: 0, in: nil).y
                let b = g.location(ofTouch: 1, in: nil).y
                let upper = min(a, b)
                let lower = max(a, b)
                pinched = false
                ends = nil
                if let top = ui.row(at: upper), let bottom = ui.row(at: lower), top != bottom {
                    ends = (top, bottom)
                    starts = (upper, lower)
                    apart = lower - upper
                }
            case .changed:
                guard let ends, g.numberOfTouches >= 2 else { return }
                let a = g.location(ofTouch: 0, in: nil).y
                let b = g.location(ofTouch: 1, in: nil).y
                let upper = min(a, b)
                let lower = max(a, b)
                if lower - upper <= apart * pinchMerge {
                    pinched = true
                }
                ui.squeeze = Squeeze(upper: ends.upper, lower: ends.lower, upperBy: upper - starts.upper, lowerBy: lower - starts.lower)
            default:
                ui.squeeze = nil
                // as a finger lets go
                if g.state == .ended, pinched, let ends {
                    model?.mergeBetween(ends.upper, ends.lower)
                }
                ends = nil
            }
        }
    }

    func makeUIView(context: Context) -> Anchor {
        let view = Anchor()
        view.isUserInteractionEnabled = false
        view.model = model
        view.ui = ui
        return view
    }

    func updateUIView(_ view: Anchor, context: Context) {}
}

/// The panel's menus, over it: the title's and the current layer's options.
private struct LayersMenus: View {
    let model: CanvasModel
    let ui: LayersUi
    let maxHeight: CGFloat

    private func closeAll() {
        withAnimation(Motion.menuClose) {
            ui.titleMenu = false
            ui.options = nil
        }
    }

    var body: some View {
        ZStack(alignment: .topLeading) {
            if ui.titleMenu {
                MenuDismissal { closeAll() }
                titleMenu
            }
            if let id = ui.options, let l = model.layers.first(where: { $0.id == id }), let row = ui.rows[id] {
                MenuDismissal { closeAll() }
                LayerOptionsMenu(model: model, ui: ui, layer: l, rowY: row.midY, maxHeight: maxHeight) { closeAll() }
            }
        }
    }

    /// Under the title: Select All; Group and Merge (with layers picked); Share and Duplicate;
    /// Delete. Mostly over the panel, whose rows the canvas's frosting doesn't blur: nearly solid.
    private var titleMenu: some View {
        GlassMenu(overPanel: true) {
            MenuRow(icon: PcIcons.menuSelectAll, label: "Select All") {
                closeAll()
                model.pickAllLayers()
            }
            MenuLine()
            if model.anyPicked {
                MenuRow(icon: PcIcons.menuGroup, label: "Group") {
                    closeAll()
                    model.groupPicked()
                }
                MenuRow(icon: PcIcons.menuMerge, label: "Merge") {
                    closeAll()
                    model.mergePicked()
                }
                MenuLine()
            }
            MenuRow(icon: PcIcons.menuShare, label: "Share") {
                closeAll()
                model.shareChosenLayers()
            }
            MenuRow(icon: PcIcons.menuDuplicate, label: "Duplicate") {
                closeAll()
                model.duplicatePicked()
            }
            MenuLine()
            MenuRow(icon: PcIcons.menuTrash, label: "Delete", ink: Pc.menuDanger) {
                closeAll()
                model.question = Question(title: "Delete these layers?", message: "Undo brings them back.") {
                    // as measured, a locked one among them stops it, said why
                    if model.chosenLocked() {
                        model.lockedDeleteTold = true
                    } else {
                        model.deletePicked()
                    }
                }
            }
        }
        .offset(x: 79 - glassMenuWidth / 2, y: 52)
        .menuTransition(.top)
    }
}

/// The current layer's options, left of the panel and centred on its row, kept below the top bar.
/// What doesn't apply to it is left out; lock, duplicate and delete are on the row's swipe.
private struct LayerOptionsMenu: View {
    let model: CanvasModel
    let ui: LayersUi
    let layer: LayerInfo
    /// The row's middle on the screen.
    let rowY: CGFloat
    let maxHeight: CGFloat
    let onDone: () -> Void

    private func act(_ option: LayerOption) {
        let l = layer
        switch option {
        case .rename:
            ui.renaming = l.id
        case .ungroup:
            model.ungroup(l)
        case .flatten:
            model.flattenGroup(l)
        case .select:
            model.selectLayerContents(l)
        case .copy:
            model.copyLayerContents(l)
        case .fill:
            model.fillLayer(l)
        case .clear:
            model.clear(l)
        case .alphaLock:
            model.toggleAlphaLock(l)
        case .mask:
            model.addMask(l)
        case .clippingMask:
            model.toggleClipped(l)
        case .drawingAssist:
            model.setGuideAssisted(!l.assisted)
        case .invert:
            model.invertLayer(l)
        case .reference:
            model.toggleReference(l)
        case .editText:
            model.showNotice("Text can't be edited on the iPad yet.", warning: true)
        case .rasterize:
            model.rasterize(l)
        case .mergeDown:
            model.mergeDown(l)
        case .combineDown:
            model.combineDown(l)
        case .mergeMask:
            model.mergeMask(l)
        }
    }

    var body: some View {
        let rows = model.layerOptions(layer)
        let shape = RoundedRectangle(cornerRadius: 24, style: .circular)
        let tall = max(CGFloat(rows.count) * 38 + 14, 64)
        // the panel hangs from the bar: its own top is the bar's height down the screen
        let top = optionsTop - Pc.barHeight
        let y = min(max(rowY - Pc.barHeight - tall / 2, top), max(maxHeight - tall, top))
        VStack(spacing: 0) {
            ForEach(rows, id: \.option) { r in
                let ink = r.enabled ? Pc.menuInk : Pc.menuDisabled
                HStack(spacing: 0) {
                    Text(verbatim: r.label)
                        .font(.system(size: 17))
                        .foregroundStyle(ink)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    if r.switchOn == true {
                        PcIconView(PcIcons.tick, size: 18, color: ink)
                    }
                }
                .padding(.leading, 19)
                .padding(.trailing, 16)
                .frame(height: 38)
                .contentShape(Rectangle())
                .onTapGesture {
                    if r.enabled {
                        act(r.option)
                        onDone()
                    }
                }
            }
        }
        .padding(.vertical, 7)
        .frame(width: 194)
        .frame(minHeight: 64)
        .background {
            // empty (a locked group's or mask's), as measured, the frost alone, untinted
            ZStack {
                shape.fill(.ultraThinMaterial)
                if !rows.isEmpty {
                    shape.fill(Pc.glassMenu)
                }
            }
        }
        .clipShape(shape)
        .shadow(color: .black.opacity(0.14), radius: 15, y: 6)
        .offset(x: -183, y: y)
        .menuTransition(.trailing)
    }
}

/// A layer's opacity and blend mode, under its row. The modes are a picker: the rows snap to the
/// list's middle and the row there is the layer's mode (set once a scroll settles), the ends padded
/// so every row can reach the middle, the layer's mode there when it opens.
private struct BlendEditor: View {
    let model: CanvasModel
    let layer: LayerInfo

    @State private var opacity: Float = 1
    /// The mode in the list's middle.
    @State private var centred: UInt32?
    @State private var settling: DispatchWorkItem?

    private func slider(_ width: CGFloat) -> some View {
        let part = CGFloat(min(max(opacity, 0), 1))
        let x = min(max(width * part, 13), max(width - 13, 13))
        return ZStack(alignment: .leading) {
            Capsule().fill(Pc.sliderRest).frame(height: 3)
            Capsule().fill(Pc.accent).frame(width: width * part, height: 3)
            Capsule().fill(Color.white)
                .frame(width: 26, height: 15)
                .offset(x: x - 13)
        }
        .frame(width: width, height: 16)
        .contentShape(Rectangle())
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { drag in
                    opacity = Float(min(max(drag.location.x / max(width, 1), 0), 1))
                    model.setLayerOpacity(layer, opacity, done: false)
                }
                .onEnded { _ in model.setLayerOpacity(layer, opacity, done: true) }
        )
    }

    var body: some View {
        let modes = model.blendModeList
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                Text(verbatim: "Opacity")
                Spacer(minLength: 0)
                Text(verbatim: opacity >= 0.995 ? "Max" : "\(wholeNumber(opacity * 100))%")
            }
            .font(.system(size: 17))
            .foregroundStyle(Pc.panelTitle)
            .padding(.horizontal, 13)
            GeometryReader { g in
                slider(g.size.width)
            }
            .frame(height: 16)
            .padding(.horizontal, 13)
            .padding(.vertical, 2)
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(modes, id: \.id) { m in
                        let chosen = m.id == layer.blend
                        HStack(spacing: 0) {
                            Text(verbatim: m.name)
                                .font(.system(size: 17))
                                .foregroundStyle(chosen ? Color.white : Pc.panelTitle)
                                .lineLimit(1)
                            Spacer(minLength: 0)
                            // its 26 frame centred 31.5 in from the right; grey unless chosen
                            if let glyph = PcIcons.blendGlyphs.first(where: { $0.0 == m.name })?.1 {
                                PcIconView(glyph, size: 26, color: chosen ? .white : blendGlyphInk)
                            }
                        }
                        .padding(.leading, 12)
                        .padding(.trailing, 18.5)
                        .frame(height: blendRowHeight)
                        .background(chosen ? Pc.rowSelected : Color.clear)
                        .contentShape(Rectangle())
                        .onTapGesture {
                            withAnimation(Motion.rowMove) { centred = m.id }
                        }
                    }
                }
                .scrollTargetLayout()
            }
            .contentMargins(.vertical, (blendListHeight - blendRowHeight) / 2, for: .scrollContent)
            .scrollTargetBehavior(.viewAligned)
            .scrollPosition(id: $centred, anchor: .center)
            .scrollIndicators(.hidden)
            .frame(height: blendListHeight)
            .background(RoundedRectangle(cornerRadius: 12, style: .circular).fill(Pc.layerRow))
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .circular))
            .padding(.top, 11)
        }
        .padding(.top, 22)
        .padding(.bottom, 15)
        .onAppear {
            opacity = layer.opacity
            centred = layer.blend
        }
        .onChange(of: centred) { _, id in
            // once the scroll settles
            settling?.cancel()
            guard let id, id != layer.blend else { return }
            let l = layer
            let job = DispatchWorkItem { model.setBlend(l, id) }
            settling = job
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.15, execute: job)
        }
    }
}

/// A layer's row.
private struct LayerRow: View {
    let model: CanvasModel
    let layer: LayerInfo
    let shape: UnevenRoundedRectangle
    /// The layer whose mask is the current one: a darker blue.
    let owner: Bool
    let renaming: Bool
    let onRenamed: (String) -> Void
    let onTap: () -> Void
    let onBlend: () -> Void

    var body: some View {
        let l = layer
        let mask = l.kind == .mask
        let masked = model.hasMask(l)
        let selected = l.current
        let ink = selected || l.picked ? Color.white : Pc.panelTitle
        let fill = selected ? Pc.rowSelected : l.picked ? pickedRow : owner ? maskOwnerRow : Pc.layerRow
        HStack(spacing: 0) {
            if l.clipped {
                PcIconView(PcIcons.clip, size: 16, color: ink)
            }
            // a mask's thumbnail and its layer's join into one, rounded only at the outer corners
            Thumb(model: model, layer: l, top: !masked, bottom: !mask)
                .padding(.leading, 4)
                .offset(y: mask ? 2 : masked ? -2 : 0)
            ZStack(alignment: .leading) {
                LockedName(layer: l, ink: ink, renaming: renaming, onRenamed: onRenamed)
                if let tag = tag(l) {
                    Text(verbatim: tag)
                        .font(.system(size: 13))
                        .foregroundStyle(ink)
                        .lineLimit(1)
                        .padding(.leading, 29)
                        .offset(y: 17.5)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            // the blend and visibility buttons overlap by 8, as measured; a mask has no blend
            if mask {
                Spacer().frame(width: 36)
            } else {
                Text(verbatim: blendCode(l.blend))
                    .font(.system(size: 14, weight: .semibold))
                    // a locked layer's mode is greyed
                    .foregroundStyle(l.locked && !l.current && !l.picked ? lockedLetter : ink)
                    .frame(width: 44, height: 47)
                    .contentShape(Rectangle())
                    .onTapGesture(perform: onBlend)
                    .offset(x: 8)
            }
            VisibilityBox(model: model, layer: l, fill: selected || l.picked ? Color.white : Pc.checkFill, height: 47)
            Spacer().frame(width: 8)
        }
        .padding(.leading, (l.clipped ? 18 : 0) + groupIndent * CGFloat(l.depth))
        .frame(maxWidth: .infinity)
        .frame(height: mask || masked ? maskedRowHeight : rowHeight)
        .background(shape.fill(fill))
        .contentShape(Rectangle())
        .onTapGesture(perform: onTap)
    }

    /// What the layer is, under its name: the reference layer, one drawing assist is on for, a
    /// private one.
    private func tag(_ l: LayerInfo) -> String? {
        if l.reference && l.assisted {
            return "Assisted Reference"
        }
        if l.reference {
            return "Reference"
        }
        if l.assisted {
            return "Assisted"
        }
        if l.private {
            return "Private"
        }
        return nil
    }

    /// The letters on a layer's row for its mode: a two-word name's initials (the second small), a
    /// one-word name's first letter, or its first two where one letter is already taken.
    private func blendCode(_ blend: UInt32) -> String {
        guard let name = model.blendModeList.first(where: { $0.id == blend })?.name else { return "N" }
        if twoLetterBlends.contains(name) {
            return String(name.prefix(2))
        }
        let words = name.split(separator: " ")
        if words.count == 1 {
            return String(name.prefix(1))
        }
        return String(words[0].prefix(1)) + words[1].prefix(1).lowercased()
    }
}

/// A layer's visibility box: tapped, shown or hidden; held, shown alone (or every layer again).
private struct VisibilityBox: View {
    let model: CanvasModel
    let layer: LayerInfo
    let fill: Color
    let height: CGFloat

    var body: some View {
        CheckBox(checked: layer.visible, fill: fill)
            .frame(width: 44, height: height)
            .contentShape(Rectangle())
            .onTapGesture { model.toggleVisible(layer) }
            .onLongPressGesture { model.toggleSolo(layer) }
    }
}

/// A group's row: a line where a thumbnail would be, its name, its open or closed arrow and its
/// box.
private struct GroupRow: View {
    let model: CanvasModel
    let layer: LayerInfo
    let shape: UnevenRoundedRectangle
    let renaming: Bool
    let onRenamed: (String) -> Void
    let onTap: () -> Void

    var body: some View {
        let l = layer
        let ink = l.current || l.picked ? Color.white : Pc.panelTitle
        HStack(spacing: 0) {
            RoundedRectangle(cornerRadius: 2, style: .circular)
                .fill(groupLine)
                .frame(width: 93, height: 4)
                .padding(.leading, 4)
            LockedName(layer: l, ink: ink, renaming: renaming, onRenamed: onRenamed)
                .frame(maxWidth: .infinity, alignment: .leading)
            PcIconView(l.collapsed ? PcIcons.chevronRight : PcIcons.chevronDown, size: 16, color: ink)
                .frame(width: 44, height: groupHeight)
                .contentShape(Rectangle())
                .onTapGesture { model.toggleCollapsed(l) }
                .offset(x: 8)
            VisibilityBox(model: model, layer: l, fill: l.current || l.picked ? Color.white : Pc.checkFill, height: groupHeight)
            Spacer().frame(width: 8)
        }
        .padding(.leading, groupIndent * CGFloat(l.depth))
        .frame(maxWidth: .infinity)
        .frame(height: groupHeight)
        .background(shape.fill(l.current ? Pc.rowSelected : l.picked ? pickedRow : Pc.layerRow))
        .contentShape(Rectangle())
        .onTapGesture(perform: onTap)
    }
}

/// A layer's name, renamed in place, with a padlock before it when it is locked (the glyph ends 12
/// short of the name).
private struct LockedName: View {
    let layer: LayerInfo
    let ink: Color
    let renaming: Bool
    let onRenamed: (String) -> Void

    var body: some View {
        ZStack(alignment: .leading) {
            if layer.locked {
                PcIconView(PcIcons.lockRow, size: 17, color: ink)
                    .offset(x: 3)
            }
            Group {
                if renaming {
                    InlineName(name: layer.name, color: ink, fontSize: 17, onDone: onRenamed)
                } else {
                    Text(verbatim: layer.name)
                        .font(.system(size: 17))
                        .foregroundStyle(ink)
                        .lineLimit(1)
                }
            }
            .padding(.leading, 29)
            .padding(.trailing, 6)
        }
    }
}

/// A layer's thumbnail, 93 x 61: what is drawn, fitted (small drawings show their pixels), on
/// grey, white for a mask, a checkerboard with alpha lock; a text layer's is its glyph. `top` and
/// `bottom` say which corners are rounded.
private struct Thumb: View {
    let model: CanvasModel
    let layer: LayerInfo
    let top: Bool
    let bottom: Bool

    var body: some View {
        let corner: CGFloat = 4
        let shape = UnevenRoundedRectangle(
            topLeadingRadius: top ? corner : 0, bottomLeadingRadius: bottom ? corner : 0,
            bottomTrailingRadius: bottom ? corner : 0, topTrailingRadius: top ? corner : 0,
            style: .circular
        )
        ZStack {
            if layer.alphaLock {
                Checker()
            } else {
                Rectangle().fill(layer.kind == .mask ? Color.white : Pc.thumbBackground)
            }
            if layer.text {
                PcIconView(PcIcons.textLayer, size: 61, color: Pc.textLayerGlyph)
            } else if let picture = model.thumbnails[layer.id] {
                Image(uiImage: picture)
                    .resizable()
                    .interpolation(.none)
                    .scaledToFit()
            }
        }
        .frame(width: 93, height: 61)
        .clipShape(shape)
    }
}

/// The transparency checkerboard behind thumbnails.
private struct Checker: View {
    var body: some View {
        Canvas { context, size in
            let cell: CGFloat = 6
            context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(.white))
            var y = 0
            while CGFloat(y) * cell < size.height {
                var x = 0
                while CGFloat(x) * cell < size.width {
                    if (x + y) % 2 == 1 {
                        context.fill(Path(CGRect(x: CGFloat(x) * cell, y: CGFloat(y) * cell, width: cell, height: cell)), with: .color(Color(argb: 0xFFD6_D6D6)))
                    }
                    x += 1
                }
                y += 1
            }
        }
    }
}

/// A visibility box: filled with the tick cut out of it (the row showing through), or an outline.
struct CheckBox: View {
    let checked: Bool
    let fill: Color

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 5, style: .circular)
        ZStack {
            if checked {
                shape.fill(fill)
                // the tick is cut out of the box
                PcIconView(PcIcons.tick, size: 14, color: .black)
                    .blendMode(.destinationOut)
            } else {
                shape.strokeBorder(fill, lineWidth: 1.5)
            }
        }
        .frame(width: 18, height: 18)
        .compositingGroup()
    }
}

/// The background colour's row: its swatch (none shown while hidden) and its visibility box.
private struct BackgroundRow: View {
    let model: CanvasModel

    var body: some View {
        let bg = model.background
        // hidden, the swatch shows no colour: a faint white over the row
        let swatch = bg.visible ? Color(.sRGB, red: Double(bg.r), green: Double(bg.g), blue: Double(bg.b)) : Color.white.opacity(0.06)
        HStack(spacing: 0) {
            HStack(spacing: 0) {
                RoundedRectangle(cornerRadius: 14, style: .circular)
                    .fill(swatch)
                    .frame(width: 92, height: 42)
                    .padding(.leading, 4)
                Text(verbatim: "Background color")
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.panelTitle)
                    .lineLimit(1)
                    .padding(.leading, 14)
                Spacer(minLength: 0)
            }
            CheckBox(checked: bg.visible, fill: Pc.checkFill)
                .frame(width: 44, height: 41)
                .contentShape(Rectangle())
                .onTapGesture { model.toggleBackground() }
            Spacer().frame(width: 8)
        }
        .frame(height: 50)
        .background(RoundedRectangle(cornerRadius: 17, style: .circular).fill(Pc.backgroundRow))
    }
}

/// The layers panel's width.
let layersPanelWidth = layersWidth
