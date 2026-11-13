import SwiftUI
import UniformTypeIdentifiers

/// A library's card: as tall as measured, how far apart, its corners, a set's row and a brush's.
private let cardHeight: CGFloat = 490
private let cardGap: CGFloat = 8
private let cardCorner: CGFloat = 16
private let cardSetRow: CGFloat = 19
private let cardBrushHeight: CGFloat = 30
/// The brushes a card has room for under its name: any more would only be cut off at its foot.
private let cardBrushesShown = 15
/// Inside a card (two to a row, 12 in from the panel's sides, 10 in from its own): the sets take
/// this share of it, the brushes the rest.
private let cardInside = (brushLibraryWidth - 24 - cardGap) / 2 - 20
private let cardSetsShare: CGFloat = 0.45
/// Menus stay below the top bar.
private let librariesMenuTop: CGFloat = 57
/// The space the cards report their places in, for a card's menu.
private let librariesSpace = "libraries"

/// Where a library's title menu hangs in its panel: from the title's left, under its row (the
/// header starting under the arrow, its title 19 down).
let libraryTitleMenuAt = CGPoint(x: 20, y: Pc.arrowHeight + 48)

/// `p`, a menu `size` big's place in a panel whose top left corner is `origin` on a screen
/// `screen` big, moved as little as keeps the menu on the screen and its top at least `top` down it.
private func keptOnScreen(_ p: CGPoint, _ size: CGSize, origin: CGPoint, screen: CGSize, below top: CGFloat) -> CGPoint {
    CGPoint(
        x: min(max(origin.x + p.x, 0), max(screen.width - size.width, 0)) - origin.x,
        y: min(max(origin.y + p.y, top), max(screen.height - size.height, top)) - origin.y
    )
}

/// A library's title menu: rename it, share it, or go back to every library's card.
struct LibraryTitleMenu: View {
    let model: CanvasModel
    let close: () -> Void

    /// Two rows, a line 11 clear, and the last row.
    static let height: CGFloat = 14 + 80 + 23 + 40

    var body: some View {
        GlassMenu(overPanel: true) {
            MenuRow(icon: PcIcons.menuRename, label: "Rename") {
                close()
                model.renamingLibrary = model.libraryPlace
            }
            MenuRow(icon: PcIcons.menuShare, label: "Share") {
                close()
                model.shareLibrary()
            }
            MenuLine(gap: 11)
            MenuRow(icon: PcIcons.menuLibraries, label: "Back to libraries") {
                close()
                model.showingLibraries = true
            }
        }
    }
}

/// What the libraries' panel has open: its title's and plus's menus, a card's menu, the file
/// picker, and where the cards are.
@Observable
private final class LibrariesUi {
    var titleMenu = false
    var adding = false
    var cardMenu: Int?
    var importing = false
    var cardFrames: [Int: CGRect] = [:]
}

private struct CardFrames: PreferenceKey {
    static let defaultValue: [Int: CGRect] = [:]
    static func reduce(value: inout [Int: CGRect], nextValue: () -> [Int: CGRect]) {
        value.merge(nextValue()) { $1 }
    }
}

/// The library panel showing every library's card (tapped, that library is brought into use;
/// held, its menu), with the plus for a new one or one from a file and the title's menu to put the
/// one the app comes with back; or a library with no sets yet, with the plus to make one.
struct LibrariesPanel: View {
    let model: CanvasModel
    let arrowX: CGFloat
    /// The panel's top left corner on the screen, and the screen's size: the menus are kept on it.
    let origin: CGPoint
    let screen: CGSize
    @State private var ui = LibrariesUi()

