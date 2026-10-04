namespace Pitboard.Core.Tests;

/// <summary>
/// The bindings against the library they were generated from. The first call checks the
/// library's contract version and every method's checksum, so a call that answers proves the
/// two agree: the same UniFFI made both, and the C# was generated from this library.
/// </summary>
[TestClass]
public sealed class BindingsTests
{
    private static readonly string[] Codes = ["claude", "codex"];
    private static readonly string[] Names = ["Claude Code", "Codex"];

    [TestMethod]
    public void TheBindingsLoadTheCoreAndAgreeWithIt()
    {
        var tools = PitboardFfiMethods.Tools();

        CollectionAssert.AreEqual(Codes, tools.Select(tool => tool.Code).ToArray());
        CollectionAssert.AreEqual(Names, tools.Select(tool => tool.Name).ToArray());
    }

    /// <summary>
    /// A sign-in is read by the core in its tool's own words, so the Windows app offers the
    /// address and the code field the macOS app does: the `https` address Codex prints after
    /// its loopback one, and no code, since Codex reads none.
    /// </summary>
    [TestMethod]
    public void ASignInIsReadAsTheCoreReadsIt()
    {
        const string said =
            "Starting local login server on http://localhost:1455.\n"
            + "If your browser did not open, navigate to this URL to authenticate:\n\n"
            + "https://auth.openai.com/oauth/authorize?state=x\n";

        var codex = PitboardFfiMethods.SignInView("codex", said, false);
        var claude = PitboardFfiMethods.SignInView("claude", "Paste code here if prompted > ", false);

        Assert.AreEqual(new SignInView("https://auth.openai.com/oauth/authorize?state=x", false), codex);
        Assert.IsTrue(claude.WantsCode);
    }
}
