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

/// A sheet is over the main window, so putting one up from the menu opens the window too,
/// on the accounts the sheet is about. Opening the window by itself leaves whatever sheet is
/// up, and whichever pane was showing.
@MainActor
@Test func aSheetIsPutUpOverTheWindow() {
    let model = AppModel(testing: FixtureCore(.twoTools))
    #expect(model.windowRequests == 0)

    model.present(.rename(provider: "claude", label: "personal"))
    #expect(model.sheet == .rename(provider: "claude", label: "personal"))
    #expect(model.windowRequests == 1)
    #expect(model.requestedPane == .accounts)

    model.showWindow()
    #expect(model.windowRequests == 2)
    #expect(model.requestedPane == nil)
    #expect(model.sheet == .rename(provider: "claude", label: "personal"))
}

/// A request for the window can ask for a pane: a notice is said on the accounts pane, and
/// the menu's item for one opens the window there, whichever pane it was left on. A request
/// that asks for none leaves the pane as it is, so it does not carry a pane an earlier
/// request asked for.
@MainActor
@Test func aRequestForTheWindowCanAskForAPane() {
    let model = AppModel(testing: FixtureCore(.twoTools))
    #expect(model.requestedPane == nil)

    model.showWindow(.machine)
    #expect(model.requestedPane == .machine)
    #expect(model.windowRequests == 1)
    model.showWindow(.accounts)
    #expect(model.requestedPane == .accounts)
    #expect(model.windowRequests == 2)

    model.showWindow()
    #expect(model.requestedPane == nil)
    model.present(ActionFailure("Couldn’t switch to personal", message: "Nothing is parked."))
    #expect(model.requestedPane == nil, "an alert is over every pane")
    #expect(model.windowRequests == 4)
}

/// The menu stays usable while the window has a sheet up, and its Add Account… or an item that
/// names an account puts up a sheet. Over a running sign-in that replaced the sheet that could
/// finish or stop it, and left the tool waiting on a browser with nothing on screen. The
/// sign-in keeps its sheet, and the window is still brought forward on the accounts pane.
@MainActor
@Test(.timeLimit(.minutes(1)))
func aRunningSignInKeepsItsSheet() async {
    let model = AppModel(testing: FixtureCore(.twoTools))
    await model.refresh()
    model.present(.add(provider: nil))
    let adding = Task { await model.signIn("travel", for: "claude") }
    #expect(await eventually { model.signingIn?.wantsCode == true })

    let requests = model.windowRequests
    model.present(.name(provider: "codex", email: "someone@example.com"))
    model.present(.rename(provider: "claude", label: "personal"))
    #expect(model.sheet == .add(provider: nil))
    #expect(model.windowRequests == requests + 2)
    #expect(model.requestedPane == .accounts)

    model.cancelSignIn()
    #expect(await adding.value == nil)
    model.present(.rename(provider: "claude", label: "personal"))
    #expect(model.sheet == .rename(provider: "claude", label: "personal"), "once it is over")
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

/// Signing in again to the account in use, from its row, keeps it the account in use and says
/// its new login is the one in use now, as a sign-in through the core does, so the UI tests
/// see what somebody on a real machine sees.
@MainActor
@Test(.timeLimit(.minutes(1)))
func signingInAgainToTheAccountInUseSaysItHasANewLogin() async throws {
    let model = AppModel(testing: FixtureCore(.oneTool))
    await model.refresh()
    model.present(.signInAgain(provider: "claude", label: "work"))
    let signing = Task { await model.signIn("work", for: "claude") }
    #expect(await eventually { model.signingIn?.wantsCode == true })

    model.paste("fixture-code")
    #expect(await signing.value == nil)
    #expect(model.sheet == nil)
    #expect(
        model.lastSwitches.first?.said
            == "Signed in to work again. Its new login is the one in use now.")
    let work = try #require(model.status?.accounts.first { $0.qualified == "claude/work" })
    #expect(work.signedIn)
    #expect(!work.switchable)
}
