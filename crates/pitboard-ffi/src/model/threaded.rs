//! The model as an app has it: its mailbox, its actor, its lanes and its listener, over the
//! real core on a machine of the test's own. What these prove is what no test of
//! `State::apply` can: that snapshots reach the listener one at a time and in order, that a
//! listener may call the model back, stopping it too, and that stopping or dropping the
//! model ends every thread it started, a drop waiting on none of them.

use super::state::Cadence;
use super::testing::World;
use super::{Intent, ModelListener, PitboardModel, PlatformError, Snapshot};
use crate::Pitboard;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

/// As long as a test waits for another thread, however slow the machine running it.
const PATIENCE: Duration = Duration::from_secs(20);

/// Looks every few milliseconds, and reads by itself only when started, once.
const QUICK: Cadence = Cadence {
    look_every: Duration::from_millis(10),
    read_every: Duration::from_secs(3_600),
    stale_after: Duration::from_secs(60),
};

/// What a listener does with the model from inside `changed`.
type Inside = Box<dyn FnOnce(&PitboardModel) + Send>;

/// Keeps every snapshot it is told of, and can call the model back from inside `changed`.
#[derive(Default)]
struct Told {
    snapshots: Mutex<Vec<Snapshot>>,
    arrived: Condvar,
    /// The model, for a listener that calls it back.
    model: OnceLock<Weak<PitboardModel>>,
    /// Done once, from inside the first `changed` told of a read that has landed, where a
    /// test says so.
    once_read: Mutex<Option<Inside>>,
    /// The revision `snapshot` gave from inside `changed`, each time, beside the one told.
    asked_inside: Mutex<Vec<(u64, u64)>>,
    /// Told the revision of each call as it starts, before it is held, where a test says so.
    entered: Mutex<Option<Sender<u64>>>,
    /// Holds each call until the test lets it go, where a test says so.
    hold: Mutex<Option<Receiver<()>>>,
}

impl ModelListener for Told {
    fn changed(&self, snapshot: Snapshot) -> Result<(), PlatformError> {
        if let Some(model) = self.model.get().and_then(Weak::upgrade) {
            let inside = model.snapshot().revision;
            self.asked_inside
                .lock()
                .expect("a test's own lock")
                .push((snapshot.revision, inside));
            if !snapshot.reading
                && snapshot.status.is_some()
                && let Some(once) = self.once_read.lock().expect("a test's own lock").take()
            {
                once(&model);
            }
        }
        if let Some(entered) = self.entered.lock().expect("a test's own lock").as_ref() {
            let _ = entered.send(snapshot.revision);
        }
        if let Some(hold) = self.hold.lock().expect("a test's own lock").as_ref() {
            let _ = hold.recv();
        }
        self.snapshots
            .lock()
            .expect("a test's own lock")
            .push(snapshot);
        self.arrived.notify_all();
        Ok(())
    }
}

impl Told {
    /// Waits until the snapshots told so far satisfy `done`, and says what they were.
    fn until(&self, what: &str, done: impl Fn(&[Snapshot]) -> bool) -> Vec<Snapshot> {
        let started = Instant::now();
        let mut snapshots = self.snapshots.lock().expect("a test's own lock");
        while !done(&snapshots) {
            let left = PATIENCE
                .checked_sub(started.elapsed())
                .unwrap_or_else(|| panic!("never told {what}: {snapshots:#?}"));
            snapshots = self
                .arrived
                .wait_timeout(snapshots, left)
                .expect("a test's own lock")
                .0;
        }
        snapshots.clone()
    }

    fn count(&self) -> usize {
        self.snapshots.lock().expect("a test's own lock").len()
    }
}

/// Waits until `done` says so, checking every few milliseconds.
fn eventually(what: &str, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < PATIENCE, "never {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn labels(snapshot: &Snapshot) -> Vec<String> {
    snapshot
        .status
        .iter()
        .flat_map(|status| &status.accounts)
        .filter_map(|account| account.label.clone())
        .collect()
}

/// A read that has landed and is over, naming `label`.
fn shows(label: &str) -> impl Fn(&[Snapshot]) -> bool + '_ {
    move |told| {
        told.last()
            .is_some_and(|last| !last.reading && labels(last) == [label])
    }
}

fn in_order(told: &[Snapshot]) -> bool {
    told.windows(2)
        .all(|pair| pair[0].revision < pair[1].revision)
}

/// A model over `core` telling `told`, which may call it back.
fn model(core: &Arc<Pitboard>, told: &Arc<Told>) -> Arc<PitboardModel> {
    let model = PitboardModel::over(
        Arc::clone(core),
        Arc::clone(told) as Arc<dyn ModelListener>,
        QUICK,
    );
    let _ = told.model.set(Arc::downgrade(&model));
    model
}

