import AudioToolbox
import SwiftUI
import UIKit
import UniformTypeIdentifiers

private let companionWidth: CGFloat = 432
private let companionHeight: CGFloat = 89
private let companionFill = Color(argb: 0xE640_4040)
private let companionInk = Color(argb: 0xFFBE_BEBE)
private let companionGrab = Color(argb: 0xFF8A_8A8A)
/// Where the bar sits at first: its middle across the screen and this far down it.
private let companionStartY: CGFloat = 0.54
/// The latest colour is named at most this often while it keeps changing, seconds.
private let colourNameGap = 0.15
/// The sound a key makes.
private let clickSound: SystemSoundID = 1104

/// What the app keeps in the system's Settings (`ios/Settings.bundle`), read as it comes forward: the
/// app itself never changes them.
enum SystemSettings {
    private static func flag(_ key: String, _ starts: Bool) -> Bool {
        (UserDefaults.standard.object(forKey: key) as? NSNumber)?.boolValue ?? starts
    }

    static var orientationMemory: Bool { flag("orientationMemory", true) }
    static var fitInside: Bool { flag("fitInside", false) }
    static var noTimeLapse: Bool { flag("noTimeLapse", false) }
    static var simplifiedUndos: Bool { flag("simplifiedUndos", true) }
    static var singleTouchCompanion: Bool { flag("singleTouchCompanion", false) }
    static var colorNames: Bool { flag("colorNames", false) }
    static var feedbackSounds: Bool { flag("feedbackSounds", false) }

    static var speedDistance: Float {
        (UserDefaults.standard.object(forKey: "speedDistance") as? NSNumber)?.floatValue ?? 1
    }

    /// What an artwork dragged out of the gallery becomes, and a picture's format when one is wanted.
    static var dragFormat: String {
        UserDefaults.standard.string(forKey: "dragFormat") ?? "ProCraft"
    }

    static var dragImageFormat: String {
        UserDefaults.standard.string(forKey: "dragImageFormat") ?? "JPEG"
    }
}

/// The app's settings on the open artwork, a hardware keyboard, and what comes in by being dropped.
extension CanvasModel {
    /// The system's Settings as they stand now.
    func readAppSettings() {
        colorNames = SystemSettings.colorNames
        feedbackSounds = SystemSettings.feedbackSounds
        singleTouchCompanion = SystemSettings.singleTouchCompanion
        if !singleTouchCompanion {
            companionMode = nil
        }
        session.setSimplifiedUndos(on: SystemSettings.simplifiedUndos)
        session.setSpeedDistance(part: SystemSettings.speedDistance)
        applyFitInsets()
    }

    /// What fitting the canvas keeps clear of: the top bar and the sidebar, with Fit Canvas Inside
    /// Interface.
    func applyFitInsets() {
        guard SystemSettings.fitInside else {
            session.setFitInsets(left: 0, top: 0, right: 0, bottom: 0)
            return
        }
        let right = AppSettings.shared.rightHanded
        let side = showSideBar ? Float((Pc.sideBarWidth + Pc.popoverGap) * pixels) : 0
        session.setFitInsets(left: right ? 0 : side, top: Float(Pc.barHeight * pixels), right: right ? side : 0, bottom: 0)
    }

    /// A new artwork just started: without time-lapse if it is turned off.
    func startedNew() {
        if SystemSettings.noTimeLapse {
            timeLapse.setRecording(false)
        }
    }

    /// A click for an action done, with Feedback Sounds on.
    func feedback() {
        if feedbackSounds {
            AudioServicesPlaySystemSound(clickSound)
        }
    }

    /// The single touch companion's Zoom or Move on, or off again.
    func toggleCompanionMode(_ mode: CompanionMode) {
        feedback()
        companionMode = companionMode == mode ? nil : mode
    }

