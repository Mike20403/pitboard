//! Signing in, and the sheet a sign-in is started from, as the Swift model did them: its tests
//! on those, each under its own name in snake case, driven through `State::apply` by hand.
//! A sign-in's thread answers once its tool has started, then with each piece of what the
//! tool says, then once it has stopped saying anything, and a test hands those answers on in
//! whatever order the Swift tests reached with gates. Where a Swift test also checked what the
//! window says of it, which comes with the model's wording, the rest of it is kept here.

use super::changing::a_look_landing_after;
use super::lanes::Lane;
use super::state::{Answer, Job};
use super::switching::a_read_that_started_before;
use super::testing::{
    Hand, Machine, a_look_or_a_read, any_read, claude, claude_code, codex, codex_account,
    enrolled_as, installed_ask, refusal, status, still_running, switched, warning,
};
use super::{Intent, Pane, RestartNeeded, RunningSignIn, Sheet, WindowRequest};
use crate::EnrolledAs;

/// What `claude auth login` 2.1.289 writes once it reads a code typed back, as the register's
/// `sign_in_output` holds it.
const PROMPT: &str = "Paste code here if prompted > ";

/// What `claude auth login` 2.1.289 writes on refusing a code typed back, as the register's
/// `sign_in_takes_another_code` holds it.
const REFUSED: &str = "Invalid code. Please make sure the full code was copied.\n";

/// An address Codex prints for a person to open.
const CODEX_ADDRESS: &str = "https://auth.openai.com/oauth/authorize?state=x\n";

fn sign_in(provider: &str, name: &str) -> Intent {
    Intent::SignIn {
        provider: provider.into(),
        name: name.into(),
    }
}

fn paste(code: &str) -> Intent {
    Intent::PasteCode { code: code.into() }
}

fn present(sheet: Sheet) -> Intent {
    Intent::PresentSheet { sheet }
}

fn add(provider: Option<&str>) -> Sheet {
    Sheet::Add {
        provider: provider.map(str::to_owned),
    }
}

/// Puts `sheet` up and answers what that asks.
fn put_up(model: &mut Hand, machine: &mut Machine, sheet: Sheet) {
    model.send(present(sheet));
    model.run(machine);
}

/// Asks for a sign-in of `provider`'s tool as `name`, and answers its start: the id it was
/// given.
fn starts(model: &mut Hand, machine: &mut Machine, provider: &str, name: &str) -> u64 {
    let jobs = model.send(sign_in(provider, name));
    let [Job::SignIn { id, .. }] = &jobs[..] else {
        panic!("one sign-in asked for: {jobs:?}");
    };
    model.run(machine);
    *id
}

/// The sign-in under way, as the snapshot shows it.
fn running(model: &Hand) -> RunningSignIn {
    model.shown().signing_in.expect("a sign-in under way")
}

/// The tool of the sign-in `id` says `text`, as its thread hands it on.
fn says(model: &mut Hand, id: u64, text: &str) {
    model.give(Answer::SignInSaid {
        id,
        text: text.into(),
    });
}

/// The tool of the sign-in `id` has stopped saying anything, and everything that leads to.
fn ends(model: &mut Hand, machine: &mut Machine, id: u64) {
    model.give(Answer::SignInQuiet { id });
    model.run(machine);
}

/// A sign-in from start to finish, its tool saying `said` on the way, as the Swift tests
/// awaited `signIn`.
pub(super) fn signs_in(
    model: &mut Hand,
    machine: &mut Machine,
    provider: &str,
    name: &str,
    said: &[&str],
) {
    let id = starts(model, machine, provider, name);
    for text in said {
        says(model, id, text);
    }
    ends(model, machine, id);
}

fn sign_in_over(job: &Job) -> bool {
    matches!(job, Job::SignInOver { .. })
}

/// AppModelTests.swift's aSignInThatCannotStartIsReported. A sign-in that cannot start says
/// why in the sheet that started it, which stays up to say it, and leaves nothing half-shown.
#[test]
fn a_sign_in_that_cannot_start_is_reported() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    machine.starting = Err(refusal(
        "claude_program_missing",
        "`claude` is not on this machine",
        Vec::new(),
    ));
    put_up(&mut model, &mut machine, add(None));
    starts(&mut model, &mut machine, "claude", "work");

    let shown = model.shown();
    assert_eq!(machine.signed_in, ["claude/work"]);
    assert_eq!(shown.signing_in, None);
    assert_eq!(shown.sheet, Some(add(None)));
    let failure = shown.sheet_failure.expect("said in the sheet");
    assert_eq!(failure.title, "Couldn’t sign in to work");
    assert_eq!(failure.message, "`claude` is not on this machine");
    assert_eq!(failure.code.as_deref(), Some("claude_program_missing"));
    assert_eq!(shown.failure, None, "not in an alert over it");
    assert_eq!(shown.read_failure, None);
}

/// AppModelTests.swift's aCodexSignInIsForCodex. Codex's sign-in prints an address and reads
/// nothing, so the tool is named in the label the core is given, and the sign-in finishes and
/// enrols.
#[test]
fn a_codex_sign_in_is_for_codex() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    put_up(&mut model, &mut machine, add(Some("codex")));
    signs_in(&mut model, &mut machine, "codex", "work", &[CODEX_ADDRESS]);
    assert_eq!(machine.signed_in, ["codex/work"]);
    assert_eq!(machine.over, [(1, true)], "finished and enrolled");
    let shown = model.shown();
    assert_eq!(shown.signing_in, None);
    assert_eq!(shown.sheet, None, "and the sheet that started it is done");
}

