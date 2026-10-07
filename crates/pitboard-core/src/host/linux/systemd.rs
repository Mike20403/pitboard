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

    /// Read from the service's `Environment=` lines, as [`unit_file`] writes them.
    fn environment(&self, ctx: &Context) -> Vec<(String, String)> {
        if !self.installed(ctx) {
            return Vec::new();
        }
        std::fs::read_to_string(Self::units(ctx).join(SERVICE))
            .map(|body| given(&body))
            .unwrap_or_default()
    }

    fn put(
        &self,
        ctx: &Context,
        permit: Permit,
        program: &Path,
        environment: &[(String, String)],
    ) -> Result<()> {
        let unit = Self::units(ctx).join(SERVICE);
        let timer_path = self.location(ctx);
        let before = (
            std::fs::read_to_string(&unit).ok(),
            std::fs::read_to_string(&timer_path).ok(),
        );
        service::write(permit, &unit, &unit_file(program, environment))?;
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

/// systemd's own format. `environment` is as for the launchd job: the service runs with
/// systemd's environment, so what it is given of the installing Pitboard's,
/// `PITBOARD_NO_ARGV` and the proxy variables, is written in, one `Environment=` line each.
/// It runs `<program> renew --scheduled`, the marker a run of the schedule is told apart by,
/// rather than by what systemd passes it. A Pitboard from before the marker rejects the
/// flag, so after going back to one the timer fails every day until that Pitboard's
/// `schedule install` rewrites the unit, which the CHANGELOG says.
fn unit_file(program: &Path, environment: &[(String, String)]) -> String {
    format!(
        "[Unit]\n\
         Description=Renew Pitboard's parked logins\n\
         Documentation=https://docs.usepitboard.com\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         {}\
         ExecStart={} {}\n",
        environment
            .iter()
            .map(|(name, value)| assignment(name, value))
            .collect::<String>(),
        program.display(),
        SCHEDULED_RUN.join(" ")
    )
}

/// The characters an assignment can hold with no quotes around it: none that systemd reads
/// as anything but itself.
fn plain(text: &str) -> bool {
    text.chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.:/@,".contains(c))
}

/// One `Environment=` line, giving the service `name` with `value`, as systemd reads it back
/// whole. Read in systemd.exec(5) and systemd.syntax(7), at systemd's main branch on 7 October
/// 2026 (a28ccc9): each line is unquoted by systemd.syntax's rules, so an assignment holding
/// a space or an `=` is put in double quotes, inside which a backslash and a double quote are
/// written with a backslash before them; a `%` starts a specifier, so it is written `%%`; `$`
/// means nothing. A character that does not print, a line break among them, is written as
/// systemd.syntax's `\xNN` or `\uNNNN`, so it never ends the line; systemd then refuses that
/// one assignment, as it refuses every value holding one.
fn assignment(name: &str, value: &str) -> String {
    if plain(name) && plain(value) {
        return format!("Environment={name}={value}\n");
    }
    let mut quoted = String::new();
    for c in format!("{name}={value}").chars() {
        match c {
            '\\' | '"' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            c if c.is_control() && c.is_ascii() => quoted.push_str(&format!("\\x{:02x}", c as u32)),
            c if c.is_control() => quoted.push_str(&format!("\\u{:04x}", c as u32)),
            c => quoted.push(c),
        }
    }
    format!("Environment=\"{quoted}\"\n")
}

/// The variables a unit [`unit_file`] wrote gives its service, in the order written: each
/// `Environment=` line read back as [`assignment`] wrote it.
fn given(body: &str) -> Vec<(String, String)> {
    body.lines()
        .filter_map(|line| {
            let written = line.strip_prefix("Environment=")?;
            let assigned = match written
                .strip_prefix('"')
                .and_then(|quoted| quoted.strip_suffix('"'))
            {
                Some(quoted) => unquoted(quoted),
                None => written.to_string(),
            };
            let (name, value) = assigned.split_once('=')?;
            Some((name.to_string(), value.to_string()))
        })
        .collect()
}

/// What [`assignment`] put between double quotes, read back: `\\`, `\"` and `%%` are each
/// the one character they stand for, and `\xNN` and `\uNNNN` the character of that number.
fn unquoted(quoted: &str) -> String {
    let mut text = String::with_capacity(quoted.len());
    let mut chars = quoted.chars();
    while let Some(c) = chars.next() {
        let escaped = match c {
            '\\' | '%' => chars.next().unwrap_or(c),
            c => {
                text.push(c);
                continue;
            }
        };
        let digits = match (c, escaped) {
            ('\\', 'x') => 2,
            ('\\', 'u') => 4,
            _ => {
                text.push(escaped);
                continue;
            }
        };
        let number: String = chars.by_ref().take(digits).collect();
        text.extend(
            u32::from_str_radix(&number, 16)
                .ok()
                .and_then(char::from_u32),
        );
    }
    text
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
    use std::os::unix::fs::PermissionsExt;

    /// The pairs a service is given, as `install` hands them over.
    fn pairs(given: &[(&str, &str)]) -> Vec<(String, String)> {
        given
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect()
    }

    /// The service runs with systemd's environment, not the shell's, so what it is given of
    /// the installing Pitboard's, `PITBOARD_NO_ARGV` and the proxy variables, is written in,
    /// one `Environment=` line each, quoted where systemd would otherwise split or change it,
    /// and nothing is written where there is nothing to give. What was written reads back as
    /// it was.
    #[test]
    fn the_unit_gives_its_service_the_variables_it_is_handed_and_reads_them_back() {
        let program = Path::new("/usr/local/bin/pitboard");
        let handed = pairs(&[
            ("PITBOARD_NO_ARGV", "1"),
            ("ALL_PROXY", "socks5h://alice:p%40ss\"w\\rd@127.0.0.1:1080"),
            ("HTTPS_PROXY", "http://proxy.example.com:3128"),
            ("no_proxy", ""),
            ("NO_PROXY", "*.anthropic.com, localhost"),
        ]);
        let unit = unit_file(program, &handed);
        assert!(
            unit.contains(
                "Type=oneshot\n\
                 Environment=PITBOARD_NO_ARGV=1\n\
                 Environment=\"ALL_PROXY=socks5h://alice:p%%40ss\\\"w\\\\rd@127.0.0.1:1080\"\n\
                 Environment=HTTPS_PROXY=http://proxy.example.com:3128\n\
                 Environment=no_proxy=\n\
                 Environment=\"NO_PROXY=*.anthropic.com, localhost\"\n\
                 ExecStart=/usr/local/bin/pitboard renew --scheduled\n"
            ),
            "{unit}"
        );
        assert_eq!(given(&unit), handed);
        assert!(!unit_file(program, &[]).contains("Environment="));
        assert_eq!(given(&unit_file(program, &[])), Vec::new());

        // A line break in a value never ends the line it is on.
        let broken = pairs(&[("NO_PROXY", "a\nExecStart=/bin/false\u{85}")]);
        let unit = unit_file(program, &broken);
        assert!(
            unit.contains("Environment=\"NO_PROXY=a\\x0aExecStart=/bin/false\\u0085\"\n"),
            "{unit}"
        );
        assert_eq!(
            unit.lines().filter(|l| l.starts_with("ExecStart=")).count(),
            1,
            "{unit}"
        );
        assert_eq!(given(&unit), broken);
    }

    /// The service runs one verb, with the marker a run of the schedule is told apart by,
    /// rather than by what systemd passes it.
    #[test]
    fn the_unit_runs_one_verb_and_the_timer_survives_a_machine_being_off() {
        let unit = unit_file(Path::new("/usr/local/bin/pitboard"), &[]);
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

    /// `pitboard schedule install` writes the proxy variables it was started with into the
    /// service, and the service and its timer only the person can read, mode 600, since a
    /// proxy's address can hold a password: over a service that was open to others too,
    /// which kept its mode when Pitboard wrote it before.
    #[test]
    fn an_installed_unit_carries_the_proxy_and_only_its_owner_can_read_it() {
        let home = std::env::temp_dir().join(format!(
            "pitboard-systemd-proxy-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        let program = home.join("bin/pitboard");
        std::fs::create_dir_all(program.parent().expect("its directory")).expect("made");
        std::fs::write(&program, "").expect("a program");
        let home_text = home.to_string_lossy().into_owned();
        let env: crate::context::Environment = [
            ("HOME", home_text.as_str()),
            ("ALL_PROXY", "socks5h://alice:s3cret@127.0.0.1:1080"),
            ("no_proxy", "localhost"),
        ]
        .into_iter()
        .collect();
        let ctx = Context::for_command_line(&env)
            .with_memory_stores(MemoryHost::new())
            .with_schedule_program(program.clone());
        let units = Systemd::units(&ctx);
        std::fs::create_dir_all(&units).expect("made");
        std::fs::write(units.join(SERVICE), "a service open to others\n").expect("written");
        std::fs::set_permissions(units.join(SERVICE), std::fs::Permissions::from_mode(0o644))
            .expect("opened to others");

        install(&ctx, Permit::for_a_test()).expect("installed");
        let written = std::fs::read_to_string(units.join(SERVICE)).expect("the service");
        assert!(
            written.contains(
                "Environment=ALL_PROXY=socks5h://alice:s3cret@127.0.0.1:1080\n\
                 Environment=no_proxy=localhost\n"
            ),
            "{written}"
        );
        for file in [SERVICE, TIMER] {
            assert_eq!(
                crate::host::fs::access(&units.join(file)).map(|access| access.described),
                Some("mode 600".to_string()),
                "{file}"
            );
        }
        assert_eq!(installed_program(&ctx), Some(program));
        let _ = std::fs::remove_dir_all(&home);
    }
}
