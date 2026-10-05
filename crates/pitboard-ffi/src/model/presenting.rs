//! What the menu bar, the menu and the window say, as the Swift app said it: the tests of
//! PresentationTests.swift and MenuTests.swift that drove a model, the rest of
//! RoutingTests.swift, and AppModelTests.swift's tests of the footing, the sheets and the
//! window's words, each under its own name in snake case. The state is driven through
//! `State::apply` by hand and read as `present` makes it, its clock times in UTC.

use super::signing::signs_in;
use super::switching::switch;
use super::testing::{
    CHATGPT, Hand, Machine, StandInApps, chatgpt_holding, claude_code, codex, enrolled_as, refusal,
    status, switched, warned, warning,
};
use super::{Intent, Pane, Sheet, Snapshot, WindowRequest};
use crate::present::testing::{LimitExt, Unreadable, Utc, account, unplaced, window};
use crate::present::{
    AccountItem, AccountsShown, Choice, Footing, MenuEntry, NoticeAction, PanelNotice, Question,
    SetupStep, Severity, present, present_on,
};
use crate::{Abandoned, Account, EnrolledAs, Warning};
use pitboard_core::host::Os;
use std::time::Duration;

/// 12:00 UTC on Wednesday 14 January 2026, a day far from any change of clocks.
const NOON: i64 = 1_768_392_000;

fn at_noon() -> Hand {
    let mut model = Hand::new();
    model.now.epoch_ms = NOON * 1000;
    model
}

/// The model at noon, having read `accounts`.
fn reading(accounts: Vec<Account>) -> (Hand, Machine) {
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(status(accounts)));
    model.refresh(&mut machine);
    (model, machine)
}

/// Every row the model shows, section by section.
fn rows(model: &Hand) -> Vec<AccountItem> {
    model
        .shown()
        .sections
        .into_iter()
        .flat_map(|section| section.accounts)
        .collect()
}

/// `account` as its row describes it at noon, with nothing else under way.
fn described(account: Account) -> AccountItem {
    rows(&reading(vec![account]).0).remove(0)
}

/// `account` as its row describes it at noon, while `under_way` is.
fn described_while(account: Account, under_way: impl FnOnce(&mut Hand)) -> AccountItem {
    let (mut model, _) = reading(vec![account]);
    under_way(&mut model);
    rows(&model).remove(0)
}

/// A switch to `qualified` under way, waiting on what holds its tool's login.
fn switching(qualified: &str) -> impl FnOnce(&mut Hand) + '_ {
    move |model| {
        model.send(Intent::SwitchTo {
            qualified: qualified.into(),
        });
    }
}

/// A sign-in under way, waiting on its tool to start.
fn signing_in(model: &mut Hand) {
    model.send(Intent::SignIn {
        provider: "claude".into(),
        name: "travel".into(),
    });
}

/// What the action of a row sends, if it has one.
fn sends(row: &AccountItem) -> Option<Intent> {
    row.action.as_ref().map(|action| action.intent.clone())
}

fn present_sheet(sheet: Sheet) -> Intent {
    Intent::PresentSheet { sheet }
}

/// What a Mac's app is shown, whichever system runs the test.
fn on_a_mac(model: &Hand) -> Snapshot {
    present_on(Os::MacOs, &model.state, model.now.epoch(), &Utc)
}

fn interrupted() -> Warning {
    warning(
        "recovery_undetermined",
        "A switch from personal to work was interrupted and cannot be finished yet.",
    )
}

fn overridden() -> Warning {
    warning("auth_overridden", "ANTHROPIC_API_KEY is set")
}

fn ids(shown: &Snapshot) -> Vec<String> {
    shown
        .notices
        .iter()
        .map(|notice| notice.id.clone())
        .collect()
}

fn give_up() -> NoticeAction {
    NoticeAction {
        title: "Give Up…".into(),
        intent: Intent::AbandonStuckSwitch,
        dismisses: false,
        switches: false,
        enabled: true,
        confirm: Some(Question {
            title: "Give up on the interrupted switch?".into(),
            message: "Every login is kept, and nothing is deleted. Pitboard stops trying to \
                      finish it."
                .into(),
            confirm: "Give Up".into(),
        }),
    }
}

fn dismiss(intent: Intent) -> NoticeAction {
    NoticeAction {
        title: "Dismiss".into(),
        intent,
        dismisses: true,
        switches: false,
        enabled: true,
        confirm: None,
    }
}

fn notice(
    id: &str,
    severity: Severity,
    title: &str,
    lines: &[&str],
    actions: Vec<NoticeAction>,
) -> PanelNotice {
    PanelNotice {
        id: id.into(),
        severity,
        spoken_severity: match severity {
            Severity::Info => "Note",
            Severity::Warning => "Warning",
            Severity::Error => "Problem",
        }
        .into(),
        title: title.into(),
        lines: lines.iter().map(|&line| line.to_owned()).collect(),
        until: None,
        until_label: None,
        actions,
    }
}

// One account, as the menu and the window describe it.

/// A row is called what a person called the account. One with no name yet is called by its
/// address, the one thing about it they will recognise, and a login Pitboard cannot use by
/// what is wrong with it, since it has neither.
///
/// PresentationTests.swift's anAccountIsCalledByItsNameOrItsAddressOrWhatIsWrongWithIt.
#[test]
fn an_account_is_called_by_its_name_or_its_address_or_what_is_wrong_with_it() {
    assert_eq!(described(account(Some("work")).build()).title, "work");
    assert_eq!(
        described(account(None).signed_in().uuid("u").build()).title,
        "u@example.com"
    );
    assert_eq!(
        described(unplaced("codex", false)).title,
        "Login Pitboard can’t use"
    );
}

/// The window's row draws every limit and says whose address it is and whether it is in
/// use, from the row alone.
///
/// PresentationTests.swift's aDescriptionCarriesTheAddressWhetherItIsInUseAndEveryLimit.
#[test]
fn a_row_carries_the_address_whether_it_is_in_use_and_every_limit() {
    let in_use = described(
        account(Some("work"))
            .signed_in()
            .limits(vec![window("session", 12.0), window("weekly_all", 64.0)])
            .build(),
    );
    assert_eq!(in_use.email, "work@example.com");
    assert!(in_use.in_use);
    let names: Vec<&str> = in_use.limits.iter().map(|l| l.short.as_str()).collect();
    assert_eq!(names, ["5h", "week"]);
    assert_eq!(in_use.spoken, "work, in use");
    assert!(!described(account(Some("spare")).build()).in_use);
    assert!(
        described(unplaced("codex", false)).limits.is_empty(),
        "nothing measured"
    );
}

/// Pressing an account in the menu or the window does the one thing its state allows,
/// decided in one place so the two cannot disagree: switch to it, sign in to it again, name
/// it, or nothing at all.
///
/// PresentationTests.swift's pressingAnAccountDoesTheOneThingItsStateAllows.
#[test]
fn pressing_an_account_does_the_one_thing_its_state_allows() {
    let spare = described(account(Some("spare")).build());
    assert_eq!(
        sends(&spare),
        Some(Intent::SwitchTo {
            qualified: "claude/spare".into()
        })
    );
    let action = spare.action.expect("an action");
    assert_eq!(
        (
            action.title.as_str(),
            action.spoken.as_str(),
            action.menu_title.as_str()
        ),
        ("Use", "Use spare", "Use spare")
    );
    assert_eq!(
        sends(&described(account(Some("spare")).of("codex").build())),
        Some(Intent::SwitchTo {
            qualified: "codex/spare".into()
        }),
        "by its label with its tool, since two tools can each have a spare"
    );
    assert_eq!(
        sends(&described(account(Some("work")).signed_in().build())),
        None,
        "already in use"
    );
    let stale = described(account(Some("stale")).switchable(false).build());
    assert_eq!(
        sends(&stale),
        Some(present_sheet(Sheet::SignInAgain {
            provider: "claude".into(),
            label: "stale".into()
        }))
    );
    assert_eq!(
        stale.action.map(|action| action.spoken),
        Some("Sign In to stale Again…".into())
    );
    let unnamed = described(account(None).of("codex").signed_in().uuid("u").build());
    assert_eq!(
        sends(&unnamed),
        Some(present_sheet(Sheet::Name {
            provider: "codex".into(),
            email: "u@example.com".into()
        }))
    );
    assert_eq!(
        unnamed.action.map(|action| (action.title, action.spoken)),
        Some(("Name…".into(), "Name u@example.com…".into()))
    );
    assert_eq!(
        sends(&described(account(None).uuid("u").build())),
        None,
        "only the login signed in now can be named"
    );
    assert_eq!(sends(&described(unplaced("codex", false))), None);
    assert_eq!(sends(&described(unplaced("codex", true))), None);
}

/// Each change waits for the one before, so while a switch runs nothing can be pressed, and
/// only the account it is for says it is switching.
///
/// PresentationTests.swift's aSwitchRunningHoldsBackEveryAccountAndMarksOnlyItsOwn.
#[test]
fn a_switch_running_holds_back_every_account_and_marks_only_its_own() {
    let running = "claude/spare";
    let spare = described_while(
        account(Some("spare"))
            .limits(vec![window("session", 5.0)])
            .build(),
        switching(running),
    );
    assert!(spare.switching);
    assert_eq!(spare.summary, "Switching…");
    assert_eq!(spare.action, None);

    let other = described_while(
        account(Some("other"))
            .limits(vec![window("session", 5.0)])
            .build(),
        switching(running),
    );
    assert!(!other.switching);
    assert_eq!(other.summary, "5-hour 5%");
    assert_eq!(other.action, None);
    let unnamed = || account(None).signed_in().uuid("u").build();
    assert_eq!(described_while(unnamed(), switching(running)).action, None);
    assert!(
        !described_while(
            account(Some("spare")).of("codex").build(),
            switching(running)
        )
        .switching,
        "Codex's spare is another account"
    );
    assert!(!described_while(unnamed(), switching(running)).switching);
}

/// A sign-in waits on somebody in a browser. It holds back another sign-in and nothing else:
/// switching meanwhile, or naming the login signed in now, is theirs to do.
///
/// PresentationTests.swift's aSignInRunningHoldsBackOnlyAnotherSignIn.
#[test]
fn a_sign_in_running_holds_back_only_another_sign_in() {
    let spare = described_while(account(Some("spare")).build(), signing_in);
    assert_eq!(
        sends(&spare),
        Some(Intent::SwitchTo {
            qualified: "claude/spare".into()
        })
    );
    assert!(spare.busy);
    assert_eq!(
        sends(&described_while(
            account(None).signed_in().uuid("u").build(),
            signing_in
        )),
        Some(present_sheet(Sheet::Name {
            provider: "claude".into(),
            email: "u@example.com".into()
        }))
    );
    assert_eq!(
        sends(&described_while(
            account(Some("stale")).switchable(false).build(),
            signing_in
        )),
        None
    );
}

/// Whether an account needs signing in again is a fact about the account, and a sign-in or a
/// switch running elsewhere does not change it. Its item cannot be chosen meanwhile, and
/// without the line saying why it reads as an account that simply does not work.
///
/// PresentationTests.swift's anAccountThatNeedsSigningInAgainSaysSoWhileSomethingElseRuns.
#[test]
fn an_account_that_needs_signing_in_again_says_so_while_something_else_runs() {
    let stale = || {
        account(Some("stale"))
            .switchable(false)
            .limits(vec![window("session", 12.0)])
            .build()
    };
    assert_eq!(described(stale()).summary, "Needs signing in again");
    assert_eq!(
        described_while(stale(), signing_in).summary,
        "Needs signing in again"
    );
    let switch_elsewhere = described_while(stale(), switching("claude/spare"));
    assert_eq!(switch_elsewhere.summary, "Needs signing in again");
    assert_eq!(switch_elsewhere.spoken, "stale, needs signing in again");
}

