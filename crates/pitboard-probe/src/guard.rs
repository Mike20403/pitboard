//! The guards every writing subcommand passes before it touches anything, written as pure
//! logic so a test proves them on every system. The Windows calls that find the real
//! profile, make a path absolute, resolve its existing part (8.3 short names, junctions and
//! symbolic links included), read the identity of each existing folder and look for the
//! marker file live in [`crate::win`]; they hand their answers to the functions here.
//!
//! A scratch path is refused outright when, as given, it is not absolute (relative,
//! drive-relative such as `C:x`, or rooted on the current drive such as `\x`) or keeps a
//! `..`: nothing is resolved against the working folder. Otherwise it is compared against
//! the forbidden roots in every form the Windows side has (as given, made absolute, and with
//! its existing part resolved), and each form must be safe: not a forbidden root nor inside
//! one, no `:` after the drive (so no stream such as `::$INDEX_ALLOCATION`), no
//! administrative or hidden share (a share name ending in `$`, such as `C$`), and, as
//! written, no UNC path to this machine. Then every existing ancestor is compared by
//! identity, its volume and file id, with the forbidden roots and their parent folders
//! ([`identity_refuses`]), which sees through any spelling the text rules miss: a mapped
//! drive, a share, a link, a junction.

use std::path::{Path, PathBuf};

/// The names inside a profile folder that hold a real login: Claude Code's folder and its
/// state file, and Codex's folder.
pub const PROFILE_LOGIN_NAMES: [&str; 3] = [".claude", ".claude.json", ".codex"];

/// The real profile's folders and files that a scratch path must never be or lie inside: a
/// mistake there would read or write a real login. The whole profile is not refused,
/// because the usual scratch root, `%LOCALAPPDATA%\Temp`, is itself inside the profile.
#[derive(Debug, Clone, Default)]
pub struct RealProfile {
    /// Every folder that may be this account's profile: `FOLDERID_Profile` asked with the
    /// process token and without one, `GetUserProfileDirectoryW`, and `%USERPROFILE%` when
    /// it is absolute, since a tool started from this process follows it.
    pub profiles: Vec<PathBuf>,
    /// Every folder that may be this account's local app data: `FOLDERID_LocalAppData`
    /// asked with the token and without one, and `%LOCALAPPDATA%` when it is absolute.
    pub local_app_data: Vec<PathBuf>,
}

impl RealProfile {
    /// For each profile, `.claude`, `.claude.json`, `.codex` and `AppData\Local\Pitboard`;
    /// for each local app data folder, `Pitboard`. Each once.
    pub fn forbidden_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for profile in &self.profiles {
            for name in PROFILE_LOGIN_NAMES {
                roots.push(profile.join(name));
            }
            roots.push(profile.join(r"AppData\Local\Pitboard"));
        }
        for lad in &self.local_app_data {
            roots.push(lad.join("Pitboard"));
        }
        let mut out: Vec<PathBuf> = Vec::new();
        for root in roots {
            if !out.iter().any(|r| same_path(r, &root)) {
                out.push(root);
            }
        }
        out
    }
}

