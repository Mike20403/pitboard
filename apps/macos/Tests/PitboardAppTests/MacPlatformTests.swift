import Foundation
import PitboardKit
import Testing
import UserNotifications

@testable import PitboardApp

// What the model asks of macOS through the traits it exports. How long an app is given to
// quit, what a run-out notice says and when a clock time names its day are the model's,
// tested in `pitboard-ffi`; these test what macOS is asked.

/// A running copy of an app that keeps what it is asked, and quits when asked only if it
/// would, as an app busy with work, or whose person said no, does not.
private final class StandInApp: RunningApp {
    let bundleURL: URL?
    var isTerminated = false
    let quits: Bool
    private(set) var asked: [String] = []

    init(at path: String, quits: Bool) {
        bundleURL = URL(fileURLWithPath: path)
        self.quits = quits
    }

    func terminate() -> Bool {
        asked.append("terminate")
        if quits { isTerminated = true }
        return quits
    }

    func forceTerminate() -> Bool {
        asked.append("forceTerminate")
        isTerminated = true
        return true
    }
}

/// An app is asked to quit the way Command-Q asks it, every running copy of it, and never
/// forced: one that declines is still running, and is asked again only when the model asks
/// again. Where a copy runs from is what is opened again, and opening opens that.
@Test func anAppIsAskedToQuitAndNeverForced() throws {
    let busy = StandInApp(at: "/Applications/ChatGPT.app", quits: false)
    let other = StandInApp(at: "/Users/x/Applications/ChatGPT.app", quits: true)
    let opened = Opened()
    let apps = MacAppControl(
        copies: { $0 == "com.openai.codex" ? [busy, other] : [] },
        open: { opened.keep($0) })

    #expect(try apps.running(app: "com.openai.codex") == "/Applications/ChatGPT.app")
    #expect(try apps.running(app: "com.example.none") == nil)

    try apps.requestQuit(app: "com.openai.codex")
    #expect(busy.asked == ["terminate"])
    #expect(other.asked == ["terminate"])
    #expect(try apps.running(app: "com.openai.codex") == "/Applications/ChatGPT.app")

    try apps.requestQuit(app: "com.openai.codex")
    #expect(busy.asked == ["terminate", "terminate"], "a copy that quit is not asked again")
    #expect(other.asked == ["terminate"])

    try apps.reopen(location: "/Applications/ChatGPT.app")
    #expect(opened.urls == [URL(fileURLWithPath: "/Applications/ChatGPT.app")])
}

private final class Opened: @unchecked Sendable {
    // Unchecked because every read and write holds `lock`.
    private let lock = NSLock()
    private var kept: [URL] = []
    var urls: [URL] { lock.withLock { kept } }
    func keep(_ url: URL) { lock.withLock { kept.append(url) } }
}

/// A clock time follows the person's 12 or 24 hours, which macOS keeps in their locale, and
/// names its weekday where the model asks for it; a date and time is abbreviated and short.
@Test func theClockFollowsThe12Or24HourSetting() throws {
    let utc = TimeZone(identifier: "UTC")!
    // 14:05 UTC on Wednesday 14 January 2026.
    let at: Int64 = 1_768_399_500
    let twelve = MacLocalTime(
        locale: Locale(identifier: "en_US"), calendar: Calendar(identifier: .gregorian),
        timeZone: utc)
    let twentyFour = MacLocalTime(
        locale: Locale(identifier: "en_US@hours=h23"),
        calendar: Calendar(identifier: .gregorian), timeZone: utc)

    #expect(try twelve.clock(epoch: at, withWeekday: false) == "2:05\u{202F}PM")
    #expect(try twentyFour.clock(epoch: at, withWeekday: false) == "14:05")
    #expect(try twentyFour.clock(epoch: at, withWeekday: true) == "Wed 14:05")
    #expect(try twentyFour.dateAndTime(epoch: at) == "Jan 14, 2026 at 14:05")

    #expect(try twentyFour.sameDay(first: at, second: at + 9 * 3600))
    #expect(try !twentyFour.sameDay(first: at, second: at + 10 * 3600), "past midnight in UTC")
}

/// A run-out is posted in the model's words, in the category whose Switch button asks the
/// model to switch to the account the notice names.
@Test func aRunOutIsPostedWithTheAccountItsButtonSwitchesTo() {
    let content = MacNotifications.content(
        of: RunOutNotice(
            id: "claude/work/session/1768399500", title: "work has no 5-hour limit left",
            subtitle: "Claude Code", body: "spare has 80% of its own left.",
            switchTo: "claude/spare"))
    #expect(content.title == "work has no 5-hour limit left")
    #expect(content.subtitle == "Claude Code")
    #expect(content.body == "spare has 80% of its own left.")
    #expect(content.categoryIdentifier == MacNotifications.category)
    #expect(content.userInfo["label"] as? String == "claude/spare")
}
