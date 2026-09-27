import XCTest

/// The app has no Dock icon and no window at launch, so the item in the menu bar is the whole
/// of what somebody finds when it starts.
final class MenuBarTests: XCTestCase {
    @MainActor
    func testTheMenuBarItemOffersAWayToQuit() {
        let app = XCUIApplication.launched("twoTools")
        let item = app.statusItems.firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: 10))
        item.click()
        XCTAssertTrue(app.menuItems["Quit pitboard"].waitForExistence(timeout: 5))
    }
}
