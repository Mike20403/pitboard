//! What the model's tests share: records as the core reports them, the state driven by hand,
//! other apps as a test says they are, and, for the threaded tests, a machine of the test's
//! own that the real core runs on.

use super::lanes;
use super::state::{Answer, Cadence, Job, Msg, Now, State};
use super::{AppControl, Intent, PlatformError, Snapshot};
use crate::{
    Abandoned, Account, Adoption, Holding, Limit, Made, Pitboard, PitboardError, Remedy, Source,
    Status, Switch, Switched, Tool, Usage, Warning,
};
use pitboard_core::context::Context;
use pitboard_core::provider::ProviderId;
use pitboard_core::service;
use pitboard_core::testing::{MemoryHost, ScriptedApi, live_service};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The ChatGPT app's bundle id, which is how the core's holder detection names it.
pub(super) const CHATGPT: &str = "com.openai.codex";

/// Where ChatGPT runs its own `codex`, as the process list of a Mac running ChatGPT
/// 26.928.31416 gave it, which is how the core tells the app is open.
pub(super) const CHATGPT_CODEX: &str =
    "/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex";

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

/// The warning a Codex switch carries when `codex` sessions it cannot reach still run, as
/// the core words it.
pub(super) fn still_running() -> Warning {
    warning(
        "sessions_still_running",
        "2 `codex` sessions started before this switch are still running and still using \
         `codex/personal`. Quit them and start again to use the new account. Do not sign out \
         in any of them: signing out there revokes `codex/personal`'s login, which Pitboard \
         has just parked.",
    )
}

/// A switch of `provider`'s tool, as the core reports one: `from` and `to` typed the way
/// the core types them, bare for Claude Code and with the tool for any other. Codex's open
/// sessions never follow; Claude Code's follow within 33 seconds.
pub(super) fn switched(
    provider: &str,
    from: &str,
    to: &str,
    warnings: Vec<Warning>,
) -> Result<Switched, Refusal> {
    let adoption = if provider == "codex" {
        Adoption::Restart {
            program: "codex".into(),
        }
    } else {
        Adoption::Follows { within_seconds: 33 }
    };
    Ok(Switched {
        outcome: Switch::Switched {
            provider: provider.into(),
            from: from.into(),
            to: to.into(),
            adoption,
        },
        warnings,
    })
}

/// A switch to the account already in use, as the core reports one.
pub(super) fn already_active(label: &str, warnings: Vec<Warning>) -> Result<Switched, Refusal> {
    Ok(Switched {
        outcome: Switch::AlreadyActive {
            label: label.into(),
        },
        warnings,
    })
}

/// ChatGPT running Codex's login, as the core's holder detection reports it.
pub(super) fn chatgpt_holding() -> Holding {
    Holding {
        kind: "chatgpt_app".into(),
        phrase: "the ChatGPT app".into(),
        pids: vec![4242, 4243],
        remedy: Remedy::ReopenApp {
            bundle_id: CHATGPT.into(),
            name: "ChatGPT".into(),
        },
    }
}

/// What else an app quitting does, given the app's id.
type OnQuit = Box<dyn Fn(&str) + Send>;

/// Other apps, as a test says they are, and nothing on the machine running the tests: this
/// machine may have ChatGPT open. Records every app asked to quit and every app opened.
pub(super) struct StandInApps {
    running: Mutex<BTreeSet<String>>,
    /// Whether an app asked to quit does. One busy with work, or whose person said no, does
    /// not.
    quits: AtomicBool,
    asked: Mutex<Vec<String>>,
    running_calls: AtomicUsize,
    fail_running: AtomicBool,
    fail_quit: AtomicBool,
    fail_open: AtomicBool,
    /// Holds each call of `running` until the test lets it go, telling the test as it
    /// arrives, where a test says so.
    hold: Mutex<Option<(Sender<()>, Receiver<()>)>>,
    /// What else an app quitting does, such as its processes leaving the process list.
    on_quit: Mutex<Option<OnQuit>>,
}

impl StandInApps {
    pub(super) fn new(running: &[&str], quits: bool) -> Arc<StandInApps> {
        Arc::new(StandInApps {
            running: Mutex::new(running.iter().map(|&id| id.to_owned()).collect()),
            quits: AtomicBool::new(quits),
            asked: Mutex::new(Vec::new()),
            running_calls: AtomicUsize::new(0),
            fail_running: AtomicBool::new(false),
            fail_quit: AtomicBool::new(false),
            fail_open: AtomicBool::new(false),
            hold: Mutex::new(None),
            on_quit: Mutex::new(None),
        })
    }

