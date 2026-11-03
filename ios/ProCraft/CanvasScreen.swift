import SwiftUI
import UIKit

/// The canvas filling the screen with the interface over it: the bar across the top and the side
/// bar. The artwork is kept saved as it changes, as the app goes into the background, and as it
/// is left.
struct CanvasScreen: View {
    let model: CanvasModel

    var body: some View {
        GeometryReader { screen in
            ZStack(alignment: .topLeading) {
                CanvasHost(model: model)
                if model.panel != .none {
                    // a touch on the canvas while a panel is open closes it and goes no further
                    Color.clear
                        .contentShape(Rectangle())
                        .onTapGesture { model.closePanels() }
                }
                TopBar(model: model)
                NoticeBand(model: model)
                SideBar(model: model)
                    .offset(y: screen.size.height * Pc.sideBarCentre - Pc.sideBarHeight / 2)
                if model.panel != .none {
                    PanelHost(model: model, screen: screen.size)
                }
            }
        }
        .ignoresSafeArea()
    }
}

/// The open panel, hanging from its button under the bar, clear of the side bar.
private struct PanelHost: View {
    let model: CanvasModel
    let screen: CGSize

    var body: some View {
        // panels keep clear of the side bar
        let clear = Pc.sideBarWidth + Pc.popoverGap
        switch model.panel {
        case .brushes:
            let buttonX = screen.width - Pc.toolCentreFromRight(libraryButton(model.tool), barWidth: screen.width)
            let place = popoverPlace(width: brushLibraryWidth, buttonCentreX: buttonX, screenWidth: screen.width, minLeft: clear)
            // ending 25 above the screen's edge, as measured
            BrushLibrary(model: model, arrowX: place.arrowX)
                .frame(height: screen.height - Pc.barHeight - 25)
                .offset(x: place.left, y: Pc.barHeight)
        case .layers:
            let buttonX = screen.width - Pc.toolCentreFromRight(1, barWidth: screen.width)
            let place = popoverPlace(width: layersPanelWidth, buttonCentreX: buttonX, screenWidth: screen.width, minLeft: clear)
            LayersPanel(model: model, arrowX: place.arrowX, maxHeight: screen.height - Pc.barHeight - Pc.popoverGap)
                .offset(x: place.left, y: Pc.barHeight)
        case .color:
            let buttonX = screen.width - Pc.toolCentreFromRight(0, barWidth: screen.width)
            let place = popoverPlace(width: colorPanelWidth, buttonCentreX: buttonX, screenWidth: screen.width, minLeft: clear)
            ColorPanel(model: model, arrowX: place.arrowX)
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
    private var observers: [NSObjectProtocol] = []
    /// Back to the gallery, once the artwork is kept.
    private let leave: () -> Void
    /// The colour under the eyedropper while it is open (0xRRGGBB).
    private var picked: UInt32?

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
        model.session.startAutosave(dir: model.dir)
        canvas.onCommands = { [weak self] in self?.obey($0) }
        let pencil = UIPencilInteraction()
        pencil.delegate = self
        canvas.addInteraction(pencil)
        // the app may be ended without a word once in the background: saved as it goes there
        observers.append(NotificationCenter.default.addObserver(
            forName: UIApplication.didEnterBackgroundNotification, object: nil, queue: .main
        ) { [weak self] _ in self?.saveInBackground() })
    }

    override var prefersStatusBarHidden: Bool { true }
    override var prefersHomeIndicatorAutoHidden: Bool { true }
    // the screen's edges paint first; a second swipe is the system's
    override var preferredScreenEdgesDeferringSystemGestures: UIRectEdge { .all }

    /// What the touches asked; the rest wait for the interface that shows them.
    private func obey(_ commands: [TouchCommand]) {
        for command in commands {
            switch command {
            case .undo:
                model.undo()
            case .redo:
                model.redo()
            case .fit:
                model.session.settleToFit()
            case .strokeEnded, .shapeEnded:
                model.refresh()
            case .brushChanged:
                model.brushTouched()
            // the eyedropper (a hold, a squeeze): the colour under it, taken as it lets go (its
            // loupe is still to come)
            case let .pick(x, y):
                if let rgb = model.session.pick(x: x, y: y, side: 1)?.rgb {
                    picked = rgb
                }
            case .pickDone:
                if let rgb = picked {
                    model.setRgb(rgb)
                }
                picked = nil
                model.pickNext = false
            case .pickCancel:
                picked = nil
            default:
                break
            }
        }
    }

    /// Back to the gallery, the artwork kept.
    private func close() {
        let session = model.session
        // no save of its own after this one
        session.stopAutosave()
        do {
            try session.save(dir: model.dir)
        } catch {
            NSLog("ProCraft: keeping the artwork: %@", "\(error)")
        }
        canvas.detach()
        observers.forEach { NotificationCenter.default.removeObserver($0) }
        observers.removeAll()
        leave()
    }

    /// Saves the artwork, the system letting the app finish first (a background task).
    private func saveInBackground() {
        let app = UIApplication.shared
        let task = app.beginBackgroundTask(withName: "Keep the artwork", expirationHandler: nil)
        do {
            try model.session.save(dir: model.dir)
        } catch {
            NSLog("ProCraft: keeping the artwork: %@", "\(error)")
        }
        if task != .invalid {
            app.endBackgroundTask(task)
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
        canvas.pencilSqueezed(squeeze, action: UIPencilInteraction.preferredSqueezeAction)
    }
}
