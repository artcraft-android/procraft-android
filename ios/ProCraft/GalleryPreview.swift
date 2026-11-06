import SwiftUI
import UIKit

private let previewClose = Color(argb: 0xFFD7_D7D7)
private let previewArrowInk = Color(argb: 0xFFB4_B4B4)
/// The most memory (bytes) an animation's frames take in Preview: past it they are smaller.
private let playedMost = 256e6
/// Pinched in, an artwork being spread open shrinks no further than this.
private let pinchLeast: CGFloat = 0.6

/// How a thumbnail `shown` points wide is filtered: at 100% or more (each of its pixels a point or
/// larger) a square of its own colour, as the canvas shows them; smaller, smoothed.
func thumbnailInterpolation(_ picture: UIImage, shown: CGFloat) -> Image.Interpolation {
    shown >= picture.size.width * picture.scale * 0.999 ? .none : .high
}

private func thumbnail(of artwork: GalleryArtwork) async -> UIImage? {
    let path = (artwork.dir as NSString).appendingPathComponent("thumbnail.png")
    return await Task.detached(priority: .userInitiated) { UIImage(contentsOfFile: path) }.value
}

/// An artwork two fingers are spreading open: its thumbnail where it showed on the window, how far
/// the fingers have spread it and moved, and whether they have let go.
@Observable
final class PinchOpening {
    let artwork: GalleryArtwork
    let from: CGRect
    private(set) var scale: CGFloat = 1
    private(set) var moved = CGSize.zero
    private(set) var letGo = false
    /// The fingers' middle when they began.
    @ObservationIgnored private let start: CGPoint

    init(artwork: GalleryArtwork, from: CGRect, at start: CGPoint) {
        self.artwork = artwork
        self.from = from
        self.start = start
    }

    func follow(scale: CGFloat, at middle: CGPoint) {
        self.scale = max(scale, pinchLeast)
        moved = CGSize(width: middle.x - start.x, height: middle.y - start.y)
    }

    func end() {
        letGo = true
    }
}

/// An artwork spread open by two fingers: its picture follows them out of its thumbnail's place, the
/// gallery darkening behind it as it nears the whole screen; let go, it settles fitted over black
/// and `done` is told true (Preview takes over), or, spread too little, back into its place and
/// `done` is told false.
struct PinchOpeningOverlay: View {
    let opening: PinchOpening
    let done: (Bool) -> Void

    @State private var picture: UIImage?
    /// Where it was when the fingers let go, and how far it has settled from there.
    @State private var held: CGRect?
    @State private var settled: CGFloat = 0
    @State private var opens = false

    /// Following the fingers: grown about the thumbnail's middle and moved with them.
    private var following: CGRect {
        let from = opening.from
        let width = from.width * opening.scale
        let height = from.height * opening.scale
        return CGRect(
            x: from.midX + opening.moved.width - width / 2, y: from.midY + opening.moved.height - height / 2,
            width: width, height: height
        )
    }

    /// Fitted to the screen, as Preview shows it.
    private func full(_ screen: CGSize) -> CGRect {
        let from = opening.from
        let fit = min(screen.width / from.width, screen.height / from.height)
        let width = from.width * fit
        let height = from.height * fit
        return CGRect(x: (screen.width - width) / 2, y: (screen.height - height) / 2, width: width, height: height)
    }

    private func shown(_ screen: CGSize) -> CGRect {
        guard let held else { return following }
        let to = opens ? full(screen) : opening.from
        func along(_ a: CGFloat, _ b: CGFloat) -> CGFloat {
            a + (b - a) * settled
        }
        return CGRect(
            x: along(held.minX, to.minX), y: along(held.minY, to.minY),
            width: along(held.width, to.width), height: along(held.height, to.height)
        )
    }

    var body: some View {
        GeometryReader { place in
            let rect = shown(place.size)
            let reach = max(full(place.size).width - opening.from.width, 1)
            // how near the whole screen it is: the gallery darkens as far
            let near = min(max((rect.width - opening.from.width) / reach, 0), 1)
            ZStack(alignment: .topLeading) {
                Color.black.opacity(near)
                if let picture {
                    Image(uiImage: picture)
                        .resizable()
                        .interpolation(thumbnailInterpolation(picture, shown: rect.width))
                        .frame(width: rect.width, height: rect.height)
                        .clipShape(RoundedRectangle(cornerRadius: 8 * (1 - near), style: .circular))
                        .offset(x: rect.minX, y: rect.minY)
                }
            }
        }
        .ignoresSafeArea()
        .allowsHitTesting(false)
        .onChange(of: opening.letGo, initial: true) { _, gone in
            guard gone else { return }
            held = following
            opens = opening.scale >= Motion.artworkPinchOpen
            withAnimation(Motion.pinchSettle, completionCriteria: .logicallyComplete) {
                settled = 1
            } completion: {
                done(opens)
            }
        }
        .task(id: opening.artwork.dir) {
            picture = await thumbnail(of: opening.artwork)
        }
    }
}

