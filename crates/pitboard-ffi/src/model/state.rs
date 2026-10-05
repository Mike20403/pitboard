//! What the model knows and decides, apart from any thread, clock or core.
//!
//! [`State::apply`] takes one message and the moment it arrived, changes what is known and
//! says which jobs to run. It calls nothing, reads no clock and waits on nothing. The actor
//! hands it every message in turn and runs the jobs it returns on the lanes, whose answers
//! come back as messages.
//!
//! A call the Swift model awaited is a job here, with two exceptions. A read's two stamps and
//! the read itself are one job, where the Swift awaited three calls, and a look's two stamps
//! are one, where it awaited two. The guards keep their meaning: a read's ticket is taken as
//! its job is made, which is after what is installed was asked, as in the Swift, and compared
//! when its answer lands. One interleaving goes with them. A look that noticed a change could
//! land between a read's stamps and the read in the Swift, which then dropped the read. Here
//! nothing runs between them on the lane of reads: the read lands with the stamps from
//! before the change, and the next look notices it, which ends where the Swift did.
//!
//! A switch is claimed here, as the intent is taken and before its first job is queued, so a
//! second asked for meanwhile does nothing, and it stays claimed until the read after it has
//! landed, as the Swift model's `switching` stayed set until its read returned. Meanwhile the
//! change poll leaves the account index alone: the change is the switch's own.
//!
//! A test hands `apply` answers in whatever order it likes, which is how the interleavings
//! the Swift model's tests reached with gates are reached here without a thread.

use super::{Failure, Intent, LastSwitch, Pane, QuitQuestion, ReadFailure, RestartNeeded};
use super::{Snapshot, WindowRequest};
use crate::{
    Abandoned, Account, Adoption, PitboardError, Status, Switch, Switched, Tool, Usage, Warning,
};
use pitboard_core::label::SEPARATOR;
use pitboard_core::provider::ProviderId;
use std::collections::HashMap;
use std::time::Duration;

/// How often the model does what it does by itself, and how long it waits for an app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cadence {
    /// How often it looks whether anything on this machine has changed: one look at when
    /// Pitboard's account index and its usage readings were last written, which costs no
    /// network and no keychain. Before it, a switch typed in a terminal left the menu bar
    /// naming the account the person had just stopped using for as long as five minutes.
    pub(crate) look_every: Duration,
    /// How often it reads every account by itself. Usage is asked of each tool's service
    /// for every account, so it is asked sparingly.
    pub(crate) read_every: Duration,
    /// How old the numbers shown may be before somebody glancing at them has them read
    /// again: a menu is opened far more often than the numbers change.
    pub(crate) stale_after: Duration,
    /// How long an app has to quit once asked. Long enough for one that asks about work in
    /// progress to be answered; past it, nothing has changed and the switch is not made.
    pub(crate) quit_within: Duration,
    /// How often, meanwhile, it is asked whether the app is still running.
    pub(crate) quit_checked_every: Duration,
}

impl Cadence {
    /// The app's, as the Swift model kept it.
    pub(crate) const APP: Cadence = Cadence {
        look_every: Duration::from_secs(2),
        read_every: Duration::from_secs(300),
        stale_after: Duration::from_secs(60),
        quit_within: Duration::from_secs(30),
        quit_checked_every: Duration::from_millis(200),
    };
}

/// When a message arrived, by two clocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Now {
    /// Since the model started, by a clock that never goes back: what the timers are set by,
    /// so the wall clock being put back does not hold them up. On macOS it stops while the
    /// machine sleeps, where the Swift loops slept on one that does not, so an app sends
    /// [`Intent::Woke`] as it wakes, which reads at once.
    pub(crate) running: Duration,
    /// Epoch milliseconds by the wall clock: what a snapshot says the time is, and what the
    /// age of the numbers is measured by, as the Swift model measured it by `Date`.
    pub(crate) epoch_ms: i64,
}

impl Now {
    /// Epoch seconds, as every timestamp the bindings carry is.
    pub(crate) fn epoch(self) -> i64 {
        self.epoch_ms.div_euclid(1000)
    }
}

/// What comes to the model, one at a time.
#[derive(Debug)]
pub(crate) enum Msg {
    /// What an app asked for.
    Intent(Intent),
    /// A timer may be due. The actor sends this when it has waited until the next one.
    Tick,
    /// What a job came to.
    Done(Answer),
    /// Stop: the actor ends, and with it the lanes. `apply` does nothing with it.
    Stop,
}

