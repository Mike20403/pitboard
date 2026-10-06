//! Files only their owner can reach, the POSIX way: a mode, set when the file is created so
//! it is never open for a moment, whatever the umask.
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

/// `options`, made to create a file only its owner can read and write: 0600. A file that is
/// already there keeps its own mode.
pub(crate) fn private(_: Permit, options: &mut OpenOptions) -> &mut OpenOptions {
    options.mode(0o600)
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

    #[test]
    fn a_file_created_private_is_private_however_the_umask_is_set() {
        let dir = scratch("file");
        let permit = Permit::for_a_test();
        create_private_dir(permit, &dir).unwrap();
        let path = dir.join("state.lock");
        private(permit, OpenOptions::new().write(true).create(true))
            .open(&path)
            .unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(
            access(&path),
            Some(Access {
                shared: false,
                described: "mode 600".into()
            })
        );
        testing::open_to_others(&path);
        assert!(access(&path).unwrap().shared);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
