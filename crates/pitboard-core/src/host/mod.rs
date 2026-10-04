//! The machine pitboard runs on, behind one seam.
//!
//! Everything that differs between operating systems is answered here, so the rest of the
//! crate asks a question and never which system it is on. The seam has two faces.
//!
//! [`Host`] is the part a test replaces: the stores secrets and logins live in, the process
//! list and the scheduler, reached through the [`Context`] every call carries. A test puts
//! [`memory::MemoryHost`] there and can make any of them fail.
//!
//! [`fs`], [`proc`] and [`user`] are plain functions for what the machine does the same way
//! whoever asks, which the tests run for real: creating a file only its owner can reach,
//! asking whether a process is still alive, naming the person signed in.
//!
//! The system is chosen once, in this file, and nowhere else. A fact that differs by system
//! is a `match` on [`OS`], and [`Os`] lists every system pitboard runs on, so a system added
//! there does not compile until each such fact has been said for it. This replaced branches
//! that read "macOS, or else Linux", which compiled anywhere and did the Linux thing.

use crate::context::Context;
use crate::error::Result;
use crate::store::RawStore;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(test, feature = "test-support"))]
pub mod memory;
pub(crate) mod program;
#[cfg(unix)]
mod unix;

#[cfg(target_os = "linux")]
use linux as os;
#[cfg(target_os = "macos")]
use macos as os;

pub(crate) use os::{fs, proc, user};

/// The operating systems pitboard runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
}

/// The system this build runs on.
pub const OS: Os = os::OS;

/// The only program trusted to read Claude Code's keychain item. Named here so the backend
/// that runs it and the doctor check that looks for it cannot drift apart.
pub const SECURITY: &str = "/usr/bin/security";

impl Os {
    /// The program pitboard reaches the system's own store of secrets through, where it goes
    /// through one rather than calling the system directly.
    pub fn secrets_tool(self) -> Option<&'static str> {
        match self {
            Os::MacOs => Some(SECURITY),
            Os::Linux => None,
        }
    }

    /// The command a person types on this system to make `paths` private to themselves.
    pub fn make_private_command(self, kind: Kind, paths: &[&str]) -> String {
        match self {
            Os::MacOs | Os::Linux => {
                let mode = match kind {
                    Kind::Directory => "700",
                    Kind::File => "600",
                    Kind::Any => "go-rwx",
                };
                format!("chmod {mode} {}", paths.join(" "))
            }
        }
    }
}

/// Who besides a file's owner can reach it, as the system says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    /// Whether anybody but the owner can read or change it.
    pub shared: bool,
    /// What the system says about it, for a person to read: `mode 600` where access is a
    /// mode.
    pub described: String,
}

/// What a person makes private with the command [`Os::make_private_command`] gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Directory,
    File,
    /// Directories and files alike.
    Any,
}

/// One process this user is running, and where its program runs from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    /// The program's path where the system says one, or its bare name where it does not.
    /// macOS gives what the program was started as, which is its full path when whatever
    /// started it named one; Linux gives the file it runs, where this user may read that.
    pub path: PathBuf,
}

/// The machine pitboard is standing on, as one value rather than a set of `cfg` branches
/// spread through the crate. A host answers where another program's secrets may be, where
/// pitboard's own parked logins go, what this user is running and how daily renewal is
/// scheduled. It takes the context on every call because a context is built by a builder
/// and can still change after it exists.
///
/// It was called `Platform` until a second provider was on the way. The name said "which
/// operating system", the body reached into Claude Code's own slot hashing, and once
/// "provider" became a word this codebase uses, a reader meeting `Platform` could not tell
/// which of the two axes it meant. What stays behind this name is the machine, and only the
/// machine: which item or file a tool keeps its login in is the tool's to say.
pub(crate) trait Host: Send + Sync + std::fmt::Debug {
    /// Secrets another program keeps in the system's own store of them, under `account`:
    /// the login keychain on macOS. `None` where the system has no such store, and a tool
    /// keeps its login in a file instead.
    ///
    /// Which items and which account is the other program's business, so both are handed
    /// in. Deriving them here is how Claude Code's slot hashing came to live inside what
    /// claimed to be an operating-system abstraction.
    fn foreign_secrets(&self, ctx: &Context, account: &str) -> Option<Box<dyn RawStore>>;

