import AVFoundation
import SwiftUI
import UIKit

/// The replay's and the video's frames a second.
private let framesASecond = Int32(timeLapseFps())
/// A video's bit rate is never less than this, bits a second.
private let leastBitRate = 1_000_000

// the canvas's surround and grid, light with the light interface
private var replayFill: Color { Look.shared.light ? Color(argb: 0xFFCE_D1D2) : Color(argb: 0xFF25_2525) }
private var replayGrid: Color { Look.shared.light ? Color(argb: 0xFFCA_CDCE) : Color(argb: 0xFF2E_2E2E) }

/// Time-lapse videos, by the system's codecs.
enum VideoWriter {
    /// The largest size no bigger than `width` x `height` (same shape, even sides, longest side at
    /// most `maxSide`).
    static func fit(width: Int, height: Int, maxSide: Int) -> (Int, Int) {
        let scale = min(1, Double(maxSide) / Double(max(width, height, 1)))
        func even(_ side: Int) -> Int {
            max(Int(Double(side) * scale) / 2 * 2, 2)
        }
        return (even(width), even(height))
    }

    /// `count` frames, `width` x `height` (even), as an MP4 at `url`. `draw` paints frame n in a
    /// context the frame's size and says whether it did. False if the video wasn't made; nothing is
    /// left at `url` then.
    static func write(
        to url: URL, width: Int, height: Int, count: Int, hevc: Bool, bitsPerPixel: Int,
        draw: (Int, CGContext) -> Bool
    ) -> Bool {
        try? FileManager.default.removeItem(at: url)
        guard let writer = try? AVAssetWriter(outputURL: url, fileType: .mp4) else { return false }
        let settings: [String: Any] = [
            AVVideoCodecKey: hevc ? AVVideoCodecType.hevc : AVVideoCodecType.h264,
            AVVideoWidthKey: width,
            AVVideoHeightKey: height,
            AVVideoCompressionPropertiesKey: [AVVideoAverageBitRateKey: max(width * height * bitsPerPixel, leastBitRate)],
        ]
        let input = AVAssetWriterInput(mediaType: .video, outputSettings: settings)
        input.expectsMediaDataInRealTime = false
        let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
            kCVPixelBufferWidthKey as String: width,
            kCVPixelBufferHeightKey as String: height,
        ])
        guard writer.canAdd(input) else { return false }
        writer.add(input)
        guard writer.startWriting() else { return false }
        writer.startSession(atSourceTime: .zero)
        let space = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
        let layout = CGImageAlphaInfo.noneSkipFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue
        var whole = true
        for n in 0 ..< count {
            while !input.isReadyForMoreMediaData, writer.status == .writing {
                Thread.sleep(forTimeInterval: 0.002)
            }
            var made: CVPixelBuffer?
            guard writer.status == .writing, let pool = adaptor.pixelBufferPool,
                  CVPixelBufferPoolCreatePixelBuffer(nil, pool, &made) == kCVReturnSuccess, let buffer = made
            else {
                whole = false
                break
            }
            CVPixelBufferLockBaseAddress(buffer, [])
            let context = CGContext(
                data: CVPixelBufferGetBaseAddress(buffer), width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: CVPixelBufferGetBytesPerRow(buffer), space: space, bitmapInfo: layout
            )
            let drawn = context.map { draw(n, $0) } ?? false
            CVPixelBufferUnlockBaseAddress(buffer, [])
            guard drawn, adaptor.append(buffer, withPresentationTime: CMTime(value: Int64(n), timescale: framesASecond)) else {
                whole = false
                break
            }
        }
        input.markAsFinished()
        guard whole else {
            writer.cancelWriting()
            try? FileManager.default.removeItem(at: url)
            return false
        }
        let finished = DispatchSemaphore(value: 0)
        writer.finishWriting { finished.signal() }
        finished.wait()
        return writer.status == .completed
    }

    /// `parts` one after another as one video at `out`, nothing made again. False if they can't be.
    static func join(_ parts: [URL], to out: URL) -> Bool {
        let joined = AVMutableComposition()
        guard let track = joined.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid) else { return false }
        var at = CMTime.zero
        for part in parts {
            let asset = AVURLAsset(url: part)
            guard let source = asset.tracks(withMediaType: .video).first else { return false }
            let range = CMTimeRange(start: .zero, duration: asset.duration)
            guard (try? track.insertTimeRange(range, of: source, at: at)) != nil else { return false }
            at = CMTimeAdd(at, asset.duration)
        }
        guard let export = AVAssetExportSession(asset: joined, presetName: AVAssetExportPresetPassthrough) else { return false }
        try? FileManager.default.removeItem(at: out)
        export.outputURL = out
        export.outputFileType = .mp4
        let finished = DispatchSemaphore(value: 0)
        export.exportAsynchronously { finished.signal() }
        finished.wait()
        return export.status == .completed
    }
}

