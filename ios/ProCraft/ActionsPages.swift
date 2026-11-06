import SwiftUI

private func themed(_ dark: UInt32, _ light: UInt32) -> Color {
    Color(argb: Look.shared.light ? light : dark)
}

/// The pages the actions panel turns to.
enum ActionsPage {
    case main, cursor, pressure, patches, plugins
}

/// A page's title with the way back.
struct PageHeader: View {
    let title: String
    let onBack: () -> Void

    var body: some View {
        HStack(spacing: 0) {
            PcIconView(PcIcons.chevronLeft, size: 22, color: Pc.panelTitle)
                .frame(width: 44, height: 34)
                .contentShape(Rectangle())
                .onTapGesture(perform: onBack)
            Text(verbatim: title)
                .font(.system(size: 22, weight: .bold))
                .foregroundStyle(Pc.panelTitle)
                .lineLimit(1)
        }
        .frame(height: 44)
        .padding(.leading, 8)
        .padding(.top, 12)
    }
}

/// Advanced cursor settings, a page of the panel: when the brush cursor shows, and how it is drawn.
struct CursorPage: View {
    let onBack: () -> Void

    private func heading(_ text: String, top: CGFloat) -> some View {
        Text(verbatim: text)
            .font(.system(size: 17, weight: .bold))
            .foregroundStyle(Look.shared.light ? Pc.tabIdle : Color(argb: 0xFFB5_B5B5))
            .padding(.leading, 14)
            .padding(.top, top)
            .padding(.bottom, 8)
    }

    var body: some View {
        let settings = AppSettings.shared
        VStack(alignment: .leading, spacing: 0) {
            PageHeader(title: "Advanced cursor", onBack: onBack)
            VStack(alignment: .leading, spacing: 0) {
                heading("Brush cursor visibility", top: 6)
                ChoiceGroup(
                    choices: ["Show while hovering", "Show while painting", "Show both"],
                    chosen: settings.cursorShows.rawValue
                ) { settings.cursorShows = CursorShows(rawValue: $0) ?? .hovering }
                heading("Brush outline style", top: 28)
                ChoiceGroup(
                    choices: ["High contrast", "Active color", "Per brush"],
                    chosen: settings.cursorStyle.rawValue
                ) { settings.cursorStyle = CursorStyle(rawValue: $0) ?? .brush }
            }
            .padding(.horizontal, 12)
            .padding(.top, 8)
            .padding(.bottom, 13)
        }
    }
}

/// Choices as rows of a rounded group, the chosen one a full blue row.
private struct ChoiceGroup: View {
    let choices: [String]
    let chosen: Int
    let onPick: (Int) -> Void

    var body: some View {
        VStack(spacing: 0) {
            ForEach(Array(choices.enumerated()), id: \.offset) { i, label in
                if i > 0, i != chosen, i - 1 != chosen {
                    Pc.actionsSeparator.frame(height: 1)
                }
                Text(verbatim: label)
                    .font(.system(size: 17))
                    .foregroundStyle(i == chosen ? Color.white : Pc.panelTitle)
                    .lineLimit(1)
                    .padding(.leading, 16)
                    .frame(maxWidth: .infinity, minHeight: 42, maxHeight: 42, alignment: .leading)
                    .background(i == chosen ? Pc.accent : Color.clear)
                    .contentShape(Rectangle())
                    .onTapGesture { onPick(i) }
            }
        }
        .background(Pc.actionsGroup)
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .circular))
    }
}

/// A slider row: the name and value on top, the bar under them; drag or tap along it.
private struct SlideRow: View {
    let label: String
    let value: String
    let fraction: Float
    let onChange: (Float) -> Void

    private let inset: CGFloat = 16
    private let tall: CGFloat = 64

    var body: some View {
        GeometryReader { g in
            let w = g.size.width
            let span = max(w - 2 * inset, 1)
            let x = inset + CGFloat(min(max(fraction, 0), 1)) * span
            let y = tall - 17.5
            ZStack(alignment: .topLeading) {
                Pc.slideTrack
                    .frame(width: span, height: 3)
                    .offset(x: inset, y: y - 1.5)
                Pc.accent
                    .frame(width: x - inset, height: 3)
                    .offset(x: inset, y: y - 1.5)
                Capsule().fill(Color.white)
                    .frame(width: 16, height: 10)
                    .offset(x: x - 8, y: y - 5)
                HStack(spacing: 0) {
                    Text(verbatim: label)
                        .foregroundStyle(Pc.slideLabel)
                    Spacer(minLength: 0)
                    Text(verbatim: value)
                        .foregroundStyle(Pc.slideValue)
                }
                .font(.system(size: 17))
                .lineLimit(1)
                .padding(.leading, 16)
                .padding(.trailing, 19)
                .padding(.top, 9)
            }
            .frame(width: w, height: tall, alignment: .topLeading)
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
                onChange(Float(min(max((drag.location.x - inset) / span, 0), 1)))
            })
        }
        .frame(height: tall)
    }
}

