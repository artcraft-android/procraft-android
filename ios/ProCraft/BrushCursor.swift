import QuartzCore
import UIKit

/// The brush's outline where the Pencil (or the pointer) hovers over the canvas: the brush's own
/// shape at the size it paints on the screen, as the core gives it, drawn as a thin dark line
/// edged in white, or in the paint colour, as the brush says (none at all for some).
final class BrushCursorView: UIView {
    private let under = CAShapeLayer()
    private let over = CAShapeLayer()
    /// What was last read: the radius (points), how it is drawn (the brush's `cursor_outline`:
    /// none, rings, the colour) and when.
    private var radius: CGFloat = 0
    private var style = 1
    private var readAt: CFTimeInterval = 0

    override init(frame: CGRect) {
        super.init(frame: frame)
        isUserInteractionEnabled = false
        isOpaque = false
        backgroundColor = .clear
        isHidden = true
        for line in [under, over] {
            line.fillColor = nil
            line.lineJoin = .round
            layer.addSublayer(line)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("the cursor is made in code")
    }

    /// Shows the outline at `point` (points in this view), `alpha` strong, sized and shaped for
    /// the current brush as `session` has it (`pixels` window pixels a point).
    func show(at point: CGPoint, alpha: CGFloat, session: CanvasSession, pixels: CGFloat) {
        let now = CACurrentMediaTime()
        // read as it appears, and again now and then: the brush or its size may change meanwhile
        if isHidden || now - readAt >= 0.1 {
            readAt = now
            let info = session.info()
            radius = CGFloat(info.brushDiameter * info.zoom / 2) / pixels
            style = Int(session.currentBrushSetting(key: "cursor_outline").rounded())
            restyle(colour: session.color(), pixels: pixels)
            reshape(session.brushOutline())
        }
        // too small to see, or a brush that shows none
        guard radius >= 2, style != 0, alpha > 0 else {
            isHidden = true
            return
        }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        under.position = point
        over.position = point
        self.alpha = min(alpha, 1)
        isHidden = false
        CATransaction.commit()
    }

    func hide() {
        isHidden = true
    }

    /// Contrasting rings (white under black), or the colour over a light edge.
    private func restyle(colour: [Float], pixels: CGFloat) {
        under.contentsScale = pixels
        over.contentsScale = pixels
        under.strokeColor = UIColor(white: 1, alpha: style == 2 ? 0.6 : 1).cgColor
        under.lineWidth = 3
        if style == 2, colour.count >= 3 {
            over.strokeColor = UIColor(red: CGFloat(colour[0]), green: CGFloat(colour[1]), blue: CGFloat(colour[2]), alpha: 1).cgColor
            over.lineWidth = 1.5
        } else {
            over.strokeColor = UIColor.black.cgColor
            over.lineWidth = 1
        }
    }

    /// The brush's own loops (x, y pairs in brush diameters about its middle), or a circle for the
    /// round tip, about the layers' position.
    private func reshape(_ loops: [[Float]]) {
        let path = CGMutablePath()
        let across = 2 * radius
        let drawn = loops.filter { $0.count >= 6 }
        if drawn.isEmpty {
            path.addEllipse(in: CGRect(x: -radius, y: -radius, width: across, height: across))
        }
        for loop in drawn {
            path.move(to: CGPoint(x: CGFloat(loop[0]) * across, y: CGFloat(loop[1]) * across))
            for i in stride(from: 2, to: loop.count - 1, by: 2) {
                path.addLine(to: CGPoint(x: CGFloat(loop[i]) * across, y: CGFloat(loop[i + 1]) * across))
            }
            path.closeSubpath()
        }
        under.path = path
        over.path = path
    }
}
