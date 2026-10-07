//! `pitboard-probe-detached`: the probe's command line with `consoleAllocationPolicy=detached`
//! in the documented `asmv3` spelling embedded by `build.rs`, for block E4. Started as
//! `console-child`, it records whether it got a console.

fn main() -> std::process::ExitCode {
    pitboard_probe::cli::main()
}
