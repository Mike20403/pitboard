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
            XCTAssertTrue(app.menuItems[title].waitForExistence(timeout: 5), title)
        }
        for command in [
            "Add Account…", "Refresh", "Open pitboard", "Settings…", "Quit pitboard",
        ] {
            XCTAssertTrue(app.menuItems[command].exists, command)
        }
    }

    /// Choosing another account switches to it, and the window then shows it in use.
    @MainActor
    func testChoosingAnotherAccountSwitchesToIt() {
        let app = XCUIApplication.launched(.oneTool)
        app.openMenu()
        app.menuItems["personal"].click()
        app.openMenu()
        app.menuItems["Open pitboard"].click()
        let row = app.accountRow("claude/personal")
        XCTAssertTrue(row.waitForExistence(timeout: 5))
        XCTAssertTrue(row.staticTexts["In Use"].waitForExistence(timeout: 5))
    }

    /// An account whose parked login expired opens the sheet that signs in to it again.
    @MainActor
    func testAnExpiredAccountOpensSigningInAgain() {
        let app = XCUIApplication.launched(.twoTools)
        app.openMenu()
        app.menuItems["old"].click()
        XCTAssertTrue(app.staticTexts["Sign In to old Again"].waitForExistence(timeout: 5))
    }

    /// Add Account opens the window with its sheet over it.
    @MainActor
    func testAddAccountOpensTheSheet() {
        let app = XCUIApplication.launched(.oneTool)
        app.openMenu()
        app.menuItems["Add Account…"].click()
        XCTAssertTrue(app.staticTexts["Add Account"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.textFields["sheet.name"].exists)
    }

    /// Settings opens where macOS users look for it.
    @MainActor
    func testSettingsOpens() {
        let app = XCUIApplication.launched(.oneTool)
        app.openMenu()
        app.menuItems["Settings…"].click()
        XCTAssertTrue(app.windows["General"].waitForExistence(timeout: 5))
    }

    /// A machine without Claude Code says so first, with where to learn how to install it.
    @MainActor
    func testAMachineWithoutClaudeCodeSaysSo() {
        let app = XCUIApplication.launched(.noClaudeCode)
        app.openMenu()
        XCTAssertTrue(app.menuItems["Claude Code isn’t installed"].waitForExistence(timeout: 5))
    }

    /// Quit quits.
    @MainActor
    func testQuitQuits() {
        let app = XCUIApplication.launched(.oneTool)
        app.openMenu()
        app.menuItems["Quit pitboard"].click()
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 5))
    }
}
