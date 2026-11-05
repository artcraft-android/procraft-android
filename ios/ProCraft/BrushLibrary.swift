import SwiftUI
import UniformTypeIdentifiers

private let libraryWidth: CGFloat = 440
private let setsWidth: CGFloat = 168
private let brushHeight: CGFloat = 80
private let brushGap: CGFloat = 8
private let namePill = Color(argb: 0x8000_0000)
/// The space the library's rows report their places in, and the drags follow.
private let librarySpace = "library"
/// A brush swiped left shows three buttons filling its place, this far apart.
private let swipeGap: CGFloat = 5.5
private let swipeFill = Color(argb: 0xFF3F_3F3F)
private let swipeInk = Color(argb: 0xFFC5_C5C5)
private let swipeDelete = Color(argb: 0xFFFF_3A46)
private let searchFill = Color(argb: 0x1F76_7680)
private let searchHint = Color(argb: 0x4DEB_EBF5)
private let resultFill = Color(argb: 0x14FF_FFFF)
/// Menus stay below the top bar.
private let menuTop: CGFloat = 57
/// The set icon picker: how many of the icons it offers, its columns, how far left of the set it
/// starts, and the room it keeps below.
private let pickedSetIcons = 42
private let pickerColumns = 5
private let pickerInset: CGFloat = 11
private let pickerMargin: CGFloat = 25

/// A panel's title row: its title (with a chevron when it has a menu) at its left, `action` at its
/// right.
struct PanelHeader<Action: View>: View {
    let title: String
    var start: CGFloat = 24
    var end: CGFloat = 18
    var height: CGFloat = 64
    var chevron = true
    var chevronOn = false
    var onTitle: (() -> Void)?
    @ViewBuilder let action: () -> Action

    var body: some View {
        ZStack(alignment: .topLeading) {
            HStack(spacing: 0) {
                Text(verbatim: title)
                    .font(.system(size: 24, weight: .bold))
                    .foregroundStyle(Pc.panelTitle)
                    .lineLimit(1)
                if chevron {
                    ZStack {
                        Circle().fill(chevronOn ? Pc.rowSelected : Pc.chevronDisc)
                        PcIconView(PcIcons.chevronDown, size: 13, color: Pc.chevronGlyph)
                    }
                    .frame(width: 17, height: 17)
                    .padding(.leading, 10)
                    .offset(y: -1.5)
                }
            }
            .padding(.leading, start)
            .padding(.top, 19)
            .contentShape(Rectangle())
            .onTapGesture { onTitle?() }
            HStack {
                Spacer()
                action()
            }
            .padding(.trailing, end)
        }
        .frame(maxWidth: .infinity, minHeight: height, maxHeight: height, alignment: .topLeading)
    }
}

/// What the library's rows share while it is open: its menus, the search, and a brush or set held
/// and dragged, with where the rows are.
@Observable
private final class LibraryUi {
    var adding = false
    var setMenu: Int?
    var recentMenu = false
    var iconPicker = false
    var importing = false
    var query = ""
    /// The search field has come down (pulled past a list's top).
    var searchShown = false
    var dragged: BrushInfo?
    var draggedSet: Int?
    /// Where the finger is, and where on the row it took hold.
    var dragAt = CGPoint.zero
    var grab = CGPoint.zero
    var setFrames: [Int: CGRect] = [:]
    var brushFrames: [String: CGRect] = [:]
    /// How tall the panel is.
    var height: CGFloat = 0

    /// The set the dragged brush or set is over, if any.
    var overSet: Int? {
        guard dragged != nil || draggedSet != nil else { return nil }
        return setFrames.first { $0.value.contains(dragAt) }?.key
    }

    var dragging: Bool {
        dragged != nil || draggedSet != nil
    }

    /// A brush let go of: onto a set, to its top; onto a brush of the shown set, to its place.
    func dropBrush(_ model: CanvasModel, shown: [BrushInfo]) {
        guard let b = dragged else { return }
        dragged = nil
        if let set = setFrames.first(where: { $0.value.contains(dragAt) })?.key {
            if set != Int(b.set) {
                model.moveBrush(b, set: set, index: 0)
            }
            return
        }
        if let target = shown.first(where: { brushFrames[CanvasModel.brushKey($0)]?.contains(dragAt) == true }) {
            model.moveBrush(b, set: Int(target.set), index: Int(target.index))
        }
    }

