//! Pitboard's own directory. Every directory Pitboard creates is private to its owner,
//! whatever the umask: park file names contain account identifiers, so on a shared machine a
//! listing would leak.

use crate::context::Context;
use crate::error::{Error, Result};
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

pub fn ensure(ctx: &Context) -> io::Result<PathBuf> {
    let path = dir(ctx);
    crate::host::fs::create_private_dir(&path)?;
    Ok(path)
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
