import SwiftUI

private let actionsWidth: CGFloat = 390

/// One row of the actions panel. Rows whose features are still to come on the iPad show dimmed.
enum ActionRow {
    /// A button row (dimmed without an action), with its value on the right if it has one.
    case act(String, value: String? = nil, action: (() -> Void)?)
    /// A switch row (dimmed without an action).
    case toggle(String, on: Bool, change: ((Bool) -> Void)?)
    /// A button row that a swipe to the left slides open, its private action's button coming in.
    case privately(String, label: String, action: (() -> Void)?, onPrivate: (() -> Void)?)
    /// A slider along the row's foot, its value at the right.
    case slide(String, value: String, share: Float, change: (Float) -> Void)
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
    /// Brush scaling's menu is open.
    @State private var scalingMenu = false

    /// Brush scaling's choices beside its row, the third of the group: its left edge 54 in from the
    /// row's right end.
    private var scalingChoices: some View {
        ZStack(alignment: .topLeading) {
            MenuDismissal { scalingMenu = false }
            GlassMenu(overPanel: true, width: 254) {
                ForEach(0 ..< 2, id: \.self) { i in
                    let chosen = AppSettings.shared.brushOnScreen == (i == 1)
                    HStack(spacing: 0) {
                        Text(verbatim: i == 0 ? "Relative to canvas" : "Relative to screen")
                            .font(.system(size: 17))
                            .foregroundStyle(Pc.text)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                        if chosen {
                            PcIconView(PcIcons.menuTick, size: 24, color: Pc.accent)
                        }
                    }
                    .padding(.leading, 20)
                    .padding(.trailing, 19)
                    .frame(height: 40)
                    .contentShape(Rectangle())
                    .onTapGesture {
                        model.changeBrushScaling(onScreen: i == 1)
                        scalingMenu = false
                    }
                }
            }
            .offset(x: actionsWidth - 12 - 54, y: 218)
        }
    }

