//! `pitboard-probe`: the plain program. Its command line is the library's ([`pitboard_probe::cli`]),
//! which each of the manifest variants in `src/bin` shares, so a variant differs from this one
//! in the manifest built into it and nothing else.

fn main() -> std::process::ExitCode {
    pitboard_probe::cli::main()
}
