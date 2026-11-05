import SwiftUI

private let actionsWidth: CGFloat = 390

/// One row of the actions panel. Rows whose features are still to come on the iPad show dimmed.
private enum ActionRow {
    /// A button row (dimmed without an action), with its value on the right if it has one.
    case act(String, value: String? = nil, action: (() -> Void)?)
    /// A switch row (dimmed without an action).
    case toggle(String, on: Bool, change: ((Bool) -> Void)?)
    /// A group's heading, above it.
    case heading(String)
}

private struct ActionsTab {
    let label: String
    let icon: PcIcon
}

private let tabs = [
    ActionsTab(label: "Add", icon: PcIcons.tabAdd),
    ActionsTab(label: "Canvas", icon: PcIcons.tabCanvas),
    ActionsTab(label: "Share", icon: PcIcons.tabShare),
    ActionsTab(label: "Video", icon: PcIcons.tabVideo),
    ActionsTab(label: "Prefs", icon: PcIcons.tabPrefs),
    ActionsTab(label: "Help", icon: PcIcons.tabHelp),
]

/// The actions panel: six tabs of grouped rows (adding, the canvas, sharing, video, preferences,
/// help).
struct ActionsPanel: View {
    let model: CanvasModel
    let arrowX: CGFloat

    #if DEBUG
    @State private var page = ScreenCapture.actionsPage
    #else
    @State private var page = ActionsPage.main
    #endif

    var body: some View {
        Popover(width: actionsWidth, arrowX: arrowX) {
            switch page {
            case .main:
                main
            case .cursor:
                CursorPage { page = .main }
            case .pressure:
                PressurePage(model: model) { page = .main }
            }
        }
    }

    private var main: some View {
            VStack(alignment: .leading, spacing: 0) {
                Text(verbatim: "Actions")
                    .font(.system(size: 24, weight: .bold))
                    .foregroundStyle(Pc.panelTitle)
                    .frame(height: 34)
                    .padding(.leading, 24)
                    .padding(.top, 22)
                HStack(spacing: 10) {
                    ForEach(Array(tabs.enumerated()), id: \.offset) { i, tab in
                        let lit = i == model.actionsTab
                        ZStack(alignment: .top) {
                            // a 50 frame centred 24 down
                            PcIconView(tab.icon, size: 50, color: lit ? Pc.accent : Pc.tabIdle)
                                .offset(y: -1)
                            Text(verbatim: tab.label)
                                .font(.system(size: 12, weight: .medium))
                                .foregroundStyle(lit ? Pc.text : Pc.tabIdle)
                                .padding(.top, 41)
                        }
                        .frame(width: 54, height: 60, alignment: .top)
                        .contentShape(Rectangle())
                        .onTapGesture { model.actionsTab = i }
                    }
                }
                .padding(.leading, 10)
                .frame(height: 60, alignment: .top)
                ScrollView {
                    VStack(spacing: 20) {
                        ForEach(Array(groups().enumerated()), id: \.offset) { _, rows in
                            ActionsGroup(rows: rows)
                        }
                    }
                    .padding(.horizontal, 12)
                    .padding(.top, 20)
                    .padding(.bottom, 11)
                }
                .scrollIndicators(.hidden)
                .frame(maxHeight: 760)
                .fixedSize(horizontal: false, vertical: true)
            }
    }

    /// The shown tab's groups of rows.
    private func groups() -> [[ActionRow]] {
        switch model.actionsTab {
        case 0:
            return [
                [
                    .act("Insert a file", action: nil),
                    .act("Insert a photo", action: nil),
                    .act("Take a photo", action: nil),
                    .act("Add text", action: nil),
                ],
                // Paste is grey until something has been cut or copied
                [
                    .act("Cut", action: { model.cutLayer() }),
                    .act("Copy", action: { model.copyLayer() }),
                    .act("Copy canvas", action: { model.copyCanvas() }),
                    .act("Paste", action: model.canPaste ? { model.paste() } : nil),
                ],
            ]
        case 1:
            return [
                [.act("Crop & Resize", action: nil)],
                [
                    .toggle("Animation Assist", on: false, change: nil),
                    .toggle("Page Assist", on: false, change: nil),
                    .toggle("Drawing Guide", on: model.guideOn, change: { model.setGuideOn($0) }),
                    .act("Edit Drawing Guide", action: nil),
                ],
                [.toggle("Reference", on: false, change: nil)],
                [
                    .act("Flip horizontal", action: { model.flipCanvas(horizontal: true) }),
                    .act("Flip vertical", action: { model.flipCanvas(horizontal: false) }),
                ],
                [.act("Canvas information", action: nil)],
            ]
        case 2:
            return [
                [
                    .heading("Share Image"),
                    .act("ProCraft", action: nil),
                    .act("PSD", action: nil),
                    .act("PDF", action: nil),
                    .act("JPEG", action: nil),
                    .act("PNG", action: nil),
                    .act("TIFF", action: nil),
                ],
                [
                    .heading("Share Layers"),
                    .act("PDF", action: nil),
                    .act("PNG Files", action: nil),
                    .act("Animated GIF", action: nil),
                    .act("Animated PNG", action: nil),
                    .act("Animated MP4", action: nil),
                    .act("Animated HEVC", action: nil),
                ],
            ]
        case 3:
            return [
                [
                    .act("Time-lapse Replay", action: nil),
                    .toggle("Time-lapse Recording", on: true, change: nil),
                ],
                [.act("Export time-lapse video", action: nil)],
            ]
        case 4:
            return [
                [
                    .toggle("Light interface", on: AppSettings.shared.lightInterface, change: { model.changeLightInterface($0) }),
                    .toggle("Right-hand interface", on: AppSettings.shared.rightHanded, change: { AppSettings.shared.rightHanded = $0 }),
                    .act("Brush scaling", value: "Canvas", action: nil),
                    .toggle("Project canvas", on: false, change: nil),
                    .toggle("Brush cursor", on: AppSettings.shared.brushCursorOn, change: { AppSettings.shared.brushCursorOn = $0 }),
                    .act("Advanced cursor settings", action: { page = .cursor }),
                ],
                [
                    .act("Pressure and Smoothing", action: { page = .pressure }),
                    .act("Gesture controls", action: {
                        withAnimation(Motion.studioIn) { model.gestureControlsOpen = true }
                    }),
                ],
            ]
        default:
            return [
                [
                    .act("About ProCraft", action: nil),
                    .act("Licences", action: nil),
                ],
            ]
        }
    }
}