/// The line under an account's name in the menu says what stands in its way before
/// anything about its limits, since that is what decides whether it can be chosen.
///
/// PresentationTests.swift's theMenuSaysWhatStandsInAnAccountsWayBeforeItsLimits.
#[test]
fn the_menu_says_what_stands_in_an_accounts_way_before_its_limits() {
    let limits = || vec![window("session", 12.0)];
    let login = unplaced("codex", false);
    assert_eq!(
        Some(described(login.clone()).summary),
        login.stale_explanation.clone()
    );
    let unexplained = Account {
        stale_explanation: None,
        ..login
    };
    assert_eq!(described(unexplained).summary, "Can’t be read or switched");
    assert_eq!(
        described(account(None).signed_in().uuid("u").limits(limits()).build()).summary,
        "Not named yet"
    );
    assert_eq!(
        described(
            account(Some("stale"))
                .switchable(false)
                .limits(limits())
                .build()
        )
        .summary,
        "Needs signing in again"
    );
}

/// An account with nothing measured yet still has a line under its name, and its address is
/// the one true thing to put there.
///
/// PresentationTests.swift's anAccountWithNothingMeasuredIsDescribedByItsAddress.
#[test]
fn an_account_with_nothing_measured_is_described_by_its_address() {
    assert_eq!(
        described(account(Some("spare")).build()).summary,
        "spare@example.com"
    );
    assert_eq!(
        described(account(Some("spare")).unmeasured().build()).summary,
        "spare@example.com"
    );
    assert_eq!(
        described(account(Some("work")).signed_in().build()).summary,
        "work@example.com"
    );
}

/// Every limit in one line, in the order the service gave them, the way a sentence starts: a
/// capital on its first word and on nothing after it. A limit scoped to one model is named
/// with its model, so it does not read as the account's own.
///
/// PresentationTests.swift's limitsAreSaidInOneLineWithOnlyItsFirstLetterCapitalised.
#[test]
fn limits_are_said_in_one_line_with_only_its_first_letter_capitalised() {
    let summary =
        |limits: Vec<crate::Limit>| described(account(Some("work")).limits(limits).build()).summary;
    assert_eq!(
        summary(vec![window("weekly_all", 64.4), window("session", 12.0)]),
        "Weekly 64%, 5-hour 12%"
    );
    assert_eq!(
        summary(vec![
            window("session", 12.5),
            window("weekly_all", 30.0),
            window("weekly_scoped", 98.0).scope("Fable"),
        ]),
        "5-hour 13%, weekly 30%, weekly Fable 98%"
    );
    assert_eq!(
        summary(vec![window("weekly_scoped", 7.0).scope("Fable")]),
        "Weekly Fable 7%"
    );
}

/// A limit that has run out is worth knowing with when it comes back. Later today that is a
/// clock time, and on another day its weekday too, so a menu left open past midnight is
/// still right. Once that moment has passed the next reading is what says it came back, and
/// until then the line says only that it is used up.
///
/// PresentationTests.swift's aUsedUpLimitSaysWhenItComesBack, the clock read in UTC.
#[test]
fn a_used_up_limit_says_when_it_comes_back() {
    let today = NOON + 2 * 3600;
    let later = NOON + 2 * 86_400;
    let spent = |resets: Option<i64>| {
        described(
            account(Some("work"))
                .signed_in()
                .limits(vec![window("session", 100.0).resets(resets)])
                .build(),
        )
        .summary
    };
    assert_eq!(spent(Some(today)), "5-hour used up until 14:00");
    assert_eq!(spent(Some(later)), "5-hour used up until Fri 12:00");
    assert_eq!(spent(Some(NOON - 60)), "5-hour used up");
    assert_eq!(
        spent(Some(NOON)),
        "5-hour used up",
        "coming back now is not coming back later"
    );
    assert_eq!(
        spent(None),
        "5-hour 100%",
        "with no reset known there is only the figure"
    );
    assert_eq!(
        described(
            account(Some("work"))
                .limits(vec![
                    window("weekly_all", 100.0).resets(Some(today)),
                    window("session", 40.0),
                ])
                .build()
        )
        .summary,
        "Weekly used up until 14:00, 5-hour 40%"
    );
}

/// A clock the app's own code cannot read is said in UTC, and named so, rather than not at
/// all.
#[test]
fn a_clock_the_app_cannot_read_is_said_in_utc_rather_than_not_at_all() {
    let today = NOON + 2 * 3600;
    let (model, _) = reading(vec![
        account(Some("work"))
            .signed_in()
            .limits(vec![window("session", 100.0).resets(Some(today))])
            .build(),
    ]);
    let shown = present(&model.state, NOON, &Unreadable);
    assert_eq!(
        shown.sections[0].accounts[0].summary,
        "5-hour used up until 14:00 UTC"
    );
}

/// Why an account cannot be used is said in full only where it cannot be: a login Pitboard
/// cannot use, or an account that cannot be switched to. Numbers that are merely not live do
/// not stop anybody choosing an account, and the account in use needs no switch.
///
/// PresentationTests.swift's onlyAnAccountThatCannotBeUsedSaysWhy.
#[test]
fn only_an_account_that_cannot_be_used_says_why() {
    let refused_login = "Its parked login was refused. Sign in to it again.";
    let old_numbers = "Anthropic did not answer, so these are the last numbers measured.";
    let login = unplaced("codex", false);
    assert_eq!(described(login.clone()).problem, login.stale_explanation);
    assert_eq!(
        described(
            account(Some("stale"))
                .switchable(false)
                .explained(refused_login)
                .build()
        )
        .problem
        .as_deref(),
        Some(refused_login)
    );
    assert_eq!(
        described(account(Some("stale")).switchable(false).build()).problem,
        None,
        "nothing to say"
    );
    assert_eq!(
        described(account(Some("spare")).explained(old_numbers).build()).problem,
        None,
        "it can still be switched to"
    );
    assert_eq!(
        described(
            account(Some("work"))
                .signed_in()
                .switchable(false)
                .explained(old_numbers)
                .build()
        )
        .problem,
        None,
        "it is the one in use"
    );
}

/// Numbers that are not new are worth knowing about where the account can still be used:
/// its service could not be reached or is rate limiting, or its session has expired. That is
/// said beside the numbers of the account in use and of one that can be switched to, and not
/// a second time where the account cannot be used and why is said already. The menu's help
/// says whichever there is.
///
/// PresentationTests.swift's whyAUsableAccountsNumbersAreOldIsSaidBesideThem.
#[test]
fn why_a_usable_accounts_numbers_are_old_is_said_beside_them() {
    let old_numbers = "Anthropic did not answer, so these are the last numbers measured.";
    let refused_login = "Its parked login was refused. Sign in to it again.";
    let spare = described(account(Some("spare")).explained(old_numbers).build());
    assert_eq!(spare.stale_note.as_deref(), Some(old_numbers));
    assert_eq!(spare.problem, None);
    assert_eq!(spare.help.as_deref(), Some(old_numbers));
    let in_use = described(
        account(Some("work"))
            .signed_in()
            .switchable(false)
            .explained(old_numbers)
            .build(),
    );
    assert_eq!(in_use.stale_note.as_deref(), Some(old_numbers));
    assert_eq!(in_use.problem, None);

    let expired = described(
        account(Some("stale"))
            .switchable(false)
            .explained(refused_login)
            .build(),
    );
    assert_eq!(expired.problem.as_deref(), Some(refused_login));
    assert_eq!(
        expired.stale_note, None,
        "said once, as why it cannot be used"
    );
    assert_eq!(expired.help.as_deref(), Some(refused_login));
    assert_eq!(described(unplaced("codex", false)).stale_note, None);
    assert_eq!(
        described(account(Some("spare")).build()).stale_note,
        None,
        "numbers that are new"
    );
    assert_eq!(described(account(Some("spare")).build()).help, None);
}

/// How long a parked login stays usable is about a switch to it, so it is said of every
/// account but the one in use, whose login is not parked.
///
/// PresentationTests.swift's aParkedLoginsLifeIsSaidOnlyOfAnAccountNotInUse.
#[test]
fn a_parked_logins_life_is_said_only_of_an_account_not_in_use() {
    let parking = NOON + 3 * 86_400 + 60;
    assert_eq!(
        described(account(Some("spare")).parked_until(parking).build())
            .parked_note
            .as_deref(),
        Some("Parked login good for 3 more days")
    );
    assert_eq!(
        described(
            account(Some("work"))
                .signed_in()
                .parked_until(parking)
                .build()
        )
        .parked_note,
        None
    );
    assert_eq!(described(account(Some("spare")).build()).parked_note, None);
}

/// How long an account lasts is a sentence of its own in the window, so it starts with a
/// capital whichever way the account is going, its span read as the reset beside each bar
/// is and as `pitboard status` says it.
///
/// PresentationTests.swift's howLongAnAccountLastsIsSaidAsASentence.
#[test]
fn how_long_an_account_lasts_is_said_as_a_sentence() {
    let pace = |left: Option<i64>, burning: bool| {
        described(
            account(Some("work"))
                .signed_in()
                .lasts(left, burning)
                .build(),
        )
        .pace
    };
    assert_eq!(
        pace(Some(5400), true).as_deref(),
        Some("About 1h 30m left at this rate")
    );
    assert_eq!(pace(Some(5400), false).as_deref(), Some("Resets in 1h 30m"));
    assert_eq!(pace(Some(3900), false).as_deref(), Some("Resets in 1h 05m"));
    assert_eq!(
        pace(Some(3 * 86_400 + 7200 + 300), true).as_deref(),
        Some("About 3d 2h left at this rate")
    );
    assert_eq!(pace(Some(30), true).as_deref(), Some("About to run out"));
    assert_eq!(pace(Some(0), false).as_deref(), Some("Resets any moment"));
    assert_eq!(pace(None, true), None, "nothing to go on yet");
}

/// Only an enrolled account that is not the one in use may be forgotten: forgetting that one
/// would throw away the only record of who is signed in, and the core refuses it. The
/// question asked first names it, and says what forgetting it deletes in the words the
/// account windows say it in.
///
/// AccountsPane.swift's canForget and its alert, which the Swift tested nowhere.
#[test]
fn only_an_enrolled_account_not_in_use_may_be_forgotten() {
    let spare = account(Some("spare")).build();
    let accounts = vec![account(Some("work")).signed_in().build(), spare.clone()];
    let (model, _) = reading(accounts.clone());
    let shown = rows(&model);
    assert!(!shown[0].can_forget);
    assert_eq!(shown[0].forget_question, None);
    assert!(shown[1].can_forget);
    assert_eq!(
        shown[1].forget_question,
        Some(Question {
            title: "Forget “spare”?".into(),
            message: crate::forget_message(spare, accounts),
            confirm: "Forget".into(),
        })
    );
    assert!(shown[1].renamable);
    assert!(!described(account(None).signed_in().uuid("u").build()).can_forget);
    assert!(!described(unplaced("codex", false)).renamable);
}

