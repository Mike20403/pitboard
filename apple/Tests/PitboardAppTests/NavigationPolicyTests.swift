import Foundation
import PitboardSites
import Testing

@testable import PitboardApp

private let chatGPT = NavigationPolicy(site: .chatGPT, scheme: "https")
private let claude = NavigationPolicy(site: .claude, scheme: "https")

private func request(
    _ text: String, _ target: NavigationRequest.Target = .page, clicked: Bool = false,
    download: Bool = false, asker: NavigationRequest.Asker = .other
) -> NavigationRequest {
    NavigationRequest(
        url: URL(string: text) ?? URL(string: "about:blank")!, target: target, clicked: clicked,
        download: download, asker: asker)
}

/// A window asked for with no address yet, as WebKit hands `window.open('')` over: an empty
/// address, measured on macOS 27.
private func emptyWindow(asker: NavigationRequest.Asker = .site) -> NavigationRequest {
    NavigationRequest(url: NSURL(string: "")! as URL, target: .newWindow, asker: asker)
}

// MARK: - An account window's own page

@Test func theSiteLoadsInItsWindow() {
    #expect(chatGPT.decide(request("https://chatgpt.com/c/abc"), in: .window) == .load)
    #expect(chatGPT.decide(request("https://CHATGPT.com/"), in: .window) == .load)
    #expect(
        chatGPT.decide(request("https://chat.openai.com/c/abc"), in: .window) == .load,
        "an alias redirects to the site with the path kept")
    #expect(claude.decide(request("https://claude.ai/new"), in: .window) == .load)
}

/// A window per account: a link of the site's that asks for a tab stays in this window, so it
/// stays signed in as this account, and Back returns to where the person was.
@Test func aSiteLinkAskingForANewWindowLoadsInThePage() {
    #expect(
        chatGPT.decide(request("https://chatgpt.com/c/abc", .newWindow), in: .window)
            == .loadInPage)
}

/// The site's sign-in leaves the site: it loads in the window, and asked for in a new window
/// it opens a sign-in window sharing the account's store.
@Test func theSitesSignInLoadsOrOpensASignInWindow() {
    let auth = "https://auth.openai.com/authorize?client_id=x"
    #expect(chatGPT.decide(request(auth), in: .window) == .load)
    #expect(chatGPT.decide(request(auth, .newWindow, asker: .site), in: .window) == .popup)
    #expect(
        chatGPT.decide(
            request("https://login.live.com/oauth", .newWindow, asker: .site), in: .window)
            == .popup)
    #expect(
        claude.decide(request(auth, .newWindow), in: .window)
            == .openElsewhere(URL(string: auth)!),
        "another site's sign-in is not this one's")
}

/// Measured on macOS 27: `window.open('')` asks for a window with an empty address and gives
/// it one afterwards, which the sign-in window's own policy decides.
@Test func aBlankWindowOpensAsASignInWindow() {
    #expect(chatGPT.decide(emptyWindow(), in: .window) == .popup)
    #expect(chatGPT.decide(emptyWindow(asker: .signIn), in: .window) == .popup)
    #expect(
        chatGPT.decide(request("about:blank", .newWindow, asker: .site), in: .window) == .popup)
    #expect(chatGPT.decide(request("about:blank"), in: .window) == .load)
    #expect(chatGPT.decide(request("about:srcdoc", .frame), in: .window) == .load)
}

/// The page decides what it embeds: artifacts render in frames of their own origin. A web
/// page never opens a local file.
@Test func framesLoadWhatThePageEmbeds() {
    #expect(chatGPT.decide(request("https://cdn.example/x", .frame), in: .window) == .load)
    #expect(
        claude.decide(request("https://www.claudeusercontent.com/", .frame), in: .window)
            == .load)
    #expect(chatGPT.decide(request("file:///etc/hosts", .frame), in: .window) == .ignore)
}

@Test func otherWebPagesGoToTheBrowser() {
    let away = URL(string: "https://example.com/docs")!
    #expect(chatGPT.decide(request(away.absoluteString), in: .window) == .openElsewhere(away))
    #expect(
        chatGPT.decide(request(away.absoluteString, .newWindow), in: .window)
            == .openElsewhere(away))
    #expect(
        chatGPT.decide(request("http://example.com/"), in: .window)
            == .openElsewhere(URL(string: "http://example.com/")!))
    #expect(
        claude.decide(request("https://claude.ai:8443/"), in: .window)
            == .openElsewhere(URL(string: "https://claude.ai:8443/")!),
        "a port is not the site")
    #expect(
        claude.decide(request("https://x@claude.ai/"), in: .window)
            == .openElsewhere(URL(string: "https://x@claude.ai/")!),
        "neither is user information hiding the host")
}

