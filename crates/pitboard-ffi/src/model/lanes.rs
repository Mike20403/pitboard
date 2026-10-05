//! Where the model's jobs run, and the thread that tells an app what changed.
//!
//! Each lane is a thread of its own that takes jobs in the order they come and answers each
//! with [`Msg::Done`], as the Swift service's queues did. `reads` is the one queue of reads,
//! so a look for changes waits behind a read on the network exactly as it did there.
//! `discovery` asks which tools are installed, which can wait on the person's login shell,
//! and is needed before the first read can say anything useful about it, so it waits
//! behind nothing. A lane ends once the actor has gone and its last job is done.

use super::state::{Answer, Job, Msg};
use super::{ModelListener, Snapshot};
use crate::Pitboard;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

/// The lanes, as the actor hands them jobs. Dropped, they end.
pub(crate) struct Lanes {
    reads: Sender<Job>,
    discovery: Sender<Job>,
}

impl Lanes {
    /// Starts the lanes over `core`, answering to `answers`.
    pub(crate) fn open(core: &Arc<Pitboard>, answers: &Sender<Msg>) -> Lanes {
        Lanes {
            reads: lane("pitboard-reads", core, answers),
            discovery: lane("pitboard-discovery", core, answers),
        }
    }

    /// Hands `job` to its lane, and waits for nothing.
    pub(crate) fn run(&self, job: Job) {
        let lane = match job {
            Job::AskInstalled => &self.discovery,
            Job::Read { .. } | Job::ReadOffline { .. } | Job::Look => &self.reads,
        };
        // A lane that has gone has nothing left to answer to.
        let _ = lane.send(job);
    }
}

fn lane(name: &str, core: &Arc<Pitboard>, answers: &Sender<Msg>) -> Sender<Job> {
    let (jobs, taken) = channel::<Job>();
    let core = Arc::clone(core);
    let answers = answers.clone();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            for job in taken {
                // The core does not panic on purpose, and a panic there must not stop every
                // job after it, so it is taken as the job coming to nothing.
                let answer = catch_unwind(AssertUnwindSafe(|| work(&core, job.clone())))
                    .unwrap_or(Answer::Lost(job));
                if answers.send(Msg::Done(answer)).is_err() {
                    break;
                }
            }
        })
        .expect("the system starts a thread for one of the model's lanes");
    jobs
}

/// Does `job` with the core, which may block on the keychain, a lock, the network or the
/// login shell.
fn work(core: &Pitboard, job: Job) -> Answer {
    match job {
        Job::Read { ticket, fresh } => {
            // Before the read: a session can record newer numbers, and a terminal can switch,
            // while the read waits on a service. One job, where the Swift awaited each of the
            // three calls on its queue of reads, so no look runs between them.
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
    }
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
    use crate::model::testing::World;
    use std::time::Duration;

    /// The read the state asks for first, once what is installed is in.
    fn a_read(core: &Pitboard) -> Job {
        let mut state = State::new(Cadence::APP);
        let now = Now {
            running: Duration::ZERO,
            epoch_ms: 0,
        };
        state.apply(Msg::Intent(Intent::Refresh { asked: false }), now);
        let answer = work(core, Job::AskInstalled);
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
        let job = a_read(&core);
        let (readings, changed) = (core.readings_changed_at(), core.changed_at());
        let Answer::Read {
            readings_before,
            changed_before,
            read,
            ..
        } = work(&core, job)
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
}
