import Foundation
import PitboardKit

/// Which page asks: an account window's own, or a sign-in window one of its pages opened.
enum PageRole: Equatable {
    case window
    case popup
}

/// A navigation as the policy sees it, from what WebKit says about it.
struct NavigationRequest: Equatable {
    enum Target: Equatable {
        /// The page itself.
        case page
        /// A frame inside it.
        case frame
        /// A new window: WebKit's target frame is nil.
        case newWindow

        /// The target of a navigation whose target frame is main, is not, or is not there at
        /// all, as `WKNavigationAction.targetFrame?.isMainFrame` says.
        init(frameIsMain: Bool?) {
            switch frameIsMain {
            case nil: self = .newWindow
            case true?: self = .page
            case false?: self = .frame
            }
        }
    }

    /// Which page asked.
    enum Asker: Equatable {
        /// The site's own page, as the window's main page.
        case site
        /// A page of the site's sign-in, as the window's main page, or a sign-in window's own
        /// page.
        case signIn
        /// Anything else: a frame inside the page, such as an artifact's, another window's
        /// page, or a load the app started itself.
        case other
    }

    let url: URL
    let target: Target
    /// A person clicked a link or submitted a form.
    var clicked = false
    /// WebKit says to download what this asks for.
    var download = false
    var asker = Asker.other
}

/// What becomes of a navigation.
enum NavigationDecision: Equatable {
    /// Loaded where it was asked for.
    case load
    /// A new window's page, loaded in the page that asked instead: a window per account, so
    /// a link of the site's that asks for a tab stays signed in as this account.
    case loadInPage
    /// A new window for the site's sign-in, sharing the account's store, so what it signs in
    /// is this account's. A page that opens a blank window and gives it an address afterwards
    /// gets one too, and the sign-in window's own policy decides where that address goes.
    case popup
    /// Saved to the downloads folder; `ask` asks the person first, as Safari asks before a
    /// website it does not know downloads.
    case download(ask: Bool)
    /// Handed to macOS: the default browser for a web page, the default email app for an
    /// address.
    case openElsewhere(URL)
    /// Stopped, and the window says why.
    case refuse(WindowNote.Kind)
    /// Stopped, and nothing is said.
    case ignore
}

/// What becomes of a response a page receives.
enum ResponseDecision: Equatable {
    case show
    case download(ask: Bool)
}

/// Where an account window's navigations go: the site and its sign-in stay in the window,
/// Google's sign-in is refused, other web pages go to the browser, and nothing else leaves.
///
/// A pure function of what WebKit says, so every rule is tested without starting WebKit.
struct NavigationPolicy: Equatable {
    let site: Site
    /// The scheme every page the window keeps is on: `https` in a live run, and the fixture's
    /// own in a fixture, where each stand-in site keeps its host on that scheme.
    let scheme: String

    /// Where a window of `site` starts.
    var home: URL { URL(string: "\(scheme)://\(site.host)/")! }

    /// What becomes of `request`, asked by a page in `role`. The first rule that matches
    /// decides.
    ///
    /// A new window is decided once, when WebKit asks for the page to put in it: a link that
    /// asks for one reaches the navigation's own decision first, and refusing it there stops
    /// WebKit asking for the window at all, so a sign-in link would never open its window.
    func decide(_ request: NavigationRequest, in role: PageRole) -> NavigationDecision {
        let url = request.url
        let scheme = url.scheme?.lowercased() ?? ""
        // Saving a file does not move the page. Artifact downloads are often `blob:` links,
        // whose host Foundation does not see. Nothing plain `http`, local or unknown is
        // saved, and a sign-in window saves nothing. Only the site's own page saves without
        // asking: an artifact runs somebody else's code in a frame of its own.
        if request.download {
            guard role == .window, ["https", "blob", "data"].contains(scheme) else {
                return .ignore
            }
            return .download(ask: request.asker != .site)
        }
        // The page decides what it embeds: artifacts render in frames of their own origin.
        // What decides is where the page goes. A web page never opens a local file.
        if request.target == .frame {
            return scheme == "file" ? .ignore : .load
        }
        if role == .popup {
            return decideInPopup(request)
        }
        // A blank page, or a window asked for with no address yet, which a page fills in
        // after opening it. In a sign-in window the address it is given is decided again. Only
        // the site's own page and its sign-in get one: a frame given a window could fill it with
        // a page of its own, which nothing on the window's title would tell apart.
        if url.absoluteString.isEmpty || scheme == "about" {
            if request.target == .page { return .load }
            return request.asker == .other ? .ignore : .popup
        }
        if isSite(url) {
            return request.target == .page ? .load : .loadInPage
        }
        if isSignIn(url) {
            if request.target == .page { return .load }
            return request.asker == .other ? .ignore : .popup
        }
        return leaving(request)
    }

