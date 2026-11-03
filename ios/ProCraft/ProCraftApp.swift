import os
import SwiftUI
import UIKit

/// ProCraft on the iPad: the gallery, and an artwork open on the canvas the core draws, painted
/// on with the Pencil and moved with fingers, the interface over it.
@main
struct ProCraftApp: App {
    init() {
        // the layers' textures may take a quarter of what the app may use (an iPad app is ended
        // well short of the device's memory); none said (the simulator), all of them
        let available = os_proc_available_memory()
        if available > 0 {
            setResidentBudget(bytes: UInt64(available / 4))
        }
        // the GPU device and the canvases' pipelines made ahead of the first artwork
        DispatchQueue.global(qos: .userInitiated).async { warmGpu() }
    }

    var body: some Scene {
        WindowGroup {
            RootView()
        }
    }
}

/// An artwork open on the canvas, and its folder in the gallery.
struct OpenArtwork {
    let session: CanvasSession
    let dir: String
}

/// The gallery, or the artwork open in place of it.
struct RootView: View {
    @State private var open: CanvasModel?

    var body: some View {
        Group {
            if let open {
                CanvasScreen(model: open)
                    .statusBarHidden()
                    .persistentSystemOverlays(.hidden)
                    .defersSystemGestures(on: .all)
            } else {
                GalleryView { artwork in
                    let model = CanvasModel(session: artwork.session, dir: artwork.dir)
                    model.onClose = { self.open = nil }
                    self.open = model
                }
            }
        }
        #if DEBUG
        .task { ScreenCapture.start { open = $0 } }
        #endif
    }
}

#if DEBUG
/// For the screen captures the iPad workflow takes on the simulator: launched with `-screen NAME`,
/// the app turns on its side and opens a new canvas the size of the screen with the panel NAME
/// open (`canvas` for none, `gallery` for the gallery).
enum ScreenCapture {
    static func start(open: @escaping (CanvasModel) -> Void) {
        let arguments = ProcessInfo.processInfo.arguments
        guard let at = arguments.firstIndex(of: "-screen"), at + 1 < arguments.count else { return }
        let name = arguments[at + 1]
        guard let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene else { return }
        scene.requestGeometryUpdate(.iOS(interfaceOrientations: .landscapeRight)) { _ in }
        if name == "gallery" {
            return
        }
        // after the turn, the screen's size the other way round
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            let bounds = scene.screen.bounds
            let scale = scene.screen.scale
            let width = UInt32(max(bounds.width, bounds.height) * scale)
            let height = UInt32(min(bounds.width, bounds.height) * scale)
            let model = CanvasModel(session: CanvasSession(width: width, height: height), dir: Gallery.store.newFolder(folder: nil))
            open(model)
            switch name {
            case "brushes": model.toolPressed(.paint)
            case "layers": model.toggle(.layers)
            case "colors": model.toggle(.color)
            default: break
            }
        }
    }
}
#endif
