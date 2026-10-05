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

/// The account windows of a fixture's accounts, read once, on stand-in pages and with stores
/// a test can see.
@MainActor
private func windows(
    _ fixture: Fixture = .twoTools, stores: StandInStores = StandInStores(),
    defaults: UserDefaults = TestDefaults()
) async -> AccountWindows {
    let model = AppModel(testing: FixtureCore(fixture))
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
    await model.refresh()
    return windows
}

@MainActor
private func account(_ label: String, in windows: AccountWindows) -> WindowAccount {
    windows.accounts.first { $0.label == label }!
}

@MainActor
@Test func theWindowsAreTheEnrolledAccountsOfEachSite() async {
    let windows = await windows()
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
@Test func aWindowStartsAtItsLinkItsLastPageOrItsHome() async throws {
    let windows = await windows()
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
@Test func aLinkForAnOpenWindowLoadsInIt() async throws {
    let windows = await windows()
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
@Test func onlyAWindowsFirstOpeningSaysHowToSignIn() async {
    let windows = await windows()
    let main = account("main", in: windows)
    let first = windows.session(for: main)
    #expect(first.note?.kind == .signIn)
    #expect(first.note?.text.contains("dana@work.example") == true)
    windows.closed(main.id)
    #expect(windows.session(for: main).note == nil)
    windows.closed(main.id)
}

/// Forgetting an account closes its window and deletes its store, and nobody else's.
@MainActor
@Test func forgettingAnAccountClosesItsWindowAndDeletesItsStore() async {
    let stores = StandInStores()
    let windows = await windows(stores: stores)
    let personal = account("personal", in: windows)
    let spare = account("spare", in: windows)
    _ = windows.session(for: personal)
    _ = windows.session(for: spare)

    _ = await windows.model.forget("claude/personal")
    #expect(windows.sessions.keys.sorted() == [spare.id])
    #expect(windows.account(personal.id) == nil)
    // The sweep runs after the read; give it its turn.
    for _ in 0..<50 where stores.removed.isEmpty { await Task.yield() }
    #expect(stores.removed == [personal.id])
    windows.closed(spare.id)
}

/// A read that failed says nothing about who was forgotten, so nothing is deleted, not even a
/// store no account derives.
@MainActor
@Test func aFailedReadDeletesNothing() async {
    let stores = StandInStores()
    let windows = await windows(.readFailure, stores: stores)
    _ = windows.session(for: account("work", in: windows))
    let orphan = UUID()
    _ = windows.janitor.store(for: orphan)
    await windows.model.refresh(asked: true)
    for _ in 0..<20 { await Task.yield() }
    #expect(stores.removed.isEmpty)
    #expect(windows.janitor.hasMade(orphan))
    #expect(windows.sessions.count == 1)
}

/// `pitboard forget` in a terminal reaches the app through the change it notices on this Mac:
/// the account's window closes and its store is deleted.
@MainActor
@Test func forgettingOnTheCommandLineClosesTheWindowAndDeletesItsStore() async throws {
    let stores = StandInStores()
    let core = FixtureCore(.twoTools)
    let model = AppModel(testing: core)
    let defaults = TestDefaults()
    let windows = AccountWindows(
        model: model,
        environment: WebEnvironment(
            scheme: WebEnvironment.fixtureScheme, stores: stores,
            record: StoreRecord(defaults: defaults, directory: "/test"),
            pages: PageRecord(defaults: defaults, directory: "/test"),
            downloads: FileManager.default.temporaryDirectory, openElsewhere: { _ in },
            configure: { _ in }, pause: { _ in }),
        presence: AppPresence(apply: { _ in }, bringForward: {}, settle: {}),
        scheme: "pitboard-debug")
    await model.refresh()
    await model.noticeOtherChangesForTesting()
    let personal = account("personal", in: windows)
    _ = windows.session(for: personal)
    windows.remember(URL(string: "pitboard-fixture://claude.ai/chat/x"), of: personal)

    _ = try await core.forget("claude/personal")
    await model.noticeOtherChangesForTesting()
    #expect(windows.sessions.isEmpty)
    for _ in 0..<50 where stores.removed.isEmpty { await Task.yield() }
    #expect(stores.removed == [personal.id])
    #expect(
        PageRecord(defaults: defaults, directory: "/test").page(of: personal.id) == nil,
        "its last page goes with it")
}

/// The Dock icon's menu asks for a window from outside any view; the menu bar item opens it.
@MainActor
@Test func aWindowAskedForFromTheDockWaitsForTheMenuBarItem() async {
    let windows = await windows()
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
