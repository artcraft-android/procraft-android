import Foundation
import Observation

/// When the brush cursor shows: while the Pencil hovers, while painting, or both.
enum CursorShows: Int, CaseIterable {
    case hovering, painting, both
}

/// How the brush cursor is drawn: light and dark rings, in the colour painted with, or as the
/// brush says.
enum CursorStyle: Int, CaseIterable {
    case contrast, color, brush
}

/// What the app keeps between runs, in its own settings; what shows them follows a change.
@Observable
final class AppSettings {
    static let shared = AppSettings()

    /// The hue a first run starts on.
    static let defaultHue: Float = 0.59
    /// The shortest and longest rapid undo delay, seconds (the core's).
    static let rapidUndoRange: ClosedRange<Float> = 0.1 ... 1.5
    /// The widest gap ColorDrop keeps out of, canvas pixels.
    static let mostFillGap = 32

    /// The brush studio's open page, whichever brush it opens on.
    var studioTab: Int { didSet { kept.set(studioTab, forKey: Key.studioTab) } }
    /// The drawing pad's 3D preview: the pad a sphere on every page.
    var pad3d: Bool { didSet { kept.set(pad3d, forKey: Key.pad3d) } }
    /// The side bar on the right edge instead of the left.
    var rightHanded: Bool { didSet { kept.set(rightHanded, forKey: Key.rightHanded) } }
    /// How long two fingers are held before undo repeats, seconds.
    var rapidUndoDelay: Float { didSet { kept.set(rapidUndoDelay, forKey: Key.rapidUndoDelay) } }
    /// The brush's outline where the Pencil paints or hovers.
    var brushCursorOn: Bool { didSet { kept.set(brushCursorOn, forKey: Key.brushCursor) } }
    var cursorShows: CursorShows { didSet { kept.set(cursorShows.rawValue, forKey: Key.cursorShows) } }
    var cursorStyle: CursorStyle { didSet { kept.set(cursorStyle.rawValue, forKey: Key.cursorStyle) } }
    /// The palettes show as cards (named tiles) rather than compact rows of swatches.
    var paletteCards: Bool { didSet { kept.set(paletteCards, forKey: Key.paletteCards) } }
    /// The short colour panel shows the colour history under the picker, not the active palette.
    var colorHistoryShown: Bool { didSet { kept.set(colorHistoryShown, forKey: Key.colorHistoryShown) } }
    /// The canvas, without the interface, on a second display when there is one.
    var projectCanvas: Bool { didSet { kept.set(projectCanvas, forKey: Key.projectCanvas) } }
    /// Brush scaling relative to the screen (else to the canvas).
    var brushOnScreen: Bool { didSet { kept.set(brushOnScreen, forKey: Key.brushOnScreen) } }
    /// How strongly what isn't selected is banded, 0..1.
    var selectionVisibility: Float { didSet { kept.set(selectionVisibility, forKey: Key.selectionVisibility) } }
    /// ColorDrop keeps out of gaps up to this many canvas pixels wide in its lines (0: none).
    var fillGap: Int { didSet { kept.set(fillGap, forKey: Key.fillGap) } }
    /// The actions panel's open tab.
    var actionsTab: Int { didSet { kept.set(actionsTab, forKey: Key.actionsTab) } }
    /// The colour panel's mode.
    var colorMode: Int { didSet { kept.set(colorMode, forKey: Key.colorMode) } }

    // pressure and smoothing, for every stroke
    var inputStabilization: Float { didSet { kept.set(inputStabilization, forKey: Key.inputStabilization) } }
    var inputMotionFiltering: Float { didSet { kept.set(inputMotionFiltering, forKey: Key.inputMotion) } }
    var inputMotionExpression: Float { didSet { kept.set(inputMotionExpression, forKey: Key.inputExpression) } }
    var inputPressureSmoothing: Float { didSet { kept.set(inputPressureSmoothing, forKey: Key.inputSmoothing) } }
    /// The pressure curve: x, y pairs, 0..1.
    var inputCurve: [Float] { didSet { kept.set(inputCurve, forKey: Key.inputCurve) } }

    // the brush colour and the masks' (hue, saturation, brightness), as they were last left
    var brushColor: [Float] { didSet { kept.set(brushColor, forKey: Key.brushColor) } }
    var maskColor: [Float] { didSet { kept.set(maskColor, forKey: Key.maskColor) } }

    /// The light interface: light grey panels and bars with dark text.
    var lightInterface: Bool {
        didSet {
            kept.set(lightInterface, forKey: Key.lightInterface)
            Look.shared.light = lightInterface
        }
    }

    @ObservationIgnored private let kept = UserDefaults.standard

