import Foundation
import PitboardKit
import Testing
import WebKit

@testable import PitboardApp

// Which stores go, and when, are the model's decisions, tested in pitboard-ffi's
// `model/windowing.rs` and `account_windows/records.rs`. These are WebKit's side: the tries
// and pauses of a deletion, and what the model is told of it.

/// What a store refuses while something still holds it.
private struct InUse: Error {}

/// Stores as WebKit keeps them, without WebKit: each refuses its deletion as many times as a
/// test says, as one a window or WebKit's network process still holds does.
@MainActor
final class StandInStores: WebsiteDataStores {
    var refusals: [UUID: Int] = [:]
    private(set) var made: [UUID] = []
    private(set) var removed: [UUID] = []
    private(set) var attempts: [UUID: Int] = [:]
    private var stores: [UUID: WKWebsiteDataStore] = [:]

    func store(for id: UUID) -> WKWebsiteDataStore {
        made.append(id)
        if let store = stores[id] { return store }
        let store = WKWebsiteDataStore.nonPersistent()
        stores[id] = store
        return store
    }

    func remove(_ id: UUID) async throws {
        attempts[id, default: 0] += 1
        if let left = refusals[id], left > 0 {
            refusals[id] = left - 1
            throw InUse()
        }
        stores[id] = nil
        removed.append(id)
    }
}

/// The pauses a janitor was told to wait, waited for no time at all.
@MainActor
final class Pauses {
    private(set) var asked: [Duration] = []
    func pause(_ duration: Duration) async { asked.append(duration) }
}

/// What a janitor told the model, in order: each store, and whether it went.
@MainActor
private final class Said {
    var told: [(UUID, Bool)] = []
}

@MainActor
private func janitor(_ stores: StandInStores, pauses: Pauses = Pauses()) -> StoreJanitor {
    StoreJanitor(stores: stores, pause: { await pauses.pause($0) })
}

private let one = UUID(uuidString: "00000000-0000-4000-8000-000000000001")!

/// The model's ask for `one`, the `ask`th.
private func asked(_ ask: UInt64) -> StoreDeletion {
    StoreDeletion(store: one.uuidString.lowercased(), ask: ask)
}

/// Asks `janitor` to delete what `asks` name, and waits until it has told of each.
@MainActor
private func delete(_ janitor: StoreJanitor, _ asks: [StoreDeletion], said: Said) async {
    let before = said.told.count
    janitor.delete(asks) { store, went in said.told.append((store, went)) }
    for _ in 0..<500 where said.told.count < before + asks.count { await Task.yield() }
}

/// Measured on macOS 27: a store whose last web view is released stays in use for 20 to 60
/// ms. The janitor waits and tries again, and says it went.
@MainActor
@Test func aStoreInUseIsTriedAgainAfterAPause() async {
    let stores = StandInStores()
    let pauses = Pauses()
    let said = Said()
    let janitor = janitor(stores, pauses: pauses)
    stores.refusals[one] = 2

    await delete(janitor, [asked(1)], said: said)
    #expect(stores.removed == [one])
    #expect(stores.attempts[one] == 3)
    #expect(pauses.asked == Array(StoreJanitor.retries.prefix(2)))
    #expect(said.told.map(\.0) == [one])
    #expect(said.told.map(\.1) == [true])
}

/// A store something keeps holding is said to be held after every try, for the model to ask
/// for again after its next read, which deletes it once nothing holds it.
///
/// StoreJanitorTests.swift's aStoreStillHeldIsLeftForTheNextSweep, as it was at a3e5ce0,
/// whose record is the model's now.
@MainActor
@Test func aStoreStillHeldIsSaidToBeHeld() async {
    let stores = StandInStores()
    let pauses = Pauses()
    let said = Said()
    let janitor = janitor(stores, pauses: pauses)
    stores.refusals[one] = 100

    await delete(janitor, [asked(1)], said: said)
    #expect(stores.removed.isEmpty)
    #expect(stores.attempts[one] == StoreJanitor.retries.count + 1)
    #expect(pauses.asked == StoreJanitor.retries)
    #expect(said.told.map(\.1) == [false])

    stores.refusals[one] = 0
    await delete(janitor, [asked(2)], said: said)
    #expect(stores.removed == [one])
    #expect(said.told.map(\.1) == [false, true])
}

/// The retries are short, as measured, and end within a few seconds.
@Test func theRetriesEndWithinFourSeconds() {
    let total = StoreJanitor.retries.reduce(Duration.zero, +)
    #expect(total < .seconds(4))
    #expect(StoreJanitor.retries.first == .milliseconds(50))
}

/// An ask that several snapshots list before the model has heard how it went is deleted
/// once, while a new ask for the same store, after it was held, is deleted again.
///
/// StoreJanitorTests.swift's twoSweepsAtOnceDeleteAStoreOnce, as it was at a3e5ce0, whose
/// claimed set is the model's now.
@MainActor
@Test func anAskIsDeletedOnce() async {
    let stores = StandInStores()
    let said = Said()
    let janitor = janitor(stores)
    stores.refusals[one] = 1

    janitor.delete([asked(1)]) { store, went in said.told.append((store, went)) }
    await delete(janitor, [asked(1)], said: said)
    #expect(stores.removed == [one])
    #expect(stores.attempts[one] == 2, "one deletion, tried twice")
    #expect(said.told.count == 1)

    await delete(janitor, [asked(2)], said: said)
    #expect(stores.attempts[one] == 3, "a new ask")
}

/// The records are kept under the Pitboard directory the core reads, standardised as they
/// were when the app worked the directory out itself, so an update keeps every store and page
/// it recorded. The core gives the path as the environment does, `.`, `..` and trailing `/`
/// included, and the key is that path without them.
@Test func theRecordsAreKeptUnderTheCoresDirectoryStandardised() {
    let dotted = ["PITBOARD_HOME": "/Users/dana/./work/../pitboard/"]
    #expect(pitboardDirectory(environment: dotted) == "/Users/dana/./work/../pitboard/")
    #expect(WebEnvironment.recordKey(environment: dotted) == "/Users/dana/pitboard")
    #expect(
        WebEnvironment.recordKey(environment: ["HOME": "/Users/dana"])
            == "/Users/dana/.pitboard")
    #expect(
        WebEnvironment.recordKey(environment: ["HOME": "/Users/dana/"])
            == "/Users/dana/.pitboard")
    let home = FileManager.default.homeDirectoryForCurrentUser
    #expect(
        WebEnvironment.recordKey(environment: [:])
            == home.appendingPathComponent(".pitboard").standardizedFileURL.path,
        "without HOME, the account's own, where Foundation found it")
}

/// The records are kept in the app's own folder in Application Support, named by its bundle
/// id, whatever `HOME` says, as WebKit keeps the stores they describe. Nothing is written.
@Test func theRecordsAreKeptInTheAppsOwnFolder() throws {
    let support = try #require(
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first)
    let directory = WebEnvironment.recordsDirectory(bundle: .main)
    #expect(directory.deletingLastPathComponent().path == support.path)
    #expect(
        directory.lastPathComponent
            == (Bundle.main.bundleIdentifier ?? "com.usepitboard.Pitboard"))
}
