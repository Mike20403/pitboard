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

/// What a test or a fixture does to the disk to set up a machine the way a person, an
/// installer or another program might have left it, and what it reads back of a file's
/// access. Nothing outside a host's face sets a file's mode, makes a file runnable or makes a
/// link but through here, so a system whose files have no mode, or that makes a link to a
/// directory otherwise than one to a file, says how in its own face's `testing` and nowhere
/// else. Other crates reach it as `pitboard_core::testing::fs`.
///
/// Each says what went wrong rather than panicking, since a fixture is made in an app as well
/// as in a test, and an app that cannot make one says so.
#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    use std::io;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn set(path: &Path, mode: u32) -> io::Result<()> {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }

    /// A file anybody may read, as a copy or a careless umask leaves one: 644.
    pub fn open_to_others(path: &Path) -> io::Result<()> {
        set(path, 0o644)
    }

    /// A file or a directory only its owner may reach, as Pitboard makes its own: 600 for a
    /// file and 700 for a directory.
    pub fn make_private(path: &Path) -> io::Result<()> {
        set(path, private_mode(path)?)
    }

    /// Whether `path` is private as Pitboard makes its own files and directories: exactly
    /// what [`make_private`] gives it, so its owner may still read and change it.
    pub fn is_private(path: &Path) -> io::Result<bool> {
        let mode = std::fs::metadata(path)?.permissions().mode() & 0o777;
        Ok(mode == private_mode(path)?)
    }

    fn private_mode(path: &Path) -> io::Result<u32> {
        Ok(if std::fs::metadata(path)?.is_dir() {
            0o700
        } else {
            0o600
        })
    }

    /// A file its owner may read and nobody may change: 400.
    pub fn read_only_for_owner(path: &Path) -> io::Result<()> {
        set(path, 0o400)
    }

    /// A file nobody may read or change, its owner included: 000. The system's administrator
    /// still may, so a test that needs it unread checks that it is.
    pub fn deny_reading(path: &Path) -> io::Result<()> {
        set(path, 0o000)
    }

    /// A file anybody may run: 755.
    pub fn make_runnable(path: &Path) -> io::Result<()> {
        set(path, 0o755)
    }

    /// A file nobody may run, which anybody may still read: 644, as a copy that lost a
    /// program's modes leaves one.
    pub fn deny_running(path: &Path) -> io::Result<()> {
        set(path, 0o644)
    }

    /// A directory nothing can be added to or removed from: 555.
    pub fn deny_changes(dir: &Path) -> io::Result<()> {
        set(dir, 0o555)
    }

    /// A directory its owner may change again: 755.
    pub fn allow_changes(dir: &Path) -> io::Result<()> {
        set(dir, 0o755)
    }

    /// A link at `at` that leads to `target`, a file or nothing at all, as an installer or a
    /// person makes one. A relative `target` is read from the link's own directory. A link to
    /// a directory is [`link_dir`]'s.
    pub fn link(target: &Path, at: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, at)
    }

    /// A link at `at` that leads to the directory `target`. Apart from [`link`], since not
    /// every system makes a link to a directory the way it makes one to a file. On macOS and
    /// Linux both are the same symbolic link.
    pub fn link_dir(target: &Path, at: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, at)
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
            testing::open_to_others(there).unwrap();
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
        testing::link(&dir.join("elsewhere"), &planted).unwrap();
        assert!(create_private(permit, &planted).is_err(), "a link is there");
        assert!(
            !dir.join("elsewhere").exists(),
            "and nothing is written through it"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// What a test or a fixture sets up through `testing` is what each says, and a link leads
    /// where it was asked to.
    #[test]
    fn each_way_a_test_sets_a_file_up_does_what_it_says() {
        let dir = scratch("testing");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("file");
        std::fs::write(&file, "").unwrap();
        let gives = |set: fn(&Path) -> io::Result<()>, expected: u32| {
            set(&file).unwrap();
            assert_eq!(mode(&file), expected);
        };
        gives(testing::open_to_others, 0o644);
        gives(testing::make_private, 0o600);
        gives(testing::read_only_for_owner, 0o400);
        gives(testing::deny_reading, 0o000);
        gives(testing::make_runnable, 0o755);
        gives(testing::deny_running, 0o644);
        assert!(!testing::is_private(&file).unwrap());
        testing::make_private(&file).unwrap();
        assert!(testing::is_private(&file).unwrap());

        let folder = dir.join("folder");
        std::fs::create_dir(&folder).unwrap();
        testing::make_private(&folder).unwrap();
        assert_eq!(mode(&folder), 0o700, "a directory's own private mode");
        assert!(testing::is_private(&folder).unwrap());
        testing::deny_changes(&folder).unwrap();
        assert_eq!(mode(&folder), 0o555);
        testing::allow_changes(&folder).unwrap();
        assert_eq!(mode(&folder), 0o755);
        assert!(!testing::is_private(&folder).unwrap());

        testing::link(Path::new("file"), &dir.join("to-file")).unwrap();
        testing::link_dir(&folder, &dir.join("to-folder")).unwrap();
        assert_eq!(
            std::fs::read_link(dir.join("to-file")).unwrap(),
            Path::new("file")
        );
        assert!(
            dir.join("to-file").is_file(),
            "read from the link's own directory"
        );
        assert!(dir.join("to-folder").is_dir());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