/// AppModelTests.swift's anAccountThatCannotBeSwitchedToIsSignedInToAgainFromThePanel, its
/// rows: the sign-in itself is signing.rs's. An enrolled account that cannot be switched to
/// is signed in to again from its row, in its own tool.
#[test]
fn an_account_that_cannot_be_switched_to_is_signed_in_to_again_from_the_panel() {
    let (model, _) = reading(vec![
        account(Some("personal")).of("codex").signed_in().build(),
        account(Some("work")).of("codex").switchable(false).build(),
        account(Some("spare")).switchable(false).build(),
    ]);
    let shown = rows(&model);
    let again = |provider: &str, label: &str| {
        Some(present_sheet(Sheet::SignInAgain {
            provider: provider.into(),
            label: label.into(),
        }))
    };
    // A section per tool, Claude Code's first.
    let actions: Vec<Option<Intent>> = shown.iter().map(sends).collect();
    assert_eq!(
        actions,
        [again("claude", "spare"), None, again("codex", "work")]
    );
    assert_eq!(
        shown[2]
            .action
            .as_ref()
            .map(|action| action.spoken.as_str()),
        Some("Sign In to work (Codex) Again…"),
        "named with its tool beside another tool's accounts"
    );
}

// Sections.

/// A tool Pitboard does not list yet still gets a section, after the ones it lists, headed by
/// its code, rather than its accounts going missing.
///
/// PresentationTests.swift's aToolNobodyListsIsGroupedLastUnderItsCode.
#[test]
fn a_tool_nobody_lists_is_grouped_last_under_its_code() {
    let (model, _) = reading(vec![
        account(Some("g")).of("gemini").build(),
        account(Some("job")).of("codex").build(),
        account(Some("work")).build(),
    ]);
    let sections = model.shown().sections;
    let ids: Vec<&str> = sections.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["claude", "codex", "gemini"]);
    let headings: Vec<Option<&str>> = sections.iter().map(|s| s.heading.as_deref()).collect();
    assert_eq!(
        headings,
        [Some("Claude Code"), Some("Codex"), Some("gemini")]
    );
    let titles: Vec<Vec<&str>> = sections
        .iter()
        .map(|s| s.accounts.iter().map(|a| a.title.as_str()).collect())
        .collect();
    assert_eq!(titles, [vec!["work"], vec!["job"], vec!["g"]]);
}

/// One tool's accounts are one section with no heading, known by the tool's code, with the
/// rows in the order they came.
///
/// PresentationTests.swift's oneToolsAccountsAreOneSectionKnownByItsCode.
#[test]
fn one_tools_accounts_are_one_section_known_by_its_code() {
    let (model, _) = reading(vec![
        account(Some("spare")).of("codex").build(),
        account(Some("job")).of("codex").signed_in().build(),
    ]);
    let sections = model.shown().sections;
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].id, "codex");
    assert_eq!(sections[0].heading, None);
    let ids: Vec<&str> = sections[0].accounts.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["codex:spare", "codex:job"]);
}

/// One tool, whichever it is: no headings, the rows in the order the core gave them, and the
/// bar naming the first account signed in, as it always has. A login signed in with no name
/// yet is called unnamed there, which says what it is and fits where an address would not.
///
/// MenuTests.swift's oneToolLooksAsItAlwaysDid.
#[test]
fn one_tool_looks_as_it_always_did() {
    for tool in ["claude", "codex"] {
        let (model, _) = reading(vec![
            account(None)
                .of(tool)
                .signed_in()
                .uuid("u")
                .limits(vec![window("session", 80.0)])
                .build(),
            account(Some("work"))
                .of(tool)
                .limits(vec![window("session", 10.0)])
                .build(),
        ]);
        let shown = model.shown();
        assert_eq!(shown.sections.len(), 1, "{tool}");
        assert_eq!(shown.sections[0].heading, None);
        assert_eq!(shown.sections[0].accounts.len(), 2);
        assert_eq!(shown.menu_bar.name_and_usage, "unnamed 80%");
        assert!(!shown.shows_tools);
    }
    assert!(reading(Vec::new()).0.shown().sections.is_empty());
}

/// More than one tool: a section per tool, headed by its name, in the order the tools are
/// listed whatever order the rows came in.
///
/// MenuTests.swift's accountsOfTwoToolsAreGroupedByTool.
#[test]
fn accounts_of_two_tools_are_grouped_by_tool() {
    let (model, _) = reading(vec![
        account(Some("job")).of("codex").build(),
        account(Some("work")).signed_in().build(),
        account(Some("side")).of("codex").signed_in().build(),
    ]);
    let shown = model.shown();
    assert!(shown.shows_tools);
    let headings: Vec<Option<&str>> = shown
        .sections
        .iter()
        .map(|s| s.heading.as_deref())
        .collect();
    assert_eq!(headings, [Some("Claude Code"), Some("Codex")]);
    let titles: Vec<Vec<&str>> = shown
        .sections
        .iter()
        .map(|s| s.accounts.iter().map(|a| a.title.as_str()).collect())
        .collect();
    assert_eq!(titles, [vec!["work"], vec!["job", "side"]]);
}

// The menu bar.

/// AppModelTests.swift's aReadFillsInTheTitleAndTheRows, its title.
#[test]
fn a_read_fills_in_the_title() {
    let (model, _) = reading(vec![
        account(Some("work"))
            .signed_in()
            .limits(vec![window("session", 42.0).resets(None)])
            .build(),
    ]);
    let bar = model.shown().menu_bar;
    assert_eq!(bar.name_and_usage, "work 42%");
    assert_eq!(bar.usage, "42%");
}

/// The menu bar follows whichever account is closest to running out, and two tools can each
/// have a `work`, so once there is more than one tool VoiceOver says which.
///
/// AppModelTests.swift's theMenuBarSaysWhichToolItIsAboutOnceThereAreTwo.
#[test]
fn the_menu_bar_says_which_tool_it_is_about_once_there_are_two() {
    let mut model = at_noon();
    assert_eq!(model.shown().menu_bar.spoken, "Pitboard");
    let mut machine = Machine::reading(Ok(status(vec![
        account(Some("work"))
            .signed_in()
            .limits(vec![window("session", 42.0)])
            .build(),
    ])));
    model.refresh(&mut machine);
    assert_eq!(model.shown().menu_bar.spoken, "Pitboard, work 42%");

    machine.answer = Ok(status(vec![
        account(Some("work"))
            .signed_in()
            .limits(vec![window("session", 60.0)])
            .build(),
        account(Some("work"))
            .of("codex")
            .signed_in()
            .limits(vec![window("five_hour", 80.0).resets(None)])
            .build(),
    ]));
    model.refresh(&mut machine);
    assert_eq!(model.shown().menu_bar.spoken, "Pitboard, work 80%, Codex");
}

// What the window and the menu have to tell somebody.

/// A machine with nothing wrong has nothing to say, before the first read and after it.
///
/// PresentationTests.swift's aMachineWithNothingWrongHasNoNotices.
#[test]
fn a_machine_with_nothing_wrong_has_no_notices() {
    let mut model = at_noon();
    assert!(model.shown().notices.is_empty());
    let mut machine = Machine::reading(Ok(status(vec![
        account(Some("work"))
            .signed_in()
            .limits(vec![window("session", 10.0)])
            .build(),
        account(Some("spare")).build(),
    ])));
    model.refresh(&mut machine);
    assert!(model.shown().notices.is_empty());
}

/// An interrupted switch nothing can finish stops Pitboard working, so it is said first,
/// with the way out, which asks first. The warning the read carries about it is the same
/// fact and is not said a second time; anything else the read warned about still is.
///
/// PresentationTests.swift's anInterruptedSwitchIsSaidFirstWithTheWayOutAndOnlyOnce.
#[test]
fn an_interrupted_switch_is_said_first_with_the_way_out_and_only_once() {
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(warned(
        vec![
            account(Some("work")).signed_in().build(),
            account(Some("job")).of("codex").signed_in().build(),
        ],
        vec![interrupted(), overridden()],
    )));
    model.refresh(&mut machine);

    let notices = model.shown().notices;
    assert_eq!(notices.len(), 2);
    assert_eq!(
        notices[0],
        notice(
            "stuck",
            Severity::Error,
            "An interrupted switch is waiting",
            &[
                "An interrupted switch can’t be finished until Anthropic or OpenAI answers.",
                "Giving up on it keeps every login. Nothing is deleted.",
            ],
            vec![give_up()],
        )
    );
    assert_eq!(
        notices[1].title,
        "An environment variable overrides the login"
    );
    assert!(
        !notices
            .iter()
            .any(|notice| notice.lines.contains(&interrupted().message))
    );
}

/// When the read itself failed because of the interrupted switch, what stopped it is the
/// reason given, and it is not said again as a read that failed.
///
/// PresentationTests.swift's anInterruptedSwitchThatStoppedTheReadGivesItsReason.
#[test]
fn an_interrupted_switch_that_stopped_the_read_gives_its_reason() {
    let reason = "Anthropic could not be reached to finish the switch to work.";
    let mut model = at_noon();
    let mut machine = Machine::reading(Err(refusal(
        "recovery_undetermined",
        reason,
        vec![interrupted()],
    )));
    machine.offline = Ok(status(vec![account(Some("work")).signed_in().build()]));
    model.refresh(&mut machine);
    assert_eq!(
        model.shown().notices,
        [notice(
            "stuck",
            Severity::Error,
            "An interrupted switch is waiting",
            &[
                reason,
                "Giving up on it keeps every login. Nothing is deleted."
            ],
            vec![give_up()],
        )]
    );
}

/// A read that failed says why, and that the numbers shown are the last ones measured rather
/// than now's. A warning that only repeats why is not said twice; any other it carried is.
///
/// PresentationTests.swift's aFailedReadSaysWhyAndThatTheNumbersAreOld.
#[test]
fn a_failed_read_says_why_and_that_the_numbers_are_old() {
    let reason = "Anthropic could not be reached";
    let mut model = at_noon();
    let mut machine = Machine::reading(Err(refusal(
        "unreachable",
        reason,
        vec![warning("unreachable", reason), overridden()],
    )));
    machine.offline = Ok(status(vec![
        account(Some("work"))
            .signed_in()
            .limits(vec![window("session", 10.0)])
            .build(),
    ]));
    model.refresh(&mut machine);

    let notices = model.shown().notices;
    let titles: Vec<&str> = notices.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Couldn’t read usage",
            "An environment variable overrides the login"
        ]
    );
    assert_eq!(
        notices[0],
        notice(
            "read",
            Severity::Error,
            "Couldn’t read usage",
            &[reason, "The numbers shown are the last ones measured."],
            Vec::new(),
        )
    );
    assert_eq!(notices[1].lines, [overridden().message]);
}

/// The line saying the numbers shown are the last ones measured is said only where there are
/// numbers shown. With nothing measured, or nothing known at all, it pointed at numbers that
/// were not there.
///
/// PresentationTests.swift's aFailedReadSaysTheNumbersAreOldOnlyWhereThereAreSome.
#[test]
fn a_failed_read_says_the_numbers_are_old_only_where_there_are_some() {
    let reason = "Anthropic could not be reached";
    for known in [
        Ok(status(vec![
            account(Some("work")).signed_in().unmeasured().build(),
        ])),
        Ok(status(Vec::new())),
        Err(refusal(
            "state_unreadable",
            "~/.pitboard/state.json could not be read",
            Vec::new(),
        )),
    ] {
        let mut model = at_noon();
        let mut machine = Machine::reading(Err(refusal("unreachable", reason, Vec::new())));
        machine.offline = known;
        model.refresh(&mut machine);
        assert_eq!(model.shown().notices[0].lines, [reason]);
    }
}