/// AppModelTests.swift's aSignInToTheAccountInUseSaysItsNewLoginIsInUse, apart from the notice,
/// which comes with what the window says. Signing in again to the account in use puts its new
/// login in use at once. It says so, and what the core warned about sessions still on the old
/// login stays beside it until the tool has another account in use, the way a switch's
/// warning does, rather than among the read's warnings, which the next read replaces.
#[test]
fn a_sign_in_to_the_account_in_use_says_its_new_login_is_in_use() {
    let old_login = warning(
        "sessions_keep_old_login",
        "2 `codex` sessions started before this sign-in are still running and still using \
         `codex/work`'s old login.",
    );
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(vec![codex_account("work", true)])));
    machine.enrolling = enrolled_as(EnrolledAs::InUse { again: true }, vec![old_login.clone()]);
    signs_in(&mut model, &mut machine, "codex", "work", &[CODEX_ADDRESS]);

    let shown = model.shown();
    let [last] = &shown.last_switches[..] else {
        panic!("what the sign-in said: {:#?}", shown.last_switches);
    };
    assert_eq!(
        (last.provider.as_str(), last.to.as_str()),
        ("codex", "codex/work")
    );
    assert_eq!(
        last.said.as_deref(),
        Some("Signed in to work again. Its new login is the one in use now.")
    );
    assert_eq!(last.restart, None, "nothing switched away from anything");
    assert_eq!(last.follows_at, None);
    assert_eq!(last.warnings, std::slice::from_ref(&old_login));
    assert!(
        !shown.warnings.contains(&old_login),
        "said once, with the sign-in"
    );
}

/// AppModelTests.swift's aFirstSignInToTheAccountInUseSaysItWasEnrolled. A browser often signs
/// in to the session it already has, so the account signed in now can be enrolled by a
/// sign-in under a new name. It says it was enrolled, not signed in to again.
#[test]
fn a_first_sign_in_to_the_account_in_use_says_it_was_enrolled() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(vec![codex_account("work", true)])));
    machine.enrolling = enrolled_as(EnrolledAs::InUse { again: false }, Vec::new());
    signs_in(&mut model, &mut machine, "codex", "work", &[]);
    assert_eq!(
        model.shown().last_switches[0].said.as_deref(),
        Some("Enrolled work, the account signed in now. Its new login is the one in use.")
    );
}

/// AppModelTests.swift's aSignInToTheAccountInUseKeepsWhatTheLastSwitchSaid. The tool did not
/// switch, so what its last switch said about sessions still using the account it left is
/// still true, and stays: the restart it asked for, and the warning not to sign out inside
/// one. The sign-in's own count of the same sessions, naming this account's old login, would
/// contradict it and is not added.
#[test]
fn a_sign_in_to_the_account_in_use_keeps_what_the_last_switch_said() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(vec![
        codex_account("work", true),
        claude("personal", false, 0.0),
    ])));
    machine.switched = switched(
        "codex",
        "codex/personal",
        "codex/work",
        vec![still_running()],
    );
    model.send(Intent::SwitchTo {
        qualified: "codex/work".into(),
    });
    model.run(&mut machine);

    let old_login = warning("sessions_keep_old_login", "2 sessions");
    let parked = warning("written_on_the_command_line", "on the argument line");
    machine.enrolling = enrolled_as(
        EnrolledAs::InUse { again: true },
        vec![old_login, parked.clone()],
    );
    signs_in(&mut model, &mut machine, "codex", "work", &[]);

    let shown = model.shown();
    let [last] = &shown.last_switches[..] else {
        panic!("one tool's: {:#?}", shown.last_switches);
    };
    assert_eq!(
        last.restart,
        Some(RestartNeeded {
            program: "codex".into(),
            from: "personal".into(),
        })
    );
    assert_eq!(
        last.said.as_deref(),
        Some("Signed in to work again. Its new login is the one in use now.")
    );
    assert_eq!(last.warnings, [still_running(), parked]);
}

/// AppModelTests.swift's aSignInThatWasParkedSaysWhatItWarnedAbout. A sign-in that parked its
/// login rather than put it in use says why, after the read that follows it, which would
/// otherwise put the warning away.
#[test]
fn a_sign_in_that_was_parked_says_what_it_warned_about() {
    let untold = warning(
        "sign_in_parked_not_in_use",
        "Codex goes on with the login it has",
    );
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(vec![codex_account("work", true)])));
    machine.enrolling = enrolled_as(EnrolledAs::Renewed, vec![untold.clone()]);
    let id = starts(&mut model, &mut machine, "codex", "work");
    model.give(Answer::SignInQuiet { id });
    model.run_but(&mut machine, any_read);
    assert!(
        model.shown().warnings.is_empty(),
        "not before the read lands"
    );
    let read = model.take(any_read);
    model.give(machine.answer(read));

    let shown = model.shown();
    assert_eq!(shown.warnings, [untold]);
    assert!(shown.last_switches.is_empty());
}

/// AppModelTests.swift's aRefusedSignInSaysWhatItFoundOnTheWay, from the sheet a sign-in is
/// started from. A sign-in refused before it started says what that refusal found on the
/// way, such as a switch interrupted earlier and finished now, not only why it was refused.
/// The panel says it too, since what was found is about the machine and outlives the sheet.
#[test]
fn a_refused_sign_in_says_what_it_found_on_the_way() {
    let recovered = warning(
        "interrupted_switch_undone",
        "an earlier switch was interrupted",
    );
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    machine.starting = Err(refusal(
        "claude_program_missing",
        "`claude` is not on this machine",
        vec![recovered.clone()],
    ));
    put_up(&mut model, &mut machine, add(None));
    starts(&mut model, &mut machine, "claude", "work");

    let shown = model.shown();
    let failure = shown.sheet_failure.expect("said in the sheet");
    assert_eq!(failure.message, "`claude` is not on this machine");
    assert_eq!(failure.warnings, std::slice::from_ref(&recovered));
    assert_eq!(shown.warnings, [recovered]);
}

/// AppModelTests.swift's aSignInOfAnotherAccountSaysNothingMore. A sign-in of another account
/// adds a row and says nothing more.
#[test]
fn a_sign_in_of_another_account_says_nothing_more() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(vec![codex_account("work", true)])));
    signs_in(
        &mut model,
        &mut machine,
        "codex",
        "personal",
        &[CODEX_ADDRESS],
    );
    let shown = model.shown();
    assert!(shown.last_switches.is_empty());
    assert!(shown.warnings.is_empty());
    assert_eq!((shown.failure, shown.sheet_failure), (None, None));
}