/// Something to do away from the model, on a lane, whose answer comes back as
/// [`Msg::Done`]. It carries what the model needs to make sense of the answer, which the
/// lane hands back untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Job {
    /// Read every account: first when the usage readings and the account index were last
    /// written, as they stand before the read, then the read itself. `fresh` asks each tool's
    /// service about every account whatever it was asked moments ago.
    Read { ticket: Ticket, fresh: bool },
    /// Read what is already known, asking nobody: the last numbers measured, and who each
    /// tool's own files say is signed in.
    ReadOffline { why: Why },
    /// When the account index and the usage readings were last written.
    Look,
    /// Which tools' programs were found. Its own lane, so it never waits behind a read on
    /// the network, and a read can wait for it.
    AskInstalled,
    /// Whether a running app holds the login of the tool `qualified` is of, which the core
    /// answers from the process list and the app from what is running.
    Holding { qualified: String },
    /// Ask the app the question names to quit, and wait until it has, for as long as the
    /// cadence gives it.
    Quit { question: QuitQuestion },
    /// The switch itself. `reopen` is where the app Pitboard quit for it was opened from,
    /// which is opened again once the switch is over, whether or not it worked.
    Switch {
        qualified: String,
        reopen: Option<String>,
    },
    /// Open the app at `location` again.
    Open { location: String },
    /// Give up on an interrupted switch, keeping every login it names.
    Abandon,
}

/// A read under way, as things stood when it started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ticket {
    /// How many changes made elsewhere the poll had noticed, and made here, when the read
    /// started. A read that lands once there has been another was made with who was signed
    /// in before that change, and is dropped.
    changes_seen: u64,
    /// Whether the timer that reads every few minutes asked for it. That timer is set again
    /// once its read is over, as the Swift model slept only after its read.
    timed: bool,
    /// Whether it is the read after a switch, whose landing ends the switch.
    after_switch: bool,
}

/// Why what is already known is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Why {
    /// A read that failed before anything was shown: the last numbers measured are still
    /// true, and an empty list would say the accounts are gone.
    InPlaceOf(Ticket),
    /// The account index changed somewhere else, which can be a switch: who is signed in,
    /// and the numbers. `measured` is when the readings were written, as the look found it.
    Changed { measured: i64 },
    /// Only the readings moved, newer numbers some session or the command line recorded:
    /// the numbers and nothing else.
    Numbers { measured: i64 },
}

/// What came of asking an app to quit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum QuitOutcome {
    /// It quit. `copy` is where the one that was running was opened from, the one to open
    /// again.
    Quit { copy: String },
    /// It was not running, so there was nothing to quit and there is nothing to open.
    NotRunning,
    /// It is still running, as it was, or the app's own code could not say it had gone.
    StillRunning,
}

/// What a job came to.
#[derive(Debug)]
pub(crate) enum Answer {
    Read {
        ticket: Ticket,
        /// When the usage readings were last written, in epoch milliseconds, before the read.
        readings_before: i64,
        /// When the account index was last written, in epoch seconds, before the read.
        changed_before: i64,
        read: Result<Status, PitboardError>,
    },
    ReadOffline {
        why: Why,
        read: Result<Status, PitboardError>,
    },
    Looked {
        /// When the account index was last written, in epoch seconds.
        changed: i64,
        /// When the usage readings were last written, in epoch milliseconds.
        measured: i64,
    },
    Installed {
        tools: Vec<Tool>,
    },
    /// The app holding the login, where one is running, as the question to ask about it.
    Held {
        qualified: String,
        question: Option<QuitQuestion>,
    },
    Quit {
        question: QuitQuestion,
        outcome: QuitOutcome,
    },
    Switched {
        qualified: String,
        reopen: Option<String>,
        done: Result<Switched, PitboardError>,
    },
    /// The app was opened again, or could not be, which nothing here can mend.
    Opened,
    Abandoned(Result<Option<Abandoned>, PitboardError>),
    /// The job stopped with a panic and came to nothing. The lane goes on, and the model
    /// goes on as though the job had answered with nothing new.
    Lost(Job),
}

/// A timer of the model's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Timer {
    /// Not started.
    Off,
    /// Due at this moment, since the model started.
    Due(Duration),
    /// What it starts is under way, and the timer is set again once that is over.
    Running,
}

/// A read somebody or something asked for. By default, a read whatever the age of the
/// numbers, which nobody asked for.
#[derive(Debug, Clone, Copy, Default)]
struct Asked {
    /// Whether somebody asked, which has the core ask every service again.
    fresh: bool,
    /// Nothing is read when the numbers shown are younger than this.
    older_than: Duration,
    /// Whether the timer that reads every few minutes asked.
    timed: bool,
    /// Whether a switch asked, which is over once this read lands.
    after_switch: bool,
}

/// What a person asked for that did not happen, before it is numbered.
struct Refused {
    title: String,
    message: String,
    code: Option<String>,
    warnings: Vec<Warning>,
}

