import SwiftUI

private var modeBar: Color { Look.shared.light ? Color(argb: 0xD6EE_EEEE) : Color(argb: 0xD63B_3B3B) }
private var modeText: Color { Look.shared.light ? Color(argb: 0xFF6A_6A6A) : Color(argb: 0xFFC5_C5C5) }
private var modesFill: Color { Look.shared.light ? Color(argb: 0xFFFF_FFFF) : Color(argb: 0xFF7B_7B7B) }

private extension QuickShapeKind {
    var label: String {
        switch self {
        case .line: "Line"
        case .polyline: "Polyline"
        case .arc: "Arc"
        case .ellipse: "Ellipse"
        case .circle: "Circle"
        case .triangle: "Triangle"
        case .equilateralTriangle: "Equilateral"
        case .quadrilateral: "Quadrilateral"
        case .rectangle: "Rectangle"
        case .square: "Square"
        }
    }

    var icon: PcIcon {
        switch self {
        case .line: PcIcons.shapeLine
        case .polyline: PcIcons.shapePolyline
        case .arc: PcIcons.shapeArc
        case .ellipse: PcIcons.shapeEllipse
        case .circle: PcIcons.shapeCircle
        case .triangle: PcIcons.shapeTriangle
        case .equilateralTriangle: PcIcons.shapeEquilateral
        case .quadrilateral: PcIcons.shapeQuad
        case .rectangle: PcIcons.shapeRect
        case .square: PcIcons.shapeSquare
        }
    }
}

/// The bar a snapped stroke shows: the shape's name (and, while its handles show, "Editing" and
/// the kind in grey); a tap shows the handles, and the kinds it can be shown as below.
struct QuickShapeBar: View {
    let model: CanvasModel

    var body: some View {
        if let info = model.quickShape {
            let choosing = model.quickShapeEditing && info.choices.count > 1
            VStack(spacing: 0) {
                HStack(spacing: 0) {
                    // drawn in a 28 frame, shown as it is
                    PcIconView(info.kind.icon, size: 28, color: modeText)
                        .frame(width: 29, height: 28)
                    Text(verbatim: model.quickShapeEditing ? "Editing" : info.kind.label)
                        .font(.system(size: 17, weight: .bold))
                        .foregroundStyle(modeText)
                        .lineLimit(1)
                    if choosing {
                        Text(verbatim: info.kind.label)
                            .font(.system(size: 15))
                            .foregroundStyle(Color(argb: 0xFF9A_9A9A))
                            .lineLimit(1)
                            .padding(.leading, 3)
                    }
                    if info.choices.count > 1 {
                        PcIconView(PcIcons.disclose, size: 32, color: modeText)
                            .frame(width: 33, height: 32)
                    }
                }
                .frame(maxWidth: .infinity, minHeight: 32, maxHeight: 32)
                .background(modeBar)
                .contentShape(Rectangle())
                .onTapGesture { model.quickShapeEditing = true }
                if choosing {
                    let each: CGFloat = info.choices.count > 2 ? 154 : 116
                    HStack(spacing: 2) {
                        ForEach(info.choices, id: \.self) { kind in
                            let chosen = kind == info.kind
                            HStack(spacing: 6) {
                                PcIconView(kind.icon, size: 28, color: .white)
                                Text(verbatim: kind.label)
                                    .font(.system(size: 14))
                                    .foregroundStyle(Color.white)
                                    .lineLimit(1)
                            }
                            .frame(width: each, height: 28)
                            .background(Capsule().fill(chosen ? Pc.accent : Color.clear))
                            .contentShape(Rectangle())
                            .onTapGesture { model.chooseQuickShape(kind) }
                        }
                    }
                    .padding(2)
                    .background(Capsule().fill(modesFill))
                    .padding(.top, 20)
                }
            }
        }
    }
}

/// The snapped shape's handles, while they show: blue dots ringed in white.
struct QuickShapeHandles: View {
    let model: CanvasModel

    var body: some View {
        if let info = model.quickShape, model.quickShapeEditing {
            let pixels = model.pixels
            Canvas { context, _ in
                let h = info.handles
                for i in 0 ..< h.count / 2 {
                    let x = CGFloat(h[i * 2]) / pixels
                    let y = CGFloat(h[i * 2 + 1]) / pixels
                    context.fill(Path(ellipseIn: CGRect(x: x - 6.5, y: y - 6.5, width: 13, height: 13)), with: .color(.white))
                    context.fill(Path(ellipseIn: CGRect(x: x - 5, y: y - 5, width: 10, height: 10)), with: .color(Pc.accent))
                }
            }
            .allowsHitTesting(false)
        }
    }
}

/// With the interface hidden, the button in the top left that brings it back.
struct ShowInterfaceButton: View {
    let model: CanvasModel

    var body: some View {
        // its glyph is drawn wider than it is tall: 50 units shown 36.7 by 32
        PcIconView(PcIcons.fullScreen, size: 36.7, color: Pc.sideGlyph)
            .frame(width: 44, height: 44)
            .contentShape(Rectangle())
            .onTapGesture { model.toggleBars() }
            .offset(x: 20)
    }
}

/// Layer Select's list: the layers drawn under the point, top first, the current one blue. A tap
/// beside it closes it.
struct LayerChoiceList: View {
    let model: CanvasModel
    let choice: LayerChoiceAt
    let screen: CGSize

    var body: some View {
        let height = CGFloat(choice.ids.count * 44 + 16)
        let x = min(max(choice.at.x, 8), screen.width - 208)
        let y = min(max(choice.at.y + 24, 8), screen.height - height)
        let shape = RoundedRectangle(cornerRadius: 16, style: .circular)
        ZStack(alignment: .topLeading) {
            Color.clear
                .contentShape(Rectangle())
                .onTapGesture { model.layerChoice = nil }
            VStack(spacing: 0) {
                ForEach(choice.ids, id: \.self) { id in
                    if let l = model.layers.first(where: { $0.id == id }) {
                        Text(verbatim: l.name)
                            .font(.system(size: 16))
                            .foregroundStyle(l.current ? Color.white : Pc.panelTitle)
                            .lineLimit(1)
                            .padding(.leading, 16)
                            .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44, alignment: .leading)
                            .background(l.current ? Pc.accent : Color.clear)
                            .contentShape(Rectangle())
                            .onTapGesture {
                                model.selectLayer(id)
                                model.layerChoice = nil
                            }
                    }
                }
            }
            .padding(.vertical, 8)
            .frame(width: 200, height: height)
            .background(shape.fill(Pc.popoverFill))
            .clipShape(shape)
            .offset(x: x, y: y)
        }
    }
}
