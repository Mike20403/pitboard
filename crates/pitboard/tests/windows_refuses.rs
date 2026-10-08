//! A Windows build of Pitboard changes nothing before its Windows face is built: run as the
//! command line it is, with every folder Windows names for a person, and every home Pitboard
//! and the tools read, at scratch folders of this test's own. Apart from the harness the other
//! tests share, which plants logins the way each system keeps them and runs on Windows from
//! W13.
//!
//! Which refusal it meets is the build's, and CI's Windows jobs run it against both. One
//! opened to Windows before its release (`pitboard_core::release`) reaches the face that
//! refuses what is not built: a change is refused because nobody can tell whether it runs
//! elevated, and the reads answer. One that is not opened refuses every command but
//! `--version`, `--help`, `completions` and `manpage` with `windows_not_released`, which CI
//! checks again of a build made as a release is, without the tests' features.
#![cfg(windows)]

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The folders a person's Windows account and Pitboard's environment name, each pointed at a
/// scratch folder of this test's own. Codex's home must be there for Codex to take it.
const FOLDERS: [&str; 7] = [
    "USERPROFILE",
    "HOME",
    "APPDATA",
    "LOCALAPPDATA",
    "CODEX_HOME",
    "CLAUDE_CONFIG_DIR",
    "PITBOARD_HOME",
];

/// Every change the command line makes, as a person types it. `--json` is added to each.
const CHANGES: [&[&str]; 15] = [
    &["enroll", "work"],
    &["enroll", "codex/work", "--sign-in"],
    &["use", "work"],
    &["forget", "work", "-y"],
    &["abandon"],
    &["repair"],
    &["adopt"],
    &["renew"],
    &["renew", "--scheduled"],
    &["schedule", "install"],
    &["schedule", "uninstall"],
    &["uninstall", "-y"],
    &["rename", "work", "job"],
    &["watch"],
    &["watch", "--once"],
];

/// What reads and changes nothing, as a person types it. `--json` is added to each.
const READS: [&[&str]; 5] = [
    &["status"],
    &["status", "--offline"],
    &["doctor"],
    &["schedule", "status"],
    &["log"],
];

/// The scratch folders, taken away with everything in them once the test is done.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch() -> Scratch {
    let root =
        std::env::temp_dir().join(format!("pitboard-windows-refuses-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for folder in FOLDERS {
        std::fs::create_dir_all(root.join(folder)).expect("a scratch folder");
    }
    Scratch(root)
}

/// `pitboard <args>`, with nothing Pitboard reads taken from whoever runs the test, every
/// folder at its scratch one, and no `PATH`, so no installed tool is found.
fn pitboard(root: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pitboard"));
    for name in pitboard_core::testing::variables() {
        command.env_remove(name);
    }
    for folder in FOLDERS {
        command.env(folder, root.join(folder));
    }
    command
        .args(args)
        .current_dir(root)
        .output()
        .expect("pitboard runs")
}

/// `pitboard <args> --json`, the envelope it printed, and the status it exited with.
fn envelope(root: &Path, args: &[&str]) -> (Value, Option<i32>) {
    let output = pitboard(root, &[args, &["--json"]].concat());
    let printed = String::from_utf8_lossy(&output.stdout);
    let envelope = serde_json::from_str(&printed)
        .unwrap_or_else(|e| panic!("{args:?} printed no envelope ({e}): {printed}"));
    (envelope, output.status.code())
}

/// Whatever the build, `--version`, `--help` and a shell's completions answer, and nothing is
/// left in any folder a person's files are in.
#[test]
fn a_windows_build_answers_what_it_may_and_changes_nothing() {
    let scratch = scratch();
    let root = &scratch.0;

    let version = pitboard(root, &["--version"]);
    assert!(version.status.success(), "{version:?}");
    assert!(
        String::from_utf8_lossy(&version.stdout).starts_with("pitboard "),
        "{version:?}"
    );
    for answers in [&["--help"][..], &["completions", "powershell"]] {
        let output = pitboard(root, answers);
        assert!(output.status.success(), "{answers:?}: {output:?}");
        assert!(!output.stdout.is_empty(), "{answers:?}");
    }

    let opened = pitboard_core::release::check().is_ok();
    for change in CHANGES {
        let (envelope, exit) = envelope(root, change);
        assert_eq!(envelope["ok"], false, "{change:?}: {envelope}");
        assert_ne!(exit, Some(0), "{change:?}: {envelope}");
        let code = envelope["error"]["code"].as_str().unwrap_or_default();
        if opened {
            assert_eq!(code, "elevated", "{change:?}: {envelope}");
        } else {
            assert_eq!(code, "windows_not_released", "{change:?}: {envelope}");
        }
    }
    for read in READS {
        let (envelope, exit) = envelope(root, read);
        assert_eq!(envelope["command"], read[0], "{read:?}: {envelope}");
        if opened {
            assert_ne!(
                envelope["error"]["code"], "windows_not_released",
                "{read:?}: {envelope}"
            );
            assert!(matches!(exit, Some(0 | 3)), "{read:?}: {exit:?} {envelope}");
        } else {
            assert_eq!(
                envelope["error"]["code"], "windows_not_released",
                "{read:?}: {envelope}"
            );
            assert_eq!(
                envelope["error"]["message"],
                "Pitboard for Windows is not released yet. This build changes nothing.",
                "{read:?}"
            );
            assert_eq!(exit, Some(1), "{read:?}");
        }
    }

    for folder in FOLDERS {
        let left: Vec<_> = std::fs::read_dir(root.join(folder))
            .expect("the scratch folder is still there")
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert!(left.is_empty(), "{folder} holds {left:?}");
    }
}
