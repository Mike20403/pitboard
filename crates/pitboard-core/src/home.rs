//! Pitboard's own directory. Every directory Pitboard creates is private to its owner,
//! whatever the umask: park file names contain account identifiers, so on a shared machine a
//! listing would leak.

use crate::context::Context;
use crate::error::{Error, Result};
use crate::service::Permit;
use std::io;
use std::path::{Path, PathBuf};

/// Directory names that mean a sync client would copy this to another machine, where a
/// parked login must never go.
const SYNCED: [&str; 5] = [
    "Dropbox",
    "Google Drive",
    "OneDrive",
    "com~apple~CloudDocs",
    "Sync",
];

pub fn dir(ctx: &Context) -> PathBuf {
    ctx.pitboard_home.clone()
}

pub fn ensure(ctx: &Context, permit: Permit) -> io::Result<PathBuf> {
    let path = dir(ctx);
    crate::host::fs::create_private_dir(permit, &path)?;
    Ok(path)
}

/// Refuses a home this context names that is empty or relative, with `home_not_absolute`
/// and the variable that named it.
///
/// Every path Pitboard reads or writes is under one of these: the person's home, which
/// holds the scheduler's files and each tool's default folder; Pitboard's own directory;
/// Claude Code's config directory and the one its credential slot is named from; and
/// Codex's home. A relative one leads somewhere else from every folder a program runs in.
/// Claude Code uses `CLAUDE_CONFIG_DIR` and `CLAUDE_SECURESTORAGE_CONFIG_DIR` as they are
/// given, and Codex canonicalises `CODEX_HOME` against the folder it runs in, so a relative
/// one names a different login in each, and Pitboard would act on the one under the folder
/// it was run from. An empty `HOME` or `PITBOARD_HOME` left Pitboard's files in that folder.
///
/// The home is asked first, so a relative home is named rather than the Pitboard directory
/// worked out from it. Unset, each tool's variable names nothing, and an empty
/// `CLAUDE_SECURESTORAGE_CONFIG_DIR` names Claude Code's default folder.
///
/// The one gate every change passes asks this ([`crate::service::Permit`]), and so does
/// every read of Pitboard's account list, so nothing is read or written under such a home.
pub fn check_absolute(ctx: &Context) -> Result<()> {
    let homes: [(&'static str, Option<&Path>); 5] = [
        ("HOME", Some(ctx.home())),
        ("PITBOARD_HOME", Some(&ctx.pitboard_home)),
        (
            "CLAUDE_CONFIG_DIR",
            ctx.claude_config_dir.as_deref().map(Path::new),
        ),
        (
            "CLAUDE_SECURESTORAGE_CONFIG_DIR",
            ctx.secure_storage_dir
                .as_deref()
                .filter(|dir| !dir.is_empty())
                .map(Path::new),
        ),
        ("CODEX_HOME", ctx.codex_home().map(Path::new)),
    ];
    match homes
        .into_iter()
        .find_map(|(variable, path)| path.filter(|p| !p.is_absolute()).map(|p| (variable, p)))
    {
        Some((variable, path)) => Err(Error::HomeNotAbsolute {
            variable,
            path: path.to_path_buf(),
        }),
        None => Ok(()),
    }
}

pub fn check_location(path: &Path) -> Result<()> {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.to_string_lossy();
    match SYNCED.iter().find(|marker| text.contains(**marker)) {
        Some(marker) => Err(Error::StateOnSyncedDrive {
            path: resolved.clone(),
            marker: (*marker).to_string(),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Environment;

    /// What `check_absolute` says of the context `pairs` make: the variable it names, or
    /// `None` where every home is a full path.
    fn refused_over(pairs: &[(&str, &str)]) -> Option<(&'static str, PathBuf)> {
        let env: Environment = pairs.iter().copied().collect();
        match check_absolute(&Context::for_command_line(&env)) {
            Ok(()) => None,
            Err(Error::HomeNotAbsolute { variable, path }) => Some((variable, path)),
            Err(other) => panic!("{other}"),
        }
    }

    /// Each home the environment names is a full path or refused, by the variable that
    /// named it, and the home is asked first, so a relative one is named rather than the
    /// Pitboard directory worked out from it. Unset, a tool's variable names nothing, and an
    /// empty `CLAUDE_SECURESTORAGE_CONFIG_DIR` names Claude Code's default folder.
    #[test]
    fn a_home_that_is_not_a_full_path_is_refused_by_the_variable_that_named_it() {
        let home = ("HOME", "/Users/x");
        for pairs in [
            &[home][..],
            &[home, ("PITBOARD_HOME", "/elsewhere/./pitboard/")],
            &[home, ("CLAUDE_CONFIG_DIR", "/Users/x/claude")],
            &[home, ("CLAUDE_SECURESTORAGE_CONFIG_DIR", "")],
            &[home, ("CLAUDE_SECURESTORAGE_CONFIG_DIR", "/Users/x/slot")],
            &[home, ("CODEX_HOME", "/Users/x/codex")],
            &[home, ("CODEX_HOME", "")],
        ] {
            assert_eq!(refused_over(pairs), None, "{pairs:?}");
        }
        for (pairs, variable, path) in [
            (&[("HOME", "")][..], "HOME", ""),
            (&[("HOME", "relative")], "HOME", "relative"),
            (
                &[("HOME", "x"), ("PITBOARD_HOME", "/Users/x/.pitboard")],
                "HOME",
                "x",
            ),
            (&[home, ("PITBOARD_HOME", "")], "PITBOARD_HOME", ""),
            (
                &[home, ("PITBOARD_HOME", "pitboard")],
                "PITBOARD_HOME",
                "pitboard",
            ),
            (
                &[home, ("CLAUDE_CONFIG_DIR", "claude")],
                "CLAUDE_CONFIG_DIR",
                "claude",
            ),
            (
                &[home, ("CLAUDE_SECURESTORAGE_CONFIG_DIR", "./slot")],
                "CLAUDE_SECURESTORAGE_CONFIG_DIR",
                "./slot",
            ),
            (&[home, ("CODEX_HOME", "codex")], "CODEX_HOME", "codex"),
        ] {
            assert_eq!(
                refused_over(pairs),
                Some((variable, PathBuf::from(path))),
                "{pairs:?}"
            );
        }
    }

    /// The refusal says which variable, what it holds, and the two ways out.
    #[test]
    fn a_home_that_is_not_a_full_path_is_said_with_the_way_out() {
        let said = |pairs: &[(&str, &str)]| {
            let env: Environment = pairs.iter().copied().collect();
            let refused = check_absolute(&Context::for_command_line(&env)).expect_err("refused");
            assert_eq!(refused.code(), "home_not_absolute");
            refused.to_string()
        };
        assert_eq!(
            said(&[("HOME", "")]),
            "HOME is empty, so the folder it names would depend on where each program runs. \
             Set it to a full path, or unset it; Pitboard reads and changes nothing until \
             then."
        );
        assert!(
            said(&[("HOME", "/Users/x"), ("CODEX_HOME", "codex")])
                .starts_with("CODEX_HOME is `codex`, which is not a full path, so ")
        );
    }

    #[test]
    fn a_cloud_synced_location_is_refused() {
        for synced in [
            "/Users/x/Library/Mobile Documents/com~apple~CloudDocs/pitboard",
            "/home/x/Dropbox/pitboard",
            "/home/x/OneDrive/tools/pitboard",
        ] {
            assert!(check_location(Path::new(synced)).is_err(), "{synced}");
        }
        assert!(check_location(Path::new("/Users/x/.pitboard")).is_ok());
        assert!(check_location(Path::new("/home/x/.pitboard")).is_ok());
    }
}