/// A failed read with nothing to list is the window's whole content, with why and a way to
/// try again, and not a spinner that never stops or a list with nothing in it, whether
/// nothing is known at all or what is known is empty. A machine without Claude Code says that
/// instead. With accounts to show, the list stays, with the failure above it as a notice.
///
/// PresentationTests.swift's aFailedReadWithNothingToListIsWhatTheWindowShows.
#[test]
fn a_failed_read_with_nothing_to_list_is_what_the_window_shows() {
    let reason = "~/.pitboard/state.json was written on another Mac.";
    let failed = || Err(refusal("state_wrong_machine", reason, Vec::new()));
    let read_failed = AccountsShown::ReadFailed {
        title: "Couldn’t Read Accounts".into(),
        detail: reason.into(),
        retry: Choice {
            title: "Try Again".into(),
            intent: Intent::Refresh { asked: true },
        },
    };

    let mut unknown = at_noon();
    let mut nothing = Machine::reading(failed());
    nothing.offline = failed();
    unknown.refresh(&mut nothing);
    let shown = unknown.shown();
    assert_eq!(shown.status, None);
    assert_eq!(
        shown.footing,
        Footing::Ready,
        "not a machine without Claude Code"
    );
    assert_eq!(shown.accounts_shown, read_failed);

    let mut empty = at_noon();
    empty.refresh(&mut Machine::reading(failed()));
    let shown = empty.shown();
    assert_eq!(shown.status.map(|s| s.accounts.len()), Some(0));
    assert_eq!(
        shown.footing,
        Footing::NoOneSignedIn,
        "the read's problem is said ahead of it"
    );
    assert_eq!(shown.accounts_shown, read_failed);

    let mut some = at_noon();
    let mut listed = Machine::reading(failed());
    listed.offline = Ok(status(vec![
        account(Some("work"))
            .signed_in()
            .limits(vec![window("session", 5.0)])
            .build(),
    ]));
    some.refresh(&mut listed);
    let shown = some.shown();
    assert_eq!(ids(&shown), ["read"]);
    assert_eq!(shown.accounts_shown, AccountsShown::List);

    let mut missing = at_noon();
    let mut bare = Machine::reading(failed());
    bare.found = Vec::new();
    missing.refresh(&mut bare);
    let shown = missing.shown();
    assert_eq!(shown.footing, Footing::NoClaudeCode);
    assert!(matches!(shown.accounts_shown, AccountsShown::NoTool { .. }));
}

/// Without a tool there is nothing to read, and the menu says how to install one instead.
/// A failed read beside that would be the same fact said as a fault. Once another tool has
/// an account here the machine is not without a tool, and a failed read is one.
///
/// PresentationTests.swift's aMachineWithoutClaudeCodeIsNotToldItsReadFailed.
#[test]
fn a_machine_without_claude_code_is_not_told_its_read_failed() {
    let missing = || {
        let mut machine = Machine::reading(Err(refusal(
            "unreachable",
            "Anthropic could not be reached",
            Vec::new(),
        )));
        machine.found = Vec::new();
        machine
    };
    let mut bare = at_noon();
    bare.refresh(&mut missing());
    let shown = bare.shown();
    assert_eq!(shown.footing, Footing::NoClaudeCode);
    assert!(shown.notices.is_empty());

    let mut with_codex = at_noon();
    let mut machine = missing();
    machine.offline = Ok(status(vec![
        account(Some("job")).of("codex").signed_in().build(),
    ]));
    with_codex.refresh(&mut machine);
    assert_eq!(ids(&with_codex.shown()), ["read"]);
}

/// Sessions of a tool that follows a switch by itself pick it up within a moment, and the
/// notice counts down to it. Once that moment has passed a countdown would count to
/// something already over, so there is none. Dismissing the notice puts it away.
///
/// PresentationTests.swift's aSwitchCountsDownOnlyUntilOpenSessionsHaveFollowed.
#[test]
fn a_switch_counts_down_only_until_open_sessions_have_followed() {
    let (mut model, mut machine) = reading(vec![
        account(Some("work")).signed_in().build(),
        account(Some("personal")).build(),
    ]);
    machine.switched = switched("claude", "personal", "work", Vec::new());
    switch(&mut model, &mut machine, "claude/work");
    let follows = NOON + 33;

    let followed = notice(
        "switch/claude",
        Severity::Info,
        "Switched to work",
        &[],
        vec![dismiss(Intent::DismissSwitch {
            provider: "claude".into(),
        })],
    );
    let counting = PanelNotice {
        until: Some(follows),
        until_label: Some("Sessions already open follow in".into()),
        ..followed.clone()
    };
    model.later(Duration::from_secs(23));
    assert_eq!(model.shown().notices, [counting]);
    model.later(Duration::from_secs(10));
    assert_eq!(model.shown().notices, std::slice::from_ref(&followed));
    model.later(Duration::from_secs(60));
    assert_eq!(model.shown().notices, [followed]);

    model.send(Intent::DismissSwitch {
        provider: "claude".into(),
    });
    assert!(model.shown().notices.is_empty());
}

/// With two tools each switch is said on its own, so it names its tool, and so does its
/// countdown: one beside a Codex notice would otherwise read as contradicting it.
///
/// PresentationTests.swift's aSwitchNamesItsToolOnlyBesideAnother.
#[test]
fn a_switch_names_its_tool_only_beside_another() {
    let (mut model, mut machine) = reading(vec![
        account(Some("work")).signed_in().build(),
        account(Some("job")).of("codex").signed_in().build(),
    ]);
    machine.switched = switched("claude", "personal", "work", Vec::new());
    switch(&mut model, &mut machine, "claude/work");
    let notice = model.shown().notices.remove(0);
    assert_eq!(notice.title, "Switched Claude Code to work");
    assert_eq!(
        notice.until_label.as_deref(),
        Some("Claude Code sessions already open follow in")
    );
}

/// A running `codex` never picks a switch up, so a countdown would promise what does not
/// happen. The notice is a warning that says to start its sessions again, or, once the core
/// has counted them, the core's own warning, which says the same with the count.
///
/// PresentationTests.swift's aSwitchThatNeedsARestartIsAWarningWithoutACountdown.
#[test]
fn a_switch_that_needs_a_restart_is_a_warning_without_a_countdown() {
    let (mut model, mut machine) = reading(vec![
        account(Some("work")).of("codex").signed_in().build(),
        account(Some("personal")).of("codex").build(),
    ]);
    machine.switched = switched("codex", "codex/personal", "codex/work", Vec::new());
    switch(&mut model, &mut machine, "codex/work");
    assert_eq!(
        model.shown().notices,
        [notice(
            "switch/codex",
            Severity::Warning,
            "Switched to work",
            &[
                "Any codex session started before this switch keeps using personal until it \
               is quit and started again."
            ],
            vec![dismiss(Intent::DismissSwitch {
                provider: "codex".into()
            })],
        )]
    );

    let counted = warning(
        "sessions_still_running",
        "2 `codex` sessions started before this switch are still running.",
    );
    machine.switched = switched(
        "codex",
        "codex/personal",
        "codex/work",
        vec![counted.clone()],
    );
    switch(&mut model, &mut machine, "codex/work");
    let notices = model.shown().notices;
    let severities: Vec<Severity> = notices.iter().map(|n| n.severity).collect();
    assert_eq!(severities, [Severity::Warning]);
    assert_eq!(notices[0].lines, [counted.message]);
    assert_eq!(notices[0].until, None);
}

/// When the core could not count any sessions, the app's sentence is all there is, and it is
/// said of any session rather than of ones that may not exist.
///
/// AppModelTests.swift's aRestartIsExplainedWhenNoSessionsWereCounted.
#[test]
fn a_restart_is_explained_when_no_sessions_were_counted() {
    let (mut model, mut machine) =
        reading(vec![account(Some("work")).of("codex").signed_in().build()]);
    machine.switched = switched("codex", "codex/personal", "codex/work", Vec::new());
    switch(&mut model, &mut machine, "codex/work");
    assert_eq!(
        model.shown().notices[0].lines,
        [
            "Any codex session started before this switch keeps using personal until it is quit \
          and started again."
        ]
    );
}

/// A warning the read after a switch also carries is shown once, not twice: the switch's
/// notice leaves it to the read.
///
/// AppModelTests.swift's aSwitchWarningTheReadRepeatsIsSaidOnce, and PresentationTests.swift's
/// aWarningTheSwitchAndTheReadAfterItBothCarryIsSaidOnce, which the switch leaves to the read,
/// so the read has to say it: a warning both carry is said once, and not dropped from both.
#[test]
fn a_switch_warning_the_read_repeats_is_said_once() {
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(warned(
        vec![account(Some("b")).signed_in().build()],
        vec![overridden()],
    )));
    machine.switched = switched("claude", "a", "b", vec![overridden()]);
    switch(&mut model, &mut machine, "claude/b");
    let shown = model.shown();
    assert_eq!(shown.warnings, [overridden()]);
    let switched_notice = shown
        .notices
        .iter()
        .find(|notice| notice.id == "switch/claude")
        .expect("the switch's notice");
    assert!(switched_notice.lines.is_empty(), "{switched_notice:?}");
    let said = shown
        .notices
        .iter()
        .flat_map(|notice| &notice.lines)
        .filter(|line| **line == overridden().message)
        .count();
    assert_eq!(said, 1);
}

/// A warning a switch carried is said with the switch, where it belongs, and nowhere else.
///
/// PresentationTests.swift's aWarningTheLastSwitchCarriedIsSaidWithItAndNowhereElse.
#[test]
fn a_warning_the_last_switch_carried_is_said_with_it_and_nowhere_else() {
    let (mut model, mut machine) = reading(vec![account(Some("work")).signed_in().build()]);
    machine.switched = switched("claude", "personal", "work", vec![overridden()]);
    switch(&mut model, &mut machine, "claude/work");
    let shown = model.shown();
    assert_eq!(ids(&shown), ["switch/claude"]);
    assert_eq!(shown.notices[0].severity, Severity::Warning);
    assert_eq!(shown.notices[0].lines, [overridden().message]);
}

/// Signing in to the account in use puts its new login in use at once, and sessions already
/// running keep the old one, exactly as a switch leaves them on the old account. So it is
/// said the way a switch is: the account has a new login, and what that means for them.
///
/// PresentationTests.swift's aSignInThatPutANewLoginInUseSaysTheAccountHasOne.
#[test]
fn a_sign_in_that_put_a_new_login_in_use_says_the_account_has_one() {
    let (mut model, mut machine) = reading(vec![account(Some("work")).signed_in().build()]);
    machine.enrolling = enrolled_as(EnrolledAs::InUse { again: true }, Vec::new());
    signs_in(&mut model, &mut machine, "claude", "work", &[]);
    assert_eq!(
        model.shown().notices,
        [notice(
            "switch/claude",
            Severity::Info,
            "work has a new login",
            &["Signed in to work again. Its new login is the one in use now."],
            vec![dismiss(Intent::DismissSwitch {
                provider: "claude".into()
            })],
        )]
    );

    let old_login = warning(
        "sessions_keep_old_login",
        "2 `codex` sessions are still using `codex/work`'s old login.",
    );
    let (mut model, mut machine) =
        reading(vec![account(Some("work")).of("codex").signed_in().build()]);
    machine.enrolling = enrolled_as(EnrolledAs::InUse { again: false }, vec![old_login.clone()]);
    signs_in(&mut model, &mut machine, "codex", "work", &[]);
    assert_eq!(
        model.shown().notices,
        [notice(
            "switch/codex",
            Severity::Warning,
            "work has a new login",
            &[
                "Enrolled work, the account signed in now. Its new login is the one in use.",
                &old_login.message,
            ],
            vec![dismiss(Intent::DismissSwitch {
                provider: "codex".into()
            })],
        )]
    );
}

