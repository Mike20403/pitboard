import XCTest

/// An account's window on its site: claude.ai for a Claude Code account, chatgpt.com for a
/// Codex one. In a fixture it loads a stand-in page, so nothing reaches either site.
final class AccountWindowTests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    /// A Claude Code account's window opens titled with its label, on the fixture's
    /// stand-in claude.ai.
    @MainActor
    func testOpeningAClaudeAccountsWindowFromTheMenu() {
        let app = XCUIApplication.launched(.onlyOne)
        app.openMenu()
        app.menuItem("Open claude.ai as work").click()
        let window = app.accountWindow("work")
        XCTAssertTrue(window.waitForExistence(timeout: 10))
        XCTAssertTrue(
            page(of: window).text("==", "claude.ai stand-in").waitForExistence(timeout: 10))
    }

    /// A site with several accounts offers them in a submenu, each site its own tool's.
    @MainActor
    func testEachSiteOffersItsOwnToolsAccounts() {
        let app = XCUIApplication.launched(.twoTools)
        app.openMenu()
        let claude = app.menuItem("Open claude.ai")
        XCTAssertTrue(claude.waitForExistence(timeout: 5))
        claude.hover()
        for label in ["work", "personal", "old"] {
            XCTAssertTrue(claude.menuItems[label].waitForExistence(timeout: 5), label)
        }
        XCTAssertFalse(claude.menuItems["main"].exists)
        let chatGPT = app.menuItem("Open chatgpt.com")
        XCTAssertTrue(chatGPT.exists)
        chatGPT.hover()
        for label in ["main", "spare"] {
            XCTAssertTrue(chatGPT.menuItems[label].waitForExistence(timeout: 5), label)
        }
        XCTAssertFalse(chatGPT.menuItems["work"].exists)
        app.typeKey(.escape, modifierFlags: [])
    }

    /// With no account there is no window to offer.
    @MainActor
    func testNoAccountOffersNoWindow() {
        let app = XCUIApplication.launched(.empty)
        app.openMenu()
        XCTAssertTrue(app.menuItem("Open Pitboard").waitForExistence(timeout: 5))
        XCTAssertFalse(app.menuItem("Open claude.ai").exists)
        XCTAssertFalse(app.menuItem("Open chatgpt.com").exists)
        app.typeKey(.escape, modifierFlags: [])
    }

    /// Opening an account's window again brings the one open forward: one window per account.
    @MainActor
    func testOpeningAWindowAgainBringsTheSameOneForward() {
        let app = openChatGPT()
        app.openMenu()
        let open = app.menuItem("Open chatgpt.com")
        open.hover()
        open.menuItems["main"].click()
        XCTAssertTrue(app.accountWindow("main").waitForExistence(timeout: 5))
        XCTAssertEqual(app.accountWindows.count, 1)
    }

    /// A sign-in link that asks for a new window opens a sign-in window of its own, which
    /// closes itself once it is done, leaving the account's window as it was. WebKit asks
    /// about such a link before it asks for the window, and refusing it there would open none.
    @MainActor
    func testASignInLinkOpensASignInWindowThatClosesItself() {
        let app = openChatGPT()
        let window = app.accountWindow("main")
        page(of: window).links["Continue with appleid.apple.com"].click()
        let signIn = app.signInWindow
        XCTAssertTrue(signIn.waitForExistence(timeout: 10))
        let done = signIn.webViews.firstMatch.buttons["Done"]
        XCTAssertTrue(done.waitForExistence(timeout: 10))
        done.click()
        XCTAssertTrue(signIn.waitForNonExistence(timeout: 10))
        XCTAssertTrue(page(of: window).text("==", "chatgpt.com stand-in").exists)
    }

    /// A page's script opening a window for the sign-in opens a sign-in window too.
    @MainActor
    func testASignInScriptOpensASignInWindow() {
        let app = openChatGPT()
        page(of: app.accountWindow("main")).buttons["Sign in in a window"].click()
        XCTAssertTrue(app.signInWindow.waitForExistence(timeout: 10))
        let done = app.signInWindow.webViews.firstMatch.buttons["Done"]
        XCTAssertTrue(done.waitForExistence(timeout: 10))
        done.click()
        XCTAssertTrue(app.signInWindow.waitForNonExistence(timeout: 10))
    }

    /// A sign-in window never outlives the account's window that opened it.
    @MainActor
    func testClosingTheAccountsWindowClosesItsSignInWindow() {
        let app = openChatGPT()
        let window = app.accountWindow("main")
        page(of: window).links["Continue with appleid.apple.com"].click()
        XCTAssertTrue(app.signInWindow.waitForExistence(timeout: 10))
        window.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(window.waitForNonExistence(timeout: 10))
        XCTAssertTrue(app.signInWindow.waitForNonExistence(timeout: 10))
    }

    /// Google's sign-in is stopped in the window, which says so and stays on its page.
    @MainActor
    func testGoogleSignInIsStoppedInTheWindow() {
        let app = openChatGPT()
        let window = app.accountWindow("main")
        page(of: window).links["Continue with Google"].click()
        let note = window.descendants(matching: .any)["window.note"]
        XCTAssertTrue(note.text("CONTAINS", "Pitboard stopped it").waitForExistence(timeout: 5))
        XCTAssertTrue(page(of: window).text("==", "chatgpt.com stand-in").exists)
    }

    /// A file the site's own page downloads is saved without asking and listed in the
    /// window's downloads.
    @MainActor
    func testASiteDownloadIsListedInTheWindow() {
        let app = openChatGPT()
        let window = app.accountWindow("main")
        page(of: window).links["Download notes"].click()
        let downloads = window.toolbars.buttons["Downloads"]
        XCTAssertTrue(downloads.waitForExistence(timeout: 10))
        downloads.click()
        // A row is one element for VoiceOver, its name and state read together.
        let row = app.descendants(matching: .any)["download"]
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        let done = NSPredicate(
            format: "label CONTAINS 'notes.txt' AND label CONTAINS 'Downloaded'")
        expectation(for: done, evaluatedWith: row)
        waitForExpectations(timeout: 10)
    }

    /// Edit > Find shows the system's find bar above the page.
    @MainActor
    func testFindShowsTheFindBar() {
        let app = openChatGPT()
        let window = app.accountWindow("main")
        page(of: window).click()
        app.typeKey("f", modifierFlags: .command)
        XCTAssertTrue(window.searchFields.firstMatch.waitForExistence(timeout: 5))
    }

    /// Removing a window's data asks first, and the window starts again at its site's home
    /// saying how to sign in.
    @MainActor
    func testRemovingWebsiteDataAsksFirstAndStartsOver() {
        let app = openChatGPT()
        let window = app.accountWindow("main")
        let note = window.descendants(matching: .any)["window.note"]
        // The first opening says how to sign in; put it away, and leave the home page.
        note.buttons["Dismiss"].click()
        XCTAssertTrue(note.waitForNonExistence(timeout: 5))
        page(of: window).links["A chat"].click()
        XCTAssertTrue(
            page(of: window).text("BEGINSWITH", "A Pitboard fixture page at /chat/fixture")
                .waitForExistence(timeout: 10))

        let file = app.menuBars.menuBarItems["File"]
        file.click()
        file.menuItems["Remove Website Data…"].click()
        let remove = window.sheets.firstMatch.buttons["Remove"]
        XCTAssertTrue(remove.waitForExistence(timeout: 5))
        remove.click()
        XCTAssertTrue(
            note.text("BEGINSWITH", "Sign in to chatgpt.com").waitForExistence(timeout: 10))
        XCTAssertTrue(
            page(of: window).text("BEGINSWITH", "A Pitboard fixture page at /.")
                .waitForExistence(timeout: 10))
    }

    /// An account's shortcut menu in the window opens its window on its site.
    @MainActor
    func testAnAccountsShortcutMenuOpensItsWindow() {
        let app = XCUIApplication.launched(.twoTools)
        app.openWindow()
        let row = app.accountRow("codex/spare")
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        row.rightClick()
        // The File menu has an Open chatgpt.com submenu too; the shortcut menu is the one
        // with the account's own items.
        let menu = app.menus.containing(
            NSPredicate(format: "title == %@", "Copy Email Address")
        )
        .firstMatch
        menu.menuItems["Open chatgpt.com"].click()
        XCTAssertTrue(app.accountWindow("spare").waitForExistence(timeout: 10))
    }

    /// The fixture with both tools, and `main`'s window open on the stand-in chatgpt.com.
    @MainActor
    private func openChatGPT() -> XCUIApplication {
        let app = XCUIApplication.launched(.twoTools)
        app.openMenu()
        let open = app.menuItem("Open chatgpt.com")
        XCTAssertTrue(open.waitForExistence(timeout: 5))
        open.hover()
        open.menuItems["main"].click()
        let window = app.accountWindow("main")
        XCTAssertTrue(window.waitForExistence(timeout: 10))
        XCTAssertTrue(
            page(of: window).text("==", "chatgpt.com stand-in").waitForExistence(timeout: 10))
        return app
    }

    @MainActor
    private func page(of window: XCUIElement) -> XCUIElement {
        window.webViews.firstMatch
    }
}
