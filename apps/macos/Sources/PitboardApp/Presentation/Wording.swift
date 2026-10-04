import Foundation
import PitboardKit

/// Phrases the window and the settings both say, out of a view so a test can read them.
/// A sentence about time that is quietly wrong is worse than no sentence, and inside a
/// `body` there is nothing to assert against.

/// The answer to the question the whole tool exists for: how long the account you are on
/// is good for. `burning` is a limit filling rather than a limit resetting, which is the
/// difference between "about an hour left" and "whole again in an hour".
func lasting(_ seconds: Int64, burning: Bool) -> String {
    // Under a minute the units formatter says "0 min", which reads as though nothing were
    // left when the difference is seconds either way.
    guard seconds >= 60 else { return burning ? "about to run out" : "resets any moment" }
    let span = Duration.seconds(seconds)
        .formatted(
            .units(allowed: [.days, .hours, .minutes], width: .narrow).locale(english))
    return burning ? "about \(span) left at this rate" : "resets in \(span)"
}

/// What a renewal run did. Everything here is a login that was going to expire, so "nothing
/// happened" is the good answer and has to read like one.
func renewalNote(_ renewals: [Renewed]) -> String {
    let renewed = renewals.filter { $0.outcome == "renewed" }.count
    switch (renewals.count, renewed) {
    case (0, _): return "Nothing was due."
    case (_, 0): return "\(renewals.count) due; none could be renewed this time."
    case (let all, let done) where all == done:
        return done == 1 ? "Renewed one." : "Renewed all \(done)."
    case (let all, let done): return "Renewed \(done) of \(all)."
    }
}

/// How the `pitboard` a terminal runs is kept up to date: with the app when it is the one
/// inside it, and otherwise the way it was installed. No other way of installing it updates
/// it by itself, and saying it "updates on its own" read as though one did.
func updateNote(bundled: Bool) -> String {
    bundled
        ? "The one inside this app, so it updates with the app."
        : "Installed apart from this app, so update it the way you installed it."
}

/// How a person names a window in a sentence: "5-hour", "weekly", "daily", "3-hour".
///
/// From its length, which is the one thing both services agree on: Anthropic names its
/// windows and OpenAI times them, and "session" means nothing to somebody reading about a
/// Codex account. A window whose length is not known is named from its kind.
func windowName(_ window: Limit) -> String {
    guard let seconds = window.lengthSeconds, seconds > 0 else {
        switch window.kind {
        case "session", "five_hour": return "5-hour"
        case "seven_day": return "weekly"
        case let kind where kind.hasPrefix("weekly"): return "weekly"
        default: return window.kind.replacingOccurrences(of: "_", with: " ")
        }
    }
    switch seconds {
    case 7 * 86_400: return "weekly"
    case 86_400: return "daily"
    case let days where days % 86_400 == 0: return "\(days / 86_400)-day"
    case let hours where hours % 3600 == 0: return "\(hours / 3600)-hour"
    case let minutes where minutes % 60 == 0: return "\(minutes / 60)-minute"
    default: return "\(seconds)-second"
    }
}

/// The same name, short enough for the column beside a bar: "5h", "week", "day", "3h".
func windowShortName(_ window: Limit) -> String {
    let base: String
    if let seconds = window.lengthSeconds, seconds > 0 {
        switch seconds {
        case 7 * 86_400: base = "week"
        case 86_400: base = "day"
        case let days where days % 86_400 == 0: base = "\(days / 86_400)d"
        case let hours where hours % 3600 == 0: base = "\(hours / 3600)h"
        case let minutes where minutes % 60 == 0: base = "\(minutes / 60)m"
        default: base = "\(seconds)s"
        }
    } else {
        switch window.kind {
        case "session", "five_hour": base = "5h"
        case "weekly_all", "seven_day", "weekly_scoped": base = "week"
        default: base = window.kind
        }
    }
    return window.scope.map { "\(base) · \($0)" } ?? base
}

