import XCTest

/// The menu the menu bar item opens: the glance and the switch.
final class MenuBarTests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    /// Every account is an item under its tool, and the commands follow them.
    @MainActor
    func testTheMenuListsEveryAccountUnderItsTool() {
        let app = XCUIApplication.launched(.twoTools)
        app.openMenu()
        for title in ["Claude Code", "Codex", "work", "personal", "old", "main", "spare"] {
            XCTAssertTrue(app.menuItem(title).waitForExistence(timeout: 5), title)
        }
        for command in [
            "Add Account…", "Refresh", "Open pitboard", "Settings…", "Quit pitboard",
        ] {
            XCTAssertTrue(app.menuItem(command).exists, command)
        }
    }

    /// Choosing another account switches to it, and the window then shows it in use.
    @MainActor
    func testChoosingAnotherAccountSwitchesToIt() {
        let app = XCUIApplication.launched(.oneTool)
        app.openMenu()
        app.menuItem("personal").click()
        app.openMenu()
        app.menuItem("Open pitboard").click()
        let row = app.accountRow("claude/personal")
        XCTAssertTrue(row.waitForExistence(timeout: 5))
        XCTAssertTrue(row.staticTexts["In Use"].waitForExistence(timeout: 5))
    }

    /// An account whose parked login expired opens the sheet that signs in to it again.
    @MainActor
    func testAnExpiredAccountOpensSigningInAgain() {
        let app = XCUIApplication.launched(.twoTools)
        app.openMenu()
        app.menuItem("old").click()
        XCTAssertTrue(app.staticTexts["Sign In to old Again"].waitForExistence(timeout: 5))
    }

    /// Add Account opens the window with its sheet over it.
    @MainActor
    func testAddAccountOpensTheSheet() {
        let app = XCUIApplication.launched(.oneTool)
        app.openMenu()
        app.menuItem("Add Account…").click()
        XCTAssertTrue(app.staticTexts["Add Account"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.textFields["sheet.name"].exists)
    }

    /// Settings opens where macOS users look for it.
    @MainActor
    func testSettingsOpens() {
        let app = XCUIApplication.launched(.oneTool)
        app.openMenu()
        app.menuItem("Settings…").click()
        XCTAssertTrue(app.windows["General"].waitForExistence(timeout: 5))
    }

    /// A machine without Claude Code says so first, with where to learn how to install it.
    @MainActor
    func testAMachineWithoutClaudeCodeSaysSo() {
        let app = XCUIApplication.launched(.noClaudeCode)
        app.openMenu()
        XCTAssertTrue(app.menuItem("Claude Code isn’t installed").waitForExistence(timeout: 5))
    }

    /// A read that failed is one item in the menu, and choosing it opens the window, where
    /// the notice says why above the last numbers measured.
    @MainActor
    func testAFailedReadIsSaidInTheMenuAndInTheWindow() {
        let app = XCUIApplication.launched(.readFailure)
        app.openMenu()
        let item = app.menuItem("Couldn’t read usage")
        XCTAssertTrue(item.waitForExistence(timeout: 5))
        item.click()
        let notice = app.descendants(matching: .any)["notice.read"]
        XCTAssertTrue(notice.waitForExistence(timeout: 5))
        XCTAssertTrue(notice.staticTexts["Couldn’t read usage"].exists)
        XCTAssertTrue(notice.text("BEGINSWITH", "Anthropic could not be reached").exists)
        XCTAssertTrue(app.accountRow("claude/work").exists)
    }

    /// The menu's item for something to look at opens the window on the accounts, where it
    /// is said, whichever pane the window was left on.
    @MainActor
    func testShowingANoticeOpensTheWindowOnTheAccounts() {
        let app = XCUIApplication.launched(.stuck)
        app.openWindow()
        app.descendants(matching: .any)["sidebar.machine"].click()
        let checks = app.staticTexts["Keychain"]
        XCTAssertTrue(checks.waitForExistence(timeout: 5))

        app.openMenu()
        let item = app.menuItem("An interrupted switch is waiting")
        XCTAssertTrue(item.waitForExistence(timeout: 5))
        item.click()
        XCTAssertTrue(app.buttons["Give Up…"].waitForExistence(timeout: 5))
        XCTAssertTrue(checks.waitForNonExistence(timeout: 5))
    }

    /// Quit quits.
    @MainActor
    func testQuitQuits() {
        let app = XCUIApplication.launched(.oneTool)
        app.openMenu()
        app.menuItem("Quit pitboard").click()
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 5))
    }
}