    var body: some View {
        Popover(width: actionsWidth, arrowX: arrowX) {
            switch page {
            case .main:
                main
            case .cursor:
                CursorPage { page = .main }
            case .pressure:
                PressurePage(model: model) { page = .main }
            case .patches, .plugins:
                PluginsPage(model: model, patches: page == .patches) { page = .main }
            }
        }
        .overlay(alignment: .topLeading) {
            if scalingMenu {
                scalingChoices
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

    private func ask(_ how: InsertAsk, _ isPrivate: Bool) {
        model.insertPrivate = isPrivate
        model.insertAsk = how
    }

    /// The shown tab's groups of rows.
    private func groups() -> [[ActionRow]] {
        let settings = AppSettings.shared
        let undoRange = AppSettings.rapidUndoRange
        switch model.actionsTab {
        case 0:
            return [
                // swiped to the left, the first three and Paste put what they bring on a private layer
                [
                    .privately("Insert a file", label: "Insert a private file", action: { ask(.file, false) }, onPrivate: { ask(.file, true) }),
                    .privately("Insert a photo", label: "Insert a private photo", action: { ask(.photo, false) }, onPrivate: { ask(.photo, true) }),
                    .privately("Take a photo", label: "Take a private photo", action: { ask(.camera, false) }, onPrivate: { ask(.camera, true) }),
                    .act("Add text", action: { model.text.add() }),
                ],
                // Paste is grey until something has been cut or copied
                [
                    .act("Cut", action: { model.cutLayer() }),
                    .act("Copy", action: { model.copyLayer() }),
                    .act("Copy canvas", action: { model.copyCanvas() }),
                    .privately(
                        "Paste", label: "Paste Private",
                        action: model.canPaste ? { model.paste() } : nil,
                        onPrivate: model.canPaste ? { model.paste(isPrivate: true) } : nil
                    ),
                ],
            ]
        case 1:
            return [
                [.act("Crop & Resize", action: { model.crop.begin() })],
                [
                    .toggle("Animation Assist", on: false, change: nil),
                    .toggle("Page Assist", on: false, change: nil),
                    .toggle("Drawing Guide", on: model.guideOn, change: { model.setGuideOn($0) }),
                    // only with the guide on
                    .act("Edit Drawing Guide", action: model.guideOn ? { model.guide.open() } : nil),
                ],
                [.toggle("Reference", on: model.reference.on, change: { model.reference.setOn($0) })],
                [
                    .act("Flip horizontal", action: { model.flipCanvas(horizontal: true) }),
                    .act("Flip vertical", action: { model.flipCanvas(horizontal: false) }),
                ],
                [.act("Canvas information", action: {
                    model.closePanels()
                    withAnimation(Motion.studioIn) { model.canvasInfoOpen = true }
                })],
            ]
        case 2:
            return [
                [
                    .heading("Share Image"),
                    // ours first, then the same package named for the other app
                    .act("ProCraft", action: { model.sharePackage(suffix: ".procraft") }),
                    .act("Procreate", action: { model.sharePackage(suffix: ".procreate") }),
                    .act("PSD", action: { model.share(.psd) }),
                    .act("PDF", action: { model.pdfAsking = .pdf }),
                    .act("JPEG", action: { model.share(.jpeg) }),
                    .act("PNG", action: { model.share(.png) }),
                    .act("TIFF", action: { model.share(.tiff) }),
                ],
                [
                    .heading("Share Layers"),
                    .act("PDF", action: { model.pdfAsking = .layersPdf }),
                    .act("PNG Files", action: { model.share(.layerPngs) }),
                    .act("Animated GIF", action: nil),
                    .act("Animated PNG", action: nil),
                    .act("Animated MP4", action: nil),
                    .act("Animated HEVC", action: nil),
                ],
            ]
        case 3:
            return [
                [
                    // Replay and Export wait for something recorded
                    .act("Time-lapse Replay", action: model.timeLapse.recorded ? {
                        // the panel fades away as the replay takes the screen
                        model.fadePanels()
                        withAnimation(Motion.replayFade) { model.timeLapse.replaying = true }
                    } : nil),
                    .toggle("Time-lapse Recording", on: model.timeLapse.recording, change: { model.timeLapse.setRecording($0) }),
                ],
                [.act("Export time-lapse video", action: model.timeLapse.recorded ? { model.timeLapse.lengthAsking = true } : nil)],
            ]
        case 4:
            return [
                [
                    .toggle("Light interface", on: AppSettings.shared.lightInterface, change: { model.changeLightInterface($0) }),
                    .toggle("Right-hand interface", on: AppSettings.shared.rightHanded, change: { AppSettings.shared.rightHanded = $0 }),
                    .act("Brush scaling", value: AppSettings.shared.brushOnScreen ? "Screen" : "Canvas", action: { scalingMenu = true }),
                    .toggle("Project canvas", on: false, change: nil),
                    .toggle("Brush cursor", on: AppSettings.shared.brushCursorOn, change: { AppSettings.shared.brushCursorOn = $0 }),
                    .act("Advanced cursor settings", action: { page = .cursor }),
                ],
                [
                    .act("Pressure and Smoothing", action: { page = .pressure }),
                    .act("Gesture controls", action: {
                        withAnimation(Motion.studioIn) { model.gestureControlsOpen = true }
                    }),
                    .act("Patches", action: { page = .patches }),
                    .act("Plugins", action: { page = .plugins }),
                ],
                [
                    .slide(
                        "Rapid undo delay", value: String(format: "%.2fs", settings.rapidUndoDelay),
                        share: (settings.rapidUndoDelay - undoRange.lowerBound) / (undoRange.upperBound - undoRange.lowerBound),
                        change: { model.changeRapidUndoDelay($0) }
                    ),
                    .slide(
                        "Selection mask visibility", value: String(format: "%.1f%%", settings.selectionVisibility * 100),
                        share: settings.selectionVisibility, change: { model.changeSelectionVisibility($0) }
                    ),
                ],
                [.toggle("Size & Opacity sidebar", on: model.showSideBar, change: { model.showSideBar = $0 })],
            ]
        default:
            return [
                [
                    .act("About ProCraft", action: {
                        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""
                        model.saying = ("ProCraft", "Version \(version)")
                    }),
                    .act("Licences", action: {
                        model.closePanels()
                        withAnimation(Motion.studioIn) { model.licencesOpen = true }
                    }),
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
struct RowView: View {
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
        case let .privately(label, privateLabel, action, onPrivate):
            PrivateRow(label: label, privateLabel: privateLabel, action: action, onPrivate: onPrivate)
        case let .slide(label, value, share, change):
            ActionSlideRow(label: label, value: value, share: share, change: change)
        case let .heading(label):
            Text(verbatim: label)
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(Pc.panelTitle)
                .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                .padding(.leading, 16)
        }
    }
}

/// A row that a swipe to the left slides open, its private action's button coming in from the
/// right; let go past halfway it stays open. A tap on the row closes it again.
private struct PrivateRow: View {
    let label: String
    let privateLabel: String
    let action: (() -> Void)?
    let onPrivate: (() -> Void)?

    private let full: CGFloat = 248

    @State private var slide: CGFloat = 0
    @State private var from: CGFloat?

    var body: some View {
        ZStack(alignment: .trailing) {
            Text(verbatim: label)
                .font(.system(size: 17))
                .foregroundStyle(action != nil ? Pc.actionsRow : Pc.actionsDisabled)
                .lineLimit(1)
                .padding(.leading, 16)
                .frame(maxWidth: .infinity, alignment: .leading)
                .offset(x: -slide)
            if slide > 0 {
                Text(verbatim: privateLabel)
                    .font(.system(size: 17))
                    .foregroundStyle(onPrivate != nil ? Color(argb: 0xFFA8_A8A8) : Pc.actionsDisabled)
                    .lineLimit(1)
                    .frame(width: 245)
                    .frame(maxHeight: .infinity)
                    .background(RoundedRectangle(cornerRadius: 13, style: .circular).fill(Color(argb: 0xFF45_4545)))
                    .contentShape(Rectangle())
                    .onTapGesture {
                        slide = 0
                        onPrivate?()
                    }
                    .offset(x: full - slide)
            }
        }
        .frame(height: 44)
        .clipped()
        .contentShape(Rectangle())
        .onTapGesture {
            if slide > 0 {
                withAnimation(Motion.swipeSettle) { slide = 0 }
            } else {
                action?()
            }
        }
        .simultaneousGesture(
            DragGesture(minimumDistance: 10)
                .onChanged { drag in
                    guard from != nil || abs(drag.translation.width) > abs(drag.translation.height) else { return }
                    let start = from ?? slide
                    from = start
                    slide = min(max(start - drag.translation.width, 0), full)
                }
                .onEnded { _ in
                    guard from != nil else { return }
                    from = nil
                    withAnimation(Motion.swipeSettle) { slide = slide > full / 2 ? full : 0 }
                }
        )
    }
}

/// A row with a slider along its foot, 16 in from each side; a touch anywhere on it moves it.
struct ActionSlideRow: View {
    let label: String
    let value: String
    let share: Float
    let change: (Float) -> Void
    var onDone: (() -> Void)?

    var body: some View {
        GeometryReader { g in
            let w = g.size.width
            let span = max(w - 32, 1)
            let x = 16 + CGFloat(min(max(share, 0), 1)) * span
            ZStack(alignment: .topLeading) {
                Text(verbatim: label)
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.slideLabel)
                    .lineLimit(1)
                    .padding(.leading, 16)
                    .padding(.top, 9)
                Text(verbatim: value)
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.slideValue)
                    .lineLimit(1)
                    .padding(.trailing, 16)
                    .padding(.top, 9)
                    .frame(maxWidth: .infinity, alignment: .trailing)
                Rectangle().fill(Pc.slideTrack)
                    .frame(width: span, height: 3)
                    .offset(x: 16, y: 63 - 17.5 - 1.5)
                Rectangle().fill(Pc.accent)
                    .frame(width: max(x - 16, 0), height: 3)
                    .offset(x: 16, y: 63 - 17.5 - 1.5)
                Capsule().fill(Color.white)
                    .frame(width: 16, height: 10)
                    .offset(x: x - 8, y: 63 - 17.5 - 5)
            }
            .frame(width: w, height: 63, alignment: .topLeading)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        change(Float(min(max((drag.location.x - 16) / span, 0), 1)))
                    }
                    .onEnded { _ in onDone?() }
            )
        }
        .frame(height: 63)
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
