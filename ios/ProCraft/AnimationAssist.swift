import SwiftUI
import UIKit

private let wideBarWidth: CGFloat = 657
private let uprightBarWidth: CGFloat = 446
private let barHeight: CGFloat = 96
private let barGap: CGFloat = 25
private let barInset: CGFloat = 26
private let frameHeight: CGFloat = 40
private let mostOnion = 12
/// The longest hold a frame takes, frames.
private let mostHold = 60
/// The timeline's pictures, pixels on their longer side.
private let frameThumbSide: UInt32 = 96
private let heldFrame = Color(argb: 0xFFA3_A3A3)
private let frameEdge = Color(argb: 0xFF9A_9A9A)
private let rowLineFill = Color(argb: 0x09FF_FFFF)
private let swatchRim = Color(argb: 0xFF96_9696)
private var barFill: Color { Look.shared.light ? Color(argb: 0xFFE4_E4E4) : Color(argb: 0xFF2A_2A2A) }
private var trackFill: Color { Look.shared.light ? Color(argb: 0xFFD0_D0D0) : Color(argb: 0xFF47_4747) }
private var trackEdge: Color { Look.shared.light ? Color(argb: 0xFFBD_BDBD) : Color(argb: 0xFF5C_5C5C) }
private var barInk: Color { Look.shared.light ? Color(argb: 0xFF39_3939) : Color(argb: 0xFFC5_C5C5) }
private let barSpace = "animationBar"

/// Animation and Page Assist as the core keeps them: the frames, where the timeline is, playing.
@Observable
final class AnimationAssist {
    @ObservationIgnored private unowned let model: CanvasModel

    /// Its settings while it is on.
    private(set) var settings: AnimationSettings?
    /// The frames' layers, bottom first, and which one is current.
    private(set) var frames: [UInt64] = []
    private(set) var current: Int?
    /// The timeline's places: each frame's index, again for each frame it is held.
    private(set) var slots: [Int] = []
    /// The frame and how far into its hold the timeline was last left.
    private var heldAt = (frame: 0, place: 0)
    /// Playing: the timeline's place shown, and its frame.
    private(set) var playingSlot: Int?
    private(set) var playingFrame: Int?
    /// Each frame's picture for the timeline.
    private(set) var thumbnails: [UInt64: UIImage] = [:]
    var settingsOpen = false
    var frameOptionsOpen = false
    /// Goes up as the current frame's hold changes, for what shows it.
    private(set) var holdVersion = 0

    @ObservationIgnored private var playing: Task<Void, Never>?
    @ObservationIgnored private var reading: Task<Void, Never>?
    /// How many onion skin frames there were before they were last turned off.
    @ObservationIgnored private var onionKept: UInt32 = 1

    init(model: CanvasModel) {
        self.model = model
    }

    /// How far into its hold the current frame is shown (0: the frame itself).
    var heldPlace: Int {
        guard let f = current, heldAt.frame == f else { return 0 }
        return max(min(heldAt.place, slots.filter { $0 == f }.count - 1), 0)
    }

    /// The timeline's place in the middle.
    var currentSlot: Int? {
        guard let f = current, let first = slots.firstIndex(of: f) else { return nil }
        return first + heldPlace
    }

    /// Animation Assist (or Page Assist, `pages`) on, or off.
    func setAssist(_ on: Bool, pages: Bool) {
        stopPlaying()
        _ = pages ? model.session.setPageAssist(on: on) : model.session.setAnimationAssist(on: on)
        model.refresh()
    }

    /// Reads it again from the session, after any change to the layers.
    func refresh() {
        let session = model.session
        settings = session.animationSettings()
        guard settings != nil else {
            frames = []
            slots = []
            current = nil
            settingsOpen = false
            frameOptionsOpen = false
            return
        }
        frames = session.animationFrames()
        slots = session.animationSequence().map { Int($0) }
        current = session.currentFrame().map { Int($0) }
        readThumbnails()
    }

