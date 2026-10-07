//! The one shape every subcommand prints: a JSON object naming the block, the logon it ran
//! in, and either a `data` finding or an `error`. The owner sends it back and a CI step
//! uploads it, so the shape is stable. What goes into `data` is the blocks' business: they
//! print no blob and no token, name principals by their relation to the token (self,
//! SYSTEM, Administrators, other) rather than by SID, and pass every path through
//! [`crate::redact`], so the profile folder reads `<profile>` and the account name `<user>`.

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
    pub fn ok(block: impl Into<String>, logon: LogonSession, data: Value) -> Self {
        Report {
            block: block.into(),
            logon,
            body: Ok(data),
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
}
