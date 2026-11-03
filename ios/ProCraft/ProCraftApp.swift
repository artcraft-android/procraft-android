import os
import SwiftUI

/// ProCraft on the iPad: for now a plain gallery and the canvas the core draws, painted on with the
/// Pencil and moved with fingers (the interface over it is still to come).
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
    @State private var open: OpenArtwork?

    var body: some View {
        if let open {
            CanvasScreen(artwork: open) { self.open = nil }
                .ignoresSafeArea()
                .statusBarHidden()
                .persistentSystemOverlays(.hidden)
                .defersSystemGestures(on: .all)
        } else {
            GalleryView { self.open = $0 }
        }
    }
}