    /// A set dropped onto another takes its place.
    func dropSet(_ model: CanvasModel) {
        guard let from = draggedSet else { return }
        draggedSet = nil
        if let to = setFrames.first(where: { $0.value.contains(dragAt) })?.key {
            model.moveBrushSet(from: from, to: to)
        }
    }
}

private struct SetFrames: PreferenceKey {
    static let defaultValue: [Int: CGRect] = [:]
    static func reduce(value: inout [Int: CGRect], nextValue: () -> [Int: CGRect]) {
        value.merge(nextValue()) { $1 }
    }
}

private struct BrushFrames: PreferenceKey {
    static let defaultValue: [String: CGRect] = [:]
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue()) { $1 }
    }
}

/// How far a list's top has been pulled down past where it rests.
private struct ListPull: PreferenceKey {
    static let defaultValue: CGFloat = 0
    /// Of the two lists, the one being moved: the other rests at its top.
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        let next = nextValue()
        if abs(next) > abs(value) {
            value = next
        }
    }
}

/// The brush library hanging from the tool that opened it: the sets down its left, the shown
/// set's brushes (each its name over its sample stroke) on its right. Tap the chosen set for its
/// menu, swipe a brush left to share, duplicate or delete it, hold and drag it onto a set or to
/// another place in its own, and add brushes and sets with the plus.
struct BrushLibrary: View {
    let model: CanvasModel
    let arrowX: CGFloat
    @State private var ui = LibraryUi()
    @FocusState private var searching: Bool

    var body: some View {
        let title: String = switch model.tool {
        case .erase: "Erase library"
        case .smudge: "Smudge library"
        case .paint: "Brush library"
        }
        Popover(width: libraryWidth, arrowX: arrowX) {
            PanelHeader(title: title, start: 20, end: 12, height: 52) {
                PcIconView(PcIcons.plusMedium, size: 41, color: Pc.panelGlyph)
                    .frame(width: 41, height: 40)
                    .contentShape(Rectangle())
                    .onTapGesture { withAnimation(Motion.menuOpen) { ui.adding = true } }
                    .padding(.top, 12)
            }
            VStack(spacing: 0) {
                // the search field comes down as a list is pulled past its top, and goes again as
                // the lists scroll up
                if ui.searchShown || searching || !ui.query.isEmpty {
                    LibrarySearch(ui: ui, focus: $searching)
                }
                if searching || !ui.query.isEmpty {
                    SearchResults(model: model, query: ui.query)
                } else {
                    HStack(spacing: 0) {
                        SetColumn(model: model, ui: ui)
                        BrushColumn(model: model, ui: ui)
                    }
                    .frame(maxHeight: .infinity)
                }
            }
            .frame(maxHeight: .infinity)
            .onPreferenceChange(ListPull.self) { pull in
                if pull > 24, !ui.searchShown {
                    withAnimation(Motion.settle(0.25)) { ui.searchShown = true }
                } else if pull < -6, ui.searchShown, ui.query.isEmpty, !searching {
                    withAnimation(Motion.settle(0.25)) { ui.searchShown = false }
                }
            }
        }
        .coordinateSpace(.named(librarySpace))
        .background(GeometryReader { g in
            Color.clear
                .onAppear { ui.height = g.size.height }
                .onChange(of: g.size.height) { _, height in ui.height = height }
        })
        .onPreferenceChange(SetFrames.self) { ui.setFrames = $0 }
        .onPreferenceChange(BrushFrames.self) { ui.brushFrames = $0 }
        .overlay(alignment: .topLeading) { LibraryMenus(model: model, ui: ui) }
        .overlay(alignment: .topLeading) { DragGhost(model: model, ui: ui) }
        .fileImporter(isPresented: $ui.importing, allowedContentTypes: [.item]) { result in
            if case let .success(url) = result {
                model.importBrushes(from: url)
            }
        }
    }
}

/// The library's search field: a magnifier, what is typed, and a button clearing it.
private struct LibrarySearch: View {
    let ui: LibraryUi
    let focus: FocusState<Bool>.Binding

