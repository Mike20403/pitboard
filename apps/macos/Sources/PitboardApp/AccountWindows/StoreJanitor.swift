import Foundation
import PitboardKit
import WebKit

/// The account windows' stores as WebKit has them: makes each one a window needs, wipes one
/// in place, and deletes the ones the model asks it to, trying a store WebKit still holds
/// again a few times.
///
/// Which stores go, and when, is the model's: one this Pitboard directory recorded making,
/// that a read which succeeded no longer derives from any enrolled account and that no other
/// Pitboard directory still there recorded too, which it asks for in
/// `AccountWindowsShown.deleting`. The model records a store before it says a window opens,
/// so every store made here is one it can ask for.
@MainActor
final class StoreJanitor {
    private let stores: any WebsiteDataStores
    private let pause: @MainActor (Duration) async -> Void
    /// The asks taken, while the model still lists them: each is deleted once, however many
    /// snapshots list it before the model has heard how it went.
    private var taken: Set<StoreDeletion> = []

    /// How long to wait between attempts to delete a store WebKit still holds. Measured on
    /// macOS 27: a store whose last web view and object are released stays in use for 20 to
    /// 60 ms; one an object still holds stays in use until it is released.
    nonisolated static let retries: [Duration] = [50, 100, 200, 400, 800, 1600].map {
        .milliseconds($0)
    }

    init(stores: any WebsiteDataStores, pause: @escaping @MainActor (Duration) async -> Void) {
        self.stores = stores
        self.pause = pause
    }

    /// The store for the account window `id` names, which the model recorded before it said
    /// the window opens.
    func store(for id: UUID) -> WKWebsiteDataStore {
        stores.store(for: id)
    }

    /// Removes everything `store` keeps, while its windows go on using it: cookies, storage,
    /// caches and the rest. Measured on macOS 27, this clears a live store's cookies, the
    /// ones pages cannot read included, its local and session storage and its databases.
    func wipe(_ store: WKWebsiteDataStore) async {
        await store.removeData(
            ofTypes: WKWebsiteDataStore.allWebsiteDataTypes(), modifiedSince: .distantPast)
    }

    /// Deletes each store `asked` names that is not being deleted already, telling `told` of
    /// each whether it went. One still in use is tried again a few times, then told as held,
    /// for the model to ask for again after its next read.
    func delete(
        _ asked: [StoreDeletion], told: @escaping @MainActor (UUID, Bool) -> Void
    ) {
        taken.formIntersection(asked)
        for ask in asked where !taken.contains(ask) {
            taken.insert(ask)
            // The model drops a recorded store id that is not a UUID as it reads the records,
            // so it never asks for one.
            guard let id = UUID(uuidString: ask.store) else { continue }
            Task {
                told(id, await delete(id))
            }
        }
    }

    /// Deletes the store `id` names, trying again after each pause while WebKit still holds
    /// it. Whether it went.
    private func delete(_ id: UUID) async -> Bool {
        var waits = Self.retries[...]
        while true {
            do {
                try await stores.remove(id)
                return true
            } catch {
                guard let wait = waits.popFirst() else { return false }
                await pause(wait)
            }
        }
    }
}
