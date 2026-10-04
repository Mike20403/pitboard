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
    /// Which stores this Pitboard directory has made.
    let record: StoreRecord
    /// The page each account's window was last on.
    let pages: PageRecord
    /// Where downloads are saved.
    let downloads: URL
    /// Hands a link to macOS: the default browser for a web page, the default email app for
    /// an address.
    let openElsewhere: @MainActor (URL) -> Void
    /// Anything a configuration needs before a web view is made from it.
    let configure: @MainActor (WKWebViewConfiguration) -> Void
    /// Waits between attempts to delete a store WebKit still holds.
    let pause: @MainActor (Duration) async -> Void

    /// The sites themselves, WebKit's stores recorded in `defaults` for the Pitboard directory
    /// this launch reads, the Downloads folder and the default browser.
    static func live(
        defaults: UserDefaults,
        environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> WebEnvironment {
        let home = FileManager.default.homeDirectoryForCurrentUser
        let directory = Settings.pitboardDirectory(environment: environment)
        return WebEnvironment(
            scheme: "https",
            stores: WebKitDataStores(),
            record: StoreRecord(defaults: defaults, directory: directory),
            pages: PageRecord(defaults: defaults, directory: directory),
            downloads: FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask)
                .first ?? home.appendingPathComponent("Downloads"),
            openElsewhere: { NSWorkspace.shared.open($0) },
            configure: { _ in },
            pause: { try? await Task.sleep(for: $0) })
    }
}
