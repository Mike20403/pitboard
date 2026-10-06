import Foundation
import PitboardKit
import WebKit

/// Every account's window on its site, as WebKit has it: the session of each open window,
/// the windows asked for from outside any view, and what deletes the windows' stores and
/// keeps their downloads.
///
/// What the windows show is the model's, in `AppModel.accountWindows`: which accounts have
/// one, which store keeps each one's data and which this Pitboard directory made, the page
/// each window starts at and each link opened in it, which windows close once a read finds
/// their account forgotten, which stores go, the link waiting for an account and the
/// downloads. This tells the model what WebKit and the windows did, and does what it says.
@MainActor
@Observable
public final class AccountWindows {
    @ObservationIgnored let model: AppModel
    @ObservationIgnored let presence: AppPresence
    @ObservationIgnored let janitor: StoreJanitor
    let downloads: DownloadCenter
    @ObservationIgnored private let environment: WebEnvironment

    /// The session of each open window, by its account's store.
    private(set) var sessions: [UUID: WebSession] = [:]
    /// Windows asked for from outside any view, such as the Dock's menu, which the menu bar
    /// item opens, since only a view can open a window.
    private(set) var requested: [UUID] = []

    init(model: AppModel, environment: WebEnvironment, presence: AppPresence) {
        self.model = model
        self.environment = environment
        self.presence = presence
        janitor = StoreJanitor(stores: environment.stores, pause: environment.pause)
        downloads = DownloadCenter(folder: environment.downloads, model: model)
        model.accountWindowsChanged = { [weak self] shown in self?.follow(shown) }
    }

    /// Every enrolled account's window, as the model has the accounts.
    var accounts: [WindowAccount] { model.accountWindows.accounts }

    /// The menus' entries, one per site that has an account with a window.
    var menus: [SiteMenu] { model.accountWindows.menus }

    /// The account whose window keeps `store`, while it is enrolled.
    func account(_ store: UUID) -> WindowAccount? {
        accounts.first { $0.id == store }
    }

    /// The window of `store` as the model has it open, once it can show its page.
    func opened(_ store: UUID) -> OpenWindow? {
        model.accountWindows.open.first { $0.id == store }
    }

    /// Whether the model says the window of `store` closes: a read no longer lists its
    /// account.
    func isClosing(_ store: UUID) -> Bool {
        model.accountWindows.closing.contains { UUID(uuidString: $0) == store }
    }

    /// The navigation policy of `site`'s windows.
    func policy(for site: Site) -> NavigationPolicy {
        NavigationPolicy(site: site, scheme: environment.scheme)
    }

    // MARK: - Windows

    /// The window of `store` has opened. The model records its store, and says once it has
    /// what the window shows.
    func opening(_ store: UUID) {
        model.send(.windowOpened(store: store.uuidString))
    }

    /// The session of the window `open` is, made with the page the model says it starts at
    /// as it first shows it, and loading each page the model asks of it after that, once.
    func session(for open: OpenWindow) -> WebSession {
        if let session = sessions[open.id] {
            session.update(open.account)
            session.load(open.load)
            return session
        }
        let session = WebSession(
            open: open, policy: policy(for: open.account.site),
            store: janitor.store(for: open.id), environment: environment, downloads: downloads)
        sessions[open.id] = session
        return session
    }

    /// The window that keeps `store` has closed.
    func closed(_ store: UUID) {
        sessions.removeValue(forKey: store)?.close()
        model.send(.windowClosed(store: store.uuidString))
    }

    /// Asks for `account`'s window from outside any view.
    func request(_ account: WindowAccount) {
        requested.append(account.id)
    }

    /// The windows asked for, which are then no longer asked for.
    func takeRequests() -> [UUID] {
        defer { requested = [] }
        return requested
    }

    /// A link the app was asked to open, which waits for somebody to choose an account.
    func receive(_ link: URL) {
        model.send(.linkArrived(text: link.absoluteString))
    }

    /// Removes everything the window of `session` keeps on this Mac, which signs the window
    /// out of its site without telling the site, and starts the window again at its home.
    ///
    /// The signed-in page leaves first, so nothing it still does, a response setting a cookie
    /// or a page leaving and saving its state, lands in the store after it is cleared, and
    /// Back cannot bring it back.
    func removeWebsiteData(of session: WebSession) async {
        let store = session.store
        session.closePopup()
        await session.page.leave()
        session.close()
        model.send(.websiteDataRemoved(store: session.account.store))
        await janitor.wipe(store)
        session.startOver()
    }

    /// The window of `account` is on `page`, which the model keeps as the page it opens at
    /// next time where it is one of the site's own.
    func remember(_ page: URL?, of account: WindowAccount) {
        guard let page else { return }
        model.send(.pageShown(store: account.store, url: page.absoluteString))
    }

    // MARK: - What the model says

    /// Does what the model says of the windows: the session of a window whose account a read
    /// no longer lists stops, its view closing the window, and each store asked for is
    /// deleted once, the model told how it went.
    func follow(_ shown: AccountWindowsShown) {
        for store in shown.closing.compactMap(UUID.init(uuidString:)) {
            sessions.removeValue(forKey: store)?.close()
        }
        let model = model
        janitor.delete(shown.deleting) { store, deleted in
            model.send(
                deleted
                    ? .storeDeleted(store: store.uuidString)
                    : .storeHeld(store: store.uuidString))
        }
    }
}
