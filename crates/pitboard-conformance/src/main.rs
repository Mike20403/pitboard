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
//! the binary's own header, ELF, Mach-O or PE, and a fact is read only from the builds its
//! register's table reads it on. Before that, a Linux build reported both keychain facts
//! gone and a keyring arrived, on every build from 2.1.278 on, while the macOS build missed
//! the one real change in 2.1.281. A fact not read on a build's system is listed with the
//! reason, or with the pull request of the Windows work that reads it there.
//!
//! ```text
//! pitboard-conformance <path to a binary> [--provider claude|codex] [--json]
//! ```
//!
//! Each tool has its own register, and a build of one tool says nothing about another's
//! facts, so a run checks one tool's build against that tool's register. Claude Code is
//! the default, which is what every run before there was a second tool meant.

use pitboard_core::assumptions::{self, Assumption, OnSystem, Platform, Reading};
use pitboard_core::provider::ProviderId;
use std::fmt::Write as _;
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
        eprintln!(
            "{path} is not an ELF, a Mach-O, or an x64 or ARM64 PE binary, so its system is \
             unknown"
        );
        return ExitCode::from(2);
    };
    let strings = assumptions::printable_runs(&bytes, 6);
    let sorted = Sorted::of(provider, platform, &strings);

    if as_json {
        println!("{}", json_report(&path, provider, platform, &sorted));
    } else {
        print!("{}", text_report(&path, provider, platform, &sorted));
    }

    if sorted.moved().is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// One tool's facts as one build reads them, each where its line in the register's table
/// puts it on the build's system.
///
/// A fact not read from this system's builds is left out of the readings, not reported:
/// its literals being absent here says nothing about it. What the register says of it
/// instead is kept, so nothing is skipped without a reason.
struct Sorted {
    /// The facts read on this system, with what the build showed of each.
    readings: Vec<(&'static Assumption, Reading)>,
    /// The facts not read on this system, with why.
    not_read: Vec<(&'static Assumption, &'static str)>,
    /// The facts whose reading on this system waits on the Windows work, with the pull
    /// requests that read them and what those read.
    pending: Vec<(&'static Assumption, &'static [&'static str], &'static str)>,
}

impl Sorted {
    fn of(provider: ProviderId, platform: Platform, strings: &str) -> Sorted {
        let mut sorted = Sorted {
            readings: Vec::new(),
            not_read: Vec::new(),
            pending: Vec::new(),
        };
        for a in assumptions::of(provider) {
            match assumptions::on(provider, a.name, platform)
                .expect("every fact has a line in its register's table")
            {
                OnSystem::Read(_) => sorted
                    .readings
                    .push((a, assumptions::read_from_build(a, strings))),
                OnSystem::NotRead(why) => sorted.not_read.push((a, why)),
                OnSystem::Pending { by, reads } => sorted.pending.push((a, by, reads)),
            }
        }
        sorted
    }

    /// The facts read here that moved. Both kinds of drift count: a fact that rested on a
    /// keyring backend not existing is as broken by one arriving as a service name is by
    /// being renamed.
    fn moved(&self) -> Vec<&'static Assumption> {
        self.readings
            .iter()
            .filter(|(_, r)| matches!(r, Reading::Moved(_) | Reading::Appeared(_)))
            .map(|(a, _)| *a)
            .collect()
    }
}

/// The report `--json` prints.
fn json_report(
    path: &str,
    provider: ProviderId,
    platform: Platform,
    sorted: &Sorted,
) -> serde_json::Value {
    serde_json::json!({
        "build": path,
        "platform": platform.code(),
        "verified_against": assumptions::verified_against(provider, platform),
        "assumptions": sorted.readings.iter().map(|(a, r)| serde_json::json!({
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
            "verified_against": assumptions::verified_on(provider, a.name, platform),
            "depends": a.depends,
        })).collect::<Vec<_>>(),
        "moved": sorted.moved().iter().map(|a| a.name).collect::<Vec<_>>(),
        "not_read_here": sorted.not_read.iter().map(|(a, _)| a.name).collect::<Vec<_>>(),
        "pending_here": sorted.pending.iter().map(|(a, _, _)| a.name).collect::<Vec<_>>(),
    })
}

/// The report a person reads. Writing to a `String` cannot fail, so what `writeln!`
/// returns is dropped.
fn text_report(path: &str, provider: ProviderId, platform: Platform, sorted: &Sorted) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{path}\na {} build; Pitboard's facts about {} were read from {}\n",
        platform.code(),
        provider.code(),
        assumptions::verified_against(provider, platform)
    );
    for (a, reading) in &sorted.readings {
        match reading {
            Reading::Holds => {
                let _ = writeln!(out, "  ok       {}", a.name);
            }
            Reading::NotReadable => {
                let _ = writeln!(
                    out,
                    "  no probe {}  (a fact about behaviour, not a name)",
                    a.name
                );
            }
            Reading::Moved(gone) => {
                let _ = writeln!(out, "  MOVED    {}", a.name);
                for needle in gone {
                    let _ = writeln!(out, "             gone: {needle}");
                }
                let _ = writeln!(out, "             this breaks: {}", a.depends);
            }
            Reading::Appeared(found) => {
                let _ = writeln!(out, "  APPEARED {}", a.name);
                for needle in found {
                    let _ = writeln!(out, "             now present: {needle}");
                }
                let _ = writeln!(out, "             this breaks: {}", a.depends);
            }
        }
    }
    for (a, why) in &sorted.not_read {
        let _ = writeln!(
            out,
            "  skipped  {}  (not read from {} builds)",
            a.name,
            platform.code()
        );
        let _ = writeln!(out, "             {why}");
    }
    for (a, by, reads) in &sorted.pending {
        let _ = writeln!(
            out,
            "  pending  {}  (its {} reading waits on {})",
            a.name,
            platform.code(),
            by.join(" and ")
        );
        let _ = writeln!(out, "             {reads}");
    }
    let _ = writeln!(out);
    let moved = sorted.moved();
    if moved.is_empty() {
        let _ = writeln!(
            out,
            "Everything Pitboard can read from a build is still there."
        );
    } else {
        let _ = writeln!(
            out,
            "{} of Pitboard's facts can no longer be read from this build. Re-measure them \
             against it before trusting a switch.",
            moved.len()
        );
    }
    out
}

