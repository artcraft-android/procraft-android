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
    /// The colour panel's mode.
    var colorMode: ColorMode = .disc
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

    /// The Actions panel's tab.
    var actionsTab = 0
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
        let c = session.color()
        if c.count >= 3 {
            let (h, s, v) = Self.hsv(c[0], c[1], c[2])
            hue = h
            saturation = s
            brightness = v
        }
        tool = session.info().tool
        currentBrush = session.currentBrush()
        if let current = currentBrush {
            shownSet = Int(current.set)
        }
        readSliders()
        refresh()
        pushSecondary()
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

    /// Keeps the tools' brushes, the sets' choices and each brush's sliders for the next session.
    private func keepChoices() {
        let session = session
        let dir = Self.brushesDir
        DispatchQueue.global(qos: .utility).async {
            _ = session.keepBrushChoices(dir: dir)
        }
    }

    /// Draws `b`'s sample stroke off the main thread (half its own canvas: the cell shows it across
    /// its width), and whether it runs under the brush's name.
    func loadPreview(_ b: BrushInfo) {
        let key = Self.brushKey(b)
        guard previews[key] == nil, !previewsAsked.contains(key) else { return }
        previewsAsked.insert(key)
        let session = session
        let name = b.name
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let t = session.brushPreview(set: b.set, index: b.index, width: 530, height: 162),
                  let image = Self.image(t) else { return }
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

    /// The modes open now, as the core's `modeSwitch` takes them.
    private func openModes() -> Modes {
        Modes(
            quickshape: false,
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
        layers = session.layers()
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
        hue = h
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
