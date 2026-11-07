import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// How faint an artwork's cell stays while it is dragged.
let draggedAlpha = 0.3
/// The blue over an artwork (or stack) a dragged artwork would be stacked with.
let dropWash = Color(argb: 0x8000_64DC)

/// How long a dragged artwork rests on a stack (or a stack's way back) before it opens (the gallery).
private let restSeconds = 0.6
/// How near the grid's top or bottom a drag scrolls it, and how far each frame.
private let dragScrollEdge: CGFloat = 60
private let dragScrollStep: CGFloat = 12
/// How opaque a dragged artwork's (or stack's) picture is, and how much larger than in its place.
private let dragPictureAlpha: CGFloat = 204 / 255
private let artworkLifted: CGFloat = 1.07
/// What an artwork dragged out to another app becomes, as Settings says: the file, and a picture for
/// those that can't read it.
private var dragFormats: [String] {
    let file = SystemSettings.dragFormat
    return ["JPEG", "PNG", "TIFF"].contains(file) ? [file] : [file, SystemSettings.dragImageFormat]
}

private func type(of format: String) -> UTType {
    switch format {
    case "PSD": UTType("com.adobe.photoshop-image") ?? .data
    case "JPEG": .jpeg
    case "PNG": .png
    case "TIFF": .tiff
    default: UTType(filenameExtension: "procraft") ?? .data
    }
}

/// What a drag shows on the gallery's cells.
@Observable
final class GalleryDragShown {
    /// The artwork or stack being dragged (by folder), faint in its place.
    var dragging: String?
    /// What it would be stacked with if let go, washed blue.
    var onto: String?
    /// It is resting on a stack's way back.
    var backLit = false
}

/// Where a drag over the gallery's grid is: the cell it would be stacked onto, or else the slot it
/// would go in.
private struct DropAt {
    let onto: String?
    let slot: Int
}

private extension GalleryEntry {
    /// The artwork that shows for it: a stack's top one.
    var shownArtwork: GalleryArtwork? {
        switch self {
        case let .artwork(artwork): artwork
        case let .stack(_, _, artworks): artworks.first
        }
    }
}

/// The thumbnail's box in a cell at `cell` on the window.
private func thumbnailBox(_ cell: CGRect) -> CGRect {
    CGRect(x: cell.minX + 20, y: cell.minY + 39, width: cell.width - 40, height: cell.height - 82)
}

/// An artwork's picture as it sits in its thumbnail's `box`: fitted, standing on the box's bottom.
private func fitted(_ box: CGRect, _ artwork: GalleryArtwork) -> CGRect {
    let ratio: CGFloat = artwork.width > 0 && artwork.height > 0 ? CGFloat(artwork.width) / CGFloat(artwork.height) : 0.75
    let width = ratio <= box.width / box.height ? box.height * ratio : box.width
    let height = width / ratio
    return CGRect(x: box.midX - width / 2, y: box.maxY - height, width: width, height: height)
}

/// A copy of a dropped file, which goes when its reader returns: in a folder of its own, as two may
/// have one name.
private func keepDropped(_ url: URL, in folder: URL) -> URL? {
    let inside = folder.appendingPathComponent(UUID().uuidString)
    let copy = inside.appendingPathComponent(url.lastPathComponent)
    try? FileManager.default.createDirectory(at: inside, withIntermediateDirectories: true)
    return (try? FileManager.default.copyItem(at: url, to: copy)) != nil ? copy : nil
}

/// Copies of what another app dropped, gathered as they arrive.
private final class Dropped {
    var urls: [URL] = []
}

/// What `session` holds copied while it lasts, then `done` with the copies and the folder they are in.
func loadDropped(_ session: any UIDropSession, done: @escaping ([URL], URL) -> Void) {
    let folder = FileManager.default.temporaryDirectory.appendingPathComponent("drop-\(UUID().uuidString)")
    let dropped = Dropped()
    let group = DispatchGroup()
    for item in session.items {
        guard let type = item.itemProvider.registeredTypeIdentifiers.first else { continue }
        group.enter()
        _ = item.itemProvider.loadFileRepresentation(forTypeIdentifier: type) { url, _ in
            let kept = url.flatMap { keepDropped($0, in: folder) }
            DispatchQueue.main.async {
                if let kept {
                    dropped.urls.append(kept)
                }
                group.leave()
            }
        }
    }
    group.notify(queue: .main) {
        done(dropped.urls, folder)
    }
}

