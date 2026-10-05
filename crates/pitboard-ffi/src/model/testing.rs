//! What the model's tests share: records as the core reports them, the state driven by hand,
//! and, for the threaded tests, a machine of the test's own that the real core runs on.

use super::state::{Answer, Cadence, Job, Msg, Now, State};
use super::{Intent, Snapshot};
use crate::{Account, Limit, Made, Pitboard, PitboardError, Source, Status, Tool, Usage, Warning};
use pitboard_core::context::Context;
use pitboard_core::provider::ProviderId;
use pitboard_core::service;
use pitboard_core::testing::{MemoryHost, ScriptedApi, live_service};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) fn claude_code() -> Tool {
    crate::tool(ProviderId::Claude)
}

pub(super) fn codex() -> Tool {
    crate::tool(ProviderId::Codex)
}

/// A limit as the core reports one.
pub(super) fn window(kind: &str, percent: f64) -> Limit {
    Limit {
        kind: kind.into(),
        length_seconds: None,
        scope: None,
        percent,
        resets_at: Some(100),
        severity: None,
        is_active: true,
    }
}

/// An account as the core reports one. `label` `None` is a login signed in and not
/// enrolled. Switchable unless it is the one signed in, as a real one is.
pub(super) fn account(
    label: Option<&str>,
    provider: &str,
    signed_in: bool,
    windows: Vec<Limit>,
) -> Account {
    let uuid = label.unwrap_or("someone");
    Account {
        id: format!("{provider}:{uuid}"),
        provider: provider.into(),
        label: label.map(str::to_owned),
        qualified: label.map(|label| format!("{provider}/{label}")),
        unplaced: false,
        email: format!("{uuid}@example.com"),
        account_uuid: uuid.into(),
        signed_in,
        switchable: !signed_in && label.is_some(),
        parked: None,
        usage: Some(Usage {
            source: Source::Live,
            observed_at: Some(0),
            windows,
        }),
        stale: None,
        stale_explanation: None,
        lasts_seconds: None,
        lasts_burning: false,
    }
}

/// A Claude Code account whose five-hour window is `percent` used.
pub(super) fn claude(label: &str, signed_in: bool, percent: f64) -> Account {
    account(
        Some(label),
        "claude",
        signed_in,
        vec![window("session", percent)],
    )
}

/// A Codex account with no limits read.
pub(super) fn codex_account(label: &str, signed_in: bool) -> Account {
    account(Some(label), "codex", signed_in, Vec::new())
}

pub(super) fn status(accounts: Vec<Account>) -> Status {
    warned(accounts, Vec::new())
}

pub(super) fn warned(accounts: Vec<Account>, warnings: Vec<Warning>) -> Status {
    Status {
        now: 0,
        accounts,
        warnings,
    }
}

pub(super) fn warning(code: &str, message: &str) -> Warning {
    Warning {
        code: code.into(),
        message: message.into(),
    }
}

/// A failure as the core reports one.
#[derive(Debug, Clone)]
pub(super) struct Refusal {
    pub code: String,
    pub message: String,
    pub warnings: Vec<Warning>,
}

impl Refusal {
    fn error(&self) -> PitboardError {
        PitboardError::Failed {
            code: self.code.clone(),
            cause: None,
            message: self.message.clone(),
            warnings: self.warnings.clone(),
        }
    }
}

pub(super) fn refusal(code: &str, message: &str, warnings: Vec<Warning>) -> Refusal {
    Refusal {
        code: code.into(),
        message: message.into(),
        warnings,
    }
}

/// The percentage the first window of the account at `index` shows.
pub(super) fn percent(snapshot: &Snapshot, index: usize) -> Option<f64> {
    let usage = snapshot
        .status
        .as_ref()?
        .accounts
        .get(index)?
        .usage
        .as_ref()?;
    Some(usage.windows.first()?.percent)
}

/// What the core answers, as a test says, the way the Swift tests' stub answered.
pub(super) struct Machine {
    /// What a read gives.
    pub answer: Result<Status, Refusal>,
    /// What reading what is already known gives.
    pub offline: Result<Status, Refusal>,
    /// When the account index was last written, in seconds.
    pub changed: i64,
    /// When the usage readings were last written. A read moves it, as the core's does: what
    /// it measured is recorded.
    pub readings: i64,
    /// The tools whose program the app found.
    pub found: Vec<Tool>,
}