/// Two tools can each have a `work`, so once both are shown a notice about one account says
/// which tool it is for, as a switch's notice and advice both do.
///
/// PresentationTests.swift's aSignInNoticeNamesItsToolBesideAnother.
#[test]
fn a_sign_in_notice_names_its_tool_beside_another() {
    let (mut model, mut machine) = reading(vec![
        account(Some("work")).signed_in().build(),
        account(Some("work")).of("codex").signed_in().build(),
    ]);
    machine.enrolling = enrolled_as(EnrolledAs::InUse { again: true }, Vec::new());
    signs_in(&mut model, &mut machine, "codex", "work", &[]);
    let notice = model.shown().notices.remove(0);
    assert_eq!(notice.id, "switch/codex");
    assert_eq!(notice.title, "work in Codex has a new login");
}

/// Every warning a read carries is a notice of its own, headed from its code so the menu can
/// name it in a line, with the core's message under it saying what to do. Two warnings of
/// one code are two notices, each with an identity of its own.
///
/// PresentationTests.swift's eachWarningAReadCarriesIsANoticeHeadedFromItsCode.
#[test]
fn each_warning_a_read_carries_is_a_notice_headed_from_its_code() {
    let second = warning("auth_overridden", "CLAUDE_CODE_OAUTH_TOKEN is set");
    let novel = warning("something_new", "Something Pitboard does not know yet");
    let warnings = vec![overridden(), novel, second];
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(warned(
        vec![account(Some("work")).signed_in().build()],
        warnings.clone(),
    )));
    model.refresh(&mut machine);

    let notices = model.shown().notices;
    let titles: Vec<&str> = notices.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "An environment variable overrides the login",
            "Pitboard has a warning",
            "An environment variable overrides the login",
        ]
    );
    for (notice, warning) in notices.iter().zip(&warnings) {
        assert_eq!(notice.lines, std::slice::from_ref(&warning.message));
        assert_eq!(notice.severity, Severity::Warning);
        assert!(notice.actions.is_empty());
        assert!(
            notice.id.starts_with(&format!("warning/{}/", warning.code)),
            "{}",
            notice.id
        );
    }
    let distinct: std::collections::BTreeSet<&str> =
        notices.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(distinct.len(), warnings.len());
}

/// Two warnings alike are two notices still, each with an identity of its own, which the
/// Swift's `hashValue` gave both alike.
#[test]
fn two_warnings_alike_are_two_notices() {
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(warned(
        vec![account(Some("work")).signed_in().build()],
        vec![overridden(), overridden()],
    )));
    model.refresh(&mut machine);
    let shown = model.shown();
    assert_eq!(shown.notices.len(), 2);
    assert_ne!(shown.notices[0].id, shown.notices[1].id);
}

/// Giving up on an interrupted switch deletes nothing, and says so, with how many logins were
/// kept, until somebody puts it away.
///
/// PresentationTests.swift's givingUpOnASwitchSaysWhatWasKeptUntilPutAway, its notice.
#[test]
fn giving_up_on_a_switch_says_what_was_kept_until_put_away() {
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(warned(
        vec![account(Some("work")).signed_in().build()],
        vec![interrupted()],
    )));
    model.refresh(&mut machine);
    assert_eq!(ids(&model.shown()), ["stuck"]);

    machine.answer = Ok(status(vec![account(Some("work")).signed_in().build()]));
    machine.abandoned = Ok(Some(Abandoned {
        from: "personal".into(),
        to: "work".into(),
        logins_kept: 2,
    }));
    model.send(Intent::AbandonStuckSwitch);
    model.run(&mut machine);
    assert_eq!(
        model.shown().notices,
        [notice(
            "abandoned",
            Severity::Info,
            "Gave up on the interrupted switch",
            &[
                "The switch from personal to work was given up. 2 logins kept, and nothing \
               was deleted."
            ],
            vec![dismiss(Intent::DismissAbandoned)],
        )]
    );

    machine.abandoned = Ok(Some(Abandoned {
        from: "personal".into(),
        to: "work".into(),
        logins_kept: 1,
    }));
    model.send(Intent::AbandonStuckSwitch);
    model.run(&mut machine);
    assert_eq!(
        model.shown().notices[0].lines,
        [
            "The switch from personal to work was given up. 1 login kept, and nothing was \
          deleted."
        ]
    );

    model.send(Intent::DismissAbandoned);
    assert!(model.shown().notices.is_empty());
}

/// The most pressing first: what stops Pitboard working, then an account that ran out, then
/// what each tool's last switch said, then warnings, then what is only worth knowing.
///
/// PresentationTests.swift's noticesComeTheMostPressingFirst.
#[test]
fn notices_come_the_most_pressing_first() {
    let accounts = || {
        vec![
            account(Some("work"))
                .signed_in()
                .limits(vec![window("session", 10.0)])
                .build(),
            account(Some("spare"))
                .limits(vec![window("session", 20.0)])
                .build(),
            account(Some("job")).of("codex").signed_in().build(),
        ]
    };
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(warned(accounts(), vec![interrupted()])));
    model.refresh(&mut machine);

    machine.answer = Ok(status(accounts()));
    machine.abandoned = Ok(Some(Abandoned {
        from: "personal".into(),
        to: "work".into(),
        logins_kept: 2,
    }));
    model.send(Intent::AbandonStuckSwitch);
    model.run(&mut machine);

    machine.switched = switched("codex", "codex/side", "codex/job", Vec::new());
    switch(&mut model, &mut machine, "codex/job");

    machine.answer = Ok(status(vec![
        account(Some("work"))
            .signed_in()
            .limits(vec![window("session", 100.0)])
            .build(),
        account(Some("spare"))
            .limits(vec![window("session", 20.0)])
            .build(),
        account(Some("job")).of("codex").signed_in().build(),
    ]));
    model.refresh(&mut machine);

    machine.answer = Err(refusal(
        "unreachable",
        "Anthropic could not be reached",
        vec![overridden()],
    ));
    model.refresh(&mut machine);

    let notices = model.shown().notices;
    let kinds: Vec<&str> = notices
        .iter()
        .map(|n| n.id.split('/').next().unwrap_or_default())
        .collect();
    assert_eq!(kinds, ["read", "advice", "switch", "warning", "abandoned"]);
    let severities: Vec<Severity> = notices.iter().map(|n| n.severity).collect();
    assert_eq!(
        severities,
        [
            Severity::Error,
            Severity::Warning,
            Severity::Warning,
            Severity::Warning,
            Severity::Info
        ]
    );
}

/// Advice offers the account of the same tool with the most room, as a button that switches
/// to it by its label with its tool. It names the tool only once two are shown, so a machine
/// with one tool reads as it always did.
///
/// PresentationTests.swift's adviceOffersTheAccountWithRoomAndNamesItsToolOnlyBesideAnother.
#[test]
fn advice_offers_the_account_with_room_and_names_its_tool_only_beside_another() {
    let spent = || {
        vec![
            account(Some("work"))
                .signed_in()
                .limits(vec![window("session", 100.0)])
                .build(),
            account(Some("spare"))
                .limits(vec![window("session", 20.0)])
                .build(),
        ]
    };
    let (alone, _) = reading(spent());
    assert_eq!(
        alone.shown().notices,
        [notice(
            "advice/claude/work/session/",
            Severity::Warning,
            "work has no 5-hour limit left",
            &["spare has 80% of its own left."],
            vec![NoticeAction {
                title: "Switch to spare".into(),
                intent: Intent::SwitchTo {
                    qualified: "claude/spare".into()
                },
                dismisses: false,
                switches: true,
                enabled: true,
                confirm: None,
            }],
        )]
    );

    let mut beside = spent();
    beside.push(account(Some("job")).of("codex").signed_in().build());
    let (beside, _) = reading(beside);
    let titles: Vec<String> = beside
        .shown()
        .notices
        .into_iter()
        .map(|n| n.title)
        .collect();
    assert_eq!(titles, ["Claude Code: work has no 5-hour limit left"]);
}

/// A switch of one tool leaves another tool's accounts as they were. Advice about a Claude
/// Code account still out, beside another that still has room, is as true after a Codex
/// switch as before it, and it is never told again, so putting it away loses it.
///
/// PresentationTests.swift's adviceAboutOneToolOutlivesASwitchOfAnother.
#[test]
fn advice_about_one_tool_outlives_a_switch_of_another() {
    let spent = || {
        account(Some("work"))
            .signed_in()
            .limits(vec![window("session", 100.0)])
            .build()
    };
    let spare = || {
        account(Some("spare"))
            .limits(vec![window("session", 20.0)])
            .build()
    };
    let (mut model, mut machine) = reading(vec![
        spent(),
        spare(),
        account(Some("side")).of("codex").signed_in().build(),
        account(Some("job")).of("codex").build(),
    ]);
    assert_eq!(ids(&model.shown()), ["advice/claude/work/session/"]);

    machine.answer = Ok(status(vec![
        spent(),
        spare(),
        account(Some("side")).of("codex").build(),
        account(Some("job")).of("codex").signed_in().build(),
    ]));
    machine.switched = switched("codex", "codex/side", "codex/job", Vec::new());
    switch(&mut model, &mut machine, "codex/job");
    assert_eq!(
        ids(&model.shown()),
        ["advice/claude/work/session/", "switch/codex"]
    );
}

/// The menu turns advice into an item that switches, held back while a switch is under way,
/// and only advice: a notice offers an account when one of its actions is a switch to it.
///
/// PresentationTests.swift's aNoticeOffersAnAccountOnlyWhenItCanSwitchToOne, over what the
/// model says rather than notices a test made up.
#[test]
fn a_notice_offers_an_account_only_when_it_can_switch_to_one() {
    let (mut model, _) = reading(vec![
        account(Some("work"))
            .signed_in()
            .limits(vec![window("session", 100.0)])
            .build(),
        account(Some("spare"))
            .limits(vec![window("session", 20.0)])
            .build(),
    ]);
    let menu = model.shown().menu_notices;
    assert_eq!(
        menu.switches,
        [MenuEntry {
            title: "Switch to spare".into(),
            subtitle: Some("work has no 5-hour limit left".into()),
            help: None,
            severity: None,
            intent: Some(Intent::SwitchTo {
                qualified: "claude/spare".into()
            }),
            link: None,
            enabled: true,
        }]
    );
    assert_eq!(
        menu.others, None,
        "advice is not something else to know about"
    );

    model.send(Intent::SwitchTo {
        qualified: "claude/spare".into(),
    });
    let shown = model.shown();
    assert!(!shown.menu_notices.switches[0].enabled);
    assert!(!shown.notices[0].actions[0].enabled);
}

