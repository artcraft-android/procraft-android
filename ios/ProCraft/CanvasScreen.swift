import SwiftUI
import UIKit

/// The canvas filling the screen, with a way back to the gallery: kept saved as it changes, as
/// the app goes into the background, and as it is left.
struct CanvasScreen: UIViewControllerRepresentable {
    let artwork: OpenArtwork
    let onClose: () -> Void

    func makeUIViewController(context: Context) -> CanvasViewController {
        CanvasViewController(artwork: artwork, onClose: onClose)
    }

    func updateUIViewController(_ controller: CanvasViewController, context: Context) {}
}

final class CanvasViewController: UIViewController, UIPencilInteractionDelegate {
    private let artwork: OpenArtwork
    private let onClose: () -> Void
    private let canvas: CanvasView
    private var observers: [NSObjectProtocol] = []
    /// The colour under the eyedropper while it is open (0xRRGGBB).
    private var picked: UInt32?

    init(artwork: OpenArtwork, onClose: @escaping () -> Void) {
        self.artwork = artwork
        self.onClose = onClose
        canvas = CanvasView(session: artwork.session)
        super.init(nibName: nil, bundle: nil)
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
        artwork.session.startAutosave(dir: artwork.dir)
        canvas.onCommands = { [weak self] in self?.obey($0) }
        let pencil = UIPencilInteraction()
        pencil.delegate = self
        canvas.addInteraction(pencil)
        let gallery = UIButton(type: .system)
        gallery.setTitle("Gallery", for: .normal)
        gallery.addTarget(self, action: #selector(close), for: .touchUpInside)
        gallery.translatesAutoresizingMaskIntoConstraints = false
        canvas.addSubview(gallery)
        NSLayoutConstraint.activate([
            gallery.leadingAnchor.constraint(equalTo: canvas.safeAreaLayoutGuide.leadingAnchor, constant: 16),
            gallery.topAnchor.constraint(equalTo: canvas.safeAreaLayoutGuide.topAnchor, constant: 8),
        ])
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
                _ = artwork.session.undo()
            case .redo:
                _ = artwork.session.redo()
            case .fit:
                artwork.session.settleToFit()
            // the eyedropper (a hold, a squeeze): the colour under it, taken as it lets go (its
            // loupe is still to come)
            case let .pick(x, y):
                if let rgb = artwork.session.pick(x: x, y: y, side: 1)?.rgb {
                    picked = rgb
                }
            case .pickDone:
                if let rgb = picked {
                    let channel = { (shift: UInt32) in Float((rgb >> shift) & 0xFF) / 255 }
                    artwork.session.setColor(r: channel(16), g: channel(8), b: channel(0))
                }
                picked = nil
            case .pickCancel:
                picked = nil
            default:
                break
            }
        }
    }

    /// Back to the gallery, the artwork kept.
    @objc private func close() {
        let session = artwork.session
        // no save of its own after this one
        session.stopAutosave()
        do {
            try session.save(dir: artwork.dir)
        } catch {
            NSLog("ProCraft: keeping the artwork: %@", "\(error)")
        }
        canvas.detach()
        observers.forEach { NotificationCenter.default.removeObserver($0) }
        observers.removeAll()
        onClose()
    }

    /// Saves the artwork, the system letting the app finish first (a background task).
    private func saveInBackground() {
        let app = UIApplication.shared
        let task = app.beginBackgroundTask(withName: "Keep the artwork", expirationHandler: nil)
        do {
            try artwork.session.save(dir: artwork.dir)
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