/// An artwork's time-lapse: a picture of the whole canvas after each change, kept as numbered frames
/// in the artwork's folder, replayed or made into a video. The core keeps the frames, the settings
/// and a package's videos; this makes and reads the videos with the system's codecs.
///
/// As the artwork file keeps a time-lapse, its frames end up in videos only: a package's come in as
/// they are (`take`), the frames recorded become one more when the artwork is left or shared
/// (`forPackage`), their pictures then gone; `frame` reads those from the videos.
final class ArtworkTimeLapse {
    private let core: TimeLapse
    private let lock = NSLock()
    /// The videos the frames are in, and the one being read.
    private var clips: [TimeLapseClip]?
    private var reading: (path: String, generator: AVAssetImageGenerator)?

    init(dir: String) {
        core = TimeLapse(artwork: dir)
    }

    /// The video's size, quality and codec, kept with the artwork.
    var settings: TimeLapseSettings {
        get { core.settings() }
        set { core.setSettings(s: newValue) }
    }

    /// Frames recorded so far.
    var count: Int {
        Int(core.count())
    }

    /// Whether changes are recorded (on unless turned off for this artwork).
    var recording: Bool {
        get { core.recording() }
        set { core.setRecording(on: newValue) }
    }

    /// What the recorded frames take on disk, bytes.
    var bytes: UInt64 {
        core.bytes()
    }

    /// Keeps a frame unless it is the same as the last.
    func record(_ shot: ThumbnailImage) {
        _ = core.record(rgba: shot.rgba, width: shot.width, height: shot.height)
    }

    /// Deletes what has been recorded; the settings, and whether it records, stay.
    func purge() {
        core.purge()
        forgetClips()
    }

    /// Frame `i`: its picture, or read from the video it is in.
    func frame(_ i: Int) -> UIImage? {
        if let file = core.frameFile(i: UInt32(i)), let picture = UIImage(contentsOfFile: file) {
            return picture
        }
        return clipFrame(i)
    }

    private func clipFrame(_ i: Int) -> UIImage? {
        lock.lock()
        defer { lock.unlock() }
        let all = clips ?? core.clips()
        clips = all
        guard let clip = all.first(where: { i >= Int($0.first) && i - Int($0.first) < Int($0.frames) }) else { return nil }
        if reading?.path != clip.path {
            let generator = AVAssetImageGenerator(asset: AVURLAsset(url: URL(fileURLWithPath: clip.path)))
            generator.requestedTimeToleranceBefore = .zero
            generator.requestedTimeToleranceAfter = .zero
            reading = (clip.path, generator)
        }
        let at = CMTime(value: Int64(i - Int(clip.first)), timescale: framesASecond)
        return (try? reading?.generator.copyCGImage(at: at, actualTime: nil)).map { UIImage(cgImage: $0) }
    }

    /// The videos read again next time (they changed), and the one being read let go.
    private func forgetClips() {
        lock.lock()
        defer { lock.unlock() }
        clips = nil
        reading = nil
    }

    /// What it holds while reading its videos let go (the artwork is left).
    func close() {
        forgetClips()
    }

    /// The frames as an MP4 at `url`: all of them, or (with `seconds`) spread evenly over that long.
    /// `going` hears how far it has come (0..1) before each frame, and stops it by saying false. False
    /// if there are none or it wasn't made.
    func export(to url: URL, seconds: Int?, going: (Float) -> Bool) -> Bool {
        let picks = timeLapsePicks(frames: core.count(), seconds: seconds.map { UInt32($0) }).map { Int($0) }
        // all of it, as asked or too short to cut: the videos kept for packages, made where they
        // don't reach yet, put end to end, nothing read back from them
        if picks.count == count, let done = joined(to: url, going: going) {
            return done
        }
        return encode(to: url, picks: picks, size: nil, hevc: settings.hevc, going: going) != nil
    }

