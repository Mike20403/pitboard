//! The app's model in Rust: what both apps show, and every decision about it.
//!
//! An app makes one [`PitboardModel`], sends it what the person or the system asked for as
//! an [`Intent`], and shows the [`Snapshot`]s its [`ModelListener`] is told of. Nothing it
//! exports is async, and nothing it exports waits on the core: `send` puts an intent in the
//! model's mailbox and waits for nothing, `snapshot` takes the lock the actor holds only
//! while it compares and copies a snapshot, and `shutdown` waits for the actor to take the
//! messages already in its mailbox, none of which waits on anything.
//!
//! Inside, one thread, the actor, owns the `State` and nothing else touches it. It takes
//! each message in turn, has `State::apply` say what follows, which does no I/O at all, and
//! runs the jobs that returns on the lanes in `lanes.rs`, whose answers come back to it as
//! messages, so a read still answers while a switch waits on the core or an app is given its
//! time to quit. Between messages it waits until the next timer is due. After
//! each message it makes the snapshot, and where that differs from the last one it numbers
//! it one higher and hands it to the one notifier thread, which tells the listener in order.
//!
//! This part reads accounts, notices changes made elsewhere, switches, quits an app that
//! holds a login when asked to, and keeps what each tool's last switch said. Signing in and
//! what the window says are still the Swift model's, and come here after it.

mod lanes;
mod state;

#[cfg(test)]
mod cadence;
#[cfg(test)]
mod reading;
#[cfg(test)]
mod switching;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod threaded;

use crate::{Abandoned, Pitboard, Status, Tool, Warning};
use lanes::Lanes;
use state::{Cadence, Msg, Now, State};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// What the app was started with, which the model reads as the command line reads its own:
/// the environment, and where the app is. Not called `Launch`, which the macOS app already
/// has a type of.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AppLaunch {
    /// The environment the app was started with, every variable of it.
    pub environment: HashMap<String, String>,
    /// Where the app is, as `Pitboard::for_app` takes it: on macOS the `.app`, which names
    /// the command line inside it that the renewal schedule runs. `None` for anything that
    /// is not an app, such as a test or a build directory.
    pub app_location: Option<String>,
}

/// Something the person or the system asked the model for.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Intent {
    /// Start what runs by itself: a read now and every five minutes, and a look every two
    /// seconds for a change made somewhere else, which reads what is already known. Until
    /// an app sends this, nothing runs by itself. Sent again, it starts nothing more.
    Start,
    /// The machine woke from sleep: numbers read before it slept say nothing about now. An
    /// app sends it every time: on macOS the model's timers count only the time the machine
    /// is awake, so without it the timer's read after a long sleep comes up to five minutes
    /// late.
    Woke,
    /// Somebody looked, by opening the app's menu: the accounts are read again where the
    /// numbers shown are a minute old.
    Glanced,
    /// Read the accounts again. `asked` is somebody asking, from a Refresh button, rather
    /// than something producing it, and has each tool's service asked about every account
    /// whatever it was asked moments ago.
    Refresh { asked: bool },
    /// Switch to the account `qualified` names, its label with its tool as
    /// `Account::qualified` gives it, from the window, the menu or a notification. One switch
    /// at a time: asked for while another is under way, it does nothing, beyond bringing a
    /// question about quitting an app back to the front. Where an app keeps the tool's login
    /// in memory, as ChatGPT keeps Codex's, the switch waits on `Snapshot::quit_question`.
    SwitchTo { qualified: String },
    /// The person let Pitboard quit the app the quit question for the switch to `qualified`
    /// names: it is asked to quit the way a person quits it, the switch is made once it has,
    /// and the copy that was running is opened again whether or not the switch worked. An
    /// app still open after thirty seconds stops the switch before anything has changed.
    ///
    /// It names the question it answers, as AppModel.swift's quitAndSwitch took it, because
    /// an alert can say it closed before its button acts: it is taken before or after
    /// `KeepAppOpen` closed that question, and does nothing once taken, or once another
    /// switch has been asked for in that question's place.
    QuitAndSwitch { qualified: String },
    /// The person kept the app open, or the question was closed some other way: the quit
    /// question goes, and nothing is switched. The answer of the button that closed it, sent
    /// after this, still switches.
    KeepAppOpen,
    /// Somebody has read what `provider`'s tool's last switch said, and put it away.
    DismissSwitch { provider: String },
    /// Give up on an interrupted switch nothing can finish, keeping every login it names.
    AbandonStuckSwitch,
    /// Somebody has read what giving up on an interrupted switch kept, and put it away.
    DismissAbandoned,
}

