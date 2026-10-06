import Foundation
import PitboardKit
import Testing

@testable import PitboardApp

// What the settings do with macOS's own APIs, which the model does not: opening at login,
// and linking the command line with an administrator's password. Daily renewal, Renew Now
// and which `pitboard` a terminal runs are the model's, tested in `pitboard-ffi`.

/// A login item that answers the way macOS can: a registration may wait for approval or be
/// refused, and the person may change it in System Settings while the app is running.
@MainActor
private final class ScriptedLoginItem: LoginItem {
    var state: LoginItemState
    /// Whether a registration waits for the person to allow it in System Settings.
    var needsApproval = false
    /// What the next register or unregister is refused with, if anything.
    var refusing: Error?
    private(set) var settingsOpened = 0

    init(_ state: LoginItemState = .disabled) {
        self.state = state
    }

    func register() throws {
        if let refusing { throw refusing }
        state = needsApproval ? .requiresApproval : .enabled
    }
    func unregister() throws {
        if let refusing { throw refusing }
        state = .disabled
    }
    func openSystemSettings() { settingsOpened += 1 }
}

// MARK: - Opening at login

/// The switch shows what macOS has, not what was last asked for, and the person can change
/// it in System Settings at any time, so it is read again when the settings ask.
@MainActor
@Test func openingAtLoginIsWhatMacOSSays() {
    let item = ScriptedLoginItem(.enabled)
    let openAtLogin = OpenAtLogin(item)
    #expect(openAtLogin.state == .enabled)

    openAtLogin.set(false)
    #expect(openAtLogin.state == .disabled)
    openAtLogin.set(true)
    #expect(openAtLogin.state == .enabled)
    #expect(openAtLogin.failed == nil)

    item.state = .disabled
    #expect(openAtLogin.state == .enabled, "not read again until something asks")
    openAtLogin.read()
    #expect(openAtLogin.state == .disabled)
}

/// macOS can hold a new login item until the person allows it, and until then the app does
/// not open at login. That is not a failure: the settings say it is waiting and offer
/// System Settings, where it is allowed.
@MainActor
@Test func aLoginItemWaitingForApprovalSaysSo() {
    let item = ScriptedLoginItem()
    item.needsApproval = true
    let openAtLogin = OpenAtLogin(item)

    openAtLogin.set(true)
    #expect(openAtLogin.state == .requiresApproval)
    #expect(openAtLogin.failed == nil)
    openAtLogin.openSystemSettings()
    #expect(item.settingsOpened == 1)

    item.state = .enabled
    openAtLogin.read()
    #expect(openAtLogin.state == .enabled)
}

/// A refused change says why beside the switch, and the switch shows what macOS has rather
/// than what was asked for. The next change that works puts the reason away.
@MainActor
@Test func aRefusedLoginItemSaysWhyAndShowsWhatMacOSHas() {
    let item = ScriptedLoginItem()
    item.refusing = NSError(
        domain: "LoginItems", code: 1,
        userInfo: [NSLocalizedDescriptionKey: "Operation not permitted"])
    let openAtLogin = OpenAtLogin(item)

    openAtLogin.set(true)
    #expect(openAtLogin.failed == "Operation not permitted")
    #expect(openAtLogin.state == .disabled)

    item.state = .enabled
    openAtLogin.set(false)
    #expect(openAtLogin.failed == "Operation not permitted")
    #expect(openAtLogin.state == .enabled)

    item.refusing = nil
    openAtLogin.set(false)
    #expect(openAtLogin.failed == nil)
    #expect(openAtLogin.state == .disabled)
}

// MARK: - Linking the command line

/// A stand-in app in a temporary directory with a command line inside it, and a `bin` for
/// the link. Removed by `remove()`.
private struct StandInApp {
    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent("pitboard-settings-\(UUID().uuidString)")
    var helper: String {
        root.appendingPathComponent("Pitboard.app/Contents/Helpers/pitboard").path
    }
    var link: String { root.appendingPathComponent("bin/pitboard").path }

    init() throws {
        try FileManager.default.createDirectory(
            atPath: (helper as NSString).deletingLastPathComponent,
            withIntermediateDirectories: true)
        try Data("#!/bin/sh\n".utf8).write(to: URL(fileURLWithPath: helper))
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o755], ofItemAtPath: helper)
    }

    /// This app's command line, linked by running `execute`.
    func tool(_ execute: @escaping CommandLineTool.Runner) -> CommandLineTool {
        CommandLineTool(helper: helper, link: link, execute: execute)
    }

    /// What the script run as an administrator does.
    func makeLink() {
        try? FileManager.default.createDirectory(
            atPath: (link as NSString).deletingLastPathComponent,
            withIntermediateDirectories: true)
        try? FileManager.default.createSymbolicLink(
            atPath: link, withDestinationPath: helper)
    }

    func remove() { try? FileManager.default.removeItem(at: root) }
}

/// The button says it is linking for as long as macOS's password prompt is up, and once the
/// link is made the model is asked to look for the `pitboard` a terminal runs again, which
/// is how the settings come to show this app's own on the `PATH`.
@MainActor
@Test func linkingSaysSoUntilThePasswordPromptIsAnswered() async throws {
    let app = try StandInApp()
    defer { app.remove() }
    let (prompted, prompting) = AsyncStream.makeStream(of: Void.self)
    let answered = DispatchSemaphore(value: 0)
    let standIn = StandInModel(snapshot(0))
    let link = CommandLineLink(
        app.tool { _ in
            prompting.yield()
            answered.wait()
            app.makeLink()
            return nil
        }, model: AppModel(model: standIn))

    let linking = Task { await link.install() }
    for await _ in prompted { break }
    #expect(link.linking)
    #expect(standIn.sent.isEmpty, "nothing is looked for while the prompt is up")
    answered.signal()
    await linking.value
    #expect(!link.linking)
    #expect(link.failed == nil)
    #expect(standIn.sent == [.lookForCommandLine])
    #expect(FileManager.default.fileExists(atPath: app.link))
}

/// Why a link could not be made is said beside the button, in AppleScript's words, and
/// never as a failure in the window: the model is only asked to look again. A dismissed
/// password prompt is somebody's answer, so it says nothing and puts away what the last try
/// said. A copy with nothing to link says so without asking for a password.
@MainActor
@Test func aLinkThatFailedSaysWhyAndADismissedPromptSaysNothing() async throws {
    let app = try StandInApp()
    defer { app.remove() }
    let scripts = Scripts()
    let standIn = StandInModel(snapshot(0))
    let link = CommandLineLink(app.tool(scripts.run), model: AppModel(model: standIn))

    scripts.raise(1, "ln: \(app.link): Permission denied")
    await link.install()
    #expect(link.failed == "ln: \(app.link): Permission denied")
    #expect(!link.linking)

    scripts.raise(-128, "User canceled.")
    await link.install()
    #expect(link.failed == nil)
    #expect(scripts.ran.count == 2)
    #expect(standIn.sent == [.lookForCommandLine, .lookForCommandLine])

    let unlinkable = CommandLineLink(
        CommandLineTool(helper: nil, link: app.link, execute: scripts.run),
        model: AppModel(model: standIn))
    await unlinkable.install()
    #expect(
        unlinkable.failed == "This copy of Pitboard cannot link the command line inside it.")
    #expect(scripts.ran.count == 2)
}
