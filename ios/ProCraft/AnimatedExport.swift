import AVFoundation
import SwiftUI
import UIKit

private var screenFill: Color { Look.shared.light ? Color(argb: 0xFFF2_F2F2) : Color(argb: 0xFF1F_1F1F) }
private var titleInk: Color { Look.shared.light ? Color(argb: 0xFF3D_3D3D) : Color(argb: 0xFFAA_AAAA) }
private var rowInk: Color { Look.shared.light ? Color(argb: 0xFF59_5959) : Color(argb: 0xFFC1_C1C1) }
private var panelFill: Color { Look.shared.light ? Color(argb: 0xFFEB_EBEB) : Color(argb: 0xFF26_2626) }
private var panelEdge: Color { Look.shared.light ? Color(argb: 0xFFDA_DADA) : Color(argb: 0xFF36_3636) }
private var segmentTrack: Color { Look.shared.light ? Color(argb: 0xFFD5_D5D5) : Color(argb: 0xFF3C_3C3C) }
private var capsuleFill: Color { Look.shared.light ? Color(argb: 0xFFFB_FBFB) : Color(argb: 0xFF1A_1A1A) }
private var capsuleEdge: Color { Look.shared.light ? Color(argb: 0xFFE0_E0E0) : Color(argb: 0xFF37_3737) }
private var sizeInk: Color { Look.shared.light ? Color(argb: 0xFF3F_3F3F) : Color(argb: 0xFF8E_8E8E) }
private var fpsTrack: Color { Look.shared.light ? Color(argb: 0xFFDD_DDDD) : Color(argb: 0xFF33_3333) }
private let doneBlue = Color(argb: 0xFF01_90FF)
private let panelWidth: CGFloat = 400
/// Web Ready fits this, else the canvas's size (a video as large as it can be, up to the most).
private let webReadySide = 480
private let mostVideoSide = 4096
/// The preview plays at this rate whatever the export's.
private let previewFps = 15

/// The animated formats Share offers.
enum AnimatedFormat {
    case gif, png, mp4, hevc

    var title: String {
        switch self {
        case .gif: "Animated GIF"
        case .png: "Animated PNG"
        case .mp4: "Animated MP4"
        case .hevc: "Animated HEVC"
        }
    }

    var fileExtension: String {
        switch self {
        case .gif: "gif"
        case .png: "png"
        case .mp4, .hevc: "mp4"
        }
    }

    /// Made by the system's encoder a frame at a time (its progress counted here), not by the core.
    var video: Bool {
        self == .mp4 || self == .hevc
    }

    var canBeTransparent: Bool {
        self == .gif || self == .png
    }
}

/// What an animated export is asked to make.
struct AnimatedChoices: Equatable {
    var webReady = false
    var dither = true
    var perFramePalette = false
    var transparent = false
    var fps: Float = 15
}

extension CanvasModel {
    /// The animation as `format` in the exports folder, `choices.fps` frames a second: Web Ready fits
    /// 480 pixels, else the canvas's size. `meter` hears how far a video has come; it stops when
    /// `cancelled` says so. Slow: off the main thread.
    static func exportAnimation(
        _ session: CanvasSession, _ format: AnimatedFormat, _ choices: AnimatedChoices,
        meter: ExportMeter? = nil, cancelled: @escaping () -> Bool = { false }
    ) -> URL? {
        let base = exportFileName(name: session.name(), package: false)
        let url = Sharing.exportsDir.appendingPathComponent(base + "." + format.fileExtension)
        let side = UInt32(choices.webReady ? webReadySide : 0)
        do {
            switch format {
            case .gif:
                try session.exportGif(
                    path: url.path, maxSide: side, fps: choices.fps, dither: choices.dither,
                    perFramePalette: choices.perFramePalette, transparent: choices.transparent
                )
            case .png:
                try session.exportApng(path: url.path, maxSide: side, fps: choices.fps, transparent: choices.transparent)
            case .mp4, .hevc:
                let info = session.info()
                let (width, height) = VideoWriter.fit(
                    width: Int(info.width), height: Int(info.height), maxSide: choices.webReady ? webReadySide : mostVideoSide
                )
                let order = session.animationSequence()
                let made = VideoWriter.write(
                    to: url, width: width, height: height, count: order.count, hevc: format == .hevc,
                    bitsPerPixel: 4, fps: choices.fps
                ) { n, context in
                    guard !cancelled() else { return false }
                    meter?.share = Float(n) / Float(max(order.count, 1))
                    guard let frame = session.animationFrame(index: order[n], transparent: false, maxSide: UInt32(max(width, height))),
                          let picture = CanvasModel.image(frame)?.cgImage
                    else { return false }
                    context.interpolationQuality = .high
                    context.draw(picture, in: CGRect(x: 0, y: 0, width: width, height: height))
                    return true
                }
                if !made {
                    return nil
                }
            }
            return url
        } catch {
            return nil
        }
    }