impl Refused {
    /// `error`, said under `title`.
    fn of(title: String, error: PitboardError) -> Refused {
        let PitboardError::Failed {
            code,
            message,
            warnings,
            ..
        } = error;
        Refused {
            title,
            message,
            code: Some(code),
            warnings,
        }
    }

    /// A job that came to nothing, under `title`: what it did is not known.
    fn lost(title: String) -> Refused {
        Refused {
            title,
            message: "Pitboard stopped before it could say how this went. Refresh to see \
                      where things stand."
                .into(),
            code: None,
            warnings: Vec::new(),
        }
    }
}

/// "Couldn’t switch to personal", for the account `qualified` names.
fn could_not_switch(qualified: &str) -> String {
    format!("Couldn’t switch to {}", split(qualified).1)
}

const COULD_NOT_GIVE_UP: &str = "Couldn’t give up on the interrupted switch";

/// A label as the core types it, taken apart: `codex/work` is Codex's `work`, and a bare
/// `work` is Claude Code's, which is what a bare label has always meant.
pub(crate) fn split(typed: &str) -> (&str, &str) {
    typed
        .split_once(SEPARATOR)
        .unwrap_or((ProviderId::Claude.code(), typed))
}

/// An account as somebody types it at the command line, which is how the core names the
/// account a switch went to: bare for Claude Code, and with its tool for any other.
fn typed(account: &Account) -> Option<&str> {
    if account.provider == ProviderId::Claude.code() {
        account.label.as_deref()
    } else {
        account.qualified.as_deref()
    }
}

/// The quit question last asked. A question closed unanswered is kept, unasked, because an
/// app can close it before it sends the answer that closed it: MainWindow.swift's alert
/// closes it from its binding and answers from its button on a task of its own, which
/// AppModel.swift's quitAndSwitch says runs once the alert has closed it.
#[derive(Debug)]
struct Quitting {
    question: QuitQuestion,
    /// Whether it is still asked, and so shown and holding every other switch back.
    asked: bool,
}

/// Everything the model knows. The actor thread owns it, and nothing else touches it.
#[derive(Debug)]
pub(crate) struct State {
    cadence: Cadence,
    /// Whether what runs by itself has been started. A test drives everything itself until
    /// it says so, as the Swift model's tests did.
    started: bool,
    /// What the last read gave, or the last numbers known, or nothing before anything has
    /// been read.
    status: Option<Status>,
    /// Everything that went wrong on the way, not only the first of them.
    warnings: Vec<Warning>,
    /// What went wrong with the last read, when it did not answer. The numbers shown are
    /// then the last ones measured.
    failure: Option<ReadFailure>,
    /// An interrupted switch nothing can finish, which the app offers a way out of.
    stuck: bool,
    /// When the accounts were last read, in epoch milliseconds by the wall clock.
    updated_ms: Option<i64>,
    /// Reads under way. Reads overlap, a timer's with one somebody asked for, so they are
    /// counted rather than flagged: the first to end would otherwise say none is running
    /// while the other still is.
    reads: u32,
    /// Counts the changes this app has made and the ones the poll noticed made elsewhere. A
    /// read that started before one lands after it with who was signed in before, and would
    /// put away what the change said, so it is dropped: the read the change starts itself
    /// says what is true now.
    changes_seen: u64,
    /// The tools whose program was found, once an answer has come. Asked by the first read,
    /// and by each read while none has been found: the core asks a login shell that was too
    /// slow to answer once more, and finding none is what says to install a tool.
    installed: Option<Vec<Tool>>,
    /// Whether that is being asked now. A read asked for meanwhile waits for the same answer
    /// rather than asking again.
    asking_installed: bool,
    /// Reads waiting for that answer, which go ahead once it comes.
    waiting: Vec<Asked>,
    /// When the account index was last written, as last seen. `None` until something has
    /// looked: a machine with no index reports 0, which is a real answer.
    changed_at: Option<i64>,
    /// The same for the usage readings, which every session's status line records into.
    readings_at: Option<i64>,
    look: Timer,
    timed_read: Timer,
    /// The account a switch is running for, as its label with its tool: from the moment it
    /// is asked for, through asking what holds its tool's login, quitting an app and the
    /// switch itself, until the read after it lands.
    switching: Option<String>,
    /// A switch waiting for the person to let Pitboard quit an app first: the question last
    /// asked, while it is asked and after it is closed unanswered, until another switch is
    /// asked for.
    quitting: Option<Quitting>,
    /// What each tool's last switch said that is still true, one per tool at most.
    last_switches: Vec<LastSwitch>,
    /// What giving up on an interrupted switch kept, until somebody has read it.
    abandoned: Option<Abandoned>,
    /// The last thing asked for that did not happen.
    presented: Option<Failure>,
    /// How many failures have been said, which numbers the next.
    failures: u64,
    /// The requests for the main window.
    window: WindowRequest,
}

