//! The strings a tool may hash a home folder from, and whether a Credential Manager target's
//! hash suffix is one of them (blocks B2, G4 and H4). Claude Code names a slot's item
//! `Claude Code-credentials-<hash>` and Codex its opt-in items `cli|<hash>` and
//! `secrets|<hash>`; which spelling of the folder each hashes is what W14, W21 and W23 need,
//! and what a non-ASCII account name, an 8.3 short name or a `\\?\` prefix can change.
//!
//! The probe does not claim either tool's formula. It hashes every candidate spelling with
//! SHA-256 and reports which candidates' hex digest begins with the hex a target carries, so
//! the owner's reading names the formula, and the folder itself is never printed.

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

/// One spelling of a folder, with a stable label for the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub label: &'static str,
    pub text: String,
}

/// The lowercase hex SHA-256 of `text`'s UTF-8 bytes.
pub fn sha256_hex(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// Every spelling worth trying for a folder given as `given`, whose canonical form (as
/// `std::fs::canonicalize` returns it, `\\?\` and all) is `canonical` when the folder exists.
pub fn candidates(given: &str, canonical: Option<&str>) -> Vec<Candidate> {
    let mut out = vec![
        Candidate {
            label: "as_given",
            text: given.to_string(),
        },
        Candidate {
            label: "as_given_nfc",
            text: given.nfc().collect(),
        },
        Candidate {
            label: "as_given_forward_slashes",
            text: given.replace('\\', "/"),
        },
    ];
    if let Some(c) = canonical {
        let stripped = strip_verbatim(c);
        out.push(Candidate {
            label: "canonical_verbatim",
            text: c.to_string(),
        });
        out.push(Candidate {
            label: "canonical_stripped",
            text: stripped.clone(),
        });
        out.push(Candidate {
            label: "canonical_stripped_nfc",
            text: stripped.nfc().collect(),
        });
        out.push(Candidate {
            label: "canonical_stripped_lowercase",
            text: stripped.to_lowercase(),
        });
    }
    out
}

/// `path` without a leading `\\?\` (and `\\?\UNC\server` as `\\server`).
pub fn strip_verbatim(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// The run of hex digits a target carries after its family's prefix: after `cli|`,
/// `secrets|` or `-credentials-`, up to the first character that is not hex. `None` when the
/// target has no such run, or one too short to tell candidates apart.
pub fn target_hash(target: &str) -> Option<String> {
    let lower = target.to_lowercase();
    let after = ["cli|", "secrets|", "-credentials-"]
        .iter()
        .find_map(|marker| lower.find(marker).map(|at| &lower[at + marker.len()..]))?;
    let run: String = after.chars().take_while(char::is_ascii_hexdigit).collect();
    (run.len() >= 6).then_some(run)
}

/// The labels of the candidates whose digest begins with `target`'s hash.
pub fn matching_labels(target: &str, candidates: &[Candidate]) -> Vec<&'static str> {
    let Some(hash) = target_hash(target) else {
        return Vec::new();
    };
    candidates
        .iter()
        .filter(|c| sha256_hex(&c.text).starts_with(&hash))
        .map(|c| c.label)
        .collect()
}

/// Facts about an account or folder name that decide which spellings differ, printed in
/// place of the name.
pub fn name_facts(name: &str) -> serde_json::Value {
    let nfc: String = name.nfc().collect();
    let nfd: String = name.nfd().collect();
    serde_json::json!({
        "is_ascii": name.is_ascii(),
        "utf16_units": name.encode_utf16().count(),
        "utf8_bytes": name.len(),
        "is_nfc": nfc == name,
        "is_nfd": nfd == name,
        "has_space": name.contains(' '),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_is_the_standard_digest() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn the_candidates_cover_nfc_and_the_verbatim_prefix() {
        // "Đạt" written decomposed: the NFC candidate differs from the given one.
        let decomposed = "C:\\Users\\D\u{0323}a\u{0323}t";
        let c = candidates(decomposed, Some(r"\\?\C:\Users\Đạt"));
        let get = |l| c.iter().find(|x| x.label == l).unwrap().text.clone();
        assert_ne!(get("as_given"), get("as_given_nfc"));
        assert_eq!(get("canonical_stripped"), r"C:\Users\Đạt");
        assert_eq!(get("canonical_verbatim"), r"\\?\C:\Users\Đạt");
        assert_eq!(
            get("as_given_forward_slashes"),
            "C:/Users/D\u{0323}a\u{0323}t"
        );
    }

    #[test]
    fn verbatim_unc_strips_to_a_plain_unc_path() {
        assert_eq!(strip_verbatim(r"\\?\UNC\srv\share\x"), r"\\srv\share\x");
        assert_eq!(strip_verbatim(r"C:\x"), r"C:\x");
    }

    #[test]
    fn a_target_hash_is_read_after_its_family() {
        assert_eq!(target_hash("cli|0123abcd"), Some("0123abcd".into()));
        assert_eq!(
            target_hash("cli|0123ABCDef.Codex Auth"),
            Some("0123abcdef".into())
        );
        assert_eq!(target_hash("secrets|deadbeef99"), Some("deadbeef99".into()));
        assert_eq!(
            target_hash("Claude Code-credentials-e80beed8#0"),
            Some("e80beed8".into())
        );
        assert_eq!(target_hash("Claude Code-credentials"), None);
        assert_eq!(target_hash("cli|abc"), None);
    }

    #[test]
    fn a_target_matches_the_candidate_it_was_hashed_from() {
        let c = candidates(r"C:\scratch\codex", Some(r"\\?\C:\scratch\codex"));
        let stripped = sha256_hex(r"C:\scratch\codex");
        let target = format!("cli|{}", &stripped[..16]);
        let labels = matching_labels(&target, &c);
        assert!(labels.contains(&"canonical_stripped"), "{labels:?}");
        assert!(labels.contains(&"as_given"));
        assert!(!labels.contains(&"canonical_verbatim"));
        assert!(matching_labels("cli|ffffffffffff", &c).is_empty());
    }

    #[test]
    fn name_facts_say_what_differs_without_the_name() {
        let f = name_facts("Đạt Thử");
        assert_eq!(f["is_ascii"], false);
        assert_eq!(f["is_nfc"], true);
        assert_eq!(f["has_space"], true);
        assert_eq!(name_facts("dana")["is_ascii"], true);
    }
}
