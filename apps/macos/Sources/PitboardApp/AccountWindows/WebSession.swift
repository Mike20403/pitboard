import AppKit
import Foundation
import PitboardKit
import WebKit

/// One account's window on its site while it is open: the account, the policy its pages
/// follow, its own page, the sign-in window one of its pages opened, and what it has to say.
@MainActor
@Observable
final class WebSession {
    private(set) var account: WindowAccount
    let policy: NavigationPolicy
    /// The window's own page. Made again when the window's data is removed, so nothing the
    /// signed-in page does can land in the store after it is cleared.
    private(set) var page: Page
    /// What the bar above the page says, until it is dismissed or replaced.
    var note: WindowNote?
    /// File > Remove Website Data… was chosen for this window, which asks first.
    var removalAsked = false

    @ObservationIgnored private let delegate: PageDelegate
    @ObservationIgnored private let downloads: DownloadCenter
    @ObservationIgnored private let openElsewhere: @MainActor (URL) -> Void
    /// What a page of this window is made from: the account's store and the environment's
    /// own setup.
    @ObservationIgnored private let configuration: () -> WKWebViewConfiguration
    /// The sign-in window a page of this one opened, while it is open. One at a time: a
    /// second replaces the first, as a site starting its sign-in again expects.
    @ObservationIgnored private(set) var popup: PopupWindow?

    /// The window of `account`, on `store`, starting at `firstPage`. `storeMadeBefore` says
    /// this Pitboard directory made the store before; a window whose store is new says how to
    /// sign in.
    init(
        account: WindowAccount, policy: NavigationPolicy, store: WKWebsiteDataStore,
        environment: WebEnvironment, downloads: DownloadCenter, firstPage: URL,
        storeMadeBefore: Bool
    ) {
        self.account = account
        self.policy = policy
        self.downloads = downloads
        openElsewhere = environment.openElsewhere
        let configure = environment.configure
        configuration = { Page.configuration(store: store, configure: configure) }
        page = Page(role: .window, configuration: configuration())
        delegate = PageDelegate()
        delegate.attach(to: self, page: page)
        note = openingNote(storeMadeBefore: storeMadeBefore).map {
            WindowNote($0, for: account)
        }
        page.load(firstPage)
    }

    /// Takes the account as a read found it again: a rename retitles the window and keeps
    /// everything else.
    func update(_ account: WindowAccount) {
        guard account != self.account else { return }
        self.account = account
    }

    /// Opens `url`, one of the site's own links, in this window. Back returns to where the
    /// window was.
    func open(_ url: URL) {
        page.load(url)
    }

    /// Stops everything, for good: the window has closed. Downloads carry on.
    func close() {
        popup?.close()
        page.close()
    }

    /// The account's store, which every page of this window keeps its data in.
    var store: WKWebsiteDataStore {
        page.webView.configuration.websiteDataStore
    }

    /// Starts the window again with a page of its own at the site's home, as a window that
    /// has never been signed in, once its data has been removed.
    func startOver() {
        page = Page(role: .window, configuration: configuration())
        delegate.attach(to: self, page: page)
        note = openingNote(storeMadeBefore: false).map { WindowNote($0, for: account) }
        page.load(policy.home)
    }

    // MARK: - What the page's delegate hands over

    /// The navigation `action` asks for of `page`, as the policy sees it.
    func request(for action: WKNavigationAction, of page: Page) -> NavigationRequest {
        let target = NavigationTarget(frameIsMain: action.targetFrame?.isMainFrame)
        let frame = Self.askingFrame(of: action)
        let asker: Asker
        switch page.role {
        case .window:
            asker = self.asker(frame)
        case .popup:
            // Only the sign-in window's own page asks for itself; its opener does not.
            let own = frame?.isMainFrame == true && frame?.webView === page.webView
            asker = own ? .signIn : .other
        }
        return NavigationRequest(
            // A window asked for with no address yet comes with an empty one.
            url: (action.request.url ?? URL(string: "about:blank")!).absoluteString,
            target: target,
            clicked: [.linkActivated, .formSubmitted].contains(action.navigationType),
            download: action.shouldPerformDownload,
            asker: asker)
    }

    /// Which page `frame` is, as the policy tells pages apart.
    func asker(_ frame: WKFrameInfo?) -> Asker {
        guard let frame else { return .other }
        let origin = frame.securityOrigin
        return frameAsker(
            policy: policy,
            frame: FrameOrigin(
                mainFrame: frame.isMainFrame, scheme: origin.protocol, host: origin.host,
                port: Int64(origin.port)))
    }

    /// The frame that asked for `action`. WebKit's header says there always is one, and WebKit
    /// leaves it out for a load the app starts (WebKit bug 173235), so it is read as one that
    /// may be missing rather than trusted to be there.
    static func askingFrame(of action: WKNavigationAction) -> WKFrameInfo? {
        action.value(forKey: #keyPath(WKNavigationAction.sourceFrame)) as? WKFrameInfo
    }

    /// Says `kind` in the bar above the page.
    func say(_ kind: WindowNoteKind) {
        note = WindowNote(kind, for: account)
    }

    /// Hands `url` to macOS, then says `note` when the policy gave one: a web page the site
    /// sent to the browser without a click is usually a sign-in to connect something, which
    /// the browser connects to whichever account of the site it is signed in to.
    func handOver(_ url: String, note: WindowNoteKind?) {
        guard let url = URL(string: url) else { return }
        openElsewhere(url)
        if let note { say(note) }
    }

    /// A sign-in window for a page of this one, made from `configuration`, which WebKit hands
    /// over as a copy of the opener's: the window shares the account's store, and the page in
    /// it keeps `window.opener`.
    func openPopup(
        configuration: WKWebViewConfiguration, features: WKWindowFeatures,
        over parent: NSWindow?
    ) -> WKWebView {
        popup?.close()
        let page = Page(role: .popup, configuration: configuration)
        let size = signInWindowSize(
            width: features.width?.doubleValue, height: features.height?.doubleValue)
        let opened = PopupWindow(
            page: page, session: self, size: CGSize(width: size.width, height: size.height),
            over: parent)
        opened.onClose = { [weak self, weak opened] in
            if self?.popup === opened { self?.popup = nil }
        }
        popup = opened
        return page.webView
    }

    /// The sign-in window's page asked to close, as a sign-in does once it is done.
    func closePopup() {
        popup?.close()
    }

    /// Hands a download a page started to the downloads, asking first when `ask` says to.
    func download(_ download: WKDownload, ask: Bool) {
        downloads.start(download, for: account, asking: ask)
    }
}
