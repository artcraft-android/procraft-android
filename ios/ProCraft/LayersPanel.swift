import SwiftUI

private let layersWidth: CGFloat = 384
private let rowHeight: CGFloat = 69
/// A layer with a mask, and its mask: their rows sit closer.
private let maskedRowHeight: CGFloat = 65
private let rowGap: CGFloat = 1
private let listCorner: CGFloat = 8
private let groupHeight: CGFloat = 44
private let groupIndent: CGFloat = 21
private let groupLine = Color(argb: 0xFF4E_4E4E)
private let maskOwnerRow = Color(argb: 0xFF2C_5281)
private let pickedRow = Color(argb: 0xFF2C_5281)
private let lockedLetter = Color(argb: 0xFF80_8080)
/// Blend modes shown by their first two letters (their first is another mode's).
private let twoLetterBlends: Set<String> = ["Darken", "Shade", "Lighten", "Subtract", "Divide", "Saturation"]

/// The layers panel: its title and a button adding a layer, then the layers top first (a closed
/// group's left out), each its thumbnail, name, blend mode's letters and visibility box, and the
/// background colour's row under them. A tap on a row makes it the current layer.
struct LayersPanel: View {
    let model: CanvasModel
    let arrowX: CGFloat
    /// The most the panel may be tall: its list scrolls past that.
    let maxHeight: CGFloat
    /// How tall the rows are together.
    @State private var listHeight: CGFloat = 0

    var body: some View {
        let shown = model.layers.filter { !model.hidden($0) }
        Popover(width: layersWidth, arrowX: arrowX) {
            PanelHeader(title: "Layers", chevronOn: model.layers.contains { $0.picked }) {
                PcIconView(PcIcons.plusMedium, size: 41, color: Pc.panelGlyph)
                    .frame(width: 40, height: 41)
                    .contentShape(Rectangle())
                    .onTapGesture { model.addLayer() }
                    .padding(.top, 12)
            }
            ScrollView {
                VStack(spacing: rowGap) {
                    ForEach(Array(shown.enumerated()), id: \.element.id) { index, l in
                        // the list's ends are rounded
                        let first = index == 0
                        let last = index == shown.count - 1
                        let shape = UnevenRoundedRectangle(
                            topLeadingRadius: first ? listCorner : 0, bottomLeadingRadius: last ? listCorner : 0,
                            bottomTrailingRadius: last ? listCorner : 0, topTrailingRadius: first ? listCorner : 0,
                            style: .circular
                        )
                        if l.kind == .group {
                            GroupRow(model: model, layer: l, shape: shape)
                        } else {
                            LayerRow(model: model, layer: l, shape: shape, owner: model.layers.contains { $0.kind == .mask && $0.parent == l.id && $0.current })
                        }
                    }
                    BackgroundRow(model: model)
                        .padding(.top, 10)
                }
                .padding(.horizontal, 12)
                .padding(.bottom, 12)
                .background {
                    GeometryReader { rows in
                        Color.clear
                            .onAppear { listHeight = rows.size.height }
                            .onChange(of: rows.size.height) { _, height in listHeight = height }
                    }
                }
            }
            .scrollIndicators(.hidden)
            // as tall as its rows, up to what the screen leaves under the header
            .frame(height: max(0, min(listHeight, maxHeight - 64 - Pc.arrowHeight)))
        }
    }
}

/// A layer's row.
private struct LayerRow: View {
    let model: CanvasModel
    let layer: LayerInfo
    let shape: UnevenRoundedRectangle
    /// The layer whose mask is the current one: a darker blue.
    let owner: Bool

