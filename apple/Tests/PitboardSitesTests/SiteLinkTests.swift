import Foundation
import PitboardSites
import Testing

/// The link `text` opens, as the window loads it, or nil when it is refused.
private func accepted(_ text: String) -> String? {
    (try? SiteLink(text))?.url.absoluteString
}

private func site(_ text: String) -> Site? {
    (try? SiteLink(text))?.site
}

private func refusal(_ text: String) -> LinkRefusal? {
    do {
        _ = try SiteLink(text)
        return nil
    } catch {
        return error
    }
}

// MARK: - The sites

@Test func eachSiteServesOneTool() {
    #expect(Site.all == [.claude, .chatGPT], "in the order a listing shows them")
    #expect(Site.claude.tool == "claude")
    #expect(Site.chatGPT.tool == "codex", "a Codex login is a ChatGPT sign-in")
    #expect(Site.sites(for: "claude") == [.claude])
    #expect(Site.sites(for: "codex") == [.chatGPT])
    #expect(Site.sites(for: "gemini").isEmpty)
    #expect(Site.claude.name == "claude.ai")
    #expect(Site.chatGPT.name == "chatgpt.com", "never ChatGPT, which is OpenAI's app")
}

/// Hashed into every window's store id: a change would leave every window without its data.
@Test func theStoreNamesNeverChange() {
    #expect(Site.claude.storeName == "claude")
    #expect(Site.chatGPT.storeName == "codex")
}

@Test func aSiteIsFoundByAnyOfItsHosts() {
    #expect(Site.serving(host: "claude.ai") == .claude)
    #expect(Site.serving(host: "CLAUDE.AI") == .claude)
    #expect(Site.serving(host: "chat.openai.com") == .chatGPT)
    #expect(Site.serving(host: "auth.openai.com") == nil, "a sign-in host is no site")
    #expect(Site.serving(host: "sub.claude.ai") == nil)
}

/// Google refuses its pages inside apps, whichever site leads to them.
@Test func everySiteBlocksGooglesSignIn() {
    for site in Site.all {
        #expect(site.blockedHosts.contains("accounts.google.com"), "\(site.name)")
        #expect(site.signInHosts.isDisjoint(with: site.blockedHosts))
    }
}

@Test func theSitesAreNamedInASentence() {
    #expect(Site.names(.and) == "claude.ai and chatgpt.com")
    #expect(Site.names(.or) == "claude.ai or chatgpt.com")
}

// MARK: - What may be opened

@Test func onlyTheSitesOwnLinksAreAccepted() {
    #expect(accepted("https://claude.ai/new") == "https://claude.ai/new")
    #expect(accepted("  https://claude.ai/new\n") == "https://claude.ai/new")
    #expect(accepted("claude.ai/chat/abc") == "https://claude.ai/chat/abc")
    #expect(accepted("Claude.AI/chat/abc") == "https://claude.ai/chat/abc")
    #expect(accepted("claude.ai") == "https://claude.ai/")
    #expect(accepted("http://claude.ai/chat/abc") == "https://claude.ai/chat/abc")
    #expect(accepted("HTTPS://CLAUDE.AI/new") == "https://claude.ai/new")
    #expect(accepted("https://claude.ai") == "https://claude.ai/")
    #expect(
        accepted("https://claude.ai/public/artifacts/0e5a?x=1&y=%20#frag")
            == "https://claude.ai/public/artifacts/0e5a?x=1&y=%20#frag")
    #expect(
        accepted("https://chatgpt.com/c/68d1?model=x") == "https://chatgpt.com/c/68d1?model=x")
    #expect(accepted("chatgpt.com/share/abc") == "https://chatgpt.com/share/abc")
    #expect(accepted("chatgpt.com") == "https://chatgpt.com/")
    #expect(
        accepted("https://chatgpt.com/codex/tasks/t1") == "https://chatgpt.com/codex/tasks/t1")

    #expect(refusal("https://example.com/") == .notASite(host: "example.com"))
    #expect(refusal("example.com/x") == .noLink, "only a site's host is taken without a scheme")
    #expect(
        refusal("https://claude.ai.evil.example/") == .notASite(host: "claude.ai.evil.example")
    )
    #expect(refusal("https://evilclaude.ai/") == .notASite(host: "evilclaude.ai"))
    #expect(refusal("https://sub.claude.ai/") == .notASite(host: "sub.claude.ai"))
    #expect(refusal("https://sub.chatgpt.com/") == .notASite(host: "sub.chatgpt.com"))
    #expect(refusal("https://claude.ai:8443/") == .notASite(host: "claude.ai:8443"))
    #expect(refusal("https://x@claude.ai/") == .notASite(host: nil))
    #expect(refusal("https://x:y@chatgpt.com/") == .notASite(host: nil))
    #expect(
        refusal("https://claude.ai@evil.example/") == .notASite(host: "evil.example"),
        "the host is what follows the @")
    #expect(refusal("javascript:alert(1)") == .noLink)
    #expect(refusal("data:text/html,hi") == .noLink)
    #expect(refusal("file:///etc/hosts") == .noLink)
    #expect(refusal("pitboard-fixture://claude.ai/x") == .noLink)
    #expect(refusal("") == .noLink)
    #expect(refusal("   ") == .noLink)
    #expect(refusal("nothing to open") == .noLink)
    #expect(refusal("https://claude.ai/" + String(repeating: "a", count: 8200)) == .tooLong)
}

