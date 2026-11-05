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
    private(set) var adjusting = false
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
    private(set) var guideOn = false

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

    init(session: CanvasSession, dir: String) {
        self.session = session
        self.dir = dir
        _ = session.openBrushes(dir: Self.brushesDir)
        brushSets = session.brushLibrary()
        readSetChoices()
        let settings = AppSettings.shared
        session.setSelectionVisibility(visibility: settings.selectionVisibility)
        session.setFillGap(pixels: UInt32(settings.fillGap))
        session.setBrushScaling(onScreen: settings.brushOnScreen)
        session.setLightInterface(on: settings.lightInterface)
        applyInput()
        session.useColourHistory(history: Self.colourHistory)
        history = Self.colourHistory.colours()
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
        DispatchQueue.global(qos: .utility).async {
            _ = session.keepBrushChoices(dir: dir)
        }
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
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let t = session.brushPreview(set: b.set, index: b.index, width: 530, height: 162),
                  let image = Self.image(t) else {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
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
        _ = switchModes(.panel(colour: p == .color))
        panel = panel == p ? .none : p
        if panel == .layers {
            refreshThumbnails()
        }
    }

    func closePanels() {
        panel = .none
    }

    /// Whether the side bar shows: a mode taking the screen slides it off its edge.
    var sideBarShown: Bool {
        !transforming && !selecting && !adjusting
    }

    /// The modes open now, as the core's `modeSwitch` takes them.
    private func openModes() -> Modes {
        Modes(
            quickshape: quickShape != nil,
            filling: false,
            recolor: recoloring,
            layerOpacity: false,
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
        if change.end.recolor { recoloring = false }
        if change.end.transform { transforming = false }
        if change.end.selection { selecting = false }
        if change.end.adjustment { adjusting = false }
        return change.goesOn
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

    private func readSliders() {
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
        DispatchQueue.global(qos: .utility).async {
            _ = session.keepBrushes(dir: dir)
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
        let could = canUndo
        let name = session.undo()
        if could || name != nil {
            showNotice("Undo" + (name.map { " \($0)" } ?? ""))
        }
        refresh()
    }

    func redo() {
        let could = canRedo
        let name = session.redo()
        if could {
            showNotice("Redo" + (name.map { " \($0)" } ?? ""))
        }
        refresh()
    }

    /// Reads again what the core may have changed (a stroke, an undo).
    func refresh() {
        refreshFlags()
        // undo and redo keep a snapped shape as drawn first
        if quickShape != nil, session.quickshapeInfo() == nil {
            quickShape = nil
            quickShapeEditing = false
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
        if let cut = session.cutLayer() {
            clipboard = cut
            canPaste = true
        }
        refresh()
    }

    func copyLayer() {
        if let copied = session.copyLayer() {
            clipboard = copied
            canPaste = true
        }
    }

    func copyCanvas() {
        if let copied = session.copyCanvas() {
            clipboard = copied
            canPaste = true
        }
    }

    func paste() {
        guard let image = clipboard else { return }
        _ = session.paste(image: image)
        refresh()
    }

    func flipCanvas(horizontal: Bool) {
        session.flipCanvas(horizontal: horizontal)
        refresh()
    }

    func setGuideOn(_ on: Bool) {
        guideOn = session.setGuideOn(on: on).on
    }

    // MARK: the layers

    /// The side the layers' thumbnails are read at, in pixels.
    static let thumbSide: UInt32 = 192

    func addLayer() {
        if session.addLayer() == 0 {
            let info = session.info()
            showNotice("This canvas holds at most \(layersFor(width: max(info.width, 1), height: max(info.height, 1))) layers.", warning: true)
        }
        refresh()
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

    private func followView() {
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

    // MARK: pressure and smoothing

    /// The app's smoothing and pressure, for every stroke.
    func applyInput() {
        let s = AppSettings.shared
        session.setInputSettings(
            stabilization: s.inputStabilization,
            motionFiltering: s.inputMotionFiltering,
            motionExpression: s.inputMotionExpression,
            pressureSmoothing: s.inputPressureSmoothing,
            curve: s.inputCurve
        )
    }

    // MARK: keeping the artwork

    /// Watches for a failed autosave: told once, until a save succeeds again.
    private func watchSaves() {
        let session = session
        saveWatch = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in
            guard let reason = session.takeSaveFailure() else { return }
            self?.saveFailed(reason)
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
