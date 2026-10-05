//! Where the model's jobs run, and the thread that tells an app what changed.
//!
//! Each lane is a thread of its own that takes jobs in the order they come and answers each
//! with [`Msg::Done`], as the Swift service's queues did. `reads` is the one queue of reads,
//! so a look for changes waits behind a read on the network exactly as it did there.
//! `changes` makes one change at a time, so a read answers while a switch waits on a lock or
//! on a service. `processes` asks what holds a tool's login, which lists processes and
//! nothing else, and asks the app's own code about other apps, waiting while one is given its
//! time to quit. `discovery` asks which tools are installed, which can wait on the person's
//! login shell, and is needed before the first read can say anything useful about it, so it
//! waits behind nothing. A lane ends once the actor has gone and its last job is done.

use super::state::{Answer, Cadence, Job, Msg, QuitOutcome, split};
use super::{AppControl, ModelListener, QuitQuestion, Snapshot};
use crate::{Holding, Pitboard, Remedy};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

/// Which lane a job runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lane {
    Reads,
    Changes,
    Processes,
    Discovery,
}

impl Job {
    /// The lane `self` runs on, as the Swift service put each call on its queue.
    pub(crate) fn lane(&self) -> Lane {
        match self {
            Job::Read { .. } | Job::ReadOffline { .. } | Job::Look => Lane::Reads,
            Job::Switch { .. } | Job::Abandon => Lane::Changes,
            Job::Holding { .. } | Job::Quit { .. } | Job::Open { .. } => Lane::Processes,
            Job::AskInstalled => Lane::Discovery,
        }
    }
}

/// The lanes, as the actor hands them jobs. Dropped, they end.
pub(crate) struct Lanes {
    reads: Sender<Job>,
    changes: Sender<Job>,
    processes: Sender<Job>,
    discovery: Sender<Job>,
}

impl Lanes {
    /// Starts the lanes over `core` and `apps`, answering to `answers`, giving an app as
    /// long to quit as `cadence` says.
    pub(crate) fn open(
        core: Arc<Pitboard>,
        apps: Arc<dyn AppControl>,
        cadence: Cadence,
        answers: &Sender<Msg>,
    ) -> Lanes {
        let worker = Arc::new(Worker {
            core,
            apps,
            quit_within: cadence.quit_within,
            quit_checked_every: cadence.quit_checked_every,
        });
        Lanes {
            reads: lane("pitboard-reads", &worker, answers),
            changes: lane("pitboard-changes", &worker, answers),
            processes: lane("pitboard-processes", &worker, answers),
            discovery: lane("pitboard-discovery", &worker, answers),
        }
    }

    /// Hands `job` to its lane, and waits for nothing.
    pub(crate) fn run(&self, job: Job) {
        let lane = match job.lane() {
            Lane::Reads => &self.reads,
            Lane::Changes => &self.changes,
            Lane::Processes => &self.processes,
            Lane::Discovery => &self.discovery,
        };
        // A lane that has gone has nothing left to answer to.
        let _ = lane.send(job);
    }
}

fn lane(name: &str, worker: &Arc<Worker>, answers: &Sender<Msg>) -> Sender<Job> {
    let (jobs, taken) = channel::<Job>();
    let worker = Arc::clone(worker);
    let answers = answers.clone();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            for job in taken {
                // The core does not panic on purpose, and a panic there must not stop every
                // job after it, so it is taken as the job coming to nothing.
                let answer = catch_unwind(AssertUnwindSafe(|| worker.work(job.clone())))
                    .unwrap_or(Answer::Lost(job));
                if answers.send(Msg::Done(answer)).is_err() {
                    break;
                }
            }
        })
        .expect("the system starts a thread for one of the model's lanes");
    jobs
}

/// What every lane works with.
pub(crate) struct Worker {
    pub(crate) core: Arc<Pitboard>,
    pub(crate) apps: Arc<dyn AppControl>,
    pub(crate) quit_within: Duration,
    pub(crate) quit_checked_every: Duration,
}

