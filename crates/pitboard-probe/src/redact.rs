//! Keep the account out of what the probe prints. Every path and every string a Windows
//! item carries (a credential's user name or comment, a task's path) passes through a
//! [`Redactor`] first: each form of the profile folder (long, 8.3 short, verbatim) becomes
//! `<profile>`, and a path component that is the account's name, in either form, becomes
//! `<user>`. What a block needs to know about the name itself, such as whether it is ASCII
//! or survives NFC, it reports as facts, not as the name. An address-shaped run becomes
//! `<address>` ([`mask_addresses`]), and a SID becomes `<sid>` for this account or a
//! numbered `<other_sid_N>` for any other ([`SidRedactor`]).

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

    /// `value` with every string in it, at any depth, passed through [`Redactor::redact`]
    /// and [`mask_addresses`]; keys, numbers and the shape are kept. For values copied from a
    /// file a tool wrote, whose fields the probe does not know in advance.
    pub fn redact_json(&self, value: &serde_json::Value) -> serde_json::Value {
        use serde_json::Value;
        match value {
            Value::String(s) => Value::String(mask_addresses(&self.redact(s))),
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| self.redact_json(v)).collect())
            }
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), self.redact_json(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

/// `text` with every run that holds an `@` replaced by `<address>`, a run ending at a space,
/// a separator, `|`, `#`, `:` or `=`: an address-shaped user name inside a Credential
/// Manager target or a tool's field is a sign-in's, so only its shape is printed.
pub fn mask_addresses(text: &str) -> String {
    let is_break = |c: char| c.is_whitespace() || matches!(c, '/' | '\\' | '|' | '#' | ':' | '=');
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.contains('@') {
            out.push_str("<address>");
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for c in text.chars() {
        if is_break(c) {
            flush(&mut run, &mut out);
            out.push(c);
        } else {
            run.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Replaces SIDs in text the probe prints: this account's becomes `<sid>`, and each other
/// account's becomes `<other_sid_N>`, numbered in the order they are first met, so two other
/// accounts' items stay apart without either SID being printed.
#[derive(Debug, Clone, Default)]
pub struct SidRedactor {
    own: Option<String>,
    others: Vec<String>,
}

impl SidRedactor {
    pub fn new(own: Option<&str>) -> Self {
        SidRedactor {
            own: own.map(str::to_uppercase),
            others: Vec::new(),
        }
    }

    /// `text` with every SID-shaped run (`S-1-` then dash-separated numbers) replaced.
    pub fn redact(&mut self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < chars.len() {
            match sid_at(&chars, i) {
                Some(end) => {
                    let sid: String = chars[i..end].iter().collect::<String>().to_uppercase();
                    if self.own.as_deref() == Some(sid.as_str()) {
                        out.push_str("<sid>");
                    } else {
                        let n = match self.others.iter().position(|o| *o == sid) {
                            Some(n) => n,
                            None => {
                                self.others.push(sid);
                                self.others.len() - 1
                            }
                        };
                        out.push_str(&format!("<other_sid_{}>", n + 1));
                    }
                    i = end;
                }
                None => {
                    out.push(chars[i]);
                    i += 1;
                }
            }
        }
        out
    }
}

/// The end of a SID that starts at `i` (`S-1-` and at least two more dash-separated
/// numbers, not inside a longer word), or `None`.
fn sid_at(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && chars[i - 1].is_ascii_alphanumeric() {
        return None;
    }
    let head: String = chars.get(i..i + 4)?.iter().collect();
    if !head.eq_ignore_ascii_case("s-1-") {
        return None;
    }
    let mut j = i + 4;
    let mut parts = 0;
    loop {
        let start = j;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        if j == start {
            return None;
        }
        parts += 1;
        if j + 1 < chars.len() && chars[j] == '-' && chars[j + 1].is_ascii_digit() {
            j += 1;
        } else {
            break;
        }
    }
    let ends_cleanly = chars.get(j).is_none_or(|c| !c.is_ascii_alphanumeric());
    (parts >= 2 && ends_cleanly).then_some(j)
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

    #[test]
    fn json_from_a_tool_is_redacted_at_every_depth_with_its_keys_kept() {
        let r = dana();
        let v = serde_json::json!({
            "startedBy": r"C:\Users\dana\.local\bin\claude.exe",
            "startTime": 133_000_000_000u64,
            "nested": [{ "who": "dana@example.com" }],
        });
        assert_eq!(
            r.redact_json(&v),
            serde_json::json!({
                "startedBy": r"<profile>\.local\bin\claude.exe",
                "startTime": 133_000_000_000u64,
                "nested": [{ "who": "<address>" }],
            })
        );
    }

    #[test]
    fn an_address_inside_a_target_name_is_masked() {
        assert_eq!(
            mask_addresses("Codex MCP Credentials/linear|dana@example.com#0"),
            "Codex MCP Credentials/linear|<address>#0"
        );
        assert_eq!(
            mask_addresses("Claude Code-credentials-e80beed8"),
            "Claude Code-credentials-e80beed8"
        );
    }

    #[test]
    fn sids_become_this_account_or_a_numbered_other() {
        let own = "S-1-5-21-1004336348-1177238915-682003330-1001";
        let other = "S-1-5-21-1004336348-1177238915-682003330-1002";
        let third = "S-1-5-21-1004336348-1177238915-682003330-1003";
        let mut r = SidRedactor::new(Some(own));
        assert_eq!(
            r.redact(&format!("pitboard-probe-e1-{own}")),
            "pitboard-probe-e1-<sid>"
        );
        assert_eq!(
            r.redact(&format!("pitboard-probe-e1-{other}")),
            "pitboard-probe-e1-<other_sid_1>"
        );
        assert_eq!(
            r.redact(&format!("pitboard-probe-e1-{third} and {other}")),
            "pitboard-probe-e1-<other_sid_2> and <other_sid_1>"
        );
        // Not a SID: too short, or part of a longer word.
        assert_eq!(r.redact("S-1-5"), "S-1-5");
        assert_eq!(r.redact("XS-1-5-21-1"), "XS-1-5-21-1");
        assert_eq!(r.redact("pitboard-probe-e1"), "pitboard-probe-e1");
        // Without this account's SID, every SID is another's.
        let mut none = SidRedactor::new(None);
        assert_eq!(none.redact(own), "<other_sid_1>");
    }
}
