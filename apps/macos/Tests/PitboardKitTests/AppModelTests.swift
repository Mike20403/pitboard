import Foundation
import Observation
import PitboardKit
import Testing

// The app's model shows what the Rust model says. What the Rust model says, and when, is
// tested in `pitboard-ffi`; these test the glue: which snapshot is shown, what is drawn
// again, on which thread a snapshot arrives, and that the bindings carry one end to end.

/// Stands in for the Rust model: hands out the snapshot a test gives it, and keeps every
/// intent sent, so nothing a test does reaches a core.
private final class StandInModel: PitboardModelProtocol, @unchecked Sendable {
    // Unchecked because every read and write holds `lock`.
    private let lock = NSLock()
    private var first: Snapshot
    private var intents: [Intent] = []
    private var stops = 0

    init(_ first: Snapshot) {
        self.first = first
    }

    var sent: [Intent] { lock.withLock { intents } }
    var stopped: Int { lock.withLock { stops } }

    func send(intent: Intent) { lock.withLock { intents.append(intent) } }
    func shutdown() { lock.withLock { stops += 1 } }
    func snapshot() -> Snapshot { lock.withLock { first } }
}

/// A snapshot with nothing in it but what a test says.
private func snapshot(
    _ revision: UInt64, status: Status? = nil, reading: Bool = false,
    readFailure: ReadFailure? = nil, window: WindowRequest = WindowRequest(serial: 0, pane: nil)
) -> Snapshot {
    Snapshot(
        revision: revision, now: Int64(revision), reading: reading, updatedAt: nil,
        status: status, warnings: [], readFailure: readFailure, stuck: false, installed: nil,
        switchUnderWay: nil, quitQuestion: nil, lastSwitches: [], abandoned: nil, failure: nil,
        windowRequest: window, signingIn: nil, sheet: nil, sheetFailure: nil,
        menuBar: MenuBarText(nameAndUsage: "", usage: "", spoken: "Pitboard"), sections: [],
        showsTools: false, notices: [],
        menuNotices: MenuNotices(install: nil, switches: [], others: nil), footing: .ready,
        setup: nil, accountsShown: .reading(title: "Reading accounts…"),
        menuAccountsNote: nil, updatedMenu: "", updatedWindow: "", sheetText: nil,
        signingInText: nil, quitConfirmation: nil, failureAlert: nil,
        machine: MachineShown(
            schedule: ScheduleShown(
                schedule: nil, on: false, changing: false, enabled: true, runs: nil,
                scheduledIn: nil, note: nil, failed: nil),
            renewal: RenewalShown(renewing: false, note: ""),
            checks: ChecksShown(
                lines: [], summary: nil, checking: false, checked: nil, waiting: nil),
            activity: ActivityShown(lines: [], empty: nil),
            commandLine: CommandLineShown(
                found: nil, inTerminal: nil, updateNote: nil, offersLink: false,
                cannotLink: nil)))
}

/// Claude Code accounts read at `now`, the first signed in, each measured at `measured`
/// where it was.
private func status(_ labels: String..., now: Int64 = 0, measured: Int64? = nil) -> Status {
    Status(
        now: now,
        accounts: labels.map { label in
            Account(
                id: "claude:\(label)", provider: "claude", label: label,
                qualified: "claude/\(label)", unplaced: false, email: "\(label)@example.com",
                accountUuid: label, signedIn: label == labels.first, switchable: true,
                parked: nil,
                usage: measured.map { Usage(source: .live, observedAt: $0, windows: []) },
                stale: nil, staleExplanation: nil, lastsSeconds: nil, lastsBurning: false)
        },
        warnings: [])
}

/// Whether reading `part` of `model` is told of a change while `change` runs.
@MainActor
private func changes<Value>(
    _ model: AppModel, _ part: KeyPath<AppModel, Value>,
    when change: () -> Void
) -> Bool {
    let told = Told()
    withObservationTracking {
        _ = model[keyPath: part]
    } onChange: {
        told.mark()
    }
    change()
    return told.happened
}

