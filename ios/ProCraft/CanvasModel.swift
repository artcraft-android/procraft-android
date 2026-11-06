import Foundation
import Observation
import UIKit

/// Which panel hangs from the top bar.
enum Panel {
    case none, layers, color, brushes, actions, adjustments
}

/// Which slider's panel shows beside it: the size's or the opacity's.
enum SliderPanel {
    case size, opacity
}

/// A passing message under the top bar (what undo and redo did, and the like); `id` tells repeats
/// apart; a `warning` stays longer.
struct Notice: Equatable {
    let text: String
    let id: Int
    let warning: Bool
}

/// The current layer turned a change away, and why: a dialog offers to unlock or show it, or open
/// Layers.
struct BlockedLayer {
    let why: Blocked
    let layer: LayerInfo
}

/// A question over everything before something that can't be undone: what it asks, and what
/// happens on yes.
struct Question {
    let title: String
    var message = "It can't be brought back."
    var action = "Delete"
    var icon: PcIcon? = PcIcons.trash
    let perform: () -> Void
}

/// The layers under a point held with Layer Select, the first of them made current.
struct LayerChoiceAt {
    let at: CGPoint
    let ids: [UInt64]
}

/// The open artwork as the interface shows it: what the bars and panels show, read from the core
/// and changed through it.
@Observable
final class CanvasModel {
    /// How near a slider value pulls onto a notch (the core's NOTCH_NEAR).
    static let notchNear: Float = 0.04
    /// A press on □ shorter than this, nothing using it, is a tap (seconds).
    static let modifyTapSeconds: TimeInterval = 0.5

    let session: CanvasSession
    /// The artwork's folder in the gallery.
    let dir: String
    /// Back to the gallery, the artwork kept.
    @ObservationIgnored var onClose: () -> Void = {}

    private(set) var tool: ToolKind = .paint
    private(set) var size: Float = 0
    private(set) var opacity: Float = 1
    /// The colour panel's mode, kept from one session to the next.
    var colorMode: ColorMode {
        get {
            let all = ColorMode.allCases
            return all.indices.contains(AppSettings.shared.colorMode) ? all[AppSettings.shared.colorMode] : .disc
        }
        set { AppSettings.shared.colorMode = ColorMode.allCases.firstIndex(of: newValue) ?? 0 }
    }
    /// The second colour (0xRRGGBB), beside the current one in the colour panel: swapped with it,
    /// and mixed in by brushes whose colour dynamics say so.
    private(set) var secondaryRgb: UInt32 = 0xFFFFFF
    /// The paint colour as hue (0..1), saturation and brightness.
    private(set) var hue: Float = 0
    private(set) var saturation: Float = 0
    private(set) var brightness: Float = 0
    private(set) var panel: Panel = .none
    private(set) var canUndo = false
    private(set) var canRedo = false
    private(set) var currentBrush: BrushInfo?
    /// The current brush's saved sizes and opacities: the side sliders' notches (0..1).
    private(set) var sizeNotches: [Float] = []
    private(set) var opacityNotches: [Float] = []
    /// The panel beside a slider: shown while it is dragged, and kept up after it is held still.
    var sliderPanel: SliderPanel?
    /// The slider's panel stays up (the slider was held still) until the next touch elsewhere.
    var sliderPanelKept = false
    private(set) var notice: Notice?
    @ObservationIgnored private var notices = 0

    /// Window pixels to a point, as the canvas's view has it.
    var pixels: CGFloat = 2
    /// The interface is hidden (four fingers, or Full Screen).
    private(set) var barsHidden = false
    /// The eyedropper, while it is open.
    private(set) var picking: PickState?
    @ObservationIgnored private var pendingPick: (x: Float, y: Float)?
    @ObservationIgnored private var pickRunning = false
    @ObservationIgnored private var pickEnding = false
    /// A stroke held still and snapped to a shape; `quickShapeEditing` shows its handles.
    private(set) var quickShape: QuickShapeInfo?
    var quickShapeEditing = false
    /// The current layer turned a change away.
    private(set) var blocked: BlockedLayer?
    /// A stroke began on a text layer: it takes paint once rasterized.
    var askRasterize = false
    /// Layer Select found more than one layer under the finger.
    var layerChoice: LayerChoiceAt?
    /// Colours painted with lately (0xRRGGBB), newest first.
    private(set) var history: [UInt32] = []
    /// Whether a mask is current, the colour then the one masks paint with.
    @ObservationIgnored private var onMask = false
    /// The colour not in use (hue, saturation, brightness): masks' while a layer of another kind is
    /// current, the brush's while a mask is.
    @ObservationIgnored private var otherColour: [Float] = AppSettings.shared.maskColor
    /// What is drawn over the canvas follows the view while it moves into place.
    @ObservationIgnored private var settling: Timer?
    /// A save failed and was told of: the next isn't, until one succeeds again.
    @ObservationIgnored private var saveFailing = false
    @ObservationIgnored private var saveWatch: Timer?
    /// When time on the artwork last started counting; none while away.
    @ObservationIgnored private var activeSince: Date?

    /// The colour history, kept across artworks: each canvas's paint strokes add to it.
    static let colourHistory = ColourHistory()

    /// The modify button was pressed: the next touch on the canvas picks a colour (the touches
    /// know too).
    var pickNext: Bool {
        get { pickNextShown }
        set {
            pickNextShown = newValue
            session.setPickNext(on: newValue)
        }
    }

    private var pickNextShown = false

    /// □ is held down: a canvas touch now does what □ + Touch (or □ + Pen) is set to.
    private(set) var modifyHeld = false
    @ObservationIgnored private var modifyDownAt = Date.distantPast
    @ObservationIgnored private var modifyUsed = false

    // the modes the top bar's buttons open (each still to come on the iPad)
    private(set) var transforming = false
    private(set) var selecting = false
    /// The export card, and its Cancel tapped.
    var exportShown: ProgressShown?
    @ObservationIgnored var exportCancelled = false
    /// How a picture is being brought in; what waits for a PDF's quality; what a small alert says.
    var insertAsk: InsertAsk?
    var insertPrivate = false
    var pdfAsking: ShareKind?
    var saying: (title: String, message: String)?
    /// The size and opacity side bar shows.
    var showSideBar = true

    /// The QuickMenu, open around this window point (in points); the Copy & Paste panel.
    var quickMenuAt: CGPoint?
    var copyPasteOpen = false

    /// The adjustment open, its values as the core has them, the menu a tap on the canvas opened
    /// during it (where, in points), and liquify's list (1) or adjust slider (2).
    private(set) var adjustment: Adjustment?
    private(set) var adjustShown: AdjustShown?
    var adjustMenuAt: CGPoint?
    var liquifyPopup = 0
    /// Layer or Pencil shows under the bar.
    var adjustModesOpen = false
    /// The touch coming down closed that menu: the tap it makes doesn't open it again.
    @ObservationIgnored private(set) var closedAdjustMenu = false

    var adjusting: Bool {
        adjustment != nil
    }
    private(set) var recoloring = false

    /// The Actions panel's tab, kept from one session to the next.
    var actionsTab: Int {
        get { AppSettings.shared.actionsTab }
        set { AppSettings.shared.actionsTab = newValue }
    }
    /// What was last cut or copied, for pasting.
    @ObservationIgnored private var clipboard: ThumbnailImage?
    private(set) var canPaste = false
    /// The drawing guide is on.
    var guideOn: Bool {
        guide.info.on
    }

    // the layers, top first, the background under them, and their thumbnails
    private(set) var layers: [LayerInfo] = []
    private(set) var background = BackgroundInfo(r: 1, g: 1, b: 1, visible: true)
    private(set) var thumbnails: [UInt64: UIImage] = [:]
    /// The pixels' version each thumbnail was read at: read again only once its layer's is new.
    @ObservationIgnored private var thumbnailVersions: [UInt64: UInt64] = [:]
    /// The blend modes, for the letters on the layers' rows.
    @ObservationIgnored let blendModeList = blendModes()

