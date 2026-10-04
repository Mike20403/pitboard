import Foundation
import WebKit

/// Where each account's window keeps what its site stores: cookies, local storage and
/// everything else a browser keeps for a site, in one WebKit store per account.
///
/// pitboard makes a store, wipes one and deletes one. It never reads what is in one, copies
/// it or changes it: WebKit keeps a sign-in exactly as a browser would.
@MainActor
protocol WebsiteDataStores: AnyObject {
    /// The store for `id`, made if there is none. While an object for an identifier is
    /// alive, WebKit hands back that same object, so every page of an account shares it.
    func store(for id: UUID) -> WKWebsiteDataStore
    /// Deletes the store `id` names and everything in it. Throws while any object, web view
    /// or WebKit process still holds it; succeeds for an identifier with no store.
    func remove(_ id: UUID) async throws
}

/// WebKit's persistent stores, under `~/Library/WebKit/<bundle id>/WebsiteDataStore`.
@MainActor
final class WebKitDataStores: WebsiteDataStores {
    private var ready = false

    func store(for id: UUID) -> WKWebsiteDataStore {
        ready = true
        return WKWebsiteDataStore(forIdentifier: id)
    }

    func remove(_ id: UUID) async throws {
        setUpWebKit()
        try await WKWebsiteDataStore.remove(forIdentifier: id)
    }

    /// Makes sure WebKit is set up before a deletion asks it to remove a store.
    ///
    /// WebKit answers a deletion on a main run loop it sets up when it makes its first
    /// object, and `remove(forIdentifier:)` does not set it up itself. Asked first, as a sweep
    /// at launch asks before any window has opened, it crashes: measured on macOS 27,
    /// `remove(forIdentifier:)` as a process's first WebKit call ends in a segmentation fault
    /// in `WTF::RunLoop::dispatch`, and making any store first prevents it. A non-persistent
    /// store touches no disk and starts no process. Done here, not at launch, so a person who
    /// never opens a window never starts WebKit.
    private func setUpWebKit() {
        guard !ready else { return }
        _ = WKWebsiteDataStore.nonPersistent()
        ready = true
    }
}

/// The stores one pitboard directory has made, kept in the app's preferences under that
/// directory's path.
///
/// WebKit keeps every store of one app under the person's own Library, whatever `HOME` says,
/// while the accounts come from `PITBOARD_HOME` or `HOME`. So a copy of the app with the same
/// bundle identifier run with another home, as a release build is tested, shares stores with
/// the copy installed. A store it did not record making is not evidence of a forgotten
/// account, and nor is one another directory recorded too, which an account enrolled in both
/// derives: deleting either would sign the other copy's window out.
@MainActor
struct StoreRecord {
    let defaults: UserDefaults
    /// The pitboard directory the app reads its accounts from.
    let directory: String
    /// Whether a pitboard directory is still there.
    var directoryExists: (String) -> Bool = { FileManager.default.fileExists(atPath: $0) }

    var ids: Set<UUID> {
        Set((all[directory] ?? []).compactMap(UUID.init(uuidString:)))
    }

    func add(_ id: UUID) {
        guard !ids.contains(id) else { return }
        save(ids.union([id]))
    }

    func remove(_ id: UUID) {
        guard ids.contains(id) else { return }
        save(ids.subtracting([id]))
    }

    /// Whether another pitboard directory that is still there recorded `id` as well. One that
    /// is gone, such as a test's scratch home, has no account using it any more.
    func isShared(_ id: UUID) -> Bool {
        all.contains { directory, ids in
            directory != self.directory && ids.contains(id.uuidString)
                && directoryExists(directory)
        }
    }

    private var all: [String: [String]] {
        defaults.object(forKey: DefaultsKey.webStores) as? [String: [String]] ?? [:]
    }

    private func save(_ ids: Set<UUID>) {
        var all = self.all
        all[directory] = ids.isEmpty ? nil : ids.map(\.uuidString).sorted()
        if all.isEmpty {
            defaults.removeObject(forKey: DefaultsKey.webStores)
        } else {
            defaults.set(all, forKey: DefaultsKey.webStores)
        }
    }
}

/// The page each account's window was last on, kept in the app's preferences under the
/// pitboard directory's path, so a window opens there again: after it is closed, and after
/// pitboard quits. Only the site's own pages are kept, and a window's page goes when its data
/// is removed or its account is forgotten.
@MainActor
struct PageRecord {
    let defaults: UserDefaults
    /// The pitboard directory the app reads its accounts from.
    let directory: String

    /// The page the window that keeps `store` was last on.
    func page(of store: UUID) -> URL? {
        (all[directory]?[store.uuidString]).flatMap(URL.init(string:))
    }

    /// Keeps `page` as the page of the window that keeps `store`, or forgets it for nil.
    func set(_ page: URL?, of store: UUID) {
        var pages = all[directory] ?? [:]
        guard pages[store.uuidString] != page?.absoluteString else { return }
        pages[store.uuidString] = page?.absoluteString
        save(pages)
    }

    /// Forgets the pages of every window not in `stores`.
    func keep(only stores: Set<UUID>) {
        let pages = all[directory] ?? [:]
        let kept = pages.filter { key, _ in UUID(uuidString: key).map(stores.contains) == true }
        if kept.count != pages.count { save(kept) }
    }

    private var all: [String: [String: String]] {
        defaults.object(forKey: DefaultsKey.windowPages) as? [String: [String: String]] ?? [:]
    }

    private func save(_ pages: [String: String]) {
        var all = self.all
        all[directory] = pages.isEmpty ? nil : pages
        if all.isEmpty {
            defaults.removeObject(forKey: DefaultsKey.windowPages)
        } else {
            defaults.set(all, forKey: DefaultsKey.windowPages)
        }
    }
}
