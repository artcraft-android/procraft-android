import SwiftUI

/// How the interface moves: every animation's timing in one place, as measured.
enum Motion {
    /// The ease in and out of most timed changes.
    static func easeInOut(_ seconds: Double) -> Animation {
        .timingCurve(0.42, 0, 0.58, 1, duration: seconds)
    }

    /// Slowing as it arrives.
    static func easeOut(_ seconds: Double) -> Animation {
        .timingCurve(0, 0, 0.58, 1, duration: seconds)
    }

    /// Speeding up as it goes.
    static func easeIn(_ seconds: Double) -> Animation {
        .timingCurve(0.42, 0, 1, 1, duration: seconds)
    }

    /// The curve most timed changes take.
    static func settle(_ seconds: Double) -> Animation {
        .timingCurve(0.2, 0.8, 0.5, 1, duration: seconds)
    }

    /// A menu opening: it grows out of the point it hangs from, overshooting a little.
    static let menuOpen = Animation.timingCurve(0.26, 1.22, 0.39, 1.01, duration: 0.3)
    static let menuClose = settle(0.3)
    static let menuStartScale: CGFloat = 0.001
    static let menuEndScale: CGFloat = 0.8

    /// The bars hidden and shown (the interface hidden).
    static let bars = easeInOut(0.3)
    /// The side bar sliding off its edge as a mode takes the screen, and back.
    static let sideBarOut = easeIn(0.13)
    static let sideBarIn = easeOut(0.2)
    /// A bottom toolbar rising from below the screen on a spring settling in 0.4 s, and dropping
    /// away in 0.075 s.
    static let toolbarIn = Animation.spring(duration: 0.4, bounce: 0.1)
    static let toolbarOut = easeIn(0.075)
    /// The brush studio and the full-screen pages sliding up and down.
    static let studioIn = Animation.spring(duration: 0.5, bounce: 0)
    static let studioOut = easeInOut(0.3)
    /// A tool's header fading in over the top bar as its mode begins.
    static let toolHeaderIn = easeInOut(0.22)
    /// A panel's pages pushed in and back.
    static let panelPage = Animation.timingCurve(0.25, 0.1, 0.25, 1, duration: 0.45)
    /// A panel fading away as something picked from it takes over.
    static let panelFade = easeInOut(0.25)
    /// The Actions panel changing tabs.
    static let actionsTabIn = easeInOut(0.2)
    /// An alert coming and going.
    static let alertIn = easeInOut(0.3)
    static let alertOut = easeOut(0.35)
    /// A swiped row settling open or shut.
    static let swipeSettle = Animation.spring(duration: 0.3, bounce: 0.2)
    /// Rows moving to make room.
    static let rowMove = settle(0.35)
    /// The QuickMenu's buttons spreading from its middle, and drawing back in.
    static let quickMenuOpen = Animation.timingCurve(0.25, 0.1, 0.25, 1, duration: 0.15)
    static let quickMenuClose = easeIn(0.12)
    /// A swatch pressed and let go.
    static let swatchPressed: CGFloat = 0.78
    static let swatchPress = Animation.spring(duration: 0.1, bounce: 0.2)
    static let swatchRelease = Animation.spring(duration: 0.3, bounce: 0.5)
    /// The gallery going into and out of Select.
    static let selectMode = easeInOut(0.15)
}

/// A touch's press and release on a view, the way a finger held on a row is seen: `held` follows
/// it, and `perform` runs once if it is let go without having slid off.
struct PressGesture: ViewModifier {
    @Binding var held: Bool
    var enabled = true
    let perform: () -> Void

    func body(content: Content) -> some View {
        content.gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { _ in
                    if enabled {
                        held = true
                    }
                }
                .onEnded { drag in
                    held = false
                    if enabled, abs(drag.translation.height) < 10, abs(drag.translation.width) < 10 {
                        perform()
                    }
                }
        )
    }
}