impl Machine {
    pub(super) fn reading(answer: Result<Status, Refusal>) -> Machine {
        Machine {
            answer,
            offline: Ok(status(Vec::new())),
            changed: 0,
            readings: 0,
            found: vec![claude_code()],
        }
    }

    /// What the core answers `job` with.
    pub(super) fn answer(&mut self, job: Job) -> Answer {
        match job {
            Job::Read { ticket, .. } => {
                let (readings_before, changed_before) = (self.readings, self.changed);
                self.readings += 1;
                Answer::Read {
                    ticket,
                    readings_before,
                    changed_before,
                    read: self.answer.clone().map_err(|refused| refused.error()),
                }
            }
            Job::ReadOffline { why } => Answer::ReadOffline {
                why,
                read: self.offline.clone().map_err(|refused| refused.error()),
            },
            Job::Look => Answer::Looked {
                changed: self.changed,
                measured: self.readings,
            },
            Job::AskInstalled => Answer::Installed {
                tools: self.found.clone(),
            },
        }
    }
}

/// The model's state driven by hand. Each message is handed to `State::apply` with the time
/// it arrived, and each job that says to run waits for the test to answer it: all of them in
/// turn, as the Swift model awaited each call, or one at a time in whatever order a test
/// wants.
pub(super) struct Hand {
    pub state: State,
    pub now: Now,
    /// Jobs asked for and not yet answered, oldest first.
    pending: VecDeque<Job>,
    /// Every job asked for, in order.
    pub asked: Vec<Job>,
}

impl Hand {
    pub(super) fn new() -> Hand {
        Hand {
            state: State::new(Cadence::APP),
            now: Now {
                running: Duration::ZERO,
                epoch_ms: 1_800_000_000_000,
            },
            pending: VecDeque::new(),
            asked: Vec::new(),
        }
    }

    fn took(&mut self, jobs: Vec<Job>) -> Vec<Job> {
        self.asked.extend(jobs.iter().cloned());
        self.pending.extend(jobs.iter().cloned());
        jobs
    }

    pub(super) fn send(&mut self, intent: Intent) -> Vec<Job> {
        let jobs = self.state.apply(Msg::Intent(intent), self.now);
        self.took(jobs)
    }

    /// Hands the model what a job came to.
    pub(super) fn give(&mut self, answer: Answer) -> Vec<Job> {
        let jobs = self.state.apply(Msg::Done(answer), self.now);
        self.took(jobs)
    }

    /// The actor waking for a timer.
    pub(super) fn tick(&mut self) -> Vec<Job> {
        let jobs = self.state.apply(Msg::Tick, self.now);
        self.took(jobs)
    }

    /// The change poll, for a test that must not wait for a timer.
    pub(super) fn look(&mut self) -> Vec<Job> {
        let jobs = self.state.look_now();
        self.took(jobs)
    }

    pub(super) fn later(&mut self, by: Duration) {
        self.now.running += by;
        self.now.epoch_ms += i64::try_from(by.as_millis()).expect("a short while");
    }

    /// The oldest job not yet answered, which the test now answers itself.
    pub(super) fn next(&mut self) -> Job {
        self.pending
            .pop_front()
            .expect("a job waiting for its answer")
    }

    /// Answers every job from `machine`, and every job those answers lead to, oldest first.
    pub(super) fn run(&mut self, machine: &mut Machine) {
        while let Some(job) = self.pending.pop_front() {
            let answer = machine.answer(job);
            self.give(answer);
        }
    }

    /// A read, as the timer's would be, and everything it leads to.
    pub(super) fn refresh(&mut self, machine: &mut Machine) {
        self.send(Intent::Refresh { asked: false });
        self.run(machine);
    }

    /// The change poll, and everything it leads to.
    pub(super) fn notice(&mut self, machine: &mut Machine) {
        self.look();
        self.run(machine);
    }

    pub(super) fn shown(&self) -> Snapshot {
        self.state.snapshot(0, self.now.epoch())
    }

    /// How many of the jobs asked for so far are `which`.
    pub(super) fn count(&self, which: fn(&Job) -> bool) -> usize {
        self.asked.iter().filter(|job| which(job)).count()
    }

    pub(super) fn pending(&self) -> usize {
        self.pending.len()
    }
}

pub(super) fn fresh_read(job: &Job) -> bool {
    matches!(job, Job::Read { fresh: true, .. })
}

pub(super) fn any_read(job: &Job) -> bool {
    matches!(job, Job::Read { .. })
}