    /// From now on an app that quits does `then` too, with its id.
    pub(super) fn on_quit(&self, then: impl Fn(&str) + Send + 'static) {
        *self.on_quit.lock().expect("a test's own lock") = Some(Box::new(then));
    }

    /// Where a test's app is, which is nowhere on the machine running it.
    pub(super) fn copy_of(app: &str) -> String {
        format!("/stand-in/{app}.app")
    }

    /// Every app asked to quit and every app opened, in order: "quit com.openai.codex".
    pub(super) fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("a test's own lock").clone()
    }

    pub(super) fn set_running(&self, apps: &[&str]) {
        *self.running.lock().expect("a test's own lock") =
            apps.iter().map(|&id| id.to_owned()).collect();
    }

    pub(super) fn is_running(&self, app: &str) -> bool {
        self.running
            .lock()
            .expect("a test's own lock")
            .contains(app)
    }

    /// How many times it was asked whether an app runs.
    pub(super) fn running_calls(&self) -> usize {
        self.running_calls.load(Ordering::SeqCst)
    }

    /// From now on asking whether an app runs throws, as an app's own code can.
    pub(super) fn fail_running(&self, fails: bool) {
        self.fail_running.store(fails, Ordering::SeqCst);
    }

    pub(super) fn fail_quit(&self, fails: bool) {
        self.fail_quit.store(fails, Ordering::SeqCst);
    }

    pub(super) fn fail_open(&self, fails: bool) {
        self.fail_open.store(fails, Ordering::SeqCst);
    }

    /// Holds every call of `running` from now on until the returned sender is dropped,
    /// telling the returned receiver as each arrives.
    pub(super) fn hold_running(&self) -> (Receiver<()>, Sender<()>) {
        let (arrived, arrivals) = channel();
        let (release, held) = channel();
        *self.hold.lock().expect("a test's own lock") = Some((arrived, held));
        (arrivals, release)
    }

    fn refused(what: &str) -> PlatformError {
        PlatformError::Failed {
            reason: format!("the stand-in was told to fail {what}"),
        }
    }
}

impl AppControl for StandInApps {
    fn running(&self, app: String) -> Result<Option<String>, PlatformError> {
        self.running_calls.fetch_add(1, Ordering::SeqCst);
        if let Some((arrived, held)) = self.hold.lock().expect("a test's own lock").as_ref() {
            let _ = arrived.send(());
            let _ = held.recv();
        }
        if self.fail_running.load(Ordering::SeqCst) {
            return Err(StandInApps::refused("to say what runs"));
        }
        Ok(self.is_running(&app).then(|| StandInApps::copy_of(&app)))
    }

    fn request_quit(&self, app: String) -> Result<(), PlatformError> {
        if self.fail_quit.load(Ordering::SeqCst) {
            return Err(StandInApps::refused("to ask an app to quit"));
        }
        self.asked
            .lock()
            .expect("a test's own lock")
            .push(format!("quit {app}"));
        if self.quits.load(Ordering::SeqCst) {
            self.running.lock().expect("a test's own lock").remove(&app);
            if let Some(then) = self.on_quit.lock().expect("a test's own lock").as_ref() {
                then(&app);
            }
        }
        Ok(())
    }

