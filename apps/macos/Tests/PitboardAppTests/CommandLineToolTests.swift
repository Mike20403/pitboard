import Foundation
import Testing

@testable import PitboardApp

/// A scratch directory with an app bundle carrying a command line, and whatever a test puts
/// beside it. Its name has a space and a quote in it, as a folder somebody made might.
private struct Scratch {
    let root: URL
    var app: URL { root.appendingPathComponent("Fake.app") }
    var helper: URL { app.appendingPathComponent("Contents/Helpers/pitboard") }

    init() throws {
        root = FileManager.default.temporaryDirectory
            .appendingPathComponent("pitboard's tool \(UUID().uuidString)")
        try FileManager.default.createDirectory(
            at: helper.deletingLastPathComponent(), withIntermediateDirectories: true)
        try program(at: helper)
    }

    /// A directory under the root, made if it is not there.
    func directory(_ name: String) throws -> String {
        let made = root.appendingPathComponent(name)
        try FileManager.default.createDirectory(at: made, withIntermediateDirectories: true)
        return made.path
    }

    func program(at url: URL, runnable: Bool = true) throws {
        try Data("#!/bin/sh\n".utf8).write(to: url)
        try FileManager.default.setAttributes(
            [.posixPermissions: runnable ? 0o755 : 0o644], ofItemAtPath: url.path)
    }

    func link(_ path: String, to destination: String) throws {
        try FileManager.default.createSymbolicLink(
            atPath: path, withDestinationPath: destination)
    }

    func remove() { try? FileManager.default.removeItem(at: root) }
}

/// A link is made only to the command line inside an app, where the core says an app keeps
/// it, and only where it is there. Whether the app stays where it is, rather than running
/// from the temporary copy macOS makes of one opened where it was downloaded, is the model's
/// to say before it offers the link. The app is a stand-in the test makes, so the answer
/// does not depend on whether the Mac running the tests has Pitboard installed.
@Test func onlyTheCommandLineInsideAnAppIsLinked() throws {
    let scratch = try Scratch()
    defer { scratch.remove() }
    let installed = CommandLineTool(bundle: scratch.app)
    #expect(installed.helper == scratch.helper.path)
    #expect(installed.linkable)

    let gone = CommandLineTool(bundle: URL(fileURLWithPath: "/nowhere/at/all/Pitboard.app"))
    #expect(gone.helper == "/nowhere/at/all/Pitboard.app/Contents/Helpers/pitboard")
    #expect(!gone.linkable, "nothing is there")

    let built = CommandLineTool(bundle: URL(fileURLWithPath: "/Users/x/apple/.build/debug"))
    #expect(built.helper == nil)
    #expect(!built.linkable)
}

/// A link is offered only to a command line this user may run, as the core judges a program
/// wherever it looks for one. A directory where the command line would be is not one, though
/// macOS lets this user search it, which `FileManager.isExecutableFile` took for running it;
/// nor is a file this user may not run.
@Test func aLinkIsOfferedOnlyToACommandLineThisUserMayRun() throws {
    let scratch = try Scratch()
    defer { scratch.remove() }
    try FileManager.default.removeItem(at: scratch.helper)
    try FileManager.default.createDirectory(
        at: scratch.helper, withIntermediateDirectories: false)
    #expect(!CommandLineTool(bundle: scratch.app).linkable, "a directory")

    try FileManager.default.removeItem(at: scratch.helper)
    try scratch.program(at: scratch.helper, runnable: false)
    #expect(!CommandLineTool(bundle: scratch.app).linkable, "a file nobody may run")
}

/// A path reaches the shell as it is: a quote cannot end the AppleScript string or the
/// shell's early, and nothing in it is read by the shell as a command.
@MainActor
@Test func thePathsReachTheShellAsTheyAre() throws {
    let helper =
        #"/Users/x/it's "mine" \ $(echo injected)/Pitboard.app/Contents/Helpers/pitboard"#
    let command = CommandLineTool.command(linking: helper, at: "/usr/local/bin/pitboard")

    var error: NSDictionary?
    let said = NSAppleScript(source: "return \(command)")?.executeAndReturnError(&error)
    #expect(error == nil, "\(String(describing: error))")
    #expect(
        said?.stringValue
            == #"mkdir -p '/usr/local/bin' && ln -sfh '/Users/x/it'\''s "mine" \ $(echo injected)"#
            + #"/Pitboard.app/Contents/Helpers/pitboard' '/usr/local/bin/pitboard'"#)

    let script = CommandLineTool.script(linking: helper, at: "/usr/local/bin/pitboard")
    #expect(script == "do shell script \(command) with administrator privileges")
    // Compiled and not run: running it asks for a password.
    let compiled = try #require(NSAppleScript(source: script))
    var refused: NSDictionary?
    let compiles = compiled.compileAndReturnError(&refused)
    #expect(compiles, "\(String(describing: refused))")
}

