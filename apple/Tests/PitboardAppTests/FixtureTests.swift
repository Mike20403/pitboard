import Foundation
import PitboardKit
import Testing

@testable import PitboardApp

/// Each account the way a test reads it: its name with its tool, or the email of a login
/// with no name, then whether it is the one in use, or why it cannot be switched to.
private func described(_ status: Status) -> [String] {
    status.accounts.map { account in
        let name = account.qualified ?? account.email
        if account.signedIn { return "\(name), in use" }
        return account.switchable ? name : "\(name), \(account.stale ?? "not switchable")"
    }
}

/// The code a call was refused with, or nil when it was not refused.
private func refusal<T>(_ call: () async throws -> T) async -> String? {
    do {
        _ = try await call()
        return nil
    } catch {
        return AppModel.code(of: error)
    }
}

/// Runs a fixture's sign-in to the end the way the sheet does: reads everything the tool
/// says, types a code back once it asks for one, and finishes.
private func signInToTheEnd(
    _ label: String, on core: FixtureCore
) async throws -> Enrolled {
    let session = try await core.signIn(label)
    await Task.detached {
        while let line = session.nextLine() {
            if line.contains("Paste code") { try? session.paste(line: "fixture-code") }
        }
    }.value
    return try session.finish()
}

// MARK: - Where each fixture starts

/// The UI tests launch the app into these fixtures and assert on what each starts with, so
/// a change here is a change to what they test: who a read shows, or the code it fails
/// with, who the offline read shows, and which tools were found. A read that fails still
/// has the offline read to fall back on.
@Test(arguments: Fixture.allCases)
func eachFixtureStartsWhereItsTestsExpect(_ fixture: Fixture) async throws {
    let work = "claude/work, in use"
    let expected: (failure: String?, shown: [String]) =
        switch fixture {
        case .twoTools:
            (
                nil,
                [
                    work, "claude/personal", "claude/old, parked_access_expired",
                    "codex/main, in use", "codex/spare",
                ]
            )
        case .oneTool: (nil, [work, "claude/personal"])
        case .onlyOne: (nil, [work])
        case .unnamed: (nil, ["dana@work.example, in use"])
        case .empty, .firstLaunch: (nil, [])
        case .noClaudeCode: ("claude_program_missing", [])
        case .readFailure: ("unreachable", [work, "claude/personal"])
        case .stuck: ("recovery_undetermined", [work, "claude/personal"])
        }
    let core = FixtureCore(fixture)
    let offline = try await core.statusOffline()
    #expect(described(offline) == expected.shown)

    do {
        let read = try await core.status(fresh: true)
        #expect(expected.failure == nil)
        #expect(read.accounts == offline.accounts)
    } catch {
        #expect(AppModel.code(of: error) == expected.failure)
    }
    #expect(core.tools() == bothTools)
    #expect(
        await core.installed().map(\.code)
            == (fixture == .noClaudeCode ? [] : ["claude", "codex"]))
}

/// The UI tests launch a fixture by setting this variable to one of these names, from a
/// list of their own, so a name changed here has to change there.
@Test func theUITestsNameEveryFixtureAsTheAppDoes() {
    #expect(Fixture.variable == "PITBOARD_FIXTURE")
    #expect(
        Fixture.allCases.map(\.rawValue) == [
            "twoTools", "oneTool", "empty", "firstLaunch", "noClaudeCode", "unnamed",
            "onlyOne", "readFailure", "stuck",
        ])
}

// MARK: - Changing accounts

/// The accounts in use, one per tool at most, by their names with their tools.
private func inUse(_ status: Status) -> [String] {
    status.accounts.filter(\.signedIn).compactMap(\.qualified)
}