    /// Every frame at `out` as the package's videos joined: the frames in none yet made into one
    /// first, the videos then copied one after another, nothing made again. False if `going` stopped
    /// it (the videos kept as they were); none if they can't be joined, so it is made afresh.
    private func joined(to out: URL, going: (Float) -> Bool) -> Bool? {
        let total = count
        guard total > 0 else { return nil }
        var plan = core.segmentPlan()
        while let making = plan {
            let todo = making.frames.count
            let before = total - todo
            var stopped = false
            let size = making.width > 0 && making.height > 0 ? (Int(making.width), Int(making.height)) : nil
            let url = URL(fileURLWithPath: making.path)
            let made = encode(to: url, picks: making.frames.map { Int($0) }, size: size, hevc: making.hevc) { done in
                let on = going((Float(before) + done * Float(todo)) / Float(total))
                stopped = !on
                return on
            }
            if stopped {
                // not kept: the next one makes it again
                try? FileManager.default.removeItem(at: url)
                return false
            }
            plan = core.segmentMade(plan: making, width: UInt32(made?.0 ?? 0), height: UInt32(made?.1 ?? 0), made: made != nil)
            if made == nil, plan == nil {
                return nil
            }
        }
        dropClipFrames()
        let parts = core.forPackage().segments.map { URL(fileURLWithPath: $0) }
        return !parts.isEmpty && VideoWriter.join(parts, to: out) ? true : nil
    }

    /// The frames `picks` as a video at `url`, each fitted on white into `size` (else into the first
    /// one's, within the settings' side); its size, or none if it wasn't made. `going` as for `export`.
    private func encode(
        to url: URL, picks: [Int], size: (Int, Int)?, hevc: Bool, going: (Float) -> Bool = { _ in true }
    ) -> (Int, Int)? {
        guard let first = picks.first else { return nil }
        let video = settings
        var fitted = size
        if fitted == nil {
            guard let picture = frame(first)?.cgImage else { return nil }
            fitted = VideoWriter.fit(width: picture.width, height: picture.height, maxSide: Int(timeLapseSide(s: video)))
        }
        guard let (width, height) = fitted else { return nil }
        let made = VideoWriter.write(
            to: url, width: width, height: height, count: picks.count, hevc: hevc,
            bitsPerPixel: Int(timeLapseBitsPerPixel(s: video))
        ) { n, context in
            guard going(Float(n) / Float(picks.count)), let source = frame(picks[n])?.cgImage else { return false }
            // fitted on white
            context.setFillColor(UIColor.white.cgColor)
            context.fill(CGRect(x: 0, y: 0, width: width, height: height))
            let scale = min(CGFloat(width) / CGFloat(source.width), CGFloat(height) / CGFloat(source.height))
            let shownWidth = CGFloat(source.width) * scale
            let shownHeight = CGFloat(source.height) * scale
            context.interpolationQuality = .high
            context.draw(source, in: CGRect(
                x: (CGFloat(width) - shownWidth) / 2, y: (CGFloat(height) - shownHeight) / 2,
                width: shownWidth, height: shownHeight
            ))
            return true
        }
        return made ? (width, height) : nil
    }

    /// The time-lapse as a package keeps it: its settings, and its frames made into videos (the ones
    /// made before kept, so a share makes only what is new; the pictures they hold then gone, as the
    /// artwork file keeps a time-lapse: only as video). Leaving the artwork does this too.
    func forPackage() -> PackageTimeLapse {
        var plan = core.segmentPlan()
        while let making = plan {
            let size = making.width > 0 && making.height > 0 ? (Int(making.width), Int(making.height)) : nil
            let made = encode(
                to: URL(fileURLWithPath: making.path), picks: making.frames.map { Int($0) }, size: size, hevc: making.hevc
            )
            plan = core.segmentMade(plan: making, width: UInt32(made?.0 ?? 0), height: UInt32(made?.1 ?? 0), made: made != nil)
        }
        dropClipFrames()
        return core.forPackage()
    }

