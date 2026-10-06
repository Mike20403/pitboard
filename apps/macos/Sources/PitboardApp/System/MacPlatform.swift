import AppKit
import PitboardKit
// Its types are not marked Sendable, though Notification Center is meant to be asked from
// any thread, as the model asks it from a thread of its own.
@preconcurrency import UserNotifications

// What the Rust model asks of macOS, through the traits it exports: other apps, Notification
// Center and the person's clock. Each is called on a thread of the model's own, never the
// main thread, and answers at once.

// MARK: - Other apps

/// A running copy of an app, as `NSRunningApplication` has it. A protocol so a test can stand
/// in for an app: `forceTerminate` is here only so a test can see that nothing calls it.
protocol RunningApp: AnyObject {
    var bundleURL: URL? { get }
    var isTerminated: Bool { get }
    @discardableResult func terminate() -> Bool
    @discardableResult func forceTerminate() -> Bool
}

extension NSRunningApplication: RunningApp {}

/// Other apps on this Mac, by bundle id, which the model may quit and open again around a
/// switch: one that runs a tool for itself and keeps the tool's login in memory while it is
/// open, as ChatGPT does with Codex's.
///
/// An app is asked to quit the way Command-Q asks it, which lets it ask about work in
/// progress, and never forced: a forced quit loses whatever it had not saved. How long it is
/// given, and what happens when it does not quit, is the model's.
final class MacAppControl: AppControl, @unchecked Sendable {
    // Unchecked because both closures are only called, never changed, and what they reach,
    // NSRunningApplication's list and NSWorkspace, are safe from any thread.
    private let copies: (String) -> [any RunningApp]
    private let open: (URL) -> Void

    init(copies: @escaping (String) -> [any RunningApp], open: @escaping (URL) -> Void) {
        self.copies = copies
        self.open = open
    }

    /// The apps running on this Mac, as macOS's own list of them has it. NSRunningApplication
    /// says it may be used from any thread; an app is opened from the main one.
    static func live() -> MacAppControl {
        MacAppControl(
            copies: { NSRunningApplication.runningApplications(withBundleIdentifier: $0) },
            open: { url in
                DispatchQueue.main.async {
                    let configuration = NSWorkspace.OpenConfiguration()
                    // Whatever Pitboard has to say about the switch stays in front of it.
                    configuration.activates = false
                    NSWorkspace.shared.openApplication(at: url, configuration: configuration)
                }
            })
    }

    private func running(_ app: String) -> [any RunningApp] {
        copies(app).filter { !$0.isTerminated }
    }

    /// An app is a bundle, so a running copy always says where it is.
    func running(app: String) throws -> String? {
        running(app).lazy.compactMap(\.bundleURL).first?.path
    }

    func requestQuit(app: String) throws {
        for copy in running(app) { copy.terminate() }
    }

    func reopen(location: String) throws {
        open(URL(fileURLWithPath: location))
    }
}

// MARK: - Notifications

/// Notification Center, for the model's notice that an account in use has run out, with a
/// Switch button that asks the model to switch. Switching is the button's, never the
/// notification's: Pitboard does not switch accounts on its own.
final class MacNotifications: NSObject, Notifications, UNUserNotificationCenterDelegate,
    @unchecked Sendable
{
    // Unchecked because `centre` is set once, and `onSwitch` is the main actor's.
    nonisolated static let category = "limit"
    nonisolated static let action = "switch"

    /// Nil where nothing is posted. Notification Center belongs to an app bundle: asked for
    /// anywhere else, including a test bundle, it stops the process.
    private let centre: UNUserNotificationCenter?
    /// Asked to switch to the account a Switch button names, its label with its tool.
    @MainActor var onSwitch: ((String) -> Void)?

    /// Claims the delegate and the category with its Switch button, which asks nothing of
    /// anyone and has to happen at launch: a notification left in Notification Center and
    /// clicked later, including right after an update relaunches the app, is delivered the
    /// moment there is someone to deliver it to.
    init(delivering: Bool) {
        centre = delivering ? .current() : nil
        super.init()
        guard let centre else { return }
        centre.delegate = self
        centre.setNotificationCategories([
            UNNotificationCategory(
                identifier: Self.category,
                actions: [
                    UNNotificationAction(
                        identifier: Self.action, title: "Switch", options: [.foreground])
                ],
                intentIdentifiers: [])
        ])
    }

    /// What the notification says, in the model's words, and the account its button
    /// switches to.
    static func content(of notice: RunOutNotice) -> UNNotificationContent {
        let content = UNMutableNotificationContent()
        content.title = notice.title
        if let subtitle = notice.subtitle { content.subtitle = subtitle }
        content.body = notice.body
        content.categoryIdentifier = category
        content.userInfo = ["label": notice.switchTo]
        return content
    }

    /// Permission is asked for when there is finally something to say, not at launch, where
    /// a prompt arrives before the app has shown what it is for. Refused is not an error: the
    /// window says the same either way.
    func post(notice: RunOutNotice) throws {
        guard let centre else { return }
        let request = UNNotificationRequest(
            identifier: notice.id, content: Self.content(of: notice), trigger: nil)
        centre.requestAuthorization(options: [.alert]) { granted, _ in
            if granted { centre.add(request) }
        }
    }

    func userNotificationCenter(
        _ centre: UNUserNotificationCenter, didReceive response: UNNotificationResponse
    ) async {
        guard response.actionIdentifier == Self.action,
            let label = response.notification.request.content.userInfo["label"] as? String
        else { return }
        await MainActor.run { onSwitch?(label) }
    }
}

// MARK: - The person's clock

/// Clock times and dates as the person reads them: their locale, their calendar and time
/// zone, and whether they read 12 or 24 hours, which macOS keeps in the locale.
final class MacLocalTime: LocalTime {
    private let locale: Locale
    private let calendar: Calendar

    init(
        locale: Locale = .autoupdatingCurrent, calendar: Calendar = .autoupdatingCurrent,
        timeZone: TimeZone = .autoupdatingCurrent
    ) {
        self.locale = locale
        var calendar = calendar
        calendar.timeZone = timeZone
        self.calendar = calendar
    }

    private func style(date: Date.FormatStyle.DateStyle, time: Date.FormatStyle.TimeStyle)
        -> Date.FormatStyle
    {
        Date.FormatStyle(
            date: date, time: time, locale: locale, calendar: calendar,
            timeZone: calendar.timeZone)
    }

    /// "14:05" or "2:05 PM", and "Wed 14:05" with its weekday.
    func clock(epoch: Int64, withWeekday: Bool) throws -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(epoch))
        let time = date.formatted(style(date: .omitted, time: .shortened))
        guard withWeekday else { return time }
        let day = date.formatted(
            Date.FormatStyle(locale: locale, calendar: calendar, timeZone: calendar.timeZone)
                .weekday(.abbreviated))
        return "\(day) \(time)"
    }

    func sameDay(first: Int64, second: Int64) throws -> Bool {
        calendar.isDate(
            Date(timeIntervalSince1970: TimeInterval(first)),
            inSameDayAs: Date(timeIntervalSince1970: TimeInterval(second)))
    }

    /// A date abbreviated and a time short, as a list of what happened when says it.
    func dateAndTime(epoch: Int64) throws -> String {
        Date(timeIntervalSince1970: TimeInterval(epoch))
            .formatted(style(date: .abbreviated, time: .shortened))
    }
}
