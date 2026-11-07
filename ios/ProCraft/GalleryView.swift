import PhotosUI
import SwiftUI
import UIKit
import UniformTypeIdentifiers

// as measured: a thumbnail's shadow dropped below it, and the rim round it
private let thumbShadowDrop: CGFloat = 5
private var thumbShadow: Color { Look.shared.light ? Color(argb: 0x1D00_0000) : Color(argb: 0x4500_0000) }
private var thumbRim: Color { Look.shared.light ? Color(argb: 0xFFBE_BEBE) : Color(argb: 0xFF42_4242) }

private let swipePillWidth: CGFloat = 94
private let swipePillHeight: CGFloat = 56
private var swipePillFill: Color { Look.shared.light ? Color(argb: 0xFFF5_F5F5) : Color(argb: 0xDB1C_1C1C) }
private var swipePillInk: Color { Look.shared.light ? Color(argb: 0xFF57_5757) : Color(argb: 0xFFA9_A9A9) }
private let swipeDeleteFill = Color(argb: 0xFFFE_604D)
/// How far a swiped artwork fades back under its actions.
private let swipedFade = 0.64
private let doneBlue = Color(argb: 0xFF01_90FF)
private let renameBlur: CGFloat = 20
/// How long Gallery Recovery's pie takes to come most of the way, the card shown long enough to be
/// read however quick the search; and how long it stays once done.
private let recoverySearchSteps = 44
private let recoveryStep = Duration.milliseconds(16)
private let recoveryDone = Duration.milliseconds(900)

/// A gallery cell's sizes: its pitch both ways (the screen's width over the columns, three upright
/// and four on its side), the content's width (40 less) and the thumbnail box's height (82 less).
private struct CellSize {
    let pitch: CGFloat
    let width: CGFloat
    let thumb: CGFloat

    init(screenWidth: CGFloat, columns: Int) {
        pitch = screenWidth / CGFloat(columns)
        width = pitch - 40
        thumb = pitch - 82
    }

    /// Where a touch on the cell is on its name rather than its picture.
    var nameTop: CGFloat {
        39 + thumb + 8
    }
}

/// An artwork's or stack's name being changed.
struct Renaming: Identifiable {
    let id: String
    let name: String
    let shown: GalleryArtwork
    let stack: Bool
}

/// Artworks to share, and the swiped Share button's place on the screen; none from Select.
struct GalleryShare {
    let artworks: [GalleryArtwork]
    let from: CGRect?
}

/// The gallery: its title and buttons, then the artworks kept on the iPad (the core's store, in the
/// app's own Application Support). A tap opens an artwork (on its name, renames it), a swipe left
/// shows Share, Duplicate and Delete, two fingers spread on it open it in Preview, and held it can
/// be dragged (`GalleryDragger`); Select picks several; + makes a new canvas.
struct GalleryView: View {
    let onOpen: (OpenArtwork) -> Void

    @Environment(\.displayScale) private var displayScale
    @State private var model = GalleryModel.shared
    @State private var failure: String?
    /// The artwork (by folder) whose actions a swipe left is showing.
    @State private var swiped: String?
    @State private var selecting = false
    @State private var selected: Set<String> = []
    @State private var moreOpen = false
    @State private var newCanvasOpen = false
    @State private var sharing: GalleryShare?
    /// Artworks waiting for their PDF's quality.
    @State private var pdfFor: [GalleryArtwork]?
    @State private var renaming: Renaming?
    @State private var deleting: GalleryArtwork?
    @State private var deletingSelected = false
    @State private var importAsked = false
    @State private var photoAsked = false
    @State private var photo: PhotosPickerItem?
    @State private var previewing: [GalleryArtwork]?
    /// Which of them Preview starts on.
    @State private var previewFrom = 0
    /// An artwork two fingers are spreading open into Preview.
    @State private var opening: PinchOpening?
    @State private var drag = GalleryDragShown()
    @State private var presses = GalleryPresses()
    /// The gallery growing back into place as a stack is left (0 to 1), and the stack's artworks
    /// flying back into its cell over it.
    @State private var regrown: CGFloat = 1
    @State private var flown: CGFloat = 0
    @State private var about = false
    /// Gallery Recovery's card, while it shows.
    @State private var recovery: ProgressShown?

    init(onOpen: @escaping (OpenArtwork) -> Void) {
        self.onOpen = onOpen
    }

    private func picked() -> [GalleryArtwork] {
        model.artworks.filter { selected.contains($0.dir) }
    }

    private func endSelecting() {
        selecting = false
        selected = []
        moreOpen = false
    }