private final class Told: @unchecked Sendable {
    // Unchecked because every read and write holds `lock`.
    private let lock = NSLock()
    private var marked = false
    var happened: Bool { lock.withLock { marked } }
    func mark() { lock.withLock { marked = true } }
}

/// The app's model shows the newest snapshot it has been given and drops any that is not
/// newer: the model can tell 3 and then 5, and `snapshot()` can give one never told, so one
/// that arrives late must never put back what a newer one replaced.
@MainActor
@Test func aSnapshotThatIsNotNewerIsDropped() {
    let model = AppModel(model: StandInModel(snapshot(0)))
    #expect(model.revision == 0)
    #expect(model.status == nil)

    model.apply(snapshot(3, status: status("work")))
    #expect(model.revision == 3)
    #expect(model.status == status("work"))

    model.apply(snapshot(2, status: status("personal")))
    model.apply(snapshot(3, status: status("personal")))
    #expect(model.revision == 3)
    #expect(model.status == status("work"), "neither older nor the same revision is shown")

    model.apply(snapshot(5, status: status("personal")))
    #expect(model.revision == 5)
    #expect(model.status == status("personal"))
}

/// Each part is assigned only where it changed, so a view reading a part that stayed the
/// same is not drawn again when another part moves. What `apply` says it assigned is what a
/// test can see of the rule itself: Observation tells an observer nothing of an equal value
/// assigned again, so what is drawn again is the same with the comparison or without it.
@MainActor
@Test func onlyThePartsThatChangedAreAssigned() {
    let model = AppModel(model: StandInModel(snapshot(1, status: status("work"))))
    let reading: [PartialKeyPath<AppModel>] = [\.revision, \.now, \.reading]
    #expect(model.apply(snapshot(2, status: status("work"), reading: true)) == reading)
    let read = changes(model, \.reading) {
        #expect(model.apply(snapshot(3, status: status("work"))) == reading)
    }
    #expect(read, "a read is no longer under way")
    let more = changes(model, \.status) {
        #expect(
            model.apply(snapshot(4, status: status("work", "spare")))
                == [\.revision, \.now, \.status])
    }
    #expect(more)
    let rows = changes(model, \.sections) {
        model.apply(snapshot(5, status: status("work")))
    }
    #expect(!rows, "nothing about the rows moved")
    #expect(model.apply(snapshot(5, status: status("personal"))).isEmpty, "dropped")
}

/// The account windows are told each read that says who is enrolled: one that answered, and
/// what the poll reads once the account index changes, while reads fail too, as the Swift
/// model told it. Not what stands in for a read that failed before anything was shown, the
/// last numbers measured, which the Swift model never told, nor the numbers a session
/// recorded, taken onto what is shown, which keep its time and change only each account's
/// usage, as the Rust model takes them. A read that failed after one that answered leaves
/// the accounts as they were, and says nothing either.
@MainActor
@Test func aReadThatSaysWhoIsEnrolledIsToldOnce() {
    let failed = ReadFailure(code: "unreachable", message: "Anthropic could not be reached")
    let model = AppModel(model: StandInModel(snapshot(0)))
    var told: [Status] = []
    model.afterRead = { told.append($0) }

    model.apply(snapshot(1, status: status("work", "spare"), readFailure: failed))
    model.apply(
        snapshot(2, status: status("work", "spare", measured: 30), readFailure: failed))
    #expect(told.isEmpty, "the last numbers measured, in place of a read that failed")

    // `pitboard forget spare` in a terminal, while Anthropic is still out of reach.
    model.apply(snapshot(3, status: status("work", now: 60), readFailure: failed))
    #expect(told == [status("work", now: 60)], "the poll's read of the changed index")

    model.apply(snapshot(4, status: status("work", now: 120)))
    model.apply(snapshot(5, status: status("work", now: 120), reading: true))
    model.apply(snapshot(6, status: status("work", now: 120, measured: 90)))
    #expect(
        told == [status("work", now: 60), status("work", now: 120)],
        "told once, and not again for the same accounts or for their numbers")

    model.apply(
        snapshot(7, status: status("work", now: 120, measured: 90), readFailure: failed))
    model.apply(snapshot(8, status: status("work", "travel", now: 180), readFailure: failed))
    #expect(
        told == [
            status("work", now: 60), status("work", now: 120),
            status("work", "travel", now: 180),
        ])
}

