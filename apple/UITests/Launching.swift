import XCTest

/// A machine in a known state, as the app's debug build knows it from `PITBOARD_FIXTURE`.
/// Named the same as the app's own `Fixture` cases.
enum Fixture: String {
    case twoTools
    case oneTool
    case empty
    case firstLaunch
    case noClaudeCode
    case unnamed
    case onlyOne
    case readFailure
    case stuck
}

@MainActor
extension XCUIApplication {
    /// The app, started into `fixture`: nothing it does reaches the keychain, the network,
    /// launchd, the login items or an administrator's password.
    static func launched(_ fixture: Fixture) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["PITBOARD_FIXTURE"] = fixture.rawValue
        app.launch()
        return app
    }

    /// Opens the menu bar item's menu.
    func openMenu() {
        let item = statusItems.firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: 10))
        item.click()
    }

    /// Opens the main window from the menu, as a person does.
    func openWindow() {
        openMenu()
        menuItems["Open pitboard"].click()
        XCTAssertTrue(windows.firstMatch.waitForExistence(timeout: 5))
    }

    /// Opens the settings from the menu.
    func openSettings() {
        openMenu()
        menuItems["Settings…"].click()
        XCTAssertTrue(windows["General"].waitForExistence(timeout: 5))
    }

    /// An account's row in the window, by its label with its tool.
    func accountRow(_ qualified: String) -> XCUIElement {
        descendants(matching: .any)["account.\(qualified)"]
    }
}