/// Google blocks its sign-in and the connection of its apps inside apps, and in the browser
/// either would sign in the browser rather than this account.
@Test func googlesSignInIsRefusedNotHandedToTheBrowser() {
    let google = "https://accounts.google.com/o/oauth2/v2/auth"
    #expect(
        chatGPT.decide(request(google, clicked: true), in: .window) == .refuse(.googleRefused))
    #expect(chatGPT.decide(request(google, .newWindow), in: .window) == .refuse(.googleRefused))
    #expect(chatGPT.decide(request(google), in: .popup) == .refuse(.googleRefused))
}

/// An address goes to the email app only when somebody asked, as a browser does. A web page
/// never launches another app through Pitboard, and one a person clicked says so.
@Test func onlyAskedForAddressesLeaveAndOtherAppsAreRefused() {
    let mail = URL(string: "mailto:help@example.com")!
    #expect(
        chatGPT.decide(request(mail.absoluteString, clicked: true), in: .window)
            == .openElsewhere(mail))
    #expect(
        chatGPT.decide(request(mail.absoluteString, .newWindow), in: .window)
            == .openElsewhere(mail))
    #expect(chatGPT.decide(request(mail.absoluteString), in: .window) == .ignore)
    #expect(
        chatGPT.decide(request("vscode://file/x", clicked: true), in: .window)
            == .refuse(.otherApp(scheme: "vscode")))
    #expect(chatGPT.decide(request("vscode://file/x"), in: .window) == .ignore)
}

/// Saving a file does not move the page. Only the site's own page saves without asking: an
/// artifact runs somebody else's code in a frame of its own.
@Test func downloadsFromTheSiteSaveAndOthersAsk() {
    #expect(
        claude.decide(
            request("https://claude.ai/files/x", download: true, asker: .site), in: .window)
            == .download(ask: false))
    #expect(
        claude.decide(
            request("blob:https://claude.ai/1", download: true, asker: .site), in: .window)
            == .download(ask: false))
    #expect(
        claude.decide(request("data:text/plain,x", .frame, download: true), in: .window)
            == .download(ask: true))
    #expect(
        claude.decide(
            request("https://example.com/x.zip", .newWindow, download: true), in: .window)
            == .download(ask: true))
    #expect(
        claude.decide(
            request("http://claude.ai/x", download: true, asker: .site), in: .window)
            == .ignore,
        "nothing plain http is saved")
    #expect(claude.decide(request("file:///x", download: true), in: .window) == .ignore)
}

// MARK: - A sign-in window's page

@Test func aSignInWindowLoadsTheSiteAndItsSignInOnly() {
    #expect(chatGPT.decide(request("https://auth.openai.com/log-in"), in: .popup) == .load)
    #expect(
        chatGPT.decide(request("https://chatgpt.com/api/auth/callback"), in: .popup) == .load)
    #expect(chatGPT.decide(request("about:blank", asker: .signIn), in: .popup) == .load)
    #expect(chatGPT.decide(emptyWindow(), in: .popup) == .ignore, "it opens no window")
    #expect(
        chatGPT.decide(request("https://auth.openai.com/x", .newWindow), in: .popup) == .ignore)
    #expect(
        chatGPT.decide(request("https://example.com/terms", clicked: true), in: .popup)
            == .openElsewhere(URL(string: "https://example.com/terms")!))
    #expect(
        chatGPT.decide(
            request("https://chatgpt.com/x", download: true, asker: .site), in: .popup)
            == .ignore,
        "it saves nothing")
}

// MARK: - Responses

@Test func aResponseIsDownloadedWhenItCannotBeShownOrIsAnAttachment() {
    #expect(chatGPT.decideResponse(canShow: true, disposition: nil, fromSite: true) == .show)
    #expect(
        chatGPT.decideResponse(canShow: true, disposition: "inline", fromSite: true) == .show)
    #expect(
        chatGPT.decideResponse(
            canShow: true, disposition: " Attachment; filename=x", fromSite: true)
            == .download(ask: false))
    #expect(
        chatGPT.decideResponse(canShow: false, disposition: nil, fromSite: false)
            == .download(ask: true))
}

