import Foundation

/// A Pencil's samples on their way into a stroke. The system gives a sample's force before it is
/// known, as an estimate, and the true value a moment later. So a sample still expecting one waits
/// here, and only those with nothing left to wait for leave, always in the order they came.
struct HeldSamples<Sample> {
    private struct Held {
        var sample: Sample
        /// What the system calls this sample when its true values come; none once it has them all.
        var waitingOn: Int?
        /// When it came, seconds.
        let since: TimeInterval
    }

    private var held: [Held] = []

    var isEmpty: Bool {
        held.isEmpty
    }

    /// When the longest held sample still waiting came, if one is.
    var oldestWaiting: TimeInterval? {
        held.first { $0.waitingOn != nil }?.since
    }

    /// The samples held, in order: what the stroke is drawn ahead through, not yet part of it.
    var waiting: [Sample] {
        held.map { $0.sample }
    }

    /// A sample, waiting on update `index` (none if it has nothing to wait for).
    mutating func add(_ sample: Sample, waitingOn index: Int?, at time: TimeInterval) {
        held.append(Held(sample: sample, waitingOn: index, since: time))
    }

    /// Update `index` came, `change` taking the sample to its true values; `final` says it was the
    /// last for this sample. An update for a sample already gone is too late to matter.
    mutating func update(_ index: Int, final: Bool, change: (Sample) -> Sample) {
        guard let at = held.firstIndex(where: { $0.waitingOn == index }) else { return }
        held[at].sample = change(held[at].sample)
        if final {
            held[at].waitingOn = nil
        }
    }

    /// Those that came by `time` wait no longer: their true values aren't coming, or too late.
    mutating func stopWaiting(before time: TimeInterval) {
        for i in held.indices where held[i].since <= time {
            held[i].waitingOn = nil
        }
    }

    /// Everything waits no longer.
    mutating func stopWaiting() {
        for i in held.indices {
            held[i].waitingOn = nil
        }
    }

    /// Takes the samples from the first on with nothing left to wait for. One still waiting holds
    /// back those after it, whatever they know: the order stays.
    mutating func takeFirm() -> [Sample] {
        let count = held.prefix { $0.waitingOn == nil }.count
        let firm = held.prefix(count).map { $0.sample }
        held.removeFirst(count)
        return firm
    }

    /// Lets everything go.
    mutating func clear() {
        held = []
    }
}
