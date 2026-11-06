import ImageIO
import PhotosUI
import SwiftUI
import UIKit

// the dark bars are glass, like the panels: what the picture has under them shows through, blurred
private var barFill: Color { Look.shared.light ? Color(argb: 0xFFED_EDED) : Color(argb: 0xDE1E_1E1E) }
private var pictureFill: Color { Look.shared.light ? Color(argb: 0xFFF0_F0F0) : Color(argb: 0xFF33_3333) }
private var grabLine: Color { Look.shared.light ? Color(argb: 0xFF90_9090) : Color(argb: 0x80BF_BFBF) }
private var titleInk: Color { Look.shared.light ? Color(argb: 0xFF3A_3A3A) : Color(argb: 0xFFC5_C5C5) }
private var closeFill: Color { Look.shared.light ? Color(argb: 0xFFFA_FAFA) : Color(argb: 0x9418_1818) }
private var closeEdge: Color { Look.shared.light ? Color(argb: 0xFFE0_E0E0) : Color(argb: 0x8051_5151) }
private var closeInk: Color { Look.shared.light ? Color(argb: 0xFF59_5959) : Color(argb: 0xFFA5_A5A5) }
private var segmentTrack: Color { Look.shared.light ? Color(argb: 0xFFD7_D7D7) : Color(argb: 0x17FF_FFFF) }
private var importFill: Color { Look.shared.light ? Color(argb: 0xFFD7_D7D7) : Color(argb: 0xFF4D_4D4D) }
private var muted: Color { Look.shared.light ? Color(argb: 0xFF3B_3B3B) : Color(argb: 0xFFAA_AAAA) }
private let topBarHeight: CGFloat = 64
private let bottomBarHeight: CGFloat = 45
private let windowCorner: CGFloat = 24
private let handleSide: CGFloat = 60
/// How far down from the window's top a drag moves it, the bars shown or not.
private let grabHeight: CGFloat = 16
private let firstSize = CGSize(width: 350, height: 423)
/// How far above the screen's foot it starts.
private let firstBottom: CGFloat = 50
private let leastSide: CGFloat = 150
private let mostSide: CGFloat = 700
/// How many pixels the canvas is read at for the window, and how long between reads.
private let canvasPixels: UInt32 = 700
private let readEvery = 0.3
/// The longest side a picture imported keeps.
private let mostPicture = 4096
/// The window the eyedropper shows, points across.
private let pickWindow: CGFloat = 54

/// What the window shows.
enum ReferenceMode {
    case canvas, image, face
}

/// The reference window: where it is, what it shows, and how each picture sits in it.
@Observable
final class ReferenceTool {
    private unowned let model: CanvasModel

    private(set) var on = false
    private(set) var mode = ReferenceMode.canvas
    /// The whole canvas as it is, kept up to date while the window shows it.
    private(set) var canvas: UIImage?
    /// `canvas` is the canvas at its own size (not made smaller): its pixels are the canvas's.
    private(set) var canvasWhole = false
    private(set) var image: UIImage?
    /// How the window shows the canvas and a picture imported, each zoomed and moved on its own.
    private(set) var canvasView = ReferenceView(zoom: 1, x: 0, y: 0)
    private(set) var imageView = ReferenceView(zoom: 1, x: 0, y: 0)
    /// Where the window is, none until it is first placed; and the size of its picture.
    private(set) var offset: CGPoint?
    private(set) var size = firstSize
    /// The window's bars are slid out of the way.
    private(set) var barsHidden = false

    @ObservationIgnored private var reading = false
    @ObservationIgnored private var stale = false

    init(model: CanvasModel) {
        self.model = model
    }

    var shown: UIImage? {
        switch mode {
        case .canvas: canvas
        case .image: image
        case .face: nil
        }
    }

    var view: ReferenceView {
        mode == .image ? imageView : canvasView
    }

    private func set(_ next: ReferenceView) {
        if mode == .image {
            imageView = next
        } else {
            canvasView = next
        }
    }

    func setOn(_ on: Bool) {
        withAnimation(Motion.referenceFade) { self.on = on }
        if on {
            refresh()
        }
    }

    func setMode(_ next: ReferenceMode) {
        mode = next
        refresh()
    }