    /// A stack's artworks flying back into its cell, over the gallery growing back.
    @ViewBuilder private func flyingBack(_ screen: CGSize) -> some View {
        if let back = model.flyingBack {
            let columns = screen.width > screen.height ? 4 : 3
            let cell = CellSize(screenWidth: screen.width, columns: columns)
            GeometryReader { place in
                let origin = Interface.shared.rect(place.frame(in: .global)).origin
                ZStack(alignment: .topLeading) {
                    ForEach(back.cells, id: \.artwork.dir) { flying in
                        let at = flying.at
                        ArtworkCell(
                            model: model, artwork: flying.artwork, cell: cell, version: model.version, chosen: nil, wash: false
                        )
                        .offset(
                            x: at.minX - origin.x + (back.to.minX - at.minX) * flown,
                            y: at.minY - origin.y + (back.to.minY - at.minY) * flown
                        )
                    }
                }
            }
            .allowsHitTesting(false)
        }
    }

    /// Nothing is open over the gallery, so a touch on a cell can be taken for a drag or a spread.
    private func quiet() -> Bool {
        guard !selecting, !newCanvasOpen, !model.customOpen, sharing == nil, renaming == nil, deleting == nil,
              pdfFor == nil, previewing == nil, opening == nil, !about
        else { return false }
        return true
    }

    /// Two fingers began at `point` on the window: the artwork whose picture is there starts to open,
    /// if nothing is open over the gallery.
    private func startOpening(at point: CGPoint) -> Bool {
        guard quiet(), let a = model.artworks.first(where: { model.thumbAt[$0.dir]?.contains(point) == true }),
              let from = model.thumbAt[a.dir]
        else { return false }
        swiped = nil
        opening = PinchOpening(artwork: a, from: from, at: point)
        return true
    }

    private func tapped(_ a: GalleryArtwork, at y: CGFloat, cell: CellSize) {
        if swiped != nil {
            withAnimation(Motion.swipeSettle) { swiped = nil }
        } else if selecting {
            if selected.contains(a.dir) {
                selected.remove(a.dir)
            } else {
                selected.insert(a.dir)
            }
        } else if y > cell.nameTop {
            renaming = Renaming(id: a.dir, name: a.name, shown: a, stack: false)
        } else {
            model.open(a)
        }
    }

    /// How far a swiped cell's actions shift to stay on the screen: they are wider than a narrow
    /// cell, so the first and last columns' would run off it.
    private func pillShift(_ a: GalleryArtwork, cell: CellSize, columns: Int) -> CGFloat {
        let over = max((swipePillWidth * 3 + 6 - cell.pitch) / 2, 0)
        let column = (model.entries.firstIndex { $0.dir == a.dir } ?? 0) % columns
        return column == 0 ? over : column == columns - 1 ? -over : 0
    }

    private func artworkCell(_ a: GalleryArtwork, cell: CellSize, columns: Int) -> some View {
        let open = swiped == a.dir
        let chosen = selected.contains(a.dir)
        return ArtworkCell(
            model: model, artwork: a, cell: cell, version: model.version,
            chosen: selecting ? chosen : nil, wash: drag.onto == a.dir,
            lift: model.lifted == a.dir ? Motion.artworkLifted : model.pressed == a.dir ? Motion.artworkPressed : 1
        )
        .modifier(CellPlace(model: model, dir: a.dir))
        .modifier(FlyIn(from: model.flyOutOf))
        // dragged, it stays behind faint
        .opacity(drag.dragging == a.dir ? draggedAlpha : 1)
        .opacity(open ? 1 - swipedFade : 1)
        // out of its place while two fingers spread it
        .opacity(opening?.artwork.dir == a.dir ? 0 : 1)
        .overlay(alignment: .top) {
            if open {
                SwipeActions(
                    onShare: { at in sharing = GalleryShare(artworks: [a], from: at) },
                    onDuplicate: {
                        swiped = nil
                        model.duplicate(a)
                    },
                    onDelete: {
                        swiped = nil
                        deleting = a
                    }
                )
                // centred on the thumbnail's frame
                .padding(.top, 39 + cell.thumb / 2 - swipePillHeight / 2)
                .offset(x: pillShift(a, cell: cell, columns: columns))
                .transition(.scale(scale: 0.2, anchor: .trailing).combined(with: .opacity))
            }
        }
        .zIndex(open ? 1 : 0)
        .contentShape(Rectangle())
        .onTapGesture { location in tapped(a, at: location.y, cell: cell) }
        .simultaneousGesture(
            DragGesture(minimumDistance: 14).onChanged { drag in
                guard !selecting, opening == nil, self.drag.dragging == nil, abs(drag.translation.width) > abs(drag.translation.height) else { return }
                let next = drag.translation.width < 0 ? a.dir : nil
                if next != swiped {
                    withAnimation(Motion.swipeSettle) { swiped = next }
                }
            }
        )
    }