/// The gallery's drags. Held and moved, an artwork goes out to other apps as a file made when they
/// ask for it, and within the gallery: let go over another artwork's picture (or a stack's) it is
/// stacked with it, anywhere else on the grid it goes where it is let go. A stack moves within the
/// gallery only. Near the grid's top or bottom the grid scrolls. Resting on a stack opens it and, in
/// a stack, resting on the way back opens the gallery, the drag going on, so an artwork can be put
/// in a stack where it is let go, or taken out of one. Files dragged in are imported, as Import's.
final class GalleryDragger: NSObject, UIDragInteractionDelegate, UIDropInteractionDelegate {
    private let model: GalleryModel
    private let shown: GalleryDragShown
    /// Whether nothing is open over the gallery.
    var canStart: () -> Bool = { false }

    private var at: DropAt?
    private var rest: DispatchWorkItem?
    /// What it rests on: "" the way back, else a stack (by folder).
    private var restingOn: String?
    /// -1 scrolling up, 1 down.
    private var edge = 0
    private var link: CADisplayLink?
    private weak var scroller: UIScrollView?

    init(model: GalleryModel, shown: GalleryDragShown) {
        self.model = model
        self.shown = shown
    }

    // MARK: what is dragged

    private func provider(for artwork: GalleryArtwork) -> NSItemProvider {
        let provider = NSItemProvider()
        provider.suggestedName = exportFileName(name: artwork.name, package: false)
        for format in dragFormats {
            provider.registerFileRepresentation(forTypeIdentifier: type(of: format).identifier, fileOptions: [], visibility: .all) { [model] done in
                model.fileForDrag(artwork, format: format) { url in
                    done(url, false, url == nil ? CocoaError(.fileWriteUnknown) : nil)
                }
                return nil
            }
        }
        return provider
    }

    func dragInteraction(_ interaction: UIDragInteraction, itemsForBeginning session: any UIDragSession) -> [UIDragItem] {
        guard canStart(), let window = interaction.view else { return [] }
        let point = Interface.shared.point(session.location(in: window))
        guard let entry = model.entries.first(where: { model.cellAt[$0.dir]?.contains(point) == true }) else { return [] }
        let item: UIDragItem
        if case let .artwork(artwork) = entry {
            item = UIDragItem(itemProvider: provider(for: artwork))
        } else {
            item = UIDragItem(itemProvider: NSItemProvider())
        }
        item.localObject = entry
        session.localContext = entry
        return [item]
    }

    /// Its picture: the thumbnail as big as it is lifted, on white with rounded corners, a little
    /// see-through.
    func dragInteraction(_ interaction: UIDragInteraction, previewForLifting item: UIDragItem, session _: any UIDragSession) -> UITargetedDragPreview? {
        guard let window = interaction.view, let entry = item.localObject as? GalleryEntry,
              let cell = model.cellAt[entry.dir], let top = entry.shownArtwork
        else { return nil }
        let place = fitted(thumbnailBox(cell), top)
        let view = UIImageView(image: UIImage(contentsOfFile: (top.dir as NSString).appendingPathComponent("thumbnail.png")))
        let lifted = artworkLifted * Interface.shared.scale
        view.frame = CGRect(x: 0, y: 0, width: place.width * lifted, height: place.height * lifted)
        view.backgroundColor = .white
        view.alpha = dragPictureAlpha
        let parameters = UIDragPreviewParameters()
        parameters.backgroundColor = .clear
        parameters.visiblePath = UIBezierPath(roundedRect: view.bounds, cornerRadius: 8)
        let target = UIDragPreviewTarget(container: window, center: Interface.shared.onWindow(CGPoint(x: place.midX, y: place.midY)))
        return UITargetedDragPreview(view: view, parameters: parameters, target: target)
    }