@Test func eachLinkBelongsToItsOwnSite() {
    #expect(site("https://claude.ai/new") == .claude)
    #expect(site("https://chatgpt.com/") == .chatGPT)
}

/// Measured on 29 September 2026: each of these answers with a redirect to the same path on
/// chatgpt.com, so a link to one is a chatgpt.com link, opened on chatgpt.com itself.
@Test func chatGPTsOtherHostsAreTakenAsChatGPT() {
    for alias in ["chat.openai.com", "www.chatgpt.com", "chat.com"] {
        #expect(site("https://\(alias)/c/abc") == .chatGPT, "\(alias)")
        #expect(accepted("https://\(alias)/c/abc?x=1#y") == "https://chatgpt.com/c/abc?x=1#y")
        #expect(accepted("\(alias)/share/x") == "https://chatgpt.com/share/x")
    }
    #expect(refusal("https://sub.chat.com/") == .notASite(host: "sub.chat.com"))
}

/// OpenAI's sign-in pages are on a host of their own. A link to one, such as the page that
/// approves a Codex sign-in on another device, is no chatgpt.com link and opens nothing.
@Test func openAIsSignInHostIsNotASite() {
    #expect(
        refusal("https://auth.openai.com/codex/device") == .notASite(host: "auth.openai.com"))
}

/// Somebody else's sign-in link would sign the window in as whoever it belongs to, while the
/// window stays titled with the person's own label.
@Test func aSignInLinkIsRefusedFromOutside() {
    for text in [
        "https://claude.ai/magic-link#a:b", "https://claude.ai/MAGIC-LINK",
        "https://claude.ai/magic-link/x", "https://claude.ai/magic-link/",
        "https://claude.ai/magic%2Dlink#a:b", "claude.ai/magic-link#a:b",
        // WebKit drops dot segments, `%2e` included, before it loads a link, and keeps an
        // empty segment that a server merging slashes would route to sign-in all the same.
        "https://claude.ai/./magic-link#a:b", "https://claude.ai/x/../magic-link#a:b",
        "https://claude.ai/a/../magic-link#a:b", "https://claude.ai/%2e/magic-link#a:b",
        "claude.ai/%2e%2e/magic-link#a:b", "claude.ai/%2E%2E/magic-link#a:b",
        "https://claude.ai/./magic-link", "https://claude.ai//magic-link#a:b",
        "https://claude.ai/%2Fmagic-link#a:b",
    ] {
        #expect(refusal(text) == .signInLink(.claude), "\(text)")
    }
    #expect(accepted("https://claude.ai/magic-links-guide") != nil)
    #expect(accepted("https://claude.ai/chat/magic-link") != nil)
}

/// chatgpt.com's sign-in comes back through `/api/auth`, on chatgpt.com and its other hosts.
@Test func aChatGPTSignInLinkIsRefusedFromOutside() {
    for text in [
        "https://chatgpt.com/api/auth/callback/openai?code=x",
        "https://chatgpt.com/API/Auth/session", "https://chatgpt.com/api/auth",
        "https://chatgpt.com//api//auth/x", "https://chatgpt.com/x/../api/auth/x",
        "https://chat.openai.com/api/auth/callback/openai",
    ] {
        #expect(refusal(text) == .signInLink(.chatGPT), "\(text)")
    }
    #expect(accepted("https://chatgpt.com/api/other") != nil)
    #expect(accepted("https://chatgpt.com/c/api/auth") != nil)
    #expect(
        accepted("https://claude.ai/api/auth") != nil, "a path only chatgpt.com signs in on")
}

/// The sites' own links never have a dot segment. One that does not lead to sign-in is
/// refused too, rather than opened somewhere other than it reads.
@Test func aLinkWithADotSegmentIsRefused() {
    #expect(refusal("https://claude.ai/chat/../new") == .noLink)
    #expect(refusal("https://claude.ai/./new") == .noLink)
    #expect(refusal("https://claude.ai/magic-link/..") == .noLink)
    #expect(refusal("https://chatgpt.com/c/../x") == .noLink)
    #expect(accepted("https://claude.ai/chat/a.b") == "https://claude.ai/chat/a.b")
    #expect(accepted("https://claude.ai/x/...") == "https://claude.ai/x/...")
}

/// The Share extension and the picker say a refusal the same way, from its reason.
@Test func eachRefusalSaysWhy() {
    #expect(
        LinkRefusal.notASite(host: "example.com").errorDescription
            == "pitboard opens claude.ai and chatgpt.com links only. This link is on "
            + "example.com.")
    #expect(
        LinkRefusal.notASite(host: nil).errorDescription
            == "pitboard opens claude.ai and chatgpt.com links only.")
    #expect(
        LinkRefusal.signInLink(.chatGPT).errorDescription
            == "pitboard doesn’t open chatgpt.com sign-in links from outside: one would sign "
            + "the window in as whoever the link belongs to. Sign in inside the account’s "
            + "chatgpt.com window.")
    #expect(
        LinkRefusal.noLink.errorDescription
            == "There is no claude.ai or chatgpt.com link in what was shared.")
    #expect(LinkRefusal.tooLong.errorDescription?.contains("too long") == true)
    #expect(LinkRefusal.unreadable.errorDescription?.contains("Update pitboard") == true)
}
