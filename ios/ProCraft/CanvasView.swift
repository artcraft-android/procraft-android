import QuartzCore
import UIKit

/// The canvas: a Metal layer the core's render thread draws into, and every touch on it handed to
/// the core's touches (`crates/core/src/touch.rs`, the same as the Android app's), which paint,
/// move the canvas and say what the app should do. A touch carries each sample the system
/// coalesced since the last, in pixels, with the Pencil's force, angles and barrel roll. The
/// Pencil's hover, double-tap and squeeze go to the core's pen gestures (`touch/pen.rs`), with
/// the system's own Apple Pencil settings for them.
final class CanvasView: UIView {
    override class var layerClass: AnyClass { CAMetalLayer.self }

    let session: CanvasSession
    /// What the touches asked of the app (undo, redo, fit, ...).
    var onCommands: ([TouchCommand]) -> Void = { _ in }

    private var attached = false
    /// Each touch's id for the core while it is down, by the touch.
    private var ids: [ObjectIdentifier: UInt64] = [:]
    private var nextId: UInt64 = 1
    /// The touch whose stroke is drawn ahead to where the system predicts it goes.
    private var strokeTouch: ObjectIdentifier?
    /// Where the Pencil last was over or on the canvas (its double-tap acts there).
    private var pencilAt: CGPoint?
    /// The touches' next wait (a hold, a repeat) running out.
    private var tick: Timer?
    /// The brush's outline where the Pencil or the pointer hovers (shown while hovering only, as
    /// Brush cursor starts).
    private let cursor = BrushCursorView()

