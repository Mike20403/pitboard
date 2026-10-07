//! The probe's command line, shared by every program the crate builds. Each subcommand is
//! one measurement a VM session or the runner-facts step needs; the hidden ones are the
//! children a measurement starts. On a system that is not Windows every subcommand prints
//! that the probe is for Windows and exits non-zero without touching anything.

use crate::report::Report;
use clap::{Parser, Subcommand, ValueEnum};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

/// Measure the Windows facts Pitboard for Windows is built on. A disposable machine and a
/// throwaway account only: never an account that holds a real login.
#[derive(Parser, Debug)]
#[command(name = "pitboard-probe", version, disable_help_subcommand = true)]
pub struct Cli {
    /// Also write the report to this file. It must lie in a folder the write guard allows,
    /// in a marked account. A child started with no console, a task's action, or a process
    /// started through runas has no other way to hand its report back.
    #[arg(long, global = true)]
    pub out: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// The logon session this process runs in, by type.
    Logon,
    /// A1: the token's elevation, type, integrity and default owner; with --scratch, the
    /// owner of a file it creates there.
    Tokens {
        #[arg(long)]
        scratch: Option<PathBuf>,
    },
    /// A1, runner facts: a Safer normal-user token computed from this one, read; with
    /// --spawn, a probe started under it reports its own token.
    Safer {
        #[arg(long)]
        spawn: bool,
        #[arg(long)]
        scratch: Option<PathBuf>,
    },
    /// A2, B1-B3: the build, every home a tool or the standard library may use, the profile
    /// type, the account name's facts, and the strings each tool may hash a home from.
    Homes {
        /// A Codex home to give the hash candidates of (B2, H4).
        #[arg(long)]
        codex_home: Option<PathBuf>,
        /// A Claude Code config folder to give the hash candidates of (B2, G4).
        #[arg(long)]
        claude_config_dir: Option<PathBuf>,
    },
    /// B4: which spellings of the search-path variable this process got, and what a child
    /// gets when its parent sets both PATH and Path.
    PathVars {
        #[arg(long)]
        scratch: PathBuf,
    },
    #[command(hide = true)]
    EnvChild,
    /// C1/C2: the access of the login folders (in a marked account), by relation; a
    /// protected-DACL file made, kept, opened from another token, removed.
    Acl {
        #[arg(long)]
        scratch: Option<PathBuf>,
        /// Make pitboard-probe-protected under --scratch with a protected DACL.
        #[arg(long)]
        create: bool,
        /// Leave the file made by --create for another token to --open.
        #[arg(long)]
        keep: bool,
        /// Try to read, write and delete-open this probe file under --scratch.
        #[arg(long)]
        open: Option<PathBuf>,
        /// Delete this probe file under --scratch.
        #[arg(long)]
        remove: Option<PathBuf>,
    },
    /// C3: the volume's file system, POSIX flag and both rename routes, each with the target
    /// held open with and without FILE_SHARE_DELETE.
    Volume {
        #[arg(long)]
        scratch: PathBuf,
    },
    /// C4: replace a file many times while readers open and read it by path, recording every
    /// error and every torn or missing read.
    ReplaceLoop {
        #[arg(long)]
        scratch: PathBuf,
        #[arg(long, default_value_t = 1000)]
        rounds: u32,
        /// Seconds to wait after seeding the target, for helpers to start against it.
        #[arg(long, default_value_t = 0)]
        lead_seconds: u64,
        /// The share mode of the reader this process runs beside the loop.
        #[arg(long, value_enum, default_value_t = ReaderShare::Delete)]
        reader: ReaderShare,
    },
    /// C4: a reader in a process of its own against the replace loop's target.
    ReplaceReader {
        #[arg(long)]
        scratch: PathBuf,
        #[arg(long, default_value_t = 60)]
        seconds: u64,
        #[arg(long, value_enum, default_value_t = ReaderShare::Delete)]
        share: ReaderShare,
    },
    /// C5: flush a directory handle opened two ways, and the file system's mtime precision.
    FlushDir {
        #[arg(long)]
        scratch: PathBuf,
    },
    /// C5: proper-lockfile's directory lock, held with its heartbeat or checked, for the Bun
    /// helper to run against from the other side.
    Lock {
        #[arg(long)]
        scratch: PathBuf,
        #[arg(long, value_enum, default_value_t = LockMode::Hold)]
        mode: LockMode,
        #[arg(long, default_value_t = 30)]
        seconds: u64,
    },
    /// C6: LockFileEx on a home's state.lock: held, read from another process, or the home
    /// removed while another process holds it.
    Lockfileex {
        #[arg(long)]
        scratch: PathBuf,
        #[arg(long, value_enum, default_value_t = LfxMode::Hold)]
        mode: LfxMode,
        #[arg(long, default_value_t = 20)]
        seconds: u64,
        #[arg(long, value_enum, default_value_t = ReaderShare::Delete)]
        share: ReaderShare,
    },
    /// D1-D3: seal and unseal a dummy secret with the vault's flags and entropy. seal keeps
    /// the sealed bytes in a scratch file, so unseal can open them after a password change,
    /// from a task or from another token.
    Dpapi {
        #[arg(long)]
        scratch: PathBuf,
        #[arg(long, value_enum, default_value_t = DpapiAction::RoundTrip)]
        action: DpapiAction,
        /// The sealed file's name under --scratch, for seal and unseal.
        #[arg(long, default_value = "pitboard-probe-sealed.bin")]
        file: String,
    },
    /// E1-E4: register, run, read, list or delete a probe task through ITaskService.
    Task {
        #[arg(long, value_enum, default_value_t = TaskAction::List)]
        action: TaskAction,
        #[arg(long)]
        scratch: Option<PathBuf>,
        /// The task's label: it is named pitboard-probe-<label>.
        #[arg(long, default_value = "e1")]
        label: String,
        #[arg(long, value_enum, default_value_t = TaskFolder::Probe)]
        folder: TaskFolder,
        /// Append the account's SID to the name, as W25 might.
        #[arg(long)]
        sid_suffix: bool,
        /// Which of the probe's programs the action runs.
        #[arg(long, value_enum, default_value_t = TaskExec::Probe)]
        exec: TaskExec,
        /// The action's arguments.
        #[arg(long, allow_hyphen_values = true)]
        args: Option<String>,
        /// A one-time trigger this many minutes from now.
        #[arg(long)]
        at_minutes: Option<u32>,
        #[arg(long)]
        start_when_available: bool,
        /// Let it start and keep running on battery.
        #[arg(long)]
        allow_battery: bool,
    },
    /// E4: whether each console variant gets a console and a window, started each way.
    Console {
        #[arg(long)]
        scratch: PathBuf,
        /// Also start each variant as a task's action (a marked account only).
        #[arg(long)]
        with_task: bool,
    },
    #[command(hide = true)]
    ConsoleChild,
    #[command(hide = true)]
    ConsoleLaunch {
        #[arg(long)]
        exe: PathBuf,
        #[arg(long)]
        flags: u32,
        #[arg(long)]
        child_out: PathBuf,
    },
    /// F1: assign a spawn chain to a Job Object right after starting it, over many rounds,
    /// and count the grandchildren that escaped.
    Job {
        #[arg(long)]
        scratch: PathBuf,
        #[arg(long, value_enum, default_value_t = Chain::Exe)]
        chain: Chain,
        #[arg(long, default_value_t = 100)]
        rounds: u32,
    },
    /// F2: open a local stand-in page from a process in a Job Object, then end the job one
    /// way, and see whether the browser was in it and survived.
    JobBrowser {
        #[arg(long)]
        scratch: PathBuf,
        #[arg(long, value_enum, default_value_t = JobEnd::TerminateJob)]
        end: JobEnd,
        #[arg(long, default_value_t = 8)]
        wait_seconds: u64,
    },
    #[command(hide = true)]
    JobLeaf {
        #[arg(long)]
        job_name: String,
    },
    #[command(hide = true)]
    JobMid {
        #[arg(long)]
        job_name: String,
    },
    #[command(hide = true)]
    JobOpenPage {
        #[arg(long)]
        page: PathBuf,
        #[arg(long)]
        job_name: String,
        #[arg(long, default_value_t = 0)]
        linger_seconds: u64,
    },
    /// H1/I1: the named tools' processes through WTS (any session) and Toolhelp: image path,
    /// session, package family, parent, and whether each runs as this user.
    Processes {
        /// An image to report, such as codex.exe: a tool's, a runtime's, Pitboard's or the
        /// probe's. Repeatable; required, since nothing else is ever named.
        #[arg(long, required = true)]
        find: Vec<String>,
    },
    /// G2-G4, H4, M: the live login families' and pitboard-* items in Credential Manager.
    /// Names only, but in a marked account their attributes too; never a blob.
    CredmanNames {
        /// Fail if a live family or a pitboard-citest-* name is present, or a family could
        /// not be read.
        #[arg(long)]
        leak_check: bool,
        /// A home folder to test each target's hash suffix against (repeatable).
        #[arg(long)]
        config_dir: Vec<PathBuf>,
    },
    /// M: write, read or delete one pitboard-probe-* Credential Manager item.
    CredmanItem {
        #[arg(long, value_enum)]
        action: ItemAction,
        #[arg(long, default_value = "pitboard-probe-item")]
        name: String,
        #[arg(long, value_enum, default_value_t = Persist::LocalMachine)]
        persist: Persist,
        #[arg(long, default_value_t = 64)]
        blob_bytes: u32,
    },
    /// R1: whether a program takes its C runtime from a DLL, from its import table. The
    /// probe itself, or a program copied under --scratch.
    PeImports {
        #[arg(long)]
        scratch: Option<PathBuf>,
        #[arg(long)]
        exe: Option<PathBuf>,
    },
    /// K1: package identity, and writes, renames and reads under %LOCALAPPDATA%\Pitboard and
    /// HKCU from this process and its children.
    Sparse {
        #[arg(long, value_enum, default_value_t = SparseAction::Identity)]
        action: SparseAction,
        #[arg(long, default_value = "self")]
        tag: String,
        #[arg(long)]
        scratch: Option<PathBuf>,
    },
    /// F3: which stand-in std's Command starts for a bare name, in each arrangement.
    ExeLookup {
        #[arg(long)]
        scratch: PathBuf,
        #[arg(long, default_value = "pitboard-probe-lookup")]
        name: String,
    },
    #[command(hide = true)]
    ExeLookupHost {
        #[arg(long)]
        name: String,
        #[arg(long)]
        child_path: Option<PathBuf>,
    },
    #[command(hide = true)]
    ExeLookupWhoami,
    /// G6: replace a file in a scratch folder with another one there, the way W16 will.
    Swap {
        #[arg(long)]
        scratch: PathBuf,
        /// The file to replace, by name, in --scratch.
        #[arg(long)]
        target: String,
        /// The file whose bytes replace it, by name, in --scratch. Left in place.
        #[arg(long)]
        source: String,
        #[arg(long, value_enum, default_value_t = Route::Movefile)]
        route: Route,
    },
    /// G8: run a program with no window and piped stdio, passing typed lines on with the
    /// chosen line ending. Refuses unless CLAUDE_CONFIG_DIR and CODEX_HOME are scratch
    /// folders.
    Windowless {
        #[arg(long)]
        scratch: PathBuf,
        #[arg(long, value_enum, default_value_t = Newline::Crlf)]
        newline: Newline,
        #[arg(last = true, required = true)]
        program: Vec<String>,
    },
    /// G9: daemon.lock's fields against the process it names, the cc-daemon pipes, and
    /// pipe.key's size, in a scratch Claude Code config folder.
    Daemon {
        #[arg(long)]
        config_dir: PathBuf,
        #[arg(long)]
        pipe_key: Option<PathBuf>,
    },
    /// Runner facts: whether this account can make a file symbolic link under --scratch.
    Symlink {
        #[arg(long)]
        scratch: PathBuf,
    },
    /// The facts every Windows CI leg reports about its runner, as one report.
    RunnerFacts {
        #[arg(long)]
        scratch: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ReaderShare {
    /// FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, as libuv opens a file.
    Delete,
    /// FILE_SHARE_READ | FILE_SHARE_WRITE.
    NoDelete,
    /// No reader.
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum LockMode {
    Hold,
    Check,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum LfxMode {
    Hold,
    Read,
    RemoveHome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum DpapiAction {
    RoundTrip,
    Seal,
    Unseal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum TaskAction {
    Register,
    Run,
    Read,
    List,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum TaskFolder {
    /// \pitboard-probe\, the probe's own folder.
    Probe,
    /// \Pitboard\, W25's candidate folder.
    Pitboard,
    /// The root folder.
    Root,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum TaskExec {
    Probe,
    Detached,
    DetachedAsmv1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Chain {
    /// probe, then probe, then the probe leaf.
    Exe,
    /// cmd.exe running a .cmd, then node.exe, then the probe leaf, as an npm shim does.
    CmdNodeExe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum JobEnd {
    TerminateJob,
    /// Terminate only the job's probe processes, as an allowlist would.
    Allowlist,
    /// Let the process that opened the page exit, and close the job.
    ParentExit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ItemAction {
    Write,
    Read,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Persist {
    Session,
    LocalMachine,
    Enterprise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum SparseAction {
    /// Whether this process has package identity.
    Identity,
    /// Write, rename and read back a file and an HKCU value tagged --tag.
    Write,
    /// From an identity process: start the plain probe and a stand-in claude.exe, each
    /// writing with a tag of its own.
    Children,
    /// Read back every tag, from wherever this process runs.
    Read,
    /// Remove everything the block made.
    Clean,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Route {
    /// MoveFileExW with MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH.
    Movefile,
    /// SetFileInformationByHandle with FileRenameInfoEx and POSIX semantics.
    Posix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Newline {
    Lf,
    Crlf,
}

/// What a run hands back.
pub enum Outcome {
    /// A report, printed and, with --out, written.
    Report(Report),
    /// An exit code and nothing printed: a helper whose answer is its code.
    Code(u8),
    /// One line printed: a helper whose answer is that line.
    Line(String),
}

/// The probe's `main`, for every program the crate builds.
pub fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Outcome::Report(report) => {
            let text = report.render();
            let mut failed = report.failed();
            if let Some(out) = &cli.out
                && let Err(reason) = write_out(out, &text)
            {
                let _ = writeln!(std::io::stderr(), "pitboard-probe: --out: {reason}");
                failed = true;
            }
            // A child started with no console may have no standard output at all.
            let _ = writeln!(std::io::stdout().lock(), "{text}");
            if failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Outcome::Code(code) => ExitCode::from(code),
        Outcome::Line(line) => {
            let _ = writeln!(std::io::stdout().lock(), "{line}");
            ExitCode::SUCCESS
        }
    }
}

#[cfg(windows)]
fn run(cli: &Cli) -> Outcome {
    crate::win::run(&cli.command, cli.out.as_deref())
}

#[cfg(windows)]
fn write_out(out: &std::path::Path, text: &str) -> Result<(), String> {
    crate::win::write_out(out, text)
}

#[cfg(not(windows))]
fn run(cli: &Cli) -> Outcome {
    Outcome::Report(Report::refused(
        block_name(&cli.command),
        crate::logon::LogonSession::unknown(),
        "pitboard-probe measures Windows facts and runs on Windows only; this is another \
         system",
    ))
}

#[cfg(not(windows))]
fn write_out(_out: &std::path::Path, _text: &str) -> Result<(), String> {
    Err("the probe writes nothing on a system that is not Windows".into())
}

/// The block a command reports as.
pub fn block_name(command: &Command) -> &'static str {
    match command {
        Command::Logon => "logon",
        Command::Tokens { .. } => "tokens",
        Command::Safer { .. } => "safer",
        Command::Homes { .. } => "homes",
        Command::PathVars { .. } => "path-vars",
        Command::EnvChild => "env-child",
        Command::Acl { .. } => "acl",
        Command::Volume { .. } => "volume",
        Command::ReplaceLoop { .. } => "replace-loop",
        Command::ReplaceReader { .. } => "replace-reader",
        Command::FlushDir { .. } => "flush-dir",
        Command::Lock { .. } => "lock",
        Command::Lockfileex { .. } => "lockfileex",
        Command::Dpapi { .. } => "dpapi",
        Command::Task { .. } => "task",
        Command::Console { .. } => "console",
        Command::ConsoleChild => "console-child",
        Command::ConsoleLaunch { .. } => "console-launch",
        Command::Job { .. } => "job",
        Command::JobBrowser { .. } => "job-browser",
        Command::JobLeaf { .. } => "job-leaf",
        Command::JobMid { .. } => "job-mid",
        Command::JobOpenPage { .. } => "job-open-page",
        Command::Processes { .. } => "processes",
        Command::CredmanNames { .. } => "credman-names",
        Command::CredmanItem { .. } => "credman-item",
        Command::PeImports { .. } => "pe-imports",
        Command::Sparse { .. } => "sparse",
        Command::ExeLookup { .. } => "exe-lookup",
        Command::ExeLookupHost { .. } => "exe-lookup-host",
        Command::ExeLookupWhoami => "exe-lookup-whoami",
        Command::Swap { .. } => "swap",
        Command::Windowless { .. } => "windowless",
        Command::Daemon { .. } => "daemon",
        Command::Symlink { .. } => "symlink",
        Command::RunnerFacts { .. } => "runner-facts",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn out_is_global_and_processes_needs_a_name() {
        let cli = Cli::try_parse_from(["pitboard-probe", "logon", "--out", "x.json"]).unwrap();
        assert_eq!(cli.out, Some(PathBuf::from("x.json")));
        assert!(Cli::try_parse_from(["pitboard-probe", "processes"]).is_err());
        let cli =
            Cli::try_parse_from(["pitboard-probe", "processes", "--find", "codex.exe"]).unwrap();
        assert!(matches!(cli.command, Command::Processes { ref find } if find == &["codex.exe"]));
    }

    #[test]
    fn windowless_takes_the_program_after_a_double_dash() {
        let cli = Cli::try_parse_from([
            "pitboard-probe",
            "windowless",
            "--scratch",
            r"C:\s",
            "--",
            "claude",
            "auth",
            "login",
        ])
        .unwrap();
        match cli.command {
            Command::Windowless { program, .. } => assert_eq!(program, ["claude", "auth", "login"]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn task_arguments_may_begin_with_a_dash() {
        let cli = Cli::try_parse_from([
            "pitboard-probe",
            "task",
            "--action",
            "register",
            "--args",
            "--out x dpapi",
        ])
        .unwrap();
        assert!(
            matches!(cli.command, Command::Task { args: Some(ref a), .. } if a == "--out x dpapi")
        );
    }

    #[test]
    fn every_command_has_a_block_name() {
        let cli = Cli::try_parse_from(["pitboard-probe", "symlink", "--scratch", "x"]).unwrap();
        assert_eq!(block_name(&cli.command), "symlink");
    }

    #[cfg(not(windows))]
    #[test]
    fn off_windows_every_command_refuses() {
        let cli = Cli::try_parse_from(["pitboard-probe", "tokens"]).unwrap();
        match run(&cli) {
            Outcome::Report(r) => {
                assert!(r.failed());
                assert_eq!(r.to_json()["block"], "tokens");
            }
            _ => panic!("a report"),
        }
        assert!(write_out(std::path::Path::new("x"), "y").is_err());
    }
}