    /// Share's animated export opens over everything, the panel fading away.
    func openAnimatedExport(_ format: AnimatedFormat) {
        fadePanels()
        withAnimation(Motion.studioIn) { animatedExport = format }
    }

    /// The artwork's own frame rate, which an animated export starts at: Animation Assist's, else 15.
    var artworkFps: Float {
        Float(session.animationSettings()?.fps ?? 15)
    }

    /// The animation made and shared, with the export card showing how far it has come.
    func shareAnimation(_ format: AnimatedFormat, _ choices: AnimatedChoices, done: @escaping () -> Void) {
        guard exportShown == nil else { return }
        let session = session
        session.resetExportProgress()
        exportCancelled = false
        exportShown = ProgressShown(progress: 0)
        let meter = ExportMeter()
        let poll = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            self?.exportShown = ProgressShown(progress: format.video ? meter.share : session.exportProgress())
        }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let url = Self.exportAnimation(session, format, choices, meter: meter) { self?.exportCancelled == true }
            DispatchQueue.main.async {
                poll.invalidate()
                guard let self else { return }
                if self.exportCancelled {
                    self.exportShown = nil
                } else if let url {
                    self.exportShown = ProgressShown(progress: 1, done: true)
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.45) {
                        self.exportShown = nil
                        done()
                        Sharing.share([url])
                    }
                } else {
                    self.exportShown = nil
                    self.showNotice("The animation could not be exported.", warning: true)
                }
            }
        }
    }
}

/// A file's size as the export screen shows it: "1.3 MB", "850 KB".
private func fileSize(_ bytes: Int) -> String {
    bytes >= 1_000_000 ? String(format: "%.1f MB", Double(bytes) / 1_000_000) : "\(max(bytes / 1000, 1)) KB"
}

/// Sharing the layers as an animation: the frames playing, the settings (the size, the frame rate,
/// and per format dithering and a transparent background), Cancel, and the round Done that exports
/// and shares. The file is made as the settings change, to show its size.
struct AnimatedExport: View {
    let model: CanvasModel
    let format: AnimatedFormat

    @State private var choices = AnimatedChoices()
    /// Where the frame rate slider is, starting at the artwork's own rate.
    @State private var fpsAt: Float = 0
    @State private var bytes: Int?
    @State private var exporting = false
    @State private var settingsOpen = false
    @State private var frames: [UIImage] = []
    @State private var shown = 0

    private var title: String {
        let name = model.session.name().trimmingCharacters(in: .whitespaces)
        return name.isEmpty ? "Untitled Artwork" : name
    }

    private func close() {
        withAnimation(Motion.studioOut) { model.animatedExport = nil }
    }

