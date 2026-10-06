import Foundation
import PitboardKit
import Testing

@testable import PitboardApp

/// A renewal schedule an app up to 0.3.0 wrote starts the app with `renew`, and the command
/// line inside this one does the renewal in its place. A copy with none inside it starts
/// nothing, rather than a menu bar app that would repair the schedule from inside its job.
/// Anything else opens the app.
@Test func aScheduleThatStartsTheAppRenewsWithTheCommandLineInsideIt() {
    let app = "/Applications/Pitboard.app/Contents/MacOS/Pitboard"
    let helper = "/Applications/Pitboard.app/Contents/Helpers/pitboard"
    #expect(Launch.action(for: [app, "renew"], helper: helper) == .renew(helper))
    #expect(Launch.action(for: [app, "renew"], helper: nil) == .fail)

    #expect(Launch.action(for: [app], helper: helper) == .app)
    #expect(Launch.action(for: [], helper: helper) == .app)
    #expect(Launch.action(for: [app, "renew", "--json"], helper: helper) == .app)
    #expect(Launch.action(for: [app, "-AppleLanguages", "(en)"], helper: helper) == .app)
}

/// A scratch Pitboard directory, with or without the model's `app.json` in it. Removed by
/// `remove()`.
private struct ScratchDirectory {
    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent("pitboard-earlier-\(UUID().uuidString)")
    var appFile: URL { root.appendingPathComponent("app.json") }

    init() throws {
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    }

    /// What the model writes once it has taken what UserDefaults held.
    func keep() throws {
        try Data(#"{"second_account_declined":["claude"],"has_been_seen":true}"#.utf8)
            .write(to: appFile)
    }

    func remove() { try? FileManager.default.removeItem(at: root) }
}

/// What this app kept in UserDefaults before the model kept its preferences in Pitboard's
/// directory is handed over as it was, with the old nudge's key beside the others, for the
/// model to read as it reads them. Nothing set is nothing handed over.
@Test func whatUserDefaultsHeldIsHandedOverAsItWas() throws {
    let directory = try ScratchDirectory()
    defer { directory.remove() }
    let defaults = TestDefaults()
    let store = EarlierStore(defaults: defaults, appFile: directory.appFile)
    #expect(store.handOver() == nil, "nothing was ever set")

    defaults.set(["codex"], forKey: "secondAccountDeclined")
    defaults.set(true, forKey: "hasBeenSeen")
    defaults.set(true, forKey: "hideSecondAccountNudge")
    #expect(
        store.handOver()
            == EarlierPreferences(
                secondAccountDeclined: ["codex"], hasBeenSeen: true,
                secondAccountNudgeHidden: true))
    #expect(store.handOver() != nil, "kept until the model has kept them")
}

/// The old keys go once the model has kept what they held in `app.json`, at launch or as the
/// app quits once the model has stopped, and not before: a launch that stopped before the
/// file was written hands them over again. Where the file is there, nothing is handed over,
/// since the model reads the file and nothing else.
@Test func theOldKeysGoOnceTheModelHasKeptThem() throws {
    let directory = try ScratchDirectory()
    defer { directory.remove() }
    let defaults = TestDefaults()
    defaults.set(["codex"], forKey: "secondAccountDeclined")
    defaults.set(true, forKey: "hasBeenSeen")
    defaults.set(true, forKey: "menuBarShows")
    let store = EarlierStore(defaults: defaults, appFile: directory.appFile)

    #expect(!store.forgetOnceKept())
    #expect(defaults.object(forKey: "hasBeenSeen") != nil)

    try directory.keep()
    #expect(store.forgetOnceKept())
    for key in ["secondAccountDeclined", "hasBeenSeen", "hideSecondAccountNudge"] {
        #expect(defaults.object(forKey: key) == nil, "\(key)")
    }
    #expect(defaults.object(forKey: "menuBarShows") != nil, "the app's own stay")

    defaults.set(true, forKey: "hasBeenSeen")
    #expect(store.handOver() == nil)
    #expect(defaults.object(forKey: "hasBeenSeen") == nil)
}

/// The store is the one in the Pitboard directory the core reads from the environment.
@Test func theStoreIsInThePitboardDirectoryTheCoreReads() {
    let store = EarlierStore(
        defaults: TestDefaults(),
        environment: ["HOME": "/Users/dana", "PITBOARD_HOME": "/Users/dana/elsewhere"])
    #expect(store.appFile.path == "/Users/dana/elsewhere/app.json")
}

#if DEBUG
    /// The UI tests launch a fixture by setting this variable to one of these names, from a
    /// list of their own in `UITests/Launching.swift`, so a name changed in the core's
    /// fixtures has to change there. The names are checked against a library that has them.
    @Test func theUITestsNameEveryFixtureAsTheAppDoes() {
        #expect(Fixture.variable == "PITBOARD_FIXTURE")
        let launched = [
            "twoTools", "oneTool", "empty", "firstLaunch", "noClaudeCode", "unnamed",
            "onlyOne", "readFailure", "stuck", "chatGPTOpen",
        ]
        if !fixtureNames().isEmpty {
            #expect(fixtureNames() == launched)
        }
    }

    /// A fixture's folder is where the model makes its world: `pitboard-fixture` in the
    /// temporary directory as Rust's `std::env::temp_dir` finds it, which reads `TMPDIR`
    /// where Foundation's temporary directory does not. Its command line is linked in that
    /// folder's `bin`, where the model looks, to the one inside its stand-in app.
    @Test func aFixtureLinksItsCommandLineWhereItsModelLooks() {
        #expect(
            Fixture.folder(environment: ["TMPDIR": "/private/tmp/x/"]).path
                == "/private/tmp/x/pitboard-fixture")
        #expect(
            Fixture.folder(environment: [:])
                == FileManager.default.temporaryDirectory.appendingPathComponent(
                    "pitboard-fixture", isDirectory: true))
        let tool = Fixture.commandLineTool(in: URL(fileURLWithPath: "/private/tmp/x/f"))
        #expect(tool.helper == "/private/tmp/x/f/Pitboard.app/Contents/Helpers/pitboard")
        #expect(tool.link == "/private/tmp/x/f/bin/pitboard")
    }
#endif