/// Whether two paths name the same place as Windows compares them as text.
fn same_path(a: &Path, b: &Path) -> bool {
    match (normalise(a), normalise(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// Whether `scratch` is safe to write under: it is absolute, holds no `..`, no `:` after its
/// drive and no administrative or hidden share, and is not, and does not lie inside, any of
/// `forbidden`. The profile's `Temp` and everything else is allowed.
pub fn scratch_is_safe(scratch: &Path, forbidden: &[PathBuf]) -> bool {
    let Some(p) = normalise(scratch) else {
        return false;
    };
    if p.components.iter().any(|c| c == "..") {
        return false;
    }
    if let Root::Unc { share, .. } = &p.root
        && share.ends_with('$')
    {
        return false;
    }
    !forbidden.iter().any(|root| match normalise(root) {
        Some(r) => r.contains(&p),
        // A root the probe cannot read as a path is one it cannot rule out.
        None => true,
    })
}

/// Whether `path` is written as a UNC path to this machine: `localhost`, an IPv4 address in
/// `127.0.0.0/8` or `0.0.0.0` in any of the forms Windows accepts (`127.1`, `0x7f000001`,
/// `2130706433`), any IPv6 literal (`*.ipv6-literal.net`, which may be `::1`), or one of
/// `this_machine`'s names. The forms `\\?\UNC\` and `\\.\UNC\` are seen through.
pub fn is_unc_to_this_machine(path: &Path, this_machine: &[String]) -> bool {
    match normalise(path) {
        Some(Normal {
            root: Root::Unc { server, .. },
            ..
        }) => {
            server == "localhost"
                || server.ends_with(".ipv6-literal.net")
                || ipv4(&server).is_some_and(|a| a >> 24 == 127 || a == 0)
                || this_machine.iter().any(|n| fold(n) == server)
        }
        _ => false,
    }
}

/// An IPv4 address the way `inet_addr` reads one: one to four parts, each decimal, octal
/// (a leading `0`) or hexadecimal (`0x`), the last part filling the bytes left.
fn ipv4(text: &str) -> Option<u32> {
    let parts: Vec<&str> = text.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let mut values = Vec::with_capacity(parts.len());
    for part in &parts {
        let (digits, radix) =
            if let Some(h) = part.strip_prefix("0x").or_else(|| part.strip_prefix("0X")) {
                (h, 16)
            } else if part.len() > 1 && part.starts_with('0') {
                (&part[1..], 8)
            } else {
                (*part, 10)
            };
        if digits.is_empty() && radix != 16 {
            return None;
        }
        values.push(if digits.is_empty() {
            0
        } else {
            u64::from_str_radix(digits, radix).ok()?
        });
    }
    let (last, head) = values.split_last()?;
    let mut addr: u64 = 0;
    for (i, v) in head.iter().enumerate() {
        if *v > 0xff {
            return None;
        }
        addr |= v << (24 - 8 * i);
    }
    let room = 32 - 8 * head.len() as u32;
    if room < 64 && *last >= (1u64 << room) {
        return None;
    }
    u32::try_from(addr | last).ok()
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

/// Whether `path`, as given, is absolute and keeps no `..`: the probe resolves nothing
/// against the working folder.
pub fn is_absolute_without_parent(path: &Path) -> bool {
    normalise(path).is_some_and(|p| !p.components.iter().any(|c| c == ".."))
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

/// Whether `name` is a plain file name carrying the probe's prefix: the only names `--out`,
/// `dpapi --file` and `swap` take, so none of them can name a file the probe did not make.
pub fn is_probe_file_name(name: &str) -> bool {
    is_plain_file_name(name)
        && name.len() > crate::PROBE_PREFIX.len()
        && name
            .get(..crate::PROBE_PREFIX.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(crate::PROBE_PREFIX))
}

/// Whether `name` is a name the probe gives a kernel object it makes (a job): its prefix,
/// then lower-case letters, digits and dashes.
pub fn is_probe_object_name(name: &str) -> bool {
    name.len() > crate::PROBE_PREFIX.len()
        && name.len() <= 64
        && name.starts_with(crate::PROBE_PREFIX)
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// What the Windows side read about one forbidden root, for [`identity_refuses`]: the
/// root's own identity when it exists, its parent folder's, and the root's last name.
#[derive(Debug, Clone)]
pub struct RootIdentity<Id> {
    pub root: Option<Id>,
    pub parent: Option<Id>,
    pub name: String,
}

/// One existing ancestor of a scratch path (the path itself counts, first), by identity,
/// with the names of the path's components below it.
#[derive(Debug, Clone)]
pub struct Ancestor<Id> {
    pub id: Id,
    pub below: Vec<String>,
}

/// Whether an existing ancestor of the scratch path is a forbidden root, or is a forbidden
/// root's parent folder with the root's name next below it, compared by identity rather
/// than by spelling. So `P:\x` on a drive mapped to the profile's `.claude`, or
/// `\\host\share\.claude` on a share of the profile, is refused even when `.claude` does
/// not exist yet.
pub fn identity_refuses<Id: PartialEq>(
    ancestors: &[Ancestor<Id>],
    roots: &[RootIdentity<Id>],
) -> bool {
    ancestors.iter().any(|a| {
        roots.iter().any(|r| {
            r.root.as_ref() == Some(&a.id)
                || (r.parent.as_ref() == Some(&a.id)
                    && a.below.first().is_some_and(|c| fold(c) == fold(&r.name)))
        })
    })
}

/// A path's root: a drive, or a UNC server and share, each folded.
#[derive(Debug, PartialEq, Eq)]
enum Root {
    Drive(String),
    Unc { server: String, share: String },
}

/// A path as Windows compares it: its root and its components, each folded to lower case.
#[derive(Debug, PartialEq, Eq)]
struct Normal {
    root: Root,
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
/// `None` for a path that is not absolute (relative, drive-relative `C:x`, or rooted on the
/// current drive `\x`), for a root that is neither a drive nor a UNC share (`\\?\Volume{…}`,
/// `\\?\GLOBALROOT`, `\\.\pipe`), and for a path with a `:` after its root, which names a
/// stream (`.claude::$INDEX_ALLOCATION` is the folder `.claude` itself).
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
        if server.contains(':') || share.contains(':') {
            return None;
        }
        Root::Unc { server, share }
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
        Root::Drive(drive)
    };
    let components: Vec<String> = parts.collect();
    if components.iter().any(|c| c.contains(':')) {
        return None;
    }
    Some(Normal { root, components })
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
            profiles: vec![PathBuf::from(r"C:\Users\dana")],
            local_app_data: vec![PathBuf::from(r"C:\Users\dana\AppData\Local")],
        }
    }

    fn safe(p: &str) -> bool {
        scratch_is_safe(Path::new(p), &profile().forbidden_roots())
    }

    #[test]
    fn a_scratch_under_temp_is_safe_even_though_temp_is_in_the_profile() {
        assert!(safe(r"C:\Users\dana\AppData\Local\Temp\pitboard-probe-1"));
        assert!(safe(r"D:\a\_temp\pitboard-probe-scratch"));
        assert!(safe(r"\\fileserver\pbprobe\probe"));
        assert!(safe(r"P:\pitboard-probe"));
    }

    #[test]
    fn a_scratch_inside_a_real_login_folder_is_refused() {
        for p in [
            r"C:\Users\dana\.claude",
            r"C:\Users\dana\.claude\x",
            r"C:\Users\dana\.claude.json",
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
        assert!(!safe(r"C:\USERS\DANA\.Claude.JSON"));
    }

    #[test]
    fn the_comparison_folds_non_ascii_case() {
        let roots = RealProfile {
            profiles: vec![PathBuf::from(r"C:\Users\Đạt Thử")],
            local_app_data: vec![PathBuf::from(r"C:\Users\Đạt Thử\AppData\Local")],
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
    fn roots_that_are_not_a_drive_or_a_share_are_refused() {
        for p in [
            r"\\?\Volume{0b6d4b4c-0000-0000-0000-100000000000}\Users\dana\x",
            r"\\?\GLOBALROOT\Device\HarddiskVolume3\Users\dana\.claude",
            r"\\.\pipe\pitboard-probe",
            r"\??\UNC\server\share\x",
        ] {
            assert!(!safe(p), "{p} should be refused");
        }
    }

    /// The second review's spellings: the admin share through loopback names, and the
    /// folder's own index stream.
    #[test]
    fn administrative_shares_and_streams_are_refused() {
        for p in [
            r"\\localhost\C$\Users\dana\.claude\x",
            r"\\127.0.0.1\c$\Users\dana\.codex",
            r"\\?\UNC\localhost\C$\Users\dana\.claude",
            r"\\.\UNC\LOCALHOST\c$\Users\dana\.claude.json",
            r"C:\Users\dana\.claude::$INDEX_ALLOCATION\x",
            r"C:\Users\dana\.codex:stream",
        ] {
            assert!(!safe(p), "{p} should be refused");
        }
        // An administrative or hidden share is refused on every server and wherever it
        // leads, since the text alone cannot tell what folder it is.
        assert!(!safe(r"\\fileserver\C$\Temp\x"));
        assert!(!safe(r"\\fileserver\hidden$\x"));
        assert!(!safe(r"\\fileserver\ADMIN$"));
        // A colon anywhere past the drive names a stream.
        assert!(!safe(r"C:\Temp\a:b\x"));
        assert!(!safe(r"\\server\sh:are\x"));
    }

    #[test]
    fn a_unc_path_to_this_machine_is_recognised_in_every_spelling() {
        let me = ["PBVM".to_string(), "pbvm.example.lan".to_string()];
        for p in [
            r"\\localhost\pbprobe\x",
            r"\\LOCALHOST.\pbprobe",
            r"\\?\UNC\localhost\pbprobe\x",
            r"\\127.0.0.1\pbprobe",
            r"\\127.1\pbprobe",
            r"\\127.255.255.254\pbprobe",
            r"\\0x7f000001\pbprobe",
            r"\\0177.0.0.1\pbprobe",
            r"\\2130706433\pbprobe",
            r"\\0.0.0.0\pbprobe",
            r"\\--1.ipv6-literal.net\pbprobe",
            r"\\0--1.ipv6-literal.net\pbprobe",
            r"\\pbvm\pbprobe",
            r"\\PBVM.example.lan\pbprobe",
        ] {
            assert!(is_unc_to_this_machine(Path::new(p), &me), "{p}");
        }
        for p in [
            r"\\fileserver\pbprobe",
            r"\\128.0.0.1\pbprobe",
            r"\\10.0.0.5\pbprobe",
            r"C:\Users\dana",
            r"P:\pbprobe",
        ] {
            assert!(!is_unc_to_this_machine(Path::new(p), &me), "{p}");
        }
    }

    #[test]
    fn ipv4_reads_every_form_inet_addr_takes() {
        assert_eq!(ipv4("127.0.0.1"), Some(0x7f00_0001));
        assert_eq!(ipv4("127.1"), Some(0x7f00_0001));
        assert_eq!(ipv4("127.0.1"), Some(0x7f00_0001));
        assert_eq!(ipv4("0x7f000001"), Some(0x7f00_0001));
        assert_eq!(ipv4("2130706433"), Some(0x7f00_0001));
        assert_eq!(ipv4("0177.0.0.01"), Some(0x7f00_0001));
        assert_eq!(ipv4("256.0.0.1"), None);
        assert_eq!(ipv4("1.2.3.4.5"), None);
        assert_eq!(ipv4("fileserver"), None);
        assert_eq!(ipv4("4294967296"), None);
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
        assert!(!is_absolute_without_parent(Path::new(r"scratch\x")));
        assert!(!is_absolute_without_parent(Path::new(r"C:\a\..\b")));
        assert!(is_absolute_without_parent(Path::new(r"C:\a\b")));
    }

    #[test]
    fn a_sibling_that_merely_shares_a_prefix_is_safe() {
        assert!(safe(r"C:\Users\dana\.claudex\x"));
        assert!(safe(r"C:\Users\dana\.claude.json.bak"));
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
    fn forbidden_roots_cover_every_profile_and_local_app_data_named() {
        let roots = RealProfile {
            profiles: vec![
                PathBuf::from(r"C:\Users\dana"),
                PathBuf::from(r"c:\users\DANA"),
                PathBuf::from(r"D:\fakehome"),
            ],
            local_app_data: vec![
                PathBuf::from(r"C:\Users\dana\AppData\Local"),
                PathBuf::from(r"E:\lad"),
            ],
        }
        .forbidden_roots();
        for p in [
            r"C:\Users\dana\.claude",
            r"C:\Users\dana\.claude.json",
            r"C:\Users\dana\.codex",
            r"C:\Users\dana\AppData\Local\Pitboard",
            r"D:\fakehome\.claude",
            r"D:\fakehome\.claude.json",
            r"D:\fakehome\AppData\Local\Pitboard",
            r"E:\lad\Pitboard",
        ] {
            assert!(
                roots.iter().any(|r| is_within(Path::new(p), r)),
                "{p} should be one of the forbidden roots"
            );
        }
        // The same profile spelled twice gives its roots once.
        assert_eq!(roots.len(), 4 + 4 + 1);
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

    #[test]
    fn only_probe_names_are_files_the_probe_may_write() {
        for ok in [
            "pitboard-probe-a1d.json",
            "PITBOARD-PROBE-x.bin",
            "pitboard-probe-new.json",
        ] {
            assert!(is_probe_file_name(ok), "{ok}");
        }
        for bad in [
            ".claude.json",
            ".credentials.json",
            "a1d.json",
            "pitboard-probe-",
            r"pitboard-probe-x\..\.claude.json",
            "pitboard-probe-x:stream",
            "x-pitboard-probe-y",
        ] {
            assert!(!is_probe_file_name(bad), "{bad}");
        }
    }

    #[test]
    fn probe_object_names_are_prefixed_and_plain() {
        assert!(is_probe_object_name("pitboard-probe-job-1234"));
        for bad in [
            "pitboard-probe-",
            "Global\\pitboard-probe-x",
            "pitboard-probe-A",
            "other-job",
        ] {
            assert!(!is_probe_object_name(bad), "{bad}");
        }
    }

    fn roots_by_id() -> Vec<RootIdentity<u32>> {
        // The profile is 10; its `.claude` exists as 11; `.codex` does not exist.
        vec![
            RootIdentity {
                root: Some(11),
                parent: Some(10),
                name: ".claude".into(),
            },
            RootIdentity {
                root: None,
                parent: Some(10),
                name: ".codex".into(),
            },
        ]
    }

    fn ancestor(id: u32, below: &[&str]) -> Ancestor<u32> {
        Ancestor {
            id,
            below: below.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn an_ancestor_that_is_a_forbidden_root_by_identity_is_refused() {
        // `P:\x`, where P: is mapped to the profile's `.claude`: P:\ has `.claude`'s id.
        assert!(identity_refuses(&[ancestor(11, &["x"])], &roots_by_id()));
        // The scratch is `.claude` itself, reached through a junction.
        assert!(identity_refuses(&[ancestor(11, &[])], &roots_by_id()));
    }

    #[test]
    fn a_missing_root_is_refused_through_its_parent_by_identity_and_name() {
        // `\\host\share\.codex\x`, the share being the profile, `.codex` not made yet.
        assert!(identity_refuses(
            &[ancestor(10, &[".codex", "x"])],
            &roots_by_id()
        ));
        // The same with a trailing dot and another case.
        assert!(identity_refuses(
            &[ancestor(10, &[".CODEX.", "x"])],
            &roots_by_id()
        ));
    }

    #[test]
    fn the_profile_itself_and_its_other_folders_pass_the_identity_check() {
        assert!(!identity_refuses(&[ancestor(10, &[])], &roots_by_id()));
        assert!(!identity_refuses(
            &[ancestor(10, &["AppData", "Local", "Temp"])],
            &roots_by_id()
        ));
        assert!(!identity_refuses(
            &[ancestor(12, &["x"]), ancestor(1, &["tmp", "x"])],
            &roots_by_id()
        ));
    }
}