/// A switch waiting for the person to let Pitboard quit an app first: the app keeps the
/// tool's login in memory, would go on with the account switched away from, and its own
/// sign-out would revoke the login Pitboard had just parked.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct QuitQuestion {
    /// The account to switch to, its label with its tool.
    pub qualified: String,
    /// The app, as the core's holder detection names it and `AppControl` takes it: on macOS
    /// its bundle id.
    pub app_id: String,
    /// The app as a person knows it: "ChatGPT".
    pub name: String,
}

/// What a switch means for a tool's running sessions, for a tool whose sessions never pick
/// a switch up: they keep the account they started with until they are started again.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RestartNeeded {
    /// The command a person quits and starts again: `codex`.
    pub program: String,
    /// The account they keep using, by its label alone, since the tool is named already.
    pub from: String,
}

/// What a tool's last switch said that the read after it does not say again, until the
/// tool no longer has the account it switched to signed in, or somebody puts it away. One
/// per tool: a switch of one tool says nothing about another's sessions.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LastSwitch {
    /// The tool, as a `Tool`'s code.
    pub provider: String,
    /// The account switched to, as the core types it: bare for Claude Code, `codex/work`
    /// for any other tool.
    pub to: String,
    /// When sessions already open will have picked it up, in epoch seconds, for a tool that
    /// follows a switch by itself.
    pub follows_at: Option<i64>,
    /// For a tool whose running sessions never pick a switch up.
    pub restart: Option<RestartNeeded>,
    /// What the switch warned about, every warning of it.
    pub warnings: Vec<Warning>,
}

/// Something a person asked for that did not happen, for the window to say: an alert's
/// title and its message.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Failure {
    /// One higher for each failure said, so an app tells a new one from one it has shown.
    pub id: u64,
    /// What was being done, as an alert's title says it: "Couldn’t switch to personal".
    pub title: String,
    /// What went wrong. Pitboard's errors already say what to do, so it is shown as it is.
    pub message: String,
    /// The stable code behind it, for deciding what to offer, where the core gave one.
    pub code: Option<String>,
    /// Everything else it warned about.
    pub warnings: Vec<Warning>,
}

/// A pane of the main window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Pane {
    Accounts,
    Activity,
    Machine,
}

/// What the model wants of the main window: it is opened each time `serial` moves, on
/// `pane` when that says which. A menu has nowhere to put a sentence or a question, so a
/// failure or a quit question asked for from one is said in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct WindowRequest {
    /// How many times the window has been asked for.
    pub serial: u64,
    /// The pane the last request wants shown, when it wants one.
    pub pane: Option<Pane>,
}

/// Why the last read of the accounts did not answer.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReadFailure {
    /// Stable, for the app to branch on: an offer survives its message changing.
    pub code: String,
    /// Pitboard's own sentence, which already says what to do, to show as it is.
    pub message: String,
}

/// Everything the model has to show, as of one moment.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Snapshot {
    /// One higher than the snapshot made before it. The listener is told only the newest of
    /// those waiting, and `snapshot` can give one it was never told, so an app can see 3 and
    /// then 5. It keeps the snapshot with the highest it has seen and drops any other.
    pub revision: u64,
    /// When this snapshot was made, in epoch seconds: the moment anything in it that depends
    /// on the time was worked out at. Not every passing second makes a new snapshot.
    pub now: i64,
    /// Whether a read of the accounts is under way, so a Refresh button can say so.
    pub reading: bool,
    /// When the accounts were last read, in epoch seconds. `None` before the first read
    /// that answered.
    pub updated_at: Option<i64>,
    /// The accounts, as the last read gave them or with newer numbers some session has
    /// recorded since, or the last numbers measured where no read has answered yet. `None`
    /// before anything is known, which is not a machine without accounts.
    pub status: Option<Status>,
    /// Everything the last read warned about, or what its failure did, and before those what
    /// a switch that failed since warned about.
    pub warnings: Vec<Warning>,
    /// Why the last read did not answer, when it did not. `status` is then the last
    /// numbers measured.
    pub read_failure: Option<ReadFailure>,
    /// An interrupted switch nothing can finish, which the app offers a way out of.
    pub stuck: bool,
    /// The tools whose program was found, in the order a listing shows them, once the
    /// first read has asked. A tool missing here may still be on some `PATH`, so this
    /// narrows what is offered and never forbids anything.
    pub installed: Option<Vec<Tool>>,
    /// The account a switch is under way for, or waiting on the quit question for, as its
    /// label with its tool, from the moment it is asked for until the read after it has
    /// landed. Nothing else is switched meanwhile, so the menu and the rows hold back.
    pub switch_under_way: Option<String>,
    /// A switch waiting for the person to let Pitboard quit an app first, to ask in the
    /// window: `Intent::QuitAndSwitch` with its `qualified`, or `Intent::KeepAppOpen`,
    /// answers it.
    pub quit_question: Option<QuitQuestion>,
    /// What each tool's last switch said that is still true, one per tool at most: a tool's
    /// newer switch takes the place of what its last said, and one kept once the last was put
    /// away goes after every other tool's. Apart from `warnings`, which the read after every
    /// switch replaces: these are about the switch, and stay true until the tool no longer
    /// has the account it switched to signed in, or somebody puts them away.
    pub last_switches: Vec<LastSwitch>,
    /// What giving up on an interrupted switch kept, until somebody has read it.
    pub abandoned: Option<Abandoned>,
    /// The last thing asked for that did not happen, which an app says once: a newer one
    /// has a higher `id`. It stays here after it is said, until another takes its place.
    pub failure: Option<Failure>,
    /// What the model wants of the main window.
    pub window_request: WindowRequest,
}

