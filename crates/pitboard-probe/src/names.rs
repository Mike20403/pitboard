//! Which Credential Manager targets `credman-names` asks for, which of what comes back it
//! may report, and what it may say about each.
//!
//! The probe never enumerates the whole vault. It asks Credential Manager once per prefix in
//! [`ENUMERATION_FILTERS`], so another application's item is never loaded into the process
//! at all. Credential Manager filters by prefix only, so a target named with the family at
//! its end (keyring-rs's `<user>.<service>` form, which Codex's MCP store may use) cannot be
//! asked for; [`UNLISTABLE_FORMS`] says so in the report, and the runbook's H4 finds it by
//! hand.
//!
//! What comes back is classified by name alone:
//!
//! - a **live login family**: the targets Claude Code and Codex keep a login in. In a
//!   throwaway account (the marker present) the probe reports the item's type, persistence,
//!   redacted user name and comment, attribute keywords and sizes, and blob size, which G3
//!   needs; elsewhere only the name and the family. Never the blob. Finding one is what the
//!   CI leak check fails on.
//! - a **citest** name (`pitboard-citest-*`): what the integration tests write. Reported
//!   like a live family; the leak check fails on it too.
//! - a **probe** name (`pitboard-probe-*`): the probe's own item, reported in full but for
//!   the blob.
//! - anything else a filter happened to return: `Other`, of which nothing is said.

use crate::PROBE_PREFIX;

/// What the integration tests name their Credential Manager items.
pub const CITEST_PREFIX: &str = "pitboard-citest-";

/// The prefixes `credman-names` enumerates, one `CredEnumerateW` call each. `pitboard-*`
/// covers both the probe's and the tests' names.
pub const ENUMERATION_FILTERS: [&str; 5] = [
    "Claude Code*",
    "cli|*",
    "secrets|*",
    "Codex MCP Credentials*",
    "pitboard-*",
];

/// Forms of a live family a prefix filter cannot reach.
pub const UNLISTABLE_FORMS: [&str; 1] = ["<key>.Codex MCP Credentials"];

/// The live login families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// `Claude Code-credentials`, `Claude Code-credentials-<hash>`, and every piece of one
    /// (`#0` onwards, `#m`, `#p`).
    ClaudeCode,
    /// Codex's `cli|*` targets ("Codex Auth").
    CodexCli,
    /// Codex's `secrets|*` targets ("codex").
    CodexSecrets,
    /// Anything under `Codex MCP Credentials`, at the start of the target or, in keyring-rs's
    /// `<user>.<service>` form, at its end.
    CodexMcp,
}

impl Family {
    pub fn as_str(self) -> &'static str {
        match self {
            Family::ClaudeCode => "claude_code_credentials",
            Family::CodexCli => "codex_cli",
            Family::CodexSecrets => "codex_secrets",
            Family::CodexMcp => "codex_mcp_credentials",
        }
    }
}

/// What may be said about a Credential Manager target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Live(Family),
    CiTest,
    Probe,
    Other,
}

impl Kind {
    /// Whether finding this target is a leak the CI step fails on.
    pub fn is_leak(self) -> bool {
        matches!(self, Kind::Live(_) | Kind::CiTest)
    }

    pub fn is_reportable(self) -> bool {
        !matches!(self, Kind::Other)
    }
}

/// Classify a Credential Manager target name, ignoring case as Credential Manager does. A
/// `LegacyGeneric:target=` prefix, which `cmdkey` shows, is seen through.
pub fn classify(target: &str) -> Kind {
    let lower = target.to_lowercase();
    let lower = lower
        .strip_prefix("legacygeneric:target=")
        .unwrap_or(&lower);

    // The probe's own and the tests' own come first, so a name that begins with one of those
    // prefixes is never mistaken for a live family.
    if lower.starts_with(PROBE_PREFIX) {
        return Kind::Probe;
    }
    if lower.starts_with(CITEST_PREFIX) {
        return Kind::CiTest;
    }
    if lower.starts_with("claude code") && lower.contains("-credentials") {
        return Kind::Live(Family::ClaudeCode);
    }
    // Anything under the MCP store's name is the family, however it goes on; and keyring-rs's
    // `<user>.<service>` form puts the store's name at the end.
    if lower.starts_with("codex mcp credentials") || lower.ends_with(".codex mcp credentials") {
        return Kind::Live(Family::CodexMcp);
    }
    if lower.starts_with("cli|") {
        return Kind::Live(Family::CodexCli);
    }
    if lower.starts_with("secrets|") {
        return Kind::Live(Family::CodexSecrets);
    }
    Kind::Other
}

