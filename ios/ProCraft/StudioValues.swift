import Foundation

/// To the nearest whole number, halves going up: -12.5 is -12, where `.rounded()` would make it -13.
func wholeNumber(_ x: Float) -> Int {
    Int((Double(x) + 0.5).rounded(.down))
}

/// A slider's value as it reads.
func shownValue(_ v: Float, _ shown: Shown, most: Float = 16) -> String {
    let percent = wholeNumber(v * 100)
    // between ends that read by name, a value never rounds onto an end (0.996 reads 99%)
    let between = "\(min(max(percent, 1), 99))%"
    let belowTop = "\(min(percent, 99))%"
    func named(_ low: String, _ high: String) -> String {
        v <= 0 ? low : v >= 1 ? high : between
    }
    switch shown {
    case .percent:
        return named("None", "Max")
    case .both:
        return v >= 1 ? "Max" : belowTop
    case .grade:
        // smoothing all the way
        return v >= 1 ? "Smooth" : belowTop
    case .double:
        // the control's middle reads as 100%
        return "\(wholeNumber(v * 200))%"
    case .twice:
        // as a percent, with the control's middle at 100%
        if v <= 0 {
            return "None"
        }
        return v >= 1 ? "Max" : "\(min(max(wholeNumber(v * 200), 1), 199))%"
    case .follow:
        // turning with the stroke: all the way reads as following it
        return v >= 1 ? "Follow stroke" : belowTop
    case .count:
        return "\(1 + wholeNumber(v * 15))"
    case .tip:
        return named("Sharp", "Blunt")
    case .movement:
        return named("Stamp", "Rolling")
    case .zoom:
        return named("Follow size", "Cropped")
    case .charge:
        return named("Disabled", "Max")
    case .metal:
        return named("Non-Metallic", "Metallic")
    case .gloss:
        return named("Glossy", "Matte")
    case .size:
        let size = most * Float(pow(Double(min(max(v, 0), 1)), 2.2))
        // Max only at the control's end: a size of more than a unit reads as its percent
        if v >= 1 {
            return "Max"
        }
        return size <= 0 ? "None" : "\(wholeNumber(size * 100))%"
    }
}

/// How a slider's value is typed: the largest number, whether it may be negative (a "-" key), and
/// what a number makes of the value.
struct TypedNumber {
    let most: Int
    let signed: Bool
    let toValue: (Int) -> Float
}

/// A slider's way of being typed: a percent of the slider (or of its doubled scale), both ways for
/// the controls going both ways, the count itself, or for a size the percent the slider reads (of
/// `most`).
func typedNumber(_ shown: Shown, most: Float) -> TypedNumber {
    switch shown {
    case .percent, .tip, .movement, .zoom, .charge, .metal, .gloss:
        return TypedNumber(most: 100, signed: false) { Float($0) / 100 }
    case .double, .twice:
        return TypedNumber(most: 200, signed: false) { Float($0) / 200 }
    case .both, .follow, .grade:
        return TypedNumber(most: 100, signed: true) { Float($0) / 100 }
    case .count:
        return TypedNumber(most: 16, signed: false) { Float(max($0, 1) - 1) / 15 }
    case .size:
        return TypedNumber(most: wholeNumber(most * 100), signed: false) { n in
            let part = (Float(n) / 100) / max(most, 1e-3)
            return Float(pow(Double(min(max(part, 0), 1)), 1 / 2.2))
        }
    }
}
