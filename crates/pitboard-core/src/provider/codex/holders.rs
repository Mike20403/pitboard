//! Where a running `codex` can be, and what makes each take a switch.
//!
//! Every one of them reads `$CODEX_HOME/auth.json` when it starts and keeps the login in
//! memory until it stops (`codex_never_follows_a_switch`), so each has to be started again,
//! each its own way. Measured on 2026-10-01 from the process list of a Mac running ChatGPT
//! 26.928.31416, its bundled codex 0.159.2, Codex's app-server daemon 0.159.3 and terminal
//! sessions, with the facts in `assumptions`. No editor with the Codex extension was
//! running there, so its place is the extension's packaged layout, unmeasured.

use crate::holder::{Holder, Location, Noun, Remedy};

pub(crate) const HOLDERS: &[Holder] = &[
    // `/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex`,
    // two of them, children of the app. Closing the app's windows leaves it running.
    Holder {
        kind: "chatgpt_app",
        noun: Noun::One("the ChatGPT app"),
        location: Location::Within("ChatGPT.app"),
        remedy: Remedy::ReopenApp {
            bundle_id: "com.openai.codex",
            name: "ChatGPT",
        },
    },
    // `~/.codex/packages/app-server-daemon/releases/0.159.3-aarch64-apple-darwin/bin/codex`,
    // a child of launchd, which terminal sessions can share.
    Holder {
        kind: "app_server_daemon",
        noun: Noun::One("Codex's background app server"),
        location: Location::Within("app-server-daemon"),
        remedy: Remedy::Run("codex app-server daemon restart"),
    },
    // `<extensions>/openai.chatgpt-<version>-<platform>/bin/<platform>/codex`.
    Holder {
        kind: "editor_extension",
        noun: Noun::One("Codex in an editor"),
        location: Location::WithinPrefixed("openai.chatgpt-"),
        remedy: Remedy::Do("run Developer: Reload Window in each editor window that uses it"),
    },
    // Started from a shell by its bare name, or by any other path.
    Holder {
        kind: "session",
        noun: Noun::Counted {
            one: "`codex` session",
            many: "`codex` sessions",
        },
        location: Location::Anywhere,
        remedy: Remedy::Restart,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::holder::classify;
    use crate::process::Process;
    use std::path::PathBuf;

    /// The paths this Mac's process list gave, each where it belongs.
    #[test]
    fn every_measured_codex_is_its_own_kind() {
        let listed = [
            "codex",
            "/Users/a/.codex/packages/standalone/releases/0.154.0-aarch64-apple-darwin/bin/codex",
            "/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
            "/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
            "/Users/a/.codex/packages/app-server-daemon/releases/0.159.3-aarch64-apple-darwin/bin/codex",
            "/Users/a/.vscode/extensions/openai.chatgpt-26.5.0-darwin-arm64/bin/macos-aarch64/codex",
        ];
        let processes: Vec<Process> = (1..)
            .zip(listed)
            .map(|(pid, path)| Process {
                pid,
                path: PathBuf::from(path),
            })
            .collect();
        let holding = classify(&processes, HOLDERS);
        let kinds: Vec<(&str, Vec<u32>)> = holding
            .iter()
            .map(|h| (h.holder.kind, h.pids.clone()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("chatgpt_app", vec![3, 4]),
                ("app_server_daemon", vec![5]),
                ("editor_extension", vec![6]),
                ("session", vec![1, 2]),
            ]
        );
    }

    /// Every list ends with a kind that is anywhere, so no `codex` goes unsaid.
    #[test]
    fn the_last_kind_is_anywhere() {
        assert_eq!(HOLDERS.last().map(|h| h.location), Some(Location::Anywhere));
        assert!(
            HOLDERS[..HOLDERS.len() - 1]
                .iter()
                .all(|h| h.location != Location::Anywhere),
            "and nothing before it claims everything"
        );
    }
}