/// Whether `name` is one the probe may write, read or delete itself: a `pitboard-probe-*`
/// target of printable ASCII, short enough for Credential Manager's limit.
pub fn is_probe_item_name(name: &str) -> bool {
    name.len() > PROBE_PREFIX.len()
        && name.len() <= 256
        && name.starts_with(PROBE_PREFIX)
        && name.bytes().all(|b| b.is_ascii_graphic())
        && classify(name) == Kind::Probe
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_live_claude_code_families_are_recognised() {
        for name in [
            "Claude Code-credentials",
            "Claude Code-credentials/claude-code-user",
            "Claude Code-credentials-e80beed8",
            "Claude Code-credentials-e80beed8#0",
            "Claude Code-credentials-e80beed8#m",
            "Claude Code-credentials-e80beed8#p",
        ] {
            assert_eq!(classify(name), Kind::Live(Family::ClaudeCode), "{name}");
        }
    }

    #[test]
    fn the_live_codex_families_are_recognised() {
        assert_eq!(classify("cli|abc123"), Kind::Live(Family::CodexCli));
        assert_eq!(
            classify("cli|abc123.Codex Auth"),
            Kind::Live(Family::CodexCli)
        );
        assert_eq!(classify("secrets|abc123"), Kind::Live(Family::CodexSecrets));
        assert_eq!(
            classify("Codex MCP Credentials"),
            Kind::Live(Family::CodexMcp)
        );
        assert_eq!(
            classify("Codex MCP Credentials/server"),
            Kind::Live(Family::CodexMcp)
        );
    }

    #[test]
    fn keyring_rs_user_dot_service_form_of_the_mcp_store_is_live() {
        assert_eq!(
            classify("linear|3f2a.Codex MCP Credentials"),
            Kind::Live(Family::CodexMcp)
        );
        assert_eq!(
            classify("LegacyGeneric:target=x.codex mcp credentials"),
            Kind::Live(Family::CodexMcp)
        );
        assert!(classify("x.Codex MCP Credentials").is_leak());
    }

    #[test]
    fn the_match_folds_case_the_way_credential_manager_does() {
        assert_eq!(
            classify("claude code-credentials"),
            Kind::Live(Family::ClaudeCode)
        );
        assert_eq!(classify("CLI|X"), Kind::Live(Family::CodexCli));
        assert_eq!(classify("PITBOARD-PROBE-x"), Kind::Probe);
    }

    #[test]
    fn the_probe_and_the_tests_name_their_own() {
        assert_eq!(classify("pitboard-probe-vault-1"), Kind::Probe);
        assert_eq!(classify("pitboard-citest-abcd"), Kind::CiTest);
    }

    #[test]
    fn everyone_elses_items_are_other_and_say_nothing() {
        for name in [
            "GitHub - https://github.com",
            "MicrosoftAccount:user=someone@example.com",
            "LegacyGeneric:target=some-app",
            "Claude something else",
            "my cli| thing",
        ] {
            let k = classify(name);
            assert_eq!(k, Kind::Other, "{name}");
            assert!(!k.is_reportable());
            assert!(!k.is_leak());
        }
    }

    #[test]
    fn a_leak_is_a_live_family_or_a_citest_name() {
        assert!(classify("Claude Code-credentials").is_leak());
        assert!(classify("pitboard-citest-x").is_leak());
        assert!(!classify("pitboard-probe-x").is_leak());
        assert!(!classify("GitHub").is_leak());
    }

    #[test]
    fn a_probe_prefix_wins_over_a_family_lookalike() {
        assert_eq!(classify("pitboard-probe-cli|x"), Kind::Probe);
    }

    #[test]
    fn every_enumeration_filter_is_a_prefix_of_a_reportable_name() {
        for filter in ENUMERATION_FILTERS {
            let stem = filter.trim_end_matches('*');
            assert!(filter.ends_with('*') && !stem.contains('*'), "{filter}");
            // A name the filter returns is reportable once it is a family's full form.
        }
        assert!(classify("Claude Code-credentials").is_reportable());
        assert!(classify("cli|x").is_reportable());
        assert!(classify("secrets|x").is_reportable());
        assert!(classify("Codex MCP Credentials").is_reportable());
        assert!(classify("pitboard-probe-x").is_reportable());
    }

    #[test]
    fn the_probe_writes_only_its_own_names() {
        assert!(is_probe_item_name("pitboard-probe-m1"));
        for bad in [
            "pitboard-probe-",
            "Claude Code-credentials",
            "pitboard-citest-x",
            "cli|x",
            "pitboard-probe-a b",
            "x-pitboard-probe-y",
        ] {
            assert!(!is_probe_item_name(bad), "{bad}");
        }
    }
}
