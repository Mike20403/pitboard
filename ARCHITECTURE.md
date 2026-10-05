# Architecture

This file describes how Pitboard's code is organised and what must stay true in it. It
also holds the measured facts the design rests on, about macOS, WebKit, Claude Code,
OpenAI's Codex CLI and the sites the app's account windows open. It changes when the layout
changes or a tool build moves a fact, not with each commit.

## Bird's eye view

Pitboard switches Claude Code or Codex between a person's own accounts on one machine. It
also shows how much of each account's limits is left. A switch parks the login in use and
puts another account's parked login in its place.

One crate, `pitboard-core`, does this for both tools. It reads and writes each tool's login,
keeps Pitboard's index of accounts, and asks each tool's service for usage.

Two front ends use the core: the command line, `pitboard`, on macOS and Linux, and the menu
bar app on macOS 14 or later. The app calls the core through UniFFI bindings. The command
line inside the app is what the app's daily renewal runs, and what the `pitboard-app` cask
puts on `PATH`.

The app also gives each enrolled account a window on its tool's site, claude.ai or
chatgpt.com, where the site's own pages run in WebKit. The windows take the list of
accounts from the core, and nothing else. [Account windows](#account-windows) describes
them.

Pitboard has no server of its own. The core sends requests only to Anthropic, for Claude
Code, and to OpenAI, for Codex. An account's window loads its site, and whatever the site's
pages load, as a browser would.

## Code map

- `crates/pitboard-core`: the engine. Parking, switching, recovery, the stores and usage.
  The front ends reach it through `service::Pitboard`, built with a `context::Context`.
  - `provider/`: one module per tool, `claude` and `codex`, each implementing the
    `Provider` trait in `provider/mod.rs`. The trait covers where the tool keeps its login,
    whose it is, how to renew it, what it has left and what its sign-in prints, which
    `provider::sign_in_view` reads for both apps. Each module's `assumptions.rs` is
    that tool's register of facts. `provider/codex/holders.rs` names where a running
    `codex` can be, and what makes each take a switch. `provider/printed.rs` reads what a
    tool printed as a terminal does: the text it shows, and where each hyperlink goes.
  - `holder.rs`: what keeps a tool's login in memory while it runs, told apart by where
    its program runs from. A switch's warning, `doctor` and the app's offer to quit an app
    all read it, so they cannot disagree.
  - `host/`: the machine, behind one seam. The `Host` trait is what a test replaces: the
    system's store of secrets, files, the vault, this user's processes and the scheduler,
    reached through `Context` and faked by `host/memory.rs`. `fs`, `proc` and `user` are
    plain functions for what the system does whoever asks: private files and directories,
    whether a process is alive, the login name, and the `PATH` the person's login shell
    builds, which `unix/shell.rs` asks for. `program.rs` finds a program the way the
    system's launcher does. `mod.rs` chooses the system, once: `macos/` (the keychain
    through `security`, `ps`, launchd) or `linux/` (`/proc`, systemd), each with what
    `unix/` holds for both. A fact that differs by system is a `match` on `host::OS`, such
    as the folders macOS asks about before an app may look in them.
  - `store/`: reading and writing logins, whichever store holds them: the chain rules, a
    file, the vault of files and the stores in memory the tests use. On macOS, parked
    logins are keychain items. On Linux, they are files in the vault.
  - `switch/`: every change to Pitboard's index (switching, enrolling, adopting, renaming,
    forgetting, renewing, repairing, abandoning and uninstalling), and the journal that
    finishes an interrupted switch.
  - `state.rs`: `state.json`, the index of accounts and where each one's login is parked.
  - `lock.rs`: the lock Claude Code takes around credential writes, taken the same way.
  - `context.rs`: what the core takes from its environment, read from a map of variables
    by the same code for every front end, apart from the `PATH` that `host/linux` reads to
    find the program daily renewal runs.
  - `app.rs`: what an app finds for itself that a command typed at a prompt is given: each
    tool's program, on the login shell's `PATH` and then where the tool's installers put
    it, and the `pitboard` a terminal would run. And the files an app keeps of its own in
    Pitboard's directory, `told.json` and `app.json`, written as the core writes its own and
    read by nothing of the core.
  - `api.rs`: the requests to Anthropic. The requests to OpenAI are in
    `provider/codex/api.rs`.
  - `status.rs`, `doctor.rs`, `statusline.rs` and `schedule.rs` serve the commands of the
    same names. `schedule.rs` decides what daily renewal runs and whose it is; the host's
    scheduler writes it.
  - `words.rs`: the sentences and column words Pitboard says in more than one place, each
    a function of typed values: spans of time, a limit's names, when it resets, how long an
    account lasts, a parked login's life, a renewal run and doctor's summary. It also holds
    `usage_level`, the steps at which a limit's colour changes. A thing said both in a
    column and in a sentence has a function for each form. The command line calls these
    functions directly, and the macOS app calls the ones it shows through `pitboard-ffi`.
    Clock times are not in it.
- `crates/pitboard`: the command line. Arguments, rendering for people, the man page, and
  the `--json` contract, pinned by the snapshots in `crates/pitboard/tests/snapshots`.
- `crates/pitboard-ffi`: the core as UniFFI bindings, for the apps: a static library for
  the macOS app, a dynamic one for the Windows app.
  - `sites.rs` gives both apps `pitboard-sites`' sites and links as records of their own,
    and says a site's sign-in steps as a window on this system can follow them: a window on
    a Mac cannot use a passkey.
  - `account_windows/` holds the rules of an account's window that each app's web code
    asks as its engine asks it: which accounts have a window and the store each one's data
    is kept in (`stores.rs`, `accounts.rs`), where a page may go and what becomes of a
    response (`policy.rs`), what a window says (`notes.rs`), what a page may do and how its
    dialogs say who asks (`pages.rs`), and what a download is called (`downloads.rs`). What
    differs by system there, such as how a copy of a file is numbered, which names are one
    file, or an alert's "on this Mac", is a `match` on `host::OS`, as what a window cannot
    sign in with is in `sites.rs`.
  - `model/` is the app model both apps are to show, which neither uses yet: the Swift model
    in `PitboardApp` still decides what the macOS app shows. An app makes a `PitboardModel`,
    sends it an `Intent` for each thing asked of it, and its `ModelListener` is told of each
    numbered `Snapshot`; its `AppControl` quits and opens other apps, and its
    `Notifications` posts what has run out. `state.rs` holds what the model knows and
    decides what follows each message, `advice.rs` which account to offer once the one in
    use has run out, and `preferences.rs` what the app's own preferences are; `lanes.rs`
    runs what it decides, on a lane of reads, a lane of changes, one at a time, a lane that
    lists processes and asks the app's `AppControl` about other apps, a lane that asks what
    is installed, a thread of its own for each sign-in, a lane that types a code back to one
    or stops it, a lane that reads and writes what the model keeps in Pitboard's directory
    and one that posts through the app's `Notifications`, and tells the listener on a thread
    of its own; `mod.rs` holds the exported types and the actor thread that owns the state.
    So far the model reads the accounts, looks every two seconds for a change made
    elsewhere, asks which tools are installed, switches, quits the app holding a tool's
    login when the person lets it, gives up on a stuck switch, keeps what each tool's last
    switch said, runs each tool's own sign-in, enrols the login signed in now, renames and
    forgets, keeps the sheet over the main window, says which account to switch to once the
    one in use has run out, notified once for each reset, and keeps the app's own
    preferences. Its tests are files of their own there: `reading.rs`, `switching.rs`,
    `signing.rs`, `changing.rs`, `advising.rs`, `keeping.rs`, `presenting.rs` and
    `cadence.rs` drive the state by hand, `lanes.rs` has the lanes' own, and `threaded.rs`
    drives the model through its threads over the real core.
  - `present/` makes each `Snapshot` from the model's state: `present` takes the state and
    the moment, and builds every sentence and row the menu bar, the menu and the window
    show, so a view decides nothing. `accounts.rs` is the menu bar's words and the
    accounts' sections and rows with their limits, `notices.rs` the notices, their order
    and what the menu says of them, `setup.rs` the footing, the step it asks for and what
    stands in for an empty list, `sheets.rs` the sheets, the quit question and a failure's
    alert, with `name_to_save`, the rule a sheet's Save and the model both save by, and
    `words.rs` the sentences both apps say and the command line does not, each a function
    of typed values. What the command line says too is `pitboard_core::words`', called from
    there. A clock time is the person's to read, so it is asked of the app's `LocalTime`.
    A button the snapshot offers comes with its words beside the intent it sends, so a view
    never words an intent; a control each app always has, such as its toolbar's "Add
    Account…" or "Quit Pitboard", and what is about the app's own system stay the app's.