    var body: some View {
        @Bindable var ui = ui
        HStack(spacing: 0) {
            Canvas { context, size in
                // a magnifier: a ring and its handle
                let c = CGPoint(x: size.width * 0.47, y: size.height * 0.46)
                let r: CGFloat = 7
                context.stroke(Path(ellipseIn: CGRect(x: c.x - r, y: c.y - r, width: r * 2, height: r * 2)), with: .color(searchHint), lineWidth: 2)
                let d = r * 0.7071
                var handle = Path()
                handle.move(to: CGPoint(x: c.x + d, y: c.y + d))
                handle.addLine(to: CGPoint(x: c.x + d + 5, y: c.y + d + 5))
                context.stroke(handle, with: .color(searchHint), style: StrokeStyle(lineWidth: 2.4, lineCap: .round))
            }
            .frame(width: 41, height: 40)
            ZStack(alignment: .leading) {
                if ui.query.isEmpty {
                    Text(verbatim: "Search")
                        .font(.system(size: 17))
                        .foregroundStyle(searchHint)
                        .lineLimit(1)
                }
                TextField(text: $ui.query) { EmptyView() }
                    .font(.system(size: 17))
                    .foregroundStyle(Color.white)
                    .tint(Pc.accent)
                    .autocorrectionDisabled()
                    .submitLabel(.search)
                    .focused(focus)
            }
            if !ui.query.isEmpty {
                // a filled circle with a cross cut out of it
                Canvas { context, size in
                    let c = CGPoint(x: size.width / 2, y: size.height / 2)
                    let r = min(size.width, size.height) / 2
                    context.fill(Path(ellipseIn: CGRect(x: c.x - r, y: c.y - r, width: r * 2, height: r * 2)), with: .color(searchHint))
                    let k: CGFloat = 3.5
                    for s: CGFloat in [1, -1] {
                        var line = Path()
                        line.move(to: CGPoint(x: c.x - k, y: c.y - k * s))
                        line.addLine(to: CGPoint(x: c.x + k, y: c.y + k * s))
                        context.stroke(line, with: .color(.black), style: StrokeStyle(lineWidth: 1.8, lineCap: .round))
                    }
                }
                .frame(width: 19, height: 18)
                .contentShape(Rectangle())
                .onTapGesture { ui.query = "" }
                .padding(.trailing, 5)
            }
        }
        .frame(height: 40)
        .background(RoundedRectangle(cornerRadius: 10, style: .circular).fill(searchFill))
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
    }
}

/// The brushes of every set whose name holds the search, two to a row; tapping one chooses it.
private struct SearchResults: View {
    let model: CanvasModel
    let query: String

    var body: some View {
        let wanted = query.trimmingCharacters(in: .whitespaces)
        let found = wanted.isEmpty ? [] : model.brushSets.flatMap(\.brushes).filter { $0.name.localizedCaseInsensitiveContains(wanted) }
        if found.isEmpty {
            Text(verbatim: "No results")
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(Pc.panelTitle)
                .padding(.top, 24)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        } else {
            ScrollView {
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 6), count: 2), spacing: 6) {
                    ForEach(found, id: \.self) { b in
                        ResultCell(model: model, brush: b)
                    }
                }
                .padding(.horizontal, 12)
                .padding(.top, 11)
                .padding(.bottom, 12)
            }
            .scrollIndicators(.hidden)
            .scrollDismissesKeyboard(.immediately)
        }
    }
}

private struct ResultCell: View {
    let model: CanvasModel
    let brush: BrushInfo

    var body: some View {
        let key = CanvasModel.brushKey(brush)
        let preview = model.previews[key]
        let selected = model.highlighted(brush)
        let shape = RoundedRectangle(cornerRadius: 12, style: .circular)
        ZStack(alignment: .topLeading) {
            shape.fill(selected ? Pc.rowSelected : resultFill)
            if let preview {
                SampleStroke(image: preview)
            }
            BrushName(name: brush.name, selected: selected, shaded: model.shadedNames[key] == true)
                .padding(.leading, 14)
                .padding(.top, 11)
                .padding(.trailing, 10)
        }
        .frame(height: 81)
        .clipShape(shape)
        .contentShape(Rectangle())
        .onTapGesture { model.chooseBrush(brush) }
        .task(id: preview == nil) {
            if preview == nil {
                model.loadPreview(brush)
            }
        }
    }
}

/// A brush's sample stroke filling its cell's height, centred, its ends past the cell's sides. It
/// lies over a clear view: wider than the cell, it would otherwise widen everything sized with it.
struct SampleStroke: View {
    let image: UIImage
    var tint: Color?

    var body: some View {
        Color.clear
            .overlay {
                if let tint {
                    Image(uiImage: image)
                        .renderingMode(.template)
                        .resizable()
                        .scaledToFill()
                        .foregroundStyle(tint)
                } else {
                    Image(uiImage: image)
                        .resizable()
                        .scaledToFill()
                }
            }
            .clipped()
    }
}

/// A brush's name on its cell: on a dark pill where its sample stroke runs under it.
private struct BrushName: View {
    let name: String
    let selected: Bool
    let shaded: Bool