    var body: some View {
        let showing = model.showingLibraries
        let renaming = !showing && model.renamingLibrary == model.libraryPlace
        Popover(width: brushLibraryWidth, arrowX: arrowX) {
            PanelHeader(
                title: showing ? "Brush libraries" : model.libraryName, start: 20, end: 12, height: 52,
                chevronOn: ui.titleMenu,
                onTitle: { withAnimation(Motion.menuOpen) { ui.titleMenu = true } },
                rename: renaming ? { model.renameLibrary(model.libraryPlace, name: $0) } : nil
            ) {
                PcIconView(PcIcons.plusMedium, size: 41, color: Pc.panelGlyph)
                    .frame(width: 41, height: 40)
                    .contentShape(Rectangle())
                    .onTapGesture { withAnimation(Motion.menuOpen) { ui.adding = true } }
                    .padding(.top, 12)
            }
            if showing {
                LibraryCards(model: model, ui: ui)
            } else {
                EmptyLibrary()
            }
        }
        .coordinateSpace(.named(librariesSpace))
        .onPreferenceChange(CardFrames.self) { ui.cardFrames = $0 }
        .overlay(alignment: .topLeading) { LibrariesMenus(model: model, ui: ui, origin: origin, screen: screen) }
        .fileImporter(isPresented: $ui.importing, allowedContentTypes: [.item]) { result in
            if case let .success(url) = result {
                model.importBrushes(from: url)
            }
        }
        #if DEBUG
        .onAppear {
            switch ScreenCapture.libraryShows {
            case "libraries-title":
                ui.titleMenu = true
            case "libraries-add":
                ui.adding = true
            case "libraries-card-menu":
                ui.cardMenu = 0
            default:
                break
            }
        }
        #endif
    }
}

/// A library with no sets yet: what to do about it.
private struct EmptyLibrary: View {
    var body: some View {
        VStack(spacing: 0) {
            Text(verbatim: "This library has no brushes yet")
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(Pc.panelTitle)
                .lineLimit(2)
                .multilineTextAlignment(.center)
            Text(verbatim: "Make a set with +, then brushes in it, or bring brushes in from your files.")
                .font(.system(size: 14))
                .foregroundStyle(Pc.panelGlyph)
                .multilineTextAlignment(.center)
                .padding(.top, 8)
        }
        .padding(.horizontal, 32)
        .padding(.vertical, 48)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
    }
}

/// Every library's card, two to a row.
private struct LibraryCards: View {
    let model: CanvasModel
    let ui: LibrariesUi

    var body: some View {
        let columns = [GridItem(.flexible(), spacing: cardGap), GridItem(.flexible(), spacing: cardGap)]
        ScrollView {
            LazyVGrid(columns: columns, spacing: cardGap) {
                ForEach(Array(model.brushLibraries.enumerated()), id: \.offset) { i, library in
                    LibraryCard(model: model, ui: ui, index: i, library: library)
                }
            }
            .padding(.horizontal, 12)
            .padding(.bottom, 12)
        }
        .scrollIndicators(.hidden)
        .frame(maxHeight: .infinity, alignment: .top)
    }
}

/// A library's card: its name over a small likeness of it, its sets down the left and the brushes
/// of one of them beside them; the one in use outlined in blue.
private struct LibraryCard: View {
    let model: CanvasModel
    let ui: LibrariesUi
    let index: Int
    let library: BrushLibraryInfo

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: cardCorner, style: .circular)
        // the one in use's shown set; another's first with a brush chosen in it
        let chosen = library.current ? model.shownSet : (library.sets.firstIndex { $0.chosen != nil } ?? 0)
        VStack(alignment: .leading, spacing: 0) {
            if model.renamingLibrary == index {
                InlineName(name: library.name, color: Pc.panelTitle, fontSize: 14) { model.renameLibrary(index, name: $0) }
            } else {
                Text(verbatim: library.name)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(Pc.panelTitle)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
            HStack(alignment: .top, spacing: 0) {
                CardSets(model: model, sets: library.sets, chosen: chosen)
                    .frame(width: cardInside * cardSetsShare, alignment: .top)
                CardBrushes(model: model, library: index, brushes: library.sets.indices.contains(chosen) ? library.sets[chosen].brushes : [])
                    .padding(.leading, 6)
                    .frame(width: cardInside * (1 - cardSetsShare), alignment: .top)
            }
            .padding(.top, 8)
        }
        .padding(10)
        .frame(maxWidth: .infinity, minHeight: cardHeight, maxHeight: cardHeight, alignment: .topLeading)
        .background(shape.fill(Pc.setCell))
        .overlay {
            if library.current {
                shape.strokeBorder(Pc.accent, lineWidth: 2)
            }
        }
        .clipShape(shape)
        .contentShape(shape)
        .onTapGesture { model.openLibrary(index) }
        .onLongPressGesture(minimumDuration: 0.5, maximumDistance: 10) {
            withAnimation(Motion.menuOpen) { ui.cardMenu = index }
        }
        .background(GeometryReader { g in
            Color.clear.preference(key: CardFrames.self, value: [index: g.frame(in: .named(librariesSpace))])
        })
    }
}

