//! The one shape every subcommand prints: a JSON object naming the block, the logon it ran
//! in, and either a `data` finding or an `error`. The owner sends it back and a CI step
//! uploads it, so the shape is stable. What goes into `data` is the blocks' business: they
//! print no blob and no token, name principals by their relation to the token (self,
//! SYSTEM, Administrators, other) rather than by SID, and pass every path through
//! [`crate::redact`], so the profile folder reads `<profile>` and the account name `<user>`.
//!
//! No object in a report holds two keys that differ only in case: PowerShell's
//! `ConvertFrom-Json`, which reads the reports in CI, refuses such an object outright. A
//! report whose data would hold one is a refusal naming the keys ([`case_collisions`]), so
//! the probe can never print one.

use crate::logon::LogonSession;
use serde_json::{Value, json};

/// A finished measurement, ready to print. A refusal carries a reason in place of data; a
/// block that ran only part way says so in its data.
#[derive(Debug, Clone)]
pub struct Report {
    block: String,
    logon: LogonSession,
    body: Result<Value, String>,
}

impl Report {
    /// A finding. Data with two keys in one object that differ only in case is a bug in
    /// the probe, and the report becomes a refusal that names them.
    pub fn ok(block: impl Into<String>, logon: LogonSession, data: Value) -> Self {
        let collisions = case_collisions(&data);
        let body = if collisions.is_empty() {
            Ok(data)
        } else {
            Err(format!(
                "the report's data holds keys that differ only in case, which PowerShell's \
                 ConvertFrom-Json refuses; a bug in the probe: {}",
                collisions.join("; ")
            ))
        };
        Report {
            block: block.into(),
            logon,
            body,
        }
    }

    /// A block that refused or could not run, with the reason in words. It still names the
    /// block and the logon, so a refusal is told from a crash.
    pub fn refused(
        block: impl Into<String>,
        logon: LogonSession,
        reason: impl Into<String>,
    ) -> Self {
        Report {
            block: block.into(),
            logon,
            body: Err(reason.into()),
        }
    }

    pub fn to_json(&self) -> Value {
        let mut obj = json!({
            "block": self.block,
            "logon": self.logon.as_str(),
            "logon_type": self.logon.raw,
            "logon_expects_user_keys": self.logon.expects_user_keys(),
            "ok": self.body.is_ok(),
        });
        let map = obj.as_object_mut().expect("a JSON object");
        match &self.body {
            Ok(data) => {
                map.insert("data".into(), data.clone());
            }
            Err(reason) => {
                map.insert("error".into(), Value::String(reason.clone()));
            }
        }
        obj
    }

    pub fn render(&self) -> String {
        serde_json::to_string_pretty(&self.to_json()).expect("JSON serialises")
    }

    /// Whether the block refused or failed, which decides the exit code.
    pub fn failed(&self) -> bool {
        self.body.is_err()
    }
}

/// Every object in `value` that holds two keys differing only in case, as
/// `<where>: <key> / <other key>`, where `<where>` is the path to the object from the top.
pub fn case_collisions(value: &Value) -> Vec<String> {
    let mut found = Vec::new();
    walk(value, "data", &mut found);
    found
}

