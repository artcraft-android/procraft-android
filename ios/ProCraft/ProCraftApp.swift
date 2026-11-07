import AVFoundation
import os
import SwiftUI
import UIKit
import UniformTypeIdentifiers

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
        TextFonts.prepare()
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

    @ViewBuilder private var screens: some View {
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

    var body: some View {
        screens
            .background {
                GeometryReader { place in
                    Color.clear.onChange(of: place.size, initial: true) { _, size in
                        Interface.shared.fit(size)
                    }
                }
                .ignoresSafeArea()
            }
            .overlay { PluginAlerts() }
            // a file opened with the app is brought into the gallery, which shows: an open artwork
            // is saved and closed first
            .onOpenURL { url in
                open?.onClose()
                GalleryModel.shared.importOpened(url)
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
    /// The page the actions panel opens on.
    static var actionsPage = ActionsPage.main
    /// What the layers panel opens, and the colour panel.
    static var layersShows: String?
    static var colorsShows: String?
    /// The studio laid out on its side: the simulator stays upright whatever is asked of it.
    static var sideways = false
    /// The screens, by how their names begin, that need the canvas attached.
    private static let canvasScreens = ["liquify", "adjust-", "transform", "selection", "effect-", "crop", "recolor", "layer-opacity", "guide", "text", "reference", "timelapse", "animation", "page-assist", "export-"]
    /// What opens over the gallery.
    static var galleryShows: String?
    /// Animation Assist's settings open on the onion skins' colours.
    static var animationColors = false
    /// The Adjustments menu opens on the filters.
    static var filtersShow = false
    /// A gradient's second stop opens its colours, in this mode.
    static var stopOpens = false
    static var compactMode: ColorMode?
    /// What opens over the QuickMenu.
    static var quickShows: String?

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

    /// A few artworks for the gallery's captures to show.
    static func seedGallery(_ gallery: GalleryModel) {
        if gallery.entries.isEmpty {
            for size in [(1200, 900), (900, 1200), (1000, 1000)] {
                let session = CanvasSession(width: UInt32(size.0), height: UInt32(size.1))
                try? session.save(dir: Gallery.store.newFolder(folder: nil))
            }
            gallery.reload()
        }
        // never drawn, they have no pictures of their own, and Preview and a spread show one
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        for (artwork, colour) in zip(gallery.artworks, [UIColor.systemTeal, .systemOrange, .systemPurple]) {
            let size = CGSize(width: CGFloat(artwork.width) / 4, height: CGFloat(artwork.height) / 4)
            let png = UIGraphicsImageRenderer(size: size, format: format).pngData { context in
                colour.setFill()
                context.fill(CGRect(origin: .zero, size: size))
                UIColor.white.setFill()
                context.cgContext.fillEllipse(in: CGRect(origin: .zero, size: size).insetBy(dx: size.width * 0.3, dy: size.height * 0.3))
            }
            try? png.write(to: URL(fileURLWithPath: artwork.dir).appendingPathComponent("thumbnail.png"))
        }
        gallery.artworkSaved()
    }

    /// Transform open over a filled layer: on an empty one there is nothing to move.
    private static func transform(_ model: CanvasModel) {
        if let l = model.layers.first(where: { $0.current }) {
            model.fillLayer(l)
        }
        model.toggleTransform()
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

    /// The folder of plugin files the capture packed.
    private static var pluginFiles: String? {
        ProcessInfo.processInfo.environment["CAPTURE_PLUGINS"]
    }

    /// Brings in the plugin files packed for the captures, as a person's are: read, then kept.
    private static func importPlugins() {
        guard let folder = pluginFiles, let names = try? FileManager.default.contentsOfDirectory(atPath: folder) else { return }
        let plugins = PluginsModel.shared
        for name in names where name.hasSuffix(".procraftplugin") {
            guard let bytes = FileManager.default.contents(atPath: folder + "/" + name) else { continue }
            if plugins.store.inspect(bytes: bytes).refused == nil {
                _ = plugins.store.install(bytes: bytes)
            }
        }
        plugins.read()
    }

    /// The patch called `name`, applied.
    private static func apply(_ name: String) -> PluginInfo? {
        importPlugins()
        let plugins = PluginsModel.shared
        guard let patch = plugins.patches.first(where: { $0.name == name }) else { return nil }
        plugins.setOn(patch, true)
        return plugins.patches.first { $0.name == name }
    }

    /// The interface at `size`, through its patch.
    private static func sized(_ size: Float) {
        for control in apply("Interface size")?.controls ?? [] {
            if case let .slider(slider) = control {
                PluginsModel.shared.setSlider(slider, size)
            }
        }
    }

    /// Frames of the canvas in four colours recorded, off the main thread; then `then`.
    private static func recordFrames(_ model: CanvasModel, then: @escaping () -> Void) {
        let session = model.session
        let lapse = model.timeLapse.lapse
        let side = timeLapseSide(s: lapse.settings)
        DispatchQueue.global(qos: .userInitiated).async {
            for colour: [Float] in [[1, 0, 0], [0, 1, 0], [0, 0, 1], [1, 1, 0]] {
                _ = session.setBackgroundColor(r: colour[0], g: colour[1], b: colour[2], done: true)
                if let shot = session.timeLapseFrame(maxSide: side) {
                    lapse.record(shot)
                }
            }
            DispatchQueue.main.async(execute: then)
        }
    }

    /// The middle pixel of `picture`, as hex.
    private static func middle(of picture: UIImage?) -> String {
        guard let image = picture?.cgImage else { return "none" }
        var pixel: [UInt8] = [0, 0, 0, 0]
        let space = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
        pixel.withUnsafeMutableBytes { bytes in
            let context = CGContext(
                data: bytes.baseAddress, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
                space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            )
            context?.draw(image, in: CGRect(x: -image.width / 2, y: -image.height / 2, width: image.width, height: image.height))
        }
        return String(format: "%02X%02X%02X", pixel[0], pixel[1], pixel[2])
    }

    /// What only a run can show, said on the screen: frames recorded, made into a video and read
    /// back out of it, as a person's are.
    private static func checkTimeLapse(_ model: CanvasModel) {
        let lapse = model.timeLapse.lapse
        recordFrames(model) {
            DispatchQueue.global(qos: .userInitiated).async {
                let frames = lapse.count
                let out = Sharing.exportsDir.appendingPathComponent("check.mp4")
                let made = lapse.export(to: out, seconds: nil) { _ in true }
                let asset = AVURLAsset(url: out)
                let size = asset.tracks(withMediaType: .video).first?.naturalSize ?? .zero
                let bytes = (try? out.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
                // the pictures gone, so these come out of the video
                let package = lapse.forPackage()
                let text = "\(frames) frames recorded. Made: \(made), \(asset.duration.seconds) s, \(Int(size.width)) x \(Int(size.height)), \(bytes) bytes. "
                    + "Package: \(package.segments.count) videos, \(package.width) x \(package.height). "
                    + "From the video: first \(middle(of: lapse.frame(0))), last \(middle(of: lapse.frame(frames - 1))), count \(lapse.count)."
                DispatchQueue.main.async { model.saying = ("Time-lapse check", text) }
            }
        }
    }

    /// The actions panel on its Prefs tab, turned to `page`.
    private static func prefs(_ model: CanvasModel, _ page: ActionsPage) {
        actionsPage = page
        model.actionsTab = 4
        model.toggle(.actions)
    }

    /// The colour panel in `mode`, what `shows` opens open.
    private static func colors(_ model: CanvasModel, _ mode: ColorMode, shows: String? = nil) {
        colorsShows = shows
        model.colorMode = mode
        model.toggle(.color)
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
        case "licences":
            model.licencesOpen = true
        case "canvas-info":
            model.canvasInfoOpen = true
        case "text":
            model.text.add()
        case "text-style":
            model.text.add()
            model.text.openStyle()
        case "guide":
            model.setGuideOn(true)
        case "guide-edit":
            model.guide.open()
        case "guide-symmetry":
            model.guide.open()
            model.guide.choose(.symmetry)
            model.guide.symmetryOptionsOpen = true
        case "recolor":
            model.toggleRecolor()
        case "layer-opacity":
            model.startLayerOpacity()
        case "crop":
            model.crop.begin()
        case "crop-settings":
            model.crop.begin()
            model.crop.settingsOpen = true
        case "adjust-modes":
            model.startAdjustment(.gaussianBlur)
            model.adjustModesOpen = true
        case "colors-floating", "colors-floating-palettes", "colors-floating-harmony", "colors-floating-value":
            model.colorMode = ColorMode.allCases.first { name.hasSuffix($0.label.lowercased()) } ?? .disc
            model.floatingColorsAt = CGPoint(x: 300, y: 160)
        case "animation", "animation-settings", "animation-frame", "animation-colors":
            model.animationAssist.setAssist(true, pages: false)
            for _ in 0 ..< 3 {
                model.addLayer()
            }
            model.animationAssist.settingsOpen = name == "animation-settings" || name == "animation-colors"
            model.animationAssist.frameOptionsOpen = name == "animation-frame"
            if name == "animation-colors" {
                animationColors = true
            }
        case "export-gif", "export-mp4":
            model.animationAssist.setAssist(true, pages: false)
            for _ in 0 ..< 3 {
                model.addLayer()
            }
            model.animatedExport = name == "export-gif" ? .gif : .mp4
        case "export-check":
            // what only a run can show: each format made, its size, frames back out of the video
            model.animationAssist.setAssist(true, pages: false)
            recordFrames(model) {
                for _ in 0 ..< 3 {
                    model.addLayer()
                }
                let session = model.session
                DispatchQueue.global(qos: .userInitiated).async {
                    var said: [String] = []
                    for format in [AnimatedFormat.gif, .png, .mp4, .hevc] {
                        let url = CanvasModel.exportAnimation(session, format, AnimatedChoices(fps: 12))
                        let bytes = url.flatMap { try? $0.resourceValues(forKeys: [.fileSizeKey]).fileSize } ?? 0
                        var more = ""
                        if format.video, let url {
                            let asset = AVURLAsset(url: url)
                            more = " \(asset.duration.seconds) s, \(asset.tracks(withMediaType: .video).first?.nominalFrameRate ?? 0) fps"
                        }
                        said.append("\(format.title): \(url == nil ? "failed" : "\(bytes) bytes")\(more)")
                    }
                    let text = "\(session.animationFrameCount()) frames.\n" + said.joined(separator: "\n")
                    DispatchQueue.main.async { model.saying = ("Export check", text) }
                }
            }
        case "page-assist":
            model.animationAssist.setAssist(true, pages: true)
            model.addLayer()
        case "companion":
            UserDefaults.standard.set(true, forKey: "singleTouchCompanion")
            model.readAppSettings()
            model.companionMode = .move
        case "key-layers":
            _ = model.runShortcut("l", command: false, shift: false, alt: false)
        case "settings-check":
            UserDefaults.standard.set(false, forKey: "singleTouchCompanion")
            model.readAppSettings()
            let file = Bundle.main.url(forResource: "Root", withExtension: "plist", subdirectory: "Settings.bundle")
            let page = file.flatMap { NSDictionary(contentsOf: $0) }
            let rows = (page?["PreferenceSpecifiers"] as? [Any])?.count ?? 0
            let types = ["procraft", "brushset", "abr", "swatches"].map { "\($0): \(UTType(filenameExtension: $0)?.identifier ?? "none")" }
            let kinds = (Bundle.main.object(forInfoDictionaryKey: "CFBundleDocumentTypes") as? [Any])?.count ?? 0
            model.saying = ("Settings check", "Settings page: \(rows) rows. Document kinds: \(kinds).\n" + types.joined(separator: "\n"))
        case "timelapse-tab":
            recordFrames(model) {
                model.refresh()
                model.actionsTab = 3
                model.toggle(.actions)
            }
        case "timelapse-replay":
            recordFrames(model) { model.timeLapse.replaying = true }
        case "timelapse-purge":
            recordFrames(model) { model.timeLapse.setRecording(false) }
        case "timelapse-length":
            model.timeLapse.lengthAsking = true
        case "timelapse-check":
            checkTimeLapse(model)
        case "reference":
            model.reference.setOn(true)
        case "reference-bars":
            model.reference.setOn(true)
            model.reference.toggleBars()
        case "reference-image":
            model.reference.setOn(true)
            model.reference.setMode(.image)
        case "prefs-patches":
            importPlugins()
            prefs(model, .patches)
        case "prefs-plugins":
            importPlugins()
            prefs(model, .plugins)
        case "prefs-patch-on":
            _ = apply("Touch position offset")
            prefs(model, .patches)
        case "plugin-offer":
            if let folder = pluginFiles {
                PluginsModel.shared.offer(URL(fileURLWithPath: folder + "/extract-lines.procraftplugin")) { _ in }
            }
        case "adjust-plugins":
            importPlugins()
            filtersShow = true
            model.toggle(.adjustments)
        case "effect-plugin":
            importPlugins()
            if let l = model.layers.first(where: { $0.current }) {
                model.fillLayer(l)
            }
            if let filter = PluginsModel.shared.store.filters().first {
                model.startPluginFilter(filter)
            }
        case "interface-again":
            // brought back at the size it was left at, nothing else asked of it
            break
        case "interface-small":
            sized(0.85)
        case "interface-large":
            sized(1.15)
            model.toggle(.color)
        case "effect-gradient":
            model.startAdjustment(.gradientMap)
        case "effect-gradient-edit":
            model.startAdjustment(.gradientMap)
            model.gradientMap.edit(model.gradientMap.index)
        case _ where name.hasPrefix("effect-gradient-stop"):
            stopOpens = true
            compactMode = ColorMode.allCases.first { name.hasSuffix($0.label.lowercased()) }
            model.startAdjustment(.gradientMap)
            model.gradientMap.edit(model.gradientMap.index)
        case "effect-hsb":
            model.startAdjustment(.hsb)
        case "effect-balance":
            model.startAdjustment(.colorBalance)
        case "effect-curves":
            model.startAdjustment(.curves)
        case "effect-glitch":
            model.startAdjustment(.glitch)
        case "effect-noise":
            model.startAdjustment(.noise)
        case "effect-bloom":
            model.startAdjustment(.bloom)
        case "effect-halftone":
            model.startAdjustment(.halftone)
        case "effect-chromatic":
            model.startAdjustment(.chromaticAberration)
        case "effect-perspective":
            model.startAdjustment(.perspectiveBlur)
        case "transform":
            transform(model)
        case "transform-snapping":
            transform(model)
            model.transform.popup = .snapping
        case "transform-interpolation":
            transform(model)
            model.transform.popup = .interpolation
        case "transform-warp":
            transform(model)
            model.transform.choose(TransformKind.warp)
        case "transform-numeric":
            transform(model)
            model.transform.numeric = TransformNumericAsk(angle: false, at: CGPoint(x: 560, y: 400), right: false)
        case "selection":
            model.toggleSelecting()
        case "selection-feather":
            model.toggleSelecting()
            model.selection.choose(.automatic)
            model.selection.openPopup(.feather)
        case "selection-saved":
            model.toggleSelecting()
            model.selection.openPopup(.saved)
        case "quick-menu":
            model.toggleQuickMenu(at: CGPoint(x: 372, y: 500))
        case "quick-menus":
            quickShows = "menus"
            model.toggleQuickMenu(at: CGPoint(x: 372, y: 500))
        case "quick-picker":
            quickShows = "picker"
            model.toggleQuickMenu(at: CGPoint(x: 372, y: 500))
        case "copy-paste":
            model.copyLayer()
            model.copyPasteOpen = true
        case "liquify":
            model.startAdjustment(.liquify)
        case "liquify-mode":
            model.startAdjustment(.liquify)
            model.liquifyPopup = 1
        case "liquify-twirl":
            model.startAdjustment(.liquify)
            model.chooseLiquifyKind(.twirlRight)
            model.liquifyPopup = 2
        case "adjust-menu":
            model.startAdjustment(.liquify)
            model.adjustMenuAt = CGPoint(x: 372, y: 500)
        case "prefs":
            prefs(model, .main)
        case "prefs-cursor":
            prefs(model, .cursor)
        case "prefs-pressure":
            prefs(model, .pressure)
        case "gestures":
            model.gestureControlsOpen = true
        case "gestures-hover":
            model.gesturePage = .hover
            model.gestureControlsOpen = true
        case "gestures-general":
            model.gesturePage = .general
            model.gestureControlsOpen = true
        case "colors-classic":
            colors(model, .classic)
        case "colors-harmony":
            colors(model, .harmony)
        case "colors-harmony-menu":
            colors(model, .harmony, shows: "harmony-menu")
        case "colors-value":
            colors(model, .value)
        case "colors-hex":
            colors(model, .value, shows: "hex")
        case "colors-palettes":
            colors(model, .palettes)
        case "colors-add":
            colors(model, .palettes, shows: "add")
        case "colors-palette-menu":
            colors(model, .palettes, shows: "palette-menu")
        case "colors-background":
            model.toggle(.layers)
            model.pickBackground()
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
        if name.hasPrefix("gallery-") {
            galleryShows = name
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
            // what works on the canvas waits for the view to attach it
            case _ where Self.canvasScreens.contains(where: { name.hasPrefix($0) }):
                DispatchQueue.main.asyncAfter(deadline: .now() + 2) { Self.open(name, in: model) }
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
