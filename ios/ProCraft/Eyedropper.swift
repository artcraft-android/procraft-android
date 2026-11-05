import SwiftUI
import UIKit

private let loupeOuter: CGFloat = 69
private let loupeInner: CGFloat = 53.5
private let loupeLift: CGFloat = 41

/// The eyedropper while it is open: where it is (window pixels), what it sees and the colour under
/// it, once read.
struct PickState {
    let x: Float
    let y: Float
    let image: UIImage?
    let rgb: UInt32?
    var fromReference = false
}

/// The bar under the top bar while the eyedropper is open.
struct PickingBar: View {
    var fromReference = false

    var body: some View {
        Text(verbatim: fromReference ? "Picking color from reference" : "Picking color from canvas")
            .font(.system(size: 14))
            .foregroundStyle(Pc.messageText)
            .frame(maxWidth: .infinity, minHeight: 24, maxHeight: 24)
            .background(Pc.messageBar)
    }
}

/// The eyedropper's loupe, centred above the finger: a ring whose top half is the colour under the
/// finger and bottom half the current colour, around what is shown there at twice the size.
struct Loupe: View {
    let pick: PickState
    let current: Color
    /// Window pixels to a point.
    let pixels: CGFloat

    var body: some View {
        let area = (loupeOuter + 6) * 2
        Canvas { context, size in
            let c = CGPoint(x: size.width / 2, y: size.height / 2)
            var shadow = context
            shadow.addFilter(.blur(radius: 3))
            shadow.fill(circle(c.x, c.y + 1.5, loupeOuter), with: .color(Color(argb: 0x4000_0000)))
            let mid = (loupeOuter + loupeInner) / 2
            let ring = StrokeStyle(lineWidth: loupeOuter - loupeInner)
            let picked = pick.rgb.map { CanvasSpace.shared.color($0) } ?? current
            var top = Path()
            top.addArc(center: c, radius: mid, startAngle: .degrees(180), endAngle: .degrees(360), clockwise: false)
            context.stroke(top, with: .color(picked), style: ring)
            var bottom = Path()
            bottom.addArc(center: c, radius: mid, startAngle: .degrees(0), endAngle: .degrees(180), clockwise: false)
            context.stroke(bottom, with: .color(current), style: ring)
            if let image = pick.image {
                var inside = context
                inside.clip(to: circle(c.x, c.y, loupeInner))
                inside.draw(
                    Image(uiImage: image).interpolation(.none),
                    in: CGRect(x: c.x - loupeInner, y: c.y - loupeInner, width: loupeInner * 2, height: loupeInner * 2)
                )
            }
            context.stroke(circle(c.x, c.y, loupeOuter), with: .color(Pc.loupeEdge), lineWidth: 0.5)
            context.stroke(circle(c.x, c.y, loupeInner), with: .color(Pc.loupeEdge), lineWidth: 0.5)
            // the crosshair inverts what is under it
            let arm = CGSize(width: 2, height: 1)
            let gap: CGFloat = 1.75
            var cross = context
            cross.blendMode = .difference
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let size = dx != 0 ? arm : CGSize(width: arm.height, height: arm.width)
                let x = dx > 0 ? c.x + gap : dx < 0 ? c.x - gap - arm.width : c.x - arm.height / 2
                let y = dy > 0 ? c.y + gap : dy < 0 ? c.y - gap - arm.width : c.y - arm.height / 2
                cross.fill(Path(CGRect(origin: CGPoint(x: x, y: y), size: size)), with: .color(.white))
            }
        }
        .frame(width: area, height: area)
        .position(x: CGFloat(pick.x) / pixels, y: CGFloat(pick.y) / pixels - loupeLift)
        .allowsHitTesting(false)
    }

    private func circle(_ x: CGFloat, _ y: CGFloat, _ r: CGFloat) -> Path {
        Path(ellipseIn: CGRect(x: x - r, y: y - r, width: r * 2, height: r * 2))
    }
}
