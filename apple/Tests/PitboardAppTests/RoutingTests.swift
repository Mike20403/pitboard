import Foundation
import PitboardKit
import Testing

@testable import PitboardApp

/// Asks again every few milliseconds, for as long as a test can reasonably wait, for
/// something that happens on another thread.
@MainActor
private func eventually(_ condition: @MainActor () -> Bool) async -> Bool {
    for _ in 0..<500 {
        if condition() { return true }
        try? await Task.sleep(for: .milliseconds(10))
    }
    return condition()
}

/// A sheet is over the main window, so putting one up from the menu opens the window too.
/// Opening the window by itself leaves whatever sheet is up.
@MainActor
@Test func aSheetIsPutUpOverTheWindow() {
    let model = AppModel(testing: FixtureCore(.twoTools))
    #expect(model.windowRequests == 0)

    model.present(.rename(provider: "claude", label: "personal"))
    #expect(model.sheet == .rename(provider: "claude", label: "personal"))
    #expect(model.windowRequests == 1)

    model.showWindow()
    #expect(model.windowRequests == 2)
    #expect(model.sheet == .rename(provider: "claude", label: "personal"))
}

/// A failure of something asked for away from the window is said in the window, which is
/// opened for it. Nothing that went wrong opens nothing.
@MainActor
@Test func aFailureAwayFromTheWindowIsSaidInIt() {
    let model = AppModel(testing: FixtureCore(.twoTools))
    model.present(nil as ActionFailure?)
    #expect(model.presentedFailure == nil)
    #expect(model.windowRequests == 0)

    let failure = ActionFailure("Couldn’t switch to personal", message: "Nothing is parked.")
    model.present(failure)
    #expect(model.presentedFailure == failure)
    #expect(model.windowRequests == 1)
}

/// A switch asked for from the menu or a notification that fails is said in the window,
/// since neither has anywhere to put a sentence. One that works opens nothing: the menu bar
/// already shows it.
@MainActor
@Test func onlyASwitchAskedForAwayFromTheWindowThatFailsOpensIt() async throws {
    let model = AppModel(testing: FixtureCore(.twoTools))
    await model.refresh()

    await model.switchAsked(to: "claude/personal")
    #expect(model.presentedFailure == nil)
    #expect(model.windowRequests == 0)
    #expect(
        model.status?.accounts.filter(\.signedIn).compactMap(\.qualified) == [
            "claude/personal", "codex/main",
        ])

    await model.switchAsked(to: "claude/nobody")
    let failure = try #require(model.presentedFailure)
    #expect(failure.title == "Couldn’t switch to nobody")
    #expect(failure.message == "There is no account called claude/nobody.")
    #expect(model.windowRequests == 1)
}

/// Adding an account from start to finish, as a UI test drives it: the sheet over the
/// window, the tool's sign-in with the code it asks for typed back, and the new account
/// parked beside the one in use once the sheet has closed by itself.
@MainActor
@Test(.timeLimit(.minutes(1)))
func anAccountIsAddedThroughTheSheetFromStartToFinish() async throws {
    let model = AppModel(testing: FixtureCore(.twoTools))
    await model.refresh()
    model.present(.add(provider: nil))
    #expect(model.windowRequests == 1)
    #expect(model.provider(for: .add(provider: nil)) == "claude")

    let adding = Task { await model.signIn("travel", for: "claude") }
    #expect(await eventually { model.signingIn?.wantsCode == true })
    #expect(model.signingIn?.url == URL(string: "https://claude.ai/oauth/authorize?fixture=1"))
    #expect(model.sheet == .add(provider: nil))

    model.paste("fixture-code")
    #expect(await adding.value == nil)
    #expect(model.signingIn == nil)
    #expect(model.sheet == nil)
    let travel = try #require(model.status?.accounts.first { $0.qualified == "claude/travel" })
    #expect(travel.email == "travel@example.com")
    #expect(!travel.signedIn)
    #expect(travel.switchable)
    #expect(model.warnings.isEmpty)
}
