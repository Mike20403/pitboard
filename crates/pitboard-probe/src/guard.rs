//! The guards every writing subcommand passes before it touches anything, written as pure
//! logic so a test proves them on every system. The Windows calls that find the real
//! profile, make a path absolute, resolve its existing part (8.3 short names, junctions and
//! symbolic links included) and look for the marker file live in [`crate::win`]; they hand
//! their answers to the functions here.
//!
//! A scratch path is compared against the forbidden folders in every form the Windows side
//! has: as given once made absolute, and with its existing part resolved. Each form must be
//! safe. A form that is not absolute, or that keeps a `..`, is refused outright.

use std::path::{Path, PathBuf};

/// The real profile's folders that a scratch path must never lie inside: a mistake there
/// would read or write a real login. The whole profile is not refused, because the usual
/// scratch root, `%LOCALAPPDATA%\Temp`, is itself inside the profile.
#[derive(Debug, Clone)]
pub struct RealProfile {
    /// `FOLDERID_Profile`, e.g. `C:\Users\dana`.
    pub profile: PathBuf,
    /// `FOLDERID_LocalAppData`, e.g. `C:\Users\dana\AppData\Local`.
    pub local_app_data: PathBuf,
}

impl RealProfile {
    /// `<profile>\.claude`, `<profile>\.codex` and `<LocalAppData>\Pitboard`.
    pub fn forbidden_roots(&self) -> Vec<PathBuf> {
        vec![
            self.profile.join(".claude"),
            self.profile.join(".codex"),
            self.local_app_data.join("Pitboard"),
        ]
    }
}

/// Whether `scratch` is safe to write under: it is absolute, holds no `..`, and is not, and
/// does not lie inside, any of `forbidden`. The profile's `Temp` and everything else is
/// allowed.
pub fn scratch_is_safe(scratch: &Path, forbidden: &[PathBuf]) -> bool {
    let Some(p) = normalise(scratch) else {
        return false;
    };
    if p.components.iter().any(|c| c == "..") {
        return false;
    }
    !forbidden.iter().any(|root| match normalise(root) {
        Some(r) => r.contains(&p),
        // A root the probe cannot read as a path is one it cannot rule out.
        None => true,
    })
}

/// Whether `path` is `ancestor` or lies inside it, comparing as Windows does: either
/// separator, any case, the `\\?\` and `\\.\` prefixes dropped, trailing dots and spaces of
/// a component dropped. Two paths that are not both absolute compare as unrelated.
pub fn is_within(path: &Path, ancestor: &Path) -> bool {
    match (normalise(path), normalise(ancestor)) {
        (Some(p), Some(a)) => a.contains(&p),
        _ => false,
    }
}

/// Whether `name` is a plain file name the probe may make inside a folder it was given: no
/// separator, no drive or stream colon, not `.` or `..`, no trailing dot or space, and
/// nothing a Windows file name refuses.
pub fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && name != "."
        && name != ".."
        && !name.ends_with(['.', ' '])
        && !name
            .chars()
            .any(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c < ' ')
}

/// A path as Windows compares it: its root (a drive, or a UNC server and share) and its
/// components, each folded to lower case.
#[derive(Debug, PartialEq, Eq)]
struct Normal {
    root: String,
    components: Vec<String>,
}

impl Normal {
    /// Whether `self` is `other` or one of its ancestors.
    fn contains(&self, other: &Normal) -> bool {
        self.root == other.root
            && self.components.len() <= other.components.len()
            && self
                .components
                .iter()
                .zip(other.components.iter())
                .all(|(a, b)| a == b)
    }
}

