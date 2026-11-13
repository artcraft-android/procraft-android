import Foundation
import SwiftUI
import UIKit

/// What the Share tab makes.
enum ShareKind {
    case psd, pdf, tiff, jpeg, png, layersPdf, layerPngs
}

/// How a picture comes in from the Add tab.
enum InsertAsk {
    case file, photo, camera
}

/// The Actions panel's work on the open artwork: what it shares, and what it brings in.
extension CanvasModel {
    /// Runs `work`, an export by the core, off the main thread with the export card showing how far
    /// it has come and a check once made; what it made goes to the share sheet. Cancelled, nothing
    /// is shared; made nothing, `failed` is told.
    private func exporting(
        failed: @escaping @autoclosure () -> String, progress: (() -> Float)? = nil,
        _ work: @escaping (CanvasSession) -> [URL]
    ) {
        guard exportShown == nil else { return }
        let session = session
        session.resetExportProgress()
        exportCancelled = false
        exportShown = ProgressShown(progress: 0)
        let poll = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            self?.exportShown = ProgressShown(progress: progress?() ?? session.exportProgress())
        }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let files = work(session)
            DispatchQueue.main.async {
                poll.invalidate()
                guard let self else { return }
                if self.exportCancelled {
                    self.exportShown = nil
                } else if files.isEmpty {
                    self.exportShown = nil
                    self.showNotice(failed(), warning: true)
                } else {
                    self.exportShown = ProgressShown(progress: 1, done: true)
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.45) {
                        self.exportShown = nil
                        Sharing.share(files)
                    }
                }
            }
        }
    }

    /// The time-lapse as a video to share: all of it, or `seconds` long; its card shows how far it has
    /// come. Which of two things went wrong, if it can't be made, is told: they are different faults.
    func shareTimeLapse(seconds: Int?) {
        let lapse = timeLapse.lapse
        guard lapse.count > 0 else {
            saying = ("Time-lapse", "There is no time-lapse to export yet.")
            return
        }
        let meter = ExportMeter()
        exporting(
            failed: lapse.frame(0) != nil ? "The time-lapse video couldn't be made." : "The time-lapse can't be read on this device.",
            progress: { meter.share }
        ) { [weak self] session in
            let out = Sharing.exportsDir.appendingPathComponent(exportFileName(name: session.name(), package: false) + ".mp4")
            let made = lapse.export(to: out, seconds: seconds) { share in
                meter.share = share
                return self?.exportCancelled != true
            }
            return made ? [out] : []
        }
    }

    /// The layered package, named with `suffix`.
    func sharePackage(suffix: String) {
        let lapse = timeLapse.lapse
        exporting(failed: "The artwork couldn't be exported.") { session in
            let base = exportFileName(name: session.name(), package: true)
            let out = Sharing.exportsDir.appendingPathComponent(base + suffix)
            let made = (try? session.exportPackage(path: out.path, seed: UInt64.random(in: 0 ... UInt64.max), timeLapse: lapse.forPackage())) != nil
            return made ? [out] : []
        }
    }

    func share(_ kind: ShareKind, quality: UInt8 = PdfQuality.best) {
        exporting(failed: "The artwork couldn't be exported.") { session in
            let dir = Sharing.exportsDir
            let base = exportFileName(name: session.name(), package: false)
            func file(_ suffix: String) -> URL {
                dir.appendingPathComponent(base + suffix)
            }
            do {
                switch kind {
                case .psd:
                    try session.exportPsd(path: file(".psd").path)
                    return [file(".psd")]
                case .tiff:
                    try session.exportTiff(path: file(".tif").path)
                    return [file(".tif")]
                case .jpeg:
                    try session.exportJpeg(path: file(".jpg").path)
                    return [file(".jpg")]
                case .png:
                    try session.exportPng(path: file(".png").path)
                    return [file(".png")]
                case .pdf:
                    try session.exportPdf(path: file(".pdf").path, layers: false, quality: quality)
                    return [file(".pdf")]
                case .layersPdf:
                    try session.exportPdf(path: file(".pdf").path, layers: true, quality: quality)
                    return [file(".pdf")]
                case .layerPngs:
                    let folder = dir.appendingPathComponent(base)
                    try? FileManager.default.removeItem(at: folder)
                    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
                    return try session.exportLayerPngs(dir: folder.path).map { URL(fileURLWithPath: $0) }
                }
            } catch {
                return []
            }
        }
    }

    /// The file a plugin makes of the artwork's layers of a kind it adds (`e`), to share.
    func sharePlugin(_ e: PluginExport) {
        exporting(failed: "The artwork couldn't be exported.") { session in
            let base = exportFileName(name: session.name(), package: false)
            let out = Sharing.exportsDir.appendingPathComponent(base).appendingPathExtension(e.`extension`)
            let made = (try? session.exportPlugin(key: e.key, path: out.path)) != nil
            return made ? [out] : []
        }
    }

    /// The bytes of a picture on a new layer, fitted to the canvas (a moving GIF's frames on the
    /// timeline instead), with `transform` ready to transform; a message if it couldn't be.
    func insertPicture(_ data: Data?, name: String?, isPrivate: Bool, transform: Bool = true) {
        closePanels()
        guard let data else {
            showNotice("This picture can't be opened.", warning: true)
            return
        }
        // a moving GIF: its frames (a still one is a picture)
        if name?.lowercased().hasSuffix(".gif") == true, let done = session.insertGifFrames(bytes: data, private: isPrivate) {
            if done.added == 0 {
                showNotice(done.full ? noRoomSaid() : "This picture can't be opened.", warning: true)
                return
            }
            refresh()
            showNotice("Frames saved to timeline")
            if done.full {
                showNotice(noRoomSaid(), warning: true)
            }
            return
        }
        guard let image = GalleryModel.decode(data, p3: CanvasSpace.shared.p3)?.image else {
            showNotice("This picture can't be opened.", warning: true)
            return
        }
        guard session.insertImage(image: image, private: isPrivate) != nil else {
            noRoomForLayers()
            return
        }
        refresh()
        if transform {
            toggleTransform()
        }
    }

    /// A file from the picker, read with the access it gave.
    func insertFile(_ url: URL, isPrivate: Bool) {
        insertPicture(url.readPicked(), name: url.lastPathComponent, isPrivate: isPrivate)
    }

    func insertPhoto(_ picture: UIImage?, isPrivate: Bool) {
        insertPicture(picture?.jpegData(compressionQuality: 1), name: nil, isPrivate: isPrivate)
    }

    func changeRapidUndoDelay(_ share: Float) {
        let range = AppSettings.rapidUndoRange
        AppSettings.shared.rapidUndoDelay = range.lowerBound + share * (range.upperBound - range.lowerBound)
    }

    func changeSelectionVisibility(_ share: Float) {
        AppSettings.shared.selectionVisibility = share
        session.setSelectionVisibility(visibility: share)
    }

    /// Project canvas: the picture on a second display while there is one.
    func changeProjectCanvas(_ on: Bool) {
        AppSettings.shared.projectCanvas = on
        Projection.shared.update()
    }

    func changeBrushScaling(onScreen: Bool) {
        AppSettings.shared.brushOnScreen = onScreen
        session.setBrushScaling(onScreen: onScreen)
    }
}
