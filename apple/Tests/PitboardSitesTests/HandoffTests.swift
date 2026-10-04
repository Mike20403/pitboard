import Foundation
import PitboardSites
import Testing

private func link(_ text: String) -> SiteLink {
    try! SiteLink(text)
}

/// The link a pitboard link `text` carries, as its address, or why it carries none.
private func carried(_ text: String, scheme: String = "pitboard") -> Result<String, LinkRefusal>
{
    Result { () throws(LinkRefusal) in
        try Handoff.link(in: URL(string: text)!, scheme: scheme).url.absoluteString
    }
}

/// The format is the contract between the Share extension and the app it came in, which a
/// release may be updated around separately, so one link is pinned exactly: everything but
/// the unreserved characters is encoded, a space as `%20` and a plus as `%2B`, as
/// JavaScript's `encodeURIComponent` encodes them.
@Test func aPitboardLinkCarriesOneEncodedLink() {
    let built = Handoff.url(
        opening: link("https://claude.ai/public/artifacts/0e5a?x=1&y=a%20b+c#frag"),
        scheme: "pitboard")
    #expect(
        built.absoluteString
            == "pitboard://open?url=https%3A%2F%2Fclaude.ai%2Fpublic%2Fartifacts%2F0e5a"
            + "%3Fx%3D1%26y%3Da%2520b%2Bc%23frag")
    #expect(
        Handoff.url(opening: link("https://claude.ai/"), scheme: "pitboard-debug").scheme
            == "pitboard-debug")
}

@Test(arguments: [
    "https://claude.ai/new",
    "https://claude.ai/public/artifacts/0e5a?x=1&y=a%20b+c#frag",
    "https://claude.ai/chat/x?q=100%25&r=a=b",
    "https://claude.ai/x?a='()!*'",
    "https://claude.ai/chat/%C3%BCber?q=%E6%97%A5",
    "https://claude.ai/x#one#two",
    "https://chatgpt.com/c/abc",
    "https://claude.ai/" + String(repeating: "a", count: 8100),
])
func handingALinkOverGivesTheSameLink(_ original: String) {
    let shared = link(original)
    let built = Handoff.url(opening: shared, scheme: "pitboard")
    #expect(carried(built.absoluteString) == .success(shared.url.absoluteString))
}

/// A link is read as Foundation reads it: a second `#` in a fragment is escaped, which is
/// the same page.
@Test func aLinkIsHandedOverAsFoundationReadsIt() {
    #expect(
        link("https://claude.ai/x#one#two").url.absoluteString
            == "https://claude.ai/x#one%23two")
}

/// An alias is handed over as the site's own host, which is what the window opens.
@Test func anAliasIsHandedOverAsTheSitesHost() {
    let built = Handoff.url(opening: link("https://chat.openai.com/c/x"), scheme: "pitboard")
    #expect(carried(built.absoluteString) == .success("https://chatgpt.com/c/x"))
}

/// Anything on the Mac can open a pitboard link, so one carrying a link no site opens is
/// refused for the same reason the link itself would be.
@Test func whatAPitboardLinkCarriesIsCheckedAsALinkFromOutside() {
    #expect(
        carried("pitboard://open?url=https%3A%2F%2Fexample.com%2F")
            == .failure(.notASite(host: "example.com")))
    #expect(
        carried("pitboard://open?url=https%3A%2F%2Fclaude.ai%2Fmagic-link%23a")
            == .failure(.signInLink(.claude)))
    #expect(
        carried("pitboard://open?url=" + String(repeating: "a", count: 8200))
            == .failure(.tooLong))
}

@Test func aLinkForAnotherSchemeOrRequestIsUnreadable() {
    let inside = "url=https%3A%2F%2Fclaude.ai%2F"
    for text in [
        "pitboard-debug://open?\(inside)", "https://open?\(inside)", "pitboard:open?\(inside)",
        "pitboard://someone@open?\(inside)", "pitboard://open:8080?\(inside)",
        "pitboard://close?\(inside)", "pitboard://open/x?\(inside)",
    ] {
        #expect(carried(text) == .failure(.unreadable), "\(text)")
    }
    #expect(carried("PITBOARD://OPEN/?\(inside)") == .success("https://claude.ai/"))
    #expect(
        carried("pitboard-debug://open?\(inside)", scheme: "pitboard-debug")
            == .success("https://claude.ai/"))
}

/// A link written by hand without encoding reads as a shorter link, another item and a
/// fragment. Opening the first part would open the wrong page without a word.
@Test(arguments: [
    "pitboard://open?url=https://claude.ai/x?a=1&b=2#c",
    "pitboard://open?url=https://claude.ai/x#c",
    "pitboard://open?url=a&url=b",
    "pitboard://open?url=https%3A%2F%2Fclaude.ai%2F&also=1",
    "pitboard://open?url=",
    "pitboard://open?link=https%3A%2F%2Fclaude.ai%2F",
    "pitboard://open?",
    "pitboard://open",
])
func anAmbiguousLinkIsUnreadable(_ text: String) {
    #expect(carried(text) == .failure(.unreadable))
}

@Test func theSchemeIsReadFromTheBundle() {
    #expect(Handoff.schemeKey == "PitboardURLScheme")
}

@Test func anExtensionFindsTheAppItIsIn() {
    let appex = URL(
        fileURLWithPath: "/Applications/Pitboard.app/Contents/PlugIns/PitboardShare.appex")
    #expect(Handoff.containingApp(of: appex)?.path == "/Applications/Pitboard.app")
    #expect(Handoff.containingApp(of: URL(fileURLWithPath: "/tmp/PitboardShare.appex")) == nil)
}
