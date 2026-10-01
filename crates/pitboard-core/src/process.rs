//! Running a helper program with a deadline. A helper that never answers must not hold
//! pitboard's lock, or an app's worker, forever.

// Linux reads its process list from /proc and has no keychain to call, so there only the
// tests run a helper.
use std::path::PathBuf;
#[cfg(any(not(target_os = "linux"), test))]
use std::{
    io::{self, Read, Write},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

/// `command`'s output, with `input` on its stdin. Past `limit` the process is killed and the
/// answer is `TimedOut`. Its output is drained on other threads, so a chatty helper cannot
/// fill a pipe and stall.
#[cfg(any(not(target_os = "linux"), test))]
pub fn output_within(mut command: Command, input: &[u8], limit: Duration) -> io::Result<Output> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> thread::JoinHandle<Vec<u8>> {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            bytes
        })
    }
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    // Dropping stdin closes it, which is how a helper reading commands learns there are no
    // more. A helper that exits without reading is not an error.
    if let Some(mut stdin) = child.stdin.take() {
        match stdin.write_all(input) {
            Err(e) if e.kind() != io::ErrorKind::BrokenPipe => return Err(e),
            _ => {}
        }
    }

    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("no answer within {}s", limit.as_secs()),
            ));
        }
        thread::sleep(Duration::from_millis(5));
    };
    Ok(Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
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

/// Every process this user is running whose program is called `program`. `None` where the
/// process list could not be read, which is not the same as none running.
///
/// What is compared is the program's own name, so a script or a shell that merely mentions
/// the program is not counted. Only this user's: another user's sessions use another
/// user's login, which a switch here never touches.
#[cfg(target_os = "linux")]
pub fn processes(program: &str) -> Option<Vec<Process>> {
    use std::os::unix::fs::MetadataExt;
    // Linux says it in /proc, which every Linux has, where `/bin/ps` is not on every one:
    // a slim container or NixOS has none, and the warning this feeds would quietly vanish.
    let me = std::fs::metadata("/proc/self").ok()?.uid();
    let mut found: Vec<Process> = std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            if entry.metadata().ok()?.uid() != me {
                return None;
            }
            let name = std::fs::read_to_string(entry.path().join("comm")).ok()?;
            (name.trim() == program).then(|| Process {
                pid,
                path: std::fs::read_link(entry.path().join("exe"))
                    .unwrap_or_else(|_| PathBuf::from(program)),
            })
        })
        .collect();
    found.sort_unstable_by_key(|p| p.pid);
    Some(found)
}

#[cfg(not(target_os = "linux"))]
pub fn processes(program: &str) -> Option<Vec<Process>> {
    // `-x` with no other selection is every process this user owns, with or without a
    // terminal.
    let mut ps = Command::new("/bin/ps");
    ps.args(["-x", "-o", "pid=,comm="]);
    let out = output_within(ps, b"", Duration::from_secs(5)).ok()?;
    if !out.status.success() {
        return None;
    }
    Some(named(&String::from_utf8_lossy(&out.stdout), program))
}

/// The processes in a `ps -o pid=,comm=` listing whose program is called `program`. A path
/// can have spaces in it, so everything after the pid is the path.
#[cfg(any(not(target_os = "linux"), test))]
fn named(listing: &str, program: &str) -> Vec<Process> {
    listing
        .lines()
        .filter_map(|line| {
            let (pid, path) = line.trim_start().split_once(char::is_whitespace)?;
            let path = PathBuf::from(path.trim());
            (path.file_name()? == program).then_some(Process {
                pid: pid.parse().ok()?,
                path,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_is_found_by_its_own_name_and_nothing_else() {
        let listing = "  412 /Users/a/.codex/packages/standalone/bin/codex\n\
                       7031 codex\n\
                       7032 /bin/zsh\n\
                         88 /usr/bin/codex-helper\n\
                       9001 /Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex\n\
                       9002 /Users/a/Codex Things/bin/codex\n\
                       node\n";
        let found = named(listing, "codex");
        assert_eq!(
            found.iter().map(|p| p.pid).collect::<Vec<_>>(),
            [412, 7031, 9001, 9002]
        );
        assert_eq!(
            found[3].path,
            PathBuf::from("/Users/a/Codex Things/bin/codex")
        );
        assert!(named(listing, "gemini").is_empty());
    }

    /// The list is readable on every machine these tests run on, and this test's own
    /// process is in it, found by its own name, with where it runs from.
    #[test]
    fn this_users_processes_can_be_read() {
        assert_eq!(processes("definitely-not-a-program-name"), Some(Vec::new()));
        let me = std::env::current_exe().expect("this test's own binary");
        let name = me.file_name().unwrap().to_string_lossy().into_owned();
        // Linux keeps fifteen characters of a process's name, and test binaries are longer.
        let name: String = if cfg!(target_os = "linux") {
            name.chars().take(15).collect()
        } else {
            name
        };
        let found = processes(&name).expect("readable");
        let mine = found
            .iter()
            .find(|p| p.pid == std::process::id())
            .unwrap_or_else(|| panic!("{name} is running: {found:?}"));
        assert_eq!(mine.path.file_name(), me.file_name());
    }

    #[test]
    fn a_prompt_answer_is_returned_whole() {
        let mut cat = Command::new("cat");
        cat.arg("-");
        let out = output_within(cat, b"hello", Duration::from_secs(5)).unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout, b"hello");
    }

    #[test]
    fn a_helper_that_never_answers_is_stopped_at_the_deadline() {
        let mut sleep = Command::new("sleep");
        sleep.arg("30");
        let started = Instant::now();
        let err = output_within(sleep, b"", Duration::from_millis(200)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_large_answer_cannot_stall_the_helper() {
        let mut yes = Command::new("head");
        yes.args(["-c", "1000000", "/dev/zero"]);
        let out = output_within(yes, b"", Duration::from_secs(10)).unwrap();
        assert_eq!(out.stdout.len(), 1_000_000);
    }
}