    fn reopen(&self, location: String) -> Result<(), PlatformError> {
        if self.fail_open.load(Ordering::SeqCst) {
            return Err(StandInApps::refused("to open an app"));
        }
        let app = location
            .strip_prefix("/stand-in/")
            .and_then(|rest| rest.strip_suffix(".app"))
            .unwrap_or(&location)
            .to_owned();
        self.asked
            .lock()
            .expect("a test's own lock")
            .push(format!("open {app}"));
        self.running.lock().expect("a test's own lock").insert(app);
        Ok(())
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
    /// What a switch gives.
    pub switched: Result<Switched, Refusal>,
    /// Every account switched to, as the model named it.
    pub switched_to: Vec<String>,
    /// What runs each tool with its login in memory, by the tool's code, as the core's
    /// holder detection finds it in the process list.
    pub held: HashMap<String, Vec<Holding>>,
    /// The other apps on this machine.
    pub apps: Arc<StandInApps>,
    /// What giving up on an interrupted switch gives.
    pub abandoned: Result<Option<Abandoned>, Refusal>,
}

impl Machine {
    pub(super) fn reading(answer: Result<Status, Refusal>) -> Machine {
        Machine {
            answer,
            offline: Ok(status(Vec::new())),
            changed: 0,
            readings: 0,
            found: vec![claude_code()],
            switched: already_active("work", Vec::new()),
            switched_to: Vec::new(),
            held: HashMap::new(),
            apps: StandInApps::new(&[], true),
            abandoned: Ok(None),
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
            // What holds the login, and quitting an app, by the lanes' own rules over what
            // the test says runs, with no time given to quit: an app quits when asked, or
            // is still running.
            Job::Holding { qualified } => {
                let provider = super::state::split(&qualified).0;
                let holdings = self.held.get(provider).map_or(&[][..], Vec::as_slice);
                Answer::Held {
                    question: lanes::holder(holdings, self.apps.as_ref(), &qualified),
                    qualified,
                }
            }
            Job::Quit { question } => Answer::Quit {
                outcome: lanes::quit(
                    self.apps.as_ref(),
                    &question.app_id,
                    Duration::ZERO,
                    Duration::ZERO,
                ),
                question,
            },
            Job::Switch { qualified, reopen } => {
                self.switched_to.push(qualified.clone());
                Answer::Switched {
                    qualified,
                    reopen,
                    done: self.switched.clone().map_err(|refused| refused.error()),
                }
            }
            Job::Open { location } => {
                let _ = self.apps.reopen(location);
                Answer::Opened
            }
            Job::Abandon => {
                Answer::Abandoned(self.abandoned.clone().map_err(|refused| refused.error()))
            }
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

    /// The oldest job not yet answered that is `which`, which the test now answers itself
    /// while the others wait.
    pub(super) fn take(&mut self, which: fn(&Job) -> bool) -> Job {
        let at = self
            .pending
            .iter()
            .position(which)
            .expect("such a job waiting for its answer");
        self.pending.remove(at).expect("a job where it was found")
    }

    /// Answers every job waiting from `machine`, except those that are `which`, and every
    /// job those answers lead to.
    pub(super) fn run_but(&mut self, machine: &mut Machine, which: fn(&Job) -> bool) {
        let mut kept = VecDeque::new();
        while let Some(job) = self.pending.pop_front() {
            if which(&job) {
                kept.push_back(job);
                continue;
            }
            let answer = machine.answer(job);
            self.give(answer);
        }
        self.pending = kept;
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

pub(super) fn a_switch(job: &Job) -> bool {
    matches!(job, Job::Switch { .. })
}

pub(super) fn holding_ask(job: &Job) -> bool {
    matches!(job, Job::Holding { .. })
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
        let login = self.claude_login(who, percent);
        self.host.live().plant(&live_service(&self.ctx), &login);
        std::fs::write(
            self.root.join(".claude.json"),
            format!(
                r#"{{"oauthAccount":{{"accountUuid":"{who}","emailAddress":"{who}@example.com","organizationUuid":"org-{who}"}}}}"#
            ),
        )
        .expect("Claude Code's config");
        self.elsewhere()
            .enroll_current(label)
            .expect("the account signed in, enrolled");
    }

    /// `who` signed in to Claude Code privately and enrolled as `label`, with its five-hour
    /// window `percent` used, which parks its login and leaves the account in use signed in,
    /// as `pitboard enroll <label> --sign-in` leaves it. No `claude` runs: the sign-in is
    /// planted as one that finished.
    pub(super) fn parked(&self, label: &str, who: &str, percent: f64) {
        let login = pitboard_core::testing::signed_in(
            &self.ctx,
            ProviderId::Claude,
            &self.claude_login(who, percent),
        )
        .expect("a sign-in");
        self.elsewhere()
            .enroll_signed_in(label, login)
            .expect("the account signed in privately, enrolled");
    }

    /// A Claude Code login of `who`, in the shape Claude Code stores one, with Anthropic
    /// scripted to say whose it is and that its five-hour window is `percent` used.
    fn claude_login(&self, who: &str, percent: f64) -> String {
        let now = epoch_now();
        let access = format!("access-{who}");
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
        format!(
            r#"{{"claudeAiOauth":{{"accessToken":"{access}","refreshToken":"refresh-{who}","expiresAt":{},"refreshTokenExpiresAt":{},"scopes":["user:profile","user:inference"]}}}}"#,
            (now + 3_600) * 1_000,
            (now + 30 * 86_400) * 1_000,
        )
    }

    /// Claude Code's write lock taken, as a session writing its login holds it, until what
    /// this returns is dropped: proper-lockfile's directory, `.storage-write.lock` in Claude
    /// Code's config directory, where pitboard-core's `Claude::write_lock` and
    /// `lock::acquire` put it. A switch waits on it for about seven and a half seconds, then
    /// refuses.
    pub(super) fn claude_code_writing(&self) -> Writing {
        let held = self.root.join(".claude").join(".storage-write.lock");
        std::fs::create_dir_all(&held).expect("Claude Code's lock");
        Writing(held)
    }

    /// Codex signed in as `who` and enrolled as `label`, as `codex login` and then
    /// `pitboard enroll codex/<label>` in a terminal leave it.
    pub(super) fn codex_enrolled(&self, label: &str, who: &str) {
        let home = self.root.join(".codex");
        std::fs::create_dir_all(&home).expect("a scratch Codex home");
        self.host
            .file_at(home.join("auth.json"))
            .plant("auth.json", &self.codex_login(who));
        self.elsewhere()
            .enroll_current(&format!("codex/{label}"))
            .expect("the Codex account signed in, enrolled");
    }

    /// `who` signed in to Codex privately and enrolled as `label`, which parks its login and
    /// leaves the account in use signed in, as `pitboard enroll codex/<label> --sign-in`
    /// leaves it. No `codex` runs: the sign-in is planted as one that finished.
    pub(super) fn codex_parked(&self, label: &str, who: &str) {
        let login =
            pitboard_core::testing::signed_in(&self.ctx, ProviderId::Codex, &self.codex_login(who))
                .expect("a sign-in");
        self.elsewhere()
            .enroll_signed_in(&format!("codex/{label}"), login)
            .expect("the Codex account signed in privately, enrolled");
    }

    /// A Codex login of the ChatGPT account `who`, in the shape `codex login` writes one,
    /// with OpenAI scripted to answer for it.
    fn codex_login(&self, who: &str) -> String {
        let now = epoch_now();
        let id_token = unsigned_token(&format!(
            r#"{{"email":"{who}@example.com","exp":{},"https://api.openai.com/auth":{{"chatgpt_account_id":"{who}","chatgpt_user_id":"user-{who}","chatgpt_plan_type":"pro"}}}}"#,
            now + 3_600
        ));
        let access = unsigned_token(&format!(
            r#"{{"exp":{},"for":"refresh-{who}"}}"#,
            now + 10 * 86_400
        ));
        self.api.using(
            &access,
            pitboard_core::usage::Snapshot {
                windows: Vec::new(),
                observed_at: Some(now),
                account_uuid: None,
                source: pitboard_core::usage::Source::Live,
            },
        );
        format!(
            r#"{{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{{"id_token":"{id_token}","access_token":"{access}","refresh_token":"refresh-{who}","account_id":"{who}"}},"last_refresh":"2026-10-01T08:00:00Z"}}"#
        )
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

/// Claude Code's write lock, held until this is dropped.
pub(super) struct Writing(PathBuf);

impl Drop for Writing {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn epoch_now() -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after 1970")
        .as_secs();
    i64::try_from(now).expect("before 2262")
}

/// A token in the shape a real one has, with a signature nothing checks, as Codex's login
/// carries its claims: three parts of unpadded base64url.
fn unsigned_token(payload: &str) -> String {
    [r#"{"alg":"RS256"}"#, payload, "not a real signature"]
        .map(|part| base64url(part.as_bytes()))
        .join(".")
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let held = chunk.iter().enumerate().fold(0u32, |held, (at, &byte)| {
            held | (u32::from(byte) << (16 - 8 * at))
        });
        for at in 0..(chunk.len() * 8).div_ceil(6) {
            let sextet = usize::try_from((held >> (18 - 6 * at)) & 0x3f).expect("six bits");
            out.push(char::from(ALPHABET[sextet]));
        }
    }
    out
}
