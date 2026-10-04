//! Finding a program somebody named, the way the system's own launcher finds it.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Where a program somebody named is: the path itself, made absolute, when it has a
/// directory in it, or the first file on `search`, a list in `PATH`'s form, that can be run.
///
/// Found the way `execvp` finds one, which passes over a directory of that name and a file
/// nobody may run, so what is found here is what starts. Only a directory named from the
/// root is looked in: a relative one names a place relative to wherever Pitboard was
/// started, which says nothing about where a tool is installed, and a sign-in that runs
/// from a directory of its own would read it as somewhere else again.
pub(crate) fn find(named: &Path, search: &OsStr) -> Option<PathBuf> {
    find_in(named, search, super::proc::can_run)
}

/// `find` with the question of whether a file can be run handed in, so a test can see every
/// place it looks.
fn find_in(
    named: &Path,
    search: &OsStr,
    mut runnable: impl FnMut(&Path) -> bool,
) -> Option<PathBuf> {
    if named.components().count() > 1 {
        let named = std::path::absolute(named).ok()?;
        return runnable(&named).then_some(named);
    }
    std::env::split_paths(search)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(named))
        .find(|candidate| runnable(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a directory named from the root is looked in. An empty entry and a relative one
    /// both name somewhere relative to wherever Pitboard was started, and a sign-in that
    /// runs from a directory of its own would start something else from there.
    #[test]
    fn only_a_directory_named_from_the_root_is_looked_in() {
        let mut looked = Vec::new();
        let root = std::env::temp_dir().join("bin");
        let search = std::env::join_paths([
            "".into(),
            "bin".into(),
            "./node_modules/.bin".into(),
            root.clone(),
        ])
        .expect("a search path");
        let found = find_in(Path::new("codex"), &search, |candidate| {
            looked.push(candidate.to_path_buf());
            false
        });
        assert_eq!(found, None);
        assert_eq!(looked, [root.join("codex")]);
    }

    /// A program named with a directory relative to where Pitboard was started is found as
    /// that place, by its full path, so the sign-in that runs from a directory of its own
    /// starts the same program.
    #[test]
    fn a_program_named_relative_to_here_is_found_by_its_full_path() {
        let found = find_in(Path::new("./bin/codex"), "".as_ref(), |_| true).expect("found");
        assert!(found.is_absolute(), "{}", found.display());
        assert_eq!(
            found,
            std::env::current_dir()
                .expect("a working directory")
                .join("bin/codex")
        );
    }
}