/// The command makes the directory and the link, and replaces a link already there, such as
/// one left by a copy of the app that has since moved. Run here without administrator
/// rights, in a scratch directory whose name a shell would otherwise read commands in.
@MainActor
@Test func theCommandMakesTheLink() throws {
    let scratch = try Scratch()
    defer { scratch.remove() }
    let hostile = try scratch.directory(#"a "b" \ $(echo injected)"#)
    let link = "\(hostile)/bin/pitboard"
    let command = CommandLineTool.command(linking: scratch.helper.path, at: link)

    func run() throws {
        var error: NSDictionary?
        NSAppleScript(source: "do shell script \(command)")?.executeAndReturnError(&error)
        #expect(error == nil, "\(String(describing: error))")
        #expect(
            try FileManager.default.destinationOfSymbolicLink(atPath: link)
                == scratch.helper.path)
    }
    try run()
    try FileManager.default.removeItem(atPath: link)
    try scratch.link(link, to: "/Applications/Moved.app/Contents/Helpers/pitboard")
    try run()
}

/// Anything where the link goes that is not a link is somebody's own, such as a Pitboard
/// they copied there, so it is kept and no script runs. A link is replaced whether or not
/// what it leads to is there, such as one left by a copy of the app that has since moved,
/// and so is nothing at all.
@Test func somebodysOwnFileWhereTheLinkGoesIsKept() async throws {
    let scratch = try Scratch()
    defer { scratch.remove() }
    let bin = try scratch.directory("bin")
    let link = "\(bin)/pitboard"
    try Data("mine".utf8).write(to: URL(fileURLWithPath: link))
    let scripts = Scripts()
    let tool = CommandLineTool(bundle: scratch.app, link: link, execute: scripts.run)

    #expect(
        await tool.install()
            == .failed("\(link) is already there and is not a link, so it was kept."))
    #expect(try String(contentsOfFile: link, encoding: .utf8) == "mine")
    try FileManager.default.removeItem(atPath: link)
    try FileManager.default.createDirectory(atPath: link, withIntermediateDirectories: false)
    #expect(await tool.install() != .linked, "a directory is kept too")
    #expect(scripts.ran.isEmpty)

    try FileManager.default.removeItem(atPath: link)
    #expect(await tool.install() == .linked, "nothing there")
    try scratch.link(link, to: "/Applications/Moved.app/Contents/Helpers/pitboard")
    #expect(await tool.install() == .linked, "a link to a copy that has moved")
    try FileManager.default.removeItem(atPath: link)
    try scratch.link(link, to: "\(bin)/../Fake.app/Contents/Helpers/pitboard")
    #expect(await tool.install() == .linked, "a link to a file that is there")
    let script = CommandLineTool.script(linking: scratch.helper.path, at: link)
    #expect(scripts.ran == [script, script, script])

    let gone = CommandLineTool(
        bundle: URL(fileURLWithPath: "/nowhere/at/all/Pitboard.app"), link: link,
        execute: scripts.run)
    #expect(
        await gone.install()
            == .failed("This copy of Pitboard cannot link the command line inside it."))
    #expect(scripts.ran.count == 3)
}

/// A password prompt somebody dismissed is their answer, not a failure: AppleScript raises
/// -128 for it. Any other error is a failure, in AppleScript's own words where it has any.
@Test func aDismissedPasswordPromptIsAnAnswer() {
    func error(_ number: Int, _ message: String? = nil) -> NSDictionary {
        var error: [String: Any] = [NSAppleScript.errorNumber: number]
        error[NSAppleScript.errorMessage] = message
        return error as NSDictionary
    }
    #expect(CommandLineTool.outcome(of: nil) == .linked)
    #expect(CommandLineTool.outcome(of: error(-128, "User canceled.")) == .cancelled)
    #expect(
        CommandLineTool.outcome(of: error(1, "ln: /usr/local/bin/pitboard: Permission denied"))
            == .failed("ln: /usr/local/bin/pitboard: Permission denied"))
    #expect(CommandLineTool.outcome(of: error(1)) == .failed("The link could not be made."))
}