    private func stackCell(dir: String, name: String, artworks: [GalleryArtwork], cell: CellSize) -> some View {
        StackCell(name: name, artworks: artworks, cell: cell, version: model.version, dim: selecting, wash: drag.onto == dir)
            .modifier(CellPlace(model: model, dir: dir))
            .opacity(drag.dragging == dir ? draggedAlpha : 1)
            .contentShape(Rectangle())
            .onTapGesture { location in
                guard !selecting else { return }
                if location.y > cell.nameTop, let top = artworks.first {
                    renaming = Renaming(id: dir, name: name, shown: top, stack: true)
                } else {
                    model.showStack(dir, name: name)
                }
            }
    }

    private func grid(_ screen: CGSize) -> some View {
        let columns = screen.width > screen.height ? 4 : 3
        let cell = CellSize(screenWidth: screen.width, columns: columns)
        return ScrollView {
            LazyVGrid(columns: Array(repeating: GridItem(.fixed(cell.pitch), spacing: 0), count: columns), spacing: 0) {
                ForEach(model.entries, id: \.dir) { entry in
                    switch entry {
                    case let .artwork(artwork):
                        artworkCell(artwork, cell: cell, columns: columns)
                    case let .stack(dir, name, artworks):
                        stackCell(dir: dir, name: name, artworks: artworks, cell: cell)
                    }
                }
            }
            .padding(.bottom, 24)
        }
        .scrollIndicators(.hidden)
        .scrollDisabled(opening != nil)
        // a stack left: the gallery grows back into place, fading in
        .scaleEffect(Motion.stackPopScale + (1 - Motion.stackPopScale) * regrown)
        .opacity(Double(regrown))
        .background {
            GeometryReader { place in
                Color.clear.onChange(of: place.frame(in: .global), initial: true) { _, frame in
                    model.gridAt = Interface.shared.rect(frame)
                }
            }
        }
    }

    @ViewBuilder private func header(_ screen: CGSize) -> some View {
        if selecting {
            SelectHeader(
                title: model.openStack?.name ?? "ProCraft",
                count: selected.count,
                wide: screen.width > screen.height,
                canStack: model.openStack == nil,
                onStack: {
                    model.stack(picked())
                    endSelecting()
                },
                onPreview: showPicked,
                onShare: { sharing = GalleryShare(artworks: picked(), from: nil) },
                onDuplicate: {
                    picked().forEach(model.duplicate)
                    endSelecting()
                },
                onDelete: { deletingSelected = true },
                onMore: { moreOpen.toggle() },
                onDone: endSelecting
            )
        } else {
            GalleryHeader(
                stack: model.openStack?.name,
                backLit: drag.backLit,
                onBackPlaced: { model.backAt = $0 },
                onBack: { model.showStack(nil) },
                onAbout: { withAnimation(Motion.easeInOut(0.3)) { about = true } },
                onSelect: {
                    swiped = nil
                    selecting = true
                },
                onImport: { importAsked = true },
                onPhoto: { photoAsked = true },
                onNew: { withAnimation(Motion.menuOpen) { newCanvasOpen.toggle() } }
            )
        }
    }

    private func startRecovery() {
        guard recovery == nil else { return }
        let said = "Looking for lost or corrupted artworks"
        Task { @MainActor in
            let search = Task { @MainActor in await model.recoverLost() }
            for step in 0 ..< recoverySearchSteps {
                recovery = ProgressShown(progress: 0.95 * Float(step) / Float(recoverySearchSteps), detail: said)
                try? await Task.sleep(for: recoveryStep)
            }
            await search.value
            recovery = ProgressShown(progress: 1, done: true, detail: said)
            try? await Task.sleep(for: recoveryDone)
            recovery = nil
            withAnimation(Motion.easeInOut(0.15)) { about = false }
        }
    }

    private func showPicked() {
        previewFrom = 0
        previewing = picked()
    }

    /// An artwork spreading open, and Preview, over the whole screen.
    @ViewBuilder private var previews: some View {
        if let spreading = opening {
            PinchOpeningOverlay(opening: spreading) { opened in
                if opened {
                    let all = model.artworks
                    previewFrom = all.firstIndex { $0.dir == spreading.artwork.dir } ?? 0
                    previewing = all
                }
                opening = nil
            }
        }
        if let list = previewing {
            ArtworkPreview(list: list, version: model.version, start: previewFrom) { previewing = nil }
        }
        if about {
            AboutApp(onRecover: startRecovery) {
                withAnimation(Motion.easeInOut(0.15)) { about = false }
            }
            .transition(.opacity)
        }
    }

