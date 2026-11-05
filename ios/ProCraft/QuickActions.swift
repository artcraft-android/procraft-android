import Foundation
import Observation

/// The QuickMenu's menus, kept for every artwork: each a name and six buttons, clockwise from the
/// top, each set to an action or a brush.
@Observable
final class QuickMenus {
    static let shared = QuickMenus()

    private static let setsKey = "quickMenus"
    private static let activeKey = "quickMenuActive"

    /// Oldest first.
    private(set) var sets: [QuickMenuSet]
    /// The menu kept as the active one.
    private(set) var active: Int
    /// The menu on show: the active one, or one just added until a row is chosen.
    var shown: Int

    private init() {
        let d = UserDefaults.standard
        let sets = quickMenus(saved: d.string(forKey: Self.setsKey), single: nil)
        let active = min(max(d.integer(forKey: Self.activeKey), 0), sets.count - 1)
        self.sets = sets
        self.active = active
        shown = active
    }

    /// The menu on show.
    var menu: QuickMenuSet? {
        sets.indices.contains(shown) ? sets[shown] : nil
    }

    /// Sets button `i` of the menu on show to `action`, and to `brush` when it gives one.
    func setSlot(_ i: Int, _ action: QuickAction, brush: UInt64 = 0) {
        guard let menu, menu.slots.indices.contains(i) else { return }
        var slots = menu.slots
        slots[i] = action
        var brushes = menu.slots.indices.map { $0 < menu.brushes.count ? menu.brushes[$0] : 0 }
        brushes[i] = brush
        sets[shown] = QuickMenuSet(name: menu.name, slots: slots, brushes: brushes)
        keep()
    }

    /// Adds a menu ("QuickMenu N", every button No Action) and shows it; the active one stays until
    /// a menu is chosen. Gives where it went.
    func add() -> Int {
        sets = quickMenuAdded(menus: sets)
        shown = sets.count - 1
        keep()
        return shown
    }

    func choose(_ i: Int) {
        guard sets.indices.contains(i) else { return }
        active = i
        shown = i
        keep()
    }

    /// Takes menu `i` away; the last one stays.
    func delete(_ i: Int) {
        guard sets.count > 1, sets.indices.contains(i) else { return }
        sets.remove(at: i)
        active = Int(quickMenuActiveAfter(active: UInt32(active), removed: UInt32(i)))
        shown = Int(quickMenuActiveAfter(active: UInt32(shown), removed: UInt32(i)))
        keep()
    }

    private func keep() {
        let d = UserDefaults.standard
        d.set(quickMenusSaved(menus: sets), forKey: Self.setsKey)
        d.set(active, forKey: Self.activeKey)
    }
}

extension CanvasModel {
    /// The QuickMenu opened around window point `at` (in points), or closed if it is open.
    func toggleQuickMenu(at: CGPoint) {
        if quickMenuAt == nil {
            QuickMenus.shared.shown = QuickMenus.shared.active
        }
        quickMenuAt = quickMenuAt == nil ? at : nil
    }

    /// The brush button `i` of the ring gives the current tool, if it is set to one still in the
    /// library.
    func quickSlotBrush(_ i: Int) -> BrushInfo? {
        guard let menu = QuickMenus.shared.menu, menu.slots.indices.contains(i),
              menu.slots[i] == .brushes, i < menu.brushes.count, menu.brushes[i] != 0 else { return nil }
        return session.quickBrush(key: menu.brushes[i])
    }

    /// What button `i` of the ring says: the brush's name when it is set to one.
    func quickSlotLabel(_ i: Int) -> String {
        if let brush = quickSlotBrush(i) {
            return brush.name
        }
        guard let menu = QuickMenus.shared.menu, menu.slots.indices.contains(i) else { return "" }
        return quickActionLabel(action: menu.slots[i])
    }