    private func readThumbnails() {
        reading?.cancel()
        let session = model.session
        let ids = frames
        reading = Task { @MainActor [weak self] in
            for id in ids {
                let made = await Task.detached(priority: .utility) {
                    session.frameThumbnail(id: id, maxSide: frameThumbSide).flatMap { CanvasModel.image($0) }
                }.value
                guard let self, !Task.isCancelled else { return }
                self.thumbnails[id] = made
            }
        }
    }

    func change(_ edit: (inout AnimationSettings) -> Void) {
        guard var next = settings else { return }
        edit(&next)
        model.session.setAnimationSettings(s: next)
        settings = model.session.animationSettings()
    }

    /// Place `slot` comes to the middle, its frame the one drawn on (a tap on the current one opens
    /// its options).
    func tapFrame(_ slot: Int) {
        if playingFrame != nil {
            stopPlaying()
        }
        if slot == currentSlot {
            frameOptionsOpen.toggle()
            settingsOpen = false
            return
        }
        frameOptionsOpen = false
        goTo(slot)
    }

    private func goTo(_ slot: Int) {
        guard slots.indices.contains(slot) else { return }
        let frame = slots[slot]
        heldAt = (frame, slot - (slots.firstIndex(of: frame) ?? slot))
        if frame != current, frames.indices.contains(frame) {
            model.selectLayer(frames[frame])
        }
    }

    /// The timeline dragged: playing stops and the frame's options close.
    func beginScrub() {
        stopPlaying()
        frameOptionsOpen = false
    }

    /// The timeline dragged past place `slot`: its frame shows alone, as playing shows frames.
    func scrub(_ slot: Int) {
        guard slots.indices.contains(slot) else { return }
        model.session.setPlayFrame(frame: UInt32(slots[slot]))
    }

    /// The timeline let go with place `slot` in its middle: its frame becomes the current one.
    func endScrub(_ slot: Int?) {
        if let slot {
            goTo(slot)
        }
        model.session.setPlayFrame(frame: nil)
    }

    func addFrame() {
        stopPlaying()
        frameOptionsOpen = false
        model.addLayer()
    }

    private var currentLayer: LayerInfo? {
        guard let f = current, frames.indices.contains(f) else { return nil }
        return model.layers.first { $0.id == frames[f] }
    }

    /// The frame copied; on a held place it splits there, the copy taking over.
    func duplicateFrame() {
        frameOptionsOpen = false
        guard let layer = currentLayer else { return }
        let held = heldPlace
        if held > 0 {
            if model.session.splitHeldFrame(id: layer.id, at: UInt32(held)) == nil {
                model.noRoomForLayers()
            }
            model.refresh()
        } else {
            model.duplicate(layer)
        }
    }

    /// The frame deleted; on a held place it is held one frame less.
    func deleteFrame() {
        frameOptionsOpen = false
        guard let layer = currentLayer else { return }
        if heldPlace > 0 {
            _ = model.session.dropHeldFrame(id: layer.id)
            model.refresh()
        } else {
            model.delete(layer)
        }
    }

    var hold: Int {
        _ = holdVersion
        guard let layer = currentLayer else { return 0 }
        return Int(model.session.frameHold(id: layer.id))
    }

    func changeHold(_ frames: Int) {
        guard let layer = currentLayer else { return }
        _ = model.session.changeFrameHold(id: layer.id, frames: UInt32(min(max(frames, 0), mostHold)), done: false)
        slots = model.session.animationSequence().map { Int($0) }
        holdVersion += 1
    }

    /// The hold's slider let go: the whole drag is one undo step.
    func endHold() {
        guard let layer = currentLayer else { return }
        _ = model.session.changeFrameHold(id: layer.id, frames: UInt32(hold), done: true)
        model.refresh()
    }

    func togglePlaying() {
        if playingFrame != nil {
            stopPlaying()
        } else {
            play()
        }
    }

