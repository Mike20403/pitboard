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
        let tool = app.popUpButtons["sheet.tool"]
        XCTAssertTrue(tool.waitForExistence(timeout: 5))
        tool.click()
        // Its own item, not the menu bar item's Codex heading.
        tool.menuItems["Codex"].click()
        let name = app.textFields["sheet.name"]
        name.click()
        name.typeText("third")
        app.buttons["Sign In"].click()
        XCTAssertTrue(app.accountRow("codex/third").waitForExistence(timeout: 10))
    }

    /// Cancelling a sign-in closes its sheet, stops it and adds nothing. Sign In stays
    /// disabled while a sign-in runs, so another sign-in that can be started and finished
    /// afterwards is what shows the first one was stopped.
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
        XCTAssertTrue(app.sheets.firstMatch.waitForNonExistence(timeout: 5))
        XCTAssertFalse(app.accountRow("claude/third").waitForExistence(timeout: 2))

        app.toolbars.buttons["Add Account"].click()
        let again = app.textFields["sheet.name"]
        XCTAssertTrue(again.waitForExistence(timeout: 5))
        again.click()
        again.typeText("third")
        app.buttons["Sign In"].click()
        let code = app.textFields["sheet.code"]
        XCTAssertTrue(code.waitForExistence(timeout: 10))
        code.click()
        code.typeText("fixture-code")
        app.buttons["Submit Code"].click()
        XCTAssertTrue(app.accountRow("claude/third").waitForExistence(timeout: 10))
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
        XCTAssertTrue(app.alert.buttons["Forget"].waitForExistence(timeout: 5))
        app.alert.buttons["Cancel"].click()
        XCTAssertTrue(row.exists)
        row.rightClick()
        app.menuItems["Forget…"].click()
        app.alert.buttons["Forget"].click()
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
        // Named with its tool, since Claude Code could have a `spare` too.
        spare.buttons["Use spare (Codex)"].click()
        let notice = app.descendants(matching: .any)["notice.switch/codex"]
        XCTAssertTrue(notice.waitForExistence(timeout: 5))
        let restart = "Any codex session started before this switch keeps using main"
        XCTAssertTrue(notice.text("BEGINSWITH", restart).exists)
        notice.buttons["Dismiss"].click()
        XCTAssertTrue(notice.waitForNonExistence(timeout: 5))
    }

    /// With ChatGPT open and running Codex's login, Use asks before quitting it. Cancel
    /// changes nothing; Quit ChatGPT and Switch switches, which the notice about the switch
    /// says. The fixture's ChatGPT is a stand-in: no real app is quit.
    @MainActor
    func testUsingACodexAccountAsksToQuitChatGPTFirst() {
        let app = XCUIApplication.launched(.chatGPTOpen)
        app.openWindow()
        let spare = app.accountRow("codex/spare")
        XCTAssertTrue(spare.waitForExistence(timeout: 5))
        let notice = app.descendants(matching: .any)["notice.switch/codex"]

        spare.buttons["Use spare (Codex)"].click()
        XCTAssertTrue(app.alert.buttons["Cancel"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.alert.staticTexts["Quit ChatGPT to switch?"].exists)
        app.alert.buttons["Cancel"].click()
        XCTAssertTrue(app.alert.waitForNonExistence(timeout: 5))
        XCTAssertFalse(notice.exists, "nothing was switched")

        spare.buttons["Use spare (Codex)"].click()
        XCTAssertTrue(
            app.alert.buttons["Quit ChatGPT and Switch"].waitForExistence(timeout: 5))
        app.alert.buttons["Quit ChatGPT and Switch"].click()
        XCTAssertTrue(notice.waitForExistence(timeout: 5))
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
        app.alert.buttons["Give Up"].click()
        let notice = app.descendants(matching: .any)["notice.abandoned"]
        XCTAssertTrue(notice.waitForExistence(timeout: 5))
        XCTAssertTrue(notice.staticTexts["Gave up on the interrupted switch"].exists)
        XCTAssertTrue(notice.text("CONTAINS", "2 logins kept").exists)
    }

    /// With one account there is nothing to switch to, so the window suggests adding a second,
    /// and Not Now puts the suggestion away.
    @MainActor
    func testOneAccountIsOfferedASecondUntilNotNow() {
        let app = XCUIApplication.launched(.onlyOne)
        app.openWindow()
        let tip = app.staticTexts["Add a second account"]
        XCTAssertTrue(tip.waitForExistence(timeout: 5))
        app.buttons["Not Now"].click()
        XCTAssertTrue(tip.waitForNonExistence(timeout: 5))
        XCTAssertTrue(app.accountRow("claude/work").exists)
    }

    /// Command-N adds an account from any pane, and the window goes to the accounts, which
    /// is what the sheet is about and where the new one is listed.
    @MainActor
    func testCommandNAddsAnAccountFromAnyPane() {
        let app = XCUIApplication.launched(.oneTool)
        app.openWindow()
        app.descendants(matching: .any)["sidebar.activity"].click()
        let activity = app.text("Switch")
        XCTAssertTrue(activity.waitForExistence(timeout: 5))
        app.typeKey("n", modifierFlags: .command)
        XCTAssertTrue(app.textFields["sheet.name"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts["Add Account"].exists)
        app.sheets.firstMatch.buttons["Cancel"].click()
        XCTAssertTrue(app.sheets.firstMatch.waitForNonExistence(timeout: 5))
        XCTAssertTrue(app.accountRow("claude/work").waitForExistence(timeout: 5))
        XCTAssertFalse(activity.exists)
    }

    /// The first launch ever opens the window by itself.
    @MainActor
    func testTheFirstLaunchOpensTheWindow() {
        let app = XCUIApplication.launched(.firstLaunch)
        XCTAssertTrue(app.staticTexts["No Accounts"].waitForExistence(timeout: 10))
    }
}
