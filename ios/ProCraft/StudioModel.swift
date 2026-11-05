import Observation
import SwiftUI
import UIKit

/// The colours the drawing pad can paint in, in the order its menu shows them.
let padColors: [UInt32] = [0xF83045, 0xFCBD03, 0x85D921, 0x2BB5E6, 0x4956A5, 0xDA3D92, 0xFFFFFF, 0x000000]
private let padWhite = 6
private let padBlack = 7
/// The pad's grey, 0..1; with the light interface it is light and paints in black.
private let padBack: Float = 31.0 / 255
private let padBackLight: Float = 243.0 / 255
/// How wide, in pixels, a source's image is on its row and in the landscape studio's square.
private let sourceRowPixels: CGFloat = 88
private let sourceBoxPixels: CGFloat = 566
/// How long after a control stops moving its sample stroke is drawn again, seconds.
private let previewDelay = 0.06

/// Who made the studio's brush, and when its reset point was made: its About page.
struct BrushAbout {
    let author: String
    let picture: UIImage?
    let signature: UIImage?
    let resetMade: Int64?
}

/// A shipped shape or grain image, as the source library shows it.
struct LibrarySource: Sendable {
    let name: String
    let pixels: Data
    let image: UIImage?
}

/// The brush studio: the brush being edited, as its controls read it, and its drawing pad, a small
/// canvas of its own that paints with that brush.
@Observable
final class StudioModel {
    @ObservationIgnored private unowned let model: CanvasModel
    private var session: CanvasSession { model.session }

    private(set) var isOpen = false
    /// The brush's name.
    private(set) var name = ""
    /// Its controls' values, by key.
    private var values: [String: Float] = [:]
    /// Its pressure curve.
    private(set) var curve: [CGPoint] = [.zero, CGPoint(x: 1, y: 1)]
    /// Its sample stroke, for the Preview page.
    private(set) var preview: UIImage?
    private(set) var about: BrushAbout?
    /// The images its sources use (white on black), by kind, for their rows; none for the round
    /// tip, no grain or a material's amount all over.
    private(set) var sources: [SourceKind: UIImage] = [:]
    /// The same images as big as the landscape studio's square shows them.
    private(set) var sourcesBig: [SourceKind: UIImage] = [:]
    /// The editor open for the brush's source of this kind, if one is.
    var sourceEditing: SourceKind?
    /// The pad's menu is open, and the middle of the button that opens it.
    var padMenuOpen = false
    var padButtonCentre: CGFloat = 0
    /// The pad has drawn: until then a cover hides it.
    var padShown = false
    /// The colour the pad paints in, a place in `padColors`.
    private(set) var padColor = padWhite

    /// The drawing pad's canvas, while the studio is open and the pad has been laid out.
    @ObservationIgnored private(set) var pad: CanvasSession?
    /// The pad is shown as a sphere with the brush's materials.
    @ObservationIgnored private var padSphere = false
    /// The studio is open on a brush just made: cancelling throws it away.
    @ObservationIgnored var brandNew = false
    @ObservationIgnored private var previewJob: DispatchWorkItem?

    /// The modes a brush and its burnt edges blend by, and those a brush's grain does.
    @ObservationIgnored let brushModes = brushBlendModes()
    @ObservationIgnored let grainModes = grainBlendModes()

    init(model: CanvasModel) {
        self.model = model
    }

    func value(_ key: String) -> Float {
        values[key] ?? 0
    }

    // MARK: opening and closing

    /// Opens on the current tool's brush. The brush library stays open under it, so Cancel and Done
    /// go back to it.
    func open() {
        if model.panel != .brushes {
            model.closePanels()
        }
        name = session.studioBegin()
        // each visit's pad paints in white, black with the light interface
        padColor = AppSettings.shared.lightInterface ? padBlack : padWhite
        read()
        withAnimation(Motion.studioIn) {
            isOpen = true
        }
    }

