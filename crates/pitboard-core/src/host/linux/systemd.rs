//! Daily renewal on Linux: a systemd user timer, which is that system's answer.

use super::super::unix::service::{self, Control};
use crate::context::Context;
use crate::error::Result;
use crate::host::Scheduler;
use crate::schedule::EVERY_SECONDS;
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

    fn program(&self, ctx: &Context) -> Option<PathBuf> {
        if !self.installed(ctx) {
            return None;
        }
        let body = std::fs::read_to_string(Self::units(ctx).join(SERVICE)).ok()?;
        body.lines()
            .find_map(|line| line.strip_prefix("ExecStart=")?.strip_suffix(" renew"))
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

    /// Never asked for here. A repair on Linux stops nothing: it rewrites the units and
    /// enables the timer again, so a run of the schedule is not in its way.
    fn started_this_process(&self, _said: Option<&str>) -> bool {
        false
    }
}

/// systemd's own format. `argument_line` is as for the launchd job: the service runs with
/// systemd's environment, so a `PITBOARD_NO_ARGV` the installing Pitboard has is given to it.
fn unit_file(program: &Path, argument_line: bool) -> String {
    format!(
        "[Unit]\n\
         Description=Renew Pitboard's parked logins\n\
         Documentation=https://docs.usepitboard.com\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         {}\
         ExecStart={} renew\n",
        if argument_line {
            ""
        } else {
            "Environment=PITBOARD_NO_ARGV=1\n"
        },
        program.display()
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

    #[test]
    fn the_unit_keeps_a_refusal_of_the_argument_line() {
        let program = Path::new("/usr/local/bin/pitboard");
        let refusing = unit_file(program, false);
        assert!(
            refusing.contains("Environment=PITBOARD_NO_ARGV=1\n"),
            "{refusing}"
        );
        assert!(refusing.contains("ExecStart=/usr/local/bin/pitboard renew"));
        assert!(!unit_file(program, true).contains("Environment="));
    }

    #[test]
    fn the_unit_runs_one_verb_and_the_timer_survives_a_machine_being_off() {
        let unit = unit_file(Path::new("/usr/local/bin/pitboard"), true);
        assert!(unit.contains("ExecStart=/usr/local/bin/pitboard renew"));
        assert!(!unit.contains("status"));
        let timer = timer();
        assert!(timer.contains("Persistent=true"));
        assert!(timer.contains("WantedBy=timers.target"));
    }
}
