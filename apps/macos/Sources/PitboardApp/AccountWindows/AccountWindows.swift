import Foundation
import PitboardKit
import WebKit

/// Every account's window on its site: which accounts have one, the session of each open
/// window, the links waiting for a window still opening, and what looks after the windows'
/// stores and downloads.
///
/// Beside the app's model rather than inside it. It reads the accounts the model last read,
/// and is told when a read succeeds, which is when it puts away what a forgotten account's
/// window kept.
@MainActor
@Observable
public final class AccountWindows {
    @ObservationIgnored let model: AppModel
    @ObservationIgnored let presence: AppPresence
    @ObservationIgnored let janitor: StoreJanitor
    let downloads: DownloadCenter
    let inbox: LinkInbox
    @ObservationIgnored private let environment: WebEnvironment

    /// The session of each open window, by its account's store.
    private(set) var sessions: [UUID: WebSession] = [:]
    /// Links chosen for a window that is still opening, which become its first page.
    @ObservationIgnored private var waiting: [UUID: URL] = [:]
    /// Windows asked for from outside any view, such as the Dock's menu, which the menu bar
    /// item opens, since only a view can open a window.
    private(set) var requested: [UUID] = []

    init(model: AppModel, environment: WebEnvironment, presence: AppPresence, scheme: String) {
        self.model = model
        self.environment = environment
        self.presence = presence
        janitor = StoreJanitor(
            stores: environment.stores, record: environment.record, pause: environment.pause)
        downloads = DownloadCenter(folder: environment.downloads)
        inbox = LinkInbox(scheme: scheme)
        model.afterRead = { [weak self] read in self?.accountsRead(read) }
    }

    /// Every enrolled account's window, as the last read found the accounts.
    var accounts: [WindowAccount] { windowAccounts(in: model.status) }

    /// The menus' entries, one per site that has an account with a window.
    var menus: [SiteMenu] { siteMenus(in: model.status) }

    /// The account whose window keeps `store`, while it is enrolled.
    func account(_ store: UUID) -> WindowAccount? {
        accounts.first { $0.store == store }
    }

    /// The navigation policy of `site`'s windows.
    func policy(for site: Site) -> NavigationPolicy {
        NavigationPolicy(site: site, scheme: environment.scheme)
    }

    // MARK: - Windows

    /// The session of `account`'s window, made when the window opens. Its first page is a
    /// link chosen for it, else the last page of its site the window showed, else the site's
    /// home.
    func session(for account: WindowAccount) -> WebSession {
        if let session = sessions[account.store] {
            session.update(account)
            return session
        }
        let policy = policy(for: account.site)
        let restored = environment.pages.page(of: account.store).flatMap {
            policy.isSite($0) ? $0 : nil
        }
        // Asked before the store is made, which records it.
        let firstOpen = !janitor.hasMade(account.store)
        let session = WebSession(
            account: account, policy: policy, store: janitor.store(for: account.store),
            environment: environment, downloads: downloads,
            firstPage: waiting.removeValue(forKey: account.store) ?? restored ?? policy.home,
            firstOpen: firstOpen)
        sessions[account.store] = session
        return session
    }

    /// The window that keeps `store` has closed.
    func closed(_ store: UUID) {
        sessions.removeValue(forKey: store)?.close()
    }

    /// Opens `link` as `account`: in its window when that is open, where Back returns to what
    /// the window showed; else as the first page of the window about to open.
    func open(_ link: SiteLink, as account: WindowAccount) {
        let url = policy(for: account.site).address(of: link)
        if let session = sessions[account.store] {
            session.open(url)
        } else {
            waiting[account.store] = url
        }
    }

    /// Asks for `account`'s window from outside any view.
    func request(_ account: WindowAccount) {
        requested.append(account.store)
    }

    /// The windows asked for, which are then no longer asked for.
    func takeRequests() -> [UUID] {
        defer { requested = [] }
        return requested
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
        environment.pages.set(nil, of: session.account.store)
        await janitor.wipe(store)
        session.startOver()
    }

    /// Keeps `page` as the page `account`'s window is on, when it is one of the site's own, so
    /// the window opens there again.
    func remember(_ page: URL?, of account: WindowAccount) {
        guard let page, policy(for: account.site).isSite(page) else { return }
        environment.pages.set(page, of: account.store)
    }

    // MARK: - After a read

    /// What a read that succeeded found: windows of accounts no longer enrolled close, the
    /// others take the account as found, and every store no enrolled account has any more is
    /// deleted. Only a read that succeeded says who is enrolled; every one lists every
    /// enrolled account.
    private func accountsRead(_ read: Status) {
        let found = windowAccounts(in: read)
        let keeping = Set(found.map(\.store))
        for (store, session) in sessions {
            if let account = found.first(where: { $0.store == store }) {
                session.update(account)
            } else {
                closed(store)
            }
        }
        waiting = waiting.filter { keeping.contains($0.key) }
        environment.pages.keep(only: keeping)
        Task { await janitor.sweep(keeping: keeping) }
    }
}

extension NavigationPolicy {
    /// `link` on the window's scheme: the link itself in a live run, and the fixture's
    /// stand-in for it in a fixture, so a fixture's window never reaches the network.
    func address(of link: SiteLink) -> URL {
        guard var parts = URLComponents(string: link.url) else {
            return home
        }
        parts.scheme = scheme
        return parts.url ?? home
    }
}
