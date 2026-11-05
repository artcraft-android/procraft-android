import Foundation
import UIKit

/// Handing files to other apps through the system's share sheet.
enum Sharing {
    /// A folder for files made to share, emptied by the system as it needs.
    static var exportsDir: URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("exports")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    /// A name that is safe as a file's: anything else in it goes, none at all is `otherwise`.
    static func safeName(_ name: String, otherwise: String) -> String {
        let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: " _-"))
        let safe = String(name.unicodeScalars.map { allowed.contains($0) ? Character($0) : "_" })
        return safe.trimmingCharacters(in: .whitespaces).isEmpty ? otherwise : safe
    }

    /// The share sheet for `files`, hanging from `from` (points on the screen), else from the middle
    /// of the screen.
    static func share(_ files: [URL], from: CGRect? = nil) {
        guard let top = topController() else { return }
        let sheet = UIActivityViewController(activityItems: files, applicationActivities: nil)
        if let popover = sheet.popoverPresentationController {
            popover.sourceView = top.view
            let bounds = top.view.bounds
            popover.sourceRect = from ?? CGRect(x: bounds.midX, y: bounds.midY, width: 0, height: 0)
            popover.permittedArrowDirections = from == nil ? [] : .any
        }
        top.present(sheet, animated: true)
    }

    /// What is shown on top now, for something to be presented over it.
    private static func topController() -> UIViewController? {
        let scene = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first
        var top = scene?.keyWindow?.rootViewController
        while let shown = top?.presentedViewController {
            top = shown
        }
        return top
    }
}

extension URL {
    /// The bytes of a file the system's picker gave, read with the access it gave.
    func readPicked() -> Data? {
        let scoped = startAccessingSecurityScopedResource()
        defer {
            if scoped {
                stopAccessingSecurityScopedResource()
            }
        }
        return try? Data(contentsOf: self)
    }
}