    private func play() {
        guard let first = settings else { return }
        let session = model.session
        // the frames in playing order, each as often as it is held (the core's)
        let order = session.animationSequence().map { Int($0) }
        guard !order.isEmpty else { return }
        settingsOpen = false
        frameOptionsOpen = false
        playing?.cancel()
        playing = Task { @MainActor [weak self] in
            var at = 0
            var step: Int32 = 1
            while !Task.isCancelled {
                guard let self else { return }
                let frame = order[at]
                self.playingFrame = frame
                self.playingSlot = at
                session.setPlayFrame(frame: UInt32(frame))
                let fps = max(Int(self.settings?.fps ?? first.fps), 1)
                try? await Task.sleep(for: .milliseconds(1000 / fps))
                // round again, turned back, or (a single pass) done, its last frame still shown
                guard let next = animationNextStep(playback: first.playback, len: UInt32(order.count), at: UInt32(at), step: step) else { return }
                at = Int(next.at)
                step = next.step
            }
        }
    }

    func stopPlaying() {
        playing?.cancel()
        playing = nil
        if playingFrame != nil {
            playingFrame = nil
            playingSlot = nil
            model.session.setPlayFrame(frame: nil)
        }
    }

    /// The next or previous frame drawn on (`by` frames along), staying in its frames.
    func step(_ by: Int) {
        guard let at = current, !frames.isEmpty else { return }
        let to = min(max(at + by, 0), frames.count - 1)
        if to != at, let slot = slots.firstIndex(of: to) {
            tapFrame(slot)
        }
    }

    /// Onion skins off, or back to as many as they were.
    func toggleOnionSkin() {
        change { a in
            if a.onionFrames > 0 {
                onionKept = a.onionFrames
                a.onionFrames = 0
            } else {
                a.onionFrames = onionKept
            }
        }
    }
}

/// Animation Assist's bar at the bottom: Play, Settings and Add Frame over the timeline of frames,
/// the current one in the middle; with its settings or the current frame's options above it.
struct AnimationBar: View {
    let model: CanvasModel
    let screen: CGSize

    /// Where Settings' middle is along the bar, for the arrow of its popover.
    @State private var settingsAt: CGFloat = 281.5
    /// Onion skin colors opens a page in the settings' place.
    @State private var colorsPage = false

    private var tool: AnimationAssist {
        model.animationAssist
    }

    private func word(_ label: String, bold: Bool = false, action: (() -> Void)?) -> some View {
        Text(verbatim: label)
            .font(.system(size: 17, weight: bold ? .bold : .regular))
            .foregroundStyle(barInk)
            .lineLimit(1)
            .frame(height: 30)
            .contentShape(Rectangle())
            .onTapGesture { action?() }
    }

    var body: some View {
        if let settings = tool.settings {
            let width = screen.width > screen.height ? wideBarWidth : uprightBarWidth
            let pages = settings.pages
            VStack(alignment: .leading, spacing: 0) {
                if tool.settingsOpen {
                    DownPopover(width: 290, arrowX: 145) {
                        if colorsPage {
                            OnionColorsPage(tool: tool, settings: settings) { colorsPage = false }
                        } else {
                            AnimationSettingsPage(tool: tool, settings: settings) { colorsPage = true }
                        }
                    }
                    .offset(x: min(max(settingsAt - 145, 0), width - 290))
                } else if tool.frameOptionsOpen {
                    DownPopover(width: 260, arrowX: 130) {
                        FrameOptionsPage(model: model, tool: tool, settings: settings)
                    }
                    .offset(x: width / 2 - 130)
                }
                ZStack(alignment: .topLeading) {
                    HStack(spacing: 0) {
                        word(pages ? "Page Assist" : tool.playingFrame != nil ? "Pause" : "Play", bold: true,
                             action: pages ? nil : { tool.togglePlaying() })
                        Spacer(minLength: 0)
                        if !pages {
                            word("Settings") {
                                colorsPage = false
                                tool.settingsOpen.toggle()
                                tool.frameOptionsOpen = false
                            }
                            .background {
                                GeometryReader { place in
                                    Color.clear.onChange(of: place.frame(in: .named(barSpace)), initial: true) { _, frame in
                                        settingsAt = frame.midX
                                    }
                                }
                            }
                        }
                        word(pages ? "New Page" : "Add Frame") { tool.addFrame() }
                            .padding(.leading, 23)
                    }
                    .padding(.horizontal, barInset)
                    .padding(.top, 8)
                    Timeline(model: model, tool: tool, width: width - 24)
                        .offset(x: 12, y: 42)
                    Capsule()
                        .fill(Pc.accent)
                        .frame(width: 24, height: 3)
                        .offset(x: width / 2 - 12, y: 89)
                }
                .frame(width: width, height: barHeight, alignment: .topLeading)
                .background(RoundedRectangle(cornerRadius: 30, style: .circular).fill(barFill))
                .coordinateSpace(.named(barSpace))
                // the whole bar takes its touches: none reach the canvas under it (a swipe would paint)
                .contentShape(Rectangle())
                .onTapGesture {}
                .padding(.bottom, barGap)
            }
            .frame(width: width)
            .onChange(of: tool.settingsOpen) { _, open in
                if !open {
                    colorsPage = false
                }
            }
            #if DEBUG
            .onAppear { colorsPage = ScreenCapture.animationColors }
            #endif
        }
    }
}