    /// The window's first place: in the middle, near the bottom of the screen.
    func place(on screen: CGSize, tall: CGFloat) {
        guard offset == nil else { return }
        offset = CGPoint(x: (screen.width - size.width) / 2, y: max(screen.height - firstBottom - tall, 0))
    }

    func move(by delta: CGSize) {
        guard let at = offset else { return }
        offset = CGPoint(x: at.x + delta.width, y: at.y + delta.height)
    }

    /// A drag on a size handle; from the left one the window's left edge moves.
    func resize(by delta: CGSize, fromLeft: Bool) {
        let old = size
        let width = min(max(old.width + (fromLeft ? -delta.width : delta.width), leastSide), mostSide)
        let height = min(max(old.height + delta.height, leastSide), mostSide)
        if fromLeft, let at = offset {
            offset = CGPoint(x: at.x + old.width - width, y: at.y)
        }
        size = CGSize(width: width, height: height)
    }

    private static func decode(_ data: Data) -> UIImage? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: mostPicture,
        ]
        return CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary).map { UIImage(cgImage: $0) }
    }

    func importPicture(_ data: Data?) {
        guard let picture = data.flatMap(Self.decode) else {
            model.showNotice("This picture can't be opened.", warning: true)
            return
        }
        image = picture
        imageView = ReferenceView(zoom: 1, x: 0, y: 0)
    }

    func clear() {
        image = nil
        imageView = ReferenceView(zoom: 1, x: 0, y: 0)
    }

    /// A tap on the window's picture: its bars slide away, or back.
    func toggleBars() {
        withAnimation(Motion.bars) { barsHidden.toggle() }
    }

    /// Two taps on the window's picture: it fits the window again, the bars out of the way.
    func fit() {
        set(ReferenceView(zoom: 1, x: 0, y: 0))
        withAnimation(Motion.bars) { barsHidden = true }
    }

    /// Fingers on the window's picture moved from `from` to `to` (their middle), `spread` times as
    /// far apart: the picture zooms and moves with them.
    func pinch(_ sizes: ReferenceSizes, from: CGPoint, to: CGPoint, spread: CGFloat) {
        set(referenceViewPinched(
            view: view, sizes: sizes, fromX: Float(from.x), fromY: Float(from.y),
            toX: Float(to.x), toY: Float(to.y), spread: Float(spread)
        ))
    }

    /// The colour of one pixel of `image`.
    private static func rgb(of image: CGImage, x: Int, y: Int) -> UInt32? {
        guard let one = image.cropping(to: CGRect(x: x, y: y, width: 1, height: 1)) else { return nil }
        var pixel: [UInt8] = [0, 0, 0, 0]
        let space = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
        let drawn: Bool = pixel.withUnsafeMutableBytes { bytes in
            guard let context = CGContext(
                data: bytes.baseAddress, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
                space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return false }
            context.draw(one, in: CGRect(x: 0, y: 0, width: 1, height: 1))
            return true
        }
        guard drawn else { return nil }
        return UInt32(pixel[0]) << 16 | UInt32(pixel[1]) << 8 | UInt32(pixel[2])
    }

    /// The eyedropper over the picture at `place` (`referencePicturePoint`'s: its point, and how many
    /// of its pixels a point spans), the finger at `at` on the screen: the colour there and what is
    /// around it.
    func pick(_ picture: UIImage, _ place: [Float], at: CGPoint) {
        guard let image = picture.cgImage, place.count >= 3 else { return }
        let x = min(max(Int(place[0]), 0), image.width - 1)
        let y = min(max(Int(place[1]), 0), image.height - 1)
        // as much of the picture as the loupe's window shows, around the point
        let n = min(max(Int((pickWindow * CGFloat(place[2])).rounded()), 1), min(image.width, image.height))
        let left = min(max(x - n / 2, 0), image.width - n)
        let top = min(max(y - n / 2, 0), image.height - n)
        let around = image.cropping(to: CGRect(x: left, y: top, width: n, height: n)).map { UIImage(cgImage: $0) }
        model.pickFromReference(around, rgb: Self.rgb(of: image, x: x, y: y), at: at)
    }

    /// Reads the canvas again for the window, at most every so often while changes come.
    func refresh() {
        guard on, mode == .canvas else { return }
        if reading {
            stale = true
            return
        }
        reading = true
        read()
    }

    private func read() {
        stale = false
        let session = model.session
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let read = session.canvasThumbnail(maxSide: canvasPixels)
            let picture = read.flatMap { CanvasModel.image($0) }
            let whole = read?.width == session.info().width
            DispatchQueue.main.async {
                guard let self else { return }
                if let picture {
                    self.canvas = picture
                    self.canvasWhole = whole
                }
                DispatchQueue.main.asyncAfter(deadline: .now() + readEvery) { [weak self] in
                    guard let self else { return }
                    if self.stale {
                        self.read()
                    } else {
                        self.reading = false
                    }
                }
            }
        }
    }
}