    private func read() {
        let keys = studioKeys
        for (key, value) in zip(keys, session.studioSettings(keys: keys)) {
            values[key] = value
        }
        curve = Self.points(session.studioCurve())
        readSources()
        readAbout()
        refreshPreview()
    }

    /// Closes: `keep` keeps the edits, otherwise the brush goes back to how it opened.
    func close(keep: Bool) {
        session.studioEnd(keep: keep)
        // a new brush cancelled is thrown away
        if !keep, brandNew, let b = model.currentBrush {
            _ = session.deleteBrush(set: b.set, index: b.index)
        }
        brandNew = false
        withAnimation(Motion.studioOut) {
            isOpen = false
        }
        padShown = false
        padMenuOpen = false
        sourceEditing = nil
        // the pad's canvas goes with the studio
        pad?.detachWindow()
        pad = nil
        preview = nil
        model.libraryChanged()
        // brushes read while the studio was open join the library now
        model.addBrushesRead()
    }

    // MARK: the controls

    func set(_ key: String, _ value: Float) {
        values[key] = value
        session.setStudioSetting(key: key, value: value)
        // the spacing sets how big the brush may be, so where the size controls sit
        if key == "spacing" {
            let keys = ["max_size", "min_size", "most_size"]
            for (key, value) in zip(keys, session.studioSettings(keys: keys)) {
                values[key] = value
            }
        }
        refreshPreview()
        syncPad()
    }

    /// Control `key`'s own pressure curve (none while it has none).
    func sliderCurve(_ key: String) -> [CGPoint]? {
        let flat = session.studioSliderCurve(key: key)
        return flat.count >= 4 ? Self.points(flat) : nil
    }

    /// Gives control `key` its own pressure curve through `points`, or takes it away (none).
    func setSliderCurve(_ key: String, _ points: [CGPoint]?) {
        session.setStudioSliderCurve(key: key, points: Self.flat(points ?? []))
        refreshPreview()
        syncPad()
    }

    func changeCurve(_ points: [CGPoint]) {
        curve = points
        session.setStudioCurve(points: Self.flat(points))
        refreshPreview()
        syncPad()
    }

    /// Every control back to how the brush ships.
    func reset() {
        session.studioReset()
        read()
        syncPad()
        padMenuOpen = false
    }

    /// Sets the brush aside as it is now, for resetting it to go back to.
    func newResetPoint() {
        session.studioNewResetPoint()
        readAbout()
    }

    private func readAbout() {
        let a = session.studioAbout()
        about = BrushAbout(
            author: a.author,
            picture: a.picture.flatMap { UIImage(data: $0) },
            signature: a.signature.flatMap { UIImage(data: $0) },
            resetMade: a.resetMade
        )
    }

    /// Draws the brush's sample stroke again, a moment after the last change.
    private func refreshPreview() {
        guard let b = model.currentBrush else { return }
        previewJob?.cancel()
        let session = session
        let job = DispatchWorkItem { [weak self] in
            DispatchQueue.global(qos: .userInitiated).async {
                let made = session.brushPreview(set: b.set, index: b.index, width: 1060, height: 324)
                let image = made.flatMap { CanvasModel.image($0) }
                DispatchQueue.main.async {
                    guard let self, self.isOpen, let image else { return }
                    self.preview = image
                }
            }
        }
        previewJob = job
        DispatchQueue.main.asyncAfter(deadline: .now() + previewDelay, execute: job)
    }

    private static func points(_ flat: [Float]) -> [CGPoint] {
        stride(from: 0, to: flat.count - 1, by: 2).map { CGPoint(x: CGFloat(flat[$0]), y: CGFloat(flat[$0 + 1])) }
    }

    private static func flat(_ points: [CGPoint]) -> [Float] {
        points.flatMap { [Float($0.x), Float($0.y)] }
    }

    // MARK: the sources

    private func readSources() {
        var row: [SourceKind: UIImage] = [:]
        var big: [SourceKind: UIImage] = [:]
        for kind in SourceImages.kinds {
            guard let image = SourceImages.image(session.studioSource(kind: kind), shape: kind == .shape) else { continue }
            row[kind] = SourceImages.small(image, side: sourceRowPixels)
            big[kind] = SourceImages.small(image, side: sourceBoxPixels)
        }
        sources = row
        sourcesBig = big
    }