    /// The menus and panels over the gallery.
    @ViewBuilder private func overlays(_ screen: CGSize) -> some View {
        if newCanvasOpen {
            Color.clear
                .contentShape(Rectangle())
                .onTapGesture { withAnimation(Motion.menuClose) { newCanvasOpen = false } }
            NewCanvasPanel(
                model: model, screen: screen, scale: displayScale,
                onDone: { withAnimation(Motion.menuClose) { newCanvasOpen = false } }
            )
            .menuTransition(.topTrailing)
        }
        if moreOpen {
            Color.clear
                .contentShape(Rectangle())
                .onTapGesture { moreOpen = false }
            MoreOptions(
                screen: screen, count: selected.count, canStack: model.openStack == nil,
                onStack: {
                    moreOpen = false
                    model.stack(picked())
                    endSelecting()
                },
                onPreview: {
                    moreOpen = false
                    showPicked()
                },
                onDuplicate: {
                    moreOpen = false
                    picked().forEach(model.duplicate)
                    endSelecting()
                }
            )
        }
        if let share = sharing {
            Color.clear
                .contentShape(Rectangle())
                .onTapGesture { sharing = nil }
            ImageFormats(share: share, screen: screen) { format in
                sharing = nil
                swiped = nil
                if format == "PDF" {
                    pdfFor = share.artworks
                } else {
                    model.share(share.artworks, format: format, from: share.from)
                }
                if selecting {
                    endSelecting()
                }
            }
        }
    }

    /// Everything that asks something over the gallery.
    @ViewBuilder private var alerts: some View {
        AlertView(
            content: deleting.map {
                AlertContent(title: "Delete “\($0.name)”?", message: "This can't be undone.", action: "Delete", icon: PcIcons.trash)
            },
            onCancel: { deleting = nil },
            onAction: {
                if let deleting {
                    model.delete(deleting)
                }
                deleting = nil
            }
        )
        AlertView(
            content: deletingSelected ? AlertContent(
                title: selected.count == 1 ? "Delete 1 artwork?" : "Delete \(selected.count) artworks?",
                message: "This can't be undone.", action: "Delete", icon: PcIcons.trash
            ) : nil,
            onCancel: { deletingSelected = false },
            onAction: {
                picked().forEach(model.delete)
                deletingSelected = false
                endSelecting()
            }
        )
        AlertView(
            content: pdfFor != nil ? AlertContent(
                title: "PDF quality", action: "Best", icon: PcIcons.page, destructive: false,
                cancel: "Good", extra: "Better"
            ) : nil,
            onCancel: { pdfFor = nil },
            onAction: { sharePdf(PdfQuality.best) },
            onExtra: { sharePdf(PdfQuality.better) },
            onFirst: { sharePdf(PdfQuality.good) }
        )
    }

    private func sharePdf(_ quality: UInt8) {
        let list = pdfFor ?? []
        pdfFor = nil
        model.share(list, format: "PDF", quality: quality, from: nil)
    }

    var body: some View {
        cards(catchers(GeometryReader { place in screenContent(place) }))
            .onAppear {
                model.onOpen = onOpen
                model.reload()
                #if DEBUG
                    // the capture says what it wants once the window is up
                    DispatchQueue.main.asyncAfter(deadline: .now() + 1) { showForCapture() }
                #endif
            }
            // an artwork just left is saved while the gallery shows: its new thumbnail read then
            .onReceive(NotificationCenter.default.publisher(for: Gallery.keptNotification)) { note in
                model.artworkSaved()
                if let reason = note.object as? String {
                    failure = saveFailureSaid(reason: reason)
                }
            }
            .alert("Can't open the artwork", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(verbatim: failure ?? "")
            }
            .fileImporter(isPresented: $importAsked, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
                if case let .success(urls) = result {
                    model.importFiles(urls)
                }
            }
            .photosPicker(isPresented: $photoAsked, selection: $photo, matching: .images)
            .onChange(of: photo) { _, item in
                guard let item else { return }
                photo = nil
                Task { @MainActor in
                    model.newFromPicture(try? await item.loadTransferable(type: Data.self))
                }
            }
    }

    /// What reads the gallery's touches on the window, and the gallery growing back from a stack.
    private func catchers<Content: View>(_ content: Content) -> some View {
        content
            .background(Pc.galleryBackground.ignoresSafeArea())
            // Preview has the whole screen, its close button where the status bar's battery is
            .statusBarHidden(previewing != nil)
            .background {
                PinchCatcher(
                    onBegin: startOpening(at:),
                    onChange: { scale, middle in opening?.follow(scale: scale, at: middle) },
                    onEnd: { opening?.end() }
                )
            }
            .background { GalleryDragCatcher(model: model, shown: drag, canStart: quiet) }
            .background { PressCatcher(model: model, presses: presses) { quiet() || selecting } }
            .onChange(of: model.leftStack) {
                regrown = 0
                flown = 0
                withAnimation(Motion.stackPop) {
                    regrown = 1
                    flown = 1
                }
            }
    }