    var body: some View {
        let l = layer
        let mask = l.kind == .mask
        let masked = model.hasMask(l)
        let selected = l.current
        let ink = selected || l.picked ? Color.white : Pc.panelTitle
        let fill = selected ? Pc.rowSelected : l.picked ? pickedRow : owner ? maskOwnerRow : Pc.layerRow
        HStack(spacing: 0) {
            if l.clipped {
                PcIconView(PcIcons.clip, size: 16, color: ink)
            }
            // a mask's thumbnail and its layer's join into one, rounded only at the outer corners
            Thumb(model: model, layer: l, top: !masked, bottom: !mask)
                .padding(.leading, 4)
                .offset(y: mask ? 2 : masked ? -2 : 0)
            ZStack(alignment: .leading) {
                LockedName(layer: l, ink: ink)
                if let tag = tag(l) {
                    Text(verbatim: tag)
                        .font(.system(size: 13))
                        .foregroundStyle(ink)
                        .lineLimit(1)
                        .padding(.leading, 29)
                        .offset(y: 17.5)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            // the blend and visibility buttons overlap by 8, as measured; a mask has no blend
            if mask {
                Spacer().frame(width: 36)
            } else {
                Text(verbatim: blendCode(l.blend))
                    .font(.system(size: 14, weight: .semibold))
                    // a locked layer's mode is greyed
                    .foregroundStyle(l.locked && !l.current && !l.picked ? lockedLetter : ink)
                    .frame(width: 44, height: 47)
                    .offset(x: 8)
            }
            CheckBox(checked: l.visible, fill: selected || l.picked ? Color.white : Pc.checkFill)
                .frame(width: 44, height: 47)
                .contentShape(Rectangle())
                .onTapGesture { model.toggleVisible(l) }
            Spacer().frame(width: 8)
        }
        .padding(.leading, (l.clipped ? 18 : 0) + groupIndent * CGFloat(l.depth))
        .frame(maxWidth: .infinity)
        .frame(height: mask || masked ? maskedRowHeight : rowHeight)
        .background(shape.fill(fill))
        .contentShape(Rectangle())
        .onTapGesture { model.selectLayer(l.id) }
    }

    /// What the layer is, under its name: the reference layer, one drawing assist is on for, a
    /// private one.
    private func tag(_ l: LayerInfo) -> String? {
        if l.reference && l.assisted {
            return "Assisted Reference"
        }
        if l.reference {
            return "Reference"
        }
        if l.assisted {
            return "Assisted"
        }
        if l.private {
            return "Private"
        }
        return nil
    }

    /// The letters on a layer's row for its mode: a two-word name's initials (the second small), a
    /// one-word name's first letter, or its first two where one letter is already taken.
    private func blendCode(_ blend: UInt32) -> String {
        guard let name = model.blendModeList.first(where: { $0.id == blend })?.name else { return "N" }
        if twoLetterBlends.contains(name) {
            return String(name.prefix(2))
        }
        let words = name.split(separator: " ")
        if words.count == 1 {
            return String(name.prefix(1))
        }
        return String(words[0].prefix(1)) + words[1].prefix(1).lowercased()
    }
}

/// A group's row: a line where a thumbnail would be, its name, its open or closed arrow and its
/// box.
private struct GroupRow: View {
    let model: CanvasModel
    let layer: LayerInfo
    let shape: UnevenRoundedRectangle

    var body: some View {
        let l = layer
        let ink = l.current || l.picked ? Color.white : Pc.panelTitle
        HStack(spacing: 0) {
            RoundedRectangle(cornerRadius: 2, style: .circular)
                .fill(groupLine)
                .frame(width: 93, height: 4)
                .padding(.leading, 4)
            LockedName(layer: l, ink: ink)
                .frame(maxWidth: .infinity, alignment: .leading)
            PcIconView(l.collapsed ? PcIcons.chevronRight : PcIcons.chevronDown, size: 16, color: ink)
                .frame(width: 44, height: groupHeight)
                .contentShape(Rectangle())
                .onTapGesture { model.toggleCollapsed(l) }
                .offset(x: 8)
            CheckBox(checked: l.visible, fill: l.current || l.picked ? Color.white : Pc.checkFill)
                .frame(width: 44, height: groupHeight)
                .contentShape(Rectangle())
                .onTapGesture { model.toggleVisible(l) }
            Spacer().frame(width: 8)
        }
        .padding(.leading, groupIndent * CGFloat(l.depth))
        .frame(maxWidth: .infinity)
        .frame(height: groupHeight)
        .background(shape.fill(l.current ? Pc.rowSelected : l.picked ? pickedRow : Pc.layerRow))
        .contentShape(Rectangle())
        .onTapGesture { model.selectLayer(l.id) }
    }
}

/// A layer's name, with a padlock before it when it is locked (the glyph ends 12 short of the
/// name).
private struct LockedName: View {
    let layer: LayerInfo
    let ink: Color

    var body: some View {
        ZStack(alignment: .leading) {
            if layer.locked {
                PcIconView(PcIcons.lockRow, size: 17, color: ink)
                    .offset(x: 3)
            }
            Text(verbatim: layer.name)
                .font(.system(size: 17))
                .foregroundStyle(ink)
                .lineLimit(1)
                .padding(.leading, 29)
                .padding(.trailing, 6)
        }
    }
}

/// A layer's thumbnail, 93 x 61: what is drawn, fitted (small drawings show their pixels), on
/// grey, white for a mask, a checkerboard with alpha lock; a text layer's is its glyph. `top` and
/// `bottom` say which corners are rounded.
private struct Thumb: View {
    let model: CanvasModel
    let layer: LayerInfo
    let top: Bool
    let bottom: Bool

    var body: some View {
        let corner: CGFloat = 4
        let shape = UnevenRoundedRectangle(
            topLeadingRadius: top ? corner : 0, bottomLeadingRadius: bottom ? corner : 0,
            bottomTrailingRadius: bottom ? corner : 0, topTrailingRadius: top ? corner : 0,
            style: .circular
        )
        ZStack {
            if layer.alphaLock {
                Checker()
            } else {
                Rectangle().fill(layer.kind == .mask ? Color.white : Pc.thumbBackground)
            }
            if layer.text {
                PcIconView(PcIcons.textLayer, size: 61, color: Pc.textLayerGlyph)
            } else if let picture = model.thumbnails[layer.id] {
                Image(uiImage: picture)
                    .resizable()
                    .interpolation(.none)
                    .scaledToFit()
            }
        }
        .frame(width: 93, height: 61)
        .clipShape(shape)
    }
}

/// The transparency checkerboard behind thumbnails.
private struct Checker: View {
    var body: some View {
        Canvas { context, size in
            let cell: CGFloat = 6
            context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(.white))
            var y = 0
            while CGFloat(y) * cell < size.height {
                var x = 0
                while CGFloat(x) * cell < size.width {
                    if (x + y) % 2 == 1 {
                        context.fill(Path(CGRect(x: CGFloat(x) * cell, y: CGFloat(y) * cell, width: cell, height: cell)), with: .color(Color(argb: 0xFFD6_D6D6)))
                    }
                    x += 1
                }
                y += 1
            }
        }
    }
}

/// A visibility box: filled with the tick cut out of it (the row showing through), or an outline.
struct CheckBox: View {
    let checked: Bool
    let fill: Color

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 5, style: .circular)
        ZStack {
            if checked {
                shape.fill(fill)
                // the tick is cut out of the box
                PcIconView(PcIcons.tick, size: 14, color: .black)
                    .blendMode(.destinationOut)
            } else {
                shape.strokeBorder(fill, lineWidth: 1.5)
            }
        }
        .frame(width: 18, height: 18)
        .compositingGroup()
    }
}