/// What can be done about a notice is a button under it, and putting it away is the icon at
/// its end: never both, and never neither.
///
/// PresentationTests.swift's aNoticesActionIsEitherAButtonOrWhatPutsItAway.
#[test]
fn a_notices_action_is_either_a_button_or_what_puts_it_away() {
    let accounts = |percent: f64| {
        vec![
            account(Some("work"))
                .of("codex")
                .signed_in()
                .limits(vec![window("five_hour", percent)])
                .build(),
            account(Some("spare"))
                .of("codex")
                .limits(vec![window("five_hour", 20.0)])
                .build(),
        ]
    };
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(warned(accounts(10.0), vec![interrupted()])));
    model.refresh(&mut machine);
    machine.switched = switched("codex", "codex/side", "codex/work", Vec::new());
    switch(&mut model, &mut machine, "codex/work");
    machine.abandoned = Ok(Some(Abandoned {
        from: "personal".into(),
        to: "work".into(),
        logins_kept: 2,
    }));
    model.send(Intent::AbandonStuckSwitch);
    model.run(&mut machine);
    machine.answer = Ok(warned(accounts(100.0), vec![interrupted()]));
    model.refresh(&mut machine);

    let actions: Vec<NoticeAction> = model
        .shown()
        .notices
        .into_iter()
        .flat_map(|notice| notice.actions)
        .collect();
    let titles: Vec<&str> = actions.iter().map(|a| a.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Give Up…", "Switch to spare", "Dismiss", "Dismiss"]
    );
    let dismisses: Vec<bool> = actions.iter().map(|a| a.dismisses).collect();
    assert_eq!(dismisses, [false, false, true, true]);
    let switches: Vec<bool> = actions.iter().map(|a| a.switches).collect();
    assert_eq!(switches, [false, true, false, false]);
}

/// The menu has no room for paragraphs: everything to know about that is more than a note is
/// one item that opens the window where it is said in full, named as the one notice it is
/// or counting them, and a machine without a tool is told how to install Claude Code.
///
/// MenuBarContent.swift's attention section, which the Swift tested nowhere.
#[test]
fn the_menu_says_what_needs_attention_in_one_item() {
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(warned(
        vec![account(Some("work")).signed_in().build()],
        vec![overridden()],
    )));
    model.refresh(&mut machine);
    let menu = model.shown().menu_notices;
    assert_eq!(menu.install, None);
    assert!(menu.switches.is_empty());
    assert_eq!(
        menu.others,
        Some(MenuEntry {
            title: "An environment variable overrides the login".into(),
            subtitle: Some("Show in Pitboard".into()),
            help: Some("ANTHROPIC_API_KEY is set".into()),
            severity: Some(Severity::Warning),
            intent: Some(Intent::ShowWindow {
                pane: Some(Pane::Accounts)
            }),
            link: None,
            enabled: true,
        })
    );

    machine.answer = Ok(warned(
        vec![account(Some("work")).signed_in().build()],
        vec![interrupted(), overridden()],
    ));
    model.refresh(&mut machine);
    let others = model.shown().menu_notices.others.expect("one item");
    assert_eq!(others.title, "2 things to look at");
    assert_eq!(
        others.subtitle.as_deref(),
        Some("An interrupted switch is waiting")
    );
    assert_eq!(others.help, None);
    assert_eq!(others.severity, Some(Severity::Error));

    // A note is not pressing enough for the menu.
    machine.answer = Ok(status(vec![account(Some("work")).signed_in().build()]));
    machine.abandoned = Ok(Some(Abandoned {
        from: "personal".into(),
        to: "work".into(),
        logins_kept: 1,
    }));
    model.send(Intent::AbandonStuckSwitch);
    model.run(&mut machine);
    assert_eq!(ids(&model.shown()), ["abandoned"]);
    assert_eq!(model.shown().menu_notices.others, None);

    let mut bare = at_noon();
    let mut nothing = Machine::reading(Ok(status(Vec::new())));
    nothing.found = Vec::new();
    bare.refresh(&mut nothing);
    let install = bare.shown().menu_notices.install.expect("how to install");
    assert_eq!(install.title, "Claude Code isn’t installed");
    assert_eq!(install.subtitle.as_deref(), Some("Learn how to install it"));
    assert_eq!(
        install.link.as_deref(),
        Some("https://docs.claude.com/en/docs/claude-code/setup")
    );
}

/// When the numbers were read is said as a time rather than an age, since a menu can stay
/// open, and "just now" would still say so ten minutes later; while a read runs, that it
/// does; before one has answered, that nothing is read yet or that the last numbers measured
/// are shown. The window's subtitle says nothing before the first read.
///
/// MenuBarContent.swift's and AccountsPane.swift's `updated`, which the Swift tested nowhere.
#[test]
fn when_the_numbers_were_read_is_said_as_a_time() {
    let mut model = at_noon();
    let shown = model.shown();
    assert_eq!(
        (shown.updated_menu.as_str(), shown.updated_window.as_str()),
        ("Not read yet", "")
    );
    let mut machine = Machine::reading(Ok(status(vec![account(Some("work")).signed_in().build()])));
    model.send(Intent::Refresh { asked: false });
    model.run_but(&mut machine, super::testing::any_read);
    let shown = model.shown();
    assert_eq!(
        (shown.updated_menu.as_str(), shown.updated_window.as_str()),
        ("Reading…", "Reading…")
    );
    model.run(&mut machine);
    let shown = model.shown();
    assert_eq!(
        (shown.updated_menu.as_str(), shown.updated_window.as_str()),
        ("Updated 12:00", "Updated 12:00")
    );

    let mut failed = at_noon();
    failed.refresh(&mut Machine::reading(Err(refusal(
        "unreachable",
        "Anthropic could not be reached",
        Vec::new(),
    ))));
    let shown = failed.shown();
    assert_eq!(shown.updated_menu, "Showing the last numbers measured");
    assert_eq!(shown.updated_window, "");
}

/// Where the menu has no accounts to list it says why, and the window says it in full: still
/// reading, nothing to show after a read that could not say, or nobody signed in, with how to
/// add an account. A machine without Claude Code says that alone.
///
/// MenuBarContent.swift's accounts section and AccountsPane.swift's content, which the
/// Swift tested nowhere.
#[test]
fn where_there_are_no_accounts_the_menu_and_the_window_say_why() {
    let model = at_noon();
    let shown = model.shown();
    assert_eq!(
        shown.menu_accounts_note.as_deref(),
        Some("Reading accounts…")
    );
    assert_eq!(
        shown.accounts_shown,
        AccountsShown::Reading {
            title: "Reading accounts…".into()
        }
    );

    let mut unknown = at_noon();
    let mut machine = Machine::reading(Err(refusal("unreachable", "x", Vec::new())));
    machine.offline = Err(refusal("state_unreadable", "y", Vec::new()));
    unknown.refresh(&mut machine);
    assert_eq!(
        unknown.shown().menu_accounts_note.as_deref(),
        Some("No accounts to show")
    );

    let (empty, _) = reading(Vec::new());
    let shown = empty.shown();
    assert_eq!(shown.menu_accounts_note.as_deref(), Some("No accounts yet"));
    assert_eq!(
        shown.accounts_shown,
        AccountsShown::NoAccounts {
            title: "No Accounts".into(),
            detail: "Sign in once here and Pitboard parks that login, so signing in to another \
                     account doesn’t cost you the first."
                .into(),
            add: Choice {
                title: "Add Account…".into(),
                intent: present_sheet(Sheet::Add { provider: None }),
            },
        }
    );

    let mut bare = at_noon();
    let mut nothing = Machine::reading(Ok(status(Vec::new())));
    nothing.found = Vec::new();
    bare.refresh(&mut nothing);
    let shown = bare.shown();
    assert_eq!(shown.menu_accounts_note, None);
    assert_eq!(
        shown.accounts_shown,
        AccountsShown::NoTool {
            title: "Claude Code Isn’t Installed".into(),
            detail: "Pitboard switches the logins of Claude Code and Codex, so there is nothing \
                     for it to do until one of them is installed and signed in once."
                .into(),
            link_title: "How to Install Claude Code".into(),
            link: "https://docs.claude.com/en/docs/claude-code/setup".into(),
        }
    );

    let (listed, _) = reading(vec![account(Some("work")).signed_in().build()]);
    assert_eq!(listed.shown().menu_accounts_note, None);
    assert_eq!(listed.shown().accounts_shown, AccountsShown::List);
}

// What a machine that is not set up yet is told to do.

fn footing(accounts: Vec<Account>) -> Footing {
    reading(accounts).0.shown().footing
}

/// A new install of the app. Before the first read there is nothing true to say, and a setup
/// step shown to somebody who finished it years ago is worse than silence.
///
/// AppModelTests.swift's nothingIsAskedOfAnyoneBeforeTheFirstRead.
#[test]
fn nothing_is_asked_of_anyone_before_the_first_read() {
    let shown = Hand::new().shown();
    assert_eq!(shown.footing, Footing::Ready);
    assert_eq!(shown.setup, None);
}

/// The one state Pitboard cannot do anything about. It has to say so rather than show an
/// empty panel, which reads as an app that does not work. The core's read succeeds with
/// nothing to list on such a machine, so what says it is that no tool was found.
///
/// AppModelTests.swift's aMachineWithoutClaudeCodeIsToldThatFirst.
#[test]
fn a_machine_without_claude_code_is_told_that_first() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    machine.found = Vec::new();
    assert_eq!(
        model.shown().footing,
        Footing::Ready,
        "nothing is said before the first read"
    );
    model.refresh(&mut machine);
    assert_eq!(model.shown().footing, Footing::NoClaudeCode);
}

/// A tool that was found, or a login or an account of any tool, means a tool is here, and
/// the machine is one to set up rather than one to install on.
///
/// AppModelTests.swift's aToolFoundOrSignedInIsNotAMachineWithoutOne.
#[test]
fn a_tool_found_or_signed_in_is_not_a_machine_without_one() {
    let mut found = Hand::new();
    let mut codex_only = Machine::reading(Ok(status(Vec::new())));
    codex_only.found = vec![codex()];
    found.refresh(&mut codex_only);
    assert_eq!(found.shown().footing, Footing::NoOneSignedIn);

    let mut unfound = Hand::new();
    let mut signed_in = Machine::reading(Ok(status(vec![
        account(Some("job")).of("codex").signed_in().build(),
    ])));
    signed_in.found = Vec::new();
    unfound.refresh(&mut signed_in);
    assert_ne!(unfound.shown().footing, Footing::NoClaudeCode);
}

/// AppModelTests.swift's whatIsInstalledIsAskedAgainWhileNothingIsFound, its footing: the
/// asking is reading.rs's. Once a tool is found the machine is one to sign in on.
#[test]
fn what_is_installed_is_asked_again_while_nothing_is_found() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    machine.found = Vec::new();
    model.refresh(&mut machine);
    assert_eq!(model.shown().footing, Footing::NoClaudeCode);
    machine.found = vec![claude_code()];
    model.refresh(&mut machine);
    assert_eq!(model.shown().footing, Footing::NoOneSignedIn);
}

/// AppModelTests.swift's anEmptyMachineIsAskedToSignInOnce.
#[test]
fn an_empty_machine_is_asked_to_sign_in_once() {
    assert_eq!(footing(Vec::new()), Footing::NoOneSignedIn);
}

