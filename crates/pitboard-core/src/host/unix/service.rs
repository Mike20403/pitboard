//! A job a POSIX service manager runs: the files that describe it, and the manager asked to
//! start or stop it the way a person would type it. launchd and systemd both work this way.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Whoever answers for the service manager.
pub(crate) trait Control: Send + Sync + std::fmt::Debug {
    /// Run `program` with `args`.
    fn run(&self, program: &str, args: &[&str]) -> Result<()>;

    /// Run `program` with `args` to start a job, which is the one request whose refusal
    /// changes what a scheduler leaves behind.
    fn start(&self, program: &str, args: &[&str]) -> Result<()> {
        self.run(program, args)
    }
}

/// The system's own service manager.
#[derive(Debug)]
pub(crate) struct System;

impl Control for System {
    /// Never from a test. A test's home is a scratch directory, but the manager it would ask
    /// is the person's own: launchd finds a job by the label inside its file, so booting out
    /// a scratch copy stops their real schedule, and `systemctl --user` reaches the one
    /// session there is. A test gives its context a `MemoryHost`, whose scheduler asks
    /// [`Pretend`], and this refuses the rest.
    fn run(&self, program: &str, args: &[&str]) -> Result<()> {
        if cfg!(test) {
            return Err(Error::ScheduleRefused {
                detail: format!("a test asked {program} itself"),
            });
        }
        let out = std::process::Command::new(program)
            .args(args)
            .output()
            .map_err(|source| Error::HomeUnwritable {
                path: PathBuf::from(program),
                source,
            })?;
        if out.status.success() {
            return Ok(());
        }
        Err(Error::ScheduleRefused {
            detail: format!(
                "{program} exited {}: {}",
                out.status
                    .code()
                    .map_or_else(|| "on a signal".into(), |c| c.to_string()),
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        })
    }
}

/// A service manager that answers without asking anybody, for a machine in memory. It
/// refuses a start once when told to, which is how a test reaches what happens when the
/// system will not start a schedule.
#[derive(Debug)]
#[cfg(any(test, feature = "test-support"))]
pub(crate) struct Pretend {
    pub(crate) refuse_start: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(any(test, feature = "test-support"))]
impl Control for Pretend {
    fn run(&self, _program: &str, _args: &[&str]) -> Result<()> {
        Ok(())
    }

    fn start(&self, program: &str, _args: &[&str]) -> Result<()> {
        if self
            .refuse_start
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(Error::ScheduleRefused {
                detail: format!("{program} refused to start it in a test"),
            });
        }
        Ok(())
    }
}

/// Write one of the job's files, keeping the mode one that is there already has.
pub(crate) fn write(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::HomeUnwritable {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    crate::atomic::write(path, body.as_bytes(), crate::atomic::Perms::MatchExisting).map_err(
        |source| Error::HomeUnwritable {
            path: path.to_path_buf(),
            source,
        },
    )
}

/// Put a file back the way it was before this run wrote it: its old contents, or not there.
/// Best effort, because it runs on the way out of a failure that is already being reported.
pub(crate) fn restore(path: &Path, before: Option<&str>) {
    let _ = match before {
        Some(body) => write(path, body),
        None => remove(path),
    };
}

/// Take one of the job's files away. One that is not there is already gone.
pub(crate) fn remove(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::HomeUnwritable {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// The control a host's scheduler asks.
pub(crate) fn system() -> Arc<dyn Control> {
    Arc::new(System)
}