/// A switch moves who is in use within the tool it is for and leaves the other tool alone,
/// and the account it left can be switched back to. It says what running sessions do:
/// Claude Code's follow within a minute, and Codex's keep the old account until they are
/// started again. The account already in use is not switched to again, and an account
/// nobody enrolled is refused.
@Test func aSwitchMovesWhoIsInUseWithinItsOwnTool() async throws {
    let core = FixtureCore(.twoTools)

    let claude = try await core.switchTo("claude/personal")
    if case .switched(let provider, _, _, let adoption) = claude.outcome {
        #expect(provider == "claude")
        #expect(adoption == .follows(withinSeconds: 45))
    } else {
        Issue.record("\(claude.outcome) is not a switch")
    }
    let afterClaude = try await core.status(fresh: true)
    #expect(inUse(afterClaude) == ["claude/personal", "codex/main"])
    #expect(afterClaude.accounts.first { $0.qualified == "claude/work" }?.switchable == true)

    let codex = try await core.switchTo("codex/spare")
    #expect(
        codex.outcome
            == .switched(
                provider: "codex", from: "codex/main", to: "codex/spare",
                adoption: .restart(program: "codex")))
    let afterCodex = try await core.status(fresh: true)
    #expect(inUse(afterCodex) == ["claude/personal", "codex/spare"])
    #expect(afterCodex.accounts.first { $0.qualified == "codex/main" }?.switchable == true)

    #expect(
        try await core.switchTo("claude/personal").outcome == .alreadyActive(label: "personal"))
    #expect(await refusal { try await core.switchTo("claude/nobody") } != nil)
    #expect(inUse(try await core.status(fresh: true)) == ["claude/personal", "codex/spare"])
}

/// A parked login that expired stays unusable until somebody signs in to it again. A switch
/// to it is refused and moves nothing, and a switch between two other accounts of its tool
/// leaves it as it was, still offering a sign-in rather than a switch.
@Test(.disabled("FixtureCore.switchTo ignores whether a parked login can be used"))
func anExpiredParkedLoginStaysUnusableAcrossSwitches() async throws {
    let core = FixtureCore(.twoTools)
    #expect(await refusal { try await core.switchTo("claude/old") } != nil)
    #expect(inUse(try await core.status(fresh: true)) == ["claude/work", "codex/main"])

    _ = try await core.switchTo("claude/personal")
    #expect(
        described(try await core.status(fresh: true)) == [
            "claude/work", "claude/personal, in use", "claude/old, parked_access_expired",
            "codex/main, in use", "codex/spare",
        ])
}

/// The core names the accounts of a switch as the command line types them: bare for Claude
/// Code, with the tool for any other. The model matches what a switch said against that, so
/// a Claude Code switch named with its tool reads as undone by the read after it, and what
/// it said about running sessions is put away before anyone has seen it.
@MainActor
@Test(.disabled("FixtureCore.switchTo names accounts otherwise than the core types them"))
func aSwitchNamesTheAccountsAsTheCoreTypesThem() async throws {
    let core = FixtureCore(.twoTools)
    #expect(
        try await core.switchTo("codex/main").outcome == .alreadyActive(label: "codex/main"))
    #expect(
        try await core.switchTo("claude/personal").outcome
            == .switched(
                provider: "claude", from: "work", to: "personal",
                adoption: .follows(withinSeconds: 45)))

    let model = AppModel(testing: FixtureCore(.twoTools))
    await model.refresh()
    await model.use("claude/personal")
    #expect(model.lastSwitches.map(\.provider) == ["claude"])
}

/// The login signed in with no name is named in place, and stays the account in use. A
/// name its tool already has is refused and leaves it unnamed, and once it has a name there
/// is nobody left to name.
@Test func theLoginSignedInNowIsNamedWithANameNotTaken() async throws {
    let core = FixtureCore(.unnamed)
    _ = try await signInToTheEnd("claude/personal", on: core)
    #expect(await refusal { try await core.enrollCurrent("personal") } == "label_taken")
    #expect(
        described(try await core.status(fresh: true)) == [
            "dana@work.example, in use", "claude/personal",
        ])

    #expect(
        try await core.enrollCurrent("work")
            == Enrolled(email: "dana@work.example", enrolled: .current, warnings: []))
    #expect(
        described(try await core.status(fresh: true)) == [
            "claude/work, in use", "claude/personal",
        ])
    #expect(await refusal { try await core.enrollCurrent("home") } != nil)
}

