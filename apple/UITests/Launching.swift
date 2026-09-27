import XCTest

extension XCUIApplication {
    /// The debug app, started in the fixture called `fixture`. It reads the name from
    /// `PITBOARD_FIXTURE` and runs against stand-ins, so nothing a test does reaches the
    /// keychain, the network or the accounts of whoever runs it.
    @MainActor
    static func launched(_ fixture: String) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["PITBOARD_FIXTURE"] = fixture
        app.launch()
        return app
    }
}
