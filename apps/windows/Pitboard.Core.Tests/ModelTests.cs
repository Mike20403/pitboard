namespace Pitboard.Core.Tests;

/// <summary>
/// The app model's types, as the Windows app will hold them. One test makes a model and takes
/// its first snapshot without starting it: a model reads nothing until it is sent an intent,
/// and this one is given a fresh folder for every home it could read all the same. So the
/// launch, the listener and the snapshot cross the library there. The library calling a
/// listener written in C# waits for a fixture's model, which can be started with no machine
/// to read. What the model does is the Rust tests' to prove.
///
/// Loading the library compares the checksum of every export, the model's constructor and
/// methods and the listener's one method among them, and hands the library the listener's
/// table of calls. The rest proves the records, the intents and the listener have the shape
/// the app's code is written against.
/// </summary>
[TestClass]
public sealed class ModelTests
{
    private const long At = 1_800_000_000;

    /// <summary>
    /// What an app's listener does: keeps each snapshot it is told of, and refuses one it
    /// cannot take in the one way the model takes a refusal.
    /// </summary>
    private sealed class Keeping : ModelListener
    {
        public List<Snapshot> Told { get; } = [];

        public void Changed(Snapshot snapshot)
        {
            if (snapshot.Revision == 0)
            {
                throw new PlatformException.Failed("the first snapshot is the app's to take");
            }

            Told.Add(snapshot);
        }
    }

    private static Snapshot Read(ulong revision)
    {
        var work = new Account(
            Id: "claude:work", Provider: "claude", Label: "work", Qualified: "claude/work",
            Unplaced: false, Email: "work@example.com", AccountUuid: "work", SignedIn: true,
            Switchable: false, Parked: null,
            Usage: new Usage(
                Source.Live, At,
                [new Limit("session", 18_000, null, 42.0, At + 3_600, null, true)]),
            Stale: null, StaleExplanation: null, LastsSeconds: null, LastsBurning: false);
        return new Snapshot(
            Revision: revision, Now: At, Reading: false, UpdatedAt: At,
            Status: new Status(At, [work], []), Warnings: [], ReadFailure: null, Stuck: false,
            Installed: [PitboardFfiMethods.Tools()[0]]);
    }

    [TestMethod]
    public void TheBindingsAgreeWithTheLibraryOnTheModel()
    {
        // The first call loads the library: every checksum is compared, and the listener's
        // calls are registered, before it answers.
        Assert.AreEqual("claude", PitboardFfiMethods.Tools()[0].Code);
    }

    /// <summary>
    /// A model made from what the app was started with answers its first snapshot at once,
    /// before anything is read, and stops when told to. Every home it could read is a folder
    /// of its own, and it is never started, so nothing on this machine is read.
    /// </summary>
    [TestMethod]
    public void AModelAnswersItsFirstSnapshotBeforeItReadsAnything()
    {
        var home = Directory.CreateTempSubdirectory("pitboard-model-").FullName;
        try
        {
            var environment = new Dictionary<string, string>
            {
                ["HOME"] = home,
                ["USERPROFILE"] = home,
                ["CODEX_HOME"] = Path.Combine(home, ".codex"),
                ["CLAUDE_CONFIG_DIR"] = Path.Combine(home, ".claude"),
                ["PITBOARD_HOME"] = Path.Combine(home, ".pitboard"),
            };
            var keeping = new Keeping();
            using var model = new PitboardModel(new AppLaunch(environment, null), keeping);

            var first = model.Snapshot();
            model.Shutdown();

            Assert.AreEqual(0UL, first.Revision);
            Assert.IsFalse(first.Reading);
            Assert.IsNull(first.Status);
            Assert.IsNull(first.Installed);
            Assert.AreEqual(0UL, model.Snapshot().Revision);
            Assert.IsEmpty(keeping.Told);
            Assert.IsEmpty(Directory.EnumerateFileSystemEntries(home));
        }
        finally
        {
            Directory.Delete(home, recursive: true);
        }
    }

    /// <summary>
    /// An intent is a variant an app sends, with what it carries, and two of the same are
    /// equal.
    /// </summary>
    [TestMethod]
    public void AnIntentIsAVariantWithWhatItCarries()
    {
        Intent[] intents = [new Intent.Start(), new Intent.Woke(), new Intent.Glanced(), new Intent.Refresh(Asked: true)];

        Assert.AreEqual<Intent>(new Intent.Refresh(true), intents[3]);
        Assert.AreNotEqual<Intent>(new Intent.Refresh(false), intents[3]);
        Assert.IsTrue(intents.OfType<Intent.Refresh>().Single().Asked);
        Assert.AreEqual(1, intents.OfType<Intent.Start>().Count());
    }

    /// <summary>
    /// A snapshot carries the accounts as the core's records do, and what went wrong as a
    /// record of its own. Its lists are arrays, which a C# record compares by reference: two
    /// snapshots read alike are not equal, so the app goes by the revision and syncs each list
    /// by its rows' ids.
    /// </summary>
    [TestMethod]
    public void ASnapshotCarriesWhatWasRead()
    {
        var snapshot = Read(3);
        var failed = snapshot with { ReadFailure = new ReadFailure("unreachable", "Anthropic could not be reached") };

        Assert.AreEqual("work", snapshot.Status?.Accounts[0].Label);
        Assert.AreEqual(42.0, snapshot.Status?.Accounts[0].Usage?.Windows[0].Percent);
        Assert.AreEqual("Claude Code", snapshot.Installed?[0].Name);
        Assert.AreEqual("unreachable", failed.ReadFailure?.Code);
        Assert.AreEqual(snapshot.Revision, failed.Revision);
        Assert.AreNotEqual(Read(3), Read(3));
    }

    /// <summary>
    /// The listener is an interface the app implements, and what it cannot take in it throws
    /// as the one exception the model takes from it.
    /// </summary>
    [TestMethod]
    public void AListenerIsAnInterfaceTheAppImplements()
    {
        var keeping = new Keeping();
        ModelListener listener = keeping;

        listener.Changed(Read(1));
        var refused = Assert.ThrowsExactly<PlatformException.Failed>(() => listener.Changed(Read(0)));

        Assert.AreEqual(1UL, keeping.Told.Single().Revision);
        Assert.AreEqual("the first snapshot is the app's to take", refused.reason);
    }

    /// <summary>
    /// What the app was started with goes in as it was given: the environment, and the folder
    /// the app is in.
    /// </summary>
    [TestMethod]
    public void TheAppsLaunchIsItsEnvironmentAndWhereItIs()
    {
        var launch = new AppLaunch(
            new Dictionary<string, string> { ["USERPROFILE"] = @"C:\Users\x" }, @"C:\Program Files\Pitboard");

        Assert.AreEqual(@"C:\Users\x", launch.Environment["USERPROFILE"]);
        Assert.AreEqual(@"C:\Program Files\Pitboard", launch.AppLocation);
    }
}