    var body: some View {
        Text(verbatim: name)
            .font(.system(size: 14, weight: .semibold))
            .foregroundStyle(selected ? Color.white : Pc.panelTitle)
            .lineLimit(1)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background {
                if shaded {
                    Capsule().fill(namePill)
                }
            }
            .offset(x: -6, y: -2)
    }
}

/// A row's touch held still, then dragged, in the library's own space: `began` hears where it took
/// hold, and `ended` that it is over, let go or taken away by the system alike (a gesture's state
/// goes back either way, where its `onEnded` is only told of the first).
private struct HeldDrag: ViewModifier {
    let ui: LibraryUi
    let began: (CGPoint) -> Void
    let ended: () -> Void
    @GestureState private var held = false

    func body(content: Content) -> some View {
        content
            .simultaneousGesture(
                LongPressGesture(minimumDuration: 0.5)
                    .sequenced(before: DragGesture(minimumDistance: 0, coordinateSpace: .named(librarySpace)))
                    .updating($held) { value, state, _ in
                        if case .second(true, _) = value {
                            state = true
                        }
                    }
                    .onChanged { value in
                        if case let .second(true, drag?) = value {
                            if !ui.dragging {
                                began(drag.startLocation)
                            }
                            ui.dragAt = drag.location
                        }
                    }
            )
            .onChange(of: held) { _, now in
                if !now {
                    ended()
                }
            }
    }
}

/// The library's sets, the shown one lit, the Recent set above them.
private struct SetColumn: View {
    let model: CanvasModel
    let ui: LibraryUi

    var body: some View {
        ScrollView {
            VStack(spacing: 0) {
                PullReader()
                LazyVStack(spacing: 12) {
                    RecentRow(model: model, ui: ui)
                    ForEach(Array(model.brushSets.enumerated()), id: \.offset) { i, set in
                        SetRow(model: model, ui: ui, index: i, set: set)
                    }
                }
                .padding(12)
            }
        }
        .coordinateSpace(.named(listSpace))
        .scrollIndicators(.hidden)
        .scrollDisabled(ui.dragging)
        .frame(width: setsWidth)
        .background(Pc.setColumn)
    }
}

/// The name of the space a list's top is read in.
private let listSpace = "list"

/// Says how far the list it heads has been pulled down.
private struct PullReader: View {
    var body: some View {
        GeometryReader { g in
            Color.clear.preference(key: ListPull.self, value: g.frame(in: .named(listSpace)).minY)
        }
        .frame(height: 0)
    }
}

/// The brushes last painted with, and those pinned there, above the sets.
private struct RecentRow: View {
    let model: CanvasModel
    let ui: LibraryUi

    var body: some View {
        let recent = model.shownSet == CanvasModel.recentSet
        let ink = recent ? Color.white : Pc.panelTitle
        HStack(spacing: 0) {
            PcIconView(PcIcons.recentSet, size: 40, color: ink)
                .padding(.leading, 4)
            Text(verbatim: "Recent")
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(ink)
                .lineLimit(1)
                .padding(.leading, 1)
                .padding(.trailing, 8)
            Spacer(minLength: 0)
        }
        .frame(height: 50)
        .background(RoundedRectangle(cornerRadius: 12, style: .circular).fill(recent ? Pc.rowSelected : Pc.setCell))
        .background(GeometryReader { g in
            Color.clear.preference(key: SetFrames.self, value: [CanvasModel.recentSet: g.frame(in: .named(librarySpace))])
        })
        .contentShape(Rectangle())
        .onTapGesture {
            if recent {
                withAnimation(Motion.menuOpen) { ui.recentMenu = true }
            } else {
                model.showSet(CanvasModel.recentSet)
            }
        }
    }
}

/// A set: tapped, it shows (tapped again, its menu); held, it lifts and goes where it is dragged,
/// faint where it was.
private struct SetRow: View {
    let model: CanvasModel
    let ui: LibraryUi
    let index: Int
    let set: BrushSetInfo

