//! Enrolling the login signed in now, renaming and forgetting, as the Swift model did them:
//! its tests on those, each under its own name in snake case, driven through `State::apply`
//! by hand. Each closes only its own sheet, says what went wrong where it was asked, and reads
//! the accounts afterwards.

use super::state::{Answer, Job};
use super::switching::{a_read_that_started_before, switch};
use super::testing::{
    Hand, Machine, claude, codex_account, enrolled_as, refusal, status, switched, warning,
};
use super::{Intent, Sheet};
use crate::EnrolledAs;
use crate::present::testing::account;

fn enrol(provider: &str, name: &str) -> Intent {
    Intent::Enrol {
        provider: provider.into(),
        name: name.into(),
    }
}

fn rename(provider: &str, label: &str, to: &str) -> Intent {
    Intent::Rename {
        provider: provider.into(),
        label: label.into(),
        to: to.into(),
    }
}

fn forget(qualified: &str) -> Intent {
    Intent::Forget {
        qualified: qualified.into(),
    }
}

fn naming(provider: &str, email: &str) -> Sheet {
    Sheet::Name {
        provider: provider.into(),
        email: email.into(),
    }
}

fn renaming(provider: &str, label: &str) -> Sheet {
    Sheet::Rename {
        provider: provider.into(),
        label: label.into(),
    }
}

/// Puts `sheet` up and answers what that asks.
fn put_up(model: &mut Hand, machine: &mut Machine, sheet: Sheet) {
    model.send(Intent::PresentSheet { sheet });
    model.run(machine);
}

fn tos(model: &Hand) -> Vec<String> {
    model
        .shown()
        .last_switches
        .into_iter()
        .map(|last| last.to)
        .collect()
}

/// The account signed in but not enrolled is the one the app can record by itself: no
/// browser, no terminal. The sheet closes once the name is taken, and stays open with the
/// name in it when it is refused, so it can be corrected rather than typed again.
///
/// AppModelTests.swift's onlyAnUnenrolledSignedInAccountCanBeNamedHere.
#[test]
fn only_an_unenrolled_signed_in_account_can_be_named_here() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(vec![
        account(None).signed_in().uuid("a").build(),
    ])));
    model.refresh(&mut machine);
    assert!(matches!(
        model.shown().footing,
        crate::Footing::Unnamed { .. }
    ));

    let name = naming("claude", "a@example.com");
    put_up(&mut model, &mut machine, name.clone());
    machine.enrolling_current = Err(refusal(
        "label_taken",
        "There is already an account called work.",
        vec![warning("auth_overridden", "ANTHROPIC_API_KEY is set")],
    ));
    model.send(enrol("claude", "work"));
    model.run(&mut machine);
    let shown = model.shown();
    let refused = shown.sheet_failure.expect("said in the sheet");
    assert_eq!(refused.title, "Couldn’t name this account");
    assert_eq!(refused.message, "There is already an account called work.");
    assert_eq!(shown.sheet, Some(name), "the sheet stays open to say why");
    assert!(
        shown.warnings.is_empty(),
        "said in the sheet, not in the panel"
    );
    assert_eq!(shown.failure, None);

    machine.enrolling_current = enrolled_as(EnrolledAs::Current, Vec::new());
    model.send(enrol("claude", "work"));
    model.run(&mut machine);
    assert_eq!(machine.enrolled, ["claude/work", "claude/work"]);
    assert_eq!(
        model.shown().sheet,
        None,
        "the sheet closes once it has been used"
    );
}

/// Naming a Codex login enrols it as Codex's. A bare name means Claude Code to the core, and
/// would have enrolled nothing or the wrong tool's login.
///
/// AppModelTests.swift's aCodexLoginIsNamedAsCodexs.
#[test]
fn a_codex_login_is_named_as_codexs() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(vec![
        claude("work", true, 0.0),
        account(None).of("codex").signed_in().uuid("c").build(),
    ])));
    model.refresh(&mut machine);
    assert_eq!(
        model.shown().footing,
        crate::Footing::Unnamed {
            provider: "codex".into(),
            email: "c@example.com".into()
        }
    );
    model.send(enrol("codex", "job"));
    model.run(&mut machine);
    assert_eq!(machine.enrolled, ["codex/job"]);
}