impl State {
    pub(crate) fn new(cadence: Cadence) -> State {
        State {
            cadence,
            started: false,
            status: None,
            warnings: Vec::new(),
            failure: None,
            stuck: false,
            updated_ms: None,
            reads: 0,
            changes_seen: 0,
            installed: None,
            asking_installed: false,
            waiting: Vec::new(),
            changed_at: None,
            readings_at: None,
            look: Timer::Off,
            timed_read: Timer::Off,
            switching: None,
            quitting: None,
            last_switches: Vec::new(),
            abandoned: None,
            presented: None,
            failures: 0,
            window: WindowRequest {
                serial: 0,
                pane: None,
            },
        }
    }

    /// Takes in `msg`, which arrived at `now`, and says what to run. Any timer due by `now`
    /// goes off too, whatever the message, so a stream of messages cannot hold one back.
    pub(crate) fn apply(&mut self, msg: Msg, now: Now) -> Vec<Job> {
        let mut jobs = Vec::new();
        match msg {
            Msg::Intent(intent) => self.intent(intent, now, &mut jobs),
            Msg::Done(answer) => self.answer(answer, now, &mut jobs),
            Msg::Tick | Msg::Stop => {}
        }
        self.go_off(now, &mut jobs);
        jobs
    }

    /// When the next timer is due, since the model started, if any is set.
    pub(crate) fn next_due(&self) -> Option<Duration> {
        [self.look, self.timed_read]
            .into_iter()
            .filter_map(|timer| match timer {
                Timer::Due(at) => Some(at),
                Timer::Off | Timer::Running => None,
            })
            .min()
    }

    /// What an app is shown: what is known now, under `revision`, made at `now` in epoch
    /// seconds.
    pub(crate) fn snapshot(&self, revision: u64, now: i64) -> Snapshot {
        Snapshot {
            revision,
            now,
            reading: self.reads > 0,
            updated_at: self.updated_ms.map(|ms| ms.div_euclid(1000)),
            status: self.status.clone(),
            warnings: self.warnings.clone(),
            read_failure: self.failure.clone(),
            stuck: self.stuck,
            installed: self.installed.clone(),
            switch_under_way: self.switch_under_way().map(str::to_owned),
            quit_question: self.asking().cloned(),
            last_switches: self.last_switches.clone(),
            abandoned: self.abandoned.clone(),
            failure: self.presented.clone(),
            window_request: self.window,
        }
    }

    /// The account a switch is running for, or waiting on the quit question for.
    fn switch_under_way(&self) -> Option<&str> {
        self.switching
            .as_deref()
            .or(self.asking().map(|question| question.qualified.as_str()))
    }

    /// The quit question, while it is asked.
    fn asking(&self) -> Option<&QuitQuestion> {
        self.quitting
            .as_ref()
            .filter(|quitting| quitting.asked)
            .map(|quitting| &quitting.question)
    }

    fn intent(&mut self, intent: Intent, now: Now, jobs: &mut Vec<Job>) {
        match intent {
            Intent::Start => self.start(now),
            // Numbers read before the machine slept say nothing about now.
            Intent::Woke => self.refresh(Asked::default(), now, jobs),
            // A menu opening is somebody looking at what it says, which is the glance the
            // whole app exists for.
            Intent::Glanced => self.refresh(
                Asked {
                    older_than: self.cadence.stale_after,
                    ..Asked::default()
                },
                now,
                jobs,
            ),
            Intent::Refresh { asked } => self.refresh(
                Asked {
                    fresh: asked,
                    ..Asked::default()
                },
                now,
                jobs,
            ),
            Intent::SwitchTo { qualified } => self.switch_asked(qualified, jobs),
            Intent::QuitAndSwitch { qualified } => self.quit_and_switch(&qualified, jobs),
            // The question is gone, and nothing was switched. It is kept, unasked, for the
            // answer an app sends after closing it.
            Intent::KeepAppOpen => {
                if let Some(quitting) = &mut self.quitting {
                    quitting.asked = false;
                }
            }
            Intent::DismissSwitch { provider } => {
                self.last_switches.retain(|last| last.provider != provider);
            }
            Intent::AbandonStuckSwitch => jobs.push(Job::Abandon),
            Intent::DismissAbandoned => self.abandoned = None,
        }
    }

    /// Starts what runs by itself: a read now and every few minutes, and a look for changes
    /// made somewhere else now and every few seconds. Once: a second start starts nothing.
    fn start(&mut self, now: Now) {
        if self.started {
            return;
        }
        self.started = true;
        self.timed_read = Timer::Due(now.running);
        self.look = Timer::Due(now.running);
    }