/// The reference window: the whole canvas as it is, or a picture to work from, floating over the
/// canvas. Its grab line drags it; the round button closes it; handles over its bottom corners size
/// it. Its bars lie over the picture, and a tap on it slides them away and back.
struct ReferenceWindow: View {
    let model: CanvasModel
    let screen: CGSize

    @State private var choosing = false
    @State private var photo: PhotosPickerItem?
    /// How far the drag under way has been told.
    @State private var dragged = CGSize.zero

    private var tool: ReferenceTool {
        model.reference
    }

    /// The picture's area and `picture`'s size, for the core's view of it.
    private static func sizes(_ area: CGSize, _ picture: UIImage) -> ReferenceSizes {
        ReferenceSizes(
            windowWidth: Float(area.width), windowHeight: Float(area.height),
            pictureWidth: Float(picture.size.width * picture.scale), pictureHeight: Float(picture.size.height * picture.scale)
        )
    }

    /// What a drag has moved since last told, in the interface's points.
    private func step(_ drag: DragGesture.Value) -> CGSize {
        let k = Interface.shared.scale
        let now = CGSize(width: drag.translation.width / k, height: drag.translation.height / k)
        let delta = CGSize(width: now.width - dragged.width, height: now.height - dragged.height)
        dragged = now
        return delta
    }

    private var picture: some View {
        let shown = tool.shown
        let view = tool.view
        let crispable = tool.mode == .canvas && tool.canvasWhole
        return Canvas { context, size in
            guard let shown else { return }
            let r = referencePictureRect(view: view, sizes: Self.sizes(size, shown))
            guard r.count >= 4 else { return }
            let rect = CGRect(x: CGFloat(r[0]), y: CGFloat(r[1]), width: CGFloat(r[2]), height: CGFloat(r[3]))
            // the canvas's pixels crisp from 100%, as on the canvas
            let crisp = crispable && rect.width >= shown.size.width * shown.scale * 0.999
            context.draw(Image(uiImage: shown).interpolation(crisp ? .none : .low), in: rect)
        }
    }

    private func touches(at: CGPoint) -> some View {
        ReferenceTouches(
            hold: Double(Gestures.shared.delay(.eyedropper)),
            canPick: tool.shown != nil,
            onTap: { tool.toggleBars() },
            onDoubleTap: { tool.fit() },
            onMove: { from, to, spread, area in
                if let shown = tool.shown {
                    tool.pinch(Self.sizes(area, shown), from: from, to: to, spread: spread)
                }
            },
            onPick: { point, area in
                guard let shown = tool.shown else { return }
                let place = referencePicturePoint(
                    view: tool.view, sizes: Self.sizes(area, shown), x: Float(point.x), y: Float(point.y)
                )
                if let place {
                    tool.pick(shown, place, at: CGPoint(x: at.x + point.x, y: at.y + point.y))
                }
            },
            onPickEnd: {
                if model.picking != nil {
                    model.finishPick()
                }
            }
        )
    }

    @ViewBuilder private var empty: some View {
        switch tool.mode {
        case .canvas:
            EmptyView()
        case .image:
            if tool.image == nil {
                Text(verbatim: "Import Image")
                    .font(.system(size: 16))
                    .foregroundStyle(muted)
                    .frame(width: 180, height: 60)
                    .background(Capsule().fill(importFill))
                    .contentShape(Capsule())
                    .onTapGesture { choosing = true }
            }
        case .face:
            Text(verbatim: "Not available on this device")
                .font(.system(size: 14))
                .foregroundStyle(muted)
                .allowsHitTesting(false)
        }
    }