/// Forgetting is destructive, so what the model does with a refusal matters. The refusal is
/// said to whoever asked, in the core's own words, and one that went through says nothing.
///
/// AppModelTests.swift's aRefusedForgetIsReported.
#[test]
fn a_refused_forget_is_reported() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    model.send(forget("claude/alpha"));
    model.run(&mut machine);
    assert_eq!(model.shown().failure, None);

    machine.forgetting = Err(refusal(
        "cannot_forget_active_account",
        "beta is the account in use, so it cannot be forgotten.",
        vec![warning("auth_overridden", "ANTHROPIC_API_KEY is set")],
    ));
    model.send(forget("claude/beta"));
    model.run(&mut machine);
    assert_eq!(machine.forgot, ["claude/alpha", "claude/beta"]);
    let shown = model.shown();
    let refused = shown.failure.expect("said");
    assert_eq!(refused.title, "Couldn’t forget beta");
    assert_eq!(
        refused.message,
        "beta is the account in use, so it cannot be forgotten."
    );
    assert_eq!(
        refused.code.as_deref(),
        Some("cannot_forget_active_account")
    );
    let codes: Vec<&str> = refused.warnings.iter().map(|w| w.code.as_str()).collect();
    assert_eq!(codes, ["auth_overridden"]);
    assert_eq!(shown.read_failure, None, "a refusal is not a failed read");
    assert!(
        shown.warnings.is_empty(),
        "its warnings are said with it, not in the panel"
    );
}

/// Two tools can each have a `work`. Every call names the one meant, with its tool, and the
/// rows are told apart by their id rather than by a label they share.
///
/// AppModelTests.swift's twoToolsWorkAccountsAreSwitchedAndForgottenByTheirOwnName, the
/// forgetting and the row's name: the switch is switching.rs's.
#[test]
fn two_tools_work_accounts_are_forgotten_by_their_own_name() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(vec![
        account(Some("work")).signed_in().uuid("same").build(),
        account(Some("personal")).build(),
        account(Some("work")).of("codex").uuid("same").build(),
        account(Some("spare")).of("codex").signed_in().build(),
    ])));
    model.refresh(&mut machine);
    let rows: Vec<crate::AccountItem> = model
        .shown()
        .sections
        .into_iter()
        .flat_map(|section| section.accounts)
        .collect();
    let ids: std::collections::BTreeSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids.len(), rows.len(), "one uuid, two tools, two rows");
    let codex_work = rows
        .iter()
        .find(|row| row.provider == "codex" && row.title == "work")
        .expect("Codex's work");
    let claude_work = rows
        .iter()
        .find(|row| row.provider == "claude" && row.title == "work")
        .expect("Claude Code's work");
    assert_ne!(codex_work.id, claude_work.id);

    for row in [codex_work, claude_work] {
        model.send(forget(row.qualified.as_deref().expect("enrolled")));
        model.run(&mut machine);
    }
    assert_eq!(machine.forgot, ["codex/work", "claude/work"]);
    assert_eq!(codex_work.spoken_name, "work (Codex)");
}

/// Naming or renaming closes its own sheet once it is done, and only that. The name is saved
/// even when somebody has since put up another sheet, and closing that one threw away
/// whatever was in it.
///
/// AppModelTests.swift's namingAndRenamingCloseOnlyTheirOwnSheet.
#[test]
fn naming_and_renaming_close_only_their_own_sheet() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(vec![
        account(None).signed_in().uuid("a").build(),
        account(Some("personal")).build(),
    ])));
    model.refresh(&mut machine);

    let others = [
        Sheet::Add { provider: None },
        naming("codex", "c@example.com"),
        renaming("claude", "other"),
    ];
    for other in &others {
        put_up(&mut model, &mut machine, other.clone());
        model.send(enrol("claude", "work"));
        model.run(&mut machine);
        let shown = model.shown();
        assert_eq!(shown.sheet_failure, None);
        assert_eq!(shown.sheet.as_ref(), Some(other));
    }
    put_up(&mut model, &mut machine, naming("claude", "a@example.com"));
    model.send(enrol("claude", "work"));
    model.run(&mut machine);
    assert_eq!(model.shown().sheet, None);

    for other in others.iter().chain([&renaming("codex", "personal")]) {
        put_up(&mut model, &mut machine, other.clone());
        model.send(rename("claude", "personal", "home"));
        model.run(&mut machine);
        assert_eq!(model.shown().sheet.as_ref(), Some(other));
    }
    put_up(&mut model, &mut machine, renaming("claude", "personal"));
    model.send(rename("claude", "personal", "home"));
    model.run(&mut machine);
    assert_eq!(model.shown().sheet, None);
    assert_eq!(
        machine.renamed.last(),
        Some(&("claude/personal".to_owned(), "home".to_owned()))
    );
}