    /// The pictures its videos hold too gone (the core checks each video's frames first), once this
    /// device has read those videos back: where it can't, the pictures are all there is to replay.
    private func dropClipFrames() {
        forgetClips()
        let readable = core.clipsWithPictures().allSatisfy { clip in
            [clip.first, clip.first + clip.frames - 1].allSatisfy { clipFrame(Int($0)) != nil }
        }
        forgetClips()
        if readable {
            _ = core.dropClipFrames()
        }
    }

    /// Takes a package's time-lapse: as the package keeps it, its videos as they are (nothing decoded);
    /// else (a video whose frames can't be counted) its settings, and its videos' frames after these.
    func take(_ package: PackageTimeLapse) {
        if core.takeClips(t: package) {
            return
        }
        core.takeBegin(t: package)
        for path in package.segments {
            core.takeVideo(path: path, frames: UInt32(keepFrames(of: path)))
        }
        core.takeEnd()
    }

    /// The video at `path`'s frames kept as the next ones; how many there were.
    private func keepFrames(of path: String) -> Int {
        let asset = AVURLAsset(url: URL(fileURLWithPath: path))
        guard let track = asset.tracks(withMediaType: .video).first, let reader = try? AVAssetReader(asset: asset) else { return 0 }
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
        ])
        guard reader.canAdd(output) else { return 0 }
        reader.add(output)
        guard reader.startReading() else { return 0 }
        var frames = 0
        while let sample = output.copyNextSampleBuffer() {
            guard let buffer = CMSampleBufferGetImageBuffer(sample) else { continue }
            CVPixelBufferLockBaseAddress(buffer, .readOnly)
            defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
            guard let base = CVPixelBufferGetBaseAddress(buffer) else { continue }
            let width = CVPixelBufferGetWidth(buffer)
            let height = CVPixelBufferGetHeight(buffer)
            let row = CVPixelBufferGetBytesPerRow(buffer)
            let source = base.assumingMemoryBound(to: UInt8.self)
            var rgba = Data(count: width * height * 4)
            rgba.withUnsafeMutableBytes { (out: UnsafeMutableRawBufferPointer) in
                for y in 0 ..< height {
                    for x in 0 ..< width {
                        let from = y * row + x * 4
                        let to = (y * width + x) * 4
                        out[to] = source[from + 2]
                        out[to + 1] = source[from + 1]
                        out[to + 2] = source[from]
                        out[to + 3] = 255
                    }
                }
            }
            _ = core.keepFrame(rgba: rgba, width: UInt32(width), height: UInt32(height))
            frames += 1
        }
        return frames
    }
}

/// The open artwork's time-lapse: recording as it changes, and what the Video tab shows.
@Observable
final class TimeLapseTool {
    @ObservationIgnored private unowned let model: CanvasModel
    @ObservationIgnored let lapse: ArtworkTimeLapse

    private(set) var recording: Bool
    /// Something has been recorded (Replay and Export need it).
    private(set) var recorded: Bool
    /// The replay shows over everything.
    var replaying = false
    /// Recording was turned off with something recorded: whether to delete it is asked.
    private(set) var purgeAsking = false
    /// How long a video to make is asked.
    var lengthAsking = false

    @ObservationIgnored private var taking = false
    @ObservationIgnored private var stale = false

    init(model: CanvasModel) {
        self.model = model
        let lapse = ArtworkTimeLapse(dir: model.dir)
        self.lapse = lapse
        recording = lapse.recording
        recorded = lapse.count > 0
    }

    func setRecording(_ on: Bool) {
        lapse.recording = on
        recording = on
        // the switch goes off at once; then what was recorded may be deleted
        if !on, lapse.count > 0 {
            purgeAsking = true
        }
    }

    /// The answer to `purgeAsking`: what was recorded deleted, or kept.
    func answerPurge(_ purge: Bool) {
        purgeAsking = false
        if purge {
            lapse.purge()
        }
        recorded = lapse.count > 0
    }

    /// Records the canvas as a frame, one at a time while changes come.
    func record() {
        guard recording else { return }
        if taking {
            stale = true
            return
        }
        taking = true
        take()
    }

    private func take() {
        stale = false
        let session = model.session
        let lapse = lapse
        DispatchQueue.global(qos: .utility).async { [weak self] in
            // frames as large as the video they make, private layers left out
            if let shot = session.timeLapseFrame(maxSide: timeLapseSide(s: lapse.settings)) {
                lapse.record(shot)
            }
            let count = lapse.count
            DispatchQueue.main.async {
                guard let self else { return }
                if self.stale {
                    self.take()
                } else {
                    self.taking = false
                    self.recorded = count > 0
                }
            }
        }
    }
}