    // the brush library
    private(set) var brushSets: [BrushSetInfo] = []
    /// The set the library shows.
    private(set) var shownSet = 0
    /// Each set's last chosen brush (its place in the set).
    private(set) var setChoice: [Int: Int] = [:]
    /// The brushes' sample strokes, by `brushKey`, and those whose stroke runs under its name.
    private(set) var previews: [String: UIImage] = [:]
    private(set) var shadedNames: [String: Bool] = [:]
    @ObservationIgnored private var previewsAsked: Set<String> = []
    /// Goes up as the library changes: a preview drawn for a place that has since moved is dropped.
    @ObservationIgnored private var previewsMade = 0
    /// The Recent set's place among the sets.
    static let recentSet = -1
    /// The Recent set: the brushes last painted with, and those pinned there.
    private(set) var recentBrushes: [RecentBrush] = []
    /// The library's brush swiped open to its buttons (its key), if one is.
    var swipedBrush: String?
    /// The set whose name is being edited.
    var renamingSet: Int?
    /// A brush found from the Recent set, to be scrolled to in its own.
    var foundBrush: String?
    /// The brush chosen before the current one, for Previous brush.
    @ObservationIgnored private var lastBrush: (set: UInt32, index: UInt32)?
    /// Brushes read from files, waiting to join the library.
    @ObservationIgnored private var brushesRead: [BrushesRead] = []
    /// The library couldn't be kept and that was told: not again until it is kept once more.
    @ObservationIgnored private var libraryFailing = false
    /// Writes of the library, one at a time: each takes the library as it is when it runs.
    private static let libraryWrites = DispatchQueue(label: "ProCraft.brushLibrary", qos: .utility)
    /// The brush studio.
    @ObservationIgnored private(set) lazy var studio = StudioModel(model: self)
    @ObservationIgnored private(set) lazy var drop = ColorDrop(model: self)
    @ObservationIgnored private(set) lazy var selection = SelectionTool(model: self)
    @ObservationIgnored private(set) lazy var transform = TransformTool(model: self)
    @ObservationIgnored private(set) lazy var gradientMap = GradientMapTool(model: self)
    @ObservationIgnored private(set) lazy var crop = CropTool(model: self)
    @ObservationIgnored private(set) lazy var recolor = RecolorTool(model: self)
    @ObservationIgnored private(set) lazy var guide = GuideTool(model: self)
    @ObservationIgnored private(set) lazy var reference = ReferenceTool(model: self)
    @ObservationIgnored private(set) lazy var text = TextTool(model: self)
    /// The middle of the screen, window pixels.
    @ObservationIgnored var screenCentre = CGPoint.zero
    /// The layer whose opacity a drag changes, while the mode is on, and its opacity as it is dragged.
    private(set) var opacityLayer: LayerInfo?
    private(set) var opacityValue: Float = 1
    /// The view as the core has it: `[a, b, c, d, e, f]`, canvas pixels to window pixels.
    private(set) var viewMatrix: [Float] = []
    /// Delete on a locked layer: why not, OK alone.
    var lockedDeleteTold = false
    /// Gesture controls are open, on this page: it opens on the page it was last on.
    var gestureControlsOpen = false
    var canvasInfoOpen = false
    var licencesOpen = false
    var gesturePage = GesturePurpose.eyedropper
    /// The palettes, kept by the shared core across artworks.
    static let paletteStore: Palettes = {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory())
        try? FileManager.default.createDirectory(at: support, withIntermediateDirectories: true)
        return Palettes.open(path: support.appendingPathComponent("palettes.json").path)
    }()
    private(set) var palettes: [PaletteInfo] = []
    /// The palette shown under the history in the other colour modes.
    private(set) var defaultPalette = 0
    var harmonyKind = HarmonyKind.complementary
    /// The background's colour being picked, instead of the layers.
    var backgroundPick: HeldColor?
    /// A question open over everything.
    var question: Question? {
        didSet {
            // while it is up, brushes read from a file wait to join the library: it may hold a
            // brush by its place
            if question == nil {
                addBrushesRead()
            }
        }
    }

    init(session: CanvasSession, dir: String) {
        self.session = session
        self.dir = dir
        _ = session.openBrushes(dir: Self.brushesDir)
        addPendingBrushes()
        brushSets = session.brushLibrary()
        readSetChoices()
        let settings = AppSettings.shared
        session.setSelectionVisibility(visibility: settings.selectionVisibility)
        applyPlugins()
        PluginsModel.shared.onChange = { [weak self] in self?.applyPlugins() }
        session.setBrushScaling(onScreen: settings.brushOnScreen)
        session.setLightInterface(on: settings.lightInterface)
        applyInput()
        session.useColourHistory(history: Self.colourHistory)
        history = Self.colourHistory.colours()
        refreshPalettes()
        // the colour as it was last left
        hue = settings.brushColor[0]
        saturation = settings.brushColor[1]
        brightness = settings.brushColor[2]
        applyColor()
        tool = session.info().tool
        currentBrush = session.currentBrush()
        if let current = currentBrush {
            shownSet = Int(current.set)
        }
        readSliders()
        refresh()
        pushSecondary()
        activeSince = Date()
        watchSaves()
    }

    /// Where the brush library and the brushes in use are kept.
    static var brushesDir: String {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory())
        let dir = support.appendingPathComponent("brushes")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.path
    }

    // MARK: tools and panels

    /// A tool button: the first tap picks the tool, the next opens its brush library.
    func toolPressed(_ pressed: ToolKind) {
        // leaving a mode for a tool keeps what it made, and only picks the tool
        guard switchModes(.tool) else {
            select(pressed)
            return
        }
        if tool == pressed {
            if let current = currentBrush {
                shownSet = Int(current.set)
            }
            toggle(.brushes)
        } else {
            select(pressed)
            if panel == .brushes {
                panel = .none
            }
        }
    }

    /// A tool button held: that tool takes the brush the current one has (erasing with the
    /// painting brush, say), and becomes the tool.
    func toolHeld(_ target: ToolKind) {
        guard let brush = currentBrush, target != tool, switchModes(.toolHeld) else { return }
        select(target)
        if session.selectBrush(set: brush.set, index: brush.index) {
            currentBrush = session.currentBrush()
            readSliders()
        }
        if panel == .brushes {
            panel = .none
        }
        let doing: String
        switch target {
        case .paint: doing = "Painting"
        case .smudge: doing = "Smudging"
        case .erase: doing = "Erasing"
        }
        showNotice("\(doing) with the current brush")
    }

    func select(_ next: ToolKind) {
        session.setTool(tool: next)
        tool = next
        readSliders()
        currentBrush = session.currentBrush()
    }

    // MARK: the brush library

    /// A brush's key among the previews: its set and place.
    static func brushKey(_ b: BrushInfo) -> String {
        "\(b.set)-\(b.index)"
    }

    /// The icon a set shows.
    func setIcon(_ set: BrushSetInfo) -> PcIcon {
        let i = Int(set.icon)
        return PcIcons.setIcons.indices.contains(i) ? PcIcons.setIcons[i] : PcIcons.brushSet
    }

    /// Whether the library lights `b`: the current brush, or the brush last chosen in its set.
    func highlighted(_ b: BrushInfo) -> Bool {
        if let c = currentBrush, c.set == b.set {
            return c.index == b.index
        }
        return setChoice[Int(b.set)] == Int(b.index)
    }

    /// Shows set `index`; its last chosen brush becomes the current one, as choosing it again
    /// would.
    func showSet(_ index: Int) {
        shownSet = index
        if index == Self.recentSet {
            recentBrushes = session.recentBrushes()
            return
        }
        guard let choice = setChoice[index], brushSets.indices.contains(index) else { return }
        let brushes = brushSets[index].brushes
        if brushes.indices.contains(choice) {
            chooseBrush(brushes[choice])
        }
    }

    func chooseBrush(_ b: BrushInfo) {
        let before = currentBrush
        guard session.selectBrush(set: b.set, index: b.index) else { return }
        // the set left behind keeps showing the brush it had
        if let before {
            if before.set != b.set || before.index != b.index {
                lastBrush = (before.set, before.index)
            }
            setChoice[Int(before.set)] = Int(before.index)
        }
        currentBrush = session.currentBrush()
        setChoice[Int(b.set)] = Int(b.index)
        readSliders()
        keepChoices()
    }

    /// Each set's last choice, as the library read last has it.
    private func readSetChoices() {
        var choices: [Int: Int] = [:]
        for (i, set) in brushSets.enumerated() {
            if let chosen = set.chosen {
                choices[i] = Int(chosen)
            }
        }
        setChoice = choices
    }

    /// As `keepChoices`, for the artwork being left.
    func keepChoicesNow() {
        keepChoices()
    }

    /// Keeps the tools' brushes, the sets' choices and each brush's sliders for the next session.
    private func keepChoices() {
        let session = session
        let dir = Self.brushesDir
        Self.libraryWrites.async { [weak self] in
            let kept = session.keepBrushChoices(dir: dir)
            DispatchQueue.main.async { self?.libraryKept(kept) }
        }
    }

    /// Tells, once until the library is kept again, that it couldn't be (the storage full, say).
    private func libraryKept(_ kept: Bool) {
        if kept {
            libraryFailing = false
        } else if !libraryFailing {
            libraryFailing = true
            showNotice("The brush library couldn't be kept.", warning: true)
        }
    }

    /// After a change to the library: read it again, keep it, and draw previews again (they go by
    /// place).
    func libraryChanged() {
        brushSets = session.brushLibrary()
        currentBrush = session.currentBrush()
        readSetChoices()
        if shownSet != Self.recentSet {
            shownSet = min(max(shownSet, 0), max(brushSets.count - 1, 0))
        }
        recentBrushes = session.recentBrushes()
        readSliders()
        previewsMade += 1
        previews = [:]
        shadedNames = [:]
        previewsAsked = []
        keepBrushLibrary()
    }

    func duplicateBrush(_ b: BrushInfo) {
        guard session.duplicateBrush(set: b.set, index: b.index) != nil else { return }
        libraryChanged()
    }

    func deleteBrush(_ b: BrushInfo) {
        if session.deleteBrush(set: b.set, index: b.index) {
            libraryChanged()
        }
    }

    /// Moves `b` to place `index` of set `set` (a brush dragged onto a set, or along its own).
    func moveBrush(_ b: BrushInfo, set: Int, index: Int) {
        if set == Int(b.set), index == Int(b.index) {
            return
        }
        if session.moveBrush(set: b.set, index: b.index, toSet: UInt32(set), toIndex: UInt32(index)) {
            libraryChanged()
        }
    }

    /// A new brush at the top of the shown set, opened in the studio.
    func newBrush() {
        if shownSet == Self.recentSet {
            shownSet = currentBrush.map { Int($0.set) } ?? 0
        }
        guard session.newBrush(set: UInt32(shownSet)) != nil else { return }
        libraryChanged()
        studio.brandNew = true
        studio.open()
    }

    /// A new set at the top of the library, shown, with its name ready to edit.
    func newBrushSet() {
        shownSet = Int(session.newBrushSet())
        libraryChanged()
        renamingSet = shownSet
    }

    func renameBrushSet(_ i: Int, name: String) {
        renamingSet = nil
        if session.renameBrushSet(set: UInt32(i), name: name) {
            libraryChanged()
        }
    }

    func changeBrushSetIcon(_ i: Int, icon: Int) {
        if session.setBrushSetIcon(set: UInt32(i), icon: UInt32(icon)) {
            libraryChanged()
        }
    }

    /// Moves set `from` to place `to` (a set held and dragged onto another); the shown set stays
    /// shown.
    func moveBrushSet(from: Int, to: Int) {
        guard from != to, session.moveBrushSet(set: UInt32(from), to: UInt32(to)) else { return }
        if shownSet == from {
            shownSet = to
        } else if shownSet > from, shownSet <= to {
            shownSet -= 1
        } else if shownSet >= to, shownSet < from {
            shownSet += 1
        }
        libraryChanged()
    }

    func duplicateBrushSet(_ i: Int) {
        guard let made = session.duplicateBrushSet(set: UInt32(i)) else { return }
        shownSet = Int(made)
        libraryChanged()
    }

    func deleteBrushSet(_ i: Int) {
        guard session.deleteBrushSet(set: UInt32(i)) else { return }
        if shownSet >= i {
            shownSet = max(shownSet - 1, 0)
        }
        libraryChanged()
    }

    /// Pins a brush of the Recent set there, or unpins it.
    func pinRecentBrush(_ b: BrushInfo, pinned: Bool) {
        guard session.pinRecentBrush(set: b.set, index: b.index, pinned: pinned) else { return }
        recentBrushes = session.recentBrushes()
        keepChoices()
    }

    /// Takes a brush out of the Recent set.
    func clearRecentBrush(_ b: BrushInfo) {
        guard session.clearRecentBrush(set: b.set, index: b.index) else { return }
        recentBrushes = session.recentBrushes()
        keepChoices()
    }

    /// Empties the Recent set, its pinned brushes too.
    func clearRecentBrushes() {
        session.clearRecentBrushes()
        recentBrushes = []
        keepChoices()
    }

    /// A brush of the Recent set shown in its own set, chosen there.
    func findBrush(_ b: BrushInfo) {
        shownSet = Int(b.set)
        chooseBrush(b)
        foundBrush = Self.brushKey(b)
    }

    /// Back to the brush chosen before this one (and Previous brush again comes back here). Nothing
    /// calls this until the gesture controls are on the iPad.
    func previousBrush() {
        guard let last = lastBrush else { return }
        let now = currentBrush
        guard session.selectBrush(set: last.set, index: last.index) else { return }
        lastBrush = now.map { ($0.set, $0.index) }
        currentBrush = session.currentBrush()
        setChoice[Int(last.set)] = Int(last.index)
        readSliders()
        keepChoices()
    }

    /// A brush, or (no brush) the shown set, written to a file and handed to the share sheet.
    func shareBrush(_ b: BrushInfo?) {
        let session = session
        let set = shownSet
        guard b != nil || brushSets.indices.contains(set) else { return }
        let name = b?.name ?? brushSets[set].name
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let bytes = b.map { session.brushAsFile(set: $0.set, index: $0.index) } ?? session.brushSetAsFile(set: UInt32(set))
            var made: URL?
            if let bytes {
                let file = Sharing.exportsDir
                    .appendingPathComponent(Sharing.safeName(name, otherwise: "Brush"))
                    .appendingPathExtension(b != nil ? "brush" : "brushset")
                if (try? bytes.write(to: file, options: .atomic)) != nil {
                    made = file
                }
            }
            DispatchQueue.main.async {
                if let made {
                    Sharing.share([made])
                } else {
                    self?.showNotice("The brush couldn't be exported.", warning: true)
                }
            }
        }
    }

    /// A brush or set file from elsewhere: read off the main thread, it joins the library and its
    /// set is shown.
    func importBrushes(from url: URL) {
        let name = url.lastPathComponent
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let bytes = url.readPicked()
            let read = bytes.flatMap { readBrushes(bytes: $0, fileName: name) }
            DispatchQueue.main.async {
                guard let self else { return }
                if bytes == nil {
                    self.showNotice("This file could not be read.", warning: true)
                } else if let read {
                    self.brushesRead.append(read)
                    self.addBrushesRead()
                } else {
                    self.showNotice("This file has no brushes in it.", warning: true)
                }
            }
        }
    }

    /// Brush files brought in with no artwork open join the library now.
    private func addPendingBrushes() {
        let manager = FileManager.default
        let dir = Gallery.pendingBrushes
        let files = (try? manager.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)) ?? []
        for file in files {
            // the name it came with, after what kept it apart
            let name = String(file.lastPathComponent.dropFirst(37))
            if let bytes = try? Data(contentsOf: file), let read = readBrushes(bytes: bytes, fileName: name) {
                _ = session.addBrushesRead(read: read, shown: 0)
            }
            try? manager.removeItem(at: file)
        }
    }

    /// Brushes read meanwhile join the library (a set of their own at the top, or a brush at the
    /// top of a set), that set shown; not while the studio or a question holds a brush by its
    /// place, which they would move: then once that ends.
    func addBrushesRead() {
        guard !studio.isOpen, question == nil, !brushesRead.isEmpty else { return }
        if shownSet == Self.recentSet {
            shownSet = currentBrush.map { Int($0.set) } ?? 0
        }
        for read in brushesRead {
            if let set = session.addBrushesRead(read: read, shown: UInt32(shownSet)) {
                shownSet = Int(set)
            }
        }
        brushesRead = []
        libraryChanged()
        showNotice("Brushes added to the library.")
    }

    /// Draws `b`'s sample stroke off the main thread (half its own canvas: the cell shows it across
    /// its width), and whether it runs under the brush's name. None comes until the canvas has its
    /// GPU, so one that doesn't is asked for again a little later, a few times.
    func loadPreview(_ b: BrushInfo, tries: Int = 8) {
        let key = Self.brushKey(b)
        guard previews[key] == nil, !previewsAsked.contains(key) else { return }
        previewsAsked.insert(key)
        let session = session
        let name = b.name
        let made = previewsMade
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let t = session.brushPreview(set: b.set, index: b.index, width: 530, height: 162),
                  let image = Self.image(t) else {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                    guard self?.previewsMade == made else { return }
                    self?.previewsAsked.remove(key)
                    if tries > 1 {
                        self?.loadPreview(b, tries: tries - 1)
                    }
                }
                return
            }
            // the name's box (19.5 in, 14 to 26 down, about 7.7 a letter wide, in a cell the
            // picture fills 80 high) covered more than 2/255 on average
            let k: Float = 162 / 80
            let x0 = Int((19.5 + 6.85) * k)
            let x1 = min(Int((19.5 + 6.85 + 7.7 * Float(name.count)) * k), Int(t.width))
            let y0 = Int(14 * k)
            let y1 = Int(26 * k)
            var cover = 0
            if x1 > x0 {
                for y in y0 ..< y1 {
                    for x in x0 ..< x1 {
                        cover += Int(t.rgba[(y * Int(t.width) + x) * 4 + 3])
                    }
                }
            }
            let under = x1 > x0 && cover > 2 * (x1 - x0) * (y1 - y0)
            DispatchQueue.main.async {
                guard self?.previewsMade == made else { return }
                self?.previews[key] = image
                self?.shadedNames[key] = under
            }
        }
    }

    /// A picture the core made (premultiplied RGBA rows).
    static func image(_ t: ThumbnailImage) -> UIImage? {
        let width = Int(t.width)
        let height = Int(t.height)
        guard width > 0, height > 0, t.rgba.count >= width * height * 4,
              let provider = CGDataProvider(data: Data(t.rgba) as CFData),
              let image = CGImage(
                  width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
                  space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                  provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent
              ) else { return nil }
        return UIImage(cgImage: image)
    }

    /// A picture the core made with nothing see-through in it.
    static func opaqueImage(_ t: ThumbnailImage) -> UIImage? {
        let width = Int(t.width)
        let height = Int(t.height)
        guard width > 0, height > 0, t.rgba.count >= width * height * 4,
              let provider = CGDataProvider(data: Data(t.rgba) as CFData),
              let image = CGImage(
                  width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
                  space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                  provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent
              ) else { return nil }
        return UIImage(cgImage: image)
    }

    func toggle(_ p: Panel) {
        // with a brush swiped open, its button only closes the swipe
        if p == .brushes, panel == .brushes, swipedBrush != nil {
            swipedBrush = nil
            return
        }
        _ = switchModes(.panel(colour: p == .color))
        panel = panel == p ? .none : p
        if panel == .layers {
            refreshThumbnails()
        }
    }

    func closePanels() {
        panel = .none
        swipedBrush = nil
    }

    /// Something is about to show over the canvas: all that hangs over it goes first.
    func closePopovers() {
        closePanels()
        sliderPanel = nil
        sliderPanelKept = false
        layerChoice = nil
        copyPasteOpen = false
    }

    /// A tap outside the open panel closes it; with a brush swiped open, only the swipe.
    func tapOutsidePanel() {
        if panel == .brushes, swipedBrush != nil {
            swipedBrush = nil
        } else {
            closePanels()
        }
    }

    /// Whether the side bar shows: a mode taking the screen slides it off its edge.
    var sideBarShown: Bool {
        showSideBar && !transforming && !selecting && crop.shown == nil && opacityLayer == nil && !guide.editing && text.info == nil
            && (!adjusting || adjustShown?.pencil == true)
    }

    /// The modes open now, as the core's `modeSwitch` takes them.
    private func openModes() -> Modes {
        Modes(
            quickshape: quickShape != nil,
            filling: drop.band == .filling,
            recolor: recoloring,
            layerOpacity: opacityLayer != nil,
            transform: transforming,
            selection: selecting,
            adjustment: adjusting
        )
    }

    /// `button` pressed: the modes it leaves behind end first (keeping what they made), as the
    /// core says; true if it goes on to do what it does.
    private func switchModes(_ button: ModeButton) -> Bool {
        let change = modeSwitch(button: button, open: openModes())
        if change.end.quickshape { endQuickShape() }
        if change.end.filling { drop.endBand() }
        if change.end.recolor { endRecolor() }
        if change.end.layerOpacity { endLayerOpacity() }
        if change.end.transform { endTransform(commit: true) }
        if change.end.selection { closeSelecting() }
        if change.end.adjustment { endAdjustment(commit: true) }
        return change.goesOn
    }

    // MARK: Recolor and Layer Opacity

    /// Recolor on, the reticle in the middle of the screen, or off (keeping what it filled).
    func toggleRecolor() {
        if recoloring {
            _ = switchModes(.recolor)
            return
        }
        guard currentTakesChanges() else { return }
        // text takes no colour until it is rasterized
        if layers.first(where: { $0.current })?.text == true {
            askRasterize = true
            return
        }
        _ = switchModes(.recolor)
        closePanels()
        recoloring = true
        // the reticle is placed through the view
        refreshView()
        recolor.start(at: screenCentre)
    }

    /// Recolor ends, keeping what it filled.
    func endRecolor() {
        guard recoloring else { return }
        recolor.end()
        recoloring = false
        refresh()
    }

    /// A drag across the canvas changes the current layer's opacity, read in the band under the top
    /// bar; the shared touches drag it, each drag one undo step.
    func startLayerOpacity() {
        guard let l = layers.first(where: { $0.current }), l.kind == .paint || l.kind == .group else { return }
        _ = switchModes(.layerOpacity)
        closePanels()
        opacityLayer = l
        opacityValue = l.opacity
        session.setTouchLayerOpacity(id: l.id)
    }

    /// The shared touches dragged the opacity: shown as it is; let go, the layers read again.
    func layerOpacityTouched(ended: Bool) {
        guard opacityLayer != nil else { return }
        if let value = session.touchLayerOpacity() {
            opacityValue = value
        }
        if ended {
            refresh()
        }
    }

    func endLayerOpacity() {
        guard opacityLayer != nil else { return }
        session.setTouchLayerOpacity(id: nil)
        opacityLayer = nil
    }

    // MARK: transform and selection

    /// The transform button: opens transform on the current layer, or keeps it and closes.
    func toggleTransform() {
        if transforming {
            _ = switchModes(.transform)
            return
        }
        guard currentTakesChanges() else { return }
        _ = switchModes(.transform)
        panel = .none
        pickNext = false
        guard transform.begin() else { return }
        refreshView()
        transforming = true
    }

    /// Ends transform: `commit` keeps the change as one undo step, otherwise the layer goes back.
    func endTransform(commit: Bool) {
        guard transforming else { return }
        session.transformEnd(commit: commit)
        transforming = false
        transform.ended()
        refresh()
    }

    /// The selection button: opens or closes the selection tool (a selection made stays).
    func toggleSelecting() {
        // open, it closes; else the other modes end
        guard switchModes(.selection) else { return }
        panel = .none
        pickNext = false
        selection.opened()
        selecting = true
        refreshView()
        selection.showLook()
    }

    func closeSelecting() {
        guard selecting else { return }
        selection.closed()
        selecting = false
        selection.showLook()
    }

    /// Window pixels to canvas pixels through the view, or none when it cannot be inverted.
    func windowToCanvas(_ p: CGPoint) -> CGPoint? {
        guard viewMatrix.count == 6 else { return nil }
        let m = viewMatrix.map { CGFloat($0) }
        let det = m[0] * m[3] - m[1] * m[2]
        guard abs(det) >= 1e-9 else { return nil }
        let x = p.x - m[4]
        let y = p.y - m[5]
        return CGPoint(x: (m[3] * x - m[2] * y) / det, y: (m[0] * y - m[1] * x) / det)
    }

    /// Canvas pixels to window points through the view.
    func canvasToWindow(_ p: CGPoint) -> CGPoint {
        guard viewMatrix.count == 6 else { return p }
        let m = viewMatrix.map { CGFloat($0) }
        return CGPoint(
            x: (m[0] * p.x + m[2] * p.y + m[4]) / pixels,
            y: (m[1] * p.x + m[3] * p.y + m[5]) / pixels
        )
    }

    // MARK: the adjustments

    /// The adjustment's values as the core has them now.
    func readAdjust() {
        let shown = session.adjustmentShown()
        adjustShown = shown
        adjustment = shown.adjustment
    }

    /// Opens adjustment `a` on the current layer (keeping one already open).
    func startAdjustment(_ a: Adjustment) {
        guard currentTakesChanges() else { return }
        _ = switchModes(.adjustment)
        panel = .none
        // Gradient Map maps by the library
        if a == .gradientMap {
            _ = gradientMap
        }
        guard session.adjustmentStart(adjustment: a) else { return }
        adjustModesOpen = false
        liquifyPopup = 0
        readAdjust()
        // cloning paints with the brush: its sliders show
        if a == .clone {
            readSliders()
        }
    }

    /// Opens the plugin's filter on the current layer. The core saves the artwork ahead of the
    /// session's first one and compiles it, which isn't for the main thread.
    func startPluginFilter(_ filter: PluginInfo) {
        guard currentTakesChanges() else { return }
        _ = switchModes(.adjustment)
        panel = .none
        let session = session
        let dir = dir
        let id = filter.id
        let plugins = PluginsModel.shared.store
        Task { @MainActor in
            let why = await Task.detached(priority: .userInitiated) {
                session.pluginFilterStart(plugins: plugins, id: id, savedIn: dir)
            }.value
            if let why {
                showNotice(why, warning: true)
            }
            adjustModesOpen = false
            liquifyPopup = 0
            readAdjust()
            refresh()
            // stopped already: no panel comes up to look for that
            if adjustment == nil, let failure = session.takeFailure() {
                showNotice(failure, warning: true)
            }
        }
    }

    /// Ends the adjustment: `commit` keeps it as one undo step, otherwise the layer goes back.
    func endAdjustment(commit: Bool) {
        guard adjustment != nil else { return }
        // a gradient being edited is kept as it is
        if adjustment == .gradientMap {
            gradientMap.endEdit()
        }
        adjustMenuAt = nil
        adjustModesOpen = false
        liquifyPopup = 0
        let painted = adjustShown?.pencil == true
        _ = session.adjustmentEnd(commit: commit)
        if commit {
            showNotice("Adjustment applied")
        }
        readAdjust()
        adjustment = nil
        if painted {
            readSliders()
        }
        refresh()
    }

    /// Keeps what the adjustment did as one undo step and starts it again from there.
    func applyAdjustment() {
        guard adjustment != nil else { return }
        session.adjustmentApply()
        showNotice("Adjustment applied")
        readAdjust()
        refresh()
    }

    /// Takes back the adjustment's changes, or with none, the last one kept.
    func undoAdjustment() {
        guard adjustment != nil else { return }
        session.adjustmentUndo()
        readAdjust()
        refresh()
    }

    func togglePreview() {
        session.adjustmentPreview(original: adjustShown?.preview != true)
        readAdjust()
    }

    /// Every value back to where the adjustment started.
    func resetAdjustment() {
        guard adjustment != nil else { return }
        let painted = adjustShown?.pencil == true
        session.adjustmentReset()
        readAdjust()
        if painted, adjustShown?.pencil != true {
            readSliders()
        }
        refresh()
    }

    /// Liquify's slider `index` (size, pressure, distortion, momentum), 0..1.
    func liquifyValue(_ index: Int) -> Float {
        let values = adjustShown?.liquify ?? []
        return values.indices.contains(index) ? values[index] : 0
    }

    func chooseLiquifyKind(_ kind: LiquifyKind) {
        session.adjustmentLiquifyKind(kind: kind)
        liquifyPopup = 0
        readAdjust()
    }

    func setLiquifyValue(_ index: Int, _ value: Float) {
        session.adjustmentLiquifyValue(index: UInt32(min(max(index, 0), 3)), value: value)
        readAdjust()
    }

    func changeLiquifyAmount(_ amount: Float) {
        session.adjustmentLiquifyAmount(amount: amount)
        readAdjust()
    }

    func resetLiquify() {
        session.adjustmentLiquifyReset()
        liquifyPopup = 0
        readAdjust()
    }

    // MARK: the side bar

    func changeSize(_ value: Float) {
        size = value
        session.setBrushSize(slider: value)
    }

    func changeOpacity(_ value: Float) {
        opacity = value
        session.setBrushOpacity(slider: value)
    }

    /// The notch `value` is on, of the size or opacity slider's (`size`), if any.
    func notchAt(size: Bool, value: Float) -> Float? {
        session.sliderNotchAt(size: size, value: value)
    }

    /// Whether the slider's panel offers to save its value: not when it is on a notch or the
    /// slider is full.
    func canSaveNotch(size: Bool) -> Bool {
        session.canSaveSliderNotch(size: size)
    }

    /// The slider panel's button: saves the value as a notch, or takes away the notch it is on.
    func toggleNotch(size: Bool) {
        session.toggleSliderNotch(size: size)
        readNotches()
        keepBrushLibrary()
    }

    /// The brush's diameter on the screen now, in pixels.
    func brushScreenDiameter() -> Float {
        let info = session.info()
        return info.brushDiameter * info.zoom
    }

    /// The brush's own shape as loops across its diameter (none: the round tip's circle).
    func brushOutline() -> [[Float]] {
        session.brushOutline()
    }

    /// The canvas's touches changed the brush's size or opacity: the sliders show it.
    func brushTouched() {
        readSliders()
    }

    func readSliders() {
        let info = session.info()
        size = info.size
        opacity = info.opacity
        readNotches()
    }

    private func readNotches() {
        sizeNotches = session.sliderNotches(size: true)
        opacityNotches = session.sliderNotches(size: false)
    }

    /// Keeps the library as it is now, with the brushes in use, in its folder, off the main
    /// thread.
    private func keepBrushLibrary() {
        let session = session
        let dir = Self.brushesDir
        Self.libraryWrites.async { [weak self] in
            let kept = session.keepBrushes(dir: dir)
            DispatchQueue.main.async { self?.libraryKept(kept) }
        }
    }

    /// □ pressed: held, it changes what a canvas touch does.
    func modifyPressed() {
        modifyHeld = true
        modifyUsed = false
        modifyDownAt = Date()
        _ = session.setModifyHeld(held: true)
    }

    /// □ let go: a short press nothing used is a tap (`cancelled` when the finger slid off).
    func modifyReleased(cancelled: Bool) {
        modifyHeld = false
        // a touch the canvas's touches took meanwhile used it too
        if session.setModifyHeld(held: false) {
            modifyUsed = true
        }
        if !cancelled, !modifyUsed, Date().timeIntervalSince(modifyDownAt) < Self.modifyTapSeconds {
            // Tap □ as it is set at first: the eyedropper
            pickNext.toggle()
        }
    }

    // MARK: undo and redo

    func undo() {
        // a finished text's step goes back to editing that text first, as a step of its own
        if text.info == nil, !transforming, let id = session.textUndoLayer(), text.resume(id) {
            return
        }
        // editing text, a change to its box's width goes back a step at a time, still editing
        if text.info != nil, text.stepBox(undo: true) {
            return
        }
        // undoing while editing text otherwise undoes the whole edit
        if text.info != nil {
            text.drop()
        }
        if transforming {
            transform.apply(.undo)
            return
        }
        let could = canUndo
        // recoloring, the core takes the fill under the reticle back first (after the fills queued)
        let name = recoloring ? recolor.afterFills { session.undo() } : session.undo()
        if recoloring {
            recolor.readPoint()
        }
        if could || name != nil {
            showNotice("Undo" + (name.map { " \($0)" } ?? ""))
        }
        // cloning, the core put the source back where it was before the stroke
        if adjustment != nil {
            readAdjust()
        }
        refresh()
    }

    func redo() {
        if transforming {
            return
        }
        if text.info != nil, text.stepBox(undo: false) {
            return
        }
        // text being edited is kept first
        let editing = text.info != nil
        text.drop()
        // a text edit redone is edited again
        let textLayer = editing ? nil : session.textRedoLayer()
        defer {
            if let textLayer {
                _ = text.resume(textLayer)
            }
        }
        let could = canRedo
        // recoloring, the core steps the open fill aside and makes it again over what comes back
        let name = recoloring ? recolor.afterFills { session.redo() } : session.redo()
        if could {
            showNotice("Redo" + (name.map { " \($0)" } ?? ""))
        }
        if adjustment != nil {
            readAdjust()
        }
        refresh()
    }

    /// Reads again what the core may have changed (a stroke, an undo).
    func refresh() {
        reference.refresh()
        refreshFlags()
        // undo and redo step through a snapped shape: its handles where they now are, and past its
        // drags it gives way to the stroke as drawn and ends
        if quickShape != nil {
            quickShape = session.quickshapeInfo()
            if quickShape == nil {
                quickShapeEditing = false
            }
        }
        layers = session.layers()
        followMaskColour()
        background = session.background()
        if panel == .layers {
            refreshThumbnails()
        }
    }

    // MARK: Actions

    func cutLayer() {
        guard currentTakesChanges() else {
            closePanels()
            return
        }
        let cut = session.cutLayer()
        if let cut {
            clipboard = cut
            canPaste = true
        }
        closePanels()
        refresh()
        if cut != nil {
            showNotice("Layer cut")
        }
    }

    func copyLayer() {
        let copied = session.copyLayer()
        if let copied {
            clipboard = copied
            canPaste = true
        }
        closePanels()
        if copied != nil {
            showNotice("Layer copied")
        }
    }

    func copyLayerContents(_ l: LayerInfo) {
        if let copied = session.copyLayerContents(id: l.id) {
            clipboard = copied
            canPaste = true
        }
    }

    func copyCanvas() {
        closePanels()
        let session = session
        // off the main thread: the picture waits for the canvas to finish any remaking first
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let copied = session.copyCanvas()
            DispatchQueue.main.async {
                guard let self, let copied else { return }
                self.clipboard = copied
                self.canPaste = true
                self.showNotice("Canvas copied")
            }
        }
    }

    /// What was cut or copied, on a new layer; a private one is left out of the gallery's thumbnail.
    func paste(isPrivate: Bool = false) {
        guard let image = clipboard else { return }
        if (isPrivate ? session.pastePrivate(image: image) : session.paste(image: image)) == nil {
            noRoomForLayers()
        }
        closePanels()
        refresh()
    }

    /// Cut & Paste: what is selected (or the layer) cut out and put back as a new layer.
    func cutAndPaste() {
        guard currentTakesChanges() else {
            closePanels()
            return
        }
        cutLayer()
        paste()
    }

    /// Duplicate: the selection copied to a new layer, or without one the current layer.
    func duplicateCurrent() {
        if session.hasSelection() {
            copyLayer()
            paste()
        } else if let l = layers.first(where: { $0.current }) {
            duplicate(l)
        }
    }

    func flipCanvas(horizontal: Bool) {
        session.flipCanvas(horizontal: horizontal)
        refresh()
    }

    func setGuideOn(_ on: Bool) {
        guide.setOn(on)
    }

    // MARK: the layers

    /// The side the layers' thumbnails are read at, in pixels.
    static let thumbSide: UInt32 = 192

    /// Reads the background alone: a picker moving changes nothing else.
    func readBackground() {
        background = session.background()
    }

    /// The palettes as the store has them now.
    func setPalettes(_ list: [PaletteInfo], defaultPalette: Int) {
        palettes = list
        self.defaultPalette = defaultPalette
    }

    func addLayer() {
        if session.addLayer() == 0 {
            noRoomForLayers()
        }
        refresh()
    }

    /// Says how many layers this canvas holds, there being no room for another.
    /// Says the canvas holds no more layers, and how many it holds.
    func noRoomSaid() -> String {
        let info = session.info()
        return "This canvas holds at most \(layersFor(width: max(info.width, 1), height: max(info.height, 1))) layers."
    }

    func noRoomForLayers() {
        let info = session.info()
        showNotice("This canvas holds at most \(layersFor(width: max(info.width, 1), height: max(info.height, 1))) layers.", warning: true)
    }

    func selectLayer(_ id: UInt64) {
        _ = session.selectLayer(id: id)
        refresh()
    }

    /// Shows or hides `l`, saying so in the notice band as measured ("Hide layer").
    func toggleVisible(_ l: LayerInfo) {
        if session.setLayerVisible(id: l.id, visible: !l.visible) {
            showNotice(l.visible ? "Hide layer" : "Show layer")
        }
        refresh()
    }

    func toggleBackground() {
        _ = session.setBackgroundVisible(visible: !background.visible)
        refresh()
    }

    /// Opens or closes a group's rows.
    func toggleCollapsed(_ l: LayerInfo) {
        _ = session.setGroupCollapsed(id: l.id, collapsed: !l.collapsed)
        refresh()
    }

    /// Whether a closed group hides `l`'s row.
    func hidden(_ l: LayerInfo) -> Bool {
        let byId = Dictionary(layers.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        // from a mask, its layer's place
        var at = l.kind == .mask ? byId[l.parent ?? 0]?.parent : l.parent
        for _ in 0 ..< layers.count {
            guard let id = at, let group = byId[id] else { return false }
            if group.kind == .group, group.collapsed {
                return true
            }
            at = group.parent
        }
        return false
    }

    /// Whether `l` has a mask (the row under it is its mask).
    func hasMask(_ l: LayerInfo) -> Bool {
        layers.contains { $0.kind == .mask && $0.parent == l.id }
    }

    /// Reads again the thumbnails of the layers whose pixels changed since theirs was read (or have
    /// none), off the main thread.
    func refreshThumbnails() {
        let session = session
        let known = thumbnailVersions
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            var read: [(UInt64, UInt64, UIImage)] = []
            for v in session.layerVersions() where known[v.id] != v.version {
                if let t = session.layerThumbnail(id: v.id, maxSide: Self.thumbSide), let image = Self.image(t) {
                    read.append((v.id, v.version, image))
                }
            }
            guard !read.isEmpty else { return }
            DispatchQueue.main.async {
                for (id, version, image) in read {
                    self?.thumbnails[id] = image
                    self?.thumbnailVersions[id] = version
                }
            }
        }
    }

    private func refreshFlags() {
        let info = session.info()
        canUndo = info.canUndo
        canRedo = info.canRedo
    }

    // MARK: the colour

    /// The colour as 0xRRGGBB.
    func colorRgb() -> UInt32 {
        Self.rgb(hue: hue, saturation: saturation, brightness: brightness)
    }

    func setColor(hue h: Float, saturation s: Float, brightness v: Float) {
        hue = min(max(h, 0), 1)
        saturation = min(max(s, 0), 1)
        brightness = min(max(v, 0), 1)
        applyColor()
    }

    func setRgb(_ rgb: UInt32) {
        let channel = { (shift: UInt32) in Float((rgb >> shift) & 0xFF) / 255 }
        let (h, s, v) = Self.hsv(channel(16), channel(8), channel(0))
        // a grey keeps the hue it had, so the hue slider does not jump
        hue = s > 0 ? h : hue
        saturation = s
        brightness = v
        applyColor()
    }

    /// The current and second colours trade places.
    func swapColors() {
        let current = colorRgb()
        setRgb(secondaryRgb)
        secondaryRgb = current
        pushSecondary()
    }

    private func pushSecondary() {
        let c = secondaryRgb
        let channel = { (shift: UInt32) in Float((c >> shift) & 0xFF) / 255 }
        session.setSecondaryColor(r: channel(16), g: channel(8), b: channel(0))
    }

    private func applyColor() {
        let c = colorRgb()
        let channel = { (shift: UInt32) in Float((c >> shift) & 0xFF) / 255 }
        session.setColor(r: channel(16), g: channel(8), b: channel(0))
        // recoloring, the area under the reticle takes the new colour
        if recoloring {
            recolor.changeColour(r: channel(16), g: channel(8), b: channel(0))
        }
        keepColour()
    }

    /// Keeps the brush colour and the masks' for the next session.
    private func keepColour() {
        let now = [hue, saturation, brightness]
        AppSettings.shared.brushColor = onMask ? otherColour : now
        AppSettings.shared.maskColor = onMask ? now : otherColour
    }

    /// A mask became current, or stopped being: the colour trades places with the other kind's, as
    /// masks paint with a colour of their own.
    private func followMaskColour() {
        let mask = layers.first { $0.current }?.kind == .mask
        guard mask != onMask else { return }
        onMask = mask
        let now = [hue, saturation, brightness]
        let next = otherColour
        otherColour = now
        setColor(hue: next[0], saturation: next[1], brightness: next[2])
    }

    /// The colour button held: back to the colour painted with before this one, if there was one.
    func previousColor() {
        guard let before = Self.colourHistory.previous(current: colorRgb()) else { return }
        setRgb(before)
        showNotice("Previous colour")
    }

    func clearHistory() {
        Self.colourHistory.clear()
        history = []
    }

    /// Hue (0..1), saturation and brightness to 0xRRGGBB, each channel rounded.
    static func rgb(hue: Float, saturation s: Float, brightness v: Float) -> UInt32 {
        let h = (hue * 6).truncatingRemainder(dividingBy: 6)
        let sector = Int(h)
        let f = h - Float(sector)
        let p = v * (1 - s)
        let q = v * (1 - s * f)
        let t = v * (1 - s * (1 - f))
        let (r, g, b): (Float, Float, Float)
        switch sector {
        case 0: (r, g, b) = (v, t, p)
        case 1: (r, g, b) = (q, v, p)
        case 2: (r, g, b) = (p, v, t)
        case 3: (r, g, b) = (p, q, v)
        case 4: (r, g, b) = (t, p, v)
        default: (r, g, b) = (v, p, q)
        }
        let byte = { (x: Float) in UInt32(min(max((x * 255).rounded(), 0), 255)) }
        return byte(r) << 16 | byte(g) << 8 | byte(b)
    }

    /// Red, green and blue (0..1) to hue (0..1), saturation and brightness.
    static func hsv(_ r: Float, _ g: Float, _ b: Float) -> (Float, Float, Float) {
        let most = max(r, g, b)
        let least = min(r, g, b)
        let range = most - least
        var h: Float = 0
        if range > 0 {
            if most == r {
                h = ((g - b) / range).truncatingRemainder(dividingBy: 6)
            } else if most == g {
                h = (b - r) / range + 2
            } else {
                h = (r - g) / range + 4
            }
            h /= 6
            if h < 0 {
                h += 1
            }
        }
        return (h, most > 0 ? range / most : 0, most)
    }

    // MARK: the canvas's touches

    /// A touch is coming down on an idle canvas: the touches take the settings as they are now.
    func prepareTouches() {
        session.setTouchSettings(settings: Gestures.shared.touchSettings())
        // Continue filling: each touch fills where it lands; slid, it changes the threshold
        session.setTouchFilling(fill: drop.touchFill())
        // the selection tool's touch selects as the tool is set
        session.setTouchSelecting(how: selection.touch())
        if selecting {
            selection.setPopup(.none)
        }
        // a transform's touch takes the node it lands on (the warp's inner points with the advanced mesh)
        session.setTouchTransforming(transforming: transforming ? TransformTouch(advancedMesh: transform.advancedMesh) : nil)
        // a touch on the canvas closes an adjustment's menu and liquify's popups
        closedAdjustMenu = adjustMenuAt != nil
        if adjustment != nil {
            adjustMenuAt = nil
            adjustModesOpen = false
            liquifyPopup = 0
        }
    }

    /// A stroke finished: its layer's thumbnail and the undo state changed.
    func strokeEnded() {
        // a snapped shape the Pencil moved: its handles where it now is
        if quickShape != nil {
            quickShape = session.quickshapeInfo() ?? quickShape
        }
        // a stroke on a locked or hidden layer drew nothing: say why (text layers ask to rasterize)
        if !adjusting, layers.first(where: { $0.current })?.text != true {
            _ = currentTakesChanges()
        }
        refreshFlags()
        // the stroke's colour went into the history
        history = Self.colourHistory.colours()
        if panel == .layers {
            refreshThumbnails()
        }
    }

    /// Fits the canvas to the window again; what is over it follows the view as it settles.
    func settleToFit() {
        session.settleToFit()
        followView()
    }

    /// One canvas pixel to a screen pixel; what is over it follows the view as it settles.
    func settleToActualSize() {
        session.settleToActualSize()
        followView()
    }

    func followView() {
        settling?.invalidate()
        let session = session
        let until = Date().addingTimeInterval(Double(session.viewSettleSeconds()) + 0.05)
        settling = Timer.scheduledTimer(withTimeInterval: 1.0 / 60, repeats: true) { [weak self] timer in
            self?.refreshView()
            if Date() >= until || !session.viewMoving() {
                timer.invalidate()
                self?.refreshView()
            }
        }
    }

    /// The view moved: what is drawn over the canvas follows.
    func refreshView() {
        viewMatrix = session.viewMatrix()
        // the guide's lines follow the canvas
        if guide.info.on || guide.editing {
            guide.read()
        }
        // the perspective and clone pucks follow the canvas
        if adjustment == .perspectiveBlur || adjustment == .clone {
            readAdjust()
        }
        // the shape's handles follow the canvas
        if quickShape != nil {
            quickShape = session.quickshapeInfo()
        }
    }

    /// The interface hidden or shown again (not while transforming).
    func toggleBars() {
        guard switchModes(.bars) else { return }
        barsHidden.toggle()
        if barsHidden {
            panel = .none
        }
    }

    func clearCurrentLayer() {
        refresh()
        guard let l = layers.first(where: { $0.current }), currentTakesChanges() else { return }
        _ = session.selectLayer(id: l.id)
        session.clearLayer()
        refresh()
        showNotice("Layer cleared")
    }

    /// Layer Select held at window point (`x`, `y`): the layer drawn there becomes current; more
    /// than one there, a list offers the rest.
    func layerSelect(x: Float, y: Float) {
        let ids = session.layersAt(x: x, y: y)
        guard let first = ids.first else {
            showNotice("Nothing is drawn there.")
            return
        }
        selectLayer(first)
        let at = CGPoint(x: CGFloat(x) / pixels, y: CGFloat(y) / pixels)
        layerChoice = ids.count > 1 ? LayerChoiceAt(at: at, ids: ids) : nil
    }

    /// Whether the current layer takes a change (a stroke, a transform, an adjustment, clearing,
    /// cutting); if not (locked, hidden, or an empty group), a dialog says why. What was tried is
    /// not done afterwards.
    func currentTakesChanges() -> Bool {
        guard let l = layers.first(where: { $0.current }), let why = session.whyBlocked() else { return true }
        blocked = BlockedLayer(why: why, layer: l)
        return false
    }

    /// The dialog's answer: unlock or show the layer (`fix`), open Layers (`openLayers`), or neither.
    func answerBlocked(fix: Bool, openLayers: Bool) {
        guard let b = blocked else { return }
        blocked = nil
        if fix {
            switch b.why {
            case .locked:
                toggleLocked(b.layer)
            case .hidden:
                toggleVisible(b.layer)
            case .emptyGroup:
                break
            }
        }
        if openLayers {
            panel = .layers
            refreshThumbnails()
        }
    }

    func toggleLocked(_ l: LayerInfo) {
        let on = !l.locked
        let said = session.lockSaid(id: l.id, on: on)
        if session.setLayerLocked(id: l.id, on: on) {
            showNotice(said)
        }
        refresh()
    }

    func rasterizeCurrent() {
        askRasterize = false
        if let l = layers.first(where: { $0.current }) {
            _ = session.rasterizeLayer(id: l.id)
            refresh()
        }
    }

    // MARK: the eyedropper

    /// How wide a window of the screen the eyedropper reads, in pixels (odd, so a pixel is in the
    /// middle).
    var pickSide: UInt32 {
        UInt32(54 * pixels) | 1
    }

    /// Moves the eyedropper to window point (`x`, `y`) and reads what is shown there, off the main
    /// thread; points that arrive meanwhile replace each other.
    func pick(x: Float, y: Float) {
        pendingPick = (x, y)
        if picking == nil {
            picking = PickState(x: x, y: y, image: nil, rgb: nil)
        }
        if !pickRunning {
            pickRunning = true
            readPick()
        }
    }

    private func readPick() {
        guard let (x, y) = pendingPick else {
            pickRunning = false
            if pickEnding {
                endPick()
            }
            return
        }
        pendingPick = nil
        let session = session
        let side = pickSide
        DispatchQueue.global(qos: .userInteractive).async { [weak self] in
            let seen = session.pick(x: x, y: y, side: side)
            let image = seen.flatMap { Self.opaqueImage(ThumbnailImage(width: $0.side, height: $0.side, rgba: $0.pixels)) }
            DispatchQueue.main.async {
                guard let self else { return }
                if let now = self.picking {
                    self.picking = PickState(x: x, y: y, image: image ?? now.image, rgb: seen?.rgb ?? now.rgb)
                }
                self.readPick()
            }
        }
    }

    /// The eyedropper over the reference window's picture, the finger at `at` on the screen: what
    /// it sees there, already read.
    func pickFromReference(_ image: UIImage?, rgb: UInt32?, at: CGPoint) {
        picking = PickState(x: Float(at.x * pixels), y: Float(at.y * pixels), image: image, rgb: rgb, fromReference: true)
    }

    /// The eyedropper lets go: its colour becomes the current one (a quick tap ends before the
    /// colour under it has been read: it is taken once it has).
    func finishPick() {
        pickNext = false
        if pickRunning {
            pickEnding = true
        } else {
            endPick()
        }
    }

    private func endPick() {
        pickEnding = false
        pendingPick = nil
        if let rgb = picking?.rgb {
            setRgb(rgb)
        }
        picking = nil
    }

    func cancelPick() {
        pickEnding = false
        pendingPick = nil
        picking = nil
    }

    // MARK: QuickShape

    /// What a stroke snapped to, for the band under the top bar.
    private func snappedTo(_ kind: QuickShapeKind) -> String {
        let shape: String
        switch kind {
        case .line: shape = "a line"
        case .polyline: shape = "a polyline"
        case .arc: shape = "an arc"
        case .ellipse: shape = "an ellipse"
        case .circle: shape = "a circle"
        case .triangle: shape = "a triangle"
        case .equilateralTriangle: shape = "an equilateral triangle"
        case .quadrilateral: shape = "a quadrilateral"
        case .rectangle: shape = "a rectangle"
        case .square: shape = "a square"
        }
        return "Snapped to \(shape)"
    }

    /// The stroke held still snapped to a shape (the canvas's touches did it).
    func quickShapeSnapped() {
        guard let info = session.quickshapeInfo() else { return }
        quickShape = info
        quickShapeEditing = false
        showNotice(snappedTo(info.kind))
    }

    /// The snapped shape changed (dragged, or a finger down while it is held).
    func quickShapeChanged() {
        quickShape = session.quickshapeInfo() ?? quickShape
    }

    /// The snapped shape was let go of as it is (the canvas's touches did it).
    func quickShapeEnded() {
        quickShape = nil
        quickShapeEditing = false
        refreshFlags()
    }

    /// The last stroke snaps to the shape it looks like, if nothing has been done since.
    func quickShapeLast() {
        guard let info = session.quickshapeLast() else { return }
        quickShape = info
        quickShapeEditing = false
        showNotice(snappedTo(info.kind))
        refreshFlags()
    }

    func chooseQuickShape(_ kind: QuickShapeKind) {
        quickShape = session.quickshapeChoose(kind: kind) ?? quickShape
    }

    /// Keeps the snapped shape as drawn.
    func endQuickShape() {
        guard quickShape != nil else { return }
        session.quickshapeDone()
        quickShape = nil
        quickShapeEditing = false
        refreshFlags()
    }

    /// The light interface: the panels and bars, and the surround and its grid.
    func changeLightInterface(_ on: Bool) {
        AppSettings.shared.lightInterface = on
        session.setLightInterface(on: on)
    }

    // MARK: pressure and smoothing

    /// The app's smoothing and pressure, for every stroke.
    func applyInput() {
        let s = AppSettings.shared
        session.setInputSettings(
            stabilization: s.inputStabilization,
            motionFiltering: s.inputMotionFiltering,
            motionExpression: s.inputMotionExpression,
            pressureSmoothing: s.inputPressureSmoothing,
            tipAttached: s.inputTipAttached,
            curve: s.inputCurve
        )
    }

    // MARK: keeping the artwork

    /// Watches for a failed autosave, told once until a save succeeds again, and for what stops away
    /// from any touch: a patch or a smudge the GPU had no memory for, or its device lost. A patch's
    /// card says so too, once its page reads the list again.
    private func watchSaves() {
        let session = session
        saveWatch = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in
            // off the main thread: a big artwork's first frame holds the session a while
            DispatchQueue.global(qos: .utility).async {
                let reason = session.takeSaveFailure()
                let failure = session.takeFailure()
                guard reason != nil || failure != nil else { return }
                DispatchQueue.main.async {
                    if let reason {
                        self?.saveFailed(reason)
                    }
                    if let failure {
                        self?.showNotice(failure, warning: true)
                        PluginsModel.shared.read()
                    }
                }
            }
        }
    }

    /// A save failed, for `reason`: said the first time since one last succeeded.
    func saveFailed(_ reason: String) {
        if !saveFailing {
            saveFailing = true
            showNotice(saveFailureSaid(reason: reason), warning: true)
        }
    }

    /// A save went through: the next failure is told again.
    func saveSucceeded() {
        saveFailing = false
    }

    /// Time on the artwork stops counting and goes on its record.
    func noteTime() {
        if let since = activeSince {
            session.addTrackedTime(ms: UInt64(max(Date().timeIntervalSince(since), 0) * 1000))
        }
        activeSince = nil
    }

    /// The app is back in front: time on the artwork counts again.
    func resumeTracking() {
        if activeSince == nil {
            activeSince = Date()
        }
    }

    /// The artwork is being left: nothing goes on watching it.
    func stopWatching() {
        saveWatch?.invalidate()
        saveWatch = nil
        settling?.invalidate()
        settling = nil
    }

    // MARK: messages

    /// Shows `text` in the band under the top bar, in place of any message there; a `warning`
    /// stays longer.
    func showNotice(_ text: String, warning: Bool = false) {
        notices += 1
        notice = Notice(text: text, id: notices, warning: warning)
    }

    /// The message under the top bar has gone.
    func noticeGone(_ n: Notice) {
        if notice == n {
            notice = nil
        }
    }
}