/// AppModelTests.swift's aReadThatStartedBeforeAChangeIsDroppedWhenItLands, for an
/// enrolment.
#[test]
fn a_read_that_started_before_naming_an_account_is_dropped_when_it_lands() {
    a_read_that_started_before(|model, machine| {
        model.send(enrol("codex", "job"));
        model.run(machine);
    });
}

/// The same for a rename.
#[test]
fn a_read_that_started_before_a_rename_is_dropped_when_it_lands() {
    a_read_that_started_before(|model, machine| {
        model.send(rename("codex", "spare", "home"));
        model.run(machine);
    });
}

/// The same for forgetting.
#[test]
fn a_read_that_started_before_forgetting_is_dropped_when_it_lands() {
    a_read_that_started_before(|model, machine| {
        model.send(forget("codex/spare"));
        model.run(machine);
    });
}

/// A rename changes what an account is called and nothing else about it. What its tool's
/// last switch said is still true of it, so it is said under the new name, and one tool's
/// rename says nothing about another tool's accounts. Keyed by the old name, the read after a
/// rename took the switch for undone.
///
/// AppModelTests.swift's aRenameCarriesWhatWasSaidAboutTheAccount, what the last switches
/// said: advice and what was told come with telling.
#[test]
fn a_rename_carries_what_was_said_about_the_account() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(vec![
        claude("work", true, 100.0),
        claude("personal", false, 10.0),
        codex_account("work", true),
        codex_account("personal", false),
    ])));
    machine.switched = switched("claude", "personal", "work", Vec::new());
    switch(&mut model, &mut machine, "claude/work");
    machine.switched = switched("codex", "codex/personal", "codex/work", Vec::new());
    switch(&mut model, &mut machine, "codex/work");
    assert_eq!(tos(&model), ["work", "codex/work"]);
    let from = |model: &Hand| {
        model.shown().last_switches[1]
            .restart
            .as_ref()
            .map(|restart| restart.from.clone())
    };
    assert_eq!(from(&model).as_deref(), Some("personal"));

    // Each read after a rename fails here, so what is said is what the rename carried.
    machine.answer = Err(refusal("unreachable", "could not be reached", Vec::new()));

    model.send(rename("claude", "personal", "spare"));
    model.run(&mut machine);
    assert_eq!(
        from(&model).as_deref(),
        Some("personal"),
        "Codex's is another account"
    );
    model.send(rename("codex", "personal", "home"));
    model.run(&mut machine);
    assert_eq!(from(&model).as_deref(), Some("home"));
    model.send(rename("codex", "work", "job"));
    model.run(&mut machine);
    assert_eq!(tos(&model), ["work", "codex/job"]);
    model.send(rename("claude", "work", "office"));
    model.run(&mut machine);
    assert_eq!(tos(&model), ["office", "codex/job"]);

    machine.answer = Ok(status(vec![
        claude("office", true, 100.0),
        claude("spare", false, 10.0),
        codex_account("job", true),
        codex_account("home", false),
    ]));
    model.refresh(&mut machine);
    assert_eq!(tos(&model), ["office", "codex/job"], "still in use");
    let notice = model
        .shown()
        .notices
        .into_iter()
        .find(|notice| notice.id == "switch/codex")
        .expect("Codex's switch");
    assert!(
        notice
            .lines
            .iter()
            .any(|line| line.contains("keeps using home")),
        "{notice:?}"
    );
}

