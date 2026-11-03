import Foundation
import Observation

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

    init(session: CanvasSession, dir: String) {
        self.session = session
        self.dir = dir
        _ = session.openBrushes(dir: Self.brushesDir)
        let c = session.color()
        if c.count >= 3 {
            let (h, s, v) = Self.hsv(c[0], c[1], c[2])
            hue = h
            saturation = s
            brightness = v
        }
        tool = session.info().tool
        currentBrush = session.currentBrush()
        readSliders()
        refreshFlags()
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

    func toggle(_ p: Panel) {
        _ = switchModes(.panel(colour: p == .color))
        panel = panel == p ? .none : p
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