/// AppModelTests.swift's aCodeIsAskedForOnlyWhereTheToolTakesOne. Whether a code is asked for
/// is the core's to read, in each tool's own words, and its cases are the core's tests. The
/// sheet asks it about the tool being signed in to, and a code typed back is not asked for
/// again. What is typed goes to the tool on the lane of sign-in calls, apart from the actor
/// and from the sign-in's own thread, which waits on the browser.
#[test]
fn a_code_is_asked_for_only_where_the_tool_takes_one() {
    for (provider, takes) in [("claude", true), ("codex", false)] {
        let mut model = Hand::new();
        let mut machine = Machine::reading(Ok(status(Vec::new())));
        let id = starts(&mut model, &mut machine, provider, "work");
        says(&mut model, id, PROMPT);
        assert_eq!(running(&model).wants_code, takes, "{provider}");

        let typed = model.send(paste("abc"));
        if takes {
            assert_eq!(
                typed,
                [Job::PasteCode {
                    id,
                    code: "abc".into()
                }]
            );
            assert_eq!(typed[0].lane(), Lane::SignInCalls);
            assert!(!running(&model).wants_code, "asked once");
        } else {
            assert_eq!(typed, [], "nothing is typed to a tool that reads nothing");
        }
        ends(&mut model, &mut machine, id);
        assert_eq!(model.shown().signing_in, None);
    }
}

/// AppModelTests.swift's aCancelledSignInStopsTheToolAndReportsNothing. Cancel stops the tool
/// on the lane of sign-in calls: stopping waits for the tool, and a Codex sign-in waiting on
/// the browser held the whole app while it did. What the stopped tool leaves behind is not a
/// failure to report, and nothing is enrolled.
#[test]
fn a_cancelled_sign_in_stops_the_tool_and_reports_nothing() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    let id = starts(&mut model, &mut machine, "codex", "work");
    says(&mut model, id, CODEX_ADDRESS);
    assert!(running(&model).url.is_some());

    let stop = model.send(Intent::CancelSignIn);
    assert_eq!(stop, [Job::StopSignIn { id }]);
    assert_eq!(stop[0].lane(), Lane::SignInCalls);
    assert_eq!(model.shown().signing_in, None);
    model.run(&mut machine);
    ends(&mut model, &mut machine, id);

    assert_eq!(machine.stopped, [id], "the tool is stopped");
    assert_eq!(machine.over, [(id, false)], "and nothing is enrolled");
    let shown = model.shown();
    assert!(shown.warnings.is_empty());
    assert_eq!((shown.failure, shown.sheet_failure), (None, None));
    assert_eq!(
        model.count(any_read),
        0,
        "nothing changed, so nothing is read"
    );
}

/// AppModelTests.swift's anAccountThatCannotBeSwitchedToIsSignedInToAgainFromThePanel, apart
/// from the row's action, which comes with what the window says. An account whose parked
/// login can no longer be used is signed in to again through the sign-in a new account gets,
/// so the address Codex prints shows the same way. The sheet has the label alone, and the
/// core is given it with its tool once, as for a new account.
#[test]
fn an_account_that_cannot_be_switched_to_is_signed_in_to_again() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(vec![
        codex_account("personal", true),
        codex_account("work", false),
        claude("spare", false, 0.0),
    ])));
    model.refresh(&mut machine);
    let again = Sheet::SignInAgain {
        provider: "codex".into(),
        label: "work".into(),
    };
    put_up(&mut model, &mut machine, again);
    let id = starts(&mut model, &mut machine, "codex", "work");
    says(&mut model, id, CODEX_ADDRESS);
    let shown = running(&model);
    assert_eq!(machine.signed_in, ["codex/work"]);
    assert_eq!(
        (shown.provider.as_str(), shown.name.as_str()),
        ("codex", "work")
    );
    assert!(shown.url.is_some());
    ends(&mut model, &mut machine, id);
    assert_eq!(machine.over, [(id, true)]);
    assert_eq!(model.shown().signing_in, None);
    assert_eq!(model.shown().sheet, None);

    let spare = Sheet::SignInAgain {
        provider: "claude".into(),
        label: "spare".into(),
    };
    put_up(&mut model, &mut machine, spare);
    signs_in(&mut model, &mut machine, "claude", "spare", &[]);
    assert_eq!(machine.signed_in, ["codex/work", "claude/spare"]);
}

/// AppModelTests.swift's aSecondSignInWhileOneRunsStartsNothing. One sign-in at a time. A
/// second asked for while one runs would take the place of the one on screen and leave the
/// first tool waiting on a browser with nothing to stop it.
#[test]
fn a_second_sign_in_while_one_runs_starts_nothing() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    let first = starts(&mut model, &mut machine, "codex", "work");
    says(&mut model, first, CODEX_ADDRESS);

    assert_eq!(model.send(sign_in("codex", "home")), []);
    let shown = running(&model);
    assert_eq!((shown.id, shown.name.as_str()), (first, "work"));
    ends(&mut model, &mut machine, first);
    assert_eq!(machine.signed_in, ["codex/work"]);
    assert_eq!(model.shown().signing_in, None);
}

/// AppModelTests.swift's aSignInCancelledWhileItStartsStopsWhatStarted. Cancel can be pressed
/// before the tool has started. What then starts is stopped rather than watched, and nothing
/// is enrolled or said.
#[test]
fn a_sign_in_cancelled_while_it_starts_stops_what_started() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    model.send(sign_in("codex", "work"));
    let start = model.next();
    let Job::SignIn { id, .. } = start else {
        panic!("the sign-in asked for: {start:?}");
    };
    assert!(model.shown().signing_in.is_some());

    assert_eq!(model.send(Intent::CancelSignIn), [], "nothing has started");
    assert_eq!(model.shown().signing_in, None);
    let stop = model.give(machine.answer(start));
    assert_eq!(stop, [Job::StopSignIn { id }], "what started is stopped");
    model.run(&mut machine);
    says(&mut model, id, CODEX_ADDRESS);
    ends(&mut model, &mut machine, id);

    assert_eq!(machine.over, [(id, false)]);
    let shown = model.shown();
    assert_eq!(shown.signing_in, None);
    assert!(shown.warnings.is_empty());
    assert!(shown.last_switches.is_empty());
    assert_eq!((shown.failure, shown.sheet_failure), (None, None));
}

fn a_sign_in(job: &Job) -> bool {
    matches!(job, Job::SignIn { .. })
}

