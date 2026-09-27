#if DEBUG
    import Foundation
    import PitboardKit

    /// A machine in a known state, for the UI tests and for looking at the app without
    /// touching the one it runs on. Only a debug build has these: a release build never reads
    /// `PITBOARD_FIXTURE`, so nothing outside can put the app in a world that is not real.
    public enum Fixture: String, CaseIterable, Sendable {
        /// Claude Code and Codex, each with an account in use and one to switch to, and a
        /// Claude Code account whose parked login needs a sign-in.
        case twoTools
        /// Claude Code alone, with two accounts.
        case oneTool
        /// Claude Code is installed and nobody is signed in to it.
        case empty
        /// `empty`, opened for the first time.
        case firstLaunch
        /// Claude Code is not on this machine.
        case noClaudeCode
        /// Somebody is signed in to Claude Code and pitboard has no name for them.
        case unnamed
        /// One Claude Code account, so nothing to switch to.
        case onlyOne
        /// The service could not be reached, and the last numbers measured are shown.
        case readFailure
        /// An interrupted switch that cannot be finished until the service answers.
        case stuck

        /// The environment variable a debug build reads the fixture's name from.
        static let variable = "PITBOARD_FIXTURE"

        /// The defaults a fixture keeps its preferences in, emptied at every launch so each
        /// test starts from the same place and nothing reaches the real app's.
        static let suite = "com.usepitboard.Pitboard.fixture"

        @MainActor
        func dependencies() -> Dependencies {
            let defaults = UserDefaults(suiteName: Self.suite) ?? .standard
            defaults.removePersistentDomain(forName: Self.suite)
            if self != .firstLaunch { defaults.set(true, forKey: DefaultsKey.hasBeenSeen) }
            return Dependencies(
                core: FixtureCore(self),
                defaults: defaults,
                loginItem: FixtureLoginItem(),
                commandLineTool: CommandLineTool(
                    bundle: URL(fileURLWithPath: "/Applications/Pitboard.app"),
                    home: NSTemporaryDirectory(),
                    link: NSTemporaryDirectory() + "pitboard-fixture/bin/pitboard",
                    execute: { _ in nil }),
                notifies: false,
                watching: false)
        }
    }

    /// A login item that remembers what it was told and registers nothing.
    @MainActor
    final class FixtureLoginItem: LoginItem {
        private(set) var state: LoginItemState = .disabled
        func register() throws { state = .enabled }
        func unregister() throws { state = .disabled }
        func openSystemSettings() {}
    }
#endif
