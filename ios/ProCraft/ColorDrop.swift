import SwiftUI
import UIKit

/// How long the band offers Continue filling after a drop, seconds.
private let bandShown: TimeInterval = 2.5
/// How far a touch goes before it is a drag, points; how long it stays still to be a hold, seconds.
private let dragSlop: CGFloat = 8
private let holdWait: TimeInterval = 0.4
/// The dragged colour's dot, across.
private let dotSize: CGFloat = 55.5

/// ColorDrop: the colour dragged from the colour button (or a swatch) and dropped on the canvas
/// fills the area around where it lands. The core says when it rests and what a slide does; this
/// pours, and keeps what the band shows.
@Observable
final class ColorDrop {
    /// What the band under the top bar shows.
    enum Band {
        case none, dragging, dropped, filling
    }

    private static let thresholdKey = "dropThreshold"

    @ObservationIgnored private unowned let model: CanvasModel

    private(set) var band = Band.none
    /// The dragged colour's dot (window points) and its colour (0xRRGGBB), while it is dragged.
    private(set) var dot: (at: CGPoint, rgb: UInt32)?
    /// The threshold the colour fills with (0..1), kept from one drop to the next.
    private(set) var threshold: Float
    /// The threshold is being slid: its row shows under the band.
    private(set) var sliding = false
    /// Where a dragged colour let go fills nothing (the colour panel, while open), window points.
    @ObservationIgnored var avoid: CGRect?

    /// The swatch's colour when a swatch was dropped (filling goes on with it); else the current one.
    @ObservationIgnored private var swatch: UInt32?
    @ObservationIgnored private var bandTimer: Timer?
    @ObservationIgnored private var restTimer: Timer?
    /// The colour being dragged: where it is and whether it has come to rest (the core's rules).
    @ObservationIgnored private var drag: ColourDrag?
    /// It came to rest and has begun filling (or tried to, and couldn't).
    @ObservationIgnored private var poured = false
    @ObservationIgnored private var pourRefused = false

    /// A fill's work, in order, off the main thread (a slid threshold redoes the whole fill).
    @ObservationIgnored private let queue = DispatchQueue(label: "pour", qos: .userInitiated)
    /// The latest threshold waiting for that queue.
    @ObservationIgnored private let waiting = NSLock()
    @ObservationIgnored private var nextThreshold: Float?

    init(model: CanvasModel) {
        self.model = model
        threshold = UserDefaults.standard.object(forKey: Self.thresholdKey) as? Float ?? 0.5
    }

    private func keepThreshold() {
        if sliding {
            UserDefaults.standard.set(threshold, forKey: Self.thresholdKey)
        }
        sliding = false
    }

    private static func channels(_ rgb: UInt32) -> (r: Float, g: Float, b: Float) {
        (Float((rgb >> 16) & 0xFF) / 255, Float((rgb >> 8) & 0xFF) / 255, Float(rgb & 0xFF) / 255)
    }

    private static var now: Double {
        ProcessInfo.processInfo.systemUptime * 1000
    }

    private static func point(_ step: DragStep) -> CGPoint {
        CGPoint(x: CGFloat(step.dotX), y: CGFloat(step.dotY))
    }

    // MARK: the drag

    /// A colour starts being dragged from the colour button, or `swatch` from the palette, by a pen
    /// or not.
    func start(at: CGPoint, swatch: UInt32?, pen: Bool) {
        if swatch == nil {
            model.closePanels()
        }
        bandTimer?.invalidate()
        self.swatch = swatch
        dot = (at, swatch ?? model.colorRgb())
        band = .dragging
        poured = false
        pourRefused = false
        drag = ColourDrag(x: Float(at.x), y: Float(at.y), ms: Self.now, pen: pen, threshold: threshold)
        waitForRest()
    }

