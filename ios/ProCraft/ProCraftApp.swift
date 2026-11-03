import os
import SwiftUI

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
}