    /// What shows over the whole gallery: Preview, the alerts, the cards.
    private func cards<Content: View>(_ content: Content) -> some View {
        content
            .overlay {
                ZStack {
                    previews
                }
            }
            .overlay { alerts }
            .overlay {
                ProgressCard(shown: model.exportShown, title: "Exporting", glyph: PcIcons.tabShare, onCancel: model.cancelExport)
            }
            .overlay {
                ProgressCard(shown: model.importShown, title: "Importing", glyph: PcIcons.tabImport, onCancel: model.cancelImport)
            }
            .overlay {
                ProgressCard(shown: recovery, title: "Gallery Recovery", glyph: PcIcons.tabImport, onCancel: nil)
            }
    }

    private func screenContent(_ place: GeometryProxy) -> some View {
            let screen = Interface.shared.screen(place.size)
            let renamed = renaming != nil || about
            return ZStack(alignment: .topLeading) {
                VStack(spacing: 0) {
                    header(screen.size)
                    if model.entries.isEmpty {
                        Text(verbatim: "Tap + to start a new canvas")
                            .font(.system(size: 17))
                            .foregroundStyle(Pc.galleryText)
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                    } else {
                        grid(screen.size)
                    }
                }
                .overlay { flyingBack(screen.size) }
                .blur(radius: renamed ? renameBlur : 0)
                .animation(.easeInOut(duration: renamed ? 0.3 : 0.15), value: renamed)
                overlays(screen.size)
                if let what = renaming {
                    RenameArtwork(renaming: what, from: model.thumbAt[what.shown.dir], screen: screen.size) { name in
                        if name != what.name {
                            if what.stack {
                                model.renameStack(what.id, name)
                            } else {
                                model.rename(what.shown, name)
                            }
                        }
                        renaming = nil
                    }
                    .id(what.id)
                }
                if let text = model.message {
                    Text(verbatim: text)
                        .foregroundStyle(Pc.text)
                        .padding(.horizontal, 18)
                        .padding(.vertical, 12)
                        .background(RoundedRectangle(cornerRadius: 10, style: .circular).fill(Pc.panel))
                        .padding(32)
                        .frame(width: screen.size.width, height: screen.size.height, alignment: .bottom)
                        .allowsHitTesting(false)
                        .task(id: text) {
                            try? await Task.sleep(for: .seconds(3))
                            if !Task.isCancelled {
                                model.message = nil
                            }
                        }
                }
                if model.customOpen {
                    CustomCanvas(model: model) { newCanvasOpen = false }
                        .transition(.move(edge: .bottom))
                }
            }
            .animation(Motion.studioIn, value: model.customOpen)
            .sized(screen)
    }

    #if DEBUG
        /// What a capture of the gallery opens.
        private func showForCapture() {
            guard let shows = ScreenCapture.galleryShows else { return }
            ScreenCapture.seedGallery(model)
            let first = model.artworks.first
            switch shows {
            case "gallery-select":
                selecting = true
                selected = Set(model.artworks.prefix(2).map(\.dir))
            case "gallery-swipe":
                swiped = first?.dir
            case "gallery-new":
                newCanvasOpen = true
            case "gallery-formats":
                selecting = true
                sharing = GalleryShare(artworks: model.artworks, from: nil)
            case "gallery-rename":
                renaming = first.map { Renaming(id: $0.dir, name: $0.name, shown: $0, stack: false) }
            case "gallery-custom", "gallery-custom-colour":
                model.openCustom()
            case "gallery-more":
                selecting = true
                selected = Set(model.artworks.prefix(2).map(\.dir))
                moreOpen = true
            case "gallery-about":
                about = true
            case "gallery-recovery":
                about = true
                recovery = ProgressShown(progress: 0.6, detail: "Looking for lost or corrupted artworks")
            case "gallery-press":
                model.pressed = first?.dir
            case "gallery-lift":
                model.lifted = first?.dir
            case "gallery-stack":
                model.stack(Array(model.artworks.prefix(2)))
                DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
                    if case let .stack(dir, name, _)? = model.entries.first(where: { if case .stack = $0 { return true } else { return false } }) {
                        model.showStack(dir, name: name)
                    }
                }
            case "gallery-drag":
                drag.dragging = model.artworks.dropFirst().first?.dir
                drag.onto = first?.dir
            case "gallery-preview", "gallery-preview-controls":
                previewFrom = 0
                previewing = model.artworks
            case "gallery-pinch":
                // once the seeded cells have been laid out
                DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
                    guard let a = model.artworks.first, let from = model.thumbAt[a.dir] else { return }
                    let middle = CGPoint(x: from.midX, y: from.midY)
                    let spreading = PinchOpening(artwork: a, from: from, at: middle)
                    spreading.follow(scale: 1.6, at: CGPoint(x: middle.x + 60, y: middle.y + 40))
                    opening = spreading
                }
            default:
                break
            }
        }
    #endif
}

