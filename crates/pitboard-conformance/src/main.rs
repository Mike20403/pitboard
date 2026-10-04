//! Check what Pitboard believes about a coding tool against a build of that tool.
//!
//! Every load-bearing fact in Pitboard was read out of one build and lives in
//! [`pitboard_core::assumptions`] with the literals it is readable by. This reads those
//! literals out of a binary and says which ones are still there.
//!
//! What it is: a cheap, shallow drift alarm. A literal being present does not prove the
//! behaviour around it is unchanged, and this never says it does. A literal disappearing
//! does prove something moved, which is the only thing worth waking somebody for.
//!
//! What it is not: a test of Pitboard against a running Claude Code. That needs a real
//! sign-in and a real keychain and cannot run unattended.
//!
//! A build is for one system, and a tool's builds for different systems carry different
//! code: Claude Code's Linux build has no keychain code at all. So the system is read from
//! the binary's own header, and a fact is read only from the builds its register entry
//! names. Before that, a Linux build reported both keychain facts gone and a keyring
//! arrived, on every build from 2.1.278 on, while the macOS build missed the one real
//! change in 2.1.281.
//!
//! ```text
//! pitboard-conformance <path to a binary> [--provider claude|codex] [--json]
//! ```
//!
//! Each tool has its own register, and a build of one tool says nothing about another's
//! facts, so a run checks one tool's build against that tool's register. Claude Code is
//! the default, which is what every run before there was a second tool meant.

use pitboard_core::assumptions::{self, Platform, Reading};
use pitboard_core::provider::ProviderId;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (path, provider, as_json) = match parse(&args) {
        Ok(parsed) => parsed,
        Err(problem) => {
            eprintln!("{problem}");
            eprintln!(
                "usage: pitboard-conformance <path to a binary> [--provider {}] [--json]",
                known().join("|")
            );
            return ExitCode::from(2);
        }
    };

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return ExitCode::from(2);
        }
    };
    let Some(platform) = platform_of(&bytes) else {
        eprintln!("{path} is neither an ELF nor a Mach-O binary, so its system is unknown");
        return ExitCode::from(2);
    };
    let strings = assumptions::printable_runs(&bytes, 6);

    // A fact not read from this system's builds is left out, not reported: its literals
    // being absent here says nothing about it.
    let (readings, elsewhere): (Vec<_>, Vec<_>) = assumptions::of(provider)
        .iter()
        .partition(|a| assumptions::read_on(provider, a.name).contains(&platform));
    let readings: Vec<(&assumptions::Assumption, Reading)> = readings
        .into_iter()
        .map(|a| (a, assumptions::read_from_build(a, &strings)))
        .collect();
    // Both kinds of drift count. A fact that rested on a keyring backend not existing is
    // as broken by one arriving as a service name is by being renamed.
    let moved: Vec<&assumptions::Assumption> = readings
        .iter()
        .filter(|(_, r)| matches!(r, Reading::Moved(_) | Reading::Appeared(_)))
        .map(|(a, _)| *a)
        .collect();

    if as_json {
        let report = serde_json::json!({
            "build": path,
            "platform": platform.code(),
            "verified_against": assumptions::verified_against(provider),
            "assumptions": readings.iter().map(|(a, r)| serde_json::json!({
                "name": a.name,
                "reading": match r {
                    Reading::Holds => "holds",
                    Reading::NotReadable => "not_readable",
                    Reading::Moved(_) => "moved",
                    Reading::Appeared(_) => "appeared",
                },
                "gone": match r {
                    Reading::Moved(gone) => gone.clone(),
                    _ => Vec::new(),
                },
                "appeared": match r {
                    Reading::Appeared(found) => found.clone(),
                    _ => Vec::new(),
                },
                "verified_against": a.verified_against,
                "depends": a.depends,
            })).collect::<Vec<_>>(),
            "moved": moved.iter().map(|a| a.name).collect::<Vec<_>>(),
            "not_read_here": elsewhere.iter().map(|a| a.name).collect::<Vec<_>>(),
        });
        println!("{report}");
    } else {
        println!(
            "{path}\na {} build; Pitboard's facts about {} were read from {}\n",
            platform.code(),
            provider.code(),
            assumptions::verified_against(provider)
        );
        for (a, reading) in &readings {
            match reading {
                Reading::Holds => println!("  ok       {}", a.name),
                Reading::NotReadable => {
                    println!(
                        "  no probe {}  (a fact about behaviour, not a name)",
                        a.name
                    );
                }
                Reading::Moved(gone) => {
                    println!("  MOVED    {}", a.name);
                    for needle in gone {
                        println!("             gone: {needle}");
                    }
                    println!("             this breaks: {}", a.depends);
                }
                Reading::Appeared(found) => {
                    println!("  APPEARED {}", a.name);
                    for needle in found {
                        println!("             now present: {needle}");
                    }
                    println!("             this breaks: {}", a.depends);
                }
            }
        }
        for a in &elsewhere {
            let on: Vec<&str> = assumptions::read_on(provider, a.name)
                .iter()
                .map(|p| p.code())
                .collect();
            println!(
                "  skipped  {}  (read from {} builds)",
                a.name,
                on.join(" and ")
            );
        }
        println!();
        if moved.is_empty() {
            println!("Everything Pitboard can read from a build is still there.");
        } else {
            println!(
                "{} of Pitboard's facts can no longer be read from this build. Re-measure \
                 them against it before trusting a switch.",
                moved.len()
            );
        }
    }

    if moved.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The system a binary is built for, from its first four bytes: an ELF binary is taken
