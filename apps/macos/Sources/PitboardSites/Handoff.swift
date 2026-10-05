import Foundation

/// How the Share extension hands a link to the app it came in: a Pitboard link,
/// `<scheme>://open?url=<link>`, which the app's account picker receives.
///
/// Anything on the Mac can open a Pitboard link, so the app reads one as strictly as it reads
/// a link typed by a stranger, and opens nothing until the person chooses an account.
public enum Handoff {
    /// The Pitboard link that asks the app to open `link`, with everything but the unreserved
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

    /// The link a Pitboard link of `scheme` carries, or why it carries none this build opens.
    ///
    /// Strict on purpose. A Pitboard link written without encoding the link it carries,
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
}