/// The title row: the app's name (in a stack, the way back and its name), the Select, Import and
/// Photo capsule and the round + that makes a new canvas.
private struct GalleryHeader: View {
    let stack: String?
    /// An artwork dragged rests on the way back, to go back.
    let backLit: Bool
    let onBackPlaced: (CGRect) -> Void
    let onBack: () -> Void
    let onAbout: () -> Void
    let onSelect: () -> Void
    let onImport: () -> Void
    let onPhoto: () -> Void
    let onNew: () -> Void

    private func word(_ label: String, _ width: CGFloat, _ action: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 16))
            .foregroundStyle(Pc.galleryText)
            .frame(width: width, height: 45)
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
    }

    var body: some View {
        ZStack(alignment: .topLeading) {
            if let stack {
                // in a stack: back to the gallery, and the stack's name
                let ink = backLit ? Pc.accent : Pc.galleryTitle
                HStack(spacing: 0) {
                    PcIconView(PcIcons.chevronLeft, size: 28, color: ink)
                        .frame(width: 40)
                    Text(verbatim: stack)
                        .font(.system(size: 36, weight: .bold))
                        .foregroundStyle(ink)
                        .lineLimit(1)
                }
                .frame(height: 41)
                .contentShape(Rectangle())
                .onTapGesture(perform: onBack)
                .background {
                    GeometryReader { place in
                        Color.clear.onChange(of: place.frame(in: .global), initial: true) { _, frame in
                            onBackPlaced(Interface.shared.rect(frame))
                        }
                    }
                }
                .padding(.leading, 20)
                .padding(.top, 12)
            } else {
                Text(verbatim: "ProCraft")
                    .font(.system(size: 36, weight: .bold))
                    .foregroundStyle(Pc.galleryTitle)
                    .contentShape(Rectangle())
                    .onTapGesture(perform: onAbout)
                    .padding(.leading, 32)
                    .padding(.top, 11)
            }
            HStack(spacing: 0) {
                Spacer()
                HStack(spacing: 0) {
                    word("Select", 69, onSelect)
                    word("Import", 71, onImport)
                    word("Photo", 66, onPhoto)
                }
                .background(Capsule().fill(Pc.capsule))
                .overlay(Capsule().strokeBorder(Pc.capsuleEdge, lineWidth: 1))
                ZStack {
                    Circle().fill(Pc.capsule).frame(width: 42, height: 42)
                    Circle().strokeBorder(Pc.capsuleEdge, lineWidth: 1).frame(width: 42, height: 42)
                    PcIconView(PcIcons.add, size: 40, color: Pc.galleryText)
                }
                .frame(width: 44, height: 45)
                .contentShape(Rectangle())
                .onTapGesture(perform: onNew)
                .padding(.leading, 12)
            }
            .padding(.top, 10)
            .padding(.trailing, 20)
        }
        .frame(height: 55, alignment: .top)
    }
}

/// The title row while artworks are picked: on a wide screen everything that can be done with
/// them, else Share, Delete and More; and the blue tick that ends it.
private struct SelectHeader: View {
    let title: String
    let count: Int
    let wide: Bool
    let canStack: Bool
    let onStack: () -> Void
    let onPreview: () -> Void
    let onShare: () -> Void
    let onDuplicate: () -> Void
    let onDelete: () -> Void
    let onMore: () -> Void
    let onDone: () -> Void