- `crates/pitboard-sites`: the sites an account's window opens, and what a link from outside
  may be. A leaf, with no I/O and nothing of the core, whose one dependency is `url`, for
  IDNA alone.
  - `site.rs` declares each site as values: its host, the tool whose accounts it serves and
    the hosts that redirect to it. The hosts its sign-in goes to, the hosts it blocks, its
    sign-in paths, its store name, its sign-in steps and whether its sign-in offers a
    passkey are values too. Nothing else names a site, so a site is added to `ALL`, with its
    fixture page and tests.
  - `link.rs` checks a link from outside: a site's own host or alias, over `https`, with no
    port or user information, and never a sign-in path. `LinkRefusal` says why one is
    refused, in the sentence every front end shows.
  - `handoff.rs` writes and reads the Pitboard link, `<scheme>://open?url=<link>`.
  - `address.rs` splits a link as Foundation's `URLComponents` does, which is how the macOS
    app read one before the rule was Rust. [Foundation's URLs](#foundations-urls) has what
    was measured.
  - `web.rs` reads a page's address, for an account window's rules, as Foundation's `URL`
    does: its scheme, and the host, port and user it names, with the host a request goes
    to.
- `crates/pitboard-share-ffi`: `pitboard-sites` as UniFFI bindings for the macOS Share
  extension alone, a static library with one function, `share_link`. It checks a shared
  page and writes the Pitboard link that hands it to the app, or says why not in the
  sentence the app shows. It holds nothing of the core.
- `crates/uniffi-bindgen-swift` and `crates/uniffi-bindgen-csharp`: generate the Swift and
  the C# bindings with exactly the UniFFI version the library uses. The bindings check
  method checksums when they load. The C# generator is NordSecurity's, pinned to a release
  built against that UniFFI; it is a build tool, so `deny.toml` leaves it out of the graph.
  Every type the core's bindings export is declared in `pitboard-ffi`, because the C#
  generator cannot use a type from another crate. `pitboard-share-ffi`, which only Swift
  reads, declares its own.
- `crates/pitboard-conformance`: checks a tool's register against a build of that tool.
- `apps/`: the native apps, one directory for each system.
- `apps/windows/`: the Windows app. `Pitboard.Core` is the core's C# bindings as an
  assembly of their own, generated into `Generated/` and not committed, and
  `Pitboard.Core.Tests` calls the core through them.
- `apps/macos/`: the menu bar app. The Swift package holds it as libraries its tests load
  without starting it. `PitboardKit` calls the bindings off the main thread,
  `PitboardShareBindings` is `pitboard-share-ffi`'s, for the Share extension,
  `PitboardLinkTarget` is where a Pitboard link goes, which the app and the extension both
  link, and `PitboardApp` is everything the app does. The account windows are mapped under
  [Account windows](#account-windows).
  - `project.yml` is the app itself, the spec XcodeGen generates `Pitboard.xcodeproj`
    from. Its `Pitboard` target in `App` starts `PitboardApp` and adds Sparkle, and
    `PitboardUITests` in `UITests` drives it. `PitboardShare`, from `ShareExtension`, is the
    Share extension the app embeds. Only the project's `Package.resolved` is committed,
    which pins Sparkle's revision.
  - A renewal schedule written by an app up to 0.3.0 starts the app with `renew`.
    `App/Main.swift` then replaces the process with the command line inside the app.
  - `scripts/build-xcframework.sh` builds the core, as `PitboardFFI.xcframework`, and
    `pitboard-share-ffi`, as `PitboardShareFFI.xcframework`, each with its Swift bindings,
    for both Mac architectures. `scripts/build-app.sh` generates the project and builds
    `Pitboard.app` with `xcodebuild`, with the command line inside at
    `Contents/Helpers/pitboard`.
- `packaging/`: the files a release writes into the tap `datlechin/homebrew-tap`. They are
  the casks `pitboard.rb` for the command line and `pitboard-app.rb` for the app,
  `tap_migrations.json`, and the tap's README.
- `docs/`: the Mintlify source of docs.usepitboard.com.
- `website/`: reserved for the usepitboard.com site, to be built with Astro; empty.
- `.github/`:
  - `workflows/ci.yml` checks every push to `main` and every pull request, and
    `workflows/release.yml` turns a `v` tag into a release.
  - `workflows/conformance.yml` checks each tool's newest build against its register.
  - `workflows/sparkle.yml` opens an issue when Sparkle has a release newer than the one
    `apps/macos/project.yml` pins, because Dependabot cannot read that pin.
  - `actions/xcodegen` puts the pinned XcodeGen on `PATH` for every job that builds the
    app.
  - `workflows/rotation.yml` rehearses rotating the update key.
  - `actions/apple-keychain` imports the Developer ID certificate for every job that signs.
  - `scripts/` holds the EdDSA key and signature helpers, and the scripts that add Sparkle,
    the command line and the Share extension's library to the app's bill of materials.
  - `dependabot.yml` asks for weekly updates of Cargo dependencies and GitHub Actions.

## Invariants

- The core prints nothing.
- The core reads its environment in `Context::read`, from a map of variables, the same way
  for every front end. The command line passes its own through `Context::from_env`. An app
  passes the one it was started with through `AppContext::discover`, which also asks the
  person's login shell for `PATH`, because an app the system started has none of a
  shell's.
- Three variables are also read straight from the process. `PATH` is read when a context
  made with `Context::new` was given no search path (`context.rs`), and on Linux to find
  the program daily renewal runs (`host/linux`). `NO_COLOR` is read by the status line
  (`pitboard/src/main.rs`). `XPC_SERVICE_NAME`, which launchd sets, is read in
  `host/macos/launchd.rs`. `clippy.toml` refuses `std::env::var`, `var_os`, `vars` and
  `vars_os` outside `context.rs`, so every other read carries an `#[allow]` that says why
  it is meant.
- `READ` in `context.rs` and `settings::OVERRIDING_ENV` name every variable Pitboard
  reads, and `Environment` refuses, in a build with debug assertions, to read one they do
  not name. The integration tests withhold every one of them from each command they run,
  except `HOME` and `USER`, and the core's unit tests make their context with
  `Context::for_unit_test`, which withholds all of them, so a variable exported where
  `cargo test` runs, such as `PITBOARD_CLAUDE`, never reaches a test.
- Which system Pitboard runs on is decided in `host/mod.rs` and nowhere else. Anything
  that differs by system is either the host's to answer or a `match` on `host::OS`, so a
  system added to `host::Os` does not compile until it is said for every one.
- No test starts the person's own login shell. A test names a shell of its own in `SHELL`,
  one that is not there, or hands in what a shell said.
- A test never reaches the system's own scheduler. A test context schedules through
  `MemoryHost`, which writes the files in the test's home and asks no service manager, and
  the real hosts refuse to ask launchd or systemd from a unit test at all.
- `pitboard-ffi` exports records, enums, three error types, free functions, three objects,
  `SignIn`, `Pitboard` and `PitboardModel`, and four traits an app implements,
  `ModelListener`, `AppControl`, `Notifications` and `LocalTime`. Nothing it exports is
  async. A call to
  `SignIn` or `Pitboard` is synchronous and may block on the keychain, a lock, the network
  or the person's login shell, and `PitboardKit` makes each off the main thread; making a
  `Pitboard` blocks on none of them, since it reads its environment on first use.
  `PitboardModel` blocks on none of them anywhere: making one starts its threads, `send`
  only posts an intent, `snapshot` only copies the last snapshot, `shutdown` waits only for
  the actor to take what is already in its mailbox, and the core, a tool's sign-in and the
  app's `AppControl`, `Notifications` and `LocalTime` are called on the model's own threads.
  The free
  functions block on nothing, and the app makes them where it likes: `tools`,
  `sign_in_view`, `name_to_save`, the rule `same_reset` from `usage.rs`, and `usage_level`
  and the sentences and column words of `words.rs` the apps show, which read only what they
  are given; `sites`, `sites_for`, `site_names`, `site_link`, `read_pitboard_link`,
  `pitboard_link` and `link_refusal_reason`, which are `pitboard-sites`' and read only what
  they are given too; the account windows' rules, such as `store_id`, `window_accounts`,
  `decide_navigation` and `window_note`, which read only what they are given as well, so a
  web view's delegate asks them as it is asked; `download_destination`, which asks the file
  system whether each name it tries is taken; `command_line_places` and `app_command_line`,
  which only join paths; `can_run`, which asks the file system about one path; and
  `home_directory` and `pitboard_directory`, which read the environment they are given and,
  without `HOME`, this account's passwd entry. `find_command_line` looks along a search
  path, so the app makes it off the main thread.
- What a snapshot says is made by `present`, which reads the state and the moment and asks
  nothing of anyone but the app's `LocalTime`, for each clock time and whether a moment is
  on another day than now. Where that cannot say, the time is said in UTC and named so,
  from the moments alone, so the machine's own time zone is read by the app's `LocalTime`
  and nothing else. The actor makes it before it takes the lock `snapshot` takes,
  so a `LocalTime` may call the model back. Text that depends on the time alone is made
  again on the minute tick, once started, and a countdown is the app's own, from
  `PanelNotice::until`.
- A run-out is notified once for each reset of its limit, by the core's rule for one reset,
  across launches: what was notified is kept in `told.json` in Pitboard's directory, read
  as the model starts. Nothing is advised on until it is in, and what was read meanwhile is
  advised on then, so a read that lands first never notifies again what an earlier launch
  did; what stands in for a read that failed, the last numbers measured, is not, as the
  Swift model's fallback was not. A record that is there and cannot be read is nothing
  told, and is written whole over it the next time something is told, so at worst a run-out
  is notified once more. The window's advice is worked out as the Swift model worked it
  out, from what this launch has told, so it still says a run-out notified before a
  relaunch.
- The app's own preferences, the tools somebody said "Not Now" to a second account for and
  whether the app has ever shown anybody anything, are the model's, kept in `app.json` in
  Pitboard's directory, so they follow `PITBOARD_HOME`. Where that file is there it wins;
  where it is not, the model takes what the app's earlier store held, handed over in
  `AppLaunch::earlier_preferences`, and keeps it at once, so that store is read once. The
  model writes them only once they are read, from the file or, where there is none, from
  that store, and never in their place: a file that is there and cannot be read, because it
  is not this user's, a disk failed, another program holds it or it is not text, is left as
  it is for as long as the app is open, a "Not Now" said meanwhile holding until it quits,
  and is not taken for a first launch. Text that does not read as preferences is taken as
  no file, and what that store held is written over it. Until they are read no tool is
  nudged toward a second account, so a read that lands first never shows the step to
  somebody who declined it. No foreign trait reaches the app's own store for them.
- The app model's state belongs to its actor thread alone, and `State::apply` does no I/O:
  it calls neither the core nor the app, reads no clock and waits on nothing. It says what
  to run as jobs, which run on the model's lanes and answer as messages. A call the Swift
  model awaited is a job, except that a read's two stamps and the read are one job, and a
  look's two stamps are one, so no look lands between a read's stamps and the read. A read
  that started before a change, one the poll noticed or one this app made, counted in
  `changes_seen` when the read's job is made and compared when it lands, is dropped, and a
  read records when the index and the readings were written as they stood before it.
- A switch is claimed inside `State::apply`, as its intent is taken and before its first
  job is queued, and stays claimed until the read after it has landed, so one switch runs
  at a time and the poll leaves the account index alone meanwhile. The switch runs on the
  lane of changes, and what holds the login and quitting an app on the lane of processes,
  so a read answers while either waits. An app is quit only when the person says so, the
  way a person quits it and never by force, and is given 30 seconds, asked every 200
  milliseconds whether it still runs; one that has not quit by then stops the switch before
  anything changes. An `AppControl` that throws says the app still runs, so nothing is
  switched under an app nobody saw go. Pitboard opens again only the copy it quit, as soon
  as the switch is over, whether or not it worked. `Intent::QuitAndSwitch` names the
  account of the question it answers, as `AppModel.quitAndSwitch` took the question, so an
  app may close the question with `Intent::KeepAppOpen` before or after it sends the answer:
  a question closed unanswered is kept until another switch is asked for, and an answer is
  taken once.
- What a tool's last switch said is kept apart from the read's warnings, one per tool, until
  that tool no longer has the account it switched to signed in or the person puts it away:
  a Codex switch's warning that open sessions still use the account it parked, and must not
  sign out, outlives every read and every write that leaves that account signed in. A
  sign-in that puts a new login in use for the account in use is kept there too, beside
  what that tool's last switch said, since sessions already running keep the old login.
- One sign-in runs at a time, told apart by an id, on a thread of its own that starts the
  tool's own sign-in, hands on what the tool says and, once the tool has stopped saying
  anything, enrols what it signed in to only if it is still the sign-in under way. So a
  cancel comes before the tool has started, and stops it as it starts; while it waits on a
  browser, and stops it then; or once it is being enrolled, too late to stop, and the
  accounts are read. A code is typed back, and a tool stopped, on a lane of their own, never
  on the sign-in's own thread, which waits on the browser, and only while the tool has
  started and is still saying something. A code is typed only where the tool asks for one,
  once, and again once Claude Code has refused it: it reads another in the same sign-in, as
  its register's `sign_in_takes_another_code` holds. Once the model has gone, every sign-in
  still under way is stopped, so no thread waits on a browser for ever.
- A sign-in under way keeps the sheet it was started from: putting up another while it runs
  would leave the tool running with nothing on screen to finish or stop it. What goes wrong
  is said in that sheet, or in the window where the sheet has gone, and the sheet closes once
  the sign-in has finished.
- `AppControl` names an app by one string, the id the core's holder detection gives it,
  which on macOS is its bundle id, in `provider/codex/holders.rs`. What names an app on
  Windows, and so what the core's holder detection gives there, is a question still open
  for the owner; the trait takes one string so that a Windows id fits it unchanged.
- One thread tells a `ModelListener`, so snapshots arrive in revision order, and a snapshot
  is told only where it differs from the last. The model never holds a lock while it calls
  out, so a listener may call `snapshot`, `send` and `shutdown`. Dropping a `PitboardModel`
  waits on no thread, since .NET can free it from its finalizer thread
  ([The C# bindings](#the-c-bindings)); `shutdown` waits for the actor alone.
- No test makes the app model over the machine's own environment. Its Rust tests run the
  real core over a context of their own, with `MemoryHost` and `ScriptedApi` and a home in a
  scratch directory. The Swift tests make no model, and the C# tests make one only as
  `ModelTests.cs` does: every home a fresh folder, and never started or sent an intent, so
  it reads nothing. So the launch, a listener, an `AppControl`, `Notifications`, a
  `LocalTime` and a first snapshot cross the bindings in a test, but no intent does yet, and
  no test has the library call one written in Swift or C#. Every `AppControl` a test hands
  the model is a stand-in that records what it was asked, never one that reaches a real app,
  and its `Notifications` keeps what it is given and posts nothing. A test that signs in
  runs a stand-in for `claude`, a shell script of its own in its scratch home, and plants
  the login it would have stored in `MemoryHost`'s keychain: never a real `claude` or
  `codex`.
- The app has no rule of its own for what the core decides: its home, Pitboard's directory,
  whether a path is a program, the sites and which links from outside it opens are asked of
  the core. The account windows key their records by Pitboard's directory standardised as
  Foundation standardises a file URL, as they did before the core said where it is, in
  `WebEnvironment.recordKey` alone.
- On macOS, only `/usr/bin/security` reads or writes Claude Code's keychain item and
  Pitboard's parked items. No keychain item is touched through the Security framework. The
  reason is under [macOS](#macos) in Measured facts.
- Pitboard takes Claude Code's write lock, the same way Claude Code takes it, before writing
  Claude Code's login. It writes the login where it already lives.
- Codex takes no lock on `auth.json`, so a switch reads the file again before replacing
  it. Every switch reads the live login back rather than trusting its own write.
- Pitboard never answers a failed keychain write by writing Claude Code's plaintext file.
  That demotion is Claude Code's to make.
- A Codex login is moved, never copied (`ParkSemantics::MoveOnly`). The parked login is
  read back before the incoming login is written. Codex's own sign-in and sign-out revoke
  the stored refresh token, so two usable copies of one login must never be at rest.
- No login moves until Pitboard knows whose it is. A Claude Code login's account is asked of
  Anthropic; a Codex login's is read from its ID token.
- Pitboard never renews the login in use. That is the tool's own job, and a second renewer
  would break it.
- Nothing outside `pitboard-core` writes Pitboard's index. Every change goes through
  `switch`, which records what it is about to do first and finishes an interrupted change
  before starting another.
- Additive writes become durable before destructive ones. A run that dies midway leaves a
  spare copy of a login, never a missing one.
- The `--json` contract changes only on purpose. A change to a snapshot is a change to the
  contract.
- The state file is read forwards only.

## Boundaries

- The core and its front ends meet at `service::Pitboard`, `context::Context` and the types
  those two return. That is the supported interface of `pitboard-core`. The other public
  modules are public only so the front ends in this repository can reach them, and may
  change in any release.
- In that interface, adding an error, warning or check code is not a breaking change.
  Renaming or removing one is. While Pitboard is at 0.x, a breaking change gets a new minor
  version, as in 0.3.0, and after 1.0 a new major version.
- Pitboard and each tool meet at the tool's register. What Pitboard relies on about a tool
  is written there, and the conformance run checks it against the tool's newest build twice
  a week.
- Pitboard and each service meet at a few requests. The list, with what each request
  carries, is in
  [What leaves your machine](https://docs.usepitboard.com/security#what-leaves-your-machine).
- Pitboard and claude.ai or chatgpt.com meet at an account's window, which a person opens.
  The site's own pages sign the window in, WebKit keeps that sign-in in the account's
  store, and Pitboard decides only where each navigation goes.
- Pitboard and a browser meet at the Share extension, which hands the app the one link the
  browser shares, as a Pitboard link. Pitboard reads nothing a browser keeps.
- The code and the release meet at a `v` tag, which `.github/workflows/release.yml` turns
  into a release. [RELEASING.md](RELEASING.md) has the procedure.

## The state file

`~/.pitboard/state.json` is Pitboard's index: which accounts it knows, and where each one's
login is parked. It carries a `schema` number.

The app and the command line inside it update together. A command line installed another
way updates by its own route. So on one machine, an older Pitboard can meet a file a newer
one wrote.

Reading forwards is `state::migrate`. Each schema bump adds an arm that rewrites the
document and falls through to the next. A file two versions behind comes forward in one
read.

Reading backwards is not possible. The older Pitboard refuses the file and says to update
it. A bump needs a test that loads a file the previous version wrote.

Schema 4 records each account's tool, and which account is signed in for each tool. A
schema 3 file is brought forward on its first read, with no keychain item or vault file
touched.

A file naming a tool this build does not know is reported as written by a newer Pitboard,
not as corrupt. The advice for a corrupt file is to delete it, and following that here would
orphan every parked login.

## Tool registers

Every fact Pitboard relies on about a tool was read out of one build of that tool. Each tool
keeps its facts in a register, `crates/pitboard-core/src/provider/<tool>/assumptions.rs`,
dated with the build they were read from.

A fact names literals a build must contain (`probe`), or literals whose arrival would
disprove it (`absent`). Beside the facts, the register names those that only one system's
build can be read for (`read_on`). A fact about behaviour, such as the 30 second cache,
names no literals. `pitboard-conformance`
tells a macOS build from a Linux one by its header, and reports which facts can still be
read from it, which have moved, which name nothing to look for, and which it skipped as
read from the other system's builds.

The check is shallow on purpose. A literal being present does not prove the behaviour around
it is unchanged. A literal disappearing, or a ruled-out one appearing, does prove something
moved.

On 22 September 2026, the twelve facts the Claude Code register then held were checked
against six macOS builds. They could be read from 2.1.273 to 2.1.278, and the check went
red on 2.1.124. That build predates the credential write lock, two of the five
account-scoped keys and the keychain error classification. On 29 September the register was
read again from the macOS and Linux builds of 2.1.278, 2.1.281 and 2.1.284: every fact
holds on 2.1.281 and 2.1.284, and on 2.1.278 the three that describe the 2.1.281 change are
reported moved, as they should be.

`.github/workflows/conformance.yml` checks the newest build of each tool against its
register on Mondays and Thursdays, or a version given by hand: Claude Code's macOS and
Linux builds, and Codex's Linux build. Its most recent run says
which facts can still be read from the build it checked, and which have moved.

Adding a tool takes three things: a register read out of a named build, a module under
`provider/` implementing `Provider`, and a conformance job. `ProviderId`, `ProviderId::ALL`
and the matches in `provider::of` and `assumptions::of` name every tool. The compiler and
the tests then point at what an added tool has to fill in.

How to run the checker and add a fact is in
[Tool registers in CONTRIBUTING.md](CONTRIBUTING.md#tool-registers).

## Account windows

The app gives each enrolled account a window on its tool's site: claude.ai for a Claude
Code account, and chatgpt.com for a Codex account. The site's own pages run in it, in
WebKit, with a store of website data that belongs to that account alone. A page shared from
a browser's Share menu reaches the app as a Pitboard link, and the person chooses which
account's window opens it.

The windows read the accounts from the app's last read. No window is ever given a Claude
Code or Codex login. The facts this rests on are under
[WebKit and SwiftUI](#webkit-and-swiftui) and
[claude.ai and chatgpt.com](#claudeai-and-chatgptcom).

### Where the code is

- `crates/pitboard-sites`: what a site is, and what a link from outside may be, in the
  [code map](#code-map). The app reaches it through `pitboard-ffi`'s `Site` and `SiteLink`
  records: its windows and menus ask `sites`, `sites_for` and `site_names`, and
  `LinkInbox.swift` reads each Pitboard link with `read_pitboard_link` and says a refusal
  with `link_refusal_reason`. Only the tests make a link with `site_link`. The Share
  extension reaches it through `pitboard-share-ffi`.
- `apps/macos/Sources/PitboardLinkTarget`: where a Pitboard link goes, which the app and the
  Share extension both link. `LinkTarget.swift` names the Info.plist key `PitboardURLScheme`
  that gives each build its scheme, and finds the app an extension is inside.
- `crates/pitboard-ffi/src/account_windows`: the windows' rules, in the
  [code map](#code-map). Which accounts have a window, the store each one's data is kept
  in, the menus' entries and the forget alert's text; where each navigation, new window,
  response and download of a page goes; what a window says above its page and before it
  removes what it keeps; what a page may use and close; whose words a dialog says; what a
  download is called and where it comes from; what becomes of a page whose content stops;
  and how big a sign-in window opens. The Swift below turns what WebKit says into what
  these read, and does what they decide.
- `apps/macos/Sources/PitboardApp/AccountWindows`: the windows.
  - `WindowAccount.swift` gives a window's store as the `UUID` WebKit names a store by and
    the scene keeps a window's value as.
  - `NavigationPolicy.swift` turns WebKit's URLs and a navigation's target frame into what
    `decide_navigation` reads. `WindowNote.swift` is what a window says above its page.
  - `Page.swift` owns one `WKWebView` and publishes its title, address, progress and
    history, in the shape of SwiftUI's `WebPage`. `PageDelegate.swift` answers WebKit's
    navigation and UI delegates for a page by asking the core's rules.
  - `WebSession.swift` is one open window: its account, policy, page, note and sign-in
    window. `PopupWindow.swift` is that sign-in window, an AppKit window, since WebKit needs
    its web view back before a SwiftUI scene could open.
  - `Downloads.swift` keeps every window's downloads past the window, gives each the name
    `download_destination` chooses, and asks before one the site's own page did not start.
    `PageDialogs.swift` shows a page's alerts, questions and file choosers as sheets, and
    asks that download question.
  - `WebViewHost.swift` places a page's web view in SwiftUI, with the system find bar above
    it. `AccountWindowView.swift` is the window, and `AccountWindowCommands.swift` its scene
    and its items in the **File**, **Edit**, **View** and **Go** menus.
  - `AccountWindows.swift` owns the feature: the open sessions, links waiting for a window
    still opening, windows asked for from the Dock, each window's last page, the store
    janitor and the downloads. `AppModel.afterRead` tells it of each read that succeeded.
  - `StoreJanitor.swift` records, makes, wipes and deletes stores. `WebEnvironment.swift` is
    the world a launch's windows run in: the sites and WebKit's stores, or a fixture's
    stand-ins.
  - `LinkInbox.swift` holds the link the Share extension handed over, and
    `AccountPicker.swift` is the **Open Link** window that asks which account opens it.
- `apps/macos/Sources/PitboardApp/System/WebsiteData.swift`: WebKit's persistent stores, behind
  the `WebsiteDataStores` protocol. Beside them, two records kept in the app's preferences
  under each Pitboard directory's path: `StoreRecord`, the stores the directory made, under
  `webStores`, and `PageRecord`, the page each account's window was last on, under
  `windowPages`, by store.
- `apps/macos/Sources/PitboardApp/App`: `AppDelegate.swift` owns the app's models and answers
  what only a delegate can: the Dock icon's menu, a click on the Dock icon, and quitting
  while a download runs. `AppPresence.swift` gives the app a Dock icon and its menus while
  any of its windows is open. `RefreshCommand.swift` is **View** > **Refresh**, whose
  title and action the window in front gives. `PitboardScenes.swift` adds the account
  windows' scene and the **Open Link** window, the one scene that takes a Pitboard link.
- `apps/macos/Sources/PitboardApp/Fixture/FixtureWeb.swift`: a fixture's stand-in pages for each
  site and sign-in host, on `pitboard-fixture://`, with stores in memory and links to
  anywhere else recorded and opened nowhere.
- `apps/macos/ShareExtension`: the `PitboardShare` target. It checks the shared page with
  `share_link`, which writes the Pitboard link too, then opens that link with the app it is
  inside, not whichever copy Launch Services would pick. The flow of an app extension stays
  Swift, and `PitboardLinkTarget` finds the app it is inside and the scheme its Info.plist
  names.

### What must stay true

- One store per account, derived from the account. A window's store is a version 5 UUID of
  `<store name>:<account id>` in a fixed namespace, which `store_id` writes in lower case.
  It is also the window's value, so there is one window per account, and a rename keeps its
  sign-in. The namespace and the store names, `claude` and `codex`, never change: a change
  would leave every window without its data, and the next sweep would delete that data.
  Golden tests in `stores.rs` pin them, and that the account id is lowered a character at
  a time, as Swift's `lowercased()` lowered it for every store released, where
  `str::to_lowercase` lowers a sigma that ends a word otherwise. A store id an app hands
  back is compared without regard to case, since Foundation writes a UUID in upper case.
- A store is recorded before WebKit makes it, in the app's preferences, under the path of
  the Pitboard directory the app reads. A store WebKit made and nobody recorded would never
  be deleted.
- A store is deleted only when this Pitboard directory recorded it, and a read that
  succeeded no longer derives it from any enrolled account. Every read that succeeds lists
  every enrolled account, from `state.json`, so that is a forgotten account, forgotten in
  the app or with `pitboard forget`. Nothing is deleted before the first read that
  succeeds, or after one that failed.
- A store that another Pitboard directory recorded too is never deleted while that
  directory exists. The same account enrolled in both derives the same store, so deleting
  it would sign the other directory's window out. Forgetting the account in one removes
  only that directory's record.
- A store nobody recorded is left alone. WebKit keeps every store of one bundle under the
  person's own Library, whatever `HOME` says, so it can belong to a copy run with another
  home. A store something still holds is left recorded, and tried again at the next read.
- Nothing from outside opens a window by itself. A Pitboard link only shows the **Open
  Link** window, and only a person's choice there opens a window, on the link's own site.
  The app checks the link as strictly as the extension did, since anything on the Mac can
  open one. A site's sign-in link is refused, since it would sign the window in as whoever
  it belongs to.
- A window's page goes only to its site, the site's sign-in hosts and blank pages. Google's
  sign-in is refused, other web pages go to the default browser, an email address goes to
  the email app when clicked, and nothing else leaves. What the page embeds in its frames,
  such as an artifact, is the page's own choice, except a local file.
- A new window is decided only in `createWebViewWith`, where WebKit asks for the page to
  put in it. The navigation's own decision lets through a link that asks for one: refused
  there, WebKit would never ask for the window, and a sign-in link would open nothing.
- Only the site's own main page, or a sign-in page as the window's main page, opens a
  sign-in window. A frame, such as an artifact, gets none: it could fill the window with a
  page of its own, which nothing on the window's title would tell apart.
- A sign-in window is made from the configuration WebKit hands `createWebViewWith`, a copy
  of its opener's, so it shares the account's store and keeps `window.opener`. Its page
  loads only the site and its sign-in hosts, and goes blank only when the page itself asks,
  never its opener. It saves nothing and opens no window.
- Only a site's own pages are kept as a window's last page. **Remove Website Data** takes
  it away, and so does a read that succeeded and no longer lists the account.
- Hands off the session. Pitboard makes a store, wipes one when asked and deletes one when
  its account is forgotten. It never reads, copies or changes what a site keeps there,
  adds no script or message handler to a page and sets no user agent of its own. No web
  session is made from a Claude Code or Codex login.
- The Pitboard link's format is a contract between the Share extension and the app, pinned
  by a golden test. A release claims `pitboard://` and a debug build `pitboard-debug://`,
  so a debug build never answers a link meant for an installed copy. A release build from
  `build-app.sh` claims `pitboard://` like the installed copy, until it is unregistered.
  `build-app.sh` fails a bundle whose app or extension names another scheme.
- The Share extension stays sandboxed, with no other entitlement, and links only
  `PitboardShareBindings` and `PitboardLinkTarget`. `build-app.sh` signs it with its
  entitlements before the app, and fails when its signature is not sandboxed. The release
  workflow checks the installed copy again.
- The extension checks a link by the same Rust as the app, `pitboard-sites`, but through
  `pitboard-share-ffi`, and never links the core: the core's bindings check every export's
  checksum when they load, so linking them brings all of it in. Two Rust static libraries
  never meet in one binary, since each carries its own copy of Rust's standard library.
  `build-app.sh` fails when the extension's binary has a `uniffi_pitboard_ffi_` symbol or
  none of `uniffi_pitboard_share_ffi_`, or the app's has a `uniffi_pitboard_share_ffi_`
  symbol or none of `uniffi_pitboard_ffi_`. No test target of the Swift package links
  `PitboardShareBindings`, since SwiftPM may link every test target into one bundle.
- `pitboard-sites` stays a leaf: no I/O, nothing of the core and no UniFFI, and `url` for
  IDNA alone. Each binding crate declares its own types over it, since the C# generator
  cannot use another crate's.

## Measured facts

These decide the design. Each gives the build it was read from, or the date it was measured
or written down. The conformance run reads only the facts in a tool's register, and only
by their literals.

### macOS

Measured on macOS 26 and written down on 22 September 2026. Before changing code that
depends on the `security -i` limit or the Security framework's cost, measure them again on
a scratch item.

- `security -i` reads at most 4097 bytes of command from standard input, with no line
  continuation. Its interactive `-w` prompt takes 128 bytes.
- A keychain item written in process, through the Security framework, stays slow to read.
  Every later read of it by `security` takes about a second instead of 0.01 seconds. On a
  scratch item, reads went from 0.01 seconds to 20.55, then settled around 0.8.
- Claude Code reads its login on every cache miss, so writing its item through the framework
  would slow Claude Code for good. Pitboard writes a large login on the argument line
  instead, where `ps` can see it for the length of one call.
- Undoing the framework's change needs `security set-key-partition-list`, which asks for the
  keychain password.
- APFS keeps a directory's mtime in nanoseconds, but not exactly: setting one and reading it
  straight back gives a value 18 to 60 nanoseconds away. A lock that remembered the value it
  asked for would abandon every switch, so `lock.rs` keeps the value read back.
- Pitboard lists its parked items with `security dump-keychain` without `-d`. It never
  prompts, and emits attributes only, no secret of any item. It exits 0 in 0.06 seconds
  against a keychain of 362 items.
- Reads after a `dump-keychain` take the usual 0.016 seconds, so listing has none of the
  access-list cost of an in-process read. Each service name is on a line of the form
  `"svce"<blob>="<name>"`.
- Measured on macOS 27.0 on 5 October 2026, with Rust 1.98.1: a lock taken with
  `File::try_lock`, which is `flock`, can stay held after its `File` is dropped, though std
  opens every file so that a program started after does not keep it. A process another
  thread is starting holds a copy of the descriptor until it runs its program, or ends.
  Dropped and taken again at once, in 3000 rounds each, the lock was still held 437 to 1042
  times, for up to 5.5 ms, while another thread started processes the way std forks and
  execs, which it does for a program named bare with `PATH` set, as the core runs a tool's
  program it found nowhere; 8 to 15 times, for up to 77 µs, where std uses `posix_spawn`;
  and never with no process started. Through the core's own sign-in, one started right
  after a cancel in the same home was refused as one already waiting 64 times in 100 while
  another thread started a program it could not find, and never in 100 while one started
  `/usr/bin/true` or none started anything. So the core's one sign-in at a time can outlive
  a cancel by that long. No person starts a sign-in that fast, and a test that signs in
  again right after a cancel does so in a home of its own.

### Claude Code

Read against Claude Code 2.1.284's own storage layer, macOS and Linux builds alike, on 29
September 2026, unless a fact gives its own date. The conformance runs of 24 and 28
September reported three facts moved on 2.1.281 and 2.1.283. All three were false: they
read the Linux build, which has no keychain code, and a `libsecret` that belongs to the Bun
runtime Claude Code ships in. The one real change, in 2.1.281, is how a locked keychain is
treated, and only the macOS build shows it. The run reads both builds since.

- A running session serves its login from a 30 second cache, so it picks up a switch within
  about 33 seconds.
- Every write of the login takes proper-lockfile's directory lock at
  `<storage dir>/.storage-write`: stale after 15000 ms, ten retries, 100 ms to 1000 ms of
  backoff. `lock.rs` carries the same numbers.
- Every write under that lock drops the read cache, reads the login again inside the lock,
  and abandons the write when that read fails. A stale account cannot be written back. From
  2.1.281, a locked keychain counts as a failed read here once the process has seen its
  item; before, it read as empty.
- Claude Code treats its own lock going missing as a warning and keeps writing. Pitboard
  cannot expect the other side to stop.
- A write can be marked as already locked without the lock being taken. `/logout` does this
  after retrying for 7.5 seconds, and deletes the login with no lock held.
- The keychain write is `security -i` while the command is at most 4032 bytes. Past that it
  is `add-generic-password -U -a <account> -s <service> -X <hex>`, on the argument line. It
  never refuses or splits a login, and each call has a 2 second timeout.
- A failed write is transient, and does not move the login to the plaintext file, when it
  timed out or, from 2.1.281, when it exited 36 after the process had seen its item. Any
  other failure moves the login to the file.
- The login goes hex-encoded, two characters a byte, so standard input carries about 2 KB of
  it. A larger login is on Claude Code's own argument line at every token refresh. Pitboard
  keeps its `security -i` command within the same 4032 bytes, the limit its messages give.
- The keychain read is `find-generic-password -a <account> -w -s <service>`. Exit 0 with
  output is the login; exit 0 with nothing, 44, or output that is not JSON is absent. Exit
  36, a locked keychain, is absent to an ordinary read, a failed read to a write once the
  process has seen its item (from 2.1.281), and a failed read outright only when a caller
  asks. Pitboard reads 36 as unreadable on purpose, the strict end of that.
  `security show-keychain-info` exiting 36 only adds an unlock hint.
- On macOS the live chain is the keychain, with the plaintext file `.credentials.json`
  behind it. The successor backend, behind the `tengu_hover_rest` flag, replaces only the
  fallback half, and only for a caller that hands one in. An ordinary `claude` still reads
  the keychain first.
- Claude Code demotes to the plaintext file when a keychain write fails for good, and
  deletes the keychain item when it does. A locked keychain after the item was seen is not
  failing for good, from 2.1.281.
- The supervisor daemon records itself in `<config dir>/daemon.lock`, with its pid and the
  Claude Code version that started it. It leaves the file behind when it stops.
- Claude Code's storage backends are `keychain`, `plaintext` and `windows-credman`, the
  last behind the `tengu_windows_credman` flag. Its code has no `libsecret`,
  `org.freedesktop.secrets`, `gnome-keyring` or `SecretService`. The Linux binary does
  contain `libsecret`, in the Bun runtime it ships in, behind `Bun.secrets`, which Claude
  Code's code never calls.
- `secret-tool` and `kwallet-query` do appear in the bundle, in the credential helpers its
  Bash sandbox keeps out of a shell. So neither name is used to look for a keyring backend.
- On Linux, Claude Code has no keychain backend at all: the plaintext file holds the login.
  Claude Code writes it, then sets its mode to 0600, and Pitboard's Linux host matches
  that.
- The register holds that absence as `no_keyring_off_macos`, and the conformance run looks
  for `Bun.secrets` and each keyring name in every Linux build. A fact that rests on
  something not existing is wrong the moment it does.
- A Claude Code parked login holds the account's slice of Claude Code's credential
  document. On one real account, measured on 22 September 2026, the slice was 524 bytes
  against 506 for the OAuth block alone. An account holding a device token has not been
  measured.
- Claude Code's config file can be a day behind the login it describes, so Pitboard asks
  Anthropic whose a login is. This was written down on 21 September 2026, with no build
  named.
- On 22 September 2026, the machine measured sat within 0.75 seconds of the `Date` header
  of api.anthropic.com across eight requests. `Date` has a granularity of one second.
- That spread is inside the noise, so Pitboard keeps no estimate of clock skew. A renewal's
  expiries are counted from the `Date` of the answer that carried them.
- Read from 2.1.289 on 5 October 2026: before it opens the browser, `claude auth login`
  writes an `https` address and `Paste code here if prompted > ` to stdout, and from then
  on reads a pasted code. So the app offers the code field with the address. The address
  is the manual one, whose page shows the code; the browser it opens goes to another,
  which comes back to the loopback. Piped, the address is bare unless `FORCE_HYPERLINK` is
  set or the environment names a terminal that takes hyperlinks, such as `TERM_PROGRAM` set
  to iTerm.app or `WT_SESSION` set. Then it is an OSC 8 hyperlink ended by BEL, with the
  address again as its text. The sign-in runs with the app's whole environment, so
  `provider/printed.rs` reads what it printed as a terminal does, and the address offered
  is where the hyperlink goes.
- Read from 2.1.289 on 5 October 2026, in the Windows arm64 build's sources and the macOS
  and Linux builds, where the conformance run finds each literal, and finds none in 2.1.110:
  `claude auth login` reads every line typed back while it waits. One that is not
  `<code>#<state>` with both halves, once trimmed and split at `#`, it refuses with `Invalid
  code. Please make sure the full code was copied.` on stderr, and goes on reading in the
  same process, with no new prompt. The first with both halves it takes, whatever its state
  half says. After that it still refuses a line without both halves, and ignores one with
  them; a code the token exchange refuses ends the sign-in with `Login failed: ` and exit
  status 1. So the model offers the code field again once Claude Code has refused one, to
  the same sign-in, and starts nothing again.
- Read on 5 October 2026 from the macOS builds of 2.1.283 and 2.1.289 and the Linux build of
  2.1.289: the native installer puts its launcher at `~/.local/bin/claude`, and says so
  when that directory is not on `PATH`. A global npm install puts `claude` in npm's global
  `bin`, which is Homebrew's `/opt/homebrew/bin` or `/usr/local/bin` where Homebrew
  installed Node, and `/usr/local/bin` where nodejs.org's installer did; the build lists
  both among npm's places. Claude Code tells a global npm install by
  `/node_modules/@anthropic-ai/` in the path the running program resolves to, and the
  Homebrew cask's by a path through a Homebrew `Caskroom`. So an app with no shell's `PATH`
  looks in `~/.local/bin`, `/opt/homebrew/bin` and `/usr/local/bin`, after the login
  shell's.

### Codex

Read against codex-cli 0.154.0: the binary, its public source at tag `rust-v0.154.0`, and a
real `auth.json` that build wrote. The register is `provider/codex/assumptions.rs`.

- The login is `$CODEX_HOME/auth.json`, by default `~/.codex/auth.json`, at mode 0600.
- `file` is the packaged default store on every platform, and the only one Pitboard
  supports. `keyring`, `auto` and `ephemeral` are the others.
- `keyring` and `auto` keep the login in a keychain item, `Codex Auth`, that Codex makes
  through the Security framework. With either of those, the `secret_auth_storage` feature
  keeps it in `secrets/codex_auth.age` instead, under a keychain key.
- Those items trust only `codex`. A read by another program brings up a permission prompt,
  and choosing **Always Allow** would change Codex's item. So Pitboard refuses `keyring`
  and `auto`, with or without that feature.
- The login document holds `auth_mode`, `OPENAI_API_KEY`, `last_refresh`, and `tokens` with
  `id_token`, `access_token`, `refresh_token` and `account_id`. `OPENAI_API_KEY` can hold an
  API key Codex obtained at sign-in.
- `codex login` and `codex logout` both POST the stored refresh token to
  `https://auth.openai.com/oauth/revoke` before clearing it. This is why parking a Codex
  login moves it (`ParkSemantics::MoveOnly`).
- A running Codex holds its login in memory for the life of the process and watches no file.
  It refuses a reload whose account id has changed (`Adoption::RestartRequired`), so it
  never picks up a switch.
- A Codex refresh already under way when the file changes writes its own account's tokens
  under whatever account id it finds there.
- Codex writes `auth.json` with no lock of any kind, so there is none for Pitboard to share.
- Measured on 2026-10-01 from the process list of a Mac running each of them:
  - OpenAI's ChatGPT app for macOS 26.928.31416, bundle id `com.openai.codex`, runs a codex
    0.159.2 of its own: two processes of
    `ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex`, children
    of the app. That closing its windows leaves it running, and quitting it stops them, is
    read from the app's code and not yet watched.
  - Codex's background app server runs from
    `$CODEX_HOME/packages/app-server-daemon/releases/<version>/bin/codex`, a child of
    launchd, and 0.159.3 has `codex app-server daemon restart`.
  - On macOS, `ps` gives what a process was started as. A `codex` started from a shell by
    its bare name lists as `codex`; one started by its path lists the path. Pitboard tells
    the kinds apart by the directories in that path, and a bare name is a `codex` session.
  - No editor with the Codex extension was running there. Its place, a folder named
    `openai.chatgpt-<version>`, is the extension's packaged layout, not measured.
- The ID token names the account: `email`, and under `https://api.openai.com/auth`,
  `chatgpt_account_id` and `chatgpt_user_id`. A Team or Business workspace shares one
  `chatgpt_account_id`, and `chatgpt_user_id` is the person.
- Pitboard identifies a Codex account by that pair, with no network call.
- Renewal is `POST https://auth.openai.com/oauth/token` with a JSON body
  `{client_id, grant_type, refresh_token}` and client id `app_EMoamEEZ73f0CkXaXp7hrann`.
  Each token in the answer is written only if present.
- `last_refresh` must be in the login, as an RFC 3339 string, or Codex reads the login as
  having no token data.
- A spent or revoked refresh token answers 400 `invalid_grant`, or one of the older
  `refresh_token_expired`, `refresh_token_reused` and `refresh_token_invalidated`, or 401.
  Any other 400 is not a dead login.
- Usage is `GET https://chatgpt.com/backend-api/wham/usage` with `Authorization: Bearer`
  and `ChatGPT-Account-ID`, and spends no quota.
- The usage answer's shape was read from a live answer. A parser written from the source
  looked for the windows in the wrong place, and returned nothing while the request
  succeeded.
- `CODEX_HOME` moves everything Codex keeps, and an empty one means unset. Pitboard's own
  sign-in, the one `enroll --sign-in` runs, sets it to a directory that exists, and runs
  `codex login` from inside it.
- The sign-in starts inside that directory because Codex also reads `.codex/config.toml`
  from a trusted project it starts in. That file could name a keychain store.
- `codex login` revokes the login stored in its home before signing in. It opens the
  browser itself and reads nothing from standard input.
- Read from 0.160.0 on 5 October 2026: `codex login` prints to stderr its loopback address,
  `http://localhost:<port>`, and then, bare on a line of its own, the `https` address on
  auth.openai.com to open. So the first `https` address it prints is the one to open.
- Codex's standalone installer, `scripts/install/install.sh` at tag `rust-v0.159.2`, links
  `~/.local/bin/codex`, or `$CODEX_INSTALL_DIR/codex`, to
  `$CODEX_HOME/packages/standalone/current/bin/codex`; a standalone install of 0.159.2 on a
  Mac had left that link, read on 5 October 2026. Its macOS and Linux binaries name
  `npm install -g @openai/codex` and `brew upgrade --cask codex` as the other ways it is
  kept up to date. 0.154.0 names those too, but not the standalone package layout.

### WebKit and SwiftUI

Measured on macOS 27.0 on 4 October 2026, with probe apps built by Xcode 27.1 against the
macOS 27 SDK, unless a fact gives its own date. Several were checked again with the app's
own code driving WebKit on a fixture's stand-in pages. Before changing code that depends on
one, measure it again.

- SwiftUI's `WebPage` cannot host an account window. `window.open` returns null for every
  trigger and no app callback fires, so a sign-in that opens a window never starts. A
  `target=_blank` link loads nothing unless the app loads it itself, which loses the
  opener. A download never reaches the app or the disk, and `WebPage` has no page zoom or
  print. So `Page` keeps a `WKWebView`, in `WebPage`'s shape.
- On a `WKWebView`, a `target=_blank` link reaches `decidePolicyFor` first, with no target
  frame. Cancelled there, it never reaches `createWebViewWith`; allowed, it does, with the
  same action. `window.open` goes straight to `createWebViewWith`.
- `window.open('')` reaches `createWebViewWith` with an empty address. The page in the new
  window then navigates, through its own `decidePolicyFor`. `webViewDidClose` fires on
  `window.close()`. A `target=_blank` link without `rel=opener` gives the new page no
  opener.
- WebKit quarantines a downloaded file itself, and makes the suggested name safe:
  `../../evil:name.txt` becomes `_.._evil_name.txt`. A response turned into a download also
  fails its navigation with code 102 in `WebKitErrorDomain`, which the window does not show.
  `WKDownload.originatingFrame` is macOS 15.2 and later.
- A response from an app's own scheme handler loses its headers and cannot become a
  download, so a fixture's downloads are `data:` and `blob:` links.
- `allDataStoreIdentifiers` and `remove(forIdentifier:)` each end in a segmentation fault in
  `WTF::RunLoop::dispatch` when they are a process's first WebKit call. Making any store
  first prevents it. So `WebKitDataStores` makes a non-persistent store before its first
  deletion, not at launch, and a person who never opens a window never starts WebKit.
- `remove(forIdentifier:)` reports a store in use for 20 to 60 ms after its last web view
  and store object are released, then succeeds. It reports it in use for as long as a
  `WKWebsiteDataStore` object for it is held. The janitor's retries, from 50 ms doubling to
  1600 ms, cover the first case, and the next read covers the second.
- `removeData(ofTypes:modifiedSince:)` with every type since `.distantPast`, on a store a
  page is using, clears every cookie, `HttpOnly` ones included, local and session storage,
  and IndexedDB. **Remove Website Data** rests on it to sign a window out while it stays
  open.
- A `WKWebView` answers neither `performFindPanelAction:` nor `performTextFinderAction:`.
  An `NSTextFinder` with the web view as its client and `WebContainer` as its bar's
  container shows the system find bar and steps through the matches.
- With incremental searching on, **Find Next** left the page's selection where it was, so
  it is off and a search runs on Return. A finder whose bar is already in the view, hidden,
  never shows it, so `WebContainer` adds the bar only while it shows.
  `TextEditingCommands()` adds the **Edit** > **Find** items, which send
  `performFindPanelAction:` with `NSTextFinder.Action` tags.
- Setting `pageZoom` leaves a pinch's `magnification` as it was, so **Actual Size** resets
  both.
- SwiftUI hands a URL to the `Window` scene that has `.handlesExternalEvents(matching:)`,
  through its `onOpenURL`, whether the app was running or not. It opens that window when
  none is open, reuses one that is, and opens no other window. With no scene claiming the
  URL, it went to the first window scene, the Pitboard window's, and opened that window.
- With an app delegate's `application(_:open:)` as well, the claiming scene's `onOpenURL`
  gets the URL and the delegate is then called with an empty list. With no scene claiming
  it, only the delegate gets it, and no window opens. So the **Open Link** window alone
  takes Pitboard links, and `AppDelegate` has no `application(_:open:)`.
- Those URLs were sent with `open -a` from a terminal. A cold launch that way left the app
  inactive, behind the terminal, so the **Open Link** window brings Pitboard forward when a
  link arrives. `AppPresence.comeForward` records that a link from the Share extension at a
  launch left Pitboard in the background too. A link opened from a browser was not measured.
- In CI, on macOS 26.6.2, the running app's **Open Link** window took each link too.
  `XCUIApplication.open(_:)` sent them, and it launched a second copy of the app rather
  than handing the link to the one running: the picker opened in one copy and the Pitboard
  window in the one the test watched. The copy left running kept its menu bar item, and
  with a few of those a later test's own item sat under the menus, out of reach. So the
  UI tests share a link through `XCUIDevice.shared.system.open(_:)`, which opens it with
  Launch Services, as the Share extension does, and so reaches the copy running.
- A window's root view gets `onAppear` when its window opens and `onDisappear` when it
  closes, not when it is minimised or becomes a background tab. `AppPresence` counts open
  windows by them. `openWindow(id:value:)` with the value of an open window brings that
  window back and opens no other.
- `SceneStorage`'s documentation in the macOS 27 SDK says its data is destroyed when a
  window is closed on macOS. So an account window's last page is kept in the app's
  preferences, by the window's store, and comes back in a window opened from a menu as well
  as in one macOS restores. This was read, not measured.
- The account picker's root view gets `onDisappear` before the account window it opened
  gets `onAppear`. Going back to `.accessory` in between gives the app's activation away, so
  the account window opens behind the browser; `AppPresence` waits a moment before going
  back to the menu bar.
- SwiftUI puts an item that opens each `Window` scene in the **Window** menu, unless
  `.commandsRemoved()` is applied, and does not list that scene's window there. A
  `WindowGroup`'s windows are listed while they are open.
- After `setActivationPolicy(.regular)` on an app that is already active,
  `NSWorkspace.menuBarOwningApplication` goes on naming the app before it, but the menu bar
  shows the app's own menus, seen in a screen capture on 4 October 2026 with a probe that
  has a main menu of its own. The reported owner is not what the person sees. An app made
  regular while inactive and then opened by Launch Services is reported as the owner as
  well. `NSApp.deactivate()` from an active app left it active.
- Measured on 4 October 2026 with a probe app driven by `open -g`: `NSApp.activate()` from
  an app in the background, with no click in it, is refused. Launch Services opening the
  app, `NSWorkspace.openApplication` with `activates` set, brings it forward. So
  `AppPresence.comeForward` asks Launch Services to open Pitboard when a shared link arrives
  while Pitboard is in the background.
- The macOS 27 SDK has no SwiftUI API for a Dock menu. `applicationDockMenu(_:)` on the app
  delegate is the API.
- Measured on 29 September 2026: WebKit keeps the persistent stores of an app that is not
  sandboxed under `~/Library/WebKit/<bundle id>/WebsiteDataStore/<identifier>`, cookies
  included, as ordinary files. It roots them at the Library folder Foundation gives the
  app, and Foundation does not read `HOME`. With `HOME` pointed at a scratch directory,
  `NSHomeDirectory()`, `.libraryDirectory` and `homeDirectoryForCurrentUser` all still gave
  the real home. The core reads `HOME` and `PITBOARD_HOME`, so the record of stores is kept
  per Pitboard directory.

### What the Swift app said

Measured on macOS 27.0 with Swift 6.4 on 5 October 2026, so that `pitboard-ffi`'s
`present` says what the Swift app said.

- VoiceOver heard a limit's reset as a span Foundation's `Duration.UnitsFormatStyle` said:
  days, hours and minutes, wide, at most two units, in `en_US_POSIX`, of at least a minute.
  Asked for 100 spans, it rounds the span to whole minutes, a half to the even one, to see
  which units it has, and says the two largest that are not nought, so 1 day and 1 minute
  is "1 day, 1 minute". The last unit it says is the rest of the span in that unit, rounded
  the same way, and a rest that rounds up to a whole one of the unit before it is carried:
  1 day 23 hours 30 minutes is "2 days, 0 hours". It writes no thousands separator: 11,574
  days is "11574 days, 2 hours". `words::spoken_span` is tested against every one.
- The menu bar cut a label past 12 `Character`s to 11 and an ellipsis, and Swift counts
  extended grapheme clusters. Over 22 labels, accents written as combining marks, flags,
  emoji joined by zero-width joiners, keycaps, tag sequences, Hangul written in jamo, Thai,
  Arabic's prepended number sign, a carriage return before a line feed and Devanagari
  conjuncts, joined and not, `unicode-segmentation` 1.13.3's extended graphemes counted and
  cut each as Swift did.

### Foundation's URLs

Measured on macOS 27.0 on 5 October 2026, by giving the Swift `SiteLink` and `Handoff` of
0.7.0, and `URLComponents(string:)`, the same text as `pitboard-sites`. `pitboard-sites`
reads a link as Foundation does, so a link from outside means what it meant to the macOS
app.

- `URLComponents(string:)` splits a link by RFC 3986. A path, query or fragment holding a
  character RFC 3986 does not allow there is kept with every such character
  percent-encoded, a `%` among them, so an escape already in it is encoded too: `/%41 x` is
  kept as `/%2541%20x`. One holding none is kept as written, escapes and all. A second `#`
  is `%23` in the fragment.
- A host is never encoded: one with a character RFC 3986 does not allow, or a `%` that
  starts no escape, makes no link. A host in plain ASCII is percent-decoded and kept in its
  case, `claude.ai%00` included. One that is not, or that has an `xn--` label, goes through
  ICU's IDNA: `ｃｌａｕｄｅ.ai` is `claude.ai`, `xn--bcher-kva.de` reads back as `bücher.de`,
  and a joiner in a label makes no link. Anything between brackets is an IP literal.
- `user` and `password` are nil where their bytes are not UTF-8, `port` is nil where an
  `Int` cannot hold the number, and `path` is empty where its bytes are not UTF-8. So the
  Swift `SiteLink` opened `https://%FF@claude.ai/` and `https://claude.ai:99999999999999999999/`
  without what it dropped, took `claude.ai/magic-link/%FF` for no sign-in link, and opened
  `claude.ai/x/../%FF` without seeing its dot segment. The Swift `Handoff` read
  `pitboard://open/%FF?url=claude.ai` as `pitboard://open?url=claude.ai`. `pitboard-sites`
  refuses each.
- Swift compares strings by grapheme cluster, so a combining mark or a joiner right after a
  `/` makes one character with it. The Swift `SiteLink` took `claude.ai/magic-link/%CC%81`
  for no sign-in link, and `chat.com/` followed by a combining mark for no link at all. It
  counted a link's length in clusters too, and opened one of 4106 clusters and 8194
  scalars. `pitboard-sites` compares the text, and counts a link's length in Unicode
  scalars.
- `CharacterSet.whitespacesAndNewlines`, which trims a link, is Unicode's `White_Space` and
  U+200B ZERO WIDTH SPACE, checked over every scalar.
- The WHATWG URL standard, which the `url` crate and WebKit follow, reads a link otherwise.
  It resolves dot segments, takes `\` for `/` in an `https` link, drops a default port such
  as `:443`, finds a host in `https:claude.ai`, strips a tab or a newline and leaves a second
  `#` as it is. It reads a host whose last label is a number as an IPv4 address, and decodes
  a host's escapes before IDNA. So `pitboard-sites` asks `url` for IDNA alone, with a label
  after the host so that no host is read as an address.
- 450,000 generated links and Pitboard links, ASCII and not, had the same answer from both
  apart from: Pitboard links that `URL(string:)` refuses, which never reach the macOS app,
  though the Windows app, reading a Pitboard link from its command line as text, can be
  given one; the things above that `pitboard-sites` refuses; grapheme clusters; and hosts
  that are not ASCII, where ICU's and WHATWG's IDNA differ over empty labels, escapes and
  what a label may hold. Each such host was refused by both, though one named it otherwise,
  apart from `ｃ%EF%BD%8Caude.ai`: `pitboard-sites` decodes its escapes before IDNA, as the
  WHATWG standard does, and takes it for `claude.ai`, where the Swift `SiteLink` said it was
  on `cｌaude.ai`.
- `URL(string:)` reads a host otherwise than `URLComponents`, and an account window's rules
  compared the `URL` WebKit handed them, so `pitboard-sites`' `WebAddress` reads one as
  `URL.host` does, measured on 5 October 2026. A host in ASCII is percent-decoded and kept
  in its case, `xn--bcher-kva.de` and `XN--BCHER-KVA.de` as written, and so is one with an
  `xn--` label IDNA refuses, `xn--claude-.ai`, for which `URLComponents.host` is nil. A
  host that is not ASCII is given in IDNA's ASCII form, `аpple.com` with a Cyrillic а as
  `xn--pple-43d.com`, and one IDNA refuses makes no `URL`. An IP literal is given without
  its brackets. `https:///x`, `https:claude.ai/x`, `blob:` and `data:` links name no host.
  `port` is `0` for `:0`, and nil for `claude.ai:`; `user` is empty, not nil, for
  `https://@claude.ai/`. `WebAddress` reads two things otherwise: it keeps a port no `Int`
  holds, and names no host in `//claude.ai/x`, where `URL` reads `claude.ai` on no scheme.
- `NSString` splits a file name into a base and an extension at its last `.`, and finds no
  extension where it would be empty or hold a space, or where the base would be empty, `.`
  or `..`: `....a` has the extension `a`, `...a` none. `lastPathComponent` drops the
  slashes at the end, and is `/` for slashes alone. Measured the same day on 98 names, with
  the Swift `DownloadCenter` naming 52 of them as a download, free and with its own name
  taken.
- A file `URL`'s `path` is decomposed: `URL(fileURLWithPath:)` and `appendingPathComponent`
  give `Báo cáo.pdf` as `Ba\u{301}o ca\u{301}o.pdf`. So the Swift `DownloadCenter`, which
  compared `URL`s, numbered a download whose name a running one had in either form, though
  not `\u{F900}.pdf` against `\u{8C48}.pdf`, its canonical decomposition, nor two names
  that differ only in case. APFS, on this Mac's volume that is not case-sensitive, takes
  each pair for one file: one made under either name is found under the other. Measured the
  same day. `download_destination` compares the names it reserved decomposed, so it numbers
  the U+F900 pair too, and takes two that differ only in case for two, as the Swift did.
- `CharacterSet.whitespaces`, which trims a `Content-Disposition`, is a tab, Unicode's
  `Zs` and U+200B ZERO WIDTH SPACE, checked over every scalar. It holds no newline.

### claude.ai and chatgpt.com

Read and measured on 29 September 2026. Nobody has watched a sign-in complete in a window.
That needs a person, in a debug build with a scratch home.

- `chat.openai.com` answers 308, `www.chatgpt.com` 301 and `chat.com` 307, each to the same
  path and query on `chatgpt.com`, measured with `curl`. So `Site.chatGPT` takes them as
  aliases.
- OpenAI's help centre, article 7426629, lists signing in to ChatGPT with a password or with
  Google, Microsoft or Apple, and names `auth.openai.com` among the hosts its sign-in needs.
- Third-party code that drives the sign-in, and the buttons' connection names, put the
  Microsoft and Apple sign-ins on `login.live.com`, `login.microsoftonline.com` and
  `appleid.apple.com`. The same reading has the sign-in come back through
  `chatgpt.com/api/auth`. How chatgpt.com's Microsoft and Apple buttons open their sign-in,
  in a new window, by a link or by a redirect, is not known.
- Not measured: whether email sign-in completes in a window, whether a window's session
  survives a restart, and whether Cloudflare's challenges pass WebKit's own user agent.
- The Share menu lists app extensions in Safari's toolbar and **File** menu, and in
  **File** > **Share** in Chrome and in Firefox from 92. That was read from the browsers'
  source and bug trackers, and no browser was run. Which other browsers list the extension
  is not known.

### The C# bindings

Read in the C# that uniffi-bindgen-cs v0.11.0+v0.31.0 generated from `pitboard-ffi`, and
measured by `apps/windows/Pitboard.Core.Tests` against the debug library, on 5 October 2026.
Nothing here ran on Windows.

- An exported object's class frees its Rust object from a finalizer: `~PitboardModel()`
  calls `Destroy()`. So .NET's finalizer thread can be the one that drops a
  `PitboardModel`, and its `Drop` waits on nothing.
- The first call loads the library, and before it answers, the bindings compare the
  checksum of every export, the model's constructor and methods and
  `ModelListener.changed` among them, and hand the library each foreign trait's table of
  calls.
- A trait an app implements is an interface of the trait's own name, `ModelListener`,
  `AppControl`, `Notifications` and `LocalTime`. Its error is an exception named for it,
  `PlatformException`, whose variant the app throws. An `Option<String>` it gives back is a
  `string?`.
- A record holds a list as an array, and a C# record compares arrays by reference, so two
  snapshots read alike are not equal. `ModelTests.ASnapshotCarriesWhatWasRead` measures it.
- A call's checksum is taken over what UniFFI records of it: its module, object and name,
  its arguments, the types it takes, gives and throws, and its doc comment. A record type is
  recorded by its module and name alone, so no checksum covers a record's fields. Read in
  `uniffi_macros` 0.31.2, `fnsig.rs` and `record.rs`.

### The Swift bindings

Measured with `build-xcframework.sh` and `swift test --package-path apps/macos`, with the
Swift UniFFI 0.31.2 generates and the Swift 6.4 of Xcode 27.1, on 5 October 2026.

- A method of a trait an app implements may not have a name on the generator's list of
  Swift keywords, which has `open` though Swift takes `open` as a method's name, as
  `PitboardApp`'s own `AppControl.open` is. The generator puts such a name in backticks, and
  writes the C header's table of calls through the same filter, so the header has
  `` UniffiCallbackInterfaceAppControlMethod2 _Nonnull `open`; ``, which clang refuses
  ("expected member name or ';' after declaration specifiers"), and the bindings do not
  build. Read in `uniffi_bindgen` 0.31.2: the list in `bindings/swift/gen_swift/mod.rs`, and
  `BridgingHeaderTemplate.h` naming each field through `var_name`. `AppControl`'s method that
  opens an app again is `reopen` for that reason.
- A type the bindings export that `PitboardApp` also declares is the app's own inside
  `PitboardApp`, and ambiguous in a module that imports both, as the app's tests do: with
  the bindings' `AppControl` beside the app's protocol of that name, `swift test` stopped at
  "'AppControl' is ambiguous for type lookup in this context" in `Fixtures.swift`, which
  names the app's as `PitboardApp.AppControl` since.

### The model's timers

Read on 5 October 2026 in Rust 1.98.1's standard library and in the macOS SDK of Xcode 27.1,
with Swift 6.4.

- `std::time::Instant` reads `CLOCK_UPTIME_RAW` on Apple's systems, which macOS's
  `clock_gettime(3)` says does not increment while the system is asleep. The model's timers
  run on it, so on macOS they count only the time the machine is awake.
- The Swift model's loops slept with `Task.sleep(for:)`, whose clock is `.continuous` unless
  it is given another, and `ContinuousClock` does not stop while the system is asleep. After
  a long sleep its five-minute read came as the machine woke, where the model's comes up to
  five minutes of waking time later. An app sends `Intent::Woke` as the machine wakes, which
  reads at once, as the Swift model read on `NSWorkspace.didWakeNotification`.
- The 30 seconds an app is given to quit run on `Instant` too, where the Swift model's ran
  on `ContinuousClock`: a machine put to sleep while an app is asked to quit gives it its
  30 seconds of waking time, where the Swift gave it none once the machine woke.
- On Linux the standard library reads `CLOCK_MONOTONIC`, and on Windows
  `QueryPerformanceCounter`. How either counts a sleep was not read.
