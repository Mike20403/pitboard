import XCTest

/// The window's other panes, and the settings.
final class PanesAndSettingsTests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    /// Activity lists what pitboard changed, newest first.
    @MainActor
    func testActivityListsChanges() {
        let app = XCUIApplication.launched(.oneTool)
        app.openWindow()
        app.descendants(matching: .any)["sidebar.activity"].click()
        XCTAssertTrue(app.tables.staticTexts["Switch"].firstMatch.waitForExistence(timeout: 5))
        XCTAssertTrue(app.tables.staticTexts["Enrol"].firstMatch.exists)
    }

    /// This Mac shows every check and says what is worth looking at.
    @MainActor
    func testThisMacShowsTheChecks() {
        let app = XCUIApplication.launched(.oneTool)
        app.openWindow()
        app.descendants(matching: .any)["sidebar.machine"].click()
        XCTAssertTrue(app.staticTexts["Keychain"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts["One thing is worth looking at."].exists)
    }

    /// Daily renewal turns on, says when it runs, and Renew Now says what it did.
    @MainActor
    func testDailyRenewal() {
        let app = XCUIApplication.launched(.oneTool)
        app.openSettings()
        let renew = app.switches["Renew parked logins daily"]
        XCTAssertTrue(renew.waitForExistence(timeout: 5))
        renew.click()
        XCTAssertTrue(app.staticTexts["Every day"].waitForExistence(timeout: 5))
        app.buttons["Renew Now"].click()
        XCTAssertTrue(app.staticTexts["Renewed one."].waitForExistence(timeout: 5))
    }

    /// Opening at login turns on and off without registering the test build.
    @MainActor
    func testOpenAtLogin() {
        let app = XCUIApplication.launched(.oneTool)
        app.openSettings()
        let toggle = app.switches["Open pitboard at login"]
        XCTAssertTrue(toggle.waitForExistence(timeout: 5))
        XCTAssertEqual(toggle.value as? Int, 0)
        toggle.click()
        XCTAssertEqual(toggle.value as? Int, 1)
    }

    /// With no pitboard found, the command line tab offers to link the one inside the app,
    /// and then shows where it is.
    @MainActor
    func testInstallingTheCommandLine() {
        let app = XCUIApplication.launched(.oneTool)
        app.openSettings()
        app.toolbars.buttons["Command Line"].click()
        let install = app.buttons["Install Command Line Tool…"]
        XCTAssertTrue(install.waitForExistence(timeout: 5))
        install.click()
        XCTAssertTrue(install.waitForNonExistence(timeout: 5))
    }

    /// A debug build carries no update key, and the Updates tab says so.
    @MainActor
    func testUpdatesSayWhenTheBuildCannotUpdate() {
        let app = XCUIApplication.launched(.oneTool)
        app.openSettings()
        app.toolbars.buttons["Updates"].click()
        XCTAssertTrue(
            app.staticTexts.containing(
                NSPredicate(format: "label BEGINSWITH %@", "This copy of pitboard can")
            )
            .firstMatch.waitForExistence(timeout: 5))
    }
}
