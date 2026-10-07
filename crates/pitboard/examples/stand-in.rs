//! The program the tests start in place of `claude`, `codex` and every other program they
//! run: it plays the script beside it, as `pitboard_core::testing::stand_in` says. A whole
//! `cargo test` builds it; a test run alone needs `cargo build -p pitboard --example
//! stand-in` first. Nothing installs it, and no build of Pitboard holds it.

fn main() -> std::process::ExitCode {
    pitboard_core::testing::stand_in::main()
}
