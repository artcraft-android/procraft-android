import SwiftUI
import UIKit

private let panelWidth: CGFloat = 376
private let ringOuter: CGFloat = 182
private let ringInner: CGFloat = 124
private let discRadius: CGFloat = 105

/// The colour panel's modes.
enum ColorMode: CaseIterable {
    case disc, classic, harmony, value, palettes

    var label: String {
        switch self {
        case .disc: "Disc"
        case .classic: "Classic"
        case .harmony: "Harmony"
        case .value: "Value"
        case .palettes: "Palettes"
        }
    }

    var icon: PcIcon {
        switch self {
        case .disc: PcIcons.modeDisc
        case .classic: PcIcons.modeClassic
        case .harmony: PcIcons.modeHarmony
        case .value: PcIcons.modeValue
        case .palettes: PcIcons.modePalettes
        }
    }
}

/// The colour panel: the current and second colours, the picker of its mode, and the mode tabs.
/// (The Disc picker for now; the other modes show it until theirs come.)
struct ColorPanel: View {
    let model: CanvasModel
    let arrowX: CGFloat

    var body: some View {
        Popover(width: panelWidth, arrowX: arrowX) {
            ZStack(alignment: .top) {
                PanelHeader(title: "Colors", start: 20, end: 20, chevron: false) {
                    HStack(spacing: 7) {
                        let swatch = RoundedRectangle(cornerRadius: 6, style: .circular)
                        swatch.fill(CanvasSpace.shared.color(model.colorRgb()))
                            .overlay(swatch.strokeBorder(Pc.swatchEdge, lineWidth: 1))
                            .frame(width: 43, height: 25)
                        swatch.fill(CanvasSpace.shared.color(model.secondaryRgb))
                            .frame(width: 43, height: 25)
                            .contentShape(Rectangle())
                            .onTapGesture { model.swapColors() }
                    }
                    .padding(.top, 21.5)
                }
                Capsule()
                    .fill(Pc.grabber)
                    .frame(width: 40, height: 4)
                    .padding(.top, 4)
            }
            DiscPicker(model: model)
            ModeTabs(model: model)
        }
    }
}

/// The disc picker, 364 square: a hue ring (radii 124 to 182, the hue scale running anticlockwise
/// from the right) around the saturation/brightness disc (radius 105).
private struct DiscPicker: View {
    let model: CanvasModel
    @Environment(\.displayScale) private var displayScale
    /// The disc's picture for the hue it was made for.
    @State private var disc: (hue: Float, image: UIImage)?
    /// Whether the touch now held began on the ring.
    @State private var onRing: Bool?

    private static let ring: AngularGradient = {
        let hues = hueStops(control: .ring)
        let last = Float(max(hues.count - 1, 1))
        // anticlockwise from the right: a hue at t round the ring sits at 1 - t of the clockwise sweep
        let stops = hues.enumerated().map { i, h in
            Gradient.Stop(color: CanvasSpace.shared.color(CanvasModel.rgb(hue: h / 360, saturation: 1, brightness: 1)), location: CGFloat(1 - Float(i) / last))
        }
        return AngularGradient(stops: stops.reversed(), center: .center, startAngle: .zero, endAngle: .degrees(360))
    }()

