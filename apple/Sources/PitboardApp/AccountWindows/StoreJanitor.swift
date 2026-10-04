import Foundation
import WebKit

/// Looks after the account windows' stores: makes each one on a window's first need,
/// records it first, wipes one in place, and deletes the ones no enrolled account has any
/// more.
///
/// A store is deleted only when this pitboard recorded making it and a read of the accounts
/// that succeeded no longer derives it, which is what forgetting an account leaves behind, in
/// the app or on the command line. Nothing is deleted before the first read that succeeds, or
/// after one that failed.
@MainActor
final class StoreJanitor {
    private let stores: any WebsiteDataStores
    private let record: StoreRecord
    private let pause: @MainActor (Duration) async -> Void
    /// Stores being deleted, so two sweeps never delete one store at once.
    private var deleting: Set<UUID> = []

    /// How long to wait between attempts to delete a store WebKit still holds. Measured on
    /// macOS 27: a store whose last web view and object are released stays in use for 20 to
    /// 60 ms; one an object still holds stays in use until it is released.
    nonisolated static let retries: [Duration] = [50, 100, 200, 400, 800, 1600].map {
        .milliseconds($0)
    }

    init(
        stores: any WebsiteDataStores, record: StoreRecord,
        pause: @escaping @MainActor (Duration) async -> Void
    ) {
        self.stores = stores
        self.record = record
        self.pause = pause
    }

    /// The store for the account window `id` names, recorded as this pitboard's before it is
    /// made: a store WebKit has made and nobody recorded would never be deleted.
    func store(for id: UUID) -> WKWebsiteDataStore {
        record.add(id)
        return stores.store(for: id)
    }

    /// Whether this pitboard has made the store `id` names: a window that has never opened
    /// has nothing in it yet, so its page says how to sign in.
    func hasMade(_ id: UUID) -> Bool {
        record.ids.contains(id)
    }

    /// Removes everything `store` keeps, while its windows go on using it: cookies, storage,
    /// caches and the rest. Measured on macOS 27, this clears a live store's cookies, the
    /// ones pages cannot read included, its local and session storage and its databases.
    func wipe(_ store: WKWebsiteDataStore) async {
        await store.removeData(
            ofTypes: WKWebsiteDataStore.allWebsiteDataTypes(), modifiedSince: .distantPast)
    }

    /// Deletes every store this pitboard recorded that is not in `keeping`, the stores of the
    /// enrolled accounts a read that succeeded found. A store still in use is tried again a
    /// few times, then left recorded for the next sweep.
    func sweep(keeping: Set<UUID>) async {
        let orphans = record.ids.subtracting(keeping).subtracting(deleting)
        // Claimed before any deletion starts, so a sweep that starts meanwhile leaves them.
        deleting.formUnion(orphans)
        await withTaskGroup(of: Void.self) { group in
            for id in orphans {
                group.addTask { await self.delete(id) }
            }
        }
    }

    private func delete(_ id: UUID) async {
        defer { deleting.remove(id) }
        // Another pitboard directory's account derives it as well: it is that one's to keep.
        if record.isShared(id) {
            record.remove(id)
            return
        }
        var waits = Self.retries[...]
        while true {
            do {
                try await stores.remove(id)
                record.remove(id)
                return
            } catch {
                guard let wait = waits.popFirst() else { return }
                await pause(wait)
            }
        }
    }
}