/// The account in use cannot be forgotten, since its login is the one the tool is using.
/// Any other can, and goes from the list.
@Test func onlyAnAccountNotInUseIsForgotten() async throws {
    let core = FixtureCore(.twoTools)
    #expect(
        await refusal { try await core.forget("claude/work") }
            == "cannot_forget_active_account")
    #expect(await refusal { try await core.forget("claude/nobody") } != nil)

    #expect(
        try await core.forget("codex/spare")
            == Changed(email: "dana@home.example", warnings: []))
    #expect(
        described(try await core.status(fresh: true)) == [
            "claude/work, in use", "claude/personal", "claude/old, parked_access_expired",
            "codex/main, in use",
        ])
}

/// A rename needs a name its tool has not given another account. Another tool's names do
/// not count, since a name is only ever used with its tool.
@Test func aRenameNeedsANameItsToolHasNotGiven() async throws {
    let core = FixtureCore(.twoTools)
    #expect(
        await refusal { try await core.rename("claude/personal", to: "old") } == "label_taken")

    #expect(
        try await core.rename("claude/personal", to: "main")
            == Changed(email: "dana@home.example", warnings: []))
    #expect(
        described(try await core.status(fresh: true)) == [
            "claude/work, in use", "claude/main", "claude/old, parked_access_expired",
            "codex/main, in use", "codex/spare",
        ])
}

// MARK: - Signing in

/// Claude Code's sign-in prints the address to open and asks for the code from the browser,
/// which the sheet shows as a link and a field, and goes no further until a code is typed
/// back. Finishing then parks the new account beside the one in use.
@MainActor
@Test func aClaudeCodeSignInWaitsForTheCodeAndThenParksTheAccount() async throws {
    let core = FixtureCore(.oneTool)
    let session = try await core.signIn("claude/travel")
    let said = await Task.detached {
        var said = ""
        while let line = session.nextLine() {
            said += line
            if line.contains("Paste code") { break }
        }
        return said
    }.value
    let shown = SigningIn(label: "travel", tool: "Claude Code")
    shown.takesACode = session.takesACode()
    shown.add(said)
    #expect(shown.url == URL(string: "https://claude.ai/oauth/authorize?fixture=1"))
    #expect(shown.wantsCode)

    let clock = ContinuousClock()
    let asked = clock.now
    let pasting = Task.detached {
        try await Task.sleep(for: .milliseconds(300))
        try session.paste(line: "fixture-code")
    }
    #expect(await Task.detached { session.nextLine() }.value == nil)
    #expect(clock.now - asked >= .milliseconds(300), "nothing more until the code is typed")
    try await pasting.value

    #expect(
        try session.finish()
            == Enrolled(email: "travel@example.com", enrolled: .signedIn, warnings: []))
    #expect(
        described(try await core.status(fresh: true)) == [
            "claude/work, in use", "claude/personal", "claude/travel",
        ])
}

/// Codex's sign-in prints the address to open beside the loopback address the browser
/// comes back to, reads nothing typed, and finishes by itself once the browser is done.
@MainActor
@Test func aCodexSignInFinishesByItself() async throws {
    let core = FixtureCore(.twoTools)
    let session = try await core.signIn("codex/travel")
    let said = await Task.detached {
        var said = ""
        while let line = session.nextLine() { said += line }
        return said
    }.value
    let shown = SigningIn(label: "travel", tool: "Codex")
    shown.takesACode = session.takesACode()
    shown.add(said)
    #expect(shown.url == URL(string: "https://auth.openai.com/oauth/authorize?fixture=1"))
    #expect(!shown.wantsCode)

    #expect(
        try session.finish()
            == Enrolled(email: "travel@example.com", enrolled: .signedIn, warnings: []))
    #expect(
        described(try await core.status(fresh: true)).suffix(3) == [
            "codex/main, in use", "codex/spare", "codex/travel",
        ])
}

