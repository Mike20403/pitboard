//! The console-allocation block (VM E4) and the runner facts' `console`: three builds of the
//! probe that differ only in the manifest built into them, each started several ways, each
//! recording from inside whether it got a console and whether that console has a window.
//! `consoleAllocationPolicy=detached` changes only what happens to a console program that
//! has no console to inherit, so that scenario is the one that answers whether the policy is
//! honoured; the others show it changes nothing else.

/// The three builds. Each is a `/SUBSYSTEM:CONSOLE` program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// No `consoleAllocationPolicy`.
    Plain,
    /// `consoleAllocationPolicy=detached` in the documented spelling
    /// (`asmv3:windowsSettings`, `manifests/detached.manifest`).
    Detached,
    /// The same policy in an undocumented asm.v1 spelling
    /// (`manifests/detached-asmv1.manifest`).
    DetachedAsmV1,
}

impl Variant {
    pub const ALL: [Variant; 3] = [Variant::Plain, Variant::Detached, Variant::DetachedAsmV1];

    pub fn as_str(self) -> &'static str {
        match self {
            Variant::Plain => "plain",
            Variant::Detached => "detached_documented",
            Variant::DetachedAsmV1 => "detached_asmv1",
        }
    }

    /// The program cargo builds for this variant.
    pub fn bin_name(self) -> &'static str {
        match self {
            Variant::Plain => "pitboard-probe",
            Variant::Detached => "pitboard-probe-detached",
            Variant::DetachedAsmV1 => "pitboard-probe-detached-asmv1",
        }
    }

    /// The spelling its manifest should carry once embedded.
    pub fn expected_spelling(self) -> Spelling {
        match self {
            Variant::Plain => Spelling::None,
            Variant::Detached => Spelling::Asmv3WindowsSettings,
            Variant::DetachedAsmV1 => Spelling::Asmv1Application,
        }
    }
}

/// How the variant is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// From a parent that itself has no console (started with `DETACHED_PROCESS`), with no
    /// creation flags: the one case the policy changes. A plain console program gets a new
    /// console, with a window; a honoured `detached` gets none.
    NoConsoleToInherit,
    /// From a parent started with `CREATE_NO_WINDOW`, with no flags: it inherits a console
    /// that has no window.
    CreateNoWindowParent,
    /// Started with `CREATE_NO_WINDOW` itself.
    CreateNoWindow,
    /// Started with `DETACHED_PROCESS` itself.
    DetachedProcess,
    /// From this probe, with no flags: it attaches to the console the probe runs in, and must
    /// still do so with the policy set.
    FromConsoleParent,
    /// As a Task Scheduler task's action (in a marked account only, with `--with-task`).
    FromTask,
}

impl Scenario {
    pub const ALL: [Scenario; 6] = [
        Scenario::NoConsoleToInherit,
        Scenario::CreateNoWindowParent,
        Scenario::CreateNoWindow,
        Scenario::DetachedProcess,
        Scenario::FromConsoleParent,
        Scenario::FromTask,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Scenario::NoConsoleToInherit => "no_console_to_inherit",
            Scenario::CreateNoWindowParent => "create_no_window_parent",
            Scenario::CreateNoWindow => "create_no_window",
            Scenario::DetachedProcess => "detached_process",
            Scenario::FromConsoleParent => "from_console_parent",
            Scenario::FromTask => "from_task",
        }
    }
}

/// Which spelling of the policy a manifest carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spelling {
    None,
    Asmv3WindowsSettings,
    Asmv1Application,
    /// The policy is there, in a form neither of the above.
    Other,
}

impl Spelling {
    pub fn as_str(self) -> &'static str {
        match self {
            Spelling::None => "none",
            Spelling::Asmv3WindowsSettings => "asmv3_windows_settings",
            Spelling::Asmv1Application => "asmv1_application",
            Spelling::Other => "other",
        }
    }
}