    var body: some View {
        let selected = index == model.shownSet
        let ink = selected ? Color.white : Pc.panelTitle
        let fill = ui.overSet == index ? Pc.accent : selected ? Pc.rowSelected : Pc.setCell
        HStack(spacing: 0) {
            // a 40 frame centred 24 in
            PcIconView(model.setIcon(set), size: 40, color: ink)
                .padding(.leading, 4)
            if model.renamingSet == index {
                InlineName(name: set.name, color: ink, fontSize: 14) { model.renameBrushSet(index, name: $0) }
                    .padding(.leading, 1)
                    .padding(.trailing, 8)
            } else {
                Text(verbatim: set.name)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(ink)
                    .lineLimit(2)
                    .padding(.leading, 1)
                    .padding(.trailing, 8)
            }
            Spacer(minLength: 0)
        }
        .frame(height: 50)
        .background(RoundedRectangle(cornerRadius: 12, style: .circular).fill(fill))
        .opacity(ui.draggedSet == index ? 0.3 : 1)
        .background(GeometryReader { g in
            Color.clear.preference(key: SetFrames.self, value: [index: g.frame(in: .named(librarySpace))])
        })
        .contentShape(Rectangle())
        .onTapGesture {
            if selected {
                withAnimation(Motion.menuOpen) { ui.setMenu = index }
            } else {
                model.showSet(index)
            }
        }
        .modifier(HeldDrag(
            ui: ui,
            began: { at in
                ui.setMenu = nil
                let frame = ui.setFrames[index] ?? .zero
                ui.grab = CGPoint(x: at.x - frame.minX, y: at.y - frame.minY)
                ui.draggedSet = index
            },
            ended: { ui.dropSet(model) }
        ))
    }
}

/// A brush the list shows, and whether it is pinned in the Recent set.
private struct ListedBrush: Identifiable {
    var id: String { CanvasModel.brushKey(brush) }
    let brush: BrushInfo
    let pinned: Bool
}

/// The shown set's brushes (or the Recent set's, each marked if pinned).
private struct BrushColumn: View {
    let model: CanvasModel
    let ui: LibraryUi

    private var listed: [ListedBrush] {
        if model.shownSet == CanvasModel.recentSet {
            return model.recentBrushes.compactMap { r -> ListedBrush? in
                guard model.brushSets.indices.contains(Int(r.set)) else { return nil }
                let brushes = model.brushSets[Int(r.set)].brushes
                guard brushes.indices.contains(Int(r.index)) else { return nil }
                return ListedBrush(brush: brushes[Int(r.index)], pinned: r.pinned)
            }
        }
        guard model.brushSets.indices.contains(model.shownSet) else { return [] }
        return model.brushSets[model.shownSet].brushes.map { ListedBrush(brush: $0, pinned: false) }
    }

    var body: some View {
        let listed = listed
        ScrollViewReader { reader in
            ScrollView {
                VStack(spacing: 0) {
                    PullReader()
                    LazyVStack(spacing: brushGap) {
                        ForEach(listed) { item in
                            BrushRow(model: model, ui: ui, brush: item.brush, pinned: item.pinned, listed: listed.map { $0.brush })
                                .id(item.id)
                        }
                    }
                    .padding(.leading, 12)
                    .padding(.trailing, 12)
                    .padding(.top, 9.75)
                    .padding(.bottom, 12)
                }
            }
            .coordinateSpace(.named(listSpace))
            // a brush found from the Recent set is scrolled to in its own
            .onChange(of: model.foundBrush, initial: true) { _, found in
                if let found {
                    reader.scrollTo(found, anchor: .center)
                    model.foundBrush = nil
                }
            }
        }
        // another set's brushes replace these at once
        .id(model.shownSet)
        .scrollIndicators(.hidden)
        .scrollDisabled(ui.dragging)
        .frame(maxWidth: .infinity)
        .background(Pc.brushColumn)
    }
}

/// A button a swiped brush shows.
private struct SwipeAction: Identifiable {
    var id: String { label }
    let icon: PcIcon
    let label: String
    var fill = swipeFill
    var ink = swipeInk
    let perform: () -> Void
}

/// A brush that slides away to the left, showing its `actions` in its place: Share, Duplicate and
/// Delete; in the Recent set Find, Pin (or Unpin) and Clear.
private struct SwipeRow<Content: View>: View {
    let open: Bool
    let onOpen: (Bool) -> Void
    let actions: [SwipeAction]
    @ViewBuilder let content: () -> Content

    @State private var width: CGFloat = 0
    @State private var offset: CGFloat = 0
    /// The touch is going along the row, not down the list.
    @State private var sliding = false

    private var rest: CGFloat {
        open ? -width : 0
    }