    private func word(_ label: String, _ width: CGFloat, enabled: Bool, _ action: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 16))
            .foregroundStyle(enabled ? Pc.galleryTitle : Pc.gallerySize)
            .frame(width: width, height: 45)
            .contentShape(Rectangle())
            .onTapGesture {
                if enabled {
                    action()
                }
            }
    }

    var body: some View {
        let any = count > 0
        ZStack(alignment: .topLeading) {
            Text(verbatim: title)
                .font(.system(size: 36, weight: .bold))
                .foregroundStyle(Pc.galleryTitle)
                .lineLimit(1)
                .padding(.leading, 32)
                .padding(.top, 11)
            HStack(spacing: 0) {
                Spacer()
                HStack(spacing: 0) {
                    if wide {
                        word("Stack", 65, enabled: canStack && count >= 2, onStack)
                        word("Preview", 80, enabled: any, onPreview)
                        word("Share", 65, enabled: any, onShare)
                        word("Duplicate", 91, enabled: any, onDuplicate)
                        word("Delete", 70, enabled: any, onDelete)
                    } else {
                        word("Share", 65, enabled: any, onShare)
                        word("Delete", 70, enabled: any, onDelete)
                        word("More", 61, enabled: true, onMore)
                    }
                }
                .background(Capsule().fill(Pc.capsule))
                .overlay(Capsule().strokeBorder(Pc.capsuleEdge, lineWidth: 1))
                PcIconView(PcIcons.doneTick, size: 40, color: .white)
                    .frame(width: 44, height: 45)
                    .background(Capsule().fill(doneBlue))
                    .contentShape(Capsule())
                    .onTapGesture(perform: onDone)
                    .padding(.leading, 13)
            }
            .padding(.top, 10)
            .padding(.trailing, 20)
        }
        .frame(height: 55, alignment: .top)
    }
}

/// A swiped artwork's actions, over its thumbnail.
private struct SwipeActions: View {
    let onShare: (CGRect) -> Void
    let onDuplicate: () -> Void
    let onDelete: () -> Void

    @State private var shareAt = CGRect.zero

    private func pill(_ label: String, delete: Bool = false, action: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 15, weight: .semibold))
            .foregroundStyle(delete ? Color.white : swipePillInk)
            .lineLimit(1)
            .frame(width: swipePillWidth, height: swipePillHeight)
            .background(RoundedRectangle(cornerRadius: 12, style: .circular).fill(delete ? swipeDeleteFill : swipePillFill))
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
    }

    var body: some View {
        HStack(spacing: 3) {
            pill("Share") { onShare(shareAt) }
                .background {
                    GeometryReader { place in
                        Color.clear.onChange(of: place.frame(in: .global), initial: true) { _, frame in
                            shareAt = frame
                        }
                    }
                }
            pill("Duplicate", action: onDuplicate)
            pill("Delete", delete: true, action: onDelete)
        }
        .frame(width: swipePillWidth * 3 + 6)
    }
}

/// Flies what it is on in from where `from` (the window, in the interface's points) is, as its own
/// top left there; hidden until it is placed. Nothing moves without `from`.
private struct FlyIn: ViewModifier {
    let from: CGRect?

    @State private var placed = false
    @State private var shift = CGSize.zero

    func body(content: Content) -> some View {
        content
            .background {
                GeometryReader { place in
                    Color.clear.onChange(of: place.frame(in: .global), initial: true) { _, frame in
                        guard let from, !placed else { return }
                        placed = true
                        let at = Interface.shared.rect(frame).origin
                        shift = CGSize(width: from.minX - at.x, height: from.minY - at.y)
                        DispatchQueue.main.async {
                            withAnimation(Motion.stackPushMove) { shift = .zero }
                        }
                    }
                }
            }
            .offset(shift)
            .opacity(from == nil || placed ? 1 : 0)
            .zIndex(from == nil ? 0 : 2)
    }
}

/// Tells the gallery where a cell is on the window while it shows, for where a drag lands.
private struct CellPlace: ViewModifier {
    let model: GalleryModel
    let dir: String

    func body(content: Content) -> some View {
        content.background {
            GeometryReader { place in
                Color.clear
                    .onChange(of: place.frame(in: .global), initial: true) { _, frame in
                        model.cellAt[dir] = Interface.shared.rect(frame)
                    }
                    .onDisappear { model.cellAt[dir] = nil }
            }
        }
    }
}

/// An artwork's picture in its thumbnail box, as large as fits, with a soft shadow under it.
private struct Thumbnail: View {
    let artwork: GalleryArtwork
    let cell: CellSize
    let version: Int
    var chosen: Bool?
    /// Blue, as what a dragged artwork would be stacked with.
    var wash = false
    @State private var picture: UIImage?

