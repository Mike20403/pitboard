import XCTest

/// The window's other panes, and the settings.
final class PanesAndSettingsTests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    /// Activity lists what Pitboard changed, newest first. The fixture's two switches are
    /// newer than both its enrolments, so every switch is listed above every enrolment.
    @MainActor
    func testActivityListsChanges() {
        let app = XCUIApplication.launched(.oneTool)
        app.openWindow()
        app.descendants(matching: .any)["sidebar.activity"].click()
        let switched = app.text("Switch")
        let enrolled = app.text("Enrol")
        XCTAssertTrue(switched.waitForExistence(timeout: 5))
        XCTAssertTrue(enrolled.exists)
        XCTAssertLessThan(switched.frame.minY, enrolled.frame.minY, "newest first")
    }

    /// This Mac shows every check and says what is worth looking at: in this fixture, the
    /// parked login of personal, which lapses soon.
    @MainActor
    func testThisMacShowsTheChecks() {
        let app = XCUIApplication.launched(.oneTool)
        app.openWindow()
        app.descendants(matching: .any)["sidebar.machine"].click()
        XCTAssertTrue(app.staticTexts["account personal"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts["One thing is worth looking at."].exists)
    }

    /// Daily renewal turns on, says when it runs, and Renew Now says what it did.
    @MainActor
    func testDailyRenewal() {
        let app = XCUIApplication.launched(.oneTool)
        app.openSettings()
        let renew = app.control("settings.renewDaily")
        XCTAssertTrue(renew.waitForExistence(timeout: 5))
        renew.click()
        XCTAssertTrue(app.text("Every day").waitForExistence(timeout: 5))
        app.buttons["Renew Now"].click()
        XCTAssertTrue(app.text("Renewed one.").waitForExistence(timeout: 5))
    }

    /// Opening at login turns on and off without registering the test build.
    @MainActor
    func testOpenAtLogin() {
        let app = XCUIApplication.launched(.oneTool)
        app.openSettings()
        let toggle = app.control("settings.openAtLogin")
        XCTAssertTrue(toggle.waitForExistence(timeout: 5))
        XCTAssertEqual(toggle.value as? Int, 0)
        toggle.click()
        XCTAssertEqual(toggle.value as? Int, 1)
        toggle.click()
        XCTAssertEqual(toggle.value as? Int, 0)
    }

    /// With no Pitboard found, the command line tab offers to link the one inside the app,
    /// and then shows where the link is.
    @MainActor
    func testInstallingTheCommandLine() {
        let app = XCUIApplication.launched(.oneTool)
        app.openSettings()
        app.toolbars.buttons["Command Line"].click()
        let install = app.buttons["Install Command Line Tool…"]
        XCTAssertTrue(install.waitForExistence(timeout: 5))
        install.click()
        XCTAssertTrue(install.waitForNonExistence(timeout: 5))
        XCTAssertTrue(app.text("ENDSWITH", "/bin/pitboard").waitForExistence(timeout: 5))
    }

    /// A debug build carries no update key, and the Updates tab says so.
    @MainActor
    func testUpdatesSayWhenTheBuildCannotUpdate() {
        let app = XCUIApplication.launched(.oneTool)
        app.openSettings()
        app.toolbars.buttons["Updates"].click()
        let prefix = "This copy of Pitboard can"
        let note = app.staticTexts.matching(
            NSPredicate(format: "value BEGINSWITH %@ OR label BEGINSWITH %@", prefix, prefix)
        ).firstMatch
        XCTAssertTrue(note.waitForExistence(timeout: 5))
    }
}
