import Foundation

/// A website pitboard opens in an account's window: whose accounts it serves, which hosts are
/// its own, where its sign-in goes, and what its window says about signing in.
///
/// Everything that tells one site from another is a value here. The window, its navigation
/// policy, the menus, the account picker and the Share extension read these values and never
/// name a site, so a site is added by declaring it in `all`, with its fixture page and tests.
public struct Site: Hashable, Sendable, Identifiable {
    /// The site's own host, in lower case: where its windows start and what links to it are
    /// opened on. It is also the site's name, so a window on chatgpt.com is never confused
    /// with OpenAI's ChatGPT app, which pitboard quits around a Codex switch.
    public let host: String
    /// The tool whose enrolled accounts get this site's windows, as the core names it in
    /// `Tool.code`.
    public let tool: String
    /// Other hosts that send a browser to `host` with the path kept, taken as `host`. Each is
    /// matched exactly: a subdomain of one is not one.
    public let aliases: Set<String>
    /// Hosts the site's own sign-in leaves for and comes back from. A window's page loads them
    /// as it loads the site, and one asked for in a new window opens in a sign-in window that
    /// shares the account's store. A link from outside to one opens nothing.
    public let signInHosts: Set<String>
    /// Hosts a window refuses as its page or a new window, and never hands to the browser.
    /// Google blocks its sign-in and the connection of its apps inside apps, and in the browser
    /// either would sign in the browser rather than this account. A frame the page embeds is
    /// the page's own decision.
    public let blockedHosts: Set<String>
    /// Paths that sign in whoever a link belongs to, as the lower-case segments they start
    /// with. Opened from outside, somebody else's would sign the window in as them, under the
    /// person's own label.
    public let signInPaths: [[String]]
    /// Hashed into the store id of each of the site's windows. It never changes once a site
    /// has shipped: a change would leave every window of the site without its data, and the
    /// next sweep would delete that data. Kept apart from `tool` on purpose, so a tool renamed
    /// in the core cannot move it.
    public let storeName: String
    /// How to sign in to the site in a window, as the steps its sign-in page shows.
    public let signInSteps: String
    /// What the blocked hosts leave unusable in the site's window.
    public let blockedServices: String

    public init(
        host: String, tool: String, aliases: Set<String> = [], signInHosts: Set<String> = [],
        blockedHosts: Set<String> = [], signInPaths: [[String]] = [], storeName: String,
        signInSteps: String, blockedServices: String
    ) {
        self.host = host
        self.tool = tool
        self.aliases = aliases
        self.signInHosts = signInHosts
        self.blockedHosts = blockedHosts
        self.signInPaths = signInPaths
        self.storeName = storeName
        self.signInSteps = signInSteps
        self.blockedServices = blockedServices
    }

    public var id: String { host }
    /// The site as a sentence names it.
    public var name: String { host }
    /// Every host whose links this site opens.
    public var hosts: Set<String> { aliases.union([host]) }
}

extension Site {
    /// Google's sign-in, which Google refuses inside apps.
    static let google: Set<String> = ["accounts.google.com"]

    /// claude.ai, for Claude Code's accounts. Its emailed sign-in link is under `/magic-link`,
    /// and its email sign-in never leaves claude.ai.
    public static let claude = Site(
        host: "claude.ai", tool: "claude", blockedHosts: google,
        signInPaths: [["magic-link"]], storeName: "claude",
        signInSteps: "Click Continue with email, open the email on your phone, tap its link, "
            + "then enter here the code claude.ai shows there.",
        blockedServices: "Gmail, Google Drive, Google Calendar and Google single sign-on "
            + "cannot be connected in this window.")

    /// chatgpt.com, for Codex's accounts: a Codex login is a ChatGPT sign-in. Measured on 29
    /// September 2026, `chat.openai.com` answers 308, `www.chatgpt.com` 301 and `chat.com` 307,
    /// each to the same path on chatgpt.com. Its sign-in pages are on auth.openai.com, which
    /// OpenAI's help centre names among the hosts its sign-in needs, and it offers Microsoft
    /// and Apple, whose pages these are (article 7426629, read on 29 September 2026). It comes
    /// back through `/api/auth`. A passkey needs an app macOS lets act as a browser, which
    /// pitboard is not.
    public static let chatGPT = Site(
        host: "chatgpt.com", tool: "codex",
        aliases: ["chat.openai.com", "www.chatgpt.com", "chat.com"],
        signInHosts: [
            "auth.openai.com", "login.live.com", "login.microsoftonline.com",
            "appleid.apple.com",
        ],
        blockedHosts: google, signInPaths: [["api", "auth"]], storeName: "codex",
        signInSteps: "Enter your email address, then its password or the code chatgpt.com "
            + "emails you, or click Continue with Microsoft or Continue with Apple. Passkeys "
            + "do not work in this window.",
        blockedServices: "Google Drive, Gmail and Google Calendar cannot be connected in this "
            + "window.")

    /// Every site, in the order a listing shows them.
    public static let all: [Site] = [.claude, .chatGPT]

    /// The sites whose windows the accounts of `tool` get.
    public static func sites(for tool: String) -> [Site] {
        all.filter { $0.tool == tool }
    }

    /// The site whose own host or alias `host` is, ignoring case.
    public static func serving(host: String) -> Site? {
        let host = host.lowercased()
        return all.first { $0.hosts.contains(host) }
    }

    /// The sites' names as a sentence lists them: "claude.ai and chatgpt.com".
    public static func names(_ type: ListFormatStyle<StringStyle, [String]>.ListType) -> String
    {
        all.map(\.name).formatted(.list(type: type))
    }
}