/// What a platform's own code could not do. Every method of a trait an app implements
/// returns it, so an exception the app's code throws reaches the model as an error rather
/// than as a crash.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PlatformError {
    #[error("{reason}")]
    Failed { reason: String },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for PlatformError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> PlatformError {
        PlatformError::Failed {
            reason: error.reason,
        }
    }
}

/// Told of every snapshot, by the app.
#[uniffi::export(with_foreign)]
pub trait ModelListener: Send + Sync {
    /// A newer snapshot, on a thread of the model's own, never the app's main thread, and
    /// one call at a time, in order. It may call `snapshot` and `send` on the model, and
    /// should hand the snapshot to the app's main thread and return: the next one waits.
    fn changed(&self, snapshot: Snapshot) -> Result<(), PlatformError>;
}

/// Other apps on this machine, by an app id, which Pitboard may quit and open again around a
/// switch: one that runs a tool for itself and keeps the tool's login in memory while it is
/// open, as ChatGPT does with Codex's. The id is the one the core's holder detection names
/// an app by, which on macOS is its bundle id. What names an app on Windows is still the
/// owner's to decide, and the id is one string so that a Windows id fits.
///
/// Only quitting the way a person quits an app and opening one again are offered: never a
/// forced quit, which would lose whatever the app had not saved. Called on a thread of the
/// model's own, never the app's main thread, one call at a time. An error from `running` or
/// `request_quit` is taken as the app still running, since Pitboard never switches under an
/// app it could not see go; one from `reopen` leaves the app for the person to open.
#[uniffi::export(with_foreign)]
pub trait AppControl: Send + Sync {
    /// Where the running copy of the app was opened from, which is what `reopen` takes, or
    /// `None` when it is not running.
    fn running(&self, app: String) -> Result<Option<String>, PlatformError>;
    /// Asks the app to quit the way a person quits it, which lets it ask about work in
    /// progress. Returns at once; the app may take a while, or decline.
    fn request_quit(&self, app: String) -> Result<(), PlatformError>;
    /// Opens the app at `location` again, as `running` gave it, the way the system's own
    /// shell would, without bringing it to the front: whatever Pitboard has to say about the
    /// switch stays in front of it. Not called `open`, which UniFFI 0.31.2's list of Swift
    /// keywords has: it writes a name on that list in backticks into the C header's table
    /// of calls as well as into the Swift, and no C compiler takes it there.
    fn reopen(&self, location: String) -> Result<(), PlatformError>;
}

/// The app's model. Making one starts its threads and asks nothing of anyone: the core is
/// made when the first read needs it, which can mean asking the person's login shell.
#[derive(uniffi::Object)]
pub struct PitboardModel {
    mailbox: Sender<Msg>,
    shown: Arc<Mutex<Snapshot>>,
    /// Set once the model is stopped, after which the listener is told nothing more.
    stopped: Arc<AtomicBool>,
    actor: Mutex<Option<JoinHandle<()>>>,
}

#[uniffi::export]
impl PitboardModel {
    /// The app's model, over the core the app's environment and location make, read the
    /// way the command line reads its own, as `Pitboard::for_app` makes it, and over `apps`,
    /// the other apps on this machine. Nothing runs by itself until the app sends
    /// `Intent::Start`.
    #[uniffi::constructor]
    pub fn new(
        launch: AppLaunch,
        listener: Arc<dyn ModelListener>,
        apps: Arc<dyn AppControl>,
    ) -> Arc<Self> {
        PitboardModel::over(
            Pitboard::for_app(launch.environment, launch.app_location),
            listener,
            apps,
            Cadence::APP,
        )
    }

    /// The last snapshot, for a first paint and whenever an app wants it. A lock and a
    /// copy, on any thread, the listener's own included.
    pub fn snapshot(&self) -> Snapshot {
        lock(&self.shown).clone()
    }

