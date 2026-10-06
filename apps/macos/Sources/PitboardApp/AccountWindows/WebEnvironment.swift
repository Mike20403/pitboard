import AppKit
import Foundation
import PitboardKit
import WebKit

/// Everything an account window reaches outside the app through, so a launch decides once
/// which world the windows run in: the sites themselves, or a fixture's stand-in pages that
/// reach no network and keep nothing on disk.
@MainActor
struct WebEnvironment {
    /// The scheme every page a window keeps is on: `https` in a live run, and the fixture's
    /// own in a fixture.
    let scheme: String
    /// WebKit's persistent stores, or stand-ins.
    let stores: any WebsiteDataStores
    /// Where downloads are saved.
    let downloads: URL
    /// Hands a link to macOS: the default browser for a web page, the default email app for
    /// an address.
    let openElsewhere: @MainActor (URL) -> Void
    /// Anything a configuration needs before a web view is made from it.
    let configure: @MainActor (WKWebViewConfiguration) -> Void
    /// Waits between attempts to delete a store WebKit still holds.
    let pause: @MainActor (Duration) async -> Void

    /// The sites themselves, WebKit's stores, the Downloads folder and the default browser.
    static func live() -> WebEnvironment {
        let home = FileManager.default.homeDirectoryForCurrentUser
        return WebEnvironment(
            scheme: "https",
            stores: WebKitDataStores(),
            downloads: FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask)
                .first ?? home.appendingPathComponent("Downloads"),
            openElsewhere: { NSWorkspace.shared.open($0) },
            configure: { _ in },
            pause: { try? await Task.sleep(for: $0) })
    }

    /// What the account windows' records of the Pitboard directory an app started with
    /// `environment` reads are kept under, which the model is handed as
    /// `WindowsLaunch.key`: the core's directory, standardised as Foundation standardises a
    /// file URL. The records were keyed that way before the core said where the directory
    /// is, and the core gives the path as the environment does, so a `PITBOARD_HOME` holding
    /// a `.`, a `..` or a trailing `/` would key them differently and orphan every store and
    /// page already recorded. This is the one place that keys them.
    nonisolated static func recordKey(environment: [String: String]) -> String {
        let directory = URL(fileURLWithPath: pitboardDirectory(environment: environment))
        return directory.standardizedFileURL.path
    }

    /// Where the model keeps the account windows' records, `windows.json`: the app's own
    /// folder in Application Support, named by its bundle id, under the person's own Library
    /// whatever `HOME` says, as WebKit keeps the stores they describe. A debug build and a
    /// release have bundle ids, stores and records of their own.
    nonisolated static func recordsDirectory(bundle: Bundle = .main) -> URL {
        let support =
            FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first
            ?? FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support")
        return support.appendingPathComponent(
            bundle.bundleIdentifier ?? "com.usepitboard.Pitboard", isDirectory: true)
    }
}
