//! Keep the account out of what the probe prints. Every path and every string a Windows
//! item carries (a credential's user name or comment, a task's path) passes through a
//! [`Redactor`] first: each form of the profile folder (long, 8.3 short, verbatim) becomes
//! `<profile>`, and a path component that is the account's name, in either form, becomes
//! `<user>`. What a block needs to know about the name itself, such as whether it is ASCII
//! or survives NFC, it reports as facts, not as the name.

/// Replaces the profile folder and the account name in text the probe prints.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    /// Profile folders, longest first, so `C:\Users\dana` wins over `C:\Users\DANA~1` only
    /// when it is the one present.
    profiles: Vec<String>,
    /// Names that stand for the account: the profile folder's last component (long and
    /// short), and the account name Windows reports.
    names: Vec<String>,
}

impl Redactor {
    /// A redactor for these profile folder forms and these account names. Empty strings are
    /// ignored.
    pub fn new(profiles: &[String], names: &[String]) -> Self {
        let mut profiles: Vec<String> = profiles
            .iter()
            .map(|p| p.trim_end_matches(['\\', '/']).to_string())
            .filter(|p| !p.is_empty())
            .collect();
        profiles.sort_by_key(|p| std::cmp::Reverse(p.chars().count()));
        profiles.dedup();
        let mut names: Vec<String> = names.iter().filter(|n| !n.is_empty()).cloned().collect();
        names.dedup();
        Redactor { profiles, names }
    }

    /// `text` with every profile form replaced by `<profile>` and every path component that
    /// is an account name replaced by `<user>`. A profile form matches only where it ends at
    /// a separator or the end, so `C:\Users\dana` is not found inside `C:\Users\danaX`.
    pub fn redact(&self, text: &str) -> String {
        let mut out = text.to_string();
        for profile in &self.profiles {
            out = replace_path_prefixes(&out, profile, "<profile>");
        }
        if self.names.is_empty() {
            return out;
        }
        // Component by component, keeping the separators as they were.
        let mut result = String::with_capacity(out.len());
        let mut component = String::new();
        for c in out.chars() {
            if c == '\\' || c == '/' {
                result.push_str(&self.name_or(&component));
                component.clear();
                result.push(c);
            } else {
                component.push(c);
            }
        }
        result.push_str(&self.name_or(&component));
        result
    }

    fn name_or(&self, component: &str) -> String {
        if self.names.iter().any(|n| eq_fold(n, component)) {
            "<user>".to_string()
        } else {
            component.to_string()
        }
    }
}

/// Whether two strings are equal ignoring case across Unicode.
fn eq_fold(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// Replace each occurrence of `needle` in `hay`, ignoring case and treating `/` and `\` as
/// one, that ends at a separator or at the end of `hay`.
fn replace_path_prefixes(hay: &str, needle: &str, with: &str) -> String {
    let hay_chars: Vec<char> = hay.chars().collect();
    let needle_chars: Vec<char> = needle.chars().collect();
    let n = needle_chars.len();
    let mut out = String::with_capacity(hay.len());
    let mut i = 0;
    while i < hay_chars.len() {
        let fits = i + n <= hay_chars.len()
            && hay_chars[i..i + n]
                .iter()
                .zip(needle_chars.iter())
                .all(|(a, b)| same_char(*a, *b))
            && hay_chars.get(i + n).is_none_or(|c| *c == '\\' || *c == '/');
        if n > 0 && fits {
            out.push_str(with);
            i += n;
        } else {
            out.push(hay_chars[i]);
            i += 1;
        }
    }
    out
}

fn same_char(a: char, b: char) -> bool {
    let sep = |c: char| c == '\\' || c == '/';
    (sep(a) && sep(b)) || a == b || a.to_lowercase().eq(b.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dana() -> Redactor {
        Redactor::new(
            &[r"C:\Users\dana".into(), r"C:\Users\DANA~1".into()],
            &["dana".into(), "DANA~1".into()],
        )
    }

    #[test]
    fn the_profile_becomes_a_placeholder_in_every_spelling() {
        let r = dana();
        assert_eq!(r.redact(r"C:\Users\dana\.claude"), r"<profile>\.claude");
        assert_eq!(r.redact(r"c:/users/DANA/AppData"), r"<profile>/AppData");
        assert_eq!(
            r.redact(r"\\?\C:\Users\dana\.codex"),
            r"\\?\<profile>\.codex"
        );
        assert_eq!(
            r.redact(r"C:\Users\DANA~1\AppData\Local\Temp"),
            r"<profile>\AppData\Local\Temp"
        );
        assert_eq!(r.redact(r"C:\Users\dana"), "<profile>");
    }

    #[test]
    fn a_longer_folder_that_shares_the_prefix_is_not_the_profile() {
        let r = dana();
        assert_eq!(r.redact(r"C:\Users\danaX\y"), r"C:\Users\danaX\y");
    }

    #[test]
    fn the_account_name_as_a_component_becomes_a_placeholder() {
        let r = dana();
        assert_eq!(r.redact(r"D:\homes\dana\x"), r"D:\homes\<user>\x");
        assert_eq!(r.redact("dana"), "<user>");
        // Not inside another word.
        assert_eq!(r.redact(r"D:\danabase\x"), r"D:\danabase\x");
    }

    #[test]
    fn a_non_ascii_name_folds_case_too() {
        let r = Redactor::new(&[r"C:\Users\Đạt Thử".into()], &["Đạt Thử".into()]);
        assert_eq!(r.redact(r"c:\users\đạt thử\.claude"), r"<profile>\.claude");
        assert_eq!(r.redact(r"E:\ĐẠT THỬ"), r"E:\<user>");
    }

    #[test]
    fn an_empty_redactor_changes_nothing() {
        let r = Redactor::default();
        assert_eq!(r.redact(r"C:\Users\dana"), r"C:\Users\dana");
    }
}
