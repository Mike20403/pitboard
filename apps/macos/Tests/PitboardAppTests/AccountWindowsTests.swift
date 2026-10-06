import AppKit
import Foundation
import PitboardKit
import Testing
import WebKit

@testable import PitboardApp

// MARK: - The app's presence

/// What a presence applied and did, in order.
@MainActor
private final class PresenceLog {
    var said: [String] = []

    func presence() -> AppPresence {
        AppPresence(
            apply: { self.said.append($0 == .regular ? "regular" : "accessory") },
            bringForward: { self.said.append("activate") },
            settle: { await Task.yield() })
    }
}

/// Pitboard is a regular app while any of its windows is open, and a menu bar app while
/// none is.
@MainActor
@Test func pitboardIsRegularWhileAnyWindowIsOpen() async {
    let log = PresenceLog()
    let presence = log.presence()
    let main = UUID()
    let account = UUID()

    presence.opened(main)
    presence.opened(account)
    #expect(presence.hasWindows)
    presence.closed(main)
    await settled()
    #expect(log.said == ["regular", "regular"], "one window still open")
    presence.closed(account)
    await settled()
    #expect(log.said.last == "accessory")
    #expect(!presence.hasWindows)
}

/// The account picker closes before the window it opened appears. Going back to the menu bar
/// in between would give the app's activation away, so the window would open behind.
@MainActor
@Test func aWindowReplacingTheLastOneKeepsPitboardRegular() async {
    let log = PresenceLog()
    let presence = log.presence()
    let picker = UUID()
    presence.opened(picker)
    presence.activate()
    presence.closed(picker)
    presence.opened(UUID())
    await settled()
    #expect(!log.said.contains("accessory"))
}

/// A shared link can find Pitboard in the background, where its own activation is refused:
/// it asks Launch Services to open it instead.
@MainActor
@Test func aLinkFromOutsideBringsPitboardForwardFromTheBackground() {
    var said: [String] = []
    var active = false
    let presence = AppPresence(
        apply: { said.append($0 == .regular ? "regular" : "accessory") },
        bringForward: { said.append("activate") }, reopen: { said.append("reopen") },
        isActive: { active }, settle: {})
    presence.comeForward()
    #expect(said == ["regular", "reopen"])
    said = []
    active = true
    presence.comeForward()
    #expect(said == ["regular", "activate"])
}

/// Lets the presence's pending checks run.
@MainActor
private func settled() async {
    for _ in 0..<10 { await Task.yield() }
}

/// Measured on macOS 27: an app made regular while it is already active does not get the
/// menu bar until it is next activated, so it is made regular first.
@MainActor
@Test func bringingPitboardForwardMakesItRegularFirst() {
    let log = PresenceLog()
    log.presence().activate()
    #expect(log.said == ["regular", "activate"])
}

// MARK: - Windows and their sessions

/// The accounts of the fixture both tools are in, as a read lists them: Claude Code's work
/// in use, personal, and old, Codex's main in use and spare.
private let twoTools = [
    account("work", email: "dana@work.example", signedIn: true),
    account("personal", email: "dana@home.example"),
    account("old", email: "dana@old.example"),
    account("main", of: "codex", email: "dana@work.example", signedIn: true),
    account("spare", of: "codex", email: "dana@home.example"),
]

/// The account windows of `accounts`, read once through the app's model, on stand-in pages
/// and with stores a test can see. The model is a stand-in that reads nothing: a test hands
/// the app's model each snapshot, as the Rust model would.
@MainActor
private func windows(
    _ accounts: [Account] = twoTools, stores: StandInStores = StandInStores(),
    defaults: UserDefaults = TestDefaults()
) -> AccountWindows {
    let model = AppModel(model: StandInModel(snapshot(0)))
    let environment = WebEnvironment(
        scheme: WebEnvironment.fixtureScheme, stores: stores,
        record: StoreRecord(defaults: defaults, directory: "/test"),
        pages: PageRecord(defaults: defaults, directory: "/test"),
        downloads: FileManager.default.temporaryDirectory, openElsewhere: { _ in },
        configure: { _ in }, pause: { _ in })
    let windows = AccountWindows(
        model: model, environment: environment,
        presence: AppPresence(apply: { _ in }, bringForward: {}, settle: {}),
        scheme: "pitboard-debug")
    model.apply(snapshot(1, status: status(accounts)))
    return windows
}