    /// A sign-in window loads the site and its sign-in pages, saves nothing and opens no
    /// window of its own; anything else goes where it would from the window. Its page may go
    /// blank itself, and nothing else may make it: a frame that opened it could then fill it
    /// with a page of its own.
    private func decideInPopup(_ request: NavigationRequest) -> NavigationDecision {
        let url = request.url
        let scheme = url.scheme?.lowercased() ?? ""
        if url.absoluteString.isEmpty || scheme == "about" {
            return request.target == .page && request.asker != .other ? .load : .ignore
        }
        if isSite(url) || isSignIn(url) {
            return request.target == .page ? .load : .ignore
        }
        return leaving(request)
    }

    /// Where a request goes that leaves the window's own hosts. Google's sign-in is refused,
    /// since Google blocks it inside apps and in the browser it would sign in the browser.
    /// Any other web page goes to the browser, and an address a person clicked, or one asked
    /// for in a new window, to the default email app, as a browser does. A web page never
    /// launches another app through Pitboard; one a person clicked is said to be refused.
    private func leaving(_ request: NavigationRequest) -> NavigationDecision {
        let url = request.url
        let scheme = url.scheme?.lowercased() ?? ""
        if scheme == "http" || scheme == "https" {
            if let host = url.host?.lowercased(), site.blockedHosts.contains(host) {
                return .refuse(.googleRefused)
            }
            return .openElsewhere(url)
        }
        let asked = request.clicked || request.target == .newWindow
        if scheme == "mailto" {
            return asked ? .openElsewhere(url) : .ignore
        }
        return asked && !scheme.isEmpty ? .refuse(.otherApp(scheme: scheme)) : .ignore
    }

    /// What becomes of a response: downloaded when WebKit cannot show it or the server says
    /// it is an attachment, and shown otherwise. `disposition` is its `Content-Disposition`.
    /// Only a response to the site's own page saves without asking.
    func decideResponse(canShow: Bool, disposition: String?, fromSite: Bool) -> ResponseDecision
    {
        let kind = disposition?.trimmingCharacters(in: .whitespaces).lowercased() ?? ""
        guard !canShow || kind.hasPrefix("attachment") else { return .show }
        return .download(ask: !fromSite)
    }

    /// Whether `url` is one of the site's own pages: on the window's scheme, on the site's
    /// host or an alias of it, with no port and no user information to hide the host behind.
    func isSite(_ url: URL) -> Bool {
        matches(url, hosts: site.hosts)
    }

    /// Whether `url` is a page of the site's own sign-in, which leaves the site.
    func isSignIn(_ url: URL) -> Bool {
        matches(url, hosts: site.signInHosts)
    }

    /// Which page asked, from the frame WebKit says asked: its main page on the site, its main
    /// page on the site's sign-in, or anything else.
    func asker(isMainFrame: Bool, scheme: String, host: String, port: Int)
        -> NavigationRequest
        .Asker
    {
        guard isMainFrame, scheme.lowercased() == self.scheme, port == 0 else { return .other }
        let host = host.lowercased()
        if site.hosts.contains(host) { return .site }
        if site.signInHosts.contains(host) { return .signIn }
        return .other
    }

    private func matches(_ url: URL, hosts: [String]) -> Bool {
        guard url.scheme?.lowercased() == scheme, let host = url.host?.lowercased(),
            hosts.contains(host), url.port == nil, url.user == nil, url.password == nil
        else { return false }
        return true
    }
}
