import Foundation

/// A link one of the sites opens: the site, and the address its window loads.
///
/// Made only by checking a link from outside the app, so holding one means the check passed.
public struct SiteLink: Hashable, Sendable {
    public let site: Site
    /// The link on the site's own host over `https`, with its path, query and fragment.
    public let url: URL

    /// The longest link read. The sites' own links to a page are under 200 characters, and
    /// anything longer would only fill the picker.
    public static let longest = 8192

    /// `text` as a link one of the sites opens, or why it is not one.
    ///
    /// Spaces around it are dropped, a bare host of a site is taken as `https`, and `http` as
    /// `https`. Only a site's own host or one of its aliases is accepted: no other host, no
    /// subdomain, no port and no user information. A site's sign-in link is refused as well,
    /// since opened in an account's window somebody else's would sign that window in as them.
    public init(_ text: String) throws(LinkRefusal) {
        var typed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard typed.count <= Self.longest else { throw .tooLong }
        let bare = typed.lowercased()
        let bareHost = Site.all.flatMap(\.hosts).contains { host in
            bare == host || ["/", "?", "#"].contains { bare.hasPrefix(host + $0) }
        }
        if bareHost { typed = "https://" + typed }
        guard !typed.isEmpty, let parts = URLComponents(string: typed),
            let scheme = parts.scheme?.lowercased(), scheme == "https" || scheme == "http",
            let host = parts.host?.lowercased(), !host.isEmpty
        else { throw .noLink }
        guard let site = Site.serving(host: host) else { throw .notASite(host: host) }
        if let port = parts.port { throw .notASite(host: "\(host):\(port)") }
        guard parts.user == nil, parts.password == nil else { throw .notASite(host: nil) }
        if let refusal = Self.refusal(ofPath: parts.path, on: site) { throw refusal }
        var canonical = URLComponents()
        canonical.scheme = "https"
        canonical.host = site.host
        canonical.percentEncodedPath =
            parts.percentEncodedPath.isEmpty ? "/" : parts.percentEncodedPath
        canonical.percentEncodedQuery = parts.percentEncodedQuery
        canonical.percentEncodedFragment = parts.percentEncodedFragment
        guard let url = canonical.url else { throw .noLink }
        self.site = site
        self.url = url
    }

    /// Why a path of `site`'s, percent-decoded as `URLComponents.path` gives it, is not
    /// opened, or nil when it may be.
    ///
    /// Checked by segment, as WebKit loads it: WebKit drops `.` and `..` segments, `%2e`
    /// included, before the request is sent, and Foundation keeps them. The sites' own links
    /// never have one, so a path with a dot segment is refused, as a sign-in link when that is
    /// where it leads. Empty segments are skipped, so `//magic-link`, which a server merging
    /// slashes would route to sign-in, is refused as well.
    static func refusal(ofPath path: String, on site: Site) -> LinkRefusal? {
        let segments = path.lowercased().split(separator: "/", omittingEmptySubsequences: true)
        var resolved: [Substring] = []
        for segment in segments {
            switch segment {
            case ".": continue
            case "..": _ = resolved.popLast()
            default: resolved.append(segment)
            }
        }
        let signsIn = site.signInPaths.contains { prefix in
            resolved.count >= prefix.count && zip(resolved, prefix).allSatisfy { $0 == $1 }
        }
        if signsIn { return .signInLink(site) }
        return segments.contains { $0 == "." || $0 == ".." } ? .noLink : nil
    }
}

/// Why a link from outside is not opened.
public enum LinkRefusal: Error, Hashable, Sendable, LocalizedError {
    /// Nothing that reads as a link to a web page.
    case noLink
    /// A link to a host none of the sites is, with the host when there is one to name.
    case notASite(host: String?)
    /// A site's sign-in link, which would sign a window in as whoever it is for.
    case signInLink(Site)
    /// Longer than any of the sites' links is.
    case tooLong
    /// A Pitboard link this version does not read: another scheme, another request, or a
    /// link inside it that was not encoded.
    case unreadable

    public var errorDescription: String? {
        switch self {
        case .noLink:
            "There is no \(Site.names(.or)) link in what was shared."
        case .notASite(let host?):
            "Pitboard opens \(Site.names(.and)) links only. This link is on \(host)."
        case .notASite(nil):
            "Pitboard opens \(Site.names(.and)) links only."
        case .signInLink(let site):
            "Pitboard doesn’t open \(site.name) sign-in links from outside: one would sign the "
                + "window in as whoever the link belongs to. Sign in inside the account’s "
                + "\(site.name) window."
        case .tooLong:
            "What was shared is too long to be a \(Site.names(.or)) link."
        case .unreadable:
            "This Pitboard link isn’t one this version of Pitboard can read. Update Pitboard "
                + "and share the page again."
        }
    }
}
