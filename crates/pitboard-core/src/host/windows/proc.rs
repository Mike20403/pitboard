//! Processes on Windows, before this face is written.

use std::path::Path;

/// Whether process `pid` may still be running: always, until W18 asks Windows. The callers
/// only ever leave alone what a live process may still be using, so this keeps everything.
pub(crate) fn may_be_running(_pid: u32) -> bool {
    true
}

/// Whether `path` is a program this user may run: none is, until W17 finds programs as a
/// Windows shell does, by `PATHEXT`. So no tool's program is found, and no sign-in starts.
pub(crate) fn can_run(_path: &Path) -> bool {
    false
}
