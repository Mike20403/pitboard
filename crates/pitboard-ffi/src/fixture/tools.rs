//! Each tool's own sign-in on a fixture's machine, played through pitboard-core's seam for a
//! sign-in that starts no program, together with the person at the browser. Everything
//! around it is the real core's: the private directory it reserves, the login it reads back
//! from where the tool stores it there, and the enrolment.
//!
//! Each prints what its tool's register records it printing. Claude Code's, as
//! `sign_in_output` has 2.1.289, prints that it opens the browser, the address to open where
//! the browser did not, and its prompt for the code the browser shows, and then waits for
//! one; where 2.1.289 refuses a line that is not `<code>#<state>`, as
//! `sign_in_takes_another_code` has it, this takes whatever is typed back, since a UI test
//! types `fixture-code`. Codex's, as `codex_login_prints_its_address` has 0.160.0, prints
//! where its loopback server listens and the address to open, reads nothing typed back, and
//! finishes by itself a moment later, once the browser would have come back.
//!
//! Whoever signs in is the person the account is for: one enrolled already under that name
//! signs in again as themselves, and anybody else is somebody new, `<name>@example.com`,
//! with a five-hour limit barely touched.

use super::worlds::{Machine, Person};
use pitboard_core::provider::ProviderId;
use pitboard_core::testing::{ScriptedSignIn, SignInScript, service_for_dir};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long a tool takes between the pieces it prints, as the macOS app's fixture took.
const BETWEEN: Duration = Duration::from_millis(200);

/// How long Codex's browser takes to come back once its address is printed.
const BROWSER: Duration = Duration::from_millis(1_500);

/// The address Claude Code's sign-in prints for a browser that did not open, in the shape the
/// register records: the manual one, on claude.com. Nothing opens it: a UI test only sees
/// that it is offered.
pub(crate) const CLAUDE_ADDRESS: &str =
    "https://claude.com/cai/oauth/authorize?code=true&state=pitboard-fixture";

/// The address Codex's sign-in prints, in the shape the register records: on the issuer,
/// auth.openai.com.
pub(crate) const CODEX_ADDRESS: &str =
    "https://auth.openai.com/oauth/authorize?response_type=code&state=pitboard-fixture";

/// The browser and each tool's sign-in on a fixture's machine.
pub(crate) struct Browser {
    machine: Arc<Machine>,
}

impl Browser {
    pub(crate) fn new(machine: Arc<Machine>) -> Browser {
        Browser { machine }
    }
}

impl std::fmt::Debug for Browser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Browser")
            .field("root", &self.machine.root())
            .finish()
    }
}

/// What a played sign-in is told from outside.
enum Told {
    /// A line typed back.
    Typed,
    /// Stopped, as a kill stops the program.
    Stopped,
}

impl SignInScript for Browser {
    fn start(
        &self,
        which: ProviderId,
        label: Option<&str>,
        dir: &Path,
        say: Sender<String>,
    ) -> Box<dyn ScriptedSignIn> {
        let person = self
            .machine
            .signing_in_as(which, label.unwrap_or("someone"));
        let (told, hears) = channel();
        let machine = Arc::clone(&self.machine);
        let dir = dir.to_path_buf();
        let ended = std::thread::Builder::new()
            .name("pitboard-fixture-sign-in".into())
            .spawn(move || play(&machine, &person, &dir, &say, &hears))
            .ok();
        Box::new(Playing { told, ended })
    }
}

/// One sign-in under way, as its program would be driven.
struct Playing {
    told: Sender<Told>,
    /// Its thread, which says whether it ended signed in. `None` once waited for, and where
    /// the system would not start one, which is a sign-in that never finishes.
    ended: Option<JoinHandle<bool>>,
}

impl ScriptedSignIn for Playing {
    fn typed(&mut self, _line: &str) -> bool {
        self.told.send(Told::Typed).is_ok()
    }

