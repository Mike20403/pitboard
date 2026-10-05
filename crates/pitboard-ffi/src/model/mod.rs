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
//! messages. Between messages it waits until the next timer is due. After
//! each message it makes the snapshot, and where that differs from the last one it numbers
//! it one higher and hands it to the one notifier thread, which tells the listener in order.
//!
//! This part reads accounts and notices changes made elsewhere. Switching, signing in and
//! what the window says are still the Swift model's, and come here after it.

mod lanes;
mod state;

#[cfg(test)]
mod cadence;
#[cfg(test)]
mod reading;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod threaded;

use crate::{Pitboard, Status, Tool, Warning};
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
    /// Everything the last read warned about, or what its failure did.
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
    /// way the command line reads its own, as `Pitboard::for_app` makes it. Nothing runs by
    /// itself until the app sends `Intent::Start`.
    #[uniffi::constructor]
    pub fn new(launch: AppLaunch, listener: Arc<dyn ModelListener>) -> Arc<Self> {
        PitboardModel::over(
            Pitboard::for_app(launch.environment, launch.app_location),
            listener,
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
    /// A model over `core`, doing what it does by itself every `cadence`.
    pub(crate) fn over(
        core: Arc<Pitboard>,
        listener: Arc<dyn ModelListener>,
        cadence: Cadence,
    ) -> Arc<PitboardModel> {
        let clock = Clock::starting();
        let state = State::new(cadence);
        let shown = Arc::new(Mutex::new(state.snapshot(0, clock.now().epoch())));
        let stopped = Arc::new(AtomicBool::new(false));
        let (mailbox, mail) = channel();
        let lanes = Lanes::open(&core, &mailbox);
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