fn walk(value: &Value, at: &str, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            let keys: Vec<&String> = map.keys().collect();
            for (i, a) in keys.iter().enumerate() {
                if let Some(b) = keys[..i]
                    .iter()
                    .find(|b| b.to_lowercase() == a.to_lowercase())
                {
                    found.push(format!("{at}: {b} / {a}"));
                }
            }
            for (k, v) in map {
                walk(v, &format!("{at}.{k}"), found);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                walk(v, &format!("{at}[{i}]"), found);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ok_report_carries_data_the_logon_and_its_type() {
        let r = Report::ok(
            "tokens",
            LogonSession::from_security_logon_type(2),
            json!({"elevated": false}),
        );
        let v = r.to_json();
        assert_eq!(v["block"], "tokens");
        assert_eq!(v["logon"], "interactive");
        assert_eq!(v["logon_type"], 2);
        assert_eq!(v["logon_expects_user_keys"], true);
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["elevated"], false);
        assert!(v.get("error").is_none());
        assert!(!r.failed());
    }

    #[test]
    fn a_refusal_carries_a_reason_and_no_data() {
        let r = Report::refused(
            "dpapi",
            LogonSession::from_security_logon_type(3),
            "no throwaway marker in this account",
        );
        let v = r.to_json();
        assert_eq!(v["ok"], false);
        assert_eq!(v["logon"], "network");
        assert_eq!(v["logon_expects_user_keys"], false);
        assert_eq!(v["error"], "no throwaway marker in this account");
        assert!(v.get("data").is_none());
        assert!(r.failed());
    }

    #[test]
    fn an_unread_logon_prints_as_unknown_with_no_type() {
        let v = Report::ok("homes", LogonSession::unknown(), json!({})).to_json();
        assert_eq!(v["logon"], "unknown");
        assert!(v["logon_type"].is_null());
        assert!(v["logon_expects_user_keys"].is_null());
    }

    #[test]
    fn it_renders_as_pretty_json() {
        let text = Report::ok("homes", LogonSession::unknown(), json!({"x": 1})).render();
        assert!(text.contains("\"block\": \"homes\""));
        assert!(text.starts_with('{'));
    }

    #[test]
    fn keys_that_differ_only_in_case_are_found_at_any_depth() {
        assert!(case_collisions(&json!({"a": 1, "b": {"c": [1, {"d": 2}]}})).is_empty());
        let found = case_collisions(&json!({
            "x": [{"var_os_PATH": "a", "var_os_Path": "b"}],
            "Start": 1,
            "start": 2,
        }));
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().any(|f| f.starts_with("data.x[0]: ")));
        assert!(found.iter().any(|f| f == "data: Start / start"));
    }

    /// The CI run read `{"unreadable_bytes": 28311}` for the job user's runner facts:
    /// `path-vars` and `env-child` printed keys that differed only in case. No report the
    /// probe prints can do that now; it refuses and names the keys instead.
    #[test]
    fn no_report_the_probe_prints_holds_keys_that_differ_only_in_case() {
        let r = Report::ok(
            "env-child",
            LogonSession::unknown(),
            json!({"spellings": [], "var_os_PATH": "a", "var_os_Path": "b"}),
        );
        assert!(r.failed());
        let v = r.to_json();
        assert!(v.get("data").is_none());
        assert!(
            v["error"]
                .as_str()
                .unwrap()
                .contains("var_os_PATH / var_os_Path")
        );
        assert!(case_collisions(&v).is_empty());
    }

    /// Every key the probe's sources spell out, in a `json!` object, an index or an insert,
    /// differs from every other one in more than case, so no block reaches the refusal above
    /// with the keys it writes itself.
    #[test]
    fn no_two_keys_in_the_probes_sources_differ_only_in_case() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut keys: Vec<(String, String)> = Vec::new();
        let mut dirs = vec![src];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).expect("the crate's sources") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).expect("a source file");
                    // Each file's tests come last; their keys are not the probe's.
                    let code = text.split("#[cfg(test)]").next().unwrap_or_default();
                    let file = path.file_name().unwrap().to_string_lossy().into_owned();
                    keys.extend(key_literals(code).into_iter().map(|k| (k, file.clone())));
                }
            }
        }
        assert!(keys.len() > 100, "the scan found the keys: {}", keys.len());
        let mut clashes = Vec::new();
        for (i, (a, fa)) in keys.iter().enumerate() {
            for (b, fb) in &keys[..i] {
                if a != b && a.to_lowercase() == b.to_lowercase() {
                    clashes.push(format!("{b} ({fb}) / {a} ({fa})"));
                }
            }
        }
        assert!(clashes.is_empty(), "{clashes:?}");
    }

    /// The identifier-shaped string literals in `text` that stand as a JSON key: followed by
    /// a `:` that is not a `::`, inside `[...]`, or first in an `insert(`.
    fn key_literals(text: &str) -> Vec<String> {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'"' {
                i += 1;
                continue;
            }
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            let is_ident =
                j > start && j < bytes.len() && bytes[j] == b'"' && !bytes[start].is_ascii_digit();
            if !is_ident {
                i += 1;
                continue;
            }
            let word = &text[start..j];
            let after = text[j + 1..].trim_start();
            let before = text[..i].trim_end();
            let is_key = (after.starts_with(':') && !after.starts_with("::"))
                || (before.ends_with('[') && after.starts_with(']'))
                || before.ends_with("insert(");
            if is_key {
                out.push(word.to_string());
            }
            i = j + 1;
        }
        out
    }

    #[test]
    fn the_key_scan_finds_keys_in_each_position() {
        let text = r#"json!({ "a_key": 1 }); v["b_key"] = x; m.insert("c_key".into(), y); "d"::x; "not one""#;
        assert_eq!(key_literals(text), ["a_key", "b_key", "c_key"]);
    }
}
