//! The fixtures, each made on the real core: the macOS app's FixtureTests.swift, ported to
//! the worlds that replace its FixtureCore, and what each UI test reads in its world, asked
//! of the model an app launches into it. Where the real core answers otherwise than the
//! Swift fixture did, the test says so and holds what the core does.

use super::apps::CHATGPT;
use super::pages;
use super::tools::{CLAUDE_ADDRESS, CODEX_ADDRESS};
use super::worlds::{Folder, Launched, World, make};
use crate::model::{Intent, ModelListener, Pane, PitboardModel, PlatformError, Sheet, Snapshot};
use crate::present::testing::Utc;
use crate::present::{AccountsShown, Footing};
use crate::{
    Adoption, Changed, Enrolled, EnrolledAs, FoundCommandLine, Pitboard, PitboardError, Schedule,
    Status, Switch,
};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// As long as a test waits for another thread, however slow the machine running it.
const PATIENCE: Duration = Duration::from_secs(30);

/// `world`, made in a folder of this test's own.
fn made(world: World) -> Launched {
    make(world, Folder::own(world.name()).expect("a folder"))
        .unwrap_or_else(|unmade| panic!("{}: {unmade}", world.name()))
}

/// Each account the way a test reads it: its name with its tool, or the email of a login
/// with no name, then whether it is the one in use, or why it cannot be switched to.
fn described(status: &Status) -> Vec<String> {
    status
        .accounts
        .iter()
        .map(|account| {
            let name = account
                .qualified
                .clone()
                .unwrap_or_else(|| account.email.clone());
            if account.signed_in {
                format!("{name}, in use")
            } else if account.switchable {
                name
            } else {
                format!(
                    "{name}, {}",
                    account.stale.as_deref().unwrap_or("not switchable")
                )
            }
        })
        .collect()
}

/// The accounts enrolled that a read gives, each by its name with its tool and whether it is
/// in use: what an offline read and a fresh one agree on, whatever each knows of the
/// numbers. A login nobody has named is the fresh read's alone, since it asks its service
/// whose it is.
fn who(status: &Status) -> Vec<(String, bool)> {
    status
        .accounts
        .iter()
        .filter_map(|account| Some((account.qualified.clone()?, account.signed_in)))
        .collect()
}

/// The accounts in use, one per tool at most, by their names with their tools.
fn in_use(status: &Status) -> Vec<String> {
    status
        .accounts
        .iter()
        .filter(|account| account.signed_in)
        .filter_map(|account| account.qualified.clone())
        .collect()
}

/// The code a call was refused with, or `None` when it was not refused.
fn refusal<T>(answer: Result<T, PitboardError>) -> Option<String> {
    match answer {
        Ok(_) => None,
        Err(PitboardError::Failed { code, .. }) => Some(code),
    }
}

fn read(core: &Pitboard) -> Status {
    core.status(true).expect("a read")
}

/// Runs a sign-in to the end the way the sheet does: reads everything the tool says, types a
/// code back once it asks for one, and finishes.
fn sign_in_to_the_end(core: &Pitboard, label: &str) -> Result<Enrolled, PitboardError> {
    let session = core.sign_in(label.into())?;
    while let Some(line) = session.next_line() {
        if line.contains("Paste code") {
            session.paste("fixture-code".into())?;
        }
    }
    session.finish()
}

// Where each fixture starts.

/// The UI tests launch the app into these fixtures and assert on what each starts with, so
/// a change here is a change to what they test: who a read shows, who the offline read
/// shows, and which tools were found. Read on the real core, a read whose service cannot be
/// reached still answers, each account saying so, and an interrupted switch is not said by a
/// read at all: the Swift fixture failed both reads, `unreachable` and
/// `recovery_undetermined`, which the core never does.
///
/// FixtureTests.swift's eachFixtureStartsWhereItsTestsExpect.
#[test]
fn each_fixture_starts_where_its_tests_expect() {
    let work = "claude/work, in use";
    for world in World::ALL {
        let expected: Vec<&str> = match world {
            World::TwoTools | World::ChatGptOpen => vec![
                work,
                "claude/old, parked_access_expired",
                "claude/personal",
                "codex/main, in use",
                "codex/spare",
            ],
            World::OneTool | World::ReadFailure | World::Stuck => vec![work, "claude/personal"],
            World::OnlyOne => vec![work],
            World::Unnamed => vec!["dana@work.example, in use"],
            World::Empty | World::FirstLaunch | World::NoClaudeCode => Vec::new(),
        };
        let launched = made(world);
        let core = &launched.core;
        let fresh = read(core);
        assert_eq!(described(&fresh), expected, "{}", world.name());
        let offline = core.status_offline().expect("what is known");
        assert_eq!(who(&offline), who(&fresh), "{}", world.name());

        let unreachable = fresh
            .accounts
            .iter()
            .filter(|account| account.stale.as_deref() == Some("unreachable"))
            .count();
        assert_eq!(
            unreachable,
            if world == World::ReadFailure { 2 } else { 0 },
            "{}",
            world.name()
        );
        if world == World::ReadFailure {
            assert!(
                fresh.accounts.iter().all(|account| account.usage.is_some()),
                "the last numbers measured"
            );
        }
        assert_eq!(
            crate::tools()
                .iter()
                .map(|tool| tool.code.as_str())
                .collect::<Vec<_>>(),
            ["claude", "codex"]
        );
        let installed: Vec<String> = core.installed().into_iter().map(|tool| tool.code).collect();
        let expected: &[&str] = if world == World::NoClaudeCode {
            &[]
        } else {
            &["claude", "codex"]
        };
        assert_eq!(installed, expected, "{}", world.name());
    }
}

/// The UI tests launch a fixture by its name, from a list of their own, so a name changed
/// here has to change there.
///
/// FixtureTests.swift's theUITestsNameEveryFixtureAsTheAppDoes, but for the variable, which
/// is the app's to read.
#[test]
fn the_ui_tests_name_every_fixture_as_the_app_does() {
    assert_eq!(
        super::fixture_names(),
        [
            "twoTools",
            "oneTool",
            "empty",
            "firstLaunch",
            "noClaudeCode",
            "unnamed",
            "onlyOne",
            "readFailure",
            "stuck",
            "chatGPTOpen",
        ]
    );
}

// Changing accounts.

