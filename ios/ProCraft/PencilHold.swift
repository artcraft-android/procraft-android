import QuartzCore
import UIKit

/// How long a sample waits for its true values, seconds: then the stroke goes on with its estimate.
private let longestWait = 0.1

/// One Pencil touch's way into a stroke, for a view that paints with it. Samples still expecting
/// their true force wait in `HeldSamples` and are drawn ahead of the stroke meanwhile, and join it
/// as they become known; the touch ends once the last of them has. A pen's estimate is shown, but
/// never kept.
final class PencilHold {
    /// The stroke starts with these samples, goes on with them, is drawn ahead through them, and
    /// ends with them.
    private let begin: ([PenSample]) -> Void
    private let move: ([PenSample]) -> Void
    private let ahead: ([PenSample]) -> Void
    private let end: ([PenSample]) -> Void

    private var held = HeldSamples<PenSample>()
    /// The touch, until it lifts.
    private var touch: ObjectIdentifier?
    /// The stroke has begun, and the Pencil has lifted with samples still held.
    private var started = false
    private var lifting = false
    private var timer: Timer?

    init(
        begin: @escaping ([PenSample]) -> Void,
        move: @escaping ([PenSample]) -> Void,
        ahead: @escaping ([PenSample]) -> Void,
        end: @escaping ([PenSample]) -> Void
    ) {
        self.begin = begin
        self.move = move
        self.ahead = ahead
        self.end = end
    }

    /// Whether this is the touch being held.
    func holds(_ touch: UITouch) -> Bool {
        self.touch == ObjectIdentifier(touch)
    }

    /// A Pencil touch came down: a stroke still waiting to end does now.
    func down(_ touch: UITouch) {
        finish()
        self.touch = ObjectIdentifier(touch)
    }

    /// These of the touch's samples came (the system's coalesced touches), `sample` reading each.
    func add(_ touches: [UITouch], sample: (UITouch) -> PenSample) {
        let now = CACurrentMediaTime()
        for t in touches {
            let waiting = t.estimatedPropertiesExpectingUpdates.isEmpty ? nil : t.estimationUpdateIndex?.intValue
            held.add(sample(t), waitingOn: waiting, at: now)
        }
    }

    /// True values came for these samples, read by `sample`; each keeps the time it came at.
    func update(_ touches: Set<UITouch>, sample: (UITouch) -> PenSample) {
        for t in touches {
            guard let index = t.estimationUpdateIndex?.intValue else { continue }
            held.update(index, final: t.estimatedPropertiesExpectingUpdates.isEmpty) { before in
                var known = sample(t)
                known.timeMs = before.timeMs
                return known
            }
        }
    }

    /// Sends on what is firm, and draws what is held (and where the system predicts the Pencil
    /// goes) ahead of it.
    func release(predicted: [PenSample] = []) {
        let firm = held.takeFirm()
        if lifting, held.isEmpty {
            close(firm)
            return
        }
        if !firm.isEmpty {
            if started {
                move(firm)
            } else {
                started = true
                begin(firm)
            }
        }
        if started, !held.isEmpty || !predicted.isEmpty {
            ahead(held.waiting + predicted)
        }
        keepTime()
    }

    /// The touch lifted: the stroke ends once its last samples are known.
    func lift() {
        touch = nil
        lifting = true
        release()
    }

    /// Ends the stroke now with what is known, if one is waiting to.
    func finish() {
        guard lifting else { return }
        held.stopWaiting()
        release()
    }

    /// The system took the touch back. Returns whether the stroke had begun, so has to be undone.
    func cancel() -> Bool {
        let had = started
        reset()
        return had
    }

    private func close(_ firm: [PenSample]) {
        if started {
            end(firm)
        } else if !firm.isEmpty {
            // too quick to have begun
            begin(firm)
            end([])
        }
        reset()
    }

    private func reset() {
        held.clear()
        touch = nil
        started = false
        lifting = false
        timer?.invalidate()
        timer = nil
    }

    /// A sample whose true values never come mustn't hold the stroke up.
    private func keepTime() {
        guard timer == nil, let oldest = held.oldestWaiting else { return }
        let wait = max(oldest + longestWait - CACurrentMediaTime(), 0.001)
        timer = Timer.scheduledTimer(withTimeInterval: wait, repeats: false) { [weak self] _ in
            guard let self else { return }
            self.timer = nil
            self.held.stopWaiting(before: CACurrentMediaTime() - longestWait)
            self.release()
        }
    }
}

/// The feel Apple Pencil Pro gives under the hand. Only that Pencil, on the iPads that support it,
/// feels anything: for the rest of them the system does nothing.
final class PencilFeedback {
    /// The system's generator, from iPadOS 17.5.
    private var generator: AnyObject?

    init(view: UIView) {
        if #available(iOS 17.5, *) {
            generator = UICanvasFeedbackGenerator(view: view)
        }
    }

    /// Something is about to give feedback: the system gets ready.
    func prepare() {
        if #available(iOS 17.5, *) {
            (generator as? UICanvasFeedbackGenerator)?.prepare()
        }
    }

    /// Something drawn or moved lined up with something, at `point` in the view.
    func aligned(at point: CGPoint) {
        if #available(iOS 17.5, *) {
            (generator as? UICanvasFeedbackGenerator)?.alignmentOccurred(at: point)
        }
    }

    /// A path drawn became what it was going to be, at `point` in the view.
    func completed(at point: CGPoint) {
        if #available(iOS 17.5, *) {
            (generator as? UICanvasFeedbackGenerator)?.pathCompleted(at: point)
        }
    }
}
