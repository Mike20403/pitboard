//! Keeping parked logins alive without anybody running a command.
//!
//! A refresh token has a finite life, and the only two things that renew one are somebody
//! typing `pitboard` and the menu bar app's poll. So the tool is safe for a macOS user who
//! installed the app and leaves it running, and quietly unsafe for everyone else: the
//! whole of Linux, and any macOS user on the command line alone. Go away for the refresh
//! window, come back, and every parked login is dead and each account needs a browser
//! sign-in, which is precisely the cost Pitboard exists to spare people.
//!
//! This is opt-in, and stays opt-in. A background process that talks to Anthropic on a
//! schedule is the shape most likely to be read as automation, so it is something a person
//! turns on knowing what it is, and what it does is written where they can read it: it
//! renews the owner's own parked logins and nothing else. It never switches, never asks for
//! usage, and makes no request other than the token exchange.
//!
//! What it installs is the system's own scheduler, never a homemade daemon: a LaunchAgent
//! on macOS, which runs inside the login session so the keychain is unlocked, and a systemd
//! user timer on Linux, which is that system's answer. How each is asked is
//! [`crate::host::Scheduler`]'s; what is decided here is which Pitboard it runs, how often,
//! and whether it is this home's to change. Where there is no scheduler, it says so rather
//! than inventing one.

use crate::context::Context;
use crate::error::{Error, Result};
use crate::host::Scheduler;
use crate::service::Permit;
use std::path::{Path, PathBuf};

/// Once a day. A refresh token's life is measured in weeks and Pitboard starts renewing
/// three days out, so a daily check has three chances to catch each one, and a machine
/// that was asleep for one of them still has two.
pub(crate) const EVERY_SECONDS: u32 = 86_400;

/// Whether the schedule is installed, and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installed {
    /// Installed, at this path.
    Yes {
        path: PathBuf,
        every_seconds: u32,
    },
    No,
    /// This machine has no scheduler Pitboard knows how to ask.
    Unsupported,
}

fn scheduler(ctx: &Context) -> Option<&dyn Scheduler> {
    ctx.host().scheduler()
}

/// Where the schedule lives on this machine, or `None` where there is no scheduler.
pub fn path(ctx: &Context) -> Option<PathBuf> {
    scheduler(ctx).map(|s| s.location(ctx))
}

pub fn status(ctx: &Context) -> Installed {
    match scheduler(ctx) {
        None => Installed::Unsupported,
        Some(s) if s.installed(ctx) => Installed::Yes {
            path: s.location(ctx),
            every_seconds: EVERY_SECONDS,
        },
        Some(_) => Installed::No,
    }
}

/// Whether the schedule is this context's to look after.
///
/// The scheduler starts `renew` without `PITBOARD_HOME`, so the schedule always renews the
/// default home, `~/.pitboard` ([`crate::host::default_pitboard_home`]). A Pitboard pointed
/// at another home has none of its own: the one there is belongs to the default home.
pub(crate) fn serves(ctx: &Context) -> bool {
    crate::home::dir(ctx) == crate::host::default_pitboard_home(ctx.home())
}

/// The Pitboard the schedule should run: the one the context names, or this one, by the
/// path it was started by.
fn program(ctx: &Context) -> Result<PathBuf> {
    if let Some(program) = ctx.schedule_program() {
        return lasting(program).map(Path::to_path_buf);
    }
    crate::host::current_program().map_err(|source| Error::HomeUnwritable {
        path: PathBuf::from("the running Pitboard"),
        source,
    })
}

/// `program`, where it will still be there when the scheduler runs it. A scheduler starts a
/// program that is gone without telling anyone, every day, so a path that leads nowhere is
/// refused rather than written down.
///
/// A path inside the copy macOS makes of an app opened where it was downloaded leads
/// somewhere while that app runs and nowhere once it quits, so it is refused as well.
fn lasting(program: &Path) -> Result<&Path> {
    if in_a_temporary_copy(program) {
        return Err(Error::ScheduleProgramTemporary {
            path: program.to_path_buf(),
        });
    }
    if !program.is_file() {
        return Err(Error::ScheduleProgramMissing {
            path: program.to_path_buf(),
        });
    }
    Ok(program)
}

