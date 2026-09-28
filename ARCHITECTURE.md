# Architecture

This file describes how pitboard's code is organised and what must stay true in it. It
also holds the measured facts about macOS, Claude Code and OpenAI's Codex CLI that the
design rests on. It changes when the layout changes or a tool build moves a fact, not with
each commit.

## Bird's eye view

pitboard switches Claude Code or Codex between a person's own accounts on one machine. It
also shows how much of each account's limits is left. A switch parks the login in use and
puts another account's parked login in its place.

One crate, `pitboard-core`, does this for both tools. It reads and writes each tool's login,
keeps pitboard's index of accounts, and asks each tool's service for usage.

Two front ends use the core: the command line, `pitboard`, on macOS and Linux, and the menu
bar app on macOS 14 or later. The app calls the core through UniFFI bindings. The command
line inside the app is what the app's daily renewal runs, and what the `pitboard-app` cask
puts on `PATH`.

pitboard has no server of its own. The core sends requests only to Anthropic, for Claude
Code, and to OpenAI, for Codex.

## Code map

- `crates/pitboard-core`: the engine. Parking, switching, recovery, the stores and usage.
  The front ends reach it through `service::Pitboard`, built with a `context::Context`.
  - `provider/`: one module per tool, `claude` and `codex`, each implementing the
    `Provider` trait in `provider/mod.rs`. The trait covers where the tool keeps its login,
    whose it is, how to renew it and what it has left. Each module's `assumptions.rs` is
    that tool's register of facts.
  - `store/`: reading and writing logins. On macOS, parked logins are keychain items. On
    Linux, they are files in the vault.
  - `switch/`: every change to pitboard's index (switching, enrolling, adopting, renaming,
    forgetting, renewing, repairing, abandoning and uninstalling), and the journal that
    finishes an interrupted switch.
  - `state.rs`: `state.json`, the index of accounts and where each one's login is parked.
  - `lock.rs`: the lock Claude Code takes around credential writes, taken the same way.
  - `context.rs`: what the core takes from its environment, apart from the `PATH` that
    `schedule.rs` reads on Linux.
  - `api.rs`: the requests to Anthropic. The requests to OpenAI are in
    `provider/codex/api.rs`.
  - `status.rs`, `doctor.rs`, `statusline.rs` and `schedule.rs` serve the commands of the
    same names. `schedule.rs` installs daily renewal.
- `crates/pitboard`: the command line. Arguments, rendering for people, the man page, and
  the `--json` contract, pinned by the snapshots in `crates/pitboard/tests/snapshots`.
- `crates/pitboard-ffi`: the core as UniFFI bindings, for the app.
- `crates/uniffi-bindgen-swift`: generates the Swift bindings with exactly the UniFFI
  version the library uses. The bindings check method checksums when they load.
- `crates/pitboard-conformance`: checks a tool's register against a build of that tool.
- `apple/`: the menu bar app. The Swift package holds it as libraries its tests load
  without starting it. `PitboardKit` calls the bindings off the main thread, and
  `PitboardApp` is everything the app does.
  - `Pitboard.xcodeproj` is the app itself. Its `Pitboard` target in `App` starts
    `PitboardApp` and adds Sparkle, and `PitboardUITests` in `UITests` drives it.
  - A renewal schedule written by an app up to 0.3.0 starts the app with `renew`.
    `App/Main.swift` then replaces the process with the command line inside the app.
  - `scripts/build-xcframework.sh` builds the core and its Swift bindings for both Mac
    architectures. `scripts/build-app.sh` builds `Pitboard.app` from them with
    `xcodebuild`, with the command line inside at `Contents/Helpers/pitboard`.
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
    `Pitboard.xcodeproj` pins, because Dependabot cannot read that pin.
  - `workflows/rotation.yml` rehearses rotating the update key.
  - `actions/apple-keychain` imports the Developer ID certificate for every job that signs.
  - `scripts/` holds the EdDSA key and signature helpers, and the scripts that add Sparkle
    and the command line to the app's bill of materials.
  - `dependabot.yml` asks for weekly updates of Cargo dependencies and GitHub Actions.

## Invariants

- The core prints nothing.
- The core reads its environment in `Context::from_env`, which the command line calls. The
  app builds its own `Context`, because an app opened from Finder has none of the shell's
  environment.
- Two variables are read elsewhere. `PATH` is read when no search path was given
  (`context.rs`), and on Linux to find the program daily renewal runs (`schedule.rs`).
  `XPC_SERVICE_NAME`, which launchd sets, is read in `context.rs`.
- `pitboard-ffi` exports records, enums, one error type, the function `tools` and two
  objects, `SignIn` and `Pitboard`. Every call is synchronous and may block on the
  keychain, a lock or the network. `PitboardKit` makes each call off the main thread.
