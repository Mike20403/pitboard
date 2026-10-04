//! Daily renewal on macOS: a LaunchAgent, which runs inside the login session so the keychain
//! is unlocked.

use super::super::unix::service::{self, Control};
use crate::context::Context;
use crate::error::Result;
use crate::host::Scheduler;
use crate::schedule::EVERY_SECONDS;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What launchd calls the job, so a person can find it without Pitboard telling them.
const LABEL: &str = "com.datlechin.pitboard.renew";

const LAUNCHCTL: &str = "/bin/launchctl";

#[derive(Debug)]
pub(super) struct Launchd {
    control: Arc<dyn Control>,
}

impl Launchd {
    pub(super) fn new(control: Arc<dyn Control>) -> Launchd {
        Launchd { control }
    }

    fn agent(ctx: &Context) -> PathBuf {
        ctx.home()
            .join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist"))
    }

    /// launchd's session for this user, which `bootstrap` and `bootout` name.
    fn domain() -> String {
        // SAFETY: `getuid` cannot fail and touches no memory of this process.
        format!("gui/{}", unsafe { libc::getuid() })
    }
}

impl Scheduler for Launchd {
    fn location(&self, ctx: &Context) -> PathBuf {
        Self::agent(ctx)
    }

    fn installed(&self, ctx: &Context) -> bool {
        Self::agent(ctx).is_file()
    }

    fn program(&self, ctx: &Context) -> Option<PathBuf> {
        let body = std::fs::read_to_string(Self::agent(ctx)).ok()?;
        let (_, after) = body.split_once("<key>ProgramArguments</key>")?;
        let (_, after) = after.split_once("<string>")?;
        let (program, _) = after.split_once("</string>")?;
        Some(PathBuf::from(unescape(program)))
    }

    fn put(&self, ctx: &Context, program: &Path) -> Result<()> {
        let path = Self::agent(ctx);
        let before = std::fs::read_to_string(&path).ok();
        service::write(&path, &plist(program, ctx.argv_fallback()))?;
        // `bootstrap` is launchd's own word for this, and replaces the deprecated `load`.
        let domain = Self::domain();
        let target = path.to_string_lossy();
        let _ = self.control.run(LAUNCHCTL, &["bootout", &domain, &target]);
        if let Err(refused) = self
            .control
            .start(LAUNCHCTL, &["bootstrap", &domain, &target])
        {
            service::restore(&path, before.as_deref());
            if before.is_some() {
                let _ = self
                    .control
                    .start(LAUNCHCTL, &["bootstrap", &domain, &target]);
            }
            return Err(refused);
        }
        Ok(())
    }

    fn remove(&self, ctx: &Context) -> Result<bool> {
        let path = Self::agent(ctx);
        if !path.is_file() {
            return Ok(false);
        }
        let _ = self.control.run(
            LAUNCHCTL,
            &["bootout", &Self::domain(), &path.to_string_lossy()],
        );
        service::remove(&path)?;
        Ok(true)
    }

    /// launchd puts the label of the job it starts in `XPC_SERVICE_NAME`.
    fn started_this_process(&self, said: Option<&str>) -> bool {
        said.map(str::to_owned)
            .or_else(|| std::env::var("XPC_SERVICE_NAME").ok())
            .as_deref()
            == Some(LABEL)
    }
}

/// launchd's own format. `RunAtLoad` is off: installing this is not a reason to talk to
/// Anthropic that second, and the first run comes at the first interval.
///
/// `argument_line` is whether the Pitboard installing it may write a login on the argument
/// line. The job runs with launchd's environment, not the person's shell, so a
/// `PITBOARD_NO_ARGV` they set would not reach it; where it is set, the job is given it.
fn plist(program: &Path, argument_line: bool) -> String {
    let environment = if argument_line {
        ""
    } else {
        "  <key>EnvironmentVariables</key>\n  <dict>\n    <key>PITBOARD_NO_ARGV</key><string>1</string>\n  </dict>\n"
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>renew</string>
  </array>
  <key>StartInterval</key><integer>{EVERY_SECONDS}</integer>
  <key>RunAtLoad</key><false/>
  <key>LowPriorityIO</key><true/>
  <key>ProcessType</key><string>Background</string>
{environment}</dict>
</plist>
"#,
        escape(&program.to_string_lossy())
    )
}

