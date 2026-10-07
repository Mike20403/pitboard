//! Daily renewal on Linux: a systemd user timer, which is that system's answer.

use super::super::unix::service::{self, Control};
use crate::context::Context;
use crate::error::Result;
use crate::host::Scheduler;
use crate::schedule::{EVERY_SECONDS, SCHEDULED_RUN};
use crate::service::Permit;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SYSTEMCTL: &str = "systemctl";
const TIMER: &str = "pitboard-renew.timer";
const SERVICE: &str = "pitboard-renew.service";

#[derive(Debug)]
pub(super) struct Systemd {
    control: Arc<dyn Control>,
}

impl Systemd {
    pub(super) fn new(control: Arc<dyn Control>) -> Systemd {
        Systemd { control }
    }

    fn units(ctx: &Context) -> PathBuf {
        ctx.home().join(".config/systemd/user")
    }
}

impl Scheduler for Systemd {
    /// The timer is what says the schedule is installed; the service is what it runs.
    fn location(&self, ctx: &Context) -> PathBuf {
        Self::units(ctx).join(TIMER)
    }

    fn installed(&self, ctx: &Context) -> bool {
        self.location(ctx).is_file()
    }

    /// Read from the service's `ExecStart`, as [`unit_file`] writes it, or as Pitboard wrote
    /// it before its job carried the marker: `<program> renew`.
    fn program(&self, ctx: &Context) -> Option<PathBuf> {
        if !self.installed(ctx) {
            return None;
        }
        let body = std::fs::read_to_string(Self::units(ctx).join(SERVICE)).ok()?;
        let marked = format!(" {}", SCHEDULED_RUN.join(" "));
        body.lines()
            .find_map(|line| {
                let run = line.strip_prefix("ExecStart=")?;
                run.strip_suffix(marked.as_str())
                    .or_else(|| run.strip_suffix(" renew"))
            })
            .map(PathBuf::from)
    }

    fn put(&self, ctx: &Context, permit: Permit, program: &Path) -> Result<()> {
        let unit = Self::units(ctx).join(SERVICE);
        let timer_path = self.location(ctx);
        let before = (
            std::fs::read_to_string(&unit).ok(),
            std::fs::read_to_string(&timer_path).ok(),
        );
        service::write(permit, &unit, &unit_file(program, ctx.argv_fallback()))?;
        service::write(permit, &timer_path, &timer())?;
        let reload = ["--user", "daemon-reload"];
        let _ = self.control.run(permit, SYSTEMCTL, &reload);
        let start = ["--user", "enable", "--now", TIMER];
        if let Err(refused) = self.control.start(permit, SYSTEMCTL, &start) {
            service::restore(permit, &unit, before.0.as_deref());
            service::restore(permit, &timer_path, before.1.as_deref());
            let _ = self.control.run(permit, SYSTEMCTL, &reload);
            if before.1.is_some() {
                let _ = self.control.start(permit, SYSTEMCTL, &start);
            }
            return Err(refused);
        }
        Ok(())
    }

    fn remove(&self, ctx: &Context, permit: Permit) -> Result<bool> {
        let timer_path = self.location(ctx);
        if !timer_path.is_file() {
            return Ok(false);
        }
        let _ = self
            .control
            .run(permit, SYSTEMCTL, &["--user", "disable", "--now", TIMER]);
        service::remove(permit, &timer_path)?;
        service::remove(permit, &Self::units(ctx).join(SERVICE))?;
        let _ = self
            .control
            .run(permit, SYSTEMCTL, &["--user", "daemon-reload"]);
        Ok(true)
    }

    /// Nothing systemd passes is read here, only what a test says the job is: what it
    /// passes is not to be relied on ([`SCHEDULED_RUN`]), so the service's runs are told
    /// apart by the marker its `ExecStart` carries, which the command line started with it
    /// passes on. A repair on Linux stops nothing either way: it rewrites the units and
    /// enables the timer again, so a run of the schedule is not in its way.
    fn started_this_process(&self, said: Option<&str>) -> bool {
        said == Some(SERVICE)
    }
}

