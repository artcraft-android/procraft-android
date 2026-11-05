import SwiftUI

/// The library's menus' width.
let glassMenuWidth: CGFloat = 253
private let menuEdge = Color(argb: 0xFF5A_5D5F)
private let menuRule = Color(argb: 0xFF3F_4347)

/// A small menu: frosted over the canvas, nearly solid over a panel, with a light edge; its rows
/// 40 high.
struct GlassMenu<Content: View>: View {
    var overPanel = false
    var width: CGFloat = glassMenuWidth
    @ViewBuilder let content: () -> Content

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 16, style: .circular)
        VStack(spacing: 0) {
            content()
        }
        .padding(.vertical, 7)
        .frame(width: width)
        .background {
            ZStack {
                if overPanel {
                    shape.fill(Pc.glassMenuOverPanel)
                } else {
                    shape.fill(.ultraThinMaterial)
                    shape.fill(Pc.glassMenu)
                }
            }
        }
        .overlay(shape.strokeBorder(menuEdge, lineWidth: 1))
        .clipShape(shape)
    }
}

/// A menu row: its icon (a 40 frame) centred 29.5 in, the label from 52, a chevron 27.5 from the
/// right.
struct MenuRow: View {
    let icon: PcIcon
    let label: String
    var ink: Color?
    var trailing: PcIcon?
    var iconSize: CGFloat = 40
    let action: () -> Void

    var body: some View {
        let ink = ink ?? Pc.menuInk
        HStack(spacing: 0) {
            PcIconView(icon, size: iconSize, color: ink)
                .frame(width: 40, height: 40)
                .padding(.leading, 9.5)
            Text(verbatim: label)
                .font(.system(size: 17))
                .foregroundStyle(ink)
                .lineLimit(1)
                .padding(.leading, 2.5)
            Spacer(minLength: 0)
            if let trailing {
                PcIconView(trailing, size: 40, color: ink)
                    .padding(.trailing, 7.5)
            }
        }
        .frame(height: 40)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
    }
}

/// The line between a menu's groups of rows.
struct MenuLine: View {
    var gap: CGFloat = 10

    var body: some View {
        Rectangle()
            .fill(menuRule)
            .frame(height: 1)
            .padding(.horizontal, 15.5)
            .padding(.vertical, gap)
    }
}

extension View {
    /// A menu's coming and going: it grows out of `anchor` from almost nothing, overshooting a
    /// little; closing, it fades as it shrinks a little.
    func menuTransition(_ anchor: UnitPoint) -> some View {
        transition(.asymmetric(
            insertion: .scale(scale: Motion.menuStartScale, anchor: anchor),
            removal: .scale(scale: Motion.menuEndScale, anchor: anchor).combined(with: .opacity)
        ))
    }
}

/// Everything else on the screen, for a menu open over it: a touch there closes the menu and goes
/// no further. It reaches past the view it is put on.
struct MenuDismissal: View {
    let onDismiss: () -> Void

    var body: some View {
        Color.clear
            .frame(width: 6000, height: 6000)
            .contentShape(Rectangle())
            .onTapGesture(perform: onDismiss)
            // laid from the top left of what it is put on: this reaches every way from there
            .offset(x: -3000, y: -3000)
    }
}
