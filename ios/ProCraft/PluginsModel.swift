import AVFoundation
import Foundation
import Observation
import SwiftUI

/// How big the interface shows, as a plugin says (1: as measured). SwiftUI has no density to turn,
/// so each screen is laid out in a window smaller or larger by this and scaled to fill the real one
/// (`sized`): their points are this much bigger than the window's. A view drawing pixels (the
/// canvas's) is scaled with them, and takes this many more pixels to its point to stay at the
/// screen's own. It is done inside each screen's reader of its geometry, which pins its corner: done
/// around them it goes by the safe area, and a size under 1 grows what measures the window.
@Observable
final class Interface {
    static let shared = Interface()

    private(set) var scale: CGFloat = 1
    /// The real window, in its own points.
    private(set) var window = CGSize.zero

    /// Asks the plugins again, for the window as it stands.
    func ask() {
        guard window.width > 0, window.height > 0 else { return }
        let asked = PluginsModel.shared.store.interfaceScale(width: Float(window.width), height: Float(window.height))
        if asked.isFinite, asked > 0, CGFloat(asked) != scale {
            scale = CGFloat(asked)
        }
    }

    func fit(_ size: CGSize) {
        if size != window {
            window = size
        }
        let was = scale
        ask()
        // a plugin may fit itself to the window: what hit-tests the canvas's handles goes with it
        if scale != was {
            PluginsModel.shared.onChange()
        }
    }


    /// A place on the window, in the interface's points.
    func point(_ p: CGPoint) -> CGPoint {
        CGPoint(x: p.x / scale, y: p.y / scale)
    }

    func rect(_ r: CGRect) -> CGRect {
        CGRect(x: r.minX / scale, y: r.minY / scale, width: r.width / scale, height: r.height / scale)
    }

    /// The way back: a place in the interface's points, on the window.
    func onWindow(_ p: CGPoint) -> CGPoint {
        CGPoint(x: p.x * scale, y: p.y * scale)
    }

    func onWindow(_ r: CGRect) -> CGRect {
        CGRect(x: r.minX * scale, y: r.minY * scale, width: r.width * scale, height: r.height * scale)
    }

    /// A screen `size` points on the window, in the interface's points.
    func screen(_ size: CGSize) -> InterfaceScreen {
        InterfaceScreen(size: CGSize(width: size.width / scale, height: size.height / scale))
    }
}

/// A screen as what is on it sees it.
struct InterfaceScreen {
    let size: CGSize
}

extension View {
    /// Laid out in `screen`, then scaled to fill what it came from. As measured, this changes nothing.
    func sized(_ screen: InterfaceScreen) -> some View {
        frame(width: screen.size.width, height: screen.size.height, alignment: .topLeading)
            .scaleEffect(Interface.shared.scale, anchor: .topLeading)
    }
}

/// The plugins kept in the app's folder, each value kept as text, as the core reads it; and what
/// their page and the one screen that asks about a file draw.
@Observable
final class PluginsModel {
    static let shared = PluginsModel()

    private static let keptKey = "pluginValues"

    @ObservationIgnored let store: Plugins
    private(set) var plugins: [PluginInfo] = []
    /// Goes up with any change that may move what a plugin answers, for what asks it again.
    private(set) var changes = 0
    /// Words for someone who changed a setting that now comes as a plugin, said once.
    private(set) var movedNotice: String?
    /// A plugin file waiting on the one screen that asks about it, and what it holds.
    private(set) var offer: PluginOffer?
    @ObservationIgnored private var offered: Data?
    @ObservationIgnored private var kept: [String: String]
    /// What the artwork open does about a change.
    @ObservationIgnored var onChange: () -> Void = {}

