//! What the harness does differently on each system, said once per fact as a `match` on
//! [`OS`], so a system added to [`Os`] does not compile here until each fact has been said
//! for it. The rest of the harness asks one of these, never which system it runs on, and sets
//! a file's access or makes a link only through the core's own `pitboard_core::testing::fs`.

use pitboard_core::host::{OS, Os};
use pitboard_core::testing::fs as files;
use std::path::Path;

/// Where a login is kept, as the harness plants one, reads it back and takes it away. Told
/// apart only by a `match`, so a way of keeping one added here does not compile until every
/// part of the harness that plants, reads or takes away a login says what it does there.
#[derive(Debug, Clone, Copy)]
pub enum Kept {
    /// An item of the login keychain, under the name it is given and [`super::account`]. It
    /// outlives the test's own directory, so the harness takes it away by name.
    InKeychain,
    /// A file in the test's own directory, private to its owner, which goes with that
    /// directory.
    InFile,
}

/// Where Claude Code keeps the login it uses: an item of the login keychain on macOS, and
/// `.credentials.json` in its config directory on Linux. On Windows the harness says it once
/// it runs there (W13): no test that plants a login runs on Windows before then.
pub fn claude_code_login() -> Kept {
    match OS {
        Os::MacOs => Kept::InKeychain,
        Os::Linux => Kept::InFile,
        Os::Windows => panic!("W13 says where the harness keeps Claude Code's login on Windows"),
    }
}

/// Where Pitboard parks a login: an item of the login keychain on macOS, and a file in the
/// `vault` of its own directory on Linux. On Windows the harness says it once it runs there
/// (W13), as above.
pub fn parked_login() -> Kept {
    match OS {
        Os::MacOs => Kept::InKeychain,
        Os::Linux => Kept::InFile,
        Os::Windows => panic!("W13 says where the harness keeps a parked login on Windows"),
    }
}

/// `contents` at `path`, private to its owner, as Claude Code, Codex and Pitboard each leave
/// a login in a file: a stand-in that leaves the umask to decide writes one anybody can read,
/// which `doctor` is right to fail on and which none of them does.
pub fn write_private(path: &Path, contents: &str) {
    std::fs::write(path, contents)
        .unwrap_or_else(|e| panic!("{} could not be written: {e}", path.display()));
    files::make_private(path)
        .unwrap_or_else(|e| panic!("{} could not be made private: {e}", path.display()));
}

/// The directory `path`, made with any parent it is missing, and itself private to its owner
/// as Pitboard makes its vault.
pub fn create_private_dir(path: &Path) {
    std::fs::create_dir_all(path)
        .unwrap_or_else(|e| panic!("{} could not be made: {e}", path.display()));
    files::make_private(path)
        .unwrap_or_else(|e| panic!("{} could not be made private: {e}", path.display()));
}
