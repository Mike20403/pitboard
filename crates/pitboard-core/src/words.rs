//! What Pitboard says in more than one place, each written once as a function of typed
//! values: its sentences, the words of its columns, and the level a limit's usage is at,
//! where the command line's colours and the app's tints change. The command line calls
//! these functions directly. The macOS app calls the ones it shows through `pitboard-ffi`,
//! which exports a free function of the same name for each, so where both say a thing they
//! say it in the same words. A thing said both in a column and in a sentence, such as a
//! limit's name, has a function for each form.
//!
//! All of it is English. Clock times are not here: the command line writes them with
//! `time::moment`, and the app in the format its Mac is set to.

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const WEEK: i64 = 7 * DAY;

/// A length of time to the precision a person reads: "6d 4h", "2h 05m", "47m", "<1m".
///
/// Minutes beside hours take two digits, so a column of them lines up as they tick. Under a
/// minute is "<1m": "0m" reads as though nothing were left when the difference is seconds
/// either way.
pub fn span(seconds: i64) -> String {
    let s = seconds.max(0);
    let (days, hours, minutes) = (s / DAY, s % DAY / HOUR, s % HOUR / MINUTE);
    match (days, hours) {
        (0, 0) if minutes == 0 => "<1m".into(),
        (0, 0) => format!("{minutes}m"),
        (0, h) => format!("{h}h {minutes:02}m"),
        (d, h) => format!("{d}d {h}h"),
    }
}

/// A limit in the column form, beside its bar: "5h", "week", "day", "3d", "30m", "45s", and
/// "week · Fable" for one scoped to a model.
///
/// By how long it runs, where that is known, and otherwise by its kind. The length is what
/// makes two tools' limits comparable: OpenAI times every window and names none, Anthropic
/// names every window and times none, and Pitboard knows the length of each either way. A
/// kind is only the fallback, for a reading remembered from before the length was kept.
pub fn limit_column(kind: &str, length_seconds: Option<i64>, scope: Option<&str>) -> String {
    let base = match length_seconds.filter(|s| *s > 0) {
        Some(WEEK) => "week".into(),
        Some(DAY) => "day".into(),
        Some(s) if s % DAY == 0 => format!("{}d", s / DAY),
        Some(s) if s % HOUR == 0 => format!("{}h", s / HOUR),
        Some(s) if s % MINUTE == 0 => format!("{}m", s / MINUTE),
        Some(s) => format!("{s}s"),
        None => match kind {
            "session" | "five_hour" => "5h".into(),
            "weekly_all" | "seven_day" | "weekly_scoped" => "week".into(),
            other => other.into(),
        },
    };
    match scope {
        Some(scope) => format!("{base} · {scope}"),
        None => base,
    }
}

/// A limit in the sentence form: "5-hour", "weekly", "daily", "3-day", "30-minute".
///
/// From its length where that is known, as [`limit_column`] is: "session" means nothing to
/// somebody reading about a Codex account. Without its scope, which the sentence places
/// itself, as in "weekly Fable 98%".
pub fn limit_name(kind: &str, length_seconds: Option<i64>) -> String {
    match length_seconds.filter(|s| *s > 0) {
        Some(WEEK) => "weekly".into(),
        Some(DAY) => "daily".into(),
        Some(s) if s % DAY == 0 => format!("{}-day", s / DAY),
        Some(s) if s % HOUR == 0 => format!("{}-hour", s / HOUR),
        Some(s) if s % MINUTE == 0 => format!("{}-minute", s / MINUTE),
        Some(s) => format!("{s}-second"),
        None => match kind {
            "session" | "five_hour" => "5-hour".into(),
            "seven_day" => "weekly".into(),
            weekly if weekly.starts_with("weekly") => "weekly".into(),
            other => other.replace('_', " "),
        },
    }
}

/// How much of a limit is used, in three steps. The command line's colours and the app's
/// tints change where these do, and the words always say the number itself, because not
/// everybody sees a colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageLevel {
    /// Under 70%.
    Plenty,
    /// From 70%.
    Low,
    /// From 90%, and past 100%.
    Out,
}

/// The step a limit is at, from the share of it used: `percent` as a reading gives it, which
/// passes 100 when a service reports more used than the limit.
pub fn usage_level(percent: f64) -> UsageLevel {
    if percent >= 90.0 {
        UsageLevel::Out
    } else if percent >= 70.0 {
        UsageLevel::Low
    } else {
        UsageLevel::Plenty
    }
}

