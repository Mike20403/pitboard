//! The app's own preferences, which the model keeps in `app.json` in Pitboard's directory:
//! the tools somebody said "Not Now" to a second account for, whether the app has ever shown
//! anybody anything, and whether it switches Claude Code by itself, and at what share. The macOS app kept them in UserDefaults, and hands them over once,
//! as `AppLaunch::earlier_preferences`: where the file is there, it wins.

use super::EarlierPreferences;
use pitboard_core::autoswitch::Threshold;
use pitboard_core::provider::ProviderId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// What `app.json` holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Preferences {
    /// The tools, by code, somebody keeps one account of on purpose: the nudge to add a
    /// second is not shown for them.
    #[serde(default)]
    pub(crate) second_account_declined: BTreeSet<String>,
    /// Whether the app has ever shown anybody anything. An app with no Dock icon that
    /// launches straight into a menu bar item shows somebody who has just installed it
    /// nothing at all, so the first launch opens the window, once.
    #[serde(default)]
    pub(crate) has_been_seen: bool,
    /// Whether the app switches Claude Code by itself before the account in use runs out,
    /// which somebody turns on in the settings: off unless they did. Left out of the file
    /// while off, so a file nobody turned it on in reads as it always did.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) auto_switch: bool,
    /// The share of a limit it switches at, a whole percentage, where somebody chose one;
    /// read through [`Preferences::threshold`]. Any whole number reads, so one no version
    /// would write leaves the rest of the file readable, and is taken as the nearest share.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) auto_switch_at: Option<i64>,
}

impl Preferences {
    /// The preferences kept in `file`, `app.json`'s text where there is one, and whether they
    /// came from it. Where there is none, or it does not read as preferences, they are what
    /// the app read from its earlier store, once: "Not Now" said before there was a second
    /// tool, `hideSecondAccountNudge`, was said about Claude Code, the only tool there was.
    pub(crate) fn kept(
        file: Option<&str>,
        earlier: Option<&EarlierPreferences>,
    ) -> (Preferences, bool) {
        if let Some(kept) = file.and_then(|text| serde_json::from_str::<Preferences>(text).ok()) {
            return (kept, true);
        }
        let Some(earlier) = earlier else {
            return (Preferences::default(), false);
        };
        let mut declined: BTreeSet<String> =
            earlier.second_account_declined.iter().cloned().collect();
        if earlier.second_account_nudge_hidden {
            declined.insert(ProviderId::Claude.code().to_owned());
        }
        (
            Preferences {
                second_account_declined: declined,
                has_been_seen: earlier.has_been_seen,
                ..Preferences::default()
            },
            false,
        )
    }

    /// The share of a limit the app switches Claude Code at: the one chosen, as near as there
    /// can be one to what another version kept, or the default.
    pub(crate) fn threshold(&self) -> Threshold {
        self.auto_switch_at
            .map_or_else(Threshold::default, Threshold::clamped)
    }

    /// `app.json`'s text for these preferences.
    pub(crate) fn text(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn earlier(declined: &[&str], seen: bool, nudge: bool) -> EarlierPreferences {
        EarlierPreferences {
            second_account_declined: declined.iter().map(|&code| code.to_owned()).collect(),
            has_been_seen: seen,
            second_account_nudge_hidden: nudge,
        }
    }

    fn declined(preferences: &Preferences) -> Vec<&str> {
        preferences
            .second_account_declined
            .iter()
            .map(String::as_str)
            .collect()
    }

    /// Where the file is there it wins, whatever the app read from its earlier store: the
    /// earlier store is read once, to move what it held, and is out of date after that.
    #[test]
    fn the_file_wins_where_it_is_there() {
        let (kept, from_file) = Preferences::kept(
            Some(r#"{"second_account_declined":["codex"],"has_been_seen":true}"#),
            Some(&earlier(&["claude"], false, true)),
        );
        assert!(from_file);
        assert_eq!(declined(&kept), ["codex"]);
        assert!(kept.has_been_seen);
    }

    /// Without the file, what the app read from its earlier store is taken, once, and a "Not
    /// Now" said before there was a second tool is about Claude Code: AppModel.swift moved
    /// `hideSecondAccountNudge` the same way.
    #[test]
    fn without_the_file_the_earlier_store_is_taken() {
        let (kept, from_file) = Preferences::kept(None, Some(&earlier(&["codex"], true, true)));
        assert!(!from_file);
        assert_eq!(declined(&kept), ["claude", "codex"]);
        assert!(kept.has_been_seen);

        let (kept, _) = Preferences::kept(None, Some(&earlier(&[], false, false)));
        assert_eq!(kept, Preferences::default());
        assert_eq!(
            Preferences::kept(None, None),
            (Preferences::default(), false)
        );
    }

    /// A file that does not read as preferences is taken as none, and what the earlier store
    /// held in its place; a file missing a field takes that field's default.
    #[test]
    fn a_file_that_does_not_read_is_none() {
        let (kept, from_file) =
            Preferences::kept(Some("not json"), Some(&earlier(&["codex"], true, false)));
        assert!(!from_file);
        assert_eq!(declined(&kept), ["codex"]);
        let (kept, from_file) = Preferences::kept(Some("{}"), None);
        assert!(from_file);
        assert_eq!(kept, Preferences::default());
    }

    /// The file is what it holds, read back as it was written.
    #[test]
    fn the_file_reads_back_as_written() {
        let preferences = Preferences {
            second_account_declined: BTreeSet::from(["codex".to_owned()]),
            has_been_seen: true,
            ..Preferences::default()
        };
        let text = preferences.text().expect("text");
        assert_eq!(
            text,
            r#"{"second_account_declined":["codex"],"has_been_seen":true}"#
        );
        assert_eq!(Preferences::kept(Some(&text), None), (preferences, true));
    }

    /// Switching by itself is off unless somebody turned it on, at 95% unless they chose
    /// another share; what they chose reads back, and a share no version could keep reads as
    /// the nearest there can be.
    #[test]
    fn switching_by_itself_is_off_until_turned_on_and_keeps_its_share() {
        let none = Preferences::default();
        assert!(!none.auto_switch);
        assert_eq!(none.threshold().percent(), 95);

        let on = Preferences {
            auto_switch: true,
            auto_switch_at: Some(90),
            ..Preferences::default()
        };
        let text = on.text().expect("text");
        assert_eq!(
            text,
            r#"{"second_account_declined":[],"has_been_seen":false,"auto_switch":true,"auto_switch_at":90}"#
        );
        assert_eq!(Preferences::kept(Some(&text), None), (on, true));

        let (odd, _) =
            Preferences::kept(Some(r#"{"auto_switch":true,"auto_switch_at":120}"#), None);
        assert_eq!(odd.threshold().percent(), 99);
    }

    /// A share no version would keep, written by hand or by a later version, is read as the
    /// nearest there can be, and the rest of the file with it: one field out of range does
    /// not make the preferences unreadable.
    #[test]
    fn a_share_out_of_range_is_read_as_the_nearest_with_the_rest() {
        for (written, read) in [(300, 99), (-1, 50)] {
            let (kept, from_file) = Preferences::kept(
                Some(&format!(
                    r#"{{"second_account_declined":["codex"],"auto_switch":true,"auto_switch_at":{written}}}"#
                )),
                None,
            );
            assert!(from_file, "{written}");
            assert_eq!(declined(&kept), ["codex"]);
            assert!(kept.auto_switch);
            assert_eq!(kept.threshold().percent(), read);
        }
    }
}