    private init() {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory())
        // those the app comes with added there first, once, and off
        store = Plugins.withBundled(dir: support.appendingPathComponent("plugins").path)
        kept = UserDefaults.standard.dictionary(forKey: Self.keptKey) as? [String: String] ?? [:]
        for (key, value) in kept {
            store.readKept(key: key, value: value)
        }
        movedNotice = store.takeMovedNotice()
        read()
    }

    private func keep(_ key: String, _ value: String?) {
        kept[key] = value
        UserDefaults.standard.set(kept, forKey: Self.keptKey)
    }

    /// The list as it stands: a plugin may have stopped on a stroke since it was read.
    func read() {
        plugins = store.plugins()
    }

    private func changed() {
        read()
        changes += 1
        Interface.shared.ask()
        onChange()
    }

    func movedNoticeRead() {
        movedNotice = nil
    }

    /// Turns a plugin on or off: it goes on or off its hooks at once, and its values wait for it.
    func setOn(_ plugin: PluginInfo, _ on: Bool) {
        guard store.setOn(id: plugin.id, on: on) else { return }
        keep(plugin.onKey, on ? "true" : "false")
        changed()
    }

    /// A plugin's switch flipped; a key no control has changes nothing.
    func setSwitch(_ key: String, _ on: Bool) {
        guard store.setSwitch(key: key, on: on) else { return }
        keep(key, on ? "true" : "false")
        changed()
    }

    /// A plugin's slider let go at `value`; a key no control has changes nothing.
    func setSlider(_ slider: PluginSlider, _ value: Float) {
        guard let landed = store.setSlider(key: slider.key, value: value) else { return }
        keep(slider.key, "\(landed)")
        changed()
    }

    /// A sound picked at `url` for a plugin's file control `key`: read in the background (any sound
    /// the system can play, a minute of it at most), then kept with the plugin; `told` if it can't be
    /// used.
    func pickSound(_ key: String, from url: URL, told: @escaping (String) -> Void) {
        let name = url.deletingPathExtension().lastPathComponent
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let wav = Self.wav(of: url)
            DispatchQueue.main.async {
                guard let self else { return }
                if let wav, self.store.setSoundFile(key: key, name: name.isEmpty ? "Recording" : name, wav: wav) {
                    self.changed()
                } else {
                    told("This sound couldn't be used.")
                }
            }
        }
    }

    /// A plugin's file control `key` lets its file go.
    func clearFile(_ key: String) {
        if store.clearFile(key: key) {
            changed()
        }
    }

    /// The sound at `url` as a WAV file's bytes: one channel (all of them mixed), 16 bits, at its
    /// own rate, a minute of it at most. None if the system can't read it.
    private static func wav(of url: URL) -> Data? {
        let scoped = url.startAccessingSecurityScopedResource()
        defer {
            if scoped {
                url.stopAccessingSecurityScopedResource()
            }
        }
        guard let file = try? AVAudioFile(forReading: url) else { return nil }
        let format = file.processingFormat
        let rate = format.sampleRate
        guard rate >= 1 else { return nil }
        let frames = AVAudioFrameCount(max(0, min(file.length, AVAudioFramePosition(rate * 60))))
        guard frames > 0, let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames),
              (try? file.read(into: buffer, frameCount: frames)) != nil,
              let channels = buffer.floatChannelData else { return nil }
        let count = Int(format.channelCount)
        let length = Int(buffer.frameLength)
        guard count > 0, length > 0 else { return nil }
        var samples = [Int16](repeating: 0, count: length)
        for i in 0 ..< length {
            var sum: Float = 0
            for c in 0 ..< count {
                sum += channels[c][i]
            }
            samples[i] = Int16((min(max(sum / Float(count), -1), 1) * 32767).rounded())
        }
        var wav = Data()
        func put(_ text: String) {
            wav.append(contentsOf: Array(text.utf8))
        }
        func put32(_ v: UInt32) {
            withUnsafeBytes(of: v.littleEndian) { wav.append(contentsOf: $0) }
        }
        func put16(_ v: UInt16) {
            withUnsafeBytes(of: v.littleEndian) { wav.append(contentsOf: $0) }
        }
        let bytes = UInt32(length * 2)
        let hertz = UInt32(rate.rounded())
        put("RIFF")
        put32(36 + bytes)
        put("WAVE")
        put("fmt ")
        put32(16)
        put16(1)
        put16(1)
        put32(hertz)
        put32(hertz * 2)
        put16(2)
        put16(16)
        put("data")
        put32(bytes)
        for v in samples {
            put16(UInt16(bitPattern: v))
        }
        return wav
    }

    /// Removes an imported plugin, its folder and its values; it goes at once. Whether it could be.
    func remove(_ plugin: PluginInfo) -> Bool {
        guard let forget = store.remove(id: plugin.id) else { return false }
        forget.forEach { keep($0, nil) }
        changed()
        return true
    }

    /// No further than a file that can be taken, whatever it is.
    private static func read(_ url: URL, most: Int) -> Data? {
        let scoped = url.startAccessingSecurityScopedResource()
        defer {
            if scoped {
                url.stopAccessingSecurityScopedResource()
            }
        }
        guard let file = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? file.close() }
        guard let read = try? file.read(upToCount: most) else { return nil }
        return read
    }

    /// Asks about the plugin file at `url`: nothing in it runs, and nothing is kept until it is
    /// accepted. What can't be imported says why, through `told`.
    func offer(_ url: URL, told: @escaping (String) -> Void) {
        let most = Int(pluginFileMostBytes()) + 1
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let bytes = Self.read(url, most: most)
            DispatchQueue.main.async {
                guard let self else { return }
                guard let bytes else {
                    told("This file can't be read.")
                    return
                }
                let offer = self.store.inspect(bytes: bytes)
                if let refused = offer.refused {
                    told(refused)
                    return
                }
                self.offered = bytes
                self.offer = offer
            }
        }
    }

    func declineOffer() {
        offered = nil
        offer = nil
    }

    /// Keeps the file on offer. What went wrong, if something did.
    func acceptOffer() -> String? {
        let bytes = offered
        declineOffer()
        let said = bytes.flatMap { store.install(bytes: $0) }
        changed()
        return said
    }
}
