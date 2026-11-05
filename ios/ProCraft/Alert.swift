import SwiftUI

// buttons at least this wide, the panel growing with them or with a longer message; as measured,
// the panel is this much wider each side than its message's widest line (or its title's)
private let buttonWidth: CGFloat = 192
private let buttonGap: CGFloat = 12
private let side: CGFloat = 15
private let textSide: CGFloat = 27.5
private let mostWidth = buttonWidth * 3 + buttonGap * 2 + side * 2
private let dim = 0.7
private let startScale: CGFloat = 1.1
private var fill: Color { Look.shared.light ? Color(argb: 0xF2E3_E3E3) : Color(argb: 0xF21E_1E1E) }
private var ink: Color { Look.shared.light ? Color(argb: 0xFF22_2222) : Color(argb: 0xFFC5_C5C5) }
private var cancelFill: Color { Look.shared.light ? Color(argb: 0xFFD9_D9D9) : Color(argb: 0xFF2D_2D2D) }
private var cancelInk: Color { Look.shared.light ? .black : .white }
private var destroyFill: Color { Look.shared.light ? Color(argb: 0xFFFD_0100) : Color(argb: 0xFFFE_2D2E) }

/// What an alert says and offers: a title (after an `icon`), a message, and `cancel` beside the
/// action, red when it destroys; an `extra` choice (blue, as the action) can sit between them.
struct AlertContent {
    let title: String
    var message = ""
    let action: String
    var icon: PcIcon?
    var destructive = true
    var cancel = "Cancel"
    var extra: String?
    /// A notice: its action alone, nothing to cancel.
    var alone = false
}

/// An alert over everything while `content` is there: the screen dims to 70% black as a dark
/// glass panel fades in, settling from a little larger. A tap beside the panel cancels.
struct AlertView: View {
    let content: AlertContent?
    let onCancel: () -> Void
    let onAction: () -> Void
    var onExtra: () -> Void = {}

    /// The last one shown, kept while it goes.
    @State private var shown: AlertContent?
    @State private var up = false

    var body: some View {
        ZStack {
            if let c = shown {
                Color.black.opacity(up ? dim : 0)
                    .ignoresSafeArea()
                    .contentShape(Rectangle())
                    .onTapGesture { if content != nil { onCancel() } }
                panel(c)
                    .opacity(up ? 1 : 0)
                    .scaleEffect(up ? 1 : startScale)
            }
        }
        .onChange(of: content?.title, initial: true) { _, _ in
            if let content {
                shown = content
                withAnimation(Motion.alertIn) { up = true }
            } else if shown != nil {
                withAnimation(Motion.alertOut) { up = false } completion: { shown = nil }
            }
        }
    }

    private func panel(_ c: AlertContent) -> some View {
        let buttons: CGFloat = c.alone ? 1 : c.extra != nil ? 3 : 2
        let need = buttonWidth * buttons + buttonGap * (buttons - 1) + side * 2
        let shape = RoundedRectangle(cornerRadius: 24, style: .circular)
        return VStack(spacing: 0) {
            HStack(spacing: 8) {
                if let icon = c.icon {
                    PcIconView(icon, size: 24, color: Pc.accent)
                }
                Text(verbatim: c.title)
                    .font(.system(size: 26, weight: .bold))
                    .foregroundStyle(ink)
                    .lineLimit(1)
            }
            .frame(height: 31)
            if c.message.isEmpty {
                Spacer().frame(height: 13)
            } else {
                Text(verbatim: c.message)
                    .font(.system(size: 16))
                    .foregroundStyle(ink)
                    .multilineTextAlignment(.center)
                    .lineSpacing(1)
                    .padding(.top, 13)
                    .padding(.horizontal, textSide - side)
            }
            HStack(spacing: buttonGap) {
                if !c.alone {
                    AlertButton(label: c.cancel, fill: cancelFill, ink: cancelInk) { if content != nil { onCancel() } }
                }
                if let extra = c.extra {
                    AlertButton(label: extra, fill: Pc.accent, ink: .white) { if content != nil { onExtra() } }
                }
                AlertButton(label: c.action, fill: c.destructive ? destroyFill : Pc.accent, ink: .white) { if content != nil { onAction() } }
            }
            .padding(.top, 27)
        }
        .padding(.horizontal, side)
        .padding(.top, 21)
        .padding(.bottom, 16)
        .frame(minWidth: need, maxWidth: max(mostWidth, need))
        .fixedSize(horizontal: true, vertical: true)
        .background(shape.fill(fill))
        .overlay(shape.strokeBorder(Pc.glassMenuEdge, lineWidth: 1))
        .clipShape(shape)
        // a tap on the panel itself is not one beside it
        .contentShape(shape)
        .onTapGesture {}
    }
}

private struct AlertButton: View {
    let label: String
    let fill: Color
    let ink: Color
    let action: () -> Void
    @State private var held = false

    var body: some View {
        Text(verbatim: label)
            .font(.system(size: 17, weight: .semibold))
            .foregroundStyle(ink)
            .lineLimit(1)
            .frame(maxWidth: .infinity, minHeight: 50, maxHeight: 50)
            .background(RoundedRectangle(cornerRadius: 14, style: .circular).fill(fill))
            .opacity(held ? 0.7 : 1)
            .contentShape(Rectangle())
            .modifier(PressGesture(held: $held, perform: action))
    }
}