    /// The image the brush's `kind` of source uses now: square greyscale, white = paint; empty for
    /// the round tip, no grain or the amount all over.
    func sourcePixels(_ kind: SourceKind) -> Data {
        session.studioSource(kind: kind)
    }

    /// The editor's tick: the brush's `kind` of source becomes `pixels`, or shipped image `shipped`.
    func keepSource(_ kind: SourceKind, pixels: Data, shipped: Int?) {
        var changed = false
        if let shipped {
            changed = session.setStudioShippedSource(kind: kind, index: UInt32(shipped))
        } else if !pixels.isEmpty {
            changed = session.setStudioSource(kind: kind, pixels: pixels)
        }
        sourceEditing = nil
        if changed {
            readSources()
            refreshPreview()
            syncPad()
        }
    }

    /// A picture from elsewhere as a source image; none if it can't be read.
    func source(fromPicture data: Data?) -> Data? {
        guard let data, let picture = UIImage(data: data), let made = SourceImages.source(from: picture) else {
            model.showNotice("This picture can't be opened.", warning: true)
            return nil
        }
        return made
    }

    /// A picture on the clipboard as a source image; none if there isn't one.
    func pastedSource() -> Data? {
        guard let picture = UIPasteboard.general.image else {
            model.showNotice("There's no picture to paste.", warning: true)
            return nil
        }
        guard let made = SourceImages.source(from: picture) else {
            model.showNotice("This picture can't be opened.", warning: true)
            return nil
        }
        return made
    }

    // MARK: the drawing pad

    /// The pad's canvas is made: dark, painting with the brush being edited, keeping its strokes.
    func startPad(_ p: CanvasSession) {
        pad = p
        let back = AppSettings.shared.lightInterface ? padBackLight : padBack
        _ = p.setBackgroundColor(r: back, g: back, b: back, done: true)
        p.keepStrokes()
        syncPad()
    }

    /// The pad has drawn its first frame: it shows the brush's sample scribble, flat or on the
    /// sphere (drawn again here: a sphere asked for before now had nothing to draw on).
    func padReady(_ p: CanvasSession) {
        guard pad === p else { return }
        p.showSphere(shown: padSphere)
        p.drawSampleScribble()
    }

    /// The pad shown as a sphere with the brush's materials (the Materials page, or the 3D preview
    /// on).
    func showPadSphere(_ shown: Bool) {
        padSphere = shown
        pad?.showSphere(shown: shown)
    }

    /// The pad paints with the brush as it is now, and what is on it is drawn again that way, on
    /// the core's own thread: a slider dragged doesn't wait for it.
    private func syncPad() {
        guard let pad else { return }
        let c = padColors[min(max(padColor, 0), padColors.count - 1)]
        pad.syncPad(
            studio: session,
            r: Float((c >> 16) & 0xFF) / 255,
            g: Float((c >> 8) & 0xFF) / 255,
            b: Float(c & 0xFF) / 255
        )
    }

    func choosePadColor(_ i: Int) {
        padColor = i
        syncPad()
    }

    func clearPad() {
        pad?.forgetStrokes()
        pad?.clearLayer()
        padMenuOpen = false
    }
}

/// The studio's source images: square greyscale, white = paint.
enum SourceImages {
    static let kinds: [SourceKind] = [.shape, .grain, .metallic, .roughness]