/// for Linux and a Mach-O one, thin or universal, for macOS. The tools Pitboard reads ship
/// no other kind.
fn platform_of(bytes: &[u8]) -> Option<Platform> {
    match bytes.get(..4)? {
        [0x7f, b'E', b'L', b'F'] => Some(Platform::Linux),
        [0xcf, 0xfa, 0xed, 0xfe]
        | [0xce, 0xfa, 0xed, 0xfe]
        | [0xfe, 0xed, 0xfa, 0xcf]
        | [0xfe, 0xed, 0xfa, 0xce]
        | [0xca, 0xfe, 0xba, 0xbe] => Some(Platform::MacOs),
        _ => None,
    }
}

/// Every tool with a register, as `--provider` takes it.
fn known() -> Vec<&'static str> {
    ProviderId::ALL.iter().map(|p| p.code()).collect()
}

/// The build to read, the tool whose register to read it against, and whether to answer in
/// JSON. Flags go anywhere; the one argument that is not a flag or a flag's value is the
/// build.
fn parse(args: &[String]) -> Result<(String, ProviderId, bool), String> {
    let mut path = None;
    let mut provider = ProviderId::Claude;
    let mut as_json = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => as_json = true,
            "--provider" => {
                let named = rest.next().ok_or("--provider needs a tool")?;
                provider = ProviderId::parse(named)
                    .ok_or_else(|| format!("--provider takes one of: {}", known().join(", ")))?;
            }
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
            build if path.is_none() => path = Some(build.to_string()),
            extra => return Err(format!("one build at a time, not also {extra}")),
        }
    }
    let path = path.ok_or("no build to read")?;
    Ok((path, provider, as_json))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The system comes from the binary itself, so a build cannot be checked against the
    /// wrong list of facts. Headers as the 2.1.284 packages have them.
    #[test]
    fn the_system_is_read_from_the_binary() {
        assert_eq!(platform_of(b"\x7fELF\x02\x01"), Some(Platform::Linux));
        assert_eq!(platform_of(b"\xcf\xfa\xed\xfe\x0c"), Some(Platform::MacOs));
        assert_eq!(platform_of(b"\xca\xfe\xba\xbe"), Some(Platform::MacOs));
        assert_eq!(platform_of(b"#!/bin/sh"), None);
        assert_eq!(platform_of(b"\x7fE"), None);
    }

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    /// The build was once taken as whatever came two places after `--provider`, so a flag
    /// there was read as the file to open.
    #[test]
    fn flags_go_anywhere_and_the_build_is_the_one_bare_argument() {
        for line in [
            "/b --provider codex --json",
            "--provider codex --json /b",
            "--json /b --provider codex",
        ] {
            assert_eq!(
                parse(&args(line)).unwrap(),
                ("/b".into(), ProviderId::Codex, true),
                "{line}"
            );
        }
        assert_eq!(
            parse(&args("/b")).unwrap(),
            ("/b".into(), ProviderId::Claude, false)
        );
    }

    #[test]
    fn what_cannot_be_read_says_why() {
        assert!(parse(&args("--provider")).is_err());
        assert!(
            parse(&args("/b --provider nothing"))
                .unwrap_err()
                .contains("codex")
        );
        assert!(parse(&args("--json")).is_err());
        assert!(parse(&args("/a /b")).is_err());
    }
}
