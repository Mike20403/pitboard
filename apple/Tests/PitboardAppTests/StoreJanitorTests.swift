import Foundation
import Testing
import WebKit

@testable import PitboardApp

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
    /// Called as a store is made, so a test sees what was recorded by then.
    var onMake: ((UUID) -> Void)?
    private var stores: [UUID: WKWebsiteDataStore] = [:]

    func store(for id: UUID) -> WKWebsiteDataStore {
        made.append(id)
        onMake?(id)
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

@MainActor
private func janitor(
    _ stores: StandInStores,
    record: StoreRecord = StoreRecord(defaults: TestDefaults(), directory: "/a"),
    pauses: Pauses = Pauses()
) -> StoreJanitor {
    StoreJanitor(stores: stores, record: record, pause: { await pauses.pause($0) })
}

private let one = UUID(uuidString: "00000000-0000-4000-8000-000000000001")!
private let two = UUID(uuidString: "00000000-0000-4000-8000-000000000002")!
private let three = UUID(uuidString: "00000000-0000-4000-8000-000000000003")!

/// A store WebKit has made and nobody recorded would never be deleted, so the record comes
/// first.
@MainActor
@Test func aStoreIsRecordedBeforeItIsMade() {
    let stores = StandInStores()
    let record = StoreRecord(defaults: TestDefaults(), directory: "/a")
    var recordedFirst = false
    stores.onMake = { recordedFirst = record.ids.contains($0) }
    let janitor = janitor(stores, record: record)

    #expect(!janitor.hasMade(one))
    _ = janitor.store(for: one)
    #expect(recordedFirst)
    #expect(janitor.hasMade(one))
    #expect(stores.made == [one])
}

/// Only stores this Pitboard recorded, and no enrolled account derives, are deleted.
@MainActor
@Test func aSweepDeletesOnlyRecordedStoresNoAccountHas() async {
    let stores = StandInStores()
    let record = StoreRecord(defaults: TestDefaults(), directory: "/a")
    let janitor = janitor(stores, record: record)
    _ = janitor.store(for: one)
    _ = janitor.store(for: two)

    await janitor.sweep(keeping: [one, three])
    #expect(stores.removed == [two])
    #expect(record.ids == [one], "a deleted store is no longer recorded")
    #expect(stores.attempts[three] == nil, "a store nobody recorded is never deleted")
}

/// Measured on macOS 27: a store whose last web view is released stays in use for 20 to 60
/// ms. The janitor waits and tries again.
@MainActor
@Test func aStoreInUseIsTriedAgainAfterAPause() async {
    let stores = StandInStores()
    let pauses = Pauses()
    let janitor = janitor(stores, pauses: pauses)
    _ = janitor.store(for: one)
    stores.refusals[one] = 2

    await janitor.sweep(keeping: [])
    #expect(stores.removed == [one])
    #expect(stores.attempts[one] == 3)
    #expect(pauses.asked == Array(StoreJanitor.retries.prefix(2)))
    #expect(!janitor.hasMade(one))
}

/// A store something keeps holding is left recorded, and the next sweep tries it again.
@MainActor
@Test func aStoreStillHeldIsLeftForTheNextSweep() async {
    let stores = StandInStores()
    let pauses = Pauses()
    let janitor = janitor(stores, pauses: pauses)
    _ = janitor.store(for: one)
    stores.refusals[one] = 100

    await janitor.sweep(keeping: [])
    #expect(stores.removed.isEmpty)
    #expect(stores.attempts[one] == StoreJanitor.retries.count + 1)
    #expect(pauses.asked == StoreJanitor.retries)
    #expect(janitor.hasMade(one))

    stores.refusals[one] = 0
    await janitor.sweep(keeping: [])
    #expect(stores.removed == [one])
}

/// The retries are short, as measured, and end within a few seconds.
@Test func theRetriesEndWithinFourSeconds() {
    let total = StoreJanitor.retries.reduce(Duration.zero, +)
    #expect(total < .seconds(4))
    #expect(StoreJanitor.retries.first == .milliseconds(50))
}

/// Two sweeps at once delete one store once.
@MainActor
@Test func twoSweepsAtOnceDeleteAStoreOnce() async {
    let stores = StandInStores()
    let janitor = janitor(stores)
    _ = janitor.store(for: one)
    stores.refusals[one] = 1

    async let first: Void = janitor.sweep(keeping: [])
    async let second: Void = janitor.sweep(keeping: [])
    _ = await (first, second)
    #expect(stores.removed == [one])
    #expect(stores.attempts[one] == 2)
}

/// The record is kept per Pitboard directory: a copy of the app run with another home shares
/// WebKit's stores with the copy installed and none of its accounts.
@MainActor
@Test func eachPitboardDirectoryKeepsItsOwnRecord() {
    let defaults = TestDefaults()
    let mine = StoreRecord(defaults: defaults, directory: "/Users/dana/.pitboard")
    let other = StoreRecord(defaults: defaults, directory: "/tmp/test/.pitboard")
    mine.add(one)
    other.add(two)
    #expect(mine.ids == [one])
    #expect(other.ids == [two])
    mine.remove(one)
    #expect(mine.ids.isEmpty)
    #expect(other.ids == [two])
    #expect(
        defaults.object(forKey: "webStores") as? [String: [String]] == [
            "/tmp/test/.pitboard": [two.uuidString]
        ])
}

/// A release build run with another home shares WebKit's stores with the copy installed. A
/// store both directories recorded is the other one's to keep: a sweep here only forgets it.
@MainActor
@Test func aStoreAnotherDirectoryRecordedIsNotDeleted() async {
    let defaults = TestDefaults()
    let stores = StandInStores()
    let mine = StoreRecord(
        defaults: defaults, directory: "/test/.pitboard", directoryExists: { _ in true })
    let installed = StoreRecord(defaults: defaults, directory: "/Users/dana/.pitboard")
    installed.add(one)
    let janitor = janitor(stores, record: mine)
    _ = janitor.store(for: one)

    await janitor.sweep(keeping: [])
    #expect(stores.attempts[one] == nil)
    #expect(mine.ids.isEmpty)
    #expect(installed.ids == [one])
}

/// A directory that is gone, such as a test's scratch home, has no account using the store, so
/// its stale record does not keep the store on disk.
@MainActor
@Test func aStoreRecordedOnlyByADirectoryThatIsGoneIsDeleted() async {
    let defaults = TestDefaults()
    let stores = StandInStores()
    let mine = StoreRecord(
        defaults: defaults, directory: "/Users/dana/.pitboard",
        directoryExists: { $0 == "/Users/dana/.pitboard" })
    StoreRecord(defaults: defaults, directory: "/tmp/gone/.pitboard").add(one)
    let janitor = janitor(stores, record: mine)
    _ = janitor.store(for: one)

    await janitor.sweep(keeping: [])
    #expect(stores.removed == [one])
}

@MainActor
@Test func aWindowsLastPageIsKeptPerDirectoryUntilItsStoreGoes() {
    let defaults = TestDefaults()
    let pages = PageRecord(defaults: defaults, directory: "/a")
    let other = PageRecord(defaults: defaults, directory: "/b")
    let page = URL(string: "https://claude.ai/chat/1")!
    pages.set(page, of: one)
    other.set(page, of: two)
    #expect(pages.page(of: one) == page)
    #expect(pages.page(of: two) == nil)
    pages.keep(only: [two])
    #expect(pages.page(of: one) == nil)
    #expect(other.page(of: two) == page, "another directory's pages stay")
    other.set(nil, of: two)
    #expect(defaults.object(forKey: "windowPages") == nil)
}