/// A switch moves who is in use within the tool it is for and leaves the other tool alone,
/// and the account it left can be switched back to. It says what running sessions do:
/// Claude Code's follow within the 33 seconds the core measured, where the Swift fixture
/// said 45, and Codex's keep the old account until they are started again. The account
/// already in use is not switched to again, and an account nobody enrolled is refused.
///
/// FixtureTests.swift's aSwitchMovesWhoIsInUseWithinItsOwnTool.
#[test]
fn a_switch_moves_who_is_in_use_within_its_own_tool() {
    let launched = made(World::TwoTools);
    let core = &launched.core;

    let claude = core.switch_to("claude/personal".into()).expect("a switch");
    assert!(
        matches!(
            claude.outcome,
            Switch::Switched {
                ref provider,
                adoption: Adoption::Follows { within_seconds: 33 },
                ..
            } if provider == "claude"
        ),
        "{:?}",
        claude.outcome
    );
    let after_claude = read(core);
    assert_eq!(in_use(&after_claude), ["claude/personal", "codex/main"]);
    assert!(
        after_claude
            .accounts
            .iter()
            .any(
                |account| account.qualified.as_deref() == Some("claude/work") && account.switchable
            )
    );

    let codex = core.switch_to("codex/spare".into()).expect("a switch");
    assert_eq!(
        codex.outcome,
        Switch::Switched {
            provider: "codex".into(),
            from: "codex/main".into(),
            to: "codex/spare".into(),
            adoption: Adoption::Restart {
                program: "codex".into()
            },
        }
    );
    let after_codex = read(core);
    assert_eq!(in_use(&after_codex), ["claude/personal", "codex/spare"]);
    assert!(
        after_codex
            .accounts
            .iter()
            .any(|account| account.qualified.as_deref() == Some("codex/main")
                && account.switchable)
    );

    assert_eq!(
        core.switch_to("claude/personal".into())
            .expect("already in use")
            .outcome,
        Switch::AlreadyActive {
            label: "personal".into()
        }
    );
    assert!(refusal(core.switch_to("claude/nobody".into())).is_some());
    assert_eq!(in_use(&read(core)), ["claude/personal", "codex/spare"]);
}

/// A parked login that expired stays unusable until somebody signs in to it again. A switch
/// to it is refused and moves nothing, and a switch between two other accounts of its tool
/// leaves it as it was, still offering a sign-in rather than a switch.
///
/// FixtureTests.swift's anExpiredParkedLoginStaysUnusableAcrossSwitches.
#[test]
fn an_expired_parked_login_stays_unusable_across_switches() {
    let launched = made(World::TwoTools);
    let core = &launched.core;
    assert_eq!(
        refusal(core.switch_to("claude/old".into())).as_deref(),
        Some("parked_login_expired")
    );
    assert_eq!(in_use(&read(core)), ["claude/work", "codex/main"]);

    core.switch_to("claude/personal".into()).expect("a switch");
    assert_eq!(
        described(&read(core)),
        [
            "claude/personal, in use",
            "claude/old, parked_access_expired",
            "claude/work",
            "codex/main, in use",
            "codex/spare",
        ]
    );
}

/// The core names the accounts of a switch as the command line types them: bare for Claude
/// Code, with the tool for any other. The model matches what a switch said against that, so
/// what a Claude Code switch said is kept once the read after it has landed.
///
/// FixtureTests.swift's aSwitchNamesTheAccountsAsTheCoreTypesThem, its model half asked of
/// the model an app launches into the world.
#[test]
fn a_switch_names_the_accounts_as_the_core_types_them() {
    {
        let launched = made(World::TwoTools);
        let core = &launched.core;
        assert_eq!(
            core.switch_to("codex/main".into())
                .expect("already in use")
                .outcome,
            Switch::AlreadyActive {
                label: "codex/main".into()
            }
        );
        assert_eq!(
            core.switch_to("claude/personal".into())
                .expect("a switch")
                .outcome,
            Switch::Switched {
                provider: "claude".into(),
                from: "work".into(),
                to: "personal".into(),
                adoption: Adoption::Follows { within_seconds: 33 },
            }
        );
    }

    let launched = made(World::TwoTools);
    let (model, told) = started(&launched);
    model.send(Intent::SwitchTo {
        qualified: "claude/personal".into(),
    });
    let last = told.until("the switch, read back", |snapshot| {
        snapshot.switch_under_way.is_none()
            && !snapshot.reading
            && snapshot
                .status
                .as_ref()
                .is_some_and(|status| in_use(status).contains(&"claude/personal".into()))
    });
    assert_eq!(
        last.last_switches
            .iter()
            .map(|said| said.provider.as_str())
            .collect::<Vec<_>>(),
        ["claude"]
    );
    model.shutdown();
}

/// The login signed in with no name is named in place, and stays the account in use. A name
/// its tool already has is refused and leaves it unnamed, and once it has a name there is
/// nobody left to name.
///
/// FixtureTests.swift's theLoginSignedInNowIsNamedWithANameNotTaken.
#[test]
fn the_login_signed_in_now_is_named_with_a_name_not_taken() {
    let launched = made(World::Unnamed);
    let core = &launched.core;
    sign_in_to_the_end(core, "claude/personal").expect("signed in");
    assert_eq!(
        refusal(core.enroll_current("personal".into())).as_deref(),
        Some("label_taken")
    );
    assert_eq!(
        described(&read(core)),
        ["dana@work.example, in use", "claude/personal"]
    );

    assert_eq!(
        core.enroll_current("work".into()).expect("named"),
        Enrolled {
            email: "dana@work.example".into(),
            outcome: EnrolledAs::Current,
            warnings: Vec::new(),
        }
    );
    assert_eq!(
        described(&read(core)),
        ["claude/work, in use", "claude/personal"]
    );
    assert!(refusal(core.enroll_current("home".into())).is_some());
}

/// The account in use cannot be forgotten, since its login is the one the tool is using.
/// Any other can, and goes from the list.
///
/// FixtureTests.swift's onlyAnAccountNotInUseIsForgotten.
#[test]
fn only_an_account_not_in_use_is_forgotten() {
    let launched = made(World::TwoTools);
    let core = &launched.core;
    assert_eq!(
        refusal(core.forget("claude/work".into())).as_deref(),
        Some("cannot_forget_active_account")
    );
    assert!(refusal(core.forget("claude/nobody".into())).is_some());

    let forgotten: Changed = core.forget("codex/spare".into()).expect("forgotten");
    assert_eq!(forgotten.email, "dana@home.example");
    assert!(forgotten.warnings.is_empty());
    assert_eq!(
        described(&read(core)),
        [
            "claude/work, in use",
            "claude/old, parked_access_expired",
            "claude/personal",
            "codex/main, in use",
        ]
    );
}