/// Read the policy's spelling from a manifest's text, as embedded in a program. The linker
/// merges the manifest with its own and may rename namespace prefixes, so the test is on
/// the shape: a prefixed `windowsSettings` (the documented `asmv3:` form) or an unprefixed
/// one. Comments are dropped first.
pub fn spelling(manifest: &str) -> Spelling {
    let text = without_comments(manifest);
    if !text.contains("consoleAllocationPolicy") {
        return Spelling::None;
    }
    if text.contains(":windowsSettings") {
        Spelling::Asmv3WindowsSettings
    } else if text.contains("<windowsSettings") {
        Spelling::Asmv1Application
    } else {
        Spelling::Other
    }
}

/// The part of a manifest around the policy, for the report: the embedded manifest is the
/// probe's own and the linker's, so it holds nothing about the account.
pub fn excerpt(manifest: &str) -> Option<String> {
    let text = without_comments(manifest);
    let at = text.find("consoleAllocationPolicy")?;
    let start = text[..at]
        .char_indices()
        .rev()
        .nth(160)
        .map_or(0, |(i, _)| i);
    let end = text[at..]
        .char_indices()
        .nth(120)
        .map_or(text.len(), |(i, _)| at + i);
    Some(
        text[start..end]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// The creation flags the hidden `console-launch` go-between may start a variant with: none,
/// `CREATE_NO_WINDOW` or `DETACHED_PROCESS`.
pub const LAUNCH_FLAGS: [u32; 3] = [0, 0x0800_0000, 0x0000_0008];

/// Whether `file_name` is one of the variants' programs, which is all `console-launch`
/// starts.
pub fn is_variant_program(file_name: &str) -> bool {
    Variant::ALL
        .iter()
        .any(|v| file_name.eq_ignore_ascii_case(&format!("{}.exe", v.bin_name())))
}

fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("<!--") {
        out.push_str(&rest[..open]);
        match rest[open..].find("-->") {
            Some(close) => rest = &rest[open + close + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_names_its_own_program() {
        assert_eq!(Variant::Plain.bin_name(), "pitboard-probe");
        assert_eq!(Variant::Detached.bin_name(), "pitboard-probe-detached");
        assert_eq!(
            Variant::DetachedAsmV1.bin_name(),
            "pitboard-probe-detached-asmv1"
        );
    }

    #[test]
    fn the_manifests_in_the_crate_carry_the_spelling_their_variant_names() {
        let documented = include_str!("../manifests/detached.manifest");
        let asmv1 = include_str!("../manifests/detached-asmv1.manifest");
        assert_eq!(spelling(documented), Variant::Detached.expected_spelling());
        assert_eq!(spelling(asmv1), Variant::DetachedAsmV1.expected_spelling());
        assert_eq!(spelling("<assembly/>"), Variant::Plain.expected_spelling());
        // A manifest whose only mention of the policy is a comment carries none.
        assert_eq!(
            spelling("<!-- consoleAllocationPolicy --><assembly/>"),
            Spelling::None
        );
    }

    #[test]
    fn the_excerpt_shows_the_policy_and_its_element() {
        let documented = include_str!("../manifests/detached.manifest");
        let e = excerpt(documented).unwrap();
        assert!(e.contains("consoleAllocationPolicy>detached"), "{e}");
        assert!(e.contains("asmv3:windowsSettings"), "{e}");
        assert_eq!(excerpt("<assembly/>"), None);
    }

    #[test]
    fn the_go_between_starts_only_a_variant_with_a_known_flag() {
        assert!(is_variant_program("pitboard-probe.exe"));
        assert!(is_variant_program("PITBOARD-PROBE-DETACHED.EXE"));
        assert!(is_variant_program("pitboard-probe-detached-asmv1.exe"));
        for bad in ["claude.exe", "pitboard-probe", "pitboard.exe", "cmd.exe"] {
            assert!(!is_variant_program(bad), "{bad}");
        }
        assert!(LAUNCH_FLAGS.contains(&0));
        assert!(!LAUNCH_FLAGS.contains(&0x10));
    }

    #[test]
    fn there_is_a_scenario_with_no_console_to_inherit() {
        let names: Vec<_> = Scenario::ALL.iter().map(|s| s.as_str()).collect();
        assert!(names.contains(&"no_console_to_inherit"));
        assert!(names.contains(&"from_task"));
        assert_eq!(names.len(), 6);
    }
}