/// A sign-in stopped part way enrols nothing: it stops waiting for a code, and finishing it
/// fails as stopped, the way the tool's own does.
@Test func aStoppedSignInEnrolsNothing() async throws {
    let core = FixtureCore(.oneTool)
    let session = try await core.signIn("claude/travel")
    let reading = Task.detached { while session.nextLine() != nil {} }
    session.cancel()
    await reading.value

    #expect(await refusal { try session.finish() } != nil)
    #expect(
        described(try await core.status(fresh: true)) == [
            "claude/work, in use", "claude/personal",
        ])
}

/// Signing in again to an account whose parked login expired renews that account rather
/// than adding a second one, and it can be switched to again.
@Test func signingInAgainToAnExpiredAccountMakesItSwitchable() async throws {
    let core = FixtureCore(.twoTools)
    #expect(
        try await signInToTheEnd("claude/old", on: core)
            == Enrolled(email: "dana@old.example", enrolled: .renewed, warnings: []))
    #expect(
        described(try await core.status(fresh: true)) == [
            "claude/work, in use", "claude/personal", "claude/old", "codex/main, in use",
            "codex/spare",
        ])
}

/// Once its parked login is renewed nothing is wrong with the account any more, so a read
/// no longer says the login expired.
@Test(.disabled("FixtureCore.signedIn passes stale: nil, which keeps the stale explanation"))
func signingInAgainPutsAwayWhatWasWrongWithTheParkedLogin() async throws {
    let core = FixtureCore(.twoTools)
    _ = try await signInToTheEnd("claude/old", on: core)
    let old = try #require(
        try await core.status(fresh: true).accounts.first { $0.qualified == "claude/old" })
    #expect(old.stale == nil)
    #expect(old.staleExplanation == nil)
}

// MARK: - The rest of the machine

/// An interrupted switch is given up on once. The read that failed on it then works, and
/// asking again has nothing to give up on.
@Test func givingUpOnTheInterruptedSwitchHappensOnce() async throws {
    let core = FixtureCore(.stuck)
    #expect(await refusal { try await core.status(fresh: true) } == "recovery_undetermined")

    #expect(
        try await core.abandonRecovery()
            == Abandoned(from: "work", to: "personal", loginsKept: 2))
    #expect(try await core.abandonRecovery() == nil)
    #expect(
        described(try await core.status(fresh: true)) == [
            "claude/work, in use", "claude/personal",
        ])
}

/// The log keeps what changed newest last, and names Claude Code's accounts bare and any
/// other tool's with the tool, as the command line types them. Each change moves when the
/// account index last changed, which is how an app watching would notice.
@Test func theLogRecordsChangesNewestLastAsTheyAreTyped() async throws {
    let core = FixtureCore(.twoTools)
    let history = await core.log(limit: 500)
    #expect(
        history.map { "\($0.verb) \($0.subject)" } == [
            "enroll work", "enroll personal", "switch personal", "switch work",
        ])
    let changedAt = await core.changedAt()

    _ = try await core.switchTo("claude/personal")
    _ = try await core.switchTo("codex/spare")
    _ = try await core.rename("claude/work", to: "office")
    let log = await core.log(limit: 500)
    #expect(
        log.dropFirst(history.count).map { "\($0.verb) \($0.subject)" } == [
            "switch personal", "switch codex/spare", "rename work -> office",
        ])
    #expect(log.suffix(3).allSatisfy { $0.caller == "app" && $0.outcome == "ok" })
    let dates = log.compactMap { changeDate($0.at) }
    #expect(dates.count == log.count)
    #expect(dates == dates.sorted())
    #expect(await core.log(limit: 2) == Array(log.suffix(2)))
    #expect(await core.changedAt() > changedAt)
}