    /// A source as the editors show it: a `shape` white over what is behind, a grain as its greys;
    /// none if it is empty.
    static func image(_ pixels: Data, shape: Bool) -> UIImage? {
        let n = Int(Double(pixels.count).squareRoot())
        guard n > 0, n * n == pixels.count else { return nil }
        let space = shape ? CGColorSpaceCreateDeviceRGB() : CGColorSpaceCreateDeviceGray()
        var bytes = pixels
        if shape {
            // white at the image's strength, as a premultiplied image has it
            var rgba = [UInt8](repeating: 0, count: n * n * 4)
            for (i, v) in pixels.enumerated() {
                rgba[i * 4] = v
                rgba[i * 4 + 1] = v
                rgba[i * 4 + 2] = v
                rgba[i * 4 + 3] = v
            }
            bytes = Data(rgba)
        }
        guard let provider = CGDataProvider(data: bytes as CFData),
              let image = CGImage(
                  width: n, height: n, bitsPerComponent: 8, bitsPerPixel: shape ? 32 : 8, bytesPerRow: shape ? n * 4 : n,
                  space: space,
                  bitmapInfo: CGBitmapInfo(rawValue: shape ? CGImageAlphaInfo.premultipliedLast.rawValue : CGImageAlphaInfo.none.rawValue),
                  provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent
              )
        else { return nil }
        return UIImage(cgImage: image)
    }

    /// `image` shrunk by halves to about `side` pixels across, each step averaging, so fine detail
    /// evens out.
    static func small(_ image: UIImage, side: CGFloat) -> UIImage {
        var out = image
        let format = UIGraphicsImageRendererFormat.default()
        format.scale = 1
        format.opaque = false
        while out.size.width >= side * 2 {
            let from = out
            let half = CGSize(width: (from.size.width / 2).rounded(.down), height: (from.size.height / 2).rounded(.down))
            out = UIGraphicsImageRenderer(size: half, format: format).image { drawn in
                drawn.cgContext.interpolationQuality = .medium
                from.draw(in: CGRect(origin: .zero, size: half))
            }
        }
        return out
    }

    /// `picture` no more than 2048 pixels across, fitted into the square on black by its lightness
    /// (the shared core's). None if it can't be drawn.
    static func source(from picture: UIImage) -> Data? {
        let across = picture.size.width * picture.scale
        let down = picture.size.height * picture.scale
        guard across > 0, down > 0 else { return nil }
        let shrink = min(1, 2048 / max(across, down))
        let w = max(Int(across * shrink), 1)
        let h = max(Int(down * shrink), 1)
        var rgba = [UInt8](repeating: 0, count: w * h * 4)
        let drawn: Bool = rgba.withUnsafeMutableBytes { bytes in
            guard let context = CGContext(
                data: bytes.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return false }
            // the picture's own way up, its top row first
            context.translateBy(x: 0, y: CGFloat(h))
            context.scaleBy(x: 1, y: -1)
            UIGraphicsPushContext(context)
            picture.draw(in: CGRect(x: 0, y: 0, width: w, height: h))
            UIGraphicsPopContext()
            return true
        }
        guard drawn else { return nil }
        // straight, as the core takes it
        for i in stride(from: 0, to: rgba.count, by: 4) {
            let a = Int(rgba[i + 3])
            if a > 0, a < 255 {
                for c in 0 ..< 3 {
                    rgba[i + c] = UInt8(min(255, Int(rgba[i + c]) * 255 / a))
                }
            }
        }
        return sourceFromPicture(rgba: Data(rgba), width: UInt32(w), height: UInt32(h))
    }

    /// Library image `index`'s place among the shipped images: none for the grains' Blank, which
    /// isn't one.
    static func shipped(grain: Bool, index: Int) -> Int? {
        guard grain else { return index }
        return index >= 1 ? index - 1 : nil
    }

    /// The shipped images, read once each: `grain`'s start with a blank one.
    static func library(grain: Bool) -> [LibrarySource] {
        libraryLock.lock()
        defer { libraryLock.unlock() }
        if let kept = libraries[grain] {
            return kept
        }
        let made = sourceLibrary(grain: grain).map {
            LibrarySource(name: $0.name, pixels: $0.pixels, image: image($0.pixels, shape: false))
        }
        libraries[grain] = made
        return made
    }

    private static let libraryLock = NSLock()
    private static var libraries: [Bool: [LibrarySource]] = [:]
}
