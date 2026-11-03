import SwiftUI

/// The side of a popover its arrow sticks out of, pointing at its button; or none.
enum ArrowSide {
    case top, bottom, left, right, none
}

/// A popover's outline: its rounded body and the arrow out of its `side`, the arrow's tip `arrowX`
/// from the body's left edge (from its top, for an arrow on the left or right).
struct PopoverShape: Shape {
    var side: ArrowSide
    var arrowX: CGFloat
    var corner: CGFloat

    func path(in rect: CGRect) -> Path {
        let arrowH = Pc.arrowHeight
        let half = arrowH
        let tip: CGFloat = 2
        let at = arrowX
        let w = rect.width
        let h = rect.height
        var path = Path()
        func body(_ r: CGRect) {
            path.addRoundedRect(in: r, cornerSize: CGSize(width: corner, height: corner), style: .circular)
        }
        switch side {
        case .top:
            body(CGRect(x: 0, y: arrowH, width: w, height: h - arrowH))
            path.move(to: CGPoint(x: at - half - tip, y: arrowH + 0.5))
            path.addLine(to: CGPoint(x: at - tip, y: tip))
            path.addQuadCurve(to: CGPoint(x: at + tip, y: tip), control: CGPoint(x: at, y: 0))
            path.addLine(to: CGPoint(x: at + half + tip, y: arrowH + 0.5))
            path.closeSubpath()
        case .bottom:
            body(CGRect(x: 0, y: 0, width: w, height: h - arrowH))
            path.move(to: CGPoint(x: at - half - tip, y: h - arrowH - 0.5))
            path.addLine(to: CGPoint(x: at - tip, y: h - tip))
            path.addQuadCurve(to: CGPoint(x: at + tip, y: h - tip), control: CGPoint(x: at, y: h))
            path.addLine(to: CGPoint(x: at + half + tip, y: h - arrowH - 0.5))
            path.closeSubpath()
        case .left:
            body(CGRect(x: arrowH, y: 0, width: w - arrowH, height: h))
            path.move(to: CGPoint(x: arrowH + 0.5, y: at - half - tip))
            path.addLine(to: CGPoint(x: tip, y: at - tip))
            path.addQuadCurve(to: CGPoint(x: tip, y: at + tip), control: CGPoint(x: 0, y: at))
            path.addLine(to: CGPoint(x: arrowH + 0.5, y: at + half + tip))
            path.closeSubpath()
        case .right:
            body(CGRect(x: 0, y: 0, width: w - arrowH, height: h))
            path.move(to: CGPoint(x: w - arrowH - 0.5, y: at - half - tip))
            path.addLine(to: CGPoint(x: w - tip, y: at - tip))
            path.addQuadCurve(to: CGPoint(x: w - tip, y: at + tip), control: CGPoint(x: w, y: at))
            path.addLine(to: CGPoint(x: w - arrowH - 0.5, y: at + half + tip))
            path.closeSubpath()
        case .none:
            body(rect)
        }
        return path
    }
}

/// A panel hanging from the top bar, with an arrow pointing up at its button: `arrowX` is the
/// tip's distance from the panel's left edge (from its top, for an arrow on the `side`).
/// `popoverPlace` finds where it goes. Without a `fill` of its own it is frosted over the canvas:
/// translucent, with what is behind it blurred. `width` is the body's: a side arrow adds to it.
struct Popover<Content: View>: View {
    let width: CGFloat
    let arrowX: CGFloat
    var fill: Color?
    var corner: CGFloat = Pc.popoverCorner
    var side: ArrowSide = .top
    @ViewBuilder let content: () -> Content

    var body: some View {
        let shape = PopoverShape(side: side, arrowX: arrowX, corner: corner)
        let sideways = side == .left || side == .right
        let shadow = Pc.popoverShadow
        VStack(spacing: 0) {
            content()
        }
        .clipShape(RoundedRectangle(cornerRadius: corner, style: .circular))
        .padding(.top, side == .top ? Pc.arrowHeight : 0)
        .padding(.bottom, side == .bottom ? Pc.arrowHeight : 0)
        .padding(.leading, side == .left ? Pc.arrowHeight : 0)
        .padding(.trailing, side == .right ? Pc.arrowHeight : 0)
        .frame(width: sideways ? width + Pc.arrowHeight : width)
        .background {
            ZStack {
                // the shadow only around the panel: none of it shows through
                shape.fill(Color.black)
                    .shadow(color: shadow, radius: Pc.popoverShadowRadius / 2)
                    .mask {
                        Rectangle()
                            .padding(-Pc.popoverShadowRadius * 2)
                            .overlay(shape.blendMode(.destinationOut))
                            .compositingGroup()
                    }
                if let fill {
                    shape.fill(fill)
                } else {
                    shape.fill(.ultraThinMaterial)
                    shape.fill(Pc.popoverFrost)
                }
            }
        }
    }
}

/// Where a popover goes: its left edge and its arrow's tip from that edge.
struct PopoverPlace {
    let left: CGFloat
    let arrowX: CGFloat
}

/// A popover `width` wide is centred on its button (the tip sits `Pc.arrowShift` right of the
/// button's centre) and kept `Pc.popoverGap` inside a screen `screenWidth` wide, and at least
/// `minLeft` from its left edge (clear of the side bar).
func popoverPlace(
    width: CGFloat, buttonCentreX: CGFloat, screenWidth: CGFloat,
    minLeft: CGFloat = Pc.popoverGap, minRight: CGFloat = Pc.popoverGap
) -> PopoverPlace {
    let tip = buttonCentreX + Pc.arrowShift
    let left = min(max(tip - width / 2, minLeft), max(screenWidth - minRight - width, minLeft))
    return PopoverPlace(left: left, arrowX: tip - left)
}