    /// A hardware keyboard's key as the shared shortcuts name it.
    private static func name(of key: UIKey) -> String? {
        switch key.keyCode {
        case .keyboardSpacebar: return "space"
        case .keyboardDeleteOrBackspace: return "backspace"
        case .keyboardDeleteForward: return "delete"
        case .keyboardLeftArrow: return "left"
        case .keyboardRightArrow: return "right"
        case .keyboardUpArrow: return "up"
        case .keyboardDownArrow: return "down"
        default: break
        }
        let typed = key.charactersIgnoringModifiers.lowercased()
        return typed.count == 1 && "abcdefghijklmnopqrstuvwxyz0123456789[];".contains(typed) ? typed : nil
    }

    /// A key went down; whether it did anything.
    func keyPressed(_ key: UIKey) -> Bool {
        guard let name = Self.name(of: key) else { return false }
        let flags = key.modifierFlags
        return runShortcut(
            name, command: flags.contains(.command) || flags.contains(.control),
            shift: flags.contains(.shift), alt: flags.contains(.alternate)
        )
    }

    /// The key `name`'s shortcut, the shared shortcuts saying what it does; whether it has one.
    func runShortcut(_ name: String, command: Bool, shift: Bool, alt: Bool) -> Bool {
        // the arrows move what is transformed, or the strokes the selection takes
        let nudges = transforming || !selection.takenBox.isEmpty
        guard let shortcut = keyShortcut(key: name, command: command, shift: shift, alt: alt, transforming: nudges) else { return false }
        switch shortcut {
        case .undo: undo()
        case .redo: redo()
        case .copyLayer: copyLayer()
        case .cutLayer: cutLayer()
        case .paste: paste()
        case .paint: select(.paint)
        case .erase: select(.erase)
        case .eyedropper: pickNext.toggle()
        case .selection: toggleSelecting()
        case .transform: toggleTransform()
        case .hueSaturationBrightness: startAdjustment(.hsb)
        case .colours: toggle(.color)
        case .actions: toggle(.actions)
        case .layers: toggle(.layers)
        case .swapColours: swapColors()
        case .deselect: selection.clear()
        case .duplicateLayer: duplicateCurrent()
        case .interface: toggleBars()
        case .clearLayer: clearCurrentLayer()
        case .copyCanvas: runQuickAction(.copyCanvas, at: screenCentre)
        case .colourBalance: runQuickAction(.colorBalance, at: screenCentre)
        case .perspectiveGuide: runQuickAction(.guidePerspective, at: screenCentre)
        case .quickMenu: toggleQuickMenu(at: screenCentre)
        case .greyView: session.setGreyView(on: !session.greyView())
        case let .brushSize(by): changeSize(min(max(size + by, 0), 1))
        case let .nudge(dx, dy):
            if transforming {
                if let moved = session.transformArrow(dx: dx, dy: dy) {
                    transform.moved(moved)
                }
            } else if session.nudgePluginItems(dx: dx, dy: dy) {
                refresh()
            }
        }
        return true
    }

    /// Files dropped on the canvas: pictures on layers of their own, a picture alone ready to
    /// transform; other files' brushes join the library. The copies in `folder` go after them.
    func dropOnCanvas(_ urls: [URL], from folder: URL) {
        let pictures = urls.filter { UTType(filenameExtension: $0.pathExtension)?.conforms(to: .image) == true }
        let others = urls.filter { !pictures.contains($0) }
        guard !urls.isEmpty else { return }
        let gone = DispatchGroup()
        for file in others {
            gone.enter()
            importBrushes(from: file) { gone.leave() }
        }
        if !pictures.isEmpty {
            if transforming {
                endTransform(commit: true)
            }
            closePanels()
            for file in pictures {
                insertPicture(try? Data(contentsOf: file), name: file.lastPathComponent, isPrivate: false, transform: pictures.count == 1)
            }
        }
        gone.notify(queue: .global(qos: .utility)) {
            try? FileManager.default.removeItem(at: folder)
        }
    }
}

/// Files dragged in from another app, onto the canvas.
final class CanvasDropper: NSObject, UIDropInteractionDelegate {
    private unowned let model: CanvasModel