/// When a limit resets, beside its bar: "resets in 2h 05m", and "resetting now" once that
/// moment has come. Whether it did reset is the next reading's to say, and until then the
/// figure beside it is the last one measured.
pub fn resets(at: i64, now: i64) -> String {
    if at <= now {
        "resetting now".into()
    } else {
        format!("resets in {}", span(at - now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_read_at_a_glance() {
        assert_eq!(span(-5), "<1m");
        assert_eq!(span(59), "<1m");
        assert_eq!(span(60 * 47), "47m");
        assert_eq!(span(3600 * 2 + 60 * 5), "2h 05m");
        assert_eq!(span(86_400 * 6 + 3600 * 4 + 59), "6d 4h");
    }

    /// A limit is named by how long it runs, which both services agree on, so a Codex limit
    /// reads the way a Claude Code one does, in a sentence and in a column alike.
    #[test]
    fn a_limit_is_named_for_its_length() {
        for (length, column, sentence) in [
            (18_000, "5h", "5-hour"),
            (604_800, "week", "weekly"),
            (86_400, "day", "daily"),
            (10_800, "3h", "3-hour"),
            (172_800, "2d", "2-day"),
            (5_400, "90m", "90-minute"),
            (1_800, "30m", "30-minute"),
            (45, "45s", "45-second"),
        ] {
            assert_eq!(limit_column("any", Some(length), None), column, "{length}");
            assert_eq!(limit_name("any", Some(length)), sentence, "{length}");
        }
    }

    /// A reading taken before the length was kept names its limit the way it always did,
    /// and so does one whose length makes no sense.
    #[test]
    fn a_limit_of_unknown_length_is_named_from_its_kind() {
        assert_eq!(limit_name("session", None), "5-hour");
        assert_eq!(limit_name("five_hour", Some(0)), "5-hour");
        assert_eq!(limit_name("weekly_all", None), "weekly");
        assert_eq!(limit_name("seven_day", None), "weekly");
        assert_eq!(limit_name("primary_window", None), "primary window");
        assert_eq!(limit_column("session", None, None), "5h");
        assert_eq!(limit_column("seven_day", Some(-1), None), "week");
        assert_eq!(limit_column("primary", None, None), "primary");
        assert_eq!(
            limit_column("weekly_scoped", None, Some("Fable")),
            "week · Fable"
        );
        assert_eq!(
            limit_column("weekly_scoped", Some(604_800), Some("Fable")),
            "week · Fable"
        );
    }

    /// A limit turns amber at 70% and red at 90%, and stays red past 100%. The steps are
    /// where its colour changes, so each side of each is checked.
    #[test]
    fn a_limits_level_changes_at_seventy_and_at_ninety() {
        assert_eq!(usage_level(0.0), UsageLevel::Plenty);
        assert_eq!(usage_level(69.9), UsageLevel::Plenty);
        assert_eq!(usage_level(70.0), UsageLevel::Low);
        assert_eq!(usage_level(89.9), UsageLevel::Low);
        assert_eq!(usage_level(90.0), UsageLevel::Out);
        assert_eq!(usage_level(100.0), UsageLevel::Out);
        assert_eq!(usage_level(130.0), UsageLevel::Out);
    }

    /// The column beside a bar says how long until a limit resets in the largest units that
    /// stay true, and once that moment has come, that it is resetting.
    #[test]
    fn a_reset_is_said_in_the_largest_units_that_fit() {
        const NOW: i64 = 1_789_935_000;
        let resets = |left: i64| resets(NOW + left, NOW);
        assert_eq!(resets(-5), "resetting now");
        assert_eq!(resets(0), "resetting now");
        assert_eq!(resets(1), "resets in <1m");
        assert_eq!(resets(59), "resets in <1m");
        assert_eq!(resets(125), "resets in 2m");
        assert_eq!(resets(3599), "resets in 59m");
        assert_eq!(resets(3600), "resets in 1h 00m");
        assert_eq!(resets(3600 + 5 * 60), "resets in 1h 05m");
        assert_eq!(resets(3 * 3600 + 25 * 60), "resets in 3h 25m");
        assert_eq!(resets(86_399), "resets in 23h 59m");
        assert_eq!(resets(86_400), "resets in 1d 0h");
        assert_eq!(resets(2 * 86_400 + 4 * 3600 + 59 * 60), "resets in 2d 4h");
    }
}