    /// Sets button `i` of the ring to give brush `b` to the current tool.
    func setQuickSlotBrush(_ i: Int, _ b: BrushInfo) {
        let key = session.quickBrushKey(set: b.set, index: b.index)
        guard key != 0 else { return }
        QuickMenus.shared.setSlot(i, .brushes, brush: key)
        // the library keeps the brush's key, following it about
        keepChoicesNow()
    }

    /// Runs button `i` of the ring: its brush given to the current tool, else its action.
    func runQuickSlot(_ i: Int, at: CGPoint) {
        if let brush = quickSlotBrush(i) {
            chooseBrush(brush)
            return
        }
        guard let menu = QuickMenus.shared.menu, menu.slots.indices.contains(i) else { return }
        runQuickAction(menu.slots[i], at: at)
    }

    /// Does `a`, a QuickMenu button opened at window point `at`. What has no screen on the iPad yet
    /// (text, the animation's and the reference's, the adjustments but Liquify) does nothing.
    func runQuickAction(_ a: QuickAction, at: CGPoint) {
        func onCurrent(_ f: (LayerInfo) -> Void) {
            if let l = layers.first(where: { $0.current }) {
                f(l)
            }
        }
        switch a {
        case .newLayer: addLayer()
        case .duplicateLayer: onCurrent(duplicate)
        case .deleteLayer: onCurrent(delete)
        case .mergeDown: onCurrent(mergeDown)
        case .clearLayer: clearCurrentLayer()
        case .fillLayer: onCurrent(fillLayer)
        case .selectLayer: onCurrent(selectLayerContents)
        case .alphaLock: onCurrent(toggleAlphaLock)
        case .layerVisibility: onCurrent(toggleVisible)
        case .solo: onCurrent(toggleSolo)
        case .copy: copyLayer()
        case .copyCanvas: copyCanvas()
        case .cut: cutLayer()
        case .paste: paste()
        case .copyPasteMenu: copyPasteOpen = true
        case .flipHorizontal: flipCanvas(horizontal: true)
        case .flipVertical: flipCanvas(horizontal: false)
        case .undo: undo()
        case .redo: redo()
        case .paint: select(.paint)
        case .smudge: select(.smudge)
        case .erase: select(.erase)
        case .eyedropper: pickNext = true
        case .swapColors: previousColor()
        case .previousBrush: previousBrush()
        case .selection: toggleSelecting()
        case .transform: toggleTransform()
        case .adjustments: toggle(.adjustments)
        case .liquify: startAdjustment(.liquify)
        case .colors: toggle(.color)
        case .layers: toggle(.layers)
        case .brushes: toggle(.brushes)
        case .interface: toggleBars()
        case .fitCanvas: settleToFit()
        case .actualSize: settleToActualSize()
        case .quickshape: quickShapeLast()
        case .quickshapeOn:
            let gestures = Gestures.shared
            gestures.set(.quickshape, "DRAW_HOLD", !gestures.isOn(.quickshape, "DRAW_HOLD"))
        case .layerSelect: layerSelect(x: Float(at.x * pixels), y: Float(at.y * pixels))
        case .guide2d: toggleGuideKind(.grid)
        case .guideIsometric: toggleGuideKind(.isometric)
        case .guidePerspective: toggleGuideKind(.perspective)
        case .guideSymmetry: toggleGuideKind(.symmetry)
        case .drawingAssist: setGuideAssisted(!session.guide().assisted)
        case .brushScaling:
            let settings = AppSettings.shared
            settings.brushOnScreen.toggle()
            session.setBrushScaling(onScreen: settings.brushOnScreen)
        default: break
        }
    }

    /// The drawing guide on as `kind`, or off if it already is that.
    private func toggleGuideKind(_ kind: GuideType) {
        let guide = session.guide()
        if guide.on, guide.kind == kind {
            setGuideOn(false)
            return
        }
        if !guide.on {
            setGuideOn(true)
        }
        _ = session.setGuideKind(kind: kind)
    }
}