    init(session: CanvasSession) {
        self.session = session
        super.init(frame: .zero)
        isMultipleTouchEnabled = true
        isOpaque = true
        backgroundColor = .black
        cursor.frame = bounds
        cursor.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        addSubview(cursor)
        // the Pencil held over the glass, and a trackpad's or mouse's pointer, each on its own
        let pencilHover = UIHoverGestureRecognizer(target: self, action: #selector(pencilHovered(_:)))
        pencilHover.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.pencil.rawValue)]
        addGestureRecognizer(pencilHover)
        let pointerHover = UIHoverGestureRecognizer(target: self, action: #selector(pointerHovered(_:)))
        pointerHover.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.indirectPointer.rawValue)]
        addGestureRecognizer(pointerHover)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("the canvas is made in code")
    }

    /// Pixels to a point on the screen the view is on.
    private var scale: CGFloat {
        window?.screen.scale ?? traitCollection.displayScale
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window == nil {
            detach()
        } else {
            setNeedsLayout()
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard window != nil, bounds.width > 0, bounds.height > 0 else { return }
        contentScaleFactor = scale
        layer.contentsScale = scale
        let width = UInt32(max(1, (bounds.width * scale).rounded()))
        let height = UInt32(max(1, (bounds.height * scale).rounded()))
        if attached {
            session.resizeWindow(width: width, height: height)
        } else {
            attach(width: width, height: height)
        }
    }

    /// Hands the layer to the core, whose render thread draws into it from now on.
    private func attach(width: UInt32, height: UInt32) {
        guard let metal = layer as? CAMetalLayer else { return }
        session.setDisplayScale(pixelsPerPoint: Float(scale))
        // the core takes a retain of its own, let go with the window
        let pointer = UInt64(UInt(bitPattern: Unmanaged.passRetained(metal).toOpaque()))
        let id = acquireLayer(layer: pointer)
        guard id != 0 else {
            NSLog("ProCraft: the canvas's layer wasn't taken")
            return
        }
        do {
            try session.attachWindow(window: id, width: width, height: height)
            attached = true
        } catch {
            releaseLayer(id: id)
            NSLog("ProCraft: attaching the canvas: %@", "\(error)")
        }
    }

    /// Stops drawing: returns once the core's render thread has let go of the layer.
    func detach() {
        tick?.invalidate()
        tick = nil
        guard attached else { return }
        session.detachWindow()
        attached = false
    }

    // MARK: touches

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        // painting, the outline goes (Brush cursor shows it while hovering)
        cursor.hide()
        var out: [TouchCommand] = []
        for touch in touches {
            let key = ObjectIdentifier(touch)
            let id = nextId
            nextId &+= 1
            ids[key] = id
            if ids.count == 1 {
                strokeTouch = key
            }
            out += session.touchDown(id: id, kind: kind(of: touch), samples: samples(of: touch, in: event))
        }
        obey(out)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        var out: [TouchCommand] = []
        for touch in touches {
            guard let id = ids[ObjectIdentifier(touch)] else { continue }
            out += session.touchMove(id: id, samples: samples(of: touch, in: event))
        }
        // the stroke drawn ahead to where it is predicted to go, until the next move
        if let key = strokeTouch,
           let touch = touches.first(where: { ObjectIdentifier($0) == key }),
           let predicted = event?.predictedTouches(for: touch), !predicted.isEmpty
        {
            session.strokePredict(samples: predicted.map(sample))
        }
        obey(out)
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        var out: [TouchCommand] = []
        for touch in touches {
            guard let id = ids.removeValue(forKey: ObjectIdentifier(touch)) else { continue }
            out += session.touchUp(id: id, samples: samples(of: touch, in: event))
        }
        if ids.isEmpty {
            strokeTouch = nil
        }
        obey(out)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        var out: [TouchCommand] = []
        for touch in touches {
            guard let id = ids.removeValue(forKey: ObjectIdentifier(touch)) else { continue }
            // the system took the touch back (a palm, its own gesture): what it did is undone
            out += session.touchCancelOne(id: id)
        }
        if ids.isEmpty {
            strokeTouch = nil
        }
        obey(out)
    }

    // MARK: the Pencil's own gestures

    /// The Pencil held over the canvas: the core follows it (the brush's outline, Hover's touch, a
    /// squeeze held meanwhile), and the outline shows as strongly as the core says.
    @objc private func pencilHovered(_ hover: UIHoverGestureRecognizer) {
        switch hover.state {
        case .began, .changed:
            let at = hover.location(in: self)
            pencilAt = at
            obey(session.touchPenHover(hover: penHover(at, height: hover.zOffset, seconds: CACurrentMediaTime())))
            showHoverCursor()
        default:
            obey(session.touchPenHover(hover: nil))
            cursor.hide()
        }
    }

    /// A trackpad's or mouse's pointer over the canvas: the brush's outline follows it (it isn't
    /// the Pencil's hover, which sets the brush with a finger).
    @objc private func pointerHovered(_ hover: UIHoverGestureRecognizer) {
        switch hover.state {
        case .began, .changed:
            // a pointer has no height over the glass: one that has is the Pencil, followed above
            guard hover.zOffset == 0 else { return }
            cursor.show(at: hover.location(in: self), alpha: 1, session: session, pixels: scale)
        default:
            cursor.hide()
        }
    }

    /// The outline under the hovering Pencil, as the core has it.
    private func showHoverCursor() {
        guard let shown = session.penHoverCursor() else {
            cursor.hide()
            return
        }
        let at = CGPoint(x: CGFloat(shown.x) / scale, y: CGFloat(shown.y) / scale)
        cursor.show(at: at, alpha: CGFloat(shown.alpha), session: session, pixels: scale)
    }

    /// The Pencil's double-tap, where it hovers (`hovering`, else where it last was): what Gesture
    /// controls set it to do, else what the system's own setting for it asks (`action`); that set
    /// to off, nothing.
    func pencilDoubleTapped(at hovering: CGPoint?, action: UIPencilPreferredAction) {
        if let hovering {
            pencilAt = hovering
        }
        let at = hovering ?? pencilAt ?? CGPoint(x: bounds.midX, y: bounds.midY)
        obey(session.touchPenDoubleTap(preferred: penPreferred(action), x: Float(at.x * scale), y: Float(at.y * scale)))
    }

    /// The Pencil squeezed (Apple Pencil Pro): what Gesture controls set it to do, held (the
    /// eyedropper's loupe and Layer Select follow the hovering Pencil), else what the system's own
    /// setting for a squeeze asks (`action`).
    @available(iOS 17.5, *)
    func pencilSqueezed(_ squeeze: UIPencilInteraction.Squeeze, action: UIPencilPreferredAction) {
        let phase: SqueezePhase
        switch squeeze.phase {
        case .began:
            phase = .began
        case .changed:
            phase = .changed
        case .ended:
            phase = .ended
        case .cancelled:
            phase = .cancelled
        @unknown default:
            phase = .cancelled
        }
        var hover: PenHover?
        if let pose = squeeze.hoverPose {
            pencilAt = pose.location
            hover = penHover(pose.location, height: pose.zOffset, seconds: squeeze.timestamp)
        }
        obey(session.touchPenSqueeze(phase: phase, preferred: penPreferred(action), hover: hover, timeMs: squeeze.timestamp * 1000))
        // the eyedropper's loupe stands in for the outline while it shows
        showHoverCursor()
    }

    /// The Pencil over the canvas as the core takes it: pixels, its height (0..1 of how high it is
    /// sensed), and the time in milliseconds of the clock the touches run on.
    private func penHover(_ at: CGPoint, height: CGFloat, seconds: TimeInterval) -> PenHover {
        PenHover(x: Float(at.x * scale), y: Float(at.y * scale), height: Float(height), timeMs: seconds * 1000)
    }

    /// The system's own setting for a Pencil gesture (Settings, Apple Pencil), as the core takes it.
    private func penPreferred(_ action: UIPencilPreferredAction) -> PenPreferred {
        switch action {
        case .ignore:
            return .ignore
        case .switchEraser:
            return .switchEraser
        case .switchPrevious:
            return .switchPrevious
        case .showColorPalette:
            return .showColours
        case .showInkAttributes:
            return .showInkAttributes
        default:
            // the newer choices (iPadOS 17.5) by number: a palette where the Pencil is, and a
            // shortcut the system runs itself; one still newer is the system's own too
            return action.rawValue == 5 ? .showContextualPalette : .runSystemShortcut
        }
    }

    private func kind(of touch: UITouch) -> PointerKind {
        switch touch.type {
        case .pencil:
            return .stylus
        case .indirectPointer:
            return .mouse
        default:
            return .finger
        }
    }

    /// The touch's samples in this event: those the system coalesced since the last, then the
    /// latest.
    private func samples(of touch: UITouch, in event: UIEvent?) -> [PenSample] {
        let coalesced = event?.coalescedTouches(for: touch) ?? []
        return (coalesced.isEmpty ? [touch] : coalesced).map(sample)
    }

    /// One sample, as the core takes it: pixels; pressure 0..1 (the force against the most the
    /// device senses; a touch with none presses fully); the lean from upright and the direction
    /// the Pencil's top leans, clockwise from up (its azimuth is from the view's x axis); the time
    /// in milliseconds of the clock the core's waits run on (`CACurrentMediaTime`'s); and how far
    /// the barrel is turned (Apple Pencil Pro, iPadOS 17.5; 0 for the others).
    private func sample(_ touch: UITouch) -> PenSample {
        let at = touch.preciseLocation(in: self)
        let pencil = touch.type == .pencil
        if pencil {
            pencilAt = at
        }
        let force = touch.maximumPossibleForce > 0 ? touch.force / touch.maximumPossibleForce : 1
        let tilt = pencil ? Double.pi / 2 - Double(touch.altitudeAngle) : 0
        var lean = pencil ? Double(touch.azimuthAngle(in: self)) + Double.pi / 2 : 0
        lean = lean.truncatingRemainder(dividingBy: 2 * Double.pi)
        if lean < 0 {
            lean += 2 * Double.pi
        }
        var roll: Float = 0
        if #available(iOS 17.5, *), pencil {
            roll = Float(touch.rollAngle)
        }
        return PenSample(
            x: Float(at.x * scale),
            y: Float(at.y * scale),
            pressure: Float(min(max(force, 0), 1)),
            tilt: Float(tilt),
            orientation: Float(lean),
            timeMs: touch.timestamp * 1000,
            roll: roll
        )
    }

    /// Hands what the touches asked to the app, and waits for the touches' next deadline.
    private func obey(_ out: [TouchCommand]) {
        if !out.isEmpty {
            onCommands(out)
        }
        tick?.invalidate()
        tick = nil
        guard let due = session.touchDeadline() else { return }
        let wait = max(0, due / 1000 - CACurrentMediaTime())
        tick = Timer.scheduledTimer(withTimeInterval: wait, repeats: false) { [weak self] _ in
            guard let self else { return }
            self.obey(self.session.touchTick(nowMs: CACurrentMediaTime() * 1000))
        }
    }
}