@MainActor
private func account(_ label: String, in windows: AccountWindows) -> WindowAccount {
    windows.accounts.first { $0.label == label }!
}

@MainActor
@Test func theWindowsAreTheEnrolledAccountsOfEachSite() {
    let windows = windows()
    #expect(windows.accounts.map(\.label) == ["work", "personal", "old", "main", "spare"])
    let titles = windows.menus.map { menu in
        switch menu {
        case .one(let title, _), .several(let title, _, _): title
        }
    }
    #expect(titles == ["Open claude.ai", "Open chatgpt.com"])
    #expect(windows.account(account("main", in: windows).id)?.site == .chatGPT)
    #expect(
        windows.account(UUID(uuidString: account("work", in: windows).store.uppercased())!)?
            .label == "work",
        "a window's store read back from its scene, as UUID writes one")
}

/// A window starts at a link chosen for it, else at the page it was last on if that is one
/// of the site's own, else at the site's home: after it is closed as well as after a relaunch.
@MainActor
@Test func aWindowStartsAtItsLinkItsLastPageOrItsHome() throws {
    let windows = windows()
    let work = account("work", in: windows)
    let home = windows.session(for: work)
    #expect(home.page.webView.url?.absoluteString == "pitboard-fixture://claude.ai/")
    windows.remember(URL(string: "pitboard-fixture://claude.ai/chat/old"), of: work)
    windows.closed(work.id)

    let reopened = windows.session(for: work)
    #expect(
        reopened.page.webView.url?.absoluteString == "pitboard-fixture://claude.ai/chat/old")
    windows.closed(work.id)

    windows.remember(URL(string: "https://example.com/somewhere"), of: work)
    #expect(
        windows.session(for: work).page.webView.url?.absoluteString
            == "pitboard-fixture://claude.ai/chat/old",
        "a page off the site is never kept")
    windows.closed(work.id)

    windows.open(try SiteLink("https://claude.ai/chat/shared"), as: work)
    let linked = windows.session(for: work)
    #expect(
        linked.page.webView.url?.absoluteString == "pitboard-fixture://claude.ai/chat/shared")
    windows.closed(work.id)
}

/// A link chosen for an account whose window is open loads in that window, where Back returns
/// to what it showed.
@MainActor
@Test func aLinkForAnOpenWindowLoadsInIt() throws {
    let windows = windows()
    let main = account("main", in: windows)
    let session = windows.session(for: main)
    #expect(windows.session(for: main) === session, "one window per account")

    windows.open(try SiteLink("https://chatgpt.com/c/abc"), as: main)
    #expect(session.page.webView.url?.absoluteString == "pitboard-fixture://chatgpt.com/c/abc")
    windows.closed(main.id)
    #expect(windows.sessions.isEmpty)
}

/// A window that has never opened on this Mac says how to sign in; one that has does not.
@MainActor
@Test func onlyAWindowsFirstOpeningSaysHowToSignIn() {
    let windows = windows()
    let main = account("main", in: windows)
    let first = windows.session(for: main)
    #expect(first.note?.kind == .signIn)
    #expect(first.note?.text.contains("dana@work.example") == true)
    windows.closed(main.id)
    #expect(windows.session(for: main).note == nil)
    windows.closed(main.id)
}

