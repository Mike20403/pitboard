import XCTest

/// The window's accounts: adding, signing in again, naming, renaming, forgetting, switching.
final class AccountsWindowTests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    /// A new Claude Code account: its sign-in asks for the code from the browser, and once it
    /// is pasted the account is listed and the sheet closes.
    @MainActor
    func testAddingAClaudeCodeAccount() {
        let app = XCUIApplication.launched(.oneTool)
        app.openWindow()
        app.toolbars.buttons["Add Account"].click()
        let name = app.textFields["sheet.name"]
        XCTAssertTrue(name.waitForExistence(timeout: 5))
        name.click()
        name.typeText("third")
        app.buttons["Sign In"].click()
        let code = app.textFields["sheet.code"]
        XCTAssertTrue(code.waitForExistence(timeout: 10))
        XCTAssertTrue(app.links["Open Sign-In Page"].exists)
        code.click()
        code.typeText("fixture-code")
        app.buttons["Submit Code"].click()
        XCTAssertTrue(app.accountRow("claude/third").waitForExistence(timeout: 10))
        XCTAssertFalse(app.sheets.firstMatch.exists)
    }

    /// A Codex sign-in finishes by itself once the browser is done.
    @MainActor
    func testAddingACodexAccount() {
        let app = XCUIApplication.launched(.twoTools)
        app.openWindow()
        app.toolbars.buttons["Add Account"].click()
        let tool = app.popUpButtons["Tool"]
        XCTAssertTrue(tool.waitForExistence(timeout: 5))
        tool.click()
        app.menuItems["Codex"].click()
        let name = app.textFields["sheet.name"]
        name.click()
        name.typeText("third")
        app.buttons["Sign In"].click()
        XCTAssertTrue(app.accountRow("codex/third").waitForExistence(timeout: 10))
    }

    /// Cancelling a sign-in stops it and adds nothing.
    @MainActor
    func testCancellingASignInAddsNothing() {
        let app = XCUIApplication.launched(.oneTool)
        app.openWindow()
        app.toolbars.buttons["Add Account"].click()
        let name = app.textFields["sheet.name"]
        XCTAssertTrue(name.waitForExistence(timeout: 5))
        name.click()
        name.typeText("third")
        app.buttons["Sign In"].click()
        XCTAssertTrue(app.textFields["sheet.code"].waitForExistence(timeout: 10))
        app.sheets.firstMatch.buttons["Cancel"].click()
        XCTAssertFalse(app.accountRow("claude/third").waitForExistence(timeout: 2))
    }

    /// A rename to a name another account has is refused in the sheet, with the name still
    /// there to correct; a free name is taken.
    @MainActor
    func testRenamingAnAccount() {
        let app = XCUIApplication.launched(.oneTool)
        app.openWindow()
        let row = app.accountRow("claude/personal")
        XCTAssertTrue(row.waitForExistence(timeout: 5))
        row.rightClick()
        app.menuItems["Rename…"].click()
        let name = app.textFields["sheet.name"]
        XCTAssertTrue(name.waitForExistence(timeout: 5))
        name.click()
        name.typeKey("a", modifierFlags: .command)
        name.typeText("work")
        app.buttons["Rename"].click()
        XCTAssertTrue(app.staticTexts["Couldn’t rename personal"].waitForExistence(timeout: 5))
        name.typeKey("a", modifierFlags: .command)
        name.typeText("home")
        app.buttons["Rename"].click()
        XCTAssertTrue(app.accountRow("claude/home").waitForExistence(timeout: 5))
    }

    /// Forgetting asks first, and Cancel keeps the account.
    @MainActor
    func testForgettingAsksFirst() {
        let app = XCUIApplication.launched(.oneTool)
        app.openWindow()
        let row = app.accountRow("claude/personal")
        XCTAssertTrue(row.waitForExistence(timeout: 5))
        row.rightClick()
        app.menuItems["Forget…"].click()
        XCTAssertTrue(app.buttons["Forget"].waitForExistence(timeout: 5))
        app.buttons["Cancel"].click()
        XCTAssertTrue(row.exists)
        row.rightClick()
        app.menuItems["Forget…"].click()
        app.buttons["Forget"].click()
        XCTAssertTrue(row.waitForNonExistence(timeout: 5))
    }

    /// The account in use cannot be forgotten, so its menu does not offer it.
    @MainActor
    func testTheAccountInUseOffersNoForget() {
        let app = XCUIApplication.launched(.oneTool)
        app.openWindow()
        let row = app.accountRow("claude/work")
        XCTAssertTrue(row.waitForExistence(timeout: 5))
        row.rightClick()
        XCTAssertTrue(app.menuItems["Rename…"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.menuItems["Forget…"].exists)
        app.typeKey(.escape, modifierFlags: [])
    }

    /// Use switches from the window, and a Codex switch says running sessions keep the old
    /// account until they are restarted, until it is dismissed.
    @MainActor
    func testUsingACodexAccountSaysSessionsKeepTheOldOne() {
        let app = XCUIApplication.launched(.twoTools)
        app.openWindow()
        let spare = app.accountRow("codex/spare")
        XCTAssertTrue(spare.waitForExistence(timeout: 5))
        spare.buttons["Use spare"].click()
        let notice = app.descendants(matching: .any)["notice.switch/codex"]
        XCTAssertTrue(notice.waitForExistence(timeout: 5))
        notice.buttons["Dismiss"].click()
        XCTAssertTrue(notice.waitForNonExistence(timeout: 5))
    }

    /// Naming the login in use, from the tip above it.
    @MainActor
    func testNamingTheAccountInUse() {
        let app = XCUIApplication.launched(.unnamed)
        app.openWindow()
        XCTAssertTrue(app.staticTexts["Give this account a name"].waitForExistence(timeout: 5))
        app.buttons["Name…"].firstMatch.click()
        let name = app.textFields["sheet.name"]
        XCTAssertTrue(name.waitForExistence(timeout: 5))
        name.click()
        name.typeText("work")
        app.buttons["Save"].click()
        XCTAssertTrue(app.accountRow("claude/work").waitForExistence(timeout: 5))
    }

    /// A machine nobody is signed in on offers adding an account.
    @MainActor
    func testAnEmptyMachineOffersAnAccount() {
        let app = XCUIApplication.launched(.empty)
        app.openWindow()
        XCTAssertTrue(app.staticTexts["No Accounts"].waitForExistence(timeout: 5))
        app.buttons["Add Account…"].click()
        XCTAssertTrue(app.textFields["sheet.name"].waitForExistence(timeout: 5))
    }

    /// An interrupted switch can be given up on, after asking, and says what it kept.
    @MainActor
    func testGivingUpOnAnInterruptedSwitch() {
        let app = XCUIApplication.launched(.stuck)
        app.openWindow()
        let giveUp = app.buttons["Give Up…"]
        XCTAssertTrue(giveUp.waitForExistence(timeout: 5))
        giveUp.click()
        app.buttons["Give Up"].click()
        XCTAssertTrue(
            app.staticTexts["Gave up on the interrupted switch"].waitForExistence(timeout: 5))
    }

    /// The first launch ever opens the window by itself.
    @MainActor
    func testTheFirstLaunchOpensTheWindow() {
        let app = XCUIApplication.launched(.firstLaunch)
        XCTAssertTrue(app.staticTexts["No Accounts"].waitForExistence(timeout: 10))
    }
}