/// Cancel and then Sign In at once, as a person can press one after the other. The sign-in
/// cancelled holds the core's one sign-in at a time until its tool has stopped, so the new one
/// is shown as starting meanwhile, in the sheet it was started from, and asked for only once
/// the cancelled one has let go: as the stop on the lane of sign-in calls answers, where it
/// stopped the tool, or else once its own thread has answered last. Asked for at once, it
/// started on a thread of its own before the stop had run, and was refused as a sign-in
/// already waiting. Asked for only once that thread had answered, it waited as long as a
/// program the tool started held the tool's output open, as `codex login` does once Codex's
/// npm launcher is killed.
#[test]
fn a_sign_in_asked_for_after_a_cancel_starts_once_the_cancelled_one_has_stopped() {
    for stops_its_tool in [true, false] {
        let case = if stops_its_tool {
            "the stop stopped its tool"
        } else {
            "it had gone quiet before the stop"
        };
        let mut model = Hand::new();
        let mut machine = Machine::reading(Ok(status(Vec::new())));
        put_up(&mut model, &mut machine, add(None));
        let first = starts(&mut model, &mut machine, "claude", "work");
        says(&mut model, first, PROMPT);

        assert_eq!(
            model.send(Intent::CancelSignIn),
            [Job::StopSignIn { id: first }],
            "{case}"
        );
        assert_eq!(
            model.send(sign_in("claude", "work")),
            [],
            "{case}: nothing asked for while the first is stopping"
        );
        let waiting = running(&model);
        assert_ne!(waiting.id, first);
        assert_eq!(
            (waiting.said.as_str(), waiting.url, waiting.wants_code),
            ("", None, false),
            "{case}: shown as starting"
        );
        assert_eq!(model.shown().sheet, Some(add(None)), "{case}");
        assert_eq!(
            model.send(paste("early#code")),
            [],
            "{case}: nothing to type to yet"
        );

        let stop = model.next();
        assert_eq!(stop, Job::StopSignIn { id: first }, "{case}");
        let asked_for = Job::SignIn {
            id: waiting.id,
            qualified: "claude/work".into(),
        };
        if stops_its_tool {
            // As the lane answers once it has stopped the tool and waited for it. The
            // thread may go on reading what the tool said for as long as its output is open.
            assert_eq!(
                model.give(Answer::SignInStopped { id: first }),
                [asked_for],
                "{case}: asked for as the stop answers"
            );
            model.run(&mut machine);
            says(&mut model, waiting.id, PROMPT);
            assert!(running(&model).wants_code, "{case}");
            assert_eq!(
                model.give(Answer::SignInQuiet { id: first }),
                [Job::SignInOver {
                    id: first,
                    enrol: false
                }],
                "{case}"
            );
            model.run(&mut machine);
            assert_eq!(model.count(a_sign_in), 2, "{case}: nothing asked for again");
            assert_eq!(running(&model).id, waiting.id, "{case}");
            assert!(
                running(&model).wants_code,
                "{case}: the new one left as it was"
            );
        } else {
            // As the lane answers where the tool had stopped saying anything first: the
            // thread sees to the tool, and its last answer is what lets the next start.
            assert_eq!(
                model.give(machine.answer(stop)),
                [],
                "{case}: a stop that stopped nothing is not over"
            );
            assert_eq!(
                model.give(Answer::SignInQuiet { id: first }),
                [Job::SignInOver {
                    id: first,
                    enrol: false
                }],
                "{case}"
            );
            let over = model.next();
            assert_eq!(
                model.give(machine.answer(over)),
                [asked_for],
                "{case}: asked for once the first's thread is over"
            );
            model.run(&mut machine);
            says(&mut model, waiting.id, PROMPT);
            assert!(running(&model).wants_code, "{case}");
        }
        assert_eq!(machine.signed_in, ["claude/work", "claude/work"], "{case}");
        assert_eq!(machine.over, [(first, false)], "{case}");
        let shown = model.shown();
        assert_eq!((shown.failure, shown.sheet_failure), (None, None), "{case}");
    }
}

/// However a sign-in cancelled before it lets go of the core's one sign-in at a time, that is
/// what lets the one asked for since start: its tool failing to start, or starting and then
/// being stopped as it starts, said by the stop where it stopped the tool and by its thread
/// otherwise, or its start or its finish lost to a panic, or what it signed in to enrolled
/// too late to stop. Its thread answering last after the stop said so asks for nothing more.
/// A sign-in cancelled while it waits is never asked for, and once nothing is left of the one
/// before it the next starts at once.
#[test]
fn a_sign_in_waiting_on_a_cancelled_one_starts_once_that_one_has_let_go() {
    type Ends = fn(&mut Hand, &mut Machine, u64);
    let cases: [(&str, Ends); 6] = [
        ("could not start", |model, _, id| {
            model.give(Answer::SignInStarted {
                id,
                started: Err(refusal("sign_in_incomplete", "no", Vec::new()).error()),
            });
        }),
        (
            "stopped as it started, by its thread",
            |model, machine, id| {
                model.give(Answer::SignInStarted {
                    id,
                    started: Ok(()),
                });
                model.run(machine);
                assert_eq!(machine.stopped, [id]);
                model.give(Answer::SignInQuiet { id });
                model.run(machine);
            },
        ),
        (
            "stopped as it started, by the stop",
            |model, machine, id| {
                model.give(Answer::SignInStarted {
                    id,
                    started: Ok(()),
                });
                assert_eq!(
                    model.take(|job| matches!(job, Job::StopSignIn { .. })),
                    Job::StopSignIn { id }
                );
                model.give(Answer::SignInStopped { id });
                model.run_but(machine, a_sign_in);
                model.give(Answer::SignInQuiet { id });
                model.run_but(machine, a_sign_in);
            },
        ),
        ("its start lost", |model, _, id| {
            model.give(Answer::Lost(Job::SignIn {
                id,
                qualified: "claude/one".into(),
            }));
        }),
        ("its finish lost", |model, _, id| {
            model.give(Answer::SignInStarted {
                id,
                started: Ok(()),
            });
            model.give(Answer::Lost(Job::SignInOver { id, enrol: true }));
        }),
        ("enrolled too late to stop", |model, machine, id| {
            model.give(Answer::SignInStarted {
                id,
                started: Ok(()),
            });
            model.give(Answer::SignInFinished {
                id,
                done: machine.enrolling.clone().map_err(|refused| refused.error()),
            });
            model.run_but(machine, a_sign_in);
        }),
    ];
    for (case, ends) in cases {
        let mut model = Hand::new();
        let mut machine = Machine::reading(Ok(status(Vec::new())));
        model.send(sign_in("claude", "one"));
        let Job::SignIn { id: first, .. } = model.next() else {
            panic!("{case}: the first sign-in asked for");
        };
        model.send(Intent::CancelSignIn);
        assert_eq!(model.send(sign_in("claude", "two")), [], "{case}");
        let second = running(&model).id;
        ends(&mut model, &mut machine, first);
        let asked: Vec<&Job> = model.asked.iter().filter(|job| a_sign_in(job)).collect();
        assert_eq!(
            asked,
            [
                &Job::SignIn {
                    id: first,
                    qualified: "claude/one".into()
                },
                &Job::SignIn {
                    id: second,
                    qualified: "claude/two".into()
                }
            ],
            "{case}"
        );
        assert_eq!(running(&model).id, second, "{case}");
        assert_eq!(
            model.shown().failure,
            None,
            "{case}: nothing said of the first"
        );
    }

    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    let first = starts(&mut model, &mut machine, "claude", "one");
    model.send(Intent::CancelSignIn);
    model.send(sign_in("claude", "two"));
    model.send(Intent::CancelSignIn);
    assert_eq!(model.shown().signing_in, None);
    model.run(&mut machine);
    ends(&mut model, &mut machine, first);
    assert_eq!(model.count(a_sign_in), 1, "the second was never asked for");
    assert_eq!(model.send(sign_in("claude", "three")).len(), 1, "at once");
}