    /// Asks the model for `intent`. Waits for nothing and calls nothing back: what comes of
    /// it is told to the listener. Once the model has stopped, nothing comes of it.
    pub fn send(&self, intent: Intent) {
        let _ = self.mailbox.send(Msg::Intent(intent));
    }

    /// Stops the model: its timers, its lanes once each has finished what it is doing, and
    /// its listener, which is called no more once this returns, beyond a call already under
    /// way on its own thread. Waits for the actor to end, which it does once it has taken
    /// the messages already in its mailbox, none of which waits on anything, and for nothing
    /// else, so the listener may call it too. For an app to call as it quits: the listener
    /// holds the app, which holds the model, so neither is freed before that.
    pub fn shutdown(&self) {
        self.stop();
        let actor = self.actor.lock().ok().and_then(|mut actor| actor.take());
        if let Some(actor) = actor
            && actor.thread().id() != std::thread::current().id()
        {
            let _ = actor.join();
        }
    }
}

impl PitboardModel {
    /// A model over `core` and `apps`, doing what it does by itself every `cadence`.
    pub(crate) fn over(
        core: Arc<Pitboard>,
        listener: Arc<dyn ModelListener>,
        apps: Arc<dyn AppControl>,
        cadence: Cadence,
    ) -> Arc<PitboardModel> {
        let clock = Clock::starting();
        let state = State::new(cadence);
        let shown = Arc::new(Mutex::new(state.snapshot(0, clock.now().epoch())));
        let stopped = Arc::new(AtomicBool::new(false));
        let (mailbox, mail) = channel();
        let lanes = Lanes::open(core, apps, cadence, &mailbox);
        let actor = Actor {
            state,
            lanes,
            shown: Arc::clone(&shown),
            tell: lanes::notifier(listener, Arc::clone(&stopped)),
            clock,
        };
        let actor = std::thread::Builder::new()
            .name("pitboard-model".into())
            .spawn(move || actor.run(&mail))
            .expect("the system starts a thread for the model");
        Arc::new(PitboardModel {
            mailbox,
            shown,
            stopped,
            actor: Mutex::new(Some(actor)),
        })
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = self.mailbox.send(Msg::Stop);
    }
}

/// Stops the model and waits for nothing: .NET's finalizer thread can be the one dropping
/// it, and must never wait on the model's threads.
impl Drop for PitboardModel {
    fn drop(&mut self) {
        self.stop();
    }
}

fn lock(shown: &Mutex<Snapshot>) -> MutexGuard<'_, Snapshot> {
    // A snapshot is whole whenever the lock is let go, so one held through a panic is still
    // worth showing.
    shown
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The model's two clocks: `Instant` for the timers, which on macOS is `CLOCK_UPTIME_RAW` and
/// stops while the machine sleeps, and the wall clock for what a snapshot says the time is.
struct Clock {
    started: Instant,
}

impl Clock {
    fn starting() -> Clock {
        Clock {
            started: Instant::now(),
        }
    }

    fn now(&self) -> Now {
        let epoch_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
            });
        Now {
            running: self.started.elapsed(),
            epoch_ms,
        }
    }
}

/// The actor thread's own: the state, the lanes, and where snapshots go.
struct Actor {
    state: State,
    lanes: Lanes,
    shown: Arc<Mutex<Snapshot>>,
    tell: Sender<Snapshot>,
    clock: Clock,
}

impl Actor {
    /// Takes each message in turn, waiting no longer than the next timer, until told to
    /// stop. Dropping the lanes and the notifier's sender then ends their threads.
    fn run(mut self, mail: &Receiver<Msg>) {
        loop {
            let msg = match self.state.next_due() {
                None => mail.recv().unwrap_or(Msg::Stop),
                Some(due) => {
                    match mail.recv_timeout(due.saturating_sub(self.clock.now().running)) {
                        Ok(msg) => msg,
                        Err(RecvTimeoutError::Timeout) => Msg::Tick,
                        Err(RecvTimeoutError::Disconnected) => Msg::Stop,
                    }
                }
            };
            if matches!(msg, Msg::Stop) {
                return;
            }
            let now = self.clock.now();
            for job in self.state.apply(msg, now) {
                self.lanes.run(job);
            }
            self.publish(now);
        }
    }

    /// Hands the listener a snapshot where anything in it changed. Compared with the last
    /// one under its revision and time, so neither moving alone makes a new one.
    fn publish(&self, now: Now) {
        let mut shown = lock(&self.shown);
        let made = self.state.snapshot(shown.revision, shown.now);
        if made == *shown {
            return;
        }
        let snapshot = Snapshot {
            revision: shown.revision + 1,
            now: now.epoch(),
            ..made
        };
        *shown = snapshot.clone();
        drop(shown);
        let _ = self.tell.send(snapshot);
    }
}
