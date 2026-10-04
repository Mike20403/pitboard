//! Processes, the POSIX way.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// Whether process `pid` may still be running. A pid that cannot be probed is taken as
/// running: the callers only ever leave alone what a live process may still be using.
pub(crate) fn may_be_running(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return true;
    };
    // SAFETY: signal 0 is never delivered; `kill` only reports whether `pid` exists and may
    // be signalled, and touches no memory of this process.
    let probed = unsafe { libc::kill(pid, 0) };
    probed == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Whether `path` is a file somebody may run, as `execvp` judges it: a file, not a
/// directory, with an execute bit.
pub(crate) fn can_run(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|found| found.is_file() && found.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_running_and_one_that_exited_is_not() {
        assert!(may_be_running(std::process::id()));
        let exited = {
            let mut child = std::process::Command::new("true").spawn().unwrap();
            let pid = child.id();
            child.wait().unwrap();
            pid
        };
        assert!(!may_be_running(exited));
    }
}