    private init() {
        let d = UserDefaults.standard
        func float(_ key: String, _ otherwise: Float) -> Float {
            d.object(forKey: key) == nil ? otherwise : d.float(forKey: key)
        }
        func bool(_ key: String, _ otherwise: Bool) -> Bool {
            d.object(forKey: key) == nil ? otherwise : d.bool(forKey: key)
        }
        func colour(_ key: String, _ otherwise: [Float]) -> [Float] {
            let c = (d.array(forKey: key) as? [NSNumber])?.map(\.floatValue) ?? []
            return c.count == 3 ? c.map { min(max($0, 0), 1) } : otherwise
        }
        studioTab = d.integer(forKey: Key.studioTab)
        pad3d = d.bool(forKey: Key.pad3d)
        rightHanded = d.bool(forKey: Key.rightHanded)
        rapidUndoDelay = float(Key.rapidUndoDelay, 0.65)
        brushCursorOn = bool(Key.brushCursor, true)
        cursorShows = CursorShows(rawValue: d.integer(forKey: Key.cursorShows)) ?? .hovering
        cursorStyle = CursorStyle(rawValue: d.object(forKey: Key.cursorStyle) == nil ? 2 : d.integer(forKey: Key.cursorStyle)) ?? .brush
        paletteCards = d.bool(forKey: Key.paletteCards)
        colorHistoryShown = d.bool(forKey: Key.colorHistoryShown)
        projectCanvas = d.bool(forKey: Key.projectCanvas)
        brushOnScreen = d.bool(forKey: Key.brushOnScreen)
        selectionVisibility = float(Key.selectionVisibility, 0.25)
        fillGap = min(max(d.integer(forKey: Key.fillGap), 0), Self.mostFillGap)
        actionsTab = d.integer(forKey: Key.actionsTab)
        colorMode = d.integer(forKey: Key.colorMode)
        inputStabilization = d.float(forKey: Key.inputStabilization)
        inputMotionFiltering = d.float(forKey: Key.inputMotion)
        inputMotionExpression = d.float(forKey: Key.inputExpression)
        inputPressureSmoothing = d.float(forKey: Key.inputSmoothing)
        let curve = (d.array(forKey: Key.inputCurve) as? [NSNumber])?.map(\.floatValue) ?? []
        inputCurve = curve.count >= 4 ? curve : [0, 0, 1, 1]
        brushColor = colour(Key.brushColor, [Self.defaultHue, 1, 1])
        maskColor = colour(Key.maskColor, [0, 0, 0])
        lightInterface = d.bool(forKey: Key.lightInterface)
        Look.shared.light = lightInterface
    }

    private enum Key {
        static let studioTab = "studioTab"
        static let pad3d = "pad3d"
        static let rightHanded = "rightHanded"
        static let rapidUndoDelay = "rapidUndoDelay"
        static let brushCursor = "brushCursor"
        static let cursorShows = "cursorShows"
        static let cursorStyle = "cursorStyle"
        static let paletteCards = "paletteCards"
        static let colorHistoryShown = "colorHistoryShown"
        static let projectCanvas = "projectCanvas"
        static let brushOnScreen = "brushOnScreen"
        static let selectionVisibility = "selectionVisibility"
        static let fillGap = "fillGap"
        static let actionsTab = "actionsTab"
        static let colorMode = "colorMode"
        static let inputStabilization = "inputStabilization"
        static let inputMotion = "inputMotion"
        static let inputExpression = "inputExpression"
        static let inputSmoothing = "inputSmoothing"
        static let inputCurve = "inputCurve"
        static let brushColor = "brushColor"
        static let maskColor = "maskColor"
        static let lightInterface = "lightInterface"
    }
}

/// Gesture controls: the pages, their switches, what starts on, the delays and the rule that a way
/// of touching does one thing are the core's (gestures.rs); this keeps them in the app's settings.
@Observable
final class Gestures {
    static let shared = Gestures()

    private static let switchesKey = "gestures.switches"
    private static let delaysKey = "gestures.delays"

    @ObservationIgnored private let core: GestureSettings
    /// Read wherever the settings are, so what shows them follows a change.
    private var changes = 0

    private init() {
        let d = UserDefaults.standard
        let switches = (d.dictionary(forKey: Self.switchesKey) ?? [:]).compactMapValues { ($0 as? NSNumber)?.boolValue }
        let delays = (d.dictionary(forKey: Self.delaysKey) ?? [:]).compactMapValues { ($0 as? NSNumber)?.floatValue }
        core = GestureSettings(kept: KeptGestures(switches: switches, delays: delays))
    }

    func isOn(_ purpose: GesturePurpose, _ key: String) -> Bool {
        _ = changes
        return core.isOn(purpose: purpose, key: key)
    }

    /// Turns a switch on or off; on, its trigger goes off everywhere else.
    func set(_ purpose: GesturePurpose, _ key: String, _ value: Bool) {
        _ = core.set(purpose: purpose, key: key, value: value)
        keep()
    }

    /// What `trigger` does, if anything.
    func purposeOf(_ trigger: Trigger) -> GesturePurpose? {
        _ = changes
        return core.purposeOf(trigger: trigger)
    }

    /// A page's delay, seconds.
    func delay(_ purpose: GesturePurpose) -> Float {
        _ = changes
        return core.delay(purpose: purpose)
    }

    func setDelay(_ purpose: GesturePurpose, seconds: Float) {
        _ = core.setDelay(purpose: purpose, seconds: seconds)
        keep()
    }

    /// Every switch and delay as the app ships them.
    func resetAll() {
        core.resetAll()
        keep()
    }

    /// The settings as the canvas's touches take them.
    func touchSettings() -> TouchSettings {
        _ = changes
        return core.touchSettings(rapidUndoMs: Double(AppSettings.shared.rapidUndoDelay) * 1000)
    }

    private func keep() {
        let all = core.kept()
        let d = UserDefaults.standard
        d.set(all.switches, forKey: Self.switchesKey)
        d.set(all.delays, forKey: Self.delaysKey)
        changes += 1
    }
}