    /// The dragged colour's pointer moved: the dot follows, or, resting, the threshold slides.
    func move(to at: CGPoint) {
        guard let drag else { return }
        let step = drag.moved(x: Float(at.x), y: Float(at.y), ms: Self.now)
        let shown = Self.point(step)
        dot = dot.map { (shown, $0.rgb) }
        if step.slid {
            if poured {
                pourThreshold(step.threshold)
            } else if !pourRefused {
                threshold = step.threshold
                sliding = true
                pourHere(shown)
            }
        }
        waitForRest()
    }

    private func pourHere(_ at: CGPoint) {
        poured = pourBegin(at)
        pourRefused = !poured
        if poured {
            dot = nil
        }
    }

    /// Held still a moment, the colour comes to rest.
    private func waitForRest() {
        restTimer?.invalidate()
        guard let due = drag?.restDue() else { return }
        restTimer = Timer.scheduledTimer(withTimeInterval: max(due - Self.now, 1) / 1000, repeats: false) { [weak self] _ in
            self?.rest()
            self?.waitForRest()
        }
    }

    /// The colour comes to rest if it is due, on the artwork off the bars, and fills there at once,
    /// the fill showing instead of the dot (a slide then sets the threshold).
    private func rest() {
        guard let drag else { return }
        let at = Self.point(drag.now())
        let pixels = model.pixels
        let lands = at.y > Pc.barHeight && avoid?.contains(at) != true
            && model.session.pourLands(x: Float(at.x * pixels), y: Float(at.y * pixels))
        guard drag.rest(ms: Self.now, droppable: lands), !poured, !pourRefused else { return }
        pourHere(at)
    }

    /// The dragged colour let go at `at` (nil: somewhere it doesn't fill, like the top bar).
    func end(at: CGPoint?) {
        let step = drag?.now()
        drag = nil
        restTimer?.invalidate()
        // come to rest, it fills where it rests (already, if its threshold was slid)
        if poured {
            poured = false
            dot = nil
            pourEnd(keep: true)
            offerToContinue()
            return
        }
        let rested = step.flatMap { $0.resting ? Self.point($0) : nil }
        let target = at.map { rested ?? $0 }
        dot = nil
        if let target, avoid?.contains(target) != true, pourBegin(target) {
            pourEnd(keep: true)
            offerToContinue()
        } else {
            band = .none
        }
    }

    /// The band offers to go on filling for a moment.
    private func offerToContinue() {
        band = .dropped
        bandTimer = Timer.scheduledTimer(withTimeInterval: bandShown, repeats: false) { [weak self] _ in
            if self?.band == .dropped {
                self?.band = .none
            }
        }
    }

    // MARK: filling

    /// Continue filling: each touch on the canvas fills, until `endFilling`.
    func continueFilling() {
        bandTimer?.invalidate()
        band = .filling
    }

    /// Filling ends, and Recolor with it.
    func endFilling() {
        endBand()
        model.endRecolor()
    }

    /// The band goes, and Continue filling with it.
    func endBand() {
        bandTimer?.invalidate()
        band = .none
        sliding = false
    }

    /// Continue filling: the colour and threshold the canvas's touches fill with; none when not
    /// filling.
    func touchFill() -> Fill? {
        guard band == .filling else { return nil }
        let c = Self.channels(swatch ?? model.colorRgb())
        return Fill(r: c.r, g: c.g, b: c.b, threshold: threshold)
    }

    /// The touches' fill was let go of (kept, or taken back by a second finger).
    func filled() {
        keepThreshold()
        model.refresh()
    }

    /// Fills from window point `p` with the dropped colour; false if nothing was filled.
    private func pourBegin(_ p: CGPoint) -> Bool {
        guard model.currentTakesChanges() else { return false }
        // text takes no colour until it is rasterized
        if model.layers.first(where: { $0.current })?.text == true {
            model.askRasterize = true
            return false
        }
        let c = Self.channels(swatch ?? model.colorRgb())
        let session = model.session
        let x = Float(p.x * model.pixels)
        let y = Float(p.y * model.pixels)
        let t = threshold
        return queue.sync {
            session.pourBegin(x: x, y: y, r: c.r, g: c.g, b: c.b, threshold: t)
        }
    }