/// The background colour's row: its swatch (none shown while hidden) and its visibility box.
private struct BackgroundRow: View {
    let model: CanvasModel

    var body: some View {
        let bg = model.background
        // hidden, the swatch shows no colour: a faint white over the row
        let swatch = bg.visible ? Color(.sRGB, red: Double(bg.r), green: Double(bg.g), blue: Double(bg.b)) : Color.white.opacity(0.06)
        HStack(spacing: 0) {
            HStack(spacing: 0) {
                RoundedRectangle(cornerRadius: 14, style: .circular)
                    .fill(swatch)
                    .frame(width: 92, height: 42)
                    .padding(.leading, 4)
                Text(verbatim: "Background color")
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.panelTitle)
                    .lineLimit(1)
                    .padding(.leading, 14)
                Spacer(minLength: 0)
            }
            CheckBox(checked: bg.visible, fill: Pc.checkFill)
                .frame(width: 44, height: 41)
                .contentShape(Rectangle())
                .onTapGesture { model.toggleBackground() }
            Spacer().frame(width: 8)
        }
        .frame(height: 50)
        .background(RoundedRectangle(cornerRadius: 17, style: .circular).fill(Pc.backgroundRow))
    }
}

/// The layers panel's width.
let layersPanelWidth = layersWidth