impl Worker {
    /// Does `job` with the core or the app's own code, either of which may block: the core
    /// on the keychain, a lock, the network or the login shell, and an app given its time to
    /// quit for as long as `quit_within`.
    pub(crate) fn work(&self, job: Job) -> Answer {
        let core = &self.core;
        match job {
            Job::Read { ticket, fresh } => {
                // Before the read: a session can record newer numbers, and a terminal can
                // switch, while the read waits on a service. One job, where the Swift awaited
                // each of the three calls on its queue of reads, so no look runs between
                // them.
                let readings_before = core.readings_changed_at();
                let changed_before = core.changed_at();
                Answer::Read {
                    ticket,
                    readings_before,
                    changed_before,
                    read: core.status(fresh),
                }
            }
            Job::ReadOffline { why } => Answer::ReadOffline {
                why,
                read: core.status_offline(),
            },
            Job::Look => Answer::Looked {
                changed: core.changed_at(),
                measured: core.readings_changed_at(),
            },
            Job::AskInstalled => Answer::Installed {
                tools: core.installed(),
            },
            Job::Holding { qualified } => {
                let holdings = core.holding(split(&qualified).0.to_owned());
                Answer::Held {
                    question: holder(&holdings, self.apps.as_ref(), &qualified),
                    qualified,
                }
            }
            Job::Quit { question } => Answer::Quit {
                outcome: quit(
                    self.apps.as_ref(),
                    &question.app_id,
                    self.quit_within,
                    self.quit_checked_every,
                ),
                question,
            },
            Job::Switch { qualified, reopen } => Answer::Switched {
                done: core.switch_to(qualified.clone()),
                qualified,
                reopen,
            },
            Job::Open { location } => {
                // An app that cannot be opened again is the person's to open: nothing here
                // can mend it, and the switch is made either way.
                let _ = self.apps.reopen(location);
                Answer::Opened
            }
            Job::Abandon => Answer::Abandoned(core.abandon_recovery()),
        }
    }
}

/// An app on this machine running the tool `qualified` is of with its login in memory, that
/// Pitboard may quit and open again, as the question to ask about it. Asked of the core,
/// which read the process list into `holdings`, and only taken where the app is running:
/// what is left of one that has gone cannot be quit. An app whose own code cannot say
/// whether it runs is asked about, since the process list says it does.
pub(crate) fn holder(
    holdings: &[Holding],
    apps: &dyn AppControl,
    qualified: &str,
) -> Option<QuitQuestion> {
    holdings.iter().find_map(|held| match &held.remedy {
        Remedy::ReopenApp { bundle_id, name } => {
            let running = !matches!(apps.running(bundle_id.clone()), Ok(None));
            running.then(|| QuitQuestion {
                qualified: qualified.to_owned(),
                app_id: bundle_id.clone(),
                name: name.clone(),
            })
        }
        Remedy::Restart | Remedy::Run { .. } | Remedy::Do { .. } => None,
    })
}

/// Asks the app `app` to quit and waits until it has, for as long as `within`, asking every
/// `every` whether it still runs. One not running is not asked. An app whose own code fails
/// to answer is taken as still running, and one it fails to ask is still running, so nothing
/// is switched under it.
pub(crate) fn quit(
    apps: &dyn AppControl,
    app: &str,
    within: Duration,
    every: Duration,
) -> QuitOutcome {
    let copy = match apps.running(app.to_owned()) {
        Ok(Some(copy)) => copy,
        Ok(None) => return QuitOutcome::NotRunning,
        Err(_) => return QuitOutcome::StillRunning,
    };
    if apps.request_quit(app.to_owned()).is_err() {
        return QuitOutcome::StillRunning;
    }
    let deadline = Instant::now() + within;
    while !matches!(apps.running(app.to_owned()), Ok(None)) {
        let now = Instant::now();
        if now >= deadline {
            return QuitOutcome::StillRunning;
        }
        std::thread::sleep(every.min(deadline - now));
    }
    QuitOutcome::Quit { copy }
}

