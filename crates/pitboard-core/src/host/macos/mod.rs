//! macOS: the login keychain, files, `ps` and launchd.

mod helper;
mod keychain;
mod launchd;
mod ps;

pub(crate) use super::unix::{fs, proc, user};

use super::unix::service;
use super::{Host, Os, Process, Scheduler};
use crate::context::Context;
use crate::store::{PlainFile, RawStore};
use std::path::PathBuf;
use std::sync::Arc;

pub(super) const OS: Os = Os::MacOs;

/// The keychain account pitboard stores its own items under.
///
/// It is Claude Code's derivation, and it stays Claude Code's derivation, because every
/// park already on every machine is filed under whatever this returned the day it was
/// written. Changing it would not move those items; it would make them unfindable, which
/// is the same as deleting every parked login on upgrade.
fn vault_account(ctx: &Context) -> String {
    crate::provider::claude::slot::account_name(ctx)
}

#[derive(Debug)]
struct MacOs {
    scheduler: launchd::Launchd,
}

impl Host for MacOs {
    fn foreign_secrets(&self, ctx: &Context, account: &str) -> Option<Box<dyn RawStore>> {
        Some(Box::new(keychain::Keychain::foreign(
            ctx,
            account.to_string(),
        )))
    }

    fn file(&self, path: PathBuf) -> Box<dyn RawStore> {
        Box::new(PlainFile::at(path))
    }

    fn vault(&self, ctx: &Context) -> Box<dyn RawStore> {
        Box::new(keychain::Keychain::vault(ctx))
    }

    fn vault_is_shared(&self) -> bool {
        true
    }

    fn processes(&self, program: &str) -> Option<Vec<Process>> {
        ps::processes(program)
    }

    fn scheduler(&self) -> Option<&dyn Scheduler> {
        Some(&self.scheduler)
    }
}

pub(super) fn host() -> Arc<dyn Host> {
    Arc::new(MacOs {
        scheduler: launchd::Launchd::new(service::system()),
    })
}

/// macOS already says what path a program was started by.
pub(super) fn current_program() -> std::io::Result<PathBuf> {
    std::env::current_exe()
}

/// launchd as a machine in memory has it: real files in the test's own home, and a service
/// manager that asks nobody.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn pretend_scheduler(
    refuse_start: Arc<std::sync::atomic::AtomicBool>,
) -> Box<dyn Scheduler> {
    Box::new(launchd::Launchd::new(Arc::new(service::Pretend {
        refuse_start,
    })))
}