    var body: some View {
        let hue = model.hue
        let side = Int((discRadius * 2 * displayScale).rounded())
        Canvas { context, size in
            let o = CGPoint(x: size.width / 2, y: size.height / 2)
            let mid = (ringInner + ringOuter) / 2
            context.stroke(
                Path(ellipseIn: CGRect(x: o.x - mid, y: o.y - mid, width: mid * 2, height: mid * 2)),
                with: .style(Self.ring),
                lineWidth: ringOuter - ringInner
            )
            if let picture = disc?.image {
                context.draw(Image(uiImage: picture), in: CGRect(x: o.x - discRadius, y: o.y - discRadius, width: discRadius * 2, height: discRadius * 2))
            }
            // the ring's knob sits on the middle of the ring, where the hue is
            let turn = CGFloat(huePosition(control: .ring, hue: hue)) * 2 * .pi
            knob(&context, at: CGPoint(x: o.x + cos(turn) * mid, y: o.y - sin(turn) * mid), radius: 26.2,
                 fill: CanvasSpace.shared.color(CanvasModel.rgb(hue: hue, saturation: 1, brightness: 1)))
            let p = squareToDiscPoint(x: model.saturation * 2 - 1, y: 1 - model.brightness * 2)
            if p.count >= 2 {
                knob(&context, at: CGPoint(x: o.x + CGFloat(p[0]) * discRadius, y: o.y + CGFloat(p[1]) * discRadius), radius: 10.75,
                     fill: CanvasSpace.shared.color(model.colorRgb()))
            }
        }
        .frame(width: 364, height: 364)
        .padding(.leading, 6)
        .padding(.top, 8)
        .padding(.bottom, 17)
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { drag in
                    // in the picker's own square (past its padding)
                    let centre = CGPoint(x: 6 + 182, y: 8 + 182)
                    let d = CGPoint(x: drag.location.x - centre.x, y: drag.location.y - centre.y)
                    if onRing == nil {
                        let start = CGPoint(x: drag.startLocation.x - centre.x, y: drag.startLocation.y - centre.y)
                        onRing = hypot(start.x, start.y) > (discRadius + ringInner) / 2
                    }
                    if onRing == true {
                        var turn = Float(atan2(-d.y, d.x) / (2 * .pi))
                        turn = (turn + 1).truncatingRemainder(dividingBy: 1)
                        model.setColor(hue: hueAt(control: .ring, position: turn), saturation: model.saturation, brightness: model.brightness)
                    } else {
                        var u = d.x / discRadius
                        var v = d.y / discRadius
                        let length = hypot(u, v)
                        if length > 1 {
                            u /= length
                            v /= length
                        }
                        let sq = discToSquarePoint(u: Float(u), v: Float(v))
                        if sq.count >= 2 {
                            model.setColor(hue: model.hue, saturation: (sq[0] + 1) / 2, brightness: (1 - sq[1]) / 2)
                        }
                    }
                }
                .onEnded { _ in onRing = nil }
        )
        .task(id: hue) {
            // the saturation/brightness disc for the hue, with a soft edge, as the core draws it
            if disc?.hue == hue {
                return
            }
            let made = await Task.detached(priority: .userInitiated) { () -> UIImage? in
                let rgba = discPicture(hue: hue, side: UInt32(side))
                return CanvasModel.image(ThumbnailImage(width: UInt32(side), height: UInt32(side), rgba: rgba))
            }.value
            if let made {
                disc = (hue, made)
            }
        }
    }

    /// A colour knob: `fill` inside a thin white ring, over a soft shadow.
    private func knob(_ context: inout GraphicsContext, at c: CGPoint, radius r: CGFloat, fill: Color) {
        var shadowed = context
        shadowed.addFilter(.blur(radius: 2))
        shadowed.fill(Path(ellipseIn: CGRect(x: c.x - r, y: c.y - r + 2, width: r * 2, height: r * 2)), with: .color(Color(argb: 0x5900_0000)))
        context.fill(Path(ellipseIn: CGRect(x: c.x - r, y: c.y - r, width: r * 2, height: r * 2)), with: .color(fill))
        let ring = r - 0.6
        context.stroke(Path(ellipseIn: CGRect(x: c.x - ring, y: c.y - ring, width: ring * 2, height: ring * 2)), with: .color(.white), lineWidth: 1.2)
    }
}

/// The mode tabs along the panel's foot: each its icon over its name, the chosen one lit.
private struct ModeTabs: View {
    let model: CanvasModel

    var body: some View {
        HStack(spacing: 0) {
            ForEach(ColorMode.allCases, id: \.self) { mode in
                let selected = model.colorMode == mode
                ZStack(alignment: .top) {
                    // a 36 frame centred 27 down
                    PcIconView(mode.icon, size: 36, color: selected ? Pc.accent : Pc.panelTitle)
                        .padding(.top, 9)
                    Text(verbatim: mode.label)
                        .font(.system(size: 13, weight: .medium))
                        // the chosen one white, or black with the light interface
                        .foregroundStyle(!selected ? Pc.panelTitle : (Look.shared.light ? Color.black : Color.white))
                        .lineLimit(1)
                        .padding(.top, 41)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                .contentShape(Rectangle())
                .onTapGesture { model.colorMode = mode }
            }
        }
        .frame(height: 70)
    }
}

/// The colour panel's width.
let colorPanelWidth = panelWidth
