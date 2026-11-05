import QuartzCore
import SwiftUI
import UIKit

/// How often the pad is asked whether it has drawn yet, and for how long at most, seconds.
private let padPoll = 0.032
private let padPollLimit = 4.0

/// The brush studio's drawing pad: a small canvas of its own, as big as the view, painted with the
/// brush being edited. Pencil and finger both paint; there are no gestures.
final class PadView: UIView {
    override class var layerClass: AnyClass { CAMetalLayer.self }

    private let studio: StudioModel
    private var attached = false
    /// The touch painting, while one is.
    private var stroke: ObjectIdentifier?
    /// The Pencil's samples, each held until its true force is known.
    private lazy var pencil = PencilHold(
        begin: { [weak self] samples in
            guard let pad = self?.studio.pad else { return }
            // the first stroke of one's own takes the sample scribble's place
            pad.beginOwnStroke()
            pad.strokeBegin(pointer: .stylus, samples: samples)
        },
        move: { [weak self] samples in
            self?.studio.pad?.strokeAdd(samples: samples)
        },
        ahead: { [weak self] samples in
            self?.studio.pad?.strokePredict(samples: samples)
        },
        end: { [weak self] samples in
            self?.studio.pad?.strokeEnd(samples: samples)
        }
    )

    init(studio: StudioModel) {
        self.studio = studio
        super.init(frame: .zero)
        isOpaque = true
        backgroundColor = .black
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("the pad is made in code")
    }

    /// Pixels to a point on the screen the view is on.
    private var scale: CGFloat {
        window?.screen.scale ?? traitCollection.displayScale
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window == nil {
            detach()
        } else {
            setNeedsLayout()
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        // closing, the pad's canvas has gone: none is made again as the studio slides away
        guard window != nil, bounds.width > 0, bounds.height > 0, studio.isOpen else { return }
        contentScaleFactor = scale
        layer.contentsScale = scale
        let width = UInt32(max(1, (bounds.width * scale).rounded()))
        let height = UInt32(max(1, (bounds.height * scale).rounded()))
        let pad: CanvasSession
        if let made = studio.pad {
            pad = made
        } else {
            pad = CanvasSession(width: width, height: height)
            studio.startPad(pad)
        }
        if attached {
            pad.resizeWindow(width: width, height: height)
        } else {
            attach(pad, width: width, height: height)
        }
    }

    /// Hands the layer to the pad's canvas, whose render thread draws into it from now on.
    private func attach(_ pad: CanvasSession, width: UInt32, height: UInt32) {
        guard let metal = layer as? CAMetalLayer else { return }
        // the core takes a retain of its own, let go with the window
        let pointer = UInt64(UInt(bitPattern: Unmanaged.passRetained(metal).toOpaque()))
        let id = acquireLayer(layer: pointer)
        guard id != 0 else {
            NSLog("ProCraft: the pad's layer wasn't taken")
            return
        }
        do {
            try pad.attachWindow(window: id, width: width, height: height)
            pad.setDisplayScale(pixelsPerPoint: Float(scale))
            attached = true
            // uncovered once it has drawn a frame of its own
            uncoverWhenDrawn(pad, waited: 0)
        } catch {
            releaseLayer(id: id)
            NSLog("ProCraft: attaching the pad: %@", "\(error)")
        }
    }

    /// Once the pad has presented a frame (or after a while regardless), draws its sample scribble,
    /// and uncovers it a poll later, the scribble drawn.
    private func uncoverWhenDrawn(_ pad: CanvasSession, waited: Double) {
        guard studio.pad === pad else { return }
        if waited >= padPollLimit || pad.info().frames > 0 {
            studio.padReady(pad)
            DispatchQueue.main.asyncAfter(deadline: .now() + padPoll) { [weak self] in
                guard let self, self.studio.pad === pad else { return }
                self.studio.padShown = true
            }
            return
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + padPoll) { [weak self] in
            self?.uncoverWhenDrawn(pad, waited: waited + padPoll)
        }
    }

    private func detach() {
        guard attached else { return }
        studio.pad?.detachWindow()
        attached = false
    }

    // MARK: touches

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard stroke == nil, let pad = studio.pad, let touch = touches.first else { return }
        studio.padMenuOpen = false
        stroke = ObjectIdentifier(touch)
        if touch.type == .pencil {
            pencil.down(touch)
            pencil.add(coalesced(touch, in: event), sample: sample)
            pencil.release()
        } else {
            // the first stroke of one's own takes the sample scribble's place
            pad.beginOwnStroke()
            pad.strokeBegin(pointer: .finger, samples: samples(of: touch, in: event))
        }
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let pad = studio.pad, let touch = painting(touches) else { return }
        if pencil.holds(touch) {
            pencil.add(coalesced(touch, in: event), sample: sample)
            pencil.release(predicted: (event?.predictedTouches(for: touch) ?? []).map(sample))
        } else {
            pad.strokeAdd(samples: samples(of: touch, in: event))
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch = painting(touches) else { return }
        stroke = nil
        if pencil.holds(touch) {
            pencil.add(coalesced(touch, in: event), sample: sample)
            pencil.lift()
        } else {
            studio.pad?.strokeEnd(samples: samples(of: touch, in: event))
        }
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch = painting(touches) else { return }
        stroke = nil
        if pencil.holds(touch) {
            if pencil.cancel() {
                studio.pad?.strokeCancel()
            }
        } else {
            studio.pad?.strokeCancel()
        }
    }

    /// True values for samples given as estimates: the stroke waiting on them goes on.
    override func touchesEstimatedPropertiesUpdated(_ touches: Set<UITouch>) {
        pencil.update(touches, sample: sample)
        pencil.release()
    }

    /// The touch painting, if it is among `touches`.
    private func painting(_ touches: Set<UITouch>) -> UITouch? {
        touches.first { ObjectIdentifier($0) == stroke }
    }

    private func samples(of touch: UITouch, in event: UIEvent?) -> [PenSample] {
        coalesced(touch, in: event).map(sample)
    }

    private func coalesced(_ touch: UITouch, in event: UIEvent?) -> [UITouch] {
        let all = event?.coalescedTouches(for: touch) ?? []
        return all.isEmpty ? [touch] : all
    }

    private func sample(_ touch: UITouch) -> PenSample {
        penSample(touch, in: self, scale: scale)
    }
}

/// The pad in the studio's layout.
struct PadHost: UIViewRepresentable {
    let studio: StudioModel

    func makeUIView(context: Context) -> PadView {
        PadView(studio: studio)
    }

    func updateUIView(_ view: PadView, context: Context) {}
}