- On macOS, only `/usr/bin/security` reads or writes Claude Code's keychain item and
  pitboard's parked items. No keychain item is touched through the Security framework. The
  reason is under [macOS](#macos) in Measured facts.
- pitboard takes Claude Code's write lock, the same way Claude Code takes it, before writing
  Claude Code's login. It writes the login where it already lives.
- Codex takes no lock on `auth.json`, so a switch reads the file again before replacing
  it. Every switch reads the live login back rather than trusting its own write.
- pitboard never answers a failed keychain write by writing Claude Code's plaintext file.
  That demotion is Claude Code's to make.
- A Codex login is moved, never copied (`ParkSemantics::MoveOnly`). The parked login is
  read back before the incoming login is written. Codex's own sign-in and sign-out revoke
  the stored refresh token, so two usable copies of one login must never be at rest.
- No login moves until pitboard knows whose it is. A Claude Code login's account is asked of
  Anthropic; a Codex login's is read from its ID token.
- pitboard never renews the login in use. That is the tool's own job, and a second renewer
  would break it.
- Nothing outside `pitboard-core` writes pitboard's index. Every change goes through
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
  Renaming or removing one is. While pitboard is at 0.x, a breaking change gets a new minor
  version, as in 0.3.0, and after 1.0 a new major version.
- pitboard and each tool meet at the tool's register. What pitboard relies on about a tool
  is written there, and the conformance run checks it against the tool's newest build twice
  a week.
- pitboard and each service meet at a few requests. The list, with what each request
  carries, is in
  [What leaves your machine](https://docs.usepitboard.com/security#what-leaves-your-machine).
- The code and the release meet at a `v` tag, which `.github/workflows/release.yml` turns
  into a release. [RELEASING.md](RELEASING.md) has the procedure.

## The state file

`~/.pitboard/state.json` is pitboard's index: which accounts it knows, and where each one's
login is parked. It carries a `schema` number.

The app and the command line inside it update together. A command line installed another
way updates by its own route. So on one machine, an older pitboard can meet a file a newer
one wrote.

Reading forwards is `state::migrate`. Each schema bump adds an arm that rewrites the
document and falls through to the next. A file two versions behind comes forward in one
read.

Reading backwards is not possible. The older pitboard refuses the file and says to update
it. A bump needs a test that loads a file the previous version wrote.

Schema 4 records each account's tool, and which account is signed in for each tool. A
schema 3 file is brought forward on its first read, with no keychain item or vault file
touched.

A file naming a tool this build does not know is reported as written by a newer pitboard,
not as corrupt. The advice for a corrupt file is to delete it, and following that here would
orphan every parked login.

## Tool registers

Every fact pitboard relies on about a tool was read out of one build of that tool. Each tool
keeps its facts in a register, `crates/pitboard-core/src/provider/<tool>/assumptions.rs`,
dated with the build they were read from.

A fact names literals a build must contain (`probe`), or literals whose arrival would
disprove it (`absent`). A fact about behaviour, such as the 30 second cache, names none.
`pitboard-conformance` reports which facts can still be read from a build, which have
moved, and which name nothing to look for.

The check is shallow on purpose. A literal being present does not prove the behaviour around
it is unchanged. A literal disappearing, or a ruled-out one appearing, does prove something
moved.

On 22 September 2026, the twelve facts the Claude Code register then held were checked
against six builds. They could be read from 2.1.273 to 2.1.278, and the check went red on
2.1.124. That build predates the credential write lock, two of the five account-scoped keys
and the keychain error classification. `no_keyring_off_macos` and
`plaintext_credential_mode` were added after that measurement.

`.github/workflows/conformance.yml` checks the newest build of each tool against its
register on Mondays and Thursdays, or a version given by hand. Its most recent run says
which facts can still be read from the build it checked, and which have moved.

Adding a tool takes three things: a register read out of a named build, a module under
`provider/` implementing `Provider`, and a conformance job. `ProviderId`, `ProviderId::ALL`
and the matches in `provider::of` and `assumptions::of` name every tool. The compiler and
the tests then point at what an added tool has to fill in.

How to run the checker and add a fact is in
[Tool registers in CONTRIBUTING.md](CONTRIBUTING.md#tool-registers).

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
  would slow Claude Code for good. pitboard writes a large login on the argument line
  instead, where `ps` can see it for the length of one call.
- Undoing the framework's change needs `security set-key-partition-list`, which asks for the
  keychain password.
- APFS keeps a directory's mtime in nanoseconds, but not exactly: setting one and reading it
  straight back gives a value 18 to 60 nanoseconds away. A lock that remembered the value it
  asked for would abandon every switch, so `lock.rs` keeps the value read back.
- pitboard lists its parked items with `security dump-keychain` without `-d`. It never
  prompts, and emits attributes only, no secret of any item. It exits 0 in 0.06 seconds
  against a keychain of 362 items.
- Reads after a `dump-keychain` take the usual 0.016 seconds, so listing has none of the
  access-list cost of an in-process read. Each service name is on a line of the form
  `"svce"<blob>="<name>"`.

### Claude Code

Read against Claude Code 2.1.278's own storage layer, unless a fact gives its own date. The
conformance runs of 24 and 28 September 2026, on 2.1.281 and 2.1.283, reported three of
these moved. They are the keychain write route, the keychain read's exit codes, and the
absence of a Linux keyring backend.

- A running session serves its login from a 30 second cache, so it picks up a switch within
  about 33 seconds.
- Every write of the login takes proper-lockfile's directory lock at
  `<storage dir>/.storage-write`: stale after 15000 ms, ten retries, 100 ms to 1000 ms of
  backoff. `lock.rs` carries the same numbers.
- Every write under that lock drops the read cache, reads the login again inside the lock,
  and abandons the write when that read fails. A stale account cannot be written back.
- Claude Code treats its own lock going missing as a warning and keeps writing. pitboard
  cannot expect the other side to stop.
- A write can be marked as already locked without the lock being taken. `/logout` does this
  after retrying for 7.5 seconds, and deletes the login with no lock held.
- In 2.1.278, the keychain write is `security -i` while the command is at most 4032 bytes.
  Past that it is `add-generic-password -U -a <account> -s <service> -X <hex>`, on the
  argument line. It has a 2 second timeout, and only a timeout counts as retryable.
- The login goes hex-encoded, two characters a byte, so standard input carries about 2 KB of
  it. In 2.1.278, a larger login is on Claude Code's own argument line at every token
  refresh. pitboard keeps its `security -i` command within the same 4032 bytes, the limit
  its messages give.
- In 2.1.278, the keychain read is `find-generic-password -a <account> -w -s <service>`.
  Exit 0 with nothing, or 44, means absent; 36 means a locked keychain, unreadable and not
  empty. `security show-keychain-info` exiting 36 is the same signal for the whole keychain.
- The live chain is the keychain, with the plaintext file `.credentials.json` behind it.
  The successor backend, behind the `tengu_hover_rest` flag, replaces only the fallback
  half, and only for a caller that hands one in. An ordinary `claude` still reads the
  keychain first.
- Claude Code demotes to the plaintext file when a keychain write fails for good, and
  deletes the keychain item when it does.
- The supervisor daemon records itself in `<config dir>/daemon.lock`, with its pid and the
  Claude Code version that started it. It leaves the file behind when it stops.
- In 2.1.278, Claude Code has two guarded credential stores: the macOS keychain, and the
  Windows credential manager behind the `tengu_windows_credman` flag. A search of the whole
  bundle finds no `libsecret`, `org.freedesktop.secrets`, `gnome-keyring` or
  `SecretService`.
- `secret-tool` and `kwallet-query` do appear in the bundle, in the credential helpers its
  Bash sandbox keeps out of a shell. So neither name is used to look for a keyring backend.
- In 2.1.278 on Linux, the keychain backend's `security` call fails, and the plaintext file
  holds the login. Claude Code writes it, then sets its mode to 0600, and pitboard's
  `PlainUnix` host matches that.
- The register holds that absence as `no_keyring_off_macos`, and the conformance run looks
  for each of those names in every build it checks. A fact that rests on something not
  existing is wrong the moment it does.
- A Claude Code parked login holds the account's slice of Claude Code's credential
  document. On one real account, measured on 22 September 2026, the slice was 524 bytes
  against 506 for the OAuth block alone. An account holding a device token has not been
  measured.
- Claude Code's config file can be a day behind the login it describes, so pitboard asks
  Anthropic whose a login is. This was written down on 21 September 2026, with no build
  named.
- On 22 September 2026, the machine measured sat within 0.75 seconds of the `Date` header
  of api.anthropic.com across eight requests. `Date` has a granularity of one second.
- That spread is inside the noise, so pitboard keeps no estimate of clock skew. A renewal's
  expiries are counted from the `Date` of the answer that carried them.

### Codex

Read against codex-cli 0.154.0: the binary, its public source at tag `rust-v0.154.0`, and a
real `auth.json` that build wrote. The register is `provider/codex/assumptions.rs`.

- The login is `$CODEX_HOME/auth.json`, by default `~/.codex/auth.json`, at mode 0600.
- `file` is the packaged default store on every platform, and the only one pitboard
  supports. `keyring`, `auto` and `ephemeral` are the others.
- `keyring` and `auto` keep the login in a keychain item, `Codex Auth`, that Codex makes
  through the Security framework. With either of those, the `secret_auth_storage` feature
  keeps it in `secrets/codex_auth.age` instead, under a keychain key.
- Those items trust only `codex`. A read by another program brings up a permission prompt,
  and choosing **Always Allow** would change Codex's item. So pitboard refuses `keyring`
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
- Codex writes `auth.json` with no lock of any kind, so there is none for pitboard to share.
- The ID token names the account: `email`, and under `https://api.openai.com/auth`,
  `chatgpt_account_id` and `chatgpt_user_id`. A Team or Business workspace shares one
  `chatgpt_account_id`, and `chatgpt_user_id` is the person.
- pitboard identifies a Codex account by that pair, with no network call.
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
- `CODEX_HOME` moves everything Codex keeps, and an empty one means unset. pitboard's own
  sign-in, the one `enroll --sign-in` runs, sets it to a directory that exists, and runs
  `codex login` from inside it.
- The sign-in starts inside that directory because Codex also reads `.codex/config.toml`
  from a trusted project it starts in. That file could name a keychain store.
- `codex login` revokes the login stored in its home before signing in. It opens the
  browser itself and reads nothing from standard input.