/// Artworks one at a time over the whole screen, from the one at `start`: the picked ones, or the
/// gallery's from a thumbnail spread open; swiped through, closed by its cross (pinching in doesn't
/// close it, as measured).
struct ArtworkPreview: View {
    let list: [GalleryArtwork]
    let version: Int
    let onClose: () -> Void

    @Environment(\.displayScale) private var displayScale
    @State private var index: Int
    /// A tap shows (or hides) the close button and the arrows to the other artworks.
    @State private var controls: Bool
    @State private var picture: UIImage?
    /// An animated artwork's frame, playing over its thumbnail once its frames are read.
    @State private var frame: UIImage?

    init(list: [GalleryArtwork], version: Int, start: Int, onClose: @escaping () -> Void) {
        self.list = list
        self.version = version
        self.onClose = onClose
        _index = State(initialValue: start)
        #if DEBUG
            _controls = State(initialValue: ScreenCapture.galleryShows == "gallery-preview-controls")
        #else
            _controls = State(initialValue: false)
        #endif
    }

    private func show(_ artwork: GalleryArtwork, side: UInt32) async {
        picture = nil
        frame = nil
        let read = await thumbnail(of: artwork)
        guard !Task.isCancelled else { return }
        picture = read
        let played = await Task.detached(priority: .userInitiated) { readFrames(artwork.dir, side: side) }.value
        guard let played, !Task.isCancelled else { return }
        // as its settings say
        var at: UInt32 = 0
        var step: Int32 = 1
        while !Task.isCancelled {
            let slot = Int(at) < played.order.count ? Int(played.order[Int(at)]) : 0
            frame = played.pictures.indices.contains(slot) ? played.pictures[slot] : nil
            try? await Task.sleep(for: .milliseconds(1000 / max(played.fps, 1)))
            // round again, turned back, or (a single pass) staying on its last frame
            guard let next = animationNextStep(playback: played.playback, len: UInt32(played.order.count), at: at, step: step) else { break }
            at = next.at
            step = next.step
        }
    }

    private func swiped(_ drag: DragGesture.Value) {
        let moved = drag.translation.width
        guard abs(moved) > abs(drag.translation.height) else { return }
        if moved < -30, index < list.count - 1 {
            index += 1
        } else if moved > 30, index > 0 {
            index -= 1
        }
    }

    private var controlsShown: some View {
        ZStack {
            // a light disc with a black cross, 41 in from the top right
            ZStack {
                Circle().fill(previewClose).frame(width: 23.5, height: 23.5)
                PcIconView(PcIcons.cross, size: 15, color: .black)
            }
            .frame(width: 44, height: 44)
            .contentShape(Rectangle())
            .onTapGesture(perform: onClose)
            .padding(.trailing, 19)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
            PreviewArrow(left: true) {
                if index > 0 {
                    index -= 1
                }
            }
            .padding(.leading, 18)
            .frame(maxWidth: .infinity, alignment: .leading)
            PreviewArrow(left: false) {
                if index < list.count - 1 {
                    index += 1
                }
            }
            .padding(.trailing, 18)
            .frame(maxWidth: .infinity, alignment: .trailing)
        }
    }

    var body: some View {
        GeometryReader { place in
            ZStack {
                Color.black
                if let shown = frame ?? picture {
                    let ratio = shown.size.width / max(shown.size.height, 1)
                    Image(uiImage: shown)
                        .resizable()
                        .interpolation(thumbnailInterpolation(shown, shown: min(place.size.width, place.size.height * ratio)))
                        .scaledToFit()
                }
                if controls {
                    controlsShown
                        .transition(.opacity)
                }
            }
            .contentShape(Rectangle())
            .onTapGesture { withAnimation(Motion.easeInOut(0.2)) { controls.toggle() } }
            .gesture(DragGesture(minimumDistance: 10).onEnded(swiped))
            .task(id: "\(index)-\(version)") {
                guard list.indices.contains(index) else {
                    onClose()
                    return
                }
                await show(list[index], side: UInt32(max(place.size.width, place.size.height) * displayScale))
            }
        }
        .ignoresSafeArea()
    }
}

/// One of Preview's arrows: a 13.5 x 23.5 chevron 40 in from its side, in a 44 wide touch area.
private struct PreviewArrow: View {
    let left: Bool
    let onTap: () -> Void