/// A login with no name cannot be parked, so this is the step between signing in and
/// Pitboard being able to do anything at all. Until it has one it is called by its email
/// address, which is how the person knows it.
///
/// AppModelTests.swift's anAccountSignedInWithoutANameIsAskedForOne.
#[test]
fn an_account_signed_in_without_a_name_is_asked_for_one() {
    let (model, _) = reading(vec![account(None).signed_in().uuid("a").build()]);
    let shown = model.shown();
    assert_eq!(
        shown.footing,
        Footing::Unnamed {
            provider: "claude".into(),
            email: "a@example.com".into()
        }
    );
    assert_eq!(shown.sections[0].accounts[0].spoken_name, "a@example.com");
    assert_eq!(
        shown.setup,
        Some(SetupStep {
            title: "Give this account a name".into(),
            detail: "a@example.com is signed in. Pitboard parks logins under a name you \
                     choose, and can’t park this one until it has one."
                .into(),
            actions: vec![Choice {
                title: "Name…".into(),
                intent: present_sheet(Sheet::Name {
                    provider: "claude".into(),
                    email: "a@example.com".into()
                }),
            }],
        })
    );
}

/// AppModelTests.swift's oneEnrolledAccountIsToldThereIsNothingToSwitchTo, and what the step
/// above the accounts says of it.
#[test]
fn one_enrolled_account_is_told_there_is_nothing_to_switch_to() {
    let (model, _) = reading(vec![
        account(Some("work"))
            .signed_in()
            .limits(vec![window("session", 10.0)])
            .build(),
    ]);
    let shown = model.shown();
    assert_eq!(
        shown.footing,
        Footing::OnlyOne {
            provider: "claude".into(),
            label: "work".into()
        }
    );
    assert_eq!(
        shown.setup,
        Some(SetupStep {
            title: "Add a second account".into(),
            detail: "work is the only account Pitboard knows, so there’s nothing to switch to. \
                     Adding another signs in to it and parks its login beside this one."
                .into(),
            actions: vec![Choice {
                title: "Add Account…".into(),
                intent: present_sheet(Sheet::Add {
                    provider: Some("claude".into())
                }),
            }],
        })
    );
}

/// AppModelTests.swift's twoAccountsAreAskedNothing.
#[test]
fn two_accounts_are_asked_nothing() {
    assert_eq!(
        footing(vec![
            account(Some("work")).signed_in().build(),
            account(Some("personal")).build(),
        ]),
        Footing::Ready
    );
}

/// Mid-switch, and a login signed out from somewhere else, both leave accounts enrolled with
/// nobody signed in. Neither is a machine that needs setting up, and asking somebody to sign
/// in again there would have them sign in over an account Pitboard already holds.
///
/// AppModelTests.swift's enrolledAccountsWithNobodySignedInAreNotAskedToStartOver.
#[test]
fn enrolled_accounts_with_nobody_signed_in_are_not_asked_to_start_over() {
    assert_eq!(footing(vec![account(Some("work")).build()]), Footing::Ready);
}

/// Somebody signed in to Codex is not somebody nobody is signed in to.
///
/// AppModelTests.swift's aCodexLoginIsNotAnEmptyMachine.
#[test]
fn a_codex_login_is_not_an_empty_machine() {
    assert_eq!(
        footing(vec![account(Some("work")).of("codex").signed_in().build()]),
        Footing::OnlyOne {
            provider: "codex".into(),
            label: "work".into()
        }
    );
}

/// A Claude Code account and a Codex account are two accounts and nothing to switch to: an
/// account is only ever switched to another of its own tool. The step names the tool, once
/// there are two.
///
/// AppModelTests.swift's onlyOneAccountIsCountedPerTool.
#[test]
fn only_one_account_is_counted_per_tool() {
    let (model, _) = reading(vec![
        account(Some("work")).signed_in().build(),
        account(Some("spare")).build(),
        account(Some("job")).of("codex").signed_in().build(),
    ]);
    let shown = model.shown();
    assert_eq!(
        shown.footing,
        Footing::OnlyOne {
            provider: "codex".into(),
            label: "job".into()
        }
    );
    let step = shown.setup.expect("a step");
    assert_eq!(step.title, "Add a second Codex account");
    assert!(
        step.detail
            .starts_with("job is the only Codex account Pitboard knows"),
        "{}",
        step.detail
    );
}

/// A login Pitboard could not read, or cannot switch, has no account behind it. Offering to
/// name it would enrol something that can never be switched to.
///
/// AppModelTests.swift's anUnplacedLoginIsNeitherUnenrolledNorOfferedAName.
#[test]
fn an_unplaced_login_is_neither_unenrolled_nor_offered_a_name() {
    for row in [unplaced("codex", false), unplaced("codex", true)] {
        let (model, _) = reading(vec![
            account(Some("work")).signed_in().build(),
            account(Some("spare")).build(),
            account(Some("job")).of("codex").build(),
            account(Some("side")).of("codex").build(),
            row,
        ]);
        let shown = model.shown();
        assert_eq!(shown.footing, Footing::Ready);
        assert_eq!(shown.setup, None);
        assert!(
            rows(&model).iter().all(|row| !matches!(
                row.action.as_ref().map(|a| &a.intent),
                Some(Intent::PresentSheet {
                    sheet: Sheet::Name { .. }
                })
            )),
            "nothing to name"
        );
    }
}

// The sheets.

/// The model with the sheet for a new account up, on a machine where `found` were found and
/// `accounts` are shown, as a Mac's app has it.
fn adding(found: Vec<crate::Tool>, accounts: Vec<Account>) -> Snapshot {
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(status(accounts)));
    machine.found = found;
    model.refresh(&mut machine);
    model.send(present_sheet(Sheet::Add { provider: None }));
    model.run(&mut machine);
    on_a_mac(&model)
}

/// Only tools the app found a program for are offered for a new account, and the form says
/// which were left out rather than leaving them out without a word. A tool with an account
/// here is here, wherever its program is.
///
/// AppModelTests.swift's aNewAccountIsOfferedForToolsThatAreHere.
#[test]
fn a_new_account_is_offered_for_tools_that_are_here() {
    let claude_only = adding(vec![claude_code()], Vec::new())
        .sheet_text
        .expect("the sheet");
    assert_eq!(claude_only.tool, "claude");
    assert!(claude_only.tools.is_empty(), "nothing to pick from");
    assert_eq!(
        claude_only.not_offered.as_deref(),
        Some("Codex is not offered: Pitboard did not find codex on this Mac.")
    );

    let codex_only = adding(vec![codex()], Vec::new())
        .sheet_text
        .expect("the sheet");
    assert_eq!(codex_only.tool, "codex");
    assert!(codex_only.message.contains("Codex’s own sign-in"));

    let both = adding(vec![claude_code(), codex()], Vec::new())
        .sheet_text
        .expect("the sheet");
    assert_eq!(both.not_offered, None);
    let offered: Vec<&str> = both.tools.iter().map(|t| t.code.as_str()).collect();
    assert_eq!(offered, ["claude", "codex"]);
    assert!(both.tools[1].message.contains("Codex’s own sign-in"));

    let known = adding(
        vec![claude_code()],
        vec![account(Some("work")).of("codex").signed_in().build()],
    )
    .sheet_text
    .expect("the sheet");
    let offered: Vec<&str> = known.tools.iter().map(|t| t.code.as_str()).collect();
    assert_eq!(offered, ["claude", "codex"]);
}

/// A machine where the app found neither program reads as one with Claude Code alone, as it
/// always did: whatever it did not find where it looks is not on the PATH of an app opened
/// from Finder either, so offering every tool offered sign-ins that could not start. Who is
/// asked is Anthropic.
///
/// AppModelTests.swift's nothingFoundOffersClaudeCodeAlone.
#[test]
fn nothing_found_offers_claude_code_alone() {
    let sheet = adding(Vec::new(), Vec::new())
        .sheet_text
        .expect("the sheet");
    assert_eq!(sheet.tool, "claude");
    assert!(sheet.tools.is_empty());

    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(warned(Vec::new(), vec![interrupted()])));
    machine.found = Vec::new();
    model.refresh(&mut machine);
    assert_eq!(
        model.shown().notices[0].lines[0],
        "An interrupted switch can’t be finished until Anthropic answers."
    );
}

/// AppModelTests.swift's whatIsInstalledIsAskedOnce, what the sheet offers: the asking is
/// reading.rs's. Nothing is known to be here until a read asks.
#[test]
fn what_is_installed_is_asked_once() {
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    machine.found = vec![claude_code(), codex()];
    model.refresh(&mut machine);
    model.send(present_sheet(Sheet::Add { provider: None }));
    model.run(&mut machine);
    let offered: Vec<String> = model
        .shown()
        .sheet_text
        .expect("the sheet")
        .tools
        .into_iter()
        .map(|tool| tool.code)
        .collect();
    assert_eq!(offered, ["claude", "codex"]);
}

/// AppModelTests.swift's whatIsInstalledIsAskedAgainWhenTheFormForAnotherAccountOpens, what
/// the sheet offers: the asking is signing.rs's. The sheet offers what the answer found once
/// it is in.
#[test]
fn what_is_installed_is_asked_again_when_the_form_for_another_account_opens() {
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    model.refresh(&mut machine);
    machine.found = vec![claude_code(), codex()];
    model.send(present_sheet(Sheet::Add { provider: None }));
    assert!(
        model
            .shown()
            .sheet_text
            .expect("the sheet")
            .tools
            .is_empty(),
        "what was found before"
    );
    model.run(&mut machine);
    assert_eq!(model.shown().sheet_text.expect("the sheet").tools.len(), 2);
}

/// The form starts on the tool it was asked about, and otherwise on the first it offers.
///
/// AppModelTests.swift's theFormStartsOnTheToolItIsAbout.
#[test]
fn the_form_starts_on_the_tool_it_is_about() {
    let starts = |sheet: Sheet| {
        let mut model = Hand::new();
        model.send(present_sheet(sheet));
        model.shown().sheet_text.expect("the sheet").tool
    };
    assert_eq!(starts(Sheet::Add { provider: None }), "claude");
    assert_eq!(
        starts(Sheet::Add {
            provider: Some("codex".into())
        }),
        "codex"
    );
    assert_eq!(
        starts(Sheet::Name {
            provider: "codex".into(),
            email: "c@example.com".into()
        }),
        "codex"
    );
    assert_eq!(
        starts(Sheet::SignInAgain {
            provider: "codex".into(),
            label: "work".into()
        }),
        "codex"
    );
    assert_eq!(
        starts(Sheet::Rename {
            provider: "codex".into(),
            label: "work".into()
        }),
        "codex"
    );
}

