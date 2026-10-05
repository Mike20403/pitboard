namespace Pitboard.Core.Tests;

/// <summary>
/// The app model's types, as the Windows app will hold them. One test makes a model and takes
/// its first snapshot without starting it: a model reads nothing until it is sent an intent,
/// and this one is given a fresh folder for every home it could read all the same. So the
/// launch, the listener, AppControl and the snapshot cross the library there. The library
/// calling a listener or an AppControl written in C# waits for a fixture's model, which can
/// be started with no machine to read. What the model does is the Rust tests' to prove.
///
/// Loading the library compares the checksum of every export, the model's constructor and
/// methods, the listener's one method and each of AppControl's among them, and hands the
/// library each trait's table of calls. The rest proves the records, the intents, the
/// listener and AppControl have the shape the app's code is written against.
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
            Installed: [PitboardFfiMethods.Tools()[0]], SwitchUnderWay: null, QuitQuestion: null,
            LastSwitches: [], Abandoned: null, Failure: null,
            WindowRequest: new WindowRequest(Serial: 0, Pane: null));
    }

    /// <summary>
    /// What an app's AppControl does: says what runs, asks an app to quit, opens one again,
    /// and throws what its system refuses as the one exception the model takes from it.
    /// </summary>
    private sealed class StandInApps : AppControl
    {
        public HashSet<string> Open { get; } = ["OpenAI.ChatGPT"];

        public List<string> Asked { get; } = [];

        public string? Running(string app) =>
            Open.Contains(app) ? $@"C:\Program Files\{app}\{app}.exe" : null;

        public void RequestQuit(string app)
        {
            if (!Open.Remove(app))
            {
                throw new PlatformException.Failed($"{app} is not running");
            }

            Asked.Add($"quit {app}");
        }

        public void Reopen(string location) => Asked.Add($"open {location}");
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
            var apps = new StandInApps();
            using var model = new PitboardModel(new AppLaunch(environment, null), keeping, apps);

            var first = model.Snapshot();
            model.Shutdown();

            Assert.AreEqual(0UL, first.Revision);
            Assert.IsFalse(first.Reading);
            Assert.IsNull(first.Status);
            Assert.IsNull(first.Installed);
            Assert.AreEqual(0UL, model.Snapshot().Revision);
            Assert.IsEmpty(keeping.Told);
            Assert.IsEmpty(apps.Asked);
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
    /// A switch is asked for by the account's label with its tool, and so is the answer that
    /// lets Pitboard quit the app the question names, which says which question it answers;
    /// what a switch said is put away by its tool, and keeping the app open and giving up
    /// carry nothing.
    /// </summary>
    [TestMethod]
    public void AnIntentToSwitchCarriesWhatItIsAbout()
    {
        Intent[] intents =
        [
            new Intent.SwitchTo(Qualified: "codex/work"), new Intent.QuitAndSwitch(Qualified: "codex/work"),
            new Intent.KeepAppOpen(), new Intent.DismissSwitch(Provider: "codex"), new Intent.AbandonStuckSwitch(),
            new Intent.DismissAbandoned(),
        ];

        Assert.AreEqual<Intent>(new Intent.SwitchTo("codex/work"), intents[0]);
        Assert.AreNotEqual<Intent>(new Intent.SwitchTo("claude/work"), intents[0]);
        Assert.AreEqual("codex", intents.OfType<Intent.DismissSwitch>().Single().Provider);
        Assert.AreEqual("codex/work", intents.OfType<Intent.QuitAndSwitch>().Single().Qualified);
        Assert.AreNotEqual<Intent>(new Intent.QuitAndSwitch("codex/spare"), intents[1]);
        Assert.AreNotEqual<Intent>(new Intent.SwitchTo("codex/work"), intents[1]);
        Assert.AreNotEqual<Intent>(new Intent.KeepAppOpen(), intents[1]);
    }

    /// <summary>
    /// A snapshot carries what a switch said as records of their own: the switch under way and
    /// the question about quitting the app that holds its login, what each tool's last switch
    /// said, what giving up kept, a numbered failure, and the window asked for on a pane.
    /// </summary>
    [TestMethod]
    public void ASnapshotCarriesWhatASwitchSaid()
    {
        var stillRunning = new Warning("sessions_still_running", "2 `codex` sessions are still running");
        var switched = new LastSwitch(
            Provider: "codex", To: "codex/work", FollowsAt: null,
            Restart: new RestartNeeded(Program: "codex", From: "personal"), Warnings: [stillRunning]);
        var snapshot = Read(4) with
        {
            SwitchUnderWay = "codex/spare",
            QuitQuestion = new QuitQuestion(Qualified: "codex/spare", AppId: "com.openai.codex", Name: "ChatGPT"),
            LastSwitches = [switched],
            Abandoned = new Abandoned(From: "personal", To: "work", LoginsKept: 2),
            Failure = new Failure(
                Id: 2, Title: "Couldn’t switch to spare", Message: "ChatGPT is still open, so nothing has changed.",
                Code: null, Warnings: []),
            WindowRequest = new WindowRequest(Serial: 3, Pane: Pane.Accounts),
        };

        Assert.AreEqual("ChatGPT", snapshot.QuitQuestion?.Name);
        Assert.AreEqual("personal", snapshot.LastSwitches[0].Restart?.From);
        Assert.IsNull(snapshot.LastSwitches[0].FollowsAt);
        Assert.AreEqual("sessions_still_running", snapshot.LastSwitches[0].Warnings[0].Code);
        Assert.AreEqual(2U, snapshot.Abandoned?.LoginsKept);
        Assert.AreEqual(2UL, snapshot.Failure?.Id);
        Assert.IsNull(snapshot.Failure?.Code);
        Assert.AreEqual(Pane.Accounts, snapshot.WindowRequest.Pane);
        Assert.AreEqual(new WindowRequest(3, Pane.Accounts), snapshot.WindowRequest);
        Assert.AreEqual(new RestartNeeded("codex", "personal"), switched.Restart);
    }

    /// <summary>
    /// AppControl is an interface the app implements over its system's own apps, by an app id
    /// of one string, and what its system refuses it throws as the one exception the model
    /// takes from it.
    /// </summary>
    [TestMethod]
    public void AppControlIsAnInterfaceTheAppImplements()
    {
        var stand = new StandInApps();
        AppControl apps = stand;

        var copy = apps.Running("OpenAI.ChatGPT");
        apps.RequestQuit("OpenAI.ChatGPT");
        apps.Reopen(copy!);
        var refused = Assert.ThrowsExactly<PlatformException.Failed>(() => apps.RequestQuit("OpenAI.ChatGPT"));

        Assert.AreEqual(@"C:\Program Files\OpenAI.ChatGPT\OpenAI.ChatGPT.exe", copy);
        Assert.IsNull(apps.Running("OpenAI.ChatGPT"));
        CollectionAssert.AreEqual(
            new[] { "quit OpenAI.ChatGPT", $"open {copy}" }, stand.Asked);
        Assert.AreEqual("OpenAI.ChatGPT is not running", refused.reason);
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