// What the Swift model did not do.

/// A name is saved without the white space around it, as the sheet's Save offers it, and a
/// name with nothing in it, or a rename to the name an account has, saves nothing.
#[test]
fn a_name_is_saved_as_its_sheet_offers_it() {
    let mut model = Hand::new();
    assert_eq!(model.send(enrol("claude", " \n")), []);
    assert_eq!(model.send(rename("claude", "work", " work ")), []);
    assert_eq!(
        model.send(enrol("codex", " job\t")),
        [Job::Enrol {
            provider: "codex".into(),
            name: "job".into(),
            from: None,
        }]
    );
    assert_eq!(
        model.send(rename("claude", "work", " office ")),
        [Job::Rename {
            provider: "claude".into(),
            label: "work".into(),
            to: "office".into(),
            from: None,
        }]
    );
}

/// A name being saved cannot be withdrawn, so its sheet says it is saving, which holds its
/// buttons back, and a second Save from it saves nothing until the first has answered, as
/// NameSheets.swift's `saving` held them back.
#[test]
fn a_sheet_saving_a_name_holds_back_until_it_answers() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    put_up(&mut model, &mut machine, naming("claude", "a@example.com"));
    let jobs = model.send(enrol("claude", "work"));
    assert_eq!(jobs.len(), 1);
    assert!(model.shown().sheet_text.expect("the sheet").saving);
    assert_eq!(model.send(enrol("claude", "work")), [], "once");

    machine.enrolling_current = Err(refusal("label_taken", "Taken.", Vec::new()));
    let job = model.next();
    let answer = machine.answer(job);
    model.give(answer);
    let shown = model.shown();
    assert!(!shown.sheet_text.expect("the sheet").saving);
    assert!(shown.sheet_failure.is_some());
    assert_eq!(model.send(enrol("claude", "work")).len(), 1, "and again");
}

/// What a sheet could not save once it has gone, closed or replaced while the save ran, is
/// said in the window: something asked for that did not happen is said somewhere. The Swift
/// handed it back to the sheet that asked, which had gone, so it was said nowhere.
///
/// Reaches people in PR 10.
#[test]
fn a_name_that_cannot_be_saved_once_its_sheet_has_gone_is_said_in_the_window() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    machine.enrolling_current = Err(refusal("label_taken", "Taken.", Vec::new()));
    machine.renaming = Err(refusal("label_taken", "Taken too.", Vec::new()));

    put_up(&mut model, &mut machine, naming("claude", "a@example.com"));
    model.send(enrol("claude", "work"));
    model.send(Intent::CloseSheet);
    let requests = model.shown().window_request.serial;
    model.run(&mut machine);
    let shown = model.shown();
    assert_eq!(shown.sheet_failure, None);
    let failure = shown.failure.expect("said in the window");
    assert_eq!(failure.title, "Couldn’t name this account");
    assert_eq!(shown.window_request.serial, requests + 1);

    put_up(&mut model, &mut machine, renaming("claude", "work"));
    model.send(rename("claude", "work", "office"));
    put_up(&mut model, &mut machine, Sheet::Add { provider: None });
    let shown = model.shown();
    assert_eq!(shown.sheet_failure, None, "not in another sheet");
    assert_eq!(
        shown.failure.map(|failure| failure.title),
        Some("Couldn’t rename work".into())
    );
}

/// A save whose job came to nothing is said as a failure of its own, where it was asked.
#[test]
fn a_save_that_came_to_nothing_says_so() {
    let mut model = Hand::new();
    let mut machine = Machine::reading(Ok(status(Vec::new())));
    put_up(&mut model, &mut machine, renaming("claude", "work"));
    let jobs = model.send(rename("claude", "work", "office"));
    let job = jobs.into_iter().next().expect("the rename");
    model.give(Answer::Lost(job));
    let shown = model.shown();
    let failure = shown.sheet_failure.expect("said in the sheet");
    assert_eq!(failure.title, "Couldn’t rename work");
    assert!(failure.message.starts_with("Pitboard stopped before"));
    assert!(!shown.sheet_text.expect("the sheet").saving);
}