    fn go_off(&mut self, now: Now, jobs: &mut Vec<Job>) {
        if matches!(self.look, Timer::Due(at) if at <= now.running) {
            self.look = Timer::Running;
            jobs.push(Job::Look);
        }
        if matches!(self.timed_read, Timer::Due(at) if at <= now.running) {
            self.timed_read = Timer::Running;
            self.refresh(
                Asked {
                    timed: true,
                    ..Asked::default()
                },
                now,
                jobs,
            );
        }
    }

    /// A read, once what is installed is known. The first read asks which tools are
    /// installed, and each read asks again while none has been found.
    fn refresh(&mut self, asked: Asked, now: Now, jobs: &mut Vec<Job>) {
        if self.installed.as_ref().is_none_or(Vec::is_empty) {
            self.waiting.push(asked);
            if !self.asking_installed {
                self.asking_installed = true;
                jobs.push(Job::AskInstalled);
            }
            return;
        }
        self.read(asked, now, jobs);
    }

    fn read(&mut self, asked: Asked, now: Now, jobs: &mut Vec<Job>) {
        let older_than = i64::try_from(asked.older_than.as_millis()).unwrap_or(i64::MAX);
        if let Some(at) = self.updated_ms
            && now.epoch_ms.saturating_sub(at) < older_than
        {
            self.read_over(asked.timed, now);
            if asked.after_switch {
                self.switching = None;
            }
            return;
        }
        self.reads += 1;
        jobs.push(Job::Read {
            ticket: Ticket {
                changes_seen: self.changes_seen,
                timed: asked.timed,
                after_switch: asked.after_switch,
            },
            fresh: asked.fresh,
        });
    }

    /// A read is over, or was not needed: the timer that asked for it is set again.
    fn read_over(&mut self, timed: bool, now: Now) {
        if timed && self.started {
            self.timed_read = Timer::Due(now.running + self.cadence.read_every);
        }
    }

    /// A read that was under way has ended, one way or another. The read after a switch
    /// ending ends the switch.
    fn landed(&mut self, ticket: Ticket, now: Now) {
        self.reads = self.reads.saturating_sub(1);
        if ticket.after_switch {
            self.switching = None;
        }
        self.read_over(ticket.timed, now);
    }

    fn look_over(&mut self, now: Now) {
        if self.started {
            self.look = Timer::Due(now.running + self.cadence.look_every);
        }
    }

    fn answer(&mut self, answer: Answer, now: Now, jobs: &mut Vec<Job>) {
        match answer {
            Answer::Read {
                ticket,
                readings_before,
                changed_before,
                read,
            } => self.read_landed(ticket, readings_before, changed_before, read, now, jobs),
            Answer::ReadOffline { why, read } => self.known(why, read.ok(), now),
            Answer::Looked { changed, measured } => self.looked(changed, measured, now, jobs),
            Answer::Installed { tools } => {
                self.installed = Some(tools);
                self.installed_over(now, jobs);
            }
            Answer::Held {
                qualified,
                question,
            } => self.held(qualified, question, jobs),
            Answer::Quit { question, outcome } => self.quit_over(question, outcome, jobs),
            Answer::Switched {
                qualified,
                reopen,
                done,
            } => self.switched(&qualified, reopen, done.map_err(Some), now, jobs),
            Answer::Opened => {}
            Answer::Abandoned(done) => self.abandon_over(done.map_err(Some), now, jobs),
            Answer::Lost(job) => match job {
                Job::Read { ticket, .. } => self.landed(ticket, now),
                Job::ReadOffline { why } => self.known(why, None, now),
                Job::Look => self.look_over(now),
                Job::AskInstalled => self.installed_over(now, jobs),
                // As the Swift model took a holding it could not read: nothing holds it.
                Job::Holding { qualified } => self.held(qualified, None, jobs),
                // Nothing is switched under an app nobody saw go.
                Job::Quit { question } => self.quit_over(question, QuitOutcome::StillRunning, jobs),
                Job::Switch { qualified, reopen } => {
                    self.switched(&qualified, reopen, Err(None), now, jobs);
                }
                Job::Open { .. } => {}
                Job::Abandon => self.abandon_over(Err(None), now, jobs),
            },
        }
    }

    /// The question of what is installed is answered, or came to nothing: the reads that
    /// waited for it go ahead, as the Swift model's read did once its question returned.
    fn installed_over(&mut self, now: Now, jobs: &mut Vec<Job>) {
        self.asking_installed = false;
        for asked in std::mem::take(&mut self.waiting) {
            self.read(asked, now, jobs);
        }
    }