/// A read that no longer lists an account, as the read after forgetting it in the app or
/// with `pitboard forget` in a terminal, closes its window and deletes its store, and
/// nobody else's, and its last page goes with it.
@MainActor
@Test func forgettingAnAccountClosesItsWindowAndDeletesItsStore() async {
    let stores = StandInStores()
    let defaults = TestDefaults()
    let windows = windows(stores: stores, defaults: defaults)
    let personal = account("personal", in: windows)
    let spare = account("spare", in: windows)
    _ = windows.session(for: personal)
    _ = windows.session(for: spare)
    windows.remember(URL(string: "pitboard-fixture://claude.ai/chat/x"), of: personal)

    windows.model.apply(
        snapshot(2, status: status(twoTools.filter { $0.label != "personal" })))
    #expect(windows.sessions.keys.sorted() == [spare.id])
    #expect(windows.account(personal.id) == nil)
    #expect(
        PageRecord(defaults: defaults, directory: "/test").page(of: personal.id) == nil,
        "its last page goes with it")
    // The sweep runs after the read; give it its turn.
    for _ in 0..<50 where stores.removed.isEmpty { await Task.yield() }
    #expect(stores.removed == [personal.id])
    windows.closed(spare.id)
}

/// A read that failed says nothing about who was forgotten, and nor does what stands in for
/// it, the last numbers measured: nothing is deleted, not even a store no account derives,
/// and no window closes.
@MainActor
@Test func aFailedReadDeletesNothing() async {
    let stores = StandInStores()
    let failed = ReadFailure(code: "unreachable", message: "Anthropic could not be reached")
    let model = AppModel(model: StandInModel(snapshot(0)))
    let windows = AccountWindows(
        model: model,
        environment: WebEnvironment(
            scheme: WebEnvironment.fixtureScheme, stores: stores,
            record: StoreRecord(defaults: TestDefaults(), directory: "/test"),
            pages: PageRecord(defaults: TestDefaults(), directory: "/test"),
            downloads: FileManager.default.temporaryDirectory, openElsewhere: { _ in },
            configure: { _ in }, pause: { _ in }),
        presence: AppPresence(apply: { _ in }, bringForward: {}, settle: {}),
        scheme: "pitboard-debug")
    let orphan = UUID()
    _ = windows.janitor.store(for: orphan)
    model.apply(snapshot(1, status: status(Array(twoTools.prefix(2))), readFailure: failed))
    _ = windows.session(for: account("work", in: windows))
    model.apply(snapshot(2, status: status(Array(twoTools.prefix(2))), readFailure: failed))
    for _ in 0..<20 { await Task.yield() }
    #expect(stores.removed.isEmpty)
    #expect(windows.janitor.hasMade(orphan))
    #expect(windows.sessions.count == 1)
}

/// What the poll reads once the account index changes says who is enrolled even while the
/// reads that ask a service fail: `pitboard forget` in a terminal, on a Mac that cannot reach
/// Anthropic, closes the account's window all the same.
@MainActor
@Test func forgettingOnTheCommandLineWhileReadsFailClosesTheWindow() async {
    let stores = StandInStores()
    let failed = ReadFailure(code: "unreachable", message: "Anthropic could not be reached")
    let windows = windows(stores: stores)
    let personal = account("personal", in: windows)
    _ = windows.session(for: personal)

    windows.model.apply(snapshot(2, status: status(twoTools), readFailure: failed))
    #expect(windows.sessions.keys.sorted() == [personal.id], "a failed read changes nothing")

    windows.model.apply(
        snapshot(
            3, status: status(twoTools.filter { $0.label != "personal" }), readFailure: failed))
    #expect(windows.sessions.isEmpty)
    for _ in 0..<50 where stores.removed.isEmpty { await Task.yield() }
    #expect(stores.removed == [personal.id])
}