/// A rename needs a name its tool has not given another account. Another tool's names do
/// not count, since a name is only ever used with its tool.
///
/// FixtureTests.swift's aRenameNeedsANameItsToolHasNotGiven.
#[test]
fn a_rename_needs_a_name_its_tool_has_not_given() {
    let launched = made(World::TwoTools);
    let core = &launched.core;
    assert_eq!(
        refusal(core.rename("claude/personal".into(), "old".into())).as_deref(),
        Some("label_taken")
    );

    let renamed = core
        .rename("claude/personal".into(), "main".into())
        .expect("renamed");
    assert_eq!(renamed.email, "dana@home.example");
    assert!(renamed.warnings.is_empty());
    assert_eq!(
        described(&read(core)),
        [
            "claude/work, in use",
            "claude/old, parked_access_expired",
            "claude/main",
            "codex/main, in use",
            "codex/spare",
        ]
    );
}

// Signing in.

/// Claude Code's sign-in prints the address to open and asks for the code from the browser,
/// which the sheet shows as a link and a field, and goes no further until a code is typed
/// back. Finishing then parks the new account beside the one in use.
///
/// FixtureTests.swift's aClaudeCodeSignInWaitsForTheCodeAndThenParksTheAccount.
#[test]
fn a_claude_code_sign_in_waits_for_the_code_and_then_parks_the_account() {
    let launched = made(World::OneTool);
    let core = &launched.core;
    let session = core.sign_in("claude/travel".into()).expect("started");
    let mut said = String::new();
    while let Some(line) = session.next_line() {
        said.push_str(&line);
        if line.contains("Paste code") {
            break;
        }
    }
    let shown = crate::sign_in_view("claude".into(), said, false);
    assert_eq!(shown.url.as_deref(), Some(CLAUDE_ADDRESS));
    assert!(shown.wants_code);

    let asked = Instant::now();
    let pasting = {
        let session = Arc::clone(&session);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            session.paste("fixture-code".into())
        })
    };
    assert_eq!(session.next_line(), None);
    assert!(
        asked.elapsed() >= Duration::from_millis(300),
        "nothing more until the code is typed"
    );
    pasting.join().expect("the paste").expect("typed back");

    assert_eq!(
        session.finish().expect("enrolled"),
        Enrolled {
            email: "travel@example.com".into(),
            outcome: EnrolledAs::SignedIn,
            warnings: Vec::new(),
        }
    );
    assert_eq!(
        described(&read(core)),
        ["claude/work, in use", "claude/personal", "claude/travel"]
    );
}

/// Codex's sign-in prints the address to open beside the loopback address the browser comes
/// back to, reads nothing typed, and finishes by itself once the browser is done.
///
/// FixtureTests.swift's aCodexSignInFinishesByItself.
#[test]
fn a_codex_sign_in_finishes_by_itself() {
    let launched = made(World::TwoTools);
    let core = &launched.core;
    let session = core.sign_in("codex/travel".into()).expect("started");
    let mut said = String::new();
    while let Some(line) = session.next_line() {
        said.push_str(&line);
    }
    let shown = crate::sign_in_view("codex".into(), said, false);
    assert_eq!(shown.url.as_deref(), Some(CODEX_ADDRESS));
    assert!(!shown.wants_code);

    assert_eq!(
        session.finish().expect("enrolled"),
        Enrolled {
            email: "travel@example.com".into(),
            outcome: EnrolledAs::SignedIn,
            warnings: Vec::new(),
        }
    );
    let accounts = described(&read(core));
    assert_eq!(
        accounts[accounts.len() - 3..],
        ["codex/main, in use", "codex/spare", "codex/travel"]
    );
}

/// A sign-in stopped part way enrols nothing: it stops waiting for a code, and finishing it
/// fails as stopped, the way the tool's own does.
///
/// FixtureTests.swift's aStoppedSignInEnrolsNothing.
#[test]
fn a_stopped_sign_in_enrols_nothing() {
    let launched = made(World::OneTool);
    let core = &launched.core;
    let session = core.sign_in("claude/travel".into()).expect("started");
    let reading = {
        let session = Arc::clone(&session);
        std::thread::spawn(move || while session.next_line().is_some() {})
    };
    session.cancel();
    reading.join().expect("the reading ends");

    assert!(refusal(session.finish()).is_some());
    assert_eq!(
        described(&read(core)),
        ["claude/work, in use", "claude/personal"]
    );
}

/// Signing in again to an account whose parked login expired renews that account rather
/// than adding a second one, and it can be switched to again, with nothing wrong with it any
/// more.
///
/// FixtureTests.swift's signingInAgainToAnExpiredAccountMakesItSwitchable and
/// signingInAgainPutsAwayWhatWasWrongWithTheParkedLogin.
#[test]
fn signing_in_again_to_an_expired_account_makes_it_switchable() {
    let launched = made(World::TwoTools);
    let core = &launched.core;
    assert_eq!(
        sign_in_to_the_end(core, "claude/old").expect("signed in"),
        Enrolled {
            email: "dana@old.example".into(),
            outcome: EnrolledAs::Renewed,
            warnings: Vec::new(),
        }
    );
    let after = read(core);
    assert_eq!(
        described(&after),
        [
            "claude/work, in use",
            "claude/old",
            "claude/personal",
            "codex/main, in use",
            "codex/spare",
        ]
    );
    let old = after
        .accounts
        .iter()
        .find(|account| account.qualified.as_deref() == Some("claude/old"))
        .expect("old");
    assert_eq!((&old.stale, &old.stale_explanation), (&None, &None));
}

/// Signing in again to the account in use puts its new login in use at once, as the core
/// does, and parks nothing: it stays the account in use and is not one to switch to.
///
/// FixtureTests.swift's signingInAgainToTheAccountInUseKeepsItInUse.
#[test]
fn signing_in_again_to_the_account_in_use_keeps_it_in_use() {
    let launched = made(World::OneTool);
    let core = &launched.core;
    assert_eq!(
        sign_in_to_the_end(core, "claude/work").expect("signed in"),
        Enrolled {
            email: "dana@work.example".into(),
            outcome: EnrolledAs::InUse { again: true },
            warnings: Vec::new(),
        }
    );
    let after = read(core);
    assert_eq!(
        described(&after),
        ["claude/work, in use", "claude/personal"]
    );
    assert!(
        after.accounts.iter().any(
            |account| account.qualified.as_deref() == Some("claude/work") && !account.switchable
        )
    );
}

// The rest of the machine.