    private var glass: some View {
        ZStack {
            Rectangle().fill(.ultraThinMaterial)
            barFill
        }
    }

    private func word(_ label: String, action: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 15))
            .foregroundStyle(Pc.accent)
            .padding(6)
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
    }

    private var topBar: some View {
        ZStack(alignment: .topLeading) {
            Text(verbatim: "Reference")
                .font(.system(size: 22, weight: .bold))
                .foregroundStyle(titleInk)
                .lineLimit(1)
                .padding(.leading, 20)
                .padding(.top, 10)
            HStack(spacing: 0) {
                Spacer(minLength: 0)
                if tool.mode == .image, tool.image != nil {
                    word("Clear") { tool.clear() }
                    word("Import") { choosing = true }
                }
                PcIconView(PcIcons.cross, size: 20, color: closeInk)
                    .frame(width: 43, height: 43)
                    .background(Circle().fill(closeFill))
                    .overlay(Circle().strokeBorder(closeEdge, lineWidth: 1))
                    .contentShape(Circle())
                    .onTapGesture { tool.setOn(false) }
            }
            .padding(.trailing, 10)
            .frame(maxHeight: .infinity)
        }
        .frame(height: topBarHeight)
        .background(glass)
        // touches here go no further: not to the picture under it
        .contentShape(Rectangle())
        .onTapGesture {}
    }

    private func segment(_ mode: ReferenceMode, _ label: String) -> some View {
        let on = tool.mode == mode
        return Text(verbatim: label)
            .font(.system(size: 15))
            .foregroundStyle(on ? Color.white : titleInk)
            .lineLimit(1)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Capsule().fill(on ? Pc.accent : Color.clear))
            .contentShape(Capsule())
            .onTapGesture { tool.setMode(mode) }
    }

    private var bottomBar: some View {
        HStack(spacing: 11) {
            segment(.canvas, "Canvas")
            segment(.image, "Image")
            segment(.face, "Face")
        }
        .padding(2)
        .frame(width: 270, height: 35)
        .background(Capsule().fill(segmentTrack))
        .frame(maxWidth: .infinity)
        .frame(height: bottomBarHeight)
        .background(glass)
        .contentShape(Rectangle())
        .onTapGesture {}
    }

    /// The grab line stays when the bars go, and dragging by it moves the window.
    private var grab: some View {
        Capsule()
            .fill(grabLine)
            .frame(width: 40, height: 4)
            .padding(.top, 4)
            .frame(maxWidth: .infinity)
            .frame(height: grabHeight, alignment: .top)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 1, coordinateSpace: .global)
                    .onChanged { tool.move(by: step($0)) }
                    .onEnded { _ in dragged = .zero }
            )
    }

    /// A size handle, unseen, centred 12 in from the window's bottom corner and 15 up.
    private func handle(left: Bool, width: CGFloat, tall: CGFloat) -> some View {
        Color.clear
            .frame(width: handleSide, height: handleSide)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 1, coordinateSpace: .global)
                    .onChanged { tool.resize(by: step($0), fromLeft: left) }
                    .onEnded { _ in dragged = .zero }
            )
            .offset(x: left ? 12 - handleSide / 2 : width - 12 - handleSide / 2, y: tall - 15 - handleSide / 2)
    }

    var body: some View {
        let width = tool.size.width
        let tall = topBarHeight + tool.size.height + bottomBarHeight
        let hidden = tool.barsHidden
        let at = tool.offset ?? .zero
        let shape = RoundedRectangle(cornerRadius: windowCorner, style: .circular)
        ZStack(alignment: .topLeading) {
            ZStack(alignment: .top) {
                pictureFill
                picture
                touches(at: at)
                empty
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                topBar
                    .offset(y: hidden ? -topBarHeight : 0)
                    .allowsHitTesting(!hidden)
                bottomBar
                    .offset(y: hidden ? bottomBarHeight + 1 : 0)
                    .frame(maxHeight: .infinity, alignment: .bottom)
                    .allowsHitTesting(!hidden)
                grab
            }
            .frame(width: width, height: tall)
            .clipShape(shape)
            .background(shape.fill(Color.black).shadow(color: Pc.popoverShadow, radius: 18))
            handle(left: true, width: width, tall: tall)
            handle(left: false, width: width, tall: tall)
        }
        .frame(width: width, height: tall, alignment: .topLeading)
        .offset(x: at.x, y: at.y)
        .onAppear { tool.place(on: screen, tall: tall) }
        .photosPicker(isPresented: $choosing, selection: $photo, matching: .images)
        .onChange(of: photo) { _, item in
            guard let item else { return }
            photo = nil
            Task { @MainActor in
                tool.importPicture(try? await item.loadTransferable(type: Data.self))
            }
        }
    }
}