/// AppModelTests.swift's aFinishedSignInClosesOnlyTheSheetItStartedFrom. A sign-in that
/// finishes closes the sheet it was started from. A sheet up by then would be somebody
/// else's, such as a name half typed, and closing it threw that away. Here no other sheet can
/// be up while a sign-in runs, since it keeps its own and nothing puts one up any other way,
/// as the Swift test's direct assignment did: so one closed meanwhile stays closed, and the
/// next is put up once the sign-in is over.
#[test]
fn a_finished_sign_in_closes_only_the_sheet_it_started_from() {
    let rename = Sheet::Rename {
        provider: "claude".into(),
        label: "home".into(),
    };
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    put_up(&mut model, &mut machine, add(Some("codex")));
    let id = starts(&mut model, &mut machine, "codex", "work");
    says(&mut model, id, CODEX_ADDRESS);

    put_up(&mut model, &mut machine, rename.clone());
    assert_eq!(model.shown().sheet, Some(add(Some("codex"))));
    model.send(Intent::CloseSheet);
    put_up(&mut model, &mut machine, rename.clone());
    assert_eq!(model.shown().sheet, None);

    ends(&mut model, &mut machine, id);
    assert_eq!(machine.over, [(id, true)]);
    assert_eq!(model.shown().signing_in, None);
    assert_eq!(model.shown().sheet, None);
    put_up(&mut model, &mut machine, rename.clone());
    assert_eq!(model.shown().sheet, Some(rename));
}

/// AppModelTests.swift's aSignInCancelledWhileItFinishesLeavesWhatCameAfterIt, both ways.
/// Cancel pressed while what the tool signed in to is being enrolled comes too late to stop
/// it. It is enrolled all the same, so the accounts are read again, and nothing else is
/// touched: the sign-in and the sheet on screen by then are somebody else's, a sign-in started
/// since among them. Where the cancel reached the tool first and finishing fails as stopped,
/// that was somebody's answer and says nothing.
#[test]
fn a_sign_in_cancelled_while_it_finishes_leaves_what_came_after_it() {
    for stopped_first in [false, true] {
        let travel = codex_account("travel", false);
        let mut model = Hand::new();
        let mut machine = Machine::reading(Ok(status(vec![codex_account("work", true)])));
        put_up(&mut model, &mut machine, add(Some("codex")));
        let late = starts(&mut model, &mut machine, "codex", "travel");
        model.give(Answer::SignInQuiet { id: late });
        let finishing = model.take(sign_in_over);
        assert_eq!(
            finishing,
            Job::SignInOver {
                id: late,
                enrol: true
            }
        );
        assert_eq!(
            model.send(Intent::CancelSignIn),
            [Job::StopSignIn { id: late }]
        );
        model.run(&mut machine);

        put_up(&mut model, &mut machine, add(Some("claude")));
        assert_eq!(
            model.send(sign_in("claude", "other")),
            [],
            "not while the one finishing holds the core's one sign-in at a time"
        );
        let next = running(&model).id;
        let reads = model.count(any_read);
        machine.answer = Ok(status(vec![codex_account("work", true), travel.clone()]));
        let done = if stopped_first {
            Err(refusal(
                "sign_in_gone",
                "The sign-in is no longer running.",
                vec![warning("interrupted_switch_undone", "A switch was undone.")],
            ))
        } else {
            enrolled_as(EnrolledAs::SignedIn, Vec::new())
        };
        model.give(Answer::SignInFinished {
            id: late,
            done: done.map_err(|refused| refused.error()),
        });
        model.run(&mut machine);
        assert_eq!(
            machine.signed_in,
            ["codex/travel", "claude/other"],
            "asked for once the one finishing is over"
        );

        let shown = model.shown();
        let still = shown
            .signing_in
            .as_ref()
            .expect("the sign-in started since");
        assert_eq!((still.id, still.name.as_str()), (next, "other"));
        assert_eq!(shown.sheet, Some(add(Some("claude"))), "{stopped_first}");
        assert!(shown.warnings.is_empty(), "{stopped_first}");
        assert_eq!((shown.failure, shown.sheet_failure), (None, None));
        if stopped_first {
            assert_eq!(
                model.count(any_read),
                reads,
                "nothing was enrolled, so nothing is read"
            );
        } else {
            assert_eq!(model.count(any_read), reads + 1);
            assert!(
                shown.status.expect("accounts").accounts.contains(&travel),
                "enrolled all the same"
            );
        }
    }
}

/// AppModelTests.swift's aReadThatStartedBeforeAChangeIsDroppedWhenItLands, for a sign-in, the
/// case `switching.rs` has with the others: a read that started before this app's sign-in
/// finished lands after it with who was signed in before, and is dropped.
#[test]
fn a_read_that_started_before_a_sign_in_is_dropped_when_it_lands() {
    a_read_that_started_before(|model, machine| {
        signs_in(model, machine, "codex", "travel", &[]);
        assert_eq!(machine.signed_in, ["codex/travel"]);
    });
}

