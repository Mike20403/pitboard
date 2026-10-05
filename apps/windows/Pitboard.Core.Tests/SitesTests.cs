namespace Pitboard.Core.Tests;

/// <summary>
/// The sites and the links from outside, reached from C# as the Windows app will reach them.
/// A link a person shares, or the one the pitboard:// handler is given on its command line,
/// is read by the core's rule, the one the macOS app and its Share extension read it by. What
/// the rule says is the core's to test; this proves the records and refusals cross.
/// </summary>
[TestClass]
public sealed class SitesTests
{
    private static readonly string[] Hosts = ["claude.ai", "chatgpt.com"];

    [TestMethod]
    public void TheSitesAreTheCoresInTheirOrder()
    {
        var sites = PitboardFfiMethods.Sites();

        CollectionAssert.AreEqual(Hosts, sites.Select(site => site.Host).ToArray());
        Assert.AreEqual("codex", sites[1].Provider);
        Assert.AreEqual("codex", sites[1].StoreName);
        Assert.AreEqual("chatgpt.com", PitboardFfiMethods.SitesFor("codex").Single().Name);
        Assert.IsEmpty(PitboardFfiMethods.SitesFor("gemini"));
        Assert.AreEqual("claude.ai or chatgpt.com", PitboardFfiMethods.SiteNames(Conjunction.Or));
    }

    /// <summary>
    /// A Pitboard link written for a link reads back as the link on its site's own host, as
    /// the macOS app reads one its Share extension wrote.
    /// </summary>
    [TestMethod]
    public void APitboardLinkReadsBackAsTheLinkItCarries()
    {
        var written = PitboardFfiMethods.PitboardLink(" chat.openai.com/c/x?y=1#z ", "pitboard");
        var link = PitboardFfiMethods.ReadPitboardLink(written, "pitboard");

        Assert.AreEqual("pitboard://open?url=https%3A%2F%2Fchatgpt.com%2Fc%2Fx%3Fy%3D1%23z", written);
        Assert.AreEqual("https://chatgpt.com/c/x?y=1#z", link.Url);
        Assert.AreEqual("chatgpt.com", link.Site.Host);
    }

    /// <summary>
    /// A refusal is thrown as its own kind, and said in the sentence both apps show.
    /// </summary>
    [TestMethod]
    public void ARefusalSaysWhy()
    {
        var signIn = Assert.ThrowsExactly<LinkRefusal.SignInLink>(
            () => PitboardFfiMethods.SiteLink("https://claude.ai/magic-link#a"));

        Assert.AreEqual("claude.ai", signIn.host);
        Assert.AreEqual(
            "Pitboard doesn’t open claude.ai sign-in links from outside: one would sign the "
            + "window in as whoever the link belongs to. Sign in inside the account’s claude.ai "
            + "window.",
            PitboardFfiMethods.LinkRefusalReason(signIn));
        Assert.ThrowsExactly<LinkRefusal.Unreadable>(
            () => PitboardFfiMethods.ReadPitboardLink("pitboard://open?url=a&url=b", "pitboard"));
    }
}
