//! Pitboard's core for its native apps, as UniFFI bindings.
//!
//! An app makes one `PitboardModel`, sends it what was asked of it, and shows the snapshots
//! its listener is told of. Nothing the model exports waits on the core: `launch.rs` makes
//! the core from what the app was started with, and only the model's own threads call it.
//! The free functions answer at once from what they are given, apart from `can_run`, which
//! asks the file system about one path, `download_destination`, which asks it whether each
//! name it tries is taken, and `find_command_line`, which looks along a search path and is
//! called off the main thread. Timestamps are epoch seconds.
//!
//! The model and what it presents came from the macOS app's Swift. Where a comment here
//! names a Swift file the app no longer has, such as `AppModel.swift`, `MachineModel.swift`
//! or `Notifier.swift`, or a test in `AppModelTests.swift`, `PresentationTests.swift` or
//! `MenuTests.swift`, it means that file as it was before the app ran on the model, at
//! commit 277539b. The `AppModelTests.swift` the app's package has since is another. The
//! account windows' bookkeeping moved later, and a comment that names its Swift, such as
//! `LinkInbox.swift`, `StoreJanitor.swift`'s sweep or `AccountPickerTests.swift`, means it as
//! it was at commit a3e5ce0, where the comment says so.

use pitboard_core::context::Environment;
use pitboard_core::provider::ProviderId;
use pitboard_core::{doctor, switch, usage, words};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

uniffi::setup_scaffolding!();

mod sites;
pub use sites::{
    Conjunction, LinkRefusal, Site, SiteLink, link_refusal_reason, pitboard_link,
    read_pitboard_link, site_link, site_names, sites, sites_for,
};

mod model;
pub use model::{
    AppControl, AppLaunch, DownloadEnd, EarlierPreferences, EarlierWindowRecords, Failure, Intent,
    LastSwitch, LocalTime, ModelListener, Notifications, Pane, PitboardModel, PlatformError,
    QuitQuestion, ReadFailure, RestartNeeded, RunOutNotice, RunningSignIn, Sheet, Snapshot,
    WindowRequest, WindowsLaunch,
};

// The fixtures, which the `fixture` feature compiles; what is exported of them is the same
// in every build.
mod fixture;
pub use fixture::{FixtureError, fixture_names, fixture_page};

mod present;
pub use present::{
    AccountItem, AccountSection, AccountWindowsShown, AccountsShown, ActivityLine, ActivityShown,
    CheckLine, ChecksShown, Choice, CommandLineShown, DownloadShown, DownloadState, EmptyList,
    Footing, ItemAction, ItemOffer, LimitRow, LinkPicker, MachineShown, MenuBarText, MenuEntry,
    MenuNotices, NoticeAction, OpenWindow, PageLoad, PanelNotice, PickerAccount, PickerShown,
    Question, RenewalShown, ScheduleShown, SetupStep, Severity, SheetText, SheetTool,
    SigningInText, StoreDeletion, WaitingShown, WindowOffer, WindowWaiting,
    downloads_quit_question, name_to_save,
};

mod account_windows;
pub use account_windows::{
    AlertText, Asker, FrameOrigin, NavigationDecision, NavigationPolicy, NavigationRequest,
    NavigationTarget, PagePermission, PageRole, ProcessEnded, ResponseDecision, ResponseFacts,
    SignInWindowSize, SiteMenu, WindowAccount, WindowNoteKind, after_content_process_ended,
    decide_navigation, decide_response, dialog_title, download_destination, download_host,
    download_question, frame_asker, is_site_page, opening_note, page_may_close, page_may_use,
    remove_data_alert, sign_in_window_size, site_menus, store_id, window_accounts, window_address,
    window_home, window_note, window_of_store,
};

// The core the model's lanes call, made from what the app was started with, and what it
// answers that only the model reads. None of it is exported.
mod launch;
pub(crate) use launch::{
    Adoption, AppCore, Change, Enrolled, EnrolledAs, Holding, OwnCommandLine, PitboardError,
    Remedy, SignInSession, Switch, Switched,
};
// What a test or a fixture makes the app's core of, in place of the environment.
#[cfg(any(test, feature = "fixture"))]
pub(crate) use launch::{ASK_AGAIN_AFTER, Made};