    var body: some View {
        ZStack {
            if offset < 0 {
                HStack(spacing: swipeGap) {
                    ForEach(actions) { a in
                        VStack(spacing: 2) {
                            // 19 across, as measured
                            PcIconView(a.icon, size: 19, color: a.ink)
                            Text(verbatim: a.label)
                                .font(.system(size: 14, weight: .medium))
                                .foregroundStyle(a.ink)
                                .lineLimit(1)
                        }
                        .padding(.top, 5)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .background(RoundedRectangle(cornerRadius: 16, style: .circular).fill(a.fill))
                        .contentShape(Rectangle())
                        .onTapGesture {
                            onOpen(false)
                            a.perform()
                        }
                    }
                }
            }
            content()
                .offset(x: offset)
                .simultaneousGesture(
                    DragGesture(minimumDistance: 14)
                        .onChanged { drag in
                            if !sliding, abs(drag.translation.width) > abs(drag.translation.height) {
                                sliding = true
                            }
                            if sliding {
                                offset = min(max(rest + drag.translation.width, -width), 0)
                            }
                        }
                        .onEnded { _ in
                            guard sliding else { return }
                            sliding = false
                            let opened = offset < -width / 3
                            if opened == open {
                                withAnimation(Motion.swipeSettle) { offset = rest }
                            } else {
                                onOpen(opened)
                            }
                        }
                )
        }
        .background(GeometryReader { g in
            Color.clear
                .onAppear { width = g.size.width }
                .onChange(of: g.size.width) { _, w in width = w }
        })
        .clipped()
        .onChange(of: open) { _, _ in
            withAnimation(Motion.swipeSettle) { offset = rest }
        }
    }
}

/// A brush in the library: its sample stroke filling the cell's height, its name over it (on a
/// dark pill where the stroke runs under it), lit when it is the current brush or its set's last.
private struct BrushRow: View {
    let model: CanvasModel
    let ui: LibraryUi
    let brush: BrushInfo
    let pinned: Bool
    let listed: [BrushInfo]

    private var actions: [SwipeAction] {
        let b = brush
        let model = model
        if model.shownSet == CanvasModel.recentSet {
            return [
                SwipeAction(icon: PcIcons.swipeFind, label: "Find") { model.findBrush(b) },
                pinned
                    ? SwipeAction(icon: PcIcons.swipeUnpin, label: "Unpin") { model.pinRecentBrush(b, pinned: false) }
                    : SwipeAction(icon: PcIcons.swipePin, label: "Pin") { model.pinRecentBrush(b, pinned: true) },
                SwipeAction(icon: PcIcons.swipeClear, label: "Clear") { model.clearRecentBrush(b) },
            ]
        }
        return [
            SwipeAction(icon: PcIcons.swipeShare, label: "Share") { model.shareBrush(b) },
            SwipeAction(icon: PcIcons.swipeDuplicate, label: "Duplicate") { model.duplicateBrush(b) },
            SwipeAction(icon: PcIcons.swipeTrash, label: "Delete", fill: swipeDelete, ink: .white) {
                model.question = Question(title: "Delete this brush?") { model.deleteBrush(b) }
            },
        ]
    }

    var body: some View {
        let key = CanvasModel.brushKey(brush)
        let recent = model.shownSet == CanvasModel.recentSet
        // the current brush, or the brush last chosen in this set (in the Recent set the current
        // brush alone)
        let selected = recent
            ? model.currentBrush.map { $0.set == brush.set && $0.index == brush.index } == true
            : model.highlighted(brush)
        let preview = model.previews[key]
        let shape = RoundedRectangle(cornerRadius: 12, style: .circular)
        SwipeRow(open: model.swipedBrush == key, onOpen: { model.swipedBrush = $0 ? key : nil }, actions: actions) {
            ZStack(alignment: .topLeading) {
                shape.fill(selected ? Pc.rowSelected : Color.clear)
                if let preview {
                    // the samples are drawn white: dark on the light interface's cells
                    SampleStroke(image: preview, tint: Look.shared.light && !selected ? .black : nil)
                }
                BrushName(name: brush.name, selected: selected, shaded: model.shadedNames[key] == true)
                    .padding(.leading, 18)
                    .padding(.top, 11.75)
                // a pinned brush's mark, at the cell's top right
                if pinned {
                    PcIconView(PcIcons.pinMark, size: 16, color: selected ? .white : Pc.panelTitle)
                        .padding(.top, 12.5)
                        .padding(.trailing, 11)
                        .frame(maxWidth: .infinity, alignment: .trailing)
                }
            }
            .frame(maxWidth: .infinity, minHeight: brushHeight, maxHeight: brushHeight, alignment: .topLeading)
            .clipShape(shape)
            .opacity(ui.dragged == brush ? 0.3 : 1)
            .background(GeometryReader { g in
                Color.clear.preference(key: BrushFrames.self, value: [key: g.frame(in: .named(librarySpace))])
            })
            .contentShape(Rectangle())
            .onTapGesture {
                // with a brush swiped open, a tap only closes it
                if model.swipedBrush != nil {
                    model.swipedBrush = nil
                    return
                }
                model.chooseBrush(brush)
                // a highlighted brush opens in the studio
                if selected {
                    model.studio.open()
                }
            }
            .modifier(HeldDrag(
                ui: ui,
                began: { at in
                    // the Recent set's brushes stay where they are
                    guard !recent else { return }
                    model.swipedBrush = nil
                    let frame = ui.brushFrames[key] ?? .zero
                    ui.grab = CGPoint(x: at.x - frame.minX, y: at.y - frame.minY)
                    ui.dragged = brush
                },
                ended: { ui.dropBrush(model, shown: listed) }
            ))
        }
        // a brush's preview goes by place, and is drawn again when the library changes
        .task(id: preview == nil) {
            if preview == nil {
                model.loadPreview(brush)
            }
        }
    }
}

