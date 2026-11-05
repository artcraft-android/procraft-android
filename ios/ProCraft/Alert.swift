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
    /// The left button is a choice of its own (blue), not Cancel.
    var onFirst: (() -> Void)?

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
                    if let onFirst {
                        AlertButton(label: c.cancel, fill: Pc.accent, ink: .white) { if content != nil { onFirst() } }
                    } else {
                        AlertButton(label: c.cancel, fill: cancelFill, ink: cancelInk) { if content != nil { onCancel() } }
                    }
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

/// The card over everything while a file is made or brought in (`shown` not nil): `title` after
/// its glyph, a file's name under it when there is one, a pie filling clockwise from the top with
/// how far it has come, and Cancel if it can be; once done the pie is full and a check is drawn in it. It comes
/// and goes as an alert does, the screen dimmed; nothing under it is touched meanwhile.
struct ProgressCard: View {
    let shown: ProgressShown?
    let title: String
    let glyph: PcIcon
    let onCancel: (() -> Void)?

    /// The last one shown, kept while it goes.
    @State private var kept: ProgressShown?
    @State private var up = false
    @State private var check: CGFloat = 0

    private func pie(_ c: ProgressShown) -> some View {
        Canvas { context, size in
            let r = min(size.width, size.height) / 2
            let centre = CGPoint(x: size.width / 2, y: size.height / 2)
            let disc = Path(ellipseIn: CGRect(x: 0, y: 0, width: size.width, height: size.height))
            if c.done {
                context.fill(disc, with: .color(Pc.accent))
                // the check drawn along its length as it shows
                var mark = Path()
                mark.move(to: CGPoint(x: centre.x - 14, y: centre.y + 1))
                mark.addLine(to: CGPoint(x: centre.x - 4, y: centre.y + 11))
                mark.addLine(to: CGPoint(x: centre.x + 16, y: centre.y - 10))
                context.stroke(
                    mark.trimmedPath(from: 0, to: check), with: .color(.white),
                    style: StrokeStyle(lineWidth: 6, lineCap: .round, lineJoin: .round)
                )
            } else {
                context.stroke(
                    Path(ellipseIn: CGRect(x: 2, y: 2, width: size.width - 4, height: size.height - 4)),
                    with: .color(Pc.accent), lineWidth: 4
                )
                var wedge = Path()
                wedge.move(to: centre)
                wedge.addArc(
                    center: centre, radius: r, startAngle: .degrees(-90),
                    endAngle: .degrees(-90 + 360 * Double(min(max(c.progress, 0), 1))), clockwise: false
                )
                wedge.closeSubpath()
                context.fill(wedge, with: .color(Pc.accent))
            }
        }
        .frame(width: 76, height: 76)
    }

    private func card(_ c: ProgressShown) -> some View {
        let shape = RoundedRectangle(cornerRadius: 24, style: .circular)
        return VStack(spacing: 0) {
            HStack(spacing: c.detail != nil ? 12 : 13) {
                PcIconView(glyph, size: 59, color: Pc.accent)
                    .frame(width: c.detail != nil ? 16.5 : 22, height: 31)
                    .offset(y: c.detail != nil ? 3.5 : 0)
                Text(verbatim: title)
                    .font(.system(size: 26, weight: .bold))
                    .foregroundStyle(ink)
                    .lineLimit(1)
            }
            .frame(height: 31)
            if let detail = c.detail {
                Text(verbatim: detail)
                    .font(.system(size: 18))
                    .foregroundStyle(ink)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .frame(height: 58)
            } else {
                Spacer().frame(height: 40)
            }
            pie(c)
            Spacer().frame(height: onCancel != nil ? 22 : 7)
            if let onCancel {
                AlertButton(label: "Cancel", fill: cancelFill, ink: cancelInk) {
                    if shown != nil, !c.done {
                        onCancel()
                    }
                }
            }
        }
        .padding(.horizontal, 17)
        .padding(.top, c.detail != nil ? 25.5 : 21)
        .padding(.bottom, 15)
        .frame(width: 398)
        .background(shape.fill(fill))
        .overlay(shape.strokeBorder(Pc.glassMenuEdge, lineWidth: 1))
        .clipShape(shape)
    }

    var body: some View {
        ZStack {
            if let c = kept {
                Color.black.opacity(up ? dim : 0)
                    .ignoresSafeArea()
                    .contentShape(Rectangle())
                    .onTapGesture {}
                card(c)
                    .opacity(up ? 1 : 0)
                    .scaleEffect(up ? 1 : startScale)
            }
        }
        .onChange(of: shown, initial: true) { _, next in
            if let next {
                let first = kept == nil
                kept = next
                if first {
                    withAnimation(Motion.alertIn) { up = true }
                }
                if next.done {
                    withAnimation(.linear(duration: 0.3)) { check = 1 }
                }
            } else if kept != nil {
                withAnimation(Motion.alertOut) { up = false } completion: {
                    kept = nil
                    check = 0
                }
            }
        }
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