/// The frames in a rounded track, the current (or played) one in the middle; each frame's card is 40
/// tall and as wide as the canvas's shape makes it. Dragged, the track scrolls through the frames,
/// each showing as it passes the middle; let go, it settles on a frame, which becomes the current one.
private struct Timeline: View {
    let model: CanvasModel
    let tool: AnimationAssist
    let width: CGFloat

    /// The place in the middle while a drag moves it, and where it began.
    @State private var dragged: CGFloat?
    @State private var from: CGFloat = 0
    @State private var shown = 0

    var body: some View {
        let slots = tool.slots
        let focus = tool.playingSlot ?? tool.currentSlot ?? 0
        let info = model.session.info()
        let aspect = info.width > 0 && info.height > 0 ? min(max(CGFloat(info.width) / CGFloat(info.height), 0.25), 2) : 0.66
        let frameWidth = frameHeight * aspect + 1
        let pitch = frameWidth + 1
        let last = max(slots.count - 1, 0)
        let at = dragged ?? CGFloat(focus)
        ZStack(alignment: .topLeading) {
            ForEach(Array(slots.enumerated()), id: \.offset) { i, frame in
                // a frame's held places: grey cards after it
                let held = i > 0 && slots[i - 1] == frame
                let x = width / 2 - frameWidth / 2 + pitch * (CGFloat(i) - at)
                if x > -frameWidth, x < width {
                    let card = RoundedRectangle(cornerRadius: 6, style: .circular)
                    ZStack {
                        held ? heldFrame : Color.white
                        if !held, tool.frames.indices.contains(frame), let picture = tool.thumbnails[tool.frames[frame]] {
                            Image(uiImage: picture).resizable().scaledToFit()
                        }
                    }
                    .frame(width: frameWidth, height: frameHeight)
                    .clipShape(card)
                    .overlay(card.strokeBorder(frameEdge, lineWidth: 1))
                    .contentShape(card)
                    .onTapGesture { tool.tapFrame(i) }
                    .offset(x: x, y: 2)
                }
            }
        }
        .frame(width: width, height: 44, alignment: .topLeading)
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .circular))
        .background(RoundedRectangle(cornerRadius: 18, style: .circular).fill(trackFill))
        .overlay(RoundedRectangle(cornerRadius: 18, style: .circular).strokeBorder(trackEdge, lineWidth: 1))
        .animation(tool.playingFrame != nil || dragged != nil ? nil : Motion.frameSlide, value: focus)
        .gesture(
            DragGesture(minimumDistance: 6)
                .onChanged { drag in
                    guard slots.count > 1 else { return }
                    if dragged == nil {
                        from = CGFloat(focus)
                        shown = focus
                        tool.beginScrub()
                    }
                    let p = min(max(from - drag.translation.width / pitch, 0), CGFloat(last))
                    dragged = p
                    let i = Int(p.rounded())
                    if i != shown {
                        shown = i
                        tool.scrub(i)
                    }
                }
                .onEnded { drag in
                    guard dragged != nil else { return }
                    // it coasts as far as the flick would take it, and settles on a frame
                    let target = Int(min(max(from - drag.predictedEndTranslation.width / pitch, 0), CGFloat(last)).rounded())
                    withAnimation(Motion.frameSlide) { dragged = CGFloat(target) }
                    tool.endScrub(target)
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { dragged = nil }
                }
        )
    }
}