    fn read_landed(
        &mut self,
        ticket: Ticket,
        readings_before: i64,
        changed_before: i64,
        read: Result<Status, PitboardError>,
        now: Now,
        jobs: &mut Vec<Job>,
    ) {
        // Started before a change, so with who was signed in before it.
        if ticket.changes_seen != self.changes_seen {
            self.landed(ticket, now);
            return;
        }
        match read {
            Ok(read) => {
                self.stuck = read
                    .warnings
                    .iter()
                    .any(|w| w.code == "recovery_undetermined");
                self.forget_switches_undone(&read);
                self.warnings = read.warnings.clone();
                self.status = Some(read);
                self.failure = None;
                self.updated_ms = Some(now.epoch_ms);
                // As they stood before the read, because a session can record newer
                // numbers, and a terminal can switch, while the read waits on a service, and
                // the read then shows nothing of either. Taken after, they counted as seen
                // though nothing had shown them. What the read writes itself costs the next
                // look one read of what is known.
                self.changed_at = Some(changed_before);
                self.readings_at = Some(readings_before);
                self.landed(ticket, now);
            }
            Err(PitboardError::Failed {
                code,
                message,
                warnings,
                ..
            }) => {
                // Said as the read fails, so a read that lands before what is known is in
                // says its own and keeps it. The Swift said it once its fallback was in, over
                // whatever a read that landed meanwhile had said.
                self.stuck = code == "recovery_undetermined";
                self.failure = Some(ReadFailure { code, message });
                // What went wrong this time, in place of what was wrong last time. A failure
                // carries its own warnings, and leaving the previous read's in place showed a
                // fresh network error above warnings that may have been fixed since.
                self.warnings = warnings;
                if self.status.is_none() {
                    // Still reading until what is known is in.
                    jobs.push(Job::ReadOffline {
                        why: Why::InPlaceOf(ticket),
                    });
                } else {
                    self.landed(ticket, now);
                }
            }
        }
    }

    /// What is already known came in, or could not be read.
    fn known(&mut self, why: Why, read: Option<Status>, now: Now) {
        match why {
            Why::InPlaceOf(ticket) => {
                // Only where there is still nothing to show: a read that has landed since
                // knows better.
                if self.status.is_none() {
                    self.status = read;
                }
                self.landed(ticket, now);
            }
            Why::Changed { measured } => {
                if let Some(read) = read {
                    self.forget_switches_undone(&read);
                    self.status = Some(read);
                    self.readings_at = Some(measured);
                }
                self.look_over(now);
            }
            Why::Numbers { measured } => {
                // Onto what is shown as the numbers land, which a read may have replaced
                // while they were read. The Swift put them onto what was shown when the
                // look found them, and so put back what that read had replaced.
                if let (Some(read), Some(shown)) = (read, self.status.as_ref()) {
                    self.status = Some(numbers(&read, shown));
                    self.readings_at = Some(measured);
                }
                self.look_over(now);
            }
        }
    }

    /// Has anything on this machine changed since the last look.
    ///
    /// Two things are looked at, because they mean different things. The account index
    /// changing can be a switch made somewhere else, so who is signed in is read again. The
    /// readings changing is only numbers, newer ones a session or the command line has seen,
    /// so only the numbers are taken. Read again every time, who is signed in would come from
    /// each tool's own files several times a minute, and a switch that could not update
    /// Claude Code's config leaves it naming the account before.
    fn looked(&mut self, changed: i64, measured: i64, now: Now, jobs: &mut Vec<Job>) {
        // The first look only records where things stand; there is nothing to compare to.
        let (Some(seen), Some(seen_readings)) = (self.changed_at, self.readings_at) else {
            self.changed_at = Some(changed);
            self.readings_at = Some(measured);
            self.look_over(now);
            return;
        };
        // A switch this app has under way is its own change and not somebody else's, and
        // taking it for one put away what the switch had just said. What changed meanwhile
        // stays unseen until it is shown: a switch that fails reads nothing after it, and
        // one that failed after finishing an interrupted switch has still moved who is
        // signed in.
        if self.switching.is_some() {
            self.look_over(now);
            return;
        }
        self.changed_at = Some(changed);
        if seen != changed {
            self.changes_seen += 1;
            jobs.push(Job::ReadOffline {
                why: Why::Changed { measured },
            });
        } else if seen_readings != measured && self.status.is_some() {
            jobs.push(Job::ReadOffline {
                why: Why::Numbers { measured },
            });
        } else {
            self.look_over(now);
        }
    }