/// A tool Pitboard handles, as the app names it to a person.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Tool {
    /// What a label's prefix and every `provider` field say: `claude`, `codex`.
    pub code: String,
    /// As its own documentation names it: `Claude Code`, `Codex`.
    pub name: String,
    /// The command that runs it, which is what a person restarts.
    pub program: String,
    /// The company behind it, which is who is asked about its accounts.
    pub service: String,
}

/// Every tool Pitboard handles, in the order a listing shows them.
#[uniffi::export]
pub fn tools() -> Vec<Tool> {
    ProviderId::ALL.iter().copied().map(tool).collect()
}

fn tool(tool: ProviderId) -> Tool {
    Tool {
        code: tool.code().into(),
        name: tool.name().into(),
        program: tool.program().into(),
        service: tool.service().into(),
    }
}

/// The `pitboard` a terminal would run.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum FoundCommandLine {
    /// The app's own, at this path or linked to from it.
    Bundled { path: String },
    /// Another install, at this path.
    Another { path: String },
    /// None anywhere a terminal would look.
    Nowhere,
}

/// Where each way of installing Pitboard puts `pitboard` under `home`, and where the
/// system's package managers put programs, looked in after the login shell's `PATH`.
#[uniffi::export]
pub fn command_line_places(home: String) -> Vec<String> {
    pitboard_core::app::command_line_places(Path::new(&home))
        .iter()
        .map(|place| place.to_string_lossy().into_owned())
        .collect()
}

/// What a tool's sign-in has printed so far comes to, for the sheet that shows it running.
#[derive(Debug, PartialEq, uniffi::Record)]
pub struct SignInView {
    /// The address the tool printed for a person to open when the browser did not open by
    /// itself, as printed, to make a link of.
    pub url: Option<String>,
    /// Whether to offer a field for the code the browser shows, which the tool is waiting
    /// to have typed back.
    pub wants_code: bool,
}

/// What a sign-in of `provider`, a `Tool`'s `code`, has printed so far, `said`, comes to.
/// `pasted` is whether a code has been typed back already, after which none is asked for.
///
/// Each tool's own module reads its own words, so both apps show the same. It reads only
/// what it is given and answers at once, on any thread. A provider nobody knows offers
/// nothing.
#[uniffi::export]
pub fn sign_in_view(provider: String, said: String, pasted: bool) -> SignInView {
    let read = pitboard_core::provider::ProviderId::parse(&provider)
        .map(|tool| pitboard_core::provider::sign_in_view(tool, &said, pasted))
        .unwrap_or_default();
    SignInView {
        url: read.url,
        wants_code: read.wants_code,
    }
}

/// The first `pitboard` a terminal would run: on `search_path`, in `PATH`'s form, then in
/// `places`; and whether it is the command line at `helper`, the app's own, once every link
/// is followed. Looks at the file system, so call it off the main thread. The model looks on
/// the login shell's `PATH` its core read, which no export gives, so a caller passes a
/// search path of its own, as the C# tests do.
#[uniffi::export]
pub fn find_command_line(
    search_path: Option<String>,
    places: Vec<String>,
    helper: Option<String>,
) -> FoundCommandLine {
    let places: Vec<PathBuf> = places.into_iter().map(PathBuf::from).collect();
    found_command_line(pitboard_core::app::find_command_line(
        search_path.as_deref().map(std::ffi::OsStr::new),
        &places,
        helper.as_deref().map(Path::new),
    ))
}

pub(crate) fn found_command_line(found: pitboard_core::app::CommandLine) -> FoundCommandLine {
    let shown = |path: PathBuf| path.to_string_lossy().into_owned();
    match found {
        pitboard_core::app::CommandLine::Bundled(path) => {
            FoundCommandLine::Bundled { path: shown(path) }
        }
        pitboard_core::app::CommandLine::Another(path) => {
            FoundCommandLine::Another { path: shown(path) }
        }
        pitboard_core::app::CommandLine::Nowhere => FoundCommandLine::Nowhere,
    }
}

