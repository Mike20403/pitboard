//! `pitboard-probe-detached-asmv1`: the probe's command line with
//! `consoleAllocationPolicy=detached` in the undocumented asm.v1 spelling embedded by
//! `build.rs`, so block E4 can tell whether only the documented spelling is honoured.

fn main() -> std::process::ExitCode {
    pitboard_probe::cli::main()
}
