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
}