/// `changing.rs`'s look landing after a change, for what a sign-in enrols: the look finds the
/// account index as the enrolment wrote it, and lands after the sign-in has finished and
/// before the read after it, which it dropped. The Codex half of the fixture's
/// `accounts_are_added_through_each_tools_sign_in` met it, cancelling nothing.
#[test]
fn a_look_landing_after_a_sign_in_enrols_leaves_the_read_after_it() {
    let after = vec![
        codex_account("personal", true),
        codex_account("spare", false),
        codex_account("travel", false),
    ];
    a_look_landing_after(after, |model, machine| {
        model.send(sign_in("codex", "travel"));
        model.run_but(machine, a_look_or_a_read);
        let id = running(model).id;
        says(model, id, CODEX_ADDRESS);
        model.give(Answer::SignInQuiet { id });
        model.run_but(machine, a_look_or_a_read);
        assert_eq!(machine.over, [(id, true)]);
        assert_eq!(model.shown().signing_in, None, "finished");
    });
}

/// RoutingTests.swift's aRunningSignInKeepsItsSheet. The menu stays usable while the window
/// has a sheet up, and its Add Account… or an item that names an account puts up a sheet.
/// Over a running sign-in that replaced the sheet that could finish or stop it, and left the
/// tool waiting on a browser with nothing on screen. The sign-in keeps its sheet, and the
/// window is still brought forward on the accounts pane.
#[test]
fn a_running_sign_in_keeps_its_sheet() {
    let rename = Sheet::Rename {
        provider: "claude".into(),
        label: "personal".into(),
    };
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(vec![claude("work", true, 0.0)])));
    model.refresh(&mut machine);
    put_up(&mut model, &mut machine, add(None));
    let id = starts(&mut model, &mut machine, "claude", "travel");
    says(&mut model, id, PROMPT);
    assert!(running(&model).wants_code);

    let requests = model.shown().window_request.serial;
    put_up(
        &mut model,
        &mut machine,
        Sheet::Name {
            provider: "codex".into(),
            email: "someone@example.com".into(),
        },
    );
    put_up(&mut model, &mut machine, rename.clone());
    let shown = model.shown();
    assert_eq!(shown.sheet, Some(add(None)));
    assert_eq!(
        shown.window_request,
        WindowRequest {
            serial: requests + 2,
            pane: Some(Pane::Accounts),
        }
    );

    model.send(Intent::CancelSignIn);
    model.run(&mut machine);
    ends(&mut model, &mut machine, id);
    put_up(&mut model, &mut machine, rename.clone());
    assert_eq!(model.shown().sheet, Some(rename), "once it is over");
}

/// RoutingTests.swift's anAccountIsAddedThroughTheSheetFromStartToFinish, apart from the tool
/// the sheet starts on, which comes with what the window says. The Swift drove its fixture;
/// this drives the model's own state, and `threaded.rs` adds one through the real core and a
/// stand-in for `claude`. The sheet over the window, the tool's sign-in with the code it asks
/// for typed back, and the new account parked beside the one in use once the sheet has closed
/// by itself.
#[test]
fn an_account_is_added_through_the_sheet_from_start_to_finish() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(vec![claude("work", true, 0.0)])));
    model.refresh(&mut machine);
    put_up(&mut model, &mut machine, add(None));
    assert_eq!(model.shown().window_request.serial, 1);

    let id = starts(&mut model, &mut machine, "claude", "travel");
    says(
        &mut model,
        id,
        "Opening browser to sign in\u{2026}\nIf the browser didn't open, visit: \
         https://claude.ai/oauth/authorize?fixture=1\n",
    );
    says(&mut model, id, PROMPT);
    let shown = running(&model);
    assert!(shown.wants_code);
    assert_eq!(
        shown.url.as_deref(),
        Some("https://claude.ai/oauth/authorize?fixture=1")
    );
    assert_eq!(model.shown().sheet, Some(add(None)));

    model.send(paste("fixture-code"));
    machine.answer = Ok(status(vec![
        claude("work", true, 0.0),
        claude("travel", false, 0.0),
    ]));
    model.run(&mut machine);
    ends(&mut model, &mut machine, id);

    assert_eq!(machine.pasted, [(id, "fixture-code".to_owned())]);
    let shown = model.shown();
    assert_eq!(shown.signing_in, None);
    assert_eq!(shown.sheet, None);
    let accounts = shown.status.expect("accounts").accounts;
    let travel = accounts
        .iter()
        .find(|account| account.qualified.as_deref() == Some("claude/travel"))
        .expect("travel enrolled");
    assert!(!travel.signed_in);
    assert!(travel.switchable);
    assert!(shown.warnings.is_empty());
}

/// RoutingTests.swift's signingInAgainToTheAccountInUseSaysItHasANewLogin, on the model's own
/// state where the Swift ran its fixture. Signing in again to the account in use, from its
/// row, keeps it the account in use and says its new login is the one in use now, as a
/// sign-in through the core does.
#[test]
fn signing_in_again_to_the_account_in_use_says_it_has_a_new_login() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(vec![claude("work", true, 0.0)])));
    model.refresh(&mut machine);
    let again = Sheet::SignInAgain {
        provider: "claude".into(),
        label: "work".into(),
    };
    put_up(&mut model, &mut machine, again);
    machine.enrolling = enrolled_as(EnrolledAs::InUse { again: true }, Vec::new());
    let id = starts(&mut model, &mut machine, "claude", "work");
    says(&mut model, id, PROMPT);
    model.send(paste("fixture-code"));
    model.run(&mut machine);
    ends(&mut model, &mut machine, id);

    let shown = model.shown();
    assert_eq!(shown.sheet, None);
    let last = shown.last_switches.first().expect("what the sign-in said");
    assert_eq!(
        (last.provider.as_str(), last.to.as_str()),
        ("claude", "work")
    );
    assert_eq!(
        last.said.as_deref(),
        Some("Signed in to work again. Its new login is the one in use now.")
    );
    let work = &shown.status.expect("accounts").accounts[0];
    assert!(work.signed_in);
    assert!(!work.switchable);
}

