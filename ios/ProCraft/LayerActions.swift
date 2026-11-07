import Foundation

/// What can be done to the layers, each read again afterwards. Those changing pixels have their
/// thumbnails read again with the layers.
extension CanvasModel {
    /// Whether layers are picked to act on with the current one.
    var anyPicked: Bool {
        layers.contains { $0.picked }
    }

    /// Layer `l`'s options menu, as the core lists it.
    func layerOptions(_ l: LayerInfo) -> [LayerOptionRow] {
        session.layerOptions(id: l.id)
    }

    /// The rows a drag of `l` takes along, it among them (the shared core decides).
    func movingWith(_ l: LayerInfo) -> Set<UInt64> {
        Set(session.movingWith(id: l.id))
    }

    /// Moves `l` (with what goes with it) just above or below `target`'s row; a chosen layer with
    /// others picked takes them along, in their order (the shared core decides).
    func moveNextTo(_ l: LayerInfo, _ target: LayerInfo, above: Bool) {
        _ = session.moveNextTo(id: l.id, target: target.id, above: above)
        refresh()
    }

    func setLayerOpacity(_ l: LayerInfo, _ value: Float, done: Bool) {
        _ = session.setLayerOpacity(id: l.id, opacity: value, done: done)
        refresh()
    }

    func setBlend(_ l: LayerInfo, _ blend: UInt32) {
        _ = session.setLayerBlend(id: l.id, blend: blend)
        refresh()
    }

    func toggleAlphaLock(_ l: LayerInfo) {
        _ = session.setLayerAlphaLock(id: l.id, on: !l.alphaLock)
        refresh()
    }

    func toggleClipped(_ l: LayerInfo) {
        _ = session.setLayerClipped(id: l.id, on: !l.clipped)
        refresh()
    }

    func toggleReference(_ l: LayerInfo) {
        _ = session.setLayerReference(id: l.id, on: !l.reference)
        refresh()
    }

    /// Layer `l` shown alone, or, already alone, every layer again; a message says which (the
    /// core's words). The other rows' boxes show them hidden while it is alone.
    func toggleSolo(_ l: LayerInfo) {
        if let said = session.holdVisibility(id: l.id) {
            showNotice(said)
        }
        refresh()
    }

    func rename(_ l: LayerInfo, _ name: String) {
        _ = session.renameLayer(id: l.id, name: name)
        refresh()
    }

    func duplicate(_ l: LayerInfo) {
        if session.duplicateLayer(id: l.id) == nil {
            noRoomForLayers()
        }
        refresh()
    }

    /// Delete on a locked layer's row is told about rather than done (as measured).
    func delete(_ l: LayerInfo) {
        if l.locked {
            lockedDeleteTold = true
            return
        }
        _ = session.deleteLayer(id: l.id)
        refresh()
    }

    func clear(_ l: LayerInfo) {
        _ = session.selectLayer(id: l.id)
        session.clearLayer()
        refresh()
    }

    func addMask(_ l: LayerInfo) {
        if session.addMask(id: l.id) == nil, !hasMask(l) {
            noRoomForLayers()
        }
        refresh()
    }

    func mergeDown(_ l: LayerInfo) {
        _ = session.mergeDown(id: l.id)
        refresh()
    }

    func combineDown(_ l: LayerInfo) {
        _ = session.combineDown(id: l.id)
        refresh()
    }

    func mergeMask(_ l: LayerInfo) {
        _ = session.mergeMask(id: l.id)
        refresh()
    }

    func ungroup(_ l: LayerInfo) {
        _ = session.ungroup(id: l.id)
        refresh()
    }

    func flattenGroup(_ l: LayerInfo) {
        _ = session.flattenGroup(id: l.id)
        refresh()
    }

    func selectLayerContents(_ l: LayerInfo) {
        _ = session.selectLayerContents(id: l.id)
        refresh()
    }

    func fillLayer(_ l: LayerInfo) {
        _ = session.fillLayer(id: l.id)
        refresh()
    }

    func invertLayer(_ l: LayerInfo) {
        _ = session.invertLayer(id: l.id)
        refresh()
    }

    func setGuideAssisted(_ on: Bool) {
        guide.setAssisted(on)
    }

    func rasterize(_ l: LayerInfo) {
        _ = session.rasterizeLayer(id: l.id)
        refresh()
        showNotice("Text rasterized", warning: true)
    }

    // MARK: the picked layers

    /// Picks layer `l` to act on with the current one, or unpicks it.
    func pickLayer(_ l: LayerInfo) {
        _ = session.pickLayer(id: l.id)
        refresh()
    }

    func pickAllLayers() {
        _ = session.pickAllLayers()
        refresh()
    }

    func groupPicked() {
        _ = session.groupPicked()
        refresh()
    }

    func mergePicked() {
        _ = session.mergePicked()
        refresh()
    }

    /// Two rows pinched together: layers `a` and `b` and every one between merge into the lowest.
    func mergeBetween(_ a: UInt64, _ b: UInt64) {
        _ = session.mergeBetween(a: a, b: b)
        refresh()
    }

    func duplicatePicked() {
        if !session.duplicatePicked() {
            noRoomForLayers()
        }
        refresh()
    }

    func deletePicked() {
        _ = session.deletePicked()
        refresh()
    }

    /// Whether the chosen layers include a locked one (Delete then removes nothing).
    func chosenLocked() -> Bool {
        session.chosenLocked()
    }

    /// The current and picked layers each as a PNG, handed to the share sheet.
    func shareChosenLayers() {
        let session = session
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let folder = Sharing.exportsDir.appendingPathComponent("layers")
            try? FileManager.default.removeItem(at: folder)
            try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let files = (try? session.exportChosenPngs(dir: folder.path)) ?? []
            DispatchQueue.main.async {
                if files.isEmpty {
                    self?.showNotice("The layers couldn't be exported.", warning: true)
                } else {
                    Sharing.share(files.map { URL(fileURLWithPath: $0) })
                }
            }
        }
    }
}
