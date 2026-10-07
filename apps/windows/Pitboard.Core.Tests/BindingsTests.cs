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

    [TestMethod]
    public void TheBindingsLoadTheCoreAndAgreeWithIt()
    {
        var sites = PitboardFfiMethods.Sites();

        CollectionAssert.AreEqual(Codes, sites.Select(site => site.Provider).ToArray());
    }

    /// <summary>
    /// An app reaches the core through one object, the model, whose calls wait on nothing.
    /// The objects the macOS app called before it ran on the model, whose calls waited on the
    /// keychain, the network and the person's login shell, are not offered, nor what only
    /// they took, answered or threw, nor the two account windows' calls whose answers the
    /// snapshot carries now. Nor are the free functions no app called, whose answers the
    /// snapshot carries too, such as a sign-in's address, a limit's names or doctor's summary,
    /// nor what only they took or answered. uniffi-bindgen-cs writes an exported object as a
    /// class with an interface of its own, named `I` and the class's name.
    /// </summary>
    [TestMethod]
    public void TheBindingsOfferTheModelAndNothingOlder()
    {
        var exported = typeof(PitboardModel).Assembly.GetExportedTypes().Where(type => !type.IsNested);
        var objects = exported
            .Where(type => type.IsClass && type.GetInterface("I" + type.Name) is not null)
            .Select(type => type.Name)
            .Order(StringComparer.Ordinal);
        string[] older =
        [
            "Pitboard", "SignIn", "Settings", "PitboardException", "Cause", "Switched", "Switch",
            "Adoption", "Enrolled", "EnrolledAs", "Changed", "Holding", "Remedy", "Diagnosis", "Change",
            "SignInView", "Check", "Renewed",
        ];
        var stillOffered = exported.Select(type => type.Name).Intersect(older).Order(StringComparer.Ordinal);
        string[] olderCalls =
        [
            "WindowsOf", "ForgetMessage", "Tools", "CommandLinePlaces", "SignInView", "FindCommandLine",
            "HomeDirectory", "LimitColumn", "LimitName", "UsageLevel", "Resets", "Runway", "ParkedLife",
            "RenewalNote", "DoctorSummary", "SameReset",
        ];
        var stillCalled = typeof(PitboardFfiMethods).GetMethods()
            .Select(method => method.Name)
            .Intersect(olderCalls)
            .Order(StringComparer.Ordinal);

        Assert.AreEqual(nameof(PitboardModel), string.Join(", ", objects));
        Assert.AreEqual("", string.Join(", ", stillOffered));
        Assert.AreEqual("", string.Join(", ", stillCalled));
    }

    /// <summary>
    /// Where Pitboard's directory is, which command line an app comes with, and whether a path
    /// is a program, are the core's to say, so the app has no rule of its own for any of them:
    /// what the app was given goes in as it was given, and the answer comes out as the core
    /// reads it.
    /// </summary>
    [TestMethod]
    public void TheAppAsksTheCoreWhereThingsAreAndWhatRuns()
    {
        var environment = new Dictionary<string, string> { ["HOME"] = "/home/x" };

        Assert.AreEqual("/home/x/.pitboard", PitboardFfiMethods.PitboardDirectory(environment));
        Assert.IsNull(PitboardFfiMethods.AppCommandLine("/home/x/pitboard"));
        Assert.IsFalse(PitboardFfiMethods.CanRun("/nowhere/at/all"));
    }
}