/// Each sheet says what it is for, at the top: what signing in does and to whom, which login
/// is being named, what a rename changes. A rename starts with the name the account has.
///
/// NameSheets.swift's and SignInSheet.swift's titles and messages, which the Swift tested
/// nowhere.
#[test]
fn each_sheet_says_what_it_is_for() {
    let shown = |sheet: Sheet| {
        let mut model = Hand::new();
        model.send(present_sheet(sheet));
        model.shown().sheet_text.expect("the sheet")
    };
    let add = shown(Sheet::Add { provider: None });
    assert_eq!(add.title, "Add Account");
    assert_eq!(
        add.message,
        "Pitboard opens Claude Code’s own sign-in in your browser. Sign in as the account \
         you’re adding, and Pitboard parks its login beside the one in use."
    );
    assert_eq!((add.name.as_str(), add.prompt.as_str()), ("", "work"));
    assert_eq!(add.account, None);

    let again = shown(Sheet::SignInAgain {
        provider: "codex".into(),
        label: "work".into(),
    });
    assert_eq!(again.title, "Sign In to work Again");
    assert_eq!(
        again.message,
        "Pitboard opens Codex’s own sign-in in your browser. Sign in as work to give Pitboard \
         a new login for it."
    );
    assert_eq!(again.account.as_deref(), Some("work"));

    let name = shown(Sheet::Name {
        provider: "codex".into(),
        email: "c@example.com".into(),
    });
    assert_eq!(name.title, "Name This Account");
    assert_eq!(
        name.message,
        "c@example.com is signed in to Codex. Pitboard parks its login under this name \
         whenever you switch to another account."
    );

    let rename = shown(Sheet::Rename {
        provider: "claude".into(),
        label: "work".into(),
    });
    assert_eq!(rename.title, "Rename “work”");
    assert_eq!(
        rename.message,
        "The account keeps its parked login. Only the name you switch to it by changes, here \
         and in the command line."
    );
    assert_eq!(
        (rename.name.as_str(), rename.prompt.as_str()),
        ("work", "work")
    );
    assert!(!rename.saving);
}

/// A sign-in under way says what to do in the browser and what the field for a code is for,
/// and once the tool has refused a code typed back, why the field is offered again.
///
/// SignInSheet.swift's progress, which the Swift tested nowhere, and the sentence for a code
/// refused, which it never had.
#[test]
fn a_sign_in_under_way_says_what_to_do_in_the_browser() {
    let mut model = Hand::new();
    let jobs = model.send(Intent::SignIn {
        provider: "claude".into(),
        name: "travel".into(),
    });
    let [super::state::Job::SignIn { id, .. }] = jobs[..] else {
        panic!("a sign-in: {jobs:?}");
    };
    model.give(super::state::Answer::SignInStarted {
        id,
        started: Ok(()),
    });
    let text = model.shown().signing_in_text.expect("what it says");
    assert_eq!(text.title, "Signing In to Claude Code");
    assert_eq!(
        text.message,
        "Finish signing in as travel in your browser. This closes once Claude Code says \
         you’re in."
    );
    assert_eq!(
        text.code_note,
        "Claude Code takes the code shown after you sign in, whether or not your browser came \
         back to it."
    );
    assert_eq!(text.refused, None);

    model.give(super::state::Answer::SignInSaid {
        id,
        text: "Paste code here if prompted > ".into(),
    });
    model.send(Intent::PasteCode {
        code: "half".into(),
    });
    model.give(super::state::Answer::SignInSaid {
        id,
        text: "Invalid code. Please make sure the full code was copied.\n".into(),
    });
    assert_eq!(
        model.shown().signing_in_text.and_then(|text| text.refused),
        Some(
            "Claude Code didn’t take that code. Copy the whole code your browser shows, and \
             paste it again."
                .into()
        )
    );
}

/// The question about quitting the app that holds a tool's login is asked in the app's name,
/// while it is asked.
///
/// MainWindow.swift's alert, which the Swift tested nowhere.
#[test]
fn the_question_about_quitting_an_app_is_asked_in_its_name() {
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    machine.held.insert("codex".into(), vec![chatgpt_holding()]);
    machine.apps = StandInApps::new(&[CHATGPT], true);
    switch(&mut model, &mut machine, "codex/spare");
    let asked = model.shown().quit_confirmation.expect("the question");
    assert_eq!(asked.title, "Quit ChatGPT to switch?");
    assert_eq!(asked.confirm, "Quit ChatGPT and Switch");
    model.send(Intent::KeepAppOpen);
    assert_eq!(model.shown().quit_confirmation, None);
}

// The window.

/// A sheet is over the main window, so putting one up opens the window too, on the accounts
/// the sheet is about. Opening the window by itself leaves whatever sheet is up, and whichever
/// pane was showing.
///
/// RoutingTests.swift's aSheetIsPutUpOverTheWindow, the window opened by itself: putting the
/// sheet up is signing.rs's.
#[test]
fn opening_the_window_by_itself_leaves_the_sheet_up() {
    let rename = Sheet::Rename {
        provider: "claude".into(),
        label: "personal".into(),
    };
    let mut model = Hand::new();
    model.send(present_sheet(rename.clone()));
    model.send(Intent::ShowWindow { pane: None });
    let shown = model.shown();
    assert_eq!(
        shown.window_request,
        WindowRequest {
            serial: 2,
            pane: None
        }
    );
    assert_eq!(shown.sheet, Some(rename));
}

/// A request for the window can ask for a pane: a notice is said on the accounts pane, and
/// the menu's item for one opens the window there, whichever pane it was left on. A request
/// that asks for none leaves the pane as it is, so it does not carry a pane an earlier
/// request asked for, and a failure said in the window is over every pane.
///
/// RoutingTests.swift's aRequestForTheWindowCanAskForAPane.
#[test]
fn a_request_for_the_window_can_ask_for_a_pane() {
    let mut model = Hand::new();
    assert_eq!(model.shown().window_request.pane, None);
    model.send(Intent::ShowWindow {
        pane: Some(Pane::Machine),
    });
    assert_eq!(
        model.shown().window_request,
        WindowRequest {
            serial: 1,
            pane: Some(Pane::Machine)
        }
    );
    model.send(Intent::ShowWindow {
        pane: Some(Pane::Accounts),
    });
    assert_eq!(
        model.shown().window_request,
        WindowRequest {
            serial: 2,
            pane: Some(Pane::Accounts)
        }
    );
    model.send(Intent::ShowWindow { pane: None });
    assert_eq!(model.shown().window_request.pane, None);

    let mut machine = Machine::reading(Ok(status(Vec::new())));
    machine.forgetting = Err(refusal("account_unknown", "Nothing is parked.", Vec::new()));
    model.send(Intent::Forget {
        qualified: "claude/personal".into(),
    });
    model.run(&mut machine);
    assert_eq!(
        model.shown().window_request,
        WindowRequest {
            serial: 4,
            pane: None
        },
        "an alert is over every pane"
    );
}

/// A failure of something asked for away from the window is said in the window, which is
/// opened for it, as an alert with everything it warned about, until somebody has read it.
/// Nothing that went wrong opens nothing.
///
/// RoutingTests.swift's aFailureAwayFromTheWindowIsSaidInIt.
#[test]
fn a_failure_away_from_the_window_is_said_in_it() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    model.send(Intent::Forget {
        qualified: "claude/alpha".into(),
    });
    model.run(&mut machine);
    let shown = model.shown();
    assert_eq!(shown.failure, None);
    assert_eq!(shown.failure_alert, None);
    assert_eq!(shown.window_request.serial, 0);

    machine.forgetting = Err(refusal(
        "cannot_forget_active_account",
        "beta is the account in use, so it cannot be forgotten.",
        vec![overridden()],
    ));
    model.send(Intent::Forget {
        qualified: "claude/beta".into(),
    });
    model.run(&mut machine);
    let shown = model.shown();
    assert_eq!(shown.window_request.serial, 1);
    let alert = shown.failure_alert.expect("an alert");
    assert_eq!(alert.title, "Couldn’t forget beta");
    assert_eq!(
        alert.message,
        "beta is the account in use, so it cannot be forgotten.\n\nANTHROPIC_API_KEY is set"
    );

    model.send(Intent::DismissFailure);
    let shown = model.shown();
    assert_eq!(shown.failure, None);
    assert_eq!(shown.failure_alert, None);
    assert_eq!(
        shown.window_request.serial, 1,
        "putting it away opens nothing"
    );
}

/// A switch asked for from the menu or a notification that fails is said in the window,
/// since neither has anywhere to put a sentence. One that works opens nothing: the menu bar
/// already shows it.
///
/// RoutingTests.swift's onlyASwitchAskedForAwayFromTheWindowThatFailsOpensIt, on the model's
/// own state where the Swift drove its fixture.
#[test]
fn only_a_switch_asked_for_away_from_the_window_that_fails_opens_it() {
    let (mut model, mut machine) = reading(vec![
        account(Some("work")).signed_in().build(),
        account(Some("personal")).build(),
    ]);
    machine.switched = switched("claude", "work", "personal", Vec::new());
    switch(&mut model, &mut machine, "claude/personal");
    let shown = model.shown();
    assert_eq!(shown.failure, None);
    assert_eq!(shown.window_request.serial, 0);

    machine.switched = Err(refusal(
        "account_unknown",
        "There is no account called claude/nobody.",
        Vec::new(),
    ));
    switch(&mut model, &mut machine, "claude/nobody");
    let shown = model.shown();
    let failure = shown.failure.expect("said in the window");
    assert_eq!(failure.title, "Couldn’t switch to nobody");
    assert_eq!(failure.message, "There is no account called claude/nobody.");
    assert_eq!(shown.window_request.serial, 1);
}

/// What the window shows of the time is made again on the minute tick, once started, and on
/// nothing else: a reset that was "resets in 2h 05m" says "resets in 2h 04m" a minute later
/// with nothing read, as the Swift app's bars redrew every minute.
#[test]
fn what_depends_on_the_time_is_made_again_every_minute() {
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(status(vec![
        account(Some("work"))
            .signed_in()
            .limits(vec![
                window("session", 42.0).resets(Some(NOON + 2 * 3600 + 5 * 60)),
            ])
            .build(),
    ])));
    model.send(Intent::Start);
    model.run(&mut machine);
    let resets = |model: &Hand| {
        model.shown().sections[0].accounts[0].limits[0]
            .resets
            .clone()
    };
    assert_eq!(resets(&model), "resets in 2h 05m");
    let due = model.state.next_due().expect("a timer");
    assert!(due <= model.now.running + Duration::from_secs(60));

    model.later(Duration::from_secs(60));
    assert!(
        model
            .tick()
            .iter()
            .all(|job| matches!(job, super::state::Job::Look))
    );
    assert_eq!(resets(&model), "resets in 2h 04m");
    assert!(
        model.state.next_due().expect("a timer") <= model.now.running + Duration::from_secs(60),
        "and again a minute later"
    );
}

/// The words for who is asked follow the tools shown, so a machine with Claude Code alone
/// reads exactly as it did: here in what an interrupted switch waits for.
///
/// AppModelTests.swift's theServiceAskedIsNamedForTheToolsShown.
#[test]
fn the_service_asked_is_named_for_the_tools_shown() {
    let mut model = at_noon();
    let mut machine = Machine::reading(Ok(warned(
        vec![account(Some("work")).signed_in().build()],
        vec![interrupted()],
    )));
    model.refresh(&mut machine);
    let shown = model.shown();
    assert_eq!(
        shown.notices[0].lines[0],
        "An interrupted switch can’t be finished until Anthropic answers."
    );
    assert!(!shown.shows_tools);

    machine.answer = Ok(warned(
        vec![
            account(Some("work")).signed_in().build(),
            account(Some("job")).of("codex").signed_in().build(),
        ],
        vec![interrupted()],
    ));
    model.refresh(&mut machine);
    let shown = model.shown();
    assert_eq!(
        shown.notices[0].lines[0],
        "An interrupted switch can’t be finished until Anthropic or OpenAI answers."
    );
    assert!(shown.shows_tools);
}

/// A label as the core types it, taken apart: bare means Claude Code.
///
/// WordingTests.swift's aTypedLabelIsTakenApart.
#[test]
fn a_typed_label_is_taken_apart() {
    assert_eq!(super::state::split("codex/work"), ("codex", "work"));
    assert_eq!(super::state::split("work"), ("claude", "work"));
}
