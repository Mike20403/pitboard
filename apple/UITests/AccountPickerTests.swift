import XCTest

/// A page shared from a browser, as the Share extension hands it over: a pitboard link of the
/// debug build, which opens the account picker and nothing else until somebody chooses.
final class AccountPickerTests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    /// A shared chatgpt.com link asks which Codex account opens it, and opens nothing until
    /// one is chosen: then that account's window opens on the link.
    @MainActor
    func testASharedLinkAsksWhichAccountOpensIt() {
        let app = XCUIApplication.launched(.twoTools)
        app.share("https://chatgpt.com/c/shared")
        let picker = app.picker
        XCTAssertTrue(picker.waitForExistence(timeout: 10))
        XCTAssertTrue(
            picker.text("==", "Open this chatgpt.com link as:").waitForExistence(timeout: 10))
        // Each row is one element for VoiceOver, its label and email read together.
        let spare = picker.descendants(matching: .any)["picker.account.spare"]
        XCTAssertTrue(spare.waitForExistence(timeout: 5))
        XCTAssertTrue(picker.descendants(matching: .any)["picker.account.main"].exists)
        XCTAssertFalse(
            picker.descendants(matching: .any)["picker.account.work"].exists,
            "only the site's own accounts")
        XCTAssertEqual(app.accountWindows.count, 0, "nothing opens before a choice")

        spare.click()
        // Open waits a moment after the accounts appear, so a stray Return opens nothing.
        let open = picker.buttons["Open"]
        XCTAssertTrue(open.wait(for: \.isEnabled, toEqual: true, timeout: 5))
        open.click()
        let window = app.accountWindow("spare")
        XCTAssertTrue(window.waitForExistence(timeout: 10))
        XCTAssertTrue(
            window.webViews.firstMatch.text("CONTAINS", "/c/shared").waitForExistence(
                timeout: 10))
        XCTAssertTrue(picker.waitForNonExistence(timeout: 5))
    }

    /// A link of a site pitboard does not open is refused in the picker, saying why.
    @MainActor
    func testALinkToAnotherSiteIsRefused() {
        let app = XCUIApplication.launched(.twoTools)
        app.share("https://example.com/page")
        let picker = app.picker
        XCTAssertTrue(picker.waitForExistence(timeout: 10))
        XCTAssertTrue(picker.text("==", "Can’t Open This Link").waitForExistence(timeout: 10))
        XCTAssertTrue(picker.text("CONTAINS", "This link is on example.com.").exists)
        picker.buttons["OK"].click()
        XCTAssertTrue(picker.waitForNonExistence(timeout: 5))
        XCTAssertEqual(app.accountWindows.count, 0)
    }

    /// Cancelling closes the picker and opens nothing.
    @MainActor
    func testCancellingOpensNothing() {
        let app = XCUIApplication.launched(.onlyOne)
        app.share("https://claude.ai/new")
        let picker = app.picker
        XCTAssertTrue(
            picker.text("==", "Open this claude.ai link as:").waitForExistence(timeout: 10))
        picker.buttons["Cancel"].click()
        XCTAssertTrue(picker.waitForNonExistence(timeout: 5))
        XCTAssertEqual(app.accountWindows.count, 0)
    }

    /// A link for a site no account opens offers to add one.
    @MainActor
    func testALinkForASiteWithNoAccountOffersToAddOne() {
        let app = XCUIApplication.launched(.onlyOne)
        app.share("https://chatgpt.com/")
        let picker = app.picker
        XCTAssertTrue(picker.text("==", "No chatgpt.com Account").waitForExistence(timeout: 10))
        XCTAssertTrue(picker.buttons["Add Account…"].exists)
        XCTAssertTrue(picker.buttons["Open in Browser"].exists)
        picker.buttons["Cancel"].click()
    }

    /// Someone else's sign-in link is refused: it would sign the window in as them.
    @MainActor
    func testASignInLinkIsRefused() {
        let app = XCUIApplication.launched(.onlyOne)
        app.share("https://claude.ai/magic-link#someone")
        let picker = app.picker
        XCTAssertTrue(picker.text("==", "Can’t Open This Link").waitForExistence(timeout: 10))
        XCTAssertTrue(
            picker.text("BEGINSWITH", "pitboard doesn’t open claude.ai sign-in links").exists)
        picker.buttons["OK"].click()
    }
}