/// A card's sets, the one whose brushes it shows lit.
private struct CardSets: View {
    let model: CanvasModel
    let sets: [BrushSetInfo]
    let chosen: Int

    var body: some View {
        VStack(spacing: 3) {
            ForEach(Array(sets.enumerated()), id: \.offset) { s, set in
                CardSetRow(icon: model.setIcon(set), name: set.name, lit: s == chosen)
            }
        }
    }
}

private struct CardSetRow: View {
    let icon: PcIcon
    let name: String
    let lit: Bool

    var body: some View {
        let ink = lit ? Color.white : Pc.panelTitle
        HStack(spacing: 0) {
            PcIconView(icon, size: 18, color: ink)
                .frame(width: 18, height: 18)
            Text(verbatim: name)
                .font(.system(size: 8))
                .foregroundStyle(ink)
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: 0)
        }
        .frame(height: cardSetRow)
        .background(RoundedRectangle(cornerRadius: 5, style: .circular).fill(lit ? Pc.rowSelected : Pc.popoverFill.opacity(0.35)))
    }
}

/// A card's brushes: each its name over its stroke, drawn small.
private struct CardBrushes: View {
    let model: CanvasModel
    let library: Int
    let brushes: [BrushInfo]

    var body: some View {
        VStack(spacing: 2) {
            ForEach(Array(brushes.prefix(cardBrushesShown).enumerated()), id: \.offset) { _, b in
                CardBrush(model: model, library: library, brush: b)
            }
        }
    }
}

private struct CardBrush: View {
    let model: CanvasModel
    let library: Int
    let brush: BrushInfo

    var body: some View {
        let key = "\(library)-\(brush.set)-\(brush.index)"
        let stroke = model.cardPreviews[key]
        let missing = stroke == nil
        ZStack(alignment: .topLeading) {
            if let stroke {
                Image(uiImage: stroke)
                    .resizable()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            Text(verbatim: brush.name)
                .font(.system(size: 8))
                .foregroundStyle(Pc.panelTitle)
                .lineLimit(1)
                .truncationMode(.tail)
                .padding(.leading, 3)
                .padding(.top, 1)
        }
        .frame(maxWidth: .infinity, minHeight: cardBrushHeight, maxHeight: cardBrushHeight, alignment: .topLeading)
        // drawn once it shows, and again once the libraries are read again (they go by place)
        .task(id: "\(key) \(missing)") {
            if missing {
                model.loadCardPreview(library: library, set: brush.set, index: brush.index)
            }
        }
    }
}

/// The libraries' panel's menus: the title's, the plus's and a card's, each kept on the screen.
private struct LibrariesMenus: View {
    let model: CanvasModel
    let ui: LibrariesUi
    let origin: CGPoint
    let screen: CGSize

    private func kept(_ p: CGPoint, _ size: CGSize) -> CGPoint {
        keptOnScreen(p, size, origin: origin, screen: screen, below: librariesMenuTop)
    }

