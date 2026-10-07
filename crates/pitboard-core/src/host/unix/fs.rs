//! Files only their owner can reach, the POSIX way: a mode, set when the file is created so
//! it is never open for a moment, whatever the umask. The face creates each such file itself
//! and hands it back open, rather than setting an option on a file its caller then opens, so
//! how a file is made private is said here and nowhere else, and a face can refuse to make
//! one.
//!
//! Every way Pitboard changes the disk outside [`crate::atomic::write`] is here too, and
//! each takes the [`Permit`] only the one gate every change passes makes: creating a file
//! or a directory, giving one an access or a time, and moving, copying or removing one. So
//! nothing removes or makes a file without having asked whether this process may.

use crate::host::Access;
use crate::service::Permit;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::time::SystemTime;

/// Create `path` and any missing parent so only the owner can reach them: 0700. A directory
/// that already exists keeps its mode: it may be one the person named, and `pitboard
/// doctor` reports it if others can read it.
pub(crate) fn create_private_dir(_: Permit, path: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

/// A new file at `path`, open for writing, that only its owner can read or write: 0600.
/// Fails where anything is there already, a link included, so nothing planted there is
/// written through.
pub(crate) fn create_private(_: Permit, path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// The file at `path`, open to add to its end, made where it is not there so only its owner
/// can read or write it: 0600. A file already there keeps its own mode.
pub(crate) fn open_private_append(_: Permit, path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}

/// The file at `path`, open for writing and left as it is, made where it is not there so
/// only its owner can read or write it: 0600. What a lock is taken on: nothing is written to
/// it, and a file already there keeps its own mode.
pub(crate) fn open_private_lock(_: Permit, path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(path)
}

/// Give `temp` exactly the access the file at `existing` has, tighter or looser than
/// private, so replacing another program's file neither opens nor closes it. Nothing
/// changes where nothing is there, and where `existing` is a link: a link's own mode says
/// nothing about its target, and the rename that follows replaces the link rather than
/// following it, as Claude Code's own writes do.
pub(crate) fn copy_access(_: Permit, existing: &Path, temp: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(existing) {
        Ok(found) if !found.file_type().is_symlink() => {
            let mode = found.permissions().mode() & 0o777;
            std::fs::set_permissions(temp, std::fs::Permissions::from_mode(mode))
        }
        _ => Ok(()),
    }
}

/// Make a rename in `dir` durable. ext4 and xfs can lose a rename across a crash even when
/// the file's contents were synced. Best effort: the contents are durable already.
pub(crate) fn sync_dir(dir: &Path) {
    let _ = File::open(dir).and_then(|d| d.sync_all());
}

/// Set the modification time of the directory at `path` to `at`.
pub(crate) fn touch_dir(_: Permit, path: &Path, at: SystemTime) -> io::Result<()> {
    File::open(path)?.set_modified(at)
}

/// Make the directory `path`, and any missing parent, with the access the umask gives: for a
/// directory another program keeps, which it made its own way.
pub(crate) fn create_dir_all(_: Permit, path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Make the one directory `path`, which fails where something is there already: how a lock
/// made of a directory is taken.
pub(crate) fn create_dir(_: Permit, path: &Path) -> io::Result<()> {
    std::fs::create_dir(path)
}

/// Copy the file at `from` to `to`.
pub(crate) fn copy(_: Permit, from: &Path, to: &Path) -> io::Result<u64> {
    std::fs::copy(from, to)
}

/// Move the file at `from` to `to`, over whatever is there.
pub(crate) fn rename(_: Permit, from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// Remove the file at `path`, or the link there, never what a link leads to.
pub(crate) fn remove_file(_: Permit, path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// Remove the empty directory at `path`.
pub(crate) fn remove_dir(_: Permit, path: &Path) -> io::Result<()> {
    std::fs::remove_dir(path)
}

/// Remove the directory at `path` and everything in it.
pub(crate) fn remove_dir_all(_: Permit, path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}

/// Who besides its owner can reach `path`. `None` where it cannot be looked at.
pub fn access(path: &Path) -> Option<Access> {
    let mode = std::fs::metadata(path).ok()?.permissions().mode() & 0o777;
    Some(Access {
        // Group and other, read, write or search. Anything there is somebody who is not
        // the owner.
        shared: mode & 0o077 != 0,
        described: format!("mode {mode:o}"),
    })
}

/// What a test does to a file's access to set up a machine, the way a person or another
/// program might have left it.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    fn set(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .unwrap_or_else(|e| panic!("{} could not be changed: {e}", path.display()));
    }

    /// A file anybody may read, as a copy or a careless umask leaves one.
    pub(crate) fn open_to_others(path: &Path) {
        set(path, 0o644);
    }

    /// A file only its owner may read or write.
    pub(crate) fn make_private(path: &Path) {
        set(path, 0o600);
    }

    /// A file its owner may read and nobody may change.
    pub(crate) fn read_only_for_owner(path: &Path) {
        set(path, 0o400);
    }

    /// A file anybody may run.
    pub(crate) fn make_runnable(path: &Path) {
        set(path, 0o755);
    }

    /// A directory nothing can be added to or removed from.
    pub(crate) fn deny_changes(dir: &Path) {
        set(dir, 0o555);
    }

    /// A directory its owner may change again.
    pub(crate) fn allow_changes(dir: &Path) {
        set(dir, 0o755);
    }

    /// A link at `link` that leads to `target`.
    pub(crate) fn link(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link)
            .unwrap_or_else(|e| panic!("{} could not be linked: {e}", link.display()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pitboard-host-fs-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn mode(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn created_directories_are_private_and_existing_ones_keep_their_mode() {
        let scratch = scratch("dirs");
        let permit = Permit::for_a_test();
        create_private_dir(permit, &scratch.join("nested")).unwrap();
        assert_eq!(
            mode(&scratch),
            0o700,
            "a created parent must be private too"
        );
        assert_eq!(mode(&scratch.join("nested")), 0o700);

        std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o755)).unwrap();
        create_private_dir(permit, &scratch).unwrap();
        assert_eq!(
            mode(&scratch),
            0o755,
            "a directory the user named is theirs to set"
        );
        std::fs::remove_dir_all(&scratch).unwrap();
    }

    /// Each way the face makes a file makes it private. One to add to or to lock that is
    /// there already is opened as it is, mode and contents, and a new one is refused where
    /// anything is there, a link included.
    #[test]
    fn a_file_created_private_is_private_however_the_umask_is_set() {
        use std::io::Write;
        let dir = scratch("file");
        let permit = Permit::for_a_test();
        create_private_dir(permit, &dir).unwrap();
        let (fresh, history, lock) = (
            dir.join(".state.json.1.0.pitboard"),
            dir.join("history.jsonl"),
            dir.join("state.lock"),
        );
        create_private(permit, &fresh).unwrap();
        open_private_append(permit, &history).unwrap();
        open_private_lock(permit, &lock).unwrap();
        for made in [&fresh, &history, &lock] {
            assert_eq!(
                access(made),
                Some(Access {
                    shared: false,
                    described: "mode 600".into()
                }),
                "{}",
                made.display()
            );
        }

        for there in [&history, &lock] {
            std::fs::write(there, "one\n").unwrap();
            testing::open_to_others(there);
            assert!(access(there).unwrap().shared);
        }
        writeln!(open_private_append(permit, &history).unwrap(), "two").unwrap();
        open_private_lock(permit, &lock).unwrap();
        assert_eq!(std::fs::read_to_string(&history).unwrap(), "one\ntwo\n");
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), "one\n");
        for there in [&history, &lock] {
            assert_eq!(
                mode(there),
                0o644,
                "a file already there keeps its own mode"
            );
        }

        assert!(create_private(permit, &fresh).is_err(), "a file is there");
        let planted = dir.join(".usage.json.1.0.pitboard");
        testing::link(&dir.join("elsewhere"), &planted);
        assert!(create_private(permit, &planted).is_err(), "a link is there");
        assert!(
            !dir.join("elsewhere").exists(),
            "and nothing is written through it"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
