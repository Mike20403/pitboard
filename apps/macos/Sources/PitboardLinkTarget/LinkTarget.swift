import Foundation

/// Where a Pitboard link goes on this Mac: the scheme a build answers, and the app a Share
/// extension is inside.
///
/// Both are read from bundles, so they stay native. Foundation only, and nothing an app
/// extension may not use: the app and its Share extension both link it, so the key they read
/// their scheme by is written once.
public enum LinkTarget {
    /// The Info.plist key the app and its Share extension each read their build's scheme
    /// from: `pitboard` in a release, `pitboard-debug` in a debug build, so a debug build
    /// never receives a link meant for a copy installed. A release build made from a branch
    /// claims `pitboard` like the copy installed; the extension still hands its link to the
    /// app it came in.
    public static let schemeKey = "PitboardURLScheme"

    /// The scheme `bundle` declares under `schemeKey`.
    public static func scheme(in bundle: Bundle) -> String? {
        bundle.object(forInfoDictionaryKey: schemeKey) as? String
    }

    /// The app an extension is inside: `Pitboard.app` for
    /// `Pitboard.app/Contents/PlugIns/PitboardShare.appex`, or nil when it is inside none.
    /// The extension opens its link with that app rather than whichever copy Launch Services
    /// picks, so a debug extension reaches the debug app.
    public static func containingApp(of extensionURL: URL) -> URL? {
        var folder = extensionURL.standardizedFileURL.deletingLastPathComponent()
        while folder.path != "/" && !folder.path.isEmpty {
            if folder.pathExtension == "app" { return folder }
            folder = folder.deletingLastPathComponent()
        }
        return nil
    }
}