/// Starts the one thread that calls `listener`, with each snapshot sent to the returned
/// sender, in the order they were sent. One thread, so revisions arrive in the order they
/// were made, which several threads calling at once would not keep.
///
/// Where several are waiting, only the newest is told: an app shows the newest and drops
/// the rest. Once `stopped` is set nothing more is told. The thread ends once the sender has
/// gone, and drops the listener then.
pub(crate) fn notifier(
    listener: Arc<dyn ModelListener>,
    stopped: Arc<AtomicBool>,
) -> Sender<Snapshot> {
    let (tell, told) = channel::<Snapshot>();
    std::thread::Builder::new()
        .name("pitboard-notifier".into())
        .spawn(move || notify(listener.as_ref(), &told, &stopped))
        .expect("the system starts a thread for the model's listener");
    tell
}

fn notify(listener: &dyn ModelListener, told: &Receiver<Snapshot>, stopped: &AtomicBool) {
    while let Ok(mut snapshot) = told.recv() {
        while let Ok(newer) = told.try_recv() {
            snapshot = newer;
        }
        if stopped.load(Ordering::SeqCst) {
            return;
        }
        // What the app could not take in is the app's to say; the next snapshot comes all
        // the same.
        let _ = listener.changed(snapshot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Intent;
    use crate::model::state::{Cadence, Now, State};
    use crate::model::testing::{CHATGPT_CODEX, StandInApps, World, chatgpt_holding};

    const CHATGPT: &str = crate::model::testing::CHATGPT;

    fn worker(core: Arc<Pitboard>, apps: Arc<StandInApps>) -> Worker {
        Worker {
            core,
            apps,
            quit_within: Duration::from_millis(50),
            quit_checked_every: Duration::from_millis(5),
        }
    }

    /// The read the state asks for first, once what is installed is in.
    fn a_read(worker: &Worker) -> Job {
        let mut state = State::new(Cadence::APP);
        let now = Now {
            running: Duration::ZERO,
            epoch_ms: 0,
        };
        state.apply(Msg::Intent(Intent::Refresh { asked: false }), now);
        let answer = worker.work(Job::AskInstalled);
        let mut jobs = state.apply(Msg::Done(answer), now);
        jobs.pop().expect("a read")
    }

    /// A read takes when the readings and the index were last written as they stood before
    /// it, though the read itself records what it measured: taken after, a session's newer
    /// numbers recorded while the read waited counted as seen though nothing showed them.
    #[test]
    fn a_read_takes_the_stamps_as_they_stood_before_it() {
        let world = World::new("stamps");
        world.enrolled("work", "here", 10.0);
        let core = world.core();
        let worker = worker(Arc::clone(&core), StandInApps::new(&[], true));
        let job = a_read(&worker);
        let (readings, changed) = (core.readings_changed_at(), core.changed_at());
        let Answer::Read {
            readings_before,
            changed_before,
            read,
            ..
        } = worker.work(job)
        else {
            panic!("a read answers as one");
        };
        assert!(read.is_ok());
        assert_eq!((readings_before, changed_before), (readings, changed));
        assert!(
            core.readings_changed_at() > readings_before,
            "the read recorded what it measured"
        );
    }

    /// Reads answer while a switch runs, as the Swift service's queue of reads answered
    /// while its queue of changes waited: a switch and giving up are changes, made one at a
    /// time on a lane of their own, and what holds a login and quitting an app are asked on
    /// another, which lists processes and waits on apps and never on the network.
    #[test]
    fn a_switch_runs_on_a_lane_of_its_own() {
        let question = QuitQuestion {
            qualified: "codex/work".into(),
            app_id: CHATGPT.into(),
            name: "ChatGPT".into(),
        };
        let switch = Job::Switch {
            qualified: "codex/work".into(),
            reopen: None,
        };
        assert_eq!(switch.lane(), Lane::Changes);
        assert_eq!(Job::Abandon.lane(), Lane::Changes);
        for job in [
            Job::Holding {
                qualified: "codex/work".into(),
            },
            Job::Quit { question },
            Job::Open {
                location: "/Applications/ChatGPT.app".into(),
            },
        ] {
            assert_eq!(job.lane(), Lane::Processes, "{job:?}");
        }
        assert_eq!(Job::Look.lane(), Lane::Reads);
        assert_eq!(Job::AskInstalled.lane(), Lane::Discovery);
    }

    /// AppModelTests.swift's onlyARunningAppHoldingTheToolIsAskedAbout once more, on the real
    /// core, whose holder detection reads the process list: `switching.rs` has it over the
    /// holdings a test says. What is left of an app that has gone cannot be quit, and a tool
    /// nothing holds is switched at once: ChatGPT holds Codex's login, not Claude Code's.
    #[test]
    fn the_core_says_which_running_app_holds_a_login() {
        let world = World::new("holder");
        world.host.runs_at("codex", &[CHATGPT_CODEX, "codex"]);
        let apps = StandInApps::new(&[CHATGPT], true);
        let worker = worker(world.core(), Arc::clone(&apps));

        let held = |qualified: &str| match worker.work(Job::Holding {
            qualified: qualified.into(),
        }) {
            Answer::Held { question, .. } => question,
            other => panic!("not what holds the login: {other:?}"),
        };
        assert_eq!(
            held("codex/spare"),
            Some(QuitQuestion {
                qualified: "codex/spare".into(),
                app_id: CHATGPT.into(),
                name: "ChatGPT".into(),
            })
        );
        assert_eq!(held("claude/personal"), None, "not Claude Code's login");
        assert_eq!(held("personal"), None, "a bare label is Claude Code's");

        apps.set_running(&[]);
        assert_eq!(held("codex/spare"), None, "gone, so nothing to quit");
        assert!(apps.asked().is_empty(), "and nothing was asked to quit");
    }

    /// A tool whose sessions are started again by a person, or by a command, has nothing
    /// for Pitboard to quit.
    #[test]
    fn only_an_app_pitboard_may_reopen_is_asked_about() {
        let apps = StandInApps::new(&[CHATGPT], true);
        let sessions = Holding {
            kind: "session".into(),
            phrase: "2 `codex` sessions".into(),
            pids: vec![1, 2],
            remedy: Remedy::Restart,
        };
        let daemon = Holding {
            kind: "app_server_daemon".into(),
            phrase: "Codex's background app server".into(),
            pids: vec![3],
            remedy: Remedy::Run {
                command: "codex app-server daemon restart".into(),
            },
        };
        assert_eq!(holder(&[sessions, daemon], apps.as_ref(), "codex/w"), None);
        let found = holder(&[chatgpt_holding()], apps.as_ref(), "codex/w");
        assert_eq!(found.map(|q| q.name), Some("ChatGPT".into()));
    }

    /// An app whose own code cannot say whether it runs is asked about: the core's process
    /// list says it does, and switched under it, it would go on with the account left.
    #[test]
    fn an_app_nobody_can_see_is_taken_as_running() {
        let apps = StandInApps::new(&[], true);
        apps.fail_running(true);
        let found = holder(&[chatgpt_holding()], apps.as_ref(), "codex/w");
        assert_eq!(found.map(|q| q.app_id), Some(CHATGPT.into()));
    }

    /// AppModelTests.swift's anAppIsGivenItsTimeToQuitAndNoMore. An app that quits when
    /// asked is waited for, and the copy that ran is the one to open again; one not running
    /// is not asked; one that does not quit is given its time and no more, and is left
    /// running.
    #[test]
    fn an_app_is_given_its_time_to_quit_and_no_more() {
        let apps = StandInApps::new(&[CHATGPT], true);
        let within = Duration::from_millis(50);
        let every = Duration::from_millis(5);
        assert_eq!(
            quit(apps.as_ref(), CHATGPT, within, every),
            QuitOutcome::Quit {
                copy: StandInApps::copy_of(CHATGPT)
            }
        );
        assert_eq!(
            quit(apps.as_ref(), CHATGPT, within, every),
            QuitOutcome::NotRunning
        );
        assert_eq!(
            apps.asked(),
            [format!("quit {CHATGPT}")],
            "one not running is not asked"
        );

        let busy = StandInApps::new(&[CHATGPT], false);
        let within = Duration::from_millis(100);
        let started = Instant::now();
        assert_eq!(
            quit(busy.as_ref(), CHATGPT, within, every),
            QuitOutcome::StillRunning
        );
        assert!(started.elapsed() >= within);
        assert!(busy.is_running(CHATGPT));
    }

    /// While it waits it asks whether the app still runs every so often, and not more:
    /// thirty seconds at every 200 milliseconds is 150 questions, not a busy loop.
    #[test]
    fn an_app_given_its_time_is_asked_after_it_every_so_often() {
        assert_eq!(Cadence::APP.quit_within, Duration::from_secs(30));
        assert_eq!(Cadence::APP.quit_checked_every, Duration::from_millis(200));
        let busy = StandInApps::new(&[CHATGPT], false);
        let outcome = quit(
            busy.as_ref(),
            CHATGPT,
            Duration::from_millis(100),
            Duration::from_millis(20),
        );
        assert_eq!(outcome, QuitOutcome::StillRunning);
        // Once before asking it to quit, then at 0, 20, 40, 60, 80 and 100 ms, give or take
        // a slow machine's sleeps, which only ever ask less: never fewer than once before
        // and once at the end.
        let asked = busy.running_calls();
        assert!((2..=7).contains(&asked), "asked {asked} times");
    }

    /// An app whose own code fails is still running as far as Pitboard can tell, so nothing
    /// is switched under it: failing to say where it runs, failing to ask it, and failing to
    /// say it has gone all end as an app still open.
    #[test]
    fn an_app_nobody_can_ask_or_see_go_is_still_running() {
        let every = Duration::from_millis(5);
        let within = Duration::from_millis(30);
        let unseen = StandInApps::new(&[CHATGPT], true);
        unseen.fail_running(true);
        assert_eq!(
            quit(unseen.as_ref(), CHATGPT, within, every),
            QuitOutcome::StillRunning
        );
        assert!(
            unseen.asked().is_empty(),
            "nothing asked of an app not seen"
        );

        let unasked = StandInApps::new(&[CHATGPT], true);
        unasked.fail_quit(true);
        assert_eq!(
            quit(unasked.as_ref(), CHATGPT, within, every),
            QuitOutcome::StillRunning
        );
        assert!(unasked.is_running(CHATGPT));
    }

    /// An app is opened again through the app's own code, where `running` said it was, and
    /// one that cannot be opened is not the switch's failure.
    #[test]
    fn opening_again_goes_through_the_apps_own_code() {
        let apps = StandInApps::new(&[], true);
        let world = World::new("open");
        let worker = worker(world.core(), Arc::clone(&apps));
        let answer = worker.work(Job::Open {
            location: StandInApps::copy_of(CHATGPT),
        });
        assert!(matches!(answer, Answer::Opened));
        assert_eq!(apps.asked(), [format!("open {CHATGPT}")]);

        apps.fail_open(true);
        let answer = worker.work(Job::Open {
            location: StandInApps::copy_of(CHATGPT),
        });
        assert!(matches!(answer, Answer::Opened));
    }
}