/// Whether `program` is inside the copy macOS makes of an app opened where it was
/// downloaded, which is there while the app runs and gone once it quits: a path through a
/// directory named `AppTranslocation`. Neither a schedule nor a link can rely on one, so an
/// app asks this of the command line inside it, by the rule a schedule is refused by.
pub fn in_a_temporary_copy(program: &Path) -> bool {
    program
        .components()
        .any(|part| part.as_os_str() == "AppTranslocation")
}

/// The Pitboard the installed schedule runs, read back from what `install` wrote. `None`
/// where nothing is installed, and where what is there does not name one the way `install`
/// writes it.
pub fn installed_program(ctx: &Context) -> Option<PathBuf> {
    scheduler(ctx)?.program(ctx)
}

/// Install it, and ask the system to start it. Returns where it went.
pub fn install(ctx: &Context, permit: Permit) -> Result<PathBuf> {
    let path = put(ctx, permit, &program(ctx)?)?;
    crate::audit::record(ctx, permit, "schedule", "install", "ok");
    Ok(path)
}

/// Point a schedule that runs an app's own program at the command line the context names.
/// `true` when it did.
///
/// An app up to 0.3.0 scheduled itself, and that app renews nothing when started with
/// `renew`: launchd started a second menu bar app every day instead. The app calls this
/// when it starts, so nothing changes unless the schedule is such a one and belongs to this
/// home, and the context names a command line that will still be there when the scheduler
/// runs it.
///
/// Nothing changes from inside the schedule's own run either: launchd stops a job's process
/// when it unloads the job, which a repair does before loading it again, so nothing would be
/// left to load it back.
pub fn repair(ctx: &Context, permit: Permit) -> Result<bool> {
    let Some(scheduler) = scheduler(ctx) else {
        return Ok(false);
    };
    if scheduler.started_this_process(ctx.scheduled_job()) {
        return Ok(false);
    }
    let Some(named) = ctx.schedule_program() else {
        return Ok(false);
    };
    if !serves(ctx)
        || lasting(named).is_err()
        || !installed_program(ctx).is_some_and(|program| an_apps_own_program(&program))
    {
        return Ok(false);
    }
    let repaired = put(ctx, permit, named);
    crate::audit::record(
        ctx,
        permit,
        "schedule",
        "repair",
        match &repaired {
            Ok(_) => "ok",
            Err(e) => e.code(),
        },
    );
    repaired.map(|_| true)
}

/// Whether `program` is the one an app bundle starts, `Contents/MacOS/<name>`, where no
/// command line is ever kept.
pub(crate) fn an_apps_own_program(program: &Path) -> bool {
    let mut dirs = program
        .ancestors()
        .skip(1)
        .map(|dir| dir.file_name().and_then(|n| n.to_str()));
    dirs.next() == Some(Some("MacOS")) && dirs.next() == Some(Some("Contents"))
}

/// Schedule `program`, and ask the system to start it.
fn put(ctx: &Context, permit: Permit, program: &Path) -> Result<PathBuf> {
    let Some(scheduler) = scheduler(ctx) else {
        return Err(Error::ScheduleUnsupported);
    };
    scheduler.put(ctx, permit, program)?;
    Ok(scheduler.location(ctx))
}

