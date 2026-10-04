import Foundation

/// How the Share extension hands a link to the app it came in: a pitboard link,
/// `<scheme>://open?url=<link>`, which the app's account picker receives.
///
/// Anything on the Mac can open a pitboard link, so the app reads one as strictly as it reads
/// a link typed by a stranger, and opens nothing until the person chooses an account.
public enum Handoff {
    /// The Info.plist key both the app and the extension read their build's scheme from:
    /// `pitboard` in a release, `pitboard-debug` in a debug build, so a debug build never
    /// receives a link meant for a copy installed. A release build made from a branch claims
    /// `pitboard` like the copy installed; the extension still hands its link to the app it
    /// came in.
    public static let schemeKey = "PitboardURLScheme"

    /// The scheme `bundle` declares under `schemeKey`.
    public static func scheme(in bundle: Bundle) -> String? {
        bundle.object(forInfoDictionaryKey: schemeKey) as? String
    }

    /// The pitboard link that asks the app to open `link`, with everything but the unreserved
    /// characters percent-encoded. That is a subset of what JavaScript's `encodeURIComponent`
    /// leaves bare, so a link either of them builds reads back the same.
    public static func url(opening link: SiteLink, scheme: String) -> URL {
        let unreserved = CharacterSet(
            charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")
        let encoded =
            link.url.absoluteString.addingPercentEncoding(withAllowedCharacters: unreserved)
            ?? ""
        var parts = URLComponents()
        parts.scheme = scheme
        parts.host = "open"
        parts.percentEncodedQuery = "url=\(encoded)"
        // Only unreserved characters and escapes, after a scheme the app declares: it parses.
        return parts.url!
    }

    /// The link a pitboard link of `scheme` carries, or why it carries none this build opens.
    ///
    /// Strict on purpose. A pitboard link written without encoding the link it carries,
    /// `pitboard://open?url=https://claude.ai/x?a=1&b=2#c`, reads as a shorter link, another
    /// item and a fragment, and opening the first part would open the wrong page without a
    /// word.
    public static func link(in url: URL, scheme: String) throws(LinkRefusal) -> SiteLink {
        guard url.scheme?.lowercased() == scheme.lowercased(),
            let parts = URLComponents(url: url, resolvingAgainstBaseURL: false),
            parts.host?.lowercased() == "open", parts.path.isEmpty || parts.path == "/",
            parts.user == nil, parts.password == nil, parts.port == nil, parts.fragment == nil,
            let items = parts.queryItems, items.count == 1, let item = items.first,
            item.name == "url", let carried = item.value, !carried.isEmpty
        else { throw .unreadable }
        return try SiteLink(carried)
    }

    /// The app an extension is inside: `Pitboard.app` for
    /// `Pitboard.app/Contents/PlugIns/PitboardShare.appex`, or nil when it is inside none.
    public static func containingApp(of extensionURL: URL) -> URL? {
        var folder = extensionURL.standardizedFileURL.deletingLastPathComponent()
        while folder.path != "/" && !folder.path.isEmpty {
            if folder.pathExtension == "app" { return folder }
            folder = folder.deletingLastPathComponent()
        }
        return nil
    }
}