    /// The fill under way redone with `value`, which is kept for the next.
    func pourThreshold(_ value: Float) {
        let t = min(max(value, 0), 1)
        threshold = t
        sliding = true
        let session = model.session
        let first = waiting.withLock {
            let first = nextThreshold == nil
            nextThreshold = t
            return first
        }
        guard first else { return }
        queue.async { [weak self] in
            guard let self else { return }
            let latest = waiting.withLock {
                let latest = self.nextThreshold
                self.nextThreshold = nil
                return latest
            }
            if let latest {
                session.pourThreshold(threshold: latest)
            }
        }
    }

    /// The fill under way ends: `keep` makes it an undo step, else it is taken back.
    private func pourEnd(keep: Bool) {
        let session = model.session
        queue.sync {
            session.pourEnd(keep: keep, name: nil)
        }
        keepThreshold()
        model.refresh()
    }
}

/// What takes the touches on a colour that can be dragged out: its own so a pen is told from a
/// finger.
final class DropTouchView: UIView {
    var drop: ColorDrop?
    var swatch: UInt32?
    var onHold: (() -> Void)?
    var onTap: () -> Void = {}

    private var touch: UITouch?
    private var start = CGPoint.zero
    private var dragging = false
    private var held = false
    private var holdTimer: Timer?

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard touch == nil, let first = touches.first else { return }
        touch = first
        start = Interface.shared.point(first.location(in: nil))
        dragging = false
        held = false
        guard onHold != nil else { return }
        holdTimer = Timer.scheduledTimer(withTimeInterval: holdWait, repeats: false) { [weak self] _ in
            self?.held = true
            self?.onHold?()
        }
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch, touches.contains(touch), !held else { return }
        let at = Interface.shared.point(touch.location(in: nil))
        if !dragging {
            guard hypot(at.x - start.x, at.y - start.y) > dragSlop else { return }
            holdTimer?.invalidate()
            dragging = true
            drop?.start(at: start, swatch: swatch, pen: touch.type == .pencil)
        }
        drop?.move(to: at)
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch, touches.contains(touch) else { return }
        let at = Interface.shared.point(touch.location(in: nil))
        finish {
            // let go on the top bar, it is put back
            drop?.end(at: at.y > Pc.barHeight ? at : nil)
        } otherwise: {
            if !held {
                onTap()
            }
        }
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch, touches.contains(touch) else { return }
        finish {
            drop?.end(at: nil)
        } otherwise: {}
    }

    private func finish(dragged: () -> Void, otherwise: () -> Void) {
        holdTimer?.invalidate()
        touch = nil
        if dragging {
            dragged()
        } else {
            otherwise()
        }
        dragging = false
    }
}

/// A colour that can be dragged out onto the canvas to fill there (the colour button, or a swatch
/// giving its colour): tapped it does `onTap`, held still `onHold`; dragged, its colour goes with
/// the finger as a dot and fills where it is let go below the top bar.
struct DropSource: UIViewRepresentable {
    let model: CanvasModel
    var swatch: UInt32?
    var onHold: (() -> Void)?
    let onTap: () -> Void

    func makeUIView(context: Context) -> DropTouchView {
        DropTouchView()
    }

    func updateUIView(_ view: DropTouchView, context: Context) {
        view.drop = model.drop
        view.swatch = swatch
        view.onHold = onHold
        view.onTap = onTap
    }
}

/// The colour being dragged out: a flat dot under the finger.
struct ColorDropDot: View {
    let drop: ColorDrop

    var body: some View {
        if let dot = drop.dot {
            Circle()
                .fill(CanvasSpace.shared.color(dot.rgb))
                .frame(width: dotSize, height: dotSize)
                .offset(x: dot.at.x - dotSize / 2, y: dot.at.y - dotSize / 2)
                .allowsHitTesting(false)
        }
    }
}