    /// A switch asked for from the window, the menu or a notification.
    ///
    /// Where an app runs the tool with its login in memory, as ChatGPT runs Codex, the switch
    /// waits for the person to let Pitboard quit it first: switched under it, the app would go
    /// on with the account left behind, and its own sign-out would revoke the login Pitboard
    /// has just parked.
    fn switch_asked(&mut self, qualified: String, jobs: &mut Vec<Job>) {
        // One switch at a time: the second would wait behind the first anyway, and its
        // choice was made from a menu that did not yet show the first. A question waiting to
        // be answered comes back to the front rather than being left behind unseen.
        if self.switch_under_way().is_some() {
            if self.asking().is_some() {
                self.show_window(Some(Pane::Accounts));
            }
            return;
        }
        // Claimed before anything is asked, so a second request made meanwhile waits too,
        // and in place of a question closed unanswered, which no answer takes after this.
        self.quitting = None;
        self.switching = Some(qualified.clone());
        jobs.push(Job::Holding { qualified });
    }

    /// What holds the login is known: an app to ask about, or nothing, and then the switch.
    fn held(&mut self, qualified: String, question: Option<QuitQuestion>, jobs: &mut Vec<Job>) {
        match question {
            Some(question) => {
                self.switching = None;
                self.quitting = Some(Quitting {
                    question,
                    asked: true,
                });
                self.show_window(Some(Pane::Accounts));
            }
            None => jobs.push(Job::Switch {
                qualified,
                reopen: None,
            }),
        }
    }

    /// The person let Pitboard quit the app for the switch to `qualified`: the switch is
    /// under way again from here, and the question is gone. Taken whether or not the question
    /// is still asked, since an app can close it first, but only as the answer to the
    /// question last asked, and once. A switch asked for since has taken that question's
    /// place, so an answer never starts a second switch.
    fn quit_and_switch(&mut self, qualified: &str, jobs: &mut Vec<Job>) {
        let Some(Quitting { question, .. }) = self
            .quitting
            .take_if(|quitting| quitting.question.qualified == qualified)
        else {
            return;
        };
        self.switching = Some(question.qualified.clone());
        jobs.push(Job::Quit { question });
    }

    /// Quits the app, switches, and opens the same copy of the app again: Pitboard closed it,
    /// so Pitboard opens it. An app that is already gone is not opened. One that does not
    /// quit, because it was busy or its person said no, stops everything before anything has
    /// changed.
    fn quit_over(&mut self, question: QuitQuestion, outcome: QuitOutcome, jobs: &mut Vec<Job>) {
        let QuitQuestion {
            qualified, name, ..
        } = question;
        match outcome {
            QuitOutcome::StillRunning => {
                self.switching = None;
                self.present(Refused {
                    title: could_not_switch(&qualified),
                    message: format!(
                        "{name} is still open, so nothing has changed. Quit it, then switch again."
                    ),
                    code: None,
                    warnings: Vec::new(),
                });
            }
            QuitOutcome::NotRunning => jobs.push(Job::Switch {
                qualified,
                reopen: None,
            }),
            QuitOutcome::Quit { copy } => jobs.push(Job::Switch {
                qualified,
                reopen: Some(copy),
            }),
        }
    }

    /// The switch is over: what it said is kept and the accounts are read, or what stopped
    /// it is said in the window. `Err(None)` is a switch that came to nothing.
    ///
    /// What the switch means for sessions already running depends on the tool. One that
    /// follows by itself gets when it will have; one that never does gets said so, since a
    /// countdown there would promise something that is not going to happen.
    fn switched(
        &mut self,
        qualified: &str,
        reopen: Option<String>,
        done: Result<Switched, Option<PitboardError>>,
        now: Now,
        jobs: &mut Vec<Job>,
    ) {
        // Pitboard closed it, so Pitboard opens it, whether or not the switch worked: as
        // soon as the switch is made, and not after the read that follows it, which can wait
        // on the network.
        if let Some(location) = reopen {
            jobs.push(Job::Open { location });
        }
        match done {
            Ok(done) => {
                self.changes_seen += 1;
                self.said(qualified, done, now);
                self.updated_ms = None;
                self.refresh(
                    Asked {
                        after_switch: true,
                        ..Asked::default()
                    },
                    now,
                    jobs,
                );
            }
            Err(error) => {
                self.switching = None;
                let title = could_not_switch(qualified);
                let refused = match error {
                    Some(error) => Refused::of(title, error),
                    None => Refused::lost(title),
                };
                // Said beside the warnings already shown, first, with nothing the last read
                // found dropped to make room for them. Nothing moved, so what the last switch
                // said stands too.
                let mut warnings = refused.warnings.clone();
                warnings.extend(
                    self.warnings
                        .drain(..)
                        .filter(|warning| !refused.warnings.contains(warning)),
                );
                self.warnings = warnings;
                self.present(refused);
            }
        }
    }