/// The same on a Mac that has not reached Anthropic since Pitboard opened, whose accounts
/// stand in for a read that failed: what the poll reads once the account index changes says
/// who is enrolled, as the Swift model said it, and `pitboard forget` closes the window.
@MainActor
@Test func forgettingOnTheCommandLineWhileEveryReadHasFailedClosesTheWindow() async {
    let stores = StandInStores()
    let failed = ReadFailure(code: "unreachable", message: "Anthropic could not be reached")
    let model = AppModel(model: StandInModel(snapshot(0)))
    let windows = AccountWindows(
        model: model,
        environment: WebEnvironment(
            scheme: WebEnvironment.fixtureScheme, stores: stores,
            record: StoreRecord(defaults: TestDefaults(), directory: "/test"),
            pages: PageRecord(defaults: TestDefaults(), directory: "/test"),
            downloads: FileManager.default.temporaryDirectory, openElsewhere: { _ in },
            configure: { _ in }, pause: { _ in }),
        presence: AppPresence(apply: { _ in }, bringForward: {}, settle: {}),
        scheme: "pitboard-debug")
    model.apply(snapshot(1, status: status(twoTools), readFailure: failed))
    let personal = account("personal", in: windows)
    _ = windows.session(for: personal)

    model.apply(
        snapshot(
            2, status: status(twoTools.filter { $0.label != "personal" }), readFailure: failed))
    #expect(windows.sessions.isEmpty)
    for _ in 0..<50 where stores.removed.isEmpty { await Task.yield() }
    #expect(stores.removed == [personal.id])
}

/// The Dock icon's menu asks for a window from outside any view; the menu bar item opens it.
@MainActor
@Test func aWindowAskedForFromTheDockWaitsForTheMenuBarItem() {
    let windows = windows()
    let work = account("work", in: windows)
    windows.request(work)
    #expect(windows.requested == [work.id])
    #expect(windows.takeRequests() == [work.id])
    #expect(windows.requested.isEmpty)
}

// MARK: - Pages

// What a page may do, what a download is called and what a dialog is titled are the core's
// rules, tested in pitboard-ffi. These are WebKit's side.

@Test func zoomFollowsSafarisSteps() {
    #expect(Page.zoom(from: 1, toward: .zoomIn) == 1.15)
    #expect(Page.zoom(from: 1, toward: .zoomOut) == 0.85)
    #expect(Page.zoom(from: 3, toward: .zoomIn) == 3, "it stays at the ends")
    #expect(Page.zoom(from: 0.5, toward: .zoomOut) == 0.5)
    #expect(Page.zoom(from: 1.3, toward: .zoomIn) == 1.5, "from between two steps")
    #expect(Page.zoom(from: 2.5, toward: .actualSize) == 1)
}

/// A load stopped for another, or by a decision such as a download, is no failure worth a
/// word; anything else is said in place of the page.
@Test func onlyRealFailuresAreShown() {
    #expect(!Page.isShown(NSError(domain: NSURLErrorDomain, code: NSURLErrorCancelled)))
    #expect(!Page.isShown(NSError(domain: "WebKitErrorDomain", code: 102)))
    #expect(
        Page.isShown(NSError(domain: NSURLErrorDomain, code: NSURLErrorNotConnectedToInternet)))
    #expect(Page.isShown(NSError(domain: "WebKitErrorDomain", code: 101)))
}

/// A page that keeps ending its content process says so in place of loading forever, as the
/// core's rule decides from when it last ended: a heavy page ends it after it has loaded, so
/// loading again is no sign it works.
@MainActor
@Test func aPageThatKeepsCrashingSaysSo() {
    var now = Date(timeIntervalSince1970: 0)
    let page = Page(
        role: .window, configuration: WKWebViewConfiguration(), now: { now })
    page.contentEnded()
    #expect(page.failure == nil, "loaded again once")
    page.loadCommitted()
    now += 5
    page.contentEnded()
    #expect(page.failure == "The page stopped working.")

    page.reload()
    // Past the minute within which a second end counts.
    now += 61
    page.contentEnded()
    #expect(page.failure == nil, "a crash long after the last one is loaded again")
    page.close()
}

/// WebKit's own download items in a page's shortcut menu would do nothing, so they are left
/// out; everything else stays.
@Test func aPagesShortcutMenuLeavesOutWebKitsDownloadItems() {
    #expect(!PageWebView.keeps("WKMenuItemIdentifierDownloadLinkedFile"))
    #expect(!PageWebView.keeps("WKMenuItemIdentifierDownloadImage"))
    #expect(!PageWebView.keeps("WKMenuItemIdentifierDownloadMedia"))
    #expect(PageWebView.keeps("WKMenuItemIdentifierCopyLink"))
    #expect(PageWebView.keeps(""))
}