/// An interrupted switch nothing can finish is given up on once, and asking again has
/// nothing to give up on. On the real core a read never fails over one, as the Swift
/// fixture's did: what is refused until then is every change, which finishes an
/// interrupted switch before anything else. Claude Code's session has expired meanwhile,
/// which giving up does not change.
///
/// FixtureTests.swift's givingUpOnTheInterruptedSwitchHappensOnce.
#[test]
fn giving_up_on_the_interrupted_switch_happens_once() {
    let launched = made(World::Stuck);
    let core = &launched.core;
    assert!(core.status(true).is_ok(), "a read is never refused over it");
    assert_eq!(
        refusal(core.switch_to("claude/personal".into())).as_deref(),
        Some("recovery_undetermined")
    );

    assert_eq!(
        core.abandon_recovery().expect("given up"),
        Some(crate::Abandoned {
            from: "work".into(),
            to: "personal".into(),
            logins_kept: 2,
        })
    );
    assert_eq!(core.abandon_recovery().expect("nothing to give up"), None);
    assert_eq!(
        described(&read(core)),
        ["claude/work, in use", "claude/personal"]
    );
    assert_ne!(
        refusal(core.switch_to("claude/personal".into())).as_deref(),
        Some("recovery_undetermined"),
        "nothing is waiting any more"
    );
}

/// The log keeps what changed newest last, and names Claude Code's accounts bare and any
/// other tool's with the tool, as the command line types them. The core logs a switch as
/// `use`, where the Swift fixture wrote `switch`.
///
/// FixtureTests.swift's theLogRecordsChangesNewestLastAsTheyAreTyped, but for when the
/// account index last changed, which is kept to the second, so a change within the second
/// the world was made in does not move it: that a change made elsewhere is noticed is
/// threaded.rs's `a_change_another_front_end_makes_is_told_without_asking_anyone`.
#[test]
fn the_log_records_changes_newest_last_as_they_are_typed() {
    let launched = made(World::TwoTools);
    let core = &launched.core;
    let history = core.log(500);
    assert_eq!(
        history
            .iter()
            .map(|change| format!("{} {} {}", change.caller, change.verb, change.subject))
            .collect::<Vec<_>>(),
        [
            "cli enroll old",
            "cli enroll codex/main",
            "cli enroll work",
            "app enroll personal",
            "app enroll codex/spare",
            "cli use personal",
            "app use work",
        ]
    );

    core.switch_to("claude/personal".into()).expect("a switch");
    core.switch_to("codex/spare".into()).expect("a switch");
    core.rename("claude/work".into(), "office".into())
        .expect("renamed");
    let log = core.log(500);
    assert_eq!(
        log[history.len()..]
            .iter()
            .map(|change| format!("{} {}", change.verb, change.subject))
            .collect::<Vec<_>>(),
        ["use personal", "use codex/spare", "rename work -> office"]
    );
    assert!(
        log[log.len() - 3..]
            .iter()
            .all(|change| change.caller == "app" && change.outcome == "ok")
    );
    let dates: Vec<i64> = log
        .iter()
        .filter_map(|change| pitboard_core::time::parse(&change.at))
        .collect();
    assert_eq!(dates.len(), log.len());
    assert!(dates.windows(2).all(|pair| pair[0] <= pair[1]), "{dates:?}");
    assert_eq!(core.log(2), log[log.len() - 2..]);
}

/// Daily renewal can be turned on and off, and taking away a schedule that is not there
/// says there was nothing to take away. Nothing is ever repaired. The schedule is a file in
/// the fixture's own folder, which no scheduler is asked to start.
///
/// FixtureTests.swift's theScheduleTurnsOnAndOff.
#[test]
fn the_schedule_turns_on_and_off() {
    let launched = made(World::OneTool);
    let core = &launched.core;
    assert_eq!(core.schedule(), Schedule::Absent);
    assert_eq!(core.schedule_uninstall().ok(), Some(false));

    let path = core.schedule_install().expect("installed");
    assert!(
        std::path::Path::new(&path).starts_with(launched.machine.root()),
        "{path}"
    );
    assert_eq!(
        core.schedule(),
        Schedule::Installed {
            path,
            every_seconds: 86_400
        }
    );
    assert_eq!(core.schedule_uninstall().ok(), Some(true));
    assert_eq!(core.schedule(), Schedule::Absent);
    assert_eq!(core.schedule_repair().ok(), Some(false));
}

// Launching into a fixture.