    var body: some View {
        ZStack(alignment: .topLeading) {
            if ui.titleMenu {
                MenuDismissal { closeAll() }
                titleMenu
            }
            if ui.adding {
                MenuDismissal { closeAll() }
                addMenu
            }
            if let i = ui.cardMenu, let frame = ui.cardFrames[i], model.brushLibraries.indices.contains(i) {
                MenuDismissal { closeAll() }
                cardMenu(i, frame: frame)
            }
        }
    }

    private func closeAll() {
        withAnimation(Motion.menuClose) {
            ui.titleMenu = false
            ui.adding = false
            ui.cardMenu = nil
        }
    }

    /// Every library's card: the one the app comes with put back; one library: its own menu.
    @ViewBuilder
    private var titleMenu: some View {
        if model.showingLibraries {
            let at = kept(libraryTitleMenuAt, CGSize(width: glassMenuWidth, height: 14 + 40))
            GlassMenu(overPanel: true) {
                MenuRow(icon: PcIcons.menuRestore, label: "Restore default library") {
                    closeAll()
                    model.restoreLibraries()
                }
            }
            .offset(x: at.x, y: at.y)
            .menuTransition(.topLeading)
        } else {
            let at = kept(libraryTitleMenuAt, CGSize(width: glassMenuWidth, height: LibraryTitleMenu.height))
            LibraryTitleMenu(model: model) { closeAll() }
                .offset(x: at.x, y: at.y)
                .menuTransition(.topLeading)
        }
    }

    /// Hanging from the plus: a new library or one from a file; or, in a library with no sets, a new
    /// set or brushes from a file.
    private var addMenu: some View {
        let showing = model.showingLibraries
        let height: CGFloat = showing ? 14 + 80 : 14 + 40 + 23 + 40
        // the plus's lower right corner: 12 in from the header's right, its 40 under the header's
        // first 12 (the header starting under the arrow)
        let plus = CGPoint(x: brushLibraryWidth - 12, y: Pc.arrowHeight + 12 + 40)
        let spot = kept(CGPoint(x: plus.x - glassMenuWidth, y: plus.y), CGSize(width: glassMenuWidth, height: height))
        return GlassMenu(overPanel: true) {
            if showing {
                MenuRow(icon: PcIcons.menuNewLibrary, label: "Create new library") {
                    closeAll()
                    model.newLibrary()
                }
                MenuRow(icon: PcIcons.menuFolder, label: "Import from Files") {
                    closeAll()
                    ui.importing = true
                }
            } else {
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
        }
        .offset(x: spot.x, y: spot.y)
        .menuTransition(.topTrailing)
    }

    /// Over a card held, centred on it a quarter of the way down: rename, duplicate or delete it
    /// (not the only one).
    private func cardMenu(_ i: Int, frame: CGRect) -> some View {
        let canDelete = model.brushLibraries.count > 1
        let height: CGFloat = 14 + 80 + (canDelete ? 23 + 40 : 0)
        let at = kept(CGPoint(x: frame.midX - glassMenuWidth / 2, y: frame.minY + frame.height / 4), CGSize(width: glassMenuWidth, height: height))
        return GlassMenu {
            MenuRow(icon: PcIcons.menuRename, label: "Rename") {
                closeAll()
                model.renamingLibrary = i
            }
            MenuRow(icon: PcIcons.menuDuplicate, label: "Duplicate") {
                closeAll()
                model.duplicateLibrary(i)
            }
            if canDelete {
                MenuLine(gap: 11)
                MenuRow(icon: PcIcons.menuTrash, label: "Delete", ink: Pc.menuDanger) {
                    closeAll()
                    model.question = Question(title: "Delete this library?", message: "Its sets and brushes go with it.") {
                        model.deleteLibrary(i)
                    }
                }
            }
        }
        .offset(x: at.x, y: at.y)
        .menuTransition(.top)
    }
}
