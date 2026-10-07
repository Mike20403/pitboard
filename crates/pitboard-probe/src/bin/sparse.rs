//! `pitboard-probe-sparse`: the probe's command line with the `<msix>` identity element of the
//! sparse probe package embedded by `build.rs`, for block K1. Run from the package's external
//! location once the package is registered, it carries the package's identity.

fn main() -> std::process::ExitCode {
    pitboard_probe::cli::main()
}