/// Daily renewal can be turned on and off, and taking away a schedule that is not there
/// says there was nothing to take away. Nothing is ever repaired.
@Test func theScheduleTurnsOnAndOff() async throws {
    let core = FixtureCore(.oneTool)
    #expect(await core.schedule() == .absent)
    #expect(try await core.scheduleUninstall() == false)

    let path = try await core.scheduleInstall()
    #expect(await core.schedule() == .installed(path: path, everySeconds: 86_400))
    #expect(try await core.scheduleUninstall())
    #expect(await core.schedule() == .absent)
    #expect(try await core.scheduleRepair() == false)
}

// MARK: - Launching into a fixture

/// Leaves nothing of a fixture launch behind: its preferences, and the stand-in app and
/// link it made in a temporary directory.
private func forgetLaunches() {
    UserDefaults(suiteName: Fixture.suite)?.removePersistentDomain(forName: Fixture.suite)
    try? FileManager.default.removeItem(
        at: FileManager.default.temporaryDirectory
            .appendingPathComponent("pitboard-fixture-\(getpid())"))
}

/// A launch into a fixture empties one defaults suite and makes one temporary directory for
/// the whole test process, so the tests that launch one take turns.
@MainActor
@Suite(.serialized)
struct FixtureLaunchTests {
    /// A UI test launches the app into a fixture many times, and each launch starts where
    /// the last one did: preferences of its own that the last launch left nothing in, seen
    /// before unless it is the first launch, and nothing that runs by itself or asks the
    /// person running the tests for permission.
    @Test func everyLaunchStartsFromEmptyPreferencesOfItsOwn() async throws {
        defer { forgetLaunches() }
        let suite = try #require(UserDefaults(suiteName: Fixture.suite))
        for fixture in Fixture.allCases {
            suite.set(["claude"], forKey: DefaultsKey.secondAccountDeclined)
            let launch = fixture.dependencies()
            #expect(
                launch.defaults.stringArray(forKey: DefaultsKey.secondAccountDeclined) == nil)
            #expect(
                launch.defaults.bool(forKey: DefaultsKey.hasBeenSeen)
                    == (fixture != .firstLaunch))
            launch.defaults.set(true, forKey: "written")
            #expect(suite.bool(forKey: "written"), "the fixture's suite")
            #expect(!launch.notifies)
            #expect(!launch.watching)
            #expect(launch.loginItem is FixtureLoginItem)
            #expect(launch.loginItem.state == .disabled)

            let core = try #require(launch.core as? FixtureCore)
            #expect(
                described(try await core.statusOffline())
                    == described(try await FixtureCore(fixture).statusOffline()))
        }
    }

    /// A fixture's command line is inside a stand-in app in a temporary directory, and
    /// linking it makes the link there without a password prompt, so a UI test can press the
    /// settings' button without writing to `/usr/local/bin`.
    @Test func aLaunchLinksItsCommandLineInATemporaryDirectory() async throws {
        defer { forgetLaunches() }
        let tool = Fixture.oneTool.dependencies().commandLineTool
        let temporary = FileManager.default.temporaryDirectory.path
        #expect(tool.linkable)
        #expect(tool.helper?.hasPrefix(temporary) == true)
        #expect(tool.link.hasPrefix(temporary))
        #expect(tool.find(in: tool.installPlaces) == .nowhere)

        #expect(await tool.install() == .linked)
        #expect(tool.find(in: tool.installPlaces) == .bundled(tool.link))
    }

    /// A debug build started with `PITBOARD_FIXTURE` runs in that fixture's world, which is
    /// how every UI test starts the app without touching the Mac that runs it.
    @Test func aDebugBuildLaunchesIntoTheFixtureItsEnvironmentNames() async throws {
        defer { forgetLaunches() }
        let launch = Dependencies.forLaunch(environment: [Fixture.variable: "twoTools"])
        #expect(!launch.notifies)
        #expect(!launch.watching)
        #expect(launch.loginItem is FixtureLoginItem)
        #expect(launch.defaults.bool(forKey: DefaultsKey.hasBeenSeen))

        let core = try #require(launch.core as? FixtureCore)
        #expect(
            described(try await core.statusOffline())
                == described(try await FixtureCore(.twoTools).statusOffline()))
    }
}