/// Split a Windows path, given as text on any system, into its root and folded components.
/// `None` for a path that is not absolute: relative, drive-relative (`C:x`) or rooted on
/// the current drive (`\x`).
fn normalise(path: &Path) -> Option<Normal> {
    let text = path.to_string_lossy().replace('/', "\\");
    // `\\?\UNC\server\share` and `\\.\UNC\...` are the UNC path `\\server\share`; `\\?\C:\`,
    // `\\.\C:\` and NT's `\??\C:\` are `C:\`.
    let (unc, rest) = if let Some(r) =
        strip_prefix_ci(&text, r"\\?\UNC\").or_else(|| strip_prefix_ci(&text, r"\\.\UNC\"))
    {
        (true, r.to_string())
    } else if let Some(r) = text
        .strip_prefix(r"\\?\")
        .or_else(|| text.strip_prefix(r"\\.\"))
        .or_else(|| text.strip_prefix(r"\??\"))
    {
        (false, r.to_string())
    } else if let Some(r) = text.strip_prefix(r"\\") {
        (true, r.to_string())
    } else {
        (false, text.clone())
    };

    let folded: Vec<String> = rest
        .split('\\')
        .filter(|s| !s.is_empty() && *s != ".")
        .map(fold)
        .collect();
    // A component that is nothing but dots and spaces (`...`) folds to nothing; what Windows
    // makes of it is not worth guessing, so the path is refused.
    if folded.iter().any(String::is_empty) {
        return None;
    }
    let mut parts = folded.into_iter();
    let root = if unc {
        let server = parts.next()?;
        let share = parts.next()?;
        format!(r"\\{server}\{share}")
    } else {
        let drive = parts.next()?;
        let bytes = drive.as_bytes();
        if bytes.len() != 2 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
            return None;
        }
        // A drive given with no separator after it (`C:` or `C:x`) is relative to that
        // drive's current folder.
        if !rest.get(2..).is_some_and(|r| r.starts_with('\\')) {
            return None;
        }
        drive
    };
    Some(Normal {
        root,
        components: parts.collect(),
    })
}

/// One component as Windows compares it: trailing dots and spaces dropped (Win32 drops them
/// when it opens a path, so `.claude.` opens `.claude`), then folded to lower case across
/// all of Unicode, not ASCII alone. `..` is kept as it is, for the caller to refuse.
fn fold(component: &str) -> String {
    if component == ".." {
        return component.to_string();
    }
    component.trim_end_matches(['.', ' ']).to_lowercase()
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> RealProfile {
        RealProfile {
            profile: PathBuf::from(r"C:\Users\dana"),
            local_app_data: PathBuf::from(r"C:\Users\dana\AppData\Local"),
        }
    }

    fn safe(p: &str) -> bool {
        scratch_is_safe(Path::new(p), &profile().forbidden_roots())
    }

    #[test]
    fn a_scratch_under_temp_is_safe_even_though_temp_is_in_the_profile() {
        assert!(safe(r"C:\Users\dana\AppData\Local\Temp\pitboard-probe-1"));
        assert!(safe(r"D:\a\_temp\pitboard-probe-scratch"));
        assert!(safe(r"\\localhost\pbprobe\probe"));
    }

    #[test]
    fn a_scratch_inside_a_real_login_folder_is_refused() {
        for p in [
            r"C:\Users\dana\.claude",
            r"C:\Users\dana\.claude\x",
            r"C:\Users\dana\.codex\auth.json",
            r"C:\Users\dana\AppData\Local\Pitboard",
            r"C:\Users\dana\AppData\Local\Pitboard\vault\a",
        ] {
            assert!(!safe(p), "{p} should be refused");
        }
    }

    #[test]
    fn the_comparison_folds_case_and_separators() {
        assert!(!safe(r"c:/users/DANA/.CLAUDE/x"));
    }

    #[test]
    fn the_comparison_folds_non_ascii_case() {
        let roots = RealProfile {
            profile: PathBuf::from(r"C:\Users\Đạt Thử"),
            local_app_data: PathBuf::from(r"C:\Users\Đạt Thử\AppData\Local"),
        }
        .forbidden_roots();
        assert!(!scratch_is_safe(
            Path::new(r"c:\users\đạt thử\.claude\x"),
            &roots
        ));
        assert!(!scratch_is_safe(
            Path::new(r"C:\USERS\ĐẠT THỬ\.codex"),
            &roots
        ));
    }

    #[test]
    fn verbatim_and_device_prefixes_are_seen_through() {
        assert!(!safe(r"\\?\C:\Users\dana\.claude"));
        assert!(!safe(r"\\.\C:\Users\dana\.codex\x"));
        assert!(!safe(r"\??\C:\Users\dana\AppData\Local\Pitboard"));
        // The UNC forms reach the same share.
        let unc = [PathBuf::from(r"\\server\share\.claude")];
        assert!(!scratch_is_safe(
            Path::new(r"\\?\UNC\server\share\.claude\x"),
            &unc
        ));
        assert!(!scratch_is_safe(Path::new(r"\\SERVER\Share\.claude"), &unc));
    }

    #[test]
    fn a_trailing_dot_or_space_names_the_same_folder() {
        assert!(!safe(r"C:\Users\dana\.claude."));
        assert!(!safe(r"C:\Users\dana\.claude \x"));
        assert!(!safe(r"C:\Users\dana\.codex...\x"));
        // A component of dots alone is refused rather than guessed at.
        assert!(!safe(r"C:\Users\dana\...\.claude"));
    }

    #[test]
    fn a_relative_or_drive_relative_path_is_refused() {
        // A console's default folder is often the profile, where `.claude` is the real one.
        assert!(!safe(r".claude"));
        assert!(!safe(r"scratch\x"));
        assert!(!safe(r"C:.claude"));
        assert!(!safe(r"C:"));
        assert!(!safe(r"\Users\dana\AppData\Local\Temp\x"));
        assert!(!safe(""));
    }

    #[test]
    fn a_sibling_that_merely_shares_a_prefix_is_safe() {
        assert!(safe(r"C:\Users\dana\.claudex\x"));
        assert!(safe(r"C:\Users\dana\AppData\Local\PitboardProbe"));
    }

    #[test]
    fn a_parent_component_is_refused() {
        assert!(!safe(r"C:\Users\dana\AppData\Local\Temp\..\.claude"));
        assert!(!safe(r"C:\Users\dana\AppData\Local\Temp\..\x"));
    }

    #[test]
    fn a_root_the_probe_cannot_read_rules_everything_out() {
        assert!(!scratch_is_safe(
            Path::new(r"C:\x"),
            &[PathBuf::from("relative-root")]
        ));
    }

    #[test]
    fn is_within_compares_absolute_paths_only() {
        assert!(is_within(Path::new(r"C:\a\b"), Path::new(r"c:\A")));
        assert!(is_within(Path::new(r"C:\a"), Path::new(r"C:\a")));
        assert!(!is_within(Path::new(r"C:\ab"), Path::new(r"C:\a")));
        assert!(!is_within(Path::new(r"D:\a\b"), Path::new(r"C:\a")));
        assert!(!is_within(Path::new(r"a\b"), Path::new(r"a")));
    }

    #[test]
    fn forbidden_roots_are_the_three_login_folders() {
        let roots = profile().forbidden_roots();
        assert_eq!(roots.len(), 3);
        for p in [
            r"C:\Users\dana\.claude",
            r"C:\Users\dana\.codex",
            r"C:\Users\dana\AppData\Local\Pitboard",
        ] {
            assert!(
                roots.iter().any(|r| is_within(Path::new(p), r)),
                "{p} should be one of the forbidden roots"
            );
        }
    }

    #[test]
    fn plain_file_names_carry_no_path_or_stream() {
        for ok in ["pitboard-probe-a.json", ".credentials.json", "a b"] {
            assert!(is_plain_file_name(ok), "{ok}");
        }
        for bad in [
            "", ".", "..", r"..\x", "a/b", r"a\b", "C:x", "a:stream", "a.", "a ", "a*", "a\u{1}",
        ] {
            assert!(!is_plain_file_name(bad), "{bad:?}");
        }
    }
}