/// Pressure and Smoothing, a page of the panel: the app's stabilization, motion filtering and its
/// expression, Tip Attachment, the pressure curve every stroke goes through, and Reset all.
struct PressurePage: View {
    let model: CanvasModel
    let onBack: () -> Void

    /// What the curve opens over the page, where the scroll can't cut it.
    @State private var ui = StudioUi()

    private func percent(_ v: Float) -> String {
        v <= 0 ? "None" : String(format: "%.1f%%", v * 100)
    }

    private func group<Rows: View>(@ViewBuilder _ rows: () -> Rows) -> some View {
        VStack(spacing: 0) {
            rows()
        }
        .background(Pc.actionsGroup)
        .clipShape(RoundedRectangle(cornerRadius: 22, style: .circular))
    }

    private func slide(_ label: String, _ value: Float, _ set: @escaping (Float) -> Void) -> some View {
        SlideRow(label: label, value: percent(value), fraction: value) {
            set($0)
            model.applyInput()
        }
    }

    private var curve: [CGPoint] {
        let flat = AppSettings.shared.inputCurve
        return stride(from: 0, to: flat.count - 1, by: 2).map { CGPoint(x: CGFloat(flat[$0]), y: CGFloat(flat[$0 + 1])) }
    }

    private func reset() {
        let settings = AppSettings.shared
        settings.inputStabilization = 0
        settings.inputMotionFiltering = 0
        settings.inputMotionExpression = 0
        settings.inputPressureSmoothing = 0
        settings.inputCurve = [0, 0, 1, 1]
        model.applyInput()
    }

    var body: some View {
        let settings = AppSettings.shared
        VStack(alignment: .leading, spacing: 0) {
            PageHeader(title: "Pressure & Smoothing", onBack: onBack)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    group {
                        slide("Stabilization", settings.inputStabilization) { settings.inputStabilization = $0 }
                        slide("Motion Filtering", settings.inputMotionFiltering) { settings.inputMotionFiltering = $0 }
                        slide("Motion Filtering Expression", settings.inputMotionExpression) { settings.inputMotionExpression = $0 }
                    }
                    Color.clear.frame(height: 16)
                    // the stroke always starts at the pen's tip here
                    group {
                        HStack(spacing: 0) {
                            Text(verbatim: "Tip Attachment")
                                .font(.system(size: 17))
                                .foregroundStyle(Pc.actionsDisabled)
                                .lineLimit(1)
                            Spacer(minLength: 0)
                            PcSwitch(on: true, enabled: false)
                        }
                        .padding(.leading, 16)
                        .padding(.trailing, 8)
                        .frame(height: 44)
                    }
                    Color.clear.frame(height: 24)
                    Text(verbatim: "App Pressure Sensitivity")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(Pc.slideLabel)
                        .frame(height: 19)
                        .padding(.leading, 13)
                    PressureCurve(
                        points: curve,
                        grid: themed(0xFF35_3535, 0xFFCA_CACA),
                        edge: themed(0xFF66_6666, 0xFF99_9999),
                        line: Look.shared.light ? .black : .white,
                        ui: ui
                    ) { points in
                        settings.inputCurve = points.flatMap { [Float($0.x), Float($0.y)] }
                        model.applyInput()
                    }
                    .aspectRatio(1, contentMode: .fit)
                    .background(themed(0xFF26_2626, 0xFFE7_E7E9))
                    .padding(.horizontal, 7)
                    Color.clear.frame(height: 27)
                    group {
                        Text(verbatim: "Reset all")
                            .font(.system(size: 17))
                            .foregroundStyle(Pc.actionsRow)
                            .padding(.leading, 16)
                            .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44, alignment: .leading)
                            .contentShape(Rectangle())
                            .onTapGesture { reset() }
                    }
                }
                .padding(.horizontal, 12)
                .padding(.top, 20)
                .padding(.bottom, 13)
            }
            .scrollIndicators(.hidden)
            .frame(maxHeight: 760)
            .fixedSize(horizontal: false, vertical: true)
        }
        .coordinateSpace(.named(studioSpace))
        .overlay(alignment: .topLeading) {
            if let ask = ui.nodeMenu {
                NodeMenu(ask: ask) { ui.nodeMenu = nil }
            }
        }
    }
}
