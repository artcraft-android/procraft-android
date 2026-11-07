import PhotosUI
import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// The canvas filling the screen with the interface over it: the bar across the top and the side
/// bar. The artwork is kept saved as it changes, as the app goes into the background, and as it
/// is left.
struct CanvasScreen: View {
    let model: CanvasModel

    @State private var photo: PhotosPickerItem?

    private func asked(_ ask: InsertAsk) -> Binding<Bool> {
        Binding(get: { model.insertAsk == ask }, set: { if !$0 { model.insertAsk = nil } })
    }

    private func sharePdf(_ quality: UInt8) {
        if let kind = model.pdfAsking {
            model.share(kind, quality: quality)
        }
        model.pdfAsking = nil
    }

    var body: some View {
        GeometryReader { place in
            let screen = Interface.shared.screen(place.size)
            let right = AppSettings.shared.rightHanded
            let hidden = model.barsHidden
            ZStack(alignment: .topLeading) {
                CanvasHost(model: model)
                if model.panel != .none {
                    // a touch on the canvas while a panel is open closes it and goes no further
                    Color.clear
                        .contentShape(Rectangle())
                        .onTapGesture { model.tapOutsidePanel() }
                }
                if let pick = model.picking {
                    Loupe(pick: pick, current: CanvasSpace.shared.color(model.colorRgb()), pixels: model.pixels)
                    if !hidden {
                        PickingBar(fromReference: pick.fromReference)
                            .offset(y: Pc.barHeight)
                    }
                }
                // with the interface hidden, a button in the top left brings it back
                if hidden {
                    ShowInterfaceButton(model: model)
                        .transition(.opacity)
                }
                // a slider's panel kept up goes with the next touch anywhere else
                if model.sliderPanelKept, model.sliderPanel != nil {
                    Color.clear
                        .contentShape(Rectangle())
                        .gesture(DragGesture(minimumDistance: 0).onChanged { _ in
                            model.sliderPanel = nil
                            model.sliderPanelKept = false
                        })
                }
                GuideOverlay(model: model)
                if model.transforming {
                    TransformOverlay(model: model)
                }
                if model.selecting {
                    SelectionOverlay(model: model)
                }
                ColorDropBand(drop: model.drop, width: screen.size.width)
                // the top bar slides up off the screen and the side bar out to its side as they fade
                TopBar(model: model)
                    .offset(y: hidden ? -Pc.barHeight : 0)
                    .opacity(hidden ? 0 : 1)
                NoticeBand(model: model)
                SideBar(model: model, right: right)
                    .frame(maxWidth: .infinity, alignment: right ? .trailing : .leading)
                    .offset(x: sideBarShift(right), y: screen.size.height * Pc.sideBarCentre - Pc.sideBarHeight / 2)
                    .opacity(hidden ? 0 : 1)
                // over the canvas, under the panels; not over a page that takes the screen
                if !hidden, !model.canvasInfoOpen, !model.gestureControlsOpen, model.crop.shown == nil, !model.timeLapse.replaying,
                   model.animatedExport == nil {
                    AnimationBar(model: model, screen: screen.size)
                        .frame(width: screen.size.width, height: screen.size.height, alignment: .bottom)
                }
                if model.panel != .none || model.panelFading {
                    // the one view all the way, so a list fading keeps its place
                    PanelHost(model: model, screen: screen.size)
                        .opacity(model.panelFading ? 0 : 1)
                        .allowsHitTesting(!model.panelFading)
                }
                if !hidden {
                    FloatingColors(model: model, screen: screen.size)
                }
                QuickShapeHandles(model: model)
                if !hidden {
                    QuickShapeBar(model: model)
                        .offset(y: Pc.barHeight)
                }
                if model.recoloring {
                    RecolorReticle(model: model)
                }
                if model.opacityLayer != nil, !hidden {
                    LayerOpacityBar(value: model.opacityValue)
                        .offset(y: Pc.barHeight)
                }
                if model.adjustment == .clone, !hidden {
                    ClonePuck(model: model)
                }
                if model.adjustment == .perspectiveBlur, !hidden {
                    PerspectivePuck(model: model)
                }
                if model.adjusting, !hidden {
                    AdjustmentBar(model: model)
                        .offset(y: Pc.barHeight)
                    AdjustModes(model: model)
                }
                if model.adjustment == .liquify, !hidden {
                    LiquifyPanel(model: model, screen: screen.size)
                        .transition(.move(edge: .bottom))
                }
                if model.adjusting, !hidden {
                    AdjustmentMenu(model: model, screen: screen.size)
                }
                // the bottom toolbars rise from below the screen, and drop away
                ZStack {
                    if model.recoloring, !hidden {
                        RecolorPanel(tool: model.recolor)
                            .transition(.move(edge: .bottom))
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
                .animation(model.recoloring ? Motion.toolbarIn : Motion.toolbarOut, value: model.recoloring)
                ZStack {
                    if model.adjusting, model.adjustment != .liquify, !hidden {
                        AdjustmentPanel(model: model, screen: screen.size)
                            .transition(.move(edge: .bottom))
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
                .animation(model.adjusting ? Motion.toolbarIn : Motion.toolbarOut, value: model.adjusting)
                ZStack {
                    if model.transforming, !hidden {
                        TransformToolbar(model: model, screenWidth: screen.size.width)
                            .transition(.move(edge: .bottom))
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
                .animation(model.transforming ? Motion.toolbarIn : Motion.toolbarOut, value: model.transforming)
                if model.transforming, !hidden {
                    TransformPopover(model: model, screen: screen.size)
                    TransformNumeric(model: model, screen: screen.size)
                }
                ZStack {
                    if model.selecting, !hidden {
                        SelectionToolbar(model: model, screenWidth: screen.size.width)
                            .transition(.move(edge: .bottom))
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
                .animation(model.selecting ? Motion.toolbarIn : Motion.toolbarOut, value: model.selecting)
                if model.selecting, !hidden {
                    SelectionPopover(model: model, screen: screen.size)
                }
                if model.adjustment == .gradientMap, let pick = model.gradientMap.stopColour, !hidden {
                    // over the stop, its arrow at the stop's top, kept inside the window
                    let left = min(max(pick.at.x - compactColorsWidth / 2, 0), max(screen.size.width - compactColorsWidth, 0))
                    Color.clear
                        .contentShape(Rectangle())
                        .onTapGesture { model.gradientMap.endColour() }
                    CompactColors(c: pick.held, arrowX: pick.at.x - left, screen: screen.size, side: .bottom, harmonyKind: model.harmonyKind)
                        .frame(width: screen.size.width, height: pick.at.y, alignment: .bottomLeading)
                        .offset(x: left)
                }
                TextEditing(model: model, screen: screen.size)
                if model.reference.on {
                    ReferenceWindow(model: model, screen: screen.size)
                        .transition(.opacity)
                }
                if model.singleTouchCompanion {
                    TouchCompanion(model: model, screen: screen.size)
                }
                if let format = model.animatedExport {
                    AnimatedExport(model: model, format: format)
                        .transition(.move(edge: .bottom))
                }
                if model.timeLapse.replaying {
                    TimeLapseReplay(model: model)
                        .transition(.opacity)
                }
                CropOverlay(model: model)
                if model.guide.editing {
                    GuideEditor(model: model, screen: screen.size)
                }
                if model.copyPasteOpen {
                    CopyPastePanel(model: model, screen: screen.size)
                }
                QuickMenuView(model: model, screen: screen.size)
                if let choice = model.layerChoice {
                    LayerChoiceList(model: model, choice: choice, screen: screen.size)
                }
                if model.studio.isOpen {
                    BrushStudio(model: model)
                    #if DEBUG
                        .sidewaysInCaptures(screen.size)
                    #endif
                        .transition(.move(edge: .bottom))
                    NoticeBand(model: model)
                }
                if model.licencesOpen {
                    LicencesPage { withAnimation(Motion.studioOut) { model.licencesOpen = false } }
                        .transition(.move(edge: .bottom))
                }
                if model.canvasInfoOpen {
                    CanvasInfoPage(model: model, screen: screen.size)
                        .transition(.move(edge: .bottom))
                }
                if model.gestureControlsOpen {
                    GestureControls(model: model)
                        .transition(.move(edge: .bottom))
                }
                ColorDropDot(drop: model.drop)
                ProgressCard(shown: model.exportShown, title: "Exporting", glyph: PcIcons.tabShare) {
                    model.exportCancelled = true
                }
                // its Cancel: the card goes at once, and the brushes read are left out
                ProgressCard(shown: model.importShown, title: "Importing", glyph: PcIcons.tabImport) {
                    model.importCancelled = true
                    model.importShown = nil
                }
                AlertView(
                    content: model.pdfAsking != nil ? AlertContent(
                        title: "PDF quality", action: "Best", icon: PcIcons.page, destructive: false,
                        cancel: "Good", extra: "Better"
                    ) : nil,
                    onCancel: { model.pdfAsking = nil },
                    onAction: { sharePdf(PdfQuality.best) },
                    onExtra: { sharePdf(PdfQuality.better) },
                    onFirst: { sharePdf(PdfQuality.good) }
                )
                AlertView(
                    content: model.saying.map {
                        AlertContent(title: $0.title, message: $0.message, action: "OK", destructive: false, alone: true)
                    },
                    onCancel: { model.saying = nil },
                    onAction: { model.saying = nil }
                )
                // the time-lapse in full or cut to 30 seconds (a tap outside: neither)
                AlertView(
                    content: model.timeLapse.lengthAsking ? AlertContent(
                        title: "Video length", action: "30 Seconds", icon: PcIcons.clock, destructive: false, cancel: "Full length"
                    ) : nil,
                    onCancel: { model.timeLapse.lengthAsking = false },
                    onAction: {
                        model.timeLapse.lengthAsking = false
                        model.shareTimeLapse(seconds: 30)
                    },
                    onFirst: {
                        model.timeLapse.lengthAsking = false
                        model.shareTimeLapse(seconds: nil)
                    }
                )
                // recording turned off with something recorded: deleted, or kept (a tap beside it keeps it)
                AlertView(
                    content: model.timeLapse.purgeAsking ? AlertContent(
                        title: "Purge Video?", message: "The video recorded so far will be deleted.\nIt can't be brought back.",
                        action: "Purge", icon: PcIcons.alertQuestion, cancel: "Don't Purge"
                    ) : nil,
                    onCancel: { model.timeLapse.answerPurge(false) },
                    onAction: { model.timeLapse.answerPurge(true) }
                )
                BlockedDialog(model: model)
                AlertView(
                    content: model.question.map {
                        AlertContent(title: $0.title, message: $0.message, action: $0.action, icon: $0.icon)
                    },
                    onCancel: { model.question = nil },
                    onAction: {
                        let perform = model.question?.perform
                        model.question = nil
                        perform?()
                    }
                )
                AlertView(
                    content: model.lockedDeleteTold ? AlertContent(
                        title: "Locked layer selection",
                        message: "A locked layer can’t be deleted. Unlock it first.",
                        action: "OK",
                        icon: PcIcons.pageLocked,
                        destructive: false,
                        alone: true
                    ) : nil,
                    onCancel: { model.lockedDeleteTold = false },
                    onAction: { model.lockedDeleteTold = false }
                )
                AlertView(
                    content: model.askRasterize ? AlertContent(
                        title: "Rasterize this text layer?",
                        message: "Its text can't be edited afterwards; you can paint on it.",
                        action: "Rasterize",
                        destructive: false
                    ) : nil,
                    onCancel: { model.askRasterize = false },
                    onAction: { model.rasterizeCurrent() }
                )
            }
            .animation(Motion.bars, value: hidden)
            .animation(model.sideBarShown ? Motion.sideBarIn : Motion.sideBarOut, value: model.sideBarShown)
            .sized(screen)
        }
        .ignoresSafeArea()
        .fileImporter(isPresented: asked(.file), allowedContentTypes: [.image]) { result in
            if case let .success(url) = result {
                model.insertFile(url, isPrivate: model.insertPrivate)
            }
        }
        .photosPicker(isPresented: asked(.photo), selection: $photo, matching: .images)
        .onChange(of: photo) { _, item in
            guard let item else { return }
            photo = nil
            Task { @MainActor in
                let data = try? await item.loadTransferable(type: Data.self)
                model.insertPicture(data, name: nil, isPrivate: model.insertPrivate)
            }
        }
        .fullScreenCover(isPresented: asked(.camera)) {
            CameraPicker { picture in
                model.insertAsk = nil
                if let picture {
                    model.insertPhoto(picture, isPrivate: model.insertPrivate)
                }
            }
            .ignoresSafeArea()
        }
    }

    /// How far the side bar has moved off its edge: all the way with the interface hidden or a
    /// mode taking the screen.
    private func sideBarShift(_ right: Bool) -> CGFloat {
        guard model.barsHidden || !model.sideBarShown else { return 0 }
        return right ? Pc.sideBarWidth : -Pc.sideBarWidth
    }
}

/// The current layer turned a change away (locked, hidden, or an empty group): Cancel, unlock or
/// show it, or open Layers. What was tried is not done afterwards.
private struct BlockedDialog: View {
    let model: CanvasModel

    var body: some View {
        AlertView(
            content: model.blocked.map(content),
            onCancel: { model.answerBlocked(fix: false, openLayers: false) },
            onAction: { model.answerBlocked(fix: false, openLayers: true) },
            onExtra: { model.answerBlocked(fix: true, openLayers: false) }
        )
    }

    private func content(_ b: BlockedLayer) -> AlertContent {
        let name = b.layer.name
        switch b.why {
        case .locked:
            return AlertContent(
                title: "Layer is locked", message: "\u{201C}\(name)\u{201D} can't be changed while it is locked.",
                action: "Open Layers", icon: PcIcons.pageLocked, destructive: false, extra: "Unlock"
            )
        case .hidden:
            return AlertContent(
                title: "Layer is hidden", message: "\u{201C}\(name)\u{201D} can't be changed while it is hidden.",
                action: "Open Layers", icon: PcIcons.eyeStruck, destructive: false, extra: "Show"
            )
        case .emptyGroup:
            return AlertContent(
                title: "Group is empty", message: "\u{201C}\(name)\u{201D} holds no layers to change.",
                action: "Open Layers", icon: PcIcons.folder, destructive: false
            )
        }
    }
}

/// The open panel, hanging from its button under the bar, clear of the side bar.
private struct PanelHost: View {
    let model: CanvasModel
    let screen: CGSize

    /// Where a panel `width` wide goes under its button: clear of the side bar, on either side.
    private func place(_ width: CGFloat, _ buttonX: CGFloat) -> PopoverPlace {
        let clear = Pc.sideBarWidth + Pc.popoverGap
        return AppSettings.shared.rightHanded
            ? popoverPlace(width: width, buttonCentreX: buttonX, screenWidth: screen.width, minRight: clear)
            : popoverPlace(width: width, buttonCentreX: buttonX, screenWidth: screen.width, minLeft: clear)
    }

    var body: some View {
        switch model.panel == .none ? model.lastPanel : model.panel {
        case .brushes:
            let buttonX = screen.width - Pc.toolCentreFromRight(libraryButton(model.tool), barWidth: screen.width)
            let place = place(brushLibraryWidth, buttonX)
            // ending 25 above the screen's edge, as measured
            BrushLibrary(model: model, arrowX: place.arrowX, origin: CGPoint(x: place.left, y: Pc.barHeight), screen: screen)
                .frame(height: screen.height - Pc.barHeight - 25)
                .offset(x: place.left, y: Pc.barHeight)
        case .layers:
            let buttonX = screen.width - Pc.toolCentreFromRight(1, barWidth: screen.width)
            if let held = model.backgroundPick {
                let place = place(colorPanelWidth, buttonX)
                ColorPanel(model: model, arrowX: place.arrowX, screenHeight: screen.height, background: held)
                    .offset(x: place.left, y: Pc.barHeight)
                    // closed any way at all, the colour is kept
                    .onDisappear { model.endBackgroundPick() }
            } else {
                let place = place(layersPanelWidth, buttonX)
                LayersPanel(model: model, arrowX: place.arrowX, maxHeight: screen.height - Pc.barHeight - Pc.popoverGap)
                    .offset(x: place.left, y: Pc.barHeight)
            }
        case .actions:
            let buttonX = Pc.actionCentre(0, barWidth: screen.width)
            let place = place(actionsPanelWidth, buttonX)
            // ending 25 above the screen's edge, its rows scrolling, as measured
            ActionsPanel(model: model, arrowX: place.arrowX)
                .frame(maxHeight: screen.height - Pc.barHeight - 25, alignment: .top)
                .offset(x: place.left, y: Pc.barHeight)
        case .adjustments:
            let buttonX = Pc.actionCentre(1, barWidth: screen.width)
            let place = place(adjustmentsPanelWidth, buttonX)
            AdjustmentsPanel(model: model, arrowX: place.arrowX)
                .frame(maxHeight: screen.height - Pc.barHeight - 25, alignment: .top)
                .offset(x: place.left, y: Pc.barHeight)
        case .color:
            let buttonX = screen.width - Pc.toolCentreFromRight(0, barWidth: screen.width)
            let place = place(colorPanelWidth, buttonX)
            ColorPanel(model: model, arrowX: place.arrowX, screenHeight: screen.height)
                .offset(x: place.left, y: Pc.barHeight)
        default:
            EmptyView()
        }
    }
}

/// The canvas's view and its Pencil interaction, kept by a controller of their own.
private struct CanvasHost: UIViewControllerRepresentable {
    let model: CanvasModel

    func makeUIViewController(context: Context) -> CanvasViewController {
        CanvasViewController(model: model)
    }

    func updateUIViewController(_ controller: CanvasViewController, context: Context) {}
}

final class CanvasViewController: UIViewController, UIPencilInteractionDelegate {
    private let model: CanvasModel
    private let canvas: CanvasView
    private lazy var dropper = CanvasDropper(model: model)
    private var observers: [NSObjectProtocol] = []
    /// Back to the gallery, once the artwork is kept.
    private let leave: () -> Void

    init(model: CanvasModel) {
        self.model = model
        leave = model.onClose
        canvas = CanvasView(session: model.session)
        super.init(nibName: nil, bundle: nil)
        // the top bar's Gallery keeps the artwork first
        model.onClose = { [weak self] in self?.close() }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("the canvas is made in code")
    }

    override func loadView() {
        view = canvas
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        let session = model.session
        let dir = model.dir
        // a new artwork exists in the gallery from the start (an opened one is there already)
        if !FileManager.default.fileExists(atPath: (dir as NSString).appendingPathComponent("artwork.json")) {
            DispatchQueue.global(qos: .utility).async { try? session.save(dir: dir) }
            model.startedNew()
        }
        canvas.addInteraction(UIDropInteraction(delegate: dropper))
        // on a second display too, with Project canvas on
        Projection.shared.artwork(session)
        // kept saved by the core a few seconds after it changes; a failure told
        session.startAutosave(dir: dir)
        canvas.onCommands = { [weak self] in self?.obey($0) }
        canvas.beforeTouches = { [weak self] in
            self?.model.prepareTouches()
            // a field that had the keys let them go: they come back here
            if self?.isFirstResponder == false, self?.model.text.info == nil {
                self?.becomeFirstResponder()
            }
        }
        canvas.afterTouches = { [weak self] ended in self?.model.layerOpacityTouched(ended: ended) }
        let pencil = UIPencilInteraction()
        pencil.delegate = self
        canvas.addInteraction(pencil)
        // the app may be ended without a word once in the background: saved as it goes there
        observers.append(NotificationCenter.default.addObserver(
            forName: UIApplication.didEnterBackgroundNotification, object: nil, queue: .main
        ) { [weak self] _ in self?.saveInBackground() })
        observers.append(NotificationCenter.default.addObserver(
            forName: UIApplication.willEnterForegroundNotification, object: nil, queue: .main
        ) { [weak self] _ in
            self?.model.resumeTracking()
            self?.model.readAppSettings()
        })
    }

    override var canBecomeFirstResponder: Bool { true }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        becomeFirstResponder()
    }

    private static func isAlt(_ key: UIKey) -> Bool {
        key.keyCode == .keyboardLeftAlt || key.keyCode == .keyboardRightAlt
    }

    /// A hardware keyboard's shortcuts on the canvas (keys no text field took); Alt held is □ held.
    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var taken = false
        for key in presses.compactMap(\.key) {
            if Self.isAlt(key) {
                model.modifyPressed()
                taken = true
            } else if model.keyPressed(key) {
                taken = true
            }
        }
        if !taken {
            super.pressesBegan(presses, with: event)
        }
    }

    private func altUp(_ presses: Set<UIPress>) -> Bool {
        guard presses.compactMap(\.key).contains(where: Self.isAlt), model.modifyHeld else { return false }
        model.modifyReleased(cancelled: true)
        return true
    }

    override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        if !altUp(presses) {
            super.pressesEnded(presses, with: event)
        }
    }

    override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        if !altUp(presses) {
            super.pressesCancelled(presses, with: event)
        }
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        let was = model.pixels
        model.pixels = (view.window?.screen.scale ?? view.traitCollection.displayScale) * Interface.shared.scale
        if model.pixels != was {
            model.applyFitInsets()
        }
        model.screenCentre = CGPoint(x: view.bounds.midX * model.pixels, y: view.bounds.midY * model.pixels)
    }

    override var prefersStatusBarHidden: Bool { true }
    override var prefersHomeIndicatorAutoHidden: Bool { true }
    // the screen's edges paint first; a second swipe is the system's
    override var preferredScreenEdgesDeferringSystemGestures: UIRectEdge { .all }

    /// What the touches asked; those of tools not on the iPad yet do nothing until they are.
    private func obey(_ commands: [TouchCommand]) {
        for command in commands {
            switch command {
            case .undo:
                model.undo()
            case .redo:
                model.redo()
            case .clearLayer:
                model.clearCurrentLayer()
            case .toggleBars:
                model.toggleBars()
            case .fit:
                model.settleToFit()
            case let .layerSelect(x, y):
                model.layerSelect(x: x, y: y)
            case .quickShapeLast:
                model.quickShapeLast()
            case let .pick(x, y):
                model.pick(x: x, y: y)
            case .pickDone:
                model.finishPick()
            case .pickCancel:
                model.cancelPick()
            case .shapeSnapped:
                model.quickShapeSnapped()
                canvas.shapeSnapped()
            case .shapeChanged:
                model.quickShapeChanged()
            case .shapeEnded:
                model.quickShapeEnded()
                model.refresh()
            case .strokeEnded:
                model.strokeEnded()
            case .viewMoved:
                model.refreshView()
            case let .cursor(x, y):
                let pixels = model.pixels
                canvas.showCursor(at: CGPoint(x: CGFloat(x) / pixels, y: CGFloat(y) / pixels), hovering: false)
            case .cursorGone:
                canvas.showCursor(at: nil, hovering: false)
            case .textLayer:
                model.askRasterize = true
            case let .recolorMoved(x, y):
                model.recolor.move(to: CGPoint(x: CGFloat(x), y: CGFloat(y)))
            case let .recolorTapped(x, y):
                model.recolor.tap(CGPoint(x: CGFloat(x), y: CGFloat(y)))
            case .copyPaste:
                model.copyPasteOpen = true
            case .transformTouched:
                model.transform.popup = .none
            case let .transformMoved(transform):
                model.transform.moved(transform)
            case let .transformHeld(handle, x, y):
                model.transform.hold(handle, x: x, y: y)
            case let .transformLetGo(handle, x, y):
                model.transform.release(handle, x: x, y: y)
            case let .transformNumeric(angle, x, y, right):
                let at = CGPoint(x: CGFloat(x) / model.pixels, y: CGFloat(y) / model.pixels)
                model.transform.numeric = TransformNumericAsk(angle: angle, at: at, right: right)
            case let .selectionDrag(x0, y0, x1, y1):
                model.selection.dragged(CGPoint(x: CGFloat(x0), y: CGFloat(y0)), CGPoint(x: CGFloat(x1), y: CGFloat(y1)))
            case let .selectionThreshold(x, y, threshold, done):
                model.selection.colourSlid(CGPoint(x: CGFloat(x), y: CGFloat(y)), threshold: threshold, done: done)
            case .freehandChanged:
                model.selection.freehandChanged()
            case .selectionChanged:
                model.selection.touched()
            case let .fillThreshold(threshold):
                model.drop.pourThreshold(threshold)
            case .filled:
                model.drop.filled()
            case let .quickMenu(x, y):
                model.toggleQuickMenu(at: CGPoint(x: CGFloat(x) / model.pixels, y: CGFloat(y) / model.pixels))
            case .adjustChanged:
                model.adjustModesOpen = false
                model.readAdjust()
            case let .adjustMenu(x, y):
                if !model.closedAdjustMenu {
                    model.adjustMenuAt = CGPoint(x: CGFloat(x) / model.pixels, y: CGFloat(y) / model.pixels)
                }
            case .brushChanged:
                model.brushTouched()
            case .showColours:
                model.toggle(.color)
            case .showBrushes:
                model.toggle(.brushes)
            default:
                break
            }
        }
    }

    /// Back to the gallery, the artwork kept.
    private func close() {
        let session = model.session
        let dir = model.dir
        let lapse = model.timeLapse.lapse
        model.noteTime()
        model.stopWatching()
        model.keepChoicesNow()
        canvas.detach()
        observers.forEach { NotificationCenter.default.removeObserver($0) }
        observers.removeAll()
        // the second display let go before the canvas is
        Projection.shared.artwork(nil)
        leave()
        Gallery.closingSaves += 1
        // off the main thread: a large artwork takes a while, and the gallery shows meanwhile
        DispatchQueue.global(qos: .userInitiated).async {
            // after the autosave under way, if one is, and no save of its own after this one
            session.stopAutosave()
            var failure: String?
            do {
                try session.save(dir: dir)
            } catch {
                failure = "\(error)"
                NSLog("ProCraft: keeping the artwork: %@", "\(error)")
            }
            // the artwork file keeps a time-lapse as video
            _ = lapse.forPackage()
            lapse.close()
            // its memory free at once, not when the object goes
            session.releaseCanvas()
            DispatchQueue.main.async { Gallery.artworkKept(failure: failure) }
        }
    }

    /// Saves the artwork off the main thread, the system letting the app finish first (a background
    /// task); the app may be ended without a word once it is in the background.
    private func saveInBackground() {
        model.noteTime()
        let model = model
        let session = model.session
        let dir = model.dir
        let app = UIApplication.shared
        var task = UIBackgroundTaskIdentifier.invalid
        task = app.beginBackgroundTask(withName: "Keep the artwork") {
            if task != .invalid {
                app.endBackgroundTask(task)
                task = .invalid
            }
        }
        DispatchQueue.global(qos: .userInitiated).async {
            var failure: String?
            do {
                try session.save(dir: dir)
            } catch {
                failure = "\(error)"
                NSLog("ProCraft: keeping the artwork: %@", "\(error)")
            }
            DispatchQueue.main.async {
                if let failure {
                    model.saveFailed(failure)
                } else {
                    model.saveSucceeded()
                }
                if task != .invalid {
                    app.endBackgroundTask(task)
                    task = .invalid
                }
            }
        }
    }

    // MARK: UIPencilInteractionDelegate

    /// The double-tap before iPadOS 17.5 (from then on it comes with where the Pencil hovers, below).
    func pencilInteractionDidTap(_ interaction: UIPencilInteraction) {
        if #available(iOS 17.5, *) {
            return
        }
        canvas.pencilDoubleTapped(at: nil, action: UIPencilInteraction.preferredTapAction)
    }

    @available(iOS 17.5, *)
    func pencilInteraction(_ interaction: UIPencilInteraction, didReceiveTap tap: UIPencilInteraction.Tap) {
        canvas.pencilDoubleTapped(at: tap.hoverPose?.location, action: UIPencilInteraction.preferredTapAction)
    }

    @available(iOS 17.5, *)
    func pencilInteraction(_ interaction: UIPencilInteraction, didReceiveSqueeze squeeze: UIPencilInteraction.Squeeze) {
        let action = UIPencilInteraction.preferredSqueezeAction
        // what a squeeze opens over the canvas clears the way, as does the system's own palette
        let purpose = Gestures.shared.purposeOf(.penSqueeze)
        let opens = purpose == .eyedropper || purpose == .quickmenu || purpose == .layerSelect
        if squeeze.phase == .began, opens || action == .showContextualPalette {
            model.closePopovers()
        }
        canvas.pencilSqueezed(squeeze, action: action)
    }
}