/// The system a binary is built for, from its header: an ELF binary is taken for Linux, a
/// Mach-O one, thin or universal, for macOS, and a PE one for x64 or ARM64 for Windows.
/// The tools Pitboard reads ship no other kind, and anything else is refused rather than
/// guessed at.
fn platform_of(bytes: &[u8]) -> Option<Platform> {
    match bytes.get(..4)? {
        [0x7f, b'E', b'L', b'F'] => Some(Platform::Linux),
        [0xcf, 0xfa, 0xed, 0xfe]
        | [0xce, 0xfa, 0xed, 0xfe]
        | [0xfe, 0xed, 0xfa, 0xcf]
        | [0xfe, 0xed, 0xfa, 0xce]
        | [0xca, 0xfe, 0xba, 0xbe] => Some(Platform::MacOs),
        [b'M', b'Z', ..] => windows_machine(bytes),
        _ => None,
    }
}

/// `IMAGE_FILE_MACHINE_AMD64` and `IMAGE_FILE_MACHINE_ARM64`, the two machines Claude Code
/// and Codex ship Windows builds for.
const PE_MACHINES: [u16; 2] = [0x8664, 0xaa64];

/// A Windows binary starts `MZ`, and the four bytes at 0x3C give where its `PE\0\0`
/// signature is. The machine it is for is the two bytes after that. An `MZ` file with no
/// signature where its header says, or one for another machine, is not one this reads.
fn windows_machine(bytes: &[u8]) -> Option<Platform> {
    let at = bytes.get(0x3c..0x40)?;
    let at = usize::try_from(u32::from_le_bytes([at[0], at[1], at[2], at[3]])).ok()?;
    let header = bytes.get(at..at.checked_add(6)?)?;
    if header[..4] != *b"PE\0\0" {
        return None;
    }
    let machine = u16::from_le_bytes([header[4], header[5]]);
    PE_MACHINES.contains(&machine).then_some(Platform::Windows)
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

    /// A PE header: `MZ`, the signature's offset at 0x3C, then the signature and the
    /// machine there.
    fn pe(signature_at: u32, signature: &[u8; 4], machine: u16) -> Vec<u8> {
        let at = signature_at as usize;
        let mut bytes = vec![0; at + 6];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&signature_at.to_le_bytes());
        bytes[at..at + 4].copy_from_slice(signature);
        bytes[at + 4..].copy_from_slice(&machine.to_le_bytes());
        bytes
    }

    /// A Windows build is told by its PE header, for x64 and for ARM64. Before, a Windows
    /// build was refused as neither ELF nor Mach-O. Claude Code 2.1.289's and Codex
    /// 0.160.0's Windows builds, x64 and ARM64, all put the signature at 0x78; the header
    /// says where, and another place is followed as well.
    #[test]
    fn a_windows_build_is_read_from_its_pe_header() {
        assert_eq!(
            platform_of(&pe(0x78, b"PE\0\0", 0x8664)),
            Some(Platform::Windows)
        );
        assert_eq!(
            platform_of(&pe(0x78, b"PE\0\0", 0xaa64)),
            Some(Platform::Windows)
        );
        assert_eq!(
            platform_of(&pe(0x118, b"PE\0\0", 0xaa64)),
            Some(Platform::Windows)
        );
    }

    /// Anything that is not quite a PE header for one of the two machines is unknown, never
    /// taken for Windows on its first two bytes.
    #[test]
    fn what_is_not_an_x64_or_arm64_pe_binary_is_unknown() {
        // A DOS program: `MZ`, and no signature at the offset its header gives.
        let mut dos = vec![0u8; 0x80];
        dos[..2].copy_from_slice(b"MZ");
        assert_eq!(platform_of(&dos), None);
        // Something else where the signature should be.
        assert_eq!(platform_of(&pe(0x80, b"NE\0\0", 0x8664)), None);
        // Cut short: before the offset, inside the signature, and inside the machine.
        assert_eq!(platform_of(b"MZ\x90\x00"), None);
        assert_eq!(platform_of(&pe(0x80, b"PE\0\0", 0x8664)[..0x82]), None);
        assert_eq!(platform_of(&pe(0x80, b"PE\0\0", 0x8664)[..0x85]), None);
        // An offset past the end, and one at the very top of the range.
        let mut far = pe(0x80, b"PE\0\0", 0x8664);
        far[0x3c..0x40].copy_from_slice(&0x1000u32.to_le_bytes());
        assert_eq!(platform_of(&far), None);
        far[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(platform_of(&far), None);
        // 32-bit x86 and 32-bit ARM, which neither tool ships.
        assert_eq!(platform_of(&pe(0x80, b"PE\0\0", 0x014c)), None);
        assert_eq!(platform_of(&pe(0x80, b"PE\0\0", 0x01c4)), None);
    }

    const SYSTEMS: [Platform; 3] = [Platform::MacOs, Platform::Linux, Platform::Windows];

    /// Every literal the facts read on one system probe for, as a build that still holds
    /// them all would show them.
    fn every_probe(provider: ProviderId, platform: Platform) -> String {
        assumptions::of(provider)
            .iter()
            .filter(|a| assumptions::verified_on(provider, a.name, platform).is_some())
            .flat_map(|a| a.probe.iter().copied())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn names<T>(list: &[(&'static Assumption, T)]) -> Vec<&'static str> {
        list.iter().map(|(a, _)| a.name).collect()
    }

    /// Each fact lands once, where its line in the register's table puts it on the build's
    /// system: read, not read with the line's reason, or waiting on the pull requests the
    /// line names.
    #[test]
    fn each_fact_is_read_skipped_or_pending_as_its_line_says() {
        for &provider in ProviderId::ALL {
            for platform in SYSTEMS {
                let sorted = Sorted::of(provider, platform, "");
                let on = |name| assumptions::on(provider, name, platform);
                for (a, _) in &sorted.readings {
                    assert!(matches!(on(a.name), Some(OnSystem::Read(_))), "{}", a.name);
                }
                for &(a, why) in &sorted.not_read {
                    assert_eq!(on(a.name), Some(OnSystem::NotRead(why)), "{}", a.name);
                }
                for &(a, by, reads) in &sorted.pending {
                    assert_eq!(
                        on(a.name),
                        Some(OnSystem::Pending { by, reads }),
                        "{}",
                        a.name
                    );
                }
                let mut seen = names(&sorted.readings);
                seen.extend(names(&sorted.not_read));
                seen.extend(sorted.pending.iter().map(|(a, _, _)| a.name));
                seen.sort_unstable();
                let mut facts: Vec<&str> =
                    assumptions::of(provider).iter().map(|a| a.name).collect();
                facts.sort_unstable();
                assert_eq!(seen, facts, "{provider:?} on {}", platform.code());
            }
        }
    }

    /// Claude Code's Windows build carries `Bun.secrets` for its Credential Manager store,
    /// and none of the keychain facts' literals need be there. Neither is drift, since
    /// neither fact is read there; on Linux the same `Bun.secrets` is a keyring arrived.
    #[test]
    fn what_a_build_is_not_read_for_is_never_drift() {
        let windows = format!(
            "{}\nBun.secrets.get",
            every_probe(ProviderId::Claude, Platform::Windows)
        );
        let sorted = Sorted::of(ProviderId::Claude, Platform::Windows, &windows);
        assert!(sorted.moved().is_empty(), "{:?}", names(&sorted.readings));
        assert!(names(&sorted.not_read).contains(&"no_keyring_off_macos"));
        assert!(names(&sorted.not_read).contains(&"keychain_write_route"));

        let linux = format!(
            "{}\nBun.secrets.get",
            every_probe(ProviderId::Claude, Platform::Linux)
        );
        let sorted = Sorted::of(ProviderId::Claude, Platform::Linux, &linux);
        let moved: Vec<&str> = sorted.moved().iter().map(|a| a.name).collect();
        assert_eq!(moved, ["no_keyring_off_macos"]);
    }

    /// The text lists every fact it does not read, a skipped one with the reason its line
    /// gives and a pending one with the pull requests it waits on and what they read, so
    /// nothing is left out without a word.
    #[test]
    fn the_text_says_why_each_fact_left_out_is_left_out() {
        for &provider in ProviderId::ALL {
            for platform in SYSTEMS {
                let sorted = Sorted::of(provider, platform, &every_probe(provider, platform));
                let text = text_report("/b", provider, platform, &sorted);
                let system = platform.code();
                for (a, why) in &sorted.not_read {
                    let line = format!(
                        "  skipped  {}  (not read from {system} builds)\n             {why}\n",
                        a.name
                    );
                    assert!(text.contains(&line), "{text}");
                }
                for (a, by, reads) in &sorted.pending {
                    let line = format!(
                        "  pending  {}  (its {system} reading waits on {})\n             {reads}\n",
                        a.name,
                        by.join(" and ")
                    );
                    assert!(text.contains(&line), "{text}");
                }
                assert!(
                    text.ends_with("Everything Pitboard can read from a build is still there.\n")
                );
            }
        }
        let sorted = Sorted::of(ProviderId::Claude, Platform::Windows, "");
        let text = text_report("claude.exe", ProviderId::Claude, Platform::Windows, &sorted);
        assert!(text.starts_with(
            "claude.exe\na windows build; Pitboard's facts about claude were read from 2.1.289\n\n"
        ));
        assert!(
            text.contains(
                "  pending  live_chain_order  (its windows reading waits on W22 and W23)\n"
            )
        );
        assert!(!text.contains("MOVED    live_chain_order"));
    }

    /// `--json` lists the facts it does not read in two keys, `not_read_here` and
    /// `pending_here`, beside the readings, and dates each reading by the build its system
    /// was read from.
    #[test]
    fn the_json_lists_what_is_not_read_and_what_waits() {
        for &provider in ProviderId::ALL {
            for platform in SYSTEMS {
                let sorted = Sorted::of(provider, platform, "");
                let report = json_report("/b", provider, platform, &sorted);
                let listed = |key: &str| -> Vec<&str> {
                    report[key]
                        .as_array()
                        .expect(key)
                        .iter()
                        .map(|v| v.as_str().expect(key))
                        .collect()
                };
                assert_eq!(listed("not_read_here"), names(&sorted.not_read));
                let pending: Vec<&str> = sorted.pending.iter().map(|(a, _, _)| a.name).collect();
                assert_eq!(listed("pending_here"), pending);
                let read = report["assumptions"].as_array().expect("assumptions");
                assert_eq!(read.len(), sorted.readings.len());
                for (entry, (a, _)) in read.iter().zip(&sorted.readings) {
                    assert_eq!(entry["name"], a.name);
                    assert_eq!(
                        entry["verified_against"],
                        assumptions::verified_on(provider, a.name, platform).expect("read here")
                    );
                }
                assert_eq!(report["platform"], platform.code());
            }
        }
        let sorted = Sorted::of(ProviderId::Codex, Platform::Windows, "");
        let report = json_report("codex.exe", ProviderId::Codex, Platform::Windows, &sorted);
        assert_eq!(report["verified_against"], "0.160.0");
        assert!(
            report["pending_here"]
                .as_array()
                .expect("pending_here")
                .contains(&serde_json::json!("codex_login_location"))
        );
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