    func dragInteraction(_: UIDragInteraction, prefersFullSizePreviewsFor _: any UIDragSession) -> Bool {
        true
    }

    func dragInteraction(_: UIDragInteraction, sessionIsRestrictedToDraggingApplication session: any UIDragSession) -> Bool {
        if case .stack = session.localContext as? GalleryEntry {
            return true
        }
        return false
    }

    func dragInteraction(_: UIDragInteraction, sessionWillBegin session: any UIDragSession) {
        guard let entry = session.localContext as? GalleryEntry else { return }
        withAnimation(Motion.artworkLift) { shown.dragging = entry.dir }
    }

    func dragInteraction(_: UIDragInteraction, session _: any UIDragSession, didEndWith _: UIDropOperation) {
        withAnimation(Motion.artworkLift) { shown.dragging = nil }
        settle()
    }

    // MARK: where it goes

    /// Where `point` (on the window) is over the entries: over another's picture (the thumbnail as
    /// it sits in its box, or a stack's box) it is that one, when `stacking`; anywhere else the slot
    /// between cells it is nearest, before a cell's middle or after it.
    private func dropAt(_ point: CGPoint, dragged: String, stacking: Bool) -> DropAt? {
        let entries = model.entries
        for (i, entry) in entries.enumerated() {
            guard let cell = model.cellAt[entry.dir], cell.contains(point) else { continue }
            var picture = thumbnailBox(cell)
            if case let .artwork(artwork) = entry {
                picture = fitted(picture, artwork)
            }
            if stacking, entry.dir != dragged, picture.contains(point) {
                return DropAt(onto: entry.dir, slot: i)
            }
            return DropAt(onto: nil, slot: point.x < cell.midX ? i : i + 1)
        }
        // past the last cell: last
        guard let end = entries.last.flatMap({ model.cellAt[$0.dir] }) else { return nil }
        let past = point.y >= end.maxY || (point.y >= end.minY && point.x >= end.maxX)
        return past ? DropAt(onto: nil, slot: entries.count) : nil
    }

    private func stopResting() {
        rest?.cancel()
        rest = nil
        restingOn = nil
        shown.backLit = false
    }

    /// After a moment resting on `on` (the same each time it moves), `then`.
    private func resting(on: String, then: @escaping () -> Void) {
        if restingOn == on {
            return
        }
        stopResting()
        restingOn = on
        shown.backLit = on.isEmpty
        let work = DispatchWorkItem { [weak self] in
            self?.stopResting()
            then()
        }
        rest = work
        DispatchQueue.main.asyncAfter(deadline: .now() + restSeconds, execute: work)
    }

    /// The scroll view under the grid's middle.
    private func gridScroller(on window: UIView) -> UIScrollView? {
        var under = window.hitTest(Interface.shared.onWindow(CGPoint(x: model.gridAt.midX, y: model.gridAt.midY)), with: nil)
        while let view = under {
            if let found = view as? UIScrollView {
                return found
            }
            under = view.superview
        }
        return nil
    }

