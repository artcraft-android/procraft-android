import SwiftUI

private let libraryWidth: CGFloat = 440
private let setsWidth: CGFloat = 168
private let brushHeight: CGFloat = 80
private let brushGap: CGFloat = 8
private let namePill = Color(argb: 0x8000_0000)

/// A panel's title row: its title (with a chevron when it has a menu) at its left, `action` at its
/// right.
struct PanelHeader<Action: View>: View {
    let title: String
    var start: CGFloat = 24
    var end: CGFloat = 18
    var height: CGFloat = 64
    var chevron = true
    var chevronOn = false
    var onTitle: (() -> Void)?
    @ViewBuilder let action: () -> Action

    var body: some View {
        ZStack(alignment: .topLeading) {
            HStack(spacing: 0) {
                Text(verbatim: title)
                    .font(.system(size: 24, weight: .bold))
                    .foregroundStyle(Pc.panelTitle)
                    .lineLimit(1)
                if chevron {
                    ZStack {
                        Circle().fill(chevronOn ? Pc.rowSelected : Pc.chevronDisc)
                        PcIconView(PcIcons.chevronDown, size: 13, color: Pc.chevronGlyph)
                    }
                    .frame(width: 17, height: 17)
                    .padding(.leading, 10)
                    .offset(y: -1.5)
                }
            }
            .padding(.leading, start)
            .padding(.top, 19)
            .contentShape(Rectangle())
            .onTapGesture { onTitle?() }
            HStack {
                Spacer()
                action()
            }
            .padding(.trailing, end)
        }
        .frame(maxWidth: .infinity, minHeight: height, maxHeight: height, alignment: .topLeading)
    }
}

/// The brush library hanging from the tool that opened it: the sets down its left, the shown
/// set's brushes (each its name over its sample stroke) on its right. A tap on a set shows it
/// (its last brush becoming the current one); a tap on a brush chooses it.
struct BrushLibrary: View {
    let model: CanvasModel
    let arrowX: CGFloat

    var body: some View {
        let title: String = switch model.tool {
        case .erase: "Erase library"
        case .smudge: "Smudge library"
        case .paint: "Brush library"
        }
        Popover(width: libraryWidth, arrowX: arrowX) {
            PanelHeader(title: title, start: 20, end: 12, height: 52) {
                PcIconView(PcIcons.plusMedium, size: 41, color: Pc.panelGlyph)
                    .frame(width: 41, height: 40)
                    .padding(.top, 12)
            }
            HStack(spacing: 0) {
                SetColumn(model: model)
                BrushColumn(model: model)
            }
            .frame(maxHeight: .infinity)
        }
    }
}

/// The library's sets, the shown one lit.
private struct SetColumn: View {
    let model: CanvasModel

    var body: some View {
        ScrollView {
            LazyVStack(spacing: 12) {
                ForEach(Array(model.brushSets.enumerated()), id: \.offset) { i, set in
                    let selected = i == model.shownSet
                    let ink = selected ? Color.white : Pc.panelTitle
                    let icon = model.setIcon(set)
                    HStack(spacing: 0) {
                        // a 40 frame centred 24 in
                        PcIconView(icon, size: 40, color: ink)
                            .padding(.leading, 4)
                        Text(verbatim: set.name)
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(ink)
                            .lineLimit(2)
                            .lineSpacing(0)
                            .padding(.leading, 1)
                            .padding(.trailing, 8)
                        Spacer(minLength: 0)
                    }
                    .frame(height: 50)
                    .background(
                        RoundedRectangle(cornerRadius: 12, style: .circular)
                            .fill(selected ? Pc.rowSelected : Pc.setCell)
                    )
                    .contentShape(Rectangle())
                    .onTapGesture { model.showSet(i) }
                }
            }
            .padding(12)
        }
        .scrollIndicators(.hidden)
        .frame(width: setsWidth)
        .background(Pc.setColumn)
    }
}

/// The shown set's brushes.
private struct BrushColumn: View {
    let model: CanvasModel

    var body: some View {
        let brushes = model.brushSets.indices.contains(model.shownSet) ? model.brushSets[model.shownSet].brushes : []
        ScrollView {
            LazyVStack(spacing: brushGap) {
                ForEach(brushes, id: \.index) { b in
                    BrushCell(model: model, brush: b)
                }
            }
            .padding(.leading, 12)
            .padding(.trailing, 12)
            .padding(.top, 9.75)
            .padding(.bottom, 12)
        }
        // another set's brushes replace these at once
        .id(model.shownSet)
        .scrollIndicators(.hidden)
        .frame(maxWidth: .infinity)
        .background(Pc.brushColumn)
    }
}

/// A brush in the library: its sample stroke filling the cell's height, its name over it (on a
/// dark pill where the stroke runs under it), lit when it is the current brush or its set's last.
private struct BrushCell: View {
    let model: CanvasModel
    let brush: BrushInfo

    var body: some View {
        let key = CanvasModel.brushKey(brush)
        let selected = model.highlighted(brush)
        let preview = model.previews[key]
        ZStack(alignment: .topLeading) {
            if let preview {
                // the samples are drawn white: dark on the light interface's cells
                if Look.shared.light && !selected {
                    Image(uiImage: preview)
                        .renderingMode(.template)
                        .resizable()
                        .scaledToFill()
                        .foregroundStyle(Color.black)
                } else {
                    Image(uiImage: preview)
                        .resizable()
                        .scaledToFill()
                }
            }
            Text(verbatim: brush.name)
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(selected ? Color.white : Pc.panelTitle)
                .lineLimit(1)
                .padding(.horizontal, 6)
                .padding(.vertical, 2)
                .background {
                    if model.shadedNames[key] == true {
                        Capsule().fill(namePill)
                    }
                }
                .offset(x: -6, y: -2)
                .padding(.leading, 18)
                .padding(.top, 11.75)
        }
        .frame(maxWidth: .infinity, minHeight: brushHeight, maxHeight: brushHeight, alignment: .topLeading)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .circular))
        .background(
            RoundedRectangle(cornerRadius: 12, style: .circular)
                .fill(selected ? Pc.rowSelected : Color.clear)
        )
        .contentShape(Rectangle())
        .onTapGesture { model.chooseBrush(brush) }
        .onAppear { model.loadPreview(brush) }
    }
}

/// The library's place under the bar: its width and which tool button it hangs from (0 is the
/// colour button, counting leftwards).
func libraryButton(_ tool: ToolKind) -> Int {
    switch tool {
    case .erase: 2
    case .smudge: 3
    case .paint: 4
    }
}

/// The library's width.
let brushLibraryWidth = libraryWidth
