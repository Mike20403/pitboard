namespace Pitboard.Core.Tests;

/// <summary>
/// The core's sentences about limits, logins and checks, reached from C# as a Windows app
/// would reach them. What each one says is the core's to test; this proves the free
/// functions take the records and give back the core's sentences.
/// </summary>
[TestClass]
public sealed class WordsTests
{
    private static Limit Limit(string kind, long? lengthSeconds, string? scope) =>
        new(kind, lengthSeconds, scope, 42, null, null, true);

    [TestMethod]
    public void ALimitIsNamedAsTheCommandLineNamesIt()
    {
        var scoped = Limit("weekly_scoped", 604_800, "Fable");
        Assert.AreEqual("week · Fable", PitboardFfiMethods.LimitColumn(scoped));
        Assert.AreEqual("90m", PitboardFfiMethods.LimitColumn(Limit("90_minute", 5_400, null)));
        Assert.AreEqual("5-hour", PitboardFfiMethods.LimitName(Limit("session", null, null)));
        Assert.AreEqual(UsageLevel.Low, PitboardFfiMethods.UsageLevel(70));
    }

    [TestMethod]
    public void TimeIsSaidAsTheCommandLineSaysIt()
    {
        Assert.AreEqual("resets in 1h 05m", PitboardFfiMethods.Resets(1_000 + 3_900, 1_000));
        Assert.AreEqual("resetting now", PitboardFfiMethods.Resets(1_000, 1_000));
        Assert.AreEqual("about 1h 30m left at this rate", PitboardFfiMethods.Runway(5_400, true));
        Assert.AreEqual("about to run out", PitboardFfiMethods.Runway(30, true));
        Assert.IsNull(PitboardFfiMethods.Runway(null, true));
        Assert.IsTrue(PitboardFfiMethods.SameReset(between: 1_000, and: 1_059));
        Assert.IsFalse(PitboardFfiMethods.SameReset(between: 1_000, and: 1_060));
    }

    [TestMethod]
    public void ChecksAreSummedUp()
    {
        Check[] checks = [new("keychain", "Keychain", Level.Warn, "locked", "Unlock it.")];
        Assert.AreEqual("One thing is worth looking at.", PitboardFfiMethods.DoctorSummary(checks));
        Check[] broken = [.. checks, new("credential", "Credential", Level.Fail, "unreadable", "Sign in.")];
        Assert.AreEqual("1 broken: do not switch accounts until fixed.", PitboardFfiMethods.DoctorSummary(broken));
    }
}