/// AppModelTests.swift's whatIsInstalledIsAskedAgainWhenTheFormForAnotherAccountOpens, apart
/// from what the sheet offers, which comes with what the window says. Opening the form for
/// another account asks again what is installed: the first answer may have come while the
/// person's login shell was too slow to say, and the core asks it once more when that was so.
/// Naming the account in use asks nothing, nor does a read once something is found.
#[test]
fn what_is_installed_is_asked_again_when_the_form_for_another_account_opens() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    machine.found = vec![claude_code()];
    model.refresh(&mut machine);
    assert_eq!(model.shown().installed, Some(vec![claude_code()]));

    machine.found = vec![claude_code(), codex()];
    assert_eq!(model.send(present(add(None))), [Job::AskInstalled]);
    model.run(&mut machine);
    assert_eq!(model.shown().installed, Some(vec![claude_code(), codex()]));
    assert_eq!(model.count(installed_ask), 2);
    assert_eq!(
        model.send(present(add(Some("codex")))),
        [],
        "not for one put up in its place"
    );

    model.send(Intent::CloseSheet);
    put_up(
        &mut model,
        &mut machine,
        Sheet::Name {
            provider: "claude".into(),
            email: "a@example.com".into(),
        },
    );
    model.refresh(&mut machine);
    assert_eq!(
        model.count(installed_ask),
        2,
        "not for naming the account in use, nor for a read"
    );
}

/// RoutingTests.swift's aSheetIsPutUpOverTheWindow, the sheet: a sheet is over the main
/// window, so putting one up opens the window too, on the accounts the sheet is about. The
/// rest of it, opening the window by itself, comes with what the window says.
#[test]
fn a_sheet_is_put_up_over_the_window() {
    let rename = Sheet::Rename {
        provider: "claude".into(),
        label: "personal".into(),
    };
    let mut model = Hand::new();
    assert_eq!(model.shown().window_request.serial, 0);
    model.send(present(rename.clone()));
    let shown = model.shown();
    assert_eq!(shown.sheet, Some(rename));
    assert_eq!(
        shown.window_request,
        WindowRequest {
            serial: 1,
            pane: Some(Pane::Accounts),
        }
    );
}

// What the Swift model did not do.

/// After Claude Code refuses a code typed back, the person may paste one again: the owner's
/// decision. Claude Code 2.1.289 refuses a line that is not the whole code with `Invalid
/// code.` and goes on reading in the same sign-in, as the register's
/// `sign_in_takes_another_code` holds, so the field is offered again for the same sign-in,
/// saying the code was refused, and the next code goes to the same tool. The Swift model took
/// any code typed back as the one, and never asked again, leaving the person a sign-in that
/// could only be cancelled.
#[test]
fn a_code_claude_code_refused_may_be_pasted_again() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    let id = starts(&mut model, &mut machine, "claude", "travel");
    says(&mut model, id, PROMPT);
    assert_eq!(
        model.send(paste("half")),
        [Job::PasteCode {
            id,
            code: "half".into()
        }]
    );
    let shown = running(&model);
    assert!(!shown.wants_code && !shown.code_refused);

    says(&mut model, id, REFUSED);
    let shown = running(&model);
    assert!(shown.wants_code, "asked again");
    assert!(shown.code_refused);

    let typed = model.send(paste("the-code#the-state"));
    assert_eq!(
        typed,
        [Job::PasteCode {
            id,
            code: "the-code#the-state".into()
        }],
        "to the same sign-in"
    );
    let shown = running(&model);
    assert!(
        !shown.wants_code && !shown.code_refused,
        "nothing said of it yet"
    );
    model.run(&mut machine);
    ends(&mut model, &mut machine, id);
    assert_eq!(
        machine.signed_in,
        ["claude/travel"],
        "one sign-in throughout"
    );
    assert_eq!(machine.over, [(id, true)]);
    assert_eq!(model.shown().signing_in, None);
}

/// What the tool said before a code was typed back says nothing of that code: a refusal of
/// the one before is not a refusal of the next.
#[test]
fn only_what_the_tool_says_after_a_code_refuses_it() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    let id = starts(&mut model, &mut machine, "claude", "travel");
    says(&mut model, id, PROMPT);
    model.send(paste("one"));
    says(&mut model, id, "Invalid ");
    assert!(
        !running(&model).wants_code,
        "not until the whole of it has come"
    );
    says(&mut model, id, REFUSED.trim_start_matches("Invalid "));
    model.send(paste("two"));
    says(&mut model, id, "Login successful.\n");
    let shown = running(&model);
    assert!(!shown.wants_code && !shown.code_refused);
}

/// A name and a code are taken without the white space around them, as the Swift sheets took
/// it away with Foundation's `whitespacesAndNewlines`, which holds U+200B ZERO WIDTH SPACE as
/// well as Unicode's White_Space, as measured on macOS 27.0; one with nothing else in it
/// starts or types nothing.
#[test]
fn a_name_and_a_code_are_trimmed_as_the_sheet_trimmed_them() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    assert_eq!(model.send(sign_in("claude", " \u{200b}\n")), []);
    assert_eq!(
        model.send(sign_in("claude", "\u{3000}travel\u{200b} ")),
        [Job::SignIn {
            id: 1,
            qualified: "claude/travel".into()
        }]
    );
    assert_eq!(running(&model).name, "travel");
    model.run(&mut machine);
    says(&mut model, 1, PROMPT);
    assert_eq!(model.send(paste(" \t\u{200b}")), []);
    assert_eq!(
        model.send(paste("\u{200b}abc#def\n")),
        [Job::PasteCode {
            id: 1,
            code: "abc#def".into()
        }]
    );
}

/// A code is typed only to a tool that has started and asks for one: before then there is
/// nothing to type to, and a code typed again before the tool has said anything of the last
/// one is the same code twice, which the sheet's field, gone once it is typed, never sent.
#[test]
fn a_code_is_typed_only_where_the_tool_asks_for_one() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    model.send(sign_in("claude", "travel"));
    assert_eq!(model.send(paste("early")), [], "not started");
    model.run(&mut machine);
    assert_eq!(model.send(paste("early")), [], "not asked for yet");
    says(&mut model, 1, PROMPT);
    assert_eq!(model.send(paste("abc")).len(), 1);
    assert_eq!(model.send(paste("abc")), [], "typed once");
    model.run(&mut machine);
    assert_eq!(machine.pasted, [(1, "abc".to_owned())]);
}