/// Touches on the window's picture: a tap, then two; a finger moves it and two zoom and move it; a
/// finger held still for the eyedropper's delay picks a colour from it, following the finger until
/// it lets go. They never reach the canvas under the window.
private struct ReferenceTouches: UIViewRepresentable {
    let hold: Double
    let canPick: Bool
    let onTap: () -> Void
    let onDoubleTap: () -> Void
    /// Their middle from, to, how many times as far apart, and the picture's area.
    let onMove: (CGPoint, CGPoint, CGFloat, CGSize) -> Void
    let onPick: (CGPoint, CGSize) -> Void
    let onPickEnd: () -> Void

    final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        var touches: ReferenceTouches
        let view = UIView()
        private let held = UILongPressGestureRecognizer()
        private let pinch = UIPinchGestureRecognizer()
        private let pan = UIPanGestureRecognizer()
        private var last = CGPoint.zero
        private var fingers = 0

        init(_ touches: ReferenceTouches) {
            self.touches = touches
            super.init()
            let two = UITapGestureRecognizer(target: self, action: #selector(Coordinator.tappedTwice))
            two.numberOfTapsRequired = 2
            let one = UITapGestureRecognizer(target: self, action: #selector(Coordinator.tapped))
            // it waits to see whether a second comes
            one.require(toFail: two)
            pan.addTarget(self, action: #selector(Coordinator.moved(_:)))
            pinch.addTarget(self, action: #selector(Coordinator.moved(_:)))
            held.addTarget(self, action: #selector(Coordinator.picked(_:)))
            pan.delegate = self
            pinch.delegate = self
            for recognizer in [two, one, pan, pinch, held] as [UIGestureRecognizer] {
                view.addGestureRecognizer(recognizer)
            }
        }

        func update(_ next: ReferenceTouches) {
            touches = next
            held.minimumPressDuration = max(next.hold, 0.05)
            held.isEnabled = next.canPick
        }

        /// A pan and a pinch go together; nothing else does.
        func gestureRecognizer(_ a: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith b: UIGestureRecognizer) -> Bool {
            (a === pan && b === pinch) || (a === pinch && b === pan)
        }

        @objc func tapped() {
            touches.onTap()
        }

        @objc func tappedTwice() {
            touches.onDoubleTap()
        }

        @objc func moved(_ recognizer: UIGestureRecognizer) {
            let at = recognizer.location(in: view)
            switch recognizer.state {
            case .began:
                last = at
                fingers = recognizer.numberOfTouches
            case .changed:
                // a finger came or went: their middle jumps, which nothing moved
                if recognizer.numberOfTouches != fingers {
                    fingers = recognizer.numberOfTouches
                    last = at
                }
                let spread = pinch.state == .changed ? pinch.scale : 1
                pinch.scale = 1
                touches.onMove(last, at, spread, view.bounds.size)
                last = at
            default:
                break
            }
        }

        @objc func picked(_ recognizer: UILongPressGestureRecognizer) {
            switch recognizer.state {
            case .began, .changed:
                touches.onPick(recognizer.location(in: view), view.bounds.size)
            case .ended, .cancelled, .failed:
                touches.onPickEnd()
            default:
                break
            }
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    func makeUIView(context: Context) -> UIView {
        context.coordinator.update(self)
        return context.coordinator.view
    }

    func updateUIView(_: UIView, context: Context) {
        context.coordinator.update(self)
    }
}