    var body: some View {
        let ratio = artwork.width > 0 && artwork.height > 0 ? CGFloat(artwork.width) / CGFloat(artwork.height) : 0.75
        let tall = ratio <= cell.width / cell.thumb
        let size = tall ? CGSize(width: cell.thumb * ratio, height: cell.thumb) : CGSize(width: cell.width, height: cell.width / ratio)
        let corner = RoundedRectangle(cornerRadius: 8, style: .circular)
        ZStack(alignment: .topTrailing) {
            // a picture with its background hidden shows the gallery through it (white while it
            // loads)
            corner.fill(picture == nil ? Color.white : Pc.galleryBackground)
            if let picture {
                Image(uiImage: picture)
                    .resizable()
                    .interpolation(thumbnailInterpolation(picture, shown: size.width))
            }
            if wash {
                dropWash
            }
            if let chosen {
                // unpicked, a thin light ring
                ZStack {
                    if chosen {
                        Circle().fill(Color(argb: 0xFF0F_7EFD))
                        PcIconView(PcIcons.menuTick, size: 10, color: .white)
                    } else {
                        Circle().strokeBorder(Color(argb: 0xFFCC_CCCC), lineWidth: 1)
                    }
                }
                .frame(width: 14, height: 14)
                .padding(12)
            }
        }
        .frame(width: size.width, height: size.height)
        .clipShape(corner)
        .overlay(corner.strokeBorder(thumbRim, lineWidth: 1))
        .background(
            corner.fill(thumbShadow)
                .blur(radius: 2)
                .offset(y: thumbShadowDrop)
        )
        .task(id: "\(artwork.modifiedMs)-\(version)") {
            let path = (artwork.dir as NSString).appendingPathComponent("thumbnail.png")
            picture = await Task.detached(priority: .userInitiated) { UIImage(contentsOfFile: path) }.value
        }
    }
}

/// An artwork's cell: its thumbnail, then its name and size.
private struct ArtworkCell: View {
    let model: GalleryModel
    let artwork: GalleryArtwork
    let cell: CellSize
    let version: Int
    let chosen: Bool?
    let wash: Bool
    /// How big its thumbnail shows: shrunk while pressed, lifted while held.
    var lift: CGFloat = 1

    var body: some View {
        VStack(spacing: 0) {
            Thumbnail(artwork: artwork, cell: cell, version: version, chosen: chosen, wash: wash)
                .background {
                    GeometryReader { place in
                        Color.clear
                            .onChange(of: place.frame(in: .global), initial: true) { _, frame in
                                model.thumbAt[artwork.dir] = Interface.shared.rect(frame)
                            }
                            // scrolled away it is nowhere, whatever it last said
                            .onDisappear { model.thumbAt[artwork.dir] = nil }
                    }
                }
                .frame(width: cell.width, height: cell.thumb, alignment: .bottom)
                .scaleEffect(lift)
                .padding(.top, 39)
            Text(verbatim: artwork.name)
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(chosen == true ? Color.white : Pc.galleryText)
                .lineLimit(1)
                .truncationMode(.tail)
                .padding(.top, 12)
            Text(verbatim: "\(artwork.width) × \(artwork.height)px")
                .font(.system(size: 10))
                .foregroundStyle(Pc.gallerySize)
        }
        .frame(width: cell.pitch, height: cell.pitch, alignment: .top)
    }
}

/// A stack: its newest artwork on two more cards, the name, and how many it holds.
private struct StackCell: View {
    let name: String
    let artworks: [GalleryArtwork]
    let cell: CellSize
    let version: Int
    let dim: Bool
    let wash: Bool

    var body: some View {
        VStack(spacing: 0) {
            if let top = artworks.first {
                ZStack(alignment: .bottom) {
                    // the top artwork drawn again on two cards under it, a little turned (none for one)
                    if artworks.count > 1 {
                        ForEach([5.0, -4.0], id: \.self) { turn in
                            Thumbnail(artwork: top, cell: cell, version: version)
                                .rotationEffect(.degrees(turn))
                        }
                    }
                    Thumbnail(artwork: top, cell: cell, version: version, wash: wash)
                }
                .frame(width: cell.width, height: cell.thumb, alignment: .bottom)
                .padding(.top, 39)
            }
            Text(verbatim: name)
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(dim ? Pc.gallerySize : Pc.galleryText)
                .lineLimit(1)
                .truncationMode(.tail)
                .padding(.top, 12)
            Text(verbatim: "\(artworks.count) artworks")
                .font(.system(size: 10))
                .foregroundStyle(Pc.gallerySize)
        }
        .frame(width: cell.pitch, height: cell.pitch, alignment: .top)
    }
}

/// Where the gallery's artworks are kept.
enum Gallery {
    /// An artwork left has been saved; its object is why it couldn't be, if it couldn't.
    static let keptNotification = Notification.Name("ProCraft.artworkKept")
    /// Artworks left and still being saved, on the main thread.
    static var closingSaves = 0

    static func artworkKept(failure: String?) {
        closingSaves -= 1
        NotificationCenter.default.post(name: keptNotification, object: failure)
    }

    private static var support: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory())
    }

    static let store: ArtworkStore = ArtworkStore.open(root: support.appendingPathComponent("gallery").path)

    /// Brush files brought in with no artwork open, for the next one to add to the library.
    static var pendingBrushes: URL {
        let dir = support.appendingPathComponent("pending-brushes")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }
}