/// A UI test launches the app into a fixture many times, and each launch starts where the
/// last one did, whatever the last one left: the folder emptied and made again, seen before
/// unless it is the first launch, what it has told empty, and the same accounts. The first
/// launch is kept while the second is made in its folder, as the folder an app launches
/// into is kept from one launch to the next, so what the first left is there to be emptied.
///
/// FixtureTests.swift's everyLaunchStartsWhereTheLastOneDid, but for the login item and
/// notifications' permission, which are the app's.
#[test]
fn every_launch_starts_where_the_last_one_did() {
    use pitboard_core::app::AppFile;
    for world in World::ALL {
        let first = made(world);
        let core = &first.core;
        let accounts = who(&core.status_offline().expect("what is known"));
        let preferences = core.app_file(AppFile::Preferences).expect("readable");
        assert_eq!(
            preferences
                .as_deref()
                .map(|text| text.contains(r#""has_been_seen":true"#)),
            (world != World::FirstLaunch).then_some(true),
            "{}",
            world.name()
        );
        // What a launch leaves behind: a switch, what it told, preferences of its own, and
        // anything else in its folder.
        core.switch_to("claude/personal".into()).ok();
        core.keep_app_file(AppFile::Told, r#"{"claude/work/session/":7200}"#)
            .expect("told");
        core.keep_app_file(
            AppFile::Preferences,
            r#"{"has_been_seen":true,"second_account_declined":["claude"]}"#,
        )
        .expect("kept");
        let left = first.machine.root().join("left behind");
        std::fs::write(&left, "").expect("left behind");

        let again = made(world);
        assert_eq!(
            again.machine.root(),
            first.machine.root(),
            "the same folder"
        );
        assert!(!left.exists(), "{}: the folder emptied", world.name());
        let core = &again.core;
        assert_eq!(
            who(&core.status_offline().expect("what is known")),
            accounts,
            "{}",
            world.name()
        );
        assert_eq!(
            core.app_file(AppFile::Preferences).expect("readable"),
            preferences,
            "{}",
            world.name()
        );
        assert_eq!(
            core.app_file(AppFile::Told).expect("readable"),
            None,
            "{}",
            world.name()
        );
        assert!(again.machine.root().starts_with(std::env::temp_dir()));
    }
}

/// The command line is inside a stand-in app in the fixture's own folder, one a schedule
/// would keep reaching and a link could reach, and a terminal finds none until one is linked
/// in the folder's `bin`, where the macOS app's fixture links it: linking is the app's own.
///
/// FixtureTests.swift's aLaunchLinksItsCommandLineInATemporaryDirectory, but for linking.
#[test]
fn a_launch_keeps_its_command_line_in_its_own_folder() {
    let launched = made(World::OneTool);
    let core = &launched.core;
    let own = core.own_command_line();
    assert!(own.lasting(), "{own:?}");
    assert!(
        launched
            .machine
            .helper()
            .starts_with(launched.machine.root())
    );
    assert_eq!(core.command_line(), FoundCommandLine::Nowhere);

    // A link as the macOS app makes one, which only a Unix system has.
    #[cfg(unix)]
    {
        let link = launched.machine.bin().join("pitboard");
        std::os::unix::fs::symlink(launched.machine.helper(), &link).expect("linked");
        assert_eq!(
            core.command_line(),
            FoundCommandLine::Bundled {
                path: link.to_string_lossy().into_owned()
            }
        );
    }
}

// The model an app launches into a fixture.

/// Keeps every snapshot it is told of.
#[derive(Default)]
struct Told {
    snapshots: Mutex<Vec<Snapshot>>,
    arrived: Condvar,
}

impl ModelListener for Told {
    fn changed(&self, snapshot: Snapshot) -> Result<(), PlatformError> {
        self.snapshots
            .lock()
            .expect("a test's own lock")
            .push(snapshot);
        self.arrived.notify_all();
        Ok(())
    }
}

impl Told {
    /// Waits until the last snapshot told satisfies `done`, and gives it.
    fn until(&self, what: &str, done: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let started = Instant::now();
        let mut snapshots = self.snapshots.lock().expect("a test's own lock");
        loop {
            if let Some(last) = snapshots.last()
                && done(last)
            {
                return last.clone();
            }
            let left = PATIENCE
                .checked_sub(started.elapsed())
                .unwrap_or_else(|| panic!("never told {what}: {:#?}", snapshots.last()));
            snapshots = self
                .arrived
                .wait_timeout(snapshots, left)
                .expect("a test's own lock")
                .0;
        }
    }
}

/// Whether the accounts have been read, and nothing is being read.
fn read_in(snapshot: &Snapshot) -> bool {
    snapshot.status.is_some() && !snapshot.reading && snapshot.updated_at.is_some()
}

/// The app's model over `launched`, started, once the accounts have been read.
fn started(launched: &Launched) -> (Arc<PitboardModel>, Arc<Told>) {
    let told = Arc::new(Told::default());
    let model = launched.model(Arc::clone(&told) as Arc<dyn ModelListener>, Arc::new(Utc));
    model.send(Intent::Start);
    told.until("the accounts read", read_in);
    (model, told)
}

/// The UI tests open the menu before the window, and the menu shows only what a read has
/// found. The app launched into a fixture reads by itself, as it does on a real machine,
/// with nothing opened and nothing pressed.
///
/// FixtureTests.swift's aLaunchReadsWithoutBeingAsked.
#[test]
fn a_launch_reads_without_being_asked() {
    let launched = made(World::TwoTools);
    let (model, told) = started(&launched);
    let last = told.until("the accounts read", read_in);
    assert_eq!(
        described(last.status.as_ref().expect("read")),
        described(&read(&launched.core))
    );
    model.shutdown();
}

/// The fixture's ChatGPT runs Codex's login while it is open, as the process list shows it,
/// and its app control quits and opens it: a switch through the fixture quits it, switches
/// and opens it again, which is what the UI test drives.
///
/// FixtureTests.swift's theChatGPTFixtureIsQuitForACodexSwitchAndOpenedAgain.
#[test]
fn the_chatgpt_fixture_is_quit_for_a_codex_switch_and_opened_again() {
    let launched = made(World::ChatGptOpen);
    let core = &launched.core;
    assert_eq!(
        core.holding("codex".into())
            .iter()
            .map(|held| held.kind.as_str())
            .collect::<Vec<_>>(),
        ["chatgpt_app"]
    );
    assert!(core.holding("claude".into()).is_empty());

    let (model, told) = started(&launched);
    model.send(Intent::SwitchTo {
        qualified: "codex/spare".into(),
    });
    let asked = told.until("the quit question", |snapshot| {
        snapshot.quit_question.is_some()
    });
    let question = asked.quit_question.expect("asked");
    assert_eq!(question.name, "ChatGPT");
    model.send(Intent::QuitAndSwitch {
        qualified: question.qualified,
    });
    told.until("the switch, read back", |snapshot| {
        snapshot.switch_under_way.is_none()
            && snapshot
                .status
                .as_ref()
                .is_some_and(|status| in_use(status).contains(&"codex/spare".into()))
    });
    assert_eq!(
        launched.apps.asked(),
        [format!("quit {CHATGPT}"), format!("open {CHATGPT}")]
    );
    assert!(launched.apps.is_running(CHATGPT));
    model.shutdown();
}

/// The fixture's ChatGPT leaves the process list as it quits and comes back as it is
/// opened again, so the core sees it go and come back as it would on a real machine.
#[test]
fn chatgpt_leaves_the_process_list_as_it_quits() {
    use crate::model::AppControl;
    let launched = made(World::ChatGptOpen);
    let (apps, core) = (&launched.apps, &launched.core);
    let copy = apps
        .running(CHATGPT.into())
        .expect("asked")
        .expect("running");
    apps.request_quit(CHATGPT.into()).expect("asked to quit");
    assert_eq!(apps.running(CHATGPT.into()).expect("asked"), None);
    assert!(core.holding("codex".into()).is_empty());
    apps.reopen(copy).expect("opened");
    assert_eq!(core.holding("codex".into()).len(), 1);
    assert!(
        !made(World::TwoTools).apps.is_running(CHATGPT),
        "only chatGPTOpen has it open"
    );
}

// What each UI test reads in its world.

/// Every account is an item under its tool, as MenuBarTests.swift's
/// testTheMenuListsEveryAccountUnderItsTool reads them.
#[test]
fn the_menu_lists_every_account_under_its_tool() {
    let launched = made(World::TwoTools);
    let (model, told) = started(&launched);
    let shown = told.until("the accounts read", read_in);
    let sections: Vec<(Option<String>, Vec<String>)> = shown
        .sections
        .iter()
        .map(|section| {
            (
                section.heading.clone(),
                section
                    .accounts
                    .iter()
                    .map(|item| item.title.clone())
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        sections,
        [
            (
                Some("Claude Code".into()),
                vec!["work".into(), "old".into(), "personal".into()]
            ),
            (Some("Codex".into()), vec!["main".into(), "spare".into()]),
        ]
    );
    model.shutdown();
}

/// A machine without Claude Code says so first, as MenuBarTests.swift's
/// testAMachineWithoutClaudeCodeSaysSo reads it; one nobody is signed in on offers an
/// account, as AccountsWindowTests.swift's testAnEmptyMachineOffersAnAccount reads it; and
/// only the first launch ever opens the window by itself, as its
/// testTheFirstLaunchOpensTheWindow reads it.
#[test]
fn a_machine_with_nothing_on_it_says_what_to_do_first() {
    for world in [World::NoClaudeCode, World::Empty, World::FirstLaunch] {
        let launched = made(world);
        let (model, told) = started(&launched);
        let shown = told.until("the accounts read", read_in);
        match world {
            World::NoClaudeCode => {
                assert_eq!(shown.footing, Footing::NoClaudeCode);
                assert_eq!(
                    shown
                        .menu_notices
                        .install
                        .as_ref()
                        .map(|item| item.title.as_str()),
                    Some("Claude Code isn’t installed")
                );
            }
            _ => assert!(
                matches!(&shown.accounts_shown, AccountsShown::NoAccounts { title, .. } if title == "No Accounts"),
                "{}: {:?}",
                world.name(),
                shown.accounts_shown
            ),
        }
        assert_eq!(
            shown.window_request.serial > 0,
            world == World::FirstLaunch,
            "{}",
            world.name()
        );
        model.shutdown();
    }
}

/// The login in use with no name is offered one, as AccountsWindowTests.swift's
/// testNamingTheAccountInUse reads it, and naming it lists it.
#[test]
fn the_login_in_use_is_offered_a_name() {
    let launched = made(World::Unnamed);
    let (model, told) = started(&launched);
    let shown = told.until("the accounts read", read_in);
    assert_eq!(
        shown.setup.as_ref().map(|step| step.title.as_str()),
        Some("Give this account a name")
    );
    model.send(Intent::Enrol {
        provider: "claude".into(),
        name: "work".into(),
    });
    told.until("the account named", |snapshot| {
        read_in(snapshot)
            && snapshot.status.as_ref().is_some_and(|status| {
                status
                    .accounts
                    .iter()
                    .any(|account| account.qualified.as_deref() == Some("claude/work"))
            })
    });
    model.shutdown();
}

/// One account is offered a second until Not Now, as AccountsWindowTests.swift's
/// testOneAccountIsOfferedASecondUntilNotNow reads it, which the fixture's own folder keeps.
#[test]
fn one_account_is_offered_a_second_until_not_now() {
    let launched = made(World::OnlyOne);
    let (model, told) = started(&launched);
    let shown = told.until("the accounts read", read_in);
    assert_eq!(
        shown.setup.as_ref().map(|step| step.title.as_str()),
        Some("Add a second account")
    );
    model.send(Intent::DeclineSecondAccount {
        provider: "claude".into(),
    });
    told.until("the nudge put away", |snapshot| snapshot.setup.is_none());
    let started = Instant::now();
    while !launched
        .core
        .app_file(pitboard_core::app::AppFile::Preferences)
        .expect("readable")
        .is_some_and(|kept| kept.contains(r#""second_account_declined":["claude"]"#))
    {
        assert!(started.elapsed() < PATIENCE, "never kept");
        std::thread::sleep(Duration::from_millis(10));
    }
    model.shutdown();
}

/// Activity lists what Pitboard changed, newest first, every switch above every enrolment,
/// as PanesAndSettingsTests.swift's testActivityListsChanges and AccountsWindowTests.swift's
/// testCommandNAddsAnAccountFromAnyPane read it. A switch is named "Switch" from the verb
/// the core logs it under, `use`.
#[test]
fn activity_lists_every_switch_above_every_enrolment() {
    let launched = made(World::OneTool);
    let (model, told) = started(&launched);
    model.send(Intent::PaneShown {
        pane: Pane::Activity,
    });
    let shown = told.until("the log read", |snapshot| {
        !snapshot.machine.activity.lines.is_empty()
    });
    assert_eq!(
        shown
            .machine
            .activity
            .lines
            .iter()
            .map(|line| line.change.as_str())
            .collect::<Vec<_>>(),
        ["Switch", "Switch", "Enrol", "Enrol"]
    );
    model.shutdown();
}

/// Daily renewal turns on, says how often it runs, and Renew Now renews the one parked login
/// that is due, as PanesAndSettingsTests.swift's testDailyRenewal reads it.
#[test]
fn daily_renewal_turns_on_and_renew_now_says_what_it_did() {
    let launched = made(World::OneTool);
    let (model, told) = started(&launched);
    model.send(Intent::SetSchedule { on: true });
    let on = told.until("the schedule on", |snapshot| {
        snapshot.machine.schedule.on && !snapshot.machine.schedule.changing
    });
    assert_eq!(on.machine.schedule.runs.as_deref(), Some("Every day"));
    model.send(Intent::RenewNow);
    told.until("the renewal said", |snapshot| {
        snapshot.machine.renewal.note == "Renewed one." && !snapshot.machine.renewal.renewing
    });
    model.shutdown();
}

/// A Codex switch says running sessions keep the old account until they are restarted, as
/// AccountsWindowTests.swift's testUsingACodexAccountSaysSessionsKeepTheOldOne reads it.
#[test]
fn a_codex_switch_says_sessions_keep_the_old_account() {
    let launched = made(World::TwoTools);
    let (model, told) = started(&launched);
    model.send(Intent::SwitchTo {
        qualified: "codex/spare".into(),
    });
    let shown = told.until("what the switch said", |snapshot| {
        snapshot
            .notices
            .iter()
            .any(|notice| notice.id == "switch/codex")
    });
    let notice = shown
        .notices
        .iter()
        .find(|notice| notice.id == "switch/codex")
        .expect("said");
    assert!(
        notice.lines.iter().any(|line| line
            .starts_with("Any codex session started before this switch keeps using main")),
        "{notice:?}"
    );
    model.shutdown();
}

/// Adding accounts as AccountsWindowTests.swift's tests do: Claude Code's sign-in asks for
/// the code and lists the account once it is typed back, Codex's lists it by itself, and one
/// cancelled adds nothing and lets another start.
#[test]
fn accounts_are_added_through_each_tools_sign_in() {
    let launched = made(World::OneTool);
    let (model, told) = started(&launched);
    model.send(Intent::PresentSheet {
        sheet: Sheet::Add { provider: None },
    });
    model.send(Intent::SignIn {
        provider: "claude".into(),
        name: "third".into(),
    });
    let asking = told.until("the code asked for", |snapshot| {
        snapshot
            .signing_in
            .as_ref()
            .is_some_and(|signing| signing.wants_code)
    });
    assert_eq!(
        asking.signing_in.and_then(|signing| signing.url).as_deref(),
        Some(CLAUDE_ADDRESS)
    );
    model.send(Intent::CancelSignIn);
    told.until("the sign-in over", |snapshot| snapshot.signing_in.is_none());
    model.send(Intent::SignIn {
        provider: "claude".into(),
        name: "third".into(),
    });
    told.until("the code asked for", |snapshot| {
        snapshot
            .signing_in
            .as_ref()
            .is_some_and(|signing| signing.wants_code)
    });
    model.send(Intent::PasteCode {
        code: "fixture-code".into(),
    });
    told.until("the account listed", |snapshot| {
        snapshot.signing_in.is_none()
            && read_in(snapshot)
            && snapshot.status.as_ref().is_some_and(|status| {
                status
                    .accounts
                    .iter()
                    .any(|account| account.qualified.as_deref() == Some("claude/third"))
            })
    });
    model.shutdown();

    let launched = made(World::TwoTools);
    let (model, told) = started(&launched);
    model.send(Intent::SignIn {
        provider: "codex".into(),
        name: "third".into(),
    });
    told.until("the account listed", |snapshot| {
        snapshot.signing_in.is_none()
            && read_in(snapshot)
            && snapshot.status.as_ref().is_some_and(|status| {
                status
                    .accounts
                    .iter()
                    .any(|account| account.qualified.as_deref() == Some("codex/third"))
            })
    });
    model.shutdown();
}

/// A rename to a name another account has is refused in its sheet, and a free name is
/// taken, as AccountsWindowTests.swift's testRenamingAnAccount reads it; the account in use
/// offers no Forget, and any other is forgotten, as its testTheAccountInUseOffersNoForget
/// and testForgettingAsksFirst read it.
#[test]
fn accounts_are_renamed_and_forgotten_as_the_window_offers() {
    let launched = made(World::OneTool);
    let (model, told) = started(&launched);
    let shown = told.until("the accounts read", read_in);
    let can_forget: Vec<(String, bool)> = shown
        .sections
        .iter()
        .flat_map(|section| &section.accounts)
        .map(|item| (item.title.clone(), item.can_forget))
        .collect();
    assert_eq!(
        can_forget,
        [("work".into(), false), ("personal".into(), true)]
    );

    model.send(Intent::PresentSheet {
        sheet: Sheet::Rename {
            provider: "claude".into(),
            label: "personal".into(),
        },
    });
    model.send(Intent::Rename {
        provider: "claude".into(),
        label: "personal".into(),
        to: "work".into(),
    });
    let refused = told.until("the rename refused", |snapshot| {
        snapshot.sheet_failure.is_some()
    });
    assert_eq!(
        refused
            .sheet_failure
            .map(|failure| failure.title)
            .as_deref(),
        Some("Couldn’t rename personal")
    );
    model.send(Intent::Rename {
        provider: "claude".into(),
        label: "personal".into(),
        to: "home".into(),
    });
    told.until("the account renamed", |snapshot| {
        read_in(snapshot)
            && snapshot.status.as_ref().is_some_and(|status| {
                in_use(status) == ["claude/work"]
                    && status
                        .accounts
                        .iter()
                        .any(|account| account.qualified.as_deref() == Some("claude/home"))
            })
    });
    model.send(Intent::Forget {
        qualified: "claude/home".into(),
    });
    told.until("the account forgotten", |snapshot| {
        read_in(snapshot)
            && snapshot
                .status
                .as_ref()
                .is_some_and(|status| status.accounts.len() == 1)
    });
    model.shutdown();
}

/// With Anthropic out of reach, the real core still reads: each account says why its
/// numbers are the last measured, and nothing says a read failed, where the Swift fixture's
/// read failed and the menu said "Couldn’t read usage". MenuBarTests.swift's
/// testAFailedReadIsSaidInTheMenuAndInTheWindow reads that, and so its answer changes in
/// PR 10 unless the core's read comes to say so.
#[test]
fn a_service_out_of_reach_leaves_the_last_numbers_and_says_why_on_each_account() {
    let launched = made(World::ReadFailure);
    let (model, told) = started(&launched);
    let shown = told.until("the accounts read", read_in);
    let items: Vec<_> = shown
        .sections
        .iter()
        .flat_map(|section| &section.accounts)
        .collect();
    assert_eq!(items.len(), 2);
    for item in items {
        assert_eq!(
            item.stale_note.as_deref(),
            Some("Anthropic could not be reached"),
            "{item:?}"
        );
        assert!(!item.limits.is_empty(), "the last numbers measured");
    }
    assert!(shown.read_failure.is_none());
    assert!(shown.notices.iter().all(|notice| notice.id != "read"));
    model.shutdown();
}

/// An interrupted switch nothing can finish is said once a switch is refused over it, as
/// the core refuses every change then, and giving up says what it kept. No read of the real
/// core says one is waiting, so the notice that offers Give Up… is not shown as the window
/// opens, where the Swift fixture's read failed and showed it: AccountsWindowTests.swift's
/// testGivingUpOnAnInterruptedSwitch and MenuBarTests.swift's
/// testShowingANoticeOpensTheWindowOnTheAccounts read that, and so their answers change in
/// PR 10 unless the core's read comes to say so.
#[test]
fn an_interrupted_switch_is_said_when_a_switch_is_refused_over_it() {
    let launched = made(World::Stuck);
    let (model, told) = started(&launched);
    let shown = told.until("the accounts read", read_in);
    assert!(!shown.stuck);
    assert!(shown.notices.iter().all(|notice| notice.id != "stuck"));

    model.send(Intent::SwitchTo {
        qualified: "claude/personal".into(),
    });
    let refused = told.until("the switch refused", |snapshot| snapshot.failure.is_some());
    let failure = refused.failure.expect("said");
    assert_eq!(failure.code.as_deref(), Some("recovery_undetermined"));
    assert!(failure.message.contains("was interrupted"), "{failure:?}");

    model.send(Intent::AbandonStuckSwitch);
    let given_up = told.until("what giving up kept", |snapshot| {
        snapshot
            .notices
            .iter()
            .any(|notice| notice.id == "abandoned")
    });
    let notice = given_up
        .notices
        .iter()
        .find(|notice| notice.id == "abandoned")
        .expect("said");
    assert_eq!(notice.title, "Gave up on the interrupted switch");
    assert!(
        notice
            .lines
            .iter()
            .any(|line| line.contains("2 logins kept")),
        "{notice:?}"
    );
    model.shutdown();
}

/// This Mac shows the core's checks, as PanesAndSettingsTests.swift's
/// testThisMacShowsTheChecks reads them, and they are not the Swift fixture's. That passed a
/// check called "Keychain" and warned that daily renewal was off, which made "One thing is
/// worth looking at.". The core has no check called "Keychain", and says nothing of a
/// schedule that is not there. oneTool's one thing is personal's parked login instead, which
/// is the world's own doing: it lasts two days more, so that Renew Now renews it, where the
/// Swift fixture's lasted eleven. Renew Now finds a parked login due that the read before it
/// left alone only while its refresh token lapses within three days, which is when doctor
/// warns of it, since both ask `doctor::renewal_due`.
///
/// So that test's answer changes in PR 10, unless the core comes to say what the Swift
/// fixture said: the sentence is the same, of personal's parked login rather than of daily
/// renewal.
#[test]
fn this_mac_shows_the_cores_checks() {
    let launched = made(World::OneTool);
    let (model, told) = started(&launched);
    model.send(Intent::PaneShown {
        pane: Pane::Machine,
    });
    let shown = told.until("the checks made", |snapshot| {
        !snapshot.machine.checks.lines.is_empty() && !snapshot.machine.checks.checking
    });
    let checks = shown.machine.checks;
    assert!(
        checks.lines.iter().all(|line| line.name != "Keychain"),
        "{:#?}",
        checks.lines
    );
    let worth_looking_at: Vec<(&str, &str)> = checks
        .lines
        .iter()
        .filter(|line| line.level != crate::Level::Ok)
        .map(|line| (line.code.as_str(), line.name.as_str()))
        .collect();
    assert_eq!(
        worth_looking_at,
        [("parked_login", "account personal")],
        "{:#?}",
        checks.lines
    );
    let personal = checks
        .lines
        .iter()
        .find(|line| line.name == "account personal")
        .expect("personal's");
    assert!(
        personal.detail.starts_with("its parked login expires in "),
        "{personal:?}"
    );
    assert_eq!(
        checks.summary.as_deref(),
        Some("One thing is worth looking at.")
    );
    model.shutdown();
}

// The stand-in pages.

/// Each site's stand-in has what its window's tests press and read: its title, the path it
/// was asked for, a link outside it, Google's sign-in, another app's link, a chat, a
/// download, a dialog each way, its sign-in three ways, and an artifact's frame.
#[test]
fn each_site_has_a_stand_in_page_with_what_its_tests_look_for() {
    let page = pages::page("pitboard-fixture://chatgpt.com/c/shared?x=1#y");
    assert!(page.starts_with("<!doctype html>"));
    for part in [
        "<title>chatgpt.com stand-in</title>",
        "<h1>chatgpt.com stand-in</h1>",
        "A Pitboard fixture page at /c/shared. Nothing here reaches the network.",
        "id=\"outside\" href=\"https://example.com/\">A link outside chatgpt.com</a>",
        "href=\"https://accounts.google.com/o/oauth2/v2/auth\">Continue with Google</a>",
        "href=\"vscode://file/x\">Open in an editor</a>",
        "href=\"pitboard-fixture://chatgpt.com/chat/fixture\">A chat</a>",
        "download=\"notes.txt\" href=\"data:text/plain,notes\">Download notes</a>",
        "onclick=\"alert('Saved.')\">Alert</button>",
        "'confirmed' : 'declined'\">Confirm</button>",
        "href=\"pitboard-fixture://appleid.apple.com/sign-in\" target=\"_blank\" rel=\"opener\">Continue with appleid.apple.com</a>",
        "window.open('pitboard-fixture://appleid.apple.com/sign-in', 'sign-in', 'width=480,height=600')",
        "const w = window.open(''); w.location = 'pitboard-fixture://appleid.apple.com/sign-in'",
        "src=\"pitboard-fixture://artifact.fixture/\"",
    ] {
        assert!(page.contains(part), "{part}\n{page}");
    }
    assert!(pages::page("pitboard-fixture://claude.ai/").contains("A Pitboard fixture page at /."));
    assert!(
        pages::page("pitboard-fixture://CLAUDE.AI/new")
            .contains("<title>claude.ai stand-in</title>")
    );
}

/// A host a site's sign-in goes to has a sign-in stand-in whose Done closes its window; the
/// artifact's host has the frame whose link a message clicks; and anything else has nothing.
#[test]
fn every_other_host_has_its_own_stand_in() {
    let sign_in = pages::page("pitboard-fixture://appleid.apple.com/sign-in");
    assert!(sign_in.contains("<title>appleid.apple.com sign-in stand-in</title>"));
    assert!(sign_in.contains("<button id=\"done\" onclick=\"window.close()\">Done</button>"));
    let artifact = pages::page("pitboard-fixture://artifact.fixture/");
    assert!(artifact.contains("<title>Artifact</title>"));
    assert!(artifact.contains("document.getElementById('artifact-download').click()"));
    for nowhere in [
        "pitboard-fixture://example.com/",
        "pitboard-fixture:",
        "not a link",
    ] {
        assert!(
            pages::page(nowhere).contains("<p>Nothing is here.</p>"),
            "{nowhere}"
        );
    }
}

// What every build exports.

/// One exported fixture at a time: they share their folder.
static EXPORTED: Mutex<()> = Mutex::new(());

/// A fixture's name that is none of them is refused, naming every one there is.
#[test]
fn an_unknown_fixture_is_refused_naming_every_one() {
    let _one = EXPORTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let refused =
        PitboardModel::fixture("twoTool".into(), Arc::new(Told::default()), Arc::new(Utc));
    match refused {
        Err(super::FixtureError::Unknown { reason }) => {
            assert!(reason.contains("\"twoTool\""), "{reason}");
            assert!(reason.contains("twoTools, oneTool, empty"), "{reason}");
        }
        other => panic!("{:?}", other.map(|_| ())),
    }
}

/// Forgets the launches into the folder an app's fixture is kept in once a test is done with
/// it, however the test ends, as FixtureTests.swift's forgetLaunches does.
struct ForgetsTheLaunches;

impl Drop for ForgetsTheLaunches {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("pitboard-fixture"));
    }
}

/// The exported constructor makes a fixture in the folder an app's fixture is kept in,
/// emptying what the last launch left there, and its model, once started, reads its accounts
/// and tells the app's listener. Like the Swift tests that launch into a fixture, it empties
/// that folder and then removes it, so a debug build launched into a fixture meanwhile loses
/// its world.
#[test]
fn an_exported_fixture_is_started_and_reads_its_accounts() {
    let _one = EXPORTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _forgets = ForgetsTheLaunches;
    let shared = std::env::temp_dir().join("pitboard-fixture");
    let told = shared.join("home/.pitboard/told.json");
    std::fs::create_dir_all(told.parent().expect("a folder")).expect("a folder");
    std::fs::write(&told, r#"{"claude/work/session/":7200}"#).expect("left behind");

    let listener = Arc::new(Told::default());
    let model = PitboardModel::fixture(
        "oneTool".into(),
        Arc::clone(&listener) as Arc<dyn ModelListener>,
        Arc::new(Utc),
    )
    .expect("a fixture");
    assert!(shared.join("home/.pitboard").is_dir());
    assert!(!told.exists(), "what the last launch told is gone");
    model.send(Intent::Start);
    let shown = listener.until("the accounts read", read_in);
    assert_eq!(
        described(shown.status.as_ref().expect("read")),
        ["claude/work, in use", "claude/personal"]
    );
    model.shutdown();
}
