import SwiftUI
import UIKit

/// Project canvas: the whole picture, without the interface, on a second display. The display's
/// scene gives a layer; the open artwork, with the setting on, draws into it.
final class Projection {
    static let shared = Projection()

    /// The second display's view, while there is one.
    private weak var view: ProjectionView?
    /// The artwork open, while there is one.
    private weak var session: CanvasSession?
    private var attached = false

    /// A second display came, or went (none).
    func display(_ view: ProjectionView?) {
        self.view = view
        update()
    }

    /// An artwork opened, or (none) was left: before it lets go of its canvas.
    func artwork(_ session: CanvasSession?) {
        if session == nil {
            detach()
        }
        self.session = session
        update()
    }

    /// The display's size changed.
    func resized() {
        guard attached, let session, let size = view?.pixels else { return }
        session.resizeProjection(width: size.width, height: size.height)
    }

    /// Shows the artwork there if everything is in place, else lets go of the display.
    func update() {
        guard AppSettings.shared.projectCanvas, let session, let view, let metal = view.layer as? CAMetalLayer else {
            detach()
            return
        }
        guard !attached else {
            resized()
            return
        }
        let size = view.pixels
        // the core takes a retain of its own, let go with the window
        let pointer = UInt64(UInt(bitPattern: Unmanaged.passRetained(metal).toOpaque()))
        let id = acquireLayer(layer: pointer)
        guard id != 0 else { return }
        do {
            try session.attachProjection(window: id, width: size.width, height: size.height)
            attached = true
        } catch {
            releaseLayer(id: id)
            NSLog("ProCraft: projecting the canvas: %@", "\(error)")
        }
    }

    private func detach() {
        guard attached else { return }
        attached = false
        session?.detachProjection()
    }
}

/// The second display's whole screen, a layer the core draws the canvas into, on black.
final class ProjectionView: UIView {
    override class var layerClass: AnyClass {
        CAMetalLayer.self
    }

    /// Its size in pixels.
    var pixels: (width: UInt32, height: UInt32) {
        let scale = window?.screen.scale ?? contentScaleFactor
        return (UInt32(max(1, (bounds.width * scale).rounded())), UInt32(max(1, (bounds.height * scale).rounded())))
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let scale = window?.screen.scale ?? contentScaleFactor
        contentScaleFactor = scale
        layer.contentsScale = scale
        Projection.shared.resized()
    }
}

/// The second display's scene: one window of one view, shown while the display is there. Named in
/// `project.yml`'s scene manifest for the external display's role; the app's own scene is SwiftUI's.
final class ProjectionSceneDelegate: NSObject, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(_ scene: UIScene, willConnectTo _: UISceneSession, options _: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: scene)
        let controller = UIViewController()
        let view = ProjectionView()
        view.backgroundColor = .black
        controller.view = view
        window.rootViewController = controller
        window.isHidden = false
        self.window = window
        Projection.shared.display(view)
    }

    func sceneDidDisconnect(_: UIScene) {
        Projection.shared.display(nil)
        window = nil
    }
}
