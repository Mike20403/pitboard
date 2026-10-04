//! Nothing Pitboard reads reaches a command a test runs from the environment `cargo test`
//! was started in. `PITBOARD_CLAUDE` exported in the developer's shell would otherwise
//! have every sign-in test run that `claude`, the real one, with the real home.
//!
//! The variables have to be in the test process's environment from the start, as a shell
//! exporting them would put them, and setting them from a test would change the environment
//! while the harness's other threads may read it. So the test runs this binary again, as a
//! child given both, and the child runs the same test, which then signs in.

mod common;

use std::path::PathBuf;
use std::process::Command;

/// Set in the child's environment, which is how the child knows it is the one to sign in.
const CHILD: &str = "PITBOARD_TEST_INHERITED_CHILD";

/// This test's name, which the child is asked to run and nothing else.
const NAME: &str = "a_sign_in_never_runs_the_program_the_tests_environment_names";

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "the child is told it is the child through its environment"
)]
fn a_sign_in_never_runs_the_program_the_tests_environment_names() {
    match std::env::var_os(CHILD) {
        Some(_) => signing_in_runs_the_stand_in(),
        None => in_a_child_given_both_variables(),
    }
}

/// A scratch directory, removed however the test ends.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs this test again in a child whose environment names, for both tools, a program no
/// test may run, which only writes down that it was run.
fn in_a_child_given_both_variables() {
    let scratch =
        Scratch(std::env::temp_dir().join(format!("pitboard-inherited-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&scratch.0);
    std::fs::create_dir_all(&scratch.0).expect("a scratch directory");
    let ran = scratch.0.join("ran");
    let trap = scratch.0.join("must-not-run");
    common::write_program(
        &trap,
        &format!("#!/bin/sh\necho \"$0 $*\" >> '{}'\nexit 1\n", ran.display()),
    );

    let out = Command::new(std::env::current_exe().expect("this test binary"))
        .args([NAME, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .env("PITBOARD_CLAUDE", &trap)
        .env("PITBOARD_CODEX", &trap)
        .output()
        .expect("this test binary runs");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let trapped = std::fs::read_to_string(&ran).unwrap_or_default();
    assert!(
        trapped.is_empty(),
        "a sign-in ran the program the environment names: {trapped}\n{said}"
    );
    assert!(out.status.success(), "the child failed:\n{said}");
    assert!(said.contains("1 passed"), "the child ran no test:\n{said}");
}

/// A sign-in for each tool runs the test's stand-in, which the harness puts first on `PATH`,
/// and never the program the test process's environment names, which the parent checks.
fn signing_in_runs_the_stand_in() {
    let mut env = common::Env::new("inherited");
    let (b, p) = (env.uuid('b'), env.uuid('p'));
    let (_, err, code) = env.enroll_by_signing_in("work", &b, "b@example.com", &p, "refresh-b");
    assert_eq!(code, 0, "enroll work --sign-in: {err}");
    env.install_fake_codex_login(&common::codex_login(
        &env.uuid('c'),
        "c@example.com",
        "codex-refresh-c",
    ));
    let (_, err, code) = env.run(&["enroll", "codex/work", "--sign-in"]);
    assert_eq!(code, 0, "enroll codex/work --sign-in: {err}");
}
