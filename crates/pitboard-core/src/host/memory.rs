//! A machine in memory, where a test can make anything fail.
//!
//! It fakes what a real host offers and nothing above it, so the code under test is the
//! real one. Before this it faked `read_signin` with a map keyed by directory, which meant
//! no test ever ran Claude Code's own slot hashing on the sign-in path: the double answered
//! the question the code was supposed to answer.

use super::{Host, Process, Scheduler};
use crate::context::Context;
use crate::store::memory::MemoryStore;
use crate::store::{Backend, Cost, Error, RawStore};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The vault as one context opens it. A real keychain takes `PITBOARD_NO_ARGV` from the
/// context it is opened with, so above its ceiling it writes on the argument line or
/// refuses; this one does the same.
struct Vault {
    store: Arc<MemoryStore>,
    argument_line: bool,
}

impl RawStore for Vault {
    fn kind(&self) -> Backend {
        RawStore::kind(&self.store)
    }

    fn contains(&self, service: &str) -> Result<bool, Error> {
        RawStore::contains(&self.store, service)
    }

    fn read(&self, service: &str) -> Result<Option<String>, Error> {
        RawStore::read(&self.store, service)
    }

    fn write(&self, service: &str, contents: &str) -> Result<(), Error> {
        if let Some(cost) = self.cost(service, contents)
            && cost.refused()
        {
            return Err(Error::Write(format!(
                "this credential is {} bytes, past the {}-byte command limit",
                cost.needs, cost.limit
            )));
        }
        RawStore::write(&self.store, service, contents)
    }

    fn delete(&self, service: &str) -> Result<(), Error> {
        RawStore::delete(&self.store, service)
    }

    fn list(&self) -> Result<Option<Vec<String>>, Error> {
        RawStore::list(&self.store)
    }

    fn cost(&self, service: &str, contents: &str) -> Option<Cost> {
        RawStore::cost(&self.store, service, contents).map(|cost| Cost {
            second_route: self.argument_line,
            ..cost
        })
    }
}

/// A machine whose keychain and filesystem are both in memory, and whose scheduler writes
/// its files in the test's own home and asks no service manager to start them.
#[derive(Debug)]
pub struct MemoryHost {
    keychain: Arc<MemoryStore>,
    vault: Arc<MemoryStore>,
    files: Mutex<HashMap<PathBuf, Arc<MemoryStore>>>,
    running: Mutex<HashMap<String, Vec<Process>>>,
    /// Whether every home parks in `vault`, the way every home on macOS parks in the login
    /// keychain. So by default, because that is where the rules about another Pitboard's
    /// parks are needed.
    shared_vault: AtomicBool,
    scheduler: Box<dyn Scheduler>,
    /// Whether the scheduler refuses the next schedule it is asked to start.
    refuse_start: Arc<AtomicBool>,
    /// Whether this machine has no scheduler at all.
    unscheduled: AtomicBool,
}

impl Default for MemoryHost {
    fn default() -> MemoryHost {
        let refuse_start = Arc::new(AtomicBool::new(false));
        MemoryHost {
            // Keychain, because that is the chain the interesting rules are written for.
            keychain: MemoryStore::of(Backend::Keychain),
            vault: MemoryStore::of(Backend::Keychain),
            files: Mutex::new(HashMap::new()),
            running: Mutex::new(HashMap::new()),
            shared_vault: AtomicBool::new(true),
            scheduler: super::os::pretend_scheduler(Arc::clone(&refuse_start)),
            refuse_start,
            unscheduled: AtomicBool::new(false),
        }
    }
}

impl MemoryHost {
    pub fn new() -> Arc<MemoryHost> {
        Arc::new(MemoryHost::default())
    }

    /// The keychain, where a tool that uses one keeps its live credential.
    pub fn live(&self) -> &Arc<MemoryStore> {
        &self.keychain
    }

    /// Where Pitboard's parked logins are.
    pub fn vault(&self) -> &Arc<MemoryStore> {
        &self.vault
    }

    /// The file at `path`, where a tool that keeps its login in a file keeps it.
    pub fn file_at(&self, path: PathBuf) -> Arc<MemoryStore> {
        Arc::clone(
            self.files
                .lock()
                .expect("a poisoned test host is a failed test")
                .entry(path)
                .or_insert_with(|| MemoryStore::of(Backend::File)),
        )
    }

    /// From now on the vault belongs to one home alone, the way Pitboard's vault of files
    /// does off macOS.
    pub fn vault_of_its_own(&self) {
        self.shared_vault.store(false, Ordering::SeqCst);
    }

    /// Say that `count` processes are running `program`, each started by its bare name, the
    /// way a shell starts one from `PATH`.
    pub fn runs(&self, program: &str, count: usize) {
        self.runs_at(program, &vec![program; count]);
    }

    /// Say that a process is running `program` from each of `paths`.
    pub fn runs_at(&self, program: &str, paths: &[&str]) {
        let processes = (1..)
            .zip(paths)
            .map(|(pid, path)| Process {
                pid,
                path: PathBuf::from(path),
            })
            .collect();
        self.running
            .lock()
            .expect("a poisoned test host is a failed test")
            .insert(program.to_string(), processes);
    }

    /// The next time a schedule is to be started, the system will not start it.
    pub fn refuse_next_start(&self) {
        self.refuse_start.store(true, Ordering::SeqCst);
    }

    /// From now on there is no scheduler here.
    pub fn without_a_scheduler(&self) {
        self.unscheduled.store(true, Ordering::SeqCst);
    }
}

impl Host for MemoryHost {
    fn foreign_secrets(&self, _ctx: &Context, _account: &str) -> Option<Box<dyn RawStore>> {
        Some(Box::new(Arc::clone(&self.keychain)))
    }

    fn file(&self, path: PathBuf) -> Box<dyn RawStore> {
        Box::new(self.file_at(path))
    }

    fn vault(&self, ctx: &Context) -> Box<dyn RawStore> {
        Box::new(Vault {
            store: Arc::clone(&self.vault),
            argument_line: ctx.argv_fallback(),
        })
    }

    fn vault_is_shared(&self) -> bool {
        self.shared_vault.load(Ordering::SeqCst)
    }

    /// What a test said is running, and nothing on the machine running the tests.
    fn processes(&self, program: &str) -> Option<Vec<Process>> {
        Some(
            self.running
                .lock()
                .expect("a poisoned test host is a failed test")
                .get(program)
                .cloned()
                .unwrap_or_default(),
        )
    }

    fn scheduler(&self) -> Option<&dyn Scheduler> {
        (!self.unscheduled.load(Ordering::SeqCst)).then_some(self.scheduler.as_ref())
    }
}
