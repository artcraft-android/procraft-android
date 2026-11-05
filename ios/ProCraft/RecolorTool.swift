import SwiftUI

private func themed(_ dark: UInt32, _ light: UInt32) -> Color {
    Color(argb: Look.shared.light ? light : dark)
}

private var panelFill: Color { themed(0xF71E_1E1E, 0xF7F1_F1F1) }
private var modeBar: Color { themed(0xD63B_3B3B, 0xD6EE_EEEE) }
private var modeText: Color { themed(0xFFC5_C5C5, 0xFF6A_6A6A) }
private var sliderLabel: Color { themed(0xFF80_8080, 0xFF59_5959) }
private var sliderValue: Color { themed(0xFFED_EDED, 0xFF36_3636) }
private var sliderTrack: Color { themed(0xFF2E_2E2E, 0xFFDD_DDDD) }

/// Recolor: the area under a reticle takes the current colour. Dragged about, the fill follows it; a
/// tap keeps the fill and starts again where it lands. The core keeps it; its fills run on a queue of
/// their own, each kind of change taking the newest when it gets there.
@Observable
final class RecolorTool {
    private static let thresholdKey = "recolorThreshold"

    @ObservationIgnored private unowned let model: CanvasModel

    /// Where the reticle is, canvas pixels.
    private(set) var point: CGPoint?
    /// Its Flood slider, 0..1, kept from one time to the next.
    private(set) var threshold: Float

    @ObservationIgnored private let queue = DispatchQueue(label: "recolor", qos: .userInitiated)
    @ObservationIgnored private let waiting = NSLock()
    @ObservationIgnored private var latest: [String: () -> Void] = [:]

    init(model: CanvasModel) {
        self.model = model
        threshold = UserDefaults.standard.object(forKey: Self.thresholdKey) as? Float ?? 0
    }

    /// Runs `block` on the queue, unless another of its `kind` is given before it gets there.
    private func later(_ kind: String, _ block: @escaping () -> Void) {
        let first = waiting.withLock {
            let first = self.latest[kind] == nil
            self.latest[kind] = block
            return first
        }
        guard first else { return }
        queue.async { [weak self] in
            guard let self else { return }
            let run = self.waiting.withLock { self.latest.removeValue(forKey: kind) }
            run?()
        }
    }

    /// Waits for the fills queued, then does `work`.
    func afterFills<T>(_ work: () -> T) -> T {
        queue.sync(execute: work)
    }

    /// The reticle where the core has it.
    func readPoint() {
        point = model.session.recolorPoint().flatMap { p in
            p.count >= 2 ? model.windowToCanvas(CGPoint(x: CGFloat(p[0]), y: CGFloat(p[1]))) : nil
        }
    }

    /// The reticle in the middle of the screen (window pixels), filling with the current colour.
    func start(at centre: CGPoint) {
        let session = model.session
        let c = model.colorRgb()
        let t = threshold
        queue.sync {
            session.recolorStart(
                x: Float(centre.x), y: Float(centre.y),
                r: Float((c >> 16) & 0xFF) / 255, g: Float((c >> 8) & 0xFF) / 255, b: Float(c & 0xFF) / 255,
                threshold: t
            )
        }
        readPoint()
        model.refresh()
    }

    /// The touches dragged the reticle to window pixels `at`: shown there at once, the fill following.
    func move(to at: CGPoint) {
        guard let shown = model.windowToCanvas(at) else { return }
        point = shown
        let session = model.session
        later("point") { session.recolorMoveTo(x: Float(at.x), y: Float(at.y)) }
    }

    /// A tap while recoloring: the fill so far is kept, and the reticle starts again at `at`.
    func tap(_ at: CGPoint) {
        let session = model.session
        queue.sync {
            session.recolorTap(x: Float(at.x), y: Float(at.y))
        }
        readPoint()
        model.refresh()
    }

    /// The Flood slider moved: the fill redone with it.
    func changeThreshold(_ value: Float) {
        let t = min(max(value, 0), 1)
        threshold = t
        let session = model.session
        later("threshold") { session.recolorThreshold(threshold: t) }
    }

    /// The colour changed: the area under the reticle takes it.
    func changeColour(r: Float, g: Float, b: Float) {
        let session = model.session
        later("colour") { session.recolorColour(r: r, g: g, b: b) }
    }

    /// Ends, keeping what it filled.
    func end() {
        let session = model.session
        queue.sync {
            session.recolorEnd()
        }
        point = nil
        UserDefaults.standard.set(threshold, forKey: Self.thresholdKey)
    }
}

/// Recolor's reticle: four short bars around the point it fills from, 44 across, black over a light
/// colour and white over a dark one.
struct RecolorReticle: View {
    let model: CanvasModel