/// A path as XML text. An app can be kept in a folder whose name has an ampersand in it,
/// and launchd refuses a plist that is not well formed.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// What [`escape`] wrote, read back.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::memory::MemoryHost;
    use crate::schedule::{install, installed_program, repair};

    /// launchd stops a job's own process when it unloads the job, and a repair unloads the
    /// schedule before loading it again. Made from inside the schedule's own job, as by an
    /// app the old schedule started, it would leave nothing loaded, so it is left to the app
    /// once it is opened any other way.
    #[test]
    fn a_schedule_is_not_repaired_from_inside_its_own_job() {
        let home = std::env::temp_dir().join(format!(
            "pitboard-launchd-inside-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".pitboard")).expect("a Pitboard home");
        let app = home.join("Applications/Pitboard.app/Contents/MacOS/Pitboard");
        let bundled = home.join("Applications/Pitboard.app/Contents/Helpers/pitboard");
        for program in [&app, &bundled] {
            std::fs::create_dir_all(program.parent().unwrap()).expect("its directory");
            std::fs::write(program, "").expect("a program");
        }
        let ctx = Context::new(home.clone()).with_memory_stores(MemoryHost::new());
        install(&ctx.clone().with_schedule_program(app.clone())).expect("0.3.0's schedule");
        let the_app = ctx.clone().with_schedule_program(bundled.clone());

        assert!(
            !repair(&the_app.clone().with_scheduled_job(LABEL.into())).expect("nothing to do"),
            "started by the schedule"
        );
        assert_eq!(installed_program(&ctx), Some(app));
        assert_eq!(
            crate::audit::read(&ctx, 1)
                .iter()
                .map(|e| e.subject.as_str())
                .collect::<Vec<_>>(),
            ["install"],
            "and nothing recorded"
        );

        let opened = the_app.with_scheduled_job("application.com.datlechin.pitboard.1.2".into());
        assert!(repair(&opened).expect("repaired"), "opened from Finder");
        assert_eq!(installed_program(&ctx), Some(bundled));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// The path is written as XML text and read back as the path it was.
    #[test]
    fn a_path_launchd_has_to_escape_reads_back_as_it_was() {
        let home = std::env::temp_dir().join(format!(
            "pitboard-launchd-escape-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        let bundled = home.join("Tools&Apps/Pitboard.app/Contents/Helpers/pitboard");
        std::fs::create_dir_all(bundled.parent().unwrap()).expect("its directory");
        std::fs::write(&bundled, "").expect("a program");
        let ctx = Context::new(home.clone())
            .with_memory_stores(MemoryHost::new())
            .with_schedule_program(bundled.clone());
        install(&ctx).expect("installed");
        let written = std::fs::read_to_string(Launchd::agent(&ctx)).expect("the agent");
        assert!(written.contains("/Tools&amp;Apps/"), "{written}");
        assert!(!written.contains("/Tools&Apps/"), "{written}");
        assert_eq!(installed_program(&ctx), Some(bundled));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// What it runs is one verb with no arguments, and what it does not do is as much the
    /// point as what it does.
    #[test]
    fn the_agent_runs_one_verb_and_does_not_run_at_load() {
        let body = plist(Path::new("/usr/local/bin/pitboard"), true);
        assert!(body.contains("<string>/usr/local/bin/pitboard</string>"));
        assert!(body.contains("<string>renew</string>"));
        assert!(!body.contains("status"), "it never asks for usage");
        assert!(!body.contains("use"), "and it never switches");
        assert!(
            body.contains("<key>RunAtLoad</key><false/>"),
            "installing it is not a reason to talk to Anthropic that second"
        );
        assert!(body.contains(&format!("<integer>{EVERY_SECONDS}</integer>")));
    }

    /// launchd refuses a plist that is not well formed, and a folder's name can hold any of
    /// the characters XML gives a meaning to.
    #[test]
    fn the_agent_writes_a_path_as_xml_text() {
        let body = plist(Path::new("/Users/x/A&B <old>/Pitboard.app"), true);
        assert!(
            body.contains("<string>/Users/x/A&amp;B &lt;old&gt;/Pitboard.app</string>"),
            "{body}"
        );
    }

    /// The job runs with launchd's environment, not the shell's, so a `PITBOARD_NO_ARGV`
    /// the installing Pitboard has is written into it, and nothing is written otherwise.
    /// The program is still read back from what was written.
    #[test]
    fn the_agent_keeps_a_refusal_of_the_argument_line() {
        let program = Path::new("/usr/local/bin/pitboard");
        let refusing = plist(program, false);
        assert!(
            refusing.contains("<key>PITBOARD_NO_ARGV</key><string>1</string>"),
            "{refusing}"
        );
        assert!(!plist(program, true).contains("EnvironmentVariables"));
        let (_, after) = refusing
            .split_once("<key>ProgramArguments</key>")
            .expect("its arguments");
        assert!(
            after
                .trim_start()
                .starts_with("<array>\n    <string>/usr/local/bin/pitboard")
        );
    }
}