/// A group of rows on the panel's dark wash, a faint line between rows; one led by a heading shows
/// it above the group, not as a row of it.
private struct ActionsGroup: View {
    let rows: [ActionRow]

    var body: some View {
        let heading: String? = if case let .heading(label) = rows.first { label } else { nil }
        let listed = heading == nil ? rows : Array(rows.dropFirst())
        VStack(alignment: .leading, spacing: 0) {
            if let heading {
                Text(verbatim: heading)
                    .font(.system(size: 14, weight: .bold))
                    .foregroundStyle(Pc.panelTitle)
                    .padding(.leading, 13)
                    .padding(.top, 7)
                    .frame(height: 45, alignment: .topLeading)
            }
            VStack(spacing: 0) {
                ForEach(Array(listed.enumerated()), id: \.offset) { i, row in
                    if i > 0 {
                        Rectangle().fill(Pc.actionsSeparator).frame(height: 1)
                    }
                    RowView(row: row)
                }
            }
            .background(RoundedRectangle(cornerRadius: 22, style: .circular).fill(Pc.actionsGroup))
            .clipShape(RoundedRectangle(cornerRadius: 22, style: .circular))
        }
    }
}

/// A row: its label, and a value or a switch at its right; a button row turns blue while held.
private struct RowView: View {
    let row: ActionRow
    @State private var held = false

    var body: some View {
        switch row {
        case let .act(label, value, action):
            let enabled = action != nil
            HStack(spacing: 0) {
                Text(verbatim: label)
                    .font(.system(size: 17))
                    .foregroundStyle(held ? Color.white : enabled ? Pc.actionsRow : Pc.actionsDisabled)
                    .lineLimit(1)
                Spacer(minLength: 0)
                if let value {
                    Text(verbatim: value)
                        .font(.system(size: 15))
                        .foregroundStyle(held ? Color.white : Pc.tabIdle)
                        .padding(.trailing, 8)
                }
            }
            .padding(.leading, 16)
            .padding(.trailing, 8)
            .frame(height: 44)
            .background(held ? Pc.accent : Color.clear)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { _ in if enabled { held = true } }
                    .onEnded { drag in
                        held = false
                        if abs(drag.translation.height) < 10, abs(drag.translation.width) < 10 {
                            action?()
                        }
                    }
            )
        case let .toggle(label, on, change):
            HStack(spacing: 0) {
                Text(verbatim: label)
                    .font(.system(size: 17))
                    .foregroundStyle(change != nil ? Pc.actionsRow : Pc.actionsDisabled)
                    .lineLimit(1)
                Spacer(minLength: 0)
                PcSwitch(on: on, enabled: change != nil)
            }
            .padding(.leading, 16)
            .padding(.trailing, 8)
            .frame(height: 44)
            .contentShape(Rectangle())
            .onTapGesture { change?(!on) }
        case let .heading(label):
            Text(verbatim: label)
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(Pc.panelTitle)
                .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                .padding(.leading, 16)
        }
    }
}

/// A 63 x 28 switch: a blue track when on, grey when off, with a white knob that slides across.
struct PcSwitch: View {
    let on: Bool
    var enabled = true
    /// Its colours where they aren't the usual ones: fixed off, and its track when off.
    var fixedTrack: Color?
    var fixedKnob: Color?
    var offTrack: Color?

    var body: some View {
        let track = !enabled ? fixedTrack ?? Pc.switchFixed : on ? Pc.accent : offTrack ?? Pc.switchOff
        ZStack(alignment: .leading) {
            Capsule().fill(track)
            Capsule()
                .fill(enabled ? Color.white : fixedKnob ?? Pc.switchFixedKnob)
                .frame(width: 37, height: 24)
                .offset(x: 2 + (on ? 22 : 0))
        }
        .frame(width: 63, height: 28)
        .animation(.easeInOut(duration: 0.2), value: on)
    }
}

/// The actions panel's width.
let actionsPanelWidth = actionsWidth