    private var panel: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(verbatim: format.title)
                .font(.system(size: 24, weight: .bold))
                .foregroundStyle(titleInk)
                .lineLimit(1)
                .frame(height: 36)
                .padding(.leading, 24)
                .padding(.top, 19)
                .padding(.bottom, 11.5)
            segments
            FpsSlider(share: fpsAt, value: exportFpsLabel(fps: choices.fps)) { at in
                fpsAt = at
                choices.fps = exportFpsAt(fraction: at)
            }
            if format == .gif {
                SettingLine(label: "Dithering", on: choices.dither) { choices.dither.toggle() }
                SettingLine(label: "Per frame color palette", on: choices.perFramePalette) { choices.perFramePalette.toggle() }
            }
            if format.canBeTransparent {
                SettingLine(label: "Transparent background", on: choices.transparent) { choices.transparent.toggle() }
            }
            Spacer(minLength: 0)
        }
        .frame(width: panelWidth)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(RoundedRectangle(cornerRadius: 20, style: .circular).fill(panelFill))
        .overlay(RoundedRectangle(cornerRadius: 20, style: .circular).strokeBorder(panelEdge, lineWidth: 1))
        .contentShape(Rectangle())
        .onTapGesture {}
        .padding(10)
    }

    /// Max Resolution or Web Ready, side by side in a grey track, the chosen one blue.
    private var segments: some View {
        HStack(spacing: 12) {
            ForEach(["Max Resolution", "Web Ready"].indices, id: \.self) { i in
                let on = choices.webReady == (i == 1)
                Text(verbatim: i == 0 ? "Max Resolution" : "Web Ready")
                    .font(.system(size: 15))
                    .foregroundStyle(on ? Color.white : rowInk)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Capsule().fill(on ? Pc.accent : Color.clear))
                    .contentShape(Capsule())
                    .onTapGesture { choices.webReady = i == 1 }
            }
        }
        .padding(2)
        .frame(width: 384, height: 34)
        .background(Capsule().fill(segmentTrack))
        .padding(.leading, 7.5)
    }

    private var buttons: some View {
        HStack(spacing: 13) {
            Text(verbatim: "Cancel")
                .font(.system(size: 17))
                .foregroundStyle(Color(argb: 0xFFA5_A5A5))
                .padding(.horizontal, 13)
                .frame(height: 43)
                .background(Capsule().fill(capsuleFill))
                .overlay(Capsule().strokeBorder(capsuleEdge, lineWidth: 1))
                .contentShape(Capsule())
                .onTapGesture(perform: close)
            PcIconView(PcIcons.doneTick, size: 40, color: .white)
                .frame(width: 44, height: 45)
                .background(Capsule().fill(doneBlue))
                .contentShape(Capsule())
                .onTapGesture {
                    guard !exporting else { return }
                    exporting = true
                    model.shareAnimation(format, choices) { close() }
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { exporting = false }
                }
        }
        .padding(.top, 16)
        .padding(.trailing, 16)
    }

    var body: some View {
        GeometryReader { place in
            let screen = place.size
            let wide = screen.width > screen.height
            let left: CGFloat = wide ? panelWidth + 10 : 0
            let inset: CGFloat = wide ? 41 : 24
            let boxWidth = screen.width - left - inset * 2
            let boxHeight = wide ? min(boxWidth, screen.height - 104) : boxWidth
            ZStack(alignment: .topLeading) {
                screenFill
                    .contentShape(Rectangle())
                    .onTapGesture { settingsOpen = false }
                // the frames, fitted to the box, played over and over
                ZStack {
                    if frames.indices.contains(shown) {
                        Image(uiImage: frames[shown]).resizable().scaledToFit()
                    }
                }
                .frame(width: boxWidth, height: boxHeight)
                .offset(x: left + inset, y: 110)
                .allowsHitTesting(false)
                if wide {
                    // the name and size centred between the panel and the buttons
                    VStack(spacing: 0) {
                        Text(verbatim: title)
                            .font(.system(size: 20, weight: .bold))
                            .foregroundStyle(titleInk)
                            .lineLimit(1)
                            .frame(height: 30)
                        Text(verbatim: bytes.map(fileSize) ?? "")
                            .font(.system(size: 13))
                            .foregroundStyle(sizeInk)
                            .lineLimit(1)
                            .frame(height: 20)
                    }
                    .frame(width: max(screen.width - left - 24 - 224, 0))
                    .offset(x: left + 24, y: 23)
                } else {
                    HStack(spacing: 0) {
                        PcIconView(PcIcons.chevronLeft, size: 28, color: titleInk)
                            .frame(width: 60)
                        // on two lines, as there
                        Text(verbatim: "Export\nSettings")
                            .font(.system(size: 24, weight: .bold))
                            .foregroundStyle(titleInk)
                    }
                    .padding(.trailing, 12)
                    .contentShape(Rectangle())
                    .onTapGesture { settingsOpen.toggle() }
                    .offset(x: 14, y: 20)
                }
                buttons
                    .frame(width: screen.width, alignment: .trailing)
                if wide || settingsOpen {
                    panel
                }
            }
            .task(id: "\(choices.transparent && format.canBeTransparent)-\(Int(min(boxWidth, boxHeight)))") {
                let session = model.session
                let side = UInt32(max(min(boxWidth, boxHeight) * 2, 1))
                let clear = choices.transparent && format.canBeTransparent
                let read = await Task.detached(priority: .userInitiated) { () -> [UIImage] in
                    (0 ..< session.animationFrameCount()).compactMap { i in
                        session.animationFrame(index: i, transparent: clear, maxSide: side).flatMap { CanvasModel.image($0) }
                    }
                }.value
                frames = read
                shown = 0
            }
        }
        .ignoresSafeArea()
        .onAppear {
            fpsAt = exportFpsFraction(fps: model.artworkFps)
            choices.fps = exportFpsAt(fraction: fpsAt)
        }
        .task(id: frames.count) {
            while frames.count > 1, !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(1000 / previewFps))
                shown = (shown + 1) % max(frames.count, 1)
            }
        }
        // the file as the settings make it, for its size
        .task(id: choices) {
            bytes = nil
            let session = model.session
            let asked = choices
            let made = await Task.detached(priority: .utility) { CanvasModel.exportAnimation(session, format, asked) }.value
            if !Task.isCancelled, let made {
                bytes = (try? made.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? nil
            }
        }
    }
}

