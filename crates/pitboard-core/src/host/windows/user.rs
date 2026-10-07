//! The person signed in to Windows, before this face is written.

use crate::host::Elevation;
use std::path::PathBuf;

/// The person's account name, as Claude Code reads it where `USER` is unset: not read until
/// W22, which names Claude Code's store on Windows by it.
pub(crate) fn login_name() -> Option<String> {
    None
}

/// This account's own home, where the environment names none: not read until W14 asks
/// Windows for the profile folder. Without one, Pitboard refuses the empty home as one that
/// is not a full path.
pub(crate) fn home() -> Option<PathBuf> {
    None
}

/// Whether `path` is this account's own home, for a build for tests to refuse to run as the
/// daily renewal schedule there. Nobody can tell until W14 reads the profile folder, and
/// nothing needs to: such a run renews Pitboard's default home, which on Windows is none
/// until W14 finds it too, and is refused as a home that is not a full path
/// ([`crate::host::default_pitboard_home`]).
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn is_the_accounts_own_home(_path: &std::path::Path) -> bool {
    false
}

/// Whether this process runs as the person themselves: nobody can tell until W12 reads its
/// token, whatever `sudo` says, and the one gate every change passes refuses what nobody can
/// tell.
pub(crate) fn elevation(_sudo: bool) -> Elevation {
    Elevation::Unknown
}

/// The one way a unit test reaches this account's real home, which on Windows reaches
/// nothing until W14.
#[cfg(test)]
pub(crate) mod testing {
    /// What a unit test keeps while it reaches the real home.
    pub(crate) fn reaching_the_real_home() -> Reaching {
        Reaching(())
    }

    /// What [`reaching_the_real_home`] returns.
    pub(crate) struct Reaching(());
}