/// Take it away. `false` when there was nothing installed.
pub fn uninstall(ctx: &Context, permit: Permit) -> Result<bool> {
    let Some(scheduler) = scheduler(ctx) else {
        return Err(Error::ScheduleUnsupported);
    };
    if !scheduler.remove(ctx, permit)? {
        return Ok(false);
    }
    crate::audit::record(ctx, permit, "schedule", "uninstall", "ok");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::memory::MemoryHost;
    use std::sync::Arc;

    /// A home of the test's own, gone when the test is, on a machine whose scheduler asks
    /// nobody: its files are written in that home, and launchd or systemd is never told.
    struct Scratch(PathBuf, Arc<MemoryHost>);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let root = std::env::temp_dir().join(format!(
                "pitboard-schedule-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("a scratch home");
            Scratch(root, MemoryHost::new())
        }

        fn ctx(&self) -> Context {
            Context::new(self.0.clone()).with_memory_stores(Arc::clone(&self.1))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Put a program at `path`, for a schedule to name.
    fn a_program_at(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("its directory");
        std::fs::write(path, "").expect("a program");
    }

    /// A schedule the scheduler will not start leaves nothing behind saying renewal is on.
    #[test]
    fn a_schedule_the_scheduler_will_not_start_is_not_left_on_disk() {
        let home = Scratch::new("refused");
        let program = home.0.join("bin/pitboard");
        a_program_at(&program);
        let ctx = home.ctx().with_schedule_program(program);

        home.1.refuse_next_start();
        let refused = install(&ctx, Permit::for_a_test()).expect_err("refused");

        assert_eq!(refused.code(), "schedule_refused");
        assert_eq!(status(&ctx), Installed::No);
        assert_eq!(installed_program(&ctx), None);
    }

    /// A repair the scheduler will not start leaves the schedule that was there, which is
    /// what the app and doctor then go on reporting.
    #[test]
    fn a_repair_the_scheduler_will_not_start_leaves_the_schedule_as_it_was() {
        let home = Scratch::new("repair-refused");
        std::fs::create_dir_all(home.0.join(".pitboard")).expect("a Pitboard home");
        let app = home
            .0
            .join("Applications/Pitboard.app/Contents/MacOS/Pitboard");
        let bundled = home
            .0
            .join("Applications/Pitboard.app/Contents/Helpers/pitboard");
        a_program_at(&app);
        a_program_at(&bundled);
        let ctx = home.ctx();
        install(
            &ctx.clone().with_schedule_program(app.clone()),
            Permit::for_a_test(),
        )
        .expect("0.3.0's schedule");

        home.1.refuse_next_start();
        let refused = repair(
            &ctx.clone().with_schedule_program(bundled),
            Permit::for_a_test(),
        )
        .expect_err("refused");

        assert_eq!(refused.code(), "schedule_refused");
        assert!(matches!(status(&ctx), Installed::Yes { .. }));
        assert_eq!(installed_program(&ctx), Some(app));
    }

    #[test]
    fn nothing_is_installed_on_a_machine_where_nothing_was_installed() {
        let home = Scratch::new("none");
        let ctx = home.ctx();
        assert_eq!(status(&ctx), Installed::No);
        assert_eq!(installed_program(&ctx), None);
    }

    /// A machine with no scheduler Pitboard knows says so, and writes nothing.
    #[test]
    fn a_machine_with_no_scheduler_says_so() {
        let home = Scratch::new("unsupported");
        home.1.without_a_scheduler();
        let program = home.0.join("bin/pitboard");
        a_program_at(&program);
        let ctx = home.ctx().with_schedule_program(program);
        assert_eq!(status(&ctx), Installed::Unsupported);
        assert_eq!(path(&ctx), None);
        assert_eq!(
            install(&ctx, Permit::for_a_test())
                .expect_err("nowhere")
                .code(),
            "schedule_unsupported"
        );
        assert_eq!(
            uninstall(&ctx, Permit::for_a_test())
                .expect_err("nowhere")
                .code(),
            "schedule_unsupported"
        );
        assert!(!repair(&ctx, Permit::for_a_test()).expect("nothing to do"));
    }

    #[test]
    fn the_schedule_runs_the_pitboard_the_context_names_and_otherwise_this_one() {
        let home = Scratch::new("program");
        let ctx = home.ctx();
        let scheduled = program(&ctx).expect("this program");
        let running = std::env::current_exe().expect("this test's own program");
        assert_eq!(
            std::fs::canonicalize(&scheduled).expect("there"),
            std::fs::canonicalize(&running).expect("there"),
            "the command line schedules itself"
        );
        let bundled = home
            .0
            .join("Applications/Pitboard.app/Contents/Helpers/pitboard");
        a_program_at(&bundled);
        assert_eq!(
            program(&ctx.with_schedule_program(bundled.clone())).expect("the named one"),
            bundled,
            "an app schedules the command line it comes with"
        );
    }

    /// A Pitboard that is named is written down only where it will still be there when the
    /// scheduler runs it. macOS runs an app opened where it was downloaded from a temporary
    /// copy, which is there while the app runs and gone once it quits, so being there now
    /// is not enough.
    #[test]
    fn a_named_pitboard_that_will_not_be_there_is_refused() {
        let home = Scratch::new("refused");
        let ctx = home.ctx();

        let missing = home.0.join("Pitboard.app/Contents/Helpers/pitboard");
        let refused = install(
            &ctx.clone().with_schedule_program(missing.clone()),
            Permit::for_a_test(),
        )
        .expect_err("nothing there to run");
        assert_eq!(refused.code(), "schedule_program_missing");
        assert!(
            refused.to_string().contains(&missing.display().to_string()),
            "{refused}"
        );

        let temporary = home
            .0
            .join("AppTranslocation/6A1C/d/Pitboard.app/Contents/Helpers/pitboard");
        a_program_at(&temporary);
        let refused = install(
            &ctx.clone().with_schedule_program(temporary.clone()),
            Permit::for_a_test(),
        )
        .expect_err("a copy that goes away");
        assert_eq!(refused.code(), "schedule_program_temporary");
        assert!(
            refused
                .to_string()
                .contains(&temporary.display().to_string())
                && refused.to_string().contains("Applications folder"),
            "{refused}"
        );

        assert_eq!(status(&ctx), Installed::No, "nothing was written");
    }

    /// A copy macOS runs from a temporary place is told by a directory of the path, as the
    /// Swift app told it by "/AppTranslocation/" in its command line's path, and by nothing
    /// that only looks like one.
    #[test]
    fn a_temporary_copy_is_told_by_its_path() {
        for (path, temporary) in [
            (
                "/private/var/folders/xy/abc/T/AppTranslocation/0A1B2C/d/Pitboard.app/Contents/\
                 Helpers/pitboard",
                true,
            ),
            (
                "/Applications/Pitboard.app/Contents/Helpers/pitboard",
                false,
            ),
            (
                "/Users/x/MyAppTranslocation/Pitboard.app/Contents/Helpers/pitboard",
                false,
            ),
            (
                "/Users/x/AppTranslocations/Pitboard.app/Contents/Helpers/pitboard",
                false,
            ),
        ] {
            assert_eq!(in_a_temporary_copy(Path::new(path)), temporary, "{path}");
        }
    }

    /// What `pitboard doctor` reads back is what was written, including a path the
    /// scheduler's format has to escape.
    #[test]
    fn an_installed_schedule_says_which_pitboard_it_runs() {
        let home = Scratch::new("installed");
        let bundled = home
            .0
            .join("Tools&Apps/Pitboard.app/Contents/Helpers/pitboard");
        a_program_at(&bundled);
        let ctx = home.ctx().with_schedule_program(bundled.clone());

        install(&ctx, Permit::for_a_test()).expect("installed");
        assert!(matches!(status(&ctx), Installed::Yes { .. }));
        assert_eq!(installed_program(&ctx), Some(bundled));

        assert!(uninstall(&ctx, Permit::for_a_test()).expect("taken away"));
        assert_eq!(status(&ctx), Installed::No);
        assert_eq!(installed_program(&ctx), None);
        assert!(!uninstall(&ctx, Permit::for_a_test()).expect("nothing there"));
    }

    /// An app up to 0.3.0 scheduled itself, and launchd has started a second menu bar app
    /// every day since. An app that names the command line it comes with puts that in its
    /// place, and every other schedule is left as it is.
    #[test]
    fn a_schedule_that_runs_an_app_is_pointed_at_its_command_line() {
        let home = Scratch::new("repair");
        std::fs::create_dir_all(home.0.join(".pitboard")).expect("a Pitboard home");
        let app = home
            .0
            .join("Applications/Pitboard.app/Contents/MacOS/Pitboard");
        let bundled = home
            .0
            .join("Applications/Pitboard.app/Contents/Helpers/pitboard");
        a_program_at(&app);
        a_program_at(&bundled);
        let ctx = home.ctx();
        let the_app = ctx.clone().with_schedule_program(bundled.clone());

        assert!(
            !repair(&the_app, Permit::for_a_test()).expect("nothing to do"),
            "nothing installed"
        );

        install(
            &ctx.clone().with_schedule_program(app.clone()),
            Permit::for_a_test(),
        )
        .expect("0.3.0's schedule");
        assert!(
            !repair(&ctx, Permit::for_a_test()).expect("nothing to do"),
            "no command line named"
        );
        let gone = home.0.join("Old.app/Contents/Helpers/pitboard");
        assert!(
            !repair(
                &ctx.clone().with_schedule_program(gone),
                Permit::for_a_test()
            )
            .expect("nothing to do"),
            "a command line that is not there"
        );
        assert!(
            !repair(
                &the_app.clone().with_pitboard_home(home.0.join("elsewhere")),
                Permit::for_a_test()
            )
            .expect("nothing to do"),
            "a schedule another home's Pitboard looks after"
        );
        assert_eq!(installed_program(&ctx), Some(app.clone()));

        assert!(repair(&the_app, Permit::for_a_test()).expect("repaired"));
        assert_eq!(installed_program(&ctx), Some(bundled));
        let logged = crate::audit::read(&ctx, 1);
        assert_eq!(
            logged
                .iter()
                .map(|e| (e.verb.as_str(), e.subject.as_str(), e.outcome.as_str()))
                .collect::<Vec<_>>(),
            [("schedule", "repair", "ok")]
        );

        assert!(
            !repair(&the_app, Permit::for_a_test()).expect("nothing to do"),
            "a schedule that runs a command line already"
        );
        install(
            &ctx.clone().with_schedule_program(app.clone()),
            Permit::for_a_test(),
        )
        .expect("the app again");
        let temporary = home
            .0
            .join("AppTranslocation/6A1C/d/Pitboard.app/Contents/Helpers/pitboard");
        a_program_at(&temporary);
        assert!(
            !repair(
                &ctx.clone().with_schedule_program(temporary),
                Permit::for_a_test()
            )
            .expect("nothing to do"),
            "a command line that is gone once the app quits"
        );
        assert_eq!(installed_program(&ctx), Some(app));
    }

    /// A process some other job started is not the schedule's own run, and repairs the way
    /// one opened by hand does.
    #[test]
    fn a_process_another_job_started_still_repairs() {
        let home = Scratch::new("another-job");
        std::fs::create_dir_all(home.0.join(".pitboard")).expect("a Pitboard home");
        let app = home
            .0
            .join("Applications/Pitboard.app/Contents/MacOS/Pitboard");
        let bundled = home
            .0
            .join("Applications/Pitboard.app/Contents/Helpers/pitboard");
        a_program_at(&app);
        a_program_at(&bundled);
        let ctx = home.ctx();
        install(
            &ctx.clone().with_schedule_program(app),
            Permit::for_a_test(),
        )
        .expect("0.3.0's schedule");

        let opened = ctx
            .clone()
            .with_schedule_program(bundled.clone())
            .with_scheduled_job("application.com.datlechin.pitboard.1.2".into());
        assert!(repair(&opened, Permit::for_a_test()).expect("repaired"));
        assert_eq!(installed_program(&ctx), Some(bundled));
    }

    /// Only what `install` writes is read, and only the way it writes it.
    #[test]
    fn a_schedule_pitboard_did_not_write_names_no_program() {
        let home = Scratch::new("foreign");
        let ctx = home.ctx();
        let installed = path(&ctx).expect("a scheduler here");
        std::fs::create_dir_all(installed.parent().expect("its directory")).expect("made");
        std::fs::write(&installed, "not what install writes\n").expect("written");
        if let Some(beside) = installed.parent() {
            // What systemd's timer runs is its service, beside it.
            std::fs::write(
                beside.join("pitboard-renew.service"),
                "[Service]\nExecStart=/bin/true\n",
            )
            .expect("written");
        }

        assert!(matches!(status(&ctx), Installed::Yes { .. }));
        assert_eq!(installed_program(&ctx), None);
    }

    /// The scheduler starts `renew` with the default home, so a Pitboard pointed anywhere
    /// else leaves the schedule alone.
    #[test]
    fn the_schedule_belongs_to_the_default_home_alone() {
        let ctx = Context::new(PathBuf::from("/home/x"));
        assert!(serves(&ctx));
        assert!(serves(
            &ctx.clone()
                .with_pitboard_home(PathBuf::from("/home/x/.pitboard/"))
        ));
        assert!(!serves(
            &ctx.with_pitboard_home(PathBuf::from("/tmp/elsewhere"))
        ));
    }

    /// A real context's scheduler is never asked from a test: the system's own service
    /// manager would reach the person's real schedule.
    #[test]
    fn a_test_never_reaches_the_systems_own_scheduler() {
        let root = std::env::temp_dir().join(format!(
            "pitboard-schedule-real-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let program = root.join("bin/pitboard");
        a_program_at(&program);
        let ctx = Context::new(root.clone()).with_schedule_program(program);
        let refused =
            install(&ctx, Permit::for_a_test()).expect_err("refused before it reaches the system");
        assert_eq!(refused.code(), "schedule_refused");
        assert_eq!(status(&ctx), Installed::No, "and nothing is left written");
        let _ = std::fs::remove_dir_all(&root);
    }
}