    /// Keeps what a switch that worked said, in place of what its own tool's last switch
    /// said and beside what every other tool's did.
    fn said(&mut self, qualified: &str, done: Switched, now: Now) {
        match done.outcome {
            Switch::Switched {
                provider,
                from,
                to,
                adoption,
            } => {
                let (follows_at, restart) = match adoption {
                    Adoption::Follows { within_seconds } => {
                        (Some(now.epoch() + i64::from(within_seconds)), None)
                    }
                    Adoption::Restart { program } => (
                        None,
                        Some(RestartNeeded {
                            program,
                            from: split(&from).1.to_owned(),
                        }),
                    ),
                };
                self.remember(LastSwitch {
                    provider,
                    to,
                    follows_at,
                    restart,
                    warnings: done.warnings,
                });
            }
            Switch::AlreadyActive { label } => {
                // Nothing moved, so what this tool's last switch said still stands, and
                // anything this one warned about is said beside it.
                if done.warnings.is_empty() {
                    return;
                }
                let provider = split(qualified).0;
                let mut said = self
                    .last_switches
                    .iter()
                    .find(|last| last.provider == provider && last.to == label)
                    .cloned()
                    .unwrap_or_else(|| LastSwitch {
                        provider: provider.to_owned(),
                        to: label,
                        follows_at: None,
                        restart: None,
                        warnings: Vec::new(),
                    });
                for warning in done.warnings {
                    if !said.warnings.contains(&warning) {
                        said.warnings.push(warning);
                    }
                }
                self.remember(said);
            }
        }
    }

    /// In place of what the same tool's last switch said, or after the others.
    fn remember(&mut self, said: LastSwitch) {
        match self
            .last_switches
            .iter_mut()
            .find(|last| last.provider == said.provider)
        {
            Some(last) => *last = said,
            None => self.last_switches.push(said),
        }
    }

    /// Puts away what a switch said once its tool no longer has the account it switched to
    /// signed in: a switch made somewhere else, or a sign-out. Anything else that writes the
    /// account index leaves the sessions it describes exactly as they were, and taking every
    /// write for a switch put away the one warning that keeps somebody from revoking the
    /// login a Codex switch had just parked.
    fn forget_switches_undone(&mut self, read: &Status) {
        self.last_switches.retain(|last| {
            read.accounts.iter().any(|account| {
                account.provider == last.provider
                    && account.signed_in
                    && typed(account) == Some(last.to.as_str())
            })
        });
    }

    /// Giving up on an interrupted switch is over: what it kept is said and the accounts are
    /// read again, asking each service, or what stopped it is said in the window.
    fn abandon_over(
        &mut self,
        done: Result<Option<Abandoned>, Option<PitboardError>>,
        now: Now,
        jobs: &mut Vec<Job>,
    ) {
        match done {
            Ok(abandoned) => {
                self.abandoned = abandoned;
                self.changes_seen += 1;
                self.stuck = false;
                self.refresh(
                    Asked {
                        fresh: true,
                        ..Asked::default()
                    },
                    now,
                    jobs,
                );
            }
            Err(Some(error)) => self.present(Refused::of(COULD_NOT_GIVE_UP.into(), error)),
            Err(None) => self.present(Refused::lost(COULD_NOT_GIVE_UP.into())),
        }
    }

    /// Says a failure in the window, numbered one higher than the last.
    fn present(&mut self, refused: Refused) {
        self.failures += 1;
        self.presented = Some(Failure {
            id: self.failures,
            title: refused.title,
            message: refused.message,
            code: refused.code,
            warnings: refused.warnings,
        });
        self.show_window(None);
    }

    /// Asks for the main window, on `pane` when it matters which.
    fn show_window(&mut self, pane: Option<Pane>) {
        self.window.serial += 1;
        self.window.pane = pane;
    }

    /// A look now, as the timer's would be, for a test that must not wait for the timer.
    #[cfg(test)]
    pub(super) fn look_now(&mut self) -> Vec<Job> {
        if self.look != Timer::Off {
            self.look = Timer::Running;
        }
        vec![Job::Look]
    }

    #[cfg(test)]
    pub(super) fn changes_seen(&self) -> u64 {
        self.changes_seen
    }
}

/// `shown` with each account's numbers as `read` has them, and everything else as it was. An
/// account `read` has no numbers for keeps its own.
fn numbers(read: &Status, shown: &Status) -> Status {
    let mut measured: HashMap<&str, &Usage> = HashMap::new();
    for account in &read.accounts {
        if let Some(usage) = &account.usage {
            measured.entry(account.id.as_str()).or_insert(usage);
        }
    }
    Status {
        now: shown.now,
        accounts: shown
            .accounts
            .iter()
            .map(|account| match measured.get(account.id.as_str()) {
                Some(usage) => Account {
                    usage: Some((*usage).clone()),
                    ..account.clone()
                },
                None => account.clone(),
            })
            .collect(),
        warnings: shown.warnings.clone(),
    }
}