    init(model: CanvasModel) {
        self.model = model
    }

    func dropInteraction(_: UIDropInteraction, canHandle session: any UIDropSession) -> Bool {
        session.localDragSession == nil && session.hasItemsConforming(toTypeIdentifiers: [UTType.item.identifier])
    }

    func dropInteraction(_: UIDropInteraction, sessionDidUpdate _: any UIDropSession) -> UIDropProposal {
        UIDropProposal(operation: .copy)
    }

    func dropInteraction(_: UIDropInteraction, performDrop session: any UIDropSession) {
        loadDropped(session) { [model] urls, folder in
            model.dropOnCanvas(urls, from: folder)
        }
    }
}

/// The single touch gestures companion: a floating bar standing in for gestures that need more than
/// one finger. Undo, redo, Zoom and Move (while one is on, a finger dragged on the canvas zooms or
/// moves it), and Fit Canvas. Dragged by its top edge.
struct TouchCompanion: View {
    let model: CanvasModel
    let screen: CGSize

    /// Where it is, none until it is moved; and where a drag began.
    @State private var at: CGPoint?
    @State private var from: CGPoint?

    private func button(_ icon: PcIcon, _ label: String, on: Bool = false, size: CGFloat, action: @escaping () -> Void) -> some View {
        VStack(spacing: 0) {
            PcIconView(icon, size: size, color: on ? Pc.accent : companionInk)
                .frame(height: 30)
            Text(verbatim: label)
                .font(.system(size: 13))
                .foregroundStyle(companionInk)
                .lineLimit(1)
        }
        .frame(width: 80, height: 50)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 30, style: .circular)
        let first = CGPoint(x: (screen.width - companionWidth) / 2, y: screen.height * companionStartY - companionHeight / 2)
        let place = at ?? first
        VStack(spacing: 0) {
            // the grab line, and the strip it sits in, drags the bar about, kept on the screen
            Capsule()
                .fill(companionGrab)
                .frame(width: 40, height: 5)
                .frame(maxWidth: .infinity)
                .frame(height: 24)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 1, coordinateSpace: .global)
                        .onChanged { drag in
                            let start = from ?? place
                            from = start
                            let k = Interface.shared.scale
                            at = CGPoint(
                                x: min(max(start.x + drag.translation.width / k, 0), screen.width - companionWidth),
                                y: min(max(start.y + drag.translation.height / k, 0), screen.height - companionHeight)
                            )
                        }
                        .onEnded { _ in from = nil }
                )
            HStack(spacing: 4) {
                button(PcIcons.undo, "Undo", size: 30) { model.undo() }
                button(PcIcons.redo, "Redo", size: 30) { model.redo() }
                button(PcIcons.zoom, "Zoom", on: model.companionMode == .zoom, size: 22) { model.toggleCompanionMode(.zoom) }
                button(PcIcons.moveCanvas, "Move", on: model.companionMode == .move, size: 22) { model.toggleCompanionMode(.move) }
                button(PcIcons.fitCanvas, "Fit Canvas", size: 22) { model.settleToFit() }
            }
            .padding(.leading, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            Spacer(minLength: 0)
        }
        .frame(width: companionWidth, height: companionHeight)
        .background(shape.fill(companionFill).shadow(color: .black.opacity(0.3), radius: 8))
        .clipShape(shape)
        .contentShape(shape)
        .onTapGesture {}
        .offset(x: place.x, y: place.y)
    }
}

extension CanvasModel {
    /// Names the colour as it changes (Color Description Notifications): at once, or, while it keeps
    /// changing, the latest colour at most every `colourNameGap`.
    func nameColourSoon() {
        naming?.cancel()
        let wait = max(namedAt.timeIntervalSinceNow + colourNameGap, 0)
        naming = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(wait))
            guard let self, !Task.isCancelled else { return }
            self.namedAt = Date()
            self.showNotice(colourName(rgb: self.colorRgb() & 0xFFFFFF))
        }
    }
}