/// ColorDrop's band under the top bar, sliding down from behind it: while a colour is dragged and
/// while filling goes on, with the threshold as a blue line across the screen under it; after a
/// drop, for a moment, offering to continue filling. Sliding the threshold adds a row reading it.
struct ColorDropBand: View {
    let drop: ColorDrop
    let width: CGFloat

    /// How far down it is, and what it showed, kept while it slides away.
    @State private var shown: CGFloat = 0
    @State private var kept = ColorDrop.Band.none

    private var light: Bool {
        Look.shared.light
    }

    private var blue: Color {
        Color(argb: light ? 0xFF0A_8CFD : 0xFF00_76FD)
    }

    private var ink: Color {
        Color(argb: light ? 0xFF22_2222 : 0xFFC5_C5C5)
    }

    private var tick: some View {
        Canvas { context, size in
            let u = size.width / 20
            var path = Path()
            path.move(to: CGPoint(x: 5.8 * u, y: 10.1 * u))
            path.addLine(to: CGPoint(x: 8.6 * u, y: 12.6 * u))
            path.addLine(to: CGPoint(x: 14 * u, y: 7.2 * u))
            context.stroke(
                path, with: .color(.white),
                style: StrokeStyle(lineWidth: 2 * u, lineCap: .round, lineJoin: .round)
            )
        }
        .frame(width: 20, height: 20)
        .background(Circle().fill(blue))
        .contentShape(Circle())
        .onTapGesture { drop.endFilling() }
    }

    private var continueButton: some View {
        Text(verbatim: "Continue filling")
            .font(.system(size: 11.7))
            .foregroundStyle(Color.white)
            .lineLimit(1)
            .frame(width: 101, height: 20)
            .background(RoundedRectangle(cornerRadius: 10, style: .circular).fill(blue))
            .contentShape(Rectangle())
            .onTapGesture { drop.continueFilling() }
    }

    var body: some View {
        let line = kept == .dragging || kept == .filling
        let height: CGFloat = 32 + (drop.sliding ? 24 : 0) + (line ? 4 : 0)
        let opening = drop.band != .none
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 13) {
                Text(verbatim: "ColorDrop")
                    .font(.system(size: 16.5, weight: .bold))
                    .foregroundStyle(ink)
                    .lineLimit(1)
                if kept == .filling {
                    // the tick ends filling
                    tick
                } else {
                    continueButton
                }
            }
            .frame(width: width, height: 32)
            .background(
                Rectangle()
                    .fill(Color(argb: light ? 0xD6EE_EEEE : 0xD63B_3B3B))
                    .allowsHitTesting(false)
            )
            if drop.sliding {
                Text(verbatim: "Threshold \(wholeNumber(drop.threshold * 100))%")
                    .font(.system(size: 14.4))
                    .foregroundStyle(ink)
                    .lineLimit(1)
                    .frame(width: width, height: 24)
                    .background(Rectangle().fill(Color(argb: light ? 0xCCDC_DCDC : 0xCC4A_4A4A)))
                    .allowsHitTesting(false)
            }
            if line {
                Rectangle()
                    .fill(blue)
                    .frame(width: width * CGFloat(drop.threshold), height: 4)
                    .allowsHitTesting(false)
            }
        }
        .offset(y: -(1 - shown) * height)
        .frame(width: width, height: height, alignment: .top)
        .clipped()
        .offset(y: Pc.barHeight)
        .opacity(shown > 0 ? 1 : 0)
        .allowsHitTesting(opening)
        .onChange(of: drop.band, initial: true) { _, band in
            if band != .none {
                kept = band
            }
            withAnimation(.timingCurve(0.42, 0, 0.58, 1, duration: 0.3)) {
                shown = band == .none ? 0 : 1
            }
        }
    }
}