/// How far an export has come, told from the thread doing it.
final class ExportMeter {
    private let lock = NSLock()
    private var kept: Float = 0

    var share: Float {
        get {
            lock.lock()
            defer { lock.unlock() }
            return kept
        }
        set {
            lock.lock()
            kept = newValue
            lock.unlock()
        }
    }
}

/// The time-lapse replayed over everything at its frame rate; dragging across the screen scrubs
/// through it. A bar on top: Done, the title and the time into the replay.
struct TimeLapseReplay: View {
    let model: CanvasModel

    @State private var count = 0
    @State private var index = 0
    @State private var scrubbing = false
    @State private var shown: UIImage?
    @State private var unreadable = false
    /// Where a scrub began, and the frame it began on.
    @State private var scrubFrom: (x: CGFloat, frame: Int)?

    private var time: String {
        let fps = Int(framesASecond)
        let seconds = index / fps
        return String(format: "%02d:%02d:%02d:%02d", seconds / 3600, seconds / 60 % 60, seconds % 60, index % fps)
    }

    private var said: String? {
        if count == 0 {
            return "Nothing recorded yet"
        }
        return unreadable && shown == nil ? "The time-lapse can't be read on this device" : nil
    }

    /// The surround's grid as the canvas draws it (1 1/3 pt lines every 16, none under the top bar).
    private var grid: some View {
        Canvas { context, size in
            let step: CGFloat = 16
            let line: CGFloat = 4 / 3
            let top: CGFloat = 44
            var x: CGFloat = 0.5
            while x < size.width {
                context.fill(Path(CGRect(x: x - line / 2, y: top, width: line, height: size.height - top)), with: .color(replayGrid))
                x += step
            }
            var y = top + 0.5
            while y < size.height {
                context.fill(Path(CGRect(x: 0, y: y - line / 2, width: size.width, height: line)), with: .color(replayGrid))
                y += step
            }
        }
    }

    var body: some View {
        GeometryReader { place in
            let share = count > 1 ? CGFloat(index) / CGFloat(count - 1) : 1
            ZStack(alignment: .top) {
                replayFill
                grid
                if let shown {
                    // edge to edge, under the header
                    Image(uiImage: shown)
                        .resizable()
                        .scaledToFit()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
                if let said {
                    Text(verbatim: said)
                        .font(.system(size: 17))
                        .foregroundStyle(Color(argb: 0xFF8E_8E93))
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
                ToolHeader(title: "Time-lapse Replay", buttons: [ToolHeaderButton(label: time) {}]) {
                    withAnimation(Motion.replayFade) { model.timeLapse.replaying = false }
                }
                // how far it has played
                Color(argb: 0xFF00_76FD)
                    .frame(width: place.size.width * min(max(share, 0), 1), height: 4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.top, 85)
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 4)
                    .onChanged { drag in
                        let from = scrubFrom ?? (drag.startLocation.x, index)
                        scrubFrom = from
                        scrubbing = true
                        let moved = (drag.location.x - from.x) / max(place.size.width, 1)
                        index = min(max(from.frame + Int(moved * CGFloat(count)), 0), max(count - 1, 0))
                    }
                    .onEnded { _ in
                        scrubFrom = nil
                        scrubbing = false
                    }
            )
        }
        .task { count = model.timeLapse.lapse.count }
        // play, frame by frame, until the end or a scrub
        .task(id: scrubbing) {
            while !scrubbing, !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(1000 / Int(framesASecond)))
                if index < count - 1 {
                    index += 1
                } else if count > 0 {
                    break
                }
            }
        }
        // one read at a time, of the newest frame: where a read takes longer than a frame some are
        // skipped, and those read still show
        .task {
            let lapse = model.timeLapse.lapse
            var last = -1
            while !Task.isCancelled {
                let i = index
                guard i != last else {
                    try? await Task.sleep(for: .milliseconds(8))
                    continue
                }
                last = i
                let read = await Task.detached(priority: .userInitiated) { lapse.frame(i) }.value
                if let read {
                    shown = read
                } else {
                    unreadable = true
                }
            }
        }
    }
}