    private func scroll(_ next: Int, on window: UIView?) {
        guard next != edge else { return }
        edge = next
        link?.invalidate()
        link = nil
        guard next != 0, let window else { return }
        scroller = gridScroller(on: window)
        let made = CADisplayLink(target: self, selector: #selector(GalleryDragger.scrollStep))
        made.add(to: .main, forMode: .common)
        link = made
    }

    @objc private func scrollStep() {
        guard let scroller else { return }
        let inset = scroller.adjustedContentInset
        let least = -inset.top
        let most = max(scroller.contentSize.height + inset.bottom - scroller.bounds.height, least)
        scroller.contentOffset.y = min(max(scroller.contentOffset.y + CGFloat(edge) * dragScrollStep, least), most)
    }

    /// Only a change: this comes with every move, and each telling draws the grid again.
    private func showOnto(_ dir: String?) {
        if shown.onto != dir {
            shown.onto = dir
        }
    }

    private func track(_ entry: GalleryEntry, at point: CGPoint, on window: UIView?) {
        if model.openStack != nil, model.backAt.contains(point) {
            at = nil
            scroll(0, on: window)
            showOnto(nil)
            resting(on: "") { [model] in model.showStack(nil) }
            return
        }
        let grid = model.gridAt
        scroll(point.y < grid.minY + dragScrollEdge ? -1 : point.y > grid.maxY - dragScrollEdge ? 1 : 0, on: window)
        var stacking = false
        if case .artwork = entry {
            stacking = model.openStack == nil
        }
        at = dropAt(point, dragged: entry.dir, stacking: stacking)
        showOnto(at?.onto)
        if case let .stack(dir, name, _)? = model.entries.first(where: { $0.dir == at?.onto }) {
            resting(on: dir) { [model] in model.showStack(dir, name: name) }
        } else {
            stopResting()
        }
    }

    private func settle() {
        at = nil
        scroll(0, on: nil)
        stopResting()
        showOnto(nil)
    }

    private func ownEntry(_ session: any UIDropSession) -> GalleryEntry? {
        session.localDragSession?.localContext as? GalleryEntry
    }

    func dropInteraction(_: UIDropInteraction, canHandle session: any UIDropSession) -> Bool {
        ownEntry(session) != nil || (canStart() && session.hasItemsConforming(toTypeIdentifiers: [UTType.item.identifier]))
    }

    func dropInteraction(_ interaction: UIDropInteraction, sessionDidUpdate session: any UIDropSession) -> UIDropProposal {
        guard let entry = ownEntry(session) else { return UIDropProposal(operation: .copy) }
        if let window = interaction.view {
            track(entry, at: Interface.shared.point(session.location(in: window)), on: window)
        }
        return UIDropProposal(operation: .move)
    }

    func dropInteraction(_: UIDropInteraction, sessionDidExit _: any UIDropSession) {
        settle()
    }

    func dropInteraction(_: UIDropInteraction, sessionDidEnd _: any UIDropSession) {
        settle()
    }

    func dropInteraction(_: UIDropInteraction, performDrop session: any UIDropSession) {
        guard let entry = ownEntry(session) else {
            bringIn(session)
            return
        }
        guard let drop = at else { return }
        let list = model.entries
        if case let .artwork(artwork) = entry, let onto = list.first(where: { $0.dir == drop.onto }) {
            model.put(artwork, onto: onto)
        } else {
            // from a stack it comes in at the slot; from here, the slot counted without it
            let from = list.firstIndex { $0.dir == entry.dir }
            let to = from.map { drop.slot > $0 ? drop.slot - 1 : drop.slot } ?? drop.slot
            if from != to {
                model.move(entry, to: to)
            }
        }
    }

    /// Files from another app: each copied while it lasts, then brought in as Import's.
    private func bringIn(_ session: any UIDropSession) {
        loadDropped(session) { [model] urls, folder in
            model.importDropped(urls, from: folder)
        }
    }
}

/// Puts the gallery's drags (`GalleryDragger`) on the window it is in, so one press finds the cell
/// under it and a drag can end anywhere.
struct GalleryDragCatcher: UIViewRepresentable {
    let model: GalleryModel
    let shown: GalleryDragShown
    let canStart: () -> Bool

    final class Host: UIView {
        var kept: [UIInteraction] = []

        override func didMoveToWindow() {
            super.didMoveToWindow()
            for interaction in kept {
                interaction.view?.removeInteraction(interaction)
                window?.addInteraction(interaction)
            }
        }
    }

    func makeCoordinator() -> GalleryDragger {
        GalleryDragger(model: model, shown: shown)
    }

    func makeUIView(context: Context) -> Host {
        let host = Host()
        host.isUserInteractionEnabled = false
        host.kept = [UIDragInteraction(delegate: context.coordinator), UIDropInteraction(delegate: context.coordinator)]
        return host
    }

    func updateUIView(_: Host, context: Context) {
        context.coordinator.canStart = canStart
    }

    static func dismantleUIView(_ host: Host, coordinator _: GalleryDragger) {
        for interaction in host.kept {
            interaction.view?.removeInteraction(interaction)
        }
    }
}