/// The command line the app at `app` comes with, which its renewal schedule runs. `None` for
/// anything that is not an app, such as a test or a build directory.
#[uniffi::export]
pub fn app_command_line(app: String) -> Option<String> {
    pitboard_core::app::app_command_line(Path::new(&app))
        .map(|path| path.to_string_lossy().into_owned())
}

/// Whether `path` is a program this user may run, as the core judges every program it finds:
/// a regular file, once every link is followed, that this user may execute. Asks the file
/// system about that one path.
#[uniffi::export]
pub fn can_run(path: String) -> bool {
    pitboard_core::app::can_run(Path::new(&path))
}

/// The home an app started with `environment` has, as the core reads it: `HOME`, or this
/// account's own where it is unset.
#[uniffi::export]
pub fn home_directory(environment: HashMap<String, String>) -> String {
    let environment: Environment = environment.into_iter().collect();
    environment.home().to_string_lossy().into_owned()
}

/// The Pitboard directory of an app started with `environment`, as the core reads it:
/// `PITBOARD_HOME`, or `.pitboard` in the home. The path as the environment gives it, with
/// any `.`, `..` or trailing `/` it holds.
#[uniffi::export]
pub fn pitboard_directory(environment: HashMap<String, String>) -> String {
    let environment: Environment = environment.into_iter().collect();
    environment.pitboard_home().to_string_lossy().into_owned()
}

/// Something to know about that did not stop the operation.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Warning {
    pub code: String,
    pub message: String,
}

/// An interrupted switch that was given up on, keeping every login it named.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Abandoned {
    pub from: String,
    pub to: String,
    /// Copies kept rather than deleted, because which one is live is now unknown.
    pub logins_kept: u32,
}

/// What renewing every due parked login came to.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Renewed {
    /// As a person types it: bare for Claude Code, `codex/work` for Codex.
    pub label: String,
    /// Which tool's login it is.
    pub provider: String,
    /// `renewed`, `renewal_deferred`, `parked_login_refused`, or the code of a failure.
    pub outcome: String,
}

/// Whether anything keeps parked logins alive on this machine without a command being run.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Schedule {
    /// The platform's own scheduler runs `pitboard renew` every `every_seconds`.
    Installed { path: String, every_seconds: u32 },
    /// Nothing does. Parked logins are renewed when Pitboard runs, and otherwise not.
    Absent,
    /// This platform has no scheduler Pitboard knows how to write.
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Source {
    Live,
    ClaudeCodeCache,
    Remembered,
}