/// A limit as VoiceOver says it: "5-hour limit, 42 percent used, resets in 3 hours". The
/// column beside the bar says "5h" and "in 30m", which is read letter by letter or as a
/// unit: "m" is read as "meters".
func spokenLimit(_ window: Limit, resettingIn seconds: TimeInterval?) -> String {
    let name = window.scope.map { "\(windowName(window)) \($0)" } ?? windowName(window)
    let used = "\(name) limit, \(Int(window.percent.rounded())) percent used"
    guard let seconds, seconds > 0 else { return used }
    let span = Duration.seconds(max(60, Int64(seconds))).formatted(
        .units(allowed: [.days, .hours, .minutes], width: .wide, maximumUnitCount: 2)
            .locale(english))
    return "\(used), resets in \(span)"
}

/// What a switch means for sessions of a tool that never picks one up by itself. Said of
/// any session and not of running ones, since the core counts those itself when it can,
/// and a notice left in the panel should not claim sessions that may not exist. `from` is
/// empty when nothing was signed in before, and then there is no old account to name.
func restartNotice(program: String, from: String) -> String {
    let old = from.isEmpty ? "the account it started with" : from
    return
        "Any \(program) session started before this switch keeps using \(old) until it is "
        + "quit and started again."
}

/// The locale spans of time are written in. Pitboard says everything else in English, and a
/// sentence that switches language halfway, "about 1h 30min left", reads as a mistake.
private let english = Locale(identifier: "en_US_POSIX")

/// The tool a bare label means, as the core reads one.
let defaultProvider = "claude"

/// A name typed for a new account, with the tool it is for, as the core takes it. The
/// picker is what says which tool, so a slash typed into the name is the core's to refuse
/// rather than read as a second choice of tool.
func qualified(_ name: String, for provider: String) -> String {
    "\(provider)/\(name)"
}

/// A label as the core types it, taken apart: `codex/work` is Codex's `work`, and a bare
/// `work` is Claude Code's.
func split(_ typed: String) -> (provider: String, label: String) {
    let parts = typed.split(separator: "/", maxSplits: 1)
    guard parts.count == 2 else { return (defaultProvider, typed) }
    return (String(parts[0]), String(parts[1]))
}

/// A label as somebody types it at the command line: bare for Claude Code, which is what a
/// bare label has always meant, and with its tool for any other.
func typed(_ account: Account) -> String? {
    account.provider == defaultProvider ? account.label : account.qualified
}

/// How long until a moment, short enough for the column beside a bar: "in 3h", "in 2d 4h",
/// "in 12m". Nil once it has passed: the next reading is what says whether a limit actually
/// reset.
func resetsIn(_ seconds: TimeInterval) -> String? {
    guard seconds > 0 else { return nil }
    let hours = Int(seconds / 3600)
    if hours >= 24 { return "in \(hours / 24)d \(hours % 24)h" }
    if hours >= 1 { return "in \(hours)h \(Int(seconds / 60) % 60)m" }
    return "in \(max(1, Int(seconds / 60)))m"
}

/// A change as the activity list names it: what was done, by the verb the log keeps.
func changeVerb(_ verb: String) -> String {
    switch verb {
    case "switch": "Switch"
    case "enroll": "Enrol"
    case "forget": "Forget"
    case "rename": "Rename"
    case "renew": "Renew"
    case "abandon": "Give up on a switch"
    case "repair": "Repair"
    case "adopt": "Adopt"
    case "uninstall": "Uninstall"
    default: verb.replacingOccurrences(of: "_", with: " ").capitalizedFirst
    }
}

/// How a change ended: "Done", or what stopped it, from the code the log keeps.
func changeOutcome(_ outcome: String) -> String {
    outcome == "ok" ? "Done" : outcome.replacingOccurrences(of: "_", with: " ").capitalizedFirst
}

/// Who asked for a change.
func changeCaller(_ caller: String) -> String {
    switch caller {
    case "app": "Pitboard app"
    case "cli": "Command line"
    case "unknown": "Unknown"
    default: caller.capitalizedFirst
    }
}

/// When a change was made, from the local time the log keeps. The log's own text when it
/// does not parse, rather than nothing.
func changeDate(_ at: String) -> Date? {
    try? Date(at, strategy: .iso8601)
}
