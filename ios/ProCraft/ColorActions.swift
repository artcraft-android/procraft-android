import Observation
import SwiftUI
import UIKit

/// Icons worked out rather than listed.
extension PcIcons {
    /// Nine small squares.
    static let menuGrid = PcIcon(name: "menu-grid", width: 40, height: 40, parts: (0 ..< 9).map { k in
        let x = 11.5 + Double(k % 3) * 6.2
        let y = 11.5 + Double(k / 3) * 6.2
        let d = "M\(x + 0.8) \(y)h3.4a0.8 0.8 0 0 1 0.8 0.8v3.4a0.8 0.8 0 0 1-0.8 0.8h-3.4a0.8 0.8 0 0 1-0.8-0.8v-3.4a0.8 0.8 0 0 1 0.8-0.8z"
        return PcIconPart(d: d, stroke: 0, fill: true, evenOdd: false)
    })

    /// A 23 x 18 tag pointing left, a round-ended cross cut out of it.
    static let hexBackspace = PcIcon(name: "hex-backspace", width: 24, height: 24, parts: [
        PcIconPart(
            d: "M8.5 3H22a1.5 1.5 0 0 1 1.5 1.5v15a1.5 1.5 0 0 1-1.5 1.5H8.5L1.2 12.7Q0.5 12 1.2 11.3z"
                + "M14.5 10.657L16.828 8.328a0.95 0.95 0 0 1 1.344 1.344L15.843 12L18.172 14.328a0.95 0.95 0 0 1-1.344 1.344"
                + "L14.5 13.343L12.172 15.672a0.95 0.95 0 0 1-1.344-1.344L13.157 12L10.828 9.672a0.95 0.95 0 0 1 1.344-1.344z",
            stroke: 0, fill: true, evenOdd: true
        ),
    ])
}

/// A colour the pickers show and change: the one painted with, or one of its own (a background).
protocol ColorPick: AnyObject {
    var hue: Float { get }
    var saturation: Float { get }
    var brightness: Float { get }
    func rgb() -> UInt32
    func setHsv(_ h: Float, _ s: Float, _ v: Float)
    func setRgb(_ rgb: UInt32)
}

/// The colour painted with.
final class BrushColor: ColorPick {
    private let model: CanvasModel

    init(model: CanvasModel) {
        self.model = model
    }

    var hue: Float { model.hue }
    var saturation: Float { model.saturation }
    var brightness: Float { model.brightness }

    func rgb() -> UInt32 {
        model.colorRgb()
    }

    func setHsv(_ h: Float, _ s: Float, _ v: Float) {
        model.setColor(hue: h, saturation: s, brightness: v)
    }

    func setRgb(_ rgb: UInt32) {
        model.setRgb(rgb)
    }
}

/// A colour of its own, starting at `rgb`; `onChange` hears every change. Greys keep their hue, and
/// a grey opens at 180 degrees, as measured.
@Observable
final class HeldColor: ColorPick {
    private(set) var hue: Float = 0.5
    private(set) var saturation: Float = 0
    private(set) var brightness: Float = 0
    @ObservationIgnored private let onChange: (UInt32) -> Void

    init(rgb: UInt32, onChange: @escaping (UInt32) -> Void) {
        self.onChange = onChange
        take(rgb)
    }

    private func take(_ rgb: UInt32) {
        let r = Float((rgb >> 16) & 0xFF) / 255
        let g = Float((rgb >> 8) & 0xFF) / 255
        let b = Float(rgb & 0xFF) / 255
        let most = max(r, g, b)
        let span = most - min(r, g, b)
        brightness = most
        saturation = most > 0 ? span / most : 0
        guard span > 0 else { return }
        var turn: Float
        if most == r {
            turn = ((g - b) / span).truncatingRemainder(dividingBy: 6)
        } else if most == g {
            turn = (b - r) / span + 2
        } else {
            turn = (r - g) / span + 4
        }
        if turn < 0 {
            turn += 6
        }
        hue = turn / 6
    }

    func rgb() -> UInt32 {
        CanvasModel.rgb(hue: hue, saturation: saturation, brightness: brightness)
    }

    func setHsv(_ h: Float, _ s: Float, _ v: Float) {
        hue = min(max(h, 0), 1)
        saturation = min(max(s, 0), 1)
        brightness = min(max(v, 0), 1)
        onChange(rgb())
    }

    func setRgb(_ rgb: UInt32) {
        take(rgb)
        onChange(rgb)
    }
}

/// The palettes and the background's colour.
extension CanvasModel {
    /// Reads the palettes again; a change that couldn't be written out (the storage full, say) is
    /// told, once until one is.
    func refreshPalettes() {
        let store = Self.paletteStore
        setPalettes(store.list(), defaultPalette: Int(store.defaultPalette()))
        if store.takeFailure() != nil {
            showNotice("The palettes couldn't be kept.", warning: true)
        }
    }

    func addPalette() {
        Self.paletteStore.add()
        refreshPalettes()
    }

