import SwiftUI

private let noticeFill = Color(argb: 0xC242_4242)
private let noticeInk = Color(argb: 0xFFD7_D7D7)
private let noticeHeight: CGFloat = 24

/// What undo or redo did (and other passing messages), in a band across the screen just under the
/// top bar: it slides down from behind the bar, stays a moment and slides back up. A new message
/// takes the place of the one there: down, its text changes in place; moving, the band goes on
/// down from where it is. Either way it stays its full time from the new message.
struct NoticeBand: View {
    let model: CanvasModel
    /// How far down the band is (0 hidden behind the bar, 1 all the way).
    @State private var shown: CGFloat = 0
    @State private var text = ""

    var body: some View {
        ZStack {
            // frosted over the canvas, as measured
            Rectangle().fill(.ultraThinMaterial)
            Rectangle().fill(noticeFill)
            Text(verbatim: text)
                .font(.system(size: 13))
                .foregroundStyle(noticeInk)
                .lineLimit(1)
        }
        .frame(height: noticeHeight)
        .offset(y: -(1 - shown) * noticeHeight)
        .frame(maxWidth: .infinity)
        .frame(height: noticeHeight)
        .clipped()
        .offset(y: Pc.barHeight)
        .opacity(shown > 0 ? 1 : 0)
        .allowsHitTesting(false)
        .task(id: model.notice?.id) {
            guard let n = model.notice else { return }
            text = n.text
            let start = ContinuousClock.now
            let easeInOut = Animation.timingCurve(0.42, 0, 0.58, 1, duration: 0.3)
            if shown < 1 {
                withAnimation(easeInOut) { shown = 1 }
                try? await Task.sleep(for: .milliseconds(300))
            }
            // its time counts from the message, the slide down included
            let shownFor: Duration = .milliseconds(n.warning ? 1500 : 750)
            let left = shownFor - (ContinuousClock.now - start)
            if left > .zero {
                try? await Task.sleep(for: left)
            }
            // a newer message cancels this wait, and stays its own time
            guard !Task.isCancelled else { return }
            withAnimation(easeInOut) { shown = 0 }
            try? await Task.sleep(for: .milliseconds(300))
            model.noticeGone(n)
        }
    }
}
