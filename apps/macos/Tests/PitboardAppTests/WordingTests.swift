import Foundation
import PitboardKit
import Testing

@testable import PitboardApp

private func renewed(_ outcome: String) -> Renewed {
    Renewed(label: "acc", provider: "claude", outcome: outcome)
}

/// Nothing due is the ordinary case, and it has to read as ordinary rather than as a
/// failure to do anything.
@Test func aRenewalRunSaysWhatItDid() {
    #expect(renewalNote([]) == "Nothing was due.")
    #expect(renewalNote([renewed("renewed")]) == "Renewed one.")
    #expect(renewalNote([renewed("renewed"), renewed("renewed")]) == "Renewed all 2.")
    #expect(renewalNote([renewed("renewed"), renewed("expired")]) == "Renewed 1 of 2.")
    #expect(renewalNote([renewed("expired")]) == "1 due; none could be renewed this time.")
}

/// A `pitboard` installed apart from the app is updated the way it was installed, and none
/// of those ways does it by itself. Saying it "updates on its own" read as though nothing
/// needed doing, until the app moved on and the command line refused its newer files.
@Test func aCommandLineInstalledApartSaysHowToUpdateIt() {
    #expect(updateNote(bundled: true) == "The one inside this app, so it updates with the app.")
    #expect(
        updateNote(bundled: false)
            == "Installed apart from this app, so update it the way you installed it.")
}

/// A check is shown as a shape and a colour, and said as a word. If two levels ever came to
/// look or sound the same, a broken check would read as a passing one.
@Test func everyLevelLooksAndSoundsLikeItself() {
    let levels: [Level] = [.ok, .warn, .fail]
    #expect(Set(levels.map(\.symbol)).count == levels.count)
    #expect(Set(levels.map(\.spoken)).count == levels.count)
    #expect(levels.allSatisfy { !$0.spoken.isEmpty })
}

/// VoiceOver reads the column's "30m" as thirty meters and "5h" as letters, so a limit is
/// said in words: its name as a sentence says it, what it has used, and when it resets.
@Test func aLimitIsSpokenInWordsAndNotInItsColumnsShorthand() {
    #expect(
        spokenLimit(window("five_hour", 42, length: 18_000), resettingIn: 3 * 3600)
            == "5-hour limit, 42 percent used, resets in 3 hours")
    #expect(
        spokenLimit(window("30_minute", 12, length: 1800), resettingIn: nil)
            == "30-minute limit, 12 percent used")
    #expect(
        spokenLimit(window("weekly_scoped", 98, scope: "Fable"), resettingIn: 0)
            == "weekly Fable limit, 98 percent used, resetting now",
        "a reset whose time has come is said, as the column says it")
}

/// Pitboard says everything else in English, so a span of time VoiceOver reads inside one of
/// its sentences is English too, whatever the region of the Mac: "resets in 3 Stunden"
/// reads as a mistake. A test cannot change the region of the process it runs in, so this
/// checks that the same span in German reads differently, which is what a span following a
/// German Mac's region would show, and that Pitboard's reads as English. The spans shown on
/// screen are the core's, which has no region.
@Test func aSpokenSpanOfTimeReadsTheSameInEveryRegion() {
    let wide = Duration.seconds(3 * 3600).formatted(
        .units(allowed: [.days, .hours, .minutes], width: .wide, maximumUnitCount: 2)
            .locale(Locale(identifier: "de_DE")))
    #expect(wide != "3 hours")
    #expect(
        spokenLimit(window("five_hour", 42, length: 18_000), resettingIn: 90 * 60)
            == "5-hour limit, 42 percent used, resets in 1 hour, 30 minutes")
}

/// A label as the core types it, taken apart: bare means Claude Code.
@Test func aTypedLabelIsTakenApart() {
    #expect(split("codex/work") == ("codex", "work"))
    #expect(split("work") == ("claude", "work"))
}