    fn wait(&mut self) -> bool {
        self.ended
            .take()
            .is_some_and(|ended| ended.join().unwrap_or(false))
    }

    fn stop(&mut self) {
        let _ = self.told.send(Told::Stopped);
    }
}

/// `person`'s tool signs in, as it prints and reads, into `dir`. Whether it ended signed in;
/// `say` goes with it, which is the tool having stopped saying anything.
fn play(
    machine: &Machine,
    person: &Person,
    dir: &Path,
    say: &Sender<String>,
    hears: &Receiver<Told>,
) -> bool {
    let printed = match person.tool {
        ProviderId::Claude => vec![
            "Opening browser to sign in\u{2026}\n".to_owned(),
            format!("If the browser didn't open, visit: {CLAUDE_ADDRESS}\n"),
            "Paste code here if prompted > ".to_owned(),
        ],
        ProviderId::Codex => vec![
            "Starting local login server on http://localhost:1455.\n".to_owned(),
            format!(
                "If your browser did not open, navigate to this URL to authenticate:\n\n\
                 {CODEX_ADDRESS}\n"
            ),
        ],
        // A tool the fixture has no sign-in of fails to start one.
        _ => return false,
    };
    // A line typed back before the tool reads one waits for it, as it would in its stdin.
    let mut typed = false;
    let last = printed.len() - 1;
    for (at, piece) in printed.into_iter().enumerate() {
        if say.send(piece).is_err() {
            return false;
        }
        if at < last {
            match heard_within(hears, BETWEEN) {
                Heard::Stopped => return false,
                Heard::Typed => typed = true,
                Heard::Nothing => {}
            }
        }
    }
    let done = match person.tool {
        // It waits for the code the browser shows, however long that takes.
        ProviderId::Claude => typed || matches!(hears.recv(), Ok(Told::Typed)),
        // It reads nothing typed back, and the browser comes back by itself.
        ProviderId::Codex => heard_within(hears, BROWSER) != Heard::Stopped,
        _ => false,
    };
    if done {
        machine.signed_in_privately(person, dir);
    }
    done
}

/// What a sign-in was told while it waited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Heard {
    Nothing,
    /// A line was typed back, and it was not stopped.
    Typed,
    Stopped,
}

/// What it is told within `wait`: whether it was stopped, and otherwise whether a line was
/// typed back meanwhile.
fn heard_within(hears: &Receiver<Told>, wait: Duration) -> Heard {
    let until = Instant::now() + wait;
    let mut heard = Heard::Nothing;
    loop {
        match hears.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok(Told::Typed) => heard = Heard::Typed,
            Ok(Told::Stopped) | Err(RecvTimeoutError::Disconnected) => return Heard::Stopped,
            Err(RecvTimeoutError::Timeout) => return heard,
        }
    }
}

impl Machine {
    /// Whom a sign-in of `which`'s tool for `label` signs in as: the person enrolled under
    /// it, or somebody new.
    fn signing_in_as(&self, which: ProviderId, label: &str) -> Person {
        self.enrolled_as(which, label).unwrap_or_else(|| Person {
            tool: which,
            id: format!("{label}-{}", which.code()),
            email: format!("{label}@example.com"),
        })
    }

    /// A new login of `person`'s, stored in `dir` where their tool stores the login of a
    /// sign-in into it: Claude Code in the keychain item named for the directory, Codex in
    /// `auth.json` inside it.
    fn signed_in_privately(&self, person: &Person, dir: &Path) {
        let now = self.now_on_its_clock();
        let login = self.login(person, now + 86_400, now + 30 * 86_400);
        match person.tool {
            ProviderId::Claude => self
                .host
                .live()
                .plant(&service_for_dir(&dir.to_string_lossy()), &login.document),
            ProviderId::Codex => self
                .host
                .file_at(PathBuf::from(dir).join("auth.json"))
                .plant("auth.json", &login.document),
            _ => {}
        }
    }
}
