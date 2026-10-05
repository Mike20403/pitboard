//! Processes, the POSIX way.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
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

/// Whether `path` is a program this user may run, as `execvp` judges it: a regular file,
/// once every link is followed, that `access` lets this user execute. An execute bit for
/// somebody else is not enough, and `access` alone would take a directory, which this user
/// may search, for one.
pub(crate) fn can_run(path: &Path) -> bool {
    let Ok(name) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `name` is a NUL-terminated string that outlives the call, and `access` only
    // reads it.
    let executable = unsafe { libc::access(name.as_ptr(), libc::X_OK) } == 0;
    executable && std::fs::metadata(path).is_ok_and(|found| found.is_file())
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

    /// A program is what `execvp` would start: a regular file, once every link is followed,
    /// that this user may execute. One only its group and others may execute is not this
    /// user's to run, however many execute bits it has; nor is a directory, which `access`
    /// would let this user search.
    #[test]
    fn a_program_is_a_regular_file_this_user_may_execute() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("pitboard-can-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let made = |name: &str, mode: u32| {
            let path = dir.join(name);
            std::fs::write(&path, "#!/bin/sh\n").expect("a file");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
                .expect("its mode");
            path
        };
        let everyone = made("everyone", 0o755);
        let mine = made("mine", 0o700);
        let theirs = made("theirs", 0o611);
        let plain = made("plain", 0o644);
        let folder = dir.join("folder");
        std::fs::create_dir(&folder).expect("a directory");
        let link = dir.join("link");
        std::os::unix::fs::symlink(&mine, &link).expect("a link");
        let dangling = dir.join("dangling");
        std::os::unix::fs::symlink(dir.join("gone"), &dangling).expect("a link");

        assert!(can_run(&everyone));
        assert!(can_run(&mine));
        assert!(can_run(&link), "a link to a program is that program");
        assert!(!can_run(&plain));
        assert!(!can_run(&folder), "a directory is not a program");
        assert!(!can_run(&dangling));
        assert!(!can_run(&dir.join("absent")));
        // SAFETY: geteuid only reports this process's effective user id.
        if unsafe { libc::geteuid() } != 0 {
            assert!(
                !can_run(&theirs),
                "only its group and others may execute it"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
