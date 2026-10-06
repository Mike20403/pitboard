//! One file as a credential store. Claude Code falls back to one on macOS and has only one
//! on Linux; Codex keeps its whole login in one.

use super::{Backend, Error, RawStore};
use crate::atomic;
use crate::service::Permit;
use std::path::PathBuf;

/// A tool that keeps its login in a file keeps exactly one per directory, so the service
/// name selects nothing here.
pub(crate) struct PlainFile {
    path: PathBuf,
}

impl PlainFile {
    pub(crate) fn at(path: PathBuf) -> PlainFile {
        PlainFile { path }
    }
}

impl RawStore for PlainFile {
    fn kind(&self) -> Backend {
        Backend::File
    }

    fn contains(&self, _service: &str) -> Result<bool, Error> {
        super::exists(&self.path)
    }

    fn read(&self, _service: &str) -> Result<Option<String>, Error> {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Unreadable(format!(
                "cannot read {}: {e}",
                self.path.display()
            ))),
        }
    }

    fn write(&self, permit: Permit, service: &str, contents: &str) -> Result<(), Error> {
        let path = &self.path;
        atomic::write(permit, path, contents.as_bytes(), atomic::Perms::Secret)
            .map_err(|e| Error::Write(format!("cannot write {}: {e}", path.display())))?;
        match self.read(service)? {
            Some(back) if back == contents => Ok(()),
            _ => Err(Error::NotDurable(format!(
                "{} does not hold what was written",
                path.display()
            ))),
        }
    }

    fn delete(&self, permit: Permit, _service: &str) -> Result<(), Error> {
        match crate::host::fs::remove_file(permit, &self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::Write(e.to_string())),
        }
    }
}
