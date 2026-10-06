//! What a fixture's model asks of the app's own system, played by the fixture: the other
//! apps on its machine, and notifications, which it posts none of.

use crate::model::{AppControl, Notifications, PlatformError, RunOutNotice};
use pitboard_core::testing::MemoryHost;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

/// ChatGPT, as the core's holder detection names it: its bundle id.
pub(crate) const CHATGPT: &str = "com.openai.codex";

/// Where ChatGPT runs its own `codex`, as the process list of a Mac running ChatGPT
/// 26.928.31416 gave it, recorded in pitboard-core's `provider::codex::holders`: what tells
/// the core the app is open. Two of them, as there were.
const CHATGPT_CODEX: &str =
    "/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex";

/// The apps running on a fixture's machine. The core sees ChatGPT's `codex` in the machine's
/// process list while it runs, and the model quits and opens it through this, so the two
/// agree the way the process list and the system's list of apps agree on a real machine.
/// Nothing here touches an app of the machine running the fixture.
pub(crate) struct FixtureApps {
    host: Arc<MemoryHost>,
    held: Mutex<Held>,
}

#[derive(Default)]
struct Held {
    running: BTreeSet<String>,
    /// Every app asked to quit and every app opened, in order: "quit com.openai.codex".
    asked: Vec<String>,
}

impl FixtureApps {
    /// The apps of the machine `host` is, `running` among them from the start.
    pub(crate) fn new(host: Arc<MemoryHost>, running: &[&str]) -> Arc<FixtureApps> {
        let apps = Arc::new(FixtureApps {
            host,
            held: Mutex::default(),
        });
        for app in running {
            apps.started(app);
        }
        apps
    }

    fn held(&self) -> MutexGuard<'_, Held> {
        // Every change here is whole whenever the lock is let go.
        self.held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Every app asked to quit and every app opened, in order.
    #[cfg(test)]
    pub(crate) fn asked(&self) -> Vec<String> {
        self.held().asked.clone()
    }

    /// Whether `app` is running.
    pub(crate) fn is_running(&self, app: &str) -> bool {
        self.held().running.contains(app)
    }

    /// Where an app of the fixture's is, which is nowhere on the machine running it.
    fn copy_of(app: &str) -> String {
        format!("/fixture/{app}.app")
    }

    fn started(&self, app: &str) {
        self.held().running.insert(app.to_owned());
        if app == CHATGPT {
            self.host.runs_at("codex", &[CHATGPT_CODEX, CHATGPT_CODEX]);
        }
    }
}

impl AppControl for FixtureApps {
    fn running(&self, app: String) -> Result<Option<String>, PlatformError> {
        Ok(self.is_running(&app).then(|| FixtureApps::copy_of(&app)))
    }

    /// Quits at once, as an app with no work in progress does, and its processes leave the
    /// process list with it.
    fn request_quit(&self, app: String) -> Result<(), PlatformError> {
        let mut held = self.held();
        held.asked.push(format!("quit {app}"));
        held.running.remove(&app);
        drop(held);
        if app == CHATGPT {
            self.host.runs_at("codex", &[]);
        }
        Ok(())
    }

    fn reopen(&self, location: String) -> Result<(), PlatformError> {
        let app = location
            .strip_prefix("/fixture/")
            .and_then(|rest| rest.strip_suffix(".app"))
            .unwrap_or(&location)
            .to_owned();
        self.held().asked.push(format!("open {app}"));
        self.started(&app);
        Ok(())
    }
}

/// Notifications, of which a fixture posts none: the person running the tests is asked for
/// nothing, a notification's permission included.
pub(crate) struct Unposted;

impl Notifications for Unposted {
    fn post(&self, _notice: RunOutNotice) -> Result<(), PlatformError> {
        Ok(())
    }
}