/// A panel standing on the bar, its arrow pointing down at `arrowX` from its left edge.
private struct DownPopover<Content: View>: View {
    let width: CGFloat
    let arrowX: CGFloat
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            content()
        }
        .padding(.horizontal, 8)
        .padding(.top, 8)
        .padding(.bottom, 8)
        .frame(width: width, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 24, style: .circular).fill(Pc.popoverFill))
        .padding(.bottom, Pc.arrowHeight)
        .background(alignment: .bottomLeading) {
            Path { p in
                let h = Pc.arrowHeight
                p.move(to: CGPoint(x: arrowX - h, y: 0))
                p.addLine(to: CGPoint(x: arrowX, y: h))
                p.addLine(to: CGPoint(x: arrowX + h, y: 0))
                p.closeSubpath()
            }
            .fill(Pc.popoverFill)
            .frame(width: width, height: Pc.arrowHeight)
        }
        .padding(.bottom, 2)
        .contentShape(Rectangle())
        .onTapGesture {}
    }
}

/// The faint 2 pt line between a group's rows.
private struct RowLine: View {
    var body: some View {
        rowLineFill.frame(height: 2)
    }
}

private func group<Rows: View>(@ViewBuilder _ rows: () -> Rows) -> some View {
    VStack(spacing: 0) {
        rows()
    }
    .frame(maxWidth: .infinity)
    .background(Pc.actionsGroup)
    .clipShape(RoundedRectangle(cornerRadius: 18, style: .circular))
}

/// A group of sliders: `top` above the first, 11.5 between.
private func sliderGroup<Rows: View>(top: CGFloat = 8, @ViewBuilder _ rows: () -> Rows) -> some View {
    VStack(spacing: 11.5) {
        rows()
    }
    .padding(.top, top)
    .frame(maxWidth: .infinity)
    .background(Pc.actionsGroup)
    .clipShape(RoundedRectangle(cornerRadius: 18, style: .circular))
}

private func title(_ text: String, bottom: CGFloat) -> some View {
    Text(verbatim: text)
        .font(.system(size: 23, weight: .bold))
        .foregroundStyle(Pc.panelTitle)
        .lineLimit(1)
        .padding(.leading, 14)
        .padding(.top, 11)
        .padding(.bottom, bottom)
}

private struct ToggleLine: View {
    let label: String
    let on: Bool
    let height: CGFloat
    let action: () -> Void

    var body: some View {
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 17))
                .foregroundStyle(Pc.panelTitle)
                .lineLimit(1)
            Spacer(minLength: 0)
            PcSwitch(on: on)
        }
        .padding(.leading, 16)
        .padding(.trailing, 6)
        .frame(height: height)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
    }
}

private struct ActLine: View {
    let label: String
    var height: CGFloat = 44
    let action: () -> Void

    var body: some View {
        Text(verbatim: label)
            .font(.system(size: 17))
            .foregroundStyle(Pc.panelTitle)
            .lineLimit(1)
            .padding(.leading, 16)
            .frame(maxWidth: .infinity, minHeight: height, maxHeight: height, alignment: .leading)
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
    }
}