/// One limit an account is measured against, such as the five-hour session or the week.
/// Named for what it is rather than `Window`, which SwiftUI and WinUI each have a type of.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Limit {
    /// The service's own name for it: Anthropic's `session`, `weekly_all` or
    /// `weekly_scoped`, or one named after its length for OpenAI.
    pub kind: String,
    /// How long the window runs, where that is known. The way to name a window to a person
    /// whatever its service called it.
    pub length_seconds: Option<i64>,
    /// The model a scoped limit applies to.
    pub scope: Option<String>,
    /// Share already used; past 100 once exceeded.
    pub percent: f64,
    pub resets_at: Option<i64>,
    /// How Anthropic grades this row, when it grades it.
    pub severity: Option<String>,
    /// Whether this limit is one the account is working against now.
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Usage {
    pub source: Source,
    pub observed_at: Option<i64>,
    pub windows: Vec<Limit>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Parked {
    pub parked_at: i64,
    pub access_expires_at: Option<i64>,
    pub refresh_expires_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Account {
    /// Unique among the accounts of one status, and stable between two: the tool and the
    /// account, or the tool alone for a login that belongs to no account Pitboard can name.
    /// Two tools' accounts can share a label, so a label cannot be an identity.
    pub id: String,
    /// Which tool the account is for, as a `Tool`'s `code`.
    pub provider: String,
    /// `None` for an account signed in but not enrolled.
    pub label: Option<String>,
    /// The label with its tool, `claude/work` or `codex/work`: what to pass back to switch
    /// to, forget or rename it, which names exactly one account whatever else is enrolled.
    /// `None` exactly when `label` is.
    pub qualified: Option<String>,
    /// A login of this tool is there and belongs to no account Pitboard can name: one it
    /// could not read, or one it cannot switch, such as an API key. Not an account to enrol.
    pub unplaced: bool,
    pub email: String,
    pub account_uuid: String,
    pub signed_in: bool,
    /// Whether switching to it would work now.
    pub switchable: bool,
    pub parked: Option<Parked>,
    pub usage: Option<Usage>,
    /// Why the usage is not live, when it is not.
    pub stale: Option<String>,
    /// What to tell a person about `stale`, when it is worth a word.
    pub stale_explanation: Option<String>,
    /// How long this account lasts, in seconds: until its tightest limit fills at the rate
    /// it has been filling, or until that limit resets, whichever comes first.
    ///
    /// `None` until there is enough to go on. A wrong runway tells somebody to switch when
    /// they need not, which is worse than none.
    pub lasts_seconds: Option<i64>,
    /// Whether `lasts_seconds` is a limit filling or a limit resetting, which is the
    /// difference between "about an hour left" and "whole again in an hour".
    pub lasts_burning: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Status {
    pub now: i64,
    /// The signed-in account first.
    pub accounts: Vec<Account>,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Check {
    pub code: String,
    pub name: String,
    pub level: Level,
    pub detail: String,
    /// Empty when there is nothing to do.
    pub advice: String,
}

// What the apps show as the core says or decides it, as free functions of the records the
// bindings export: the sentences, column words and usage level of `pitboard_core::words`,
// and `usage::same_reset`, the rule merging readings follows. Each wraps the core's function
// of the same name. The command line calls those directly wherever it says the same thing,
// and never these. No app calls these either: `present/` says what an app shows of them as
// it makes the snapshot, through the core's functions or these, and only the C# tests call
// them across the bindings. None reads a clock, a file or the keychain, so one may be called
// on any thread.

/// A limit in the column form, beside its bar: "5h", "week", "30m", "week · Fable".
#[uniffi::export]
pub fn limit_column(limit: Limit) -> String {
    words::limit_column(&limit.kind, limit.length_seconds, limit.scope.as_deref())
}

/// A limit in the sentence form, without its scope: "5-hour", "weekly", "daily".
#[uniffi::export]
pub fn limit_name(limit: Limit) -> String {
    words::limit_name(&limit.kind, limit.length_seconds)
}

/// How much of a limit is used, in the three steps its colour changes at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UsageLevel {
    /// Under 70%.
    Plenty,
    /// From 70%.
    Low,
    /// From 90%, and past 100%.
    Out,
}

/// The step a limit is at, from a `Limit`'s `percent`, which passes 100 when a service
/// reports more used than the limit.
#[uniffi::export]
pub fn usage_level(percent: f64) -> UsageLevel {
    match words::usage_level(percent) {
        words::UsageLevel::Plenty => UsageLevel::Plenty,
        words::UsageLevel::Low => UsageLevel::Low,
        words::UsageLevel::Out => UsageLevel::Out,
    }
}

/// When a limit resets, as of `now`: "resets in 2h 05m", or "resetting now" once it is due.
#[uniffi::export]
pub fn resets(resets_at: i64, now: i64) -> String {
    words::resets(resets_at, now)
}

/// How long an account lasts, from an `Account`'s `lasts_seconds` and `lasts_burning`:
/// "about 1h 30m left at this rate", "resets in 1h 30m", and under a minute "about to run
/// out" or "resets any moment". `None` where `lasts_seconds` is, until there is enough to go
/// on.
#[uniffi::export]
pub fn runway(seconds: Option<i64>, burning: bool) -> Option<String> {
    use pitboard_core::history::Runway;
    words::runway(match seconds {
        None => Runway::Unknown,
        Some(seconds) if burning => Runway::Burning(seconds),
        Some(seconds) => Runway::Resting(seconds),
    })
}

/// How long a parked login stays usable, as of `now`, in the sentence form: "Parked login
/// good for 20 more days". `None` when nothing says.
#[uniffi::export]
pub fn parked_life(parked: Option<Parked>, now: i64) -> Option<String> {
    words::parked_life(parked?.refresh_expires_at, now)
}

/// What a renewal run did, from what renewing each due login came to: "No parked login was
/// due.", "Renewed one.", "Renewed 1 of 2; the rest are tried again next time.". No export
/// answers a `Renewed` now that the model renews, so a caller makes those it passes, as the
/// C# tests do.
#[uniffi::export]
pub fn renewal_note(renewals: Vec<Renewed>) -> String {
    renewal_note_of(&renewals)
}

/// `renewal_note`, of renewals the model holds.
pub(crate) fn renewal_note_of(renewals: &[Renewed]) -> String {
    let renewed = renewals
        .iter()
        .filter(|r| r.outcome == switch::Renewal::Renewed.code())
        .count();
    words::renewal_note(renewals.len(), renewed)
}

/// The line over a diagnosis's checks: what is worth looking at while checks only warn,
/// and not to switch accounts while one fails. No export answers a `Check` now that the
/// model runs doctor's checks, so a caller makes those it passes, as the C# tests do.
#[uniffi::export]
pub fn doctor_summary(checks: Vec<Check>) -> String {
    doctor_summary_of(&checks)
}

/// `doctor_summary`, of checks the model holds.
pub(crate) fn doctor_summary_of(checks: &[Check]) -> String {
    words::doctor_summary(checks.iter().map(|c| match c.level {
        Level::Ok => doctor::Level::Ok,
        Level::Warn => doctor::Level::Warn,
        Level::Fail => doctor::Level::Fail,
    }))
}

/// Whether two resets of a limit are one, as the core counts them when it merges readings:
/// less than a minute apart, in either order. A session is given a reset in whole seconds
/// and Anthropic's answer a fraction that is dropped, so one window can come back a second
/// apart.
#[uniffi::export]
pub fn same_reset(between: i64, and: i64) -> bool {
    usage::same_reset(between, and)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sign-in is read by its own tool's module, named by its code as every `provider`
    /// field names it, and one nobody knows offers nothing rather than another tool's
    /// reading.
    #[test]
    fn a_sign_in_is_read_by_its_own_tools_module() {
        let said = "If the browser didn't open, visit: https://claude.com/cai/oauth/authorize?x\n\
                    Paste code here if prompted > ";
        let address = Some("https://claude.com/cai/oauth/authorize?x".to_string());
        assert_eq!(
            sign_in_view("claude".into(), said.into(), false),
            SignInView {
                url: address.clone(),
                wants_code: true
            }
        );
        assert!(!sign_in_view("claude".into(), said.into(), true).wants_code);
        assert_eq!(
            sign_in_view("codex".into(), said.into(), false),
            SignInView {
                url: address,
                wants_code: false
            }
        );
        assert_eq!(
            sign_in_view("gemini".into(), said.into(), false),
            SignInView {
                url: None,
                wants_code: false
            }
        );
    }

    fn limit(kind: &str, length_seconds: Option<i64>, scope: Option<&str>) -> Limit {
        Limit {
            kind: kind.into(),
            length_seconds,
            scope: scope.map(str::to_owned),
            percent: 42.0,
            resets_at: None,
            severity: None,
            is_active: true,
        }
    }

    /// The apps name a limit from the record they were given, as the command line names
    /// the reading it came from.
    #[test]
    fn a_limit_is_named_from_its_record() {
        let fable = limit("weekly_scoped", Some(604_800), Some("Fable"));
        assert_eq!(limit_column(fable), "week · Fable");
        let fable = limit("weekly_scoped", Some(604_800), Some("Fable"));
        assert_eq!(
            limit_name(fable),
            "weekly",
            "a sentence places the scope itself"
        );
        assert_eq!(limit_column(limit("90_minute", Some(5_400), None)), "90m");
        assert_eq!(limit_name(limit("session", None, None)), "5-hour");
    }

    /// An account's runway reaches the apps as seconds and whether its limit is filling,
    /// and reads the way `pitboard status` says it. Without the seconds there is nothing to
    /// say, whichever way the account is going.
    #[test]
    fn a_runway_is_said_from_an_accounts_two_fields() {
        assert_eq!(
            runway(Some(5_400), true).as_deref(),
            Some("about 1h 30m left at this rate")
        );
        assert_eq!(
            runway(Some(3_900), false).as_deref(),
            Some("resets in 1h 05m")
        );
        assert_eq!(runway(Some(30), true).as_deref(), Some("about to run out"));
        assert_eq!(
            runway(Some(30), false).as_deref(),
            Some("resets any moment")
        );
        assert_eq!(runway(None, true), None);
        assert_eq!(runway(None, false), None);
    }

    /// Only a renewal that renewed counts as one: a deferred or refused one was due and
    /// was not renewed.
    #[test]
    fn a_renewal_note_counts_what_was_renewed() {
        let renewed = |outcome: &str| Renewed {
            label: "work".into(),
            provider: "claude".into(),
            outcome: outcome.into(),
        };
        assert_eq!(renewal_note(Vec::new()), "No parked login was due.");
        assert_eq!(
            renewal_note(vec![renewed("renewed"), renewed("renewal_deferred")]),
            "Renewed 1 of 2; the rest are tried again next time."
        );
        assert_eq!(
            renewal_note(vec![renewed("parked_login_refused")]),
            "1 due; none could be renewed this time."
        );
    }

    /// A check that warns is worth looking at, and one that fails outweighs every warning.
    #[test]
    fn the_doctor_summary_says_not_to_switch_while_a_check_fails() {
        let check = |level: Level| Check {
            code: "credential".into(),
            name: "credential".into(),
            level,
            detail: String::new(),
            advice: String::new(),
        };
        assert_eq!(
            doctor_summary(vec![check(Level::Ok)]),
            "Everything Pitboard checks is in order."
        );
        assert_eq!(
            doctor_summary(vec![
                check(Level::Warn),
                check(Level::Ok),
                check(Level::Fail)
            ]),
            "1 broken: do not switch accounts until fixed."
        );
        assert_eq!(
            doctor_summary(vec![check(Level::Warn), check(Level::Warn)]),
            "2 things are worth looking at."
        );
    }

    /// The apps read a parked login's life from the record they hold, in the sentence form.
    #[test]
    fn a_parked_logins_life_is_read_from_its_record() {
        let parked = |refresh_expires_at| Parked {
            parked_at: 0,
            access_expires_at: None,
            refresh_expires_at,
        };
        assert_eq!(parked_life(None, 1_000), None);
        assert_eq!(parked_life(Some(parked(None)), 1_000), None);
        assert_eq!(
            parked_life(Some(parked(Some(1_000 + 3 * 86_400))), 1_000).as_deref(),
            Some("Parked login good for 3 more days")
        );
    }

    /// The command line's search crosses the bindings with its answer as the core gives it.
    #[test]
    fn a_command_line_found_nowhere_is_said_to_be_nowhere() {
        assert_eq!(
            find_command_line(
                Some("/nowhere/at/all".into()),
                vec!["/nowhere/else".into()],
                None
            ),
            FoundCommandLine::Nowhere
        );
        let places = command_line_places("/Users/x".into());
        assert_eq!(places[..2], ["/Users/x/.cargo/bin", "/Users/x/.local/bin"]);
        assert_eq!(app_command_line("/Users/x/pitboard".into()), None);
    }

    /// Where an app's home and Pitboard directory are, and whether a path is a program, cross
    /// the bindings as the core reads them, so the app has no rule of its own for either.
    #[test]
    fn the_app_asks_the_core_where_things_are_and_what_runs() {
        let environment = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect()
        };
        assert_eq!(
            home_directory(environment(&[("HOME", "/Users/x")])),
            "/Users/x"
        );
        assert_eq!(
            pitboard_directory(environment(&[("HOME", "/Users/x")])),
            "/Users/x/.pitboard"
        );
        assert_eq!(
            pitboard_directory(environment(&[
                ("HOME", "/Users/x"),
                ("PITBOARD_HOME", "/elsewhere/./p/")
            ])),
            "/elsewhere/./p/"
        );
        assert!(can_run("/bin/sh".into()));
        assert!(!can_run("/bin".into()), "a directory is not a program");
        assert!(!can_run("/nowhere/at/all".into()));
    }
}