/// systemd's own format. `argument_line` is as for the launchd job: the service runs with
/// systemd's environment, so a `PITBOARD_NO_ARGV` the installing Pitboard has is given to it.
/// It runs `<program> renew --scheduled`, the marker a run of the schedule is told apart by,
/// rather than by what systemd passes it. A Pitboard from before the marker rejects the
/// flag, so after going back to one the timer fails every day until that Pitboard's
/// `schedule install` rewrites the unit, which the CHANGELOG says.
fn unit_file(program: &Path, argument_line: bool) -> String {
    format!(
        "[Unit]\n\
         Description=Renew Pitboard's parked logins\n\
         Documentation=https://docs.usepitboard.com\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         {}\
         ExecStart={} {}\n",
        if argument_line {
            ""
        } else {
            "Environment=PITBOARD_NO_ARGV=1\n"
        },
        program.display(),
        SCHEDULED_RUN.join(" ")
    )
}

fn timer() -> String {
    format!(
        "[Unit]\n\
         Description=Renew Pitboard's parked logins daily\n\
         \n\
         [Timer]\n\
         OnUnitActiveSec={EVERY_SECONDS}\n\
         OnStartupSec=900\n\
         Persistent=true\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::host::memory::MemoryHost;
    use crate::schedule::{install, installed_program};

    #[test]
    fn the_unit_keeps_a_refusal_of_the_argument_line() {
        let program = Path::new("/usr/local/bin/pitboard");
        let refusing = unit_file(program, false);
        assert!(
            refusing.contains("Environment=PITBOARD_NO_ARGV=1\n"),
            "{refusing}"
        );
        assert!(refusing.contains("ExecStart=/usr/local/bin/pitboard renew --scheduled\n"));
        assert!(!unit_file(program, true).contains("Environment="));
    }

    /// The service runs one verb, with the marker a run of the schedule is told apart by,
    /// rather than by what systemd passes it.
    #[test]
    fn the_unit_runs_one_verb_and_the_timer_survives_a_machine_being_off() {
        let unit = unit_file(Path::new("/usr/local/bin/pitboard"), true);
        assert!(
            unit.contains("ExecStart=/usr/local/bin/pitboard renew --scheduled\n"),
            "{unit}"
        );
        assert!(!unit.contains("status"));
        let timer = timer();
        assert!(timer.contains("Persistent=true"));
        assert!(timer.contains("WantedBy=timers.target"));
    }

    /// A unit Pitboard wrote before its job carried the marker still names the Pitboard it
    /// runs, and installing again writes the marker in. A test says the job by the unit's
    /// own name; nothing systemd passes is read.
    #[test]
    fn a_unit_written_without_the_marker_still_names_its_program_and_install_adds_it() {
        let home = std::env::temp_dir().join(format!(
            "pitboard-systemd-marker-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        let program = home.join("bin/pitboard");
        std::fs::create_dir_all(program.parent().expect("its directory")).expect("made");
        std::fs::write(&program, "").expect("a program");
        let ctx = Context::new(home.clone())
            .with_memory_stores(MemoryHost::new())
            .with_schedule_program(program.clone());
        let units = Systemd::units(&ctx);
        std::fs::create_dir_all(&units).expect("made");
        std::fs::write(units.join(TIMER), timer()).expect("written");
        std::fs::write(
            units.join(SERVICE),
            format!(
                "[Service]\nType=oneshot\nExecStart={} renew\n",
                program.display()
            ),
        )
        .expect("written");
        assert_eq!(installed_program(&ctx), Some(program.clone()), "as before");

        install(&ctx, Permit::for_a_test()).expect("installed again");
        let written = std::fs::read_to_string(units.join(SERVICE)).expect("the service");
        assert!(
            written.contains(&format!(
                "ExecStart={} renew --scheduled\n",
                program.display()
            )),
            "{written}"
        );
        assert_eq!(installed_program(&ctx), Some(program));

        let systemd = Systemd::new(Arc::new(service::Pretend {
            refuse_start: Arc::default(),
        }));
        assert!(systemd.started_this_process(Some(SERVICE)));
        assert!(!systemd.started_this_process(Some(TIMER)));
        assert!(
            !systemd.started_this_process(None),
            "what systemd passes is not read"
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