/// A setting's name and value over a bar dragged or tapped along.
private struct ValueSlider: View {
    let label: String
    let value: String
    let share: Float
    var onEnd: () -> Void = {}
    let onChange: (Float) -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                Text(verbatim: label)
                    .foregroundStyle(Pc.slideLabel)
                    .lineLimit(1)
                Spacer(minLength: 0)
                Text(verbatim: value)
                    .foregroundStyle(Pc.slideValue)
                    .lineLimit(1)
            }
            .font(.system(size: 17))
            .padding(.top, 6.5)
            GeometryReader { place in
                let w = place.size.width
                let x = CGFloat(min(max(share, 0), 1)) * w
                ZStack(alignment: .leading) {
                    Rectangle().fill(Pc.slideTrack).frame(height: 3)
                    Rectangle().fill(Pc.accent).frame(width: x, height: 3)
                    Capsule().fill(Color.white)
                        .frame(width: 16, height: 10)
                        .offset(x: min(max(x - 8, 0), max(w - 16, 0)))
                }
                .frame(width: w, height: 22)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { onChange(Float(min(max($0.location.x / max(w, 1), 0), 1))) }
                        .onEnded { _ in onEnd() }
                )
            }
            .frame(height: 22)
        }
        .padding(.horizontal, 16)
        .frame(height: 55, alignment: .top)
    }
}

/// Animation Assist's settings: playback, frames a second, onion skins, the primary frame.
private struct AnimationSettingsPage: View {
    let tool: AnimationAssist
    let settings: AnimationSettings
    let onColors: () -> Void

    var body: some View {
        let ways: [(AnimationPlayback, String)] = [(.loop, "Loop"), (.pingPong, "Ping-Pong"), (.oneShot, "One Shot")]
        let onion = Int(settings.onionFrames)
        VStack(alignment: .leading, spacing: 0) {
            title("Settings", bottom: 21)
            group {
                ForEach(ways.indices, id: \.self) { i in
                    let (way, label) = ways[i]
                    let chosen = settings.playback == way
                    if i > 0 {
                        RowLine()
                    }
                    HStack(spacing: 0) {
                        Text(verbatim: label)
                            .font(.system(size: 17))
                            .foregroundStyle(chosen ? Color.white : Pc.panelTitle)
                        Spacer(minLength: 0)
                        if chosen {
                            PcIconView(PcIcons.menuTick, size: 22, color: .white)
                        }
                    }
                    .padding(.horizontal, 16)
                    .frame(height: 40)
                    .background(chosen ? Pc.accent : Color.clear)
                    .contentShape(Rectangle())
                    .onTapGesture { tool.change { $0.playback = way } }
                }
            }
            sliderGroup {
                ValueSlider(label: "Frames Per Second", value: "\(settings.fps)", share: Float(settings.fps - 1) / 59) { v in
                    tool.change { $0.fps = UInt32(1 + wholeNumber(v * 59)) }
                }
                ValueSlider(
                    label: "Onion skin frames", value: onion == 0 ? "None" : onion == mostOnion ? "Max" : "\(onion)",
                    share: Float(onion) / Float(mostOnion)
                ) { v in
                    tool.change { $0.onionFrames = UInt32(wholeNumber(v * Float(mostOnion))) }
                }
                ValueSlider(label: "Onion skin opacity", value: "\(wholeNumber(settings.onionOpacity * 100)) %", share: settings.onionOpacity) { v in
                    tool.change { $0.onionOpacity = v }
                }
            }
            .padding(.top, 10)
            group {
                ToggleLine(label: "Blend primary frame", on: settings.blendPrimary, height: 44.5) {
                    tool.change { $0.blendPrimary.toggle() }
                }
                RowLine()
                ActLine(label: "Onion skin colors", height: 44.5, action: onColors)
            }
            .padding(.top, 9)
        }
    }
}

/// The onion skins' colours: back to the settings, "Colors" and Reset over two swatches (the frames
/// before the current one, then those after; a tap picks which the picker below edits) and the
/// compact picker with its mode tabs. Black leaves the skins as they are.
private struct OnionColorsPage: View {
    let tool: AnimationAssist
    let settings: AnimationSettings
    let onBack: () -> Void

    @State private var after = false
    @State private var picked: HeldColor?