pub(super) fn offline_read(job: &Job) -> bool {
    matches!(job, Job::ReadOffline { .. })
}

pub(super) fn installed_ask(job: &Job) -> bool {
    matches!(job, Job::AskInstalled)
}

/// A machine of a test's own for the real core: a home in a scratch directory, Claude Code's
/// keychain, the vault and the scheduler in memory, and Anthropic scripted. Nothing of the
/// machine running the tests is read: no environment, no `PATH`, no login shell.
pub(super) struct World {
    root: PathBuf,
    pub host: Arc<MemoryHost>,
    pub api: Arc<ScriptedApi>,
    ctx: Context,
}

impl World {
    pub(super) fn new(name: &str) -> World {
        let root = std::env::temp_dir().join(format!(
            "pitboard-model-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a scratch home");
        let host = MemoryHost::new();
        let api = ScriptedApi::new();
        let ctx = Context::new(root.clone())
            .with_pitboard_home(root.join(".pitboard"))
            .with_codex_home(root.join(".codex").to_string_lossy().into_owned())
            .with_user("tester".into())
            .with_search_path(String::new())
            .with_memory_stores(Arc::clone(&host))
            .with_scripted_api(Arc::clone(&api))
            .with_caller("app".into());
        World {
            root,
            host,
            api,
            ctx,
        }
    }

    /// The app's core over this machine, as the bindings make one, with Claude Code's
    /// program found.
    pub(super) fn core(&self) -> Arc<Pitboard> {
        let ctx = self.ctx.clone();
        Arc::new(Pitboard::asking(
            move || {
                let made = Made {
                    core: service::Pitboard::new(ctx.clone()),
                    found: vec![ProviderId::Claude],
                    search_path: None,
                    schedules_a_command_line: false,
                };
                (made, false)
            },
            crate::ASK_AGAIN_AFTER,
        ))
    }

    /// Another front end on the same machine, as the command line in a terminal is.
    pub(super) fn elsewhere(&self) -> service::Pitboard {
        service::Pitboard::new(self.ctx.clone().with_caller("cli".into()))
    }

    /// Claude Code signed in as `who` and enrolled as `label`, with its five-hour window
    /// `percent` used, as Anthropic answers.
    pub(super) fn enrolled(&self, label: &str, who: &str, percent: f64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("after 1970")
            .as_secs();
        let now = i64::try_from(now).expect("before 2262");
        let access = format!("access-{who}");
        let login = format!(
            r#"{{"claudeAiOauth":{{"accessToken":"{access}","refreshToken":"refresh-{who}","expiresAt":{},"refreshTokenExpiresAt":{},"scopes":["user:profile","user:inference"]}}}}"#,
            (now + 3_600) * 1_000,
            (now + 30 * 86_400) * 1_000,
        );
        self.host.live().plant(&live_service(&self.ctx), &login);
        std::fs::write(
            self.root.join(".claude.json"),
            format!(
                r#"{{"oauthAccount":{{"accountUuid":"{who}","emailAddress":"{who}@example.com","organizationUuid":"org-{who}"}}}}"#
            ),
        )
        .expect("Claude Code's config");
        self.api.owned_by(
            &access,
            pitboard_core::api::Owner {
                account_uuid: who.into(),
                email: format!("{who}@example.com"),
                organization_uuid: format!("org-{who}"),
            },
        );
        self.api.using(
            &access,
            pitboard_core::usage::Snapshot {
                windows: vec![pitboard_core::usage::Window {
                    kind: "session".into(),
                    scope: None,
                    percent,
                    resets_at: Some(now + 3_600),
                    is_active: true,
                    severity: None,
                    length_seconds: Some(18_000),
                }],
                observed_at: Some(now),
                account_uuid: None,
                source: pitboard_core::usage::Source::Live,
            },
        );
        self.elsewhere()
            .enroll_current(label)
            .expect("the account signed in, enrolled");
    }

    /// Says the account index was written `seconds` later than it was. The index's time is
    /// kept to the second, so a write within the same second as the last one looks like
    /// none; a test says it came later rather than waiting a second.
    pub(super) fn index_written_later(&self, seconds: u64) {
        let index = self.root.join(".pitboard").join("state.json");
        let file = std::fs::File::options()
            .write(true)
            .open(&index)
            .expect("the account index");
        let written = file
            .metadata()
            .and_then(|meta| meta.modified())
            .expect("when it was written");
        file.set_modified(written + Duration::from_secs(seconds))
            .expect("a later time");
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