fn last_revision(told: &Told) -> Option<u64> {
    told.snapshots
        .lock()
        .expect("a test's own lock")
        .last()
        .map(|last| last.revision)
}

/// The model reads the accounts once started, through its lanes over the real core, and
/// tells its listener each snapshot in order, the last of them the one `snapshot` gives.
/// Before it is started it shows nothing and asks nothing.
#[test]
fn a_started_model_reads_the_accounts_and_tells_its_listener_in_order() {
    let world = World::new("reads");
    world.enrolled("work", "here", 42.0);
    let asked = world.api.calls();
    let told = Arc::new(Told::default());
    let model = model(&world.core(), &told);
    let first = model.snapshot();
    assert_eq!((first.revision, first.status.as_ref()), (0, None));
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(told.count(), 0, "nothing runs before Start");
    assert_eq!(world.api.calls(), asked);

    model.send(Intent::Start);
    let snapshots = told.until("the account read", shows("work"));
    assert!(in_order(&snapshots), "{snapshots:#?}");
    let last = snapshots.last().expect("a snapshot");
    assert_eq!(last.installed.as_ref().map(Vec::len), Some(1));
    let percent = last.status.as_ref().expect("accounts").accounts[0]
        .usage
        .as_ref()
        .expect("numbers")
        .windows[0]
        .percent;
    assert_eq!(percent, 42.0);
    eventually("told the newest snapshot", || {
        last_revision(&told) == Some(model.snapshot().revision)
    });
    model.shutdown();
}

/// A change another front end makes, here a rename typed in a terminal, reaches the
/// listener within a look or two, read from what is already known: nobody is asked.
#[test]
fn a_change_another_front_end_makes_is_told_without_asking_anyone() {
    let world = World::new("elsewhere");
    world.enrolled("work", "here", 10.0);
    let told = Arc::new(Told::default());
    let model = model(&world.core(), &told);
    model.send(Intent::Start);
    told.until("the account read", shows("work"));

    world
        .elsewhere()
        .rename("work", "office")
        .expect("renamed in a terminal");
    world.index_written_later(5);
    let asked = world.api.calls();
    told.until("the rename", shows("office"));
    assert_eq!(world.api.calls(), asked, "nobody was asked");
    model.shutdown();
}

/// A listener may ask for the snapshot and send an intent from inside `changed` without
/// the model waiting on itself: `snapshot` is a lock the model never holds while it calls
/// out, and `send` only posts. The snapshot it is given there is never older than the one
/// it is being told of.
#[test]
fn a_listener_may_call_the_model_while_it_is_told() {
    let world = World::new("inside");
    world.enrolled("work", "here", 10.0);
    let told = Arc::new(Told::default());
    // Sent once the start's read has landed, when nothing else asks Anthropic: a read
    // somebody asked for asks it whatever it was asked moments ago, so only that read can
    // make more calls than there were then.
    let sent_at = Arc::new(OnceLock::new());
    let api = Arc::clone(&world.api);
    let at = Arc::clone(&sent_at);
    *told.once_read.lock().expect("a test's own lock") = Some(Box::new(move |model| {
        let _ = at.set(api.calls());
        model.send(Intent::Refresh { asked: true });
    }));
    let model = model(&world.core(), &told);
    model.send(Intent::Start);
    eventually("the read sent from inside the listener", || {
        sent_at
            .get()
            .is_some_and(|&sent| world.api.calls() > sent && !model.snapshot().reading)
    });
    told.until("the read it asked for", shows("work"));
    let inside = told.asked_inside.lock().expect("a test's own lock").clone();
    assert!(!inside.is_empty());
    assert!(
        inside.iter().all(|(being_told, given)| given >= being_told),
        "{inside:?}"
    );
    model.shutdown();
}

/// Intents sent from many threads at once, each starting a read, make many snapshots, and
/// they reach the listener in order and one at a time, ending with the newest.
#[test]
fn snapshots_made_while_many_threads_send_arrive_in_order() {
    let world = World::new("many");
    world.enrolled("work", "here", 10.0);
    let told = Arc::new(Told::default());
    let model = model(&world.core(), &told);
    let senders: Vec<_> = (0..4)
        .map(|_| {
            let model = Arc::clone(&model);
            std::thread::spawn(move || {
                for _ in 0..25 {
                    model.send(Intent::Refresh { asked: false });
                }
            })
        })
        .collect();
    for sender in senders {
        sender.join().expect("a thread that sends");
    }
    let snapshots = told.until("the last read over", |told| {
        told.last()
            .is_some_and(|last| !last.reading && last.revision == model.snapshot().revision)
    });
    assert!(in_order(&snapshots), "{snapshots:#?}");
    model.shutdown();
}