/// What a view asks for reaches the model as it was asked, at once, and the app quitting
/// stops it.
@MainActor
@Test func whatIsAskedReachesTheModelAsItWasAsked() {
    let standIn = StandInModel(snapshot(0))
    let model = AppModel(model: standIn)
    model.send(.switchTo(qualified: "codex/spare"))
    model.send(.closeSheet)
    #expect(standIn.sent == [.switchTo(qualified: "codex/spare"), .closeSheet])
    model.shutdown()
    #expect(standIn.stopped == 1)
}

/// The model tells its listener on a thread of its own, one snapshot after another; the
/// listener hands each to the main actor, and they arrive there in the order they were told.
@MainActor
@Test func theListenerDeliversOnTheMainActorInOrder() async throws {
    let listener = ModelListening()
    var arrived: [UInt64] = []
    var onMain = true
    listener.deliver = { snapshot in
        onMain = onMain && Thread.isMainThread
        arrived.append(snapshot.revision)
    }
    let told = Array(UInt64(1)...200)
    let notifier = Thread {
        for revision in told { try? listener.changed(snapshot: snapshot(revision)) }
    }
    notifier.start()
    let deadline = Date().addingTimeInterval(10)
    while arrived.count < told.count, Date() < deadline {
        try await Task.sleep(for: .milliseconds(10))
    }
    #expect(arrived == told)
    #expect(onMain)
}

/// What is said of time in a fixture's snapshot, in UTC, as `pitboard-ffi`'s own tests say
/// it.
private final class Utc: LocalTime {
    func clock(epoch: Int64, withWeekday: Bool) throws -> String { "\(epoch)" }
    func sameDay(first: Int64, second: Int64) throws -> Bool { true }
    func dateAndTime(epoch: Int64) throws -> String { "\(epoch)" }
}

/// The fixture's folder, as Rust's `std::env::temp_dir` finds it.
private var fixtureFolder: URL {
    (ProcessInfo.processInfo.environment["TMPDIR"].map { URL(fileURLWithPath: $0) }
        ?? FileManager.default.temporaryDirectory)
        .appendingPathComponent("pitboard-fixture")
}

/// The bindings carry the model end to end: a fixture's model, the real core over a machine
/// of its own that reaches nothing of this one, started from Swift, reads its accounts and
/// tells a listener written in Swift, which hands them to the app's model on the main actor.
/// Only a library built with `build-xcframework.sh --fixture` has fixtures, as CI builds it
/// before these tests.
@MainActor
@Test(.enabled(if: !fixtureNames().isEmpty, "the library was built without fixtures"))
func aFixturesModelPublishesItsAccounts() async throws {
    defer { try? FileManager.default.removeItem(at: fixtureFolder) }
    let model = try AppModel.listening { listener in
        try PitboardModel.fixture(name: "twoTools", listener: listener, localTime: Utc())
    }
    #expect(model.status == nil, "nothing is read before it is started")
    model.send(.start)
    let deadline = Date().addingTimeInterval(30)
    while model.status == nil || model.reading, Date() < deadline {
        try await Task.sleep(for: .milliseconds(20))
    }
    model.shutdown()
    let labels = model.status?.accounts.compactMap(\.qualified) ?? []
    #expect(
        Set(labels) == [
            "claude/work", "claude/personal", "claude/old", "codex/main", "codex/spare",
        ])
    #expect(model.sections.map(\.heading) == ["Claude Code", "Codex"])
    #expect(model.revision > 0)
}