    /// Our example palettes back, those missing added at the end.
    func restoreExamplePalettes() {
        Self.paletteStore.restoreExamples()
        refreshPalettes()
    }

    func renamePalette(_ i: Int, _ name: String) {
        Self.paletteStore.rename(index: UInt32(i), name: name)
        refreshPalettes()
    }

    func duplicatePalette(_ i: Int) {
        Self.paletteStore.duplicate(index: UInt32(i))
        refreshPalettes()
    }

    func deletePalette(_ i: Int) {
        Self.paletteStore.delete(index: UInt32(i))
        refreshPalettes()
    }

    func chooseDefaultPalette(_ i: Int) {
        Self.paletteStore.chooseDefault(index: UInt32(i))
        refreshPalettes()
    }

    /// Puts `rgb` in a palette's `slot`, or empties it when none.
    func setSwatch(_ i: Int, _ slot: Int, _ rgb: UInt32?) {
        Self.paletteStore.setSwatch(index: UInt32(i), slot: UInt32(slot), rgb: rgb)
        refreshPalettes()
    }

    /// Palette `i` as a swatches file, handed to the share sheet.
    func sharePalette(_ i: Int) {
        guard palettes.indices.contains(i), let bytes = Self.paletteStore.swatchesFile(index: UInt32(i)) else { return }
        let file = Sharing.exportsDir
            .appendingPathComponent(Sharing.safeName(palettes[i].name, otherwise: "Palette"))
            .appendingPathExtension("swatches")
        if (try? bytes.write(to: file, options: .atomic)) != nil {
            Sharing.share([file])
        } else {
            showNotice("The palette couldn't be exported.", warning: true)
        }
    }

    /// A palette file (ours, or another program's swatches) or a picture to take colours from, picked
    /// from Files.
    func paletteFromFile(_ url: URL) {
        guard let bytes = url.readPicked() else {
            showNotice("This file could not be read.", warning: true)
            return
        }
        if Self.paletteStore.importFile(bytes: bytes, fileName: url.lastPathComponent) {
            refreshPalettes()
        } else {
            paletteFromPicture(UIImage(data: bytes))
        }
    }

    /// A palette of the main colours in `picture`, light to dark, added at the end.
    func paletteFromPicture(_ picture: UIImage?) {
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let pixels = picture.flatMap { Self.smallPixels(of: $0) }
            let added = pixels.map { Self.paletteStore.addFromPixels(pixels: $0) } ?? false
            DispatchQueue.main.async {
                if added {
                    self?.refreshPalettes()
                } else {
                    self?.showNotice("This picture can't be read.", warning: true)
                }
            }
        }
    }

    /// `picture` read small, as 0xAARRGGBB: a few hundred pixels across is plenty to find its colours.
    private static func smallPixels(of picture: UIImage) -> [UInt32]? {
        let across = picture.size.width * picture.scale
        let down = picture.size.height * picture.scale
        guard across > 0, down > 0 else { return nil }
        let shrink = min(1, 256 / max(across, down))
        let w = max(Int(across * shrink), 1)
        let h = max(Int(down * shrink), 1)
        var pixels = [UInt32](repeating: 0, count: w * h)
        let info = CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue
        let drawn: Bool = pixels.withUnsafeMutableBytes { bytes in
            guard let context = CGContext(
                data: bytes.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: info
            ) else { return false }
            context.translateBy(x: 0, y: CGFloat(h))
            context.scaleBy(x: 1, y: -1)
            UIGraphicsPushContext(context)
            picture.draw(in: CGRect(x: 0, y: 0, width: w, height: h))
            UIGraphicsPopContext()
            return true
        }
        return drawn ? pixels : nil
    }

    /// The background's colour as 0xRRGGBB.
    func backgroundRgb() -> UInt32 {
        func channel(_ v: Float) -> UInt32 {
            UInt32(wholeNumber(min(max(v, 0), 1) * 255))
        }
        return (channel(background.r) << 16) | (channel(background.g) << 8) | channel(background.b)
    }

    /// Sets the background's colour, live while a picker moves; `done` makes the change one undo
    /// step.
    func setBackgroundRgb(_ rgb: UInt32, done: Bool) {
        let r = Float((rgb >> 16) & 0xFF) / 255
        let g = Float((rgb >> 8) & 0xFF) / 255
        let b = Float(rgb & 0xFF) / 255
        _ = session.setBackgroundColor(r: r, g: g, b: b, done: done)
        if done {
            refresh()
        } else {
            readBackground()
        }
    }

    /// The Layers panel shows the colour panel, picking the background's colour.
    func pickBackground() {
        backgroundPick = HeldColor(rgb: backgroundRgb()) { [weak self] rgb in
            self?.setBackgroundRgb(rgb, done: false)
        }
    }

    /// The background picker closes (its tick, or the panel going away): the colour is kept as one
    /// undo step.
    func endBackgroundPick() {
        guard backgroundPick != nil else { return }
        backgroundPick = nil
        setBackgroundRgb(backgroundRgb(), done: true)
    }
}