/// A sign-in that fails once the sheet it was started from has gone, or one started where no
/// sheet was up, says so in the window, which is opened for it: something asked for that did
/// not happen is said somewhere. The Swift model handed it back to the sheet, which had gone,
/// so it was said nowhere.
#[test]
fn a_sign_in_that_fails_once_its_sheet_has_gone_says_so_in_the_window() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    put_up(&mut model, &mut machine, add(Some("codex")));
    let id = starts(&mut model, &mut machine, "codex", "work");
    says(&mut model, id, CODEX_ADDRESS);
    model.send(Intent::CloseSheet);
    machine.enrolling = Err(refusal(
        "sign_in_incomplete",
        "The sign-in did not finish.",
        Vec::new(),
    ));
    let requests = model.shown().window_request.serial;
    ends(&mut model, &mut machine, id);

    let shown = model.shown();
    let failure = shown.failure.expect("said in the window");
    assert_eq!(failure.title, "Couldn’t sign in to work");
    assert_eq!(failure.message, "The sign-in did not finish.");
    assert_eq!(
        shown.window_request,
        WindowRequest {
            serial: requests + 1,
            pane: None,
        }
    );
    assert_eq!((shown.sheet, shown.sheet_failure), (None, None));

    machine.starting = Err(refusal("claude_program_missing", "no claude", Vec::new()));
    starts(&mut model, &mut machine, "claude", "other");
    let failure = model.shown().failure.expect("said in the window");
    assert_eq!(
        (failure.id, failure.title.as_str()),
        (2, "Couldn’t sign in to other")
    );
}

/// A sign-in whose tool stopped before it finished says so in the sheet it was started
/// from, with the name still in it, and the panel says what it warned of. Nothing was
/// enrolled, so nothing is read.
#[test]
fn a_sign_in_that_does_not_finish_says_so_in_its_sheet() {
    let undone = warning("interrupted_switch_undone", "A switch was undone.");
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    machine.enrolling = Err(refusal(
        "sign_in_incomplete",
        "The sign-in did not finish.",
        vec![undone.clone()],
    ));
    put_up(&mut model, &mut machine, add(Some("codex")));
    signs_in(&mut model, &mut machine, "codex", "work", &[CODEX_ADDRESS]);

    let shown = model.shown();
    assert_eq!(shown.signing_in, None);
    assert_eq!(shown.sheet, Some(add(Some("codex"))));
    let failure = shown.sheet_failure.expect("said in the sheet");
    assert_eq!(failure.message, "The sign-in did not finish.");
    assert_eq!(shown.warnings, [undone]);
    assert_eq!(shown.failure, None);
    assert_eq!(model.count(any_read), 0);
}

/// What went wrong in a sheet goes with it: closed, replaced by another, or signed in from
/// again. It is numbered with what is said in the window, so an app tells each failure from
/// one it has shown.
#[test]
fn what_went_wrong_in_a_sheet_goes_with_it() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    machine.switched = Err(refusal("nothing_parked", "nothing parked", Vec::new()));
    model.send(Intent::SwitchTo {
        qualified: "claude/spare".into(),
    });
    model.run(&mut machine);
    assert_eq!(model.shown().failure.map(|f| f.id), Some(1));

    machine.starting = Err(refusal("claude_program_missing", "no claude", Vec::new()));
    put_up(&mut model, &mut machine, add(None));
    starts(&mut model, &mut machine, "claude", "work");
    assert_eq!(model.shown().sheet_failure.map(|f| f.id), Some(2));
    put_up(&mut model, &mut machine, add(None));
    assert!(model.shown().sheet_failure.is_some(), "the same sheet");

    starts(&mut model, &mut machine, "claude", "work");
    assert_eq!(model.shown().sheet_failure.map(|f| f.id), Some(3));
    machine.starting = Ok(());
    model.send(sign_in("claude", "work"));
    assert_eq!(model.shown().sheet_failure, None, "signed in from again");
    model.run(&mut machine);
    let id = running(&model).id;
    model.send(Intent::CancelSignIn);
    model.run(&mut machine);
    ends(&mut model, &mut machine, id);

    machine.starting = Err(refusal("claude_program_missing", "no claude", Vec::new()));
    starts(&mut model, &mut machine, "claude", "work");
    put_up(&mut model, &mut machine, add(Some("codex")));
    assert_eq!(model.shown().sheet_failure, None, "another sheet");
    starts(&mut model, &mut machine, "codex", "work");
    model.send(Intent::CloseSheet);
    let shown = model.shown();
    assert_eq!((shown.sheet, shown.sheet_failure), (None, None));
}

/// A sign-in whose start or finish stopped with a panic is over, said as a failure of its
/// own in its sheet, since what its tool did is not known.
#[test]
fn a_sign_in_lost_to_a_panic_is_said_and_over() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    put_up(&mut model, &mut machine, add(None));
    model.send(sign_in("claude", "work"));
    let start = model.next();
    assert_eq!(model.give(Answer::Lost(start)), []);
    let shown = model.shown();
    assert_eq!(shown.signing_in, None);
    let failure = shown.sheet_failure.expect("said in the sheet");
    assert_eq!(failure.title, "Couldn’t sign in to work");
    assert_eq!(failure.code, None);

    let id = starts(&mut model, &mut machine, "claude", "work");
    model.give(Answer::SignInQuiet { id });
    let over = model.take(sign_in_over);
    assert_eq!(model.give(Answer::Lost(over)), []);
    let shown = model.shown();
    assert_eq!(shown.signing_in, None);
    assert_eq!(shown.sheet_failure.map(|f| f.id), Some(2));
}

/// Every sign-in's thread is told once whether to enrol, once its tool has stopped saying
/// anything, and only the sign-in still under way is: one cancelled, or that has already
/// failed to start, is never enrolled.
#[test]
fn each_sign_in_is_told_once_whether_to_enrol() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    let first = starts(&mut model, &mut machine, "claude", "one");
    ends(&mut model, &mut machine, first);
    let second = starts(&mut model, &mut machine, "claude", "two");
    model.send(Intent::CancelSignIn);
    model.run(&mut machine);
    ends(&mut model, &mut machine, second);
    assert_eq!(machine.over, [(first, true), (second, false)]);
    assert_eq!(
        model.give(Answer::SignInSaid {
            id: second,
            text: PROMPT.into()
        }),
        [],
        "a sign-in no longer under way says nothing more"
    );
}
