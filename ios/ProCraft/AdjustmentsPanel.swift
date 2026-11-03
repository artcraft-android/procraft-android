import SwiftUI

private let adjustmentsWidth: CGFloat = 330
private var menuText: Color { Look.shared.light ? Color(argb: 0xFF39_3939) : Color(argb: 0xFFC5_C5C5) }

/// The adjustments menu hanging from its button, in groups as the core lists them: colour, blurs,
/// effects, painting. (Each adjustment's own screen is still to come on the iPad.)
struct AdjustmentsPanel: View {
    let model: CanvasModel
    let arrowX: CGFloat
    private let groups = adjustmentGroups()

    var body: some View {
        Popover(width: adjustmentsWidth, arrowX: arrowX) {
            VStack(alignment: .leading, spacing: 0) {
                // a 52 header, the title's capitals from 28.5 below the panel's top; the groups 8
                // into the list under it
                Text(verbatim: "Adjustments")
                    .font(.system(size: 24, weight: .bold))
                    .foregroundStyle(Pc.panelTitle)
                    .lineLimit(1)
                    .padding(.leading, 24)
                    .padding(.top, 22.5)
                    .frame(height: 52, alignment: .topLeading)
                ScrollView {
                    VStack(spacing: 10) {
                        ForEach(Array(groups.enumerated()), id: \.offset) { _, group in
                            VStack(spacing: 0) {
                                ForEach(Array(group.enumerated()), id: \.offset) { i, adjustment in
                                    if i > 0 {
                                        Rectangle().fill(Pc.actionsSeparator).frame(height: 1)
                                    }
                                    Text(verbatim: adjustmentLabel(adjustment: adjustment))
                                        .font(.system(size: 17))
                                        .foregroundStyle(menuText)
                                        .lineLimit(1)
                                        .padding(.leading, 15)
                                        .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44, alignment: .leading)
                                }
                            }
                            .background(RoundedRectangle(cornerRadius: 13, style: .circular).fill(Pc.actionsGroup))
                            .clipShape(RoundedRectangle(cornerRadius: 13, style: .circular))
                        }
                    }
                    .padding(.horizontal, 10)
                    .padding(.top, 8)
                    .padding(.bottom, 10)
                }
                .scrollIndicators(.hidden)
            }
        }
    }
}

/// The adjustments menu's width.
let adjustmentsPanelWidth = adjustmentsWidth