    var body: some View {
        if let point = model.recolor.point {
            let at = model.canvasToWindow(point)
            let c = model.colorRgb()
            let red = 0.2126 * Float((c >> 16) & 0xFF)
            let green = 0.7152 * Float((c >> 8) & 0xFF)
            let blue = 0.0722 * Float(c & 0xFF)
            let ink: Color = red + green + blue > 0.5 * 255 ? .black : .white
            Canvas { context, _ in
                var bars = Path()
                for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    bars.move(to: CGPoint(x: at.x + dx * 2.5, y: at.y + dy * 2.5))
                    bars.addLine(to: CGPoint(x: at.x + dx * 11.5, y: at.y + dy * 11.5))
                }
                context.stroke(bars, with: .color(ink), lineWidth: 1)
            }
            .allowsHitTesting(false)
        }
    }
}

/// Recolor's toolbar: the Flood slider, how far from the colour under the reticle the fill reaches.
struct RecolorPanel: View {
    let tool: RecolorTool

    var body: some View {
        let t = tool.threshold
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                Text(verbatim: "Flood")
                    .foregroundStyle(sliderLabel)
                Spacer(minLength: 0)
                Text(verbatim: "\(wholeNumber(t * 100))%")
                    .foregroundStyle(sliderValue)
            }
            .font(.system(size: 16))
            .lineLimit(1)
            .padding(.leading, 10)
            .padding(.trailing, 15)
            .padding(.top, 6)
            GeometryReader { g in
                let w = g.size.width
                let x = w * CGFloat(t)
                let knob = min(max(x, 8.5), max(w - 8.5, 8.5))
                ZStack(alignment: .leading) {
                    Capsule().fill(sliderTrack).frame(height: 2)
                    Capsule().fill(Pc.accent).frame(width: x, height: 2)
                    Capsule().fill(Color.white).frame(width: 17, height: 11).offset(x: knob - 8.5)
                }
                .frame(width: w, height: 24)
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
                    guard drag.translation != .zero else { return }
                    tool.changeThreshold(Float(min(max(drag.location.x / max(w, 1), 0), 1)))
                })
            }
            .frame(height: 24)
            .padding(.leading, 10)
            .padding(.trailing, 11)
        }
        .frame(width: 300, height: 54)
        .padding(.top, 8)
        .frame(width: 332, height: 70, alignment: .top)
        .background(RoundedRectangle(cornerRadius: 33, style: .circular).fill(panelFill))
        .contentShape(Rectangle())
        .onTapGesture {}
        .offset(y: -25)
    }
}

/// Layer opacity's band: its glyph, "Opacity" and the opacity, the blue line across the screen under
/// it.
struct LayerOpacityBar: View {
    let value: Float

    /// A ring behind and up to the right, a disc in front, striped where it covers the ring.
    private var glyph: some View {
        Canvas { context, size in
            let u = size.width / 14
            let r = 4.9 * u
            func disc(_ x: CGFloat, _ y: CGFloat) -> Path {
                Path(ellipseIn: CGRect(x: x * u - r, y: y * u - r, width: r * 2, height: r * 2))
            }
            context.stroke(disc(8.7, 5.3), with: .color(modeText), lineWidth: 1.1 * u)
            context.fill(disc(5.4, 8.6), with: .color(modeText))
            // the stripes, only where the disc covers the ring's inside
            var clipped = context
            clipped.clip(to: disc(8.7, 5.3))
            clipped.blendMode = .clear
            var stripes = Path()
            for k in -1 ... 1 {
                let o = CGFloat(k) * 2.2
                stripes.move(to: CGPoint(x: (5.4 + o - 5) * u, y: (8.6 - o - 5) * u))
                stripes.addLine(to: CGPoint(x: (5.4 + o + 5) * u, y: (8.6 - o + 5) * u))
            }
            clipped.stroke(stripes, with: .color(.black), lineWidth: 0.9 * u)
        }
        .frame(width: 14, height: 14)
        .frame(width: 33, height: 32)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 0) {
                glyph
                Text(verbatim: "Opacity")
                    .font(.system(size: 17, weight: .bold))
                    .foregroundStyle(modeText)
                    .lineLimit(1)
                Text(verbatim: "\(wholeNumber(value * 100))%")
                    .font(.system(size: 14))
                    .foregroundStyle(modeText)
                    .lineLimit(1)
                    .padding(.leading, 3)
            }
            .frame(maxWidth: .infinity)
            .frame(height: 32)
            .background(modeBar)
            GeometryReader { g in
                Pc.accent.frame(width: g.size.width * CGFloat(min(max(value, 0), 1)))
            }
            .frame(height: 4)
        }
        .allowsHitTesting(false)
    }
}
