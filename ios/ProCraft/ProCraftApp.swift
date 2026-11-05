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
/// open (`canvas` for none, `gallery` for the gallery), or one of the screens below it: a menu of
/// the library, a page of the brush studio, something open over it, a source editor. Nothing can
/// be touched in a capture, so what a touch would open is opened here.
enum ScreenCapture {
    /// What the studio opens over its page once it is up.
    static var studioShows: ((StudioModel, StudioUi) -> Void)?
    /// The control its page is scrolled to.
    static var studioRow: String?
    /// The page the keypad opens on.
    static var keypadPage = KeypadPage.numeric
    /// What the source editor opens over itself, and what the brush library opens.
    static var sourceShows: String?
    static var libraryShows: String?
    /// What the layers panel opens.
    static var layersShows: String?
    /// The studio laid out on its side: the simulator stays upright whatever is asked of it.
    static var sideways = false

    /// The studio's pages, by capture.
    private static let pages: [String: Int] = [
        "studio": 3, "studio-taper": 2, "studio-grain": 4, "studio-stylus": 9, "studio-materials": 11,
        "studio-preview": 12, "studio-about": 13,
    ]

    /// Where a value is in the portrait studio, for a keypad to hang from.
    private static let pill = CGRect(x: 648, y: 951, width: 72, height: 28)

    private static func studio(_ model: CanvasModel, page: Int, shows: ((StudioModel, StudioUi) -> Void)? = nil) {
        AppSettings.shared.studioTab = page
        studioShows = shows
        model.studio.open()
    }

    private static func keypad(_ page: KeypadPage) -> (StudioModel, StudioUi) -> Void {
        { studio, ui in
            keypadPage = page
            ui.keypad = KeypadAsk(
                id: "count", title: "Count", pill: pill, typed: typedNumber(.count, most: 16),
                onValue: { _ in }, extras: keypadExtras(studio, "count")
            )
        }
    }

    private static func source(_ model: CanvasModel, _ kind: SourceKind, shows: String? = nil) {
        sourceShows = shows
        model.studio.open()
        model.studio.sourceEditing = kind
    }

    /// The layers panel over a few layers, what `shows` opens open.
    private static func layers(_ model: CanvasModel, shows: String) {
        layersShows = shows
        model.addLayer()
        model.addLayer()
        model.addLayer()
        if shows == "picked", let other = model.layers.first(where: { !$0.current }) {
            model.pickLayer(other)
        }
        model.toggle(.layers)
    }

    private static func library(_ model: CanvasModel, shows: String) {
        libraryShows = shows
        model.toolPressed(.paint)
    }

    /// Opens what capture `name` shows, if it is one of these.
    private static func open(_ name: String, in model: CanvasModel) {
        if let page = pages[name] {
            studio(model, page: page)
            return
        }
        switch name {
        case "studio-roundness":
            studioRow = "roundness"
            studio(model, page: 3)
        case "studio-tilt":
            studioRow = "tilt"
            studio(model, page: 9)
        case "studio-landscape":
            sideways = true
            studio(model, page: 3)
        case "keypad":
            studio(model, page: 3, shows: keypad(.numeric))
        case "keypad-pressure":
            studio(model, page: 3, shows: keypad(.pressure))
        case "keypad-tilt":
            studio(model, page: 3, shows: keypad(.tilt))
        case "blend-list":
            studio(model, page: 4) { studio, ui in
                ui.blendList = BlendListAsk(modes: studio.brushModes, current: 0, centreY: 700) { _ in }
            }
        case "node-menu":
            studio(model, page: 9) { _, ui in
                let points = [CGPoint.zero, CGPoint(x: 0.5, y: 0.6), CGPoint(x: 1, y: 1)]
                let box = CGRect(x: 109, y: 620, width: 607, height: 450)
                ui.nodeMenu = NodeMenuAsk(points: points, index: 1, box: box) { _ in }
            }
        case "pad-menu":
            studio(model, page: 3) { studio, _ in
                studio.padButtonCentre = 176
                studio.padMenuOpen = true
            }
        case "source":
            source(model, .grain)
        case "source-shape":
            source(model, .shape)
        case "source-import":
            source(model, .grain, shows: "import")
        case "repeat":
            source(model, .grain, shows: "repeat")
        case "source-library":
            source(model, .grain, shows: "library")
        case "source-landscape":
            sideways = true
            source(model, .grain)
        case "layers-many":
            layers(model, shows: "")
        case "layers-swipe":
            layers(model, shows: "swipe")
        case "layers-options":
            layers(model, shows: "options")
        case "layers-blend":
            layers(model, shows: "blend")
        case "layers-menu":
            layers(model, shows: "menu")
        case "layers-rename":
            layers(model, shows: "rename")
        case "layers-picked":
            layers(model, shows: "picked")
        case "library-add":
            library(model, shows: "add")
        case "library-set-menu":
            library(model, shows: "set-menu")
        case "library-icons":
            library(model, shows: "icons")
        case "library-search":
            library(model, shows: "search")
        case "library-swipe":
            library(model, shows: "swipe")
        case "library-recent":
            library(model, shows: "recent")
        default:
            break
        }
    }

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
            case "actions": model.toggle(.actions)
            case "adjustments": model.toggle(.adjustments)
            default: Self.open(name, in: model)
            }
        }
    }
}

extension View {
    /// In a capture of the sideways studio, laid out as it is on its side.
    @ViewBuilder
    func sidewaysInCaptures(_ screen: CGSize) -> some View {
        if ScreenCapture.sideways {
            frame(width: screen.height, height: screen.width)
                .rotationEffect(.degrees(90))
                .frame(width: screen.width, height: screen.height)
        } else {
            self
        }
    }
}
#endif