    /// The single file at `path`, as a store.
    fn file(&self, path: PathBuf) -> Box<dyn RawStore>;

    /// Where pitboard's own parked logins go: the system's store of secrets where there is
    /// one, a private directory of files where there is not. This one really is a fact
    /// about the machine.
    fn vault(&self, ctx: &Context) -> Box<dyn RawStore>;

    /// Whether every `PITBOARD_HOME` on this machine parks its logins in the one vault. A
    /// keychain belongs to the whole login session, so a park in it that one home cannot
    /// account for may be another home's; a vault of files lives inside its home, and
    /// nothing in it can be anybody else's.
    fn vault_is_shared(&self) -> bool;

    /// The processes this user is running `program` in, with where each runs from. `None`
    /// where the process list could not be read, which is not the same as none running.
    ///
    /// What is compared is the program's own name, so a script or a shell that merely
    /// mentions the program is not counted. Only this user's: another user's sessions use
    /// another user's login, which a switch here never touches.
    fn processes(&self, program: &str) -> Option<Vec<Process>>;

    /// The system's own scheduler, which runs daily renewal. `None` where there is none
    /// pitboard knows how to ask.
    fn scheduler(&self) -> Option<&dyn Scheduler>;
}

/// The system's own scheduler, which starts `pitboard renew` once a day: launchd on macOS,
/// a systemd user timer on Linux. Never a homemade daemon.
///
/// What pitboard decides about the schedule, which program it runs, how often, and whether
/// it is this home's to change, is [`crate::schedule`]'s. This is only how the system is
/// asked, and what it says back.
pub(crate) trait Scheduler: Send + Sync + std::fmt::Debug {
    /// Where the schedule is kept, where a person would look for it.
    fn location(&self, ctx: &Context) -> PathBuf;

    /// Whether a schedule is there.
    fn installed(&self, ctx: &Context) -> bool;

    /// The pitboard the schedule runs, read back from what [`Scheduler::put`] wrote. `None`
    /// where nothing is installed, and where what is there does not name one the way `put`
    /// writes it.
    fn program(&self, ctx: &Context) -> Option<PathBuf>;

    /// Schedule `program renew`, and ask the system to start it.
    ///
    /// Where the system will not start it, what was there goes back as it was, and is
    /// started again. The status, doctor and the app all read what is there, so a schedule
    /// left written that nothing runs would say renewal is on while it is not, which is the
    /// failure nobody would notice until the parked logins had run out.
    fn put(&self, ctx: &Context, program: &Path) -> Result<()>;

    /// Stop the schedule and take it away. `false` when nothing was there.
    fn remove(&self, ctx: &Context) -> Result<bool>;

    /// Whether this process is a run the schedule itself started. `said` is the job a test
    /// says this process runs as; `None` leaves it to what the system says.
    fn started_this_process(&self, said: Option<&str>) -> bool;
}

/// The host this build is standing on, which is what every real context uses.
pub(crate) fn current() -> Arc<dyn Host> {
    os::host()
}

/// The path this program was started by, where that lasts longer than the file it runs.
pub(crate) fn current_program() -> std::io::Result<PathBuf> {
    os::current_program()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine without a store of secrets must say so rather than hand back something
    /// that behaves like one. A tool's own module builds its chain out of this answer, so a
    /// host that always offered one would build a chain that cannot work.
    #[test]
    fn a_store_of_secrets_is_offered_only_where_there_is_one() {
        let ctx = Context::from_env();
        let offered = ctx.host().foreign_secrets(&ctx, "someone");
        assert_eq!(
            offered.map(|k| k.kind()),
            match OS {
                Os::MacOs => Some(crate::store::Backend::Keychain),
                Os::Linux => None,
            }
        );
        assert_eq!(
            ctx.host().file(PathBuf::from("/nowhere/at/all")).kind(),
            crate::store::Backend::File
        );
    }

    /// Every system pitboard runs on has a scheduler of its own that pitboard writes for.
    #[test]
    fn this_system_has_a_scheduler() {
        assert!(current().scheduler().is_some());
    }
}