/// A setting's name and its switch, 52 tall.
private struct SettingLine: View {
    let label: String
    let on: Bool
    let action: () -> Void

    var body: some View {
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 17))
                .foregroundStyle(rowInk)
                .lineLimit(1)
                .frame(width: 307, alignment: .leading)
            PcSwitch(on: on)
        }
        .padding(.leading, 16)
        .padding(.trailing, 4)
        .frame(height: 52)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
    }
}

/// "Frames per second" and the rate over a 3-tall track blue up to a small white knob; dragged
/// anywhere along it, the knob follows.
private struct FpsSlider: View {
    let share: Float
    let value: String
    let onChange: (Float) -> Void

    @State private var from: Float?

    var body: some View {
        ZStack(alignment: .topLeading) {
            HStack(spacing: 0) {
                Text(verbatim: "Frames per second")
                Spacer(minLength: 0)
                Text(verbatim: value)
            }
            .font(.system(size: 17))
            .foregroundStyle(rowInk)
            .lineLimit(1)
            .padding(.leading, 18)
            .padding(.trailing, 22)
            .padding(.top, 18)
            GeometryReader { place in
                let w = place.size.width
                // the knob's middle from 22.5 in to 23 in
                let x0: CGFloat = 22.5
                let x1 = w - 23
                let x = x0 + (x1 - x0) * CGFloat(min(max(share, 0), 1))
                ZStack(alignment: .leading) {
                    Capsule().fill(fpsTrack).frame(width: w - 36, height: 3).offset(x: 18)
                    Capsule().fill(Pc.accent).frame(width: max(x - 18, 0), height: 3).offset(x: 18)
                    RoundedRectangle(cornerRadius: 4, style: .circular)
                        .fill(Color.white)
                        .shadow(color: .black.opacity(0.19), radius: 1.5, y: 1)
                        .frame(width: 16, height: 10)
                        .offset(x: x - 8)
                }
                .frame(width: w, height: 28)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { drag in
                            let start = from ?? share
                            from = start
                            onChange(min(max(start + Float(drag.translation.width / max(x1 - x0, 1)), 0), 1))
                        }
                        .onEnded { _ in from = nil }
                )
            }
            .frame(height: 28)
            .offset(y: 36)
        }
        .frame(height: 72.5, alignment: .topLeading)
    }
}