    var body: some View {
        Canvas { context, size in
            // drawn a little larger than the 13.5 x 23.5 it shows (the stroke's edges soften)
            let width: CGFloat = 14.5
            let height: CGFloat = 25.5
            let stroke: CGFloat = 4
            let middle = size.width / 2
            let tip = left ? middle - width / 2 + stroke / 2 : middle + width / 2 - stroke / 2
            let back = left ? middle + width / 2 - stroke / 2 : middle - width / 2 + stroke / 2
            var path = Path()
            path.move(to: CGPoint(x: back, y: size.height / 2 - height / 2 + stroke / 2))
            path.addLine(to: CGPoint(x: tip, y: size.height / 2))
            path.addLine(to: CGPoint(x: back, y: size.height / 2 + height / 2 - stroke / 2))
            context.stroke(path, with: .color(previewArrowInk), style: StrokeStyle(lineWidth: stroke, lineJoin: .miter))
        }
        .frame(width: 44, height: 60)
        .contentShape(Rectangle())
        .onTapGesture(perform: onTap)
    }
}

/// An artwork's frames as read for Preview, and how they play.
private struct PlayedFrames: @unchecked Sendable {
    let pictures: [UIImage]
    let order: [UInt32]
    let fps: Int
    let playback: AnimationPlayback
}

/// The frames of the artwork in `dir` if it is animated, at no more than `side` pixels. Slow.
private func readFrames(_ dir: String, side: UInt32) -> PlayedFrames? {
    guard artworkPlays(dir: dir), let session = try? CanvasSession.open(dir: dir),
          let settings = session.animationSettings(), !settings.pages, session.drawWithoutWindow()
    else { return nil }
    let count = session.animationFrameCount()
    let fit = UInt32((playedMost / 4 / Double(max(count, 1))).squareRoot())
    let pictures = (0 ..< count).compactMap { i in
        session.animationFrame(index: i, transparent: false, maxSide: min(side, fit)).flatMap { CanvasModel.image($0) }
    }
    return PlayedFrames(pictures: pictures, order: session.animationSequence(), fps: Int(settings.fps), playback: settings.playback)
}

/// Hears two fingers on the window, over whatever they are on: SwiftUI's own pinch can't say where
/// the fingers' middle has gone, and a spread has to reach past the grid's touches without taking
/// them. `onBegin` is told where it began (on the window), and says whether to follow it.
struct PinchCatcher: UIViewRepresentable {
    let onBegin: (CGPoint) -> Bool
    let onChange: (CGFloat, CGPoint) -> Void
    let onEnd: () -> Void

    final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        var catcher: PinchCatcher
        private var following = false

        private(set) lazy var recognizer: UIPinchGestureRecognizer = {
            let made = UIPinchGestureRecognizer(target: self, action: #selector(Coordinator.pinched(_:)))
            made.cancelsTouchesInView = false
            made.delaysTouchesEnded = false
            made.delegate = self
            return made
        }()

        init(_ catcher: PinchCatcher) {
            self.catcher = catcher
        }

        func gestureRecognizer(_: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith _: UIGestureRecognizer) -> Bool {
            true
        }

        private func end() {
            guard following else { return }
            following = false
            catcher.onEnd()
        }

        @objc private func pinched(_ pinch: UIPinchGestureRecognizer) {
            switch pinch.state {
            case .began:
                pinch.scale = 1
                following = catcher.onBegin(Interface.shared.point(pinch.location(in: nil)))
            case .changed:
                // a finger up ends it, though the recognizer waits for the last
                if pinch.numberOfTouches < 2 {
                    end()
                } else if following {
                    catcher.onChange(pinch.scale, Interface.shared.point(pinch.location(in: nil)))
                }
            case .ended, .cancelled, .failed:
                end()
            default:
                break
            }
        }
    }

    /// Puts the recognizer on the window it comes to.
    final class Host: UIView {
        var recognizer: UIPinchGestureRecognizer?

        override func didMoveToWindow() {
            super.didMoveToWindow()
            guard let recognizer else { return }
            recognizer.view?.removeGestureRecognizer(recognizer)
            window?.addGestureRecognizer(recognizer)
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    func makeUIView(context: Context) -> Host {
        let host = Host()
        host.isUserInteractionEnabled = false
        host.recognizer = context.coordinator.recognizer
        return host
    }

    func updateUIView(_: Host, context: Context) {
        context.coordinator.catcher = self
    }

    static func dismantleUIView(_ host: Host, coordinator _: Coordinator) {
        if let recognizer = host.recognizer {
            recognizer.view?.removeGestureRecognizer(recognizer)
        }
    }
}