// MARK: - Origins

/// A fixture keeps each site's host on a scheme of its own, so the same policy runs there, and
/// a link of the site's opens the stand-in rather than the network.
@Test func aFixturesWindowKeepsItsOwnScheme() throws {
    let fixture = NavigationPolicy(site: .chatGPT, scheme: "pitboard-fixture")
    #expect(fixture.home.absoluteString == "pitboard-fixture://chatgpt.com/")
    #expect(fixture.decide(request("pitboard-fixture://chatgpt.com/c/x"), in: .window) == .load)
    #expect(
        fixture.decide(request("https://chatgpt.com/c/x"), in: .window)
            == .openElsewhere(URL(string: "https://chatgpt.com/c/x")!))
    let link = try SiteLink("https://chat.openai.com/c/x?y=1#z")
    #expect(
        fixture.address(of: link).absoluteString == "pitboard-fixture://chatgpt.com/c/x?y=1#z")
    #expect(chatGPT.address(of: link) == link.url)
}

/// A frame given a blank window could fill it with a page of its own, which nothing on the
/// window would tell apart from the site's: only the site's page and its sign-in get one.
@Test func aFrameGetsNoBlankWindow() {
    #expect(chatGPT.decide(emptyWindow(asker: .other), in: .window) == .ignore)
    #expect(chatGPT.decide(request("about:blank", .newWindow), in: .window) == .ignore)
}

/// What decides is the frame that asked: a frame's download aimed at the top page still asks,
/// and the site's own download asking for a new window does not.
@Test func theFrameThatAskedDecidesWhetherADownloadAsks() {
    #expect(
        claude.decide(request("data:text/plain,x", .page, download: true), in: .window)
            == .download(ask: true))
    #expect(
        claude.decide(
            request("https://claude.ai/x", .newWindow, download: true, asker: .site),
            in: .window) == .download(ask: false))
    #expect(
        claude.decide(
            request("https://auth.openai.com/x", download: true, asker: .signIn), in: .window)
            == .download(ask: true))
}

@Test func theFrameThatAskedIsToldApart() {
    #expect(
        chatGPT.asker(isMainFrame: true, scheme: "https", host: "chatgpt.com", port: 0) == .site
    )
    #expect(
        chatGPT.asker(isMainFrame: true, scheme: "https", host: "chat.com", port: 0) == .site)
    #expect(
        chatGPT.asker(isMainFrame: true, scheme: "https", host: "auth.openai.com", port: 0)
            == .signIn)
    #expect(
        chatGPT.asker(isMainFrame: false, scheme: "https", host: "chatgpt.com", port: 0)
            == .other)
    #expect(
        chatGPT.asker(isMainFrame: true, scheme: "http", host: "chatgpt.com", port: 0) == .other
    )
    #expect(
        chatGPT.asker(isMainFrame: true, scheme: "https", host: "chatgpt.com", port: 8443)
            == .other)
    #expect(chatGPT.asker(isMainFrame: true, scheme: "https", host: "", port: 0) == .other)
    #expect(
        chatGPT.asker(isMainFrame: true, scheme: "https", host: "evil.example", port: 0)
            == .other)
}

/// Only the site's own page or its sign-in opens a sign-in window: a frame that could open one
/// could fill it with a page of its own, titled as it likes.
@Test func aFrameOpensNoSignInWindow() {
    let auth = "https://login.microsoftonline.com/x"
    #expect(chatGPT.decide(request(auth, .newWindow, asker: .site), in: .window) == .popup)
    #expect(chatGPT.decide(request(auth, .newWindow, asker: .signIn), in: .window) == .popup)
    #expect(chatGPT.decide(request(auth, .newWindow), in: .window) == .ignore)
}

/// A sign-in window's page may go blank itself; its opener may not make it.
@Test func onlyASignInWindowsOwnPageBlanksIt() {
    #expect(chatGPT.decide(request("about:blank", asker: .signIn), in: .popup) == .load)
    #expect(chatGPT.decide(request("about:blank"), in: .popup) == .ignore)
    #expect(chatGPT.decide(emptyWindow(asker: .other), in: .popup) == .ignore)
}