/// `shutdown` stops everything the model started: the listener is told nothing more, a
/// change made elsewhere is not looked for, and the actor, the lanes and the notifier end,
/// letting go of the listener and the core. The model still answers `snapshot`, and `send`
/// comes to nothing.
#[test]
fn shutdown_stops_the_timers_the_lanes_and_the_listener() {
    let world = World::new("shutdown");
    world.enrolled("work", "here", 10.0);
    let core = world.core();
    let told = Arc::new(Told::default());
    let model = model(&core, &told);
    model.send(Intent::Start);
    told.until("the account read", shows("work"));

    model.shutdown();
    let count = told.count();
    eventually("the notifier let go of the listener", || {
        Arc::strong_count(&told) == 1
    });
    eventually("the lanes let go of the core", || {
        Arc::strong_count(&core) == 1
    });

    world
        .elsewhere()
        .rename("work", "office")
        .expect("renamed in a terminal");
    world.index_written_later(5);
    model.send(Intent::Refresh { asked: true });
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(told.count(), count, "told nothing after shutdown");
    assert_eq!(labels(&model.snapshot()), ["work"]);
    model.shutdown();
}

/// A listener may stop the model from inside `changed`: `shutdown` waits for the actor alone,
/// which never waits on the listener, and not for the thread it is called on. The call it is
/// made from is the last the listener is told of, and every thread ends.
#[test]
fn a_listener_may_shut_the_model_down_while_it_is_told() {
    let world = World::new("shut-inside");
    world.enrolled("work", "here", 10.0);
    let core = world.core();
    let told = Arc::new(Told::default());
    let (shut, was_shut) = channel();
    *told.once_read.lock().expect("a test's own lock") = Some(Box::new(move |model| {
        model.shutdown();
        let _ = shut.send(());
    }));
    let model = model(&core, &told);
    model.send(Intent::Start);
    was_shut
        .recv_timeout(PATIENCE)
        .expect("shut down from inside the listener");

    eventually("the notifier let go of the listener", || {
        Arc::strong_count(&told) == 1
    });
    eventually("the lanes let go of the core", || {
        Arc::strong_count(&core) == 1
    });
    let snapshots = told.snapshots.lock().expect("a test's own lock").clone();
    let read = snapshots
        .iter()
        .position(|snapshot| !snapshot.reading && snapshot.status.is_some());
    assert_eq!(
        read,
        Some(snapshots.len() - 1),
        "told nothing after the call that shut it down: {snapshots:#?}"
    );
    model.shutdown();
}

/// Dropping the model waits for nothing, since .NET's finalizer thread can be the one dropping
/// it: not for its listener, held up here inside a call, nor for its actor, held up here
/// making a snapshot. A drop that joined either thread would wait until the test let it go.
/// Once let go, the threads end, letting go of the listener and the core.
#[test]
fn dropping_the_model_waits_for_nothing() {
    let world = World::new("drop");
    world.enrolled("work", "here", 10.0);
    let core = world.core();
    let told = Arc::new(Told::default());
    let (release, held): (Sender<()>, Receiver<()>) = channel();
    *told.hold.lock().expect("a test's own lock") = Some(held);
    let (entered, inside) = channel();
    *told.entered.lock().expect("a test's own lock") = Some(entered);
    // Not given the model, so the test's is the last of it.
    let model = PitboardModel::over(
        Arc::clone(&core),
        Arc::clone(&told) as Arc<dyn ModelListener>,
        QUICK,
    );
    model.send(Intent::Start);
    inside
        .recv_timeout(PATIENCE)
        .expect("the listener held up inside a call");

    // The actor takes this lock after every message it takes, and before the stop the drop
    // sends it takes the intent sent while the test holds it.
    let shown = Arc::clone(&model.shown);
    let making = super::lock(&shown);
    model.send(Intent::Refresh { asked: false });

    let (dropped, done) = channel();
    std::thread::spawn(move || {
        drop(model);
        let _ = dropped.send(());
    });
    done.recv_timeout(Duration::from_secs(5))
        .expect("dropped without waiting for the actor or the listener");
    assert_eq!(told.count(), 0, "the listener is still held up");

    drop(making);
    drop(release);
    eventually("the notifier let go of the listener", || {
        Arc::strong_count(&told) == 1
    });
    eventually("the lanes let go of the core", || {
        Arc::strong_count(&core) == 1
    });
}
