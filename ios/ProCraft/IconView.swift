import SwiftUI

/// One of the interface's icons: its parts, in a `width` x `height` unit frame (Icons.swift).
struct PcIcon {
    let name: String
    let width: CGFloat
    let height: CGFloat
    let parts: [PcIconPart]
}

/// A path filled (even-odd or not) or stroked `stroke` wide, with round caps and joins.
struct PcIconPart {
    let d: String
    let stroke: CGFloat
    let fill: Bool
    let evenOdd: Bool
}

/// An icon drawn in one colour, its frame shown `size` across (else as many points as it has
/// units).
struct PcIconView: View {
    let icon: PcIcon
    var size: CGFloat?
    let color: Color

    init(_ icon: PcIcon, size: CGFloat? = nil, color: Color) {
        self.icon = icon
        self.size = size
        self.color = color
    }

    var body: some View {
        let width = size ?? icon.width
        let scale = width / icon.width
        let paths = IconPaths.paths(icon)
        Canvas { context, _ in
            context.scaleBy(x: scale, y: scale)
            for (part, path) in zip(icon.parts, paths) {
                if part.fill {
                    context.fill(path, with: .color(color), style: FillStyle(eoFill: part.evenOdd))
                }
                if part.stroke > 0 {
                    context.stroke(path, with: .color(color), style: StrokeStyle(lineWidth: part.stroke, lineCap: .round, lineJoin: .round))
                }
            }
        }
        .frame(width: width, height: icon.height * scale)
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

/// The icons' paths, read from their path data once each.
enum IconPaths {
    private static var cache: [String: [Path]] = [:]

    static func paths(_ icon: PcIcon) -> [Path] {
        if let kept = cache[icon.name] {
            return kept
        }
        let made = icon.parts.map { SvgPath.parse($0.d) }
        cache[icon.name] = made
        return made
    }
}

/// SVG path data (as icons and shapes are written) read into a path: moves, lines, curves
/// (cubic, quadratic, smooth) and elliptical arcs, absolute and relative.
enum SvgPath {
    static func parse(_ d: String) -> Path {
        var reader = Reader(Array(d.utf8))
        var path = Path()
        var command: UInt8 = 0
        var current = CGPoint.zero
        var start = CGPoint.zero
        // the last curve's second control point, for a smooth curve's reflection
        var lastCubic: CGPoint?
        var lastQuad: CGPoint?
        while true {
            reader.skipSeparators()
            guard let c = reader.peek() else { break }
            if Reader.isCommand(c) {
                command = c
                reader.advance()
            } else if command == 0 || command == UInt8(ascii: "Z") || command == UInt8(ascii: "z") {
                // numbers with no command to take them
                break
            }
            let relative = command >= UInt8(ascii: "a")
            let origin = relative ? current : .zero
            func point(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
                CGPoint(x: origin.x + x, y: origin.y + y)
            }
            var cubic: CGPoint?
            var quad: CGPoint?
            switch command | 0x20 {
            case UInt8(ascii: "m"):
                guard let x = reader.number(), let y = reader.number() else { return path }
                current = point(x, y)
                start = current
                path.move(to: current)
                // further pairs after a move are lines
                command = relative ? UInt8(ascii: "l") : UInt8(ascii: "L")
            case UInt8(ascii: "l"):
                guard let x = reader.number(), let y = reader.number() else { return path }
                current = point(x, y)
                path.addLine(to: current)
            case UInt8(ascii: "h"):
                guard let x = reader.number() else { return path }
                current = CGPoint(x: origin.x + x, y: current.y)
                path.addLine(to: current)
            case UInt8(ascii: "v"):
                guard let y = reader.number() else { return path }
                current = CGPoint(x: current.x, y: origin.y + y)
                path.addLine(to: current)
            case UInt8(ascii: "c"):
                guard let x1 = reader.number(), let y1 = reader.number(), let x2 = reader.number(),
                      let y2 = reader.number(), let x = reader.number(), let y = reader.number() else { return path }
                let c2 = point(x2, y2)
                current = point(x, y)
                path.addCurve(to: current, control1: point(x1, y1), control2: c2)
                cubic = c2
            case UInt8(ascii: "s"):
                guard let x2 = reader.number(), let y2 = reader.number(), let x = reader.number(),
                      let y = reader.number() else { return path }
                let c1 = lastCubic.map { CGPoint(x: 2 * current.x - $0.x, y: 2 * current.y - $0.y) } ?? current
                let c2 = point(x2, y2)
                current = point(x, y)
                path.addCurve(to: current, control1: c1, control2: c2)
                cubic = c2
            case UInt8(ascii: "q"):
                guard let x1 = reader.number(), let y1 = reader.number(), let x = reader.number(),
                      let y = reader.number() else { return path }
                let c = point(x1, y1)
                current = point(x, y)
                path.addQuadCurve(to: current, control: c)
                quad = c
            case UInt8(ascii: "t"):
                guard let x = reader.number(), let y = reader.number() else { return path }
                let c = lastQuad.map { CGPoint(x: 2 * current.x - $0.x, y: 2 * current.y - $0.y) } ?? current
                current = point(x, y)
                path.addQuadCurve(to: current, control: c)
                quad = c
            case UInt8(ascii: "a"):
                guard let rx = reader.number(), let ry = reader.number(), let rotation = reader.number(),
                      let large = reader.flag(), let sweep = reader.flag(), let x = reader.number(),
                      let y = reader.number() else { return path }
                let end = point(x, y)
                arc(&path, from: current, to: end, rx: rx, ry: ry, degrees: rotation, large: large, sweep: sweep)
                current = end
            case UInt8(ascii: "z"):
                path.closeSubpath()
                current = start
            default:
                return path
            }
            lastCubic = cubic
            lastQuad = quad
        }
        return path
    }

    /// An elliptical arc from `p0` to `p1` as cubic curves (each a quarter turn at most).
    private static func arc(
        _ path: inout Path, from p0: CGPoint, to p1: CGPoint, rx rx0: CGFloat, ry ry0: CGFloat,
        degrees: CGFloat, large: Bool, sweep: Bool
    ) {
        if p0 == p1 {
            return
        }
        var rx = abs(rx0)
        var ry = abs(ry0)
        if rx == 0 || ry == 0 {
            path.addLine(to: p1)
            return
        }
        let phi = degrees * .pi / 180
        let cosPhi = cos(phi)
        let sinPhi = sin(phi)
        let dx = (p0.x - p1.x) / 2
        let dy = (p0.y - p1.y) / 2
        let x1 = cosPhi * dx + sinPhi * dy
        let y1 = -sinPhi * dx + cosPhi * dy
        // radii too small to reach are grown until they just do
        let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry)
        if lambda > 1 {
            rx *= lambda.squareRoot()
            ry *= lambda.squareRoot()
        }
        let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1
        let den = rx * rx * y1 * y1 + ry * ry * x1 * x1
        var coef = den == 0 ? 0 : max(0, num / den).squareRoot()
        if large == sweep {
            coef = -coef
        }
        let cxp = coef * rx * y1 / ry
        let cyp = -coef * ry * x1 / rx
        let cx = cosPhi * cxp - sinPhi * cyp + (p0.x + p1.x) / 2
        let cy = sinPhi * cxp + cosPhi * cyp + (p0.y + p1.y) / 2
        func angle(_ ux: CGFloat, _ uy: CGFloat, _ vx: CGFloat, _ vy: CGFloat) -> CGFloat {
            atan2(ux * vy - uy * vx, ux * vx + uy * vy)
        }
        let theta = angle(1, 0, (x1 - cxp) / rx, (y1 - cyp) / ry)
        var delta = angle((x1 - cxp) / rx, (y1 - cyp) / ry, (-x1 - cxp) / rx, (-y1 - cyp) / ry)
        if !sweep, delta > 0 {
            delta -= 2 * .pi
        } else if sweep, delta < 0 {
            delta += 2 * .pi
        }
        // a point on the unit circle, onto the ellipse
        func onEllipse(_ ux: CGFloat, _ uy: CGFloat) -> CGPoint {
            CGPoint(x: cx + rx * ux * cosPhi - ry * uy * sinPhi, y: cy + rx * ux * sinPhi + ry * uy * cosPhi)
        }
        let segments = max(1, Int((abs(delta) / (.pi / 2)).rounded(.up)))
        let step = delta / CGFloat(segments)
        let t = 4 / 3 * tan(step / 4)
        var a = theta
        for i in 0 ..< segments {
            let b = a + step
            let c1 = onEllipse(cos(a) - t * sin(a), sin(a) + t * cos(a))
            let c2 = onEllipse(cos(b) + t * sin(b), sin(b) - t * cos(b))
            let end = i == segments - 1 ? p1 : onEllipse(cos(b), sin(b))
            path.addCurve(to: end, control1: c1, control2: c2)
            a = b
        }
    }

    /// Path data's numbers and flags, read in turn.
    private struct Reader {
        let bytes: [UInt8]
        var i = 0

        init(_ bytes: [UInt8]) {
            self.bytes = bytes
        }

        static func isCommand(_ c: UInt8) -> Bool {
            switch c | 0x20 {
            case UInt8(ascii: "m"), UInt8(ascii: "l"), UInt8(ascii: "h"), UInt8(ascii: "v"),
                 UInt8(ascii: "c"), UInt8(ascii: "s"), UInt8(ascii: "q"), UInt8(ascii: "t"),
                 UInt8(ascii: "a"), UInt8(ascii: "z"):
                return true
            default:
                return false
            }
        }

        func peek() -> UInt8? {
            i < bytes.count ? bytes[i] : nil
        }

        mutating func advance() {
            i += 1
        }

        mutating func skipSeparators() {
            while let c = peek(), c == 0x20 || c == 0x2C || c == 0x0A || c == 0x0D || c == 0x09 {
                i += 1
            }
        }

        /// An arc's flag: a single 0 or 1, which may run straight into what follows.
        mutating func flag() -> Bool? {
            skipSeparators()
            guard let c = peek(), c == UInt8(ascii: "0") || c == UInt8(ascii: "1") else { return nil }
            i += 1
            return c == UInt8(ascii: "1")
        }

        /// A number: a sign, digits, a point and more digits, an exponent; a second point starts
        /// the next number.
        mutating func number() -> CGFloat? {
            skipSeparators()
            let begin = i
            if let c = peek(), c == UInt8(ascii: "-") || c == UInt8(ascii: "+") {
                i += 1
            }
            var digits = false
            while let c = peek(), c >= UInt8(ascii: "0"), c <= UInt8(ascii: "9") {
                i += 1
                digits = true
            }
            if peek() == UInt8(ascii: ".") {
                i += 1
                while let c = peek(), c >= UInt8(ascii: "0"), c <= UInt8(ascii: "9") {
                    i += 1
                    digits = true
                }
            }
            guard digits else {
                i = begin
                return nil
            }
            if let c = peek(), c == UInt8(ascii: "e") || c == UInt8(ascii: "E") {
                let mark = i
                i += 1
                if let s = peek(), s == UInt8(ascii: "-") || s == UInt8(ascii: "+") {
                    i += 1
                }
                var exponent = false
                while let c = peek(), c >= UInt8(ascii: "0"), c <= UInt8(ascii: "9") {
                    i += 1
                    exponent = true
                }
                if !exponent {
                    i = mark
                }
            }
            let text = String(decoding: bytes[begin ..< i], as: UTF8.self)
            return Double(text).map { CGFloat($0) }
        }
    }
}