    private static func rgb(_ v: [Float]) -> UInt32 {
        v.prefix(3).reduce(UInt32(0)) { ($0 << 8) | UInt32(wholeNumber(min(max($1, 0), 1) * 255)) }
    }

    private static func unit(_ rgb: UInt32) -> [Float] {
        [(rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF].map { Float($0) / 255 }
    }

    private func take(after: Bool) {
        self.after = after
        let tool = tool
        picked = HeldColor(rgb: Self.rgb(after ? settings.after : settings.before)) { rgb in
            tool.change { a in
                if after {
                    a.after = Self.unit(rgb)
                } else {
                    a.before = Self.unit(rgb)
                }
            }
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 0) {
                PcIconView(PcIcons.chevronLeft, size: 34, color: Pc.slideValue)
                    .frame(width: 44, height: 34)
                    .contentShape(Rectangle())
                    .onTapGesture(perform: onBack)
                Text(verbatim: "Colors")
                    .font(.system(size: 23, weight: .bold))
                    .foregroundStyle(Pc.panelTitle)
                Spacer(minLength: 0)
                Text(verbatim: "Reset")
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.slideValue)
                    .padding(.leading, 12)
                    .contentShape(Rectangle())
                    .onTapGesture {
                        tool.change {
                            $0.before = [0, 0, 0]
                            $0.after = [0, 0, 0]
                        }
                        take(after: after)
                    }
            }
            .frame(height: 34)
            .offset(x: -8)
            HStack(spacing: 5) {
                ForEach([false, true], id: \.self) { which in
                    let shape = RoundedRectangle(cornerRadius: 6, style: .circular)
                    shape.fill(Color(argb: 0xFF00_0000 | Self.rgb(which ? settings.after : settings.before)))
                        .frame(width: 42, height: 24)
                        .padding(1)
                        .background(RoundedRectangle(cornerRadius: 7, style: .circular).fill(swatchRim))
                        .contentShape(shape)
                        .onTapGesture { take(after: which) }
                }
            }
            .padding(.leading, 13)
            .padding(.top, 7)
            if let picked {
                CompactPicker(c: picked)
                    .id(after)
                    .padding(.top, 11)
                    .frame(maxWidth: .infinity)
            }
        }
        .onAppear { take(after: false) }
    }
}

/// The current frame's options: how long it is held, Duplicate, Delete, and Foreground for the last
/// frame and Background for the first (both for a lone frame). Pages: Duplicate, Delete, and
/// Background for the first.
private struct FrameOptionsPage: View {
    let model: CanvasModel
    let tool: AnimationAssist
    let settings: AnimationSettings

    var body: some View {
        if let index = tool.current {
            let hold = tool.hold
            // (a held place has neither)
            let foreground = !settings.pages && index == tool.frames.count - 1 && tool.heldPlace == 0
            let background = index == 0 && tool.heldPlace == 0
            VStack(alignment: .leading, spacing: 0) {
                title(settings.pages ? "Page options" : "Frame options", bottom: 15)
                if !settings.pages {
                    sliderGroup(top: 4) {
                        ValueSlider(
                            label: "Hold duration", value: hold == 0 ? "None" : "\(hold)",
                            share: Float(hold) / Float(mostHold), onEnd: { tool.endHold() }
                        ) { v in
                            tool.changeHold(wholeNumber(v * Float(mostHold)))
                        }
                    }
                    .padding(.bottom, 10)
                }
                group {
                    ActLine(label: "Duplicate") { tool.duplicateFrame() }
                    RowLine()
                    ActLine(label: "Delete") { tool.deleteFrame() }
                }
                if foreground || background {
                    group {
                        if foreground {
                            ToggleLine(label: "Foreground", on: settings.foreground, height: 44) {
                                tool.change { $0.foreground.toggle() }
                            }
                        }
                        if foreground, background {
                            RowLine()
                        }
                        if background {
                            ToggleLine(label: "Background", on: settings.background, height: 44) {
                                tool.change { $0.background.toggle() }
                            }
                        }
                    }
                    .padding(.top, 10)
                }
            }
        }
    }
}