/// The dragged brush or set, following the finger.
private struct DragGhost: View {
    let model: CanvasModel
    let ui: LibraryUi

    var body: some View {
        let at = CGPoint(x: ui.dragAt.x - ui.grab.x, y: ui.dragAt.y - ui.grab.y)
        let shape = RoundedRectangle(cornerRadius: 12, style: .circular)
        if let d = ui.dragged {
            let key = CanvasModel.brushKey(d)
            ZStack(alignment: .topLeading) {
                shape.fill(Pc.rowSelected.opacity(0.9))
                if let preview = model.previews[key] {
                    SampleStroke(image: preview)
                }
                Text(verbatim: d.name)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(Color.white)
                    .lineLimit(1)
                    .padding(.leading, 18)
                    .padding(.top, 11)
            }
            .frame(width: ui.brushFrames[key]?.width ?? 0, height: brushHeight)
            .clipShape(shape)
            .offset(x: at.x, y: at.y)
            .allowsHitTesting(false)
        } else if let i = ui.draggedSet, model.brushSets.indices.contains(i) {
            let set = model.brushSets[i]
            HStack(spacing: 0) {
                PcIconView(model.setIcon(set), size: 40, color: .white)
                    .padding(.leading, 4)
                Text(verbatim: set.name)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(Color.white)
                    .lineLimit(2)
                    .padding(.leading, 1)
                    .padding(.trailing, 8)
                Spacer(minLength: 0)
            }
            .frame(width: ui.setFrames[i]?.width ?? 0, height: 50)
            .background(shape.fill(Pc.rowSelected.opacity(0.9)))
            .offset(x: at.x, y: at.y)
            .allowsHitTesting(false)
        }
    }
}

/// The library's menus: the plus's, hanging below it with their right edges in line; a set's, left
/// of it and centred on it; the Recent set's, as a set's, of one item.
private struct LibraryMenus: View {
    let model: CanvasModel
    let ui: LibraryUi

    /// Where a menu `height` tall goes for a row: left of it, 6 clear, centred on it, kept below the
    /// top bar and on the screen.
    private func beside(_ frame: CGRect, height: CGFloat) -> CGPoint {
        // the panel starts under the top bar and ends 25 above the screen's edge
        let top = menuTop - Pc.barHeight
        let bottom = ui.height + 25 - height
        return CGPoint(x: frame.minX - glassMenuWidth - 6, y: min(max(frame.midY - height / 2, top), max(bottom, top)))
    }

    var body: some View {
        ZStack(alignment: .topLeading) {
            if ui.adding {
                MenuDismissal { closeAll() }
                addMenu
            }
            if ui.recentMenu, let frame = ui.setFrames[CanvasModel.recentSet] {
                MenuDismissal { closeAll() }
                let at = beside(frame, height: 65)
                GlassMenu {
                    MenuRow(icon: PcIcons.menuClear, label: "Clear") {
                        closeAll()
                        model.clearRecentBrushes()
                    }
                    .padding(.vertical, 5.5)
                }
                .offset(x: at.x, y: at.y)
                .menuTransition(.trailing)
            }
            if let i = ui.setMenu, let frame = ui.setFrames[i], model.brushSets.indices.contains(i) {
                MenuDismissal { closeAll() }
                setMenu(i, frame: frame)
            }
        }
    }

    private func closeAll() {
        withAnimation(Motion.menuClose) {
            ui.adding = false
            ui.recentMenu = false
            ui.setMenu = nil
            ui.iconPicker = false
        }
    }

    private var addMenu: some View {
        GlassMenu(overPanel: true) {
            // the Recent set holds no brushes of its own to add one to
            if model.shownSet != CanvasModel.recentSet {
                MenuRow(icon: PcIcons.menuNewBrush, label: "Create new brush") {
                    closeAll()
                    model.newBrush()
                }
            }
            MenuRow(icon: PcIcons.menuNewSet, label: "Create new set") {
                closeAll()
                model.newBrushSet()
            }
            MenuLine(gap: 11)
            MenuRow(icon: PcIcons.menuFolder, label: "Import from Files") {
                closeAll()
                ui.importing = true
            }
        }
        .offset(x: libraryWidth - glassMenuWidth - 12, y: 64)
        .menuTransition(.topTrailing)
    }

    @ViewBuilder
    private func setMenu(_ i: Int, frame: CGRect) -> some View {
        let set = model.brushSets[i]
        let canDelete = model.brushSets.count > 1
        // four rows and a line, and Delete under a line of its own
        let height: CGFloat = 14 + 160 + 21 + (canDelete ? 61 : 0)
        let at = beside(frame, height: height)
        GlassMenu {
            MenuRow(icon: PcIcons.menuRename, label: "Rename") {
                closeAll()
                model.renamingSet = i
            }
            MenuRow(icon: PcIcons.menuDuplicate, label: "Duplicate") {
                closeAll()
                model.duplicateBrushSet(i)
            }
            MenuRow(icon: model.setIcon(set), label: "Customize icon", trailing: PcIcons.menuChevron) {
                withAnimation(Motion.menuOpen) { ui.iconPicker.toggle() }
            }
            MenuLine()
            MenuRow(icon: PcIcons.menuShare, label: "Share") {
                closeAll()
                model.shareBrush(nil)
            }
            if canDelete {
                MenuLine()
                MenuRow(icon: PcIcons.menuTrash, label: "Delete", ink: Pc.menuDanger) {
                    closeAll()
                    model.question = Question(title: "Delete this set and its brushes?") { model.deleteBrushSet(i) }
                }
            }
        }
        .offset(x: at.x, y: at.y)
        .menuTransition(.trailing)
        if ui.iconPicker {
            iconPicker(i, set: set, frame: frame, rowTop: at.y + 7 + 80)
        }
    }

    /// The icons to choose from, right of the menu over the library: under the Customize icon row,
    /// or over it when there is no room below; scrolled to the set's own icon.
    private func iconPicker(_ i: Int, set: BrushSetInfo, frame: CGRect, rowTop: CGFloat) -> some View {
        let tall: CGFloat = 325
        let below = rowTop + 40
        let room = ui.height + 25 - pickerMargin
        let y = below + tall <= room ? below : rowTop - tall
        let shape = RoundedRectangle(cornerRadius: 26, style: .circular)
        return ScrollViewReader { reader in
            ScrollView {
                LazyVGrid(columns: Array(repeating: GridItem(.fixed(48), spacing: 0), count: pickerColumns), spacing: 0) {
                    ForEach(0 ..< pickedSetIcons, id: \.self) { k in
                        ZStack {
                            RoundedRectangle(cornerRadius: 21, style: .circular)
                                .fill(k == Int(set.icon) ? Pc.accent : Color.clear)
                            PcIconView(PcIcons.setIcons[k], size: 40, color: .white)
                        }
                        .frame(width: 48, height: 48)
                        .contentShape(Rectangle())
                        .onTapGesture {
                            model.changeBrushSetIcon(i, icon: k)
                            closeAll()
                        }
                        .id(k)
                    }
                }
            }
            .scrollIndicators(.hidden)
            .onAppear {
                if Int(set.icon) < pickedSetIcons {
                    reader.scrollTo(Int(set.icon), anchor: .center)
                }
            }
        }
        .padding(3)
        .frame(width: 250, height: tall)
        .background(shape.fill(Color(argb: 0xFA30_3335)))
        .overlay(shape.strokeBorder(Color(argb: 0xFF5A_5D5F), lineWidth: 1))
        .clipShape(shape)
        .offset(x: frame.minX - pickerInset, y: y)
        .menuTransition(.topLeading)
    }
}

/// The library's place under the bar: its width and which tool button it hangs from (0 is the
/// colour button, counting leftwards).
func libraryButton(_ tool: ToolKind) -> Int {
    switch tool {
    case .erase: 2
    case .smudge: 3
    case .paint: 4
    }
}

/// The library's width.
let brushLibraryWidth = libraryWidth
